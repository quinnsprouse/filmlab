use std::io::Write;

use filmlab_engine::core::{Engine, probe};
use serde_json::Value;

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    let [_, photo, settings_patch, view, out] = args.as_slice() else {
        return Err("usage: render <photo> <settings-patch.json> <view.json> <out.rgba>".into());
    };
    let bytes = std::fs::read(photo).map_err(|e| e.to_string())?;
    let name = std::path::Path::new(photo).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let probed: Value = serde_json::from_str(&probe(&name, &bytes)?).map_err(|e| e.to_string())?;
    let facts = probed["facts"].to_string();
    let patch = std::fs::read_to_string(settings_patch).map_err(|e| e.to_string())?;
    let settings = filmlab_engine::core::apply_profile(&probed["defaults"].to_string(), &patch, 1.0)?;
    let view = std::fs::read_to_string(view).map_err(|e| e.to_string())?;
    let max_edge: usize = serde_json::from_str::<Value>(&view).ok().and_then(|v| v["maxEdge"].as_u64()).unwrap_or(2560) as usize;
    let mut engine = Engine::new();
    engine.load("photo", &facts, &bytes, max_edge)?;
    let frame = engine.render("photo", &settings, &view)?;
    std::fs::File::create(out).and_then(|mut f| f.write_all(&frame.rgba)).map_err(|e| e.to_string())?;
    println!("{} {}", frame.width, frame.height);
    Ok(())
}
