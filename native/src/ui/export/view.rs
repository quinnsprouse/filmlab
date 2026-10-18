use std::sync::atomic::Ordering;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::run::{FORMATS, Format};
use super::{Phase, back, close, ids, start};
use crate::ui::app::FilmlabApp;
use crate::ui::controls::{Segmented, header};
use crate::ui::slider::ParamSlider;
use crate::ui::theme;

fn plural(count: usize) -> String {
    if count == 1 { "1 photo".into() } else { format!("{count} photos") }
}

fn button(id: &'static str, label: impl Into<SharedString>, primary: bool, enabled: bool, cx: &mut Context<FilmlabApp>, on_click: impl Fn(&mut FilmlabApp, &mut Window, &mut Context<FilmlabApp>) + 'static) -> impl IntoElement {
    let base = div()
        .id(id)
        .h(px(32.))
        .px(px(14.))
        .flex()
        .items_center()
        .rounded(px(theme::RADIUS))
        .text_size(px(13.))
        .font_weight(FontWeight::MEDIUM)
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());
    let styled = if primary { base.bg(theme::fill_active()).text_color(theme::primary()) } else { base.text_color(theme::label()) };
    if enabled {
        styled.cursor_pointer().hover(|s| s.bg(theme::selected()).text_color(theme::focus())).on_click(cx.listener(move |app, _, window, cx| on_click(app, window, cx))).child(label.into())
    } else {
        styled.opacity(0.4).child(label.into())
    }
}

fn form(app: &FilmlabApp, cx: &mut Context<FilmlabApp>) -> Vec<AnyElement> {
    let choices = app.export.choices;
    let me = cx.entity();
    let (formats, sizes) = (me.clone(), me.clone());
    let mut rows = vec![
        Segmented::new("export-format", FORMATS.iter().map(|f| (SharedString::from(f.label()), *f == choices.format)).collect(), move |i, _, cx| {
            formats.update(cx, |app, cx| {
                app.export.choices.format = FORMATS[i];
                cx.notify();
            })
        })
        .label("Format")
        .into_any_element(),
        Segmented::new("export-size", vec![("Full size".into(), choices.long_edge.is_none()), ("Long edge".into(), choices.long_edge.is_some())], move |i, _, cx| {
            sizes.update(cx, |app, cx| {
                app.export.choices.long_edge = (i == 1).then_some(2048);
                cx.notify();
            })
        })
        .label("Size")
        .into_any_element(),
    ];
    if let Some(edge) = choices.long_edge {
        let me = me.clone();
        rows.push(
            ParamSlider::new("export-edge", "Long edge", f64::from(edge), 64.0, 8192.0)
                .step(64.0)
                .fallback(2048.0)
                .shown(format!("{edge} px"))
                .on_change(move |v, _, cx| {
                    me.update(cx, |app, cx| {
                        app.export.choices.long_edge = Some(v.round().max(64.0) as u32);
                        cx.notify();
                    })
                })
                .into_any_element(),
        );
    }
    if choices.format == Format::Jpeg {
        rows.push(
            ParamSlider::new("export-quality", "Quality", f64::from(choices.quality), 0.0, 100.0)
                .fallback(90.0)
                .on_change(move |v, _, cx| {
                    me.update(cx, |app, cx| {
                        app.export.choices.quality = v.round() as u32;
                        cx.notify();
                    })
                })
                .into_any_element(),
        );
    }
    rows
}

fn note(text: impl Into<SharedString>) -> Div {
    div().text_size(px(12.)).text_color(theme::tertiary()).child(text.into())
}

fn progress(app: &FilmlabApp) -> Div {
    let state = &app.export;
    let shown = (state.finished + 1).min(state.total);
    let text = if state.stopped() { "Stopping…".to_string() } else { format!("Exporting {shown} of {}…", state.total) };
    let fraction = if state.total == 0 { 0.0 } else { state.finished as f32 / state.total as f32 };
    div()
        .flex()
        .flex_col()
        .gap(px(theme::GAP))
        .child(div().text_size(px(13.)).text_color(theme::label()).child(text))
        .child(div().h(px(6.)).w_full().rounded(px(3.)).bg(theme::track()).child(div().h_full().w(relative(fraction)).rounded(px(3.)).bg(theme::handle())))
}

