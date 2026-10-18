use filmlab_engine::core::{decode, probe};
use serde_json::Value;

fn main() -> Result<(), String> {
    let path = std::env::args().nth(1).ok_or("photo")?;
    let t = std::time::Instant::now();
    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
    let read = t.elapsed();
    let t = std::time::Instant::now();
    let p: Value = serde_json::from_str(&probe("p.dng", &bytes)?).map_err(|e| e.to_string())?;
    let probed = t.elapsed();
    for edge in [512usize, 2560, 0] {
        let t = std::time::Instant::now();
        filmlab::perf::interactive(|| decode(&p["facts"].to_string(), &bytes, edge))?;
        println!("decode edge {edge:>4}: {:.1} ms", t.elapsed().as_secs_f64() * 1000.0);
    }
    println!("read {:.1} ms, probe {:.1} ms", read.as_secs_f64() * 1000.0, probed.as_secs_f64() * 1000.0);
    Ok(())
}
