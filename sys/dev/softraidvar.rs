/* $OpenBSD: softraidvar.h,v 1.176 2022/12/19 15:27:06 kn Exp $ */
/* <LICENSES> */
/*
 * Copyright (c) 2006 Marco Peereboom <marco@peereboom.us>
 * Copyright (c) 2008 Chris Kuethe <ckuethe@openbsd.org>
 *
 * Permission to use, copy, modify, and distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
 * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
 * ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
 * OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 */
/* </LICENSES> */

//! `<dev/softraidvar.h>`: softraid(4), the software RAID: the on-disk metadata (`struct
//! sr_metadata`, the chunk and optional metadata that follow it), the userland key-derivation
//! hints of the crypto discipline, the boot-time assembly lists, and the kernel's work units,
//! ccbs, chunks, volumes, disciplines and softc. The functions live in `dev/softraid.rs` and
//! the `dev/softraid_*.rs` disciplines.
//!
//! Upstream: sys/dev/softraidvar.h @ 3ce1f3f79392
//!
//! The on-disk structures are `__packed` in C and are read and changed in place, in buffers
//! the I/O code fills, through shared pointers. Here they are `#[repr(C)]` structures of
//! alignment-1 cells: byte arrays are `Cell<[u8; N]>`, integers [`PackedCell<T>`] (a
//! `Cell<[u8; size_of::<T>()]>` read and written in the machine's byte order, as the C
//! stores them). Such a structure has the C's exact size and offsets (checked at compile
//! time), any bytes are a valid value of it ([`SrMetaView`]), and `&SrMetadata` over a
//! buffer is the C's `struct sr_metadata *` into it.
//!
//! The kernel structures (`sr_discipline`, `sr_workunit`, `sr_ccb`, `sr_chunk`, ...) are
//! allocated by `malloc(9)` and shared through pointers at `splbio` (and under `sd_wu_mtx`
//! for the work unit queues), as in C: they travel as `&'static` references (the pool-object
//! idiom), their members are `Cell`s, list links the `queue.h` entries. All of them except
//! [`SrCcb`] are valid as all-zero bytes ([`SrZeroed`]) and are allocated `M_ZERO` as in C;
//! a ccb embeds a `struct buf`, which is not, and is written in place by `sr_ccb_alloc`.
//!
//! ## Deviations
//! - `SR_DEBUG` is not configured: `sr_debug`, `DNPRINTF` and the `SR_D_*` flags do not
//!   exist; the `DNPRINTF` sites in `softraid.rs` are comments.
//! - `union { generic; pbkdf; } _kdfhint` of `struct sr_crypto_kdfinfo` is its larger
//!   member, `struct sr_crypto_pbkdf`, whose first member is the generic hint; the C's
//!   `genkdf`/`pbkdf` shorthands are methods. The union in `struct sr_meta_crypto`
//!   (`_scm_chk`) is its 64 bytes, with `chk_hmac_sha1` accessors.
//! - The C's member-alias macros `ssdi`, `scmi`, `mds`, `genkdf`, `pbkdf`, `chk_hmac_sha1`
//!   are methods of the same names over the real members (`_sdd_invariant`, ...).
//! - `union sd_dis_specific` is [`SdDisSpecific`], its members side by side (the C union
//!   is never read through an inactive member, except one `key_disk` read of `sr_ioctl_vol`
//!   and `sr_ioctl_disk` that `softraid.rs` reads through `mdd_raid1c`).
//! - `struct sr_crypto_wu` keeps the work unit first and the preallocated DMA buffer
//!   (`cr_dmabuf`); its `struct uio`, `struct iovec` and `struct cryptop *` are not members:
//!   a Rust `Cryptop` borrows its buffers, so `softraid_crypto.rs` builds them per I/O (its
//!   port may add members that are valid as zero bytes).
//! - A discipline whose work units are larger than [`SrWorkunit`] (`sd_wu_size`) names the
//!   type with [`SrDiscipline::set_wu_type`]; [`sr_wu_ext`] checks it before the C's cast
//!   (`(struct sr_crypto_wu *)wu`).
//! - The function pointer members are `Cell<Option<fn ...>>`; a method of the same name
//!   calls the hook (`sd.sd_scsi_rw(wu)`), panicking where the C would call through NULL.
//!   Hooks that return an `int` that is 0 or an errno, or 0 or "1 = failure", return
//!   `Result<(), Errno>`; where the C returns a bare 1 the Rust returns `Err(EIO)`.
//! - `sv_chunks` (an array of `chunk_no` pointers) and `sd_ccb` (an array of ccbs) keep
//!   their length beside the pointer and are reached through bounds-checked accessors.
//! - `struct sr_softc`'s `sc_status` is an `UnsafeCell` protected by `sc_lock` (write);
//!   `sc_scsibus` holds the bus device; `sc_targets` are `Option<&'static SrDiscipline>`.
//! - `sr_bootuuid`/`sr_bootkey` (set by amd64's `bios_bootsr`, the boot loader's softraid
//!   key hand-over) live in `softraid.rs`; with Limine nothing hands keys over
//!   (`replaced-by-limine`), so they stay zero.

use core::any::TypeId;
use core::cell::{Cell, UnsafeCell};
use core::ffi::c_void;
use core::ptr::{self, NonNull};

use crate::crypto::md5::MD5_DIGEST_LENGTH;
use crate::dev::biovar::{BioStatus, BiocCreateraid, BiocDiscipline};
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::subr_prf::panic;
use crate::queue_adapter;
use crate::scsi::scsi_all::ScsiSenseData;
use crate::scsi::scsiconf::{ScsiIopool, ScsiXfer, ScsibusSoftc};
use crate::sys::buf::Buf;
use crate::sys::device::{Device, Softc};
use crate::sys::disk::Disk;
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_DEVBUF, M_ZERO};
use crate::sys::mutex::Mutex;
use crate::sys::param::{DEV_BSIZE, MAXPHYS};
use crate::sys::proc::Proc;
use crate::sys::queue::{SlistEntry, SlistHead, TailqEntry, TailqHead};
use crate::sys::rwlock::Rwlock;
use crate::sys::sensors::{Ksensor, Ksensordev, SensorTask};
use crate::sys::task::{Task, Taskq};
use crate::sys::types::{Daddr, Dev};
use crate::sys::vnode::Vnode;

/// `SR_META_VERSION`: bump when sr_metadata changes.
pub const SR_META_VERSION: u32 = 6;
/// `SR_META_SIZE`: save space at chunk beginning (blocks).
pub const SR_META_SIZE: usize = 64;
/// `SR_META_OFFSET`: skip 8192 bytes at chunk beginning (blocks).
pub const SR_META_OFFSET: Daddr = 16;

/// `SR_BOOT_OFFSET`.
pub const SR_BOOT_OFFSET: Daddr = SR_META_OFFSET + SR_META_SIZE as Daddr;
/// `SR_BOOT_LOADER_SIZE`: size of boot loader storage (blocks).
pub const SR_BOOT_LOADER_SIZE: usize = 320;
/// `SR_BOOT_LOADER_OFFSET`.
pub const SR_BOOT_LOADER_OFFSET: Daddr = SR_BOOT_OFFSET;
/// `SR_BOOT_BLOCKS_SIZE`: size of boot block storage (blocks).
pub const SR_BOOT_BLOCKS_SIZE: usize = 128;
/// `SR_BOOT_BLOCKS_OFFSET`.
pub const SR_BOOT_BLOCKS_OFFSET: Daddr = SR_BOOT_LOADER_OFFSET + SR_BOOT_LOADER_SIZE as Daddr;
/// `SR_BOOT_SIZE`.
pub const SR_BOOT_SIZE: usize = SR_BOOT_LOADER_SIZE + SR_BOOT_BLOCKS_SIZE;

/// `SR_CRYPTO_MAXKEYBYTES`: max bytes in a key (AES-XTS-256).
pub const SR_CRYPTO_MAXKEYBYTES: usize = 32;
/// `SR_CRYPTO_MAXKEYS`: max keys per volume.
pub const SR_CRYPTO_MAXKEYS: usize = 32;
/// `SR_CRYPTO_KEYBITS`: AES-XTS with 2 * 256 bit keys.
pub const SR_CRYPTO_KEYBITS: usize = 512;
/// `SR_CRYPTO_KEYBYTES`.
pub const SR_CRYPTO_KEYBYTES: usize = SR_CRYPTO_KEYBITS >> 3;
/// `SR_CRYPTO_KDFHINTBYTES`: size of opaque KDF hint.
pub const SR_CRYPTO_KDFHINTBYTES: usize = 256;
/// `SR_CRYPTO_CHECKBYTES`: size of generic key chksum struct.
pub const SR_CRYPTO_CHECKBYTES: usize = 64;
/// `SR_CRYPTO_KEY_BLKSHIFT`: 0.5TB per key.
pub const SR_CRYPTO_KEY_BLKSHIFT: u32 = 30;
/// `SR_CRYPTO_KEY_BLKSIZE`.
pub const SR_CRYPTO_KEY_BLKSIZE: u64 = 1u64 << SR_CRYPTO_KEY_BLKSHIFT;
/// `SR_CRYPTO_MAXSIZE`.
pub const SR_CRYPTO_MAXSIZE: u64 = SR_CRYPTO_KEY_BLKSIZE * SR_CRYPTO_MAXKEYS as u64;

/// `SR_CRYPTOKDFT_INVALID`.
pub const SR_CRYPTOKDFT_INVALID: u32 = 0;
/// `SR_CRYPTOKDFT_PKCS5_PBKDF2`.
pub const SR_CRYPTOKDFT_PKCS5_PBKDF2: u32 = 1;
/// `SR_CRYPTOKDFT_KEYDISK`.
pub const SR_CRYPTOKDFT_KEYDISK: u32 = 2;
/// `SR_CRYPTOKDFT_BCRYPT_PBKDF`.
pub const SR_CRYPTOKDFT_BCRYPT_PBKDF: u32 = 3;

/// `struct sr_crypto_genkdf`: a generic hint for the KDF performed in userland, not
/// interpreted by the kernel.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SrCryptoGenkdf {
    /// `len`.
    pub len: u32,
    /// `type`: `SR_CRYPTOKDFT_*`.
    pub r#type: u32,
}

/// `struct sr_crypto_pbkdf`: a hint for a PBKDF performed in userland, not interpreted by
/// the kernel.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct SrCryptoPbkdf {
    /// `generic`.
    pub generic: SrCryptoGenkdf,
    /// `rounds`.
    pub rounds: u32,
    /// `salt`.
    pub salt: [u8; 128],
}

impl Default for SrCryptoPbkdf {
    fn default() -> Self {
        Self {
            generic: SrCryptoGenkdf::default(),
            rounds: 0,
            salt: [0; 128],
        }
    }
}

/// `SR_CRYPTOKDF_INVALID`.
pub const SR_CRYPTOKDF_INVALID: u32 = 0;
/// `SR_CRYPTOKDF_KEY`.
pub const SR_CRYPTOKDF_KEY: u32 = 1 << 0;
/// `SR_CRYPTOKDF_HINT`.
pub const SR_CRYPTOKDF_HINT: u32 = 1 << 1;

