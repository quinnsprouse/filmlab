use super::curve::{Area, Handles, Intake, Levels, Mapped};
use super::look::Shot;

const WIDTH: usize = 384;
const HEIGHT: usize = 400;
const INSET: f64 = 12.0;
const SIDE: f64 = 360.0;
const RAIL: f64 = 4.0;
const LINE: f64 = 2.8;
const POINT: f64 = 6.0;
const POINT_GROUND: f64 = 2.0;
const RING: f64 = 9.5;
const DASH: (f64, f64) = (4.0, 4.0);
const HISTOGRAM_REACH: f64 = 0.8;
const QUARTERS: [f64; 3] = [0.25, 0.5, 0.75];
const GROUND: [u8; 3] = [0x30, 0x30, 0x30];
const WHITE: [u8; 3] = [0xff, 0xff, 0xff];
const DOTS: [[u8; 3]; 4] = [[0xf2, 0xf2, 0xf2], INKS[1], INKS[2], INKS[3]];
const INKS: [[u8; 3]; 4] = [[0xeb, 0xeb, 0xeb], [0xf7, 0x7a, 0x69], [0x5c, 0xbe, 0x56], [0x7b, 0xa3, 0xf6]];

type At = (f64, f64);

fn x(level: f64) -> f64 {
    INSET + level / 255.0 * SIDE
}

fn y(level: f64) -> f64 {
    INSET + (1.0 - level / 255.0) * SIDE
}

fn traced(levels: &Levels) -> Vec<At> {
    levels.iter().enumerate().map(|(i, v)| (x(i as f64), y(*v))).collect()
}

fn distance(p: At, a: At, b: At) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let length = dx * dx + dy * dy;
    let t = if length > 0.0 { (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / length).clamp(0.0, 1.0) } else { 0.0 };
    (p.0 - a.0 - t * dx).hypot(p.1 - a.1 - t * dy)
}

fn dashed(path: &[At], (on, off): (f64, f64)) -> Vec<(At, At)> {
    let mut out = Vec::new();
    let mut walked = 0.0;
    for w in path.windows(2) {
        let (a, b) = (w[0], w[1]);
        let length = (b.0 - a.0).hypot(b.1 - a.1);
        let at = |t: f64| (a.0 + (b.0 - a.0) * t / length.max(1e-9), a.1 + (b.1 - a.1) * t / length.max(1e-9));
        let mut t = 0.0;
        while t < length {
            let phase = walked % (on + off);
            let step = if phase < on { on - phase } else { on + off - phase }.min(length - t);
            if phase < on {
                out.push((at(t), at(t + step)));
            }
            t += step;
            walked += step;
        }
    }
    out
}

struct Canvas {
    rgba: Vec<u8>,
}

impl Canvas {
    fn new() -> Canvas {
        Canvas { rgba: [GROUND[0], GROUND[1], GROUND[2], 0xff].repeat(WIDTH * HEIGHT) }
    }

    fn blend(&mut self, px: usize, py: usize, color: [u8; 3], alpha: f64) {
        if px >= WIDTH || py >= HEIGHT || alpha <= 0.0 {
            return;
        }
        let i = (py * WIDTH + px) * 4;
        for (k, c) in color.iter().enumerate() {
            let d = f64::from(self.rgba[i + k]);
            self.rgba[i + k] = (d + (f64::from(*c) - d) * alpha.min(1.0)).round() as u8;
        }
    }

    fn area(&mut self, from: At, to: At, cover: impl Fn(At) -> f64, color: [u8; 3], alpha: f64) {
        let (x0, y0) = (from.0.floor().max(0.0) as usize, from.1.floor().max(0.0) as usize);
        let (x1, y1) = ((to.0.ceil().max(0.0) as usize).min(WIDTH - 1), (to.1.ceil().max(0.0) as usize).min(HEIGHT - 1));
        for py in y0..=y1 {
            for px in x0..=x1 {
                let c = cover((px as f64 + 0.5, py as f64 + 0.5));
                self.blend(px, py, color, alpha * c);
            }
        }
    }

