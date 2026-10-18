import { Slider, Toggle } from "dialkit";

import { updateShape } from "./masks-commands";
import type { Mask } from "./masks-model";

export function RadialControls({ mask }: { mask: Mask }) {
  const found = mask.components.findIndex(({ shape }) => shape.kind === "radial");
  const shape = mask.components[found]?.shape;
  if (shape?.kind !== "radial") {
    return null;
  }
  return (
    <>
      <div onDoubleClick={() => updateShape(mask.id, found, { ...shape, feather: 50 })}>
        <Slider
          label="Feather"
          value={shape.feather}
          min={0}
          max={100}
          step={1}
          onChange={(feather) => updateShape(mask.id, found, { ...shape, feather }, "Mask feather")}
        />
      </div>
      <Toggle
        label="Invert area"
        checked={shape.invert}
        onChange={(invert) => updateShape(mask.id, found, { ...shape, invert }, "Invert area")}
      />
    </>
  );
}
