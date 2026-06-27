# Repository Guidelines

## Project Structure & Module Organization

FlowClone is a pnpm + Rust workspace for a Tauri desktop app. The React UI lives in `apps/desktop/src`; its Tauri command layer is in `apps/desktop/src-tauri`. Core cloning behavior belongs in Rust crates under `crates/flowclone-*`: `core` orchestrates jobs, while `disk`, `raw`, `verify`, and `report` own focused engine work. Shared TypeScript types live in `packages/shared-types`. Design, safety, and architecture notes are in `docs/`; static icons and app assets are under `assets/`.

## Build, Test, and Development Commands

- `pnpm install`: install workspace JavaScript dependencies.
- `pnpm dev`: run the desktop app in Tauri/Vite development mode.
- `pnpm build`: build the desktop package via the `desktop` workspace.
- `pnpm lint`: run workspace lint scripts, currently ESLint for `apps/desktop`.
- `pnpm typecheck`: run TypeScript checks across packages.
- `pnpm test`: run package tests, then `cargo test --workspace`.
- `cargo run -p flowclone-cli -- list-disks`: exercise Rust disk discovery from the CLI.

## Coding Style & Naming Conventions

Use Rust 2021 with `rustfmt` formatting and `clippy` cleanup before substantial Rust changes. Keep crate responsibilities narrow; do not add cyclic dependencies from engine crates back into `flowclone-core`. TypeScript uses ES modules, React components in `PascalCase`, hooks as `use-*` files/functions, and utility modules in lowercase or kebab-case paths. The UI is a presenter only; cloning, verification, and validation must stay in Rust.

## Testing Guidelines

Rust unit tests live beside code in `#[cfg(test)] mod tests`; add focused tests for validation, raw IO, verification, and report behavior. Destructive or safety-sensitive paths need explicit tests before merging. There is no dedicated frontend test runner yet, so use `pnpm typecheck`, `pnpm lint`, and manual Tauri verification for UI changes.

## Commit & Pull Request Guidelines

The current history only shows `Initial commit`; use small, imperative commit subjects such as `Add clone validation test`. PRs should describe the behavioral change, link the issue when available, include screenshots for UI changes, and call out any disk, raw IO, privilege, or data-loss risk. Read `docs/SAFETY.md` before touching validation, raw cloning, or helper code.

## Agent-Specific Instructions

When running shell commands in this repo, prefix them with `rtk` as requested by the local Codex instructions, for example `rtk pnpm test`.
