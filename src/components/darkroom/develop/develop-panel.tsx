import { Folder, Toggle } from "dialkit";

import { useStore } from "@/hooks/use-store";
import { sections, type Row } from "@/lib/develop/panel";
import { develop, editSettings } from "@/lib/develop/session";
import { controlValues } from "@/lib/engine/engine";
import type { DevelopSettings, Facts } from "@/lib/engine/types";

import { DustRows } from "./dust-rows";
import { FilmStockRows, GrainRows, HalationRows } from "./film-rows";
import { ControlSlider, Segmented, PanelRoot } from "./panel-controls";
import { GradingRows, MixerRows, WhiteBalanceRows } from "./panel-rows";

function record(settings: DevelopSettings, section: string): Record<string, unknown> {
  const value = settings[section];
  return typeof value === "object" && value !== null ? { ...value } : {};
}

interface RowProps {
  row: Row;
  facts: Facts;
  settings: DevelopSettings;
  values: Record<string, number>;
}

function RowView({ row, facts, settings, values }: RowProps) {
  switch (row.kind) {
    case "slider":
      return <ControlSlider id={row.id} label={row.label} value={values[row.id] ?? 0} />;
    case "toggle": {
      const [section, field] = row.path;
      return (
        <Toggle
          label={row.label}
          checked={record(settings, section)[field] === true}
          onChange={(checked) =>
            editSettings(
              { [section]: { [field]: checked } },
              `${row.label} ${checked ? "on" : "off"}`,
              `${section}.${field}`,
            )
          }
        />
      );
    }
    case "treatment":
      return (
        <Segmented
          label="Treatment"
          value={settings.treatment === "bw" ? "bw" : "color"}
          onChange={(treatment) =>
            editSettings({ treatment }, treatment === "bw" ? "Black & white" : "Color", "treatment")
          }
          options={[
            { value: "color", label: "Color" },
            { value: "bw", label: "B&W" },
          ]}
        />
      );
    case "whiteBalance":
      return <WhiteBalanceRows facts={facts} settings={settings} />;
    case "filmStock":
      return <FilmStockRows />;
    case "halation":
      return <HalationRows />;
    case "grain":
      return <GrainRows />;
    case "dust":
      return <DustRows />;
    case "mixer":
      return <MixerRows values={values} bw={settings.treatment === "bw"} />;
    case "grading":
      return <GradingRows values={values} />;
    case "vignetteStyle": {
      const style = record(settings, "vignette").style;
      return (
        <Segmented
          label="Style"
          value={typeof style === "string" ? style : "highlightPriority"}
          onChange={(next) =>
            editSettings({ vignette: { style: next } }, "Vignette style", "vignette.style")
          }
          options={[
            { value: "highlightPriority", label: "Highlight" },
            { value: "colorPriority", label: "Color" },
            { value: "paintOverlay", label: "Paint" },
          ]}
        />
      );
    }
    case "group":
      return (
        <Folder title={row.title} defaultOpen={false}>
          {row.rows.map((child) => (
            <RowView
              key={keyOf(child)}
              row={child}
              facts={facts}
              settings={settings}
              values={values}
            />
          ))}
        </Folder>
      );
  }
}

function keyOf(row: Row): string {
  switch (row.kind) {
    case "slider":
      return row.id;
    case "toggle":
      return row.path.join(".");
    case "group":
      return row.title;
    case "treatment":
    case "whiteBalance":
    case "mixer":
    case "grading":
    case "vignetteStyle":
    case "filmStock":
    case "halation":
    case "grain":
    case "dust":
      return row.kind;
  }
}

export function DevelopPanel() {
  const facts = useStore(develop, (state) => state.facts);
  const settings = useStore(develop, (state) => state.settings);
  if (!facts || !settings) {
    return null;
  }
  const values = controlValues(settings);
  return (
    <PanelRoot title="Develop">
      {sections.map((section) => (
        <Folder key={section.id} title={section.title} defaultOpen={section.open}>
          {section.rows.map((row) => (
            <RowView key={keyOf(row)} row={row} facts={facts} settings={settings} values={values} />
          ))}
        </Folder>
      ))}
    </PanelRoot>
  );
}
