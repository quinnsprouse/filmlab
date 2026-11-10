use std::sync::{Arc, Mutex, OnceLock};

use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::film::{exp2_table, fast_exp2, fast_log2, log2_table};
use crate::grain_tables::{DOMAIN, SPECTRUM, SPECTRUM_SIZES};
use crate::texture::{Region, Size, luma};

// why: measured from Resolve renders of FilmVision's grain (scratchpad flc/grain): noise strength per unit of
// grainAmount, the field's mean offset, its sub-Gaussian marginal, and the softness and defocus blur scales
pub(crate) const AMPLITUDE: f32 = 0.3405;
pub(crate) const OFFSET: f32 = 0.107;
pub(crate) const FLATTEN: f32 = 0.003_703_8;
pub(crate) const FLATTEN_NORM: f32 = 0.988_68;
const SOFT_PER_SHORT_EDGE: f32 = 0.00463;
const DEFOCUS_PER_SHORT_EDGE: f32 = 0.00297;
pub(crate) const DOMAIN_STEPS: f32 = 5000.0;
pub(crate) const DOMAIN_TOP: f32 = 1.25;
pub(crate) const MINUS_TWO_LN2: f32 = -1.386_294_4;
pub(crate) const COS_STEPS: usize = 4096;
const SPECTRUM_STEP: f64 = 5.0;
const ENERGY_LEFT: f64 = 1e-4;
const MAX_RADIUS: usize = 64;
const RANK: usize = 3;
const KERNELS_KEPT: usize = 8;
// why: fitted to the luma spectra of 35 mm Portra 400, Gold 200 and CineStill 800T scans with each scan's pixel
// aperture allowed for (.scratch/film-study/grain/fit.py): a dye cloud and a clump as (peak power, sigma in cycles per
// frame short edge) at size 1, a 36x24 mm frame. The layers' own grain is coarser, and stills amounts are the grain's
// strength sampled at 4000 px on the short edge
const CLOUDS: [(f64, f64); 2] = [(1.0, 1083.27), (0.4401, 123.99)];
const LAYER_STRETCH: f64 = 2.5;
const REFERENCE: f64 = 4000.0;
const ALIASES: i32 = 24;
const FADE_FROM: f32 = 0.42;
const FADE_RATE: f32 = 10.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Structure {
    #[default]
    Cine,
    Stills,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GrainParams {
    pub amount: f32,
    pub size: f32,
    pub softness: f32,
    pub defocus: f32,
    pub structure: Structure,
    pub color: f32,
    pub highlights: f32,
    pub seed: u32,
}

impl GrainParams {
    pub(crate) fn scale(&self) -> f32 {
        AMPLITUDE * self.amount
    }

    pub(crate) fn grains(&self) -> bool {
        self.amount > 0.0
    }

    pub(crate) fn grains_in_finish(&self) -> bool {
        self.structure == Structure::Stills && self.grains()
    }

    pub(crate) fn textured_part(self) -> GrainParams {
        match self.structure {
            Structure::Cine => GrainParams { color: 0.0, highlights: 1.0, ..self },
            Structure::Stills => GrainParams { amount: 0.0, size: 0.0, softness: 0.0, color: 0.0, highlights: 1.0, ..self },
        }
    }
}

pub(crate) fn cos_table() -> &'static [f32] {
    static TABLE: OnceLock<Vec<f32>> = OnceLock::new();
    TABLE.get_or_init(|| (0..=COS_STEPS).map(|i| (std::f64::consts::TAU * i as f64 / COS_STEPS as f64).cos() as f32).collect())
}

pub(crate) fn forward(v: f32) -> f32 {
    let t = v.clamp(0.0, DOMAIN_TOP) * DOMAIN_STEPS;
    let i = (t as usize).min(DOMAIN.len() - 2);
    DOMAIN[i] + (DOMAIN[i + 1] - DOMAIN[i]) * (t - i as f32)
}

