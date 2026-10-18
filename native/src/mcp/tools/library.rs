use std::path::PathBuf;

use gpui_kit::AsyncApp;
use serde::Deserialize;
use serde_json::{Value, json};

use super::app::strip;
use super::{Answer, Output, args, edit};
use crate::library::PhotoRecord;
use crate::ui::driver::Driver;

/// A photo's capture facts in one short object: date, camera, lens, exposure, size.
pub fn facts_of(record: &PhotoRecord) -> Value {
    let Some(probed) = &record.probed else { return json!({}) };
    let facts = &probed.facts;
    let meta = &facts["meta"];
    let captured = facts["captured"].as_str().map(|c| c.chars().take(16).collect::<String>().replace('T', " "));
    let shutter = meta["shutter"].as_str().map(|s| s.trim_end_matches(" s").trim_end_matches('s').to_string());
    let exposure = [meta["aperture"].as_f64().map(|a| format!("f/{a}")), shutter.map(|s| format!("{s}s")), meta["iso"].as_f64().map(|i| format!("ISO {i}"))];
    strip(json!({
        "taken": captured,
        "camera": meta["camera"],
        "lens": meta["lens"],
        "focal": meta["focal_mm"],
        "exposure": exposure.into_iter().flatten().collect::<Vec<_>>().join(" "),
        "size": [facts["width"], facts["height"]],
        "format": facts["format"],
    }))
}

fn expand(path: &str) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => std::env::var_os("HOME").map_or_else(|| PathBuf::from(path), |home| PathBuf::from(home).join(rest)),
        None => PathBuf::from(path),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OpenFolder {
    path: String,
    wait: Option<bool>,
}

