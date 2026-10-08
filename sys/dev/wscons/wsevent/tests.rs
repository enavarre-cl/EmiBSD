//! Host tests of the event queue: opening, reading whole events (with the ring's wrap),
//! the non-blocking and short reads, the knote's count, closing.

use std::vec;
use std::vec::Vec;

use super::*;
use crate::kern::subr_pool::tests::setup_real_memory;
use crate::sys::time::Timespec;
use crate::sys::uio::{Iovec, UioRw, UioSeg};

/// Puts an event at `ws_put`, as a driver does.
fn put(ev: &Wseventvar, value: i32) {
    mtx_enter(&ev.ws_mtx);
    let at = ev.ws_put.get();
    let e = WsconsEvent {
        type_: 2,
        value,
        time: Timespec::default(),
    };
    // SAFETY: the queue is open, `at` is inside it, `ws_mtx` is held.
    unsafe { ev.q_write(at, e) };
    ev.ws_put.set((at + 1) % WSEVENT_QSIZE);
    mtx_leave(&ev.ws_mtx);
}

/// Reads into a buffer of `len` bytes; the values of the whole events read.
fn read(ev: &Wseventvar, len: usize, flags: i32) -> Result<Vec<i32>, Errno> {
    let mut buf = vec![0u8; len];
    let mut iov = [Iovec {
        iov_base: buf.as_mut_ptr().cast(),
        iov_len: len,
    }];
    let mut uio = Uio {
        uio_iov: &mut iov,
        uio_offset: 0,
        uio_resid: len,
        uio_segflg: UioSeg::UIO_SYSSPACE,
        uio_rw: UioRw::UIO_READ,
        uio_procp: None,
    };
    wsevent_read(ev, &mut uio, flags)?;
    let done = len - uio.uio_resid;
    assert_eq!(done % size_of::<WsconsEvent>(), 0, "whole events only");
    Ok(buf[..done]
        .chunks(size_of::<WsconsEvent>())
        .map(|c| i32::from_ne_bytes([c[4], c[5], c[6], c[7]]))
        .collect())
}

const EV: usize = size_of::<WsconsEvent>();

#[test]
fn read_takes_whole_events_in_order() {
    let _g = setup_real_memory();
    let ev = Wseventvar::new();
    wsevent_init(&ev).unwrap();
    assert!(!ev.ws_q.get().is_null());
    let q = ev.ws_q.get();
    wsevent_init(&ev).unwrap();
    assert_eq!(ev.ws_q.get(), q, "a second init keeps the queue");

    assert_eq!(read(&ev, EV, IO_NDELAY), Err(Errno::EWOULDBLOCK));
    assert_eq!(read(&ev, EV - 1, IO_NDELAY), Err(Errno::EMSGSIZE));

    for v in 1..=3 {
        put(&ev, v);
    }
    assert_eq!(read(&ev, EV, IO_NDELAY).unwrap(), [1]);
    assert_eq!(read(&ev, 4 * EV, IO_NDELAY).unwrap(), [2, 3]);
    assert_eq!(read(&ev, EV, IO_NDELAY), Err(Errno::EWOULDBLOCK));

    wsevent_fini(&ev);
    assert!(ev.ws_q.get().is_null());
    wsevent_fini(&ev); // already closed: nothing happens
}

#[test]
fn read_wraps_around_the_end_of_the_ring() {
    let _g = setup_real_memory();
    let ev = Wseventvar::new();
    wsevent_init(&ev).unwrap();
    ev.ws_get.set(WSEVENT_QSIZE - 2);
    ev.ws_put.set(WSEVENT_QSIZE - 2);
    for v in 10..14 {
        put(&ev, v);
    }
    assert_eq!(ev.ws_put.get(), 2);
    assert_eq!(read(&ev, 8 * EV, IO_NDELAY).unwrap(), [10, 11, 12, 13]);
    assert_eq!((ev.ws_get.get(), ev.ws_put.get()), (2, 2));

    // A read that stops at the end of the ring leaves the rest.
    ev.ws_get.set(WSEVENT_QSIZE - 1);
    ev.ws_put.set(WSEVENT_QSIZE - 1);
    put(&ev, 20);
    put(&ev, 21);
    assert_eq!(read(&ev, EV, IO_NDELAY).unwrap(), [20]);
    assert_eq!(ev.ws_get.get(), 0);
    assert_eq!(read(&ev, EV, IO_NDELAY).unwrap(), [21]);
    wsevent_fini(&ev);
}

#[test]
fn knote_counts_the_queued_events() {
    let _g = setup_real_memory();
    let ev = Wseventvar::new();
    wsevent_init(&ev).unwrap();
    let kn = Knote::new();
    kn.kn_hook.set(ptr::from_ref(&ev).cast_mut().cast());

    assert!(!filt_wseventread(&kn, 0));
    put(&ev, 1);
    put(&ev, 2);
    assert!(filt_wseventread(&kn, 0));
    assert_eq!(kn.kn_data().get(), 2);

    ev.ws_get.set(WSEVENT_QSIZE - 1);
    ev.ws_put.set(1);
    assert!(filt_wseventread(&kn, 0));
    assert_eq!(kn.kn_data().get(), 2, "across the wrap");

    kn.kn_filter().set(crate::sys::event::EVFILT_WRITE);
    assert_eq!(wsevent_kqfilter(&ev, &kn), Err(Errno::EINVAL));
    wsevent_fini(&ev);
}
