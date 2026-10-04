/*	$OpenBSD: ufsmount.h,v 1.13 2016/02/27 18:50:38 natano Exp $	*/
/*	$NetBSD: ufsmount.h,v 1.4 1994/12/21 20:00:23 mycroft Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1982, 1986, 1989, 1993
 *	The Regents of the University of California.  All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. Neither the name of the University nor the names of its contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE REGENTS AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE REGENTS OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 *	@(#)ufsmount.h	8.4 (Berkeley) 10/27/94
 */
/* </LICENSES> */

//! `<ufs/ufs/ufsmount.h>`: the UFS specific mount structure data (`struct ufsmount`), hung
//! from a UFS mount's `mnt_data`.
//!
//! Upstream: sys/ufs/ufs/ufsmount.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - The structure is `malloc(M_UFSMNT)`ed by `ffs_mountfs` and freed by `ffs_unmount`; it is
//!   reached as `&'static Ufsmount` through [`vfstoufs`] (the C's `VFSTOUFS` cast), whose one
//!   `unsafe` checks that the mount is a UFS one. The members are `Cell`s.
//! - `ufsmount_u` (the super-block pointer of FFS or of EXT2FS) is `um_fs`; with feature
//!   `ext2fs` (`option EXT2FS`) the union's other member is the separate field `um_e2fs`
//!   (`Cell<Option<&'static MExt2fs>>`, [`Ufsmount::e2fs`]): a mount is FFS's or ext2fs's,
//!   never both. The C's `um_e2fsb` (`um_e2fs->s_es`) names a member that does not exist
//!   (nothing uses it); [`Ufsmount::with_e2fsb`] is its meaning, the in-core super block
//!   `um_e2fs->e2fs` for the duration of a closure.
//! - `um_export` (`struct netexport`) is kept whether or not `nfsserver` is configured, as the
//!   C does; without the feature `vfs_export` answers `ENOTSUP` and the list stays empty.
//! - The quota members (`um_quotas`, `um_cred`, `um_btime`, `um_itime`, `um_qflags`) exist
//!   with or without feature `quota`, as the C has no `#ifdef QUOTA` here; without it they stay
//!   NULL and zero. `um_cred` is NULL or `NOCRED` while no quota file is open (`ufs_quota.rs`).
//! - `MNINDIR`, `blkptrtodb` and `is_sequential` are functions with the macros' names in lower
//!   case.

use core::cell::Cell;
use core::ptr;

use crate::kern::subr_prf::panic;
use crate::sys::mount::{Mount, Netexport};
use crate::sys::types::{Daddr, Dev, Time};
use crate::sys::ucred::Ucred;
use crate::sys::vnode::Vnode;
#[cfg(feature = "ext2fs")]
use crate::ufs::ext2fs::ext2fs::{Ext2fs, MExt2fs};
use crate::ufs::ffs::fs::Fs;
use crate::ufs::ufs::quota::MAXQUOTAS;

/// `struct ufsmount`: the UFS specific mount structure data.
pub struct Ufsmount {
    /// `um_mountp`: filesystem vfs structure.
    pub um_mountp: Cell<Option<&'static Mount>>,
    /// `um_dev`: device mounted.
    pub um_dev: Cell<Dev>,
    /// `um_devvp`: block device mounted vnode.
    pub um_devvp: Cell<Option<&'static Vnode>>,
    /// `um_fstype`: type of file system (`UM_UFS1`, `UM_UFS2`).
    pub um_fstype: Cell<u64>,
    /// `um_fs`: pointer to superblock (FFS).
    pub um_fs: Cell<Option<&'static Fs>>,
    /// `um_e2fs` (`ufsmount_u.e2fs`): pointer to the in-core super block (EXT2FS), in place
    /// of `um_fs` for an ext2fs mount.
    #[cfg(feature = "ext2fs")]
    pub um_e2fs: Cell<Option<&'static MExt2fs>>,
    /// `um_quotas`: pointer to quota files.
    pub um_quotas: [Cell<Option<&'static Vnode>>; MAXQUOTAS],
    /// `um_cred`: quota file access cred.
    pub um_cred: [Cell<*const Ucred>; MAXQUOTAS],
    /// `um_nindir`: indirect ptrs per block.
    pub um_nindir: Cell<u64>,
    /// `um_bptrtodb`: indir ptr to disk block.
    pub um_bptrtodb: Cell<u64>,
    /// `um_seqinc`: inc between seq blocks.
    pub um_seqinc: Cell<u64>,
    /// `um_btime`: block quota time limit.
    pub um_btime: [Cell<Time>; MAXQUOTAS],
    /// `um_itime`: inode quota time limit.
    pub um_itime: [Cell<Time>; MAXQUOTAS],
    /// `um_qflags`: quota specific flags.
    pub um_qflags: [Cell<u8>; MAXQUOTAS],
    /// `um_export`: export information.
    pub um_export: Netexport,
    /// `um_savedmaxfilesize`: XXX - limit maxfilesize.
    pub um_savedmaxfilesize: Cell<u64>,
    /// `um_maxsymlinklen`: max size of short symlink.
    pub um_maxsymlinklen: Cell<u32>,
}

// SAFETY: the members are changed under the kernel lock while mounting and unmounting, as in
// C.
unsafe impl Sync for Ufsmount {}

impl Ufsmount {
    /// A zeroed structure, as `malloc(M_ZERO)` returns it.
    pub const fn new() -> Self {
        Self {
            um_mountp: Cell::new(None),
            um_dev: Cell::new(0),
            um_devvp: Cell::new(None),
            um_fstype: Cell::new(0),
            um_fs: Cell::new(None),
            #[cfg(feature = "ext2fs")]
            um_e2fs: Cell::new(None),
            um_quotas: [const { Cell::new(None) }; MAXQUOTAS],
            um_cred: [const { Cell::new(ptr::null()) }; MAXQUOTAS],
            um_nindir: Cell::new(0),
            um_bptrtodb: Cell::new(0),
            um_seqinc: Cell::new(0),
            um_btime: [const { Cell::new(0) }; MAXQUOTAS],
            um_itime: [const { Cell::new(0) }; MAXQUOTAS],
            um_qflags: [const { Cell::new(0) }; MAXQUOTAS],
            um_export: Netexport::new(),
            um_savedmaxfilesize: Cell::new(0),
            um_maxsymlinklen: Cell::new(0),
        }
    }

