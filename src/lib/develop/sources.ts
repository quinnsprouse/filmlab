import { renderPool } from "@/lib/engine/runtime";
import { bytesOf } from "@/lib/folder/folder";
import { photoRecord } from "@/lib/library/library";

export type Level = "preview" | "full";

export const previewEdge = 2560;

const loads = new Map<string, Promise<void>>();
const ready = new Set<string>();
const tokens = new Map<string, { current: boolean }>();

export function sourceKey(id: string, level: Level): string {
  return `${id}#${level}`;
}

export function isLoaded(id: string, level: Level): boolean {
  return ready.has(sourceKey(id, level));
}

async function load(id: string, level: Level): Promise<void> {
  const record = photoRecord(id);
  if (!record?.probed) {
    throw new Error(`${id} hasn't been read yet.`);
  }
  const bytes = await bytesOf(record.file.source);
  await renderPool().run(
    "loupe",
    {
      kind: "load",
      key: sourceKey(id, level),
      facts: record.probed.facts,
      bytes,
      maxEdge: level === "full" ? 0 : previewEdge,
    },
    { transfer: [bytes.buffer] },
  );
}

export function ensureSource(id: string, level: Level): Promise<void> {
  const key = sourceKey(id, level);
  const existing = loads.get(key);
  if (existing) {
    return existing;
  }
  const token = { current: true };
  const pending = load(id, level).then(
    () => {
      if (token.current) {
        ready.add(key);
      }
      return undefined;
    },
    (error: unknown) => {
      loads.delete(key);
      throw error;
    },
  );
  tokens.set(key, token);
  loads.set(key, pending);
  return pending;
}

export function keepSources(keep: string[]): void {
  const wanted = new Set(keep);
  for (const key of Array.from(loads.keys())) {
    if (!wanted.has(key)) {
      const token = tokens.get(key);
      if (token) {
        token.current = false;
      }
      tokens.delete(key);
      loads.delete(key);
      ready.delete(key);
      void renderPool().run("loupe", { kind: "release", key });
    }
  }
}
