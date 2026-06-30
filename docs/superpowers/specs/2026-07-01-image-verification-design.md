# FlowClone Image Verification — Design Spec

**Date:** 2026-07-01
**Status:** Approved (brainstorming)
**Feature:** Image integrity verification (Verification, item 3 of ROADMAP "Next")

## Goal

Let a user confirm a `.flowimg` is still intact and fully recoverable — *"is my
backup good?"* — by embedding a SHA-256 of the image's logical payload at create
time and adding a `verify-image` operation that re-reads the image, recomputes
the digest, and compares.

Verification is **read-only on a file**: it needs no raw disk access, no Full
Disk Access, and no admin prompt. An image can be verified anytime, even with no
disk attached. This fits the Phase 1 "no destructive writes" invariant.

## Scope decisions (locked)

1. **What verify confirms:** image integrity (the `.flowimg` is intact and its
   payload decodes to the original logical bytes). *Not* post-restore target
   compare and *not* post-create source compare — those are follow-ons that
   reuse this digest (see Out of scope).
2. **Coverage:** full hash of the entire logical payload — not sampling.
   Sampling is reserved for device-to-device Direct Clone verification.
3. **Hash domain:** the **logical payload** (pre-compression bytes, in stored
   order) — i.e. the byte stream the decoder produces and restore writes to the
   target. Not the compressed/stored file bytes. This makes the digest reusable
   for a future post-restore target check and catches compression-decode bugs.

## Image format change (no version bump)

Add an **optional, additive** field to the v2 header:

```jsonc
{
  "format": "flowclone-image",
  "version": 2,
  "source": { ... },
  "block_size": 4194304,
  "uncompressed_bytes": 256060514304,
  "compression": "zstd",
  "mode": "used-only",
  "block_map": { ... },
  "payload_sha256": "0000...0000"   // NEW — 64-char lowercase hex
}
```

- **Additive ⇒ back-compatible.** `serde` ignores the field on older readers;
  newer readers treat a missing field as `None` ("no checksum / unverifiable").
  No bump to `FLOW_IMAGE_VERSION_V2`.
- **v1 images** never gain a digest (legacy format, left untouched).
- **All-zeros sentinel.** A present-but-all-zero `payload_sha256` means
  "unfinalized / unverifiable", **not** a real digest. If `create-image` is
  killed after the payload is written but before the digest is rewritten, the
  image reports *unverifiable*, never *corrupt*. (A real SHA-256 of all zeros is
  astronomically improbable, so treating all-zeros as a sentinel is safe.)

### How the digest is written (seek-back rewrite)

The digest is known only after the whole payload has streamed, but the header
sits at the front of the file. So `create-image`:

1. Writes the header with a placeholder `payload_sha256 = "0" * 64`.
2. Streams the payload, hashing the **logical bytes on the read side** (tapped
   from the raw source read, *before* the zstd encoder).
3. After the payload is fully written and flushed, seeks back to the header
   region (fixed offset `MAGIC.len() + 8`) and rewrites the entire header with
   the real digest.

The header byte length is unchanged by the rewrite: the only field that changes
value is a fixed-width 64-char hex string, so the serialized JSON length is
identical and the payload offset never moves.

## Hash semantics (per mode)

The digest always covers the **logical payload stream** = the uncompressed bytes
in stored order (what the encoder consumes / the decoder produces / restore
writes):

| Mode | Logical payload hashed |
| --- | --- |
| Full (uncompressed or zstd) | raw disk bytes `[0, total_bytes)` |
| Used-only (uncompressed or zstd) | present blocks concatenated in stored order |

`verify-image` re-derives this stream by decoding the payload (honoring
`compression` and `block_map`) and hashing the decoded bytes, then compares to
`payload_sha256`. This catches file rot, truncation, **and** a
compression/decode bug (compressed bytes intact but decoding to wrong content).

## CLI surface (`flowclone-cli`)

### `create-image` (modified)

Computes the logical-payload SHA-256 during the existing single read pass and
writes it into the header via the seek-back rewrite. Applies to all three write
paths: `create_flow_image_file`, `create_compressed_image_file`,
`create_sparse_image_file`.

### `verify-image` (new subcommand)

```
flowclone verify-image --image <path.flowimg>
```

