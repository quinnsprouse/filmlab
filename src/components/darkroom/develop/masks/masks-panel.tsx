import { Folder, Toggle } from "dialkit";

import { PanelRoot } from "@/components/darkroom/develop/panel-controls";
import { useStore } from "@/hooks/use-store";
import { develop } from "@/lib/develop/session";

import { BrushSettings } from "./brush-settings";
import { ComponentList } from "./component-list";
import { CreateGrid } from "./create-grid";
import { MaskAdjust } from "./mask-adjust";
import { MaskList } from "./mask-list";
import { readMasks } from "./masks-model";
import { masksState, patchMasks } from "./masks-state";
import { RadialControls } from "./radial-feather";
import { useMaskKeys, useMaskOverlaySync } from "./use-mask-effects";

function Hint({ mode }: { mode: string }) {
  if (mode === "paint") {
    return (
      <p className="px-s pb-xs text-xs text-tertiary-text">
        Drag on the photo to paint. Hold Alt to swap paint and erase, Shift-click to draw a straight
        line. [ and ] resize the brush.
      </p>
    );
  }
  return null;
}

export function MasksPanel() {
  useMaskKeys();
  useMaskOverlaySync();
  const settings = useStore(develop, (state) => state.settings);
  const photoId = useStore(develop, (state) => state.photoId);
  const state = useStore(masksState, (current) => current);
  const masks = readMasks(settings);
  const activeId = state.photoId === photoId ? state.active : undefined;
  const mask = masks.find((candidate) => candidate.id === activeId);
  const mode = state.interaction.mode;

  return (
    <PanelRoot title="Masks">
      {state.error ? (
        <p role="alert" className="px-s py-xs text-s text-foreground">
          {state.error}
        </p>
      ) : null}
      <Folder title="Create" defaultOpen>
        <CreateGrid />
      </Folder>
      <Folder title="Masks" defaultOpen>
        <MaskList masks={masks} activeId={activeId} />
        <Toggle
          label="Show overlay (O)"
          checked={state.showOverlay}
          onChange={(showOverlay) => patchMasks({ showOverlay })}
        />
      </Folder>
      {mask ? (
        <>
          <Folder title="Selection" defaultOpen>
            <ComponentList mask={mask} />
            <RadialControls mask={mask} />
          </Folder>
          {mode === "paint" ? (
            <Folder title="Brush" defaultOpen>
              <Hint mode={mode} />
              <BrushSettings />
            </Folder>
          ) : null}
          <Folder title="Adjustments" defaultOpen>
            <MaskAdjust mask={mask} />
          </Folder>
        </>
      ) : null}
      {!mask && mode === "paint" ? (
        <Folder title="Brush" defaultOpen>
          <Hint mode={mode} />
          <BrushSettings />
        </Folder>
      ) : null}
    </PanelRoot>
  );
}
