import { SelectControl, Slider } from "dialkit";

import { Button } from "@/components/ui/button";
import { useStore } from "@/hooks/use-store";
import { develop, editFilm } from "@/lib/develop/session";
import type { FilmSettings } from "@/lib/engine/types";
import { filmIsOn, gaugeNames, gauges, noFilm, normalizeFilm, type Gauge } from "@/lib/film/film";
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
  onChange: (value: number) => void;
}

export function PlainSlider({
  label,
  value,
  min,
  max,
  step,
  fallback,
  onChange,
}: PlainSliderProps) {
  return (
    <div onDoubleClick={() => onChange(fallback)}>
      <Slider
        label={label}
        value={Math.min(max, Math.max(min, value))}
        min={min}
        max={max}
        step={step}
        onChange={onChange}
      />
    </div>
  );
}

export function useFilm(): FilmSettings {
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

export function NeedsStock() {
  return (
    <p className="px-xxs py-xs text-xs text-pretty text-tertiary-text">
      Choose a negative and a print to add halation, grain and dust.
    </p>
  );
}

export function HalationRows() {
  const film = useFilm();
  if (!filmIsOn(film)) {
    return <NeedsStock />;
  }
  const set = (patch: Partial<FilmSettings["halation"]>, label: string) =>
    editFilm(
      { ...film, halation: { ...film.halation, ...patch } },
      label,
      `film.halation.${label}`,
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
        onChange={(v) => set({ amount: v / 100 }, `Halation ${v}`)}
      />
      <PlainSlider
        label="Radius"
        value={radius}
        min={1}
        max={20}
        step={0.1}
        fallback={noFilm.halation.radius}
        onChange={(v) => set({ radius: v }, `Halation radius ${v.toFixed(1)}`)}
      />
      <PlainSlider
        label="Threshold"
        value={threshold}
        min={0.2}
        max={2}
        step={0.01}
        fallback={noFilm.halation.threshold}
        onChange={(v) => set({ threshold: v }, `Halation threshold ${v.toFixed(2)}`)}
      />
    </>
  );
}

function close(a: number, b: number): boolean {
  return Math.abs(a - b) < 1e-4;
}

function gaugeOf(film: FilmSettings): Gauge | "custom" {
  const match = gaugeNames.find((name) => {
    const { grain, blur } = gauges[name];
    return (
      close(grain.amount, film.grain.amount) &&
      close(grain.size, film.grain.size) &&
      close(grain.softness, film.grain.softness) &&
      close(grain.defocus, film.grain.defocus) &&
      close(blur, film.blur)
    );
  });
  return match ?? "custom";
}

export function GrainRows() {
  const film = normalizeFilm(useFilm());
  if (!filmIsOn(film)) {
    return <NeedsStock />;
  }
  const set = (patch: Partial<FilmSettings["grain"]>, label: string, key: string) =>
    editFilm({ ...film, grain: { ...film.grain, ...patch } }, label, `film.grain.${key}`);
  const fallback = noFilm.grain;
  const { amount, size, softness, defocus } = film.grain;
  return (
    <>
      <Segmented
        label="Gauge"
        value={gaugeOf(film)}
        onChange={(gauge) => {
          if (gauge !== "custom") {
            const { grain, blur } = gauges[gauge];
            editFilm({ ...film, grain: { ...grain }, blur }, `Gauge ${gauge} mm`, "film.gauge");
          }
        }}
        options={gaugeNames.map((name) => ({ value: name, label: name }))}
      />
      <PlainSlider
        label="Amount"
        value={Math.round(amount * 1000) / 10}
        min={0}
        max={100}
        step={0.5}
        fallback={fallback.amount * 100}
        onChange={(v) => set({ amount: v / 100 }, `Grain ${v}`, "amount")}
      />
      <PlainSlider
        label="Size"
        value={size}
        min={0}
        max={4}
        step={0.1}
        fallback={fallback.size}
        onChange={(v) => set({ size: v }, `Grain size ${v.toFixed(1)}`, "size")}
      />
      <PlainSlider
        label="Softness"
        value={Math.round(softness * 100)}
        min={0}
        max={100}
        step={1}
        fallback={fallback.softness * 100}
        onChange={(v) => set({ softness: v / 100 }, `Grain softness ${v}`, "softness")}
      />
      <PlainSlider
        label="Defocus"
        value={Math.round((1 - defocus) * 100)}
        min={0}
        max={50}
        step={1}
        fallback={(1 - fallback.defocus) * 100}
        onChange={(v) => set({ defocus: 1 - v / 100 }, `Image defocus ${v}`, "defocus")}
      />
      <PlainSlider
        label="Film blur"
        value={Math.round(film.blur * 1000) / 10}
        min={0}
        max={20}
        step={0.5}
        fallback={noFilm.blur * 100}
        onChange={(v) => editFilm({ ...film, blur: v / 100 }, `Film blur ${v}`, "film.blur")}
      />
    </>
  );
}
