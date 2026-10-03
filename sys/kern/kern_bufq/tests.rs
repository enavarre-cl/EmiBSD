//! Host tests for the disk queues: the fifo order, the nscan sorting of segments and the
//! packing of in-order buffers into a short segment, the water marks and `bufq_drain`.

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
