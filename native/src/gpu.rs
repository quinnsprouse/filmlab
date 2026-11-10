use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use filmlab_engine::gpu::{Block, Program, Words};

const SHADER: &str = include_str!("film.wgsl");
const PARAMS_SLOT: u64 = 512;
const SPARE: usize = 4;
// uploads smaller than this (the tables, dust and taps) go through queue.write_buffer
const STAGED: usize = 1 << 20;
const SLICE: Duration = Duration::from_millis(2);
const STALL: Duration = Duration::from_secs(10);
const STORAGE: [u32; 9] = [1, 2, 3, 4, 5, 6, 7, 8, 9];
const WRITABLE: [u32; 3] = [2, 7, 9];

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

    fn reuse(&mut self, bytes: u64, usage: wgpu::BufferUsages) -> Option<wgpu::Buffer> {
        let fits = |b: &wgpu::Buffer| b.usage() == usage && b.size() >= bytes && b.size() <= bytes + bytes / 2;
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

// why: one film program runs at a time, from upload through readback; a thumbnail's, a neighbour's or an export
// band's program waits while one someone is looking at waits, rather than racing it for the store
#[derive(Default)]
struct Turns {
    state: Mutex<Waiting>,
    changed: std::sync::Condvar,
}

#[derive(Default)]
struct Waiting {
    busy: bool,
    urgent: usize,
}

struct Turn<'a>(&'a Turns);

impl Turns {
    fn take(&self, urgent: bool) -> Option<Turn<'_>> {
        let mut state = self.state.lock().ok()?;
        state.urgent += usize::from(urgent);
        while state.busy || (!urgent && state.urgent > 0) {
            state = self.changed.wait(state).ok()?;
        }
        state.urgent -= usize::from(urgent);
        state.busy = true;
        Some(Turn(self))
    }
}

impl Drop for Turn<'_> {
    fn drop(&mut self) {
        if let Ok(mut state) = self.0.state.lock() {
            state.busy = false;
        }
        self.0.changed.notify_all();
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
    turns: Turns,
    mappable: bool,
    stalled: AtomicBool,
}

fn bits<'a>(words: &Words<'a>) -> &'a [u32] {
    match *words {
        Words::Float(w) => bytemuck::cast_slice(w),
        Words::Bits(w) => w,
    }
}

fn packed(words: &Words) -> Vec<u32> {
    bits(words).chunks(3).flat_map(|c| [c[0], c.get(1).copied().unwrap_or(0), c.get(2).copied().unwrap_or(0), 0]).collect()
}

