export interface WorkerScope<In, Out> {
  addEventListener(type: "message", listener: (event: MessageEvent<In>) => void): void;
  postMessage(message: Out, transfer: Transferable[]): void;
}

function isWorkerScope<In, Out>(value: unknown): value is WorkerScope<In, Out> {
  return (
    typeof value === "object" &&
    value !== null &&
    "postMessage" in value &&
    "addEventListener" in value &&
    !("document" in value)
  );
}

export function workerScope<In, Out>(): WorkerScope<In, Out> {
  const scope: unknown = globalThis;
  if (!isWorkerScope<In, Out>(scope)) {
    throw new Error("This module runs only inside a worker.");
  }
  return scope;
}