/// `struct sr_crypto_kdfinfo`: copies masking keys and KDF hints from/to userland. The
/// embedded hint structures are not interpreted by the kernel.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SrCryptoKdfinfo {
    /// `len`.
    pub len: u32,
    /// `flags`: `SR_CRYPTOKDF_*`.
    pub flags: u32,
    /// `maskkey`.
    pub maskkey: [u8; SR_CRYPTO_MAXKEYBYTES],
    /// `_kdfhint`: the union of `generic` and `pbkdf`, whose larger member it is (the generic
    /// hint is its first member).
    pub _kdfhint: SrCryptoPbkdf,
}

impl SrCryptoKdfinfo {
    /// `genkdf` (`_kdfhint.generic`).
    pub fn genkdf(&self) -> &SrCryptoGenkdf {
        &self._kdfhint.generic
    }

    /// `pbkdf` (`_kdfhint.pbkdf`).
    pub fn pbkdf(&self) -> &SrCryptoPbkdf {
        &self._kdfhint
    }
}

/// `SR_IOCTL_GET_KDFHINT`: get KDF hint.
pub const SR_IOCTL_GET_KDFHINT: u32 = 0x01;
/// `SR_IOCTL_CHANGE_PASSPHRASE`: change passphrase.
pub const SR_IOCTL_CHANGE_PASSPHRASE: u32 = 0x02;

/// `struct sr_crypto_kdfpair`: the old and new KDF info of `SR_IOCTL_CHANGE_PASSPHRASE`
/// (user addresses).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct SrCryptoKdfpair {
    /// `kdfinfo1`.
    pub kdfinfo1: *mut SrCryptoKdfinfo,
    /// `kdfsize1`.
    pub kdfsize1: u32,
    /// `kdfinfo2`.
    pub kdfinfo2: *mut SrCryptoKdfinfo,
    /// `kdfsize2`.
    pub kdfsize2: u32,
}

/// `SR_META_V3_SIZE`.
pub const SR_META_V3_SIZE: usize = 64;
/// `SR_META_V3_OFFSET`.
pub const SR_META_V3_OFFSET: Daddr = 16;
/// `SR_META_V3_DATA_OFFSET`.
pub const SR_META_V3_DATA_OFFSET: Daddr = SR_META_V3_OFFSET + SR_META_V3_SIZE as Daddr;

/// `SR_META_F_NATIVE`: native metadata format.
pub const SR_META_F_NATIVE: i32 = 0;
/// `SR_META_F_INVALID`.
pub const SR_META_F_INVALID: i32 = -1;

/// `SR_HEADER_SIZE`.
pub const SR_HEADER_SIZE: usize = SR_META_SIZE + SR_BOOT_SIZE;
/// `SR_DATA_OFFSET`.
pub const SR_DATA_OFFSET: Daddr = SR_META_OFFSET + SR_HEADER_SIZE as Daddr;

/// `SR_HOTSPARE_LEVEL`.
pub const SR_HOTSPARE_LEVEL: u32 = 0xffff_ffff;
/// `SR_HOTSPARE_VOLID`.
pub const SR_HOTSPARE_VOLID: u32 = 0xffff_ffff;
/// `SR_KEYDISK_LEVEL`.
pub const SR_KEYDISK_LEVEL: u32 = 0xffff_fffe;
/// `SR_KEYDISK_VOLID`.
pub const SR_KEYDISK_VOLID: u32 = 0xffff_fffe;

/// `SR_UUID_MAX`.
pub const SR_UUID_MAX: usize = 16;

/// `SR_MAGIC`.
pub const SR_MAGIC: u64 = 0x4d41_5243_6372_616d;

/// `SR_META_DIRTY`.
pub const SR_META_DIRTY: u32 = 0x1;

/// `SR_OPT_INVALID`.
pub const SR_OPT_INVALID: u32 = 0x00;
/// `SR_OPT_CRYPTO`.
pub const SR_OPT_CRYPTO: u32 = 0x01;
/// `SR_OPT_BOOT`.
pub const SR_OPT_BOOT: u32 = 0x02;
/// `SR_OPT_KEYDISK`.
pub const SR_OPT_KEYDISK: u32 = 0x03;

/// `SR_CRYPTOA_AES_XTS_128`.
pub const SR_CRYPTOA_AES_XTS_128: u32 = 1;
/// `SR_CRYPTOA_AES_XTS_256`.
pub const SR_CRYPTOA_AES_XTS_256: u32 = 2;
/// `SR_CRYPTOF_INVALID`.
pub const SR_CRYPTOF_INVALID: u32 = 0;
/// `SR_CRYPTOF_KEY`.
pub const SR_CRYPTOF_KEY: u32 = 1 << 0;
/// `SR_CRYPTOF_KDFHINT`.
pub const SR_CRYPTOF_KDFHINT: u32 = 1 << 1;
/// `SR_CRYPTOM_AES_ECB_256`.
pub const SR_CRYPTOM_AES_ECB_256: u32 = 1;
/// `SR_CRYPTOC_HMAC_SHA1`.
pub const SR_CRYPTOC_HMAC_SHA1: u32 = 1;

/// `SR_MAX_BOOT_DISKS`.
pub const SR_MAX_BOOT_DISKS: usize = 16;

/// `SR_OLD_META_OPT_SIZE`.
pub const SR_OLD_META_OPT_SIZE: usize = 2480;
/// `SR_OLD_META_OPT_OFFSET`.
pub const SR_OLD_META_OPT_OFFSET: usize = 8;
/// `SR_OLD_META_OPT_MD5`.
pub const SR_OLD_META_OPT_MD5: usize = SR_OLD_META_OPT_SIZE - MD5_DIGEST_LENGTH;

/// `SR_MAX_LD`.
pub const SR_MAX_LD: usize = 256;
/// `SR_MAX_CMDS`.
pub const SR_MAX_CMDS: usize = 16;
/// `SR_MAX_STATES`.
pub const SR_MAX_STATES: usize = 7;
/// `SR_VM_IGNORE_DIRTY`.
pub const SR_VM_IGNORE_DIRTY: i32 = 1;
/// `SR_REBUILD_IO_SIZE`: blocks.
pub const SR_REBUILD_IO_SIZE: u64 = 128;

/// `SR_CCB_FREE`.
pub const SR_CCB_FREE: i32 = 0;
/// `SR_CCB_INPROGRESS`.
pub const SR_CCB_INPROGRESS: i32 = 1;
/// `SR_CCB_OK`.
pub const SR_CCB_OK: i32 = 2;
/// `SR_CCB_FAILED`.
pub const SR_CCB_FAILED: i32 = 3;

/// `SR_CCBF_FREEBUF`: free `ccb_buf.b_data`.
pub const SR_CCBF_FREEBUF: i32 = 1 << 0;

/// `SR_WU_FREE`.
pub const SR_WU_FREE: i32 = 0;
/// `SR_WU_INPROGRESS`.
pub const SR_WU_INPROGRESS: i32 = 1;
/// `SR_WU_OK`.
pub const SR_WU_OK: i32 = 2;
/// `SR_WU_FAILED`.
pub const SR_WU_FAILED: i32 = 3;
/// `SR_WU_PARTIALLYFAILED`.
pub const SR_WU_PARTIALLYFAILED: i32 = 4;
/// `SR_WU_DEFERRED`.
pub const SR_WU_DEFERRED: i32 = 5;
/// `SR_WU_PENDING`.
pub const SR_WU_PENDING: i32 = 6;
/// `SR_WU_RESTART`.
pub const SR_WU_RESTART: i32 = 7;
/// `SR_WU_REQUEUE`.
pub const SR_WU_REQUEUE: i32 = 8;
/// `SR_WU_CONSTRUCT`.
pub const SR_WU_CONSTRUCT: i32 = 9;

/// `SR_WUF_REBUILD`: rebuild io.
pub const SR_WUF_REBUILD: i32 = 1 << 0;
/// `SR_WUF_REBUILDIOCOMP`: rebuild io complete.
pub const SR_WUF_REBUILDIOCOMP: i32 = 1 << 1;
/// `SR_WUF_FAIL`: RAID6: failure.
pub const SR_WUF_FAIL: i32 = 1 << 2;
/// `SR_WUF_FAILIOCOMP`.
pub const SR_WUF_FAILIOCOMP: i32 = 1 << 3;
/// `SR_WUF_WAKEUP`: wakeup on I/O completion.
pub const SR_WUF_WAKEUP: i32 = 1 << 4;
/// `SR_WUF_DISCIPLINE`: discipline specific I/O.
pub const SR_WUF_DISCIPLINE: i32 = 1 << 5;
/// `SR_WUF_FAKE`: faked workunit.
pub const SR_WUF_FAKE: i32 = 1 << 6;

/// `SR_RAID0_NOWU`.
pub const SR_RAID0_NOWU: u32 = 16;
/// `SR_RAID1_NOWU`.
pub const SR_RAID1_NOWU: u32 = 16;
/// `SR_RAID5_NOWU`.
pub const SR_RAID5_NOWU: u32 = 16;
/// `SR_RAID6_NOWU`.
pub const SR_RAID6_NOWU: u32 = 16;
/// `SR_CRYPTO_NOWU`.
pub const SR_CRYPTO_NOWU: u32 = 16;
/// `SR_CONCAT_NOWU`.
pub const SR_CONCAT_NOWU: u32 = 16;
/// `SR_RAID1C_NOWU`.
pub const SR_RAID1C_NOWU: u32 = 16;

/// `SR_MD_RAID0`.
pub const SR_MD_RAID0: u8 = 0;
/// `SR_MD_RAID1`.
pub const SR_MD_RAID1: u8 = 1;
/// `SR_MD_RAID5`.
pub const SR_MD_RAID5: u8 = 2;
/// `SR_MD_CACHE`.
pub const SR_MD_CACHE: u8 = 3;
/// `SR_MD_CRYPTO`.
pub const SR_MD_CRYPTO: u8 = 4;
// AOE was 5 and 6.
// SR_MD_RAID4 was 7.
/// `SR_MD_RAID6`.
pub const SR_MD_RAID6: u8 = 8;
/// `SR_MD_CONCAT`.
pub const SR_MD_CONCAT: u8 = 9;
/// `SR_MD_RAID1C`.
pub const SR_MD_RAID1C: u8 = 10;

/// `SR_CAP_SYSTEM_DISK`: attaches as a system disk.
pub const SR_CAP_SYSTEM_DISK: u32 = 0x0000_0001;
/// `SR_CAP_AUTO_ASSEMBLE`: can auto assemble.
pub const SR_CAP_AUTO_ASSEMBLE: u32 = 0x0000_0002;
/// `SR_CAP_REBUILD`: supports rebuild.
pub const SR_CAP_REBUILD: u32 = 0x0000_0004;
/// `SR_CAP_NON_COERCED`: uses non-coerced size.
pub const SR_CAP_NON_COERCED: u32 = 0x0000_0008;
/// `SR_CAP_REDUNDANT`: redundant copies of data.
pub const SR_CAP_REDUNDANT: u32 = 0x0000_0010;

/// The bytes of a [`PackedCell`]'s integer: `[u8; size_of::<T>()]`.
///
/// # Safety
///
/// `Bytes` is `[u8; size_of::<Self>()]`, and `from_ne`/`to_ne` are the machine-order
/// conversions (`from_ne_bytes`/`to_ne_bytes`).
pub unsafe trait PackedInt: Copy {
    /// `[u8; size_of::<Self>()]`.
    type Bytes: Copy;
    /// The zero value's bytes.
    const ZERO: Self::Bytes;
    /// `Self::from_ne_bytes`.
    fn from_ne(b: Self::Bytes) -> Self;
    /// `self.to_ne_bytes()`.
    fn to_ne(self) -> Self::Bytes;
}

