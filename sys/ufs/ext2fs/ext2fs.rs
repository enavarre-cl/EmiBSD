/*	$OpenBSD: ext2fs.h,v 1.27 2024/07/15 13:27:36 martijn Exp $	*/
/*	$NetBSD: ext2fs.h,v 1.10 2000/01/28 16:00:23 bouyer Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1997 Manuel Bouyer.
 * Copyright (c) 1982, 1986, 1993
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
 *	@(#)fs.h	8.10 (Berkeley) 10/27/94
 *  Modified for ext2fs by Manuel Bouyer.
 */
/* </LICENSES> */

//! `<ufs/ext2fs/ext2fs.h>`: the layout of a second extended file system: the super-block
//! (`struct ext2fs`), its in-core form (`struct m_ext2fs`), the block group descriptor
//! (`struct ext2_gd`), the feature flags, and the macros that turn inode numbers, logical
//! blocks and fragments into disk addresses.
//!
//! Upstream: sys/ufs/ext2fs/ext2fs.h @ 3ce1f3f79392
//!
//! Each disk drive contains some number of file systems. A file system consists of a number
//! of block groups ("cylinder groups"). Each group has inodes and data. A file system is
//! described by its super-block, which in turn describes the groups. The super-block is
//! critical data and is replicated in the groups to protect against catastrophic loss.
//!
//! Inodes are, like in UFS, 32-bit unsigned integers ([`Ufsino`]). Disk blocks are 32-bit if
//! the file system is not operating in 64-bit mode (the incompatible ext4 64BIT flag); the C
//! says "XXX disk blocks are simply `u_int32_t` for now", and so does this port.
//!
//! Ext2 metadata is stored in little-endian byte order (the JBD2 journal of ext3 and ext4 is
//! big-endian).
//!
//! ## Deviations
//! - `struct ext2fs` and `struct ext2_gd` are plain `#[repr(C)]` data ([`Ext2fs`], [`Ext2Gd`]),
//!   the on-disk image; their sizes are pinned by the compile-time checks below. The in-core
//!   [`MExt2fs`] keeps its `Ext2fs` in an `UnsafeCell` (the C changes `fs->e2fs.e2fs_fbcount`
//!   and friends through shared pointers); it is reached through [`MExt2fs::with_e2fs`] and
//!   [`MExt2fs::with_e2fs_mut`] (as `inode.rs`'s `with_din1`) and the scalar getters and setters
//!   (`e2fs_ipg()`, `set_e2fs_fbcount()`, ...), none of which hands out a reference that
//!   outlives the call. The remaining members of `struct m_ext2fs` are `Cell`s, as `Fs`'s are.
//! - `e2fs_gd` is a raw pointer to the `malloc`ed descriptors, read and written through the
//!   bounds-checked [`MExt2fs::gd`], [`MExt2fs::set_gd`] and [`MExt2fs::with_gd_mut`]
//!   (`e2fs_ncg` descriptors).
//! - Every macro is a function with the macro's name ([`fsbtodb`], [`dbtofsb`], [`ino_to_cg`],
//!   [`ino_to_fsba`], [`ino_to_fsbo`], [`dtog`], [`dtogd`], [`blkoff`], [`lblktosize`],
//!   [`lblkno`], [`blkroundup`], [`fragroundup`], [`freespace`]), and `NINDIR` is [`nindir`].
//!   `fsbtodb`/`dbtofsb`/`lblktosize` work on `daddr_t`/`off_t` here, so a block number times
//!   the sectors per block does not wrap at 32 bits as the C macros do on a `u_int32_t`.
//!   `ino_to_*` wrap `x - 1` at 32 bits as the C does for inode 0.
//! - `e2fs_sbload`/`e2fs_sbsave`/`e2fs_cgload`/`e2fs_cgsave` are functions that are always
//!   compiled: they copy on a little-endian machine and call the byte swappers of
//!   `ext2fs_bswap.rs` on a big-endian one (the C picks one or the other with `#if
//!   BYTE_ORDER`). `e2fs_cgload` and `e2fs_cgsave` decode each descriptor from little-endian
//!   bytes, so they need no swapper.
//! - The `static const` feature name tables are the constants `RO_COMPAT` and `INCOMPAT`.
//! - `cg_has_sb` computes in 64 bits, where the C's `int` powers overflow for huge `i`.
//! - `e2fs_overflow` is a free function over `&MExt2fs`.

use core::cell::{Cell, UnsafeCell};
use core::mem::size_of;
use core::ptr;

use crate::kern::subr_prf::panic;
use crate::sys::param::DEV_BSIZE;
use crate::sys::types::{Daddr, Off};
use crate::ufs::ufs::dinode::Ufsino;

/// `BBSIZE`: the boot block's size.
pub const BBSIZE: usize = 1024;
/// `SBSIZE`: the super-block's size.
pub const SBSIZE: usize = 1024;
/// `BBOFF`.
pub const BBOFF: Off = 0;
/// `SBOFF`.
pub const SBOFF: Off = BBOFF + BBSIZE as Off;
/// `BBLOCK`.
pub const BBLOCK: Daddr = 0;
/// `SBLOCK`.
pub const SBLOCK: Daddr = BBLOCK + (BBSIZE / DEV_BSIZE) as Daddr;

/// `LOG_MINBSIZE`.
pub const LOG_MINBSIZE: u32 = 10;
/// `MINBSIZE`: the smallest allowable block size.
pub const MINBSIZE: usize = 1 << LOG_MINBSIZE;
/// `LOG_MINFSIZE`.
pub const LOG_MINFSIZE: u32 = 10;
/// `MINFSIZE`.
pub const MINFSIZE: usize = 1 << LOG_MINFSIZE;

/// `MAXMNTLEN`: the space allocated in `struct m_ext2fs` for the name mounted on.
pub const MAXMNTLEN: usize = 512;

/// `MINFREE`: the minimum acceptable percentage of file system blocks which may be free.
pub const MINFREE: i32 = 5;

/// `E2FS_MAGIC`: the ext2fs magic number.
pub const E2FS_MAGIC: u16 = 0xef53;
/// `E2FS_REV0`: revision level 0.
pub const E2FS_REV0: u32 = 0;
/// `E2FS_REV1`: revision level 1.
pub const E2FS_REV1: u32 = 1;

