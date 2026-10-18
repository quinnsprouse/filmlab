use filmlab::gpu::{Gpu, Runner};
use filmlab_engine::core::{Engine, decode, probe};
use serde_json::{Value, json};

#[global_allocator]
static ALLOCATOR: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn footprint() -> f64 {
    let mut info: libc::rusage_info_v4 = unsafe { std::mem::zeroed() };
    unsafe { libc::proc_pid_rusage(libc::getpid(), libc::RUSAGE_INFO_V4, (&raw mut info).cast()) };
    info.ri_phys_footprint as f64 / 1_048_576.0
}

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    let gpu = args.iter().any(|a| a == "--gpu");
    if gpu {
        let g = Gpu::open().ok_or("gpu")?;
        filmlab::gpu::share(&g);
        filmlab_engine::install_gpu(Box::new(Runner(g)));
    }
    let mut last = footprint();
    let pid = std::process::id().to_string();
    // why: FILMLAB_LIVE collects mimalloc before each reading, so the numbers show what is held, not what the
    // allocator is still caching
    let live = std::env::var_os("FILMLAB_LIVE").is_some();
    let mut step = |label: &str| {
        if live {
            unsafe { libmimalloc_sys::mi_collect(true) };
            rayon::broadcast(|_| unsafe { libmimalloc_sys::mi_collect(true) });
            std::thread::sleep(std::time::Duration::from_millis(1500));
        }
        let now = footprint();
        let detail = std::process::Command::new("footprint").arg(&pid).output().map(|o| String::from_utf8_lossy(&o.stdout).to_string()).unwrap_or_default();
        let pick = |what: &str| detail.lines().find(|l| l.contains(what)).map(|l| l.split_whitespace().take(2).collect::<Vec<_>>().join(" ")).unwrap_or_default();
        println!("{label:>28}: {now:7.1} MB ({:+7.1})  malloc-large {:>10}  graphics {:>10}", now - last, pick("MALLOC_LARGE"), pick("(graphics)"));
        last = now;
    };
    step("start");
    let bytes = std::fs::read(&args[1]).map_err(|e| e.to_string())?;
    let p: Value = serde_json::from_str(&probe("p.dng", &bytes)?).map_err(|e| e.to_string())?;
    let facts = p["facts"].to_string();
    step("read + probe");
    let mut e = Engine::new();
    e.insert("k", decode(&facts, &bytes, 2560)?);
    step("preview decode (2560)");
    let dir = filmlab::stocks::folder();
    for id in ["Negative/Vision3 250D", "Print/I- Standard/Print SA1 Kodak 2393"] {
        e.load_lut(id, &std::fs::read_to_string(dir.join(format!("{id}.cube"))).map_err(|e| e.to_string())?)?;
    }
    step("2 LUTs");
    let s = p["defaults"].to_string();
    e.render("k", &s, &json!({"maxW": 2292, "maxH": 1528, "stages": "loupe"}).to_string())?;
    step("plain fit render (cached)");
    let film = json!({"negative": "Negative/Vision3 250D", "print": "Print/I- Standard/Print SA1 Kodak 2393"});
    e.render("k", &s, &json!({"maxW": 2292, "maxH": 1528, "stages": "loupe", "film": film}).to_string())?;
    step("film fit render");
    for i in 0..10 {
        let mut v: Value = serde_json::from_str(&s).map_err(|e| e.to_string())?;
        v["light"]["exposure"] = json!(0.05 * f64::from(i));
        e.render("k", &v.to_string(), &json!({"maxW": 2292, "maxH": 1528, "stages": "loupe", "film": film}).to_string())?;
    }
    step("10 film edits");
    e.insert("f", decode(&facts, &bytes, 0)?);
    step("full decode");
    e.render("f", &s, &json!({"maxW": 6000, "maxH": 4000, "stages": "loupe-zoom", "window": [2000, 1500, 2300, 1500], "film": film}).to_string())?;
    step("zoom window render");
    e.forget_view("f", "loupe-zoom");
    step("forget zoom");
    e.release("f");
    step("release full");
    filmlab::gpu::trim();
    step("gpu trim");
    e.release("k");
    filmlab::gpu::reset();
    step("release all + gpu reset");
    unsafe extern "C" {
        fn malloc_zone_pressure_relief(zone: *mut std::ffi::c_void, goal: usize) -> usize;
    }
    let freed = unsafe { malloc_zone_pressure_relief(std::ptr::null_mut(), 0) };
    step(&format!("pressure relief ({} MB)", freed / 1_048_576));
    unsafe { libmimalloc_sys::mi_collect(true) };
    std::thread::sleep(std::time::Duration::from_secs(2));
    step("mi_collect + 2 s");
    drop(e);
    unsafe { libmimalloc_sys::mi_collect(true) };
    std::thread::sleep(std::time::Duration::from_secs(1));
    step("engine dropped");
    Ok(())
}
