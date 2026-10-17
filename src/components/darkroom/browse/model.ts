import type { Flag } from "@/lib/engine/types";
import type { PhotoRecord } from "@/lib/library/library";

export const rowHeight = 136;
const defaultAspect = 3 / 2;

export type Filter = "all" | "picks" | "rejected";
export type Sort = "newest" | "oldest" | "name";

export interface Tile {
  id: string;
  index: number;
  width: number;
  name: string;
  time: string;
}

export interface DayGroup {
  key: string;
  label: string;
  tiles: Tile[];
}

export interface FilterCounts {
  all: number;
  picks: number;
  rejected: number;
}

const flagOfFilter: Record<Filter, Flag | undefined> = {
  all: undefined,
  picks: "pick",
  rejected: "reject",
};

const undatedKey = "undated";
const indexingKey = "indexing";

export function countFlags(photos: PhotoRecord[]): FilterCounts {
  let picks = 0;
  let rejected = 0;
  for (const record of photos) {
    picks += record.state?.flag === "pick" ? 1 : 0;
    rejected += record.state?.flag === "reject" ? 1 : 0;
  }
  return { all: photos.length, picks, rejected };
}

function dayKeyOf(record: PhotoRecord): string {
  if (!record.probed) {
    return indexingKey;
  }
  return record.probed.facts.captured?.slice(0, 10) ?? undatedKey;
}

function dayLabel(key: string): string {
  if (key === indexingKey) {
    return "Indexing";
  }
  if (key === undatedKey) {
    return "Undated";
  }
  const [year, month, day] = key.split("-").map(Number);
  return new Date(year ?? 0, (month ?? 1) - 1, day ?? 1).toLocaleDateString("en-US", {
    weekday: "short",
    month: "short",
    day: "numeric",
  });
}

function tileOf(record: PhotoRecord, index: number): Tile {
  const facts = record.probed?.facts;
  const aspect = facts && facts.height > 0 ? facts.width / facts.height : defaultAspect;
  return {
    id: record.file.id,
    index: index + 1,
    width: Math.round(rowHeight * aspect),
    name: record.file.name,
    time: facts?.captured?.slice(11) ?? "",
  };
}

function compareTiles(sort: Sort): (a: Tile, b: Tile) => number {
  return (a, b) =>
    sort === "name"
      ? a.name.localeCompare(b.name, undefined, { numeric: true })
      : a.time.localeCompare(b.time) || a.index - b.index;
}

function compareDays(sort: Sort): (a: string, b: string) => number {
  const order = (key: string) => (key === indexingKey || key === undatedKey ? 1 : 0);
  return (a, b) => {
    const rank = order(a) - order(b);
    if (rank !== 0) {
      return rank;
    }
    return sort === "oldest" ? a.localeCompare(b) : b.localeCompare(a);
  };
}

export function groupPhotos(photos: PhotoRecord[], filter: Filter, sort: Sort): DayGroup[] {
  const wanted = flagOfFilter[filter];
  const days = new Map<string, Tile[]>();
  for (const [index, record] of photos.entries()) {
    if (!wanted || record.state?.flag === wanted) {
      const key = dayKeyOf(record);
      days.set(key, [...(days.get(key) ?? []), tileOf(record, index)]);
    }
  }
  return [...days.keys()].toSorted(compareDays(sort)).map((key) => ({
    key,
    label: dayLabel(key),
    tiles: (days.get(key) ?? []).toSorted(compareTiles(sort)),
  }));
}

export function photoCount(count: number): string {
  return `${count} ${count === 1 ? "photo" : "photos"}`;
}

export function cameraModel(camera: string): string {
  const withoutCompany = camera.replace(/^.*\b(?:LTD|INC|CO|CORP|CORPORATION)\b\.?,?\s*/i, "");
  const maker = camera.split(/\s+/)[0] ?? "";
  const model = withoutCompany.replace(new RegExp(`^${maker}\\s+`, "i"), "");
  return model || withoutCompany || camera;
}

export function capturedParts(captured: string | null): { day: string; time: string } | undefined {
  if (!captured) {
    return undefined;
  }
  return { day: dayLabel(captured.slice(0, 10)), time: captured.slice(11, 16) };
}
