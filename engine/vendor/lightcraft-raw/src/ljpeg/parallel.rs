//! Filmlab: the lossless-JPEG frame most DNGs carry, decoded in parallel with the same result bit for bit.
//!
//! why: a Ricoh GR III DNG (and most others) is one 6112 x 4064 strip of predictor-1 lossless JPEG with one
//! Huffman table for both components and no restart markers: a single serial bitstream, which [`super::decode`]
//! reads at about 6 ns a sample, ~150 ms per photo on one core while every other core waits.
//!
//! The stream is cut into segments that are decoded at the same time from arbitrary bit offsets. Huffman codes
//! resynchronise: a decoder started mid-symbol soon lands on a true symbol boundary and from there on reads what
//! the serial decoder reads. Stitching walks the segments in order with the true position, finds where it meets
//! each segment's decode (decoding the few symbols before that itself), and keeps each segment's samples from
//! there. Predictor 1 then rebuilds the samples: down the first column, then along each row.
//!
//! Every symbol is decoded by [`Huffman::decode`] and [`diff_value`] themselves, or by a table built from them
//! (`symbols`), and the samples are summed modulo 2^16 as `decode` sums them, so the frame is the same. Anything
//! this path is not sure of (an invalid code inside the frame, a stream that runs out before the frame ends,
//! other layouts) returns `None`, and [`super::decode`] decodes the frame as before.

use super::{Bits, Header, Huffman, diff_value};
use rayon::prelude::*;

/// Bits of the symbol table: a code and its difference bits up to this long decode in one lookup.
const TABLE_BITS: u32 = 12;
/// Entry flag: the code fits the table but its difference bits do not (`TABLE_BITS` bits hold no difference).
const CODE_ONLY: u32 = 1 << 22;
/// Segment length in stream bytes, and the most segments a frame is cut into.
const SEGMENT_BYTES: usize = 1 << 18;
const MAX_SEGMENTS: usize = 256;
/// Symbol start positions kept per segment to meet the true position at (they resynchronise within a few dozen).
const KEPT_STARTS: usize = 1024;
/// Invalid codes noted per segment (a decode that starts mid-symbol can hit some before it resynchronises).
const KEPT_INVALID: usize = 64;

/// 64 bits of an unstuffed stream starting at bit `pos`, first bit most significant; at least 57 of them are the
/// stream's (zeros past its end, as [`super::BitReader`] feeds).
#[inline]
fn window(clean: &[u8], pos: u64) -> u64 {
    let byte = (pos >> 3) as usize;
    let word = match clean.get(byte..).and_then(<[u8]>::first_chunk::<8>) {
        Some(b) => u64::from_be_bytes(*b),
        None => u64::from_be_bytes(std::array::from_fn(|i| clean.get(byte + i).copied().unwrap_or(0))),
    };
    word << (pos & 7)
}

/// The bits of one [`window`], for [`Huffman::decode`] and [`diff_value`] (they read at most 32).
struct Window {
    bits: u64,
    used: u32,
}

impl Bits for Window {
    #[inline]
    fn peek(&mut self, n: u32) -> u32 {
        ((self.bits << self.used) >> (64 - n)) as u32
    }
    #[inline]
    fn consume(&mut self, n: u32) {
        self.used += n;
    }
}

/// The symbol table: for each `TABLE_BITS`-bit prefix, the bits the symbol takes (bits 16..22) and its difference
/// modulo 2^16 (bits 0..16), or the code's length and category (bits 24..32) with [`CODE_ONLY`] when only the code
/// fits; 0 when the code is longer (or invalid). Built by decoding each prefix with the decoder's own functions.
fn symbols(table: &Huffman) -> Vec<u32> {
    (0..1u32 << TABLE_BITS)
        .map(|prefix| {
            let mut w = Window { bits: u64::from(prefix) << (64 - TABLE_BITS), used: 0 };
            let Ok(ssss) = table.decode(&mut w) else { return 0 };
            let code = w.used;
            if code > TABLE_BITS {
                return 0;
            }
            let extra = match ssss {
                0 | 16 => 0,
                s => u32::from(s.min(16)),
            };
            if code + extra > TABLE_BITS {
                return (u32::from(ssss) << 24) | CODE_ONLY | (code << 16);
            }
            let diff = diff_value(&mut w, ssss);
            (w.used << 16) | u32::from(diff as u16)
        })
        .collect()
}

