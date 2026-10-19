//! Raw decode timing and bit-exact parity: every fixture decoded at each max edge, timed (median of several runs)
//! and fingerprinted (the decoded pixels' f32 bits, the `SourceInfo` the engine keeps, and the probe's facts).
//!
//! cargo run --release --example decode_parity -- [--runs N] [--edges 2560,512,0] [--threads N] [--save F | --check F] [files…]
//!
//! With no files it reads every photo in `.scratch/fixtures` and `.scratch/fixtures/roll`. `--save` writes the
//! fingerprints, `--check` compares against a saved run and exits non-zero on any difference.

use std::hash::Hasher;
use std::path::{Path, PathBuf};
use std::time::Instant;

struct Fnv(u64);

impl Fnv {
    fn new() -> Fnv {
        Fnv(0xcbf2_9ce4_8422_2325)
    }
}

impl Hasher for Fnv {
    fn write(&mut self, bytes: &[u8]) {
        // (word at a time: a full-size decode is ~300 MB of floats)
        let (words, rest) = bytes.as_chunks::<8>();
        for word in words {
            self.0 = (self.0 ^ u64::from_le_bytes(*word)).wrapping_mul(0x100_0000_01b3);
            self.0 ^= self.0 >> 29;
        }
        for b in rest {
            self.0 = (self.0 ^ u64::from(*b)).wrapping_mul(0x100_0000_01b3);
        }
    }

    fn finish(&self) -> u64 {
        self.0
    }
}

fn pixels_hash(img: &lightcraft_raster::Rgb32f) -> u64 {
    let mut h = Fnv::new();
    h.write(&(img.width as u64).to_le_bytes());
    h.write(&(img.height as u64).to_le_bytes());
    let mut buf = Vec::with_capacity(img.data.len().min(1 << 20) * 12);
    for chunk in img.data.chunks(1 << 20) {
        buf.clear();
        for p in chunk {
            for v in p {
                buf.extend_from_slice(&v.to_bits().to_le_bytes());
            }
        }
        h.write(&buf);
    }
    h.finish()
}

fn text_hash(s: &str) -> u64 {
    let mut h = Fnv::new();
    h.write(s.as_bytes());
    h.finish()
}

fn fixtures() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../.scratch/fixtures");
    let mut files = Vec::new();
    for dir in [root.clone(), root.join("roll")] {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        let mut found: Vec<PathBuf> = entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().and_then(|e| e.to_str()).is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "dng" | "jpg" | "jpeg")))
            .collect();
        found.sort();
        files.extend(found);
    }
    files
}

fn median(v: &mut [f64]) -> f64 {
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

pub fn main() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let (mut runs, mut edges, mut threads, mut save, mut check, mut files) = (5usize, vec![2560usize, 512, 0], 0usize, None, None, Vec::new());
    while let Some(a) = args.next() {
        let mut value = || args.next().ok_or(format!("{a} needs a value"));
        match a.as_str() {
            "--runs" => runs = value()?.parse().map_err(|e| format!("{e}"))?,
            "--threads" => threads = value()?.parse().map_err(|e| format!("{e}"))?,
            "--edges" => edges = value()?.split(',').map(|e| e.parse().map_err(|e| format!("{e}"))).collect::<Result<_, _>>()?,
            "--save" => save = Some(value()?),
            "--check" => check = Some(value()?),
            _ => files.push(PathBuf::from(a)),
        }
    }
    if files.is_empty() {
        files = fixtures();
    }
    let pool = rayon::ThreadPoolBuilder::new().num_threads(threads).build().map_err(|e| e.to_string())?;
    println!("{} threads, {runs} runs each, median / min ms", pool.current_num_threads());
    let mut lines = Vec::new();
    let mut totals = vec![(0.0f64, 0.0f64); edges.len()];
    for path in &files {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("photo").to_string();
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let facts = filmlab_engine::core::probe(&name, &bytes)?;
        let facts_hash = text_hash(&facts);
        for (slot, &edge) in edges.iter().enumerate() {
            let max_edge = if edge == 0 { usize::MAX } else { edge };
            let mut times = Vec::with_capacity(runs);
            let mut fingerprint = None;
            for _ in 0..runs.max(1) {
                let t = Instant::now();
                let (img, info) = pool.install(|| lightcraft_engine::files::load_bytes(&bytes, max_edge))?;
                times.push(t.elapsed().as_secs_f64() * 1000.0);
                let f = (img.width, img.height, pixels_hash(&img), text_hash(&format!("{info:?}")));
                drop(img);
                if fingerprint.is_some_and(|p| p != f) {
                    return Err(format!("{name} @ {edge}: not deterministic ({fingerprint:?} then {f:?})"));
                }
                fingerprint = Some(f);
            }
            let (w, h, px, info) = fingerprint.unwrap_or_default();
            let min = times.iter().cloned().fold(f64::MAX, f64::min);
            let med = median(&mut times);
            totals[slot].0 += med;
            totals[slot].1 += min;
            println!("{name:<16} {edge:>5} {w:>5}x{h:<5} {med:>8.1} {min:>8.1}  px {px:016x} info {info:016x} facts {facts_hash:016x}");
            lines.push(format!("{name} {edge} {w}x{h} px {px:016x} info {info:016x} facts {facts_hash:016x}"));
        }
    }
    for (slot, &edge) in edges.iter().enumerate() {
        println!("total {edge:>5}: median sum {:.1} ms, min sum {:.1} ms", totals[slot].0, totals[slot].1);
    }
    let digest = text_hash(&lines.join("\n"));
    println!("digest {digest:016x} ({} decodes)", lines.len());
    if let Some(path) = save {
        std::fs::write(&path, lines.join("\n") + "\n").map_err(|e| e.to_string())?;
    }
    if let Some(path) = check {
        let want = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
        let want: Vec<&str> = want.lines().collect();
        let bad: Vec<String> = lines.iter().filter(|l| !want.contains(&l.as_str())).cloned().collect();
        if !bad.is_empty() || want.len() != lines.len() {
            for b in &bad {
                println!("MISMATCH {b}");
            }
            return Err(format!("{} of {} fingerprints differ from {path}", bad.len(), lines.len()));
        }
        println!("all {} fingerprints match {path}", lines.len());
    }
    Ok(())
}
