use std::collections::HashMap;
use std::sync::Arc;

use lightcraft_engine::Session;
use lightcraft_engine::catalog::{Catalog, ColorLabel, Flag, MediaKind, Meta, Op, Photo, PhotoId, Source};
use lightcraft_engine::develop::{CONTROLS, DevelopSettings, EmbeddedLens, SettingsGroup, WbMode, apply_partial, controls as specs, extract_groups};
use lightcraft_engine::pipeline::local::{effective_wb, normalize_wb};
use lightcraft_engine::export::{ExportOptions, MetadataPolicy, encode_rendered, export_metadata, output_size};
use lightcraft_engine::media::{DecodedSource, SourceLevel, source_info};
use lightcraft_engine::pipeline::geometry::Frame as Geometry;
use lightcraft_engine::pipeline::{self, MaskView, OutputDepth, OutputSpace, Overlay, PixelWindow, Quality, RenderRequest, SourceInfo, StageCache};
use lightcraft_engine::sidecar::{OWNED, OWNED_IF_STATED, merge_into, parse_sidecar, sidecar_packet};
use serde::{Deserialize, Serialize};

use crate::film::{Capture, Developer, FilmChain, FilmRequest, Light, LutLibrary, Scan, Scene, Textured, Toned};
use crate::texture::{self, Region, Size};
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
            quality: Quality::Full,
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

