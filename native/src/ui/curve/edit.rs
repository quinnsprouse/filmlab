use std::time::Instant;

use filmlab_engine::tone_curve::on_curve;
use gpui_kit::*;
use serde_json::{Value, json};

use super::model::{self, Plot};
use super::{Ease, Focused, GRAPH, Gesture, PointDrag, REGIONS, SPLITS, Tab, Target, point_text, region_text};
use crate::develop::{Compare, POINT_CURVES};
use crate::ui::app::FilmlabApp;
use crate::ui::{inspector, motion, slider};

const HIT: f32 = 8.0;
const OFF_GRAPH: f32 = 12.0;
const NEIGHBOUR_GAP: f64 = 0.01;
const SLOP: f32 = 2.0;
const SPLIT_REACH: f32 = 6.0;
const RAIL_REACH: f32 = 9.0;
const REGION_SPAN: f64 = 200.0;
const FINE: f64 = 0.1;
const SAMPLE_REACH: f32 = 3.0;

enum Commit {
    Points { channel: usize, points: Vec<[f64; 2]>, label: String, key: String },
    Control { id: &'static str, value: f64, label: String },
}

fn plot(app: &FilmlabApp) -> Option<Plot> {
    app.ui.placed.get(GRAPH).map(Plot::of)
}

fn distance(a: Point<Pixels>, b: Point<Pixels>) -> f32 {
    f32::from(a.x - b.x).hypot(f32::from(a.y - b.y))
}

fn nearest(points: &[[f64; 2]], plot: Plot, at: Point<Pixels>) -> Option<usize> {
    points.iter().enumerate().map(|(i, p)| (i, distance(plot.at(p[0], p[1]), at))).filter(|(_, d)| *d <= HIT).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(i, _)| i)
}

fn band_of(u: f64, splits: [f64; 3]) -> usize {
    splits.iter().filter(|s| u * 100.0 >= **s).count()
}

fn target(app: &FilmlabApp, plot: Plot, at: Point<Pixels>) -> Option<Target> {
    let snapshot = app.ui.curve.snapshot.borrow();
    if snapshot.tab != Tab::Regions {
        return nearest(&snapshot.points, plot, at).map(Target::Point);
    }
    let rail = plot.rail();
    if f32::from(at.y - rail.center().y).abs() <= RAIL_REACH
        && let Some(i) = snapshot.splits.iter().enumerate().map(|(i, s)| (i, f32::from(plot.at(s / 100.0, 0.0).x - at.x).abs())).filter(|(_, d)| *d <= SPLIT_REACH).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(i, _)| i)
    {
        return Some(Target::Split(i));
    }
    let (u, v) = plot.uv(at);
    ((0.0..=1.0).contains(&u) && (0.0..=1.0).contains(&v)).then(|| Target::Band(band_of(u, snapshot.splits)))
}

fn limits(points: &[[f64; 2]], index: usize) -> (f64, f64) {
    let lo = if index == 0 { 0.0 } else { points[index - 1][0] + NEIGHBOUR_GAP };
    let hi = if index + 1 == points.len() { 1.0 } else { points[index + 1][0] - NEIGHBOUR_GAP };
    (lo, hi.max(lo))
}

fn interior(points: &[[f64; 2]], index: usize) -> bool {
    index > 0 && index + 1 < points.len()
}

fn settings(app: &FilmlabApp, cx: &App) -> Option<Value> {
    app.develop.read(cx).settings.clone()
}

fn apply(app: &mut FilmlabApp, commit: Commit, cx: &mut Context<FilmlabApp>) {
    match commit {
        Commit::Points { channel, points, label, key } => {
            if settings(app, cx).is_some_and(|s| model::points(&s, channel) == points) {
                return;
            }
            app.develop.update(cx, |d, cx| d.command("develop.curve", json!({"channel": POINT_CURVES[channel], "points": points}), &label, &key, cx));
        }
        Commit::Control { id, value, label } => app.develop.update(cx, |d, cx| d.edit_controls(&[(id, value)], &label, cx)),
    }
}

fn removed(tab: Tab) -> String {
    format!("{} curve point removed", tab.label())
}

pub fn pressed(app: &mut FilmlabApp, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<FilmlabApp>) {
    let Some(plot) = plot(app) else { return };
    let focus = app.ui.curve.focus_handle(cx);
    window.focus(&focus, cx);
    let state = &mut app.ui.curve;
    state.ease = None;
    state.gestures += 1;
    let target = target(app, plot, event.position);
    let tab = app.ui.curve.tab;
    match tab.channel() {
        None => press_regions(app, plot, target, event, cx),
        Some(channel) => press_points(app, plot, channel, event, cx),
    }
    cx.notify();
}

