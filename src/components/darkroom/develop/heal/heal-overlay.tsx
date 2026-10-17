import { useRef, useState } from "react";

import { useStore } from "@/hooks/use-store";
import { develop } from "@/lib/develop/session";
import { cn } from "@/lib/utils";

import {
  cssToImage,
  distance,
  gripAt,
  gripsOf,
  radiusOf,
  shapesOf,
  type Grip,
  type Mapping,
} from "./geometry";
import { brush, pickSpot, selectSpot, setBrush, useSelectedSpot } from "./heal-state";
import { SpotGraphics } from "./spot-graphics";
import { addSpot, commandMessage, spotsOf, updateSpot, type Spot, type Vec } from "./spots";
import { useHealKeys } from "./use-heal-keys";

type Gesture =
  | { kind: "move"; grip: Grip; start: Vec; last: Vec; moved: boolean }
  | { kind: "paint" };

const dragThreshold = 3;

function guarded(action: () => void): void {
  try {
    action();
    setBrush({ message: undefined });
  } catch (error) {
    setBrush({ message: commandMessage(error) });
  }
}

function localPoint(event: React.PointerEvent<HTMLElement>): Vec {
  const rect = event.currentTarget.getBoundingClientRect();
  return { x: event.clientX - rect.left, y: event.clientY - rect.top };
}

function BrushCursor({ at, radius, feather }: { at: Vec; radius: number; feather: number }) {
  return (
    <g fill="none">
      <circle cx={at.x} cy={at.y} r={radius} className="stroke-white" strokeWidth={1} />
      <circle
        cx={at.x}
        cy={at.y}
        r={radius * Math.max(0.05, 1 - feather / 100)}
        className="stroke-white/50"
        strokeWidth={1}
      />
    </g>
  );
}

function PaintPreview({ points, radius }: { points: Vec[]; radius: number }) {
  const d = points.map((p, i) => `${i === 0 ? "M" : "L"}${p.x} ${p.y}`).join(" ");
  return (
    <g fill="none" className="stroke-white/30">
      <path d={d} strokeWidth={radius * 2} strokeLinecap="round" strokeLinejoin="round" />
      {points.length === 1 ? (
        <circle cx={points[0]?.x} cy={points[0]?.y} r={radius} className="fill-white/30" />
      ) : null}
    </g>
  );
}

interface SurfaceProps {
  mapping: Mapping;
  spots: Spot[];
}

function Surface({ mapping, spots }: SurfaceProps) {
  const selected = useSelectedSpot(spots.length);
  const current = useStore(brush, (state) => state);
  const gesture = useRef<Gesture | undefined>(undefined);
  const paintPoints = useRef<Vec[]>([]);
  const [paint, setPaint] = useState<Vec[]>([]);
  const [hover, setHover] = useState<Vec | undefined>(undefined);
  const [moving, setMoving] = useState(false);
  useHealKeys(selected);

  const shapes = shapesOf(mapping, spots);
  const grips = gripsOf(shapes);
  const overGrip =
    hover && !moving && paint.length === 0 ? gripAt(grips, hover, selected) : undefined;
  const radius = radiusOf(mapping.frame, current.size);

  const onPointerDown = (event: React.PointerEvent<HTMLDivElement>) => {
    if (event.button !== 0) {
      return;
    }
    event.currentTarget.setPointerCapture(event.pointerId);
    const at = localPoint(event);
    const grip = gripAt(grips, at, selected);
    if (grip) {
      pickSpot(grip.index, spots[grip.index]);
      gesture.current = { kind: "move", grip, start: at, last: at, moved: false };
      return;
    }
    gesture.current = { kind: "paint" };
    paintPoints.current = [at];
    setPaint([at]);
  };

  const dragGrip = (state: Extract<Gesture, { kind: "move" }>, at: Vec) => {
    if (!state.moved && distance(state.start, at) < dragThreshold) {
      return;
    }
    const [from, to] = cssToImage(mapping, [state.last, at]);
    state.moved = true;
    state.last = at;
    setMoving(true);
    if (!from || !to) {
      return;
    }
    const delta = [to.x - from.x, to.y - from.y];
    const { index, source } = state.grip;
    guarded(() =>
      updateSpot(
        index,
        { [source ? "moveSource" : "move"]: delta },
        source ? "Move source" : "Move spot",
        source ? "move-source" : "move",
      ),
    );
  };

  const onPointerMove = (event: React.PointerEvent<HTMLDivElement>) => {
    const at = localPoint(event);
    setHover(at);
    const state = gesture.current;
    if (state?.kind === "move") {
      dragGrip(state, at);
    } else if (state?.kind === "paint") {
      const last = paintPoints.current.at(-1);
      if (!last || distance(last, at) >= Math.max(radius * 0.35, 2)) {
        paintPoints.current = [...paintPoints.current, at];
        setPaint(paintPoints.current);
      }
    }
  };

  const finish = () => {
    const state = gesture.current;
    gesture.current = undefined;
    setMoving(false);
    if (state?.kind !== "paint") {
      return;
    }
    const points = cssToImage(mapping, paintPoints.current);
    paintPoints.current = [];
    setPaint([]);
    guarded(() => {
      const index = addSpot({
        mode: current.mode,
        points: points.map((p): [number, number] => [p.x, p.y]),
        size: current.size,
        feather: current.feather,
        opacity: current.opacity,
      });
      selectSpot(index);
    });
  };

  return (
    <div
      role="presentation"
      className={cn(
        "absolute inset-0 touch-none overflow-hidden",
        overGrip ? "cursor-grab" : "cursor-none",
      )}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={finish}
      onPointerCancel={() => {
        gesture.current = undefined;
        paintPoints.current = [];
        setPaint([]);
        setMoving(false);
      }}
      onPointerLeave={() => setHover(undefined)}
    >
      <svg className="pointer-events-none absolute inset-0 size-full" aria-hidden="true">
        {shapes.map((shape) => (
          <SpotGraphics
            key={`spot-${shape.index}`}
            shape={shape}
            selected={shape.index === selected}
          />
        ))}
        {paint.length > 0 ? <PaintPreview points={paint} radius={radius} /> : null}
        {hover && !overGrip && !moving && paint.length === 0 ? (
          <BrushCursor at={hover} radius={radius} feather={current.feather} />
        ) : null}
      </svg>
    </div>
  );
}

export function HealOverlay() {
  const facts = useStore(develop, (state) => state.facts);
  const settings = useStore(develop, (state) => state.settings);
  const frame = useStore(develop, (state) => state.frame);
  const compare = useStore(develop, (state) => state.compare);
  if (!facts || !settings || !frame || compare === "before") {
    return null;
  }
  return <Surface mapping={{ facts, settings, frame }} spots={spotsOf(settings)} />;
}
