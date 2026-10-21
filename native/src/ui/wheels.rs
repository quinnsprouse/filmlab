use std::rc::Rc;
use std::sync::{Arc, LazyLock};

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::controls::Placed;
use super::oklch::{self, Reach};
use super::slider::{Nothing, released};
use super::theme;

const COLUMN: f32 = 64.0;
const RING: f32 = 60.0;
const DISC: f32 = 52.0;
const DISC_PIXELS: u32 = 104;
const PUCK: f32 = 10.0;
const PUCK_OUTLINE: f32 = 1.0;
const NEUTRAL: f64 = 0.66;
const PUCK_LIGHTNESS: f64 = 0.7;
const REACH: Reach = Reach { share_of_gamut: 1.0, cap: 0.19 };
const FULL_TINT_AT: f64 = 40.0;
const GRAY_FADE_BY_CORNER_DISTANCE: [(f64, f64); 3] = [(0.0, 1.0), (0.3, 140.0 / 255.0), (0.78, 0.0)];

#[derive(Clone, Copy)]
pub struct Tint {
    pub hue: f64,
    pub sat: f64,
}

impl Tint {
    pub const NONE: Tint = Tint { hue: 0.0, sat: 0.0 };
}

pub struct Wheel {
    pub region: &'static str,
    pub label: SharedString,
    pub tint: Tint,
}

type OnSelect = Rc<dyn Fn(usize, &mut Window, &mut App)>;
type OnTint = Rc<dyn Fn(usize, Tint, &mut Window, &mut App)>;

#[derive(Clone)]
struct WheelDrag(usize);

#[derive(IntoElement)]
pub struct GradingWheels {
    wheels: Vec<Wheel>,
    selected: usize,
    placed: Placed,
    on_select: OnSelect,
    on_tint: OnTint,
}

impl GradingWheels {
    pub fn new(wheels: Vec<Wheel>, selected: usize, placed: Placed, on_select: impl Fn(usize, &mut Window, &mut App) + 'static, on_tint: impl Fn(usize, Tint, &mut Window, &mut App) + 'static) -> Self {
        GradingWheels { wheels, selected, placed, on_select: Rc::new(on_select), on_tint: Rc::new(on_tint) }
    }

    pub fn disc_id(region: &str) -> String {
        format!("grading-wheel-{region}")
    }

    fn disc(&self, index: usize, wheel: &Wheel) -> impl IntoElement {
        let id = Self::disc_id(wheel.region);
        let (select, reset, tint) = (self.on_select.clone(), self.on_tint.clone(), self.on_tint.clone());
        div()
            .id(SharedString::from(id.clone()))
            .relative()
            .size(px(DISC))
            .rounded_full()
            .cursor_crosshair()
            .when_some(DISC_IMAGE.clone(), |d, image| d.child(img(image).absolute().inset_0().size_full().rounded_full()))
            .child(div().absolute().inset_0().rounded_full().border_1().border_color(theme::outline()))
            .child(puck(wheel.tint))
            .child(self.placed.record(id))
            .on_mouse_down(MouseButton::Left, move |event, window, cx| {
                cx.stop_propagation();
                select(index, window, cx);
                if event.click_count >= 2 {
                    reset(index, Tint::NONE, window, cx);
                }
            })
            .on_mouse_up(MouseButton::Left, |_, _, cx| released(cx))
            .on_mouse_up_out(MouseButton::Left, |_, _, cx| released(cx))
            .on_drag(WheelDrag(index), |_, _, _, cx| cx.new(|_| Nothing))
            .on_drag_move(move |event: &DragMoveEvent<WheelDrag>, window, cx| {
                if event.drag(cx).0 == index {
                    tint(index, tint_at(event.event.position, event.bounds), window, cx);
                }
            })
    }
}

