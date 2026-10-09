/* <LICENSES> */
/*
 * Copyright (c) 2001 Ian Dowse.  All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 * $FreeBSD: src/sys/ufs/ufs/dirhash.h,v 1.4 2003/01/01 18:48:59 schweikh Exp $
 */
/* </LICENSES> */

/* <CODE> */
//! `<ufs/ufs/dirhash.h>`: the hash of a large directory. For fast operations on large
//! directories, we maintain a hash that maps the file name to the offset of the directory
//! entry within the directory file.
//!
//! Upstream: sys/ufs/ufs/dirhash.h @ 3ce1f3f79392
//!
//! The hashing uses a dumb spillover to the next free slot on collisions, so we must keep
//! the utilisation low to avoid long linear searches. Deleted entries that are not the last
//! in a chain must be marked [`DIRHASH_DEL`].
//!
//! We also maintain information about free space in each block to speed up creations.
//!
//! Dirhash uses a score mechanism to achieve a hybrid between a least-recently-used and a
//! least-often-used algorithm for entry recycling. The score is incremented when a directory
//! is used, and decremented when the directory is a candidate for recycling. When the score
//! reaches zero, the hash is recycled. Hashes are linked together on a TAILQ list, and hashes
//! with higher scores filter towards the tail (most recently used) end of the list.
//!
//! New hash entries are given an initial score of [`DH_SCOREINIT`] and are placed at the
//! most-recently-used end of the list. This helps a lot in the worst-case case scenario where
//! every directory access is to a directory that is not hashed (i.e. the working set of hash
//! candidates is much larger than the configured memory limit). In this case it limits the
//! number of hash builds to `1/DH_SCOREINIT` of the number of accesses.
//!
//! The main hash table has 2 levels. It is an array of pointers to blocks of [`DH_NBLKOFF`]
//! offsets.
//!
//! ## Deviations
//! - `struct dirhash` is [`Dirhash`]: a `malloc(M_DIRHASH)` block whose members are `Cell`s
//!   (the C changes them through shared pointers under `dh_mtx`). `dh_hash` (the array of
//!   pointers to `ufsdirhash_pool` blocks) and `dh_blkfree` stay raw pointers beside their
//!   counts (`docs/C_TO_RUST.md`, a `malloc`ed array kept as a pointer and a count);
//!   `DH_ENTRY(dh, slot)` is [`Dirhash::dh_entry`]/[`Dirhash::set_dh_entry`] and
//!   `dh_blkfree[i]` is [`Dirhash::blkfree`]/[`Dirhash::set_blkfree`], which check the index
//!   and the pointer and panic where the C would read out of bounds or through NULL.
//! - The prototypes and the `ufs_mindirhashsize`/`ufs_dirhashmaxmem`/`ufs_dirhashmem`
//!   `extern`s are the definitions in `ufs_dirhash.rs` (feature `ufs_dirhash`, the C's
//!   `option UFS_DIRHASH`); this header is compiled always, as `inode.h` names
//!   `struct dirhash *i_dirhash` without the option.

use core::cell::Cell;
use core::ptr;

use crate::kern::subr_prf::panic;
use crate::queue_adapter;
use crate::sys::queue::{TailqEntry, TailqHead};
use crate::sys::rwlock::Rwlock;
use crate::ufs::ufs::dir::{Doff, MAXNAMLEN, directsiz};

/// `DIRHASH_EMPTY`: entry unused.
pub const DIRHASH_EMPTY: Doff = -1;
/// `DIRHASH_DEL`: deleted entry; may be part of chain.
pub const DIRHASH_DEL: Doff = -2;

/// `DIRALIGN`.
pub const DIRALIGN: i32 = 4;
/// `DH_NFSTATS`: max `DIRALIGN` words in a directory entry.
pub const DH_NFSTATS: usize = directsiz(MAXNAMLEN + 1) / DIRALIGN as usize;

/// `DH_SCOREINIT`: initial `dh_score` when dirhash built.
pub const DH_SCOREINIT: i32 = 8;
/// `DH_SCOREMAX`: max `dh_score` value.
pub const DH_SCOREMAX: i32 = 64;

