use std::io::Cursor;
use std::rc::Rc;

use filmlab_engine::core;
use gpui_kit::{AppContext, AsyncApp};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Answer, Output, args};
use crate::develop::plans::PREVIEW_EDGE;
use crate::library::ensure_luts;
use crate::mcp::session::{Held, Hub};
use crate::ui::driver::Driver;

pub struct Shot {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

const STAGES: &str = "mcp";

fn ordered(region: [f64; 4]) -> Result<[f64; 4], String> {
    let [a, b, c, d] = region.map(|v| v.clamp(0.0, 1.0));
    let r = [a.min(c), b.min(d), a.max(c), b.max(d)];
    if r[2] - r[0] < 1e-3 || r[3] - r[1] < 1e-3 {
        return Err("region is empty: give [x0, y0, x1, y1] with x1 > x0 and y1 > y0, in 0-1".into());
    }
    Ok(r)
}

/// Renders the held photo off screen: the whole cropped frame within `max_edge`, or `region` (0-1 of the cropped
/// frame) at up to full resolution.
pub async fn render(held: &Held, driver: &Driver, cx: &mut AsyncApp, max_edge: u32, region: Option<[f64; 4]>, before: bool) -> Result<Shot, String> {
    let region = region.map(ordered).transpose()?;
    let probe = held.render_job(driver, cx, before, false).await?;
    let size: [f64; 2] = serde_json::from_str(&core::frame_size(&probe.facts.to_string(), &probe.settings.to_string(), true)?).map_err(|e| e.to_string())?;
    let [w, h] = size;
    let edge = f64::from(max_edge);
    let (view, full) = match region {
        None => (json!({"maxW": max_edge, "maxH": max_edge, "stages": STAGES, "applyCrop": true}), false),
        Some([x0, y0, x1, y1]) => {
            let (rw, rh) = ((x1 - x0) * w, (y1 - y0) * h);
            let s = (edge / rw.max(rh)).min(1.0);
            let (fw, fh) = ((w * s).round().max(1.0), (h * s).round().max(1.0));
            let window = [(x0 * fw).round(), (y0 * fh).round(), (rw * s).round().max(1.0), (rh * s).round().max(1.0)];
            let window = [window[0], window[1], window[2].min(fw - window[0]), window[3].min(fh - window[1])];
            (json!({"maxW": fw as usize, "maxH": fh as usize, "stages": STAGES, "applyCrop": true, "window": window.map(|v| v as usize)}), fw.max(fh) > PREVIEW_EDGE)
        }
    };
    let job = if full && !probe.full { held.render_job(driver, cx, before, true).await? } else { probe };
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
        let frame = crate::perf::interactive(|| engine.render(&job.key, &job.settings.to_string(), &view.to_string()));
        engine.forget_view(&job.key, STAGES);
        let mut frame = frame?;
        Ok(Shot { width: frame.width(), height: frame.height(), rgba: frame.take_rgba() })
    })
    .await
}

pub fn jpeg(shot: &Shot, quality: u8) -> Result<Vec<u8>, String> {
    let rgb: Vec<u8> = shot.rgba.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
    let mut out = Cursor::new(Vec::new());
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality)
        .encode(&rgb, shot.width, shot.height, image::ExtendedColorType::Rgb8)
        .map_err(|e| e.to_string())?;
    Ok(out.into_inner())
}

fn luma(p: &[u8]) -> f64 {
    (0.2126 * f64::from(p[0]) + 0.7152 * f64::from(p[1]) + 0.0722 * f64::from(p[2])) / 255.0
}

fn percentile(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let i = ((sorted.len() - 1) as f64 * q).round() as usize;
    (sorted[i] * 1000.0).round() / 1000.0
}

fn round(v: f64, places: i32) -> f64 {
    let f = 10f64.powi(places);
    (v * f).round() / f
}

