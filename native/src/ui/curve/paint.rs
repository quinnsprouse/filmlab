use std::rc::Rc;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::model::{self, HEIGHT, Plot, SAMPLES};
use super::{GRAPH, Gesture, LeapDown, LeapLeft, LeapRight, LeapUp, LeaveCurve, NextPoint, NudgeDown, NudgeLeft, NudgeRight, NudgeUp, PreviousPoint, RemovePoint, SPLITS, Shown, Tab, Target, edit, point_text, region_text, tab_index};
use crate::ui::app::FilmlabApp;
use crate::ui::{inspector, theme};

const DASH: [Pixels; 2] = [px(2.), px(3.)];
const POINT: f32 = 4.5;
const POINT_LIT: f32 = 5.5;
const POINT_GROUND: f32 = 1.5;
const HALO: f32 = 11.0;
const FOCUS_RING: f32 = 9.5;
const POINTER_RING: f32 = 7.0;
const OFF_GRAPH_RING: f32 = 6.5;
const HISTOGRAM_REACH: f32 = 0.8;
const QUARTERS: [f64; 3] = [0.25, 0.5, 0.75];

fn white(alpha: f32) -> Hsla {
    Hsla { h: 0.0, s: 0.0, l: 1.0, a: alpha }
}

fn ground() -> Hsla {
    rgba(0x303030ff).into()
}

struct Dot {
    at: [f64; 2],
    radius: f32,
    halo: bool,
    focus: bool,
}

struct Picture {
    tints: Option<(Hsla, Hsla)>,
    band: Option<(f64, f64)>,
    histogram: Option<(Rc<Vec<f32>>, Hsla)>,
    verticals: ([f64; 3], f32),
    envelope: Option<(Vec<f64>, Vec<f64>)>,
    ghost: Option<Vec<f64>>,
    crosshair: Option<[f64; 2]>,
    line: Vec<f64>,
    ink: Hsla,
    dot: Hsla,
    dots: Vec<Dot>,
    off_graph: Option<Point<Pixels>>,
    splits: Option<([f64; 3], Option<usize>)>,
    pointer: Option<[f64; 2]>,
    focus: FocusHandle,
    keyboard: bool,
    easing: bool,
}

struct Readout {
    text: String,
    primary: bool,
    ring: bool,
}

fn readout(app: &FilmlabApp, shown: &Shown, points: &[[f64; 2]]) -> Option<Readout> {
    let state = &app.ui.curve;
    let says = |text: String, primary: bool| Some(Readout { text, primary, ring: false });
    let split = |which: usize, value: f64| {
        let (id, label) = SPLITS[which];
        format!("{label} {}", inspector::spec(id).map_or_else(|| format!("{value:.0}"), |s| inspector::format(s, value)))
    };
    match &state.gesture {
        Some(Gesture::Point(d)) if d.removing => return says("Release to remove".into(), true),
        Some(Gesture::Point(d)) => return says(point_text(d.at), true),
        Some(Gesture::Band { region, value, .. }) => return says(region_text(*region, *value), true),
        Some(Gesture::Split { which, value }) => return says(split(*which, *value), true),
        None => {}
    }
    match state.hover {
        Some(Target::Point(i)) if i < points.len() => return says(point_text(points[i]), false),
        Some(Target::Band(b)) if shown.tab == Tab::Regions => return says(region_text(b, shown.regions[b]), true),
        Some(Target::Split(i)) if shown.tab == Tab::Regions => return says(split(i, shown.splits[i]), false),
        _ => {}
    }
    if let Some(f) = state.focused.filter(|f| f.visible && Some(f.channel) == shown.tab.channel() && f.index < points.len()) {
        return says(point_text(points[f.index]), true);
    }
    shown.pointer.map(|at| Readout { text: point_text(at), primary: true, ring: true })
}

