use std::rc::Rc;

use gpui_kit::{AppContext, AsyncApp};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::app::strip;
use super::look::{self, Pick, Shot};
use super::{Answer, Output, args, curve, edit, scopes};
use crate::film::FilmSettings;
use crate::mcp::session::{Edit, Held, Hub, NO_PHOTO};
use crate::ui::driver::Driver;

/// The colour difference (CIE76 ΔE) at which a pixel counts as changed: well under what the eye notices side by
/// side (about 2.3), and above what rounding to 8 bits moves.
const CHANGED: f64 = 1.0;
const GRID: usize = 4;
const STATES: &str = "from and to take a history step, \"previous\", \"current\", \"before\" or a snapshot name";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Compare {
    photo: Option<String>,
    from: Option<Value>,
    to: Option<Value>,
    values: Option<Map<String, Value>>,
    curve: Option<Value>,
    region: Option<[f64; 4]>,
    max_edge: Option<u32>,
    image: Option<String>,
    detail: Option<bool>,
}

#[derive(Clone)]
enum State {
    Step(usize),
    Current,
    Previous,
    Before,
    Snapshot(String),
}

fn state(v: &Value) -> Result<State, String> {
    match v {
        Value::Number(n) => n.as_u64().and_then(|n| usize::try_from(n).ok()).map(State::Step).ok_or_else(|| STATES.into()),
        Value::String(s) => Ok(match s.trim() {
            "current" => State::Current,
            "previous" => State::Previous,
            "before" => State::Before,
            name => name.parse().map_or_else(|_| State::Snapshot(name.to_string()), State::Step),
        }),
        _ => Err(STATES.into()),
    }
}

/// The held photo's edit as the comparison needs it.
struct Now {
    id: String,
    settings: Value,
    film: Option<FilmSettings>,
    before: Option<Value>,
    facts: Option<Value>,
    steps: Vec<(String, Value, Option<FilmSettings>)>,
    cursor: usize,
}

impl Now {
    fn of(e: &Edit<'_>) -> Now {
        Now {
            id: e.id.to_string(),
            settings: e.settings.clone(),
            film: e.film.cloned(),
            before: e.before.cloned(),
            facts: e.facts.cloned(),
            steps: e.history.iter().map(|s| (s.label.clone(), s.settings.clone(), s.film.clone())).collect(),
            cursor: e.cursor,
        }
    }
}

/// One side of a comparison: what to call it and the edit to render.
struct Side {
    name: String,
    settings: Value,
    film: Option<FilmSettings>,
}

fn side(state: State, now: &Now, driver: &Driver, cx: &mut AsyncApp) -> Result<Side, String> {
    let step = |i: usize| {
        let (label, settings, film) = now.steps.get(i).ok_or_else(|| format!("no step {i}; steps are 0..{}", now.steps.len().saturating_sub(1)))?;
        Ok::<Side, String>(Side { name: format!("step {i}: {label}"), settings: settings.clone(), film: film.clone() })
    };
    match state {
        State::Step(i) => step(i),
        State::Current => Ok(Side { name: format!("current, step {}", now.cursor), settings: now.settings.clone(), film: now.film.clone() }),
        State::Previous if now.cursor == 0 => Err("no previous step: the photo is at step 0 (as opened); compare from \"before\" or a snapshot".into()),
        State::Previous => step(now.cursor - 1),
        State::Before => now.before.clone().map(|settings| Side { name: "before: unedited defaults, no film".into(), settings, film: None }).ok_or_else(|| "the photo's state before editing isn't known".into()),
        State::Snapshot(name) => edit::snapshot(driver, cx, &now.id, &name).map(|(settings, film)| Side { name: format!("snapshot {name}"), settings, film }),
    }
}

pub fn curve_at(held: &Held, driver: &Driver, cx: &mut AsyncApp, at: &Value) -> Result<(String, Value), String> {
    let now = held.read(driver, cx, |e| Now::of(&e)).ok_or(NO_PHOTO)?;
    let side = side(state(at)?, &now, driver, cx)?;
    Ok((side.name, side.settings["curve"].clone()))
}

