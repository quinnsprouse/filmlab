import type { Facts } from "@/lib/engine/types";

const makers = [
  "RICOH IMAGING COMPANY, LTD.",
  "RICOH",
  "Canon",
  "NIKON CORPORATION",
  "NIKON",
  "SONY",
];

function cameraModel(camera: string): string {
  let model = camera.trim();
  for (const maker of makers) {
    if (model.toLowerCase().startsWith(maker.toLowerCase())) {
      model = model.slice(maker.length).trim();
    }
  }
  return model || camera;
}

function trimmed(value: number): string {
  return String(Math.round(value * 10) / 10);
}

export function exposureSummary(facts: Facts): string {
  const { meta } = facts;
  const parts = [
    cameraModel(meta.camera),
    meta.focal_mm === null ? "" : `${trimmed(meta.focal_mm)} mm`,
    meta.aperture === null ? "" : `ƒ/${trimmed(meta.aperture)}`,
    meta.shutter ? `${meta.shutter} s` : "",
    meta.iso === null ? "" : `ISO ${meta.iso}`,
  ];
  return parts.filter(Boolean).join("  ·  ");
}
