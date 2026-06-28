//! Windows raw-disk helpers for the privileged image/restore worker.
//!
//! macOS clears the way for raw I/O with `diskutil unmountDisk`; Windows has no
//! such command. To write a whole `\\.\PHYSICALDRIVE`, every volume on that disk
//! must be locked and dismounted *by the writing process* and the locks held for
//! the duration of the write — otherwise the kernel rejects writes to sectors
//! owned by a mounted filesystem. This module does that with the Win32 volume
//! FSCTLs, holding the volume handles in a guard so the locks live exactly as
//! long as the caller needs them.
//!
//! Reading a disk needs none of this: Windows happily serves raw reads of a
//! `\\.\PHYSICALDRIVE` while its volumes stay mounted, so create-image does not
//! dismount anything.

use anyhow::{anyhow, Result};
use std::ffi::c_void;
use std::fs::{File, OpenOptions};
use std::io;
use std::mem::size_of;
use std::os::windows::ffi::OsStringExt;
use std::os::windows::io::AsRawHandle;
use std::ptr;

type Handle = isize;
const INVALID_HANDLE_VALUE: Handle = -1;

// IOCTL/FSCTL codes are stable, documented control codes (winioctl.h). Spelled
// out here rather than recomputed via CTL_CODE so the destructive ones are
// auditable at a glance.
const IOCTL_STORAGE_GET_DEVICE_NUMBER: u32 = 0x002D_1080;
const IOCTL_DISK_GET_DRIVE_GEOMETRY: u32 = 0x0007_0000;
const FSCTL_LOCK_VOLUME: u32 = 0x0009_0018;
const FSCTL_DISMOUNT_VOLUME: u32 = 0x0009_0020;
const IOCTL_DISK_UPDATE_PROPERTIES: u32 = 0x0007_0140;

// Win32 error codes a flush of a raw device handle can legitimately return.
const ERROR_INVALID_FUNCTION: i32 = 1;
const ERROR_NOT_SUPPORTED: i32 = 50;
// End of FindNextVolumeW enumeration (vs. a real failure).
const ERROR_NO_MORE_FILES: i32 = 18;
// Removable drive with no media — such a volume can't be the target, which has
// media we just validated.
const ERROR_NOT_READY: i32 = 21;
const ERROR_NO_MEDIA_IN_DEVICE: i32 = 1112;

#[link(name = "kernel32")]
extern "system" {
    fn FindFirstVolumeW(lpszVolumeName: *mut u16, cchBufferLength: u32) -> Handle;
    fn FindNextVolumeW(hFindVolume: Handle, lpszVolumeName: *mut u16, cchBufferLength: u32) -> i32;
    fn FindVolumeClose(hFindVolume: Handle) -> i32;
    fn DeviceIoControl(
        hDevice: Handle,
        dwIoControlCode: u32,
        lpInBuffer: *mut c_void,
        nInBufferSize: u32,
        lpOutBuffer: *mut c_void,
        nOutBufferSize: u32,
        lpBytesReturned: *mut u32,
        lpOverlapped: *mut c_void,
    ) -> i32;
}

/// `STORAGE_DEVICE_NUMBER` — `DeviceNumber` is the `\\.\PHYSICALDRIVE{N}` index.
#[repr(C)]
struct StorageDeviceNumber {
    device_type: u32,
    device_number: u32,
    partition_number: u32,
}

/// `DISK_GEOMETRY` — only `bytes_per_sector` (the logical sector size) is used.
#[repr(C)]
struct DiskGeometry {
    cylinders: i64,
    media_type: u32,
    tracks_per_cylinder: u32,
    sectors_per_track: u32,
    bytes_per_sector: u32,
}

/// Locked + dismounted volume handles for one physical disk. Dropping it closes
/// the handles (releasing the locks) and then asks Windows to re-read the
/// partition table so the volumes re-mount — so the disk is restored to a usable
/// state on both the success path and any error/early-return path.
pub struct VolumeLocks {
    disk_number: u32,
    volumes: Vec<File>,
}

impl Drop for VolumeLocks {
    fn drop(&mut self) {
        if self.volumes.is_empty() {
            // Nothing was dismounted (e.g. a blank target), so nothing to remount.
            return;
        }
        // Release the locks (close handles) before the rescan, or the rescan
        // can't re-mount the volumes it's about to rediscover.
        self.volumes.clear();
        update_disk_properties(self.disk_number);
    }
}

/// Parse the physical-drive index out of a `\\.\PHYSICALDRIVE{N}` device path.
pub fn disk_number_from_path(device_path: &str) -> Option<u32> {
    let upper = device_path.to_ascii_uppercase();
    let suffix = upper.strip_prefix("\\\\.\\PHYSICALDRIVE")?;
    suffix.parse::<u32>().ok()
}

/// Whether a `sync_all`/flush error on a raw device handle is benign. Raw device
/// handles do not support buffered-flush semantics, so the kernel can answer a
/// flush with "invalid function"/"not supported"; those are expected, not real
/// failures (the writes are already on the device).
pub fn flush_error_is_benign(error: &io::Error) -> bool {
    matches!(
        error.raw_os_error(),
        Some(ERROR_INVALID_FUNCTION) | Some(ERROR_NOT_SUPPORTED)
    ) || error.kind() == io::ErrorKind::Unsupported
}

