use std::rc::Rc;
use std::sync::Arc;

use filmlab_engine::core::Commands;
use gpui_kit::{AppContext, AsyncApp};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::app::strip;
use super::curve::Knob;
use super::edit::{self, Slider};
use super::look::{self, MEASURED, Pick, State};
use super::{Answer, args, effect, scopes};
use crate::mcp::session::{Held, Hub, NO_PHOTO};
use crate::ui::driver::Driver;

const TRIES: usize = 8;

/// How near a measure must come to its target: about the least the numbers and the eye tell apart.
pub fn tolerance(measure: &str) -> f64 {
    match measure {
        "white%" | "black%" => 0.05,
        "cast_a" | "cast_b" => 0.25,
        "chroma50" | "chroma90" => 0.5,
        _ => 0.004,
    }
}

/// What to reach: a measure of the photo (or of a region of it) at a value, by moving one control, kept within
/// `limits` (the control's own units) when they're set.
pub struct Goal {
    pub control: String,
    pub measure: String,
    pub target: f64,
    pub region: Option<[f64; 4]>,
    pub limits: Option<(f64, f64)>,
}

/// The values a measure can take, for refusing a target no edit can reach.
fn span(measure: &str) -> (f64, f64) {
    match measure {
        "white%" | "black%" => (0.0, 100.0),
        "cast_a" | "cast_b" => (-128.0, 128.0),
        "chroma50" | "chroma90" | "skin.c" => (0.0, 150.0),
        "skin.hue" => (0.0, 360.0),
        "texture" => (0.0, 100.0),
        m if m.ends_with(".a") || m.ends_with(".b") => (-128.0, 128.0),
        _ => (0.0, 1.0),
    }
}

impl Goal {
    pub fn new(control: &str, measure: &str, target: f64, region: Option<[f64; 4]>) -> Result<Goal, String> {
        let measure = measure.trim();
        if !look::MEASURES.contains(&measure) && !scopes::MEASURES.contains(&measure) {
            return Err(format!("unknown measure {measure}; measures: {}, {}", look::MEASURES.join(", "), scopes::MEASURES.join(", ")));
        }
        let (lo, hi) = span(measure);
        if !(lo..=hi).contains(&target) {
            return Err(format!("{measure} runs {lo} to {hi}, so {target} can't be reached"));
        }
        Ok(Goal { control: control.trim().to_string(), measure: measure.to_string(), target, region, limits: None })
    }
}

/// A measure as every answer reports it: a cast only where enough of the frame is near neutral to tell.
pub fn standard(m: &Value, measure: &str) -> Option<f64> {
    if measure.starts_with("cast") && !look::has_cast(m) {
        return None;
    }
    look::reading(m, measure)
}

/// The held photo's whole frame under `state`, measured.
pub async fn measured(hub: &Hub, held: &Held, driver: &Driver, cx: &mut AsyncApp, state: &State) -> Result<Value, String> {
    let shot = look::frame(hub, held, driver, cx, state, MEASURED).await?;
    Ok(cx.background_spawn(async move { look::measure(&shot) }).await)
}

/// Where a solve ended: the closest try, its settings and what the search measured there, and whether the control
/// ran out of range.
pub struct Solved {
    pub value: f64,
    pub settings: Value,
    pub reading: f64,
    pub at_limit: bool,
}

/// The slider moved in a space where its effect is near even: mireds for a kelvin temperature, as is.
struct Axis {
    kelvin: bool,
}

impl Axis {
    fn to(&self, v: f64) -> f64 {
        if self.kelvin { -1e6 / v } else { v }
    }

    fn from(&self, x: f64) -> f64 {
        if self.kelvin { -1e6 / x } else { x }
    }
}

/// Reads one measure of the held photo, of its whole frame or a region. A held meter reads a cast from the pixels its
/// first reading found near neutral, so a white balance swinging far from neutral still reads as moving; a standard
/// one reads it as every answer does, from whatever is near neutral now.
struct Meter {
    measure: String,
    region: Option<[f64; 4]>,
    neutrals: Option<Arc<Vec<usize>>>,
    standard: bool,
}

impl Meter {
    fn new(measure: &str, region: Option<[f64; 4]>) -> Meter {
        Meter { measure: measure.to_string(), region, neutrals: None, standard: false }
    }

