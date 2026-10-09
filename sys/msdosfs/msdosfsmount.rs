/* <LICENSES> */
/*-
 * Copyright (C) 1994, 1995, 1997 Wolfgang Solfrank.
 * Copyright (C) 1994, 1995, 1997 TooLs GmbH.
 * All rights reserved.
 * Original code by Paul Popelka (paulp@uts.amdahl.com) (see below).
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. All advertising materials mentioning features or use of this software
 *    must display the following acknowledgement:
 *	This product includes software developed by TooLs GmbH.
 * 4. The name of TooLs GmbH may not be used to endorse or promote products
 *    derived from this software without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY TOOLS GMBH ``AS IS'' AND ANY EXPRESS OR
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES
 * OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED.
 * IN NO EVENT SHALL TOOLS GMBH BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
 * SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO,
 * PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS;
 * OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
 * WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR
 * OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF
 * ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */
/*
 * Written by Paul Popelka (paulp@uts.amdahl.com)
 *
 * You can do anything you want with this software, just don't say you wrote
 * it, and don't remove this notice.
 *
 * This software is provided "as is".
 *
 * The author supplies this software to be publicly redistributed on the
 * understanding that the author is not responsible for the correct
 * functioning of this software in any circumstances and is not liable for
 * any damages caused by this software.
 *
 * October 1992
 */
/* </LICENSES> */

/* <CODE> */
//! `<msdosfs/msdosfsmount.h>`: the mount control block of a msdos file system (`struct
//! msdosfsmount`), hung from the mount's `mnt_data`, the `MSDOSFSMNT_*` flags and the macros
//! that convert between byte offsets, blocks and clusters.
//!
//! Upstream: sys/msdosfs/msdosfsmount.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - The structure is `malloc(M_MSDOSFSMNT)`ed by `msdosfs_mountfs` and freed by
//!   `msdosfs_unmount`; it is reached as `&'static Msdosfsmount` through [`vfstomsdosfs`] (the
//!   C's `VFSTOMSDOSFS` cast), whose one `unsafe` checks that the mount is a msdos one. The
//!   members are `Cell`s: the C changes them through shared pointers under the kernel lock.
//! - `pm_bpb` is a `Cell<Bpb50>`; the `pm_BytesPerSec`, ... shorthands are methods with the
//!   macros' names, and a `set_` method for each (the C assigns through the macros).
//! - `pm_inusemap` stays the `malloc`ed pointer (`Cell<*mut u32>`); [`Msdosfsmount::inusemap`]
//!   is the bitmap as `howmany(pm_maxcluster + 1, N_INUSEBITS)` words, the size
//!   `msdosfs_mountfs` allocates it with.
//! - `pm_export` (`struct netexport`) is kept whether or not `nfsserver` is configured, as the C
//!   does, as `um_export` is in `ufsmount.rs`.
//! - The flags are `u32`, the type of `pm_flags`; `MSDOSFSMNT_SHORTNAME`, `_LONGNAME` and
//!   `_NOWIN95` are the `<sys/mount.h>` ones (`sys/sys/mount.rs`), as the C's `#if 0` says.
//! - The arithmetic macros (`de_cluster`, `cntobn`, ...) are generic functions over the
//!   [`DeArith`] integers, so each computes in the type of its argument as the macro does
//!   (an `off_t` offset stays 64-bit, a cluster number wraps at 32 bits); the macros' unsigned
//!   additions and subtractions wrap. `bptoep` returns the entry's offset in the buffer (the
//!   caller views it with `Direntry::at`). `FATOFS` computes in `u32`, the type of `cn` and of
//!   the multipliers.
//! - The prototypes of the virtual filesystem operations (`msdosfs_mount`, ...) are the
//!   functions of `msdosfs_vfsops.rs`; `msdosfs_init` is in `msdosfs_denode.rs`, where the C
//!   defines it.

use core::cell::Cell;
use core::ops::{Shl, Shr};
use core::ptr;

