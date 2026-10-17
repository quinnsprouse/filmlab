# Filmlab

A film photo editor that runs in the browser and edits the photos already on your computer. Open a folder,
develop with Lightroom-style controls, and print through FilmVision Pro film stocks. Nothing is uploaded: edits
are saved as XMP sidecars next to the originals, and exports go to an `Exports` folder beside them.

The image math is [LightCraft](https://github.com/storytold/lightcraft)'s own Rust pipeline compiled to
WebAssembly, so renders match LightCraft desktop (exactly on native; within one level on ~0.002 % of values in
the browser, from the browser's math library). The film stage adds FilmVision's negative and print LUTs after
LightCraft's scene-referred stages.

## Requirements

- Node 24 and npm 11 (see `package.json`).
- Rust via mise: `mise install rust@1.99.1`, plus `rustup target add wasm32-unknown-unknown` and
  `rustup toolchain install nightly --component rust-src --target wasm32-unknown-unknown` for the threaded build.
- `cargo install wasm-bindgen-cli --version 0.2.129 --locked`, and binaryen (`brew install binaryen`).
- Chrome, Edge or Arc to save into folders (Safari and Firefox can open a folder read-only).
- FilmVision Pro installed in DaVinci Resolve for film stocks. The app reads them from
  `/Library/Application Support/Blackmagic Design/DaVinci Resolve/LUT/FilmVision Pro` (override with
  `FILMVISION_DIR`); they are never copied into the project.

## Develop

```bash
npm ci
npm run build:engine   # wasm engine: engine/pkg (single-threaded) and engine/pkg-mt (threads)
npm run dev            # http://localhost:3000
```

`npm run check` runs format, lint, types and guardrail tests; `npm run check:push` adds the build, dead-code check
and the Playwright suite, including a byte-parity test against a LightCraft desktop render.

## Layout

- `engine/`: the Rust crate wrapping LightCraft (decode, render, export, XMP, LightCraft's command layer) and the
  film stage; `engine/vendor/lightcraft-pipeline` is LightCraft's pipeline with a tone-map hook for film.
- `src/lib/`: engine workers, folder access, library, develop session, profiles, film stocks.
- `src/components/darkroom/`: Browse, Develop (DialKit panels, crop, heal, masks), top bar, first run.
- `e2e/`: Playwright tests.

## License

MIT for this project. LightCraft is MIT OR Apache-2.0 (`engine/vendor/lightcraft-pipeline/LICENSE-*`).
FilmVision Pro LUTs are licensed separately and not included.
