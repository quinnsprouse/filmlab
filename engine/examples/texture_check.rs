use std::time::Instant;

use filmlab_engine::texture::{Plane, Region, Size, TextureParams, margin, texture};

fn read(path: &str) -> Vec<f32> {
    std::fs::read(path).expect(path).as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b)).collect()
}

fn stats(a: &[[f32; 3]], b: &[f32]) -> (f64, f64) {
    let mut sum = 0.0f64;
    let mut max = 0.0f64;
    for (p, q) in a.iter().flatten().zip(b) {
        let d = f64::from((p - q).abs()) * 65535.0;
        sum += d;
        max = max.max(d);
    }
    (sum / b.len() as f64, max)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let dir = &args[1];
    for name in &args[2..] {
        let meta: Vec<usize> = std::fs::read_to_string(format!("{dir}/{name}.meta")).unwrap().split_whitespace().map(|v| v.parse().unwrap()).collect();
        let (w, h, blur) = (meta[0], meta[1], meta[2] as f32 / 1000.0);
        let input = read(&format!("{dir}/{name}.in"));
        let rgb: Vec<[f32; 3]> = input.as_chunks::<3>().0.to_vec();
        let frame = Size { w, h };
        let p = TextureParams { film_blur: blur, grain: None, dust: None, halation: None };
        let t = Instant::now();
        let whole = texture(Plane { region: Region::whole(w, h), data: rgb.clone() }, frame, Region::whole(w, h), &p);
        let ms = t.elapsed().as_secs_f64() * 1e3;
        let numpy = read(&format!("{dir}/{name}.np"));
        let resolve = read(&format!("{dir}/{name}.rs"));
        let (mn, xn) = stats(&whole, &numpy);
        let (mr, xr) = stats(&whole, &resolve);
        let window = Region { x: w / 3, y: h / 4, w: w / 3, h: h / 3 };
        let grown = window.grown(margin(frame, &p), frame);
        let crop: Vec<[f32; 3]> = (grown.y..grown.y + grown.h).flat_map(|y| (grown.x..grown.x + grown.w).map(move |x| (y, x))).map(|(y, x)| rgb[y * w + x]).collect();
        let part = texture(Plane { region: grown, data: crop }, frame, window, &p);
        let same = part.iter().enumerate().all(|(k, v)| *v == whole[(window.y + k / window.w) * w + window.x + k % window.w]);
        println!("{name:>16} {w}x{h} {ms:6.1} ms | vs numpy mean {mn:.2} max {xn:.0} | vs Resolve mean {mr:.2} max {xr:.0} LSB16 | window identical: {same}");
    }
}