fn press_regions(app: &mut FilmlabApp, plot: Plot, target: Option<Target>, event: &MouseDownEvent, cx: &mut Context<FilmlabApp>) {
    let values = app.develop.read(cx).control_values();
    let value = |id: &str| values.get(id).copied().unwrap_or(0.0);
    app.ui.curve.gesture = match target {
        Some(Target::Split(which)) => Some(Gesture::Split { which, value: value(SPLITS[which].0) }),
        Some(Target::Band(region)) => {
            let from = value(REGIONS[region].0);
            Some(Gesture::Band { region, anchor: plot.uv(event.position).1, from, fine: event.modifiers.alt, value: from })
        }
        _ => None,
    };
}

fn press_points(app: &mut FilmlabApp, plot: Plot, channel: usize, event: &MouseDownEvent, cx: &mut Context<FilmlabApp>) {
    let Some(settings) = settings(app, cx) else { return };
    let tab = app.ui.curve.tab;
    let points = model::points(&settings, channel);
    let key = format!("curve.{}#{}", POINT_CURVES[channel], app.ui.curve.gestures);
    let just_added = std::mem::take(&mut app.ui.curve.just_added).filter(|_| event.click_count >= 2);
    let (u, v) = plot.uv(event.position);
    let (index, base, pending) = match nearest(&points, plot, event.position) {
        Some(i) if event.click_count >= 2 && interior(&points, i) => {
            if just_added.is_none_or(|(c, x)| c != channel || (x - points[i][0]).abs() > 1e-9) {
                let mut kept = points;
                kept.remove(i);
                apply(app, Commit::Points { channel, points: kept, label: removed(tab), key }, cx);
                app.ui.curve.focused = None;
            }
            return;
        }
        Some(i) => (i, points, false),
        None if (0.0..=1.0).contains(&u) && (0.0..=1.0).contains(&v) => {
            let i = points.partition_point(|p| p[0] < u);
            let roomy = i > 0 && i < points.len() && u > points[i - 1][0] + NEIGHBOUR_GAP && u < points[i][0] - NEIGHBOUR_GAP;
            if !roomy {
                return;
            }
            let mut base = points.clone();
            base.insert(i, [u, on_curve(&points, u)]);
            (i, base, true)
        }
        None => return,
    };
    let at = base[index];
    let held = (event.modifiers.alt, event.modifiers.shift);
    app.ui.curve.focused = Some(Focused { channel, index, visible: false });
    app.ui.curve.gesture = Some(Gesture::Point(PointDrag {
        channel,
        index,
        base,
        anchor: (u, v),
        from: at,
        held,
        at,
        pressed: event.position,
        pointer: event.position,
        removing: false,
        pending,
        moved: false,
        key,
    }));
}

pub fn moved(app: &mut FilmlabApp, event: &MouseMoveEvent, window: &Window, cx: &mut Context<FilmlabApp>) {
    if app.ui.curve.gesture.is_some() {
        if event.pressed_button == Some(MouseButton::Left) {
            drag(app, event, cx);
        } else {
            released(app, cx);
        }
        return;
    }
    let target = plot(app).filter(|p| p.bounds.contains(&event.position)).and_then(|p| target(app, p, event.position));
    let pointer = over_photo(app, event.position, window, cx).and_then(|(u, v, reach)| app.develop.read(cx).curve_input.as_ref()?.mean_around(u, v, reach));
    let state = &mut app.ui.curve;
    if target != state.hover || pointer != state.pointer {
        state.hover = target;
        state.pointer = pointer;
        cx.notify();
    }
}

pub fn left_window(app: &mut FilmlabApp, cx: &mut Context<FilmlabApp>) {
    let state = &mut app.ui.curve;
    if state.hover.is_some() || state.pointer.is_some() {
        state.hover = None;
        state.pointer = None;
        cx.notify();
    }
}

fn over_photo(app: &FilmlabApp, at: Point<Pixels>, window: &Window, cx: &App) -> Option<(f64, f64, f64)> {
    if app.develop.read(cx).compare != Compare::After {
        return None;
    }
    let stage = app.stage(window);
    let (x, y) = (f64::from(f32::from(at.x)), f64::from(f32::from(at.y)));
    let (frame, _) = app.fit_frame(window, cx)?;
    let on_stage = x >= stage.x && y >= stage.y && x < stage.x + stage.width && y < stage.y + stage.height;
    let (width, height) = (f32::from(frame.size.width), f32::from(frame.size.height));
    (on_stage && frame.contains(&at)).then(|| (f64::from(f32::from(at.x - frame.left()) / width), f64::from(f32::from(at.y - frame.top()) / height), f64::from(SAMPLE_REACH / width)))
}

