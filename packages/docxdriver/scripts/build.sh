#!/usr/bin/env bash
# Build the docxdriver npm package: WASM runtime, then TypeScript.
# Output (wasm/, dist/) is generated locally and ignored by Git.
set -euo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"
repo="$(cd "$here/../.." && pwd)"

bash "$repo/scripts/build-wasm.sh"

(cd "$here" && npx tsc -p tsconfig.json)
