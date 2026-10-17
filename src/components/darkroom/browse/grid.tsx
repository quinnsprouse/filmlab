import { type RefObject, type UIEvent } from "react";

import { useStore } from "@/hooks/use-store";
import { ui } from "@/lib/ui";

import { EmptyGrid, type EmptyKind } from "./empty";
import { photoCount, rowHeight, type DayGroup } from "./model";
import { PhotoTile } from "./tile";

function GroupHeader({ group }: { group: DayGroup }) {
  const selection = useStore(ui, (state) => state.selection);
  const selected = group.tiles.filter((tile) => selection.includes(tile.id)).length;
  return (
    <div className="flex items-baseline gap-2.5">
      <h2 className="text-m font-heavy text-foreground">{group.label}</h2>
      <span className="font-data text-xs font-medium text-tertiary-text">
        {photoCount(group.tiles.length)}
        {selected > 0 ? ` · ${selected} selected` : ""}
      </span>
    </div>
  );
}

interface GridProps {
  groups: DayGroup[];
  empty: EmptyKind | undefined;
  gridRef: RefObject<HTMLDivElement | null>;
  onScroll: (event: UIEvent<HTMLDivElement>) => void;
}

export function Grid({ groups, empty, gridRef, onScroll }: GridProps) {
  if (empty) {
    return <EmptyGrid kind={empty} />;
  }
  return (
    <div ref={gridRef} onScroll={onScroll} className="min-h-0 flex-1 overflow-y-auto">
      <div
        data-photo-grid=""
        style={{ "--row-height": `${rowHeight}px` }}
        className="flex flex-col gap-7 px-l pt-xs pb-l"
      >
        {groups.map((group) => (
          <section key={group.key} data-day={group.key} className="flex flex-col gap-3.5">
            <GroupHeader group={group} />
            <div className="flex flex-wrap gap-x-xs gap-y-3.5">
              {group.tiles.map((tile) => (
                <PhotoTile key={tile.id} tile={tile} />
              ))}
            </div>
          </section>
        ))}
      </div>
    </div>
  );
}