pub async fn open_folder(driver: &Driver, cx: &mut AsyncApp, value: Value) -> Answer {
    let p: OpenFolder = args(value)?;
    let folder = expand(&p.path);
    if !folder.is_dir() {
        return Err(format!("{} isn't a folder", folder.display()));
    }
    let target = folder.clone();
    driver.act(cx, move |a, cx| a.open_folder(target, cx)).ok_or("Filmlab's window is gone")?;
    let indexed = |a: &crate::ui::app::FilmlabApp, cx: &gpui_kit::App| a.library.read(cx).photos.iter().all(|r| r.thumbnail.is_some() || r.error.is_some());
    let done = !p.wait.unwrap_or(true) || driver.wait(cx, 240, indexed).await;
    let summary = driver
        .read(cx, |a, cx| {
            let l = a.library.read(cx);
            let errors: Vec<String> = l.photos.iter().filter_map(|r| r.error.as_ref().map(|e| format!("{}: {e}", r.file.name))).take(5).collect();
            json!({
                "folder": folder,
                "photos": l.photos.len(),
                "indexing": l.photos.iter().filter(|r| r.thumbnail.is_none() && r.error.is_none()).count(),
                "errors": errors,
                "next": if l.photos.is_empty() { "the folder has no photos" } else { "list_photos, then open_photo" },
            })
        })
        .ok_or("Filmlab's window is gone")?;
    let mut summary = strip(summary);
    if !done {
        summary["note"] = json!("still indexing; thumbnails and flags fill in as it goes");
    }
    Ok(Output::json(summary))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OpenPhoto {
    id: String,
}

pub async fn open_photo(driver: &Driver, cx: &mut AsyncApp, value: Value) -> Answer {
    let p: OpenPhoto = args(value)?;
    let known = driver.read(cx, |a, cx| a.library.read(cx).record(&p.id).is_some()).unwrap_or(false);
    if !known {
        return Err(format!("no photo {} in the open folder; list_photos gives the ids", p.id));
    }
    let id = p.id.clone();
    driver.act(cx, move |a, cx| a.open_photo(&id, cx));
    if !driver.settled(cx, false).await {
        let error = driver.read(cx, |a, cx| a.develop.read(cx).error.clone()).flatten();
        return Err(error.unwrap_or_else(|| "the photo didn't render within 60 s".into()));
    }
    let facts = driver.read(cx, |a, cx| a.library.read(cx).record(&p.id).map(facts_of)).flatten().unwrap_or_default();
    let edit = edit::summary(driver, cx, false).unwrap_or_default();
    Ok(Output::json(json!({"id": p.id, "facts": facts, "edit": edit})))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ListPhotos {
    filter: Option<String>,
    offset: Option<usize>,
    limit: Option<usize>,
    detail: Option<bool>,
}

fn edited(record: &PhotoRecord) -> Option<bool> {
    let (state, probed) = (record.state.as_ref()?, record.probed.as_ref()?);
    Some(state.settings != probed.defaults || state.film.is_some())
}

pub fn list_photos(driver: &Driver, cx: &mut AsyncApp, value: Value) -> Answer {
    let p: ListPhotos = args(value)?;
    let filter = p.filter.as_deref().unwrap_or("all");
    if !["all", "picks", "rejected", "edited", "unedited"].contains(&filter) {
        return Err(format!("unknown filter {filter}; use all, picks, rejected, edited or unedited"));
    }
    let (offset, limit, detail) = (p.offset.unwrap_or(0), p.limit.unwrap_or(50).clamp(1, 500), p.detail.unwrap_or(false));
    let table = driver
        .read(cx, |a, cx| {
            let wanted = |r: &&PhotoRecord| match filter {
                "picks" => r.state.as_ref().is_some_and(|s| s.flag == "pick"),
                "rejected" => r.state.as_ref().is_some_and(|s| s.flag == "reject"),
                "edited" => edited(r) == Some(true),
                "unedited" => edited(r) == Some(false),
                _ => true,
            };
            let matching: Vec<&PhotoRecord> = a.library.read(cx).photos.iter().filter(wanted).collect();
            let rows: Vec<Value> = matching
                .iter()
                .skip(offset)
                .take(limit)
                .map(|r| {
                    let facts = facts_of(r);
                    let taken = facts["taken"].as_str().map_or(String::new(), |t| t.chars().skip(5).collect());
                    let flag = match r.state.as_ref().map(|s| s.flag.as_str()) {
                        Some(f @ ("pick" | "reject")) => f,
                        _ => "",
                    };
                    let film = r.state.as_ref().and_then(|s| s.film()).and_then(|f| f.negative).map(|id| a.stocks.iter().find(|s| s.id == id).map_or(id, |s| s.name.clone()));
                    let mut row = vec![json!(r.file.id), json!(taken), json!(flag), json!(r.state.as_ref().map_or(0, |s| s.rating)), json!(edited(r).unwrap_or(false)), json!(film.unwrap_or_default())];
                    if detail {
                        row.extend([facts["camera"].clone(), facts["exposure"].clone(), facts["size"].clone()].map(|v| if v.is_null() { json!("") } else { v }));
                    }
                    Value::Array(row)
                })
                .collect();
            let mut columns = vec!["id", "taken", "flag", "rating", "edited", "film"];
            if detail {
                columns.extend(["camera", "exposure", "size"]);
            }
            let more = offset + rows.len() < matching.len();
            json!({"total": matching.len(), "offset": offset, "columns": columns, "rows": rows, "more": more.then_some(true)})
        })
        .ok_or("Filmlab's window is gone")?;
    Ok(Output::json(strip(table)))
}

// why: `"label": null` clears the label while an absent `label` leaves it, so the two must stay apart
fn present<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Option<String>>, D::Error> {
    Option::deserialize(d).map(Some)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetFlags {
    ids: Vec<String>,
    flag: Option<String>,
    rating: Option<u8>,
    #[serde(default, deserialize_with = "present")]
    label: Option<Option<String>>,
}

pub async fn set_flags(driver: &Driver, cx: &mut AsyncApp, value: Value) -> Answer {
    let p: SetFlags = args(value)?;
    let flag = match p.flag.as_deref() {
        None => None,
        Some(f @ ("pick" | "reject" | "none")) => Some(f.to_string()),
        Some(other) => return Err(format!("unknown flag {other}; use pick, reject or none")),
    };
    if p.rating.is_some_and(|r| r > 5) {
        return Err("rating is 0-5".into());
    }
    if flag.is_none() && p.rating.is_none() && p.label.is_none() {
        return Err("give a flag, rating or label to set".into());
    }
    let gone = "Filmlab's window is gone";
    let (known, unknown): (Vec<String>, Vec<String>) = driver
        .read(cx, |a, cx| {
            let library = a.library.read(cx);
            p.ids.iter().cloned().partition(|id| library.record(id).is_some())
        })
        .ok_or(gone)?;
    if known.is_empty() {
        return Err(format!("no photo {} in the open folder; list_photos gives the ids", unknown.join(", ")));
    }
    let ids = known.clone();
    let (rating, label) = (p.rating, p.label);
    let waiting = driver
        .act(cx, move |a, cx| {
            a.library.update(cx, |l, cx| {
                let indexed = |id: &String| l.record(id).is_some_and(|r| r.state.is_some());
                let (ready, waiting): (Vec<String>, Vec<String>) = ids.into_iter().partition(indexed);
                if let Some(flag) = &flag {
                    l.set_flag(&ready, flag, cx);
                }
                if let Some(rating) = rating {
                    l.set_rating(&ready, rating, cx);
                }
                if let Some(label) = label {
                    l.set_label(&ready, label.filter(|l| !l.trim().is_empty()), cx);
                }
                waiting
            })
        })
        .ok_or(gone)?;
    let saved = driver.wait(cx, 10, |a, cx| a.library.read(cx).saving != crate::library::Saving::Saving).await;
    let error = driver.read(cx, |a, cx| a.library.read(cx).save_error.clone()).flatten();
    Ok(Output::json(strip(json!({
        "updated": known.len() - waiting.len(),
        "unknown": unknown,
        "still_indexing": waiting,
        "error": error.or_else(|| (!saved).then(|| "saving to the sidecars is slow; check list_photos".to_string())),
    }))))
}
