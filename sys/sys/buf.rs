/*	$OpenBSD: buf.h,v 1.123 2026/08/03 03:27:45 jsg Exp $	*/
/*	$NetBSD: buf.h,v 1.25 1997/04/09 21:12:17 mycroft Exp $	*/
/* <LICENSES> */
/*
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
 *	@(#)buf.h	8.7 (Berkeley) 1/21/94
 */
/* </LICENSES> */

//! `<sys/buf.h>`: the buffer header that describes an I/O operation in the kernel (`struct
//! buf`), its `B_*` flags, the disk queues (`struct bufq`) and the 2Q cache's queues
//! (`struct bufcache`), and the interface of the buffer cache (`vfs_bio.c`,
//! `vfs_biomem.c`, `kern_bufq.c`).
//!
//! Upstream: sys/sys/buf.h @ 3ce1f3f79392
//!
//! A buffer is a `bufpool` item, so it travels as `&'static Buf` with `Cell` members, as a
//! vnode or an mbuf does: the C mutates buffers through shared pointers at `splbio`, and the
//! `B_BUSY` flag says who owns one. `buf_put` hands it back to the pool; nobody touches it
//! after that, as in C.
//!
//! ## Deviations
//! - `NOLIST`, the sentinel the C writes into `b_vnbufs.le_next` and `b_freelist.tqe_next` of
//!   a buffer on no vnode list or free list, is two flags, `b_onvnbufs` and `b_onfreelist`:
//!   the queue links are private to `sys/queue.rs`.
//! - `union bufq_data` (`bufq_data_fifo` and `bufq_data_nscan`, both one `SIMPLEQ_ENTRY`) is
//!   one link, `b_bufq`, which both disciplines use through [`BufqEntries`].
//! - `b_error` is `Option<Errno>`: `None` is the C's 0.
//! - `b_iodone` is an `Option<fn(&'static Buf)>`, `b_data` a raw pointer (the buffer's kernel
//!   mapping, or NULL), `b_saveaddr` a `*mut c_void` as in C.
//! - `b_flags` is an `i64` (the C's `volatile long`) changed at `splbio`; the `B_*` flags are
//!   `i64` constants.
//! - `UNCLEAN_PAGES` is [`unclean_pages`]; `clrbuf` is a function; the prototypes are the
//!   functions of `vfs_bio.rs`, `vfs_biomem.rs`, `kern_bufq.rs` and `vfs_subr.rs`.
//! - `struct bufq_impl` is `kern_bufq.c`'s, so `bufq_impl` names `kern_bufq.rs`'s
//!   [`BufqImpl`].

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::Ordering;

use crate::kern::kern_bufq::BufqImpl;
use crate::kern::vfs_bio::BCSTATS;
use crate::machine::intr::IPL_BIO;
use crate::queue_adapter;
use crate::sys::errno::Errno;
use crate::sys::mutex::Mutex;
use crate::sys::param::{MAXPHYS, PAGE_SIZE};
use crate::sys::proc::Proc;
use crate::sys::queue::{
    ListEntry, ListHead, SimpleqEntry, SimpleqHead, SlistEntry, TailqEntry, TailqHead,
};
use crate::sys::tree::RbtEntry;
use crate::sys::types::{Daddr, Dev, Off};
use crate::sys::vnode::Vnode;
use crate::uvm::uvm_object::UvmObject;

// Buffer queues

/// `BUFQ_NSCAN_N`: the segment the nscan discipline sorts at a time.
pub const BUFQ_NSCAN_N: i32 = 128;
/// `BUFQ_FIFO`.
pub const BUFQ_FIFO: i32 = 0;
/// `BUFQ_NSCAN`.
pub const BUFQ_NSCAN: i32 = 1;
/// `BUFQ_DEFAULT`.
pub const BUFQ_DEFAULT: i32 = BUFQ_NSCAN;
/// `BUFQ_HOWMANY`.
pub const BUFQ_HOWMANY: usize = 2;

// Write limits for bufq - defines high and low water marks for how many kva slots are allowed
// to be consumed to parallelize writes from the buffer cache from any individual bufq.

/// `BUFQ_HI`.
pub const BUFQ_HI: u32 = 128;
/// `BUFQ_LOW`.
pub const BUFQ_LOW: u32 = 64;

