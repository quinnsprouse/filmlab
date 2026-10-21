use std::path::Path;

use filmlab_engine::core::{Engine, probe, read_sidecar};
use filmlab_engine::tone_curve::{Curves, on_curve};
use lightcraft_engine::develop::ToneCurve;
use lightcraft_engine::pipeline::film_hook::with_curve_input;
use lightcraft_engine::pipeline::finish::curve_luts;
use serde_json::{Value, json};

fn photo() -> (Vec<u8>, String, Value) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let raw = std::fs::read(dir.join(".cache/ricoh-gr3.dng")).expect("the fixture raw (run scripts/fixtures.sh)");
    let xmp = std::fs::read_to_string(dir.join("ricoh-gr3.xmp")).expect("the fixture sidecar");
    let probed: Value = serde_json::from_str(&probe("R0000001.DNG", &raw).unwrap()).unwrap();
    let facts = probed["facts"].to_string();
    let state: Value = serde_json::from_str(&read_sidecar(&facts, &xmp).unwrap()).unwrap();
    (raw, facts, state["settings"].clone())
}

fn curve() -> Value {
    let points = |p: &[[f64; 2]]| p.iter().map(|[x, y]| json!({"x": x, "y": y})).collect::<Vec<_>>();
    json!({
        "darks": 24.0,
        "highlights": -18.0,
        "split_shadows": 30.0,
        "master": points(&[[0.0, 0.04], [0.25, 0.25], [0.72, 0.76], [1.0, 0.97]]),
        "red": points(&[[0.0, 0.0], [0.62, 0.66], [1.0, 1.0]]),
        "blue": points(&[[0.0, 0.07], [0.5, 0.49], [1.0, 0.93]]),
    })
}

fn curved(settings: &Value) -> Value {
    let mut s = settings.clone();
    s["curve"] = curve();
    s["grain"]["amount"] = json!(0.0);
    s
}

fn render(engine: &mut Engine, settings: &Value, view: Value) -> Vec<u8> {
    engine.render("photo", &settings.to_string(), &view.to_string()).unwrap().take_rgba()
}

#[test]
fn the_curves_an_editor_draws_are_the_ones_a_render_applies() {
    let tone: ToneCurve = serde_json::from_value(curve()).unwrap();
    let applied = curve_luts(&tone).expect("the curve is in use");
    let curves = Curves::of(&curve().to_string()).unwrap();
    let into_channels = curves.master.after(&curves.regions);
    for (channel, lut) in curves.channels.iter().zip(&applied) {
        let drawn = channel.after(&into_channels);
        let worst = (0..=1000).map(|i| f64::from(i) / 1000.0).map(|x| (drawn.at(x) - f64::from(lut.eval(x as f32))).abs()).fold(0.0, f64::max);
        assert!(worst < 1e-5, "the drawn curve is {worst} off the applied one");
    }
    assert!(!curves.regions.is_identity() && !curves.master.is_identity() && curves.channels[1].is_identity());
    assert!(Curves::of("null").unwrap().regions.is_identity(), "no curve section is the identity");
}

#[test]
fn a_point_added_on_the_line_sits_on_it() {
    let points = [[0.0, 0.0], [0.25, 0.2], [0.5, 0.5], [0.75, 0.8], [1.0, 1.0]];
    let curves = Curves::of(&json!({"master": points.iter().map(|[x, y]| json!({"x": x, "y": y})).collect::<Vec<_>>()}).to_string()).unwrap();
    for x in [0.1, 0.33, 0.6, 0.9] {
        assert!((on_curve(&points, x) - curves.master.at(x)).abs() < 1e-4);
    }
    assert_eq!(on_curve(&[[0.0, 0.0], [1.0, 1.0]], 0.3), 0.3, "a straight line keeps a point on it exactly");
}

#[test]
fn a_curve_input_render_holds_the_values_entering_the_curve() {
    let (raw, facts, settings) = photo();
    let mut engine = Engine::new();
    engine.load("photo", &facts, &raw, 640).unwrap();
    let edited = curved(&settings);
    let view = json!({"maxW": 320, "maxH": 320, "stages": "loupe"});
    let shown = render(&mut engine, &edited, view.clone());
    let input = render(&mut engine, &edited, json!({"maxW": 320, "maxH": 320, "stages": "loupe", "curveInput": true}));
    let mut flat = edited.clone();
    flat["curve"] = json!({});
    let without = render(&mut engine, &flat, view);

    assert!(input == without, "the curve input isn't the render without the curve");
    let curves = Curves::of(&curve().to_string()).unwrap();
    let into_channels = curves.master.after(&curves.regions);
    let applied = curves.channels.each_ref().map(|c| c.after(&into_channels));
    let deltas: Vec<u8> = input
        .as_chunks::<4>()
        .0
        .iter()
        .zip(shown.as_chunks::<4>().0)
        .flat_map(|(i, s)| (0..3).map(move |c| (i[c], s[c], c)))
        .map(|(i, s, c)| ((applied[c].at(f64::from(i) / 255.0) * 255.0).round() as u8).abs_diff(s))
        .collect();
    let worst = deltas.iter().copied().max().unwrap_or(0);
    let mean = deltas.iter().map(|d| f64::from(*d)).sum::<f64>() / deltas.len() as f64;
    let moved = input.iter().zip(&shown).filter(|(a, b)| a != b).count() as f64 / input.len() as f64;
    assert!(moved > 0.3, "the curve barely changes the render ({moved})");
    assert!(worst <= 2 && mean < 0.35, "the curves applied to the curve input miss the render by up to {worst} (mean {mean})");
}

#[test]
fn a_render_ignores_a_curve_input_pass_left_on_its_thread() {
    let (raw, facts, settings) = photo();
    let mut engine = Engine::new();
    engine.load("photo", &facts, &raw, 640).unwrap();
    let edited = curved(&settings);
    let view = json!({"maxW": 320, "maxH": 320});

    let alone = render(&mut engine, &edited, view.clone());
    let nested = with_curve_input(|| render(&mut engine, &edited, view));

    assert!(nested == alone, "the render took the curve input pass of another render on its thread");
}