/// `side` with `values` and a curve set as an edit would, without saving anything.
fn tried(side: Side, values: Option<&Map<String, Value>>, shape: Option<&Value>, facts: &Value) -> Result<(Side, Vec<String>), String> {
    let (settings, mut clamped) = match values {
        Some(values) => edit::with_values(&side.settings, values, facts)?,
        None => (side.settings.clone(), Vec::new()),
    };
    let (settings, label) = match shape {
        Some(shape) => {
            let curved = curve::with_curve(&settings, shape, facts)?;
            clamped.extend(curved.tally.clamped);
            (curved.settings, Some(curved.label))
        }
        None => (settings, None),
    };
    let names: Vec<String> = values.into_iter().flat_map(|v| v.keys().cloned()).chain(label).collect();
    Ok((Side { name: format!("{} + {}", side.name, names.join(", ")), settings, film: side.film }, clamped))
}

#[derive(Clone, Copy)]
enum Picture {
    Heat,
    Pair,
    None,
}

fn picture(name: Option<&str>) -> Result<Picture, String> {
    match name.unwrap_or("heat") {
        "heat" => Ok(Picture::Heat),
        "pair" => Ok(Picture::Pair),
        "none" => Ok(Picture::None),
        other => Err(format!("unknown image {other}; heat, pair or none")),
    }
}

/// `b - a` for every number in two values of the same shape, rounded.
fn delta(a: &Value, b: &Value) -> Value {
    match (a, b) {
        (Value::Object(a), Value::Object(b)) => Value::Object(a.iter().filter_map(|(k, va)| Some((k.clone(), delta(va, b.get(k)?)))).collect()),
        (Value::Array(a), Value::Array(b)) => Value::Array(a.iter().zip(b).map(|(a, b)| delta(a, b)).collect()),
        (Value::Number(a), Value::Number(b)) => json!(look::round(b.as_f64().unwrap_or(0.0) - a.as_f64().unwrap_or(0.0), 3)),
        _ => Value::Null,
    }
}

/// Colourfulness for its lightness: mean chroma over L* of the pixels that aren't near black, so brightening alone
/// barely moves it.
fn saturation(labs: &[[f64; 3]]) -> f64 {
    let lit: Vec<f64> = labs.iter().filter(|c| c[0] >= 10.0).map(|c| c[1].hypot(c[2]) / c[0]).collect();
    lit.iter().sum::<f64>() / lit.len().max(1) as f64
}

fn percent(a: f64, b: f64) -> f64 {
    if a > 1e-6 { look::round((b / a - 1.0) * 100.0, 1) } else { 0.0 }
}

/// Where the changed pixels are: the box around them (0-1 of the frame) and the mean ΔE of each cell of a grid,
/// top row first.
fn extent(de: &[f64], width: usize) -> (Option<[f64; 4]>, Vec<Vec<f64>>) {
    let height = de.len() / width.max(1);
    let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
    let mut cells = [[(0.0, 0usize); GRID]; GRID];
    for (i, d) in de.iter().enumerate() {
        let (x, y) = (i % width, i / width);
        let cell = &mut cells[y * GRID / height.max(1)][x * GRID / width.max(1)];
        cell.0 += d;
        cell.1 += 1;
        if *d >= CHANGED {
            (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1));
        }
    }
    let (w, h) = (width.max(1) as f64, height.max(1) as f64);
    let bbox = (x0 != usize::MAX).then(|| [x0 as f64 / w, y0 as f64 / h, x1 as f64 / w, y1 as f64 / h].map(|v| look::round(v, 3)));
    let grid = cells.iter().map(|row| row.iter().map(|(sum, n)| look::round(sum / (*n).max(1) as f64, 1)).collect()).collect();
    (bbox, grid)
}

fn spread(m: &Value) -> f64 {
    m["luma"]["p95"].as_f64().unwrap_or(0.0) - m["luma"]["p5"].as_f64().unwrap_or(0.0)
}

/// Whether two edits put the photo's pixels in different places (crop, straightening, orientation, transforms, lens
/// distortion), so a pixel diff mixes moved content with any change of tone or colour.
pub fn moved(a: &Value, b: &Value) -> bool {
    ["crop", "geometry", "orientation"].iter().any(|k| a[k] != b[k]) || ["distortion", "profile_distortion", "lens_profile"].iter().any(|k| a["optics"][k] != b["optics"][k])
}

