use filmlab_engine::develop::SpotMode;
use serde_json::{Value, json};

use crate::settings::Settings;
use crate::ui::app::FilmlabApp;
use gpui_kit::Context;

pub type V = (f64, f64);

pub const SIZE_MIN: f64 = 0.001;
pub const SIZE_MAX: f64 = 0.25;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Remove,
    Heal,
    Clone,
    Fill,
}

impl Mode {
    pub const ALL: [Mode; 4] = [Mode::Remove, Mode::Heal, Mode::Clone, Mode::Fill];

    pub fn id(self) -> &'static str {
        match self {
            Mode::Remove => "remove",
            Mode::Heal => "heal",
            Mode::Clone => "clone",
            Mode::Fill => "fill",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Mode::Remove => "Remove",
            Mode::Heal => "Heal",
            Mode::Clone => "Clone",
            Mode::Fill => "Fill",
        }
    }

    fn of(mode: SpotMode) -> Mode {
        match mode {
            SpotMode::Remove => Mode::Remove,
            SpotMode::Heal => Mode::Heal,
            SpotMode::Clone => Mode::Clone,
            SpotMode::Fill => Mode::Fill,
        }
    }
}

#[derive(Clone)]
pub struct Spot {
    pub mode: Mode,
    pub points: Vec<V>,
    pub size: f64,
    pub feather: f64,
    pub opacity: f64,
    pub source: Option<V>,
}

fn spot_of(raw: &filmlab_engine::develop::Spot) -> Spot {
    Spot {
        mode: Mode::of(raw.mode),
        points: raw.points.iter().map(|p| (p.x, p.y)).collect(),
        size: raw.size,
        feather: raw.feather,
        opacity: raw.opacity,
        source: raw.source_offset.map(|p| (p.x, p.y)),
    }
}

pub fn spots_of(settings: Option<&Settings>) -> Vec<Spot> {
    settings.map(|s| s.spots.iter().map(spot_of).collect()).unwrap_or_default()
}

/// Runs a LightCraft command and keeps the failure text for the panel.
pub fn run(app: &mut FilmlabApp, cx: &mut Context<FilmlabApp>, name: &str, params: Value, label: &str, key: &str) -> Option<Value> {
    let (result, error) = app.develop.update(cx, |d, cx| {
        let result = d.command(name, params, label, key, cx);
        let error = if result.is_none() { d.error.take() } else { None };
        (result, error)
    });
    app.tools.heal.message = error;
    cx.notify();
    result
}

pub fn add_spot(app: &mut FilmlabApp, cx: &mut Context<FilmlabApp>, mode: Mode, points: &[V], size: f64, feather: f64, opacity: f64) -> Option<usize> {
    app.tools.heal.adds += 1;
    let key = format!("spot.add.{}", app.tools.heal.adds);
    let pairs: Vec<[f64; 2]> = points.iter().map(|&(x, y)| [x, y]).collect();
    let params = json!({"mode": mode.id(), "points": pairs, "size": size, "feather": feather, "opacity": opacity});
    let result = run(app, cx, "spot.add", params, &format!("{} spot", mode.label()), &key)?;
    result.get("index").and_then(Value::as_u64).map(|i| i as usize)
}

pub fn update_spot(app: &mut FilmlabApp, cx: &mut Context<FilmlabApp>, index: usize, field: &str, value: Value, label: &str, key: &str) {
    let mut params = json!({"index": index});
    params[field] = value;
    run(app, cx, "spot.update", params, label, &format!("spot.{index}.{key}"));
}

pub fn delete_spot(app: &mut FilmlabApp, cx: &mut Context<FilmlabApp>, index: usize) {
    run(app, cx, "spot.delete", json!({"index": index}), "Delete spot", "spot.delete");
}

pub fn clear_spots(app: &mut FilmlabApp, cx: &mut Context<FilmlabApp>, count: usize) {
    for index in (0..count).rev() {
        if run(app, cx, "spot.delete", json!({"index": index}), "Clear spots", "spot.clear").is_none() {
            break;
        }
    }
}

pub fn refresh_source(app: &mut FilmlabApp, cx: &mut Context<FilmlabApp>, index: usize) {
    run(app, cx, "spot.refreshSource", json!({"index": index}), "Refresh source", &format!("spot.{index}.refresh"));
}

pub fn find_dust(app: &mut FilmlabApp, cx: &mut Context<FilmlabApp>) {
    let Some(result) = run(app, cx, "spot.findDust", json!({}), "Find dust", "spot.dust") else { return };
    if result.get("added").and_then(Value::as_u64).unwrap_or(0) == 0 {
        app.tools.heal.message = Some("No dust spots found.".into());
    }
}
