use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde::Deserialize;
use wgpu::util::DeviceExt;

const SHADER: &str = include_str!("../../src/lib/engine/film.wgsl");
const PARAMS_SLOT: u64 = 512;
const SPARE: usize = 4;
// uploads smaller than this (the tables, dust and taps) go through queue.write_buffer
const STAGED: usize = 1 << 20;
const STORAGE: [u32; 9] = [1, 2, 3, 4, 5, 6, 7, 8, 9];
const WRITABLE: [u32; 3] = [2, 7, 9];
const ENTRIES: [&str; 23] = [
    "code",
    "blurVertical",
    "blurHorizontal",
    "lumaPlane",
    "reduce",
    "boxVertical",
    "boxHorizontal",
    "detail",
    "defocusVertical",
    "defocusHorizontal",
    "grainNoise",
    "grainVertical",
    "grainHorizontal",
    "softVertical",
    "grainApply",
    "dust",
    "samples",
    "finish",
    "halationLayers",
    "pyramidReduce",
    "pyramidBlur",
    "pyramidExpand",
    "pyramidCombine",
];

#[derive(Deserialize)]
struct Block {
    id: String,
    at: usize,
    len: usize,
}

#[derive(Deserialize)]
struct BufferSpec {
    name: String,
    words: usize,
    upload: Option<Block>,
    pack: bool,
}

#[derive(Deserialize)]
struct Write {
    buffer: String,
    offset: usize,
    block: Block,
}

#[derive(Deserialize)]
struct Pass {
    entry: String,
    params: Vec<u32>,
    bind: Vec<(u32, String)>,
    groups: [usize; 2],
}

#[derive(Deserialize)]
struct Read {
    buffer: String,
    at: usize,
    len: usize,
}

#[derive(Deserialize)]
struct Copy {
    from: String,
    to: String,
    offset: usize,
}

#[derive(Deserialize)]
struct Keep {
    buffer: String,
    id: String,
    need: bool,
}

#[derive(Deserialize)]
struct Program {
    buffers: Vec<BufferSpec>,
    copies: Vec<Copy>,
    writes: Vec<Write>,
    passes: Vec<Pass>,
    reads: Vec<Read>,
    #[serde(default)]
    keep: Vec<Keep>,
}

struct Stored {
    buffer: wgpu::Buffer,
    words: usize,
    holds: Option<String>,
}

struct Upload {
    buffer: wgpu::Buffer,
    after: wgpu::SubmissionIndex,
}

#[derive(Default)]
struct Store {
    order: Vec<String>,
    buffers: HashMap<String, Stored>,
    // why: a frame-sized buffer freed by one render (an edit's old scene and texture, an export band's) is the size
    // the next one asks for; reusing it skips Metal's allocation and wgpu's zero fill. trim and reset drop them
    spare: Vec<wgpu::Buffer>,
    params: Option<(wgpu::Buffer, u64)>,
    staging: Option<(wgpu::Buffer, u64)>,
    upload: Option<Upload>,
}

impl Store {
    fn free(&mut self, buffer: wgpu::Buffer) {
        self.spare.push(buffer);
        if self.spare.len() > SPARE {
            self.spare.remove(0).destroy();
        }
    }

    fn reuse(&mut self, bytes: u64) -> Option<wgpu::Buffer> {
        let fits = |b: &wgpu::Buffer| b.size() >= bytes && b.size() <= bytes + bytes / 2;
        let at = self.spare.iter().enumerate().filter(|(_, b)| fits(b)).min_by_key(|(_, b)| b.size()).map(|(k, _)| k)?;
        Some(self.spare.remove(at))
    }
}

// why: `keep:` images (a slider's image from before its own stage, re-run as the slider moves) live while one is
// dragged and go with trim once work settles; two can be kept, so one slider's doesn't evict another's
fn kept(kind: &str) -> Option<usize> {
    match kind {
        "scene" => Some(3),
        "lut" => Some(8),
        "tex" => Some(4),
        "keep" => Some(2),
        _ => None,
    }
}

pub struct Gpu {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    layout: wgpu::BindGroupLayout,
    pipelines: HashMap<&'static str, wgpu::ComputePipeline>,
    dummies: Vec<(u32, wgpu::Buffer)>,
    store: Mutex<Store>,
    mappable: bool,
}

