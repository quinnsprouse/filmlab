use filmlab_engine::core::Commands;
use filmlab_engine::tone_curve::{Curve, Curves, on_curve};
use serde_json::{Map, Value, json};

use super::app::strip;
use super::look::{self, Shot};
use crate::develop::POINT_CURVES;

pub const CHANNELS: [&str; 4] = ["rgb", "red", "green", "blue"];
const TITLES: [&str; 4] = ["RGB", "Red", "Green", "Blue"];
const NEAR: f64 = 3.0;
const TONES: [usize; 7] = [32, 64, 96, 128, 160, 192, 224];
const GREYS: [(&str, f64); 3] = [("sh", 0.15), ("mid", 0.5), ("hi", 0.85)];
const ZONES: [(&str, f64); 3] = [("sh", 0.1), ("mid", 0.5), ("hi", 0.9)];
const REACH: f64 = 12.0;
const LUMA: [f64; 3] = [0.2126, 0.7152, 0.0722];
const SHAPE: &str = "curve is an object: rgb, red, green or blue as [[in, out], ...], {\"in\": out} or null, or preset alone; levels 0-255 (guide curves)";

pub type Levels = [f64; 256];
pub type Point = [f64; 2];
type Tint = [f64; 2];
pub type Handles = (usize, Vec<Point>);

fn level(v: f64) -> i64 {
    (v * 255.0).round() as i64
}

fn stored(curve: &Value, channel: usize) -> Vec<Point> {
    curve[POINT_CURVES[channel]].as_array().map(|a| a.iter().filter_map(|p| Some([p["x"].as_f64()?, p["y"].as_f64()?])).collect()).unwrap_or_default()
}

fn drawn(curve: &Value, channel: usize) -> Vec<Point> {
    let points = stored(curve, channel);
    if points.len() < 2 { vec![[0.0, 0.0], [1.0, 1.0]] } else { points }
}

fn in_use(curve: &Value) -> Result<[bool; 4], String> {
    let c = Curves::of(&curve.to_string())?;
    Ok([&c.master, &c.channels[0], &c.channels[1], &c.channels[2]].map(|c| !c.is_identity()))
}

pub fn moved(a: &Value, b: &Value) -> bool {
    a["curve"] != b["curve"]
}

pub fn points(curve: &Value, all: bool) -> Result<Map<String, Value>, String> {
    let used = in_use(curve)?;
    Ok(CHANNELS
        .iter()
        .enumerate()
        .filter(|(i, _)| all || used[*i])
        .map(|(i, name)| ((*name).to_string(), json!(drawn(curve, i).iter().map(|[x, y]| [level(*x), level(*y)]).collect::<Vec<_>>())))
        .collect())
}

fn run(commands: &mut Commands, name: &str, params: Value) -> Result<Value, String> {
    serde_json::from_str(&commands.run(name, &params.to_string())?).map_err(|e| e.to_string())
}

fn opened(settings: &Value, facts: &Value) -> Result<Commands, String> {
    let mut commands = Commands::new();
    commands.open(&facts.to_string(), &settings.to_string())?;
    Ok(commands)
}

fn presets(commands: &mut Commands) -> Result<Value, String> {
    Ok(run(commands, "curve.presets", json!({}))?["result"].take())
}

/// The tone curves the user saved (profiles that keep only the Tone curve), by name, as the curve menu lists them.
fn saved() -> Vec<(String, Value)> {
    crate::ui::rail::store::load().into_iter().filter(|p| p.is_curve()).map(|p| (p.name, p.partial["curve"].clone())).collect()
}

pub fn summary(settings: &Value, facts: Option<&Value>, all: bool) -> Option<Value> {
    let curve = &settings["curve"];
    let shaped = in_use(curve).ok()?.iter().any(|u| *u);
    if !shaped && !all {
        return None;
    }
    let mut out = points(curve, all).ok()?;
    let mine = saved().into_iter().find(|(_, c)| *c == *curve).map(|(name, _)| name);
    let preset = mine.or_else(|| facts.filter(|_| shaped).and_then(|f| presets(&mut opened(settings, f).ok()?).ok()).and_then(|p| p["current"].as_str().map(str::to_string)));
    if let Some(name) = preset {
        out.insert("preset".into(), json!(name));
    }
    Some(Value::Object(out))
}

