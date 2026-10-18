import { Engine, initSync, probe, readSidecar } from "@engine/engine.js";

import type { WorkerMessage, WorkerReply } from "./protocol";
import { runJob } from "./worker-jobs";
import { workerScope } from "./worker-scope";

const scope = workerScope<WorkerMessage, WorkerReply>();
let engine: Engine | undefined;

scope.addEventListener("message", (event) => {
  const message = event.data;
  if (message.kind === "init") {
    if (message.module) {
      initSync({ module: message.module });
      engine = new Engine();
    }
    return;
  }
  const current = engine;
  if (!current) {
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
  void runJob(current, { probe, readSidecar }, message.id, message.request).then(
    ([reply, transfer]) => scope.postMessage(reply, transfer),
  );
});