pub(crate) fn inverse(u: f32) -> f32 {
    let last = DOMAIN.len() - 1;
    if u <= DOMAIN[0] {
        return 0.0;
    }
    if u >= DOMAIN[last] {
        return DOMAIN_TOP;
    }
    let i = DOMAIN.partition_point(|&d| d <= u).clamp(1, last) - 1;
    let f = (u - DOMAIN[i]) / (DOMAIN[i + 1] - DOMAIN[i]);
    (i as f32 + f) / DOMAIN_STEPS
}

fn gaussian(sigma: f64, radius: usize) -> Vec<f32> {
    let raw: Vec<f64> = (-(radius as isize)..=radius as isize).map(|k| (-0.5 * (k * k) as f64 / (sigma * sigma)).exp()).collect();
    let total: f64 = raw.iter().sum();
    raw.iter().map(|w| (w / total) as f32).collect()
}

pub(crate) fn defocus_weights(frame: Size, p: &GrainParams) -> Vec<f32> {
    let sigma = f64::from(DEFOCUS_PER_SHORT_EDGE * frame.short_edge() as f32 * (1.0 - p.defocus).max(0.0));
    if sigma < 0.05 { Vec::new() } else { gaussian(sigma, (6.0 * sigma + 0.5) as usize) }
}

pub(crate) fn soft_weights(frame: Size, p: &GrainParams) -> Vec<f32> {
    let sigma = f64::from(SOFT_PER_SHORT_EDGE * frame.short_edge() as f32 * p.softness.max(0.0).powf(1.5));
    if sigma <= 0.12 { Vec::new() } else { gaussian(sigma, (4.5 * sigma).ceil() as usize + 1) }
}

pub(crate) fn defocus_radius(frame: Size, p: &GrainParams) -> usize {
    defocus_weights(frame, p).len() / 2
}

pub(crate) fn reflect(i: isize, n: usize) -> usize {
    let n = n as isize;
    let r = if i < 0 { -i - 1 } else if i >= n { 2 * n - i - 1 } else { i };
    r.clamp(0, n - 1) as usize
}

pub(crate) fn pcg(v: u32) -> u32 {
    let state = v.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    let word = ((state >> ((state >> 28) + 4)) ^ state).wrapping_mul(277_803_737);
    (word >> 22) ^ word
}

fn uniform(ix: i32, iy: i32, seed: u32) -> f32 {
    let h = pcg((ix as u32).wrapping_add(pcg((iy as u32).wrapping_add(pcg(seed)))));
    ((h >> 9) as f32 + 0.5) * (1.0 / 8_388_608.0)
}

pub(crate) struct Tables<'a> {
    pub(crate) log2: &'a [f32],
    pub(crate) exp2: &'a [f32],
    pub(crate) cos: &'a [f32],
}

impl Tables<'_> {
    // why: Box–Muller through the shared log2/exp2/cos tables, so the GPU draws the same noise bit for bit
    fn white(&self, ix: i32, iy: i32, seed: u32) -> f32 {
        let u1 = uniform(ix, iy, seed.wrapping_mul(2).wrapping_add(1));
        let u2 = uniform(ix, iy, seed.wrapping_mul(2).wrapping_add(2));
        let radius = fast_exp2(fast_log2(MINUS_TWO_LN2 * fast_log2(u1, self.log2), self.log2) * 0.5, self.exp2);
        let t = u2 * COS_STEPS as f32;
        let i = (t as usize).min(COS_STEPS - 1);
        radius * (self.cos[i] + (self.cos[i + 1] - self.cos[i]) * (t - i as f32))
    }
}

pub(crate) fn flatten(z: f32) -> f32 {
    (z - FLATTEN * (z * z * z)) / FLATTEN_NORM
}

#[derive(Clone, Copy)]
pub(crate) struct Mix {
    scale: f32,
    offset: f32,
    highlights: f32,
    shared: f32,
    layer: f32,
}

impl Mix {
    fn new(frame: Size, p: &GrainParams) -> Mix {
        let (offset, highlights, shared, layer) = match p.structure {
            Structure::Cine => (OFFSET, 1.0, 1.0, 0.0),
            Structure::Stills => {
                let color = p.color.clamp(0.0, 1.0);
                let (shared, layer) = (sampled(frame.short_edge(), p.size, 1.0), sampled(frame.short_edge(), p.size, LAYER_STRETCH));
                (0.0, p.highlights, shared * (1.0 - color * color).sqrt(), layer * color)
            }
        };
        Mix { scale: p.scale(), offset, highlights, shared, layer }
    }

