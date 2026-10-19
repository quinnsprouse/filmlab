use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};

use filmlab_engine::core::{self, Engine};
use gpui_kit::{App, AppContext, Context, Entity, RenderImage};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::cache;
use crate::film::{FilmSettings, render_film};
use crate::stocks;

const RAW: [&str; 15] = ["dng", "cr3", "cr2", "arw", "nef", "nrw", "raf", "rw2", "rwl", "orf", "pef", "srw", "erf", "3fr", "iiq"];
const IMAGE: [&str; 8] = ["jpg", "jpeg", "png", "tif", "tiff", "webp", "heic", "heif"];
const THUMBNAIL_EDGE: usize = 480;
const THUMBNAIL_SOURCE: usize = 512;
const INDEXERS: usize = 3;

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
    pub defaults: Value,
    pub embedded_xmp: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct PhotoState {
    pub settings: Value,
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
    pub refreshes: u64,
    pub probed: Option<Probed>,
    pub state: Option<PhotoState>,
    pub thumbnail: Option<Arc<RenderImage>>,
    pub error: Option<String>,
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

fn state_of(probed: &Probed, sidecar: Option<&str>) -> Result<PhotoState, String> {
    match sidecar.or(probed.embedded_xmp.as_deref()) {
        Some(xmp) => serde_json::from_str(&core::read_sidecar(&probed.facts.to_string(), xmp)?).map_err(|e| e.to_string()),
        None => Ok(PhotoState { settings: probed.defaults.clone(), rating: 0, flag: "none".into(), label: None, film: None }),
    }
}

pub fn image_of(width: u32, height: u32, mut rgba: Vec<u8>) -> Option<Arc<RenderImage>> {
    for px in rgba.as_chunks_mut::<4>().0 {
        px.swap(0, 2);
    }
    let buffer = image::RgbaImage::from_raw(width, height, rgba)?;
    Some(Arc::new(RenderImage::new(vec![image::Frame::new(buffer)])))
}

fn look_of(settings: &Value, film: Option<&FilmSettings>) -> u64 {
    let film = film.map(|f| f.to_value().to_string()).unwrap_or_default();
    cache::look(&settings.to_string(), &film, THUMBNAIL_EDGE, THUMBNAIL_SOURCE)
}

fn cached_image(pixels: cache::Pixels) -> Option<Arc<RenderImage>> {
    let cache::Pixels { width, height, mut rgba } = pixels;
    rgba.as_chunks_mut::<4>().0.iter_mut().for_each(|px| px.swap(0, 2));
    let buffer = image::RgbaImage::from_raw(width, height, rgba)?;
    Some(Arc::new(RenderImage::new(vec![image::Frame::new(buffer)])))
}

// why: the engine is locked here, outside the thumbnail pool, and only its decode and render run in the pool; a pool
// thread that waited on a lock held by a render it is helping would deadlock the pool. A folder's first index is
// what the user is waiting on, so it runs user-initiated on every core (edits, user-interactive, still come first);
// a refresh after an edit runs at utility QoS on the efficiency cores so it never slows the next edit
fn thumbnail(slot: usize, facts: &Value, bytes: &[u8], settings: &Value, film: Option<&FilmSettings>, key: Option<&cache::Key>, waited_on: bool) -> Result<Arc<RenderImage>, String> {
    let mut engine = INDEX_ENGINES[slot % INDEXERS].lock().map_err(|e| e.to_string())?;
    let engine = &mut *engine;
    ensure_luts(engine, film)?;
    let mut view = json!({"maxW": THUMBNAIL_EDGE, "maxH": THUMBNAIL_EDGE});
    if let Some(f) = film {
        view["film"] = f.to_value();
    }
    let work = || {
        engine.load("scratch", &facts.to_string(), bytes, THUMBNAIL_SOURCE)?;
        engine.render("scratch", &settings.to_string(), &view.to_string())
    };
    let frame = if waited_on { crate::perf::initiated(work) } else { crate::perf::background(work) };
    engine.release("scratch");
    let mut frame = frame?;
    let rgba = frame.take_rgba();
    if let Some(key) = key
        && let Some(jpeg) = cache::encode(frame.width, frame.height, &rgba)
    {
        cache::store_thumbnail(key, look_of(settings, film), &jpeg);
    }
    image_of(frame.width, frame.height, rgba).ok_or_else(|| "the thumbnail has no pixels".to_string())
}

struct Indexed {
    probed: Probed,
    state: PhotoState,
    thumbnail: Arc<RenderImage>,
}

/// A photo whose probe and thumbnail were both found in the cache.
struct Warm {
    id: String,
    probed: Probed,
    state: PhotoState,
    thumbnail: Arc<RenderImage>,
}

fn cached_probe(key: Option<&cache::Key>) -> Option<Probed> {
    serde_json::from_str(&cache::probe(key?)?).ok()
}

fn sidecar_text(file: &PhotoFile) -> Option<String> {
    file.sidecar.as_ref().and_then(|p| std::fs::read_to_string(p).ok())
}

fn warm(file: &PhotoFile) -> Option<Warm> {
    let key = cache::key(&file.path)?;
    let probed = cached_probe(Some(&key))?;
    let state = state_of(&probed, sidecar_text(file).as_deref()).ok()?;
    let film = render_film(state.film().as_ref());
    let pixels = cache::thumbnail(&key, look_of(&state.settings, film.as_ref()))?;
    Some(Warm { id: file.id.clone(), probed, state, thumbnail: cached_image(pixels)? })
}

fn index(slot: usize, file: &PhotoFile) -> Result<Indexed, String> {
    let key = cache::key(&file.path);
    let bytes = std::fs::read(&file.path).map_err(|e| e.to_string())?;
    let sidecar = sidecar_text(file);
    let probed = match cached_probe(key.as_ref()) {
        Some(probed) => probed,
        None => {
            let json = core::probe(&file.name, &bytes)?;
            if let Some(key) = &key {
                cache::store_probe(key, &json);
            }
            serde_json::from_str(&json).map_err(|e| e.to_string())?
        }
    };
    let state = state_of(&probed, sidecar.as_deref())?;
    let film = render_film(state.film().as_ref());
    let thumbnail = thumbnail(slot, &probed.facts, &bytes, &state.settings, film.as_ref(), key.as_ref(), true)?;
    Ok(Indexed { probed, state, thumbnail })
}

impl Library {
    pub fn new() -> Library {
        Library { folder: None, photos: Vec::new(), saving: Saving::Idle, save_error: None, generation: 0, writing: HashMap::new(), failed: HashSet::new() }
    }

    pub fn record(&self, id: &str) -> Option<&PhotoRecord> {
        self.photos.iter().find(|r| r.file.id == id)
    }

    fn record_mut(&mut self, id: &str) -> Option<&mut PhotoRecord> {
        self.photos.iter_mut().find(|r| r.file.id == id)
    }

    pub fn index_of(&self, id: &str) -> Option<usize> {
        self.photos.iter().position(|r| r.file.id == id)
    }

    pub fn open(&mut self, folder: PathBuf, cx: &mut Context<Library>) {
        self.generation += 1;
        let run = self.generation;
        for image in self.photos.iter_mut().filter_map(|r| r.thumbnail.take()) {
            cx.drop_image(image, None);
        }
        self.photos = photos_in(&folder).into_iter().map(|file| PhotoRecord { file, refreshes: 0, probed: None, state: None, thumbnail: None, error: None }).collect();
        self.folder = Some(folder);
        self.saving = Saving::Idle;
        self.failed.clear();
        cache::prune();
        let files: Vec<PhotoFile> = self.photos.iter().map(|r| r.file.clone()).collect();
        cx.spawn(async move |this, cx| {
            let looked = cx
                .background_spawn(async move {
                    use rayon::prelude::*;
                    files.par_iter().map(|file| (file.clone(), warm(file))).collect::<Vec<_>>()
                })
                .await;
            let _ = this.update(cx, |library, cx| {
                if library.generation != run {
                    return;
                }
                let mut missed = Vec::new();
                for (file, hit) in looked {
                    match hit {
                        Some(hit) => {
                            if let Some(record) = library.record_mut(&hit.id) {
                                record.probed = Some(hit.probed);
                                record.state = Some(hit.state);
                                record.thumbnail = Some(hit.thumbnail);
                            }
                        }
                        None => missed.push(file),
                    }
                }
                cx.notify();
                library.index_all(missed, run, cx);
            });
        })
        .detach();
        cx.notify();
    }

    fn index_all(&mut self, files: Vec<PhotoFile>, run: usize, cx: &mut Context<Library>) {
        let queue = Arc::new(Mutex::new(files.into_iter().collect::<std::collections::VecDeque<_>>()));
        let working = Arc::new(std::sync::atomic::AtomicUsize::new(INDEXERS));
        for slot in 0..INDEXERS {
            let queue = queue.clone();
            let working = working.clone();
            cx.spawn(async move |this, cx| {
                loop {
                    let Some(file) = queue.lock().ok().and_then(|mut q| q.pop_front()) else {
                        // why: indexing decodes many photos; once the last indexer is done their memory goes back
                        if working.fetch_sub(1, std::sync::atomic::Ordering::AcqRel) == 1 {
                            crate::perf::give_back();
                        }
                        break;
                    };
                    let id = file.id.clone();
                    let result = cx.background_spawn(async move { index(slot, &file) }).await;
                    let alive = this.update(cx, |library, cx| {
                        if library.generation != run {
                            return false;
                        }
                        if let Some(record) = library.record_mut(&id) {
                            match result {
                                Ok(done) => {
                                    record.probed = Some(done.probed);
                                    record.state = Some(done.state);
                                    if let Some(old) = record.thumbnail.replace(done.thumbnail) {
                                        cx.drop_image(old, None);
                                    }
                                }
                                Err(e) => record.error = Some(e),
                            }
                        }
                        cx.notify();
                        true
                    });
                    if !alive.unwrap_or(false) {
                        break;
                    }
                }
            })
            .detach();
        }
    }

    pub fn refresh_thumbnail(&mut self, id: &str, cx: &mut Context<Library>) {
        let Some(record) = self.record_mut(id) else { return };
        record.refreshes += 1;
        let ticket = record.refreshes;
        let (Some(probed), Some(state)) = (record.probed.clone(), record.state.clone()) else { return };
        let path = record.file.path.clone();
        let id = id.to_string();
        cx.spawn(async move |this, cx| {
            let made = cx
                .background_spawn(async move {
                    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
                    thumbnail(0, &probed.facts, &bytes, &state.settings, render_film(state.film().as_ref()).as_ref(), cache::key(&path).as_ref(), false)
                })
                .await;
            if let Ok(image) = made {
                let _ = this.update(cx, |library, cx| {
                    if let Some(record) = library.record_mut(&id)
                        && record.refreshes == ticket
                    {
                        // why: GPUI keeps every image it has drawn in its atlas until it is dropped
                        if let Some(old) = record.thumbnail.replace(image) {
                            cx.drop_image(old, None);
                        }
                        cx.notify();
                    }
                });
            }
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