// why: engine programs name its memory by address; the engine keeps every named block alive for the
// duration of the call that hands us the program
fn memory(at: usize, len: usize) -> &'static [u32] {
    if len == 0 {
        return &[];
    }
    unsafe { std::slice::from_raw_parts(at as *const u32, len) }
}

fn packed(block: &Block) -> Vec<u32> {
    memory(block.at, block.len).chunks(3).flat_map(|c| [c[0], c.get(1).copied().unwrap_or(0), c.get(2).copied().unwrap_or(0), 0]).collect()
}

impl Gpu {
    pub fn open() -> Option<Arc<Gpu>> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor { backends: wgpu::Backends::METAL, ..wgpu::InstanceDescriptor::new_without_display_handle() });
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions { power_preference: wgpu::PowerPreference::HighPerformance, ..Default::default() })).ok()?;
        let limits = adapter.limits();
        let required = wgpu::Limits {
            max_buffer_size: limits.max_buffer_size,
            max_storage_buffer_binding_size: limits.max_storage_buffer_binding_size,
            max_storage_buffers_per_shader_stage: STORAGE.len() as u32,
            ..wgpu::Limits::default()
        };
        // why: with unified memory a storage buffer the CPU can map is read back where the shader wrote it, without a
        // copy to a staging buffer first; a discrete GPU would reach such a buffer over the bus
        let unified = adapter.get_info().device_type == wgpu::DeviceType::IntegratedGpu;
        let features = if unified { adapter.features() & wgpu::Features::MAPPABLE_PRIMARY_BUFFERS } else { wgpu::Features::empty() };
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor { label: Some("filmlab"), required_limits: required, required_features: features, ..Default::default() })).ok()?;
        let entry = |binding: u32, ty: wgpu::BufferBindingType| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer { ty, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        };
        let mut entries = vec![entry(0, wgpu::BufferBindingType::Uniform)];
        entries.extend(STORAGE.iter().map(|&b| entry(b, wgpu::BufferBindingType::Storage { read_only: !WRITABLE.contains(&b) })));
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: None, entries: &entries });
        // why: the film shader is our own and only indexes inside its buffers, so wgpu's per-access clamps and
        // loop guards are dropped; the CPU parity check would show any read outside them
        let checks = wgpu::ShaderRuntimeChecks { bounds_checks: false, force_loop_bounding: false, ..wgpu::ShaderRuntimeChecks::default() };
        let module = unsafe { device.create_shader_module_trusted(wgpu::ShaderModuleDescriptor { label: Some("film"), source: wgpu::ShaderSource::Wgsl(shader().into()) }, checks) };
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        let pipelines = ENTRIES
            .iter()
            .map(|&name| {
                let p = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some(name),
                    layout: Some(&pipeline_layout),
                    module: &module,
                    entry_point: Some(name),
                    compilation_options: wgpu::PipelineCompilationOptions { zero_initialize_workgroup_memory: false, ..Default::default() },
                    cache: None,
                });
                (name, p)
            })
            .collect();
        let dummies = STORAGE
            .iter()
            .map(|&b| (b, device.create_buffer(&wgpu::BufferDescriptor { label: None, size: 16, usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false })))
            .collect();
        let mappable = features.contains(wgpu::Features::MAPPABLE_PRIMARY_BUFFERS);
        Some(Arc::new(Gpu { instance, adapter, device, queue, layout, pipelines, dummies, store: Mutex::default(), mappable }))
    }

    fn ensure(&self, store: &mut Store, name: &str, size: usize) {
        if store.buffers.get(name).is_some_and(|s| s.words >= size) {
            store.order.retain(|k| k != name);
            store.order.push(name.to_string());
            return;
        }
        if let Some(old) = store.buffers.remove(name) {
            store.free(old.buffer);
        }
        store.order.retain(|k| k != name);
        let words = size.max(4);
        let buffer = store.reuse((words * 4) as u64).unwrap_or_else(|| {
            self.device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: (words * 4) as u64,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST | if self.mappable { wgpu::BufferUsages::MAP_READ } else { wgpu::BufferUsages::empty() },
                mapped_at_creation: false,
            })
        });
        store.buffers.insert(name.to_string(), Stored { buffer, words, holds: None });
        store.order.push(name.to_string());
        let kind = name.split(':').next().unwrap_or("");
        let Some(limit) = kept(kind) else { return };
        let same: Vec<String> = store.order.iter().filter(|k| k.starts_with(&format!("{kind}:"))).cloned().collect();
        for key in same.iter().take(same.len().saturating_sub(limit)) {
            if let Some(s) = store.buffers.remove(key) {
                store.free(s.buffer);
            }
            store.order.retain(|k| k != key);
        }
    }

    fn upload_into(&self, store: &mut Store, name: &str, block: &Block, pack: bool) {
        self.ensure(store, name, if pack { block.len / 3 * 4 } else { block.len });
        if store.buffers.get(name).is_none_or(|t| t.holds.as_deref() == Some(block.id.as_str())) {
            return;
        }
        let data = if pack { std::borrow::Cow::Owned(packed(block)) } else { std::borrow::Cow::Borrowed(memory(block.at, block.len)) };
        let bytes: &[u8] = bytemuck::cast_slice(&data);
        if bytes.len() < STAGED || !self.stage(store, name, bytes) {
            let Some(target) = store.buffers.get(name) else { return };
            self.queue.write_buffer(&target.buffer, 0, bytes);
        }
        if let Some(target) = store.buffers.get_mut(name) {
            target.holds = Some(block.id.clone());
        }
    }

    // why: queue.write_buffer makes a staging buffer per call, and a frame's worth of fresh pages costs more than the
    // copy; one kept staging buffer is mapped again once its last copy is done, which an upload never waits on, and a
    // new one is zeroed by the GPU (its first copy) rather than by wgpu on the CPU
    fn stage(&self, store: &mut Store, name: &str, bytes: &[u8]) -> bool {
        let size = bytes.len() as u64;
        let Some(target) = store.buffers.get(name).map(|t| t.buffer.clone()) else { return false };
        if store.upload.as_ref().is_none_or(|u| u.buffer.size() < size) {
            if let Some(old) = store.upload.take() {
                old.buffer.destroy();
            }
            let room = (size + size / 3 + 3) & !3;
            let buffer = self.device.create_buffer(&wgpu::BufferDescriptor { label: None, size: room, usage: wgpu::BufferUsages::MAP_WRITE | wgpu::BufferUsages::COPY_SRC, mapped_at_creation: false });
            let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            encoder.copy_buffer_to_buffer(&buffer, 0, &target, 0, Some(size));
            let after = self.queue.submit([encoder.finish()]);
            store.upload = Some(Upload { buffer, after });
        }
        let Some(upload) = store.upload.as_mut() else { return false };
        let (tx, rx) = std::sync::mpsc::channel();
        upload.buffer.slice(..size).map_async(wgpu::MapMode::Write, move |r| {
            let _ = tx.send(r);
        });
        let _ = self.device.poll(wgpu::PollType::Wait { submission_index: Some(upload.after.clone()), timeout: None });
        if !rx.try_recv().is_ok_and(|r| r.is_ok()) {
            return false;
        }
        match upload.buffer.slice(..size).get_mapped_range_mut() {
            Ok(mut view) => view.copy_from_slice(bytes),
            Err(_) => return false,
        }
        upload.buffer.unmap();
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        encoder.copy_buffer_to_buffer(&upload.buffer, 0, &target, 0, Some(size));
        upload.after = self.queue.submit([encoder.finish()]);
        true
    }

    fn execute(&self, program: &Program) -> Option<()> {
        let started = std::time::Instant::now();
        let traced = self.execute_inner(program);
        if std::env::var_os("FILMLAB_GPU_TRACE").is_some() {
            let entries: Vec<&str> = program.passes.iter().map(|p| p.entry.as_str()).collect();
            let mb = |f: &dyn Fn(&str) -> bool| program.buffers.iter().filter(|b| f(&b.name)).map(|b| b.words * 4).sum::<usize>() as f64 / 1_048_576.0;
            let failed = if traced.is_none() { " failed" } else { "" };
            eprintln!("gpu {:.1} ms {} passes {:?}{failed} scratch {:.0} MB in {} slots, other {:.0} MB", started.elapsed().as_secs_f64() * 1000.0, entries.len(), entries.first(), mb(&|n| n.starts_with("tmp:")), program.buffers.iter().filter(|b| b.name.starts_with("tmp:slot")).count(), mb(&|n| !n.starts_with("tmp:")));
            for b in program.buffers.iter().filter(|b| b.words > 1_000_000) {
                eprintln!("    {} {:.0} MB", b.name, (b.words * 4) as f64 / 1_048_576.0);
            }
        }
        traced
    }

    fn execute_inner(&self, program: &Program) -> Option<()> {
        let mut store = self.store.lock().ok()?;
        let store = &mut *store;
        if program.keep.iter().any(|k| k.need && store.buffers.get(&k.buffer).and_then(|b| b.holds.as_deref()) != Some(k.id.as_str())) {
            return None;
        }
        let scope = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        for spec in &program.buffers {
            match &spec.upload {
                Some(block) => self.upload_into(store, &spec.name, block, spec.pack),
                None => self.ensure(store, &spec.name, spec.words),
            }
        }
        for w in &program.writes {
            let target = store.buffers.get_mut(&w.buffer)?;
            target.holds = None;
            self.queue.write_buffer(&target.buffer, (w.offset * 4) as u64, bytemuck::cast_slice(memory(w.block.at, w.block.len)));
        }
        let bytes = (program.passes.len().max(1) as u64) * PARAMS_SLOT;
        if store.params.as_ref().is_none_or(|(_, size)| *size < bytes) {
            let buffer = self.device.create_buffer(&wgpu::BufferDescriptor { label: None, size: bytes, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
            store.params = Some((buffer, bytes));
        }
        let params = store.params.as_ref()?.0.clone();
        for (k, pass) in program.passes.iter().enumerate() {
            self.queue.write_buffer(&params, k as u64 * PARAMS_SLOT, bytemuck::cast_slice(&pass.params));
        }
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        // why: a copy target (the frame's LUTs side by side) that already holds these sources at these offsets is left as it is
        let mut targets: Vec<&str> = program.copies.iter().map(|c| c.to.as_str()).collect();
        targets.dedup();
        for to in targets {
            let copies = program.copies.iter().filter(|c| c.to == to);
            let sources: Option<Vec<String>> = copies.clone().map(|c| store.buffers.get(&c.from).and_then(|f| f.holds.as_ref()).map(|h| format!("{h}@{}", c.offset))).collect();
            let holds = sources.map(|s| s.join(","));
            if holds.is_some() && store.buffers.get(to)?.holds == holds {
                continue;
            }
            for c in copies {
                let from = store.buffers.get(&c.from)?;
                encoder.copy_buffer_to_buffer(&from.buffer, 0, &store.buffers.get(to)?.buffer, (c.offset * 4) as u64, Some((from.words * 4) as u64));
            }
            store.buffers.get_mut(to)?.holds = holds;
        }
        for (_, name) in program.passes.iter().flat_map(|p| &p.bind).filter(|(b, _)| WRITABLE.contains(b)) {
            store.buffers.get_mut(name)?.holds = None;
        }
        let binds = program
            .passes
            .iter()
            .enumerate()
            .map(|(k, pass)| {
                let mut entries = vec![wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: &params, offset: k as u64 * PARAMS_SLOT, size: wgpu::BufferSize::new(PARAMS_SLOT) }),
                }];
                for (binding, dummy) in &self.dummies {
                    let buffer = match pass.bind.iter().find(|(b, _)| b == binding) {
                        Some((_, name)) => &store.buffers.get(name)?.buffer,
                        None => dummy,
                    };
                    entries.push(wgpu::BindGroupEntry { binding: *binding, resource: buffer.as_entire_binding() });
                }
                Some(self.device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &self.layout, entries: &entries }))
            })
            .collect::<Option<Vec<_>>>()?;
        let profile = std::env::var_os("FILMLAB_GPU_PROFILE").is_some();
        // why: one compute pass holds every dispatch (a serial Metal encoder orders them), instead of an encoder each
        let mut compute = (!profile).then(|| encoder.begin_compute_pass(&wgpu::ComputePassDescriptor::default()).forget_lifetime());
        for (pass, bind) in program.passes.iter().zip(&binds) {
            let pipeline = self.pipelines.get(pass.entry.as_str())?;
            let groups = (pass.groups[0].max(1) as u32, pass.groups[1].max(1) as u32);
            if let Some(compute) = compute.as_mut() {
                compute.set_pipeline(pipeline);
                compute.set_bind_group(0, bind, &[]);
                compute.dispatch_workgroups(groups.0, groups.1, 1);
                continue;
            }
            {
                let mut compute = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor::default());
                compute.set_pipeline(pipeline);
                compute.set_bind_group(0, bind, &[]);
                compute.dispatch_workgroups(groups.0, groups.1, 1);
            }
            let t = std::time::Instant::now();
            self.queue.submit([std::mem::replace(&mut encoder, self.device.create_command_encoder(&Default::default())).finish()]);
            let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
            eprintln!("  {:>18} {:>5}x{:<5} {:.2} ms", pass.entry, pass.groups[0], pass.groups[1], t.elapsed().as_secs_f64() * 1000.0);
        }
        drop(compute);
        let total: u64 = program.reads.iter().map(|r| (r.len * 4) as u64).sum();
        if self.mappable {
            self.queue.submit([encoder.finish()]);
            if pollster::block_on(scope.pop()).is_some() {
                return None;
            }
            mark(store, program);
            let reads: Vec<(&Read, &wgpu::Buffer)> = program.reads.iter().filter(|r| r.len > 0).map(|r| store.buffers.get(&r.buffer).map(|b| (r, &b.buffer))).collect::<Option<_>>()?;
            let mapped: Vec<_> = reads
                .iter()
                .map(|(r, b)| {
                    let (tx, rx) = std::sync::mpsc::channel();
                    b.slice(..(r.len * 4) as u64).map_async(wgpu::MapMode::Read, move |m| {
                        let _ = tx.send(m);
                    });
                    rx
                })
                .collect();
            if !reads.is_empty() {
                self.device.poll(wgpu::PollType::wait_indefinitely()).ok()?;
            }
            for ((r, b), rx) in reads.iter().zip(mapped) {
                rx.recv().ok()?.ok()?;
                {
                    let view = b.slice(..(r.len * 4) as u64).get_mapped_range().ok()?;
                    let dst = unsafe { std::slice::from_raw_parts_mut(r.at as *mut u8, r.len * 4) };
                    dst.copy_from_slice(&view);
                }
                b.unmap();
            }
            return Some(());
        }
        if total > 0 && store.staging.as_ref().is_none_or(|(_, size)| *size < total) {
            let buffer = self.device.create_buffer(&wgpu::BufferDescriptor { label: None, size: total, usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
            store.staging = Some((buffer, total));
        }
        let mut offset = 0u64;
        for r in &program.reads {
            if let Some((out, _)) = &store.staging {
                encoder.copy_buffer_to_buffer(&store.buffers.get(&r.buffer)?.buffer, 0, out, offset, Some((r.len * 4) as u64));
            }
            offset += (r.len * 4) as u64;
        }
        self.queue.submit([encoder.finish()]);
        if pollster::block_on(scope.pop()).is_some() {
            return None;
        }
        mark(store, program);
        if total == 0 {
            return Some(());
        }
        let (out, _) = store.staging.as_ref()?;
        let slice = out.slice(..total);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        self.device.poll(wgpu::PollType::wait_indefinitely()).ok()?;
        rx.recv().ok()?.ok()?;
        {
            let mapped = slice.get_mapped_range().ok()?;
            let mut at = 0usize;
            for r in &program.reads {
                let bytes = r.len * 4;
                let dst = unsafe { std::slice::from_raw_parts_mut(r.at as *mut u8, bytes) };
                dst.copy_from_slice(&mapped[at..at + bytes]);
                at += bytes;
            }
        }
        out.unmap();
        Some(())
    }
}

