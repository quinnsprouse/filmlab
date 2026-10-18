use filmlab_engine::core::{decode, probe};
use serde_json::Value;

fn main() -> Result<(), String> {
    let path = std::env::args().nth(1).ok_or("photo")?;
    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
    let p: Value = serde_json::from_str(&probe("p.dng", &bytes)?).map_err(|e| e.to_string())?;
    let facts = p["facts"].to_string();
    for _ in 0..40 {
        decode(&facts, &bytes, 2560)?;
    }
    Ok(())
}
