pub mod plans;

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use filmlab_engine::core::{self, Commands, Engine};
use gpui_kit::{AppContext, Context, Entity, Task};
use serde_json::{Value, json};

use crate::camera::{Camera, Native, Region, Size, WHOLE, Zoom, camera_of, mix_log_scale};
use crate::film::{FilmSettings, render_film};
use crate::library::{Library, PhotoState, ensure_luts};
use plans::{Inputs, Layer, Level, Plan, fit_plan, plans, same_plan};

const COALESCE: Duration = Duration::from_millis(900);
const SAVE_DELAY: Duration = Duration::from_millis(700);
const GESTURE_SETTLE: Duration = Duration::from_millis(90);
const ZOOM_ANIMATION: f64 = 0.22;
const TRIM_AFTER: Duration = Duration::from_secs(2);
/// Quiet time before neighbouring photos are rendered ahead, and before the open photo's full-size source is decoded
/// for zooming.
const IDLE_AFTER: Duration = Duration::from_millis(300);
const FULL_AFTER: Duration = Duration::from_millis(1200);
const PREFETCH: &str = "prefetch";
/// Edits closer together than this are a slider being dragged; until it rests this long, the fit view is drawn as a
/// draft.
const DRAG_GAP: Duration = Duration::from_millis(150);

#[derive(Clone, Debug)]
pub struct Step {
    pub label: String,
    pub key: String,
    pub settings: Value,
    pub film: Option<FilmSettings>,
    pub at: Instant,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    Idle,
    Loading,
    Ready,
    Failed,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Compare {
    After,
    Before,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Edit,
    Crop,
    Heal,
    Masks,
}

pub struct RenderJob {
    pub engine: Arc<Mutex<Engine>>,
    pub key: String,
    pub full: bool,
    pub settings: Value,
    pub film: Option<FilmSettings>,
    pub facts: Value,
}

pub struct DisplayFrame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub region: Region,
    pub histogram: Vec<u32>,
}

struct Animation {
    from: Camera,
    to: Camera,
    started: Instant,
}

pub struct Develop {
    library: Entity<Library>,
    pub photo_id: Option<String>,
    pub facts: Option<Value>,
    pub base: Option<PhotoState>,
    pub before: Option<Value>,
    pub settings: Option<Value>,
    pub film: Option<FilmSettings>,
    pub revision: u64,
    pub history: Vec<Step>,
    pub cursor: usize,
    pub status: Status,
    pub error: Option<String>,
    pub compare: Compare,
    pub tool: Tool,
    pub zoom: Zoom,
    pub overlay: Option<Value>,
    pub fit: Option<Arc<DisplayFrame>>,
    pub detail: Option<Arc<DisplayFrame>>,
    pub viewport: Size,
    pub camera: Camera,
    pub native: Native,
    commands: Commands,
    path: Option<std::path::PathBuf>,
    engine: Arc<Mutex<Engine>>,
    loading: HashSet<String>,
    ready: HashSet<String>,
    shown: HashMap<bool, Plan>,
    in_flight: bool,
    settle: Option<Task<()>>,
    save: Option<Task<()>>,
    trim: Option<Task<()>>,
    animation: Option<Animation>,
    last_request: Instant,
    load_token: u64,
    pub applied: Option<Applied>,
    /// Fit frames of the neighbouring photos, rendered while idle so the arrow keys show them at once.
    prerendered: HashMap<String, (Plan, Arc<DisplayFrame>)>,
    prerendering: bool,
    idle: Option<Task<()>>,
    step: isize,
    edited: Option<Instant>,
    /// Live while a slider is being dragged; it ends DRAG_GAP after the last edit and asks for the full frame.
    dragging: Option<Task<()>>,
    /// The decodes that make the next zoom or photo instant were let go while the window was in the background.
    rested: bool,
    /// Controls an agent set that nobody has touched since (control id, or `film.…` for the film look, to the value
    /// it set); the inspector draws them amber until you change them.
    pub agent_marks: HashMap<String, Value>,
    flat: HashMap<String, Value>,
}

// why: a run of slider edits in one section (exposure, then contrast, then highlights) is one change in the history
// and undoes as one; keys of slider edits are their control ids, so the section comes from the controls catalog
/// The ids whose values differ between two flattened edits. For an agent's change, a film look switched on from
/// none counts only its stocks: the halation, grain and dust defaults that come with it weren't the agent's choice.
fn changed(before: &HashMap<String, Value>, after: &HashMap<String, Value>, by_agent: bool) -> Vec<String> {
    let had_film = before.keys().any(|k| k.starts_with("film."));
    after
        .iter()
        .filter(|(id, v)| before.get(*id) != Some(*v) && !(v.is_null() && !before.contains_key(*id)))
        .filter(|(id, _)| !by_agent || had_film || !id.starts_with("film.") || matches!(id.as_str(), "film.negative" | "film.print" | "film.utility"))
        .map(|(id, _)| id.clone())
        .collect()
}

/// A photo's edit as flat ids and values: every control (`light.exposure`) and the film look (`film.grain.size`).
fn flatten(settings: &Value, film: Option<&FilmSettings>) -> HashMap<String, Value> {
    fn walk(prefix: &str, v: &Value, out: &mut HashMap<String, Value>) {
        match v {
            Value::Object(map) => map.iter().for_each(|(k, v)| walk(&format!("{prefix}.{k}"), v, out)),
            other => {
                out.insert(prefix.to_string(), other.clone());
            }
        }
    }
    let values: HashMap<String, f64> = core::control_values(&settings.to_string()).ok().and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_default();
    let mut out: HashMap<String, Value> = values.into_iter().map(|(k, v)| (k, json!(v))).collect();
    if let Some(film) = film {
        walk("film", &film.normalized().to_value(), &mut out);
    }
    out
}

