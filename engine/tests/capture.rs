use std::path::Path;

use filmlab_engine::core::{Engine, probe, read_sidecar};
use serde_json::{Value, json};

const IDENTITY_CUBE: &str = "LUT_3D_SIZE 2\n0 0 0\n1 0 0\n0 1 0\n1 1 0\n0 0 1\n1 0 1\n0 1 1\n1 1 1\n";

#[test]
fn owned_film_capture_matches_cached_capture_and_preserves_the_cache() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let raw = std::fs::read(dir.join(".cache/ricoh-gr3.dng")).expect("the fixture raw (run scripts/fixtures.sh)");
    let xmp = std::fs::read_to_string(dir.join("ricoh-gr3.xmp")).unwrap();
    let probed: Value = serde_json::from_str(&probe("photo.dng", &raw).unwrap()).unwrap();
    let facts = probed["facts"].to_string();
    let state: Value = serde_json::from_str(&read_sidecar(&facts, &xmp).unwrap()).unwrap();
    let mut settings = state["settings"].clone();
    settings["effects"]["clarity"] = json!(40.0);
    settings["effects"]["texture"] = json!(35.0);
    settings["effects"]["dehaze"] = json!(20.0);
    let mut engine = Engine::new();
    engine.load("photo", &facts, &raw, 1024).unwrap();
    engine.load_lut("identity", IDENTITY_CUBE).unwrap();
    for masked in [false, true] {
        settings["masks"] = if masked {
            json!([{
                "id": 1, "name": "Local tone", "visible": true, "invert": false,
                "components": [{"op": "add", "invert": false, "shape": {
                    "kind": "linear", "start": {"x": 0.2, "y": 0.3}, "end": {"x": 0.8, "y": 0.7}
                }}],
                "adjust": {"exposure": 0.4, "temp": 12.0, "texture": 25.0, "clarity": 15.0, "noise": 20.0, "moire": 15.0, "defringe": 30.0}
            }])
        } else {
            json!([])
        };
        for edge in [641, 1023] {
            let view = json!({"maxW": edge, "maxH": edge, "film": {"negative": "identity", "print": "identity"}});
            let mut cached = view.clone();
            cached["stages"] = json!("reference");
            for exposure in [0.0, 0.75, 0.0] {
                settings["light"]["exposure"] = json!(exposure);
                let settings = settings.to_string();
                let reference = engine.render("photo", &settings, &cached.to_string()).unwrap();
                let owned = engine.render("photo", &settings, &view.to_string()).unwrap();
                assert_eq!((owned.width, owned.height), (reference.width, reference.height));
                assert_eq!(owned.width.max(owned.height), edge);
                assert!(owned.rgba == reference.rgba, "edge {edge}, masked {masked}, exposure {exposure}");
                let retained = engine.render("photo", &settings, &cached.to_string()).unwrap();
                assert!(retained.rgba == reference.rgba, "the cached source was modified");
            }
        }
    }
}
