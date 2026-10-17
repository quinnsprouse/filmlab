import { mapPoints } from "@/lib/engine/engine";
import type { DevelopSettings, Facts } from "@/lib/engine/types";

export type Vec = [number, number];

export interface CropRect {
  x0: number;
  y0: number;
  x1: number;
  y1: number;
}

export interface CropState {
  rect: CropRect;
  angle: number;
  aspect: Vec | undefined;
  flipH: boolean;
  flipV: boolean;
}

const fullRect: CropRect = { x0: 0, y0: 0, x1: 1, y1: 1 };

function record(value: unknown): Record<string, unknown> {
  return typeof value === "object" && value !== null
    ? Object.fromEntries(Object.entries(value))
    : {};
}

function num(value: unknown, fallback: number): number {
  return typeof value === "number" && Number.isFinite(value) ? value : fallback;
}

function pair(value: unknown): Vec | undefined {
  if (Array.isArray(value) && value.length === 2) {
    const w: unknown = value[0];
    const h: unknown = value[1];
    if (typeof w === "number" && typeof h === "number" && w > 0 && h > 0) {
      return [w, h];
    }
  }
  return undefined;
}

export function readCrop(settings: DevelopSettings): CropState {
  const crop = record(settings.crop);
  const geometry = record(crop.geometry);
  const rect = record(geometry.rect);
  return {
    rect: {
      x0: num(rect.x0, fullRect.x0),
      y0: num(rect.y0, fullRect.y0),
      x1: num(rect.x1, fullRect.x1),
      y1: num(rect.y1, fullRect.y1),
    },
    angle: num(geometry.angle, 0),
    aspect: pair(crop.aspect),
    flipH: crop.flip_h === true,
    flipV: crop.flip_v === true,
  };
}

export interface FrameSize {
  width: number;
  height: number;
}

export interface FrameMap {
  size: FrameSize;
  toStraight: (frame: Vec, angle: number) => Vec;
  corners: (crop: CropState) => Vec[];
}

function rotateAbout(point: Vec, degrees: number, center: Vec): Vec {
  const radians = (degrees * Math.PI) / 180;
  const cos = Math.cos(radians);
  const sin = Math.sin(radians);
  const dx = point[0] - center[0];
  const dy = point[1] - center[1];
  return [center[0] + cos * dx - sin * dy, center[1] + sin * dx + cos * dy];
}

interface Affine {
  origin: Vec;
  unitX: Vec;
  unitY: Vec;
}

function uprightAffine(facts: Facts, settings: DevelopSettings, size: FrameSize): Affine {
  const points = mapPoints(facts, settings, {
    width: size.width,
    height: size.height,
    applyCrop: false,
    points: [
      [0, 0],
      [1, 0],
      [0, 1],
    ],
    toImage: false,
  });
  const origin = points[0] ?? [0, 0];
  const x = points[1] ?? [1, 0];
  const y = points[2] ?? [0, 1];
  return {
    origin,
    unitX: [x[0] - origin[0], x[1] - origin[1]],
    unitY: [y[0] - origin[0], y[1] - origin[1]],
  };
}

export function frameMap(facts: Facts, settings: DevelopSettings, size: FrameSize): FrameMap {
  const { width, height } = size;
  const { origin, unitX, unitY } = uprightAffine(facts, settings, size);
  const det = unitX[0] * unitY[1] - unitY[0] * unitX[1] || 1;
  const center: Vec = [width / 2, height / 2];
  const toNorm = ([x, y]: Vec): Vec => {
    const dx = x - origin[0];
    const dy = y - origin[1];
    return [(unitY[1] * dx - unitY[0] * dy) / det, (unitX[0] * dy - unitX[1] * dx) / det];
  };
  const toFrame = ([nx, ny]: Vec): Vec => [
    origin[0] + unitX[0] * nx + unitY[0] * ny,
    origin[1] + unitX[1] * nx + unitY[1] * ny,
  ];
  const toStraight = (frame: Vec, angle: number): Vec => {
    const norm = toNorm(frame);
    const rotated = rotateAbout([norm[0] * width, norm[1] * height], angle, center);
    return [rotated[0] / width, rotated[1] / height];
  };
  return {
    size,
    toStraight,
    corners: ({ rect, angle }) =>
      (
        [
          [rect.x0, rect.y0],
          [rect.x1, rect.y0],
          [rect.x1, rect.y1],
          [rect.x0, rect.y1],
        ] satisfies Vec[]
      ).map((corner) => toFrame(unrotate(corner, angle, size, center))),
  };
}

function unrotate(point: Vec, angle: number, size: FrameSize, center: Vec): Vec {
  const rotated = rotateAbout([point[0] * size.width, point[1] * size.height], -angle, center);
  return [rotated[0] / size.width, rotated[1] / size.height];
}

export function insideRect(point: Vec, rect: CropRect): boolean {
  return point[0] >= rect.x0 && point[0] <= rect.x1 && point[1] >= rect.y0 && point[1] <= rect.y1;
}

export function formatAngle(angle: number): string {
  const rounded = Math.round(angle * 100) / 100;
  const text = Math.abs(rounded).toFixed(2);
  return `${rounded > 0 ? "+" : rounded < 0 ? "-" : ""}${text}`;
}
