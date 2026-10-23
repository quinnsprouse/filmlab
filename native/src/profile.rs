#[cfg(feature = "profile")]
macro_rules! zone {
    ($($span:tt)+) => {
        tracing::info_span!($($span)+).entered()
    };
}

#[cfg(not(feature = "profile"))]
macro_rules! zone {
    ($($span:tt)+) => {
        $crate::profile::Zone
    };
}

#[cfg(feature = "profile")]
macro_rules! span {
    ($($span:tt)+) => {
        tracing::info_span!($($span)+)
    };
}

#[cfg(not(feature = "profile"))]
macro_rules! span {
    ($($span:tt)+) => {
        $crate::profile::Zone
    };
}

pub(crate) use {span, zone};

/// What `zone!` and `span!` make when profiling is off: nothing.
#[cfg(not(feature = "profile"))]
pub struct Zone;

#[cfg(not(feature = "profile"))]
impl Zone {
    pub fn entered(self) -> Zone {
        self
    }
}

#[cfg(feature = "dhat")]
static HEAP: std::sync::Mutex<Option<dhat::Profiler>> = std::sync::Mutex::new(None);

/// Starts what the build profiles: Tracy's collector, and the heap profiler.
pub fn start() {
    #[cfg(feature = "profile")]
    {
        use tracing_subscriber::layer::SubscriberExt;
        let _ = tracing::subscriber::set_global_default(tracing_subscriber::registry().with(tracing_tracy::TracyLayer::new(Fields::default())));
    }
    #[cfg(feature = "dhat")]
    if let Ok(mut heap) = HEAP.lock() {
        *heap = Some(dhat::Profiler::new_heap());
    }
}

/// Writes `dhat-heap.json` in the working folder when the app quits.
#[cfg(feature = "dhat")]
pub fn on_quit(cx: &mut gpui_kit::App) {
    cx.on_app_quit(|_| {
        if let Ok(mut heap) = HEAP.lock() {
            heap.take();
        }
        async {}
    })
    .detach();
}

#[cfg(not(feature = "dhat"))]
pub fn on_quit(_: &mut gpui_kit::App) {}

/// Ends a Tracy frame: the viewer presented one.
pub fn presented() {
    #[cfg(feature = "profile")]
    if let Some(client) = tracy_client::Client::running() {
        client.frame_mark();
        client.plot(tracy_client::plot_name!("footprint MB"), crate::perf::footprint() as f64 / 1_048_576.0);
        client.plot(tracy_client::plot_name!("metal MB"), crate::gpu::allocated() as f64 / 1_048_576.0);
    }
}

/// Marks a frame of the interface (GPUI's render), as Tracy's secondary "ui" frames.
pub fn ui_frame() {
    #[cfg(feature = "profile")]
    if let Some(client) = tracy_client::Client::running() {
        client.secondary_frame_mark(tracy_client::frame_name!("ui"));
    }
}

/// `work` as one MCP tool call: a zone named for the tool each time it runs on the main thread between awaits.
#[cfg(feature = "profile")]
pub fn tool<F: std::future::Future>(name: &str, work: F) -> impl std::future::Future<Output = F::Output> {
    use tracing::Instrument;
    work.instrument(tracing::info_span!("mcp tool", tool = name))
}

#[cfg(not(feature = "profile"))]
pub fn tool<F: std::future::Future>(_: &str, work: F) -> F {
    work
}

/// The device features the profiler needs: timestamps around compute passes, for GPU zones.
#[cfg(feature = "profile")]
pub const GPU_FEATURES: wgpu::Features = wgpu::Features::TIMESTAMP_QUERY;

#[cfg(not(feature = "profile"))]
pub const GPU_FEATURES: wgpu::Features = wgpu::Features::empty();

#[cfg(feature = "profile")]
#[derive(Default)]
struct Fields(tracing_subscriber::fmt::format::DefaultFields);

#[cfg(feature = "profile")]
impl tracing_tracy::Config for Fields {
    type Formatter = tracing_subscriber::fmt::format::DefaultFields;

    fn formatter(&self) -> &Self::Formatter {
        &self.0
    }

    fn format_fields_in_zone_name(&self) -> bool {
        false
    }
}

