# Filmlab

A film photo editor that runs in the browser and edits the photos already on your computer. Open a folder,
develop with Lightroom-style controls, and print through FilmVision Pro film stocks. Nothing is uploaded: edits
are saved as XMP sidecars next to the originals, and exports go to an `Exports` folder beside them.

The image math is [LightCraft](https://github.com/storytold/lightcraft)'s own Rust pipeline compiled to
WebAssembly, so renders match LightCraft desktop (exactly on native; within one level on ~0.002 % of values in
the browser, from the browser's math library). The film stage adds FilmVision's negative and print LUTs after
LightCraft's scene-referred stages, with FilmVision's Texture node between them: its tone and detail
operator, its saturation roll-off, film blur, dust specks, image defocus and grain (`engine/src/texture.rs`,
`engine/src/dust.rs`, `engine/src/grain.rs`). These were measured from Resolve renders of FilmVision Pro; the
deterministic parts match Resolve to about one 16-bit level, dust to within a few levels on common frame
shapes, and grain matches its statistics (Resolve's random generator can't be reproduced).

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

## How previews stay fast

- Edits render at full quality in a threaded wasm worker, newest edit first; stale ones are dropped.
- The film stage keeps the scene before the film, its scan levels and Texture output, and the filmed
  pixels, so an edit only reruns what it changes (a colour edit skips the film entirely).
- The develop view keeps a fit render of the whole photo and, when zoomed, a full-resolution render
  of the visible window. Zoom and pan move both instantly; the window re-renders when it settles.
- The film stage runs on the GPU when the browser has WebGPU (`src/lib/engine/film.wgsl`, driven by
  `gpu.worker.ts`). Every operation is forced to round on its own and divisions are corrected, so
  its pixels match the CPU path bit for bit; without a GPU the CPU path runs instead.
- `cargo run --release --example pipeline_bench -- <photo> <negative.cube> <print.cube>` (in
  `engine/`) times a run of edits natively and prints a hash of each frame, so speed work can prove
  it changed no pixels.

## License

MIT for this project. LightCraft is MIT OR Apache-2.0 (`engine/vendor/lightcraft-pipeline/LICENSE-*`).
FilmVision Pro LUTs are licensed separately and not included.
