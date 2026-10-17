import { editCommand } from "@/lib/develop/session";
import type { DevelopSettings } from "@/lib/engine/types";

export type SpotMode = "remove" | "heal" | "clone" | "fill";

export interface Vec {
  x: number;
  y: number;
}

export interface Spot {
  mode: SpotMode;
  points: Vec[];
  size: number;
  feather: number;
  opacity: number;
  source: Vec | undefined;
}

export const spotModes: { value: SpotMode; label: string }[] = [
  { value: "remove", label: "Remove" },
  { value: "heal", label: "Heal" },
  { value: "clone", label: "Clone" },
  { value: "fill", label: "Fill" },
];

export const sizeRange = { min: 0.001, max: 0.25 };

const modeLabels: Record<SpotMode, string> = {
  remove: "Remove",
  heal: "Heal",
  clone: "Clone",
  fill: "Fill",
};

export function modeLabel(mode: SpotMode): string {
  return modeLabels[mode];
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function toVec(value: unknown): Vec | undefined {
  if (isRecord(value) && typeof value.x === "number" && typeof value.y === "number") {
    return { x: value.x, y: value.y };
  }
  return undefined;
}

function toMode(value: unknown): SpotMode {
  return spotModes.find((entry) => entry.value === value)?.value ?? "remove";
}

function num(value: unknown, fallback: number): number {
  return typeof value === "number" ? value : fallback;
}

function toSpot(value: unknown): Spot {
  const raw = isRecord(value) ? value : {};
  const points = Array.isArray(raw.points)
    ? raw.points.map(toVec).filter((p) => p !== undefined)
    : [];
  return {
    mode: toMode(raw.mode),
    points,
    size: num(raw.size, 0.02),
    feather: num(raw.feather, 50),
    opacity: num(raw.opacity, 100),
    source: toVec(raw.source_offset),
  };
}

export function spotsOf(settings: DevelopSettings | undefined): Spot[] {
  const raw = settings?.spots;
  return Array.isArray(raw) ? raw.map(toSpot) : [];
}

export function commandMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export interface NewSpot {
  mode: SpotMode;
  points: [number, number][];
  size: number;
  feather: number;
  opacity: number;
}

let addCount = 0;

function indexOf(result: unknown): number | undefined {
  return isRecord(result) && typeof result.index === "number" ? result.index : undefined;
}

export function addSpot(spot: NewSpot): number | undefined {
  addCount += 1;
  const result = editCommand(
    "spot.add",
    { ...spot },
    {
      label: `${modeLabel(spot.mode)} spot`,
      key: `spot.add.${addCount}`,
      draft: false,
    },
  );
  return indexOf(result);
}

export function updateSpot(
  index: number,
  params: Record<string, unknown>,
  label: string,
  key: string,
  draft = true,
): void {
  editCommand("spot.update", { index, ...params }, { label, key: `spot.${index}.${key}`, draft });
}

export function deleteSpot(index: number, label = "Delete spot"): void {
  editCommand("spot.delete", { index }, { label, key: "spot.delete", draft: false });
}

export function clearSpots(count: number): void {
  for (let index = count - 1; index >= 0; index -= 1) {
    editCommand(
      "spot.delete",
      { index },
      { label: "Clear spots", key: "spot.clear", draft: false },
    );
  }
}

export function refreshSource(index: number): void {
  editCommand(
    "spot.refreshSource",
    { index },
    { label: "Refresh source", key: `spot.${index}.refresh`, draft: false },
  );
}

export function findDust(): number {
  const result = editCommand(
    "spot.findDust",
    {},
    { label: "Find dust", key: "spot.dust", draft: false },
  );
  return isRecord(result) && typeof result.added === "number" ? result.added : 0;
}
