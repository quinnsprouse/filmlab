use serde::Serialize;

use crate::film::{CubeLut, Developer, Scan, Scene, Textured, Toned, contrast_slope, next_id};
use crate::dust;
use crate::grain::{self, Field, GrainParams};
use crate::grain_tables::DOMAIN;
use crate::halation;
use crate::pyramid::Span;
use crate::texture::{self, Guide, Region, Size, Tap, TextureParams};
use crate::halation_tables::{F_BELOW_SLOPE, F_GRID, FROM_DWG, KAPPA, MASK_COEF, TO_DWG};
use crate::texture_tables::{BASE, BEND2, BEND3, CHROMA, GAIN_HIGH, GAIN_LOW, KNEE, KNEE_TAIL, TONE};

const DWG: [[f32; 3]; 6] = [TO_DWG[0], TO_DWG[1], TO_DWG[2], FROM_DWG[0], FROM_DWG[1], FROM_DWG[2]];
const MASK: [f32; 21] = {
    let mut m = [0.0f32; 21];
    m[0] = KAPPA;
    m[1] = F_BELOW_SLOPE;
    let mut i = 0;
    while i < 19 {
        m[2 + i] = MASK_COEF[i];
        i += 1;
    }
    m
};

const CONTRAST: u32 = 1;
const UTILITY: u32 = 2;
const COEFFICIENTS: u32 = 4;
const CANDIDATE: u32 = 8;
const HIGHLIGHTS: u32 = 16;
const VERTICAL: u32 = 32;
const ACCUMULATE: u32 = 64;
const SAMPLES: usize = 60_000;
const GROUP: usize = 16;

#[derive(Serialize)]
struct Block {
    id: String,
    at: usize,
    len: usize,
}

impl Block {
    fn of<T>(id: String, data: &[T]) -> Block {
        Block { id, at: data.as_ptr() as usize, len: std::mem::size_of_val(data) / 4 }
    }
}

#[derive(Serialize)]
struct Buffer {
    name: String,
    words: usize,
    upload: Option<Block>,
    pack: bool,
}

#[derive(Serialize)]
struct Write {
    buffer: &'static str,
    offset: usize,
    block: Block,
}

#[derive(Serialize)]
struct Pass {
    entry: &'static str,
    params: Vec<u32>,
    bind: Vec<(u32, String)>,
    groups: [usize; 2],
}

#[derive(Serialize)]
struct Read {
    buffer: String,
    at: usize,
    len: usize,
}

#[derive(Serialize)]
struct Copy {
    from: String,
    to: &'static str,
    offset: usize,
}

#[derive(Default, Serialize)]
struct Program {
    buffers: Vec<Buffer>,
    copies: Vec<Copy>,
    writes: Vec<Write>,
    passes: Vec<Pass>,
    reads: Vec<Read>,
}

impl Program {
    fn buffer(&mut self, name: &str, words: usize) {
        self.buffers.push(Buffer { name: name.to_string(), words, upload: None, pack: false });
    }

    fn upload<T>(&mut self, name: String, data: &[T], pack: bool) -> String {
        let words = std::mem::size_of_val(data) / 4;
        let words = if pack { words / 3 * 4 } else { words };
        self.buffers.push(Buffer { name: name.clone(), words, upload: Some(Block::of(name.clone(), data)), pack });
        name
    }

