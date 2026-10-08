/*	$OpenBSD: denode.h,v 1.36 2022/08/15 01:47:09 jsg Exp $	*/
/*	$NetBSD: denode.h,v 1.24 1997/10/17 11:23:39 ws Exp $	*/
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
//! `<msdosfs/denode.h>`: the pc filesystem specific portion of the vnode structure (`struct
//! denode`), its fat cache, the transfer of directory entries between the internal and the
//! external form, and the msdosfs file handle.
//!
//! Upstream: sys/msdosfs/denode.h @ 3ce1f3f79392
//!
//! To describe a file uniquely the `de_dirclust`, `de_diroffset`, and `de_StartCluster` fields
//! are used. `de_dirclust` contains the cluster number of the directory cluster containing the
//! entry for a file or directory. `de_diroffset` is the index into the cluster for the entry
//! describing a file or directory. `de_StartCluster` is the number of the first cluster of the
//! file or directory.
//!
//! Now to describe the quirks of the pc filesystem.
//! - Clusters 0 and 1 are reserved.
//! - The first allocatable cluster is 2.
//! - The root directory is of fixed size and all blocks that make it up are contiguous.
//! - Cluster 0 refers to the root directory when it is found in the startcluster field of a
//!   directory entry that points to another directory.
//! - Cluster 0 implies a 0 length file when found in the start cluster field of a directory
//!   entry that points to a file.
//! - You can't use the cluster number 0 to derive the address of the root directory.
//! - Multiple directory entries can point to a directory. The entry in the parent directory
//!   points to a child directory. Any directories in the child directory contain a ".." entry
//!   that points back to the parent. The child directory itself contains a "." entry that
//!   points to itself.
//! - The root directory does not contain a "." or ".." entry.
//! - Directory entries for directories are never changed once they are created (except when
//!   removed). The size stays 0, and the last modification time is never changed. This is
//!   because so many directory entries can point to the physical clusters that make up a
//!   directory. It would lead to an update nightmare.
//! - The length field in a directory entry pointing to a directory contains 0 (always). The
//!   only way to find the end of a directory is to follow the cluster chain until the "last
//!   cluster" marker is found.
//!
//! My extensions to make this house of cards work. These apply only to the in memory copy of
//! the directory entry.
//! - A reference count for each denode will be kept since dos doesn't keep such things.
//!
//! ## Deviations
//! - A denode is `malloc(M_MSDOSFSNODE)`ed by `deget`, hung from its vnode's `v_data`, and
//!   freed by `msdosfs_reclaim`; it is reached as `&'static Denode` through [`vtode`] (the C's
//!   `VTODE`), whose one `unsafe` checks the vnode's tag. The members are `Cell`s: the C
//!   changes them through shared pointers under the denode's lock (`de_lock`). The vnode
//!   operations also build one on the stack (`struct denode ndirent` in `msdosfs_create` and
//!   `msdosfs_mkdir`), which [`Denode::new`] allows.
//! - The hand-made hash chain (`de_next`, and `de_prev` pointing at the previous link) is a
//!   `queue.h` list entry, `de_hash` (the [`DeHash`] adapter); `de_prev == NULL` (a denode on
//!   no chain) is the `de_hashed` flag, set while the denode is linked (see
//!   `msdosfs_denode.rs`).
//! - `de_lockf` is a `LockfStateSlot` (`sys/lockf.rs`).
//! - `de_Name` is a `Cell<[u8; 11]>`; `de_fc` is an array of `Cell<Fatcache>`.
//! - `fc_setcache`, `DE_INTERNALIZE32`, `DE_INTERNALIZE`, `DE_EXTERNALIZE`, `DETIMES`, `VTODE`
//!   are functions with the macros' names in lower case; `DETOV` is [`Denode::detov`]. The
//!   on-disk entry is a `Direntry` view over the directory block.
//! - `de_forw`/`de_back` name a `de_chain` member the structure does not have; they are dead
//!   macros and are left out.
//! - The vnode operation prototypes (`msdosfs_lookup`, ...) are the functions of
//!   `msdosfs_vnops.rs`, except `msdosfs_inactive` and `msdosfs_reclaim`, which the C defines
//!   in `msdosfs_denode.c`; the internal service routines are in `msdosfs_denode.rs`
//!   (`deget`, `deupdat`, `detrunc`, `deextend`, `reinsert`) and `msdosfs_lookup.rs` (the
//!   rest).