fn section_of(key: &str) -> Option<&'static str> {
    static SECTIONS: std::sync::LazyLock<HashMap<String, String>> = std::sync::LazyLock::new(|| {
        let raw: Vec<Value> = core::controls().ok().and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_default();
        raw.iter().filter_map(|c| Some((c["id"].as_str()?.to_string(), c["section"].as_str()?.to_string()))).collect()
    });
    let mut sections = key.split(',').map(|id| SECTIONS.get(id).map(String::as_str));
    let first = sections.next()??;
    sections.all(|s| s == Some(first)).then(|| section_title(first))
}

fn section_title(section: &str) -> &'static str {
    match section {
        "light" => "Light",
        "curve" => "Tone curve",
        "color" => "Color",
        "mixer" => "Color mixer",
        "bwMix" | "bw_mix" => "B&W mix",
        "grading" => "Color grading",
        "effects" => "Effects",
        "vignette" => "Vignette",
        "grain" => "Grain",
        "detail" => "Detail",
        "optics" => "Optics",
        "geometry" => "Geometry",
        "profile" => "Profile",
        "calibration" => "Calibration",
        "pointColor" | "point_color" => "Point color",
        "redEye" | "red_eye" => "Red eye",
        _ => "Edit",
    }
}

fn source_key(id: &str, level: Level) -> String {
    format!("{id}#{}", level.tag())
}

fn is_detail(layer: Layer) -> bool {
    layer == Layer::Detail
}

impl Develop {
    pub fn new(library: Entity<Library>) -> Develop {
        Develop {
            library,
            photo_id: None,
            facts: None,
            base: None,
            before: None,
            settings: None,
            film: None,
            revision: 0,
            history: Vec::new(),
            cursor: 0,
            status: Status::Idle,
            error: None,
            compare: Compare::After,
            tool: Tool::Edit,
            zoom: Zoom::Fit,
            overlay: None,
            fit: None,
            detail: None,
            viewport: Size::default(),
            camera: Camera { scale: 1.0, cx: 0.5, cy: 0.5 },
            native: (1.0, 1.0),
            commands: Commands::new(),
            path: None,
            engine: Arc::new(Mutex::new(Engine::new())),
            loading: HashSet::new(),
            ready: HashSet::new(),
            shown: HashMap::new(),
            in_flight: false,
            settle: None,
            save: None,
            trim: None,
            animation: None,
            last_request: Instant::now(),
            load_token: 0,
            applied: None,
            prerendered: HashMap::new(),
            prerendering: false,
            idle: None,
            step: 1,
            edited: None,
            dragging: None,
            rested: false,
            agent_marks: HashMap::new(),
            flat: HashMap::new(),
        }
    }

    pub fn library(&self) -> &Entity<Library> {
        &self.library
    }

    /// The revision the next render plans for: the edit's, or a separate one for the before view.
    pub fn planned_revision(&self) -> u64 {
        self.revision + u64::from(self.compare == Compare::Before) * (1 << 40)
    }

    fn shown_settings(&self) -> Option<&Value> {
        if self.compare == Compare::Before { self.before.as_ref() } else { self.settings.as_ref() }
    }

    fn inputs(&self) -> Option<Inputs> {
        let settings = self.shown_settings()?.clone();
        if self.status != Status::Ready {
            return None;
        }
        let photo_id = self.photo_id.clone()?;
        let facts = self.facts.as_ref()?;
        let apply_crop = self.tool != Tool::Crop;
        let native: [f64; 2] = serde_json::from_str(&core::frame_size(&facts.to_string(), &settings.to_string(), apply_crop).ok()?).ok()?;
        let film = if self.compare == Compare::Before { None } else { render_film(self.film.as_ref()) };
        Some(Inputs {
            photo_id,
            revision: self.planned_revision(),
            settings,
            film,
            overlay: self.overlay.clone(),
            apply_crop,
            native: (native[0], native[1]),
        })
    }

    fn target_zoom(&self) -> Zoom {
        if self.tool == Tool::Edit { self.zoom } else { Zoom::Fit }
    }