    fn channels(&self, shared: f32, layers: Option<[f32; 3]>) -> [f32; 3] {
        let z0 = self.shared * shared;
        layers.map_or([z0; 3], |l| l.map(|zl| z0 + self.layer * zl))
    }

    pub(crate) fn grained(&self, c: [f32; 3], z: [f32; 3]) -> [f32; 3] {
        let fade = 1.0 + (self.highlights - 1.0) * ((luma(c) - FADE_FROM) * FADE_RATE).clamp(0.0, 1.0);
        let a = self.scale * fade;
        [0, 1, 2].map(|k| inverse(forward(c[k]) * (1.0 + a * (z[k] - self.offset))).clamp(0.0, 1.0))
    }

    pub(crate) fn symmetric_display(&self, c: [f32; 3], z: [f32; 3], display: impl Fn([f32; 3]) -> [f32; 3]) -> [f32; 3] {
        let (plain, up, down) = (display(c), display(self.grained(c, z)), display(self.grained(c, z.map(|v| -v))));
        [0, 1, 2].map(|k| plain[k] + (up[k] - down[k]) * 0.5)
    }

    pub(crate) fn words(&self) -> [f32; 6] {
        [self.offset, self.highlights, self.shared, self.layer, FADE_FROM, FADE_RATE]
    }
}

pub struct Kernel {
    pub(crate) radius: usize,
    pub(crate) a: [Vec<f32>; RANK],
    pub(crate) b: [Vec<f32>; RANK],
}

impl Kernel {
    pub(crate) fn words(&self) -> Vec<f32> {
        (0..RANK).flat_map(|k| self.a[k].iter().chain(&self.b[k]).copied()).collect()
    }
}

fn spectrum_row(size: f64) -> Vec<f64> {
    let sizes: Vec<f64> = SPECTRUM_SIZES.iter().map(|&s| f64::from(s)).collect();
    let size = size.clamp(sizes[0], sizes[sizes.len() - 1]);
    let i = (sizes.partition_point(|&s| s <= size).max(1) - 1).min(sizes.len() - 2);
    let t = (size - sizes[i]) / (sizes[i + 1] - sizes[i]);
    SPECTRUM[i].iter().zip(&SPECTRUM[i + 1]).map(|(&a, &b)| (1.0 - t) * f64::from(a) + t * f64::from(b)).collect()
}

fn spectrum_at(row: &[f64], nu: f64) -> f64 {
    let top = (row.len() - 1) as f64 * SPECTRUM_STEP;
    if nu > top {
        return 0.0;
    }
    let t = nu / SPECTRUM_STEP;
    let i = (t as usize).min(row.len() - 2);
    row[i] + (row[i + 1] - row[i]) * (t - i as f64)
}

fn eigen(mut a: Vec<f64>, n: usize) -> (Vec<f64>, Vec<f64>) {
    let mut v = vec![0.0; n * n];
    for i in 0..n {
        v[i * n + i] = 1.0;
    }
    for _ in 0..60 {
        let off: f64 = (0..n).flat_map(|i| (0..n).filter(move |&j| j != i).map(move |j| (i, j))).map(|(i, j)| a[i * n + j] * a[i * n + j]).sum();
        if off < 1e-30 {
            break;
        }
        for p in 0..n {
            for q in p + 1..n {
                let apq = a[p * n + q];
                if apq.abs() < 1e-300 {
                    continue;
                }
                let theta = (a[q * n + q] - a[p * n + p]) / (2.0 * apq);
                let t = theta.signum() / (theta.abs() + (theta * theta + 1.0).sqrt());
                let t = if theta == 0.0 { 1.0 } else { t };
                let c = 1.0 / (t * t + 1.0).sqrt();
                let s = t * c;
                for k in 0..n {
                    let (akp, akq) = (a[k * n + p], a[k * n + q]);
                    a[k * n + p] = c * akp - s * akq;
                    a[k * n + q] = s * akp + c * akq;
                }
                for k in 0..n {
                    let (apk, aqk) = (a[p * n + k], a[q * n + k]);
                    a[p * n + k] = c * apk - s * aqk;
                    a[q * n + k] = s * apk + c * aqk;
                }
                for k in 0..n {
                    let (vkp, vkq) = (v[k * n + p], v[k * n + q]);
                    v[k * n + p] = c * vkp - s * vkq;
                    v[k * n + q] = s * vkp + c * vkq;
                }
            }
        }
    }
    ((0..n).map(|i| a[i * n + i]).collect(), v)
}

