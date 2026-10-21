use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io::Cursor;
use std::rc::Rc;
use std::sync::Arc;

use filmlab_engine::core::{self, Facts};
use gpui_kit::{AppContext, AsyncApp, Task};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::app::strip;
use super::{Answer, Output, args, chart, curve, edit, scopes};
use crate::develop::RenderJob;
use crate::develop::plans::PREVIEW_EDGE;
use crate::film::{FilmSettings, render_film};
use crate::library::ensure_luts;
use crate::mcp::session::{Held, Hub, NO_PHOTO};
use crate::settings::Settings;
use crate::ui::driver::Driver;

pub struct Shot {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

const STAGES: &str = "mcp";

/// The size changes are measured at: enough for tones and colour, and quick to draw.
pub const MEASURED: u32 = 512;

/// One edit of a photo: its settings and film look.
#[derive(Clone)]
pub struct State {
    pub settings: Value,
    pub film: Option<FilmSettings>,
}

impl State {
    /// The held photo's edit now.
    pub fn now(held: &Held, driver: &Driver, cx: &mut AsyncApp) -> Result<State, String> {
        held.read(driver, cx, |e| State { settings: e.settings.clone(), film: e.film.cloned() }).ok_or_else(|| NO_PHOTO.into())
    }

    /// The held photo as opened, without film.
    pub fn opened(held: &Held, driver: &Driver, cx: &mut AsyncApp) -> Result<State, String> {
        held.read(driver, cx, |e| e.before.map(|settings| State { settings, film: None }))
            .flatten()
            .ok_or_else(|| "the photo's state before editing isn't known".into())
    }

    pub fn same(&self, other: &State) -> bool {
        self.settings == other.settings && render_film(self.film.as_ref()) == render_film(other.film.as_ref())
    }

    /// A short name for exactly this edit of photo `id`, for a change to require it hasn't moved on (`if_state`).
    pub fn digest(&self, id: &str) -> String {
        crate::mcp::session::digest(id, &self.settings, self.film.as_ref())
    }
}

/// Which edit of the held photo to render: its current one, the photo as opened without film, or any other.
pub enum Pick<'a> {
    Current,
    Before,
    Edit(&'a Value, Option<&'a FilmSettings>),
}

impl Pick<'_> {
    fn of(before: bool) -> Pick<'static> {
        if before { Pick::Before } else { Pick::Current }
    }

    pub fn onto(&self, mut job: RenderJob) -> Result<RenderJob, String> {
        if let Pick::Edit(settings, film) = self {
            job.settings = Settings::parse(settings)?;
            job.film = render_film(*film);
        }
        Ok(job)
    }
}

fn ordered(region: [f64; 4]) -> Result<[f64; 4], String> {
    if region.iter().any(|v| !(0.0..=1.0).contains(v)) {
        return Err("region is [x0, y0, x1, y1], each 0-1 of the frame".into());
    }
    let [a, b, c, d] = region;
    let r = [a.min(c), b.min(d), a.max(c), b.max(d)];
    if r[2] - r[0] < 1e-3 || r[3] - r[1] < 1e-3 {
        return Err("region is empty: give [x0, y0, x1, y1] with x1 > x0 and y1 > y0, in 0-1".into());
    }
    Ok(r)
}

/// The size of the photo's frame under `settings`, cropped or not.
pub fn frame_size(facts: &Value, settings: &Value, crop: bool) -> Result<[f64; 2], String> {
    serde_json::from_str(&core::frame_size(&facts.to_string(), &settings.to_string(), crop)?).map_err(|e| e.to_string())
}

/// The size of the frame `job` draws, cropped or not.
fn job_frame(job: &RenderJob, crop: bool) -> Result<[f64; 2], String> {
    let facts: Facts = serde_json::from_str(&job.facts.to_string()).map_err(|e| e.to_string())?;
    let [w, h] = core::frame_size_of(&facts, &job.settings, crop);
    Ok([w as f64, h as f64])
}

/// Renders the held photo off screen: the whole frame within `max_edge`, or `region` (0-1 of the frame) at up to full
/// resolution. The frame is cropped unless `crop` is false.
pub async fn render(held: &Held, driver: &Driver, cx: &mut AsyncApp, max_edge: u32, region: Option<[f64; 4]>, pick: &Pick<'_>, crop: bool) -> Result<Shot, String> {
    let (job, view) = framing(held, driver, cx, max_edge, region, pick, crop).await?;
    draw(job, view, cx).await
}

pub async fn framing(held: &Held, driver: &Driver, cx: &mut AsyncApp, max_edge: u32, region: Option<[f64; 4]>, pick: &Pick<'_>, crop: bool) -> Result<(RenderJob, Value), String> {
    let region = region.map(ordered).transpose()?;
    let before = matches!(pick, Pick::Before);
    let probe = pick.onto(held.render_job(driver, cx, before, false).await?)?;
    let [w, h] = job_frame(&probe, crop)?;
    let edge = f64::from(max_edge);
    let (view, full) = match region {
        None => (json!({"maxW": max_edge, "maxH": max_edge, "stages": STAGES, "applyCrop": crop}), false),
        Some([x0, y0, x1, y1]) => {
            let (rw, rh) = ((x1 - x0) * w, (y1 - y0) * h);
            let s = (edge / rw.max(rh)).min(1.0);
            let (fw, fh) = ((w * s).round().max(1.0), (h * s).round().max(1.0));
            let window = [(x0 * fw).round(), (y0 * fh).round(), (rw * s).round().max(1.0), (rh * s).round().max(1.0)];
            let window = [window[0], window[1], window[2].min(fw - window[0]), window[3].min(fh - window[1])];
            (json!({"maxW": fw as usize, "maxH": fh as usize, "stages": STAGES, "applyCrop": crop, "window": window.map(|v| v as usize)}), fw.max(fh) > PREVIEW_EDGE)
        }
    };
    let job = if full && !probe.full { pick.onto(held.render_job(driver, cx, before, true).await?)? } else { probe };
    Ok((job, view))
}

