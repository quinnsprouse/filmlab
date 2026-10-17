# React

Lint enforces the rules of hooks and effect dependencies, and React Compiler handles memoization. What's left is judgment.

- **Effects sync with the outside world.** Use one for a timer, a subscription, or an imperative browser API, and for nothing else. Derive values during render, handle actions in event handlers, and load data in loaders.
- **Cleanup undoes setup.** Strict Mode runs both twice in development, so both must be safe to repeat. Ignore a stale async result in cleanup.
- **Browser state goes through `useSyncExternalStore`**, with a server snapshot that matches hydration. See `src/hooks/use-prefers-reduced-motion.ts`.
- **Reset state with a `key`** when the entity changes: `<Editor key={doc.id} initialValue={doc.content} />`.
- **Skip `useMemo`, `useCallback`, and `memo` for speed.** The compiler already memoizes. Use them only when an API needs a stable identity.
- **Compose with `children`** instead of growing a list of config props. Replace booleans for mutually exclusive states with one variant prop.

**The test:** if you can delete the effect and compute the value during render, delete the effect.
