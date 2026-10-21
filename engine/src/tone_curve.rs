use lightcraft_color::spline::{Lut1, MonotoneCurve};
use lightcraft_engine::develop::ToneCurve;
use lightcraft_engine::pipeline::finish::{point_lut, region_lut};

use crate::core::Fallible;

pub struct Curve(Lut1);

impl Curve {
    pub fn at(&self, x: f64) -> f64 {
        f64::from(self.0.eval(x as f32))
    }

    pub fn after(&self, inner: &Curve) -> Curve {
        Curve(self.0.compose(&inner.0))
    }

    pub fn is_identity(&self) -> bool {
        self.0.is_identity()
    }
}

pub struct Curves {
    pub regions: Curve,
    pub master: Curve,
    pub channels: [Curve; 3],
}

impl Curves {
    pub fn of(curve: &str) -> Fallible<Curves> {
        let c: ToneCurve = serde_json::from_str::<Option<ToneCurve>>(curve).map_err(|e| e.to_string())?.unwrap_or_default();
        Ok(Curves { regions: Curve(region_lut(&c)), master: Curve(point_lut(&c.master)), channels: [&c.red, &c.green, &c.blue].map(|p| Curve(point_lut(p))) })
    }
}

pub fn on_curve(points: &[[f64; 2]], x: f64) -> f64 {
    MonotoneCurve::new(&points.iter().map(|p| (p[0], p[1])).collect::<Vec<_>>()).eval(x)
}
