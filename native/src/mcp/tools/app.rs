use std::rc::Rc;

use gpui_kit::{AppContext, AsyncApp};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Answer, Output, args};
use crate::mcp::session::Hub;
use crate::camera::{Camera, Zoom};
use crate::develop::{Compare, Status, Tool};
use crate::ui::app::Mode;
use crate::ui::driver::Driver;

pub fn tool_name(tool: Tool) -> &'static str {
    match tool {
        Tool::Edit => "edit",
        Tool::Crop => "crop",
        Tool::Heal => "heal",
        Tool::Masks => "masks",
    }
}

pub fn status(hub: &Hub, driver: &Driver, cx: &mut AsyncApp) -> Answer {
    let sessions = hub.live();
    let value = driver
        .read(cx, |a, cx| {
            let library = a.library.read(cx);
            let d = a.develop.read(cx);
            let indexing = library.photos.iter().filter(|r| r.thumbnail.is_none() && r.error.is_none()).count();
            let photo = d.photo_id.as_ref().map(|id| {
                let name = library.record(id).map_or(id.as_str(), |r| r.file.name.as_str());
                json!({"id": id, "name": name, "size": [d.native.0.round(), d.native.1.round()]})
            });
            let render = match d.status {
                Status::Idle => "idle",
                Status::Loading => "loading",
                Status::Ready => "ready",
                Status::Failed => "failed",
            };
            let zoomed = a.is_zoomed(cx);
            let zoom = if zoomed { json!((d.camera.scale * 1000.0).round() / 10.0) } else { json!("fit") };
            let seen = zoomed.then(|| {
                let r = crate::develop::plans::visible_region(d.camera, d.native, d.viewport);
                [r.u0, r.v0, r.u1, r.v1].map(|v| (v * 1000.0).round() / 1000.0)
            });
            let last = d.history.get(d.cursor).map(|s| s.label.clone());
            json!({
                "mode": if a.mode == Mode::Develop { "develop" } else { "library" },
                "folder": library.folder,
                "photos": library.photos.len(),
                "indexing": indexing,
                "selected": a.browse.selection,
                "photo": photo,
                "tool": tool_name(d.tool),
                "zoom": zoom,
                "view_region": seen,
                "compare": if d.compare == Compare::Before { "before" } else { "after" },
                "history": {"step": d.cursor, "steps": d.history.len(), "last": last},
                "render": render,
                "error": d.error,
                "sessions": sessions,
            })
        })
        .ok_or("Filmlab's window is gone")?;
    Ok(Output::json(strip(value)))
}

