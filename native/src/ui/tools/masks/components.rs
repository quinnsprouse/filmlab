use gpui_kit::*;

use super::commands::{ComponentAction, add_component, edit_component};
use super::model::{CreateKind, Mask, MaskOp};
use super::range::range_controls;
use super::widgets::{MenuButton, icon_button, on_index, on_press};
use crate::ui::app::FilmlabApp;
use crate::ui::theme;

fn op_cell(mask: &Mask, index: usize, cx: &mut Context<FilmlabApp>) -> AnyElement {
    let id = mask.id;
    let op = mask.components[index].op;
    if index == 0 {
        return div().size(px(24.)).flex_none().flex().items_center().justify_center().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).text_color(theme::tertiary()).child(MaskOp::Add.sign()).into_any_element();
    }
    let entries = MaskOp::ALL.iter().map(|op| op.label().to_string()).collect();
    MenuButton::new(SharedString::from(format!("masks-op-{id}-{index}")), op.sign(), entries, on_index(cx, move |app, pick, cx| edit_component(app, id, index, ComponentAction::Op(MaskOp::ALL[pick]), cx))).into_any_element()
}

fn component_row(mask: &Mask, index: usize, window: &mut Window, app: &FilmlabApp, cx: &mut Context<FilmlabApp>) -> AnyElement {
    let id = mask.id;
    let component = &mask.components[index];
    let name = mask.component_name(index);
    let head = div()
        .h(px(32.))
        .w_full()
        .pl(px(4.))
        .pr(px(4.))
        .flex()
        .items_center()
        .gap(px(6.))
        .rounded(px(theme::RADIUS))
        .bg(theme::surface())
        .child(op_cell(mask, index, cx))
        .child(div().flex_1().min_w_0().truncate().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child(name))
        .child(icon_button(SharedString::from(format!("masks-cinvert-{id}-{index}")), "contrast", component.invert, on_press(cx, move |app, _, cx| edit_component(app, id, index, ComponentAction::Invert, cx))))
        .child(icon_button(SharedString::from(format!("masks-cdelete-{id}-{index}")), "delete02", false, on_press(cx, move |app, _, cx| edit_component(app, id, index, ComponentAction::Delete, cx))));
    let ranges = range_controls(app, id, index, &component.shape, window, cx);
    div().flex().flex_col().gap(px(theme::GAP)).child(head).children(ranges).into_any_element()
}

pub fn component_list(app: &FilmlabApp, mask: &Mask, window: &mut Window, cx: &mut Context<FilmlabApp>) -> Vec<AnyElement> {
    let id = mask.id;
    let mut rows: Vec<AnyElement> = (0..mask.components.len()).map(|index| component_row(mask, index, window, app, cx)).collect();
    let kinds: Vec<String> = CreateKind::ALL.iter().map(|kind| kind.label().to_string()).collect();
    let combine = MaskOp::ALL.map(|op| {
        MenuButton::new(SharedString::from(format!("masks-combine-{id}-{}", op.key())), op.label(), kinds.clone(), on_index(cx, move |app, pick, cx| add_component(app, id, op, CreateKind::ALL[pick], cx))).wide()
    });
    rows.push(div().flex().gap(px(theme::GAP)).children(combine).into_any_element());
    rows
}
