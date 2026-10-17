# Testing

Test what a user can observe, end to end. App code has no unit tests. A unit test pins the shape a function has today, so it breaks on every refactor and costs tokens to rewrite.

**Pick the test:**

- For a page or a user journey, write a Playwright test in `e2e/` with the `page` fixture.
- For a server function or HTTP behavior such as a status code, a header, or CSRF, use the `request` fixture. It calls the built server without a browser, as `e2e/security.spec.ts` does.
- For a lint rule, the lint config, or a hook, write a guardrail test in `lint/` or `.agents/hooks/` that runs the real binary.

**Write browser tests this way:**

- Run `npm run test:e2e:install` once. `npm run test:e2e` builds with `--mode e2e`, which loads `.env.e2e`, then tests the built server.
- Select elements by role or text. Assert that the page threw nothing, with `trackBrowserErrors` from `e2e/smoke.spec.ts`.
- To reach an error that a browser API controls, stub the API with `page.addInitScript`, as the clipboard tests do.
- Add each new route to `e2e/accessibility.spec.ts`. Fix an axe violation instead of turning the rule off.

**Keep guardrail tests fast.** The Stop hook reruns all of them whenever `lint/` or `.agents/hooks/` changes. Start one process for many cases, as `lint/policy.test.ts` does with a single `vp lint` run.

**The check:** change the internals without changing what a user sees. If the test fails, it tested the implementation.

## Hooks that check your work

- `pre-tool-guard.mjs` blocks edits to generated files, new unit tests, new docs outside `docs/agents/` and `gui/`, Git hook bypasses, and package managers other than npm. It asks a human before a force push or a destructive Git command. It matches raw command text, so even a heredoc that mentions `--no-verify` is blocked. Write that content with a file tool.
- `post-edit.mjs` formats, lints, and typechecks each file an edit tool writes. Shell edits skip it.
- `stop-check.mjs` checks your changes before the turn ends and sends you back if they fail.
- Vite+ installs the Git hooks from `.vite-hooks/`. Pre-push runs `npm run check:push`. Don't add Husky.

`npm run test:template` runs the whole starter from a clean copy of the staged files. CI runs it. Stage your changes before you run it locally.
