/* $OpenBSD: ufs_dirhash.c,v 1.43 2024/01/09 03:15:59 guenther Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 2001, 2002 Ian Dowse.  All rights reserved.
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
 */
/* </LICENSES> */

//! A hash-based lookup scheme for UFS directories (`option UFS_DIRHASH`, cargo feature
//! `ufs_dirhash`): `ufsdirhash_build` hashes a directory of at least `ufs_mindirhashsize`
//! bytes the first time `ufs_lookup` searches it, mapping each name to the offset of its
//! entry and keeping the free space of every `DIRBLKSIZ` block, so that lookups and
//! creations in large directories do not scan the whole directory. `ufs_direnter`,
//! `ufs_dirremove` and the truncation keep the hash in step (`ufsdirhash_add`, `_remove`,
//! `_move`, `_newblk`, `_dirtrunc`). The memory of all hashes is bounded by
//! `ufs_dirhashmaxmem`: a hash that does not fit recycles the least used ones
//! (`ufsdirhash_recycle`), whose owners rebuild them on their next lookup.
//!
//! Upstream: sys/ufs/ufs/ufs_dirhash.c @ 3ce1f3f79392
//!
//! Locking order: `ufsdirhash_mtx`, then `dh_mtx`. The `dh_mtx` lock should be acquired
//! either via the inode lock, or via `ufsdirhash_mtx`. Only the owner of the inode may free
//! the associated dirhash, but anything can steal its memory and set `dh_hash` to NULL.
//!
//! ## Deviations
//! - The `ufs_mindirhashsize`, `ufs_dirhashmaxmem`, `ufs_dirhashmem` and
//!   `ufs_dirhashcheck` globals are the atomics [`UFS_MINDIRHASHSIZE`],
//!   [`UFS_DIRHASHMAXMEM`], [`UFS_DIRHASHMEM`] and [`UFS_DIRHASHCHECK`] (`ffs_vars[]` hands
//!   the first three to `sysctl_bounded_arr`); `ufs_dirhashmem` is still changed only under
//!   `ufsdirhash_mtx`. `ufsdirhash_key` is two atomic words, written by `ufsdirhash_init`.
//! - The memory accounted per hash uses `size_of::<Dirhash>()`, the Rust structure's size,
//!   for the C's `sizeof(struct dirhash)`.
//! - A `struct direct *` argument (`ufsdirhash_add`, `_remove`, `_move`) is the entry's
//!   name: all the C reads from it is `d_name`, `d_namlen` and `DIRSIZ(dp)`, which follows
//!   from `d_namlen`. A directory block (`ufsdirhash_checkblock`'s `buf`) is a byte slice,
//!   and `ufsdirhash_getprev` takes the block and the entry's place in it instead of the
//!   entry's pointer.
//! - Return values: `ufsdirhash_build` is `bool` (`true` for the C's 0, the directory is
//!   hashed); `ufsdirhash_findfree` and `ufsdirhash_enduseful` return `Option` (`None` for
//!   the C's -1), `ufsdirhash_findfree` with the slot size the C stores in `*slotsize`;
//!   `ufsdirhash_lookup` returns the offset and the buffer (the C's `*offp`, `*bpp`), and
//!   `ENOENT` or `EJUSTRETURN` as errors; `ufsdirhash_getprev` returns `Option`;
//!   `ufsdirhash_recycle` returns `true` (the C's 0) with the list locked.
//! - `ufsdirhash_lookup` reads `TAILQ_NEXT(dh, dh_list)` without the list lock only while
//!   `dh_onlist` says the hash is on the list: a recycled hash's links are poisoned under
//!   feature `diagnostic` (`_Q_INVALIDATE`), which the C reads as a non-NULL hint and never
//!   follows, and a Rust reference may not be made from them.
//! - `ufsdirhash_free` clears `dh_hash` before freeing the arrays, so that no slot accessor
//!   can reach them; the C frees them and then the structure.
//! - `ufsdirhash_checkblock` runs, as in C, only when `ufs_dirhashcheck` is set (0 by
//!   default; no sysctl sets it in OpenBSD); the host tests set it.

use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicI32, AtomicU64, Ordering};

use crate::crypto::siphash::{SipHash24, SiphashKey};
use crate::dev::rnd::arc4random_buf;
use crate::kassert;
use crate::kern::kern_malloc::{free, malloc, mallocarray};
use crate::kern::kern_rwlock::{rw_enter_write, rw_exit_write, rw_init};
use crate::kern::subr_pool::{pool_destroy, pool_get, pool_init, pool_put};
use crate::kern::subr_prf::{Str, panic};
use crate::kern::vfs_bio::brelse;
use crate::machine::intr::IPL_NONE;
use crate::sys::buf::Buf;
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_DIRHASH, M_NOWAIT, M_ZERO};
use crate::sys::param::howmany;
use crate::sys::pool::{PR_WAITOK, Pool};
use crate::sys::queue::TailqHead;
use crate::sys::rwlock::Rwlock;
use crate::ufs::ufs::dir::{DIRBLKSIZ, Doff, d_ino, d_name, d_namlen, d_reclen, directsiz, dirsiz};
use crate::ufs::ufs::dirhash::{
    DH_NBLKOFF, DH_NFSTATS, DH_SCOREINIT, DH_SCOREMAX, DIRALIGN, DIRHASH_DEL, DIRHASH_EMPTY,
    Dirhash, DirhashList, DirhashListHead,
};
use crate::ufs::ufs::inode::{Inode, UFS_BUFATOFF};

/// `DIRBLKSIZ` as an `int`.
const DIRBLK: i32 = DIRBLKSIZ as i32;

/// `WRAPINCR(val, limit)`.
const fn wrapincr(val: i32, limit: i32) -> i32 {
    if val + 1 == limit { 0 } else { val + 1 }
}

/// `WRAPDECR(val, limit)`.
const fn wrapdecr(val: i32, limit: i32) -> i32 {
    if val == 0 { limit - 1 } else { val - 1 }
}

/// `BLKFREE2IDX(n)`.
const fn blkfree2idx(n: i32) -> usize {
    if n > DH_NFSTATS as i32 {
        DH_NFSTATS
    } else {
        n as usize
    }
}

