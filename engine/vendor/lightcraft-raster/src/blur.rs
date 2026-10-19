//! Gaussian-like blur by three successive box filters (running sums, O(1) per pixel per pass).

use crate::resample::Pixel;
use crate::{Image, par_rows};

/// The largest blur sigma (pixels) the box radii are made for. A box wider than any image (the
/// encoders' limit is 65535 px) blurs to the same flat result, and a larger sigma made the box
/// width's square overflow (from about 23 170 px: a panic where overflow is checked, a garbage
/// radius of billions of pixels, i.e. a hung render, where it isn't).
pub const MAX_SIGMA: f32 = 65_536.0;

/// Box radii approximating a Gaussian of `sigma` with 3 passes (Kovesi / Wells). A sigma that is
/// not a number counts as 0 (no blur); one above [`MAX_SIGMA`] (infinity too) as `MAX_SIGMA`.
pub fn box_radii(sigma: f32) -> [usize; 3] {
    let sigma = if sigma.is_nan() { 0.0 } else { sigma.clamp(0.0, MAX_SIGMA) };
    let n = 3.0f32;
    let w_ideal = (12.0 * sigma * sigma / n + 1.0).sqrt();
    let mut wl = w_ideal.floor() as i32;
    if wl % 2 == 0 {
        wl -= 1;
    }
    let wu = wl + 2;
    // (the square in i64: it passes i32 from a box 46 341 px wide)
    let m_ideal = (12.0 * sigma * sigma - n * (i64::from(wl) * i64::from(wl)) as f32 - 4.0 * n * wl as f32 - 3.0 * n) / (-4.0 * wl as f32 - 4.0);
    let m = m_ideal.round() as i32;
    std::array::from_fn(|i| (((if (i as i32) < m { wl } else { wu }) - 1) / 2).max(0) as usize)
}

/// Standard deviation (px) of the blur [`gaussian`] actually makes for `sigma`: its boxes are
/// whole pixels wide, so small radii snap to a few steps (0 below about 0.58 px, then √(2/3) px for one
/// 3-px box per axis, …).
pub fn box_sigma(sigma: f32) -> f32 {
    if sigma.is_nan() || sigma <= 0.3 {
        return 0.0;
    }
    box_radii(sigma).iter().map(|&r| (((2 * r + 1) * (2 * r + 1) - 1) as f32) / 12.0).sum::<f32>().sqrt()
}

/// One horizontal box pass over `row` (clamped edges), using `tmp` as scratch.
#[inline]
fn box_row<T: Pixel>(row: &mut [T], tmp: &mut Vec<T>, r: usize) {
    let w = row.len();
    if r == 0 || w == 0 {
        return;
    }
    tmp.clear();
    tmp.extend_from_slice(row);
    let s = &tmp[..];
    let last = w - 1;
    let inv = 1.0 / (2 * r + 1) as f32;
    let mut acc = T::zero();
    for i in 0..=2 * r {
        acc = acc.madd(s[i.saturating_sub(r).min(last)], 1.0);
    }
    for x in 0..w {
        row[x] = T::zero().madd(acc, inv);
        acc = acc.madd(s[(x + r + 1).min(last)], 1.0).madd(s[x.saturating_sub(r)], -1.0);
    }
}

/// Rows the horizontal passes step through together.
// why: Filmlab: a row's running sum is one long chain of dependent additions, so one row at a time the core
// mostly waits on them; four rows side by side keep four independent chains in flight. Each row's sums are
// the same additions in the same order as `box_row`'s, so the result is bit-identical
const ROWS_H: usize = 4;