    // why: a scratch buffer lives from the first pass that binds it to the last (or to the readback); scratch whose
    // lives don't overlap share one slot, so a render holds a few frame-sized buffers instead of one per step.
    // Buffers filled by writes or copies are filled before any pass runs, so they keep their own
    fn alias(&mut self) {
        let end = self.passes.len();
        let pinned: Vec<&str> = self.writes.iter().map(|w| w.buffer).chain(self.copies.iter().flat_map(|c| [c.to, c.from.as_str()])).collect();
        let scratch = |name: &str| name.starts_with("tmp:") && !pinned.contains(&name);
        let mut lives: Vec<(String, usize, usize)> = Vec::new();
        let uses = self.passes.iter().enumerate().flat_map(|(k, p)| p.bind.iter().map(move |(_, n)| (n, k))).chain(self.reads.iter().map(|r| (&r.buffer, end)));
        for (name, at) in uses.filter(|(n, _)| scratch(n)) {
            match lives.iter_mut().find(|l| &l.0 == name) {
                Some(life) => life.2 = at,
                None => lives.push((name.clone(), at, at)),
            }
        }
        let words = |name: &str| self.buffers.iter().filter(|b| b.name == name && b.upload.is_none()).map(|b| b.words).max();
        let mut slots: Vec<(usize, usize)> = Vec::new();
        let mut renamed: Vec<(String, String)> = Vec::new();
        for (name, first, last) in &lives {
            let Some(need) = words(name) else { continue };
            let free = slots.iter().enumerate().filter(|(_, s)| s.0 < *first);
            let fit = free.clone().filter(|(_, s)| s.1 >= need).min_by_key(|(_, s)| s.1).or_else(|| free.max_by_key(|(_, s)| s.1)).map(|(k, _)| k);
            let k = fit.unwrap_or_else(|| {
                slots.push((0, 0));
                slots.len() - 1
            });
            slots[k] = (*last, slots[k].1.max(need));
            renamed.push((name.clone(), format!("tmp:slot{k}")));
        }
        let to = |name: &str| renamed.iter().find(|r| r.0 == name).map(|r| r.1.clone());
        self.buffers.retain(|b| to(&b.name).is_none() && !(scratch(&b.name) && b.upload.is_none()));
        self.buffers.extend(slots.iter().enumerate().map(|(k, s)| Buffer { name: format!("tmp:slot{k}"), words: s.1, upload: None, pack: false }));
        for (_, name) in self.passes.iter_mut().flat_map(|p| p.bind.iter_mut()) {
            if let Some(slot) = to(name) {
                *name = slot;
            }
        }
        for r in &mut self.reads {
            if let Some(slot) = to(&r.buffer) {
                r.buffer = slot;
            }
        }
    }

    fn run(mut self) -> bool {
        self.alias();
        serde_json::to_string(&self).is_ok_and(|json| bridge::run(&json))
    }
}

#[derive(Clone, Copy)]
struct Frame<'a> {
    flags: u32,
    radius: u32,
    count: u32,
    src: Region,
    dst: Region,
    bounds: [u32; 4],
    taps: [u32; 4],
    aux: [u32; 4],
    scale: f32,
    dev: &'a Developer,
}

fn region(r: Region) -> [u32; 4] {
    [r.x as u32, r.y as u32, r.w as u32, r.h as u32]
}

fn rows(m: &[[f32; 3]; 3]) -> impl Iterator<Item = u32> + '_ {
    m.iter().flat_map(|r| [r[0].to_bits(), r[1].to_bits(), r[2].to_bits(), 0])
}

fn lut_words(lut: Option<&CubeLut>, offset: usize) -> [u32; 12] {
    let Some(l) = lut else { return [0; 12] };
    let [a, b, c] = l.min.map(f32::to_bits);
    let [d, e, f] = l.max.map(f32::to_bits);
    [l.size as u32, u32::from(l.unit), offset as u32, 0, a, b, c, 0, d, e, f, 0]
}

fn lut_offsets(dev: &Developer) -> [usize; 3] {
    let chain = &dev.chain;
    let utility = chain.utility.as_ref().map_or(0, |u| u.data.len());
    [0, utility, utility + chain.negative.data.len()]
}

impl Frame<'_> {
    fn words(&self) -> Vec<u32> {
        let chain = &self.dev.chain;
        let amount = self.dev.light.contrast;
        let slope = if amount != 0.0 { contrast_slope(amount) } else { 1.0 };
        let mut w = vec![0, self.flags, self.radius, self.count];
        for part in [region(self.src), region(self.dst), self.bounds, self.taps, self.aux] {
            w.extend(part);
        }
        w.extend([slope.to_bits(), self.scale.to_bits(), 0, 0]);
        w.extend(rows(&chain.to_awg));
        w.extend(rows(&chain.from_709));
        let [u, n, p] = lut_offsets(self.dev);
        w.extend(lut_words(chain.utility.as_deref(), u));
        w.extend(lut_words(Some(&chain.negative), n));
        w.extend(lut_words(Some(&chain.print), p));
        w
    }
}

fn groups(r: Region) -> [usize; 2] {
    [r.w.div_ceil(GROUP), r.h.div_ceil(GROUP)]
}

fn tap_words(taps: &[Tap]) -> impl Iterator<Item = u32> + '_ {
    taps.iter().flat_map(|t| [t.lo as u32, t.hi as u32, t.f.to_bits()])
}

const CONSTS: [f32; 10] = [GAIN_LOW, GAIN_HIGH, BEND2, BEND3, 0.01, CHROMA, KNEE_TAIL[0], KNEE_TAIL[1], KNEE_TAIL[2], KNEE_TAIL[3]];
const TRANSFER_AT: usize = 29068;
const DWG_AT: usize = 37069;
const MASK_AT: usize = 37087;
const TABLE_WORDS: usize = 37108;