macro_rules! packed_int {
    ($($t:ty),*) => {$(
        // SAFETY: `Bytes` is the type's byte array, and the conversions are its own.
        unsafe impl PackedInt for $t {
            type Bytes = [u8; size_of::<$t>()];
            const ZERO: Self::Bytes = [0; size_of::<$t>()];
            fn from_ne(b: Self::Bytes) -> Self {
                <$t>::from_ne_bytes(b)
            }
            fn to_ne(self) -> Self::Bytes {
                self.to_ne_bytes()
            }
        }
    )*};
}

packed_int!(u16, u32, i32, u64, i64);

/// An integer member of a `__packed` structure: its bytes in the machine's order, at any
/// alignment, changed through a shared reference like a `Cell`.
#[repr(transparent)]
pub struct PackedCell<T: PackedInt>(Cell<T::Bytes>);

impl<T: PackedInt> PackedCell<T> {
    /// Zero.
    pub const fn zeroed() -> Self {
        Self(Cell::new(T::ZERO))
    }

    /// The value.
    pub fn get(&self) -> T {
        T::from_ne(self.0.get())
    }

    /// Stores `v`.
    pub fn set(&self, v: T) {
        self.0.set(v.to_ne());
    }
}

impl<T: PackedInt + core::fmt::Debug> core::fmt::Debug for PackedCell<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.get().fmt(f)
    }
}

/// An on-disk (`__packed`) softraid structure viewed in place: `#[repr(C)]` of alignment-1
/// cells, so a `&T` over any bytes of `size_of::<T>()` is a valid value.
///
/// # Safety
///
/// The implementor is `#[repr(C)]`, has alignment 1, no padding, every one of its bytes is
/// inside an `UnsafeCell` (a `Cell`), and every bit pattern is a valid value of it.
pub unsafe trait SrMetaView: Sized {
    /// The structure's bytes, as cells.
    fn cells(&self) -> &[Cell<u8>] {
        // SAFETY: the trait's contract: alignment 1, no padding, every byte in a cell, so the
        // `size_of::<Self>()` bytes at `self` are that many `Cell<u8>`s, borrowed with `self`.
        unsafe {
            core::slice::from_raw_parts(ptr::from_ref(self).cast::<Cell<u8>>(), size_of::<Self>())
        }
    }

    /// A view of the first `size_of::<Self>()` bytes of `area`, `None` when it is shorter.
    fn view(area: &[Cell<u8>]) -> Option<&Self> {
        if area.len() < size_of::<Self>() {
            return None;
        }
        // SAFETY: the trait's contract: alignment 1 and any bytes are a valid value; `area`
        // covers `size_of::<Self>()` cells, borrowed for the result's lifetime.
        Some(unsafe { &*area.as_ptr().cast::<Self>() })
    }

    /// `memcpy(self, src, sizeof(*self))`.
    fn copy_from(&self, src: &Self) {
        cells_copy(self.cells(), src.cells());
    }

    /// `bzero(self, sizeof(*self))`.
    fn bzero(&self) {
        self.cells().iter().for_each(|c| c.set(0));
    }
}

/// `memcpy(dst, src, n)` between cell views, `n` the shorter length; overlapping views are
/// copied as by `memmove`.
pub fn cells_copy(dst: &[Cell<u8>], src: &[Cell<u8>]) {
    let n = dst.len().min(src.len());
    // SAFETY: both ranges are `n` live cells; cells may be written through a shared
    // reference, and `ptr::copy` allows the two ranges to overlap.
    unsafe {
        ptr::copy(
            src.as_ptr().cast::<u8>(),
            dst.as_ptr().cast::<u8>().cast_mut(),
            n,
        )
    };
}

/// Copies `src` into the cells of `dst` (`memcpy` from a byte buffer).
pub fn cells_write(dst: &[Cell<u8>], src: &[u8]) {
    dst.iter().zip(src).for_each(|(d, s)| d.set(*s));
}

/// Copies the cells of `src` into `dst` (`memcpy` into a byte buffer).
pub fn cells_read(dst: &mut [u8], src: &[Cell<u8>]) {
    dst.iter_mut().zip(src).for_each(|(d, s)| *d = s.get());
}

/// A `T` view at byte `off` of `area`, `None` when it does not fit (`(struct T *)((u_int8_t
/// *)area + off)`).
pub fn sr_view<T: SrMetaView>(area: &[Cell<u8>], off: usize) -> Option<&T> {
    T::view(area.get(off..)?)
}

/// A softraid kernel structure that is valid as all-zero bytes, so that `malloc(9)` with
/// `M_ZERO` makes one, as the C does.
///
/// # Safety
///
/// Every member is valid as zero bytes (integers, `Cell`s of them, `Option`s of references
/// or `NonNull`, null raw pointers, empty `queue.h` heads and entries, zeroed tasks and
/// mutexes), and the type needs no `Drop`.
pub unsafe trait SrZeroed: Sized {}

/// `malloc(sizeof(T), M_DEVBUF, flags | M_ZERO)`: a zeroed `T`, `None` when `M_NOWAIT`
/// finds no memory. The caller frees it with [`sr_free`].
pub fn sr_malloc<T: SrZeroed>(flags: i32) -> Option<NonNull<T>> {
    sr_malloc_size::<T>(size_of::<T>(), flags)
}

/// `malloc(size, M_DEVBUF, flags | M_ZERO)` of a `T` that may be followed by more bytes
/// (`size >= sizeof(T)`); `None` when `M_NOWAIT` finds no memory.
pub fn sr_malloc_size<T: SrZeroed>(size: usize, flags: i32) -> Option<NonNull<T>> {
    if size < size_of::<T>() {
        panic(format_args!(
            "sr_malloc_size: {} < {}",
            size,
            size_of::<T>()
        ));
    }
    // malloc's chunks are aligned to their power-of-two size (at least 16 bytes), more than
    // any softraid structure needs; zeroed bytes are a `T` (`SrZeroed`).
    malloc(size, M_DEVBUF, flags | M_ZERO).map(NonNull::cast)
}

/// `free(p, M_DEVBUF, size)` of an [`sr_malloc`] or [`sr_malloc_size`] allocation.
pub fn sr_free<T>(p: NonNull<T>, size: usize) {
    free(p.cast::<u8>(), M_DEVBUF, size);
}

/// `struct sr_uuid`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SrUuid {
    /// `sui_id`.
    pub sui_id: [u8; SR_UUID_MAX],
}

/// `struct sr_disk`: a disk `sr_boot_assembly` has looked at.
pub struct SrDisk {
    /// `sdk_devno`.
    pub sdk_devno: Cell<Dev>,
    /// `sdk_link`.
    pub sdk_link: SlistEntry<SrDisk>,
}

// SAFETY: an integer cell and a list entry, valid as zero bytes; no `Drop`.
unsafe impl SrZeroed for SrDisk {}

queue_adapter!(
    /// `SLIST_HEAD(sr_disk_head, sr_disk)`, through `sdk_link`.
    pub SrDiskLink: SrDisk, sdk_link => SlistEntry<SrDisk>
);

/// `struct sr_disk_head`.
pub type SrDiskHead = SlistHead<SrDiskLink>;

/// `struct sr_meta_invariant`: the part of the metadata the checksum covers.
#[repr(C)]
#[derive(Debug)]
pub struct SrMetaInvariant {
    /// `ssd_magic`: magic id (do not change order of `ssd_magic`, `ssd_version`).
    pub ssd_magic: PackedCell<u64>,
    /// `ssd_version`: meta data version.
    pub ssd_version: PackedCell<u32>,
    /// `ssd_vol_flags`: volume specific flags.
    pub ssd_vol_flags: PackedCell<u32>,
    /// `ssd_uuid`: unique identifier.
    pub ssd_uuid: Cell<SrUuid>,
    /// `ssd_chunk_no`: number of chunks.
    pub ssd_chunk_no: PackedCell<u32>,
    /// `ssd_chunk_id`: chunk identifier.
    pub ssd_chunk_id: PackedCell<u32>,
    /// `ssd_opt_no`: nr of optional md elements.
    pub ssd_opt_no: PackedCell<u32>,
    /// `ssd_secsize`.
    pub ssd_secsize: PackedCell<u32>,
    /// `ssd_volid`: volume id.
    pub ssd_volid: PackedCell<u32>,
    /// `ssd_level`: raid level.
    pub ssd_level: PackedCell<u32>,
    /// `ssd_size`: virt disk size in blocks.
    pub ssd_size: PackedCell<i64>,
    /// `ssd_vendor`: scsi vendor.
    pub ssd_vendor: Cell<[u8; 8]>,
    /// `ssd_product`: scsi product.
    pub ssd_product: Cell<[u8; 16]>,
    /// `ssd_revision`: scsi revision.
    pub ssd_revision: Cell<[u8; 4]>,
    /// `ssd_strip_size`: strip size (optional volume member).
    pub ssd_strip_size: PackedCell<u32>,
}

// SAFETY: `#[repr(C)]` of alignment-1 cells without padding (sizes checked below).
unsafe impl SrMetaView for SrMetaInvariant {}

/// `struct sr_metadata`: the volume metadata at `SR_META_OFFSET` of every chunk, followed on
/// disk by `ssd_chunk_no` [`SrMetaChunk`]s and `ssd_opt_no` optional metadata items.
#[repr(C)]
#[derive(Debug)]
pub struct SrMetadata {
    /// `_sdd_invariant` (`ssdi`).
    pub _sdd_invariant: SrMetaInvariant,
    /// `ssd_checksum`: MD5 of invariant metadata.
    pub ssd_checksum: Cell<[u8; MD5_DIGEST_LENGTH]>,
    /// `ssd_devname`: /dev/XXXXX.
    pub ssd_devname: Cell<[u8; 32]>,
    /// `ssd_meta_flags`: `SR_META_DIRTY`.
    pub ssd_meta_flags: PackedCell<u32>,
    /// `ssd_data_blkno`.
    pub ssd_data_blkno: PackedCell<u32>,
    /// `ssd_ondisk`: on disk version counter.
    pub ssd_ondisk: PackedCell<u64>,
    /// `ssd_rebuild`: last block of rebuild.
    pub ssd_rebuild: PackedCell<i64>,
}

// SAFETY: `#[repr(C)]` of alignment-1 cells without padding (sizes checked below).
unsafe impl SrMetaView for SrMetadata {}
// SAFETY: cells of bytes, valid as zero; no `Drop`.
unsafe impl SrZeroed for SrMetadata {}

impl SrMetadata {
    /// `ssdi` (`_sdd_invariant`).
    pub fn ssdi(&self) -> &SrMetaInvariant {
        &self._sdd_invariant
    }
}

/// `struct sr_meta_chunk_invariant`.
#[repr(C)]
#[derive(Debug)]
pub struct SrMetaChunkInvariant {
    /// `scm_volid`: vd we belong to.
    pub scm_volid: PackedCell<u32>,
    /// `scm_chunk_id`: chunk id.
    pub scm_chunk_id: PackedCell<u32>,
    /// `scm_devname`: /dev/XXXXX.
    pub scm_devname: Cell<[u8; 32]>,
    /// `scm_size`: size of partition in blocks.
    pub scm_size: PackedCell<i64>,
    /// `scm_coerced_size`: coerced sz of part in blk.
    pub scm_coerced_size: PackedCell<i64>,
    /// `scm_uuid`: unique identifier.
    pub scm_uuid: Cell<SrUuid>,
}

