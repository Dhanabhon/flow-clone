# Changelog

All notable changes to FlowClone are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.3] - 2026-06-29

### Fixed

- **The "Disk Access Required" screen no longer exposes developer details in a
  packaged app.** Production previously showed the raw error chain (`os error 1`,
  `/dev/rdiskN`…) and a copy-able `sudo ./target/debug/flowclone …` command meant
  only for development. Production builds now show plain guidance — grant Full
  Disk Access to FlowClone, then quit and reopen — with **Open Full Disk Access**
  and **Check Again** buttons. The developer workaround still appears in
  `tauri dev`.

### Documentation

- README shows a **version badge** that tracks the latest GitHub release.
- User guide: added **"Restoring onto a brand-new or larger SSD"** — the macOS
  "disk not readable" → **Ignore** prompt, the target size requirement, expanding
  the partition after restoring onto a bigger disk, and slight byte differences
  between same-nominal-size drives.

## [0.3.2] - 2026-06-29

### Fixed

- **Used-only / compressed images could not be restored or verified** — they
  failed with "stream did not contain valid UTF-8". The desktop image validator
  only understood the v1 format, so any v2 image (`--used-only` and/or
  `--compress`) fell through to reading the whole binary file as text. Because
  **restore runs the same validator as a pre-flight check, Smart/Compress images
  could not be restored from the GUI at all** — it errored before the CLI ran.
  The validator now understands v2 (header, mode/block-map consistency,
  compression); the CLI restore engine already handled v2.
- **Accurate progress for Smart/Compress images.** The cloning screen showed a
  stuck low percentage, a ~60-minute ETA, and a 256 GB image size for a used-only
  job that actually stores ~51 GB. The GUI estimated progress from the growing
  `.part` file over the full disk size, which is meaningless when the payload is
  sparse or zstd-compressed. The CLI now publishes a progress file with the real
  bytes-stored / total-to-store and the GUI reads it, so percentage, ETA, and the
  image size reflect the used data.
- **The progress ring was invisible in light mode** — its track color matched the
  white card background. It now uses a visible border color in both themes.
- The desktop app no longer shows the WebView's right-click menu (Reload / Back /
  Forward); editable fields keep Cut/Copy/Paste.

### Added

- **Live "GB written" readout** under the progress percentage (e.g.
  "15 GB / 51 GB"), updating in real time.
- **Image filenames carry the chosen settings** — the suggested `.flowimg` name
  includes `exact`/`smart` and `zstd` (e.g.
  `FlowClone-<timestamp>-smart-zstd.flowimg`) so images are distinguishable in
  Finder. It reflects intent; the header remains the source of truth.
- **Interruption modal.** If the source disk drops off mid-migration, a centered
  dialog reports it and FlowClone auto-resumes when the drive returns; if it
  can't recover, the dialog offers to start over.

### Changed

- **Larger default window** (1200×780, min 1000×680) so the 1100 px max content
  width designed for the UI is fully usable.
- **Clearer mode icons.** Image Migration and Restore use a drive with an up /
  down arrow (read off the disk vs. write onto it); Direct Clone uses a small
  drive and a larger drive with an arrow between them, suggesting an upgrade to a
  bigger SSD.

## [0.3.1] - 2026-06-29

### Fixed

- **`--used-only` now works on real macOS disks.** The GPT/NTFS parsers did
  small, unaligned reads, but macOS raw devices (`/dev/rdiskN`) only allow
  whole-sector, sector-aligned reads — so used-only always failed at detection
  ("no GPT found") and fell back to a full image, even on a Windows/NTFS disk.
  Detection now uses one aligned read and the parsers read through an
  alignment-buffering wrapper. (The in-memory tests allowed any read and missed
  this; a sector-aligned-only reader mock now guards it.)
- The Full Disk Access fallback's copy-able CLI command now includes the
  selected **Smart** (`--used-only`) and **Compress** (`--compress`) options.
  They were stored only in the home screen, so the cloning screen built the
  command without them — running it produced a full, uncompressed image. The
  options now live in the shared flow store.
