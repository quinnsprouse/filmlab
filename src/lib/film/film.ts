import type { FilmSettings } from "@/lib/engine/types";

// why: FilmVision Pro's own gauge PowerGrades (grain amount, size, softness, image defocus, film blur);
// 120 continues their scale
export const gauges = {
  "16": { grain: { amount: 0.25, size: 2.5, softness: 0.25, defocus: 0.85 }, blur: 0.05 },
  "35": { grain: { amount: 0.1495, size: 2, softness: 0.05, defocus: 0.9 }, blur: 0.05 },
  "65": { grain: { amount: 0.0995, size: 1, softness: 0.05, defocus: 1 }, blur: 0.05 },
  "120": { grain: { amount: 0.065, size: 0.6, softness: 0.02, defocus: 1 }, blur: 0.03 },
} as const;

export type Gauge = keyof typeof gauges;

export const gaugeNames: Gauge[] = ["16", "35", "65", "120"];

export const noFilm: FilmSettings = {
  negative: null,
  print: null,
  utility: null,
  halation: { amount: 0.3, radius: 5, threshold: 0.55 },
  grain: { ...gauges["65"].grain },
  blur: gauges["65"].blur,
  dust: { on: true, size: 1.758, softness: 0.245 },
};

const earlierAmounts: Record<Gauge, number> = {
  "16": 0.045,
  "35": 0.03,
  "65": 0.022,
  "120": 0.014,
};

// why: photos edited before FilmVision's grain and dust were identified stored a different grain
// shape and no dust; FilmVision's own PowerGrade has its dust on
export function normalizeFilm(film: FilmSettings): FilmSettings {
  const dust = typeof film.dust === "object" ? film.dust : { ...noFilm.dust };
  if (typeof film.grain.softness === "number" && typeof film.blur === "number") {
    return dust === film.dust ? film : { ...film, dust };
  }
  const gauge =
    gaugeNames.find((name) => Math.abs(earlierAmounts[name] - film.grain.amount) < 1e-4) ?? "65";
  return { ...film, grain: { ...gauges[gauge].grain }, blur: gauges[gauge].blur, dust };
}

export function filmIsOn(film: FilmSettings | undefined): film is FilmSettings {
  return Boolean(film?.negative && film.print);
}

export function lutsOf(film: FilmSettings | undefined): string[] {
  if (!filmIsOn(film)) {
    return [];
  }
  return [film.negative, film.print, film.utility].filter((id): id is string => Boolean(id));
}

export function renderFilm(film: FilmSettings | undefined): FilmSettings | undefined {
  return filmIsOn(film) ? normalizeFilm(film) : undefined;
}
