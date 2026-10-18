use std::collections::HashMap;
use std::sync::LazyLock;

use filmlab_engine::core;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use serde::Deserialize;
use serde_json::{Value, json};

use super::app::FilmlabApp;
use super::controls::{Picker, PickerItem, Segmented, Toggle, header};
use super::panel::{GRADING_REGIONS, MIXER_BANDS, Row, SECTIONS, sentence};
use super::slider::ParamSlider;
use super::{icon, theme};
use crate::develop::Develop;
use crate::film::{FilmSettings, GAUGES};
use crate::stocks::{DEFAULT_NEGATIVE, DEFAULT_PRINT, Stock, StockKind};

#[derive(Deserialize, Clone)]
pub struct Spec {
    pub min: f64,
    pub max: f64,
    pub default: f64,
    pub step: f64,
    pub decimals: usize,
}

static SPECS: LazyLock<HashMap<String, Spec>> = LazyLock::new(|| {
    #[derive(Deserialize)]
    struct Raw {
        id: String,
        #[serde(flatten)]
        spec: Spec,
    }
    let raw: Vec<Raw> = core::controls().ok().and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_default();
    raw.into_iter().map(|r| (r.id, r.spec)).collect()
});

pub fn spec(id: &str) -> Option<&'static Spec> {
    SPECS.get(id)
}

fn format(spec: &Spec, value: f64) -> String {
    let text = format!("{value:.prec$}", prec = spec.decimals);
    if value > 0.0 && spec.min < 0.0 { format!("+{text}") } else { text }
}

#[derive(Deserialize)]
struct WhiteBalance {
    mode: String,
    temp: f64,
    tint: f64,
    kelvin: bool,
}

const WB_MODES: [(&str, &str); 9] = [
    ("asShot", "As shot"),
    ("auto", "Auto"),
    ("daylight", "Daylight"),
    ("cloudy", "Cloudy"),
    ("shade", "Shade"),
    ("tungsten", "Tungsten"),
    ("fluorescent", "Fluorescent"),
    ("flash", "Flash"),
    ("custom", "Custom"),
];

pub struct Ui {
    pub open: HashMap<&'static str, bool>,
    pub mixer: usize,
    pub grading: usize,
}

impl Default for Ui {
    fn default() -> Self {
        Ui { open: SECTIONS.iter().map(|s| (s.id, s.open)).collect(), mixer: 0, grading: 1 }
    }
}

type Rows = Vec<AnyElement>;

fn control(id: impl Into<String>, label: impl Into<String>, values: &HashMap<String, f64>, develop: &Entity<Develop>) -> Option<AnyElement> {
    let (id, label): (String, String) = (id.into(), label.into());
    let spec = spec(&id)?;
    let d = develop.clone();
    let reset = spec.default;
    let value = values.get(&id).copied().unwrap_or(0.0);
    Some(
        ParamSlider::new(SharedString::from(id.clone()), label.clone(), value, spec.min, spec.max)
            .step(spec.step)
            .decimals(spec.decimals)
            .fallback(reset)
            .on_change(move |v, _, cx| {
                let text = format(spec, v);
                d.update(cx, |d, cx| d.edit_controls(&[(id.as_str(), v)], &format!("{label} {text}"), cx));
            })
            .into_any_element(),
    )
}

