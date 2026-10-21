use filmlab_engine::tone_curve::{Curve, Curves};
use gpui_kit::*;
use serde_json::Value;

use super::Tab;
use crate::develop::{CurveInput, POINT_CURVES};
use crate::ui::color_rail;

pub const INSET: f32 = 10.0;
pub const HEIGHT: f32 = 292.0;
pub const SAMPLES: usize = 256;
const RAIL_HEIGHT: f32 = 3.0;
const LUMA: [f64; 3] = [0.2126, 0.7152, 0.0722];
const HISTOGRAM_PIXELS: usize = 300_000;

#[derive(Clone, Copy, Debug)]
pub struct Plot {
    pub bounds: Bounds<Pixels>,
    origin: Point<Pixels>,
    side: f32,
}

impl Plot {
    pub fn of(bounds: Bounds<Pixels>) -> Plot {
        Plot { bounds, origin: bounds.origin + point(px(INSET), px(INSET)), side: (f32::from(bounds.size.width) - 2.0 * INSET).max(1.0) }
    }

    pub fn at(&self, u: f64, v: f64) -> Point<Pixels> {
        point(self.origin.x + px(u as f32 * self.side), self.origin.y + px((1.0 - v) as f32 * self.side))
    }

    pub fn uv(&self, p: Point<Pixels>) -> (f64, f64) {
        (f64::from(f32::from(p.x - self.origin.x) / self.side), 1.0 - f64::from(f32::from(p.y - self.origin.y) / self.side))
    }

    pub fn rail(&self) -> Bounds<Pixels> {
        Bounds::new(point(self.origin.x, self.origin.y + px(self.side + INSET)), size(px(self.side), px(RAIL_HEIGHT)))
    }

    pub fn outside_by(&self, p: Point<Pixels>) -> f32 {
        let b = self.bounds;
        let dx = f32::from((b.left() - p.x).max(p.x - b.right())).max(0.0);
        let dy = f32::from((b.top() - p.y).max(p.y - b.bottom())).max(0.0);
        dx.max(dy)
    }
}

pub fn level(value: f64) -> i64 {
    (value * 255.0).round() as i64
}

pub fn points(settings: &Value, channel: usize) -> Vec<[f64; 2]> {
    let stored: Vec<[f64; 2]> = settings["curve"][POINT_CURVES[channel]].as_array().map(|a| a.iter().filter_map(|p| Some([p["x"].as_f64()?, p["y"].as_f64()?])).collect()).unwrap_or_default();
    if stored.len() < 2 { vec![[0.0, 0.0], [1.0, 1.0]] } else { stored }
}

pub fn line(curves: &Curves, tab: Tab) -> &Curve {
    match tab.channel() {
        None => &curves.regions,
        Some(0) => &curves.master,
        Some(c) => &curves.channels[c - 1],
    }
}

pub fn samples(curve: &Curve) -> Vec<f64> {
    (0..=SAMPLES).map(|i| curve.at(i as f64 / SAMPLES as f64)).collect()
}

fn white(alpha: f32) -> Hsla {
    Hsla { h: 0.0, s: 0.0, l: 1.0, a: alpha }
}

pub fn ink(tab: Tab) -> Hsla {
    match tab {
        Tab::Regions | Tab::Rgb => rgba(0xebebebff).into(),
        Tab::Red => color_rail::ink([1.0, 0.0, 0.0]),
        Tab::Green => color_rail::ink([0.0, 1.0, 0.0]),
        Tab::Blue => color_rail::ink([0.0, 0.0, 1.0]),
    }
}

pub fn opposite(tab: Tab) -> Option<(&'static str, Hsla)> {
    match tab {
        Tab::Red => Some(("Cyan", color_rail::ink([0.0, 1.0, 1.0]))),
        Tab::Green => Some(("Magenta", color_rail::ink([1.0, 0.0, 1.0]))),
        Tab::Blue => Some(("Yellow", color_rail::ink([1.0, 1.0, 0.0]))),
        Tab::Regions | Tab::Rgb => None,
    }
}

