import { useEffect, type RefObject } from "react";

import type { Flag } from "@/lib/engine/types";
import { ui } from "@/lib/ui";

import {
  actionTargets,
  clearSelection,
  flagPhotos,
  openInDevelop,
  moveFocusTo,
  selectAll,
  stepFocus,
} from "./selection";

const flagKeys: Record<string, Flag> = { p: "pick", x: "reject", u: "none" };
const arrowKeys = new Set(["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"]);
const ownedSelector =
  "input, textarea, select, [contenteditable], [role='slider'], [role='menu'], [role='dialog'], [role='listbox']";
const pressableSelector = "button:not([data-photo-id]), a, summary, [role='button']";

function ignoresKeys(target: EventTarget | null): boolean {
  return target instanceof Element && target.closest(ownedSelector) !== null;
}

function openTarget(): string | undefined {
  const { focused, selection } = ui.get();
  return focused ?? selection[0];
}

function handleKey(event: KeyboardEvent, root: Element): void {
  const command = event.metaKey || event.ctrlKey;
  if (event.altKey || ignoresKeys(event.target)) {
    return;
  }
  const key = event.key.length === 1 ? event.key.toLowerCase() : event.key;
  if (command && key === "a") {
    event.preventDefault();
    selectAll(root);
    return;
  }
  if (command) {
    return;
  }
  const flag = flagKeys[key];
  if (flag) {
    event.preventDefault();
    void flagPhotos(actionTargets(), flag);
  } else if (arrowKeys.has(key)) {
    const next = stepFocus(root, key);
    if (next) {
      event.preventDefault();
      moveFocusTo(root, next, event.shiftKey);
    }
  } else if (key === "Escape") {
    clearSelection();
  } else if (key === "Enter") {
    const tile = event.target instanceof Element ? event.target.closest("[data-photo-id]") : null;
    const target = tile?.getAttribute("data-photo-id") ?? openTarget();
    const pressable = event.target instanceof Element && event.target.closest(pressableSelector);
    if (target && !pressable) {
      event.preventDefault();
      openInDevelop(target);
    }
  }
}

export function useBrowseKeys(root: RefObject<HTMLElement | null>): void {
  useEffect(() => {
    const listener = (event: KeyboardEvent) => {
      if (root.current) {
        handleKey(event, root.current);
      }
    };
    window.addEventListener("keydown", listener);
    return () => window.removeEventListener("keydown", listener);
  }, [root]);
}
