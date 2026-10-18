use filmlab::gpu::{Gpu, Runner};
use filmlab_engine::core::{Engine, probe};
use serde_json::{Value, json};

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--gpu") {
        filmlab_engine::install_gpu(Box::new(Runner(Gpu::open().ok_or("no gpu")?)));
    }
    let bytes = std::fs::read(&args[1]).map_err(|e| e.to_string())?;
    let p: Value = serde_json::from_str(&probe("p.dng", &bytes)?).map_err(|e| e.to_string())?;
    let mut e = Engine::new();
    e.load("k", &p["facts"].to_string(), &bytes, 2560)?;
    let dir = filmlab::stocks::folder();
    for id in ["Negative/Vision3 250D", "Print/I- Standard/Print SA1 Kodak 2393"] {
        e.load_lut(id, &std::fs::read_to_string(dir.join(format!("{id}.cube"))).map_err(|e| e.to_string())?)?;
    }
    let film = json!({"negative": "Negative/Vision3 250D", "print": "Print/I- Standard/Print SA1 Kodak 2393", "grain": {"amount": 0.0995, "size": 1.0, "softness": 0.05, "defocus": 1.0}, "blur": 0.05, "dust": {"on": true, "size": 1.758, "softness": 0.245}, "halation": {"on": std::env::var_os("HAL").is_some()}});
    let view = json!({"maxW": 2292, "maxH": 1528, "stages": "loupe", "film": film}).to_string();
    let mut s = p["defaults"].clone();
    let n: usize = args.get(2).and_then(|a| a.parse().ok()).unwrap_or(200);
    let t = std::time::Instant::now();
    for i in 0..n {
        s["light"]["exposure"] = json!(0.001 * i as f64);
        e.render("k", &s.to_string(), &view)?;
    }
    println!("{:.1} ms per exposure edit", t.elapsed().as_secs_f64() * 1000.0 / n as f64);
    Ok(())
}