fn tables(program: &mut Program, dev: &Developer, scan: Option<&Scan>, weights: &[f32]) {
    program.buffer("tables", TABLE_WORDS + weights.len().max(1));
    let chain = &dev.chain;
    let mut put = |offset: usize, id: &str, data: &[f32]| program.writes.push(Write { buffer: "tables", offset, block: Block::of(id.to_string(), data) });
    put(0, "log2", chain.log2);
    put(4097, "exp2", chain.exp2);
    if let Some(scan) = scan {
        put(8194, "scan", &scan.table[..]);
    }
    put(16387, "base", &BASE);
    put(17388, "tone", &TONE);
    put(18389, "consts", &CONSTS);
    put(18399, "knee", &KNEE);
    put(18720, "domain", &DOMAIN);
    put(24971, "cos", grain::cos_table());
    put(TRANSFER_AT, "transfer", &F_GRID);
    put(DWG_AT, "dwg", DWG.as_flattened());
    put(MASK_AT, "mask", &MASK);
    if !weights.is_empty() {
        put(TABLE_WORDS, "weights", weights);
    }
}

fn luts(program: &mut Program, dev: &Developer) -> String {
    let chain = &dev.chain;
    let [u, n, p] = lut_offsets(dev);
    let mut parts: Vec<(&CubeLut, usize)> = vec![(&chain.negative, n), (&chain.print, p)];
    if let Some(utility) = chain.utility.as_deref() {
        parts.push((utility, u));
    }
    let total = p + chain.print.data.len();
    program.buffer("tmp:luts", total * 4);
    for (lut, offset) in parts {
        let name = program.upload(format!("lut:{}", lut.id), &lut.data, true);
        program.copies.push(Copy { from: name, to: "tmp:luts", offset: offset * 4 });
    }
    "tmp:luts".to_string()
}

fn scene_name(scene: &Scene) -> String {
    format!("scene:{}", scene.id)
}

// why: a scene or texture on the GPU is freed with the engine object that names it, so the GPU holds only the
// frames the engine still caches
pub(crate) fn forget_scene(id: u64) {
    bridge::forget(&format!("scene:{id}"));
}

pub(crate) fn forget_lut(id: u64) {
    bridge::forget(&format!("lut:{id}"));
}

pub(crate) fn forget_texture(id: u64) {
    bridge::forget(&format!("tex:{id}"));
}

pub fn prefetch(scene: &Scene) {
    if let Ok(block) = serde_json::to_string(&Block::of(scene_name(scene), &scene.rgb)) {
        bridge::upload(&block);
    }
}

struct Plan<'a> {
    frame: Size,
    region: Region,
    out: Region,
    texture: Option<TextureParams>,
    dust: &'a [u32],
}

struct Blur {
    level: usize,
    at: u32,
    radius: u32,
    down: u32,
    down_radius: u32,
    up: u32,
}

struct HalationAt {
    octaves: [Blur; 3],
    mask: Option<Blur>,
    weights: u32,
    total: u32,
    reach: usize,
}

#[derive(Default)]
struct Weights {
    data: Vec<f32>,
    blur: usize,
    defocus: [u32; 2],
    soft: [u32; 2],
    kernel: [u32; 2],
    grain: Option<(GrainParams, Field)>,
    halation: Option<HalationAt>,
}

impl Weights {
    fn new(dev: &Developer, frame: Size) -> Weights {
        let Some(p) = dev.texture else { return Weights::default() };
        let data = texture::blur_weights(frame, &p);
        let mut w = Weights { blur: data.len(), data, ..Weights::default() };
        let at = |w: &Weights| (TABLE_WORDS + w.data.len()) as u32;
        if let Some(h) = texture::halation_of(frame, &p) {
            let blur = |o: &halation::Octave, w: &mut Weights| {
                let kernel = at(w);
                w.data.extend(&o.kernel);
                let down = at(w);
                w.data.extend(&o.down);
                let up = at(w);
                w.data.extend(&o.up);
                Blur { level: o.level, at: kernel, radius: (o.kernel.len() / 2) as u32, down, down_radius: (o.down.len() / 2) as u32, up }
            };
            let octaves = [blur(&h.octaves[0], &mut w), blur(&h.octaves[1], &mut w), blur(&h.octaves[2], &mut w)];
            let mask = h.mask.as_ref().map(|m| blur(m, &mut w));
            let weights = at(&w);
            w.data.extend(h.weights.as_flattened());
            let total = at(&w);
            w.data.extend((0..3).map(|j| h.weights[0][j] + h.weights[1][j] + h.weights[2][j]));
            w.halation = Some(HalationAt { octaves, mask, weights, total, reach: h.reach() });
        }
        let Some(g) = p.grain else { return w };
        let defocus = grain::defocus_weights(frame, &g);
        w.defocus = [at(&w), (defocus.len() / 2) as u32];
        w.data.extend(defocus);
        if g.grains() {
            let field = Field::new(frame, &g);
            w.soft = [at(&w), (field.soft.len() / 2) as u32];
            w.data.extend(&field.soft);
            w.kernel = [at(&w), field.kernel.radius as u32];
            w.data.extend(field.kernel.words());
            w.grain = Some((g, field));
        }
        w
    }
}

