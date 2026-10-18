import type { Zoom } from "./state";

export interface Size {
  width: number;
  height: number;
}

export interface Camera {
  scale: number;
  cx: number;
  cy: number;
}

export interface Region {
  u0: number;
  v0: number;
  u1: number;
  v1: number;
}

export interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

const maxScale = 4;

export const wholeFrame: Region = { u0: 0, v0: 0, u1: 1, v1: 1 };

export function fitScale(native: [number, number], viewport: Size): number {
  return Math.min(viewport.width / native[0], viewport.height / native[1], 1);
}

function clampAxis(centre: number, shown: number, extent: number): number {
  const half = extent / (2 * shown);
  return half >= 0.5 ? 0.5 : Math.min(Math.max(centre, half), 1 - half);
}

function clampCamera(camera: Camera, native: [number, number], viewport: Size): Camera {
  const scale = Math.min(Math.max(camera.scale, fitScale(native, viewport)), maxScale);
  return {
    scale,
    cx: clampAxis(camera.cx, native[0] * scale, viewport.width),
    cy: clampAxis(camera.cy, native[1] * scale, viewport.height),
  };
}

export function cameraOf(zoom: Zoom, native: [number, number], viewport: Size): Camera {
  if (zoom.level === "fit") {
    return { scale: fitScale(native, viewport), cx: 0.5, cy: 0.5 };
  }
  return clampCamera({ scale: zoom.level, cx: zoom.cx, cy: zoom.cy }, native, viewport);
}

export function zoomOf(camera: Camera, native: [number, number], viewport: Size): Zoom {
  const fit = fitScale(native, viewport);
  return camera.scale <= fit * 1.001
    ? { level: "fit", cx: 0.5, cy: 0.5 }
    : { level: camera.scale, cx: camera.cx, cy: camera.cy };
}

export function rectOf(
  camera: Camera,
  region: Region,
  native: [number, number],
  viewport: Size,
): Rect {
  const [w, h] = native;
  const x = viewport.width / 2 + (region.u0 - camera.cx) * w * camera.scale;
  const y = viewport.height / 2 + (region.v0 - camera.cy) * h * camera.scale;
  return {
    x,
    y,
    width: (region.u1 - region.u0) * w * camera.scale,
    height: (region.v1 - region.v0) * h * camera.scale,
  };
}

function pointAt(
  camera: Camera,
  x: number,
  y: number,
  native: [number, number],
  viewport: Size,
): [number, number] {
  return [
    camera.cx + (x - viewport.width / 2) / (native[0] * camera.scale),
    camera.cy + (y - viewport.height / 2) / (native[1] * camera.scale),
  ];
}

export function zoomAround(
  camera: Camera,
  scale: number,
  x: number,
  y: number,
  native: [number, number],
  viewport: Size,
): Camera {
  const [u, v] = pointAt(camera, x, y, native, viewport);
  const next = Math.min(Math.max(scale, fitScale(native, viewport)), maxScale);
  return clampCamera(
    {
      scale: next,
      cx: u - (x - viewport.width / 2) / (native[0] * next),
      cy: v - (y - viewport.height / 2) / (native[1] * next),
    },
    native,
    viewport,
  );
}

export function panBy(
  camera: Camera,
  dx: number,
  dy: number,
  native: [number, number],
  viewport: Size,
): Camera {
  return clampCamera(
    {
      scale: camera.scale,
      cx: camera.cx - dx / (native[0] * camera.scale),
      cy: camera.cy - dy / (native[1] * camera.scale),
    },
    native,
    viewport,
  );
}

export function mixCamerasLogScale(from: Camera, to: Camera, t: number): Camera {
  const scale = Math.exp(Math.log(from.scale) + (Math.log(to.scale) - Math.log(from.scale)) * t);
  return { scale, cx: from.cx + (to.cx - from.cx) * t, cy: from.cy + (to.cy - from.cy) * t };
}
