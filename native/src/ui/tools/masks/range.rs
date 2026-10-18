use std::sync::Arc;

use gpui_kit::*;

use super::commands::{current_shape, update_shape};
use super::model::{ColorRange, Luminance, Shape};
use super::state::Interaction;
use super::widgets::{body_text, on_flag, on_value, text_button};
use crate::ui::app::FilmlabApp;
use crate::ui::controls::Toggle;
use crate::ui::slider::{Nothing, ParamSlider};
use crate::ui::theme;

const MIN_GAP: f64 = 0.01;
const MAX_FEATHER: f64 = 0.5;

#[derive(Clone)]
struct RangeDrag(ElementId);

fn child_id(id: &ElementId, name: &'static str) -> ElementId {
    ElementId::NamedChild(Arc::new(id.clone()), name.into())
}

fn value_at(x: Pixels, bounds: Bounds<Pixels>) -> f64 {
    let width = f64::from(f32::from(bounds.size.width)).max(1.0);
    (f64::from(f32::from(x - bounds.left())) / width).clamp(0.0, 1.0)
}

fn set_edge(app: &mut FilmlabApp, id: u32, index: usize, hi: bool, value: f64, cx: &mut Context<FilmlabApp>) {
    let Some(Shape::Luminance(mut range)) = current_shape(app, id, index, cx) else { return };
    if hi {
        range.hi = value.max(range.lo + MIN_GAP);
    } else {
        range.lo = value.min(range.hi - MIN_GAP);
    }
    update_shape(app, id, index, &Shape::Luminance(range), "Luminance range", cx);
}

fn range_bar(id: u32, index: usize, range: &Luminance, window: &mut Window, cx: &mut Context<FilmlabApp>) -> AnyElement {
    let key: ElementId = SharedString::from(format!("masks-range-{id}-{index}")).into();
    let bounds = window.use_keyed_state(child_id(&key, "bounds"), cx, |_, _| Bounds::<Pixels>::default());
    let which = window.use_keyed_state(child_id(&key, "which"), cx, |_, _| false);
    let (record, press_bounds, move_bounds) = (bounds.clone(), bounds.clone(), bounds.clone());
    let (press_which, move_which) = (which.clone(), which);
    let drag_key = key.clone();
    let (lo, hi) = (range.lo as f32, range.hi as f32);
    let dot = |at: f32| div().absolute().top(px(6.)).left(relative(at)).ml(px(-6.)).size(px(12.)).rounded_full().bg(theme::primary());
    div()
        .id(key.clone())
        .relative()
        .h(px(24.))
        .w_full()
        .cursor_ew_resize()
        .child(canvas(move |b, _, cx| record.update(cx, |s, _| *s = b), |_, _, _, _| {}).absolute().size_full())
        .child(div().absolute().left_0().right_0().top(px(4.)).bottom(px(4.)).rounded(px(6.)).bg(linear_gradient(90., linear_color_stop(hsla(0., 0., 0.04, 1.), 0.), linear_color_stop(hsla(0., 0., 0.95, 1.), 1.))))
        .child(div().absolute().top(px(2.)).bottom(px(2.)).left(relative(lo)).w(relative((hi - lo).max(0.))).rounded(px(3.)).border_1().border_color(theme::focus()))
        .child(dot(lo))
        .child(dot(hi))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |app, event: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                let value = value_at(event.position.x, *press_bounds.read(cx));
                let Some(Shape::Luminance(current)) = current_shape(app, id, index, cx) else { return };
                let upper = (value - current.lo).abs() > (value - current.hi).abs();
                press_which.update(cx, |s, _| *s = upper);
                app.tools.masks.engaged = true;
                set_edge(app, id, index, upper, value, cx);
            }),
        )
        .on_drag(RangeDrag(key), |_, _, _, cx| cx.new(|_| Nothing))
        .on_drag_move(cx.listener(move |app, event: &DragMoveEvent<RangeDrag>, _, cx| {
            if event.drag(cx).0 != drag_key {
                return;
            }
            let value = value_at(event.event.position.x, *move_bounds.read(cx));
            let upper = *move_which.read(cx);
            set_edge(app, id, index, upper, value, cx);
        }))
        .on_mouse_up(MouseButton::Left, cx.listener(|app, _: &MouseUpEvent, _, cx| release(app, cx)))
        .on_mouse_up_out(MouseButton::Left, cx.listener(|app, _: &MouseUpEvent, _, cx| release(app, cx)))
        .into_any_element()
}