    /// The reading, None where the sample can't give one (too little near neutral for a cast), an error when the
    /// photo couldn't be drawn.
    async fn read(&mut self, hub: &Hub, held: &Held, driver: &Driver, cx: &mut AsyncApp, state: &State) -> Result<Option<f64>, String> {
        let shot = match self.region {
            None => look::frame(hub, held, driver, cx, state, MEASURED).await?,
            Some(region) => Arc::new(look::render(held, driver, cx, MEASURED, Some(region), &Pick::Edit(&state.settings, state.film.as_ref()), true).await?),
        };
        if scopes::MEASURES.contains(&self.measure.as_str()) {
            let name = self.measure.clone();
            return Ok(cx.background_spawn(async move { scopes::reading(&shot, &name) }).await);
        }
        if self.standard || !self.measure.starts_with("cast") {
            let measured = cx.background_spawn(async move { look::measure(&shot) }).await;
            return Ok(standard(&measured, &self.measure));
        }
        let picked = self.neutrals.clone();
        let (neutrals, (a, b)) = cx
            .background_spawn(async move {
                let neutrals = picked.unwrap_or_else(|| Arc::new(look::neutrals(&shot)));
                let cast = look::cast_at(&shot, &neutrals);
                (neutrals, cast)
            })
            .await;
        if neutrals.is_empty() {
            return Ok(None);
        }
        self.neutrals = Some(neutrals);
        Ok(Some(if self.measure == "cast_a" { a } else { b }))
    }
}

/// The settings with `control` at `value`, made by LightCraft as set_controls would.
fn with_value(commands: &mut Commands, base: &Value, facts: &Value, control: &str, value: f64) -> Result<Value, String> {
    let (targets, _) = edit::targets(&Map::from_iter([(control.to_string(), json!(value))]), base, Some(facts))?;
    let out: Value = serde_json::from_str(&commands.run("develop.set", &json!({"values": targets}).to_string())?).map_err(|e| e.to_string())?;
    Ok(out["settings"].clone())
}

#[derive(Clone, Copy)]
enum Control {
    Slider(&'static edit::Spec),
    Point(Knob),
}

const POINT_PLACES: i32 = 1;

impl Control {
    fn of(id: &str) -> Result<Control, String> {
        match Knob::of(id) {
            Some(knob) => knob.map(Control::Point),
            None => edit::spec(id).map(Control::Slider).ok_or_else(|| format!("unknown control {id}; guide controls lists ids, and curve.rgb@128 is a curve point")),
        }
    }

    fn range(self, id: &str, settings: &Value, facts: &Value) -> Result<Slider, String> {
        match self {
            Control::Slider(_) => edit::slider(id, settings, Some(facts)),
            Control::Point(knob) => {
                let (min, max) = knob.reach(settings);
                Ok(Slider { value: knob.value(settings), min, max, kelvin: false })
            }
        }
    }

    fn round(self, v: f64, min: f64, max: f64) -> Option<f64> {
        match self {
            Control::Slider(spec) => edit::rounded_within(spec, v, min, max),
            Control::Point(_) => Some(look::round(v.clamp(min, max), POINT_PLACES)),
        }
    }

    fn step(self) -> f64 {
        match self {
            Control::Slider(spec) => edit::step(spec),
            Control::Point(_) => 10f64.powi(-POINT_PLACES),
        }
    }

    fn settings(self, commands: &mut Commands, base: &Value, facts: &Value, id: &str, v: f64) -> Result<Value, String> {
        match self {
            Control::Slider(_) => with_value(commands, base, facts, id, v),
            Control::Point(knob) => Ok(knob.with(base, facts, v)?.settings),
        }
    }
}


/// The photo a fit works on: the hub and hold that reach it, the edit it starts from and the photo's facts.
pub struct Fitting<'a> {
    pub hub: &'a Hub,
    pub held: &'a Held,
    pub driver: &'a Driver,
    pub base: &'a State,
    pub facts: &'a Value,
}

/// One try: where the control was (on the search's axis), how far off the target it read, the settings it made, and
/// the control's value in them.
type Try = (f64, f64, Value, f64);

/// A search along one control: its range in its own units (`min`, `max`) and on the axis it moves evenly on (`lo`,
/// `hi`), and LightCraft making each try's settings.
struct Search<'a, 'b> {
    at: &'b Fitting<'a>,
    goal: &'b Goal,
    control: Control,
    axis: Axis,
    min: f64,
    max: f64,
    lo: f64,
    hi: f64,
    commands: Commands,
}

impl Search<'_, '_> {
    /// The settings with the control at `x` on the axis, rounded as the control shows it, and that value; none when
    /// no value at the control's precision fits the range.
    fn at_x(&mut self, x: f64) -> Result<Option<(f64, Value)>, String> {
        let Some(v) = self.control.round(self.axis.from(x.clamp(self.lo, self.hi)), self.min, self.max) else { return Ok(None) };
        Ok(Some((v, self.control.settings(&mut self.commands, &self.at.base.settings, self.at.facts, &self.goal.control, v)?)))
    }

