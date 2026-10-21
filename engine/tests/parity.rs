use std::io::Cursor;
use std::path::{Path, PathBuf};

use filmlab_engine::core::{Engine, export_plan, probe, read_sidecar};
use serde_json::{Value, json};

struct Rgb {
    size: (u32, u32),
    values: Vec<u8>,
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)
}

fn read(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e} (run scripts/fixtures.sh)", path.display()))
}

fn rgb(png_bytes: &[u8]) -> Rgb {
    let mut decoder = png::Decoder::new(Cursor::new(png_bytes));
    decoder.set_transformations(png::Transformations::EXPAND);
    let mut reader = decoder.read_info().expect("a PNG header");
    let mut buffer = vec![0; reader.output_buffer_size().expect("a bounded PNG")];
    let frame = reader.next_frame(&mut buffer).expect("a PNG frame");
    assert_eq!(frame.bit_depth, png::BitDepth::Eight, "8-bit PNG");
    let channels = frame.color_type.samples();
    let values = buffer[..frame.buffer_size()].chunks_exact(channels).flat_map(|px| px[..3].to_vec()).collect();
    Rgb { size: (frame.width, frame.height), values }
}

const IDENTITY_CUBE: &str = "LUT_3D_SIZE 2\n0 0 0\n1 0 0\n0 1 0\n1 1 0\n0 0 1\n1 0 1\n0 1 1\n1 1 1\n";

fn export_png(raw: &[u8], xmp: &str, long_edge: u32) -> Vec<u8> {
    export_edited_png(raw, xmp, long_edge, |_| (), None)
}

fn export_edited_png(raw: &[u8], xmp: &str, long_edge: u32, edit: impl Fn(&mut Value), film: Option<Value>) -> Vec<u8> {
    let probed: Value = serde_json::from_str(&probe("R0000001.DNG", raw).unwrap()).unwrap();
    let facts = probed["facts"].to_string();
    let mut state: Value = serde_json::from_str(&read_sidecar(&facts, xmp).unwrap()).unwrap();
    edit(&mut state["settings"]);
    let state = state.to_string();
    let options = json!({"format": "png", "resize": {"mode": "longEdge", "value": long_edge}}).to_string();
    let plan: Value = serde_json::from_str(&export_plan(&facts, &state, &options).unwrap()).unwrap();
    let mut engine = Engine::new();
    engine.load_lut("identity", IDENTITY_CUBE).unwrap();
    engine.load("photo", &facts, raw, plan["maxEdge"].as_u64().unwrap() as usize).unwrap();
    engine.export("photo", &facts, &state, &options, &film.unwrap_or(Value::Null).to_string()).unwrap()
}

#[test]
fn png_export_matches_lightcraft_desktop() {
    let raw = read(&fixture(".cache/ricoh-gr3.dng"));
    let xmp = String::from_utf8(read(&fixture("ricoh-gr3.xmp"))).unwrap();
    let exported = rgb(&export_png(&raw, &xmp, 640));
    let golden = rgb(&read(&fixture("lightcraft-golden.png")));

    assert_eq!(golden.size, (640, 427));
    assert_eq!(exported.size, golden.size);
    let deltas: Vec<u8> = exported.values.iter().zip(&golden.values).map(|(a, b)| a.abs_diff(*b)).collect();
    let differing = deltas.iter().filter(|d| **d > 0).count();
    assert_eq!((deltas.iter().max(), differing), (Some(&0), 0), "export differs from LightCraft desktop");
}

#[test]
fn film_export_of_an_hdr_edit_is_its_sdr_rendition() {
    let raw = read(&fixture(".cache/ricoh-gr3.dng"));
    let xmp = String::from_utf8(read(&fixture("ricoh-gr3.xmp"))).unwrap();
    let film = || Some(json!({"negative": "identity", "print": "identity"}));
    let hdr = export_edited_png(
        &raw,
        &xmp,
        320,
        |s| s["hdr"] = json!({"enabled": true, "sdr_contrast": 60.0, "sdr_whites": -40.0}),
        film(),
    );
    let offsets_applied = export_edited_png(
        &raw,
        &xmp,
        320,
        |s| {
            let contrast = s["light"]["contrast"].as_f64().unwrap();
            let whites = s["light"]["whites"].as_f64().unwrap();
            s["light"]["contrast"] = json!((contrast + 60.0).clamp(-100.0, 100.0));
            s["light"]["whites"] = json!((whites - 40.0).clamp(-100.0, 100.0));
        },
        film(),
    );
    assert!(rgb(&hdr).values == rgb(&offsets_applied).values, "the film ignored the SDR contrast and whites offsets");
}

#[test]
fn film_export_of_an_edit_with_light_turned_off_leaves_its_light_out() {
    let raw = read(&fixture(".cache/ricoh-gr3.dng"));
    let xmp = String::from_utf8(read(&fixture("ricoh-gr3.xmp"))).unwrap();
    let film = || Some(json!({"negative": "identity", "print": "identity"}));
    let off = export_edited_png(
        &raw,
        &xmp,
        320,
        |s| {
            s["light"]["contrast"] = json!(80.0);
            s["light"]["whites"] = json!(-50.0);
            s["disabled_sections"] = json!(["light"]);
        },
        film(),
    );
    let defaults = export_edited_png(
        &raw,
        &xmp,
        320,
        |s| {
            s["light"]["contrast"] = json!(0.0);
            s["light"]["whites"] = json!(0.0);
            s["light"]["blacks"] = json!(0.0);
            s["light"]["exposure"] = json!(0.0);
            s["light"]["highlights"] = json!(0.0);
            s["light"]["shadows"] = json!(0.0);
            s["disabled_sections"] = json!(["light"]);
        },
        film(),
    );
    assert!(rgb(&off).values == rgb(&defaults).values, "the film kept the contrast and whites of a light section turned off");
}
