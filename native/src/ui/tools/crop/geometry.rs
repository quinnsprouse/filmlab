use filmlab_engine::core::{self, Facts, MapRequest};

use crate::settings::Settings;

pub type Vec2 = [f64; 2];

#[derive(Clone, Copy)]
pub struct Rect {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

const FULL: Rect = Rect { x0: 0.0, y0: 0.0, x1: 1.0, y1: 1.0 };

impl Rect {
    pub fn array(self) -> [f64; 4] {
        [self.x0, self.y0, self.x1, self.y1]
    }

    pub fn contains(self, p: Vec2) -> bool {
        p[0] >= self.x0 && p[0] <= self.x1 && p[1] >= self.y0 && p[1] <= self.y1
    }
}

#[derive(Clone, Copy)]
pub struct Crop {
    pub rect: Rect,
    pub angle: f64,
    pub aspect: Option<Vec2>,
}

fn finite(value: f64, fallback: f64) -> f64 {
    if value.is_finite() { value } else { fallback }
}

pub fn read_crop(settings: &Settings) -> Crop {
    let crop = &settings.crop;
    let (geometry, rect) = (&crop.geometry, &crop.geometry.rect);
    Crop {
        rect: Rect { x0: finite(rect.x0, FULL.x0), y0: finite(rect.y0, FULL.y0), x1: finite(rect.x1, FULL.x1), y1: finite(rect.y1, FULL.y1) },
        angle: finite(geometry.angle, 0.0),
        aspect: crop.aspect.filter(|&(w, h)| w > 0 && h > 0).map(|(w, h)| [f64::from(w), f64::from(h)]),
    }
}

fn rotate_about(p: Vec2, degrees: f64, centre: Vec2) -> Vec2 {
    let (sin, cos) = degrees.to_radians().sin_cos();
    let (dx, dy) = (p[0] - centre[0], p[1] - centre[1]);
    [centre[0] + cos * dx - sin * dy, centre[1] + sin * dx + cos * dy]
}

/// Maps between the uncropped, upright frame (in frame pixels) and normalised straightened coordinates,
/// through `core::map_points`.
pub struct FrameMap {
    width: f64,
    height: f64,
    origin: Vec2,
    unit_x: Vec2,
    unit_y: Vec2,
}

impl FrameMap {
    pub fn new(facts: &Facts, settings: &Settings, size: (u32, u32)) -> Option<FrameMap> {
        let request = MapRequest { width: size.0 as usize, height: size.1 as usize, apply_crop: false, points: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]], to_image: false };
        let points = core::mapped_points(facts, settings, &request);
        let [origin, x, y] = [*points.first()?, *points.get(1)?, *points.get(2)?];
        Some(FrameMap { width: f64::from(size.0), height: f64::from(size.1), origin, unit_x: [x[0] - origin[0], x[1] - origin[1]], unit_y: [y[0] - origin[0], y[1] - origin[1]] })
    }

    fn centre(&self) -> Vec2 {
        [self.width / 2.0, self.height / 2.0]
    }

    fn to_norm(&self, p: Vec2) -> Vec2 {
        let det = match self.unit_x[0] * self.unit_y[1] - self.unit_y[0] * self.unit_x[1] {
            0.0 => 1.0,
            d => d,
        };
        let (dx, dy) = (p[0] - self.origin[0], p[1] - self.origin[1]);
        [(self.unit_y[1] * dx - self.unit_y[0] * dy) / det, (self.unit_x[0] * dy - self.unit_x[1] * dx) / det]
    }

    fn to_frame(&self, n: Vec2) -> Vec2 {
        [self.origin[0] + self.unit_x[0] * n[0] + self.unit_y[0] * n[1], self.origin[1] + self.unit_x[1] * n[0] + self.unit_y[1] * n[1]]
    }

    /// A point in frame pixels, as a normalised point of the straightened image.
    pub fn to_straight(&self, frame: Vec2, angle: f64) -> Vec2 {
        let n = self.to_norm(frame);
        let r = rotate_about([n[0] * self.width, n[1] * self.height], angle, self.centre());
        [r[0] / self.width, r[1] / self.height]
    }

    fn unrotate(&self, p: Vec2, angle: f64) -> Vec2 {
        let r = rotate_about([p[0] * self.width, p[1] * self.height], -angle, self.centre());
        [r[0] / self.width, r[1] / self.height]
    }

    /// The crop rectangle's corners (top-left, top-right, bottom-right, bottom-left) in frame pixels.
    pub fn corners(&self, crop: &Crop) -> [Vec2; 4] {
        let r = crop.rect;
        [[r.x0, r.y0], [r.x1, r.y0], [r.x1, r.y1], [r.x0, r.y1]].map(|c| self.to_frame(self.unrotate(c, crop.angle)))
    }
}

pub fn format_angle(angle: f64) -> String {
    let rounded = (angle * 100.0).round() / 100.0;
    let sign = if rounded > 0.0 { "+" } else if rounded < 0.0 { "-" } else { "" };
    format!("{sign}{:.2}", rounded.abs())
}
