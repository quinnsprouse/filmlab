use std::collections::HashMap;

use serde_json::{Map, Value, json};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pt {
    pub x: f64,
    pub y: f64,
}

impl Pt {
    pub fn new(x: f64, y: f64) -> Pt {
        Pt { x, y }
    }

    fn json(self) -> Value {
        json!({"x": self.x, "y": self.y})
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MaskOp {
    Add,
    Subtract,
    Intersect,
}

impl MaskOp {
    pub const ALL: [MaskOp; 3] = [MaskOp::Add, MaskOp::Subtract, MaskOp::Intersect];

    pub fn key(self) -> &'static str {
        match self {
            MaskOp::Add => "add",
            MaskOp::Subtract => "subtract",
            MaskOp::Intersect => "intersect",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            MaskOp::Add => "Add",
            MaskOp::Subtract => "Subtract",
            MaskOp::Intersect => "Intersect",
        }
    }

    pub fn sign(self) -> &'static str {
        match self {
            MaskOp::Add => "+",
            MaskOp::Subtract => "−",
            MaskOp::Intersect => "∩",
        }
    }

    fn parse(value: Option<&str>) -> MaskOp {
        match value {
            Some("subtract") => MaskOp::Subtract,
            Some("intersect") => MaskOp::Intersect,
            _ => MaskOp::Add,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CreateKind {
    Linear,
    Radial,
    Brush,
    LuminanceRange,
    ColorRange,
    Subject,
    Sky,
    Background,
}

impl CreateKind {
    pub const ALL: [CreateKind; 8] = [
        CreateKind::Linear,
        CreateKind::Radial,
        CreateKind::Brush,
        CreateKind::LuminanceRange,
        CreateKind::ColorRange,
        CreateKind::Subject,
        CreateKind::Sky,
        CreateKind::Background,
    ];

    pub fn key(self) -> &'static str {
        match self {
            CreateKind::Linear => "linear",
            CreateKind::Radial => "radial",
            CreateKind::Brush => "brush",
            CreateKind::LuminanceRange => "luminanceRange",
            CreateKind::ColorRange => "colorRange",
            CreateKind::Subject => "subject",
            CreateKind::Sky => "sky",
            CreateKind::Background => "background",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            CreateKind::Linear => "Linear gradient",
            CreateKind::Radial => "Radial gradient",
            CreateKind::Brush => "Brush",
            CreateKind::LuminanceRange => "Luminance range",
            CreateKind::ColorRange => "Color range",
            CreateKind::Subject => "Subject",
            CreateKind::Sky => "Sky",
            CreateKind::Background => "Background",
        }
    }

    pub fn short(self) -> &'static str {
        match self {
            CreateKind::Linear => "Linear",
            CreateKind::Radial => "Radial",
            CreateKind::Brush => "Brush",
            CreateKind::LuminanceRange => "Luminance",
            CreateKind::ColorRange => "Color",
            CreateKind::Subject => "Subject",
            CreateKind::Sky => "Sky",
            CreateKind::Background => "Backdrop",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            CreateKind::Linear => "mask-linear",
            CreateKind::Radial => "circle",
            CreateKind::Brush => "paint-brush01",
            CreateKind::LuminanceRange => "sliders-horizontal",
            CreateKind::ColorRange => "color-picker",
            CreateKind::Subject => "user",
            CreateKind::Sky => "sun01",
            CreateKind::Background => "mountain",
        }
    }

    pub fn shortcut(self) -> Option<&'static str> {
        match self {
            CreateKind::Brush => Some("B"),
            CreateKind::Linear => Some("L"),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Radial {
    pub center: Pt,
    pub rx: f64,
    pub ry: f64,
    pub angle: f64,
    pub feather: f64,
    pub invert: bool,
}

#[derive(Clone, Debug)]
pub struct Luminance {
    pub lo: f64,
    pub hi: f64,
    pub lo_feather: f64,
    pub hi_feather: f64,
}

#[derive(Clone, Debug)]
pub struct ColorRange {
    pub samples: Vec<[f64; 3]>,
    pub refine: f64,
}

#[derive(Clone, Debug)]
pub enum Shape {
    Linear { start: Pt, end: Pt },
    Radial(Radial),
    Brush { strokes: usize },
    Luminance(Luminance),
    Color(ColorRange),
    Subject,
    Sky,
    Background,
    Other(String),
}

impl Shape {
    pub fn kind(&self) -> Option<CreateKind> {
        Some(match self {
            Shape::Linear { .. } => CreateKind::Linear,
            Shape::Radial(_) => CreateKind::Radial,
            Shape::Brush { .. } => CreateKind::Brush,
            Shape::Luminance(_) => CreateKind::LuminanceRange,
            Shape::Color(_) => CreateKind::ColorRange,
            Shape::Subject => CreateKind::Subject,
            Shape::Sky => CreateKind::Sky,
            Shape::Background => CreateKind::Background,
            Shape::Other(_) => return None,
        })
    }

    pub fn label(&self) -> String {
        match self {
            Shape::Other(name) => name.clone(),
            shape => shape.kind().map(|kind| kind.label().to_string()).unwrap_or_default(),
        }
    }

    pub fn params(&self) -> Value {
        match self {
            Shape::Linear { start, end } => json!({"kind": "linear", "start": start.json(), "end": end.json()}),
            Shape::Radial(r) => json!({"kind": "radial", "center": r.center.json(), "rx": r.rx, "ry": r.ry, "angle": r.angle, "feather": r.feather, "invert": r.invert}),
            Shape::Brush { strokes } => json!({"kind": "brush", "strokes": strokes}),
            Shape::Luminance(l) => json!({"kind": "luminanceRange", "lo": l.lo, "hi": l.hi, "lo_feather": l.lo_feather, "hi_feather": l.hi_feather}),
            Shape::Color(c) => json!({"kind": "colorRange", "samples": c.samples, "refine": c.refine}),
            Shape::Subject => json!({"kind": "subject"}),
            Shape::Sky => json!({"kind": "sky"}),
            Shape::Background => json!({"kind": "background"}),
            Shape::Other(name) => json!({"kind": "other", "name": name}),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Component {
    pub op: MaskOp,
    pub invert: bool,
    pub shape: Shape,
}

#[derive(Clone, Debug)]
pub struct Mask {
    pub id: u32,
    pub name: String,
    pub visible: bool,
    pub invert: bool,
    pub refine: f64,
    pub components: Vec<Component>,
    pub adjust: HashMap<String, f64>,
}

impl Mask {
    pub fn lead_kind(&self) -> CreateKind {
        self.components.first().and_then(|c| c.shape.kind()).unwrap_or(CreateKind::Subject)
    }

    pub fn component_name(&self, index: usize) -> String {
        self.components.get(index).map(|c| c.shape.label()).unwrap_or_else(|| "Component".into())
    }

    pub fn radial(&self) -> Option<(usize, &Radial)> {
        self.components.iter().enumerate().find_map(|(i, c)| match &c.shape {
            Shape::Radial(r) => Some((i, r)),
            _ => None,
        })
    }
}

fn num(value: Option<&Value>, fallback: f64) -> f64 {
    value.and_then(Value::as_f64).filter(|n| n.is_finite()).unwrap_or(fallback)
}

fn pt(value: Option<&Value>, fallback: Pt) -> Pt {
    match value {
        Some(v) if v.is_object() => Pt::new(num(v.get("x"), fallback.x), num(v.get("y"), fallback.y)),
        _ => fallback,
    }
}

fn samples(value: Option<&Value>) -> Vec<[f64; 3]> {
    let Some(list) = value.and_then(Value::as_array) else { return Vec::new() };
    list.iter()
        .filter_map(|sample| {
            let s = sample.as_array().filter(|s| s.len() >= 3)?;
            Some([num(s.first(), 0.0), num(s.get(1), 0.0), num(s.get(2), 0.0)])
        })
        .collect()
}

fn parse_shape(value: Option<&Value>) -> Shape {
    let Some(v) = value.filter(|v| v.is_object()) else { return Shape::Other("Component".into()) };
    match v.get("kind").and_then(Value::as_str) {
        Some("linear") => Shape::Linear { start: pt(v.get("start"), Pt::new(0.5, 0.25)), end: pt(v.get("end"), Pt::new(0.5, 0.6)) },
        Some("radial") => Shape::Radial(Radial {
            center: pt(v.get("center"), Pt::new(0.5, 0.5)),
            rx: num(v.get("rx"), 0.22),
            ry: num(v.get("ry"), 0.16),
            angle: num(v.get("angle"), 0.0),
            feather: num(v.get("feather"), 50.0),
            invert: v.get("invert").and_then(Value::as_bool) == Some(true),
        }),
        Some("brush") => Shape::Brush { strokes: v.get("strokes").and_then(Value::as_array).map_or(0, Vec::len) },
        Some("luminanceRange") => Shape::Luminance(Luminance {
            lo: num(v.get("lo"), 0.6),
            hi: num(v.get("hi"), 1.0),
            lo_feather: num(v.get("lo_feather"), 0.1),
            hi_feather: num(v.get("hi_feather"), 0.1),
        }),
        Some("colorRange") => Shape::Color(ColorRange { samples: samples(v.get("samples")), refine: num(v.get("refine"), 50.0) }),
        Some("subject") => Shape::Subject,
        Some("sky") => Shape::Sky,
        Some("background") => Shape::Background,
        Some("object") => Shape::Other("Object".into()),
        Some("prompt") => Shape::Other("Described object".into()),
        Some("depthRange") => Shape::Other("Depth range".into()),
        Some("people") => Shape::Other("People".into()),
        Some("landscape") => Shape::Other("Landscape".into()),
        _ => Shape::Other("Component".into()),
    }
}

fn parse_mask(value: &Value) -> Option<Mask> {
    let id = value.get("id").and_then(Value::as_u64)? as u32;
    let adjust: HashMap<String, f64> = value
        .get("adjust")
        .and_then(Value::as_object)
        .map(|map: &Map<String, Value>| map.iter().map(|(k, v)| (k.clone(), num(Some(v), 0.0))).collect())
        .unwrap_or_default();
    let components = value
        .get("components")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter(|c| c.is_object())
                .map(|c| Component {
                    op: MaskOp::parse(c.get("op").and_then(Value::as_str)),
                    invert: c.get("invert").and_then(Value::as_bool) == Some(true),
                    shape: parse_shape(c.get("shape")),
                })
                .collect()
        })
        .unwrap_or_default();
    Some(Mask {
        id,
        name: value.get("name").and_then(Value::as_str).map_or_else(|| format!("Mask {id}"), str::to_string),
        visible: value.get("visible").and_then(Value::as_bool) != Some(false),
        invert: value.get("invert").and_then(Value::as_bool) == Some(true),
        refine: num(value.get("refine"), 0.0),
        components,
        adjust,
    })
}

pub fn read_masks(settings: Option<&Value>) -> Vec<Mask> {
    settings.and_then(|s| s.get("masks")).and_then(Value::as_array).map(|list| list.iter().filter_map(parse_mask).collect()).unwrap_or_default()
}