pub fn into_curve(mut view: Value) -> Value {
    view["curveInput"] = json!(true);
    view
}

/// The view that draws the whole frame within `max_edge`, cropped.
pub fn whole(max_edge: u32) -> Value {
    json!({"maxW": max_edge, "maxH": max_edge, "stages": STAGES, "applyCrop": true})
}

/// Draws `job` through `view` on a background thread.
pub async fn draw(job: RenderJob, view: Value, cx: &mut AsyncApp) -> Result<Shot, String> {
    cx.background_spawn(async move {
        // why: the lock is taken here, outside the render pool; a pool thread waiting on it while helping a render
        // that holds it would deadlock the pool
        let mut engine = job.engine.lock().map_err(|e| e.to_string())?;
        let engine = &mut *engine;
        ensure_luts(engine, job.film.as_ref())?;
        let mut view = view;
        if let Some(f) = &job.film {
            view["film"] = f.to_value();
        }
        let frame = crate::perf::interactive(|| engine.render_develop(&job.key, &job.settings, &view.to_string()));
        engine.forget_view(&job.key, STAGES);
        let mut frame = frame?;
        let shot = Shot { width: frame.width(), height: frame.height(), rgba: frame.take_rgba() };
        if shot.rgba.is_empty() || shot.rgba.len() != shot.width as usize * shot.height as usize * 4 {
            eprintln!("mcp: {} rendered {}x{} with {} bytes", job.key, shot.width, shot.height, shot.rgba.len());
            return Err(format!("the render of {} came back without its pixels", job.key));
        }
        Ok(shot)
    })
    .await
}

fn look_of(job: &RenderJob, edge: u32, input: bool) -> u64 {
    let mut hasher = DefaultHasher::new();
    job.key.hash(&mut hasher);
    match input {
        true => {
            let mut uncurved = job.settings.to_value();
            if let Some(o) = uncurved.as_object_mut() {
                o.remove("curve");
            }
            uncurved.to_string().hash(&mut hasher);
        }
        false => job.settings.json().hash(&mut hasher),
    }
    job.film.as_ref().map(|f| f.to_value().to_string()).hash(&mut hasher);
    edge.hash(&mut hasher);
    input.hash(&mut hasher);
    hasher.finish()
}

/// The whole frame of `state` within `edge`, from the photo's current decode. The hub keeps the last few, so the
/// state before an agent's change is usually drawn already.
pub async fn frame(hub: &Hub, held: &Held, driver: &Driver, cx: &mut AsyncApp, state: &State, edge: u32) -> Result<Arc<Shot>, String> {
    kept(hub, held, driver, cx, state, edge, false).await
}

pub async fn input_frame(hub: &Hub, held: &Held, driver: &Driver, cx: &mut AsyncApp, state: &State, edge: u32) -> Result<Arc<Shot>, String> {
    kept(hub, held, driver, cx, state, edge, true).await
}

async fn kept(hub: &Hub, held: &Held, driver: &Driver, cx: &mut AsyncApp, state: &State, edge: u32, input: bool) -> Result<Arc<Shot>, String> {
    let id = held.id(driver, cx).ok_or(NO_PHOTO)?;
    let job = Pick::Edit(&state.settings, state.film.as_ref()).onto(held.render_job(driver, cx, false, false).await?)?;
    let look = look_of(&job, edge, input);
    if let Some(shot) = hub.kept(&id, look) {
        return Ok(shot);
    }
    let view = if input { into_curve(whole(edge)) } else { whole(edge) };
    let shot = Arc::new(draw(job, view, cx).await?);
    hub.keep(&id, look, shot.clone());
    Ok(shot)
}

/// Each photo's whole frame within `edge`, as edited (`before`: as opened), a few photos at a time.
pub async fn frames(hub: &Rc<Hub>, driver: &Driver, cx: &mut AsyncApp, ids: &[String], edge: u32, before: bool) -> Vec<Result<Arc<Shot>, String>> {
    let tasks: Vec<Task<Result<Arc<Shot>, String>>> = ids
        .iter()
        .map(|id| {
            let (hub, driver, id) = (hub.clone(), driver.clone(), id.clone());
            cx.spawn(async move |cx| {
                let _turn = hub.turn().await;
                let held = hub.hold(&driver, cx, Some(&id)).await?;
                let state = if before { State::opened(&held, &driver, cx)? } else { State::now(&held, &driver, cx)? };
                frame(&hub, &held, &driver, cx, &state, edge).await
            })
        })
        .collect();
    let mut out = Vec::with_capacity(ids.len());
    for task in tasks {
        out.push(task.await);
    }
    out
}