fn pill(r: Readout) -> impl IntoElement {
    div()
        .absolute()
        .left(px(8.))
        .top(px(8.))
        .pl(px(if r.ring { 6. } else { 7. }))
        .pr(px(7.))
        .py(px(2.))
        .rounded(px(6.))
        .bg(rgba(0x1e1e1ecc))
        .flex()
        .items_center()
        .gap(px(5.))
        .when(r.ring, |d| d.child(div().flex_none().size(px(9.)).rounded_full().border(px(1.5)).border_color(white(1.0))))
        .child(div().text_size(px(12.)).line_height(px(18.)).font_weight(FontWeight::SEMIBOLD).font_features(theme::tabular()).whitespace_nowrap().text_color(if r.primary { theme::primary() } else { theme::secondary() }).child(r.text))
}

fn corner(label: String, color: Hsla) -> Div {
    div().absolute().text_size(px(11.)).line_height(px(16.)).font_weight(FontWeight::SEMIBOLD).whitespace_nowrap().text_color(color).child(label)
}

pub fn graph(app: &FilmlabApp, shown: &Shown, cx: &mut Context<FilmlabApp>) -> AnyElement {
    let state = &app.ui.curve;
    let tab = shown.tab;
    let drag = match &state.gesture {
        Some(Gesture::Point(d)) if Some(d.channel) == tab.channel() && (d.moved || d.pending) => Some(d),
        _ => None,
    };
    let points: Vec<[f64; 2]> = match drag {
        Some(d) if d.removing => d.base.iter().enumerate().filter(|(i, _)| *i != d.index).map(|(_, p)| *p).collect(),
        Some(d) => d.base.iter().enumerate().map(|(i, p)| if i == d.index { d.at } else { *p }).collect(),
        None => shown.points.clone(),
    };
    let dragged = drag.filter(|d| !d.removing).map(|d| d.index);
    let hovered = match state.hover {
        Some(Target::Point(i)) => Some(i),
        _ => None,
    };
    let focused = state.focused.filter(|f| f.visible && Some(f.channel) == tab.channel()).map(|f| f.index);
    let dots = points
        .iter()
        .enumerate()
        .map(|(i, at)| Dot { at: *at, radius: if [dragged, hovered, focused].contains(&Some(i)) { POINT_LIT } else { POINT }, halo: dragged == Some(i), focus: focused == Some(i) && dragged.is_none() })
        .collect();
    let mut line = model::samples(model::line(&shown.curves, tab));
    let easing = state.easing(tab);
    if let Some((from, t)) = easing {
        line.iter_mut().zip(from).for_each(|(y, f)| *y = f + (*y - f) * f64::from(t));
    }
    let curves = &shown.curves;
    let ghost = match tab {
        Tab::Regions => (!curves.master.is_identity()).then(|| model::samples(&curves.master)),
        Tab::Rgb => (!curves.regions.is_identity()).then(|| model::samples(&curves.regions)),
        _ => {
            let into = curves.master.after(&curves.regions);
            (!into.is_identity()).then(|| model::samples(&into))
        }
    };
    let regions = tab == Tab::Regions;
    let lit_split = match (&state.gesture, state.hover) {
        (Some(Gesture::Split { which, .. }), _) => Some(*which),
        (None, Some(Target::Split(i))) => Some(i),
        _ => None,
    };
    let edges = [0.0, shown.splits[0] / 100.0, shown.splits[1] / 100.0, shown.splits[2] / 100.0, 1.0];
    let read = readout(app, shown, &points);
    let opposite = model::opposite(tab);
    let picture = Picture {
        tints: opposite.map(|(_, other)| (model::ink(tab).opacity(0.07), other.opacity(0.07))),
        band: shown.band.map(|b| (edges[b], edges[b + 1])),
        histogram: shown.histogram.clone().map(|h| (h, model::histogram_fill(tab))),
        verticals: if regions { (shown.splits.map(|s| s / 100.0), 0.08) } else { (QUARTERS, 0.05) },
        envelope: shown.envelope.clone(),
        ghost,
        crosshair: drag.filter(|d| !d.removing).map(|d| d.at),
        line,
        ink: model::ink(tab),
        dot: if shown.agent[tab_index(tab)] { theme::accent() } else { model::dot(tab) },
        dots: if regions { Vec::new() } else { dots },
        off_graph: drag.filter(|d| d.removing).map(|d| d.pointer),
        splits: regions.then_some((shown.splits.map(|s| s / 100.0), lit_split)),
        pointer: shown.pointer,
        focus: state.focus_handle(cx),
        keyboard: focused.is_some(),
        easing: easing.is_some(),
    };
    let cursor = match (state.hover, regions) {
        (Some(Target::Split(_)), _) => CursorStyle::ResizeLeftRight,
        (_, true) => CursorStyle::ResizeUpDown,
        (_, false) => CursorStyle::Crosshair,
    };
    let labels = opposite.map(|(other, other_ink)| {
        let top = (read.is_none()).then(|| corner(format!("+ {}", tab.label()), model::ink(tab)).left(px(16.)).top(px(14.)));
        let bottom = corner(format!("+ {other}"), other_ink).right(px(16.)).top(px(HEIGHT - 42.));
        top.into_iter().chain([bottom])
    });
    let me = cx.entity();
    let focus = picture.focus.clone();
    div()
        .id(GRAPH)
        .relative()
        .w_full()
        .h(px(HEIGHT))
        .rounded(px(theme::RADIUS))
        .bg(theme::surface())
        .overflow_hidden()
        .cursor(cursor)
        .track_focus(&focus)
        .key_context("ToneCurve")
        .on_action(cx.listener(|app, _: &NextPoint, _, cx| edit::step_focus(app, 1, cx)))
        .on_action(cx.listener(|app, _: &PreviousPoint, _, cx| edit::step_focus(app, -1, cx)))
        .on_action(cx.listener(|app, _: &NudgeLeft, _, cx| edit::nudge(app, (-1.0, 0.0), 1.0, cx)))
        .on_action(cx.listener(|app, _: &NudgeRight, _, cx| edit::nudge(app, (1.0, 0.0), 1.0, cx)))
        .on_action(cx.listener(|app, _: &NudgeUp, _, cx| edit::nudge(app, (0.0, 1.0), 1.0, cx)))
        .on_action(cx.listener(|app, _: &NudgeDown, _, cx| edit::nudge(app, (0.0, -1.0), 1.0, cx)))
        .on_action(cx.listener(|app, _: &LeapLeft, _, cx| edit::nudge(app, (-1.0, 0.0), 10.0, cx)))
        .on_action(cx.listener(|app, _: &LeapRight, _, cx| edit::nudge(app, (1.0, 0.0), 10.0, cx)))
        .on_action(cx.listener(|app, _: &LeapUp, _, cx| edit::nudge(app, (0.0, 1.0), 10.0, cx)))
        .on_action(cx.listener(|app, _: &LeapDown, _, cx| edit::nudge(app, (0.0, -1.0), 10.0, cx)))
        .on_action(cx.listener(|app, _: &RemovePoint, _, cx| edit::remove_focused(app, cx)))
        .on_action(cx.listener(|app, _: &LeaveCurve, window, cx| edit::leave(app, window, cx)))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|app, event: &MouseDownEvent, window, cx| {
                cx.stop_propagation();
                edit::pressed(app, event, window, cx);
            }),
        )
        .on_mouse_down_out(cx.listener(|app, _: &MouseDownEvent, window, cx| edit::blur(app, window, cx)))
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    paint(&picture, Plot::of(bounds), window);
                    listen(me, window);
                },
            )
            .absolute()
            .size_full(),
        )
        .when_some(read, |d, r| d.child(pill(r)))
        .children(labels.into_iter().flatten())
        .child(app.ui.placed.record(GRAPH))
        .into_any_element()
}

