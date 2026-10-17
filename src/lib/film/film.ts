import type { FilmSettings } from "@/lib/engine/types";

export const gauges = {
  "16": { amount: 0.045, size: 2.5 },
  "35": { amount: 0.03, size: 1.8 },
  "65": { amount: 0.022, size: 1.2 },
  "120": { amount: 0.014, size: 0.7 },
} as const;

export type Gauge = keyof typeof gauges;

export const noFilm: FilmSettings = {
  negative: null,
  print: null,
  utility: null,
  halation: { amount: 0.3, radius: 5, threshold: 0.55 },
  grain: { amount: gauges["65"].amount, size: gauges["65"].size, color: 0 },
};

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
  return filmIsOn(film) ? film : undefined;
}
