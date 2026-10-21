use std::path::Path;
use std::sync::Arc;

use filmlab_engine::core::{Engine, probe, read_sidecar};
use lightcraft_engine::pipeline::film_hook::{Pixel, ToneHook, with_tone_hook};
use serde_json::{Value, json};

/// The hook a film render's capture pass installs on its thread: it records the scene and makes no pixels.
struct Capturing;

impl ToneHook for Capturing {
    fn tone(&self, scene: [f32; 3], _at: Pixel) -> [f32; 3] {
        scene
    }

    fn capture_only(&self) -> bool {
        true
    }
}

fn photo() -> (Vec<u8>, String, String) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let raw = std::fs::read(dir.join(".cache/ricoh-gr3.dng")).expect("the fixture raw (run scripts/fixtures.sh)");
    let xmp = std::fs::read_to_string(dir.join("ricoh-gr3.xmp")).expect("the fixture sidecar");
    let probed: Value = serde_json::from_str(&probe("R0000001.DNG", &raw).unwrap()).unwrap();
    let facts = probed["facts"].to_string();
    let state: Value = serde_json::from_str(&read_sidecar(&facts, &xmp).unwrap()).unwrap();
    (raw, facts, state.to_string())
}

// why: a render pool thread waiting inside one render's film capture can pick up another render whole (rayon
// helping), so a render runs on a thread that already holds someone else's film hook
#[test]
fn a_render_ignores_a_film_hook_left_on_its_thread() {
    let (raw, facts, state) = photo();
    let settings = serde_json::from_str::<Value>(&state).unwrap()["settings"].to_string();
    let mut engine = Engine::new();
    engine.load("photo", &facts, &raw, 640).unwrap();
    let view = json!({"maxW": 320, "maxH": 320}).to_string();

    let alone = engine.render("photo", &settings, &view).unwrap();
    let mut nested = with_tone_hook(Arc::new(Capturing), || engine.render("photo", &settings, &view)).unwrap();

    assert_eq!((nested.width(), nested.height()), (alone.width(), alone.height()));
    assert_eq!(nested.take_rgba().len(), alone.width() as usize * alone.height() as usize * 4, "the render inherited another render's capture and came back without pixels");
}

#[test]
fn an_export_ignores_a_film_hook_left_on_its_thread() {
    let (raw, facts, state) = photo();
    let options = json!({"format": "png", "resize": {"mode": "longEdge", "value": 320}}).to_string();
    let mut engine = Engine::new();
    engine.load("photo", &facts, &raw, 640).unwrap();

    let alone = engine.export("photo", &facts, &state, &options, "null").unwrap();
    let nested = with_tone_hook(Arc::new(Capturing), || engine.export("photo", &facts, &state, &options, "null")).unwrap();

    assert!(nested == alone, "the export inherited another render's capture");
}
