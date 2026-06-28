# Sparse Image — Design (`.flowimg` v2)

Status: **design / not yet implemented**. This is the plan agreed before writing
any destructive format code. Read `docs/SAFETY.md` first.

## Why

Today an image is a **full raw copy** of the source disk, so a 256 GB SSD with
49 GB used still produces a ~256 GB `.flowimg`. A sparse image stores **only the
blocks that matter** (used blocks, or at least non-zero blocks), so the file and
the time shrink to roughly the used size.

| | Current (v1, full raw) | Sparse (v2) |
| --- | --- | --- |
| File size | = disk capacity | ≈ used data |
| Create time | whole disk | used blocks only |
| FS knowledge needed | none | yes (for "used-only") |
| Restore target | any disk ≥ source | any disk ≥ source |

## User-facing modes

One **Smart / Exact** toggle, plus a **Compress** checkbox, plus a live
**size + time estimate** before the user commits.

- **Smart (default)** — FS-aware "used-only". Reads the filesystem's allocation
  map and copies only allocated blocks. Falls back to **Exact** automatically
  when the filesystem is unknown/unsupported (never silently produces a bad
  image).
- **Exact** — today's full raw copy. Always available; the safe baseline.
- **Compress** (optional, both modes) — zstd the payload. Trades CPU for a
  smaller file. Off by default.

CLI mirrors this: `--used-only` (Smart), `--full` (Exact, default), `--compress`.

## `.flowimg` v2 format

v1 stays readable forever; restore auto-detects the version from the header.

```
magic    : "FLOWCLONE_FLOWIMG_V2\n"     (v1 magic still accepted on restore)
hdr_len  : u64 LE                        (length of the JSON header)
header   : JSON  (FlowImageHeaderV2)
payload  : concatenated present blocks, in ascending block index order,
           each optionally zstd-framed when compress=true
```

`FlowImageHeaderV2` (superset of v1):

```jsonc
{
  "format": "flowclone-image",
  "version": 2,
  "source": { "model": "...", "serial": "...", "capacity_bytes": 256060514304 },
  "block_size": 4194304,          // 4 MiB, multiple of 4096 (sector-aligned)
  "total_blocks": 61035,          // ceil(capacity / block_size)
  "mode": "used-only" | "full",
  "compression": "none" | "zstd",
  "block_map": {                  // which blocks are present in the payload
    "encoding": "rle",            // run-length: [start, count] pairs of present runs
    "runs": [[0, 12], [40, 3], ...],
    "present_blocks": 8123
  },
  "payload_bytes": 34072936448,   // sum of stored (post-compression) block bytes
  "note": "..."
}
```

Notes:
- **Block map** lists present blocks as RLE runs of block indices. Absent blocks
  are not stored; on restore they are left as-is on the target (or explicitly
  zeroed — see Restore).
- With `compression: zstd`, each stored block is an independent zstd frame so a
  block can be located/decoded without streaming the whole file; a per-block
  length table is appended after the block map (or each frame is self-delimiting
  via its frame header — implementation detail decided in Phase 1).
- `block_size` is fixed at the existing 4 MiB constant (already asserted
  sector-aligned), so v2 reuses the current sector-alignment guarantees.

## Create pipeline

```
choose mode
  Smart → probe filesystem on the source
            ├─ NTFS  → parse $Bitmap → allocated cluster ranges → block map
            ├─ APFS  → (Phase 3) space manager → block map
            └─ other/unknown → fall back to Exact (full)
  Exact → block map = all blocks [0, total_blocks)
for each present block (ascending):
  read 4 MiB (existing ddrescue-style bad-block skipping still applies)
  if compress: zstd-encode
  append to payload, record length
write header (with final block_map + payload_bytes), then payload → <path>.part → rename
```

"Used-only" is a **superset-safe** optimization: if bitmap parsing is uncertain,
mark more blocks present (never fewer). A block wrongly omitted = data loss on
restore, so the bias is always toward including blocks.

## Restore pipeline

```
read header → detect version
  v1 → linear restore (today's path, unchanged)
  v2 → for each present run, seek target to block*block_size, write block
       (decompress first if zstd)
absent blocks:
  default: leave target untouched (fresh/zeroed disk assumed)
  optional `--zero-absent`: explicitly write zeros over absent ranges
validation unchanged: reject boot/internal/read-only/too-small; require ERASE.
target must be ≥ source.capacity_bytes (same rule as v1).
```

## NTFS `$Bitmap` (Phase 2, first real "Smart")

- NTFS `$Bitmap` (MFT record 6) is a bit-per-cluster allocation map. Parse the
  boot sector for `bytes_per_sector`, `sectors_per_cluster`, `mft_cluster`; read
  the `$Bitmap` data runs; map allocated clusters → byte ranges → 4 MiB blocks.
- Always include partition tables, boot sectors, `$MFT`, and any partition gaps
  the FS doesn't describe (bias to include).
- Read-only parsing; no writes to the source.

## Phasing (each phase ships with tests; no destructive code without `#[cfg(test)]`)

1. **v2 container + Exact mode** — write/read the v2 format with `mode: full`
   (block map = all blocks) and optional zstd. Restore handles v1 + v2. This
   delivers compression with zero FS knowledge and proves the format round-trips.
2. **NTFS used-only** — `$Bitmap` parser → block map; Smart picks it, falls back
   to full. Round-trip + restore tests on a synthetic NTFS image.
3. **APFS used-only** — APFS space manager → block map; extend Smart.
4. **UI** — Smart/Exact toggle, Compress checkbox, size/time estimate, wired to
   the new CLI flags.

## Risks / safety

- **Wrong block map = data loss.** Mitigation: bias to include; full-raw
  fallback; round-trip tests that diff restored image vs source for every mode.
- **Format mistakes = unrestorable files.** Mitigation: version field + explicit
  tests restoring both v1 and v2; `.part`+rename atomic write retained.
- Compression must be deterministic and verifiable; verify by decompressing in
  tests, not by trusting the encoder.
```
