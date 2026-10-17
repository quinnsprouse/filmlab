import { memo, type MouseEvent } from "react";

import { Alert02Icon, Cancel01Icon, Flag02Icon, Icon, Tick02Icon } from "@/components/icons";
import { useStore } from "@/hooks/use-store";
import { library } from "@/lib/library/library";
import { ui } from "@/lib/ui";
import { cn } from "@/lib/utils";

import type { Tile as TileData } from "./model";
import { clickTile, openInDevelop } from "./selection";

function ThumbnailFace({ index }: { index: number }) {
  const record = useStore(library, (state) => state.photos[index - 1]);
  if (record?.error) {
    return (
      <div
        title={record.error}
        className="flex size-full flex-col items-center justify-center gap-xxs bg-control px-xs text-center text-xxs text-tertiary-text"
      >
        <Icon icon={Alert02Icon} size={16} />
        Couldn't read this file
      </div>
    );
  }
  if (!record?.thumbnail) {
    return <div className="size-full animate-pulse bg-control motion-reduce:animate-none" />;
  }
  return (
    <img
      src={record.thumbnail}
      alt={record.file.name}
      draggable={false}
      className={cn(
        "size-full object-cover outline outline-1 -outline-offset-1 outline-outline-image",
        record.state?.flag === "reject" && "opacity-35",
      )}
    />
  );
}

function FlagMark({ index }: { index: number }) {
  const flag = useStore(library, (state) => state.photos[index - 1]?.state?.flag);
  if (flag === "pick") {
    return <Icon icon={Flag02Icon} size={12} className="text-secondary-text" />;
  }
  if (flag === "reject") {
    return <Icon icon={Cancel01Icon} size={12} className="text-secondary-text" />;
  }
  return null;
}

function TileComponent({ tile }: { tile: TileData }) {
  const selected = useStore(ui, (state) => state.selection.includes(tile.id));
  const focused = useStore(ui, (state) => state.focused === tile.id);

  function select(event: MouseEvent<HTMLButtonElement>) {
    const root = event.currentTarget.closest("[data-photo-grid]");
    if (root) {
      clickTile(root, tile.id, {
        range: event.shiftKey,
        toggle: event.metaKey || event.ctrlKey,
      });
    }
  }

  return (
    <button
      type="button"
      aria-pressed={selected}
      aria-label={tile.name}
      data-photo-id={tile.id}
      onClick={select}
      onDoubleClick={() => openInDevelop(tile.id)}
      style={{ "--tile-width": `${tile.width}px` }}
      className="flex w-(--tile-width) shrink-0 cursor-default flex-col gap-xs text-left outline-none select-none"
    >
      <div className="relative h-(--row-height)">
        <ThumbnailFace index={tile.index} />
        <div
          className={cn(
            "pointer-events-none absolute inset-0",
            focused && "inset-ring-3 inset-ring-primary",
            !focused && selected && "inset-ring-2 inset-ring-foreground",
          )}
        />
        {selected ? (
          <div className="pointer-events-none absolute top-s left-s grid size-[18px] place-items-center rounded-full bg-foreground text-canvas shadow-s">
            <Icon icon={Tick02Icon} size={12} />
          </div>
        ) : null}
      </div>
      <div className="flex h-[14px] items-center gap-s">
        <span
          className={cn(
            "font-data text-xxs tabular-nums",
            focused ? "font-heavy text-primary" : "font-medium text-tertiary-text",
          )}
        >
          ▸{tile.index}
        </span>
        <FlagMark index={tile.index} />
      </div>
    </button>
  );
}

export const PhotoTile = memo(TileComponent);
