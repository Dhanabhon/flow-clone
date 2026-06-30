//! Hashing primitives used by verification.

use sha2::{Digest, Sha256};
use std::io::{self, Read, Write};

/// Compute the SHA-256 of a byte slice.
pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let out = hasher.finalize();
    let mut buf = [0u8; 32];
    buf.copy_from_slice(&out);
    buf
}

/// Render a 32-byte digest as a lowercase hex string.
pub fn hex(digest: &[u8; 32]) -> String {
    let mut s = String::with_capacity(64);
    for b in digest {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_has_known_digest() {
        let d = hex(&sha256(b""));
        assert_eq!(
            d,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn hex_is_64_chars() {
        assert_eq!(hex(&sha256(b"flowclone")).len(), 64);
    }

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
}
