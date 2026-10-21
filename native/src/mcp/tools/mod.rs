mod app;
mod chart;
mod compare;
mod curve;
mod edit;
mod effect;
mod export;
mod fit;
mod guide;
mod library;
pub mod look;
mod paste;
mod scopes;
mod surface;

use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::{AsyncApp, Task};
use rmcp::model::{Tool, ToolAnnotations};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use super::session::Hub;
use crate::ui::driver::Driver;

/// A tool's answer: compact text (usually one line of JSON) and any JPEG images.
#[derive(Clone)]
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
    /// can delete something the user made
    Removes,
}

struct Spec {
    name: &'static str,
    kind: Kind,
    description: &'static str,
    schema: &'static str,
}

// why: the tool list is sent to the model on every turn, so there are few tools, descriptions are short and schemas
// carry only what an agent needs often; the arguments of rare operations live behind `guide schema`, and each
// operation checks its own arguments before it does anything
const SPECS: &[Spec] = &[
    Spec {
        name: "guide",
        kind: Kind::Read,
        description: "How to work with Filmlab (read workflow first), control ids, stocks, metrics, and the arguments of rare operations: topic schema, filter photos.list, view, command.mask...",
        schema: r#"{"type":"object","properties":{"topic":{"enum":["workflow","film","controls","curves","masks","crop","metrics","schema"]},"filter":{"type":"string"}},"required":["topic"]}"#,
    },
    Spec {
        name: "status",
        kind: Kind::Read,
        description: "The app (part app, the default), or a photo's edit with its state digest (state), every control (controls), history or snapshots. photo defaults to the open one.",
        schema: r#"{"type":"object","properties":{"part":{"enum":["app","state","controls","history","snapshots"]},"photo":{"type":"string"}}}"#,
    },
    Spec {
        name: "photos",
        kind: Kind::Act,
        description: "The library: open a folder, list photos, flag/rate, open one in the window, export (never overwrites). args: guide schema photos.<action>.",
        schema: r#"{"type":"object","properties":{"action":{"enum":["folder","list","flags","open","export"]},"args":{"type":"object"}},"required":["action"]}"#,
    },
    Spec {
        name: "edit",
        kind: Kind::Act,
        description: "Change photos, one undo step each, answered with the effect: words and only the numbers that moved. values (numbers absolute, \"+0.3\" relative), curve and film together; or fit a control to a measure; or paste another photo's look (no profile); or an action. photo: id or ids, default the open one.",
        schema: r#"{"type":"object","properties":{"photo":{"type":["string","array"],"items":{"type":"string"}},"values":{"type":"object","description":"control ids: guide controls"},"film":{"type":"object","description":"negative, print (null: off), gauge 16|35|65|120; guide schema film"},"curve":{"type":"object","description":"guide curves"},"fit":{"type":"object","properties":{"control":{"type":"string"},"measure":{"type":"string","description":"guide metrics"},"to":{"type":"number"},"like":{"type":"string"},"max_delta":{"type":"number"}},"required":["control","measure"]},"paste":{"type":"object","properties":{"from":{"type":"string"},"groups":{"type":"array","items":{"type":"string"}},"amount":{"type":"number"},"match":{"type":"array","items":{"enum":["exposure","wb"]}}}},"region":{"type":"array","items":{"type":"number"},"minItems":4,"maxItems":4},"watch":{"type":"array","items":{"type":"string"}},"if_state":{"type":"string"},"preview":{"type":["integer","boolean"]},"label":{"type":"string"},"action":{"enum":["command","undo","redo","go_to","snapshot.save","snapshot.restore","snapshot.delete"]},"args":{"type":"object"},"request_id":{"type":"string","description":"new for each change"},"retry":{"type":"boolean","description":"resending a lost one: its answer or outcome unknown, never a second run"}}}"#,
    },
    Spec {
        name: "look",
        kind: Kind::Read,
        description: "See photos without changing them. preview: JPEG (several: a numbered sheet). analyze: numbers, or scopes. compare: two states diffed, heat or pair. trials: the photo now beside up to 8 alternatives. screenshot: the window. region [x0,y0,x1,y1] 0-1: that area at full detail.",
        schema: r#"{"type":"object","properties":{"mode":{"enum":["preview","analyze","compare","trials","screenshot"]},"photo":{"type":["string","array"],"items":{"type":"string"}},"region":{"type":"array","items":{"type":"number"},"minItems":4,"maxItems":4},"edge":{"type":"integer"},"scopes":{"type":"array","items":{"enum":["glance","zones","hues","skin","clip","tail","wave","texture","curve"]}},"from":{"type":["string","integer"],"description":"step, previous, current, before or a snapshot"},"to":{"type":["string","integer"]},"image":{"enum":["heat","pair","none","curve"]},"try":{"type":"array","items":{"type":"object","properties":{"values":{"type":"object"},"film":{"type":"object"},"curve":{"type":"object"}}}},"args":{"type":"object"}},"required":["mode"]}"#,
    },
    Spec {
        name: "view",
        kind: Kind::Set,
        description: "Change what the window shows: mode, tool, zoom, before/after, selection. args: guide schema view.",
        schema: r#"{"type":"object","properties":{"args":{"type":"object"}},"required":["args"]}"#,
    },
    Spec {
        name: "profiles",
        kind: Kind::Removes,
        description: "The user's saved looks: list and apply. Save or delete only when the user asks; to carry a look, edit paste. args: guide schema profiles.",
        schema: r#"{"type":"object","properties":{"action":{"enum":["list","save","apply","delete"]},"args":{"type":"object"}},"required":["action"]}"#,
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
                Kind::Removes => ToolAnnotations::new().destructive(true),
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

/// The operations that take several photos and answer for each in turn.
const EACH: [&str; 7] = ["set_controls", "set_film", "change", "command", "fit", "history", "snapshots"];

/// A batch's answer after `head`: every photo's, and whether all, some or none of them were done.
pub fn gathered(mut head: Value, results: Vec<Value>, errors: Vec<String>) -> Output {
    let status = match (results.is_empty(), errors.is_empty()) {
        (_, true) => None,
        (false, false) => Some("partial"),
        (true, false) => Some("failed"),
    };
    head["status"] = json!(status);
    head["results"] = json!(results);
    head["errors"] = json!(errors);
    let out = Output::json(app::strip(head));
    Output { error: status == Some("failed"), ..out }
}

/// Runs a call that names several photos as one call per photo, a few at a time, and gathers their answers.
async fn each(hub: &Rc<Hub>, driver: &Driver, cx: &mut AsyncApp, name: &str, value: Value) -> Answer {
    let mut ids: Vec<String> = serde_json::from_value(value["photo"].clone()).map_err(|_| "photo is an id or a list of ids")?;
    let mut seen = std::collections::HashSet::new();
    ids.retain(|id| seen.insert(id.clone()));
    if ids.is_empty() {
        return Err("photo is an empty list".into());
    }
    if value.get("preview").is_some() {
        return Err("preview shows one photo; for several, call preview with the same photo list".into());
    }
    let tasks: Vec<Task<Output>> = ids
        .iter()
        .map(|id| {
            let mut one = value.clone();
            one["photo"] = json!(id);
            let (hub, driver, name) = (hub.clone(), driver.clone(), name.to_string());
            cx.spawn(async move |cx| {
                let _turn = hub.turn().await;
                let call: Pin<Box<dyn Future<Output = Answer>>> = Box::pin(operate(&hub, &driver, cx, &name, one));
                call.await.unwrap_or_else(Output::error)
            })
        })
        .collect();
    let mut results = Vec::new();
    let mut errors = Vec::new();
    for (id, task) in ids.iter().zip(tasks) {
        let out = task.await;
        if out.error {
            errors.push(format!("{id}: {}", out.text));
        } else {
            let mut answer: Value = serde_json::from_str(&out.text).unwrap_or(Value::String(out.text));
            if let Value::Object(o) = &mut answer {
                o.insert("photo".into(), json!(id));
            }
            results.push(answer);
        }
    }
    Ok(gathered(json!({}), results, errors))
}

/// Runs one call. Calls arrive concurrently; each operation takes the hub's locks it needs (the window for what the
/// user sees, the photo for edits), so only calls on the same photo or on the window wait for each other.
pub async fn run(hub: &Rc<Hub>, driver: &Driver, cx: &mut AsyncApp, name: &str, mut value: Value) -> Output {
    let request = value.as_object_mut().and_then(|o| Some((o.remove("request_id")?, o.remove("retry"))));
    match request {
        Some((Value::String(id), None | Some(Value::Null))) => once(hub, driver, cx, name, value, &id, false).await,
        Some((Value::String(id), Some(Value::Bool(retry)))) => once(hub, driver, cx, name, value, &id, retry).await,
        Some((Value::String(_), Some(_))) => Output::error("retry is true or false"),
        Some(_) => Output::error("request_id is a string the caller makes up, new for each change"),
        None if value.get("retry").is_some() => Output::error("retry resends a request_id; give the request_id too"),
        None => call(hub, driver, cx, name, value).await,
    }
}

/// Runs a call that names itself with a request id at most once: the same request again answers with its first
/// answer (waiting for it if it's still running), another request under the same id is refused, and a retry of an id
/// this app doesn't know says the outcome is unknown rather than guess.
async fn once(hub: &Rc<Hub>, driver: &Driver, cx: &mut AsyncApp, name: &str, value: Value, id: &str, retry: bool) -> Output {
    let key = format!("{name}:{id}");
    let body = value.to_string();
    loop {
        match hub.request(&key) {
            Some((seen, _)) if seen != body => return Output::error(format!("request_id {id} was already used for a different request")),
            Some((_, Some(out))) => return out,
            Some((_, None)) => cx.background_executor().timer(std::time::Duration::from_millis(50)).await,
            None if retry => return Output::error(format!("outcome unknown: request {id} isn't known here (never received, or long done); read status before changing anything again")),
            None => break,
        }
    }
    hub.start_request(&key, &body);
    let out = call(hub, driver, cx, name, value).await;
    hub.finish_request(&key, out.clone());
    out
}

async fn call(hub: &Rc<Hub>, driver: &Driver, cx: &mut AsyncApp, name: &str, value: Value) -> Output {
    let answer = match name {
        "guide" => guide::guide(driver, cx, value),
        "status" => surface::status(hub, driver, cx, value).await,
        "photos" => surface::photos(hub, driver, cx, value).await,
        "edit" => surface::edit(hub, driver, cx, value).await,
        "look" => surface::look(hub, driver, cx, value).await,
        "view" => surface::view(hub, driver, cx, value).await,
        "profiles" => surface::profiles(hub, driver, cx, value).await,
        _ => Err(format!("unknown tool {name}")),
    };
    answer.unwrap_or_else(Output::error)
}

/// One of the operations the tools route to, by name; one on several photos runs once per photo.
pub async fn operate(hub: &Rc<Hub>, driver: &Driver, cx: &mut AsyncApp, op: &str, value: Value) -> Answer {
    let several = value["photo"].is_array();
    match op {
        _ if several && EACH.contains(&op) => each(hub, driver, cx, op, value).await,
        "preview" if several => look::preview_many(hub, driver, cx, value).await,
        "analyze" if several => look::analyze_many(hub, driver, cx, value).await,
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
        "change" => edit::change(hub, driver, cx, value).await,
        "command" => edit::command(hub, driver, cx, value).await,
        "fit" => fit::fit(hub, driver, cx, value).await,
        "paste" => paste::paste(hub, driver, cx, value).await,
        "history" => edit::history(hub, driver, cx, value).await,
        "profiles" => edit::profiles(hub, driver, cx, value).await,
        "snapshots" => edit::snapshots(hub, driver, cx, value).await,
        "preview" => look::preview(hub, driver, cx, value).await,
        "analyze" => look::analyze(hub, driver, cx, value).await,
        "compare" => compare::compare(hub, driver, cx, value).await,
        "export" => export::export(driver, cx, value).await,
        _ => Err(format!("unknown operation {op}")),
    }
}
