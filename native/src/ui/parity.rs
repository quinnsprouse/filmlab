use std::path::{Path, PathBuf};

use filmlab_engine::core::{self, Engine};
use serde_json::{Value, json};

use crate::film::{Dust, FilmSettings, Gauge, Grain, Halation, render_film};
use crate::library::ensure_luts;
use crate::stocks::{DEFAULT_NEGATIVE, DEFAULT_PRINT};

const OTHER_NEGATIVE: &str = "Negative/Vision3 500T (High Density)";
const OTHER_PRINT: &str = "Print/I- Standard/Print SD1 Kodak 2383";

/// One parity check: its name, whether it passed and what it measured.
pub type Outcome = (String, bool, String);

// why: FILMLAB_CPU=1 keeps the film stage off the GPU for the whole process, so the two paths are compared across
// two runs: the GPU run renders these edits into FILMLAB_E2E_PARITY and the CPU run renders them again and compares
fn folder() -> Option<PathBuf> {
    std::env::var_os("FILMLAB_E2E_PARITY").map(PathBuf::from)
}

fn film() -> FilmSettings {
    let gauge = Gauge::G35;
    FilmSettings {
        negative: Some(DEFAULT_NEGATIVE.into()),
        print: Some(DEFAULT_PRINT.into()),
        utility: None,
        halation: Halation::stock(gauge),
        grain: gauge.grain(),
        blur: gauge.blur(),
        dust: Some(Dust { on: true, size: 1.758, softness: 0.245 }),
    }
}

fn renders(photo: &Path) -> Result<Vec<(&'static str, Vec<u8>)>, String> {
    let bytes = std::fs::read(photo).map_err(|e| e.to_string())?;
    let probed: Value = serde_json::from_str(&core::probe(&photo.to_string_lossy(), &bytes)?).map_err(|e| e.to_string())?;
    let facts = probed["facts"].to_string();
    let mut engine = Engine::new();
    engine.load("parity", &facts, &bytes, 0)?;
    let other = render_film(Some(&FilmSettings { negative: Some(OTHER_NEGATIVE.into()), print: Some(OTHER_PRINT.into()), ..film() }));
    ensure_luts(&mut engine, other.as_ref())?;
    let film = render_film(Some(&film()));
    ensure_luts(&mut engine, film.as_ref())?;
    let cine = film.clone().map(|f| FilmSettings { grain: Gauge::G35.cine_grain(), blur: Gauge::G35.cine_blur(), ..f });
    let strong = film.clone().map(|f| FilmSettings { grain: Grain { amount: 0.6, color: 0.9, softness: 0.3, defocus: 0.9, ..f.grain }, ..f });
    let regrained = film.clone().map(|f| FilmSettings { grain: Grain { amount: 2.0 * f.grain.amount, ..f.grain }, ..f });
    let [film, cine, strong, other, regrained] = [film, cine, strong, other, regrained].map(|f| f.as_ref().map(FilmSettings::to_value));
    let mut settings = probed["defaults"].clone();
    settings["light"]["exposure"] = json!(0.35);
    settings["light"]["contrast"] = json!(20.0);
    settings["color"]["vibrance"] = json!(15.0);
    let settings = settings.to_string();
    let size: Value = serde_json::from_str(&core::native_size(&facts, &settings)?).map_err(|e| e.to_string())?;
    let (w, h) = (size[0].as_u64().unwrap_or(1) as usize, size[1].as_u64().unwrap_or(1) as usize);
    let views = [
        ("fit", json!({"maxW": 1600, "maxH": 1600, "stages": "loupe", "applyCrop": true, "film": film})),
        ("window", json!({"maxW": w, "maxH": h, "stages": "loupe-zoom", "applyCrop": true, "window": [w / 3, h / 3, 1024.min(w / 2), 768.min(h / 2)], "film": film})),
        ("cine window", json!({"maxW": w, "maxH": h, "stages": "loupe-zoom", "applyCrop": true, "window": [w / 3, h / 3, 1024.min(w / 2), 768.min(h / 2)], "film": cine})),
        ("strong stills window", json!({"maxW": w, "maxH": h, "stages": "loupe-zoom", "applyCrop": true, "window": [w / 3, h / 3, 1024.min(w / 2), 768.min(h / 2)], "film": strong})),
        ("other film 1", json!({"maxW": 400, "maxH": 400, "stages": "other 1", "applyCrop": true, "film": other})),
        ("other film 2", json!({"maxW": 500, "maxH": 500, "stages": "other 2", "applyCrop": true, "film": other})),
        ("other film 3", json!({"maxW": 600, "maxH": 600, "stages": "other 3", "applyCrop": true, "film": other})),
        ("other film 4", json!({"maxW": 700, "maxH": 700, "stages": "other 4", "applyCrop": true, "film": other})),
        ("regrained fit", json!({"maxW": 1600, "maxH": 1600, "stages": "loupe", "applyCrop": true, "film": regrained})),
        ("fit after it", json!({"maxW": 1200, "maxH": 1200, "stages": "after", "applyCrop": true, "film": film})),
        ("strong stills fit", json!({"maxW": 1600, "maxH": 1600, "stages": "loupe", "applyCrop": true, "film": strong})),
        ("strong stills small fit", json!({"maxW": 512, "maxH": 512, "stages": "small", "applyCrop": true, "film": strong})),
    ];
    views.into_iter().map(|(name, view)| Ok((name, engine.render("parity", &settings, &view.to_string())?.take_rgba()))).collect()
}

