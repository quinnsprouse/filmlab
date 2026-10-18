use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::commands::{choose_mask, delete_mask, duplicate_mask, invert_mask, rename_mask, toggle_visible};
use super::model::Mask;
use super::state::Rename;
use super::widgets::{body_text, glyph, icon_button, on_press, pressable};
use crate::ui::app::FilmlabApp;
use crate::ui::theme;

fn begin_rename(app: &mut FilmlabApp, mask: &Mask, window: &mut Window, cx: &mut Context<FilmlabApp>) {
    let focus = cx.focus_handle();
    let blur = cx.on_blur(&focus, window, |app, _, cx| finish_rename(app, true, cx));
    window.focus(&focus, cx);
    app.tools.masks.rename = Some(Rename { id: mask.id, draft: mask.name.clone(), fresh: true, focus, _blur: blur });
    cx.notify();
}

fn finish_rename(app: &mut FilmlabApp, commit: bool, cx: &mut Context<FilmlabApp>) {
    if let Some(rename) = app.tools.masks.rename.take()
        && commit
    {
        rename_mask(app, rename.id, &rename.draft, cx);
    }
    cx.notify();
}

fn rename_key(app: &mut FilmlabApp, event: &KeyDownEvent, cx: &mut Context<FilmlabApp>) {
    let keystroke = &event.keystroke;
    cx.stop_propagation();
    match keystroke.key.as_str() {
        "enter" => return finish_rename(app, true, cx),
        "escape" => return finish_rename(app, false, cx),
        _ => {}
    }
    let Some(rename) = app.tools.masks.rename.as_mut() else { return };
    if keystroke.key == "backspace" {
        if rename.fresh {
            rename.draft.clear();
            rename.fresh = false;
        } else {
            rename.draft.pop();
        }
    } else if !keystroke.modifiers.platform
        && !keystroke.modifiers.control
        && let Some(typed) = &keystroke.key_char
    {
        if rename.fresh {
            rename.draft.clear();
            rename.fresh = false;
        }
        rename.draft.push_str(typed);
    }
    cx.notify();
}

fn name_field(rename: &Rename, cx: &mut Context<FilmlabApp>) -> AnyElement {
    div()
        .id("masks-rename")
        .key_context("MaskRename")
        .track_focus(&rename.focus)
        .on_key_down(cx.listener(|app, event: &KeyDownEvent, _, cx| rename_key(app, event, cx)))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .flex_1()
        .min_w_0()
        .h(px(24.))
        .px(px(6.))
        .flex()
        .items_center()
        .rounded(px(6.))
        .bg(theme::fill_active())
        .text_size(px(13.))
        .font_weight(FontWeight::MEDIUM)
        .child(div().min_w_0().truncate().when(rename.fresh, |d| d.bg(theme::accent().opacity(0.35))).child(rename.draft.clone()))
        .child(div().ml(px(1.)).w(px(1.)).h(px(14.)).bg(theme::primary()))
        .into_any_element()
}

fn row(app: &FilmlabApp, mask: &Mask, selected: bool, cx: &mut Context<FilmlabApp>) -> AnyElement {
    let id = mask.id;
    let renaming = app.tools.masks.rename.as_ref().filter(|r| r.id == id);
    let picker = |renaming: Option<&Rename>, cx: &mut Context<FilmlabApp>| -> AnyElement {
        if let Some(rename) = renaming {
            return name_field(rename, cx);
        }
        let named = mask.clone();
        pressable(SharedString::from(format!("masks-pick-{id}")))
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .items_center()
            .gap(px(8.))
            .text_size(px(13.))
            .font_weight(FontWeight::MEDIUM)
            .text_color(if selected { theme::primary() } else { theme::label() })
            .when(!mask.visible, |d| d.opacity(0.5))
            .on_click(cx.listener(move |app, event: &ClickEvent, window, cx| {
                if event.click_count() >= 2 {
                    begin_rename(app, &named, window, cx);
                } else {
                    choose_mask(app, Some(id), cx);
                }
            }))
            .child(glyph(mask.lead_kind().icon(), 14., theme::primary()))
            .child(div().min_w_0().truncate().child(mask.name.clone()))
            .when(mask.components.len() > 1, |d| d.child(div().flex_none().text_size(px(11.)).font_features(theme::tabular()).text_color(theme::tertiary()).child(mask.components.len().to_string())))
            .into_any_element()
    };
    let named = mask.clone();
    let actions = (selected && renaming.is_none()).then(|| {
        div()
            .flex()
            .items_center()
            .gap(px(2.))
            .child(icon_button(SharedString::from(format!("masks-rename-{id}")), "pencil-edit02", false, on_press(cx, move |app, window, cx| begin_rename(app, &named, window, cx))))
            .child(icon_button(SharedString::from(format!("masks-invert-{id}")), "contrast", mask.invert, on_press(cx, move |app, _, cx| invert_mask(app, id, cx))))
            .child(icon_button(SharedString::from(format!("masks-copy-{id}")), "copy01", false, on_press(cx, move |app, _, cx| duplicate_mask(app, id, cx))))
            .child(icon_button(SharedString::from(format!("masks-delete-{id}")), "delete02", false, on_press(cx, move |app, _, cx| delete_mask(app, id, cx))))
    });
    div()
        .id(SharedString::from(format!("masks-row-{id}")))
        .h(px(theme::ROW))
        .w_full()
        .pl(px(10.))
        .pr(px(6.))
        .flex()
        .items_center()
        .gap(px(2.))
        .rounded(px(theme::RADIUS))
        .bg(if selected { theme::fill() } else { theme::surface() })
        .when(!selected, |d| d.hover(|s| s.bg(theme::surface_hover())))
        .on_hover(cx.listener(move |app, hovered: &bool, _, cx| {
            let state = &mut app.tools.masks;
            if *hovered {
                state.hovered = Some(id);
            } else if state.hovered == Some(id) {
                state.hovered = None;
            }
            cx.notify();
        }))
        .child(picker(renaming, cx))
        .children(actions)
        .child(icon_button(SharedString::from(format!("masks-visible-{id}")), if mask.visible { "view" } else { "view-off" }, false, on_press(cx, move |app, _, cx| toggle_visible(app, id, cx))))
        .into_any_element()
}

pub fn mask_list(app: &FilmlabApp, masks: &[Mask], active: Option<u32>, cx: &mut Context<FilmlabApp>) -> Vec<AnyElement> {
    if masks.is_empty() {
        return vec![body_text("No masks yet. Choose a mask type above, then adjust it here.").into_any_element()];
    }
    masks.iter().map(|mask| row(app, mask, Some(mask.id) == active, cx)).collect()
}