fn grid(length: usize) -> usize {
    if length >= 1400 { 512 } else { 256 }
}

// why: the cine grain's spectrum is FilmVision's, folded for point sampling at this short edge (singular values of its
// kernel 1 : 0.12 : 0.01)
fn cine_spectrum(length: usize, size: f32) -> Vec<f64> {
    let h = length as f64;
    let n = grid(length);
    let half = n / 2;
    let row = spectrum_row(f64::from(size));
    let top = (row.len() - 1) as f64 * SPECTRUM_STEP;
    let fold = ((top / h + 0.5) as isize).min(6);
    (0..=half)
        .into_par_iter()
        .flat_map_iter(|b| (0..=half).map(move |a| (a, b)))
        .map(|(a, b)| {
            let (fx, fy) = (a as f64 / n as f64, b as f64 / n as f64);
            let mut s = 0.0;
            for m in -fold..=fold {
                for k in -fold..=fold {
                    s += spectrum_at(&row, h * (fx + m as f64).hypot(fy + k as f64));
                }
            }
            s
        })
        .collect()
}

fn sinc2(f: f64) -> f64 {
    if f == 0.0 {
        return 1.0;
    }
    let x = std::f64::consts::PI * f;
    (x.sin() / x).powi(2)
}

fn cloud_axes(n: usize, length: f64, size: f32, stretch: f64) -> Vec<(f64, Vec<f64>)> {
    CLOUDS
        .iter()
        .map(|&(power, sigma)| {
            let width = sigma / (f64::from(size) * stretch);
            let axis = (0..=n / 2)
                .map(|u| {
                    (-ALIASES..=ALIASES)
                        .map(|m| {
                            let f = u as f64 / n as f64 + f64::from(m);
                            (-0.5 * (f * length / width).powi(2)).exp() * sinc2(f)
                        })
                        .sum()
                })
                .collect();
            (power, axis)
        })
        .collect()
}

fn stills_spectrum(length: usize, size: f32, stretch: f64) -> Vec<f64> {
    let n = grid(length);
    let axes = cloud_axes(n, length as f64, size, stretch);
    (0..=n / 2).flat_map(|b| (0..=n / 2).map(move |a| (a, b))).map(|(a, b)| axes.iter().map(|(power, axis)| power * axis[a] * axis[b]).sum()).collect()
}

fn variance(length: f64, size: f32, stretch: f64) -> f64 {
    let n = 256;
    let mean = |axis: &[f64]| (axis[0] + axis[n / 2] + 2.0 * axis[1..n / 2].iter().sum::<f64>()) / n as f64;
    length * length * cloud_axes(n, length, size, stretch).iter().map(|(power, axis)| power * mean(axis).powi(2)).sum::<f64>()
}

fn sampled(length: usize, size: f32, stretch: f64) -> f32 {
    (variance(length as f64, size, stretch) / variance(REFERENCE, size, stretch)).sqrt() as f32
}

