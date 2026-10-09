/* <LICENSES> */
/*
 * Copyright (c) 1994 Christopher G. Demetriou
 * Copyright (c) 1982, 1986, 1989, 1993
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
 *	@(#)vfs_bio.c	8.6 (Berkeley) 1/11/94
 */
/*
 * Copyright (c) 2014 Ted Unangst <tedu@openbsd.org>
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

/* <CODE> */
//! The buffer cache: `bread`/`bwrite` and their friends over the per-vnode buffers, the
//! cleaner thread (`buf_daemon`) that writes delayed buffers out, and the 2Q cache of clean
//! buffers (hot, warm and cold queues) that decides which ones to throw away.
//!
//! Upstream: sys/kern/vfs_bio.c @ 3ce1f3f79392
//!
//! Some references:
//! - Bach: The Design of the UNIX Operating System (Prentice Hall, 1986)
//! - Leffler, et al.: The Design and Implementation of the 4.3BSD UNIX Operating System
//!   (Addison Welley, 1989)
//!
//! ## How a file system uses it
//! - `let (bp, error) = bread(vp, lblkno, size)`: the buffer comes back busy in both cases,
//!   as the C's `*bpp` does; on error the caller `brelse`s it.
//! - `getblk(vp, lblkno, size, 0, INFSLP)` for a block it will overwrite; `bwrite`,
//!   `bawrite` or `bdwrite` to write it (they release it), `brelse` to give it back unchanged.
//! - The file system's `VOP_STRATEGY` maps `b_lblkno` to `b_blkno` (`VOP_BMAP`) and hands
//!   the buffer to its device vnode's `VOP_STRATEGY` (`spec_strategy`), whose driver calls
//!   `biodone` when the transfer is over.
//!
//! ## Deviations
//! - `bread`, `breadn` and `bread_cluster` return the buffer with the result, `(bp,
//!   Result<(), Errno>)`, instead of filling `*bpp`: the C always fills it.
//! - `bcstats` is [`BCSTATS`], `struct bcachestats` as atomics with a `snapshot()` for
//!   `vfs.generic.bcachestat` (as `nchstats`); the tunables (`lodirtypages`, `targetpages`,
//!   ...) are atomics, `bufhead`, `cleancache` and `dirtyqueue` `Sync` statics changed at
//!   `splbio`.
//! - `getblk` and `buf_get` return `Option` (the C's NULL); `incore` likewise.
//! - The cleaner's `TRACEPOINT`s (`dt(4)`) are not configured, nor is `HIBERNATE`
//!   (`hibernate_suspend_bufcache`); the DDB printer `bcstats_print` waits for the ddb
//!   command loop (`db_command.c`).
//! - `curproc->p_ru.ru_inblock++`/`ru_oublock++` are skipped when there is no `curproc`
//!   (the C always has one).
//! - `pool_get(PR_WAITOK)` and `uvm_pagealloc_multi(UVM_PLA_WAITOK)` cannot sleep yet
//!   (`subr_pool.rs`, `uvm_pdaemon.rs`): `buf_get` returns NULL when the pool fails, which
//!   `getblk` retries as the C does after a sleep, and `buf_alloc_pages` panics as the C does
//!   when even the waiting allocation fails.

use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicI32, AtomicI64, AtomicPtr, AtomicUsize, Ordering};

use crate::kassert;
use crate::kern::kern_bufq::{bufq_done, bufq_wait};
use crate::kern::kern_malloc::{free, mallocarray};
use crate::kern::kern_synch::{tsleep_nsec, wakeup};
use crate::kern::sched_bsd::r#yield;
use crate::kern::subr_pool::{pool_get, pool_init, pool_put};
use crate::kern::subr_prf::panic;
use crate::kern::vfs_biomem::{
    buf_acquire, buf_acquire_nomap, buf_alloc_pages, buf_dealloc_mem, buf_fix_mapping, buf_map,
    buf_mem_init, buf_release,
};
use crate::kern::vfs_subr::{bgetvp, brelvp, reassignbuf, vwakeup};
use crate::kern::vfs_sync::syncerproc;
use crate::kern::vfs_vops::{VOP_BMAP, VOP_BWRITE, VOP_STRATEGY};
use crate::machine::Machine;
use crate::machine::cpu::curproc;
use crate::machine::intr::{IPL_BIO, splassert, splbio, splx};
use crate::machine::pmap::Pmap;
use crate::sys::buf::{
    B_AGE, B_ASYNC, B_BC, B_BUSY, B_CACHE, B_CALL, B_COLD, B_DEFERRED, B_DELWRI, B_DONE, B_EINTR,
    B_ERROR, B_INVAL, B_NEEDCOMMIT, B_NOCACHE, B_PHYS, B_RAW, B_READ, B_WANTED, B_WARM,
    B_WRITEINPROG, BCACHE_MIN, BList, Buf, Bufcache, Bufhead, Bufqueue, RESERVE_PAGES,
    RESERVE_SLOTS, unclean_pages,
};
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_NOWAIT, M_TEMP};
use crate::sys::mount::{Bcachestats, MNT_ASYNC, Mount};
use crate::sys::param::{MAXPHYS, NODEV, PAGE_SIZE, PRIBIO, btodb};
use crate::sys::pool::{PR_NOWAIT, PR_WAITOK, PR_ZERO, Pool};
use crate::sys::proc::Proc;
use crate::sys::queue::ListHead;
use crate::sys::sched::sched_pause;
use crate::sys::systm::{INFSLP, PHYSMEM};
use crate::sys::types::Daddr;
use crate::sys::vnode::{VBIOERROR, VBLK, VREG, Vnode};
use crate::uvm::uvm_extern::UvmConstraintRange;
use crate::uvm::uvm_init::UVMEXP;
use crate::uvm::uvm_km::NO_CONSTRAINT;
use crate::uvm::uvm_object::{BUFCACHE_PAGER, uvm_obj_init};
use crate::uvm::uvm_page::{uvm_pagecount, uvm_pagelookup, uvm_pagerealloc};
use crate::uvm::uvm_param::{VM_KERNEL_SPACE_SIZE, atop, ptoa, round_page};

/// `bcstats`: the buffer cache counters (`struct bcachestats`), as atomics.
pub struct Bcstats {
    /// `numbufs`: number of buffers allocated.
    pub numbufs: AtomicI64,
    /// `numbufpages`: number of pages in buffer cache.
    pub numbufpages: AtomicI64,
    /// `numdirtypages`: number of dirty free pages.
    pub numdirtypages: AtomicI64,
    /// `numcleanpages`: number of clean free pages.
    pub numcleanpages: AtomicI64,
    /// `pendingwrites`: number of pending writes.
    pub pendingwrites: AtomicI64,
    /// `pendingreads`: number of pending reads.
    pub pendingreads: AtomicI64,
    /// `numwrites`: total writes started.
    pub numwrites: AtomicI64,
    /// `numreads`: total reads started.
    pub numreads: AtomicI64,
    /// `cachehits`: total reads found in cache.
    pub cachehits: AtomicI64,
    /// `busymapped`: number of busy and mapped buffers.
    pub busymapped: AtomicI64,
    /// `delwribufs`: delayed write buffers.
    pub delwribufs: AtomicI64,
    /// `kvaslots`: kva slots total.
    pub kvaslots: AtomicI64,
    /// `kvaslots_avail`: available kva slots.
    pub kvaslots_avail: AtomicI64,
}

impl Bcstats {
    /// Every counter zero.
    pub const fn new() -> Self {
        Self {
            numbufs: AtomicI64::new(0),
            numbufpages: AtomicI64::new(0),
            numdirtypages: AtomicI64::new(0),
            numcleanpages: AtomicI64::new(0),
            pendingwrites: AtomicI64::new(0),
            pendingreads: AtomicI64::new(0),
            numwrites: AtomicI64::new(0),
            numreads: AtomicI64::new(0),
            cachehits: AtomicI64::new(0),
            busymapped: AtomicI64::new(0),
            delwribufs: AtomicI64::new(0),
            kvaslots: AtomicI64::new(0),
            kvaslots_avail: AtomicI64::new(0),
        }
    }

    /// The counters as the `struct bcachestats` `vfs.generic.bcachestat` copies out.
    pub fn snapshot(&self) -> Bcachestats {
        let r = |c: &AtomicI64| c.load(Ordering::Relaxed);
        Bcachestats {
            numbufs: r(&self.numbufs),
            numbufpages: r(&self.numbufpages),
            numdirtypages: r(&self.numdirtypages),
            numcleanpages: r(&self.numcleanpages),
            pendingwrites: r(&self.pendingwrites),
            pendingreads: r(&self.pendingreads),
            numwrites: r(&self.numwrites),
            numreads: r(&self.numreads),
            cachehits: r(&self.cachehits),
            busymapped: r(&self.busymapped),
            delwribufs: r(&self.delwribufs),
            kvaslots: r(&self.kvaslots),
            kvaslots_avail: r(&self.kvaslots_avail),
        }
    }
}

impl Default for Bcstats {
    fn default() -> Self {
        Self::new()
    }
}

// SAFETY: `#[repr(C)]`, thirteen `i64`s, no padding.
unsafe impl crate::sys::sysctl::SysctlPlain for Bcachestats {}

/// `bufhead`'s type: the list of every buffer, a `Sync` static.
pub struct BufheadStatic(pub Bufhead);

// SAFETY: changed at `splbio` under the kernel lock, as in C.
unsafe impl Sync for BufheadStatic {}

/// `dirtyqueue`'s type: a `Sync` static queue.
pub struct BufqueueStatic(pub Bufqueue);

// SAFETY: changed at `splbio` under the kernel lock, as in C.
unsafe impl Sync for BufqueueStatic {}

