import { openFolder } from "@/lib/library/library";

import { folderFromFiles, folderFromHandle, photosIn, supportsFolderWrites } from "./folder";
import { rememberFolder, reopen, type RecentFolder } from "./recent";

async function openHandle(handle: FileSystemDirectoryHandle): Promise<void> {
  const folder = await folderFromHandle(handle);
  openFolder(folder);
  await rememberFolder({
    name: folder.name,
    handle,
    photos: photosIn(folder).length,
    openedAt: new Date().toISOString(),
  });
}

export async function pickFolder(): Promise<boolean> {
  if (!supportsFolderWrites()) {
    return false;
  }
  try {
    const handle = await window.showDirectoryPicker({
      id: "filmlab-folder",
      mode: "readwrite",
      startIn: "pictures",
    });
    await openHandle(handle);
    return true;
  } catch (error) {
    if (error instanceof DOMException && error.name === "AbortError") {
      return false;
    }
    throw error;
  }
}

export async function openDroppedItems(items: DataTransferItemList): Promise<boolean> {
  const pending = [...items]
    .filter((item) => item.kind === "file" && "getAsFileSystemHandle" in item)
    .map((item) => item.getAsFileSystemHandle());
  const handles = await Promise.all(pending);
  const folder = handles.find((handle) => handle instanceof FileSystemDirectoryHandle);
  if (!(folder instanceof FileSystemDirectoryHandle)) {
    return false;
  }
  await openHandle(folder);
  return true;
}

export function openPickedFiles(files: FileList | File[]): void {
  openFolder(folderFromFiles([...files]));
}

export async function reopenFolder(folder: RecentFolder): Promise<boolean> {
  if (!(await reopen(folder))) {
    return false;
  }
  await openHandle(folder.handle);
  return true;
}