/// [`box_row`] on each of the rows of `rows` (`w` pixels each), the first [`ROWS_H`] of them stepped together.
fn box_rows<T: Pixel>(rows: &mut [T], w: usize, tmp: &mut Vec<T>, r: usize) {
    if r == 0 || w == 0 {
        return;
    }
    if rows.len() != ROWS_H * w {
        for row in rows.chunks_mut(w) {
            box_row(row, tmp, r);
        }
        return;
    }
    tmp.clear();
    tmp.extend_from_slice(rows);
    let s: [&[T]; ROWS_H] = std::array::from_fn(|j| &tmp[j * w..(j + 1) * w]);
    let (o0, rest) = rows.split_at_mut(w);
    let (o1, rest) = rest.split_at_mut(w);
    let (o2, o3) = rest.split_at_mut(w);
    let out: [&mut [T]; ROWS_H] = [o0, o1, o2, o3];
    let last = w - 1;
    let inv = 1.0 / (2 * r + 1) as f32;
    let mut acc = [T::zero(); ROWS_H];
    for i in 0..=2 * r {
        let k = i.saturating_sub(r).min(last);
        for j in 0..ROWS_H {
            acc[j] = acc[j].madd(s[j][k], 1.0);
        }
    }
    for x in 0..w {
        let (add, sub) = ((x + r + 1).min(last), x.saturating_sub(r));
        for j in 0..ROWS_H {
            out[j][x] = T::zero().madd(acc[j], inv);
            acc[j] = acc[j].madd(s[j][add], 1.0).madd(s[j][sub], -1.0);
        }
    }
}

/// Rows per parallel band in the vertical pass (each band re-primes its running sums).
pub const BAND: usize = 32;

/// One vertical box pass (radius `r`) made row by row in order, as `box_v` made it at f34c7a8: each band of
/// [`BAND`] rows from a running sum per column primed afresh at the band's first row (clamped edges). It keeps its
/// last `rows` rows for the pass after it.
// why: Filmlab: `gaussian`'s three vertical passes each read and wrote the whole image; streamed through one
// another, a strip of columns at a time, the image is read once and the result written once, and a pass's rows
// live only as long as the next pass needs them. The bands, their priming and every sum are as before, so the
// result is bit-identical
struct Pass<T> {
    r: usize,
    inv: f32,
    /// The next row this pass makes.
    next: usize,
    acc: Vec<T>,
    ring: Vec<T>,
    rows: usize,
}

/// Bands of rows per task of [`box_v_stream`].
const STREAM_BANDS: usize = 4;
/// Columns per strip of [`box_v_stream`].
const STREAM_COLS: usize = 512;

/// The strip of `src` the passes work on: columns `x0..x0 + xw`.
#[derive(Clone, Copy)]
struct Strip<'a, T> {
    src: &'a Image<T>,
    x0: usize,
    xw: usize,
}

/// Row `y` of pass `k` (which must still keep it).
fn kept<T>(passes: &[Pass<T>], k: usize, y: usize, xw: usize) -> &[T] {
    let p = &passes[k];
    let at = (y % p.rows) * xw;
    &p.ring[at..at + xw]
}

/// Add `sign` × row `y` of pass `k`'s input (the strip's source for the first pass) into pass `k`'s sums.
fn add_input<T: Pixel>(passes: &mut [Pass<T>], s: Strip<'_, T>, k: usize, y: usize, sign: f32) {
    if k == 0 {
        let w = s.src.width;
        let row = &s.src.data[y * w + s.x0..y * w + s.x0 + s.xw];
        for (a, v) in passes[0].acc.iter_mut().zip(row) {
            *a = a.madd(*v, sign);
        }
        return;
    }
    make(passes, s, k - 1, y);
    let (before, from) = passes.split_at_mut(k);
    for (a, v) in from[0].acc.iter_mut().zip(kept(before, k - 1, y, s.xw)) {
        *a = a.madd(*v, sign);
    }
}

/// Make pass `k` produce its rows up to `y`.
fn make<T: Pixel>(passes: &mut [Pass<T>], s: Strip<'_, T>, k: usize, y: usize) {
    let last = s.src.height - 1;
    while passes[k].next <= y {
        let (row, r) = (passes[k].next, passes[k].r);
        if row % BAND == 0 {
            passes[k].acc.iter_mut().for_each(|a| *a = T::zero());
            for i in 0..=2 * r {
                add_input(passes, s, k, (row + i).saturating_sub(r).min(last), 1.0);
            }
        }
        let p = &mut passes[k];
        let at = (row % p.rows) * s.xw;
        for (d, a) in p.ring[at..at + s.xw].iter_mut().zip(&p.acc) {
            *d = T::zero().madd(*a, p.inv);
        }
        // (the sums after a band's last row are never read: the next row primes them afresh)
        if (row + 1) % BAND != 0 && row < last {
            add_input(passes, s, k, (row + r + 1).min(last), 1.0);
            add_input(passes, s, k, row.saturating_sub(r), -1.0);
        }
        passes[k].next = row + 1;
    }
}