fn fields(curve: &Value) -> Result<&Map<String, Value>, String> {
    let Value::Object(fields) = curve else { return Err(SHAPE.into()) };
    if fields.is_empty() {
        return Err(SHAPE.into());
    }
    if fields.contains_key("preset") && fields.len() > 1 {
        return Err("preset sets all four point curves; give it alone, without channels".into());
    }
    match fields.keys().find(|k| *k != "preset" && !CHANNELS.contains(&k.as_str())).map(String::as_str) {
        None => Ok(fields),
        Some("regions") => Err("the regions are values: curve.highlights, curve.lights, curve.darks, curve.shadows and the splits".into()),
        Some("master") => Err("the master curve is rgb here".into()),
        Some(other) => Err(format!("unknown curve channel {other}; rgb, red, green, blue or preset")),
    }
}

#[derive(Default)]
pub struct Tally {
    pub clamped: Vec<String>,
    pub set: Map<String, Value>,
}

fn output(name: &str, at: f64, wanted: f64, tally: &mut Tally) -> Result<f64, String> {
    if !wanted.is_finite() {
        return Err(format!("curve.{name}@{}: give a level 0-255, or a relative string like \"+8\"", at.round()));
    }
    let v = wanted.clamp(0.0, 255.0);
    if (v - wanted).abs() > 1e-9 {
        tally.clamped.push(format!("curve.{name}@{} to {}", at.round(), v.round()));
    }
    Ok(v)
}

fn inside(name: &str, raw: f64) -> Result<f64, String> {
    match (0.0..=255.0).contains(&raw) {
        true => Ok(raw),
        false => Err(format!("curve.{name}: input {raw} is outside 0-255")),
    }
}

fn anchored(mut points: Vec<Point>) -> Vec<Point> {
    if points.first().is_none_or(|p| p[0] > 0.0) {
        points.insert(0, [0.0, 0.0]);
    }
    if points.last().is_none_or(|p| p[0] < 255.0) {
        points.push([255.0, 255.0]);
    }
    points
}

fn replaced(name: &str, list: &[Value], tally: &mut Tally) -> Result<Vec<Point>, String> {
    let mut points = Vec::with_capacity(list.len() + 2);
    for pair in list {
        let Some([Some(x), Some(y)]) = pair.as_array().filter(|p| p.len() == 2).map(|p| [p[0].as_f64(), p[1].as_f64()]) else {
            return Err(format!("curve.{name}: each point is [in, out], numbers 0-255; got {pair}"));
        };
        let x = inside(name, x)?;
        points.push([x, output(name, x, y, tally)?]);
    }
    points.sort_by(|a, b| a[0].total_cmp(&b[0]));
    if let Some(w) = points.windows(2).find(|w| w[1][0] - w[0][0] < 1.0) {
        return Err(format!("curve.{name}: inputs {} and {} are the same level; give one point per input", w[0][0], w[1][0]));
    }
    Ok(anchored(points))
}

fn relative(name: &str, at: f64, wanted: &Value, from: f64) -> Result<(f64, bool), String> {
    match wanted {
        Value::Number(n) => Ok((n.as_f64().unwrap_or(f64::NAN), false)),
        Value::String(t) => {
            let t = t.trim();
            let n: f64 = t.parse().map_err(|_| format!("curve.{name}@{}: \"{t}\" isn't a level; use 72 (absolute) or \"+8\" / \"-8\" (relative)", at.round()))?;
            Ok(if t.starts_with(['+', '-']) { (from + n, true) } else { (n, false) })
        }
        _ => Ok((f64::NAN, false)),
    }
}