    fn rect(&mut self, from: At, to: At, color: [u8; 3], alpha: f64) {
        let overlap = |lo: f64, hi: f64, p: f64| (hi.min(p + 0.5) - lo.max(p - 0.5)).clamp(0.0, 1.0);
        self.area(from, to, |p| overlap(from.0, to.0, p.0) * overlap(from.1, to.1, p.1), color, alpha);
    }

    fn disc(&mut self, at: At, radius: f64, color: [u8; 3], alpha: f64) {
        let reach = radius + 1.0;
        self.area((at.0 - reach, at.1 - reach), (at.0 + reach, at.1 + reach), |p| (radius + 0.5 - (p.0 - at.0).hypot(p.1 - at.1)).clamp(0.0, 1.0), color, alpha);
    }

    fn ring(&mut self, at: At, radius: f64, width: f64, color: [u8; 3], alpha: f64) {
        let reach = radius + width + 1.0;
        let band = |d: f64| ((width / 2.0 + 0.5) - (d - radius).abs()).clamp(0.0, 1.0);
        self.area((at.0 - reach, at.1 - reach), (at.0 + reach, at.1 + reach), |p| band((p.0 - at.0).hypot(p.1 - at.1)), color, alpha);
    }

    fn stroke(&mut self, segments: &[(At, At)], width: f64, color: [u8; 3], alpha: f64) {
        let half = width / 2.0;
        let mut cover = vec![0.0f64; WIDTH * HEIGHT];
        let (mut lo, mut hi) = ((WIDTH, HEIGHT), (0, 0));
        for (a, b) in segments {
            let x0 = (a.0.min(b.0) - half - 1.0).floor().max(0.0) as usize;
            let y0 = (a.1.min(b.1) - half - 1.0).floor().max(0.0) as usize;
            let x1 = ((a.0.max(b.0) + half + 1.0).ceil().max(0.0) as usize).min(WIDTH - 1);
            let y1 = ((a.1.max(b.1) + half + 1.0).ceil().max(0.0) as usize).min(HEIGHT - 1);
            for py in y0..=y1 {
                for px in x0..=x1 {
                    let c = (half + 0.5 - distance((px as f64 + 0.5, py as f64 + 0.5), *a, *b)).clamp(0.0, 1.0);
                    let slot = &mut cover[py * WIDTH + px];
                    *slot = slot.max(c);
                }
            }
            lo = (lo.0.min(x0), lo.1.min(y0));
            hi = (hi.0.max(x1), hi.1.max(y1));
        }
        for py in lo.1..=hi.1.min(HEIGHT - 1) {
            for px in lo.0..=hi.0.min(WIDTH - 1) {
                self.blend(px, py, color, alpha * cover[py * WIDTH + px]);
            }
        }
    }

    fn line(&mut self, path: &[At], width: f64, color: [u8; 3], alpha: f64) {
        let segments: Vec<(At, At)> = path.windows(2).map(|w| (w[0], w[1])).collect();
        self.stroke(&segments, width, color, alpha);
    }

    fn shot(self) -> Shot {
        Shot { rgba: self.rgba, width: WIDTH as u32, height: HEIGHT as u32 }
    }
}

fn profile(counts: &Levels) -> Vec<f64> {
    let smooth: Vec<f64> = (0..256usize)
        .map(|i| {
            let (lo, hi) = (i.saturating_sub(2), (i + 3).min(256));
            counts[lo..hi].iter().sum::<f64>() / (hi - lo) as f64
        })
        .collect();
    let mut inner = smooth[2..254].to_vec();
    inner.sort_by(f64::total_cmp);
    let peak = inner[(inner.len() as f64 * 0.995) as usize].max(1.0);
    smooth.iter().map(|v| (v / peak).min(1.0).sqrt()).collect()
}

fn inverse(levels: &Levels, wanted: f64) -> f64 {
    match levels.iter().position(|v| *v >= wanted) {
        None => 255.0,
        Some(0) => 0.0,
        Some(i) => {
            let (a, b) = (levels[i - 1], levels[i]);
            (i - 1) as f64 + if b > a { (wanted - a) / (b - a) } else { 0.0 }
        }
    }
}