/// Drops nulls, empty arrays and empty objects so answers stay short.
pub fn strip(value: Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(k, v)| (k, strip(v)))
                .filter(|(_, v)| !(v.is_null() || v.as_array().is_some_and(Vec::is_empty) || v.as_object().is_some_and(serde_json::Map::is_empty)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.into_iter().map(strip).collect()),
        other => other,
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct View {
    mode: Option<String>,
    tool: Option<String>,
    zoom: Option<Value>,
    at: Option<[f64; 2]>,
    compare: Option<String>,
    select: Option<Vec<String>>,
    rail: Option<bool>,
}

// why: zoom is "fit", "100" or a scale ("2", 2), centred on `at` or the current centre; the {scale,x,y} object
// stays accepted
fn zoom_of(value: &Value, at: Option<[f64; 2]>, current: Camera) -> Result<Zoom, String> {
    let [cx, cy] = at.map_or([current.cx, current.cy], |p| p.map(|v| v.clamp(0.0, 1.0)));
    let scale = |s: f64| if s > 0.0 { Ok(Zoom::At { scale: s, cx, cy }) } else { Err("zoom scale must be above 0".to_string()) };
    match value {
        Value::String(s) if s == "fit" => Ok(Zoom::Fit),
        Value::String(s) if s == "100" => scale(1.0),
        Value::String(s) if s.trim_end_matches('x').parse::<f64>().is_ok() => scale(s.trim_end_matches('x').parse().unwrap_or(1.0)),
        Value::Number(n) => scale(n.as_f64().unwrap_or(1.0)),
        Value::Object(o) => {
            let number = |key: &str, default: f64| o.get(key).map_or(Some(default), Value::as_f64);
            let (Some(scale), Some(x), Some(y)) = (number("scale", 1.0), number("x", 0.5), number("y", 0.5)) else {
                return Err("zoom {scale,x,y} takes numbers".into());
            };
            Ok(Zoom::At { scale, cx: x, cy: y })
        }
        _ => Err("zoom is \"fit\", \"100\" or a scale like \"2\"".into()),
    }
}

pub async fn view(hub: &Rc<Hub>, driver: &Driver, cx: &mut AsyncApp, value: Value) -> Answer {
    let p: View = args(value)?;
    let gone = "Filmlab's window is gone";
    if let Some(ids) = &p.select {
        let unknown = driver.read(cx, |a, cx| ids.iter().filter(|id| a.library.read(cx).record(id).is_none()).cloned().collect::<Vec<_>>()).ok_or(gone)?;
        if !unknown.is_empty() {
            return Err(format!("no photo {} in the open folder; list_photos gives the ids", unknown.join(", ")));
        }
    }
    let mode = match p.mode.as_deref() {
        None => None,
        Some("library") => Some(Mode::Browse),
        Some("develop") => Some(Mode::Develop),
        Some(other) => return Err(format!("unknown mode {other}; use library or develop")),
    };
    let tool = match p.tool.as_deref() {
        None => None,
        Some("edit") => Some(Tool::Edit),
        Some("crop") => Some(Tool::Crop),
        Some("heal") => Some(Tool::Heal),
        Some("masks") => Some(Tool::Masks),
        Some(other) => return Err(format!("unknown tool {other}; use edit, crop, heal or masks")),
    };
    let compare = match p.compare.as_deref() {
        None => None,
        Some("before") => Some(Compare::Before),
        Some("after") => Some(Compare::After),
        Some(other) => return Err(format!("unknown compare {other}; use before or after")),
    };
    let _window = hub.window().await;
    let develops = mode == Some(Mode::Develop);
    if develops {
        let opens = driver.read(cx, |a, cx| a.develop.read(cx).photo_id.is_none().then(|| a.selected.clone())).ok_or(gone)?;
        if opens == Some(None) {
            return Err("no photo is open or selected; open_photo first".into());
        }
        // why: a photo an agent has open off screen finishes its running call and is saved before Develop reads it
        let adopting = match opens.flatten() {
            Some(id) => Some((hub.photo(&id).await, id)),
            None => None,
        };
        driver
            .act(cx, |a, cx| match a.develop.read(cx).photo_id.is_none().then(|| a.selected.clone()).flatten() {
                Some(id) => a.open_photo(&id, cx),
                None => a.set_mode(Mode::Develop, cx),
            })
            .ok_or(gone)?;
        if let Some((_, id)) = &adopting {
            hub.hand_over(driver, cx, id);
        }
    }
    let in_develop = driver.read(cx, |a, _| a.mode == Mode::Develop).ok_or(gone)?;
    if (tool.is_some() || p.zoom.is_some() || compare.is_some()) && !(develops || (mode.is_none() && in_develop)) {
        return Err("tool, zoom and compare need a photo in Develop; view mode develop, or open_photo".into());
    }
    let zoom = match &p.zoom {
        Some(z) => {
            let camera = driver.read(cx, |a, cx| a.develop.read(cx).camera).ok_or(gone)?;
            Some(zoom_of(z, p.at, camera)?)
        }
        None => None,
    };
    driver
        .act(cx, move |a, cx| {
            if mode == Some(Mode::Browse) {
                a.set_mode(Mode::Browse, cx);
            }
            if let Some(tool) = tool
                && a.develop.read(cx).tool != tool
            {
                a.set_tool(tool, cx);
            }
            a.develop.update(cx, |d, cx| {
                if let Some(zoom) = zoom {
                    d.set_zoom(zoom, false, false, cx);
                }
                if let Some(compare) = compare {
                    d.set_compare(compare, cx);
                }
            });
            if let Some(ids) = p.select {
                a.selected = ids.first().cloned();
                a.browse.selection = ids;
            }
            if let Some(open) = p.rail {
                a.rail_open = open;
            }
            cx.notify();
        })
        .ok_or(gone)?;
    let watching = driver.read(cx, |a, _| a.mode == Mode::Develop).ok_or(gone)?;
    if watching && (zoom.is_some() || compare.is_some() || develops) {
        let detail = driver.read(cx, |a, cx| a.is_zoomed(cx)).unwrap_or(false);
        driver.settled(cx, detail).await;
    }
    status(hub, driver, cx)
}

pub async fn screenshot(hub: &Hub, driver: &Driver, cx: &mut AsyncApp, value: Value) -> Answer {
    let p: Shot = args(value)?;
    let _window = hub.window().await;
    let edge = p.max_edge.unwrap_or(1280).clamp(256, 2048);
    let number = driver
        .on_window(cx, |window, _| crate::glass::renderer_view(window).and_then(|v| v.window()).map(|w| w.windowNumber()))
        .flatten()
        .ok_or("Filmlab's window is gone")?;
    let file = std::env::temp_dir().join(format!("filmlab-mcp-{}.jpg", std::process::id()));
    let target = file.clone();
    let taken = cx
        .background_spawn(async move {
            let status = std::process::Command::new("screencapture").args(["-x", "-o", "-t", "jpg", &format!("-l{number}")]).arg(&target).status();
            let bytes = std::fs::read(&target);
            let _ = std::fs::remove_file(&target);
            match (status, bytes) {
                (Ok(s), Ok(bytes)) if s.success() && !bytes.is_empty() => Ok(bytes),
                _ => Err("the window couldn't be captured; is Screen Recording allowed for Filmlab?".to_string()),
            }
        })
        .await?;
    let image = image::load_from_memory_with_format(&taken, image::ImageFormat::Jpeg).map_err(|e| e.to_string())?;
    let image = if image.width().max(image.height()) > edge { image.resize(edge, edge, image::imageops::FilterType::Triangle) } else { image };
    let (width, height) = (image.width(), image.height());
    let mut out = std::io::Cursor::new(Vec::new());
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 75).encode(image.to_rgb8().as_raw(), width, height, image::ExtendedColorType::Rgb8).map_err(|e| e.to_string())?;
    Ok(Output::json(json!({"size": [width, height]})).with_image(out.into_inner()))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Shot {
    max_edge: Option<u32>,
}