fn local(w: usize, h: usize) -> Region {
    Region { x: 0, y: 0, w, h }
}

fn texture_passes(program: &mut Program, dev: &Developer, plan: &Plan, code: &str, tex: &str, taps: &[u32], weights: &Weights) {
    let Some(p) = plan.texture else { return };
    let frame = plan.frame;
    let (region, out) = (plan.region, plan.out);
    let rd = weights.defocus[1] as usize;
    let after = p.grain.is_some() && (rd > 0 || weights.grain.is_some());
    let wide = if after { out.grown(rd, frame) } else { out };
    let detailed = if after { "tmp:detail" } else { tex };
    let n = region.w * region.h;
    let base = Frame { flags: 0, radius: 0, count: 0, src: region, dst: region, bounds: [frame.w as u32, frame.h as u32, 0, 0], taps: [0; 4], aux: [0; 4], scale: 0.0, dev };
    if weights.blur > 0 {
        program.buffer("tmp:blur", n * 3);
        let radius = (weights.blur / 2) as u32;
        program.passes.push(Pass { entry: "blurVertical", params: Frame { radius, ..base }.words(), bind: vec![(1, code.into()), (2, "tmp:blur".into()), (4, "tables".into())], groups: groups(region) });
        program.passes.push(Pass { entry: "blurHorizontal", params: Frame { radius, ..base }.words(), bind: vec![(1, "tmp:blur".into()), (2, code.into()), (4, "tables".into())], groups: groups(region) });
    }
    if !plan.dust.is_empty() {
        let count = plan.dust.len() / 2;
        program.buffer("tmp:dust", plan.dust.len());
        program.writes.push(Write { buffer: "tmp:dust", offset: 0, block: Block::of("dust".into(), plan.dust) });
        program.passes.push(Pass { entry: "dust", params: Frame { count: count as u32, ..base }.words(), bind: vec![(5, "tmp:dust".into()), (2, code.into())], groups: [count.div_ceil(256), 1] });
    }
    program.buffer("tmp:luma", n);
    program.passes.push(Pass { entry: "lumaPlane", params: base.words(), bind: vec![(1, code.into()), (7, "tmp:luma".into())], groups: groups(region) });
    let guide = Guide::new(frame);
    let reach = weights.halation.as_ref().map_or(0, |h| h.reach);
    let hwide = wide.grown(reach, frame);
    let (reduced, _) = guide.reach(hwide);
    let rn = reduced.w * reduced.h * 2;
    for name in ["tmp:moments", "tmp:box", "tmp:coefficients", "tmp:smooth"] {
        program.buffer(name, rn);
    }
    let (dx, dy, ux) = (0u32, guide.down_x.len() as u32, (guide.down_x.len() + guide.down_y.len()) as u32);
    let uy = ux + guide.up_x.len() as u32;
    program.buffer("tmp:taps", taps.len());
    program.writes.push(Write { buffer: "tmp:taps", offset: 0, block: Block::of("taps".into(), taps) });
    let rb = [guide.reduced.w as u32, guide.reduced.h as u32, 0, 0];
    let reduce = Frame { src: region, dst: reduced, taps: [dx, dy, ux, uy], ..base };
    program.passes.push(Pass { entry: "reduce", params: reduce.words(), bind: vec![(6, "tmp:luma".into()), (5, "tmp:taps".into()), (9, "tmp:moments".into())], groups: groups(reduced) });
    let boxed = |flags: u32| Frame { flags, radius: guide.radius as u32, src: reduced, dst: reduced, bounds: rb, ..base }.words();
    let mut box_pass = |entry: &'static str, flags: u32, from: &str, to: &str| {
        program.passes.push(Pass { entry, params: boxed(flags), bind: vec![(8, from.into()), (9, to.into()), (4, "tables".into())], groups: groups(reduced) });
    };
    box_pass("boxVertical", 0, "tmp:moments", "tmp:box");
    box_pass("boxHorizontal", COEFFICIENTS, "tmp:box", "tmp:coefficients");
    box_pass("boxVertical", 0, "tmp:coefficients", "tmp:box");
    box_pass("boxHorizontal", 0, "tmp:box", "tmp:smooth");
    if after {
        program.buffer(detailed, wide.w * wide.h * 3);
    }
    let target = if weights.halation.is_some() { "tmp:cand" } else { detailed };
    if weights.halation.is_some() {
        program.buffer(target, hwide.w * hwide.h * 3);
    }
    let flags = if weights.halation.is_some() { CANDIDATE } else { 0 };
    let detail = Frame { flags, src: region, dst: hwide, taps: [dx, dy, ux, uy], aux: region_words(reduced), ..base };
    program.passes.push(Pass {
        entry: "detail",
        params: detail.words(),
        bind: vec![(1, code.into()), (6, "tmp:luma".into()), (8, "tmp:smooth".into()), (5, "tmp:taps".into()), (4, "tables".into()), (2, target.into())],
        groups: groups(hwide),
    });
    halation_passes(program, base, weights, frame, wide, hwide, detailed);
    if !after {
        return;
    }
    let focused = if rd == 0 { detailed } else { defocus_passes(program, base, weights, wide, out, tex) };
    grain_passes(program, base, weights, frame, out, focused, tex);
}

