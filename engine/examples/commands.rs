use filmlab_engine::core::{Commands, probe};
use serde_json::Value;

fn main() -> Result<(), String> {
    let path = std::env::args().nth(1).ok_or("usage: commands <photo>")?;
    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
    let probed: Value = serde_json::from_str(&probe("photo.dng", &bytes)?).map_err(|e| e.to_string())?;
    let mut c = Commands::new();
    c.open(&probed["facts"].to_string(), &probed["defaults"].to_string())?;
    for (cmd, params) in [
        ("develop.set", r#"{"values":{"light.exposure":0.5,"wb.temp":5000}}"#),
        ("develop.wb", r#"{"mode":"cloudy"}"#),
        ("crop.set", r#"{"angle":5}"#),
        ("mask.add", r#"{"kind":"radial"}"#),
        ("develop.merge", r#"{"settings":{"treatment":"bw"}}"#),
    ] {
        let out = c.run(cmd, params)?;
        println!("{cmd}: {}", &out[..out.len().min(300)]);
    }
    Ok(())
}
