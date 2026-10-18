use serde_json::{Value, json};

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

    fn parse(id: Option<&str>) -> Mode {
        Mode::ALL.into_iter().find(|m| Some(m.id()) == id).unwrap_or(Mode::Remove)
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

fn vec_of(value: &Value) -> Option<V> {
    Some((value.get("x")?.as_f64()?, value.get("y")?.as_f64()?))
}

fn spot_of(raw: &Value) -> Spot {
    let num = |key: &str, fallback: f64| raw.get(key).and_then(Value::as_f64).unwrap_or(fallback);
    Spot {
        mode: Mode::parse(raw.get("mode").and_then(Value::as_str)),
        points: raw.get("points").and_then(Value::as_array).map(|a| a.iter().filter_map(vec_of).collect()).unwrap_or_default(),
        size: num("size", 0.02),
        feather: num("feather", 50.0),
        opacity: num("opacity", 100.0),
        source: raw.get("source_offset").and_then(vec_of),
    }
}

pub fn spots_of(settings: Option<&Value>) -> Vec<Spot> {
    settings.and_then(|s| s.get("spots")).and_then(Value::as_array).map(|a| a.iter().map(spot_of).collect()).unwrap_or_default()
}

/// Runs a LightCraft command and keeps the failure text for the panel (the web `guarded`).
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
