/* <LICENSES> */
/*
 * Copyright (c) 2026 Emilio Navarrete Lineros <enavarre@outlook.com>
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

/*
 * Copyright (c) 1982, 1989, 1993
 *	The Regents of the University of California.  All rights reserved.
 * (c) UNIX System Laboratories, Inc.
 * All or some portions of this file are derived from material licensed
 * to the University of California by American Telephone and Telegraph
 * Co. or Unix System Laboratories, Inc. and are reproduced herein with
 * the permission of UNIX System Laboratories, Inc.
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
 *	@(#)inode.h	8.5 (Berkeley) 7/8/94
 */
/* </LICENSES> */

/* <CODE> */
//! `<ufs/ufs/inode.h>`: the in-core inode. The inode is used to describe each active (or
//! recently active) file in the UFS filesystem. It is composed of two types of information.
//! The first part is the information that is needed only while the file is active (such as
//! the identity of the file and linkage to speed its lookup). The second part is the
//! permanent meta-data associated with the file which is read in from the permanent dinode
//! from long term storage when the file becomes active, and is put back when the file is no
//! longer being used.
//!
//! Upstream: sys/ufs/ufs/inode.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - An inode is an `ffs_ino_pool` item hung from its vnode's `v_data`, freed by
//!   `ffs_reclaim`; it is reached as `&'static Inode` through [`vtoi`] (the C's `VTOI`), whose
//!   one `unsafe` checks the vnode's tag. The members are `Cell`s: the C changes them through
//!   shared pointers under the inode's lock (`i_lock`).
//! - `dinode_u` (the union of the pool-allocated `ufs1_dinode`, `ufs2_dinode` and
//!   `ext2fs_dinode` pointers) is one `*mut c_void`; the `DIP`, `DIP_ASSIGN`, `DIP_ADD`, ...
//!   macros are the `dip_*`/`dip_set_*` methods, which pick the UFS1 or UFS2 member by
//!   `um_fstype` as the C does under `FFS2` (feature `ffs2`), and panic on an inode without
//!   a dinode where the C would follow NULL. A getter returns the wider of the two members'
//!   types (`DIP`'s conditional expression does the same); a setter truncates to the
//!   member's type, as the assignment does. `SHORTLINK(ip)` is [`Inode::with_shortlink`].
//! - `inode_u` is `i_fs`; with feature `ext2fs` (`option EXT2FS`) the union's other member is
//!   the separate field `i_e2fs` (`Cell<Option<&'static MExt2fs>>`, [`Inode::e2fs`]): an inode is
//!   FFS's or ext2fs's, never both. Likewise `inode_ext.e2fs` (`struct ext2fs_inode_ext`,
//!   [`Ext2fsInodeExt`]) is the field `i_e2fs_ext`, beside `i_dirhash`, and the `i_e2fs_*`
//!   shorthands are methods: `i_e2fs_last_lblk()`, `i_e2fs_last_blk()`, `i_e2fs_uid()`,
//!   `i_e2fs_gid()`, `i_e2fs_ext_cache()` return the member's `&Cell`; `i_e2din` is
//!   [`Inode::with_e2din`] (the one dinode pointer cast to `ext2fs_dinode`) and the shorthands
//!   through it (`i_e2fs_mode`, `i_e2fs_size`, ...) are `i_e2fs_<name>()` getters and
//!   `set_i_e2fs_<name>(v)` setters; `i_e2fs_blocks` is `i_e2fs_blocks()` (a copy of the 15
//!   pointers), `i_e2fs_block(i)` and `set_i_e2fs_block(i, v)`. The C's `i_e2fs_faddr_hi`
//!   names a member that does not exist and is left out. `EXT2FS_ITIMES` is
//!   [`Inode::ext2fs_itimes`].
//! - `i_dquot[]` exists only with feature `quota` (`option QUOTA`), whose `ufs_quota.rs`
//!   defines `struct dquot`; `NODQUOT` is `None`.
//! - `i_dirhash` is `inode_ext` alone, a `Cell<Option<NonNull<Dirhash>>>`, NULL without
//!   feature `ufs_dirhash` (`option UFS_DIRHASH`).
//! - `i_lockf` is a `LockfStateSlot` (`sys/lockf.rs`).
//! - The `struct inode_vtbl` calls (`UFS_TRUNCATE`, `UFS_UPDATE`, ...) are functions with the
//!   macros' names; `iv_inode_alloc` and `iv_buf_alloc` return the vnode or buffer the C
//!   passes back through `vpp`/`bpp`, `iv_bufatoff` the buffer and the offset of `res` in it.
//! - `VTOI`/`ITOV`/`DOINGASYNC` are [`vtoi`], [`Inode::itov`] and [`doingasync`]; `i_devvp`
//!   is [`Inode::i_devvp`].
//! - `struct ufid` is [`Ufid`], converted to and from the `struct fid` it overlays.
//! - The non-`_KERNEL` userland compatibility names (`i_atime`, ...) are left out.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::{self, NonNull};

