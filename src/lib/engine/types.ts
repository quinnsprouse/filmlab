export type DevelopSettings = Record<string, unknown>;

export type Flag = "none" | "pick" | "reject";

interface PhotoMeta {
  camera: string;
  lens: string;
  focal_mm: number | null;
  aperture: number | null;
  shutter: string;
  iso: number | null;
}

export interface Facts {
  name: string;
  format: string;
  kind: "image" | "raw" | "video";
  width: number;
  height: number;
  captured: string | null;
  meta: PhotoMeta;
  asShotWb: [number, number] | null;
  embeddedLens: unknown;
  previewOnly: string | null;
}

export interface Probed {
  facts: Facts;
  defaults: DevelopSettings;
  embeddedXmp: string | null;
}

export interface FilmSettings {
  negative: string | null;
  print: string | null;
  utility: string | null;
  halation: { amount: number; radius: number; threshold: number };
  grain: { amount: number; size: number; color: number };
}

export interface PhotoState {
  settings: DevelopSettings;
  rating: number;
  flag: Flag;
  label: string | null;
  film?: FilmSettings;
}

type MaskView = "color" | "colorOnBw" | "imageOnBlack" | "imageOnWhite" | "whiteOnBlack";

export type OverlayRequest =
  | { kind: "mask"; id: number; view?: MaskView; color: [number, number, number]; opacity: number }
  | { kind: "allMasks"; view?: MaskView; color: [number, number, number]; opacity: number }
  | { kind: "spots"; threshold: number };

export interface View {
  maxW: number;
  maxH: number;
  draft?: boolean;
  applyCrop?: boolean;
  stages?: string;
  window?: [number, number, number, number];
  overlay?: OverlayRequest;
  film?: FilmSettings;
}

export interface RenderedFrame {
  width: number;
  height: number;
  rgba: Uint8ClampedArray<ArrayBuffer>;
  histogram: Uint32Array<ArrayBuffer>;
}

export interface DisplayFrame extends RenderedFrame {
  pixelWidth: number;
  pixelHeight: number;
  window?: { x: number; y: number; fullWidth: number; fullHeight: number };
}

export interface ControlSpec {
  id: string;
  label: string;
  section: string;
  min: number;
  max: number;
  default: number;
  step: number;
  decimals: number;
}

export type ExportFormat = "jpeg" | "png" | "tiff";

export const exportFormats: Record<ExportFormat, { extension: string; mime: string; label: string }> = {
  jpeg: { extension: "jpg", mime: "image/jpeg", label: "JPEG" },
  png: { extension: "png", mime: "image/png", label: "PNG" },
  tiff: { extension: "tif", mime: "image/tiff", label: "TIFF" },
};

export interface ExportOptions {
  format: ExportFormat;
  quality: number;
  resize?: { mode: "longEdge"; value: number };
}

export interface ExportPlan {
  width: number;
  height: number;
  maxEdge: number;
}
