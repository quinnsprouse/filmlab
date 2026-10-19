use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Once;
use std::time::UNIX_EPOCH;

use image::ExtendedColorType;
use image::codecs::jpeg::JpegEncoder;

/// Bump when thumbnail rendering changes in a way the settings hash can't see.
const VERSION: u64 = 1;
const LIMIT: u64 = 500 << 20;
const QUALITY: u8 = 88;

/// A decoded thumbnail: RGBA pixels, top row first.
pub struct Pixels {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Identifies a photo file by path, size and modification time.
#[derive(Clone)]
pub struct Key(u64);

pub fn hash(parts: &[&[u8]]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for part in parts {
        for b in part.iter().chain(&[0xff]) {
            h = (h ^ *b as u64).wrapping_mul(0x100000001b3);
        }
    }
    h
}

/// Hash of everything that changes a thumbnail's pixels.
pub fn look(settings: &str, film: &str, edge: usize, source: usize) -> u64 {
    hash(&[settings.as_bytes(), film.as_bytes(), &edge.to_le_bytes(), &source.to_le_bytes(), &VERSION.to_le_bytes()])
}

fn dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join("Library/Caches/Filmlab"))
}

pub fn key(path: &Path) -> Option<Key> {
    let meta = fs::metadata(path).ok()?;
    let mtime = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_nanos();
    Some(Key(hash(&[path.as_os_str().as_encoded_bytes(), &meta.len().to_le_bytes(), &mtime.to_le_bytes()])))
}

fn file(key: &Key, ext: &str) -> Option<PathBuf> {
    Some(dir()?.join(format!("{:016x}.{ext}", key.0)))
}

fn write(target: PathBuf, bytes: &[u8]) {
    let Some(parent) = target.parent() else { return };
    let _ = fs::create_dir_all(parent);
    let tmp = target.with_extension(format!("tmp{:?}", std::thread::current().id()).replace(['(', ')'], ""));
    if fs::write(&tmp, bytes).is_ok() && fs::rename(&tmp, &target).is_err() {
        let _ = fs::remove_file(&tmp);
    }
}

pub fn probe(key: &Key) -> Option<String> {
    String::from_utf8(fs::read(file(key, "probe")?).ok()?).ok()
}

pub fn store_probe(key: &Key, json: &str) {
    if let Some(target) = file(key, "probe") {
        write(target, json.as_bytes());
    }
}

/// The cached thumbnail when it was rendered with the same `look`.
pub fn thumbnail(key: &Key, look: u64) -> Option<Pixels> {
    let bytes = fs::read(file(key, "thumb")?).ok()?;
    let (head, jpeg) = bytes.split_at_checked(8)?;
    if head != look.to_le_bytes() {
        return None;
    }
    let image = image::load_from_memory_with_format(jpeg, image::ImageFormat::Jpeg).ok()?.into_rgba8();
    Some(Pixels { width: image.width(), height: image.height(), rgba: image.into_raw() })
}

/// JPEG bytes of the RGBA pixels, ready for `store_thumbnail`.
pub fn encode(width: u32, height: u32, rgba: &[u8]) -> Option<Vec<u8>> {
    let rgb: Vec<u8> = rgba.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
    let mut out = Vec::new();
    JpegEncoder::new_with_quality(&mut out, QUALITY).encode(&rgb, width, height, ExtendedColorType::Rgb8).ok()?;
    Some(out)
}

pub fn store_thumbnail(key: &Key, look: u64, jpeg: &[u8]) {
    if let Some(target) = file(key, "thumb") {
        let mut bytes = look.to_le_bytes().to_vec();
        bytes.extend_from_slice(jpeg);
        write(target, &bytes);
    }
}

/// Deletes the oldest entries once the cache passes its size limit, once per process.
pub fn prune() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        std::thread::spawn(|| {
            let Some(dir) = dir() else { return };
            let Ok(entries) = fs::read_dir(dir) else { return };
            let mut files: Vec<_> = entries.flatten().filter_map(|e| e.metadata().ok().map(|m| (m.modified().unwrap_or(UNIX_EPOCH), m.len(), e.path()))).collect();
            let mut total: u64 = files.iter().map(|f| f.1).sum();
            files.sort();
            for (_, len, path) in files {
                if total <= LIMIT {
                    break;
                }
                if fs::remove_file(path).is_ok() {
                    total -= len;
                }
            }
        });
    });
}