/// `ufs_mindirhashsize`: the smallest directory, in bytes, that is hashed
/// (`vfs.ffs.dirhash_dirsize`).
pub static UFS_MINDIRHASHSIZE: AtomicI32 = AtomicI32::new(0);
/// `ufs_dirhashmaxmem`: the memory all hashes may use, in bytes (`vfs.ffs.dirhash_maxmem`).
pub static UFS_DIRHASHMAXMEM: AtomicI32 = AtomicI32::new(0);
/// `ufs_dirhashmem`: the memory the hashes use, in bytes (`vfs.ffs.dirhash_mem`). Changed
/// under `ufsdirhash_mtx`.
pub static UFS_DIRHASHMEM: AtomicI32 = AtomicI32::new(0);
/// `ufs_dirhashcheck`: run `ufsdirhash_checkblock`'s consistency checks.
pub static UFS_DIRHASHCHECK: AtomicI32 = AtomicI32::new(0);

/// `ufsdirhash_key`: the SipHash key of every hash (`k0`, `k1`), set by `ufsdirhash_init`.
static UFSDIRHASH_KEY: [AtomicU64; 2] = [AtomicU64::new(0), AtomicU64::new(0)];

/// `ufsdirhash_pool`: the blocks of `DH_NBLKOFF` offsets of the hash arrays.
pub static UFSDIRHASH_POOL: Pool = Pool::new();

/// `ufsdirhash_list`: dirhash list; recently-used entries are near the tail.
pub static UFSDIRHASH_LIST: DirhashListHead = DirhashListHead(TailqHead::new());

/// `ufsdirhash_mtx`: protects `ufsdirhash_list`, the `dh_list` field, `ufs_dirhashmem`.
pub static UFSDIRHASH_MTX: Rwlock = Rwlock::new("dirhash_list");

/// `DIRHASHLIST_LOCK()`.
fn dirhashlist_lock() {
    rw_enter_write(&UFSDIRHASH_MTX);
}

/// `DIRHASHLIST_UNLOCK()`.
fn dirhashlist_unlock() {
    rw_exit_write(&UFSDIRHASH_MTX);
}

/// `DIRHASH_LOCK(dh)`.
fn dirhash_lock(dh: &Dirhash) {
    rw_enter_write(&dh.dh_mtx);
}

/// `DIRHASH_UNLOCK(dh)`.
fn dirhash_unlock(dh: &Dirhash) {
    rw_exit_write(&dh.dh_mtx);
}

/// `DIRHASH_BLKALLOC_WAITOK()`: a block of `DH_NBLKOFF` offsets.
fn dirhash_blkalloc_waitok() -> Option<NonNull<Doff>> {
    pool_get(&UFSDIRHASH_POOL, PR_WAITOK).map(NonNull::cast)
}

/// `DIRHASH_BLKFREE(v)`.
fn dirhash_blkfree(v: *mut Doff) {
    if let Some(v) = NonNull::new(v) {
        pool_put(&UFSDIRHASH_POOL, v.cast());
    }
}

/// `ip->i_dirhash` as a reference.
///
/// # Safety
///
/// The caller holds the inode's lock (or is the inode's only user) and does not use the
/// reference after `ufsdirhash_free(ip)`: the dirhash lives from `ufsdirhash_build` until
/// `ufsdirhash_free`, which only the inode's owner calls.
unsafe fn i_dirhash(ip: &Inode) -> Option<&Dirhash> {
    // SAFETY: a set `i_dirhash` points at a live, initialised `Dirhash` (the contract).
    ip.i_dirhash.get().map(|p| unsafe { p.as_ref() })
}

/// The bytes of the hash arrays of a hash of `narrays` blocks and `nblk` block counters,
/// without the structure: what `ufsdirhash_free` and `ufsdirhash_recycle` give back.
const fn arrays_mem(narrays: i32, nblk: i32) -> i32 {
    narrays * size_of::<*mut Doff>() as i32
        + narrays * DH_NBLKOFF * size_of::<Doff>() as i32
        + nblk * size_of::<u8>() as i32
}

/// Frees the arrays of a hash detached from its dirhash: `narrays` pool blocks, the
/// pointer array and the `nblk` block counters (either array may be NULL).
fn free_arrays(hash: *mut *mut Doff, narrays: i32, blkfree: *mut u8, nblk: i32) {
    if let Some(h) = NonNull::new(hash) {
        for i in 0..narrays as usize {
            // SAFETY: `hash` has `narrays` pointers (zero-filled at allocation, so the ones
            // never set are NULL), and nothing else reaches it any more.
            dirhash_blkfree(unsafe { h.as_ptr().add(i).read() });
        }
        free(
            h.cast(),
            M_DIRHASH,
            narrays as usize * size_of::<*mut Doff>(),
        );
    }
    if let Some(b) = NonNull::new(blkfree) {
        free(b, M_DIRHASH, nblk as usize * size_of::<u8>());
    }
}

