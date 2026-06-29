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

/// The NTFS boot-sector (BPB) fields needed to locate and size the `$Bitmap`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NtfsBoot {
    pub bytes_per_sector: u64,
    pub sectors_per_cluster: u64,
    pub total_sectors: u64,
    /// Cluster number (LCN) of the `$MFT`.
    pub mft_lcn: u64,
    /// Size of one MFT record, in bytes.
    pub mft_record_size: u64,
}

impl NtfsBoot {
    pub fn cluster_size(&self) -> u64 {
        self.bytes_per_sector * self.sectors_per_cluster
    }

    /// Total clusters in the volume (the `$Bitmap` has one bit per cluster).
    pub fn total_clusters(&self) -> u64 {
        self.total_sectors / self.sectors_per_cluster
    }
}

const NTFS_OEM_ID: &[u8; 8] = b"NTFS    ";
/// Plausibility caps — a value outside these means "don't trust it, fall back to
/// a full image" rather than risk a wrong layout.
const MAX_BYTES_PER_SECTOR: u64 = 4096;
const MAX_CLUSTER_SIZE: u64 = 2 * 1024 * 1024;
const MAX_MFT_RECORD_SIZE: u64 = 64 * 1024;

/// Parse an NTFS boot sector (the first sector of an NTFS partition).
pub fn parse_ntfs_boot(boot: &[u8]) -> Result<NtfsBoot> {
    if boot.len() < 512 {
        anyhow::bail!("boot sector too short");
    }
    if &boot[3..11] != NTFS_OEM_ID {
        anyhow::bail!("not an NTFS partition");
    }

    let bytes_per_sector = u16::from_le_bytes(boot[11..13].try_into().unwrap()) as u64;
    if !(256..=MAX_BYTES_PER_SECTOR).contains(&bytes_per_sector)
        || !bytes_per_sector.is_power_of_two()
    {
        anyhow::bail!("implausible bytes-per-sector: {bytes_per_sector}");
    }

    // Sectors per cluster: a literal power of two when <= 0x80, otherwise a
    // negative power (`2^(256 - value)`) for very large clusters.
    let spc_raw = boot[13];
    let sectors_per_cluster = if spc_raw <= 0x80 {
        spc_raw as u64
    } else {
        1u64 << (256 - spc_raw as u32)
    };
    if sectors_per_cluster == 0 || !sectors_per_cluster.is_power_of_two() {
        anyhow::bail!("implausible sectors-per-cluster: {spc_raw:#x}");
    }
    let cluster_size = bytes_per_sector * sectors_per_cluster;
    if cluster_size > MAX_CLUSTER_SIZE {
        anyhow::bail!("implausible cluster size: {cluster_size}");
    }

    let total_sectors = u64::from_le_bytes(boot[40..48].try_into().unwrap());
    if total_sectors == 0 {
        anyhow::bail!("zero total sectors");
    }
    let mft_lcn = u64::from_le_bytes(boot[48..56].try_into().unwrap());

    // MFT record size: a positive value is in clusters; a negative one is
    // `2^(-value)` bytes (the common 1024-byte record is stored as -10).
    let clusters_per_mft = boot[64] as i8;
    let mft_record_size = if clusters_per_mft >= 0 {
        clusters_per_mft as u64 * cluster_size
    } else {
        1u64 << ((-clusters_per_mft) as u32)
    };
    if !(256..=MAX_MFT_RECORD_SIZE).contains(&mft_record_size) || !mft_record_size.is_power_of_two()
    {
        anyhow::bail!("implausible MFT record size: {mft_record_size}");
    }

    Ok(NtfsBoot {
        bytes_per_sector,
        sectors_per_cluster,
        total_sectors,
        mft_lcn,
        mft_record_size,
    })
}