fn drag(app: &mut FilmlabApp, event: &MouseMoveEvent, cx: &mut Context<FilmlabApp>) {
    let Some(plot) = plot(app) else { return };
    let (u, v) = plot.uv(event.position);
    let tab = app.ui.curve.tab;
    let splits = app.ui.curve.snapshot.borrow().splits;
    let Some(gesture) = app.ui.curve.gesture.as_mut() else { return };
    let commit = match gesture {
        Gesture::Point(drag) => {
            let held = (event.modifiers.alt, event.modifiers.shift);
            if held != drag.held {
                drag.anchor = (u, v);
                drag.from = drag.at;
                drag.held = held;
            }
            let scale = if held.0 { FINE } else { 1.0 };
            let (lo, hi) = limits(&drag.base, drag.index);
            let x = if held.1 { drag.from[0] } else { drag.from[0] + (u - drag.anchor.0) * scale };
            drag.at = [x.clamp(lo, hi), (drag.from[1] + (v - drag.anchor.1) * scale).clamp(0.0, 1.0)];
            drag.pointer = event.position;
            drag.removing = interior(&drag.base, drag.index) && plot.outside_by(event.position) > OFF_GRAPH;
            drag.moved |= distance(event.position, drag.pressed) > SLOP;
            drag.moved.then(|| {
                let mut points = drag.base.clone();
                let label = if drag.removing {
                    points.remove(drag.index);
                    removed(tab)
                } else {
                    points[drag.index] = drag.at;
                    format!("{} curve {}", tab.label(), point_text(drag.at))
                };
                Commit::Points { channel: drag.channel, points, label, key: drag.key.clone() }
            })
        }
        Gesture::Band { region, anchor, from, fine, value } => {
            let alt = event.modifiers.alt;
            if alt != *fine {
                (*anchor, *from, *fine) = (v, *value, alt);
            }
            let next = (*from + (v - *anchor) * REGION_SPAN * if alt { FINE } else { 1.0 }).clamp(-100.0, 100.0).round();
            (next != *value).then(|| {
                *value = next;
                Commit::Control { id: REGIONS[*region].0, value: next, label: region_text(*region, next) }
            })
        }
        Gesture::Split { which, value } => {
            let (id, label) = SPLITS[*which];
            let spec = inspector::spec(id);
            let below = if *which == 0 { f64::MIN } else { splits[*which - 1] + 1.0 };
            let above = splits.get(*which + 1).map_or(f64::MAX, |s| s - 1.0);
            let lo = spec.map_or(0.0, |s| s.min).max(below);
            let hi = spec.map_or(100.0, |s| s.max).min(above).max(lo);
            let next = (u * 100.0).round().clamp(lo, hi);
            (next != *value).then(|| {
                *value = next;
                Commit::Control { id, value: next, label: format!("{label} {}", spec.map_or_else(|| format!("{next:.0}"), |s| inspector::format(s, next))) }
            })
        }
    };
    if let Some(commit) = commit {
        apply(app, commit, cx);
    }
    cx.notify();
}

pub fn released(app: &mut FilmlabApp, cx: &mut Context<FilmlabApp>) {
    let Some(gesture) = app.ui.curve.gesture.take() else { return };
    if let Gesture::Point(drag) = gesture {
        if drag.pending && !drag.moved {
            let label = format!("{} curve point at {}", app.ui.curve.tab.label(), model::level(drag.at[0]));
            apply(app, Commit::Points { channel: drag.channel, points: drag.base.clone(), label, key: drag.key.clone() }, cx);
            app.ui.curve.just_added = Some((drag.channel, drag.at[0]));
        }
        if drag.removing && drag.moved {
            app.ui.curve.focused = None;
        }
    }
    slider::released(cx);
    cx.notify();
}

pub fn blur(app: &mut FilmlabApp, window: &mut Window, cx: &mut Context<FilmlabApp>) {
    if app.ui.curve.holds_focus(window) {
        app.focus_app(window, cx);
    }
    if let Some(f) = app.ui.curve.focused.as_mut() {
        f.visible = false;
    }
}

fn focused_points(app: &FilmlabApp, cx: &App) -> Option<(usize, Vec<[f64; 2]>, Option<usize>)> {
    let channel = app.ui.curve.tab.channel()?;
    let points = model::points(&settings(app, cx)?, channel);
    let index = app.ui.curve.focused.filter(|f| f.channel == channel && f.index < points.len()).map(|f| f.index);
    Some((channel, points, index))
}

pub fn step_focus(app: &mut FilmlabApp, by: isize, cx: &mut Context<FilmlabApp>) {
    let Some((channel, points, index)) = focused_points(app, cx) else {
        cx.propagate();
        return;
    };
    let n = points.len() as isize;
    let next = index.map_or(if by > 0 { 0 } else { n - 1 }, |i| (i as isize + by).rem_euclid(n));
    app.ui.curve.focused = Some(Focused { channel, index: next as usize, visible: true });
    cx.notify();
}

