use std::collections::HashMap;
use std::sync::Arc;

use serde::Deserialize;

pub struct CubeLut {
    size: usize,
    min: [f32; 3],
    max: [f32; 3],
    data: Vec<[f32; 3]>,
}

impl CubeLut {
    pub fn parse(text: &str) -> Result<CubeLut, String> {
        let mut size = 0usize;
        let (mut min, mut max) = ([0.0f32; 3], [1.0f32; 3]);
        let mut data = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with("TITLE") {
                continue;
            }
            let mut parts = line.split_whitespace();
            match parts.next() {
                Some("LUT_3D_SIZE") => size = parts.next().and_then(|v| v.parse().ok()).ok_or("bad LUT_3D_SIZE")?,
                Some("LUT_1D_SIZE") => return Err("1D LUTs are not supported".into()),
                Some("DOMAIN_MIN") => min = triple(parts)?,
                Some("DOMAIN_MAX") => max = triple(parts)?,
                Some(first) => {
                    let r: f32 = first.parse().map_err(|_| format!("unexpected line `{line}`"))?;
                    let [g, b] = pair(parts)?;
                    data.push([r, g, b]);
                }
                None => {}
            }
        }
        if size < 2 || data.len() != size * size * size {
            return Err(format!("expected {} entries for size {size}, found {}", size * size * size, data.len()));
        }
        Ok(CubeLut { size, min, max, data })
    }

    fn at(&self, r: usize, g: usize, b: usize) -> [f32; 3] {
        self.data[r + self.size * (g + self.size * b)]
    }

    pub fn apply(&self, c: [f32; 3]) -> [f32; 3] {
        let n = (self.size - 1) as f32;
        let mut base = [0usize; 3];
        let mut frac = [0f32; 3];
        for i in 0..3 {
            let t = ((c[i] - self.min[i]) / (self.max[i] - self.min[i])).clamp(0.0, 1.0) * n;
            let lo = (t.floor() as usize).min(self.size - 2);
            base[i] = lo;
            frac[i] = t - lo as f32;
        }
        let [r, g, b] = base;
        let [fr, fg, fb] = frac;
        let lerp = |a: [f32; 3], z: [f32; 3], t: f32| [a[0] + (z[0] - a[0]) * t, a[1] + (z[1] - a[1]) * t, a[2] + (z[2] - a[2]) * t];
        let c00 = lerp(self.at(r, g, b), self.at(r + 1, g, b), fr);
        let c10 = lerp(self.at(r, g + 1, b), self.at(r + 1, g + 1, b), fr);
        let c01 = lerp(self.at(r, g, b + 1), self.at(r + 1, g, b + 1), fr);
        let c11 = lerp(self.at(r, g + 1, b + 1), self.at(r + 1, g + 1, b + 1), fr);
        lerp(lerp(c00, c10, fg), lerp(c01, c11, fg), fb)
    }
}

fn triple<'a>(mut parts: impl Iterator<Item = &'a str>) -> Result<[f32; 3], String> {
    let mut out = [0.0; 3];
    for v in &mut out {
        *v = parts.next().and_then(|p| p.parse().ok()).ok_or("expected three numbers")?;
    }
    Ok(out)
}

fn pair<'a>(mut parts: impl Iterator<Item = &'a str>) -> Result<[f32; 2], String> {
    let mut out = [0.0; 2];
    for v in &mut out {
        *v = parts.next().and_then(|p| p.parse().ok()).ok_or("expected three numbers per entry")?;
    }
    Ok(out)
}

type Mat3 = [[f64; 3]; 3];