fn build(n: usize, quarter: &[f64]) -> Kernel {
    let half = n / 2;
    let fold_index = |u: usize| u.min(n - u);
    let at = |u: usize, v: usize| quarter[fold_index(v) * (half + 1) + fold_index(u)];
    let mean = (0..n).flat_map(|v| (0..n).map(move |u| (u, v))).map(|(u, v)| at(u, v)).sum::<f64>() / (n * n) as f64;
    let cosn: Vec<f64> = (0..n).map(|k| (std::f64::consts::TAU * k as f64 / n as f64).cos()).collect();
    let columns = MAX_RADIUS + 1;
    let t: Vec<f64> = (0..n)
        .into_par_iter()
        .flat_map_iter(|v| {
            let k: Vec<f64> = (0..n).map(|u| (at(u, v) / mean).sqrt()).collect();
            let cosn = &cosn;
            (0..columns).map(move |x| k.iter().enumerate().map(|(u, kv)| kv * cosn[(u * x) % n]).sum::<f64>())
        })
        .collect();
    let n3 = (n * n * n) as f64;
    let energy: Vec<f64> = (0..columns).map(|x| (0..n).map(|v| t[v * columns + x].powi(2)).sum::<f64>() / n3).collect();
    let mut cum = 0.0;
    let mut radius = MAX_RADIUS + 1;
    for (x, e) in energy.iter().enumerate() {
        cum += if x == 0 { *e } else { 2.0 * e };
        if cum >= 1.0 - ENERGY_LEFT {
            radius = x + 1;
            break;
        }
    }
    let radius = radius.clamp(2, MAX_RADIUS);
    let n2 = (n * n) as f64;
    let side = 2 * radius + 1;
    let hv = |y: usize, x: usize| (0..n).map(|v| t[v * columns + x] * cosn[(v * y) % n]).sum::<f64>() / n2;
    let corner: Vec<f64> = (0..=radius).flat_map(|y| (0..=radius).map(move |x| (y, x))).map(|(y, x)| hv(y, x)).collect();
    let ker: Vec<f64> = (0..side * side)
        .map(|i| {
            let (y, x) = ((i / side).abs_diff(radius), (i % side).abs_diff(radius));
            corner[y * (radius + 1) + x]
        })
        .collect();
    let (values, vectors) = eigen(ker, side);
    let mut order: Vec<usize> = (0..side).collect();
    order.sort_by(|&p, &q| values[q].abs().total_cmp(&values[p].abs()));
    let pairs: Vec<(Vec<f64>, Vec<f64>)> = order[..RANK]
        .iter()
        .map(|&k| {
            let s = values[k].abs().sqrt();
            let a: Vec<f64> = (0..side).map(|i| vectors[i * side + k] * s).collect();
            let b: Vec<f64> = a.iter().map(|x| x * values[k].signum()).collect();
            (a, b)
        })
        .collect();
    let norm = (0..side * side)
        .map(|i| pairs.iter().map(|(a, b)| a[i / side] * b[i % side]).sum::<f64>().powi(2))
        .sum::<f64>()
        .sqrt()
        .sqrt();
    let scaled = |v: &[f64]| v.iter().map(|x| (x / norm) as f32).collect::<Vec<f32>>();
    Kernel { radius, a: [0, 1, 2].map(|k| scaled(&pairs[k].0)), b: [0, 1, 2].map(|k| scaled(&pairs[k].1)) }
}

type Kernels = Mutex<Vec<((Structure, usize, u32, bool), Arc<Kernel>)>>;

fn kernel(structure: Structure, length: usize, size: f32, layer: bool) -> Arc<Kernel> {
    static CACHE: OnceLock<Kernels> = OnceLock::new();
    let key = (structure, length, size.to_bits(), layer);
    let cache = CACHE.get_or_init(|| Mutex::new(Vec::new()));
    if let Some(found) = cache.lock().ok().and_then(|c| c.iter().find(|(k, _)| *k == key).map(|(_, v)| v.clone())) {
        return found;
    }
    let spectrum = match structure {
        Structure::Cine => cine_spectrum(length, size),
        Structure::Stills => stills_spectrum(length, size, if layer { LAYER_STRETCH } else { 1.0 }),
    };
    let built = Arc::new(build(grid(length), &spectrum));
    if let Ok(mut c) = cache.lock() {
        c.push((key, built.clone()));
        if c.len() > KERNELS_KEPT {
            c.remove(0);
        }
    }
    built
}

pub(crate) struct Grain {
    pub(crate) shared: Field,
    pub(crate) layers: Option<Field>,
    pub(crate) mix: Mix,
}

impl Grain {
    pub(crate) fn new(frame: Size, p: &GrainParams) -> Grain {
        let field = |layer: bool| {
            let kernel = kernel(p.structure, frame.short_edge(), p.size, layer);
            let soft = soft_weights(frame, p);
            Field { margin: kernel.radius + soft.len() / 2, soft, kernel }
        };
        let mix = Mix::new(frame, p);
        Grain { shared: field(false), layers: (mix.layer > 0.0).then(|| field(true)), mix }
    }

