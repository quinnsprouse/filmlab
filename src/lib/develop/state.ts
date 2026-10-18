import type {
  BitmapFrame,
  DevelopSettings,
  Facts,
  FilmSettings,
  OverlayRequest,
  PhotoState,
} from "@/lib/engine/types";
import { createStore } from "@/lib/store";

import type { Region } from "./camera";

interface HistoryStep {
  label: string;
  key: string;
  settings: DevelopSettings;
  film: FilmSettings | undefined;
  at: number;
}

export type Compare = "after" | "before";

export type Tool = "edit" | "crop" | "heal" | "masks";

export interface Zoom {
  level: "fit" | number;
  cx: number;
  cy: number;
}

export interface DisplayFrame extends BitmapFrame {
  region: Region;
}

export interface DevelopState {
  photoId: string | undefined;
  facts: Facts | undefined;
  base: PhotoState | undefined;
  before: DevelopSettings | undefined;
  settings: DevelopSettings | undefined;
  film: FilmSettings | undefined;
  history: HistoryStep[];
  cursor: number;
  frame: DisplayFrame | undefined;
  detail: DisplayFrame | undefined;
  status: "idle" | "loading" | "ready" | "failed";
  error: string | undefined;
  compare: Compare;
  tool: Tool;
  zoom: Zoom;
  overlay: OverlayRequest | undefined;
  applied: { profileId: string; amount: number; before: DevelopSettings; step: number } | undefined;
}

export const emptyDevelop: DevelopState = {
  photoId: undefined,
  facts: undefined,
  base: undefined,
  before: undefined,
  settings: undefined,
  film: undefined,
  history: [],
  cursor: 0,
  frame: undefined,
  detail: undefined,
  status: "idle",
  error: undefined,
  compare: "after",
  tool: "edit",
  zoom: { level: "fit", cx: 0.5, cy: 0.5 },
  overlay: undefined,
  applied: undefined,
};

export const develop = createStore<DevelopState>(emptyDevelop);
