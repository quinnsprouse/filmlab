import { startGpu, storageBindings, usage, type Gpu } from "./gpu-device";
import {
  GpuState,
  type GpuMessage,
  type GpuProgram,
  type GpuReply,
  type MemoryBlock,
} from "./gpu-protocol";
import { parsed, withKeys } from "./shapes";
import { workerScope } from "./worker-scope";

interface Stored {
  buffer: GPUBuffer;
  words: number;
  holds: string | undefined;
}

const scope = workerScope<GpuMessage, GpuReply>();
const isProgram = withKeys<GpuProgram>("buffers", "copies", "writes", "passes", "reads");
const isBlock = withKeys<MemoryBlock>("id", "at", "len");
const mapRead = 0x1;
const paramsSlot = 512;
const kept: Record<string, number> = { scene: 3, lut: 8, tex: 4 };

let gpu: Gpu | undefined;
let memory: WebAssembly.Memory | undefined;
let signal: Int32Array | undefined;
const stored = new Map<string, Stored>();
let paramsBuffer: GPUBuffer | undefined;
let staging: { buffer: GPUBuffer; bytes: number } | undefined;

function words(block: MemoryBlock): Float32Array<SharedArrayBuffer> {
  if (!memory) {
    throw new Error("The GPU worker has no engine memory.");
  }
  const buffer = memory.buffer;
  if (!(buffer instanceof SharedArrayBuffer)) {
    throw new Error("The engine memory isn't shared, so the GPU can't read it.");
  }
  return new Float32Array(buffer, block.at, block.len);
}

function packed(block: MemoryBlock): Float32Array<ArrayBuffer> {
  const source = words(block);
  const out = new Float32Array((block.len / 3) * 4);
  for (let i = 0, o = 0; i < block.len; i += 3, o += 4) {
    out[o] = source[i] ?? 0;
    out[o + 1] = source[i + 1] ?? 0;
    out[o + 2] = source[i + 2] ?? 0;
  }
  return out;
}

function evict(name: string): void {
  const kind = name.split(":")[0] ?? "";
  const limit = kept[kind];
  if (limit === undefined) {
    return;
  }
  const same = [...stored.keys()].filter((key) => key.startsWith(`${kind}:`));
  for (const key of same.slice(0, Math.max(same.length - limit, 0))) {
    stored.get(key)?.buffer.destroy();
    stored.delete(key);
  }
}

function ensure(g: Gpu, name: string, size: number): Stored {
  const known = stored.get(name);
  if (known && known.words >= size) {
    stored.delete(name);
    stored.set(name, known);
    return known;
  }
  known?.buffer.destroy();
  const fresh: Stored = {
    buffer: g.device.createBuffer({
      size: Math.max(size, 4) * 4,
      usage: usage.storage | usage.copySrc | usage.copyDst,
    }),
    words: Math.max(size, 4),
    holds: undefined,
  };
  stored.set(name, fresh);
  evict(name);
  return fresh;
}

function upload(g: Gpu, name: string, block: MemoryBlock, pack: boolean): void {
  const target = ensure(g, name, pack ? (block.len / 3) * 4 : block.len);
  if (target.holds !== block.id) {
    g.device.queue.writeBuffer(target.buffer, 0, pack ? packed(block) : words(block));
    target.holds = block.id;
  }
}

function paramsFor(g: Gpu, passes: number): GPUBuffer {
  const bytes = Math.max(passes, 1) * paramsSlot;
  if (!paramsBuffer || paramsBuffer.size < bytes) {
    paramsBuffer?.destroy();
    paramsBuffer = g.device.createBuffer({ size: bytes, usage: usage.uniform | usage.copyDst });
  }
  return paramsBuffer;
}

function stagingFor(g: Gpu, bytes: number): GPUBuffer {
  if (!staging || staging.bytes < bytes) {
    staging?.buffer.destroy();
    staging = {
      buffer: g.device.createBuffer({ size: bytes, usage: usage.mapRead | usage.copyDst }),
      bytes,
    };
  }
  return staging.buffer;
}

function bufferOf(name: string): GPUBuffer {
  const found = stored.get(name);
  if (!found) {
    throw new Error(`The GPU program reads ${name}, which isn't on the GPU.`);
  }
  return found.buffer;
}

function settle(state: GpuState): void {
  if (signal) {
    Atomics.store(signal, 0, state);
    Atomics.notify(signal, 0);
  }
}

