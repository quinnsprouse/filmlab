import { Slider, Toggle } from "dialkit";
import { useRef, type PointerEvent } from "react";

import { useStore } from "@/hooks/use-store";

import { swatchColor } from "./color-sample";
import { updateShape } from "./masks-commands";
import type { Shape } from "./masks-model";
import { masksState, patchMasks } from "./masks-state";

type Luminance = Extract<Shape, { kind: "luminanceRange" }>;
type ColorRange = Extract<Shape, { kind: "colorRange" }>;

const minGap = 0.01;
const maxFeather = 0.5;

function valueAt(event: PointerEvent<HTMLElement>): number {
  const box = event.currentTarget.getBoundingClientRect();
  return Math.min(1, Math.max(0, (event.clientX - box.left) / box.width));
}

function RangeBar({ maskId, index, shape }: { maskId: number; index: number; shape: Luminance }) {
  const dragging = useRef<"lo" | "hi" | undefined>(undefined);
  const move = (event: PointerEvent<HTMLElement>) => {
    const value = valueAt(event);
    const handle = dragging.current;
    if (!handle) {
      return;
    }
    const next =
      handle === "lo"
        ? { ...shape, lo: Math.min(value, shape.hi - minGap) }
        : { ...shape, hi: Math.max(value, shape.lo + minGap) };
    updateShape(maskId, index, next, "Luminance range");
  };
  return (
    <fieldset
      className="relative m-0 h-6 touch-none rounded-m border-0 p-0"
      onPointerDown={(event) => {
        const value = valueAt(event);
        dragging.current = Math.abs(value - shape.lo) <= Math.abs(value - shape.hi) ? "lo" : "hi";
        event.currentTarget.setPointerCapture(event.pointerId);
        patchMasks({ engaged: true });
        move(event);
      }}
      onPointerMove={move}
      onPointerUp={() => {
        dragging.current = undefined;
        patchMasks({ engaged: false });
      }}
      onPointerCancel={() => {
        dragging.current = undefined;
        patchMasks({ engaged: false });
      }}
    >
      <legend className="sr-only">Luminance range</legend>
      <div className="absolute inset-x-0 top-1 bottom-1 rounded-s bg-linear-to-r from-canvas to-foreground ring ring-border" />
      <div
        className="absolute top-0.5 bottom-0.5 left-(--lo) w-(--span) rounded-xs ring ring-foreground"
        style={{ "--lo": `${shape.lo * 100}%`, "--span": `${(shape.hi - shape.lo) * 100}%` }}
      />
      {(["lo", "hi"] as const).map((handle) => (
        <span
          key={handle}
          className="absolute top-1/2 left-(--at) size-3 -translate-1/2 rounded-full bg-foreground ring ring-canvas/50"
          style={{ "--at": `${shape[handle] * 100}%` }}
        />
      ))}
    </fieldset>
  );
}

function LuminanceControls({
  maskId,
  index,
  shape,
}: {
  maskId: number;
  index: number;
  shape: Luminance;
}) {
  const lumaMap = useStore(masksState, (state) => state.lumaMap);
  const smooth = Math.round(((shape.loFeather + shape.hiFeather) / 2 / maxFeather) * 100);
  return (
    <div className="flex flex-col gap-xs">
      <RangeBar maskId={maskId} index={index} shape={shape} />
      <div
        onDoubleClick={() =>
          updateShape(maskId, index, { ...shape, loFeather: 0.1, hiFeather: 0.1 })
        }
      >
        <Slider
          label="Smoothness"
          value={smooth}
          min={0}
          max={100}
          step={1}
          onChange={(value) => {
            const feather = (value / 100) * maxFeather;
            updateShape(maskId, index, { ...shape, loFeather: feather, hiFeather: feather });
          }}
        />
      </div>
      <Toggle
        label="Show luminance map"
        checked={lumaMap}
        onChange={(checked) => patchMasks({ lumaMap: checked })}
      />
    </div>
  );
}

function Swatches({ shape }: { shape: ColorRange }) {
  if (shape.samples.length === 0) {
    return <p className="text-s text-tertiary-text">No colors picked yet.</p>;
  }
  return (
    <ul className="flex gap-xs" aria-label="Picked colors">
      {shape.samples.map((sample) => (
        <li
          key={sample.join(",")}
          className="size-6 rounded-s bg-(--swatch) ring ring-border-strong"
          style={{ "--swatch": swatchColor(sample) }}
        />
      ))}
    </ul>
  );
}

function ColorControls({
  maskId,
  index,
  shape,
}: {
  maskId: number;
  index: number;
  shape: ColorRange;
}) {
  const picking = useStore(masksState, (state) => state.interaction.mode === "pick");
  return (
    <div className="flex flex-col gap-xs">
      <div className="flex items-center justify-between gap-s">
        <Swatches shape={shape} />
        <button
          type="button"
          aria-pressed={picking}
          onClick={() =>
            patchMasks({ interaction: picking ? { mode: "adjust" } : { mode: "pick" } })
          }
          className="h-6 shrink-0 rounded-s bg-control px-s text-s font-medium text-secondary-text hover:bg-control-hover hover:text-foreground aria-pressed:bg-control-active aria-pressed:text-foreground"
        >
          {picking ? "Done picking" : "Pick color"}
        </button>
      </div>
      <p className="text-xs text-tertiary-text">
        Click the photo to pick. Shift-click adds up to 5.
      </p>
      <div onDoubleClick={() => updateShape(maskId, index, { ...shape, refine: 50 })}>
        <Slider
          label="Refine"
          value={shape.refine}
          min={0}
          max={100}
          step={1}
          onChange={(value) => updateShape(maskId, index, { ...shape, refine: value })}
        />
      </div>
    </div>
  );
}

interface RangeControlsProps {
  maskId: number;
  index: number;
  shape: Shape;
}

export function RangeControls({ maskId, index, shape }: RangeControlsProps) {
  if (shape.kind === "luminanceRange") {
    return <LuminanceControls maskId={maskId} index={index} shape={shape} />;
  }
  if (shape.kind === "colorRange") {
    return <ColorControls maskId={maskId} index={index} shape={shape} />;
  }
  if (shape.kind === "subject" || shape.kind === "sky" || shape.kind === "background") {
    return (
      <p className="px-xs text-xs text-tertiary-text">
        Uses the file's embedded mattes or a quick estimate.
      </p>
    );
  }
  return null;
}
