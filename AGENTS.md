Agent-ready starter: Vite+, TanStack Start, React 19, shadcn/ui on Base UI, Tailwind v4, and Nitro.

## Commands

- `npm run dev` starts the dev server on localhost:3000.
- `npm run check` runs format, lint, types, and guardrail tests. Run it while you work.
- `npm run check:push` adds the build, dead-code check, and Playwright. The pre-push hook runs it.
- `npm run check:ci` adds a clean template install, React Doctor, and `npm audit`.
- `npm run setup` reinstalls the Git hooks after `git init`.

## Rules

- **Fix the cause the lint error names.** Warnings fail too. Never suppress a rule inline. If a minimal example proves a rule wrong for a file, add an exception to `lint.overrides` in `vite.config.ts` with a `why:` comment.
- **No comments.** Make the code say it with a clearer name, a smaller function, or a type. A `why:` comment is a last resort, only for a constraint outside the code that you can't fix, such as a browser bug.
- **No unit tests in app code.** Test behavior end to end with Playwright in `e2e/`.
- **UI work follows `GUI.md`.** Use the `graphical-ui`, `graphical-convert`, or `graphical-audit` skill as the task needs.
- **No new docs.** `docs/agents/` holds the only guides, and `GUI.md` with `gui/` holds the theme guidance. Plans, notes, PRDs, experiments, and generated output go in `.scratch/`.
- **Commits** follow Conventional Commits, `type(scope): description`, with no Co-Authored-By trailer.

## What you need from me

- End every turn where you're blocked on me, or where the next step needs me, with a short "**What I need from you**" section. Number the items, one concrete action each: exactly what to do, where (which site, app, file, or person), and what to send back. For example, "In Vercel, add `VITE_APP_URL` under **Environment Variables**, redeploy, and send me the deployment URL", not "I need the production config".
- If there are several, put the quickest or most blocking one first, and say what you'll get on with in the meantime.
- If you don't need anything from me, say "Nothing needed from you right now" so I don't have to ask.
- Never bury a request for me in the middle of a long update.

## Guides

Open one when the task touches it:

- [TanStack Start](docs/agents/TANSTACK_START.md): loaders, server functions, secrets, and the document head.
- [React](docs/agents/REACT_PATTERNS.md): effects, state, and components.
- [UI and motion](docs/agents/UI_MOTION.md): components, color tokens, icons, and animation.
- [Testing](docs/agents/TESTING.md): which test to write, and the hooks that check your work.
- [Lint rules](docs/agents/LINT_RULES.md): comments, where code goes, and errors that need more context.
- [Vite+](docs/agents/VITE_PLUS.md): the `vp` toolchain and its traps.
- [Deploy](docs/agents/DEPLOY.md): Node, Vercel, and Cloudflare Workers.
