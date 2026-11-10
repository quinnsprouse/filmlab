use std::io::Cursor;
use std::path::{Path, PathBuf};

use filmlab_engine::core::{Engine, export_plan, probe, read_sidecar};
use serde_json::{Value, json};

const IDENTITY_CUBE: &str = "LUT_3D_SIZE 2\n0 0 0\n1 0 0\n0 1 0\n1 1 0\n0 0 1\n1 0 1\n0 1 1\n1 1 1\n";
const LUMA: [f64; 3] = [0.2126, 0.7152, 0.0722];

struct Picture {
    width: usize,
    rgb: Vec<[f64; 3]>,
    luma: Vec<f64>,
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)
}

fn read(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e} (run scripts/fixtures.sh)", path.display()))
}

fn picture(png_bytes: &[u8]) -> Picture {
    let mut decoder = png::Decoder::new(Cursor::new(png_bytes));
    decoder.set_transformations(png::Transformations::EXPAND);
    let mut reader = decoder.read_info().expect("a PNG header");
    let mut buffer = vec![0; reader.output_buffer_size().expect("a bounded PNG")];
    let frame = reader.next_frame(&mut buffer).expect("a PNG frame");
    let channels = frame.color_type.samples();
    let rgb: Vec<[f64; 3]> = buffer[..frame.buffer_size()].chunks_exact(channels).map(|px| [0, 1, 2].map(|c| f64::from(px[c]))).collect();
    let luma = rgb.iter().map(|px| (0..3).map(|c| LUMA[c] * px[c]).sum()).collect();
    Picture { width: frame.width as usize, rgb, luma }
}

fn curved_print() -> String {
    let n = 17;
    let mut cube = format!("LUT_3D_SIZE {n}\n");
    for i in 0..n * n * n {
        let [r, g, b] = [i % n, i / n % n, i / (n * n)].map(|v| v as f64 / (n - 1) as f64);
        let [r, g, b] = [0.8 * r + 0.2 * g, 0.1 * r + 0.8 * g + 0.1 * b, 0.3 * g + 0.7 * b].map(|v: f64| v.powf(0.45));
        cube += &format!("{r} {g} {b}\n");
    }
    cube
}

fn export(grain: Value, long_edge: u32) -> Picture {
    export_through("identity", grain, long_edge)
}

fn export_through(print: &str, grain: Value, long_edge: u32) -> Picture {
    let raw = read(&fixture(".cache/ricoh-gr3.dng"));
    let xmp = String::from_utf8(read(&fixture("ricoh-gr3.xmp"))).unwrap();
    let probed: Value = serde_json::from_str(&probe("R0000001.DNG", &raw).unwrap()).unwrap();
    let facts = probed["facts"].to_string();
    let mut state: Value = serde_json::from_str(&read_sidecar(&facts, &xmp).unwrap()).unwrap();
    state["settings"]["color"]["vibrance"] = json!(0.0);
    let state = state.to_string();
    let options = json!({"format": "png", "resize": {"mode": "longEdge", "value": long_edge}}).to_string();
    let plan: Value = serde_json::from_str(&export_plan(&facts, &state, &options).unwrap()).unwrap();
    let mut engine = Engine::new();
    engine.load_lut("identity", IDENTITY_CUBE).unwrap();
    engine.load_lut("curved", &curved_print()).unwrap();
    engine.load("photo", &facts, &raw, plan["maxEdge"].as_u64().unwrap() as usize).unwrap();
    let film = json!({"negative": "identity", "print": print, "blur": 0.025, "grain": grain});
    picture(&engine.export("photo", &facts, &state, &options, &film.to_string()).unwrap())
}

fn stills(amount: f64) -> Value {
    coloured(amount, 0.33)
}

fn coloured(amount: f64, color: f64) -> Value {
    json!({"structure": "stills", "amount": amount, "size": 1.0, "softness": 0.0, "defocus": 1.0, "color": color, "highlights": 0.44})
}

fn grain_of(grained: &Picture, plain: &Picture) -> Vec<f64> {
    grained.luma.iter().zip(&plain.luma).map(|(g, p)| g - p).collect()
}