fn reshaped(name: &str, current: &[Point], levels: &Map<String, Value>, tally: &mut Tally) -> Result<(Vec<Point>, Option<Point>), String> {
    let mut asked: Vec<(f64, &Value)> = Vec::with_capacity(levels.len());
    for (key, wanted) in levels {
        let at: f64 = key.trim().parse().map_err(|_| format!("curve.{name}: \"{key}\" isn't an input level; keys are levels 0-255, as {{\"64\": 72}}"))?;
        asked.push((inside(name, at)?, wanted));
    }
    if asked.is_empty() {
        return Err(format!("curve.{name} is empty; give {{\"64\": 72}} or {{\"64\": \"+8\"}}"));
    }
    asked.sort_by(|a, b| a.0.total_cmp(&b.0));
    if let Some(w) = asked.windows(2).find(|w| w[1].0 - w[0].0 <= NEAR) {
        return Err(format!("curve.{name}: inputs {} and {} are within {NEAR} levels, so they'd move one point; give one", w[0].0, w[1].0));
    }
    let lifted = |x: f64| on_curve(current, x / 255.0) * 255.0;
    let mut points: Vec<Point> = current.iter().map(|[x, y]| [x * 255.0, y * 255.0]).collect();
    if points.first().is_none_or(|p| p[0] > 0.0) {
        points.insert(0, [0.0, lifted(0.0)]);
    }
    if points.last().is_none_or(|p| p[0] < 255.0) {
        points.push([255.0, lifted(255.0)]);
    }
    let mut moved: Vec<f64> = Vec::new();
    let mut last = None;
    for (at, wanted) in &asked {
        let near = points.iter().enumerate().filter(|(_, p)| (p[0] - at).abs() <= NEAR).min_by(|a, b| (a.1[0] - at).abs().total_cmp(&(b.1[0] - at).abs())).map(|(i, _)| i);
        let index = match near {
            Some(i) => i,
            None => {
                let i = points.iter().position(|p| p[0] > *at).unwrap_or(points.len());
                points.insert(i, [*at, lifted(*at)]);
                i
            }
        };
        let x = points[index][0];
        if moved.iter().any(|m| (m - x).abs() < 1e-9) {
            return Err(format!("curve.{name}: two inputs move the point at {}; give one", x.round()));
        }
        moved.push(x);
        let (y, by) = relative(name, *at, wanted, points[index][1])?;
        points[index][1] = output(name, *at, y, tally)?;
        if by {
            tally.set.insert(format!("curve.{name}@{}", at.round()), json!(points[index][1].round() as i64));
        }
        last = Some(points[index]);
    }
    Ok((points, last.filter(|_| asked.len() == 1)))
}

fn shaped(channel: usize, current: &[Point], wanted: &Value, tally: &mut Tally) -> Result<(Vec<Point>, String), String> {
    let (name, title) = (CHANNELS[channel], TITLES[channel]);
    match wanted {
        Value::Null => Ok((Vec::new(), format!("{title} curve reset"))),
        Value::Array(list) => Ok((replaced(name, list, tally)?, format!("{title} curve"))),
        Value::Object(levels) => {
            let (points, one) = reshaped(name, current, levels, tally)?;
            let label = one.map_or_else(|| format!("{title} curve"), |[x, y]| format!("{title} curve {} → {}", x.round(), y.round()));
            Ok((points, label))
        }
        _ => Err(format!("curve.{name} is [[in, out], ...], {{\"in\": out}} or null")),
    }
}

pub fn check(curve: &Value) -> Result<(), String> {
    let fields = fields(curve)?;
    if let Some(name) = fields.get("preset") {
        return name.as_str().map(|_| ()).ok_or_else(|| "preset is a name: Linear, Medium Contrast, Strong Contrast or a saved curve".into());
    }
    for (name, wanted) in fields {
        let channel = CHANNELS.iter().position(|c| c == name).unwrap_or(0);
        shaped(channel, &[[0.0, 0.0], [1.0, 1.0]], wanted, &mut Tally::default())?;
    }
    Ok(())
}

pub struct Curved {
    pub settings: Value,
    pub tally: Tally,
    pub label: String,
}