#[cfg(feature = "ext2fs")]
use crate::kern::kern_tc::gettime;
use crate::kern::subr_prf::panic;
use crate::queue_adapter;
use crate::sys::buf::{Buf, ClusterInfo};
use crate::sys::errno::Errno;
use crate::sys::lockf::LockfStateSlot;
use crate::sys::mount::{Fid, MNT_ASYNC};
use crate::sys::queue::ListEntry;
use crate::sys::rwlock::Rrwlock;
use crate::sys::types::{Daddr, Dev, Mode, Off};
use crate::sys::ucred::Ucred;
use crate::sys::vnode::{VT_EXT2FS, VT_MFS, VT_UFS, Vnode};
#[cfg(feature = "ext2fs")]
use crate::ufs::ext2fs::ext2fs::MExt2fs;
#[cfg(feature = "ext2fs")]
use crate::ufs::ext2fs::ext2fs_dinode::Ext2fsDinode;
#[cfg(feature = "ext2fs")]
use crate::ufs::ext2fs::ext2fs_extents::Ext4ExtentCache;
use crate::ufs::ffs::fs::Fs;
use crate::ufs::ufs::dinode::{
    MAXSYMLINKLEN_UFS1, MAXSYMLINKLEN_UFS2, Ufs1Dinode, Ufs2Dinode, Ufsino,
};
#[cfg(feature = "ext2fs")]
use crate::ufs::ufs::dinode::{NDADDR, NIADDR};
use crate::ufs::ufs::dir::Doff;
use crate::ufs::ufs::dirhash::Dirhash;
#[cfg(feature = "quota")]
use crate::ufs::ufs::quota::MAXQUOTAS;
#[cfg(feature = "quota")]
use crate::ufs::ufs::ufs_quota::Dquot;
#[cfg(feature = "ffs2")]
use crate::ufs::ufs::ufsmount::UM_UFS2;
use crate::ufs::ufs::ufsmount::{UM_UFS1, Ufsmount};

/// `struct ext2fs_inode_ext`: the ext2fs members of an inode.
#[cfg(feature = "ext2fs")]
pub struct Ext2fsInodeExt {
    /// `ext2fs_last_lblk`: last logical blk allocated.
    pub ext2fs_last_lblk: Cell<u32>,
    /// `ext2fs_last_blk`: last blk allocated on disk.
    pub ext2fs_last_blk: Cell<u32>,
    /// `ext2fs_effective_uid`: effective inode uid.
    pub ext2fs_effective_uid: Cell<u32>,
    /// `ext2fs_effective_gid`: effective inode gid.
    pub ext2fs_effective_gid: Cell<u32>,
    /// `ext2fs_extent_cache`.
    pub ext2fs_extent_cache: Cell<Ext4ExtentCache>,
}

#[cfg(feature = "ext2fs")]
impl Ext2fsInodeExt {
    /// A zeroed structure.
    pub const fn new() -> Self {
        Self {
            ext2fs_last_lblk: Cell::new(0),
            ext2fs_last_blk: Cell::new(0),
            ext2fs_effective_uid: Cell::new(0),
            ext2fs_effective_gid: Cell::new(0),
            ext2fs_extent_cache: Cell::new(Ext4ExtentCache {
                ec_start: 0,
                ec_blk: 0,
                ec_len: 0,
                ec_type: 0,
            }),
        }
    }
}

#[cfg(feature = "ext2fs")]
impl Default for Ext2fsInodeExt {
    fn default() -> Self {
        Self::new()
    }
}