/// `high_constraint`: the range of memory above the DMA range (set by the machine code that
/// has one; unused by this file).
pub static HIGH_CONSTRAINT: UvmConstraintRange = UvmConstraintRange {
    ucr_low: crate::sys::types::Paddr::new(0),
    ucr_high: crate::sys::types::Paddr::new(0),
};

/// `nobuffers`: the syncer or the cleaner sleeps for buffers.
pub static NOBUFFERS: AtomicI32 = AtomicI32::new(0);
/// `needbuffer`: threads sleeping for the cache to shrink.
pub static NEEDBUFFER: AtomicI32 = AtomicI32::new(0);

/// `bufpool`: buffer pool for I/O buffers.
pub static BUFPOOL: Pool = Pool::new();
/// `bufhead`: all allocated buffers.
pub static BUFHEAD: BufheadStatic = BufheadStatic(ListHead::new());

/// `bcstats`: counters.
pub static BCSTATS: Bcstats = Bcstats::new();
/// `lodirtypages`: dirty page count low water mark.
pub static LODIRTYPAGES: AtomicI64 = AtomicI64::new(0);
/// `hidirtypages`: dirty page count high water mark.
pub static HIDIRTYPAGES: AtomicI64 = AtomicI64::new(0);
/// `targetpages`: target number of pages for cache size.
pub static TARGETPAGES: AtomicI64 = AtomicI64::new(0);
/// `buflowpages`: smallest size cache allowed.
pub static BUFLOWPAGES: AtomicI64 = AtomicI64::new(0);
/// `bufhighpages`: largest size cache allowed.
pub static BUFHIGHPAGES: AtomicI64 = AtomicI64::new(0);
/// `bufbackpages`: minimum number of pages we shrink when asked to.
pub static BUFBACKPAGES: AtomicI64 = AtomicI64::new(0);

/// `bufkvm`: the kernel virtual space for mapping buffers (0: let `bufinit` decide).
pub static BUFKVM: AtomicUsize = AtomicUsize::new(0);

/// `cleanerproc`: the cleaner thread.
pub static CLEANERPROC: AtomicPtr<Proc> = AtomicPtr::new(ptr::null_mut());
/// `bd_req`: sleep point for cleaner daemon.
pub static BD_REQ: AtomicI32 = AtomicI32::new(0);

/// `cleancache`: the 2Q cache of clean buffers.
pub static CLEANCACHE: Bufcache = Bufcache::new();
/// `dirtyqueue`: the delayed-write buffers, oldest first.
pub static DIRTYQUEUE: BufqueueStatic = BufqueueStatic(Bufqueue::new());

/// `cleanerproc`, the thread `buf_daemon` runs in (NULL before `main` creates it).
pub fn cleanerproc() -> *const Proc {
    CLEANERPROC.load(Ordering::Relaxed)
}

/// The `bcstats` counter `c` plus `d`.
fn bc_add(c: &AtomicI64, d: i64) {
    c.fetch_add(d, Ordering::Relaxed);
}

/// `curproc`'s address, NULL when no thread runs (early boot).
fn curproc_ptr() -> *const Proc {
    curproc().map_or(ptr::null(), ptr::from_ref)
}

/// `bp->b_vp`, which a buffer on its way to the disk has.
fn b_vp(bp: &Buf) -> &'static Vnode {
    match bp.b_vp.get() {
        Some(vp) => vp,
        None => panic(format_args!("buf {:p}: no b_vp", bp)),
    }
}

/// `buf_put`: frees a buffer that is on no list (its memory first; the header goes back to
/// `bufpool`, or later when `buf_dealloc_mem` keeps it for its kva).
pub fn buf_put(bp: &'static Buf) {
    splassert(IPL_BIO, "buf_put");

    #[cfg(feature = "diagnostic")]
    {
        if !bp.b_pobj.get().is_null() {
            kassert!(bp.b_bufsize.get() > 0);
        }
        if bp.isset(B_DELWRI) {
            panic(format_args!("buf_put: releasing dirty buffer"));
        }
        if bp.b_onfreelist.get() {
            panic(format_args!("buf_put: still on the free list"));
        }
        if bp.b_onvnbufs.get() {
            panic(format_args!("buf_put: still on the vnode list"));
        }
    }

    // SAFETY: every buffer out of `buf_get` is on `bufhead` until here.
    unsafe { ListHead::<BList>::remove(bp) };
    bc_add(&BCSTATS.numbufs, -1);

    if buf_dealloc_mem(bp) != 0 {
        return;
    }
    pool_put(&BUFPOOL, NonNull::from(bp).cast());
}

/// Initialize buffers and hash links for buffers.
pub fn bufinit() {
    use crate::conf::param::{bufcachepercent, bufpages};

    let mut pages = uvm_pagecount(&NO_CONSTRAINT) as u64;
    // take away a guess at how much of this the kernel will consume
    let physmem = PHYSMEM.load(Ordering::Relaxed) as u64;
    let free = UVMEXP.free.load(Ordering::Relaxed).max(0) as u64;
    pages = pages
        .wrapping_sub((atop(physmem as usize) as u64).wrapping_sub(atop(free as usize) as u64));
    let pages = pages as i64;

    // If MD code doesn't say otherwise, use up to 10% of DMA'able memory for buffers.
    if bufcachepercent.load(Ordering::Relaxed) == 0 {
        bufcachepercent.store(10, Ordering::Relaxed);
    }

    // XXX these values and their same use in kern_sysctl need to move into buf.h
    let pct = i64::from(bufcachepercent.load(Ordering::Relaxed));
    kassert!(pct <= 90);
    kassert!(pct >= 5);
    if bufpages.load(Ordering::Relaxed) == 0 {
        bufpages.store(pages * pct / 100, Ordering::Relaxed);
    }
    if bufpages.load(Ordering::Relaxed) < BCACHE_MIN {
        bufpages.store(BCACHE_MIN, Ordering::Relaxed);
    }
    kassert!(bufpages.load(Ordering::Relaxed) < pages);

    BUFHIGHPAGES.store(bufpages.load(Ordering::Relaxed), Ordering::Relaxed);

    // Set the base backoff level for the buffer cache. We will not allow uvm to steal back
    // more than this number of pages.
    let mut buflowpages = pages * 5 / 100;
    if buflowpages < BCACHE_MIN {
        buflowpages = BCACHE_MIN;
    }
    BUFLOWPAGES.store(buflowpages, Ordering::Relaxed);

    // set bufbackpages to 100 pages, or 10 percent of the low water mark if we don't have
    // that many pages.
    let mut bufbackpages = buflowpages * 10 / 100;
    if bufbackpages > 100 {
        bufbackpages = 100;
    }
    BUFBACKPAGES.store(bufbackpages, Ordering::Relaxed);

    // If the MD code does not say otherwise, reserve 10% of kva space for mapping buffers.
    let mut bufkvm = BUFKVM.load(Ordering::Relaxed);
    if bufkvm == 0 {
        bufkvm = VM_KERNEL_SPACE_SIZE / 10;
    }

    // Don't use more than twice the amount of bufpages for mappings. It's twice since we map
    // things sparsely.
    let bufpages_bytes = bufpages.load(Ordering::Relaxed) as usize * PAGE_SIZE;
    if bufkvm > bufpages_bytes {
        bufkvm = bufpages_bytes;
    }
    // Round bufkvm to MAXPHYS because we allocate chunks of va space in MAXPHYS chunks.
    bufkvm &= !(MAXPHYS - 1);
    BUFKVM.store(bufkvm, Ordering::Relaxed);

    pool_init(&BUFPOOL, size_of::<Buf>(), 0, IPL_BIO, 0, "bufpl", None);

    bufcache_init();

    // hmm - bufkvm is an argument because it's static, while bufpages is global because it
    // can change while running.
    buf_mem_init(bufkvm);

    // Set the dirty page high water mark to be less than the low water mark for pages in the
    // buffer cache. This ensures we can always back off by throwing away clean pages, and give
    // ourselves a chance to write out the dirty pages eventually.
    HIDIRTYPAGES.store((buflowpages / 4) * 3, Ordering::Relaxed);
    LODIRTYPAGES.store(buflowpages / 2, Ordering::Relaxed);

    // We are allowed to use up to the reserve.
    TARGETPAGES.store(
        bufpages.load(Ordering::Relaxed) - RESERVE_PAGES,
        Ordering::Relaxed,
    );
}

/// Change cachepct.
pub fn bufadjust(newbufpages: i64) {
    use crate::conf::param::bufpages;

    let newbufpages = newbufpages.max(BUFLOWPAGES.load(Ordering::Relaxed));

    let s = splbio();
    bufpages.store(newbufpages, Ordering::Relaxed);

    // We are allowed to use up to the reserve
    let targetpages = newbufpages - RESERVE_PAGES;
    TARGETPAGES.store(targetpages, Ordering::Relaxed);

    // Shrinking the cache happens here only if someone has manually adjusted bufcachepercent
    // - or the pagedaemon has told us to give back memory *now* - so we give it all back.
    let numbufpages = BCSTATS.numbufpages.load(Ordering::Relaxed);
    if numbufpages > targetpages {
        let _ = bufcache_recover_pages(false, numbufpages - targetpages);
    }
    bufcache_adjust();

    // Wake up the cleaner if we have lots of dirty pages, or if we are getting low on buffer
    // cache kva.
    if unclean_pages() >= HIDIRTYPAGES.load(Ordering::Relaxed)
        || BCSTATS.kvaslots_avail.load(Ordering::Relaxed) <= 2 * RESERVE_SLOTS
    {
        wakeup(ptr::from_ref(&BD_REQ));
    }

    splx(s);
}

