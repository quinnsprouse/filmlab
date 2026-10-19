use std::path::PathBuf;

use filmlab_engine::core::Engine;
use gpui_kit::{AppContext, AsyncApp};
use serde::Deserialize;
use serde_json::{Value, json};

use super::app::strip;
use super::{Answer, Output, args};
use crate::ui::driver::Driver;
use crate::ui::export::run::{self, Choices, Format};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Export {
    ids: Option<Vec<String>>,
    format: Option<String>,
    long_edge: Option<u32>,
    quality: Option<u32>,
    folder: Option<String>,
    name: Option<String>,
}

pub async fn export(driver: &Driver, cx: &mut AsyncApp, value: Value) -> Answer {
    let p: Export = args(value)?;
    let gone = "Filmlab's window is gone";
    let format = match p.format.as_deref() {
        None | Some("jpeg") => Format::Jpeg,
        Some("png") => Format::Png,
        Some("tiff") => Format::Tiff,
        Some(other) => return Err(format!("unknown format {other}; use jpeg, png or tiff")),
    };
    let choices = Choices { format, long_edge: p.long_edge, quality: p.quality.unwrap_or(90).clamp(1, 100) };
    let folder = p.folder.map(|f| match f.strip_prefix("~/") {
        Some(rest) => std::env::var_os("HOME").map_or_else(|| PathBuf::from(&f), |home| PathBuf::from(home).join(rest)),
        None => PathBuf::from(f),
    });
    let given = p.ids;
    let name = p.name.map(|n| n.trim().trim_end_matches(&['.', ' '][..]).to_string()).filter(|n| !n.is_empty() && !n.contains('/'));
    let planned = driver
        .act(cx, move |a, cx| {
            let ids = match given {
                Some(ids) => ids,
                None => {
                    let open = a.develop.read(cx).photo_id.clone();
                    open.or_else(|| a.selected.clone()).into_iter().collect()
                }
            };
            if ids.is_empty() {
                return Err("no photo is open or selected; give ids".to_string());
            }
            a.develop.update(cx, |d, cx| d.flush(cx));
            Ok(run::jobs_for(a.library.read(cx), a.develop.read(cx), &ids, &choices))
        })
        .ok_or(gone)??;
    let (files, errors) = cx
        .background_spawn(async move {
            let mut engine = Engine::new();
            let (mut files, mut errors) = (Vec::new(), Vec::new());
            let single = planned.len() == 1;
            for (id, job) in planned {
                let job = job.map(|mut job| {
                    if let (true, Some(name)) = (single, &name) {
                        job.stem.clone_from(name);
                    }
                    job
                });
                let exported = job.and_then(|job| {
                    let decoded = job.loupe().and_then(run::Loupe::decoded);
                    crate::perf::initiated(|| run::export_one(&mut engine, &job, &choices, folder.as_deref(), decoded))
                });
                match exported {
                    Ok(path) => files.push(path.display().to_string()),
                    Err(e) => errors.push(format!("{id}: {e}")),
                }
            }
            (files, errors)
        })
        .await;
    Ok(Output::json(strip(json!({"files": files, "errors": errors}))))
}
