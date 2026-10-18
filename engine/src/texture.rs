use rayon::prelude::*;

use crate::dust::{self, DustParams};
use crate::halation::{self, Halation, HalationParams};
use crate::film::{exp2_table, fast_exp2, fast_log2, log2_table};
use crate::grain::{self, Field, GrainParams};
use crate::texture_tables::{BASE, BEND2, BEND3, CHROMA, GAIN_HIGH, GAIN_LOW, KNEE, KNEE_TAIL, TONE};

const LUMA: [f32; 3] = [0.2126, 0.7152, 0.0722];
const EPS: f32 = 0.01;
const BLUR_PER_HEIGHT: f32 = 0.0099;
pub(crate) const DECODE_CUT: f32 = 0.081;
pub(crate) const ENCODE_CUT: f32 = 0.018;
pub(crate) const SLOPE_709: f32 = 4.5;
pub(crate) const SCALE_709: f32 = 1.099;
pub(crate) const OFFSET_709: f32 = 0.099;
pub(crate) const GAMMA_709: f32 = 0.45;
pub(crate) const DECODE_POWER: f32 = 1.0 / GAMMA_709;
pub(crate) const KNEE_START: f32 = 0.76;
pub(crate) const KNEE_STEPS: f32 = 1000.0;
pub(crate) const TAIL_START: f32 = 1.08;
pub(crate) const TAIL_END: f32 = 2.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Region {
    pub x: usize,
    pub y: usize,
    pub w: usize,
    pub h: usize,
}

impl Region {
    pub fn whole(w: usize, h: usize) -> Region {
        Region { x: 0, y: 0, w, h }
    }

    pub fn grown(self, by: usize, frame: Size) -> Region {
        let x = self.x.saturating_sub(by);
        let y = self.y.saturating_sub(by);
        Region { x, y, w: (self.x + self.w + by).min(frame.w) - x, h: (self.y + self.h + by).min(frame.h) - y }
    }

    fn pixels(self) -> usize {
        self.w * self.h
    }

    fn column(self, x: isize) -> usize {
        x.clamp(self.x as isize, (self.x + self.w) as isize - 1) as usize - self.x
    }

