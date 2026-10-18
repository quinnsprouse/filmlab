use gpui_kit::*;

use super::commands::{add_mask, delete_mask};
use super::model::CreateKind;
use super::state::Interaction;
use crate::ui::app::FilmlabApp;

fn resize_brush(app: &mut FilmlabApp, larger: bool, shift: bool) {
    let brush = &mut app.tools.masks.brush;
    if shift {
        brush.feather = (brush.feather + if larger { 10.0 } else { -10.0 }).clamp(0.0, 100.0);
    } else {
        let factor = if larger { 1.2 } else { 1.0 / 1.2 };
        brush.size = (brush.size * factor).clamp(0.002, 0.5);
    }
}

/// Masks shortcuts: O overlay, B brush, L linear gradient, Delete removes the mask, [ ] resize the brush,
/// Escape leaves painting or picking. Returns whether the key was used.
pub fn on_key(app: &mut FilmlabApp, event: &KeyDownEvent, cx: &mut Context<FilmlabApp>) -> bool {
    let keystroke = &event.keystroke;
    let m = keystroke.modifiers;
    if app.tools.masks.rename.is_some() || m.platform || m.control || m.alt {
        return false;
    }
    let photo = app.develop.read(cx).photo_id.clone();
    let active = app.tools.masks.active_for(photo.as_ref());
    match keystroke.key.as_str() {
        "[" | "]" => resize_brush(app, keystroke.key == "]", m.shift),
        _ if m.shift => return false,
        "o" => app.tools.masks.show_overlay = !app.tools.masks.show_overlay,
        "b" => add_mask(app, CreateKind::Brush, cx),
        "l" => add_mask(app, CreateKind::Linear, cx),
        "delete" | "backspace" if active.is_some() => {
            if let Some(id) = active {
                delete_mask(app, id, cx);
            }
        }
        "escape" if app.tools.masks.interaction != Interaction::Adjust => app.tools.masks.interaction = Interaction::Adjust,
        _ => return false,
    }
    cx.notify();
    true
}
