#!/usr/bin/env bash
# Generate TypeScript types from Rust crates (placeholder).
#
# Once `ts-rs` is wired into the relevant crates, this will compile them with
# the TS export feature and collect the output into packages/shared-types.
set -euo pipefail

cd "$(dirname "$0")/.."

echo "› type generation not wired up yet (see packages/shared-types/README.md)"
echo "  for now, types are hand-mirrored in packages/shared-types/src/index.ts"
