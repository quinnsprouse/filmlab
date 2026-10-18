use gpui_kit::*;

use super::draw::Layers;
use super::model::read_masks;
use super::pointer;
use super::scene::build_scene;
use super::state::{Drag, Interaction};
use super::view::ViewSpace;
use crate::ui::app::FilmlabApp;

pub fn overlay(app: &mut FilmlabApp, frame: Bounds<Pixels>, window: &mut Window, cx: &mut Context<FilmlabApp>) -> Option<AnyElement> {
    let (_, pixels) = app.fit_frame(window, cx)?;
    let (facts, settings, photo) = {
        let d = app.develop.read(cx);
        (d.facts.clone()?, d.settings.clone()?, d.photo_id.clone())
    };
    let scale = f64::from(pixels.0) / f64::from(f32::from(frame.size.width)).max(1.0);
    let space = ViewSpace::new(&facts, &settings, pixels, scale);
    let masks = read_masks(Some(&settings));
    let active = app.tools.masks.active_for(photo.as_ref()).filter(|id| masks.iter().any(|m| m.id == *id));
    let scene = build_scene(&masks, active, &space);
    let radius = (app.tools.masks.brush.size * space.long_edge()) as f32;
    let state = &mut app.tools.masks;
    let pointer = &mut state.pointer;
    pointer.scene = scene.clone();
    pointer.masks = masks;
    pointer.active = active;
    pointer.space = Some(space);
    pointer.frame = frame;
    let interaction = state.interaction;
    let painting = matches!(interaction, Interaction::Paint(_));
    let erasing = state.brush.erase != pointer.alt;
    let stroke = match &pointer.drag {
        Some(Drag::Paint { points }) => points.clone(),
        _ => Vec::new(),
    };
    let layers = Layers::new(scene, painting, erasing, radius, state.brush.feather, pointer.cursor, stroke);
    let over_grip = pointer.over_grip;
    let dragging = pointer.drag.is_some();
    let focus = state.focus.get_or_insert_with(|| cx.focus_handle()).clone();
    let me = cx.entity();
    let surface = canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            layers.paint(bounds, window);
            let on_move = me.clone();
            window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                if phase == DispatchPhase::Bubble {
                    on_move.update(cx, |app, cx| pointer::moved(app, event, cx));
                }
            });
            let on_up = me.clone();
            window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                if phase == DispatchPhase::Bubble && event.button == MouseButton::Left {
                    on_up.update(cx, |app, cx| pointer::up(app, event, cx));
                }
            });
        },
    )
    .absolute()
    .size_full();
    let cursor = match interaction {
        Interaction::Pick | Interaction::Paint(_) => CursorStyle::Crosshair,
        Interaction::Adjust if dragging => CursorStyle::ClosedHand,
        Interaction::Adjust if over_grip => CursorStyle::OpenHand,
        Interaction::Adjust => CursorStyle::Arrow,
    };
    Some(
        div()
            .id("masks-overlay")
            .absolute()
            .left(frame.origin.x)
            .top(frame.origin.y)
            .w(frame.size.width)
            .h(frame.size.height)
            .cursor(cursor)
            .track_focus(&focus)
            .key_context("MasksOverlay")
            .on_key_down(cx.listener(|app, event: &KeyDownEvent, _, cx| {
                if super::keys::on_key(app, event, cx) {
                    cx.stop_propagation();
                }
            }))
            .on_mouse_down(MouseButton::Left, cx.listener(|app, event: &MouseDownEvent, window, cx| pointer::down(app, event, window, cx)))
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .child(surface)
            .into_any_element(),
    )
}