/// The settings value for a Temp slider position: kelvin for raw photos, the relative -100..100 scale (converted to
/// the kelvin LightCraft stores) for rendered ones.
pub fn temp_setting(facts: &str, slider: f64) -> Fallible<f64> {
    let p = photo_of(&parse(facts)?);
    Ok(if kelvin_scale(&p) { slider } else { rel_to_k(slider) })
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

fn native_frame(width: u32, height: u32, s: &DevelopSettings) -> Size {
    let (w, h) = pipeline::native_output_size(width.max(1) as usize, height.max(1) as usize, s);
    Size { w: w.round().max(1.0) as usize, h: h.round().max(1.0) as usize }
}

pub fn native_size(facts: &str, settings: &str) -> Fallible<String> {
    let f: Facts = parse(facts)?;
    let d: DevelopSettings = parse(settings)?;
    let (w, h) = pipeline::native_output_size(f.width.max(1) as usize, f.height.max(1) as usize, &d);
    to_json(&[w.round().max(1.0) as usize, h.round().max(1.0) as usize])
}

// why: in crop mode the loupe shows the uncropped frame, so its layout needs that frame's size
pub fn frame_size(facts: &str, settings: &str, apply_crop: bool) -> Fallible<String> {
    let f: Facts = parse(facts)?;
    let d: DevelopSettings = parse(settings)?;
    let (w, h) = Geometry::new(f.width.max(1) as usize, f.height.max(1) as usize, &d, apply_crop).native_size();
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
    width: u32,
    height: u32,
}

struct CachedScene {
    key: u64,
    scene: Arc<Scene>,
    ceiling: Option<(CeilingKey, f32)>,
    textured: Option<(String, Arc<Textured>)>,
    scan: Option<(String, Scan)>,
    toned: Option<(String, Arc<Toned>)>,
}

// what the halation's ceiling depends on besides the scene: contrast and the utility LUT
type CeilingKey = (u32, Option<u64>);

impl CachedScene {
    fn new(key: u64, scene: Scene) -> CachedScene {
        CachedScene { key, scene: Arc::new(scene), ceiling: None, textured: None, scan: None, toned: None }
    }

    fn look(&self, key: u64) -> CachedScene {
        CachedScene { key, scene: self.scene.clone(), ceiling: self.ceiling, textured: self.textured.clone(), scan: self.scan.clone(), toned: None }
    }

    fn textured(&mut self, dev: &Developer, key: &str, out: Region) -> Arc<Textured> {
        if let Some((k, t)) = &self.textured
            && *k == key
        {
            return t.clone();
        }
        let t = Arc::new(dev.textured(&self.scene, out));
        self.textured = Some((key.to_string(), t.clone()));
        t
    }

    fn on_cpu(&mut self, dev: &Developer, key: &str, out: Region) -> Arc<Textured> {
        let t = Arc::new(dev.textured_on_cpu(&self.scene, out));
        self.textured = Some((key.to_string(), t.clone()));
        t
    }

    // why: a texture kept on the GPU can be dropped there (or the device lost), so each GPU step
    // falls back to redoing the texture on the CPU
    fn scan(&mut self, dev: &Developer, keys: (&str, &str), textured: Arc<Textured>, out: Region, auto: bool) -> (Scan, Arc<Textured>) {
        if let Some((k, scan)) = &self.scan
            && *k == keys.1
        {
            return (scan.clone(), textured);
        }
        let (scan, textured) = match dev.scan(&textured, auto) {
            Some(scan) => (scan, textured),
            None => {
                let t = self.on_cpu(dev, keys.0, out);
                (dev.scan(&t, auto).unwrap_or_else(|| Scan::measure(&mut [], dev.light, false)), t)
            }
        };
        self.scan = Some((keys.1.to_string(), scan.clone()));
        (scan, textured)
    }
}

/// A photo decoded outside any engine, so a slow raw decode never holds an engine's lock.
pub struct Decoded(Loaded);

pub fn decode(facts: &str, bytes: &[u8], max_edge: usize) -> Fallible<Decoded> {
    let f: Facts = parse(facts)?;
    let edge = if max_edge == 0 { usize::MAX } else { max_edge };
    let (image, info) = lightcraft_engine::files::load_bytes(bytes, edge).map_err(fail)?;
    let header = source_info(&photo_of(&f));
    Ok(Decoded(Loaded { source: DecodedSource::new(Arc::new(image), Some(info)), header, width: f.width, height: f.height }))
}

#[derive(Default)]
pub struct Engine {
    sources: HashMap<String, Loaded>,
    originals: HashMap<String, lightcraft_meta::Metadata>,
    stages: HashMap<String, Arc<StageCache>>,
    luts: LutLibrary,
    scenes: HashMap<String, CachedScene>,
    looks: HashMap<String, CachedScene>,
}

const DISPLAY_GROUPS: [&str; 9] = ["curve", "color", "mixer", "bw_mix", "point_colors", "grading", "vignette", "grain", "treatment"];

fn look_key(source: &str, s: &DevelopSettings, req: &RenderRequest) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut v = s.to_json();
    if let Some(map) = v.as_object_mut() {
        for group in DISPLAY_GROUPS {
            map.remove(group);
        }
        if let Some(light) = map.get_mut("light").and_then(Value::as_object_mut) {
            for k in ["contrast", "whites", "blacks"] {
                light.remove(k);
            }
        }
    }
    let mut h = std::collections::hash_map::DefaultHasher::new();
    source.hash(&mut h);
    v.to_string().hash(&mut h);
    (req.apply_crop, req.overlay.key()).hash(&mut h);
    h.finish()
}

fn view_key(look: u64, req: &RenderRequest) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (look, req.max_w, req.max_h, req.window.map(|w| (w.x, w.y, w.w, w.h))).hash(&mut h);
    h.finish()
}

#[derive(Clone, Copy)]
enum Lane {
    View,
    Look,
}

struct FilmJob<'a> {
    luts: &'a LutLibrary,
    film: Option<&'a FilmRequest>,
    light: Light,
    view: Option<CachedScene>,
    look: Option<CachedScene>,
    look_key: u64,
    native: Size,
}

struct FilmOutcome {
    rendered: pipeline::Rendered,
    keep: Option<Region>,
    view: Option<CachedScene>,
    look: Option<CachedScene>,
}

// why: the pixels leave as bytes without a copy, and a zoomed window's margin is cut off its rows in parallel
fn cropped(w: usize, h: usize, rgba: Vec<[u8; 4]>, keep: Option<Region>) -> (usize, usize, Vec<u8>) {
    use rayon::prelude::*;
    match keep {
        Some(k) if (k.w, k.h) != (w, h) => {
            let mut out = vec![0u8; k.w * k.h * 4];
            out.par_chunks_mut(k.w.max(1) * 4).enumerate().for_each(|(j, row)| row.copy_from_slice(rgba[(k.y + j) * w + k.x..][..k.w].as_flattened()));
            (k.w, k.h, out)
        }
        _ => (w, h, rgba.into_flattened()),
    }
}

