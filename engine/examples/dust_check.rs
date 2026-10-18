use filmlab_engine::dust::{DustParams, strength};
use filmlab_engine::texture::{Region, Size};

// why: writes FilmVision dust strength maps for scratchpad port/dust_compare.py to diff against the model
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let dir = &args[1];
    for case in args[2..].chunks(4) {
        let [w, h, size, softness] = [&case[0], &case[1], &case[2], &case[3]];
        let frame = Size { w: w.parse().unwrap(), h: h.parse().unwrap() };
        let p = DustParams { size: size.parse().unwrap(), softness: softness.parse().unwrap(), rows: DustParams::rows_of(frame) };
        let whole = Region::whole(frame.w, frame.h);
        let mut map = vec![0.0f32; frame.w * frame.h];
        for (k, s) in strength(frame, whole, &p) {
            map[k] = s;
        }
        let window = Region { x: frame.w / 5, y: frame.h / 6, w: frame.w / 2, h: frame.h / 2 };
        let part = strength(frame, window, &p);
        let same = part.iter().all(|&(k, s)| map[(window.y + k / window.w) * frame.w + window.x + k % window.w] == s)
            && part.len() == (window.y..window.y + window.h).flat_map(|y| (window.x..window.x + window.w).map(move |x| (y, x))).filter(|&(y, x)| map[y * frame.w + x] > 0.0).count();
        let bytes: Vec<u8> = map.iter().flat_map(|v| v.to_le_bytes()).collect();
        std::fs::write(format!("{dir}/{w}x{h}_{size}_{softness}.f32"), bytes).unwrap();
        println!("{w}x{h} size {size} softness {softness}: {} pixels, window identical: {same}", map.iter().filter(|v| **v > 0.0).count());
    }
}
