import { useStore } from "@/hooks/use-store";
import { develop } from "@/lib/develop/session";
import type { DevelopSettings, Facts, RenderedFrame } from "@/lib/engine/types";
import { cn } from "@/lib/utils";

import { readMasks } from "./masks-model";
import { useMaskPointer } from "./masks-pointer";
import { buildScene } from "./masks-scene";
import { masksState } from "./masks-state";
import { viewSpace } from "./masks-view";
import { BrushLayer } from "./overlay-brush";
import { SceneLayer } from "./overlay-scene";

interface LoadedProps {
  facts: Facts;
  settings: DevelopSettings;
  frame: RenderedFrame;
}

function Loaded({ facts, settings, frame }: LoadedProps) {
  const photoId = useStore(develop, (state) => state.photoId);
  const state = useStore(masksState, (current) => current);
  const masks = readMasks(settings);
  const activeId = state.photoId === photoId ? state.active : undefined;
  const space = viewSpace(facts, settings, frame, window.devicePixelRatio);
  const scene = buildScene(masks, activeId, space);
  const { interaction, brush } = state;
  const pointer = useMaskPointer({ space, scene, masks, activeId, interaction, brush });
  const painting = interaction.mode === "paint";
  const erasing = brush.erase !== pointer.alt;

  return (
    <div
      className={cn(
        "absolute inset-0 touch-none select-none",
        interaction.mode === "pick" && "cursor-crosshair",
        painting && "cursor-none",
        interaction.mode === "adjust" && pointer.overGrip && "cursor-grab",
      )}
      data-testid="masks-overlay"
      {...pointer.handlers}
    >
      <svg className="pointer-events-none size-full overflow-hidden" aria-hidden="true">
        <SceneLayer scene={scene} />
        {painting ? (
          <BrushLayer
            cursor={pointer.cursor}
            stroke={pointer.stroke}
            radius={brush.size * space.longEdge}
            feather={brush.feather}
            erasing={erasing}
          />
        ) : null}
      </svg>
    </div>
  );
}

export function MasksOverlay() {
  const facts = useStore(develop, (state) => state.facts);
  const settings = useStore(develop, (state) => state.settings);
  const frame = useStore(develop, (state) => state.frame);
  if (!facts || !settings || !frame) {
    return null;
  }
  return <Loaded facts={facts} settings={settings} frame={frame} />;
}