/// In the GPU run: renders the edits on the GPU and keeps them for the CPU run.
pub fn record(photo: &Path) -> Vec<Outcome> {
    let Some(dir) = folder() else { return Vec::new() };
    let gpu = filmlab_engine::gpu::installed();
    let kept = std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(dir.join("photo"), photo.to_string_lossy().as_bytes())).is_ok();
    let saved = renders(photo).map(|r| r.iter().all(|(name, rgba)| std::fs::write(dir.join(format!("{name}.rgba")), rgba).is_ok()));
    vec![("parity: the film stage renders the edits on the GPU".into(), gpu && kept && saved == Ok(true), format!("gpu {gpu}, saved {saved:?}"))]
}

/// In the CPU run: renders the same edits on the CPU and compares them with the GPU's pixels.
pub fn compare() -> Vec<Outcome> {
    let Some(dir) = folder() else { return vec![("parity: FILMLAB_E2E_PARITY names the GPU run's folder".into(), false, String::new())] };
    let on_cpu = !filmlab_engine::gpu::installed();
    let photo = std::fs::read_to_string(dir.join("photo")).map(PathBuf::from);
    let rendered = photo.map_err(|e| e.to_string()).and_then(|p| renders(&p));
    let Ok(rendered) = rendered else { return vec![("parity: the CPU renders the edits".into(), false, format!("{:?}", rendered.err()))] };
    rendered
        .into_iter()
        .map(|(name, cpu)| {
            let gpu = std::fs::read(dir.join(format!("{name}.rgba"))).unwrap_or_default();
            let differ = gpu.iter().zip(&cpu).filter(|(a, b)| a != b).count();
            let worst = gpu.iter().zip(&cpu).map(|(a, b)| a.abs_diff(*b)).max().unwrap_or(0);
            let mean = cpu.iter().map(|b| f64::from(*b)).sum::<f64>() / cpu.len().max(1) as f64 / 255.0;
            let ok = on_cpu && gpu.len() == cpu.len() && differ == 0 && (0.02..0.98).contains(&mean);
            (format!("parity: the {name} render is identical on the GPU and the CPU"), ok, format!("{} bytes, {differ} differ, worst by {worst}, mean {mean:.3}, cpu only {on_cpu}", cpu.len()))
        })
        .collect()
}