/// `EXT2F_COMPAT_PREALLOC`.
pub const EXT2F_COMPAT_PREALLOC: u32 = 0x0001;
/// `EXT2F_COMPAT_IMAGIC_INODES`.
pub const EXT2F_COMPAT_IMAGIC_INODES: u32 = 0x0002;
/// `EXT2F_COMPAT_HAS_JOURNAL`.
pub const EXT2F_COMPAT_HAS_JOURNAL: u32 = 0x0004;
/// `EXT2F_COMPAT_EXT_ATTR`.
pub const EXT2F_COMPAT_EXT_ATTR: u32 = 0x0008;
/// `EXT2F_COMPAT_RESIZE`.
pub const EXT2F_COMPAT_RESIZE: u32 = 0x0010;
/// `EXT2F_COMPAT_DIR_INDEX`.
pub const EXT2F_COMPAT_DIR_INDEX: u32 = 0x0020;
/// `EXT2F_COMPAT_SPARSE_SUPER2`.
pub const EXT2F_COMPAT_SPARSE_SUPER2: u32 = 0x0200;

/// `EXT2F_ROCOMPAT_SPARSE_SUPER`.
pub const EXT2F_ROCOMPAT_SPARSE_SUPER: u32 = 0x0001;
/// `EXT2F_ROCOMPAT_LARGE_FILE`.
pub const EXT2F_ROCOMPAT_LARGE_FILE: u32 = 0x0002;
/// `EXT2F_ROCOMPAT_BTREE_DIR`.
pub const EXT2F_ROCOMPAT_BTREE_DIR: u32 = 0x0004;
/// `EXT2F_ROCOMPAT_HUGE_FILE`.
pub const EXT2F_ROCOMPAT_HUGE_FILE: u32 = 0x0008;
/// `EXT2F_ROCOMPAT_GDT_CSUM`.
pub const EXT2F_ROCOMPAT_GDT_CSUM: u32 = 0x0010;
/// `EXT2F_ROCOMPAT_DIR_NLINK`.
pub const EXT2F_ROCOMPAT_DIR_NLINK: u32 = 0x0020;
/// `EXT2F_ROCOMPAT_EXTRA_ISIZE`.
pub const EXT2F_ROCOMPAT_EXTRA_ISIZE: u32 = 0x0040;
/// `EXT2F_ROCOMPAT_QUOTA`.
pub const EXT2F_ROCOMPAT_QUOTA: u32 = 0x0100;
/// `EXT2F_ROCOMPAT_BIGALLOC`.
pub const EXT2F_ROCOMPAT_BIGALLOC: u32 = 0x0200;
/// `EXT2F_ROCOMPAT_METADATA_CKSUM`.
pub const EXT2F_ROCOMPAT_METADATA_CKSUM: u32 = 0x0400;
/// `EXT2F_ROCOMPAT_READONLY`.
pub const EXT2F_ROCOMPAT_READONLY: u32 = 0x1000;
/// `EXT2F_ROCOMPAT_PROJECT`.
pub const EXT2F_ROCOMPAT_PROJECT: u32 = 0x2000;

/// `EXT2F_INCOMPAT_COMP`.
pub const EXT2F_INCOMPAT_COMP: u32 = 0x0001;
/// `EXT2F_INCOMPAT_FTYPE`.
pub const EXT2F_INCOMPAT_FTYPE: u32 = 0x0002;
/// `EXT2F_INCOMPAT_RECOVER`.
pub const EXT2F_INCOMPAT_RECOVER: u32 = 0x0004;
/// `EXT2F_INCOMPAT_JOURNAL_DEV`.
pub const EXT2F_INCOMPAT_JOURNAL_DEV: u32 = 0x0008;
/// `EXT2F_INCOMPAT_META_BG`.
pub const EXT2F_INCOMPAT_META_BG: u32 = 0x0010;
/// `EXT2F_INCOMPAT_EXTENTS`.
pub const EXT2F_INCOMPAT_EXTENTS: u32 = 0x0040;
/// `EXT2F_INCOMPAT_64BIT`.
pub const EXT2F_INCOMPAT_64BIT: u32 = 0x0080;
/// `EXT2F_INCOMPAT_MMP`.
pub const EXT2F_INCOMPAT_MMP: u32 = 0x0100;
/// `EXT2F_INCOMPAT_FLEX_BG`.
pub const EXT2F_INCOMPAT_FLEX_BG: u32 = 0x0200;
/// `EXT2F_INCOMPAT_EA_INODE`.
pub const EXT2F_INCOMPAT_EA_INODE: u32 = 0x0400;
/// `EXT2F_INCOMPAT_DIRDATA`.
pub const EXT2F_INCOMPAT_DIRDATA: u32 = 0x1000;
/// `EXT2F_INCOMPAT_CSUM_SEED`.
pub const EXT2F_INCOMPAT_CSUM_SEED: u32 = 0x2000;
/// `EXT2F_INCOMPAT_LARGEDIR`.
pub const EXT2F_INCOMPAT_LARGEDIR: u32 = 0x4000;
/// `EXT2F_INCOMPAT_INLINE_DATA`.
pub const EXT2F_INCOMPAT_INLINE_DATA: u32 = 0x8000;
/// `EXT2F_INCOMPAT_ENCRYPT`.
pub const EXT2F_INCOMPAT_ENCRYPT: u32 = 0x10000;

/// `struct ext2_feature`: a feature flag and its name.
#[derive(Clone, Copy, Debug)]
pub struct Ext2Feature {
    /// `mask`.
    pub mask: u32,
    /// `name`.
    pub name: &'static str,
}

/// `ro_compat[]`: the read-only compatible features and their names.
pub const RO_COMPAT: [Ext2Feature; 12] = [
    Ext2Feature {
        mask: EXT2F_ROCOMPAT_SPARSE_SUPER,
        name: "sparse_super",
    },
    Ext2Feature {
        mask: EXT2F_ROCOMPAT_LARGE_FILE,
        name: "large_file",
    },
    Ext2Feature {
        mask: EXT2F_ROCOMPAT_BTREE_DIR,
        name: "btree_dir",
    },
    Ext2Feature {
        mask: EXT2F_ROCOMPAT_HUGE_FILE,
        name: "huge_file",
    },
    Ext2Feature {
        mask: EXT2F_ROCOMPAT_GDT_CSUM,
        name: "uninit_bg",
    },
    Ext2Feature {
        mask: EXT2F_ROCOMPAT_DIR_NLINK,
        name: "dir_nlink",
    },
    Ext2Feature {
        mask: EXT2F_ROCOMPAT_EXTRA_ISIZE,
        name: "extra_isize",
    },
    Ext2Feature {
        mask: EXT2F_ROCOMPAT_QUOTA,
        name: "quota",
    },
    Ext2Feature {
        mask: EXT2F_ROCOMPAT_BIGALLOC,
        name: "bigalloc",
    },
    Ext2Feature {
        mask: EXT2F_ROCOMPAT_METADATA_CKSUM,
        name: "metadata_csum",
    },
    Ext2Feature {
        mask: EXT2F_ROCOMPAT_READONLY,
        name: "read-only",
    },
    Ext2Feature {
        mask: EXT2F_ROCOMPAT_PROJECT,
        name: "project",
    },
];

