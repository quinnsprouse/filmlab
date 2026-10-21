use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use filmlab_engine::core::{self, Engine};
use gpui_kit::{App, AppContext, Context, Entity, RenderImage};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::cache;
use crate::film::{FilmSettings, render_film};
use crate::pictures::{Picture, Pictures, Source, image_of};
use crate::settings::Settings;
use crate::stocks;

const RAW: [&str; 15] = ["dng", "cr3", "cr2", "arw", "nef", "nrw", "raf", "rw2", "rwl", "orf", "pef", "srw", "erf", "3fr", "iiq"];
const IMAGE: [&str; 8] = ["jpg", "jpeg", "png", "tif", "tiff", "webp", "heic", "heif"];
const THUMBNAIL_EDGE: usize = 480;
const THUMBNAIL_SOURCE: usize = 512;
const INDEXERS: usize = 3;
const PROBERS: usize = 4;
const LOADERS: usize = 4;
const NOTIFY_EVERY: Duration = Duration::from_millis(33);
const IDLE_POLL: Duration = Duration::from_millis(50);

#[derive(Clone, Debug)]
pub struct PhotoFile {
    pub id: String,
    pub name: String,
    pub stem: String,
    pub raw: bool,
    pub path: PathBuf,
    pub sidecar: Option<PathBuf>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Probed {
    pub facts: Value,
    pub defaults: Settings,
    pub embedded_xmp: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct PhotoState {
    pub settings: Settings,
    pub rating: u8,
    pub flag: String,
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub film: Option<Value>,
}

impl PhotoState {
    pub fn film(&self) -> Option<FilmSettings> {
        self.film.as_ref().and_then(FilmSettings::of)
    }
}

#[derive(Clone)]
pub struct PhotoRecord {
    pub file: PhotoFile,
    pub probed: Option<Probed>,
    pub state: Option<PhotoState>,
    /// The look of the film thumbnail in the disk cache, once one is rendered.
    pub film: Option<u64>,
    pub error: Option<String>,
}

impl PhotoRecord {
    /// Whether indexing is done with this photo: its film thumbnail is rendered, or it couldn't be read.
    pub fn indexed(&self) -> bool {
        self.film.is_some() || self.error.is_some()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Saving {
    Idle,
    Saving,
    Saved,
    Failed,
}

pub struct Library {
    pub folder: Option<PathBuf>,
    pub photos: Vec<PhotoRecord>,
    pub saving: Saving,
    pub save_error: Option<String>,
    generation: usize,
    // why: one sidecar write at a time per file, with only the newest waiting behind it, so two quick saves of a
    // photo can't land on disk out of order
    writing: HashMap<PathBuf, Option<Write>>,
    /// Photos whose last write failed.
    failed: HashSet<String>,
    by_id: HashMap<String, usize>,
    revision: u64,
    pictures: Pictures,
    loading: HashSet<String>,
    /// Photos with no embedded preview to show while their film thumbnail renders.
    previewless: HashSet<String>,
    unrendered: VecDeque<String>,
    rendering: HashSet<String>,
    renderers: usize,
    probing: bool,
    /// Photos whose thumbnail is being re-rendered after an edit, and whether another edit came in meanwhile.
    refreshing: HashMap<String, bool>,
    notifying: bool,
    previews: usize,
}

struct Write {
    id: String,
    target: PathBuf,
    facts: String,
    state: PhotoState,
    refresh: bool,
}

fn extension(name: &str) -> String {
    Path::new(name).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

fn stem(name: &str) -> String {
    Path::new(name).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| name.to_string())
}

fn natural(a: &str, b: &str) -> std::cmp::Ordering {
    let key = |s: &str| {
        let mut parts: Vec<(String, u64)> = Vec::new();
        let mut text = String::new();
        let mut digits = String::new();
        for c in s.chars() {
            if c.is_ascii_digit() {
                digits.push(c);
            } else {
                if !digits.is_empty() {
                    parts.push((std::mem::take(&mut text), digits.parse().unwrap_or(0)));
                    digits.clear();
                }
                text.extend(c.to_lowercase());
            }
        }
        parts.push((text, digits.parse().unwrap_or(0)));
        parts
    };
    key(a).cmp(&key(b))
}

pub fn photos_in(folder: &Path) -> Vec<PhotoFile> {
    let Ok(entries) = std::fs::read_dir(folder) else { return Vec::new() };
    let mut by_stem: HashMap<String, Vec<String>> = HashMap::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || !entry.file_type().is_ok_and(|t| t.is_file()) {
            continue;
        }
        by_stem.entry(stem(&name).to_lowercase()).or_default().push(name);
    }
    let mut photos: Vec<PhotoFile> = by_stem
        .values()
        .filter_map(|group| {
            let raw = group.iter().find(|n| RAW.contains(&extension(n).as_str()));
            let image = group.iter().find(|n| IMAGE.contains(&extension(n).as_str()));
            let source = raw.or(image)?;
            let sidecar = group.iter().find(|n| extension(n) == "xmp").map(|n| folder.join(n));
            Some(PhotoFile { id: source.clone(), name: source.clone(), stem: stem(source), raw: raw.is_some(), path: folder.join(source), sidecar })
        })
        .collect();
    photos.sort_by(|a, b| natural(&a.name, &b.name));
    photos
}

// why: the engines share each parsed stock, so a .cube's text (several MB) is read only when an engine first
// needs it and isn't kept
pub fn lut_text(id: &str) -> Result<String, String> {
    stocks::lut_text(id).map_err(|e| format!("{id}: {e}"))
}

pub fn ensure_luts(engine: &mut Engine, film: Option<&FilmSettings>) -> Result<(), String> {
    for id in film.map(FilmSettings::luts).unwrap_or_default() {
        if !engine.has_lut(&id) {
            engine.load_lut(&id, &lut_text(&id)?)?;
        }
    }
    Ok(())
}

static INDEX_ENGINES: LazyLock<Vec<Mutex<Engine>>> = LazyLock::new(|| (0..INDEXERS).map(|_| Mutex::new(Engine::new())).collect());

/// The engine that re-renders a thumbnail after an edit, holding the last photo it decoded for the next edit of it.
struct Refresher {
    engine: Engine,
    decoded: Option<cache::Key>,
}

const REFRESHED: &str = "refreshed";

static REFRESHER: LazyLock<Mutex<Refresher>> = LazyLock::new(|| Mutex::new(Refresher { engine: Engine::new(), decoded: None }));

fn state_of(probed: &Probed, sidecar: Option<&str>) -> Result<PhotoState, String> {
    match sidecar.or(probed.embedded_xmp.as_deref()) {
        Some(xmp) => serde_json::from_str(&core::read_sidecar(&probed.facts.to_string(), xmp)?).map_err(|e| e.to_string()),
        None => Ok(PhotoState { settings: probed.defaults.clone(), rating: 0, flag: "none".into(), label: None, film: None }),
    }
}

/// Hash of everything in `state` that changes its thumbnail's pixels.
fn look_of(state: &PhotoState) -> u64 {
    let film = render_film(state.film().as_ref()).map(|f| f.to_value().to_string()).unwrap_or_default();
    cache::look(&state.settings.json(), &film, THUMBNAIL_EDGE, THUMBNAIL_SOURCE)
}

fn view_of(film: Option<&FilmSettings>) -> String {
    let mut view = json!({"maxW": THUMBNAIL_EDGE, "maxH": THUMBNAIL_EDGE});
    if let Some(f) = film {
        view["film"] = f.to_value();
    }
    view.to_string()
}

struct Rendered {
    look: u64,
    image: Arc<RenderImage>,
}

fn finish(mut frame: core::Frame, state: &PhotoState, key: Option<&cache::Key>) -> Result<Rendered, String> {
    let look = look_of(state);
    let rgba = frame.take_rgba();
    if let Some(key) = key
        && let Some(jpeg) = cache::encode(frame.width, frame.height, &rgba)
    {
        cache::store_thumbnail(key, look, &jpeg);
    }
    let image = image_of(frame.width, frame.height, rgba).ok_or_else(|| "the thumbnail has no pixels".to_string())?;
    Ok(Rendered { look, image })
}

// why: the engine is locked here, outside the thumbnail pool, and only its decode and render run in the pool; a pool
// thread that waited on a lock held by a render it is helping would deadlock the pool. A folder's first index is
// what the user is waiting on, so it runs user-initiated on every core (edits, user-interactive, still come first)
fn index_render(slot: usize, file: &PhotoFile, probed: &Probed, state: &PhotoState) -> Result<Rendered, String> {
    let bytes = std::fs::read(&file.path).map_err(|e| e.to_string())?;
    let film = render_film(state.film().as_ref());
    let mut engine = INDEX_ENGINES[slot % INDEXERS].lock().map_err(|e| e.to_string())?;
    let engine = &mut *engine;
    ensure_luts(engine, film.as_ref())?;
    let view = view_of(film.as_ref());
    let frame = crate::perf::initiated(|| {
        engine.load("scratch", &probed.facts.to_string(), &bytes, THUMBNAIL_SOURCE)?;
        engine.render_develop("scratch", &state.settings, &view)
    });
    engine.release("scratch");
    finish(frame?, state, cache::key(&file.path).as_ref())
}

// why: a refresh after an edit runs at utility QoS on the efficiency cores so it never slows the next edit, and
// keeps the photo decoded because the next edit is most likely to the same photo
fn refresh_render(path: &Path, probed: &Probed, state: &PhotoState) -> Result<Rendered, String> {
    let key = cache::key(path);
    let film = render_film(state.film().as_ref());
    let mut refresher = REFRESHER.lock().map_err(|e| e.to_string())?;
    let Refresher { engine, decoded } = &mut *refresher;
    ensure_luts(engine, film.as_ref())?;
    let view = view_of(film.as_ref());
    let reuse = key.is_some() && *decoded == key && engine.has(REFRESHED);
    let frame = crate::perf::background(|| {
        if !reuse {
            let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
            engine.load(REFRESHED, &probed.facts.to_string(), &bytes, THUMBNAIL_SOURCE)?;
        }
        engine.render_develop(REFRESHED, &state.settings, &view)
    });
    *decoded = if frame.is_ok() { key.clone() } else { None };
    finish(frame?, state, key.as_ref())
}

/// The film thumbnail of a photo rendered on a fresh engine, bypassing every cache, as RGBA.
pub fn fresh_thumbnail(path: &Path, probed: &Probed, state: &PhotoState) -> Result<Vec<u8>, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let film = render_film(state.film().as_ref());
    let mut engine = Engine::new();
    ensure_luts(&mut engine, film.as_ref())?;
    engine.load("fresh", &probed.facts.to_string(), &bytes, THUMBNAIL_SOURCE)?;
    Ok(engine.render_develop("fresh", &state.settings, &view_of(film.as_ref()))?.take_rgba())
}

/// What the disk cache already knows about a photo: its probe, and whether its film thumbnail matches its look.
struct Known {
    probed: Probed,
    state: PhotoState,
    film: Option<u64>,
}

fn cached_probe(key: Option<&cache::Key>) -> Option<Probed> {
    serde_json::from_str(&cache::probe(key?)?).ok()
}

fn sidecar_text(file: &PhotoFile) -> Option<String> {
    file.sidecar.as_ref().and_then(|p| std::fs::read_to_string(p).ok())
}

fn known(file: &PhotoFile) -> Option<Known> {
    let key = cache::key(&file.path)?;
    let probed = cached_probe(Some(&key))?;
    let state = state_of(&probed, sidecar_text(file).as_deref()).ok()?;
    let look = look_of(&state);
    let film = (cache::thumbnail_look(&key) == Some(look)).then_some(look);
    Some(Known { probed, state, film })
}

fn probe_file(file: &PhotoFile) -> Result<(Probed, PhotoState), String> {
    let key = cache::key(&file.path);
    let probed = match cached_probe(key.as_ref()) {
        Some(probed) => probed,
        None => {
            let bytes = std::fs::read(&file.path).map_err(|e| e.to_string())?;
            let json = core::probe(&file.name, &bytes)?;
            if let Some(key) = &key {
                cache::store_probe(key, &json);
            }
            serde_json::from_str(&json).map_err(|e| e.to_string())?
        }
    };
    let state = state_of(&probed, sidecar_text(file).as_deref())?;
    Ok((probed, state))
}

struct RenderJob {
    id: String,
    file: PhotoFile,
    probed: Probed,
    state: PhotoState,
}

impl Library {
    pub fn new() -> Library {
        Library {
            folder: None,
            photos: Vec::new(),
            saving: Saving::Idle,
            save_error: None,
            generation: 0,
            writing: HashMap::new(),
            failed: HashSet::new(),
            by_id: HashMap::new(),
            revision: 0,
            pictures: Pictures::default(),
            loading: HashSet::new(),
            previewless: HashSet::new(),
            unrendered: VecDeque::new(),
            rendering: HashSet::new(),
            renderers: 0,
            probing: false,
            refreshing: HashMap::new(),
            notifying: false,
            previews: 0,
        }
    }

    pub fn record(&self, id: &str) -> Option<&PhotoRecord> {
        self.photos.get(*self.by_id.get(id)?)
    }

    fn record_mut(&mut self, id: &str) -> Option<&mut PhotoRecord> {
        self.revision += 1;
        self.photos.get_mut(*self.by_id.get(id)?)
    }

    pub fn index_of(&self, id: &str) -> Option<usize> {
        self.by_id.get(id).copied()
    }

    /// Changes whenever a photo's file, facts, state or error does, so views can keep what they derive from them.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// The decoded thumbnail of `id`, when it is in memory; `want` asks for it.
    pub fn picture(&self, id: &str) -> Option<&Picture> {
        self.pictures.get(id)
    }

    /// How many embedded previews have been decoded to stand in for film thumbnails still rendering.
    pub fn previews_shown(&self) -> usize {
        self.previews
    }

    /// Starts a frame: what the views drew last frame stops being wanted until they want it again.
    pub fn begin_frame(&mut self) {
        self.pictures.begin_frame();
    }

    /// The views on screen show these photos, most important first; their thumbnails are decoded if they aren't in
    /// memory, and their film renders come before the rest of the folder's.
    pub fn want<'a>(&mut self, ids: impl IntoIterator<Item = &'a String>, cx: &mut Context<Library>) {
        for id in ids {
            self.pictures.want(id);
        }
        self.load_pictures(cx);
    }

    fn notify_soon(&mut self, cx: &mut Context<Library>) {
        if self.notifying {
            return;
        }
        self.notifying = true;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(NOTIFY_EVERY).await;
            let _ = this.update(cx, |library, cx| {
                library.notifying = false;
                cx.notify();
            });
        })
        .detach();
    }

    pub fn open(&mut self, folder: PathBuf, cx: &mut Context<Library>) {
        self.generation += 1;
        let run = self.generation;
        self.pictures.clear(cx);
        self.loading.clear();
        self.previewless.clear();
        self.unrendered.clear();
        self.rendering.clear();
        self.refreshing.clear();
        self.probing = false;
        self.renderers = 0;
        self.photos = photos_in(&folder).into_iter().map(|file| PhotoRecord { file, probed: None, state: None, film: None, error: None }).collect();
        self.by_id = self.photos.iter().enumerate().map(|(at, r)| (r.file.id.clone(), at)).collect();
        self.revision += 1;
        self.folder = Some(folder);
        self.saving = Saving::Idle;
        self.failed.clear();
        cache::prune();
        let files: Vec<PhotoFile> = self.photos.iter().map(|r| r.file.clone()).collect();
        cx.spawn(async move |this, cx| {
            let looked = cx
                .background_spawn(async move {
                    use rayon::prelude::*;
                    files.par_iter().map(|file| (file.clone(), known(file))).collect::<Vec<_>>()
                })
                .await;
            let _ = this.update(cx, |library, cx| {
                if library.generation != run {
                    return;
                }
                let mut unprobed = Vec::new();
                for (file, hit) in looked {
                    match hit {
                        Some(hit) => library.learn(&file.id, hit.probed, hit.state, hit.film),
                        None => unprobed.push(file),
                    }
                }
                cx.notify();
                library.probe_all(unprobed, run, cx);
                library.render_all(run, cx);
                library.load_pictures(cx);
            });
        })
        .detach();
        cx.notify();
    }

    fn learn(&mut self, id: &str, probed: Probed, state: PhotoState, film: Option<u64>) {
        let Some(record) = self.record_mut(id) else { return };
        record.probed = Some(probed);
        record.state = Some(state);
        record.film = film;
        if film.is_none() {
            self.unrendered.push_back(id.to_string());
        }
    }

    fn fail(&mut self, id: &str, error: String) {
        if let Some(record) = self.record_mut(id) {
            record.error = Some(error);
        }
    }

    fn probe_all(&mut self, files: Vec<PhotoFile>, run: usize, cx: &mut Context<Library>) {
        if files.is_empty() {
            return;
        }
        self.probing = true;
        cx.spawn(async move |this, cx| {
            for batch in files.chunks(PROBERS) {
                let batch = batch.to_vec();
                let probed = cx
                    .background_spawn(async move {
                        use rayon::prelude::*;
                        batch.par_iter().map(|file| (file.id.clone(), probe_file(file))).collect::<Vec<_>>()
                    })
                    .await;
                let alive = this.update(cx, |library, cx| {
                    if library.generation != run {
                        return false;
                    }
                    for (id, result) in probed {
                        match result {
                            Ok((probed, state)) => library.learn(&id, probed, state, None),
                            Err(e) => library.fail(&id, e),
                        }
                    }
                    library.notify_soon(cx);
                    true
                });
                if !alive.unwrap_or(false) {
                    return;
                }
            }
            let _ = this.update(cx, |library, _| {
                if library.generation == run {
                    library.probing = false;
                }
            });
        })
        .detach();
    }

    fn needs_render(&self, id: &str) -> bool {
        !self.rendering.contains(id) && self.record(id).is_some_and(|r| r.film.is_none() && r.error.is_none() && r.probed.is_some())
    }

    /// The next film thumbnail to render: a photo on screen first, then the folder in order.
    fn next_render(&mut self) -> Option<RenderJob> {
        let id = match self.pictures.wanted().iter().find(|id| self.needs_render(id)) {
            Some(id) => id.clone(),
            None => loop {
                let id = self.unrendered.pop_front()?;
                if self.needs_render(&id) {
                    break id;
                }
            },
        };
        let record = self.record(&id)?;
        let job = RenderJob { id: id.clone(), file: record.file.clone(), probed: record.probed.clone()?, state: record.state.clone()? };
        self.rendering.insert(id);
        Some(job)
    }

    fn render_all(&mut self, run: usize, cx: &mut Context<Library>) {
        while self.renderers < INDEXERS {
            let slot = self.renderers;
            self.renderers += 1;
            cx.spawn(async move |this, cx| {
                loop {
                    let next = this.update(cx, |library, _| (library.generation == run).then(|| library.next_render()));
                    let Ok(Some(next)) = next else { break };
                    let Some(job) = next else {
                        if !this.read_with(cx, |library, _| library.probing).unwrap_or(false) {
                            break;
                        }
                        cx.background_executor().timer(IDLE_POLL).await;
                        continue;
                    };
                    let id = job.id.clone();
                    let result = cx.background_spawn(async move { index_render(slot, &job.file, &job.probed, &job.state) }).await;
                    let _ = this.update(cx, |library, cx| {
                        if library.generation == run {
                            library.rendered(&id, result, cx);
                        }
                    });
                }
                let _ = this.update(cx, |library, _| {
                    if library.generation != run {
                        return;
                    }
                    library.renderers -= 1;
                    // why: indexing decodes many photos; once the last indexer is done their memory goes back
                    if library.renderers == 0 {
                        crate::perf::give_back();
                    }
                });
            })
            .detach();
        }
    }

    fn rendered(&mut self, id: &str, result: Result<Rendered, String>, cx: &mut Context<Library>) {
        self.rendering.remove(id);
        match result {
            Ok(rendered) if self.record(id).and_then(|r| r.state.as_ref()).map(look_of) == Some(rendered.look) => self.show_film(id, rendered, cx),
            Ok(_) if !self.refreshing.contains_key(id) => self.unrendered.push_back(id.to_string()),
            Ok(_) => {}
            Err(e) => self.fail(id, e),
        }
        self.notify_soon(cx);
    }

    fn show_film(&mut self, id: &str, rendered: Rendered, cx: &mut Context<Library>) {
        if let Some(at) = self.by_id.get(id) {
            self.photos[*at].film = Some(rendered.look);
        }
        if self.pictures.is_wanted(id) || self.pictures.get(id).is_some() {
            self.pictures.insert(id, Picture { image: rendered.image, film: Some(rendered.look) }, cx);
        }
    }

    fn next_picture(&self) -> Option<(String, Source)> {
        self.pictures.wanted().iter().filter(|id| !self.loading.contains(*id)).find_map(|id| {
            let record = self.record(id)?;
            let shown = self.pictures.get(id).map(|p| p.film);
            let path = record.file.path.clone();
            match record.film {
                Some(look) if shown != Some(Some(look)) => Some((id.clone(), Source::Film { path, look })),
                None if shown.is_none() && record.error.is_none() && !self.previewless.contains(id) => Some((id.clone(), Source::Preview { path })),
                _ => None,
            }
        })
    }

    fn load_pictures(&mut self, cx: &mut Context<Library>) {
        while self.loading.len() < LOADERS {
            let Some((id, source)) = self.next_picture() else { break };
            self.loading.insert(id.clone());
            let run = self.generation;
            let film = match &source {
                Source::Film { look, .. } => Some(*look),
                Source::Preview { .. } => None,
            };
            cx.spawn(async move |this, cx| {
                let loaded = cx.background_spawn(async move { source.load(THUMBNAIL_EDGE) }).await;
                let _ = this.update(cx, |library, cx| {
                    if library.generation != run {
                        return;
                    }
                    library.loading.remove(&id);
                    library.loaded(&id, film, loaded, cx);
                    library.load_pictures(cx);
                });
            })
            .detach();
        }
    }

    fn loaded(&mut self, id: &str, film: Option<u64>, loaded: Option<Picture>, cx: &mut Context<Library>) {
        let current = self.record(id).and_then(|r| r.film);
        match (film, loaded) {
            (Some(look), Some(picture)) if current == Some(look) => self.pictures.insert(id, picture, cx),
            (Some(_), Some(_)) => return,
            (Some(look), None) if current == Some(look) => {
                if let Some(at) = self.by_id.get(id) {
                    self.photos[*at].film = None;
                }
                self.unrendered.push_front(id.to_string());
                self.render_all(self.generation, cx);
                return;
            }
            (Some(_), None) => return,
            (None, Some(picture)) if self.pictures.get(id).is_none() => {
                self.previews += 1;
                self.pictures.insert(id, picture, cx);
            }
            (None, Some(_)) => return,
            (None, None) => {
                self.previewless.insert(id.to_string());
                return;
            }
        }
        self.notify_soon(cx);
    }

    pub fn refresh_thumbnail(&mut self, id: &str, cx: &mut Context<Library>) {
        if let Some(again) = self.refreshing.get_mut(id) {
            *again = true;
            return;
        }
        let Some(record) = self.record(id) else { return };
        let (Some(probed), Some(state)) = (record.probed.clone(), record.state.clone()) else { return };
        let path = record.file.path.clone();
        let id = id.to_string();
        let run = self.generation;
        self.refreshing.insert(id.clone(), false);
        cx.spawn(async move |this, cx| {
            let made = cx.background_spawn(async move { refresh_render(&path, &probed, &state) }).await;
            let _ = this.update(cx, |library, cx| {
                if library.generation != run {
                    return;
                }
                let again = library.refreshing.remove(&id).unwrap_or(false);
                if let Ok(rendered) = made {
                    library.show_film(&id, rendered, cx);
                    cx.notify();
                }
                if again {
                    library.refresh_thumbnail(&id, cx);
                }
            });
        })
        .detach();
    }

    pub fn persist(&mut self, id: &str, state: PhotoState, cx: &mut Context<Library>) {
        self.write(id, state, true, cx);
    }

    pub fn set_flag(&mut self, ids: &[String], flag: &str, cx: &mut Context<Library>) {
        for id in ids {
            let Some(state) = self.record(id).and_then(|r| r.state.clone()) else { continue };
            if state.flag != flag {
                self.write(id, PhotoState { flag: flag.to_string(), ..state }, false, cx);
            }
        }
        cx.notify();
    }

    pub fn set_rating(&mut self, ids: &[String], rating: u8, cx: &mut Context<Library>) {
        for id in ids {
            let Some(state) = self.record(id).and_then(|r| r.state.clone()) else { continue };
            if state.rating != rating {
                self.write(id, PhotoState { rating, ..state }, false, cx);
            }
        }
        cx.notify();
    }

    pub fn set_label(&mut self, ids: &[String], label: Option<String>, cx: &mut Context<Library>) {
        for id in ids {
            let Some(state) = self.record(id).and_then(|r| r.state.clone()) else { continue };
            if state.label != label {
                self.write(id, PhotoState { label: label.clone(), ..state }, false, cx);
            }
        }
        cx.notify();
    }

    /// Where `id`'s edits are written: its sidecar, or a new one beside the photo.
    fn target(&self, id: &str) -> Option<PathBuf> {
        let record = self.record(id)?;
        let name = record.file.sidecar.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| format!("{}.xmp", record.file.stem));
        Some(self.folder.as_ref()?.join(name))
    }

    /// Whether `id`'s latest state is on disk, being written, or failed to write.
    pub fn saved(&self, id: &str) -> Saving {
        match self.target(id) {
            None => Saving::Idle,
            Some(target) if self.writing.contains_key(&target) => Saving::Saving,
            Some(_) if self.failed.contains(id) => Saving::Failed,
            Some(_) => Saving::Saved,
        }
    }

    fn write(&mut self, id: &str, state: PhotoState, refresh: bool, cx: &mut Context<Library>) {
        let Some(target) = self.target(id) else { return };
        let Some(record) = self.record_mut(id) else { return };
        record.state = Some(state.clone());
        let Some(probed) = record.probed.as_ref() else { return };
        let write = Write { id: id.to_string(), target, facts: probed.facts.to_string(), state, refresh };
        self.saving = Saving::Saving;
        if let Some(queued) = self.writing.get_mut(&write.target) {
            let refresh = write.refresh || queued.as_ref().is_some_and(|q| q.refresh);
            *queued = Some(Write { refresh, ..write });
            return;
        }
        self.writing.insert(write.target.clone(), None);
        self.start_write(write, cx);
    }

    fn start_write(&mut self, write: Write, cx: &mut Context<Library>) {
        cx.spawn(async move |this, cx| {
            let written = {
                let (target, facts, state) = (write.target.clone(), write.facts.clone(), write.state.clone());
                cx.background_spawn(async move {
                    let existing = std::fs::read_to_string(&target).ok();
                    let state = serde_json::to_string(&state).map_err(|e| e.to_string())?;
                    let text = core::write_sidecar(&facts, &state, existing)?;
                    std::fs::write(&target, text).map_err(|e| e.to_string())
                })
                .await
            };
            let _ = this.update(cx, |library, cx| {
                let here = library.folder.as_deref() == write.target.parent();
                match written {
                    Ok(()) => {
                        library.save_error = None;
                        library.failed.remove(&write.id);
                        if here && let Some(record) = library.record_mut(&write.id) {
                            record.file.sidecar = Some(write.target.clone());
                        }
                    }
                    Err(e) => {
                        library.save_error = Some(e);
                        library.failed.insert(write.id.clone());
                    }
                }
                if write.refresh && here {
                    library.refresh_thumbnail(&write.id, cx);
                }
                match library.writing.remove(&write.target).flatten() {
                    Some(next) => {
                        library.writing.insert(next.target.clone(), None);
                        library.start_write(next, cx);
                    }
                    None if library.writing.is_empty() => library.saving = if library.save_error.is_some() { Saving::Failed } else { Saving::Saved },
                    None => {}
                }
                cx.notify();
            });
        })
        .detach();
    }
}

impl Default for Library {
    fn default() -> Self {
        Library::new()
    }
}

pub fn new_library(cx: &mut App) -> Entity<Library> {
    cx.new(|_| Library::new())
}