use crate::kern::subr_prf::panic;
use crate::msdosfs::bpb::Bpb50;
use crate::msdosfs::fat::{CLUST_FIRST, MSDOSFSROOT};
use crate::sys::mount::{
    MOUNT_MSDOS, MSDOSFSMNT_LONGNAME, MSDOSFSMNT_NOWIN95, MSDOSFSMNT_SHORTNAME, Mount, Netexport,
};
use crate::sys::types::{Dev, Gid, Mode, Uid};
use crate::sys::vnode::Vnode;

/// `MSDOSFSMNT_MNTOPT`: all the `<sys/mount.h>` flags.
pub const MSDOSFSMNT_MNTOPT: u32 =
    (MSDOSFSMNT_SHORTNAME | MSDOSFSMNT_LONGNAME | MSDOSFSMNT_NOWIN95) as u32;
/// `MSDOSFSMNT_RONLY`: mounted read-only.
pub const MSDOSFSMNT_RONLY: u32 = 0x8000_0000;
/// `MSDOSFSMNT_WAITONFAT`: mounted synchronous.
pub const MSDOSFSMNT_WAITONFAT: u32 = 0x4000_0000;
/// `MSDOSFS_FATMIRROR`: FAT is mirrored.
pub const MSDOSFS_FATMIRROR: u32 = 0x2000_0000;

/// `N_INUSEBITS`: number of bits in one `pm_inusemap` item.
pub const N_INUSEBITS: u32 = 8 * size_of::<u32>() as u32;