/// The vertical box passes of radii `radii` (each above 0) over `src`, one after another, the last one's rows
/// handed to `take(y, row, out_row)` (`out` holds `src`'s rows; the rows come in strips of columns, `row` and
/// `out_row` the same strip of row `y`).
fn box_v_stream<T: Pixel, U: Send>(src: &Image<T>, radii: &[usize], out: &mut [U], take: &(impl Fn(usize, &[T], &mut [U]) + Sync + Send)) {
    let (w, h) = (src.width, src.height);
    let n = radii.len();
    if w == 0 || h == 0 || n == 0 {
        return;
    }
    let cols = STREAM_COLS.min(w);
    par_rows(out, w * BAND * STREAM_BANDS, |task, out| {
        let y0 = task * BAND * STREAM_BANDS;
        let y1 = y0 + out.len() / w;
        // each pass keeps the rows the next one reads around its row: 2r + 2 of them (the last pass, its row)
        let mut passes: Vec<Pass<T>> = (0..n)
            .map(|k| {
                let rows = if k + 1 < n { 2 * radii[k + 1] + 2 } else { 1 };
                Pass { r: radii[k], inv: 1.0 / (2 * radii[k] + 1) as f32, next: 0, acc: vec![T::zero(); cols], ring: vec![T::zero(); rows * cols], rows }
            })
            .collect();
        for x0 in (0..w).step_by(cols) {
            let xw = cols.min(w - x0);
            for p in passes.iter_mut() {
                p.acc.truncate(xw);
                p.ring.truncate(p.rows * xw);
            }
            // each pass starts at the band holding the first row the pass after it primes with
            let mut start = y0;
            for k in (0..n).rev() {
                passes[k].next = start;
                start = start.saturating_sub(passes[k].r) / BAND * BAND;
            }
            let s = Strip { src, x0, xw };
            for y in y0..y1 {
                make(&mut passes, s, n - 1, y);
                let o = (y - y0) * w + x0;
                take(y, kept(&passes, n - 1, y, xw), &mut out[o..o + xw]);
            }
        }
    });
}

/// The box radii of [`gaussian`] on a `w × h` image (the GPU renderer's twin uses them too), or
/// `None` when the blur leaves the image as it is: `sigma` at most 0.3 or not a number, or an
/// empty image. A sigma wider than twice the image's long side counts as that: a crop a hair
/// wide shown to fit asked the tone and dehaze stages for sigmas of millions of pixels (infinite
/// for a crop of no size), which blur no flatter than that but cost boxes that wide on every row.
pub fn radii_for(sigma: f32, w: usize, h: usize) -> Option<[usize; 3]> {
    if sigma.is_nan() || sigma <= 0.3 || w == 0 || h == 0 {
        return None;
    }
    let cap = (w.max(h) as f32 * 2.0).max(1.0);
    Some(box_radii(sigma.min(cap)))
}

/// The first half of [`gaussian`] (its horizontal passes) of the `width × height` image whose rows `fill(y, row)`
/// writes. Finish it with [`gaussian_v_into`] at the same `sigma`.
// why: Filmlab: with the two halves apart a caller can blur an image it computes row by row, and use the result
// row by row as the last pass makes it, without the input's copy and the result buffer (colour noise reduction:
// two full-size passes and buffers fewer); the arithmetic is `gaussian`'s, so the result is bit-identical
pub fn gaussian_h<T: Pixel>(width: usize, height: usize, sigma: f32, fill: impl Fn(usize, &mut [T]) + Sync + Send) -> Image<T> {
    let radii = radii_for(sigma, width, height);
    let mut a = Image::<T>::new(width, height);
    par_rows(&mut a.data, width * ROWS_H, |band, rows| {
        for (k, row) in rows.chunks_mut(width).enumerate() {
            fill(band * ROWS_H + k, row);
        }
        if let Some(radii) = radii {
            let mut tmp = Vec::with_capacity(rows.len());
            for r in radii {
                box_rows(rows, width, &mut tmp, r);
            }
        }
    });
    a
}