/// The colour cast change of the pixels that were near neutral in the first render, in a* and b*, and their share of
/// the frame. Their shift is fitted as a common offset plus a push away from grey: a white balance change moves every
/// pixel by the same offset, more saturation pushes each pixel along its own colour, so only the offset is the cast.
struct Neutral {
    a: f64,
    b: f64,
    share: f64,
}

impl Neutral {
    fn of(lab_a: &[[f64; 3]], lab_b: &[[f64; 3]]) -> Neutral {
        let moves: Vec<[f64; 4]> = lab_a
            .iter()
            .zip(lab_b)
            .filter(|(p, _)| p[1].hypot(p[2]) < 12.0 && (20.0..90.0).contains(&p[0]))
            .map(|(p, q)| [p[1], p[2], q[1] - p[1], q[2] - p[2]])
            .collect();
        let n = moves.len().max(1) as f64;
        let mean = |f: &dyn Fn(&[f64; 4]) -> f64| moves.iter().map(f).sum::<f64>() / n;
        let (pa, pb, da, db) = (mean(&|m| m[0]), mean(&|m| m[1]), mean(&|m| m[2]), mean(&|m| m[3]));
        let spread = mean(&|m| (m[0] - pa).powi(2) + (m[1] - pb).powi(2));
        let push = if spread > 1e-9 { mean(&|m| (m[0] - pa) * (m[2] - da) + (m[1] - pb) * (m[3] - db)) / spread } else { 0.0 };
        Neutral { a: da - push * pa, b: db - push * pb, share: moves.len() as f64 / lab_a.len().max(1) as f64 * 100.0 }
    }

    // why: with few neutral pixels the frame's mean colour stands in, though it also moves with exposure and saturation
    fn cast(&self, lab_mean: &Value) -> (f64, f64) {
        if self.share >= 2.0 { (self.b, self.a) } else { (lab_mean[2].as_f64().unwrap_or(0.0), lab_mean[1].as_f64().unwrap_or(0.0)) }
    }
}

/// The measured shifts the summary reads. `strength` is the p95 ΔE of the changed pixels alone, so a strong change
/// to a small area reads as strong.
struct Signals {
    brightness: f64,
    contrast: f64,
    warmth: f64,
    magenta: f64,
    saturation: f64,
    detail: f64,
    changed: f64,
    area: f64,
    strength: f64,
}

impl Signals {
    fn strength(&self) -> &'static str {
        match self.strength {
            v if v < 2.3 => "faint",
            v if v < 5.0 => "subtle",
            v if v < 10.0 => "clear",
            _ => "strong",
        }
    }

    fn extent(&self) -> &'static str {
        if self.changed >= 80.0 {
            "everywhere"
        } else if self.area < 0.5 {
            "in a small area"
        } else {
            "in parts"
        }
    }

    /// The change in words; only what clears the noise.
    fn words(&self) -> Vec<&'static str> {
        let sign = |v: f64, limit: f64, up: &'static str, down: &'static str| {
            if v >= limit {
                Some(up)
            } else if v <= -limit {
                Some(down)
            } else {
                None
            }
        };
        [
            Some(self.strength()),
            sign(self.brightness, 0.01, "brighter", "darker"),
            sign(self.contrast, 0.02, "more contrast", "less contrast"),
            sign(self.warmth, 2.0, "warmer", "cooler"),
            sign(self.magenta, 2.0, "more magenta", "greener"),
            sign(self.saturation, 5.0, "more saturated", "less saturated"),
            sign(self.detail, 5.0, "more texture", "less texture"),
            Some(self.extent()),
        ]
        .into_iter()
        .flatten()
        .collect()
    }
}

