use gpui_kit::*;

use super::commands::add_mask;
use super::model::CreateKind;
use super::widgets::{glyph, pressable};
use crate::ui::app::FilmlabApp;
use crate::ui::theme;

pub fn create_grid(cx: &mut Context<FilmlabApp>) -> AnyElement {
    let cells = CreateKind::ALL.map(|kind| {
        pressable(SharedString::from(format!("masks-create-{}", kind.key())))
            .h(px(52.))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(4.))
            .rounded(px(theme::RADIUS))
            .bg(theme::surface())
            .text_color(theme::label())
            .hover(|s| s.bg(theme::surface_hover()).text_color(theme::primary()))
            .on_click(cx.listener(move |app, _: &ClickEvent, _, cx| add_mask(app, kind, cx)))
            .child(glyph(kind.icon(), 18., theme::primary()).opacity(0.85))
            .child(div().text_size(px(11.)).font_weight(FontWeight::MEDIUM).child(kind.short()))
    });
    div().grid().grid_cols(4).gap(px(theme::GAP)).children(cells).into_any_element()
}