fn light_of(s: &DevelopSettings) -> Light {
    Light { contrast: s.light.contrast as f32, whites: s.light.whites as f32, blacks: s.light.blacks as f32 }
}

fn capture_scene(region: Region, render: impl Fn() -> pipeline::Rendered) -> Scene {
    let capture = Arc::new(Capture::new(region));
    pipeline::film_hook::with_tone_hook(capture.clone(), render);
    match Arc::try_unwrap(capture) {
        Ok(owned) => owned.into_scene(),
        Err(shared) => shared.scene(),
    }
}

// why: scan levels belong to the whole frame, so a zoomed window measures them on this small
// render of it rather than on the part it shows; an export uses the same render so it matches
const LOOK_EDGE: usize = 1024;
const EXPORT_BAND: usize = 1024;

fn textured_key(request: &FilmRequest, dev: &Developer) -> String {
    let ceiling = dev.texture.and_then(|t| t.halation).map(|h| h.ceiling);
    format!("{:?}{}{}{ceiling:?}", request.utility, request.texture_key(), dev.light.contrast)
}

fn scan_key(request: &FilmRequest, dev: &Developer) -> String {
    format!("{}{:?}{:?}{}{:?}", textured_key(request, dev), request.negative, request.print, request.auto_scan, dev.light)
}

/// The small render of the whole frame that scan levels and the halation's ceiling are measured on.
fn whole_look<'a>(
    req: &RenderRequest,
    size_of: impl Fn(&RenderRequest) -> (usize, usize),
    render: impl Fn(&RenderRequest) -> pipeline::Rendered,
    look: &'a mut Option<CachedScene>,
    look_key: u64,
) -> &'a mut CachedScene {
    let whole = RenderRequest { max_w: LOOK_EDGE, max_h: LOOK_EDGE, window: None, ..*req };
    look.get_or_insert_with(|| {
        let (lw, lh) = size_of(&whole);
        let fresh = CachedScene::new(look_key, capture_scene(Region::whole(lw, lh), || render(&whole)));
        crate::gpu::prefetch(&fresh.scene);
        fresh
    })
}

/// Sets the halation's ceiling from `cached`, a render of the whole frame (halation::Headroom), which keeps it while
/// what it depends on stays.
fn measure_ceiling(dev: &mut Developer, cached: &mut CachedScene) {
    if dev.texture.is_some_and(|t| t.halation.is_some()) {
        let key = (dev.light.contrast.to_bits(), dev.chain.utility.as_ref().map(|u| u.id));
        let ceiling = match cached.ceiling {
            Some((k, ceiling)) if k == key => ceiling,
            _ => {
                let ceiling = spent("ceiling", || dev.chain.ceiling(&cached.scene, dev.light.contrast));
                cached.ceiling = Some((key, ceiling));
                ceiling
            }
        };
        if pipeline::profiling() {
            eprintln!("  film halation ceiling: {ceiling:.4}");
        }
        dev.texture = dev.texture.map(|t| t.with_ceiling(ceiling));
    }
}

fn whole_scan(
    dev: &Developer,
    request: &FilmRequest,
    req: &RenderRequest,
    size_of: impl Fn(&RenderRequest) -> (usize, usize),
    render: impl Fn(&RenderRequest) -> pipeline::Rendered,
    look: &mut Option<CachedScene>,
    look_key: u64,
) -> Scan {
    let scan_key = scan_key(request, dev);
    let look = whole_look(req, size_of, render, look, look_key);
    if let Some((_, scan)) = look.scan.as_ref().filter(|(k, _)| *k == scan_key) {
        return scan.clone();
    }
    let region = look.scene.region;
    let textured_key = textured_key(request, dev);
    let textured = look.textured(dev, &textured_key, region);
    look.scan(dev, (&textured_key, &scan_key), textured, region, request.auto_scan).0
}

