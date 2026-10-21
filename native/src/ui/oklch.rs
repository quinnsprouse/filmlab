use gpui_kit::Rgba;

#[derive(Clone, Copy)]
pub struct Reach {
    pub share_of_gamut: f64,
    pub cap: f64,
}

impl Reach {
    pub fn color(self, lightness: f64, strength: f64, hue: f64) -> Rgba {
        let chroma = self.cap.min(self.share_of_gamut * max_chroma(lightness, hue)) * strength;
        srgb(lightness, chroma, hue)
    }
}

pub fn gray(lightness: f64) -> Rgba {
    srgb(lightness, 0.0, 0.0)
}

pub fn hue_of_hsv(degrees: f64) -> f64 {
    let sector = degrees.rem_euclid(360.0) / 60.0;
    let rising = 1.0 - (sector % 2.0 - 1.0).abs();
    let rgb = match sector as u32 {
        0 => [1.0, rising, 0.0],
        1 => [rising, 1.0, 0.0],
        2 => [0.0, 1.0, rising],
        3 => [0.0, rising, 1.0],
        4 => [rising, 0.0, 1.0],
        _ => [1.0, 0.0, rising],
    };
    hue_of_srgb(rgb)
}

pub fn hue_of_srgb(encoded: [f64; 3]) -> f64 {
    let [r, g, b] = encoded.map(decode);
    let l = (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b).cbrt();
    let m = (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b).cbrt();
    let s = (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b).cbrt();
    let a = 1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s;
    let b = 0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s;
    b.atan2(a).to_degrees().rem_euclid(360.0)
}

fn linear(lightness: f64, chroma: f64, hue: f64) -> [f64; 3] {
    let (sin, cos) = hue.to_radians().sin_cos();
    let (a, b) = (chroma * cos, chroma * sin);
    let l = (lightness + 0.3963377774 * a + 0.2158037573 * b).powi(3);
    let m = (lightness - 0.1055613458 * a - 0.0638541728 * b).powi(3);
    let s = (lightness - 0.0894841775 * a - 1.2914855480 * b).powi(3);
    [
        4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
        -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
        -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s,
    ]
}

fn max_chroma(lightness: f64, hue: f64) -> f64 {
    let in_gamut = |chroma| linear(lightness, chroma, hue).iter().all(|c| (-1e-4..=1.0 + 1e-4).contains(c));
    let (mut inside, mut outside) = (0.0, 0.4);
    for _ in 0..32 {
        let mid = (inside + outside) / 2.0;
        if in_gamut(mid) { inside = mid } else { outside = mid }
    }
    inside
}

fn srgb(lightness: f64, chroma: f64, hue: f64) -> Rgba {
    let [r, g, b] = linear(lightness, chroma, hue).map(encode);
    Rgba { r, g, b, a: 1.0 }
}

fn encode(linear: f64) -> f32 {
    let c = linear.clamp(0.0, 1.0);
    (if c <= 0.0031308 { 12.92 * c } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }) as f32
}

fn decode(encoded: f64) -> f64 {
    if encoded <= 0.04045 { encoded / 12.92 } else { ((encoded + 0.055) / 1.055).powf(2.4) }
}
