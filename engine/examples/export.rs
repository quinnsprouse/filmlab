use std::io::Write;

use filmlab_engine::core::{Engine, export_plan, probe, set_controls};
use serde_json::{Value, json};

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    let [_, photo, values, options, out] = args.as_slice() else {
        return Err("usage: export <photo> <values.json> <options.json> <out>".into());
    };
    let bytes = std::fs::read(photo).map_err(|e| e.to_string())?;
    let name = std::path::Path::new(photo).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let probed: Value = serde_json::from_str(&probe(&name, &bytes)?).map_err(|e| e.to_string())?;
    let facts = probed["facts"].to_string();
    let values = std::fs::read_to_string(values).map_err(|e| e.to_string())?;
    let settings: Value = serde_json::from_str(&set_controls(&facts, &probed["defaults"].to_string(), &values)?).map_err(|e| e.to_string())?;
    let state = json!({"settings": settings, "rating": 0, "flag": "none", "label": null}).to_string();
    let options = std::fs::read_to_string(options).map_err(|e| e.to_string())?;
    let plan: Value = serde_json::from_str(&export_plan(&facts, &state, &options)?).map_err(|e| e.to_string())?;
    let mut engine = Engine::new();
    engine.load("photo", &facts, &bytes, plan["maxEdge"].as_u64().unwrap_or(0) as usize)?;
    let encoded = engine.export("photo", &facts, &state, &options, "null")?;
    std::fs::File::create(out).and_then(|mut f| f.write_all(&encoded)).map_err(|e| e.to_string())?;
    println!("{plan}");
    Ok(())
}
