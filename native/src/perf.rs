use std::sync::{Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

use libc::qos_class_t;

/// How long every pool stays quiet before the GPU's scratch buffers and the pools' freed memory are handed back.
const SETTLE_AFTER: Duration = Duration::from_secs(2);

fn cores(level: u32) -> usize {
    let name = std::ffi::CString::new(format!("hw.perflevel{level}.physicalcpu")).unwrap_or_default();
    let mut value: i32 = 0;
    let mut size = std::mem::size_of::<i32>();
    let ok = unsafe { libc::sysctlbyname(name.as_ptr(), (&raw mut value).cast(), &raw mut size, std::ptr::null_mut(), 0) } == 0;
    if ok { value.max(0) as usize } else { 0 }
}

fn pool(name: &'static str, threads: usize, class: qos_class_t) -> rayon::ThreadPool {
    rayon::ThreadPoolBuilder::new()
        .num_threads(threads.max(1))
        .thread_name(move |i| format!("filmlab-{name}-{i}"))
        .start_handler(move |_| unsafe {
            libc::pthread_set_qos_class_self_np(class, 0);
        })
        .build()
        .unwrap_or_else(|_| rayon::ThreadPoolBuilder::new().build().expect("a rayon pool"))
}

/// The Mac's physical memory in bytes.
pub fn memory_bytes() -> u64 {
    let name = std::ffi::CString::new("hw.memsize").unwrap_or_default();
    let mut value: u64 = 0;
    let mut size = std::mem::size_of::<u64>();
    let ok = unsafe { libc::sysctlbyname(name.as_ptr(), (&raw mut value).cast(), &raw mut size, std::ptr::null_mut(), 0) } == 0;
    if ok { value } else { 0 }
}

fn all_cores() -> usize {
    let total = cores(0) + cores(1);
    if total > 0 { total } else { std::thread::available_parallelism().map_or(8, |n| n.get()) }
}

// why: Apple silicon schedules by QoS: the loupe's renders run user-interactive across every core, while
// thumbnails run at utility QoS on a few threads (the efficiency cores on M1–M4) and yield to edits
static INTERACTIVE: OnceLock<rayon::ThreadPool> = OnceLock::new();
static INITIATED: OnceLock<rayon::ThreadPool> = OnceLock::new();
static BACKGROUND: OnceLock<rayon::ThreadPool> = OnceLock::new();

pub fn interactive<R: Send>(work: impl FnOnce() -> R + Send) -> R {
    let _job = Job::start();
    INTERACTIVE.get_or_init(|| pool("render", all_cores(), qos_class_t::QOS_CLASS_USER_INTERACTIVE)).install(work)
}

pub fn initiated<R: Send>(work: impl FnOnce() -> R + Send) -> R {
    let _job = Job::start();
    INITIATED.get_or_init(|| pool("export", all_cores(), qos_class_t::QOS_CLASS_USER_INITIATED)).install(work)
}

pub fn background<R: Send>(work: impl FnOnce() -> R + Send) -> R {
    let _job = Job::start();
    BACKGROUND.get_or_init(|| pool("index", (cores(1) / 2).clamp(2, 4), qos_class_t::QOS_CLASS_UTILITY)).install(work)
}

struct Activity {
    running: usize,
    /// When the last job ended, until the work since the last settle has been settled.
    ended: Option<Instant>,
}

static ACTIVITY: Mutex<Activity> = Mutex::new(Activity { running: 0, ended: None });
static CHANGED: Condvar = Condvar::new();
static SETTLER: OnceLock<()> = OnceLock::new();

/// One piece of work in a pool, from the start of `interactive`, `initiated` or `background` to its end.
struct Job;

impl Job {
    fn start() -> Job {
        SETTLER.get_or_init(|| {
            let _ = std::thread::Builder::new().name("filmlab-settle".into()).spawn(settle);
        });
        if let Ok(mut a) = ACTIVITY.lock() {
            a.running += 1;
        }
        Job
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        if let Ok(mut a) = ACTIVITY.lock() {
            a.running = a.running.saturating_sub(1);
            a.ended = Some(Instant::now());
        }
        CHANGED.notify_all();
    }
}

// why: renders, decodes, exports and thumbnails (Develop's, agents' and the library's alike) grow the GPU's scratch
// buffers to the largest frame they drew and leave freed pages with the pool threads; once every pool has been quiet
// for a moment both go back, whichever work it was, so memory settles without each caller arranging it
fn settle() {
    loop {
        let Ok(mut a) = ACTIVITY.lock() else { return };
        loop {
            let quiet = a.ended.map(|t| t.elapsed()).filter(|_| a.running == 0);
            match quiet {
                Some(quiet) if quiet >= SETTLE_AFTER => break,
                Some(quiet) => a = CHANGED.wait_timeout(a, SETTLE_AFTER - quiet).unwrap_or_else(|e| e.into_inner()).0,
                None => a = CHANGED.wait(a).unwrap_or_else(|e| e.into_inner()),
            }
        }
        a.ended = None;
        drop(a);
        crate::gpu::trim();
        give_back();
    }
}

// why: mimalloc keeps a thread's freed pages, and the freed frame buffers in its arenas, until that thread
// allocates again, which an idle pool never does; once work settles each pool thread and the arenas hand the
// memory back to macOS
pub fn give_back() {
    unsafe { libmimalloc_sys::mi_collect(false) };
    std::thread::spawn(|| {
        for pool in [&INTERACTIVE, &INITIATED, &BACKGROUND].into_iter().filter_map(OnceLock::get) {
            pool.broadcast(|_| unsafe { libmimalloc_sys::mi_collect(true) });
        }
        unsafe { libmimalloc_sys::mi_collect(true) };
    });
}
