use rayon::prelude::*;

use crate::halation_tables::{
    BETA, DOWN_SD, F_BELOW_SLOPE, F_GRID, FROM_DWG, G_HUE, G_LAYER, KAPPA, KERNEL_TRUNCATE, LEVEL_MAX, LEVEL_SWITCH, LEVEL_VAR, MASK_COEF, MASK_SIGMA, MASK_TERMS, MASK_TRUNCATE,
    OCTAVE_WEIGHTS, SIGMA0, TO_DWG, UP_SD,
};
use crate::pyramid::{Level, Span};
use crate::texture::{Curves, Plane, Region, Size};

// why: Resolve blurs an octave on the deepest level n ≤ LEVEL_MAX where (sigma/2^n)² ≥ LEVEL_SWITCH (scratchpad
// flc/hal/pyr_cf.py, at the noise floor on levels 1–3, interior and borders). The level is decimated straight from
// full resolution by s = 2^n (a Gaussian of DOWN_SD level px over ±4s taps), blurred there by a sampled Gaussian of
// variance (sigma/s)² − LEVEL_VAR, LEVEL_VAR being what the decimation and the interpolation add, and interpolated
// back from 4 level samples per phase (a Gaussian of UP_SD level px)
const LEVEL_TRUNCATE: f64 = 4.0;

fn level_of(sigma: f64) -> usize {
    let mut n = 0;
    while n < LEVEL_MAX && (sigma / f64::from(2u32 << n)).powi(2) >= LEVEL_SWITCH {
        n += 1;
    }
    n
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HalationParams {
    pub amount: f32,
    pub hue: f32,
    pub sat: f32,
    pub radius: f32,
    pub highlights_only: bool,
}

pub(crate) struct Octave {
    pub(crate) level: usize,
    /// the blur at the octave's level
    pub(crate) kernel: Vec<f32>,
    /// the decimation taps (2·4s + 1) and the interpolation weights (4 per phase), empty at full resolution
    pub(crate) down: Vec<f32>,
    pub(crate) up: Vec<f32>,
}

impl Octave {
    fn of(sigma: f64, truncate: f64) -> Octave {
        let level = level_of(sigma);
        if level == 0 {
            return Octave::full(sigma, truncate);
        }
        let s = f64::from(1u32 << level);
        let blur = ((sigma / s).powi(2) - LEVEL_VAR).max(1e-12).sqrt();
        let kernel = gaussian(blur, ((LEVEL_TRUNCATE * blur).ceil() as isize).max(1));
        let down = gaussian(DOWN_SD * s, 4 << level);
        let up = (0..1u32 << level)
            .flat_map(|p| {
                let raw: [f64; 4] = std::array::from_fn(|k| (-(f64::from(p) / s - (k as f64 - 1.0)).powi(2) / (2.0 * UP_SD * UP_SD)).exp());
                let total: f64 = raw.iter().sum();
                raw.map(|v| (v / total) as f32)
            })
            .collect();
        Octave { level, kernel, down, up }
    }

    fn full(sigma: f64, truncate: f64) -> Octave {
        Octave { level: 0, kernel: gaussian(sigma, (truncate * sigma).ceil() as isize), down: Vec::new(), up: Vec::new() }
    }

    pub(crate) fn scale(&self) -> usize {
        1 << self.level
    }

    fn reach(&self) -> usize {
        let r = self.kernel.len() / 2;
        if self.level == 0 { r + 2 } else { self.scale() * (r + 3) + self.down.len() / 2 + 2 }
    }

    fn blurred(&self, base: &Level, frame: Size) -> Level {
        if self.level == 0 {
            return base.blur(&self.kernel, false).blur(&self.kernel, true);
        }
        let s = self.scale();
        let level = base.reduce(&self.down, s, false).reduce(&self.down, s, true);
        let level = level.blur(&self.kernel, false).blur(&self.kernel, true);
        level.expand(&self.up, s, frame.w, false).expand(&self.up, s, frame.h, true)
    }
}

pub(crate) struct Halation {
    /// weight of octave k for DWG layer j
    pub(crate) weights: [[f32; 3]; 3],
    pub(crate) octaves: [Octave; 3],
    pub(crate) mask: Option<Octave>,
}

// why: a sampled Gaussian normalised in f64; Resolve's level-0 halation fits any support of 4σ or more equally
// (scratchpad flc/hal/halation_tables.json)
fn gaussian(sigma: f64, radius: isize) -> Vec<f32> {
    let raw: Vec<f64> = (-radius..=radius).map(|x| (-0.5 / (sigma * sigma) * (x * x) as f64).exp()).collect();
    let total: f64 = raw.iter().sum();
    raw.iter().map(|v| (v / total) as f32).collect()
}

fn interp(x: f64, xs: &[f64], ys: impl Fn(usize) -> f64) -> f64 {
    if x <= xs[0] {
        return ys(0);
    }
    let last = xs.len() - 1;
    if x >= xs[last] {
        return ys(last);
    }
    let i = xs.partition_point(|&v| v <= x) - 1;
    ys(i) + (ys(i + 1) - ys(i)) * (x - xs[i]) / (xs[i + 1] - xs[i])
}

fn mix(sat: f64, g: f64) -> f64 {
    let v = (1.0 - sat) + sat * g / (2.0 - g);
    2.0 * v / (1.0 + v)
}

fn octave_weight(k: usize, amount: f64, t: f64) -> f64 {
    match k {
        0 => OCTAVE_WEIGHTS[0] * (1.0 - (-BETA * amount * t).exp()) / (1.0 - (-BETA * 0.5).exp()),
        _ => OCTAVE_WEIGHTS[k] * amount * t / 0.5,
    }
}

impl Halation {
    // why: Film Look Creator's halation as measured from Resolve renders (scratchpad flc/hal): three octave blurs of
    // each DaVinci Wide Gamut layer in a steep transfer domain, tinted per layer by hue and saturation
    pub(crate) fn new(frame: Size, p: &HalationParams) -> Option<Halation> {
        if p.amount <= 0.0 {
            return None;
        }
        let (amount, hue, sat) = (f64::from(p.amount), f64::from(p.hue), f64::from(p.sat));
        let s0 = SIGMA0 * (f64::from(p.radius) / 4.0) * (frame.h as f64 / 1600.0);
        let layer = |j: usize, k: usize| match j {
            0 => 1.0,
            1 => mix(sat, interp(hue, &G_HUE, |i| G_LAYER[i][k])),
            _ => mix(sat, 0.0),
        };
        let weights = std::array::from_fn(|k| std::array::from_fn(|j| octave_weight(k, amount, layer(j, k)) as f32));
        let octaves = std::array::from_fn(|k| Octave::of(s0 * f64::from(1u32 << k), KERNEL_TRUNCATE));
        let mask = p.highlights_only.then(|| Octave::of(MASK_SIGMA * s0, MASK_TRUNCATE));
        Some(Halation { weights, octaves, mask })
    }

    pub(crate) fn reach(&self) -> usize {
        self.octaves.iter().chain(&self.mask).map(Octave::reach).max().unwrap_or(0)
    }

}

const STEPS: f32 = (F_GRID.len() - 1) as f32;

pub(crate) fn transfer(w: f32) -> f32 {
    if w < 0.0 {
        return F_GRID[0] + F_BELOW_SLOPE * w;
    }
    let t = w.min(1.0) * STEPS;
    let i = (t as usize).min(F_GRID.len() - 2);
    F_GRID[i] + (F_GRID[i + 1] - F_GRID[i]) * (t - i as f32)
}

pub(crate) fn untransfer(u: f32) -> f32 {
    if u < F_GRID[0] {
        return (u - F_GRID[0]) / F_BELOW_SLOPE;
    }
    let i = F_GRID.partition_point(|&v| v <= u).clamp(1, F_GRID.len() - 1) - 1;
    (i as f32 + (u - F_GRID[i]) / (F_GRID[i + 1] - F_GRID[i])) / STEPS
}

fn times(m: &[[f32; 3]; 3], v: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|j| m[j][0] * v[0] + m[j][1] * v[1] + m[j][2] * v[2])
}

