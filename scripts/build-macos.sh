#!/usr/bin/env bash
# Build a release .app / .dmg for macOS.
set -euo pipefail

cd "$(dirname "$0")/.."

echo "› building desktop app (release)"
pnpm tauri build

echo "› artifacts in apps/desktop/src-tauri/target/release/bundle/"
