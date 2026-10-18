export type Row =
  | { kind: "slider"; id: string; label: string }
  | { kind: "toggle"; path: readonly [string, string]; label: string }
  | { kind: "treatment" }
  | { kind: "whiteBalance" }
  | { kind: "mixer" }
  | { kind: "grading" }
  | { kind: "vignetteStyle" }
  | { kind: "filmStock" }
  | { kind: "halation" }
  | { kind: "grain" }
  | { kind: "dust" }
  | { kind: "group"; title: string; rows: Row[] };

export interface Section {
  id: string;
  title: string;
  open: boolean;
  rows: Row[];
}

const slider = (id: string, label: string): Row => ({ kind: "slider", id, label });

export const sections: Section[] = [
  { id: "film", title: "Film", open: true, rows: [{ kind: "filmStock" }] },
  { id: "halation", title: "Halation", open: false, rows: [{ kind: "halation" }] },
  { id: "grain", title: "Grain", open: false, rows: [{ kind: "grain" }] },
  { id: "dust", title: "Dust", open: false, rows: [{ kind: "dust" }] },
  {
    id: "light",
    title: "Light",
    open: true,
    rows: [
      slider("light.exposure", "Exposure"),
      slider("light.contrast", "Contrast"),
      slider("light.highlights", "Highlights"),
      slider("light.shadows", "Shadows"),
      slider("light.whites", "Whites"),
      slider("light.blacks", "Blacks"),
    ],
  },
  {
    id: "color",
    title: "Color",
    open: true,
    rows: [
      { kind: "treatment" },
      { kind: "whiteBalance" },
      slider("color.vibrance", "Vibrance"),
      slider("color.saturation", "Saturation"),
    ],
  },
  {
    id: "curve",
    title: "Tone curve",
    open: false,
    rows: [
      slider("curve.highlights", "Highlights"),
      slider("curve.lights", "Lights"),
      slider("curve.darks", "Darks"),
      slider("curve.shadows", "Shadows"),
      {
        kind: "group",
        title: "Region splits",
        rows: [
          slider("curve.splitShadows", "Shadows split"),
          slider("curve.splitMid", "Midtones split"),
          slider("curve.splitHighlights", "Highlights split"),
        ],
      },
      slider("curve.refineSaturation", "Refine saturation"),
    ],
  },
  { id: "mixer", title: "Color mixer", open: false, rows: [{ kind: "mixer" }] },
  { id: "grading", title: "Color grading", open: false, rows: [{ kind: "grading" }] },
  {
    id: "effects",
    title: "Effects",
    open: false,
    rows: [
      slider("effects.texture", "Texture"),
      slider("effects.clarity", "Clarity"),
      slider("effects.dehaze", "Dehaze"),
      {
        kind: "group",
        title: "Vignette",
        rows: [
          { kind: "vignetteStyle" },
          slider("vignette.amount", "Amount"),
          slider("vignette.midpoint", "Midpoint"),
          slider("vignette.roundness", "Roundness"),
          slider("vignette.feather", "Feather"),
          slider("vignette.highlights", "Highlights"),
        ],
      },
    ],
  },
  {
    id: "detail",
    title: "Detail",
    open: false,
    rows: [
      slider("detail.sharpenAmount", "Sharpening"),
      slider("detail.sharpenMasking", "Masking"),
      slider("detail.nrLuminance", "Noise reduction"),
      slider("detail.nrDetail", "Detail"),
      slider("detail.nrColor", "Color noise"),
      slider("detail.nrColorDetail", "Color detail"),
      slider("detail.nrColorSmoothness", "Smoothness"),
    ],
  },
  {
    id: "optics",
    title: "Optics",
    open: false,
    rows: [
      { kind: "toggle", path: ["optics", "remove_ca"], label: "Remove chromatic aberration" },
      { kind: "toggle", path: ["optics", "lens_profile"], label: "Lens corrections" },
      slider("optics.distortion", "Distortion"),
      slider("optics.vignetting", "Vignetting"),
      slider("optics.vignettingMidpoint", "Vignetting midpoint"),
      {
        kind: "group",
        title: "Defringe",
        rows: [
          slider("optics.defringePurple", "Purple amount"),
          slider("optics.defringePurpleHueLo", "Purple hue low"),
          slider("optics.defringePurpleHueHi", "Purple hue high"),
          slider("optics.defringeGreen", "Green amount"),
          slider("optics.defringeGreenHueLo", "Green hue low"),
          slider("optics.defringeGreenHueHi", "Green hue high"),
        ],
      },
    ],
  },
  {
    id: "calibration",
    title: "Calibration",
    open: false,
    rows: [
      slider("calibration.shadowsTint", "Shadows tint"),
      slider("calibration.redHue", "Red hue"),
      slider("calibration.redSat", "Red saturation"),
      slider("calibration.greenHue", "Green hue"),
      slider("calibration.greenSat", "Green saturation"),
      slider("calibration.blueHue", "Blue hue"),
      slider("calibration.blueSat", "Blue saturation"),
    ],
  },
];

export const mixerBands = [
  "red",
  "orange",
  "yellow",
  "green",
  "aqua",
  "blue",
  "purple",
  "magenta",
] as const;

export const gradingRegions = ["shadows", "midtones", "highlights", "global"] as const;

export const lookGroups = [
  "film",
  "profile",
  "treatment",
  "toneCurve",
  "color",
  "colorMixer",
  "colorGrading",
  "effects",
  "vignette",
  "calibration",
] as const;

export const perPhotoGroups = [
  "whiteBalance",
  "light",
  "detail",
  "optics",
  "geometry",
  "crop",
  "masks",
  "spots",
] as const;