fn listen(me: Entity<FilmlabApp>, window: &mut Window) {
    let moves = me.clone();
    window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
        if phase == DispatchPhase::Bubble {
            moves.update(cx, |app, cx| edit::moved(app, event, window, cx));
        }
    });
    let ups = me.clone();
    window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
        if phase == DispatchPhase::Bubble && event.button == MouseButton::Left {
            ups.update(cx, edit::released);
        }
    });
    window.on_mouse_event(move |_: &MouseExitEvent, phase, _, cx| {
        if phase == DispatchPhase::Bubble {
            me.update(cx, edit::left_window);
        }
    });
}

fn polygon(points: impl IntoIterator<Item = Point<Pixels>>, color: Hsla, window: &mut Window) {
    let mut path = PathBuilder::fill();
    for (i, p) in points.into_iter().enumerate() {
        if i == 0 { path.move_to(p) } else { path.line_to(p) }
    }
    path.close();
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}

fn polyline(points: impl IntoIterator<Item = Point<Pixels>>, width: f32, dashed: bool, color: Hsla, window: &mut Window) {
    let mut path = PathBuilder::stroke(px(width));
    if dashed {
        path = path.dash_array(&DASH);
    }
    for (i, p) in points.into_iter().enumerate() {
        if i == 0 { path.move_to(p) } else { path.line_to(p) }
    }
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}

