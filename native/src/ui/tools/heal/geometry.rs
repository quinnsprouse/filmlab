use std::sync::Arc;

use filmlab_engine::core::{self, Facts, MapRequest};

use super::spots::{Spot, V};
use crate::settings::Settings;

/// Maps between the overlay box (window points) and normalized image coordinates.
pub struct Mapping {
    pub facts: Arc<Facts>,
    pub settings: Settings,
    pub frame: (u32, u32),
    pub scale: f64,
}

/// A spot's radius in window points, for a frame of `frame` pixels drawn at `scale` pixels a point.
pub fn radius_of(size: f64, frame: (u32, u32), scale: f64) -> f64 {
    size * f64::from(frame.0.max(frame.1)) / scale
}

pub fn distance(a: V, b: V) -> f64 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

impl Mapping {
    fn map(&self, points: &[V], to_image: bool) -> Vec<V> {
        if points.is_empty() {
            return Vec::new();
        }
        let request = MapRequest { width: self.frame.0 as usize, height: self.frame.1 as usize, apply_crop: true, points: points.iter().map(|&(x, y)| [x, y]).collect(), to_image };
        core::mapped_points(&self.facts, &self.settings, &request).into_iter().map(|[x, y]| (x, y)).collect()
    }

    pub fn css_to_image(&self, points: &[V]) -> Vec<V> {
        let scaled: Vec<V> = points.iter().map(|&(x, y)| (x * self.scale, y * self.scale)).collect();
        self.map(&scaled, true)
    }

    fn image_to_css(&self, points: &[V]) -> Vec<V> {
        self.map(points, false).into_iter().map(|(x, y)| (x / self.scale, y / self.scale)).collect()
    }

    pub fn radius_of(&self, size: f64) -> f64 {
        radius_of(size, self.frame, self.scale)
    }

    pub fn shapes_of(&self, spots: &[Spot]) -> Vec<SpotShape> {
        let mut flat: Vec<V> = Vec::new();
        for spot in spots {
            flat.extend(&spot.points);
            if let Some((ox, oy)) = spot.source {
                flat.extend(spot.points.iter().map(|&(x, y)| (x + ox, y + oy)));
            }
        }
        let mapped = self.image_to_css(&flat);
        let mut cursor = 0;
        let mut take = |count: usize| {
            let end = (cursor + count).min(mapped.len());
            let slice = mapped[cursor.min(end)..end].to_vec();
            cursor += count;
            slice
        };
        spots
            .iter()
            .enumerate()
            .map(|(index, spot)| {
                let points = take(spot.points.len());
                let source_points = if spot.source.is_some() { take(spot.points.len()) } else { Vec::new() };
                SpotShape { index, radius: self.radius_of(spot.size), points, source_points }
            })
            .collect()
    }
}

pub struct SpotShape {
    pub index: usize,
    pub radius: f64,
    pub points: Vec<V>,
    pub source_points: Vec<V>,
}

#[derive(Clone, Copy)]
pub struct Grip {
    pub index: usize,
    pub source: bool,
    pub center: V,
    pub radius: f64,
}

pub fn grips_of(shapes: &[SpotShape]) -> Vec<Grip> {
    let grip = |index, source, center, radius| Grip { index, source, center, radius };
    shapes
        .iter()
        .flat_map(|s| {
            let target = s.points.first().map(|&c| grip(s.index, false, c, s.radius));
            let source = s.source_points.first().map(|&c| grip(s.index, true, c, s.radius));
            target.into_iter().chain(source)
        })
        .collect()
}

pub fn grip_at(grips: &[Grip], at: V, selected: Option<usize>) -> Option<Grip> {
    grips
        .iter()
        .filter(|g| distance(g.center, at) < g.radius.max(8.0))
        .min_by(|a, b| {
            let rank = |g: &Grip| u8::from(Some(g.index) != selected);
            rank(a).cmp(&rank(b)).then(distance(a.center, at).total_cmp(&distance(b.center, at)))
        })
        .copied()
}
