use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use serde_json::json;

use super::spots::{self, Mode, SIZE_MAX, SIZE_MIN, Spot};
use crate::develop::Tool;
use crate::ui::app::FilmlabApp;
use crate::ui::controls::{Segmented, Toggle, header};
use crate::ui::slider::ParamSlider;
use crate::ui::{icon, theme};

#[derive(Clone, Copy)]
enum Field {
    Size,
    Feather,
    Opacity,
}

impl Field {
    fn name(self) -> &'static str {
        match self {
            Field::Size => "size",
            Field::Feather => "feather",
            Field::Opacity => "opacity",
        }
    }
}

fn change(app: &mut FilmlabApp, cx: &mut Context<FilmlabApp>, field: Field, value: f64) {
    let brush = &mut app.tools.heal.brush;
    match field {
        Field::Size => brush.size = value,
        Field::Feather => brush.feather = value,
        Field::Opacity => brush.opacity = value,
    }
    let (photo, spots) = super::current(app, cx);
    if let Some(i) = app.tools.heal.selected(photo.as_deref(), spots.len()) {
        let name = field.name();
        spots::update_spot(app, cx, i, name, json!(value), &format!("Spot {name}"), name);
    }
    cx.notify();
}

fn button(id: &'static str, label: &'static str, enabled: bool, on_click: impl Fn(&mut Window, &mut App) + 'static) -> impl IntoElement {
    div()
        .id(id)
        .flex_1()
        .h(px(32.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(theme::RADIUS))
        .bg(theme::surface())
        .text_size(px(13.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(theme::label())
        .when(!enabled, |d| d.opacity(0.4))
        .when(enabled, |d| d.cursor_pointer().hover(|s| s.bg(theme::surface_hover()).text_color(theme::primary())))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .when(enabled, |d| d.on_click(move |_, window, cx| on_click(window, cx)))
        .child(label)
}

fn brush_rows(app: &FilmlabApp, me: &Entity<FilmlabApp>) -> Vec<AnyElement> {
    let brush = &app.tools.heal.brush;
    let options = Mode::ALL.iter().map(|m| (SharedString::from(m.label()), *m == brush.mode)).collect();
    let mode = me.clone();
    let slider = |id: &'static str, label: &'static str, field: Field, value: f64, min: f64, max: f64, fallback: f64, step: f64| {
        let me = me.clone();
        ParamSlider::new(id, label, value, min, max)
            .step(step)
            .fallback(fallback)
            .on_change(move |v, _, cx| me.update(cx, |app, cx| change(app, cx, field, v)))
    };
    vec![
        Segmented::new("heal-mode", options, move |i, _, cx| {
            mode.update(cx, |app, cx| {
                if let Some(m) = Mode::ALL.get(i) {
                    app.tools.heal.brush.mode = *m;
                }
                cx.notify();
            })
        })
        .label("Mode")
        .into_any_element(),
        slider("heal-size", "Size", Field::Size, brush.size * 100.0, SIZE_MIN * 100.0, SIZE_MAX * 100.0, 2.0, 0.1)
            .shown(format!("{:.1}%", brush.size * 100.0))
            .into_any_element(),
        slider("heal-feather", "Feather", Field::Feather, brush.feather, 0.0, 100.0, 50.0, 1.0).into_any_element(),
        slider("heal-opacity", "Opacity", Field::Opacity, brush.opacity, 0.0, 100.0, 100.0, 1.0).into_any_element(),
    ]
}

fn visualize_rows(app: &FilmlabApp, cx: &App) -> Vec<AnyElement> {
    let d = app.develop.read(cx);
    let threshold = d.overlay.as_ref().filter(|o| o["kind"] == "spots").map(|o| o["threshold"].as_f64().unwrap_or(50.0));
    let (toggle, slide) = (app.develop.clone(), app.develop.clone());
    let mut rows = vec![
        Toggle::new("heal-visualize", "Visualize spots", threshold.is_some(), move |on, _, cx| {
            toggle.update(cx, |d, cx| d.set_overlay(on.then(|| json!({"kind": "spots", "threshold": 50})), cx))
        })
        .into_any_element(),
    ];
    if let Some(value) = threshold {
        rows.push(
            ParamSlider::new("heal-sensitivity", "Sensitivity", value, 0.0, 100.0)
                .fallback(50.0)
                .on_change(move |v, _, cx| slide.update(cx, |d, cx| d.set_overlay(Some(json!({"kind": "spots", "threshold": v})), cx)))
                .into_any_element(),
        );
    }
    rows
}

fn spot_row(index: usize, spot: &Spot, active: bool, me: &Entity<FilmlabApp>) -> impl IntoElement {
    let (pick, remove) = (me.clone(), me.clone());
    let (picked, mode) = (spot.clone(), spot.mode);
    div()
        .flex()
        .items_center()
        .rounded(px(6.))
        .when(active, |d| d.bg(theme::selected()))
        .child(
            div()
                .id(("heal-spot", index))
                .flex_1()
                .h(px(30.))
                .px(px(8.))
                .flex()
                .items_center()
                .gap(px(8.))
                .cursor_pointer()
                .text_size(px(13.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(if active { theme::primary() } else { theme::label() })
                .hover(|s| s.bg(theme::hover()))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(move |_, _, cx| {
                    pick.update(cx, |app, cx| {
                        let (photo, spots) = super::current(app, cx);
                        let h = &mut app.tools.heal;
                        if h.selected(photo.as_deref(), spots.len()) == Some(index) {
                            h.select(photo.as_deref(), None);
                        } else {
                            h.pick(photo.as_deref(), index, Some(&picked));
                        }
                        cx.notify();
                    })
                })
                .child(div().w(px(16.)).text_color(theme::tertiary()).font_features(theme::tabular()).child(format!("{}", index + 1)))
                .child(mode.label()),
        )
        .child(
            div()
                .id(("heal-spot-delete", index))
                .size(px(30.))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .opacity(0.6)
                .hover(|s| s.opacity(1.0))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(move |_, _, cx| {
                    remove.update(cx, |app, cx| {
                        spots::delete_spot(app, cx, index);
                        let photo = app.develop.read(cx).photo_id.clone();
                        app.tools.heal.select(photo.as_deref(), None);
                        cx.notify();
                    })
                })
                .child(icon::icon("cancel01", px(14.))),
        )
}

fn spot_list(spots: &[Spot], selected: Option<usize>, me: &Entity<FilmlabApp>) -> impl IntoElement {
    let count = spots.len();
    let clear = me.clone();
    div()
        .flex()
        .flex_col()
        .gap(px(4.))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(div().flex().items_center().gap(px(6.)).child(header("Spots")).child(div().text_size(px(12.)).text_color(theme::tertiary()).font_features(theme::tabular()).child(format!("{count}"))))
                .child(
                    div()
                        .id("heal-clear")
                        .text_size(px(12.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme::label())
                        .when(count == 0, |d| d.opacity(0.4))
                        .when(count > 0, |d| {
                            d.cursor_pointer().hover(|s| s.text_color(theme::primary())).on_click(move |_, _, cx| {
                                clear.update(cx, |app, cx| {
                                    spots::clear_spots(app, cx, count);
                                    let photo = app.develop.read(cx).photo_id.clone();
                                    app.tools.heal.select(photo.as_deref(), None);
                                    cx.notify();
                                })
                            })
                        })
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child("Clear all"),
                ),
        )
        .child(if count == 0 {
            div().text_size(px(13.)).text_color(theme::tertiary()).child("Click the photo to add a spot, or drag to paint one.").into_any_element()
        } else {
            div()
                .id("heal-spots")
                .max_h(px(192.))
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap(px(2.))
                .children(spots.iter().enumerate().map(|(i, s)| spot_row(i, s, Some(i) == selected, me)))
                .into_any_element()
        })
}

pub fn panel(app: &mut FilmlabApp, _window: &mut Window, cx: &mut Context<FilmlabApp>) -> AnyElement {
    let me = cx.entity();
    let (photo, spots) = super::current(app, cx);
    let selected = app.tools.heal.selected(photo.as_deref(), spots.len());
    let sourced = selected.and_then(|i| spots.get(i)).is_some_and(|s| s.mode != Mode::Fill);
    let (refresh, dust, done) = (me.clone(), me.clone(), me.clone());
    div()
        .id("heal-scroll")
        .size_full()
        .overflow_y_scroll()
        .px(px(12.))
        .pt(px(12.))
        .pb(px(12.))
        .flex()
        .flex_col()
        .gap(px(theme::GAP))
        .child(div().h(px(28.)).flex().items_center().text_size(px(15.)).font_weight(FontWeight::SEMIBOLD).text_color(theme::focus()).child("Heal"))
        .children(brush_rows(app, &me))
        .children(visualize_rows(app, cx))
        .child(
            div()
                .flex()
                .gap(px(theme::GAP))
                .child(button("heal-refresh", "Refresh source", selected.is_some() && sourced, move |_, cx| {
                    refresh.update(cx, |app, cx| {
                        let (photo, spots) = super::current(app, cx);
                        if let Some(i) = app.tools.heal.selected(photo.as_deref(), spots.len()) {
                            spots::refresh_source(app, cx, i);
                        }
                    })
                }))
                .child(button("heal-dust", "Find dust", true, move |_, cx| dust.update(cx, |app, cx| spots::find_dust(app, cx)))),
        )
        .children(app.tools.heal.message.clone().map(|m| div().text_size(px(13.)).text_color(theme::label()).child(m)))
        .child(div().mt(px(6.)).child(spot_list(&spots, selected, &me)))
        .child(div().mt(px(6.)).flex().child(button("heal-done", "Done", true, move |_, cx| {
            done.update(cx, |app, cx| {
                app.develop.update(cx, |d, cx| d.set_tool(Tool::Edit, cx));
                cx.notify();
            })
        })))
        .into_any_element()
}
