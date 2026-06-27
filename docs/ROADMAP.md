# FlowClone — Roadmap

## MVP (current)

Goal: a calm, trustworthy single-disk clone on macOS, end to end.

- [x] Monorepo scaffold (Rust workspace + Tauri v2 + React)
- [x] `flowclone-disk` macOS discovery via `diskutil`
- [x] `flowclone-raw` block read/write engine + buffer pool + throttle
- [x] `flowclone-verify` SHA-256 block comparison
- [x] `flowclone-report` Markdown + JSON export
- [x] `flowclone-core` orchestration, jobs, progress, validation
- [x] Home screen (source/target selection, validation, warning banner)
- [ ] Confirmation screen (typed ERASE confirmation, serial/capacity summary)
- [ ] Cloning screen (circular progress, throughput, ETA, flow animation)
- [ ] Verification screen (shield indicator, separate progress)
- [ ] Success screen + report export
- [ ] Privileged helper for raw device access on macOS
- [ ] Full `diskutil info -plist` parsing (model, serial, SMART health)

## Phase 2 — Polish & reliability

- Resume an interrupted clone
- Robust read-failure handling (retry / abort)
- Disk-removed detection with reconnect instructions
- Sound feedback (single soft success sound)
- Preferences window (theme, default verify, throttle)

## Phase 3 — Beyond MVP (from DESIGN.md)

- Image file mode (clone to / restore from an image)
- Clone queue
- SMART disk health surface
- Multi-pass / statistical verification modes
- Network clone

## Platform support

macOS is the primary target. Windows and Linux disk backends exist as stubs and
will be filled in once the macOS MVP is stable.