use core::cell::Cell;

use crate::kern::subr_prf::panic;
use crate::msdosfs::bpb::{getulong, getushort, putulong, putushort};
use crate::msdosfs::direntry::{
    ATTR_ARCHIVE, ATTR_DIRECTORY, CASE_LOWER_BASE, CASE_LOWER_EXT, Direntry,
};
use crate::msdosfs::fat::fat32;
use crate::msdosfs::msdosfs_conv::unix2dostime;
use crate::msdosfs::msdosfsmount::Msdosfsmount;
use crate::queue_adapter;
use crate::sys::lockf::LockfStateSlot;
use crate::sys::mount::{Fid, MSDOSFSMNT_NOWIN95};
use crate::sys::queue::ListEntry;
use crate::sys::rwlock::Rrwlock;
use crate::sys::time::Timespec;
use crate::sys::types::Dev;
use crate::sys::vnode::{VT_MSDOSFS, Vnode};

/// `MSDOSFSROOT_OFS`: internal pseudo-offset for (nonexistent) directory entry for the root
/// dir in the root dir.
pub const MSDOSFSROOT_OFS: u32 = 0x1fffffff;

/// `FC_SIZE`: number of entries in the cache.
pub const FC_SIZE: usize = 3;
/// `FC_LASTMAP`: entry the last call to `pcbmap()` resolved to.
pub const FC_LASTMAP: usize = 0;
/// `FC_LASTFC`: entry for the last cluster in the file.
pub const FC_LASTFC: usize = 1;
/// `FC_OLASTFC`: entry for the previous last cluster.
pub const FC_OLASTFC: usize = 2;

/// `FCE_EMPTY`: doesn't represent an actual cluster #.
pub const FCE_EMPTY: u32 = 0xffffffff;

/// `DE_UPDATE`: modification time update request.
pub const DE_UPDATE: u32 = 0x0004;
/// `DE_CREATE`: creation time update.
pub const DE_CREATE: u32 = 0x0008;
/// `DE_ACCESS`: access time update.
pub const DE_ACCESS: u32 = 0x0010;
/// `DE_MODIFIED`: denode has been modified.
pub const DE_MODIFIED: u32 = 0x0020;
/// `DE_RENAME`: denode is in the process of being renamed.
pub const DE_RENAME: u32 = 0x0040;

/// `WIN_MAXLEN`: maximum filename length in Win95. Note: must be < `sizeof(dirent.d_name)`.
pub const WIN_MAXLEN: usize = 255;

/// `MSDOSFS_FILESIZE_MAX`: maximum size of a file on a FAT filesystem.
pub const MSDOSFS_FILESIZE_MAX: i64 = 0xFFFFFFFF;

/// `struct fatcache`: the fat cache structure. `fc_fsrcn` is the filesystem relative cluster
/// number that corresponds to the file relative cluster number in this structure (`fc_frcn`).
///
/// The fat entry cache as it stands helps make extending files a "quick" operation by avoiding
/// having to scan the fat to discover the last cluster of the file. The cache also helps
/// sequential reads by remembering the last cluster read from the file. This also prevents us
/// from having to rescan the fat to find the next cluster to read. This cache is probably
/// pretty worthless if a file is opened by multiple processes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Fatcache {
    /// `fc_frcn`: file relative cluster number.
    pub fc_frcn: u32,
    /// `fc_fsrcn`: filesystem relative cluster number.
    pub fc_fsrcn: u32,
}