/// `ufsdirhash_build`: attempt to build up a hash table for the directory contents in
/// inode `ip`. Returns `true` on success (the C's 0), or `false` (-1) if the operation
/// failed or the directory should not be hashed.
pub fn ufsdirhash_build(ip: &Inode) -> bool {
    let size = ip.dip_size() as i64;

    // Check if we can/should use dirhash.
    // SAFETY: the inode is locked by its caller (ufs_lookup), and `dh` is not used after
    // `ufsdirhash_free`.
    match unsafe { i_dirhash(ip) } {
        None => {
            if size < i64::from(UFS_MINDIRHASHSIZE.load(Ordering::Relaxed)) {
                return false;
            }
        }
        Some(dh) => {
            // Hash exists, but sysctls could have changed.
            if size < i64::from(UFS_MINDIRHASHSIZE.load(Ordering::Relaxed))
                || UFS_DIRHASHMEM.load(Ordering::Relaxed)
                    > UFS_DIRHASHMAXMEM.load(Ordering::Relaxed)
            {
                ufsdirhash_free(ip);
                return false;
            }
            // Check if hash exists and is intact (note: unlocked read).
            if !dh.dh_hash.get().is_null() {
                return true;
            }
            // Free the old, recycled hash and build a new one.
            ufsdirhash_free(ip);
        }
    }

    // Don't hash removed directories.
    if ip.i_effnlink.get() == 0 {
        return false;
    }

    // Allocate 50% more entries than this dir size could ever need.
    kassert!(size >= DIRBLKSIZ as i64);
    let mut nslots = (size / directsiz(1) as i64) as i32;
    nslots = (nslots * 3 + 1) / 2;
    let narrays = howmany(nslots as usize, DH_NBLKOFF as usize) as i32;
    nslots = narrays * DH_NBLKOFF;
    let dirblocks = howmany(size as usize, DIRBLKSIZ) as i32;
    let nblocks = (dirblocks * 3 + 1) / 2;

    let memreqd = size_of::<Dirhash>() as i32 + arrays_mem(narrays, nblocks);
    dirhashlist_lock();
    if memreqd + UFS_DIRHASHMEM.load(Ordering::Relaxed) > UFS_DIRHASHMAXMEM.load(Ordering::Relaxed)
    {
        dirhashlist_unlock();
        if memreqd > UFS_DIRHASHMAXMEM.load(Ordering::Relaxed) / 2 {
            return false;
        }

        // Try to free some space.
        if !ufsdirhash_recycle(memreqd) {
            return false;
        }
        // Enough was freed, and list has been locked.
    }
    UFS_DIRHASHMEM.fetch_add(memreqd, Ordering::Relaxed);
    dirhashlist_unlock();

    // Use non-blocking mallocs so that we will revert to a linear lookup on failure rather
    // than potentially blocking forever.
    let Some(mem) = malloc(size_of::<Dirhash>(), M_DIRHASH, M_NOWAIT | M_ZERO) else {
        dirhashlist_lock();
        UFS_DIRHASHMEM.fetch_sub(memreqd, Ordering::Relaxed);
        dirhashlist_unlock();
        return false;
    };
    let dhp = mem.cast::<Dirhash>();
    // SAFETY: a fresh allocation of `size_of::<Dirhash>()` bytes, aligned by malloc(9),
    // written once before anything else sees it.
    unsafe { dhp.as_ptr().write(Dirhash::new()) };
    // SAFETY: as above; it lives until `ufsdirhash_free` or the failure path below.
    let dh = unsafe { dhp.as_ref() };

    let hash = mallocarray(
        narrays as usize,
        size_of::<*mut Doff>(),
        M_DIRHASH,
        M_NOWAIT | M_ZERO,
    );
    dh.dh_hash
        .set(hash.map_or(ptr::null_mut(), |p| p.cast().as_ptr()));
    let blkfree = mallocarray(
        nblocks as usize,
        size_of::<u8>(),
        M_DIRHASH,
        M_NOWAIT | M_ZERO,
    );
    dh.dh_blkfree
        .set(blkfree.map_or(ptr::null_mut(), NonNull::as_ptr));

    'fail: {
        let hash = dh.dh_hash.get();
        if hash.is_null() || dh.dh_blkfree.get().is_null() {
            break 'fail;
        }
        for i in 0..narrays as usize {
            let Some(blk) = dirhash_blkalloc_waitok() else {
                break 'fail;
            };
            // SAFETY: `hash` has `narrays` pointers; `blk` is a fresh pool item of
            // `DH_NBLKOFF` offsets that nothing else reaches yet.
            unsafe {
                hash.add(i).write(blk.as_ptr());
                core::slice::from_raw_parts_mut(blk.as_ptr(), DH_NBLKOFF as usize)
                    .fill(DIRHASH_EMPTY);
            }
        }

        // Initialise the hash table and block statistics.
        rw_init(&dh.dh_mtx, "dirhash");
        dh.dh_narrays.set(narrays);
        dh.dh_hlen.set(nslots);
        dh.dh_nblk.set(nblocks);
        dh.dh_dirblks.set(dirblocks);
        for i in 0..dirblocks {
            dh.set_blkfree(i, (DIRBLK / DIRALIGN) as u8);
        }
        for i in 0..DH_NFSTATS {
            dh.set_firstfree(i, -1);
        }
        dh.set_firstfree(DH_NFSTATS, 0);
        dh.dh_seqopt.set(0);
        dh.dh_seqoff.set(0);
        dh.dh_score.set(DH_SCOREINIT);
        ip.i_dirhash.set(Some(dhp));

        let bmask = ip.ump().mountp().mnt_stat.get().f_iosize as i32 - 1;
        let mut bp: Option<&'static Buf> = None;
        let mut pos: Doff = 0;
        while i64::from(pos) < ip.dip_size() as i64 {
            // If necessary, get the next directory block.
            if pos & bmask == 0 {
                if let Some(b) = bp.take() {
                    brelse(b);
                }
                match UFS_BUFATOFF(ip, i64::from(pos)) {
                    Ok((b, _)) => bp = Some(b),
                    Err(_) => break 'fail,
                }
            }
            let Some(b) = bp else {
                panic(format_args!("ufsdirhash_build: no directory block"));
            };
            // SAFETY: the buffer is ours (busy from UFS_BUFATOFF) and mapped; the slice dies
            // before it is released.
            let data = unsafe { b.data() };
            // Add this entry to the hash.
            let ep = (pos & bmask) as usize;
            let reclen = i32::from(d_reclen(data, ep));
            if reclen == 0 || reclen > DIRBLK - (pos & (DIRBLK - 1)) {
                // Corrupted directory.
                brelse(b);
                break 'fail;
            }
            if d_ino(data, ep) != 0 {
                // Add the entry (simplified ufsdirhash_add).
                let namlen = d_namlen(data, ep);
                let mut slot = ufsdirhash_hash(dh, d_name(data, ep, usize::from(namlen)));
                while dh.dh_entry(slot) != DIRHASH_EMPTY {
                    slot = wrapincr(slot, dh.dh_hlen.get());
                }
                dh.dh_hused.set(dh.dh_hused.get() + 1);
                dh.set_dh_entry(slot, pos);
                ufsdirhash_adjfree(dh, pos, -(dirsiz(namlen) as i32));
            }
            pos += reclen;
        }

        if let Some(b) = bp {
            brelse(b);
        }
        dirhashlist_lock();
        // SAFETY: under `ufsdirhash_mtx`; the new dirhash is on no list and stays in place
        // until `ufsdirhash_free` or `ufsdirhash_recycle` unlinks it.
        unsafe { UFSDIRHASH_LIST.0.insert_tail(dh) };
        dh.dh_onlist.set(1);
        dirhashlist_unlock();
        return true;
    }

    // fail:
    let hash = dh.dh_hash.replace(ptr::null_mut());
    let blkfree = dh.dh_blkfree.replace(ptr::null_mut());
    free_arrays(hash, narrays, blkfree, nblocks);
    ip.i_dirhash.set(None);
    free(dhp.cast(), M_DIRHASH, size_of::<Dirhash>());
    dirhashlist_lock();
    UFS_DIRHASHMEM.fetch_sub(memreqd, Ordering::Relaxed);
    dirhashlist_unlock();
    false
}

