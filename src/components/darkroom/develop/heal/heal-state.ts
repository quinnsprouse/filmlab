import { useStore } from "@/hooks/use-store";
import { develop } from "@/lib/develop/session";
import { createStore } from "@/lib/store";

import type { Spot, SpotMode } from "./spots";

interface Brush {
  mode: SpotMode;
  size: number;
  feather: number;
  opacity: number;
  message: string | undefined;
}

export const brush = createStore<Brush>({
  mode: "heal",
  size: 0.02,
  feather: 50,
  opacity: 100,
  message: undefined,
});

export function setBrush(change: Partial<Brush>): void {
  brush.update((current) => ({ ...current, ...change }));
}

interface Selection {
  photoId: string | undefined;
  index: number | undefined;
}

const selection = createStore<Selection>({ photoId: undefined, index: undefined });

export function selectSpot(index: number | undefined): void {
  selection.set({ photoId: develop.get().photoId, index });
}

export function useSelectedSpot(count: number): number | undefined {
  const photoId = useStore(develop, (state) => state.photoId);
  const index = useStore(selection, (current) =>
    current.photoId === photoId ? current.index : undefined,
  );
  return index !== undefined && index < count ? index : undefined;
}

export function pickSpot(index: number, spot: Spot | undefined): void {
  selectSpot(index);
  if (spot) {
    setBrush({ size: spot.size, feather: spot.feather, opacity: spot.opacity, mode: spot.mode });
  }
}