/// The symbol at the start of `bits`: (bits it takes, difference modulo 2^16); `None` for an invalid code.
#[inline]
fn symbol(lookup: &[u32], table: &Huffman, bits: u64) -> Option<(u32, u16)> {
    let e = lookup[(bits >> (64 - TABLE_BITS)) as usize];
    let len = (e >> 16) & 0x3f;
    if e & CODE_ONLY == 0 && len != 0 {
        return Some((len, e as u16));
    }
    let mut w = Window { bits, used: 0 };
    let ssss = if e & CODE_ONLY != 0 {
        w.used = len;
        (e >> 24) as u8
    } else {
        table.decode(&mut w).ok()?
    };
    let diff = diff_value(&mut w, ssss);
    Some((w.used, diff as u16))
}

/// What a decode of the symbols starting in `[from, to)` read.
struct Run {
    /// Differences modulo 2^16, in stream order.
    diffs: Vec<u16>,
    /// Bit positions of the first [`KEPT_STARTS`] symbols (`starts[j]` is where `diffs[j]` starts).
    starts: Vec<u64>,
    /// Invalid codes met: (bit position, symbols decoded before it); `None` when more than [`KEPT_INVALID`].
    invalid: Option<Vec<(u64, usize)>>,
    /// Bit position after the last symbol or invalid code (at or past `to`).
    end: u64,
    /// Bit position after the last valid symbol.
    valid_end: u64,
}

fn run(clean: &[u8], lookup: &[u32], table: &Huffman, from: u64, to: u64, capacity: usize) -> Run {
    let mut r = Run { diffs: Vec::with_capacity(capacity), starts: Vec::with_capacity(KEPT_STARTS), invalid: Some(Vec::new()), end: from, valid_end: from };
    let mut pos = from;
    while pos < to {
        // the table's symbols straight from one load (a window holds at least 57 bits, a table symbol at most
        // `TABLE_BITS`), until one needs more bits than the table has
        let bits = window(clean, pos);
        let mut used = 0;
        let mut longer = false;
        while used <= 57 - TABLE_BITS && pos + u64::from(used) < to {
            let e = lookup[((bits << used) >> (64 - TABLE_BITS)) as usize];
            let len = (e >> 16) & 0x3f;
            if e & CODE_ONLY != 0 || len == 0 {
                longer = true;
                break;
            }
            if r.starts.len() < KEPT_STARTS {
                r.starts.push(pos + u64::from(used));
            }
            r.diffs.push(e as u16);
            used += len;
        }
        if used > 0 {
            pos += u64::from(used);
            r.valid_end = pos;
        }
        if !longer || pos >= to {
            continue;
        }
        match symbol(lookup, table, window(clean, pos)) {
            Some((len, diff)) => {
                if r.starts.len() < KEPT_STARTS {
                    r.starts.push(pos);
                }
                r.diffs.push(diff);
                pos += u64::from(len);
                r.valid_end = pos;
            }
            None => {
                // (not a symbol boundary unless the stream itself is invalid: stitching decides which)
                if let Some(list) = &mut r.invalid {
                    if list.len() < KEPT_INVALID { list.push((pos, r.diffs.len())) } else { r.invalid = None }
                }
                pos += 1;
            }
        }
    }
    r.end = pos;
    r
}

/// The entropy-coded bytes with the `FF 00` stuffing removed, up to the first marker, as [`super::BitReader`]
/// reads them (a lone `FF` at the very end reads as `FF`); 8 zero bytes follow for [`window`]. Returns the bytes
/// and how many of them are the stream's. Done in pieces in parallel: one pass measures each piece, one writes it.
fn unstuff(d: &[u8]) -> (Vec<u8>, usize) {
    // pieces end anywhere but just after an FF (an FF's next byte decides what it is)
    let pieces = d.len().div_ceil(SEGMENT_BYTES).clamp(1, MAX_SEGMENTS);
    let mut cuts: Vec<usize> = (0..=pieces).map(|k| d.len() * k / pieces).collect();
    for k in 1..pieces {
        cuts[k] = cuts[k].max(cuts[k - 1]);
        while cuts[k] < d.len() && d[cuts[k] - 1] == 0xff {
            cuts[k] += 1;
        }
    }
    let ranges: Vec<(usize, usize)> = cuts.windows(2).map(|c| (c[0], c[1].max(c[0]))).collect();
    let sizes: Vec<(usize, bool)> = ranges.par_iter().map(|&(a, b)| unstuff_piece(d, a, b, None)).collect();
    let pieces = sizes.iter().position(|s| s.1).map_or(sizes.len(), |k| k + 1);
    let len = sizes[..pieces].iter().map(|s| s.0).sum();
    let mut out = vec![0u8; len + 8];
    let mut rest = &mut out[..len];
    let mut slots = Vec::with_capacity(pieces);
    for &(n, _) in &sizes[..pieces] {
        let (slot, tail) = rest.split_at_mut(n);
        slots.push(slot);
        rest = tail;
    }
    slots.into_par_iter().zip(&ranges[..pieces]).for_each(|(slot, &(a, b))| {
        unstuff_piece(d, a, b, Some(slot));
    });
    (out, len)
}

