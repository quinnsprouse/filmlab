use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::*;

use super::aspect::parse_ratio;
use super::commands;
use crate::ui::app::FilmlabApp;
use crate::ui::theme;

pub struct Fields {
    width: Entity<InputState>,
    height: Entity<InputState>,
}

fn field(value: &'static str, window: &mut Window, cx: &mut Context<FilmlabApp>) -> Entity<InputState> {
    let state = cx.new(|cx| InputState::new(window, cx).default_value(value));
    cx.subscribe(&state, |app, _, event: &InputEvent, cx| {
        if matches!(event, InputEvent::PressEnter { .. }) {
            apply(app, cx);
        }
    })
    .detach();
    state
}

pub fn fields(app: &mut FilmlabApp, window: &mut Window, cx: &mut Context<FilmlabApp>) -> (Entity<InputState>, Entity<InputState>) {
    let made = app.tools.crop.fields.take().unwrap_or_else(|| Fields { width: field("3", window, cx), height: field("2", window, cx) });
    let pair = (made.width.clone(), made.height.clone());
    app.tools.crop.fields = Some(made);
    pair
}

pub fn typing(app: &FilmlabApp, window: &Window, cx: &App) -> bool {
    app.tools.crop.fields.as_ref().is_some_and(|f| [&f.width, &f.height].into_iter().any(|s| s.read(cx).focus_handle(cx).is_focused(window)))
}

fn apply(app: &mut FilmlabApp, cx: &mut Context<FilmlabApp>) {
    let Some(fields) = app.tools.crop.fields.as_ref() else { return };
    let (w, h) = (parse_ratio(&fields.width.read(cx).value()), parse_ratio(&fields.height.read(cx).value()));
    let error = match (w, h) {
        (Some(w), Some(h)) => commands::set_aspect_ratio(app, w, h, cx),
        _ => Some("Enter a width and a height above zero, for example 3 and 2.".into()),
    };
    app.tools.crop.custom = error.is_some();
    app.tools.crop.error = error;
    cx.notify();
}

fn box_for(state: &Entity<InputState>) -> Div {
    div().flex_1().min_w_0().h(px(theme::ROW)).px(px(8.)).flex().items_center().rounded(px(theme::RADIUS)).bg(theme::surface()).text_size(px(13.)).child(Input::new(state).appearance(false).bordered(false))
}

pub fn row(width: &Entity<InputState>, height: &Entity<InputState>, cx: &mut Context<FilmlabApp>) -> Div {
    let apply_button = div()
        .id("crop-aspect-apply")
        .h(px(theme::ROW))
        .px(px(12.))
        .flex()
        .items_center()
        .rounded(px(theme::RADIUS))
        .bg(theme::fill())
        .text_size(px(13.))
        .font_weight(FontWeight::MEDIUM)
        .cursor_pointer()
        .hover(|s| s.bg(theme::fill_active()))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(|app, _, _, cx| apply(app, cx)))
        .child("Apply");
    div().flex().items_center().gap(px(theme::GAP)).child(box_for(width)).child(div().text_color(theme::label()).child(":")).child(box_for(height)).child(apply_button)
}
