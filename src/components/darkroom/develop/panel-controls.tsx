import { Slider } from "dialkit";
import type { ReactNode } from "react";

import { editControls } from "@/lib/develop/session";
import { controlSpecs } from "@/lib/engine/engine";
import type { ControlSpec } from "@/lib/engine/types";

let specCache: Map<string, ControlSpec> | undefined;

function specOf(id: string): ControlSpec | undefined {
  specCache ??= new Map(controlSpecs().map((spec) => [spec.id, spec]));
  return specCache.get(id);
}

function formatValue(spec: ControlSpec, value: number): string {
  const text = value.toFixed(spec.decimals);
  return value > 0 && spec.min < 0 ? `+${text}` : text;
}

export function sentence(text: string): string {
  return text.charAt(0).toUpperCase() + text.slice(1);
}

interface SegmentedProps<T extends string> {
  label: string;
  value: T;
  options: { value: T; label: string }[];
  onChange: (value: T) => void;
}

export function Segmented<T extends string>({
  label,
  value,
  options,
  onChange,
}: SegmentedProps<T>) {
  return (
    <fieldset className="dialkit-labeled-control">
      <legend className="dialkit-labeled-control-label float-left">{label}</legend>
      <div className="dialkit-segmented">
        {options.map((option) => (
          <label
            key={option.value}
            data-active={String(option.value === value)}
            className="dialkit-segmented-button"
          >
            <input
              type="radio"
              className="sr-only"
              name={label}
              value={option.value}
              checked={option.value === value}
              onChange={() => onChange(option.value)}
            />
            {option.label}
          </label>
        ))}
      </div>
    </fieldset>
  );
}

interface ControlSliderProps {
  id: string;
  label: string;
  value: number;
  range?: { min: number; max: number; step: number };
  onChange?: (value: number) => void;
  onReset?: () => void;
}

export function ControlSlider({ id, label, value, range, onChange, onReset }: ControlSliderProps) {
  const spec = specOf(id);
  if (!spec) {
    return null;
  }
  const { min, max, step } = range ?? spec;
  const change =
    onChange ??
    ((next: number) => editControls({ [id]: next }, `${label} ${formatValue(spec, next)}`));
  const decimals = Math.max(spec.decimals, decimalsOf(step));
  const reset = onReset ?? (() => editControls({ [id]: spec.default }, `${label} reset`));
  return (
    <div onDoubleClick={reset} data-control={id}>
      <Slider
        label={label}
        value={Math.min(max, Math.max(min, value))}
        min={min}
        max={max}
        step={step}
        onChange={(next) => change(Number(next.toFixed(decimals)))}
      />
    </div>
  );
}

function decimalsOf(step: number): number {
  const text = String(step);
  const dot = text.indexOf(".");
  return dot < 0 ? 0 : text.length - dot - 1;
}

export function PanelRoot({ title, children }: { title: string; children: ReactNode }) {
  return (
    <div className="dialkit-root" data-mode="inline" data-theme="dark">
      <div className="dialkit-panel" data-mode="inline">
        <div className="dialkit-panel-inner dialkit-panel-inline">
          <section
            className="dialkit-folder dialkit-folder-root"
            data-open="true"
            aria-label={title}
          >
            <div className="dialkit-folder-header dialkit-panel-header">
              <div className="dialkit-folder-header-top">
                <div className="dialkit-folder-title-row">
                  <h2 className="dialkit-folder-title dialkit-folder-title-root">{title}</h2>
                </div>
              </div>
            </div>
            <div className="dialkit-folder-content">
              <div className="dialkit-folder-inner">{children}</div>
            </div>
          </section>
        </div>
      </div>
    </div>
  );
}