pub fn with_curve(settings: &Value, curve: &Value, facts: &Value) -> Result<Curved, String> {
    let fields = fields(curve)?;
    let mut commands = opened(settings, facts)?;
    if let Some(wanted) = fields.get("preset") {
        let listed = presets(&mut commands)?;
        let names: Vec<&str> = listed["presets"].as_array().map(|a| a.iter().filter_map(|p| p["name"].as_str()).collect()).unwrap_or_default();
        let asked = wanted.as_str().unwrap_or_default().trim();
        if let Some(name) = names.iter().find(|n| n.eq_ignore_ascii_case(asked)) {
            let out = run(&mut commands, "curve.applyPreset", json!({"name": name}))?;
            return Ok(Curved { settings: out["settings"].clone(), tally: Tally::default(), label: format!("Point curve {name}") });
        }
        let mine = saved();
        let Some((name, curve)) = mine.iter().find(|(n, _)| n.eq_ignore_ascii_case(asked)) else {
            let all: Vec<&str> = names.into_iter().chain(mine.iter().map(|(n, _)| n.as_str())).collect();
            return Err(format!("no curve preset \"{asked}\"; presets: {}", all.join(", ")));
        };
        let mut out = settings.clone();
        out["curve"] = curve.clone();
        return Ok(Curved { settings: out, tally: Tally::default(), label: format!("Tone curve {name}") });
    }
    let mut tally = Tally::default();
    let mut labels = Vec::new();
    let mut out = settings.clone();
    for (name, wanted) in fields {
        let channel = CHANNELS.iter().position(|c| c == name).unwrap_or(0);
        let (points, label) = shaped(channel, &drawn(&settings["curve"], channel), wanted, &mut tally)?;
        let unit: Vec<Point> = points.iter().map(|[x, y]| [x / 255.0, y / 255.0]).collect();
        out = run(&mut commands, "develop.curve", json!({"channel": POINT_CURVES[channel], "points": unit}))?["settings"].take();
        labels.push(label);
    }
    let label = match labels.as_slice() {
        [one] => one.clone(),
        _ => "Tone curve".into(),
    };
    Ok(Curved { settings: out, tally, label })
}

#[derive(Clone, Copy)]
pub struct Knob {
    channel: usize,
    input: f64,
}

impl Knob {
    pub fn of(control: &str) -> Option<Result<Knob, String>> {
        let (channel, at) = control.trim().strip_prefix("curve.")?.split_once('@')?;
        let Some(index) = CHANNELS.iter().position(|c| *c == channel) else { return Some(Err(format!("{control}: the channel is rgb, red, green or blue"))) };
        Some(at.trim().parse().map_err(|_| format!("{control}: give an input level 0-255, as curve.rgb@128")).and_then(|v| inside(channel, v)).map(|input| Knob { channel: index, input }))
    }

    pub fn value(&self, settings: &Value) -> f64 {
        let points = drawn(&settings["curve"], self.channel);
        let near = points.iter().filter(|p| (p[0] * 255.0 - self.input).abs() <= NEAR).min_by(|a, b| (a[0] * 255.0 - self.input).abs().total_cmp(&(b[0] * 255.0 - self.input).abs()));
        near.map_or_else(|| on_curve(&points, self.input / 255.0) * 255.0, |p| p[1] * 255.0)
    }

    pub fn reach(&self, settings: &Value) -> (f64, f64) {
        let points = drawn(&settings["curve"], self.channel);
        let below = points.iter().rev().find(|p| p[0] * 255.0 < self.input - NEAR).map_or(0.0, |p| p[1] * 255.0);
        let above = points.iter().find(|p| p[0] * 255.0 > self.input + NEAR).map_or(255.0, |p| p[1] * 255.0);
        if below <= above { (below, above) } else { (0.0, 255.0) }
    }

    pub fn with(&self, settings: &Value, facts: &Value, output: f64) -> Result<Curved, String> {
        with_curve(settings, &json!({CHANNELS[self.channel]: {self.input.to_string(): output}}), facts)
    }
}