pub fn nudge(app: &mut FilmlabApp, (dx, dy): (f64, f64), levels: f64, cx: &mut Context<FilmlabApp>) {
    let Some((channel, mut points, Some(index))) = focused_points(app, cx) else {
        cx.propagate();
        return;
    };
    let step = levels / 255.0;
    let (lo, hi) = limits(&points, index);
    let at = [(points[index][0] + dx * step).clamp(lo, hi), (points[index][1] + dy * step).clamp(0.0, 1.0)];
    points[index] = at;
    let tab = app.ui.curve.tab;
    app.ui.curve.focused = Some(Focused { channel, index, visible: true });
    apply(app, Commit::Points { channel, points, label: format!("{} curve {}", tab.label(), point_text(at)), key: format!("curve.{}.key{index}", POINT_CURVES[channel]) }, cx);
    cx.notify();
}

pub fn remove_focused(app: &mut FilmlabApp, cx: &mut Context<FilmlabApp>) {
    let Some((channel, mut points, Some(index))) = focused_points(app, cx).filter(|(_, p, i)| i.is_some_and(|i| interior(p, i))) else { return };
    points.remove(index);
    let tab = app.ui.curve.tab;
    app.ui.curve.gestures += 1;
    let key = format!("curve.{}#{}", POINT_CURVES[channel], app.ui.curve.gestures);
    app.ui.curve.focused = Some(Focused { channel, index: index - 1, visible: true });
    apply(app, Commit::Points { channel, points, label: removed(tab), key }, cx);
    cx.notify();
}

pub fn leave(app: &mut FilmlabApp, window: &mut Window, cx: &mut Context<FilmlabApp>) {
    app.ui.curve.focused = None;
    app.focus_app(window, cx);
    cx.notify();
}

pub fn pick_tab(app: &mut FilmlabApp, tab: Tab, reset: bool, cx: &mut Context<FilmlabApp>) {
    let state = &mut app.ui.curve;
    state.tab = tab;
    state.hover = None;
    state.focused = None;
    if reset {
        reset_tab(app, tab, cx);
    }
    cx.notify();
}

fn reset_tab(app: &mut FilmlabApp, tab: Tab, cx: &mut Context<FilmlabApp>) {
    let Some(settings) = settings(app, cx) else { return };
    let from = model::samples(model::line(&app.ui.curve.curves(&settings["curve"]), tab));
    let (label, key) = (format!("{} curve reset", tab.label()), format!("curve.reset.{}", tab.reset()));
    let changed = app.develop.update(cx, |d, cx| d.try_command("curve.reset", json!({"channel": tab.reset()}), &label, &key, cx)).is_ok_and(|(_, changed)| changed);
    if changed && !motion::reduced() {
        app.ui.curve.ease = Some(Ease { tab, from, started: Instant::now() });
    }
}

pub(super) fn names_in(presets: Option<&Value>) -> Vec<String> {
    presets.and_then(|p| p["presets"].as_array()).map(|a| a.iter().filter_map(|p| p["name"].as_str().map(String::from)).collect()).unwrap_or_default()
}

pub fn preset_names(app: &FilmlabApp) -> Vec<String> {
    names_in(app.ui.curve.cache.borrow().presets.as_ref().map(|(_, v)| v))
}

/// What the curve menu offers: LightCraft's built-in point curves, the user's saved curves, and saving this one.
#[derive(Clone)]
pub enum Preset {
    BuiltIn(String),
    Saved(String),
    Save,
}

pub fn choose(app: &mut FilmlabApp, preset: Preset, window: &mut Window, cx: &mut Context<FilmlabApp>) {
    match preset {
        Preset::BuiltIn(name) => {
            let _ = app.develop.update(cx, |d, cx| d.try_command("curve.applyPreset", json!({"name": name}), &format!("Point curve {name}"), "curve.preset", cx));
        }
        Preset::Saved(id) => apply_saved(app, &id, cx),
        Preset::Save => crate::ui::rail::save_curve(app, window, cx),
    }
}

/// Puts a saved curve on the photo: its whole tone curve (points, regions and splits), as one step.
pub fn apply_saved(app: &mut FilmlabApp, id: &str, cx: &mut Context<FilmlabApp>) {
    let Some((name, curve)) = app.rail.curves().find(|p| p.id == id).map(|p| (p.name.clone(), p.partial["curve"].clone())) else { return };
    app.develop.update(cx, |d, cx| d.edit_settings(json!({"curve": curve}), &format!("Tone curve {name}"), "curve.preset", cx));
}

pub fn pick_preset(app: &mut FilmlabApp, index: usize, cx: &mut Context<FilmlabApp>) {
    let Some(name) = preset_names(app).get(index).cloned() else { return };
    let _ = app.develop.update(cx, |d, cx| d.try_command("curve.applyPreset", json!({"name": name}), &format!("Point curve {name}"), "curve.preset", cx));
}
