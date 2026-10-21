use std::rc::Rc;
use std::time::{Duration, Instant};
use std::sync::Arc;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::color_rail::{self, ColorRail};
use super::theme;

type OnChange = Rc<dyn Fn(f64, &mut Window, &mut App)>;

#[derive(Clone)]
pub(super) struct DragValue(pub(super) ElementId);

pub struct Nothing;

/// Runs when any slider is let go; the app uses it to draw the full frame straight after a drag's drafts.
pub struct OnRelease(pub Rc<dyn Fn(&mut App)>);

impl Global for OnRelease {}

pub(super) fn released(cx: &mut App) {
    if let Some(f) = cx.try_global::<OnRelease>().map(|g| g.0.clone()) {
        f(cx);
    }
}

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
    agent: bool,
    lit: bool,
    rail: Option<ColorRail>,
    on_change: Option<OnChange>,
}

impl ParamSlider {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>, value: f64, min: f64, max: f64) -> Self {
        ParamSlider { id: id.into(), label: label.into(), value, min, max, step: 1.0, fallback: 0.0, decimals: 0, shown: None, disabled: false, agent: false, lit: false, rail: None, on_change: None }
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

    /// An agent set this value and nobody has changed it since: the fill and handle turn amber.
    pub fn agent(mut self, agent: bool) -> Self {
        self.agent = agent;
        self
    }

    pub fn lit(mut self, lit: bool) -> Self {
        self.lit = lit;
        self
    }