// SAFETY: `#[repr(C)]` of alignment-1 cells without padding (sizes checked below).
unsafe impl SrMetaView for SrMetaChunkInvariant {}

/// `struct sr_meta_chunk`: one chunk's metadata.
#[repr(C)]
#[derive(Debug)]
pub struct SrMetaChunk {
    /// `_scm_invariant` (`scmi`).
    pub _scm_invariant: SrMetaChunkInvariant,
    /// `scm_checksum`: MD5 of invariant chunk metadata.
    pub scm_checksum: Cell<[u8; MD5_DIGEST_LENGTH]>,
    /// `scm_status`: use bio `bioc_disk` status.
    pub scm_status: PackedCell<u32>,
}

// SAFETY: `#[repr(C)]` of alignment-1 cells without padding (sizes checked below).
unsafe impl SrMetaView for SrMetaChunk {}

impl SrMetaChunk {
    /// `scmi` (`_scm_invariant`).
    pub fn scmi(&self) -> &SrMetaChunkInvariant {
        &self._scm_invariant
    }
}

/// `struct sr_crypto_chk_hmac_sha1`: check that `HMAC-SHA1_k(decrypted scm_key) ==
/// sch_mac`, where `k = SHA1(masking key)`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SrCryptoChkHmacSha1 {
    /// `sch_mac`.
    pub sch_mac: [u8; 20],
}

/// `struct sr_meta_opt_hdr`: the header of every optional metadata item.
#[repr(C)]
#[derive(Debug)]
pub struct SrMetaOptHdr {
    /// `som_type`: optional metadata type.
    pub som_type: PackedCell<u32>,
    /// `som_length`: optional metadata length.
    pub som_length: PackedCell<u32>,
    /// `som_checksum`.
    pub som_checksum: Cell<[u8; MD5_DIGEST_LENGTH]>,
}

// SAFETY: `#[repr(C)]` of alignment-1 cells without padding (sizes checked below).
unsafe impl SrMetaView for SrMetaOptHdr {}

/// `struct sr_meta_crypto`: the crypto discipline's optional metadata.
#[repr(C)]
#[derive(Debug)]
pub struct SrMetaCrypto {
    /// `scm_hdr`.
    pub scm_hdr: SrMetaOptHdr,
    /// `scm_alg`: vol crypto algorithm (`SR_CRYPTOA_*`).
    pub scm_alg: PackedCell<u32>,
    /// `scm_flags`: key & kdfhint valid (`SR_CRYPTOF_*`).
    pub scm_flags: PackedCell<u32>,
    /// `scm_mask_alg`: disk key masking crypt alg (`SR_CRYPTOM_*`).
    pub scm_mask_alg: PackedCell<u32>,
    /// `scm_pad1`.
    pub scm_pad1: PackedCell<u32>,
    /// `scm_reserved`.
    pub scm_reserved: Cell<[u8; 64]>,
    /// `scm_key`: symmetric keys used for disk encryption.
    pub scm_key: [Cell<[u8; SR_CRYPTO_KEYBYTES]>; SR_CRYPTO_MAXKEYS],
    /// `scm_kdfhint`: hint to kdf algorithm (opaque to kernel).
    pub scm_kdfhint: Cell<[u8; SR_CRYPTO_KDFHINTBYTES]>,
    /// `scm_check_alg`: key chksum algorithm (`SR_CRYPTOC_*`).
    pub scm_check_alg: PackedCell<u32>,
    /// `scm_pad2`.
    pub scm_pad2: PackedCell<u32>,
    /// `_scm_chk`: the union of `chk_hmac_sha1` and `chk_reserved2[64]`.
    pub _scm_chk: Cell<[u8; 64]>,
}

// SAFETY: `#[repr(C)]` of alignment-1 cells without padding (sizes checked below).
unsafe impl SrMetaView for SrMetaCrypto {}

impl SrMetaCrypto {
    /// `chk_hmac_sha1` (`_scm_chk.chk_hmac_sha1`), read.
    pub fn chk_hmac_sha1(&self) -> SrCryptoChkHmacSha1 {
        let mut c = SrCryptoChkHmacSha1::default();
        c.sch_mac.copy_from_slice(&self._scm_chk.get()[..20]);
        c
    }

    /// `chk_hmac_sha1` (`_scm_chk.chk_hmac_sha1`), written.
    pub fn set_chk_hmac_sha1(&self, c: &SrCryptoChkHmacSha1) {
        let mut chk = self._scm_chk.get();
        chk[..20].copy_from_slice(&c.sch_mac);
        self._scm_chk.set(chk);
    }
}

/// `struct sr_meta_boot`: the boot optional metadata (`installboot`).
#[repr(C)]
#[derive(Debug)]
pub struct SrMetaBoot {
    /// `sbm_hdr`.
    pub sbm_hdr: SrMetaOptHdr,
    /// `sbm_bootblk_size`.
    pub sbm_bootblk_size: PackedCell<u32>,
    /// `sbm_bootldr_size`.
    pub sbm_bootldr_size: PackedCell<u32>,
    /// `sbm_root_duid`.
    pub sbm_root_duid: Cell<[u8; 8]>,
    /// `sbm_boot_duid`.
    pub sbm_boot_duid: [Cell<[u8; 8]>; SR_MAX_BOOT_DISKS],
}

// SAFETY: `#[repr(C)]` of alignment-1 cells without padding (sizes checked below).
unsafe impl SrMetaView for SrMetaBoot {}

/// `struct sr_meta_keydisk`: the key disk's optional metadata.
#[repr(C)]
#[derive(Debug)]
pub struct SrMetaKeydisk {
    /// `skm_hdr`.
    pub skm_hdr: SrMetaOptHdr,
    /// `skm_maskkey`.
    pub skm_maskkey: Cell<[u8; SR_CRYPTO_MAXKEYBYTES]>,
}

// SAFETY: `#[repr(C)]` of alignment-1 cells without padding (sizes checked below).
unsafe impl SrMetaView for SrMetaKeydisk {}

/// `struct sr_meta_opt_item`: one optional metadata item of a volume, in a `malloc`ed
/// buffer of `som_length` bytes.
pub struct SrMetaOptItem {
    /// `omi_som`: the item (`som_length` bytes, at least a header); NULL until set.
    omi_som: Cell<Option<NonNull<u8>>>,
    /// The allocation size of `omi_som` (the C frees it with size 0).
    omi_len: Cell<usize>,
    /// `omi_link`.
    pub omi_link: SlistEntry<SrMetaOptItem>,
}

// SAFETY: `Option<NonNull>` (None), an integer cell and a list entry; no `Drop`.
unsafe impl SrZeroed for SrMetaOptItem {}

impl SrMetaOptItem {
    /// `omi = malloc(sizeof(*omi), M_ZERO); omi->omi_som = malloc(len, M_ZERO)`: an item
    /// with a zeroed buffer of `len` bytes (at least a header), not on any list. `None` when
    /// `M_NOWAIT` finds no memory.
    pub fn alloc(len: usize, flags: i32) -> Option<&'static SrMetaOptItem> {
        let len = len.max(size_of::<SrMetaOptHdr>());
        let omi = sr_malloc::<SrMetaOptItem>(flags)?;
        let Some(som) = malloc(len, M_DEVBUF, flags | M_ZERO) else {
            sr_free(omi, size_of::<SrMetaOptItem>());
            return None;
        };
        // SAFETY: a zeroed `SrMetaOptItem` (`SrZeroed`), freed only by `free`.
        let omi: &'static SrMetaOptItem = unsafe { omi.as_ref() };
        omi.omi_som.set(Some(som));
        omi.omi_len.set(len);
        Some(omi)
    }

    /// Frees the item and its buffer (`free(omi->omi_som); free(omi)`).
    ///
    /// # Safety
    ///
    /// `omi` came from [`SrMetaOptItem::alloc`], is on no list, and nothing uses it or a
    /// view of its buffer afterwards.
    pub unsafe fn free(omi: &SrMetaOptItem) {
        if let Some(som) = omi.omi_som.take() {
            free(som, M_DEVBUF, omi.omi_len.get());
        }
        sr_free(NonNull::from(omi), size_of::<SrMetaOptItem>());
    }

    /// The item's buffer, as cells (`som_length` bytes as allocated).
    pub fn som_cells(&self) -> &[Cell<u8>] {
        match self.omi_som.get() {
            // SAFETY: `omi_som` is a live allocation of `omi_len` bytes owned by this item
            // (`alloc`), freed only with it; bytes are valid as cells.
            Some(p) => unsafe {
                core::slice::from_raw_parts(p.as_ptr().cast::<Cell<u8>>(), self.omi_len.get())
            },
            None => &[],
        }
    }

    /// `omi->omi_som`: the item's header.
    pub fn omi_som(&self) -> &SrMetaOptHdr {
        match SrMetaOptHdr::view(self.som_cells()) {
            Some(h) => h,
            None => panic(format_args!("sr_meta_opt_item without buffer")),
        }
    }

    /// `(struct T *)omi->omi_som`: the item as its type's structure, `None` when the buffer
    /// is shorter.
    pub fn som_as<T: SrMetaView>(&self) -> Option<&T> {
        T::view(self.som_cells())
    }
}

queue_adapter!(
    /// `SLIST_HEAD(sr_meta_opt_head, sr_meta_opt_item)`, through `omi_link`.
    pub SrMetaOptLink: SrMetaOptItem, omi_link => SlistEntry<SrMetaOptItem>
);

/// `struct sr_meta_opt_head`.
pub type SrMetaOptHead = SlistHead<SrMetaOptLink>;

/// `struct sr_boot_chunk`: a chunk found at boot (`sr_meta_native_bootprobe`).
pub struct SrBootChunk {
    /// `sbc_metadata`: a `malloc`ed copy of the chunk's `struct sr_metadata`.
    pub sbc_metadata: Cell<Option<NonNull<SrMetadata>>>,
    /// `sbc_mm`: device major/minor.
    pub sbc_mm: Cell<Dev>,
    /// `sbc_chunk_id`: chunk ID.
    pub sbc_chunk_id: Cell<u32>,
    /// `sbc_state`: chunk state.
    pub sbc_state: Cell<u32>,
    /// `sbc_disk`: disk number.
    pub sbc_disk: Cell<u32>,
    /// `sbc_part`: partition number.
    pub sbc_part: Cell<i32>,
    /// `sbc_ondisk`: ondisk version.
    pub sbc_ondisk: Cell<u64>,
    /// `sbc_diskinfo`: MD disk information.
    pub sbc_diskinfo: Cell<*mut c_void>,
    /// `sbc_link`.
    pub sbc_link: SlistEntry<SrBootChunk>,
}

// SAFETY: cells of integers, `Option<NonNull>`, a raw pointer and a list entry; no `Drop`.
unsafe impl SrZeroed for SrBootChunk {}

impl SrBootChunk {
    /// `bc->sbc_metadata`.
    pub fn sbc_metadata(&self) -> &SrMetadata {
        match self.sbc_metadata.get() {
            // SAFETY: a `malloc`ed `sizeof(struct sr_metadata)` copy the boot chunk owns
            // until `sr_boot_assembly` frees it with the chunk; any bytes are a valid view.
            Some(p) => unsafe { p.as_ref() },
            None => panic(format_args!("sr_boot_chunk without metadata")),
        }
    }
}

queue_adapter!(
    /// `SLIST_HEAD(sr_boot_chunk_head, sr_boot_chunk)`, through `sbc_link`.
    pub SrBootChunkLink: SrBootChunk, sbc_link => SlistEntry<SrBootChunk>
);

