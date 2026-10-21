use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use filmlab_engine::core;
use serde_json::{Value, json};

use super::{POINT_CURVES, Step};
use crate::film::FilmSettings;

/// A photo's edit as flat ids and values: every control (`light.exposure`) and the film look (`film.grain.size`).
pub type Flat = HashMap<String, Value>;

/// Controls an agent set that nobody has touched since, to the value it set.
pub type Marks = HashMap<String, Value>;

/// The ids whose values differ between two flattened edits. For an agent's change, a film look switched on from
/// none counts only its stocks: the halation, grain and dust defaults that come with it weren't the agent's choice.
pub fn changed(before: &Flat, after: &Flat, by_agent: bool) -> Vec<String> {
    let had_film = before.keys().any(|k| k.starts_with("film."));
    after
        .iter()
        .filter(|(id, v)| before.get(*id) != Some(*v) && !(v.is_null() && !before.contains_key(*id)))
        .filter(|(id, _)| !by_agent || had_film || !id.starts_with("film.") || matches!(id.as_str(), "film.negative" | "film.print" | "film.utility"))
        .map(|(id, _)| id.clone())
        .collect()
}

pub fn flatten(settings: &Value, film: Option<&FilmSettings>) -> Flat {
    fn walk(prefix: &str, v: &Value, out: &mut Flat) {
        match v {
            Value::Object(map) => map.iter().for_each(|(k, v)| walk(&format!("{prefix}.{k}"), v, out)),
            other => {
                out.insert(prefix.to_string(), other.clone());
            }
        }
    }
    let values: HashMap<String, f64> = core::control_values(&settings.to_string()).ok().and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_default();
    let mut out: Flat = values.into_iter().map(|(k, v)| (k, json!(v))).collect();
    for channel in POINT_CURVES {
        out.insert(format!("curve.{channel}"), settings["curve"][channel].clone());
    }
    if let Some(film) = film {
        walk("film", &film.normalized().to_value(), &mut out);
    }
    out
}

/// Marks `ids` of `now` as the agent's, or clears them for anyone else's change.
pub fn apply(marks: &mut Marks, ids: Vec<String>, now: &Flat, by_agent: bool) {
    for id in ids {
        if by_agent {
            marks.insert(id.clone(), now[&id].clone());
        } else {
            marks.remove(&id);
        }
    }
}

struct Replayed {
    key: String,
    at: Instant,
    flat: Arc<Flat>,
    marks: Marks,
}

// why: undo, redo and a photo opened mid-session move through history at once, so the marks are replayed from the
// first step up to the current one; an agent change you undid, or one you edited after, isn't marked
/// The marks each history step ends with when history is replayed from its first step, kept for the steps replayed
/// so far. A step is known by its key and time, so a history changed or replaced since is replayed again from where
/// it differs.
#[derive(Default)]
pub struct Replay(Vec<Replayed>);

impl Replay {
    /// The flattened edit and marks at `index`; `flat` is that step's flattened edit when the caller has it.
    pub fn at(&mut self, history: &[Step], index: usize, flat: Option<Arc<Flat>>) -> Option<(Arc<Flat>, Marks)> {
        let valid = self.0.iter().zip(history).take_while(|(r, s)| r.at == s.at && r.key == s.key).count();
        self.0.truncate(valid);
        let mut flat = flat;
        while self.0.len() <= index {
            let i = self.0.len();
            let step = history.get(i)?;
            let now = if i == index { flat.take() } else { None }.unwrap_or_else(|| Arc::new(flatten(&step.settings, step.film.as_ref())));
            let mut marks = Marks::new();
            if let Some(previous) = self.0.last() {
                let by_agent = step.key.starts_with("agent.");
                marks.clone_from(&previous.marks);
                apply(&mut marks, changed(&previous.flat, &now, by_agent), &now, by_agent);
            }
            self.0.push(Replayed { key: step.key.clone(), at: step.at, flat: now, marks });
        }
        self.0.get(index).map(|r| (r.flat.clone(), r.marks.clone()))
    }
}
