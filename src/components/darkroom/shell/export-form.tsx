import { Slider } from "dialkit";

import type { ExportFormat } from "@/lib/engine/types";

import { Segmented } from "./segmented";

type SizeMode = "full" | "edge";

export interface ExportChoices {
  format: ExportFormat;
  size: SizeMode;
  edge: number;
  quality: number;
}

interface ExportFormProps {
  choices: ExportChoices;
  destination: string;
  onChange: (choices: ExportChoices) => void;
}

export function ExportForm({ choices, destination, onChange }: ExportFormProps) {
  return (
    <div className="flex flex-col gap-m">
      <div className="flex items-center justify-between gap-m">
        <span className="text-s font-medium text-secondary-text">Format</span>
        <Segmented
          label="Format"
          value={choices.format}
          options={[
            { value: "jpeg", label: "JPEG" },
            { value: "png", label: "PNG" },
            { value: "tiff", label: "TIFF" },
          ]}
          onChange={(format) => onChange({ ...choices, format })}
        />
      </div>
      <div className="flex items-center justify-between gap-m">
        <span className="text-s font-medium text-secondary-text">Size</span>
        <div className="flex items-center gap-xs">
          {choices.size === "edge" && (
            <label className="flex items-center gap-xs text-s text-secondary-text">
              <input
                type="number"
                min={64}
                max={16384}
                step={64}
                value={choices.edge}
                aria-label="Long edge in pixels"
                onChange={(event) => onChange({ ...choices, edge: Number(event.target.value) })}
                className="h-8 w-20 rounded-s bg-control px-xs text-right font-data text-s text-foreground tabular-nums ring ring-border focus-visible:ring-primary"
              />
              px
            </label>
          )}
          <Segmented
            label="Size"
            value={choices.size}
            options={[
              { value: "full", label: "Full size" },
              { value: "edge", label: "Long edge" },
            ]}
            onChange={(size) => onChange({ ...choices, size })}
          />
        </div>
      </div>
      {choices.format === "jpeg" && (
        <div className="dialkit-root" data-mode="inline" data-theme="dark">
          <Slider
            label="Quality"
            value={choices.quality}
            min={0}
            max={100}
            step={1}
            onChange={(quality) => onChange({ ...choices, quality })}
          />
        </div>
      )}
      <p className="text-xs text-tertiary-text">{destination}</p>
    </div>
  );
}
