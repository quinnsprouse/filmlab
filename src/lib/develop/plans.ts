import { nativeSize } from "@/lib/engine/engine";
import type { DevelopSettings, FilmSettings, OverlayRequest, View } from "@/lib/engine/types";
import { renderFilm } from "@/lib/film/film";

import { cameraOf, fitScale, wholeFrame, type Camera, type Region, type Size } from "./camera";
import { previewEdge, type Level } from "./sources";
import type { DevelopState, Zoom } from "./state";

const loupeKey = "loupe";
const detailMargin = 64;

export type Layer = "fit" | "detail";

export interface Plan {
  layer: Layer;
  photoId: string;
  level: Level;
  settings: DevelopSettings;
  look: string;
  film: FilmSettings | undefined;
  overlay: OverlayRequest | undefined;
  view: Omit<View, "film" | "overlay">;
  region: Region;
}

function shownSettings(state: DevelopState): DevelopSettings | undefined {
  return state.compare === "before" ? state.before : state.settings;
}

function shownFilm(state: DevelopState): FilmSettings | undefined {
  return state.compare === "before" ? undefined : renderFilm(state.film);
}

export function targetZoom(state: DevelopState): Zoom {
  return state.tool === "edit" ? state.zoom : { level: "fit", cx: 0.5, cy: 0.5 };
}

function levelFor(shownLong: number): Level {
  return shownLong > previewEdge ? "full" : "preview";
}

interface Inputs {
  photoId: string;
  settings: DevelopSettings;
  film: FilmSettings | undefined;
  overlay: OverlayRequest | undefined;
  applyCrop: boolean;
  look: string;
  native: [number, number];
}

export function inputsOf(state: DevelopState): Inputs | undefined {
  const settings = shownSettings(state);
  if (state.status !== "ready" || !settings || !state.photoId || !state.facts) {
    return undefined;
  }
  const film = shownFilm(state);
  const applyCrop = state.tool !== "crop";
  return {
    photoId: state.photoId,
    settings,
    film,
    overlay: state.overlay,
    applyCrop,
    look: JSON.stringify([film, state.overlay, applyCrop]),
    native: nativeSize(state.facts, settings),
  };
}

function fitPlan(inputs: Inputs, viewport: Size): Plan {
  const fit = fitScale(inputs.native, viewport);
  return {
    layer: "fit",
    photoId: inputs.photoId,
    level: levelFor(Math.max(inputs.native[0], inputs.native[1]) * fit),
    settings: inputs.settings,
    look: inputs.look,
    film: inputs.film,
    overlay: inputs.overlay,
    view: {
      maxW: Math.max(1, Math.round(viewport.width)),
      maxH: Math.max(1, Math.round(viewport.height)),
      stages: loupeKey,
      applyCrop: inputs.applyCrop,
    },
    region: wholeFrame,
  };
}

function visibleRegion(camera: Camera, native: [number, number], viewport: Size): Region {
  const halfW = viewport.width / (2 * native[0] * camera.scale);
  const halfH = viewport.height / (2 * native[1] * camera.scale);
  return {
    u0: Math.max(0, camera.cx - halfW),
    v0: Math.max(0, camera.cy - halfH),
    u1: Math.min(1, camera.cx + halfW),
    v1: Math.min(1, camera.cy + halfH),
  };
}

function covers(outer: Region, inner: Region): boolean {
  const slack = 1e-6;
  return (
    outer.u0 <= inner.u0 + slack &&
    outer.v0 <= inner.v0 + slack &&
    outer.u1 >= inner.u1 - slack &&
    outer.v1 >= inner.v1 - slack
  );
}

function renderScale(camera: Camera): number {
  return Math.min(camera.scale, 1);
}

function detailPlan(inputs: Inputs, camera: Camera, viewport: Size): Plan {
  const scale = renderScale(camera);
  const fullWidth = Math.max(1, Math.round(inputs.native[0] * scale));
  const fullHeight = Math.max(1, Math.round(inputs.native[1] * scale));
  const width = Math.min(
    Math.ceil((viewport.width * scale) / camera.scale) + 2 * detailMargin,
    fullWidth,
  );
  const height = Math.min(
    Math.ceil((viewport.height * scale) / camera.scale) + 2 * detailMargin,
    fullHeight,
  );
  const x = Math.round(Math.min(Math.max(camera.cx * fullWidth - width / 2, 0), fullWidth - width));
  const y = Math.round(
    Math.min(Math.max(camera.cy * fullHeight - height / 2, 0), fullHeight - height),
  );
  return {
    layer: "detail",
    photoId: inputs.photoId,
    level: levelFor(Math.max(fullWidth, fullHeight)),
    settings: inputs.settings,
    look: inputs.look,
    film: inputs.film,
    overlay: inputs.overlay,
    view: {
      maxW: fullWidth,
      maxH: fullHeight,
      stages: `${loupeKey}-zoom`,
      applyCrop: inputs.applyCrop,
      window: [x, y, width, height],
    },
    region: {
      u0: x / fullWidth,
      v0: y / fullHeight,
      u1: (x + width) / fullWidth,
      v1: (y + height) / fullHeight,
    },
  };
}

function sameLook(a: Plan | undefined, b: Plan): a is Plan {
  return (
    a !== undefined && a.photoId === b.photoId && a.settings === b.settings && a.look === b.look
  );
}

export function samePlan(a: Plan | undefined, b: Plan): boolean {
  return sameLook(a, b) && a.level === b.level && JSON.stringify(a.view) === JSON.stringify(b.view);
}

function stillCovers(
  previous: Plan | undefined,
  wanted: Plan,
  camera: Camera,
  native: [number, number],
  viewport: Size,
): previous is Plan {
  return (
    sameLook(previous, wanted) &&
    previous.view.maxW === wanted.view.maxW &&
    covers(previous.region, visibleRegion(camera, native, viewport))
  );
}

export function plans(state: DevelopState, viewport: Size, shownDetail: Plan | undefined): Plan[] {
  const inputs = inputsOf(state);
  if (!inputs || viewport.width < 1 || viewport.height < 1) {
    return [];
  }
  const fit = fitPlan(inputs, viewport);
  const camera = cameraOf(targetZoom(state), inputs.native, viewport);
  if (camera.scale <= fitScale(inputs.native, viewport) * 1.001) {
    return [fit];
  }
  const detail = detailPlan(inputs, camera, viewport);
  return [
    stillCovers(shownDetail, detail, camera, inputs.native, viewport) ? shownDetail : detail,
    fit,
  ];
}