fn histogram(canvas: &mut Canvas, intake: &Intake) {
    let heights = profile(&intake.hist[0]);
    let bottom = y(0.0);
    for px in INSET as usize..(INSET + SIDE) as usize {
        let u = ((px as f64 + 0.5 - INSET) / SIDE * 255.0).clamp(0.0, 255.0);
        let (i, t) = ((u.floor() as usize).min(254), u - u.floor().min(254.0));
        let top = bottom - (heights[i] + (heights[i + 1] - heights[i]) * t) * HISTOGRAM_REACH * SIDE;
        canvas.rect((px as f64, top), (px as f64 + 1.0, bottom), WHITE, 0.08);
    }
}

fn grid(canvas: &mut Canvas) {
    let (top, bottom, left, right) = (y(255.0), y(0.0), x(0.0), x(255.0));
    for q in QUARTERS {
        let (gx, gy) = (x(q * 255.0), y(q * 255.0));
        canvas.rect((gx - 0.5, top), (gx + 0.5, bottom), WHITE, 0.06);
        canvas.rect((left, gy - 0.5), (right, gy + 0.5), WHITE, 0.06);
    }
    canvas.line(&[(left, bottom), (right, top)], 1.0, WHITE, 0.18);
}

fn rail(canvas: &mut Canvas) {
    let top = y(0.0) + INSET;
    for px in INSET as usize..(INSET + SIDE) as usize {
        let grey = ((px as f64 + 0.5 - INSET) / SIDE * 255.0).round().clamp(0.0, 255.0) as u8;
        canvas.rect((px as f64, top), (px as f64 + 1.0, top + RAIL), [grey; 3], 1.0);
    }
}

fn marked(canvas: &mut Canvas, m: &Mapped, area: &Area) {
    let middle = area.tones[1];
    let ring = (x(middle), y(m.at(0, middle)));
    canvas.stroke(&dashed(&[(ring.0, ring.1 + RING + 2.0), (ring.0, y(0.0))], DASH), 1.0, WHITE, 0.3);
    canvas.ring(ring, RING + 2.0, 4.0, GROUND, 1.0);
    canvas.ring(ring, RING, 1.8, WHITE, 1.0);
    let rail = y(0.0) + INSET + RAIL / 2.0;
    canvas.rect((ring.0 - 0.75, rail - 7.5), (ring.0 + 0.75, rail + 7.5), WHITE, 1.0);
}

pub fn draw(m: &Mapped, points: &[Handles], ghost: Option<&Mapped>, intake: &Intake, area: Option<&Area>) -> Shot {
    let mut canvas = Canvas::new();
    histogram(&mut canvas, intake);
    grid(&mut canvas);
    if let Some(area) = area {
        let [low, _, high] = area.tones;
        canvas.rect((x(low), y(255.0)), (x(high).max(x(low) + 1.0), y(0.0)), WHITE, 0.07);
    }
    if let Some(g) = ghost {
        let tinted = g.tinted();
        for c in (1..4).filter(|c| tinted[c - 1]) {
            canvas.stroke(&dashed(&traced(&g.levels[c]), DASH), 1.4, INKS[c], 0.55);
        }
        canvas.stroke(&dashed(&traced(&g.levels[0]), DASH), 1.4, WHITE, 0.4);
    }
    let tinted = m.tinted();
    for c in (1..4).filter(|c| tinted[c - 1]) {
        canvas.line(&traced(&m.levels[c]), LINE, INKS[c], 1.0);
    }
    canvas.line(&traced(&m.levels[0]), LINE, INKS[0], 1.0);
    for (channel, list) in points.iter().rev() {
        let inner = if *channel == 0 { &m.regions } else { &m.levels[0] };
        let reach = inner[0] - 0.5..=inner[255] + 0.5;
        for [px, py] in list.iter().filter(|p| reach.contains(&p[0])) {
            let at = (x(inverse(inner, *px)), y(*py));
            canvas.disc(at, POINT + POINT_GROUND, GROUND, 1.0);
            canvas.disc(at, POINT, DOTS[*channel], 1.0);
        }
    }
    rail(&mut canvas);
    if let Some(area) = area {
        marked(&mut canvas, m, area);
    }
    canvas.shot()
}
