import { useEffect } from "react";

import { brush, selectSpot, setBrush } from "./heal-state";
import { commandMessage, deleteSpot, refreshSource, sizeRange, updateSpot } from "./spots";

function isTyping(target: EventTarget | null): boolean {
  return (
    target instanceof HTMLElement &&
    (target.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName))
  );
}

function run(action: () => void): void {
  try {
    action();
    setBrush({ message: undefined });
  } catch (error) {
    setBrush({ message: commandMessage(error) });
  }
}

function resize(selected: number | undefined, factor: number): void {
  const size = Math.min(sizeRange.max, Math.max(sizeRange.min, brush.get().size * factor));
  setBrush({ size });
  if (selected !== undefined) {
    run(() => updateSpot(selected, { size }, "Spot size", "size"));
  }
}

export function useHealKeys(selected: number | undefined): void {
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (isTyping(event.target) || event.metaKey || event.ctrlKey || event.altKey) {
        return;
      }
      if ((event.key === "Delete" || event.key === "Backspace") && selected !== undefined) {
        event.preventDefault();
        run(() => deleteSpot(selected));
        selectSpot(undefined);
      } else if (event.key === "/" && selected !== undefined) {
        event.preventDefault();
        run(() => refreshSource(selected));
      } else if (event.key === "Escape" && selected !== undefined) {
        selectSpot(undefined);
      } else if (event.key === "[" || event.key === "]") {
        event.preventDefault();
        resize(selected, event.key === "]" ? 1.15 : 1 / 1.15);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [selected]);
}