/// `struct denode`: the in memory variant of a dos directory entry. It is usually contained
/// within a vnode.
///
/// Protected by: the denode's lock (`de_lock`), held through its vnode; the hash chain by the
/// kernel lock.
#[allow(non_snake_case)] // the C field names
pub struct Denode {
    /// `de_next`/`de_prev`: hash chain.
    pub de_hash: ListEntry<Denode>,
    /// `de_prev != NULL`: the denode is on its hash chain.
    pub de_hashed: Cell<bool>,
    /// `de_vnode`: addr of vnode we are part of.
    pub de_vnode: Cell<Option<&'static Vnode>>,
    /// `de_devvp`: vnode of blk dev we live on.
    pub de_devvp: Cell<Option<&'static Vnode>>,
    /// `de_flag`: flag bits (`DE_*`).
    pub de_flag: Cell<u32>,
    /// `de_dev`: device where direntry lives.
    pub de_dev: Cell<Dev>,
    /// `de_dirclust`: cluster of the directory file containing this entry.
    pub de_dirclust: Cell<u32>,
    /// `de_diroffset`: offset of this entry in the directory cluster.
    pub de_diroffset: Cell<u32>,
    /// `de_fndoffset`: offset of found dir entry.
    pub de_fndoffset: Cell<u32>,
    /// `de_fndcnt`: number of slots before `de_fndoffset`.
    pub de_fndcnt: Cell<i32>,
    /// `de_refcnt`: reference count.
    pub de_refcnt: Cell<i64>,
    /// `de_pmp`: addr of our mount struct.
    pub de_pmp: Cell<Option<&'static Msdosfsmount>>,
    /// `de_lockf`: byte level lock list.
    pub de_lockf: LockfStateSlot,
    /// `de_lock`: denode lock.
    pub de_lock: Rrwlock,
    /// `de_Name`: name, from DOS directory entry.
    pub de_Name: Cell<[u8; 11]>,
    /// `de_Attributes`: attributes, from directory entry.
    pub de_Attributes: Cell<u8>,
    /// `de_CTimeHundredth`: creation time, 1/100th of a sec.
    pub de_CTimeHundredth: Cell<u8>,
    /// `de_CTime`: creation time.
    pub de_CTime: Cell<u16>,
    /// `de_CDate`: creation date.
    pub de_CDate: Cell<u16>,
    /// `de_ADate`: access date.
    pub de_ADate: Cell<u16>,
    /// `de_MTime`: modification time.
    pub de_MTime: Cell<u16>,
    /// `de_MDate`: modification date.
    pub de_MDate: Cell<u16>,
    /// `de_StartCluster`: starting cluster of file.
    pub de_StartCluster: Cell<u32>,
    /// `de_FileSize`: size of file in bytes.
    pub de_FileSize: Cell<u32>,
    /// `de_fc`: fat cache.
    pub de_fc: [Cell<Fatcache>; FC_SIZE],
}

// SAFETY: the members are changed under the denode's lock or the kernel lock, as in C.
unsafe impl Sync for Denode {}

impl Denode {
    /// A zeroed denode, as `malloc(M_ZERO)` returns it (or a `struct denode` on the stack the
    /// caller fills).
    pub const fn new() -> Self {
        Self {
            de_hash: ListEntry::new(),
            de_hashed: Cell::new(false),
            de_vnode: Cell::new(None),
            de_devvp: Cell::new(None),
            de_flag: Cell::new(0),
            de_dev: Cell::new(0),
            de_dirclust: Cell::new(0),
            de_diroffset: Cell::new(0),
            de_fndoffset: Cell::new(0),
            de_fndcnt: Cell::new(0),
            de_refcnt: Cell::new(0),
            de_pmp: Cell::new(None),
            de_lockf: Cell::new(None),
            de_lock: Rrwlock::new("denode"),
            de_Name: Cell::new([0; 11]),
            de_Attributes: Cell::new(0),
            de_CTimeHundredth: Cell::new(0),
            de_CTime: Cell::new(0),
            de_CDate: Cell::new(0),
            de_ADate: Cell::new(0),
            de_MTime: Cell::new(0),
            de_MDate: Cell::new(0),
            de_StartCluster: Cell::new(0),
            de_FileSize: Cell::new(0),
            de_fc: [const {
                Cell::new(Fatcache {
                    fc_frcn: 0,
                    fc_fsrcn: 0,
                })
            }; FC_SIZE],
        }
    }

    /// `DETOV(de)`: the denode's vnode.
    pub fn detov(&self) -> &'static Vnode {
        match self.de_vnode.get() {
            Some(vp) => vp,
            None => panic(format_args!("denode {:p}: no vnode", self)),
        }
    }

    /// `dep->de_pmp`, which every denode out of `deget` has.
    pub fn pmp(&self) -> &'static Msdosfsmount {
        match self.de_pmp.get() {
            Some(pmp) => pmp,
            None => panic(format_args!("denode {:p}: no msdosfsmount", self)),
        }
    }

    /// `dep->de_flag |= f`.
    pub fn set_flag(&self, f: u32) {
        self.de_flag.set(self.de_flag.get() | f);
    }

    /// `dep->de_flag &= ~f`.
    pub fn clr_flag(&self, f: u32) {
        self.de_flag.set(self.de_flag.get() & !f);
    }

    /// `dep->de_fc[slot]`.
    pub fn fc(&self, slot: usize) -> Fatcache {
        self.de_fc[slot].get()
    }
}