fn film_rows(film: &FilmSettings, stocks: &[Stock], develop: &Entity<Develop>) -> Rows {
    let mut rows: Rows = Vec::new();
    let available = |id: &str| stocks.iter().any(|s| s.id == id);
    for (kind, label) in [(StockKind::Negative, "Negative"), (StockKind::Print, "Print"), (StockKind::Utility, "Utility")] {
        let current = match kind {
            StockKind::Negative => film.negative.clone(),
            StockKind::Print => film.print.clone(),
            StockKind::Utility => film.utility.clone(),
        };
        let mut options: Vec<(Option<String>, String)> = vec![(None, "None".into())];
        options.extend(stocks.iter().filter(|s| s.kind == kind).map(|s| (Some(s.id.clone()), s.label())));
        let shown = options.iter().find(|(id, _)| *id == current).map(|(_, l)| l.clone()).unwrap_or_else(|| "None".into());
        let listed: Vec<(bool, String)> = options.iter().map(|(id, l)| (*id == current, l.clone())).collect();
        let d = develop.clone();
        let base = film.clone();
        let fallback_negative = available(DEFAULT_NEGATIVE).then(|| DEFAULT_NEGATIVE.to_string());
        let fallback_print = available(DEFAULT_PRINT).then(|| DEFAULT_PRINT.to_string());
        rows.push(
            Picker::new(
                SharedString::from(format!("stock-{label}")),
                label,
                shown,
                move || listed.iter().map(|(checked, l)| PickerItem { label: l.clone(), checked: *checked, header: false }).collect(),
                move |i, _, cx| {
                    let (id, name) = options[i].clone();
                    let mut next = base.clone();
                    match kind {
                        StockKind::Negative => {
                            next.negative = id.clone();
                            if id.is_some() && next.print.is_none() {
                                next.print = fallback_print.clone();
                            }
                        }
                        StockKind::Print => {
                            next.print = id.clone();
                            if id.is_some() && next.negative.is_none() {
                                next.negative = fallback_negative.clone();
                            }
                        }
                        StockKind::Utility => next.utility = id,
                    }
                    d.update(cx, |d, cx| d.edit_film(next, &format!("{label} {name}"), &format!("film.{label}"), cx));
                },
            )
            .into_any_element(),
        );
    }
    rows
}

fn needs_stock() -> AnyElement {
    div().py(px(4.)).text_size(px(13.)).text_color(theme::tertiary()).child("Choose a negative and a print to add halation, grain and dust.").into_any_element()
}

// why: Film Look Creator's own halation controls, as FilmVision's PowerGrade sets them
fn halation_rows(film: &FilmSettings, develop: &Entity<Develop>) -> Rows {
    let h = film.halation;
    let stock = crate::film::Halation::stock(film.gauge().unwrap_or(crate::film::Gauge::G65));
    let edit = |develop: &Entity<Develop>, film: &FilmSettings, key: &'static str, label: String, change: Box<dyn Fn(&mut crate::film::Halation)>| {
        let mut next = film.clone();
        change(&mut next.halation);
        let d = develop.clone();
        move |cx: &mut App| d.update(cx, |d, cx| d.edit_film(next.clone(), &label, &format!("film.halation.{key}"), cx))
    };
    let (d, f) = (develop.clone(), film.clone());
    let mut rows: Rows = vec![
        Toggle::new("hal-on", "Halation", h.enabled(), move |on, _, cx| edit(&d, &f, "on", format!("Halation {}", if on { "on" } else { "off" }), Box::new(move |h| h.on = Some(on)))(cx))
            .into_any_element(),
    ];
    let slider = |id: &'static str, label: &'static str, value: f64, min: f64, max: f64, step: f64, reset: f64, apply: fn(&mut crate::film::Halation, f64)| {
        let (d, f) = (develop.clone(), film.clone());
        ParamSlider::new(id, label, value, min, max)
            .step(step)
            .fallback(reset)
            .disabled(!h.enabled())
            .on_change(move |v, _, cx| edit(&d, &f, id, format!("Halation {} {v}", label.to_lowercase()), Box::new(move |h| apply(h, v)))(cx))
            .into_any_element()
    };
    rows.push(slider("amount", "Amount", (h.amount * 100.0).round(), 0.0, 100.0, 1.0, stock.amount * 100.0, |h, v| h.amount = v / 100.0));
    rows.push(slider("hue", "Hue", (h.hue.unwrap_or(0.45) * 100.0).round(), 0.0, 100.0, 1.0, 45.0, |h, v| h.hue = Some(v / 100.0)));
    rows.push(slider("sat", "Saturation", (h.sat.unwrap_or(1.0) * 100.0).round(), 0.0, 100.0, 1.0, 100.0, |h, v| h.sat = Some(v / 100.0)));
    rows.push(slider("radius", "Radius", h.radius, 1.0, 10.0, 0.1, stock.radius, |h, v| h.radius = v));
    let (d, f) = (develop.clone(), film.clone());
    let only = h.highlights_only.unwrap_or(true);
    rows.push(
        Toggle::new("hal-highlights", "Highlights only", only, move |on, _, cx| {
            edit(&d, &f, "highlightsOnly", format!("Halation highlights only {}", if on { "on" } else { "off" }), Box::new(move |h| h.highlights_only = Some(on)))(cx)
        })
        .into_any_element(),
    );
    rows
}