/// `incompat[]`: the incompatible features and their names.
pub const INCOMPAT: [Ext2Feature; 15] = [
    Ext2Feature {
        mask: EXT2F_INCOMPAT_COMP,
        name: "compression",
    },
    Ext2Feature {
        mask: EXT2F_INCOMPAT_FTYPE,
        name: "filetype",
    },
    Ext2Feature {
        mask: EXT2F_INCOMPAT_RECOVER,
        name: "needs_recovery",
    },
    Ext2Feature {
        mask: EXT2F_INCOMPAT_JOURNAL_DEV,
        name: "journal_dev",
    },
    Ext2Feature {
        mask: EXT2F_INCOMPAT_META_BG,
        name: "meta_bg",
    },
    Ext2Feature {
        mask: EXT2F_INCOMPAT_EXTENTS,
        name: "extents",
    },
    Ext2Feature {
        mask: EXT2F_INCOMPAT_64BIT,
        name: "64bit",
    },
    Ext2Feature {
        mask: EXT2F_INCOMPAT_MMP,
        name: "mmp",
    },
    Ext2Feature {
        mask: EXT2F_INCOMPAT_FLEX_BG,
        name: "flex_bg",
    },
    Ext2Feature {
        mask: EXT2F_INCOMPAT_EA_INODE,
        name: "ea_inode",
    },
    Ext2Feature {
        mask: EXT2F_INCOMPAT_DIRDATA,
        name: "dirdata",
    },
    Ext2Feature {
        mask: EXT2F_INCOMPAT_CSUM_SEED,
        name: "metadata_csum_seed",
    },
    Ext2Feature {
        mask: EXT2F_INCOMPAT_LARGEDIR,
        name: "large_dir",
    },
    Ext2Feature {
        mask: EXT2F_INCOMPAT_INLINE_DATA,
        name: "inline_data",
    },
    Ext2Feature {
        mask: EXT2F_INCOMPAT_ENCRYPT,
        name: "encrypt",
    },
];

/// `EXT2F_COMPAT_SUPP`: compatible features supported in this implementation.
pub const EXT2F_COMPAT_SUPP: u32 = 0x0000;
/// `EXT2F_ROCOMPAT_SUPP`: read-only compatible features supported in this implementation.
pub const EXT2F_ROCOMPAT_SUPP: u32 = EXT2F_ROCOMPAT_SPARSE_SUPER | EXT2F_ROCOMPAT_LARGE_FILE;
/// `EXT2F_INCOMPAT_SUPP`: incompatible features supported in this implementation.
pub const EXT2F_INCOMPAT_SUPP: u32 = EXT2F_INCOMPAT_FTYPE;
/// `EXT4F_RO_INCOMPAT_SUPP`: incompatible features supported when mounted read-only.
pub const EXT4F_RO_INCOMPAT_SUPP: u32 = EXT2F_INCOMPAT_EXTENTS
    | EXT2F_INCOMPAT_FLEX_BG
    | EXT2F_INCOMPAT_META_BG
    | EXT2F_INCOMPAT_RECOVER;

/// `E2FS_BEH_CONTINUE`: continue operation.
pub const E2FS_BEH_CONTINUE: u16 = 1;
/// `E2FS_BEH_READONLY`: remount the file system read only.
pub const E2FS_BEH_READONLY: u16 = 2;
/// `E2FS_BEH_PANIC`: cause a panic.
pub const E2FS_BEH_PANIC: u16 = 3;
/// `E2FS_BEH_DEFAULT`.
pub const E2FS_BEH_DEFAULT: u16 = E2FS_BEH_CONTINUE;

/// `E2FS_OS_LINUX`.
pub const E2FS_OS_LINUX: u32 = 0;
/// `E2FS_OS_HURD`.
pub const E2FS_OS_HURD: u32 = 1;
/// `E2FS_OS_MASIX`.
pub const E2FS_OS_MASIX: u32 = 2;

/// `E2FS_ISCLEAN`: the file system is clean.
pub const E2FS_ISCLEAN: u16 = 0x01;
/// `E2FS_ERRORS`: the file system has errors.
pub const E2FS_ERRORS: u16 = 0x02;

