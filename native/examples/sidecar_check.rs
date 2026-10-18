use filmlab::film::{FilmSettings, render_film};
use filmlab_engine::core::{Engine, probe, read_sidecar};
use serde_json::{Value, json};

fn main() -> Result<(), String> {
    let mut args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--gpu") {
        args.retain(|a| a != "--gpu");
        filmlab_engine::install_gpu(Box::new(filmlab::gpu::Runner(filmlab::gpu::Gpu::open().ok_or("no gpu")?)));
    }
    let (w, h, edge): (usize, usize, usize) = (args[3].parse().unwrap_or(480), args[4].parse().unwrap_or(480), args[5].parse().unwrap_or(512));
    let bytes = std::fs::read(&args[1]).map_err(|e| e.to_string())?;
    let xmp = std::fs::read_to_string(&args[2]).map_err(|e| e.to_string())?;
    let p: Value = serde_json::from_str(&probe("p.dng", &bytes)?).map_err(|e| e.to_string())?;
    let state: Value = serde_json::from_str(&read_sidecar(&p["facts"].to_string(), &xmp)?).map_err(|e| e.to_string())?;
    let film = render_film(FilmSettings::of(&state["film"]).as_ref());
    println!("film {:?}", film);
    println!("light {}", state["settings"]["light"]);
    let mut e = Engine::new();
    e.load("k", &p["facts"].to_string(), &bytes, edge)?;
    filmlab::library::ensure_luts(&mut e, film.as_ref())?;
    let mut view = json!({"maxW": w, "maxH": h, "stages": "loupe"});
    if let Some(f) = &film {
        view["film"] = f.to_value();
    }
    let f = e.render("k", &state["settings"].to_string(), &view.to_string())?;
    let n = (f.width * f.height) as f64;
    let mean: Vec<f64> = (0..3).map(|c| f.rgba.chunks(4).map(|px| f64::from(px[c])).sum::<f64>() / n).collect();
    let hash = f.rgba.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x100_0000_01b3));
    println!("{}x{} mean {:.1} {:.1} {:.1} {hash:016x}", f.width, f.height, mean[0], mean[1], mean[2]);
    Ok(())
}
