use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

use filmlab_engine::core;
use gpui_kit::AsyncApp;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use super::app::strip;
use super::look::State;
use super::{Answer, Output, args, effect, look};
use crate::film::{Dust, FilmSettings, GAUGES, Gauge, Halation, STOCK_DUST, Structure};
use crate::mcp::session::{Edit, Held, Hub, NO_PHOTO, control_values};
use crate::settings::Settings;
use crate::stocks::{DEFAULT_NEGATIVE, DEFAULT_PRINT, Stock, StockKind};
use crate::ui::driver::Driver;
use crate::ui::rail::store::{self, LOOK_GROUPS, PER_PHOTO_GROUPS};

const GONE: &str = "Filmlab's window is gone";

#[derive(Deserialize)]
pub struct Spec {
    id: String,
    pub label: String,
    section: String,
    min: f64,
    max: f64,
    default: f64,
    step: f64,
    decimals: i32,
}

fn specs() -> &'static [Spec] {
    static SPECS: OnceLock<Vec<Spec>> = OnceLock::new();
    SPECS.get_or_init(|| core::controls().ok().and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_default())
}

pub fn spec(id: &str) -> Option<&'static Spec> {
    specs().iter().find(|s| s.id == id)
}

// why: every agent edit is its own undo step; a shared key would let two quick calls coalesce into one
pub fn next_key(kind: &str) -> String {
    static N: AtomicU64 = AtomicU64::new(0);
    format!("agent.{kind}.{}", N.fetch_add(1, Ordering::Relaxed))
}

fn num(v: f64, places: i32) -> Value {
    let f = 10f64.powi(places);
    let r = (v * f).round() / f;
    if r == r.trunc() && r.abs() < 1e12 { json!(r as i64) } else { json!(r) }
}

fn shown(spec: &Spec, v: f64) -> Value {
    num(v, spec.decimals + 1)
}

/// `v` at the precision the control is shown at.
pub fn rounded(spec: &Spec, v: f64) -> f64 {
    shown(spec, v).as_f64().unwrap_or(v)
}

/// The smallest step the control is shown in.
pub fn step(spec: &Spec) -> f64 {
    10f64.powi(-(spec.decimals + 1))
}

/// `v` at the control's precision and inside `min..=max`, rounding toward the inside at an end; none when no value
/// at that precision lies inside.
pub fn rounded_within(spec: &Spec, v: f64, min: f64, max: f64) -> Option<f64> {
    let (r, step) = (rounded(spec, v), step(spec));
    let inside = if r > max {
        (max / step + 1e-9).floor() * step
    } else if r < min {
        (min / step - 1e-9).ceil() * step
    } else {
        r
    };
    let inside = rounded(spec, inside);
    (min - 1e-9..=max + 1e-9).contains(&inside).then_some(inside)
}

pub fn text(v: f64) -> String {
    num(v, 4).to_string()
}

fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let next = (prev + usize::from(ca != *cb)).min(row[j] + 1).min(row[j + 1] + 1);
            prev = row[j + 1];
            row[j + 1] = next;
        }
    }
    row[b.len()]
}

/// The few options nearest to what was asked for, for "did you mean" errors.
fn close<'a>(query: &str, options: impl Iterator<Item = &'a str>) -> String {
    let q = query.trim().to_lowercase();
    let mut scored: Vec<(usize, &str)> = options
        .map(|o| {
            let l = o.to_lowercase();
            let tail = l.rsplit(['/', '.']).next().unwrap_or(&l);
            let d = if l.contains(&q) { 0 } else { distance(&q, &l).min(distance(&q, tail) + 1) };
            (d, o)
        })
        .collect();
    scored.sort_by_key(|(d, o)| (*d, o.len()));
    let best = scored.first().map_or(0, |(d, _)| *d);
    scored.into_iter().take_while(|(d, _)| *d <= best + 2).take(4).map(|(_, o)| o).collect::<Vec<_>>().join(", ")
}

struct Wb {
    mode: String,
    temp: f64,
    tint: f64,
    kelvin: bool,
}

fn white_balance(facts: &Value, settings: &Value) -> Option<Wb> {
    let v: Value = serde_json::from_str(&core::white_balance(&facts.to_string(), &settings.to_string()).ok()?).ok()?;
    Some(Wb { mode: v["mode"].as_str()?.to_string(), temp: v["temp"].as_f64()?, tint: v["tint"].as_f64()?, kelvin: v["kelvin"].as_bool()? })
}

fn gauge_mm(g: Gauge) -> u32 {
    g.label().parse().unwrap_or(65)
}

/// The film look in a few fields: stocks, gauge (or the grain numbers), and halation and dust only where they
/// differ from FilmVision's stock.
fn film_summary(film: Option<&FilmSettings>) -> Value {
    let Some(f) = film.filter(|f| f.is_on()).map(FilmSettings::normalized) else { return json!("off") };
    let gauge = f.grain_gauge();
    let stock = Halation::stock(gauge.unwrap_or_default());
    let h = f.halation;
    let differs = |a: Option<f64>, b: Option<f64>| a.zip(b).is_some_and(|(a, b)| (a - b).abs() > 1e-4);
    let halation = if !h.enabled() {
        json!({"on": false})
    } else {
        json!({
            "amount": ((h.amount - stock.amount).abs() > 1e-4).then(|| num(h.amount, 3)),
            "hue": differs(h.hue, stock.hue).then(|| h.hue.map(|v| num(v, 3))),
            "sat": differs(h.sat, stock.sat).then(|| h.sat.map(|v| num(v, 3))),
            "radius": ((h.radius - stock.radius).abs() > 1e-4).then(|| num(h.radius, 2)),
            "highlights_only": (h.highlights_only != stock.highlights_only).then_some(h.highlights_only),
        })
    };
    let dust = match f.dust.unwrap_or(STOCK_DUST) {
        d if d == STOCK_DUST => json!(true),
        d if !d.on => json!(false),
        d => json!({"on": d.on, "size": num(d.size, 3), "softness": num(d.softness, 3)}),
    };
    let g = f.grain;
    let stills = g.structure == Structure::Stills;
    let grain = match gauge {
        None => {
            let mut custom = json!({"structure": g.structure, "amount": num(g.amount, 4), "size": num(g.size, 3), "softness": num(g.softness, 3), "defocus": num(g.defocus, 3)});
            if stills {
                custom["color"] = num(g.color, 3);
                custom["highlights"] = num(g.highlights, 3);
            }
            custom
        }
        Some(_) if !stills => json!({"structure": g.structure}),
        Some(_) => Value::Null,
    };
    strip(json!({
        "negative": f.negative,
        "print": f.print,
        "utility": f.utility,
        "gauge": gauge.map(gauge_mm),
        "grain": grain,
        "blur": gauge.is_none().then(|| num(f.blur, 4)),
        "halation": halation,
        "dust": dust,
    }))
}