fn film_export(
    job: FilmJob<'_>,
    req: &RenderRequest,
    bands: bool,
    size_of: impl Fn(&RenderRequest) -> (usize, usize),
    render: impl Fn(&RenderRequest) -> pipeline::Rendered,
) -> Fallible<pipeline::Rendered> {
    let Some(request) = job.film else { return Ok(render(req)) };
    let Some(chain) = FilmChain::new(job.luts, request)? else { return Ok(render(req)) };
    let mut dev = Developer { chain, light: job.light, texture: request.texture_params(job.native) };
    let (fw, fh) = size_of(req);
    let frame = Size { w: fw, h: fh };
    let mut look = None;
    measure_ceiling(&mut dev, whole_look(req, &size_of, &render, &mut look, 0));
    let scan = spent("export scan", || whole_scan(&dev, request, req, &size_of, &render, &mut look, 0));
    let margin = dev.texture.map_or(0, |p| texture::margin(frame, &p));
    let whole = spent("export capture", || capture_scene(Region::whole(fw, fh), || render(req)));
    let band = |y: usize| Region { x: 0, y, w: fw, h: EXPORT_BAND.min(fh - y) };
    let part = |out: Region| {
        let scene = whole.part(out.grown(margin, frame));
        crate::gpu::prefetch(&scene);
        scene
    };
    // why: with `bands`, the final pass is made a band at a time (`Toned::band`), each while the GPU textures the
    // next band, and no toned copy of the whole frame is kept; every row is the one the whole frame's pass makes.
    // Otherwise the bands' toned rows are read back into the frame's own, without a buffer of their own and a copy
    let mut rgb = if bands { Vec::new() } else { vec![[0.0f32; 3]; fw * fh] };
    let mut image: Vec<[u8; 4]> = Vec::with_capacity(if bands { fw * fh } else { 0 });
    let mut scene = (fh > 0).then(|| spent("band part", || part(band(0))));
    let mut tex = scene.as_ref().map(|s| spent("band textured", || dev.band(s, band(0))));
    for y in (0..fh).step_by(EXPORT_BAND) {
        let out = band(y);
        let (Some(this), Some(textured)) = (scene.take(), tex.take()) else { break };
        // why: the next band is cut and sent to the GPU while the GPU textures this one
        let next = (y + EXPORT_BAND < fh).then(|| spent("band part", || part(band(y + EXPORT_BAND))));
        let mut own = Vec::new();
        let rows: &mut [[f32; 3]] = if bands {
            own = vec![[0.0f32; 3]; out.w * out.h];
            &mut own
        } else {
            &mut rgb[y * fw..(y + out.h) * fw]
        };
        if !spent("band finish", || dev.finish_into(&textured, &scan, rows)) && !dev.finish_into(&dev.textured_on_cpu(&this, out), &scan, rows) {
            return Err("the film stage failed on the CPU".into());
        }
        tex = next.as_ref().map(|s| spent("band textured", || dev.band(s, band(y + EXPORT_BAND))));
        scene = next;
        if bands {
            let toned = Arc::new(Toned { region: out, rgb: own, masks: whole.masks.clone(), band: true });
            let rendered = spent("band final", || pipeline::film_hook::with_tone_hook(toned, || render(req)));
            if (rendered.image.width, rendered.image.height) != (fw, out.h) {
                return Err("an export band's final pass made the wrong rows".into());
            }
            image.extend_from_slice(&rendered.image.data);
        }
    }
    if bands {
        let image = lightcraft_raster::Rgba8 { width: fw, height: fh, data: image };
        let histogram = lightcraft_raster::Histogram::of_srgb8(&image);
        return Ok(pipeline::Rendered { image, histogram, deep: None });
    }
    let toned = Arc::new(Toned { region: Region::whole(fw, fh), rgb, masks: whole.masks.clone(), band: false });
    Ok(spent("export final", || pipeline::film_hook::with_tone_hook(toned, || render(req))))
}

fn spent<R>(what: &str, f: impl FnOnce() -> R) -> R {
    if !pipeline::profiling() {
        return f();
    }
    let t = std::time::Instant::now();
    let r = f();
    eprintln!("  film {what}: {:.1} ms", t.elapsed().as_secs_f64() * 1e3);
    r
}

