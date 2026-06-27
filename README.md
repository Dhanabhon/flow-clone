# FlowClone

Safe SSD cloning for macOS.

FlowClone is a small desktop app for cloning one disk to another. It walks
through source and target selection, runs the clone through Rust, verifies the
result, and writes a report. The desktop shell uses Tauri v2, React, and Rust.

## Architecture

```
React UI -> Tauri commands -> Rust core -> disk / raw / verify / report
```

The UI is a presenter. It asks Rust to do the work and never clones directly.
`flowclone-core` owns job orchestration, progress, and validation.

### Repository layout

| Path | Purpose |
| --- | --- |
| `apps/desktop` | Tauri v2 + React desktop application |
| `crates/flowclone-core` | Clone orchestration (jobs, progress, validation) |
| `crates/flowclone-disk` | Disk discovery and metadata |
| `crates/flowclone-raw` | Raw read/write engine |
| `crates/flowclone-verify` | Verification engine |
| `crates/flowclone-report` | Markdown and JSON report generation |
| `crates/flowclone-cli` | Optional CLI for testing and debugging |
| `native/macos-helper` | Privileged helper for a later macOS phase |
| `docs/` | Design and architecture docs |

## Requirements

- Rust 1.75+ (stable)
- Node 20+
- pnpm 10+
- macOS 13+ (primary target)

## Getting started

```bash
# Install JavaScript dependencies
pnpm install

# Run the desktop app in development
pnpm dev

# Run the optional Rust CLI
cargo run -p flowclone-cli -- list-disks

# Run all tests
pnpm test
```

See [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) for the full design and
[`docs/SAFETY.md`](docs/SAFETY.md) for the safety model.

## License

[MIT](LICENSE)