/// `DH_BLKOFFSHIFT`.
pub const DH_BLKOFFSHIFT: i32 = 8;
/// `DH_NBLKOFF`: offsets in one block of the hash array.
pub const DH_NBLKOFF: i32 = 1 << DH_BLKOFFSHIFT;
/// `DH_BLKOFFMASK`.
pub const DH_BLKOFFMASK: i32 = DH_NBLKOFF - 1;

/// `struct dirhash`.
///
/// Protected by: `dh_mtx` for every member but `dh_list`, which `ufsdirhash_mtx` protects.
/// The `dh_mtx` lock is acquired either via the inode lock, or via `ufsdirhash_mtx`. Only
/// the owner of the inode may free the associated dirhash, but anything can steal its
/// memory and set `dh_hash` to NULL.
pub struct Dirhash {
    /// `dh_mtx`: protects all fields except `dh_list`.
    pub dh_mtx: Rwlock,
    /// `dh_hash`: the hash array (2-level): `dh_narrays` pointers to blocks of
    /// `DH_NBLKOFF` offsets; NULL once recycled.
    pub dh_hash: Cell<*mut *mut Doff>,
    /// `dh_narrays`: number of entries in `dh_hash`.
    pub dh_narrays: Cell<i32>,
    /// `dh_hlen`: total slots in the 2-level hash array.
    pub dh_hlen: Cell<i32>,
    /// `dh_hused`: entries in use.
    pub dh_hused: Cell<i32>,
    /// `dh_blkfree`: free `DIRALIGN` words in each dir block (`dh_nblk` bytes). Free space
    /// statistics. XXX assumes `DIRBLKSIZ` is 512.
    pub dh_blkfree: Cell<*mut u8>,
    /// `dh_nblk`: size of `dh_blkfree` array.
    pub dh_nblk: Cell<i32>,
    /// `dh_dirblks`: number of `DIRBLKSIZ` blocks in dir.
    pub dh_dirblks: Cell<i32>,
    /// `dh_firstfree`: first blk with N words free.
    pub dh_firstfree: [Cell<i32>; DH_NFSTATS + 1],
    /// `dh_seqopt`: sequential access optimisation enabled.
    pub dh_seqopt: Cell<i32>,
    /// `dh_seqoff`: sequential access optimisation offset.
    pub dh_seqoff: Cell<Doff>,
    /// `dh_score`: access count for this dirhash.
    pub dh_score: Cell<i32>,
    /// `dh_onlist`: true if on the `ufsdirhash_list` chain.
    pub dh_onlist: Cell<i32>,
    /// `dh_list`: chain of all dirhashes. Protected by `ufsdirhash_mtx`.
    pub dh_list: TailqEntry<Dirhash>,
}

// SAFETY: the members are changed under `dh_mtx` (or `ufsdirhash_mtx` for `dh_list`), or
// while the dirhash is reachable by one thread only (being built or freed), as in C.
unsafe impl Sync for Dirhash {}

impl Dirhash {
    /// A dirhash with no arrays, as `malloc(M_ZERO)` returns it.
    pub const fn new() -> Self {
        Self {
            dh_mtx: Rwlock::new("dirhash"),
            dh_hash: Cell::new(ptr::null_mut()),
            dh_narrays: Cell::new(0),
            dh_hlen: Cell::new(0),
            dh_hused: Cell::new(0),
            dh_blkfree: Cell::new(ptr::null_mut()),
            dh_nblk: Cell::new(0),
            dh_dirblks: Cell::new(0),
            dh_firstfree: [const { Cell::new(0) }; DH_NFSTATS + 1],
            dh_seqopt: Cell::new(0),
            dh_seqoff: Cell::new(0),
            dh_score: Cell::new(0),
            dh_onlist: Cell::new(0),
            dh_list: TailqEntry::new(),
        }
    }

