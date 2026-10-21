use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::*;

use super::geometry::{SpotShape, distance};
use super::spots::V;
use crate::ui::theme;

/// The strokes each spot draws (its brushed path, its source's, the dashed link between them), built once for the
/// frame's origin: building a stroke is most of what drawing a spot costs.
pub type Strokes = Rc<RefCell<Option<(Point<Pixels>, Vec<SpotStrokes>)>>>;

pub struct SpotStrokes {
    target: Option<Path<Pixels>>,
    source: Option<Path<Pixels>>,
    link: Option<Path<Pixels>>,
}

pub struct Scene {
    pub shapes: Rc<[SpotShape]>,
    pub strokes: Strokes,
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

fn stroke(origin: Point<Pixels>, points: &[V], width: f32, dash: Option<&[Pixels]>) -> Option<Path<Pixels>> {
    let (first, rest) = points.split_first()?;
    let mut builder = PathBuilder::stroke(px(width));
    if let Some(dash) = dash {
        builder = builder.dash_array(dash);
    }
    builder.move_to(at(origin, *first));
    for p in rest {
        builder.line_to(at(origin, *p));
    }
    builder.build().ok()
}

fn polyline(window: &mut Window, origin: Point<Pixels>, points: &[V], width: f32, color: Hsla) {
    if let Some(path) = stroke(origin, points, width, None) {
        window.paint_path(path, color);
    }
}

fn brushed(origin: Point<Pixels>, points: &[V], radius: f64) -> Option<Path<Pixels>> {
    (points.len() > 1).then(|| stroke(origin, points, (radius * 2.0) as f32, None)).flatten()
}

fn strokes_of(origin: Point<Pixels>, shape: &SpotShape) -> SpotStrokes {
    let link = shape.points.first().zip(shape.source_points.first()).and_then(|(&target, &source)| link(origin, shape.radius, target, source));
    SpotStrokes { target: brushed(origin, &shape.points, shape.radius), source: brushed(origin, &shape.source_points, shape.radius), link }
}

impl Scene {
    pub fn paint(&self, origin: Point<Pixels>, window: &mut Window) {
        let mut built = self.strokes.borrow_mut();
        if built.as_ref().is_none_or(|(o, _)| *o != origin) {
            *built = Some((origin, self.shapes.iter().map(|s| strokes_of(origin, s)).collect()));
        }
        if let Some((_, strokes)) = built.as_ref() {
            for (shape, strokes) in self.shapes.iter().zip(strokes) {
                self.spot(window, origin, shape, strokes, Some(shape.index) == self.selected);
            }
        }
        if !self.paint.is_empty() {
            polyline(window, origin, &self.paint, (self.radius * 2.0) as f32, white(0.3));
            if let [only] = self.paint[..] {
                circle(window, origin, only, self.radius, Some(white(0.3)), None);
            }
        }
        if let Some(c) = self.cursor {
            circle(window, origin, c, self.radius, None, Some((1.0, white(1.0))));
            circle(window, origin, c, self.radius * (1.0 - self.feather / 100.0).max(0.05), None, Some((1.0, white(0.5))));
        }
    }

    fn spot(&self, window: &mut Window, origin: Point<Pixels>, shape: &SpotShape, strokes: &SpotStrokes, selected: bool) {
        outline(window, origin, &shape.points, strokes.target.as_ref(), shape.radius, if selected { 1.0 } else { 0.6 });
        let (Some(&target), Some(&source)) = (shape.points.first(), shape.source_points.first()) else {
            if let Some(&target) = shape.points.first() {
                pin(window, origin, target, selected);
            }
            return;
        };
        outline(window, origin, &shape.source_points, strokes.source.as_ref(), shape.radius, if selected { 0.8 } else { 0.35 });
        if let Some(link) = &strokes.link {
            window.paint_path(link.clone(), white(0.7));
        }
        circle(window, origin, source, 3.0, Some(white(if selected { 1.0 } else { 0.5 })), None);
        pin(window, origin, target, selected);
    }
}

fn outline(window: &mut Window, origin: Point<Pixels>, points: &[V], brushed: Option<&Path<Pixels>>, radius: f64, alpha: f32) {
    let (Some(&first), Some(&last)) = (points.first(), points.last()) else { return };
    if let Some(path) = brushed {
        window.paint_path(path.clone(), white(0.15));
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

fn link(origin: Point<Pixels>, radius: f64, target: V, source: V) -> Option<Path<Pixels>> {
    let gap = distance(target, source);
    if gap < 1.0 || gap <= radius * 2.0 {
        return None;
    }
    let unit = ((target.0 - source.0) / gap, (target.1 - source.1) / gap);
    let from = (source.0 + unit.0 * radius, source.1 + unit.1 * radius);
    let to = (target.0 - unit.0 * radius, target.1 - unit.1 * radius);
    stroke(origin, &[from, to], 1.0, Some(&[px(4.0), px(3.0)]))
}