impl Default for Denode {
    fn default() -> Self {
        Self::new()
    }
}

queue_adapter!(
    /// The denode hash chains (`de_next`/`de_prev`), through `de_hash`.
    pub DeHash: Denode, de_hash => ListEntry<Denode>
);

/// `struct defid`: this overlays the fid structure (see mount.h).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Defid {
    /// `defid_len`: length of structure.
    pub defid_len: u16,
    /// `defid_pad`: force long alignment.
    pub defid_pad: u16,
    /// `defid_dirclust`: cluster this dir entry came from.
    pub defid_dirclust: u32,
    /// `defid_dirofs`: offset of entry within the cluster.
    pub defid_dirofs: u32,
    // `defid_gen` (generation number) is `#if 0` in the C.
}

impl Defid {
    /// The `struct defid` a `struct fid` holds (`(struct defid *)fhp`).
    pub fn from_fid(fid: &Fid) -> Self {
        let d = &fid.fid_data;
        Self {
            defid_len: fid.fid_len,
            defid_pad: fid.fid_reserved,
            defid_dirclust: u32::from_ne_bytes([d[0], d[1], d[2], d[3]]),
            defid_dirofs: u32::from_ne_bytes([d[4], d[5], d[6], d[7]]),
        }
    }

    /// Writes the `struct defid` over `fid`.
    pub fn to_fid(&self, fid: &mut Fid) {
        fid.fid_len = self.defid_len;
        fid.fid_reserved = self.defid_pad;
        fid.fid_data[0..4].copy_from_slice(&self.defid_dirclust.to_ne_bytes());
        fid.fid_data[4..8].copy_from_slice(&self.defid_dirofs.to_ne_bytes());
    }
}

/// `fc_setcache(dep, slot, frcn, fsrcn)`: set a slot in the fat cache.
pub fn fc_setcache(dep: &Denode, slot: usize, frcn: u32, fsrcn: u32) {
    dep.de_fc[slot].set(Fatcache {
        fc_frcn: frcn,
        fc_fsrcn: fsrcn,
    });
}

/// `DE_INTERNALIZE32(dep, dp)`: the high half of a FAT32 start cluster.
pub fn de_internalize32(dep: &Denode, dp: &Direntry) {
    dep.de_StartCluster
        .set(dep.de_StartCluster.get() | (u32::from(getushort(&dp.deHighClust)) << 16));
}

/// `DE_INTERNALIZE(dep, dp)`: transfer directory entry `dp` (external form) into denode `dep`
/// (internal form).
pub fn de_internalize(dep: &Denode, dp: &Direntry) {
    dep.de_Name.set(dp.name11());
    dep.de_Attributes.set(dp.deAttributes);
    dep.de_CTimeHundredth.set(dp.deCTimeHundredth);
    dep.de_CTime.set(getushort(&dp.deCTime));
    dep.de_CDate.set(getushort(&dp.deCDate));
    dep.de_ADate.set(getushort(&dp.deADate));
    dep.de_MTime.set(getushort(&dp.deMTime));
    dep.de_MDate.set(getushort(&dp.deMDate));
    dep.de_StartCluster
        .set(u32::from(getushort(&dp.deStartCluster)));
    dep.de_FileSize.set(getulong(&dp.deFileSize));
    if fat32(dep.pmp()) {
        de_internalize32(dep, dp);
    }
}

/// `DE_EXTERNALIZE(dp, dep)`: transfer denode `dep` (internal form) into directory entry `dp`
/// (external form).
pub fn de_externalize(dp: &mut Direntry, dep: &Denode) {
    dp.set_name11(&dep.de_Name.get());
    dp.deAttributes = dep.de_Attributes.get();
    dp.deLowerCase = CASE_LOWER_BASE | CASE_LOWER_EXT;
    dp.deCTimeHundredth = dep.de_CTimeHundredth.get();
    putushort(&mut dp.deCTime, dep.de_CTime.get());
    putushort(&mut dp.deCDate, dep.de_CDate.get());
    putushort(&mut dp.deADate, dep.de_ADate.get());
    putushort(&mut dp.deMTime, dep.de_MTime.get());
    putushort(&mut dp.deMDate, dep.de_MDate.get());
    putushort(&mut dp.deStartCluster, dep.de_StartCluster.get() as u16);
    let size = if dep.de_Attributes.get() & ATTR_DIRECTORY != 0 {
        0
    } else {
        dep.de_FileSize.get()
    };
    putulong(&mut dp.deFileSize, size);
    let high = if fat32(dep.pmp()) {
        (dep.de_StartCluster.get() >> 16) as u16
    } else {
        0
    };
    putushort(&mut dp.deHighClust, high);
}