/// Apply the NTFS Update Sequence Array fixups to a FILE/INDX record in place.
///
/// NTFS overwrites the last two bytes of every `sector_size` chunk of a record
/// with an update-sequence number; the real bytes live in the USA. Restoring
/// them is mandatory before reading any field that may straddle a sector
/// boundary (e.g. a long data-run list). A mismatch means the record is torn or
/// corrupt, so we refuse it rather than read garbage.
pub fn apply_fixups(record: &mut [u8], sector_size: usize) -> Result<()> {
    if record.len() < 8 {
        anyhow::bail!("record too short for a USA header");
    }
    let usa_offset = u16::from_le_bytes(record[4..6].try_into().unwrap()) as usize;
    let usa_count = u16::from_le_bytes(record[6..8].try_into().unwrap()) as usize;
    if usa_count == 0 {
        anyhow::bail!("record has no update sequence array");
    }
    let sectors = usa_count - 1; // entry 0 is the USN itself
    if sector_size == 0 || record.len() < sectors * sector_size {
        anyhow::bail!("record too small for its update sequence array");
    }
    if usa_offset + usa_count * 2 > record.len() {
        anyhow::bail!("update sequence array out of range");
    }

    let usn = [record[usa_offset], record[usa_offset + 1]];
    for i in 0..sectors {
        let pos = (i + 1) * sector_size - 2;
        if [record[pos], record[pos + 1]] != usn {
            anyhow::bail!("update sequence mismatch — record is corrupt");
        }
        let entry = usa_offset + 2 * (i + 1);
        let (a, b) = (record[entry], record[entry + 1]);
        record[pos] = a;
        record[pos + 1] = b;
    }
    Ok(())
}

/// A contiguous run of clusters on disk: starting LCN and length in clusters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DataRun {
    pub lcn: u64,
    pub clusters: u64,
}

/// Decode an NTFS data-run list into absolute `(LCN, length)` runs. Sparse runs
/// (no offset) are skipped — `$Bitmap` is never sparse, and skipping is the safe
/// default since those clusters hold no on-disk data.
pub fn parse_data_runs(bytes: &[u8]) -> Result<Vec<DataRun>> {
    let mut runs = Vec::new();
    let mut lcn: i64 = 0;
    let mut i = 0;
    while i < bytes.len() {
        let header = bytes[i];
        if header == 0 {
            break; // end of the run list
        }
        i += 1;
        let len_size = (header & 0x0F) as usize;
        let off_size = (header >> 4) as usize;
        if len_size == 0 || i + len_size + off_size > bytes.len() {
            anyhow::bail!("truncated data run");
        }

        let mut length: u64 = 0;
        for (b, &byte) in bytes[i..i + len_size].iter().enumerate() {
            length |= (byte as u64) << (8 * b);
        }
        i += len_size;

        if off_size == 0 {
            // Sparse run: no clusters on disk. Skip it.
            continue;
        }
        let mut offset: i64 = 0;
        for (b, &byte) in bytes[i..i + off_size].iter().enumerate() {
            offset |= (byte as i64) << (8 * b);
        }
        let shift = 64 - 8 * off_size as u32; // sign-extend the signed LE offset
        offset = (offset << shift) >> shift;
        i += off_size;

        lcn += offset;
        if lcn < 0 {
            anyhow::bail!("data run resolved to a negative LCN");
        }
        runs.push(DataRun {
            lcn: lcn as u64,
            clusters: length,
        });
    }
    Ok(runs)
}

const ATTR_DATA: u32 = 0x80;
const ATTR_END: u32 = 0xFFFF_FFFF;

