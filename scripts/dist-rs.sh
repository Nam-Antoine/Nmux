#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

winlibs=$(ls -d "${LOCALAPPDATA:-}"/Microsoft/WinGet/Packages/BrechtSanders.WinLibs*/mingw64/bin 2>/dev/null | head -1 || true)
if [ -n "$winlibs" ]; then PATH="$(cygpath -u "$winlibs"):$PATH"; fi

(cd ui && pnpm build)
cargo build --release -p nmux-server
./ui/node_modules/.bin/tauri build --config src-tauri/tauri.bundle.conf.json
ls -la target/release/bundle/nsis/
