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

/* <CODE> */
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
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for `ufs_dirhash.rs`: the slot and free-space bookkeeping on a hash built by
    // hand, `ufsdirhash_getprev` on a block, and, on an FFS image mounted on the host (the
    // `newfs` builder of the ffs tests), a directory of thousands of entries that is hashed,
    // looked up, shrunk, renamed into and grown with `ufsdirhash_checkblock` on, then checked
    // against a linear walk; and recycling under a small `ufs_dirhashmaxmem`.

    use core::ptr;
    use core::sync::atomic::Ordering;
    use std::boxed::Box;
    use std::collections::BTreeSet;
    use std::sync::MutexGuard;
    use std::vec::Vec;
    use std::{format, vec};

    use super::*;
    use crate::kern::kern_descrip::sys_close;
    use crate::kern::subr_xxx::nullop;
    use crate::kern::vfs_bio::{BCSTATS, BUFHEAD, BUFKVM, CLEANCACHE, biodone, bufinit};
    use crate::kern::vfs_default::vop_generic_bwrite;
    use crate::kern::vfs_init::{rootvnode, set_rootvnode, vfs_byname};
    use crate::kern::vfs_subr::{
        MOUNTLIST, bdevvp, vflushbuf, vfs_busy, vfs_mount_alloc, vfs_unbusy, vput, vref, vrele,
    };
    use crate::kern::vfs_syscalls::{
        dounmount, sys_getdents, sys_link, sys_mkdir, sys_open, sys_rename, sys_stat, sys_sync,
        sys_unlink,
    };
    use crate::kern::vfs_vops::VOP_UNLOCK;
    use crate::machine::Machine;
    use crate::machine::cpu::Cpu;
    use crate::machine::intr::{splbio, splx};
    use crate::sys::buf::{B_ERROR, B_READ};
    use crate::sys::fcntl::{O_CREAT, O_RDONLY, O_RDWR};
    use crate::sys::mount::{MNT_WAIT, Mount, VB_WAIT, VB_WRITE, VFS_ROOT, VFS_VGET};
    use crate::sys::param::DEV_BSIZE;
    use crate::sys::proc::Proc;
    use crate::sys::stat::Stat;
    use crate::sys::systm::{SyCall, SysArgs};
    use crate::sys::types::{Register, makedev};
    use crate::sys::vnode::{Vnode, VopFsyncArgs, VopInactiveArgs, VopStrategyArgs, Vops};
    use crate::ufs::ffs::ffs_extern::FFS_DIRHASH_MEM;
    use crate::ufs::ffs::ffs_vfsops::tests::newfs;
    use crate::ufs::ffs::ffs_vfsops::{ffs_mountfs, ffs_statfs, ffs_sysctl};
    use crate::ufs::ufs::inode::vtoi;

    /// A dirhash with `hlen` slots and `dirblks` directory blocks, its arrays leaked test
    /// memory, initialised as `ufsdirhash_build` initialises one (every block empty).
    /// The lock under which the mounting tests run `vfsinit`, whose `ufsdirhash_init` draws a
    /// new hash key: a test that hashes a name twice holds it, so the key stays put between.
    fn key_lock() -> MutexGuard<'static, ()> {
        crate::uvm::uvm_pmemrange::tests::LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    fn test_dh(hlen: i32, dirblks: i32) -> &'static Dirhash {
        let narrays = (hlen + DH_NBLKOFF - 1) / DH_NBLKOFF;
        let blocks: Vec<*mut Doff> = (0..narrays)
            .map(|_| Box::leak(Box::new([DIRHASH_EMPTY; DH_NBLKOFF as usize])).as_mut_ptr())
            .collect();
        let nblk = (dirblks * 3 + 1) / 2;
        let blkfree = vec![(DIRBLK / DIRALIGN) as u8; nblk as usize];
        let dh: &'static Dirhash = Box::leak(Box::new(Dirhash::new()));
        dh.dh_hash
            .set(Box::leak(blocks.into_boxed_slice()).as_mut_ptr());
        dh.dh_blkfree
            .set(Box::leak(blkfree.into_boxed_slice()).as_mut_ptr());
        dh.dh_narrays.set(narrays);
        dh.dh_hlen.set(hlen);
        dh.dh_nblk.set(nblk);
        dh.dh_dirblks.set(dirblks);
        for i in 0..DH_NFSTATS {
            dh.set_firstfree(i, -1);
        }
        dh.set_firstfree(DH_NFSTATS, 0);
        dh
    }

    #[test]
    fn hash_stays_in_range_and_depends_on_the_name() {
        let _g = key_lock();
        let dh = test_dh(3 * DH_NBLKOFF, 4);
        let mut seen = BTreeSet::new();
        for i in 0..200 {
            let name = format!("name{i}");
            let slot = ufsdirhash_hash(dh, name.as_bytes());
            assert!((0..dh.dh_hlen.get()).contains(&slot));
            assert_eq!(slot, ufsdirhash_hash(dh, name.as_bytes()));
            seen.insert(slot);
        }
        // 200 names over 768 slots: a hash worth the name spreads them.
        assert!(seen.len() > 120, "only {} distinct slots", seen.len());
    }

    #[test]
    fn slots_chain_on_collisions_and_deleted_chains_collapse() {
        let _g = key_lock();
        // Four slots, so that names collide; the chain wraps around the end.
        let dh = test_dh(4, 4);
        let add = |name: &[u8], off: Doff| {
            let mut slot = ufsdirhash_hash(dh, name);
            while dh.dh_entry(slot) >= 0 {
                slot = wrapincr(slot, dh.dh_hlen.get());
            }
            if dh.dh_entry(slot) == DIRHASH_EMPTY {
                dh.dh_hused.set(dh.dh_hused.get() + 1);
            }
            dh.set_dh_entry(slot, off);
            slot
        };
        let a = add(b"a", 0);
        let b = add(b"b", 12);
        let c = add(b"c", 24);
        assert_eq!(dh.dh_hused.get(), 3);
        for (n, off, slot) in [(&b"a"[..], 0, a), (b"b", 12, b), (b"c", 24, c)] {
            assert_eq!(ufsdirhash_findslot(dh, n, off), slot);
        }

        // Deleting an entry inside a chain leaves a DIRHASH_DEL marker that keeps the chain
        // walkable; deleting the chain's last live entries empties the markers too.
        let empty = (0..4).find(|&s| dh.dh_entry(s) == DIRHASH_EMPTY).unwrap();
        let chain: Vec<i32> = (1..4).map(|k| (empty + k) % 4).collect();
        let first = chain[0];
        ufsdirhash_delslot(dh, first);
        assert_eq!(dh.dh_entry(first), DIRHASH_DEL);
        assert_eq!(dh.dh_hused.get(), 3);
        for &s in &chain[1..] {
            let off = dh.dh_entry(s);
            let name: &[u8] = match off {
                0 => b"a",
                12 => b"b",
                _ => b"c",
            };
            assert_eq!(ufsdirhash_findslot(dh, name, off), s);
        }
        ufsdirhash_delslot(dh, chain[2]);
        assert_eq!(dh.dh_entry(chain[2]), DIRHASH_EMPTY);
        assert_eq!(dh.dh_hused.get(), 2);
        ufsdirhash_delslot(dh, chain[1]);
        // The whole chain collapses: chain[1], then the marker left at chain[0].
        assert!((0..4).all(|s| dh.dh_entry(s) == DIRHASH_EMPTY));
        assert_eq!(dh.dh_hused.get(), 0);
    }

    #[test]
    fn adjfree_keeps_the_first_free_lists() {
        let dh = test_dh(DH_NBLKOFF, 4);
        let full = DH_NFSTATS;
        assert_eq!(dh.firstfree(full), 0);

        // Block 0 loses 400 bytes: 28 words free, so it is the first block with 28 free, and
        // block 1 becomes the first entirely free one.
        ufsdirhash_adjfree(dh, 0, -400);
        assert_eq!(dh.blkfree(0), 28);
        assert_eq!(dh.firstfree(28), 0);
        assert_eq!(dh.firstfree(full), 1);
        // Block 2 likewise; block 0 stays the first with 28 free.
        ufsdirhash_adjfree(dh, 2 * DIRBLK + 100, -400);
        assert_eq!(dh.firstfree(28), 0);
        // Block 0 empties again: block 2 is now the first with 28 free, block 0 the first free.
        ufsdirhash_adjfree(dh, 12, 400);
        assert_eq!(dh.blkfree(0), (DIRBLK / DIRALIGN) as u8);
        assert_eq!(dh.firstfree(28), 2);
        assert_eq!(dh.firstfree(full), 0);
        // Small changes within the top bucket (more than DH_NFSTATS words free) move nothing.
        ufsdirhash_adjfree(dh, 3 * DIRBLK, -16);
        assert_eq!(dh.blkfree(3), 124);
        assert_eq!(dh.firstfree(full), 0);
        assert_eq!(blkfree2idx(124), full);
    }

    #[test]
    fn getprev_finds_the_previous_entry_in_the_block() {
        // A block at directory offset 512 with entries at 0 (12 bytes), 12 (16) and 28 (rest).
        let mut blk = vec![0u8; DIRBLKSIZ];
        let mut put = |off: usize, reclen: u16| {
            blk[off..off + 4].copy_from_slice(&5u32.to_ne_bytes());
            blk[off + 4..off + 6].copy_from_slice(&reclen.to_ne_bytes());
            blk[off + 7] = 1;
        };
        put(0, 12);
        put(12, 16);
        put(28, (DIRBLKSIZ - 28) as u16);
        assert_eq!(ufsdirhash_getprev(&blk, 28, DIRBLK + 28), Some(DIRBLK + 12));
        assert_eq!(ufsdirhash_getprev(&blk, 12, DIRBLK + 12), Some(DIRBLK));
        assert_eq!(ufsdirhash_getprev(&blk, 0, DIRBLK), None);
        // An entry offset that does not fall on an entry boundary is corruption.
        assert_eq!(ufsdirhash_getprev(&blk, 20, DIRBLK + 20), None);
    }

    /// The disk the strategy below reads and writes.
    static DISK: std::sync::Mutex<Vec<u8>> = std::sync::Mutex::new(Vec::new());

    /// The fake disk's strategy: a synchronous transfer between the buffer and the image.
    fn disk_strategy(ap: &mut VopStrategyArgs) -> Result<(), Errno> {
        let bp = ap.a_bp;
        let off = bp.b_blkno.get() as usize * DEV_BSIZE;
        let len = bp.b_bcount.get() as usize;
        {
            let mut d = DISK.lock().unwrap_or_else(|e| e.into_inner());
            if off + len > d.len() {
                bp.b_error.set(Some(Errno::EIO));
                bp.set(B_ERROR);
            } else {
                // SAFETY: the buffer is busy for this transfer and mapped.
                let data = unsafe { bp.data() };
                if bp.isset(B_READ) {
                    data.copy_from_slice(&d[off..off + len]);
                } else {
                    d[off..off + len].copy_from_slice(data);
                }
                bp.b_resid.set(0);
            }
        }
        let s = splbio();
        biodone(bp);
        splx(s);
        Ok(())
    }

    fn disk_fsync(ap: &mut VopFsyncArgs<'_>) -> Result<(), Errno> {
        vflushbuf(ap.a_vp, ap.a_waitfor == MNT_WAIT);
        Ok(())
    }

    fn disk_inactive(ap: &mut VopInactiveArgs<'_>) -> Result<(), Errno> {
        VOP_UNLOCK(ap.a_vp)
    }

    /// The fake disk's block device operations.
    static DISK_VOPS: Vops = Vops {
        vop_open: Some(|_| nullop()),
        vop_close: Some(|_| nullop()),
        vop_lock: Some(|_| nullop()),
        vop_unlock: Some(|_| nullop()),
        vop_islocked: Some(|_| 0),
        vop_inactive: Some(disk_inactive),
        vop_reclaim: Some(|_| nullop()),
        vop_strategy: Some(disk_strategy),
        vop_bwrite: Some(vop_generic_bwrite),
        vop_fsync: Some(disk_fsync),
        ..Vops::EMPTY
    };

    /// Memory, the vfs, a fresh buffer cache, the image as the disk and the thread as
    /// `curproc` (as the ffs tests set up), with the dirhash checks on and no hash memory in use.
    fn setup(image: Vec<u8>) -> (MutexGuard<'static, ()>, &'static Proc) {
        let (g, p) = crate::kern::vfs_subr::tests::setup();
        Machine::set_curproc(Machine::curcpu(), p);
        let limit: &'static crate::sys::resourcevar::Plimit =
            Box::leak(Box::new(crate::sys::resourcevar::Plimit::new()));
        for l in &limit.pl_rlimit {
            l.set(crate::sys::resource::Rlimit {
                rlim_cur: crate::sys::resource::RLIM_INFINITY,
                rlim_max: crate::sys::resource::RLIM_INFINITY,
            });
        }
        limit.pl_rlimit[crate::sys::resource::RLIMIT_NOFILE].set(crate::sys::resource::Rlimit {
            rlim_cur: 128,
            rlim_max: 128,
        });
        p.process().ps_limit.set(limit);
        p.p_limit.set(limit);

        BUFHEAD.0.init();
        for c in [
            &BCSTATS.numbufs,
            &BCSTATS.numbufpages,
            &BCSTATS.numdirtypages,
            &BCSTATS.numcleanpages,
            &BCSTATS.pendingwrites,
            &BCSTATS.pendingreads,
            &BCSTATS.numwrites,
            &BCSTATS.numreads,
            &BCSTATS.cachehits,
            &BCSTATS.busymapped,
            &BCSTATS.delwribufs,
        ] {
            c.store(0, Ordering::Relaxed);
        }
        CLEANCACHE.hotbufpages.set(0);
        CLEANCACHE.warmbufpages.set(0);
        CLEANCACHE.cachepages.set(0);
        BUFKVM.store(0, Ordering::Relaxed);
        crate::conf::param::bufpages.store(0, Ordering::Relaxed);
        bufinit();

        *DISK.lock().unwrap_or_else(|e| e.into_inner()) = image;
        // Hashes of earlier tests lived in the memory just reset (vfsinit ran ufsdirhash_init).
        UFS_DIRHASHMEM.store(0, Ordering::Relaxed);
        UFS_DIRHASHCHECK.store(1, Ordering::Relaxed);
        (g, p)
    }

    fn teardown() {
        UFS_DIRHASHCHECK.store(0, Ordering::Relaxed);
        Machine::set_curproc(Machine::curcpu(), ptr::null());
    }

    /// Mounts the disk at `/`, as the ffs tests do.
    fn mount_root(p: &'static Proc) -> &'static Mount {
        let devvp = bdevvp(makedev(17, 1)).unwrap().unwrap();
        devvp.v_op.set(Some(&DISK_VOPS));
        let mp = vfs_mount_alloc(None, vfs_byname(b"ffs").unwrap());
        mp.update_stat(|sp| sp.f_mntonname[0] = b'/');
        ffs_mountfs(devvp, mp, p).unwrap();
        let mut st = mp.mnt_stat.get();
        ffs_statfs(mp, &mut st, p).unwrap();
        mp.mnt_stat.set(st);
        vfs_unbusy(mp);
        // SAFETY: a new mount on no list.
        unsafe { MOUNTLIST.0.insert_tail(mp) };
        let root = VFS_ROOT(mp).unwrap();
        set_rootvnode(Some(root));
        p.fd().fd_cdir.set(Some(root));
        vref(root);
        let _ = VOP_UNLOCK(root);
        mp
    }

    /// Undoes `mount_root` and unmounts.
    fn unmount_root(p: &'static Proc, mp: &'static Mount) {
        if let Some(cdir) = p.fd().fd_cdir.take() {
            vrele(cdir);
        }
        if let Some(root) = rootvnode() {
            set_rootvnode(None);
            vrele(root);
        }
        vfs_busy(mp, VB_WRITE | VB_WAIT).unwrap();
        dounmount(mp, 0, p).unwrap();
    }

    /// A system call with up to six arguments; `retval[0]`.
    fn sys(f: SyCall, p: &Proc, args: &[usize]) -> Result<isize, Errno> {
        let mut v: SysArgs = [0; 6];
        for (slot, a) in v.iter_mut().zip(args) {
            *slot = *a as Register;
        }
        let mut rv = [0; 2];
        f(p, &v, &mut rv)?;
        Ok(rv[0])
    }

    /// `s` as a NUL-terminated path.
    fn cpath(s: &str) -> Vec<u8> {
        let mut v = s.as_bytes().to_vec();
        v.push(0);
        v
    }

    /// `stat(2)`: the inode number of `path`.
    fn ino_of(p: &Proc, path: &str) -> Result<u64, Errno> {
        let c = cpath(path);
        let mut sb = [0u8; Stat::SIZE];
        sys(
            sys_stat,
            p,
            &[c.as_ptr() as usize, sb.as_mut_ptr() as usize],
        )?;
        let o = core::mem::offset_of!(Stat, st_ino);
        Ok(u64::from_ne_bytes(sb[o..o + 8].try_into().unwrap()))
    }

    fn link(p: &Proc, from: &str, to: &str) -> Result<isize, Errno> {
        let (a, b) = (cpath(from), cpath(to));
        sys(sys_link, p, &[a.as_ptr() as usize, b.as_ptr() as usize])
    }

    fn rename(p: &Proc, from: &str, to: &str) -> Result<isize, Errno> {
        let (a, b) = (cpath(from), cpath(to));
        sys(sys_rename, p, &[a.as_ptr() as usize, b.as_ptr() as usize])
    }

    fn unlink(p: &Proc, path: &str) -> Result<isize, Errno> {
        let c = cpath(path);
        sys(sys_unlink, p, &[c.as_ptr() as usize])
    }

    fn mkdir(p: &Proc, path: &str) {
        let c = cpath(path);
        sys(sys_mkdir, p, &[c.as_ptr() as usize, 0o755]).unwrap();
    }

    fn create(p: &Proc, path: &str) {
        let c = cpath(path);
        let fd = sys(
            sys_open,
            p,
            &[c.as_ptr() as usize, (O_RDWR | O_CREAT) as usize, 0o644],
        )
        .unwrap();
        sys(sys_close, p, &[fd as usize]).unwrap();
    }

    /// The names in directory `path` (without `.` and `..`) read with getdents(2), which walks
    /// the blocks linearly and never consults the hash.
    fn list_dir(p: &Proc, path: &str) -> BTreeSet<Vec<u8>> {
        let c = cpath(path);
        let fd = sys(sys_open, p, &[c.as_ptr() as usize, O_RDONLY as usize, 0]).unwrap();
        let mut buf = vec![0u8; 8192];
        let mut names = BTreeSet::new();
        loop {
            let n = sys(
                sys_getdents,
                p,
                &[fd as usize, buf.as_mut_ptr() as usize, buf.len()],
            )
            .unwrap();
            if n == 0 {
                break;
            }
            let mut off = 0;
            while off < n as usize {
                let reclen = u16::from_ne_bytes([buf[off + 16], buf[off + 17]]) as usize;
                let namlen = buf[off + 19] as usize;
                let nm = &buf[off + 24..off + 24 + namlen];
                if nm != b"." && nm != b".." {
                    assert!(names.insert(nm.to_vec()), "duplicate entry");
                }
                off += reclen;
            }
        }
        sys(sys_close, p, &[fd as usize]).unwrap();
        names
    }

    /// Runs `f` on the in-core inode of directory `path`.
    fn with_dir_inode<R>(
        p: &Proc,
        mp: &'static Mount,
        path: &str,
        f: impl FnOnce(&Inode) -> R,
    ) -> R {
        let ino = ino_of(p, path).unwrap();
        let vp: &'static Vnode = VFS_VGET(mp, ino).unwrap();
        let r = f(vtoi(vp));
        vput(vp);
        r
    }

    /// Whether directory `path` has an intact hash.
    fn hashed(p: &Proc, mp: &'static Mount, path: &str) -> bool {
        with_dir_inode(p, mp, path, |ip| {
            // SAFETY: the vnode is locked by VFS_VGET for the duration of the closure.
            unsafe { i_dirhash(ip) }.is_some_and(|dh| !dh.dh_hash.get().is_null())
        })
    }

    /// `sysctl vfs.ffs.dirhash_mem`.
    fn sysctl_dirhash_mem(p: &Proc) -> i32 {
        let mut v = 0i32;
        let mut len = size_of::<i32>();
        ffs_sysctl(
            &[FFS_DIRHASH_MEM],
            ptr::from_mut(&mut v) as usize,
            &mut len,
            0,
            0,
            p,
        )
        .unwrap();
        assert_eq!(len, size_of::<i32>());
        v
    }

    /// Every name in `names` resolves to `ino` in `dir`, and a few names not there do not.
    fn check_lookups(p: &Proc, dir: &str, names: &BTreeSet<Vec<u8>>, ino: u64, tag: &str) {
        for n in names {
            let path = format!("{dir}/{}", core::str::from_utf8(n).unwrap());
            assert_eq!(ino_of(p, &path), Ok(ino), "{path}");
        }
        for i in 0..20 {
            let path = format!("{dir}/absent-{tag}-{i}");
            assert_eq!(ino_of(p, &path), Err(Errno::ENOENT), "{path}");
        }
    }

    /// The name of entry `i` of the big directory: lengths vary from 1 to 30 bytes, so that the
    /// entries have different record sizes.
    fn big_name(i: usize) -> std::string::String {
        let pad = "x".repeat(i % 23);
        format!("n{i}{pad}")
    }

    #[test]
    fn a_large_directory_is_hashed_and_kept_in_step() {
        const N: usize = 3000;
        let (_g, p) = setup(newfs::Image::new(newfs::FFS2_4M).finish());
        let mp = mount_root(p);
        assert_eq!(UFS_MINDIRHASHSIZE.load(Ordering::Relaxed), 5 * DIRBLK);
        assert_eq!(sysctl_dirhash_mem(p), 0);

        mkdir(p, "/d");
        create(p, "/d/target");
        let target = ino_of(p, "/d/target").unwrap();
        let mut names: BTreeSet<Vec<u8>> = BTreeSet::new();
        names.insert(b"target".to_vec());
        for i in 0..N {
            let n = big_name(i);
            link(p, "/d/target", &format!("/d/{n}")).unwrap();
            names.insert(n.into_bytes());
        }
        // The creations past ufs_mindirhashsize went through the hash.
        assert!(hashed(p, mp, "/d"));
        let mem = sysctl_dirhash_mem(p);
        assert!(mem > 0);
        assert_eq!(mem, UFS_DIRHASHMEM.load(Ordering::Relaxed));
        check_lookups(p, "/d", &names, target, "a");
        // A name that exists cannot be created again.
        assert_eq!(
            link(p, "/d/target", &format!("/d/{}", big_name(7))),
            Err(Errno::EEXIST)
        );

        // Remove every third entry (DELETE lookups through the hash, with the previous
        // entry's offset), so that blocks get holes.
        for i in (0..N).step_by(3) {
            let n = big_name(i);
            unlink(p, &format!("/d/{n}")).unwrap();
            names.remove(n.as_bytes());
        }
        assert!(hashed(p, mp, "/d"));
        check_lookups(p, "/d", &names, target, "b");

        // Rename some entries to longer names inside the directory: removals, and creations
        // that reuse the holes (ufsdirhash_findfree, compaction with ufsdirhash_move).
        for i in (1..N).step_by(7) {
            if i % 3 == 0 {
                continue;
            }
            let (from, to) = (big_name(i), format!("renamed-{i}-{}", "y".repeat(i % 17)));
            rename(p, &format!("/d/{from}"), &format!("/d/{to}")).unwrap();
            names.remove(from.as_bytes());
            names.insert(to.into_bytes());
        }
        // And add more, past the end of the directory (ufsdirhash_newblk).
        for i in N..N + 1000 {
            let n = big_name(i);
            link(p, "/d/target", &format!("/d/{n}")).unwrap();
            names.insert(n.into_bytes());
        }
        assert!(hashed(p, mp, "/d"));
        check_lookups(p, "/d", &names, target, "c");

        // The directory as a linear walk sees it agrees with the names.
        assert_eq!(list_dir(p, "/d"), names);

        // Without the hash (the size threshold raised: ufsdirhash_build frees it), every name
        // still resolves by the linear search, and the memory goes back.
        UFS_MINDIRHASHSIZE.store(i32::MAX, Ordering::Relaxed);
        check_lookups(p, "/d", &names, target, "d");
        assert!(!hashed(p, mp, "/d"));
        assert_eq!(sysctl_dirhash_mem(p), 0);
        UFS_MINDIRHASHSIZE.store(5 * DIRBLK, Ordering::Relaxed);

        // Rebuilt from the blocks on the next lookup: a hash of what is on disk.
        check_lookups(p, "/d", &names, target, "e");
        assert!(hashed(p, mp, "/d"));

        // Empty the directory but for `target`: the truncations (ufsdirhash_dirtrunc) follow.
        let all: Vec<Vec<u8>> = names.iter().filter(|n| *n != b"target").cloned().collect();
        for n in &all {
            unlink(p, &format!("/d/{}", core::str::from_utf8(n).unwrap())).unwrap();
        }
        create(p, "/d/last");
        let mut left = BTreeSet::new();
        left.insert(b"target".to_vec());
        left.insert(b"last".to_vec());
        assert_eq!(list_dir(p, "/d"), left);

        sys(sys_sync, p, &[]).unwrap();
        unmount_root(p, mp);
        assert_eq!(
            UFS_DIRHASHMEM.load(Ordering::Relaxed),
            0,
            "reclaim freed the hash"
        );
        newfs::check(&DISK.lock().unwrap(), true);

        // Mounted again, the directory reads back.
        let mp = mount_root(p);
        assert_eq!(ino_of(p, "/d/last").map(|i| i != target), Ok(true));
        assert_eq!(ino_of(p, "/d/target"), Ok(target));
        unmount_root(p, mp);
        teardown();
    }

    #[test]
    fn recycling_steals_the_least_used_hash() {
        const N: usize = 400;
        let (_g, p) = setup(newfs::Image::new(newfs::FFS2_4M).finish());
        let mp = mount_root(p);

        create(p, "/target");
        let target = ino_of(p, "/target").unwrap();
        let mut names = BTreeSet::new();
        for i in 0..N {
            names.insert(format!("e{i:03}").into_bytes());
        }
        for d in ["/a", "/b", "/c"] {
            mkdir(p, d);
            for n in &names {
                link(
                    p,
                    "/target",
                    &format!("{d}/{}", core::str::from_utf8(n).unwrap()),
                )
                .unwrap();
            }
        }
        assert!(hashed(p, mp, "/a") && hashed(p, mp, "/b") && hashed(p, mp, "/c"));

        // Drop the hashes (they were built while the directories grew), then measure one built
        // at the final size: the three directories are the same size, so each costs that.
        UFS_MINDIRHASHSIZE.store(i32::MAX, Ordering::Relaxed);
        for d in ["/a", "/b", "/c"] {
            assert_eq!(ino_of(p, &format!("{d}/nothere")), Err(Errno::ENOENT));
        }
        assert_eq!(UFS_DIRHASHMEM.load(Ordering::Relaxed), 0);
        UFS_MINDIRHASHSIZE.store(5 * DIRBLK, Ordering::Relaxed);
        assert_eq!(ino_of(p, "/a/nothere2"), Err(Errno::ENOENT));
        let one = UFS_DIRHASHMEM.load(Ordering::Relaxed);
        assert!(one > 0);
        // Allow two and a half hashes.
        let maxmem = 2 * one + one / 2;
        UFS_DIRHASHMAXMEM.store(maxmem, Ordering::Relaxed);

        // Look-ups in /a and /b hash them; /c does not fit.
        check_lookups(p, "/a", &names, target, "a");
        check_lookups(p, "/b", &names, target, "b");
        assert!(hashed(p, mp, "/a") && hashed(p, mp, "/b"));
        assert_eq!(UFS_DIRHASHMEM.load(Ordering::Relaxed), 2 * one);

        // Every look-up in /c that tries to build its hash takes a point off the score of the
        // least recently used hash (/a, at the head of the list); at zero, /a's memory goes to
        // /c. Each look-up is of a new name, so that the name cache does not answer it.
        let mut tries = 0;
        while !hashed(p, mp, "/c") {
            assert_eq!(ino_of(p, &format!("/c/miss{tries}")), Err(Errno::ENOENT));
            assert!(UFS_DIRHASHMEM.load(Ordering::Relaxed) <= maxmem);
            tries += 1;
            assert!(tries < 200, "/c never got a hash");
        }
        assert!(tries > 1, "recycling waits for the score to run out");
        assert!(!hashed(p, mp, "/a"), "/a's hash was recycled");
        assert!(hashed(p, mp, "/b"));
        assert!(UFS_DIRHASHMEM.load(Ordering::Relaxed) <= maxmem);

        // Every directory still answers correctly, hashed or not.
        check_lookups(p, "/c", &names, target, "c");
        check_lookups(p, "/a", &names, target, "a2");
        check_lookups(p, "/b", &names, target, "b2");
        assert!(UFS_DIRHASHMEM.load(Ordering::Relaxed) <= maxmem);

        unmount_root(p, mp);
        assert_eq!(UFS_DIRHASHMEM.load(Ordering::Relaxed), 0);
        UFS_DIRHASHMAXMEM.store(5 * 1024 * 1024, Ordering::Relaxed);
        teardown();
    }
}
/* </TESTS> */
