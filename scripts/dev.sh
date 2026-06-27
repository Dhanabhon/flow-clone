#!/usr/bin/env bash
# Run FlowClone in development: installs deps if needed, then starts Tauri.
set -euo pipefail

cd "$(dirname "$0")/.."

if [ ! -d node_modules ]; then
  echo "› installing JS dependencies"
  pnpm install
fi

echo "› starting FlowClone (Tauri dev)"
pnpm tauri dev
