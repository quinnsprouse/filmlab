use filmlab_engine::core;
use serde_json::{Value, json};

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

fn num(value: &Value, fallback: f64) -> f64 {
    value.as_f64().filter(|v| v.is_finite()).unwrap_or(fallback)
}

fn pair(value: &Value) -> Option<Vec2> {
    let list = value.as_array().filter(|l| l.len() == 2)?;
    let (w, h) = (list[0].as_f64()?, list[1].as_f64()?);
    (w > 0.0 && h > 0.0).then_some([w, h])
}

pub fn read_crop(settings: &Value) -> Crop {
    let crop = &settings["crop"];
    let geometry = &crop["geometry"];
    let rect = &geometry["rect"];
    Crop {
        rect: Rect { x0: num(&rect["x0"], FULL.x0), y0: num(&rect["y0"], FULL.y0), x1: num(&rect["x1"], FULL.x1), y1: num(&rect["y1"], FULL.y1) },
        angle: num(&geometry["angle"], 0.0),
        aspect: pair(&crop["aspect"]),
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
    pub fn new(facts: &Value, settings: &Value, size: (u32, u32)) -> Option<FrameMap> {
        let request = json!({"width": size.0, "height": size.1, "applyCrop": false, "points": [[0, 0], [1, 0], [0, 1]], "toImage": false});
        let out = core::map_points(&facts.to_string(), &settings.to_string(), &request.to_string()).ok()?;
        let points: Vec<Vec2> = serde_json::from_str(&out).ok()?;
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
