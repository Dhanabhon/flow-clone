# FlowClone

Move everything. Lose nothing.

FlowClone is a modern, open-source SSD migration assistant for macOS. It is a
Tauri desktop app with a React interface and a Rust workspace behind it.

## Safety warning

FlowClone is not ready for real disk operations. Phase 1 uses mock disk
detection, mock cloning, mock image creation, mock restore, and mock
verification. It must not be used to clone, erase, image, restore, or verify
real drives.

## MVP scope

### Direct Clone

- Detect two mock external SSDs.
- Let the user choose Source SSD and Target SSD.
- Validate that source and target are different.
- Validate that target capacity is greater than or equal to source capacity.
- Show a destructive action confirmation.
- Require the user to type `ERASE` before the clone can start.
- Run a stub clone and emit progress events.

### Image Migration

- Detect the one-disk state with `FLOWCLONE_MOCK_DISKS=one`.
- Suggest creating a `.flowimg` migration image.
- Let the user choose a save location.
- Run a stub image workflow.
- Keep restore support stubbed for a later SSD.

## Tech stack

- Tauri v2
- React
- TypeScript
- Vite
- Tailwind CSS
- shadcn/ui-style primitives
- Framer Motion
- Lucide Icons
- Zustand
- Rust workspace

## Repository layout

```text
flowclone/
  apps/
    desktop/
  crates/
    flowclone-core/
    flowclone-disk/
    flowclone-raw/
    flowclone-verify/
    flowclone-report/
    flowclone-cli/
  docs/
  assets/
  scripts/
```

## Development setup

```bash
pnpm install
pnpm dev
pnpm test
cargo run -p flowclone-cli -- list-disks
```

Use `FLOWCLONE_MOCK_DISKS=one pnpm dev` to test the Image Migration path.

## Docs

- [`docs/DESIGN.md`](docs/DESIGN.md)
- [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)
- [`docs/SAFETY.md`](docs/SAFETY.md)
- [`docs/ROADMAP.md`](docs/ROADMAP.md)

## License

MIT. Treat this as a placeholder until the project governance is finalized.