/// `struct inode`.
///
/// Protected by: the inode's lock (`i_lock`), held through its vnode.
pub struct Inode {
    /// `i_hash`: hash chain.
    pub i_hash: ListEntry<Inode>,
    /// `i_vnode`: vnode associated with this inode.
    pub i_vnode: Cell<Option<&'static Vnode>>,
    /// `i_ump`.
    pub i_ump: Cell<Option<&'static Ufsmount>>,
    /// `i_flag`: flags, see below.
    pub i_flag: Cell<u32>,
    /// `i_dev`: device associated with the inode.
    pub i_dev: Cell<Dev>,
    /// `i_number`: the identity of the inode.
    pub i_number: Cell<Ufsino>,
    /// `i_effnlink`: `i_nlink` when I/O completes.
    pub i_effnlink: Cell<i32>,
    /// `i_fs` (`inode_u.fs`): associated filesystem.
    pub i_fs: Cell<Option<&'static Fs>>,
    /// `i_e2fs` (`inode_u.e2fs`): associated ext2fs file system (feature `ext2fs`), in place of
    /// `i_fs` for an ext2fs inode.
    #[cfg(feature = "ext2fs")]
    pub i_e2fs: Cell<Option<&'static MExt2fs>>,
    /// `i_ci`.
    pub i_ci: Cell<ClusterInfo>,
    /// `i_dquot`: dquot structures.
    #[cfg(feature = "quota")]
    pub i_dquot: [Cell<Option<&'static Dquot>>; MAXQUOTAS],
    /// `i_modrev`: revision level for NFS lease.
    pub i_modrev: Cell<u64>,
    /// `i_lockf`: byte-level lock state.
    pub i_lockf: LockfStateSlot,
    /// `i_lock`: inode lock.
    pub i_lock: Rrwlock,
    // Side effects; used during directory lookup.
    /// `i_count`: size of free slot in directory.
    pub i_count: Cell<i32>,
    /// `i_endoff`: end of useful stuff in directory.
    pub i_endoff: Cell<Doff>,
    /// `i_diroff`: offset in dir, where we found last entry.
    pub i_diroff: Cell<Doff>,
    /// `i_offset`: offset of free space in directory.
    pub i_offset: Cell<Doff>,
    /// `i_ino`: inode number of found directory.
    pub i_ino: Cell<Ufsino>,
    /// `i_reclen`: size of found directory entry.
    pub i_reclen: Cell<u32>,
    /// `i_dirhash` (`inode_ext.dirhash`): hashing for large directories, a `malloc`ed
    /// `struct dirhash` (`ufs_dirhash.rs`, feature `ufs_dirhash`).
    pub i_dirhash: Cell<Option<NonNull<Dirhash>>>,
    /// `inode_ext.e2fs`: the ext2fs members (feature `ext2fs`), in place of `i_dirhash` for an
    /// ext2fs inode.
    #[cfg(feature = "ext2fs")]
    pub i_e2fs_ext: Ext2fsInodeExt,
    /// `dinode_u`: the on-disk dinode itself (`i_din1`, `i_din2`), a pool item.
    pub dinode_u: Cell<*mut c_void>,
    /// `i_vtbl`.
    pub i_vtbl: Cell<Option<&'static InodeVtbl>>,
}

// SAFETY: the members are changed under the inode's lock or the kernel lock, as in C.
unsafe impl Sync for Inode {}

/// Generates a `DIP(ip, field)` getter and a `DIP_ASSIGN(ip, field, v)` setter.
macro_rules! dip_field {
    ($(#[$doc:meta])* $get:ident, $set:ident, $t:ty, $field:ident) => {
        $(#[$doc])*
        pub fn $get(&self) -> $t {
            #[cfg(feature = "ffs2")]
            if self.is_ufs2() {
                return self.with_din2(|d| d.$field as $t);
            }
            self.with_din1(|d| d.$field as $t)
        }

        $(#[$doc])*
        pub fn $set(&self, v: $t) {
            #[cfg(feature = "ffs2")]
            if self.is_ufs2() {
                return self.with_din2(|d| d.$field = v as _);
            }
            self.with_din1(|d| d.$field = v as _)
        }
    };
}

/// Generates a getter and a setter of a member of the ext2fs dinode (feature `ext2fs`).
macro_rules! e2din_field {
    ($(#[$doc:meta])* $get:ident, $set:ident, $t:ty, $field:ident) => {
        $(#[$doc])*
        #[cfg(feature = "ext2fs")]
        pub fn $get(&self) -> $t {
            self.with_e2din(|d| d.$field)
        }

        $(#[$doc])*
        #[cfg(feature = "ext2fs")]
        pub fn $set(&self, v: $t) {
            self.with_e2din(|d| d.$field = v)
        }
    };
}

impl Inode {
    /// A zeroed inode, as `pool_get(&ffs_ino_pool, PR_ZERO)` returns it.
    pub const fn new() -> Self {
        Self {
            i_hash: ListEntry::new(),
            i_vnode: Cell::new(None),
            i_ump: Cell::new(None),
            i_flag: Cell::new(0),
            i_dev: Cell::new(0),
            i_number: Cell::new(0),
            i_effnlink: Cell::new(0),
            i_fs: Cell::new(None),
            #[cfg(feature = "ext2fs")]
            i_e2fs: Cell::new(None),
            i_ci: Cell::new(ClusterInfo {
                ci_lastr: 0,
                ci_lastw: 0,
                ci_cstart: 0,
                ci_lasta: 0,
                ci_clen: 0,
                ci_ralen: 0,
                ci_maxra: 0,
            }),
            #[cfg(feature = "quota")]
            i_dquot: [const { Cell::new(None) }; MAXQUOTAS],
            i_modrev: Cell::new(0),
            i_lockf: Cell::new(None),
            i_lock: Rrwlock::new("inode"),
            i_count: Cell::new(0),
            i_endoff: Cell::new(0),
            i_diroff: Cell::new(0),
            i_offset: Cell::new(0),
            i_ino: Cell::new(0),
            i_reclen: Cell::new(0),
            i_dirhash: Cell::new(None),
            #[cfg(feature = "ext2fs")]
            i_e2fs_ext: Ext2fsInodeExt::new(),
            dinode_u: Cell::new(ptr::null_mut()),
            i_vtbl: Cell::new(None),
        }
    }

    /// `ITOV(ip)`: the inode's vnode.
    pub fn itov(&self) -> &'static Vnode {
        match self.i_vnode.get() {
            Some(vp) => vp,
            None => panic(format_args!("inode {:p}: no vnode", self)),
        }
    }

    /// `ip->i_ump`.
    pub fn ump(&self) -> &'static Ufsmount {
        match self.i_ump.get() {
            Some(ump) => ump,
            None => panic(format_args!("inode {:p}: no ufsmount", self)),
        }
    }

    /// `ip->i_fs`.
    pub fn fs(&self) -> &'static Fs {
        match self.i_fs.get() {
            Some(fs) => fs,
            None => panic(format_args!("inode {:p}: no fs", self)),
        }
    }

    /// `ip->i_e2fs`: the inode's in-core ext2fs super block (feature `ext2fs`).
    #[cfg(feature = "ext2fs")]
    pub fn e2fs(&self) -> &'static MExt2fs {
        match self.i_e2fs.get() {
            Some(fs) => fs,
            None => panic(format_args!("inode {:p}: no e2fs", self)),
        }
    }

    /// `ip->i_devvp` (`i_ump->um_devvp`).
    pub fn i_devvp(&self) -> &'static Vnode {
        self.ump().devvp()
    }

    /// `ip->i_vtbl`.
    pub fn vtbl(&self) -> &'static InodeVtbl {
        match self.i_vtbl.get() {
            Some(v) => v,
            None => panic(format_args!("inode {:p}: no vtbl", self)),
        }
    }

    /// `ip->i_din1 == NULL`: the inode has no dinode yet (or any more).
    pub fn din_is_null(&self) -> bool {
        self.dinode_u.get().is_null()
    }

    /// Whether the inode's file system is UFS2 (`i_ump->um_fstype == UM_UFS2`).
    #[cfg(feature = "ffs2")]
    pub fn is_ufs2(&self) -> bool {
        self.ump().um_fstype.get() == UM_UFS2
    }

    /// Whether the inode's file system is UFS2: never without feature `ffs2`.
    #[cfg(not(feature = "ffs2"))]
    pub fn is_ufs2(&self) -> bool {
        false
    }

    /// `f(ip->i_din1)`: the UFS1 dinode, for the duration of `f`.
    pub fn with_din1<R>(&self, f: impl FnOnce(&mut Ufs1Dinode) -> R) -> R {
        let p = self.dinode_u.get().cast::<Ufs1Dinode>();
        if p.is_null() {
            panic(format_args!("inode {:p}: no dinode", self));
        }
        // SAFETY: `ffs_vget` points `dinode_u` at the inode's own pool dinode of the mount's
        // type before the inode is used, and `ffs_reclaim` frees it with the inode; the
        // reference does not outlive `f`, and no other one is alive meanwhile (the accessors
        // are the only way in, none of them nests, and they run under the kernel lock).
        f(unsafe { &mut *p })
    }

    /// `f(ip->i_din2)`: the UFS2 dinode, for the duration of `f`.
    pub fn with_din2<R>(&self, f: impl FnOnce(&mut Ufs2Dinode) -> R) -> R {
        let p = self.dinode_u.get().cast::<Ufs2Dinode>();
        if p.is_null() {
            panic(format_args!("inode {:p}: no dinode", self));
        }
        // SAFETY: as in `with_din1`.
        f(unsafe { &mut *p })
    }

    dip_field!(
        /// `DIP(ip, mode)`.
        dip_mode, dip_set_mode, Mode, di_mode
    );
    dip_field!(
        /// `DIP(ip, nlink)`.
        dip_nlink, dip_set_nlink, i32, di_nlink
    );
    dip_field!(
        /// `DIP(ip, uid)`.
        dip_uid, dip_set_uid, u32, di_uid
    );
    dip_field!(
        /// `DIP(ip, gid)`.
        dip_gid, dip_set_gid, u32, di_gid
    );
    dip_field!(
        /// `DIP(ip, size)`.
        dip_size, dip_set_size, u64, di_size
    );
    dip_field!(
        /// `DIP(ip, blocks)`.
        dip_blocks, dip_set_blocks, i64, di_blocks
    );
    dip_field!(
        /// `DIP(ip, atime)`.
        dip_atime, dip_set_atime, i64, di_atime
    );
    dip_field!(
        /// `DIP(ip, atimensec)`.
        dip_atimensec, dip_set_atimensec, i32, di_atimensec
    );
    dip_field!(
        /// `DIP(ip, mtime)`.
        dip_mtime, dip_set_mtime, i64, di_mtime
    );
    dip_field!(
        /// `DIP(ip, mtimensec)`.
        dip_mtimensec, dip_set_mtimensec, i32, di_mtimensec
    );
    dip_field!(
        /// `DIP(ip, ctime)`.
        dip_ctime, dip_set_ctime, i64, di_ctime
    );
    dip_field!(
        /// `DIP(ip, ctimensec)`.
        dip_ctimensec, dip_set_ctimensec, i32, di_ctimensec
    );
    dip_field!(
        /// `DIP(ip, flags)`.
        dip_flags, dip_set_flags, u32, di_flags
    );
    dip_field!(
        /// `DIP(ip, gen)`.
        dip_gen, dip_set_gen, u32, di_gen
    );

    /// `DIP(ip, db[i])`.
    pub fn dip_db(&self, i: usize) -> Daddr {
        #[cfg(feature = "ffs2")]
        if self.is_ufs2() {
            return self.with_din2(|d| d.di_db[i]);
        }
        self.with_din1(|d| Daddr::from(d.di_db[i]))
    }

    /// `DIP_ASSIGN(ip, db[i], v)`.
    pub fn dip_set_db(&self, i: usize, v: Daddr) {
        #[cfg(feature = "ffs2")]
        if self.is_ufs2() {
            return self.with_din2(|d| d.di_db[i] = v);
        }
        self.with_din1(|d| d.di_db[i] = v as i32)
    }

    /// `DIP(ip, ib[i])`.
    pub fn dip_ib(&self, i: usize) -> Daddr {
        #[cfg(feature = "ffs2")]
        if self.is_ufs2() {
            return self.with_din2(|d| d.di_ib[i]);
        }
        self.with_din1(|d| Daddr::from(d.di_ib[i]))
    }

    /// `DIP_ASSIGN(ip, ib[i], v)`.
    pub fn dip_set_ib(&self, i: usize, v: Daddr) {
        #[cfg(feature = "ffs2")]
        if self.is_ufs2() {
            return self.with_din2(|d| d.di_ib[i] = v);
        }
        self.with_din1(|d| d.di_ib[i] = v as i32)
    }

    /// `DIP(ip, rdev)` (`di_db[0]`).
    pub fn dip_rdev(&self) -> i64 {
        self.dip_db(0)
    }

    /// `DIP_ASSIGN(ip, rdev, v)`.
    pub fn dip_set_rdev(&self, v: i64) {
        self.dip_set_db(0, v);
    }

    /// `MAXSYMLINKLEN(ip)`: the longest symbolic link kept in the dinode itself.
    pub fn maxsymlinklen(&self) -> usize {
        if self.ump().um_fstype.get() == UM_UFS1 {
            MAXSYMLINKLEN_UFS1
        } else {
            MAXSYMLINKLEN_UFS2
        }
    }

    /// `f(SHORTLINK(ip))`: the bytes of `di_db` and `di_ib`, where a short symbolic link
    /// keeps its target, for the duration of `f`.
    pub fn with_shortlink<R>(&self, f: impl FnOnce(&mut [u8]) -> R) -> R {
        #[cfg(feature = "ffs2")]
        if self.is_ufs2() {
            return self.with_din2(|d| {
                let p = ptr::from_mut(d).cast::<u8>();
                // SAFETY: `di_db` and `di_ib` are adjacent `int64_t` arrays of
                // `MAXSYMLINKLEN_UFS2` bytes inside the dinode `d` borrows (the dinode.rs
                // layout checks), and `d` is not used while the slice lives.
                let s = unsafe {
                    core::slice::from_raw_parts_mut(
                        p.add(core::mem::offset_of!(Ufs2Dinode, di_db)),
                        MAXSYMLINKLEN_UFS2,
                    )
                };
                f(s)
            });
        }
        self.with_din1(|d| {
            let p = ptr::from_mut(d).cast::<u8>();
            // SAFETY: as above, `MAXSYMLINKLEN_UFS1` bytes of `int32_t` arrays.
            let s = unsafe {
                core::slice::from_raw_parts_mut(
                    p.add(core::mem::offset_of!(Ufs1Dinode, di_db)),
                    MAXSYMLINKLEN_UFS1,
                )
            };
            f(s)
        })
    }

    /// `f(ip->i_e2din)`: the ext2fs dinode, for the duration of `f`.
    #[cfg(feature = "ext2fs")]
    pub fn with_e2din<R>(&self, f: impl FnOnce(&mut Ext2fsDinode) -> R) -> R {
        let p = self.dinode_u.get().cast::<Ext2fsDinode>();
        if p.is_null() {
            panic(format_args!("inode {:p}: no dinode", self));
        }
        // SAFETY: `ext2fs_vget` points `dinode_u` at the inode's own pool `ext2fs_dinode`
        // before the inode is used, and `ext2fs_reclaim` frees it with the inode; the
        // reference does not outlive `f`, and no other one is alive meanwhile (the accessors
        // are the only way in, none of them nests, and they run under the kernel lock).
        f(unsafe { &mut *p })
    }

    /// `ip->i_e2fs_last_lblk` (`inode_ext.e2fs.ext2fs_last_lblk`).
    #[cfg(feature = "ext2fs")]
    pub fn i_e2fs_last_lblk(&self) -> &Cell<u32> {
        &self.i_e2fs_ext.ext2fs_last_lblk
    }

    /// `ip->i_e2fs_last_blk`.
    #[cfg(feature = "ext2fs")]
    pub fn i_e2fs_last_blk(&self) -> &Cell<u32> {
        &self.i_e2fs_ext.ext2fs_last_blk
    }

    /// `ip->i_e2fs_uid`: the effective uid.
    #[cfg(feature = "ext2fs")]
    pub fn i_e2fs_uid(&self) -> &Cell<u32> {
        &self.i_e2fs_ext.ext2fs_effective_uid
    }

    /// `ip->i_e2fs_gid`: the effective gid.
    #[cfg(feature = "ext2fs")]
    pub fn i_e2fs_gid(&self) -> &Cell<u32> {
        &self.i_e2fs_ext.ext2fs_effective_gid
    }

    /// `ip->i_e2fs_ext_cache`: the extent cache.
    #[cfg(feature = "ext2fs")]
    pub fn i_e2fs_ext_cache(&self) -> &Cell<Ext4ExtentCache> {
        &self.i_e2fs_ext.ext2fs_extent_cache
    }

    e2din_field!(
        /// `ip->i_e2fs_mode`.
        i_e2fs_mode, set_i_e2fs_mode, u16, e2di_mode
    );
    e2din_field!(
        /// `ip->i_e2fs_size`.
        i_e2fs_size, set_i_e2fs_size, u32, e2di_size
    );
    e2din_field!(
        /// `ip->i_e2fs_atime`.
        i_e2fs_atime, set_i_e2fs_atime, u32, e2di_atime
    );
    e2din_field!(
        /// `ip->i_e2fs_ctime`.
        i_e2fs_ctime, set_i_e2fs_ctime, u32, e2di_ctime
    );
    e2din_field!(
        /// `ip->i_e2fs_mtime`.
        i_e2fs_mtime, set_i_e2fs_mtime, u32, e2di_mtime
    );
    e2din_field!(
        /// `ip->i_e2fs_dtime`.
        i_e2fs_dtime, set_i_e2fs_dtime, u32, e2di_dtime
    );
    e2din_field!(
        /// `ip->i_e2fs_nlink`.
        i_e2fs_nlink, set_i_e2fs_nlink, u16, e2di_nlink
    );
    e2din_field!(
        /// `ip->i_e2fs_nblock`.
        i_e2fs_nblock, set_i_e2fs_nblock, u32, e2di_nblock
    );
    e2din_field!(
        /// `ip->i_e2fs_flags`.
        i_e2fs_flags, set_i_e2fs_flags, u32, e2di_flags
    );
    e2din_field!(
        /// `ip->i_e2fs_gen`.
        i_e2fs_gen, set_i_e2fs_gen, u32, e2di_gen
    );
    e2din_field!(
        /// `ip->i_e2fs_facl`.
        i_e2fs_facl, set_i_e2fs_facl, u32, e2di_facl
    );
    e2din_field!(
        /// `ip->i_e2fs_size_hi`.
        i_e2fs_size_hi, set_i_e2fs_size_hi, u32, e2di_size_hi
    );
    e2din_field!(
        /// `ip->i_e2fs_faddr`.
        i_e2fs_faddr, set_i_e2fs_faddr, u32, e2di_faddr
    );
    e2din_field!(
        /// `ip->i_e2fs_nblock_hi`.
        i_e2fs_nblock_hi, set_i_e2fs_nblock_hi, u16, e2di_nblock_hi
    );
    e2din_field!(
        /// `ip->i_e2fs_uid_low`.
        i_e2fs_uid_low, set_i_e2fs_uid_low, u16, e2di_uid_low
    );
    e2din_field!(
        /// `ip->i_e2fs_gid_low`.
        i_e2fs_gid_low, set_i_e2fs_gid_low, u16, e2di_gid_low
    );
    e2din_field!(
        /// `ip->i_e2fs_uid_high`.
        i_e2fs_uid_high, set_i_e2fs_uid_high, u16, e2di_uid_high
    );
    e2din_field!(
        /// `ip->i_e2fs_gid_high`.
        i_e2fs_gid_high, set_i_e2fs_gid_high, u16, e2di_gid_high
    );

    /// `ip->i_e2fs_blocks`: a copy of the block pointers (`NDADDR + NIADDR` of them).
    #[cfg(feature = "ext2fs")]
    pub fn i_e2fs_blocks(&self) -> [u32; NDADDR + NIADDR] {
        self.with_e2din(|d| d.e2di_blocks)
    }

    /// `ip->i_e2fs_blocks[i]`.
    #[cfg(feature = "ext2fs")]
    pub fn i_e2fs_block(&self, i: usize) -> u32 {
        self.with_e2din(|d| d.e2di_blocks[i])
    }

    /// `ip->i_e2fs_blocks[i] = v`.
    #[cfg(feature = "ext2fs")]
    pub fn set_i_e2fs_block(&self, i: usize, v: u32) {
        self.with_e2din(|d| d.e2di_blocks[i] = v)
    }

    /// `EXT2FS_ITIMES(ip)`: updates the access, modification and change times the flags ask
    /// for, as `UFS_ITIMES` does for an FFS inode.
    #[cfg(feature = "ext2fs")]
    pub fn ext2fs_itimes(&self) {
        let flag = self.i_flag.get();
        if flag & (IN_ACCESS | IN_CHANGE | IN_UPDATE) == 0 {
            return;
        }
        self.i_flag.set(flag | IN_MODIFIED);
        if flag & IN_ACCESS != 0 {
            self.set_i_e2fs_atime(gettime() as u32);
        }
        if flag & IN_UPDATE != 0 {
            self.set_i_e2fs_mtime(gettime() as u32);
        }
        if flag & IN_CHANGE != 0 {
            self.set_i_e2fs_ctime(gettime() as u32);
            self.i_modrev.set(self.i_modrev.get().wrapping_add(1));
        }
        self.i_flag
            .set(self.i_flag.get() & !(IN_ACCESS | IN_CHANGE | IN_UPDATE));
    }

    /// `*ip->i_din1`: a copy of the UFS1 dinode.
    pub fn din1(&self) -> Ufs1Dinode {
        self.with_din1(|d| *d)
    }

    /// `*ip->i_din2`: a copy of the UFS2 dinode.
    pub fn din2(&self) -> Ufs2Dinode {
        self.with_din2(|d| *d)
    }
}

impl Default for Inode {
    fn default() -> Self {
        Self::new()
    }
}

queue_adapter!(
    /// `LIST_HEAD(ihashhead, inode)`: an inode hash chain, through `i_hash`.
    pub IHash: Inode, i_hash => ListEntry<Inode>
);

/// `iv_truncate`.
pub type IvTruncateFn = fn(&Inode, Off, i32, *const Ucred) -> Result<(), Errno>;
/// `iv_update`.
pub type IvUpdateFn = fn(&Inode, i32) -> Result<(), Errno>;
/// `iv_inode_alloc`: the new inode's vnode, locked and referenced.
pub type IvInodeAllocFn = fn(&Inode, Mode, *const Ucred) -> Result<&'static Vnode, Errno>;
/// `iv_inode_free`.
pub type IvInodeFreeFn = fn(&Inode, Ufsino, Mode) -> Result<(), Errno>;
/// `iv_buf_alloc`: `bpp` is the C's out-parameter, NULL when the caller wants no buffer.
pub type IvBufAllocFn =
    fn(&Inode, Off, i32, *const Ucred, i32, Option<&mut Option<&'static Buf>>) -> Result<(), Errno>;
/// `iv_bufatoff`: the buffer and the offset of `*res` in its data.
pub type IvBufatoffFn = fn(&Inode, Off) -> Result<(&'static Buf, usize), Errno>;

/// `struct inode_vtbl`: the file system's inode operations the UFS layer calls.
pub struct InodeVtbl {
    /// `iv_truncate`.
    pub iv_truncate: IvTruncateFn,
    /// `iv_update`.
    pub iv_update: IvUpdateFn,
    /// `iv_inode_alloc`.
    pub iv_inode_alloc: IvInodeAllocFn,
    /// `iv_inode_free`.
    pub iv_inode_free: IvInodeFreeFn,
    /// `iv_buf_alloc`.
    pub iv_buf_alloc: IvBufAllocFn,
    /// `iv_bufatoff`.
    pub iv_bufatoff: IvBufatoffFn,
}

/// `UFS_TRUNCATE(ip, off, flags, cred)`.
#[allow(non_snake_case)] // the C macro's name
pub fn UFS_TRUNCATE(ip: &Inode, off: Off, flags: i32, cred: *const Ucred) -> Result<(), Errno> {
    (ip.vtbl().iv_truncate)(ip, off, flags, cred)
}

/// `UFS_UPDATE(ip, sync)`.
#[allow(non_snake_case)] // the C macro's name
pub fn UFS_UPDATE(ip: &Inode, sync: i32) -> Result<(), Errno> {
    (ip.vtbl().iv_update)(ip, sync)
}

/// `UFS_INODE_ALLOC(pip, mode, cred, vpp)`.
#[allow(non_snake_case)] // the C macro's name
pub fn UFS_INODE_ALLOC(
    pip: &Inode,
    mode: Mode,
    cred: *const Ucred,
) -> Result<&'static Vnode, Errno> {
    (pip.vtbl().iv_inode_alloc)(pip, mode, cred)
}

/// `UFS_INODE_FREE(pip, ino, mode)`.
#[allow(non_snake_case)] // the C macro's name
pub fn UFS_INODE_FREE(pip: &Inode, ino: Ufsino, mode: Mode) -> Result<(), Errno> {
    (pip.vtbl().iv_inode_free)(pip, ino, mode)
}

/// `UFS_BUF_ALLOC(ip, startoffset, size, cred, flags, bpp)`: the buffer the C returns in
/// `*bpp`.
#[allow(non_snake_case)] // the C macro's name
pub fn UFS_BUF_ALLOC(
    ip: &Inode,
    startoffset: Off,
    size: i32,
    cred: *const Ucred,
    flags: i32,
) -> Result<&'static Buf, Errno> {
    let mut bp = None;
    (ip.vtbl().iv_buf_alloc)(ip, startoffset, size, cred, flags, Some(&mut bp))?;
    match bp {
        Some(bp) => Ok(bp),
        None => panic(format_args!("UFS_BUF_ALLOC: no buffer")),
    }
}

/// `UFS_BUFATOFF(ip, offset, res, bpp)`: the buffer and the offset of `res` in its data.
#[allow(non_snake_case)] // the C macro's name
pub fn UFS_BUFATOFF(ip: &Inode, offset: Off) -> Result<(&'static Buf, usize), Errno> {
    (ip.vtbl().iv_bufatoff)(ip, offset)
}

/// `IN_ACCESS`: access time update request.
pub const IN_ACCESS: u32 = 0x0001;
/// `IN_CHANGE`: inode change time update request.
pub const IN_CHANGE: u32 = 0x0002;
/// `IN_UPDATE`: modification time update request.
pub const IN_UPDATE: u32 = 0x0004;
/// `IN_MODIFIED`: inode has been modified.
pub const IN_MODIFIED: u32 = 0x0008;
/// `IN_RENAME`: inode is being renamed.
pub const IN_RENAME: u32 = 0x0010;
/// `IN_SHLOCK`: file has shared lock.
pub const IN_SHLOCK: u32 = 0x0020;
/// `IN_EXLOCK`: file has exclusive lock.
pub const IN_EXLOCK: u32 = 0x0040;
/// `IN_LAZYMOD`: modified, but don't write yet.
pub const IN_LAZYMOD: u32 = 0x0080;
/// `IN_HASHED`: inode is on the hash chain.
pub const IN_HASHED: u32 = 0x0100;

impl Inode {
    /// `ip->i_flag |= f`.
    pub fn set_flag(&self, f: u32) {
        self.i_flag.set(self.i_flag.get() | f);
    }

