import { createStore } from "@/lib/store";

export const exportDialog = createStore<boolean>(false);

export function openExport(): void {
  exportDialog.set(true);
}

export function closeExport(): void {
  exportDialog.set(false);
}
