import { useRef, useState, type PointerEvent } from "react";

import { paintStroke, chooseMask, updateShape, sampleColor } from "./masks-commands";
import { movedShape, offset, radialContains } from "./masks-geometry";
import type { Mask, Pt } from "./masks-model";
import type { Grip, Scene } from "./masks-scene";
import { masksState, patchMasks, type BrushSettings, type Interaction } from "./masks-state";
import type { ViewSpace } from "./masks-view";

const gripRadius = 10;

interface HandleDrag {
  kind: "handle";
  grip: Grip;
  start: Pt;
}

interface PaintDrag {
  kind: "paint";
  points: Pt[];
  pressure: number[];
  penOnly: boolean;
}

type Drag = HandleDrag | PaintDrag;

export interface PointerInputs {
  space: ViewSpace;
  scene: Scene;
  masks: Mask[];
  activeId: number | undefined;
  interaction: Interaction;
  brush: BrushSettings;
}

function local(event: PointerEvent<HTMLElement>): Pt {
  const box = event.currentTarget.getBoundingClientRect();
  return { x: event.clientX - box.left, y: event.clientY - box.top };
}

function distance(a: Pt, b: Pt): number {
  return Math.hypot(a.x - b.x, a.y - b.y);
}

function gripAt(scene: Scene, at: Pt, activeId: number | undefined): Grip | undefined {
  const near = scene.grips.filter((grip) => distance(grip.at, at) < gripRadius);
  return near.toSorted(
    (a, b) =>
      Number(a.maskId !== activeId) - Number(b.maskId !== activeId) ||
      distance(a.at, at) - distance(b.at, at),
  )[0];
}

function bodyGrip(inputs: PointerInputs, at: Pt): Grip | undefined {
  const mask = inputs.masks.find((candidate) => candidate.id === inputs.activeId);
  if (!mask) {
    return undefined;
  }
  const [norm] = inputs.space.toImage([at]);
  const first = mask.components[0];
  if (first?.shape.kind === "linear") {
    return { maskId: mask.id, component: 0, handle: 0, at, shape: first.shape };
  }
  const component = mask.components.findIndex(
    ({ shape }) =>
      norm && shape.kind === "radial" && radialContains(shape, inputs.space.frame, norm),
  );
  const shape = mask.components[component]?.shape;
  return shape ? { maskId: mask.id, component, handle: 0, at, shape } : undefined;
}

function pressureOf(event: PointerEvent<HTMLElement>, brush: BrushSettings): number | undefined {
  return event.pointerType === "pen" && brush.pressure ? event.pressure : undefined;
}

export function useMaskPointer(inputs: PointerInputs) {
  const drag = useRef<Drag | undefined>(undefined);
  const [cursor, setCursor] = useState<Pt | undefined>(undefined);
  const [stroke, setStroke] = useState<Pt[] | undefined>(undefined);
  const [alt, setAlt] = useState(false);
  const [overGrip, setOverGrip] = useState(false);
  const { interaction } = inputs;

  const beginHandle = (grip: Grip, at: Pt) => {
    if (grip.maskId !== inputs.activeId) {
      chooseMask(grip.maskId);
    }
    const [start] = inputs.space.toImage([at]);
    if (start) {
      drag.current = { kind: "handle", grip, start };
      patchMasks({ engaged: true });
    }
  };

  const onPointerDown = (event: PointerEvent<HTMLElement>) => {
    if (event.button !== 0) {
      return;
    }
    const at = local(event);
    if (interaction.mode === "pick") {
      const maskId = inputs.activeId;
      const [norm] = inputs.space.toImage([at]);
      if (maskId !== undefined && norm) {
        sampleColor(maskId, norm, event.shiftKey);
      }
      return;
    }
    event.currentTarget.setPointerCapture(event.pointerId);
    if (interaction.mode === "paint") {
      const pressure = pressureOf(event, inputs.brush);
      drag.current = {
        kind: "paint",
        points: [at],
        pressure: pressure === undefined ? [] : [pressure],
        penOnly: pressure !== undefined,
      };
      setStroke([at]);
      patchMasks({ engaged: true });
      return;
    }
    const grip = gripAt(inputs.scene, at, inputs.activeId) ?? bodyGrip(inputs, at);
    if (grip) {
      beginHandle(grip, at);
    }
  };

  const extendStroke = (event: PointerEvent<HTMLElement>, current: PaintDrag) => {
    const box = event.currentTarget.getBoundingClientRect();
    const coalesced = event.nativeEvent.getCoalescedEvents?.() ?? [];
    for (const sample of coalesced.length > 0 ? coalesced : [event.nativeEvent]) {
      const point = { x: sample.clientX - box.left, y: sample.clientY - box.top };
      const last = current.points.at(-1);
      if (last && distance(last, point) < 0.5) {
        continue;
      }
      current.points.push(point);
      if (current.penOnly) {
        current.pressure.push(sample.pressure);
      }
    }
    setStroke([...current.points]);
  };

  const moveHandle = (event: PointerEvent<HTMLElement>, current: HandleDrag) => {
    const [at] = inputs.space.toImage([local(event)]);
    if (!at) {
      return;
    }
    const shape = movedShape(current.grip.shape, inputs.space.frame, {
      handle: current.grip.handle,
      at,
      delta: offset(current.start, at),
      proportional: event.altKey,
    });
    updateShape(current.grip.maskId, current.grip.component, shape, true);
  };

  const hover = (at: Pt) => {
    const grip =
      interaction.mode === "adjust" ? gripAt(inputs.scene, at, inputs.activeId) : undefined;
    setOverGrip(
      grip !== undefined || (interaction.mode === "adjust" && bodyGrip(inputs, at) !== undefined),
    );
    const pinMask = grip?.handle === 0 ? grip.maskId : undefined;
    if (masksState.get().hovered !== pinMask) {
      patchMasks({ hovered: pinMask });
    }
  };

  const onPointerMove = (event: PointerEvent<HTMLElement>) => {
    const at = local(event);
    setCursor(at);
    setAlt(event.altKey);
    const current = drag.current;
    if (current?.kind === "paint") {
      extendStroke(event, current);
    } else if (current?.kind === "handle") {
      moveHandle(event, current);
    } else {
      hover(at);
    }
  };

  const finishStroke = (event: PointerEvent<HTMLElement>, current: PaintDrag) => {
    setStroke(undefined);
    const norm = inputs.space.toImage(current.points);
    const maskId = interaction.mode === "paint" ? interaction.maskId : undefined;
    paintStroke({
      maskId,
      points: norm,
      pressure: current.penOnly ? current.pressure : [],
      erase: inputs.brush.erase !== event.altKey,
      connect: event.shiftKey && current.points.length === 1,
    });
  };

  const onPointerUp = (event: PointerEvent<HTMLElement>) => {
    const current = drag.current;
    drag.current = undefined;
    if (current?.kind === "paint") {
      finishStroke(event, current);
    }
    if (current) {
      patchMasks({ engaged: false });
    }
  };

  const onPointerLeave = () => {
    setCursor(undefined);
    if (!drag.current && masksState.get().hovered !== undefined) {
      patchMasks({ hovered: undefined });
    }
  };

  return {
    handlers: {
      onPointerDown,
      onPointerMove,
      onPointerUp,
      onPointerCancel: onPointerUp,
      onPointerLeave,
    },
    cursor,
    stroke,
    alt,
    overGrip,
  };
}
