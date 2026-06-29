# Changelog

All notable changes to FlowClone are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **`.flowimg` v2 format + compression (CLI)** — `create-image --compress`
  writes a v2 image whose payload is a single zstd stream, producing a much
  smaller file on compressible disks. Restore auto-detects v1 vs v2 and
  decompresses transparently; existing v1 images still restore unchanged. This
  is the first slice of the sparse-image work (see `docs/SPARSE_IMAGE.md`);
  filesystem-aware "used-only" imaging follows in a later phase. The GUI does not
  expose compression yet.

## [0.1.0] - 2026-06-28

First release intended for trying real workflows: **Image Migration** and
**Restore Image** both work end-to-end on macOS (CLI and GUI). This release
introduces the first destructive operation (Restore), gated behind validation,
typed `ERASE` confirmation, and an admin prompt.

### Added

- **Restore Image** — write a `.flowimg` back onto a target disk. The GUI runs
  the trusted `flowclone` CLI behind a macOS admin prompt (`restore-image
  --confirm-erase`) with live progress, speed, and ETA. Refuses boot, internal,
  read-only, and too-small targets.
- **Skip unreadable blocks** when imaging (ddrescue-style): retry once, then
  zero-fill and record the bad region in `<image>.badblocks.txt` instead of
  aborting — so a single bad sector doesn't kill the whole image.
- **`.flowimg` document icon** — declares a document type and exported UTI so a
  built, registered `.app` shows the FlowClone icon on `.flowimg` files.
- **Bundled CLI sidecar** — the built `.app` ships the `flowclone` CLI (via
  `bundle.externalBin`), so Image Migration and Restore work in a distributed
  app. Build it with `scripts/build-sidecar.sh` (or `pnpm sidecar`) before
  `tauri build`.

### Changed

- **Direct Clone is temporarily disabled** in the UI (shown with a "coming soon"
  tooltip) while imaging and restore are stabilized.
- Restore tolerates `ENOTTY` from flushing the unbuffered raw device (writes are
  already durable), so a successful restore no longer reports failure.

### Fixed

- `pnpm install` (run by `tauri dev`) no longer fails on pnpm 11 — esbuild's
  build script is approved via `allowBuilds`.

## [0.0.1] - 2026-06-28

Initial tracked release. FlowClone is a Tauri v2 desktop app for SSD migration on
macOS, with a React/TypeScript UI and a Rust workspace behind it. Phase 1 ships
no destructive writes — disk detection is read-only and direct clone/restore are
still stubbed.

### Added

- **Disk detection** — read-only macOS enumeration via `diskutil`, with per-disk
  usage aggregated from mounted volumes (APFS `CapacityInUse`, others via `df`).
- **Event-driven disk refresh** — a native DiskArbitration watcher emits
  `disks://changed` on attach/detach so the disk list updates instantly instead
  of polling, with a 30s fallback refresh.
- **Image Migration** — create a `.flowimg` raw image from a source SSD. The GUI
  runs the trusted `flowclone` CLI behind a native macOS admin prompt for the
  privileged raw read, unmounts the source first, and shows live progress,
  speed, and ETA.
- **Interruption handling** — automatic resume after a disk drops off the bus
  (re-acquire by serial, seek, continue), an "Interrupted — reconnecting" status,
  a clear interrupted-migration screen, and power-loss recovery that flags an
  unfinished image on the next launch.
- **Cancellation** — image creation can be cancelled (with confirmation); the
  elevated copy is stopped via a sentinel file and the partial file is removed.
- **Direct Clone** and **Restore Image** workflows (validation + UI; the actual
  writes are stubbed in Phase 1).
- **App menu** — "About FlowClone" with the app logo, plus a "Check For
  Update…" placeholder; standard Edit/Window menus.
- **UI** — four-screen flow, English/Thai localization, light/dark themes.

### Safety

- Hard validation (same-device, target-too-small, missing source/target, boot
  disk as target) lives in the Rust core, not the UI.
- Destructive actions require typed `ERASE` confirmation.

[Unreleased]: https://github.com/Dhanabhon/flow-clone/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/Dhanabhon/flow-clone/compare/v0.0.1...v0.1.0
[0.0.1]: https://github.com/Dhanabhon/flow-clone/releases/tag/v0.0.1
