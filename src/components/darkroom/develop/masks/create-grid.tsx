import {
  ColorPickerIcon,
  CircleIcon,
  MountainIcon,
  PaintBrush01Icon,
  SlidersHorizontalIcon,
  Sun01Icon,
  UserIcon,
} from "@hugeicons/core-free-icons";
import type { IconSvgElement } from "@hugeicons/react";

import { Icon } from "@/components/icons";

import { addMask } from "./masks-commands";
import { kindLabels, type CreateKind } from "./masks-model";

function LinearGlyph() {
  return (
    <svg
      viewBox="0 0 24 24"
      width={18}
      height={18}
      fill="none"
      stroke="currentColor"
      aria-hidden="true"
    >
      <rect x="3.5" y="3.5" width="17" height="17" rx="3" strokeWidth="1.5" />
      <path d="M3.5 9h17" strokeWidth="1.5" opacity="0.8" />
      <path d="M3.5 13h17" strokeWidth="1.5" opacity="0.5" />
      <path d="M3.5 17h17" strokeWidth="1.5" opacity="0.25" />
    </svg>
  );
}

const kindIcons: Record<CreateKind, IconSvgElement | undefined> = {
  linear: undefined,
  radial: CircleIcon,
  brush: PaintBrush01Icon,
  luminanceRange: SlidersHorizontalIcon,
  colorRange: ColorPickerIcon,
  subject: UserIcon,
  sky: Sun01Icon,
  background: MountainIcon,
};

export const createKinds: CreateKind[] = [
  "linear",
  "radial",
  "brush",
  "luminanceRange",
  "colorRange",
  "subject",
  "sky",
  "background",
];

const shortLabels: Record<CreateKind, string> = {
  linear: "Linear",
  radial: "Radial",
  brush: "Brush",
  luminanceRange: "Luminance",
  colorRange: "Color",
  subject: "Subject",
  sky: "Sky",
  background: "Backdrop",
};

export function KindGlyph({ kind, size = 16 }: { kind: CreateKind; size?: number }) {
  const icon = kindIcons[kind];
  return icon ? <Icon icon={icon} size={size} /> : <LinearGlyph />;
}

const shortcuts: Partial<Record<CreateKind, string>> = { brush: "B", linear: "L" };

export function CreateGrid() {
  return (
    <div className="grid grid-cols-4 gap-xs p-xs">
      {createKinds.map((kind) => (
        <button
          key={kind}
          type="button"
          title={`${kindLabels[kind]}${shortcuts[kind] ? ` (${shortcuts[kind]})` : ""}`}
          aria-label={`Add ${kindLabels[kind].toLowerCase()}`}
          onClick={() => addMask(kind)}
          className="flex h-[52px] flex-col items-center justify-center gap-xxs rounded-m bg-control text-secondary-text transition-colors duration-(--motion-duration) ease-(--motion-easing) hover:bg-control-hover hover:text-foreground"
        >
          <KindGlyph kind={kind} />
          <span className="text-xxs font-medium">{shortLabels[kind]}</span>
        </button>
      ))}
    </div>
  );
}
