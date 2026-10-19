use std::time::Instant;

use filmlab::gpu::{Gpu, Runner};
use filmlab_engine::core::{Engine, export_plan, probe};
use serde_json::{Value, json};

// Times one full-size export with the default film look: decode, the film render and the encode.
fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--gpu") {
        filmlab_engine::install_gpu(Box::new(Runner(Gpu::open().ok_or("no gpu")?)));
    }
    let format = args.get(2).filter(|a| !a.starts_with("--")).cloned().unwrap_or_else(|| "jpeg".into());
    let bytes = std::fs::read(&args[1]).map_err(|e| e.to_string())?;
    let probed: Value = serde_json::from_str(&probe("p.dng", &bytes)?).map_err(|e| e.to_string())?;
    let facts = probed["facts"].to_string();
    let state = json!({"settings": probed["defaults"], "rating": 0, "flag": "none", "label": null}).to_string();
    let options = json!({"format": format, "quality": 90}).to_string();
    let plan: Value = serde_json::from_str(&export_plan(&facts, &state, &options)?).map_err(|e| e.to_string())?;
    let mut engine = Engine::new();
    let dir = filmlab::stocks::folder();
    for id in [filmlab::stocks::DEFAULT_NEGATIVE, filmlab::stocks::DEFAULT_PRINT] {
        engine.load_lut(id, &std::fs::read_to_string(dir.join(format!("{id}.cube"))).map_err(|e| e.to_string())?)?;
    }
    let film = filmlab::film::FilmSettings { negative: Some(filmlab::stocks::DEFAULT_NEGATIVE.into()), print: Some(filmlab::stocks::DEFAULT_PRINT.into()), ..Default::default() };
    let film = filmlab::film::render_film(Some(&film)).map(|f| f.to_value()).unwrap_or(Value::Null).to_string();
    for round in 0..2 {
        let t = Instant::now();
        filmlab::perf::initiated(|| engine.load("x", &facts, &bytes, plan["maxEdge"].as_u64().unwrap_or(0) as usize))?;
        let load = t.elapsed();
        let t = Instant::now();
        let encoded = filmlab::perf::initiated(|| engine.export("x", &facts, &state, &options, &film))?;
        let export = t.elapsed();
        engine.release("x");
        println!("round {round}: decode {:.0} ms, export {:.0} ms, {} KB {format}", load.as_secs_f64() * 1e3, export.as_secs_f64() * 1e3, encoded.len() / 1024);
    }
    Ok(())
}
