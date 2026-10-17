import { Slider, Toggle } from "dialkit";

import { Segmented } from "@/components/darkroom/develop/panel-controls";
import { useStore } from "@/hooks/use-store";

import { masksState, patchBrush, patchMasks } from "./masks-state";

const sizeScale = 400;

export function BrushSettings() {
  const brush = useStore(masksState, (state) => state.brush);
  return (
    <>
      <Segmented
        label="Brush"
        value={brush.erase ? "erase" : "paint"}
        onChange={(mode) => patchBrush({ erase: mode === "erase" })}
        options={[
          { value: "paint", label: "Paint" },
          { value: "erase", label: "Erase" },
        ]}
      />
      <Slider
        label="Size"
        value={Math.round(brush.size * sizeScale)}
        min={1}
        max={100}
        step={1}
        onChange={(value) => patchBrush({ size: value / sizeScale })}
      />
      <Slider
        label="Feather"
        value={brush.feather}
        min={0}
        max={100}
        step={1}
        onChange={(feather) => patchBrush({ feather })}
      />
      <Slider
        label="Flow"
        value={brush.flow}
        min={1}
        max={100}
        step={1}
        onChange={(flow) => patchBrush({ flow })}
      />
      <Toggle
        label="Auto mask"
        checked={brush.autoMask}
        onChange={(autoMask) => patchBrush({ autoMask })}
      />
      <Toggle
        label="Pen pressure"
        checked={brush.pressure}
        onChange={(pressure) => patchBrush({ pressure })}
      />
      <button
        type="button"
        onClick={() => patchMasks({ interaction: { mode: "adjust" } })}
        className="mx-xs h-8 rounded-m bg-control text-s font-medium text-secondary-text hover:bg-control-hover hover:text-foreground"
      >
        Done painting
      </button>
    </>
  );
}