/// A darkened grey copy of `shot` with each changed pixel coloured from red to yellow to white as its ΔE nears
/// `scale`.
fn heat(shot: &Shot, de: &[f64], scale: f64) -> Shot {
    let rgba = shot
        .rgba
        .as_chunks::<4>()
        .0
        .iter()
        .zip(de)
        .flat_map(|(p, d)| {
            let base = look::luma(p) * 0.35;
            let t = (d / scale).clamp(0.0, 1.0);
            let hot = [(3.0 * t).min(1.0), (3.0 * t - 1.0).clamp(0.0, 1.0), (3.0 * t - 2.0).clamp(0.0, 1.0)];
            let alpha = if *d < CHANGED { 0.0 } else { 0.3 + 0.5 * t };
            let [r, g, b] = hot.map(|c| ((base * (1.0 - alpha) + c * alpha) * 255.0).round() as u8);
            [r, g, b, 255]
        })
        .collect();
    Shot { rgba, width: shot.width, height: shot.height }
}

fn pair(a: &Shot, b: &Shot) -> Shot {
    let (w, h) = (a.width as usize, a.height as usize);
    let mut rgba = Vec::with_capacity(w * h * 8);
    for y in 0..h {
        rgba.extend_from_slice(&a.rgba[y * w * 4..(y + 1) * w * 4]);
        rgba.extend_from_slice(&b.rgba[y * w * 4..(y + 1) * w * 4]);
    }
    Shot { rgba, width: a.width * 2, height: a.height }
}

/// Two renders of one frame measured against each other.
struct Diff {
    summary: String,
    identical: bool,
    changed: f64,
    strength: f64,
    everywhere: bool,
    was: Value,
    now: Value,
    detail: Value,
    de: Vec<f64>,
    scale: f64,
}

impl Diff {
    fn of(a: &Shot, b: &Shot, moved: bool) -> Diff {
        let (pa, pb) = (a.rgba.as_chunks::<4>().0, b.rgba.as_chunks::<4>().0);
        let width = a.width as usize;
        let n = pa.len().max(1) as f64;
        let (lab_a, lab_b): (Vec<[f64; 3]>, Vec<[f64; 3]>) = pa.iter().zip(pb).map(|(x, y)| (look::lab(x), look::lab(y))).unzip();
        let de: Vec<f64> = lab_a.iter().zip(&lab_b).map(|(p, q)| ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) + (p[2] - q[2]).powi(2)).sqrt()).collect();
        let mut sorted = de.clone();
        sorted.sort_by(f64::total_cmp);
        let identical = pa.iter().zip(pb).all(|(x, y)| x[..3] == y[..3]);
        let max_channel = pa.iter().zip(pb).flat_map(|(x, y)| (0..3).map(move |k| x[k].abs_diff(y[k]))).max().unwrap_or(0);
        let moved_px: Vec<f64> = sorted.iter().copied().filter(|d| *d >= CHANGED).collect();
        let changed = moved_px.len() as f64 / n * 100.0;
        let (luma_a, luma_b): (Vec<f64>, Vec<f64>) = pa.iter().zip(pb).map(|(x, y)| (look::luma(x), look::luma(y))).unzip();
        let drift = luma_a.iter().zip(&luma_b).map(|(x, y)| (y - x).abs()).sum::<f64>() / n;
        let (ma, mb) = (look::measure(a), look::measure(b));
        let neutral = Neutral::of(&lab_a, &lab_b);
        let shift = json!({
            "luma": delta(&ma["luma"], &mb["luma"]),
            "clip%": delta(&ma["clip%"], &mb["clip%"]),
            "lab_mean": delta(&ma["lab_mean"], &mb["lab_mean"]),
            "chroma": delta(&ma["chroma"], &mb["chroma"]),
            "neutral": {"a": look::round(neutral.a, 2), "b": look::round(neutral.b, 2), "share%": look::round(neutral.share, 1)},
        });
        let (bbox, grid) = extent(&de, width);
        let (warmth, magenta) = neutral.cast(&shift["lab_mean"]);
        let signals = Signals {
            brightness: shift["luma"]["mean"].as_f64().unwrap_or(0.0),
            contrast: look::round(spread(&mb) - spread(&ma), 3),
            warmth,
            magenta,
            saturation: percent(saturation(&lab_a), saturation(&lab_b)),
            detail: percent(scopes::texture(&luma_a, width), scopes::texture(&luma_b, width)),
            changed,
            area: bbox.map_or(0.0, |[x0, y0, x1, y1]| (x1 - x0) * (y1 - y0)),
            strength: look::percentile(&moved_px, 0.95),
        };
        let summary = match (moved, identical, moved_px.is_empty()) {
            (false, true, _) => vec!["identical"],
            (false, false, true) => vec!["no visible change"],
            (false, false, false) => signals.words(),
            (true, true, _) => vec!["geometry changed", "nothing else"],
            (true, false, true) => vec!["geometry changed", "nothing else visible"],
            (true, false, false) => vec!["geometry changed", signals.strength(), signals.extent()],
        };
        let detail = json!({
            "identical": identical,
            "geometry_changed": moved,
            "changed%": look::round(changed, 3),
            "changed_px": moved_px.len(),
            "dE": {"mean": look::round(de.iter().sum::<f64>() / n, 2), "p50": look::round(look::percentile(&sorted, 0.5), 2), "p95": look::round(look::percentile(&sorted, 0.95), 2), "max": look::round(sorted.last().copied().unwrap_or(0.0), 2), "changed_p95": look::round(signals.strength, 2)},
            "max_channel": max_channel,
            "shift": shift,
            "contrast": signals.contrast,
            "saturation%": signals.saturation,
            "detail%": signals.detail,
            "luma_moved": look::round(drift, 4),
            "where": {"bbox": bbox, "grid": grid},
        });
        Diff {
            summary: summary.join(", "),
            identical,
            changed,
            strength: signals.strength,
            everywhere: signals.extent() == "everywhere",
            was: ma,
            now: mb,
            detail,
            scale: look::percentile(&sorted, 0.99).max(2.0 * CHANGED),
            de,
        }
    }

    /// The comparison in brief: what changed in words, how much and where, and the photo's numbers on each side.
    fn brief(&self) -> Value {
        let local = self.changed > 0.0 && !self.everywhere;
        json!({
            "summary": self.summary,
            "identical": self.identical.then_some(true),
            "changed%": look::round(self.changed, 1),
            "dE": look::round(self.strength, 1),
            "where": local.then(|| self.detail["where"].clone()),
            "was": look::glance(&self.was),
            "now": look::glance(&self.now),
        })
    }
}