- `--used-only` now reports a **permission/Full Disk Access** failure directly,
  instead of the misleading "used-only unavailable; writing a full image" — the
  full image needs the same access and would fail identically, so it fails fast
  with a clear message. (Non-permission cases, e.g. a non-NTFS disk, still fall
  back to a full image.)

## [0.3.0] - 2026-06-29

### Added

- **Image Migration options in the GUI.** A **Smart / Exact** toggle and a
  **Compress** switch, with a live size/time estimate and a recommendation hint.
  Smart stores only used blocks (NTFS, falling back to a full image otherwise);
  Compress writes a zstd-compressed `.flowimg`. The default is Exact (the proven
  full-copy path); both are opt-in.
- **NTFS used-only imaging: `create-image --used-only`.** Reads the GPT and each
  NTFS partition's `$Bitmap` and stores only the allocated blocks — much faster
  and smaller on a mostly-empty disk (e.g. ~50 GB used on a 256 GB drive). The v2
  format carries a `block_map`; restore reconstructs the disk (present blocks from
  the payload, absent blocks as zeros). Biased to include — GPT, gaps, non-NTFS
  partitions, and anything that fails to parse are kept whole — and it falls back
  to a full image whenever the disk isn't GPT/NTFS. Combines with `--compress`.
  Covered by NTFS-parsing and create→restore round-trip tests.
- **Job-done notification.** A desktop notification fires when a migration or
  restore finishes (success or failure).
- **Close-while-running confirmation.** Quitting the app during a migration or
  restore now asks before interrupting the job.

## [0.2.1] - 2026-06-29

### Fixed

- **Eject** now works on USB enclosures that keep their device node after
  ejecting. It trusts `diskutil`'s success instead of waiting for the disk to
  disappear (which never happens on those enclosures), force-unmounts a busy
  volume and retries, and hides the ejected disk from the list so the card
  actually disappears — no more false "still in use" error.

### Changed

- Tightened the hero tagline and subtitle copy (English and Thai).

## [0.2.0] - 2026-06-29

### Added

- **Eject** — external disk cards now have an eject button that safely powers
  down the drive (macOS `diskutil eject`; Windows "Safely Remove") so it can be
  unplugged without a separate tool.
- **`.flowimg` v2 format + compression (CLI)** — `create-image --compress`
  writes a v2 image whose payload is a single zstd stream, producing a much
  smaller file on compressible disks. Restore auto-detects v1 vs v2 and
  decompresses transparently; existing v1 images still restore unchanged. This
  is the first slice of the sparse-image work (see `docs/SPARSE_IMAGE.md`);
  filesystem-aware "used-only" imaging follows in a later phase. The GUI does not
  expose compression yet.
- **User guide** — `docs/USER_GUIDE.md` covers install, imaging, restore, eject,
  the `.flowimg` file, and troubleshooting.

### Fixed

- **Dark-mode controls were unreadable** — the floating controls toolbar stayed
  light in dark mode while its text turned near-white. The text is now dark and
  readable, and the color tokens were reworked so opacity-based styles (badge and
  banner tints) render instead of collapsing to transparent.
- **macOS app menu** — the About, Hide, and Quit items showed the crate name
  ("flowclone-desktop"); they now read "About FlowClone", "Hide FlowClone", and
  "Quit".
- **Restore wording** — removed stale copy claiming the restore step "does not
  write to disk"; it does, and the text now reflects that.

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

[Unreleased]: https://github.com/Dhanabhon/flow-clone/compare/v0.3.3...HEAD
[0.3.3]: https://github.com/Dhanabhon/flow-clone/compare/v0.3.2...v0.3.3
[0.3.2]: https://github.com/Dhanabhon/flow-clone/compare/v0.3.1...v0.3.2
[0.3.1]: https://github.com/Dhanabhon/flow-clone/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/Dhanabhon/flow-clone/compare/v0.2.1...v0.3.0
[0.2.1]: https://github.com/Dhanabhon/flow-clone/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/Dhanabhon/flow-clone/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/Dhanabhon/flow-clone/compare/v0.0.1...v0.1.0
[0.0.1]: https://github.com/Dhanabhon/flow-clone/releases/tag/v0.0.1
