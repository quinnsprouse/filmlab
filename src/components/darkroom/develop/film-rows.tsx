import { SelectControl, Slider } from "dialkit";

import { Button } from "@/components/ui/button";
import { useStore } from "@/hooks/use-store";
import { develop, editFilm } from "@/lib/develop/session";
import type { FilmSettings } from "@/lib/engine/types";
import { filmIsOn, gauges, noFilm, type Gauge } from "@/lib/film/film";
import { connectFilmFolder, stocks, type Stock, type StockKind } from "@/lib/film/stocks";

import { Segmented } from "./panel-controls";

const defaults: Record<StockKind, string> = {
  Negative: "Negative/Vision3 250D",
  Print: "Print/I- Standard/Print SA1 Kodak 2393",
  Utility: "",
};

interface PlainSliderProps {
  label: string;
  value: number;
  min: number;
  max: number;
  step: number;
  fallback: number;
  onChange: (value: number, draft: boolean) => void;
}

function PlainSlider({ label, value, min, max, step, fallback, onChange }: PlainSliderProps) {
  return (
    <div onDoubleClick={() => onChange(fallback, false)}>
      <Slider
        label={label}
        value={Math.min(max, Math.max(min, value))}
        min={min}
        max={max}
        step={step}
        onChange={(next) => onChange(next, true)}
      />
    </div>
  );
}

function useFilm(): FilmSettings {
  return useStore(develop, (state) => state.film) ?? noFilm;
}

function stockLabel(stock: Stock): string {
  return stock.group && stock.group !== "Standard" ? `${stock.name} · ${stock.group}` : stock.name;
}

function ConnectRow() {
  const status = useStore(stocks, (state) => state.status);
  const error = useStore(stocks, (state) => state.error);
  return (
    <div className="flex flex-col gap-xs px-xxs py-xs">
      <p className="text-xs text-pretty text-secondary-text">
        {status === "needsPermission"
          ? "Allow access to your FilmVision Pro folder again to use its film stocks."
          : (error ??
            "Film stocks come from your own FilmVision Pro LUTs. Choose the FilmVision Pro folder once; nothing is copied or uploaded.")}
      </p>
      <Button variant="secondary" onClick={() => void connectFilmFolder()}>
        {status === "needsPermission" ? "Allow access" : "Choose FilmVision folder"}
      </Button>
    </div>
  );
}

function pick(kind: StockKind, film: FilmSettings, id: string): FilmSettings {
  const available = new Set(stocks.get().stocks.map((stock) => stock.id));
  const fill = (current: string | null, fallbackKind: StockKind) =>
    current ?? (available.has(defaults[fallbackKind]) ? defaults[fallbackKind] : null);
  const value = id === "" ? null : id;
  switch (kind) {
    case "Negative":
      return { ...film, negative: value, print: value ? fill(film.print, "Print") : film.print };
    case "Print":
      return {
        ...film,
        print: value,
        negative: value ? fill(film.negative, "Negative") : film.negative,
      };
    case "Utility":
      return { ...film, utility: value };
  }
}

function StockSelect({ kind, label }: { kind: StockKind; label: string }) {
  const film = useFilm();
  const all = useStore(stocks, (state) => state.stocks);
  const current =
    kind === "Negative" ? film.negative : kind === "Print" ? film.print : film.utility;
  const options = [
    { value: "", label: "None" },
    ...all
      .filter((stock) => stock.kind === kind)
      .map((stock) => ({ value: stock.id, label: stockLabel(stock) })),
  ];
  return (
    <SelectControl
      label={label}
      value={current ?? ""}
      options={options}
      onChange={(id) => {
        const chosen = options.find((option) => option.value === id);
        editFilm(pick(kind, film, id), `${label} ${chosen?.label ?? "None"}`, `film.${kind}`);
      }}
    />
  );
}

