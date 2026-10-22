use std::path::Path;

use filmlab_engine::core::{Engine, probe, read_sidecar};
use serde_json::{Value, json};

const IDENTITY_CUBE: &str = "LUT_3D_SIZE 2\n0 0 0\n1 0 0\n0 1 0\n1 1 0\n0 0 1\n1 0 1\n0 1 1\n1 1 1\n";
const EDGE: usize = 1600;

fn photo() -> (Vec<u8>, String, Value) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let raw = std::fs::read(dir.join(".cache/ricoh-gr3.dng")).expect("the fixture raw (run scripts/fixtures.sh)");
    let xmp = std::fs::read_to_string(dir.join("ricoh-gr3.xmp")).expect("the fixture sidecar");
    let probed: Value = serde_json::from_str(&probe("R0000001.DNG", &raw).unwrap()).unwrap();
    let facts = probed["facts"].to_string();
    let state: Value = serde_json::from_str(&read_sidecar(&facts, &xmp).unwrap()).unwrap();
    (raw, facts, state["settings"].clone())
}

/// The fixture's edit with what a zoomed window has to place right: masks (one reading the scene's luminance), a
/// rotated crop, a heal spot (which widens the rendered window), a vignette and grain over the whole frame.
fn retouched(settings: &Value) -> Value {
    let mut s = settings.clone();
    s["masks"] = json!([
        {"id": 1, "name": "Sky", "visible": true, "invert": false, "components": [
            {"op": "add", "invert": false, "shape": {"kind": "radial", "center": {"x": 0.45, "y": 0.4}, "rx": 0.2, "ry": 0.15, "angle": 20.0, "feather": 60.0, "invert": false}},
            {"op": "intersect", "invert": false, "shape": {"kind": "luminanceRange", "lo": 0.4, "hi": 1.0, "lo_feather": 0.1, "hi_feather": 0.1}}
        ], "adjust": {"exposure": -0.4, "saturation": 25.0, "temp": 8.0}},
        {"id": 2, "name": "Ground", "visible": true, "invert": false, "components": [
            {"op": "add", "invert": false, "shape": {"kind": "linear", "start": {"x": 0.5, "y": 0.6}, "end": {"x": 0.5, "y": 0.9}}}
        ], "adjust": {"exposure": 0.3, "contrast": 20.0}}
    ]);
    s["crop"]["geometry"] = json!({"rect": {"x0": 0.05, "y0": 0.08, "x1": 0.93, "y1": 0.95}, "angle": 3.5});
    s["spots"] = json!([{"mode": "heal", "points": [{"x": 0.5, "y": 0.5}], "size": 0.02, "feather": 50.0, "opacity": 100.0, "source_offset": {"x": 0.06, "y": 0.02}}]);
    s["vignette"]["amount"] = json!(-30.0);
    s["grain"]["amount"] = json!(25.0);
    s
}

/// A window of the frame at `EDGE` with film; `overlay` adds one that draws nothing (its mask isn't there), which
/// keeps the final pass on the whole captured window, cut down afterwards.
fn zoomed(window: [usize; 4], overlay: bool) -> Value {
    let mut view = json!({
        "maxW": EDGE, "maxH": EDGE, "stages": "loupe-zoom", "window": window,
        "film": {"negative": "identity", "print": "identity"},
    });
    if overlay {
        view["overlay"] = json!({"kind": "mask", "id": 99, "color": [255, 0, 0], "opacity": 50});
    }
    view
}

#[test]
fn a_zoomed_film_window_finished_alone_matches_the_window_cut_from_its_margin() {
    let (raw, facts, settings) = photo();
    let settings = retouched(&settings).to_string();
    let mut engine = Engine::new();
    engine.load("photo", &facts, &raw, EDGE).unwrap();
    engine.load_lut("identity", IDENTITY_CUBE).unwrap();
    let windows = [[420, 300, 520, 360], [0, 0, 400, 300], [600, 380, 192, 360], [900, 650, 300, 250]];
    for window in windows {
        let alone = engine.render("photo", &settings, &zoomed(window, false).to_string()).unwrap();
        let cut = engine.render("photo", &settings, &zoomed(window, true).to_string()).unwrap();
        assert_eq!((alone.width, alone.height), (cut.width, cut.height), "window {window:?}: sizes differ");
        assert!(alone.rgba == cut.rgba, "window {window:?}: the pixels differ");
    }
}