fn mark(store: &mut Store, program: &Program) {
    for k in program.keep.iter().filter(|k| !k.need) {
        if let Some(b) = store.buffers.get_mut(&k.buffer) {
            b.holds = Some(k.id.clone());
        }
    }
}

// why: naga can't evaluate bitcast in a const expression, which the browser's compiler can; each one is a
// finite f32 written by its bits, so it becomes a decimal literal that parses back to the same bits
fn shader() -> String {
    const OPEN: &str = "bitcast<f32>(0x";
    let mut out = String::with_capacity(SHADER.len());
    let mut rest = SHADER;
    while let Some(at) = rest.find(OPEN) {
        out.push_str(&rest[..at]);
        let tail = &rest[at + OPEN.len()..];
        let end = tail.find("u)").unwrap_or(0);
        match u32::from_str_radix(&tail[..end], 16) {
            Ok(bits) if end > 0 => {
                out.push_str(&format!("{:.16e}f", f64::from(f32::from_bits(bits))));
                rest = &tail[end + 2..];
            }
            _ => {
                out.push_str(OPEN);
                rest = tail;
            }
        }
    }
    out.push_str(rest);
    out
}

static SHARED: std::sync::OnceLock<Arc<Gpu>> = std::sync::OnceLock::new();

impl Gpu {
    fn release(&self, keep: impl Fn(&str) -> bool) {
        let Ok(mut store) = self.store.lock() else { return };
        let gone: Vec<String> = store.buffers.keys().filter(|k| !keep(k)).cloned().collect();
        for name in gone {
            if let Some(stored) = store.buffers.remove(&name) {
                stored.buffer.destroy();
            }
        }
        store.order.retain(|k| keep(k));
        for spare in store.spare.drain(..) {
            spare.destroy();
        }
        // why: the upload buffer stays while it's about the size of a kept scene, so the next edit's upload doesn't
        // make it again; one grown by a zoom or an export goes
        let scene = store.buffers.iter().filter(|(k, _)| k.starts_with("scene:")).map(|(_, b)| b.buffer.size()).max().unwrap_or(0);
        if store.upload.as_ref().is_some_and(|u| u.buffer.size() > scene + scene / 2) {
            store.upload.take().inspect(|u| u.buffer.destroy());
        }
        store.staging = None;
        store.params = None;
        drop(store);
        // why: destroyed buffers are freed once the device sees their last submission finish
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
    }
}