/// `struct ext2fs`: the super block of an ext2fs file system, as it is on disk (little-endian
/// on disk, native here after [`e2fs_sbload`]).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Ext2fs {
    /// `e2fs_icount`: inode count.
    pub e2fs_icount: u32,
    /// `e2fs_bcount`: blocks count.
    pub e2fs_bcount: u32,
    /// `e2fs_rbcount`: reserved blocks count.
    pub e2fs_rbcount: u32,
    /// `e2fs_fbcount`: free blocks count.
    pub e2fs_fbcount: u32,
    /// `e2fs_ficount`: free inodes count.
    pub e2fs_ficount: u32,
    /// `e2fs_first_dblock`: first data block.
    pub e2fs_first_dblock: u32,
    /// `e2fs_log_bsize`: block size = 1024*(2^e2fs_log_bsize).
    pub e2fs_log_bsize: u32,
    /// `e2fs_log_fsize`: fragment size log2.
    pub e2fs_log_fsize: u32,
    /// `e2fs_bpg`: blocks per group.
    pub e2fs_bpg: u32,
    /// `e2fs_fpg`: frags per group.
    pub e2fs_fpg: u32,
    /// `e2fs_ipg`: inodes per group.
    pub e2fs_ipg: u32,
    /// `e2fs_mtime`: mount time.
    pub e2fs_mtime: u32,
    /// `e2fs_wtime`: write time.
    pub e2fs_wtime: u32,
    /// `e2fs_mnt_count`: mount count.
    pub e2fs_mnt_count: u16,
    /// `e2fs_max_mnt_count`: max mount count.
    pub e2fs_max_mnt_count: u16,
    /// `e2fs_magic`: magic number.
    pub e2fs_magic: u16,
    /// `e2fs_state`: file system state.
    pub e2fs_state: u16,
    /// `e2fs_beh`: behavior on errors.
    pub e2fs_beh: u16,
    /// `e2fs_minrev`: minor revision level.
    pub e2fs_minrev: u16,
    /// `e2fs_lastfsck`: time of last fsck.
    pub e2fs_lastfsck: u32,
    /// `e2fs_fsckintv`: max time between fscks.
    pub e2fs_fsckintv: u32,
    /// `e2fs_creator`: creator OS.
    pub e2fs_creator: u32,
    /// `e2fs_rev`: revision level.
    pub e2fs_rev: u32,
    /// `e2fs_ruid`: default uid for reserved blocks.
    pub e2fs_ruid: u16,
    /// `e2fs_rgid`: default gid for reserved blocks.
    pub e2fs_rgid: u16,
    // EXT2_DYNAMIC_REV superblocks.
    /// `e2fs_first_ino`: first non-reserved inode.
    pub e2fs_first_ino: u32,
    /// `e2fs_inode_size`: size of inode structure.
    pub e2fs_inode_size: u16,
    /// `e2fs_block_group_nr`: block group number of this super block.
    pub e2fs_block_group_nr: u16,
    /// `e2fs_features_compat`: compatible feature set.
    pub e2fs_features_compat: u32,
    /// `e2fs_features_incompat`: incompatible feature set.
    pub e2fs_features_incompat: u32,
    /// `e2fs_features_rocompat`: RO-compatible feature set.
    pub e2fs_features_rocompat: u32,
    /// `e2fs_uuid`: 128-bit uuid for volume.
    pub e2fs_uuid: [u8; 16],
    /// `e2fs_vname`: volume name.
    pub e2fs_vname: [u8; 16],
    /// `e2fs_fsmnt`: name mounted on.
    pub e2fs_fsmnt: [u8; 64],
    /// `e2fs_algo`: for compression.
    pub e2fs_algo: u32,
    /// `e2fs_prealloc`: number of blocks to preallocate.
    pub e2fs_prealloc: u8,
    /// `e2fs_dir_prealloc`: number of blocks to preallocate for dir.
    pub e2fs_dir_prealloc: u8,
    /// `e2fs_reserved_ngdb`: number of reserved gd blocks for resize.
    pub e2fs_reserved_ngdb: u16,
    // Ext3 JBD2 journaling.
    /// `e2fs_journal_uuid`.
    pub e2fs_journal_uuid: [u8; 16],
    /// `e2fs_journal_ino`.
    pub e2fs_journal_ino: u32,
    /// `e2fs_journal_dev`.
    pub e2fs_journal_dev: u32,
    /// `e2fs_last_orphan`: start of list of inodes to delete.
    pub e2fs_last_orphan: u32,
    /// `e2fs_hash_seed`: htree hash seed.
    pub e2fs_hash_seed: [u32; 4],
    /// `e2fs_def_hash_version`.
    pub e2fs_def_hash_version: u8,
    /// `e2fs_journal_backup_type`.
    pub e2fs_journal_backup_type: u8,
    /// `e2fs_gdesc_size`.
    pub e2fs_gdesc_size: u16,
    /// `e2fs_default_mount_opts`.
    pub e2fs_default_mount_opts: u32,
    /// `e2fs_first_meta_bg`.
    pub e2fs_first_meta_bg: u32,
    /// `e2fs_mkfs_time`.
    pub e2fs_mkfs_time: u32,
    /// `e2fs_journal_backup`.
    pub e2fs_journal_backup: [u32; 17],
    /// `e2fs_bcount_hi`: high bits of blocks count.
    pub e2fs_bcount_hi: u32,
    /// `e2fs_rbcount_hi`: high bits of reserved blocks count.
    pub e2fs_rbcount_hi: u32,
    /// `e2fs_fbcount_hi`: high bits of free blocks count.
    pub e2fs_fbcount_hi: u32,
    /// `e2fs_min_extra_isize`: all inodes have some bytes.
    pub e2fs_min_extra_isize: u16,
    /// `e2fs_want_extra_isize`: inodes must reserve some bytes.
    pub e2fs_want_extra_isize: u16,
    /// `e2fs_flags`: miscellaneous flags.
    pub e2fs_flags: u32,
    /// `e2fs_raid_stride`: RAID stride.
    pub e2fs_raid_stride: u16,
    /// `e2fs_mmpintv`: seconds to wait in MMP checking.
    pub e2fs_mmpintv: u16,
    /// `e2fs_mmpblk`: block for multi-mount protection.
    pub e2fs_mmpblk: u64,
    /// `e2fs_raid_stripe_wid`: blocks on data disks (N * stride).
    pub e2fs_raid_stripe_wid: u32,
    /// `e2fs_log_gpf`: FLEX_BG group size.
    pub e2fs_log_gpf: u8,
    /// `e2fs_chksum_type`: metadata checksum algorithm used.
    pub e2fs_chksum_type: u8,
    /// `e2fs_encrypt`: versioning level for encryption.
    pub e2fs_encrypt: u8,
    /// `e2fs_reserved_pad`.
    pub e2fs_reserved_pad: u8,
    /// `e2fs_kbytes_written`: number of lifetime kilobytes.
    pub e2fs_kbytes_written: u64,
    /// `e2fs_snapinum`: inode number of active snapshot.
    pub e2fs_snapinum: u32,
    /// `e2fs_snapid`: sequential ID of active snapshot.
    pub e2fs_snapid: u32,
    /// `e2fs_snaprbcount`: reserved blocks for active snapshot.
    pub e2fs_snaprbcount: u64,
    /// `e2fs_snaplist`: inode number for on-disk snapshot.
    pub e2fs_snaplist: u32,
    /// `e2fs_errcount`: number of file system errors.
    pub e2fs_errcount: u32,
    /// `e2fs_first_errtime`: first time an error happened.
    pub e2fs_first_errtime: u32,
    /// `e2fs_first_errino`: inode involved in first error.
    pub e2fs_first_errino: u32,
    /// `e2fs_first_errblk`: block involved of first error.
    pub e2fs_first_errblk: u64,
    /// `e2fs_first_errfunc`: function where error happened.
    pub e2fs_first_errfunc: [u8; 32],
    /// `e2fs_first_errline`: line number where error happened.
    pub e2fs_first_errline: u32,
    /// `e2fs_last_errtime`: most recent time of an error.
    pub e2fs_last_errtime: u32,
    /// `e2fs_last_errino`: inode involved in last error.
    pub e2fs_last_errino: u32,
    /// `e2fs_last_errline`: line number where error happened.
    pub e2fs_last_errline: u32,
    /// `e2fs_last_errblk`: block involved of last error.
    pub e2fs_last_errblk: u64,
    /// `e2fs_last_errfunc`: function where error happened.
    pub e2fs_last_errfunc: [u8; 32],
    /// `e2fs_mount_opts`.
    pub e2fs_mount_opts: [u8; 64],
    /// `e2fs_usrquota_inum`: inode for tracking user quota.
    pub e2fs_usrquota_inum: u32,
    /// `e2fs_grpquota_inum`: inode for tracking group quota.
    pub e2fs_grpquota_inum: u32,
    /// `e2fs_overhead_clusters`: overhead blocks/clusters.
    pub e2fs_overhead_clusters: u32,
    /// `e2fs_backup_bgs`: groups with sparse_super2 super blocks.
    pub e2fs_backup_bgs: [u32; 2],
    /// `e2fs_encrypt_algos`: encryption algorithms in use.
    pub e2fs_encrypt_algos: [u8; 4],
    /// `e2fs_encrypt_pw_salt`: salt used for string2key.
    pub e2fs_encrypt_pw_salt: [u8; 16],
    /// `e2fs_lpf_ino`: location of the lost+found inode.
    pub e2fs_lpf_ino: u32,
    /// `e2fs_proj_quota_inum`: inode for tracking project quota.
    pub e2fs_proj_quota_inum: u32,
    /// `e2fs_chksum_seed`: checksum seed.
    pub e2fs_chksum_seed: u32,
    /// `e2fs_reserved`: padding to the end of the block.
    pub e2fs_reserved: [u32; 98],
    /// `e2fs_sbchksum`: super block checksum.
    pub e2fs_sbchksum: u32,
}

