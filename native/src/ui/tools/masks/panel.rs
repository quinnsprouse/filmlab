use gpui_kit::*;
use serde_json::{Value, json};

use super::adjust::{mask_adjust, radial_controls};
use super::brush_panel::brush_controls;
use super::components::component_list;
use super::create::create_grid;
use super::list::mask_list;
use super::model::read_masks;
use super::state::Interaction;
use super::widgets::{folder, on_flag};
use crate::ui::app::FilmlabApp;
use crate::ui::controls::Toggle;
use crate::ui::theme;

const OVERLAY_COLOR: [u8; 3] = [255, 40, 40];
const OVERLAY_OPACITY: u8 = 50;

/// The overlay the engine paints on the photo: the hovered mask, else the selected one while the
/// overlay is shown, being edited, or the luminance map is on.
fn overlay_request(app: &FilmlabApp, active: Option<u32>) -> Option<Value> {
    let state = &app.tools.masks;
    if state.interaction == Interaction::Pick {
        return None;
    }
    let shown = state.show_overlay || state.engaged || state.luma_map;
    let target = state.hovered.or(if shown { active } else { None })?;
    let mut request = json!({"kind": "mask", "id": target, "color": OVERLAY_COLOR, "opacity": OVERLAY_OPACITY});
    if state.luma_map {
        request["view"] = json!("colorOnBw");
    }
    Some(request)
}

fn sync_overlay(app: &FilmlabApp, active: Option<u32>, cx: &mut Context<FilmlabApp>) {
    let wanted = overlay_request(app, active);
    if app.develop.read(cx).overlay != wanted {
        app.develop.update(cx, |d, dcx| d.set_overlay(wanted, dcx));
    }
}

pub fn panel(app: &mut FilmlabApp, window: &mut Window, cx: &mut Context<FilmlabApp>) -> AnyElement {
    let (photo, settings) = {
        let d = app.develop.read(cx);
        (d.photo_id.clone(), d.settings.clone())
    };
    let Some(settings) = settings else {
        return div().p_4().text_size(px(12.)).text_color(theme::tertiary()).child("Open a photo to develop it.").into_any_element();
    };
    let masks = read_masks(Some(&settings));
    let active = app.tools.masks.active_for(photo.as_ref()).filter(|id| masks.iter().any(|m| m.id == *id));
    sync_overlay(app, active, cx);
    let app: &FilmlabApp = app;
    let mask = masks.iter().find(|m| Some(m.id) == active);
    let painting = matches!(app.tools.masks.interaction, Interaction::Paint(_));
    let mut sections = vec![
        folder("masks-create", "Create", true, false, cx, app, |cx| vec![create_grid(cx)]),
        folder("masks-list", "Masks", true, false, cx, app, |cx| {
            let mut rows = mask_list(app, &masks, active, cx);
            rows.push(
                Toggle::new("masks-show-overlay", "Show overlay (O)", app.tools.masks.show_overlay, on_flag(cx, |app, on, cx| {
                    app.tools.masks.show_overlay = on;
                    cx.notify();
                }))
                .into_any_element(),
            );
            rows
        }),
    ];
    if let Some(mask) = mask {
        sections.push(folder("masks-selection", "Selection", true, false, cx, app, |cx| {
            let mut rows = component_list(app, mask, window, cx);
            rows.extend(radial_controls(mask, cx));
            rows
        }));
    }
    if painting {
        sections.push(folder("masks-brush", "Brush", true, false, cx, app, |cx| brush_controls(app, cx)));
    }
    if let Some(mask) = mask {
        sections.push(folder("masks-adjust", "Adjustments", true, false, cx, app, |cx| mask_adjust(app, mask, cx)));
    }
    let count = sections.len();
    let mut body: Vec<AnyElement> = Vec::new();
    for (i, section) in sections.into_iter().enumerate() {
        body.push(section);
        if i + 1 < count {
            body.push(div().h(px(1.)).bg(theme::divider()).into_any_element());
        }
    }
    let error = app.tools.masks.error.clone();
    div()
        .id("masks-scroll")
        .size_full()
        .overflow_y_scroll()
        .px(px(12.))
        .pt(px(12.))
        .pb(px(8.))
        .flex()
        .flex_col()
        .child(div().h(px(28.)).flex().items_center().text_size(px(15.)).font_weight(FontWeight::SEMIBOLD).text_color(theme::focus()).child("Masks"))
        .children(error.map(|message| div().py(px(6.)).text_size(px(12.)).text_color(theme::danger()).child(message)))
        .children(body)
        .into_any_element()
}
