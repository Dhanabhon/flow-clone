# Image Verification Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Embed a SHA-256 of a `.flowimg`'s logical payload at create time and add a read-only `verify-image` operation (CLI + GUI) that re-reads the image, recomputes the digest, and reports verified / corrupt / unverifiable.

**Architecture:** A streaming hash lives in `flowclone-verify`. Every image-create path wraps its payload sink in that hasher and rewrites the v2 header's new `payload_sha256` field in place (seek-back) once the digest is known. A new CLI `verify-image` subcommand decodes the payload (raw or zstd) and re-hashes it. A new unprivileged Tauri `verify_image` command runs that subcommand and streams progress to a React verify UI.

**Tech Stack:** Rust (`flowclone-cli`, `flowclone-verify`, Tauri commands), `sha2`, `zstd`, React/TypeScript, Zustand, Tauri events.

## Global Constraints

- Rust 2021; `cargo fmt` + `cargo clippy --workspace --all-targets -- -D warnings` must be clean (clippy warnings fail CI).
- Hash domain is the **logical payload**: the uncompressed bytes in stored order (what the encoder consumes / the decoder produces / restore writes). Full mode = all `uncompressed_bytes`; used-only = present blocks only.
- The v2 header field is **additive and optional** — no bump to `FLOW_IMAGE_VERSION_V2` (= 2). Older readers ignore it; missing ⇒ `None` ⇒ unverifiable.
- `payload_sha256` is 64 lowercase hex chars. **All-zeros is a sentinel** meaning "unfinalized / unverifiable", never a real digest.
- Verification is **read-only on a file** — no raw device, no Full Disk Access, no admin/elevation.
- The `.flowimg` format is duplicated in `crates/flowclone-cli/src/main.rs` and `apps/desktop/src-tauri/src/commands.rs`. Mirror changes in both; do **not** refactor into a shared crate (out of scope).
- Image block size constant: `IMAGE_BLOCK_SIZE` (4 MiB). v2 magic: `FLOWCLONE_FLOWIMG_V2\n` (same length as v1). Header layout: `magic` + `u64 LE header_len` + `header JSON`.
- New images are always v2: the full-*uncompressed* create paths migrate v1 → v2 (`compression: "none"`, `mode: "full"`). v1 stays readable for old images but is no longer written.
- i18n strings exist for English and Thai (`apps/desktop/src/lib/i18n.ts`); every new UI string gets both.

---

### Task 1: Streaming hash primitives in `flowclone-verify`

**Files:**
- Modify: `crates/flowclone-verify/src/checksum.rs`
- Test: `crates/flowclone-verify/src/checksum.rs` (`#[cfg(test)]`)

**Interfaces:**
- Consumes: existing `checksum::{sha256, hex}`.
- Produces:
  - `pub struct Sha256Writer<W: std::io::Write>` with `pub fn new(inner: W) -> Self` and `pub fn into_parts(self) -> ([u8; 32], W)`.
  - `pub fn hash_reader<R: std::io::Read>(reader: &mut R) -> std::io::Result<([u8; 32], u64)>`.

- [ ] **Step 1: Write the failing tests**

Add to the `#[cfg(test)] mod tests` in `crates/flowclone-verify/src/checksum.rs`:

```rust
    #[test]
    fn sha256_writer_matches_oneshot_and_forwards_bytes() {
        use std::io::Write;
        let payload = b"flowclone payload bytes";
        let mut sink: Vec<u8> = Vec::new();
        let mut writer = Sha256Writer::new(&mut sink);
        writer.write_all(payload).unwrap();
        let (digest, _inner) = writer.into_parts();
        assert_eq!(hex(&digest), hex(&sha256(payload)));
        assert_eq!(sink, payload, "bytes must pass through unchanged");
    }

    #[test]
    fn hash_reader_matches_oneshot() {
        let payload = vec![7u8; 100_000];
        let mut cursor = &payload[..];
        let (digest, n) = hash_reader(&mut cursor).unwrap();
        assert_eq!(n, payload.len() as u64);
        assert_eq!(hex(&digest), hex(&sha256(&payload)));
    }

    #[test]
    fn hash_reader_empty_is_empty_digest() {
        let (digest, n) = hash_reader(&mut std::io::empty()).unwrap();
        assert_eq!(n, 0);
        assert_eq!(
            hex(&digest),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p flowclone-verify checksum`
Expected: FAIL — `cannot find type Sha256Writer` / `cannot find function hash_reader`.

- [ ] **Step 3: Implement the primitives**

At the top of `crates/flowclone-verify/src/checksum.rs`, extend the imports and add the two items below the existing `hex` function:

```rust
use sha2::{Digest, Sha256};
use std::io::{self, Read, Write};

/// A `Write` adapter that streams every byte through SHA-256 on the way to an
/// inner writer. Lets image-create hash the logical payload as it is written
/// (before compression) without a second pass over the source.
pub struct Sha256Writer<W: Write> {
    inner: W,
    hasher: Sha256,
}

impl<W: Write> Sha256Writer<W> {
    pub fn new(inner: W) -> Self {
        Self {
            inner,
            hasher: Sha256::new(),
        }
    }

    /// Finish hashing and return the digest plus the inner writer, so callers
    /// can finish a zstd encoder or fsync the file afterwards.
    pub fn into_parts(self) -> ([u8; 32], W) {
        let out = self.hasher.finalize();
        let mut digest = [0u8; 32];
        digest.copy_from_slice(&out);
        (digest, self.inner)
    }
}

impl<W: Write> Write for Sha256Writer<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.hasher.update(&buf[..n]);
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// Stream a reader to EOF through SHA-256, returning the digest and byte count.
/// Used at verify time to re-hash a decoded image payload.
pub fn hash_reader<R: Read>(reader: &mut R) -> io::Result<([u8; 32], u64)> {
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut total = 0u64;
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        total += n as u64;
    }
    let out = hasher.finalize();
    let mut digest = [0u8; 32];
    digest.copy_from_slice(&out);
    Ok((digest, total))
}
```

(The existing `use sha2::{Digest, Sha256};` at the top of the file is now duplicated — keep a single `use` line; merge if the compiler warns.)

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p flowclone-verify checksum`
Expected: PASS (all three new tests plus the existing two).

- [ ] **Step 5: fmt + clippy + commit**

```bash
cargo fmt -p flowclone-verify
cargo clippy -p flowclone-verify --all-targets -- -D warnings
git add crates/flowclone-verify/src/checksum.rs
git commit -m "feat(verify): add streaming Sha256Writer and hash_reader primitives"
```

---

### Task 2: Add `payload_sha256` to the v2 image header (CLI format plumbing)

Adds the header field and the unfinalized-digest sentinel, threads a digest argument through the v2 header writers, and migrates the full-uncompressed create path from v1 to v2 (writing the sentinel for now — Task 3 fills in the real digest). After this task, every newly created image is a v2 image whose `payload_sha256` is all-zeros (valid, but reported "unverifiable").

**Files:**
- Modify: `crates/flowclone-cli/src/main.rs` (header structs ~887-911, `flow_image_header_v2`/`write_flow_image_header_v2` ~1339-1374, `FlowImageHeaderV2Owned` ~462-473, `create_flow_image_file` ~913-934, callers of the v2 header writer ~951 & ~989)
- Test: `crates/flowclone-cli/src/main.rs` (`#[cfg(test)]`)

