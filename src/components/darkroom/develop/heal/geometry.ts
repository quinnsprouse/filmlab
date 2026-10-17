import { mapPoints } from "@/lib/engine/engine";
import type { DevelopSettings, Facts, RenderedFrame } from "@/lib/engine/types";

import type { Spot, Vec } from "./spots";

export interface Mapping {
  facts: Facts;
  settings: DevelopSettings;
  frame: RenderedFrame;
}

function ratio(): number {
  return window.devicePixelRatio;
}

function map(mapping: Mapping, points: Vec[], toImage: boolean): Vec[] {
  if (points.length === 0) {
    return [];
  }
  const { facts, settings, frame } = mapping;
  return mapPoints(facts, settings, {
    width: frame.width,
    height: frame.height,
    applyCrop: true,
    points: points.map((p): [number, number] => [p.x, p.y]),
    toImage,
  }).map(([x, y]) => ({ x, y }));
}

export function cssToImage(mapping: Mapping, points: Vec[]): Vec[] {
  return map(
    mapping,
    points.map((p) => ({ x: p.x * ratio(), y: p.y * ratio() })),
    true,
  );
}

function imageToCss(mapping: Mapping, points: Vec[]): Vec[] {
  return map(mapping, points, false).map((p) => ({ x: p.x / ratio(), y: p.y / ratio() }));
}

export function radiusOf(frame: RenderedFrame, size: number): number {
  return (size * Math.max(frame.width, frame.height)) / ratio();
}

export interface SpotShape {
  index: number;
  mode: Spot["mode"];
  radius: number;
  points: Vec[];
  sourcePoints: Vec[];
}

export function shapesOf(mapping: Mapping, spots: Spot[]): SpotShape[] {
  const flat = spots.flatMap((spot) => {
    const offset = spot.source;
    return [
      ...spot.points,
      ...(offset ? spot.points.map((p) => ({ x: p.x + offset.x, y: p.y + offset.y })) : []),
    ];
  });
  const mapped = imageToCss(mapping, flat);
  let cursor = 0;
  return spots.map((spot, index) => {
    const count = spot.points.length;
    const points = mapped.slice(cursor, cursor + count);
    cursor += count;
    const sourcePoints = spot.source ? mapped.slice(cursor, cursor + count) : [];
    cursor += sourcePoints.length;
    return {
      index,
      mode: spot.mode,
      radius: radiusOf(mapping.frame, spot.size),
      points,
      sourcePoints,
    };
  });
}

export interface Grip {
  index: number;
  source: boolean;
  center: Vec;
  radius: number;
}

export function gripsOf(shapes: SpotShape[]): Grip[] {
  return shapes.flatMap((shape) => {
    const target = shape.points[0];
    const source = shape.sourcePoints[0];
    return [
      ...(target
        ? [{ index: shape.index, source: false, center: target, radius: shape.radius }]
        : []),
      ...(source
        ? [{ index: shape.index, source: true, center: source, radius: shape.radius }]
        : []),
    ];
  });
}

export function distance(a: Vec, b: Vec): number {
  return Math.hypot(a.x - b.x, a.y - b.y);
}

export function gripAt(grips: Grip[], at: Vec, selected: number | undefined): Grip | undefined {
  const hits = grips.filter((grip) => distance(grip.center, at) < Math.max(grip.radius, 8));
  return hits.toSorted((a, b) => {
    const rank = Number(a.index !== selected) - Number(b.index !== selected);
    return rank === 0 ? distance(a.center, at) - distance(b.center, at) : rank;
  })[0];
}
