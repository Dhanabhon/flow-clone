# Changelog

All notable changes to FlowClone are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

[Unreleased]: https://github.com/Dhanabhon/flow-clone/compare/v0.0.1...HEAD
[0.0.1]: https://github.com/Dhanabhon/flow-clone/releases/tag/v0.0.1
