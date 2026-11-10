#!/usr/bin/env bash
# Builds the Rust Nmux installer into target/release/bundle/nsis/.
#
# The product is called "Nmux Rust" while the Electron Nmux is still the
# installed app, so the two can sit side by side.
set -euo pipefail
cd "$(dirname "$0")/.."

# This machine builds with the GNU toolchain; its MinGW-w64 must come before
# any older MinGW on PATH.
winlibs=$(ls -d "${LOCALAPPDATA:-}"/Microsoft/WinGet/Packages/BrechtSanders.WinLibs*/mingw64/bin 2>/dev/null | head -1 || true)
if [ -n "$winlibs" ]; then PATH="$(cygpath -u "$winlibs"):$PATH"; fi

(cd ui && pnpm exec tsc --noEmit && pnpm exec vite build)
cargo build --release -p nmux-server
./ui/node_modules/.bin/tauri build --config src-tauri/tauri.bundle.conf.json
ls -la target/release/bundle/nsis/