// why: scratch buffers grow to the largest render (a zoomed window, an export band) and would stay that size; once
// rendering settles they're dropped and the next render makes them again at its own size
pub fn trim() {
    if let Some(gpu) = SHARED.get() {
        gpu.release(|name| !name.starts_with("tmp:") && !name.starts_with("keep:"));
        if std::env::var_os("FILMLAB_TRACE").is_some()
            && let Ok(store) = gpu.store.lock()
        {
            let words: usize = store.buffers.values().map(|b| b.words).sum();
            eprintln!("gpu trim: {} buffers, {:.0} MB kept {:?}", store.buffers.len(), (words * 4) as f64 / 1_048_576.0, store.order);
        }
    }
}

/// Drops every buffer, film textures included, for when no photo is open.
pub fn reset() {
    if let Some(gpu) = SHARED.get() {
        gpu.release(|name| name.starts_with("lut:"));
    }
}

pub fn share(gpu: &Arc<Gpu>) {
    let _ = SHARED.set(gpu.clone());
}

pub struct Runner(pub Arc<Gpu>);

impl filmlab_engine::Runner for Runner {
    fn run(&self, program: &str) -> bool {
        serde_json::from_str::<Program>(program).ok().and_then(|p| self.0.execute(&p)).is_some()
    }

    fn upload(&self, block: &str) {
        let Ok(block) = serde_json::from_str::<Block>(block) else { return };
        if let Ok(mut store) = self.0.store.lock() {
            self.0.upload_into(&mut store, &block.id.clone(), &block, false);
            // why: an upload waits for the next submit; flushing it here keeps its staging memory from piling up
            self.0.queue.submit(std::iter::empty());
            let _ = self.0.device.poll(wgpu::PollType::Poll);
        }
    }

    fn forget(&self, name: &str) {
        let Ok(mut store) = self.0.store.lock() else { return };
        if let Some(stored) = store.buffers.remove(name) {
            store.free(stored.buffer);
            store.order.retain(|k| k != name);
        }
    }
}

pub fn texture_rgba(gpu: &Gpu, width: u32, height: u32, rgba: &[u8]) -> wgpu::Texture {
    gpu.device.create_texture_with_data(
        &gpu.queue,
        &wgpu::TextureDescriptor {
            label: Some("photo"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        rgba,
    )
}
