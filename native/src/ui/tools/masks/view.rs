use std::sync::Arc;

use filmlab_engine::core::{self, Facts, MapRequest};

use super::model::Pt;
use crate::settings::Settings;

/// Maps between normalized image points and the overlay box (window points), through the engine's own
/// geometry.
#[derive(Clone)]
pub struct ViewSpace {
    facts: Arc<Facts>,
    settings: Settings,
    pub width: f64,
    pub height: f64,
    scale: f64,
}

impl ViewSpace {
    pub fn new(facts: Arc<Facts>, settings: Settings, frame: (u32, u32), scale: f64) -> ViewSpace {
        ViewSpace { facts, settings, width: f64::from(frame.0), height: f64::from(frame.1), scale: scale.max(f64::EPSILON) }
    }

    pub fn long_edge(&self) -> f64 {
        self.width.max(self.height) / self.scale
    }

    fn map(&self, points: &[Pt], to_image: bool) -> Vec<Pt> {
        let request = MapRequest { width: self.width as u32 as usize, height: self.height as u32 as usize, apply_crop: true, points: points.iter().map(|p| [p.x, p.y]).collect(), to_image };
        core::mapped_points(&self.facts, &self.settings, &request).into_iter().map(|[x, y]| Pt::new(x, y)).collect()
    }

    pub fn to_view(&self, points: &[Pt]) -> Vec<Pt> {
        self.map(points, false).into_iter().map(|p| Pt::new(p.x / self.scale, p.y / self.scale)).collect()
    }

    pub fn to_image(&self, points: &[Pt]) -> Vec<Pt> {
        let scaled: Vec<Pt> = points.iter().map(|p| Pt::new(p.x * self.scale, p.y * self.scale)).collect();
        self.map(&scaled, true)
    }
}