/// `ufsdirhash_free`: free any hash table associated with inode `ip`.
pub fn ufsdirhash_free(ip: &Inode) {
    let Some(dhp) = ip.i_dirhash.get() else {
        return;
    };
    // SAFETY: the inode's owner frees its dirhash, which is live until the `free` below.
    let dh = unsafe { dhp.as_ref() };
    dirhashlist_lock();
    dirhash_lock(dh);
    if dh.dh_onlist.get() != 0 {
        // SAFETY: under `ufsdirhash_mtx`; `dh_onlist` says the dirhash is on the list.
        unsafe { UFSDIRHASH_LIST.0.remove(dh) };
    }
    dirhash_unlock(dh);
    dirhashlist_unlock();

    // The dirhash pointed to by 'dh' is exclusively ours now.

    let mut mem = size_of::<Dirhash>() as i32;
    let hash = dh.dh_hash.replace(ptr::null_mut());
    if !hash.is_null() {
        let blkfree = dh.dh_blkfree.replace(ptr::null_mut());
        free_arrays(hash, dh.dh_narrays.get(), blkfree, dh.dh_nblk.get());
        mem += arrays_mem(dh.dh_narrays.get(), dh.dh_nblk.get());
    }
    ip.i_dirhash.set(None);
    free(dhp.cast(), M_DIRHASH, size_of::<Dirhash>());

    dirhashlist_lock();
    UFS_DIRHASHMEM.fetch_sub(mem, Ordering::Relaxed);
    dirhashlist_unlock();
}

/// `ufsdirhash_lookup`: find the offset of the specified name within the given inode.
/// Returns the offset and the buffer holding the entry on success, `ENOENT` if the entry
/// does not exist, or `EJUSTRETURN` if the caller should revert to a linear search.
///
/// If `prevoffp` is given, the offset of the previous entry within the `DIRBLKSIZ`-sized
/// block is stored in it (if the entry is the first in a block, the start of the block is
/// used).
pub fn ufsdirhash_lookup(
    ip: &Inode,
    name: &[u8],
    prevoffp: Option<&mut Doff>,
) -> Result<(Doff, &'static Buf), Errno> {
    // SAFETY: the inode is locked by its caller (ufs_lookup), and `dh` is not used after
    // `ufsdirhash_free`.
    let Some(dh) = (unsafe { i_dirhash(ip) }) else {
        return Err(Errno::EJUSTRETURN);
    };
    // Move this dirhash towards the end of the list if it has a score higher than the next
    // entry, and acquire the dh_mtx. Optimise the case where it's already the last by
    // performing an unlocked read of the TAILQ_NEXT pointer.
    //
    // In both cases, end up holding just dh_mtx.
    if dh.dh_onlist.get() != 0 && TailqHead::<DirhashList>::next(dh).is_some() {
        dirhashlist_lock();
        dirhash_lock(dh);
        // If the new score will be greater than that of the next entry, then move this
        // entry past it. With both mutexes held, dh_next won't go away, but its dh_score
        // could change; that's not important since it is just a hint.
        if !dh.dh_hash.get().is_null()
            && let Some(dh_next) = TailqHead::<DirhashList>::next(dh)
            && dh.dh_score.get() >= dh_next.dh_score.get()
        {
            kassert!(dh.dh_onlist.get() != 0);
            // SAFETY: under `ufsdirhash_mtx`; a dirhash with a hash is on the list, and so
            // is the one after it.
            unsafe {
                UFSDIRHASH_LIST.0.remove(dh);
                UFSDIRHASH_LIST.0.insert_after(dh_next, dh);
            }
        }
        dirhashlist_unlock();
    } else {
        // Already the last, though that could change as we wait.
        dirhash_lock(dh);
    }
    if dh.dh_hash.get().is_null() {
        dirhash_unlock(dh);
        ufsdirhash_free(ip);
        return Err(Errno::EJUSTRETURN);
    }

    // Update the score.
    if dh.dh_score.get() < DH_SCOREMAX {
        dh.dh_score.set(dh.dh_score.get() + 1);
    }

    let bmask = ip.ump().mountp().mnt_stat.get().f_iosize as i32 - 1;
    let mut blkoff: Doff = -1;
    let mut bp: Option<&'static Buf> = None;
    let mut prevoffp = prevoffp;
    'restart: loop {
        let mut slot = ufsdirhash_hash(dh, name);

        if dh.dh_seqopt.get() != 0 {
            // Sequential access optimisation. dh_seqoff contains the offset of the directory
            // entry immediately following the last entry that was looked up. Check if this
            // offset appears in the hash chain for the name we are looking for.
            let mut i = slot;
            let mut offset;
            loop {
                offset = dh.dh_entry(i);
                if offset == DIRHASH_EMPTY || offset == dh.dh_seqoff.get() {
                    break;
                }
                i = wrapincr(i, dh.dh_hlen.get());
            }
            if offset == dh.dh_seqoff.get() {
                // We found an entry with the expected offset. This is probably the entry we
                // want, but if not, the code below will turn off seqopt and retry.
                slot = i;
            } else {
                dh.dh_seqopt.set(0);
            }
        }

        loop {
            let offset = dh.dh_entry(slot);
            if offset == DIRHASH_EMPTY {
                break;
            }
            if offset == DIRHASH_DEL {
                slot = wrapincr(slot, dh.dh_hlen.get());
                continue;
            }
            dirhash_unlock(dh);

            if offset < 0 || i64::from(offset) >= ip.dip_size() as i64 {
                panic(format_args!("ufsdirhash_lookup: bad offset in hash array"));
            }
            if offset & !bmask != blkoff {
                if let Some(b) = bp.take() {
                    brelse(b);
                }
                blkoff = offset & !bmask;
                match UFS_BUFATOFF(ip, i64::from(blkoff)) {
                    Ok((b, _)) => bp = Some(b),
                    Err(_) => return Err(Errno::EJUSTRETURN),
                }
            }
            let Some(b) = bp else {
                panic(format_args!("ufsdirhash_lookup: no directory block"));
            };
            // SAFETY: the buffer is ours (busy from UFS_BUFATOFF) and mapped; the slice dies
            // before it is released or returned.
            let data = unsafe { b.data() };
            let dp = (offset & bmask) as usize;
            let reclen = i32::from(d_reclen(data, dp));
            if reclen == 0 || reclen > DIRBLK - (offset & (DIRBLK - 1)) {
                // Corrupted directory.
                brelse(b);
                return Err(Errno::EJUSTRETURN);
            }
            let namlen = d_namlen(data, dp);
            if usize::from(namlen) == name.len() && d_name(data, dp, name.len()) == name {
                // Found. Get the prev offset if needed.
                if let Some(prevoffp) = prevoffp.take() {
                    let prevoff = if offset & (DIRBLK - 1) != 0 {
                        match ufsdirhash_getprev(data, dp, offset) {
                            Some(prevoff) => prevoff,
                            None => {
                                brelse(b);
                                return Err(Errno::EJUSTRETURN);
                            }
                        }
                    } else {
                        offset
                    };
                    *prevoffp = prevoff;
                }

                // Check for sequential access, and update offset.
                if dh.dh_seqopt.get() == 0 && dh.dh_seqoff.get() == offset {
                    dh.dh_seqopt.set(1);
                }
                dh.dh_seqoff.set(offset + dirsiz(namlen) as Doff);

                return Ok((offset, b));
            }

            dirhash_lock(dh);
            if dh.dh_hash.get().is_null() {
                dirhash_unlock(dh);
                if let Some(b) = bp.take() {
                    brelse(b);
                }
                ufsdirhash_free(ip);
                return Err(Errno::EJUSTRETURN);
            }
            // When the name doesn't match in the seqopt case, go back and search normally.
            if dh.dh_seqopt.get() != 0 {
                dh.dh_seqopt.set(0);
                continue 'restart;
            }
            slot = wrapincr(slot, dh.dh_hlen.get());
        }
        break;
    }
    dirhash_unlock(dh);
    if let Some(b) = bp {
        brelse(b);
    }
    Err(Errno::ENOENT)
}