/// `struct bufq`: a disk's queue of buffers, with the discipline `bufq_impl` keeps in
/// `bufq_data`.
///
/// Protected by: `bufq_mtx` (the list of all queues by `bufqs_mtx`).
pub struct Bufq {
    /// `bufq_entries`: the link in `bufqs`.
    pub bufq_entries: SlistEntry<Bufq>,
    /// `bufq_mtx`.
    pub bufq_mtx: Mutex,
    /// `bufq_data`: the discipline's head, from its `impl_create`.
    pub bufq_data: Cell<*mut c_void>,
    /// `bufq_outstanding`: buffers queued and not yet done.
    pub bufq_outstanding: Cell<u32>,
    /// `bufq_hi`: `bufq_wait` sleeps at this many outstanding buffers.
    pub bufq_hi: Cell<u32>,
    /// `bufq_low`: and wakes below this many.
    pub bufq_low: Cell<u32>,
    /// `bufq_waiting`: the writers sleeping in `bufq_wait`.
    pub bufq_waiting: Cell<i32>,
    /// `bufq_stop`: `bufq_quiesce` stopped the queue.
    pub bufq_stop: Cell<i32>,
    /// `bufq_type`: `BUFQ_FIFO` or `BUFQ_NSCAN`.
    pub bufq_type: Cell<i32>,
    /// `bufq_impl`: the discipline.
    pub bufq_impl: Cell<Option<&'static BufqImpl>>,
}

// SAFETY: the members are changed under `bufq_mtx` (or `bufqs_mtx` for the link), as in C;
// the kernel runs one CPU.
unsafe impl Sync for Bufq {}

impl Bufq {
    /// A queue with no discipline, for `bufq_init` to set up (a disk's softc holds one).
    pub const fn new() -> Self {
        Self {
            bufq_entries: SlistEntry::new(),
            bufq_mtx: Mutex::new(IPL_BIO),
            bufq_data: Cell::new(ptr::null_mut()),
            bufq_outstanding: Cell::new(0),
            bufq_hi: Cell::new(0),
            bufq_low: Cell::new(0),
            bufq_waiting: Cell::new(0),
            bufq_stop: Cell::new(0),
            bufq_type: Cell::new(0),
            bufq_impl: Cell::new(None),
        }
    }
}

impl Default for Bufq {
    fn default() -> Self {
        Self::new()
    }
}

queue_adapter!(
    /// `SLIST_HEAD(, bufq) bufqs`: every initialised queue, through `bufq_entries`.
    pub BufqList: Bufq, bufq_entries => SlistEntry<Bufq>
);

queue_adapter!(
    /// `SIMPLEQ_HEAD(bufq_fifo_head, buf)` and `SIMPLEQ_HEAD(bufq_nscan_head, buf)`: a
    /// discipline's buffers, through `b_bufq` (the C's `bqf_entries` of either union member).
    pub BufqEntries: Buf, b_bufq => SimpleqEntry<Buf>
);

/// `struct bufq_fifo_head`.
pub type BufqFifoHead = SimpleqHead<BufqEntries>;
/// `struct bufq_nscan_head`.
pub type BufqNscanHead = SimpleqHead<BufqEntries>;

