use std::rc::Rc;

use gpui_kit::AsyncApp;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::{Answer, Output, args, operate};
use crate::mcp::session::{Hub, digest};
use crate::ui::driver::Driver;

/// An operation's arguments without the fields left unset (null at the top); nulls inside them keep their meaning,
/// as film's `negative: null` turns the film off.
fn unset(mut call: Value) -> Value {
    if let Value::Object(o) = &mut call {
        o.retain(|_, v| !v.is_null());
    }
    call
}

/// A tool's `args`, or an empty object.
fn given(args: Option<Value>) -> Value {
    args.filter(|a| !a.is_null()).unwrap_or_else(|| json!({}))
}

/// `args` when they hold only fields named in `allowed`, so nothing a caller puts there is quietly left unread.
fn only_args(args: Option<Value>, allowed: &[&str], what: &str) -> Result<Value, String> {
    let args = given(args);
    let Value::Object(fields) = &args else { return Err("args is an object".into()) };
    match fields.keys().find(|k| !allowed.contains(&k.as_str())) {
        Some(key) if allowed.is_empty() => Err(format!("{what} takes no args, so {key} would go unread")),
        Some(key) => Err(format!("{what} args are {}; {key} would go unread", allowed.join(", "))),
        None => Ok(args),
    }
}

