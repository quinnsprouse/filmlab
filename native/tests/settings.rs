use std::path::{Path, PathBuf};

use filmlab::film::FilmSettings;
use filmlab::library::PhotoState;
use filmlab::mcp::session::digest;
use filmlab::settings::Settings;
use filmlab::stocks::{DEFAULT_NEGATIVE, DEFAULT_PRINT};
use filmlab_engine::core::{Commands, probe, read_sidecar, write_sidecar};
use serde_json::{Value, json};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../engine/tests/fixtures").join(name)
}

fn read(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e} (run scripts/fixtures.sh)", path.display()))
}

struct Photo {
    raw: Vec<u8>,
    facts: Value,
    xmp: String,
}

fn photo() -> Photo {
    let raw = read(&fixture(".cache/ricoh-gr3.dng"));
    let probed: Value = serde_json::from_str(&probe("R0000001.DNG", &raw).unwrap()).unwrap();
    let xmp = String::from_utf8(read(&fixture("ricoh-gr3.xmp"))).unwrap();
    Photo { raw, facts: probed["facts"].clone(), xmp }
}

/// The state as the app keeps it (typed) and as it kept it before (JSON) give the same text, the same sidecar
/// written over `existing`, and the same state read back from that sidecar.
fn same_state(photo: &Photo, text: &str, existing: Option<&str>) -> String {
    let typed: PhotoState = serde_json::from_str(text).unwrap();
    let raw: Value = serde_json::from_str(text).unwrap();
    let typed_text = serde_json::to_string(&typed).unwrap();
    assert_eq!(typed_text, raw.to_string());
    assert_eq!(typed.settings.to_value(), raw["settings"]);
    assert_eq!(typed.settings.json(), raw["settings"].to_string());
    let facts = photo.facts.to_string();
    let written = write_sidecar(&facts, &typed_text, existing.map(str::to_string)).unwrap();
    assert_eq!(written, write_sidecar(&facts, &raw.to_string(), existing.map(str::to_string)).unwrap());
    let again = read_sidecar(&facts, &written).unwrap();
    let back: PhotoState = serde_json::from_str(&again).unwrap();
    assert_eq!(back.settings, typed.settings);
    assert_eq!(serde_json::to_string(&back).unwrap(), serde_json::from_str::<Value>(&again).unwrap().to_string());
    written
}

#[test]
fn sidecars_round_trip_unchanged() {
    let photo = photo();
    let state = read_sidecar(&photo.facts.to_string(), &photo.xmp).unwrap();
    same_state(&photo, &state, None);
    same_state(&photo, &state, Some(&photo.xmp));

    let mut filmed: Value = serde_json::from_str(&state).unwrap();
    let film = FilmSettings { negative: Some(DEFAULT_NEGATIVE.into()), print: Some(DEFAULT_PRINT.into()), ..FilmSettings::default() };
    filmed["film"] = film.normalized().to_value();
    filmed["rating"] = json!(4);
    filmed["label"] = json!("red");
    let written = same_state(&photo, &filmed.to_string(), Some(&photo.xmp));
    let read_back = read_sidecar(&photo.facts.to_string(), &written).unwrap();
    same_state(&photo, &read_back, Some(&written));
}

/// Edits of every kind the app and agents make, as LightCraft commands.
fn edits() -> Vec<(&'static str, Value)> {
    vec![
        ("develop.set", json!({"values": {"light.exposure": 0.37, "light.contrast": -12.5, "color.vibrance": 18.0, "wb.temp": 5120.0}})),
        ("develop.curve", json!({"channel": "master", "points": [[0.0, 0.0], [0.31, 0.24], [0.72, 0.81], [1.0, 1.0]]})),
        ("develop.curve", json!({"channel": "red", "points": [[0.0, 0.03], [0.5, 0.52], [1.0, 0.97]]})),
        ("develop.merge", json!({"settings": {"optics": {"lens_profile": true}, "treatment": "bw", "vignette": {"style": "colorPriority"}}, "label": "merge"})),
        ("develop.wb", json!({"mode": "daylight"})),
        ("crop.aspect", json!({"aspect": "4x5"})),
        ("develop.set", json!({"values": {"crop.angle": 3.25}})),
        ("spot.add", json!({"mode": "heal", "points": [[0.21, 0.33], [0.23, 0.36]], "size": 0.012, "feather": 40.0, "opacity": 90.0})),
        ("spot.add", json!({"mode": "remove", "points": [[0.61, 0.44]], "size": 0.02, "feather": 50.0, "opacity": 100.0})),
        ("mask.add", json!({"kind": "radial"})),
        ("mask.add", json!({"kind": "linear"})),
        ("mask.brushStroke", json!({"points": [[0.4, 0.4], [0.45, 0.42], [0.5, 0.47]], "size": 0.05, "feather": 60.0, "flow": 80.0, "erase": false, "autoMask": false, "connect": false})),
    ]
}

#[test]
fn command_settings_match_their_json() {
    let photo = photo();
    let facts = photo.facts.to_string();
    let opened: Value = serde_json::from_str(&read_sidecar(&facts, &photo.xmp).unwrap()).unwrap();
    let (mut typed, mut text) = (Commands::new(), Commands::new());
    let facts_typed = serde_json::from_str(&facts).unwrap();
    typed.open_settings(&facts_typed, serde_json::from_value(opened["settings"].clone()).unwrap()).unwrap();
    text.open(&facts, &opened["settings"].to_string()).unwrap();
    typed.attach(photo.raw.clone());
    text.attach(photo.raw.clone());
    let film = FilmSettings { negative: Some(DEFAULT_NEGATIVE.into()), ..FilmSettings::default() };
    let mut last = None;
    for (command, params) in edits() {
        let (result, settings) = typed.run_settings(command, &params).unwrap_or_else(|e| panic!("{command}: {e}"));
        let out: Value = serde_json::from_str(&text.run(command, &params.to_string()).unwrap()).unwrap();
        let settings = Settings::from(settings);
        assert_eq!(result, out["result"], "{command}");
        assert_eq!(settings.json(), out["settings"].to_string(), "{command}");
        assert_eq!(settings.to_value(), out["settings"], "{command}");
        assert_eq!(Settings::parse(&out["settings"]).unwrap(), settings, "{command}");
        assert_eq!(digest("R0000001.DNG", &settings.to_value(), Some(&film)), digest("R0000001.DNG", &out["settings"], Some(&film)), "{command}");
        let state = json!({"settings": out["settings"], "rating": 0, "flag": "none", "label": null});
        same_state(&photo, &state.to_string(), Some(&photo.xmp));
        last = Some(settings);
    }
    let edited = last.unwrap();
    assert_eq!((edited.spots.len(), edited.masks.len(), edited.curve.master.len()), (2, 2, 4));
    assert!(edited.crop.aspect.is_some() && edited.optics.lens_profile);
}

#[test]
fn unreadable_settings_fail_as_the_engine_says() {
    let photo = photo();
    let bad = json!({"light": {"exposure": "bright"}});
    let ours = Settings::parse(&bad).unwrap_err();
    let engine = Commands::new().open(&photo.facts.to_string(), &bad.to_string()).unwrap_err();
    assert_eq!(ours, engine);
}
