use std::rc::Rc;

use filmlab_engine::core;
use gpui_kit::{AsyncApp, Task};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::app::strip;
use super::fit::{Fitting, Goal, measured, solve, standard, tolerance};
use super::look::{self, State};
use super::{Answer, args, edit, effect, gathered};
use crate::film::FilmSettings;
use crate::mcp::session::{Held, Hub, NO_PHOTO};
use crate::ui::driver::Driver;
use crate::ui::rail::store::LOOK_GROUPS;

/// Every group of settings a paste can carry, as LightCraft names them, and the film look.
const GROUPS: [&str; 21] = [
    "film",
    "profile",
    "treatment",
    "whiteBalance",
    "light",
    "toneCurve",
    "color",
    "colorMixer",
    "colorGrading",
    "effects",
    "vignette",
    "grain",
    "detail",
    "optics",
    "geometry",
    "crop",
    "masks",
    "spots",
    "redEye",
    "lensBlur",
    "calibration",
];

/// The groups tied to what is in one frame, which "all" leaves out.
const OWN: [&str; 3] = ["masks", "spots", "redEye"];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Paste {
    from: Option<String>,
    to: Value,
    groups: Option<Vec<String>>,
    amount: Option<f64>,
    #[serde(rename = "match")]
    matching: Option<Vec<String>>,
    if_state: Option<String>,
    watch: Option<Vec<String>>,
}

fn groups_of(asked: Option<Vec<String>>) -> Result<Vec<String>, String> {
    let asked = asked.unwrap_or_else(|| vec!["look".into()]);
    let mut out: Vec<String> = Vec::new();
    for g in asked {
        let named: Vec<&str> = match g.trim() {
            "look" => LOOK_GROUPS.to_vec(),
            "all" => GROUPS.iter().copied().filter(|g| !OWN.contains(g)).collect(),
            one => vec![GROUPS.iter().copied().find(|g| g.eq_ignore_ascii_case(one)).ok_or_else(|| format!("unknown group {one}; groups: look, all, {}", GROUPS.join(", ")))?],
        };
        out.extend(named.into_iter().map(str::to_string).filter(|g| !out.contains(g)).collect::<Vec<_>>());
    }
    Ok(out)
}

/// What a paste carries to each photo: the source's groups (and its film, when the film group is in them), at an
/// amount, the source's numbers to match, and the history label.
struct Carry {
    partial: Value,
    film: Option<Option<FilmSettings>>,
    amount: f64,
    exposure: Option<f64>,
    cast: Option<(f64, f64)>,
    label: String,
    /// The history key prefix naming this transfer: the source's state, the groups and the matching, not the amount.
    transfer: String,
    if_state: Option<String>,
    watch: Vec<String>,
}

impl Carry {
    fn key(&self) -> String {
        edit::next_key(self.transfer.trim_start_matches("agent.").trim_end_matches('.'))
    }
}

/// How far a match may move a photo's exposure: further than this, the scenes differ rather than the exposures.
const MATCH_STOPS: f64 = 2.0;

fn on_target(m: &Value, goals: &[(&str, &str, f64)]) -> bool {
    goals.iter().all(|(_, measure, target)| standard(m, measure).is_some_and(|at| (at - target).abs() <= tolerance(measure)))
}

/// The state a paste starts from: the photo's own, or, when its current step is the same transfer (the same source
/// state, groups and matching) at another amount, the state before it, so the new amount replaces the old one.
fn base_of(held: &Held, driver: &Driver, cx: &mut AsyncApp, transfer: &str) -> Option<(usize, State)> {
    held.read(driver, cx, |e| {
        let step = e.history.get(e.cursor)?;
        let prior = e.history.get(e.cursor.checked_sub(1)?)?;
        step.key.starts_with(transfer).then(|| (e.cursor - 1, State { settings: prior.settings.clone(), film: prior.film.clone() }))
    })
    .flatten()
}

