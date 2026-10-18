use serde::{Deserialize, Serialize};
use serde_json::Value;

// why: FilmVision's PowerGrades turn Film Look Creator's halation on at amount 0.5, hue 0.45, saturation 1,
// highlights only, with the radius set by the gauge
// why: photos edited before halation was identified stored {amount, radius, threshold}; the new fields are
// optional so those read back and are replaced by FilmVision's stock halation
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Halation {
    #[serde(default)]
    pub on: Option<bool>,
    pub amount: f64,
    #[serde(default)]
    pub hue: Option<f64>,
    #[serde(default)]
    pub sat: Option<f64>,
    pub radius: f64,
    #[serde(default)]
    pub highlights_only: Option<bool>,
}

impl Halation {
    pub fn stock(gauge: Gauge) -> Halation {
        Halation { on: Some(true), amount: 0.5, hue: Some(0.45), sat: Some(1.0), radius: gauge.halation_radius(), highlights_only: Some(true) }
    }

    fn identified(&self) -> bool {
        self.on.is_some() && self.hue.is_some() && self.sat.is_some() && self.highlights_only.is_some()
    }

    pub fn enabled(&self) -> bool {
        self.on.unwrap_or(true)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Grain {
    pub amount: f64,
    pub size: f64,
    #[serde(default = "nan")]
    pub softness: f64,
    #[serde(default = "nan")]
    pub defocus: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Dust {
    pub on: bool,
    pub size: f64,
    pub softness: f64,
}

fn nan() -> f64 {
    f64::NAN
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FilmSettings {
    pub negative: Option<String>,
    pub print: Option<String>,
    pub utility: Option<String>,
    pub halation: Halation,
    pub grain: Grain,
    #[serde(default = "nan")]
    pub blur: f64,
    #[serde(default)]
    pub dust: Option<Dust>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Gauge {
    G16,
    G35,
    G65,
    G120,
}

pub const GAUGES: [Gauge; 4] = [Gauge::G16, Gauge::G35, Gauge::G65, Gauge::G120];

// why: FilmVision Pro's own gauge PowerGrades (grain amount, size, softness, image defocus, film blur);
// 120 continues their scale
impl Gauge {
    pub fn label(self) -> &'static str {
        match self {
            Gauge::G16 => "16",
            Gauge::G35 => "35",
            Gauge::G65 => "65",
            Gauge::G120 => "120",
        }
    }

    pub fn grain(self) -> Grain {
        match self {
            Gauge::G16 => Grain { amount: 0.25, size: 2.5, softness: 0.25, defocus: 0.85 },
            Gauge::G35 => Grain { amount: 0.1495, size: 2.0, softness: 0.05, defocus: 0.9 },
            Gauge::G65 => Grain { amount: 0.0995, size: 1.0, softness: 0.05, defocus: 1.0 },
            Gauge::G120 => Grain { amount: 0.065, size: 0.6, softness: 0.02, defocus: 1.0 },
        }
    }

    pub fn halation_radius(self) -> f64 {
        match self {
            Gauge::G16 => 6.0,
            Gauge::G35 => 5.0,
            Gauge::G65 => 4.0,
            Gauge::G120 => 3.0,
        }
    }

    pub fn blur(self) -> f64 {
        if self == Gauge::G120 { 0.03 } else { 0.05 }
    }

    fn earlier_amount(self) -> f64 {
        match self {
            Gauge::G16 => 0.045,
            Gauge::G35 => 0.03,
            Gauge::G65 => 0.022,
            Gauge::G120 => 0.014,
        }
    }
}

pub const STOCK_DUST: Dust = Dust { on: true, size: 1.758, softness: 0.245 };

impl Default for FilmSettings {
    fn default() -> Self {
        FilmSettings {
            negative: None,
            print: None,
            utility: None,
            halation: Halation::stock(Gauge::G65),
            grain: Gauge::G65.grain(),
            blur: Gauge::G65.blur(),
            dust: Some(STOCK_DUST),
        }
    }
}

impl FilmSettings {
    pub fn of(value: &Value) -> Option<FilmSettings> {
        serde_json::from_value(value.clone()).ok()
    }

    pub fn is_on(&self) -> bool {
        self.negative.is_some() && self.print.is_some()
    }

    // why: photos edited before FilmVision's grain and dust were identified stored a different grain shape
    // and no dust; FilmVision's own PowerGrade has its dust on
    pub fn normalized(&self) -> FilmSettings {
        let mut film = self.clone();
        film.dust = Some(self.dust.unwrap_or(STOCK_DUST));
        if self.grain.softness.is_nan() || self.blur.is_nan() {
            let gauge = GAUGES.into_iter().find(|g| (g.earlier_amount() - self.grain.amount).abs() < 1e-4).unwrap_or(Gauge::G65);
            film.grain = gauge.grain();
            film.blur = gauge.blur();
        }
        if !self.halation.identified() {
            film.halation = Halation::stock(film.grain_gauge().unwrap_or(Gauge::G65));
        }
        film
    }

    pub fn gauge(&self) -> Option<Gauge> {
        self.grain_gauge().filter(|g| (g.halation_radius() - self.halation.radius).abs() < 1e-4)
    }

    fn grain_gauge(&self) -> Option<Gauge> {
        let close = |a: f64, b: f64| (a - b).abs() < 1e-4;
        GAUGES.into_iter().find(|g| {
            let grain = g.grain();
            close(grain.amount, self.grain.amount)
                && close(grain.size, self.grain.size)
                && close(grain.softness, self.grain.softness)
                && close(grain.defocus, self.grain.defocus)
                && close(g.blur(), self.blur)
        })
    }

    pub fn luts(&self) -> Vec<String> {
        if !self.is_on() {
            return Vec::new();
        }
        [&self.negative, &self.print, &self.utility].into_iter().flatten().cloned().collect()
    }

    pub fn to_value(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }
}

pub fn render_film(film: Option<&FilmSettings>) -> Option<FilmSettings> {
    film.filter(|f| f.is_on()).map(FilmSettings::normalized)
}
