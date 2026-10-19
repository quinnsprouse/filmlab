//! Filmlab: a DCT-scaled decode of a JPEG with restart markers, cut into bands of MCU rows decoded on all cores,
//! with the same pixels as one `jpeg-decoder` decode of the whole file.
//!
//! why: a raw's camera preview is decoded at 1/8 scale on every open and every thumbnail (the camera look is
//! fitted to it); for a Ricoh GR III DNG that is a 6000 x 4000 JPEG, ~35 ms of serial Huffman decoding, as long as
//! the rest of a 2560 px decode. Camera JPEGs carry restart markers: the entropy coding starts afresh at each one.
//!
//! Each band is a JPEG of its own: the file's segments with the frame height cut to the band, the scan data
//! between two restart markers (renumbered from RST0) and an end-of-image marker. It starts where a restart
//! interval and an MCU row start together, so `jpeg-decoder` decodes its blocks exactly as it decodes them in the
//! whole file (fresh DC predictors, the same tables, the same DCT scale). Upsampled chroma reads one chroma row
//! above and below: each band is decoded with a margin of MCU rows around the rows it keeps, so every kept row has
//! the same neighbours. Any structure this path doesn't expect returns `None`, and the whole file is decoded as
//! before.

use jpeg_decoder::{Decoder, PixelFormat};
use rayon::prelude::*;

/// What the frame and scan headers say: (SOF segment offset, width, height, MCU columns, MCU rows, MCU height in
/// pixels, max vertical sampling, restart interval, scan data offset, components).
struct Layout {
    sof: usize,
    width: usize,
    height: usize,
    mcus_x: usize,
    mcus_y: usize,
    mcu_h: usize,
    v_max: usize,
    restart: usize,
    scan: usize,
    components: usize,
}

fn be16(b: &[u8], i: usize) -> Option<usize> {
    b.get(i..i + 2).map(|s| usize::from(u16::from_be_bytes([s[0], s[1]])))
}

/// One baseline (or extended) 8-bit sequential frame, one interleaved scan of all components, restart markers.
fn layout(b: &[u8]) -> Option<Layout> {
    let mut p = 2;
    let (mut sof, mut restart, mut frame) = (None, 0, None);
    loop {
        if *b.get(p)? != 0xFF {
            return None;
        }
        let marker = *b.get(p + 1)?;
        let len = be16(b, p + 2)?;
        let seg = b.get(p + 4..p + 2 + len)?;
        match marker {
            0xC0 | 0xC1 if sof.is_none() => {
                let n = usize::from(*seg.get(5)?);
                if seg[0] != 8 || !(n == 1 || n == 3) || seg.len() < 6 + 3 * n {
                    return None;
                }
                let sampling: Vec<(usize, usize)> = (0..n).map(|c| (usize::from(seg[7 + 3 * c] >> 4), usize::from(seg[7 + 3 * c] & 15))).collect();
                sof = Some(p);
                frame = Some((be16(seg, 3)?, be16(seg, 1)?, n, sampling));
            }
            0xDD => restart = be16(seg, 0)?,
            0xDA => {
                let (width, height, n, sampling) = frame?;
                // all components in this one scan, the whole spectrum, no successive approximation
                if usize::from(*seg.first()?) != n || seg.get(1 + 2 * n..4 + 2 * n)? != [0, 63, 0] {
                    return None;
                }
                let h_max = sampling.iter().map(|s| s.0).max()?;
                let v_max = sampling.iter().map(|s| s.1).max()?;
                if !(1..=4).contains(&h_max) || !(1..=4).contains(&v_max) || sampling.iter().any(|s| s.0 == 0 || s.1 == 0) || width == 0 || height == 0 {
                    return None;
                }
                let (mcu_w, mcu_h) = (8 * h_max, 8 * v_max);
                return Some(Layout {
                    sof: sof?,
                    width,
                    height,
                    mcus_x: width.div_ceil(mcu_w),
                    mcus_y: height.div_ceil(mcu_h),
                    mcu_h,
                    v_max,
                    restart,
                    scan: p + 2 + len,
                    components: n,
                });
            }
            // tables, application data and comments; anything else (another frame type, DNL, arithmetic coding) is not
            // this layout
            0xC4 | 0xDB | 0xE0..=0xEF | 0xFE => {}
            _ => return None,
        }
        p += 2 + len;
    }
}

/// Positions of the restart markers of the scan starting at `scan`, and of the end-of-image marker after it.
/// `None` unless the markers come in order (RST0, RST1, …) and the scan ends with EOI.
fn markers(b: &[u8], scan: usize) -> Option<(Vec<usize>, usize)> {
    let mut rst = Vec::new();
    let mut p = scan;
    loop {
        p += super::find_ff(b.get(p..)?)?;
        match *b.get(p + 1)? {
            0x00 => p += 2,
            m @ 0xD0..=0xD7 => {
                if usize::from(m - 0xD0) != rst.len() % 8 {
                    return None;
                }
                rst.push(p);
                p += 2;
            }
            0xD9 => return Some((rst, p)),
            _ => return None,
        }
    }
}