/// The physical device number reported for a handle, or `None` if the query
/// fails (the handle is not a disk volume, etc.).
fn device_number(handle: Handle) -> Option<u32> {
    let mut sdn = StorageDeviceNumber {
        device_type: 0,
        device_number: 0,
        partition_number: 0,
    };
    let mut returned: u32 = 0;
    // SAFETY: `handle` is a live volume handle; the out-buffer is a valid local
    // of exactly the size we declare and the call does not retain it.
    let ok = unsafe {
        DeviceIoControl(
            handle,
            IOCTL_STORAGE_GET_DEVICE_NUMBER,
            ptr::null_mut(),
            0,
            &mut sdn as *mut _ as *mut c_void,
            size_of::<StorageDeviceNumber>() as u32,
            &mut returned,
            ptr::null_mut(),
        )
    };
    if ok == 0 {
        None
    } else {
        Some(sdn.device_number)
    }
}

/// Issue an FSCTL with no in/out payload on a file handle.
fn fsctl(file: &File, code: u32) -> bool {
    let mut returned: u32 = 0;
    // SAFETY: a live handle from `file`; the FSCTLs used here take and return no
    // buffers, so all buffer pointers are null/zero as the API allows.
    unsafe {
        DeviceIoControl(
            file.as_raw_handle() as Handle,
            code,
            ptr::null_mut(),
            0,
            ptr::null_mut(),
            0,
            &mut returned,
            ptr::null_mut(),
        ) != 0
    }
}