/// Back off "size" buffer cache pages. Called by the page daemon to consume buffer cache pages
/// rather than scanning.
///
/// It returns the number of freed pages.
pub fn bufbackoff(_range: &UvmConstraintRange, size: i64) -> u64 {
    use crate::conf::param::bufpages;

    // Back off by at least bufbackpages. If the page daemon gave us a larger size, back off by
    // that much.
    let mut pdelta = size.max(BUFBACKPAGES.load(Ordering::Relaxed));

    let buflowpages = BUFLOWPAGES.load(Ordering::Relaxed);
    let cur = bufpages.load(Ordering::Relaxed);
    if cur <= buflowpages {
        return 0;
    }
    if cur - pdelta < buflowpages {
        pdelta = cur - buflowpages;
    }
    let oldbufpages = cur;
    bufadjust(cur - pdelta);
    (oldbufpages - bufpages.load(Ordering::Relaxed)) as u64
}

/// `bio_doread`: the buffer of `blkno`, with a read started when it holds no valid data.
pub fn bio_doread(vp: &'static Vnode, blkno: Daddr, size: i32, async_: i64) -> &'static Buf {
    let bp = loop {
        if let Some(bp) = getblk(vp, blkno, size, 0, INFSLP) {
            break bp;
        }
    };

    // If buffer does not have valid data, start a read. Note that if buffer is B_INVAL,
    // getblk() won't return it. Therefore, it's valid if its I/O has completed or been
    // delayed.
    if !bp.isset(B_DONE | B_DELWRI) {
        bp.set(B_READ | async_);
        bc_add(&BCSTATS.pendingreads, 1);
        bc_add(&BCSTATS.numreads, 1);
        let _ = VOP_STRATEGY(b_vp(bp), bp);
        // Pay for the read.
        if let Some(p) = curproc() {
            p.p_ru.ru_inblock.set(p.p_ru.ru_inblock.get() + 1); // XXX
        }
    } else if async_ != 0 {
        brelse(bp);
    }

    let mp = if vp.v_type.get() == VBLK {
        vp.v_specmountpoint()
    } else {
        vp.v_mount.get()
    };

    // Collect statistics on synchronous and asynchronous reads. Reads from block devices are
    // charged to their associated filesystem (if any).
    if let Some(mp) = mp {
        if async_ == 0 {
            mp.update_stat(|sp| sp.f_syncreads += 1);
        } else {
            mp.update_stat(|sp| sp.f_asyncreads += 1);
        }
    }

    bp
}

/// Read a disk block. This algorithm described in Bach (p.54).
pub fn bread(vp: &'static Vnode, blkno: Daddr, size: i32) -> (&'static Buf, Result<(), Errno>) {
    // Get buffer for block.
    let bp = bio_doread(vp, blkno, size, 0);

    // Wait for the read to complete, and return result.
    (bp, biowait(bp))
}

/// Read-ahead multiple disk blocks. The first is sync, the rest async. Trivial modification
/// to the breada algorithm presented in Bach (p.55).
pub fn breadn(
    vp: &'static Vnode,
    blkno: Daddr,
    size: i32,
    rablks: &[Daddr],
    rasizes: &[i32],
) -> (&'static Buf, Result<(), Errno>) {
    let bp = bio_doread(vp, blkno, size, 0);

    // For each of the read-ahead blocks, start a read, if necessary.
    for (&rablk, &rasize) in rablks.iter().zip(rasizes) {
        // If it's in the cache, just go on to next one.
        if incore(vp, rablk).is_some() {
            continue;
        }

        // Get a buffer for the read-ahead block
        let _ = bio_doread(vp, rablk, rasize, B_ASYNC);
    }

    // Otherwise, we had to start a read for it; wait until it's valid.
    (bp, biowait(bp))
}

/// The read-ahead buffers of a cluster: the `NULL`-terminated array `bread_cluster` hangs
/// from `b_saveaddr`, `[0]` being the buffer that does the I/O.
///
/// # Safety
///
/// `xbpp` is the array `bread_cluster` allocated with `n + 1` slots, and nothing else uses it.
unsafe fn xbpp_slice<'a>(
    xbpp: *mut Option<&'static Buf>,
    n: usize,
) -> &'a mut [Option<&'static Buf>] {
    // SAFETY: the caller's contract.
    unsafe { core::slice::from_raw_parts_mut(xbpp, n + 1) }
}

/// Called from interrupt context.
pub fn bread_cluster_callback(bp: &'static Buf) {
    let xbpp = bp.b_saveaddr.get().cast::<Option<&'static Buf>>();
    // The array is NULL-terminated: count its buffers.
    let mut n = 0;
    // SAFETY: `bread_cluster` stored its `howmany + 1`-slot array, NULL-terminated, here.
    while unsafe { (*xbpp.add(n)).is_some() } {
        n += 1;
    }
    // SAFETY: as above, `n` buffers and the terminator.
    let xbpp = unsafe { xbpp_slice(xbpp, n) };

    if let Some(x1) = xbpp[1] {
        let newsize = x1.b_bufsize.get();

        // Shrink this buffer's mapping to only cover its part of the total I/O.
        buf_fix_mapping(bp, newsize as usize);
        bp.b_bcount.set(newsize);
    }

    // Invalidate read-ahead buffers if read short
    if bp.b_resid.get() > 0 {
        let mut i = n;
        while i > 1 {
            i -= 1;
            let Some(x) = xbpp[i] else { break };
            let xsize = x.b_bufsize.get() as usize;
            if xsize <= bp.b_resid.get() {
                bp.b_resid.set(bp.b_resid.get() - xsize);
                x.set(B_INVAL);
            } else if bp.b_resid.get() > 0 {
                bp.b_resid.set(0);
                x.set(B_INVAL);
            } else {
                break;
            }
        }
    }

    for x in xbpp[1..n].iter().flatten() {
        if bp.isset(B_ERROR) {
            x.set(B_INVAL | B_ERROR);
        }
        // Move the pages from the master buffer's uvm object into the individual buffer's
        // uvm objects.
        let newobj = &x.b_uobj;
        let oldobj = &bp.b_uobj;

        uvm_obj_init(newobj, Some(&BUFCACHE_PAGER), 1);
        for page in 0..atop(x.b_bufsize.get() as usize) {
            let off = x.b_poffs.get() + ptoa(page) as i64;
            let Some(pg) = uvm_pagelookup(oldobj, off) else {
                panic(format_args!(
                    "bread_cluster_callback: page {off:#x} is gone"
                ));
            };
            kassert!(pg.wire_count.get() == 1);
            uvm_pagerealloc(pg, Some(newobj), off);
        }
        x.b_pobj.set(newobj);

        biodone(x);
    }

    free(
        NonNull::from(&mut xbpp[0]).cast(),
        M_TEMP,
        (n + 1) * size_of::<Option<&'static Buf>>(),
    );

    if bp.isset(B_ASYNC) {
        brelse(bp);
    } else {
        bp.clr(B_WANTED);
        wakeup(ptr::from_ref(bp));
    }
}

/// Read-ahead multiple disk blocks, but make sure only one (big) I/O request is sent to the
/// disk.
///
/// XXX This should probably be dropped and breadn should instead be optimized XXX to do fewer
/// I/O requests.
pub fn bread_cluster(
    vp: &'static Vnode,
    blkno: Daddr,
    size: i32,
) -> (&'static Buf, Result<(), Errno>) {
    let rbp = bio_doread(vp, blkno, size, 0);

    'out: {
        // If the buffer is in the cache skip any I/O operation.
        if rbp.isset(B_CACHE) {
            break 'out;
        }

        if size as usize != round_page(size as usize) {
            break 'out;
        }

        let mut sblkno: Daddr = 0;
        let mut maxra: i32 = 0;
        if VOP_BMAP(vp, blkno + 1, None, Some(&mut sblkno), Some(&mut maxra)).is_err() {
            break 'out;
        }

        maxra += 1;
        if sblkno == -1 || maxra < 2 {
            break 'out;
        }

        let howmany = ((MAXPHYS / size as usize) as i32).min(maxra) as usize;

        let Some(mem) = mallocarray(
            howmany + 1,
            size_of::<Option<&'static Buf>>(),
            M_TEMP,
            M_NOWAIT,
        ) else {
            break 'out;
        };
        let raw = mem.as_ptr().cast::<Option<&'static Buf>>();
        // SAFETY: a fresh allocation of `howmany + 1` slots; `None` is a valid bit pattern
        // for every slot once written.
        let xbpp = unsafe {
            for i in 0..=howmany {
                raw.add(i).write(None);
            }
            xbpp_slice(raw, howmany)
        };

        let mut i = howmany;
        while i > 0 {
            i -= 1;
            // First buffer allocates big enough size to cover what all the other buffers
            // need.
            let sz = if i == 0 { howmany * size as usize } else { 0 };

            xbpp[i] = buf_get(Some(vp), blkno + i as Daddr + 1, sz);
            if xbpp[i].is_none() {
                for x in xbpp[i + 1..howmany].iter().flatten() {
                    x.set(B_INVAL);
                    brelse(x);
                }
                free(
                    mem,
                    M_TEMP,
                    (howmany + 1) * size_of::<Option<&'static Buf>>(),
                );
                break 'out;
            }
        }

        let Some(bp) = xbpp[0] else {
            break 'out;
        };

        xbpp[howmany] = None;

        let inc = btodb(size as usize) as Daddr;

        for (i, x) in xbpp[1..howmany].iter().enumerate() {
            let Some(x) = x else { continue };
            let i = i + 1;
            bc_add(&BCSTATS.pendingreads, 1);
            bc_add(&BCSTATS.numreads, 1);
            x.set(B_READ | B_ASYNC);
            x.b_blkno.set(sblkno + i as Daddr * inc);
            x.b_bufsize.set(i64::from(size));
            x.b_bcount.set(i64::from(size));
            x.b_data.set(ptr::null_mut());
            x.b_pobj.set(bp.b_pobj.get());
            x.b_poffs
                .set(bp.b_poffs.get() + (i as i64 * i64::from(size)));
        }

        kassert!(bp.b_lblkno.get() == blkno + 1);
        kassert!(bp.b_vp.get().is_some_and(|v| ptr::eq(v, vp)));

        bp.b_blkno.set(sblkno);
        bp.set(B_READ | B_ASYNC | B_CALL);

        bp.b_saveaddr.set(raw.cast());
        bp.b_iodone.set(Some(bread_cluster_callback));

        bc_add(&BCSTATS.pendingreads, 1);
        bc_add(&BCSTATS.numreads, 1);
        let _ = VOP_STRATEGY(b_vp(bp), bp);
        if let Some(p) = curproc() {
            p.p_ru.ru_inblock.set(p.p_ru.ru_inblock.get() + 1);
        }
    }

    // out:
    (rbp, biowait(rbp))
}

