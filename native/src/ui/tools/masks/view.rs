use filmlab_engine::core;
use serde_json::{Value, json};

use super::model::Pt;

/// Maps between normalized image points and the overlay box (window points), through the engine's own
/// geometry.
#[derive(Clone)]
pub struct ViewSpace {
    facts: String,
    settings: String,
    pub width: f64,
    pub height: f64,
    scale: f64,
}

impl ViewSpace {
    pub fn new(facts: &Value, settings: &Value, frame: (u32, u32), scale: f64) -> ViewSpace {
        ViewSpace { facts: facts.to_string(), settings: settings.to_string(), width: f64::from(frame.0), height: f64::from(frame.1), scale: scale.max(f64::EPSILON) }
    }

    pub fn long_edge(&self) -> f64 {
        self.width.max(self.height) / self.scale
    }

    fn map(&self, points: &[Pt], to_image: bool) -> Vec<Pt> {
        let request = json!({
            "width": self.width as u32,
            "height": self.height as u32,
            "applyCrop": true,
            "points": points.iter().map(|p| [p.x, p.y]).collect::<Vec<_>>(),
            "toImage": to_image,
        });
        let mapped = core::map_points(&self.facts, &self.settings, &request.to_string()).ok().and_then(|out| serde_json::from_str::<Vec<[f64; 2]>>(&out).ok());
        mapped.map(|list| list.into_iter().map(|[x, y]| Pt::new(x, y)).collect()).unwrap_or_default()
    }

    pub fn to_view(&self, points: &[Pt]) -> Vec<Pt> {
        self.map(points, false).into_iter().map(|p| Pt::new(p.x / self.scale, p.y / self.scale)).collect()
    }

    pub fn to_image(&self, points: &[Pt]) -> Vec<Pt> {
        let scaled: Vec<Pt> = points.iter().map(|p| Pt::new(p.x * self.scale, p.y * self.scale)).collect();
        self.map(&scaled, true)
    }
}