fn grain_rows(film: &FilmSettings, develop: &Entity<Develop>) -> Rows {
    let g = film.grain;
    let fallback = crate::film::Gauge::G65.grain();
    let gauge = film.gauge();
    let mut options: Vec<(SharedString, bool)> = GAUGES.iter().map(|g| (SharedString::from(g.label()), Some(*g) == gauge)).collect();
    if gauge.is_none() {
        options.push(("Custom".into(), true));
    }
    let (d, f) = (develop.clone(), film.clone());
    let mut rows: Rows = vec![
        Segmented::new("gauge", options, move |i, _, cx| {
            let Some(gauge) = GAUGES.get(i) else { return };
            let mut next = f.clone();
            next.grain = gauge.grain();
            next.blur = gauge.blur();
            next.halation.radius = gauge.halation_radius();
            d.update(cx, |d, cx| d.edit_film(next, &format!("Gauge {} mm", gauge.label()), "film.gauge", cx));
        })
        .label("Gauge")
        .into_any_element(),
    ];
    let slider = |id: &'static str, label: &'static str, value: f64, min: f64, max: f64, step: f64, reset: f64, apply: fn(&mut FilmSettings, f64)| {
        let (d, f) = (develop.clone(), film.clone());
        ParamSlider::new(id, label, value, min, max)
            .step(step)
            .fallback(reset)
            .on_change(move |v, _, cx| {
                let mut next = f.clone();
                apply(&mut next, v);
                d.update(cx, |d, cx| d.edit_film(next, &format!("{label} {v}"), id, cx));
            })
            .into_any_element()
    };
    rows.push(slider("film.grain.amount", "Amount", (g.amount * 1000.0).round() / 10.0, 0.0, 100.0, 0.5, fallback.amount * 100.0, |f, v| f.grain.amount = v / 100.0));
    rows.push(slider("film.grain.size", "Size", g.size, 0.0, 4.0, 0.1, fallback.size, |f, v| f.grain.size = v));
    rows.push(slider("film.grain.softness", "Softness", (g.softness * 100.0).round(), 0.0, 100.0, 1.0, fallback.softness * 100.0, |f, v| f.grain.softness = v / 100.0));
    rows.push(slider("film.grain.defocus", "Defocus", ((1.0 - g.defocus) * 100.0).round(), 0.0, 50.0, 1.0, (1.0 - fallback.defocus) * 100.0, |f, v| f.grain.defocus = 1.0 - v / 100.0));
    rows.push(slider("film.blur", "Film blur", (film.blur * 1000.0).round() / 10.0, 0.0, 20.0, 0.5, 5.0, |f, v| f.blur = v / 100.0));
    rows
}