    fn row(self, y: isize) -> usize {
        y.clamp(self.y as isize, (self.y + self.h) as isize - 1) as usize - self.y
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Size {
    pub w: usize,
    pub h: usize,
}

pub struct Plane<T> {
    pub region: Region,
    pub data: Vec<T>,
}

impl<T: Copy> Plane<T> {
    pub(crate) fn at(&self, x: isize, y: isize) -> T {
        self.data[self.region.row(y) * self.region.w + self.region.column(x)]
    }
}

#[derive(Clone, Copy, Debug)]
pub struct TextureParams {
    pub film_blur: f32,
    pub grain: Option<GrainParams>,
    pub dust: Option<DustParams>,
    pub halation: Option<HalationParams>,
}

#[derive(Clone, Copy)]
pub(crate) struct Tap {
    pub(crate) lo: usize,
    pub(crate) hi: usize,
    pub(crate) f: f32,
}

pub(crate) fn taps(n_in: usize, n_out: usize) -> Vec<Tap> {
    let scale = n_in as f64 / n_out as f64;
    let last = n_in as isize - 1;
    (0..n_out)
        .map(|i| {
            let src = (i as f64 + 0.5) * scale - 0.5;
            let lo = src.floor();
            let f = (src - lo) as f32;
            let lo = lo as isize;
            Tap { lo: lo.clamp(0, last) as usize, hi: (lo + 1).clamp(0, last) as usize, f }
        })
        .collect()
}

fn mix(a: f32, b: f32, t: f32) -> f32 {
    a * (1.0 - t) + b * t
}

pub(crate) struct Guide {
    pub(crate) step: usize,
    pub(crate) radius: usize,
    pub(crate) reduced: Size,
    pub(crate) down_x: Vec<Tap>,
    pub(crate) down_y: Vec<Tap>,
    pub(crate) up_x: Vec<Tap>,
    pub(crate) up_y: Vec<Tap>,
}

impl Guide {
    pub(crate) fn new(frame: Size) -> Guide {
        let long = frame.w.max(frame.h);
        let step = (long / 320).clamp(1, 4);
        let radius = (((long / step) as f64 / 100.0 + 0.23).floor() as usize).max(1);
        let reduced = Size { w: (frame.w / step).max(1), h: (frame.h / step).max(1) };
        Guide {
            step,
            radius,
            reduced,
            down_x: taps(frame.w, reduced.w),
            down_y: taps(frame.h, reduced.h),
            up_x: taps(reduced.w, frame.w),
            up_y: taps(reduced.h, frame.h),
        }
    }

    pub(crate) fn reach(&self, out: Region) -> (Region, Region) {
        let r = self.radius * 2;
        let span = |taps: &[Tap], from: usize, to: usize, n: usize| {
            let lo = taps[from].lo.saturating_sub(r);
            let hi = (taps[to - 1].hi + r).min(n - 1);
            (lo, hi)
        };
        let (rx0, rx1) = span(&self.up_x, out.x, out.x + out.w, self.reduced.w);
        let (ry0, ry1) = span(&self.up_y, out.y, out.y + out.h, self.reduced.h);
        let reduced = Region { x: rx0, y: ry0, w: rx1 - rx0 + 1, h: ry1 - ry0 + 1 };
        let fx0 = self.down_x[rx0].lo;
        let fx1 = self.down_x[rx1].hi;
        let fy0 = self.down_y[ry0].lo;
        let fy1 = self.down_y[ry1].hi;
        (reduced, Region { x: fx0, y: fy0, w: fx1 - fx0 + 1, h: fy1 - fy0 + 1 })
    }
}

fn box_filter(plane: &Plane<[f32; 2]>, radius: usize, frame: Size) -> Plane<[f32; 2]> {
    let region = plane.region;
    let n = (2 * radius + 1) as f32;
    let r = radius as isize;
    let clamp_y = |y: isize| y.clamp(0, frame.h as isize - 1);
    let clamp_x = |x: isize| x.clamp(0, frame.w as isize - 1);
    let mut column = vec![[0.0f32; 2]; region.pixels()];
    column.par_chunks_mut(region.w).enumerate().for_each(|(j, row)| {
        let y = (region.y + j) as isize;
        for (i, v) in row.iter_mut().enumerate() {
            let x = (region.x + i) as isize;
            let mut sum = [0.0f32; 2];
            for k in -r..=r {
                let s = plane.at(x, clamp_y(y + k));
                sum[0] += s[0];
                sum[1] += s[1];
            }
            *v = [sum[0] / n, sum[1] / n];
        }
    });
    let column = Plane { region, data: column };
    let mut out = vec![[0.0f32; 2]; region.pixels()];
    out.par_chunks_mut(region.w).enumerate().for_each(|(j, row)| {
        let y = (region.y + j) as isize;
        for (i, v) in row.iter_mut().enumerate() {
            let x = (region.x + i) as isize;
            let mut sum = [0.0f32; 2];
            for k in -r..=r {
                let s = column.at(clamp_x(x + k), y);
                sum[0] += s[0];
                sum[1] += s[1];
            }
            *v = [sum[0] / n, sum[1] / n];
        }
    });
    Plane { region, data: out }
}

fn guided_base(luma: &Plane<f32>, frame: Size, out: Region) -> Vec<f32> {
    let guide = Guide::new(frame);
    let (reduced, _) = guide.reach(out);
    let mut moments = vec![[0.0f32; 2]; reduced.pixels()];
    moments.par_chunks_mut(reduced.w).enumerate().for_each(|(j, row)| {
        let ty = guide.down_y[reduced.y + j];
        for (i, v) in row.iter_mut().enumerate() {
            let tx = guide.down_x[reduced.x + i];
            let at = |x: usize, y: usize| luma.at(x as isize, y as isize);
            let left = mix(at(tx.lo, ty.lo), at(tx.lo, ty.hi), ty.f);
            let right = mix(at(tx.hi, ty.lo), at(tx.hi, ty.hi), ty.f);
            let s = mix(left, right, tx.f);
            *v = [s, s * s];
        }
    });
    let means = box_filter(&Plane { region: reduced, data: moments }, guide.radius, guide.reduced);
    let coefficients: Vec<[f32; 2]> = means
        .data
        .par_iter()
        .map(|&[mean, square]| {
            let var = square - mean * mean;
            let a = var / (var + EPS);
            [a, mean * (1.0 - a)]
        })
        .collect();
    let smooth = box_filter(&Plane { region: reduced, data: coefficients }, guide.radius, guide.reduced);
    let mut q = vec![0.0f32; out.pixels()];
    q.par_chunks_mut(out.w).enumerate().for_each(|(j, row)| {
        let y = out.y + j;
        let ty = guide.up_y[y];
        for (i, v) in row.iter_mut().enumerate() {
            let x = out.x + i;
            let tx = guide.up_x[x];
            let at = |x: usize, y: usize| smooth.at(x as isize, y as isize);
            let (p00, p01, p10, p11) = (at(tx.lo, ty.lo), at(tx.lo, ty.hi), at(tx.hi, ty.lo), at(tx.hi, ty.hi));
            let a = mix(mix(p00[0], p01[0], ty.f), mix(p10[0], p11[0], ty.f), tx.f);
            let b = mix(mix(p00[1], p01[1], ty.f), mix(p10[1], p11[1], ty.f), tx.f);
            *v = a * luma.at(x as isize, y as isize) + b;
        }
    });
    q
}

fn on_grid(x: f32, table: &[f32]) -> f32 {
    let steps = (table.len() - 1) as f32;
    let t = x.clamp(0.0, 1.0) * steps;
    let i = (t as usize).min(table.len() - 2);
    table[i] + (table[i + 1] - table[i]) * (t - i as f32)
}

fn tone(q: f32, d: f32) -> f32 {
    let u = on_grid(q, &BASE) + d + BEND2 * d * d + BEND3 * d * d * d;
    let top = BASE[BASE.len() - 1];
    if u > top {
        TONE[TONE.len() - 1] + GAIN_HIGH * (u - top)
    } else if u < 0.0 {
        TONE[0] + GAIN_LOW * u
    } else {
        let i = BASE.partition_point(|&b| b <= u).clamp(1, BASE.len() - 1) - 1;
        let f = (u - BASE[i]) / (BASE[i + 1] - BASE[i]);
        TONE[i] + (TONE[i + 1] - TONE[i]) * f
    }
}

fn luma(c: [f32; 3]) -> f32 {
    LUMA[0] * c[0] + LUMA[1] * c[1] + LUMA[2] * c[2]
}

pub(crate) struct Curves<'a> {
    pub(crate) log2: &'a [f32],
    pub(crate) exp2: &'a [f32],
}

impl Curves<'_> {
    pub(crate) fn to_linear(&self, v: f32) -> f32 {
        if v < DECODE_CUT { v / SLOPE_709 } else { fast_exp2(fast_log2((v + OFFSET_709) / SCALE_709, self.log2) * DECODE_POWER, self.exp2) }
    }