pub(crate) fn layers(c: [f32; 3], curves: &Curves) -> [f32; 3] {
    times(&TO_DWG, c.map(|v| curves.to_linear(v))).map(|l| transfer(curves.to_code(l)))
}

fn gray(w: [f32; 3]) -> f32 {
    let mean = (w[0] + w[1] + w[2]) / 3.0;
    let mut g = mean;
    for (term, coef) in MASK_TERMS.iter().zip(MASK_COEF) {
        let (mut product, mut power) = (1.0f32, 1.0f32);
        for &i in term.iter().filter(|&&i| i < 3) {
            product *= w[i];
            power *= mean;
        }
        g += coef * (product - power);
    }
    g.clamp(w[0].min(w[1]).min(w[2]), w[0].max(w[1]).max(w[2]))
}

pub(crate) fn weight(c: [f32; 3], curves: &Curves) -> f32 {
    let w = times(&TO_DWG, c.map(|v| curves.to_linear(v.clamp(0.0, 1.0)))).map(|l| curves.to_code(l.max(0.0)).clamp(0.0, 1.0));
    let v = KAPPA * (transfer(gray(w)) - F_GRID[0]);
    v * v.clamp(0.0, 1.0)
}

pub(crate) fn back(acc: [f32; 3], curves: &Curves) -> [f32; 3] {
    times(&FROM_DWG, acc.map(|u| curves.to_linear(untransfer(u)))).map(|l| curves.to_code(l))
}