fn summary(app: &FilmlabApp) -> Div {
    let state = &app.export;
    let verb = if state.stopped() { "Stopped after " } else { "Exported " };
    let headline = if state.exported.is_empty() && !state.errors.is_empty() { "Nothing was exported".to_string() } else { format!("{verb}{} to Exports", plural(state.exported.len())) };
    div()
        .flex()
        .flex_col()
        .gap(px(theme::GAP))
        .child(div().text_size(px(13.)).text_color(theme::label()).child(headline))
        .children(state.errors.iter().map(|e| div().text_size(px(12.)).text_color(theme::danger()).child(e.clone())))
}

fn footer(app: &FilmlabApp, count: usize, cx: &mut Context<FilmlabApp>) -> Div {
    let row = div().mt(px(18.)).flex().justify_end().gap(px(theme::GAP));
    match app.export.phase {
        Phase::Idle => row
            .child(button("export-cancel", "Cancel", false, true, cx, |app, window, cx| close(app, window, cx)))
            .child(button("export-go", format!("Export {}", plural(count)), true, count > 0, cx, |app, _, cx| start(app, cx))),
        Phase::Running => row.child(button("export-stop", "Cancel", false, !app.export.stopped(), cx, |app, _, cx| {
            app.export.cancel.store(true, Ordering::Relaxed);
            cx.notify();
        })),
        Phase::Done => {
            let first = app.export.exported.first().and_then(|p| p.parent()).map(|p| p.to_path_buf());
            let failed = !app.export.errors.is_empty();
            row.when(failed, |r| r.child(button("export-back", "Back", false, true, cx, |app, _, cx| back(app, cx))))
                .when_some(first, |r, folder| r.child(button("export-reveal", "Show in Finder", false, true, cx, move |_, _, cx| cx.reveal_path(&folder))))
                .child(button("export-done", "Done", true, true, cx, |app, window, cx| close(app, window, cx)))
        }
    }
}

pub fn dialog(app: &mut FilmlabApp, _window: &mut Window, cx: &mut Context<FilmlabApp>) -> Option<AnyElement> {
    if !app.export.open {
        return None;
    }
    let focus = app.export.focus.clone()?;
    let count = ids(app, cx).len();
    let shown = if app.export.phase == Phase::Idle { count } else { app.export.total };
    let destination = app.library.read(cx).folder.as_ref().and_then(|f| f.file_name()).map(|n| format!("Saved to {}/Exports", n.to_string_lossy())).unwrap_or_else(|| "Open a folder first".into());
    let body = match app.export.phase {
        Phase::Idle => div().flex().flex_col().gap(px(theme::GAP)).children(form(app, cx)).child(div().pt(px(6.)).child(note(destination))),
        Phase::Running => progress(app),
        Phase::Done => summary(app),
    };
    let panel = div()
        .w(px(440.))
        .p(px(18.))
        .rounded(px(22.))
        .bg(hsla(0.0, 0.0, 0.1, 0.72))
        .border_1()
        .border_color(hsla(0.0, 0.0, 1.0, 0.1))
        .shadow_lg()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(header(format!("Export {}", plural(shown))))
        .child(body)
        .child(footer(app, count, cx));
    Some(
        div()
            .id("export-dialog")
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(hsla(0.0, 0.0, 0.0, 0.5))
            .occlude()
            .track_focus(&focus)
            .key_context("Dialog")
            .on_mouse_down(MouseButton::Left, cx.listener(|app, _, window, cx| if app.export.phase != Phase::Running { close(app, window, cx) }))
            .on_key_down(cx.listener(|app, event: &KeyDownEvent, window, cx| {
                cx.stop_propagation();
                match event.keystroke.key.as_str() {
                    "escape" => close(app, window, cx),
                    "enter" if app.export.phase == Phase::Idle => start(app, cx),
                    "enter" if app.export.phase == Phase::Done => close(app, window, cx),
                    _ => {}
                }
            }))
            .child(panel)
            .into_any_element(),
    )
}
