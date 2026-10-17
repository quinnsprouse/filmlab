import { useSyncExternalStore } from "react";

import type { Store } from "@/lib/store";

export function useStore<T, S>(store: Store<T>, select: (state: T) => S): S {
  return useSyncExternalStore(
    (listener) => store.subscribe(listener),
    () => select(store.get()),
    () => select(store.get()),
  );
}
