use serde_json::{Value, json};

use super::geometry::format_angle;
use crate::ui::app::FilmlabApp;
use gpui_kit::Context;

type Cx<'a> = Context<'a, FilmlabApp>;

fn run(app: &FilmlabApp, command: &str, params: Value, label: &str, key: &str, cx: &mut Cx) -> Result<Value, String> {
    app.develop.update(cx, |d, cx| {
        d.error = None;
        d.command(command, params, label, key, cx).ok_or_else(|| d.error.take().unwrap_or_else(|| "That change couldn't be applied.".into()))
    })
}

fn attempt(app: &FilmlabApp, command: &str, params: Value, label: &str, key: &str, cx: &mut Cx) -> Option<String> {
    run(app, command, params, label, key, cx).err()
}

pub fn set_angle(app: &FilmlabApp, angle: f64, cx: &mut Cx) {
    let label = format!("Straighten {}°", format_angle(angle));
    app.develop.update(cx, |d, cx| d.command("develop.set", json!({"values": {"crop.angle": angle}}), &label, "crop.angle", cx));
}

pub fn reset_angle(app: &FilmlabApp, cx: &mut Cx) {
    app.develop.update(cx, |d, cx| d.command("develop.set", json!({"values": {"crop.angle": 0}}), "Straighten reset", "crop.angle.reset", cx));
}

pub fn drag_handle(app: &FilmlabApp, params: Value, gesture: u64, cx: &mut Cx) -> Option<String> {
    attempt(app, "crop.drag", params, "Crop", &format!("crop.drag:{gesture}"), cx)
}

pub fn straighten(app: &FilmlabApp, angle: f64, gesture: u64, cx: &mut Cx) -> Option<String> {
    let label = format!("Straighten {}°", format_angle(angle));
    attempt(app, "crop.straighten", json!({"angle": angle}), &label, &format!("crop.straighten:{gesture}"), cx)
}

pub fn set_aspect(app: &FilmlabApp, aspect: &str, label: &str, cx: &mut Cx) -> Option<String> {
    attempt(app, "crop.aspect", json!({"aspect": aspect}), label, "crop.aspect", cx)
}

pub fn set_aspect_ratio(app: &FilmlabApp, width: f64, height: f64, cx: &mut Cx) -> Option<String> {
    attempt(app, "crop.aspect", json!({"aspect": [width, height]}), &format!("Crop {width} : {height}"), "crop.aspect", cx)
}

pub fn rotate_aspect(app: &FilmlabApp, cx: &mut Cx) -> Option<String> {
    attempt(app, "crop.rotateAspect", json!({}), "Rotate crop", "crop.rotateAspect", cx)
}

pub fn reset_crop(app: &FilmlabApp, cx: &mut Cx) -> Option<String> {
    attempt(app, "crop.reset", json!({}), "Reset crop", "crop.reset", cx)
}

pub fn auto_straighten(app: &FilmlabApp, cx: &mut Cx) -> Option<String> {
    match run(app, "crop.autoStraighten", json!({}), "Auto straighten", "crop.autoStraighten", cx) {
        Err(e) if e.contains("no decoder") => Some("Auto straighten can't read this photo's pixels yet. Straighten by hand instead.".into()),
        Err(e) => Some(e),
        Ok(outcome) if outcome["changed"] == json!(false) => Some("No clear horizon or vertical lines found. Straighten by hand instead.".into()),
        Ok(_) => None,
    }
}

pub fn orient(app: &FilmlabApp, kind: &str, label: &str, cx: &mut Cx) -> Option<String> {
    attempt(app, &format!("photo.{kind}"), json!({}), label, &format!("photo.{kind}"), cx)
}