impl Ext2fs {
    /// A zeroed super block.
    pub const fn new() -> Self {
        // SAFETY: `Ext2fs` is integers and arrays of integers only; all-zero is a valid value.
        unsafe { core::mem::zeroed() }
    }

    /// The super block in the first `SBSIZE` bytes of `bytes`, little-endian as on disk, in
    /// native byte order (`e2fs_sbload`).
    pub fn from_disk(bytes: &[u8]) -> Self {
        let mut sb = Self::new();
        e2fs_sbload(bytes, &mut sb);
        sb
    }
}

impl Default for Ext2fs {
    fn default() -> Self {
        Self::new()
    }
}

/// `struct ext2_gd`: an ext2 file system block group descriptor.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ext2Gd {
    /// `ext2bgd_b_bitmap`: blocks bitmap block.
    pub ext2bgd_b_bitmap: u32,
    /// `ext2bgd_i_bitmap`: inodes bitmap block.
    pub ext2bgd_i_bitmap: u32,
    /// `ext2bgd_i_tables`: inodes table block.
    pub ext2bgd_i_tables: u32,
    /// `ext2bgd_nbfree`: number of free blocks.
    pub ext2bgd_nbfree: u16,
    /// `ext2bgd_nifree`: number of free inodes.
    pub ext2bgd_nifree: u16,
    /// `ext2bgd_ndirs`: number of directories.
    pub ext2bgd_ndirs: u16,
    /// `reserved`.
    pub reserved: u16,
    /// `reserved2`.
    pub reserved2: [u32; 3],
}

impl Ext2Gd {
    /// The descriptor in 32 little-endian bytes, as it is on disk.
    pub fn from_le_bytes(b: &[u8]) -> Self {
        let u32_at = |o: usize| u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
        let u16_at = |o: usize| u16::from_le_bytes([b[o], b[o + 1]]);
        Self {
            ext2bgd_b_bitmap: u32_at(0),
            ext2bgd_i_bitmap: u32_at(4),
            ext2bgd_i_tables: u32_at(8),
            ext2bgd_nbfree: u16_at(12),
            ext2bgd_nifree: u16_at(14),
            ext2bgd_ndirs: u16_at(16),
            reserved: u16_at(18),
            reserved2: [u32_at(20), u32_at(24), u32_at(28)],
        }
    }

    /// The descriptor as the 32 little-endian bytes it is on disk.
    pub fn to_le_bytes(&self) -> [u8; EXT2_GD_SIZE] {
        let mut b = [0u8; EXT2_GD_SIZE];
        b[0..4].copy_from_slice(&self.ext2bgd_b_bitmap.to_le_bytes());
        b[4..8].copy_from_slice(&self.ext2bgd_i_bitmap.to_le_bytes());
        b[8..12].copy_from_slice(&self.ext2bgd_i_tables.to_le_bytes());
        b[12..14].copy_from_slice(&self.ext2bgd_nbfree.to_le_bytes());
        b[14..16].copy_from_slice(&self.ext2bgd_nifree.to_le_bytes());
        b[16..18].copy_from_slice(&self.ext2bgd_ndirs.to_le_bytes());
        b[18..20].copy_from_slice(&self.reserved.to_le_bytes());
        for (i, r) in self.reserved2.iter().enumerate() {
            b[20 + 4 * i..24 + 4 * i].copy_from_slice(&r.to_le_bytes());
        }
        b
    }
}

/// `sizeof(struct ext2_gd)`.
pub const EXT2_GD_SIZE: usize = size_of::<Ext2Gd>();