/// `struct sr_boot_chunk_head`.
pub type SrBootChunkHead = SlistHead<SrBootChunkLink>;

/// `struct sr_boot_volume`: a volume being assembled at boot.
pub struct SrBootVolume {
    /// `sbv_uuid`: volume UUID.
    pub sbv_uuid: Cell<SrUuid>,
    /// `sbv_level`: RAID level.
    pub sbv_level: Cell<u32>,
    /// `sbv_volid`: volume ID.
    pub sbv_volid: Cell<u32>,
    /// `sbv_chunk_no`: number of chunks.
    pub sbv_chunk_no: Cell<u32>,
    /// `sbv_flags`: volume specific flags.
    pub sbv_flags: Cell<u32>,
    /// `sbv_state`: volume state.
    pub sbv_state: Cell<u32>,
    /// `sbv_size`: virtual disk size.
    pub sbv_size: Cell<i64>,
    /// `sbv_secsize`: sector size.
    pub sbv_secsize: Cell<u32>,
    /// `sbv_data_blkno`: data offset.
    pub sbv_data_blkno: Cell<u32>,
    /// `sbv_ondisk`: ondisk version.
    pub sbv_ondisk: Cell<u64>,
    /// `sbv_chunks_found`: number of chunks found.
    pub sbv_chunks_found: Cell<u32>,
    /// `sbv_unit`: disk unit number.
    pub sbv_unit: Cell<u32>,
    /// `sbv_part`: partition opened.
    pub sbv_part: Cell<u8>,
    /// `sbv_diskinfo`: MD disk information.
    pub sbv_diskinfo: Cell<*mut c_void>,
    /// `sbv_keys`: disk keys for volume.
    pub sbv_keys: Cell<*mut u8>,
    /// `sbv_maskkey`: mask key for disk keys.
    pub sbv_maskkey: Cell<*mut u8>,
    /// `sbv_chunks`: list of chunks.
    pub sbv_chunks: SrBootChunkHead,
    /// `sbv_meta_opt`: list of optional metadata.
    pub sbv_meta_opt: SrMetaOptHead,
    /// `sbv_link`.
    pub sbv_link: SlistEntry<SrBootVolume>,
}

// SAFETY: cells of integers and raw pointers, list heads and an entry; no `Drop`.
unsafe impl SrZeroed for SrBootVolume {}

queue_adapter!(
    /// `SLIST_HEAD(sr_boot_volume_head, sr_boot_volume)`, through `sbv_link`.
    pub SrBootVolumeLink: SrBootVolume, sbv_link => SlistEntry<SrBootVolume>
);

/// `struct sr_boot_volume_head`.
pub type SrBootVolumeHead = SlistHead<SrBootVolumeLink>;

/// `DEVNAME(_s)`: the softc's device name.
#[allow(non_snake_case)] // the C macro's name
pub fn DEVNAME(sc: &SrSoftc) -> &str {
    sc.sc_dev.xname()
}

/// `struct sr_ccb`: one I/O to one chunk, a `struct buf` the discipline sends down the
/// chunk's vnode.
#[repr(C)]
pub struct SrCcb {
    /// `ccb_buf`: MUST BE FIRST!! (`sr_raid_intr` and the disciplines' `sd_scsi_intr` get
    /// the ccb back from the buffer: [`sr_ccb_from_buf`]).
    pub ccb_buf: Buf,
    /// `ccb_wu`: the work unit the ccb belongs to.
    pub ccb_wu: Cell<Option<&'static SrWorkunit>>,
    /// `ccb_dis`: the discipline that owns the ccb.
    pub ccb_dis: Cell<*const SrDiscipline>,
    /// `ccb_target`: the chunk, or -1.
    pub ccb_target: Cell<i32>,
    /// `ccb_state`: `SR_CCB_*`.
    pub ccb_state: Cell<i32>,
    /// `ccb_flags`: `SR_CCBF_*`.
    pub ccb_flags: Cell<i32>,
    /// `ccb_opaque`: discipline usable pointer.
    pub ccb_opaque: Cell<*mut c_void>,
    /// `ccb_link`: in the discipline's free queue, or the work unit's `swu_ccb`.
    pub ccb_link: TailqEntry<SrCcb>,
}

impl SrCcb {
    /// A ccb with every member zero, as `mallocarray(..., M_ZERO)` leaves the C's.
    pub const fn new() -> Self {
        Self {
            ccb_buf: Buf::new(),
            ccb_wu: Cell::new(None),
            ccb_dis: Cell::new(ptr::null()),
            ccb_target: Cell::new(0),
            ccb_state: Cell::new(0),
            ccb_flags: Cell::new(0),
            ccb_opaque: Cell::new(ptr::null_mut()),
            ccb_link: TailqEntry::new(),
        }
    }

    /// `ccb->ccb_wu`, which `sr_wu_enqueue_ccb` sets before the ccb is started.
    pub fn wu(&self) -> &'static SrWorkunit {
        match self.ccb_wu.get() {
            Some(wu) => wu,
            None => panic(format_args!("sr_ccb {:p} without work unit", self)),
        }
    }

    /// `ccb->ccb_dis`, which `sr_ccb_alloc` sets.
    pub fn dis(&self) -> &'static SrDiscipline {
        // SAFETY: `sr_ccb_alloc` points every ccb at the discipline whose `sd_ccb` array
        // holds it; the array is freed (`sr_ccb_free`) before the discipline.
        match unsafe { self.ccb_dis.get().as_ref() } {
            Some(sd) => sd,
            None => panic(format_args!("sr_ccb {:p} without discipline", self)),
        }
    }
}

impl Default for SrCcb {
    fn default() -> Self {
        Self::new()
    }
}

/// `(struct sr_ccb *)bp`: the ccb whose `ccb_buf` is `bp`.
///
/// # Safety
///
/// `bp` is the `ccb_buf` of an [`SrCcb`]: the buffer `sr_ccb_rw` set up, given back to its
/// `b_iodone` (`sd_scsi_intr`) or found on a work unit's `swu_ccb`.
pub unsafe fn sr_ccb_from_buf(bp: &'static Buf) -> &'static SrCcb {
    // SAFETY: `SrCcb` is `#[repr(C)]` with `ccb_buf` first, so the buffer's address is the
    // ccb's; the caller guarantees the buffer is a ccb's.
    unsafe { &*ptr::from_ref(bp).cast::<SrCcb>() }
}

queue_adapter!(
    /// `TAILQ_HEAD(sr_ccb_list, sr_ccb)`, through `ccb_link`.
    pub SrCcbLink: SrCcb, ccb_link => TailqEntry<SrCcb>
);

/// `struct sr_ccb_list`.
pub type SrCcbList = TailqHead<SrCcbLink>;

/// `struct sr_workunit`: one SCSI command (or rebuild or discipline I/O) of a volume, made of
/// the ccbs that do it on the chunks. It is the `void *` opening of the discipline's
/// `scsi_iopool` (`xs->io`).
#[repr(C)]
pub struct SrWorkunit {
    /// `swu_xs`: the transfer, or NULL for discipline and rebuild I/O without one.
    pub swu_xs: Cell<Option<&'static ScsiXfer>>,
    /// `swu_dis`: the discipline.
    pub swu_dis: Cell<*const SrDiscipline>,
    /// `swu_state`: `SR_WU_*`.
    pub swu_state: Cell<i32>,
    /// `swu_flags`: additional hints (`SR_WUF_*`).
    pub swu_flags: Cell<i32>,
    /// `swu_blk_start`: workunit io range.
    pub swu_blk_start: Cell<Daddr>,
    /// `swu_blk_end`.
    pub swu_blk_end: Cell<Daddr>,
    /// `swu_io_count`: number of ios that makes up the whole work unit.
    pub swu_io_count: Cell<u32>,
    /// `swu_ios_complete`: in flight totals.
    pub swu_ios_complete: Cell<u32>,
    /// `swu_ios_failed`.
    pub swu_ios_failed: Cell<u32>,
    /// `swu_ios_succeeded`.
    pub swu_ios_succeeded: Cell<u32>,
    /// `swu_collider`: colliding wu.
    pub swu_collider: Cell<Option<&'static SrWorkunit>>,
    /// `swu_ccb`: all ios that make up this workunit.
    pub swu_ccb: SrCcbList,
    /// `swu_task`: task memory (`sr_wu_done_callback`).
    pub swu_task: Task,
    /// `swu_cb_active`: in callback.
    pub swu_cb_active: Cell<i32>,
    /// `swu_link`: link in processing queue.
    pub swu_link: TailqEntry<SrWorkunit>,
    /// `swu_next`: next work unit in chain.
    pub swu_next: TailqEntry<SrWorkunit>,
}

// SAFETY: `Option`s of references and raw pointers (NULL), integer cells, a list head, a
// zeroed task and list entries; no `Drop`.
unsafe impl SrZeroed for SrWorkunit {}

impl SrWorkunit {
    /// `wu->swu_dis`, which `sr_wu_alloc` sets.
    pub fn dis(&self) -> &'static SrDiscipline {
        // SAFETY: `sr_wu_alloc` points every work unit at the discipline that lists it in
        // `sd_wu`; `sr_wu_free` frees the units before the discipline goes.
        match unsafe { self.swu_dis.get().as_ref() } {
            Some(sd) => sd,
            None => panic(format_args!("sr_workunit {:p} without discipline", self)),
        }
    }

    /// `wu->swu_xs`, for the paths where the C dereferences it.
    pub fn xs(&self) -> &'static ScsiXfer {
        match self.swu_xs.get() {
            Some(xs) => xs,
            None => panic(format_args!("sr_workunit {:p} without scsi_xfer", self)),
        }
    }
}

queue_adapter!(
    /// `TAILQ_HEAD(sr_wu_list, sr_workunit)` of the processing queues (free, pending,
    /// deferred), through `swu_link`.
    pub SrWuLink: SrWorkunit, swu_link => TailqEntry<SrWorkunit>
);

queue_adapter!(
    /// `TAILQ_HEAD(sr_wu_list, sr_workunit)` of all the discipline's work units, through
    /// `swu_next`.
    pub SrWuNext: SrWorkunit, swu_next => TailqEntry<SrWorkunit>
);

/// `struct sr_wu_list` on `swu_link` (`sd_wu_freeq`, `sd_wu_pendq`, `sd_wu_defq`).
pub type SrWuList = TailqHead<SrWuLink>;
/// `struct sr_wu_list` on `swu_next` (`sd_wu`).
pub type SrWuAllList = TailqHead<SrWuNext>;

/// A discipline's work unit type that extends [`SrWorkunit`] (`struct sr_crypto_wu`), the
/// discipline's `sd_wu_size`.
///
/// # Safety
///
/// The implementor is `#[repr(C)]` with an [`SrWorkunit`] as its first member, and is
/// [`SrZeroed`] (`sr_wu_alloc` makes it with `M_ZERO`).
pub unsafe trait SrWorkunitExt: SrZeroed + 'static {}