/// `ufsdirhash_findfree`: find a directory block with room for `slotneeded` bytes. Returns
/// the offset of the directory entry that begins the free space. This will either be the
/// offset of an existing entry that has free space at the end, or the offset of an entry
/// with `d_ino == 0` at the start of a `DIRBLKSIZ` block.
///
/// To use the space, the caller may need to compact existing entries in the directory. The
/// total number of bytes in all of the entries involved in the compaction is returned with
/// the offset (the C's `*slotsize`). In other words, all of the entries that must be
/// compacted are exactly contained in the region beginning at the returned offset and
/// spanning that many bytes.
///
/// Returns `None` (-1) if no space was found, indicating that the directory must be
/// extended.
pub fn ufsdirhash_findfree(ip: &Inode, slotneeded: i32) -> Option<(Doff, i32)> {
    // SAFETY: the inode is locked by its caller (ufs_lookup), and `dh` is not used after
    // `ufsdirhash_free`.
    let dh = unsafe { i_dirhash(ip) }?;
    dirhash_lock(dh);
    if dh.dh_hash.get().is_null() {
        dirhash_unlock(dh);
        ufsdirhash_free(ip);
        return None;
    }

    // Find a directory block with the desired free space.
    let need = howmany(slotneeded as usize, DIRALIGN as usize);
    let Some(dirblock) = (need..=DH_NFSTATS)
        .map(|i| dh.firstfree(i))
        .find(|&b| b != -1)
    else {
        dirhash_unlock(dh);
        return None;
    };

    kassert!(dirblock < dh.dh_nblk.get() && usize::from(dh.blkfree(dirblock)) >= need);
    dirhash_unlock(dh);
    let pos = dirblock * DIRBLK;
    let (bp, dpoff) = UFS_BUFATOFF(ip, i64::from(pos)).ok()?;
    // SAFETY: the buffer is ours (busy from UFS_BUFATOFF) and mapped; the slice dies before
    // it is released.
    let data = unsafe { bp.data() };

    // Find the first entry with free space.
    let mut i: i32 = 0;
    while i < DIRBLK {
        let dp = dpoff + i as usize;
        let reclen = i32::from(d_reclen(data, dp));
        if reclen == 0 {
            brelse(bp);
            return None;
        }
        if d_ino(data, dp) == 0 || reclen > dirsiz(d_namlen(data, dp)) as i32 {
            break;
        }
        i += reclen;
    }
    if i > DIRBLK {
        brelse(bp);
        return None;
    }
    let slotstart = pos + i;

    // Find the range of entries needed to get enough space
    let mut freebytes = 0;
    while i < DIRBLK && freebytes < slotneeded {
        let dp = dpoff + i as usize;
        let reclen = i32::from(d_reclen(data, dp));
        freebytes += reclen;
        if d_ino(data, dp) != 0 {
            freebytes -= dirsiz(d_namlen(data, dp)) as i32;
        }
        if reclen == 0 {
            brelse(bp);
            return None;
        }
        i += reclen;
    }
    if i > DIRBLK {
        brelse(bp);
        return None;
    }
    if freebytes < slotneeded {
        panic(format_args!("ufsdirhash_findfree: free mismatch"));
    }
    brelse(bp);
    Some((slotstart, pos + i - slotstart))
}

