# TanStack Start

Load data in route loaders, validate everything that reaches the server, and keep server-only code in `*.server.ts`. Lint enforces most of this. These are the traps it can't see.

- **Loaders run on both sides.** Put secrets and privileged work behind a server function, never in the loader body.
- **Validate at the boundary.** A server function that reads `data` needs `.validator(schema)` with a Zod Mini schema. Search params need one too, in `validateSearch`. Wrap a field in `z.catch(schema, fallback)` when a bad value should fall back instead of throwing.
- **Keep `src/start.ts`.** Its CSRF middleware rejects server-function calls from other sites. Start adds CSRF on its own only while the file doesn't exist, so keep that middleware when you add more.
- **Refresh with `router.invalidate()`** after a mutation, or to retry a failed loader.
- **Secrets come from `process.env` in a `*.server.ts` module**, parsed with a Zod Mini schema the way `src/config/env.ts` parses `clientEnv`. Never put one in a `VITE_*` variable, because Vite inlines those into the client bundle. Add every variable to its schema and to `.env.example`.
- **Each page owns its head.** `HeadContent` keeps every `link`, so never add a canonical link in a child route. Put a preload that one page needs in that page's `head`, not the root's. A same-name `meta` pair such as `theme-color` goes in the root `shellComponent`, because `HeadContent` keeps one `meta` per name.
- **A third-party script needs a CSP change first.** The CSP in `vite.config.ts` has no `script-src`, because hydration uses inline scripts. Add one with a nonce.
- **Leave `verbatimModuleSyntax` off and import protection at `error`.** Both keep server code out of the client bundle.

`src/routes/index.tsx` is the full example. It puts URL state in `loaderDeps`, calls a server function from `src/lib/starter-status.ts` in its loader, and turns a failure into a typed result. Copy its shape.
