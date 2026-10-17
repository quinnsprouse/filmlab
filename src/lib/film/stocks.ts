import { renderPool } from "@/lib/engine/runtime";
import { readValue, writeValue } from "@/lib/storage/kv";
import { createStore } from "@/lib/store";

export type StockKind = "Negative" | "Print" | "Utility";

export interface Stock {
  id: string;
  kind: StockKind;
  name: string;
  group: string;
}

export interface StocksState {
  installed: boolean;
  folder: FileSystemDirectoryHandle | undefined;
  stocks: Stock[];
  status: "none" | "needsPermission" | "ready" | "failed";
  error: string | undefined;
}

export const stocks = createStore<StocksState>({
  installed: false,
  folder: undefined,
  stocks: [],
  status: "none",
  error: undefined,
});

const key = "film-folder";
const loaded = new Map<string, Promise<void>>();

async function filesIn(
  directory: FileSystemDirectoryHandle,
  path: string[],
): Promise<{ path: string[]; handle: FileSystemFileHandle }[]> {
  const found: { path: string[]; handle: FileSystemFileHandle }[] = [];
  const nested: Promise<{ path: string[]; handle: FileSystemFileHandle }[]>[] = [];
  for await (const entry of directory.values()) {
    if (entry.name.startsWith(".")) {
      continue;
    }
    if (entry.kind === "directory") {
      nested.push(filesIn(entry, [...path, entry.name]));
    } else if (entry.name.toLowerCase().endsWith(".cube")) {
      found.push({ path: [...path, entry.name], handle: entry });
    }
  }
  return [...found, ...(await Promise.all(nested)).flat()];
}

function stockOf(path: string[]): Stock | undefined {
  const [kind, ...rest] = path;
  if (kind !== "Negative" && kind !== "Print" && kind !== "Utility") {
    return undefined;
  }
  const file = rest.at(-1) ?? "";
  const name = file
    .replace(/\.cube$/i, "")
    .replace(/^Print /, "")
    .replace(/^\d+- /, "");
  const group = rest.length > 1 ? (rest[0] ?? "").replace(/^[IV]+- /, "") : "";
  return { id: path.join("/").replace(/\.cube$/i, ""), kind, name, group };
}

async function scan(folder: FileSystemDirectoryHandle): Promise<void> {
  const files = await filesIn(folder, []);
  const found = files.map((file) => stockOf(file.path)).filter((stock) => stock !== undefined);
  if (found.length === 0) {
    stocks.set({
      installed: false,
      folder,
      stocks: [],
      status: "failed",
      error:
        "No FilmVision LUTs found. Choose the “FilmVision Pro” folder that holds Negative and Print.",
    });
    return;
  }
  stocks.set({
    installed: false,
    folder,
    stocks: byName(found),
    status: "ready",
    error: undefined,
  });
}

function byName(found: Stock[]): Stock[] {
  return found.toSorted((a, b) => a.id.localeCompare(b.id, undefined, { numeric: true }));
}

async function installedStocks(): Promise<boolean> {
  const response = await fetch("/filmvision/index.json").catch(() => undefined);
  if (!response?.ok) {
    return false;
  }
  const ids: unknown = await response.json();
  const found = (Array.isArray(ids) ? ids : [])
    .filter((id): id is string => typeof id === "string")
    .map((id) => stockOf(`${id}.cube`.split("/")))
    .filter((stock) => stock !== undefined);
  if (found.length === 0) {
    return false;
  }
  stocks.set({
    installed: true,
    folder: undefined,
    stocks: byName(found),
    status: "ready",
    error: undefined,
  });
  return true;
}

export async function restoreFilmFolder(): Promise<void> {
  if (await installedStocks()) {
    return;
  }
  const folder = await readValue(key);
  if (!(folder instanceof FileSystemDirectoryHandle)) {
    return;
  }
  if ((await folder.queryPermission({ mode: "read" })) !== "granted") {
    stocks.set({
      installed: false,
      folder,
      stocks: [],
      status: "needsPermission",
      error: undefined,
    });
    return;
  }
  await scan(folder);
}

export async function connectFilmFolder(): Promise<void> {
  const existing = stocks.get().folder;
  if (existing && stocks.get().status === "needsPermission") {
    if ((await existing.requestPermission({ mode: "read" })) === "granted") {
      await scan(existing);
      return;
    }
  }
  const folder = await window.showDirectoryPicker({ id: "filmvision", mode: "read" });
  await writeValue(key, folder);
  await scan(folder);
}

async function readLut(id: string): Promise<string> {
  if (stocks.get().installed) {
    const response = await fetch(
      `/filmvision/${id.split("/").map(encodeURIComponent).join("/")}.cube`,
    );
    if (!response.ok) {
      throw new Error(`Unable to load the film stock ${id}. ${await response.text()}`);
    }
    return response.text();
  }
  const folder = stocks.get().folder;
  if (!folder) {
    throw new Error("Connect your FilmVision Pro folder to use film stocks.");
  }
  const parts = `${id}.cube`.split("/");
  const name = parts.pop() ?? "";
  const directory = await parts.reduce(
    async (parent, part) => (await parent).getDirectoryHandle(part),
    Promise.resolve(folder),
  );
  return (await (await directory.getFileHandle(name)).getFile()).text();
}

async function loadLut(id: string): Promise<void> {
  const text = await readLut(id);
  await renderPool().broadcast({ kind: "lut", id, text });
}

export async function ensureLuts(ids: string[]): Promise<void> {
  await Promise.all(
    ids.map((id) => {
      let pending = loaded.get(id);
      if (!pending) {
        pending = loadLut(id);
        pending.catch(() => loaded.delete(id));
        loaded.set(id, pending);
      }
      return pending;
    }),
  );
}
