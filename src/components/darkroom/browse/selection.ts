import { applyProfile } from "@/lib/engine/engine";
import type { Flag } from "@/lib/engine/types";
import { persistPhoto, photoRecord, refreshThumbnail } from "@/lib/library/library";
import type { Profile } from "@/lib/profiles/profiles";
import { focusPhoto, selectPhotos, setMode, ui } from "@/lib/ui";

const tileSelector = "[data-photo-id]";

let anchor: string | undefined;

function visibleIds(root: Element): string[] {
  return [...root.querySelectorAll<HTMLElement>(tileSelector)].map(
    (element) => element.dataset.photoId ?? "",
  );
}

function tileElement(root: Element, id: string): HTMLElement | null {
  return root.querySelector<HTMLElement>(`[data-photo-id="${CSS.escape(id)}"]`);
}

function rangeBetween(ids: string[], from: string | undefined, to: string): string[] {
  const start = from ? ids.indexOf(from) : -1;
  const end = ids.indexOf(to);
  if (start < 0 || end < 0) {
    return [to];
  }
  return ids.slice(Math.min(start, end), Math.max(start, end) + 1);
}

interface ClickModifiers {
  range: boolean;
  toggle: boolean;
}

export function clickTile(root: Element, id: string, modifiers: ClickModifiers): void {
  const { selection, focused } = ui.get();
  if (modifiers.range) {
    anchor ??= focused;
    selectPhotos(rangeBetween(visibleIds(root), anchor, id), id);
    return;
  }
  anchor = id;
  if (modifiers.toggle) {
    const next = selection.includes(id)
      ? selection.filter((other) => other !== id)
      : [...selection, id];
    selectPhotos(next, id);
    return;
  }
  selectPhotos([id], id);
}

export function moveFocusTo(root: Element, id: string, extend: boolean): void {
  if (extend) {
    anchor ??= ui.get().focused;
    selectPhotos(rangeBetween(visibleIds(root), anchor, id), id);
  } else {
    anchor = id;
    selectPhotos([id], id);
  }
  tileElement(root, id)?.scrollIntoView({ block: "nearest" });
}

export function selectAll(root: Element): void {
  const ids = visibleIds(root);
  selectPhotos(ids, ui.get().focused ?? ids[0]);
}

export function clearSelection(): void {
  anchor = undefined;
  selectPhotos([]);
}

export function openInDevelop(id: string): void {
  focusPhoto(id);
  setMode("develop");
}

export function actionTargets(): string[] {
  const { selection, focused } = ui.get();
  if (selection.length > 0) {
    return selection;
  }
  return focused ? [focused] : [];
}

export async function flagPhotos(ids: string[], flag: Flag): Promise<void> {
  await Promise.all(
    ids.map(async (id) => {
      const state = photoRecord(id)?.state;
      if (state && state.flag !== flag) {
        await persistPhoto(id, { ...state, flag });
      }
    }),
  );
}

export async function applyProfileToPhotos(
  ids: string[],
  profile: Profile,
  amount: number,
): Promise<void> {
  await Promise.all(
    ids.map(async (id) => {
      const state = photoRecord(id)?.state;
      if (!state) {
        return;
      }
      const settings = applyProfile(state.settings, profile.partial, amount);
      const film = profile.film ?? state.film;
      await persistPhoto(id, film ? { ...state, settings, film } : { ...state, settings });
      await refreshThumbnail(id);
    }),
  );
}

function neighbour(root: Element, id: string, direction: 1 | -1): string | undefined {
  const current = tileElement(root, id)?.getBoundingClientRect();
  if (!current) {
    return undefined;
  }
  const centre = current.left + current.width / 2;
  let best: { id: string; row: number; offset: number } | undefined;
  for (const element of root.querySelectorAll<HTMLElement>(tileSelector)) {
    const rect = element.getBoundingClientRect();
    const row = (rect.top - current.top) * direction;
    const offset = Math.abs(rect.left + rect.width / 2 - centre);
    const closer = !best || row < best.row || (row === best.row && offset < best.offset);
    if (row > 1 && closer) {
      best = { id: element.dataset.photoId ?? "", row, offset };
    }
  }
  return best?.id;
}

export function stepFocus(root: Element, key: string): string | undefined {
  const ids = visibleIds(root);
  const focused = ui.get().focused;
  const at = focused ? ids.indexOf(focused) : -1;
  if (at < 0) {
    return ids[0];
  }
  switch (key) {
    case "ArrowLeft":
      return ids[Math.max(0, at - 1)];
    case "ArrowRight":
      return ids[Math.min(ids.length - 1, at + 1)];
    case "ArrowUp":
      return neighbour(root, ids[at] ?? "", -1) ?? ids[at];
    default:
      return neighbour(root, ids[at] ?? "", 1) ?? ids[at];
  }
}
