import type { WorkerReplies, WorkerReply, WorkerRequest } from "./protocol";
import type { PhotoState, Probed, RenderedFrame } from "./types";

interface FrameLike {
  readonly width: number;
  readonly height: number;
  takeRgba(): Uint8Array;
  takeHistogram(): Uint32Array;
  free(): void;
}

interface EngineLike {
  load(key: string, facts: string, bytes: Uint8Array, maxEdge: number): void;
  release(key: string): void;
  render(key: string, settings: string, view: string): FrameLike;
  export(key: string, facts: string, state: string, options: string, film: string): Uint8Array;
  loadLut(id: string, text: string): void;
  hasLut(id: string): boolean;
}

export interface Bindings {
  probe(name: string, bytes: Uint8Array): string;
  readSidecar(facts: string, xmp: string): string;
}

type Engine = EngineLike;

function frameOf(e: Engine, key: string, settings: string, view: string): RenderedFrame {
  const frame = e.render(key, settings, view);
  const rgba = frame.takeRgba();
  const histogram = frame.takeHistogram();
  const result = {
    width: frame.width,
    height: frame.height,
    rgba: new Uint8ClampedArray(rgba.buffer as ArrayBuffer, rgba.byteOffset, rgba.byteLength),
    histogram: new Uint32Array(histogram.buffer as ArrayBuffer, histogram.byteOffset, histogram.length),
  };
  frame.free();
  return result;
}

async function jpegOf(frame: RenderedFrame): Promise<Blob> {
  const canvas = new OffscreenCanvas(frame.width, frame.height);
  const context = canvas.getContext("2d");
  if (!context) {
    throw new Error("This browser can't draw thumbnails off screen.");
  }
  context.putImageData(new ImageData(frame.rgba, frame.width, frame.height), 0, 0);
  return canvas.convertToBlob({ type: "image/jpeg", quality: 0.86 });
}

const scratchKey = "scratch";

function withScratch<T>(
  e: Engine,
  request: { facts: unknown; bytes: Uint8Array<ArrayBuffer>; maxEdge: number },
  use: () => T,
): T {
  e.load(scratchKey, JSON.stringify(request.facts), request.bytes, request.maxEdge);
  try {
    return use();
  } finally {
    e.release(scratchKey);
  }
}

function stateOf(bindings: Bindings, probed: Probed, sidecar: string | null): PhotoState {
  const xmp = sidecar ?? probed.embeddedXmp;
  if (xmp) {
    return JSON.parse(bindings.readSidecar(JSON.stringify(probed.facts), xmp));
  }
  return { settings: probed.defaults, rating: 0, flag: "none", label: null };
}

async function run(
  e: Engine,
  bindings: Bindings,
  request: WorkerRequest,
): Promise<[WorkerReplies[WorkerRequest["kind"]], Transferable[]]> {
  switch (request.kind) {
    case "probe":
      return [JSON.parse(bindings.probe(request.name, request.bytes)), []];
    case "index": {
      const probed: Probed = JSON.parse(bindings.probe(request.name, request.bytes));
      const state = stateOf(bindings, probed, request.sidecar);
      const view = state.film ? { ...request.view, film: state.film } : request.view;
      const frame = withScratch(e, { ...request, facts: probed.facts }, () =>
        frameOf(e, scratchKey, JSON.stringify(state.settings), JSON.stringify(view)),
      );
      return [{ probed, state, thumbnail: await jpegOf(frame) }, []];
    }
    case "lut":
      if (!e.hasLut(request.id)) {
        e.loadLut(request.id, request.text);
      }
      return [null, []];
    case "load":
      e.load(request.key, JSON.stringify(request.facts), request.bytes, request.maxEdge);
      return [null, []];
    case "render": {
      const frame = frameOf(e, request.key, request.settings, JSON.stringify(request.view));
      return [frame, [frame.rgba.buffer, frame.histogram.buffer]];
    }
    case "thumbnail": {
      const frame = withScratch(e, request, () =>
        frameOf(e, scratchKey, request.settings, JSON.stringify(request.view)),
      );
      return [await jpegOf(frame), []];
    }
    case "export": {
      const bytes = withScratch(e, request, () =>
        e.export(
          scratchKey,
          JSON.stringify(request.facts),
          JSON.stringify(request.state),
          JSON.stringify(request.options),
          JSON.stringify(request.film),
        ),
      );
      const owned = new Uint8Array(bytes.buffer as ArrayBuffer, bytes.byteOffset, bytes.byteLength);
      return [owned, [owned.buffer]];
    }
    case "release":
      e.release(request.key);
      return [null, []];
  }
}

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export async function runJob(
  engine: Engine,
  bindings: Bindings,
  id: number,
  request: WorkerRequest,
): Promise<[WorkerReply, Transferable[]]> {
  const started = performance.now();
  try {
    const [value, transfer] = await run(engine, bindings, request);
    return [{ id, ok: true, value, ms: performance.now() - started }, transfer];
  } catch (error) {
    return [{ id, ok: false, error: messageOf(error) }, []];
  }
}