    pub fn rail(mut self, rail: ColorRail) -> Self {
        self.rail = Some(rail);
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

pub(super) fn value_at(x: Pixels, bounds: Bounds<Pixels>, min: f64, max: f64, step: f64) -> f64 {
    let width = f64::from(f32::from(bounds.size.width)).max(1.0);
    let t = (f64::from(f32::from(x - bounds.left())) / width).clamp(0.0, 1.0);
    snap(min + t * (max - min), min, max, step)
}

/// How long a slider takes to turn amber when an agent sets it, or back to gray when you take it over.
const MORPH: Duration = Duration::from_millis(240);

#[derive(Default)]
pub(super) struct Interaction {
    pub(super) bounds: Bounds<Pixels>,
    hovered: bool,
    dragging: bool,
    /// The amber state last drawn (none before the first paint), and the morph toward it: where it started and when.
    agent: Option<bool>,
    from: f32,
    goal: f32,
    since: Option<Instant>,
}

impl Interaction {
    /// How amber the slider is now (0 gray, 1 amber) and whether it's still changing.
    fn amber(&self) -> (f32, bool) {
        let Some(since) = self.since else { return (self.goal, false) };
        let p = (since.elapsed().as_secs_f32() / MORPH.as_secs_f32()).min(1.0);
        (self.from + (self.goal - self.from) * super::motion::emphasized()(p), p < 1.0)
    }

    // why: the first paint shows the state as it is; later changes morph from wherever the last one had got to, so a
    // quick take-over and agent edit never jump
    fn morph(&mut self, agent: bool, calm: bool) -> (f32, bool) {
        let goal = if agent { 1.0 } else { 0.0 };
        match self.agent {
            None => {
                (self.from, self.goal, self.since) = (goal, goal, None);
            }
            Some(was) if was != agent => {
                self.from = self.amber().0;
                self.goal = goal;
                self.since = (!calm).then(Instant::now);
            }
            _ => {}
        }
        self.agent = Some(agent);
        self.amber()
    }

    pub(super) fn lit(&self, active: bool) -> bool {
        active && (self.hovered || self.dragging)
    }

    pub(super) fn set_dragging(&mut self, dragging: bool, cx: &mut Context<Self>) {
        self.dragging = dragging;
        cx.notify();
    }
}

pub(super) fn interaction(id: &ElementId, window: &mut Window, cx: &mut App) -> Entity<Interaction> {
    window.use_keyed_state(ElementId::NamedChild(Arc::new(id.clone()), "slider".into()), cx, |_, _| Interaction::default())
}

pub(super) fn amber(state: &Entity<Interaction>, agent: bool, window: &mut Window, cx: &mut App) -> f32 {
    let (amber, morphing) = state.update(cx, |s, _| s.morph(agent, super::motion::reduced()));
    if morphing {
        window.request_animation_frame();
    }
    amber
}

pub(super) fn fill_of(lit: bool, amber: f32) -> Hsla {
    let gray = if lit { theme::fill_active() } else { theme::fill() };
    blend(gray, if lit { rgba(0xFFB34033).into() } else { rgba(0xFFB34024).into() }, amber)
}

pub(super) fn handle_of(amber: f32) -> Hsla {
    blend(theme::handle(), theme::accent(), amber)
}

pub(super) fn handle(at: f32, color: Hsla) -> Div {
    div().absolute().top(px(8.)).h(px(20.)).w(px(3.)).ml(px(-1.5)).left(relative(at.clamp(0.012, 0.988))).rounded_full().bg(color)
}

pub(super) fn caption(label: SharedString, value: SharedString, lit: bool) -> Div {
    div()
        .absolute()
        .inset_0()
        .px(px(10.))
        .flex()
        .items_center()
        .justify_between()
        .text_size(px(13.))
        .font_weight(FontWeight::MEDIUM)
        .child(div().text_color(theme::label()).child(label))
        .child(div().pr(px(2.)).font_features(theme::tabular()).text_color(if lit { theme::focus() } else { theme::label() }).child(value))
}

pub(super) fn measured(state: &Entity<Interaction>) -> Canvas<()> {
    let record = state.clone();
    canvas(move |b, _, cx| record.update(cx, |s, _| s.bounds = b), |_, _, _, _| {}).absolute().size_full()
}

pub(super) fn tracked(row: Stateful<Div>, state: &Entity<Interaction>) -> Stateful<Div> {
    let (hover, release, end) = (state.clone(), state.clone(), state.clone());
    row.on_hover(move |over, _, cx| {
        hover.update(cx, |s, cx| {
            s.hovered = *over;
            cx.notify();
        })
    })
    .on_mouse_up(MouseButton::Left, move |_, _, cx| {
        release.update(cx, |s, cx| s.set_dragging(false, cx));
        released(cx);
    })
    .on_mouse_up_out(MouseButton::Left, move |_, _, cx| {
        end.update(cx, |s, cx| s.set_dragging(false, cx));
        released(cx);
    })
}

fn blend(a: Hsla, b: Hsla, t: f32) -> Hsla {
    let (a, b, t) = (Rgba::from(a), Rgba::from(b), t.clamp(0.0, 1.0));
    Rgba { r: a.r + (b.r - a.r) * t, g: a.g + (b.g - a.g) * t, b: a.b + (b.b - a.b) * t, a: a.a + (b.a - a.a) * t }.into()
}

// why: DialKit's slider is the whole row: label and value sit inside a soft bar whose fill and thin handle
// mark the value; on Liquid Glass the bar is a translucent white so the glass shows through
impl RenderOnce for ParamSlider {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let (min, max, step) = (self.min, self.max, self.step);
        let value = self.value.clamp(min, max);
        let at = ((value - min) / (max - min).max(f64::EPSILON)) as f32;
        let text = self.readout(value);
        let state = interaction(&self.id, window, cx);
        let active = !self.disabled && self.on_change.is_some();
        let lit = state.read(cx).lit(active) || (active && self.lit);
        let amber = amber(&state, self.agent, window, cx);
        let centre = (min < 0.0 && max > 0.0).then(|| (-min / (max - min)) as f32);
        let press = state.clone();
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
            .child(measured(&state))
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
                    .bg(fill_of(lit, amber)),
            )
            .when_some(self.rail, |d, rail| d.child(color_rail::strip(rail, self.disabled)))
            .when_some(centre.filter(|_| lit), |d, c| d.child(div().absolute().top(px(14.)).h(px(8.)).w(px(1.)).left(relative(c)).rounded_full().bg(theme::hashmark())))
            .child(handle(at, handle_of(amber)))
            .child(caption(self.label, text, lit))
            .when(active, |d| {
                tracked(d, &state)
                    .cursor_ew_resize()
                    .on_mouse_down(MouseButton::Left, move |event, window, cx| {
                        cx.stop_propagation();
                        if event.click_count >= 2 {
                            if let Some(f) = &on_reset {
                                f(fallback, window, cx);
                            }
                            return;
                        }
                        let b = press.read(cx).bounds;
                        press.update(cx, |s, cx| s.set_dragging(true, cx));
                        if let Some(f) = &on_press {
                            f(value_at(event.position.x, b, min, max, step), window, cx);
                        }
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
