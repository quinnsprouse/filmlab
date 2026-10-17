import { createStore } from "@/lib/store";

export type Mode = "browse" | "develop";

export interface UiState {
  mode: Mode;
  selection: string[];
  focused: string | undefined;
}

export const ui = createStore<UiState>({ mode: "browse", selection: [], focused: undefined });

export function setMode(mode: Mode): void {
  ui.update((state) => ({ ...state, mode }));
}

export function focusPhoto(id: string | undefined): void {
  ui.update((state) => ({ ...state, focused: id }));
}

export function selectPhotos(selection: string[], focused?: string): void {
  ui.update((state) => ({ ...state, selection, focused: focused ?? state.focused }));
}