    pub(crate) fn to_code(&self, l: f32) -> f32 {
        if l < ENCODE_CUT { SLOPE_709 * l } else { SCALE_709 * fast_exp2(fast_log2(l, self.log2) * GAMMA_709, self.exp2) - OFFSET_709 }
    }

    // why: FilmVision softens colours past 0.76 HSV saturation in Rec.709 linear light, twice, keeping luminance
    // (identified from Resolve renders, scratchpad flc/sat/report.md); the 709 curves keep their 0.081 step
    fn saturated(&self, cand: [f32; 3]) -> [f32; 3] {
        self.saturated_free(cand.map(|c| c.min(1.0)))
    }

    // why: after the halation the stage takes its values unclipped (above 1 and below 0); only its output is clipped
    // (scratchpad flc/hal/model5.py sat_stage, identified from Resolve renders)
    fn saturated_free(&self, value: [f32; 3]) -> [f32; 3] {
        let lin = value.map(|c| self.to_linear(c));
        desaturate(desaturate(lin)).map(|l| self.to_code(l).clamp(0.0, 1.0))
    }
}

fn knee(s: f32) -> f32 {
    if s <= KNEE_START {
        s
    } else if s > TAIL_START {
        let e = s.min(TAIL_END) - TAIL_START;
        KNEE_TAIL[0] + KNEE_TAIL[1] * e + KNEE_TAIL[2] * e * e + KNEE_TAIL[3] * (s - TAIL_END).max(0.0)
    } else {
        let t = (s - KNEE_START) * KNEE_STEPS;
        let i = (t as usize).min(KNEE.len() - 2);
        KNEE[i] + (KNEE[i + 1] - KNEE[i]) * (t - i as f32)
    }
}