pub fn command_label(name: &str, params: &Value) -> Option<String> {
    let tab = |channel: &str| match channel {
        "master" | "rgb" => Some("RGB"),
        "red" => Some("Red"),
        "green" => Some("Green"),
        "blue" => Some("Blue"),
        "parametric" => Some("Regions"),
        "point" => Some("Point"),
        "all" => Some("Tone"),
        _ => None,
    };
    match name {
        "develop.curve" => tab(params["channel"].as_str().unwrap_or("master")).map(|t| format!("{t} curve")),
        "curve.reset" => tab(params["channel"].as_str().unwrap_or("all")).map(|t| format!("{t} curve reset")),
        "curve.applyPreset" => params["name"].as_str().map(|n| format!("Point curve {}", n.trim())),
        _ => None,
    }
}

pub struct Mapped {
    curves: [Curve; 4],
    pub regions: Levels,
    pub levels: [Levels; 4],
}

impl Mapped {
    pub fn of(curve: &Value) -> Result<Mapped, String> {
        let c = Curves::of(&curve.to_string())?;
        let rgb = c.master.after(&c.regions);
        let [r, g, b] = c.channels.each_ref().map(|ch| ch.after(&rgb));
        let sample = |curve: &Curve| -> Levels { std::array::from_fn(|i| curve.at(i as f64 / 255.0) * 255.0) };
        let curves = [rgb, r, g, b];
        Ok(Mapped { regions: sample(&c.regions), levels: curves.each_ref().map(sample), curves })
    }

    pub fn at(&self, kind: usize, input: f64) -> f64 {
        self.curves[kind].at(input / 255.0) * 255.0
    }

    pub fn tinted(&self) -> [bool; 3] {
        std::array::from_fn(|c| self.levels[c + 1].iter().zip(&self.levels[0]).any(|(a, b)| (a.round() - b.round()).abs() >= 1.0))
    }

    fn slope(&self, kind: usize, at: f64) -> f64 {
        let (lo, hi) = ((at - REACH).max(0.0), (at + REACH).min(255.0));
        (self.at(kind, hi) - self.at(kind, lo)) / (hi - lo).max(1.0)
    }

    fn grey(&self, g: f64) -> Tint {
        let lab = look::lab_of([1, 2, 3].map(|k| self.curves[k].at(g)));
        [lab[1], lab[2]]
    }

    fn window(&self, kind: usize, lo: usize, hi: usize) -> f64 {
        (self.levels[kind][hi] - self.levels[kind][lo]) / (hi - lo) as f64
    }

    fn differs(&self, other: &Mapped) -> bool {
        self.levels.iter().zip(&other.levels).any(|(a, b)| a.iter().zip(b).any(|(x, y)| (x - y).abs() >= 0.5))
    }
}

pub struct Intake {
    width: usize,
    height: usize,
    pixels: Vec<[u8; 4]>,
    pub hist: [Levels; 4],
}

impl Intake {
    pub fn of(shot: &Shot) -> Intake {
        let pixels: Vec<[u8; 4]> = shot.rgba.as_chunks::<4>().0.iter().map(|p| [(look::luma(p) * 255.0).round() as u8, p[0], p[1], p[2]]).collect();
        let mut hist = [[0.0; 256]; 4];
        for p in &pixels {
            for (k, h) in hist.iter_mut().enumerate() {
                h[usize::from(p[k])] += 1.0;
            }
        }
        Intake { width: shot.width as usize, height: shot.height as usize, pixels, hist }
    }

    fn n(&self) -> f64 {
        self.pixels.len().max(1) as f64
    }

    fn share(&self, kind: usize, lo: usize, hi: usize) -> f64 {
        self.hist[kind][lo..hi.min(256)].iter().sum::<f64>() / self.n()
    }

    fn tone(&self, q: f64) -> f64 {
        let wanted = q * self.n();
        let mut seen = 0.0;
        for (i, count) in self.hist[0].iter().enumerate() {
            seen += count;
            if seen >= wanted {
                return i as f64;
            }
        }
        255.0
    }

    pub fn zones(&self) -> [f64; 3] {
        ZONES.map(|(_, q)| self.tone(q))
    }

    pub fn curved(&self, m: &Mapped) -> Vec<f64> {
        self.pixels.iter().map(|p| (0..3).map(|c| LUMA[c] * m.levels[c + 1][usize::from(p[c + 1])]).sum()).collect()
    }