/// What a change did, for an edit's answer: the change in words, as compare says it, and the photo's numbers now.
pub fn effect(a: &Shot, b: &Shot, moved: bool) -> Value {
    if (a.width, a.height) != (b.width, b.height) {
        return json!({"effect": format!("geometry changed: frame {}x{} to {}x{}", a.width, a.height, b.width, b.height), "now": look::glance(&look::measure(b))});
    }
    let d = Diff::of(a, b, moved);
    let (delta, now) = look::changes(&d.was, &d.now);
    let edge = b.width.max(b.height);
    let words = match d.summary.as_str() {
        "identical" => format!("unchanged at {edge} px"),
        "no visible change" => format!("slight, under what shows at {edge} px"),
        other => other.to_string(),
    };
    let mut out = json!({"effect": words});
    if !delta.is_empty() {
        out["delta"] = Value::Object(delta);
    }
    if !now.is_empty() {
        out["now"] = Value::Object(now);
    }
    out
}

fn diff(a: &Shot, b: &Shot, picture: Picture, moved: bool, detail: bool, lossless: bool) -> (Value, Option<Result<Vec<u8>, String>>) {
    let d = Diff::of(a, b, moved);
    let mut stats = if detail { d.detail.clone() } else { d.brief() };
    stats["size"] = json!([a.width, a.height]);
    if detail {
        stats["summary"] = json!(d.summary);
    }
    if matches!(picture, Picture::Heat) {
        stats["heat_full_at_dE"] = json!(look::round(d.scale, 1));
    }
    let encode = |shot: &Shot| if lossless { look::png(shot) } else { look::jpeg(shot, 80) };
    let image = match picture {
        Picture::Heat => Some(encode(&heat(b, &d.de, d.scale))),
        Picture::Pair => Some(encode(&pair(a, b))),
        Picture::None => None,
    };
    (stats, image)
}

