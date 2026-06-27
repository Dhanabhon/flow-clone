# FlowClone — Architecture

> The UI is a presenter. The Rust core orchestrates everything.

```
┌─────────────────────────────────────────────────────────────┐
│  React UI  (apps/desktop)                                   │
│  presenter only — never clones directly                     │
└───────────────────────────┬─────────────────────────────────┘
                            │ Tauri commands + events
┌───────────────────────────▼─────────────────────────────────┐
│  Tauri Command Layer  (apps/desktop/src-tauri)               │
│  thin marshalling — no business logic                       │
└───────────────────────────┬─────────────────────────────────┘
                            │
┌───────────────────────────▼─────────────────────────────────┐
│  flowclone-core  ← the most important module                │
│  validate · create job · start clone · emit progress        │
│  run verification · generate report                         │
└───┬───────────────┬──────────────────┬──────────────┬───────┘
    │               │                  │              │
    ▼               ▼                  ▼              ▼
 flowclone-disk  flowclone-raw   flowclone-verify  flowclone-report
 discovery       read/write      checksum/compare  markdown/json
```

## Why this layering

FlowClone performs **destructive disk operations**. The architecture exists
to make unsafe states unrepresentable:

- **UI cannot clone.** React only renders state and invokes typed Tauri
  commands. There is no path from a button to a raw `write()` that bypasses
  the core.
- **Core owns ordering & safety.** `CloneEngine::run` is the single entry
  point. It enforces validation (`CloneRequest::validate`), drives the raw
  copy, runs verification, and emits progress. Callers cannot skip a stage.
- **Dependency direction is one-way.** `core` depends on the engine crates;
  the engine crates never depend on `core`. The raw crate reports progress
  through a `ProgressSink` trait that `core` adapts to its own emitter, so
  there are no cyclic dependencies.

## Crates

| Crate | Responsibility |
| --- | --- |
| `flowclone-core` | Job lifecycle, validation, orchestration, progress events |
| `flowclone-disk` | Disk discovery & metadata (per-platform backends) |
| `flowclone-raw` | Raw block read/write engine + buffer pool + throttle |
| `flowclone-verify` | Post-clone verification (SHA-256 block compare) |
| `flowclone-report` | Markdown / JSON report rendering |
| `flowclone-cli` | Optional CLI for testing & debugging |
| `flowclone-desktop` | Tauri v2 shell + command layer |

## Data flow for one clone

1. UI calls `list_disks` → core re-reads the disk catalog.
2. User picks source/target. UI validates locally (size, same-device).
3. UI calls `start_clone(source, target, verify)`.
4. Core resolves fresh `DiskInfo` from paths (TOCTOU guard), builds a
   `CloneJob`, validates, and spawns the run.
5. Core emits `clone://progress` events (preparing → cloning → verifying →
   completed). UI renders them.
6. On completion, UI offers Export Report → core renders via
   `flowclone-report` and writes the file.

## Progress model

`ProgressEmitter` is a `tokio::broadcast` channel. The raw engine pushes ~10 Hz
snapshots through a `ProgressSink`; the core adapter enriches them into a
`Progress` struct (fraction, ETA, throughput) and broadcasts. The Tauri layer
forwards each snapshot as a `clone://progress` event. Slow UI subscribers drop
intermediate updates rather than blocking the copy.

## Privileged access

Raw device writes on macOS require elevation. This is isolated in
`native/macos-helper` (future phase) and reached over a strict, audited IPC
contract. The main process never shells out to arbitrary commands.

## Further reading

- [`DESIGN.md`](./DESIGN.md) — product & UX design language
- [`SAFETY.md`](./SAFETY.md) — safety model & threat surface
- [`ROADMAP.md`](./ROADMAP.md) — MVP scope and future phases
