use serde_json::{Map, Value, json};

use super::look::{self, Shot, round};

/// The scopes `analyze` can add, as a colourist reads them.
pub const SCOPES: [&str; 9] = ["glance", "zones", "hues", "skin", "clip", "tail", "wave", "texture", "curve"];

/// The scope numbers fit and watch can take: the colour of a tonal zone and texture. Hue shares and skin follow
/// whichever pixels fall in their range, so moving pixels out of it would read as a fix; they are for looking at.
pub const MEASURES: [&str; 7] = ["sh.a", "sh.b", "mid.a", "mid.b", "hi.a", "hi.b", "texture"];

/// Hue sectors of CIELAB hue angle, named as a viewer would name the colour: name, from, to (degrees).
const SECTORS: [(&str, f64, f64); 6] = [("red", 350.0, 50.0), ("yellow", 50.0, 110.0), ("green", 110.0, 170.0), ("cyan", 170.0, 250.0), ("blue", 250.0, 310.0), ("magenta", 310.0, 350.0)];

/// Tonal zones by code luma: shadows, mid tones, highlights.
const ZONES: [(&str, f64, f64); 3] = [("sh", 0.0, 0.3), ("mid", 0.3, 0.7), ("hi", 0.7, 1.01)];

/// The vectorscope's skin line (the in-phase axis) in BT.709 Cb/Cr from sRGB code values, degrees from +Cb.
const SKIN_LINE: f64 = 123.0;

fn hue(c: &[f64; 3]) -> f64 {
    c[2].atan2(c[1]).to_degrees().rem_euclid(360.0)
}

fn chroma(c: &[f64; 3]) -> f64 {
    c[1].hypot(c[2])
}

fn within(h: f64, from: f64, to: f64) -> bool {
    if from <= to { (from..to).contains(&h) } else { h >= from || h < to }
}

fn share(k: usize, n: usize) -> f64 {
    round(k as f64 / n.max(1) as f64 * 100.0, 1)
}

/// `a - b` in degrees, wrapped to -180..180.
fn turn(a: f64, b: f64) -> f64 {
    (a - b + 180.0).rem_euclid(360.0) - 180.0
}

/// The circular mean of angles in degrees, and how concentrated they are (0 spread evenly, 1 all alike).
fn circular(angles: &[f64]) -> (f64, f64) {
    let n = angles.len().max(1) as f64;
    let (x, y) = angles.iter().fold((0.0, 0.0), |(x, y), a| (x + a.to_radians().cos(), y + a.to_radians().sin()));
    let (x, y) = (x / n, y / n);
    (y.atan2(x).to_degrees().rem_euclid(360.0), x.hypot(y))
}

fn quantile(mut v: Vec<f64>, q: f64) -> f64 {
    v.sort_by(f64::total_cmp);
    look::percentile(&v, q)
}

/// The colour of the shadows, mid tones and highlights, for split toning: each zone's share of the frame, mean luma,
/// mean a* and b* and chroma p90 (so colours that cancel in the mean still show), for zones with 1% of it or more.
fn zones(labs: &[[f64; 3]], luma: &[f64]) -> Value {
    let zone = |lo: f64, hi: f64| {
        let at: Vec<usize> = (0..luma.len()).filter(|&i| (lo..hi).contains(&luma[i])).collect();
        let k = at.len().max(1) as f64;
        let mean = |f: &dyn Fn(usize) -> f64| at.iter().map(|&i| f(i)).sum::<f64>() / k;
        (at.len() * 100 >= labs.len()).then(|| {
            json!({
                "%": share(at.len(), labs.len()),
                "y": round(mean(&|i| luma[i]), 3),
                "a": round(mean(&|i| labs[i][1]), 1),
                "b": round(mean(&|i| labs[i][2]), 1),
                "c90": round(quantile(at.iter().map(|&i| chroma(&labs[i])).collect(), 0.9), 1),
            })
        })
    };
    Value::Object(ZONES.iter().filter_map(|(name, lo, hi)| zone(*lo, *hi).map(|z| ((*name).to_string(), z))).collect())
}