    /// `ip->i_flag &= ~f`.
    pub fn clr_flag(&self, f: u32) {
        self.i_flag.set(self.i_flag.get() & !f);
    }
}

/// `struct indir`: logical block paths generated by `ufs_getlbns` and used by truncate and
/// bmap code.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Indir {
    /// `in_lbn`: logical block number.
    pub in_lbn: Daddr,
    /// `in_off`: offset in buffer.
    pub in_off: i32,
    /// `in_exists`: flag if the block exists.
    pub in_exists: i32,
}

/// `VTOI(vp)`: the inode of a UFS vnode.
pub fn vtoi(vp: &Vnode) -> &'static Inode {
    let data = vp.v_data.get();
    if data.is_null() || !matches!(vp.v_tag.get(), VT_UFS | VT_MFS | VT_EXT2FS) {
        panic(format_args!("VTOI: vnode {:p} has no inode", vp));
    }
    // SAFETY: a UFS vnode's `v_data` (checked above) is the inode `ffs_vget` hung there,
    // which lives until `ffs_reclaim` frees it and clears `v_data`; the caller holds the
    // vnode (a reference or its lock), so it is not reclaimed meanwhile.
    unsafe { &*data.cast::<Inode>() }
}

/// `DOINGASYNC(vp)`: the file system is mounted `MNT_ASYNC`.
pub fn doingasync(vp: &Vnode) -> bool {
    vp.v_mount
        .get()
        .is_some_and(|mp| mp.mnt_flag.get() & MNT_ASYNC != 0)
}

