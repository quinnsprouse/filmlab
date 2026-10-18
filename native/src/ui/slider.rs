use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::theme;

type OnChange = Rc<dyn Fn(f64, &mut Window, &mut App)>;

#[derive(Clone)]
struct DragValue(ElementId);

pub struct Nothing;

impl Render for Nothing {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

#[derive(IntoElement)]
pub struct ParamSlider {
    id: ElementId,
    label: SharedString,
    value: f64,
    min: f64,
    max: f64,
    step: f64,
    fallback: f64,
    decimals: usize,
    shown: Option<SharedString>,
    disabled: bool,
    on_change: Option<OnChange>,
}

impl ParamSlider {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>, value: f64, min: f64, max: f64) -> Self {
        ParamSlider { id: id.into(), label: label.into(), value, min, max, step: 1.0, fallback: 0.0, decimals: 0, shown: None, disabled: false, on_change: None }
    }

    pub fn step(mut self, step: f64) -> Self {
        self.step = step;
        self.decimals = if step >= 1.0 { 0 } else { (-step.log10()).ceil().max(0.0) as usize };
        self
    }

    pub fn decimals(mut self, decimals: usize) -> Self {
        self.decimals = decimals;
        self
    }

    pub fn fallback(mut self, fallback: f64) -> Self {
        self.fallback = fallback;
        self
    }

    pub fn shown(mut self, text: impl Into<SharedString>) -> Self {
        self.shown = Some(text.into());
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_change(mut self, f: impl Fn(f64, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }

    fn readout(&self, value: f64) -> SharedString {
        if let Some(text) = &self.shown {
            return text.clone();
        }
        let v = if value.abs() < 0.5 * 10f64.powi(-(self.decimals as i32)) { 0.0 } else { value };
        let s = format!("{v:.prec$}", prec = self.decimals);
        if self.min < 0.0 && v > 0.0 { format!("+{s}").into() } else { s.into() }
    }
}

fn snap(value: f64, min: f64, max: f64, step: f64) -> f64 {
    let stepped = if step > 0.0 { min + ((value - min) / step).round() * step } else { value };
    ((stepped * 1e6).round() / 1e6).clamp(min, max)
}

fn value_at(x: Pixels, bounds: Bounds<Pixels>, min: f64, max: f64, step: f64) -> f64 {
    let width = f64::from(f32::from(bounds.size.width)).max(1.0);
    let t = (f64::from(f32::from(x - bounds.left())) / width).clamp(0.0, 1.0);
    snap(min + t * (max - min), min, max, step)
}

#[derive(Default)]
struct Interaction {
    bounds: Bounds<Pixels>,
    hovered: bool,
    dragging: bool,
}

// why: DialKit's slider is the whole row: label and value sit inside a soft bar whose fill and thin handle
// mark the value; on Liquid Glass the bar is a translucent white so the glass shows through
impl RenderOnce for ParamSlider {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let (min, max, step) = (self.min, self.max, self.step);
        let value = self.value.clamp(min, max);
        let at = ((value - min) / (max - min).max(f64::EPSILON)) as f32;
        let text = self.readout(value);
        let state = window.use_keyed_state(ElementId::NamedChild(Arc::new(self.id.clone()), "slider".into()), cx, |_, _| Interaction::default());
        let (hovered, dragging) = {
            let s = state.read(cx);
            (s.hovered, s.dragging)
        };
        let active = !self.disabled && self.on_change.is_some();
        let lit = active && (hovered || dragging);
        let centre = (min < 0.0 && max > 0.0).then(|| (-min / (max - min)) as f32);
        let (record, hover, press, release, end) = (state.clone(), state.clone(), state.clone(), state.clone(), state.clone());
        let (on_press, on_drag, on_reset) = (self.on_change.clone(), self.on_change.clone(), self.on_change.clone());
        let fallback = self.fallback;
        let drag_id = self.id.clone();
        div()
            .id(self.id.clone())
            .relative()
            .h(px(theme::ROW))
            .w_full()
            .rounded(px(theme::RADIUS))
            .overflow_hidden()
            .bg(theme::surface())
            .when(self.disabled, |d| d.opacity(0.4))
            .child(canvas(move |b, _, cx| record.update(cx, |s, _| s.bounds = b), |_, _, _, _| {}).absolute().size_full())
            // why: GPUI clips children to the rectangle, not the rounded corners, so the fill rounds its own
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left_0()
                    .w(relative(at))
                    .rounded_l(px(theme::RADIUS))
                    .when(at > 0.98, |d| d.rounded_r(px(theme::RADIUS)))
                    .bg(if lit { theme::fill_active() } else { theme::fill() }),
            )
            .when_some(centre.filter(|_| lit), |d, c| d.child(div().absolute().top(px(14.)).h(px(8.)).w(px(1.)).left(relative(c)).rounded_full().bg(theme::hashmark())))
            .child(div().absolute().top(px(8.)).h(px(20.)).w(px(3.)).ml(px(-1.5)).left(relative(at.clamp(0.012, 0.988))).rounded_full().bg(theme::handle()))
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .text_size(px(13.))
                    .font_weight(FontWeight::MEDIUM)
                    .child(div().text_color(theme::label()).child(self.label))
                    .child(div().pr(px(2.)).font_features(theme::tabular()).text_color(if lit { theme::focus() } else { theme::label() }).child(text)),
            )
            .when(active, |d| {
                d.cursor_ew_resize()
                    .on_hover(move |over, _, cx| {
                        hover.update(cx, |s, cx| {
                            s.hovered = *over;
                            cx.notify();
                        })
                    })
                    .on_mouse_down(MouseButton::Left, move |event, window, cx| {
                        cx.stop_propagation();
                        if event.click_count >= 2 {
                            if let Some(f) = &on_reset {
                                f(fallback, window, cx);
                            }
                            return;
                        }
                        let b = press.read(cx).bounds;
                        press.update(cx, |s, cx| {
                            s.dragging = true;
                            cx.notify();
                        });
                        if let Some(f) = &on_press {
                            f(value_at(event.position.x, b, min, max, step), window, cx);
                        }
                    })
                    .on_mouse_up(MouseButton::Left, move |_, _, cx| {
                        release.update(cx, |s, cx| {
                            s.dragging = false;
                            cx.notify();
                        })
                    })
                    .on_mouse_up_out(MouseButton::Left, move |_, _, cx| {
                        end.update(cx, |s, cx| {
                            s.dragging = false;
                            cx.notify();
                        })
                    })
                    .on_drag(DragValue(drag_id.clone()), |_, _, _, cx| cx.new(|_| Nothing))
                    .on_drag_move(move |event: &DragMoveEvent<DragValue>, window, cx| {
                        if event.drag(cx).0 != drag_id {
                            return;
                        }
                        if let Some(f) = &on_drag {
                            f(value_at(event.event.position.x, event.bounds, min, max, step), window, cx);
                        }
                    })
            })
    }
}