type Grid = (Span, Span);

fn grid_words((x, y): Grid) -> [u32; 4] {
    [x.lo as u32, y.lo as u32, x.len() as u32, y.len() as u32]
}

fn grid_groups((x, y): Grid) -> [usize; 2] {
    [x.len().div_ceil(GROUP), y.len().div_ceil(GROUP)]
}

struct Planner<'a> {
    program: &'a mut Program,
    base: Frame<'a>,
    next: usize,
}

impl Planner<'_> {
    fn name(&mut self) -> String {
        self.next += 1;
        format!("tmp:hp{}", self.next)
    }

    // why: one axis of the decimation, the level blur or the interpolation, with the spans halation::apply gives the
    // same step; `scale` is the octave's decimation factor
    #[allow(clippy::too_many_arguments)]
    fn axis(&mut self, entry: &'static str, from: &(String, Grid), to: Grid, vertical: bool, at: u32, radius: u32, scale: usize) -> (String, Grid) {
        let name = self.name();
        self.program.buffer(&name, to.0.len() * to.1.len() * 3);
        let flags = if vertical { VERTICAL } else { 0 };
        let src = from.1;
        let frame = Frame { flags, radius, count: scale as u32, src: region_of(src), dst: region_of(to), bounds: [src.0.n as u32, src.1.n as u32, 0, 0], aux: [0, 0, at, 0], ..self.base };
        self.program.passes.push(Pass { entry, params: frame.words(), bind: vec![(1, from.0.clone()), (2, name.clone()), (4, "tables".into())], groups: grid_groups(to) });
        (name, to)
    }

    fn reduce(&mut self, from: &(String, Grid), b: &Blur) -> (String, Grid) {
        let (s, r) = (1 << b.level, b.down_radius as usize);
        let across = self.axis("pyramidReduce", from, (from.1.0.down(s, r), from.1.1), false, b.down, b.down_radius, s);
        self.axis("pyramidReduce", &across, (across.1.0, across.1.1.down(s, r)), true, b.down, b.down_radius, s)
    }

    fn blur(&mut self, from: &(String, Grid), b: &Blur) -> (String, Grid) {
        let r = b.radius as usize;
        let across = self.axis("pyramidBlur", from, (from.1.0.blur(r), from.1.1), false, b.at, b.radius, 1);
        self.axis("pyramidBlur", &across, (across.1.0, across.1.1.blur(r)), true, b.at, b.radius, 1)
    }

    fn expand(&mut self, from: &(String, Grid), b: &Blur, frame: Size) -> (String, Grid) {
        let s = 1 << b.level;
        let across = self.axis("pyramidExpand", from, (from.1.0.up(s, frame.w), from.1.1), false, b.up, 0, s);
        self.axis("pyramidExpand", &across, (across.1.0, across.1.1.up(s, frame.h)), true, b.up, 0, s)
    }

    // why: the same steps as halation::Octave::blurred
    fn octave(&mut self, from: &(String, Grid), b: &Blur, frame: Size) -> (String, Grid) {
        if b.level == 0 {
            return self.blur(from, b);
        }
        let level = self.reduce(from, b);
        let level = self.blur(&level, b);
        self.expand(&level, b, frame)
    }

    fn add(&mut self, blurred: &(String, Grid), acc: Option<&(String, Grid)>, weights: u32) -> (String, Grid) {
        let to = match acc {
            Some(a) => (blurred.1.0.meet(a.1.0), blurred.1.1.meet(a.1.1)),
            None => blurred.1,
        };
        let name = self.name();
        self.program.buffer(&name, to.0.len() * to.1.len() * 3);
        let flags = if acc.is_some() { ACCUMULATE } else { 0 };
        let taps = acc.map_or([0; 4], |a| grid_words(a.1));
        let frame = Frame { flags, src: region_of(blurred.1), dst: region_of(to), taps, aux: [0, 0, 0, weights], ..self.base };
        let mut bind = vec![(1, blurred.0.clone()), (2, name.clone()), (4, "tables".to_string())];
        if let Some(a) = acc {
            bind.push((6, a.0.clone()));
        }
        self.program.passes.push(Pass { entry: "pyramidAdd", params: frame.words(), bind, groups: grid_groups(to) });
        (name, to)
    }
}

