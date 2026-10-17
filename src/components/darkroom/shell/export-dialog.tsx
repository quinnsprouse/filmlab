import { Dialog } from "@base-ui/react/dialog";
import { useRef, useState } from "react";

import { Button } from "@/components/ui/button";
import { useStore } from "@/hooks/use-store";
import { flushEdits } from "@/lib/develop/session";
import type { ExportOptions } from "@/lib/engine/types";
import { library } from "@/lib/library/library";

import { ExportForm, type ExportChoices } from "./export-form";
import { runExport, type ExportResult } from "./export-run";
import { exportDialog, closeExport } from "./export-state";
import { useExportIds } from "./use-export-ids";

type Run =
  | { phase: "idle" }
  | { phase: "running"; finished: number; total: number }
  | { phase: "done"; result: ExportResult; cancelled: boolean; total: number };

const defaults: ExportChoices = { format: "jpeg", size: "full", edge: 2048, quality: 90 };

function plural(count: number): string {
  return count === 1 ? "1 photo" : `${count} photos`;
}

function optionsFrom(choices: ExportChoices): ExportOptions {
  const base: ExportOptions = { format: choices.format, quality: choices.quality };
  return choices.size === "edge"
    ? { ...base, resize: { mode: "longEdge", value: choices.edge } }
    : base;
}

export function ExportDialog() {
  const open = useStore(exportDialog, (state) => state);
  const writable = useStore(library, (state) => state.folder?.writable === true);
  const folderName = useStore(library, (state) => state.folder?.name);
  const ids = useExportIds();
  const [choices, setChoices] = useState(defaults);
  const [run, setRun] = useState<Run>({ phase: "idle" });
  const abort = useRef<AbortController | undefined>(undefined);

  const destination = writable ? `Saved to ${folderName}/Exports` : "Downloads";

  async function start() {
    const controller = new AbortController();
    abort.current = controller;
    const total = ids.length;
    setRun({ phase: "running", finished: 0, total });
    await flushEdits();
    const result = await runExport({
      ids,
      options: optionsFrom(choices),
      signal: controller.signal,
      onProgress: (finished) => setRun({ phase: "running", finished, total }),
    });
    setRun({ phase: "done", result, cancelled: controller.signal.aborted, total });
  }

  function change(next: boolean) {
    if (next) {
      exportDialog.set(true);
      return;
    }
    abort.current?.abort();
    closeExport();
    setRun({ phase: "idle" });
  }

  return (
    <Dialog.Root open={open} onOpenChange={change}>
      <Dialog.Portal>
        <Dialog.Backdrop className="fixed inset-0 bg-canvas/70" />
        <Dialog.Popup className="fixed top-1/2 left-1/2 w-[440px] -translate-x-1/2 -translate-y-1/2 rounded-l bg-panel p-l text-foreground ring ring-border-strong outline-none">
          <Dialog.Title className="mb-m text-m font-heavy">
            Export {plural(run.phase === "idle" ? ids.length : run.total)}
          </Dialog.Title>
          {run.phase === "idle" && (
            <ExportForm choices={choices} destination={destination} onChange={setChoices} />
          )}
          {run.phase === "running" && (
            <output className="block text-s text-secondary-text">
              Exporting {Math.min(run.finished + 1, run.total)} of {run.total}…
            </output>
          )}
          {run.phase === "done" && (
            <output className="flex flex-col gap-xs text-s text-secondary-text">
              <p>
                {run.cancelled ? "Stopped after " : "Exported "}
                {plural(run.result.exported)} to {writable ? "Exports" : "Downloads"}
              </p>
              {run.result.errors.map((message) => (
                <p key={message} className="text-xs text-tertiary-text">
                  {message}
                </p>
              ))}
            </output>
          )}
          <div className="mt-l flex justify-end gap-xs">
            {run.phase === "idle" && (
              <>
                <Button variant="ghost" onClick={() => change(false)}>
                  Cancel
                </Button>
                <Button disabled={ids.length === 0} onClick={() => void start()}>
                  Export {plural(ids.length)}
                </Button>
              </>
            )}
            {run.phase === "running" && (
              <Button variant="outline" onClick={() => abort.current?.abort()}>
                Cancel
              </Button>
            )}
            {run.phase === "done" && <Button onClick={() => change(false)}>Done</Button>}
          </div>
        </Dialog.Popup>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
