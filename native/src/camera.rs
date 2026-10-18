const MAX_SCALE: f64 = 4.0;

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Size {
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub scale: f64,
    pub cx: f64,
    pub cy: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Region {
    pub u0: f64,
    pub v0: f64,
    pub u1: f64,
    pub v1: f64,
}

pub const WHOLE: Region = Region { u0: 0.0, v0: 0.0, u1: 1.0, v1: 1.0 };

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Zoom {
    Fit,
    At { scale: f64, cx: f64, cy: f64 },
}

pub type Native = (f64, f64);

pub fn fit_scale(native: Native, viewport: Size) -> f64 {
    (viewport.width / native.0).min(viewport.height / native.1).min(1.0)
}

fn clamp_axis(centre: f64, shown: f64, extent: f64) -> f64 {
    let half = extent / (2.0 * shown);
    if half >= 0.5 { 0.5 } else { centre.max(half).min(1.0 - half) }
}

fn clamp(camera: Camera, native: Native, viewport: Size) -> Camera {
    let scale = camera.scale.max(fit_scale(native, viewport)).min(MAX_SCALE);
    Camera { scale, cx: clamp_axis(camera.cx, native.0 * scale, viewport.width), cy: clamp_axis(camera.cy, native.1 * scale, viewport.height) }
}

pub fn camera_of(zoom: Zoom, native: Native, viewport: Size) -> Camera {
    match zoom {
        Zoom::Fit => Camera { scale: fit_scale(native, viewport), cx: 0.5, cy: 0.5 },
        Zoom::At { scale, cx, cy } => clamp(Camera { scale, cx, cy }, native, viewport),
    }
}

pub fn zoom_of(camera: Camera, native: Native, viewport: Size) -> Zoom {
    if camera.scale <= fit_scale(native, viewport) * 1.001 { Zoom::Fit } else { Zoom::At { scale: camera.scale, cx: camera.cx, cy: camera.cy } }
}

pub fn rect_of(camera: Camera, region: Region, native: Native, viewport: Size) -> Rect {
    let (w, h) = native;
    Rect {
        x: viewport.width / 2.0 + (region.u0 - camera.cx) * w * camera.scale,
        y: viewport.height / 2.0 + (region.v0 - camera.cy) * h * camera.scale,
        width: (region.u1 - region.u0) * w * camera.scale,
        height: (region.v1 - region.v0) * h * camera.scale,
    }
}

pub fn point_at(camera: Camera, x: f64, y: f64, native: Native, viewport: Size) -> (f64, f64) {
    (camera.cx + (x - viewport.width / 2.0) / (native.0 * camera.scale), camera.cy + (y - viewport.height / 2.0) / (native.1 * camera.scale))
}

pub fn zoom_around(camera: Camera, scale: f64, x: f64, y: f64, native: Native, viewport: Size) -> Camera {
    let (u, v) = point_at(camera, x, y, native, viewport);
    let next = scale.max(fit_scale(native, viewport)).min(MAX_SCALE);
    clamp(
        Camera { scale: next, cx: u - (x - viewport.width / 2.0) / (native.0 * next), cy: v - (y - viewport.height / 2.0) / (native.1 * next) },
        native,
        viewport,
    )
}

pub fn pan_by(camera: Camera, dx: f64, dy: f64, native: Native, viewport: Size) -> Camera {
    clamp(Camera { scale: camera.scale, cx: camera.cx - dx / (native.0 * camera.scale), cy: camera.cy - dy / (native.1 * camera.scale) }, native, viewport)
}

pub fn mix_log_scale(from: Camera, to: Camera, t: f64) -> Camera {
    let scale = (from.scale.ln() + (to.scale.ln() - from.scale.ln()) * t).exp();
    Camera { scale, cx: from.cx + (to.cx - from.cx) * t, cy: from.cy + (to.cy - from.cy) * t }
}