fn region_of((x, y): Grid) -> Region {
    Region { x: x.lo, y: y.lo, w: x.len(), h: y.len() }
}

// why: the same steps, spans and summation order as halation::apply, so the GPU's halation matches the CPU's bit
// for bit
fn halation_passes(program: &mut Program, base: Frame, weights: &Weights, frame: Size, wide: Region, hwide: Region, to: &str) {
    let Some(h) = &weights.halation else { return };
    let highlights = h.mask.is_some();
    let flags = if highlights { HIGHLIGHTS } else { 0 };
    let span = |lo: usize, len: usize, n: usize| Span { lo, hi: lo + len, n };
    let start: Grid = (span(hwide.x, hwide.w, frame.w), span(hwide.y, hwide.h, frame.h));
    program.buffer("tmp:hu", hwide.w * hwide.h * 3);
    let mut layers = vec![(1, "tmp:cand".to_string()), (2, "tmp:hu".to_string()), (4, "tables".to_string())];
    if highlights {
        program.buffer("tmp:hm", hwide.w * hwide.h * 3);
        layers.push((7, "tmp:hm".into()));
    }
    program.passes.push(Pass { entry: "halationLayers", params: Frame { flags, src: hwide, dst: hwide, ..base }.words(), bind: layers, groups: groups(hwide) });
    let mut plan = Planner { program, base, next: 0 };
    let mut sum: Option<(String, Grid)> = None;
    for (k, o) in h.octaves.iter().enumerate() {
        let blurred = plan.octave(&("tmp:hu".to_string(), start), o, frame);
        sum = Some(plan.add(&blurred, sum.as_ref(), h.weights + 3 * k as u32));
    }
    let Some(sum) = sum else { return };
    let mask = h.mask.as_ref().map(|o| plan.octave(&("tmp:hm".to_string(), start), o, frame));
    let combine = Frame {
        flags,
        src: hwide,
        dst: wide,
        aux: grid_words(sum.1),
        taps: mask.as_ref().map_or([0; 4], |m| grid_words(m.1)),
        count: h.total,
        ..base
    };
    let mut bind = vec![(1, sum.0.clone()), (6, "tmp:hu".to_string()), (2, to.to_string()), (4, "tables".to_string())];
    if let Some(m) = &mask {
        bind.push((5, m.0.clone()));
    }
    plan.program.passes.push(Pass { entry: "pyramidCombine", params: combine.words(), bind, groups: groups(wide) });
}

fn defocus_passes<'a>(program: &mut Program, base: Frame, weights: &Weights, wide: Region, out: Region, tex: &'a str) -> &'a str {
    let band = Region { x: wide.x, y: out.y, w: wide.w, h: out.h };
    program.buffer("tmp:band", band.w * band.h * 3);
    let target = if weights.grain.is_some() { "tmp:focus" } else { tex };
    if weights.grain.is_some() {
        program.buffer(target, out.w * out.h * 3);
    }
    let [at, radius] = weights.defocus;
    let aux = [0, 0, at, 0];
    let vertical = Frame { src: wide, dst: band, radius, aux, ..base };
    program.passes.push(Pass { entry: "defocusVertical", params: vertical.words(), bind: vec![(1, "tmp:detail".into()), (2, "tmp:band".into()), (4, "tables".into())], groups: groups(band) });
    let horizontal = Frame { src: band, dst: out, radius, aux, ..base };
    program.passes.push(Pass { entry: "defocusHorizontal", params: horizontal.words(), bind: vec![(1, "tmp:band".into()), (2, target.into()), (4, "tables".into())], groups: groups(out) });
    target
}