/// The mount a buffer's writes are charged to: the file system mounted on a block device,
/// or the vnode's own.
fn bp_mount(vp: Option<&'static Vnode>) -> Option<&'static Mount> {
    let vp = vp?;
    if vp.v_type.get() == VBLK {
        vp.v_specmountpoint()
    } else {
        vp.v_mount.get()
    }
}

/// Block write. Described in Bach (p.56)
pub fn bwrite(bp: &'static Buf) -> Result<(), Errno> {
    let vp = bp.b_vp.get();
    let mp = bp_mount(vp);

    // Remember buffer type, to switch on it later. If the write was synchronous, but the file
    // system was mounted with MNT_ASYNC, convert it to a delayed write.
    // XXX note that this relies on delayed tape writes being converted to async, not sync
    // writes (which is safe, but ugly).
    let async_ = bp.isset(B_ASYNC);
    if !async_ && mp.is_some_and(|mp| mp.mnt_flag.get() & MNT_ASYNC != 0) {
        // Don't convert writes from VND on async filesystems that already have delayed writes
        // in the upper layer.
        if !bp.isset(B_NOCACHE) {
            bdwrite(bp);
            return Ok(());
        }
    }

    // Collect statistics on synchronous and asynchronous writes. Writes to block devices are
    // charged to their associated filesystem (if any).
    if let Some(mp) = mp {
        if async_ {
            mp.update_stat(|sp| sp.f_asyncwrites += 1);
        } else {
            mp.update_stat(|sp| sp.f_syncwrites += 1);
        }
    }
    bc_add(&BCSTATS.pendingwrites, 1);
    bc_add(&BCSTATS.numwrites, 1);

    let wasdelayed = bp.isset(B_DELWRI);
    bp.clr(B_READ | B_DONE | B_ERROR | B_DELWRI);

    let s = splbio();

    // If not synchronous, pay for the I/O operation and make sure the buf is on the correct
    // vnode queue. We have to do this now, because if we don't, the vnode may not be properly
    // notified that its I/O has completed.
    if wasdelayed {
        reassignbuf(bp);
    } else if let Some(p) = curproc() {
        p.p_ru.ru_oublock.set(p.p_ru.ru_oublock.get() + 1);
    }

    // Initiate disk write. Make sure the appropriate party is charged.
    let vp = b_vp(bp);
    vp.v_numoutput.set(vp.v_numoutput.get() + 1);
    bp.set(B_WRITEINPROG);
    splx(s);
    let _ = VOP_STRATEGY(vp, bp);

    // If the queue is above the high water mark, wait till the number of outstanding write
    // bufs drops below the low water mark.
    if let Some(bq) = bp.b_bq.get() {
        bufq_wait(bq);
    }

    if async_ {
        return Ok(());
    }

    // If I/O was synchronous, wait for it to complete.
    let rv = biowait(bp);

    // Release the buffer.
    brelse(bp);

    rv
}

/// Delayed write.
///
/// The buffer is marked dirty, but is not queued for I/O. This routine should be used when
/// the buffer is expected to be modified again soon, typically a small write that partially
/// fills a buffer.
///
/// NB: magnetic tapes cannot be delayed; they must be written in the order that the writes are
/// requested.
///
/// Described in Leffler, et al. (pp. 208-213).
pub fn bdwrite(bp: &'static Buf) {
    // If the block hasn't been seen before:
    //	(1) Mark it as having been seen,
    //	(2) Charge for the write.
    //	(3) Make sure it's on its vnode's correct block list,
    //	(4) If a buffer is rewritten, move it to end of dirty list
    if !bp.isset(B_DELWRI) {
        bp.set(B_DELWRI);
        let s = splbio();
        reassignbuf(bp);
        splx(s);
        if let Some(p) = curproc() {
            p.p_ru.ru_oublock.set(p.p_ru.ru_oublock.get() + 1); // XXX
        }
    }

    // The "write" is done, so mark and release the buffer.
    bp.clr(B_NEEDCOMMIT);
    bp.clr(B_NOCACHE); // Must cache delayed writes
    bp.set(B_DONE);
    brelse(bp);
}

/// Asynchronous block write; just an asynchronous bwrite().
pub fn bawrite(bp: &'static Buf) {
    bp.set(B_ASYNC);
    let _ = VOP_BWRITE(bp);
}

/// Must be called at splbio()
pub fn buf_dirty(bp: &'static Buf) {
    splassert(IPL_BIO, "buf_dirty");

    #[cfg(feature = "diagnostic")]
    if !bp.isset(B_BUSY) {
        panic(format_args!("Trying to dirty buffer on freelist!"));
    }

    if !bp.isset(B_DELWRI) {
        bp.set(B_DELWRI);
        reassignbuf(bp);
    }
}

/// Must be called at splbio()
pub fn buf_undirty(bp: &'static Buf) {
    splassert(IPL_BIO, "buf_undirty");

    #[cfg(feature = "diagnostic")]
    if !bp.isset(B_BUSY) {
        panic(format_args!("Trying to undirty buffer on freelist!"));
    }
    if bp.isset(B_DELWRI) {
        bp.clr(B_DELWRI);
        reassignbuf(bp);
    }
}

/// Release a buffer on to the free lists. Described in Bach (p. 46).
pub fn brelse(bp: &'static Buf) {
    let s = splbio();

    if !bp.b_data.get().is_null() {
        kassert!(bp.b_bufsize.get() > 0);
    }

    // Determine which queue the buffer should be on, then put it there.

    // If it's not cacheable, or an error, mark it invalid.
    if bp.isset(B_NOCACHE | B_ERROR) {
        bp.set(B_INVAL);
    }
    // If it's a write error, also mark the vnode as damaged.
    if bp.isset(B_ERROR)
        && !bp.isset(B_READ)
        && let Some(vp) = bp.b_vp.get()
        && vp.v_type.get() == VREG
    {
        vp.v_bioflag.set(vp.v_bioflag.get() | VBIOERROR);
    }

    if bp.isset(B_INVAL) {
        // If the buffer is invalid, free it now rather than leaving it in a queue and wasting
        // memory.
        if bp.isset(B_DELWRI) {
            bp.clr(B_DELWRI);
        }

        if let Some(vp) = bp.b_vp.get() {
            // SAFETY: a buffer with a vnode is in that vnode's tree (`buf_get`).
            unsafe { vp.v_bufs_tree.remove(bp) };
            brelvp(bp);
        }
        bp.b_vp.set(None);

        // Wake up any processes waiting for _this_ buffer to become free. They are not allowed
        // to grab it since it will be freed. But the only sleeper is getblk and it will
        // restart the operation after sleep.
        if bp.isset(B_WANTED) {
            bp.clr(B_WANTED);
            wakeup(ptr::from_ref(bp));
        }
        buf_put(bp);
    } else {
        // It has valid data. Put it on the end of the appropriate queue, so that it'll stick
        // around for as long as possible.
        bufcache_release(bp);

        // Unlock the buffer.
        bp.clr(B_AGE | B_ASYNC | B_NOCACHE | B_DEFERRED);
        buf_release(bp);

        // Wake up any processes waiting for _this_ buffer to become free.
        if bp.isset(B_WANTED) {
            bp.clr(B_WANTED);
            wakeup(ptr::from_ref(bp));
        }

        let numbufpages = BCSTATS.numbufpages.load(Ordering::Relaxed);
        let targetpages = TARGETPAGES.load(Ordering::Relaxed);
        if numbufpages > targetpages {
            let _ = bufcache_recover_pages(false, numbufpages - targetpages);
        }
        bufcache_adjust();
    }

    // Wake up syncer and cleaner processes waiting for buffers.
    if NOBUFFERS.load(Ordering::Relaxed) != 0 {
        NOBUFFERS.store(0, Ordering::Relaxed);
        wakeup(ptr::from_ref(&NOBUFFERS));
    }

    // Wake up any processes waiting for any buffer to become free.
    if NEEDBUFFER.load(Ordering::Relaxed) != 0
        && BCSTATS.numbufpages.load(Ordering::Relaxed) < TARGETPAGES.load(Ordering::Relaxed)
        && BCSTATS.kvaslots_avail.load(Ordering::Relaxed) > RESERVE_SLOTS
    {
        NEEDBUFFER.store(0, Ordering::Relaxed);
        wakeup(ptr::from_ref(&NEEDBUFFER));
    }

    splx(s);
}

/// The buffer of `blkno` in `vp`'s tree, valid or not (the C's `RBT_FIND` with a key `struct
/// buf` on the stack).
fn rbt_find(vp: &'static Vnode, blkno: Daddr) -> Option<&'static Buf> {
    let b = Buf::new();
    b.b_lblkno.set(blkno);
    vp.v_bufs_tree.find(&b)
}

/// Determine if a block is in the cache. Just look on what would be its hash chain. If it's
/// there, return a pointer to it, unless it's marked invalid.
fn incore_locked(vp: &'static Vnode, blkno: Daddr) -> Option<&'static Buf> {
    splassert(IPL_BIO, "incore_locked");

    // Search buf lookup tree
    rbt_find(vp, blkno).filter(|bp| !bp.isset(B_INVAL))
}

/// `incore(vp, blkno)`: the valid buffer of `blkno`, if the cache has one.
pub fn incore(vp: &'static Vnode, blkno: Daddr) -> Option<&'static Buf> {
    let s = splbio();
    let bp = incore_locked(vp, blkno);
    splx(s);

    bp
}

/// Get a block of requested size that is associated with a given vnode and block offset. If it
/// is found in the block cache, mark it as having been found, make it busy and return it.
/// Otherwise, return an empty block of the correct size. It is up to the caller to ensure that
/// the cached blocks be of the correct size.
pub fn getblk(
    vp: &'static Vnode,
    blkno: Daddr,
    size: i32,
    slpflag: i32,
    slptimeo: u64,
) -> Option<&'static Buf> {
    // XXX
    // The following is an inlined version of 'incore()', but with the 'invalid' test moved to
    // after the 'busy' test. It's necessary because there are some cases in which the NFS code
    // sets B_INVAL prior to writing data to the server, but in which the buffers actually
    // contain valid data. In this case, we can't allow the system to allocate a new buffer for
    // the block until the write is finished.
    loop {
        // start:
        let s = splbio();
        if let Some(bp) = rbt_find(vp, blkno) {
            if bp.isset(B_BUSY) {
                bp.set(B_WANTED);
                let error = tsleep_nsec(
                    ptr::from_ref(bp),
                    slpflag | (PRIBIO + 1),
                    "getblk",
                    slptimeo,
                );
                splx(s);
                if error.is_err() {
                    return None;
                }
                continue;
            }

            if !bp.isset(B_INVAL) {
                bc_add(&BCSTATS.cachehits, 1);
                bp.set(B_CACHE);
                bufcache_take(bp);
                buf_acquire(bp);
                splx(s);
                return Some(bp);
            }
        }
        splx(s);

        if let Some(bp) = buf_get(Some(vp), blkno, size as usize) {
            return Some(bp);
        }
    }
}

/// Get an empty, disassociated buffer of given size.
pub fn geteblk(size: usize) -> &'static Buf {
    loop {
        if let Some(bp) = buf_get(None, 0, size) {
            return bp;
        }
    }
}

/// Allocate a buffer. If vp is given, put it into the buffer cache for that vnode. If size !=
/// 0, allocate memory and call buf_map(). If there is already a buffer for the given
/// vnode/blkno, return NULL.
pub fn buf_get(vp: Option<&'static Vnode>, blkno: Daddr, size: usize) -> Option<&'static Buf> {
    use crate::conf::param::bufpages;

    let poolwait = if size == 0 { PR_NOWAIT } else { PR_WAITOK };

    let s = splbio();
    if size != 0 {
        // Wake up the cleaner if we have lots of dirty pages, or if we are getting low on
        // buffer cache kva.
        if unclean_pages() >= HIDIRTYPAGES.load(Ordering::Relaxed)
            || BCSTATS.kvaslots_avail.load(Ordering::Relaxed) <= 2 * RESERVE_SLOTS
        {
            wakeup(ptr::from_ref(&BD_REQ));
        }

        let npages = atop(round_page(size)) as i64;

        // if our cache has been previously shrunk, allow it to grow again with use up to
        // bufhighpages (cachepercent)
        if bufpages.load(Ordering::Relaxed) < BUFHIGHPAGES.load(Ordering::Relaxed) {
            bufadjust(BUFHIGHPAGES.load(Ordering::Relaxed));
        }

        // If we would go over the page target with our new allocation, free enough buffers
        // first to stay at the target with our new allocation.
        if BCSTATS.numbufpages.load(Ordering::Relaxed) + npages
            > TARGETPAGES.load(Ordering::Relaxed)
        {
            let _ = bufcache_recover_pages(false, npages);
            bufcache_adjust();
        }

        // If we get here, we tried to free the world down above, and couldn't get down - Wake
        // the cleaner and wait for it to push some buffers out.
        let cur = curproc_ptr();
        if (BCSTATS.numbufpages.load(Ordering::Relaxed) + npages
            > TARGETPAGES.load(Ordering::Relaxed)
            || BCSTATS.kvaslots_avail.load(Ordering::Relaxed) <= RESERVE_SLOTS)
            && cur != syncerproc()
            && cur != cleanerproc()
        {
            wakeup(ptr::from_ref(&BD_REQ));
            NEEDBUFFER.fetch_add(1, Ordering::Relaxed);
            let _ = tsleep_nsec(ptr::from_ref(&NEEDBUFFER), PRIBIO, "needbuffer", INFSLP);
            splx(s);
            return None;
        }
        if BCSTATS.numbufpages.load(Ordering::Relaxed) + npages > bufpages.load(Ordering::Relaxed) {
            // cleaner or syncer
            NOBUFFERS.store(1, Ordering::Relaxed);
            let _ = tsleep_nsec(ptr::from_ref(&NOBUFFERS), PRIBIO, "nobuffers", INFSLP);
            splx(s);
            return None;
        }
    }

    let Some(mem) = pool_get(&BUFPOOL, poolwait | PR_ZERO) else {
        splx(s);
        return None;
    };
    let bp = mem.cast::<Buf>();
    // SAFETY: a fresh, suitably aligned pool item of `size_of::<Buf>()` bytes, written once
    // before anything else sees it.
    unsafe { bp.as_ptr().write(Buf::new()) };
    // SAFETY: as above; the item stays allocated until `buf_put` (or `buf_unmap`) gives it
    // back.
    let bp: &'static Buf = unsafe { bp.as_ref() };

    // bp->b_freelist.tqe_next = NOLIST: `b_onfreelist` is false.
    bp.b_dev.set(NODEV);
    bp.b_bcount.set(size as i64);

    buf_acquire_nomap(bp);

    if let Some(vp) = vp {
        // We insert the buffer into the hash with B_BUSY set while we allocate pages for it.
        // This way any getblk that happens while we allocate pages will wait for this buffer
        // instead of starting its own buf_get.
        //
        // But first, we check if someone beat us to it.
        if incore_locked(vp, blkno).is_some() {
            pool_put(&BUFPOOL, mem);
            splx(s);
            return None;
        }

        bp.b_blkno.set(blkno);
        bp.b_lblkno.set(blkno);
        bgetvp(vp, bp);
        // SAFETY: the new buffer is in no tree; it stays in its pool item while linked.
        if unsafe { vp.v_bufs_tree.insert(bp) }.is_some() {
            panic(format_args!("buf_get: dup lblk vp {:p} bp {:p}", vp, bp));
        }
    } else {
        // bp->b_vnbufs.le_next = NOLIST: `b_onvnbufs` is false.
        bp.set(B_INVAL);
        bp.b_vp.set(None);
    }

    // SAFETY: the new buffer is on no list; it stays in its pool item until `buf_put`
    // unlinks it.
    unsafe { BUFHEAD.0.insert_head(bp) };
    bc_add(&BCSTATS.numbufs, 1);

    if size != 0 {
        buf_alloc_pages(bp, round_page(size));
        buf_map(bp);
    }

    bp.set(B_BC);
    splx(s);

    Some(bp)
}

/// Buffer cleaning daemon.
pub fn buf_daemon(_arg: *mut core::ffi::c_void) {
    let mut bp: Option<&'static Buf> = None;
    let mut pushed = 0;

    let mut s = splbio();
    loop {
        if bp.is_none()
            || (pushed >= 16
                && unclean_pages() < HIDIRTYPAGES.load(Ordering::Relaxed)
                && BCSTATS.kvaslots_avail.load(Ordering::Relaxed) > 2 * RESERVE_SLOTS)
        {
            pushed = 0;
            // Wake up anyone who was waiting for buffers to be released.
            if NEEDBUFFER.load(Ordering::Relaxed) != 0 {
                NEEDBUFFER.store(0, Ordering::Relaxed);
                wakeup(ptr::from_ref(&NEEDBUFFER));
            }
            let _ = tsleep_nsec(ptr::from_ref(&BD_REQ), PRIBIO - 7, "cleaner", INFSLP);
        }

        loop {
            bp = bufcache_getdirtybuf();
            let Some(b) = bp else { break };
            // TRACEPOINT(vfs, cleaner, ...): dt(4), not configured.

            if unclean_pages() < LODIRTYPAGES.load(Ordering::Relaxed)
                && BCSTATS.kvaslots_avail.load(Ordering::Relaxed) > 2 * RESERVE_SLOTS
                && pushed >= 16
            {
                break;
            }

            bufcache_take(b);
            buf_acquire(b);
            splx(s);

            if b.isset(B_INVAL) {
                brelse(b);
                s = splbio();
                continue;
            }
            #[cfg(feature = "diagnostic")]
            if !b.isset(B_DELWRI) {
                panic(format_args!("Clean buffer on dirty queue"));
            }
            bawrite(b);
            pushed += 1;

            sched_pause(r#yield);

            s = splbio();
        }
    }
}

/// Wait for operations on the buffer to complete. When they do, extract and return the I/O's
/// error value.
pub fn biowait(bp: &'static Buf) -> Result<(), Errno> {
    kassert!(!bp.isset(B_ASYNC));

    let s = splbio();
    while !bp.isset(B_DONE) {
        let _ = tsleep_nsec(ptr::from_ref(bp), PRIBIO + 1, "biowait", INFSLP);
    }
    splx(s);

    // check for interruption of I/O (e.g. via NFS), then errors.
    if bp.isset(B_EINTR) {
        bp.clr(B_EINTR);
        return Err(Errno::EINTR);
    }

    if bp.isset(B_ERROR) {
        Err(bp.b_error.get().unwrap_or(Errno::EIO))
    } else {
        Ok(())
    }
}

/// Mark I/O complete on a buffer.
///
/// If a callback has been requested, e.g. the pageout daemon, do so. Otherwise, awaken waiting
/// processes.
///
/// \[ Leffler, et al., says on p.247: "This routine wakes up the blocked process, frees the
/// buffer for an asynchronous write, or, for a request by the pagedaemon process, invokes a
/// procedure specified in the buffer structure" \]
///
/// In real life, the pagedaemon (or other system processes) wants to do async stuff to, and
/// doesn't want the buffer brelse()'d. (for swap pager, that puts swap buffers on the free
/// lists (!!!), for the vn device, that puts malloc'd buffers on the free lists!)
///
/// Must be called at splbio().
pub fn biodone(bp: &'static Buf) {
    splassert(IPL_BIO, "biodone");

    if bp.isset(B_DONE) {
        panic(format_args!("biodone already"));
    }
    bp.set(B_DONE); // note that it's done

    if let Some(bq) = bp.b_bq.get() {
        bufq_done(bq, bp);
    }

    if !bp.isset(B_READ) {
        bp.clr(B_WRITEINPROG);
        vwakeup(bp.b_vp.get());
    }
    if BCSTATS.numbufs.load(Ordering::Relaxed) != 0 && !(bp.isset(B_RAW) || bp.isset(B_PHYS)) {
        if !bp.isset(B_READ) {
            bc_add(&BCSTATS.pendingwrites, -1);
        } else {
            bc_add(&BCSTATS.pendingreads, -1);
        }
    }
    if bp.isset(B_CALL) {
        // if necessary, call out
        bp.clr(B_CALL); // but note callout done
        match bp.b_iodone.get() {
            Some(iodone) => iodone(bp),
            None => panic(format_args!("biodone: B_CALL without b_iodone")),
        }
    } else if bp.isset(B_ASYNC) {
        // if async, release it
        brelse(bp);
    } else {
        // or just wakeup the buffer
        bp.clr(B_WANTED);
        wakeup(ptr::from_ref(bp));
    }
}

// DDB: bcstats_print waits for the ddb command loop (db_command.c).

/// `buf_adjcnt(bp, ncount)`: shortens the valid part of a buffer.
pub fn buf_adjcnt(bp: &Buf, ncount: i64) {
    kassert!(ncount <= bp.b_bufsize.get());
    bp.b_bcount.set(ncount);
}

// bufcache freelist code below
//
// The code below implements a variant of the 2Q buffer cache algorithm by Johnson and Shasha.
//
// General Outline
// We divide the buffer cache into three working sets: current, previous, and long term. Each
// list is itself LRU and buffers get promoted and moved around between them. A buffer starts
// its life in the current working set. As time passes and newer buffers push it out, it will
// turn into the previous working set and is subject to recycling. But if it's accessed again
// from the previous working set, that's an indication that it's actually in the long term
// working set, so we promote it there. The separation of current and previous working sets
// prevents us from promoting a buffer that's only temporarily hot to the long term cache.
//
// The objective is to provide scan resistance by making the long term working set ineligible
// for immediate recycling, even as the current working set is rapidly turned over.
//
// Implementation
// The code below identifies the current, previous, and long term sets as hotqueue, coldqueue,
// and warmqueue. The hot and warm queues are capped at 1/3 of the total clean pages, after
// which point they start pushing their oldest buffers into coldqueue.
// A buf always starts out with neither WARM or COLD flags set (implying HOT). When released,
// it will be returned to the tail of the hotqueue list. When the hotqueue gets too large, the
// oldest hot buf will be moved to the coldqueue, with the B_COLD flag set. When a cold buf is
// released, we set the B_WARM flag and put it onto the warmqueue. Warm bufs are also directly
// returned to the end of the warmqueue. As with the hotqueue, when the warmqueue grows too
// large, B_WARM bufs are moved onto the coldqueue.
//
// Note that this design does still support large working sets, greater than the cap of
// hotqueue or warmqueue would imply. The coldqueue is still cached and has no maximum length.
// The hot and warm queues form a Y feeding into the coldqueue. Moving bufs between queues is
// constant time, so this design decays to one long warm->cold queue.
//
// In the 2Q paper, hotqueue and coldqueue are A1in and A1out. The warmqueue is Am. We always
// cache pages, as opposed to pointers to pages for A1.
//
// This implementation adds support for multiple 2q caches.
//
// If we have more than one 2q cache, as bufs fall off the cold queue for recycling, bufs that
// have been warm before (which retain the B_WARM flag in addition to B_COLD) can be put into
// the hot queue of a second level 2Q cache. buffers which are only B_COLD are recycled. Bufs
// falling off the last cache's cold queue are always recycled.

/// Which queue of a cache `chillbufs` works on (the C compares the queue's address with the
/// cache's members).
#[derive(Clone, Copy, PartialEq, Eq)]
enum ChillQueue {
    /// `cache->hotqueue`, counted by `hotbufpages`.
    Hot,
    /// `cache->warmqueue`, counted by `warmbufpages`.
    Warm,
}

/// `bufcache_init`: the empty queues.
pub fn bufcache_init() {
    CLEANCACHE.hotqueue.init();
    CLEANCACHE.coldqueue.init();
    CLEANCACHE.warmqueue.init();
    DIRTYQUEUE.0.init();
}

/// if the buffer caches have shrunk, we may need to rebalance our queues.
pub fn bufcache_adjust() {
    while chillbufs(&CLEANCACHE, ChillQueue::Warm) || chillbufs(&CLEANCACHE, ChillQueue::Hot) {}
}

/// The first buffer of the cold, else warm, else hot queue: the next victim.
fn bufcache_first(cache: &'static Bufcache) -> Option<&'static Buf> {
    cache
        .coldqueue
        .first()
        .or_else(|| cache.warmqueue.first())
        .or_else(|| cache.hotqueue.first())
}

/// The pages of a buffer's memory.
fn bufpages_of(bp: &Buf) -> i64 {
    atop(bp.b_bufsize.get() as usize) as i64
}

/// Takes `bp` off the free-list queue it is on.
///
/// # Safety
///
/// `bp` is on `queue`.
unsafe fn freelist_remove(queue: &Bufqueue, bp: &'static Buf) {
    // SAFETY: the caller's contract.
    unsafe { queue.remove(bp) };
    bp.b_onfreelist.set(false);
}

/// Puts `bp` at the tail of `queue`.
///
/// # Safety
///
/// `bp` is on no free-list queue.
unsafe fn freelist_insert_tail(queue: &Bufqueue, bp: &'static Buf) {
    // SAFETY: the caller's contract; the buffer stays in its pool item while linked.
    unsafe { queue.insert_tail(bp) };
    bp.b_onfreelist.set(true);
}

/// Get a clean buffer from the cache. if "discard" is set do not promote previously warm
/// buffers as normal, because we are tossing everything away such as in a hibernation
pub fn bufcache_getcleanbuf(discard: bool) -> Option<&'static Buf> {
    let cache = &CLEANCACHE;

    splassert(IPL_BIO, "bufcache_getcleanbuf");

    // try cold queue
    while let Some(b) = bufcache_first(cache) {
        let pages = bufpages_of(b);

        if discard {
            // Victim selected, give it up
            return Some(b);
        }
        // If this buffer was warm before, move it to the hot queue in the next cache

        // Move the buffer to the hot queue in the next cache
        let queue = if b.isset(B_COLD) {
            &cache.coldqueue
        } else if b.isset(B_WARM) {
            cache.warmbufpages.set(cache.warmbufpages.get() - pages);
            &cache.warmqueue
        } else {
            cache.hotbufpages.set(cache.hotbufpages.get() - pages);
            &cache.hotqueue
        };
        // SAFETY: the flags say which queue the buffer is on.
        unsafe { freelist_remove(queue, b) };
        cache.cachepages.set(cache.cachepages.get() - pages);
        b.clr(B_WARM);
        b.clr(B_COLD);
    }
    // The loop only ends when every queue is empty: the C's bp is NULL here.
    None
}

/// `discard_buffer(bp)`: takes a clean buffer out of the cache and frees it.
pub fn discard_buffer(bp: &'static Buf) {
    splassert(IPL_BIO, "discard_buffer");

    bufcache_take(bp);
    if let Some(vp) = bp.b_vp.get() {
        // SAFETY: a buffer with a vnode is in that vnode's tree (`buf_get`).
        unsafe { vp.v_bufs_tree.remove(bp) };
        brelvp(bp);
    }
    buf_put(bp);
}

/// `bufcache_recover_pages(discard, howmany)`: frees clean buffers, oldest first, until
/// `howmany` pages came back; returns the pages freed.
pub fn bufcache_recover_pages(_discard: bool, howmany: i64) -> i64 {
    let cache = &CLEANCACHE;
    let mut recovered = 0;

    splassert(IPL_BIO, "bufcache_recover_pages");

    while recovered < howmany {
        let Some(bp) = bufcache_first(cache) else {
            break;
        };
        let pages = bufpages_of(bp);

        recovered += pages;
        discard_buffer(bp);
    }
    recovered
}

/// `bufcache_getdirtybuf()`: the oldest delayed-write buffer.
pub fn bufcache_getdirtybuf() -> Option<&'static Buf> {
    DIRTYQUEUE.0.first()
}

/// `bufcache_take(bp)`: takes a free buffer off its cache queue (clean or dirty), to be
/// acquired.
pub fn bufcache_take(bp: &'static Buf) {
    splassert(IPL_BIO, "bufcache_take");
    kassert!(bp.isset(B_BC));

    let pages = bufpages_of(bp);

    // TRACEPOINT(vfs, bufcache_take, ...): dt(4), not configured.

    let cache = &CLEANCACHE;
    let queue = if !bp.isset(B_DELWRI) {
        let queue = if bp.isset(B_COLD) {
            &cache.coldqueue
        } else if bp.isset(B_WARM) {
            cache.warmbufpages.set(cache.warmbufpages.get() - pages);
            &cache.warmqueue
        } else {
            cache.hotbufpages.set(cache.hotbufpages.get() - pages);
            &cache.hotqueue
        };
        bc_add(&BCSTATS.numcleanpages, -pages);
        cache.cachepages.set(cache.cachepages.get() - pages);
        queue
    } else {
        bc_add(&BCSTATS.numdirtypages, -pages);
        bc_add(&BCSTATS.delwribufs, -1);
        &DIRTYQUEUE.0
    };
    // SAFETY: a free cache buffer is on the queue its flags name (`bufcache_release`).
    unsafe { freelist_remove(queue, bp) };
}

/// move buffers from a hot or warm queue to a cold queue in a cache
fn chillbufs(cache: &'static Bufcache, which: ChillQueue) -> bool {
    // We limit the hot queue to be small, with a max of 4096 pages. We limit the warm queue
    // to half the cache size.
    //
    // We impose a minimum size of 96 to prevent too much "wobbling".
    let (queue, queuepages, limit) = match which {
        ChillQueue::Hot => (
            &cache.hotqueue,
            &cache.hotbufpages,
            (cache.cachepages.get() / 20).min(4096),
        ),
        ChillQueue::Warm => (
            &cache.warmqueue,
            &cache.warmbufpages,
            cache.cachepages.get() / 2,
        ),
    };

    if queuepages.get() > 96 && queuepages.get() > limit {
        let Some(bp) = queue.first() else {
            panic(format_args!("inconsistent bufpage counts"));
        };
        let pages = bufpages_of(bp);
        queuepages.set(queuepages.get() - pages);
        // SAFETY: `bp` is the head of `queue` and moves to the cold queue.
        unsafe {
            freelist_remove(queue, bp);
            // we do not clear B_WARM
            bp.set(B_COLD);
            freelist_insert_tail(&cache.coldqueue, bp);
        }
        return true;
    }
    false
}

/// `bufcache_release(bp)`: puts a buffer that is being released on the right cache queue:
/// the dirty queue, the warm queue for a buffer that was warm or cold, the hot queue for the
/// rest.
pub fn bufcache_release(bp: &'static Buf) {
    let cache = &CLEANCACHE;

    kassert!(bp.isset(B_BC));
    let pages = bufpages_of(bp);

    // TRACEPOINT(vfs, bufcache_rel, ...): dt(4), not configured.

    let queue = if !bp.isset(B_DELWRI) {
        let (queue, queuepages, which) = if bp.isset(B_WARM | B_COLD) {
            bp.set(B_WARM);
            bp.clr(B_COLD);
            (&cache.warmqueue, &cache.warmbufpages, ChillQueue::Warm)
        } else {
            (&cache.hotqueue, &cache.hotbufpages, ChillQueue::Hot)
        };
        queuepages.set(queuepages.get() + pages);
        bc_add(&BCSTATS.numcleanpages, pages);
        cache.cachepages.set(cache.cachepages.get() + pages);
        let _ = chillbufs(cache, which);
        queue
    } else {
        bc_add(&BCSTATS.numdirtypages, pages);
        bc_add(&BCSTATS.delwribufs, 1);
        &DIRTYQUEUE.0
    };
    // SAFETY: a buffer being released is on no free-list queue (`bufcache_take` or `buf_get`).
    unsafe { freelist_insert_tail(queue, bp) };
}

// HIBERNATE (hibernate_suspend_bufcache, hibernate_resume_bufcache): not configured.

/// `dma_constraint`, the range `buf_alloc_pages` backs off for.
pub fn dma_constraint() -> &'static UvmConstraintRange {
    <Machine as Pmap>::DMA_CONSTRAINT
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for the buffer cache: `getblk`/`brelse`/`incore` bookkeeping, `bread` through a
    // fake strategy (hits, misses, errors), delayed writes and `vflushbuf`, the 2Q queues and
    // `vinvalbuf`, over `blkfs`, a vnode whose strategy reads and writes an in-memory disk.

    use std::sync::MutexGuard;
    use std::vec::Vec;
    use std::{assert, assert_eq, vec};

    use super::*;
    use crate::kern::subr_xxx::nullop;
    use crate::kern::vfs_default::vop_generic_bwrite;
    use crate::kern::vfs_subr::{getnewvnode, vflushbuf, vinvalbuf};
    use crate::machine::cpu::Cpu;
    use crate::sys::mount::MNT_WAIT;
    use crate::sys::param::DEV_BSIZE;
    use crate::sys::ucred::NOCRED;
    use crate::sys::vnode::{
        VBIOONSYNCLIST, VT_NON, VopFsyncArgs, VopInactiveArgs, VopStrategyArgs, Vops,
    };

    /// The fake disk: `DISK_BLOCKS` sectors of `DEV_BSIZE` bytes.
    const DISK_BLOCKS: usize = 256;

    /// The disk's bytes, the strategy's read and write counts, and a block that fails.
    struct Disk {
        data: Vec<u8>,
        reads: usize,
        writes: usize,
        bad: Daddr,
    }

    static DISK: std::sync::Mutex<Option<Disk>> = std::sync::Mutex::new(None);

    fn disk<R>(f: impl FnOnce(&mut Disk) -> R) -> R {
        let mut d = DISK.lock().unwrap_or_else(|e| e.into_inner());
        f(d.as_mut().expect("the disk"))
    }

    /// `blkfs`'s strategy: a synchronous transfer between the buffer and the disk at
    /// `b_blkno`, then `biodone`.
    fn blkfs_strategy(ap: &mut VopStrategyArgs) -> Result<(), Errno> {
        let bp = ap.a_bp;
        let off = bp.b_blkno.get() as usize * DEV_BSIZE;
        let len = bp.b_bcount.get() as usize;
        disk(|d| {
            if bp.b_blkno.get() == d.bad || off + len > d.data.len() {
                bp.b_error.set(Some(Errno::EIO));
                bp.set(B_ERROR);
                return;
            }
            // SAFETY: the buffer is busy for this transfer and mapped.
            let data = unsafe { bp.data() };
            if bp.isset(B_READ) {
                data.copy_from_slice(&d.data[off..off + len]);
                d.reads += 1;
            } else {
                d.data[off..off + len].copy_from_slice(data);
                d.writes += 1;
            }
            bp.b_resid.set(0);
        });
        let s = splbio();
        biodone(bp);
        splx(s);
        Ok(())
    }

    /// `blkfs`'s fsync: `vflushbuf`, waiting for `MNT_WAIT`, as a disk file system does.
    fn blkfs_fsync(ap: &mut VopFsyncArgs<'_>) -> Result<(), Errno> {
        vflushbuf(ap.a_vp, ap.a_waitfor == MNT_WAIT);
        Ok(())
    }

    fn blkfs_inactive(ap: &mut VopInactiveArgs<'_>) -> Result<(), Errno> {
        crate::kern::vfs_vops::VOP_UNLOCK(ap.a_vp)
    }

    /// `vops` of `blkfs`: a strategy, the generic `bwrite`, no locking.
    static BLKFS_VOPS: Vops = Vops {
        vop_lock: Some(|_| nullop()),
        vop_unlock: Some(|_| nullop()),
        vop_islocked: Some(|_| 0),
        vop_inactive: Some(blkfs_inactive),
        vop_reclaim: Some(|_| nullop()),
        vop_strategy: Some(blkfs_strategy),
        vop_bwrite: Some(vop_generic_bwrite),
        vop_fsync: Some(blkfs_fsync),
        ..Vops::EMPTY
    };

    /// Memory, the vnode table, a fresh buffer cache, a disk whose sector `i` holds the byte
    /// `i`, and a `blkfs` vnode.
    fn setup() -> (MutexGuard<'static, ()>, &'static Vnode) {
        let (g, p) = crate::kern::vfs_subr::tests::setup();
        Machine::set_curproc(Machine::curcpu(), p);

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

        let mut data = vec![0u8; DISK_BLOCKS * DEV_BSIZE];
        for (i, sector) in data.chunks_mut(DEV_BSIZE).enumerate() {
            sector.fill(i as u8);
        }
        *DISK.lock().unwrap_or_else(|e| e.into_inner()) = Some(Disk {
            data,
            reads: 0,
            writes: 0,
            bad: -1,
        });

        let vp = getnewvnode(VT_NON, None, &BLKFS_VOPS).expect("a vnode");
        vp.v_type.set(VREG);
        (g, vp)
    }

    fn teardown() {
        Machine::set_curproc(Machine::curcpu(), ptr::null());
    }

    /// Whether `bp` is on `vp`'s list `which` (clean or dirty).
    fn on_list(list: &crate::sys::vnode::Buflists, bp: &Buf) -> bool {
        list.iter().any(|b| ptr::eq(b, bp))
    }

    /// The number of buffers on a cache queue.
    fn qlen(q: &Bufqueue) -> usize {
        q.iter().count()
    }

    #[test]
    fn getblk_brelse_and_incore_keep_the_books() {
        let (_g, vp) = setup();
        let size = PAGE_SIZE as i32;

        assert!(incore(vp, 5).is_none());
        let bp = getblk(vp, 5, size, 0, INFSLP).expect("a buffer");
        assert!(bp.isset(B_BUSY) && bp.isset(B_BC) && !bp.isset(B_CACHE));
        assert_eq!((bp.b_lblkno.get(), bp.b_blkno.get()), (5, 5));
        assert!(bp.b_vp.get().is_some_and(|b| ptr::eq(b, vp)));
        assert!(on_list(&vp.v_cleanblkhd, bp));
        assert_eq!(vp.v_holdcnt.get(), 1);
        assert_eq!(BCSTATS.numbufs.load(Ordering::Relaxed), 1);
        assert_eq!(BCSTATS.numbufpages.load(Ordering::Relaxed), 1);
        assert_eq!(BCSTATS.busymapped.load(Ordering::Relaxed), 1);
        assert!(incore(vp, 5).is_some_and(|b| ptr::eq(b, bp)));
        // The mapping is usable.
        // SAFETY: the buffer is busy for this test and mapped.
        unsafe { bp.data() }.fill(0xa5);

        brelse(bp);
        assert!(!bp.isset(B_BUSY));
        assert_eq!(qlen(&CLEANCACHE.hotqueue), 1);
        assert_eq!(BCSTATS.numcleanpages.load(Ordering::Relaxed), 1);
        assert_eq!(BCSTATS.busymapped.load(Ordering::Relaxed), 0);

        // A second getblk finds it in the cache, with its data.
        let again = getblk(vp, 5, size, 0, INFSLP).expect("the buffer");
        assert!(ptr::eq(again, bp));
        assert!(again.isset(B_CACHE));
        assert_eq!(BCSTATS.cachehits.load(Ordering::Relaxed), 1);
        assert_eq!(qlen(&CLEANCACHE.hotqueue), 0);
        // SAFETY: as above.
        assert!(unsafe { again.data() }.iter().all(|&b| b == 0xa5));
        brelse(again);

        // An invalidated buffer is freed by brelse, and incore no longer finds the block.
        let bp = getblk(vp, 5, size, 0, INFSLP).expect("the buffer");
        bp.set(B_INVAL);
        brelse(bp);
        assert!(incore(vp, 5).is_none());
        assert!(vp.v_cleanblkhd.is_empty());
        assert_eq!(vp.v_holdcnt.get(), 0);
        assert_eq!(BCSTATS.numbufs.load(Ordering::Relaxed), 0);
        assert_eq!(BCSTATS.numbufpages.load(Ordering::Relaxed), 0);
        teardown();
    }

    #[test]
    fn bread_reads_once_and_reports_errors() {
        let (_g, vp) = setup();
        let size = PAGE_SIZE as i32;

        let (bp, error) = bread(vp, 8, size);
        assert_eq!(error, Ok(()));
        assert!(bp.isset(B_DONE) && !bp.isset(B_CACHE));
        // SAFETY: `bread` returned it busy and mapped.
        let data = unsafe { bp.data() };
        assert_eq!(data[0], 8);
        assert_eq!(data[DEV_BSIZE], 9);
        assert_eq!(disk(|d| d.reads), 1);
        assert_eq!(BCSTATS.numreads.load(Ordering::Relaxed), 1);
        assert_eq!(BCSTATS.pendingreads.load(Ordering::Relaxed), 0);
        brelse(bp);

        // The second read is a cache hit: no I/O.
        let (bp, error) = bread(vp, 8, size);
        assert_eq!(error, Ok(()));
        assert!(bp.isset(B_CACHE));
        assert_eq!(disk(|d| d.reads), 1);
        brelse(bp);

        // A failing read returns the buffer and the error; brelse throws it away.
        disk(|d| d.bad = 40);
        let (bp, error) = bread(vp, 40, size);
        assert_eq!(error, Err(Errno::EIO));
        brelse(bp);
        assert!(incore(vp, 40).is_none());

        // breadn starts the read-ahead too.
        let (bp, error) = breadn(vp, 16, size, &[24], &[size]);
        assert_eq!(error, Ok(()));
        assert!(incore(vp, 24).is_some());
        assert_eq!(disk(|d| d.reads), 3);
        brelse(bp);

        vinvalbuf(vp, 0, NOCRED, None, 0, INFSLP).expect("invalidated");
        assert_eq!(BCSTATS.numbufs.load(Ordering::Relaxed), 0);
        teardown();
    }

    #[test]
    fn delayed_writes_go_dirty_and_vflushbuf_writes_them() {
        let (_g, vp) = setup();
        let size = PAGE_SIZE as i32;

        let bp = getblk(vp, 32, size, 0, INFSLP).expect("a buffer");
        // SAFETY: busy and mapped.
        unsafe { bp.data() }.fill(0x5a);
        bdwrite(bp);
        assert!(bp.isset(B_DELWRI) && !bp.isset(B_BUSY));
        assert!(on_list(&vp.v_dirtyblkhd, bp));
        assert!(vp.v_bioflag.get() & VBIOONSYNCLIST != 0);
        assert_eq!(BCSTATS.delwribufs.load(Ordering::Relaxed), 1);
        assert_eq!(BCSTATS.numdirtypages.load(Ordering::Relaxed), 1);
        assert!(bufcache_getdirtybuf().is_some_and(|b| ptr::eq(b, bp)));
        assert_eq!(disk(|d| d.writes), 0);

        vflushbuf(vp, true);
        assert_eq!(disk(|d| d.writes), 1);
        assert!(disk(|d| d.data[32 * DEV_BSIZE..32 * DEV_BSIZE + PAGE_SIZE]
            .iter()
            .all(|&b| b == 0x5a)));
        assert!(vp.v_dirtyblkhd.is_empty());
        assert!(on_list(&vp.v_cleanblkhd, bp));
        assert!(vp.v_bioflag.get() & VBIOONSYNCLIST == 0);
        assert_eq!(vp.v_numoutput.get(), 0);
        assert_eq!(BCSTATS.delwribufs.load(Ordering::Relaxed), 0);
        assert_eq!(BCSTATS.pendingwrites.load(Ordering::Relaxed), 0);

        // A synchronous bwrite writes and releases.
        let bp = getblk(vp, 48, size, 0, INFSLP).expect("a buffer");
        // SAFETY: busy and mapped.
        unsafe { bp.data() }.fill(0x77);
        assert_eq!(bwrite(bp), Ok(()));
        assert!(!bp.isset(B_BUSY));
        assert_eq!(disk(|d| d.data[48 * DEV_BSIZE]), 0x77);

        // vinvalbuf with V_SAVE writes a dirty buffer before throwing it away.
        let bp = getblk(vp, 64, size, 0, INFSLP).expect("a buffer");
        // SAFETY: busy and mapped.
        unsafe { bp.data() }.fill(0x33);
        bdwrite(bp);
        let p = crate::machine::cpu::curproc();
        // blkfs's fsync writes the dirty buffer, then the loop throws the clean one away.
        vinvalbuf(vp, crate::sys::vnode::V_SAVE, NOCRED, p, 0, INFSLP).expect("flushed");
        assert_eq!(disk(|d| d.data[64 * DEV_BSIZE]), 0x33);
        assert!(vp.v_dirtyblkhd.is_empty() && vp.v_cleanblkhd.is_empty());
        assert_eq!(BCSTATS.numbufs.load(Ordering::Relaxed), 0);
        teardown();
    }

    #[test]
    fn released_buffers_move_through_the_2q_queues() {
        let (_g, vp) = setup();
        let size = PAGE_SIZE as i32;

        // 100 one-page buffers released in order: the hot queue holds at most 96 pages (the
        // minimum before chilling), the oldest four go cold.
        for blk in 0..100 {
            let bp = getblk(vp, blk, size, 0, INFSLP).expect("a buffer");
            brelse(bp);
        }
        assert_eq!(qlen(&CLEANCACHE.hotqueue), 96);
        assert_eq!(qlen(&CLEANCACHE.coldqueue), 4);
        assert_eq!(CLEANCACHE.hotbufpages.get(), 96);
        assert_eq!(CLEANCACHE.cachepages.get(), 100);
        let cold: Vec<Daddr> = CLEANCACHE
            .coldqueue
            .iter()
            .map(|b| b.b_lblkno.get())
            .collect();
        assert_eq!(cold, [0, 1, 2, 3]);
        assert!(CLEANCACHE.coldqueue.iter().all(|b| b.isset(B_COLD)));

        // A cold buffer used again becomes warm.
        let bp = getblk(vp, 2, size, 0, INFSLP).expect("the buffer");
        assert!(bp.isset(B_CACHE));
        brelse(bp);
        assert!(bp.isset(B_WARM) && !bp.isset(B_COLD));
        assert_eq!(qlen(&CLEANCACHE.warmqueue), 1);
        assert_eq!(CLEANCACHE.warmbufpages.get(), 1);
        assert_eq!(qlen(&CLEANCACHE.coldqueue), 3);

        // Recovering pages takes the cold ones first, then warm, then hot.
        let s = splbio();
        assert_eq!(bufcache_recover_pages(false, 4), 4);
        splx(s);
        assert!(CLEANCACHE.coldqueue.is_empty());
        assert!(CLEANCACHE.warmqueue.is_empty());
        assert!(incore(vp, 0).is_none() && incore(vp, 2).is_none());
        assert!(incore(vp, 4).is_some());
        assert_eq!(BCSTATS.numbufs.load(Ordering::Relaxed), 96);

        // Every buffer is on the free lists exactly once.
        let mut n = 0;
        for b in CLEANCACHE.hotqueue.iter() {
            assert!(b.b_onfreelist.get());
            n += 1;
        }
        assert_eq!(n, 96);

        vinvalbuf(vp, 0, NOCRED, None, 0, INFSLP).expect("invalidated");
        assert_eq!(BCSTATS.numbufs.load(Ordering::Relaxed), 0);
        assert_eq!(CLEANCACHE.cachepages.get(), 0);
        teardown();
    }

    #[test]
    fn geteblk_gives_an_anonymous_invalid_buffer() {
        let (_g, _vp) = setup();
        let bp = geteblk(2 * PAGE_SIZE);
        assert!(bp.b_vp.get().is_none());
        assert!(bp.isset(B_INVAL) && bp.isset(B_BUSY));
        assert_eq!(bp.b_bufsize.get(), 2 * PAGE_SIZE as i64);
        assert_eq!(bp.b_dev.get(), NODEV);
        brelse(bp);
        assert_eq!(BCSTATS.numbufs.load(Ordering::Relaxed), 0);
        teardown();
    }
}
/* </TESTS> */