/// `struct ufid`: this overlays the fid structure (see mount.h).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ufid {
    /// `ufid_len`: length of structure.
    pub ufid_len: u16,
    /// `ufid_pad`: force 32-bit alignment.
    pub ufid_pad: u16,
    /// `ufid_ino`: file number (ino).
    pub ufid_ino: Ufsino,
    /// `ufid_gen`: generation number.
    pub ufid_gen: u32,
}

impl Ufid {
    /// `(struct ufid *)fhp`: the file identifier read as a UFS one.
    pub fn from_fid(fid: &Fid) -> Self {
        let d = &fid.fid_data;
        Self {
            ufid_len: fid.fid_len,
            ufid_pad: fid.fid_reserved,
            ufid_ino: u32::from_ne_bytes([d[0], d[1], d[2], d[3]]),
            ufid_gen: u32::from_ne_bytes([d[4], d[5], d[6], d[7]]),
        }
    }

    /// Writes the UFS identifier over `fid` (the C's stores through the cast pointer).
    pub fn to_fid(&self, fid: &mut Fid) {
        fid.fid_len = self.ufid_len;
        fid.fid_reserved = self.ufid_pad;
        fid.fid_data[0..4].copy_from_slice(&self.ufid_ino.to_ne_bytes());
        fid.fid_data[4..8].copy_from_slice(&self.ufid_gen.to_ne_bytes());
    }
}

