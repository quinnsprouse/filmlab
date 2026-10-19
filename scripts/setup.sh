#!/bin/sh
set -eu

cd "$(dirname "$0")/.."
git config core.hooksPath .githooks
echo "Git hooks installed from .githooks"
