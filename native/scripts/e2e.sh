#!/bin/sh
set -eu

photos=${1:?usage: native/scripts/e2e.sh <photo folder> [output folder]}
out=${2:-$(mktemp -d)}
home="$out/home"
roll="$out/photos"
rm -rf "$home" "$roll" "$out/shots"
mkdir -p "$home/Library/Application Support/Filmlab" "$roll"
for file in "$photos"/*; do
  [ -f "$file" ] && cp -c "$file" "$roll/" 2>/dev/null || cp "$file" "$roll/"
done
printf '{"folder":"%s","photo":null}' "$roll" >"$home/Library/Application Support/Filmlab/state.json"

cd "$(dirname "$0")/.."
cargo build --release --quiet
rm -rf "$out/parity"
HOME="$home" FILMLAB_MCP=0 FILMLAB_E2E=1 FILMLAB_E2E_SHOTS="$out/shots" FILMLAB_E2E_PARITY="$out/parity" ./target/release/filmlab 2>&1 | tee "$out/full.log" | grep '^e2e' | tee "$out/e2e.log"
HOME="$home" FILMLAB_MCP=0 FILMLAB_E2E=parity FILMLAB_CPU=1 FILMLAB_E2E_PARITY="$out/parity" ./target/release/filmlab 2>&1 | tee "$out/parity.log" | grep '^e2e' | tee -a "$out/e2e.log"
rm -rf "$out/parity"
echo "screenshots: $out/shots"
! grep -q ' FAIL ' "$out/e2e.log"
