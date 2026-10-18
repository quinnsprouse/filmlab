use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::theme;
use crate::menu::{self, Entry};

fn row(id: impl Into<ElementId>) -> Stateful<Div> {
    div().id(id).h(px(theme::ROW)).w_full().px(px(10.)).flex().items_center().justify_between().gap(px(8.)).rounded(px(theme::RADIUS)).bg(theme::surface()).text_size(px(13.)).font_weight(FontWeight::MEDIUM)
}

pub fn header(title: impl Into<SharedString>) -> impl IntoElement {
    div().h(px(28.)).flex().items_center().text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).text_color(theme::tertiary()).child(title.into())
}

#[derive(IntoElement)]
pub struct Segmented {
    id: ElementId,
    label: Option<SharedString>,
    options: Vec<(SharedString, bool)>,
    on_pick: Rc<dyn Fn(usize, &mut Window, &mut App)>,
}

impl Segmented {
    pub fn new(id: impl Into<ElementId>, options: Vec<(SharedString, bool)>, on_pick: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
        Segmented { id: id.into(), label: None, options, on_pick: Rc::new(on_pick) }
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }
}

impl RenderOnce for Segmented {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let id = self.id.clone();
        let buttons = self.options.into_iter().enumerate().map(move |(i, (label, active))| {
            let pick = self.on_pick.clone();
            div()
                .id(ElementId::NamedChild(Arc::new(id.clone()), format!("{i}").into()))
                .px(px(8.))
                .py(px(5.))
                .rounded(px(6.))
                .text_color(if active { theme::primary() } else { theme::label() })
                .when(active, |d| d.bg(theme::fill()))
                .when(!active, |d| d.cursor_pointer().hover(|s| s.text_color(theme::primary())))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(move |_, window, cx| pick(i, window, cx))
                .child(label)
        });
        row(ElementId::NamedChild(Arc::new(self.id.clone()), "row".into()))
            .pr(px(3.))
            .child(div().text_color(theme::label()).child(self.label.unwrap_or_default()))
            .child(div().flex().items_center().gap(px(1.)).children(buttons))
    }
}

#[derive(IntoElement)]
pub struct Toggle {
    id: ElementId,
    label: SharedString,
    on: bool,
    on_change: Rc<dyn Fn(bool, &mut Window, &mut App)>,
}

impl Toggle {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>, on: bool, on_change: impl Fn(bool, &mut Window, &mut App) + 'static) -> Self {
        Toggle { id: id.into(), label: label.into(), on, on_change: Rc::new(on_change) }
    }
}

impl RenderOnce for Toggle {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let (on, change) = (self.on, self.on_change.clone());
        row(self.id)
            .cursor_pointer()
            .hover(|s| s.bg(theme::surface_hover()))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(move |_, window, cx| change(!on, window, cx))
            .child(div().text_color(if on { theme::primary() } else { theme::label() }).child(self.label))
            .child(
                div()
                    .relative()
                    .w(px(36.))
                    .h(px(20.))
                    .rounded(px(10.))
                    .bg(if on { theme::fill_active() } else { theme::fill() })
                    .child(div().absolute().top(px(2.)).left(px(if on { 18. } else { 2. })).size(px(16.)).rounded(px(8.)).bg(theme::thumb())),
            )
    }
}

pub struct PickerItem {
    pub label: String,
    pub checked: bool,
    pub header: bool,
}

#[derive(IntoElement)]
pub struct Picker {
    id: ElementId,
    label: SharedString,
    value: SharedString,
    items: Rc<dyn Fn() -> Vec<PickerItem>>,
    on_pick: Rc<dyn Fn(usize, &mut Window, &mut App)>,
}

impl Picker {
    pub fn new(
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        value: impl Into<SharedString>,
        items: impl Fn() -> Vec<PickerItem> + 'static,
        on_pick: impl Fn(usize, &mut Window, &mut App) + 'static,
    ) -> Self {
        Picker { id: id.into(), label: label.into(), value: value.into(), items: Rc::new(items), on_pick: Rc::new(on_pick) }
    }
}

impl RenderOnce for Picker {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let bounds = window.use_keyed_state(ElementId::NamedChild(Arc::new(self.id.clone()), "bounds".into()), cx, |_, _| Bounds::<Pixels>::default());
        let record = bounds.clone();
        let (items, pick) = (self.items.clone(), self.on_pick.clone());
        row(self.id)
            .relative()
            .cursor_pointer()
            .hover(|s| s.bg(theme::surface_hover()))
            .child(canvas(move |b, _, cx| record.update(cx, |s, _| *s = b), |_, _, _, _| {}).absolute().size_full())
            .child(div().flex_none().text_color(theme::label()).child(self.label))
            .child(
                div()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(div().min_w_0().truncate().text_color(theme::primary()).child(self.value))
                    .child(div().flex_none().opacity(0.7).child(super::icon::icon("arrow-down01", px(13.)))),
            )
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(move |_, window, cx| {
                let anchor = *bounds.read(cx);
                let listed = items();
                let mut entries = Vec::new();
                let mut map = Vec::new();
                for (i, item) in listed.iter().enumerate() {
                    if item.header {
                        entries.push(Entry::Header(item.label.clone()));
                    } else {
                        entries.push(Entry::Item { label: item.label.clone(), checked: item.checked });
                        map.push(i);
                    }
                }
                let rx = menu::popup(window, anchor, entries);
                let pick = pick.clone();
                let handle = window.window_handle();
                cx.spawn(async move |cx| {
                    if let Ok(Some(k)) = rx.await
                        && let Some(&i) = map.get(k)
                    {
                        let _ = cx.update_window(handle, |_, window, cx| pick(i, window, cx));
                    }
                })
                .detach();
            })
    }
}