    pub fn open_photo(&mut self, id: &str, cx: &mut Context<Develop>) {
        let library = self.library.read(cx);
        if let (Some(from), Some(to)) = (self.photo_id.as_deref().and_then(|p| library.index_of(p)), library.index_of(id))
            && from != to
        {
            self.step = if to > from { 1 } else { -1 };
        }
        let Some(record) = library.record(id) else { return };
        let (Some(probed), Some(state)) = (record.probed.clone(), record.state.clone()) else { return };
        let path = record.file.path.clone();
        self.flush(cx);
        self.load_token += 1;
        let token = self.load_token;
        let film = state.film();
        self.photo_id = Some(id.to_string());
        self.path = Some(path.clone());
        self.facts = Some(probed.facts.clone());
        self.before = Some(probed.defaults.clone());
        self.settings = Some(state.settings.clone());
        self.film = film.clone();
        self.base = Some(state.clone());
        self.revision += 1;
        self.history = vec![Step { label: "Opened".into(), key: "open".into(), settings: state.settings.clone(), film, at: Instant::now() }];
        self.remark();
        self.cursor = 0;
        self.status = Status::Loading;
        self.error = None;
        self.compare = Compare::After;
        self.zoom = Zoom::Fit;
        self.overlay = None;
        self.fit = None;
        self.detail = None;
        self.shown.clear();
        self.applied = None;
        self.rested = false;
        self.commands.detach();
        if let Err(e) = self.commands.open(&probed.facts.to_string(), &state.settings.to_string()) {
            self.error = Some(e);
        }
        let neighbours = self.neighbours(id, cx);
        let mut keep: HashSet<String> = [source_key(id, Level::Preview), source_key(id, Level::Full)].into();
        keep.extend(neighbours.iter().map(|(n, _, _)| source_key(n, Level::Preview)));
        let stale: Vec<String> = self.ready.iter().chain(self.loading.iter()).filter(|k| !keep.contains(*k)).cloned().collect();
        for key in &stale {
            self.ready.remove(key);
            self.loading.remove(key);
        }
        // why: a render may hold the engine for tens of milliseconds; releasing off the main thread keeps the switch smooth
        if !stale.is_empty() {
            let engine = self.engine.clone();
            cx.background_spawn(async move {
                if let Ok(mut engine) = engine.lock() {
                    for key in stale {
                        engine.release(&key);
                    }
                }
            })
            .detach();
        }
        let wanted: HashSet<&str> = neighbours.iter().map(|(n, _, _)| n.as_str()).collect();
        let mut adopt = self.prerendered.remove(id);
        self.prerendered.retain(|n, _| wanted.contains(n.as_str()));
        if self.ready.contains(&source_key(id, Level::Preview)) {
            self.status = Status::Ready;
            self.sync_camera();
            if let Some((plan, frame)) = adopt.take() {
                self.adopt(plan, frame);
            }
            self.pump(cx);
        }
        let facts = probed.facts.to_string();
        let id = id.to_string();
        cx.spawn(async move |this, cx| {
            let bytes = cx.background_spawn(async move { std::fs::read(&path).map_err(|e| e.to_string()) }).await;
            let _ = this.update(cx, |develop, cx| {
                if develop.load_token != token {
                    return;
                }
                match bytes {
                    Ok(bytes) => {
                        let bytes = Arc::new(bytes);
                        develop.commands.attach((*bytes).clone());
                        develop.load_source(&id, Level::Preview, facts, bytes, cx);
                        for (other, facts, path) in neighbours {
                            develop.preload(&other, facts, path, cx);
                        }
                    }
                    Err(e) => {
                        develop.status = Status::Failed;
                        develop.error = Some(e);
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    // why: a decode can land after the photo changed; it counts whenever its source is still wanted, which the
    // open photo's cleanup decides by clearing `loading`
    fn landed(&mut self, key: String, loaded: Result<(), String>, cx: &mut Context<Develop>) {
        let wanted = self.loading.remove(&key);
        let current = self.photo_id.as_deref().map(|id| source_key(id, Level::Preview)) == Some(key.clone());
        match loaded {
            Ok(()) if wanted => {
                self.ready.insert(key);
                if current && self.status == Status::Loading {
                    self.status = Status::Ready;
                    self.sync_camera();
                }
                self.pump(cx);
            }
            Ok(()) => {
                if let Ok(mut engine) = self.engine.lock() {
                    engine.release(&key);
                }
            }
            Err(e) if wanted && current => {
                self.status = Status::Failed;
                self.error = Some(e);
            }
            Err(_) => {}
        }
        cx.notify();
    }

    // why: the next two photos in the direction of travel and the one behind are the likely next opens
    fn neighbours(&self, id: &str, cx: &Context<Develop>) -> Vec<(String, String, std::path::PathBuf)> {
        let library = self.library.read(cx);
        let Some(at) = library.index_of(id) else { return Vec::new() };
        [self.step, -self.step, 2 * self.step]
            .into_iter()
            .filter_map(|n| at.checked_add_signed(n))
            .filter_map(|i| library.photos.get(i))
            .filter_map(|r| Some((r.file.id.clone(), r.probed.as_ref()?.facts.to_string(), r.file.path.clone())))
            .collect()
    }

    // why: arrow keys open the next photo at once when its preview is already decoded
    fn preload(&mut self, id: &str, facts: String, path: std::path::PathBuf, cx: &mut Context<Develop>) {
        let key = source_key(id, Level::Preview);
        if self.loading.contains(&key) || self.ready.contains(&key) {
            return;
        }
        self.loading.insert(key.clone());
        let engine = self.engine.clone();
        cx.spawn(async move |this, cx| {
            let loaded = {
                let key = key.clone();
                cx.background_spawn(async move {
                    let decoded = crate::perf::initiated(|| {
                        let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
                        core::decode(&facts, &bytes, Level::Preview.max_edge())
                    })?;
                    engine.lock().map_err(|e| e.to_string())?.insert(&key, decoded);
                    Ok::<(), String>(())
                })
                .await
            };
            let _ = this.update(cx, |develop, cx| develop.landed(key, loaded, cx));
        })
        .detach();
    }

    // why: the raw decode runs without the engine's lock, so renders of what's on screen never wait for it
    fn load_source(&mut self, id: &str, level: Level, facts: String, bytes: Arc<Vec<u8>>, cx: &mut Context<Develop>) {
        let key = source_key(id, level);
        if self.loading.contains(&key) || self.ready.contains(&key) {
            return;
        }
        self.loading.insert(key.clone());
        let engine = self.engine.clone();
        cx.spawn(async move |this, cx| {
            let loaded = {
                let key = key.clone();
                cx.background_spawn(async move {
                    let started = Instant::now();
                    let decode = || core::decode(&facts, &bytes, level.max_edge());
                    let decoded = if level == Level::Preview { crate::perf::interactive(decode) } else { crate::perf::initiated(decode) }?;
                    if std::env::var_os("FILMLAB_TRACE").is_some() {
                        eprintln!("decode {key} {:.1} ms", started.elapsed().as_secs_f64() * 1000.0);
                    }
                    engine.lock().map_err(|e| e.to_string())?.insert(&key, decoded);
                    Ok::<(), String>(())
                })
                .await
            };
            let _ = this.update(cx, |develop, cx| develop.landed(key, loaded, cx));
        })
        .detach();
    }

    fn runnable(&self, plan: &Plan) -> Option<Plan> {
        if self.ready.contains(&source_key(&plan.photo_id, plan.level)) {
            return Some(plan.clone());
        }
        (plan.layer == Layer::Fit && self.ready.contains(&source_key(&plan.photo_id, Level::Preview))).then(|| Plan { level: Level::Preview, ..plan.clone() })
    }

    fn next_plan(&self) -> Option<Plan> {
        let inputs = self.inputs()?;
        let drafting = self.dragging.is_some() && self.tool == Tool::Edit;
        for wanted in plans(&inputs, self.target_zoom(), self.viewport, self.shown.get(&true)) {
            let wanted = if drafting && wanted.layer == Layer::Fit { plans::draft_plan(&wanted) } else { wanted };
            if same_plan(self.shown.get(&is_detail(wanted.layer)), &wanted) {
                continue;
            }
            if let Some(plan) = self.runnable(&wanted)
                && !same_plan(self.shown.get(&is_detail(plan.layer)), &plan)
            {
                return Some(plan);
            }
        }
        None
    }

    fn drop_stale_detail(&mut self) {
        let stale = self.detail.is_some() && self.inputs().is_none_or(|i| self.shown.get(&true).is_none_or(|p| p.photo_id != i.photo_id || p.revision != i.revision));
        if stale && self.target_zoom() == Zoom::Fit {
            self.shown.remove(&true);
            self.detail = None;
        }
    }

    pub fn pump(&mut self, cx: &mut Context<Develop>) {
        if self.in_flight || self.settle.is_some() {
            return;
        }
        self.drop_stale_detail();
        let Some(plan) = self.next_plan() else {
            self.schedule_trim(cx);
            self.schedule_idle(cx);
            return;
        };
        self.trim = None;
        self.idle = None;
        self.in_flight = true;
        let engine = self.engine.clone();
        let key = source_key(&plan.photo_id, plan.level);
        let mut view = plan.view.clone();
        if let Some(o) = &plan.overlay {
            view["overlay"] = o.clone();
        }
        if let Some(f) = &plan.film {
            view["film"] = f.to_value();
        }
        let settings = plan.settings.to_string();
        let film = plan.film.clone();
        let started = Instant::now();
        cx.spawn(async move |this, cx| {
            let rendered = cx
                .background_spawn(async move {
                    // why: the lock is taken outside the render pool; a pool thread waiting on it while helping a
                    // render that holds it would deadlock the pool
                    let mut engine = engine.lock().map_err(|e| e.to_string())?;
                    let engine = &mut *engine;
                    ensure_luts(engine, film.as_ref())?;
                    crate::perf::interactive(|| engine.render(&key, &settings, &view.to_string()))
                })
                .await;
            if std::env::var_os("FILMLAB_TRACE").is_some()
                && let Ok(f) = &rendered
            {
                eprintln!("render {:?} {}x{} {:.1} ms rev {} {:?} {}", plan.layer, f.width, f.height, started.elapsed().as_secs_f64() * 1000.0, plan.revision, plan.level, plan.view);
            }
            let _ = this.update(cx, |develop, cx| {
                develop.in_flight = false;
                match rendered {
                    Ok(mut f) => {
                        if develop.photo_id.as_deref() == Some(plan.photo_id.as_str()) {
                            let region = match (plan.layer, plan.view["window"].as_array()) {
                                (Layer::Detail, Some(w)) => {
                                    let (ww, wh) = (w[2].as_f64().unwrap_or(1.0), w[3].as_f64().unwrap_or(1.0));
                                    let r = plan.region;
                                    Region { u0: r.u0, v0: r.v0, u1: r.u0 + (r.u1 - r.u0) * f64::from(f.width) / ww, v1: r.v0 + (r.v1 - r.v0) * f64::from(f.height) / wh }
                                }
                                _ => WHOLE,
                            };
                            let frame = Arc::new(DisplayFrame { width: f.width, height: f.height, rgba: f.take_rgba(), region, histogram: f.take_histogram() });
                            match plan.layer {
                                Layer::Fit => develop.fit = Some(frame),
                                Layer::Detail => develop.detail = Some(frame),
                            }
                            develop.shown.insert(is_detail(plan.layer), plan);
                        }
                    }
                    Err(e) => {
                        develop.status = Status::Failed;
                        develop.error = Some(e);
                    }
                }
                cx.notify();
                develop.pump(cx);
            });
        })
        .detach();
    }

    fn sync_camera(&mut self) {
        if self.animation.is_some() {
            return;
        }
        if let Some(inputs) = self.inputs() {
            self.native = inputs.native;
            self.camera = camera_of(self.target_zoom(), inputs.native, self.viewport);
        }
    }

    pub fn request_render(&mut self, cx: &mut Context<Develop>) {
        self.last_request = Instant::now();
        self.sync_camera();
        self.pump(cx);
        cx.notify();
    }

    pub fn set_viewport(&mut self, viewport: Size, cx: &mut Context<Develop>) {
        let next = Size { width: viewport.width.round(), height: viewport.height.round() };
        if next == self.viewport {
            return;
        }
        self.viewport = next;
        self.request_render(cx);
    }

    pub fn idle_for(&self, wait: Duration) -> bool {
        !self.in_flight && self.last_request.elapsed() >= wait
    }

    pub fn tick(&mut self) -> bool {
        let Some(a) = &self.animation else { return false };
        let t = (a.started.elapsed().as_secs_f64() / ZOOM_ANIMATION).min(1.0);
        let eased = 1.0 - (1.0 - t).powi(3);
        self.camera = mix_log_scale(a.from, a.to, eased);
        if t >= 1.0 {
            self.animation = None;
        }
        true
    }

    // why: only a zoomed view needs the full-size decode (about 300 MB for a 24 MP raw), so it is read on the
    // first zoom rather than with every photo
    pub fn ensure_full(&mut self, cx: &mut Context<Develop>) {
        self.decode_full(true, cx);
    }

    // why: a zoom waits on this decode, so it takes the fast cores; decoding ahead while the user only looks yields to
    // their edits
    fn decode_full(&mut self, urgent: bool, cx: &mut Context<Develop>) {
        let (Some(id), Some(path), Some(facts)) = (self.photo_id.clone(), self.path.clone(), self.facts.clone()) else { return };
        let key = source_key(&id, Level::Full);
        if self.ready.contains(&key) || self.loading.contains(&key) {
            return;
        }
        let long = facts["width"].as_f64().unwrap_or(0.0).max(facts["height"].as_f64().unwrap_or(0.0));
        if long <= plans::PREVIEW_EDGE {
            return;
        }
        self.loading.insert(key.clone());
        let engine = self.engine.clone();
        let facts = facts.to_string();
        cx.spawn(async move |this, cx| {
            let loaded = {
                let key = key.clone();
                cx.background_spawn(async move {
                    let decode = || {
                        let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
                        core::decode(&facts, &bytes, Level::Full.max_edge())
                    };
                    let decoded = if urgent { crate::perf::initiated(decode) } else { crate::perf::background(decode) }?;
                    engine.lock().map_err(|e| e.to_string())?.insert(&key, decoded);
                    Ok::<(), String>(())
                })
                .await
            };
            let _ = this.update(cx, |develop, cx| develop.landed(key, loaded, cx));
        })
        .detach();
    }

    // why: a zoomed window's caches (full-size stages, its film scene, the whole-frame look) are worth keeping only
    // while zoomed
    fn forget_zoom(&mut self) {
        let Some(id) = self.photo_id.clone() else { return };
        self.detail = None;
        self.shown.remove(&true);
        if let Ok(mut engine) = self.engine.lock() {
            for level in [Level::Preview, Level::Full] {
                engine.forget_view(&source_key(&id, level), "loupe-zoom");
            }
        }
    }

    /// Lets go of everything the open photo holds (decodes, caches, frames), for when Develop isn't on screen.
    pub fn unload(&mut self, cx: &mut Context<Develop>) {
        self.flush(cx);
        self.load_token += 1;
        let keys: Vec<String> = self.ready.drain().chain(self.loading.drain()).collect();
        if let Ok(mut engine) = self.engine.lock() {
            for key in keys {
                engine.release(&key);
            }
        }
        self.commands.detach();
        self.photo_id = None;
        self.path = None;
        self.fit = None;
        self.detail = None;
        self.shown.clear();
        self.prerendered.clear();
        self.rested = false;
        self.status = Status::Idle;
        crate::gpu::reset();
        crate::perf::give_back();
        cx.notify();
    }

    // why: the neighbours' previews stay, so the arrow keys are instant on coming back; only the first zoom waits
    /// Lets go of the open photo's full-size decode, which only makes zoom instant, once Develop has been quiet for
    /// `quiet` at the fit view; `wake` decodes it again.
    pub fn rest(&mut self, quiet: Duration, cx: &mut Context<Develop>) {
        let Some(id) = self.photo_id.clone() else { return };
        if self.status != Status::Ready || self.target_zoom() != Zoom::Fit || !self.loading.is_empty() || self.prerendering || !self.idle_for(quiet) {
            return;
        }
        self.rested = true;
        let full = source_key(&id, Level::Full);
        if !self.ready.remove(&full) {
            return;
        }
        let engine = self.engine.clone();
        cx.background_spawn(async move {
            if let Ok(mut engine) = engine.lock() {
                engine.release(&full);
            }
            crate::perf::give_back();
        })
        .detach();
    }

    // why: one at a time, each on every core, so the first is ready in about the time of one cold open; side by side
    // they all took several times that, and an arrow key or zoom right after coming back waited on all of them
    /// Decodes again what `rest` let go of, and any neighbour preview missing by then: the next photo's first, then
    /// the open photo's full-size source, then the others.
    pub fn wake(&mut self, cx: &mut Context<Develop>) {
        if !std::mem::take(&mut self.rested) {
            return;
        }
        let (Some(id), Some(path), Some(facts)) = (self.photo_id.clone(), self.path.clone(), self.facts.clone()) else { return };
        let neighbours = self.neighbours(&id, cx).into_iter().map(|(other, facts, path)| (source_key(&other, Level::Preview), facts, path, Level::Preview));
        let mut wanted: Vec<(String, String, std::path::PathBuf, Level)> = neighbours.collect();
        let long = facts["width"].as_f64().unwrap_or(0.0).max(facts["height"].as_f64().unwrap_or(0.0));
        if long > plans::PREVIEW_EDGE && crate::perf::memory_bytes() >= 16 << 30 {
            wanted.insert(wanted.len().min(1), (source_key(&id, Level::Full), facts.to_string(), path, Level::Full));
        }
        wanted.retain(|(key, ..)| !self.ready.contains(key) && !self.loading.contains(key));
        self.loading.extend(wanted.iter().map(|(key, ..)| key.clone()));
        let engine = self.engine.clone();
        cx.spawn(async move |this, cx| {
            for (key, facts, path, level) in wanted {
                // why: a photo opened meanwhile takes what it no longer needs out of `loading`
                if !this.read_with(cx, |develop, _| develop.loading.contains(&key)).unwrap_or(false) {
                    continue;
                }
                let (engine, inserted) = (engine.clone(), key.clone());
                let loaded = cx
                    .background_spawn(async move {
                        let decoded = crate::perf::initiated(|| {
                            let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
                            core::decode(&facts, &bytes, level.max_edge())
                        })?;
                        engine.lock().map_err(|e| e.to_string())?.insert(&inserted, decoded);
                        Ok::<(), String>(())
                    })
                    .await;
                if this.update(cx, |develop, cx| develop.landed(key, loaded, cx)).is_err() {
                    return;
                }
            }
        })
        .detach();
    }

    pub fn set_zoom(&mut self, zoom: Zoom, animate: bool, gesture: bool, cx: &mut Context<Develop>) {
        self.last_request = Instant::now();
        if zoom == Zoom::Fit && self.zoom != Zoom::Fit {
            self.forget_zoom();
        } else if zoom != Zoom::Fit {
            self.ensure_full(cx);
        }
        self.zoom = zoom;
        let target = camera_of(zoom, self.native, self.viewport);
        if animate {
            self.animation = Some(Animation { from: self.camera, to: target, started: Instant::now() });
        } else {
            self.animation = None;
            self.camera = target;
        }
        self.settle = None;
        if gesture {
            self.settle = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(GESTURE_SETTLE).await;
                let _ = this.update(cx, |develop, cx| {
                    develop.settle = None;
                    develop.pump(cx);
                });
            }));
        } else {
            self.pump(cx);
        }
        cx.notify();
    }

    /// Shows a neighbour's frame rendered ahead, when it was made from the same settings at the same size.
    fn adopt(&mut self, plan: Plan, frame: Arc<DisplayFrame>) {
        let Some(inputs) = self.inputs() else { return };
        let wanted = fit_plan(&inputs, self.viewport);
        if self.target_zoom() == Zoom::Fit && plan.look == wanted.look && plan.settings == wanted.settings && plan.view == wanted.view && plan.level == wanted.level {
            self.fit = Some(frame);
            self.shown.insert(false, Plan { revision: wanted.revision, ..plan });
        }
    }

    fn schedule_idle(&mut self, cx: &mut Context<Develop>) {
        self.idle = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(IDLE_AFTER).await;
            let _ = this.update(cx, |develop, cx| {
                develop.idle = None;
                develop.ahead(cx);
            });
        }));
    }

    fn neighbour_inputs(&self, id: &str, cx: &Context<Develop>) -> Option<Inputs> {
        let library = self.library.read(cx);
        let record = library.record(id)?;
        let (probed, state) = (record.probed.as_ref()?, record.state.as_ref()?);
        let native: [f64; 2] = serde_json::from_str(&core::frame_size(&probed.facts.to_string(), &state.settings.to_string(), true).ok()?).ok()?;
        Some(Inputs { photo_id: id.to_string(), revision: 0, settings: state.settings.clone(), film: render_film(state.film().as_ref()), overlay: None, apply_crop: true, native: (native[0], native[1]) })
    }

    // why: while the user looks at a photo, the neighbours' fit frames are rendered one at a time (and their stage caches
    // dropped) so switching shows the finished photo at once; then the open photo's full-size source is decoded so the
    // first zoom is quick, on Macs with the memory for it
    fn ahead(&mut self, cx: &mut Context<Develop>) {
        if self.prerendering || self.in_flight || self.settle.is_some() || self.animation.is_some() || self.viewport.width < 1.0 {
            return;
        }
        let Some(current) = self.photo_id.clone() else { return };
        for (id, _, _) in self.neighbours(&current, cx) {
            let key = source_key(&id, Level::Preview);
            if !self.ready.contains(&key) {
                continue;
            }
            let Some(inputs) = self.neighbour_inputs(&id, cx) else { continue };
            let plan = fit_plan(&inputs, self.viewport);
            let fresh = self.prerendered.get(&id).is_some_and(|(p, _)| p.look == plan.look && p.settings == plan.settings && p.view == plan.view);
            if fresh || plan.level != Level::Preview {
                continue;
            }
            self.prerendering = true;
            let engine = self.engine.clone();
            let mut view = plan.view.clone();
            view["stages"] = json!(PREFETCH);
            if let Some(f) = &plan.film {
                view["film"] = f.to_value();
            }
            let (settings, film) = (plan.settings.to_string(), plan.film.clone());
            cx.spawn(async move |this, cx| {
                let rendered = cx
                    .background_spawn(async move {
                        let mut engine = engine.lock().map_err(|e| e.to_string())?;
                        let engine = &mut *engine;
                        ensure_luts(engine, film.as_ref())?;
                        let frame = crate::perf::initiated(|| engine.render(&key, &settings, &view.to_string()));
                        engine.forget_view(&key, PREFETCH);
                        frame
                    })
                    .await;
                let _ = this.update(cx, |develop, cx| {
                    develop.prerendering = false;
                    if let Ok(mut f) = rendered {
                        let frame = Arc::new(DisplayFrame { width: f.width, height: f.height, rgba: f.take_rgba(), region: WHOLE, histogram: f.take_histogram() });
                        develop.prerendered.insert(id, (plan, frame));
                    }
                    if develop.last_request.elapsed() >= IDLE_AFTER {
                        develop.ahead(cx);
                    }
                });
            })
            .detach();
            return;
        }
        let opened = self.history.first().map_or(Duration::ZERO, |s| s.at.elapsed());
        if opened < FULL_AFTER {
            let wait = FULL_AFTER - opened;
            self.idle = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(wait).await;
                let _ = this.update(cx, |develop, cx| {
                    develop.idle = None;
                    develop.ahead(cx);
                });
            }));
        } else if crate::perf::memory_bytes() >= 16 << 30 {
            self.decode_full(false, cx);
        }
    }

    fn schedule_trim(&mut self, cx: &mut Context<Develop>) {
        self.trim = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(TRIM_AFTER).await;
            let _ = this.update(cx, |develop, _| {
                develop.trim = None;
                crate::gpu::trim();
                crate::perf::give_back();
            });
        }));
    }

    fn schedule_save(&mut self, cx: &mut Context<Develop>) {
        self.save = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SAVE_DELAY).await;
            let _ = this.update(cx, |develop, cx| {
                develop.save = None;
                develop.save_now(cx);
            });
        }));
    }

    fn save_now(&mut self, cx: &mut Context<Develop>) {
        let (Some(id), Some(base), Some(settings)) = (self.photo_id.clone(), self.base.clone(), self.settings.clone()) else { return };
        // why: flags, rating and label can change in the library while the photo is open (grid, agents); an edit keeps them
        let base = self.library.read(cx).record(&id).and_then(|r| r.state.clone()).unwrap_or(base);
        let state = PhotoState { settings, film: self.film.as_ref().map(|f| f.normalized().to_value()), ..base };
        self.library.update(cx, |library, cx| library.persist(&id, state, cx));
    }

    /// An edit is waiting for the save delay before it's written.
    pub fn unsaved(&self) -> bool {
        self.save.is_some()
    }

    pub fn flush(&mut self, cx: &mut Context<Develop>) {
        if self.save.take().is_some() {
            self.save_now(cx);
        }
    }

    fn commit(&mut self, settings: Value, label: &str, key: &str, film: Option<FilmSettings>, cx: &mut Context<Develop>) {
        if key.starts_with("profile")
            && let Some(facts) = &self.facts
        {
            let _ = self.commands.open(&facts.to_string(), &settings.to_string());
        }
        let appending = self.cursor + 1 == self.history.len();
        self.history.truncate(self.cursor + 1);
        let now = Instant::now();
        let section = section_of(key).filter(|_| appending && self.history.len() > 1 && !key.starts_with("agent."));
        let mut label = label.to_string();
        if let Some(last) = self.history.last() {
            let same_run = section.is_some() && section_of(&last.key) == section;
            if same_run || (last.key == key && now.duration_since(last.at) < COALESCE) {
                if last.key != key && let Some(title) = section {
                    label = title.to_string();
                }
                self.history.pop();
            }
        }
        self.history.push(Step { label, key: key.to_string(), settings: settings.clone(), film: film.clone(), at: now });
        self.cursor = self.history.len() - 1;
        self.settings = Some(settings);
        self.film = film;
        self.revision += 1;
        self.compare = Compare::After;
        self.mark(key.starts_with("agent."));
        if self.edited.is_some_and(|t| now.duration_since(t) < DRAG_GAP) {
            self.drag(cx);
        }
        self.edited = Some(now);
        self.request_render(cx);
        self.schedule_save(cx);
    }

    // why: a slider drag sends an edit every frame; drawing each at a quarter of the pixels keeps up with the hand
    // (about 4x faster), and the full frame follows as soon as the slider rests
    fn drag(&mut self, cx: &mut Context<Develop>) {
        self.dragging = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(DRAG_GAP).await;
            let _ = this.update(cx, |develop, cx| {
                develop.dragging = None;
                develop.pump(cx);
            });
        }));
    }

    /// A slider was let go: the full frame is drawn now rather than when the drag times out.
    pub fn release(&mut self, cx: &mut Context<Develop>) {
        if self.dragging.take().is_some() {
            self.pump(cx);
        }
    }

    /// The fit frame on screen is a draft drawn mid-drag; the full frame follows.
    pub fn drafting(&self) -> bool {
        self.dragging.is_some() || self.shown.get(&false).is_some_and(plans::is_draft)
    }

    pub fn command(&mut self, command: &str, params: Value, label: &str, key: &str, cx: &mut Context<Develop>) -> Option<Value> {
        self.settings.as_ref()?;
        let out = match self.commands.run(command, &params.to_string()) {
            Ok(out) => out,
            Err(e) => {
                self.error = Some(e);
                cx.notify();
                return None;
            }
        };
        let parsed: Value = serde_json::from_str(&out).ok()?;
        let film = self.film.clone();
        self.commit(parsed["settings"].clone(), label, key, film, cx);
        Some(parsed["result"].clone())
    }

    /// Runs a LightCraft command for an agent: the engine's own error on failure, and a history step only when
    /// the photo's settings changed. Returns the command's result and whether the photo changed.
    pub fn try_command(&mut self, command: &str, params: Value, label: &str, key: &str, cx: &mut Context<Develop>) -> Result<(Value, bool), String> {
        let current = self.settings.clone().ok_or("no photo is open; open_photo first")?;
        let out = self.commands.run(command, &params.to_string())?;
        let mut parsed: Value = serde_json::from_str(&out).map_err(|e| e.to_string())?;
        let changed = parsed["settings"] != current;
        if changed {
            let film = self.film.clone();
            self.commit(parsed["settings"].take(), label, key, film, cx);
        }
        Ok((parsed["result"].take(), changed))
    }

    /// Every LightCraft command as (id, params doc).
    pub fn command_catalog(&self) -> Vec<(&'static str, &'static str)> {
        self.commands.catalog()
    }

    pub fn command_label(&self, id: &str) -> Option<&'static str> {
        self.commands.label_of(id)
    }

    /// Puts back a saved state of the open photo (settings and film) as one new history step.
    pub fn restore(&mut self, settings: Value, film: Option<FilmSettings>, label: &str, key: &str, cx: &mut Context<Develop>) -> Result<(), String> {
        let facts = self.facts.as_ref().ok_or("no photo is open; open_photo first")?;
        self.commands.open(&facts.to_string(), &settings.to_string())?;
        self.commit(settings, label, key, film, cx);
        Ok(())
    }

    /// Runs a LightCraft command that changes the session but not the photo (e.g. `mask.select`), so it
    /// leaves history, rendering and saving alone.
    pub fn run(&mut self, command: &str, params: Value) -> Option<Value> {
        let out = self.commands.run(command, &params.to_string()).ok()?;
        serde_json::from_str::<Value>(&out).ok().map(|v| v["result"].clone())
    }

    pub fn edit_controls(&mut self, values: &[(&str, f64)], label: &str, cx: &mut Context<Develop>) {
        let mut keys: Vec<&str> = values.iter().map(|(k, _)| *k).collect();
        keys.sort_unstable();
        // why: the Temp slider is in kelvin for raw photos and -100..100 for rendered ones, while develop.set stores kelvin
        let facts = self.facts.as_ref().map(Value::to_string).unwrap_or_default();
        let setting = |k: &str, v: f64| if k == "wb.temp" { core::temp_setting(&facts, v).unwrap_or(v) } else { v };
        let map: serde_json::Map<String, Value> = values.iter().map(|(k, v)| ((*k).to_string(), json!(setting(k, *v)))).collect();
        self.command("develop.set", json!({"values": map}), label, &keys.join(","), cx);
    }

    pub fn edit_settings(&mut self, partial: Value, label: &str, key: &str, cx: &mut Context<Develop>) {
        self.command("develop.merge", json!({"settings": partial, "label": label}), label, key, cx);
    }

    pub fn edit_film(&mut self, film: FilmSettings, label: &str, key: &str, cx: &mut Context<Develop>) {
        let Some(settings) = self.settings.clone() else { return };
        self.commit(settings, label, key, Some(film), cx);
    }

    pub fn go_to(&mut self, index: usize, cx: &mut Context<Develop>) {
        let Some(step) = self.history.get(index).cloned() else { return };
        self.settings = Some(step.settings);
        self.film = step.film;
        self.cursor = index;
        self.revision += 1;
        self.compare = Compare::After;
        if let Some(facts) = &self.facts
            && let Some(settings) = &self.settings
        {
            let _ = self.commands.open(&facts.to_string(), &settings.to_string());
        }
        self.remark();
        self.request_render(cx);
        self.schedule_save(cx);
    }

    /// Updates the agent's marks after a change: an agent's edit marks what it changed, anyone else's clears it.
    fn mark(&mut self, by_agent: bool) {
        let now = self.settings.as_ref().map(|s| flatten(s, self.film.as_ref())).unwrap_or_default();
        for id in changed(&self.flat, &now, by_agent) {
            if by_agent {
                self.agent_marks.insert(id.clone(), now[&id].clone());
            } else {
                self.agent_marks.remove(&id);
            }
        }
        if !by_agent {
            self.agent_marks.retain(|id, _| now.contains_key(id));
        }
        self.flat = now;
    }

    // why: undo, redo and a photo opened mid-session move through history at once, so the marks are replayed from the
    // first step up to the current one; an agent change you undid, or one you edited after, isn't marked
    pub fn remark(&mut self) {
        self.agent_marks.clear();
        let steps: Vec<(bool, HashMap<String, Value>)> = self.history.iter().take(self.cursor + 1).map(|s| (s.key.starts_with("agent."), flatten(&s.settings, s.film.as_ref()))).collect();
        let mut previous: Option<&HashMap<String, Value>> = None;
        for (by_agent, flat) in &steps {
            if let Some(before) = previous {
                for id in changed(before, flat, *by_agent) {
                    if *by_agent {
                        self.agent_marks.insert(id.clone(), flat[&id].clone());
                    } else {
                        self.agent_marks.remove(&id);
                    }
                }
            }
            previous = Some(flat);
        }
        self.flat = steps.into_iter().last().map(|(_, f)| f).unwrap_or_default();
    }

    pub fn undo(&mut self, cx: &mut Context<Develop>) {
        if self.cursor > 0 {
            self.go_to(self.cursor - 1, cx);
        }
    }

    pub fn redo(&mut self, cx: &mut Context<Develop>) {
        self.go_to(self.cursor + 1, cx);
    }

    pub fn set_tool(&mut self, tool: Tool, cx: &mut Context<Develop>) {
        self.tool = tool;
        if tool != Tool::Masks {
            self.overlay = None;
        }
        self.request_render(cx);
    }

    pub fn set_overlay(&mut self, overlay: Option<Value>, cx: &mut Context<Develop>) {
        self.overlay = overlay;
        self.request_render(cx);
    }

    pub fn set_compare(&mut self, compare: Compare, cx: &mut Context<Develop>) {
        self.compare = compare;
        self.request_render(cx);
    }

    /// What an off-screen render of the open photo needs: its engine, decoded sources and settings. `before` takes
    /// the photo as it was opened, without film.
    pub fn render_job(&self, before: bool) -> Option<RenderJob> {
        let id = self.photo_id.clone()?;
        let settings = if before { self.before.clone()? } else { self.settings.clone()? };
        let film = if before { None } else { render_film(self.film.as_ref()) };
        let ready = |level| self.ready.contains(&source_key(&id, level));
        let level = if ready(Level::Full) { Level::Full } else if ready(Level::Preview) { Level::Preview } else { return None };
        Some(RenderJob { engine: self.engine.clone(), key: source_key(&id, level), full: level == Level::Full, settings, film, facts: self.facts.clone()? })
    }

    /// The engine and key holding `id`'s decode at `max_edge` (0: full size), when it is the open photo and that
    /// decode is ready.
    pub fn decode_at(&self, id: &str, max_edge: usize) -> Option<(Arc<Mutex<Engine>>, String)> {
        let level = [Level::Preview, Level::Full].into_iter().find(|l| l.max_edge() == max_edge)?;
        let key = source_key(self.photo_id.as_deref().filter(|open| *open == id)?, level);
        self.ready.contains(&key).then(|| (self.engine.clone(), key))
    }

    /// Whether the full-resolution source is decoded, or the photo is small enough that the preview is full size.
    pub fn full_ready(&self) -> bool {
        let Some(id) = &self.photo_id else { return false };
        let long = self.facts.as_ref().map_or(0.0, |f| f["width"].as_f64().unwrap_or(0.0).max(f["height"].as_f64().unwrap_or(0.0)));
        self.ready.contains(&source_key(id, Level::Full)) || (long <= plans::PREVIEW_EDGE && self.ready.contains(&source_key(id, Level::Preview)))
    }

    /// The plan behind the frame on screen (the fit frame, or the zoomed window's).
    pub fn shown_plan(&self, detail: bool) -> Option<&Plan> {
        self.shown.get(&detail)
    }

    pub fn control_values(&self) -> HashMap<String, f64> {
        self.settings.as_ref().and_then(|s| core::control_values(&s.to_string()).ok()).and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_default()
    }

    // why: re-applying the same profile at the same step starts again from
    // the settings it first saw, so the amount replaces the earlier application instead of stacking
    pub fn apply_profile(&mut self, profile_id: &str, partial: &Value, amount: f64, label: &str, film: Option<FilmSettings>, cx: &mut Context<Develop>) {
        let Some(settings) = self.settings.clone() else { return };
        let before = match &self.applied {
            Some(a) if a.profile_id == profile_id && a.step == self.cursor && a.photo_id == self.photo_id => a.before.clone(),
            _ => settings,
        };
        let Ok(out) = core::apply_profile(&before.to_string(), &partial.to_string(), amount) else { return };
        let Ok(next) = serde_json::from_str::<Value>(&out) else { return };
        let film = film.or_else(|| self.film.clone());
        self.commit(next, label, &format!("profile:{profile_id}"), film, cx);
        self.applied = Some(Applied { profile_id: profile_id.to_string(), before, step: self.cursor, photo_id: self.photo_id.clone() });
    }
}

pub struct Applied {
    pub profile_id: String,
    pub before: Value,
    pub step: usize,
    pub photo_id: Option<String>,
}