fn grain_passes(program: &mut Program, base: Frame, weights: &Weights, frame: Size, out: Region, code: &str, tex: &str) {
    let Some((g, field)) = &weights.grain else { return };
    let (m, r, rs) = (field.margin, field.kernel.radius as u32, field.soft.len() / 2);
    let (nw, nh, vh, zw) = (out.w + 2 * m, out.h + 2 * m, out.h + 2 * rs, out.w + 2 * rs);
    let [ox, oy] = field.origin(frame, out);
    let kernel = [0, 0, weights.kernel[0], 0];
    let soft = [0, 0, weights.soft[0], 0];
    for (name, words) in [("tmp:noise", nw * nh), ("tmp:columns", nw * vh * 3), ("tmp:field", zw * vh), ("tmp:soft", zw * out.h)] {
        program.buffer(name, words);
    }
    let mut pass = |entry: &'static str, frame: Frame, bind: Vec<(u32, String)>| {
        let groups = groups(frame.dst);
        program.passes.push(Pass { entry, params: frame.words(), bind, groups });
    };
    let t = || (4, "tables".to_string());
    pass("grainNoise", Frame { dst: local(nw, nh), count: g.seed, aux: [ox as u32, oy as u32, 0, 0], ..base }, vec![(7, "tmp:noise".into()), t()]);
    pass("grainVertical", Frame { src: local(nw, nh), dst: local(nw, vh), radius: r, aux: kernel, ..base }, vec![(6, "tmp:noise".into()), (2, "tmp:columns".into()), t()]);
    pass("grainHorizontal", Frame { src: local(nw, vh), dst: local(zw, vh), radius: r, aux: kernel, ..base }, vec![(1, "tmp:columns".into()), (7, "tmp:field".into()), t()]);
    let smooth = if rs > 0 { "tmp:soft" } else { "tmp:field" };
    if rs > 0 {
        pass("softVertical", Frame { src: local(zw, vh), dst: local(zw, out.h), radius: rs as u32, aux: soft, ..base }, vec![(6, "tmp:field".into()), (7, "tmp:soft".into()), t()]);
    }
    let apply = Frame { src: local(zw, out.h), dst: local(out.w, out.h), radius: rs as u32, aux: soft, scale: g.scale(), ..base };
    pass("grainApply", apply, vec![(6, smooth.into()), (1, code.into()), (2, tex.into()), t()]);
}

fn region_words(r: Region) -> [u32; 4] {
    region(r)
}

fn sample_pass(program: &mut Program, dev: &Developer, tex: &str, luts: &str, textured: Region, out: &mut [f32]) {
    let len = textured.w * textured.h;
    let step = (len / SAMPLES).max(1);
    let count = len.div_ceil(step);
    program.buffer("tmp:samples", count);
    let frame = Frame { flags: 0, radius: 0, count: count as u32, src: textured, dst: textured, bounds: [0; 4], taps: [0; 4], aux: [step as u32, 0, 0, 0], scale: 0.0, dev };
    program.passes.push(Pass {
        entry: "samples",
        params: frame.words(),
        bind: vec![(1, tex.into()), (3, luts.into()), (7, "tmp:samples".into())],
        groups: [count.div_ceil(256), 1],
    });
    program.reads.push(Read { buffer: "tmp:samples".into(), at: out.as_mut_ptr() as usize, len: count });
}

fn sample_count(r: Region) -> usize {
    let len = r.w * r.h;
    len.div_ceil((len / SAMPLES).max(1))
}

pub fn textured(dev: &Developer, scene: &Scene, out: Region, lumas: bool) -> Option<(Textured, Option<Vec<f32>>)> {
    let region = scene.region;
    if region.w * region.h == 0 || scene.rgb.len() != region.w * region.h {
        return None;
    }
    let dust: Vec<u32> = dev.texture.and_then(|p| p.dust).map_or_else(Vec::new, |d| dust::strength(scene.frame, region, &d).into_iter().flat_map(|(k, s)| [k as u32, s.to_bits()]).collect());
    let plan = Plan { frame: scene.frame, region, out, texture: dev.texture, dust: &dust };
    let mut program = Program::default();
    let weights = Weights::new(dev, scene.frame);
    tables(&mut program, dev, None, &weights.data);
    let luts = luts(&mut program, dev);
    let scene_buffer = program.upload(scene_name(scene), &scene.rgb, false);
    let id = next_id();
    let tex = format!("tex:{id}");
    let code = if dev.texture.is_some() { "tmp:code".to_string() } else { tex.clone() };
    program.buffer(&code, region.w * region.h * 3);
    if dev.texture.is_some() {
        program.buffer(&tex, out.w * out.h * 3);
    }
    let mut flags = 0;
    if dev.light.contrast != 0.0 {
        flags |= CONTRAST;
    }
    if dev.chain.utility.is_some() {
        flags |= UTILITY;
    }
    let base = Frame { flags, radius: 0, count: 0, src: region, dst: region, bounds: [scene.frame.w as u32, scene.frame.h as u32, 0, 0], taps: [0; 4], aux: [0; 4], scale: 0.0, dev };
    let bind = vec![(1, scene_buffer), (2, code.clone()), (3, luts.clone()), (4, "tables".into())];
    program.passes.push(Pass { entry: "code", params: base.words(), bind, groups: groups(region) });
    let taps: Vec<u32> = match dev.texture {
        Some(_) => {
            let g = Guide::new(scene.frame);
            [&g.down_x, &g.down_y, &g.up_x, &g.up_y].into_iter().flat_map(|t| tap_words(t).collect::<Vec<_>>()).collect()
        }
        None => Vec::new(),
    };
    texture_passes(&mut program, dev, &plan, &code, &tex, &taps, &weights);
    let mut samples = vec![0.0f32; if lumas { sample_count(out) } else { 0 }];
    if lumas {
        sample_pass(&mut program, dev, &tex, &luts, out, &mut samples);
    }
    program.run().then(|| (Textured { id, region: out, rgb: Vec::new(), gpu: true, lumas: None }, lumas.then_some(samples)))
}