fn crop_summary(settings: &Value, all: bool) -> Value {
    let crop = &settings["crop"];
    let angle = crop["geometry"]["angle"].as_f64().unwrap_or(0.0);
    let r = &crop["geometry"]["rect"];
    let rect = ["x0", "y0", "x1", "y1"].map(|k| r[k].as_f64().unwrap_or(if k.ends_with('0') { 0.0 } else { 1.0 }));
    let full = rect.iter().zip([0.0, 0.0, 1.0, 1.0]).all(|(v, w)| (v - w).abs() < 1e-6);
    let aspect = crop["aspect"].as_array().and_then(|a| {
        let (w, h) = (a.first()?.as_u64()?, a.get(1)?.as_u64()?);
        let g = (1..=w.min(h)).rev().find(|g| w % g == 0 && h % g == 0).unwrap_or(1);
        Some(format!("{}x{}", w / g, h / g))
    });
    let flips: Vec<&str> = [("flip_h", "h"), ("flip_v", "v")].into_iter().filter(|(k, _)| crop[*k].as_bool() == Some(true)).map(|(_, v)| v).collect();
    let orientation = settings["orientation"].as_str().filter(|o| *o != "Normal");
    if !all && full && angle.abs() < 1e-6 && aspect.is_none() && flips.is_empty() && orientation.is_none() {
        return Value::Null;
    }
    strip(json!({
        "aspect": aspect,
        "angle": (angle.abs() >= 1e-6).then(|| num(angle, 2)),
        "rect": (!full || all).then(|| rect.map(|v| num(v, 4))),
        "flip": flips,
        "orientation": orientation,
    }))
}

fn mask_summary(settings: &Value) -> Vec<Value> {
    let Some(masks) = settings["masks"].as_array() else { return Vec::new() };
    masks
        .iter()
        .map(|m| {
            let id = m["id"].as_u64().unwrap_or(0);
            let parts = m["components"].as_array().map_or(&[][..], Vec::as_slice);
            let mut kind = parts.first().and_then(|c| c["shape"]["kind"].as_str()).unwrap_or("mask").to_string();
            if parts.len() > 1 {
                kind = format!("{kind}+{}", parts.len() - 1);
            }
            let adjust: Map<String, Value> = m["adjust"]
                .as_object()
                .map(|a| a.iter().filter_map(|(k, v)| v.as_f64().filter(|v| v.abs() > 1e-6 && !(k == "amount" && (v - 100.0).abs() < 1e-6)).map(|v| (k.clone(), num(v, 3)))).collect())
                .unwrap_or_default();
            let name = m["name"].as_str().filter(|n| *n != format!("Mask {id}"));
            strip(json!({
                "id": id,
                "kind": kind,
                "name": name,
                "hidden": (m["visible"].as_bool() == Some(false)).then_some(true),
                "inverted": (m["invert"].as_bool() == Some(true)).then_some(true),
                "adjust": adjust,
            }))
        })
        .collect()
}

fn summarize(settings: &Value, before: Option<&Value>, facts: Option<&Value>, film: Option<&FilmSettings>, values: &HashMap<String, f64>, all: bool) -> Value {
    let controls: Map<String, Value> = specs()
        .iter()
        .filter(|s| !["wb.temp", "wb.tint", "crop.angle"].contains(&s.id.as_str()))
        .filter_map(|s| {
            let v = *values.get(&s.id)?;
            (all || shown(s, v) != shown(s, s.default)).then(|| (s.id.clone(), shown(s, v)))
        })
        .collect();
    let wb = facts.and_then(|f| white_balance(f, settings)).filter(|w| all || w.mode != "asShot").map(|w| json!({"mode": w.mode, "temp": num(w.temp, 0), "tint": num(w.tint, 0)}));
    let spots = settings["spots"].as_array().map_or(0, Vec::len);
    let profile = settings["profile"]["id"].as_str().filter(|id| all || before.is_none_or(|b| b["profile"]["id"].as_str() != Some(id)));
    strip(json!({
        "controls": controls,
        "curve": super::curve::summary(settings, facts, all),
        "wb": wb,
        "profile": profile,
        "treatment": (settings["treatment"].as_str() == Some("bw")).then_some("bw"),
        "film": film_summary(film),
        "crop": crop_summary(settings, all),
        "masks": mask_summary(settings),
        "spots": (spots > 0).then_some(spots),
    }))
}

