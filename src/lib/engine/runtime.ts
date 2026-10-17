import { bootEngine } from "./engine";
import { RenderPool, backgroundLaneCount } from "./pool";

let pool: RenderPool | undefined;
let starting: Promise<RenderPool> | undefined;

export function startRuntime(): Promise<RenderPool> {
  starting ??= bootEngine().then((module) => {
    pool = new RenderPool(module, backgroundLaneCount());
    return pool;
  });
  return starting;
}

export function renderPool(): RenderPool {
  if (!pool) {
    throw new Error("The render engine hasn't started yet.");
  }
  return pool;
}
