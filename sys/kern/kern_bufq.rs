/*	$OpenBSD: kern_bufq.c,v 1.36 2025/05/17 10:13:40 jsg Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 2010 Thordur I. Bjornsson <thib@openbsd.org>
 * Copyright (c) 2010 David Gwynne <dlg@openbsd.org>
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
//! Disk buffer queues (`bufq_init(9)`): the queue a disk driver keeps its pending buffers on,
//! with a pluggable discipline (`fifo`, or `nscan`, which sorts segments of up to
//! `BUFQ_NSCAN_N` buffers by block number), the high and low water marks that throttle
//! writers (`bufq_wait`, `bufq_done`), and the quiescing of every queue for suspend.
//!
//! Upstream: sys/kern/kern_bufq.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - The discipline's head (`bufq_data`, a `void *` from `impl_create`) stays a `*mut c_void`;
//!   the `impl_*` function pointers are `unsafe fn`s whose contract is that the pointer is
//!   their own `impl_create`'s, and `impl_create` returns `Option<NonNull<c_void>>`.
//! - `impl_peek` returns `bool`.
//! - `bufqs` is a `Sync` static list changed under `bufqs_mtx`; `bufqs_stop` an atomic.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicI32, Ordering};

use crate::kassert;
use crate::kern::kern_lock::{mtx_enter, mtx_init, mtx_leave};
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_synch::{msleep_nsec, wakeup};
use crate::kern::subr_prf::panic;
use crate::kern::vfs_bio::{BCSTATS, biodone};
use crate::machine::intr::{IPL_BIO, IPL_NONE, splbio, splx};
use crate::sys::buf::{
    B_ERROR, BUFQ_HI, BUFQ_HOWMANY, BUFQ_LOW, BUFQ_NSCAN_N, Buf, Bufq, BufqFifoHead, BufqList,
    BufqNscanHead,
};
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_DEVBUF, M_NOWAIT, M_ZERO};
use crate::sys::mutex::Mutex;
use crate::sys::param::PRIBIO;
use crate::sys::queue::{SimpleqHead, SlistHead};
use crate::sys::systm::INFSLP;

/// `struct bufq_impl`: a queueing discipline.
pub struct BufqImpl {
    /// `impl_create`: a new, empty head.
    pub impl_create: fn() -> Option<NonNull<c_void>>,
    /// `impl_destroy`: frees an empty head.
    pub impl_destroy: unsafe fn(NonNull<c_void>),
    /// `impl_queue`: adds a buffer.
    pub impl_queue: unsafe fn(NonNull<c_void>, &'static Buf),
    /// `impl_dequeue`: takes the next buffer.
    pub impl_dequeue: unsafe fn(NonNull<c_void>) -> Option<&'static Buf>,
    /// `impl_peek`: whether a buffer is queued.
    pub impl_peek: unsafe fn(NonNull<c_void>) -> bool,
}

/// `bufqs`' type: a `Sync` static list.
pub struct Bufqs(pub SlistHead<BufqList>);

// SAFETY: changed under `bufqs_mtx`, as in C.
unsafe impl Sync for Bufqs {}

/// `bufqs`: every initialised queue.
pub static BUFQS: Bufqs = Bufqs(SlistHead::new());
/// `bufqs_mtx`.
pub static BUFQS_MTX: Mutex = Mutex::new(IPL_NONE);
/// `bufqs_stop`: `bufq_quiesce` stopped every queue.
pub static BUFQS_STOP: AtomicI32 = AtomicI32::new(0);

/// `bufq_impls[BUFQ_HOWMANY]`: the disciplines, by `BUFQ_*` number.
pub static BUFQ_IMPLS: [BufqImpl; BUFQ_HOWMANY] = [
    BufqImpl {
        impl_create: bufq_fifo_create,
        impl_destroy: bufq_fifo_destroy,
        impl_queue: bufq_fifo_queue,
        impl_dequeue: bufq_fifo_dequeue,
        impl_peek: bufq_fifo_peek,
    },
    BufqImpl {
        impl_create: bufq_nscan_create,
        impl_destroy: bufq_nscan_destroy,
        impl_queue: bufq_nscan_queue,
        impl_dequeue: bufq_nscan_dequeue,
        impl_peek: bufq_nscan_peek,
    },
];

/// The queue's discipline and its head, which `bufq_init` set up.
fn bq_impl(bq: &Bufq) -> (&'static BufqImpl, NonNull<c_void>) {
    match (bq.bufq_impl.get(), NonNull::new(bq.bufq_data.get())) {
        (Some(i), Some(d)) => (i, d),
        _ => panic(format_args!("bufq {:p} is not initialised", bq)),
    }
}

/// `bufq_init(bq, type)`: sets up a queue with discipline `type_` and links it on `bufqs`.
pub fn bufq_init(bq: &'static Bufq, type_: i32) -> Result<(), Errno> {
    let mut hi = BUFQ_HI;
    let mut low = BUFQ_LOW;

    if type_ < 0 || type_ as usize >= BUFQ_HOWMANY {
        panic(format_args!("bufq_init: type {type_} unknown"));
    }

    // Ensure that writes can't consume the entire amount of kva available the buffer cache if
    // we only have a limited amount of kva available to us.
    let kvaslots = BCSTATS.kvaslots.load(Ordering::Relaxed);
    if i64::from(hi) >= kvaslots / 16 {
        hi = (kvaslots / 16) as u32;
        if hi < 2 {
            hi = 2;
        }
        low = hi / 2;
    }

    mtx_init(&bq.bufq_mtx, IPL_BIO);
    bq.bufq_hi.set(hi);
    bq.bufq_low.set(low);
    bq.bufq_type.set(type_);
    let imp = &BUFQ_IMPLS[type_ as usize];
    bq.bufq_impl.set(Some(imp));
    let Some(data) = (imp.impl_create)() else {
        // we should actually return failure so disks attaching after boot in low memory
        // situations dont panic the system.
        panic(format_args!("bufq init fail"));
    };
    bq.bufq_data.set(data.as_ptr());

    mtx_enter(&BUFQS_MTX);
    while BUFQS_STOP.load(Ordering::Relaxed) != 0 {
        let _ = msleep_nsec(
            ptr::from_ref(&BUFQS_STOP),
            &BUFQS_MTX,
            PRIBIO,
            "bqinit",
            INFSLP,
        );
    }
    // SAFETY: a queue being initialised is on no list; the disk keeps it in place while it
    // is linked (`bufq_destroy` unlinks it).
    unsafe { BUFQS.0.insert_head(bq) };
    mtx_leave(&BUFQS_MTX);

    Ok(())
}

/// `bufq_destroy(bq)`: drains, frees and unlinks a queue.
pub fn bufq_destroy(bq: &'static Bufq) {
    bufq_drain(bq);

    let (imp, data) = bq_impl(bq);
    // SAFETY: `data` is the head this discipline's `impl_create` made for the queue.
    unsafe { (imp.impl_destroy)(data) };
    bq.bufq_data.set(ptr::null_mut());

    mtx_enter(&BUFQS_MTX);
    while BUFQS_STOP.load(Ordering::Relaxed) != 0 {
        let _ = msleep_nsec(
            ptr::from_ref(&BUFQS_STOP),
            &BUFQS_MTX,
            PRIBIO,
            "bqdest",
            INFSLP,
        );
    }
    // SAFETY: an initialised queue is on `bufqs` (`bufq_init`).
    unsafe { BUFQS.0.remove(bq) };
    mtx_leave(&BUFQS_MTX);
}

/// `bufq_queue(bq, bp)`: queues a buffer for the disk.
pub fn bufq_queue(bq: &'static Bufq, bp: &'static Buf) {
    mtx_enter(&bq.bufq_mtx);
    while bq.bufq_stop.get() != 0 {
        let _ = msleep_nsec(
            ptr::from_ref(&bq.bufq_stop),
            &bq.bufq_mtx,
            PRIBIO,
            "bqqueue",
            INFSLP,
        );
    }

    bp.b_bq.set(Some(bq));
    bq.bufq_outstanding.set(bq.bufq_outstanding.get() + 1);
    let (imp, data) = bq_impl(bq);
    // SAFETY: `data` is this discipline's head for the queue; the buffer is on no queue.
    unsafe { (imp.impl_queue)(data, bp) };
    mtx_leave(&bq.bufq_mtx);
}

/// `bufq_dequeue(bq)`: the next buffer for the disk.
pub fn bufq_dequeue(bq: &Bufq) -> Option<&'static Buf> {
    mtx_enter(&bq.bufq_mtx);
    let (imp, data) = bq_impl(bq);
    // SAFETY: `data` is this discipline's head for the queue.
    let bp = unsafe { (imp.impl_dequeue)(data) };
    mtx_leave(&bq.bufq_mtx);

    bp
}

/// `bufq_peek(bq)`: whether a buffer is queued.
pub fn bufq_peek(bq: &Bufq) -> bool {
    mtx_enter(&bq.bufq_mtx);
    let (imp, data) = bq_impl(bq);
    // SAFETY: `data` is this discipline's head for the queue.
    let rv = unsafe { (imp.impl_peek)(data) };
    mtx_leave(&bq.bufq_mtx);

    rv
}

/// `bufq_drain(bq)`: fails every queued buffer with `ENXIO`.
pub fn bufq_drain(bq: &Bufq) {
    while let Some(bp) = bufq_dequeue(bq) {
        bp.b_error.set(Some(Errno::ENXIO));
        bp.set(B_ERROR);
        let s = splbio();
        biodone(bp);
        splx(s);
    }
}

/// `bufq_wait(bq)`: a writer sleeps while the queue has `bufq_hi` buffers outstanding.
pub fn bufq_wait(bq: &Bufq) {
    if bq.bufq_hi.get() != 0 {
        crate::kern::subr_xxx::assertwaitok();
        mtx_enter(&bq.bufq_mtx);
        while bq.bufq_outstanding.get() >= bq.bufq_hi.get() {
            bq.bufq_waiting.set(bq.bufq_waiting.get() + 1);
            let _ = msleep_nsec(
                ptr::from_ref(&bq.bufq_waiting),
                &bq.bufq_mtx,
                PRIBIO,
                "bqwait",
                INFSLP,
            );
            bq.bufq_waiting.set(bq.bufq_waiting.get() - 1);
        }
        mtx_leave(&bq.bufq_mtx);
    }
}

/// `bufq_done(bq, bp)`: a queued buffer's I/O finished (`biodone`).
pub fn bufq_done(bq: &Bufq, bp: &Buf) {
    mtx_enter(&bq.bufq_mtx);
    kassert!(bq.bufq_outstanding.get() > 0);
    bq.bufq_outstanding.set(bq.bufq_outstanding.get() - 1);
    if bq.bufq_stop.get() != 0 && bq.bufq_outstanding.get() == 0 {
        wakeup(ptr::from_ref(&bq.bufq_outstanding));
    }
    if bq.bufq_waiting.get() != 0 && bq.bufq_outstanding.get() < bq.bufq_low.get() {
        wakeup(ptr::from_ref(&bq.bufq_waiting));
    }
    mtx_leave(&bq.bufq_mtx);
    bp.b_bq.set(None);
}

/// `bufq_quiesce()`: stops every queue and waits for its outstanding I/O.
pub fn bufq_quiesce() {
    mtx_enter(&BUFQS_MTX);
    BUFQS_STOP.store(1, Ordering::Relaxed);
    mtx_leave(&BUFQS_MTX);
    // We can safely walk the list since it can't be modified as long as bufqs_stop is
    // non-zero.
    for bq in BUFQS.0.iter() {
        mtx_enter(&bq.bufq_mtx);
        bq.bufq_stop.set(1);
        while bq.bufq_outstanding.get() != 0 {
            let _ = msleep_nsec(
                ptr::from_ref(&bq.bufq_outstanding),
                &bq.bufq_mtx,
                PRIBIO,
                "bqquies",
                INFSLP,
            );
        }
        mtx_leave(&bq.bufq_mtx);
    }
}

/// `bufq_restart()`: lets every queue run again.
pub fn bufq_restart() {
    mtx_enter(&BUFQS_MTX);
    for bq in BUFQS.0.iter() {
        mtx_enter(&bq.bufq_mtx);
        bq.bufq_stop.set(0);
        wakeup(ptr::from_ref(&bq.bufq_stop));
        mtx_leave(&bq.bufq_mtx);
    }
    BUFQS_STOP.store(0, Ordering::Relaxed);
    wakeup(ptr::from_ref(&BUFQS_STOP));
    mtx_leave(&BUFQS_MTX);
}

// fifo implementation

/// `bufq_fifo_create()`.
pub fn bufq_fifo_create() -> Option<NonNull<c_void>> {
    let head =
        malloc(size_of::<BufqFifoHead>(), M_DEVBUF, M_NOWAIT | M_ZERO)?.cast::<BufqFifoHead>();
    // SAFETY: a fresh, suitably aligned allocation of `size_of::<BufqFifoHead>()` bytes.
    unsafe {
        head.as_ptr().write(SimpleqHead::new());
        head.as_ref().init();
    }

    Some(head.cast())
}

/// The fifo head behind `data`.
///
/// # Safety
///
/// `data` came from `bufq_fifo_create` and is not destroyed.
unsafe fn fifo_head<'a>(data: NonNull<c_void>) -> &'a BufqFifoHead {
    // SAFETY: the caller's contract.
    unsafe { data.cast::<BufqFifoHead>().as_ref() }
}

/// `bufq_fifo_destroy(data)`.
///
/// # Safety
///
/// `data` came from `bufq_fifo_create`, and nothing uses it afterwards.
pub unsafe fn bufq_fifo_destroy(data: NonNull<c_void>) {
    free(data.cast(), M_DEVBUF, size_of::<BufqFifoHead>());
}

/// `bufq_fifo_queue(data, bp)`.
///
/// # Safety
///
/// `data` came from `bufq_fifo_create`; `bp` is on no disk queue.
pub unsafe fn bufq_fifo_queue(data: NonNull<c_void>, bp: &'static Buf) {
    // SAFETY: the caller's contract; the buffer stays in its pool item while queued.
    unsafe { fifo_head(data).insert_tail(bp) };
}

/// `bufq_fifo_dequeue(data)`.
///
/// # Safety
///
/// `data` came from `bufq_fifo_create`.
pub unsafe fn bufq_fifo_dequeue(data: NonNull<c_void>) -> Option<&'static Buf> {
    // SAFETY: the caller's contract.
    let head = unsafe { fifo_head(data) };
    let bp = head.first()?;
    // SAFETY: `bp` is the head of the queue.
    unsafe { head.remove_head() };
    // SAFETY: queued buffers are `&'static` pool items (`bufq_fifo_queue`).
    Some(unsafe { &*ptr::from_ref(bp) })
}

/// `bufq_fifo_peek(data)`.
///
/// # Safety
///
/// `data` came from `bufq_fifo_create`.
pub unsafe fn bufq_fifo_peek(data: NonNull<c_void>) -> bool {
    // SAFETY: the caller's contract.
    unsafe { fifo_head(data) }.first().is_some()
}

// nscan implementation

/// `BUF_INORDER(ba, bb)`.
fn buf_inorder(ba: &Buf, bb: &Buf) -> bool {
    ba.b_blkno.get() < bb.b_blkno.get()
}

/// `struct bufq_nscan_data`.
pub struct BufqNscanData {
    /// `sorted`: the segment being served, by block.
    pub sorted: BufqNscanHead,
    /// `fifo`: the buffers waiting for the next segment.
    pub fifo: BufqNscanHead,
    /// `leftoverroom`: remaining number of buffer inserts allowed.
    pub leftoverroom: Cell<i32>,
}

/// `bufq_simple_nscan(head, bp)`: inserts `bp` in block order.
///
/// # Safety
///
/// `bp` is on no disk queue and stays in place while queued.
pub unsafe fn bufq_simple_nscan(head: &BufqNscanHead, bp: &'static Buf) {
    let mut prev = None;
    // We look for the first slot where we would fit, then insert after the element we just
    // passed.
    for cur in head.iter() {
        if buf_inorder(bp, cur) {
            break;
        }
        prev = Some(cur);
    }
    // SAFETY: the caller's contract; `prev` is on `head`.
    unsafe {
        match prev {
            Some(prev) => head.insert_after(prev, bp),
            None => head.insert_head(bp),
        }
    }
}

/// Take N elements from the fifo queue and sort them
pub fn bufq_nscan_resort(data: &BufqNscanData) {
    let fifo = &data.fifo;
    let sorted = &data.sorted;
    let segmentsize = BUFQ_NSCAN_N;

    let mut count = 0;
    while count < segmentsize {
        let Some(bp) = fifo.first() else {
            break;
        };
        // SAFETY: queued buffers are `&'static` pool items (`bufq_nscan_queue`).
        let bp: &'static Buf = unsafe { &*ptr::from_ref(bp) };
        // SAFETY: `bp` is the head of the fifo; it moves to the sorted queue.
        unsafe {
            fifo.remove_head();
            bufq_simple_nscan(sorted, bp);
        }
        count += 1;
    }
    data.leftoverroom.set(segmentsize - count);
}

/// The nscan data behind `data`.
///
/// # Safety
///
/// `data` came from `bufq_nscan_create` and is not destroyed.
unsafe fn nscan_data<'a>(data: NonNull<c_void>) -> &'a BufqNscanData {
    // SAFETY: the caller's contract.
    unsafe { data.cast::<BufqNscanData>().as_ref() }
}

/// `bufq_nscan_create()`.
pub fn bufq_nscan_create() -> Option<NonNull<c_void>> {
    let data =
        malloc(size_of::<BufqNscanData>(), M_DEVBUF, M_NOWAIT | M_ZERO)?.cast::<BufqNscanData>();
    // SAFETY: a fresh, suitably aligned allocation of `size_of::<BufqNscanData>()` bytes.
    unsafe {
        data.as_ptr().write(BufqNscanData {
            sorted: SimpleqHead::new(),
            fifo: SimpleqHead::new(),
            leftoverroom: Cell::new(0),
        });
        data.as_ref().sorted.init();
        data.as_ref().fifo.init();
    }

    Some(data.cast())
}

/// `bufq_nscan_destroy(vdata)`.
///
/// # Safety
///
/// `vdata` came from `bufq_nscan_create`, and nothing uses it afterwards.
pub unsafe fn bufq_nscan_destroy(vdata: NonNull<c_void>) {
    free(vdata.cast(), M_DEVBUF, size_of::<BufqNscanData>());
}

/// `bufq_nscan_queue(vdata, bp)`.
///
/// # Safety
///
/// `vdata` came from `bufq_nscan_create`; `bp` is on no disk queue.
pub unsafe fn bufq_nscan_queue(vdata: NonNull<c_void>, bp: &'static Buf) {
    // SAFETY: the caller's contract.
    let data = unsafe { nscan_data(vdata) };

    // If the previous sorted segment was small, we will continue packing in bufs as long as
    // they're in order.
    if data.leftoverroom.get() != 0
        && let Some(next) = data.sorted.first()
        && buf_inorder(next, bp)
    {
        // SAFETY: the caller's contract.
        unsafe { bufq_simple_nscan(&data.sorted, bp) };
        data.leftoverroom.set(data.leftoverroom.get() - 1);
        return;
    }

    // SAFETY: the caller's contract; the buffer stays in its pool item while queued.
    unsafe { data.fifo.insert_tail(bp) };
}

/// `bufq_nscan_dequeue(vdata)`.
///
/// # Safety
///
/// `vdata` came from `bufq_nscan_create`.
pub unsafe fn bufq_nscan_dequeue(vdata: NonNull<c_void>) -> Option<&'static Buf> {
    // SAFETY: the caller's contract.
    let data = unsafe { nscan_data(vdata) };
    let sorted = &data.sorted;

    if sorted.first().is_none() {
        bufq_nscan_resort(data);
    }

    let bp = sorted.first()?;
    // SAFETY: `bp` is the head of the sorted queue.
    unsafe { sorted.remove_head() };
    // SAFETY: queued buffers are `&'static` pool items (`bufq_nscan_queue`).
    Some(unsafe { &*ptr::from_ref(bp) })
}

/// `bufq_nscan_peek(vdata)`.
///
/// # Safety
///
/// `vdata` came from `bufq_nscan_create`.
pub unsafe fn bufq_nscan_peek(vdata: NonNull<c_void>) -> bool {
    // SAFETY: the caller's contract.
    let data = unsafe { nscan_data(vdata) };

    data.sorted.first().is_some() || data.fifo.first().is_some()
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for the disk queues: the fifo order, the nscan sorting of segments and the
    // packing of in-order buffers into a short segment, the water marks and `bufq_drain`.

    use std::boxed::Box;
    use std::vec::Vec;
    use std::{assert, assert_eq};

    use super::*;
    use crate::sys::buf::{B_DONE, BUFQ_FIFO, BUFQ_NSCAN};
    use crate::sys::types::Daddr;

    /// A buffer for block `blkno`, leaked for the test.
    fn buf(blkno: Daddr) -> &'static Buf {
        let bp: &'static Buf = Box::leak(Box::new(Buf::new()));
        bp.b_blkno.set(blkno);
        bp
    }

    /// A queue of discipline `type_`, initialised and leaked for the test.
    fn queue(type_: i32) -> &'static Bufq {
        let bq: &'static Bufq = Box::leak(Box::new(Bufq::new()));
        bufq_init(bq, type_).expect("bufq_init");
        bq
    }

    /// Everything left on the queue, in dequeue order.
    fn drain_blocks(bq: &Bufq) -> Vec<Daddr> {
        let mut v = Vec::new();
        while let Some(bp) = bufq_dequeue(bq) {
            v.push(bp.b_blkno.get());
        }
        v
    }

    #[test]
    fn fifo_serves_in_arrival_order() {
        let _g = crate::kern::subr_pool::tests::setup_real_memory();
        let bq = queue(BUFQ_FIFO);
        assert!(!bufq_peek(bq));
        for blk in [30, 10, 20] {
            bufq_queue(bq, buf(blk));
        }
        assert!(bufq_peek(bq));
        assert_eq!(bq.bufq_outstanding.get(), 3);
        assert_eq!(drain_blocks(bq), [30, 10, 20]);
        bufq_destroy(bq);
    }

    #[test]
    fn nscan_sorts_each_segment_by_block() {
        let _g = crate::kern::subr_pool::tests::setup_real_memory();
        let bq = queue(BUFQ_NSCAN);
        for blk in [50, 10, 40, 20, 30] {
            bufq_queue(bq, buf(blk));
        }
        // The first dequeue sorts the fifo into a segment.
        let first = bufq_dequeue(bq).expect("a buffer");
        assert_eq!(first.b_blkno.get(), 10);
        // The segment was short (5 of BUFQ_NSCAN_N), so an in-order arrival joins it...
        bufq_queue(bq, buf(35));
        // ...and one before the segment's head waits for the next segment.
        bufq_queue(bq, buf(5));
        assert_eq!(drain_blocks(bq), [20, 30, 35, 40, 50, 5]);
        bufq_destroy(bq);
    }

    #[test]
    fn nscan_sorts_at_most_bufq_nscan_n_at_a_time() {
        let _g = crate::kern::subr_pool::tests::setup_real_memory();
        let bq = queue(BUFQ_NSCAN);
        let n = BUFQ_NSCAN_N as Daddr;
        // Two segments' worth, in descending order.
        for blk in (0..n + 3).rev() {
            bufq_queue(bq, buf(blk));
        }
        let got = drain_blocks(bq);
        // The first segment is the first BUFQ_NSCAN_N arrivals, sorted; then the rest, sorted.
        let mut first: Vec<Daddr> = (3..n + 3).collect();
        first.extend([0, 1, 2]);
        assert_eq!(got, first);
        bufq_destroy(bq);
    }

    #[test]
    fn water_marks_follow_the_kva_slots_and_drain_fails_the_rest() {
        let _g = crate::kern::subr_pool::tests::setup_real_memory();
        // With few kva slots the high mark shrinks to a sixteenth of them (at least 2).
        BCSTATS.kvaslots.store(64, Ordering::Relaxed);
        let bq = queue(BUFQ_FIFO);
        assert_eq!((bq.bufq_hi.get(), bq.bufq_low.get()), (4, 2));
        BCSTATS.kvaslots.store(16, Ordering::Relaxed);
        let small = queue(BUFQ_FIFO);
        assert_eq!((small.bufq_hi.get(), small.bufq_low.get()), (2, 1));
        BCSTATS.kvaslots.store(1 << 20, Ordering::Relaxed);
        let big = queue(BUFQ_FIFO);
        assert_eq!((big.bufq_hi.get(), big.bufq_low.get()), (BUFQ_HI, BUFQ_LOW));

        // biodone through bufq_done gives the slot back; drain fails what is still queued.
        let a = buf(1);
        let b = buf(2);
        bufq_queue(bq, a);
        bufq_queue(bq, b);
        let first = bufq_dequeue(bq).expect("a");
        assert!(ptr::eq(first, a));
        let s = splbio();
        biodone(a);
        splx(s);
        assert!(a.b_bq.get().is_none());
        assert_eq!(bq.bufq_outstanding.get(), 1);
        bufq_drain(bq);
        assert!(b.isset(B_ERROR) && b.isset(B_DONE));
        assert_eq!(b.b_error.get(), Some(Errno::ENXIO));
        assert_eq!(bq.bufq_outstanding.get(), 0);

        BCSTATS.kvaslots.store(0, Ordering::Relaxed);
        for q in [bq, small, big] {
            bufq_destroy(q);
        }
    }
}
/* </TESTS> */
