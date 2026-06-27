# FlowClone - Roadmap

## MVP (current)

Goal: a calm, trustworthy SSD migration flow on macOS with real read-only disk
discovery and mocked writes.

- [x] Monorepo scaffold (Rust workspace + Tauri v2 + React)
- [x] `flowclone-disk` read-only macOS disk discovery
- [x] `flowclone-raw` stub progress engine
- [x] `flowclone-verify` stub verification result
- [x] `flowclone-report` Markdown and JSON model
- [x] `flowclone-core` orchestration, jobs, progress, validation
- [x] Home screen (source/target selection, validation, warning banner)
- [x] Confirmation screen (typed ERASE confirmation, serial/capacity summary)
- [x] Cloning screen (circular progress, throughput, ETA, flow animation)
- [x] Completed screen and report preview
- [x] Image Migration stub (`.flowimg` path selection)
- [x] Real report file export
- [ ] Privileged helper for raw device access on macOS
- [x] Full `diskutil info -plist` parsing (model, serial, SMART health)

## Phase 2 — Polish & reliability

- Resume an interrupted clone
- Robust read-failure handling (retry / abort)
- Disk-removed detection with reconnect instructions
- Sound feedback (single soft success sound)
- Preferences window (theme, default verify, throttle)

## Phase 3 — Beyond MVP (from DESIGN.md)

- Real image file mode (clone to / restore from an image)
- Clone queue
- SMART disk health surface
- Multi-pass / statistical verification modes
- Network clone

## Platform support

macOS is the primary target. Windows and Linux disk backends exist as stubs and
will be filled in once the macOS MVP is stable.