/// Timestamps at the start and end of each pass of one GPU program, sent to Tracy as GPU zones once it has run.
#[cfg(feature = "profile")]
pub struct PassTimer {
    queries: wgpu::QuerySet,
    resolved: wgpu::Buffer,
    read: wgpu::Buffer,
    count: u32,
    period: f32,
}

#[cfg(not(feature = "profile"))]
pub enum PassTimer {}

#[cfg(feature = "profile")]
static GPU_CONTEXT: std::sync::OnceLock<Option<tracy_client::GpuContext>> = std::sync::OnceLock::new();

#[cfg(feature = "profile")]
impl PassTimer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, passes: usize) -> Option<PassTimer> {
        let count = u32::try_from(passes * 2).ok().filter(|n| *n > 0 && *n <= wgpu::QUERY_SET_MAX_QUERIES)?;
        if !device.features().contains(wgpu::Features::TIMESTAMP_QUERY) || tracy_client::Client::running().is_none() {
            return None;
        }
        let queries = device.create_query_set(&wgpu::QuerySetDescriptor { label: Some("pass times"), ty: wgpu::QueryType::Timestamp, count });
        let bytes = u64::from(count) * 8;
        let buffer = |usage| device.create_buffer(&wgpu::BufferDescriptor { label: Some("pass times"), size: bytes, usage, mapped_at_creation: false });
        let resolved = buffer(wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC);
        let read = buffer(wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST);
        Some(PassTimer { queries, resolved, read, count, period: queue.get_timestamp_period() })
    }

    pub fn writes(&self, pass: usize) -> wgpu::ComputePassTimestampWrites<'_> {
        let at = (pass * 2) as u32;
        wgpu::ComputePassTimestampWrites { query_set: &self.queries, beginning_of_pass_write_index: Some(at), end_of_pass_write_index: Some(at + 1) }
    }

    pub fn resolve(&self, encoder: &mut wgpu::CommandEncoder) {
        encoder.resolve_query_set(&self.queries, 0..self.count, &self.resolved, 0);
        encoder.copy_buffer_to_buffer(&self.resolved, 0, &self.read, 0, None);
    }

    /// Reads the timestamps back (`map` waits for the buffer) and sends one GPU zone a pass. Metal writes a pass's
    /// timestamps when the pass ends, untracked, so they are resolved by `resolve` only after the program finished.
    pub fn report<'a>(self, entries: impl Iterator<Item = &'a str>, map: impl FnOnce(&wgpu::BufferSlice) -> Option<()>) {
        let slice = self.read.slice(..);
        if map(&slice).is_none() {
            return;
        }
        let ticks: Vec<u64> = slice.get_mapped_range().map(|m| bytemuck::cast_slice(&m).to_vec()).unwrap_or_default();
        self.read.unmap();
        let ns = |tick: u64| (tick as f64 * f64::from(self.period)) as i64;
        let written = |[start, end]: &[u64; 2]| *start > 0 && end >= start;
        let Some(context) = GPU_CONTEXT.get_or_init(|| {
            let [first, _] = ticks.as_chunks::<2>().0.iter().find(|t| written(t))?;
            tracy_client::Client::running()?.new_gpu_context(Some("film"), tracy_client::GpuContextType::Invalid, ns(*first), 1.0).ok()
        }) else {
            return;
        };
        for (entry, [start, end]) in entries.zip(ticks.as_chunks::<2>().0).filter(|(_, t)| written(t)) {
            if let Ok(mut zone) = context.span_alloc(entry, "film.wgsl", file!(), line!()) {
                zone.end_zone();
                zone.upload_timestamp_start(ns(*start));
                zone.upload_timestamp_end(ns(*end));
            }
        }
    }
}

#[cfg(not(feature = "profile"))]
impl PassTimer {
    pub fn new(_: &wgpu::Device, _: &wgpu::Queue, _: usize) -> Option<PassTimer> {
        None
    }

    pub fn writes(&self, _: usize) -> wgpu::ComputePassTimestampWrites<'_> {
        match *self {}
    }

    pub fn resolve(&self, _: &mut wgpu::CommandEncoder) {
        match *self {}
    }

    pub fn report<'a>(self, _: impl Iterator<Item = &'a str>, _: impl FnOnce(&wgpu::BufferSlice) -> Option<()>) {
        match self {}
    }
}
