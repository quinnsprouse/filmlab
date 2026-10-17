import { readValue, writeValue } from "@/lib/storage/kv";

export interface RecentFolder {
  name: string;
  handle: FileSystemDirectoryHandle;
  photos: number;
  openedAt: string;
}

const key = "recent-folders";
const limit = 6;

function isRecentFolder(value: unknown): value is RecentFolder {
  return (
    typeof value === "object" &&
    value !== null &&
    "handle" in value &&
    value.handle instanceof FileSystemDirectoryHandle &&
    "name" in value &&
    typeof value.name === "string"
  );
}

export async function recentFolders(): Promise<RecentFolder[]> {
  const stored = await readValue(key);
  return Array.isArray(stored) ? stored.filter(isRecentFolder) : [];
}

export async function rememberFolder(folder: RecentFolder): Promise<void> {
  const known = await recentFolders();
  const same = await Promise.all(known.map((entry) => entry.handle.isSameEntry(folder.handle)));
  const others = known.filter((_, index) => !same[index]);
  await writeValue(key, [folder, ...others].slice(0, limit));
}

export async function reopen(folder: RecentFolder): Promise<boolean> {
  const options = { mode: "readwrite" } as const;
  if ((await folder.handle.queryPermission(options)) === "granted") {
    return true;
  }
  return (await folder.handle.requestPermission(options)) === "granted";
}
