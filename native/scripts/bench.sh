#!/bin/sh
set -eu

# Times what a user waits for (launch, indexing, opening and switching photos, edits, zoom) on a copy of a photo
# folder, with an empty cache and a fresh HOME, and prints the "bench" lines.
photos=${1:?usage: native/scripts/bench.sh <photo folder> [output folder]}
out=${2:-$(mktemp -d)}
home="$out/home"
roll="$out/photos"
rm -rf "$home" "$roll"
mkdir -p "$home/Library/Application Support/Filmlab" "$roll"
for file in "$photos"/*; do
  [ -f "$file" ] && cp -c "$file" "$roll/" 2>/dev/null || cp "$file" "$roll/"
done
printf '{"folder":"%s","photo":null}' "$roll" >"$home/Library/Application Support/Filmlab/state.json"

cd "$(dirname "$0")/.."
cargo build --release --quiet
HOME="$home" FILMLAB_MCP=0 FILMLAB_BENCH=1 ./target/release/filmlab 2>&1 | tee "$out/full.log" | grep '^bench'
