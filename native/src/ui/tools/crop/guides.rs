use gpui_kit::*;

use super::geometry::Vec2;
use crate::ui::theme;

#[derive(Clone, Copy, PartialEq)]
pub enum Guide {
    None,
    Thirds,
    Grid,
}

fn white(a: f32) -> Hsla {
    Hsla { h: 0.0, s: 0.0, l: 1.0, a }
}

fn point(origin: Point<Pixels>, v: Vec2) -> Point<Pixels> {
    gpui_kit::point(origin.x + px(v[0] as f32), origin.y + px(v[1] as f32))
}

fn stroke(window: &mut Window, points: &[Point<Pixels>], closed: bool, width: f32, color: Hsla) {
    let mut path = PathBuilder::stroke(px(width));
    path.add_polygon(points, closed);
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}

fn lerp(a: Vec2, b: Vec2, t: f64) -> Vec2 {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}

fn guide_lines(corners: &[Vec2; 4], guide: Guide) -> Vec<[Vec2; 2]> {
    let steps: Vec<f64> = match guide {
        Guide::None => return Vec::new(),
        Guide::Thirds => vec![1.0 / 3.0, 2.0 / 3.0],
        Guide::Grid => (1..8).map(|i| f64::from(i) / 8.0).collect(),
    };
    let [a, b, c, d] = *corners;
    steps.into_iter().flat_map(|t| [[lerp(a, b, t), lerp(d, c, t)], [lerp(a, d, t), lerp(b, c, t)]]).collect()
}

fn toward(corner: Vec2, target: Vec2, length: f64) -> Vec2 {
    let (dx, dy) = (target[0] - corner[0], target[1] - corner[1]);
    let size = dx.hypot(dy);
    let size = if size == 0.0 { 1.0 } else { size };
    let reach = length.min(size / 3.0);
    [corner[0] + dx / size * reach, corner[1] + dy / size * reach]
}

fn edge_bar(centre: Vec2, along: Vec2, length: f64) -> [Vec2; 2] {
    let size = along[0].hypot(along[1]);
    let size = if size == 0.0 { 1.0 } else { size };
    let (dx, dy) = (along[0] / size * length / 2.0, along[1] / size * length / 2.0);
    [[centre[0] - dx, centre[1] - dy], [centre[0] + dx, centre[1] + dy]]
}

fn marks(corners: &[Vec2; 4], handles: &[Vec2]) -> Vec<Vec<Vec2>> {
    let brackets = (0..4).map(|i| vec![toward(corners[i], corners[(i + 1) % 4], 18.0), corners[i], toward(corners[i], corners[(i + 3) % 4], 18.0)]);
    let bars = (0..4).map(|i| {
        let (from, to) = (corners[i], corners[(i + 1) % 4]);
        edge_bar(handles[i + 4], [to[0] - from[0], to[1] - from[1]], 22.0).to_vec()
    });
    brackets.chain(bars).collect()
}

/// Paints the dimmed outside, the crop outline, the optional guide lines and the handle marks. Points are in
/// the overlay's own coordinates; `origin` and `size` place and bound it in the window.
pub fn paint(window: &mut Window, origin: Point<Pixels>, size: Size<Pixels>, corners: &[Vec2; 4], handles: &[Vec2], guide: Guide) {
    let outer = [origin, gpui_kit::point(origin.x + size.width, origin.y), gpui_kit::point(origin.x + size.width, origin.y + size.height), gpui_kit::point(origin.x, origin.y + size.height)];
    let quad = corners.map(|c| point(origin, c));
    let mut scrim = PathBuilder::fill();
    scrim.add_polygon(&outer, true);
    scrim.add_polygon(&quad, true);
    if let Ok(path) = scrim.build() {
        window.paint_path(path, Hsla { a: 0.7, ..theme::backdrop() });
    }
    stroke(window, &quad, true, 1.0, white(0.9));
    for line in guide_lines(corners, guide) {
        stroke(window, &line.map(|p| point(origin, p)), false, 1.0, white(0.5));
    }
    let marks = marks(corners, handles);
    for (width, color) in [(5.0, Hsla { a: 0.6, ..theme::backdrop() }), (3.0, white(1.0))] {
        for mark in &marks {
            let points: Vec<_> = mark.iter().map(|p| point(origin, *p)).collect();
            stroke(window, &points, false, width, color);
        }
    }
}
