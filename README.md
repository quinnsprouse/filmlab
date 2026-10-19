# Filmlab

Filmlab is a macOS photo editor that makes raw files look like film. It develops each raw with LightCraft's
pipeline, prints it through FilmVision Pro's negative and print LUTs, and adds grain, halation and dust.

You open a folder and edit in place. Edits go into XMP sidecars next to the originals, and exports land in an
`Exports` folder beside them. The interface is GPUI with Liquid Glass, and the film stage runs on the GPU
through wgpu and Metal.

## Requirements

- macOS 26.
- [mise](https://mise.jdx.dev), which installs the Rust and Node versions pinned in `mise.toml`.
- FilmVision Pro installed in DaVinci Resolve, if you want film stocks.

Filmlab reads stocks from `/Library/Application Support/Blackmagic Design/DaVinci Resolve/LUT/FilmVision Pro`.
Set `FILMVISION_DIR` to read them from somewhere else. FilmVision Pro has its own license, so this repo doesn't
include the LUTs. Without them Filmlab still develops photos, and the stock list stays empty.

## Build and run

```sh
mise install
scripts/setup.sh
cd native && cargo run --release
```

`scripts/setup.sh` points Git at the hooks in `.githooks`.

## Check

`scripts/check.sh` runs clippy with warnings denied and the tests for both crates. The pre-push hook runs it too.

`engine/tests/parity.rs` exports a Ricoh GR III raw to PNG and compares it with LightCraft desktop's render of the
same settings. Every pixel has to match. `scripts/fixtures.sh` downloads that raw from
[raw.pixls.us](https://raw.pixls.us), where it's released as CC0, and checks its SHA-256.

`native/scripts/e2e.sh <photo folder>` runs the app's built-in end-to-end script on a copy of a folder and saves
screenshots.

## Package

`native/scripts/bundle.sh` builds `Filmlab.app`, a zip and a DMG in `native/target-dist/bundle`. Set
`FILMLAB_SIGN` to your signing identity, or the script signs the app ad hoc.

## Agents

While Filmlab runs, it serves MCP at `http://127.0.0.1:47428/mcp`. An agent can open folders and photos, change
controls and film stocks, preview, analyze and export. Each change is one undo step and goes into the sidecar,
the same as an edit by hand. The server only listens on loopback and refuses requests that carry a browser
`Origin` header.

To add it to Claude Code:

```sh
claude mcp add --transport http filmlab http://127.0.0.1:47428/mcp
```

Set `FILMLAB_MCP_PORT` to move it, or `FILMLAB_MCP=0` to turn it off. `node native/scripts/mcp_e2e.mjs <photo folder>`
tests the tools against a release build.

## Layout

- `engine/` wraps LightCraft and adds the film stage. `engine/vendor/` holds LightCraft crates with Filmlab's changes.
- `native/` is the app. `native/src/film.wgsl` is the GPU film stage, and `native/src/mcp/` is the MCP server.
- `scripts/` holds the check, the fixture download and the hook setup.

`engine/` and `native/` are separate Cargo workspaces with their own lockfiles. Run cargo inside the one you're
changing.

## Contributing

Read [AGENTS.md](AGENTS.md) first. It's written for coding agents, and the rules apply to people too. Commits
follow Conventional Commits, like `fix(native): keep the crop inside the frame`, with no Co-Authored-By trailer.
The commit-msg hook checks both, and `scripts/check.sh` has to pass.

## License

Filmlab is under the [MIT license](LICENSE). The vendored LightCraft crates are MIT OR Apache-2.0. Open Runde, the
interface font, ships with its SIL Open Font License 1.1 in `native/assets/fonts/LICENSE.txt`. The Hugeicons icons
are MIT, with their notice in `native/assets/icons/LICENSE.md`.
