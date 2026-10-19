use serde_json::{Value, json};

use gpui_kit::Context;

use super::model::{CreateKind, MaskOp, Pt, Shape};
use super::state::Interaction;
use crate::ui::app::FilmlabApp;

type Cx<'a> = Context<'a, FilmlabApp>;

fn active_mask_of(result: &Value) -> Option<u32> {
    result.get("activeMask").and_then(Value::as_u64).map(|id| id as u32)
}

fn run(app: &mut FilmlabApp, command: &str, params: Value, label: &str, key: &str, cx: &mut Cx) -> Option<Value> {
    let out = app.develop.update(cx, |d, dcx| d.command(command, params, label, key, dcx));
    app.tools.masks.error = if out.is_some() { None } else { app.develop.read(cx).error.clone() };
    cx.notify();
    out
}

// why: `mask.select` only moves the engine's active mask and isn't an edit, so
// the history the command just wrote is put back
fn engine_select(app: &mut FilmlabApp, id: Option<u32>, cx: &mut Cx) {
    app.develop.update(cx, |d, _| {
        d.run("mask.select", json!({"id": id}));
    });
}

pub fn select_mask(app: &mut FilmlabApp, id: Option<u32>, cx: &mut Cx) {
    let photo = app.develop.read(cx).photo_id.clone();
    let state = &mut app.tools.masks;
    state.photo_id = photo;
    state.active = id;
    state.interaction = Interaction::Adjust;
    state.error = None;
    cx.notify();
}

pub fn choose_mask(app: &mut FilmlabApp, id: Option<u32>, cx: &mut Cx) {
    engine_select(app, id, cx);
    select_mask(app, id, cx);
}

pub fn add_mask(app: &mut FilmlabApp, kind: CreateKind, cx: &mut Cx) {
    if kind == CreateKind::Brush {
        engine_select(app, None, cx);
        select_mask(app, None, cx);
        app.tools.masks.interaction = Interaction::Paint(None);
        return;
    }
    let label = format!("Add {}", kind.label().to_lowercase());
    let Some(result) = run(app, "mask.add", json!({"kind": kind.key()}), &label, "mask.add", cx) else { return };
    select_mask(app, active_mask_of(&result), cx);
    if kind == CreateKind::ColorRange {
        app.tools.masks.interaction = Interaction::Pick;
    }
}

pub fn add_component(app: &mut FilmlabApp, mask: u32, op: MaskOp, kind: CreateKind, cx: &mut Cx) {
    if kind != CreateKind::Brush || op != MaskOp::Add {
        let label = format!("{} {}", op.key(), kind.label().to_lowercase());
        let params = json!({"id": mask, "op": op.key(), "kind": kind.key()});
        if run(app, "mask.addComponent", params, &label, "mask.component", cx).is_none() {
            return;
        }
    }
    select_mask(app, Some(mask), cx);
    match kind {
        CreateKind::ColorRange => app.tools.masks.interaction = Interaction::Pick,
        CreateKind::Brush => app.tools.masks.interaction = Interaction::Paint(Some(mask)),
        _ => {}
    }
}

pub fn delete_mask(app: &mut FilmlabApp, id: u32, cx: &mut Cx) {
    if let Some(result) = run(app, "mask.delete", json!({"id": id}), "Delete mask", "mask.delete", cx) {
        select_mask(app, active_mask_of(&result), cx);
    }
}

pub fn toggle_visible(app: &mut FilmlabApp, id: u32, cx: &mut Cx) {
    run(app, "mask.visible", json!({"id": id}), "Toggle mask", "mask.visible", cx);
}

pub fn invert_mask(app: &mut FilmlabApp, id: u32, cx: &mut Cx) {
    run(app, "mask.invert", json!({"id": id}), "Invert mask", "mask.invert", cx);
}

pub fn rename_mask(app: &mut FilmlabApp, id: u32, name: &str, cx: &mut Cx) {
    let name = name.trim();
    if !name.is_empty() {
        run(app, "mask.rename", json!({"id": id, "name": name}), "Rename mask", "mask.rename", cx);
    }
}

pub fn duplicate_mask(app: &mut FilmlabApp, id: u32, cx: &mut Cx) {
    if let Some(result) = run(app, "mask.duplicate", json!({"id": id}), "Duplicate mask", "mask.duplicate", cx) {
        select_mask(app, active_mask_of(&result), cx);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ComponentAction {
    Invert,
    Delete,
    Op(MaskOp),
}

pub fn edit_component(app: &mut FilmlabApp, id: u32, component: usize, action: ComponentAction, cx: &mut Cx) {
    let (name, op) = match action {
        ComponentAction::Invert => ("invert", None),
        ComponentAction::Delete => ("delete", None),
        ComponentAction::Op(op) => ("op", Some(op)),
    };
    let mut params = json!({"id": id, "component": component, "action": name});
    if let Some(op) = op {
        params["op"] = json!(op.key());
    }
    let result = run(app, "mask.component", params, &format!("{name} component"), "mask.component", cx);
    if let (Some(result), ComponentAction::Delete) = (result, action) {
        select_mask(app, active_mask_of(&result), cx);
    }
}

pub fn set_adjust(app: &mut FilmlabApp, id: u32, field: &str, value: f64, label: &str, cx: &mut Cx) {
    run(app, "mask.adjust", json!({"id": id, "values": {field: value}}), label, &format!("mask.adjust:{id}:{field}"), cx);
}

pub fn set_refine(app: &mut FilmlabApp, id: u32, value: f64, cx: &mut Cx) {
    run(app, "mask.refine", json!({"id": id, "value": value}), &format!("Refine edges {}", value.round()), &format!("mask.refine:{id}"), cx);
}

pub fn update_shape(app: &mut FilmlabApp, id: u32, component: usize, shape: &Shape, label: &str, cx: &mut Cx) {
    run(app, "mask.update", json!({"id": id, "component": component, "shape": shape.params()}), label, &format!("mask.shape:{id}:{component}"), cx);
}

pub fn paint_stroke(app: &mut FilmlabApp, mask: Option<u32>, points: &[Pt], erase: bool, connect: bool, cx: &mut Cx) {
    let brush = app.tools.masks.brush;
    let mut params = json!({
        "points": points.iter().map(|p| [p.x, p.y]).collect::<Vec<_>>(),
        "size": brush.size,
        "feather": brush.feather,
        "flow": brush.flow,
        "erase": erase,
        "autoMask": brush.auto_mask,
        "connect": connect,
    });
    if let Some(id) = mask {
        params["id"] = json!(id);
    }
    let label = if erase { "Erase brush stroke" } else { "Brush stroke" };
    if let Some(result) = run(app, "mask.brushStroke", params, label, "mask.brush", cx) {
        let created = active_mask_of(&result);
        select_mask(app, created, cx);
        app.tools.masks.interaction = Interaction::Paint(created);
    }
}

pub fn sample_color(app: &mut FilmlabApp, mask: u32, at: Pt, add: bool, cx: &mut Cx) {
    let params = json!({"id": mask, "x": at.x, "y": at.y, "add": add});
    run(app, "mask.sampleColor", params, "Pick color", &format!("mask.{mask}.sample"), cx);
}

/// The component's shape as it is right now, for drags that continue past a re-render.
pub fn current_shape(app: &FilmlabApp, id: u32, component: usize, cx: &Cx) -> Option<Shape> {
    let masks = super::model::read_masks(app.develop.read(cx).settings.as_ref());
    masks.into_iter().find(|mask| mask.id == id)?.components.into_iter().nth(component).map(|c| c.shape)
}
