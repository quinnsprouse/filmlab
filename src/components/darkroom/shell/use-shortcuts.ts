import { useEffect } from "react";

import {
  redo,
  setCompare,
  setTool,
  setZoom,
  undo,
  develop,
  type Tool,
} from "@/lib/develop/session";
import { library } from "@/lib/library/library";
import { ui } from "@/lib/ui";

import { exportDialog, openExport } from "./export-state";
import { showBrowse, showDevelop } from "./navigation";

function isTyping(target: EventTarget | null): boolean {
  return (
    target instanceof HTMLElement &&
    (target.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName))
  );
}

const toolKeys: Record<string, Tool> = { e: "edit", r: "crop", q: "heal", m: "masks" };

function toggleTool(tool: Tool): void {
  setTool(develop.get().tool === tool && tool !== "edit" ? "edit" : tool);
}

function developKey(key: string): void {
  const tool = toolKeys[key];
  if (key === "\\") {
    setCompare(develop.get().compare === "before" ? "after" : "before");
  } else if (tool) {
    toggleTool(tool);
  } else if (key === "z") {
    const { zoom } = develop.get();
    setZoom({ ...zoom, level: zoom.level === "fit" ? 1 : "fit" });
  }
}

function command(event: KeyboardEvent): void {
  const developing = ui.get().mode === "develop";
  const key = event.key.toLowerCase();
  if (event.metaKey || event.ctrlKey) {
    if (event.altKey) {
      return;
    }
    if (key === "z" && developing) {
      event.preventDefault();
      (event.shiftKey ? redo : undo)();
    } else if (key === "e" && !event.shiftKey) {
      event.preventDefault();
      if (library.get().photos.length > 0) {
        openExport();
      }
    }
    return;
  }
  if (event.altKey || event.shiftKey) {
    return;
  }
  if (key === "g") {
    showBrowse();
  } else if (key === "d") {
    showDevelop();
  } else if (developing) {
    developKey(key);
  }
}

export function useShortcuts(): void {
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.defaultPrevented || exportDialog.get() || isTyping(event.target)) {
        return;
      }
      command(event);
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);
}