/// A photo's edit, compact: non-default controls, film, crop, masks, spots (`all`: every control).
pub fn describe(e: &Edit<'_>, all: bool) -> Value {
    summarize(&e.settings, e.before.as_ref(), e.facts, e.film, &e.values(), all)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GetEdit {
    photo: Option<String>,
    all: Option<bool>,
}

pub fn get_edit(hub: &Hub, driver: &Driver, cx: &mut AsyncApp, value: Value) -> Answer {
    let p: GetEdit = args(value)?;
    let all = p.all.unwrap_or(false);
    hub.peek(driver, cx, p.photo.as_deref(), |e| describe(&e, all)).map(Output::json)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetControls {
    photo: Option<String>,
    values: Map<String, Value>,
    label: Option<String>,
    preview: Option<Value>,
    if_state: Option<String>,
    watch: Option<Vec<String>>,
}

/// What the user sees for a control, in the units set_controls takes (white balance as the inspector shows it).
fn current(values: &HashMap<String, f64>, wb: Option<&Wb>, id: &str) -> Option<f64> {
    match (id, wb) {
        ("wb.temp", Some(w)) => Some(w.temp),
        ("wb.tint", Some(w)) => Some(w.tint),
        _ => values.get(id).copied(),
    }
}

/// A history label in the panel's own words: up to two names, then a count ("Exposure, Contrast +2").
fn named(names: &[String]) -> String {
    let mut label = names.iter().take(2).cloned().collect::<Vec<_>>().join(", ");
    if names.len() > 2 {
        label.push_str(&format!(" +{}", names.len() - 2));
    }
    label
}

fn controls_label(ids: &[String]) -> String {
    named(&ids.iter().map(|id| spec(id).map_or_else(|| id.clone(), |s| s.label.clone())).collect::<Vec<_>>())
}

/// LightCraft's command names are title case ("Crop Aspect"); history labels are sentence case.
fn sentence(title: &str) -> String {
    title
        .split(' ')
        .enumerate()
        .map(|(i, w)| if i == 0 || w.chars().skip(1).any(char::is_uppercase) { w.to_string() } else { w.to_lowercase() })
        .collect::<Vec<_>>()
        .join(" ")
}

fn known(values: &Map<String, Value>) -> Result<(), String> {
    if values.is_empty() {
        return Err("values is empty; give e.g. {\"light.exposure\": 0.3}".into());
    }
    let unknown: Vec<&String> = values.keys().filter(|id| spec(id).is_none()).collect();
    if !unknown.is_empty() {
        let hints: Vec<String> = unknown.iter().map(|id| format!("{id} (close: {})", close(id, specs().iter().map(|s| s.id.as_str())))).collect();
        return Err(format!("unknown control {}; guide controls lists ids", hints.join("; ")));
    }
    Ok(())
}

/// A control's value on `settings` in the units set_controls takes, and its range in them: white balance is in kelvin
/// on raw photos and -100..100 on rendered ones.
pub struct Slider {
    pub value: f64,
    pub min: f64,
    pub max: f64,
    pub kelvin: bool,
}

pub fn slider(id: &str, settings: &Value, facts: Option<&Value>) -> Result<Slider, String> {
    let s = spec(id).ok_or_else(|| format!("unknown control {id} (close: {}); guide controls lists ids", close(id, specs().iter().map(|s| s.id.as_str()))))?;
    let wb = facts.and_then(|f| white_balance(f, settings));
    let value = current(&control_values(settings), wb.as_ref(), id).unwrap_or(s.default);
    let is_wb = id == "wb.temp" || id == "wb.tint";
    let (min, max) = if is_wb && wb.as_ref().is_some_and(|w| !w.kelvin) { (-100.0, 100.0) } else { (s.min, s.max) };
    Ok(Slider { value, min, max, kelvin: id == "wb.temp" && wb.as_ref().is_some_and(|w| w.kelvin) })
}

/// What `develop.set` takes to set controls to `wanted` on top of `settings`: each value made absolute, clamped to its
/// range and in the engine's units, with a note for each one clamped.
pub fn targets(wanted: &Map<String, Value>, settings: &Value, facts: Option<&Value>) -> Result<(Map<String, Value>, Vec<String>), String> {
    known(wanted)?;
    let facts_text = facts.map(Value::to_string).unwrap_or_default();
    let mut targets = Map::new();
    let mut clamped = Vec::new();
    for (id, raw) in wanted {
        let slider = slider(id, settings, facts)?;
        let wanted = match raw {
            Value::Number(n) => n.as_f64().unwrap_or(f64::NAN),
            Value::String(t) => {
                let t = t.trim();
                let n: f64 = t.parse().map_err(|_| format!("{id}: \"{t}\" isn't a number; use 0.4 (absolute) or \"+0.4\" / \"-10\" (relative)"))?;
                if t.starts_with(['+', '-']) { slider.value + n } else { n }
            }
            _ => f64::NAN,
        };
        if !wanted.is_finite() {
            return Err(format!("{id}: give a number or a relative string like \"+0.3\""));
        }
        let v = wanted.clamp(slider.min, slider.max);
        if (v - wanted).abs() > 1e-9 {
            clamped.push(format!("{id} to {}", text(v)));
        }
        let v = if id == "wb.temp" { core::temp_setting(&facts_text, v)? } else { v };
        targets.insert(id.clone(), json!(v));
    }
    Ok((targets, clamped))
}

/// `settings` with controls set to `values` as set_controls would set them, without saving, and a note for each value
/// clamped to its range.
pub fn with_values(settings: &Value, values: &Map<String, Value>, facts: &Value) -> Result<(Value, Vec<String>), String> {
    let (targets, clamped) = targets(values, settings, Some(facts))?;
    let mut commands = core::Commands::new();
    commands.open(&facts.to_string(), &settings.to_string())?;
    let out: Value = serde_json::from_str(&commands.run("develop.set", &json!({"values": targets}).to_string())?).map_err(|e| e.to_string())?;
    Ok((out["settings"].clone(), clamped))
}

pub async fn set_controls(hub: &Rc<Hub>, driver: &Driver, cx: &mut AsyncApp, value: Value) -> Answer {
    let p: SetControls = args(value)?;
    known(&p.values)?;
    let watch = effect::watching(p.watch.clone())?;
    let held = hub.hold(driver, cx, p.photo.as_deref()).await?;
    held.expect(p.if_state.clone());
    let before = State::now(&held, driver, cx)?;
    let facts = held.read(driver, cx, |e| e.facts.cloned()).ok_or(NO_PHOTO)?;
    let (targets, clamped) = targets(&p.values, &before.settings, facts.as_ref())?;
    let ids: Vec<String> = targets.keys().cloned().collect();
    let label = p.label.unwrap_or_else(|| controls_label(&ids));
    let key = next_key("controls");
    held.command(driver, cx, "develop.set", json!({"values": targets}), &label, &key).await?;
    let (values, wb) = held.read(driver, cx, |e| (e.values(), e.facts.and_then(|f| white_balance(f, &e.settings)))).ok_or(NO_PHOTO)?;
    let relative = |id: &String| p.values[id].as_str().is_some_and(|t| t.trim().starts_with(['+', '-']));
    let set: Map<String, Value> = ids.iter().filter(|id| relative(id)).filter_map(|id| Some((id.clone(), spec(id).map(|s| shown(s, current(&values, wb.as_ref(), id).unwrap_or(s.default)))?))).collect();
    let out = strip(json!({"photo": p.photo.is_none().then(|| held.id(driver, cx)), "set": set, "clamped": clamped}));
    let out = effect::report(hub, &held, driver, cx, &before, out, &watch).await;
    look::attach(&held, driver, cx, out, p.preview.as_ref()).await
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct HalationArgs {
    on: Option<bool>,
    amount: Option<f64>,
    hue: Option<f64>,
    sat: Option<f64>,
    radius: Option<f64>,
    highlights_only: Option<bool>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct GrainArgs {
    structure: Option<Structure>,
    amount: Option<f64>,
    size: Option<f64>,
    softness: Option<f64>,
    defocus: Option<f64>,
    color: Option<f64>,
    highlights: Option<f64>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct DustArgs {
    on: Option<bool>,
    size: Option<f64>,
    softness: Option<f64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetFilm {
    #[serde(rename = "photo")]
    _photo: Option<Value>,
    #[serde(rename = "negative")]
    _negative: Option<Value>,
    #[serde(rename = "print")]
    _print: Option<Value>,
    #[serde(rename = "utility")]
    _utility: Option<Value>,
    gauge: Option<u32>,
    halation: Option<HalationArgs>,
    grain: Option<GrainArgs>,
    blur: Option<f64>,
    dust: Option<DustArgs>,
    #[serde(rename = "preview")]
    _preview: Option<Value>,
    #[serde(rename = "if_state")]
    _if_state: Option<Value>,
    #[serde(rename = "watch")]
    _watch: Option<Value>,
    #[serde(rename = "label")]
    _label: Option<Value>,
}

fn kind_name(kind: StockKind) -> &'static str {
    match kind {
        StockKind::Negative => "negative",
        StockKind::Print => "print",
        StockKind::Utility => "utility",
    }
}

fn kind_prefix(kind: StockKind) -> &'static str {
    match kind {
        StockKind::Negative => "Negative/",
        StockKind::Print => "Print/",
        StockKind::Utility => "Utility/",
    }
}

/// A stock id from what an agent wrote: the id, the id without its kind folder, or the stock's name.
fn stock_id(stocks: &[Stock], kind: StockKind, wanted: &str) -> Result<String, String> {
    let of_kind: Vec<&Stock> = stocks.iter().filter(|s| s.kind == kind).collect();
    if of_kind.is_empty() {
        return Err(format!("no FilmVision {} stocks are installed in {}", kind_name(kind), crate::stocks::folder().display()));
    }
    let w = wanted.trim().to_lowercase();
    let short = |s: &Stock| s.id[kind_prefix(kind).len().min(s.id.len())..].to_lowercase();
    if let Some(s) = of_kind.iter().find(|s| s.id.to_lowercase() == w || short(s) == w) {
        return Ok(s.id.clone());
    }
    let named: Vec<&&Stock> = of_kind.iter().filter(|s| [s.name.clone(), s.label(), s.id.rsplit('/').next().unwrap_or_default().to_string()].iter().any(|n| n.to_lowercase() == w)).collect();
    match named.as_slice() {
        [one] => Ok(one.id.clone()),
        [] => Err(format!("unknown {} stock \"{wanted}\"; close: {}. guide film lists ids", kind_name(kind), close(wanted, of_kind.iter().map(|s| s.id.as_str())))),
        many => Err(format!("\"{wanted}\" names several {} stocks: {}; give the id", kind_name(kind), many.iter().map(|s| s.id.as_str()).collect::<Vec<_>>().join(", "))),
    }
}

/// Reads a stock field: absent (unchanged), null (none) or a stock.
fn stock_field(raw: Option<&Value>, stocks: &[Stock], kind: StockKind) -> Result<Option<Option<String>>, String> {
    match raw {
        None => Ok(None),
        Some(Value::Null) => Ok(Some(None)),
        Some(Value::String(s)) if s.trim().is_empty() || s.eq_ignore_ascii_case("none") => Ok(Some(None)),
        Some(Value::String(s)) => stock_id(stocks, kind, s).map(|id| Some(Some(id))),
        Some(_) => Err(format!("{} must be a stock id or null", kind_name(kind))),
    }
}

/// A film look worked out from set_film's fields on top of `film`, without saving it.
pub struct FilmChange {
    pub film: FilmSettings,
    pub label: String,
    pub clamped: Vec<String>,
    pub off: bool,
    /// The stocks were chosen for the agent, so the answer names them.
    pub defaulted: bool,
}

/// A film object given inside another request (an edit's film beside its values, a trial): only film fields, so a
/// photo, guard or label there is refused rather than quietly ignored.
pub fn bare_film(film: &Value) -> Result<(), String> {
    let Value::Object(fields) = film else { return Err("film is an object of film fields".into()) };
    match fields.keys().find(|k| ["photo", "preview", "if_state", "watch", "label", "values"].contains(&k.as_str())) {
        Some(key) => Err(format!("{key} goes outside film, not inside it")),
        None => Ok(()),
    }
}

pub fn film_change(value: &Value, film: Option<&FilmSettings>, stocks: &[Stock]) -> Result<FilmChange, String> {
    // why: serde reads a null as an absent field, and null here means "no stock"
    let [negative, print, utility] = ["negative", "print", "utility"].map(|k| value.get(k).cloned());
    let p: SetFilm = args(value.clone())?;
    let negative = stock_field(negative.as_ref(), stocks, StockKind::Negative)?;
    let print = stock_field(print.as_ref(), stocks, StockKind::Print)?;
    let utility = stock_field(utility.as_ref(), stocks, StockKind::Utility)?;
    let mut f = film.map(FilmSettings::normalized).unwrap_or_default();
    let mut changed: Vec<&str> = Vec::new();
    let mut clamped: Vec<String> = Vec::new();
    let mut fit = |name: &str, v: f64, min: f64, max: f64| {
        let c = if v.is_finite() { v.clamp(min, max) } else { min };
        if (c - v).abs() > 1e-9 || !v.is_finite() {
            clamped.push(format!("{name} to {}", text(c)));
        }
        c
    };
    if let Some(n) = &negative {
        f.negative = n.clone();
        changed.push("negative");
    }
    if let Some(n) = &print {
        f.print = n.clone();
        changed.push("print");
    }
    if let Some(u) = utility {
        f.utility = u;
        changed.push("utility");
    }
    let off = matches!(negative, Some(None)) || matches!(print, Some(None));
    let available = |id: &str| stocks.iter().any(|s| s.id == id);
    if !off {
        if f.negative.is_none() && available(DEFAULT_NEGATIVE) {
            f.negative = Some(DEFAULT_NEGATIVE.into());
        }
        if f.print.is_none() && available(DEFAULT_PRINT) {
            f.print = Some(DEFAULT_PRINT.into());
        }
    }
    if let Some(mm) = p.gauge {
        let g = GAUGES.into_iter().find(|g| gauge_mm(*g) == mm).ok_or("gauge must be 16, 35, 65 or 120")?;
        f.grain = g.grain();
        f.blur = g.blur();
        f.halation.radius = g.halation_radius();
        changed.push("gauge");
    }
    if let Some(h) = p.halation {
        let stock = Halation::stock(f.grain_gauge().unwrap_or_default());
        f.halation.on = Some(h.on.unwrap_or(f.halation.enabled()));
        f.halation.hue = f.halation.hue.or(stock.hue);
        f.halation.sat = f.halation.sat.or(stock.sat);
        f.halation.highlights_only = h.highlights_only.or(f.halation.highlights_only).or(stock.highlights_only);
        if let Some(v) = h.amount {
            f.halation.amount = fit("halation.amount", v, 0.0, 2.0);
        }
        if let Some(v) = h.hue {
            f.halation.hue = Some(fit("halation.hue", v, 0.0, 1.0));
        }
        if let Some(v) = h.sat {
            f.halation.sat = Some(fit("halation.sat", v, 0.0, 1.0));
        }
        if let Some(v) = h.radius {
            f.halation.radius = fit("halation.radius", v, 1.0, 10.0);
        }
        changed.push("halation");
    }
    if let Some(g) = p.grain {
        if let Some(v) = g.structure {
            f.grain.structure = v;
        }
        if f.grain.structure == Structure::Cine && (g.color.is_some() || g.highlights.is_some()) {
            return Err("grain.color and grain.highlights belong to stills grain; this film's grain structure is cine (give grain.structure \"stills\" to use them)".into());
        }
        if let Some(v) = g.amount {
            f.grain.amount = fit("grain.amount", v, 0.0, 1.0);
        }
        if let Some(v) = g.size {
            f.grain.size = fit("grain.size", v, 0.0, 4.0);
        }
        if let Some(v) = g.softness {
            f.grain.softness = fit("grain.softness", v, 0.0, 1.0);
        }
        if let Some(v) = g.defocus {
            f.grain.defocus = fit("grain.defocus", v, 0.5, 1.0);
        }
        if let Some(v) = g.color {
            f.grain.color = fit("grain.color", v, 0.0, 1.0);
        }
        if let Some(v) = g.highlights {
            f.grain.highlights = fit("grain.highlights", v, 0.0, 1.0);
        }
        changed.push("grain");
    }
    if let Some(v) = p.blur {
        f.blur = fit("blur", v, 0.0, 0.2);
        changed.push("blur");
    }
    if let Some(d) = p.dust {
        let mut dust: Dust = f.dust.unwrap_or(STOCK_DUST);
        if let Some(on) = d.on {
            dust.on = on;
        }
        if let Some(v) = d.size {
            dust.size = fit("dust.size", v, 1.2, 4.0);
        }
        if let Some(v) = d.softness {
            dust.softness = fit("dust.softness", v, 0.0, 0.5);
        }
        f.dust = Some(dust);
        changed.push("dust");
    }
    let mut parts: Vec<String> = Vec::new();
    for part in changed.iter().map(|c| match *c {
        "negative" | "print" | "utility" => "Film stock",
        "gauge" => "Film gauge",
        "halation" => "Halation",
        "grain" => "Grain",
        "blur" => "Film blur",
        "dust" => "Dust",
        _ => "Film look",
    }) {
        if !parts.iter().any(|p| p == part) {
            parts.push(part.to_string());
        }
    }
    let label = if off { "Film off".to_string() } else if parts.is_empty() { "Film look".to_string() } else { named(&parts) };
    let defaulted = negative.is_none() && print.is_none() && !film.is_some_and(FilmSettings::is_on) && f.is_on();
    Ok(FilmChange { film: f, label, clamped, off, defaulted })
}

pub async fn set_film(hub: &Rc<Hub>, driver: &Driver, cx: &mut AsyncApp, value: Value) -> Answer {
    let stocks = driver.read(cx, |a, _| a.stocks.clone()).ok_or(GONE)?;
    let photo = value["photo"].as_str().map(str::to_string);
    let preview = value.get("preview").cloned();
    let watch = effect::watching(serde_json::from_value(value["watch"].clone()).map_err(|_| "watch is a list of measures")?)?;
    film_change(&value, None, &stocks)?;
    let held = hub.hold(driver, cx, photo.as_deref()).await?;
    held.expect(value["if_state"].as_str().map(str::to_string));
    let before = State::now(&held, driver, cx)?;
    let change = film_change(&value, before.film.as_ref(), &stocks)?;
    let f = change.film;
    let label = value["label"].as_str().map_or(change.label, str::to_string);
    held.set_film(driver, cx, f.clone(), &label, &next_key("film")).await?;
    let note = (!change.off && !f.is_on()).then(|| format!("film is off: no default stocks are installed in {}", crate::stocks::folder().display()));
    let out = strip(json!({"photo": photo.is_none().then(|| held.id(driver, cx)), "film": change.defaulted.then(|| film_summary(Some(&f))), "clamped": change.clamped, "note": note}));
    let out = effect::report(hub, &held, driver, cx, &before, out, &watch).await;
    look::attach(&held, driver, cx, out, preview.as_ref()).await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Change {
    photo: Option<String>,
    values: Option<Map<String, Value>>,
    film: Option<Value>,
    curve: Option<Value>,
    label: Option<String>,
    preview: Option<Value>,
    if_state: Option<String>,
    watch: Option<Vec<String>>,
}

/// Controls, the tone curve and film changed together, as one undo step.
pub async fn change(hub: &Rc<Hub>, driver: &Driver, cx: &mut AsyncApp, value: Value) -> Answer {
    let p: Change = args(value)?;
    if let Some(values) = &p.values {
        known(values)?;
    }
    let stocks = driver.read(cx, |a, _| a.stocks.clone()).ok_or(GONE)?;
    if let Some(film) = &p.film {
        bare_film(film)?;
        film_change(film, None, &stocks)?;
    }
    if let Some(shape) = &p.curve {
        super::curve::check(shape)?;
    }
    let watch = effect::watching(p.watch.clone())?;
    let held = hub.hold(driver, cx, p.photo.as_deref()).await?;
    held.expect(p.if_state.clone());
    let before = State::now(&held, driver, cx)?;
    let facts = held.read(driver, cx, |e| e.facts.cloned()).flatten().ok_or(NO_PHOTO)?;
    let (settings, clamped) = match &p.values {
        Some(values) => with_values(&before.settings, values, &facts)?,
        None => (before.settings.clone(), Vec::new()),
    };
    let curved = p.curve.as_ref().map(|shape| super::curve::with_curve(&settings, shape, &facts)).transpose()?;
    let film = p.film.as_ref().map(|f| film_change(f, before.film.as_ref(), &stocks)).transpose()?;
    let ids: Vec<String> = p.values.iter().flat_map(|v| v.keys().cloned()).collect();
    let parts: Vec<String> = [(!ids.is_empty()).then(|| controls_label(&ids)), curved.as_ref().map(|c| c.label.clone()), film.as_ref().map(|f| f.label.clone())].into_iter().flatten().collect();
    let label = p.label.unwrap_or_else(|| named(&parts));
    let (settings, tally) = match curved {
        Some(c) => (c.settings, c.tally),
        None => (settings, super::curve::Tally::default()),
    };
    if film.is_some() || settings != before.settings {
        let next = film.as_ref().map_or_else(|| before.film.clone(), |f| Some(f.film.clone()));
        held.restore(driver, cx, settings, next, &label, &next_key("edit")).await?;
    }
    let (values, wb) = held.read(driver, cx, |e| (e.values(), e.facts.and_then(|f| white_balance(f, &e.settings)))).ok_or(NO_PHOTO)?;
    let relative = |id: &String| p.values.as_ref().is_some_and(|v| v[id].as_str().is_some_and(|t| t.trim().starts_with(['+', '-'])));
    let mut set: Map<String, Value> = ids.iter().filter(|id| relative(id)).filter_map(|id| Some((id.clone(), spec(id).map(|s| shown(s, current(&values, wb.as_ref(), id).unwrap_or(s.default)))?))).collect();
    set.extend(tally.set);
    let clamped: Vec<String> = clamped.into_iter().chain(tally.clamped).chain(film.iter().flat_map(|f| f.clamped.clone())).collect();
    let defaulted = film.as_ref().filter(|f| f.defaulted).map(|f| film_summary(Some(&f.film)));
    let out = strip(json!({"photo": p.photo.is_none().then(|| held.id(driver, cx)), "set": set, "film": defaulted, "clamped": clamped}));
    let out = effect::report(hub, &held, driver, cx, &before, out, &watch).await;
    look::attach(&held, driver, cx, out, p.preview.as_ref()).await
}

const EDITING: [&str; 8] = ["develop.", "crop.", "mask.", "spot.", "redeye.", "pointColor.", "geometry.", "curve."];
const ORIENT: [&str; 4] = ["photo.rotateLeft", "photo.rotateRight", "photo.flipHorizontal", "photo.flipVertical"];
// why: the app's LightCraft session holds only the open photo and keeps no clipboard, catalog, undo stack or saved
// curve presets of its own (history lives in Develop), so these do nothing useful or fight the app's state
const HELD: [&str; 11] = [
    "develop.copy",
    "develop.paste",
    "develop.pastePrevious",
    "develop.sync",
    "develop.autoSync",
    "develop.matchExposure",
    "develop.beginInteraction",
    "develop.endInteraction",
    "develop.cancelInteraction",
    "curve.savePreset",
    "curve.deletePreset",
];

fn editing(id: &str) -> bool {
    (EDITING.iter().any(|p| id.starts_with(p)) || ORIENT.contains(&id) || id == "profiles.list") && !HELD.contains(&id)
}

fn refuse(name: &str) -> Option<String> {
    if editing(name) {
        return None;
    }
    Some(if name.starts_with("edit.") || name.starts_with("history.") {
        "use edit action undo, redo or go_to".into()
    } else if ["photo.rate", "photo.flag", "photo.pick", "photo.reject", "photo.unflag", "photo.label", "photo.cycleFlag"].iter().any(|p| name.starts_with(p)) || name.contains("Rating") || name.contains("Flag") {
        "use photos action flags for flags, ratings and labels".into()
    } else if name.starts_with("export.") {
        "use photos action export".into()
    } else if name == "curve.savePreset" || name == "curve.deletePreset" {
        "a curve is kept as the user's profile: profiles save {name, groups: [\"toneCurve\"]} or profiles delete, only when the user asks".into()
    } else {
        format!("{name} isn't an editing command; command runs develop.*, crop.*, mask.*, spot.*, redeye.*, pointColor.*, geometry.*, curve.* and photo rotate/flip on the open photo")
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Command {
    photo: Option<String>,
    name: String,
    params: Option<Value>,
    preview: Option<Value>,
    if_state: Option<String>,
    watch: Option<Vec<String>>,
}

pub async fn command(hub: &Rc<Hub>, driver: &Driver, cx: &mut AsyncApp, value: Value) -> Answer {
    let p: Command = args(value)?;
    let name = p.name.trim().to_string();
    let catalog = driver.read(cx, |a, cx| a.develop.read(cx).command_catalog()).ok_or(GONE)?;
    if !catalog.iter().any(|(id, _)| *id == name) {
        return Err(format!("unknown command {name}; close: {}. guide schema command.<prefix> lists them", close(&name, catalog.iter().map(|(id, _)| *id).filter(|id| editing(id)))));
    }
    if let Some(why) = refuse(&name) {
        return Err(why);
    }
    let params = p.params.unwrap_or_else(|| json!({}));
    let Value::Object(fields) = &params else { return Err("params is an object".into()) };
    // why: LightCraft's commands can address other photos of its catalog by `ids`; here a command acts on the one
    // photo the call names, and the call's own guards belong outside params, so either inside is refused
    if let Some(key) = fields.keys().find(|k| ["ids", "photo", "photos", "if_state", "watch", "request_id", "retry"].contains(&k.as_str())) {
        return Err(format!("{key} isn't a command param here: name the photo with edit photo, and put guards beside action, outside args"));
    }
    let label = match super::curve::command_label(&name, &params) {
        Some(label) => label,
        None => driver.read(cx, |a, cx| a.develop.read(cx).command_label(&name)).flatten().map_or_else(|| name.clone(), sentence),
    };
    let key = next_key("command");
    let watch = effect::watching(p.watch.clone())?;
    let held = hub.hold(driver, cx, p.photo.as_deref()).await?;
    held.expect(p.if_state.clone());
    let before = State::now(&held, driver, cx)?;
    let (result, _) = held.command(driver, cx, &name, params.clone(), &label, &key).await?;
    if held.is_ui() && name.starts_with("mask.") {
        let active = if name == "mask.select" { Some(params["id"].as_u64()) } else { result.get("activeMask").map(Value::as_u64) };
        if let Some(active) = active {
            driver.act(cx, |a, cx| {
                let photo = a.develop.read(cx).photo_id.clone();
                let masks = &mut a.tools.masks;
                masks.photo_id = photo;
                masks.active = active.map(|id| id as u32);
                masks.error = None;
                cx.notify();
            });
        }
    }
    let out = strip(json!({"photo": p.photo.is_none().then(|| held.id(driver, cx)), "result": result}));
    let out = effect::report(hub, &held, driver, cx, &before, out, &watch).await;
    look::attach(&held, driver, cx, out, p.preview.as_ref()).await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Catalog {
    kind: String,
    filter: Option<String>,
}

// why: the whole command list with params runs past 3k tokens, so without a filter only names are listed
const COMMANDS_BUDGET: usize = 10_000;

pub fn catalog(driver: &Driver, cx: &mut AsyncApp, value: Value) -> Answer {
    let p: Catalog = args(value)?;
    let filter = p.filter.as_deref().map(|f| f.trim().to_lowercase()).filter(|f| !f.is_empty());
    match p.kind.as_str() {
        "controls" => {
            let mut sections: BTreeMap<&str, Vec<String>> = BTreeMap::new();
            for s in specs() {
                let keep = filter.as_ref().is_none_or(|f| s.id.to_lowercase().starts_with(f.as_str()) || s.section.to_lowercase() == *f);
                if keep {
                    let step = if (s.step - 1.0).abs() < 1e-9 { String::new() } else { format!(" step {}", text(s.step)) };
                    sections.entry(s.section.as_str()).or_default().push(format!("{} {}..{} ={}{step}", s.id, text(s.min), text(s.max), text(s.default)));
                }
            }
            if sections.is_empty() {
                let all: Vec<&str> = specs().iter().map(|s| s.section.as_str()).collect::<std::collections::BTreeSet<_>>().into_iter().collect();
                return Err(format!("no controls match \"{}\"; filter by id prefix (light.) or section: {}", filter.unwrap_or_default(), all.join(", ")));
            }
            let mut out = json!(sections);
            if filter.is_none() || filter.as_deref().is_some_and(|f| f.starts_with("wb")) {
                out["note"] = json!("wb.temp is kelvin on raw photos and -100..100 on rendered ones; values as edit takes them");
            }
            Ok(Output::json(out))
        }
        "commands" => {
            let list = driver.read(cx, |a, cx| a.develop.read(cx).command_catalog()).ok_or(GONE)?;
            let chosen: Vec<(&str, &str)> = list.into_iter().filter(|(id, _)| editing(id)).filter(|(id, _)| filter.as_ref().is_none_or(|f| id.to_lowercase().starts_with(f.as_str()))).collect();
            if chosen.is_empty() {
                return Err(format!("no editing commands start with \"{}\"; try develop, crop, mask, spot", filter.unwrap_or_default()));
            }
            let full: Map<String, Value> = chosen.iter().map(|(id, params)| ((*id).to_string(), json!(params))).collect();
            if Value::Object(full.clone()).to_string().len() <= COMMANDS_BUDGET {
                return Ok(Output::json(Value::Object(full)));
            }
            let mut groups: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
            for (id, _) in &chosen {
                let (ns, rest) = id.split_once('.').unwrap_or((id, ""));
                groups.entry(ns).or_default().push(rest);
            }
            let names: Map<String, Value> = groups.into_iter().map(|(ns, names)| (ns.to_string(), json!(names.join(" ")))).collect();
            Ok(Output::json(json!({"names": names, "hint": "filter by a prefix (mask, crop.aspect) for params"})))
        }
        "stocks" => {
            let stocks = driver.read(cx, |a, _| a.stocks.clone()).ok_or(GONE)?;
            if stocks.is_empty() {
                return Err(format!("no FilmVision stocks are installed in {}", crate::stocks::folder().display()));
            }
            let mut out = Map::new();
            for kind in [StockKind::Negative, StockKind::Print, StockKind::Utility] {
                let ids: Vec<&str> = stocks
                    .iter()
                    .filter(|s| s.kind == kind)
                    .map(|s| &s.id[kind_prefix(kind).len().min(s.id.len())..])
                    .filter(|id| filter.as_ref().is_none_or(|f| id.to_lowercase().contains(f.as_str())))
                    .collect();
                if !ids.is_empty() {
                    out.insert(kind_name(kind).into(), json!(ids));
                }
            }
            if out.is_empty() {
                return Err(format!("no stock matches \"{}\"", filter.unwrap_or_default()));
            }
            out.insert("default".into(), json!({"negative": DEFAULT_NEGATIVE, "print": DEFAULT_PRINT}));
            out.insert("note".into(), json!("edit film takes these ids with or without their Negative/, Print/, Utility/ folder"));
            Ok(Output::json(Value::Object(out)))
        }
        other => Err(format!("no catalog {other}; kinds: controls, commands, stocks")),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct History {
    photo: Option<String>,
    action: String,
    step: Option<usize>,
    if_state: Option<String>,
    watch: Option<Vec<String>>,
}

fn history_at(held: &Held, driver: &Driver, cx: &mut AsyncApp) -> Option<(usize, usize, String)> {
    held.read(driver, cx, |e| (e.cursor, e.history.len(), e.history.get(e.cursor).map(|s| s.label.clone()).unwrap_or_default()))
}

pub async fn history(hub: &Rc<Hub>, driver: &Driver, cx: &mut AsyncApp, value: Value) -> Answer {
    let p: History = args(value)?;
    let watch = effect::watching(p.watch.clone())?;
    let held = hub.hold(driver, cx, p.photo.as_deref()).await?;
    held.expect(p.if_state.clone());
    let before = State::now(&held, driver, cx)?;
    let (step, steps, _) = history_at(&held, driver, cx).ok_or(NO_PHOTO)?;
    let target = match p.action.as_str() {
        "list" => {
            let list = held
                .read(driver, cx, |e| {
                    let from = e.history.len().saturating_sub(30);
                    e.history.iter().enumerate().skip(from).map(|(i, s)| json!([i, s.label])).collect::<Vec<_>>()
                })
                .ok_or(NO_PHOTO)?;
            return Ok(Output::json(json!({"step": step, "steps": steps, "list": list})));
        }
        "undo" if step == 0 => return Err("nothing to undo: at step 0 (as opened)".into()),
        "undo" => step - 1,
        "redo" if step + 1 >= steps => return Err(format!("nothing to redo: step {step} is the latest")),
        "redo" => step + 1,
        "go_to" => {
            let to = p.step.ok_or("go_to needs step; history list gives them")?;
            if to >= steps {
                return Err(format!("no step {to}; steps are 0..{}", steps - 1));
            }
            to
        }
        other => return Err(format!("unknown action {other}; list, undo, redo, go_to")),
    };
    held.go_to(driver, cx, target).await?;
    let (step, steps, label) = history_at(&held, driver, cx).ok_or(NO_PHOTO)?;
    let out = strip(json!({"photo": p.photo.is_none().then(|| held.id(driver, cx)), "step": step, "steps": steps, "label": label}));
    Ok(Output::json(effect::report(hub, &held, driver, cx, &before, out, &watch).await))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Profiles {
    photo: Option<String>,
    action: String,
    id: Option<String>,
    name: Option<String>,
    groups: Option<Vec<String>>,
    amount: Option<f64>,
}

fn profile_list(profiles: &[store::Profile]) -> Vec<Value> {
    profiles.iter().map(|p| json!({"id": p.id, "name": p.name, "groups": p.groups})).collect()
}

/// The index of the profile an agent named: by its id exactly, or by its name (in any case), one or the other.
fn find_profile(profiles: &[store::Profile], id: Option<&str>, name: Option<&str>) -> Result<usize, String> {
    let found = match (id, name) {
        (Some(id), None) => profiles.iter().position(|p| p.id == id),
        (None, Some(name)) => profiles.iter().position(|p| p.name.eq_ignore_ascii_case(name.trim())),
        (Some(_), Some(_)) => return Err("give the profile's id or its name, not both".into()),
        (None, None) => return Err("give the profile's id or name; profiles action list gives them".into()),
    };
    found.ok_or_else(|| if profiles.is_empty() { "no profiles are saved".into() } else { format!("no profile {}; saved: {}", id.or(name).unwrap_or_default(), profiles.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join(", ")) })
}

/// Refuses the fields `action` doesn't read, so a profile call never quietly does less than it was asked.
fn profile_fields(p: &Profiles) -> Result<(), String> {
    let given = [("photo", p.photo.is_some()), ("id", p.id.is_some()), ("name", p.name.is_some()), ("groups", p.groups.is_some()), ("amount", p.amount.is_some())];
    let reads: &[&str] = match p.action.as_str() {
        "list" => &[],
        "save" => &["photo", "name", "groups"],
        "apply" => &["photo", "id", "name", "amount"],
        "delete" => &["id", "name"],
        other => return Err(format!("unknown action {other}; list, save, apply, delete")),
    };
    match given.iter().find(|(field, set)| *set && !reads.contains(field)) {
        Some((field, _)) => Err(format!("profiles {} doesn't read {field}", p.action)),
        None => Ok(()),
    }
}

pub async fn profiles(hub: &Rc<Hub>, driver: &Driver, cx: &mut AsyncApp, value: Value) -> Answer {
    let p: Profiles = args(value)?;
    profile_fields(&p)?;
    match p.action.as_str() {
        "list" => {
            let (list, applied) = driver
                .act(cx, |a, cx| {
                    let d = a.develop.read(cx);
                    let applied = d.applied.as_ref().filter(|x| x.photo_id == d.photo_id).map(|x| x.profile_id.clone());
                    let list = store::shown(&mut a.rail);
                    (profile_list(list), applied.filter(|id| list.iter().any(|p| p.id == *id)))
                })
                .ok_or(GONE)?;
            Ok(Output::json(strip(json!({"profiles": list, "applied": applied}))))
        }
        "save" => {
            let name = p.name.as_deref().map(str::trim).filter(|n| !n.is_empty()).ok_or("save needs a name")?.to_string();
            let known: Vec<&str> = LOOK_GROUPS.iter().chain(PER_PHOTO_GROUPS.iter()).copied().collect();
            let groups: Vec<String> = p.groups.unwrap_or_else(|| LOOK_GROUPS.iter().map(|g| (*g).to_string()).collect());
            if let Some(bad) = groups.iter().find(|g| !known.contains(&g.as_str())) {
                return Err(format!("unknown group {bad}; groups: {}", known.join(", ")));
            }
            let groups: Vec<String> = known.iter().filter(|g| groups.iter().any(|x| x == *g)).map(|g| (*g).to_string()).collect();
            let held = hub.hold(driver, cx, p.photo.as_deref()).await?;
            let (settings, film) = held.read(driver, cx, |e| (e.settings.clone(), e.film.map(|f| f.normalized().to_value()))).ok_or(NO_PHOTO)?;
            let saved = driver
                .act(cx, |a, cx| {
                    let settings = Settings::parse(&settings)?;
                    let profile = store::build(&name, &settings, groups, film).ok_or("the edit couldn't be saved as a profile")?;
                    let list = store::shown(&mut a.rail);
                    let replaced = list.iter().position(|x| x.name.eq_ignore_ascii_case(&name)).map(|i| list.remove(i)).is_some();
                    let out = json!({"saved": {"id": profile.id, "name": profile.name, "groups": profile.groups}, "replaced": replaced.then_some(true)});
                    list.insert(0, profile);
                    store::write(list);
                    cx.notify();
                    Ok::<Value, String>(out)
                })
                .ok_or(GONE)??;
            Ok(Output::json(strip(saved)))
        }
        "apply" => {
            let amount = p.amount.unwrap_or(100.0);
            if !(0.0..=200.0).contains(&amount) {
                return Err("amount is 0-200 (%)".into());
            }
            let held = hub.hold(driver, cx, p.photo.as_deref()).await?;
            let before = State::now(&held, driver, cx)?;
            let profile = driver
                .act(cx, |a, _| {
                    let list = store::shown(&mut a.rail);
                    find_profile(list, p.id.as_deref(), p.name.as_deref()).map(|i| list[i].clone())
                })
                .ok_or(GONE)??;
            held.apply_profile(driver, cx, &profile, amount / 100.0).await?;
            let out = strip(json!({"photo": p.photo.is_none().then(|| held.id(driver, cx)), "applied": profile.name, "amount": num(amount, 1)}));
            Ok(Output::json(effect::report(hub, &held, driver, cx, &before, out, &[]).await))
        }
        "delete" => {
            let deleted = driver
                .act(cx, |a, cx| {
                    let list = store::shown(&mut a.rail);
                    let gone = list.remove(find_profile(list, p.id.as_deref(), p.name.as_deref())?);
                    store::write(list);
                    cx.notify();
                    Ok::<(String, usize), String>((gone.name, list.len()))
                })
                .ok_or(GONE)??;
            Ok(Output::json(json!({"deleted": deleted.0, "left": deleted.1})))
        }
        other => Err(format!("unknown action {other}; list, save, apply, delete")),
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct Snapshot {
    name: String,
    settings: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    film: Option<Value>,
    at: String,
}

fn snapshots_file() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join("Library/Application Support/Filmlab/snapshots.json"))
}

fn load_snapshots() -> BTreeMap<String, Vec<Snapshot>> {
    snapshots_file().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn write_snapshots(all: &BTreeMap<String, Vec<Snapshot>>) -> Result<(), String> {
    let path = snapshots_file().ok_or("HOME isn't set")?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    crate::files::write_atomic(&path, serde_json::to_string(all).map_err(|e| e.to_string())?.as_bytes()).map_err(|e| format!("unable to save snapshots: {e}"))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Snapshots {
    photo: Option<String>,
    action: String,
    name: Option<String>,
    if_state: Option<String>,
    watch: Option<Vec<String>>,
}

fn snapshot_list(list: &[Snapshot]) -> Vec<Value> {
    list.iter().map(|s| json!([s.name, s.at.chars().take(19).collect::<String>()])).collect()
}

/// Where a photo's snapshots are filed: its canonical path.
fn snapshot_key(driver: &Driver, cx: &mut AsyncApp, id: &str) -> Result<String, String> {
    let path = driver.read(cx, |a, cx| a.library.read(cx).record(id).map(|r| r.file.path.clone())).flatten().ok_or(NO_PHOTO)?;
    Ok(std::fs::canonicalize(&path).unwrap_or(path).to_string_lossy().to_string())
}

/// A photo's snapshot by name: its settings and film.
pub fn snapshot(driver: &Driver, cx: &mut AsyncApp, id: &str, name: &str) -> Result<(Value, Option<FilmSettings>), String> {
    let photo = snapshot_key(driver, cx, id)?;
    let all = load_snapshots();
    let list = all.get(&photo).map_or(&[][..], Vec::as_slice);
    let snap = list.iter().find(|s| s.name == name).ok_or_else(|| format!("no snapshot {name}; saved: {}", list.iter().map(|s| s.name.as_str()).collect::<Vec<_>>().join(", ")))?;
    Ok((snap.settings.clone(), snap.film.as_ref().and_then(FilmSettings::of)))
}

pub async fn snapshots(hub: &Rc<Hub>, driver: &Driver, cx: &mut AsyncApp, value: Value) -> Answer {
    let p: Snapshots = args(value)?;
    let watch = effect::watching(p.watch.clone())?;
    let held = hub.hold(driver, cx, p.photo.as_deref()).await?;
    held.expect(p.if_state.clone());
    let (id, settings, film) = held.read(driver, cx, |e| (e.id.to_string(), e.settings.clone(), e.film.cloned())).ok_or(NO_PHOTO)?;
    let photo = snapshot_key(driver, cx, &id)?;
    let mut all = load_snapshots();
    let name = p.name.as_deref().map(str::trim).filter(|n| !n.is_empty()).map(str::to_string);
    let list = all.entry(photo.clone()).or_default();
    if p.action != "restore" && !watch.is_empty() {
        return Err("watch reads the photo after a change, and snapshot list, save and delete don't change it".into());
    }
    if matches!(p.action.as_str(), "save" | "delete") {
        held.ready(driver, cx)?;
    }
    match p.action.as_str() {
        "list" => Ok(Output::json(json!({"snapshots": snapshot_list(list)}))),
        "save" => {
            let name = name.unwrap_or_else(|| format!("Snapshot {}", list.len() + 1));
            list.retain(|s| s.name != name);
            list.push(Snapshot { name: name.clone(), settings, film: film.map(|f| f.normalized().to_value()), at: store::iso_now() });
            let names = snapshot_list(list);
            write_snapshots(&all)?;
            Ok(Output::json(json!({"saved": name, "snapshots": names})))
        }
        "restore" => {
            let name = name.ok_or("snapshot.restore needs args.name; status part snapshots lists them")?;
            let snap = list.iter().find(|s| s.name == name).cloned().ok_or_else(|| format!("no snapshot {name}; saved: {}", list.iter().map(|s| s.name.as_str()).collect::<Vec<_>>().join(", ")))?;
            let before = State { settings, film };
            let film = snap.film.as_ref().and_then(FilmSettings::of);
            let label = format!("Snapshot {name}");
            let key = next_key("snapshot");
            held.restore(driver, cx, snap.settings, film, &label, &key).await?;
            let (step, _, _) = history_at(&held, driver, cx).ok_or(NO_PHOTO)?;
            let out = strip(json!({"photo": p.photo.is_none().then_some(id), "restored": name, "step": step}));
            Ok(Output::json(effect::report(hub, &held, driver, cx, &before, out, &watch).await))
        }
        "delete" => {
            let name = name.ok_or("delete needs a name")?;
            let before = list.len();
            list.retain(|s| s.name != name);
            if list.len() == before {
                return Err(format!("no snapshot {name}"));
            }
            let names = snapshot_list(list);
            if list.is_empty() {
                all.remove(&photo);
            }
            write_snapshots(&all)?;
            Ok(Output::json(json!({"deleted": name, "snapshots": names})))
        }
        other => Err(format!("unknown action {other}; save, list, restore, delete")),
    }
}
