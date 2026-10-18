use gpui_kit::*;

use super::widgets::{body_text, on_flag, on_index, on_press, on_value, text_button};
use crate::ui::app::FilmlabApp;
use crate::ui::controls::{Segmented, Toggle};
use crate::ui::slider::ParamSlider;

use super::state::Interaction;

const SIZE_SCALE: f64 = 400.0;

pub fn brush_controls(app: &FilmlabApp, cx: &mut Context<FilmlabApp>) -> Vec<AnyElement> {
    let brush = app.tools.masks.brush;
    vec![
        body_text("Drag on the photo to paint. Hold Alt to swap paint and erase, Shift-click to draw a straight line. [ and ] resize the brush.").into_any_element(),
        Segmented::new(
            "masks-brush-mode",
            vec![("Paint".into(), !brush.erase), ("Erase".into(), brush.erase)],
            {
                let pick = on_index(cx, |app, index, cx| {
                    app.tools.masks.brush.erase = index == 1;
                    cx.notify();
                });
                move |index, window, cx| pick(index, window, cx)
            },
        )
        .label("Brush")
        .into_any_element(),
        ParamSlider::new("masks-brush-size", "Size", (brush.size * SIZE_SCALE).round(), 1.0, 100.0)
            .on_change(on_value(cx, |app, value, cx| {
                app.tools.masks.brush.size = value / SIZE_SCALE;
                cx.notify();
            }))
            .into_any_element(),
        ParamSlider::new("masks-brush-feather", "Feather", brush.feather, 0.0, 100.0)
            .on_change(on_value(cx, |app, value, cx| {
                app.tools.masks.brush.feather = value;
                cx.notify();
            }))
            .into_any_element(),
        ParamSlider::new("masks-brush-flow", "Flow", brush.flow, 1.0, 100.0)
            .on_change(on_value(cx, |app, value, cx| {
                app.tools.masks.brush.flow = value;
                cx.notify();
            }))
            .into_any_element(),
        Toggle::new("masks-brush-auto", "Auto mask", brush.auto_mask, on_flag(cx, |app, on, cx| {
            app.tools.masks.brush.auto_mask = on;
            cx.notify();
        }))
        .into_any_element(),
        text_button("masks-brush-done", "Done painting", false, on_press(cx, |app, _, cx| {
            app.tools.masks.interaction = Interaction::Adjust;
            cx.notify();
        }))
        .into_any_element(),
    ]
}
