use std::sync::{Arc, Mutex, OnceLock};

use rayon::prelude::*;

use crate::film::{fast_exp2, fast_log2};
use crate::grain_tables::{DOMAIN, SPECTRUM, SPECTRUM_SIZES};
use crate::texture::{Region, Size};

// why: measured from Resolve renders of FilmVision's grain (scratchpad flc/grain): noise strength per unit of
// grainAmount, the field's mean offset, its sub-Gaussian marginal, and the softness and defocus blur scales
pub(crate) const AMPLITUDE: f32 = 0.3405;
pub(crate) const OFFSET: f32 = 0.107;
pub(crate) const FLATTEN: f32 = 0.003_703_8;
pub(crate) const FLATTEN_NORM: f32 = 0.988_68;
const SOFT_PER_HEIGHT: f32 = 0.00463;
const DEFOCUS_PER_HEIGHT: f32 = 0.00297;
pub(crate) const DOMAIN_STEPS: f32 = 5000.0;
pub(crate) const DOMAIN_TOP: f32 = 1.25;
pub(crate) const MINUS_TWO_LN2: f32 = -1.386_294_4;
pub(crate) const COS_STEPS: usize = 4096;
const SPECTRUM_STEP: f64 = 5.0;
const ENERGY_LEFT: f64 = 1e-4;
const MAX_RADIUS: usize = 64;
const RANK: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GrainParams {
    pub amount: f32,
    pub size: f32,
    pub softness: f32,
    pub defocus: f32,
    pub seed: u32,
}

impl GrainParams {
    pub(crate) fn scale(&self) -> f32 {
        AMPLITUDE * self.amount
    }

    pub(crate) fn grains(&self) -> bool {
        self.amount > 0.0
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
    let sigma = f64::from(DEFOCUS_PER_HEIGHT * frame.h as f32 * (1.0 - p.defocus).max(0.0));
    if sigma < 0.05 { Vec::new() } else { gaussian(sigma, (6.0 * sigma + 0.5) as usize) }
}

pub(crate) fn soft_weights(frame: Size, p: &GrainParams) -> Vec<f32> {
    let sigma = f64::from(SOFT_PER_HEIGHT * frame.h as f32 * p.softness.max(0.0).powf(1.5));
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

pub(crate) fn grained(c: [f32; 3], z: f32, scale: f32) -> [f32; 3] {
    let gain = 1.0 + scale * (z - OFFSET);
    c.map(|v| inverse(forward(v) * gain).clamp(0.0, 1.0))
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

// why: the grain's spectrum is FilmVision's, folded for point sampling at this height; the 2-D kernel is its
// inverse transform, split into three separable pairs (singular values 1 : 0.12 : 0.01)
fn build(height: usize, size: f32) -> Kernel {
    let h = height as f64;
    let n = if height >= 1400 { 512 } else { 256 };
    let half = n / 2;
    let row = spectrum_row(f64::from(size));
    let top = (row.len() - 1) as f64 * SPECTRUM_STEP;
    let fold = ((top / h + 0.5) as isize).min(6);
    let quarter: Vec<f64> = (0..=half)
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
        .collect();
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

type Kernels = Mutex<Vec<((usize, u32), Arc<Kernel>)>>;

pub(crate) fn kernel(height: usize, size: f32) -> Arc<Kernel> {
    static CACHE: OnceLock<Kernels> = OnceLock::new();
    let key = (height, size.to_bits());
    let cache = CACHE.get_or_init(|| Mutex::new(Vec::new()));
    if let Some(found) = cache.lock().ok().and_then(|c| c.iter().find(|(k, _)| *k == key).map(|(_, v)| v.clone())) {
        return found;
    }
    let built = Arc::new(build(height, size));
    if let Ok(mut c) = cache.lock() {
        c.push((key, built.clone()));
        if c.len() > 4 {
            c.remove(0);
        }
    }
    built
}

pub(crate) struct Field {
    pub(crate) margin: usize,
    pub(crate) soft: Vec<f32>,
    pub(crate) kernel: Arc<Kernel>,
}

impl Field {
    pub(crate) fn new(frame: Size, p: &GrainParams) -> Field {
        let kernel = kernel(frame.h, p.size);
        let soft = soft_weights(frame, p);
        Field { margin: kernel.radius + soft.len() / 2, soft, kernel }
    }

    pub(crate) fn origin(&self, frame: Size, out: Region) -> [i32; 2] {
        let m = self.margin as i32;
        [out.x as i32 - m - (frame.w / 2) as i32, out.y as i32 - m - (frame.h / 2) as i32]
    }

    pub(crate) fn render(&self, frame: Size, out: Region, p: &GrainParams, tables: &Tables) -> Vec<f32> {
        let (m, rs) = (self.margin, self.soft.len() / 2);
        let (nw, nh) = (out.w + 2 * m, out.h + 2 * m);
        let [ox, oy] = self.origin(frame, out);
        let mut noise = vec![0.0f32; nw * nh];
        noise.par_chunks_mut(nw).enumerate().for_each(|(j, row)| {
            for (i, v) in row.iter_mut().enumerate() {
                *v = tables.white(ox + i as i32, oy + j as i32, p.seed);
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
