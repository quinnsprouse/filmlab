import {
  ContrastIcon,
  Copy01Icon,
  Delete02Icon,
  PencilEdit02Icon,
  ViewIcon,
  ViewOffIcon,
} from "@hugeicons/core-free-icons";
import { useState } from "react";

import { cn } from "@/lib/utils";

import { KindGlyph } from "./create-grid";
import { IconAction } from "./icon-action";
import {
  chooseMask,
  deleteMask,
  duplicateMask,
  invertMask,
  renameMask,
  toggleVisible,
} from "./masks-commands";
import type { CreateKind, Mask } from "./masks-model";
import { patchMasks } from "./masks-state";

function leadKind(mask: Mask): CreateKind {
  const shape = mask.components[0]?.shape;
  return shape && shape.kind !== "other" ? shape.kind : "subject";
}

interface NameFieldProps {
  mask: Mask;
  onDone: () => void;
}

function NameField({ mask, onDone }: NameFieldProps) {
  const [draft, setDraft] = useState(mask.name);
  const finish = (commit: boolean) => {
    if (commit) {
      renameMask(mask.id, draft);
    }
    onDone();
  };
  return (
    <input
      aria-label="Mask name"
      value={draft}
      onChange={(event) => setDraft(event.target.value)}
      onBlur={() => finish(true)}
      onKeyDown={(event) => {
        if (event.key === "Enter") {
          finish(true);
        } else if (event.key === "Escape") {
          event.stopPropagation();
          finish(false);
        }
      }}
      onFocus={(event) => event.currentTarget.select()}
      ref={(node) => node?.focus()}
      className="h-6 min-w-0 grow rounded-xs bg-control-active px-xs text-s text-foreground outline-none"
    />
  );
}

interface RowProps {
  mask: Mask;
  selected: boolean;
}

function MaskRow({ mask, selected }: RowProps) {
  const [renaming, setRenaming] = useState(false);
  return (
    <li
      className={cn(
        "flex h-9 items-center gap-xxs rounded-m pr-xxs pl-xs transition-colors duration-(--motion-duration) ease-(--motion-easing)",
        selected ? "bg-control-active" : "hover:bg-control",
      )}
      onPointerEnter={() => patchMasks({ hovered: mask.id })}
      onPointerLeave={() => patchMasks({ hovered: undefined })}
    >
      {renaming ? (
        <NameField mask={mask} onDone={() => setRenaming(false)} />
      ) : (
        <button
          type="button"
          aria-pressed={selected}
          onClick={() => chooseMask(mask.id)}
          onDoubleClick={() => setRenaming(true)}
          className={cn(
            "flex h-full min-w-0 grow items-center gap-s text-left text-s font-medium",
            selected ? "text-foreground" : "text-secondary-text",
            !mask.visible && "opacity-50",
          )}
        >
          <KindGlyph kind={leadKind(mask)} size={14} />
          <span className="truncate">{mask.name}</span>
          {mask.components.length > 1 ? (
            <span className="shrink-0 font-data text-xs text-tertiary-text tabular-nums">
              {mask.components.length}
            </span>
          ) : null}
        </button>
      )}
      {selected && !renaming ? (
        <>
          <IconAction
            icon={PencilEdit02Icon}
            label={`Rename ${mask.name}`}
            onClick={() => setRenaming(true)}
          />
          <IconAction
            icon={ContrastIcon}
            label={`Invert ${mask.name}`}
            pressed={mask.invert}
            onClick={() => invertMask(mask.id)}
          />
          <IconAction
            icon={Copy01Icon}
            label={`Duplicate ${mask.name}`}
            onClick={() => duplicateMask(mask.id)}
          />
          <IconAction
            icon={Delete02Icon}
            label={`Delete ${mask.name}`}
            onClick={() => deleteMask(mask.id)}
          />
        </>
      ) : null}
      <IconAction
        icon={mask.visible ? ViewIcon : ViewOffIcon}
        label={`${mask.visible ? "Hide" : "Show"} ${mask.name}`}
        onClick={() => toggleVisible(mask.id)}
      />
    </li>
  );
}

interface MaskListProps {
  masks: Mask[];
  activeId: number | undefined;
}

export function MaskList({ masks, activeId }: MaskListProps) {
  if (masks.length === 0) {
    return (
      <p className="px-s py-xs text-s text-tertiary-text">
        No masks yet. Choose a mask type above, then adjust it here.
      </p>
    );
  }
  return (
    <ul className="flex flex-col gap-xxs p-xs" aria-label="Masks">
      {masks.map((mask) => (
        <MaskRow key={mask.id} mask={mask} selected={mask.id === activeId} />
      ))}
    </ul>
  );
}
