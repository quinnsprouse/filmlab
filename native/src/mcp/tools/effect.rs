use std::sync::Arc;

use gpui_kit::{AppContext, AsyncApp};
use serde_json::{Map, Value, json};

use super::look::{self, MEASURED, Shot, State};
use super::{compare, curve, fit, scopes};
use crate::mcp::session::{Held, Hub};
use crate::ui::driver::Driver;

/// The named measures' readings of `shot`, null where it can't be read (a cast with too little near neutral).
pub fn readings(shot: &Shot, names: &[String]) -> Value {
    let m = look::measure(shot);
    let read = |name: &str| if scopes::MEASURES.contains(&name) { scopes::reading(shot, name) } else { fit::standard(&m, name) };
    Value::Object(names.iter().map(|n| (n.clone(), read(n).map_or(Value::Null, |v| json!(look::round(v, 3))))).collect())
}

async fn measured(hub: &Hub, held: &Held, driver: &Driver, cx: &mut AsyncApp, before: &State, watch: &[String]) -> Result<Map<String, Value>, String> {
    let now = State::now(held, driver, cx)?;
    let b = look::frame(hub, held, driver, cx, &now, MEASURED).await?;
    let a: Option<Arc<Shot>> = match now.same(before) {
        true => None,
        false => Some(look::frame(hub, held, driver, cx, before, MEASURED).await?),
    };
    let moved = compare::moved(&before.settings, &now.settings);
    let traced = match curve::moved(&before.settings, &now.settings) {
        true => Some((before.settings["curve"].clone(), now.settings["curve"].clone(), look::input_frame(hub, held, driver, cx, &now, MEASURED).await?)),
        false => None,
    };
    let watch = watch.to_vec();
    Ok(cx
        .background_spawn(async move {
            let mut out = match &a {
                Some(a) => compare::effect(a, &b, moved),
                None => json!({"effect": "same settings"}),
            };
            if let Some((was, now, input)) = &traced
                && let Ok(Some(block)) = curve::block(was, now, &curve::Intake::of(input))
            {
                out["curve"] = block;
            }
            if !watch.is_empty() {
                out["watch"] = readings(&b, &watch);
            }
            match out {
                Value::Object(o) => o,
                _ => Map::new(),
            }
        })
        .await)
}

/// Adds what the change from `before` did to the held photo to an edit's answer: the change in words, the numbers that
/// moved, and the current readings of the measures in `watch`, all from the state this call left while it still holds
/// the photo. The change is made either way, so a measurement that fails marks the answer applied but unverified
/// rather than failing the call, which would invite repeating the edit.
pub async fn report(hub: &Hub, held: &Held, driver: &Driver, cx: &mut AsyncApp, before: &State, mut out: Value, watch: &[String]) -> Value {
    match measured(hub, held, driver, cx, before, watch).await {
        Ok(found) => {
            if let Value::Object(o) = &mut out {
                o.extend(found);
            }
        }
        Err(e) => {
            out["status"] = json!("applied_unverified");
            out["code"] = json!(e);
        }
    }
    out
}

/// The measures a call asked to watch, checked before it changes anything.
pub fn watching(watch: Option<Vec<String>>) -> Result<Vec<String>, String> {
    let watch = watch.unwrap_or_default();
    match watch.iter().find(|w| !look::MEASURES.contains(&w.as_str()) && !scopes::MEASURES.contains(&w.as_str())) {
        Some(bad) => Err(format!("unknown watch {bad}; guide metrics lists them")),
        None => Ok(watch),
    }
}
