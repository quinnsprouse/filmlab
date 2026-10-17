const rawExtensions = new Set([
  "dng",
  "cr3",
  "cr2",
  "arw",
  "nef",
  "nrw",
  "raf",
  "rw2",
  "rwl",
  "orf",
  "pef",
  "srw",
  "erf",
  "3fr",
  "iiq",
]);
const imageExtensions = new Set(["jpg", "jpeg", "png", "tif", "tiff", "webp", "heic", "heif"]);

export interface PhotoFile {
  id: string;
  name: string;
  stem: string;
  raw: boolean;
  source: File;
  companion: File | undefined;
  sidecar: File | undefined;
}

export interface Folder {
  name: string;
  handle: FileSystemDirectoryHandle | undefined;
  files: File[];
  writable: boolean;
}

function extensionOf(name: string): string {
  const dot = name.lastIndexOf(".");
  return dot > 0 ? name.slice(dot + 1).toLowerCase() : "";
}

function stemOf(name: string): string {
  const dot = name.lastIndexOf(".");
  return dot > 0 ? name.slice(0, dot) : name;
}

export function supportsFolderWrites(): boolean {
  return typeof window !== "undefined" && "showDirectoryPicker" in window;
}

async function filesIn(handle: FileSystemDirectoryHandle): Promise<File[]> {
  const files: File[] = [];
  for await (const entry of handle.values()) {
    if (entry.kind === "file" && !entry.name.startsWith(".")) {
      files.push(await entry.getFile());
    }
  }
  return files;
}

export async function folderFromHandle(handle: FileSystemDirectoryHandle): Promise<Folder> {
  return { name: handle.name, handle, files: await filesIn(handle), writable: true };
}

export function folderFromFiles(files: File[]): Folder {
  const first = files[0]?.webkitRelativePath ?? "";
  const name = first.split("/")[0] || "Folder";
  const topLevel = files.filter(
    (file) => file.webkitRelativePath.split("/").length <= 2 && !file.name.startsWith("."),
  );
  return { name, handle: undefined, files: topLevel, writable: false };
}

export function photosIn(folder: Folder): PhotoFile[] {
  const byStem = new Map<string, File[]>();
  for (const file of folder.files) {
    const key = stemOf(file.name).toLowerCase();
    byStem.set(key, [...(byStem.get(key) ?? []), file]);
  }
  const photos: PhotoFile[] = [];
  for (const group of byStem.values()) {
    const raw = group.find((file) => rawExtensions.has(extensionOf(file.name)));
    const image = group.find((file) => imageExtensions.has(extensionOf(file.name)));
    const source = raw ?? image;
    if (!source) {
      continue;
    }
    const sidecar = group.find((file) => extensionOf(file.name) === "xmp");
    photos.push({
      id: source.name,
      name: source.name,
      stem: stemOf(source.name),
      raw: source === raw,
      source,
      companion: raw && image ? image : undefined,
      sidecar,
    });
  }
  return photos.toSorted((a, b) => a.name.localeCompare(b.name, undefined, { numeric: true }));
}

export async function bytesOf(file: File): Promise<Uint8Array<ArrayBuffer>> {
  return new Uint8Array(await file.arrayBuffer());
}

export async function writeFile(
  folder: Folder,
  path: string[],
  name: string,
  contents: BlobPart,
): Promise<File> {
  if (!folder.handle) {
    throw new Error(
      "This browser can only read the folder. Use Chrome, Edge or Arc to save into it.",
    );
  }
  const directory = await path.reduce(
    async (parent, part) => (await parent).getDirectoryHandle(part, { create: true }),
    Promise.resolve(folder.handle),
  );
  const handle = await directory.getFileHandle(name, { create: true });
  const writable = await handle.createWritable();
  await writable.write(contents);
  await writable.close();
  return handle.getFile();
}

export function sidecarName(photo: PhotoFile): string {
  return photo.sidecar?.name ?? `${photo.stem}.xmp`;
}