fn desaturate(lin: [f32; 3]) -> [f32; 3] {
    let top = lin[0].max(lin[1]).max(lin[2]);
    if top <= 0.0 {
        return [0.0; 3];
    }
    let s = 1.0 - lin[0].min(lin[1]).min(lin[2]) / top;
    let k = if s > 0.0 { knee(s) / s } else { 1.0 };
    let y = luma(lin);
    let level = luma(lin.map(|c| c.max(0.0)));
    lin.map(|c| level + k * (c - y))
}

pub(crate) fn blur_weights(frame: Size, p: &TextureParams) -> Vec<f32> {
    if p.film_blur <= 0.0 { Vec::new() } else { gaussian_weights(BLUR_PER_HEIGHT * frame.h as f32 * p.film_blur) }
}

fn gaussian_weights(sigma: f32) -> Vec<f32> {
    let radius = (4.0 * sigma + 0.5) as usize;
    let raw: Vec<f64> = (-(radius as isize)..=radius as isize).map(|k| (-0.5 * (k * k) as f64 / f64::from(sigma * sigma)).exp()).collect();
    let total: f64 = raw.iter().sum();
    raw.iter().map(|w| (w / total) as f32).collect()
}

pub(crate) fn blur_radius(frame: Size, p: &TextureParams) -> usize {
    if p.film_blur <= 0.0 { 0 } else { (4.0 * BLUR_PER_HEIGHT * frame.h as f32 * p.film_blur + 0.5) as usize }
}

fn film_blur(plane: Plane<[f32; 3]>, frame: Size, p: &TextureParams) -> Plane<[f32; 3]> {
    let weights = blur_weights(frame, p);
    if weights.is_empty() {
        return plane;
    }
    let r = (weights.len() / 2) as isize;
    let region = plane.region;
    let pass = |src: &Plane<[f32; 3]>, vertical: bool| {
        let mut out = vec![[0.0f32; 3]; region.pixels()];
        out.par_chunks_mut(region.w).enumerate().for_each(|(j, row)| {
            let y = (region.y + j) as isize;
            for (i, v) in row.iter_mut().enumerate() {
                let x = (region.x + i) as isize;
                let mut sum = [0.0f32; 3];
                for (k, w) in (-r..=r).zip(&weights) {
                    let s = if vertical { src.at(x, (y + k).clamp(0, frame.h as isize - 1)) } else { src.at((x + k).clamp(0, frame.w as isize - 1), y) };
                    sum = [sum[0] + w * s[0], sum[1] + w * s[1], sum[2] + w * s[2]];
                }
                *v = sum;
            }
        });
        Plane { region, data: out }
    };
    let vertical = pass(&plane, true);
    pass(&vertical, false)
}

pub(crate) fn halation_of(frame: Size, p: &TextureParams) -> Option<Halation> {
    p.halation.and_then(|h| Halation::new(frame, &h))
}

pub fn margin(frame: Size, p: &TextureParams) -> usize {
    let guide = Guide::new(frame);
    let defocus = p.grain.map_or(0, |g| grain::defocus_radius(frame, &g));
    let halation = halation_of(frame, p).map_or(0, |h| h.reach());
    blur_radius(frame, p) + (2 * guide.radius + 3) * guide.step + 2 + defocus + halation
}

