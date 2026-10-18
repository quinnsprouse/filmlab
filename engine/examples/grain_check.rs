use std::time::Instant;

use filmlab_engine::grain::GrainParams;
use filmlab_engine::texture::{Plane, Region, Size, TextureParams, margin, texture};

const GAUGES: [(&str, f32, f32, f32, f32); 4] = [("16", 0.25, 2.5, 0.25, 0.85), ("35", 0.1495, 2.0, 0.05, 0.9), ("65", 0.0995, 1.0, 0.05, 1.0), ("120", 0.065, 0.6, 0.02, 1.0)];

fn read(path: &str) -> Vec<f32> {
    std::fs::read(path).expect(path).as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b)).collect()
}

fn grain(amount: f32, size: f32, softness: f32, defocus: f32) -> TextureParams {
    TextureParams { film_blur: 0.0, grain: Some(GrainParams { amount, size, softness, defocus, seed: 0 }), dust: None, halation: None }
}

fn window_identical(rgb: &[[f32; 3]], frame: Size, p: &TextureParams, whole: &[[f32; 3]]) -> bool {
    let (w, h) = (frame.w, frame.h);
    let window = Region { x: w / 3, y: h / 4, w: w / 3, h: h / 3 };
    let grown = window.grown(margin(frame, p), frame);
    let crop: Vec<[f32; 3]> = (grown.y..grown.y + grown.h).flat_map(|y| (grown.x..grown.x + grown.w).map(move |x| (y, x))).map(|(y, x)| rgb[y * w + x]).collect();
    let part = texture(Plane { region: grown, data: crop }, frame, window, p);
    part.iter().enumerate().all(|(k, v)| *v == whole[(window.y + k / window.w) * w + window.x + k % window.w])
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let dir = &args[1];
    let meta: Vec<usize> = std::fs::read_to_string(format!("{dir}/off_ph0.meta")).unwrap().split_whitespace().map(|v| v.parse().unwrap()).collect();
    let (w, h) = (meta[0], meta[1]);
    let input: Vec<[f32; 3]> = read(&format!("{dir}/off_ph0.in")).as_chunks::<3>().0.to_vec();
    let frame = Size { w, h };
    for defocus in [0.85f32, 0.9] {
        let p = grain(0.0, 1.0, 0.05, defocus);
        let out = texture(Plane { region: Region::whole(w, h), data: input.clone() }, frame, Region::whole(w, h), &p);
        let reference = read(&format!("{dir}/defocus_{}.np", (defocus * 100.0) as u32));
        let mut sum = 0.0f64;
        let mut max = 0.0f64;
        for (a, b) in out.iter().flatten().zip(&reference) {
            let d = f64::from((a - b).abs()) * 65535.0;
            sum += d;
            max = max.max(d);
        }
        let same = window_identical(&input, frame, &p, &out);
        println!("defocus {defocus}: vs numpy mean {:.3} max {max:.1} LSB16 | window identical: {same}", sum / reference.len() as f64);
    }
    for height in [1080usize, 1600, 2160] {
        let frame = Size { w: height * 3 / 2, h: height };
        let flat = vec![[0.42f32; 3]; frame.w * frame.h];
        let whole = Region::whole(frame.w, frame.h);
        let plain = texture(Plane { region: whole, data: flat.clone() }, frame, whole, &TextureParams { film_blur: 0.0, grain: None, dust: None, halation: None });
        let level = plain[plain.len() / 2][0];
        for (name, amount, size, softness, defocus) in GAUGES {
            let p = grain(amount, size, softness, defocus);
            let t = Instant::now();
            let out = texture(Plane { region: whole, data: flat.clone() }, frame, whole, &p);
            let ms = t.elapsed().as_secs_f64() * 1e3;
            let d: Vec<f64> = out.iter().map(|c| f64::from(c[1] - level)).collect();
            let n = d.len() as f64;
            let mean = d.iter().sum::<f64>() / n;
            let var = d.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n;
            let skew = d.iter().map(|v| (v - mean).powi(3)).sum::<f64>() / n / var.powf(1.5);
            let kurt = d.iter().map(|v| (v - mean).powi(4)).sum::<f64>() / n / var.powi(2) - 3.0;
            let lag = (0..frame.h).flat_map(|y| (0..frame.w - 1).map(move |x| (y, x))).map(|(y, x)| (d[y * frame.w + x] - mean) * (d[y * frame.w + x + 1] - mean)).sum::<f64>() / ((frame.w - 1) * frame.h) as f64 / var;
            let same = if height == 1600 { window_identical(&flat, frame, &p, &out).to_string() } else { "-".into() };
            println!("H {height} gauge {name:>3}: level {level:.4} std {:.5} mean {:.6} skew {skew:.3} kurt {kurt:.3} lag1 {lag:.3} | {ms:.0} ms | window identical: {same}", var.sqrt(), mean);
        }
    }
}
