import { nativeSize } from "@/lib/engine/engine";
import { renderPool } from "@/lib/engine/runtime";
import type {
  DevelopSettings,
  DisplayFrame,
  FilmSettings,
  OverlayRequest,
  View,
} from "@/lib/engine/types";
import { lutsOf, renderFilm } from "@/lib/film/film";
import { ensureLuts } from "@/lib/film/stocks";

import { ensureSource, isLoaded, previewEdge, sourceKey, type Level } from "./sources";
import { develop, type DevelopState, type Zoom } from "./state";

const loupeKey = "loupe";
const draftScale = 0.5;
const filmDraftScale = 0.35;
const settleMs = 180;

let viewport = { width: 0, height: 0 };
let inFlight = false;

interface RenderJob {
  photoId: string;
  level: Level;
  settings: DevelopSettings;
  film: FilmSettings | undefined;
  draft: boolean;
  applyCrop: boolean;
  overlay: OverlayRequest | undefined;
  zoom: Zoom;
  native: [number, number];
}

let queued: RenderJob | undefined;
let settleTimer: ReturnType<typeof setTimeout> | undefined;

function shownSettings(state: DevelopState): DevelopSettings | undefined {
  return state.compare === "before" ? state.before : state.settings;
}

function shownFilm(state: DevelopState): FilmSettings | undefined {
  return state.compare === "before" ? undefined : renderFilm(state.film);
}

function wantedLevel(zoom: Zoom, native: [number, number]): Level {
  if (zoom.level !== "fit") {
    return "full";
  }
  const shownLong = Math.max(native[0], native[1]);
  const fitScale = Math.min(viewport.width / native[0], viewport.height / native[1], 1);
  return shownLong * fitScale > previewEdge ? "full" : "preview";
}

interface Plan {
  view: Omit<View, "film" | "overlay">;
  scale: number;
  window: DisplayFrame["window"];
}

function windowPlan(job: RenderJob): Plan {
  const [fullWidth, fullHeight] = job.native;
  const width = Math.min(viewport.width, fullWidth);
  const height = Math.min(viewport.height, fullHeight);
  const x = Math.round(
    Math.min(Math.max(job.zoom.cx * fullWidth - width / 2, 0), fullWidth - width),
  );
  const y = Math.round(
    Math.min(Math.max(job.zoom.cy * fullHeight - height / 2, 0), fullHeight - height),
  );
  return {
    view: {
      maxW: fullWidth,
      maxH: fullHeight,
      draft: job.draft,
      stages: `${loupeKey}-zoom`,
      applyCrop: job.applyCrop,
      window: [x, y, width, height],
    },
    scale: 1,
    window: { x, y, fullWidth, fullHeight },
  };
}

function fitPlan(job: RenderJob): Plan {
  const scale = job.draft ? (job.film ? filmDraftScale : draftScale) : 1;
  return {
    view: {
      maxW: Math.max(1, Math.round(viewport.width * scale)),
      maxH: Math.max(1, Math.round(viewport.height * scale)),
      draft: job.draft,
      stages: loupeKey,
      applyCrop: job.applyCrop,
    },
    scale,
    window: undefined,
  };
}

async function renderJob(job: RenderJob): Promise<DisplayFrame | undefined> {
  const level = isLoaded(job.photoId, job.level) ? job.level : "preview";
  if (level !== job.level) {
    void ensureSource(job.photoId, job.level).then(() => requestRender(false));
  }
  if (!isLoaded(job.photoId, level)) {
    return undefined;
  }
  const plan = job.zoom.level === "fit" || level === "preview" ? fitPlan(job) : windowPlan(job);
  await ensureLuts(lutsOf(job.film));
  const rendered = await renderPool().run("loupe", {
    kind: "render",
    key: sourceKey(job.photoId, level),
    settings: JSON.stringify(job.settings),
    view: {
      ...plan.view,
      ...(job.overlay ? { overlay: job.overlay } : {}),
      ...(job.film ? { film: job.film } : {}),
    },
  });
  return {
    ...rendered,
    width: Math.round(rendered.width / plan.scale),
    height: Math.round(rendered.height / plan.scale),
    pixelWidth: rendered.width,
    pixelHeight: rendered.height,
    ...(plan.window ? { window: plan.window } : {}),
  };
}

async function pump(): Promise<void> {
  const job = queued;
  if (inFlight || !job || viewport.width < 1 || viewport.height < 1) {
    return;
  }
  inFlight = true;
  queued = undefined;
  try {
    const frame = await renderJob(job);
    if (frame && develop.get().photoId === job.photoId) {
      develop.update((state) => ({ ...state, frame }));
    }
  } catch (error) {
    develop.update((state) => ({ ...state, status: "failed", error: String(error) }));
  } finally {
    inFlight = false;
  }
  void pump();
}

export function requestRender(draft: boolean): void {
  const state = develop.get();
  const settings = shownSettings(state);
  if (state.status !== "ready" || !settings || !state.photoId || !state.facts) {
    return;
  }
  const native = nativeSize(state.facts, settings);
  const zoom = state.tool === "edit" ? state.zoom : { level: "fit" as const, cx: 0.5, cy: 0.5 };
  queued = {
    photoId: state.photoId,
    level: wantedLevel(zoom, native),
    settings,
    film: shownFilm(state),
    draft: draft && zoom.level === "fit",
    applyCrop: state.tool !== "crop",
    overlay: state.overlay,
    zoom,
    native,
  };
  void pump();
  clearTimeout(settleTimer);
  if (draft) {
    settleTimer = setTimeout(() => requestRender(false), settleMs);
  }
}

export function setViewport(width: number, height: number): void {
  const next = { width: Math.round(width), height: Math.round(height) };
  if (next.width === viewport.width && next.height === viewport.height) {
    return;
  }
  viewport = next;
  requestRender(false);
}

export function setZoom(zoom: Zoom): void {
  develop.update((state) => ({ ...state, zoom }));
  requestRender(false);
}
