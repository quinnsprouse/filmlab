use std::rc::Rc;

use gpui_kit::*;
use serde_json::json;

use super::draw::{Scene, Strokes};
use super::geometry::{Grip, Mapping, SpotShape, distance, grip_at, grips_of, radius_of};
use super::spots::{self, SIZE_MAX, SIZE_MIN, Spot, V};
use super::{Shapes, State};
use crate::develop::Compare;
use crate::ui::app::FilmlabApp;

const DRAG_THRESHOLD: f64 = 3.0;

pub enum Gesture {
    Move { grip: Grip, start: V, last: V, moved: bool },
    Paint,
}

fn local(position: Point<Pixels>, frame: Bounds<Pixels>) -> V {
    (f64::from(f32::from(position.x - frame.origin.x)), f64::from(f32::from(position.y - frame.origin.y)))
}

fn mapping(app: &FilmlabApp, cx: &App, frame: (u32, u32), scale: f64) -> Option<Mapping> {
    let d = app.develop.read(cx);
    Some(Mapping { facts: d.facts.as_ref()?.to_string(), settings: d.settings.as_ref()?.to_string(), frame, scale })
}

fn shapes_of(app: &mut FilmlabApp, cx: &App, frame: (u32, u32), scale: f64, spots: &[Spot]) -> Option<(Rc<[SpotShape]>, Strokes)> {
    let revision = app.develop.read(cx).revision;
    if let Some(s) = app.tools.heal.shapes.as_ref().filter(|s| s.revision == revision && s.frame == frame && s.scale == scale) {
        return Some((s.shapes.clone(), s.strokes.clone()));
    }
    let shapes: Rc<[SpotShape]> = mapping(app, cx, frame, scale)?.shapes_of(spots).into();
    app.tools.heal.grips = grips_of(&shapes);
    let strokes = Strokes::default();
    app.tools.heal.shapes = Some(Shapes { revision, frame, scale, shapes: shapes.clone(), strokes: strokes.clone() });
    Some((shapes, strokes))
}

/// What the pointer shows: the brush ring (where, if drawn) and whether it is over a spot's grip.
#[derive(PartialEq)]
struct Pointer {
    ring: Option<V>,
    over_grip: bool,
    moving: bool,
}

fn pointer(h: &State, selected: Option<usize>) -> Pointer {
    let moving = matches!(h.gesture, Some(Gesture::Move { moved: true, .. }));
    let idle = !moving && h.paint.is_empty();
    let over_grip = h.hover.filter(|_| idle).and_then(|at| grip_at(&h.grips, at, selected)).is_some();
    Pointer { ring: h.hover.filter(|_| idle && !over_grip), over_grip, moving }
}

pub fn overlay(app: &mut FilmlabApp, frame: Bounds<Pixels>, window: &mut Window, cx: &mut Context<FilmlabApp>) -> Option<AnyElement> {
    let (_, size) = app.fit_frame(window, cx)?;
    if app.develop.read(cx).compare == Compare::Before {
        return None;
    }
    let scale = f64::from(window.scale_factor());
    let (photo, spots) = super::current(app, cx);
    let (shapes, strokes) = shapes_of(app, cx, size, scale, &spots)?;
    let h = &mut app.tools.heal;
    let selected = h.selected(photo.as_deref(), spots.len());
    let focus = h.focus.get_or_insert_with(|| cx.focus_handle()).clone();
    let shown = pointer(h, selected);
    let scene = Scene { shapes, strokes, selected, paint: h.paint.clone(), cursor: shown.ring, radius: radius_of(h.brush.size, size, scale), feather: h.brush.feather };
    let cursor = if shown.moving { CursorStyle::ClosedHand } else if shown.over_grip { CursorStyle::OpenHand } else { CursorStyle::Crosshair };
    Some(
        div()
            .id("heal-overlay")
            .track_focus(&focus)
            .absolute()
            .left(frame.origin.x)
            .top(frame.origin.y)
            .w(frame.size.width)
            .h(frame.size.height)
            .overflow_hidden()
            .cursor(cursor)
            .on_mouse_down(MouseButton::Left, cx.listener(move |app, e: &MouseDownEvent, window, cx| down(app, e, frame, window, cx)))
            .on_mouse_move(cx.listener(move |app, e: &MouseMoveEvent, window, cx| moved(app, e, frame, size, window, cx)))
            .on_mouse_up(MouseButton::Left, cx.listener(move |app, e: &MouseUpEvent, window, cx| up(app, e, frame, size, window, cx)))
            .on_mouse_up_out(MouseButton::Left, cx.listener(move |app, e: &MouseUpEvent, window, cx| up(app, e, frame, size, window, cx)))
            .on_hover(cx.listener(|app, over: &bool, _, cx| {
                if !*over {
                    app.tools.heal.hover = None;
                    cx.notify();
                }
            }))
            .on_key_down(cx.listener(key))
            .child(canvas(|_, _, _| {}, move |bounds, _, window, _| scene.paint(bounds.origin, window)).absolute().size_full())
            .into_any_element(),
    )
}