    pub fn tones_in(&self) -> Vec<f64> {
        self.pixels.iter().map(|p| f64::from(p[0])).collect()
    }

    fn place(&self, kind: usize, lo: usize, hi: usize) -> Option<&'static str> {
        let (mut rows, mut cols, mut n) = ([0usize; 3], [0usize; 3], 0usize);
        for (i, p) in self.pixels.iter().enumerate() {
            if (lo..hi).contains(&usize::from(p[kind])) {
                rows[(i / self.width.max(1)) * 3 / self.height.max(1)] += 1;
                cols[(i % self.width.max(1)) * 3 / self.width.max(1)] += 1;
                n += 1;
            }
        }
        let most = |thirds: [usize; 3], names: [&'static str; 3]| (0..3).find(|&k| thirds[k] * 10 >= n * 6).map(|k| names[k]);
        most(rows, ["mostly top", "mostly middle", "mostly bottom"]).or_else(|| most(cols, ["mostly left", "mostly centre", "mostly right"])).filter(|_| n > 0)
    }
}

fn share_text(share: f64) -> String {
    let percent = share * 100.0;
    if percent >= 10.0 { format!("{percent:.0}%") } else { format!("{percent:.1}%") }
}

fn runs(width: usize, flagged: impl Fn(usize) -> bool) -> Vec<(usize, usize)> {
    let mut out: Vec<(usize, usize)> = Vec::new();
    for a in 0..=255 - width {
        if !flagged(a) {
            continue;
        }
        match out.last_mut() {
            Some(run) if a <= run.1 => run.1 = a + width,
            _ => out.push((a, a + width)),
        }
    }
    out
}

fn within(m: &Mapped, kind: usize, width: usize, (lo, hi): (usize, usize), pick: fn(f64, f64) -> f64) -> f64 {
    (lo..=hi - width).map(|a| m.window(kind, a, a + width)).fold(m.window(kind, lo, lo + width), pick)
}

fn placed(intake: &Intake, kind: usize, lo: usize, hi: usize) -> String {
    intake.place(kind, lo, hi).map_or_else(String::new, |p| format!(", {p}"))
}

#[derive(Clone, Copy, PartialEq)]
enum Flaw {
    Banding,
    Flattened,
    Reversal,
    Edge,
}

struct Found {
    flaw: Flaw,
    run: (usize, usize),
    text: String,
}

impl Found {
    fn inherited(&self, from: &[Found]) -> bool {
        let span = (self.run.1 - self.run.0).max(1);
        from.iter().any(|f| f.flaw == self.flaw && 2 * self.run.1.min(f.run.1).saturating_sub(self.run.0.max(f.run.0)) >= span)
    }
}

fn flaws(m: &Mapped, intake: &Intake, kind: usize) -> Vec<Found> {
    let name = ["", "red ", "green ", "blue "][kind];
    let f = &m.levels[kind];
    let mut out = Vec::new();
    let mut found = |flaw: Flaw, run: (usize, usize), text: String| out.push(Found { flaw, run, text });
    for run in runs(8, |a| m.window(kind, a, a + 8) > 2.5 && intake.share(kind, a, a + 8) >= 0.02) {
        let steepest = within(m, kind, 8, run, f64::max);
        found(Flaw::Banding, run, format!("{name}banding {}–{}: slope {steepest:.1} over {} of pixels{}", run.0, run.1, share_text(intake.share(kind, run.0, run.1)), placed(intake, kind, run.0, run.1)));
    }
    for run in runs(16, |a| m.window(kind, a, a + 16) < 0.35 && intake.share(kind, a, a + 16) >= 0.05) {
        let flattest = within(m, kind, 16, run, f64::min).max(0.0);
        let into = match (f[run.0..=run.1].iter().all(|v| *v >= 254.0), f[run.0..=run.1].iter().all(|v| *v <= 1.0)) {
            (true, _) => " into white",
            (_, true) => " into black",
            _ => "",
        };
        found(Flaw::Flattened, run, format!("{name}flattened {}–{}{into}: slope {flattest:.2} over {} of pixels{}", run.0, run.1, share_text(intake.share(kind, run.0, run.1)), placed(intake, kind, run.0, run.1)));
    }
    let mut a = 0;
    while a < 255 {
        if f[a + 1] >= f[a] - 0.05 {
            a += 1;
            continue;
        }
        let start = a;
        while a < 255 && f[a + 1] < f[a] - 0.05 {
            a += 1;
        }
        let drop = f[start] - f[a];
        if drop >= 1.0 {
            found(Flaw::Reversal, (start, a), format!("{name}reversal {start}–{a}: output falls {drop:.0} levels as input rises, over {} of pixels", share_text(intake.share(kind, start, a + 1))));
        }
    }
    let edge = |a: usize| f[a + 8] >= 239.0 && f[a] < 254.0 && m.window(kind, a, a + 8) > 1.6;
    for run in runs(8, edge) {
        let share = intake.share(kind, run.0, run.1);
        if share >= 0.01 {
            let steepest = within(m, kind, 8, run, f64::max);
            found(Flaw::Edge, run, format!("{name}highlight edge {}–{}: slope {steepest:.1} into white over {} of pixels{}", run.0, run.1, share_text(share), placed(intake, kind, run.0, run.1)));
        }
    }
    out
}

