import { useSyncExternalStore } from "react";

function subscribe(listener: () => void): () => void {
  let query = window.matchMedia(`(resolution: ${window.devicePixelRatio}dppx)`);
  const changed = () => {
    query = window.matchMedia(`(resolution: ${window.devicePixelRatio}dppx)`);
    query.addEventListener("change", changed, { once: true });
    listener();
  };
  query.addEventListener("change", changed, { once: true });
  return () => query.removeEventListener("change", changed);
}

export function usePixelRatio(): number {
  return useSyncExternalStore(
    subscribe,
    () => window.devicePixelRatio,
    () => 1,
  );
}
