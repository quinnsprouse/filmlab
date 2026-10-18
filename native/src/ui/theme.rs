use std::sync::Arc;

use gpui_kit::{FontFeatures, Hsla, Rgba, rgba};

fn c(hex: u32) -> Hsla {
    Hsla::from(Rgba::from(rgba(hex)))
}

fn white(alpha: f32) -> Hsla {
    Hsla { h: 0.0, s: 0.0, l: 1.0, a: alpha }
}

pub const FONT: &str = "Open Runde";
pub const BACKGROUND: [f64; 3] = [0.055, 0.055, 0.06];
pub const ROW: f32 = 36.0;
pub const RADIUS: f32 = 8.0;
pub const GAP: f32 = 6.0;

pub fn tabular() -> FontFeatures {
    FontFeatures(Arc::new(vec![("tnum".into(), 1)]))
}

pub fn primary() -> Hsla {
    white(0.95)
}

pub fn secondary() -> Hsla {
    white(0.7)
}

pub fn tertiary() -> Hsla {
    white(0.45)
}

pub fn label() -> Hsla {
    white(0.7)
}

pub fn focus() -> Hsla {
    white(1.0)
}

pub fn surface() -> Hsla {
    white(0.05)
}

pub fn surface_hover() -> Hsla {
    white(0.1)
}

pub fn fill() -> Hsla {
    white(0.11)
}

pub fn fill_active() -> Hsla {
    white(0.15)
}

pub fn hashmark() -> Hsla {
    white(0.15)
}

pub fn handle() -> Hsla {
    white(0.95)
}

pub fn divider() -> Hsla {
    white(0.06)
}

pub fn track() -> Hsla {
    white(0.11)
}

pub fn thumb() -> Hsla {
    white(0.95)
}

pub fn hover() -> Hsla {
    white(0.08)
}

pub fn selected() -> Hsla {
    white(0.14)
}

pub fn separator() -> Hsla {
    white(0.06)
}

pub fn accent() -> Hsla {
    c(0xffb340ff)
}

pub fn danger() -> Hsla {
    c(0xff6b5eff)
}

pub fn backdrop() -> Hsla {
    c(0x0e0e0fff)
}