/// The pixels of a DCT-scaled decode of `b` asking for `request` (what [`Decoder::scale`] is given), with their
/// width, height and format, or `None` (see the module docs).
pub(super) fn decode(b: &[u8], request: (u16, u16)) -> Option<(Vec<u8>, usize, usize, PixelFormat)> {
    let l = layout(b)?;
    if l.restart == 0 || l.width < 64 {
        return None;
    }
    let mcus = l.mcus_x * l.mcus_y;
    let intervals = mcus.div_ceil(l.restart);
    let (rst, eoi) = markers(b, l.scan)?;
    if rst.len() + 1 != intervals {
        return None;
    }
    // the whole file's decode: its size and format
    let mut whole = Decoder::new(b);
    whole.read_info().ok()?;
    let (fw, fh) = whole.scale(request.0, request.1).ok()?;
    let format = whole.info()?.pixel_format;
    let (fw, fh) = (usize::from(fw), usize::from(fh));
    let scale = [1, 2, 4, 8].into_iter().find(|&s| (l.width * s).div_ceil(8) == fw && (l.height * s).div_ceil(8) == fh)?;
    if !matches!((format, l.components), (PixelFormat::RGB24, 3) | (PixelFormat::L8, 1)) {
        return None;
    }
    // bands start where an MCU row and a restart interval start together: every `period` MCU rows
    let period = l.restart / gcd(l.restart, l.mcus_x);
    let units = l.mcus_y.div_ceil(period);
    let bands = units.min(rayon::current_num_threads());
    if bands < 2 {
        return None;
    }
    let band = units.div_ceil(bands) * period;
    let rows_per_mcu = l.v_max * scale;
    let decoded: Option<Vec<Vec<u8>>> = (0..l.mcus_y.div_ceil(band))
        .into_par_iter()
        .map(|i| {
            let (keep0, keep1) = (i * band, ((i + 1) * band).min(l.mcus_y));
            let (r0, r1) = (keep0.saturating_sub(period), (keep1 + period).min(l.mcus_y));
            let (i0, i1) = (r0 * l.mcus_x / l.restart, if r1 == l.mcus_y { intervals } else { r1 * l.mcus_x / l.restart });
            let height = if r1 == l.mcus_y { l.height - r0 * l.mcu_h } else { (r1 - r0) * l.mcu_h };
            let jpeg = band_file(b, &l, &rst, eoi, (i0, i1), u16::try_from(height).ok()?);
            let mut d = Decoder::new(&jpeg[..]);
            d.read_info().ok()?;
            let (w, h) = d.scale(fw as u16, u16::MAX).ok()?;
            if usize::from(w) != fw || usize::from(h) != (height * scale).div_ceil(8) {
                return None;
            }
            let px = d.decode().ok()?;
            let line = fw * l.components;
            let (skip, take) = ((keep0 - r0) * rows_per_mcu, (keep1 * rows_per_mcu).min(fh) - keep0 * rows_per_mcu);
            px.get(skip * line..(skip + take) * line).map(<[u8]>::to_vec)
        })
        .collect();
    let px = decoded?.concat();
    (px.len() == fw * fh * l.components).then_some((px, fw, fh, format))
}

/// The JPEG of restart intervals `[i0, i1)`: the file's segments up to the scan with the frame height set to
/// `height`, those intervals' scan data with their markers renumbered from RST0, and EOI.
fn band_file(b: &[u8], l: &Layout, rst: &[usize], eoi: usize, (i0, i1): (usize, usize), height: u16) -> Vec<u8> {
    let start = if i0 == 0 { l.scan } else { rst[i0 - 1] + 2 };
    let end = if i1 == rst.len() + 1 { eoi } else { rst[i1 - 1] };
    let mut out = Vec::with_capacity(l.scan + end - start + 2);
    out.extend_from_slice(&b[..l.scan]);
    out[l.sof + 5..l.sof + 7].copy_from_slice(&height.to_be_bytes());
    let mut from = start;
    for (k, &at) in rst.iter().enumerate().take(i1.saturating_sub(1)).skip(i0) {
        out.extend_from_slice(&b[from..at]);
        out.extend_from_slice(&[0xFF, 0xD0 + ((k - i0) % 8) as u8]);
        from = at + 2;
    }
    out.extend_from_slice(&b[from..end]);
    out.extend_from_slice(&[0xFF, 0xD9]);
    out
}

fn gcd(a: usize, b: usize) -> usize {
    if b == 0 { a } else { gcd(b, a % b) }
}
