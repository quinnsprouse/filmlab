import { useRef, useState, type DragEvent } from "react";

import { Folder01Icon, Icon } from "@/components/icons";
import { Button } from "@/components/ui/button";
import { supportsFolderWrites } from "@/lib/folder/folder";
import { openDroppedItems, openPickedFiles, pickFolder, reopenFolder } from "@/lib/folder/pick";
import type { RecentFolder } from "@/lib/folder/recent";
import { cn } from "@/lib/utils";

import { RecentFolders } from "./shell/recent-folders";

function explain(error: unknown): string {
  if (error instanceof DOMException && error.name === "NotAllowedError") {
    return "Filmlab doesn't have permission to read that folder. Open it again and choose Allow.";
  }
  const reason = error instanceof Error ? error.message : String(error);
  return `Unable to open that folder. ${reason} Try choosing it again.`;
}

export function FirstRun({ ready }: { ready: boolean }) {
  const [message, setMessage] = useState<string | undefined>(undefined);
  const [dragging, setDragging] = useState(false);
  const input = useRef<HTMLInputElement>(null);
  const writes = supportsFolderWrites();

  async function attempt(task: () => Promise<boolean>, fallback?: string) {
    setMessage(undefined);
    try {
      if (!(await task()) && fallback) {
        setMessage(fallback);
      }
    } catch (error) {
      setMessage(explain(error));
    }
  }

  function open() {
    if (!writes) {
      input.current?.click();
      return;
    }
    void attempt(pickFolder);
  }

  function drop(event: DragEvent) {
    event.preventDefault();
    setDragging(false);
    void attempt(
      () => openDroppedItems(event.dataTransfer.items),
      "That didn't contain a folder. Drop a folder, or choose Open folder.",
    );
  }

  function reopen(entry: RecentFolder) {
    void attempt(
      () => reopenFolder(entry),
      "Permission to that folder wasn't granted. Choose Reopen and allow access, or use Open folder.",
    );
  }

  return (
    <main
      aria-busy={!ready}
      className="flex h-full flex-col items-center justify-center overflow-y-auto pb-10"
    >
      <div className="flex w-[560px] flex-col gap-xl">
        <div className="flex flex-col gap-s">
          <span className="font-data text-xxs font-heavy tracking-xl whitespace-pre text-primary">
            ▸1 KODAK 5207
          </span>
          <h1 className="text-xl font-heavy text-foreground">Open a folder of photos</h1>
          <p className="text-m text-pretty text-secondary-text">
            filmlab edits the photos already on your computer. Nothing is uploaded, and your edits
            are saved next to the originals.
          </p>
        </div>
        <div className="flex flex-col gap-s">
          <div
            onDragOver={(event) => {
              event.preventDefault();
              setDragging(true);
            }}
            onDragLeave={() => setDragging(false)}
            onDrop={drop}
            className={cn(
              "flex h-[240px] flex-col items-center justify-center gap-m rounded-l border border-dashed border-border-strong bg-control transition-colors duration-(--motion-duration) ease-(--motion-easing)",
              dragging && "bg-control-hover",
            )}
          >
            <Icon icon={Folder01Icon} size={36} className="text-tertiary-text" />
            <Button disabled={!ready} onClick={open}>
              Open folder
            </Button>
            <p className="text-s font-medium text-tertiary-text">
              {ready ? "or drop a folder here" : "Starting the photo engine…"}
            </p>
            <input
              ref={input}
              type="file"
              hidden
              multiple
              {...{ webkitdirectory: "" }}
              onChange={(event) => {
                const files = event.target.files;
                if (files && files.length > 0) {
                  openPickedFiles(files);
                }
                event.target.value = "";
              }}
            />
          </div>
          <p className="text-center font-data text-xs font-medium text-tertiary-text">
            DNG · CR3 · ARW · NEF · RAF · JPEG
          </p>
          {message && (
            <p role="alert" className="text-center text-s text-foreground">
              {message}
            </p>
          )}
        </div>
        <RecentFolders onReopen={reopen} />
        {!writes && (
          <p className="px-2.5 text-xs text-pretty text-tertiary-text">
            Saving edits into the folder works in Chrome, Edge and Arc. In Safari and Firefox you
            can still edit, and exports download instead.
          </p>
        )}
      </div>
    </main>
  );
}