/// Unstuff `d[a..b]` into `out` (when given): the bytes it gives and whether a marker ends the stream in it.
fn unstuff_piece(d: &[u8], a: usize, b: usize, mut out: Option<&mut [u8]>) -> (usize, bool) {
    let (mut i, mut n) = (a, 0);
    while i < b {
        let next = i + next_ff(&d[i..b]);
        if let Some(o) = out.as_deref_mut() {
            o[n..n + next - i].copy_from_slice(&d[i..next]);
        }
        n += next - i;
        i = next;
        if i >= b {
            break;
        }
        match d.get(i + 1) {
            None | Some(0) => {
                if let Some(o) = out.as_deref_mut() {
                    o[n] = 0xff;
                }
                n += 1;
                i += 2;
            }
            Some(_) => return (n, true),
        }
    }
    // (a lone FF at the very end of the data ends it too)
    (n, i > d.len())
}

/// Index of the first `FF` byte in `d` (its length when there is none), eight bytes at a time.
fn next_ff(d: &[u8]) -> usize {
    let mut i = 0;
    while let Some(b) = d.get(i..).and_then(<[u8]>::first_chunk::<8>) {
        // (a zero byte of the complement is an FF; the lowest flagged byte is the first one)
        let v = !u64::from_le_bytes(*b);
        let zero = v.wrapping_sub(0x0101_0101_0101_0101) & !v & 0x8080_8080_8080_8080;
        if zero != 0 {
            return i + (zero.trailing_zeros() / 8) as usize;
        }
        i += 8;
    }
    i + d[i..].iter().position(|&b| b == 0xff).unwrap_or(d.len() - i)
}