fn film_render(
    job: FilmJob<'_>,
    req: &RenderRequest,
    size_of: impl Fn(&RenderRequest) -> (usize, usize),
    render: impl Fn(&RenderRequest, Lane) -> pipeline::Rendered,
) -> Fallible<FilmOutcome> {
    let plain = |job: FilmJob<'_>| FilmOutcome { rendered: render(req, Lane::View), keep: None, view: job.view, look: job.look };
    let Some(request) = job.film else { return Ok(plain(job)) };
    let Some(chain) = FilmChain::new(job.luts, request)? else { return Ok(plain(job)) };
    let mut dev = Developer { chain, light: job.light, texture: request.texture_params(job.native) };
    let (fw, fh) = size_of(req);
    let frame = Size { w: fw, h: fh };
    let window = req.window.map(|w| w.clamped(fw, fh));
    let out = window.map_or(Region::whole(fw, fh), |w| Region { x: w.x, y: w.y, w: w.w, h: w.h });
    let key = view_key(job.look_key, req);
    let margin = dev.texture.map_or(0, |p| texture::margin(frame, &p));
    let region = if window.is_some() { out.grown(margin, frame) } else { out };
    // why: the final pass renders the captured (grown) window too, so LightCraft's stage cache hits;
    // the margin is cut off afterwards
    let capture = RenderRequest { window: window.map(|_| PixelWindow { x: region.x, y: region.y, w: region.w, h: region.h }), ..*req };
    let mut view = match job.view.filter(|c| c.key == key) {
        Some(c) => c,
        None => {
            let fresh = CachedScene::new(key, spent("capture", || capture_scene(region, || render(&capture, Lane::View))));
            spent("prefetch", || crate::gpu::prefetch(&fresh.scene));
            fresh
        }
    };
    // why: a zoomed window takes the halation's ceiling from the whole frame, as its scan levels, so it glows as the
    // fit view and the export do
    let mut look = window.and(job.look).filter(|c| c.key == job.look_key);
    match window {
        Some(_) => measure_ceiling(&mut dev, whole_look(req, &size_of, |r| render(r, Lane::Look), &mut look, job.look_key)),
        None => measure_ceiling(&mut dev, &mut view),
    }
    let textured_key = textured_key(request, &dev);
    let scan_key = scan_key(request, &dev);
    let mut textured = spent("textured", || view.textured(&dev, &textured_key, out));
    let (scan, look) = if window.is_some() {
        let scan = whole_scan(&dev, request, req, &size_of, |r| render(r, Lane::Look), &mut look, job.look_key);
        (scan, look.ok_or("the whole-frame render is missing")?)
    } else {
        let (scan, kept) = spent("scan", || view.scan(&dev, (&textured_key, &scan_key), textured.clone(), out, request.auto_scan));
        textured = kept;
        (scan, view.look(job.look_key))
    };
    let toned_key = format!("{scan_key}{:?}", request);
    let toned = match view.toned.as_ref().filter(|(k, _)| *k == toned_key) {
        Some((_, t)) => t.clone(),
        None => {
            let t = match spent("finish", || dev.finish(&textured, &scan)) {
                Some(t) => t,
                None => {
                    let cpu = view.on_cpu(&dev, &textured_key, out);
                    dev.finish(&cpu, &scan).ok_or("the film stage failed on the CPU")?
                }
            };
            let t = Arc::new(Toned { masks: view.scene.masks.clone(), ..t });
            view.toned = Some((toned_key, t.clone()));
            t
        }
    };
    let rendered = spent("final", || pipeline::film_hook::with_tone_hook(toned, || render(&capture, Lane::View)));
    let keep = window.map(|_| Region { x: out.x - region.x, y: out.y - region.y, w: out.w, h: out.h });
    Ok(FilmOutcome { rendered, keep, view: Some(view), look: Some(look) })
}

impl Engine {
    pub fn new() -> Engine {
        Engine::default()
    }

    pub fn load(&mut self, key: &str, facts: &str, bytes: &[u8], max_edge: usize) -> Fallible<()> {
        let decoded = decode(facts, bytes, max_edge)?;
        self.insert(key, decoded);
        self.originals.insert(key.to_string(), lightcraft_meta::extract(bytes));
        Ok(())
    }

