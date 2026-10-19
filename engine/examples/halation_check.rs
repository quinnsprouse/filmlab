use filmlab_engine::halation::{HalationParams, halate_frame};
use filmlab_engine::texture::Size;

fn main() -> Result<(), String> {
    let a: Vec<String> = std::env::args().collect();
    let (w, h): (usize, usize) = (a[2].parse().map_err(|_| "w")?, a[3].parse().map_err(|_| "h")?);
    let p = HalationParams { amount: a[4].parse().unwrap_or(0.5), hue: a[5].parse().unwrap_or(0.45), sat: a[6].parse().unwrap_or(1.0), radius: a[7].parse().unwrap_or(4.0), highlights_only: a[8] == "1", ceiling: 1.0 };
    let bytes = std::fs::read(&a[1]).map_err(|e| e.to_string())?;
    let px: Vec<[f32; 3]> = bytes.as_chunks::<12>().0.iter().map(|c| std::array::from_fn(|i| f32::from_le_bytes([c[4 * i], c[4 * i + 1], c[4 * i + 2], c[4 * i + 3]]))).collect();
    let out = halate_frame(px, Size { w, h }, &p).ok_or("off")?;
    let raw: Vec<u8> = out.iter().flat_map(|c| c.iter().flat_map(|v| v.to_le_bytes())).collect();
    std::fs::write(&a[9], raw).map_err(|e| e.to_string())
}
