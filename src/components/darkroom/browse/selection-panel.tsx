import { SelectControl, Slider } from "dialkit";
import { useState } from "react";

import { Cancel01Icon, Flag02Icon, Icon } from "@/components/icons";
import { Button } from "@/components/ui/button";
import { useStore } from "@/hooks/use-store";
import type { Flag } from "@/lib/engine/types";
import { library } from "@/lib/library/library";
import { profiles } from "@/lib/profiles/profiles";
import { ui } from "@/lib/ui";
import { cn } from "@/lib/utils";

import { applyProfileToPhotos, clearSelection, flagPhotos } from "./selection";

const previewCount = 3;

const flagOptions: { value: Flag; label: string; icon: typeof Flag02Icon | undefined }[] = [
  { value: "pick", label: "Pick", icon: Flag02Icon },
  { value: "none", label: "None", icon: undefined },
  { value: "reject", label: "Reject", icon: Cancel01Icon },
];

function SelectedThumb({ id }: { id: string }) {
  const thumbnail = useStore(
    library,
    (state) => state.photos.find((record) => record.file.id === id)?.thumbnail,
  );
  return thumbnail ? (
    <img
      src={thumbnail}
      alt=""
      className="h-[40px] w-[60px] shrink-0 rounded-xs object-cover outline outline-1 -outline-offset-1 outline-outline-image"
    />
  ) : (
    <div className="h-[40px] w-[60px] shrink-0 rounded-xs bg-control" />
  );
}

function ApplyProfile({ ids }: { ids: string[] }) {
  const saved = useStore(profiles, (state) => state);
  const [chosen, setChosen] = useState<string>();
  const [amount, setAmount] = useState(100);
  const [applying, setApplying] = useState(false);
  const profile = saved.find((item) => item.id === chosen) ?? saved[0];

  async function apply() {
    if (!profile) {
      return;
    }
    setApplying(true);
    await applyProfileToPhotos(ids, profile, amount / 100);
    setApplying(false);
  }

  return (
    <div className="flex flex-col gap-s pt-s pb-3.5 divider-t">
      <h3 className="pb-xs text-s font-heavy text-secondary-text">Apply profile</h3>
      {profile ? (
        <>
          <div className="dialkit-root" data-mode="inline" data-theme="dark">
            <SelectControl
              label="Profile"
              value={profile.id}
              options={saved.map((item) => ({ value: item.id, label: item.name }))}
              onChange={setChosen}
            />
            <Slider
              label="Amount"
              value={amount}
              min={0}
              max={200}
              step={1}
              unit="%"
              onChange={setAmount}
            />
          </div>
          <Button variant="outline" disabled={applying} onClick={() => void apply()}>
            Apply to {ids.length} {ids.length === 1 ? "photo" : "photos"}
          </Button>
          <p className="px-xxs pt-xs text-xs text-tertiary-text">
            Each photo keeps its own light, white balance and crop.
          </p>
        </>
      ) : (
        <p className="px-xxs text-xs text-tertiary-text">
          Save a profile in Develop to reuse it here.
        </p>
      )}
    </div>
  );
}

function FlagRow({ ids }: { ids: string[] }) {
  const photos = useStore(library, (state) => state.photos);
  const flags = new Set(
    photos.filter((record) => ids.includes(record.file.id)).map((record) => record.state?.flag),
  );
  const current = flags.size === 1 ? [...flags][0] : undefined;
  return (
    <div className="pt-s pb-xs divider-t">
      <fieldset className="m-0 flex h-9 min-w-0 items-center justify-between rounded-m border-0 bg-control py-xxs pr-xxs pl-2.5">
        <legend className="sr-only">Flag</legend>
        <span aria-hidden className="text-s font-medium text-secondary-text">
          Flag
        </span>
        <div className="flex">
          {flagOptions.map((option) => (
            <button
              key={option.value}
              type="button"
              aria-pressed={current === option.value}
              onClick={() => void flagPhotos(ids, option.value)}
              className={cn(
                "flex h-8 items-center justify-center gap-xs rounded-s px-s text-s font-medium transition-colors duration-(--motion-duration) ease-(--motion-easing) hover:bg-control-hover",
                current === option.value
                  ? "bg-control-active text-foreground"
                  : "text-secondary-text",
              )}
            >
              {option.icon ? <Icon icon={option.icon} size={14} /> : null}
              {option.label}
            </button>
          ))}
        </div>
      </fieldset>
    </div>
  );
}

export function SelectionPanel() {
  const selection = useStore(ui, (state) => state.selection);
  const total = useStore(library, (state) => state.photos.length);
  const count = selection.length;
  return (
    <section
      aria-label="Selection"
      className="flex flex-col rounded-l bg-panel px-s pt-s pb-xxs ring ring-border"
    >
      <div className="flex items-center justify-between pb-2.5">
        <div className="flex items-baseline gap-xs">
          <h2 className="text-m font-heavy text-foreground">
            {count === 0 ? "No selection" : `${count} selected`}
          </h2>
          <span className="font-data text-xs font-medium text-tertiary-text">of {total}</span>
        </div>
        {count > 0 ? (
          <button
            type="button"
            onClick={clearSelection}
            className="h-7 rounded-s px-xs text-s font-medium text-secondary-text transition-colors duration-(--motion-duration) ease-(--motion-easing) hover:bg-control-hover"
          >
            Clear
          </button>
        ) : null}
      </div>
      {count === 0 ? (
        <p className="py-s text-xs text-tertiary-text divider-t">
          Click a photo to select it, or press ⌘A to select all.
        </p>
      ) : (
        <>
          <div className="flex items-center gap-xs pt-s pb-3.5 divider-t">
            {selection.slice(0, previewCount).map((id) => (
              <SelectedThumb key={id} id={id} />
            ))}
            {count > previewCount ? (
              <div className="grid h-[40px] min-w-0 flex-1 place-items-center rounded-xs bg-control font-data text-xs font-medium text-secondary-text">
                +{count - previewCount}
              </div>
            ) : null}
          </div>
          <ApplyProfile ids={selection} />
          <FlagRow ids={selection} />
        </>
      )}
    </section>
  );
}