pub fn jpeg(shot: &Shot, quality: u8) -> Result<Vec<u8>, String> {
    let rgb: Vec<u8> = shot.rgba.as_chunks::<4>().0.iter().flat_map(|p| [p[0], p[1], p[2]]).collect();
    let mut out = Cursor::new(Vec::new());
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality)
        .encode(&rgb, shot.width, shot.height, image::ExtendedColorType::Rgb8)
        .map_err(|e| e.to_string())?;
    Ok(out.into_inner())
}

pub fn luma(p: &[u8; 4]) -> f64 {
    (0.2126 * f64::from(p[0]) + 0.7152 * f64::from(p[1]) + 0.0722 * f64::from(p[2])) / 255.0
}

pub fn percentile(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let i = ((sorted.len() - 1) as f64 * q).round() as usize;
    (sorted[i] * 1000.0).round() / 1000.0
}

pub fn round(v: f64, places: i32) -> f64 {
    let f = 10f64.powi(places);
    let r = (v * f).round() / f;
    if r == 0.0 { 0.0 } else { r }
}

/// A preview's size and its `glance` numbers.
fn brief(shot: &Shot) -> Value {
    let mut out = glance(&measure(shot));
    out["size"] = json!([shot.width, shot.height]);
    out
}

fn decoded(v: f64) -> f64 {
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

pub fn lab(p: &[u8; 4]) -> [f64; 3] {
    lab_of([p[0], p[1], p[2]].map(|c| f64::from(c) / 255.0))
}

pub fn lab_of(encoded: [f64; 3]) -> [f64; 3] {
    let [r, g, b] = encoded.map(decoded);
    let x = (0.4124 * r + 0.3576 * g + 0.1805 * b) / 0.95047;
    let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    let z = (0.0193 * r + 0.1192 * g + 0.9505 * b) / 1.08883;
    let f = |t: f64| if t > 216.0 / 24389.0 { t.cbrt() } else { (24389.0 / 27.0 * t + 16.0) / 116.0 };
    let (fx, fy, fz) = (f(x), f(y), f(z));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

/// Tones, clipping, colour and the cast of near-neutral pixels, from display-referred sRGB.
pub fn measure(shot: &Shot) -> Value {
    let px: &[[u8; 4]] = shot.rgba.as_chunks::<4>().0;
    let n = px.len().max(1) as f64;
    let mut l: Vec<f64> = px.iter().map(luma).collect();
    l.sort_by(f64::total_cmp);
    let mut bins = [0usize; 16];
    for v in &l {
        bins[((v * 16.0) as usize).min(15)] += 1;
    }
    let labs: Vec<[f64; 3]> = px.iter().map(lab).collect();
    let mut chroma: Vec<f64> = labs.iter().map(|c| c[1].hypot(c[2])).collect();
    chroma.sort_by(f64::total_cmp);
    let mean = |i: usize| labs.iter().map(|c| c[i]).sum::<f64>() / n;
    let neutral: Vec<&[f64; 3]> = labs.iter().filter(|c| c[1].hypot(c[2]) < 12.0 && (20.0..90.0).contains(&c[0])).collect();
    let nn = neutral.len().max(1) as f64;
    let channel = |k: usize| {
        let mut c: Vec<f64> = px.iter().map(|p| f64::from(p[k]) / 255.0).collect();
        c.sort_by(f64::total_cmp);
        json!([percentile(&c, 0.01), percentile(&c, 0.5), percentile(&c, 0.99)])
    };
    json!({
        "size": [shot.width, shot.height],
        "luma": {"p1": percentile(&l, 0.01), "p5": percentile(&l, 0.05), "p25": percentile(&l, 0.25), "p50": percentile(&l, 0.5), "p75": percentile(&l, 0.75), "p95": percentile(&l, 0.95), "p99": percentile(&l, 0.99), "mean": round(l.iter().sum::<f64>() / n, 3)},
        "clip%": {
            "white": round(px.iter().filter(|p| p[..3].iter().any(|&c| c >= 254)).count() as f64 / n * 100.0, 2),
            "black": round(px.iter().filter(|p| p[..3].iter().all(|&c| c <= 1)).count() as f64 / n * 100.0, 2),
        },
        "rgb_p1_p50_p99": {"r": channel(0), "g": channel(1), "b": channel(2)},
        "lab_mean": [round(mean(0), 1), round(mean(1), 1), round(mean(2), 1)],
        "chroma": {"p50": round(percentile(&chroma, 0.5), 1), "p90": round(percentile(&chroma, 0.9), 1)},
        "neutral_cast": {"a": round(neutral.iter().map(|c| c[1]).sum::<f64>() / nn, 2), "b": round(neutral.iter().map(|c| c[2]).sum::<f64>() / nn, 2), "share%": round(neutral.len() as f64 / n * 100.0, 1)},
        "hist16%": bins.map(|b| round(b as f64 / n * 100.0, 1)),
    })
}

/// What `glance` reports, in order: luma percentiles, clipping, the cast of near-neutral pixels and chroma p90.
pub const GLANCE: [&str; 9] = ["p5", "p50", "p95", "p99", "white%", "black%", "cast_a", "cast_b", "chroma90"];

/// The measures `reading` knows.
pub const MEASURES: [&str; 14] = ["p1", "p5", "p25", "p50", "p75", "p95", "p99", "mean", "white%", "black%", "cast_a", "cast_b", "chroma50", "chroma90"];

/// One number from `measure` by its short name.
pub fn reading(m: &Value, name: &str) -> Option<f64> {
    match name {
        "p1" | "p5" | "p25" | "p50" | "p75" | "p95" | "p99" | "mean" => m["luma"][name].as_f64(),
        "white%" => m["clip%"]["white"].as_f64(),
        "black%" => m["clip%"]["black"].as_f64(),
        "cast_a" => m["neutral_cast"]["a"].as_f64(),
        "cast_b" => m["neutral_cast"]["b"].as_f64(),
        "chroma50" => m["chroma"]["p50"].as_f64(),
        "chroma90" => m["chroma"]["p90"].as_f64(),
        _ => None,
    }
}

/// The pixels a cast is read from: the mid tones near neutral, or when they're under 2% of the frame, the least
/// colourful fifth of the mid tones.
pub fn neutrals(shot: &Shot) -> Vec<usize> {
    let labs: Vec<[f64; 3]> = shot.rgba.as_chunks::<4>().0.iter().map(lab).collect();
    let chroma = |i: usize| labs[i][1].hypot(labs[i][2]);
    let mut mid: Vec<usize> = (0..labs.len()).filter(|&i| (20.0..90.0).contains(&labs[i][0])).collect();
    let near: Vec<usize> = mid.iter().copied().filter(|&i| chroma(i) < 12.0).collect();
    if near.len() as f64 >= labs.len() as f64 * 0.02 {
        return near;
    }
    mid.sort_by(|a, b| chroma(*a).total_cmp(&chroma(*b)));
    mid.truncate(mid.len().div_ceil(5));
    mid
}

/// The mean a* and b* of `pixels` in `shot`.
pub fn cast_at(shot: &Shot, pixels: &[usize]) -> (f64, f64) {
    let px = shot.rgba.as_chunks::<4>().0;
    let labs: Vec<[f64; 3]> = pixels.iter().filter_map(|&i| px.get(i)).map(lab).collect();
    let n = labs.len().max(1) as f64;
    (labs.iter().map(|c| c[1]).sum::<f64>() / n, labs.iter().map(|c| c[2]).sum::<f64>() / n)
}

/// Whether enough of the frame is near neutral for its cast to mean anything.
pub fn has_cast(m: &Value) -> bool {
    m["neutral_cast"]["share%"].as_f64().unwrap_or(0.0) >= 2.0
}

fn glanced(m: &Value, name: &str) -> Option<f64> {
    match name {
        "cast_a" | "cast_b" => has_cast(m).then(|| reading(m, name).map(|v| round(v, 1))).flatten(),
        _ => reading(m, name),
    }
}

/// The few numbers worth checking after every change, from `measure`, named as in `GLANCE`; clipping only when there
/// is some.
pub fn glance(m: &Value) -> Value {
    let pairs = GLANCE.iter().filter_map(|k| glanced(m, k).filter(|v| !k.ends_with('%') || *v > 0.0).map(|v| ((*k).to_string(), json!(v))));
    Value::Object(pairs.collect())
}

/// The least change in each `glance` number an edit's answer shows: the precision each is reported at, so a tenth of a
/// stop of exposure still shows.
fn noise(name: &str) -> f64 {
    match name {
        "white%" | "black%" => 0.01,
        "cast_a" | "cast_b" | "chroma90" => 0.1,
        _ => 0.001,
    }
}

fn places(name: &str) -> i32 {
    match name {
        "white%" | "black%" => 2,
        "cast_a" | "cast_b" | "chroma90" => 1,
        _ => 3,
    }
}

/// What moved between two measurements: in `delta` the glance numbers that moved, by how much; in `now` the current
/// value where a condition began or ended (clipping appearing or clearing, a cast becoming readable or not, as null).
pub fn changes(was: &Value, now: &Value) -> (Map<String, Value>, Map<String, Value>) {
    let mut delta = Map::new();
    let mut current = Map::new();
    for k in GLANCE {
        match (glanced(was, k), glanced(now, k)) {
            (Some(x), Some(y)) => {
                if (y - x).abs() >= noise(k) {
                    delta.insert(k.into(), json!(round(y - x, places(k))));
                }
                if k.ends_with('%') && (x == 0.0) != (y == 0.0) {
                    current.insert(k.into(), json!(y));
                }
            }
            (Some(_), None) => {
                current.insert(k.into(), Value::Null);
            }
            (None, Some(y)) => {
                current.insert(k.into(), json!(y));
            }
            (None, None) => {}
        }
    }
    (delta, current)
}

/// `glance` as a table row, in `GLANCE` order.
fn glance_row(m: &Value) -> Vec<Value> {
    GLANCE.iter().map(|k| glanced(m, k).map_or(Value::Null, |v| json!(v))).collect()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Look {
    photo: Option<String>,
    max_edge: Option<u32>,
    region: Option<[f64; 4]>,
    before: Option<bool>,
    quality: Option<u8>,
    scopes: Option<Vec<String>>,
    #[serde(rename = "try")]
    tries: Option<Vec<Candidate>>,
    image: Option<String>,
    from: Option<Value>,
}

/// One alternative a preview tries: controls to set and film fields to change, as set_controls and set_film take them.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Candidate {
    values: Option<serde_json::Map<String, Value>>,
    film: Option<Value>,
    curve: Option<Value>,
}

const MOST_TRIES: usize = 8;

/// The photo now (cell 0) beside each alternative (cells 1 on), drawn without saving anything, with what each
/// changes from now in words and numbers.
async fn trials(hub: &Rc<Hub>, driver: &Driver, cx: &mut AsyncApp, p: Look) -> Answer {
    let tries = p.tries.unwrap_or_default();
    if tries.is_empty() || tries.len() > MOST_TRIES {
        return Err(format!("try takes 1 to {MOST_TRIES} alternatives"));
    }
    if p.region.is_some() || p.before.is_some() {
        return Err("try shows whole frames of the photo now; region and before are for one preview".into());
    }
    let stocks = driver.read(cx, |a, _| a.stocks.clone()).ok_or("Filmlab's window is gone")?;
    let held = hub.hold(driver, cx, p.photo.as_deref()).await?;
    let now = State::now(&held, driver, cx)?;
    let facts = held.read(driver, cx, |e| e.facts.cloned()).flatten().ok_or(NO_PHOTO)?;
    let mut states = vec![now.clone()];
    let mut clamped = Vec::new();
    for (i, t) in tries.iter().enumerate() {
        let (settings, notes) = match &t.values {
            Some(values) => edit::with_values(&now.settings, values, &facts).map_err(|e| format!("try {}: {e}", i + 1))?,
            None => (now.settings.clone(), Vec::new()),
        };
        let (settings, notes) = match &t.curve {
            Some(shape) => {
                let curved = curve::with_curve(&settings, shape, &facts).map_err(|e| format!("try {}: {e}", i + 1))?;
                (curved.settings, notes.into_iter().chain(curved.tally.clamped).collect())
            }
            None => (settings, notes),
        };
        let film = match &t.film {
            Some(fields) => {
                edit::bare_film(fields).map_err(|e| format!("try {}: {e}", i + 1))?;
                Some(edit::film_change(fields, now.film.as_ref(), &stocks).map_err(|e| format!("try {}: {e}", i + 1))?.film)
            }
            None => now.film.clone(),
        };
        clamped.extend(notes.into_iter().map(|n| format!("try {}: {n}", i + 1)));
        states.push(State { settings, film });
    }
    let (cols, rows) = grid(states.len(), 1.5);
    let fitted = (cols as f64 * (CELL + GAP)).max(rows as f64 * (CELL / 1.5 + GAP)) + GAP;
    let edge = p.max_edge.map_or((fitted.round() as u32).clamp(256, 1024), |e| e.clamp(256, 2048));
    let (cw, ch) = cell(cols, rows, 1.5, edge);
    let mut shots = Vec::new();
    for state in &states {
        shots.push(frame(hub, &held, driver, cx, state, cw.max(ch).ceil() as u32).await?);
    }
    let mut curves = Vec::new();
    for state in &states[1..] {
        curves.push(match curve::moved(&now.settings, &state.settings) {
            true => Some((state.settings["curve"].clone(), input_frame(hub, &held, driver, cx, state, MEASURED).await?)),
            false => None,
        });
    }
    let digest = held.id(driver, cx).map(|id| now.digest(&id));
    drop(held);
    let quality = p.quality.unwrap_or(80).clamp(30, 95);
    let (image, out) = cx
        .background_spawn(async move {
            let base = &shots[0];
            let rows: Vec<Value> = shots[1..]
                .iter()
                .enumerate()
                .map(|(i, shot)| {
                    let mut row = super::compare::effect(base, shot, false);
                    row["#"] = json!(i + 1);
                    if let Some((shape, input)) = &curves[i]
                        && let Ok(Some(block)) = curve::block(&now.settings["curve"], shape, &curve::Intake::of(input))
                    {
                        row["curve"] = block;
                    }
                    row
                })
                .collect();
            let cells: Vec<Option<Arc<Shot>>> = shots.iter().cloned().map(Some).collect();
            let (picture, cols, rows_n) = sheet(&cells, edge, 0);
            (jpeg(&picture, quality), strip(json!({"state": digest, "grid": [cols, rows_n], "tries": rows, "clamped": clamped})))
        })
        .await;
    Ok(Output::json(out).with_image(image?))
}

pub async fn preview(hub: &Rc<Hub>, driver: &Driver, cx: &mut AsyncApp, value: Value) -> Answer {
    let p: Look = args(value)?;
    if p.tries.is_some() {
        return trials(hub, driver, cx, p).await;
    }
    let held = hub.hold(driver, cx, p.photo.as_deref()).await?;
    let shot = render(&held, driver, cx, p.max_edge.unwrap_or(768).clamp(64, 2048), p.region, &Pick::of(p.before.unwrap_or(false)), true).await?;
    let before = p.before.unwrap_or(false);
    let frame = held.read(driver, cx, |e| e.facts.map(|f| frame_size(f, if before { e.before.as_ref().unwrap_or(&e.settings) } else { &e.settings }, true))).flatten().transpose()?;
    drop(held);
    let quality = p.quality.unwrap_or(80).clamp(30, 95);
    let region = p.region.map(ordered).transpose()?;
    let (image, stats) = cx
        .background_spawn(async move {
            let mut stats = brief(&shot);
            if let (Some([x0, _, x1, _]), Some([w, _])) = (region, frame) {
                stats["scale"] = json!(round((x1 - x0) * w / f64::from(shot.width), 2));
            }
            (picture(&shot, region.is_some(), quality), stats)
        })
        .await;
    Ok(Output::json(stats).with_image(image?))
}

/// A preview's image: lossless for a region, which is looked at for the grain, edges and gradients JPEG blurs or bands
/// (a model is charged by an image's size, not its bytes), JPEG at `quality` for a whole frame.
fn picture(shot: &Shot, lossless: bool, quality: u8) -> Result<Vec<u8>, String> {
    if lossless { png(shot) } else { jpeg(shot, quality) }
}

/// The shot as a lossless PNG.
pub fn png(shot: &Shot) -> Result<Vec<u8>, String> {
    let rgb: Vec<u8> = shot.rgba.as_chunks::<4>().0.iter().flat_map(|p| [p[0], p[1], p[2]]).collect();
    let mut out = Cursor::new(Vec::new());
    image::ImageEncoder::write_image(image::codecs::png::PngEncoder::new(&mut out), &rgb, shot.width, shot.height, image::ExtendedColorType::Rgb8).map_err(|e| e.to_string())?;
    Ok(out.into_inner())
}

/// An edit's answer, with a JPEG of the photo now for tools that take `preview`: true (768 px) or a long edge. The
/// edit is made by then, so a preview that fails is noted in the answer rather than failing it.
pub async fn attach(held: &Held, driver: &Driver, cx: &mut AsyncApp, mut out: Value, preview: Option<&Value>) -> Answer {
    let edge = match preview {
        Some(Value::Bool(true)) => 768,
        Some(Value::Number(n)) => n.as_u64().unwrap_or(768).clamp(64, 2048) as u32,
        _ => return Ok(Output::json(out)),
    };
    let image = match render(held, driver, cx, edge, None, &Pick::Current, true).await {
        Ok(shot) => cx.background_spawn(async move { jpeg(&shot, 80) }).await,
        Err(e) => Err(e),
    };
    match image {
        Ok(jpeg) => Ok(Output::json(out).with_image(jpeg)),
        Err(e) => {
            out["preview"] = json!(format!("not drawn: {e}"));
            Ok(Output::json(out))
        }
    }
}

struct Traced {
    curve: Value,
    intake: Arc<Shot>,
    region: Option<Shot>,
    ghost: Option<(String, Value)>,
    chart: bool,
}

impl Traced {
    async fn of(hub: &Hub, held: &Held, driver: &Driver, cx: &mut AsyncApp, p: &Look, edge: u32) -> Result<Traced, String> {
        let before = p.before.unwrap_or(false);
        let state = if before { State::opened(held, driver, cx)? } else { State::now(held, driver, cx)? };
        let intake = input_frame(hub, held, driver, cx, &state, MEASURED).await?;
        let region = match p.region {
            Some(area) => {
                let (job, view) = framing(held, driver, cx, edge, Some(area), &Pick::of(before), true).await?;
                Some(draw(job, into_curve(view), cx).await?)
            }
            None => None,
        };
        let ghost = p.from.as_ref().map(|at| super::compare::curve_at(held, driver, cx, at)).transpose()?;
        Ok(Traced { curve: state.settings["curve"].clone(), intake, region, ghost, chart: p.image.is_some() })
    }

    fn read(&self) -> Result<(Value, Option<Vec<u8>>), String> {
        let intake = curve::Intake::of(&self.intake);
        let mapped = curve::Mapped::of(&self.curve)?;
        let area = self.region.as_ref().map(|r| curve::area(&curve::Intake::of(r), &mapped));
        let mut value = curve::scope(&self.curve, &mapped, &intake, area.as_ref())?;
        if let Some((name, _)) = &self.ghost {
            value["ghost"] = json!(name);
        }
        if !self.chart {
            return Ok((value, None));
        }
        let ghost = self.ghost.as_ref().map(|(_, shape)| curve::Mapped::of(shape)).transpose()?;
        let picture = chart::draw(&mapped, &curve::drawn_points(&self.curve)?, ghost.as_ref(), &intake, area.as_ref());
        Ok((value, Some(jpeg(&picture, 90)?)))
    }
}

pub async fn analyze(hub: &Rc<Hub>, driver: &Driver, cx: &mut AsyncApp, value: Value) -> Answer {
    let p: Look = args(value)?;
    if let Some(bad) = p.scopes.iter().flatten().find(|s| !scopes::SCOPES.contains(&s.as_str())) {
        return Err(format!("unknown scope {bad}; scopes: {}", scopes::SCOPES.join(", ")));
    }
    let traced = p.scopes.iter().flatten().any(|s| s == "curve");
    match (&p.image, &p.from) {
        (Some(image), _) if image != "curve" => return Err(format!("analyze draws image \"curve\"; {image} is for compare")),
        (Some(_), _) if !traced => return Err("image \"curve\" draws the curve scope: add \"curve\" to scopes".into()),
        (None, Some(_)) => return Err("analyze takes from only to draw that state as a ghost on image \"curve\"; compare diffs two states".into()),
        _ => {}
    }
    let edge = p.max_edge.unwrap_or(1024).clamp(64, 2048);
    let held = hub.hold(driver, cx, p.photo.as_deref()).await?;
    let others = p.scopes.as_ref().is_none_or(|which| which.iter().any(|s| s != "curve"));
    let shot = match others {
        true => Some(render(&held, driver, cx, edge, p.region, &Pick::of(p.before.unwrap_or(false)), true).await?),
        false => None,
    };
    let traced = match traced {
        true => Some(Traced::of(hub, &held, driver, cx, &p, edge).await?),
        false => None,
    };
    drop(held);
    let region = p.region.is_some();
    let (out, image) = cx
        .background_spawn(async move {
            let mut out = match (p.scopes, &shot) {
                (None, Some(shot)) => measure(shot),
                (which, shot) => {
                    let which = which.unwrap_or_default();
                    let mut out = json!({});
                    if let Some(shot) = shot {
                        out["size"] = json!([shot.width, shot.height]);
                        if which.iter().any(|w| w == "glance") {
                            out["glance"] = glance(&measure(shot));
                        }
                        if let (Value::Object(o), Value::Object(found)) = (&mut out, scopes::scopes(shot, &which, region)) {
                            o.extend(found);
                        }
                    }
                    out
                }
            };
            let image = match traced.map(|t| t.read()).transpose()? {
                Some((curve, image)) => {
                    out["curve"] = curve;
                    image
                }
                None => None,
            };
            Ok::<_, String>((out, image))
        })
        .await?;
    let output = Output::json(out);
    Ok(match image {
        Some(jpeg) => output.with_image(jpeg),
        None => output,
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Many {
    photo: Vec<String>,
    max_edge: Option<u32>,
    before: Option<bool>,
    quality: Option<u8>,
    region: Option<Value>,
}

const MOST: usize = 30;
const GAP: f64 = 4.0;
/// A sheet cell's width when no size is asked for: enough to judge a photo's tone and colour beside the others.
const CELL: f64 = 360.0;
const GROUND: [u8; 4] = [28, 28, 28, 255];

fn many(value: Value) -> Result<Many, String> {
    let mut p: Many = args(value)?;
    if p.region.is_some() {
        return Err("region looks closely at one photo; give photo one id".into());
    }
    let mut seen = std::collections::HashSet::new();
    p.photo.retain(|id| seen.insert(id.clone()));
    if p.photo.is_empty() || p.photo.len() > MOST {
        return Err(format!("give 1 to {MOST} photo ids"));
    }
    Ok(p)
}

/// The columns and rows that place `n` photos of `aspect` nearest a 3:2 sheet.
fn grid(n: usize, aspect: f64) -> (usize, usize) {
    let off = |cols: usize| ((cols as f64 * aspect / n.div_ceil(cols) as f64).ln() - 1.5f64.ln()).abs();
    let cols = (1..=n.max(1)).min_by(|a, b| off(*a).total_cmp(&off(*b))).unwrap_or(1);
    (cols, n.max(1).div_ceil(cols))
}

/// The size of one cell when `cols` x `rows` cells of `aspect` fill a sheet whose long edge is `edge`.
fn cell(cols: usize, rows: usize, aspect: f64, edge: u32) -> (f64, f64) {
    let (c, r, e) = (cols as f64, rows as f64, f64::from(edge));
    if c * aspect >= r {
        let w = (e - GAP * (c + 1.0)) / c;
        (w, w / aspect)
    } else {
        let h = (e - GAP * (r + 1.0)) / r;
        (h * aspect, h)
    }
}

const DIGITS: [u16; 10] = [
    0b111_101_101_101_111,
    0b010_110_010_010_111,
    0b111_001_111_100_111,
    0b111_001_111_001_111,
    0b101_101_111_001_001,
    0b111_100_111_001_111,
    0b111_100_111_101_111,
    0b111_001_001_001_001,
    0b111_101_111_101_111,
    0b111_101_111_001_111,
];

fn fill(canvas: &mut [u8], width: usize, x0: usize, y0: usize, w: usize, h: usize, rgba: [u8; 4]) {
    let height = canvas.len() / 4 / width.max(1);
    for y in y0..(y0 + h).min(height) {
        for x in x0..(x0 + w).min(width) {
            canvas[(y * width + x) * 4..][..4].copy_from_slice(&rgba);
        }
    }
}

/// Writes `n` in small block digits on a dark tab at the cell's corner, so each photo can be found by its number.
fn number(canvas: &mut [u8], width: usize, x0: usize, y0: usize, n: usize, scale: usize) {
    let digits: Vec<usize> = n.to_string().bytes().map(|b| usize::from(b - b'0')).collect();
    fill(canvas, width, x0, y0, scale * (4 * digits.len() + 1), scale * 7, [0, 0, 0, 255]);
    for (i, d) in digits.iter().enumerate() {
        for bit in 0..15 {
            if DIGITS[*d] >> (14 - bit) & 1 == 1 {
                fill(canvas, width, x0 + scale * (1 + 4 * i + bit % 3), y0 + scale * (1 + bit / 3), scale, scale, [255, 255, 255, 255]);
            }
        }
    }
}

/// The photos side by side in a numbered grid within `edge`; a photo that couldn't be drawn leaves its cell empty.
fn sheet(shots: &[Option<Arc<Shot>>], edge: u32, first: usize) -> (Shot, usize, usize) {
    let logs: Vec<f64> = shots.iter().flatten().map(|s| (f64::from(s.width) / f64::from(s.height.max(1))).ln()).collect();
    let aspect = if logs.is_empty() { 1.5 } else { (logs.iter().sum::<f64>() / logs.len() as f64).exp() };
    let (cols, rows) = grid(shots.len(), aspect);
    let (cw, ch) = cell(cols, rows, aspect, edge);
    let width = (cols as f64 * (cw + GAP) + GAP).round() as usize;
    let height = (rows as f64 * (ch + GAP) + GAP).round() as usize;
    let mut canvas: Vec<u8> = GROUND.repeat(width * height);
    let scale = ((ch / 70.0).round() as usize).max(2);
    for (i, shot) in shots.iter().enumerate() {
        let (x0, y0) = (GAP + (i % cols) as f64 * (cw + GAP), GAP + (i / cols) as f64 * (ch + GAP));
        if let Some(shot) = shot {
            let s = (cw / f64::from(shot.width)).min(ch / f64::from(shot.height));
            let (w, h) = ((f64::from(shot.width) * s).round().max(1.0) as u32, (f64::from(shot.height) * s).round().max(1.0) as u32);
            let source = image::RgbaImage::from_raw(shot.width, shot.height, shot.rgba.clone()).unwrap_or_default();
            let fitted = image::imageops::resize(&source, w, h, image::imageops::FilterType::Triangle);
            let (left, top) = ((x0 + (cw - f64::from(w)) / 2.0).round() as usize, (y0 + (ch - f64::from(h)) / 2.0).round() as usize);
            for (y, row) in fitted.as_raw().chunks(w as usize * 4).enumerate() {
                let at = ((top + y) * width + left) * 4;
                if let Some(target) = canvas.get_mut(at..at + row.len()) {
                    target.copy_from_slice(row);
                }
            }
        }
        number(&mut canvas, width, x0.round() as usize, y0.round() as usize, first + i, scale);
    }
    (Shot { rgba: canvas, width: width as u32, height: height as u32 }, cols, rows)
}

/// Several photos in one numbered contact sheet, with each one's numbers in a table.
pub async fn preview_many(hub: &Rc<Hub>, driver: &Driver, cx: &mut AsyncApp, value: Value) -> Answer {
    let p = many(value)?;
    let (cols, rows) = grid(p.photo.len(), 1.5);
    let fitted = (cols as f64 * (CELL + GAP)).max(rows as f64 * (CELL / 1.5 + GAP)) + GAP;
    let edge = match p.max_edge {
        Some(edge) => edge.clamp(256, 2048),
        None => (fitted.round() as u32).clamp(256, 1024),
    };
    let (cw, ch) = cell(cols, rows, 1.5, edge);
    let drawn = frames(hub, driver, cx, &p.photo, cw.max(ch).ceil() as u32, p.before.unwrap_or(false)).await;
    let quality = p.quality.unwrap_or(80).clamp(30, 95);
    let ids = p.photo;
    let (image, out) = cx
        .background_spawn(async move {
            let shots: Vec<Option<Arc<Shot>>> = drawn.iter().map(|d| d.as_ref().ok().cloned()).collect();
            let (picture, cols, rows) = sheet(&shots, edge, 1);
            let mut table = Vec::new();
            let mut errors = Vec::new();
            for (i, (id, d)) in ids.iter().zip(&drawn).enumerate() {
                match d {
                    Ok(shot) => table.push(Value::Array([json!(i + 1), json!(id)].into_iter().chain(glance_row(&measure(shot))).collect())),
                    Err(e) => errors.push(format!("{} {id}: {e}", i + 1)),
                }
            }
            let columns: Vec<&str> = ["#", "photo"].into_iter().chain(GLANCE).collect();
            (jpeg(&picture, quality), strip(json!({"grid": [cols, rows], "columns": columns, "rows": table, "errors": errors})))
        })
        .await;
    Ok(Output::json(out).with_image(image?))
}

/// Several photos' numbers in one table, measured as edits are.
pub async fn analyze_many(hub: &Rc<Hub>, driver: &Driver, cx: &mut AsyncApp, value: Value) -> Answer {
    let p = many(value)?;
    if p.max_edge.is_some() || p.quality.is_some() {
        return Err("several photos are measured at 512 px; max_edge and quality are for one".into());
    }
    let drawn = frames(hub, driver, cx, &p.photo, MEASURED, p.before.unwrap_or(false)).await;
    let ids = p.photo;
    let out = cx
        .background_spawn(async move {
            let mut table = Vec::new();
            let mut errors = Vec::new();
            for (id, d) in ids.iter().zip(&drawn) {
                match d {
                    Ok(shot) => table.push(Value::Array(std::iter::once(json!(id)).chain(glance_row(&measure(shot))).collect())),
                    Err(e) => errors.push(format!("{id}: {e}")),
                }
            }
            let columns: Vec<&str> = std::iter::once("photo").chain(GLANCE).collect();
            strip(json!({"columns": columns, "rows": table, "errors": errors}))
        })
        .await;
    Ok(Output::json(out))
}
