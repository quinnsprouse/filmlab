import { useEffect, useRef, type PointerEvent, type RefObject } from "react";

import { fitScale, panBy, zoomAround, zoomOf, type Camera } from "@/lib/develop/camera";
import { develop, setZoom, viewer, type ZoomMotion } from "@/lib/develop/session";

const clickSlop = 4;
const pinchSpeed = 0.01;
const largestWheelStep = 60;
const lineHeight = 16;

interface Drag {
  x: number;
  y: number;
  camera: Camera;
  moved: boolean;
}

function devicePoint(
  event: { clientX: number; clientY: number },
  element: Element,
): [number, number] {
  const box = element.getBoundingClientRect();
  const ratio = window.devicePixelRatio;
  return [(event.clientX - box.left) * ratio, (event.clientY - box.top) * ratio];
}

function show(camera: Camera, motion: ZoomMotion): void {
  const { native, viewport } = viewer.get();
  setZoom(zoomOf(camera, native, viewport), motion);
}

function isZoomed(): boolean {
  const { camera, native, viewport } = viewer.get();
  return camera.scale > fitScale(native, viewport) * 1.001;
}

function onWheel(event: WheelEvent, element: Element): void {
  const { camera, native, viewport } = viewer.get();
  const unit = event.deltaMode === WheelEvent.DOM_DELTA_LINE ? lineHeight : 1;
  if (event.ctrlKey || event.metaKey) {
    event.preventDefault();
    const [x, y] = devicePoint(event, element);
    const step = Math.max(-largestWheelStep, Math.min(largestWheelStep, event.deltaY * unit));
    const scale = camera.scale * Math.exp(-step * pinchSpeed);
    show(zoomAround(camera, scale, x, y, native, viewport), { gesture: true });
  } else if (isZoomed()) {
    event.preventDefault();
    const ratio = window.devicePixelRatio * unit;
    show(panBy(camera, -event.deltaX * ratio, -event.deltaY * ratio, native, viewport), {
      gesture: true,
    });
  }
}

export function useCamera(area: RefObject<HTMLDivElement | null>, enabled: boolean) {
  const drag = useRef<Drag | undefined>(undefined);

  useEffect(() => {
    const element = area.current;
    if (!element || !enabled) {
      return;
    }
    const listener = (event: WheelEvent) => onWheel(event, element);
    element.addEventListener("wheel", listener, { passive: false });
    return () => element.removeEventListener("wheel", listener);
  }, [area, enabled]);

  const onPointerDown = (event: PointerEvent<HTMLDivElement>) => {
    if (event.button !== 0 || develop.get().tool !== "edit") {
      return;
    }
    event.currentTarget.setPointerCapture(event.pointerId);
    drag.current = {
      x: event.clientX,
      y: event.clientY,
      camera: viewer.get().camera,
      moved: false,
    };
  };

  const onPointerMove = (event: PointerEvent<HTMLDivElement>) => {
    const current = drag.current;
    if (!current || !isZoomed()) {
      return;
    }
    const dx = event.clientX - current.x;
    const dy = event.clientY - current.y;
    if (!current.moved && Math.hypot(dx, dy) < clickSlop) {
      return;
    }
    current.moved = true;
    const { native, viewport } = viewer.get();
    const ratio = window.devicePixelRatio;
    show(panBy(current.camera, dx * ratio, dy * ratio, native, viewport), { gesture: true });
  };

  const onPointerUp = (event: PointerEvent<HTMLDivElement>) => {
    const current = drag.current;
    drag.current = undefined;
    if (!current || current.moved) {
      return;
    }
    if (isZoomed()) {
      setZoom({ level: "fit", cx: 0.5, cy: 0.5 }, { animate: true });
      return;
    }
    const { camera, native, viewport } = viewer.get();
    const [x, y] = devicePoint(event, event.currentTarget);
    show(zoomAround(camera, 1, x, y, native, viewport), { animate: true });
  };

  return { onPointerDown, onPointerMove, onPointerUp };
}