- Reads + validates the header (reuse existing header parsing).
- **No digest / all-zeros sentinel** → prints an *unverifiable* result and exits
  with a status distinct from a digest mismatch.
- **Digest present** → opens the payload at `data_offset`, decodes it
  (none/zstd, in `block_map` order), streams it through SHA-256, compares to the
  stored digest.
- Writes a progress file (mirroring the create/restore progress-file pattern) so
  the GUI can render a bar.
- Prints a `VerifyResult`-shaped JSON line for the GUI, including the expected
  and actual digests on mismatch.
- **Read-only and unprivileged** — opens a regular file, never a raw device.

## `flowclone-verify` crate

- `VerifyResult` (already defined) is the shared result model the CLI emits and
  the GUI maps. Extend it only if needed (e.g. optional `expected`/`actual` hex
  for mismatch display) — keep additions minimal.
- Add a streaming SHA-256 helper alongside the existing `checksum::sha256`
  (one-shot) so the CLI can hash a reader without buffering the whole payload.
- `sampler::verify_blockwise` (device-to-device) is **untouched** — reserved for
  Direct Clone.

## Tauri ↔ React surface

### Command

New `verify_image` Tauri command — separate from the existing
`validate_image_stub` (which checks header/structure/size; this checks the
payload digest — the two coexist). It spawns the CLI `verify-image`, streams
progress over a Tauri event, and returns a `VerifyResult`. Strongly-typed TS
wrapper added to
`apps/desktop/src/lib/tauri.ts` with the usual browser/mock fallback. Because
verify is unprivileged, this command does **not** go through the elevated-spawn
path that create/restore use.

### GUI — two entry points

1. **After Create completes (CompletedScreen):** a *"Verify image"* action that
   verifies the just-created file. Closes the trust loop where it matters most.
2. **"Verify an image…":** pick any `.flowimg` and check it, independent of a
   fresh create.

Both render live progress, then a clear result state:

- ✅ **Verified** — `verified N bytes in T s`.
- ❌ **Corrupt** — `checksum mismatch`, showing expected vs actual digest.
- ⚠️ **Unverifiable** — *"This image was created before checksums were added"*
  (missing/all-zeros digest). Informational, not an error.

i18n strings added for English and Thai, matching the existing convention.

## Error handling & back-compat

- Old image without a digest → **unverifiable** UI state, not a red error.
- Mismatch → explicit corrupt state with expected/actual digests.
- A killed create (placeholder digest) → unverifiable, never a false "corrupt".
- The duplicated image-format definitions in `flowclone-cli/src/main.rs` and
  `apps/desktop/src-tauri/src/commands.rs` are mirrored (the new field added in
  both), following the existing convention rather than refactoring into a shared
  crate (out of scope for this feature).

## Testing (Rust, `#[cfg(test)]`)

Using temp files as a fake source "disk" (as the existing sampler tests do):

- **Round-trip per mode** — create then verify for full, full+zstd, used-only,
  used-only+zstd → all report matched.
- **Tamper** — flip one payload byte → verify reports mismatch.
- **Truncate** — chop the payload → verify reports mismatch (or a decode error
  for zstd, surfaced as a failed verify).
- **Legacy** — an image with no `payload_sha256` → unverifiable, not corrupt.
- **Sentinel** — an all-zeros digest → unverifiable, not corrupt.
- **Header rewrite** — the digest is populated and `header_len` is unchanged
  before vs after the rewrite (payload offset stable).

TS side: `pnpm typecheck` and `pnpm lint` for the wrapper and UI (no frontend
test runner yet, per project convention).

## Out of scope (v1)

- **Device-to-device verification** (Direct Clone) — the next feature; reuses
  `verify_blockwise`.
- **Post-restore target compare** (re-read the written target region, hash, and
  compare to `payload_sha256`) — a direct follow-on enabled by this digest.
- **Post-create source compare** (re-read the source) — largely subsumed by the
  integrity digest.
- **Auto-verify before erase during restore** — a full read of a 256 GB image is
  slow; v1 leaves verification an explicit user action taken before restoring.
- APFS-specific handling.

## Future (noted, not built)

- Once a post-restore target check lands, surface a "Verify after restore"
  toggle in the Preferences window (ROADMAP "Next").
- Consider extracting the `.flowimg` format (magic, header, encode/decode) into a
  shared crate to remove the CLI/commands duplication.
