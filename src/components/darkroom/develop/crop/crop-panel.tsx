import { ButtonGroup, SelectControl, Toggle } from "dialkit";
import { useState } from "react";

import { ControlSlider, PanelRoot } from "@/components/darkroom/develop/panel-controls";
import { Button } from "@/components/ui/button";
import { useStore } from "@/hooks/use-store";
import { develop } from "@/lib/develop/session";
import { controlValues } from "@/lib/engine/engine";

import { aspectName, aspectOptions, aspectValue } from "./aspect";
import {
  autoStraighten,
  orient,
  resetAngle,
  resetCrop,
  rotateAspect,
  setAngle,
  setAspect,
  type Orientation,
} from "./crop-commands";
import { finishCrop, useCropSession } from "./crop-session";
import { CustomAspect } from "./custom-aspect";
import { readCrop } from "./geometry";

const angleRange = { min: -45, max: 45, step: 0.01 };

export function CropPanel() {
  const settings = useStore(develop, (state) => state.settings);
  const frame = useStore(develop, (state) => state.frame);
  const [error, setError] = useState<string | undefined>();
  const [custom, setCustom] = useState(false);
  useCropSession(setError);
  if (!settings) {
    return null;
  }
  const crop = readCrop(settings);
  const original = frame ? frame.width / frame.height : 1;
  const stored = aspectValue(crop.aspect, original);
  const shown = custom ? "custom" : stored;
  const angle = controlValues(settings)["crop.angle"] ?? 0;

  const run = (message: string | undefined) => setError(message);
  const orientation = (kind: Orientation) => () => run(orient(kind));
  const chooseAspect = (value: string) => {
    setCustom(value === "custom");
    if (value !== "custom") {
      run(setAspect(value, `Crop ${aspectName(value)}`));
    }
  };

  return (
    <div className="flex flex-col gap-m">
      <PanelRoot title="Crop">
        <SelectControl
          label="Aspect"
          value={shown}
          options={aspectOptions}
          onChange={chooseAspect}
        />
        {custom ? <CustomAspect onApply={run} onDone={() => setCustom(false)} /> : null}
        <Toggle
          label="Lock aspect"
          checked={crop.aspect !== undefined}
          onChange={(locked) =>
            run(
              setAspect(
                locked ? "current" : "free",
                locked ? "Lock crop aspect" : "Unlock crop aspect",
              ),
            )
          }
        />
        <ButtonGroup
          buttons={[
            { label: "Rotate aspect (X)", onClick: () => run(rotateAspect()) },
            { label: "Auto straighten", onClick: () => run(autoStraighten()) },
          ]}
        />
        <ControlSlider
          id="crop.angle"
          label="Angle"
          value={angle}
          range={angleRange}
          onChange={(next) => setAngle(next, true)}
          onReset={resetAngle}
        />
        <ButtonGroup
          buttons={[
            { label: "Rotate left", onClick: orientation("rotateLeft") },
            { label: "Rotate right", onClick: orientation("rotateRight") },
          ]}
        />
        <ButtonGroup
          buttons={[
            { label: "Flip horizontal", onClick: orientation("flipHorizontal") },
            { label: "Flip vertical", onClick: orientation("flipVertical") },
          ]}
        />
      </PanelRoot>
      {error ? (
        <p role="alert" className="text-xs text-secondary-text">
          {error}
        </p>
      ) : null}
      <div className="flex gap-xs">
        <Button variant="outline" className="grow basis-0" onClick={() => run(resetCrop())}>
          Reset crop
        </Button>
        <Button className="grow basis-0" onClick={finishCrop}>
          Done
        </Button>
      </div>
    </div>
  );
}