fn traced(plot: Plot, samples: &[f64]) -> impl Iterator<Item = Point<Pixels>> + '_ {
    samples.iter().enumerate().map(move |(i, y)| plot.at(i as f64 / SAMPLES as f64, *y))
}

fn disc(center: Point<Pixels>, radius: f32, color: Hsla, window: &mut Window) {
    window.paint_quad(fill(Bounds::centered_at(center, size(px(2. * radius), px(2. * radius))), color).corner_radii(px(radius)));
}

fn ring(center: Point<Pixels>, outer: f32, width: f32, color: Hsla, window: &mut Window) {
    window.paint_quad(quad(Bounds::centered_at(center, size(px(2. * outer), px(2. * outer))), px(outer), transparent_black(), px(width), color, BorderStyle::Solid));
}

fn dashes(from: Point<Pixels>, to: Point<Pixels>, color: Hsla, window: &mut Window) {
    let (dx, dy) = (f32::from(to.x - from.x), f32::from(to.y - from.y));
    let length = dx.hypot(dy);
    let (ux, uy) = (dx / length.max(1e-3), dy / length.max(1e-3));
    let [dash, gap] = DASH.map(f32::from);
    let mut at = 0.0;
    while at < length {
        let end = (at + dash).min(length);
        let a = point(from.x + px(ux * at), from.y + px(uy * at));
        let b = point(from.x + px(ux * end), from.y + px(uy * end));
        let bounds = Bounds::from_corners(point(a.x.min(b.x) - px(0.5 * uy.abs()), a.y.min(b.y) - px(0.5 * ux.abs())), point(a.x.max(b.x) + px(0.5 * uy.abs()), a.y.max(b.y) + px(0.5 * ux.abs())));
        window.paint_quad(fill(bounds, color));
        at += dash + gap;
    }
}

