import { editCommand } from "@/lib/develop/session";

import { formatAngle } from "./geometry";

function attempt(run: () => unknown): { result: unknown; error: string | undefined } {
  try {
    return { result: run(), error: undefined };
  } catch (error) {
    return { result: undefined, error: error instanceof Error ? error.message : String(error) };
  }
}

export function setAngle(angle: number): void {
  editCommand(
    "develop.set",
    { values: { "crop.angle": angle } },
    { label: `Straighten ${formatAngle(angle)}°`, key: "crop.angle" },
  );
}

export function resetAngle(): void {
  editCommand(
    "develop.set",
    { values: { "crop.angle": 0 } },
    { label: "Straighten reset", key: "crop.angle.reset" },
  );
}

export function dragHandle(params: Record<string, unknown>, gesture: number): string | undefined {
  return attempt(() =>
    editCommand("crop.drag", params, { label: "Crop", key: `crop.drag:${gesture}` }),
  ).error;
}

export function straighten(angle: number, gesture: number): string | undefined {
  return attempt(() =>
    editCommand(
      "crop.straighten",
      { angle },
      {
        label: `Straighten ${formatAngle(angle)}°`,
        key: `crop.straighten:${gesture}`,
      },
    ),
  ).error;
}

export function setAspect(aspect: string, label: string): string | undefined {
  return attempt(() => editCommand("crop.aspect", { aspect }, { label, key: "crop.aspect" })).error;
}

export function rotateAspect(): string | undefined {
  return attempt(() =>
    editCommand("crop.rotateAspect", {}, { label: "Rotate crop", key: "crop.rotateAspect" }),
  ).error;
}

export function resetCrop(): string | undefined {
  return attempt(() => editCommand("crop.reset", {}, { label: "Reset crop", key: "crop.reset" }))
    .error;
}

export function autoStraighten(): string | undefined {
  const { result, error } = attempt(() =>
    editCommand(
      "crop.autoStraighten",
      {},
      { label: "Auto straighten", key: "crop.autoStraighten" },
    ),
  );
  if (error) {
    return error.includes("no decoder")
      ? "Auto straighten can't read this photo's pixels yet. Straighten by hand instead."
      : error;
  }
  const outcome =
    typeof result === "object" && result !== null ? Object.fromEntries(Object.entries(result)) : {};
  return outcome.changed === false
    ? "No clear horizon or vertical lines found. Straighten by hand instead."
    : undefined;
}

export type Orientation = "rotateLeft" | "rotateRight" | "flipHorizontal" | "flipVertical";

const orientationLabels: Record<Orientation, string> = {
  rotateLeft: "Rotate left",
  rotateRight: "Rotate right",
  flipHorizontal: "Flip horizontal",
  flipVertical: "Flip vertical",
};

export function orient(kind: Orientation): string | undefined {
  return attempt(() =>
    editCommand(`photo.${kind}`, {}, { label: orientationLabels[kind], key: `photo.${kind}` }),
  ).error;
}

export function setAspectRatio(width: number, height: number): string | undefined {
  return attempt(() =>
    editCommand(
      "crop.aspect",
      { aspect: [width, height] },
      { label: `Crop ${width} : ${height}`, key: "crop.aspect" },
    ),
  ).error;
}
