use std::collections::HashSet;

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::store::{self, LOOK_GROUPS, PER_PHOTO_GROUPS};
use crate::ui::app::FilmlabApp;
use crate::ui::{icon, theme};

pub struct Form {
    name: Entity<InputState>,
    chosen: HashSet<&'static str>,
    _enter: Subscription,
}

pub fn open(app: &mut FilmlabApp, window: &mut Window, cx: &mut Context<FilmlabApp>) {
    let name = cx.new(|cx| InputState::new(window, cx).placeholder("Golden stone"));
    let enter = cx.subscribe_in(&name, window, |app, _, event: &InputEvent, _, cx| {
        match event {
            InputEvent::PressEnter { .. } => submit(app, cx),
            InputEvent::Change => cx.notify(),
            _ => {}
        }
    });
    name.update(cx, |input, cx| input.focus(window, cx));
    app.rail.saving = Some(Form { name, chosen: LOOK_GROUPS.into_iter().collect(), _enter: enter });
    cx.notify();
}

fn close(app: &mut FilmlabApp, cx: &mut Context<FilmlabApp>) {
    app.rail.saving = None;
    cx.notify();
}

fn submit(app: &mut FilmlabApp, cx: &mut Context<FilmlabApp>) {
    let Some(form) = &app.rail.saving else { return };
    let name = form.name.read(cx).value().to_string();
    if name.trim().is_empty() {
        return;
    }
    let groups: Vec<String> = LOOK_GROUPS.iter().chain(PER_PHOTO_GROUPS.iter()).filter(|g| form.chosen.contains(*g)).map(|g| (*g).to_string()).collect();
    let develop = app.develop.read(cx);
    let (Some(settings), film) = (develop.settings.clone(), develop.film.as_ref().map(|f| f.to_value())) else { return };
    if let Some(profile) = store::build(&name, &settings, groups, film) {
        app.rail.profiles.insert(0, profile);
        store::write(&app.rail.profiles);
    }
    close(app, cx);
}

fn checkbox_row(group: &'static str, on: bool, me: &Entity<FilmlabApp>) -> AnyElement {
    let me = me.clone();
    div()
        .id(SharedString::from(format!("save-group-{group}")))
        .min_h(px(32.))
        .w_full()
        .px(px(10.))
        .py(px(5.))
        .flex()
        .items_center()
        .justify_between()
        .gap(px(8.))
        .rounded(px(theme::RADIUS))
        .bg(theme::surface())
        .hover(|s| s.bg(theme::surface_hover()))
        .cursor_pointer()
        .text_size(px(13.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(if on { theme::primary() } else { theme::label() })
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(move |_, _, cx| {
            me.update(cx, |app, cx| {
                if let Some(form) = &mut app.rail.saving
                    && !form.chosen.remove(group)
                {
                    form.chosen.insert(group);
                }
                cx.notify();
            })
        })
        .child(div().min_w_0().child(store::group_label(group)))
        .child(
            div()
                .size(px(16.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(4.))
                .bg(if on { theme::fill_active() } else { theme::fill() })
                .when(on, |d| d.child(icon::icon("tick02", px(12.)))),
        )
        .into_any_element()
}

fn group_list(title: &'static str, hint: &'static str, groups: &[&'static str], form: &Form, me: &Entity<FilmlabApp>) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap(px(4.))
        .pt(px(10.))
        .child(
            div()
                .px(px(4.))
                .pb(px(2.))
                .flex()
                .items_baseline()
                .justify_between()
                .child(div().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).text_color(theme::label()).child(title))
                .child(div().text_size(px(11.)).text_color(theme::tertiary()).child(hint)),
        )
        .children(groups.iter().map(|g| checkbox_row(g, form.chosen.contains(g), me)))
        .into_any_element()
}

fn button(id: &'static str, label: &'static str, enabled: bool, on_click: impl Fn(&mut Window, &mut App) + 'static) -> impl IntoElement {
    div()
        .id(id)
        .h(px(theme::ROW))
        .flex_1()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(theme::RADIUS))
        .bg(theme::fill())
        .text_size(px(13.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(theme::primary())
        .when(enabled, |d| d.cursor_pointer().hover(|s| s.bg(theme::fill_active())).on_click(move |_, window, cx| on_click(window, cx)))
        .when(!enabled, |d| d.opacity(0.4))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(label)
}

pub fn form(app: &mut FilmlabApp, cx: &mut Context<FilmlabApp>) -> AnyElement {
    let Some(form) = &app.rail.saving else { return Empty.into_any_element() };
    let me = cx.entity();
    let can_save = !form.name.read(cx).value().trim().is_empty();
    let (cancel, save) = (me.clone(), me.clone());
    div()
        .size_full()
        .flex()
        .flex_col()
        .px(px(12.))
        .py(px(12.))
        .gap(px(8.))
        .child(div().h(px(28.)).flex().items_center().text_size(px(15.)).font_weight(FontWeight::SEMIBOLD).text_color(theme::focus()).child("Save profile"))
        .child(
            div()
                .h(px(theme::ROW))
                .flex_none()
                .px(px(10.))
                .flex()
                .items_center()
                .gap(px(10.))
                .rounded(px(theme::RADIUS))
                .bg(theme::fill_active())
                .text_size(px(13.))
                .font_weight(FontWeight::MEDIUM)
                .child(div().text_color(theme::label()).child("Name"))
                .child(div().min_w_0().flex_1().child(Input::new(&form.name).appearance(false).bordered(false).focus_bordered(false).text_color(theme::primary()))),
        )
        .child(
            div()
                .id("save-groups")
                .min_h_0()
                .flex_1()
                .overflow_y_scroll()
                .child(group_list("Look", "Saved in the profile", &LOOK_GROUPS, form, &me))
                .child(group_list("Per photo", "Left alone when applied", &PER_PHOTO_GROUPS, form, &me)),
        )
        .child(
            div()
                .flex_none()
                .flex()
                .gap(px(theme::GAP))
                .child(button("save-cancel", "Cancel", true, move |_, cx| cancel.update(cx, close)))
                .child(button("save-confirm", "Save profile", can_save, move |_, cx| save.update(cx, submit))),
        )
        .into_any_element()
}
