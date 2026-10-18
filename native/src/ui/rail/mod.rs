mod save;
pub(crate) mod store;

use std::time::Duration;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::app::FilmlabApp;
use super::slider::ParamSlider;
use super::{icon, theme};
use crate::film::FilmSettings;
use store::Profile;

const CONFIRM_WINDOW: Duration = Duration::from_secs(3);

pub struct State {
    profiles: Vec<Profile>,
    loaded: bool,
    amount: f64,
    confirming: Option<String>,
    saving: Option<save::Form>,
}

impl Default for State {
    fn default() -> Self {
        State { profiles: Vec::new(), loaded: false, amount: 100.0, confirming: None, saving: None }
    }
}

pub fn rail(app: &mut FilmlabApp, _window: &mut Window, cx: &mut Context<FilmlabApp>) -> AnyElement {
    if !app.rail.loaded {
        app.rail.profiles = store::load();
        app.rail.loaded = true;
    }
    if app.rail.saving.is_some() {
        return save::form(app, cx);
    }
    div()
        .size_full()
        .flex()
        .flex_col()
        .gap(px(14.))
        .px(px(12.))
        .py(px(12.))
        .child(profiles_section(app, cx))
        .child(history_section(app, cx))
        .into_any_element()
}

fn heading(title: &'static str, count: String) -> Div {
    div()
        .h(px(28.))
        .flex_none()
        .px(px(4.))
        .flex()
        .items_center()
        .justify_between()
        .child(div().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).text_color(theme::label()).child(title))
        .child(div().text_size(px(12.)).font_features(theme::tabular()).text_color(theme::tertiary()).child(count))
}

fn apply(app: &mut FilmlabApp, profile: &Profile, percent: f64, cx: &mut Context<FilmlabApp>) {
    let film = profile.film.as_ref().and_then(FilmSettings::of);
    let label = format!("Profile {}", profile.name);
    app.develop.update(cx, |develop, cx| develop.apply_profile(&profile.id, &profile.partial, percent / 100.0, &label, film, cx));
}

fn profiles_section(app: &FilmlabApp, cx: &mut Context<FilmlabApp>) -> AnyElement {
    let develop = app.develop.read(cx);
    let photo = develop.photo_id.clone();
    let applied = develop.applied.as_ref().filter(|a| a.photo_id == photo).map(|a| a.profile_id.clone());
    let thumbnail = photo.and_then(|id| app.library.read(cx).record(&id).and_then(|r| r.thumbnail.clone()));
    let me = cx.entity();
    let profiles = &app.rail.profiles;
    let rows: Vec<AnyElement> = profiles.iter().map(|p| profile_row(app, p, applied.as_deref() == Some(p.id.as_str()), thumbnail.clone(), &me)).collect();
    let amount = app.rail.amount;
    div()
        .flex_none()
        .flex()
        .flex_col()
        .gap(px(theme::GAP))
        .child(heading("Profiles", format!("{} saved", profiles.len())))
        .when(profiles.is_empty(), |d| {
            d.child(
                div()
                    .px(px(4.))
                    .pb(px(4.))
                    .text_size(px(12.))
                    .text_color(theme::tertiary())
                    .child(div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).text_color(theme::label()).child("No profiles yet"))
                    .child(div().pt(px(2.)).child("A profile saves a look, such as tone, color and grain, to reuse on other photos.")),
            )
        })
        .child(div().id("profile-rows").max_h(px(300.)).overflow_y_scroll().flex().flex_col().gap(px(2.)).children(rows))
        .when(!profiles.is_empty(), |d| {
            let me = me.clone();
            d.child(
                ParamSlider::new("profile-amount", "Amount", amount, 0.0, 200.0)
                    .fallback(100.0)
                    .shown(format!("{amount:.0}%"))
                    .on_change(move |value, _, cx| {
                        me.update(cx, |app, cx| {
                            app.rail.amount = value;
                            let active = app.develop.read(cx).applied.as_ref().map(|a| a.profile_id.clone());
                            if let Some(profile) = active.and_then(|id| app.rail.profiles.iter().find(|p| p.id == id).cloned()) {
                                apply(app, &profile, value, cx);
                            }
                            cx.notify();
                        })
                    }),
            )
        })
        .child(
            div()
                .id("save-profile")
                .h(px(theme::ROW))
                .rounded(px(theme::RADIUS))
                .bg(theme::surface())
                .hover(|s| s.bg(theme::surface_hover()))
                .cursor_pointer()
                .flex()
                .items_center()
                .justify_center()
                .gap(px(6.))
                .text_size(px(13.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme::primary())
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click({
                    let me = me.clone();
                    move |_, window, cx| me.update(cx, |app, cx| save::open(app, window, cx))
                })
                .child(icon::icon("add01", px(14.)))
                .child("Save as profile"),
        )
        .into_any_element()
}

