import type { DevelopSettings } from "@/lib/engine/types";

export interface Pt {
  x: number;
  y: number;
}

export type MaskOp = "add" | "subtract" | "intersect";

export type Shape =
  | { kind: "linear"; start: Pt; end: Pt }
  | {
      kind: "radial";
      center: Pt;
      rx: number;
      ry: number;
      angle: number;
      feather: number;
      invert: boolean;
    }
  | { kind: "brush"; strokes: number }
  | {
      kind: "luminanceRange";
      lo: number;
      hi: number;
      loFeather: number;
      hiFeather: number;
    }
  | { kind: "colorRange"; samples: [number, number, number][]; refine: number }
  | { kind: "subject" | "sky" | "background" }
  | { kind: "other"; name: string };

interface Component {
  op: MaskOp;
  invert: boolean;
  shape: Shape;
}

export interface Mask {
  id: number;
  name: string;
  visible: boolean;
  invert: boolean;
  refine: number;
  components: Component[];
  adjust: Record<string, number>;
}

export type CreateKind =
  | "linear"
  | "radial"
  | "brush"
  | "luminanceRange"
  | "colorRange"
  | "subject"
  | "sky"
  | "background";

export const kindLabels: Record<CreateKind, string> = {
  linear: "Linear gradient",
  radial: "Radial gradient",
  brush: "Brush",
  luminanceRange: "Luminance range",
  colorRange: "Color range",
  subject: "Subject",
  sky: "Sky",
  background: "Background",
};

function shapeLabel(shape: Shape): string {
  return shape.kind === "other" ? shape.name : kindLabels[shape.kind];
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function num(value: unknown, fallback: number): number {
  return typeof value === "number" && Number.isFinite(value) ? value : fallback;
}

function pt(value: unknown, fallback: Pt): Pt {
  return isRecord(value) ? { x: num(value.x, fallback.x), y: num(value.y, fallback.y) } : fallback;
}

function triple(value: unknown): [number, number, number] | undefined {
  if (!Array.isArray(value) || value.length < 3) {
    return undefined;
  }
  return [num(value[0], 0), num(value[1], 0), num(value[2], 0)];
}

function samples(value: unknown): [number, number, number][] {
  if (!Array.isArray(value)) {
    return [];
  }
  return value.map(triple).filter((sample) => sample !== undefined);
}

const origin: Pt = { x: 0.5, y: 0.5 };

function parseShape(value: unknown): Shape {
  if (!isRecord(value)) {
    return { kind: "other", name: "Component" };
  }
  switch (value.kind) {
    case "linear":
      return {
        kind: "linear",
        start: pt(value.start, { x: 0.5, y: 0.25 }),
        end: pt(value.end, { x: 0.5, y: 0.6 }),
      };
    case "radial":
      return {
        kind: "radial",
        center: pt(value.center, origin),
        rx: num(value.rx, 0.22),
        ry: num(value.ry, 0.16),
        angle: num(value.angle, 0),
        feather: num(value.feather, 50),
        invert: value.invert === true,
      };
    case "brush":
      return { kind: "brush", strokes: Array.isArray(value.strokes) ? value.strokes.length : 0 };
    case "luminanceRange":
      return {
        kind: "luminanceRange",
        lo: num(value.lo, 0.6),
        hi: num(value.hi, 1),
        loFeather: num(value.lo_feather, 0.1),
        hiFeather: num(value.hi_feather, 0.1),
      };
    case "colorRange":
      return {
        kind: "colorRange",
        samples: samples(value.samples),
        refine: num(value.refine, 50),
      };
    case "subject":
    case "sky":
    case "background":
      return { kind: value.kind };
    case "object":
      return { kind: "other", name: "Object" };
    case "prompt":
      return { kind: "other", name: "Described object" };
    case "depthRange":
      return { kind: "other", name: "Depth range" };
    case "people":
      return { kind: "other", name: "People" };
    case "landscape":
      return { kind: "other", name: "Landscape" };
    default:
      return { kind: "other", name: "Component" };
  }
}

function parseOp(value: unknown): MaskOp {
  return value === "subtract" || value === "intersect" ? value : "add";
}

function parseAdjust(value: unknown): Record<string, number> {
  const adjust: Record<string, number> = {};
  if (isRecord(value)) {
    for (const [key, entry] of Object.entries(value)) {
      adjust[key] = num(entry, 0);
    }
  }
  return adjust;
}

function parseMask(value: unknown): Mask | undefined {
  if (!isRecord(value) || typeof value.id !== "number") {
    return undefined;
  }
  const components = Array.isArray(value.components) ? value.components : [];
  return {
    id: value.id,
    name: typeof value.name === "string" ? value.name : `Mask ${value.id}`,
    visible: value.visible !== false,
    invert: value.invert === true,
    refine: num(value.refine, 0),
    adjust: parseAdjust(value.adjust),
    components: components.filter(isRecord).map((component) => ({
      op: parseOp(component.op),
      invert: component.invert === true,
      shape: parseShape(component.shape),
    })),
  };
}

export function readMasks(settings: DevelopSettings | undefined): Mask[] {
  const raw = settings?.masks;
  return Array.isArray(raw) ? raw.map(parseMask).filter((mask) => mask !== undefined) : [];
}

export function shapeParams(shape: Shape): Record<string, unknown> {
  if (shape.kind === "luminanceRange") {
    return {
      kind: shape.kind,
      lo: shape.lo,
      hi: shape.hi,
      lo_feather: shape.loFeather,
      hi_feather: shape.hiFeather,
    };
  }
  return { ...shape };
}

export function componentName(mask: Mask, index: number): string {
  const component = mask.components[index];
  return component ? shapeLabel(component.shape) : "Component";
}
