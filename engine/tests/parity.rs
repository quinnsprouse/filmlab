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

fn export_png(raw: &[u8], xmp: &str, long_edge: u32) -> Vec<u8> {
    let probed: Value = serde_json::from_str(&probe("R0000001.DNG", raw).unwrap()).unwrap();
    let facts = probed["facts"].to_string();
    let state = read_sidecar(&facts, xmp).unwrap();
    let options = json!({"format": "png", "resize": {"mode": "longEdge", "value": long_edge}}).to_string();
    let plan: Value = serde_json::from_str(&export_plan(&facts, &state, &options).unwrap()).unwrap();
    let mut engine = Engine::new();
    engine.load("photo", &facts, raw, plan["maxEdge"].as_u64().unwrap() as usize).unwrap();
    engine.export("photo", &facts, &state, &options, "null").unwrap()
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
