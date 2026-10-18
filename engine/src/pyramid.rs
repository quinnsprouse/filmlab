use rayon::prelude::*;

use crate::grain::reflect;

pub(crate) type Px = [f32; 3];

/// The samples of one axis of a pyramid level held by a region: `[lo, hi)` of the level's `n` samples. A sample
/// counts only when every input it reads lies inside its parent's span or is clamped at the level's own border, so
/// a window computes exactly the samples the whole frame would.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Span {
    pub(crate) lo: usize,
    pub(crate) hi: usize,
    pub(crate) n: usize,
}

impl Span {
    pub(crate) fn len(self) -> usize {
        self.hi.saturating_sub(self.lo)
    }

    /// the level samples m (at full-resolution x = s·m) whose taps s·m ± r this span holds
    pub(crate) fn down(self, s: usize, r: usize) -> Span {
        let n = self.n.div_ceil(s);
        let lo = if self.lo == 0 { 0 } else { (self.lo + r).div_ceil(s) };
        let hi = if self.hi == self.n { n } else if self.hi <= r { 0 } else { ((self.hi - 1 - r) / s + 1).min(n) };
        Span { lo, hi: hi.max(lo), n }
    }

    pub(crate) fn blur(self, r: usize) -> Span {
        let lo = if self.lo == 0 { 0 } else { self.lo + r };
        let hi = if self.hi == self.n { self.n } else { self.hi.saturating_sub(r) };
        Span { lo, hi: hi.max(lo), n: self.n }
    }

    /// the full-resolution samples x whose level samples x/s − 1 ..= x/s + 2 this span holds
    pub(crate) fn up(self, s: usize, n: usize) -> Span {
        let lo = if self.lo == 0 { 0 } else { (s * (self.lo + 1)).min(n) };
        let hi = if self.hi == self.n { n } else { (s * self.hi.saturating_sub(2)).min(n) };
        Span { lo, hi: hi.max(lo), n }
    }

    pub(crate) fn meet(self, other: Span) -> Span {
        let (lo, hi) = (self.lo.max(other.lo), self.hi.min(other.hi));
        Span { lo, hi: hi.max(lo), n: self.n }
    }
}

pub(crate) struct Level {
    pub(crate) x: Span,
    pub(crate) y: Span,
    pub(crate) data: Vec<Px>,
}

fn fma3(w: f32, v: Px, acc: Px) -> Px {
    [w.mul_add(v[0], acc[0]), w.mul_add(v[1], acc[1]), w.mul_add(v[2], acc[2])]
}

impl Level {
    pub(crate) fn at(&self, x: usize, y: usize) -> Px {
        self.data[(y - self.y.lo) * self.x.len() + (x - self.x.lo)]
    }

    fn build(x: Span, y: Span, f: impl Fn(usize, usize) -> Px + Sync) -> Level {
        let mut data = vec![[0.0f32; 3]; x.len() * y.len()];
        data.par_chunks_mut(x.len().max(1)).enumerate().for_each(|(j, row)| {
            for (i, v) in row.iter_mut().enumerate() {
                *v = f(x.lo + i, y.lo + j);
            }
        });
        Level { x, y, data }
    }

    // why: Resolve decimates each octave straight from full resolution: level sample m sits on full-resolution
    // sample s·m and sums its 2r + 1 neighbours, reflected at the frame's edge
    pub(crate) fn reduce(&self, w: &[f32], s: usize, vertical: bool) -> Level {
        let r = w.len() / 2;
        let (x, y) = if vertical { (self.x, self.y.down(s, r)) } else { (self.x.down(s, r), self.y) };
        let n = if vertical { self.y.n } else { self.x.n };
        Level::build(x, y, |i, j| {
            let m = if vertical { j } else { i };
            w.iter().enumerate().fold([0.0; 3], |acc, (t, &wt)| {
                let k = reflect((s * m + t) as isize - r as isize, n);
                fma3(wt, if vertical { self.at(i, k) } else { self.at(k, j) }, acc)
            })
        })
    }

    // why: the level blur reflects half-sample symmetrically at the frame's edge (…c b a | a b c…), which Resolve's
    // level-0 halation fits to 1e-6 where mirror, nearest or zero miss by 10–200× (scratchpad flc/hal edge charts)
    pub(crate) fn blur(&self, g: &[f32], vertical: bool) -> Level {
        let r = g.len() / 2;
        let (x, y) = if vertical { (self.x, self.y.blur(r)) } else { (self.x.blur(r), self.y) };
        let n = if vertical { self.y.n } else { self.x.n };
        Level::build(x, y, |i, j| {
            let m = if vertical { j } else { i };
            g.iter().enumerate().fold([0.0; 3], |acc, (t, &wt)| {
                let k = reflect((m + t) as isize - r as isize, n);
                fma3(wt, if vertical { self.at(i, k) } else { self.at(k, j) }, acc)
            })
        })
    }

    // why: full-resolution sample x = s·m0 + p interpolates level samples m0 − 1 ..= m0 + 2 with the 4 weights of
    // its phase p, reflected at the level's edge
    pub(crate) fn expand(&self, e: &[f32], s: usize, n: usize, vertical: bool) -> Level {
        let (x, y) = if vertical { (self.x, self.y.up(s, n)) } else { (self.x.up(s, n), self.y) };
        let coarse = if vertical { self.y.n } else { self.x.n };
        Level::build(x, y, |i, j| {
            let q = if vertical { j } else { i };
            let (m0, phase) = (q / s, q % s);
            (0..4).fold([0.0; 3], |acc, k| {
                let m = reflect((m0 + k) as isize - 1, coarse);
                fma3(e[4 * phase + k], if vertical { self.at(i, m) } else { self.at(m, j) }, acc)
            })
        })
    }

    /// `acc + w ∘ self` per channel over the samples both hold
    pub(crate) fn add_into(&self, w: [f32; 3], acc: Option<&Level>) -> Level {
        let (x, y) = match acc {
            Some(a) => (self.x.meet(a.x), self.y.meet(a.y)),
            None => (self.x, self.y),
        };
        Level::build(x, y, |i, j| {
            let base = acc.map_or([0.0; 3], |a| a.at(i, j));
            let v = self.at(i, j);
            [w[0].mul_add(v[0], base[0]), w[1].mul_add(v[1], base[1]), w[2].mul_add(v[2], base[2])]
        })
    }
}
