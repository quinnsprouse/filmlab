import { useRef, useState } from "react";

import { useStore } from "@/hooks/use-store";
import { develop } from "@/lib/develop/session";
import type { DevelopSettings, Facts, RenderedFrame } from "@/lib/engine/types";

import { dragHandle, straighten } from "./crop-commands";
import { CropGuides, type Guide } from "./crop-guides";
import { finishCrop } from "./crop-session";
import {
  formatAngle,
  frameMap,
  insideRect,
  readCrop,
  type CropRect,
  type FrameMap,
  type Vec,
} from "./geometry";

const reach = 12;
const moveHandle = 8;

const rotateCursor =
  "url(\"data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' width='24' height='24' viewBox='0 0 24 24'><circle cx='12' cy='12' r='11' fill='rgba(0,0,0,0.6)'/><path d='M7 12a5 5 0 0 1 9-3M17 12a5 5 0 0 1-9 3' fill='none' stroke='white' stroke-width='1.6'/></svg>\") 12 12, crosshair";

const handleCursors = [
  "nwse-resize",
  "nesw-resize",
  "nwse-resize",
  "nesw-resize",
  "ns-resize",
  "ew-resize",
  "ns-resize",
  "ew-resize",
  "move",
];

type Gesture =
  | { kind: "handle"; id: number; handle: number; start: CropRect; angle: number; origin: Vec }
  | { kind: "rotate"; id: number; startAngle: number; startPointer: number };

interface Surface {
  width: number;
  height: number;
  scale: number;
}

function handlePoints(corners: Vec[]): Vec[] {
  const edges = [0, 1, 2, 3].map((i): Vec => {
    const from = corners[i] ?? [0, 0];
    const to = corners[(i + 1) % 4] ?? from;
    return [(from[0] + to[0]) / 2, (from[1] + to[1]) / 2];
  });
  return [...corners, ...edges];
}

function nearest(points: Vec[], at: Vec): number | undefined {
  const found = points.findIndex(([x, y]) => Math.hypot(x - at[0], y - at[1]) < reach);
  return found < 0 ? undefined : found;
}

function pointerAngle(surface: Surface, at: Vec): number {
  return Math.atan2(at[1] - surface.height / 2, at[0] - surface.width / 2);
}

function turned(from: number, to: number): number {
  const delta = to - from;
  return Math.atan2(Math.sin(delta), Math.cos(delta));
}

interface SurfaceProps {
  facts: Facts;
  settings: DevelopSettings;
  frame: RenderedFrame;
}

function CropSurface({ facts, settings, frame }: SurfaceProps) {
  const rootRef = useRef<HTMLDivElement>(null);
  const gesture = useRef<Gesture | undefined>(undefined);
  const counter = useRef(0);
  const [active, setActive] = useState<Gesture["kind"] | undefined>();
  const [cursor, setCursor] = useState("move");
  const [readout, setReadout] = useState<{ at: Vec; angle: number } | undefined>();

  const scale = 1 / window.devicePixelRatio;
  const surface: Surface = { width: frame.width * scale, height: frame.height * scale, scale };
  const map: FrameMap = frameMap(facts, settings, frame);
  const crop = readCrop(settings);
  const corners = map.corners(crop).map(([x, y]): Vec => [x * scale, y * scale]);
  const handles = handlePoints(corners);

  const local = (event: React.PointerEvent): Vec => {
    const box = rootRef.current?.getBoundingClientRect();
    return [event.clientX - (box?.left ?? 0), event.clientY - (box?.top ?? 0)];
  };
  const straight = (at: Vec, angle: number): Vec =>
    map.toStraight([at[0] / scale, at[1] / scale], angle);
  const inside = (at: Vec) => insideRect(straight(at, crop.angle), crop.rect);

  const hover = (at: Vec) => {
    const handle = nearest(handles, at);
    setCursor(
      handle === undefined
        ? inside(at)
          ? "move"
          : rotateCursor
        : (handleCursors[handle] ?? "move"),
    );
  };

  const onPointerDown = (event: React.PointerEvent<HTMLDivElement>) => {
    if (event.button !== 0) {
      return;
    }
    const at = local(event);
    counter.current += 1;
    const id = counter.current;
    const handle = nearest(handles, at) ?? (inside(at) ? moveHandle : undefined);
    event.currentTarget.setPointerCapture(event.pointerId);
    if (handle === undefined) {
      gesture.current = {
        kind: "rotate",
        id,
        startAngle: crop.angle,
        startPointer: pointerAngle(surface, at),
      };
      setReadout({ at, angle: crop.angle });
    } else {
      gesture.current = {
        kind: "handle",
        id,
        handle,
        start: crop.rect,
        angle: crop.angle,
        origin: straight(at, crop.angle),
      };
    }
    setActive(gesture.current.kind);
  };

  const onPointerMove = (event: React.PointerEvent<HTMLDivElement>) => {
    const at = local(event);
    const current = gesture.current;
    if (!current) {
      hover(at);
      return;
    }
    if (current.kind === "handle") {
      const { start } = current;
      dragHandle(
        {
          handle: current.handle,
          from: current.origin,
          to: straight(at, current.angle),
          start: [start.x0, start.y0, start.x1, start.y1],
          keepRatio: event.shiftKey,
        },
        current.id,
      );
      return;
    }
    const degrees = (turned(current.startPointer, pointerAngle(surface, at)) * 180) / Math.PI;
    const angle = Math.round(Math.max(-45, Math.min(45, current.startAngle + degrees)) * 100) / 100;
    straighten(angle, current.id);
    setReadout({ at, angle });
  };

  const onPointerUp = () => {
    gesture.current = undefined;
    setActive(undefined);
    setReadout(undefined);
  };

  const onDoubleClick = (event: React.MouseEvent) => {
    const box = rootRef.current?.getBoundingClientRect();
    const at: Vec = [event.clientX - (box?.left ?? 0), event.clientY - (box?.top ?? 0)];
    if (inside(at) && nearest(handles, at) === undefined) {
      finishCrop();
    }
  };

  const guide: Guide = active === "rotate" ? "grid" : active === "handle" ? "thirds" : "none";
  return (
    <div ref={rootRef} className="absolute inset-0 touch-none select-none">
      <CropGuides
        width={surface.width}
        height={surface.height}
        corners={corners}
        handles={handles}
        guide={guide}
      />
      <div
        role="presentation"
        className="absolute -inset-[24px] cursor-(--crop-cursor)"
        style={{ "--crop-cursor": cursor }}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
        onPointerCancel={onPointerUp}
        onDoubleClick={onDoubleClick}
      />
      {readout ? (
        <span
          className="pointer-events-none absolute top-(--readout-y) left-(--readout-x) rounded-xs bg-canvas/80 px-xs py-xxs font-data text-xs text-foreground tabular-nums"
          style={{
            "--readout-x": `${readout.at[0] + 18}px`,
            "--readout-y": `${readout.at[1] + 14}px`,
          }}
        >
          {formatAngle(readout.angle)}°
        </span>
      ) : null}
    </div>
  );
}

export function CropOverlay() {
  const facts = useStore(develop, (state) => state.facts);
  const settings = useStore(develop, (state) => state.settings);
  const frame = useStore(develop, (state) => state.frame);
  if (!facts || !settings || !frame) {
    return null;
  }
  return <CropSurface facts={facts} settings={settings} frame={frame} />;
}