/// Where the colour is, by hue sector (CIELAB hue, chroma 5 and up, L* 10-95): each sector holding half a percent of
/// the frame or more, its share of the frame, mean hue (when its hues agree) and chroma p50 and p90. A summary, not a
/// vectorscope: two different spreads of colour can read alike.
fn hues(labs: &[[f64; 3]]) -> Value {
    let colourful: Vec<&[f64; 3]> = labs.iter().filter(|c| chroma(c) >= 5.0 && (10.0..=95.0).contains(&c[0])).collect();
    let mut out = Map::new();
    for (name, from, to) in SECTORS {
        let px: Vec<&&[f64; 3]> = colourful.iter().filter(|p| within(hue(p), from, to)).collect();
        if px.len() * 200 < labs.len() {
            continue;
        }
        let (h, agree) = circular(&px.iter().map(|p| hue(p)).collect::<Vec<_>>());
        let c: Vec<f64> = px.iter().map(|p| chroma(p)).collect();
        out.insert(name.into(), json!({"%": share(px.len(), labs.len()), "h": (agree >= 0.5).then(|| round(h, 0)), "c50": round(quantile(c.clone(), 0.5), 1), "c90": round(quantile(c, 0.9), 1)}));
    }
    Value::Object(out)
}

/// Skin against the vectorscope's skin line, in Cb/Cr from the frame's code values. Over a region chosen on a face
/// (`region`) every mid-tone pixel with some colour counts; over a whole frame only pixels within 30° of the line,
/// candidates that walls and wood join. dh: the mean angle less the line's (negative toward magenta, positive toward
/// yellow and green); spread: how far most pixels stray from that mean; c50, c90: saturation. Too few pixels, or
/// angles that don't agree, give no angle rather than a wrong one.
fn skin(px: &[[u8; 4]], region: bool) -> Value {
    let cbcr: Vec<(f64, f64)> = px
        .iter()
        .filter_map(|p| {
            let [r, g, b] = [p[0], p[1], p[2]].map(|c| f64::from(c) / 255.0);
            let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
            let (cb, cr) = ((b - y) / 1.8556, (r - y) / 1.5748);
            let angle = cr.atan2(cb).to_degrees().rem_euclid(360.0);
            let wanted = (0.05..=0.95).contains(&y) && cb.hypot(cr) >= 0.02 && (region || turn(angle, SKIN_LINE).abs() <= 30.0);
            wanted.then_some((angle, cb.hypot(cr)))
        })
        .collect();
    let basis = if region { "region" } else { "hue_candidates" };
    if cbcr.len() < 32 {
        return json!({"basis": basis, "%": share(cbcr.len(), px.len()), "support": "low"});
    }
    let angles: Vec<f64> = cbcr.iter().map(|(a, _)| *a).collect();
    let (mean, agree) = circular(&angles);
    let spread = quantile(angles.iter().map(|a| turn(*a, mean).abs()).collect(), 0.9);
    let radius: Vec<f64> = cbcr.iter().map(|(_, r)| *r).collect();
    json!({
        "basis": basis,
        "%": share(cbcr.len(), px.len()),
        "dh": (agree >= 0.5).then(|| round(turn(mean, SKIN_LINE), 1)),
        "spread": (agree >= 0.5).then(|| round(spread, 1)),
        "c50": round(quantile(radius.clone(), 0.5), 3),
        "c90": round(quantile(radius, 0.9), 3),
    })
}

/// Clipping by channel, as pixel counts so a single blown eye-light still shows: R, G and B at the top (254 and up)
/// and at the bottom (1 and under), where any.
fn clip(px: &[[u8; 4]]) -> Value {
    let count = |test: &dyn Fn(u8) -> bool| [0, 1, 2].map(|k| px.iter().filter(|p| test(p[k])).count());
    let (hi, lo) = (count(&|c| c >= 254), count(&|c| c <= 1));
    let mut out = Map::new();
    if hi.iter().any(|n| *n > 0) {
        out.insert("hi".into(), json!(hi));
    }
    if lo.iter().any(|n| *n > 0) {
        out.insert("lo".into(), json!(lo));
    }
    Value::Object(out)
}

