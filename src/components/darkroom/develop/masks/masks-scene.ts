import {
  pinPoint,
  radialEdgeHandles,
  radialRing,
  radialRotateHandle,
  type LinearShape,
  type RadialShape,
} from "./masks-geometry";
import type { Mask, Pt, Shape } from "./masks-model";
import type { ViewSpace } from "./masks-view";

export interface Grip {
  maskId: number;
  component: number;
  handle: number;
  at: Pt;
  shape: Shape;
}

export interface RadialDraw {
  ring: Pt[];
  featherRing: Pt[];
  handles: Pt[];
  rotate: Pt;
  maskId: number;
  component: number;
}

export interface LinearDraw {
  start: Pt;
  end: Pt;
  middle: Pt;
}

interface PinDraw {
  maskId: number;
  component: number;
  at: Pt;
  selected: boolean;
}

export interface Scene {
  radials: RadialDraw[];
  linears: LinearDraw[];
  pins: PinDraw[];
  grips: Grip[];
}

function radialDraw(
  space: ViewSpace,
  shape: RadialShape,
  maskId: number,
  component: number,
): { draw: RadialDraw; grips: Grip[] } {
  const ring = radialRing(shape, space.frame, 1);
  const featherRatio = Math.max(0.05, 1 - shape.feather / 100);
  const featherRing = radialRing(shape, space.frame, featherRatio);
  const edges = radialEdgeHandles(shape, space.frame);
  const view = space.toView([
    ...ring,
    ...featherRing,
    ...edges,
    radialRotateHandle(shape, space.frame),
  ]);
  const handles = view.slice(ring.length * 2, ring.length * 2 + 4);
  const rotate = view.at(-1) ?? { x: 0, y: 0 };
  const handlePoints = [...handles, rotate];
  return {
    draw: {
      ring: view.slice(0, ring.length),
      featherRing: view.slice(ring.length, ring.length * 2),
      handles,
      rotate,
      maskId,
      component,
    },
    grips: handlePoints.map((at, position) => ({
      maskId,
      component,
      handle: position + 1,
      at,
      shape,
    })),
  };
}

function orZero(point: Pt | undefined): Pt {
  return point ?? { x: 0, y: 0 };
}

function linearDraw(
  space: ViewSpace,
  shape: LinearShape,
  maskId: number,
  component: number,
): { draw: LinearDraw; grips: Grip[] } {
  const middle = pinPoint(shape) ?? shape.start;
  const [start, end, center] = space.toView([shape.start, shape.end, middle]);
  return {
    draw: { start: orZero(start), end: orZero(end), middle: orZero(center) },
    grips: [
      { maskId, component, handle: 1, at: orZero(start), shape },
      { maskId, component, handle: 2, at: orZero(end), shape },
    ],
  };
}

export function buildScene(masks: Mask[], activeId: number | undefined, space: ViewSpace): Scene {
  const scene: Scene = { radials: [], linears: [], pins: [], grips: [] };
  const pinned: { maskId: number; component: number; shape: Shape; norm: Pt }[] = [];
  for (const mask of masks) {
    for (const [component, { shape }] of mask.components.entries()) {
      const selected = mask.id === activeId;
      if (selected && shape.kind === "radial") {
        const built = radialDraw(space, shape, mask.id, component);
        scene.radials.push(built.draw);
        scene.grips.push(...built.grips);
      }
      if (selected && shape.kind === "linear") {
        const built = linearDraw(space, shape, mask.id, component);
        scene.linears.push(built.draw);
        scene.grips.push(...built.grips);
      }
      const norm = pinPoint(shape);
      if (norm) {
        pinned.push({ maskId: mask.id, component, shape, norm });
      }
    }
  }
  const views = pinned.length > 0 ? space.toView(pinned.map((pin) => pin.norm)) : [];
  for (const [position, pin] of pinned.entries()) {
    const at = views[position];
    if (at) {
      scene.pins.push({
        maskId: pin.maskId,
        component: pin.component,
        at,
        selected: pin.maskId === activeId,
      });
      scene.grips.push({
        maskId: pin.maskId,
        component: pin.component,
        handle: 0,
        at,
        shape: pin.shape,
      });
    }
  }
  return scene;
}
