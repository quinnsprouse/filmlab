import { Slider, Toggle } from "dialkit";

import { Segmented, PanelRoot } from "@/components/darkroom/develop/panel-controls";
import { Icon, Cancel01Icon } from "@/components/icons";
import { Button } from "@/components/ui/button";
import { useStore } from "@/hooks/use-store";
import { develop, setOverlay, setTool } from "@/lib/develop/session";
import { cn } from "@/lib/utils";

import { brush, pickSpot, selectSpot, setBrush, useSelectedSpot } from "./heal-state";
import {
  clearSpots,
  commandMessage,
  deleteSpot,
  findDust,
  modeLabel,
  refreshSource,
  sizeRange,
  spotModes,
  spotsOf,
  updateSpot,
  type Spot,
  type SpotMode,
} from "./spots";

function guarded(action: () => void): void {
  try {
    action();
    setBrush({ message: undefined });
  } catch (error) {
    setBrush({ message: commandMessage(error) });
  }
}

function BrushControls({ selected }: { selected: number | undefined }) {
  const shown = useStore(brush, (state) => state);

  const change = (key: "size" | "feather" | "opacity", value: number) => {
    setBrush({ [key]: value });
    if (selected !== undefined) {
      guarded(() => updateSpot(selected, { [key]: value }, `Spot ${key}`, key));
    }
  };

  return (
    <>
      <Segmented<SpotMode>
        label="Mode"
        value={shown.mode}
        options={spotModes}
        onChange={(mode) => {
          setBrush({ mode });
        }}
      />
      <Slider
        label="Size"
        value={shown.size * 100}
        min={sizeRange.min * 100}
        max={sizeRange.max * 100}
        step={0.1}
        unit="%"
        onChange={(value) => change("size", value / 100)}
      />
      <Slider
        label="Feather"
        value={shown.feather}
        min={0}
        max={100}
        step={1}
        onChange={(value) => change("feather", value)}
      />
      <Slider
        label="Opacity"
        value={shown.opacity}
        min={0}
        max={100}
        step={1}
        onChange={(value) => change("opacity", value)}
      />
    </>
  );
}

const quietButton =
  "rounded-s px-s py-xs text-s font-medium text-secondary-text ring ring-border transition-colors duration-(--motion-duration) ease-(--motion-easing) hover:bg-control-hover hover:text-foreground disabled:opacity-40";

function SpotRow({ spot, index, active }: { spot: Spot; index: number; active: boolean }) {
  return (
    <li
      className={cn(
        "flex items-center rounded-s transition-colors duration-(--motion-duration) ease-(--motion-easing)",
        active ? "bg-control-active" : "hover:bg-control",
      )}
    >
      <button
        type="button"
        aria-pressed={active}
        onClick={() => (active ? selectSpot(undefined) : pickSpot(index, spot))}
        className="flex min-w-0 grow items-center gap-s px-s py-xs text-left text-s text-foreground"
      >
        <span className="w-4 font-data text-xs text-tertiary-text tabular-nums">{index + 1}</span>
        {modeLabel(spot.mode)}
      </button>
      <button
        type="button"
        aria-label={`Delete spot ${index + 1}`}
        onClick={() => {
          guarded(() => deleteSpot(index));
          selectSpot(undefined);
        }}
        className="grid size-7 place-items-center text-tertiary-text hover:text-foreground"
      >
        <Icon icon={Cancel01Icon} size={14} />
      </button>
    </li>
  );
}

function SpotList({ selected }: { selected: number | undefined }) {
  const settings = useStore(develop, (state) => state.settings);
  const rows = spotsOf(settings).map((spot, position) => ({ spot, position }));
  const count = rows.length;
  return (
    <section aria-label="Spots" className="flex flex-col gap-xs">
      <div className="flex items-center justify-between px-xs">
        <h3 className="text-s font-medium text-secondary-text">
          Spots <span className="font-data text-xs text-tertiary-text tabular-nums">{count}</span>
        </h3>
        <button
          type="button"
          disabled={count === 0}
          onClick={() => {
            guarded(() => clearSpots(count));
            selectSpot(undefined);
          }}
          className="text-s text-secondary-text hover:text-foreground disabled:opacity-40"
        >
          Clear all
        </button>
      </div>
      {count === 0 ? (
        <p className="px-xs text-s text-tertiary-text">
          Click the photo to add a spot, or drag to paint one.
        </p>
      ) : (
        <ul className="flex max-h-48 flex-col gap-xxs overflow-y-auto">
          {rows.map((row) => (
            <SpotRow
              key={row.position}
              index={row.position}
              spot={row.spot}
              active={row.position === selected}
            />
          ))}
        </ul>
      )}
    </section>
  );
}

function VisualizeSpots() {
  const overlay = useStore(develop, (state) => state.overlay);
  const threshold = overlay?.kind === "spots" ? overlay.threshold : undefined;
  return (
    <>
      <Toggle
        label="Visualize spots"
        checked={threshold !== undefined}
        onChange={(on) => setOverlay(on ? { kind: "spots", threshold: 50 } : undefined)}
      />
      {threshold !== undefined ? (
        <Slider
          label="Sensitivity"
          value={threshold}
          min={0}
          max={100}
          step={1}
          onChange={(value) => setOverlay({ kind: "spots", threshold: value })}
        />
      ) : null}
    </>
  );
}

export function HealPanel() {
  const settings = useStore(develop, (state) => state.settings);
  const message = useStore(brush, (state) => state.message);
  const spots = spotsOf(settings);
  const selected = useSelectedSpot(spots.length);
  const sourced = selected !== undefined && spots[selected]?.mode !== "fill";

  return (
    <div className="flex flex-col gap-m">
      <PanelRoot title="Heal">
        <BrushControls selected={selected} />
        <VisualizeSpots />
      </PanelRoot>
      <div className="flex gap-xs px-xs">
        <button
          type="button"
          disabled={selected === undefined || !sourced}
          onClick={() => selected !== undefined && guarded(() => refreshSource(selected))}
          title="Pick a new source (/)"
          className={cn(quietButton, "grow")}
        >
          Refresh source
        </button>
        <button
          type="button"
          onClick={() => {
            guarded(() => {
              const added = findDust();
              setBrush({ message: added === 0 ? "No dust spots found." : undefined });
            });
          }}
          className={cn(quietButton, "grow")}
        >
          Find dust
        </button>
      </div>
      {message ? <output className="px-xs text-s text-secondary-text">{message}</output> : null}
      <div className="px-xs">
        <SpotList selected={selected} />
      </div>
      <div className="px-xs">
        <Button className="w-full" onClick={() => setTool("edit")}>
          Done
        </Button>
      </div>
    </div>
  );
}
