import {
  applyProfile,
  attachPixels,
  detachPixels,
  openCommands,
  runCommand,
  type WhiteBalanceMode,
} from "@/lib/engine/engine";
import type { DevelopSettings, FilmSettings, OverlayRequest } from "@/lib/engine/types";
import { bytesOf } from "@/lib/folder/folder";
import { library, persistPhoto, photoRecord, refreshThumbnail } from "@/lib/library/library";

import { requestRender } from "./render";
import { ensureSource, keepSources, sourceKey } from "./sources";
import { develop, emptyDevelop, type Compare, type Tool } from "./state";

export { setViewport, setZoom } from "./render";
export { develop } from "./state";
export type { Compare, Tool } from "./state";

const coalesceMs = 900;
const saveDelayMs = 700;

let loadToken = 0;
let saveTimer: ReturnType<typeof setTimeout> | undefined;

function scheduleSave(): void {
  clearTimeout(saveTimer);
  saveTimer = setTimeout(() => void saveNow(), saveDelayMs);
}

async function saveNow(): Promise<void> {
  const { photoId, base, settings, film } = develop.get();
  if (!photoId || !base || !settings) {
    return;
  }
  const { film: _previous, ...rest } = base;
  await persistPhoto(photoId, film ? { ...rest, settings, film } : { ...rest, settings });
  await refreshThumbnail(photoId);
}

export async function openPhoto(id: string): Promise<void> {
  const record = photoRecord(id);
  if (!record?.probed || !record.state) {
    return;
  }
  if (saveTimer) {
    clearTimeout(saveTimer);
    await saveNow();
  }
  loadToken += 1;
  const token = loadToken;
  const { facts, defaults } = record.probed;
  develop.set({
    ...emptyDevelop,
    tool: develop.get().tool,
    photoId: id,
    facts,
    base: record.state,
    before: defaults,
    settings: record.state.settings,
    film: record.state.film,
    history: [
      {
        label: "Opened",
        key: "open",
        settings: record.state.settings,
        film: record.state.film,
        at: Date.now(),
      },
    ],
    status: "loading",
  });
  detachPixels();
  openCommands(facts, record.state.settings);
  keepSources(neighbourKeys(id));
  try {
    await ensureSource(id, "preview");
    if (token !== loadToken) {
      return;
    }
    develop.update((state) => ({ ...state, status: "ready" }));
    requestRender(false);
    void bytesOf(record.file.source).then((pixels) => token === loadToken && attachPixels(pixels));
    schedulePreload(id, token);
  } catch (error) {
    if (token === loadToken) {
      develop.update((state) => ({ ...state, status: "failed", error: String(error) }));
    }
  }
}

function neighbours(id: string): string[] {
  const photos = library.get().photos;
  const index = photos.findIndex((record) => record.file.id === id);
  return [photos[index + 1], photos[index - 1]]
    .filter((record) => record?.probed !== undefined)
    .map((record) => record?.file.id ?? "");
}

function neighbourKeys(id: string): string[] {
  return [
    sourceKey(id, "preview"),
    sourceKey(id, "full"),
    ...neighbours(id).map((other) => sourceKey(other, "preview")),
  ];
}

const preloadDelayMs = 1200;

function schedulePreload(id: string, token: number): void {
  setTimeout(() => {
    if (token !== loadToken) {
      return;
    }
    for (const other of neighbours(id)) {
      void ensureSource(other, "preview").catch(() => undefined);
    }
  }, preloadDelayMs);
}

export interface Change {
  label: string;
  key: string;
  draft: boolean;
}

function commit(
  settings: DevelopSettings,
  { label, key, draft }: Change,
  film: FilmSettings | undefined = develop.get().film,
): void {
  const { facts } = develop.get();
  if (facts && key.startsWith("profile")) {
    openCommands(facts, settings);
  }
  develop.update((state) => {
    const kept = state.history.slice(0, state.cursor + 1);
    const last = kept.at(-1);
    const now = Date.now();
    const step = { label, key, settings, film, at: now };
    const coalesce = last && last.key === key && now - last.at < coalesceMs;
    const history = coalesce ? [...kept.slice(0, -1), step] : [...kept, step];
    return { ...state, settings, film, history, cursor: history.length - 1, compare: "after" };
  });
  requestRender(draft);
  scheduleSave();
}

export function editCommand(
  command: string,
  params: Record<string, unknown>,
  change: Change,
): unknown {
  if (!develop.get().settings) {
    return undefined;
  }
  const { result, settings } = runCommand(command, params);
  commit(settings, change);
  return result;
}

export function editControls(values: Record<string, number>, label: string): void {
  const key = Object.keys(values).toSorted().join(",");
  editCommand("develop.set", { values }, { label, key, draft: true });
}

export function editWhiteBalanceMode(mode: WhiteBalanceMode, label: string): void {
  editCommand("develop.wb", { mode }, { label, key: "wb.mode", draft: false });
}

export function editSettings(partial: DevelopSettings, label: string, key: string): void {
  editCommand("develop.merge", { settings: partial, label }, { label, key, draft: false });
}

export function editFilm(film: FilmSettings, label: string, key: string, draft = false): void {
  const { settings } = develop.get();
  if (!settings) {
    return;
  }
  commit(settings, { label, key, draft }, film);
}

export function applyProfileTo(
  profileId: string,
  partial: DevelopSettings,
  amount: number,
  label: string,
  film?: FilmSettings,
): void {
  const { settings, applied, cursor } = develop.get();
  if (!settings) {
    return;
  }
  const before =
    applied?.profileId === profileId && applied.step === cursor ? applied.before : settings;
  commit(
    applyProfile(before, partial, amount),
    { label, key: `profile:${profileId}`, draft: true },
    film ?? develop.get().film,
  );
  develop.update((state) => ({
    ...state,
    applied: { profileId, amount, before, step: state.cursor },
  }));
}

export function goToStep(index: number): void {
  const step = develop.get().history[index];
  if (!step) {
    return;
  }
  develop.update((state) => ({
    ...state,
    settings: step.settings,
    cursor: index,
    compare: "after",
  }));
  requestRender(false);
  scheduleSave();
}

export function undo(): void {
  goToStep(develop.get().cursor - 1);
}

export function redo(): void {
  goToStep(develop.get().cursor + 1);
}

export function setTool(tool: Tool): void {
  develop.update((state) => ({
    ...state,
    tool,
    overlay: tool === "masks" ? state.overlay : undefined,
  }));
  requestRender(false);
}

export function setOverlay(overlay: OverlayRequest | undefined): void {
  develop.update((state) => ({ ...state, overlay }));
  requestRender(false);
}

export function setCompare(compare: Compare): void {
  develop.update((state) => ({ ...state, compare }));
  requestRender(false);
}

export async function flushEdits(): Promise<void> {
  if (saveTimer) {
    clearTimeout(saveTimer);
    saveTimer = undefined;
    await saveNow();
  }
}