/// `cand` holds the clipped candidate over `out` grown by `reach`; returns the halated code values over `out`.
pub(crate) fn apply(cand: &Plane<[f32; 3]>, frame: Size, out: Region, h: &Halation, curves: &Curves) -> Vec<[f32; 3]> {
    let region = cand.region;
    let span = |lo: usize, len: usize, n: usize| Span { lo, hi: lo + len, n };
    let (x0, y0) = (span(region.x, region.w, frame.w), span(region.y, region.h, frame.h));
    let u = Level { x: x0, y: y0, data: cand.data.par_iter().map(|&c| layers(c, curves)).collect() };
    let mut sum: Option<Level> = None;
    for (k, o) in h.octaves.iter().enumerate() {
        sum = Some(o.blurred(&u, frame).add_into(h.weights[k], sum.as_ref()));
    }
    let Some(sum) = sum else { return cand.data.clone() };
    let mask = h.mask.as_ref().map(|o| {
        let m0 = Level { x: x0, y: y0, data: cand.data.par_iter().map(|&c| [weight(c, curves), 0.0, 0.0]).collect() };
        o.blurred(&m0, frame)
    });
    let total: [f32; 3] = std::array::from_fn(|j| h.weights[0][j] + h.weights[1][j] + h.weights[2][j]);
    let mut result = vec![[0.0f32; 3]; out.w * out.h];
    result.par_chunks_mut(out.w).enumerate().for_each(|(j, row)| {
        for (i, v) in row.iter_mut().enumerate() {
            let (x, y) = (out.x + i, out.y + j);
            let base = u.at(x, y);
            let s = sum.at(x, y);
            let m = mask.as_ref().map_or(1.0, |m| m.at(x, y)[0].min(1.0));
            *v = back(std::array::from_fn(|c| base[c] + m * (s[c] - total[c] * base[c])), curves);
        }
    });
    result
}

/// The whole-frame CPU operator, for checks against the reference model.
pub fn halate_frame(cand: Vec<[f32; 3]>, frame: Size, p: &HalationParams) -> Option<Vec<[f32; 3]>> {
    let h = Halation::new(frame, p)?;
    let curves = Curves { log2: crate::film::log2_table(), exp2: crate::film::exp2_table() };
    let whole = Region::whole(frame.w, frame.h);
    let clipped = Plane { region: whole, data: cand.into_iter().map(|c| c.map(|v| v.min(1.0))).collect() };
    Some(apply(&clipped, frame, whole, &h, &curves))
}

#[cfg(test)]
mod tests {
    use super::*;

    // the agent's level rule (scratchpad flc/hal): Resolve switches octave 0 to level 1 between radius 2.870 and 2.872
    // and to level 2 between 5.74 and 5.75, and nothing goes deeper than 3, at sigma = 3.16844 radius / 4
    #[test]
    fn octaves_pick_the_identified_level() {
        for (radius, level) in [(2.870, 0), (2.872, 1), (5.74, 1), (5.75, 2), (12.0, 3), (40.0, 3)] {
            assert_eq!(level_of(3.16844 * radius / 4.0), level, "radius {radius}");
        }
    }

    // a window halates exactly as the whole frame does over the same pixels
    #[test]
    fn windows_match_the_whole_frame() {
        let frame = Size { w: 300, h: 200 };
        let p = HalationParams { amount: 0.8, hue: 0.45, sat: 1.0, radius: 24.0, highlights_only: true };
        let h = Halation::new(frame, &p).expect("halation");
        assert!(h.octaves.iter().any(|o| o.level >= 2));
        let curves = Curves { log2: crate::film::log2_table(), exp2: crate::film::exp2_table() };
        let pixel = |x: usize, y: usize| {
            let v = ((x * 7 + y * 13) % 29) as f32 / 29.0;
            if (x / 17 + y / 11) % 5 == 0 { [1.0, 0.95, 0.9] } else { [v, v * 0.8, 0.3] }
        };
        let whole = Region::whole(frame.w, frame.h);
        let plane = |r: Region| Plane { region: r, data: (0..r.h).flat_map(|j| (0..r.w).map(move |i| pixel(r.x + i, r.y + j))).collect() };
        let full = apply(&plane(whole), frame, whole, &h, &curves);
        for out in [Region { x: 120, y: 70, w: 40, h: 30 }, Region { x: 0, y: 0, w: 50, h: 40 }, Region { x: 260, y: 170, w: 40, h: 30 }] {
            let window = apply(&plane(out.grown(h.reach(), frame)), frame, out, &h, &curves);
            for j in 0..out.h {
                for i in 0..out.w {
                    assert_eq!(window[j * out.w + i], full[(out.y + j) * frame.w + out.x + i], "{out:?} at {i},{j}");
                }
            }
        }
    }
}