/// `((int32_t *)b_data)[i]`: an FFS1 block pointer in an indirect block.
pub fn daddr32_at(b: &[u8], i: usize) -> Daddr {
    let o = i * 4;
    Daddr::from(i32::from_ne_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]))
}

/// `((int32_t *)b_data)[i] = v`.
pub fn set_daddr32_at(b: &mut [u8], i: usize, v: Daddr) {
    b[i * 4..i * 4 + 4].copy_from_slice(&(v as i32).to_ne_bytes());
}

/// `((int64_t *)b_data)[i]`: an FFS2 block pointer in an indirect block.
pub fn daddr64_at(b: &[u8], i: usize) -> Daddr {
    let o = i * 8;
    let mut w = [0u8; 8];
    w.copy_from_slice(&b[o..o + 8]);
    i64::from_ne_bytes(w)
}

/// `((int64_t *)b_data)[i] = v`.
pub fn set_daddr64_at(b: &mut [u8], i: usize, v: Daddr) {
    b[i * 8..i * 8 + 8].copy_from_slice(&v.to_ne_bytes());
}

/// `((struct ufs1_dinode *)b_data)[i]`: a copy of an FFS1 dinode in an inode block.
pub fn dinode1_at(b: &[u8], i: usize) -> Ufs1Dinode {
    let s = &b[i * size_of::<Ufs1Dinode>()..(i + 1) * size_of::<Ufs1Dinode>()];
    // SAFETY: `s` holds `size_of::<Ufs1Dinode>()` bytes (the slice bounds), and the
    // structure is `#[repr(C)]` integers, valid for any bit pattern; the read is unaligned.
    unsafe { ptr::read_unaligned(s.as_ptr().cast::<Ufs1Dinode>()) }
}

