use std::sync::OnceLock;

use libc::qos_class_t;

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
    INTERACTIVE.get_or_init(|| pool("render", all_cores(), qos_class_t::QOS_CLASS_USER_INTERACTIVE)).install(work)
}

pub fn initiated<R: Send>(work: impl FnOnce() -> R + Send) -> R {
    INITIATED.get_or_init(|| pool("export", all_cores(), qos_class_t::QOS_CLASS_USER_INITIATED)).install(work)
}

pub fn background<R: Send>(work: impl FnOnce() -> R + Send) -> R {
    BACKGROUND.get_or_init(|| pool("index", (cores(1) / 2).clamp(2, 4), qos_class_t::QOS_CLASS_UTILITY)).install(work)
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
