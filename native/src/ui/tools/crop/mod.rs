mod aspect;
mod commands;
mod custom;
mod geometry;
mod guides;
mod overlay;
mod panel;

use gpui_kit::*;

use crate::develop::Tool;
use crate::ui::app::FilmlabApp;

pub use self::overlay::overlay;
pub use self::panel::panel;

#[derive(Default)]
pub struct State {
    error: Option<String>,
    custom: bool,
    fields: Option<custom::Fields>,
    angle_reset: bool,
    frame: Option<(Bounds<Pixels>, (u32, u32))>,
    gesture: Option<overlay::Gesture>,
    gestures: u64,
    readout: Option<(geometry::Vec2, f64)>,
    cursor: CursorStyle,
}

/// Where the edit history stood when the crop tool opened, so Escape can go back to it.
#[derive(Clone)]
struct Entry {
    photo_id: Option<String>,
    cursor: usize,
}

fn finish(app: &mut FilmlabApp, cx: &mut Context<FilmlabApp>) {
    app.set_tool(Tool::Edit, cx);
}

fn cancel(app: &mut FilmlabApp, entry: &Entry, cx: &mut Context<FilmlabApp>) {
    let (photo_id, cursor) = {
        let d = app.develop.read(cx);
        (d.photo_id.clone(), d.cursor)
    };
    if entry.photo_id == photo_id && entry.cursor != cursor {
        app.develop.update(cx, |d, cx| d.go_to(entry.cursor, cx));
    }
    finish(app, cx);
}