pub fn dot(tab: Tab) -> Hsla {
    match tab {
        Tab::Regions | Tab::Rgb => rgba(0xf2f2f2ff).into(),
        _ => ink(tab),
    }
}

pub fn in_use_dot(tab: Tab) -> Hsla {
    match tab {
        Tab::Regions | Tab::Rgb => white(0.6),
        _ => ink(tab),
    }
}

pub fn histogram_fill(tab: Tab) -> Hsla {
    match tab {
        Tab::Regions | Tab::Rgb => white(0.07),
        _ => ink(tab).opacity(0.1),
    }
}

pub struct Intake {
    tab: Tab,
    table: [f64; 256],
    edges: [f64; 257],
}

impl Intake {
    pub fn of(curves: &Curves, tab: Tab) -> Intake {
        let into_channels = curves.master.after(&curves.regions);
        let curve = match tab {
            Tab::Regions => None,
            Tab::Rgb => Some(&curves.regions),
            _ => Some(&into_channels),
        };
        let through = |x: f64| curve.map_or(x, |c| c.at(x));
        Intake { tab, table: std::array::from_fn(|i| through(i as f64 / 255.0)), edges: std::array::from_fn(|k| through(((k as f64 - 0.5) / 255.0).clamp(0.0, 1.0))) }
    }

    pub fn key(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.tab.channel().hash(&mut h);
        self.table.iter().chain(&self.edges).for_each(|v| v.to_bits().hash(&mut h));
        h.finish()
    }

    fn spread(&self, byte: usize, count: f64, bins: &mut [f64; 256]) {
        let (a, b) = (self.edges[byte] * 255.0, self.edges[byte + 1] * 255.0);
        let (lo, hi) = (a.min(b), a.max(b));
        if hi - lo < 1e-6 {
            bins[lo.round().clamp(0.0, 255.0) as usize] += count;
            return;
        }
        let first = (lo + 0.5).floor().clamp(0.0, 255.0) as usize;
        let last = (hi + 0.5).floor().clamp(0.0, 255.0) as usize;
        for (bin, slot) in bins.iter_mut().enumerate().take(last + 1).skip(first) {
            let overlap = (hi.min(bin as f64 + 0.5) - lo.max(bin as f64 - 0.5)).max(0.0);
            *slot += count * overlap / (hi - lo);
        }
    }

    pub fn tone(&self, rgb: [u8; 3]) -> f64 {
        let t = rgb.map(|v| self.table[usize::from(v)]);
        match self.tab.channel() {
            None | Some(0) => t.iter().zip(LUMA).map(|(v, w)| v * w).sum(),
            Some(c) => t[c - 1],
        }
    }
}

pub fn histogram(input: &CurveInput, intake: &Intake) -> Vec<f32> {
    let mut counts = [0f64; 256];
    let pixels = input.rgba.len() / 4;
    let step = (pixels / HISTOGRAM_PIXELS).max(1);
    let sampled = input.rgba.as_chunks::<4>().0.iter().step_by(step);
    match intake.tab.channel() {
        Some(c) if c > 0 => {
            let mut bytes = [0f64; 256];
            sampled.for_each(|p| bytes[usize::from(p[c - 1])] += 1.0);
            bytes.iter().enumerate().filter(|(_, n)| **n > 0.0).for_each(|(b, n)| intake.spread(b, *n, &mut counts));
        }
        _ => sampled.for_each(|p| counts[(intake.tone([p[0], p[1], p[2]]) * 255.0).round().clamp(0.0, 255.0) as usize] += 1.0),
    }
    let smooth: Vec<f64> = (0..256usize)
        .map(|i| {
            let (lo, hi) = (i.saturating_sub(2), (i + 3).min(256));
            counts[lo..hi].iter().sum::<f64>() / (hi - lo) as f64
        })
        .collect();
    let mut inner = smooth[2..254].to_vec();
    inner.sort_by(f64::total_cmp);
    let peak = inner[(inner.len() as f64 * 0.995) as usize].max(1.0);
    smooth.iter().map(|v| (v / peak).min(1.0).sqrt() as f32).collect()
}
