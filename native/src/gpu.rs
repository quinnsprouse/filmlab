use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde::Deserialize;
use wgpu::util::DeviceExt;

const SHADER: &str = include_str!("../../src/lib/engine/film.wgsl");
const PARAMS_SLOT: u64 = 512;
const STORAGE: [u32; 9] = [1, 2, 3, 4, 5, 6, 7, 8, 9];
const WRITABLE: [u32; 3] = [2, 7, 9];
const ENTRIES: [&str; 24] = [
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
    "pyramidAdd",
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
struct Program {
    buffers: Vec<BufferSpec>,
    copies: Vec<Copy>,
    writes: Vec<Write>,
    passes: Vec<Pass>,
    reads: Vec<Read>,
}

struct Stored {
    buffer: wgpu::Buffer,
    words: usize,
    holds: Option<String>,
}

#[derive(Default)]
struct Store {
    order: Vec<String>,
    buffers: HashMap<String, Stored>,
    params: Option<(wgpu::Buffer, u64)>,
    staging: Option<(wgpu::Buffer, u64)>,
}

fn kept(kind: &str) -> Option<usize> {
    match kind {
        "scene" => Some(3),
        "lut" => Some(8),
        "tex" => Some(4),
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
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor { label: Some("filmlab"), required_limits: required, ..Default::default() })).ok()?;
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
        Some(Arc::new(Gpu { instance, adapter, device, queue, layout, pipelines, dummies, store: Mutex::default() }))
    }

    fn ensure(&self, store: &mut Store, name: &str, size: usize) {
        if store.buffers.get(name).is_some_and(|s| s.words >= size) {
            store.order.retain(|k| k != name);
            store.order.push(name.to_string());
            return;
        }
        if let Some(old) = store.buffers.remove(name) {
            old.buffer.destroy();
        }
        store.order.retain(|k| k != name);
        let words = size.max(4);
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: (words * 4) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        store.buffers.insert(name.to_string(), Stored { buffer, words, holds: None });
        store.order.push(name.to_string());
        let kind = name.split(':').next().unwrap_or("");
        let Some(limit) = kept(kind) else { return };
        let same: Vec<String> = store.order.iter().filter(|k| k.starts_with(&format!("{kind}:"))).cloned().collect();
        for key in same.iter().take(same.len().saturating_sub(limit)) {
            if let Some(s) = store.buffers.remove(key) {
                s.buffer.destroy();
            }
            store.order.retain(|k| k != key);
        }
    }

    fn upload_into(&self, store: &mut Store, name: &str, block: &Block, pack: bool) {
        self.ensure(store, name, if pack { block.len / 3 * 4 } else { block.len });
        let Some(target) = store.buffers.get_mut(name) else { return };
        if target.holds.as_deref() == Some(block.id.as_str()) {
            return;
        }
        if pack {
            self.queue.write_buffer(&target.buffer, 0, bytemuck::cast_slice(&packed(block)));
        } else {
            self.queue.write_buffer(&target.buffer, 0, bytemuck::cast_slice(memory(block.at, block.len)));
        }
        target.holds = Some(block.id.clone());
    }

    fn execute(&self, program: &Program) -> Option<()> {
        let started = std::time::Instant::now();
        let traced = self.execute_inner(program);
        if std::env::var_os("FILMLAB_GPU_TRACE").is_some() {
            let entries: Vec<&str> = program.passes.iter().map(|p| p.entry.as_str()).collect();
            let mb = |f: &dyn Fn(&str) -> bool| program.buffers.iter().filter(|b| f(&b.name)).map(|b| b.words * 4).sum::<usize>() as f64 / 1_048_576.0;
            eprintln!("gpu {:.1} ms {} passes {:?} scratch {:.0} MB in {} slots, other {:.0} MB", started.elapsed().as_secs_f64() * 1000.0, entries.len(), entries.first(), mb(&|n| n.starts_with("tmp:")), program.buffers.iter().filter(|b| b.name.starts_with("tmp:slot")).count(), mb(&|n| !n.starts_with("tmp:")));
            for b in program.buffers.iter().filter(|b| b.words > 1_000_000) {
                eprintln!("    {} {:.0} MB", b.name, (b.words * 4) as f64 / 1_048_576.0);
            }
        }
        traced
    }

    fn execute_inner(&self, program: &Program) -> Option<()> {
        let mut store = self.store.lock().ok()?;
        let store = &mut *store;
        let scope = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        for spec in &program.buffers {
            match &spec.upload {
                Some(block) => self.upload_into(store, &spec.name, block, spec.pack),
                None => self.ensure(store, &spec.name, spec.words),
            }
        }
        for w in &program.writes {
            let target = &store.buffers.get(&w.buffer)?.buffer;
            self.queue.write_buffer(target, (w.offset * 4) as u64, bytemuck::cast_slice(memory(w.block.at, w.block.len)));
        }
        let bytes = (program.passes.len().max(1) as u64) * PARAMS_SLOT;
        if store.params.as_ref().is_none_or(|(_, size)| *size < bytes) {
            let buffer = self.device.create_buffer(&wgpu::BufferDescriptor { label: None, size: bytes, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
            store.params = Some((buffer, bytes));
        }
        let params = &store.params.as_ref()?.0;
        for (k, pass) in program.passes.iter().enumerate() {
            self.queue.write_buffer(params, k as u64 * PARAMS_SLOT, bytemuck::cast_slice(&pass.params));
        }
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        for c in &program.copies {
            let from = store.buffers.get(&c.from)?;
            let to = &store.buffers.get(&c.to)?.buffer;
            encoder.copy_buffer_to_buffer(&from.buffer, 0, to, (c.offset * 4) as u64, Some((from.words * 4) as u64));
        }
        for (k, pass) in program.passes.iter().enumerate() {
            let pipeline = self.pipelines.get(pass.entry.as_str())?;
            let mut entries = vec![wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: params, offset: k as u64 * PARAMS_SLOT, size: wgpu::BufferSize::new(PARAMS_SLOT) }),
            }];
            for (binding, dummy) in &self.dummies {
                let buffer = match pass.bind.iter().find(|(b, _)| b == binding) {
                    Some((_, name)) => &store.buffers.get(name)?.buffer,
                    None => dummy,
                };
                entries.push(wgpu::BindGroupEntry { binding: *binding, resource: buffer.as_entire_binding() });
            }
            let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &self.layout, entries: &entries });
            {
                let mut compute = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor::default());
                compute.set_pipeline(pipeline);
                compute.set_bind_group(0, &bind, &[]);
                compute.dispatch_workgroups(pass.groups[0].max(1) as u32, pass.groups[1].max(1) as u32, 1);
            }
            if std::env::var_os("FILMLAB_GPU_PROFILE").is_some() {
                let t = std::time::Instant::now();
                self.queue.submit([std::mem::replace(&mut encoder, self.device.create_command_encoder(&Default::default())).finish()]);
                let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
                eprintln!("  {:>18} {:>5}x{:<5} {:.2} ms", pass.entry, pass.groups[0], pass.groups[1], t.elapsed().as_secs_f64() * 1000.0);
            }
        }
        let total: u64 = program.reads.iter().map(|r| (r.len * 4) as u64).sum();
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
        gpu.release(|name| !name.starts_with("tmp:"));
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
            stored.buffer.destroy();
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