export function FilmStockRows() {
  const status = useStore(stocks, (state) => state.status);
  if (status !== "ready") {
    return <ConnectRow />;
  }
  return (
    <>
      <StockSelect kind="Negative" label="Negative" />
      <StockSelect kind="Print" label="Print" />
      <StockSelect kind="Utility" label="Utility" />
    </>
  );
}

function NeedsStock() {
  return (
    <p className="px-xxs py-xs text-xs text-pretty text-tertiary-text">
      Choose a negative and a print to add halation and grain.
    </p>
  );
}

export function HalationRows() {
  const film = useFilm();
  if (!filmIsOn(film)) {
    return <NeedsStock />;
  }
  const set = (patch: Partial<FilmSettings["halation"]>, label: string, draft: boolean) =>
    editFilm(
      { ...film, halation: { ...film.halation, ...patch } },
      label,
      `film.halation.${label}`,
      draft,
    );
  const { amount, radius, threshold } = film.halation;
  return (
    <>
      <PlainSlider
        label="Amount"
        value={Math.round(amount * 100)}
        min={0}
        max={100}
        step={1}
        fallback={Math.round(noFilm.halation.amount * 100)}
        onChange={(v, draft) => set({ amount: v / 100 }, `Halation ${v}`, draft)}
      />
      <PlainSlider
        label="Radius"
        value={radius}
        min={1}
        max={20}
        step={0.1}
        fallback={noFilm.halation.radius}
        onChange={(v, draft) => set({ radius: v }, `Halation radius ${v.toFixed(1)}`, draft)}
      />
      <PlainSlider
        label="Threshold"
        value={threshold}
        min={0.2}
        max={2}
        step={0.01}
        fallback={noFilm.halation.threshold}
        onChange={(v, draft) => set({ threshold: v }, `Halation threshold ${v.toFixed(2)}`, draft)}
      />
    </>
  );
}

const grainScale = 0.06;

function gaugeOf(film: FilmSettings): Gauge | "custom" {
  const names: Gauge[] = ["16", "35", "65", "120"];
  const match = names.find(
    (gauge) =>
      Math.abs(gauges[gauge].amount - film.grain.amount) < 1e-4 &&
      Math.abs(gauges[gauge].size - film.grain.size) < 1e-4,
  );
  return match ?? "custom";
}

export function GrainRows() {
  const film = useFilm();
  if (!filmIsOn(film)) {
    return <NeedsStock />;
  }
  const set = (patch: Partial<FilmSettings["grain"]>, label: string, key: string, draft: boolean) =>
    editFilm({ ...film, grain: { ...film.grain, ...patch } }, label, `film.grain.${key}`, draft);
  const { amount, size, color } = film.grain;
  return (
    <>
      <Segmented
        label="Gauge"
        value={gaugeOf(film)}
        onChange={(gauge) => {
          if (gauge !== "custom") {
            set({ ...gauges[gauge] }, `Grain ${gauge} mm`, "gauge", false);
          }
        }}
        options={[
          { value: "16", label: "16" },
          { value: "35", label: "35" },
          { value: "65", label: "65" },
          { value: "120", label: "120" },
        ]}
      />
      <PlainSlider
        label="Amount"
        value={Math.round((amount / grainScale) * 100)}
        min={0}
        max={100}
        step={1}
        fallback={Math.round((noFilm.grain.amount / grainScale) * 100)}
        onChange={(v, draft) =>
          set({ amount: (v / 100) * grainScale }, `Grain ${v}`, "amount", draft)
        }
      />
      <PlainSlider
        label="Size"
        value={size}
        min={0.5}
        max={4}
        step={0.1}
        fallback={noFilm.grain.size}
        onChange={(v, draft) => set({ size: v }, `Grain size ${v.toFixed(1)}`, "size", draft)}
      />
      <PlainSlider
        label="Color"
        value={Math.round(color * 100)}
        min={0}
        max={100}
        step={1}
        fallback={0}
        onChange={(v, draft) => set({ color: v / 100 }, `Grain color ${v}`, "color", draft)}
      />
    </>
  );
}
