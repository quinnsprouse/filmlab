use std::collections::HashMap;

use crate::dust_tables::{PLATES, THRESHOLD};
use crate::texture::{Region, Size};

const COLUMNS: usize = 1200;
const CENTRE: f64 = 600.0;
const SIGMA_PER_U: f64 = 0.0025;
const NO_BLUR: f64 = 0.01;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DustParams {
    pub size: f64,
    pub softness: f64,
    pub rows: usize,
}

impl DustParams {
    // why: FilmVision lays its dust plate on a canvas 1200 texels wide and as tall as the frame's aspect,
    // and that height picks one of ten plates, so it comes from the full-size frame, not the preview
    pub fn rows_of(native: Size) -> usize {
        COLUMNS * native.h / native.w.max(1)
    }
}

struct Tap {
    lo: usize,
    hi: usize,
    f: f64,
}

fn taps(n_in: usize, n_out: usize) -> Vec<Tap> {
    let scale = n_in as f64 / n_out as f64;
    let last = n_in as isize - 1;
    (0..n_out)
        .map(|i| {
            let t = (i as f64 + 0.5) * scale - 0.5;
            let lo = t.floor();
            let l = lo as isize;
            Tap { lo: l.clamp(0, last) as usize, hi: (l + 1).clamp(0, last) as usize, f: t - lo }
        })
        .collect()
}

fn gaussian(sigma: f64) -> Vec<f64> {
    if sigma <= NO_BLUR {
        return vec![1.0];
    }
    let radius = (4.0 * sigma + 0.5) as isize;
    let scale = -0.5 / (sigma * sigma);
    let raw: Vec<f64> = (-radius..=radius).map(|x| (scale * (x * x) as f64).exp()).collect();
    let total: f64 = raw.iter().sum();
    raw.iter().map(|v| v / total).collect()
}

fn threshold(u: f64) -> f64 {
    let [c, w, u0] = THRESHOLD;
    let z = (u - u0) / w;
    c * w * if z > 30.0 { z } else { z.exp().ln_1p() }
}

fn value(points: &[(f64, f32)], rows: f64) -> f64 {
    let (first, last) = (points[0], points[points.len() - 1]);
    if rows <= first.0 {
        return f64::from(first.1);
    }
    if rows >= last.0 {
        return f64::from(last.1);
    }
    let k = points.partition_point(|p| p.0 <= rows) - 1;
    let (a, b) = (points[k], points[k + 1]);
    let slope = (f64::from(b.1) - f64::from(a.1)) / (b.0 - a.0);
    slope * (rows - a.0) + f64::from(a.1)
}

fn survivors(p: &DustParams) -> Vec<(usize, usize, f64)> {
    let rows = p.rows;
    let cut = threshold(rows as f64 * p.softness);
    PLATES[rows % 10]
        .iter()
        .filter_map(|t| {
            let v = (value(t.values, rows as f64) - cut * t.h).max(0.0);
            let row = (rows / 2) as isize + isize::from(t.j);
            (v > 0.0 && row >= 0 && row < rows as isize).then_some((row as usize, t.i as usize, v))
        })
        .collect()
}

fn span(taps: &[Tap], from: usize, to: usize) -> std::ops::Range<usize> {
    taps.partition_point(|t| t.hi < from)..taps.partition_point(|t| t.lo <= to)
}

// why: FilmVision's specks are a fixed plate of texels, thresholded and blurred by softness, zoomed by size
// about the canvas centre, then the canvas is resized bilinearly to the frame (scratchpad flc/dust)
pub fn strength(frame: Size, region: Region, p: &DustParams) -> Vec<(usize, f32)> {
    let rows = p.rows;
    let size = if p.size <= 0.0 { 1.0 } else { p.size };
    let texels = if rows == 0 { Vec::new() } else { survivors(p) };
    if texels.is_empty() {
        return Vec::new();
    }
    let weights = gaussian(SIGMA_PER_U * rows as f64 * p.softness);
    let rad = weights.len() / 2;
    let (tx, ty) = (taps(COLUMNS, frame.w), taps(rows, frame.h));
    let cy = (rows / 2) as f64;
    let mut sums: HashMap<usize, f64> = HashMap::new();
    for (row, col, v) in texels {
        let (r0, r1) = (row.saturating_sub(rad) as isize, (row + rad).min(rows - 1) as isize);
        let (c0, c1) = (col.saturating_sub(rad) as isize, (col + rad).min(COLUMNS - 1) as isize);
        let lattice = |r: isize, c: isize| {
            if r < r0 || r > r1 || c < c0 || c > c1 {
                0.0
            } else {
                v * weights[(r - row as isize) as usize + rad] * weights[(c - col as isize) as usize + rad]
            }
        };
        let x0 = (CENTRE + (c0 as f64 - 1.0 - CENTRE) * size).floor().max(0.0) as usize;
        let x1 = ((CENTRE + (c1 as f64 + 1.0 - CENTRE) * size).ceil().max(0.0) as usize).min(COLUMNS - 1);
        let y0 = (cy + (r0 as f64 - 1.0 - cy) * size).floor().max(0.0) as usize;
        let y1 = ((cy + (r1 as f64 + 1.0 - cy) * size).ceil().max(0.0) as usize).min(rows - 1);
        if x0 > x1 || y0 > y1 {
            continue;
        }
        let pw = x1 - x0 + 1;
        let mut patch = vec![0.0f64; pw * (y1 - y0 + 1)];
        for (k, v) in patch.iter_mut().enumerate() {
            let ci = CENTRE + ((x0 + k % pw) as f64 - CENTRE) / size;
            let cj = cy + ((y0 + k / pw) as f64 - cy) / size;
            let (a0, b0) = (ci.floor(), cj.floor());
            let (fa, fb) = (ci - a0, cj - b0);
            let (a, b) = (a0 as isize, b0 as isize);
            *v = (1.0 - fb) * ((1.0 - fa) * lattice(b, a) + fa * lattice(b, a + 1)) + fb * ((1.0 - fa) * lattice(b + 1, a) + fa * lattice(b + 1, a + 1));
        }
        let at = |y: usize, x: usize| if (y0..=y1).contains(&y) && (x0..=x1).contains(&x) { patch[(y - y0) * pw + x - x0] } else { 0.0 };
        let xs = span(&tx, x0, x1);
        let ys = span(&ty, y0, y1);
        let (xa, xb) = (xs.start.max(region.x), xs.end.min(region.x + region.w));
        let (ya, yb) = (ys.start.max(region.y), ys.end.min(region.y + region.h));
        for (y, t) in ty.iter().enumerate().take(yb).skip(ya) {
            for (x, s) in tx.iter().enumerate().take(xb).skip(xa) {
                let upper = at(t.lo, s.lo) * (1.0 - s.f) + at(t.lo, s.hi) * s.f;
                let lower = at(t.hi, s.lo) * (1.0 - s.f) + at(t.hi, s.hi) * s.f;
                *sums.entry((y - region.y) * region.w + x - region.x).or_insert(0.0) += upper * (1.0 - t.f) + lower * t.f;
            }
        }
    }
    let mut out: Vec<(usize, f32)> = sums.into_iter().map(|(k, v)| (k, v.clamp(0.0, 1.0) as f32)).filter(|(_, s)| *s > 0.0).collect();
    out.sort_unstable_by_key(|(k, _)| *k);
    out
}

pub fn apply(data: &mut [[f32; 3]], frame: Size, region: Region, p: &DustParams) {
    for (k, s) in strength(frame, region, p) {
        data[k] = data[k].map(|x| x + s * (1.0 - x));
    }
}