/// Generates a `pm_bpb` shorthand: `pmp->pm_x` read and assigned.
macro_rules! bpb_shorthand {
    ($(#[$doc:meta])* $get:ident, $set:ident, $field:ident, $t:ty) => {
        $(#[$doc])*
        #[allow(non_snake_case)] // the C macro's name
        pub fn $get(&self) -> $t {
            self.pm_bpb.get().$field
        }

        $(#[$doc])*
        #[allow(non_snake_case)] // the C macro's name
        pub fn $set(&self, v: $t) {
            let mut b = self.pm_bpb.get();
            b.$field = v;
            self.pm_bpb.set(b);
        }
    };
}

/// `struct msdosfsmount`: layout of the mount control block for a msdos file system.
#[allow(non_snake_case)] // the C field names
pub struct Msdosfsmount {
    /// `pm_mountp`: vfs mount struct for this fs.
    pub pm_mountp: Cell<Option<&'static Mount>>,
    /// `pm_dev`: block special device mounted.
    pub pm_dev: Cell<Dev>,
    /// `pm_uid`: uid to set as owner of the files.
    pub pm_uid: Cell<Uid>,
    /// `pm_gid`: gid to set as owner of the files.
    pub pm_gid: Cell<Gid>,
    /// `pm_mask`: mask to and with file protection bits.
    pub pm_mask: Cell<Mode>,
    /// `pm_devvp`: vnode for block device mntd.
    pub pm_devvp: Cell<Option<&'static Vnode>>,
    /// `pm_bpb`: BIOS parameter blk for this fs.
    pub pm_bpb: Cell<Bpb50>,
    /// `pm_BlkPerSec`: # of `DEV_BSIZE` blocks in MSDOSFS sector.
    pub pm_BlkPerSec: Cell<u32>,
    /// `pm_FATsecs`: actual number of fat sectors.
    pub pm_FATsecs: Cell<u32>,
    /// `pm_fatblk`: block # of first FAT.
    pub pm_fatblk: Cell<u32>,
    /// `pm_rootdirblk`: block # (cluster # for FAT32) of root directory number.
    pub pm_rootdirblk: Cell<u32>,
    /// `pm_rootdirsize`: size in blocks (not clusters).
    pub pm_rootdirsize: Cell<u32>,
    /// `pm_firstcluster`: block number of first cluster.
    pub pm_firstcluster: Cell<u32>,
    /// `pm_nmbrofclusters`: # of clusters in filesystem.
    pub pm_nmbrofclusters: Cell<u32>,
    /// `pm_maxcluster`: maximum cluster number.
    pub pm_maxcluster: Cell<u32>,
    /// `pm_freeclustercount`: number of free clusters.
    pub pm_freeclustercount: Cell<u32>,
    /// `pm_cnshift`: shift file offset right this amount to get a cluster number.
    pub pm_cnshift: Cell<u32>,
    /// `pm_crbomask`: and a file offset with this mask to get cluster rel offset.
    pub pm_crbomask: Cell<u32>,
    /// `pm_bnshift`: shift file offset right this amount to get a block number.
    pub pm_bnshift: Cell<u32>,
    /// `pm_bpcluster`: bytes per cluster.
    pub pm_bpcluster: Cell<u32>,
    /// `pm_fmod`: ~0 if fs is modified, this can rollover to 0.
    pub pm_fmod: Cell<u32>,
    /// `pm_fatblocksize`: size of fat blocks in bytes.
    pub pm_fatblocksize: Cell<u32>,
    /// `pm_fatblocksec`: size of fat blocks in sectors.
    pub pm_fatblocksec: Cell<u32>,
    /// `pm_fatsize`: size of fat in bytes.
    pub pm_fatsize: Cell<u32>,
    /// `pm_fatmask`: mask to use for fat numbers.
    pub pm_fatmask: Cell<u32>,
    /// `pm_fsinfo`: fsinfo block number.
    pub pm_fsinfo: Cell<u32>,
    /// `pm_fatmult`: these 2 values are used in fat offset computation.
    pub pm_fatmult: Cell<u32>,
    /// `pm_fatdiv`: (see `pm_fatmult`).
    pub pm_fatdiv: Cell<u32>,
    /// `pm_curfat`: current fat for FAT32 (0 otherwise).
    pub pm_curfat: Cell<u32>,
    /// `pm_inusemap`: ptr to bitmap of in-use clusters (see [`Msdosfsmount::inusemap`]).
    pub pm_inusemap: Cell<*mut u32>,
    /// `pm_flags`: the `MSDOSFSMNT_*` flags below.
    pub pm_flags: Cell<u32>,
    /// `pm_export`: export information.
    pub pm_export: Netexport,
}

// SAFETY: the members are changed under the kernel lock, as in C.
unsafe impl Sync for Msdosfsmount {}

impl Msdosfsmount {
    /// A zeroed structure, as `malloc(M_ZERO)` returns it.
    pub const fn new() -> Self {
        Self {
            pm_mountp: Cell::new(None),
            pm_dev: Cell::new(0),
            pm_uid: Cell::new(0),
            pm_gid: Cell::new(0),
            pm_mask: Cell::new(0),
            pm_devvp: Cell::new(None),
            pm_bpb: Cell::new(Bpb50::new()),
            pm_BlkPerSec: Cell::new(0),
            pm_FATsecs: Cell::new(0),
            pm_fatblk: Cell::new(0),
            pm_rootdirblk: Cell::new(0),
            pm_rootdirsize: Cell::new(0),
            pm_firstcluster: Cell::new(0),
            pm_nmbrofclusters: Cell::new(0),
            pm_maxcluster: Cell::new(0),
            pm_freeclustercount: Cell::new(0),
            pm_cnshift: Cell::new(0),
            pm_crbomask: Cell::new(0),
            pm_bnshift: Cell::new(0),
            pm_bpcluster: Cell::new(0),
            pm_fmod: Cell::new(0),
            pm_fatblocksize: Cell::new(0),
            pm_fatblocksec: Cell::new(0),
            pm_fatsize: Cell::new(0),
            pm_fatmask: Cell::new(0),
            pm_fsinfo: Cell::new(0),
            pm_fatmult: Cell::new(0),
            pm_fatdiv: Cell::new(0),
            pm_curfat: Cell::new(0),
            pm_inusemap: Cell::new(ptr::null_mut()),
            pm_flags: Cell::new(0),
            pm_export: Netexport::new(),
        }
    }

    /// `pmp->pm_mountp`, which every mounted msdos file system has.
    pub fn mountp(&self) -> &'static Mount {
        match self.pm_mountp.get() {
            Some(mp) => mp,
            None => panic(format_args!("msdosfsmount {:p}: no pm_mountp", self)),
        }
    }

    /// `pmp->pm_devvp`, which every mounted msdos file system has.
    pub fn devvp(&self) -> &'static Vnode {
        match self.pm_devvp.get() {
            Some(vp) => vp,
            None => panic(format_args!("msdosfsmount {:p}: no pm_devvp", self)),
        }
    }

    /// `pmp->pm_inusemap`: the bitmap of in-use clusters, `howmany(pm_maxcluster + 1,
    /// N_INUSEBITS)` words.
    pub fn inusemap(&self) -> &[Cell<u32>] {
        let p = self.pm_inusemap.get();
        if p.is_null() {
            panic(format_args!("msdosfsmount {:p}: no pm_inusemap", self));
        }
        let n = (self.pm_maxcluster.get() as usize + 1).div_ceil(N_INUSEBITS as usize);
        // SAFETY: `msdosfs_mountfs` points `pm_inusemap` at a `mallocarray` of exactly `n`
        // words, after it has set `pm_maxcluster` for good, and frees it only in
        // `msdosfs_unmount`, when nothing reaches the mount any more; `Cell<u32>` has the
        // layout of `u32`, and the words are only read and written through these `Cell`s,
        // under the kernel lock.
        unsafe { core::slice::from_raw_parts(p.cast::<Cell<u32>>(), n) }
    }

    bpb_shorthand!(
        /// `pm_BytesPerSec` (`pm_bpb.bpbBytesPerSec`).
        pm_BytesPerSec, set_pm_BytesPerSec, bpbBytesPerSec, u16
    );
    bpb_shorthand!(
        /// `pm_ResSectors` (`pm_bpb.bpbResSectors`).
        pm_ResSectors, set_pm_ResSectors, bpbResSectors, u16
    );
    bpb_shorthand!(
        /// `pm_FATs` (`pm_bpb.bpbFATs`).
        pm_FATs, set_pm_FATs, bpbFATs, u8
    );
    bpb_shorthand!(
        /// `pm_RootDirEnts` (`pm_bpb.bpbRootDirEnts`).
        pm_RootDirEnts, set_pm_RootDirEnts, bpbRootDirEnts, u16
    );
    bpb_shorthand!(
        /// `pm_Sectors` (`pm_bpb.bpbSectors`).
        pm_Sectors, set_pm_Sectors, bpbSectors, u16
    );
    bpb_shorthand!(
        /// `pm_Media` (`pm_bpb.bpbMedia`).
        pm_Media, set_pm_Media, bpbMedia, u8
    );
    bpb_shorthand!(
        /// `pm_SecPerTrack` (`pm_bpb.bpbSecPerTrack`).
        pm_SecPerTrack, set_pm_SecPerTrack, bpbSecPerTrack, u16
    );
    bpb_shorthand!(
        /// `pm_Heads` (`pm_bpb.bpbHeads`).
        pm_Heads, set_pm_Heads, bpbHeads, u16
    );
    bpb_shorthand!(
        /// `pm_HiddenSects` (`pm_bpb.bpbHiddenSecs`).
        pm_HiddenSects, set_pm_HiddenSects, bpbHiddenSecs, u32
    );
    bpb_shorthand!(
        /// `pm_HugeSectors` (`pm_bpb.bpbHugeSectors`).
        pm_HugeSectors, set_pm_HugeSectors, bpbHugeSectors, u32
    );
}

impl Default for Msdosfsmount {
    fn default() -> Self {
        Self::new()
    }
}

/// The integer types the offset and cluster macros are applied to (`uint32_t` cluster and
/// block numbers, `daddr_t` and `off_t` offsets): each macro computes in its argument's type,
/// with C's unsigned wrapping.
pub trait DeArith: Copy + Shl<u32, Output = Self> + Shr<u32, Output = Self> {
    /// The `uint32_t` value `v` in this type (the C's usual arithmetic conversion).
    fn from_u32(v: u32) -> Self;
    /// `self + o`, wrapping.
    fn wadd(self, o: Self) -> Self;
    /// `self - o`, wrapping.
    fn wsub(self, o: Self) -> Self;
}

/// `FATOFS(pmp, cn)`: byte offset in FAT on filesystem `pmp`, cluster `cn`.
pub fn fatofs(pmp: &Msdosfsmount, cn: u32) -> u32 {
    cn.wrapping_mul(pmp.pm_fatmult.get()) / pmp.pm_fatdiv.get()
}

/// `VFSTOMSDOSFS(mp)`: the msdos mount structure of `mp`.
pub fn vfstomsdosfs(mp: &Mount) -> &'static Msdosfsmount {
    let data = mp.mnt_data.get();
    let msdos = mp.mnt_vfc.get().map(|vfc| vfc.name()) == Some(MOUNT_MSDOS);
    if data.is_null() || !msdos {
        panic(format_args!(
            "VFSTOMSDOSFS: mount {:p} is not a mounted msdos file system",
            mp
        ));
    }
    // SAFETY: a msdos mount's `mnt_data` (checked above) is the `struct msdosfsmount` that
    // `msdosfs_mountfs` allocated, which lives until `msdosfs_unmount` frees it and clears
    // `mnt_data`.
    unsafe { &*data.cast::<Msdosfsmount>() }
}

