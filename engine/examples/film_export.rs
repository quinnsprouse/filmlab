use std::time::Instant;

use filmlab_engine::core::{Engine, export_plan, probe};
use serde_json::{Value, json};

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    let [_, photo, negative, print, out] = args.as_slice() else {
        return Err("usage: film_export <photo> <negative.cube> <print.cube> <out.png>".into());
    };
    let bytes = std::fs::read(photo).map_err(|e| e.to_string())?;
    let probed: Value = serde_json::from_str(&probe("photo.dng", &bytes)?).map_err(|e| e.to_string())?;
    let facts = probed["facts"].to_string();
    let state = json!({"settings": probed["defaults"], "rating": 0, "flag": "none", "label": null}).to_string();
    let options = json!({"format": "png", "quality": 90}).to_string();
    let plan: Value = serde_json::from_str(&export_plan(&facts, &state, &options)?).map_err(|e| e.to_string())?;
    let mut engine = Engine::new();
    engine.load("photo", &facts, &bytes, plan["maxEdge"].as_u64().unwrap_or(0) as usize)?;
    engine.load_lut("n", &std::fs::read_to_string(negative).map_err(|e| e.to_string())?)?;
    engine.load_lut("p", &std::fs::read_to_string(print).map_err(|e| e.to_string())?)?;
    let film = json!({"negative": "n", "print": "p"}).to_string();
    let t = Instant::now();
    let encoded = engine.export("photo", &facts, &state, &options, &film)?;
    let hash = encoded.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x100_0000_01b3));
    std::fs::write(out, &encoded).map_err(|e| e.to_string())?;
    println!("{plan} {:.0} ms {hash:016x}", t.elapsed().as_secs_f64() * 1e3);
    Ok(())
}