/// `ufsdirhash_enduseful`: return the start of the unused space at the end of a directory,
/// or `None` (-1) if there are no trailing unused blocks.
pub fn ufsdirhash_enduseful(ip: &Inode) -> Option<Doff> {
    // SAFETY: the inode is locked by its caller (ufs_lookup), and `dh` is not used after
    // `ufsdirhash_free`.
    let dh = unsafe { i_dirhash(ip) }?;
    dirhash_lock(dh);
    if dh.dh_hash.get().is_null() {
        dirhash_unlock(dh);
        ufsdirhash_free(ip);
        return None;
    }

    let empty = (DIRBLK / DIRALIGN) as u8;
    if dh.blkfree(dh.dh_dirblks.get() - 1) != empty {
        dirhash_unlock(dh);
        return None;
    }

    let mut i = dh.dh_dirblks.get() - 1;
    while i >= 0 && dh.blkfree(i) == empty {
        i -= 1;
    }
    dirhash_unlock(dh);
    Some((i + 1) * DIRBLK)
}

/// `ufsdirhash_add`: insert information into the hash about a new directory entry, the
/// entry named `name` at `offset`.
pub fn ufsdirhash_add(ip: &Inode, name: &[u8], offset: Doff) {
    // SAFETY: the inode is locked by its caller (ufs_direnter), and `dh` is not used after
    // `ufsdirhash_free`.
    let Some(dh) = (unsafe { i_dirhash(ip) }) else {
        return;
    };
    dirhash_lock(dh);
    if dh.dh_hash.get().is_null() {
        dirhash_unlock(dh);
        ufsdirhash_free(ip);
        return;
    }

    kassert!(offset < dh.dh_dirblks.get() * DIRBLK);
    // Normal hash usage is < 66%. If the usage gets too high then remove the hash entirely
    // and let it be rebuilt later.
    if dh.dh_hused.get() >= (dh.dh_hlen.get() * 3) / 4 {
        dirhash_unlock(dh);
        ufsdirhash_free(ip);
        return;
    }

    // Find a free hash slot (empty or deleted), and add the entry.
    let mut slot = ufsdirhash_hash(dh, name);
    while dh.dh_entry(slot) >= 0 {
        slot = wrapincr(slot, dh.dh_hlen.get());
    }
    if dh.dh_entry(slot) == DIRHASH_EMPTY {
        dh.dh_hused.set(dh.dh_hused.get() + 1);
    }
    dh.set_dh_entry(slot, offset);

    // Update the per-block summary info.
    ufsdirhash_adjfree(dh, offset, -(namesiz(name) as i32));
    dirhash_unlock(dh);
}

/// `DIRSIZ(dirp)` of the entry named `name`.
fn namesiz(name: &[u8]) -> usize {
    dirsiz(name.len() as u8)
}

/// `ufsdirhash_remove`: remove the specified directory entry from the hash. The entry to
/// remove is defined by the name `name`, which must exist at the specified `offset` within
/// the directory.
pub fn ufsdirhash_remove(ip: &Inode, name: &[u8], offset: Doff) {
    // SAFETY: the inode is locked by its caller (ufs_dirremove), and `dh` is not used after
    // `ufsdirhash_free`.
    let Some(dh) = (unsafe { i_dirhash(ip) }) else {
        return;
    };
    dirhash_lock(dh);
    if dh.dh_hash.get().is_null() {
        dirhash_unlock(dh);
        ufsdirhash_free(ip);
        return;
    }

    kassert!(offset < dh.dh_dirblks.get() * DIRBLK);
    // Find the entry
    let slot = ufsdirhash_findslot(dh, name, offset);

    // Remove the hash entry.
    ufsdirhash_delslot(dh, slot);

    // Update the per-block summary info.
    ufsdirhash_adjfree(dh, offset, namesiz(name) as i32);
    dirhash_unlock(dh);
}

/// `ufsdirhash_move`: change the offset associated with a directory entry in the hash. Used
/// when compacting directory blocks.
pub fn ufsdirhash_move(ip: &Inode, name: &[u8], oldoff: Doff, newoff: Doff) {
    // SAFETY: the inode is locked by its caller (ufs_direnter), and `dh` is not used after
    // `ufsdirhash_free`.
    let Some(dh) = (unsafe { i_dirhash(ip) }) else {
        return;
    };
    dirhash_lock(dh);
    if dh.dh_hash.get().is_null() {
        dirhash_unlock(dh);
        ufsdirhash_free(ip);
        return;
    }

    kassert!(oldoff < dh.dh_dirblks.get() * DIRBLK && newoff < dh.dh_dirblks.get() * DIRBLK);
    // Find the entry, and update the offset.
    let slot = ufsdirhash_findslot(dh, name, oldoff);
    dh.set_dh_entry(slot, newoff);
    dirhash_unlock(dh);
}

/// `ufsdirhash_newblk`: inform dirhash that the directory has grown by one block that
/// begins at `offset` (i.e. the new length is `offset + DIRBLKSIZ`).
pub fn ufsdirhash_newblk(ip: &Inode, offset: Doff) {
    // SAFETY: the inode is locked by its caller (ufs_direnter), and `dh` is not used after
    // `ufsdirhash_free`.
    let Some(dh) = (unsafe { i_dirhash(ip) }) else {
        return;
    };
    dirhash_lock(dh);
    if dh.dh_hash.get().is_null() {
        dirhash_unlock(dh);
        ufsdirhash_free(ip);
        return;
    }

    kassert!(offset == dh.dh_dirblks.get() * DIRBLK);
    let block = offset / DIRBLK;
    if block >= dh.dh_nblk.get() {
        // Out of space; must rebuild.
        dirhash_unlock(dh);
        ufsdirhash_free(ip);
        return;
    }
    dh.dh_dirblks.set(block + 1);

    // Account for the new free block.
    dh.set_blkfree(block, (DIRBLK / DIRALIGN) as u8);
    if dh.firstfree(DH_NFSTATS) == -1 {
        dh.set_firstfree(DH_NFSTATS, block);
    }
    dirhash_unlock(dh);
}

