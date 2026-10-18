pub enum Row {
    Slider(&'static str, &'static str),
    Toggle(&'static str, &'static str, &'static str),
    Treatment,
    WhiteBalance,
    Mixer,
    Grading,
    VignetteStyle,
    FilmStock,
    Halation,
    Grain,
    Dust,
    Group(&'static str, &'static [Row]),
}

pub struct Section {
    pub id: &'static str,
    pub title: &'static str,
    pub open: bool,
    pub rows: &'static [Row],
}

use Row::*;

pub const SECTIONS: &[Section] = &[
    Section { id: "film", title: "Film", open: true, rows: &[FilmStock] },
    Section { id: "halation", title: "Halation", open: false, rows: &[Halation] },
    Section { id: "grain", title: "Grain", open: false, rows: &[Grain] },
    Section { id: "dust", title: "Dust", open: false, rows: &[Dust] },
    Section {
        id: "light",
        title: "Light",
        open: true,
        rows: &[
            Slider("light.exposure", "Exposure"),
            Slider("light.contrast", "Contrast"),
            Slider("light.highlights", "Highlights"),
            Slider("light.shadows", "Shadows"),
            Slider("light.whites", "Whites"),
            Slider("light.blacks", "Blacks"),
        ],
    },
    Section { id: "color", title: "Color", open: true, rows: &[Treatment, WhiteBalance, Slider("color.vibrance", "Vibrance"), Slider("color.saturation", "Saturation")] },
    Section {
        id: "curve",
        title: "Tone curve",
        open: false,
        rows: &[
            Slider("curve.highlights", "Highlights"),
            Slider("curve.lights", "Lights"),
            Slider("curve.darks", "Darks"),
            Slider("curve.shadows", "Shadows"),
            Group("Region splits", &[Slider("curve.splitShadows", "Shadows split"), Slider("curve.splitMid", "Midtones split"), Slider("curve.splitHighlights", "Highlights split")]),
            Slider("curve.refineSaturation", "Refine saturation"),
        ],
    },
    Section { id: "mixer", title: "Color mixer", open: false, rows: &[Mixer] },
    Section { id: "grading", title: "Color grading", open: false, rows: &[Grading] },
    Section {
        id: "effects",
        title: "Effects",
        open: false,
        rows: &[
            Slider("effects.texture", "Texture"),
            Slider("effects.clarity", "Clarity"),
            Slider("effects.dehaze", "Dehaze"),
            Group(
                "Vignette",
                &[
                    VignetteStyle,
                    Slider("vignette.amount", "Amount"),
                    Slider("vignette.midpoint", "Midpoint"),
                    Slider("vignette.roundness", "Roundness"),
                    Slider("vignette.feather", "Feather"),
                    Slider("vignette.highlights", "Highlights"),
                ],
            ),
        ],
    },
    Section {
        id: "detail",
        title: "Detail",
        open: false,
        rows: &[
            Slider("detail.sharpenAmount", "Sharpening"),
            Slider("detail.sharpenMasking", "Masking"),
            Slider("detail.nrLuminance", "Noise reduction"),
            Slider("detail.nrDetail", "Detail"),
            Slider("detail.nrColor", "Color noise"),
            Slider("detail.nrColorDetail", "Color detail"),
            Slider("detail.nrColorSmoothness", "Smoothness"),
        ],
    },
    Section {
        id: "optics",
        title: "Optics",
        open: false,
        rows: &[
            Toggle("optics", "remove_ca", "Remove chromatic aberration"),
            Toggle("optics", "lens_profile", "Lens corrections"),
            Slider("optics.distortion", "Distortion"),
            Slider("optics.vignetting", "Vignetting"),
            Slider("optics.vignettingMidpoint", "Vignetting midpoint"),
            Group(
                "Defringe",
                &[
                    Slider("optics.defringePurple", "Purple amount"),
                    Slider("optics.defringePurpleHueLo", "Purple hue low"),
                    Slider("optics.defringePurpleHueHi", "Purple hue high"),
                    Slider("optics.defringeGreen", "Green amount"),
                    Slider("optics.defringeGreenHueLo", "Green hue low"),
                    Slider("optics.defringeGreenHueHi", "Green hue high"),
                ],
            ),
        ],
    },
    Section {
        id: "calibration",
        title: "Calibration",
        open: false,
        rows: &[
            Slider("calibration.shadowsTint", "Shadows tint"),
            Slider("calibration.redHue", "Red hue"),
            Slider("calibration.redSat", "Red saturation"),
            Slider("calibration.greenHue", "Green hue"),
            Slider("calibration.greenSat", "Green saturation"),
            Slider("calibration.blueHue", "Blue hue"),
            Slider("calibration.blueSat", "Blue saturation"),
        ],
    },
];

pub const MIXER_BANDS: [&str; 8] = ["red", "orange", "yellow", "green", "aqua", "blue", "purple", "magenta"];
pub const GRADING_REGIONS: [&str; 4] = ["shadows", "midtones", "highlights", "global"];

pub fn sentence(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
}