/// One photo's paste: the source's groups applied, then its white balance and exposure fitted to the source's when
/// asked and measured again at the end, saved as one undo step.
async fn paste_one(hub: &Rc<Hub>, driver: &Driver, cx: &mut AsyncApp, id: &str, carry: &Carry) -> Result<Value, String> {
    let held = hub.hold(driver, cx, Some(id)).await?;
    held.expect(carry.if_state.clone());
    let now = State::now(&held, driver, cx)?;
    let replacing = base_of(&held, driver, cx, &carry.transfer);
    let replaced = replacing.is_some();
    let before = replacing.as_ref().map_or_else(|| now.clone(), |(_, base)| base.clone());
    let facts = held.read(driver, cx, |e| e.facts.cloned()).flatten().ok_or(NO_PHOTO)?;
    let settings: Value = serde_json::from_str(&core::apply_profile(&before.settings.to_string(), &carry.partial.to_string(), carry.amount)?).map_err(|e| e.to_string())?;
    let film = match &carry.film {
        Some(source) => crate::film::blend(before.film.as_ref(), source.as_ref(), carry.amount),
        None => before.film.clone(),
    };
    let mut state = State { settings, film };
    let goals: Vec<(&str, &str, f64)> = [("light.exposure", "p50", carry.exposure), ("wb.temp", "cast_b", carry.cast.map(|c| c.1)), ("wb.tint", "cast_a", carry.cast.map(|c| c.0))]
        .into_iter()
        .filter_map(|(control, measure, target)| Some((control, measure, target?)))
        .collect();
    let mut notes = Vec::new();
    let exposure = edit::slider("light.exposure", &state.settings, Some(&facts))?.value;
    // why: under a film look exposure moves colour and white balance moves brightness, so the goals are fitted in
    // turn and once more if the last ones moved the first
    for round in 0..2 {
        if round > 0 && on_target(&measured(hub, &held, driver, cx, &state).await?, &goals) {
            break;
        }
        for (control, measure, target) in &goals {
            let mut goal = Goal::new(control, measure, *target, None)?;
            goal.limits = (*control == "light.exposure").then_some((exposure - MATCH_STOPS, exposure + MATCH_STOPS));
            match solve(&Fitting { hub, held: &held, driver, base: &state, facts: &facts }, cx, &goal).await {
                Ok(solved) => state.settings = solved.settings,
                Err(e) => notes.push(format!("{control}: {e}")),
            }
        }
    }
    if !State::now(&held, driver, cx)?.same(&now) {
        return Err("the photo changed while the paste was worked out, so nothing was changed; paste again".into());
    }
    if let Some((step, _)) = replacing {
        held.go_to(driver, cx, step).await?;
    }
    held.restore(driver, cx, state.settings.clone(), state.film.clone(), &carry.label, &carry.key()).await?;
    let mut matched = Map::new();
    let mut missed = Map::new();
    let checked = match goals.is_empty() {
        true => Ok(Value::Null),
        false => measured(hub, &held, driver, cx, &state).await,
    };
    if let Err(e) = &checked {
        let out = strip(json!({"photo": id, "status": "applied_unverified", "code": e}));
        return Ok(out);
    }
    if let Ok(m) = &checked
        && !goals.is_empty()
    {
        for (control, measure, target) in &goals {
            match standard(m, measure) {
                Some(at) if (at - target).abs() <= tolerance(measure) => {
                    matched.insert((*control).into(), json!(edit::slider(control, &state.settings, Some(&facts))?.value));
                }
                at => {
                    let bound = (*control == "light.exposure").then(|| [look::round(exposure - MATCH_STOPS, 2), look::round(exposure + MATCH_STOPS, 2)]);
                    missed.insert((*measure).into(), strip(json!({"at": at.map(|v| look::round(v, 3)), "target": look::round(*target, 3), "bound": bound})));
                }
            }
        }
    }
    let out = strip(json!({"photo": id, "replaced": replaced.then_some(true), "matched": matched, "missed": missed, "note": notes}));
    Ok(effect::report(hub, &held, driver, cx, &now, out, &carry.watch).await)
}