/// `((struct ufs1_dinode *)b_data)[i] = *d`.
pub fn set_dinode1_at(b: &mut [u8], i: usize, d: &Ufs1Dinode) {
    let s = &mut b[i * size_of::<Ufs1Dinode>()..(i + 1) * size_of::<Ufs1Dinode>()];
    // SAFETY: `s` holds `size_of::<Ufs1Dinode>()` writable bytes (the slice bounds); the
    // structure has no padding (the dinode.rs layout checks), so every byte written is
    // initialised; the write is unaligned.
    unsafe { ptr::write_unaligned(s.as_mut_ptr().cast::<Ufs1Dinode>(), *d) };
}

/// `((struct ufs2_dinode *)b_data)[i]`: a copy of an FFS2 dinode in an inode block.
pub fn dinode2_at(b: &[u8], i: usize) -> Ufs2Dinode {
    let s = &b[i * size_of::<Ufs2Dinode>()..(i + 1) * size_of::<Ufs2Dinode>()];
    // SAFETY: as in `dinode1_at`.
    unsafe { ptr::read_unaligned(s.as_ptr().cast::<Ufs2Dinode>()) }
}

/// `((struct ufs2_dinode *)b_data)[i] = *d`.
pub fn set_dinode2_at(b: &mut [u8], i: usize, d: &Ufs2Dinode) {
    let s = &mut b[i * size_of::<Ufs2Dinode>()..(i + 1) * size_of::<Ufs2Dinode>()];
    // SAFETY: as in `set_dinode1_at`.
    unsafe { ptr::write_unaligned(s.as_mut_ptr().cast::<Ufs2Dinode>(), *d) };
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn flags_match_the_c_header() {
        let defs = crate::reftest::defines("sys/ufs/ufs/inode.h");
        for (name, value) in [
            ("IN_ACCESS", IN_ACCESS),
            ("IN_CHANGE", IN_CHANGE),
            ("IN_UPDATE", IN_UPDATE),
            ("IN_MODIFIED", IN_MODIFIED),
            ("IN_RENAME", IN_RENAME),
            ("IN_SHLOCK", IN_SHLOCK),
            ("IN_EXLOCK", IN_EXLOCK),
            ("IN_LAZYMOD", IN_LAZYMOD),
            ("IN_HASHED", IN_HASHED),
        ] {
            assert_eq!(
                crate::reftest::int(&defs, name),
                Some(i64::from(value)),
                "{name}"
            );
        }
    }

    #[test]
    fn block_pointers_and_fids_round_trip() {
        let mut b = [0u8; 64];
        set_daddr32_at(&mut b, 3, -7);
        assert_eq!(daddr32_at(&b, 3), -7);
        set_daddr64_at(&mut b, 5, 1 << 40);
        assert_eq!(daddr64_at(&b, 5), 1 << 40);
        let u = Ufid {
            ufid_len: 12,
            ufid_pad: 0,
            ufid_ino: 99,
            ufid_gen: 0xdead_beef,
        };
        let mut fid = Fid::default();
        u.to_fid(&mut fid);
        assert_eq!(Ufid::from_fid(&fid), u);
        assert_eq!(size_of::<Ufid>(), 12);
    }
}
/* </TESTS> */
