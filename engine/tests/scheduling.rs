use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use filmlab_engine::core::{Engine, Wanted, probe, read_sidecar};
use serde_json::{Value, json};

const IDENTITY_CUBE: &str = "LUT_3D_SIZE 2\n0 0 0\n1 0 0\n0 1 0\n1 1 0\n0 0 1\n1 0 1\n0 1 1\n1 1 1\n";

fn photo() -> (Vec<u8>, String, Value) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let raw = std::fs::read(dir.join(".cache/ricoh-gr3.dng")).expect("the fixture raw (run scripts/fixtures.sh)");
    let xmp = std::fs::read_to_string(dir.join("ricoh-gr3.xmp")).expect("the fixture sidecar");
    let probed: Value = serde_json::from_str(&probe("R0000001.DNG", &raw).unwrap()).unwrap();
    let facts = probed["facts"].to_string();
    let state: Value = serde_json::from_str(&read_sidecar(&facts, &xmp).unwrap()).unwrap();
    (raw, facts, state["settings"].clone())
}

fn engine_with(raw: &[u8], facts: &str) -> Engine {
    let mut engine = Engine::new();
    engine.load("photo", facts, raw, 640).unwrap();
    engine.load_lut("identity", IDENTITY_CUBE).unwrap();
    engine
}

fn film() -> Value {
    json!({"negative": "identity", "print": "identity"})
}

fn fit(stages: &str, film: Option<Value>) -> Value {
    let mut view = json!({"maxW": 400, "maxH": 400, "stages": stages});
    if let Some(film) = film {
        view["film"] = film;
    }
    view
}

fn window(film: Option<Value>) -> Value {
    let mut view = json!({"maxW": 640, "maxH": 640, "stages": "loupe-zoom", "window": [120, 80, 300, 200]});
    if let Some(film) = film {
        view["film"] = film;
    }
    view
}

fn exposed(settings: &Value, ev: f64) -> Value {
    let mut s = settings.clone();
    s["light"]["exposure"] = json!(ev);
    s
}

fn pixels(engine: &mut Engine, settings: &Value, view: &Value) -> Vec<u8> {
    engine.render("photo", &settings.to_string(), &view.to_string()).unwrap().take_rgba()
}

fn wanted_for(checks: usize) -> Wanted {
    let asked = Arc::new(AtomicUsize::new(0));
    Arc::new(move || asked.fetch_add(1, Ordering::Relaxed) < checks)
}

fn stops_cleanly(warm: bool, view: Value) {
    let (raw, facts, settings) = photo();
    let next = exposed(&settings, 0.4);
    let expected = pixels(&mut engine_with(&raw, &facts), &next, &view);
    let mut stopped_somewhere = false;
    for checks in 0.. {
        let mut engine = engine_with(&raw, &facts);
        if warm {
            pixels(&mut engine, &settings, &view);
        }
        let frame = engine.render_while("photo", &next.to_string(), &view.to_string(), Some(wanted_for(checks))).unwrap();
        let after = pixels(&mut engine, &next, &view);
        assert!(after == expected, "the render after one stopped at check {checks} differs from a fresh render (view {view})");
        match frame {
            Some(mut frame) => {
                assert!(frame.take_rgba() == expected, "a render let run to the end differs from a fresh render");
                break;
            }
            None => stopped_somewhere = true,
        }
    }
    assert!(stopped_somewhere, "the render never stopped");
}

#[test]
fn a_render_stopped_at_any_stage_leaves_the_next_render_unchanged() {
    stops_cleanly(false, fit("loupe", None));
    stops_cleanly(true, fit("loupe", None));
}

#[test]
fn a_film_render_stopped_at_any_stage_leaves_the_next_render_unchanged() {
    stops_cleanly(false, fit("loupe", Some(film())));
    stops_cleanly(true, fit("loupe", Some(film())));
}

#[test]
fn a_zoomed_film_render_stopped_at_any_stage_leaves_the_next_render_unchanged() {
    stops_cleanly(false, window(Some(film())));
    stops_cleanly(true, window(Some(film())));
}

#[test]
fn a_render_nobody_stops_matches_a_plain_render() {
    let (raw, facts, settings) = photo();
    let view = fit("loupe", Some(film()));
    let plain = pixels(&mut engine_with(&raw, &facts), &settings, &view);
    let always: Wanted = Arc::new(|| true);
    let mut frame = engine_with(&raw, &facts).render_while("photo", &settings.to_string(), &view.to_string(), Some(always)).unwrap().expect("a wanted render finishes");
    assert!(frame.take_rgba() == plain);
}
