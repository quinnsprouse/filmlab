use std::collections::HashMap;

use gpui_kit::*;

use super::oklch::{self, Reach};
use super::panel::MIXER_BANDS;

const LIGHTNESS: f64 = 0.72;
const REACH: Reach = Reach { share_of_gamut: 0.9, cap: 0.17 };
const SWING_DEGREES: f64 = 28.65;
const MIXER_HSV_HUES: [f64; 8] = [0.0, 30.0, 60.0, 120.0, 180.0, 225.0, 270.0, 315.0];
const BLUE_TO_AMBER: (f64, f64) = (250.0, 75.0);
const GREEN_TO_MAGENTA: (f64, f64) = (145.0, 338.0);
const PRIMARY_HUES: [(&str, f64); 3] = [("red", 24.2), ("green", 152.6), ("blue", 245.1)];
const INSET: f32 = 10.0;
const HEIGHT: f32 = 3.0;
const FOOT: f32 = 4.0;
const SEGMENTS: usize = 16;
const EMERGING_SEGMENTS: usize = 24;
const SHADE: u32 = 0x252525b8;

#[derive(Clone, Copy, PartialEq)]
pub enum ColorRail {
    Balance { neutral: f64, below: f64, above: f64 },
    Emerging,
    Swing { centre: f64 },
    Chroma { hue: f64, floor: f64 },
    Tone { hue: f64, dark: f64, span: f64, strength: f64 },
    Spectrum { from: f64, to: f64 },
    HalationHue,
    HalationSaturation { halation_hue: f64 },
}

struct Swatch {
    lightness: f64,
    strength: f64,
    hue: f64,
}

pub fn ink(encoded: [f64; 3]) -> Hsla {
    REACH.color(LIGHTNESS, 1.0, oklch::hue_of_srgb(encoded)).into()
}

pub fn halation_tint_hue(halation_hue: f64) -> f64 {
    oklch::hue_of_srgb([1.0, 0.85 * halation_hue, 0.0])
}

impl ColorRail {
    pub fn temp(neutral: f64) -> Self {
        let (below, above) = BLUE_TO_AMBER;
        ColorRail::Balance { neutral, below, above }
    }

    pub fn tint(neutral: f64) -> Self {
        let (below, above) = GREEN_TO_MAGENTA;
        ColorRail::Balance { neutral, below, above }
    }

    fn swatch(self, t: f64) -> Swatch {
        let vivid = |hue| Swatch { lightness: LIGHTNESS, strength: 1.0, hue };
        match self {
            ColorRail::Balance { neutral, below, above } => {
                let neutral = neutral.clamp(1e-3, 1.0 - 1e-3);
                if t < neutral {
                    Swatch { lightness: LIGHTNESS, strength: (neutral - t) / neutral, hue: below }
                } else {
                    Swatch { lightness: LIGHTNESS, strength: (t - neutral) / (1.0 - neutral), hue: above }
                }
            }
            ColorRail::Emerging => Swatch { lightness: LIGHTNESS, strength: t, hue: oklch::hue_of_hsv((t * 2.5).fract() * 360.0) },
            ColorRail::Swing { centre } => vivid(centre + (2.0 * t - 1.0) * SWING_DEGREES),
            ColorRail::Chroma { hue, floor } => Swatch { lightness: LIGHTNESS, strength: floor + (1.0 - floor) * t, hue },
            ColorRail::Tone { hue, dark, span, strength } => Swatch { lightness: dark + span * t, strength, hue },
            ColorRail::Spectrum { from, to } => vivid(oklch::hue_of_hsv(from + (to - from) * t)),
            ColorRail::HalationHue => vivid(halation_tint_hue(t)),
            ColorRail::HalationSaturation { halation_hue } => Swatch { lightness: LIGHTNESS + 0.12 * (1.0 - t), strength: t, hue: halation_tint_hue(halation_hue) },
        }
    }

    fn color(self, t: f64, gray: bool) -> Hsla {
        let Swatch { lightness, strength, hue } = self.swatch(t.clamp(0.0, 1.0));
        REACH.color(lightness, if gray { 0.0 } else { strength }, hue).into()
    }

    fn segments(self) -> usize {
        if self == ColorRail::Emerging { EMERGING_SEGMENTS } else { SEGMENTS }
    }

    fn neutral(self) -> Option<f64> {
        match self {
            ColorRail::Balance { neutral, .. } => Some(neutral),
            _ => None,
        }
    }