fn release(app: &mut FilmlabApp, cx: &mut Context<FilmlabApp>) {
    if app.tools.masks.engaged {
        app.tools.masks.engaged = false;
        cx.notify();
    }
}

fn luminance_controls(app: &FilmlabApp, id: u32, index: usize, range: &Luminance, window: &mut Window, cx: &mut Context<FilmlabApp>) -> Vec<AnyElement> {
    let smooth = ((range.lo_feather + range.hi_feather) / 2.0 / MAX_FEATHER * 100.0).round();
    let base = range.clone();
    vec![
        range_bar(id, index, range, window, cx),
        ParamSlider::new(SharedString::from(format!("masks-smooth-{id}-{index}")), "Smoothness", smooth, 0.0, 100.0)
            .fallback(20.0)
            .on_change(on_value(cx, move |app, value, cx| {
                let feather = value / 100.0 * MAX_FEATHER;
                let shape = Shape::Luminance(Luminance { lo_feather: feather, hi_feather: feather, ..base.clone() });
                update_shape(app, id, index, &shape, "Edit mask", cx);
            }))
            .into_any_element(),
        Toggle::new("masks-luma-map", "Show luminance map", app.tools.masks.luma_map, on_flag(cx, |app, on, cx| {
            app.tools.masks.luma_map = on;
            cx.notify();
        }))
        .into_any_element(),
    ]
}

fn encode(channel: f32) -> f32 {
    let c = channel.clamp(0.0, 1.0);
    if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}

fn swatch(sample: [f64; 3]) -> Hsla {
    let [lightness, a, b] = sample.map(|v| v as f32);
    let l = (lightness + 0.396_337_78 * a + 0.215_803_76 * b).powi(3);
    let m = (lightness - 0.105_561_346 * a - 0.063_854_17 * b).powi(3);
    let s = (lightness - 0.089_484_18 * a - 1.291_485_5 * b).powi(3);
    let r = encode(4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_93 * s);
    let g = encode(-1.268_438 * l + 2.609_757_4 * m - 0.341_319_4 * s);
    let blue = encode(-0.004_196_086 * l - 0.703_418_6 * m + 1.707_614_7 * s);
    Rgba { r, g, b: blue, a: 1.0 }.into()
}

fn color_controls(app: &FilmlabApp, id: u32, index: usize, color: &ColorRange, cx: &mut Context<FilmlabApp>) -> Vec<AnyElement> {
    let picking = app.tools.masks.interaction == Interaction::Pick;
    let swatches = if color.samples.is_empty() {
        body_text("No colors picked yet.").into_any_element()
    } else {
        div()
            .flex()
            .gap(px(6.))
            .children(color.samples.iter().map(|s| div().size(px(24.)).rounded(px(6.)).border_1().border_color(theme::fill_active()).bg(swatch(*s))))
            .into_any_element()
    };
    let base = color.clone();
    vec![
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(8.))
            .child(swatches)
            .child(text_button(
                "masks-pick-color",
                if picking { "Done picking" } else { "Pick color" },
                picking,
                super::widgets::on_press(cx, move |app, _, cx| {
                    app.tools.masks.interaction = if picking { Interaction::Adjust } else { Interaction::Pick };
                    cx.notify();
                }),
            ))
            .into_any_element(),
        body_text("Click the photo to pick. Shift-click adds up to 5.").into_any_element(),
        ParamSlider::new(SharedString::from(format!("masks-refine-color-{id}-{index}")), "Refine", color.refine, 0.0, 100.0)
            .fallback(50.0)
            .on_change(on_value(cx, move |app, value, cx| {
                let shape = Shape::Color(ColorRange { refine: value, ..base.clone() });
                update_shape(app, id, index, &shape, "Edit mask", cx);
            }))
            .into_any_element(),
    ]
}

pub fn range_controls(app: &FilmlabApp, id: u32, index: usize, shape: &Shape, window: &mut Window, cx: &mut Context<FilmlabApp>) -> Vec<AnyElement> {
    match shape {
        Shape::Luminance(range) => luminance_controls(app, id, index, range, window, cx),
        Shape::Color(color) => color_controls(app, id, index, color, cx),
        Shape::Subject | Shape::Sky | Shape::Background => vec![body_text("Uses the file's embedded mattes or a quick estimate.").into_any_element()],
        _ => Vec::new(),
    }
}