fn paint(p: &Picture, plot: Plot, window: &mut Window) {
    let (top, bottom) = (plot.at(0.0, 1.0).y, plot.at(0.0, 0.0).y);
    let (left, right) = (plot.at(0.0, 0.0).x, plot.at(1.0, 0.0).x);
    if let Some((above, below)) = p.tints {
        polygon([plot.at(0.0, 0.0), plot.at(0.0, 1.0), plot.at(1.0, 1.0)], above, window);
        polygon([plot.at(0.0, 0.0), plot.at(1.0, 0.0), plot.at(1.0, 1.0)], below, window);
    }
    if let Some((a, b)) = p.band {
        window.paint_quad(fill(Bounds::from_corners(plot.at(a, 1.0), plot.at(b, 0.0)), white(0.04)));
    }
    if let Some((heights, color)) = &p.histogram {
        let rise = |h: f32| f64::from(h * HISTOGRAM_REACH);
        let outline = heights.iter().enumerate().map(|(i, h)| plot.at(i as f64 / 255.0, rise(*h)));
        polygon(std::iter::once(plot.at(0.0, 0.0)).chain(outline).chain([plot.at(1.0, 0.0)]), *color, window);
    }
    let (verticals, alpha) = p.verticals;
    for u in verticals {
        let x = plot.at(u, 0.0).x;
        window.paint_quad(fill(Bounds::from_corners(point(x - px(0.5), top), point(x + px(0.5), bottom)), white(alpha)));
    }
    for v in QUARTERS {
        let y = plot.at(0.0, v).y;
        window.paint_quad(fill(Bounds::from_corners(point(left, y - px(0.5)), point(right, y + px(0.5))), white(0.05)));
    }
    polyline([plot.at(0.0, 0.0), plot.at(1.0, 1.0)], 1.0, false, white(0.16), window);
    if let Some((low, high)) = &p.envelope {
        polygon(traced(plot, high).chain(traced(plot, low).collect::<Vec<_>>().into_iter().rev()), white(0.07), window);
    }
    if let Some(ghost) = &p.ghost {
        polyline(traced(plot, ghost), 1.0, true, white(0.3), window);
    }
    if let Some([x, y]) = p.crosshair {
        let at = plot.at(x, y);
        dashes(at, point(at.x, bottom), white(0.3), window);
        dashes(point(left, at.y), at, white(0.3), window);
    }
    polyline(traced(plot, &p.line), 2.0, false, p.ink, window);
    for end in [0, p.line.len() - 1] {
        disc(plot.at(end as f64 / SAMPLES as f64, p.line[end]), 1.0, p.ink, window);
    }
    let rail = plot.rail();
    window.paint_quad(fill(rail, linear_gradient(90., linear_color_stop(rgba(0x000000ff), 0.), linear_color_stop(rgba(0xffffffff), 1.))).corner_radii(rail.size.height / 2.));
    let focused = p.keyboard && p.focus.is_focused(window);
    for dot in &p.dots {
        let at = plot.at(dot.at[0], dot.at[1]);
        if dot.halo {
            disc(at, HALO, p.dot.opacity(0.18), window);
        }
        if dot.focus && focused {
            ring(at, FOCUS_RING + 0.75, 1.5, white(0.75), window);
        }
        disc(at, dot.radius + POINT_GROUND, ground(), window);
        disc(at, dot.radius, p.dot, window);
    }
    if let Some(pointer) = p.off_graph {
        let b = plot.bounds.inset(px(OFF_GRAPH_RING));
        let at = point(pointer.x.clamp(b.left(), b.right()), pointer.y.clamp(b.top(), b.bottom()));
        ring(at, OFF_GRAPH_RING, 1.5, white(0.55), window);
    }
    if let Some((splits, lit)) = p.splits {
        for (i, s) in splits.iter().enumerate() {
            let handle = Bounds::centered_at(point(plot.at(*s, 0.0).x, rail.center().y), size(px(3.), px(13.)));
            if lit == Some(i) {
                window.paint_quad(fill(handle.dilate(px(4.)), white(0.14)).corner_radii(px(5.5)));
            }
            window.paint_quad(fill(handle.dilate(px(1.)), Hsla { h: 0., s: 0., l: 0., a: 0.35 }).corner_radii(px(2.5)));
            window.paint_quad(fill(handle, white(1.0)).corner_radii(px(1.5)));
        }
    }
    if let Some([x, y]) = p.pointer {
        let at = plot.at(x, y);
        dashes(point(at.x, at.y + px(8.)), point(at.x, bottom), white(0.22), window);
        ring(at, POINTER_RING + 2.0, 4.0, ground(), window);
        ring(at, POINTER_RING + 0.75, 1.5, white(1.0), window);
        window.paint_quad(fill(Bounds::centered_at(point(at.x, rail.center().y), size(px(1.), px(11.))), white(1.0)).corner_radii(px(0.5)));
    }
    if focused {
        window.paint_quad(quad(plot.bounds, px(theme::RADIUS), transparent_black(), px(1.5), white(0.35), BorderStyle::Solid));
    }
    if p.easing {
        window.request_animation_frame();
    }
}