async function run(g: Gpu, program: GpuProgram): Promise<void> {
  g.device.pushErrorScope("validation");
  for (const spec of program.buffers) {
    if (spec.upload) {
      upload(g, spec.name, spec.upload, spec.pack);
    } else {
      ensure(g, spec.name, spec.words);
    }
  }
  for (const write of program.writes) {
    g.device.queue.writeBuffer(bufferOf(write.buffer), write.offset * 4, words(write.block));
  }
  const params = paramsFor(g, program.passes.length);
  for (const [k, pass] of program.passes.entries()) {
    g.device.queue.writeBuffer(params, k * paramsSlot, new Uint32Array(pass.params));
  }
  const encoder = g.device.createCommandEncoder();
  for (const copy of program.copies) {
    const from = stored.get(copy.from);
    if (!from) {
      throw new Error(`The GPU program copies ${copy.from}, which isn't on the GPU.`);
    }
    encoder.copyBufferToBuffer(from.buffer, 0, bufferOf(copy.to), copy.offset * 4, from.words * 4);
  }
  for (const [k, pass] of program.passes.entries()) {
    const pipeline = g.pipelines.get(pass.entry);
    if (!pipeline) {
      throw new Error(`The film shader has no ${pass.entry} pass.`);
    }
    const bound = new Map(pass.bind);
    const bind = g.device.createBindGroup({
      layout: g.layout,
      entries: [
        { binding: 0, resource: { buffer: params, offset: k * paramsSlot, size: paramsSlot } },
        ...storageBindings.map((binding) => {
          const name = bound.get(binding);
          const buffer = name === undefined ? g.dummies.get(binding) : bufferOf(name);
          if (!buffer) {
            throw new Error(`Binding ${binding} has no buffer.`);
          }
          return { binding, resource: { buffer } };
        }),
      ],
    });
    const compute = encoder.beginComputePass();
    compute.setPipeline(pipeline);
    compute.setBindGroup(0, bind);
    compute.dispatchWorkgroups(Math.max(pass.groups[0], 1), Math.max(pass.groups[1], 1));
    compute.end();
  }
  const total = program.reads.reduce((sum, read) => sum + read.len * 4, 0);
  const out = total > 0 ? stagingFor(g, total) : undefined;
  let offset = 0;
  for (const read of program.reads) {
    if (out) {
      encoder.copyBufferToBuffer(bufferOf(read.buffer), 0, out, offset, read.len * 4);
    }
    offset += read.len * 4;
  }
  g.device.queue.submit([encoder.finish()]);
  const failure = await g.device.popErrorScope();
  if (failure) {
    throw new Error(failure.message);
  }
  if (!out) {
    finishWrite(() => undefined);
    return;
  }
  await out.mapAsync(mapRead, 0, total);
  try {
    finishWrite(() => {
      let at = 0;
      for (const read of program.reads) {
        words({ id: "", at: read.at, len: read.len }).set(
          new Float32Array(out.getMappedRange(at, read.len * 4)),
        );
        at += read.len * 4;
      }
    });
  } finally {
    out.unmap();
  }
}

function finishWrite(write: () => void): void {
  if (
    signal &&
    Atomics.compareExchange(signal, 0, GpuState.Pending, GpuState.Writing) === GpuState.Pending
  ) {
    write();
    settle(GpuState.Done);
  }
}

async function open(): Promise<void> {
  try {
    gpu = await startGpu();
    scope.postMessage({ kind: "ready", ok: gpu !== undefined }, []);
  } catch {
    scope.postMessage({ kind: "ready", ok: false }, []);
  }
}

function fail(error: unknown): void {
  scope.postMessage({ kind: "failed", message: String(error) }, []);
  if (
    signal &&
    Atomics.compareExchange(signal, 0, GpuState.Pending, GpuState.Failed) === GpuState.Pending
  ) {
    Atomics.notify(signal, 0);
  }
}

scope.addEventListener("message", (event) => {
  const message = event.data;
  if (message.kind === "start") {
    signal = new Int32Array(message.signal);
    void open();
    return;
  }
  if (message.kind === "attach") {
    memory = message.memory;
    return;
  }
  const ready = gpu;
  if (message.kind === "upload") {
    const block = parsed(message.block, isBlock);
    if (ready) {
      upload(ready, block.id, block, false);
    }
    return;
  }
  if (!ready) {
    settle(GpuState.Failed);
    return;
  }
  try {
    run(ready, parsed(message.program, isProgram)).catch(fail);
  } catch (error) {
    fail(error);
  }
});
