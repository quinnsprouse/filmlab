use super::model::{Pt, Radial, Shape};

const ROTATE_HANDLE: usize = 5;
const MIN_RADIUS: f64 = 0.01;
const SNAP_RATIO: f64 = 0.05;

fn long_scale(frame: (f64, f64)) -> Pt {
    let long = frame.0.max(frame.1);
    Pt::new(long / frame.0, long / frame.1)
}

fn local_to_norm(shape: &Radial, frame: (f64, f64), x: f64, y: f64) -> Pt {
    let scale = long_scale(frame);
    let (sin, cos) = shape.angle.to_radians().sin_cos();
    Pt::new(shape.center.x + (x * cos - y * sin) * scale.x, shape.center.y + (x * sin + y * cos) * scale.y)
}

pub fn radial_ring(shape: &Radial, frame: (f64, f64), ratio: f64, steps: usize) -> Vec<Pt> {
    (0..steps)
        .map(|index| {
            let turn = index as f64 / steps as f64 * std::f64::consts::TAU;
            local_to_norm(shape, frame, shape.rx * ratio * turn.cos(), shape.ry * ratio * turn.sin())
        })
        .collect()
}

pub fn radial_edge_handles(shape: &Radial, frame: (f64, f64)) -> Vec<Pt> {
    vec![
        local_to_norm(shape, frame, shape.rx, 0.0),
        local_to_norm(shape, frame, 0.0, shape.ry),
        local_to_norm(shape, frame, -shape.rx, 0.0),
        local_to_norm(shape, frame, 0.0, -shape.ry),
    ]
}

pub fn radial_rotate_handle(shape: &Radial, frame: (f64, f64)) -> Pt {
    local_to_norm(shape, frame, shape.rx + 0.08, 0.0)
}

fn to_local(shape: &Radial, frame: (f64, f64), at: Pt) -> Pt {
    let scale = long_scale(frame);
    let dx = (at.x - shape.center.x) / scale.x;
    let dy = (at.y - shape.center.y) / scale.y;
    let (sin, cos) = shape.angle.to_radians().sin_cos();
    Pt::new(dx * cos + dy * sin, -dx * sin + dy * cos)
}

pub fn radial_contains(shape: &Radial, frame: (f64, f64), at: Pt) -> bool {
    let local = to_local(shape, frame, at);
    (local.x / shape.rx).powi(2) + (local.y / shape.ry).powi(2) <= 1.0
}

fn snap_radii(rx: f64, ry: f64, keep_rx: bool) -> (f64, f64) {
    if (rx - ry).abs() / rx.max(ry) < SNAP_RATIO {
        return if keep_rx { (rx, rx) } else { (ry, ry) };
    }
    (rx, ry)
}

fn resize_radial(shape: &Radial, frame: (f64, f64), handle: usize, at: Pt, proportional: bool) -> Radial {
    let local = to_local(shape, frame, at);
    if handle == 1 || handle == 3 {
        let rx = local.x.abs().max(MIN_RADIUS);
        let ry = if proportional { shape.ry * (rx / shape.rx.max(MIN_RADIUS)) } else { shape.ry };
        let (rx, ry) = snap_radii(rx, ry, true);
        return Radial { rx, ry, ..shape.clone() };
    }
    let ry = local.y.abs().max(MIN_RADIUS);
    let rx = if proportional { shape.rx * (ry / shape.ry.max(MIN_RADIUS)) } else { shape.rx };
    let (rx, ry) = snap_radii(rx, ry, false);
    Radial { rx, ry, ..shape.clone() }
}

fn shift(point: Pt, delta: Pt) -> Pt {
    Pt::new(point.x + delta.x, point.y + delta.y)
}

pub struct DragMove {
    pub handle: usize,
    pub at: Pt,
    pub delta: Pt,
    pub proportional: bool,
}

pub fn moved_shape(shape: &Shape, frame: (f64, f64), mv: &DragMove) -> Shape {
    match shape {
        Shape::Radial(r) if (1..=4).contains(&mv.handle) => Shape::Radial(resize_radial(r, frame, mv.handle, mv.at, mv.proportional)),
        Shape::Radial(r) if mv.handle == ROTATE_HANDLE => {
            let scale = long_scale(frame);
            let dx = (mv.at.x - r.center.x) / scale.x;
            let dy = (mv.at.y - r.center.y) / scale.y;
            Shape::Radial(Radial { angle: dy.atan2(dx).to_degrees(), ..r.clone() })
        }
        Shape::Radial(r) => Shape::Radial(Radial { center: shift(r.center, mv.delta), ..r.clone() }),
        Shape::Linear { end, .. } if mv.handle == 1 => Shape::Linear { start: mv.at, end: *end },
        Shape::Linear { start, end } if mv.handle == 2 => Shape::Linear { start: *start, end: mv.at },
        Shape::Linear { start, end } => Shape::Linear { start: shift(*start, mv.delta), end: shift(*end, mv.delta) },
        other => other.clone(),
    }
}

pub fn pin_point(shape: &Shape) -> Option<Pt> {
    match shape {
        Shape::Radial(r) => Some(r.center),
        Shape::Linear { start, end } => Some(Pt::new((start.x + end.x) / 2.0, (start.y + end.y) / 2.0)),
        _ => None,
    }
}

pub fn offset(from: Pt, to: Pt) -> Pt {
    Pt::new(to.x - from.x, to.y - from.y)
}
