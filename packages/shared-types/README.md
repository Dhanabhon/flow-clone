# @flowclone/shared-types

TypeScript types shared between the desktop app and any future JS consumers.

## Status

Today these types are hand-mirrored from the Rust crates in `src/index.ts`.
Keep them in sync with:

- `crates/flowclone-disk/src/model.rs` (`DiskInfo`, `Connection`, `Health`)
- `crates/flowclone-core/src/progress.rs` (`Progress`, `Phase`)

## Future

`scripts/generate-types.sh` will generate these from the Rust source via
`ts-rs` so they can never drift. Until then, treat `src/index.ts` as the source
of truth and update both sides when the Rust model changes.