pub async fn paste(hub: &Rc<Hub>, driver: &Driver, cx: &mut AsyncApp, value: Value) -> Answer {
    let p: Paste = args(value)?;
    let mut targets: Vec<String> = match &p.to {
        Value::String(id) => vec![id.clone()],
        Value::Array(ids) => ids.iter().map(|v| v.as_str().map(str::to_string).ok_or("to is a photo id or a list of them")).collect::<Result<_, _>>()?,
        _ => return Err("to is a photo id or a list of them".into()),
    };
    let amount = p.amount.unwrap_or(100.0);
    if !(0.0..=200.0).contains(&amount) || amount == 0.0 {
        return Err("amount is above 0 and up to 200 (%)".into());
    }
    let groups = groups_of(p.groups)?;
    let wants = p.matching.unwrap_or_default();
    if let Some(bad) = wants.iter().find(|m| !["exposure", "wb"].contains(&m.as_str())) {
        return Err(format!("unknown match {bad}; exposure (median brightness) or wb (neutral cast)"));
    }
    let wants = |m: &str| wants.iter().any(|w| w == m);
    let held = hub.hold(driver, cx, p.from.as_deref()).await?;
    let source = held.id(driver, cx).ok_or(NO_PHOTO)?;
    let state = State::now(&held, driver, cx)?;
    let m = match wants("exposure") || wants("wb") {
        true => Some(measured(hub, &held, driver, cx, &state).await?),
        false => None,
    };
    drop(held);
    let exposure = m.as_ref().filter(|_| wants("exposure")).and_then(|m| standard(m, "p50"));
    let cast = match (wants("wb"), &m) {
        (true, Some(m)) => Some(standard(m, "cast_a").zip(standard(m, "cast_b")).ok_or_else(|| format!("too little of {source} is near neutral to match white balance to"))?),
        _ => None,
    };
    let mut seen = std::collections::HashSet::new();
    targets.retain(|id| *id != source && seen.insert(id.clone()));
    if targets.is_empty() {
        return Err(format!("to names no photo other than the source, {source}"));
    }
    let engine_groups: Vec<&String> = groups.iter().filter(|g| *g != "film").collect();
    let partial: Value = serde_json::from_str(&core::extract_profile(&state.settings.to_string(), &serde_json::to_string(&engine_groups).map_err(|e| e.to_string())?)?).map_err(|e| e.to_string())?;
    let transfer = {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        (state.digest(&source), &groups, wants("exposure"), wants("wb")).hash(&mut hasher);
        format!("agent.paste.{:016x}.", hasher.finish())
    };
    let film = groups.iter().any(|g| g == "film").then_some(state.film);
    let watch = effect::watching(p.watch)?;
    let carry = Rc::new(Carry { partial, film, amount: amount / 100.0, exposure, cast, label: format!("Paste from {source}"), transfer, if_state: p.if_state, watch });
    let tasks: Vec<Task<Result<Value, String>>> = targets
        .iter()
        .map(|id| {
            let (hub, driver, id, carry) = (hub.clone(), driver.clone(), id.clone(), carry.clone());
            cx.spawn(async move |cx| {
                let _turn = hub.turn().await;
                paste_one(&hub, &driver, cx, &id, &carry).await
            })
        })
        .collect();
    let mut results = Vec::new();
    let mut errors = Vec::new();
    for (id, task) in targets.iter().zip(tasks) {
        match task.await {
            Ok(out) => results.push(out),
            Err(e) => errors.push(format!("{id}: {e}")),
        }
    }
    let from = strip(json!({"photo": source, "p50": exposure, "cast": cast.map(|(a, b)| [look::round(a, 1), look::round(b, 1)])}));
    Ok(gathered(json!({"from": from}), results, errors))
}
