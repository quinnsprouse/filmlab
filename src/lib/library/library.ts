import { filmOf, writeSidecar } from "@/lib/engine/engine";
import { renderPool } from "@/lib/engine/runtime";
import type { PhotoState, Probed } from "@/lib/engine/types";
import { lutsOf, renderFilm } from "@/lib/film/film";
import { ensureLuts } from "@/lib/film/stocks";
import {
  bytesOf,
  photosIn,
  sidecarName,
  writeFile,
  type Folder,
  type PhotoFile,
} from "@/lib/folder/folder";
import { createStore, limitConcurrency } from "@/lib/store";

export interface PhotoRecord {
  file: PhotoFile;
  probed: Probed | undefined;
  state: PhotoState | undefined;
  thumbnail: string | undefined;
  error: string | undefined;
}

export interface LibraryState {
  folder: Folder | undefined;
  photos: PhotoRecord[];
  saving: "idle" | "saving" | "saved" | "readOnly" | "failed";
  saveError: string | undefined;
}

export const library = createStore<LibraryState>({
  folder: undefined,
  photos: [],
  saving: "idle",
  saveError: undefined,
});

const thumbnailEdge = 480;
const thumbnailSource = 512;

let generation = 0;

function update(id: string, change: (record: PhotoRecord) => PhotoRecord): void {
  library.update((state) => ({
    ...state,
    photos: state.photos.map((record) => (record.file.id === id ? change(record) : record)),
  }));
}

export function photoRecord(id: string): PhotoRecord | undefined {
  return library.get().photos.find((record) => record.file.id === id);
}

function replaceThumbnail(id: string, blob: Blob): void {
  const url = URL.createObjectURL(blob);
  update(id, (record) => {
    if (record.thumbnail) {
      URL.revokeObjectURL(record.thumbnail);
    }
    return { ...record, thumbnail: url };
  });
}

async function indexPhoto(file: PhotoFile, run: number): Promise<void> {
  const [bytes, sidecar] = await Promise.all([bytesOf(file.source), file.sidecar?.text()]);
  if (run !== generation) {
    return;
  }
  await ensureLuts(lutsOf(sidecar ? filmOf(sidecar) : undefined)).catch(() => undefined);
  const indexed = await renderPool().run(
    "background",
    {
      kind: "index",
      name: file.name,
      bytes,
      sidecar: sidecar ?? null,
      maxEdge: thumbnailSource,
      view: { maxW: thumbnailEdge, maxH: thumbnailEdge },
    },
    { transfer: [bytes.buffer] },
  );
  if (run !== generation) {
    return;
  }
  update(file.id, (record) => ({ ...record, probed: indexed.probed, state: indexed.state }));
  replaceThumbnail(file.id, indexed.thumbnail);
}

export function openFolder(folder: Folder): void {
  generation += 1;
  const run = generation;
  for (const record of library.get().photos) {
    if (record.thumbnail) {
      URL.revokeObjectURL(record.thumbnail);
    }
  }
  const photos = photosIn(folder).map((file) => ({
    file,
    probed: undefined,
    state: undefined,
    thumbnail: undefined,
    error: undefined,
  }));
  library.set({
    folder,
    photos,
    saving: folder.writable ? "idle" : "readOnly",
    saveError: undefined,
  });
  const limit = limitConcurrency(3);
  for (const { file } of photos) {
    void limit(() => indexPhoto(file, run)).catch((error: unknown) =>
      update(file.id, (record) => ({ ...record, error: String(error) })),
    );
  }
}

export async function refreshThumbnail(id: string): Promise<void> {
  const record = photoRecord(id);
  if (!record?.probed || !record.state) {
    return;
  }
  const film = renderFilm(record.state.film);
  await ensureLuts(lutsOf(film));
  const bytes = await bytesOf(record.file.source);
  const blob = await renderPool().run(
    "background",
    {
      kind: "thumbnail",
      facts: record.probed.facts,
      bytes,
      settings: JSON.stringify(record.state.settings),
      maxEdge: thumbnailSource,
      view: { maxW: thumbnailEdge, maxH: thumbnailEdge, ...(film ? { film } : {}) },
    },
    { transfer: [bytes.buffer] },
  );
  replaceThumbnail(id, blob);
}

function setPhotoState(id: string, state: PhotoState): void {
  update(id, (record) => ({ ...record, state }));
}

async function saveSidecar(record: PhotoRecord, state: PhotoState): Promise<void> {
  const folder = library.get().folder;
  if (!folder?.writable || !record.probed) {
    return;
  }
  const existing = record.file.sidecar ? await record.file.sidecar.text() : null;
  const text = writeSidecar(record.probed.facts, state, existing);
  const written = await writeFile(folder, [], sidecarName(record.file), text);
  update(record.file.id, (current) => ({
    ...current,
    file: { ...current.file, sidecar: written },
  }));
}

export async function persistPhoto(id: string, state: PhotoState): Promise<void> {
  setPhotoState(id, state);
  const record = photoRecord(id);
  if (!record || !library.get().folder?.writable) {
    return;
  }
  library.update((current) => ({ ...current, saving: "saving" }));
  try {
    await saveSidecar(record, state);
    library.update((current) => ({ ...current, saving: "saved", saveError: undefined }));
  } catch (error) {
    library.update((current) => ({ ...current, saving: "failed", saveError: String(error) }));
  }
}
