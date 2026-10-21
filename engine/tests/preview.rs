use std::path::{Path, PathBuf};

use filmlab_engine::core::probe;
use filmlab_engine::preview::embedded_preview;
use serde_json::Value;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)
}

#[test]
fn a_raw_preview_is_upright_small_and_shaped_like_the_photo() {
    let path = fixture(".cache/ricoh-gr3.dng");
    let raw = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e} (run scripts/fixtures.sh)", path.display()));
    let probed: Value = serde_json::from_str(&probe("R0000001.DNG", &raw).unwrap()).unwrap();
    let (width, height) = (probed["facts"]["width"].as_f64().unwrap(), probed["facts"]["height"].as_f64().unwrap());

    let preview = embedded_preview(&path, 480).expect("the GR III embeds a JPEG preview");

    assert_eq!(preview.width.max(preview.height), 480);
    assert_eq!(preview.rgba.len(), preview.width as usize * preview.height as usize * 4);
    let aspect = f64::from(preview.width) / f64::from(preview.height);
    assert!((aspect - width / height).abs() < 0.02, "preview {}x{} against photo {width}x{height}", preview.width, preview.height);
    let mean = preview.rgba.iter().map(|v| f64::from(*v)).sum::<f64>() / preview.rgba.len() as f64;
    assert!((20.0..235.0).contains(&mean), "mean {mean}");
}

#[test]
fn an_image_without_a_preview_has_none() {
    assert!(embedded_preview(&fixture("lightcraft-golden.png"), 480).is_none());
}
