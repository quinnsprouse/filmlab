import { useEffect } from "react";

import { develop, goToStep, setTool } from "@/lib/develop/session";

import { rotateAspect } from "./crop-commands";

interface Entry {
  photoId: string | undefined;
  cursor: number;
}

let entry: Entry | undefined;

export function finishCrop(): void {
  setTool("edit");
}

function cancelCrop(): void {
  const state = develop.get();
  if (entry && entry.photoId === state.photoId && entry.cursor !== state.cursor) {
    goToStep(entry.cursor);
  }
  setTool("edit");
}

function takesKeys(target: EventTarget | null): boolean {
  return (
    target instanceof HTMLElement &&
    (target.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName))
  );
}

export function useCropSession(onError: (message: string | undefined) => void): void {
  useEffect(() => {
    const state = develop.get();
    entry = { photoId: state.photoId, cursor: state.cursor };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.defaultPrevented || event.metaKey || event.ctrlKey || event.altKey) {
        return;
      }
      if (event.key === "Escape" && !takesKeys(event.target)) {
        event.preventDefault();
        cancelCrop();
      } else if (
        event.key === "Enter" &&
        !takesKeys(event.target) &&
        !(event.target instanceof HTMLButtonElement)
      ) {
        event.preventDefault();
        finishCrop();
      } else if (event.key.toLowerCase() === "x" && !event.shiftKey && !takesKeys(event.target)) {
        event.preventDefault();
        onError(rotateAspect());
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onError]);
}