fn dust_rows(film: &FilmSettings, develop: &Entity<Develop>) -> Rows {
    let dust = film.dust.unwrap_or(crate::film::STOCK_DUST);
    let (d, f) = (develop.clone(), film.clone());
    let mut rows: Rows = vec![
        Toggle::new("dust-on", "Dust specks", dust.on, move |on, _, cx| {
            let mut next = f.clone();
            next.dust = Some(crate::film::Dust { on, ..dust });
            d.update(cx, |d, cx| d.edit_film(next, if on { "Dust on" } else { "Dust off" }, "film.dust.on", cx));
        })
        .into_any_element(),
    ];
    let (d, f) = (develop.clone(), film.clone());
    rows.push(
        ParamSlider::new("dust-size", "Size", dust.size, 1.2, 4.0)
            .step(0.01)
            .fallback(1.758)
            .disabled(!dust.on)
            .on_change(move |v, _, cx| {
                let mut next = f.clone();
                next.dust = Some(crate::film::Dust { size: v, ..dust });
                d.update(cx, |d, cx| d.edit_film(next, &format!("Dust size {v:.2}"), "film.dust.size", cx));
            })
            .into_any_element(),
    );
    let (d, f) = (develop.clone(), film.clone());
    rows.push(
        ParamSlider::new("dust-softness", "Softness", dust.softness * 100.0, 0.0, 50.0)
            .step(0.5)
            .fallback(24.5)
            .disabled(!dust.on)
            .on_change(move |v, _, cx| {
                let mut next = f.clone();
                next.dust = Some(crate::film::Dust { softness: v / 100.0, ..dust });
                d.update(cx, |d, cx| d.edit_film(next, &format!("Dust softness {v}"), "film.dust.softness", cx));
            })
            .into_any_element(),
    );
    rows
}

fn white_balance_rows(facts: &Value, settings: &Value, develop: &Entity<Develop>) -> Rows {
    let Some(view) = core::white_balance(&facts.to_string(), &settings.to_string()).ok().and_then(|j| serde_json::from_str::<WhiteBalance>(&j).ok()) else { return Vec::new() };
    let (temp, tint) = if view.kelvin { ((2000.0, 12000.0, 50.0), (-150.0, 150.0, 1.0)) } else { ((-100.0, 100.0, 1.0), (-100.0, 100.0, 1.0)) };
    let current = WB_MODES.iter().find(|(v, _)| *v == view.mode).map(|(_, l)| *l).unwrap_or("Custom");
    let modes: Vec<(&str, &str)> = WB_MODES.iter().copied().filter(|(v, _)| *v != "custom" || view.mode == "custom").collect();
    let listed: Vec<(bool, String)> = modes.iter().map(|(v, l)| (*v == view.mode, l.to_string())).collect();
    let d = develop.clone();
    let mode_rows = Picker::new(
        "wb-mode",
        "White balance",
        current,
        move || listed.iter().map(|(c, l)| PickerItem { label: l.clone(), checked: *c, header: false }).collect(),
        move |i, _, cx| {
            let (mode, label) = modes[i];
            d.update(cx, |d, cx| {
                d.command("develop.wb", json!({"mode": mode}), &format!("White balance {label}"), "wb.mode", cx);
            });
        },
    );
    let wb = |id: &'static str, label: &'static str, value: f64, (min, max, step): (f64, f64, f64)| {
        let d = develop.clone();
        ParamSlider::new(id, label, value, min, max)
            .step(step)
            .decimals(0)
            .on_change(move |v, _, cx| {
                d.update(cx, |d, cx| d.edit_controls(&[(id, v)], &format!("{label} {}", v.round()), cx));
            })
            .into_any_element()
    };
    vec![mode_rows.into_any_element(), wb("wb.temp", "Temp", view.temp, temp), wb("wb.tint", "Tint", view.tint, tint)]
}