/// `ufsdirhash_dirtrunc`: inform dirhash that the directory is being truncated.
pub fn ufsdirhash_dirtrunc(ip: &Inode, offset: Doff) {
    // SAFETY: the inode is locked by its caller (ufs_direnter), and `dh` is not used after
    // `ufsdirhash_free`.
    let Some(dh) = (unsafe { i_dirhash(ip) }) else {
        return;
    };
    dirhash_lock(dh);
    if dh.dh_hash.get().is_null() {
        dirhash_unlock(dh);
        ufsdirhash_free(ip);
        return;
    }

    kassert!(offset <= dh.dh_dirblks.get() * DIRBLK);
    let block = howmany(offset as usize, DIRBLKSIZ) as i32;
    // If the directory shrinks to less than 1/8 of dh_nblk blocks (about 20% of its original
    // size due to the 50% extra added in ufsdirhash_build) then free it, and let the caller
    // rebuild if necessary.
    if block < dh.dh_nblk.get() / 8 && dh.dh_narrays.get() > 1 {
        dirhash_unlock(dh);
        ufsdirhash_free(ip);
        return;
    }

    // Remove any `first free' information pertaining to the truncated blocks. All blocks
    // we're removing should be completely unused.
    if dh.firstfree(DH_NFSTATS) >= block {
        dh.set_firstfree(DH_NFSTATS, -1);
    }
    for i in block..dh.dh_dirblks.get() {
        if dh.blkfree(i) != (DIRBLK / DIRALIGN) as u8 {
            panic(format_args!("ufsdirhash_dirtrunc: blocks in use"));
        }
    }
    for i in 0..DH_NFSTATS {
        if dh.firstfree(i) >= block {
            panic(format_args!("ufsdirhash_dirtrunc: first free corrupt"));
        }
    }
    dh.dh_dirblks.set(block);
    dirhash_unlock(dh);
}

/// `ufsdirhash_checkblock`: debugging function to check that the dirhash information about
/// a directory block matches its actual contents. Panics if a mismatch is detected.
///
/// `buf` is the in-core `DIRBLKSIZ`-sized directory block, and `offset` the offset from the
/// start of the directory of that block.
pub fn ufsdirhash_checkblock(ip: &Inode, buf: &[u8], offset: Doff) {
    if UFS_DIRHASHCHECK.load(Ordering::Relaxed) == 0 {
        return;
    }
    // SAFETY: the inode is locked by its caller (ufs_direnter, ufs_dirremove), and `dh` is
    // not used after `ufsdirhash_free`.
    let Some(dh) = (unsafe { i_dirhash(ip) }) else {
        return;
    };
    dirhash_lock(dh);
    if dh.dh_hash.get().is_null() {
        dirhash_unlock(dh);
        ufsdirhash_free(ip);
        return;
    }

    let block = offset / DIRBLK;
    if offset & (DIRBLK - 1) != 0 || block >= dh.dh_dirblks.get() {
        panic(format_args!("ufsdirhash_checkblock: bad offset"));
    }

    let mut nfree = 0;
    let mut i: i32 = 0;
    while i < DIRBLK {
        let dp = i as usize;
        let reclen = i32::from(d_reclen(buf, dp));
        if reclen == 0 || i + reclen > DIRBLK {
            panic(format_args!("ufsdirhash_checkblock: bad dir"));
        }

        if d_ino(buf, dp) == 0 {
            // XXX entries with d_ino == 0 should only occur at the start of a DIRBLKSIZ
            // block. However the ufs code is tolerant of such entries at other offsets, and
            // fsck does not fix them (the C's check is under #if 0).
            nfree += reclen;
            i += reclen;
            continue;
        }

        // Check that the entry exists (will panic if it doesn't).
        let namlen = d_namlen(buf, dp);
        ufsdirhash_findslot(dh, d_name(buf, dp, usize::from(namlen)), offset + i);

        nfree += reclen - dirsiz(namlen) as i32;
        i += reclen;
    }
    if i != DIRBLK {
        panic(format_args!("ufsdirhash_checkblock: bad dir end"));
    }

    if i32::from(dh.blkfree(block)) * DIRALIGN != nfree {
        panic(format_args!("ufsdirhash_checkblock: bad free count"));
    }

    let ffslot = blkfree2idx(nfree / DIRALIGN);
    for i in 0..=DH_NFSTATS {
        if dh.firstfree(i) == block && i != ffslot {
            panic(format_args!("ufsdirhash_checkblock: bad first-free"));
        }
    }
    if dh.firstfree(ffslot) == -1 {
        panic(format_args!(
            "ufsdirhash_checkblock: missing first-free entry"
        ));
    }
    dirhash_unlock(dh);
}

/// `ufsdirhash_hash`: hash the specified filename into a dirhash slot.
fn ufsdirhash_hash(dh: &Dirhash, name: &[u8]) -> i32 {
    let key = SiphashKey {
        k0: UFSDIRHASH_KEY[0].load(Ordering::Relaxed),
        k1: UFSDIRHASH_KEY[1].load(Ordering::Relaxed),
    };
    (SipHash24(&key, name) % dh.dh_hlen.get() as u64) as i32
}

/// `ufsdirhash_adjfree`: adjust the number of free bytes in the block containing `offset`
/// by the value specified by `diff`.
///
/// The caller must ensure we have exclusive access to `dh`; normally that means that
/// `dh_mtx` should be held, but this is also called from `ufsdirhash_build()` where
/// exclusive access can be assumed.
fn ufsdirhash_adjfree(dh: &Dirhash, offset: Doff, diff: i32) {
    // Update the per-block summary info.
    let block = offset / DIRBLK;
    kassert!(block < dh.dh_nblk.get() && block < dh.dh_dirblks.get());
    let ofidx = blkfree2idx(i32::from(dh.blkfree(block)));
    dh.set_blkfree(
        block,
        (i32::from(dh.blkfree(block)) + diff / DIRALIGN) as u8,
    );
    let nfidx = blkfree2idx(i32::from(dh.blkfree(block)));

    // Update the `first free' list if necessary.
    if ofidx != nfidx {
        // If removing, scan forward for the next block.
        if dh.firstfree(ofidx) == block {
            let next = (block + 1..dh.dh_dirblks.get())
                .find(|&i| blkfree2idx(i32::from(dh.blkfree(i))) == ofidx);
            dh.set_firstfree(ofidx, next.unwrap_or(-1));
        }

        // Make this the new `first free' if necessary
        if dh.firstfree(nfidx) > block || dh.firstfree(nfidx) == -1 {
            dh.set_firstfree(nfidx, block);
        }
    }
}

