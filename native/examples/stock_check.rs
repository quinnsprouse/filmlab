use filmlab::gpu::{Gpu, Runner};
use filmlab_engine::core::{Engine, probe};
use serde_json::{Value, json};

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    let gpu = args.iter().any(|a| a == "--gpu");
    if gpu {
        filmlab_engine::install_gpu(Box::new(Runner(Gpu::open().ok_or("no gpu")?)));
    }
    let bytes = std::fs::read(&args[1]).map_err(|e| e.to_string())?;
    let p: Value = serde_json::from_str(&probe("p.dng", &bytes)?).map_err(|e| e.to_string())?;
    let mut e = Engine::new();
    e.load("k", &p["facts"].to_string(), &bytes, 512)?;
    for neg in ["Negative/Vision3 250D", "Negative/Vision3 Balanced Negative (High Density)", "Negative/Vision3 250D (High Density)"] {
        let text = std::fs::read_to_string(filmlab::stocks::folder().join(format!("{neg}.cube"))).map_err(|e| e.to_string())?;
        e.load_lut(neg, &text)?;
        let print = "Print/I- Standard/Print SA1 Kodak 2393";
        e.load_lut(print, &std::fs::read_to_string(filmlab::stocks::folder().join(format!("{print}.cube"))).map_err(|e| e.to_string())?)?;
        let film = json!({"negative": neg, "print": print, "utility": null, "halation": {"amount": 0.3, "radius": 5.0, "threshold": 0.55}, "grain": {"amount": 0.0995, "size": 1.0, "softness": 0.05, "defocus": 1.0}, "blur": 0.05, "dust": {"on": true, "size": 1.758, "softness": 0.245}});
        let f = e.render("k", &p["defaults"].to_string(), &json!({"maxW": 480, "maxH": 480, "film": film}).to_string())?;
        let n = (f.width * f.height) as f64;
        let mean: Vec<f64> = (0..3).map(|c| f.rgba.chunks(4).map(|px| f64::from(px[c])).sum::<f64>() / n).collect();
        println!("{neg}: {}x{} mean {:.1} {:.1} {:.1}", f.width, f.height, mean[0], mean[1], mean[2]);
    }
    Ok(())
}
