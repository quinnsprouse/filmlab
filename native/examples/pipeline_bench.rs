use std::time::Instant;

use filmlab_engine::core::{Engine, probe};
use filmlab::gpu::{Gpu, Runner};
use serde_json::{Value, json};

fn main() -> Result<(), String> {
    let mut args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--gpu") {
        args.retain(|a| a != "--gpu");
        let gpu = Gpu::open().ok_or("no GPU")?;
        filmlab_engine::install_gpu(Box::new(Runner(gpu)));
        eprintln!("film stage on the GPU");
    }
    let [_, photo, negative, print] = args.as_slice() else {
        return Err("usage: pipeline_bench <photo> <negative.cube> <print.cube>".into());
    };
    let bytes = std::fs::read(photo).map_err(|e| e.to_string())?;
    let probed: Value = serde_json::from_str(&probe("photo.dng", &bytes)?).map_err(|e| e.to_string())?;
    let facts = probed["facts"].to_string();
    let defaults = probed["defaults"].clone();
    let rounds = 3;
    let mut best: Vec<(String, f64, String)> = Vec::new();
    for round in 0..rounds {
    let mut engine = Engine::new();
    engine.load("k", &facts, &bytes, 0)?;
    engine.load_lut("n", &std::fs::read_to_string(negative).map_err(|e| e.to_string())?)?;
    engine.load_lut("p", &std::fs::read_to_string(print).map_err(|e| e.to_string())?)?;
    let mut step = 0usize;
    let film = json!({"negative": "n", "print": "p", "grain": {"amount": 0.1495, "size": 2.0, "softness": 0.05, "defocus": 0.9}, "blur": 0.05, "dust": {"on": true, "size": 1.758, "softness": 0.245}});
    let mut settings = defaults.clone();
    let mut film = film;
    let mut run = |label: &str, settings: &Value, view: Value| -> Result<(), String> {
        let t = Instant::now();
        let f = engine.render("k", &settings.to_string(), &view.to_string())?;
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        let hash = f.rgba.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x100_0000_01b3));
        let line = format!("{}x{} {hash:016x}", f.width, f.height);
        if round == 0 {
            best.push((label.to_string(), ms, line));
        } else {
            best[step].1 = best[step].1.min(ms);
        }
        step += 1;
        Ok(())
    };
    let plain = json!({"maxW": 2400, "maxH": 1600, "stages": "plain"});
    run("plain first", &settings, plain.clone())?;
    for v in [0.1, 0.2] {
        settings["light"]["exposure"] = json!(v);
        run(&format!("plain exposure {v}"), &settings, plain.clone())?;
    }
    for v in [10.0, 20.0] {
        settings["color"]["vibrance"] = json!(v);
        run(&format!("plain vibrance {v}"), &settings, plain.clone())?;
    }
    let mut settings = defaults.clone();
    let fit = |film: &Value| json!({"maxW": 2400, "maxH": 1600, "stages": "fit", "film": film});
    run("film first", &settings, fit(&film))?;
    run("film again", &settings, fit(&film))?;
    for v in [0.1, 0.2, 0.3] {
        settings["light"]["exposure"] = json!(v);
        run(&format!("film exposure {v}"), &settings, fit(&film))?;
    }
    for v in [10.0, 20.0, 30.0] {
        settings["color"]["vibrance"] = json!(v);
        run(&format!("film vibrance {v}"), &settings, fit(&film))?;
    }
    for v in [0.03, 0.04] {
        film["grain"]["amount"] = json!(v);
        run(&format!("film grain {v}"), &settings, fit(&film))?;
    }
    for v in [0.0, 0.4, 0.5] {
        film["halation"]["amount"] = json!(v);
        run(&format!("film halation {v}"), &settings, fit(&film))?;
    }
    for v in [10.0, 20.0] {
        settings["light"]["contrast"] = json!(v);
        run(&format!("film contrast {v}"), &settings, fit(&film))?;
    }
    let zoom = |film: &Value| json!({"maxW": 6000, "maxH": 4000, "stages": "zoom", "window": [3800, 1800, 2000, 1600], "film": film});
    run("zoom first", &settings, zoom(&film))?;
    settings["light"]["exposure"] = json!(0.15);
    run("zoom exposure", &settings, zoom(&film))?;
    film["grain"]["amount"] = json!(0.25);
    run("zoom grain", &settings, zoom(&film))?;
    }
    for (label, ms, line) in best {
        println!("{label:>22}: {ms:6.1} ms {line}");
    }
    Ok(())
}