/// `(struct T *)wu`: the discipline's extended work unit. Panics unless the discipline
/// declared `T` with [`SrDiscipline::set_wu_type`].
pub fn sr_wu_ext<T: SrWorkunitExt>(wu: &'static SrWorkunit) -> &'static T {
    let sd = wu.dis();
    let declared = sd.sd_wu_type.get().map(|f| f());
    if declared != Some(TypeId::of::<T>()) || sd.sd_wu_size.get() != size_of::<T>() {
        panic(format_args!(
            "{}: work unit is not of the discipline's type",
            DEVNAME(sd.sd_sc())
        ));
    }
    // SAFETY: the discipline declared `T` (checked above), so `sr_wu_alloc` allocated every
    // work unit as `size_of::<T>()` zeroed bytes, a valid `T` (`SrWorkunitExt`) whose first
    // member, at offset 0, is `wu`.
    unsafe { &*ptr::from_ref(wu).cast::<T>() }
}

/// `struct sr_raid0`.
#[derive(Debug)]
pub struct SrRaid0 {
    /// `sr0_strip_bits`.
    pub sr0_strip_bits: Cell<i32>,
}

/// `struct sr_raid1`.
#[derive(Debug)]
pub struct SrRaid1 {
    /// `sr1_counter`.
    pub sr1_counter: Cell<u32>,
}

/// `struct sr_raid5`.
#[derive(Debug)]
pub struct SrRaid5 {
    /// `sr5_strip_bits`.
    pub sr5_strip_bits: Cell<i32>,
}

/// `struct sr_raid6`.
#[derive(Debug)]
pub struct SrRaid6 {
    /// `sr6_strip_bits`.
    pub sr6_strip_bits: Cell<i32>,
}

/// `struct sr_crypto_wu`: the per-I/O data that we need to preallocate. We cannot afford to
/// allow I/O to start failing when memory pressure kicks in. We can store this in the WU
/// because we assert that only one ccb per WU will ever be active during crypto.
#[repr(C)]
pub struct SrCryptoWu {
    /// `cr_wu`: must be first.
    pub cr_wu: SrWorkunit,
    /// `cr_dmabuf`: a `MAXPHYS` DMA buffer (`dma_alloc`), or NULL.
    pub cr_dmabuf: Cell<*mut u8>,
}

// SAFETY: a zeroable work unit and a raw pointer cell (NULL); no `Drop`.
unsafe impl SrZeroed for SrCryptoWu {}
// SAFETY: `#[repr(C)]` with the work unit first; `SrZeroed`.
unsafe impl SrWorkunitExt for SrCryptoWu {}

/// The size of `cr_dmabuf`.
pub const SR_CRYPTO_DMABUF_SIZE: usize = MAXPHYS;

/// `struct sr_crypto`: the crypto discipline's state.
pub struct SrCrypto {
    /// `scr_meta`: the crypto optional metadata, in an item of `sd_meta_opt` (freed with the
    /// discipline), or NULL.
    pub scr_meta: Cell<*const SrMetaCrypto>,
    /// `key_disk`: the key disk chunk, or NULL.
    pub key_disk: Cell<Option<&'static SrChunk>>,
    /// `scr_alg`.
    pub scr_alg: Cell<i32>,
    /// `scr_klen`.
    pub scr_klen: Cell<i32>,
    /// `scr_key` (XXX only keep `scr_sid` over time).
    pub scr_key: [Cell<[u8; SR_CRYPTO_KEYBYTES]>; SR_CRYPTO_MAXKEYS],
    /// `scr_maskkey`.
    pub scr_maskkey: Cell<[u8; SR_CRYPTO_MAXKEYBYTES]>,
    /// `scr_sid`.
    pub scr_sid: [Cell<u64>; SR_CRYPTO_MAXKEYS],
}

impl SrCrypto {
    /// `mdd_crypto->scr_meta` (NULL is `None`).
    pub fn scr_meta(&self) -> Option<&SrMetaCrypto> {
        // SAFETY: `scr_meta` is NULL or a view into an optional metadata item of the same
        // discipline's `sd_meta_opt`, which `sr_discipline_free` frees with the discipline.
        unsafe { self.scr_meta.get().as_ref() }
    }
}

/// `struct sr_concat`.
#[derive(Debug)]
pub struct SrConcat {}

/// `struct sr_raid1c`.
pub struct SrRaid1c {
    /// `sr1c_crypto`.
    pub sr1c_crypto: SrCrypto,
    /// `sr1c_raid1`.
    pub sr1c_raid1: SrRaid1,
}

/// `union sd_dis_specific` (`mds`): the discipline specific members, side by side.
pub struct SdDisSpecific {
    /// `mdd_raid0`.
    pub mdd_raid0: SrRaid0,
    /// `mdd_raid1`.
    pub mdd_raid1: SrRaid1,
    /// `mdd_raid5`.
    pub mdd_raid5: SrRaid5,
    /// `mdd_raid6`.
    pub mdd_raid6: SrRaid6,
    /// `mdd_concat`.
    pub mdd_concat: SrConcat,
    /// `mdd_crypto` (`CRYPTO`).
    pub mdd_crypto: SrCrypto,
    /// `mdd_raid1c` (`CRYPTO`).
    pub mdd_raid1c: SrRaid1c,
}

/// `struct sr_chunk`: one chunk (a partition) of a volume, or a hotspare.
pub struct SrChunk {
    /// `src_meta`: chunk meta data.
    pub src_meta: SrMetaChunk,
    /// `src_dev_mm`: major/minor.
    pub src_dev_mm: Cell<Dev>,
    /// `src_vn`: vnode.
    pub src_vn: Cell<Option<&'static Vnode>>,
    /// `src_meta_ondisk`: set when meta is on disk (helper members before metadata makes it
    /// onto the chunk).
    pub src_meta_ondisk: Cell<i32>,
    /// `src_devname`.
    pub src_devname: Cell<[u8; 32]>,
    /// `src_duid`: chunk disklabel UID.
    pub src_duid: Cell<[u8; 8]>,
    /// `src_size`: in blocks.
    pub src_size: Cell<i64>,
    /// `src_secsize`.
    pub src_secsize: Cell<u32>,
    /// `src_link`.
    pub src_link: SlistEntry<SrChunk>,
}

// SAFETY: cell views and cells of integers and `Option`s of references, and a list entry;
// no `Drop`.
unsafe impl SrZeroed for SrChunk {}

queue_adapter!(
    /// `SLIST_HEAD(sr_chunk_head, sr_chunk)`, through `src_link`.
    pub SrChunkLink: SrChunk, src_link => SlistEntry<SrChunk>
);

/// `struct sr_chunk_head`.
pub type SrChunkHead = SlistHead<SrChunkLink>;

/// `struct sr_volume`.
pub struct SrVolume {
    /// `sv_chunk_list`: linked list of all chunks.
    pub sv_chunk_list: SrChunkHead,
    /// `sv_chunks`: array to same chunks (`mallocarray`ed), see [`SrVolume::sv_chunk`].
    sv_chunks: Cell<Option<NonNull<Option<&'static SrChunk>>>>,
    /// The number of slots of `sv_chunks`.
    sv_nchunks: Cell<usize>,
    /// `sv_chunk_minsz`: size of smallest chunk.
    pub sv_chunk_minsz: Cell<i64>,
    /// `sv_chunk_maxsz`: size of largest chunk.
    pub sv_chunk_maxsz: Cell<i64>,
    /// `sv_sensor`.
    pub sv_sensor: Ksensor,
    /// `sv_sensor_attached`.
    pub sv_sensor_attached: Cell<i32>,
}

impl SrVolume {
    /// `sv_chunks = mallocarray(n, sizeof(struct sr_chunk *), M_WAITOK | M_ZERO)`: replaces
    /// the array by `n` empty slots (the old one, if any, is freed).
    pub fn sv_chunks_alloc(&self, n: usize, flags: i32) -> Result<(), Errno> {
        self.sv_chunks_free();
        let size = n.max(1) * size_of::<Option<&'static SrChunk>>();
        let p = malloc(size, M_DEVBUF, flags | M_ZERO).ok_or(Errno::ENOMEM)?;
        self.sv_chunks.set(Some(p.cast()));
        self.sv_nchunks.set(n);
        Ok(())
    }

    /// `free(sv_chunks)`.
    pub fn sv_chunks_free(&self) {
        if let Some(p) = self.sv_chunks.take() {
            let size = self.sv_nchunks.get().max(1) * size_of::<Option<&'static SrChunk>>();
            free(p.cast::<u8>(), M_DEVBUF, size);
        }
        self.sv_nchunks.set(0);
    }

    /// The number of slots of `sv_chunks`.
    pub fn sv_nchunks(&self) -> usize {
        self.sv_nchunks.get()
    }

    /// `sv_chunks[i]`, `None` for an empty slot or outside the array.
    pub fn sv_chunk_opt(&self, i: usize) -> Option<&'static SrChunk> {
        let p = self.sv_chunks.get()?;
        if i >= self.sv_nchunks.get() {
            return None;
        }
        // SAFETY: `sv_chunks` has `sv_nchunks` initialised slots (`sv_chunks_alloc`, zeroed:
        // `None`); `i` is in range.
        unsafe { p.as_ptr().add(i).read() }
    }

    /// `sv_chunks[i]`, which the C dereferences: panics on an empty slot or outside the
    /// array.
    pub fn sv_chunk(&self, i: usize) -> &'static SrChunk {
        match self.sv_chunk_opt(i) {
            Some(c) => c,
            None => panic(format_args!("sr_volume: no chunk {}", i)),
        }
    }

    /// `sv_chunks[i] = c`; panics outside the array.
    pub fn set_sv_chunk(&self, i: usize, c: Option<&'static SrChunk>) {
        let Some(p) = self.sv_chunks.get() else {
            panic(format_args!("sr_volume: no chunk array"));
        };
        if i >= self.sv_nchunks.get() {
            panic(format_args!("sr_volume: chunk {} out of range", i));
        }
        // SAFETY: as in `sv_chunk_opt`; the slot is written in place.
        unsafe { p.as_ptr().add(i).write(c) };
    }
}

