import init, { Engine, initThreadPool, probe, readSidecar } from "@engine-mt/engine.js";
import wasmUrl from "@engine-mt/engine_bg.wasm?url";

import { installGpu } from "./gpu-bridge";
import type { WorkerMessage, WorkerReply } from "./protocol";
import { runJob } from "./worker-jobs";
import { workerScope } from "./worker-scope";

const scope = workerScope<WorkerMessage, WorkerReply>();
installGpu();

async function start(threads: number): Promise<Engine> {
  await init({ module_or_path: wasmUrl });
  await initThreadPool(threads);
  return new Engine();
}

let engine: Promise<Engine> | undefined;

scope.addEventListener("message", (event) => {
  const message = event.data;
  if (message.kind === "init") {
    engine ??= start(message.threads ?? 1);
    return;
  }
  const ready = engine;
  if (!ready) {
    scope.postMessage(
      {
        id: message.id,
        ok: false,
        error: "The render worker started before its engine.",
      },
      [],
    );
    return;
  }
  void ready
    .then((current) => runJob(current, { probe, readSidecar }, message.id, message.request))
    .then(([reply, transfer]) => scope.postMessage(reply, transfer));
});
