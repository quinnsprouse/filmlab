use std::collections::VecDeque;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use filmlab_engine::core::{Engine, export_plan, probe, read_sidecar};
use serde_json::{Value, json};

const LONG_EDGE: u32 = 640;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)
}

fn read(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e} (run scripts/fixtures.sh)", path.display()))
}

struct Luma {
    width: usize,
    height: usize,
    values: Vec<f32>,
}

impl Luma {
    fn of_png(bytes: &[u8]) -> Luma {
        let mut decoder = png::Decoder::new(Cursor::new(bytes));
        decoder.set_transformations(png::Transformations::EXPAND);
        let mut reader = decoder.read_info().expect("a PNG header");
        let mut buffer = vec![0; reader.output_buffer_size().expect("a bounded PNG")];
        let frame = reader.next_frame(&mut buffer).expect("a PNG frame");
        let channels = frame.color_type.samples();
        let values = buffer[..frame.buffer_size()]
            .chunks_exact(channels)
            .map(|px| (0.2126 * f32::from(px[0]) + 0.7152 * f32::from(px[1]) + 0.0722 * f32::from(px[2])) / 255.0)
            .collect();
        Luma { width: frame.width as usize, height: frame.height as usize, values }
    }

    fn ev(&self, i: usize) -> f32 {
        (self.values[i] + 1e-3).log2()
    }
}

fn render(exposure: f64, dehaze: f64) -> Luma {
    let raw = read(&fixture(".cache/ricoh-gr3.dng"));
    let xmp = String::from_utf8(read(&fixture("ricoh-gr3.xmp"))).unwrap();
    let probed: Value = serde_json::from_str(&probe("R0000001.DNG", &raw).unwrap()).unwrap();
    let facts = probed["facts"].to_string();
    let mut state: Value = serde_json::from_str(&read_sidecar(&facts, &xmp).unwrap()).unwrap();
    state["settings"]["light"]["exposure"] = json!(exposure);
    state["settings"]["effects"]["dehaze"] = json!(dehaze);
    let state = state.to_string();
    let options = json!({"format": "png", "resize": {"mode": "longEdge", "value": LONG_EDGE}}).to_string();
    let plan: Value = serde_json::from_str(&export_plan(&facts, &state, &options).unwrap()).unwrap();
    let mut engine = Engine::new();
    engine.load("photo", &facts, &raw, plan["maxEdge"].as_u64().unwrap() as usize).unwrap();
    Luma::of_png(&engine.export("photo", &facts, &state, &options, "null").unwrap())
}

fn neighbours(i: usize, width: usize, height: usize, reach: usize) -> impl Iterator<Item = usize> {
    let (x, y) = (i % width, i / width);
    let xs = x.saturating_sub(reach)..=(x + reach).min(width - 1);
    let ys = y.saturating_sub(reach)..=(y + reach).min(height - 1);
    ys.flat_map(move |ny| xs.clone().map(move |nx| ny * width + nx))
}

fn steps_from_open_sky(luma: &Luma) -> Vec<usize> {
    let (w, h) = (luma.width, luma.height);
    let bright: Vec<bool> = luma.values.iter().map(|v| *v > 0.8).collect();
    let mut steps = vec![usize::MAX; w * h];
    let mut queue = VecDeque::new();
    for (i, step) in steps.iter_mut().enumerate() {
        if neighbours(i, w, h, 2).all(|n| bright[n]) {
            *step = 0;
            queue.push_back(i);
        }
    }
    while let Some(i) = queue.pop_front() {
        for n in neighbours(i, w, h, 1) {
            if steps[n] == usize::MAX {
                steps[n] = steps[i] + 1;
                queue.push_back(n);
            }
        }
    }
    steps
}

fn mean_shift(before: &Luma, after: &Luma, pixels: impl Iterator<Item = usize>) -> (f32, usize) {
    let (sum, count) = pixels.fold((0.0, 0), |(s, c), i| (s + after.ev(i) - before.ev(i), c + 1));
    (sum / count as f32, count)
}

#[test]
fn dehaze_does_not_darken_land_along_a_bright_sky() {
    let (plain, dehazed) = (render(3.0, 0.0), render(3.0, 30.0));
    let steps = steps_from_open_sky(&plain);
    let long = plain.width.max(plain.height) as f32;
    let (steps, values) = (&steps, &plain.values);
    let land_between = |lo: f32, hi: f32| {
        let reach = move |i: usize| steps[i] as f32 / long;
        (0..steps.len()).filter(move |i| values[*i] < 0.6 && reach(*i) > lo && reach(*i) <= hi)
    };
    let (along_edge, edge_pixels) = mean_shift(&plain, &dehazed, land_between(0.0, 0.01));
    let (inland, inland_pixels) = mean_shift(&plain, &dehazed, land_between(0.04, 0.08));

    assert!(edge_pixels > 500 && inland_pixels > 500, "{edge_pixels} edge and {inland_pixels} inland pixels");
    assert!(
        along_edge - inland > -0.1,
        "dehaze darkens land along the sky by {along_edge:+.3} EV against {inland:+.3} EV inland"
    );
}

#[test]
fn dehaze_still_deepens_hazy_sky() {
    let (plain, dehazed) = (render(0.35, 0.0), render(0.35, 30.0));
    let sky = 0..plain.width * plain.height * 3 / 10;
    let spread = |luma: &Luma| {
        let evs: Vec<f32> = sky.clone().map(|i| luma.ev(i)).collect();
        let mean = evs.iter().sum::<f32>() / evs.len() as f32;
        (evs.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / evs.len() as f32).sqrt()
    };
    let (before, after) = (spread(&plain), spread(&dehazed));

    assert!(after > before * 1.15, "the sky's contrast went from {before:.3} EV to {after:.3} EV");
}
