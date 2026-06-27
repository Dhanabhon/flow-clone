# FlowClone - Architecture

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
│  flowclone-core                                             │
│  validate · create job · run stub clone · emit progress     │
│  run stub verification · generate report data               │
└───┬───────────────┬──────────────────┬──────────────┬───────┘
    │               │                  │              │
    ▼               ▼                  ▼              ▼
 flowclone-disk  flowclone-raw   flowclone-verify  flowclone-report
 mock catalog    stub progress   stub result       markdown/json
```

## Why this layering

FlowClone will eventually perform destructive disk operations. Phase 1 does
not. The architecture already keeps unsafe states narrow:

- **UI cannot clone.** React only renders state and invokes typed Tauri
  commands. There is no path from a button to a raw `write()` that bypasses
  the core.
- **Core owns ordering and safety.** `CloneEngine::run` is the single entry
  point. It enforces validation (`CloneRequest::validate`), drives the stub
  clone, runs stub verification, and emits progress. Callers cannot skip a
  stage.
- **Dependency direction is one-way.** `core` depends on the engine crates;
  the engine crates never depend on `core`. The raw crate reports progress
  through a `ProgressSink` trait that `core` adapts to its own emitter, so
  there are no cyclic dependencies.

## Crates

| Crate | Responsibility |
| --- | --- |
| `flowclone-core` | Job lifecycle, validation, orchestration, progress events |
| `flowclone-disk` | Mock disk discovery and metadata |
| `flowclone-raw` | Stub raw clone progress model |
| `flowclone-verify` | Stub verification result model |
| `flowclone-report` | Markdown and JSON report rendering |
| `flowclone-cli` | CLI for printing detected mock disks |
| `flowclone-desktop` | Tauri v2 shell + command layer |

## Data flow for one clone

1. UI calls `list_disks` and receives the mock disk catalog.
2. User picks source/target. UI validates locally (size, same-device).
3. UI calls `validate_clone_plan`, then `start_clone_stub`.
4. Core resolves `DiskInfo` from paths, builds a `CloneJob`, validates, and
   runs the stub workflow.
5. Core emits `clone://progress` events (preparing -> cloning -> verifying ->
   completed). UI renders them.
6. On completion, UI offers Export Report via `generate_report_stub`.

Image Migration uses `create_image_stub` and `restore_image_stub`. Neither
command writes a `.flowimg` file or a disk in Phase 1.

## Progress model

`ProgressEmitter` is a `tokio::broadcast` channel. The raw stub pushes
snapshots through a `ProgressSink`; the core adapter enriches them into a
`Progress` struct (fraction, ETA, throughput) and broadcasts. The Tauri layer
forwards each snapshot as a `clone://progress` event. Slow UI subscribers drop
intermediate updates rather than blocking the copy.

## Privileged access

Raw device writes on macOS require elevation. This is not implemented in Phase
1. The future helper will live in `native/macos-helper` and use a strict,
audited IPC contract.

## Further reading

- [`DESIGN.md`](./DESIGN.md) — product & UX design language
- [`SAFETY.md`](./SAFETY.md) — safety model & threat surface
- [`ROADMAP.md`](./ROADMAP.md) — MVP scope and future phases
