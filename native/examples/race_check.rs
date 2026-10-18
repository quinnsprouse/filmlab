use std::sync::Arc;
use std::thread;

use filmlab::film::{FilmSettings, render_film};
use filmlab_engine::core::{Engine, probe, read_sidecar};
use serde_json::{Value, json};

fn prepared(path: &str, xmp: Option<&str>, edge: usize) -> Result<(Engine, Value, Option<FilmSettings>), String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let p: Value = serde_json::from_str(&probe("p.dng", &bytes)?).map_err(|e| e.to_string())?;
    let (settings, film) = match xmp {
        Some(x) => {
            let state: Value = serde_json::from_str(&read_sidecar(&p["facts"].to_string(), &std::fs::read_to_string(x).map_err(|e| e.to_string())?)?).map_err(|e| e.to_string())?;
            (state["settings"].clone(), render_film(FilmSettings::of(&state["film"]).as_ref()))
        }
        None => (p["defaults"].clone(), render_film(Some(&FilmSettings { negative: Some("Negative/Vision3 250D".into()), print: Some("Print/I- Standard/Print SA1 Kodak 2393".into()), ..Default::default() }))),
    };
    let mut e = Engine::new();
    e.load("k", &p["facts"].to_string(), &bytes, edge)?;
    filmlab::library::ensure_luts(&mut e, film.as_ref())?;
    Ok((e, settings, film))
}

fn hash(rgba: &[u8]) -> u64 {
    rgba.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x100_0000_01b3))
}

fn main() -> Result<(), String> {
    let mut args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--gpu") {
        args.retain(|a| a != "--gpu");
        filmlab_engine::install_gpu(Box::new(filmlab::gpu::Runner(filmlab::gpu::Gpu::open().ok_or("no gpu")?)));
    }
    let dir = Arc::new(args[1].clone());
    let main = format!("{dir}/R0016851.DNG");
    let (mut e, settings, film) = prepared(&main, Some(&format!("{dir}/R0016851.xmp")), 2560)?;
    let view = |f: &Option<FilmSettings>| {
        let mut v = json!({"maxW": 1800, "maxH": 1534, "stages": "loupe"});
        if let Some(f) = f {
            v["film"] = f.to_value();
        }
        v.to_string()
    };
    let expected = hash(&e.render("k", &settings.to_string(), &view(&film))?.rgba);
    let others: Vec<_> = (0..3)
        .map(|t| {
            let dir = dir.clone();
            thread::spawn(move || -> Result<(), String> {
                for name in ["R0016812", "R0016829", "R0016852", "R0016854", "R0016857", "R0016858"].iter().cycle().skip(t).take(8) {
                    let (mut e, s, f) = prepared(&format!("{dir}/{name}.DNG"), None, 512)?;
                    let mut v = json!({"maxW": 480, "maxH": 480});
                    if let Some(f) = &f {
                        v["film"] = f.to_value();
                    }
                    e.render("k", &s.to_string(), &v.to_string())?;
                }
                Ok(())
            })
        })
        .collect();
    let mut bad = 0;
    for i in 0..12 {
        let mut s = settings.clone();
        s["light"]["exposure"] = json!(0.01 * f64::from(i % 2));
        let f = e.render("k", &s.to_string(), &view(&film))?;
        let h = hash(&f.rgba);
        if i % 2 == 0 && h != expected {
            bad += 1;
            let n = (f.width * f.height) as f64;
            println!("round {i}: MISMATCH mean {:.1}", f.rgba.chunks(4).map(|p| f64::from(p[1])).sum::<f64>() / n);
        }
    }
    for t in others {
        t.join().map_err(|_| "thread panicked")??;
    }
    println!("mismatches: {bad}");
    Ok(())
}
