export interface Store<T> {
  get(): T;
  set(next: T): void;
  update(change: (current: T) => T): void;
  subscribe(listener: () => void): () => void;
}

export function createStore<T>(initial: T): Store<T> {
  let state = initial;
  const listeners = new Set<() => void>();
  const set = (next: T) => {
    if (Object.is(next, state)) {
      return;
    }
    state = next;
    for (const listener of listeners) {
      listener();
    }
  };
  return {
    get: () => state,
    set,
    update: (change) => set(change(state)),
    subscribe(listener) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
  };
}

export function limitConcurrency(limit: number): <T>(task: () => Promise<T>) => Promise<T> {
  let active = 0;
  const waiting: (() => void)[] = [];
  const release = () => {
    active -= 1;
    waiting.shift()?.();
  };
  return async (task) => {
    if (active >= limit) {
      await new Promise<void>((resolve) => waiting.push(resolve));
    }
    active += 1;
    try {
      return await task();
    } finally {
      release();
    }
  };
}