    pub fn insert(&mut self, key: &str, decoded: Decoded) {
        self.sources.insert(key.to_string(), decoded.0);
    }

    /// The decode loaded as `key`, for another engine to [`Engine::insert`] (its pixels are shared, not copied): an
    /// export of the photo on screen can take the loupe's decode at the size it needs instead of decoding again.
    pub fn decoded(&self, key: &str) -> Option<Decoded> {
        let l = self.sources.get(key)?;
        Some(Decoded(Loaded { source: l.source.clone(), header: l.header.clone(), width: l.width, height: l.height }))
    }

    /// Reads the original file's metadata for `key`'s exports, as [`Engine::load`] does besides the pixels.
    pub fn read_original(&mut self, key: &str, bytes: &[u8]) {
        self.originals.insert(key.to_string(), lightcraft_meta::extract(bytes));
    }

    pub fn has(&self, key: &str) -> bool {
        self.sources.contains_key(key)
    }

    /// Drops the caches one view of a source keeps (its stage cache and film scene), e.g. a zoomed window once the
    /// view is back to fit.
    pub fn forget_view(&mut self, key: &str, stages: &str) {
        let view = format!("{key}:{stages}");
        self.stages.remove(&view);
        self.scenes.remove(&view);
        self.stages.remove(&format!("{key}:look"));
        self.looks.remove(key);
    }

    pub fn release(&mut self, key: &str) {
        self.sources.remove(key);
        self.originals.remove(key);
        self.stages.retain(|view, _| !view.starts_with(key));
        self.scenes.retain(|view, _| !view.starts_with(key));
        self.looks.remove(key);
    }

    pub fn render(&mut self, key: &str, settings: &str, view: &str) -> Fallible<Frame> {
        let loaded = self.sources.get(key).ok_or_else(|| fail(format!("{key} is not loaded")))?;
        let s: DevelopSettings = parse(settings)?;
        let v: ViewRequest = parse(view)?;
        let req = v.request();
        let src = &loaded.source;
        let info = src.info_or(loaded.header.clone());
        let cache = v.stages.as_ref().map(|name| self.stages.entry(format!("{key}:{name}")).or_default().clone());
        let look_cache = (v.film.is_some() && req.window.is_some()).then(|| self.stages.entry(format!("{key}:look")).or_default().clone());
        let stored = v.stages.as_ref().map(|name| format!("{key}:{name}"));
        let job = FilmJob {
            luts: &self.luts,
            film: v.film.as_ref(),
            light: light_of(&s),
            view: stored.as_ref().and_then(|k| self.scenes.remove(k)),
            look: self.looks.remove(key),
            look_key: look_key(key, &s, &req),
            native: native_frame(loaded.width, loaded.height, &s),
        };
        let size_of = |r: &RenderRequest| pipeline::output_size(src.image.width, src.image.height, &s, r);
        let outcome = film_render(job, &req, size_of, |r, lane| match (lane, &cache, &look_cache) {
            (Lane::View, Some(cache), _) | (Lane::Look, _, Some(cache)) => pipeline::render_cached(&src.image, &info, &s, r, cache),
            _ => pipeline::render(&src.image, &info, &s, r),
        })?;
        let mut rendered = outcome.rendered;
        let keep = outcome.keep;
        if let (Some(k), Some(view)) = (stored, outcome.view) {
            self.scenes.insert(k, view);
        }
        if let Some(look) = outcome.look {
            self.looks.insert(key.to_string(), look);
        }
        if let Some(h) = spent("hdr histogram", || pipeline::hdr_histogram(&src.image, &info, &s, &req)) {
            rendered.histogram = h;
        }
        let (width, height, rgba) = spent("crop", || cropped(rendered.image.width, rendered.image.height, rendered.image.data, keep));
        Ok(Frame { width: width as u32, height: height as u32, rgba, histogram: rendered.histogram.to_words() })
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
        let native = native_frame(p.width, p.height, &p.develop);
        let job = FilmJob { luts: &self.luts, film: film.as_ref(), light: light_of(&p.develop), view: None, look: None, look_key: 0, native };
        let size_of = |r: &RenderRequest| pipeline::output_size(loaded.source.image.width, loaded.source.image.height, &p.develop, r);
        // (by bands for an 8-bit render whose plan reads no pixels: see film_export)
        let bands = req.depth == OutputDepth::U8 && req.overlay == Overlay::None && !pipeline::plan_reads_pixels(&p.develop);
        let rendered = film_export(job, &req, bands, size_of, |r| pipeline::render(&loaded.source.image, &info, &p.develop, r))?;
        let meta = export_metadata(&p, &Catalog::default(), &o).map(|m| {
            let mut m = match self.originals.get(key) {
                Some(original) => with_original(m, original, &o),
                None => m,
            };
            if o.metadata != MetadataPolicy::Copyright {
                m.label = p.label.map(|l| format!("{l:?}"));
            }
            m.software = Some("Filmlab".into());
            m
        });
        spent("export encode", || encode_rendered(&rendered, &o, meta.as_ref()))
    }
}