impl RenderOnce for GradingWheels {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let columns = self.wheels.iter().enumerate().map(|(i, wheel)| {
            let selected = i == self.selected;
            div()
                .w(px(COLUMN))
                .flex_none()
                .flex()
                .flex_col()
                .items_center()
                .gap(px(6.))
                .child(
                    div()
                        .size(px(RING))
                        .rounded_full()
                        .border(px(1.5))
                        .border_color(if selected { theme::ring() } else { transparent_black() })
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(self.disc(i, wheel)),
                )
                .child(div().text_size(px(12.)).line_height(px(16.)).font_weight(FontWeight::MEDIUM).text_color(if selected { theme::primary() } else { theme::tertiary() }).child(wheel.label.clone()))
        });
        div().flex().justify_between().pt(px(4.)).pb(px(6.)).children(columns)
    }
}

fn tint_at(position: Point<Pixels>, disc: Bounds<Pixels>) -> Tint {
    let centre = disc.center();
    let radius = f64::from(f32::from(disc.size.width)) / 2.0;
    let (dx, dy) = (f64::from(f32::from(position.x - centre.x)), f64::from(f32::from(centre.y - position.y)));
    Tint { hue: dy.atan2(dx).to_degrees().rem_euclid(360.0).round() % 360.0, sat: (dx.hypot(dy) / radius.max(1.0) * 100.0).min(100.0).round() }
}

fn puck(tint: Tint) -> impl IntoElement {
    let radius = f64::from(DISC) / 2.0;
    let (sin, cos) = tint.hue.to_radians().sin_cos();
    let reach = radius * tint.sat.clamp(0.0, 100.0) / 100.0;
    let outer = PUCK + 2.0 * PUCK_OUTLINE;
    let (x, y) = ((radius + cos * reach) as f32 - outer / 2.0, (radius - sin * reach) as f32 - outer / 2.0);
    div()
        .absolute()
        .left(px(x))
        .top(px(y))
        .size(px(outer))
        .rounded_full()
        .bg(theme::puck_outline())
        .flex()
        .items_center()
        .justify_center()
        .child(div().size(px(PUCK)).rounded_full().border_2().border_color(theme::handle()).bg(puck_color(tint)))
}

fn puck_color(tint: Tint) -> Rgba {
    if tint.sat <= 0.0 {
        oklch::gray(NEUTRAL)
    } else {
        REACH.color(PUCK_LIGHTNESS, (tint.sat / FULL_TINT_AT).min(1.0), oklch::hue_of_hsv(tint.hue))
    }
}

static DISC_IMAGE: LazyLock<Option<Arc<RenderImage>>> = LazyLock::new(disc_image);

fn disc_image() -> Option<Arc<RenderImage>> {
    let neutral = oklch::gray(NEUTRAL);
    let centre = f64::from(DISC_PIXELS) / 2.0;
    let mut rgba = Vec::with_capacity((DISC_PIXELS * DISC_PIXELS * 4) as usize);
    for y in 0..DISC_PIXELS {
        for x in 0..DISC_PIXELS {
            let (dx, dy) = (f64::from(x) + 0.5 - centre, centre - (f64::from(y) + 0.5));
            let vivid = REACH.color(NEUTRAL, 1.0, oklch::hue_of_hsv(dy.atan2(dx).to_degrees()));
            let gray = gray_share(dx.hypot(dy) / centre) as f32;
            let mix = |v: f32, n: f32| ((v + (n - v) * gray) * 255.0).round() as u8;
            rgba.extend([mix(vivid.r, neutral.r), mix(vivid.g, neutral.g), mix(vivid.b, neutral.b), 255]);
        }
    }
    crate::pictures::image_of(DISC_PIXELS, DISC_PIXELS, rgba)
}

fn gray_share(radius: f64) -> f64 {
    let corner = radius / std::f64::consts::SQRT_2;
    GRAY_FADE_BY_CORNER_DISTANCE
        .windows(2)
        .find(|w| corner <= w[1].0)
        .map(|w| {
            let ((from, a), (to, b)) = (w[0], w[1]);
            a + (b - a) * (corner - from) / (to - from)
        })
        .unwrap_or(0.0)
}
