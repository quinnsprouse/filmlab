import { GpuState, type GpuMessage, type GpuReply } from "./gpu-protocol";

const waitMs = 2000;

export function installGpu(): void {
  const worker = new Worker(new URL("./gpu.worker.ts", import.meta.url), { type: "module" });
  const signal = new Int32Array(new SharedArrayBuffer(4));
  let ready = false;
  let attached: WebAssembly.Memory | undefined;
  const send = (message: GpuMessage) => worker.postMessage(message, []);
  worker.addEventListener("message", (event: MessageEvent<GpuReply>) => {
    const reply = event.data;
    if (reply.kind === "ready") {
      ready = reply.ok;
    } else {
      Object.assign(globalThis, { filmlabGpuError: reply.message });
    }
  });
  send({ kind: "start", signal: signal.buffer });

  const attach = (memory: WebAssembly.Memory) => {
    if (attached !== memory) {
      send({ kind: "attach", memory });
      attached = memory;
    }
  };
  const upload = (memory: WebAssembly.Memory, block: string): void => {
    if (ready) {
      attach(memory);
      send({ kind: "upload", block });
    }
  };

  // why: the engine's Rust code is synchronous, so its worker blocks here while the GPU worker
  // writes the developed pixels straight into the shared wasm memory
  const run = (memory: WebAssembly.Memory, program: string): number => {
    if (!ready) {
      return 0;
    }
    attach(memory);
    Atomics.store(signal, 0, GpuState.Pending);
    send({ kind: "run", program });
    const waited = Atomics.wait(signal, 0, GpuState.Pending, waitMs);
    if (
      waited === "timed-out" &&
      Atomics.compareExchange(signal, 0, GpuState.Pending, GpuState.Abandoned) === GpuState.Pending
    ) {
      ready = false;
      return 0;
    }
    while (Atomics.load(signal, 0) === GpuState.Writing) {
      Atomics.wait(signal, 0, GpuState.Writing, waitMs);
    }
    return Atomics.load(signal, 0) === GpuState.Done ? 1 : 0;
  };
  Object.assign(globalThis, { filmlabGpuRun: run, filmlabGpuUpload: upload });
}
