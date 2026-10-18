use filmlab_engine::core::{native_size, probe};
use serde_json::{Value, json};

fn main() -> Result<(), String> {
    let path = std::env::args().nth(1).ok_or("photo")?;
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let p: Value = serde_json::from_str(&probe("p.dng", &bytes)?).map_err(|e| e.to_string())?;
    let mut s = p["defaults"].clone();
    println!("crop {}", s["crop"]);
    println!("native {}", native_size(&p["facts"].to_string(), &s.to_string())?);
    s["crop"]["geometry"]["rect"] = json!({"x0": 0.2, "y0": 0.1, "x1": 0.8, "y1": 0.7});
    println!("cropped {}", native_size(&p["facts"].to_string(), &s.to_string())?);
    Ok(())
}
