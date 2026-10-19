use std::ops::Range;
use std::time::Duration;

use gpui_kit::*;

use super::{motion, theme};
use crate::mcp::{FADE, Presence};

const ENTER: Duration = Duration::from_millis(240);
/// One pass of light across the words, then a rest before the next, so it breathes rather than spins.
const TRAVEL: f32 = 1.5;
const REST: f32 = 0.9;
/// How far before and after the words the light starts and ends, in characters, so it slides in and out of them.
const RUN_OUT: f32 = 5.0;
/// The light's falloff in characters: a crisp leading edge and a longer warm tail, like a print coming up.
const LEAD: f32 = 1.1;
const TRAIL: f32 = 3.4;

/// Fades `element` in (settling up two points) when it appears, or out while the agent leaves; still under Reduce
/// Motion.
fn presence_fade<E: IntoElement + Styled + 'static>(element: E, id: &'static str, presence: Presence, calm: bool, settle: bool) -> AnyElement {
    if calm {
        return element.into_any_element();
    }
    if presence == Presence::Leaving {
        return element
            .with_animation(SharedString::from(format!("{id}-out")), Animation::new(FADE).with_easing(motion::emphasized()), |e, t| e.opacity(1.0 - t))
            .into_any_element();
    }
    element
        .with_animation(SharedString::from(format!("{id}-in")), Animation::new(ENTER).with_easing(motion::emphasized()), move |e, t| if settle { e.opacity(t).top(px(2.0 * (1.0 - t))) } else { e.opacity(t) })
        .into_any_element()
}

fn mix(a: Rgba, b: Rgba, t: f32) -> Rgba {
    let t = t.clamp(0.0, 1.0);
    Rgba { r: a.r + (b.r - a.r) * t, g: a.g + (b.g - a.g) * t, b: a.b + (b.b - a.b) * t, a: a.a + (b.a - a.a) * t }
}

fn smoothstep(edge: f32, x: f32) -> f32 {
    let t = (x / edge).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Where the light is at `t` (0..1 of a cycle), in characters from the start of the words; `None` while it rests.
fn light_at(t: f32, len: f32) -> Option<f32> {
    let elapsed = t * (TRAVEL + REST);
    if elapsed >= TRAVEL {
        return None;
    }
    let p = motion::bezier(0.645, 0.045, 0.355, 1.0)(elapsed / TRAVEL);
    Some(-RUN_OUT + p * (len + 2.0 * RUN_OUT))
}

/// A colour per character: the agent's name a little brighter than the rest, and the passing light warming each one
/// from white through amber to a hot near-white core.
fn runs(text: &str, name_len: usize, light: Option<f32>) -> Vec<(Range<usize>, HighlightStyle)> {
    let amber = rgba(0xFFB340FF);
    let core = rgba(0xFFE9C6FF);
    text.char_indices()
        .enumerate()
        .map(|(i, (at, c))| {
            let base = if at < name_len { rgba(0xFFFFFFDB) } else { rgba(0xFFFFFF85) };
            let glow = light.map_or(0.0, |x| {
                let d = i as f32 + 0.5 - x;
                let sigma = if d > 0.0 { LEAD } else { TRAIL };
                (-(d * d) / (2.0 * sigma * sigma)).exp()
            });
            let warm = mix(base, amber, smoothstep(0.7, glow));
            let color = mix(warm, core, (glow - 0.72) / 0.28);
            (at..at + c.len_utf8(), HighlightStyle { color: Some(color.into()), ..Default::default() })
        })
        .collect()
}

/// The top bar's sign that an agent is changing a photo: "Claude is developing", with a band of warm light passing
/// through the words. Under Reduce Motion the words sit still, "developing" in amber.
pub fn pill(presence: Presence) -> AnyElement {
    let calm = motion::reduced();
    let name = crate::mcp::agent();
    let name_len = name.len();
    let text: SharedString = format!("{name} is developing").into();
    let len = text.chars().count() as f32;
    let line = div().id("agent-developing").flex_none().relative().text_size(px(13.)).font_weight(FontWeight::MEDIUM).whitespace_nowrap().text_color(theme::secondary());
    let line = if calm {
        let still: Vec<(Range<usize>, HighlightStyle)> = vec![(name_len..text.len(), HighlightStyle { color: Some(rgba(0xFFB340D9).into()), ..Default::default() })];
        line.child(StyledText::new(text).with_highlights(still)).into_any_element()
    } else {
        let cycle = Duration::from_secs_f32(TRAVEL + REST);
        line.with_animation("agent-light", Animation::new(cycle).repeat(), move |d, t| d.child(StyledText::new(text.clone()).with_highlights(runs(&text, name_len, light_at(t, len)))))
            .into_any_element()
    };
    presence_fade(div().flex_none().relative().child(line), "agent-pill", presence, calm, true)
}

/// A faint accent edge around the photo while an agent edits it, fading like the words; edits to photos that aren't
/// on screen leave it off.
pub fn outline(bounds: Bounds<Pixels>) -> AnyElement {
    let Some(presence) = crate::mcp::on_view() else { return div().into_any_element() };
    let calm = motion::reduced();
    let edge = div()
        .absolute()
        .left(bounds.origin.x)
        .top(bounds.origin.y)
        .w(bounds.size.width)
        .h(bounds.size.height)
        .border_1()
        .border_color(theme::accent().opacity(0.45));
    presence_fade(edge, "agent-outline", presence, calm, false)
}