fn sd(values: &[f64]) -> f64 {
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    (values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / values.len() as f64).sqrt()
}

fn boxed(values: &[f64], width: usize, k: usize) -> Vec<f64> {
    let (w, h) = (width / k, values.len() / width / k);
    (0..h).flat_map(|y| (0..w).map(move |x| (y, x))).map(|(y, x)| (0..k * k).map(|i| values[(y * k + i / k) * width + x * k + i % k]).sum::<f64>() / (k * k) as f64).collect()
}

#[test]
fn a_grain_without_a_structure_is_the_cine_grain_which_takes_no_colour_or_highlights() {
    let old = json!({"amount": 0.3, "size": 2.0, "softness": 0.05, "defocus": 0.9});
    let cine = json!({"amount": 0.3, "size": 2.0, "softness": 0.05, "defocus": 0.9, "structure": "cine", "color": 0.9, "highlights": 0.2});
    assert!(export(old, 320).rgb == export(cine, 320).rgb, "a cine grain with colour and highlights stored rendered other than the grain without a structure");
}

fn shifts(grained: &Picture, plain: &Picture) -> Vec<(usize, [f64; 3])> {
    (0..8)
        .map(|bin| {
            let inside: Vec<[f64; 3]> = grained.rgb.iter().zip(&plain.rgb).zip(&plain.luma).filter(|(_, y)| (**y / 32.0) as usize == bin).map(|((g, p), _)| [0, 1, 2].map(|c| g[c] - p[c])).collect();
            (bin, [0, 1, 2].map(|c| inside.iter().map(|d| d[c]).sum::<f64>() / inside.len().max(1) as f64), inside.len())
        })
        .filter(|(_, _, n)| *n > 5000)
        .map(|(bin, shift, _)| (bin, shift))
        .collect()
}

#[test]
fn stills_grain_leaves_every_tone_where_it_was() {
    let plain = export(stills(0.0), 1600);
    let grained = export(stills(0.5), 1600);
    let grain = grain_of(&grained, &plain);
    let moved: Vec<_> = shifts(&grained, &plain).into_iter().filter(|(_, shift)| shift.iter().any(|s| s.abs() >= 0.5)).collect();
    assert!(sd(&grain) > 2.0, "the grain is too faint to judge: sd {}", sd(&grain));
    assert!(moved.is_empty(), "grain moved these tones (bin of 32 code values, mean shift per channel): {moved:?}");
}

#[test]
fn the_strongest_colour_grain_leaves_every_unclipped_tone_where_it_was_through_a_curved_print() {
    let plain = export_through("curved", coloured(0.0, 1.0), 1600);
    let grained = export_through("curved", coloured(1.0, 1.0), 1600);
    let red: Vec<f64> = grained.rgb.iter().zip(&plain.rgb).map(|(g, p)| g[0] - p[0]).collect();
    let unclipped = shifts(&grained, &plain).into_iter().filter(|(bin, _)| (1..7).contains(bin));
    let moved: Vec<_> = unclipped.filter(|(_, shift)| shift.iter().any(|s| s.abs() >= 0.5)).collect();
    assert!(sd(&red) > 10.0, "the grain is too faint to judge: red sd {}", sd(&red));
    assert!(moved.is_empty(), "grain moved these tones (bin of 32 code values, mean shift per channel): {moved:?}");
}

#[test]
fn a_smaller_stills_render_shows_the_larger_one_scaled_down() {
    let (large, small) = (export(stills(0.5), 1600), export(stills(0.5), 400));
    let (large_plain, small_plain) = (export(stills(0.0), 1600), export(stills(0.0), 400));
    let scaled = sd(&boxed(&grain_of(&large, &large_plain), large.width, 4));
    let rendered = sd(&grain_of(&small, &small_plain));
    assert!((rendered / scaled - 1.0).abs() < 0.2, "the 400 px render's grain sd {rendered:.3} against the 1600 px one area-resampled {scaled:.3}");
}