fn mul(a: Mat3, b: Mat3) -> Mat3 {
    let mut o = [[0.0; 3]; 3];
    for (i, row) in o.iter_mut().enumerate() {
        for (j, v) in row.iter_mut().enumerate() {
            *v = (0..3).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    o
}

fn inverse(m: Mat3) -> Mat3 {
    let d = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    let c = |a: usize, b: usize, e: usize, f: usize| m[a][b] * m[e][f] - m[a][f] * m[e][b];
    [
        [c(1, 1, 2, 2) / d, -c(0, 1, 2, 2) / d, c(0, 1, 1, 2) / d],
        [-c(1, 0, 2, 2) / d, c(0, 0, 2, 2) / d, -c(0, 0, 1, 2) / d],
        [c(1, 0, 2, 1) / d, -c(0, 0, 2, 1) / d, c(0, 0, 1, 1) / d],
    ]
}

fn rgb_to_xyz(primaries: [[f64; 2]; 3], white: [f64; 2]) -> Mat3 {
    let xyz = |[x, y]: [f64; 2]| [x / y, 1.0, (1.0 - x - y) / y];
    let p = primaries.map(xyz);
    let m = [[p[0][0], p[1][0], p[2][0]], [p[0][1], p[1][1], p[2][1]], [p[0][2], p[1][2], p[2][2]]];
    let w = xyz(white);
    let inv = inverse(m);
    let s = [0, 1, 2].map(|i| inv[i][0] * w[0] + inv[i][1] * w[1] + inv[i][2] * w[2]);
    [0, 1, 2].map(|i| [m[i][0] * s[0], m[i][1] * s[1], m[i][2] * s[2]])
}

const D65: [f64; 2] = [0.3127, 0.3290];
const REC2020: [[f64; 2]; 3] = [[0.708, 0.292], [0.170, 0.797], [0.131, 0.046]];
const REC709: [[f64; 2]; 3] = [[0.64, 0.33], [0.30, 0.60], [0.15, 0.06]];
const AWG3: [[f64; 2]; 3] = [[0.6840, 0.3130], [0.2210, 0.8480], [0.0861, -0.1020]];

fn to_f32(m: Mat3) -> [[f32; 3]; 3] {
    m.map(|row| row.map(|v| v as f32))
}

fn apply(m: &[[f32; 3]; 3], c: [f32; 3]) -> [f32; 3] {
    [0, 1, 2].map(|i| m[i][0] * c[0] + m[i][1] * c[1] + m[i][2] * c[2])
}

fn logc3(x: f32) -> f32 {
    const CUT: f32 = 0.010_591;
    const A: f32 = 5.555_556;
    const B: f32 = 0.052_272;
    const C: f32 = 0.247_190;
    const D: f32 = 0.385_537;
    const E: f32 = 5.367_655;
    const F: f32 = 0.092_809;
    if x > CUT { C * (A * x + B).log10() + D } else { E * x + F }
}

fn srgb_to_linear(v: f32) -> f32 {
    if v <= 0.040_45 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Halation {
    pub amount: f32,
    pub radius: f32,
    pub threshold: f32,
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Grain {
    pub amount: f32,
    pub size: f32,
    pub color: f32,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct FilmRequest {
    pub negative: Option<String>,
    pub print: Option<String>,
    pub utility: Option<String>,
    pub halation: Halation,
    pub grain: Grain,
    pub auto_scan: bool,
}

impl Default for FilmRequest {
    fn default() -> Self {
        FilmRequest { negative: None, print: None, utility: None, halation: Halation::default(), grain: Grain::default(), auto_scan: true }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Light {
    pub contrast: f32,
    pub whites: f32,
    pub blacks: f32,
}

#[derive(Clone, Copy)]
pub struct Scan {
    lo: f32,
    gain: f32,
    black: f32,
}

const SCAN_WHITE: f32 = 0.92;
const SCAN_BLACK: f32 = 0.03;
const SCAN_MAX_GAIN: f32 = 2.0;
const SCAN_SHOULDER: f32 = 0.86;

impl Scan {
    fn targets(light: Light) -> (f32, f32) {
        let w = light.whites / 100.0;
        let white = if w >= 0.0 { SCAN_WHITE + 0.08 * w } else { SCAN_WHITE + 0.2 * w };
        let black = SCAN_BLACK + 0.05 * light.blacks / 100.0;
        (white, black)
    }

    pub fn measure(lumas: &mut [f32], light: Light, auto: bool) -> Scan {
        let (white, black) = Scan::targets(light);
        let (lo, hi) = if auto && lumas.len() > 16 {
            lumas.sort_by(f32::total_cmp);
            let at = |q: f32| lumas[((lumas.len() - 1) as f32 * q) as usize];
            (at(0.005), at(0.995))
        } else {
            (0.0, white - black)
        };
        let gain = ((white - black) / (hi - lo).max(1e-3)).min(SCAN_MAX_GAIN);
        Scan { lo, gain, black }
    }

    fn apply(&self, display: [f32; 3]) -> [f32; 3] {
        let k = 1.0 - SCAN_SHOULDER;
        display.map(|v| {
            let out = self.black + (v - self.lo) * self.gain;
            if out > SCAN_SHOULDER { SCAN_SHOULDER + k * ((out - SCAN_SHOULDER) / k).tanh() } else { out }.clamp(0.0, 1.0)
        })
    }
}

pub fn contrast(scene: [f32; 3], amount: f32) -> [f32; 3] {
    if amount == 0.0 {
        return scene;
    }
    let c = amount / 100.0;
    let slope = if c >= 0.0 { 1.0 + 0.55 * c } else { 1.0 + 0.4 * c };
    let y = 0.2627 * scene[0] + 0.6780 * scene[1] + 0.0593 * scene[2];
    if y <= 1e-9 {
        return scene;
    }
    let target = 0.18 * ((y / 0.18).log2() * slope).exp2();
    scene.map(|v| v * target / y)
}

#[derive(Default)]
pub struct LutLibrary {
    luts: HashMap<String, Arc<CubeLut>>,
}

impl LutLibrary {
    pub fn insert(&mut self, id: &str, text: &str) -> Result<(), String> {
        let lut = CubeLut::parse(text).map_err(|e| format!("{id}: {e}"))?;
        self.luts.insert(id.to_string(), Arc::new(lut));
        Ok(())
    }

    pub fn has(&self, id: &str) -> bool {
        self.luts.contains_key(id)
    }

    fn get(&self, id: &Option<String>) -> Result<Option<Arc<CubeLut>>, String> {
        match id {
            None => Ok(None),
            Some(id) => self.luts.get(id).cloned().map(Some).ok_or_else(|| format!("film LUT `{id}` isn't loaded")),
        }
    }
}

pub struct FilmChain {
    to_awg: [[f32; 3]; 3],
    from_709: [[f32; 3]; 3],
    utility: Option<Arc<CubeLut>>,
    negative: Arc<CubeLut>,
    print: Arc<CubeLut>,
}

impl FilmChain {
    pub fn new(library: &LutLibrary, request: &FilmRequest) -> Result<Option<FilmChain>, String> {
        let (Some(negative), Some(print)) = (library.get(&request.negative)?, library.get(&request.print)?) else {
            return Ok(None);
        };
        let rec2020 = rgb_to_xyz(REC2020, D65);
        let to_awg = mul(inverse(rgb_to_xyz(AWG3, D65)), rec2020);
        let from_709 = mul(inverse(rec2020), rgb_to_xyz(REC709, D65));
        Ok(Some(FilmChain { to_awg: to_f32(to_awg), from_709: to_f32(from_709), utility: library.get(&request.utility)?, negative, print }))
    }

    pub fn log_of(&self, scene: [f32; 3]) -> [f32; 3] {
        apply(&self.to_awg, scene).map(|v| logc3(v.max(0.0)).clamp(0.0, 1.0))
    }

    pub fn negative_of(&self, log: [f32; 3]) -> [f32; 3] {
        let log = match &self.utility {
            Some(u) => u.apply(log),
            None => log,
        };
        self.negative.apply(log.map(|v| v.clamp(0.0, 1.0)))
    }

    pub fn print_display(&self, negative: [f32; 3]) -> [f32; 3] {
        self.print.apply(negative.map(|v| v.clamp(0.0, 1.0)))
    }

    pub fn linear_of(&self, display: [f32; 3]) -> [f32; 3] {
        apply(&self.from_709, display.map(srgb_to_linear))
    }

    pub fn print_of(&self, negative: [f32; 3]) -> [f32; 3] {
        self.linear_of(self.print_display(negative))
    }
}

use std::sync::atomic::{AtomicU32, Ordering};

use lightcraft_engine::pipeline::film_hook::ToneHook;

pub struct Capture {
    stride: usize,
    data: Vec<AtomicU32>,
}

impl Capture {
    pub fn new(max_w: usize, max_h: usize) -> Capture {
        let n = max_w.max(1) * max_h.max(1) * 3;
        Capture { stride: max_w.max(1), data: (0..n).map(|_| AtomicU32::new(0)).collect() }
    }

    pub fn scene_samples(&self, w: usize, h: usize, count: usize) -> Vec<[f32; 3]> {
        let step = ((w * h) / count.max(1)).max(1);
        (0..w * h)
            .step_by(step)
            .map(|i| {
                let at = ((i / w) * self.stride + i % w) * 3;
                [0, 1, 2].map(|k| f32::from_bits(self.data[at + k].load(Ordering::Relaxed)))
            })
            .collect()
    }

    fn luma_plane(&self, w: usize, h: usize) -> Vec<f32> {
        let mut out = vec![0.0; w * h];
        for y in 0..h {
            for x in 0..w {
                let i = (y * self.stride + x) * 3;
                let [r, g, b] = [0, 1, 2].map(|k| f32::from_bits(self.data[i + k].load(Ordering::Relaxed)));
                out[y * w + x] = 0.2627 * r + 0.6780 * g + 0.0593 * b;
            }
        }
        out
    }
}

impl ToneHook for Capture {
    fn tone(&self, scene: [f32; 3], x: f32, y: f32) -> [f32; 3] {
        let (x, y) = (x as usize, y as usize);
        if x < self.stride {
            let i = (y * self.stride + x) * 3;
            if i + 2 < self.data.len() {
                for (k, v) in scene.iter().enumerate() {
                    self.data[i + k].store(v.to_bits(), Ordering::Relaxed);
                }
            }
        }
        scene.map(|v| v / (1.0 + v))
    }
}

fn box_blur(src: &[f32], w: usize, h: usize, radius: usize) -> Vec<f32> {
    if radius == 0 {
        return src.to_vec();
    }
    let mut tmp = vec![0.0; w * h];
    let mut out = vec![0.0; w * h];
    let norm = 1.0 / (2 * radius + 1) as f32;
    for y in 0..h {
        let row = &src[y * w..(y + 1) * w];
        let mut acc: f32 = (0..=radius).map(|i| row[i.min(w - 1)]).sum::<f32>() + row[0] * radius as f32;
        for x in 0..w {
            tmp[y * w + x] = acc * norm;
            acc += row[(x + radius + 1).min(w - 1)] - row[x.saturating_sub(radius)];
        }
    }
    for x in 0..w {
        let at = |y: usize| tmp[y.min(h - 1) * w + x];
        let mut acc: f32 = (0..=radius).map(at).sum::<f32>() + at(0) * radius as f32;
        for y in 0..h {
            out[y * w + x] = acc * norm;
            acc += at(y + radius + 1) - at(y.saturating_sub(radius));
        }
    }
    out
}

fn gaussian(src: &[f32], w: usize, h: usize, sigma: f32) -> Vec<f32> {
    let r = ((sigma * sigma * 12.0 / 3.0 + 1.0).sqrt() / 2.0).floor() as usize;
    let mut out = src.to_vec();
    for _ in 0..3 {
        out = box_blur(&out, w, h, r);
    }
    out
}

pub struct Glow {
    w: usize,
    h: usize,
    data: Vec<f32>,
    tint: [f32; 3],
}

impl Glow {
    pub fn from_capture(capture: &Capture, w: usize, h: usize, halation: &Halation) -> Option<Glow> {
        if halation.amount <= 0.0 || w < 2 || h < 2 {
            return None;
        }
        let threshold = if halation.threshold > 0.0 { halation.threshold } else { 0.55 };
        let excess: Vec<f32> = capture
            .luma_plane(w, h)
            .into_iter()
            .map(|y| {
                let e = (y - threshold).max(0.0);
                e / (1.0 + e / 6.0)
            })
            .collect();
        let long = w.max(h) as f32;
        let radius = if halation.radius > 0.0 { halation.radius } else { 5.0 };
        let core = gaussian(&excess, w, h, (radius * 0.0007 * long).max(0.5));
        let wide = gaussian(&excess, w, h, (radius * 0.0028 * long).max(1.0));
        let data = core.iter().zip(&wide).map(|(c, d)| halation.amount * (c + 0.35 * d)).collect();
        Some(Glow { w, h, data, tint: [1.0, 0.22, 0.04] })
    }

    fn at(&self, x: f32, y: f32, frame: (usize, usize)) -> [f32; 3] {
        let gx = ((x * self.w as f32 / frame.0.max(1) as f32) as usize).min(self.w - 1);
        let gy = ((y * self.h as f32 / frame.1.max(1) as f32) as usize).min(self.h - 1);
        let v = self.data.get(gy * self.w + gx).copied().unwrap_or(0.0);
        self.tint.map(|t| t * v)
    }
}

fn hash(x: i64, y: i64, channel: u32) -> f32 {
    let mut h = (x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (y as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F) ^ u64::from(channel).wrapping_mul(0x1656_67B1_9E37_79F9);
    h ^= h >> 33;
    h = h.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    h ^= h >> 33;
    h = h.wrapping_mul(0xC4CE_B9FE_1A85_EC53);
    h ^= h >> 33;
    let u = |s: u32| ((h >> s) & 0xFFFF) as f32 / 65535.0;
    (u(0) + u(16) + u(32) + u(48) - 2.0) * 1.732
}

pub struct GrainField {
    amount: f32,
    color: f32,
    cell: f32,
    visible: f32,
}

impl GrainField {
    pub fn new(grain: &Grain, native_long: usize, out_long: usize) -> Option<GrainField> {
        if grain.amount <= 0.0 {
            return None;
        }
        let size = if grain.size > 0.0 { grain.size } else { 1.6 };
        let native_cell = (size * native_long.max(1) as f32 / 6000.0).max(0.75);
        let cell = native_cell * out_long.max(1) as f32 / native_long.max(1) as f32;
        Some(GrainField { amount: grain.amount, color: grain.color.clamp(0.0, 1.0), cell: cell.max(1e-3), visible: cell.min(1.0) })
    }

    fn noise(&self, x: f32, y: f32, channel: u32) -> f32 {
        let (gx, gy) = (x / self.cell, y / self.cell);
        let (x0, y0) = (gx.floor(), gy.floor());
        let (fx, fy) = (gx - x0, gy - y0);
        let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
        let (ix, iy) = (x0 as i64, y0 as i64);
        let a = hash(ix, iy, channel) + (hash(ix + 1, iy, channel) - hash(ix, iy, channel)) * sx;
        let b = hash(ix, iy + 1, channel) + (hash(ix + 1, iy + 1, channel) - hash(ix, iy + 1, channel)) * sx;
        a + (b - a) * sy
    }

    fn apply(&self, negative: [f32; 3], x: f32, y: f32) -> [f32; 3] {
        let luma = 0.2126 * negative[0] + 0.7152 * negative[1] + 0.0722 * negative[2];
        let weight = (1.0 - 0.6 * ((luma - 0.45) / 0.45).clamp(0.0, 1.0)) * (0.6 + 0.4 * (luma / 0.3).clamp(0.0, 1.0));
        let mono = self.noise(x + 0.5, y + 0.5, 0);
        let k = self.amount * self.visible * weight;
        let n = [1u32, 2, 3].map(|c| (1.0 - self.color) * mono + self.color * self.noise(x + 0.5, y + 0.5, c));
        [negative[0] + k * n[0], negative[1] + k * n[1], negative[2] + k * n[2]]
    }
}

pub struct FilmHook {
    pub chain: FilmChain,
    pub glow: Option<Arc<Glow>>,
    pub grain: Option<GrainField>,
    pub light: Light,
    pub scan: Scan,
    pub frame: (usize, usize),
}

impl ToneHook for FilmHook {
    fn tone(&self, scene: [f32; 3], x: f32, y: f32) -> [f32; 3] {
        let scene = match &self.glow {
            Some(g) => {
                let add = g.at(x, y, self.frame);
                [scene[0] + add[0], scene[1] + add[1], scene[2] + add[2]]
            }
            None => scene,
        };
        let scene = contrast(scene, self.light.contrast);
        let negative = self.chain.negative_of(self.chain.log_of(scene));
        let negative = match &self.grain {
            Some(g) => g.apply(negative, x, y),
            None => negative,
        };
        self.chain.linear_of(self.scan.apply(self.chain.print_display(negative)))
    }
}
