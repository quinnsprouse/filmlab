use gpui_kit::*;

use super::geometry::Vec2;
use crate::ui::theme;

#[derive(Clone, Copy, PartialEq, Default)]
pub enum Guide {
    #[default]
    None,
    Thirds,
    Level(f32),
}

const LEVEL_CELLS: f64 = 16.0;
const LEVEL_MIN_CELL: f64 = 24.0;

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

fn distance(a: Vec2, b: Vec2) -> f64 {
    (b[0] - a[0]).hypot(b[1] - a[1])
}

fn vertical(corners: &[Vec2; 4], t: f64) -> [Vec2; 2] {
    let [a, b, c, d] = *corners;
    [lerp(a, b, t), lerp(d, c, t)]
}

fn horizontal(corners: &[Vec2; 4], t: f64) -> [Vec2; 2] {
    let [a, b, c, d] = *corners;
    [lerp(a, d, t), lerp(b, c, t)]
}

fn thirds(corners: &[Vec2; 4]) -> Vec<[Vec2; 2]> {
    [1.0 / 3.0, 2.0 / 3.0].into_iter().flat_map(|t| [vertical(corners, t), horizontal(corners, t)]).collect()
}

fn from_centre(length: f64, cell: f64) -> impl Iterator<Item = (f64, bool)> {
    let reach = (length / 2.0 / cell).ceil() as i32;
    (-reach..=reach).map(move |k| (0.5 + f64::from(k) * cell / length, k == 0)).filter(|(t, _)| *t > 0.0 && *t < 1.0)
}

fn level_lines(corners: &[Vec2; 4]) -> Vec<([Vec2; 2], bool)> {
    let (width, height) = (distance(corners[0], corners[1]), distance(corners[0], corners[3]));
    let cell = (width.max(height) / LEVEL_CELLS).max(LEVEL_MIN_CELL);
    let verticals = from_centre(width, cell).map(|(t, centre)| (vertical(corners, t), centre));
    let horizontals = from_centre(height, cell).map(|(t, centre)| (horizontal(corners, t), centre));
    verticals.chain(horizontals).collect()
}

fn paint_level(window: &mut Window, origin: Point<Pixels>, corners: &[Vec2; 4], strength: f32) {
    let lines: Vec<_> = level_lines(corners).into_iter().map(|(line, centre)| (line.map(|p| point(origin, p)), centre)).collect();
    for (line, _) in &lines {
        stroke(window, line, false, 3.0, Hsla { a: 0.3 * strength, ..theme::backdrop() });
    }
    for (line, centre) in &lines {
        let alpha = if *centre { 0.75 } else { 0.38 };
        stroke(window, line, false, 1.0, white(alpha * strength));
    }
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
    match guide {
        Guide::None => {}
        Guide::Thirds => {
            for line in thirds(corners) {
                stroke(window, &line.map(|p| point(origin, p)), false, 1.0, white(0.5));
            }
        }
        Guide::Level(strength) => paint_level(window, origin, corners, strength),
    }
    stroke(window, &quad, true, 1.0, white(0.9));
    let marks = marks(corners, handles);
    for (width, color) in [(5.0, Hsla { a: 0.6, ..theme::backdrop() }), (3.0, white(1.0))] {
        for mark in &marks {
            let points: Vec<_> = mark.iter().map(|p| point(origin, *p)).collect();
            stroke(window, &points, false, width, color);
        }
    }
}
