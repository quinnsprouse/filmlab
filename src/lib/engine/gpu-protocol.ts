export const GpuState = { Pending: 0, Done: 1, Failed: 2, Writing: 3, Abandoned: 4 } as const;
export type GpuState = (typeof GpuState)[keyof typeof GpuState];

export interface MemoryBlock {
  id: string;
  at: number;
  len: number;
}

interface ProgramBuffer {
  name: string;
  words: number;
  upload: MemoryBlock | null;
  pack: boolean;
}

interface ProgramWrite {
  buffer: string;
  offset: number;
  block: MemoryBlock;
}

interface ProgramPass {
  entry: string;
  params: number[];
  bind: [number, string][];
  groups: [number, number];
}

interface ProgramRead {
  buffer: string;
  at: number;
  len: number;
}

interface ProgramCopy {
  from: string;
  to: string;
  offset: number;
}

interface ProgramKeep {
  buffer: string;
  id: string;
  need: boolean;
}

export interface GpuProgram {
  buffers: ProgramBuffer[];
  copies: ProgramCopy[];
  writes: ProgramWrite[];
  passes: ProgramPass[];
  reads: ProgramRead[];
  keep: ProgramKeep[];
}

export type GpuMessage =
  | { kind: "start"; signal: SharedArrayBuffer }
  | { kind: "attach"; memory: WebAssembly.Memory }
  | { kind: "upload"; block: string }
  | { kind: "run"; program: string };

export type GpuReply = { kind: "ready"; ok: boolean } | { kind: "failed"; message: string };