    pub fn for_control(id: &str, values: &HashMap<String, f64>) -> Option<Self> {
        let parts: Vec<&str> = id.split('.').collect();
        match parts.as_slice() {
            ["color", "vibrance" | "saturation"] => Some(ColorRail::Emerging),
            ["mixer", band, channel] => {
                let hue = band_hue(band)?;
                match *channel {
                    "hue" => Some(ColorRail::Swing { centre: hue }),
                    "sat" => Some(ColorRail::Chroma { hue, floor: 0.0 }),
                    "lum" => Some(ColorRail::Tone { hue, dark: 0.36, span: 0.5, strength: 0.9 }),
                    _ => None,
                }
            }
            ["bw", band] => Some(ColorRail::Tone { hue: band_hue(band)?, dark: 0.34, span: 0.56, strength: 0.32 }),
            ["grading", region, part] => {
                let hue = oklch::hue_of_hsv(values.get(&format!("grading.{region}.hue")).copied().unwrap_or(0.0));
                match *part {
                    "hue" => Some(ColorRail::Spectrum { from: 0.0, to: 360.0 }),
                    "sat" => Some(ColorRail::Chroma { hue, floor: 0.0 }),
                    "lum" => Some(ColorRail::Tone { hue, dark: 0.36, span: 0.5, strength: 0.4 }),
                    _ => None,
                }
            }
            ["calibration", "shadowsTint"] => Some(ColorRail::tint(0.5)),
            ["calibration", primary] => {
                let (name, hue) = PRIMARY_HUES.iter().find(|(name, _)| primary.starts_with(name))?;
                match &primary[name.len()..] {
                    "Hue" => Some(ColorRail::Swing { centre: *hue }),
                    "Sat" => Some(ColorRail::Chroma { hue: *hue, floor: 0.25 }),
                    _ => None,
                }
            }
            _ => None,
        }
    }
}

fn band_hue(band: &str) -> Option<f64> {
    let index = MIXER_BANDS.iter().position(|b| *b == band)?;
    Some(oklch::hue_of_hsv(MIXER_HSV_HUES[index]))
}

pub fn strip(rail: ColorRail, gray: bool) -> impl IntoElement {
    placed(canvas(|_, _, _| {}, move |bounds, _, window, _| paint(rail, gray, bounds, window)))
}

pub fn strip_within(rail: ColorRail, kept: (f32, f32)) -> impl IntoElement {
    placed(canvas(|_, _, _| {}, move |bounds, _, window, _| {
        paint(rail, false, bounds, window);
        shade_outside(kept, bounds, window);
    }))
}

fn placed(strip: Canvas<()>) -> Canvas<()> {
    strip.absolute().left(px(INSET)).right(px(INSET)).bottom(px(FOOT)).h(px(HEIGHT))
}

struct Track {
    bounds: Bounds<Pixels>,
}

impl Track {
    fn width(&self) -> f64 {
        f64::from(f32::from(self.bounds.size.width))
    }

    fn row(&self) -> f64 {
        self.width() + 2.0 * f64::from(INSET)
    }

    fn value_at(&self, u: f64) -> f64 {
        (f64::from(INSET) + u * self.width()) / self.row()
    }

    fn u_at(&self, t: f64) -> f64 {
        (t * self.row() - f64::from(INSET)) / self.width().max(1.0)
    }

    fn x_at(&self, u: f64) -> Pixels {
        self.bounds.left() + px((u * self.width()) as f32)
    }

    fn span(&self, from: f64, to: f64) -> Bounds<Pixels> {
        Bounds::from_corners(point(self.x_at(from), self.bounds.top()), point(self.x_at(to), self.bounds.bottom()))
    }
}

fn rounded(left: bool, right: bool) -> Corners<Pixels> {
    let (l, r) = (if left { px(HEIGHT / 2.0) } else { px(0.0) }, if right { px(HEIGHT / 2.0) } else { px(0.0) });
    Corners { top_left: l, bottom_left: l, top_right: r, bottom_right: r }
}

// why: GPUI's linear gradient takes two stops, so the rail is painted as adjacent two-stop segments
fn paint(rail: ColorRail, gray: bool, bounds: Bounds<Pixels>, window: &mut Window) {
    let track = Track { bounds };
    let count = rail.segments();
    let mut stops: Vec<f64> = (0..=count).map(|i| i as f64 / count as f64).collect();
    if let Some(neutral) = rail.neutral().map(|t| track.u_at(t)).filter(|u| *u > 0.0 && *u < 1.0) {
        stops.push(neutral);
        stops.sort_by(f64::total_cmp);
    }
    let last = stops.len() - 2;
    for (i, pair) in stops.windows(2).enumerate() {
        let (from, to) = (pair[0], pair[1]);
        let gradient = linear_gradient(90.0, linear_color_stop(rail.color(track.value_at(from), gray), 0.0), linear_color_stop(rail.color(track.value_at(to), gray), 1.0)).color_space(ColorSpace::Oklab);
        window.paint_quad(fill(track.span(from, to), gradient).corner_radii(rounded(i == 0, i == last)));
    }
}

fn shade_outside((low, high): (f32, f32), bounds: Bounds<Pixels>, window: &mut Window) {
    let track = Track { bounds };
    let (low, high) = (track.u_at(f64::from(low)), track.u_at(f64::from(high)));
    if low > 0.0 {
        window.paint_quad(fill(track.span(0.0, low.min(1.0)), rgba(SHADE)).corner_radii(rounded(true, low >= 1.0)));
    }
    if high < 1.0 {
        window.paint_quad(fill(track.span(high.max(0.0), 1.0), rgba(SHADE)).corner_radii(rounded(high <= 0.0, true)));
    }
}
