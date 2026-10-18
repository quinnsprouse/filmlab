import { Toggle } from "dialkit";

import { editFilm } from "@/lib/develop/session";
import type { FilmSettings } from "@/lib/engine/types";
import { filmIsOn, noFilm, normalizeFilm } from "@/lib/film/film";

import { NeedsStock, PlainSlider, useFilm } from "./film-rows";

export function DustRows() {
  const film = normalizeFilm(useFilm());
  if (!filmIsOn(film)) {
    return <NeedsStock />;
  }
  const set = (patch: Partial<FilmSettings["dust"]>, label: string, key: string) =>
    editFilm({ ...film, dust: { ...film.dust, ...patch } }, label, `film.dust.${key}`);
  const { on, size, softness } = film.dust;
  return (
    <>
      <Toggle
        label="Dust specks"
        checked={on}
        onChange={(checked) => set({ on: checked }, `Dust ${checked ? "on" : "off"}`, "on")}
      />
      <PlainSlider
        label="Size"
        value={size}
        min={1.2}
        max={4}
        step={0.01}
        fallback={noFilm.dust.size}
        onChange={(v) => set({ size: v }, `Dust size ${v.toFixed(2)}`, "size")}
      />
      <PlainSlider
        label="Softness"
        value={Math.round(softness * 1000) / 10}
        min={0}
        max={50}
        step={0.5}
        fallback={noFilm.dust.softness * 100}
        onChange={(v) => set({ softness: v / 100 }, `Dust softness ${v}`, "softness")}
      />
    </>
  );
}
