import type { WorkerMessage, WorkerReplies, WorkerReply, WorkerRequest } from "./protocol";

type Kind = WorkerRequest["kind"];

interface Job {
  id: number;
  request: WorkerRequest;
  transfer: Transferable[];
  resolve: (value: never) => void;
  reject: (error: Error) => void;
  signal: AbortSignal | undefined;
}

function singleThreadWorker(): Worker {
  return new Worker(new URL("./render.worker.ts", import.meta.url), { type: "module" });
}

function multiThreadWorker(): Worker {
  return new Worker(new URL("./render-mt.worker.ts", import.meta.url), { type: "module" });
}

class Lane {
  private readonly worker: Worker;
  private readonly queue: Job[] = [];
  private running: Job | undefined;

  constructor(worker: Worker, init: WorkerMessage) {
    this.worker = worker;
    this.worker.onmessage = (event: MessageEvent<WorkerReply>) => this.finish(event.data);
    this.worker.onerror = (event) => this.fail(new Error(event.message || "The render worker stopped."));
    this.post(init);
  }

  get load(): number {
    return this.queue.length + (this.running ? 1 : 0);
  }

  submit(job: Job): void {
    this.queue.push(job);
    this.next();
  }

  terminate(): void {
    this.worker.terminate();
    this.fail(new Error("The render worker was closed."));
  }

  private post(message: WorkerMessage, transfer: Transferable[] = []): void {
    this.worker.postMessage(message, transfer);
  }

  private next(): void {
    while (!this.running && this.queue.length > 0) {
      const job = this.queue.shift();
      if (!job) {
        return;
      }
      if (job.signal?.aborted) {
        job.reject(new DOMException("The job was cancelled.", "AbortError"));
        continue;
      }
      this.running = job;
      this.post({ kind: "job", id: job.id, request: job.request }, job.transfer);
    }
  }

  private finish(reply: WorkerReply): void {
    const job = this.running;
    if (!job || job.id !== reply.id) {
      return;
    }
    this.running = undefined;
    if (reply.ok) {
      job.resolve(reply.value as never);
    } else {
      job.reject(new Error(reply.error));
    }
    this.next();
  }

  private fail(error: Error): void {
    const pending = [...(this.running ? [this.running] : []), ...this.queue.splice(0)];
    this.running = undefined;
    for (const job of pending) {
      job.reject(error);
    }
  }
}

export type LaneName = "loupe" | "background";

export class RenderPool {
  private readonly loupe: Lane;
  private readonly background: Lane[];
  private nextId = 1;

  constructor(module: WebAssembly.Module, backgroundLanes: number) {
    this.loupe = canUseThreads()
      ? new Lane(multiThreadWorker(), { kind: "init", threads: renderThreads() })
      : new Lane(singleThreadWorker(), { kind: "init", module });
    this.background = Array.from(
      { length: Math.max(1, backgroundLanes) },
      () => new Lane(singleThreadWorker(), { kind: "init", module }),
    );
  }

  run<K extends Kind>(
    lane: LaneName,
    request: Extract<WorkerRequest, { kind: K }>,
    options: { transfer?: Transferable[]; signal?: AbortSignal } = {},
  ): Promise<WorkerReplies[K]> {
    const target = lane === "loupe" ? this.loupe : this.leastBusy();
    return new Promise<WorkerReplies[K]>((resolve, reject) => {
      target.submit({
        id: this.nextId++,
        request,
        transfer: options.transfer ?? [],
        resolve: resolve as (value: never) => void,
        reject,
        signal: options.signal,
      });
    });
  }

  broadcast<K extends Kind>(request: Extract<WorkerRequest, { kind: K }>): Promise<WorkerReplies[K][]> {
    return Promise.all(
      [this.loupe, ...this.background].map(
        (lane) =>
          new Promise<WorkerReplies[K]>((resolve, reject) => {
            lane.submit({
              id: this.nextId++,
              request,
              transfer: [],
              resolve: resolve as (value: never) => void,
              reject,
              signal: undefined,
            });
          }),
      ),
    );
  }

  close(): void {
    this.loupe.terminate();
    for (const lane of this.background) {
      lane.terminate();
    }
  }

  private leastBusy(): Lane {
    return this.background.reduce((best, lane) => (lane.load < best.load ? lane : best));
  }
}

function cores(): number {
  return typeof navigator === "undefined" ? 4 : navigator.hardwareConcurrency || 4;
}

function canUseThreads(): boolean {
  return typeof crossOriginIsolated !== "undefined" && crossOriginIsolated;
}

function renderThreads(): number {
  return Math.max(1, cores() - 1);
}

export function backgroundLaneCount(): number {
  return Math.min(3, Math.max(1, Math.floor(cores() / 4)));
}