fn row(app: &FilmlabApp, r: &'static Row, ctx: &Ctx, rows: &mut Rows, cx: &mut Context<FilmlabApp>) {
    let develop = &app.develop;
    match r {
        Row::Slider(id, label) => rows.extend(control(*id, *label, &ctx.values, develop)),
        Row::Toggle(section, field, label) => {
            let on = ctx.settings[section][field].as_bool().unwrap_or(false);
            let d = develop.clone();
            rows.push(
                Toggle::new(SharedString::from(format!("{section}.{field}")), *label, on, move |next, _, cx| {
                    d.update(cx, |d, cx| d.edit_settings(json!({ *section: { *field: next } }), &format!("{label} {}", if next { "on" } else { "off" }), &format!("{section}.{field}"), cx));
                })
                .into_any_element(),
            );
        }
        Row::Treatment => {
            let bw = ctx.settings["treatment"].as_str() == Some("bw");
            let d = develop.clone();
            rows.push(
                Segmented::new("treatment", vec![("Color".into(), !bw), ("B&W".into(), bw)], move |i, _, cx| {
                    let (value, label) = if i == 1 { ("bw", "Black & white") } else { ("color", "Color") };
                    d.update(cx, |d, cx| d.edit_settings(json!({"treatment": value}), label, "treatment", cx));
                })
                .label("Treatment")
                .into_any_element(),
            );
        }
        Row::WhiteBalance => rows.extend(white_balance_rows(&ctx.facts, &ctx.settings, develop)),
        Row::FilmStock => rows.extend(film_rows(&ctx.film, &app.stocks, develop)),
        Row::Halation if ctx.film.is_on() => rows.extend(halation_rows(&ctx.film, develop)),
        Row::Grain if ctx.film.is_on() => rows.extend(grain_rows(&ctx.film, develop)),
        Row::Dust if ctx.film.is_on() => rows.extend(dust_rows(&ctx.film, develop)),
        Row::Halation | Row::Grain | Row::Dust => rows.push(needs_stock()),
        Row::Mixer => {
            let bw = ctx.settings["treatment"].as_str() == Some("bw");
            if bw {
                for band in MIXER_BANDS {
                    rows.extend(control(format!("bw.{band}"), sentence(band), &ctx.values, develop));
                }
            } else {
                let channel = ["hue", "sat", "lum"][app.ui.mixer];
                let me = cx.entity();
                rows.push(
                    Segmented::new("mixer-channel", ["Hue", "Saturation", "Luminance"].iter().enumerate().map(|(i, l)| (SharedString::from(*l), i == app.ui.mixer)).collect(), move |i, _, cx| {
                        me.update(cx, |app, cx| {
                            app.ui.mixer = i;
                            cx.notify();
                        })
                    })
                    .label("Adjust")
                    .into_any_element(),
                );
                for band in MIXER_BANDS {
                    rows.extend(control(format!("mixer.{band}.{channel}"), sentence(band), &ctx.values, develop));
                }
            }
        }
        Row::Grading => {
            let region = GRADING_REGIONS[app.ui.grading];
            let me = cx.entity();
            rows.push(
                Segmented::new("grading-region", GRADING_REGIONS.iter().enumerate().map(|(i, r)| (SharedString::from(sentence(r)), i == app.ui.grading)).collect(), move |i, _, cx| {
                    me.update(cx, |app, cx| {
                        app.ui.grading = i;
                        cx.notify();
                    })
                })
                .label("Adjust")
                .into_any_element(),
            );
            for (part, label) in [("hue", "Hue"), ("sat", "Saturation"), ("lum", "Luminance")] {
                rows.extend(control(format!("grading.{region}.{part}"), label, &ctx.values, develop));
            }
            rows.extend(control("grading.blending", "Blending", &ctx.values, develop));
            rows.extend(control("grading.balance", "Balance", &ctx.values, develop));
        }
        Row::VignetteStyle => {
            let style = ctx.settings["vignette"]["style"].as_str().unwrap_or("highlightPriority").to_string();
            let styles = [("highlightPriority", "Highlight"), ("colorPriority", "Color"), ("paintOverlay", "Paint")];
            let d = develop.clone();
            rows.push(
                Segmented::new("vignette-style", styles.iter().map(|(v, l)| (SharedString::from(*l), *v == style)).collect(), move |i, _, cx| {
                    let value = styles[i].0;
                    d.update(cx, |d, cx| d.edit_settings(json!({"vignette": {"style": value}}), "Vignette style", "vignette.style", cx));
                })
                .label("Style")
                .into_any_element(),
            );
        }
        Row::Group(title, children) => {
            let mut inner: Rows = Vec::new();
            for child in children.iter() {
                row(app, child, ctx, &mut inner, cx);
            }
            rows.push(div().flex().flex_col().gap(px(theme::GAP)).child(header(*title)).children(inner).into_any_element());
        }
    }
}

struct Ctx {
    facts: Value,
    settings: Value,
    values: HashMap<String, f64>,
    film: FilmSettings,
}