/// The second half of [`gaussian`]: the vertical passes of `a` (from [`gaussian_h`] at the same `sigma`), each row
/// of the result handed to `take(y, row, out_row)` as the last pass makes it (`out` holds `a`'s height of rows as
/// wide as `a`; a row may come in pieces, `row` and `out_row` then the same columns of it), with the same sums as
/// `gaussian`.
pub fn gaussian_v_into<T: Pixel, U: Send>(a: Image<T>, sigma: f32, out: &mut [U], take: impl Fn(usize, &[T], &mut [U]) + Sync + Send) {
    let (w, h) = (a.width, a.height);
    let passes: Vec<usize> = radii_for(sigma, w, h).map(|r| r.into_iter().filter(|&r| r > 0).collect()).unwrap_or_default();
    if passes.is_empty() {
        par_rows(out, w, |y, o| take(y, a.row(y), o));
        return;
    }
    box_v_stream(&a, &passes, out, &take);
}

/// Blur with Gaussian `sigma` (pixels). `sigma <= 0.3` returns a copy.
pub fn gaussian<T: Pixel>(img: &Image<T>, sigma: f32) -> Image<T> {
    let Some(radii) = radii_for(sigma, img.width, img.height) else {
        return img.clone();
    };
    // why: Filmlab: each row is copied in the pass that blurs it (the whole-image clone ran on one core first)
    let w = img.width;
    let copy = img.data.len() == w * img.height;
    let mut a = if copy { Image::<T>::new(w, img.height) } else { img.clone() };
    par_rows(&mut a.data, w * ROWS_H, |band, rows| {
        if copy {
            let y0 = band * ROWS_H * w;
            rows.copy_from_slice(&img.data[y0..y0 + rows.len()]);
        }
        let mut tmp = Vec::with_capacity(rows.len());
        for r in radii {
            box_rows(rows, w, &mut tmp, r);
        }
    });
    let passes: Vec<usize> = radii.into_iter().filter(|&r| r > 0).collect();
    if passes.is_empty() {
        return a;
    }
    let mut b = Image::<T>::new(img.width, img.height);
    box_v_stream(&a, &passes, &mut b.data, &|_, row: &[T], out: &mut [T]| out.copy_from_slice(row));
    b
}

#[cfg(test)]
mod tests {
    /// The box radii of the formula as it was, for the check below.
    fn box_radii_before(sigma: f32) -> [usize; 3] {
        let n = 3.0f32;
        let w_ideal = (12.0 * sigma * sigma / n + 1.0).sqrt();
        let mut wl = w_ideal.floor() as i32;
        if wl % 2 == 0 {
            wl -= 1;
        }
        let wu = wl + 2;
        let m_ideal = (12.0 * sigma * sigma - n * (wl * wl) as f32 - 4.0 * n * wl as f32 - 3.0 * n) / (-4.0 * wl as f32 - 4.0);
        let m = m_ideal.round() as i32;
        std::array::from_fn(|i| (((if (i as i32) < m { wl } else { wu }) - 1) / 2).max(0) as usize)
    }