/// The frame's samples (`total` of them, before the point transform), or `None` when the frame is not the layout
/// this path decodes or its stream is not clean (see the module docs).
pub(super) fn decode(h: &Header, entropy: &[u8], tables: &[&Huffman], total: usize) -> Option<Vec<u16>> {
    let (w, ht, nc) = (h.width, h.height, h.comps.len());
    let one_table = h.table_for.windows(2).all(|t| t[0] == t[1]);
    if h.predictor != 1 || h.restart != 0 || !one_table || !(1..=4).contains(&nc) || total != w * ht * nc {
        return None;
    }
    let table = *tables.first()?;
    let lookup = symbols(table);
    let (clean, len) = unstuff(entropy);
    let nbits = len as u64 * 8;
    let segments = len.div_ceil(SEGMENT_BYTES).clamp(1, MAX_SEGMENTS);
    let cut = |k: usize| if k == segments { nbits } else { nbits * k as u64 / segments as u64 };
    let bounds: Vec<(u64, u64)> = (0..segments).map(|k| (cut(k), cut(k + 1))).collect();
    let per_bit = total as f64 / nbits.max(1) as f64;
    let runs: Vec<Run> = bounds
        .par_iter()
        .map(|&(from, to)| run(&clean, &lookup, table, from, to, ((to - from) as f64 * per_bit * 1.1) as usize + 64))
        .collect();

    // Stitch: the true position meets each segment's decode; the symbols before that are decoded here.
    let mut pieces: Vec<Vec<u16>> = Vec::new();
    let mut kept: Vec<(usize, usize, usize)> = Vec::new(); // (run, first, count) in frame order, with `pieces` slots
    let mut pos = 0u64;
    let mut count = 0usize;
    let mut valid_end = 0u64;
    let invalid_in_frame = |list: &Option<Vec<(u64, usize)>>, from_pos: u64, first: usize, before: usize| match list {
        None => true,
        Some(l) => l.iter().any(|&(q, at)| q >= from_pos && before + (at - first) < total),
    };
    for (k, (r, &(_, to))) in runs.iter().zip(&bounds).enumerate() {
        if count >= total {
            break;
        }
        let mut stepped = Vec::new();
        loop {
            if pos >= to {
                break;
            }
            if let Ok(j) = r.starts.binary_search(&pos) {
                let before = count + stepped.len();
                if invalid_in_frame(&r.invalid, pos, j, before) {
                    return None;
                }
                pieces.push(std::mem::take(&mut stepped));
                kept.push((usize::MAX, pieces.len() - 1, 0));
                kept.push((k, j, r.diffs.len() - j));
                count = before + (r.diffs.len() - j);
                pos = r.end;
                valid_end = r.valid_end;
                break;
            }
            if r.starts.last().is_none_or(|&last| pos > last) {
                // this segment's decode never met the true position: decode the rest of it here
                let rest = run(&clean, &lookup, table, pos, to, 0);
                let before = count + stepped.len();
                if invalid_in_frame(&rest.invalid, pos, 0, before) {
                    return None;
                }
                stepped.extend_from_slice(&rest.diffs);
                pos = rest.end;
                valid_end = rest.valid_end;
                break;
            }
            let (bits, diff) = symbol(&lookup, table, window(&clean, pos))?;
            stepped.push(diff);
            pos += u64::from(bits);
            valid_end = pos;
        }
        if !stepped.is_empty() {
            count += stepped.len();
            pieces.push(stepped);
            kept.push((usize::MAX, pieces.len() - 1, 0));
        }
    }
    // the frame must end inside the stream: past its end `decode` reads zeros, and may fail on them
    if count < total || (count == total && valid_end > nbits) {
        return None;
    }
    drop(clean);

    // The differences in frame order, as slices, and where each starts.
    let mut slices: Vec<&[u16]> = Vec::new();
    for &(k, first, n) in &kept {
        slices.push(if k == usize::MAX { &pieces[first][..] } else { &runs[k].diffs[first..first + n] });
    }
    let mut starts = Vec::with_capacity(slices.len());
    let mut at = 0usize;
    for s in &slices {
        starts.push(at);
        at += s.len();
    }
    // copy the differences of frame samples `[from, from + dst.len())` into `dst`
    let fill = |dst: &mut [u16], from: usize| {
        let mut i = starts.partition_point(|&s| s <= from).saturating_sub(1);
        let mut done = 0;
        while done < dst.len() && i < slices.len() {
            let off = from + done - starts[i];
            let n = (slices[i].len().saturating_sub(off)).min(dst.len() - done);
            dst[done..done + n].copy_from_slice(&slices[i][off..off + n]);
            done += n;
            i += 1;
        }
    };

    // Predictor 1: the first sample of each row predicts from the one above (the first row from 2^(P-Pt-1)), the
    // others from their left neighbour of the same component; modulo 2^16 as `decode` sums them.
    let row_len = w * nc;
    let init = (1u32 << (h.precision - h.pt - 1)) as u16;
    let mut first = vec![0u16; ht * nc];
    let mut d = vec![0u16; nc];
    for y in 0..ht {
        fill(&mut d, y * row_len);
        for c in 0..nc {
            let above = if y == 0 { init } else { first[(y - 1) * nc + c] };
            first[y * nc + c] = above.wrapping_add(d[c]);
        }
    }
    let mut out = vec![0u16; total];
    let pt = u32::from(h.pt);
    out.par_chunks_mut(row_len).enumerate().for_each(|(y, row)| {
        fill(row, y * row_len);
        let mut acc = [0u16; 4];
        acc[..nc].copy_from_slice(&first[y * nc..(y + 1) * nc]);
        for (c, v) in row[..nc].iter_mut().enumerate() {
            *v = acc[c];
        }
        for px in row[nc..].chunks_exact_mut(nc) {
            for (c, v) in px.iter_mut().enumerate() {
                acc[c] = acc[c].wrapping_add(*v);
                *v = acc[c];
            }
        }
        if pt > 0 {
            for v in row.iter_mut() {
                *v <<= pt;
            }
        }
    });
    Some(out)
}