fn histogram(counts: &[u32]) -> impl IntoElement {
    let bins = 256usize;
    let counts = counts.to_vec();
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            if counts.len() < bins * 3 {
                return;
            }
            let mut inner: Vec<u32> = (0..3).flat_map(|c| (1..bins - 1).map(move |b| (c, b))).map(|(c, b)| counts[c * bins + b]).collect();
            inner.sort_unstable();
            let peak = f64::from(inner[(inner.len() as f64 * 0.995) as usize].max(1));
            let colors = [hsla(0.01, 0.9, 0.62, 0.55), hsla(0.35, 0.6, 0.6, 0.5), hsla(0.6, 0.9, 0.65, 0.55)];
            let w = f32::from(bounds.size.width) / bins as f32;
            let h = f32::from(bounds.size.height);
            for (c, color) in colors.into_iter().enumerate() {
                for b in 0..bins {
                    let v = (f64::from(counts[c * bins + b]) / peak).min(1.0) as f32 * (h - 2.0);
                    if v < 0.5 {
                        continue;
                    }
                    let origin = point(bounds.left() + px(b as f32 * w), bounds.bottom() - px(v));
                    window.paint_quad(fill(Bounds::new(origin, size(px(w.max(1.0)), px(v))), color));
                }
            }
        },
    )
    .h(px(60.))
    .w_full()
}

pub fn inspector(app: &FilmlabApp, cx: &mut Context<FilmlabApp>) -> AnyElement {
    let d = app.develop.read(cx);
    let (Some(facts), Some(settings)) = (d.facts.clone(), d.settings.clone()) else {
        return div().p_4().text_size(px(12.)).text_color(theme::tertiary()).child("Open a photo to develop it.").into_any_element();
    };
    let ctx = Ctx { values: d.control_values(), facts, settings, film: d.film.clone().map(|f| f.normalized()).unwrap_or_default() };
    let counts = d.fit.as_ref().map(|f| f.histogram.clone()).unwrap_or_default();
    let mut sections: Vec<AnyElement> = Vec::new();
    for section in SECTIONS {
        let open = app.ui.open.get(section.id).copied().unwrap_or(section.open);
        let me = cx.entity();
        let id = section.id;
        let mut rows: Rows = Vec::new();
        if open {
            for r in section.rows {
                row(app, r, &ctx, &mut rows, cx);
            }
        }
        sections.push(
            div()
                .flex()
                .flex_col()
                .child(
                    div()
                        .id(SharedString::from(format!("section-{id}")))
                        .h(px(36.))
                        .flex()
                        .items_center()
                        .justify_between()
                        .cursor_pointer()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(move |_, _, cx| {
                            me.update(cx, |app, cx| {
                                app.ui.open.insert(id, !open);
                                cx.notify();
                            })
                        })
                        .child(div().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).text_color(theme::label()).child(section.title))
                        .child(div().opacity(0.7).child(icon::icon(if open { "arrow-down01" } else { "arrow-right01" }, px(14.)))),
                )
                .when(open, |d| d.child(div().flex().flex_col().gap(px(theme::GAP)).pb(px(10.)).children(rows)))
                .into_any_element(),
        );
    }
    let count = sections.len();
    div()
        .id("inspector-scroll")
        .size_full()
        .overflow_y_scroll()
        .px(px(12.))
        .pt(px(12.))
        .pb(px(8.))
        .flex()
        .flex_col()
        .child(div().h(px(28.)).flex().items_center().text_size(px(15.)).font_weight(FontWeight::SEMIBOLD).text_color(theme::focus()).child("Develop"))
        .child(div().mt(px(4.)).mb(px(8.)).p(px(6.)).rounded(px(theme::RADIUS)).bg(theme::surface()).child(histogram(&counts)))
        .children(sections.into_iter().enumerate().flat_map(move |(i, s)| {
            let mut parts = vec![s];
            if i + 1 < count {
                parts.push(div().h(px(1.)).bg(theme::divider()).into_any_element());
            }
            parts
        }))
        .into_any_element()
}
