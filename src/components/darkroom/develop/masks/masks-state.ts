import { develop } from "@/lib/develop/session";
import { createStore } from "@/lib/store";

export interface BrushSettings {
  size: number;
  feather: number;
  flow: number;
  erase: boolean;
  autoMask: boolean;
  pressure: boolean;
}

export type Interaction =
  | { mode: "adjust" }
  | { mode: "paint"; maskId: number | undefined }
  | { mode: "pick" };

export interface MasksState {
  photoId: string | undefined;
  active: number | undefined;
  showOverlay: boolean;
  hovered: number | undefined;
  engaged: boolean;
  lumaMap: boolean;
  interaction: Interaction;
  brush: BrushSettings;
  error: string | undefined;
}

export const masksState = createStore<MasksState>({
  photoId: undefined,
  active: undefined,
  showOverlay: false,
  hovered: undefined,
  engaged: false,
  lumaMap: false,
  interaction: { mode: "adjust" },
  brush: { size: 0.03, feather: 50, flow: 100, erase: false, autoMask: false, pressure: true },
  error: undefined,
});

export function patchMasks(patch: Partial<MasksState>): void {
  masksState.update((state) => ({ ...state, ...patch }));
}

export function patchBrush(patch: Partial<BrushSettings>): void {
  masksState.update((state) => ({ ...state, brush: { ...state.brush, ...patch } }));
}

export function selectMask(id: number | undefined): void {
  patchMasks({
    photoId: develop.get().photoId,
    active: id,
    interaction: { mode: "adjust" },
    error: undefined,
  });
}

export function reportError(error: unknown): void {
  patchMasks({ error: error instanceof Error ? error.message : String(error) });
}