/// `VTODE(vp)`: the denode of a msdosfs vnode.
pub fn vtode(vp: &Vnode) -> &'static Denode {
    let data = vp.v_data.get();
    if data.is_null() || vp.v_tag.get() != VT_MSDOSFS {
        panic(format_args!("VTODE: vnode {:p} has no denode", vp));
    }
    // SAFETY: a msdosfs vnode's `v_data` (checked above) is the denode `deget` hung there,
    // which lives until `msdosfs_reclaim` frees it and clears `v_data`; the caller holds the
    // vnode (a reference or its lock), so it is not reclaimed meanwhile.
    unsafe { &*data.cast::<Denode>() }
}

/// `DETIMES(dep, acc, mod, cre)`: turn the pending time update requests of `dep` into its
/// DOS times (`acc` for the access, `mod` for the modification, `cre` for the creation).
pub fn detimes(dep: &Denode, acc: &Timespec, r#mod: &Timespec, cre: &Timespec) {
    if dep.de_flag.get() & (DE_UPDATE | DE_CREATE | DE_ACCESS) == 0 {
        return;
    }
    dep.set_flag(DE_MODIFIED);
    if dep.de_flag.get() & DE_UPDATE != 0 {
        let (dd, dt, _) = unix2dostime(r#mod);
        dep.de_MDate.set(dd);
        dep.de_MTime.set(dt);
        dep.de_Attributes
            .set(dep.de_Attributes.get() | ATTR_ARCHIVE);
    }
    if dep.pmp().pm_flags.get() & MSDOSFSMNT_NOWIN95 as u32 == 0 {
        if dep.de_flag.get() & DE_ACCESS != 0 {
            let (dd, _, _) = unix2dostime(acc);
            dep.de_ADate.set(dd);
        }
        if dep.de_flag.get() & DE_CREATE != 0 {
            let (dd, dt, dh) = unix2dostime(cre);
            dep.de_CDate.set(dd);
            dep.de_CTime.set(dt);
            dep.de_CTimeHundredth.set(dh);
        }
    }
    dep.clr_flag(DE_UPDATE | DE_CREATE | DE_ACCESS);
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests of `denode.rs`: the transfer of directory entries between the on-disk and the
    // in-core form, `DETIMES`, and the file handle.

    use super::*;
    use crate::msdosfs::direntry::ATTR_READONLY;
    use crate::msdosfs::fat::{FAT16_MASK, FAT32_MASK};
    use std::boxed::Box;

    fn pmp(fatmask: u32) -> &'static Msdosfsmount {
        let pmp: &'static Msdosfsmount = Box::leak(Box::new(Msdosfsmount::new()));
        pmp.pm_fatmask.set(fatmask);
        pmp
    }

    /// A directory entry for "HELLO.TXT", 1234 bytes from cluster 0x12345.
    fn entry() -> [u8; 32] {
        let mut e = [0u8; 32];
        e[..11].copy_from_slice(b"HELLO   TXT");
        e[11] = ATTR_READONLY;
        e[13] = 57;
        e[14..16].copy_from_slice(&0x6000u16.to_le_bytes()); // CTime
        e[16..18].copy_from_slice(&0x5021u16.to_le_bytes()); // CDate
        e[18..20].copy_from_slice(&0x5022u16.to_le_bytes()); // ADate
        e[20..22].copy_from_slice(&0x0001u16.to_le_bytes()); // HighClust
        e[22..24].copy_from_slice(&0x6001u16.to_le_bytes()); // MTime
        e[24..26].copy_from_slice(&0x5023u16.to_le_bytes()); // MDate
        e[26..28].copy_from_slice(&0x2345u16.to_le_bytes()); // StartCluster
        e[28..32].copy_from_slice(&1234u32.to_le_bytes());
        e
    }

    #[test]
    fn internalize_fat16_ignores_the_high_cluster() {
        let dep = Denode::new();
        dep.de_pmp.set(Some(pmp(FAT16_MASK)));
        let e = entry();
        de_internalize(&dep, Direntry::at(&e, 0));
        assert_eq!(&dep.de_Name.get(), b"HELLO   TXT");
        assert_eq!(dep.de_Attributes.get(), ATTR_READONLY);
        assert_eq!(dep.de_CTimeHundredth.get(), 57);
        assert_eq!(dep.de_CTime.get(), 0x6000);
        assert_eq!(dep.de_CDate.get(), 0x5021);
        assert_eq!(dep.de_ADate.get(), 0x5022);
        assert_eq!(dep.de_MTime.get(), 0x6001);
        assert_eq!(dep.de_MDate.get(), 0x5023);
        assert_eq!(dep.de_StartCluster.get(), 0x2345);
        assert_eq!(dep.de_FileSize.get(), 1234);

        let mut out = [0xaau8; 32];
        de_externalize(Direntry::at_mut(&mut out, 0), &dep);
        let mut want = entry();
        want[12] = CASE_LOWER_BASE | CASE_LOWER_EXT;
        want[20..22].copy_from_slice(&[0, 0]);
        assert_eq!(out, want);
    }

    #[test]
    fn internalize_fat32_round_trip() {
        let dep = Denode::new();
        dep.de_pmp.set(Some(pmp(FAT32_MASK)));
        let e = entry();
        de_internalize(&dep, Direntry::at(&e, 0));
        assert_eq!(dep.de_StartCluster.get(), 0x12345);
        let mut out = [0u8; 32];
        de_externalize(Direntry::at_mut(&mut out, 0), &dep);
        let mut want = entry();
        want[12] = CASE_LOWER_BASE | CASE_LOWER_EXT;
        assert_eq!(out, want);

        // A directory's entry has size 0 on disk.
        dep.de_Attributes.set(ATTR_DIRECTORY);
        de_externalize(Direntry::at_mut(&mut out, 0), &dep);
        assert_eq!(&out[28..32], &[0, 0, 0, 0]);
    }

    #[test]
    fn detimes_turns_requests_into_dos_times() {
        let pmp = pmp(FAT16_MASK);
        let dep = Denode::new();
        dep.de_pmp.set(Some(pmp));
        // 2026-10-04 12:34:56, and 1980-01-01 00:00:00.
        let now = Timespec {
            tv_sec: 1_791_117_296,
            tv_nsec: 0,
        };
        let epoch = Timespec {
            tv_sec: 315_532_800,
            tv_nsec: 0,
        };

        // No request: nothing changes.
        detimes(&dep, &now, &now, &now);
        assert_eq!(dep.de_flag.get(), 0);

        dep.set_flag(DE_UPDATE | DE_ACCESS | DE_CREATE);
        detimes(&dep, &epoch, &now, &epoch);
        assert_eq!(dep.de_flag.get(), DE_MODIFIED);
        assert_eq!(dep.de_MDate.get(), 4 | 10 << 5 | 46 << 9);
        assert_eq!(dep.de_MTime.get(), 28 | 34 << 5 | 12 << 11);
        assert_eq!(dep.de_Attributes.get(), ATTR_ARCHIVE);
        assert_eq!(dep.de_ADate.get(), 0x21);
        assert_eq!(dep.de_CDate.get(), 0x21);
        assert_eq!((dep.de_CTime.get(), dep.de_CTimeHundredth.get()), (0, 0));

        // Without Win95 there are no access and creation times.
        pmp.pm_flags.set(MSDOSFSMNT_NOWIN95 as u32);
        dep.set_flag(DE_ACCESS | DE_CREATE);
        detimes(&dep, &now, &now, &now);
        assert_eq!(dep.de_ADate.get(), 0x21);
        assert_eq!(dep.de_CDate.get(), 0x21);
        assert_eq!(dep.de_flag.get(), DE_MODIFIED);
    }

    #[test]
    fn defid_overlays_fid() {
        let defid = Defid {
            defid_len: size_of::<Defid>() as u16,
            defid_pad: 0,
            defid_dirclust: 7,
            defid_dirofs: 96,
        };
        let mut fid = Fid::default();
        defid.to_fid(&mut fid);
        assert_eq!(fid.fid_len, 12);
        assert_eq!(Defid::from_fid(&fid), defid);
    }
}
/* </TESTS> */