/// `ufsdirhash_findslot`: find the specified name which should have the specified offset.
/// Returns a slot number, and panics on failure.
///
/// `dh` must be locked on entry and remains so on return.
fn ufsdirhash_findslot(dh: &Dirhash, name: &[u8], offset: Doff) -> i32 {
    // mtx_assert(&dh->dh_mtx, MA_OWNED): nothing, as in C.

    // Find the entry.
    kassert!(dh.dh_hused.get() < dh.dh_hlen.get());
    let mut slot = ufsdirhash_hash(dh, name);
    while dh.dh_entry(slot) != offset && dh.dh_entry(slot) != DIRHASH_EMPTY {
        slot = wrapincr(slot, dh.dh_hlen.get());
    }
    if dh.dh_entry(slot) != offset {
        panic(format_args!(
            "ufsdirhash_findslot: '{}' not found",
            Str(name)
        ));
    }

    slot
}

/// `ufsdirhash_delslot`: remove the entry corresponding to the specified slot from the hash
/// array.
///
/// `dh` must be locked on entry and remains so on return.
fn ufsdirhash_delslot(dh: &Dirhash, slot: i32) {
    // mtx_assert(&dh->dh_mtx, MA_OWNED): nothing, as in C.

    // Mark the entry as deleted.
    dh.set_dh_entry(slot, DIRHASH_DEL);

    // If this is the end of a chain of DIRHASH_DEL slots, remove them.
    let hlen = dh.dh_hlen.get();
    let mut i = slot;
    while dh.dh_entry(i) == DIRHASH_DEL {
        i = wrapincr(i, hlen);
    }
    if dh.dh_entry(i) == DIRHASH_EMPTY {
        i = wrapdecr(i, hlen);
        while dh.dh_entry(i) == DIRHASH_DEL {
            dh.set_dh_entry(i, DIRHASH_EMPTY);
            dh.dh_hused.set(dh.dh_hused.get() - 1);
            i = wrapdecr(i, hlen);
        }
        kassert!(dh.dh_hused.get() >= 0);
    }
}

/// `ufsdirhash_getprev`: given a directory entry at `dpos` in `data` and its directory
/// offset, find the offset of the previous entry in the same `DIRBLKSIZ`-sized block.
/// Returns an offset, or `None` (-1) if there is no previous entry in the block or some
/// other problem occurred.
fn ufsdirhash_getprev(data: &[u8], dpos: usize, offset: Doff) -> Option<Doff> {
    let blkoff = offset & !(DIRBLK - 1); // offset of start of block
    let entrypos = offset & (DIRBLK - 1); // entry relative to block
    let blkbuf = dpos.checked_sub(entrypos as usize)?;
    let mut prevoff = blkoff;

    // If `offset' is the start of a block, there is no previous entry.
    if entrypos == 0 {
        return None;
    }

    // Scan from the start of the block until we get to the entry.
    let mut i = 0;
    while i < entrypos {
        let reclen = i32::from(d_reclen(data, blkbuf + i as usize));
        if reclen == 0 || i + reclen > entrypos {
            return None; // Corrupted directory.
        }
        prevoff = blkoff + i;
        i += reclen;
    }
    Some(prevoff)
}

/// `ufsdirhash_recycle`: try to free up `wanted` bytes by stealing memory from existing
/// dirhashes. Returns `true` (the C's 0) with the list locked if successful.
fn ufsdirhash_recycle(wanted: i32) -> bool {
    dirhashlist_lock();
    while wanted + UFS_DIRHASHMEM.load(Ordering::Relaxed)
        > UFS_DIRHASHMAXMEM.load(Ordering::Relaxed)
    {
        // Find a dirhash, and lock it.
        let Some(dh) = UFSDIRHASH_LIST.0.first() else {
            dirhashlist_unlock();
            return false;
        };
        dirhash_lock(dh);
        kassert!(!dh.dh_hash.get().is_null());

        // Decrement the score; only recycle if it becomes zero.
        dh.dh_score.set(dh.dh_score.get() - 1);
        if dh.dh_score.get() > 0 {
            dirhash_unlock(dh);
            dirhashlist_unlock();
            return false;
        }

        // Remove it from the list and detach its memory.
        // SAFETY: under `ufsdirhash_mtx`; `dh` is the list's first element.
        unsafe { UFSDIRHASH_LIST.0.remove(dh) };
        dh.dh_onlist.set(0);
        let hash = dh.dh_hash.replace(ptr::null_mut());
        let blkfree = dh.dh_blkfree.replace(ptr::null_mut());
        let narrays = dh.dh_narrays.get();
        let nblk = dh.dh_nblk.get();
        let mem = arrays_mem(narrays, nblk);

        // Unlock everything, free the detached memory.
        dirhash_unlock(dh);
        dirhashlist_unlock();
        free_arrays(hash, narrays, blkfree, nblk);

        // Account for the returned memory, and repeat if necessary.
        dirhashlist_lock();
        UFS_DIRHASHMEM.fetch_sub(mem, Ordering::Relaxed);
    }
    // Success; return with list locked.
    true
}

/// `ufsdirhash_init`: the pool, the list and its lock, the hash key and the default limits.
pub fn ufsdirhash_init() {
    pool_init(
        &UFSDIRHASH_POOL,
        DH_NBLKOFF as usize * size_of::<Doff>(),
        0,
        IPL_NONE,
        PR_WAITOK,
        "dirhash",
        None,
    );
    rw_init(&UFSDIRHASH_MTX, "dirhash_list");
    let mut key = [0u8; 16];
    arc4random_buf(&mut key);
    let (k0, k1) = key.split_at(8);
    for (w, k) in UFSDIRHASH_KEY.iter().zip([k0, k1]) {
        let mut b = [0u8; 8];
        b.copy_from_slice(k);
        w.store(u64::from_le_bytes(b), Ordering::Relaxed);
    }
    UFSDIRHASH_LIST.0.init();
    UFS_DIRHASHMAXMEM.store(5 * 1024 * 1024, Ordering::Relaxed);
    UFS_MINDIRHASHSIZE.store(5 * DIRBLK, Ordering::Relaxed);
}

/// `ufsdirhash_uninit`.
pub fn ufsdirhash_uninit() {
    kassert!(UFSDIRHASH_LIST.0.is_empty());
    pool_destroy(&UFSDIRHASH_POOL);
}

#[cfg(test)]
mod tests;
