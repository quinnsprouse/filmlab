use std::time::Duration;

use gpui_kit::*;

use super::{motion, theme};
use crate::mcp::{FADE, Presence};

const ENTER: Duration = Duration::from_millis(220);
const BREATH: Duration = Duration::from_millis(1800);

fn activity(tool: &str) -> &'static str {
    match tool {
        "set_controls" => "Adjusting",
        "set_film" => "Film look",
        "command" => "Editing",
        "preview" | "analyze" | "screenshot" => "Looking",
        "history" => "History",
        "snapshots" => "Snapshot",
        "profiles" => "Profiles",
        "export" => "Exporting",
        "open_photo" => "Opening",
        "open_folder" | "list_photos" | "set_flags" => "Library",
        "view" => "Viewing",
        _ => "Reading",
    }
}

/// Fades `element` in when it appears, or out while the agent leaves; still under Reduce Motion.
fn presence_fade<E: IntoElement + Styled + 'static>(element: E, id: &'static str, presence: &Presence, calm: bool) -> AnyElement {
    if calm {
        return element.into_any_element();
    }
    if *presence == Presence::Leaving {
        return element
            .with_animation(SharedString::from(format!("{id}-out")), Animation::new(FADE).with_easing(motion::emphasized()), |e, t| e.opacity(1.0 - t))
            .into_any_element();
    }
    element.with_animation(SharedString::from(format!("{id}-in")), Animation::new(ENTER).with_easing(motion::emphasized()), |e, t| e.opacity(t)).into_any_element()
}

/// The top bar's sign that an agent is driving the editor: a breathing dot and what it's doing, a steady dot for a
/// moment after its last call, then a fade.
pub fn pill(presence: &Presence) -> AnyElement {
    let calm = motion::reduced();
    let working = match presence {
        Presence::Working(tool) => Some(activity(tool)),
        _ => None,
    };
    let dot = div().size(px(6.)).rounded_full().bg(theme::accent());
    let dot = match (working.is_some(), calm) {
        (true, false) => dot.with_animation("agent-dot", Animation::new(BREATH).repeat().with_easing(pulsating_between(0.35, 1.0)), |d, o| d.opacity(o)).into_any_element(),
        _ => dot.into_any_element(),
    };
    let doing = working.map(|text| {
        let label = div().text_color(theme::secondary()).child(text);
        if calm {
            label.into_any_element()
        } else {
            label
                .with_animation(SharedString::from(format!("agent-doing-{text}")), Animation::new(Duration::from_millis(160)).with_easing(motion::emphasized()), |e, t| e.opacity(t))
                .into_any_element()
        }
    });
    let pill = div()
        .flex_none()
        .flex()
        .items_center()
        .gap(px(6.))
        .h(px(22.))
        .px(px(9.))
        .rounded_full()
        .bg(theme::track())
        .text_size(px(11.))
        .font_weight(FontWeight::MEDIUM)
        .child(dot)
        .child(div().text_color(theme::label()).child("Agent"))
        .children(doing);
    presence_fade(pill, "agent-pill", presence, calm)
}

/// A faint accent edge around the photo while an agent works on it, fading like the pill.
pub fn outline(presence: &Presence, bounds: Bounds<Pixels>) -> AnyElement {
    let calm = motion::reduced();
    let edge = div()
        .absolute()
        .left(bounds.origin.x)
        .top(bounds.origin.y)
        .w(bounds.size.width)
        .h(bounds.size.height)
        .border_1()
        .border_color(theme::accent().opacity(0.45));
    presence_fade(edge, "agent-outline", presence, calm)
}
