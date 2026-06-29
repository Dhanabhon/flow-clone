//! Filesystem-aware "used-only" imaging: work out which parts of a disk hold
//! real data so a sparse `.flowimg` can skip the rest.
//!
//! Built bottom-up over Phase 2.2. Right now it parses the GPT to enumerate
//! partitions; NTFS `$Bitmap` parsing and whole-disk block-map assembly land on
//! top of this. The bias is always toward *including* a region — a block wrongly
//! omitted would be lost on restore — so anything we can't understand is kept.
//!
//! Not wired into `create-image` yet; the items below are exercised by tests
//! until the producer lands.
#![allow(dead_code)]

use anyhow::Result;
use std::io::{Read, Seek, SeekFrom};

/// GPT partition type GUID for "Microsoft Basic Data" — the NTFS/exFAT/FAT data
/// partitions on Windows disks — in GPT's on-disk (mixed-endian) byte order.
pub const MICROSOFT_BASIC_DATA_GUID: [u8; 16] = [
    0xA2, 0xA0, 0xD0, 0xEB, 0xE5, 0xB9, 0x33, 0x44, 0x87, 0xC0, 0x68, 0xB6, 0xB7, 0x26, 0x99, 0xC7,
];

const GPT_SIGNATURE: &[u8; 8] = b"EFI PART";
/// Guard against a corrupt header pointing at an absurd entry table.
const MAX_GPT_ENTRIES: u32 = 1024;
const MAX_GPT_ENTRY_SIZE: u32 = 4096;

/// One GPT partition entry, in LBAs. `last_lba` is inclusive (GPT convention).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Partition {
    pub type_guid: [u8; 16],
    pub first_lba: u64,
    pub last_lba: u64,
}

impl Partition {
    /// Whether this is a Microsoft Basic Data partition (candidate for NTFS).
    pub fn is_microsoft_basic_data(&self) -> bool {
        self.type_guid == MICROSOFT_BASIC_DATA_GUID
    }

    /// Byte offset of the partition's first sector on the disk.
    pub fn start_bytes(&self, sector_size: u64) -> u64 {
        self.first_lba * sector_size
    }

    /// Byte length of the partition (`last_lba` is inclusive).
    pub fn len_bytes(&self, sector_size: u64) -> u64 {
        (self.last_lba + 1 - self.first_lba) * sector_size
    }
}

/// Parse the primary GPT and return its used (non-empty) partition entries.
pub fn parse_gpt<R: Read + Seek>(reader: &mut R, sector_size: u64) -> Result<Vec<Partition>> {
    // The primary GPT header lives at LBA 1 (LBA 0 is the protective MBR).
    reader.seek(SeekFrom::Start(sector_size))?;
    let mut header = [0u8; 92];
    reader.read_exact(&mut header)?;
    if &header[0..8] != GPT_SIGNATURE {
        anyhow::bail!("not a GPT disk (missing EFI PART signature)");
    }

    let entries_lba = u64::from_le_bytes(header[72..80].try_into().unwrap());
    let entry_count = u32::from_le_bytes(header[80..84].try_into().unwrap());
    let entry_size = u32::from_le_bytes(header[84..88].try_into().unwrap());
    if entry_count > MAX_GPT_ENTRIES || !(128..=MAX_GPT_ENTRY_SIZE).contains(&entry_size) {
        anyhow::bail!("implausible GPT entry table: {entry_count} x {entry_size}");
    }

    reader.seek(SeekFrom::Start(entries_lba * sector_size))?;
    let mut entry = vec![0u8; entry_size as usize];
    let mut partitions = Vec::new();
    for _ in 0..entry_count {
        reader.read_exact(&mut entry)?;
        let type_guid: [u8; 16] = entry[0..16].try_into().unwrap();
        if type_guid == [0u8; 16] {
            continue; // unused slot
        }
        let first_lba = u64::from_le_bytes(entry[32..40].try_into().unwrap());
        let last_lba = u64::from_le_bytes(entry[40..48].try_into().unwrap());
        partitions.push(Partition {
            type_guid,
            first_lba,
            last_lba,
        });
    }
    Ok(partitions)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn put(buf: &mut [u8], off: usize, bytes: &[u8]) {
        buf[off..off + bytes.len()].copy_from_slice(bytes);
    }

    /// Build a minimal GPT disk image: sector 0 = MBR, 1 = header, 2 = entries.
    fn synthetic_gpt() -> Vec<u8> {
        let sector = 512usize;
        let mut disk = vec![0u8; sector * 6];

        let h = sector; // LBA 1
        put(&mut disk, h, GPT_SIGNATURE);
        put(&mut disk, h + 72, &2u64.to_le_bytes()); // entries at LBA 2
        put(&mut disk, h + 80, &4u32.to_le_bytes()); // 4 entries
        put(&mut disk, h + 84, &128u32.to_le_bytes()); // 128 bytes each

        let e0 = 2 * sector;
        put(&mut disk, e0, &MICROSOFT_BASIC_DATA_GUID);
        put(&mut disk, e0 + 32, &34u64.to_le_bytes());
        put(&mut disk, e0 + 40, &2047u64.to_le_bytes());

        let e1 = e0 + 128;
        put(&mut disk, e1, &[0x11; 16]);
        put(&mut disk, e1 + 32, &2048u64.to_le_bytes());
        put(&mut disk, e1 + 40, &4095u64.to_le_bytes());
        // Entries 2 and 3 stay zero (unused).
        disk
    }

    #[test]
    fn parse_gpt_reads_used_entries_and_skips_empty_ones() {
        let parts = parse_gpt(&mut Cursor::new(synthetic_gpt()), 512).expect("parse gpt");
        assert_eq!(parts.len(), 2);

        assert!(parts[0].is_microsoft_basic_data());
        assert_eq!(parts[0].first_lba, 34);
        assert_eq!(parts[0].last_lba, 2047);
        assert_eq!(parts[0].start_bytes(512), 34 * 512);
        assert_eq!(parts[0].len_bytes(512), (2047 + 1 - 34) * 512);

        assert!(!parts[1].is_microsoft_basic_data());
        assert_eq!(parts[1].first_lba, 2048);
    }

    #[test]
    fn parse_gpt_rejects_a_non_gpt_disk() {
        let disk = vec![0u8; 512 * 4];
        assert!(parse_gpt(&mut Cursor::new(disk), 512).is_err());
    }
}
