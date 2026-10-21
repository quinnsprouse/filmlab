use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::color_rail::{self, ColorRail};
use super::slider::{DragValue, Nothing, amber, caption, fill_of, handle, handle_of, interaction, measured, tracked, value_at};
use super::theme;

type OnRange = Rc<dyn Fn(f64, f64, &mut Window, &mut App)>;

#[derive(Clone, Copy)]
pub struct RangeEnd {
    pub value: f64,
    pub min: f64,
    pub max: f64,
    pub fallback: f64,
}

#[derive(Clone, Copy)]
enum Side {
    Low,
    High,
}

#[derive(Clone, Copy)]
struct Ends {
    low: RangeEnd,
    high: RangeEnd,
}

impl Ends {
    fn span(self) -> (f64, f64) {
        (self.low.min, self.high.max)
    }

    fn at(self, value: f64) -> f32 {
        let (min, max) = self.span();
        ((value - min) / (max - min).max(f64::EPSILON)) as f32
    }

    fn grab(self, value: f64) -> Option<Side> {
        let (to_low, to_high) = ((value - self.low.value).abs(), (value - self.high.value).abs());
        if to_low < to_high || value < self.low.value {
            Some(Side::Low)
        } else if to_high < to_low || value > self.high.value {
            Some(Side::High)
        } else {
            None
        }
    }

    fn toward(self, value: f64) -> Side {
        if value > (self.low.value + self.high.value) / 2.0 { Side::High } else { Side::Low }
    }

    fn moved(self, side: Side, value: f64) -> (f64, f64) {
        let (low, high) = (self.low, self.high);
        match side {
            Side::Low => (value.clamp(low.min, low.max).min(high.value), high.value),
            Side::High => (low.value, value.clamp(high.min, high.max).max(low.value)),
        }
    }
}

#[derive(IntoElement)]
pub struct RangeSlider {
    id: ElementId,
    label: SharedString,
    ends: Ends,
    rail: ColorRail,
    agent: bool,
    on_change: Option<OnRange>,
}

impl RangeSlider {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>, low: RangeEnd, high: RangeEnd, rail: ColorRail) -> Self {
        RangeSlider { id: id.into(), label: label.into(), ends: Ends { low, high }, rail, agent: false, on_change: None }
    }

    pub fn agent(mut self, agent: bool) -> Self {
        self.agent = agent;
        self
    }

    pub fn on_change(mut self, f: impl Fn(f64, f64, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }
}

impl RenderOnce for RangeSlider {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let ends = self.ends;
        let (min, max) = ends.span();
        let (low, high) = (ends.at(ends.low.value), ends.at(ends.high.value));
        let state = interaction(&self.id, window, cx);
        let side = window.use_keyed_state(ElementId::NamedChild(Arc::new(self.id.clone()), "side".into()), cx, |_, _| None::<Side>);
        let active = self.on_change.is_some();
        let lit = state.read(cx).lit(active);
        let amber = amber(&state, self.agent, window, cx);
        let text: SharedString = format!("{:.0}–{:.0}", ends.low.value, ends.high.value).into();
        let (press, grabbed, dragged) = (state.clone(), side.clone(), side);
        let (on_press, on_drag, on_reset) = (self.on_change.clone(), self.on_change.clone(), self.on_change.clone());
        let drag_id = self.id.clone();
        div()
            .id(self.id.clone())
            .relative()
            .h(px(theme::ROW))
            .w_full()
            .rounded(px(theme::RADIUS))
            .overflow_hidden()
            .bg(theme::surface())
            .child(measured(&state))
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(relative(low))
                    .w(relative(high - low))
                    .when(low < 0.02, |d| d.rounded_l(px(theme::RADIUS)))
                    .when(high > 0.98, |d| d.rounded_r(px(theme::RADIUS)))
                    .bg(fill_of(lit, amber)),
            )
            .child(color_rail::strip_within(self.rail, (low, high)))
            .child(handle(low, handle_of(amber)))
            .child(handle(high, handle_of(amber)))
            .child(caption(self.label, text, lit))
            .when(active, |d| {
                tracked(d, &state)
                    .cursor_ew_resize()
                    .on_mouse_down(MouseButton::Left, move |event, window, cx| {
                        cx.stop_propagation();
                        if event.click_count >= 2 {
                            if let Some(f) = &on_reset {
                                f(ends.low.fallback, ends.high.fallback, window, cx);
                            }
                            return;
                        }
                        let b = press.read(cx).bounds;
                        press.update(cx, |s, cx| s.set_dragging(true, cx));
                        let value = value_at(event.position.x, b, min, max, 1.0);
                        let side = ends.grab(value);
                        grabbed.update(cx, |s, _| *s = side);
                        if let (Some(side), Some(f)) = (side, &on_press) {
                            let (low, high) = ends.moved(side, value);
                            f(low, high, window, cx);
                        }
                    })
                    .on_drag(DragValue(drag_id.clone()), |_, _, _, cx| cx.new(|_| Nothing))
                    .on_drag_move(move |event: &DragMoveEvent<DragValue>, window, cx| {
                        if event.drag(cx).0 != drag_id {
                            return;
                        }
                        let value = value_at(event.event.position.x, event.bounds, min, max, 1.0);
                        let side = dragged.read(cx).unwrap_or_else(|| ends.toward(value));
                        dragged.update(cx, |s, _| *s = Some(side));
                        if let Some(f) = &on_drag {
                            let (low, high) = ends.moved(side, value);
                            f(low, high, window, cx);
                        }
                    })
            })
    }
}
