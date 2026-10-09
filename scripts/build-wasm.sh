#!/usr/bin/env bash
# Generate the runtime for both npm packages. Outputs are ignored by Git.
set -euo pipefail
repo="$(cd "$(dirname "$0")/.." && pwd)"
out="$repo/packages/docxdriver/wasm"

# npm may prepend its own installation directory ahead of rustup's proxies.
if [[ -x "${CARGO_HOME:-$HOME/.cargo}/bin/rustup" ]]; then
  export PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH"
fi

# Rust embeds source locations in panic strings, including dependency paths.
# Remap them so distributed artifacts do not expose the build host.
export RUSTFLAGS="${RUSTFLAGS:-} --remap-path-prefix=$repo=/workspace --remap-path-prefix=${CARGO_HOME:-$HOME/.cargo}=/cargo --remap-path-prefix=${RUSTUP_HOME:-$HOME/.rustup}=/rustup --remap-path-prefix=$HOME=/build-home"

wasm-pack build "$repo/crates/docxdriver-wasm" \
  --target web --release --out-dir "$out" --out-name docxdriver

# The nested manifest would make Node interpret the ESM glue as CommonJS.
rm -f "$out"/package.json "$out"/.gitignore "$out"/README.md "$out"/LICENSE*
mkdir -p "$repo/packages/docxdriver-pi/wasm"
cp "$out"/docxdriver.js "$out"/docxdriver.d.ts \
  "$out"/docxdriver_bg.wasm "$out"/docxdriver_bg.wasm.d.ts \
  "$repo/packages/docxdriver-pi/wasm/"
