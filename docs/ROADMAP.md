# FlowClone — Roadmap

## Shipped (0.1 → 0.3.6)

- **Disk discovery** — read-only macOS enumeration via `diskutil -plist` (model,
  serial, SMART health, per-disk used space), event-driven refresh on
  attach/detach, and safe eject.
- **Image Migration** — create a real raw `.flowimg` from a source SSD:
  **Exact** (full bit-for-bit), **Smart** (NTFS used-only via GPT + `$Bitmap`,
  falls back to a full image otherwise), and optional **zstd compression**, with
  accurate live progress, ETA, and image size.
- **Restore Image** — write a `.flowimg` back onto a target SSD; auto-detects v1
  and v2 (sparse and/or compressed) images, typed `ERASE` confirmation, and hard
  target validation (rejects boot, internal, read-only, and too-small disks) plus
  a free-space pre-check sized to the actual payload.
- **Resilience** — auto-reconnect and resume if the disk drops off the bus,
  ddrescue-style bad-block skipping, and crash / power-loss recovery that flags an
  unfinished image on the next launch.
- **Privileged raw I/O** — runs through the bundled `flowclone` CLI behind a macOS
  admin prompt + Full Disk Access (or a Windows UAC prompt).
- **UX** — first-run onboarding, English/Thai, light/dark, desktop notifications,
  a close-while-running confirmation, and locally-signed macOS arm64 `.dmg`
  releases.

## Next (macOS-first)

- **Pause & resume for Image Migration** — create only (unplug the cable while
  paused, resume on the same SSD + enclosure). Restore is intentionally excluded
  (see Not planned).
- **Preferences window** — default theme, default verify, throttle.
- **Verification** — blockwise SHA-256 sampling / full verify (currently stubbed).
- **Performance** — pipelined read/write and optional multithreaded zstd
  (measure the hardware ceiling first).

## Later

- **Direct Clone** (disk-to-disk) re-enabled — currently disabled while Image
  Migration and Restore stabilize.
- **APFS used-only** — sparse imaging for macOS-formatted disks (today Smart is
  NTFS-only).
- **Clone queue** and a richer SMART health surface.

## Platform support

macOS is the primary target. Windows code exists (UAC elevation, raw
`\\.\PHYSICALDRIVE`, volume lock/dismount) but is **not yet built or tested in
CI** — the Windows port (build, CI, release, real-hardware testing) is deferred
until the macOS version is stable. Linux remains a stub.

## Not planned

- **Restore pause/resume** — a mis-resumed write risks data loss or a damaged
  SSD; intentionally out of scope.
- Network clone; sound feedback.

## A note on the "privileged helper"

Raw disk access today works through the bundled CLI + a macOS admin prompt. A
dedicated privileged helper (`native/macos-helper`) is not implemented and is not
required for the current flow.