impl Gpu {
    pub fn open() -> Option<Arc<Gpu>> {
        let _zone = crate::profile::zone!("gpu open");
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
        let mappable = if unified { wgpu::Features::MAPPABLE_PRIMARY_BUFFERS } else { wgpu::Features::empty() };
        let features = adapter.features() & (mappable | crate::profile::GPU_FEATURES);
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
        let pipelines = entry_points()
            .map(|name| {
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
        Some(Arc::new(Gpu { instance, adapter, device, queue, layout, pipelines, dummies, store: Mutex::default(), turns: Turns::default(), mappable, stalled: AtomicBool::new(false) }))
    }

    // why: on unified memory a buffer the CPU writes (an uploaded scene or LUT) is mapped and written where the shader
    // reads it; it is write-combined, so it's never one the CPU reads back
    fn usage(&self, written: bool) -> wgpu::BufferUsages {
        let shared = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST;
        match (self.mappable, written) {
            (false, _) => shared,
            (true, false) => shared | wgpu::BufferUsages::MAP_READ,
            (true, true) => shared | wgpu::BufferUsages::MAP_WRITE,
        }
    }

    fn ensure(&self, store: &mut Store, name: &str, size: usize, written: bool) {
        let usage = self.usage(written);
        if store.buffers.get(name).is_some_and(|s| s.words >= size && s.buffer.usage() == usage) {
            store.order.retain(|k| k != name);
            store.order.push(name.to_string());
            return;
        }
        if let Some(old) = store.buffers.remove(name) {
            store.free(old.buffer);
        }
        store.order.retain(|k| k != name);
        let words = size.max(4);
        let buffer = store.reuse((words * 4) as u64, usage).unwrap_or_else(|| self.device.create_buffer(&wgpu::BufferDescriptor { label: None, size: (words * 4) as u64, usage, mapped_at_creation: false }));
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
        let len = block.words.len();
        let words = if pack { len / 3 * 4 } else { len };
        let large = words * 4 >= STAGED;
        let mapped = large && self.mappable;
        self.ensure(store, name, words, mapped);
        if store.buffers.get(name).is_none_or(|t| t.holds.as_deref() == Some(block.id.as_str())) {
            return;
        }
        let data = if pack { std::borrow::Cow::Owned(packed(&block.words)) } else { std::borrow::Cow::Borrowed(bits(&block.words)) };
        let bytes: &[u8] = bytemuck::cast_slice(&data);
        let written = match (large, mapped) {
            (true, true) => self.write_mapped(store, name, bytes),
            (true, false) => self.stage(store, name, bytes),
            _ => false,
        };
        if !written {
            let Some(target) = store.buffers.get(name) else { return };
            self.queue.write_buffer(&target.buffer, 0, bytes);
        }
        if let Some(target) = store.buffers.get_mut(name) {
            target.holds = Some(block.id.clone());
        }
    }

    fn write_mapped(&self, store: &Store, name: &str, bytes: &[u8]) -> bool {
        let Some(target) = store.buffers.get(name).map(|t| t.buffer.clone()) else { return false };
        let slice = target.slice(..bytes.len() as u64);
        if self.map(&slice, wgpu::MapMode::Write).is_none() {
            return false;
        }
        let written = slice.get_mapped_range_mut().map(|mut view| view.copy_from_slice(bytes)).is_ok();
        target.unmap();
        written
    }

    // why: on a discrete GPU, queue.write_buffer makes a staging buffer per call, and a frame's worth of fresh pages
    // costs more than the copy; one kept staging buffer is mapped again once its last copy is done, which an upload never waits on, and a
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
        if self.map(&upload.buffer.slice(..size), wgpu::MapMode::Write).is_none() {
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

    // why: a wait holds wgpu's device lock, so each is a short slice and a GPU that stops answering is given up on
    // (the engine then finishes the frame on the CPU) rather than waited on for good
    fn settle<T>(&self, mut ready: impl FnMut() -> Option<T>) -> Option<T> {
        let started = Instant::now();
        loop {
            if let Some(done) = ready() {
                return Some(done);
            }
            if started.elapsed() > STALL {
                self.stalled.store(true, Ordering::Relaxed);
                return None;
            }
            let _ = self.device.poll(wgpu::PollType::Wait { submission_index: None, timeout: Some(SLICE) });
        }
    }

    fn map(&self, slice: &wgpu::BufferSlice, mode: wgpu::MapMode) -> Option<()> {
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(mode, move |r| {
            let _ = tx.send(r);
        });
        self.settle(|| rx.try_recv().ok())?.ok()
    }

    fn execute(&self, program: Program) -> Option<()> {
        if self.stalled.load(Ordering::Relaxed) {
            return None;
        }
        let _zone = crate::profile::zone!(
            "gpu program",
            passes = program.passes.len(),
            first = program.passes.first().map_or("", |p| p.entry),
            buffer_mb = program.buffers.iter().map(|b| b.words * 4).sum::<usize>() / 1_048_576
        );
        self.execute_inner(program)
    }

    fn pipeline(&self, entry: &str) -> &wgpu::ComputePipeline {
        self.pipelines.get(entry).unwrap_or_else(|| panic!("film.wgsl has no compute entry point `{entry}`"))
    }

    fn execute_inner(&self, mut program: Program) -> Option<()> {
        let pipelines: Vec<&wgpu::ComputePipeline> = program.passes.iter().map(|p| self.pipeline(p.entry)).collect();
        let _turn = self.turns.take(crate::perf::urgent())?;
        let mut store = self.store.lock().ok()?;
        let store = &mut *store;
        if program.keep.iter().any(|k| k.need && store.buffers.get(k.buffer).and_then(|b| b.holds.as_deref()) != Some(k.id.as_str())) {
            return None;
        }
        let scope = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        for spec in &program.buffers {
            match &spec.upload {
                Some(block) => self.upload_into(store, &spec.name, block, spec.pack),
                None => self.ensure(store, &spec.name, spec.words, false),
            }
        }
        for w in &program.writes {
            let target = store.buffers.get_mut(w.buffer)?;
            target.holds = None;
            self.queue.write_buffer(&target.buffer, (w.offset * 4) as u64, bytemuck::cast_slice(bits(&w.block.words)));
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
        let mut targets: Vec<&str> = program.copies.iter().map(|c| c.to).collect();
        targets.dedup();
        let mut filled = Vec::new();
        for to in targets {
            let copies = program.copies.iter().filter(|c| c.to == to);
            let sources: Option<Vec<String>> = copies.clone().map(|c| store.buffers.get(&c.from).and_then(|f| f.holds.as_ref()).map(|h| format!("{h}@{}", c.offset))).collect();
            let holds = sources.map(|s| s.join(","));
            let target = store.buffers.get_mut(to)?;
            let held = holds.is_some() && target.holds == holds;
            target.holds = None;
            filled.push((to, holds));
            if held {
                continue;
            }
            for c in copies {
                let from = store.buffers.get(&c.from)?;
                encoder.copy_buffer_to_buffer(&from.buffer, 0, &store.buffers.get(to)?.buffer, (c.offset * 4) as u64, Some((from.words * 4) as u64));
            }
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
        let timer = crate::profile::PassTimer::new(&self.device, &self.queue, program.passes.len());
        // why: one compute pass holds every dispatch (a serial Metal encoder orders them), instead of an encoder each;
        // timing passes needs one each, as Metal timestamps only a pass's start and end
        let mut compute = timer.is_none().then(|| encoder.begin_compute_pass(&wgpu::ComputePassDescriptor::default()).forget_lifetime());
        for (k, ((pass, bind), pipeline)) in program.passes.iter().zip(&binds).zip(pipelines).enumerate() {
            let groups = (pass.groups[0].max(1) as u32, pass.groups[1].max(1) as u32);
            if let Some(compute) = compute.as_mut() {
                compute.set_pipeline(pipeline);
                compute.set_bind_group(0, bind, &[]);
                compute.dispatch_workgroups(groups.0, groups.1, 1);
                continue;
            }
            let mut timed = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some(pass.entry), timestamp_writes: timer.as_ref().map(|t| t.writes(k)) });
            timed.set_pipeline(pipeline);
            timed.set_bind_group(0, bind, &[]);
            timed.dispatch_workgroups(groups.0, groups.1, 1);
        }
        drop(compute);
        let total: u64 = program.reads.iter().map(|r| (r.into.len() * 4) as u64).sum();
        if self.mappable {
            self.queue.submit([encoder.finish()]);
            if pollster::block_on(scope.pop()).is_some() {
                return None;
            }
            for read in program.reads.iter_mut().filter(|r| !r.into.is_empty()) {
                let buffer = store.buffers.get(&read.buffer)?.buffer.clone();
                let slice = buffer.slice(..(read.into.len() * 4) as u64);
                self.map(&slice, wgpu::MapMode::Read)?;
                bytemuck::cast_slice_mut(read.into).copy_from_slice(&slice.get_mapped_range().ok()?);
                buffer.unmap();
            }
            mark(store, &program, filled);
            self.report(timer, &program);
            return Some(());
        }
        if total > 0 && store.staging.as_ref().is_none_or(|(_, size)| *size < total) {
            let buffer = self.device.create_buffer(&wgpu::BufferDescriptor { label: None, size: total, usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
            store.staging = Some((buffer, total));
        }
        let mut offset = 0u64;
        for r in &program.reads {
            let bytes = (r.into.len() * 4) as u64;
            if let Some((out, _)) = &store.staging {
                encoder.copy_buffer_to_buffer(&store.buffers.get(&r.buffer)?.buffer, 0, out, offset, Some(bytes));
            }
            offset += bytes;
        }
        self.queue.submit([encoder.finish()]);
        if pollster::block_on(scope.pop()).is_some() {
            return None;
        }
        if total == 0 {
            mark(store, &program, filled);
            self.report(timer, &program);
            return Some(());
        }
        let out = store.staging.as_ref()?.0.clone();
        let slice = out.slice(..total);
        self.map(&slice, wgpu::MapMode::Read)?;
        {
            let mapped = slice.get_mapped_range().ok()?;
            let mut at = 0usize;
            for r in program.reads.iter_mut() {
                let bytes = r.into.len() * 4;
                bytemuck::cast_slice_mut(r.into).copy_from_slice(&mapped[at..at + bytes]);
                at += bytes;
            }
        }
        out.unmap();
        mark(store, &program, filled);
        self.report(timer, &program);
        Some(())
    }

    fn report(&self, timer: Option<crate::profile::PassTimer>, program: &Program) {
        let Some(timer) = timer else { return };
        if self.done().is_none() {
            return;
        }
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        timer.resolve(&mut encoder);
        self.queue.submit([encoder.finish()]);
        timer.report(program.passes.iter().map(|p| p.entry), |slice| self.map(slice, wgpu::MapMode::Read));
    }

    fn done(&self) -> Option<()> {
        let (tx, rx) = std::sync::mpsc::channel();
        self.queue.on_submitted_work_done(move || {
            let _ = tx.send(());
        });
        self.settle(|| rx.try_recv().ok())
    }
}

fn mark(store: &mut Store, program: &Program, filled: Vec<(&str, Option<String>)>) {
    for (to, holds) in filled {
        if let Some(b) = store.buffers.get_mut(to) {
            b.holds = holds;
        }
    }
    for k in program.keep.iter().filter(|k| !k.need) {
        if let Some(b) = store.buffers.get_mut(k.buffer) {
            b.holds = Some(k.id.clone());
        }
    }
}

fn entry_points() -> impl Iterator<Item = &'static str> {
    SHADER.split("@compute").skip(1).filter_map(|after| after.split_once("fn ")?.1.split_once('(').map(|(name, _)| name.trim()))
}

// why: naga can't evaluate bitcast in a const expression; each one is a
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

impl Gpu {
    /// Bytes Metal holds for the process's GPU device: every buffer and texture of the film renders, the viewer and gpui.
    pub fn allocated(&self) -> u64 {
        let hal = unsafe { self.device.as_hal::<wgpu::hal::api::Metal>() };
        hal.map_or(0, |d| {
            let device: &objc2::runtime::AnyObject = d.raw_device().as_ref();
            unsafe { objc2::msg_send![device, currentAllocatedSize] }
        })
    }
}

static SHARED: std::sync::OnceLock<Arc<Gpu>> = std::sync::OnceLock::new();

impl Gpu {
    fn release(&self, keep: fn(&str) -> bool) {
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
        let _ = self.done();
    }
}

// why: a render holds the store while it runs, and freeing waits for the GPU, so neither happens on the thread that
// asked (the main thread, between frames)
fn release_later(keep: fn(&str) -> bool, then: fn(&Gpu)) {
    let Some(gpu) = SHARED.get().cloned() else { return };
    std::thread::spawn(move || {
        gpu.release(keep);
        then(&gpu);
    });
}

// why: scratch buffers grow to the largest render (a zoomed window, an export band) and would stay that size; once
// rendering settles they're dropped and the next render makes them again at its own size
pub fn trim() {
    release_later(|name| !name.starts_with("tmp:") && !name.starts_with("keep:"), |_| ());
}

/// Drops every buffer, film textures included, for when no photo is open.
pub fn reset() {
    release_later(|name| name.starts_with("lut:"), |_| ());
}

/// Bytes Metal holds for the process, or 0 before the GPU is open.
pub fn allocated() -> u64 {
    SHARED.get().map_or(0, |gpu| gpu.allocated())
}

pub fn share(gpu: &Arc<Gpu>) {
    let _ = SHARED.set(gpu.clone());
}

pub struct Runner(pub Arc<Gpu>);

impl filmlab_engine::Runner for Runner {
    fn run(&self, program: Program) -> bool {
        self.0.execute(program).is_some()
    }

    fn upload(&self, block: Block) {
        if self.0.stalled.load(Ordering::Relaxed) {
            return;
        }
        if let Ok(mut store) = self.0.store.lock() {
            self.0.upload_into(&mut store, &block.id, &block, false);
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

// why: wgpu reconfigures a surface only once its device is idle, and fails if another thread submits meanwhile; the
// viewer's own device is idle between its frames whatever the film renders and thumbnails are doing, and its frames
// don't queue behind their work
/// The device the viewer draws and presents with, apart from the one renders run on.
pub struct Screen {
    pub instance: wgpu::Instance,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

impl Gpu {
    pub fn screen(&self) -> Option<Screen> {
        let required_limits = wgpu::Limits { max_texture_dimension_2d: self.adapter.limits().max_texture_dimension_2d, ..wgpu::Limits::default() };
        let (device, queue) = pollster::block_on(self.adapter.request_device(&wgpu::DeviceDescriptor { label: Some("screen"), required_limits, ..Default::default() })).ok()?;
        Some(Screen { instance: self.instance.clone(), device, queue })
    }
}

impl Screen {
    pub fn texture_rgba(&self, width: u32, height: u32) -> wgpu::Texture {
        self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("photo"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        })
    }

    pub fn write_rgba(&self, texture: &wgpu::Texture, width: u32, height: u32, rgba: &[u8]) {
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            rgba,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(width * 4), rows_per_image: Some(height) },
            wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        );
    }
}