/// The few numbers that tell whether an edit worked: tones, clipping.
fn brief(shot: &Shot) -> Value {
    let mut l: Vec<f64> = shot.rgba.chunks_exact(4).map(luma).collect();
    l.sort_by(f64::total_cmp);
    let n = l.len().max(1) as f64;
    let white = shot.rgba.chunks_exact(4).filter(|p| p[..3].iter().any(|&c| c >= 254)).count() as f64 / n;
    let black = shot.rgba.chunks_exact(4).filter(|p| p[..3].iter().all(|&c| c <= 1)).count() as f64 / n;
    json!({"size": [shot.width, shot.height], "luma": {"p1": percentile(&l, 0.01), "p50": percentile(&l, 0.5), "p99": percentile(&l, 0.99)}, "clip%": {"white": round(white * 100.0, 2), "black": round(black * 100.0, 2)}})
}

fn linear(c: u8) -> f64 {
    let v = f64::from(c) / 255.0;
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

fn lab(p: &[u8]) -> [f64; 3] {
    let (r, g, b) = (linear(p[0]), linear(p[1]), linear(p[2]));
    let x = (0.4124 * r + 0.3576 * g + 0.1805 * b) / 0.95047;
    let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    let z = (0.0193 * r + 0.1192 * g + 0.9505 * b) / 1.08883;
    let f = |t: f64| if t > 216.0 / 24389.0 { t.cbrt() } else { (24389.0 / 27.0 * t + 16.0) / 116.0 };
    let (fx, fy, fz) = (f(x), f(y), f(z));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

/// Tones, clipping, colour and the cast of near-neutral pixels, from display-referred sRGB.
fn measure(shot: &Shot) -> Value {
    let px: Vec<&[u8]> = shot.rgba.chunks_exact(4).collect();
    let n = px.len().max(1) as f64;
    let mut l: Vec<f64> = px.iter().map(|p| luma(p)).collect();
    l.sort_by(f64::total_cmp);
    let mut bins = [0usize; 16];
    for v in &l {
        bins[((v * 16.0) as usize).min(15)] += 1;
    }
    let labs: Vec<[f64; 3]> = px.iter().map(|p| lab(p)).collect();
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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Look {
    photo: Option<String>,
    max_edge: Option<u32>,
    region: Option<[f64; 4]>,
    before: Option<bool>,
    quality: Option<u8>,
}

pub async fn preview(hub: &Rc<Hub>, driver: &Driver, cx: &mut AsyncApp, value: Value) -> Answer {
    let p: Look = args(value)?;
    let held = hub.hold(driver, cx, p.photo.as_deref()).await?;
    let shot = render(&held, driver, cx, p.max_edge.unwrap_or(768).clamp(64, 2048), p.region, p.before.unwrap_or(false)).await?;
    drop(held);
    let quality = p.quality.unwrap_or(80).clamp(30, 95);
    let (image, stats) = cx.background_spawn(async move { (jpeg(&shot, quality), brief(&shot)) }).await;
    Ok(Output::json(stats).with_image(image?))
}

/// A JPEG of the current edit for tools that take `preview`: true (768 px) or a long edge.
pub async fn attach(held: &Held, driver: &Driver, cx: &mut AsyncApp, output: Output, preview: Option<&Value>) -> Answer {
    let edge = match preview {
        Some(Value::Bool(true)) => 768,
        Some(Value::Number(n)) => n.as_u64().unwrap_or(768).clamp(64, 2048) as u32,
        _ => return Ok(output),
    };
    let shot = render(held, driver, cx, edge, None, false).await?;
    let image = cx.background_spawn(async move { jpeg(&shot, 80) }).await?;
    Ok(output.with_image(image))
}

pub async fn analyze(hub: &Rc<Hub>, driver: &Driver, cx: &mut AsyncApp, value: Value) -> Answer {
    let p: Look = args(value)?;
    let held = hub.hold(driver, cx, p.photo.as_deref()).await?;
    let shot = render(&held, driver, cx, p.max_edge.unwrap_or(1024).clamp(64, 2048), p.region, p.before.unwrap_or(false)).await?;
    drop(held);
    Ok(Output::json(cx.background_spawn(async move { measure(&shot) }).await))
}