pub async fn compare(hub: &Rc<Hub>, driver: &Driver, cx: &mut AsyncApp, value: Value) -> Answer {
    let p: Compare = args(value)?;
    let picture = picture(p.image.as_deref())?;
    let to_state = p.to.as_ref().map(state).transpose()?.unwrap_or(State::Current);
    let tries = p.values.is_some() || p.curve.is_some();
    let from_state = match &p.from {
        Some(v) => state(v)?,
        None if tries => to_state.clone(),
        None => State::Previous,
    };
    let held = hub.hold(driver, cx, p.photo.as_deref()).await?;
    let now = held.read(driver, cx, |e| Now::of(&e)).ok_or(NO_PHOTO)?;
    let facts = now.facts.clone().ok_or(NO_PHOTO)?;
    let from = side(from_state, &now, driver, cx)?;
    let to = side(to_state, &now, driver, cx)?;
    let (to, clamped) = match tries {
        true => tried(to, p.values.as_ref(), p.curve.as_ref(), &facts)?,
        false => (to, Vec::new()),
    };
    let cropped = [look::frame_size(&facts, &from.settings, true)?, look::frame_size(&facts, &to.settings, true)?];
    let crop = cropped[0] == cropped[1];
    if p.region.is_some() && moved(&from.settings, &to.settings) {
        return Err("the two states frame the photo differently (crop, rotation or lens geometry), so a region isn't the same place in both; compare without region, or a region of each with look preview".into());
    }
    if !crop {
        if p.region.is_some() {
            return Err(format!("the crops differ ({:?} against {:?}), so a region isn't the same place in both; compare without region", cropped[0], cropped[1]));
        }
        let whole = [look::frame_size(&facts, &from.settings, false)?, look::frame_size(&facts, &to.settings, false)?];
        if whole[0] != whole[1] {
            return Ok(Output::json(json!({"from": from.name, "to": to.name, "summary": "geometry changed, not comparable: the frames differ in shape even uncropped", "geometry_changed": true, "comparable": false, "frames": {"from": cropped[0], "to": cropped[1]}})));
        }
    }
    let edge = p.max_edge.unwrap_or(look::MEASURED).clamp(64, 2048);
    let a = look::render(&held, driver, cx, edge, p.region, &Pick::Edit(&from.settings, from.film.as_ref()), crop).await?;
    let b = look::render(&held, driver, cx, edge, p.region, &Pick::Edit(&to.settings, to.film.as_ref()), crop).await?;
    let traced = match curve::moved(&from.settings, &to.settings) {
        true => Some(look::input_frame(hub, &held, driver, cx, &look::State { settings: to.settings.clone(), film: to.film.clone() }, look::MEASURED).await?),
        false => None,
    };
    let curves = (from.settings["curve"].clone(), to.settings["curve"].clone());
    drop(held);
    if (a.width, a.height) != (b.width, b.height) {
        return Err(format!("the renders came out {}x{} and {}x{}", a.width, a.height, b.width, b.height));
    }
    let moved = moved(&from.settings, &to.settings);
    let detail = p.detail.unwrap_or(false);
    let lossless = p.region.is_some();
    let scale = match p.region {
        Some([x0, _, x1, _]) => Some(look::round((x1 - x0).abs() * look::frame_size(&facts, &to.settings, crop)?[0] / f64::from(b.width), 2)),
        None => None,
    };
    let (stats, image, block) = cx
        .background_spawn(async move {
            let block = traced.and_then(|input| curve::block(&curves.0, &curves.1, &curve::Intake::of(&input)).ok().flatten());
            let (stats, image) = diff(&a, &b, picture, moved, detail, lossless);
            (stats, image, block)
        })
        .await;
    let mut out = json!({"from": from.name, "to": to.name, "region": p.region, "scale": scale, "clamped": clamped});
    if !crop {
        out["frames"] = json!({"from": cropped[0], "to": cropped[1], "compared": "uncropped, since the crops differ"});
    }
    if let (Value::Object(out), Value::Object(stats)) = (&mut out, stats) {
        out.extend(stats);
    }
    if let Some(block) = block {
        out["curve"] = block;
    }
    let output = Output::json(strip(out));
    match image {
        Some(jpeg) => Ok(output.with_image(jpeg?)),
        None => Ok(output),
    }
}