/// `struct buf`: the buffer header describes an I/O operation in the kernel.
///
/// Locks: `splbio` for the lists and the flags; the rest belongs to whoever has it `B_BUSY`.
pub struct Buf {
    /// `b_rbbufs`: vnode "hash" tree.
    pub b_rbbufs: RbtEntry,
    /// `b_list`: all allocated buffers.
    pub b_list: ListEntry<Buf>,
    /// `b_vnbufs`: buffer's associated vnode.
    pub b_vnbufs: ListEntry<Buf>,
    /// Whether `b_vnbufs` is linked (the C's `LIST_NEXT(bp, b_vnbufs) != NOLIST`).
    pub b_onvnbufs: Cell<bool>,
    /// `b_freelist`: free list position if not active.
    pub b_freelist: TailqEntry<Buf>,
    /// Whether `b_freelist` is linked (the C's `b_freelist.tqe_next != NOLIST`).
    pub b_onfreelist: Cell<bool>,
    /// `b_proc`: associated proc; NULL if kernel.
    pub b_proc: Cell<*const Proc>,
    /// `b_flags`: `B_*` flags.
    pub b_flags: Cell<i64>,
    /// `b_bufsize`: allocated buffer size.
    pub b_bufsize: Cell<i64>,
    /// `b_bcount`: valid bytes in buffer.
    pub b_bcount: Cell<i64>,
    /// `b_resid`: remaining I/O.
    pub b_resid: Cell<usize>,
    /// `b_error`: errno value.
    pub b_error: Cell<Option<Errno>>,
    /// `b_dev`: device associated with buffer.
    pub b_dev: Cell<Dev>,
    /// `b_data`: associated data.
    pub b_data: Cell<*mut u8>,
    /// `b_saveaddr`: original `b_data` for physio.
    pub b_saveaddr: Cell<*mut c_void>,
    /// `b_valist`: LRU of va to reuse.
    pub b_valist: TailqEntry<Buf>,
    /// `b_bufq`: the link of the disk queue's discipline (`union bufq_data`).
    pub b_bufq: SimpleqEntry<Buf>,
    /// `b_bq`: what bufq this buf is on.
    pub b_bq: Cell<Option<&'static Bufq>>,
    /// `b_pobj`: the object holding the pages (`b_uobj`, or a cluster's first buffer's).
    pub b_pobj: Cell<*const UvmObject>,
    /// `b_uobj`: object containing the pages.
    pub b_uobj: UvmObject,
    /// `b_poffs`: offset within object.
    pub b_poffs: Cell<Off>,
    /// `b_lblkno`: logical block number.
    pub b_lblkno: Cell<Daddr>,
    /// `b_blkno`: underlying physical block number.
    pub b_blkno: Cell<Daddr>,
    /// `b_iodone`: function to call upon completion. Will be called at `splbio()`.
    pub b_iodone: Cell<Option<fn(&'static Buf)>>,
    /// `b_vp`: device vnode.
    pub b_vp: Cell<Option<&'static Vnode>>,
    /// `b_dirtyoff`: offset in buffer of dirty region.
    pub b_dirtyoff: Cell<i32>,
    /// `b_dirtyend`: offset of end of dirty region.
    pub b_dirtyend: Cell<i32>,
    /// `b_validoff`: offset in buffer of valid region.
    pub b_validoff: Cell<i32>,
    /// `b_validend`: offset of end of valid region.
    pub b_validend: Cell<i32>,
}

// SAFETY: the members are changed at `splbio` or by the buffer's `B_BUSY` owner, as in C; the
// kernel runs one CPU.
unsafe impl Sync for Buf {}

impl Buf {
    /// A zeroed buffer, as `pool_get(&bufpool, PR_ZERO)` returns it.
    pub const fn new() -> Self {
        Self {
            b_rbbufs: RbtEntry::new(),
            b_list: ListEntry::new(),
            b_vnbufs: ListEntry::new(),
            b_onvnbufs: Cell::new(false),
            b_freelist: TailqEntry::new(),
            b_onfreelist: Cell::new(false),
            b_proc: Cell::new(ptr::null()),
            b_flags: Cell::new(0),
            b_bufsize: Cell::new(0),
            b_bcount: Cell::new(0),
            b_resid: Cell::new(0),
            b_error: Cell::new(None),
            b_dev: Cell::new(0),
            b_data: Cell::new(ptr::null_mut()),
            b_saveaddr: Cell::new(ptr::null_mut()),
            b_valist: TailqEntry::new(),
            b_bufq: SimpleqEntry::new(),
            b_bq: Cell::new(None),
            b_pobj: Cell::new(ptr::null()),
            b_uobj: UvmObject::new(0),
            b_poffs: Cell::new(0),
            b_lblkno: Cell::new(0),
            b_blkno: Cell::new(0),
            b_iodone: Cell::new(None),
            b_vp: Cell::new(None),
            b_dirtyoff: Cell::new(0),
            b_dirtyend: Cell::new(0),
            b_validoff: Cell::new(0),
            b_validend: Cell::new(0),
        }
    }

    /// `ISSET(bp->b_flags, f)`.
    pub fn isset(&self, f: i64) -> bool {
        self.b_flags.get() & f != 0
    }

    /// `SET(bp->b_flags, f)`.
    pub fn set(&self, f: i64) {
        self.b_flags.set(self.b_flags.get() | f);
    }

    /// `CLR(bp->b_flags, f)`.
    pub fn clr(&self, f: i64) {
        self.b_flags.set(self.b_flags.get() & !f);
    }