fn notes(m: &Mapped, intake: &Intake) -> Vec<String> {
    let base = flaws(m, intake, 0);
    let tinted = m.tinted();
    let channels = (1..4).filter(|k| tinted[k - 1]).flat_map(|k| flaws(m, intake, k)).filter(|f| !f.inherited(&base));
    let mut out: Vec<String> = base.iter().map(|f| f.text.clone()).chain(channels.map(|f| f.text)).collect();
    let (sh, hi) = (m.grey(GREYS[0].1), m.grey(GREYS[2].1));
    for (axis, k, low, high) in [("b", 1, "cool", "warm"), ("a", 0, "green", "magenta")] {
        if sh[k] * hi[k] < 0.0 && sh[k].abs() >= 1.5 && hi[k].abs() >= 1.5 {
            let word = |v: f64| if v < 0.0 { low } else { high };
            out.push(format!("crossover: shadows {} ({axis} {:+.1}), highlights {} ({axis} {:+.1})", word(sh[k]), sh[k], word(hi[k]), hi[k]));
        }
    }
    let black = m.levels[0][0].round();
    if black >= 20.0 {
        out.push(format!("matte: black at {black}"));
    }
    out
}

fn drift(was: Tint, now: Tint) -> Option<String> {
    let parts: Vec<String> = ["a", "b"]
        .iter()
        .enumerate()
        .filter_map(|(k, axis)| {
            let (w, n) = (look::round(was[k], 1), look::round(now[k], 1));
            let (seen, shows) = (w.abs() >= 0.5, n.abs() >= 0.5);
            if (!seen && !shows) || (w - n).abs() < 0.2 {
                return None;
            }
            Some(match (seen, shows) {
                (true, true) => format!("{axis} {w:+.1}→{n:+.1}"),
                (true, false) => format!("{axis} {w:+.1}→0"),
                _ => format!("{axis} {n:+.1}"),
            })
        })
        .collect();
    (!parts.is_empty()).then(|| parts.join(" "))
}

