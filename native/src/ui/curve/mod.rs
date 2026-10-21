mod edit;
mod model;
mod paint;

use std::cell::{OnceCell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::{Duration, Instant};

use filmlab_engine::tone_curve::Curves;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use serde_json::{Value, json};

use super::app::FilmlabApp;
use super::controls::Picker;
use super::theme;
use crate::develop::POINT_CURVES;
use crate::menu::Entry;
use model::{Intake, level};

pub use edit::{Preset, choose, pick_preset, preset_names};
pub use paint::graph;

actions!(tone_curve, [NextPoint, PreviousPoint, NudgeLeft, NudgeRight, NudgeUp, NudgeDown, LeapLeft, LeapRight, LeapUp, LeapDown, RemovePoint, LeaveCurve]);

pub const REGIONS: [(&str, &str); 4] = [("curve.shadows", "Shadows"), ("curve.darks", "Darks"), ("curve.lights", "Lights"), ("curve.highlights", "Highlights")];
pub const SPLITS: [(&str, &str); 3] = [("curve.splitShadows", "Shadows split"), ("curve.splitMid", "Midtones split"), ("curve.splitHighlights", "Highlights split")];
pub const GRAPH: &str = "tone-curve-graph";
const EASE: Duration = Duration::from_millis(200);

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Tab {
    Regions,
    #[default]
    Rgb,
    Red,
    Green,
    Blue,
}

impl Tab {
    pub const ALL: [Tab; 5] = [Tab::Regions, Tab::Rgb, Tab::Red, Tab::Green, Tab::Blue];

    pub fn label(self) -> &'static str {
        match self {
            Tab::Regions => "Regions",
            Tab::Rgb => "RGB",
            Tab::Red => "Red",
            Tab::Green => "Green",
            Tab::Blue => "Blue",
        }
    }

    pub fn id(self) -> String {
        format!("tone-curve-tab-{}", self.label())
    }

    fn channel(self) -> Option<usize> {
        match self {
            Tab::Regions => None,
            Tab::Rgb => Some(0),
            Tab::Red => Some(1),
            Tab::Green => Some(2),
            Tab::Blue => Some(3),
        }
    }

    fn reset(self) -> &'static str {
        self.channel().map_or("parametric", |c| POINT_CURVES[c])
    }

    fn marks(self) -> Vec<String> {
        match self.channel() {
            Some(c) => vec![format!("curve.{}", POINT_CURVES[c])],
            None => REGIONS.iter().chain(&SPLITS).map(|(id, _)| id.to_string()).collect(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Target {
    Point(usize),
    Band(usize),
    Split(usize),
}

#[derive(Clone, Copy, PartialEq, Debug)]
struct Focused {
    channel: usize,
    index: usize,
    visible: bool,
}

#[derive(Clone, Debug)]
struct PointDrag {
    channel: usize,
    index: usize,
    base: Vec<[f64; 2]>,
    anchor: (f64, f64),
    from: [f64; 2],
    held: (bool, bool),
    at: [f64; 2],
    pressed: Point<Pixels>,
    pointer: Point<Pixels>,
    removing: bool,
    pending: bool,
    moved: bool,
    key: String,
}

#[derive(Clone, Debug)]
enum Gesture {
    Point(PointDrag),
    Band { region: usize, anchor: f64, from: f64, fine: bool, value: f64 },
    Split { which: usize, value: f64 },
}

struct Ease {
    tab: Tab,
    from: Vec<f64>,
    started: Instant,
}

#[derive(Clone, Default)]
struct Snapshot {
    tab: Tab,
    points: Vec<[f64; 2]>,
    splits: [f64; 3],
}

type HistogramKey = (usize, u64);

#[derive(Default)]
struct Cache {
    curves: Option<(String, Rc<Curves>)>,
    histogram: Option<(HistogramKey, Rc<Vec<f32>>)>,
    presets: Option<(u64, Value)>,
}

#[derive(Default)]
pub struct State {
    pub tab: Tab,
    hover: Option<Target>,
    gesture: Option<Gesture>,
    gestures: u64,
    focused: Option<Focused>,
    just_added: Option<(usize, f64)>,
    ease: Option<Ease>,
    pointer: Option<[u8; 3]>,
    focus: OnceCell<FocusHandle>,
    snapshot: RefCell<Snapshot>,
    cache: RefCell<Cache>,
}

impl State {
    pub fn pointing(&self) -> bool {
        self.pointer.is_some()
    }

    pub fn removing(&self) -> bool {
        matches!(&self.gesture, Some(Gesture::Point(d)) if d.removing)
    }

    /// How far the shown tab's reset has eased back to straight, 0 to 1, while it is easing.
    pub fn resetting(&self) -> Option<f32> {
        self.easing(self.tab).map(|(_, t)| t)
    }

    pub fn holds_focus(&self, window: &Window) -> bool {
        self.focus.get().is_some_and(|f| f.is_focused(window))
    }

    fn curves(&self, curve: &Value) -> Rc<Curves> {
        let key = curve.to_string();
        let mut cache = self.cache.borrow_mut();
        if let Some((k, c)) = &cache.curves
            && *k == key
        {
            return c.clone();
        }
        let curves = Rc::new(Curves::of(&key).unwrap_or_else(|_| Curves::of("null").expect("an empty tone curve")));
        cache.curves = Some((key, curves.clone()));
        curves
    }

    fn easing(&self, tab: Tab) -> Option<(&[f64], f32)> {
        let ease = self.ease.as_ref().filter(|e| e.tab == tab)?;
        let t = ease.started.elapsed().as_secs_f32() / EASE.as_secs_f32();
        (t < 1.0).then(|| (ease.from.as_slice(), super::motion::bezier(0.0, 0.0, 0.58, 1.0)(t)))
    }

    fn focus_handle(&self, cx: &mut App) -> FocusHandle {
        self.focus.get_or_init(|| cx.focus_handle()).clone()
    }
}

pub struct Shown {
    pub tab: Tab,
    curves: Rc<Curves>,
    points: Vec<[f64; 2]>,
    splits: [f64; 3],
    regions: [f64; 4],
    in_use: [bool; 5],
    straight: bool,
    agent: [bool; 5],
    band: Option<usize>,
    envelope: Option<(Vec<f64>, Vec<f64>)>,
    histogram: Option<Rc<Vec<f32>>>,
    pointer: Option<[f64; 2]>,
    presets: Option<Value>,
    saved: Vec<Saved>,
}

/// A tone curve the user saved as a profile, and whether the photo's curve is it.
struct Saved {
    id: String,
    name: String,
    on: bool,
}

impl Shown {
    pub fn of(app: &FilmlabApp, settings: &Value, values: &HashMap<String, f64>, marks: &HashSet<String>, cx: &mut Context<FilmlabApp>) -> Shown {
        let state = &app.ui.curve;
        let tab = state.tab;
        let curves = state.curves(&settings["curve"]);
        let value = |id: &str| values.get(id).copied().or_else(|| super::inspector::spec(id).map(|s| s.default)).unwrap_or(0.0);
        let splits = SPLITS.map(|(id, _)| value(id));
        let regions = REGIONS.map(|(id, _)| value(id));
        let points = tab.channel().map(|c| model::points(settings, c)).unwrap_or_default();
        let in_use = Tab::ALL.map(|t| !model::line(&curves, t).is_identity());
        let straight = match &state.gesture {
            Some(Gesture::Point(d)) if Some(d.channel) == tab.channel() => d.base.iter().all(|[x, y]| (x - y).abs() < 1e-9),
            _ => !in_use[tab_index(tab)],
        };
        let agent = Tab::ALL.map(|t| t.marks().iter().any(|m| marks.contains(m)));
        let intake = Intake::of(&curves, tab);
        let histogram = Self::histogram(app, &intake, cx);
        let pointer = state.pointer.filter(|_| state.gesture.is_none()).map(|rgb| {
            let x = intake.tone(rgb);
            [x, model::line(&curves, tab).at(x)]
        });
        let presets = tab.channel().and_then(|_| {
            let revision = app.develop.read(cx).revision;
            if let Some((r, v)) = &state.cache.borrow().presets
                && *r == revision
            {
                return Some(v.clone());
            }
            let listed = app.develop.update(cx, |d, _| d.run("curve.presets", json!({})))?;
            state.cache.borrow_mut().presets = Some((revision, listed.clone()));
            Some(listed)
        });
        let band = match (&state.gesture, state.hover) {
            (Some(Gesture::Band { region, .. }), _) => Some(*region),
            (None, Some(Target::Band(b))) => Some(b),
            _ => None,
        };
        let band = band.filter(|_| tab == Tab::Regions);
        let envelope = band.map(|b| {
            let reach = |amount: f64| {
                let mut curve = settings["curve"].clone();
                curve[REGIONS[b].0.trim_start_matches("curve.")] = json!(amount);
                Curves::of(&curve.to_string()).map(|c| model::samples(&c.regions)).unwrap_or_default()
            };
            (reach(-100.0), reach(100.0))
        });
        let saved = app.rail.curves().map(|p| Saved { id: p.id.clone(), name: p.name.clone(), on: p.partial["curve"] == settings["curve"] }).collect();
        *state.snapshot.borrow_mut() = Snapshot { tab, points: points.clone(), splits };
        Shown { tab, curves, points, splits, regions, in_use, straight, agent, band, envelope, histogram, pointer, presets, saved }
    }

    fn histogram(app: &FilmlabApp, intake: &Intake, cx: &App) -> Option<Rc<Vec<f32>>> {
        let input = app.develop.read(cx).curve_input.clone().filter(|i| i.width > 0)?;
        let key = (std::sync::Arc::as_ptr(&input) as usize, intake.key());
        let state = &app.ui.curve;
        if let Some((k, h)) = &state.cache.borrow().histogram
            && *k == key
        {
            return Some(h.clone());
        }
        let profile = Rc::new(model::histogram(&input, intake));
        state.cache.borrow_mut().histogram = Some((key, profile.clone()));
        Some(profile)
    }

    pub fn lit_region(&self) -> Option<&'static str> {
        self.band.map(|b| REGIONS[b].0)
    }

    pub fn hint(&self) -> Option<String> {
        if self.tab.channel().is_none() || !self.straight {
            return None;
        }
        Some(match model::opposite(self.tab) {
            Some((other, _)) => format!("Click the line to add a point, then drag. Up adds {}, down adds {}.", self.tab.label().to_lowercase(), other.to_lowercase()),
            None => "Click the line to add a point, then drag. Up brightens, down darkens.".to_string(),
        })
    }
}

fn tab_index(tab: Tab) -> usize {
    Tab::ALL.iter().position(|t| *t == tab).unwrap_or(0)
}

pub fn tabs(app: &FilmlabApp, shown: &Shown, cx: &mut Context<FilmlabApp>) -> AnyElement {
    let segments = Tab::ALL.into_iter().map(|tab| {
        let selected = tab == shown.tab;
        let i = tab_index(tab);
        let dot = shown.in_use[i].then(|| div().flex_none().size(px(4.)).rounded_full().bg(if shown.agent[i] { theme::accent() } else { model::in_use_dot(tab) }));
        div()
            .id(SharedString::from(tab.id()))
            .relative()
            .flex_1()
            .h(px(30.))
            .px(px(6.))
            .flex()
            .items_center()
            .justify_center()
            .gap(px(4.))
            .rounded(px(6.))
            .text_size(px(13.))
            .font_weight(FontWeight::MEDIUM)
            .text_color(if selected { theme::primary() } else { theme::label() })
            .when(selected, |d| d.bg(theme::fill()))
            .when(!selected, |d| d.cursor_pointer().hover(|s| s.text_color(theme::primary())))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |app, event: &ClickEvent, _, cx| edit::pick_tab(app, tab, event.click_count() >= 2, cx)))
            .child(div().whitespace_nowrap().child(tab.label()))
            .children(dot)
            .child(app.ui.placed.record(tab.id()))
    });
    div().h(px(theme::ROW)).w_full().p(px(3.)).flex().items_center().gap(px(1.)).rounded(px(theme::RADIUS)).bg(theme::surface()).children(segments).into_any_element()
}

