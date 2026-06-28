#!/usr/bin/env bash
#
# Build the `flowclone` CLI as a Tauri sidecar so the bundled app can run
# Image Migration / Restore. Tauri expects `binaries/flowclone-<target-triple>`
# next to tauri.conf.json; this builds the release CLI and copies it there.
#
# Usage:
#   scripts/build-sidecar.sh                      # host triple
#   scripts/build-sidecar.sh aarch64-apple-darwin # one target
#   scripts/build-sidecar.sh universal-apple-darwin   # fat binary via lipo
#
set -euo pipefail
cd "$(dirname "$0")/.."

DEST="apps/desktop/src-tauri/binaries"
mkdir -p "$DEST"

build_one() { # $1 = arch triple
  local triple="$1"
  echo "==> building flowclone for $triple"
  rustup target add "$triple" >/dev/null 2>&1 || true
  cargo build -p flowclone-cli --release --target "$triple"
  cp "target/$triple/release/flowclone" "$DEST/flowclone-$triple"
  echo "    -> $DEST/flowclone-$triple"
}

targets=("$@")
if [ ${#targets[@]} -eq 0 ]; then
  targets=("$(rustc -vV | sed -n 's/host: //p')")
fi

for triple in "${targets[@]}"; do
  if [ "$triple" = "universal-apple-darwin" ]; then
    build_one aarch64-apple-darwin
    build_one x86_64-apple-darwin
    echo "==> creating universal sidecar with lipo"
    lipo -create \
      "$DEST/flowclone-aarch64-apple-darwin" \
      "$DEST/flowclone-x86_64-apple-darwin" \
      -output "$DEST/flowclone-universal-apple-darwin"
    echo "    -> $DEST/flowclone-universal-apple-darwin"
  else
    build_one "$triple"
  fi
done

echo "done. now run e.g.: pnpm tauri build --target ${targets[0]} --bundles app,dmg"
