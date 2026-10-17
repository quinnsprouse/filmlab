import { useStore } from "@/hooks/use-store";
import { library, type PhotoRecord } from "@/lib/library/library";
import { ui } from "@/lib/ui";

import { cameraModel, capturedParts } from "./model";

function InfoRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex h-7 shrink-0 items-center justify-between gap-m">
      <dt className="text-s font-medium text-tertiary-text">{label}</dt>
      <dd className="min-w-0 truncate text-right font-data text-xs font-medium text-secondary-text">
        {value}
      </dd>
    </div>
  );
}

function shutterText(shutter: string): string {
  return shutter && !shutter.endsWith("s") ? `${shutter} s` : shutter;
}

function joinParts(parts: (string | number | null | undefined)[]): string {
  return parts.filter(Boolean).join(" · ");
}

function infoRows(record: PhotoRecord): [string, string][] {
  const facts = record.probed?.facts;
  const meta = facts?.meta;
  const taken = capturedParts(facts?.captured ?? null);
  const file = record.file.companion ? `${record.file.name} · RAW + JPEG` : record.file.name;
  return [
    ["File", file],
    ["Taken", taken ? joinParts([taken.day, taken.time]) : ""],
    [
      "Camera",
      joinParts([
        meta?.camera && cameraModel(meta.camera),
        meta?.focal_mm && `${Number(meta.focal_mm.toFixed(1))} mm`,
      ]),
    ],
    ["Lens", meta?.lens ?? ""],
    [
      "Exposure",
      joinParts([
        meta?.aperture && `ƒ/${meta.aperture}`,
        meta?.shutter && shutterText(meta.shutter),
        meta?.iso && `ISO ${meta.iso}`,
      ]),
    ],
    ["Size", facts ? `${facts.width} × ${facts.height} px` : ""],
  ];
}

export function InfoPanel() {
  const focused = useStore(ui, (state) => state.focused);
  const index = useStore(library, (state) =>
    state.photos.findIndex((record) => record.file.id === focused),
  );
  const record = useStore(library, (state) => state.photos[index]);
  if (!record) {
    return null;
  }
  const rows = infoRows(record);
  return (
    <section aria-label="Info" className="flex flex-col rounded-l bg-panel p-s ring ring-border">
      <div className="flex items-baseline justify-between pb-xs">
        <h2 className="text-s font-heavy text-secondary-text">Info</h2>
        <span className="font-data text-xs font-heavy text-primary">▸{index + 1}</span>
      </div>
      <dl className="flex flex-col">
        {rows
          .filter(([, value]) => value !== "")
          .map(([label, value]) => (
            <InfoRow key={label} label={label} value={value} />
          ))}
      </dl>
    </section>
  );
}