fn profile_row(app: &FilmlabApp, profile: &Profile, active: bool, thumbnail: Option<std::sync::Arc<RenderImage>>, me: &Entity<FilmlabApp>) -> AnyElement {
    let confirming = app.rail.confirming.as_deref() == Some(profile.id.as_str());
    let (pick, pid) = (profile.clone(), profile.id.clone());
    let (apply_me, delete_me) = (me.clone(), me.clone());
    let group = SharedString::from(format!("profile-{}", profile.id));
    div()
        .relative()
        .group(group.clone())
        .child(
            div()
                .id(SharedString::from(format!("profile-pick-{}", profile.id)))
                .h(px(52.))
                .w_full()
                .pl(px(6.))
                .pr(px(8.))
                .flex()
                .items_center()
                .gap(px(8.))
                .rounded(px(theme::RADIUS))
                .cursor_pointer()
                .when(active, |d| d.bg(theme::selected()))
                .when(!active, |d| d.hover(|s| s.bg(theme::hover())))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(move |_, _, cx| {
                    let pick = pick.clone();
                    apply_me.update(cx, |app, cx| {
                        let amount = app.rail.amount;
                        apply(app, &pick, amount, cx);
                    })
                })
                .child(
                    div().w(px(60.)).h(px(40.)).flex_none().rounded(px(4.)).overflow_hidden().bg(theme::track()).when_some(thumbnail, |d, image| d.child(img(image).size_full().object_fit(ObjectFit::Cover))),
                )
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .child(div().truncate().text_size(px(13.)).font_weight(FontWeight::MEDIUM).text_color(if active { theme::primary() } else { theme::label() }).child(profile.name.clone()))
                        .child(div().text_size(px(12.)).font_features(theme::tabular()).text_color(theme::tertiary()).child(format!("{} groups", profile.groups.len()))),
                )
                .when(active && !confirming, |d| d.child(div().flex_none().child(icon::icon("tick02", px(14.))))),
        )
        .child(
            div()
                .id(SharedString::from(format!("profile-delete-{}", profile.id)))
                .absolute()
                .top(px(14.))
                .right(px(6.))
                .h(px(24.))
                .min_w(px(24.))
                .px(if confirming { px(8.) } else { px(0.) })
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(6.))
                .cursor_pointer()
                .text_size(px(12.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme::label())
                .when(confirming, |d| d.bg(theme::fill_active()).text_color(theme::danger()))
                .when(!confirming, |d| d.opacity(0.).group_hover(group, |s| s.opacity(1.)).hover(|s| s.bg(theme::fill())))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(move |_, _, cx| {
                    let pid = pid.clone();
                    delete_me.update(cx, |app, cx| delete(app, pid, cx))
                })
                .child(if confirming { div().child("Delete").into_any_element() } else { icon::icon("delete02", px(14.)).into_any_element() }),
        )
        .into_any_element()
}

fn delete(app: &mut FilmlabApp, id: String, cx: &mut Context<FilmlabApp>) {
    if app.rail.confirming.as_deref() == Some(id.as_str()) {
        app.rail.confirming = None;
        app.rail.profiles.retain(|p| p.id != id);
        store::write(&app.rail.profiles);
    } else {
        app.rail.confirming = Some(id.clone());
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(CONFIRM_WINDOW).await;
            let _ = this.update(cx, |app, cx| {
                if app.rail.confirming.as_deref() == Some(id.as_str()) {
                    app.rail.confirming = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }
    cx.notify();
}

/// The few history steps worth showing: the newest five, or five around the current step after an undo (with up to
/// two redo steps above it).
fn shown_steps(cursor: usize, count: usize) -> std::ops::Range<usize> {
    const SHOWN: usize = 5;
    let end = (cursor + 3).min(count).max(SHOWN.min(count));
    end.saturating_sub(SHOWN)..end
}

fn history_section(app: &FilmlabApp, cx: &mut Context<FilmlabApp>) -> AnyElement {
    let develop = app.develop.read(cx);
    let cursor = develop.cursor;
    let count = develop.history.len();
    let me = cx.entity();
    let shown = shown_steps(cursor, count);
    let rows: Vec<AnyElement> = develop.history[shown.clone()]
        .iter()
        .zip(shown)
        .rev()
        .map(|(step, index)| {
            let me = me.clone();
            div()
                .id(SharedString::from(format!("history-{index}")))
                .flex_none()
                .h(px(32.))
                .w_full()
                .px(px(10.))
                .flex()
                .items_center()
                .rounded(px(theme::RADIUS))
                .cursor_pointer()
                .text_size(px(13.))
                .font_weight(FontWeight::MEDIUM)
                .when(index == cursor, |d| d.bg(theme::selected()).text_color(theme::primary()))
                .when(index < cursor, |d| d.text_color(theme::label()).hover(|s| s.bg(theme::hover())))
                .when(index > cursor, |d| d.text_color(theme::tertiary()).opacity(0.6).hover(|s| s.bg(theme::hover())))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(move |_, _, cx| {
                    me.update(cx, |app, cx| {
                        app.develop.update(cx, |develop, cx| develop.go_to(index, cx));
                        cx.notify();
                    })
                })
                .gap(px(7.))
                .when(step.key.starts_with("agent."), |d| d.child(div().flex_none().size(px(5.)).rounded_full().bg(theme::accent())))
                .child(div().min_w_0().truncate().child(step.label.clone()))
                .into_any_element()
        })
        .collect();
    div()
        .min_h_0()
        .flex_1()
        .flex()
        .flex_col()
        .justify_end()
        .child(heading("History", format!("{count} {}", if count == 1 { "step" } else { "steps" })))
        .child(div().id("history-rows").min_h_0().overflow_y_scroll().flex().flex_col().gap(px(2.)).children(rows))
        .into_any_element()
}
