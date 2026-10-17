import { exportPlan } from "@/lib/engine/engine";
import { renderPool } from "@/lib/engine/runtime";
import { exportFormats, type ExportOptions } from "@/lib/engine/types";
import { lutsOf, renderFilm } from "@/lib/film/film";
import { ensureLuts } from "@/lib/film/stocks";
import { bytesOf, writeFile } from "@/lib/folder/folder";
import { library, photoRecord } from "@/lib/library/library";
import { limitConcurrency } from "@/lib/store";

const simultaneous = 2;

export interface ExportResult {
  exported: number;
  errors: string[];
}

interface ExportRun {
  ids: string[];
  options: ExportOptions;
  signal: AbortSignal;
  onProgress: (finished: number) => void;
}

function download(name: string, mime: string, bytes: Uint8Array<ArrayBuffer>): void {
  const url = URL.createObjectURL(new Blob([bytes], { type: mime }));
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = name;
  anchor.click();
  setTimeout(() => URL.revokeObjectURL(url), 10_000);
}

async function exportOne(id: string, options: ExportOptions): Promise<void> {
  const record = photoRecord(id);
  if (!record?.probed || !record.state) {
    throw new Error("The photo hasn't finished loading. Wait a moment and try again.");
  }
  const { facts } = record.probed;
  const plan = exportPlan(facts, record.state, options);
  const film = renderFilm(record.state.film);
  await ensureLuts(lutsOf(film));
  const bytes = await bytesOf(record.file.source);
  const encoded = await renderPool().run(
    "loupe",
    {
      kind: "export",
      facts,
      state: record.state,
      options,
      film: film ?? null,
      bytes,
      maxEdge: plan.maxEdge,
    },
    { transfer: [bytes.buffer] },
  );
  const { extension, mime } = exportFormats[options.format];
  const name = `${record.file.stem}.${extension}`;
  const folder = library.get().folder;
  if (folder?.writable) {
    await writeFile(folder, ["Exports"], name, encoded);
  } else {
    download(name, mime, encoded);
  }
}

export async function runExport({
  ids,
  options,
  signal,
  onProgress,
}: ExportRun): Promise<ExportResult> {
  const limit = limitConcurrency(simultaneous);
  const result: ExportResult = { exported: 0, errors: [] };
  let finished = 0;
  await Promise.all(
    ids.map((id) =>
      limit(async () => {
        if (signal.aborted) {
          return;
        }
        try {
          await exportOne(id, options);
          result.exported += 1;
        } catch (error) {
          const reason = error instanceof Error ? error.message : String(error);
          result.errors.push(`${id}: ${reason}`);
        }
        finished += 1;
        onProgress(finished);
      }),
    ),
  );
  return result;
}
