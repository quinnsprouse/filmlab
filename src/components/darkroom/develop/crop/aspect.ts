import type { Vec } from "./geometry";

export const aspectOptions = [
  { value: "free", label: "Free" },
  { value: "original", label: "Original" },
  { value: "1x1", label: "1 : 1" },
  { value: "4x5", label: "4 : 5" },
  { value: "5x7", label: "5 : 7" },
  { value: "2x3", label: "2 : 3" },
  { value: "4x3", label: "4 : 3" },
  { value: "16x9", label: "16 : 9" },
  { value: "16x10", label: "16 : 10" },
  { value: "8.5x11", label: "8.5 : 11" },
  { value: "custom", label: "Custom…" },
];

const tolerance = 0.004;

function near(ratio: number, target: number): boolean {
  return (
    Number.isFinite(target) &&
    target > 0 &&
    (Math.abs(ratio - target) / target < tolerance ||
      Math.abs(1 / ratio - target) / target < tolerance)
  );
}

export function aspectValue(aspect: Vec | undefined, original: number): string {
  if (!aspect) {
    return "free";
  }
  const ratio = aspect[0] / aspect[1];
  if (near(ratio, original)) {
    return "original";
  }
  const preset = aspectOptions.find(({ value }) => {
    const [w, h] = value.split("x").map(Number);
    return w !== undefined && h !== undefined && near(ratio, w / h);
  });
  return preset?.value ?? "custom";
}

export function aspectName(value: string): string {
  return aspectOptions.find((option) => option.value === value)?.label ?? value;
}

export function parseRatio(text: string): number | undefined {
  const value = Number(text.trim().replace(",", "."));
  return Number.isFinite(value) && value > 0 && value <= 1000 ? value : undefined;
}
