import { OpenFolderButton } from "./open-folder-button";

export type EmptyKind = "folder" | "picks" | "rejected";

const formats = "RAW files (DNG, CR3, ARW, NEF, RAF and more) and JPEG, PNG, TIFF, WebP or HEIC";

export function EmptyGrid({ kind }: { kind: EmptyKind }) {
  if (kind === "folder") {
    return (
      <div className="flex flex-1 flex-col items-center justify-center gap-m p-l text-center">
        <div className="flex flex-col gap-xs">
          <h2 className="text-m font-heavy text-foreground">No photos in this folder</h2>
          <p className="max-w-[44ch] text-s text-secondary-text">
            Filmlab reads {formats}. Choose a folder that has some.
          </p>
        </div>
        <OpenFolderButton />
      </div>
    );
  }
  return (
    <div className="flex flex-1 items-center justify-center p-l text-center">
      <p className="text-s text-secondary-text">
        {kind === "picks"
          ? "No picks yet. Press P to pick a photo."
          : "No rejects yet. Press X to reject a photo."}
      </p>
    </div>
  );
}