fn down(app: &mut FilmlabApp, event: &MouseDownEvent, frame: Bounds<Pixels>, window: &mut Window, cx: &mut Context<FilmlabApp>) {
    cx.stop_propagation();
    if let Some(focus) = &app.tools.heal.focus {
        window.focus(focus, cx);
    }
    let at = local(event.position, frame);
    let (photo, spots) = super::current(app, cx);
    let h = &mut app.tools.heal;
    let selected = h.selected(photo.as_deref(), spots.len());
    if let Some(grip) = grip_at(&h.grips, at, selected) {
        h.pick(photo.as_deref(), grip.index, spots.get(grip.index));
        h.gesture = Some(Gesture::Move { grip, start: at, last: at, moved: false });
    } else {
        h.gesture = Some(Gesture::Paint);
        h.paint = vec![at];
    }
    cx.notify();
}

fn moved(app: &mut FilmlabApp, event: &MouseMoveEvent, frame: Bounds<Pixels>, size: (u32, u32), window: &mut Window, cx: &mut Context<FilmlabApp>) {
    if let Some(focus) = &app.tools.heal.focus
        && !focus.is_focused(window)
    {
        window.focus(focus, cx);
    }
    let at = local(event.position, frame);
    let scale = f64::from(window.scale_factor());
    let (photo, spots) = super::current(app, cx);
    let h = &mut app.tools.heal;
    let selected = h.selected(photo.as_deref(), spots.len());
    let shown = pointer(h, selected);
    h.hover = Some(at);
    if h.gesture.is_some() && event.pressed_button != Some(MouseButton::Left) {
        finish(app, size, scale, cx);
        return;
    }
    let painted = match h.gesture.take() {
        Some(Gesture::Move { grip, start, mut last, mut moved }) => {
            if moved || distance(start, at) >= DRAG_THRESHOLD {
                let from = last;
                moved = true;
                last = at;
                app.tools.heal.gesture = Some(Gesture::Move { grip, start, last, moved });
                drag_grip(app, cx, size, scale, grip, from, at);
            } else {
                app.tools.heal.gesture = Some(Gesture::Move { grip, start, last, moved });
            }
            false
        }
        Some(Gesture::Paint) => {
            h.gesture = Some(Gesture::Paint);
            let radius = f64::from(size.0.max(size.1)) * h.brush.size / scale;
            let far = h.paint.last().is_none_or(|&l| distance(l, at) >= (radius * 0.35).max(2.0));
            if far {
                h.paint.push(at);
            }
            far
        }
        None => false,
    };
    if painted || pointer(&app.tools.heal, selected) != shown {
        cx.notify();
    }
}

fn drag_grip(app: &mut FilmlabApp, cx: &mut Context<FilmlabApp>, size: (u32, u32), scale: f64, grip: Grip, from: V, to: V) {
    let Some(map) = mapping(app, cx, size, scale) else { return };
    let mapped = map.css_to_image(&[from, to]);
    let (Some(a), Some(b)) = (mapped.first(), mapped.get(1)) else { return };
    let delta = json!([b.0 - a.0, b.1 - a.1]);
    if grip.source {
        spots::update_spot(app, cx, grip.index, "moveSource", delta, "Move source", "move-source");
    } else {
        spots::update_spot(app, cx, grip.index, "move", delta, "Move spot", "move");
    }
}

fn up(app: &mut FilmlabApp, _: &MouseUpEvent, _: Bounds<Pixels>, size: (u32, u32), window: &mut Window, cx: &mut Context<FilmlabApp>) {
    finish(app, size, f64::from(window.scale_factor()), cx);
}

fn finish(app: &mut FilmlabApp, size: (u32, u32), scale: f64, cx: &mut Context<FilmlabApp>) {
    let h = &mut app.tools.heal;
    let gesture = h.gesture.take();
    let paint = std::mem::take(&mut h.paint);
    if !matches!(gesture, Some(Gesture::Paint)) {
        cx.notify();
        return;
    }
    let Some(map) = mapping(app, cx, size, scale) else { return };
    let points = map.css_to_image(&paint);
    let brush = &app.tools.heal.brush;
    let (mode, bsize, feather, opacity) = (brush.mode, brush.size, brush.feather, brush.opacity);
    let index = spots::add_spot(app, cx, mode, &points, bsize, feather, opacity);
    let photo = app.develop.read(cx).photo_id.clone();
    app.tools.heal.select(photo.as_deref(), index);
    cx.notify();
}

fn key(app: &mut FilmlabApp, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<FilmlabApp>) {
    let m = event.keystroke.modifiers;
    if m.platform || m.control || m.alt {
        return;
    }
    let (photo, spots) = super::current(app, cx);
    let selected = app.tools.heal.selected(photo.as_deref(), spots.len());
    match (event.keystroke.key.as_str(), selected) {
        ("delete" | "backspace", Some(i)) => {
            spots::delete_spot(app, cx, i);
            app.tools.heal.select(photo.as_deref(), None);
        }
        ("/", Some(i)) => spots::refresh_source(app, cx, i),
        ("escape", Some(_)) => app.tools.heal.select(photo.as_deref(), None),
        ("[", _) => resize(app, cx, selected, 1.0 / 1.15),
        ("]", _) => resize(app, cx, selected, 1.15),
        _ => return,
    }
    cx.stop_propagation();
    cx.notify();
}

fn resize(app: &mut FilmlabApp, cx: &mut Context<FilmlabApp>, selected: Option<usize>, factor: f64) {
    let size = (app.tools.heal.brush.size * factor).clamp(SIZE_MIN, SIZE_MAX);
    app.tools.heal.brush.size = size;
    if let Some(i) = selected {
        spots::update_spot(app, cx, i, "size", json!(size), "Spot size", "size");
    }
}
