/* <LICENSES> */
/* </LICENSES> */

/* <CODE> */
//! `disk.h` of arm64's efiboot: `struct diskinfo`, everything efiboot knows of a disk.
//!
//! Upstream: sys/arch/arm64/stand/efiboot/disk.h @ 3ce1f3f79392
//!
//! The file carries no notice, only its `$OpenBSD$` line.
//!
//! ## Deviations
//! - The disks are boxed in a vector (`disklist`, `efiboot.rs`) instead of a `TAILQ`; a box
//!   keeps its address, which `f_devdata` and `bootdev_dip` hold, as the C's list entries
//!   do.
//! - `diskio` takes the disk by reference and the buffer as a slice of `nsect` sectors.
//! - `sr_vol` (`struct sr_boot_volume *`) is an untyped pointer, always null: softraid is
//!   not ported (feature `softraid`, `conf.rs`).

use core::ffi::c_void;

use efi::include::efi::EfiBlockIo;
use libsa::hdr::disklabel::Disklabel;
use libsa::stand::StrategyFn;

/// `DISKINFO_FLAG_GOODLABEL`: the disk has an OpenBSD disklabel.
pub const DISKINFO_FLAG_GOODLABEL: u32 = 1 << 0;
/// `DISKINFO_FLAG_HIBVALID`: its swap holds a hibernation image.
pub const DISKINFO_FLAG_HIBVALID: u32 = 1 << 1;

/// `struct efi_diskinfo` (`efi_diskinfo_t`): the block I/O protocol of a disk and its media
/// id.
pub struct EfiDiskinfo {
    /// `blkio`.
    pub blkio: *mut EfiBlockIo,
    /// `mediaid`.
    pub mediaid: u32,
}

/// `diskio`: read or write `nsect` sectors at `off` (in `DEV_BSIZE` units); 0 or -1.
pub type DiskioFn = fn(rw: i32, dip: &DiskInfo, off: u32, nsect: i32, buf: &mut [u8]) -> i32;

/// `struct diskinfo`.
pub struct DiskInfo {
    /// `ed`: the EFI block I/O of the disk.
    pub ed: EfiDiskinfo,
    /// `disklabel`.
    pub disklabel: Disklabel,
    /// `sr_vol`: the softraid volume, null (softraid is not ported).
    #[allow(dead_code)] // read by softraid_arm64.c, not ported (feature softraid)
    pub sr_vol: *mut c_void,
    /// `part`: the partition `efiopen` opened.
    pub part: u32,
    /// `flags`: `DISKINFO_FLAG_*`.
    pub flags: u32,
    /// `diskio`.
    pub diskio: DiskioFn,
    /// `strategy`: libsa's device strategy for it.
    #[allow(dead_code)] // read by softraid_arm64.c's sr_strategy, not ported
    pub strategy: StrategyFn,
}

// SAFETY: efiboot runs on one CPU without threads; the EFI pointers inside are only used
// from it, before ExitBootServices.
unsafe impl Send for DiskInfo {}
/* </CODE> */