/// From a fixed-up MFT FILE record, return the unnamed `$DATA` attribute's
/// on-disk data runs and its real (valid) size in bytes. `$Bitmap`'s `$DATA` is
/// non-resident; a resident one would be unexpected and is rejected.
pub fn data_attribute_runs(record: &[u8]) -> Result<(Vec<DataRun>, u64)> {
    if record.len() < 24 || &record[0..4] != b"FILE" {
        anyhow::bail!("not an MFT FILE record");
    }
    let mut off = u16::from_le_bytes(record[20..22].try_into().unwrap()) as usize;
    while off + 8 <= record.len() {
        let attr_type = u32::from_le_bytes(record[off..off + 4].try_into().unwrap());
        if attr_type == ATTR_END {
            break;
        }
        let attr_len = u32::from_le_bytes(record[off + 4..off + 8].try_into().unwrap()) as usize;
        if attr_len == 0 || off + attr_len > record.len() {
            anyhow::bail!("bad attribute length");
        }
        let non_resident = record[off + 8];
        let name_len = record[off + 9];
        if attr_type == ATTR_DATA && name_len == 0 {
            if non_resident == 0 {
                anyhow::bail!("$DATA is resident (unexpected for $Bitmap)");
            }
            let runs_offset =
                u16::from_le_bytes(record[off + 0x20..off + 0x22].try_into().unwrap()) as usize;
            let real_size = u64::from_le_bytes(record[off + 0x30..off + 0x38].try_into().unwrap());
            let runs_start = off + runs_offset;
            let runs_end = off + attr_len;
            if runs_offset < 0x40 || runs_start > runs_end || runs_end > record.len() {
                anyhow::bail!("data run list out of range");
            }
            return Ok((parse_data_runs(&record[runs_start..runs_end])?, real_size));
        }
        off += attr_len;
    }
    anyhow::bail!("no non-resident unnamed $DATA attribute found");
}

/// `$Bitmap` is MFT record 6; the first MFT records are always contiguous from
/// the `$MFT` LCN, so it can be read directly without walking the MFT's own runs.
const BITMAP_MFT_RECORD: u64 = 6;
/// Cap the bitmap we read so a corrupt header can't trigger a huge allocation
/// (8 MiB covers a 256 GB disk at 4 KiB clusters).
const MAX_BITMAP_BYTES: u64 = 64 * 1024 * 1024;

