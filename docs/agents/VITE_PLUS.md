# Vite+

`vp` wraps Vite, Rolldown, Vitest, Oxlint, Oxfmt, and Vite Task. Run the gates through `npm run check*`. For anything else, use `vp dev`, `vp lint`, `vp fmt`, or `vp test`.

- **Import from `vite-plus`.** Config comes from `vite-plus`, test helpers from `vite-plus/test`, and the lint plugin API from `vite-plus/lint/plugins`.
- **Don't install the bundled tools.** Vite+ ships oxlint, oxfmt, `@oxlint/plugins`, vitest, and tsdown. A second copy drifts from its version.
- **Cached gates see only `verificationInputs`.** The gates are tasks in `run.tasks` in `vite.config.ts`. Add any new config or hook file to `verificationInputs`, or the cache returns a stale pass.
- **The `vitest` override must match.** `package.json` pins the Vitest that Vite+ bundles, and `scripts/check-toolchain.mjs` fails until the two agree. Update Vite+ with `vp migrate` on a branch.
- **`vp dev` is the dev server, not the `dev` script.** Run a package script with `vp run <name>`. There is no `vp vitest` or `vp oxlint`.