fn trimmed(text: &Option<String>) -> Option<String> {
    text.as_deref().map(str::trim).filter(|t| !t.is_empty()).map(String::from)
}

// why: LightCraft's export writes the fields its catalog keeps, and an export should carry the rest of the original's
// metadata as a raw developer's would: make apart from the model, serial numbers, lens, flash, exposure bias and
// program, metering, white balance, 35 mm focal length, GPS with altitude, the capture time with its zone, and the
// creator's text; the export's policy (camera details, location) still decides what goes out
fn with_original(mut out: lightcraft_meta::Metadata, original: &lightcraft_meta::Metadata, o: &ExportOptions) -> lightcraft_meta::Metadata {
    macro_rules! fill {
        ($($field:ident),*) => {$(
            if out.$field.is_none() {
                out.$field = original.$field.clone();
            }
        )*};
    }
    macro_rules! fill_text {
        ($($field:ident),*) => {$(
            if trimmed(&out.$field).is_none() {
                out.$field = trimmed(&original.$field);
            }
        )*};
    }
    fill_text!(artist, copyright, usage_terms, copyright_url);
    fill!(copyright_marked);
    if o.metadata == MetadataPolicy::Copyright {
        return out;
    }
    fill_text!(title, caption, alt_text, extended_description);
    if out.keywords.is_empty() {
        out.keywords.clone_from(&original.keywords);
        out.hierarchical_keywords.clone_from(&original.hierarchical_keywords);
    }
    if original.capture_time.is_some() {
        out.capture_time = original.capture_time.clone();
    }
    if !o.remove_location {
        fill_text!(sublocation, city, state, country);
        if original.gps.is_some() {
            out.gps = original.gps.clone();
        }
    }
    if o.metadata != MetadataPolicy::All {
        return out;
    }
    if let (Some(make), Some(model)) = (trimmed(&original.make), trimmed(&original.model)) {
        out.make = Some(make);
        out.model = Some(model);
    }
    fill_text!(serial_number, lens_make, lens_model, lens_serial_number);
    fill!(lens_spec, exposure_time, f_number, iso, focal_length, focal_length_35mm, flash, exposure_bias, exposure_program, metering_mode, white_balance);
    out
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

    /// Every command the session knows, as (id, params doc).
    pub fn catalog(&self) -> Vec<(&'static str, &'static str)> {
        self.session.commands().into_iter().map(|c| (c.id, c.params)).collect()
    }

    /// A command's own name, as LightCraft's menus show it.
    pub fn label_of(&self, id: &str) -> Option<&'static str> {
        self.session.commands().into_iter().find(|c| c.id == id).map(|c| c.label)
    }

    pub fn run(&mut self, command: &str, params: &str) -> Fallible<String> {
        let id = self.photo.ok_or("no photo is open")?;
        let params: Value = parse(params)?;
        let result = self.session.execute(command, &params).map_err(fail)?;
        let settings = self.session.develop_of(id).ok_or("the photo left the session")?;
        to_json(&serde_json::json!({"result": result, "settings": &*settings}))
    }
}

