use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::menu::{self, Entry};
use crate::ui::app::FilmlabApp;
use crate::ui::theme;

pub fn glyph(name: &str, size: f32, color: Hsla) -> Svg {
    svg().path(format!("icons/{name}.svg")).size(px(size)).flex_none().text_color(color)
}

pub fn body_text(text: impl Into<SharedString>) -> Div {
    div().px(px(2.)).text_size(px(12.)).line_height(px(16.)).text_color(theme::tertiary()).child(text.into())
}

/// A clickable box that never lets the press reach the photo stage underneath.
pub fn pressable(id: impl Into<ElementId>) -> Stateful<Div> {
    div().id(id).cursor_pointer().on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
}

pub fn icon_button(id: impl Into<ElementId>, icon: &str, active: bool, on_click: impl Fn(&mut Window, &mut App) + 'static) -> Stateful<Div> {
    let color = if active { theme::primary() } else { theme::label() };
    pressable(id)
        .size(px(24.))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(6.))
        .when(active, |d| d.bg(theme::fill()))
        .hover(|s| s.bg(theme::surface_hover()))
        .on_click(move |_, window, cx| on_click(window, cx))
        .child(glyph(icon, 14., color))
}

pub fn text_button(id: impl Into<ElementId>, label: impl Into<SharedString>, active: bool, on_click: impl Fn(&mut Window, &mut App) + 'static) -> Stateful<Div> {
    pressable(id)
        .h(px(32.))
        .px(px(10.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(theme::RADIUS))
        .text_size(px(13.))
        .font_weight(FontWeight::MEDIUM)
        .bg(if active { theme::fill() } else { theme::surface() })
        .text_color(if active { theme::primary() } else { theme::label() })
        .hover(|s| s.bg(theme::surface_hover()).text_color(theme::primary()))
        .on_click(move |_, window, cx| on_click(window, cx))
        .child(label.into())
}

fn sub_id(id: &ElementId, name: &'static str) -> ElementId {
    ElementId::NamedChild(Arc::new(id.clone()), name.into())
}

/// A button that opens a native menu below itself and reports the picked entry.
#[derive(IntoElement)]
pub struct MenuButton {
    id: ElementId,
    content: AnyElement,
    entries: Vec<String>,
    wide: bool,
    on_pick: Rc<dyn Fn(usize, &mut Window, &mut App)>,
}

impl MenuButton {
    pub fn new(id: impl Into<ElementId>, content: impl IntoElement, entries: Vec<String>, on_pick: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
        MenuButton { id: id.into(), content: content.into_any_element(), entries, wide: false, on_pick: Rc::new(on_pick) }
    }

    pub fn wide(mut self) -> Self {
        self.wide = true;
        self
    }
}

impl RenderOnce for MenuButton {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let bounds = window.use_keyed_state(sub_id(&self.id, "bounds"), cx, |_, _| Bounds::<Pixels>::default());
        let record = bounds.clone();
        let (entries, pick) = (self.entries, self.on_pick);
        pressable(self.id)
            .relative()
            .flex()
            .items_center()
            .justify_center()
            .when(self.wide, |d| d.h(px(32.)).flex_1().rounded(px(theme::RADIUS)).bg(theme::surface()).text_size(px(13.)).font_weight(FontWeight::MEDIUM))
            .when(!self.wide, |d| d.size(px(24.)).flex_none().rounded(px(6.)).bg(theme::fill()).text_size(px(13.)).font_weight(FontWeight::SEMIBOLD))
            .text_color(theme::label())
            .hover(|s| s.bg(theme::surface_hover()).text_color(theme::primary()))
            .child(canvas(move |b, _, cx| record.update(cx, |s, _| *s = b), |_, _, _, _| {}).absolute().size_full())
            .child(self.content)
            .on_click(move |_, window, cx| {
                let anchor = *bounds.read(cx);
                let listed = entries.iter().map(|label| Entry::Item { label: label.clone(), checked: false }).collect();
                let rx = menu::popup(window, anchor, listed);
                let pick = pick.clone();
                let handle = window.window_handle();
                cx.spawn(async move |cx| {
                    if let Ok(Some(index)) = rx.await {
                        let _ = cx.update_window(handle, |_, window, cx| pick(index, window, cx));
                    }
                })
                .detach();
            })
    }
}

/// A collapsible group; the open state lives in the masks tool state.
pub fn folder(
    id: &'static str,
    title: &'static str,
    default_open: bool,
    nested: bool,
    cx: &mut Context<FilmlabApp>,
    app: &FilmlabApp,
    body: impl FnOnce(&mut Context<FilmlabApp>) -> Vec<AnyElement>,
) -> AnyElement {
    let open = app.tools.masks.is_open(id, default_open);
    let rows = if open { body(cx) } else { Vec::new() };
    let header = pressable(SharedString::from(format!("masks-folder-{id}")))
        .h(px(if nested { 32. } else { 36. }))
        .flex()
        .items_center()
        .justify_between()
        .on_click(cx.listener(move |app, _: &ClickEvent, _, cx| {
            let open = app.tools.masks.is_open(id, default_open);
            app.tools.masks.open.insert(id, !open);
            cx.notify();
        }))
        .child(div().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).text_color(if nested { theme::tertiary() } else { theme::label() }).child(title))
        .child(div().opacity(0.7).child(glyph(if open { "arrow-down01" } else { "arrow-right01" }, 14., theme::primary())));
    div()
        .flex()
        .flex_col()
        .child(header)
        .when(open, |d| d.child(div().flex().flex_col().gap(px(theme::GAP)).pb(px(10.)).children(rows)))
        .into_any_element()
}

/// Wraps a state change as a callback for controls that report a number.
pub fn on_value(cx: &Context<FilmlabApp>, f: impl Fn(&mut FilmlabApp, f64, &mut Context<FilmlabApp>) + 'static) -> impl Fn(f64, &mut Window, &mut App) + 'static {
    let me = cx.entity();
    move |value, _, cx| me.update(cx, |app, cx| f(app, value, cx))
}

pub fn on_flag(cx: &Context<FilmlabApp>, f: impl Fn(&mut FilmlabApp, bool, &mut Context<FilmlabApp>) + 'static) -> impl Fn(bool, &mut Window, &mut App) + 'static {
    let me = cx.entity();
    move |value, _, cx| me.update(cx, |app, cx| f(app, value, cx))
}

pub fn on_index(cx: &Context<FilmlabApp>, f: impl Fn(&mut FilmlabApp, usize, &mut Context<FilmlabApp>) + 'static) -> impl Fn(usize, &mut Window, &mut App) + 'static {
    let me = cx.entity();
    move |index, _, cx| me.update(cx, |app, cx| f(app, index, cx))
}

pub fn on_press(cx: &Context<FilmlabApp>, f: impl Fn(&mut FilmlabApp, &mut Window, &mut Context<FilmlabApp>) + 'static) -> impl Fn(&mut Window, &mut App) + 'static {
    let me = cx.entity();
    move |window, cx| me.update(cx, |app, cx| f(app, window, cx))
}