/// The highlights' tail, for roll-off and blown areas: luma p99, p99.9 and the brightest pixel, and how many pixels pass
/// .9, .95 and .98.
fn tail(luma: &[f64]) -> Value {
    let mut sorted = luma.to_vec();
    sorted.sort_by(f64::total_cmp);
    let over = |t: f64| sorted.iter().rev().take_while(|v| **v >= t).count();
    json!({"p99": look::percentile(&sorted, 0.99), "p999": look::percentile(&sorted, 0.999), "max": look::percentile(&sorted, 1.0), "over": [over(0.9), over(0.95), over(0.98)]})
}

/// A waveform in eight numbers a strip: the frame cut into 8 vertical strips, left to right, each one's luma p1, p50
/// and p99.
fn wave(luma: &[f64], width: usize) -> Value {
    let strips = (0..8).map(|k| {
        let (from, to) = (k * width / 8, (k + 1) * width / 8);
        let mut strip: Vec<f64> = luma.chunks(width.max(1)).flat_map(|row| row[from.min(row.len())..to.min(row.len())].iter().copied()).collect();
        strip.sort_by(f64::total_cmp);
        json!([look::percentile(&strip, 0.01), look::percentile(&strip, 0.5), look::percentile(&strip, 0.99)])
    });
    Value::Array(strips.collect())
}

/// Texture (edges, sharpening, grain, noise alike) for its overall contrast: the mean absolute Laplacian of luma over
/// its standard deviation, so brightening or adding contrast doesn't count and sharpening, grain or blur do.
pub fn texture(luma: &[f64], width: usize) -> f64 {
    let height = luma.len() / width.max(1);
    if width < 3 || height < 3 {
        return 0.0;
    }
    let mut sum = 0.0;
    for y in 1..height - 1 {
        for x in 1..width - 1 {
            let i = y * width + x;
            sum += (4.0 * luma[i] - luma[i - 1] - luma[i + 1] - luma[i - width] - luma[i + width]).abs();
        }
    }
    let n = luma.len() as f64;
    let mean = luma.iter().sum::<f64>() / n;
    let sd = (luma.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n).sqrt();
    sum / ((width - 2) * (height - 2)) as f64 / sd.max(1e-6)
}

/// The scopes asked for, of display-referred sRGB; `region` when the shot is an area chosen to look at.
pub fn scopes(shot: &Shot, which: &[String], region: bool) -> Value {
    let px = shot.rgba.as_chunks::<4>().0;
    let labs: Vec<[f64; 3]> = px.iter().map(look::lab).collect();
    let luma: Vec<f64> = px.iter().map(look::luma).collect();
    let mut out = Map::new();
    for name in which {
        let value = match name.as_str() {
            "zones" => zones(&labs, &luma),
            "hues" => hues(&labs),
            "skin" => skin(px, region),
            "clip" => clip(px),
            "tail" => tail(&luma),
            "wave" => wave(&luma, shot.width as usize),
            "texture" => json!(round(texture(&luma, shot.width as usize), 3)),
            _ => continue,
        };
        if !value.is_null() && value.as_object().is_none_or(|o| !o.is_empty()) {
            out.insert(name.clone(), value);
        }
    }
    Value::Object(out)
}

/// One scope number by its fit name (`hi.b`, `skin.hue`, `texture`).
pub fn reading(shot: &Shot, name: &str) -> Option<f64> {
    let (scope, field) = name.split_once('.').unwrap_or((name, ""));
    let which = match scope {
        "sh" | "mid" | "hi" => "zones",
        other => other,
    };
    let value = scopes(shot, &[which.to_string()], false);
    let found = &value[which];
    match which {
        "zones" => found[scope][field].as_f64(),
        "texture" => found.as_f64(),
        _ => found[field].as_f64(),
    }
}