/// Implements [`DeArith`] for integer types.
macro_rules! de_arith {
    ($($t:ty),*) => {
        $(impl DeArith for $t {
            fn from_u32(v: u32) -> Self {
                v as $t
            }
            fn wadd(self, o: Self) -> Self {
                self.wrapping_add(o)
            }
            fn wsub(self, o: Self) -> Self {
                self.wrapping_sub(o)
            }
        })*
    };
}

de_arith!(u32, u64, i64);

/// `bptoep(pmp, bp, dirofs)`: convert pointer to buffer -> pointer to direntry; here the
/// offset of the entry in the buffer's data.
pub fn bptoep(pmp: &Msdosfsmount, dirofs: u32) -> usize {
    (dirofs & pmp.pm_crbomask.get()) as usize
}

/// `de_bn2cn(pmp, bn)`: convert block number to cluster number.
pub fn de_bn2cn<T: DeArith>(pmp: &Msdosfsmount, bn: T) -> T {
    bn >> (pmp.pm_cnshift.get() - pmp.pm_bnshift.get())
}

/// `de_cn2bn(pmp, cn)`: convert cluster number to block number.
pub fn de_cn2bn<T: DeArith>(pmp: &Msdosfsmount, cn: T) -> T {
    cn << (pmp.pm_cnshift.get() - pmp.pm_bnshift.get())
}

