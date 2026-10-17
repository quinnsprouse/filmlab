import { useRef, type PointerEvent } from "react";

import { develop, setZoom } from "@/lib/develop/session";

const clickSlop = 4;

interface Drag {
  startX: number;
  startY: number;
  cx: number;
  cy: number;
  moved: boolean;
}

function clamp(value: number): number {
  return Math.min(1, Math.max(0, value));
}

export function useZoomPointer() {
  const drag = useRef<Drag | undefined>(undefined);

  const onPointerDown = (event: PointerEvent<HTMLCanvasElement>) => {
    const { tool, zoom } = develop.get();
    if (event.button !== 0 || tool !== "edit") {
      return;
    }
    event.currentTarget.setPointerCapture(event.pointerId);
    drag.current = {
      startX: event.clientX,
      startY: event.clientY,
      cx: zoom.cx,
      cy: zoom.cy,
      moved: false,
    };
  };

  const onPointerMove = (event: PointerEvent<HTMLCanvasElement>) => {
    const current = drag.current;
    const { frame, zoom } = develop.get();
    if (!current || zoom.level === "fit" || !frame?.window) {
      return;
    }
    const dx = event.clientX - current.startX;
    const dy = event.clientY - current.startY;
    if (!current.moved && Math.hypot(dx, dy) < clickSlop) {
      return;
    }
    current.moved = true;
    const scale = window.devicePixelRatio;
    setZoom({
      level: zoom.level,
      cx: clamp(current.cx - (dx * scale) / frame.window.fullWidth),
      cy: clamp(current.cy - (dy * scale) / frame.window.fullHeight),
    });
  };

  const onPointerUp = (event: PointerEvent<HTMLCanvasElement>) => {
    const current = drag.current;
    drag.current = undefined;
    const { frame, zoom } = develop.get();
    if (!current || current.moved || !frame) {
      return;
    }
    if (zoom.level !== "fit") {
      setZoom({ level: "fit", cx: zoom.cx, cy: zoom.cy });
      return;
    }
    const box = event.currentTarget.getBoundingClientRect();
    setZoom({
      level: 1,
      cx: clamp((event.clientX - box.left) / box.width),
      cy: clamp((event.clientY - box.top) / box.height),
    });
  };

  return { onPointerDown, onPointerMove, onPointerUp };
}