    /// At most `most` tries with `meter`, the first at `start` with its settings; stops once a try is within tolerance.
    async fn run(&mut self, cx: &mut AsyncApp, meter: &mut Meter, start: (f64, Value), most: usize) -> Result<Vec<Try>, String> {
        let tol = tolerance(&self.goal.measure);
        let mut tries: Vec<Try> = Vec::new();
        let (mut v, mut settings) = start;
        for _ in 0..most {
            let state = State { settings: settings.clone(), film: self.at.base.film.clone() };
            let r = meter.read(self.at.hub, self.at.held, self.at.driver, cx, &state).await?.ok_or_else(|| format!("too little of the frame can be read for {}", self.goal.measure))?;
            tries.push((self.axis.to(v), r - self.goal.target, settings, v));
            if (r - self.goal.target).abs() <= tol {
                break;
            }
            let next = next_try(&tries, self.lo, self.hi).filter(|next| tries.iter().all(|t| (t.0 - next).abs() > 1e-9));
            match next.map(|next| self.at_x(next)).transpose()?.flatten() {
                Some(moved) if tries.iter().all(|t| (t.3 - moved.0).abs() > 1e-12) => (v, settings) = moved,
                _ => break,
            }
        }
        Ok(tries)
    }
}

fn closest(tries: Vec<Try>) -> Option<Try> {
    tries.into_iter().min_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
}

/// Moves the goal's control on the base edit until its measure of the photo reaches the target, drawing each try,
/// and returns the closest. Once two tries fall either side of the target it narrows between them, before that it
/// follows the slope of the last two. A cast is searched on the pixels first found near neutral, which keep reading as
/// the balance swings, then refined on the standard meter the answer reports, when the frame has enough near neutral.
pub async fn solve(at: &Fitting<'_>, cx: &mut AsyncApp, goal: &Goal) -> Result<Solved, String> {
    let control = Control::of(&goal.control)?;
    let Slider { value, min, max, kelvin } = control.range(&goal.control, &at.base.settings, at.facts)?;
    let (min, max) = goal.limits.map_or((min, max), |(lo, hi)| (lo.max(min), hi.min(max)));
    let axis = Axis { kelvin };
    let (lo, hi) = (axis.to(min).min(axis.to(max)), axis.to(min).max(axis.to(max)));
    let mut commands = Commands::new();
    commands.open(&at.facts.to_string(), &at.base.settings.to_string())?;
    let mut search = Search { at, goal, control, axis, min, max, lo, hi, commands };
    let mut kept = Meter::new(&goal.measure, goal.region);
    let found = closest(search.run(cx, &mut kept, (value, at.base.settings.clone()), TRIES).await?).ok_or("nothing was tried")?;
    let (_, off, settings, value) = match goal.measure.starts_with("cast") && goal.region.is_none() {
        true => {
            let mut standard = Meter { standard: true, ..Meter::new(&goal.measure, None) };
            let start = (found.3, found.2.clone());
            search.run(cx, &mut standard, start, TRIES / 2).await.ok().and_then(closest).unwrap_or(found)
        }
        false => found,
    };
    let ends = control.round(min, min, max).zip(control.round(max, min, max));
    let half = control.step() / 2.0;
    let at_limit = ends.is_none_or(|(low, high)| value <= low + half || value >= high - half);
    Ok(Solved { value, settings, reading: off + goal.target, at_limit })
}