/// `de_cluster(pmp, off)`: convert file offset to cluster number.
pub fn de_cluster<T: DeArith>(pmp: &Msdosfsmount, off: T) -> T {
    off >> pmp.pm_cnshift.get()
}

/// `de_clcount(pmp, size)`: clusters required to hold size bytes.
pub fn de_clcount<T: DeArith>(pmp: &Msdosfsmount, size: T) -> T {
    size.wadd(T::from_u32(pmp.pm_bpcluster.get()))
        .wsub(T::from_u32(1))
        >> pmp.pm_cnshift.get()
}

/// `de_blk(pmp, off)`: convert file offset to block number.
pub fn de_blk<T: DeArith>(pmp: &Msdosfsmount, off: T) -> T {
    de_cn2bn(pmp, de_cluster(pmp, off))
}

/// `de_cn2off(pmp, cn)`: convert cluster number to file offset.
pub fn de_cn2off<T: DeArith>(pmp: &Msdosfsmount, cn: T) -> T {
    cn << pmp.pm_cnshift.get()
}

/// `de_bn2off(pmp, bn)`: convert block number to file offset.
pub fn de_bn2off<T: DeArith>(pmp: &Msdosfsmount, bn: T) -> T {
    bn << pmp.pm_bnshift.get()
}

/// `cntobn(pmp, cn)`: map a cluster number into a filesystem relative block number.
pub fn cntobn<T: DeArith>(pmp: &Msdosfsmount, cn: T) -> T {
    de_cn2bn(pmp, cn.wsub(T::from_u32(CLUST_FIRST))).wadd(T::from_u32(pmp.pm_firstcluster.get()))
}