pub fn texture(input: Plane<[f32; 3]>, frame: Size, out: Region, p: &TextureParams) -> Vec<[f32; 3]> {
    let Some(g) = p.grain else { return detailed(input, frame, out, p) };
    let wide = out.grown(grain::defocus_radius(frame, &g), frame);
    let focused = defocus(Plane { region: wide, data: detailed(input, frame, wide, p) }, frame, out, &g);
    if !g.grains() {
        return focused;
    }
    let tables = grain::Tables { log2: log2_table(), exp2: exp2_table(), cos: grain::cos_table() };
    let field = Field::new(frame, &g).render(frame, out, &g, &tables);
    let scale = g.scale();
    focused.par_iter().zip(field.par_iter()).map(|(c, z)| grain::grained(*c, *z, scale)).collect()
}

// why: FilmVision's grain node defocuses the image itself, a Gaussian blur in the grain's own domain
fn defocus(plane: Plane<[f32; 3]>, frame: Size, out: Region, g: &GrainParams) -> Vec<[f32; 3]> {
    let weights = grain::defocus_weights(frame, g);
    if weights.is_empty() {
        return plane.data;
    }
    let r = (weights.len() / 2) as isize;
    let region = plane.region;
    let domain = Plane { region, data: plane.data.par_iter().map(|c| c.map(grain::forward)).collect() };
    let band = Region { x: region.x, y: out.y, w: region.w, h: out.h };
    let mut columns = vec![[0.0f32; 3]; band.pixels()];
    columns.par_chunks_mut(band.w).enumerate().for_each(|(j, row)| {
        let y = (band.y + j) as isize;
        for (i, v) in row.iter_mut().enumerate() {
            let x = (band.x + i) as isize;
            let mut sum = [0.0f32; 3];
            for (k, w) in (-r..=r).zip(&weights) {
                let s = domain.at(x, grain::reflect(y + k, frame.h) as isize);
                sum = [sum[0] + w * s[0], sum[1] + w * s[1], sum[2] + w * s[2]];
            }
            *v = sum;
        }
    });
    let columns = Plane { region: band, data: columns };
    let mut result = vec![[0.0f32; 3]; out.pixels()];
    result.par_chunks_mut(out.w).enumerate().for_each(|(j, row)| {
        let y = (out.y + j) as isize;
        for (i, v) in row.iter_mut().enumerate() {
            let x = (out.x + i) as isize;
            let mut sum = [0.0f32; 3];
            for (k, w) in (-r..=r).zip(&weights) {
                let s = columns.at(grain::reflect(x + k, frame.w) as isize, y);
                sum = [sum[0] + w * s[0], sum[1] + w * s[1], sum[2] + w * s[2]];
            }
            *v = sum.map(grain::inverse);
        }
    });
    result
}

// why: FilmVision's halation sits between the tone/detail candidate (clipped at 1) and the saturation stage
fn detailed(input: Plane<[f32; 3]>, frame: Size, out: Region, p: &TextureParams) -> Vec<[f32; 3]> {
    let mut blurred = film_blur(input, frame, p);
    let region = blurred.region;
    if let Some(d) = &p.dust {
        dust::apply(&mut blurred.data, frame, region, d);
    }
    let halation = halation_of(frame, p);
    let wide = halation.as_ref().map_or(out, |h| out.grown(h.reach(), frame));
    let luma = Plane { region, data: blurred.data.par_iter().map(|&c| luma(c)).collect() };
    let base = guided_base(&luma, frame, wide);
    let curves = Curves { log2: log2_table(), exp2: exp2_table() };
    let mut candidates = vec![[0.0f32; 3]; wide.pixels()];
    candidates.par_chunks_mut(wide.w).zip(base.par_chunks(wide.w)).enumerate().for_each(|(j, (row, q))| {
        let y = (wide.y + j) as isize;
        for (i, (v, q)) in row.iter_mut().zip(q).enumerate() {
            let x = (wide.x + i) as isize;
            let c = blurred.at(x, y);
            let y_in = luma.at(x, y);
            let y_out = tone(*q, y_in - q);
            *v = c.map(|ch| y_out + CHROMA * (ch - y_in));
        }
    });
    let Some(h) = halation else { return candidates.par_iter().map(|&c| curves.saturated(c)).collect() };
    let clipped = Plane { region: wide, data: candidates.par_iter().map(|c| c.map(|v| v.min(1.0))).collect() };
    halation::apply(&clipped, frame, out, &h, &curves).par_iter().map(|&c| curves.saturated_free(c)).collect()
}
