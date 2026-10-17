import { develop, editCommand } from "@/lib/develop/session";
import { runCommand } from "@/lib/engine/engine";

import {
  kindLabels,
  shapeParams,
  type CreateKind,
  type MaskOp,
  type Pt,
  type Shape,
} from "./masks-model";
import { masksState, patchMasks, reportError, selectMask } from "./masks-state";

function activeMaskOf(result: unknown): number | undefined {
  if (typeof result !== "object" || result === null || !("activeMask" in result)) {
    return undefined;
  }
  return typeof result.activeMask === "number" ? result.activeMask : undefined;
}

function attempt<T>(action: () => T): T | undefined {
  try {
    const value = action();
    patchMasks({ error: undefined });
    return value;
  } catch (error) {
    reportError(error);
    return undefined;
  }
}

export function chooseMask(id: number | undefined): void {
  attempt(() => runCommand("mask.select", { id: id ?? null }));
  selectMask(id);
}

export function addMask(kind: CreateKind): void {
  attempt(() => {
    if (kind === "brush") {
      runCommand("mask.select", { id: null });
      patchMasks({
        photoId: develop.get().photoId,
        active: undefined,
        interaction: { mode: "paint", maskId: undefined },
      });
      return;
    }
    const result = editCommand(
      "mask.add",
      { kind },
      { label: `Add ${kindLabels[kind].toLowerCase()}`, key: "mask.add", draft: false },
    );
    selectMask(activeMaskOf(result));
    if (kind === "colorRange") {
      patchMasks({ interaction: { mode: "pick" } });
    }
  });
}

export function addComponent(maskId: number, op: MaskOp, kind: CreateKind): void {
  attempt(() => {
    if (kind !== "brush" || op !== "add") {
      editCommand(
        "mask.addComponent",
        { id: maskId, op, kind },
        { label: `${op} ${kindLabels[kind].toLowerCase()}`, key: "mask.component", draft: false },
      );
    }
    selectMask(maskId);
    if (kind === "colorRange") {
      patchMasks({ interaction: { mode: "pick" } });
    }
    if (kind === "brush") {
      patchMasks({ interaction: { mode: "paint", maskId } });
    }
  });
}

export function deleteMask(id: number): void {
  attempt(() => {
    const result = editCommand(
      "mask.delete",
      { id },
      { label: "Delete mask", key: "mask.delete", draft: false },
    );
    selectMask(activeMaskOf(result));
  });
}

export function toggleVisible(id: number): void {
  attempt(() =>
    editCommand(
      "mask.visible",
      { id },
      { label: "Toggle mask", key: "mask.visible", draft: false },
    ),
  );
}

export function invertMask(id: number): void {
  attempt(() =>
    editCommand("mask.invert", { id }, { label: "Invert mask", key: "mask.invert", draft: false }),
  );
}

export function renameMask(id: number, name: string): void {
  const trimmed = name.trim();
  if (trimmed) {
    attempt(() =>
      editCommand(
        "mask.rename",
        { id, name: trimmed },
        { label: "Rename mask", key: "mask.rename", draft: false },
      ),
    );
  }
}

export function duplicateMask(id: number): void {
  attempt(() => {
    const result = editCommand(
      "mask.duplicate",
      { id },
      { label: "Duplicate mask", key: "mask.duplicate", draft: false },
    );
    selectMask(activeMaskOf(result));
  });
}

export type ComponentAction = "invert" | "duplicate" | "delete" | "op";

export function editComponent(
  id: number,
  component: number,
  action: ComponentAction,
  op?: MaskOp,
): void {
  attempt(() => {
    const result = editCommand(
      "mask.component",
      { id, component, action, ...(op ? { op } : {}) },
      { label: `${action} component`, key: "mask.component", draft: false },
    );
    if (action === "delete") {
      selectMask(activeMaskOf(result));
    }
  });
}

export function setAdjust(id: number, field: string, value: number, label: string): void {
  attempt(() =>
    editCommand(
      "mask.adjust",
      { id, values: { [field]: value } },
      { label, key: `mask.adjust:${id}:${field}`, draft: true },
    ),
  );
}

export function setRefine(id: number, value: number): void {
  attempt(() =>
    editCommand(
      "mask.refine",
      { id, value },
      { label: `Refine edges ${Math.round(value)}`, key: `mask.refine:${id}`, draft: true },
    ),
  );
}

export function updateShape(
  id: number,
  component: number,
  shape: Shape,
  draft: boolean,
  label = "Edit mask",
): void {
  attempt(() =>
    editCommand(
      "mask.update",
      { id, component, shape: shapeParams(shape) },
      { label, key: `mask.shape:${id}:${component}`, draft },
    ),
  );
}

export interface StrokeInput {
  maskId: number | undefined;
  points: Pt[];
  pressure: number[];
  erase: boolean;
  connect: boolean;
}

export function paintStroke({ maskId, points, pressure, erase, connect }: StrokeInput): void {
  const { brush } = masksState.get();
  attempt(() => {
    const result = editCommand(
      "mask.brushStroke",
      {
        ...(maskId === undefined ? {} : { id: maskId }),
        points: points.map((point) => [point.x, point.y]),
        size: brush.size,
        feather: brush.feather,
        flow: brush.flow,
        erase,
        autoMask: brush.autoMask,
        connect,
        ...(pressure.length > 0 ? { pressure } : {}),
      },
      { label: erase ? "Erase brush stroke" : "Brush stroke", key: "mask.brush", draft: false },
    );
    const created = activeMaskOf(result);
    patchMasks({
      photoId: develop.get().photoId,
      active: created,
      interaction: { mode: "paint", maskId: created },
    });
  });
}

export function sampleColor(maskId: number, at: Pt, add: boolean): void {
  attempt(() =>
    editCommand(
      "mask.sampleColor",
      { id: maskId, x: at.x, y: at.y, add },
      { label: "Pick color", key: `mask.${maskId}.sample`, draft: false },
    ),
  );
}