/// `roottobn(pmp, dirofs)`: calculate block number for directory entry in root dir, offset
/// `dirofs`.
pub fn roottobn<T: DeArith>(pmp: &Msdosfsmount, dirofs: T) -> T {
    de_blk(pmp, dirofs).wadd(T::from_u32(pmp.pm_rootdirblk.get()))
}

/// `detobn(pmp, dirclu, dirofs)`: calculate block number for directory entry at cluster
/// `dirclu`, offset `dirofs`.
pub fn detobn(pmp: &Msdosfsmount, dirclu: u32, dirofs: u32) -> u32 {
    if dirclu == MSDOSFSROOT {
        roottobn(pmp, dirofs)
    } else {
        cntobn(pmp, dirclu)
    }
}

/// `fsi_size(pmp)`: calculate size of fsinfo block.
pub fn fsi_size(pmp: &Msdosfsmount) -> i32 {
    1024 << (pmp.pm_BlkPerSec.get() >> 2)
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    /// A FAT16 geometry: 512-byte sectors, 4 sectors per cluster, root directory at block 9
    /// for 32 blocks, first cluster at block 41.
    fn fat16() -> Msdosfsmount {
        let pmp = Msdosfsmount::new();
        pmp.set_pm_BytesPerSec(512);
        pmp.pm_bnshift.set(9);
        pmp.pm_bpcluster.set(2048);
        pmp.pm_cnshift.set(11);
        pmp.pm_crbomask.set(2047);
        pmp.pm_rootdirblk.set(9);
        pmp.pm_rootdirsize.set(32);
        pmp.pm_firstcluster.set(41);
        pmp.pm_fatmult.set(2);
        pmp.pm_fatdiv.set(1);
        pmp
    }

    #[test]
    fn conversions() {
        let pmp = fat16();
        assert_eq!(pmp.pm_BytesPerSec(), 512);
        assert_eq!(de_cluster(&pmp, 4095u32), 1);
        assert_eq!(de_cluster(&pmp, 0x1_0000_0000i64), 0x20_0000);
        assert_eq!(de_clcount(&pmp, 0u32), 0);
        assert_eq!(de_clcount(&pmp, 1u32), 1);
        assert_eq!(de_clcount(&pmp, 2049u32), 2);
        assert_eq!(de_cn2bn(&pmp, 3u32), 12);
        assert_eq!(de_bn2cn(&pmp, 12u32), 3);
        assert_eq!(de_blk(&pmp, 5000u32), 8);
        assert_eq!(de_cn2off(&pmp, 3u32), 6144);
        assert_eq!(de_bn2off(&pmp, 3u32), 1536);
        assert_eq!(cntobn(&pmp, 2u32), 41);
        assert_eq!(cntobn(&pmp, 5i64), 53);
        assert_eq!(roottobn(&pmp, 64u32), 9);
        assert_eq!(detobn(&pmp, MSDOSFSROOT, 2048), 13);
        assert_eq!(detobn(&pmp, 3, 2048), 45);
        assert_eq!(bptoep(&pmp, 2048 + 96), 96);
        assert_eq!(fatofs(&pmp, 7), 14);
        assert_eq!(N_INUSEBITS, 32);
        pmp.pm_BlkPerSec.set(8);
        assert_eq!(fsi_size(&pmp), 4096);
    }
}
/* </TESTS> */
