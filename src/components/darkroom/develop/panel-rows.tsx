import { DialPad, SelectControl } from "dialkit";
import { useState } from "react";

import { gradingRegions, mixerBands } from "@/lib/develop/panel";
import { editControls, editWhiteBalanceMode } from "@/lib/develop/session";
import { whiteBalance, type WhiteBalanceMode } from "@/lib/engine/engine";
import type { DevelopSettings, Facts } from "@/lib/engine/types";

import { ControlSlider, Segmented, sentence } from "./panel-controls";

const whiteBalanceModes: { value: WhiteBalanceMode; label: string }[] = [
  { value: "asShot", label: "As shot" },
  { value: "auto", label: "Auto" },
  { value: "daylight", label: "Daylight" },
  { value: "cloudy", label: "Cloudy" },
  { value: "shade", label: "Shade" },
  { value: "tungsten", label: "Tungsten" },
  { value: "fluorescent", label: "Fluorescent" },
  { value: "flash", label: "Flash" },
  { value: "custom", label: "Custom" },
];

export function WhiteBalanceRows({ facts, settings }: { facts: Facts; settings: DevelopSettings }) {
  const view = whiteBalance(facts, settings);
  const temp = view.kelvin ? { min: 2000, max: 12000, step: 50 } : { min: -100, max: 100, step: 1 };
  const tint = view.kelvin ? { min: -150, max: 150, step: 1 } : { min: -100, max: 100, step: 1 };
  return (
    <>
      <SelectControl
        label="White balance"
        value={view.mode}
        options={whiteBalanceModes.filter(
          (mode) => mode.value !== "custom" || view.mode === "custom",
        )}
        onChange={(mode) => {
          const chosen = whiteBalanceModes.find((option) => option.value === mode);
          if (chosen) {
            editWhiteBalanceMode(chosen.value, `White balance ${chosen.label}`);
          }
        }}
      />
      <ControlSlider
        id="wb.temp"
        label="Temp"
        value={view.temp}
        range={temp}
        onChange={(next) => editControls({ "wb.temp": next }, `Temp ${Math.round(next)}`)}
        onReset={() => editWhiteBalanceMode("asShot", "White balance As shot")}
      />
      <ControlSlider
        id="wb.tint"
        label="Tint"
        value={view.tint}
        range={tint}
        onChange={(next) => editControls({ "wb.tint": next }, `Tint ${Math.round(next)}`)}
        onReset={() => editWhiteBalanceMode("asShot", "White balance As shot")}
      />
    </>
  );
}

type MixerChannel = "hue" | "sat" | "lum";

export function MixerRows({ values, bw }: { values: Record<string, number>; bw: boolean }) {
  const [channel, setChannel] = useState<MixerChannel>("hue");
  if (bw) {
    return mixerBands.map((band) => (
      <ControlSlider
        key={band}
        id={`bw.${band}`}
        label={sentence(band)}
        value={values[`bw.${band}`] ?? 0}
      />
    ));
  }
  return (
    <>
      <Segmented
        label="Adjust"
        value={channel}
        onChange={setChannel}
        options={[
          { value: "hue", label: "Hue" },
          { value: "sat", label: "Saturation" },
          { value: "lum", label: "Luminance" },
        ]}
      />
      {mixerBands.map((band) => {
        const id = `mixer.${band}.${channel}`;
        return <ControlSlider key={id} id={id} label={sentence(band)} value={values[id] ?? 0} />;
      })}
    </>
  );
}

type Region = (typeof gradingRegions)[number];

export function GradingRows({ values }: { values: Record<string, number> }) {
  const [region, setRegion] = useState<Region>("midtones");
  const hue = values[`grading.${region}.hue`] ?? 0;
  const sat = values[`grading.${region}.sat`] ?? 0;
  const radians = (hue * Math.PI) / 180;
  const pad = { x: (sat / 100) * Math.cos(radians), y: (sat / 100) * Math.sin(radians) };
  const regionLabel = sentence(region);
  return (
    <>
      <Segmented
        label="Adjust"
        value={region}
        onChange={setRegion}
        options={gradingRegions.map((value) => ({ value, label: sentence(value) }))}
      />
      <DialPad
        label={`${regionLabel} color`}
        value={pad}
        onChange={({ x, y }) => {
          const nextSat = Math.min(100, Math.round(Math.hypot(x, y) * 100));
          const nextHue = Math.round(((Math.atan2(y, x) * 180) / Math.PI + 360) % 360);
          editControls(
            { [`grading.${region}.hue`]: nextHue, [`grading.${region}.sat`]: nextSat },
            `${regionLabel} color ${nextHue}° ${nextSat}`,
          );
        }}
      />
      <ControlSlider
        id={`grading.${region}.lum`}
        label="Luminance"
        value={values[`grading.${region}.lum`] ?? 0}
      />
      <ControlSlider
        id="grading.blending"
        label="Blending"
        value={values["grading.blending"] ?? 50}
      />
      <ControlSlider id="grading.balance" label="Balance" value={values["grading.balance"] ?? 0} />
    </>
  );
}