/// An operation's arguments: `frame`, the fields the tool sets itself (unset ones dropped), with `nested`, the
/// caller's object for one part of the request, laid in as it came, nulls and all. A nested field that names one of
/// the frame's own (the photo, the action, the guards) is refused, so the outer request keeps its meaning.
fn laid(frame: Value, nested: Value, part: &str) -> Result<Value, String> {
    let (Value::Object(frame), Value::Object(nested)) = (frame, nested) else { return Err(format!("{part} is an object")) };
    if let Some(key) = nested.keys().find(|k| frame.contains_key(*k)) {
        return Err(format!("{key} goes outside {part}, not inside it"));
    }
    let mut out: Map<String, Value> = frame.into_iter().filter(|(_, v)| !v.is_null()).collect();
    out.extend(nested);
    Ok(Value::Object(out))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Status {
    part: Option<String>,
    photo: Option<String>,
}

pub async fn status(hub: &Rc<Hub>, driver: &Driver, cx: &mut AsyncApp, value: Value) -> Answer {
    let p: Status = args(value)?;
    let photo = p.photo.clone();
    match p.part.as_deref().unwrap_or("app") {
        "app" if photo.is_some() => Err("part app is the window's; give part state, controls, history or snapshots for a photo".into()),
        "app" => operate(hub, driver, cx, "status", json!({})).await,
        "state" => {
            let out = operate(hub, driver, cx, "get_edit", json!({"photo": photo})).await?;
            let state = hub.peek(driver, cx, photo.as_deref(), |e| digest(e.id, &e.settings, e.film))?;
            let mut edit: Value = serde_json::from_str(&out.text).map_err(|e| e.to_string())?;
            edit["state"] = json!(state);
            Ok(Output::json(edit))
        }
        "controls" => operate(hub, driver, cx, "get_edit", json!({"photo": photo, "all": true})).await,
        "history" => operate(hub, driver, cx, "history", json!({"photo": photo, "action": "list"})).await,
        "snapshots" => operate(hub, driver, cx, "snapshots", json!({"photo": photo, "action": "list"})).await,
        other => Err(format!("unknown part {other}; app, state, controls, history or snapshots")),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Act {
    action: String,
    args: Option<Value>,
}

pub async fn photos(hub: &Rc<Hub>, driver: &Driver, cx: &mut AsyncApp, value: Value) -> Answer {
    let p: Act = args(value)?;
    let given = given(p.args);
    let op = match p.action.as_str() {
        "folder" => "open_folder",
        "list" => "list_photos",
        "flags" => "set_flags",
        "open" => "open_photo",
        "export" => "export",
        other => return Err(format!("unknown action {other}; folder, list, flags, open or export (guide schema photos.<action>)")),
    };
    operate(hub, driver, cx, op, given).await
}

pub async fn view(hub: &Rc<Hub>, driver: &Driver, cx: &mut AsyncApp, value: Value) -> Answer {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct View {
        args: Value,
    }
    let p: View = args(value)?;
    operate(hub, driver, cx, "view", p.args).await
}

pub async fn profiles(hub: &Rc<Hub>, driver: &Driver, cx: &mut AsyncApp, value: Value) -> Answer {
    let p: Act = args(value)?;
    operate(hub, driver, cx, "profiles", laid(json!({"action": p.action}), given(p.args), "args")?).await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Look {
    mode: String,
    photo: Option<Value>,
    region: Option<Value>,
    edge: Option<u32>,
    scopes: Option<Vec<String>>,
    from: Option<Value>,
    to: Option<Value>,
    image: Option<String>,
    #[serde(rename = "try")]
    tries: Option<Value>,
    args: Option<Value>,
}

impl Look {
    /// `to: "before"` on a preview or analysis: the photo unedited, without film.
    fn before(&self) -> Result<Option<bool>, String> {
        match &self.to {
            None => Ok(None),
            Some(Value::String(s)) if s == "before" => Ok(Some(true)),
            Some(_) => Err(format!("{} shows the photo now or to: \"before\"; compare takes other states", self.mode)),
        }
    }

    fn only(&self, mode: &str, fields: &[(&str, bool)]) -> Result<(), String> {
        match fields.iter().find(|(_, given)| *given) {
            Some((name, _)) => Err(format!("{mode} doesn't take {name}")),
            None => Ok(()),
        }
    }
}

pub async fn look(hub: &Rc<Hub>, driver: &Driver, cx: &mut AsyncApp, value: Value) -> Answer {
    let mut p: Look = args(value)?;
    let allowed: &[&str] = match p.mode.as_str() {
        "preview" | "trials" => &["quality"],
        "compare" => &["values", "curve", "detail"],
        _ => &[],
    };
    let extra = only_args(p.args.take(), allowed, &format!("look {}", p.mode))?;
    if p.scopes.as_ref().is_some_and(Vec::is_empty) {
        return Err("scopes is empty: name the scopes, or leave scopes out for every number".into());
    }
    let edge = p.edge;
    let call = match p.mode.as_str() {
        "preview" => {
            p.only("preview", &[("scopes", p.scopes.is_some()), ("from", p.from.is_some()), ("image", p.image.is_some()), ("try", p.tries.is_some())])?;
            ("preview", json!({"photo": p.photo, "max_edge": edge, "region": p.region, "before": p.before()?, "quality": extra["quality"]}))
        }
        "trials" => {
            p.only("trials", &[("scopes", p.scopes.is_some()), ("from", p.from.is_some()), ("to", p.to.is_some()), ("image", p.image.is_some()), ("region", p.region.is_some())])?;
            ("preview", json!({"photo": p.photo, "max_edge": edge, "try": p.tries.clone().ok_or("trials needs try: [{values, film}, ...]")?, "quality": extra["quality"]}))
        }
        "analyze" => {
            p.only("analyze", &[("try", p.tries.is_some())])?;
            if (p.scopes.is_some() || p.image.is_some() || p.from.is_some()) && p.photo.as_ref().is_some_and(Value::is_array) {
                return Err("scopes read one photo; several photos get one table of their numbers".into());
            }
            ("analyze", json!({"photo": p.photo, "max_edge": edge, "region": p.region, "before": p.before()?, "scopes": p.scopes, "image": p.image, "from": p.from}))
        }
        "compare" => {
            p.only("compare", &[("scopes", p.scopes.is_some()), ("try", p.tries.is_some())])?;
            ("compare", json!({"photo": p.photo, "from": p.from, "to": p.to, "region": p.region, "max_edge": edge, "image": p.image, "values": extra["values"], "curve": extra["curve"], "detail": extra["detail"]}))
        }
        "screenshot" => {
            p.only("screenshot", &[("photo", p.photo.is_some()), ("region", p.region.is_some()), ("scopes", p.scopes.is_some()), ("try", p.tries.is_some()), ("from", p.from.is_some()), ("to", p.to.is_some()), ("image", p.image.is_some())])?;
            ("screenshot", json!({"max_edge": edge}))
        }
        other => return Err(format!("unknown mode {other}; preview, analyze, compare, trials or screenshot")),
    };
    operate(hub, driver, cx, call.0, unset(call.1)).await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Edit {
    photo: Option<Value>,
    values: Option<Map<String, Value>>,
    film: Option<Value>,
    curve: Option<Value>,
    fit: Option<Map<String, Value>>,
    paste: Option<Map<String, Value>>,
    region: Option<Value>,
    watch: Option<Vec<String>>,
    if_state: Option<String>,
    preview: Option<Value>,
    label: Option<String>,
    action: Option<String>,
    args: Option<Value>,
}

/// The internal operation an edit runs and its arguments; every one checks `if_state` against the photo it holds,
/// at the moment it commits, and reads `watch` from the state it leaves.
fn route(mut p: Edit) -> Result<(&'static str, Value), String> {
    let allowed: &[&str] = match p.action.as_deref() {
        Some("command") => &["name", "params"],
        Some("go_to") => &["step"],
        Some("snapshot.save" | "snapshot.restore" | "snapshot.delete") => &["name"],
        _ => &[],
    };
    let extra = only_args(p.args.take(), allowed, &format!("edit {}", p.action.as_deref().unwrap_or("without an action")))?;
    match &p.photo {
        None | Some(Value::String(_)) => {}
        Some(Value::Array(ids)) if !ids.is_empty() && ids.iter().all(Value::is_string) => {}
        Some(_) => return Err("photo is an id or a list of ids".into()),
    }
    match &p.preview {
        None | Some(Value::Bool(_)) => {}
        Some(Value::Number(n)) if n.as_u64().is_some_and(|e| (64..=2048).contains(&e)) => {}
        Some(_) => return Err("preview is true or a long edge of 64-2048 px".into()),
    }
    let modes = [p.values.is_some() || p.film.is_some() || p.curve.is_some(), p.fit.is_some(), p.paste.is_some(), p.action.is_some()];
    if modes.iter().filter(|m| **m).count() != 1 {
        return Err("an edit is one of: values, curve and/or film, fit, paste or action".into());
    }
    if p.region.is_some() && p.fit.is_none() {
        return Err("region is where fit measures; other edits apply to the whole photo".into());
    }
    if p.if_state.is_some() && p.photo.as_ref().is_some_and(Value::is_array) {
        return Err("if_state guards one photo's state; give photo one id".into());
    }
    if p.paste.is_some() && (p.preview.is_some() || p.label.is_some()) {
        return Err("paste names its own step and answers for each photo; look at the targets with look preview".into());
    }
    if p.fit.is_some() && p.label.is_some() {
        return Err("fit names its own step from its goal".into());
    }
    let (photo, if_state, watch, preview) = (p.photo, p.if_state, p.watch, p.preview);
    let frame = |own: Value| laid(json!({"photo": photo, "if_state": if_state, "watch": watch, "preview": preview, "label": p.label}), unset(own), "the request");
    if let Some(paste) = p.paste {
        let to = photo.clone().ok_or("paste needs photo: the photos to paste onto")?;
        return Ok(("paste", laid(json!({"to": to, "photo": null, "if_state": if_state, "watch": watch, "preview": null, "region": null}), Value::Object(paste), "paste")?));
    }
    if let Some(goal) = p.fit {
        return Ok(("fit", laid(json!({"photo": photo, "region": p.region, "preview": preview, "if_state": if_state, "watch": watch}), Value::Object(goal), "fit")?));
    }
    if let Some(action) = p.action {
        let named = |op: &'static str, action: &str| Ok::<_, String>((op, frame(json!({"action": action, "step": extra["step"], "name": extra["name"]}))?));
        return match action.as_str() {
            "command" => Ok(("command", frame(json!({"name": extra["name"], "params": extra["params"]}))?)),
            "undo" | "redo" | "go_to" => named("history", &action),
            "snapshot.save" | "snapshot.restore" | "snapshot.delete" => named("snapshots", &action["snapshot.".len()..]),
            other => Err(format!("unknown action {other}; command, undo, redo, go_to, snapshot.save, snapshot.restore or snapshot.delete")),
        };
    }
    match (p.values, p.film, p.curve) {
        (Some(values), None, None) => Ok(("set_controls", frame(json!({"values": values}))?)),
        (None, Some(film), None) => Ok(("set_film", laid(json!({"photo": photo, "preview": preview, "if_state": if_state, "watch": watch, "label": p.label}), film, "film")?)),
        (values, film, curve) => Ok(("change", frame(json!({"values": values, "film": film, "curve": curve}))?)),
    }
}

pub async fn edit(hub: &Rc<Hub>, driver: &Driver, cx: &mut AsyncApp, value: Value) -> Answer {
    let p: Edit = args(value)?;
    let (op, call) = route(p)?;
    operate(hub, driver, cx, op, call).await
}
