#!/bin/sh
set -eu

cd "$(dirname "$0")/.."
raw=engine/tests/fixtures/.cache/ricoh-gr3.dng
url='https://raw.pixls.us/getfile.php/3115/nice/Ricoh%20-%20GR%20III%20-%2014bit%20%283:2%29.DNG'
sha=05513ee72f7cac4165534c9f64995412084f414a033c80c473c79fd94154292e

matches() {
  echo "$sha  $1" | shasum -a 256 -c --status 2>/dev/null
}

if [ -f "$raw" ] && matches "$raw"; then
  exit 0
fi
mkdir -p "$(dirname "$raw")"
curl -fsSL "$url" -o "$raw.part"
if ! matches "$raw.part"; then
  echo "fixtures: $url no longer has sha256 $sha" >&2
  exit 1
fi
mv "$raw.part" "$raw"
