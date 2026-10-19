Filmlab is a macOS desktop app: a GPUI interface in `native/` over the Rust engine in `engine/`. There is no web client.

## Commands

- `cd native && cargo run --release` starts the app.
- `scripts/check.sh` runs clippy with warnings denied and the tests for both crates. Run it while you work; the pre-push hook runs it.
- `scripts/fixtures.sh` downloads the raw photo the engine's parity test needs.
- `native/scripts/e2e.sh <photo folder>` drives the real app end to end; `node native/scripts/mcp_e2e.mjs <photo folder>` exercises the MCP server against a release build.
- `native/scripts/bundle.sh` builds `Filmlab.app`, a zip and a DMG.
- `scripts/setup.sh` installs the Git hooks after `git init`.

`engine/` and `native/` are separate Cargo workspaces with their own lockfiles and target folders. Run cargo inside the one you're changing; a change to `engine/` also needs `native/` to build.

## Rules

- **Fix the cause the lint names.** Warnings fail too. Never suppress a lint inline. If a minimal example proves a lint wrong for a crate, set it in that crate's `[lints]` table in `Cargo.toml` with a `why:` comment.
- **No comments.** Make the code say it with a clearer name, a smaller function, or a type. A `why:` comment is a last resort, only for a constraint outside the code that you can't fix, such as a driver bug.
- **Test behavior end to end.** Engine behavior gets an integration test in `engine/tests/` against real photos and reference renders; app behavior goes through `native/scripts/e2e.sh` or the MCP end-to-end script.
- **Keep the GPU and CPU film paths identical.** `native/src/film.wgsl` must produce the same pixels as the engine's CPU path; compare with `FILMLAB_CPU=1`.
- **No new docs.** `AGENTS.md` and `README.md` are the only docs. Plans, notes, PRDs, experiments, and generated output go in `.scratch/`.
- **Commits** follow Conventional Commits, `type(scope): description`, with no Co-Authored-By trailer. The commit-msg hook checks both.

## What you need from me

- End every turn where you're blocked on me, or where the next step needs me, with a short "**What I need from you**" section. Number the items, one concrete action each: exactly what to do, where (which app, file, or person), and what to send back. For example, "In Keychain Access, export the Developer ID certificate as a .p12 and send me its path", not "I need signing set up".
- If there are several, put the quickest or most blocking one first, and say what you'll get on with in the meantime.
- If you don't need anything from me, say "Nothing needed from you right now" so I don't have to ask.
- Never bury a request for me in the middle of a long update.
