use std::io::Cursor;
use std::path::{Path, PathBuf};

use filmlab_engine::core::{Engine, export_plan, probe, read_sidecar};
use serde_json::{Value, json};

const IDENTITY_CUBE: &str = "LUT_3D_SIZE 2\n0 0 0\n1 0 0\n0 1 0\n1 1 0\n0 0 1\n1 0 1\n0 1 1\n1 1 1\n";
const LUMA: [f64; 3] = [0.2126, 0.7152, 0.0722];

struct Picture {
    width: usize,
    height: usize,
    rgb: Vec<[u8; 3]>,
}

impl Picture {
    fn transposed(&self) -> Picture {
        let rgb = (0..self.width).flat_map(|x| (0..self.height).map(move |y| (x, y))).map(|(x, y)| self.rgb[y * self.width + x]).collect();
        Picture { width: self.height, height: self.width, rgb }
    }

    fn luma(&self) -> Vec<f64> {
        self.rgb.iter().map(|px| (0..3).map(|c| LUMA[c] * f64::from(px[c])).sum()).collect()
    }
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
    let rgb = buffer[..frame.buffer_size()].chunks_exact(channels).map(|px| [px[0], px[1], px[2]]).collect();
    Picture { width: frame.width as usize, height: frame.height as usize, rgb }
}

fn export(orientation: &str, film: Value, long_edge: u32) -> Picture {
    let raw = read(&fixture(".cache/ricoh-gr3.dng"));
    let xmp = String::from_utf8(read(&fixture("ricoh-gr3.xmp"))).unwrap();
    let probed: Value = serde_json::from_str(&probe("R0000001.DNG", &raw).unwrap()).unwrap();
    let facts = probed["facts"].to_string();
    let mut state: Value = serde_json::from_str(&read_sidecar(&facts, &xmp).unwrap()).unwrap();
    state["settings"]["orientation"] = json!(orientation);
    let state = state.to_string();
    let options = json!({"format": "png", "resize": {"mode": "longEdge", "value": long_edge}}).to_string();
    let plan: Value = serde_json::from_str(&export_plan(&facts, &state, &options).unwrap()).unwrap();
    let mut engine = Engine::new();
    engine.load_lut("identity", IDENTITY_CUBE).unwrap();
    engine.load("photo", &facts, &raw, plan["maxEdge"].as_u64().unwrap() as usize).unwrap();
    picture(&engine.export("photo", &facts, &state, &options, &film.to_string()).unwrap())
}

fn film(grain: Value) -> Value {
    json!({"negative": "identity", "print": "identity", "blur": 0.1, "grain": grain, "dust": {"on": true, "size": 3.0, "softness": 0.2}, "halation": {"on": true, "amount": 1.0, "radius": 8.0}})
}

fn sd(values: &[f64]) -> f64 {
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    (values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / values.len() as f64).sqrt()
}

fn boxed(values: &[f64], width: usize, k: usize) -> Vec<f64> {
    let (w, h) = (width / k, values.len() / width / k);
    (0..h).flat_map(|y| (0..w).map(move |x| (y, x))).map(|(y, x)| (0..k * k).map(|i| values[(y * k + i / k) * width + x * k + i % k]).sum::<f64>() / (k * k) as f64).collect()
}

struct GrainScale {
    sd: f64,
    kept_by_2x2: f64,
}

fn grain_scale(orientation: &str, grain: Value) -> GrainScale {
    let grained = export(orientation, film(grain.clone()), 1600);
    let mut plain = grain;
    plain["amount"] = json!(0.0);
    let plain = export(orientation, film(plain), 1600);
    let diff: Vec<f64> = grained.luma().iter().zip(plain.luma()).map(|(g, p)| g - p).collect();
    let strength = sd(&diff);
    GrainScale { sd: strength, kept_by_2x2: sd(&boxed(&diff, grained.width, 2)) / strength }
}

struct Turn {
    mean: f64,
    most: i32,
}

fn turn(film: Value) -> Turn {
    let landscape = export("Normal", film.clone(), 640);
    let portrait = export("Transpose", film, 640).transposed();
    assert_eq!((portrait.width, portrait.height), (landscape.width, landscape.height));
    let off: Vec<i32> = landscape.rgb.iter().zip(&portrait.rgb).flat_map(|(a, b)| (0..3).map(|c| (i32::from(a[c]) - i32::from(b[c])).abs())).collect();
    Turn { mean: off.iter().map(|d| f64::from(*d)).sum::<f64>() / off.len() as f64, most: off.iter().copied().max().unwrap_or(0) }
}

#[test]
fn a_portrait_frame_takes_the_film_of_the_landscape_frame_turned() {
    let (pipeline, filmed) = (turn(Value::Null), turn(film(Value::Null)));
    assert!(
        filmed.mean < pipeline.mean + 0.05 && filmed.most <= pipeline.most,
        "turned back, the portrait's blur, halation and dust differ from the landscape's by mean {:.3}, most {} code values; without film by {:.3}, {}",
        filmed.mean,
        filmed.most,
        pipeline.mean,
        pipeline.most
    );
}

#[test]
fn a_portrait_frame_grains_like_the_landscape_frame() {
    let stills = json!({"structure": "stills", "amount": 0.5, "size": 1.0, "softness": 0.2, "defocus": 0.8, "color": 0.33, "highlights": 0.44});
    let cine = json!({"structure": "cine", "amount": 0.3, "size": 2.0, "softness": 0.2, "defocus": 0.8});
    for grain in [stills, cine] {
        let (landscape, portrait) = (grain_scale("Normal", grain.clone()), grain_scale("Transpose", grain.clone()));
        let (strength, scale) = (portrait.sd / landscape.sd, portrait.kept_by_2x2 / landscape.kept_by_2x2);
        assert!((strength - 1.0).abs() < 0.05 && (scale - 1.0).abs() < 0.05, "{grain}: portrait grain sd x{strength:.3} and 2x2 retention x{scale:.3} of the landscape's");
    }
}
