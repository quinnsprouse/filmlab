import type {
  ExportOptions,
  Facts,
  FilmSettings,
  PhotoState,
  Probed,
  RenderedFrame,
  View,
} from "./types";

interface Indexed {
  probed: Probed;
  state: PhotoState;
  thumbnail: Blob;
}

export type WorkerRequest =
  | { kind: "probe"; name: string; bytes: Uint8Array<ArrayBuffer> }
  | {
      kind: "index";
      name: string;
      bytes: Uint8Array<ArrayBuffer>;
      sidecar: string | null;
      maxEdge: number;
      view: View;
    }
  | { kind: "lut"; id: string; text: string }
  | { kind: "load"; key: string; facts: Facts; bytes: Uint8Array<ArrayBuffer>; maxEdge: number }
  | { kind: "render"; key: string; settings: string; view: View }
  | {
      kind: "thumbnail";
      facts: Facts;
      bytes: Uint8Array<ArrayBuffer>;
      settings: string;
      maxEdge: number;
      view: View;
    }
  | {
      kind: "export";
      facts: Facts;
      state: PhotoState;
      options: ExportOptions;
      film: FilmSettings | null;
      bytes: Uint8Array<ArrayBuffer>;
      maxEdge: number;
    }
  | { kind: "release"; key: string };

export interface WorkerReplies {
  probe: Probed;
  index: Indexed;
  lut: null;
  load: null;
  render: RenderedFrame;
  thumbnail: Blob;
  export: Uint8Array<ArrayBuffer>;
  release: null;
}

export type WorkerMessage =
  | { kind: "init"; module?: WebAssembly.Module; threads?: number }
  | { kind: "job"; id: number; request: WorkerRequest };

export type WorkerReply =
  | { id: number; ok: true; value: WorkerReplies[WorkerRequest["kind"]]; ms: number }
  | { id: number; ok: false; error: string };
