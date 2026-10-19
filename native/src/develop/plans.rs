use serde_json::{Value, json};

use crate::camera::{Camera, Native, Region, Size, WHOLE, Zoom, camera_of, fit_scale};
use crate::film::FilmSettings;

const DETAIL_MARGIN: f64 = 64.0;
pub const PREVIEW_EDGE: f64 = 2560.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Level {
    Preview,
    Full,
}

impl Level {
    pub fn max_edge(self) -> usize {
        match self {
            Level::Preview => PREVIEW_EDGE as usize,
            Level::Full => 0,
        }
    }

    pub fn tag(self) -> &'static str {
        match self {
            Level::Preview => "preview",
            Level::Full => "full",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layer {
    Fit,
    Detail,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Inputs {
    pub photo_id: String,
    pub revision: u64,
    pub settings: Value,
    pub film: Option<FilmSettings>,
    pub overlay: Option<Value>,
    pub apply_crop: bool,
    pub native: Native,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub layer: Layer,
    pub photo_id: String,
    pub revision: u64,
    pub look: String,
    pub level: Level,
    pub settings: Value,
    pub film: Option<FilmSettings>,
    pub overlay: Option<Value>,
    pub view: Value,
    pub region: Region,
}

impl Inputs {
    fn look(&self) -> String {
        json!([self.film.as_ref().map(FilmSettings::to_value), self.overlay, self.apply_crop]).to_string()
    }

    fn plan(&self, layer: Layer, level: Level, view: Value, region: Region) -> Plan {
        Plan {
            layer,
            photo_id: self.photo_id.clone(),
            revision: self.revision,
            look: self.look(),
            level,
            settings: self.settings.clone(),
            film: self.film.clone(),
            overlay: self.overlay.clone(),
            view,
            region,
        }
    }
}

fn level_for(shown_long: f64) -> Level {
    if shown_long > PREVIEW_EDGE { Level::Full } else { Level::Preview }
}

pub fn fit_plan(inputs: &Inputs, viewport: Size) -> Plan {
    let fit = fit_scale(inputs.native, viewport);
    let view = json!({
        "maxW": viewport.width.round().max(1.0) as usize,
        "maxH": viewport.height.round().max(1.0) as usize,
        "stages": "loupe",
        "applyCrop": inputs.apply_crop,
    });
    inputs.plan(Layer::Fit, level_for(inputs.native.0.max(inputs.native.1) * fit), view, WHOLE)
}

pub fn visible_region(camera: Camera, native: Native, viewport: Size) -> Region {
    let half_w = viewport.width / (2.0 * native.0 * camera.scale);
    let half_h = viewport.height / (2.0 * native.1 * camera.scale);
    Region { u0: (camera.cx - half_w).max(0.0), v0: (camera.cy - half_h).max(0.0), u1: (camera.cx + half_w).min(1.0), v1: (camera.cy + half_h).min(1.0) }
}

fn covers(outer: Region, inner: Region) -> bool {
    let slack = 1e-6;
    outer.u0 <= inner.u0 + slack && outer.v0 <= inner.v0 + slack && outer.u1 >= inner.u1 - slack && outer.v1 >= inner.v1 - slack
}

fn detail_plan(inputs: &Inputs, camera: Camera, viewport: Size) -> Plan {
    let scale = camera.scale.min(1.0);
    let full_w = (inputs.native.0 * scale).round().max(1.0);
    let full_h = (inputs.native.1 * scale).round().max(1.0);
    let width = ((viewport.width * scale / camera.scale).ceil() + 2.0 * DETAIL_MARGIN).min(full_w);
    let height = ((viewport.height * scale / camera.scale).ceil() + 2.0 * DETAIL_MARGIN).min(full_h);
    let x = (camera.cx * full_w - width / 2.0).max(0.0).min(full_w - width).round();
    let y = (camera.cy * full_h - height / 2.0).max(0.0).min(full_h - height).round();
    let view = json!({
        "maxW": full_w as usize,
        "maxH": full_h as usize,
        "stages": "loupe-zoom",
        "applyCrop": inputs.apply_crop,
        "window": [x as usize, y as usize, width as usize, height as usize],
    });
    let region = Region { u0: x / full_w, v0: y / full_h, u1: (x + width) / full_w, v1: (y + height) / full_h };
    inputs.plan(Layer::Detail, level_for(full_w.max(full_h)), view, region)
}

/// The fit plan drawn while a slider moves: half the size, a quarter of the pixels, with its own stage cache so the
/// full-size one is still warm when the slider stops.
pub fn draft_plan(fit: &Plan) -> Plan {
    let mut view = fit.view.clone();
    for k in ["maxW", "maxH"] {
        view[k] = json!((view[k].as_f64().unwrap_or(1.0) / 2.0).round().max(1.0) as usize);
    }
    view["stages"] = json!("draft");
    Plan { view, ..fit.clone() }
}

pub fn is_draft(plan: &Plan) -> bool {
    plan.view["stages"] == "draft"
}

pub fn same_look(a: &Plan, b: &Plan) -> bool {
    a.photo_id == b.photo_id && a.revision == b.revision && a.look == b.look
}

pub fn same_plan(a: Option<&Plan>, b: &Plan) -> bool {
    a.is_some_and(|a| same_look(a, b) && a.level == b.level && a.view == b.view)
}

fn still_covers(previous: Option<&Plan>, wanted: &Plan, camera: Camera, native: Native, viewport: Size) -> bool {
    previous.is_some_and(|p| same_look(p, wanted) && p.view["maxW"] == wanted.view["maxW"] && covers(p.region, visible_region(camera, native, viewport)))
}

pub fn plans(inputs: &Inputs, zoom: Zoom, viewport: Size, shown_detail: Option<&Plan>) -> Vec<Plan> {
    if viewport.width < 1.0 || viewport.height < 1.0 {
        return Vec::new();
    }
    let fit = fit_plan(inputs, viewport);
    let camera = camera_of(zoom, inputs.native, viewport);
    if camera.scale <= fit_scale(inputs.native, viewport) * 1.001 {
        return vec![fit];
    }
    let detail = detail_plan(inputs, camera, viewport);
    let first = match shown_detail {
        Some(shown) if still_covers(Some(shown), &detail, camera, inputs.native, viewport) => shown.clone(),
        _ => detail,
    };
    vec![first, fit]
}
