use gpui_kit::*;

use super::geometry::{SpotShape, distance};
use super::spots::V;
use crate::ui::theme;

pub struct Scene {
    pub shapes: Vec<SpotShape>,
    pub selected: Option<usize>,
    pub paint: Vec<V>,
    pub cursor: Option<V>,
    pub radius: f64,
    pub feather: f64,
}

fn white(alpha: f32) -> Hsla {
    Hsla { h: 0.0, s: 0.0, l: 1.0, a: alpha }
}

fn at(origin: Point<Pixels>, p: V) -> Point<Pixels> {
    point(origin.x + px(p.0 as f32), origin.y + px(p.1 as f32))
}

fn circle(window: &mut Window, origin: Point<Pixels>, c: V, r: f64, fill: Option<Hsla>, stroke: Option<(f32, Hsla)>) {
    let r = r as f32;
    let bounds = Bounds::new(at(origin, (c.0 - f64::from(r), c.1 - f64::from(r))), size(px(r * 2.0), px(r * 2.0)));
    let (width, color) = stroke.unwrap_or((0.0, white(0.0)));
    let background = fill.unwrap_or(white(0.0));
    window.paint_quad(quad(bounds, Corners::all(px(r)), background, Edges::all(px(width)), color, BorderStyle::Solid));
}

fn polyline(window: &mut Window, origin: Point<Pixels>, points: &[V], width: f32, color: Hsla, dash: Option<&[Pixels]>) {
    let Some((first, rest)) = points.split_first() else { return };
    let mut builder = PathBuilder::stroke(px(width));
    if let Some(dash) = dash {
        builder = builder.dash_array(dash);
    }
    builder.move_to(at(origin, *first));
    for p in rest {
        builder.line_to(at(origin, *p));
    }
    if let Ok(path) = builder.build() {
        window.paint_path(path, color);
    }
}

impl Scene {
    pub fn paint(&self, origin: Point<Pixels>, window: &mut Window) {
        for shape in &self.shapes {
            self.spot(window, origin, shape, Some(shape.index) == self.selected);
        }
        if !self.paint.is_empty() {
            polyline(window, origin, &self.paint, (self.radius * 2.0) as f32, white(0.3), None);
            if let [only] = self.paint[..] {
                circle(window, origin, only, self.radius, Some(white(0.3)), None);
            }
        }
        if let Some(c) = self.cursor {
            circle(window, origin, c, self.radius, None, Some((1.0, white(1.0))));
            circle(window, origin, c, self.radius * (1.0 - self.feather / 100.0).max(0.05), None, Some((1.0, white(0.5))));
        }
    }

    fn spot(&self, window: &mut Window, origin: Point<Pixels>, shape: &SpotShape, selected: bool) {
        outline(window, origin, &shape.points, shape.radius, if selected { 1.0 } else { 0.6 });
        let (Some(&target), Some(&source)) = (shape.points.first(), shape.source_points.first()) else {
            if let Some(&target) = shape.points.first() {
                pin(window, origin, target, selected);
            }
            return;
        };
        outline(window, origin, &shape.source_points, shape.radius, if selected { 0.8 } else { 0.35 });
        link(window, origin, shape.radius, target, source);
        circle(window, origin, source, 3.0, Some(white(if selected { 1.0 } else { 0.5 })), None);
        pin(window, origin, target, selected);
    }
}

fn outline(window: &mut Window, origin: Point<Pixels>, points: &[V], radius: f64, alpha: f32) {
    let (Some(&first), Some(&last)) = (points.first(), points.last()) else { return };
    if points.len() > 1 {
        polyline(window, origin, points, (radius * 2.0) as f32, white(0.15), None);
    }
    circle(window, origin, first, radius, None, Some((1.25, white(alpha))));
    if points.len() > 1 {
        circle(window, origin, last, radius, None, Some((1.25, white(alpha))));
    }
}

fn pin(window: &mut Window, origin: Point<Pixels>, c: V, selected: bool) {
    let mut backdrop = theme::backdrop();
    backdrop.a = 0.6;
    circle(window, origin, c, 5.0, Some(backdrop), Some((1.5, white(1.0))));
    circle(window, origin, c, 2.0, Some(white(if selected { 1.0 } else { 0.6 })), None);
}

fn link(window: &mut Window, origin: Point<Pixels>, radius: f64, target: V, source: V) {
    let gap = distance(target, source);
    if gap < 1.0 || gap <= radius * 2.0 {
        return;
    }
    let unit = ((target.0 - source.0) / gap, (target.1 - source.1) / gap);
    let from = (source.0 + unit.0 * radius, source.1 + unit.1 * radius);
    let to = (target.0 - unit.0 * radius, target.1 - unit.1 * radius);
    polyline(window, origin, &[from, to], 1.0, white(0.7), Some(&[px(4.0), px(3.0)]));
}