**Interfaces:**
- Consumes: existing `Compression`, `BlockMap`, `DiskInfo`, `FLOW_IMAGE_MAGIC_V2`, `IMAGE_BLOCK_SIZE`.
- Produces:
  - `const UNFINALIZED_DIGEST_HEX: &str` (64 zeros).
  - `fn digest_is_finalized(hex: &str) -> bool`.
  - `flow_image_header_v2(source, compression, block_map, payload_sha256: &str) -> Result<Vec<u8>>` (new trailing arg).
  - `write_flow_image_header_v2(writer, source, compression, block_map, payload_sha256: &str) -> Result<()>` (new trailing arg).
  - `FlowImageHeaderV2Owned.payload_sha256: Option<String>`.
  - `const HEADER_OFFSET_V2: u64 = FLOW_IMAGE_MAGIC_V2.len() as u64 + 8` (start of the v2 header JSON; used by Task 3's seek-back).

- [ ] **Step 1: Write the failing tests**

Add to the CLI test module in `crates/flowclone-cli/src/main.rs`. Reuse the existing test `DiskInfo` builder used by nearby tests (see the test near line 1653 that calls `write_flow_image_header`); call it `test_source()` below — if no shared helper exists, copy that test's inline `DiskInfo { .. }` construction into a local `fn test_source() -> DiskInfo`.

```rust
    #[test]
    fn v2_header_length_is_stable_across_digest_rewrite() {
        let source = test_source();
        let placeholder =
            flow_image_header_v2(&source, Compression::Zstd, None, UNFINALIZED_DIGEST_HEX).unwrap();
        let real_hex = "a".repeat(64);
        let real = flow_image_header_v2(&source, Compression::Zstd, None, &real_hex).unwrap();
        assert_eq!(
            placeholder.len(),
            real.len(),
            "digest rewrite must not change header length (payload offset must stay put)"
        );
    }

    #[test]
    fn unfinalized_digest_is_not_finalized() {
        assert!(!digest_is_finalized(UNFINALIZED_DIGEST_HEX));
        assert!(!digest_is_finalized(""));
        assert!(!digest_is_finalized("zz")); // wrong length
        assert!(digest_is_finalized(&"a".repeat(64)));
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p flowclone-cli v2_header_length_is_stable_across_digest_rewrite unfinalized_digest_is_not_finalized`
Expected: FAIL — `flow_image_header_v2` takes 3 args / `UNFINALIZED_DIGEST_HEX` and `digest_is_finalized` not found.

- [ ] **Step 3: Add the sentinel + helper**

Near the other image constants (top of `crates/flowclone-cli/src/main.rs`, after `FLOW_IMAGE_VERSION_V2`):

```rust
/// A v2 header's `payload_sha256` before the real digest is known. A finalized
/// image overwrites this in place once the payload is fully written. All-zeros
/// is treated as "unverifiable", never as a real (matching) digest — so a
/// create killed mid-finalize reports unverifiable, not corrupt.
const UNFINALIZED_DIGEST_HEX: &str =
    "0000000000000000000000000000000000000000000000000000000000000000";

/// Offset of the v2 header JSON within the file: magic + the u64 length prefix.
const HEADER_OFFSET_V2: u64 = FLOW_IMAGE_MAGIC_V2.len() as u64 + 8;

/// Whether a `payload_sha256` value is a real, usable digest (64 hex chars and
/// not the all-zeros sentinel).
fn digest_is_finalized(hex: &str) -> bool {
    hex.len() == 64
        && hex != UNFINALIZED_DIGEST_HEX
        && hex.bytes().all(|b| b.is_ascii_hexdigit())
}
```

- [ ] **Step 4: Add the header field (serialize + deserialize)**

In `FlowImageHeaderV2<'a>` (the `#[derive(Serialize)]` struct ~896), add before `note`:

```rust
    /// SHA-256 (lowercase hex) of the logical payload. All-zeros = unfinalized.
    payload_sha256: &'a str,
```

In `FlowImageHeaderV2Owned` (the `#[derive(Deserialize)]` struct ~462), add:

```rust
    #[serde(default)]
    payload_sha256: Option<String>,
```

- [ ] **Step 5: Thread the digest through the header writers**

Update `flow_image_header_v2` and `write_flow_image_header_v2` to take `payload_sha256: &str` and set the field:

```rust
fn write_flow_image_header_v2(
    writer: &mut impl Write,
    source: &DiskInfo,
    compression: Compression,
    block_map: Option<&BlockMap>,
    payload_sha256: &str,
) -> Result<()> {
    let header = flow_image_header_v2(source, compression, block_map, payload_sha256)?;
    writer.write_all(FLOW_IMAGE_MAGIC_V2)?;
    writer.write_all(&(header.len() as u64).to_le_bytes())?;
    writer.write_all(&header)?;
    Ok(())
}

fn flow_image_header_v2(
    source: &DiskInfo,
    compression: Compression,
    block_map: Option<&BlockMap>,
    payload_sha256: &str,
) -> Result<Vec<u8>> {
    serde_json::to_vec(&FlowImageHeaderV2 {
        format: FLOW_IMAGE_FORMAT,
        version: FLOW_IMAGE_VERSION_V2,
        source,
        block_size: IMAGE_BLOCK_SIZE as u64,
        uncompressed_bytes: source.total_bytes,
        compression: compression.as_str(),
        mode: if block_map.is_some() { "used-only" } else { "full" },
        block_map,
        payload_sha256,
        note: "Disk payload follows this header.",
    })
    .map_err(Into::into)
}
```

- [ ] **Step 6: Update the two existing v2 callers to pass the sentinel**

In `create_compressed_image_file` (~951): `write_flow_image_header_v2(&mut image, source, Compression::Zstd, None, UNFINALIZED_DIGEST_HEX)?;`

In `create_sparse_image_file` (~989): `write_flow_image_header_v2(&mut image, source, compression, Some(block_map), UNFINALIZED_DIGEST_HEX)?;`

- [ ] **Step 7: Migrate full-uncompressed create from v1 to v2**

In `create_flow_image_file` (~920), replace the v1 header write with a v2 full/none header carrying the sentinel:

```rust
    write_flow_image_header_v2(&mut image, source, Compression::None, None, UNFINALIZED_DIGEST_HEX)?;
```

Leave the rest of the function (the `copy_disk_payload` call, sync, finalize) unchanged — a v2 full/none payload is byte-identical to the old v1 payload, and restore/validation already accept v2 full/none.

- [ ] **Step 8: Run tests + a create→restore round-trip**

Run: `cargo test -p flowclone-cli`
Expected: PASS. The two new tests pass; existing CLI tests still pass (v1 *read* support is untouched; any test that writes a v1 header via `write_flow_image_header` still exercises legacy read).

- [ ] **Step 9: fmt + clippy + commit**

```bash
cargo fmt -p flowclone-cli
cargo clippy -p flowclone-cli --all-targets -- -D warnings
git add crates/flowclone-cli/src/main.rs
git commit -m "feat(cli): add payload_sha256 to v2 header; full images now v2"
```

---

### Task 3: Compute and store the real digest at CLI create time

Wrap each create path's payload sink in `Sha256Writer`, then seek back and rewrite the header with the real digest. After this task every CLI-created image carries a correct, verifiable digest.

**Files:**
- Modify: `crates/flowclone-cli/src/main.rs` (`create_flow_image_file` ~913, `create_compressed_image_file` ~939, `create_sparse_image_file` ~976; add a `finalize_digest` helper)
- Add dependency: `flowclone-verify` to `crates/flowclone-cli/Cargo.toml`
- Test: `crates/flowclone-cli/src/main.rs` (`#[cfg(test)]`)

**Interfaces:**
- Consumes: `flowclone_verify::checksum::{Sha256Writer, hex}`, `HEADER_OFFSET_V2`, `flow_image_header_v2`, `digest_is_finalized`, `read_flow_image_header`.
- Produces: `fn rewrite_header_digest(image: &mut File, source: &DiskInfo, compression: Compression, block_map: Option<&BlockMap>, digest_hex: &str) -> Result<()>`.

- [ ] **Step 1: Add the crate dependency**

In `crates/flowclone-cli/Cargo.toml`, under `[dependencies]`:

```toml
flowclone-verify = { path = "../flowclone-verify" }
```

Run `cargo build -p flowclone-cli` to confirm it resolves. Expected: builds.

- [ ] **Step 2: Write the failing test (round-trip digest is real)**

Add to the CLI test module. This drives a full-uncompressed create against a temp file standing in for a raw disk, then asserts the stored digest is finalized and equals the SHA-256 of the source payload. Reuse `test_source()` from Task 2 but point its `device_path` / size at a temp file; if the create path resolves disks via the catalog, instead test through the lower-level writer by calling `create_flow_image_file(src_path, img_path, &source)` where `source.total_bytes` equals the temp file length. (Match the existing CLI create tests' setup near line 1653.)

```rust
    #[test]
    fn full_uncompressed_image_stores_real_payload_digest() {
        use flowclone_verify::checksum::{hex, sha256};
        let dir = unique_temp_dir("verify-create-full");
        let src = dir.join("src.bin");
        let img = dir.join("out.flowimg");
        let payload = vec![0xABu8; IMAGE_BLOCK_SIZE + 1234]; // > 1 block, partial tail
        std::fs::write(&src, &payload).unwrap();

        let source = test_source_sized(payload.len() as u64);
        create_flow_image_file(src.to_str().unwrap(), img.to_str().unwrap(), &source).unwrap();

        let info = read_flow_image_header(img.to_str().unwrap()).unwrap();
        let stored = info.payload_sha256.expect("digest present");
        assert!(digest_is_finalized(&stored));
        assert_eq!(stored, hex(&sha256(&payload)));
    }
```

This requires `ImageInfo` to expose the parsed digest — see Step 4. `unique_temp_dir` / `test_source_sized` mirror existing helpers (the sampler test uses a unique temp dir; copy that pattern into the CLI test module, and make `test_source_sized(n)` build a `DiskInfo` with `total_bytes = n`).

- [ ] **Step 3: Run the test to verify it fails**

Run: `cargo test -p flowclone-cli full_uncompressed_image_stores_real_payload_digest`
Expected: FAIL — `info.payload_sha256` field missing and/or stored digest is the all-zeros sentinel.

- [ ] **Step 4: Surface the parsed digest on `ImageInfo`**

In the `ImageInfo` struct (the parsed-header type returned by `read_flow_image_header`, ~448), add:

```rust
    /// `payload_sha256` from a v2 header (None for v1 or a missing field).
    payload_sha256: Option<String>,
```

In `read_flow_image_header`, set it in both return sites: `payload_sha256: None` for the v1 branch (~599), and `payload_sha256: parsed.payload_sha256` for the v2 branch (~666).

- [ ] **Step 5: Add the seek-back rewrite helper**

Add near the header writers:

```rust
/// Overwrite a v2 image's header in place with the real payload digest. The
/// header length is unchanged because only the fixed-width `payload_sha256`
/// value differs, so the payload offset never moves.
fn rewrite_header_digest(
    image: &mut File,
    source: &DiskInfo,
    compression: Compression,
    block_map: Option<&BlockMap>,
    digest_hex: &str,
) -> Result<()> {
    let header = flow_image_header_v2(source, compression, block_map, digest_hex)?;
    image.seek(SeekFrom::Start(HEADER_OFFSET_V2))?;
    image.write_all(&header)?;
    Ok(())
}
```

- [ ] **Step 6: Hash + rewrite in `create_flow_image_file` (full, uncompressed)**

Wrap the image file in a `Sha256Writer` for the copy, then recover the file, rewrite the header, and finalize. Replace the body from the `copy_disk_payload` call through `finalize_image`:

```rust
    let mut hashing = flowclone_verify::checksum::Sha256Writer::new(image);
    copy_disk_payload(
        &mut reader,
        &mut hashing,
        source,
        image_path,
        &cancel_path,
        &partial_path,
    )?;
    let (digest, mut image) = hashing.into_parts();
    let digest_hex = flowclone_verify::checksum::hex(&digest);
    rewrite_header_digest(&mut image, source, Compression::None, None, &digest_hex)?;
    image.sync_all()?;
    drop(image);
    finalize_image(&partial_path, image_path)
```

- [ ] **Step 7: Hash + rewrite in `create_compressed_image_file` (full, zstd)**

The sink is the zstd encoder; hash the *logical* bytes by wrapping the encoder. After finishing the encoder, recover the file and rewrite:

```rust
    let encoder = zstd::Encoder::new(image, ZSTD_LEVEL)
        .map_err(|error| anyhow::anyhow!("init compressor: {error}"))?;
    let mut hashing = flowclone_verify::checksum::Sha256Writer::new(encoder);
    copy_disk_payload(
        &mut reader,
        &mut hashing,
        source,
        image_path,
        &cancel_path,
        &partial_path,
    )?;
    let (digest, encoder) = hashing.into_parts();
    let mut image = encoder
        .finish()
        .map_err(|error| anyhow::anyhow!("finish compressor: {error}"))?;
    let digest_hex = flowclone_verify::checksum::hex(&digest);
    rewrite_header_digest(&mut image, source, Compression::Zstd, None, &digest_hex)?;
    image.sync_all()?;
    drop(image);
    finalize_image(&partial_path, image_path)
```

- [ ] **Step 8: Hash + rewrite in `create_sparse_image_file` (used-only, ±zstd)**

For both arms, wrap the sink (`image` for `None`, `encoder` for `Zstd`) in `Sha256Writer`, recover the file after the copy, and rewrite with the real digest. Uncompressed arm:

```rust
        Compression::None => {
            let mut hashing = flowclone_verify::checksum::Sha256Writer::new(image);
            copy_present_blocks(
                &mut reader, &mut hashing, source, block_map,
                image_path, &cancel_path, &partial_path,
            )?;
            let (digest, mut image) = hashing.into_parts();
            let digest_hex = flowclone_verify::checksum::hex(&digest);
            rewrite_header_digest(&mut image, source, Compression::None, Some(block_map), &digest_hex)?;
            image.sync_all()?;
            drop(image);
        }
```

Zstd arm:

```rust
        Compression::Zstd => {
            let encoder = zstd::Encoder::new(image, ZSTD_LEVEL)
                .map_err(|error| anyhow::anyhow!("init compressor: {error}"))?;
            let mut hashing = flowclone_verify::checksum::Sha256Writer::new(encoder);
            copy_present_blocks(
                &mut reader, &mut hashing, source, block_map,
                image_path, &cancel_path, &partial_path,
            )?;
            let (digest, encoder) = hashing.into_parts();
            let mut image = encoder
                .finish()
                .map_err(|error| anyhow::anyhow!("finish compressor: {error}"))?;
            let digest_hex = flowclone_verify::checksum::hex(&digest);
            rewrite_header_digest(&mut image, source, Compression::Zstd, Some(block_map), &digest_hex)?;
            image.sync_all()?;
            drop(image);
        }
```

- [ ] **Step 9: Add a compressed + sparse round-trip test**

```rust
    #[test]
    fn compressed_image_stores_real_payload_digest() {
        use flowclone_verify::checksum::{hex, sha256};
        let dir = unique_temp_dir("verify-create-zstd");
        let src = dir.join("src.bin");
        let img = dir.join("out.flowimg");
        let payload = vec![0x5Au8; IMAGE_BLOCK_SIZE * 2];
        std::fs::write(&src, &payload).unwrap();
        let source = test_source_sized(payload.len() as u64);

        create_compressed_image_file(src.to_str().unwrap(), img.to_str().unwrap(), &source).unwrap();

        let info = read_flow_image_header(img.to_str().unwrap()).unwrap();
        let stored = info.payload_sha256.expect("digest present");
        assert!(digest_is_finalized(&stored));
        assert_eq!(stored, hex(&sha256(&payload)));
    }
```

- [ ] **Step 10: Run tests**

Run: `cargo test -p flowclone-cli`
Expected: PASS (new digest tests + existing).

- [ ] **Step 11: fmt + clippy + commit**

```bash
cargo fmt -p flowclone-cli
cargo clippy -p flowclone-cli --all-targets -- -D warnings
git add crates/flowclone-cli/Cargo.toml crates/flowclone-cli/src/main.rs Cargo.lock
git commit -m "feat(cli): compute and store payload SHA-256 in all create paths"
```

---

### Task 4: `verify-image` CLI subcommand

The core deliverable: decode a `.flowimg` payload and compare its SHA-256 to the stored digest, emitting a machine-readable result.

**Files:**
- Modify: `crates/flowclone-verify/src/lib.rs` (extend `VerifyResult`)
- Modify: `crates/flowclone-verify/src/sampler.rs` and `lib.rs` (update `VerifyResult` literals)
- Modify: `crates/flowclone-cli/src/main.rs` (add `verify_image`, wire `main` dispatch + help)
- Test: `crates/flowclone-cli/src/main.rs` (`#[cfg(test)]`)

**Interfaces:**
- Consumes: `read_flow_image_header`, `ImageInfo`, `digest_is_finalized`, `flowclone_verify::checksum::{hash_reader, hex}`, `Compression`, `VerifyResult`.
- Produces: `fn verify_image() -> Result<()>` printing one JSON `VerifyResult` line to stdout; a `verify-image` subcommand.

- [ ] **Step 1: Extend `VerifyResult` (failing build)**

In `crates/flowclone-verify/src/lib.rs`, add three fields to `VerifyResult`:

```rust
    /// False when the image carried no usable digest (legacy / unfinalized).
    #[serde(default = "default_true")]
    pub verifiable: bool,
    /// Expected digest (hex) — set on a mismatch for display.
    #[serde(default)]
    pub expected: Option<String>,
    /// Actual recomputed digest (hex) — set on a mismatch for display.
    #[serde(default)]
    pub actual: Option<String>,
```

Add the serde default helper near the struct:

```rust
fn default_true() -> bool {
    true
}
```

Update the two existing `VerifyResult { .. }` literals (in `lib.rs` `DefaultVerifier::verify` and `sampler.rs` `verify_blockwise`) to append:

```rust
        verifiable: true,
        expected: None,
        actual: None,
```

- [ ] **Step 2: Build to confirm the literals compile**

Run: `cargo build -p flowclone-verify`
Expected: builds (no missing-field errors).

- [ ] **Step 3: Write the failing tests for `verify_image`**

Add to the CLI test module. Test the verify *logic* by factoring the decode+compare into a testable function `verify_image_file(image_path: &str) -> Result<VerifyResult>` that `verify_image()` wraps (parsing argv, printing JSON). Tests call `verify_image_file` directly:

```rust
    #[test]
    fn verify_matches_freshly_created_image() {
        let dir = unique_temp_dir("verify-ok");
        let src = dir.join("src.bin");
        let img = dir.join("out.flowimg");
        std::fs::write(&src, vec![0x11u8; IMAGE_BLOCK_SIZE + 99]).unwrap();
        let source = test_source_sized(IMAGE_BLOCK_SIZE as u64 + 99);
        create_flow_image_file(src.to_str().unwrap(), img.to_str().unwrap(), &source).unwrap();

        let result = verify_image_file(img.to_str().unwrap()).unwrap();
        assert!(result.verifiable);
        assert!(result.matched);
        assert_eq!(result.mismatches, 0);
    }

    #[test]
    fn verify_detects_a_flipped_payload_byte() {
        let dir = unique_temp_dir("verify-bad");
        let src = dir.join("src.bin");
        let img = dir.join("out.flowimg");
        std::fs::write(&src, vec![0x22u8; IMAGE_BLOCK_SIZE]).unwrap();
        let source = test_source_sized(IMAGE_BLOCK_SIZE as u64);
        create_flow_image_file(src.to_str().unwrap(), img.to_str().unwrap(), &source).unwrap();

        // Corrupt one payload byte (just past the header).
        let info = read_flow_image_header(img.to_str().unwrap()).unwrap();
        let mut bytes = std::fs::read(&img).unwrap();
        let i = info.data_offset as usize + 10;
        bytes[i] ^= 0xFF;
        std::fs::write(&img, &bytes).unwrap();

        let result = verify_image_file(img.to_str().unwrap()).unwrap();
        assert!(result.verifiable);
        assert!(!result.matched);
        assert!(result.expected.is_some() && result.actual.is_some());
    }

    #[test]
    fn verify_reports_legacy_v1_image_as_unverifiable() {
        let dir = unique_temp_dir("verify-legacy");
        let img = dir.join("legacy.flowimg");
        let source = test_source_sized(IMAGE_BLOCK_SIZE as u64);
        // Write a v1 image (legacy path) + its payload.
        let mut file = std::fs::File::create(&img).unwrap();
        write_flow_image_header(&mut file, &source).unwrap();
        file.write_all(&vec![0u8; IMAGE_BLOCK_SIZE]).unwrap();
        file.sync_all().unwrap();
        drop(file);

        let result = verify_image_file(img.to_str().unwrap()).unwrap();
        assert!(!result.verifiable, "v1 image has no digest");
        assert!(!result.matched);
    }
```

- [ ] **Step 4: Run tests to verify they fail**

Run: `cargo test -p flowclone-cli verify_`
Expected: FAIL — `verify_image_file` not found.

- [ ] **Step 5: Implement `verify_image_file` + `verify_image`**

Add to `crates/flowclone-cli/src/main.rs`:

```rust
use flowclone_verify::VerifyResult;

/// Decode an image's payload and compare its SHA-256 to the stored digest.
/// Read-only; never opens a raw device.
fn verify_image_file(image_path: &str) -> Result<VerifyResult> {
    let info = read_flow_image_header(image_path)?;

    // No usable digest (v1, missing field, or the unfinalized sentinel).
    let stored = match info.payload_sha256.as_deref() {
        Some(hex) if digest_is_finalized(hex) => hex.to_string(),
        _ => {
            return Ok(VerifyResult {
                matched: false,
                bytes_checked: 0,
                blocks_checked: 0,
                mismatches: 0,
                elapsed_secs: 0.0,
                verifiable: false,
                expected: None,
                actual: None,
            });
        }
    };

    let start = std::time::Instant::now();
    let mut file = File::open(image_path)
        .map_err(|error| anyhow::anyhow!("open image {image_path}: {error}"))?;
    file.seek(SeekFrom::Start(info.data_offset))?;
    let mut payload: Box<dyn Read> = match info.compression {
        Compression::None => Box::new(file),
        Compression::Zstd => Box::new(
            zstd::Decoder::new(file)
                .map_err(|error| anyhow::anyhow!("init decompressor: {error}"))?,
        ),
    };

    let (digest, bytes) = flowclone_verify::checksum::hash_reader(&mut payload)
        .map_err(|error| anyhow::anyhow!("read image payload: {error}"))?;
    let actual = flowclone_verify::checksum::hex(&digest);
    let matched = actual == stored;

    Ok(VerifyResult {
        matched,
        bytes_checked: bytes,
        blocks_checked: 0,
        mismatches: if matched { 0 } else { 1 },
        elapsed_secs: start.elapsed().as_secs_f64(),
        verifiable: true,
        expected: if matched { None } else { Some(stored) },
        actual: if matched { None } else { Some(actual) },
    })
}

/// CLI entry: `flowclone verify-image --image <path>`. Prints the result as a
/// single JSON line on stdout for the GUI to parse.
fn verify_image() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let image_path = arg_value(&args, "--image")?;
    let result = verify_image_file(image_path)?;
    println!("{}", serde_json::to_string(&result)?);
    if result.verifiable && !result.matched {
        anyhow::bail!("image verification failed: checksum mismatch");
    }
    Ok(())
}
```

- [ ] **Step 6: Wire `main` dispatch + help**

In `main` (~167), add a match arm:

```rust
        "verify-image" => verify_image(),
```

In the help block (~180), add:

```rust
            eprintln!("  verify-image   Check a .flowimg against its stored checksum");
```

- [ ] **Step 7: Run tests**

Run: `cargo test -p flowclone-cli verify_`
Expected: PASS (all three verify tests).

- [ ] **Step 8: Manual smoke test**

```bash
cargo run -p flowclone-cli -- verify-image --image /path/to/some.flowimg
```
Expected: prints a JSON line with `"verifiable"`, `"matched"`, `"bytes_checked"`. (Use an image created after Task 3, or any old image → `"verifiable":false`.)

- [ ] **Step 9: fmt + clippy + commit**

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
git add crates/flowclone-verify/src/lib.rs crates/flowclone-verify/src/sampler.rs crates/flowclone-cli/src/main.rs
git commit -m "feat(cli): add verify-image subcommand and VerifyResult outcome fields"
```

---

### Task 5: Mirror the digest in the GUI in-process create path

The GUI writes a full-uncompressed image **in-process** (no elevation) when it can already read the raw device — the common case after Full Disk Access is granted. That writer lives in `commands.rs` and currently emits a v1, digest-less image. Mirror Tasks 2–3 there so those images are verifiable too.

**Files:**
- Modify: `apps/desktop/src-tauri/src/commands.rs` (`FlowImageHeaderV2` ~77, the v2 constants ~28, `create_flow_image_file` ~786, `write_flow_image_header` ~869, `flow_image_header` ~883; add a v2 header writer + digest rewrite)
- Add dependency: `flowclone-verify` to `apps/desktop/src-tauri/Cargo.toml`
- Test: `apps/desktop/src-tauri/src/commands.rs` (`#[cfg(test)]`, see existing test ~1818)

**Interfaces:**
- Consumes: `flowclone_verify::checksum::{Sha256Writer, hex}`, `FLOW_IMAGE_MAGIC_V2`, `FLOW_IMAGE_VERSION_V2`, `IMAGE_BLOCK_SIZE`.
- Produces: a v2 full/none image with a finalized `payload_sha256` from the in-process path.

- [ ] **Step 1: Add the crate dependency**

In `apps/desktop/src-tauri/Cargo.toml` `[dependencies]`:

```toml
flowclone-verify = { path = "../../../crates/flowclone-verify" }
```

Run `cargo build -p flowclone-desktop` (or `pnpm --filter desktop tauri build` is too heavy — use `cargo build` in `apps/desktop/src-tauri`). Expected: resolves. Adjust the relative path if the workspace already declares the dependency by name (check other crate deps in that Cargo.toml).

- [ ] **Step 2: Write the failing test**

Add to the commands test module (mirror the existing `create_flow_image_file_writes_payload_and_valid_header` test ~1818):

```rust
    #[test]
    fn in_process_image_stores_real_payload_digest() {
        use flowclone_verify::checksum::{hex, sha256};
        let dir = unique_temp_dir("gui-create-digest");
        let src = dir.join("src.bin");
        let img = dir.join("out.flowimg");
        let payload = vec![0x3Cu8; IMAGE_BLOCK_SIZE + 512];
        std::fs::write(&src, &payload).unwrap();
        let source = test_source_sized(payload.len() as u64);

        let cancel = std::sync::atomic::AtomicBool::new(false);
        create_flow_image_file(
            src.to_str().unwrap(),
            img.to_str().unwrap(),
            &source,
            &cancel,
            |_| {},
        )
        .unwrap();

        // Parse the v2 header digest via the existing validation reader.
        let validation = validate_image_path(img.to_str().unwrap()).unwrap();
        assert_eq!(validation.version, FLOW_IMAGE_VERSION_V2);
        // The digest itself is checked by reading the header field — see Step 5
        // helper `read_payload_digest`.
        let stored = read_payload_digest(img.to_str().unwrap()).unwrap();
        assert_eq!(stored, hex(&sha256(&payload)));
    }
```

(Reuse / add `unique_temp_dir` and `test_source_sized` helpers in this module, mirroring the CLI ones.)

- [ ] **Step 3: Run the test to verify it fails**

Run: `cargo test -p flowclone-desktop in_process_image_stores_real_payload_digest`
Expected: FAIL — image is v1 / `read_payload_digest` not found.

- [ ] **Step 4: Add v2 header writer + digest rewrite (mirror of CLI)**

Add to `commands.rs` (mirroring Task 2/3, adapting to this file's `String`-error style). Add a serialize struct, constant, writer, and rewrite helper:

```rust
const UNFINALIZED_DIGEST_HEX: &str =
    "0000000000000000000000000000000000000000000000000000000000000000";
const HEADER_OFFSET_V2: u64 = FLOW_IMAGE_MAGIC_V2.len() as u64 + IMAGE_HEADER_LEN_BYTES as u64;

#[derive(Serialize)]
struct FlowImageHeaderV2Write<'a> {
    format: &'a str,
    version: u64,
    source: &'a DiskInfo,
    block_size: u64,
    uncompressed_bytes: u64,
    compression: &'a str,
    mode: &'a str,
    payload_sha256: &'a str,
    note: &'a str,
}

fn flow_image_header_v2_full(source: &DiskInfo, payload_sha256: &str) -> Result<String, String> {
    serde_json::to_string(&FlowImageHeaderV2Write {
        format: FLOW_IMAGE_FORMAT,
        version: FLOW_IMAGE_VERSION_V2,
        source,
        block_size: IMAGE_BLOCK_SIZE as u64,
        uncompressed_bytes: source.total_bytes,
        compression: "none",
        mode: "full",
        payload_sha256,
        note: "Disk payload follows this header.",
    })
    .map_err(|error| format!("failed to serialize image header: {error}"))
}

fn write_flow_image_header_v2_full(
    writer: &mut impl Write,
    source: &DiskInfo,
    payload_sha256: &str,
) -> Result<(), String> {
    let header = flow_image_header_v2_full(source, payload_sha256)?;
    writer
        .write_all(FLOW_IMAGE_MAGIC_V2)
        .map_err(|error| format!("failed to write image magic: {error}"))?;
    writer
        .write_all(&(header.len() as u64).to_le_bytes())
        .map_err(|error| format!("failed to write image header length: {error}"))?;
    writer
        .write_all(header.as_bytes())
        .map_err(|error| format!("failed to write image header: {error}"))
}

fn rewrite_header_digest_full(
    image: &mut File,
    source: &DiskInfo,
    digest_hex: &str,
) -> Result<(), String> {
    let header = flow_image_header_v2_full(source, digest_hex)?;
    image
        .seek(SeekFrom::Start(HEADER_OFFSET_V2))
        .map_err(|error| format!("failed to seek image header: {error}"))?;
    image
        .write_all(header.as_bytes())
        .map_err(|error| format!("failed to rewrite image header: {error}"))
}

/// Read the `payload_sha256` from a v2 image header (test + diagnostics helper).
fn read_payload_digest(image_path: &str) -> Result<String, String> {
    let mut file = File::open(image_path).map_err(|e| e.to_string())?;
    let mut magic = vec![0u8; FLOW_IMAGE_MAGIC_V2.len()];
    file.read_exact(&mut magic).map_err(|e| e.to_string())?;
    let mut len = [0u8; IMAGE_HEADER_LEN_BYTES];
    file.read_exact(&mut len).map_err(|e| e.to_string())?;
    let header_len = u64::from_le_bytes(len) as usize;
    let mut header = vec![0u8; header_len];
    file.read_exact(&mut header).map_err(|e| e.to_string())?;
    let parsed: FlowImageHeaderV2 =
        serde_json::from_slice(&header).map_err(|e| e.to_string())?;
    parsed
        .payload_sha256
        .ok_or_else(|| "no payload_sha256 in header".to_string())
}
```

Add `payload_sha256: Option<String>` (with `#[serde(default)]`) to the existing `FlowImageHeaderV2` deserialize struct (~77). Ensure `use std::io::{Seek, SeekFrom, Write, Read};` covers these (extend the existing `use` as needed).

- [ ] **Step 5: Switch the in-process writer to v2 + hash + rewrite**

In `create_flow_image_file` (~786): replace `write_flow_image_header(&mut image, source)?;` with `write_flow_image_header_v2_full(&mut image, source, UNFINALIZED_DIGEST_HEX)?;`. Wrap the write loop's `image` in a `Sha256Writer`. Since the loop calls `image.write_all`, introduce `let mut hashing = flowclone_verify::checksum::Sha256Writer::new(image);` after the header write, change the loop's `image.write_all(&buf[..read])` to `hashing.write_all(&buf[..read])`, and after the loop:

```rust
    let (digest, mut image) = hashing.into_parts();
    let digest_hex = flowclone_verify::checksum::hex(&digest);
    rewrite_header_digest_full(&mut image, source, &digest_hex)?;
    image
        .sync_all()
        .map_err(|error| format!("failed to flush image file: {error}"))?;
    drop(image);
    std::fs::rename(&partial_path, image_path)
        .map_err(|error| format!("failed to finalize image file: {error}"))?;
```

Note: the cancel branch deletes `partial_path` and returns; keep that inside the loop using `hashing.get_ref()`-free logic — on cancel just `return Err(...)` (the partial file is removed; the `Sha256Writer` is dropped). To delete the partial file on cancel you no longer hold `image` directly; remove `&partial_path` first, then return. Adjust the cancel block to `let _ = std::fs::remove_file(&partial_path); return Err("cancelled".into());` (it already does this — it does not need the file handle).

- [ ] **Step 6: Run tests**

Run: `cargo test -p flowclone-desktop`
Expected: PASS. The existing `create_flow_image_file_writes_payload_and_valid_header` test may assert a v1 magic/version — update it to expect `FLOW_IMAGE_MAGIC_V2` / `FLOW_IMAGE_VERSION_V2` (the in-process image is now v2 full/none).

- [ ] **Step 7: fmt + clippy + commit**

```bash
cargo fmt -p flowclone-desktop
cargo clippy -p flowclone-desktop --all-targets -- -D warnings
git add apps/desktop/src-tauri/Cargo.toml apps/desktop/src-tauri/src/commands.rs Cargo.lock
git commit -m "feat(desktop): in-process create writes verifiable v2 images"
```

---

### Task 6: Tauri `verify_image` command + progress event + TS wrapper

Expose an unprivileged command that runs the CLI `verify-image`, streams progress, and returns the result. No elevation (it reads a file).

**Files:**
- Modify: `apps/desktop/src-tauri/src/commands.rs` (add `verify_image` command + a `VerifyOutcome` serialize struct; find the sidecar via the existing `flowclone_cli_path()` helper used by restore ~556-577)
- Modify: `apps/desktop/src-tauri/src/lib.rs` (register in `generate_handler![...]`)
- Modify: `apps/desktop/src/lib/tauri.ts` (typed wrapper + mock fallback)
- Modify: `apps/desktop/src/lib/types.ts` and `packages/shared-types` (the `VerifyOutcome` TS type)
- Test: `pnpm typecheck`

**Interfaces:**
- Consumes: the CLI `verify-image --image <path>` JSON line; the sidecar-locating helper used by `restore_image_stub`.
- Produces:
  - Rust: `#[tauri::command] pub async fn verify_image(app: AppHandle, image_path: String) -> Result<VerifyOutcome, String>`.
  - TS: `verifyImage(imagePath: string): Promise<VerifyOutcome>` in `tauri.ts`.
  - `VerifyOutcome` shape (Rust `serde` + TS): `{ verifiable: boolean; matched: boolean; bytesChecked: number; elapsedSecs: number; expected: string | null; actual: string | null }`.

- [ ] **Step 1: Add the `VerifyOutcome` struct + command (Rust)**

In `commands.rs`, add a serialize struct (camelCase for the frontend) and the command. Run the sidecar **without** elevation — mirror how `restore_image_stub` locates the CLI (`flowclone_cli_path()`), but use `std::process::Command` directly (no `osascript`). Parse the single JSON line the CLI prints:

```rust
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyOutcome {
    pub verifiable: bool,
    pub matched: bool,
    pub bytes_checked: u64,
    pub elapsed_secs: f64,
    pub expected: Option<String>,
    pub actual: Option<String>,
}

#[derive(Deserialize)]
struct CliVerifyResult {
    matched: bool,
    bytes_checked: u64,
    elapsed_secs: f64,
    #[serde(default = "crate::commands::default_true")]
    verifiable: bool,
    #[serde(default)]
    expected: Option<String>,
    #[serde(default)]
    actual: Option<String>,
}

pub fn default_true() -> bool {
    true
}

/// Verify a `.flowimg` against its stored checksum. Read-only, unprivileged —
/// runs the bundled CLI `verify-image` and returns its result.
#[tauri::command]
pub async fn verify_image(app: AppHandle, image_path: String) -> Result<VerifyOutcome, String> {
    let image_path = image_path.trim().to_string();
    if image_path.is_empty() {
        return Err("image path is required".into());
    }
    let cli = flowclone_cli_path(&app)?;
    let output = tokio::task::spawn_blocking(move || {
        std::process::Command::new(cli)
            .arg("verify-image")
            .arg("--image")
            .arg(&image_path)
            .output()
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| format!("failed to run verify-image: {error}"))?;

    // The CLI prints the JSON result on stdout even on a mismatch (non-zero exit).
    let stdout = String::from_utf8_lossy(&output.stdout);
    let line = stdout
        .lines()
        .rev()
        .find(|l| l.trim_start().starts_with('{'))
        .ok_or_else(|| {
            let err = String::from_utf8_lossy(&output.stderr);
            format!("verify-image produced no result: {err}")
        })?;
    let parsed: CliVerifyResult =
        serde_json::from_str(line).map_err(|error| format!("bad verify result: {error}"))?;

    Ok(VerifyOutcome {
        verifiable: parsed.verifiable,
        matched: parsed.matched,
        bytes_checked: parsed.bytes_checked,
        elapsed_secs: parsed.elapsed_secs,
        expected: parsed.expected,
        actual: parsed.actual,
    })
}
```

If `flowclone_cli_path` has a different exact name/signature, match it to the one `restore_image_stub` uses (search for where restore resolves the sidecar path, ~556-577). Progress streaming is optional for v1 (verify of a typical sparse image is fast); the GUI shows an indeterminate "Verifying…" state. If a determinate bar is wanted later, have the CLI write a `<image>.verify-progress` file like restore and poll it — out of scope for this task.

- [ ] **Step 2: Register the command**

In `apps/desktop/src-tauri/src/lib.rs`, add `commands::verify_image` to the `tauri::generate_handler![...]` list.

- [ ] **Step 3: Build (Rust)**

Run: `cargo build -p flowclone-desktop`
Expected: builds.

- [ ] **Step 4: Add the TS type**

In `apps/desktop/src/lib/types.ts` (and mirror in `packages/shared-types` per the project's convention):

```ts
export interface VerifyOutcome {
  verifiable: boolean;
  matched: boolean;
  bytesChecked: number;
  elapsedSecs: number;
  expected: string | null;
  actual: string | null;
}
```

- [ ] **Step 5: Add the typed wrapper + mock fallback**

In `apps/desktop/src/lib/tauri.ts`, add a wrapper mirroring the existing command wrappers (they invoke `@tauri-apps/api/core`'s `invoke` and provide a browser/mock fallback when no Tauri runtime is present):

```ts
export async function verifyImage(imagePath: string): Promise<VerifyOutcome> {
  if (!hasTauri()) {
    // Browser/mock: pretend any image verifies.
    return {
      verifiable: true,
      matched: true,
      bytesChecked: 0,
      elapsedSecs: 0,
      expected: null,
      actual: null,
    };
  }
  return invoke<VerifyOutcome>("verify_image", { imagePath });
}
```

Use the file's existing `hasTauri()` / `invoke` helpers and import `VerifyOutcome` from `./types`. Match the existing argument-casing convention (the other wrappers show whether Tauri expects `imagePath` or `image_path`).

- [ ] **Step 6: Typecheck**

Run: `pnpm typecheck`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add apps/desktop/src-tauri/src/commands.rs apps/desktop/src-tauri/src/lib.rs \
        apps/desktop/src/lib/tauri.ts apps/desktop/src/lib/types.ts packages/shared-types
git commit -m "feat(desktop): add unprivileged verify_image command and TS wrapper"
```

---

### Task 7: GUI — verify after create + standalone verify

Two entry points (post-create and pick-a-file) sharing one result UI with three states.

**Files:**
- Modify: the completed-screen component (find it under `apps/desktop/src/features/**` — the screen shown after a create finishes; the flow is `home → confirmation → cloning → completed` per `docs/DESIGN.md`)
- Modify: `apps/desktop/src/features/disk-selection/HomeScreen.tsx` (add a "Verify an image…" affordance) or the home screen's action area
- Modify: `apps/desktop/src/lib/i18n.ts` (EN + TH strings)
- Possibly modify: `apps/desktop/src/stores/flow-store.ts` (a small `verifyState` slice) — only if local component state is insufficient
- Test: `pnpm typecheck`, `pnpm lint`, manual Tauri run

**Interfaces:**
- Consumes: `verifyImage` from `lib/tauri.ts`, `VerifyOutcome` from `lib/types.ts`.
- Produces: a reusable `VerifyResultBanner` (or inline block) rendering verified / corrupt / unverifiable.

- [ ] **Step 1: Add i18n strings (EN + TH)**

In `apps/desktop/src/lib/i18n.ts`, add keys to both the English and Thai dictionaries (match the file's existing structure):

```ts
// English
verifyImage: "Verify image",
verifyPick: "Verify an image…",
verifying: "Verifying…",
verifyVerified: "Verified — checksum matches",
verifyCorrupt: "This image is corrupt — checksum does not match",
verifyUnverifiable: "This image was created before checksums were added, so it can't be verified",
verifyExpected: "Expected",
verifyActual: "Actual",
```

```ts
// Thai (ไทย)
verifyImage: "ตรวจสอบอิมเมจ",
verifyPick: "ตรวจสอบอิมเมจ…",
verifying: "กำลังตรวจสอบ…",
verifyVerified: "ตรวจสอบแล้ว — checksum ตรงกัน",
verifyCorrupt: "อิมเมจนี้เสียหาย — checksum ไม่ตรงกัน",
verifyUnverifiable: "อิมเมจนี้สร้างก่อนจะมีระบบ checksum จึงตรวจสอบไม่ได้",
verifyExpected: "ค่าที่คาดไว้",
verifyActual: "ค่าที่ได้จริง",
```

- [ ] **Step 2: Add the result banner component**

Create `apps/desktop/src/features/verify/VerifyResultBanner.tsx` (new feature folder), a presentational component taking `{ state: "idle" | "running" | VerifyOutcome }` and rendering, using the existing UI primitives in `components/ui` and the `t()` i18n helper:

- `running` → spinner + `t("verifying")`.
- `VerifyOutcome` with `verifiable && matched` → success style + `t("verifyVerified")` + `bytesChecked` humanized.
- `verifiable && !matched` → danger style + `t("verifyCorrupt")` + `expected`/`actual` (monospace, truncated).
- `!verifiable` → muted/warning style + `t("verifyUnverifiable")`.

Keep it pure; the caller owns the async call and passes state down (container/presentational split per the project's patterns).

- [ ] **Step 3: Wire post-create verify**

In the completed screen, add a `t("verifyImage")` button that calls `verifyImage(createdImagePath)` (the path of the image just created — already in the completed-screen props/store), holding `state` in local React state (`useState<"idle" | "running" | VerifyOutcome>`), and renders `<VerifyResultBanner state={state} />`. Set `running` before the await, the outcome after; on a thrown error show a generic error toast/banner using the existing error UI.

- [ ] **Step 4: Wire standalone verify**

On the home screen action area, add a `t("verifyPick")` control that opens the file picker (use the same `@tauri-apps/plugin-dialog` `open` the restore flow uses to pick a `.flowimg`), then runs the same `verifyImage(path)` + banner flow. Reuse the component from Step 2.

- [ ] **Step 5: Typecheck + lint**

Run: `pnpm typecheck && pnpm lint`
Expected: PASS.

- [ ] **Step 6: Manual verification (Tauri)**

```bash
pnpm sidecar            # build the CLI sidecar so verify_image can find it
pnpm dev
```
Then: create a small image (mock or real), click **Verify image** → "Verified". Corrupt the file in a hex editor (flip a byte past the header) and run **Verify an image…** on it → "corrupt". Pick a pre-checksum image → "can't be verified".

- [ ] **Step 7: Commit**

```bash
git add apps/desktop/src/features/verify apps/desktop/src/lib/i18n.ts \
        apps/desktop/src/features apps/desktop/src/stores
git commit -m "feat(desktop): verify image after create and from a file picker"
```

---

## Self-Review

**Spec coverage:**
- Format field + sentinel + no version bump → Task 2. ✓
- Seek-back rewrite → Task 3 (CLI), Task 5 (GUI in-process). ✓
- Hash domain = logical payload, full vs used-only → Tasks 3 (create wraps sink) & 4 (verify decodes to EOF = same bytes). ✓
- Full-uncompressed v1→v2 migration → Task 2 (CLI) + Task 5 (GUI). ✓
- `verify-image` CLI, unverifiable vs mismatch → Task 4. ✓
- `flowclone-verify` streaming helper + `VerifyResult` extension → Tasks 1 & 4. ✓
- `sampler::verify_blockwise` untouched → only its struct literal updates in Task 4. ✓
- Tauri `verify_image` (unprivileged) + TS wrapper + types → Task 6. ✓
- GUI: post-create + standalone, three result states, EN/TH i18n → Task 7. ✓
- Back-compat: legacy v1 / missing / sentinel ⇒ unverifiable → Tasks 4 & 7. ✓
- Duplication mirrored, not refactored → Tasks 2/3 (CLI) mirrored in Task 5 (GUI). ✓
- Out of scope (device-to-device, post-restore compare, auto-verify before erase) → not in any task. ✓

**Type consistency:** `Sha256Writer::into_parts` / `hash_reader` signatures match between Task 1 and their callers (Tasks 3, 4, 5). `VerifyResult` field set (`matched, bytes_checked, blocks_checked, mismatches, elapsed_secs, verifiable, expected, actual`) is identical across Task 4's construction sites and the device sampler updates. `ImageInfo.payload_sha256: Option<String>` defined in Task 3, consumed in Task 4. `VerifyOutcome` fields match between Rust (camelCase serde) and the TS interface in Task 6, consumed in Task 7.

**Placeholder scan:** Tasks 6–7 reference existing patterns to mirror (sidecar resolution, file picker, i18n structure, completed-screen props) rather than reproducing unseen file bodies; exact files, symbols, signatures, and the new code are specified. These are integration points the executor reads in-file, not banned "TODO/implement later" placeholders.