pub fn block(was: &Value, now: &Value, intake: &Intake) -> Result<Option<Value>, String> {
    let (a, b) = (Mapped::of(was)?, Mapped::of(now)?);
    if !a.differs(&b) {
        return Ok(None);
    }
    let pair = |l: usize| (a.levels[0][l].round() as i64, b.levels[0][l].round() as i64);
    let tones: Map<String, Value> = TONES
        .iter()
        .filter_map(|&l| {
            let (x, y) = pair(l);
            ((x - y).abs() >= 2).then(|| (l.to_string(), json!(format!("{x}→{y}"))))
        })
        .collect();
    let end = |l: usize| {
        let (x, y) = pair(l);
        (x != y).then(|| format!("{x}→{y}"))
    };
    let at = intake.zones();
    let slope: Map<String, Value> = ZONES
        .iter()
        .zip(at)
        .filter_map(|((name, _), t)| {
            let (x, y) = (a.slope(0, t), b.slope(0, t));
            ((x - y).abs() >= 0.03).then(|| ((*name).to_string(), json!(format!("{x:.2}→{y:.2}"))))
        })
        .collect();
    let neutral: Map<String, Value> = GREYS.iter().filter_map(|(name, g)| drift(a.grey(*g), b.grey(*g)).map(|d| ((*name).to_string(), json!(d)))).collect();
    let out = strip(json!({"tones": tones, "black": end(0), "white": end(255), "slope": slope, "neutral": neutral, "notes": notes(&b, intake)}));
    Ok(out.as_object().is_some_and(|o| !o.is_empty()).then_some(out))
}

fn per_mille(levels: &[f64]) -> Vec<i64> {
    let mut bins = [0usize; 64];
    for l in levels {
        bins[((l.round().clamp(0.0, 255.0) as usize) / 4).min(63)] += 1;
    }
    let n = levels.len().max(1) as f64;
    bins.iter().map(|b| (*b as f64 / n * 1000.0).round() as i64).collect()
}

fn spread(mut levels: Vec<f64>) -> Value {
    levels.sort_by(f64::total_cmp);
    let at = |q: f64| levels.get(((levels.len().max(1) - 1) as f64 * q).round() as usize).map_or(0, |v| v.round() as i64);
    json!({"p5": at(0.05), "p50": at(0.5), "p95": at(0.95)})
}

pub struct Area {
    pub tones: [f64; 3],
    pub summary: Value,
}

pub fn area(region: &Intake, m: &Mapped) -> Area {
    let (inside, out) = (region.tones_in(), region.curved(m));
    let summary = json!({"in": spread(inside), "out": spread(out)});
    let tone = |side: &str, p: &str| summary[side][p].as_f64().unwrap_or(0.0);
    Area { tones: [tone("in", "p5"), tone("in", "p50"), tone("in", "p95")], summary }
}

pub fn scope(curve: &Value, m: &Mapped, intake: &Intake, region: Option<&Area>) -> Result<Value, String> {
    let inputs: Vec<usize> = (0..32).map(|i| i * 8).chain([255]).collect();
    let row = |k: usize| inputs.iter().map(|&l| m.levels[k][l].round() as i64).collect::<Vec<_>>();
    let rgb = row(0);
    let mut map = Map::from_iter([("rgb".to_string(), json!(rgb))]);
    for (c, name) in CHANNELS.iter().enumerate().skip(1) {
        let channel = row(c);
        if channel != rgb {
            map.insert((*name).to_string(), json!(channel));
        }
    }
    let at = intake.zones();
    let tinted = m.tinted().iter().any(|t| *t);
    let neutral: Map<String, Value> = GREYS
        .iter()
        .map(|(name, g)| {
            let [a, b] = m.grey(*g);
            ((*name).to_string(), json!({"a": look::round(a, 1), "b": look::round(b, 1)}))
        })
        .collect();
    let slopes: Vec<f64> = at.iter().map(|t| look::round(m.slope(0, *t), 2)).collect();
    Ok(strip(json!({
        "points": points(curve, false)?,
        "map": map,
        "hist": {"in": per_mille(&intake.tones_in()), "out": per_mille(&intake.curved(m))},
        "black": m.levels[0][0].round() as i64,
        "white": m.levels[0][255].round() as i64,
        "slope": {"sh": slopes[0], "mid": slopes[1], "hi": slopes[2], "at": at.map(|t| t as i64)},
        "neutral": tinted.then_some(neutral),
        "notes": notes(m, intake),
        "region": region.map(|r| r.summary.clone()),
    })))
}

pub fn drawn_points(curve: &Value) -> Result<Vec<Handles>, String> {
    let used = in_use(curve)?;
    Ok((0..4).filter(|c| used[*c] || *c == 0).map(|c| (c, drawn(curve, c).iter().map(|[x, y]| [x * 255.0, y * 255.0]).collect())).collect())
}
