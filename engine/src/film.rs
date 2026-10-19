use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;

use serde::Deserialize;

pub fn next_id() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

pub struct CubeLut {
    pub(crate) id: u64,
    pub(crate) size: usize,
    pub(crate) min: [f32; 3],
    pub(crate) max: [f32; 3],
    pub(crate) unit: bool,
    pub(crate) data: Vec<[f32; 3]>,
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
        Ok(CubeLut { id: next_id(), size, min, max, unit: min == [0.0; 3] && max == [1.0; 3], data })
    }

    pub fn apply(&self, c: [f32; 3]) -> [f32; 3] {
        let n = (self.size - 1) as f32;
        let mut base = [0usize; 3];
        let mut frac = [0f32; 3];
        for i in 0..3 {
            // why: (c - 0) / (1 - 0) is c bit for bit, so the unit domain skips the division
            let u = if self.unit { c[i] } else { (c[i] - self.min[i]) / (self.max[i] - self.min[i]) };
            let t = u.clamp(0.0, 1.0) * n;
            let lo = (t.floor() as usize).min(self.size - 2);
            base[i] = lo;
            frac[i] = t - lo as f32;
        }
        let [r, g, b] = base;
        let [fr, fg, fb] = frac;
        let (gs, bs) = (self.size, self.size * self.size);
        let at = r + gs * g + bs * b;
        let d = &self.data[at..=at + bs + gs + 1];
        let lerp = |a: [f32; 3], z: [f32; 3], t: f32| [a[0] + (z[0] - a[0]) * t, a[1] + (z[1] - a[1]) * t, a[2] + (z[2] - a[2]) * t];
        let c00 = lerp(d[0], d[1], fr);
        let c10 = lerp(d[gs], d[gs + 1], fr);
        let c01 = lerp(d[bs], d[bs + 1], fr);
        let c11 = lerp(d[bs + gs], d[bs + gs + 1], fr);
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

const LOG2_STEPS: usize = 4096;
const EXP2_STEPS: usize = 4096;

pub(crate) fn log2_table() -> &'static [f32] {
    static TABLE: std::sync::OnceLock<Vec<f32>> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| (0..=LOG2_STEPS).map(|i| (1.0 + i as f64 / LOG2_STEPS as f64).log2() as f32).collect())
}

pub(crate) fn exp2_table() -> &'static [f32] {
    static TABLE: std::sync::OnceLock<Vec<f32>> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| (0..=EXP2_STEPS).map(|i| (i as f64 / EXP2_STEPS as f64).exp2() as f32).collect())
}

pub(crate) fn fast_log2(x: f32, table: &[f32]) -> f32 {
    let bits = x.to_bits();
    let exponent = ((bits >> 23) & 0xff) as i32 - 127;
    let mantissa = f32::from_bits((bits & 0x007f_ffff) | 0x3f80_0000) - 1.0;
    let t = mantissa * LOG2_STEPS as f32;
    let i = (t as usize).min(LOG2_STEPS - 1);
    exponent as f32 + table[i] + (table[i + 1] - table[i]) * (t - i as f32)
}

fn fast_log10(x: f32, table: &[f32]) -> f32 {
    fast_log2(x, table) * std::f32::consts::LOG10_2
}

// why: libm's exp2/log2 can't be reproduced on the GPU, so contrast uses tables both sides share
pub(crate) fn fast_exp2(x: f32, table: &[f32]) -> f32 {
    let x = x.clamp(-126.0, 126.0);
    let whole = x.floor();
    let t = (x - whole) * EXP2_STEPS as f32;
    let i = (t as usize).min(EXP2_STEPS - 1);
    let scale = f32::from_bits(((whole as i32 + 127) as u32) << 23);
    (table[i] + (table[i + 1] - table[i]) * (t - i as f32)) * scale
}

fn logc3(x: f32, log2: &[f32]) -> f32 {
    const CUT: f32 = 0.010_591;
    const A: f32 = 5.555_556;
    const B: f32 = 0.052_272;
    const C: f32 = 0.247_190;
    const D: f32 = 0.385_537;
    const E: f32 = 5.367_655;
    const F: f32 = 0.092_809;
    if x > CUT { C * fast_log10(A * x + B, log2) + D } else { E * x + F }
}

