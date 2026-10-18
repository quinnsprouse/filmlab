use super::geometry::Vec2;

pub const OPTIONS: [(&str, &str); 11] = [
    ("free", "Free"),
    ("original", "Original"),
    ("1x1", "1 : 1"),
    ("4x5", "4 : 5"),
    ("5x7", "5 : 7"),
    ("2x3", "2 : 3"),
    ("4x3", "4 : 3"),
    ("16x9", "16 : 9"),
    ("16x10", "16 : 10"),
    ("8.5x11", "8.5 : 11"),
    ("custom", "Custom…"),
];

const TOLERANCE: f64 = 0.004;

fn near(ratio: f64, target: f64) -> bool {
    target.is_finite() && target > 0.0 && ((ratio - target).abs() / target < TOLERANCE || (1.0 / ratio - target).abs() / target < TOLERANCE)
}

fn preset_ratio(value: &str) -> Option<f64> {
    let (w, h) = value.split_once('x')?;
    Some(w.parse::<f64>().ok()? / h.parse::<f64>().ok()?)
}

pub fn aspect_value(aspect: Option<Vec2>, original: f64) -> &'static str {
    let Some([w, h]) = aspect else { return "free" };
    let ratio = w / h;
    if near(ratio, original) {
        return "original";
    }
    OPTIONS.iter().find(|(value, _)| preset_ratio(value).is_some_and(|r| near(ratio, r))).map_or("custom", |(value, _)| value)
}

pub fn aspect_name(value: &str) -> &str {
    OPTIONS.iter().find(|(v, _)| *v == value).map_or(value, |(_, label)| label)
}

pub fn parse_ratio(text: &str) -> Option<f64> {
    let value: f64 = text.trim().replace(',', ".").parse().ok()?;
    (value.is_finite() && value > 0.0 && value <= 1000.0).then_some(value)
}