/// Generates the getter `$f()` and the setter `set_$f()` of a scalar member of the in-core
/// super block.
macro_rules! sb_scalar {
    ($($(#[$doc:meta])* $f:ident, $set:ident: $t:ty;)*) => {
        $(
            $(#[$doc])*
            pub fn $f(&self) -> $t {
                self.with_e2fs(|e| e.$f)
            }

            $(#[$doc])*
            pub fn $set(&self, v: $t) {
                self.with_e2fs_mut(|e| e.$f = v)
            }
        )*
    };
}

/// `struct m_ext2fs`: the in-memory data for ext2fs, the `malloc`ed in-core super block, reached
/// as `&'static MExt2fs`.
///
/// Protected by: the kernel lock; the members are changed while mounting, unmounting and
/// allocating, as in C.
pub struct MExt2fs {
    /// `e2fs`: the super block, see [`MExt2fs::with_e2fs`].
    e2fs: UnsafeCell<Ext2fs>,
    /// `e2fs_fsmnt`: name mounted on.
    pub e2fs_fsmnt: Cell<[u8; MAXMNTLEN]>,
    /// `e2fs_ronly`: mounted read-only flag.
    pub e2fs_ronly: Cell<i8>,
    /// `e2fs_fmod`: super block modified flag.
    pub e2fs_fmod: Cell<i8>,
    /// `e2fs_fsize`: fragment size.
    pub e2fs_fsize: Cell<i32>,
    /// `e2fs_bsize`: block size.
    pub e2fs_bsize: Cell<i32>,
    /// `e2fs_bshift`: `lblkno` calc of logical blkno.
    pub e2fs_bshift: Cell<i32>,
    /// `e2fs_bmask`: `blkoff` calc of blk offsets.
    pub e2fs_bmask: Cell<i32>,
    /// `e2fs_qbmask`: `~fs_bmask`, for use with quad size.
    pub e2fs_qbmask: Cell<i64>,
    /// `e2fs_fsbtodb`: `fsbtodb` and `dbtofsb` shift constant.
    pub e2fs_fsbtodb: Cell<i32>,
    /// `e2fs_ncg`: number of cylinder groups.
    pub e2fs_ncg: Cell<i32>,
    /// `e2fs_ngdb`: number of group descriptor blocks.
    pub e2fs_ngdb: Cell<i32>,
    /// `e2fs_ipb`: number of inodes per block.
    pub e2fs_ipb: Cell<i32>,
    /// `e2fs_itpg`: number of inode tables per group.
    pub e2fs_itpg: Cell<i32>,
    /// `e2fs_maxfilesize`: depends on LARGE/HUGE flags.
    pub e2fs_maxfilesize: Cell<Off>,
    /// `e2fs_gd`: group descriptors, `e2fs_ncg` of them in `malloc`ed memory (NULL until
    /// `ext2fs_mountfs` allocates them).
    pub e2fs_gd: Cell<*mut Ext2Gd>,
}

// SAFETY: the members are changed under the kernel lock, as in C.
unsafe impl Sync for MExt2fs {}

impl MExt2fs {
    /// A zeroed structure, as `malloc(M_ZERO)` returns it.
    pub const fn new() -> Self {
        Self {
            e2fs: UnsafeCell::new(Ext2fs::new()),
            e2fs_fsmnt: Cell::new([0; MAXMNTLEN]),
            e2fs_ronly: Cell::new(0),
            e2fs_fmod: Cell::new(0),
            e2fs_fsize: Cell::new(0),
            e2fs_bsize: Cell::new(0),
            e2fs_bshift: Cell::new(0),
            e2fs_bmask: Cell::new(0),
            e2fs_qbmask: Cell::new(0),
            e2fs_fsbtodb: Cell::new(0),
            e2fs_ncg: Cell::new(0),
            e2fs_ngdb: Cell::new(0),
            e2fs_ipb: Cell::new(0),
            e2fs_itpg: Cell::new(0),
            e2fs_maxfilesize: Cell::new(0),
            e2fs_gd: Cell::new(ptr::null_mut()),
        }
    }

    /// `f(&fs->e2fs)`: the super block, for the duration of `f`.
    pub fn with_e2fs<R>(&self, f: impl FnOnce(&Ext2fs) -> R) -> R {
        // SAFETY: the reference does not outlive `f`; the only writers are `with_e2fs_mut`
        // and `set_e2fs`, which run to completion under the kernel lock, and none
        // of these nests inside `f` (the accessors take and return plain values).
        f(unsafe { &*self.e2fs.get() })
    }

    /// `f(&mut fs->e2fs)`: the super block, for the duration of `f`.
    pub fn with_e2fs_mut<R>(&self, f: impl FnOnce(&mut Ext2fs) -> R) -> R {
        // SAFETY: as in `with_e2fs`: no other reference to the super block is alive while
        // `f` runs, because the accessors never hand one out and do not nest.
        f(unsafe { &mut *self.e2fs.get() })
    }

    /// A copy of the super block (1024 bytes: keep it off the stack in hot paths).
    pub fn e2fs_copy(&self) -> Ext2fs {
        self.with_e2fs(|e| *e)
    }

    /// `fs->e2fs = *sb`.
    pub fn set_e2fs(&self, sb: &Ext2fs) {
        self.with_e2fs_mut(|e| *e = *sb)
    }

    /// `fs->e2fs.e2fs_fsmnt` or `fs->e2fs_fsmnt`: a copy of the name mounted on.
    pub fn fsmnt(&self) -> [u8; MAXMNTLEN] {
        self.e2fs_fsmnt.get()
    }

    /// `fs->e2fs_fsmnt` as a C string (up to its NUL), for messages.
    pub fn fsmnt_str(&self) -> crate::kern::subr_prf::Str<'_> {
        let name = self.e2fs_fsmnt.as_ptr();
        // SAFETY: a `Cell<[u8; N]>` has the array's layout; the borrow lasts as long as the
        // caller's `&self`, during which nothing writes the name (both run under the kernel lock,
        // and printing a message does not mount).
        crate::kern::subr_prf::Str(unsafe { &*name })
    }

    /// `&fs->e2fs_gd[i]`, copied; panics beyond `e2fs_ncg` descriptors.
    pub fn gd(&self, i: usize) -> Ext2Gd {
        // SAFETY: `gd_ptr` is in bounds of the `malloc`ed descriptors (`e2fs_ncg` of them).
        unsafe { self.gd_ptr(i).read() }
    }

    /// `fs->e2fs_gd[i] = gd`; panics beyond `e2fs_ncg` descriptors.
    pub fn set_gd(&self, i: usize, gd: Ext2Gd) {
        // SAFETY: as in `gd`.
        unsafe { self.gd_ptr(i).write(gd) }
    }

    /// `f(&mut fs->e2fs_gd[i])`: a descriptor, for the duration of `f`.
    pub fn with_gd_mut<R>(&self, i: usize, f: impl FnOnce(&mut Ext2Gd) -> R) -> R {
        let p = self.gd_ptr(i);
        // SAFETY: as in `gd`; the reference does not outlive `f` and no other one is alive
        // meanwhile (the accessors copy, none nests, all run under the kernel lock).
        f(unsafe { &mut *p })
    }

    /// The pointer to descriptor `i`, after the checks that make dereferencing it sound.
    fn gd_ptr(&self, i: usize) -> *mut Ext2Gd {
        let gd = self.e2fs_gd.get();
        if gd.is_null() || i >= self.e2fs_ncg.get().max(0) as usize {
            panic(format_args!(
                "m_ext2fs {:p}: group descriptor {} out of {} (e2fs_gd {:p})",
                self,
                i,
                self.e2fs_ncg.get(),
                gd
            ));
        }
        gd.wrapping_add(i)
    }

    /// `EXT2_DINODE_SIZE(fs)`: the size of an on-disk inode.
    pub fn dinode_size(&self) -> usize {
        self.with_e2fs(|e| {
            if e.e2fs_rev > E2FS_REV0 {
                usize::from(e.e2fs_inode_size)
            } else {
                crate::ufs::ext2fs::ext2fs_dinode::EXT2_REV0_DINODE_SIZE
            }
        })
    }

    sb_scalar! {
        /// `fs->e2fs.e2fs_icount`.
        e2fs_icount, set_e2fs_icount: u32;
        /// `fs->e2fs.e2fs_bcount`.
        e2fs_bcount, set_e2fs_bcount: u32;
        /// `fs->e2fs.e2fs_rbcount`.
        e2fs_rbcount, set_e2fs_rbcount: u32;
        /// `fs->e2fs.e2fs_fbcount`.
        e2fs_fbcount, set_e2fs_fbcount: u32;
        /// `fs->e2fs.e2fs_ficount`.
        e2fs_ficount, set_e2fs_ficount: u32;
        /// `fs->e2fs.e2fs_first_dblock`.
        e2fs_first_dblock, set_e2fs_first_dblock: u32;
        /// `fs->e2fs.e2fs_log_bsize`.
        e2fs_log_bsize, set_e2fs_log_bsize: u32;
        /// `fs->e2fs.e2fs_log_fsize`.
        e2fs_log_fsize, set_e2fs_log_fsize: u32;
        /// `fs->e2fs.e2fs_bpg`.
        e2fs_bpg, set_e2fs_bpg: u32;
        /// `fs->e2fs.e2fs_fpg`.
        e2fs_fpg, set_e2fs_fpg: u32;
        /// `fs->e2fs.e2fs_ipg`.
        e2fs_ipg, set_e2fs_ipg: u32;
        /// `fs->e2fs.e2fs_mtime`.
        e2fs_mtime, set_e2fs_mtime: u32;
        /// `fs->e2fs.e2fs_wtime`.
        e2fs_wtime, set_e2fs_wtime: u32;
        /// `fs->e2fs.e2fs_mnt_count`.
        e2fs_mnt_count, set_e2fs_mnt_count: u16;
        /// `fs->e2fs.e2fs_max_mnt_count`.
        e2fs_max_mnt_count, set_e2fs_max_mnt_count: u16;
        /// `fs->e2fs.e2fs_magic`.
        e2fs_magic, set_e2fs_magic: u16;
        /// `fs->e2fs.e2fs_state`.
        e2fs_state, set_e2fs_state: u16;
        /// `fs->e2fs.e2fs_beh`.
        e2fs_beh, set_e2fs_beh: u16;
        /// `fs->e2fs.e2fs_minrev`.
        e2fs_minrev, set_e2fs_minrev: u16;
        /// `fs->e2fs.e2fs_lastfsck`.
        e2fs_lastfsck, set_e2fs_lastfsck: u32;
        /// `fs->e2fs.e2fs_fsckintv`.
        e2fs_fsckintv, set_e2fs_fsckintv: u32;
        /// `fs->e2fs.e2fs_creator`.
        e2fs_creator, set_e2fs_creator: u32;
        /// `fs->e2fs.e2fs_rev`.
        e2fs_rev, set_e2fs_rev: u32;
        /// `fs->e2fs.e2fs_ruid`.
        e2fs_ruid, set_e2fs_ruid: u16;
        /// `fs->e2fs.e2fs_rgid`.
        e2fs_rgid, set_e2fs_rgid: u16;
        /// `fs->e2fs.e2fs_first_ino`.
        e2fs_first_ino, set_e2fs_first_ino: u32;
        /// `fs->e2fs.e2fs_inode_size`.
        e2fs_inode_size, set_e2fs_inode_size: u16;
        /// `fs->e2fs.e2fs_block_group_nr`.
        e2fs_block_group_nr, set_e2fs_block_group_nr: u16;
        /// `fs->e2fs.e2fs_features_compat`.
        e2fs_features_compat, set_e2fs_features_compat: u32;
        /// `fs->e2fs.e2fs_features_incompat`.
        e2fs_features_incompat, set_e2fs_features_incompat: u32;
        /// `fs->e2fs.e2fs_features_rocompat`.
        e2fs_features_rocompat, set_e2fs_features_rocompat: u32;
        /// `fs->e2fs.e2fs_reserved_ngdb`.
        e2fs_reserved_ngdb, set_e2fs_reserved_ngdb: u16;
        /// `fs->e2fs.e2fs_first_meta_bg`.
        e2fs_first_meta_bg, set_e2fs_first_meta_bg: u32;
        /// `fs->e2fs.e2fs_gdesc_size`.
        e2fs_gdesc_size, set_e2fs_gdesc_size: u16;
        /// `fs->e2fs.e2fs_flags`.
        e2fs_flags, set_e2fs_flags: u32;
    }
}

impl Default for MExt2fs {
    fn default() -> Self {
        Self::new()
    }
}

/// `e2fs_overflow(fs, lower, value)`: whether `value` is outside `lower..=e2fs_maxfilesize`.
pub fn e2fs_overflow(fs: &MExt2fs, lower: Off, value: Off) -> bool {
    value < lower || value > fs.e2fs_maxfilesize.get()
}

/// `cg_has_sb(i)`: if the `EXT2F_ROCOMPAT_SPARSE_SUPER` flag is set, the cylinder group has a
/// copy of the super and cylinder group descriptors blocks only if it is a power of 3, 5 or 7.
pub fn cg_has_sb(i: i32) -> bool {
    if i == 0 || i == 1 {
        return true;
    }
    let i = i64::from(i);
    let (mut a3, mut a5, mut a7) = (3i64, 5i64, 7i64);
    while a3 <= i || a5 <= i || a7 <= i {
        if i == a3 || i == a5 || i == a7 {
            return true;
        }
        a3 *= 3;
        a5 *= 5;
        a7 *= 7;
    }
    false
}

/// `e2fs_sbload(old, new)`: loads the super block from the first `SBSIZE` bytes of `old`
/// (little-endian on disk).
pub fn e2fs_sbload(old: &[u8], new: &mut Ext2fs) {
    let Some(src) = old.get(..SBSIZE) else {
        panic(format_args!(
            "e2fs_sbload: {} bytes, want {}",
            old.len(),
            SBSIZE
        ));
    };
    // SAFETY: `src` is `SBSIZE` = `size_of::<Ext2fs>()` bytes (checked by the compile-time
    // assertion below), `Ext2fs` is integers only so every bit pattern is valid, and
    // `read_unaligned` needs no alignment.
    let disk = unsafe { ptr::read_unaligned(src.as_ptr().cast::<Ext2fs>()) };
    if cfg!(target_endian = "big") {
        crate::ufs::ext2fs::ext2fs_bswap::e2fs_sb_bswap(&disk, new);
    } else {
        *new = disk;
    }
}

/// `e2fs_sbsave(old, new)`: stores the super block `old` in the first `SBSIZE` bytes of `new`
/// (little-endian on disk).
pub fn e2fs_sbsave(old: &Ext2fs, new: &mut [u8]) {
    let Some(dst) = new.get_mut(..SBSIZE) else {
        panic(format_args!(
            "e2fs_sbsave: {} bytes, want {}",
            new.len(),
            SBSIZE
        ));
    };
    let mut disk = *old;
    if cfg!(target_endian = "big") {
        crate::ufs::ext2fs::ext2fs_bswap::e2fs_sb_bswap(old, &mut disk);
    }
    // SAFETY: `dst` is `SBSIZE` = `size_of::<Ext2fs>()` bytes (checked below) and
    // `write_unaligned` needs no alignment.
    unsafe { ptr::write_unaligned(dst.as_mut_ptr().cast::<Ext2fs>(), disk) }
}

/// `e2fs_cgload(old, new, size)`: loads `size` bytes of group descriptors (little-endian on
/// disk) from `old` into `new`.
pub fn e2fs_cgload(old: &[u8], new: &mut [Ext2Gd], size: usize) {
    if old.len() < size || new.len() * EXT2_GD_SIZE < size {
        panic(format_args!(
            "e2fs_cgload: {} bytes into {} descriptors from {} bytes",
            size,
            new.len(),
            old.len()
        ));
    }
    let whole = size / EXT2_GD_SIZE;
    for (gd, b) in new.iter_mut().zip(old.chunks(EXT2_GD_SIZE)).take(whole) {
        *gd = Ext2Gd::from_le_bytes(b);
    }
    let rest = size % EXT2_GD_SIZE;
    if rest > 0 {
        // The C copies the `size` bytes as they are: a partial descriptor is the first bytes
        // of the next one's little-endian image.
        let mut img = new[whole].to_le_bytes();
        img[..rest].copy_from_slice(&old[whole * EXT2_GD_SIZE..size]);
        new[whole] = Ext2Gd::from_le_bytes(&img);
    }
}

/// `e2fs_cgsave(old, new, size)`: stores `size` bytes of group descriptors from `old` into `new`
/// (little-endian on disk).
pub fn e2fs_cgsave(old: &[Ext2Gd], new: &mut [u8], size: usize) {
    if new.len() < size || old.len() * EXT2_GD_SIZE < size {
        panic(format_args!(
            "e2fs_cgsave: {} bytes from {} descriptors into {} bytes",
            size,
            old.len(),
            new.len()
        ));
    }
    let whole = size / EXT2_GD_SIZE;
    for (gd, b) in old.iter().zip(new.chunks_mut(EXT2_GD_SIZE)).take(whole) {
        b.copy_from_slice(&gd.to_le_bytes());
    }
    let rest = size % EXT2_GD_SIZE;
    if rest > 0 {
        new[whole * EXT2_GD_SIZE..size].copy_from_slice(&old[whole].to_le_bytes()[..rest]);
    }
}

/// `fsbtodb(fs, b)`: turns a file system block number into a disk block address (maps file
/// system blocks to device size blocks).
pub fn fsbtodb(fs: &MExt2fs, b: Daddr) -> Daddr {
    b << fs.e2fs_fsbtodb.get()
}

/// `dbtofsb(fs, b)`: the inverse of [`fsbtodb`].
pub fn dbtofsb(fs: &MExt2fs, b: Daddr) -> Daddr {
    b >> fs.e2fs_fsbtodb.get()
}

/// `ino_to_cg(fs, x)`: inode number to cylinder group number.
pub fn ino_to_cg(fs: &MExt2fs, x: Ufsino) -> u32 {
    x.wrapping_sub(1) / fs.e2fs_ipg()
}

/// `ino_to_fsba(fs, x)`: inode number to file system block address.
pub fn ino_to_fsba(fs: &MExt2fs, x: Ufsino) -> u32 {
    fs.gd(ino_to_cg(fs, x) as usize)
        .ext2bgd_i_tables
        .wrapping_add((x.wrapping_sub(1) % fs.e2fs_ipg()) / fs.e2fs_ipb.get() as u32)
}

/// `ino_to_fsbo(fs, x)`: inode number to file system block offset.
pub fn ino_to_fsbo(fs: &MExt2fs, x: Ufsino) -> u32 {
    x.wrapping_sub(1) % fs.e2fs_ipb.get() as u32
}

/// `dtog(fs, d)`: the cylinder group number for a file system block.
pub fn dtog(fs: &MExt2fs, d: u32) -> u32 {
    d.wrapping_sub(fs.e2fs_first_dblock()) / fs.e2fs_fpg()
}

/// `dtogd(fs, d)`: the cylinder group block number for a file system block.
pub fn dtogd(fs: &MExt2fs, d: u32) -> u32 {
    d.wrapping_sub(fs.e2fs_first_dblock()) % fs.e2fs_fpg()
}

/// `blkoff(fs, loc)`: calculates `loc % fs->e2fs_bsize`.
pub fn blkoff(fs: &MExt2fs, loc: Off) -> Off {
    loc & fs.e2fs_qbmask.get()
}

/// `lblktosize(fs, blk)`: calculates `blk * fs->e2fs_bsize`.
pub fn lblktosize(fs: &MExt2fs, blk: Daddr) -> Off {
    blk << fs.e2fs_bshift.get()
}

/// `lblkno(fs, loc)`: calculates `loc / fs->e2fs_bsize`.
pub fn lblkno(fs: &MExt2fs, loc: Off) -> Daddr {
    loc >> fs.e2fs_bshift.get()
}

/// `blkroundup(fs, size)`: calculates `roundup(size, fs->e2fs_bsize)`.
pub fn blkroundup(fs: &MExt2fs, size: Off) -> Off {
    (size + fs.e2fs_qbmask.get()) & i64::from(fs.e2fs_bmask.get())
}

/// `fragroundup(fs, size)`: calculates `roundup(size, fs->e2fs_bsize)` (as the C does: there
/// are no fragments in ext2fs).
pub fn fragroundup(fs: &MExt2fs, size: Off) -> Off {
    (size + fs.e2fs_qbmask.get()) & i64::from(fs.e2fs_bmask.get())
}

/// `freespace(fs)`: the number of available blocks given the ones held in reserve.
pub fn freespace(fs: &MExt2fs) -> u32 {
    fs.e2fs_fbcount().wrapping_sub(fs.e2fs_rbcount())
}

/// `NINDIR(fs)`: the number of indirects in a file system block.
pub fn nindir(fs: &MExt2fs) -> i64 {
    i64::from(fs.e2fs_bsize.get()) / size_of::<u32>() as i64
}

const _: () = {
    assert!(size_of::<Ext2fs>() == SBSIZE);
    assert!(size_of::<Ext2Gd>() == 32);
    assert!(core::mem::offset_of!(Ext2fs, e2fs_magic) == 56);
    assert!(core::mem::offset_of!(Ext2fs, e2fs_rev) == 76);
    assert!(core::mem::offset_of!(Ext2fs, e2fs_features_compat) == 92);
    assert!(core::mem::offset_of!(Ext2fs, e2fs_uuid) == 104);
    assert!(core::mem::offset_of!(Ext2fs, e2fs_journal_uuid) == 208);
    assert!(core::mem::offset_of!(Ext2fs, e2fs_bcount_hi) == 336);
    assert!(core::mem::offset_of!(Ext2fs, e2fs_mmpblk) == 360);
    assert!(core::mem::offset_of!(Ext2fs, e2fs_kbytes_written) == 376);
    assert!(core::mem::offset_of!(Ext2fs, e2fs_chksum_seed) == 624);
    assert!(core::mem::offset_of!(Ext2fs, e2fs_sbchksum) == 1020);
};

#[cfg(test)]
mod tests;
