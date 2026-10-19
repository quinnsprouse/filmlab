use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use serde_json::json;

use super::geometry::{Crop, FrameMap, Rect, Vec2, format_angle, read_crop};
use super::guides::{self, Guide};
use super::{commands, finish};
use crate::ui::app::FilmlabApp;
use crate::ui::theme;

const REACH: f64 = 12.0;
const MOVE_HANDLE: usize = 8;
const MARGIN: f32 = 24.0;

#[derive(Clone, Copy)]
pub enum Gesture {
    Handle { id: u64, handle: usize, start: Rect, angle: f64, origin: Vec2 },
    Rotate { id: u64, start_angle: f64, start_pointer: f64 },
}

struct View {
    map: FrameMap,
    crop: Crop,
    scale: f64,
    origin: Point<Pixels>,
    size: (f64, f64),
    corners: [Vec2; 4],
    handles: Vec<Vec2>,
}

fn view(app: &FilmlabApp, cx: &App) -> Option<View> {
    let (bounds, frame) = app.tools.crop.frame?;
    let d = app.develop.read(cx);
    let (facts, settings) = (d.facts.as_ref()?, d.settings.as_ref()?);
    let map = FrameMap::new(facts, settings, frame)?;
    let crop = read_crop(settings);
    let scale = f64::from(f32::from(bounds.size.width)) / f64::from(frame.0.max(1));
    let corners = map.corners(&crop).map(|c| [c[0] * scale, c[1] * scale]);
    let size = (f64::from(f32::from(bounds.size.width)), f64::from(f32::from(bounds.size.height)));
    Some(View { map, crop, scale, origin: bounds.origin, size, handles: handle_points(&corners), corners })
}

fn handle_points(corners: &[Vec2; 4]) -> Vec<Vec2> {
    let edges = (0..4).map(|i| {
        let (a, b) = (corners[i], corners[(i + 1) % 4]);
        [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0]
    });
    corners.iter().copied().chain(edges).collect()
}

impl View {
    fn local(&self, position: Point<Pixels>) -> Vec2 {
        [f64::from(f32::from(position.x - self.origin.x)), f64::from(f32::from(position.y - self.origin.y))]
    }

    fn straight(&self, at: Vec2, angle: f64) -> Vec2 {
        self.map.to_straight([at[0] / self.scale, at[1] / self.scale], angle)
    }

    fn nearest(&self, at: Vec2) -> Option<usize> {
        self.handles.iter().position(|h| (h[0] - at[0]).hypot(h[1] - at[1]) < REACH)
    }

    fn inside(&self, at: Vec2) -> bool {
        self.crop.rect.contains(self.straight(at, self.crop.angle))
    }

    fn near(&self, at: Vec2) -> bool {
        let m = f64::from(MARGIN);
        at[0] >= -m && at[1] >= -m && at[0] <= self.size.0 + m && at[1] <= self.size.1 + m
    }

    fn pointer_angle(&self, at: Vec2) -> f64 {
        (at[1] - self.size.1 / 2.0).atan2(at[0] - self.size.0 / 2.0)
    }

    fn cursor(&self, at: Vec2) -> CursorStyle {
        match self.nearest(at) {
            Some(handle) => handle_cursor(handle),
            None if self.inside(at) => CursorStyle::OpenHand,
            None => CursorStyle::Crosshair,
        }
    }
}

fn handle_cursor(handle: usize) -> CursorStyle {
    match handle {
        0 | 2 => CursorStyle::ResizeUpLeftDownRight,
        1 | 3 => CursorStyle::ResizeUpRightDownLeft,
        4 | 6 => CursorStyle::ResizeUpDown,
        5 | 7 => CursorStyle::ResizeLeftRight,
        _ => CursorStyle::OpenHand,
    }
}

fn press(app: &mut FilmlabApp, event: &MouseDownEvent, cx: &mut Context<FilmlabApp>) {
    let Some(v) = view(app, cx) else { return };
    let at = v.local(event.position);
    let handle = v.nearest(at);
    let inside = v.inside(at);
    if event.click_count >= 2 && inside && handle.is_none() {
        finish(app, cx);
        return;
    }
    let crop = &mut app.tools.crop;
    crop.gestures += 1;
    let id = crop.gestures;
    crop.gesture = Some(match handle.or(inside.then_some(MOVE_HANDLE)) {
        Some(handle) => Gesture::Handle { id, handle, start: v.crop.rect, angle: v.crop.angle, origin: v.straight(at, v.crop.angle) },
        None => Gesture::Rotate { id, start_angle: v.crop.angle, start_pointer: v.pointer_angle(at) },
    });
    crop.readout = matches!(crop.gesture, Some(Gesture::Rotate { .. })).then_some((at, v.crop.angle));
    crop.cursor = v.cursor(at);
    cx.notify();
}