    pub(crate) fn layer_seeds(seed: u32) -> [u32; 3] {
        [1, 2, 3].map(|k| seed.wrapping_add(k))
    }

    pub(crate) fn channel_fields(&self, frame: Size, out: Region, seed: u32) -> Vec<[f32; 3]> {
        let tables = Tables { log2: log2_table(), exp2: exp2_table(), cos: cos_table() };
        let shared = self.shared.render(frame, out, seed, &tables);
        let Some(layers) = &self.layers else { return shared.par_iter().map(|z| self.mix.channels(*z, None)).collect() };
        let [r, g, b] = Grain::layer_seeds(seed).map(|seed| layers.render(frame, out, seed, &tables));
        (0..shared.len()).into_par_iter().map(|i| self.mix.channels(shared[i], Some([r[i], g[i], b[i]]))).collect()
    }
}

pub(crate) struct Field {
    pub(crate) margin: usize,
    pub(crate) soft: Vec<f32>,
    pub(crate) kernel: Arc<Kernel>,
}

impl Field {
    pub(crate) fn origin(&self, frame: Size, out: Region) -> [i32; 2] {
        let m = self.margin as i32;
        [out.x as i32 - m - (frame.w / 2) as i32, out.y as i32 - m - (frame.h / 2) as i32]
    }

    pub(crate) fn render(&self, frame: Size, out: Region, seed: u32, tables: &Tables) -> Vec<f32> {
        let (m, rs) = (self.margin, self.soft.len() / 2);
        let (nw, nh) = (out.w + 2 * m, out.h + 2 * m);
        let [ox, oy] = self.origin(frame, out);
        let mut noise = vec![0.0f32; nw * nh];
        noise.par_chunks_mut(nw).enumerate().for_each(|(j, row)| {
            for (i, v) in row.iter_mut().enumerate() {
                *v = tables.white(ox + i as i32, oy + j as i32, seed);
            }
        });
        let vh = out.h + 2 * rs;
        let mut columns = vec![[0.0f32; RANK]; nw * vh];
        columns.par_chunks_mut(nw).enumerate().for_each(|(j, row)| {
            for (i, v) in row.iter_mut().enumerate() {
                for (slot, b) in v.iter_mut().zip(&self.kernel.b) {
                    let mut sum = 0.0f32;
                    for (t, w) in b.iter().enumerate() {
                        sum += w * noise[(j + t) * nw + i];
                    }
                    *slot = sum;
                }
            }
        });
        let zw = out.w + 2 * rs;
        let mut field = vec![0.0f32; zw * vh];
        field.par_chunks_mut(zw).enumerate().for_each(|(j, row)| {
            for (i, v) in row.iter_mut().enumerate() {
                let mut z = 0.0f32;
                for (k, a) in self.kernel.a.iter().enumerate() {
                    let mut sum = 0.0f32;
                    for (t, w) in a.iter().enumerate() {
                        sum += w * columns[j * nw + i + t][k];
                    }
                    z += sum;
                }
                *v = flatten(z);
            }
        });
        if self.soft.is_empty() {
            return field;
        }
        let mut soft = vec![0.0f32; zw * out.h];
        soft.par_chunks_mut(zw).enumerate().for_each(|(j, row)| {
            for (i, v) in row.iter_mut().enumerate() {
                let mut sum = 0.0f32;
                for (t, w) in self.soft.iter().enumerate() {
                    sum += w * field[(j + t) * zw + i];
                }
                *v = sum;
            }
        });
        let mut out_field = vec![0.0f32; out.w * out.h];
        out_field.par_chunks_mut(out.w).enumerate().for_each(|(j, row)| {
            for (i, v) in row.iter_mut().enumerate() {
                let mut sum = 0.0f32;
                for (t, w) in self.soft.iter().enumerate() {
                    sum += w * soft[j * zw + i + t];
                }
                *v = sum;
            }
        });
        out_field
    }
}