/// `int (*sd_create)(struct sr_discipline *, struct bioc_createraid *, int, int64_t)`:
/// sets up a new volume's metadata (`no_chunk` chunks of `coerced_size` blocks).
pub type SdCreateFn = fn(
    sd: &'static SrDiscipline,
    bc: &mut BiocCreateraid,
    no_chunk: i32,
    coerced_size: i64,
) -> Result<(), Errno>;
/// `int (*sd_assemble)(struct sr_discipline *, struct bioc_createraid *, int, void *)`:
/// brings up an existing volume; `data` is the boot key (`sr_bootkey`) or NULL.
pub type SdAssembleFn = fn(
    sd: &'static SrDiscipline,
    bc: &mut BiocCreateraid,
    no_chunk: i32,
    data: Option<&[u8]>,
) -> Result<(), Errno>;
/// `int (*sd_alloc_resources)(struct sr_discipline *)`.
pub type SdAllocResourcesFn = fn(sd: &'static SrDiscipline) -> Result<(), Errno>;
/// `void (*sd_free_resources)(struct sr_discipline *)`.
pub type SdFreeResourcesFn = fn(sd: &'static SrDiscipline);
/// `int (*sd_ioctl_handler)(struct sr_discipline *, struct bioc_discipline *)`.
pub type SdIoctlHandlerFn =
    fn(sd: &'static SrDiscipline, bd: &mut BiocDiscipline) -> Result<(), Errno>;
/// `int (*sd_start_discipline)(struct sr_discipline *)`.
pub type SdStartDisciplineFn = fn(sd: &'static SrDiscipline) -> Result<(), Errno>;
/// `void (*sd_set_chunk_state)(struct sr_discipline *, int, int)`: chunk `c` goes to
/// `new_state` (`BIOC_SD*`).
pub type SdSetChunkStateFn = fn(sd: &'static SrDiscipline, c: usize, new_state: i32);
/// `void (*sd_set_vol_state)(struct sr_discipline *)`.
pub type SdSetVolStateFn = fn(sd: &'static SrDiscipline);
/// `int (*sd_openings)(struct sr_discipline *)`.
pub type SdOpeningsFn = fn(sd: &'static SrDiscipline) -> i32;
/// `int (*sd_meta_opt_handler)(struct sr_discipline *, struct sr_meta_opt_hdr *)`:
/// `Ok` when the discipline took the item, an error to leave it to the generic handler.
pub type SdMetaOptHandlerFn =
    fn(sd: &'static SrDiscipline, omi: &'static SrMetaOptItem) -> Result<(), Errno>;
/// `void (*sd_rebuild)(struct sr_discipline *)`.
pub type SdRebuildFn = fn(sd: &'static SrDiscipline);
/// `int (*sd_scsi_*)(struct sr_workunit *)`: a SCSI command of the volume; an error fails the
/// transfer (`goto stuffup`), with the discipline's sense data if it set any.
pub type SdScsiFn = fn(wu: &'static SrWorkunit) -> Result<(), Errno>;
/// `void (*sd_scsi_intr)(struct buf *)`: a ccb's `b_iodone`.
pub type SdScsiIntrFn = fn(bp: &'static Buf);
/// `int (*sd_scsi_wu_done)(struct sr_workunit *)`: the work unit's I/O is complete; returns
/// its new state (`SR_WU_RESTART` when it was restarted).
pub type SdScsiWuDoneFn = fn(wu: &'static SrWorkunit) -> i32;
/// `void (*sd_scsi_done)(struct sr_workunit *)`: completes the work unit (the discipline
/// then calls `sr_scsi_done`).
pub type SdScsiDoneFn = fn(wu: &'static SrWorkunit);

/// `struct sr_discipline`: one volume, its chunks and the discipline that runs it.
pub struct SrDiscipline {
    /// `sd_sc`: link back to sr softc.
    pub sd_sc: Cell<*const SrSoftc>,
    /// `sd_wu_size`: alloc and free size of a work unit; see [`SrDiscipline::set_wu_type`].
    sd_wu_size: Cell<usize>,
    /// The type of the work units when larger than [`SrWorkunit`] (`TypeId::of::<T>`).
    sd_wu_type: Cell<Option<fn() -> TypeId>>,
    /// `sd_type`: type of discipline (`SR_MD_*`).
    pub sd_type: Cell<u8>,
    /// `sd_name`: human readable discipline name.
    pub sd_name: Cell<[u8; 10]>,
    /// `sd_target`: scsibus target discipline uses.
    pub sd_target: Cell<u16>,
    /// `sd_capabilities`: `SR_CAP_*`.
    pub sd_capabilities: Cell<u32>,
    /// `sd_dis_specific` (`mds`): dis specific members.
    pub sd_dis_specific: SdDisSpecific,
    /// `sd_taskq`.
    pub sd_taskq: Cell<Option<&'static Taskq>>,
    /// `sd_meta`: in memory copy of metadata (`SR_META_SIZE * DEV_BSIZE` bytes).
    pub sd_meta: Cell<Option<NonNull<SrMetadata>>>,
    /// `sd_meta_foreign`: non native metadata.
    pub sd_meta_foreign: Cell<*mut c_void>,
    /// `sd_meta_flags`.
    pub sd_meta_flags: Cell<u32>,
    /// `sd_meta_type`: metadata functions (`SR_META_F_*`).
    pub sd_meta_type: Cell<i32>,
    /// `sd_meta_opt`: optional metadata.
    pub sd_meta_opt: SrMetaOptHead,
    /// `sd_sync`.
    pub sd_sync: Cell<i32>,
    /// `sd_must_flush`.
    pub sd_must_flush: Cell<i32>,
    /// `sd_deleted`.
    pub sd_deleted: Cell<i32>,
    /// `sd_vol`: volume associated.
    pub sd_vol: SrVolume,
    /// `sd_vol_status`: runtime vol status (`BIOC_SV*`).
    pub sd_vol_status: Cell<i32>,
    /// `sd_ccb`: the ccbs (`sd_max_wu * sd_max_ccb_per_wu`), see `sr_ccb_alloc`.
    pub(crate) sd_ccb: Cell<Option<NonNull<SrCcb>>>,
    /// The number of ccbs at `sd_ccb`.
    pub(crate) sd_nccb: Cell<usize>,
    /// `sd_ccb_freeq`.
    pub sd_ccb_freeq: SrCcbList,
    /// `sd_max_ccb_per_wu`.
    pub sd_max_ccb_per_wu: Cell<u32>,
    /// `sd_wu`: all workunits.
    pub sd_wu: SrWuAllList,
    /// `sd_max_wu`.
    pub sd_max_wu: Cell<u32>,
    /// `sd_reb_active`: rebuild in progress.
    pub sd_reb_active: Cell<i32>,
    /// `sd_reb_abort`: abort rebuild.
    pub sd_reb_abort: Cell<i32>,
    /// `sd_ready`: fully operational.
    pub sd_ready: Cell<i32>,
    /// `sd_wu_freeq`: free wu queue. Protected by: `sd_wu_mtx`.
    pub sd_wu_freeq: SrWuList,
    /// `sd_wu_pendq`: pending wu queue. Protected by: `splbio`.
    pub sd_wu_pendq: SrWuList,
    /// `sd_wu_defq`: deferred wu queue. Protected by: `splbio`.
    pub sd_wu_defq: SrWuList,
    /// `sd_wu_mtx`.
    pub sd_wu_mtx: Mutex,
    /// `sd_iopool`: the work units as the volume's SCSI openings.
    pub sd_iopool: ScsiIopool,
    /// `sd_wu_pending`: discipline stats. Protected by: `sd_wu_mtx`.
    pub sd_wu_pending: Cell<i32>,
    /// `sd_wu_collisions`.
    pub sd_wu_collisions: Cell<u64>,

    /// `sd_create`.
    pub sd_create: Cell<Option<SdCreateFn>>,
    /// `sd_assemble`.
    pub sd_assemble: Cell<Option<SdAssembleFn>>,
    /// `sd_alloc_resources`.
    pub sd_alloc_resources: Cell<Option<SdAllocResourcesFn>>,
    /// `sd_free_resources`.
    pub sd_free_resources: Cell<Option<SdFreeResourcesFn>>,
    /// `sd_ioctl_handler`.
    pub sd_ioctl_handler: Cell<Option<SdIoctlHandlerFn>>,
    /// `sd_start_discipline`.
    pub sd_start_discipline: Cell<Option<SdStartDisciplineFn>>,
    /// `sd_set_chunk_state`.
    pub sd_set_chunk_state: Cell<Option<SdSetChunkStateFn>>,
    /// `sd_set_vol_state`.
    pub sd_set_vol_state: Cell<Option<SdSetVolStateFn>>,
    /// `sd_openings`.
    pub sd_openings: Cell<Option<SdOpeningsFn>>,
    /// `sd_meta_opt_handler`.
    pub sd_meta_opt_handler: Cell<Option<SdMetaOptHandlerFn>>,
    /// `sd_rebuild`.
    pub sd_rebuild: Cell<Option<SdRebuildFn>>,

    /// `sd_scsi_sense`: SCSI emulation.
    pub sd_scsi_sense: Cell<ScsiSenseData>,
    /// `sd_scsi_rw`.
    pub sd_scsi_rw: Cell<Option<SdScsiFn>>,
    /// `sd_scsi_intr`.
    pub sd_scsi_intr: Cell<Option<SdScsiIntrFn>>,
    /// `sd_scsi_wu_done`.
    pub sd_scsi_wu_done: Cell<Option<SdScsiWuDoneFn>>,
    /// `sd_scsi_done`.
    pub sd_scsi_done: Cell<Option<SdScsiDoneFn>>,
    /// `sd_scsi_sync`.
    pub sd_scsi_sync: Cell<Option<SdScsiFn>>,
    /// `sd_scsi_tur`.
    pub sd_scsi_tur: Cell<Option<SdScsiFn>>,
    /// `sd_scsi_start_stop`.
    pub sd_scsi_start_stop: Cell<Option<SdScsiFn>>,
    /// `sd_scsi_inquiry`.
    pub sd_scsi_inquiry: Cell<Option<SdScsiFn>>,
    /// `sd_scsi_read_cap`.
    pub sd_scsi_read_cap: Cell<Option<SdScsiFn>>,
    /// `sd_scsi_req_sense`.
    pub sd_scsi_req_sense: Cell<Option<SdScsiFn>>,

    /// `sd_background_proc`: background operation.
    pub sd_background_proc: Cell<*const Proc>,

    /// `sd_meta_save_task`.
    pub sd_meta_save_task: Task,
    /// `sd_hotspare_rebuild_task`.
    pub sd_hotspare_rebuild_task: Task,

    /// `sd_link`: in the softc's `sc_dis_list`.
    pub sd_link: TailqEntry<SrDiscipline>,
}

// SAFETY: every member is a cell of an integer, a raw pointer, a byte array, an `Option` of
// a reference, `NonNull` or `fn`, a sensor or a SCSI wire structure (all valid as zero), a
// zeroed task, mutex or iopool, or a list head or entry; no `Drop`.
unsafe impl SrZeroed for SrDiscipline {}

/// Calls a hook, panicking (as the C's call through NULL would fault) when it is not set.
fn hook<F: Copy>(f: &Cell<Option<F>>, sd: &SrDiscipline, name: &str) -> F {
    match f.get() {
        Some(f) => f,
        None => panic(format_args!(
            "{}: {}: no discipline function",
            sd.sd_sc().sc_dev.xname(),
            name
        )),
    }
}

impl SrDiscipline {
    /// `sd->sd_sc`.
    pub fn sd_sc(&self) -> &'static SrSoftc {
        // SAFETY: `sd_sc` is set when the discipline is made (`sr_ioctl_createraid`, the fake
        // ones of the boot probe and the key disk code) to the softraid softc, which lives
        // while softraid is attached, longer than any discipline.
        match unsafe { self.sd_sc.get().as_ref() } {
            Some(sc) => sc,
            None => panic(format_args!("sr_discipline {:p} without softc", self)),
        }
    }

    /// `sd->sd_meta`, which the C dereferences: panics when it is NULL.
    pub fn sd_meta(&self) -> &SrMetadata {
        match self.sd_meta.get() {
            // SAFETY: `sd_meta` is a `SR_META_SIZE * DEV_BSIZE` allocation the discipline
            // owns until `sr_discipline_free`; any bytes are a valid view.
            Some(p) => unsafe { p.as_ref() },
            None => panic(format_args!("sr_discipline {:p} without metadata", self)),
        }
    }

    /// `sd->sd_meta` as the cells of its whole `SR_META_SIZE * DEV_BSIZE` allocation.
    pub fn sd_meta_cells(&self) -> &[Cell<u8>] {
        match self.sd_meta.get() {
            // SAFETY: as in `sd_meta`; the allocation is `SR_META_SIZE * DEV_BSIZE` bytes.
            Some(p) => unsafe {
                core::slice::from_raw_parts(p.as_ptr().cast::<Cell<u8>>(), SR_META_SIZE * DEV_BSIZE)
            },
            None => &[],
        }
    }

    /// `mds` (`sd_dis_specific`).
    pub fn mds(&self) -> &SdDisSpecific {
        &self.sd_dis_specific
    }

    /// `sd->sd_wu_size`.
    pub fn sd_wu_size(&self) -> usize {
        self.sd_wu_size.get()
    }

    /// `sd->sd_wu_size = sizeof(T)`: the discipline's work units are `T`s (`struct
    /// sr_crypto_wu`), which [`sr_wu_ext`] gets back from an [`SrWorkunit`]. Must be called
    /// before `sr_wu_alloc`.
    pub fn set_wu_type<T: SrWorkunitExt>(&self) {
        self.sd_wu_size.set(size_of::<T>());
        self.sd_wu_type.set(Some(TypeId::of::<T>));
    }

    /// `sd->sd_wu_size = sizeof(struct sr_workunit)` (`sr_discipline_init`).
    pub fn set_wu_type_default(&self) {
        self.sd_wu_size.set(size_of::<SrWorkunit>());
        self.sd_wu_type.set(None);
    }

    /// `sd->sd_create(sd, bc, no_chunk, coerced_size)`.
    pub fn sd_create(
        &'static self,
        bc: &mut BiocCreateraid,
        no_chunk: i32,
        coerced_size: i64,
    ) -> Result<(), Errno> {
        hook(&self.sd_create, self, "sd_create")(self, bc, no_chunk, coerced_size)
    }

    /// `sd->sd_assemble(sd, bc, no_chunk, data)`.
    pub fn sd_assemble(
        &'static self,
        bc: &mut BiocCreateraid,
        no_chunk: i32,
        data: Option<&[u8]>,
    ) -> Result<(), Errno> {
        hook(&self.sd_assemble, self, "sd_assemble")(self, bc, no_chunk, data)
    }

    /// `sd->sd_alloc_resources(sd)`.
    pub fn sd_alloc_resources(&'static self) -> Result<(), Errno> {
        hook(&self.sd_alloc_resources, self, "sd_alloc_resources")(self)
    }

    /// `sd->sd_free_resources(sd)`.
    pub fn sd_free_resources(&'static self) {
        hook(&self.sd_free_resources, self, "sd_free_resources")(self)
    }

    /// `sd->sd_ioctl_handler(sd, bd)`.
    pub fn sd_ioctl_handler(&'static self, bd: &mut BiocDiscipline) -> Result<(), Errno> {
        hook(&self.sd_ioctl_handler, self, "sd_ioctl_handler")(self, bd)
    }

    /// `sd->sd_start_discipline(sd)`.
    pub fn sd_start_discipline(&'static self) -> Result<(), Errno> {
        hook(&self.sd_start_discipline, self, "sd_start_discipline")(self)
    }

    /// `sd->sd_set_chunk_state(sd, c, new_state)`.
    pub fn sd_set_chunk_state(&'static self, c: usize, new_state: i32) {
        hook(&self.sd_set_chunk_state, self, "sd_set_chunk_state")(self, c, new_state)
    }

    /// `sd->sd_set_vol_state(sd)`.
    pub fn sd_set_vol_state(&'static self) {
        hook(&self.sd_set_vol_state, self, "sd_set_vol_state")(self)
    }

    /// `sd->sd_openings(sd)`.
    pub fn sd_openings(&'static self) -> i32 {
        hook(&self.sd_openings, self, "sd_openings")(self)
    }

    /// `sd->sd_meta_opt_handler(sd, om)`.
    pub fn sd_meta_opt_handler(&'static self, omi: &'static SrMetaOptItem) -> Result<(), Errno> {
        hook(&self.sd_meta_opt_handler, self, "sd_meta_opt_handler")(self, omi)
    }

    /// `sd->sd_rebuild(sd)`.
    pub fn sd_rebuild(&'static self) {
        hook(&self.sd_rebuild, self, "sd_rebuild")(self)
    }

    /// `sd->sd_scsi_rw(wu)`.
    pub fn sd_scsi_rw(&self, wu: &'static SrWorkunit) -> Result<(), Errno> {
        hook(&self.sd_scsi_rw, self, "sd_scsi_rw")(wu)
    }

    /// `sd->sd_scsi_wu_done(wu)`.
    pub fn sd_scsi_wu_done(&self, wu: &'static SrWorkunit) -> i32 {
        hook(&self.sd_scsi_wu_done, self, "sd_scsi_wu_done")(wu)
    }

    /// `sd->sd_scsi_done(wu)`.
    pub fn sd_scsi_done(&self, wu: &'static SrWorkunit) {
        hook(&self.sd_scsi_done, self, "sd_scsi_done")(wu)
    }

    /// `sd->sd_scsi_sync(wu)`.
    pub fn sd_scsi_sync(&self, wu: &'static SrWorkunit) -> Result<(), Errno> {
        hook(&self.sd_scsi_sync, self, "sd_scsi_sync")(wu)
    }

    /// `sd->sd_scsi_tur(wu)`.
    pub fn sd_scsi_tur(&self, wu: &'static SrWorkunit) -> Result<(), Errno> {
        hook(&self.sd_scsi_tur, self, "sd_scsi_tur")(wu)
    }

    /// `sd->sd_scsi_start_stop(wu)`.
    pub fn sd_scsi_start_stop(&self, wu: &'static SrWorkunit) -> Result<(), Errno> {
        hook(&self.sd_scsi_start_stop, self, "sd_scsi_start_stop")(wu)
    }

    /// `sd->sd_scsi_inquiry(wu)`.
    pub fn sd_scsi_inquiry(&self, wu: &'static SrWorkunit) -> Result<(), Errno> {
        hook(&self.sd_scsi_inquiry, self, "sd_scsi_inquiry")(wu)
    }

    /// `sd->sd_scsi_read_cap(wu)`.
    pub fn sd_scsi_read_cap(&self, wu: &'static SrWorkunit) -> Result<(), Errno> {
        hook(&self.sd_scsi_read_cap, self, "sd_scsi_read_cap")(wu)
    }

    /// `sd->sd_scsi_req_sense(wu)`.
    pub fn sd_scsi_req_sense(&self, wu: &'static SrWorkunit) -> Result<(), Errno> {
        hook(&self.sd_scsi_req_sense, self, "sd_scsi_req_sense")(wu)
    }

    /// The discipline name (`sd_name`) up to its NUL.
    pub fn name(&self) -> Name<10> {
        Name(self.sd_name.get())
    }
}

queue_adapter!(
    /// `TAILQ_HEAD(sr_discipline_list, sr_discipline)`, through `sd_link`.
    pub SrDisciplineLink: SrDiscipline, sd_link => TailqEntry<SrDiscipline>
);

/// `struct sr_discipline_list`.
pub type SrDisciplineList = TailqHead<SrDisciplineLink>;

/// A fixed-size NUL-padded name (`sd_name`, `ssd_devname`, ...) printed as the C's `%s`.
#[derive(Clone, Copy)]
pub struct Name<const N: usize>(pub [u8; N]);

impl<const N: usize> Name<N> {
    /// The bytes up to the first NUL.
    pub fn bytes(&self) -> &[u8] {
        let len = self.0.iter().position(|&b| b == 0).unwrap_or(N);
        &self.0[..len]
    }
}

impl<const N: usize> core::fmt::Display for Name<N> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        for &b in self.bytes() {
            core::fmt::Write::write_char(f, char::from(b))?;
        }
        Ok(())
    }
}

/// `struct sr_softc`: softraid0.
#[repr(C)]
pub struct SrSoftc {
    /// `sc_dev`.
    pub sc_dev: Device,
    /// `sc_lock`.
    pub sc_lock: Rwlock,
    /// `sc_status`: status and messages. Protected by: `sc_lock` (write).
    pub sc_status: UnsafeCell<BioStatus>,
    /// `sc_hotspare_list`: list of hotspares.
    pub sc_hotspare_list: SrChunkHead,
    /// `sc_hs_lock`: lock for hotspares list.
    pub sc_hs_lock: Rwlock,
    /// `sc_hotspare_no`: number of hotspares.
    pub sc_hotspare_no: Cell<i32>,
    /// `sc_sensordev`.
    pub sc_sensordev: Ksensordev,
    /// `sc_sensor_task`.
    pub sc_sensor_task: Cell<Option<NonNull<SensorTask>>>,
    /// `sc_scsibus`: the bus `sr_attach` found, or NULL.
    pub sc_scsibus: Cell<Option<NonNull<ScsibusSoftc>>>,
    /// `sc_targets`: the target lookup has to be cheap since it happens for each I/O.
    pub sc_targets: [Cell<Option<&'static SrDiscipline>>; SR_MAX_LD],
    /// `sc_dis_list`.
    pub sc_dis_list: SrDisciplineList,
}

// SAFETY: `#[repr(C)]` with the device first; the rest are locks, a `BioStatus` of integers
// and byte arrays, list heads, a sensor device, cells of integers and of `Option`s of
// `NonNull` and references: all valid as zero bytes.
unsafe impl Softc for SrSoftc {}

impl SrSoftc {
    /// `sc->sc_scsibus`.
    pub fn sc_scsibus(&self) -> Option<&'static ScsibusSoftc> {
        // SAFETY: the bus device `config_found` attached under softraid; it lives until
        // `sr_detach` detaches it and clears the member.
        self.sc_scsibus.get().map(|p| unsafe { p.as_ref() })
    }
}

/// `void (*sh_hotplug)(struct sr_discipline *, struct disk *, int)`: a discipline's hotplug
/// callback (`sr_hotplug_register`).
pub type SrHotplugFn = fn(sd: &'static SrDiscipline, diskp: &Disk, action: i32);

const _: () = {
    use core::mem::{align_of, offset_of};
    assert!(size_of::<SrUuid>() == 16);
    assert!(size_of::<SrMetaInvariant>() == 96);
    assert!(align_of::<SrMetaInvariant>() == 1);
    assert!(offset_of!(SrMetaInvariant, ssd_uuid) == 16);
    assert!(offset_of!(SrMetaInvariant, ssd_size) == 56);
    assert!(offset_of!(SrMetaInvariant, ssd_strip_size) == 92);
    assert!(size_of::<SrMetadata>() == 168);
    assert!(offset_of!(SrMetadata, ssd_checksum) == 96);
    assert!(offset_of!(SrMetadata, ssd_devname) == 112);
    assert!(offset_of!(SrMetadata, ssd_ondisk) == 152);
    assert!(size_of::<SrMetaChunkInvariant>() == 72);
    assert!(size_of::<SrMetaChunk>() == 92);
    assert!(offset_of!(SrMetaChunk, scm_status) == 88);
    assert!(size_of::<SrMetaOptHdr>() == 24);
    assert!(size_of::<SrMetaCrypto>() == 2480);
    assert!(offset_of!(SrMetaCrypto, scm_key) == 104);
    assert!(offset_of!(SrMetaCrypto, scm_check_alg) == 2408);
    assert!(size_of::<SrMetaBoot>() == 168);
    assert!(size_of::<SrMetaKeydisk>() == 56);
    assert!(size_of::<SrCryptoChkHmacSha1>() == 20);
    assert!(size_of::<SrCryptoGenkdf>() == 8);
    assert!(size_of::<SrCryptoPbkdf>() == 140);
    assert!(size_of::<SrCryptoKdfinfo>() == 180);
    assert!(size_of::<SrCryptoKdfpair>() == 32);
    assert!(SR_OLD_META_OPT_SIZE == size_of::<SrMetaCrypto>());
    assert!(
        size_of::<SrMetadata>() + SR_MAX_LD * size_of::<SrMetaChunk>() <= SR_META_SIZE * DEV_BSIZE
    );
};

#[cfg(test)]
mod tests;
