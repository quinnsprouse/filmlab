import { Folder, Slider } from "dialkit";

import { setAdjust, setRefine } from "./masks-commands";
import type { Mask } from "./masks-model";

interface Field {
  id: string;
  label: string;
  min: number;
  max: number;
  step: number;
  initial: number;
}

const symmetric = (id: string, label: string): Field => ({
  id,
  label,
  min: -100,
  max: 100,
  step: 1,
  initial: 0,
});

const groups: { title: string; open: boolean; fields: Field[] }[] = [
  {
    title: "Light",
    open: true,
    fields: [
      { id: "exposure", label: "Exposure", min: -4, max: 4, step: 0.01, initial: 0 },
      symmetric("contrast", "Contrast"),
      symmetric("highlights", "Highlights"),
      symmetric("shadows", "Shadows"),
      symmetric("whites", "Whites"),
      symmetric("blacks", "Blacks"),
    ],
  },
  {
    title: "Color",
    open: false,
    fields: [
      symmetric("temp", "Temp"),
      symmetric("tint", "Tint"),
      symmetric("hue", "Hue"),
      symmetric("saturation", "Saturation"),
      { id: "color_hue", label: "Overlay hue", min: 0, max: 360, step: 1, initial: 0 },
      { id: "color_sat", label: "Overlay saturation", min: 0, max: 100, step: 1, initial: 0 },
    ],
  },
  {
    title: "Effects",
    open: false,
    fields: [
      symmetric("texture", "Texture"),
      symmetric("clarity", "Clarity"),
      symmetric("dehaze", "Dehaze"),
    ],
  },
  {
    title: "Detail",
    open: false,
    fields: [
      symmetric("sharpness", "Sharpness"),
      symmetric("noise", "Noise"),
      symmetric("moire", "Moiré"),
      symmetric("defringe", "Defringe"),
    ],
  },
];

const amount: Field = { id: "amount", label: "Amount", min: 0, max: 200, step: 1, initial: 100 };

function format(field: Field, value: number): string {
  const digits = field.step < 1 ? 2 : 0;
  return value.toFixed(digits);
}

function FieldSlider({ mask, field }: { mask: Mask; field: Field }) {
  const value = mask.adjust[field.id] ?? field.initial;
  return (
    <div
      onDoubleClick={() => setAdjust(mask.id, field.id, field.initial, `${field.label} reset`)}
      data-control={`mask.${field.id}`}
    >
      <Slider
        label={field.label}
        value={Math.min(field.max, Math.max(field.min, value))}
        min={field.min}
        max={field.max}
        step={field.step}
        onChange={(next) =>
          setAdjust(
            mask.id,
            field.id,
            next,
            `Mask ${field.label.toLowerCase()} ${format(field, next)}`,
          )
        }
      />
    </div>
  );
}

export function MaskAdjust({ mask }: { mask: Mask }) {
  return (
    <>
      <FieldSlider mask={mask} field={amount} />
      {groups.map((group) => (
        <Folder key={group.title} title={group.title} defaultOpen={group.open}>
          {group.fields.map((field) => (
            <FieldSlider key={field.id} mask={mask} field={field} />
          ))}
        </Folder>
      ))}
      <div onDoubleClick={() => setRefine(mask.id, 0)}>
        <Slider
          label="Refine edges"
          value={mask.refine}
          min={0}
          max={100}
          step={1}
          onChange={(next) => setRefine(mask.id, next)}
        />
      </div>
    </>
  );
}