pub fn presets(shown: &Shown, cx: &mut Context<FilmlabApp>) -> AnyElement {
    let saved_on = shown.saved.iter().find(|s| s.on).map(|s| s.name.clone());
    let built_in = shown.presets.as_ref().and_then(|p| p["current"].as_str()).map(String::from).filter(|_| saved_on.is_none());
    let mut menu: Vec<(Entry, Option<Preset>)> =
        edit::names_in(shown.presets.as_ref()).into_iter().map(|name| (Entry::Item { label: name.clone(), checked: built_in.as_ref() == Some(&name) }, Some(Preset::BuiltIn(name)))).collect();
    if !shown.saved.is_empty() {
        menu.push((Entry::Separator, None));
        menu.push((Entry::Header("Your curves".into()), None));
        menu.extend(shown.saved.iter().map(|s| (Entry::Item { label: s.name.clone(), checked: s.on }, Some(Preset::Saved(s.id.clone())))));
    }
    menu.push((Entry::Separator, None));
    menu.push((Entry::Item { label: "Save curve…".into(), checked: false }, Some(Preset::Save)));
    let value = saved_on.or(built_in).unwrap_or_else(|| "Custom".into());
    let (entries, picks): (Vec<Entry>, Vec<Option<Preset>>) = menu.into_iter().unzip();
    let me = cx.entity();
    Picker::new(
        "tone-curve-preset",
        "Point curve",
        value,
        move || entries.clone(),
        move |i, window, cx| {
            if let Some(Some(preset)) = picks.get(i) {
                me.update(cx, |app, cx| choose(app, preset.clone(), window, cx));
            }
        },
    )
    .into_any_element()
}

pub fn bind(cx: &mut App) {
    let context = Some("ToneCurve");
    cx.bind_keys([
        KeyBinding::new("tab", NextPoint, context),
        KeyBinding::new("shift-tab", PreviousPoint, context),
        KeyBinding::new("left", NudgeLeft, context),
        KeyBinding::new("shift-left", LeapLeft, context),
        KeyBinding::new("right", NudgeRight, context),
        KeyBinding::new("shift-right", LeapRight, context),
        KeyBinding::new("up", NudgeUp, context),
        KeyBinding::new("shift-up", LeapUp, context),
        KeyBinding::new("down", NudgeDown, context),
        KeyBinding::new("shift-down", LeapDown, context),
        KeyBinding::new("backspace", RemovePoint, context),
        KeyBinding::new("delete", RemovePoint, context),
        KeyBinding::new("escape", LeaveCurve, context),
    ]);
}

fn region_text(region: usize, value: f64) -> String {
    let (id, label) = REGIONS[region];
    let shown = super::inspector::spec(id).map_or_else(|| format!("{value:.0}"), |spec| super::inspector::format(spec, value));
    format!("{label} {shown}")
}

fn point_text(at: [f64; 2]) -> String {
    format!("{} → {}", level(at[0]), level(at[1]))
}