pub fn lumas(dev: &Developer, textured: &Textured) -> Option<Vec<f32>> {
    if !textured.gpu {
        return None;
    }
    let mut program = Program::default();
    tables(&mut program, dev, None, &[]);
    let luts = luts(&mut program, dev);
    let mut samples = vec![0.0f32; sample_count(textured.region)];
    sample_pass(&mut program, dev, &format!("tex:{}", textured.id), &luts, textured.region, &mut samples);
    program.run().then_some(samples)
}

pub fn finish(dev: &Developer, textured: &Textured, scan: &Scan) -> Option<Toned> {
    if !textured.gpu {
        return None;
    }
    let r = textured.region;
    let mut program = Program::default();
    tables(&mut program, dev, Some(scan), &[]);
    let luts = luts(&mut program, dev);
    program.buffer("tmp:toned", r.w * r.h * 3);
    let frame = Frame { flags: 0, radius: 0, count: 0, src: r, dst: r, bounds: [0; 4], taps: [0; 4], aux: [0; 4], scale: 0.0, dev };
    program.passes.push(Pass {
        entry: "finish",
        params: frame.words(),
        bind: vec![(1, format!("tex:{}", textured.id)), (3, luts), (4, "tables".into()), (2, "tmp:toned".into())],
        groups: groups(r),
    });
    let mut rgb = vec![[0.0f32; 3]; r.w * r.h];
    program.reads.push(Read { buffer: "tmp:toned".into(), at: rgb.as_mut_ptr() as usize, len: r.w * r.h * 3 });
    program.run().then_some(Toned { region: r, rgb })
}

#[cfg(target_arch = "wasm32")]
mod bridge {
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen]
    extern "C" {
        #[wasm_bindgen(catch, js_namespace = globalThis, js_name = filmlabGpuRun)]
        fn program(memory: &JsValue, program: &str) -> Result<u32, JsValue>;

        #[wasm_bindgen(catch, js_namespace = globalThis, js_name = filmlabGpuUpload)]
        fn prefetch(memory: &JsValue, block: &str) -> Result<(), JsValue>;
    }

    pub fn run(json: &str) -> bool {
        program(&wasm_bindgen::memory(), json).is_ok_and(|done| done == 1)
    }

    pub fn upload(block: &str) {
        let _ = prefetch(&wasm_bindgen::memory(), block);
    }

    pub fn forget(_name: &str) {}
}

#[cfg(not(target_arch = "wasm32"))]
pub use bridge::{Runner, install};

#[cfg(not(target_arch = "wasm32"))]
mod bridge {
    use std::sync::OnceLock;

    // why: programs name engine memory by address and length, as in the browser; a native runner reads
    // those addresses during the call, while the engine keeps the blocks alive
    pub trait Runner: Send + Sync {
        fn run(&self, program: &str) -> bool;
        fn upload(&self, block: &str);
        fn forget(&self, _name: &str) {}
    }

    static RUNNER: OnceLock<Box<dyn Runner>> = OnceLock::new();

    pub fn install(runner: Box<dyn Runner>) -> bool {
        RUNNER.set(runner).is_ok()
    }

    pub fn run(json: &str) -> bool {
        RUNNER.get().is_some_and(|r| r.run(json))
    }

    pub fn upload(block: &str) {
        if let Some(r) = RUNNER.get() {
            r.upload(block);
        }
    }

    pub fn forget(name: &str) {
        if let Some(r) = RUNNER.get() {
            r.forget(name);
        }
    }
}
