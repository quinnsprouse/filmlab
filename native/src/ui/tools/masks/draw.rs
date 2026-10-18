use gpui_kit::*;

use super::model::Pt;
use super::scene::{LinearDraw, RadialDraw, Scene};
use crate::ui::theme;

const REACH: f32 = 4000.0;

fn halo() -> Hsla {
    theme::backdrop().opacity(0.7)
}

fn line_color() -> Hsla {
    theme::focus()
}

struct Brush {
    painting: bool,
    erasing: bool,
    radius: f32,
    feather: f64,
    cursor: Option<Pt>,
    stroke: Vec<Pt>,
}

/// Everything the masks overlay paints, in overlay-local points.
pub struct Layers {
    pub scene: Scene,
    brush: Brush,
}

impl Layers {
    pub fn new(scene: Scene, painting: bool, erasing: bool, radius: f32, feather: f64, cursor: Option<Pt>, stroke: Vec<Pt>) -> Layers {
        Layers { scene, brush: Brush { painting, erasing, radius, feather, cursor, stroke } }
    }

    pub fn paint(&self, bounds: Bounds<Pixels>, window: &mut Window) {
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            let origin = bounds.origin;
            for linear in &self.scene.linears {
                paint_linear(window, origin, linear);
            }
            for radial in &self.scene.radials {
                paint_radial(window, origin, radial);
            }
            for pin in &self.scene.pins {
                let fill = if pin.selected { line_color() } else { line_color().opacity(0.5) };
                paint_dot(window, at(origin, pin.at), 6.0, fill, theme::backdrop());
            }
            if self.brush.painting {
                paint_brush(window, origin, &self.brush);
            }
        });
    }
}

fn at(origin: Point<Pixels>, p: Pt) -> Point<Pixels> {
    point(origin.x + px(p.x as f32), origin.y + px(p.y as f32))
}

fn stroke_path(window: &mut Window, points: &[Point<Pixels>], closed: bool, width: f32, color: Hsla, dash: Option<&[Pixels]>) {
    let Some((first, rest)) = points.split_first() else { return };
    let mut builder = PathBuilder::stroke(px(width));
    if let Some(dash) = dash {
        builder = builder.dash_array(dash);
    }
    builder.move_to(*first);
    for p in rest {
        builder.line_to(*p);
    }
    if closed {
        builder.close();
    }
    if let Ok(path) = builder.build() {
        window.paint_path(path, color);
    }
}

fn haloed(window: &mut Window, points: &[Point<Pixels>], closed: bool, width: f32, dash: Option<&[Pixels]>) {
    stroke_path(window, points, closed, width + 2.0, halo(), None);
    stroke_path(window, points, closed, width, line_color(), dash);
}

fn paint_dot(window: &mut Window, center: Point<Pixels>, radius: f32, fill: Hsla, border: Hsla) {
    let bounds = Bounds::centered_at(center, size(px(radius * 2.0), px(radius * 2.0)));
    window.paint_quad(quad(bounds, px(radius), fill, px(1.5), border, BorderStyle::Solid));
}

fn paint_handle(window: &mut Window, center: Point<Pixels>, radius: f32) {
    let outer = Bounds::centered_at(center, size(px((radius + 1.0) * 2.0), px((radius + 1.0) * 2.0)));
    window.paint_quad(quad(outer, px(radius + 1.0), gpui_kit::transparent_black(), px(3.0), halo(), BorderStyle::Solid));
    paint_dot(window, center, radius, theme::backdrop().opacity(0.4), line_color());
}

fn paint_radial(window: &mut Window, origin: Point<Pixels>, draw: &RadialDraw) {
    let ring: Vec<_> = draw.ring.iter().map(|p| at(origin, *p)).collect();
    let feather: Vec<_> = draw.feather_ring.iter().map(|p| at(origin, *p)).collect();
    haloed(window, &ring, true, 1.5, None);
    haloed(window, &feather, true, 1.0, Some(&[px(4.), px(4.)]));
    let edge = draw.handles.first().copied().unwrap_or(draw.rotate);
    haloed(window, &[at(origin, edge), at(origin, draw.rotate)], false, 1.0, None);
    for handle in &draw.handles {
        paint_handle(window, at(origin, *handle), 5.0);
    }
    paint_handle(window, at(origin, draw.rotate), 4.0);
}

fn paint_linear(window: &mut Window, origin: Point<Pixels>, draw: &LinearDraw) {
    let (dx, dy) = ((draw.end.x - draw.start.x) as f32, (draw.end.y - draw.start.y) as f32);
    let length = dx.hypot(dy);
    let length = if length == 0.0 { 1.0 } else { length };
    let (px_, py_) = (-dy / length * REACH, dx / length * REACH);
    let across = |p: Pt| {
        let c = at(origin, p);
        [point(c.x - px(px_), c.y - px(py_)), point(c.x + px(px_), c.y + px(py_))]
    };
    haloed(window, &across(draw.start), false, 1.0, None);
    haloed(window, &across(draw.middle), false, 1.0, Some(&[px(6.), px(6.)]));
    haloed(window, &across(draw.end), false, 1.0, None);
    paint_handle(window, at(origin, draw.start), 5.0);
    paint_handle(window, at(origin, draw.end), 5.0);
}

fn circle_points(center: Point<Pixels>, radius: f32) -> Vec<Point<Pixels>> {
    let steps = 48;
    (0..steps)
        .map(|i| {
            let turn = i as f32 / steps as f32 * std::f32::consts::TAU;
            point(center.x + px(radius * turn.cos()), center.y + px(radius * turn.sin()))
        })
        .collect()
}

fn paint_brush(window: &mut Window, origin: Point<Pixels>, brush: &Brush) {
    if !brush.stroke.is_empty() {
        let width = (brush.radius * 2.0).max(1.0);
        let color = if brush.erasing { line_color().opacity(0.2) } else { theme::danger().opacity(0.35) };
        let points: Vec<_> = brush.stroke.iter().map(|p| at(origin, *p)).collect();
        stroke_path(window, &points, false, width, color, None);
        for end in [points.first(), points.last()].into_iter().flatten() {
            let bounds = Bounds::centered_at(*end, size(px(width), px(width)));
            window.paint_quad(quad(bounds, px(width / 2.0), color, px(0.), color, BorderStyle::Solid));
        }
    }
    let Some(cursor) = brush.cursor else { return };
    let center = at(origin, cursor);
    let outer = circle_points(center, brush.radius);
    stroke_path(window, &outer, true, 3.0, halo(), None);
    let dash = [px(3.), px(3.)];
    stroke_path(window, &outer, true, 1.0, line_color(), brush.erasing.then_some(&dash[..]));
    let inner = brush.radius * (1.0 - brush.feather as f32 / 100.0).max(0.05);
    stroke_path(window, &circle_points(center, inner), true, 1.0, line_color().opacity(0.6), None);
}
