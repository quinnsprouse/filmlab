use gpui_kit::{IntoElement, Pixels, Styled, px, svg};

use super::theme;

pub fn icon(name: &'static str, size: Pixels) -> impl IntoElement {
    svg().path(format!("icons/{name}.svg")).size(size).text_color(theme::primary())
}

pub fn small(name: &'static str) -> impl IntoElement {
    icon(name, px(16.))
}