    /// The address of `DH_ENTRY(dh, slot)`; panics on a recycled hash or a slot out of
    /// range, which the C would follow anyway.
    fn dh_entry_ptr(&self, slot: i32) -> *mut Doff {
        let hash = self.dh_hash.get();
        if hash.is_null() || slot < 0 || slot >= self.dh_hlen.get() {
            panic(format_args!(
                "dirhash {:p}: slot {} out of {} (hash {:p})",
                self,
                slot,
                self.dh_hlen.get(),
                hash
            ));
        }
        // SAFETY: a non-null `dh_hash` holds `dh_narrays` pointers to blocks of `DH_NBLKOFF`
        // offsets, `dh_hlen == dh_narrays * DH_NBLKOFF` (`ufsdirhash_build`), so both indices
        // are in bounds. The arrays are freed only after `dh_hash` is cleared (recycle) or
        // with the dirhash itself (`ufsdirhash_free`, the build's failure path), both under
        // `dh_mtx` or with the dirhash reachable by one thread, and this read does not sleep.
        unsafe {
            (*hash.add((slot >> DH_BLKOFFSHIFT) as usize)).add((slot & DH_BLKOFFMASK) as usize)
        }
    }

    /// `DH_ENTRY(dh, slot)`.
    pub fn dh_entry(&self, slot: i32) -> Doff {
        // SAFETY: an in-bounds slot of a live array (`dh_entry_ptr`).
        unsafe { self.dh_entry_ptr(slot).read() }
    }

    /// `DH_ENTRY(dh, slot) = v`.
    pub fn set_dh_entry(&self, slot: i32, v: Doff) {
        // SAFETY: an in-bounds slot of a live array (`dh_entry_ptr`).
        unsafe { self.dh_entry_ptr(slot).write(v) }
    }

    /// The address of `dh_blkfree[block]`; panics on a recycled hash or a block out of
    /// range.
    fn blkfree_ptr(&self, block: i32) -> *mut u8 {
        let p = self.dh_blkfree.get();
        if p.is_null() || block < 0 || block >= self.dh_nblk.get() {
            panic(format_args!(
                "dirhash {:p}: block {} out of {} (blkfree {:p})",
                self,
                block,
                self.dh_nblk.get(),
                p
            ));
        }
        // SAFETY: a non-null `dh_blkfree` has `dh_nblk` bytes; it lives as `dh_hash` does
        // (`dh_entry_ptr`).
        unsafe { p.add(block as usize) }
    }

    /// `dh->dh_blkfree[block]`.
    pub fn blkfree(&self, block: i32) -> u8 {
        // SAFETY: an in-bounds byte of a live array (`blkfree_ptr`).
        unsafe { self.blkfree_ptr(block).read() }
    }

    /// `dh->dh_blkfree[block] = v`.
    pub fn set_blkfree(&self, block: i32, v: u8) {
        // SAFETY: an in-bounds byte of a live array (`blkfree_ptr`).
        unsafe { self.blkfree_ptr(block).write(v) }
    }

    /// `dh->dh_firstfree[i]`.
    pub fn firstfree(&self, i: usize) -> i32 {
        self.dh_firstfree[i].get()
    }

    /// `dh->dh_firstfree[i] = v`.
    pub fn set_firstfree(&self, i: usize, v: i32) {
        self.dh_firstfree[i].set(v);
    }
}

impl Default for Dirhash {
    fn default() -> Self {
        Self::new()
    }
}

queue_adapter!(
    /// `ufsdirhash_list`'s link: dirhashes through their `dh_list`.
    pub DirhashList: Dirhash, dh_list => TailqEntry<Dirhash>
);

/// `TAILQ_HEAD(, dirhash)`: the type of `ufsdirhash_list`.
pub struct DirhashListHead(pub TailqHead<DirhashList>);

// SAFETY: the queue is changed only under `ufsdirhash_mtx`, as in C.
unsafe impl Sync for DirhashListHead {}

const _: () = {
    assert!(DH_NFSTATS == 67);
    assert!(DH_NBLKOFF == 256);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn constants_match_the_c_header() {
        let defs = crate::reftest::defines("sys/ufs/ufs/dirhash.h");
        for (name, v) in [
            ("DIRHASH_EMPTY", i64::from(DIRHASH_EMPTY)),
            ("DIRHASH_DEL", i64::from(DIRHASH_DEL)),
            ("DIRALIGN", i64::from(DIRALIGN)),
            ("DH_SCOREINIT", i64::from(DH_SCOREINIT)),
            ("DH_SCOREMAX", i64::from(DH_SCOREMAX)),
            ("DH_BLKOFFSHIFT", i64::from(DH_BLKOFFSHIFT)),
        ] {
            assert_eq!(crate::reftest::int(&defs, name), Some(v), "{name}");
        }
    }
}
/* </TESTS> */
