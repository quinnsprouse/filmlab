mod app;
mod edit;
mod guide;
mod export;
mod library;
mod look;

use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::AsyncApp;
use rmcp::model::{Tool, ToolAnnotations};
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::session::Hub;
use crate::ui::driver::Driver;

/// A tool's answer: compact text (usually one line of JSON) and any JPEG images.
pub struct Output {
    pub text: String,
    pub images: Vec<Vec<u8>>,
    pub error: bool,
}

impl Output {
    pub fn json(value: Value) -> Output {
        Output { text: value.to_string(), images: Vec::new(), error: false }
    }

    pub fn error(message: impl Into<String>) -> Output {
        Output { text: message.into(), images: Vec::new(), error: true }
    }

    pub fn with_image(mut self, jpeg: Vec<u8>) -> Output {
        self.images.push(jpeg);
        self
    }
}

pub type Answer = Result<Output, String>;

#[derive(Clone, Copy)]
enum Kind {
    /// reads only
    Read,
    /// changes the edit or the app; repeating it changes nothing more
    Set,
    /// changes the edit or the app each time
    Act,
}

struct Spec {
    name: &'static str,
    kind: Kind,
    description: &'static str,
    schema: &'static str,
}

// why: the tool list is sent to the model on every turn, so descriptions are short and schemas carry only what an
// agent needs (argument checking happens when each tool parses its arguments); detail lives behind `guide` and
// `catalog`
const SPECS: &[Spec] = &[
    Spec {
        name: "guide",
        kind: Kind::Read,
        description: "How to edit with Filmlab, by topic. Read workflow before editing.",
        schema: r#"{"type":"object","properties":{"topic":{"enum":["workflow","film","controls","masks","crop","qa"]}},"required":["topic"]}"#,
    },
    Spec {
        name: "status",
        kind: Kind::Read,
        description: "Where the app is: mode, folder, photo, selection, tool, zoom, the region the user is viewing, history, agent sessions.",
        schema: r#"{"type":"object"}"#,
    },
    Spec {
        name: "open_folder",
        kind: Kind::Act,
        description: "Open a folder of photos in the library and wait for indexing.",
        schema: r#"{"type":"object","properties":{"path":{"type":"string","description":"~ ok"},"wait":{"type":"boolean","description":"default true"}},"required":["path"]}"#,
    },
    Spec {
        name: "list_photos",
        kind: Kind::Read,
        description: "Photos in the open folder as a table: id, date, flag, rating, edited, film. Paged.",
        schema: r#"{"type":"object","properties":{"filter":{"enum":["all","picks","rejected","edited","unedited"]},"offset":{"type":"integer"},"limit":{"type":"integer","description":"default 50"},"detail":{"type":"boolean","description":"add camera, exposure, size"}}}"#,
    },
    Spec {
        name: "set_flags",
        kind: Kind::Set,
        description: "Set flag, star rating or colour label on photos.",
        schema: r#"{"type":"object","properties":{"ids":{"type":"array","items":{"type":"string"}},"flag":{"enum":["pick","reject","none"]},"rating":{"type":"integer","minimum":0,"maximum":5},"label":{"type":"string","description":"\"\" clears"}},"required":["ids"]}"#,
    },
    Spec {
        name: "open_photo",
        kind: Kind::Act,
        description: "Open a photo in Develop and wait for it to render. Returns its facts and edit.",
        schema: r#"{"type":"object","properties":{"id":{"type":"string"}},"required":["id"]}"#,
    },
    Spec {
        name: "get_edit",
        kind: Kind::Read,
        description: "A photo's edit: non-default controls, film, crop, masks, spots. all: every control.",
        schema: r#"{"type":"object","properties":{"photo":{"type":"string","description":"id; default the open photo"},"all":{"type":"boolean"}}}"#,
    },
    Spec {
        name: "set_controls",
        kind: Kind::Set,
        description: "Set develop controls by id (catalog controls) in one undo step. Numbers are absolute, strings like \"+0.3\" relative.",
        schema: r#"{"type":"object","properties":{"photo":{"type":"string"},"values":{"type":"object","description":"{\"light.exposure\":0.4,\"color.vibrance\":\"+10\"}"},"label":{"type":"string"},"preview":{"type":"integer","description":"also return a JPEG this long, e.g. 768"}},"required":["values"]}"#,
    },
    Spec {
        name: "set_film",
        kind: Kind::Set,
        description: "Change the FilmVision look; omitted fields stay, negative \"\" removes it. gauge: FilmVision's grain, halation and blur for 16/35/65/120.",
        schema: r#"{"type":"object","properties":{"photo":{"type":"string"},"negative":{"type":"string"},"print":{"type":"string"},"utility":{"type":"string"},"gauge":{"enum":[16,35,65,120]},"halation":{"type":"object","description":"on, amount, hue, sat, radius, highlights_only"},"grain":{"type":"object","description":"amount, size, softness, defocus"},"blur":{"type":"number"},"dust":{"type":"object","description":"on, size, softness"},"preview":{"type":"integer"}}}"#,
    },
    Spec {
        name: "command",
        kind: Kind::Act,
        description: "Run a LightCraft command (crop.*, mask.*, spot.*, develop.*; see catalog commands) on a photo as one undo step.",
        schema: r#"{"type":"object","properties":{"photo":{"type":"string"},"name":{"type":"string"},"params":{"type":"object"},"preview":{"type":"integer"}},"required":["name"]}"#,
    },
    Spec {
        name: "catalog",
        kind: Kind::Read,
        description: "Reference lists: controls (ids, ranges, defaults), commands (names, params), stocks. filter: prefix or section.",
        schema: r#"{"type":"object","properties":{"kind":{"enum":["controls","commands","stocks"]},"filter":{"type":"string"}},"required":["kind"]}"#,
    },
    Spec {
        name: "history",
        kind: Kind::Act,
        description: "List a photo's history, undo, redo, or go to a step.",
        schema: r#"{"type":"object","properties":{"photo":{"type":"string"},"action":{"enum":["list","undo","redo","go_to"]},"step":{"type":"integer"}},"required":["action"]}"#,
    },
    Spec {
        name: "profiles",
        kind: Kind::Act,
        description: "Saved looks: list, save the current edit, apply at an amount (0-200, default 100), delete.",
        schema: r#"{"type":"object","properties":{"photo":{"type":"string"},"action":{"enum":["list","save","apply","delete"]},"id":{"type":"string"},"name":{"type":"string"},"groups":{"type":"array","items":{"type":"string"},"description":"default: the look groups"},"amount":{"type":"number"}},"required":["action"]}"#,
    },
    Spec {
        name: "snapshots",
        kind: Kind::Act,
        description: "Named states of a photo's edit: save, list, restore (one undo step), delete.",
        schema: r#"{"type":"object","properties":{"photo":{"type":"string"},"action":{"enum":["save","list","restore","delete"]},"name":{"type":"string"}},"required":["action"]}"#,
    },
    Spec {
        name: "preview",
        kind: Kind::Read,
        description: "JPEG of a photo as edited (before: as opened) with brief stats; region [x0,y0,x1,y1] (0-1) shows that area at full detail.",
        schema: r#"{"type":"object","properties":{"photo":{"type":"string"},"max_edge":{"type":"integer","description":"64-2048, default 768"},"region":{"type":"array","items":{"type":"number"}},"before":{"type":"boolean"},"quality":{"type":"integer","description":"default 80"}}}"#,
    },
    Spec {
        name: "analyze",
        kind: Kind::Read,
        description: "Numbers instead of pixels: luma percentiles, clipping, mean colour, neutral cast, chroma, histogram. Optional region and before.",
        schema: r#"{"type":"object","properties":{"photo":{"type":"string"},"region":{"type":"array","items":{"type":"number"}},"before":{"type":"boolean"}}}"#,
    },
    Spec {
        name: "view",
        kind: Kind::Set,
        description: "Change what the window shows: mode, tool, zoom, before/after, library selection, rail.",
        schema: r#"{"type":"object","properties":{"mode":{"enum":["library","develop"]},"tool":{"enum":["edit","crop","heal","masks"]},"zoom":{"type":"string","description":"fit, 100 or a scale like 2"},"at":{"type":"array","items":{"type":"number"},"description":"zoom centre [x, y], 0-1"},"compare":{"enum":["before","after"]},"select":{"type":"array","items":{"type":"string"}},"rail":{"type":"boolean"}}}"#,
    },
    Spec {
        name: "screenshot",
        kind: Kind::Read,
        description: "JPEG of the Filmlab window as the user sees it. For the photo use preview.",
        schema: r#"{"type":"object","properties":{"max_edge":{"type":"integer","description":"default 1280"}}}"#,
    },
    Spec {
        name: "export",
        kind: Kind::Act,
        description: "Export photos with edits and metadata (default: the open photo) to any folder or <photo folder>/Exports; never overwrites.",
        schema: r#"{"type":"object","properties":{"ids":{"type":"array","items":{"type":"string"}},"format":{"enum":["jpeg","png","tiff"]},"long_edge":{"type":"integer"},"quality":{"type":"integer","description":"1-100, default 90"},"folder":{"type":"string","description":"~ ok; created if missing"},"name":{"type":"string","description":"file name, one photo"}}}"#,
    },
];

