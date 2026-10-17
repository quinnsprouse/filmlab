import { useEffect } from "react";

import { useStore } from "@/hooks/use-store";
import { develop, setOverlay } from "@/lib/develop/session";

import { addMask, deleteMask } from "./masks-commands";
import { masksState, patchBrush, patchMasks } from "./masks-state";

const overlayColor: [number, number, number] = [255, 40, 40];
const overlayOpacity = 50;

function isTyping(target: EventTarget | null): boolean {
  return (
    target instanceof HTMLElement &&
    (target.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName))
  );
}

function activeId(): number | undefined {
  const { photoId, active } = masksState.get();
  return photoId === develop.get().photoId ? active : undefined;
}

function resizeBrush(event: KeyboardEvent): void {
  const { brush } = masksState.get();
  const larger = event.code === "BracketRight";
  if (event.shiftKey) {
    patchBrush({ feather: Math.min(100, Math.max(0, brush.feather + (larger ? 10 : -10))) });
  } else {
    const size = brush.size * (larger ? 1.2 : 1 / 1.2);
    patchBrush({ size: Math.min(0.5, Math.max(0.002, size)) });
  }
}

function onKeyDown(event: KeyboardEvent): void {
  if (isTyping(event.target) || event.metaKey || event.ctrlKey || event.altKey) {
    return;
  }
  const key = event.key.toLowerCase();
  const id = activeId();
  if (event.code === "BracketLeft" || event.code === "BracketRight") {
    event.preventDefault();
    resizeBrush(event);
  } else if (event.shiftKey) {
    return;
  } else if (key === "o") {
    patchMasks({ showOverlay: !masksState.get().showOverlay });
  } else if (key === "b") {
    addMask("brush");
  } else if (key === "l") {
    addMask("linear");
  } else if ((key === "delete" || key === "backspace") && id !== undefined) {
    event.preventDefault();
    deleteMask(id);
  } else if (key === "escape" && masksState.get().interaction.mode !== "adjust") {
    patchMasks({ interaction: { mode: "adjust" } });
  }
}

export function useMaskKeys(): void {
  useEffect(() => {
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);
}

export function useMaskOverlaySync(): void {
  const photoId = useStore(develop, (state) => state.photoId);
  const state = useStore(masksState, (current) => current);
  const active = state.photoId === photoId ? state.active : undefined;
  const picking = state.interaction.mode === "pick";
  const shown = state.showOverlay || state.engaged || state.lumaMap;
  const target = picking ? undefined : (state.hovered ?? (shown ? active : undefined));
  const view = state.lumaMap ? "colorOnBw" : undefined;

  useEffect(() => {
    setOverlay(
      target === undefined
        ? undefined
        : {
            kind: "mask",
            id: target,
            color: overlayColor,
            opacity: overlayOpacity,
            ...(view ? { view } : {}),
          },
    );
  }, [target, view]);

  useEffect(
    () => () => {
      setOverlay(undefined);
    },
    [],
  );
}
