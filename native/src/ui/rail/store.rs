use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use filmlab_engine::core;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const LOOK_GROUPS: [&str; 10] = ["film", "profile", "treatment", "toneCurve", "color", "colorMixer", "colorGrading", "effects", "vignette", "calibration"];
pub const PER_PHOTO_GROUPS: [&str; 8] = ["whiteBalance", "light", "detail", "optics", "geometry", "crop", "masks", "spots"];

pub fn group_label(group: &str) -> &str {
    match group {
        "film" => "Film stock, halation and grain",
        "profile" => "Camera profile",
        "treatment" => "Treatment",
        "toneCurve" => "Tone curve",
        "color" => "Vibrance and saturation",
        "colorMixer" => "Color mixer",
        "colorGrading" => "Color grading",
        "effects" => "Effects",
        "vignette" => "Vignette",
        "calibration" => "Calibration",
        "whiteBalance" => "White balance",
        "light" => "Light",
        "detail" => "Detail",
        "optics" => "Optics",
        "geometry" => "Geometry",
        "crop" => "Crop",
        "masks" => "Masks",
        "spots" => "Healing",
        other => other,
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub groups: Vec<String>,
    pub partial: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub film: Option<Value>,
    #[serde(default)]
    pub created_at: String,
}

fn path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join("Library/Application Support/Filmlab/profiles.json"))
}

pub fn load() -> Vec<Profile> {
    let Some(text) = path().and_then(|p| std::fs::read_to_string(p).ok()) else { return Vec::new() };
    let Ok(Value::Array(items)) = serde_json::from_str::<Value>(&text) else { return Vec::new() };
    items.into_iter().filter_map(|item| serde_json::from_value(item).ok()).collect()
}

pub fn write(profiles: &[Profile]) {
    let Some(path) = path() else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(text) = serde_json::to_string(profiles) {
        let _ = std::fs::write(path, text);
    }
}

// why: the web stores every group in the profile except film, whose settings travel as `film`
pub fn build(name: &str, settings: &Value, groups: Vec<String>, film: Option<Value>) -> Option<Profile> {
    let engine_groups: Vec<&String> = groups.iter().filter(|g| *g != "film").collect();
    let partial = core::extract_profile(&settings.to_string(), &serde_json::to_string(&engine_groups).ok()?).ok()?;
    let film = film.filter(|_| groups.iter().any(|g| g == "film"));
    Some(Profile { id: new_id(), name: name.trim().to_string(), groups, partial: serde_json::from_str(&partial).ok()?, film, created_at: iso_now() })
}

/// The profiles the rail shows, read from disk the first time they're needed.
pub fn shown(rail: &mut super::State) -> &mut Vec<Profile> {
    if !rail.loaded {
        rail.profiles = load();
        rail.loaded = true;
    }
    &mut rail.profiles
}

/// Sets the rail's amount slider, which re-applies the active profile when moved.
pub fn set_amount(rail: &mut super::State, percent: f64) {
    rail.amount = percent;
}

fn new_id() -> String {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let word = || {
        let mut hasher = RandomState::new().build_hasher();
        hasher.write_u128(nanos);
        hasher.finish()
    };
    let (a, b) = (word(), word());
    format!("{:08x}-{:04x}-4{:03x}-{:04x}-{:012x}", a >> 32, (a >> 16) & 0xffff, a & 0xfff, 0x8000 | (b & 0x3fff), (b >> 16) & 0xffff_ffff_ffff)
}

pub fn iso_now() -> String {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let (secs, millis) = (now.as_secs() as i64, now.subsec_millis());
    let (days, rest) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{millis:03}Z", rest / 3600, rest % 3600 / 60, rest % 60)
}