/// Lock and dismount every volume that lives on physical disk `disk_number`,
/// returning a guard that keeps the locks held until it is dropped.
///
/// This is the safety gate in front of a destructive whole-disk write, so it
/// fails **closed**: a volume we cannot classify (can't open, or its disk number
/// can't be read) might be on the target, so rather than skip it and risk
/// writing under a still-mounted filesystem, we abort the whole restore. The
/// only skips are volumes that affirmatively report a *different* disk, and
/// removable bays with no media (which can't be the target — it has media).
///
/// On any error after one or more volumes were dismounted, the returned guard
/// dropping triggers a rescan that re-mounts them, so the disk isn't left in a
/// half-dismounted state.
pub fn lock_and_dismount_disk(disk_number: u32) -> Result<VolumeLocks> {
    let mut locks = VolumeLocks {
        disk_number,
        volumes: Vec::new(),
    };

    for volume in enumerate_volumes()? {
        // Probe read-only to learn which disk the volume is on.
        match OpenOptions::new().read(true).open(&volume) {
            Ok(probe) => match device_number(probe.as_raw_handle() as Handle) {
                Some(number) if number == disk_number => {} // target — handle below
                Some(_) => continue,                        // a different disk
                None => {
                    // The volume opened but won't report its disk — genuinely
                    // anomalous (normal volumes always answer). Fail closed.
                    return Err(anyhow!(
                        "could not determine which physical disk volume {volume} belongs to; \
                         aborting the restore rather than risk writing under a mounted filesystem"
                    ));
                }
            },
            Err(error) if is_no_media_error(&error) => continue, // empty bay, not the target
            Err(error) => {
                // Can't open this volume to identify it. It *might* be on the
                // target, but it might equally be an unrelated busy/locked volume,
                // and aborting on every such case would make restore brittle.
                // Skip with a warning and rely on the kernel backstop: since
                // Vista, raw writes to sectors owned by a still-mounted volume are
                // rejected, so a missed target volume fails the write cleanly
                // rather than corrupting it.
                eprintln!(
                    "warning: could not inspect volume {volume} ({error}); \
                     if it is on the target disk the write will fail — close other disk tools"
                );
                continue;
            }
        }

        // This volume is on the target disk — take it offline for the write.
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&volume)
            .map_err(|error| {
                anyhow!("failed to open target volume {volume} for exclusive access: {error}")
            })?;

        // A real exclusive write needs the lock held, so require it (retry while
        // other handles close) instead of force-dismounting a busy volume — this
        // mirrors the macOS path, which fails when the disk is in use.
        let mut locked = false;
        for _ in 0..40 {
            if fsctl(&file, FSCTL_LOCK_VOLUME) {
                locked = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        if !locked {
            return Err(anyhow!(
                "volume {volume} on the target is in use; close anything using the disk and retry"
            ));
        }
        if !fsctl(&file, FSCTL_DISMOUNT_VOLUME) {
            return Err(anyhow!(
                "failed to dismount target volume {volume}; close anything using the disk and retry"
            ));
        }
        // Keep the handle open: that holds the lock and blocks auto-remount.
        locks.volumes.push(file);
    }

    // An empty volume set (a blank/unpartitioned target) is fine — raw writes to
    // a disk with no mounted volumes don't need any lock.
    Ok(locks)
}

/// Ask Windows to re-read the partition table of `disk_number` so the restored
/// volumes re-mount. Best-effort — it never fails the restore — but it warns if
/// it can't run, since otherwise the just-restored disk can stay invisible.
pub fn update_disk_properties(disk_number: u32) {
    let path = format!("\\\\.\\PHYSICALDRIVE{disk_number}");
    match OpenOptions::new().read(true).write(true).open(&path) {
        Ok(file) => {
            if !fsctl(&file, IOCTL_DISK_UPDATE_PROPERTIES) {
                eprintln!(
                    "warning: could not rescan {path}; if the disk doesn't reappear, \
                     replug it or rescan in Disk Management"
                );
            }
        }
        Err(error) => eprintln!(
            "warning: could not open {path} to rescan partitions ({error}); if the disk \
             doesn't reappear, replug it or rescan in Disk Management"
        ),
    }
}

/// The target disk's logical sector size (bytes), used to keep raw writes
/// sector-aligned. Falls back to 512 if the query fails: the payload is always a
/// multiple of the *source* logical sector (>= 512), so rounding up to 512 is a
/// no-op that can never write past the end of the disk — unlike a 4096 fallback,
/// which could overshoot a 512e target whose size isn't 4096-aligned.
pub fn logical_sector_size(device_path: &str) -> u32 {
    let geometry = OpenOptions::new()
        .read(true)
        .open(device_path)
        .ok()
        .and_then(|file| {
            let mut geometry = DiskGeometry {
                cylinders: 0,
                media_type: 0,
                tracks_per_cylinder: 0,
                sectors_per_track: 0,
                bytes_per_sector: 0,
            };
            let mut returned: u32 = 0;
            // SAFETY: live handle; out-buffer is a valid local of the declared size.
            let ok = unsafe {
                DeviceIoControl(
                    file.as_raw_handle() as Handle,
                    IOCTL_DISK_GET_DRIVE_GEOMETRY,
                    ptr::null_mut(),
                    0,
                    &mut geometry as *mut _ as *mut c_void,
                    size_of::<DiskGeometry>() as u32,
                    &mut returned,
                    ptr::null_mut(),
                )
            };
            (ok != 0).then_some(geometry.bytes_per_sector)
        });
    match geometry {
        Some(size) if size > 0 => size,
        _ => 512,
    }
}

/// Whether an open error means "removable device with no media" — such a device
/// can't be the restore target, so it's safe to skip rather than abort.
fn is_no_media_error(error: &io::Error) -> bool {
    matches!(
        error.raw_os_error(),
        Some(ERROR_NOT_READY) | Some(ERROR_NO_MEDIA_IN_DEVICE)
    )
}

/// Read-only diagnostic: map each system volume to the physical disk number it
/// lives on, opening nothing for write and locking/dismounting nothing. Used by
/// the hidden `list-volumes` command to sanity-check the matching that the
/// destructive restore relies on.
pub fn volume_disk_map() -> Vec<(String, Option<u32>)> {
    let volumes = match enumerate_volumes() {
        Ok(volumes) => volumes,
        Err(_) => return Vec::new(),
    };
    volumes
        .into_iter()
        .map(|volume| {
            let number = OpenOptions::new()
                .read(true)
                .open(&volume)
                .ok()
                .and_then(|file| device_number(file.as_raw_handle() as Handle));
            (volume, number)
        })
        .collect()
}

/// Enumerate the system's volume GUID paths (without the trailing backslash, so
/// each names the volume *device* and can be opened for FSCTLs).
fn enumerate_volumes() -> Result<Vec<String>> {
    let mut names = Vec::new();
    let mut buf = [0u16; 260];

    // SAFETY: `buf` is a valid writable buffer of the length we pass.
    let find = unsafe { FindFirstVolumeW(buf.as_mut_ptr(), buf.len() as u32) };
    if find == INVALID_HANDLE_VALUE {
        return Err(anyhow!(
            "failed to enumerate volumes: {}",
            io::Error::last_os_error()
        ));
    }

    // A real enumeration failure must surface (not silently truncate the list),
    // or the destructive caller could miss a target volume and write under it.
    let mut result = Ok(());
    loop {
        if let Some(name) = volume_device_path(&buf) {
            names.push(name);
        }
        // SAFETY: `find` is the live handle from FindFirstVolumeW; `buf` is valid.
        let more = unsafe { FindNextVolumeW(find, buf.as_mut_ptr(), buf.len() as u32) };
        if more == 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(ERROR_NO_MORE_FILES) {
                result = Err(anyhow!("failed while enumerating volumes: {error}"));
            }
            break;
        }
    }

    // SAFETY: `find` came from FindFirstVolumeW and is closed exactly once here.
    unsafe {
        FindVolumeClose(find);
    }
    result.map(|()| names)
}

/// Turn a `\\?\Volume{GUID}\` buffer into an openable `\\?\Volume{GUID}` device
/// path (trailing backslash stripped).
fn volume_device_path(buf: &[u16]) -> Option<String> {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    if end == 0 {
        return None;
    }
    let name = std::ffi::OsString::from_wide(&buf[..end])
        .to_string_lossy()
        .into_owned();
    Some(name.trim_end_matches('\\').to_string())
}
