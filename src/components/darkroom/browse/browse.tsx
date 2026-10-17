import { useRef, useState, type UIEvent } from "react";

import { useStore } from "@/hooks/use-store";
import { library } from "@/lib/library/library";

import type { EmptyKind } from "./empty";
import { Grid } from "./grid";
import { InfoPanel } from "./info-panel";
import { useBrowseKeys } from "./keys";
import { countFlags, groupPhotos, type Filter, type Sort } from "./model";
import { Rail } from "./rail";
import { SelectionPanel } from "./selection-panel";
import { Toolbar } from "./toolbar";

function emptyKind(total: number, filter: Filter, shown: number): EmptyKind | undefined {
  if (total === 0) {
    return "folder";
  }
  if (shown > 0) {
    return undefined;
  }
  return filter === "rejected" ? "rejected" : "picks";
}

function dayInView(scroller: HTMLElement): string | undefined {
  const top = scroller.getBoundingClientRect().top + 8;
  let current: string | undefined;
  for (const section of scroller.querySelectorAll<HTMLElement>("[data-day]")) {
    if (section.getBoundingClientRect().top <= top) {
      current = section.dataset.day;
    }
  }
  return current;
}

export function Browse() {
  const photos = useStore(library, (state) => state.photos);
  const [filter, setFilter] = useState<Filter>("all");
  const [sort, setSort] = useState<Sort>("newest");
  const [scrolledTo, setScrolledTo] = useState<string>();
  const gridRef = useRef<HTMLDivElement>(null);
  useBrowseKeys(gridRef);

  const groups = groupPhotos(photos, filter, sort);
  const shown = groups.reduce((sum, group) => sum + group.tiles.length, 0);
  const inView = groups.some((group) => group.key === scrolledTo) ? scrolledTo : groups[0]?.key;

  function jump(key: string) {
    const scroller = gridRef.current;
    const section = scroller?.querySelector<HTMLElement>(`[data-day="${CSS.escape(key)}"]`);
    if (scroller && section) {
      const offset = section.getBoundingClientRect().top - scroller.getBoundingClientRect().top;
      scroller.scrollTo({ top: scroller.scrollTop + offset });
      setScrolledTo(key);
    }
  }

  function scrolled(event: UIEvent<HTMLDivElement>) {
    setScrolledTo(dayInView(event.currentTarget));
  }

  return (
    <div className="flex h-full min-h-0">
      <Rail groups={groups} inView={inView} onJump={jump} />
      <div className="flex min-w-0 flex-1 flex-col">
        <Toolbar
          filter={filter}
          onFilter={setFilter}
          counts={countFlags(photos)}
          sort={sort}
          onSort={setSort}
        />
        <Grid
          groups={groups}
          empty={emptyKind(photos.length, filter, shown)}
          gridRef={gridRef}
          onScroll={scrolled}
        />
      </div>
      <aside className="flex w-[320px] shrink-0 flex-col gap-xs overflow-y-auto bg-chrome p-xs">
        <SelectionPanel />
        <InfoPanel />
      </aside>
    </div>
  );
}
