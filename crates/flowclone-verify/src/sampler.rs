//! Blockwise verification: read both devices in blocks, hash each, compare.
//!
//! Hashing per-block (instead of streaming the whole device) lets us localize
//! mismatches and report progress at block granularity.

use crate::checksum::sha256;
use crate::{Result, VerifyResult};
use std::fs::File;
use std::io::Read;
use std::time::Instant;

/// Verify two devices by hashing matching blocks of each.
pub fn verify_blockwise(
    source: &str,
    target: &str,
    total_bytes: u64,
    block_size: usize,
) -> Result<VerifyResult> {
    let block_size = block_size.max(4096) as u64;
    let start = Instant::now();

    let mut s = File::open(source)?;
    let mut t = File::open(target)?;

    let mut buf_s = vec![0u8; block_size as usize];
    let mut buf_t = vec![0u8; block_size as usize];

    let mut bytes_checked = 0u64;
    let mut blocks_checked = 0u64;
    let mut mismatches = 0u64;

    while bytes_checked < total_bytes {
        let want = block_size.min(total_bytes - bytes_checked) as usize;
        let n_s = read_exact_or_eof(&mut s, &mut buf_s[..want])?;
        let n_t = read_exact_or_eof(&mut t, &mut buf_t[..want])?;

        if n_s == 0 && n_t == 0 {
            break;
        }

        let hash_s = sha256(&buf_s[..n_s]);
        let hash_t = sha256(&buf_t[..n_t]);
        if hash_s != hash_t || n_s != n_t {
            mismatches += 1;
        }

        bytes_checked += n_s.max(n_t) as u64;
        blocks_checked += 1;
    }

    let matched = mismatches == 0;
    Ok(VerifyResult {
        matched,
        bytes_checked,
        blocks_checked,
        mismatches,
        elapsed_secs: start.elapsed().as_secs_f64(),
    })
}

/// Read up to `buf.len()` bytes, returning how many were actually read. `0`
/// signals EOF.
fn read_exact_or_eof(f: &mut File, buf: &mut [u8]) -> Result<usize> {
    Ok(f.read(buf)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn matching_files_verify() {
        let dir = tempfile_dir("matching");
        let a = dir.join("a.bin");
        let b = dir.join("b.bin");
        let payload = [42u8; 8192];
        std::fs::write(&a, payload).unwrap();
        std::fs::write(&b, payload).unwrap();

        let r = verify_blockwise(
            a.to_str().unwrap(),
            b.to_str().unwrap(),
            payload.len() as u64,
            4096,
        )
        .unwrap();
        assert!(r.matched);
        assert_eq!(r.bytes_checked, payload.len() as u64);
    }

    #[test]
    fn differing_files_do_not_match() {
        let dir = tempfile_dir("differing");
        let a = dir.join("a.bin");
        let b = dir.join("b.bin");
        std::fs::write(&a, [1u8; 4096]).unwrap();
        let mut bf = std::fs::File::create(&b).unwrap();
        bf.write_all(&[1u8; 2048]).unwrap();
        bf.write_all(&[2u8; 2048]).unwrap();

        let r = verify_blockwise(a.to_str().unwrap(), b.to_str().unwrap(), 4096, 4096).unwrap();
        assert!(!r.matched);
        assert_eq!(r.mismatches, 1);
    }

    fn tempfile_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "flowclone-verify-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
