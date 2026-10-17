use std::collections::HashMap;
use std::sync::Arc;

use lightcraft_engine::Session;
use lightcraft_engine::catalog::{Catalog, ColorLabel, Flag, MediaKind, Meta, Op, Photo, PhotoId, Source};
use lightcraft_engine::develop::{CONTROLS, DevelopSettings, EmbeddedLens, SettingsGroup, WbMode, apply_partial, controls as specs, extract_groups};
use lightcraft_engine::pipeline::local::{effective_wb, normalize_wb};
use lightcraft_engine::export::{ExportOptions, encode_rendered, export_metadata, output_size};
use lightcraft_engine::media::{DecodedSource, SourceLevel, source_info};
use lightcraft_engine::pipeline::geometry::Frame as Geometry;
use lightcraft_engine::pipeline::{self, MaskView, OutputDepth, OutputSpace, Overlay, PixelWindow, Quality, RenderRequest, SourceInfo, StageCache};
use lightcraft_engine::sidecar::{OWNED, OWNED_IF_STATED, merge_into, parse_sidecar, sidecar_packet};
use serde::{Deserialize, Serialize};

use crate::film::{Capture, FilmChain, FilmHook, FilmRequest, Glow, GrainField, Light, LutLibrary, Scan, contrast};
use serde_json::Value;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Facts {
    pub name: String,
    pub format: String,
    pub kind: MediaKind,
    pub width: u32,
    pub height: u32,
    pub captured: Option<String>,
    pub meta: Meta,
    pub as_shot_wb: Option<(f64, f64)>,
    pub embedded_lens: Option<EmbeddedLens>,
    pub preview_only: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Probed {
    facts: Facts,
    defaults: DevelopSettings,
    embedded_xmp: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhotoState {
    pub settings: DevelopSettings,
    pub rating: u8,
    pub flag: Flag,
    pub label: Option<ColorLabel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub film: Option<Value>,
}

const FILMLAB_NS: &str = "http://ns.filmlab.app/1.0/";

fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn xml_unescape(text: &str) -> String {
    text.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

fn with_film(packet: String, film: Option<&Value>) -> String {
    let Some(film) = film else { return packet };
    let Some(close) = packet.find("  </rdf:Description>") else { return packet };
    let mut out = packet.clone();
    out.insert_str(close, &format!("   <fl:film>{}</fl:film>\n", xml_escape(&film.to_string())));
    match out.find("xmlns:lc=") {
        Some(at) => out.insert_str(at, &format!("xmlns:fl=\"{FILMLAB_NS}\"\n    ")),
        None => return packet,
    }
    out
}

pub fn film_of(packet: &str) -> Option<Value> {
    let element = packet.split_once("<fl:film>").and_then(|(_, rest)| rest.split_once("</fl:film>")).map(|(v, _)| v);
    let attribute = packet.split_once("fl:film=\"").and_then(|(_, rest)| rest.split_once('"')).map(|(v, _)| v);
    serde_json::from_str(&xml_unescape(element.or(attribute)?)).ok()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ViewRequest {
    max_w: usize,
    max_h: usize,
    #[serde(default)]
    draft: bool,
    #[serde(default = "yes")]
    apply_crop: bool,
    #[serde(default)]
    stages: Option<String>,
    #[serde(default)]
    window: Option<[usize; 4]>,
    #[serde(default)]
    overlay: Option<OverlayRequest>,
    #[serde(default)]
    film: Option<FilmRequest>,
}

#[derive(Deserialize, Clone, Copy, Default)]
#[serde(rename_all = "camelCase")]
enum MaskViewRequest {
    #[default]
    Color,
    ColorOnBw,
    ImageOnBlack,
    ImageOnWhite,
    WhiteOnBlack,
}

impl MaskViewRequest {
    fn view(self) -> MaskView {
        match self {
            MaskViewRequest::Color => MaskView::Color,
            MaskViewRequest::ColorOnBw => MaskView::ColorOnBw,
            MaskViewRequest::ImageOnBlack => MaskView::ImageOnBlack,
            MaskViewRequest::ImageOnWhite => MaskView::ImageOnWhite,
            MaskViewRequest::WhiteOnBlack => MaskView::WhiteOnBlack,
        }
    }
}

#[derive(Deserialize, Clone, Copy)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum OverlayRequest {
    Mask {
        id: u16,
        #[serde(default)]
        view: MaskViewRequest,
        color: [u8; 3],
        opacity: u8,
    },
    AllMasks {
        #[serde(default)]
        view: MaskViewRequest,
        color: [u8; 3],
        opacity: u8,
    },
    Spots {
        threshold: u8,
    },
}

impl OverlayRequest {
    fn overlay(self) -> Overlay {
        match self {
            OverlayRequest::Mask { id, view, color, opacity } => Overlay::Mask { id, view: view.view(), color, opacity: opacity.min(100) },
            OverlayRequest::AllMasks { view, color, opacity } => Overlay::AllMasks { view: view.view(), color, opacity: opacity.min(100) },
            OverlayRequest::Spots { threshold } => Overlay::Spots(threshold.min(100)),
        }
    }
}

fn yes() -> bool {
    true
}

impl ViewRequest {
    fn request(&self) -> RenderRequest {
        RenderRequest {
            max_w: self.max_w,
            max_h: self.max_h,
            quality: if self.draft { Quality::Draft } else { Quality::Full },
            apply_crop: self.apply_crop,
            overlay: self.overlay.map_or(Overlay::None, OverlayRequest::overlay),
            space: OutputSpace::Srgb,
            depth: OutputDepth::U8,
            proof: None,
            window: self.window.map(|[x, y, w, h]| PixelWindow { x, y, w, h }),
            display: None,
        }
    }
}

pub type Fallible<T> = Result<T, String>;

fn fail(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn parse<T: for<'de> Deserialize<'de>>(json: &str) -> Fallible<T> {
    serde_json::from_str(json).map_err(fail)
}

fn to_json(v: &impl Serialize) -> Fallible<String> {
    serde_json::to_string(v).map_err(fail)
}

fn photo_of(f: &Facts) -> Photo {
    let mut p = Photo::new(PhotoId(1), Source::File { path: f.name.clone() }, &f.name, &f.format, f.width.max(1), f.height.max(1), "");
    p.kind = f.kind;
    p.meta = f.meta.clone();
    p.captured = f.captured.clone();
    p.as_shot_wb = f.as_shot_wb;
    p.embedded_lens = f.embedded_lens;
    p.set_preview_only(f.preview_only.clone());
    p
}

pub fn controls() -> Fallible<String> {
    to_json(&CONTROLS)
}

pub fn probe(name: &str, bytes: &[u8]) -> Fallible<String> {
    let info = lightcraft_engine::files::probe_bytes(name, bytes).map_err(fail)?;
    let facts = Facts {
        name: name.to_string(),
        format: info.format,
        kind: info.kind,
        width: info.width,
        height: info.height,
        captured: info.captured,
        meta: info.meta,
        as_shot_wb: info.as_shot_wb,
        embedded_lens: info.embedded_lens,
        preview_only: info.preview_only,
    };
    let defaults = photo_of(&facts).import_defaults();
    let embedded_xmp = info.xmp.filter(|_| facts.kind == MediaKind::Raw);
    to_json(&Probed { facts, defaults, embedded_xmp })
}

pub fn read_sidecar(facts: &str, xmp: &str) -> Fallible<String> {
    let f: Facts = parse(facts)?;
    let mut p = photo_of(&f);
    p.develop = Arc::new(p.import_defaults());
    let target = lightcraft_engine::crs::Target::for_file(f.kind, &f.format, f.preview_only.is_some(), &f.meta.camera);
    let sc = parse_sidecar(xmp, target).map_err(fail)?;
    merge_into(&mut p, &sc, "");
    to_json(&PhotoState { settings: (*p.develop).clone(), rating: p.rating, flag: p.flag, label: p.label, film: film_of(xmp) })
}

pub fn write_sidecar(facts: &str, state: &str, existing: Option<String>) -> Fallible<String> {
    let f: Facts = parse(facts)?;
    let s: PhotoState = parse(state)?;
    let film = s.film.clone();
    let p = edited_photo(&f, s);
    let fresh = with_film(sidecar_packet(&p, &Catalog::default()), film.as_ref());
    match existing.filter(|old| !old.trim().is_empty()) {
        None => Ok(fresh),
        Some(old) => {
            let owned: Vec<&str> = OWNED.iter().copied().chain(["fl:*"]).collect();
            let rules = lightcraft_meta::MergeRules { owned: &owned, owned_if_present: OWNED_IF_STATED };
            lightcraft_meta::merge_xmp(&old, &fresh, rules).map_err(fail)
        }
    }
}

pub fn extract_profile(settings: &str, groups: &str) -> Fallible<String> {
    let s: DevelopSettings = parse(settings)?;
    let g: Vec<SettingsGroup> = parse(groups)?;
    to_json(&extract_groups(&s, &g))
}

pub fn apply_profile(settings: &str, partial: &str, amount: f64) -> Fallible<String> {
    let s: DevelopSettings = parse(settings)?;
    let p: Value = parse(partial)?;
    to_json(&apply_partial(&s, &p, amount))
}

pub fn control_values(settings: &str) -> Fallible<String> {
    let s: DevelopSettings = parse(settings)?;
    let values: HashMap<&str, f64> = CONTROLS.iter().filter_map(|c| specs::get(&s, c.id).map(|v| (c.id, v))).collect();
    to_json(&values)
}

fn kelvin_scale(p: &Photo) -> bool {
    p.develops_raw() && !p.relative_wb()
}

fn k_to_rel(k: f64) -> f64 {
    ((1e6 / 6500.0 - 1e6 / k) / 0.8).clamp(-100.0, 100.0)
}

fn rel_to_k(r: f64) -> f64 {
    1e6 / (1e6 / 6500.0 - r * 0.8)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WhiteBalanceView {
    mode: WbMode,
    temp: f64,
    tint: f64,
    kelvin: bool,
}

pub fn white_balance(facts: &str, settings: &str) -> Fallible<String> {
    let p = photo_of(&parse(facts)?);
    let d: DevelopSettings = parse(settings)?;
    let kelvin = kelvin_scale(&p);
    let (temp, tint) = if d.wb.mode == WbMode::AsShot && p.relative_wb() {
        (6500.0, 0.0)
    } else if kelvin {
        effective_wb(&source_info(&p), &d)
    } else {
        (d.wb.temp, d.wb.tint)
    };
    let view = if kelvin {
        WhiteBalanceView { mode: d.wb.mode, temp, tint, kelvin }
    } else {
        WhiteBalanceView { mode: d.wb.mode, temp: k_to_rel(temp), tint: tint.clamp(-100.0, 100.0), kelvin }
    };
    to_json(&view)
}

pub fn set_controls(facts: &str, settings: &str, values: &str) -> Fallible<String> {
    let p = photo_of(&parse(facts)?);
    let info = source_info(&p);
    let mut d: DevelopSettings = parse(settings)?;
    let values: Vec<(String, f64)> = parse::<HashMap<String, f64>>(values)?.into_iter().collect();
    let touches_wb = values.iter().any(|(k, _)| k == "wb.temp" || k == "wb.tint");
    if touches_wb {
        normalize_wb(&mut d, &info);
        if d.wb.mode == WbMode::AsShot {
            d.wb.temp = info.as_shot_temp;
            d.wb.tint = info.as_shot_tint;
        }
    }
    for (k, v) in &values {
        let v = if k == "wb.temp" && !kelvin_scale(&p) { rel_to_k(*v) } else { *v };
        specs::set(&mut d, k, v);
        if k == "wb.temp" || k == "wb.tint" {
            d.wb.mode = WbMode::Custom;
        }
    }
    to_json(&d)
}

pub fn set_white_balance_mode(facts: &str, settings: &str, mode: &str) -> Fallible<String> {
    let p = photo_of(&parse(facts)?);
    let info = source_info(&p);
    let mut d: DevelopSettings = parse(settings)?;
    let mode: WbMode = parse(mode)?;
    let (t, tint) = match mode {
        WbMode::AsShot => (info.as_shot_temp, info.as_shot_tint),
        WbMode::Auto => return Err("Auto white balance needs the photo's pixels: use Engine::auto_white_balance".into()),
        m => m
            .preset()
            .map(|(t, ti)| if info.raw && !info.relative_wb { (t, ti) } else { (6500.0 * t / 5500.0, ti) })
            .unwrap_or((info.as_shot_temp, info.as_shot_tint)),
    };
    normalize_wb(&mut d, &info);
    d.wb.mode = mode;
    d.wb.temp = t.clamp(2000.0, 50000.0);
    d.wb.tint = tint.clamp(-150.0, 150.0);
    to_json(&d)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ExportPlan {
    width: usize,
    height: usize,
    max_edge: usize,
}

fn edited_photo(f: &Facts, state: PhotoState) -> Photo {
    let mut p = photo_of(f);
    p.develop = Arc::new(state.settings);
    p.rating = state.rating.min(5);
    p.flag = state.flag;
    p.label = state.label;
    p
}

fn source_edge_needed(p: &Photo, out_long: usize) -> usize {
    let (w, h) = (f64::from(p.width.max(1)), f64::from(p.height.max(1)));
    let r = p.develop.crop.geometry.rect;
    let shown = (r.width() * w).max(r.height() * h);
    if !shown.is_finite() || shown < 1.0 {
        return out_long;
    }
    let needed = out_long as f64 * w.max(h) / shown;
    if needed.is_finite() { needed.min(usize::MAX as f64 / 2.0) as usize } else { out_long }
}

pub fn export_plan(facts: &str, state: &str, options: &str) -> Fallible<String> {
    let p = edited_photo(&parse(facts)?, parse(state)?);
    let o: ExportOptions = parse(options)?;
    let (width, height) = output_size(&p, &o);
    let level = SourceLevel::for_size(source_edge_needed(&p, width.max(height)));
    let max_edge = if level == SourceLevel::Full { 0 } else { level.max_edge() };
    to_json(&ExportPlan { width, height, max_edge })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MapRequest {
    width: usize,
    height: usize,
    #[serde(default = "yes")]
    apply_crop: bool,
    points: Vec<[f64; 2]>,
    to_image: bool,
}

pub fn map_points(facts: &str, settings: &str, request: &str) -> Fallible<String> {
    let f: Facts = parse(facts)?;
    let d: DevelopSettings = parse(settings)?;
    let r: MapRequest = parse(request)?;
    let frame = Geometry::new(f.width.max(1) as usize, f.height.max(1) as usize, &d, r.apply_crop);
    let (w, h) = (r.width.max(1), r.height.max(1));
    let affine = if r.to_image { frame.out_to_norm(w, h) } else { frame.norm_to_out(w, h) };
    let mapped: Vec<[f64; 2]> = r
        .points
        .iter()
        .map(|&[x, y]| {
            let p = affine.apply(lightcraft_geom::Point::new(x, y));
            [p.x, p.y]
        })
        .collect();
    to_json(&mapped)
}

pub fn native_size(facts: &str, settings: &str) -> Fallible<String> {
    let f: Facts = parse(facts)?;
    let d: DevelopSettings = parse(settings)?;
    let (w, h) = pipeline::native_output_size(f.width.max(1) as usize, f.height.max(1) as usize, &d);
    to_json(&[w.round().max(1.0) as usize, h.round().max(1.0) as usize])
}

pub fn normalize_settings(settings: &str) -> Fallible<String> {
    let s: DevelopSettings = parse(settings)?;
    to_json(&s)
}

pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub histogram: Vec<u32>,
}

impl Frame {
    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn take_rgba(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.rgba)
    }

    pub fn take_histogram(&mut self) -> Vec<u32> {
        std::mem::take(&mut self.histogram)
    }
}

struct Loaded {
    source: DecodedSource,
    header: SourceInfo,
}

#[derive(Clone)]
struct FilmMeasure {
    glow: Option<Arc<Glow>>,
    scan: Scan,
}

#[derive(Default)]
pub struct Engine {
    sources: HashMap<String, Loaded>,
    stages: HashMap<String, Arc<StageCache>>,
    luts: LutLibrary,
    measures: HashMap<String, FilmMeasure>,
}

struct FilmJob<'a> {
    luts: &'a LutLibrary,
    film: Option<&'a FilmRequest>,
    light: Light,
    native_long: usize,
    reuse: Option<FilmMeasure>,
    frame: (usize, usize),
}

fn light_of(s: &DevelopSettings) -> Light {
    Light { contrast: s.light.contrast as f32, whites: s.light.whites as f32, blacks: s.light.blacks as f32 }
}

fn film_render(
    job: FilmJob<'_>,
    req: &RenderRequest,
    render: impl Fn() -> pipeline::Rendered,
) -> Fallible<(pipeline::Rendered, Option<FilmMeasure>)> {
    let Some(request) = job.film else { return Ok((render(), None)) };
    let Some(chain) = FilmChain::new(job.luts, request)? else { return Ok((render(), None)) };
    if let Some(reuse) = job.reuse {
        let frame = job.frame;
        let grain = GrainField::new(&request.grain, job.native_long, frame.0.max(frame.1));
        let hook = Arc::new(FilmHook { chain, glow: reuse.glow.clone(), grain, light: job.light, scan: reuse.scan, frame });
        let rendered = pipeline::film_hook::with_tone_hook(hook, &render);
        return Ok((rendered, Some(reuse)));
    }
    let capture = Arc::new(Capture::new(req.max_w, req.max_h));
    let probe = pipeline::film_hook::with_tone_hook(capture.clone(), &render);
    let (w, h) = (probe.image.width, probe.image.height);
    let mut lumas: Vec<f32> = capture
        .scene_samples(w, h, 60_000)
        .into_iter()
        .map(|c| {
            let d = chain.print_display(chain.negative_of(chain.log_of(contrast(c, job.light.contrast))));
            0.2126 * d[0] + 0.7152 * d[1] + 0.0722 * d[2]
        })
        .collect();
    let scan = Scan::measure(&mut lumas, job.light, request.auto_scan);
    let glow = Glow::from_capture(&capture, w, h, &request.halation).map(Arc::new);
    let grain = GrainField::new(&request.grain, job.native_long, w.max(h));
    let measure = FilmMeasure { glow: glow.clone(), scan };
    let hook = Arc::new(FilmHook { chain, glow, grain, light: job.light, scan, frame: (w, h) });
    Ok((pipeline::film_hook::with_tone_hook(hook, render), Some(measure)))
}

impl Engine {
    pub fn new() -> Engine {
        Engine::default()
    }

    pub fn load(&mut self, key: &str, facts: &str, bytes: &[u8], max_edge: usize) -> Fallible<()> {
        let f: Facts = parse(facts)?;
        let edge = if max_edge == 0 { usize::MAX } else { max_edge };
        let (image, info) = lightcraft_engine::files::load_bytes(bytes, edge).map_err(fail)?;
        let header = source_info(&photo_of(&f));
        self.sources.insert(key.to_string(), Loaded { source: DecodedSource::new(Arc::new(image), Some(info)), header });
        Ok(())
    }

    pub fn has(&self, key: &str) -> bool {
        self.sources.contains_key(key)
    }

    pub fn release(&mut self, key: &str) {
        self.sources.remove(key);
        self.stages.retain(|view, _| !view.starts_with(key));
        self.measures.retain(|view, _| !view.starts_with(key));
    }

    pub fn render(&mut self, key: &str, settings: &str, view: &str) -> Fallible<Frame> {
        let loaded = self.sources.get(key).ok_or_else(|| fail(format!("{key} is not loaded")))?;
        let s: DevelopSettings = parse(settings)?;
        let v: ViewRequest = parse(view)?;
        let req = v.request();
        let src = &loaded.source;
        let info = src.info_or(loaded.header.clone());
        let native_long = info.native_long.unwrap_or(src.image.width.max(src.image.height));
        let cache = v.stages.as_ref().map(|name| self.stages.entry(format!("{key}:{name}")).or_default().clone());
        let measure_key = v.stages.as_ref().map(|name| format!("{key}:{name}"));
        let reuse = if v.draft { measure_key.as_ref().and_then(|k| self.measures.get(k)).cloned() } else { None };
        let frame = Geometry::new(src.image.width, src.image.height, &s, req.apply_crop).fit(req.max_w, req.max_h);
        let job = FilmJob { luts: &self.luts, film: v.film.as_ref(), light: light_of(&s), native_long, reuse, frame };
        let (mut rendered, measure) = film_render(job, &req, || match &cache {
            Some(cache) => pipeline::render_cached(&src.image, &info, &s, &req, cache),
            None => pipeline::render(&src.image, &info, &s, &req),
        })?;
        if let (Some(k), Some(m)) = (measure_key, measure) {
            self.measures.insert(k, m);
        }
        if let Some(h) = pipeline::hdr_histogram(&src.image, &info, &s, &req) {
            rendered.histogram = h;
        }
        Ok(Frame {
            width: rendered.image.width as u32,
            height: rendered.image.height as u32,
            rgba: rendered.image.as_bytes(),
            histogram: rendered.histogram.to_words(),
        })
    }

    pub fn load_lut(&mut self, id: &str, text: &str) -> Fallible<()> {
        self.luts.insert(id, text)
    }

    pub fn has_lut(&self, id: &str) -> bool {
        self.luts.has(id)
    }

    pub fn export(&mut self, key: &str, facts: &str, state: &str, options: &str, film: &str) -> Fallible<Vec<u8>> {
        let film: Option<FilmRequest> = parse(film)?;
        let loaded = self.sources.get(key).ok_or_else(|| fail(format!("{key} is not loaded")))?;
        let p = edited_photo(&parse(facts)?, parse(state)?);
        let o: ExportOptions = parse(options)?;
        let (w, h) = output_size(&p, &o);
        let req = RenderRequest {
            max_w: w,
            max_h: h,
            quality: Quality::Full,
            apply_crop: true,
            overlay: Overlay::from_parts(0, 0.0),
            space: o.effective_space(),
            depth: o.effective_depth(),
            proof: None,
            window: None,
            display: None,
        };
        let info = loaded.source.info_or(loaded.header.clone());
        let native_long = info.native_long.unwrap_or(loaded.source.image.width.max(loaded.source.image.height));
        let job = FilmJob { luts: &self.luts, film: film.as_ref(), light: light_of(&p.develop), native_long, reuse: None, frame: (w, h) };
        let (rendered, _) = film_render(job, &req, || pipeline::render(&loaded.source.image, &info, &p.develop, &req))?;
        let meta = export_metadata(&p, &Catalog::default(), &o);
        encode_rendered(&rendered, &o, meta.as_ref())
    }
}

pub struct Commands {
    session: Session,
    photo: Option<PhotoId>,
    pixels: Arc<std::sync::Mutex<Option<Arc<Vec<u8>>>>>,
}

impl Default for Commands {
    fn default() -> Self {
        let pixels: Arc<std::sync::Mutex<Option<Arc<Vec<u8>>>>> = Arc::default();
        let mut session = Session::new();
        let source = pixels.clone();
        session.media.file_loader = Some(Arc::new(move |_path: &str, max_edge: usize| {
            let bytes = source.lock().unwrap_or_else(|e| e.into_inner()).clone();
            let bytes = bytes.ok_or("the photo's pixels aren't available yet; try again in a moment")?;
            lightcraft_engine::files::load_bytes(&bytes, max_edge)
        }));
        Commands { session, photo: None, pixels }
    }
}

impl Commands {
    pub fn new() -> Commands {
        Commands::default()
    }

    pub fn open(&mut self, facts: &str, settings: &str) -> Fallible<()> {
        let f: Facts = parse(facts)?;
        let d: DevelopSettings = parse(settings)?;
        if let Some(previous) = self.photo.take() {
            self.session.catalog.apply(Op::RemovePhoto { id: previous }).map_err(fail)?;
        }
        let id = self.session.catalog.alloc_photo_id();
        let mut p = photo_of(&f);
        p.id = id;
        p.develop = Arc::new(d);
        self.session.catalog.apply(Op::AddPhoto { photo: Box::new(p) }).map_err(fail)?;
        self.session.execute("library.select", &serde_json::json!({"ids": [id.0], "active": id.0})).map_err(fail)?;
        self.photo = Some(id);
        Ok(())
    }

    pub fn detach(&mut self) {
        *self.pixels.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }

    pub fn attach(&mut self, bytes: Vec<u8>) {
        *self.pixels.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(bytes));
    }

    pub fn run(&mut self, command: &str, params: &str) -> Fallible<String> {
        let id = self.photo.ok_or("no photo is open")?;
        let params: Value = parse(params)?;
        let result = self.session.execute(command, &params).map_err(fail)?;
        let settings = self.session.develop_of(id).ok_or("the photo left the session")?;
        to_json(&serde_json::json!({"result": result, "settings": &*settings}))
    }
}

