# Contributing to FlowClone

Thanks for helping make disk cloning safer. Before contributing, please read
[`SAFETY.md`](./SAFETY.md). Phase 1 is mock-only; any change that opens or
writes real disks needs a separate safety review before it lands.

## Development setup

Requirements: Rust 1.75+, Node 20+, pnpm 10+, macOS 13+.

```bash
pnpm install              # JS deps
pnpm dev                  # run the desktop app (Tauri dev)
cargo run -p flowclone-cli -- list-disks   # exercise the core from the CLI
pnpm test                 # JS + Rust tests
```

## Layout

See [`ARCHITECTURE.md`](./ARCHITECTURE.md). In short:

- All business logic lives in `crates/flowclone-*`.
- `apps/desktop/src-tauri` is a thin command layer — keep it thin.
- `apps/desktop/src` is a presenter — never clone from the UI.

## Rules

1. **Never bypass core validation.** New clone paths must go through
   `CloneEngine` and `CloneRequest::validate`.
2. **No cyclic crate dependencies.** Engine crates (`disk`, `raw`, `verify`,
   `report`) must not depend on `core`.
3. **Keep the command layer thin.** Commands marshal types and delegate.
4. **Test destructive paths.** Add unit tests for any new validation or verify
   behavior.
5. **Match existing style.** Rust: `rustfmt` + `clippy`. TS: existing ESLint
   config.

## Committing

Small, focused commits. Reference the issue/PR in the description. Don't commit
`target/`, `node_modules/`, or `dist/` (they're gitignored).

## Reporting issues

Bugs that could cause data loss → see the security policy in
[`../SECURITY.md`](../SECURITY.md) before filing.