pub fn definitions() -> Vec<Tool> {
    SPECS
        .iter()
        .map(|s| {
            let schema: serde_json::Map<String, Value> = serde_json::from_str(s.schema).unwrap_or_default();
            // why: only the hints whose spec defaults are wrong for Filmlab (destructive and open-world default to true)
            let annotations = match s.kind {
                Kind::Read => ToolAnnotations::new().read_only(true),
                Kind::Set => ToolAnnotations::new().destructive(false).idempotent(true),
                Kind::Act => ToolAnnotations::new().destructive(false),
            };
            Tool::new(s.name, s.description, Arc::new(schema)).with_annotations(annotations.open_world(false))
        })
        .collect()
}

pub fn exists(name: &str) -> bool {
    SPECS.iter().any(|s| s.name == name)
}

pub fn args<T: DeserializeOwned>(value: Value) -> Result<T, String> {
    serde_json::from_value(value).map_err(|e| format!("bad arguments: {e}"))
}

/// Runs one call. Calls arrive concurrently; each tool takes the hub's locks it needs (the window for what the user
/// sees, the photo for edits), so only calls on the same photo or on the window wait for each other.
pub async fn run(hub: &Rc<Hub>, driver: &Driver, cx: &mut AsyncApp, name: &str, value: Value) -> Output {
    let answer = match name {
        "guide" => guide::guide(value),
        "status" => app::status(hub, driver, cx),
        "screenshot" => app::screenshot(hub, driver, cx, value).await,
        "view" => app::view(hub, driver, cx, value).await,
        "open_folder" => library::open_folder(hub, driver, cx, value).await,
        "list_photos" => library::list_photos(driver, cx, value),
        "set_flags" => library::set_flags(driver, cx, value).await,
        "open_photo" => library::open_photo(hub, driver, cx, value).await,
        "get_edit" => edit::get_edit(hub, driver, cx, value),
        "set_controls" => edit::set_controls(hub, driver, cx, value).await,
        "set_film" => edit::set_film(hub, driver, cx, value).await,
        "command" => edit::command(hub, driver, cx, value).await,
        "catalog" => edit::catalog(driver, cx, value),
        "history" => edit::history(hub, driver, cx, value).await,
        "profiles" => edit::profiles(hub, driver, cx, value).await,
        "snapshots" => edit::snapshots(hub, driver, cx, value).await,
        "preview" => look::preview(hub, driver, cx, value).await,
        "analyze" => look::analyze(hub, driver, cx, value).await,
        "export" => export::export(driver, cx, value).await,
        _ => Err(format!("unknown tool {name}")),
    };
    answer.unwrap_or_else(Output::error)
}