/// The next value to try: between the nearest two tries either side of the target, or along the slope of the last
/// two (a first step of a tenth of the range when there's one try, further along a flat stretch).
fn next_try(tries: &[Try], lo: f64, hi: f64) -> Option<f64> {
    let below = tries.iter().filter(|t| t.1 < 0.0).min_by(|a, b| a.1.abs().total_cmp(&b.1.abs()));
    let above = tries.iter().filter(|t| t.1 > 0.0).min_by(|a, b| a.1.abs().total_cmp(&b.1.abs()));
    if let (Some(a), Some(b)) = (below, above) {
        let x = a.0 - a.1 * (b.0 - a.0) / (b.1 - a.1);
        let (l, r) = (a.0.min(b.0), a.0.max(b.0));
        let margin = (r - l) * 0.05;
        return Some(if x <= l + margin || x >= r - margin { (l + r) / 2.0 } else { x });
    }
    let step = (hi - lo) * 0.1;
    let next = match tries {
        [] => return None,
        [only] => {
            if only.0 + step <= hi { only.0 + step } else { only.0 - step }
        }
        [.., a, b] => {
            let slope = (b.1 - a.1) / (b.0 - a.0);
            let last = b.0 - a.0;
            if slope.abs() < 1e-12 {
                b.0 + 2.0 * last
            } else {
                let x = b.0 - b.1 / slope;
                x.clamp(b.0 - 4.0 * last.abs().max(step), b.0 + 4.0 * last.abs().max(step))
            }
        }
    };
    Some(next.clamp(lo, hi))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Fit {
    photo: Option<String>,
    control: String,
    measure: String,
    to: Option<f64>,
    like: Option<String>,
    max_delta: Option<f64>,
    region: Option<[f64; 4]>,
    preview: Option<Value>,
    if_state: Option<String>,
    watch: Option<Vec<String>>,
}

/// What another photo measures, as every answer reports it, for a goal to match it.
async fn measure_of(hub: &Rc<Hub>, driver: &Driver, cx: &mut AsyncApp, id: &str, goal: &Goal) -> Result<f64, String> {
    let held = hub.hold(driver, cx, Some(id)).await?;
    let state = State::now(&held, driver, cx)?;
    let mut meter = Meter { standard: true, ..Meter::new(&goal.measure, goal.region) };
    meter.read(hub, &held, driver, cx, &state).await?.ok_or_else(|| format!("too little of {id} can be read for {}", goal.measure))
}

pub async fn fit(hub: &Rc<Hub>, driver: &Driver, cx: &mut AsyncApp, value: Value) -> Answer {
    let p: Fit = args(value)?;
    let control = Control::of(&p.control)?;
    let watch = effect::watching(p.watch.clone())?;
    if p.max_delta.is_some_and(|d| !d.is_finite() || d <= 0.0) {
        return Err("max_delta is how far the control may move, in its own units, above 0".into());
    }
    let mut goal = Goal::new(&p.control, &p.measure, p.to.unwrap_or(0.0), p.region)?;
    goal.target = match (p.to, p.like.as_deref()) {
        (Some(to), None) => to,
        (None, Some(like)) if Some(like) == p.photo.as_deref() => return Err("like names the photo being fitted; give another photo".into()),
        (None, Some(like)) => measure_of(hub, driver, cx, like, &goal).await?,
        _ => return Err("give the target as to (a number) or like (a photo id to match)".into()),
    };
    let held = hub.hold(driver, cx, p.photo.as_deref()).await?;
    held.expect(p.if_state.clone());
    let before = State::now(&held, driver, cx)?;
    let facts = held.read(driver, cx, |e| e.facts.cloned()).flatten().ok_or(NO_PHOTO)?;
    if let Some(d) = p.max_delta {
        let start = control.range(&p.control, &before.settings, &facts)?.value;
        goal.limits = Some((start - d, start + d));
    }
    let solved = solve(&Fitting { hub, held: &held, driver, base: &before, facts: &facts }, cx, &goal).await?;
    if solved.settings != before.settings {
        if !State::now(&held, driver, cx)?.same(&before) {
            return Err("the photo changed while the fit was worked out, so nothing was changed; fit again".into());
        }
        let target = p.like.as_deref().map_or_else(|| edit::text(goal.target), |like| format!("match {like}"));
        match control {
            Control::Slider(spec) => {
                let label = format!("{} to {} {target}", spec.label, goal.measure);
                let values = Map::from_iter([(p.control.clone(), json!(solved.value))]);
                let (targets, _) = edit::targets(&values, &before.settings, Some(&facts))?;
                held.command(driver, cx, "develop.set", json!({"values": targets}), &label, &edit::next_key("fit")).await?;
            }
            Control::Point(knob) => {
                let label = knob.with(&before.settings, &facts, solved.value)?.label;
                held.restore(driver, cx, solved.settings.clone(), before.film.clone(), &label, &edit::next_key("fit")).await?;
            }
        }
    }
    let now = State::now(&held, driver, cx)?;
    let mut meter = Meter { standard: true, ..Meter::new(&goal.measure, goal.region) };
    let mut out = strip(json!({
        "photo": p.photo.is_none().then(|| held.id(driver, cx)),
        "set": {p.control.clone(): solved.value},
        "target": p.like.is_some().then(|| look::round(goal.target, 3)),
    }));
    match meter.read(hub, &held, driver, cx, &now).await {
        Ok(checked) => {
            let at = checked.unwrap_or(solved.reading);
            let status = match (at - goal.target).abs() <= tolerance(&goal.measure) {
                true => "reached",
                false if solved.at_limit => "limit",
                false => "near",
            };
            out["status"] = json!(status);
            out[goal.measure.as_str()] = json!(look::round(at, 3));
            if checked.is_none() {
                out["basis"] = json!("the least colourful mid tones, as too little is near neutral");
            }
        }
        Err(e) => {
            out["status"] = json!("applied_unverified");
            out["code"] = json!(e);
        }
    }
    let out = effect::report(hub, &held, driver, cx, &before, out, &watch).await;
    look::attach(&held, driver, cx, out, p.preview.as_ref()).await
}