/// Read the NTFS cluster-allocation `$Bitmap` for the volume at
/// `partition_offset`. Returns enough bytes to cover every cluster (one bit per
/// cluster, bit set = allocated).
pub fn read_ntfs_bitmap<R: Read + Seek>(
    reader: &mut R,
    partition_offset: u64,
    boot: &NtfsBoot,
) -> Result<Vec<u8>> {
    let cluster_size = boot.cluster_size();
    let mft_offset = partition_offset + boot.mft_lcn * cluster_size;
    let record_offset = mft_offset + BITMAP_MFT_RECORD * boot.mft_record_size;

    let mut record = vec![0u8; boot.mft_record_size as usize];
    reader.seek(SeekFrom::Start(record_offset))?;
    reader.read_exact(&mut record)?;
    apply_fixups(&mut record, boot.bytes_per_sector as usize)?;
    let (runs, real_size) = data_attribute_runs(&record)?;

    // We only need enough bytes to cover every cluster; ignore any padding the
    // attribute's real size carries beyond that.
    let needed = boot.total_clusters().div_ceil(8);
    if real_size < needed {
        anyhow::bail!("$Bitmap is smaller than the volume needs");
    }
    if needed > MAX_BITMAP_BYTES {
        anyhow::bail!("$Bitmap implausibly large: {needed} bytes");
    }

    let mut bitmap = Vec::with_capacity(needed as usize);
    for run in runs {
        if bitmap.len() as u64 >= needed {
            break;
        }
        let run_bytes = run
            .clusters
            .checked_mul(cluster_size)
            .ok_or_else(|| anyhow::anyhow!("data run overflows"))?;
        let to_read = (needed - bitmap.len() as u64).min(run_bytes);
        reader.seek(SeekFrom::Start(partition_offset + run.lcn * cluster_size))?;
        let mut buf = vec![0u8; to_read as usize];
        reader.read_exact(&mut buf)?;
        bitmap.extend_from_slice(&buf);
    }
    if (bitmap.len() as u64) < needed {
        anyhow::bail!("$Bitmap runs are shorter than the volume needs");
    }
    Ok(bitmap)
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

    fn synthetic_ntfs_boot() -> Vec<u8> {
        let mut boot = vec![0u8; 512];
        put(&mut boot, 3, NTFS_OEM_ID);
        put(&mut boot, 11, &512u16.to_le_bytes()); // bytes per sector
        boot[13] = 8; // sectors per cluster → 4 KiB clusters
        put(&mut boot, 40, &2_000_000u64.to_le_bytes()); // total sectors
        put(&mut boot, 48, &786_432u64.to_le_bytes()); // $MFT LCN
        boot[64] = (-10i8) as u8; // 1024-byte MFT records
        boot
    }

    #[test]
    fn parse_ntfs_boot_reads_geometry() {
        let boot = parse_ntfs_boot(&synthetic_ntfs_boot()).expect("parse boot");
        assert_eq!(boot.bytes_per_sector, 512);
        assert_eq!(boot.sectors_per_cluster, 8);
        assert_eq!(boot.cluster_size(), 4096);
        assert_eq!(boot.total_sectors, 2_000_000);
        assert_eq!(boot.total_clusters(), 250_000);
        assert_eq!(boot.mft_lcn, 786_432);
        assert_eq!(boot.mft_record_size, 1024);
    }

    #[test]
    fn parse_ntfs_boot_rejects_non_ntfs_and_insane_values() {
        assert!(parse_ntfs_boot(&[0u8; 512]).is_err()); // no NTFS OEM id
        let mut boot = synthetic_ntfs_boot();
        put(&mut boot, 11, &777u16.to_le_bytes()); // not a power of two
        assert!(parse_ntfs_boot(&boot).is_err());
    }

    #[test]
    fn apply_fixups_restores_sector_tails() {
        let sector = 512usize;
        let mut record = vec![0u8; 2 * sector];
        put(&mut record, 0, b"FILE");
        put(&mut record, 4, &0x30u16.to_le_bytes()); // USA offset
        put(&mut record, 6, &3u16.to_le_bytes()); // USN + 2 sectors
        let usn = [0xAA, 0xBB];
        put(&mut record, 0x30, &usn);
        put(&mut record, 0x32, &[0x01, 0x02]); // saved tail of sector 0
        put(&mut record, 0x34, &[0x03, 0x04]); // saved tail of sector 1
        put(&mut record, sector - 2, &usn); // current tails hold the USN
        put(&mut record, 2 * sector - 2, &usn);

        apply_fixups(&mut record, sector).expect("fixups");
        assert_eq!(&record[sector - 2..sector], &[0x01, 0x02]);
        assert_eq!(&record[2 * sector - 2..2 * sector], &[0x03, 0x04]);
    }

    #[test]
    fn apply_fixups_rejects_a_torn_record() {
        let sector = 512usize;
        let mut record = vec![0u8; 2 * sector];
        put(&mut record, 0, b"FILE");
        put(&mut record, 4, &0x30u16.to_le_bytes());
        put(&mut record, 6, &3u16.to_le_bytes());
        let usn = [0xAA, 0xBB];
        put(&mut record, 0x30, &usn);
        put(&mut record, sector - 2, &usn); // sector 0 tail matches
        put(&mut record, 2 * sector - 2, &[0, 0]); // sector 1 tail does not
        assert!(apply_fixups(&mut record, sector).is_err());
    }

    #[test]
    fn parse_data_runs_decodes_lengths_and_signed_offsets() {
        // 0x21: len 1B (=4), off 2B (=32)  -> LCN 32, 4 clusters
        // 0x11: len 1B (=8), off 1B (=-1)  -> LCN 31, 8 clusters
        // 0x00: end
        let bytes = [0x21, 0x04, 0x20, 0x00, 0x11, 0x08, 0xFF, 0x00];
        let runs = parse_data_runs(&bytes).expect("runs");
        assert_eq!(
            runs,
            vec![
                DataRun {
                    lcn: 32,
                    clusters: 4
                },
                DataRun {
                    lcn: 31,
                    clusters: 8
                },
            ]
        );
    }

    #[test]
    fn data_attribute_runs_extracts_nonresident_data() {
        let mut record = vec![0u8; 1024];
        put(&mut record, 0, b"FILE");
        put(&mut record, 20, &0x38u16.to_le_bytes()); // first attribute at 0x38

        let a = 0x38usize;
        put(&mut record, a, &ATTR_DATA.to_le_bytes());
        let attr_len = 0x48u32; // non-resident header (0x40) + 8 bytes of runs
        put(&mut record, a + 4, &attr_len.to_le_bytes());
        record[a + 8] = 1; // non-resident
        record[a + 9] = 0; // unnamed
        put(&mut record, a + 0x20, &0x40u16.to_le_bytes()); // data runs at +0x40
        put(&mut record, a + 0x30, &8192u64.to_le_bytes()); // real size
        put(&mut record, a + 0x40, &[0x21, 0x04, 0x20, 0x00, 0x00]); // LCN 32, 4 clusters
        put(&mut record, a + attr_len as usize, &ATTR_END.to_le_bytes());

        let (runs, real_size) = data_attribute_runs(&record).expect("data runs");
        assert_eq!(real_size, 8192);
        assert_eq!(
            runs,
            vec![DataRun {
                lcn: 32,
                clusters: 4
            }]
        );
    }

    /// A 1024-byte `$Bitmap` MFT record whose non-resident `$DATA` is a single
    /// run at `bitmap_lcn` of `real_size` bytes, with valid fixups.
    fn bitmap_mft_record(bitmap_lcn: u8, real_size: u64) -> Vec<u8> {
        let mut record = vec![0u8; 1024];
        put(&mut record, 0, b"FILE");
        put(&mut record, 4, &0x30u16.to_le_bytes()); // USA offset
        put(&mut record, 6, &3u16.to_le_bytes()); // USN + 2 sectors
        put(&mut record, 20, &0x38u16.to_le_bytes()); // first attribute
        let usn = [0xAA, 0xBB];
        put(&mut record, 0x30, &usn);
        put(&mut record, 512 - 2, &usn); // sector tails currently hold the USN
        put(&mut record, 1024 - 2, &usn);

        let a = 0x38usize;
        put(&mut record, a, &ATTR_DATA.to_le_bytes());
        let attr_len = 0x48u32;
        put(&mut record, a + 4, &attr_len.to_le_bytes());
        record[a + 8] = 1; // non-resident
        put(&mut record, a + 0x20, &0x40u16.to_le_bytes()); // runs offset
        put(&mut record, a + 0x30, &real_size.to_le_bytes());
        // One run: length 1 cluster, offset = bitmap_lcn (1-byte fields).
        put(&mut record, a + 0x40, &[0x11, 0x01, bitmap_lcn, 0x00]);
        put(&mut record, a + attr_len as usize, &ATTR_END.to_le_bytes());
        record
    }

    #[test]
    fn read_ntfs_bitmap_follows_runs_to_the_allocation_bitmap() {
        let partition_offset = 4096u64;
        let boot = NtfsBoot {
            bytes_per_sector: 512,
            sectors_per_cluster: 1,
            total_sectors: 256, // 256 clusters -> 32-byte bitmap
            mft_lcn: 16,
            mft_record_size: 1024,
        };

        let mut disk = vec![0u8; 24 * 1024];
        let rec6 = partition_offset + 16 * 512 + 6 * 1024;
        put(&mut disk, rec6 as usize, &bitmap_mft_record(30, 32));
        let bitmap_at = partition_offset + 30 * 512;
        let pattern: Vec<u8> = (0..32u8).collect();
        put(&mut disk, bitmap_at as usize, &pattern);

        let got =
            read_ntfs_bitmap(&mut Cursor::new(disk), partition_offset, &boot).expect("read bitmap");
        assert_eq!(got, pattern);
    }
}
