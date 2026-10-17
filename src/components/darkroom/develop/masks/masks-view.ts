import { mapPoints } from "@/lib/engine/engine";
import type { DevelopSettings, Facts, RenderedFrame } from "@/lib/engine/types";

import type { FrameSize } from "./masks-geometry";
import type { Pt } from "./masks-model";

export interface ViewSpace {
  frame: FrameSize;
  longEdge: number;
  toView(points: Pt[]): Pt[];
  toImage(points: Pt[]): Pt[];
}

export function viewSpace(
  facts: Facts,
  settings: DevelopSettings,
  frame: RenderedFrame,
  scale: number,
): ViewSpace {
  const map = (points: Pt[], toImage: boolean): Pt[] =>
    mapPoints(facts, settings, {
      width: frame.width,
      height: frame.height,
      applyCrop: true,
      points: points.map((point) => [point.x, point.y]),
      toImage,
    }).map(([x, y]) => ({ x: x ?? 0, y: y ?? 0 }));
  return {
    frame: { width: frame.width, height: frame.height },
    longEdge: Math.max(frame.width, frame.height) / scale,
    toView: (points) => map(points, false).map(({ x, y }) => ({ x: x / scale, y: y / scale })),
    toImage: (points) =>
      map(
        points.map(({ x, y }) => ({ x: x * scale, y: y * scale })),
        true,
      ),
  };
}
