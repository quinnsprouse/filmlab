use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::aspect::{OPTIONS, aspect_name, aspect_value};
use super::geometry::read_crop;
use super::{Entry, cancel, commands, custom, finish};
use crate::ui::app::FilmlabApp;
use crate::ui::controls::{Picker, PickerItem, Toggle, header};
use crate::ui::slider::ParamSlider;
use crate::ui::theme;

type Action = fn(&mut FilmlabApp, &mut Context<FilmlabApp>);

fn button(id: &'static str, label: &'static str, action: Action, cx: &mut Context<FilmlabApp>) -> Stateful<Div> {
    div()
        .id(id)
        .flex_1()
        .h(px(theme::ROW))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(theme::RADIUS))
        .bg(theme::surface())
        .text_size(px(13.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(theme::label())
        .cursor_pointer()
        .hover(|s| s.bg(theme::surface_hover()).text_color(theme::primary()))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(move |app, _, _, cx| {
            action(app, cx);
            cx.notify();
        }))
        .child(label)
}

fn pair(a: impl IntoElement, b: impl IntoElement) -> Div {
    div().flex().gap(px(theme::GAP)).child(a).child(b)
}

fn show(app: &mut FilmlabApp, error: Option<String>) {
    app.tools.crop.error = error;
}

fn orient(kind: &'static str, label: &'static str, app: &mut FilmlabApp, cx: &mut Context<FilmlabApp>) {
    let error = commands::orient(app, kind, label, cx);
    show(app, error);
}

fn choose_aspect(app: &mut FilmlabApp, value: &str, cx: &mut Context<FilmlabApp>) {
    app.tools.crop.custom = value == "custom";
    if value != "custom" {
        let error = commands::set_aspect(app, value, &format!("Crop {}", aspect_name(value)), cx);
        show(app, error);
    }
}

fn change_angle(app: &mut FilmlabApp, angle: f64, cx: &mut Context<FilmlabApp>) {
    // why: the slider reports a double-click reset as a plain change to 0, so the capture handler on its wrapper flags it
    if angle == 0.0 && std::mem::take(&mut app.tools.crop.angle_reset) {
        commands::reset_angle(app, cx);
    } else {
        app.tools.crop.angle_reset = false;
        commands::set_angle(app, angle, cx);
    }
}

fn key_down(app: &mut FilmlabApp, entry: &Entry, event: &KeyDownEvent, window: &Window, cx: &mut Context<FilmlabApp>) {
    let m = event.keystroke.modifiers;
    if m.platform || m.control || m.alt || custom::typing(app, window, cx) {
        return;
    }
    match event.keystroke.key.as_str() {
        "escape" => cancel(app, entry, cx),
        "enter" => finish(app, cx),
        "x" if !m.shift => {
            let error = commands::rotate_aspect(app, cx);
            show(app, error);
            cx.notify();
        }
        _ => {}
    }
}

fn keys(entry: Entity<Entry>, cx: &mut Context<FilmlabApp>) -> impl IntoElement {
    let me = cx.entity();
    canvas(
        |_, _, _| {},
        move |_, _, window, _| {
            let (me, entry) = (me.clone(), entry.clone());
            window.on_key_event(move |event: &KeyDownEvent, phase, window, cx| {
                if phase == DispatchPhase::Bubble {
                    let entry = entry.read(cx).clone();
                    me.update(cx, |app, cx| key_down(app, &entry, event, window, cx));
                }
            });
        },
    )
    .absolute()
    .size_0()
}

pub fn panel(app: &mut FilmlabApp, window: &mut Window, cx: &mut Context<FilmlabApp>) -> AnyElement {
    let (settings, original, angle, photo_id, cursor) = {
        let d = app.develop.read(cx);
        let Some(settings) = d.settings.clone() else { return Empty.into_any_element() };
        let original = d.fit.as_ref().map_or(1.0, |f| f64::from(f.width) / f64::from(f.height.max(1)));
        (settings, original, d.control_values().get("crop.angle").copied().unwrap_or(0.0), d.photo_id.clone(), d.cursor)
    };
    let entry = window.use_keyed_state("crop-session", cx, move |_, _| Entry { photo_id, cursor });
    let crop = read_crop(&settings);
    let shown = if app.tools.crop.custom { "custom" } else { aspect_value(crop.aspect, original) };
    let me = cx.entity();
    let (pick, slide) = (me.clone(), me.clone());
    let fields = app.tools.crop.custom.then(|| custom::fields(app, window, cx));
    let custom_row = fields.map(|(w, h)| custom::row(&w, &h, cx));
    let error = app.tools.crop.error.clone();
    let items = move || OPTIONS.iter().map(|(value, label)| PickerItem { label: (*label).to_string(), checked: *value == shown, header: false }).collect();
    let toggle_me = me.clone();
    div()
        .p(px(10.))
        .flex()
        .flex_col()
        .gap(px(theme::GAP))
        .child(header("Crop"))
        .child(Picker::new("crop-aspect", "Aspect", aspect_name(shown), items, move |i, _, cx| {
            if let Some((value, _)) = OPTIONS.get(i) {
                pick.update(cx, |app, cx| {
                    choose_aspect(app, value, cx);
                    cx.notify();
                });
            }
        }))
        .children(custom_row)
        .child(Toggle::new("crop-lock", "Lock aspect", crop.aspect.is_some(), move |locked, _, cx| {
            toggle_me.update(cx, |app, cx| {
                let label = if locked { "Lock crop aspect" } else { "Unlock crop aspect" };
                let error = commands::set_aspect(app, if locked { "current" } else { "free" }, label, cx);
                show(app, error);
                cx.notify();
            })
        }))
        .child(pair(
            button("crop-rotate-aspect", "Rotate aspect (X)", |app, cx| { let e = commands::rotate_aspect(app, cx); show(app, e) }, cx),
            button("crop-auto", "Auto straighten", |app, cx| { let e = commands::auto_straighten(app, cx); show(app, e) }, cx),
        ))
        .child(
            div()
                .capture_any_mouse_down(cx.listener(|app, event: &MouseDownEvent, _, _| app.tools.crop.angle_reset = event.click_count >= 2))
                .child(ParamSlider::new(ElementId::Name("crop-angle".into()), "Angle", angle, -45.0, 45.0).step(0.01).on_change(move |value, _, cx| {
                    slide.update(cx, |app, cx| {
                        change_angle(app, value, cx);
                        cx.notify();
                    })
                })),
        )
        .child(pair(
            button("crop-rotate-left", "Rotate left", |app, cx| orient("rotateLeft", "Rotate left", app, cx), cx),
            button("crop-rotate-right", "Rotate right", |app, cx| orient("rotateRight", "Rotate right", app, cx), cx),
        ))
        .child(pair(
            button("crop-flip-h", "Flip horizontal", |app, cx| orient("flipHorizontal", "Flip horizontal", app, cx), cx),
            button("crop-flip-v", "Flip vertical", |app, cx| orient("flipVertical", "Flip vertical", app, cx), cx),
        ))
        .when_some(error, |d, text| d.child(div().px(px(2.)).text_size(px(12.)).text_color(theme::secondary()).child(text)))
        .child(pair(
            button("crop-reset", "Reset crop", |app, cx| { let e = commands::reset_crop(app, cx); show(app, e) }, cx),
            button("crop-done", "Done", finish, cx),
        ))
        .child(keys(entry, cx))
        .into_any_element()
}