fn turned(from: f64, to: f64) -> f64 {
    let delta = to - from;
    delta.sin().atan2(delta.cos())
}

fn drag(app: &mut FilmlabApp, v: &View, gesture: Gesture, event: &MouseMoveEvent, cx: &mut Context<FilmlabApp>) {
    let at = v.local(event.position);
    match gesture {
        Gesture::Handle { id, handle, start, angle, origin } => {
            let params = json!({"handle": handle, "from": origin, "to": v.straight(at, angle), "start": start.array(), "keepRatio": event.modifiers.shift});
            commands::drag_handle(app, params, id, cx);
        }
        Gesture::Rotate { id, start_angle, start_pointer } => {
            let degrees = turned(start_pointer, v.pointer_angle(at)).to_degrees();
            let angle = ((start_angle + degrees).clamp(-45.0, 45.0) * 100.0).round() / 100.0;
            commands::straighten(app, angle, id, cx);
            app.tools.crop.readout = Some((at, angle));
        }
    }
    cx.notify();
}

fn moved(app: &mut FilmlabApp, event: &MouseMoveEvent, cx: &mut Context<FilmlabApp>) {
    let Some(v) = view(app, cx) else { return };
    match app.tools.crop.gesture {
        Some(_) if event.pressed_button != Some(MouseButton::Left) => release(app, cx),
        Some(gesture) => drag(app, &v, gesture, event, cx),
        None => {
            let at = v.local(event.position);
            let cursor = if v.near(at) { v.cursor(at) } else { CursorStyle::Arrow };
            if app.tools.crop.cursor != cursor {
                app.tools.crop.cursor = cursor;
                cx.notify();
            }
        }
    }
}

fn release(app: &mut FilmlabApp, cx: &mut Context<FilmlabApp>) {
    let crop = &mut app.tools.crop;
    if crop.gesture.take().is_some() {
        crop.readout = None;
        cx.notify();
    }
}

pub fn overlay(app: &mut FilmlabApp, frame: Bounds<Pixels>, _window: &mut Window, cx: &mut Context<FilmlabApp>) -> Option<AnyElement> {
    let size = app.develop.read(cx).fit.as_ref().map(|f| (f.width, f.height))?;
    app.tools.crop.frame = Some((frame, size));
    let v = view(app, cx)?;
    let guide = match app.tools.crop.gesture {
        Some(Gesture::Rotate { .. }) => Guide::Grid,
        Some(Gesture::Handle { .. }) => Guide::Thirds,
        None => Guide::None,
    };
    let (corners, handles, bounds) = (v.corners, v.handles, frame);
    let me = cx.entity();
    let painter = canvas(
        |_, _, _| {},
        move |_, _, window, _| {
            guides::paint(window, bounds.origin, bounds.size, &corners, &handles, guide);
            let moves = me.clone();
            window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                if phase == DispatchPhase::Bubble {
                    moves.update(cx, |app, cx| moved(app, event, cx));
                }
            });
            let ups = me.clone();
            window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                if phase == DispatchPhase::Bubble && event.button == MouseButton::Left {
                    ups.update(cx, release);
                }
            });
        },
    )
    .absolute()
    .size_full();
    let state = &app.tools.crop;
    let readout = state.readout.map(|(at, angle)| {
        div()
            .absolute()
            .left(px(at[0] as f32 + 18.))
            .top(px(at[1] as f32 + 14.))
            .px(px(6.))
            .py(px(2.))
            .rounded(px(4.))
            .bg(Hsla { a: 0.8, ..theme::backdrop() })
            .text_size(px(12.))
            .font_features(theme::tabular())
            .text_color(theme::primary())
            .child(format!("{}°", format_angle(angle)))
    });
    let grab = div()
        .absolute()
        .left(px(-MARGIN))
        .top(px(-MARGIN))
        .w(frame.size.width + px(2. * MARGIN))
        .h(frame.size.height + px(2. * MARGIN))
        .cursor(state.cursor)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|app, event: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                press(app, event, cx);
            }),
        );
    Some(
        div()
            .absolute()
            .left(frame.origin.x)
            .top(frame.origin.y)
            .w(frame.size.width)
            .h(frame.size.height)
            .child(painter)
            .child(grab)
            .when_some(readout, |d, r| d.child(r))
            .into_any_element(),
    )
}
