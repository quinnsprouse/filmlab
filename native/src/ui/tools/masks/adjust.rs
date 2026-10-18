use gpui_kit::*;

use super::commands::{set_adjust, set_refine, update_shape};
use super::model::{Mask, Radial, Shape};
use super::widgets::{folder, on_flag, on_value};
use crate::ui::app::FilmlabApp;
use crate::ui::controls::Toggle;
use crate::ui::slider::ParamSlider;

struct Field {
    id: &'static str,
    label: &'static str,
    min: f64,
    max: f64,
    step: f64,
    initial: f64,
}

const fn symmetric(id: &'static str, label: &'static str) -> Field {
    Field { id, label, min: -100.0, max: 100.0, step: 1.0, initial: 0.0 }
}

struct Group {
    id: &'static str,
    title: &'static str,
    open: bool,
    fields: &'static [Field],
}

const GROUPS: &[Group] = &[
    Group {
        id: "masks-light",
        title: "Light",
        open: true,
        fields: &[
            Field { id: "exposure", label: "Exposure", min: -4.0, max: 4.0, step: 0.01, initial: 0.0 },
            symmetric("contrast", "Contrast"),
            symmetric("highlights", "Highlights"),
            symmetric("shadows", "Shadows"),
            symmetric("whites", "Whites"),
            symmetric("blacks", "Blacks"),
        ],
    },
    Group {
        id: "masks-color",
        title: "Color",
        open: false,
        fields: &[
            symmetric("temp", "Temp"),
            symmetric("tint", "Tint"),
            symmetric("hue", "Hue"),
            symmetric("saturation", "Saturation"),
            Field { id: "color_hue", label: "Overlay hue", min: 0.0, max: 360.0, step: 1.0, initial: 0.0 },
            Field { id: "color_sat", label: "Overlay saturation", min: 0.0, max: 100.0, step: 1.0, initial: 0.0 },
        ],
    },
    Group { id: "masks-effects", title: "Effects", open: false, fields: &[symmetric("texture", "Texture"), symmetric("clarity", "Clarity"), symmetric("dehaze", "Dehaze")] },
    Group {
        id: "masks-detail",
        title: "Detail",
        open: false,
        fields: &[symmetric("sharpness", "Sharpness"), symmetric("noise", "Noise"), symmetric("moire", "Moiré"), symmetric("defringe", "Defringe")],
    },
];

const AMOUNT: Field = Field { id: "amount", label: "Amount", min: 0.0, max: 200.0, step: 1.0, initial: 100.0 };

fn format(field: &Field, value: f64) -> String {
    let digits = if field.step < 1.0 { 2 } else { 0 };
    format!("{value:.digits$}")
}

fn field_slider(mask: &Mask, field: &'static Field, cx: &mut Context<FilmlabApp>) -> AnyElement {
    let id = mask.id;
    let value = mask.adjust.get(field.id).copied().unwrap_or(field.initial).clamp(field.min, field.max);
    ParamSlider::new(SharedString::from(format!("mask.{}", field.id)), field.label, value, field.min, field.max)
        .step(field.step)
        .fallback(field.initial)
        .on_change(on_value(cx, move |app, next, cx| {
            let label = if (next - field.initial).abs() < f64::EPSILON { format!("{} reset", field.label) } else { format!("Mask {} {}", field.label.to_lowercase(), format(field, next)) };
            set_adjust(app, id, field.id, next, &label, cx);
        }))
        .into_any_element()
}

pub fn mask_adjust(app: &FilmlabApp, mask: &Mask, cx: &mut Context<FilmlabApp>) -> Vec<AnyElement> {
    let id = mask.id;
    let mut rows = vec![field_slider(mask, &AMOUNT, cx)];
    for group in GROUPS {
        rows.push(folder(group.id, group.title, group.open, true, cx, app, |cx| group.fields.iter().map(|field| field_slider(mask, field, cx)).collect()));
    }
    rows.push(
        ParamSlider::new("mask.refine", "Refine edges", mask.refine, 0.0, 100.0)
            .fallback(0.0)
            .on_change(on_value(cx, move |app, value, cx| set_refine(app, id, value, cx)))
            .into_any_element(),
    );
    rows
}

pub fn radial_controls(mask: &Mask, cx: &mut Context<FilmlabApp>) -> Vec<AnyElement> {
    let Some((index, radial)) = mask.radial() else { return Vec::new() };
    let id = mask.id;
    let (for_slider, for_toggle) = (radial.clone(), radial.clone());
    vec![
        ParamSlider::new("masks-radial-feather", "Feather", radial.feather, 0.0, 100.0)
            .fallback(50.0)
            .on_change(on_value(cx, move |app, feather, cx| {
                let shape = Shape::Radial(Radial { feather, ..for_slider.clone() });
                update_shape(app, id, index, &shape, "Mask feather", cx);
            }))
            .into_any_element(),
        Toggle::new("masks-radial-invert", "Invert area", radial.invert, on_flag(cx, move |app, invert, cx| {
            let shape = Shape::Radial(Radial { invert, ..for_toggle.clone() });
            update_shape(app, id, index, &shape, "Invert area", cx);
        }))
        .into_any_element(),
    ]
}
