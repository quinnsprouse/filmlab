use gpui_kit::*;

use super::commands::{choose_mask, paint_stroke, sample_color, update_shape};
use super::geometry::{DragMove, moved_shape, offset, radial_contains};
use super::model::{Pt, Shape};
use super::scene::Grip;
use super::state::{Drag, Interaction};
use crate::ui::app::FilmlabApp;

const GRIP_RADIUS: f64 = 10.0;
const MIN_STEP: f64 = 0.5;

fn distance(a: Pt, b: Pt) -> f64 {
    (a.x - b.x).hypot(a.y - b.y)
}

fn local(app: &FilmlabApp, position: Point<Pixels>) -> Pt {
    let origin = app.tools.masks.pointer.frame.origin;
    Pt::new(f64::from(f32::from(position.x - origin.x)), f64::from(f32::from(position.y - origin.y)))
}

fn inside(app: &FilmlabApp, at: Pt) -> bool {
    let size = app.tools.masks.pointer.frame.size;
    at.x >= 0.0 && at.y >= 0.0 && at.x <= f64::from(f32::from(size.width)) && at.y <= f64::from(f32::from(size.height))
}

fn grip_at(app: &FilmlabApp, at: Pt) -> Option<Grip> {
    let pointer = &app.tools.masks.pointer;
    let active = pointer.active;
    pointer
        .scene
        .grips
        .iter()
        .filter(|grip| distance(grip.at, at) < GRIP_RADIUS)
        .min_by(|a, b| (Some(a.mask) != active).cmp(&(Some(b.mask) != active)).then(distance(a.at, at).total_cmp(&distance(b.at, at))))
        .cloned()
}

fn body_grip(app: &FilmlabApp, at: Pt) -> Option<Grip> {
    let pointer = &app.tools.masks.pointer;
    let mask = pointer.masks.iter().find(|m| Some(m.id) == pointer.active)?;
    let space = pointer.space.as_ref()?;
    if let Some(first) = mask.components.first()
        && matches!(first.shape, Shape::Linear { .. })
    {
        return Some(Grip { mask: mask.id, component: 0, handle: 0, at, shape: first.shape.clone() });
    }
    let norm = *space.to_image(&[at]).first()?;
    let frame = (space.width, space.height);
    mask.components.iter().enumerate().find_map(|(component, c)| match &c.shape {
        Shape::Radial(r) if radial_contains(r, frame, norm) => Some(Grip { mask: mask.id, component, handle: 0, at, shape: c.shape.clone() }),
        _ => None,
    })
}

fn to_image(app: &FilmlabApp, at: Pt) -> Option<Pt> {
    app.tools.masks.pointer.space.as_ref()?.to_image(&[at]).first().copied()
}

fn begin_handle(app: &mut FilmlabApp, grip: Grip, at: Pt, cx: &mut Context<FilmlabApp>) {
    if Some(grip.mask) != app.tools.masks.pointer.active {
        choose_mask(app, Some(grip.mask), cx);
        app.tools.masks.pointer.active = Some(grip.mask);
    }
    if let Some(start) = to_image(app, at) {
        app.tools.masks.pointer.drag = Some(Drag::Handle { grip, start });
        app.tools.masks.engaged = true;
    }
}

pub fn down(app: &mut FilmlabApp, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<FilmlabApp>) {
    cx.stop_propagation();
    if event.button != MouseButton::Left {
        return;
    }
    if let Some(focus) = &app.tools.masks.focus {
        window.focus(focus, cx);
    }
    let at = local(app, event.position);
    match app.tools.masks.interaction {
        Interaction::Pick => {
            if let (Some(mask), Some(norm)) = (app.tools.masks.pointer.active, to_image(app, at)) {
                sample_color(app, mask, norm, event.modifiers.shift, cx);
            }
        }
        Interaction::Paint(_) => {
            app.tools.masks.pointer.drag = Some(Drag::Paint { points: vec![at] });
            app.tools.masks.engaged = true;
        }
        Interaction::Adjust => {
            if let Some(grip) = grip_at(app, at).or_else(|| body_grip(app, at)) {
                begin_handle(app, grip, at, cx);
            }
        }
    }
    cx.notify();
}

fn hover(app: &mut FilmlabApp, at: Pt) {
    let adjusting = app.tools.masks.interaction == Interaction::Adjust;
    let grip = if adjusting { grip_at(app, at) } else { None };
    app.tools.masks.pointer.over_grip = adjusting && (grip.is_some() || body_grip(app, at).is_some());
    let pin = grip.filter(|g| g.handle == 0).map(|g| g.mask);
    if app.tools.masks.hovered != pin {
        app.tools.masks.hovered = pin;
    }
}

fn move_handle(app: &mut FilmlabApp, grip: &Grip, start: Pt, at: Pt, proportional: bool, cx: &mut Context<FilmlabApp>) {
    let Some(norm) = to_image(app, at) else { return };
    let Some(space) = app.tools.masks.pointer.space.as_ref() else { return };
    let frame = (space.width, space.height);
    let shape = moved_shape(&grip.shape, frame, &DragMove { handle: grip.handle, at: norm, delta: offset(start, norm), proportional });
    update_shape(app, grip.mask, grip.component, &shape, "Edit mask", cx);
}

pub fn moved(app: &mut FilmlabApp, event: &MouseMoveEvent, cx: &mut Context<FilmlabApp>) {
    let at = local(app, event.position);
    app.tools.masks.pointer.alt = event.modifiers.alt;
    if event.pressed_button != Some(MouseButton::Left) && app.tools.masks.pointer.drag.is_some() {
        end_drag(app, event.modifiers, cx);
    }
    let was_over = app.tools.masks.pointer.over_grip;
    match app.tools.masks.pointer.drag.take() {
        Some(Drag::Paint { mut points }) => {
            if points.last().is_none_or(|last| distance(*last, at) >= MIN_STEP) {
                points.push(at);
            }
            app.tools.masks.pointer.cursor = Some(at);
            app.tools.masks.pointer.drag = Some(Drag::Paint { points });
        }
        Some(Drag::Handle { grip, start }) => {
            move_handle(app, &grip, start, at, event.modifiers.alt, cx);
            app.tools.masks.pointer.drag = Some(Drag::Handle { grip, start });
        }
        None => {
            let over = inside(app, at);
            app.tools.masks.pointer.cursor = over.then_some(at);
            if over {
                hover(app, at);
            } else {
                app.tools.masks.pointer.over_grip = false;
            }
        }
    }
    let state = &app.tools.masks;
    if state.pointer.cursor.is_some() || state.pointer.drag.is_some() || was_over != state.pointer.over_grip {
        cx.notify();
    }
}

fn end_drag(app: &mut FilmlabApp, modifiers: Modifiers, cx: &mut Context<FilmlabApp>) {
    let Some(drag) = app.tools.masks.pointer.drag.take() else { return };
    if let Drag::Paint { points } = drag
        && let Some(space) = app.tools.masks.pointer.space.clone()
    {
        let mask = match app.tools.masks.interaction {
            Interaction::Paint(mask) => mask,
            _ => None,
        };
        let norm = space.to_image(&points);
        let erase = app.tools.masks.brush.erase != modifiers.alt;
        paint_stroke(app, mask, &norm, erase, modifiers.shift && points.len() == 1, cx);
    }
    app.tools.masks.engaged = false;
    cx.notify();
}

pub fn up(app: &mut FilmlabApp, event: &MouseUpEvent, cx: &mut Context<FilmlabApp>) {
    end_drag(app, event.modifiers, cx);
}
