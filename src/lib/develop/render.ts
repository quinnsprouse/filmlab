import { renderPool } from "@/lib/engine/runtime";
import { lutsOf } from "@/lib/film/film";
import { ensureLuts, lutsResident } from "@/lib/film/stocks";
import { createStore } from "@/lib/store";

import { cameraOf, mixCamerasLogScale, wholeFrame, type Camera, type Size } from "./camera";
import { inputsOf, plans, samePlan, targetZoom, type Layer, type Plan } from "./plans";
import { ensureSource, isLoaded, sourceKey } from "./sources";
import { develop, type DevelopState, type DisplayFrame, type Zoom } from "./state";

const gestureSettleMs = 90;
const zoomAnimationMs = 220;

export interface Viewer {
  camera: Camera;
  native: [number, number];
  viewport: Size;
}

export const viewer = createStore<Viewer>({
  camera: { scale: 1, cx: 0.5, cy: 0.5 },
  native: [1, 1],
  viewport: { width: 0, height: 0 },
});

const shown: Record<Layer, Plan | undefined> = { fit: undefined, detail: undefined };
let inFlight = false;
let lastRequest = 0;
let settleTimer: ReturnType<typeof setTimeout> | undefined;
let animation: number | undefined;

function runnable(plan: Plan): Plan | undefined {
  if (isLoaded(plan.photoId, plan.level)) {
    return plan;
  }
  void ensureSource(plan.photoId, plan.level).then(() => pump());
  if (plan.layer === "fit" && isLoaded(plan.photoId, "preview")) {
    return { ...plan, level: "preview" };
  }
  return undefined;
}

async function renderPlan(plan: Plan): Promise<DisplayFrame> {
  const luts = lutsOf(plan.film);
  if (!lutsResident(luts)) {
    await ensureLuts(luts);
  }
  const rendered = await renderPool().run("loupe", {
    kind: "render",
    key: sourceKey(plan.photoId, plan.level),
    settings: JSON.stringify(plan.settings),
    view: {
      ...plan.view,
      ...(plan.overlay ? { overlay: plan.overlay } : {}),
      ...(plan.film ? { film: plan.film } : {}),
    },
  });
  if (plan.layer === "fit") {
    return { ...rendered, region: wholeFrame };
  }
  const [, , width, height] = plan.view.window ?? [0, 0, rendered.width, rendered.height];
  const { u0, v0, u1, v1 } = plan.region;
  return {
    ...rendered,
    region: {
      u0,
      v0,
      u1: u0 + ((u1 - u0) * rendered.width) / width,
      v1: v0 + ((v1 - v0) * rendered.height) / height,
    },
  };
}

function nextPlan(state: DevelopState): Plan | undefined {
  for (const wanted of plans(state, viewer.get().viewport, shown.detail)) {
    if (samePlan(shown[wanted.layer], wanted)) {
      continue;
    }
    const plan = runnable(wanted);
    if (plan && !samePlan(shown[plan.layer], plan)) {
      return plan;
    }
  }
  return undefined;
}

function dropStaleDetail(state: DevelopState): void {
  const inputs = inputsOf(state);
  const stale =
    state.detail !== undefined &&
    (!inputs ||
      shown.detail?.photoId !== inputs.photoId ||
      shown.detail.settings !== inputs.settings ||
      shown.detail.look !== inputs.look);
  if (stale && targetZoom(state).level === "fit") {
    shown.detail = undefined;
    develop.update((current) => ({ ...current, detail: undefined }));
  }
}

async function pump(): Promise<void> {
  if (inFlight || settleTimer !== undefined) {
    return;
  }
  const state = develop.get();
  dropStaleDetail(state);
  const plan = nextPlan(state);
  if (!plan) {
    return;
  }
  inFlight = true;
  try {
    const frame = await renderPlan(plan);
    if (develop.get().photoId === plan.photoId) {
      shown[plan.layer] = plan;
      const replaced = plan.layer === "fit" ? develop.get().frame : develop.get().detail;
      develop.update((current) =>
        plan.layer === "fit" ? { ...current, frame } : { ...current, detail: frame },
      );
      replaced?.bitmap.close();
    } else {
      frame.bitmap.close();
    }
  } catch (error) {
    develop.update((current) => ({ ...current, status: "failed", error: String(error) }));
  } finally {
    inFlight = false;
  }
  void pump();
}

function syncViewer(): void {
  if (animation !== undefined) {
    return;
  }
  const state = develop.get();
  const inputs = inputsOf(state);
  if (!inputs) {
    return;
  }
  viewer.update((current) => ({
    ...current,
    native: inputs.native,
    camera: cameraOf(targetZoom(state), inputs.native, current.viewport),
  }));
}

export function idleFor(ms: number): boolean {
  return !inFlight && performance.now() - lastRequest >= ms;
}

export function requestRender(): void {
  lastRequest = performance.now();
  if (!develop.get().photoId) {
    shown.fit = undefined;
    shown.detail = undefined;
  }
  syncViewer();
  void pump();
}

export function setViewport(width: number, height: number): void {
  const next = { width: Math.round(width), height: Math.round(height) };
  const { viewport } = viewer.get();
  if (next.width === viewport.width && next.height === viewport.height) {
    return;
  }
  viewer.update((current) => ({ ...current, viewport: next }));
  requestRender();
}

function prefersReducedMotion(): boolean {
  return window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

function easeOut(t: number): number {
  return 1 - (1 - t) ** 3;
}

function animateTo(target: Camera): void {
  if (animation !== undefined) {
    cancelAnimationFrame(animation);
    animation = undefined;
  }
  if (prefersReducedMotion()) {
    viewer.update((current) => ({ ...current, camera: target }));
    return;
  }
  const from = viewer.get().camera;
  const started = performance.now();
  const step = (now: number) => {
    const t = Math.min((now - started) / zoomAnimationMs, 1);
    viewer.update((current) => ({
      ...current,
      camera: mixCamerasLogScale(from, target, easeOut(t)),
    }));
    animation = t < 1 ? requestAnimationFrame(step) : undefined;
  };
  animation = requestAnimationFrame(step);
}

export interface ZoomMotion {
  animate?: boolean;
  gesture?: boolean;
}

export function setZoom(zoom: Zoom, { animate = false, gesture = false }: ZoomMotion = {}): void {
  lastRequest = performance.now();
  develop.update((state) => ({ ...state, zoom }));
  const { native, viewport } = viewer.get();
  const target = cameraOf(zoom, native, viewport);
  if (animate) {
    animateTo(target);
  } else {
    if (animation !== undefined) {
      cancelAnimationFrame(animation);
      animation = undefined;
    }
    viewer.update((current) => ({ ...current, camera: target }));
  }
  clearTimeout(settleTimer);
  settleTimer = undefined;
  if (gesture) {
    settleTimer = setTimeout(() => {
      settleTimer = undefined;
      void pump();
    }, gestureSettleMs);
    return;
  }
  void pump();
}
