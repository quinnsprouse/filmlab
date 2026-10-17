import type { Pt, Shape } from "./masks-model";

export interface FrameSize {
  width: number;
  height: number;
}

export type RadialShape = Extract<Shape, { kind: "radial" }>;
export type LinearShape = Extract<Shape, { kind: "linear" }>;

const rotateHandle = 5;

function longScale(frame: FrameSize): Pt {
  const long = Math.max(frame.width, frame.height);
  return { x: long / frame.width, y: long / frame.height };
}

function localToNorm(shape: RadialShape, frame: FrameSize, x: number, y: number): Pt {
  const scale = longScale(frame);
  const turn = (shape.angle * Math.PI) / 180;
  const sin = Math.sin(turn);
  const cos = Math.cos(turn);
  return {
    x: shape.center.x + (x * cos - y * sin) * scale.x,
    y: shape.center.y + (x * sin + y * cos) * scale.y,
  };
}

export function radialRing(shape: RadialShape, frame: FrameSize, ratio: number, steps = 64): Pt[] {
  return Array.from({ length: steps }, (_, index) => {
    const turn = (index / steps) * Math.PI * 2;
    return localToNorm(
      shape,
      frame,
      shape.rx * ratio * Math.cos(turn),
      shape.ry * ratio * Math.sin(turn),
    );
  });
}

export function radialEdgeHandles(shape: RadialShape, frame: FrameSize): Pt[] {
  return [
    localToNorm(shape, frame, shape.rx, 0),
    localToNorm(shape, frame, 0, shape.ry),
    localToNorm(shape, frame, -shape.rx, 0),
    localToNorm(shape, frame, 0, -shape.ry),
  ];
}

export function radialRotateHandle(shape: RadialShape, frame: FrameSize): Pt {
  return localToNorm(shape, frame, shape.rx + 0.08, 0);
}

function toLocal(shape: RadialShape, frame: FrameSize, at: Pt): Pt {
  const scale = longScale(frame);
  const dx = (at.x - shape.center.x) / scale.x;
  const dy = (at.y - shape.center.y) / scale.y;
  const turn = (shape.angle * Math.PI) / 180;
  const sin = Math.sin(turn);
  const cos = Math.cos(turn);
  return { x: dx * cos + dy * sin, y: -dx * sin + dy * cos };
}

export function radialContains(shape: RadialShape, frame: FrameSize, at: Pt): boolean {
  const local = toLocal(shape, frame, at);
  return (local.x / shape.rx) ** 2 + (local.y / shape.ry) ** 2 <= 1;
}

const minRadius = 0.01;
const snapRatio = 0.05;

function snapRadii(rx: number, ry: number, keep: "rx" | "ry"): { rx: number; ry: number } {
  const close = Math.abs(rx - ry) / Math.max(rx, ry) < snapRatio;
  if (!close) {
    return { rx, ry };
  }
  return keep === "rx" ? { rx, ry: rx } : { rx: ry, ry };
}

function resizeRadial(
  shape: RadialShape,
  frame: FrameSize,
  handle: number,
  at: Pt,
  proportional: boolean,
): RadialShape {
  const local = toLocal(shape, frame, at);
  if (handle === 1 || handle === 3) {
    const rx = Math.max(Math.abs(local.x), minRadius);
    const ry = proportional ? shape.ry * (rx / Math.max(shape.rx, minRadius)) : shape.ry;
    return { ...shape, ...snapRadii(rx, ry, "rx") };
  }
  const ry = Math.max(Math.abs(local.y), minRadius);
  const rx = proportional ? shape.rx * (ry / Math.max(shape.ry, minRadius)) : shape.rx;
  return { ...shape, ...snapRadii(rx, ry, "ry") };
}

function shift(point: Pt, delta: Pt): Pt {
  return { x: point.x + delta.x, y: point.y + delta.y };
}

export interface DragMove {
  handle: number;
  at: Pt;
  delta: Pt;
  proportional: boolean;
}

export function movedShape(shape: Shape, frame: FrameSize, move: DragMove): Shape {
  if (shape.kind === "radial") {
    if (move.handle >= 1 && move.handle <= 4) {
      return resizeRadial(shape, frame, move.handle, move.at, move.proportional);
    }
    if (move.handle === rotateHandle) {
      const scale = longScale(frame);
      const dx = (move.at.x - shape.center.x) / scale.x;
      const dy = (move.at.y - shape.center.y) / scale.y;
      return { ...shape, angle: (Math.atan2(dy, dx) * 180) / Math.PI };
    }
    return { ...shape, center: shift(shape.center, move.delta) };
  }
  if (shape.kind === "linear") {
    if (move.handle === 1) {
      return { ...shape, start: move.at };
    }
    if (move.handle === 2) {
      return { ...shape, end: move.at };
    }
    return { ...shape, start: shift(shape.start, move.delta), end: shift(shape.end, move.delta) };
  }
  return shape;
}

export function pinPoint(shape: Shape): Pt | undefined {
  if (shape.kind === "radial") {
    return shape.center;
  }
  if (shape.kind === "linear") {
    return { x: (shape.start.x + shape.end.x) / 2, y: (shape.start.y + shape.end.y) / 2 };
  }
  return undefined;
}

export function offset(from: Pt, to: Pt): Pt {
  return { x: to.x - from.x, y: to.y - from.y };
}
