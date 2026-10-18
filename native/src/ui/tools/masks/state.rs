use std::collections::HashMap;

use gpui_kit::{Bounds, FocusHandle, Pixels, Subscription};

use super::model::{Mask, Pt};
use super::scene::{Grip, Scene};
use super::view::ViewSpace;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum Interaction {
    #[default]
    Adjust,
    Paint(Option<u32>),
    Pick,
}

#[derive(Clone, Copy)]
pub struct BrushSettings {
    pub size: f64,
    pub feather: f64,
    pub flow: f64,
    pub erase: bool,
    pub auto_mask: bool,
}

impl Default for BrushSettings {
    fn default() -> Self {
        BrushSettings { size: 0.03, feather: 50.0, flow: 100.0, erase: false, auto_mask: false }
    }
}

pub enum Drag {
    Handle { grip: Grip, start: Pt },
    Paint { points: Vec<Pt> },
}

pub struct Rename {
    pub id: u32,
    pub draft: String,
    pub fresh: bool,
    pub focus: FocusHandle,
    pub _blur: Subscription,
}

/// What the overlay last drew and what the pointer is doing, kept between frames so the window-level
/// mouse handlers can hit-test against the same scene the user sees.
#[derive(Default)]
pub struct Pointer {
    pub drag: Option<Drag>,
    pub cursor: Option<Pt>,
    pub alt: bool,
    pub over_grip: bool,
    pub scene: Scene,
    pub space: Option<ViewSpace>,
    pub masks: Vec<Mask>,
    pub active: Option<u32>,
    pub frame: Bounds<Pixels>,
}

#[derive(Default)]
pub struct State {
    pub photo_id: Option<String>,
    pub active: Option<u32>,
    pub show_overlay: bool,
    pub hovered: Option<u32>,
    pub engaged: bool,
    pub luma_map: bool,
    pub interaction: Interaction,
    pub brush: BrushSettings,
    pub error: Option<String>,
    pub open: HashMap<&'static str, bool>,
    pub rename: Option<Rename>,
    pub pointer: Pointer,
    pub focus: Option<FocusHandle>,
}

impl State {
    /// The selected mask, but only when it belongs to the photo that is open.
    pub fn active_for(&self, photo_id: Option<&String>) -> Option<u32> {
        if self.photo_id.as_ref() == photo_id { self.active } else { None }
    }

    pub fn is_open(&self, id: &'static str, default: bool) -> bool {
        self.open.get(id).copied().unwrap_or(default)
    }
}
