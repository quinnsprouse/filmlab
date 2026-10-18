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
use plans::{Inputs, Layer, Level, Plan, plans, same_plan};

const COALESCE: Duration = Duration::from_millis(900);
const SAVE_DELAY: Duration = Duration::from_millis(700);
const GESTURE_SETTLE: Duration = Duration::from_millis(90);
const ZOOM_ANIMATION: f64 = 0.22;
const TRIM_AFTER: Duration = Duration::from_secs(2);

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
        }
    }

    pub fn library(&self) -> &Entity<Library> {
        &self.library
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
            revision: self.revision + u64::from(self.compare == Compare::Before) * (1 << 40),
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
        self.commands.detach();
        if let Err(e) = self.commands.open(&probed.facts.to_string(), &state.settings.to_string()) {
            self.error = Some(e);
        }
        let neighbours = self.neighbours(id, cx);
        let mut keep: HashSet<String> = [source_key(id, Level::Preview), source_key(id, Level::Full)].into();
        keep.extend(neighbours.iter().map(|(n, _, _)| source_key(n, Level::Preview)));
        let stale: Vec<String> = self.ready.iter().chain(self.loading.iter()).filter(|k| !keep.contains(*k)).cloned().collect();
        for key in stale {
            self.ready.remove(&key);
            self.loading.remove(&key);
            if let Ok(mut engine) = self.engine.lock() {
                engine.release(&key);
            }
        }
        if self.ready.contains(&source_key(id, Level::Preview)) {
            self.status = Status::Ready;
            self.sync_camera();
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

    fn neighbours(&self, id: &str, cx: &Context<Develop>) -> Vec<(String, String, std::path::PathBuf)> {
        let library = self.library.read(cx);
        let Some(at) = library.index_of(id) else { return Vec::new() };
        [at + 1, at.wrapping_sub(1)]
            .into_iter()
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
                    crate::perf::background(|| {
                        let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
                        let decoded = core::decode(&facts, &bytes, Level::Preview.max_edge())?;
                        engine.lock().map_err(|e| e.to_string())?.insert(&key, decoded);
                        Ok::<(), String>(())
                    })
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
                    let decode = || {
                        let decoded = core::decode(&facts, &bytes, level.max_edge())?;
                        engine.lock().map_err(|e| e.to_string())?.insert(&key, decoded);
                        Ok::<(), String>(())
                    };
                    if level == Level::Preview { crate::perf::interactive(decode) } else { crate::perf::initiated(decode) }
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
        for wanted in plans(&inputs, self.target_zoom(), self.viewport, self.shown.get(&true)) {
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
            return;
        };
        self.trim = None;
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
                    crate::perf::interactive(|| {
                        let mut engine = engine.lock().map_err(|e| e.to_string())?;
                        ensure_luts(&mut engine, film.as_ref())?;
                        engine.render(&key, &settings, &view.to_string())
                    })
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
    fn ensure_full(&mut self, cx: &mut Context<Develop>) {
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
                    crate::perf::initiated(|| {
                        let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
                        let decoded = core::decode(&facts, &bytes, Level::Full.max_edge())?;
                        engine.lock().map_err(|e| e.to_string())?.insert(&key, decoded);
                        Ok::<(), String>(())
                    })
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
        self.status = Status::Idle;
        crate::gpu::reset();
        crate::perf::give_back();
        cx.notify();
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
        let state = PhotoState { settings, film: self.film.as_ref().map(|f| f.normalized().to_value()), ..base };
        self.library.update(cx, |library, cx| library.persist(&id, state, cx));
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
        self.history.truncate(self.cursor + 1);
        let now = Instant::now();
        let coalesce = self.history.last().is_some_and(|last| last.key == key && now.duration_since(last.at) < COALESCE);
        if coalesce {
            self.history.pop();
        }
        self.history.push(Step { label: label.to_string(), key: key.to_string(), settings: settings.clone(), film: film.clone(), at: now });
        self.cursor = self.history.len() - 1;
        self.settings = Some(settings);
        self.film = film;
        self.revision += 1;
        self.compare = Compare::After;
        self.request_render(cx);
        self.schedule_save(cx);
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

    /// Runs a LightCraft command that changes the session but not the photo (e.g. `mask.select`), so it
    /// leaves history, rendering and saving alone.
    pub fn run(&mut self, command: &str, params: Value) -> Option<Value> {
        let out = self.commands.run(command, &params.to_string()).ok()?;
        serde_json::from_str::<Value>(&out).ok().map(|v| v["result"].clone())
    }

    pub fn edit_controls(&mut self, values: &[(&str, f64)], label: &str, cx: &mut Context<Develop>) {
        let mut keys: Vec<&str> = values.iter().map(|(k, _)| *k).collect();
        keys.sort_unstable();
        let map: serde_json::Map<String, Value> = values.iter().map(|(k, v)| ((*k).to_string(), json!(v))).collect();
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
        self.request_render(cx);
        self.schedule_save(cx);
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

    pub fn control_values(&self) -> HashMap<String, f64> {
        self.settings.as_ref().and_then(|s| core::control_values(&s.to_string()).ok()).and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_default()
    }

    // why: port of the web's `applyProfileTo`; re-applying the same profile at the same step starts again from
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