    /// `ump->um_fs`, which every mounted UFS has.
    pub fn fs(&self) -> &'static Fs {
        match self.um_fs.get() {
            Some(fs) => fs,
            None => panic(format_args!("ufsmount {:p}: no um_fs", self)),
        }
    }

    /// `ump->um_e2fs`, which every mounted ext2fs has (feature `ext2fs`).
    #[cfg(feature = "ext2fs")]
    pub fn e2fs(&self) -> &'static MExt2fs {
        match self.um_e2fs.get() {
            Some(fs) => fs,
            None => panic(format_args!("ufsmount {:p}: no um_e2fs", self)),
        }
    }

    /// `f(&ump->um_e2fsb)`: the in-core ext2fs super block, for the duration of `f`.
    #[cfg(feature = "ext2fs")]
    pub fn with_e2fsb<R>(&self, f: impl FnOnce(&Ext2fs) -> R) -> R {
        self.e2fs().with_e2fs(f)
    }

    /// `ump->um_devvp`, which every mounted UFS has.
    pub fn devvp(&self) -> &'static Vnode {
        match self.um_devvp.get() {
            Some(vp) => vp,
            None => panic(format_args!("ufsmount {:p}: no um_devvp", self)),
        }
    }

    /// `ump->um_mountp`.
    pub fn mountp(&self) -> &'static Mount {
        match self.um_mountp.get() {
            Some(mp) => mp,
            None => panic(format_args!("ufsmount {:p}: no um_mountp", self)),
        }
    }
}

impl Default for Ufsmount {
    fn default() -> Self {
        Self::new()
    }
}

/// `UM_UFS1`: filesystem type.
pub const UM_UFS1: u64 = 1;
/// `UM_UFS2`.
pub const UM_UFS2: u64 = 2;
/// `UM_EXT2FS`.
pub const UM_EXT2FS: u64 = 3;

/// `QTF_OPENING`: Q_QUOTAON in progress.
pub const QTF_OPENING: u8 = 0x01;
/// `QTF_CLOSING`: Q_QUOTAOFF in progress.
pub const QTF_CLOSING: u8 = 0x02;

/// `VFSTOUFS(mp)`: convert mount ptr to ufsmount ptr.
pub fn vfstoufs(mp: &Mount) -> &'static Ufsmount {
    let data = mp.mnt_data.get();
    let ufs = matches!(
        mp.mnt_vfc.get().map(|vfc| vfc.name()),
        Some(b"ffs") | Some(b"mfs") | Some(b"ext2fs")
    );
    if data.is_null() || !ufs {
        panic(format_args!(
            "VFSTOUFS: mount {:p} is not a mounted UFS",
            mp
        ));
    }
    // SAFETY: a UFS mount's `mnt_data` (checked above) is the `struct ufsmount` that
    // `ffs_mountfs` allocated, which lives until `ffs_unmount` frees it and clears
    // `mnt_data`.
    unsafe { &*data.cast::<Ufsmount>() }
}

/// `MNINDIR(ump)`: indirect pointers per block.
pub fn mnindir(ump: &Ufsmount) -> u64 {
    ump.um_nindir.get()
}

/// `blkptrtodb(ump, b)`: a block pointer to a disk block.
pub fn blkptrtodb(ump: &Ufsmount, b: Daddr) -> Daddr {
    b << ump.um_bptrtodb.get()
}

/// `is_sequential(ump, a, b)`: whether `b` follows `a` on the disk.
pub fn is_sequential(ump: &Ufsmount, a: Daddr, b: Daddr) -> bool {
    b == a + ump.um_seqinc.get() as Daddr
}