    /// The buffer's data: `b_bcount` bytes at `b_data`.
    ///
    /// # Safety
    ///
    /// The caller owns the buffer (it is `B_BUSY` for the caller, as `bread`, `getblk` and
    /// `geteblk` return it), the buffer is mapped (`b_data` is not NULL) and no other slice of
    /// it is alive.
    #[allow(clippy::mut_from_ref)] // the B_BUSY owner's view, as the C's b_data
    pub unsafe fn data(&self) -> &mut [u8] {
        let len = usize::try_from(self.b_bcount.get()).unwrap_or(0);
        // SAFETY: the caller's contract; a mapped buffer has `b_bufsize >= b_bcount` bytes at
        // `b_data` (`buf_map`).
        unsafe { core::slice::from_raw_parts_mut(self.b_data.get(), len) }
    }

    /// The buffer's object (`b_pobj`).
    pub fn pobj(&self) -> Option<&UvmObject> {
        // SAFETY: `b_pobj` is NULL, the buffer's own `b_uobj` or the `b_uobj` of the first
        // buffer of a read cluster, which outlives the cluster's I/O (`bread_cluster`).
        unsafe { self.b_pobj.get().as_ref() }
    }
}

impl Default for Buf {
    fn default() -> Self {
        Self::new()
    }
}

queue_adapter!(
    /// `LIST_HEAD(bufhead, buf)`: all allocated buffers, through `b_list`.
    pub BList: Buf, b_list => ListEntry<Buf>
);

/// `struct bufhead`.
pub type Bufhead = ListHead<BList>;

queue_adapter!(
    /// `TAILQ_HEAD(bufqueue, buf)`: the cache's queues, through `b_freelist`.
    pub BFreelist: Buf, b_freelist => TailqEntry<Buf>
);

queue_adapter!(
    /// `TAILQ_HEAD(, buf) buf_valist`: mapped buffers whose kva may be reused, through
    /// `b_valist`.
    pub BValist: Buf, b_valist => TailqEntry<Buf>
);

/// `struct bufqueue`.
pub type Bufqueue = TailqHead<BFreelist>;

/// `struct bufcache`: one 2Q cache (`vfs_bio.c`).
///
/// Protected by: `splbio`.
pub struct Bufcache {
    /// `hotbufpages`.
    pub hotbufpages: Cell<i64>,
    /// `warmbufpages`.
    pub warmbufpages: Cell<i64>,
    /// `cachepages`.
    pub cachepages: Cell<i64>,
    /// `hotqueue`.
    pub hotqueue: Bufqueue,
    /// `coldqueue`.
    pub coldqueue: Bufqueue,
    /// `warmqueue`.
    pub warmqueue: Bufqueue,
}

// SAFETY: changed at `splbio`, as in C; the kernel runs one CPU.
unsafe impl Sync for Bufcache {}

impl Bufcache {
    /// An empty cache (`bufcache_init` initialises the queues).
    pub const fn new() -> Self {
        Self {
            hotbufpages: Cell::new(0),
            warmbufpages: Cell::new(0),
            cachepages: Cell::new(0),
            hotqueue: TailqHead::new(),
            coldqueue: TailqHead::new(),
            warmqueue: TailqHead::new(),
        }
    }
}

impl Default for Bufcache {
    fn default() -> Self {
        Self::new()
    }
}

// These flags are kept in b_flags.

/// `B_WRITE`: write buffer (pseudo flag).
pub const B_WRITE: i64 = 0x00000000;
/// `B_AGE`: move to age queue when I/O done.
pub const B_AGE: i64 = 0x00000001;
/// `B_NEEDCOMMIT`: needs committing to stable storage.
pub const B_NEEDCOMMIT: i64 = 0x00000002;
/// `B_ASYNC`: start I/O, do not wait.
pub const B_ASYNC: i64 = 0x00000004;
/// `B_BAD`: bad block revectoring in progress.
pub const B_BAD: i64 = 0x00000008;
/// `B_BUSY`: I/O in progress.
pub const B_BUSY: i64 = 0x00000010;
/// `B_CACHE`: bread found us in the cache.
pub const B_CACHE: i64 = 0x00000020;
/// `B_CALL`: call `b_iodone` from biodone.
pub const B_CALL: i64 = 0x00000040;
/// `B_DELWRI`: delay I/O until buffer reused.
pub const B_DELWRI: i64 = 0x00000080;
/// `B_DONE`: I/O completed.
pub const B_DONE: i64 = 0x00000100;
/// `B_EINTR`: I/O was interrupted.
pub const B_EINTR: i64 = 0x00000200;
/// `B_ERROR`: I/O error occurred.
pub const B_ERROR: i64 = 0x00000400;
/// `B_INVAL`: does not contain valid info.
pub const B_INVAL: i64 = 0x00000800;
/// `B_NOCACHE`: do not cache block after use.
pub const B_NOCACHE: i64 = 0x00001000;
/// `B_PHYS`: I/O to user memory.
pub const B_PHYS: i64 = 0x00002000;
/// `B_RAW`: set by physio for raw transfers.
pub const B_RAW: i64 = 0x00004000;
/// `B_READ`: read buffer.
pub const B_READ: i64 = 0x00008000;
/// `B_WANTED`: process wants this buffer.
pub const B_WANTED: i64 = 0x00010000;
/// `B_WRITEINPROG`: write in progress.
pub const B_WRITEINPROG: i64 = 0x00020000;
/// `B_XXX`: debugging flag.
pub const B_XXX: i64 = 0x00040000;
/// `B_DEFERRED`: skipped over for cleaning.
pub const B_DEFERRED: i64 = 0x00080000;
/// `B_SCANNED`: block already pushed during sync.
pub const B_SCANNED: i64 = 0x00100000;
/// `B_PDAEMON`: I/O started by pagedaemon.
pub const B_PDAEMON: i64 = 0x00200000;
/// `B_RELEASED`: free this buffer after its kvm.
pub const B_RELEASED: i64 = 0x00400000;
/// `B_WARM`: buffer is or has been on the warm queue.
pub const B_WARM: i64 = 0x00800000;
/// `B_COLD`: buffer is on the cold queue.
pub const B_COLD: i64 = 0x01000000;
/// `B_BC`: buffer is managed by the cache.
pub const B_BC: i64 = 0x02000000;

/// `B_BITS`: the `%b` description of the flags.
pub const B_BITS: &[u8] = b"\x10\x01AGE\x02NEEDCOMMIT\x03ASYNC\x04BAD\x05BUSY\
\x06CACHE\x07CALL\x08DELWRI\x09DONE\x0aEINTR\x0bERROR\
\x0cINVAL\x0dNOCACHE\x0ePHYS\x0fRAW\x10READ\
\x11WANTED\x12WRITEINPROG\x13XXX(FORMAT)\x14DEFERRED\
\x15SCANNED\x16DAEMON\x17RELEASED\x18WARM\x19COLD\x1aBC";

/// Zero out the buffer's data area.
///
/// # Safety
///
/// As [`Buf::data`]: the caller owns the mapped buffer.
pub unsafe fn clrbuf(bp: &Buf) {
    // SAFETY: the caller's contract.
    unsafe { bp.data() }.fill(0);
    bp.b_resid.set(0);
}

// Flags to low-level allocation routines.

/// `B_CLRBUF`: request allocated buffer be cleared.
pub const B_CLRBUF: i32 = 0x01;
/// `B_SYNC`: do all allocations synchronously.
pub const B_SYNC: i32 = 0x02;

/// `struct cluster_info`.
#[derive(Clone, Copy, Debug, Default)]
pub struct ClusterInfo {
    /// `ci_lastr`: last read (read-ahead).
    pub ci_lastr: Daddr,
    /// `ci_lastw`: last write (write cluster).
    pub ci_lastw: Daddr,
    /// `ci_cstart`: start block of cluster.
    pub ci_cstart: Daddr,
    /// `ci_lasta`: last allocation.
    pub ci_lasta: Daddr,
    /// `ci_clen`: length of current cluster.
    pub ci_clen: i32,
    /// `ci_ralen`: read-ahead length.
    pub ci_ralen: i32,
    /// `ci_maxra`: last readahead block.
    pub ci_maxra: Daddr,
}

/// `RESERVE_SLOTS`: kva slots (of size `MAXPHYS`) reserved for syncer and cleaner.
pub const RESERVE_SLOTS: i64 = 4;
/// `RESERVE_PAGES`: buffer cache pages reserved for syncer and cleaner.
pub const RESERVE_PAGES: i64 = RESERVE_SLOTS * (MAXPHYS / PAGE_SIZE) as i64;
/// `BCACHE_MIN`: minimum size of the buffer cache, in pages.
pub const BCACHE_MIN: i64 = RESERVE_PAGES * 2;

/// `UNCLEAN_PAGES`: the cache's pages that are not clean (`bcstats.numbufpages -
/// bcstats.numcleanpages`).
pub fn unclean_pages() -> i64 {
    BCSTATS.numbufpages.load(Ordering::Relaxed) - BCSTATS.numcleanpages.load(Ordering::Relaxed)
}