fn srgb_to_linear(v: f32) -> f32 {
    if v <= 0.040_45 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct GrainRequest {
    pub amount: f32,
    pub size: f32,
    pub softness: f32,
    pub defocus: f32,
}

impl Default for GrainRequest {
    fn default() -> Self {
        GrainRequest { amount: 0.0995, size: 1.0, softness: 0.05, defocus: 1.0 }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct DustRequest {
    pub on: bool,
    pub size: f64,
    pub softness: f64,
}

impl Default for DustRequest {
    fn default() -> Self {
        DustRequest { on: true, size: 1.758, softness: 0.245 }
    }
}

// why: FilmVision's PowerGrades turn Film Look Creator's halation on at amount 0.5, hue 0.45, saturation 1,
// highlights only; the radius follows the gauge (6 / 5 / 4 for 16 / 35 / 65 mm)
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct HalationRequest {
    pub on: bool,
    pub amount: f32,
    pub hue: f32,
    pub sat: f32,
    pub radius: f32,
    pub highlights_only: bool,
}

impl Default for HalationRequest {
    fn default() -> Self {
        HalationRequest { on: true, amount: 0.5, hue: 0.45, sat: 1.0, radius: 4.0, highlights_only: true }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct FilmRequest {
    pub negative: Option<String>,
    pub print: Option<String>,
    pub utility: Option<String>,
    pub texture: bool,
    pub blur: f32,
    pub grain: Option<GrainRequest>,
    pub dust: DustRequest,
    pub halation: HalationRequest,
    pub auto_scan: bool,
}

impl Default for FilmRequest {
    fn default() -> Self {
        FilmRequest { negative: None, print: None, utility: None, texture: true, blur: 0.05, grain: Some(GrainRequest::default()), dust: DustRequest::default(), halation: HalationRequest::default(), auto_scan: true }
    }
}

impl FilmRequest {
    pub fn texture_params(&self, native: Size) -> Option<TextureParams> {
        let grain = self.grain.map(|g| GrainParams { amount: g.amount, size: g.size, softness: g.softness, defocus: g.defocus, seed: 0 });
        let d = self.dust;
        let dust = d.on.then(|| DustParams { size: d.size, softness: d.softness, rows: DustParams::rows_of(native) });
        let h = self.halation;
        let halation = (h.on && h.amount > 0.0).then_some(HalationParams { amount: h.amount, hue: h.hue, sat: h.sat, radius: h.radius, highlights_only: h.highlights_only, ceiling: 1.0 });
        self.texture.then_some(TextureParams { film_blur: self.blur, grain, dust, halation })
    }

    pub fn texture_key(&self) -> String {
        format!("{}{:?}{:?}{:?}{:?}", self.texture, self.blur, self.grain, self.dust, self.halation)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Light {
    pub contrast: f32,
    pub whites: f32,
    pub blacks: f32,
}

#[derive(Clone)]
pub struct Scan {
    pub(crate) table: Arc<Vec<f32>>,
}

const SCAN_STEPS: usize = 8192;

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
            let mut at = |q: f32| {
                let k = ((lumas.len() - 1) as f32 * q) as usize;
                *lumas.select_nth_unstable_by(k, f32::total_cmp).1
            };
            (at(0.005), at(0.995))
        } else {
            (0.0, white - black)
        };
        let gain = ((white - black) / (hi - lo).max(1e-3)).min(SCAN_MAX_GAIN);
        let k = 1.0 - SCAN_SHOULDER;
        let table = (0..=SCAN_STEPS)
            .map(|i| {
                let v = i as f32 / SCAN_STEPS as f32;
                let out = black + (v - lo) * gain;
                let out = if out > SCAN_SHOULDER { SCAN_SHOULDER + k * ((out - SCAN_SHOULDER) / k).tanh() } else { out };
                srgb_to_linear(out.clamp(0.0, 1.0))
            })
            .collect();
        Scan { table: Arc::new(table) }
    }

    fn linear(&self, display: [f32; 3]) -> [f32; 3] {
        display.map(|v| {
            let t = v.clamp(0.0, 1.0) * SCAN_STEPS as f32;
            let i = (t as usize).min(SCAN_STEPS - 1);
            self.table[i] + (self.table[i + 1] - self.table[i]) * (t - i as f32)
        })
    }
}

pub fn contrast_slope(amount: f32) -> f32 {
    let c = amount / 100.0;
    if c >= 0.0 { 1.0 + 0.55 * c } else { 1.0 + 0.4 * c }
}

#[derive(Default)]
pub struct LutLibrary {
    luts: HashMap<String, Arc<CubeLut>>,
}

// why: every engine (the loupe's and each thumbnail indexer's) loads the same stocks; they share one parsed
// table, and so one copy on the GPU, while any of them holds it
static PARSED: std::sync::LazyLock<std::sync::Mutex<HashMap<u64, std::sync::Weak<CubeLut>>>> = std::sync::LazyLock::new(Default::default);

fn shared_lut(text: &str) -> Result<Arc<CubeLut>, String> {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut hasher);
    let key = hasher.finish();
    let mut parsed = PARSED.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(lut) = parsed.get(&key).and_then(std::sync::Weak::upgrade) {
        return Ok(lut);
    }
    let lut = Arc::new(CubeLut::parse(text)?);
    parsed.retain(|_, l| l.strong_count() > 0);
    parsed.insert(key, Arc::downgrade(&lut));
    Ok(lut)
}

impl Drop for CubeLut {
    fn drop(&mut self) {
        crate::gpu::forget_lut(self.id);
    }
}

impl LutLibrary {
    pub fn insert(&mut self, id: &str, text: &str) -> Result<(), String> {
        let lut = shared_lut(text).map_err(|e| format!("{id}: {e}"))?;
        self.luts.insert(id.to_string(), lut);
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
    pub(crate) to_awg: [[f32; 3]; 3],
    pub(crate) from_709: [[f32; 3]; 3],
    pub(crate) utility: Option<Arc<CubeLut>>,
    pub(crate) negative: Arc<CubeLut>,
    pub(crate) print: Arc<CubeLut>,
    pub(crate) log2: &'static [f32],
    pub(crate) exp2: &'static [f32],
}

impl FilmChain {
    pub fn new(library: &LutLibrary, request: &FilmRequest) -> Result<Option<FilmChain>, String> {
        let (Some(negative), Some(print)) = (library.get(&request.negative)?, library.get(&request.print)?) else {
            return Ok(None);
        };
        let rec2020 = rgb_to_xyz(REC2020, D65);
        let to_awg = mul(inverse(rgb_to_xyz(AWG3, D65)), rec2020);
        let from_709 = mul(inverse(rec2020), rgb_to_xyz(REC709, D65));
        Ok(Some(FilmChain { to_awg: to_f32(to_awg), from_709: to_f32(from_709), utility: library.get(&request.utility)?, negative, print, log2: log2_table(), exp2: exp2_table() }))
    }

    pub fn contrast(&self, scene: [f32; 3], amount: f32) -> [f32; 3] {
        if amount == 0.0 {
            return scene;
        }
        let slope = contrast_slope(amount);
        let y = 0.2627 * scene[0] + 0.6780 * scene[1] + 0.0593 * scene[2];
        if y <= 1e-9 {
            return scene;
        }
        let target = 0.18 * fast_exp2(fast_log2(y / 0.18, self.log2) * slope, self.exp2);
        scene.map(|v| v * target / y)
    }

    pub fn log_of(&self, scene: [f32; 3]) -> [f32; 3] {
        apply(&self.to_awg, scene).map(|v| logc3(v.max(0.0), self.log2).clamp(0.0, 1.0))
    }

    fn utility_of(&self, log: [f32; 3]) -> [f32; 3] {
        match &self.utility {
            Some(u) => u.apply(log),
            None => log,
        }
    }

    pub fn negative_of(&self, log: [f32; 3]) -> [f32; 3] {
        self.negative.apply(self.utility_of(log).map(|v| v.clamp(0.0, 1.0)))
    }

    pub fn code_of(&self, scene: [f32; 3], contrast: f32) -> [f32; 3] {
        self.utility_of(self.log_of(self.contrast(scene, contrast)))
    }

    // why: where the frame's highlights top out for the halation (halation::Headroom): its brightest 0.05%, so a few
    // hot pixels don't set it, and never under three stops over grey, so an overcast or dim frame's brightest parts
    // don't start to glow
    pub fn ceiling(&self, scene: &Scene, contrast: f32) -> f32 {
        const SAMPLES: usize = 200_000;
        let top = |c: [f32; 3]| {
            let k = self.code_of(c, contrast);
            k[0].max(k[1]).max(k[2])
        };
        let step = (scene.rgb.len() / SAMPLES).max(1);
        let mut codes: Vec<f32> = scene.rgb.par_iter().step_by(step).map(|&c| top(c)).collect();
        if codes.is_empty() {
            return 1.0;
        }
        let at = ((codes.len() - 1) as f64 * 0.9995) as usize;
        // why: the value at `at` is the `keep`-th largest, which is the `keep`-th largest of every part's `keep`
        // largest too: those are picked from the parts in parallel, and it from them
        let keep = codes.len() - at;
        let mut tops: Vec<f32> = codes
            .par_chunks_mut(16_384)
            .flat_map_iter(|part| {
                let n = part.len();
                if n > keep {
                    part.select_nth_unstable_by(n - keep, f32::total_cmp);
                }
                part[n.saturating_sub(keep)..].to_vec()
            })
            .collect();
        let rank = tops.len() - keep;
        let (_, brightest, _) = tops.select_nth_unstable_by(rank, f32::total_cmp);
        brightest.max(top([0.18 * 8.0; 3])).min(1.0)
    }

    pub fn display_of(&self, code: [f32; 3]) -> [f32; 3] {
        self.print_display(self.negative.apply(code.map(|v| v.clamp(0.0, 1.0))))
    }

    pub fn display_luma(&self, code: [f32; 3]) -> f32 {
        let d = self.display_of(code);
        0.2126 * d[0] + 0.7152 * d[1] + 0.0722 * d[2]
    }

    pub fn print_display(&self, negative: [f32; 3]) -> [f32; 3] {
        self.print.apply(negative.map(|v| v.clamp(0.0, 1.0)))
    }


    pub(crate) fn to_working(&self, linear_709: [f32; 3]) -> [f32; 3] {
        apply(&self.from_709, linear_709)
    }

}

use lightcraft_engine::pipeline::film_hook::{Pixel, ToneHook};
use lightcraft_engine::pipeline::masks::Evaluated;
use rayon::prelude::*;

use crate::dust::DustParams;
use crate::halation::HalationParams;
use crate::grain::GrainParams;
use crate::texture::{self, Plane, Region, Size, TextureParams};

pub struct Capture {
    region: Region,
    captured: std::sync::OnceLock<Captured>,
    masks: std::sync::OnceLock<Arc<Vec<Evaluated>>>,
}

#[derive(Clone)]
struct Captured {
    rgb: Vec<[f32; 3]>,
    width: usize,
    origin: [usize; 2],
    frame: [f32; 2],
}

impl Capture {
    pub fn new(region: Region) -> Capture {
        Capture { region, captured: std::sync::OnceLock::new(), masks: std::sync::OnceLock::new() }
    }

    // why: the pass hands over its scene in one buffer, which is the region's scene itself unless spots grew the
    // rendered window; then the region is cut from it (zero where nothing was drawn, as an untouched pixel was)
    pub fn into_scene(self) -> Scene {
        let r = self.region;
        let masks = self.masks.into_inner();
        let Some(c) = self.captured.into_inner().filter(|c| !c.rgb.is_empty()) else {
            return Scene { id: next_id(), region: r, frame: Size { w: r.w, h: r.h }, rgb: vec![[0.0; 3]; r.w * r.h], masks };
        };
        let frame = Size { w: c.frame[0] as usize, h: c.frame[1] as usize };
        let height = c.rgb.len() / c.width;
        let [ox, oy] = c.origin;
        let rgb = if [ox, oy] == [r.x, r.y] && (c.width, height) == (r.w, r.h) {
            c.rgb
        } else {
            let mut rgb = vec![[0.0f32; 3]; r.w * r.h];
            rgb.par_chunks_mut(r.w.max(1)).enumerate().for_each(|(j, row)| {
                let y = r.y + j;
                if y < oy || y >= oy + height {
                    return;
                }
                let drawn = &c.rgb[(y - oy) * c.width..][..c.width];
                for (k, px) in row.iter_mut().enumerate() {
                    let x = r.x + k;
                    if x >= ox && x < ox + c.width {
                        *px = drawn[x - ox];
                    }
                }
            });
            rgb
        };
        Scene { id: next_id(), region: r, frame, rgb, masks }
    }

    pub fn scene(&self) -> Scene {
        Capture { region: self.region, captured: self.captured.clone(), masks: self.masks.clone() }.into_scene()
    }
}

impl ToneHook for Capture {
    fn tone(&self, scene: [f32; 3], _: Pixel) -> [f32; 3] {
        scene
    }

    fn capture_only(&self) -> bool {
        true
    }

    fn capture(&self, scene: Vec<[f32; 3]>, width: usize, origin: [f32; 2], frame: [f32; 2]) {
        let _ = self.captured.set(Captured { rgb: scene, width, origin: origin.map(|v| v as usize), frame });
    }

    fn captured_masks(&self, masks: Arc<Vec<Evaluated>>) {
        let _ = self.masks.set(masks);
    }
}

pub struct Scene {
    pub(crate) id: u64,
    pub(crate) region: Region,
    pub(crate) frame: Size,
    pub(crate) rgb: Vec<[f32; 3]>,
    /// The masks the capture's pass evaluated (their alphas), for the final pass of the same render.
    pub(crate) masks: Option<Arc<Vec<Evaluated>>>,
}

impl Drop for Scene {
    fn drop(&mut self) {
        crate::gpu::forget_scene(self.id);
    }
}

impl Scene {
    pub fn frame(&self) -> Size {
        self.frame
    }

    pub fn part(&self, region: Region) -> Scene {
        let r = self.region;
        // why: a part as wide as the scene (an export band) is one run of it, copied in parallel; otherwise row slices
        // are copied into a buffer sized up front (collecting a flat_map grew the vector pixel by pixel)
        if (region.x, region.w) == (r.x, r.w) {
            let rgb = self.rgb[(region.y - r.y) * r.w..][..region.w * region.h].par_iter().copied().collect();
            return Scene { id: next_id(), region, frame: self.frame, rgb, masks: None };
        }
        let mut rgb = Vec::with_capacity(region.w * region.h);
        for y in region.y..region.y + region.h {
            rgb.extend_from_slice(&self.rgb[(y - r.y) * r.w + region.x - r.x..][..region.w]);
        }
        Scene { id: next_id(), region, frame: self.frame, rgb, masks: None }
    }
}

pub struct Textured {
    pub(crate) id: u64,
    pub(crate) region: Region,
    pub(crate) rgb: Vec<[f32; 3]>,
    pub(crate) gpu: bool,
    pub(crate) lumas: Option<((u64, u64), Vec<f32>)>,
}

impl Drop for Textured {
    fn drop(&mut self) {
        if self.gpu {
            crate::gpu::forget_texture(self.id);
        }
    }
}

impl Textured {
    fn samples(&self, count: usize) -> impl Iterator<Item = [f32; 3]> + '_ {
        self.rgb.iter().step_by((self.rgb.len() / count.max(1)).max(1)).copied()
    }
}

fn crop(plane: &Plane<[f32; 3]>, out: Region) -> Vec<[f32; 3]> {
    let r = plane.region;
    (out.y..out.y + out.h).flat_map(|y| (out.x..out.x + out.w).map(move |x| plane.data[(y - r.y) * r.w + x - r.x])).collect()
}

pub struct Developer {
    pub chain: FilmChain,
    pub light: Light,
    pub texture: Option<TextureParams>,
}

impl Developer {
    pub fn textured(&self, scene: &Scene, out: Region) -> Textured {
        if let Some((mut t, lumas)) = crate::gpu::textured(self, scene, out, true) {
            t.lumas = lumas.map(|l| (self.stocks(), l));
            return t;
        }
        self.textured_on_cpu(scene, out)
    }

    // why: an export band takes its scan from the whole frame, so it reads nothing back and the GPU runs on while
    // the next band is prepared
    pub fn band(&self, scene: &Scene, out: Region) -> Textured {
        match crate::gpu::textured(self, scene, out, false) {
            Some((t, _)) => t,
            None => self.textured_on_cpu(scene, out),
        }
    }

    pub fn textured_on_cpu(&self, scene: &Scene, out: Region) -> Textured {
        let code = Plane { region: scene.region, data: scene.rgb.par_iter().map(|c| self.chain.code_of(*c, self.light.contrast)).collect() };
        let rgb = match &self.texture {
            Some(p) => texture::texture(code, scene.frame, out, p),
            None => crop(&code, out),
        };
        Textured { id: next_id(), region: out, rgb, gpu: false, lumas: None }
    }

    fn stocks(&self) -> (u64, u64) {
        (self.chain.negative.id, self.chain.print.id)
    }

    pub fn scan(&self, textured: &Textured, auto: bool) -> Option<Scan> {
        let mut lumas = match (&textured.lumas, textured.gpu) {
            (Some((stocks, l)), true) if *stocks == self.stocks() => l.clone(),
            (_, true) => crate::gpu::lumas(self, textured)?,
            _ => {
                let samples: Vec<[f32; 3]> = textured.samples(60_000).collect();
                samples.par_iter().map(|c| self.chain.display_luma(*c)).collect()
            }
        };
        Some(Scan::measure(&mut lumas, self.light, auto))
    }

    pub fn finish(&self, textured: &Textured, scan: &Scan) -> Option<Toned> {
        if textured.gpu {
            return crate::gpu::finish(self, textured, scan);
        }
        let rgb = textured.rgb.par_iter().map(|c| self.toned(*c, scan)).collect();
        Some(Toned { region: textured.region, rgb, masks: None, band: false })
    }

    /// [`Developer::finish`] into `out`, the region's pixels row by row (an export band's rows of the frame).
    pub fn finish_into(&self, textured: &Textured, scan: &Scan, out: &mut [[f32; 3]]) -> bool {
        if textured.gpu {
            return crate::gpu::finish_into(self, textured, scan, out);
        }
        if out.len() != textured.rgb.len() {
            return false;
        }
        out.par_iter_mut().zip(textured.rgb.par_iter()).for_each(|(o, c)| *o = self.toned(*c, scan));
        true
    }

    fn toned(&self, code: [f32; 3], scan: &Scan) -> [f32; 3] {
        self.chain.to_working(scan.linear(self.chain.display_of(code)))
    }
}

pub struct Toned {
    pub(crate) region: Region,
    pub(crate) rgb: Vec<[f32; 3]>,
    /// The masks the scene's capture evaluated, handed to the final pass ([`ToneHook::masks`]).
    pub(crate) masks: Option<Arc<Vec<Evaluated>>>,
    /// The region is a band of the frame's rows, and the final pass makes only those ([`ToneHook::rows`]).
    pub(crate) band: bool,
}

impl ToneHook for Toned {
    fn tone(&self, _: [f32; 3], at: Pixel) -> [f32; 3] {
        let r = self.region;
        let x = (at.x as usize).clamp(r.x, r.x + r.w - 1) - r.x;
        let y = (at.y as usize).clamp(r.y, r.y + r.h - 1) - r.y;
        self.rgb[y * r.w + x]
    }

    fn precomputed(&self) -> bool {
        true
    }

    fn masks(&self) -> Option<Arc<Vec<Evaluated>>> {
        self.masks.clone()
    }

    fn rows(&self) -> Option<std::ops::Range<usize>> {
        self.band.then(|| self.region.y..self.region.y + self.region.h)
    }

    // why: `tone` along a row: pixels left of the region repeat its first, those past it its last, the rest are the
    // row's own, copied at once
    fn tone_row(&self, at: Pixel, len: usize, out: &mut Vec<[f32; 3]>) {
        let r = self.region;
        let y = (at.y as usize).clamp(r.y, r.y + r.h - 1) - r.y;
        let row = &self.rgb[y * r.w..][..r.w];
        let x = at.x as usize;
        let before = r.x.saturating_sub(x).min(len);
        let from = (x + before).saturating_sub(r.x).min(r.w);
        let inside = (len - before).min(r.w - from);
        out.extend(std::iter::repeat_n(row[0], before));
        out.extend_from_slice(&row[from..from + inside]);
        out.extend(std::iter::repeat_n(row[r.w - 1], len - before - inside));
    }
}
