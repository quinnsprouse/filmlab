#!/bin/sh
set -eu

cd "$(dirname "$0")/.."
scripts/fixtures.sh
node --test '.agents/hooks/*.test.mjs'
for crate in engine native; do
  echo "== $crate: clippy"
  (cd "$crate" && cargo clippy --all-targets --message-format short -- -D warnings)
  echo "== $crate: test"
  (cd "$crate" && cargo test --quiet)
done
