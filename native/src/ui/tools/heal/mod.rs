mod draw;
mod geometry;
mod overlay;
mod panel;
mod spots;

use gpui_kit::*;

use self::geometry::Grip;
use self::overlay::Gesture;
use self::spots::{Mode, Spot, V};
use crate::ui::app::FilmlabApp;

pub use self::overlay::overlay;
pub use self::panel::panel;

struct Brush {
    mode: Mode,
    size: f64,
    feather: f64,
    opacity: f64,
}

pub struct State {
    brush: Brush,
    message: Option<String>,
    selection: Option<(String, usize)>,
    hover: Option<V>,
    gesture: Option<Gesture>,
    paint: Vec<V>,
    grips: Vec<Grip>,
    focus: Option<FocusHandle>,
    adds: u32,
}

impl Default for State {
    fn default() -> Self {
        State {
            brush: Brush { mode: Mode::Heal, size: 0.02, feather: 50.0, opacity: 100.0 },
            message: None,
            selection: None,
            hover: None,
            gesture: None,
            paint: Vec::new(),
            grips: Vec::new(),
            focus: None,
            adds: 0,
        }
    }
}

impl State {
    fn selected(&self, photo: Option<&str>, count: usize) -> Option<usize> {
        let (id, index) = self.selection.as_ref()?;
        (Some(id.as_str()) == photo && *index < count).then_some(*index)
    }

    fn select(&mut self, photo: Option<&str>, index: Option<usize>) {
        self.selection = photo.zip(index).map(|(p, i)| (p.to_string(), i));
    }

    fn pick(&mut self, photo: Option<&str>, index: usize, spot: Option<&Spot>) {
        self.select(photo, Some(index));
        if let Some(s) = spot {
            self.brush = Brush { mode: s.mode, size: s.size, feather: s.feather, opacity: s.opacity };
        }
    }
}

fn current(app: &FilmlabApp, cx: &App) -> (Option<String>, Vec<Spot>) {
    let d = app.develop.read(cx);
    (d.photo_id.clone(), spots::spots_of(d.settings.as_ref()))
}