    /// A blur sigma from a degenerate edit or a deep zoom (found by a UI campaign: a render
    /// worker's blur with a huge sigma) squared the box width past i32, which panics in a debug
    /// build and gave a radius of billions of pixels in a release one. Huge, infinite and NaN
    /// sigmas give bounded radii, and a blur with them is quick and stays finite; every sigma that
    /// worked before keeps exactly its radii.
    #[test]
    fn huge_and_non_finite_sigmas_give_bounded_radii() {
        for sigma in [26_800.0, 46_341.0, 1e5, 1e9, f32::MAX, f32::INFINITY, f32::NEG_INFINITY, f32::NAN, -5.0] {
            let r = box_radii(sigma);
            assert!(r.iter().all(|&r| r <= MAX_SIGMA as usize * 2), "{sigma}: {r:?}");
        }
        assert_eq!(box_radii(f32::NAN), [0, 0, 0]);
        assert_eq!(box_radii(f32::NEG_INFINITY), [0, 0, 0]);
        assert_eq!(box_radii(1e9), box_radii(MAX_SIGMA));
        assert_eq!(box_radii(f32::INFINITY), box_radii(MAX_SIGMA));
        // on an image, the blur is at most twice its long side; NaN and tiny sigmas don't blur
        assert_eq!(radii_for(3.1e7, 1400, 900), Some(box_radii(2800.0)));
        assert_eq!(radii_for(f32::INFINITY, 1400, 900), Some(box_radii(2800.0)));
        assert_eq!(radii_for(40.0, 1400, 900), Some(box_radii(40.0)), "an ordinary sigma is as asked");
        assert_eq!(radii_for(f32::NAN, 1400, 900), None);
        assert_eq!(radii_for(0.2, 1400, 900), None);
        assert_eq!(radii_for(5.0, 0, 900), None);
        let p = crate::Plane { width: 4, height: 3, data: (0..12).map(|i| i as f32).collect() };
        let t = std::time::Instant::now();
        for sigma in [1e9, f32::INFINITY, f32::NAN] {
            let b = gaussian(&p, sigma);
            assert!(b.data.iter().all(|v| v.is_finite()), "{sigma}");
        }
        assert!(t.elapsed().as_secs_f64() < 30.0, "a huge sigma's blur is bounded");
        let mut s = 0.0f32;
        while s < 23_000.0 {
            assert_eq!(box_radii(s), box_radii_before(s), "sigma {s}");
            s = s * 1.013 + 0.07;
        }
    }

    use super::*;

    #[test]
    fn preserves_constant_and_mass() {
        let img = Image::<f32>::filled(50, 30, 0.7);
        let b = gaussian(&img, 4.0);
        assert!(b.data.iter().all(|v| (v - 0.7).abs() < 1e-5));
        let mut imp = Image::<f32>::new(101, 101);
        imp.set(50, 50, 1.0);
        let b = gaussian(&imp, 5.0);
        let sum: f32 = b.data.iter().sum();
        assert!((sum - 1.0).abs() < 1e-3, "{sum}");
        // peak moved to the centre and symmetric
        assert!((b.get(45, 50) - b.get(55, 50)).abs() < 1e-6);
        assert!((b.get(50, 45) - b.get(50, 55)).abs() < 1e-6);
    }

    #[test]
    fn box_sigma_is_the_blur_made() {
        assert_eq!(box_sigma(0.3), 0.0);
        assert_eq!(box_sigma(0.5), 0.0, "no box wider than 1 px");
        assert!((box_sigma(1.0) - (2.0f32 / 3.0).sqrt()).abs() < 1e-6);
        for sigma in [1.0f32, 2.5, 8.0] {
            let mut imp = Image::<f32>::new(201, 1);
            imp.set(100, 0, 1.0);
            let b = gaussian(&imp, sigma);
            let var: f32 = b.data.iter().enumerate().map(|(i, v)| v * ((i as f32 - 100.0).powi(2))).sum();
            assert!((var.sqrt() - box_sigma(sigma)).abs() < 1e-3, "{sigma}: {} vs {}", var.sqrt(), box_sigma(sigma));
        }
    }

    #[test]
    fn variance_close_to_sigma_squared() {
        let mut imp = Image::<f32>::new(201, 1);
        imp.set(100, 0, 1.0);
        let sigma = 8.0;
        let b = gaussian(&imp, sigma);
        let var: f32 = b.data.iter().enumerate().map(|(i, v)| v * ((i as f32 - 100.0).powi(2))).sum();
        assert!((var.sqrt() - sigma).abs() / sigma < 0.1, "{}", var.sqrt());
    }
}
