//! Host tests for the socket filters: `kevent` registrations on a `socketpair(2)`
//! (`soo_kqfilter` hooks the knotes on the buffers' lists, `sowakeup`'s `knote_locked`
//! activates them, the peer's close gives `EV_EOF`, a filter sockets do not have is
//! `EINVAL`), and `filt_soread`/`filt_sowrite` called directly (`NOTE_LOWAT`, poll's
//! `__EV_HUP` once the peer is gone).
//!
//! They run on the UNIX domain tests' setup (`uipc_usrreq/tests.rs`), with the kqueue pools
//! of `kqueue_init` and the kqueues `kern_event`'s tests make.

use std::{assert, assert_eq};

use super::*;
use crate::kern::kern_event::kqueue_register;
use crate::kern::kern_event::tests::{close_kqueue, new_kqueue, scan};
use crate::kern::uipc_usrreq::tests::{close, recv, send, setup, socketpair, teardown};
use crate::sys::event::{EV_ADD, EV_ONESHOT, EVFILT_VNODE, ev_set, klist_empty};
use crate::sys::proc::Proc;

/// The socket of descriptor `fd` of `p`, with its file.
fn sock(p: &Proc, fd: i32) -> (&'static File, &'static Socket) {
    let fp = p.fd().ofile(fd as usize).expect("an open descriptor");
    (fp, fp_socket(fp))
}

/// A knote on `fp` for calling the filters directly: `filter`, `flags` and `sfflags` set.
fn knote_on(fp: &'static File, filter: i16, flags: u16, sfflags: u32) -> Knote {
    let kn = Knote::new();
    kn.kn_fp().set(Some(fp));
    kn.kn_filter().set(filter);
    kn.kn_flags().set(flags);
    kn.kn_sfflags.set(sfflags);
    kn
}

#[test]
fn kevent_on_a_socket_pair() {
    let (_g, p) = setup();
    crate::kern::kern_event::kqueue_init();
    let [a, b] = socketpair(p);
    let (_, so_a) = sock(p, a);
    let (_, so_b) = sock(p, b);
    let kq = new_kqueue(p);
    let mut out = [Kevent::default(); 4];
    let mut buf = [0u8; 8];

    // EVFILT_READ on a: hooked on a's receive buffer, nothing pending.
    let mut kev = ev_set(a as usize, EVFILT_READ, EV_ADD, 0, 0, 7);
    assert_eq!(kqueue_register(kq, &mut kev, 0, Some(p)), Ok(()));
    assert!(!klist_empty(&so_a.so_rcv.sb_klist));
    assert!(klist_empty(&so_a.so_snd.sb_klist));
    assert_eq!(scan(p, kq, &mut out), (0, Ok(())));

    // b sends: sowakeup's knote_locked activates the knote, with the byte count.
    assert_eq!(send(so_b, b"hello", 0), Ok(5));
    assert_eq!(kq.kq_count.get(), 1);
    assert_eq!(scan(p, kq, &mut out), (1, Ok(())));
    assert_eq!((out[0].ident, out[0].filter), (a as usize, EVFILT_READ));
    assert_eq!(
        (out[0].data, out[0].udata, out[0].flags & EV_EOF),
        (5, 7, 0)
    );

    // Drained: the next scan finds the knote inactive.
    assert_eq!(recv(so_a, &mut buf, 0), Ok((5, 0)));
    assert_eq!(scan(p, kq, &mut out), (0, Ok(())));

    // A one-shot EVFILT_WRITE on b: room to send, then gone from b's send buffer.
    let mut kev = ev_set(b as usize, EVFILT_WRITE, EV_ADD | EV_ONESHOT, 0, 0, 8);
    assert_eq!(kqueue_register(kq, &mut kev, 0, Some(p)), Ok(()));
    assert!(!klist_empty(&so_b.so_snd.sb_klist));
    assert_eq!(scan(p, kq, &mut out), (1, Ok(())));
    assert_eq!((out[0].ident, out[0].filter), (b as usize, EVFILT_WRITE));
    assert!(out[0].data > 0);
    assert!(klist_empty(&so_b.so_snd.sb_klist));

    // EVFILT_EXCEPT for out-of-band data: hooked on a's receive buffer, quiet.
    let mut kev = ev_set(a as usize, EVFILT_EXCEPT, EV_ADD, NOTE_OOB, 0, 0);
    assert_eq!(kqueue_register(kq, &mut kev, 0, Some(p)), Ok(()));
    assert_eq!(scan(p, kq, &mut out), (0, Ok(())));

    // Sockets have no vnode filter.
    let mut kev = ev_set(a as usize, EVFILT_VNODE, EV_ADD, 0, 0, 0);
    assert_eq!(
        kqueue_register(kq, &mut kev, 0, Some(p)),
        Err(Errno::EINVAL)
    );

    // b goes away: a's read knote reports EOF; the except knote stays quiet.
    close(p, b);
    assert_eq!(scan(p, kq, &mut out), (1, Ok(())));
    assert_eq!((out[0].ident, out[0].filter), (a as usize, EVFILT_READ));
    assert!(out[0].flags & EV_EOF != 0);

    // Closing the kqueue detaches its knotes from a's lists (sorele's klist_free checks).
    close_kqueue(p, kq);
    assert!(klist_empty(&so_a.so_rcv.sb_klist));
    close(p, a);
    teardown();
}

#[test]
fn read_and_write_filters() {
    let (_g, p) = setup();
    let [a, b] = socketpair(p);
    let (fa, so_a) = sock(p, a);
    let (fb, so_b) = sock(p, b);

    // NOTE_LOWAT: readable from the user's mark on, not the buffer's.
    let rd = knote_on(fa, EVFILT_READ, 0, NOTE_LOWAT);
    rd.kn_sdata.set(3);
    assert_eq!(send(so_b, b"ab", 0), Ok(2));
    mtx_enter(&so_a.so_rcv.sb_mtx);
    assert!(!filt_soread(&rd, 0));
    assert_eq!(rd.kn_data().get(), 2);
    mtx_leave(&so_a.so_rcv.sb_mtx);
    assert_eq!(send(so_b, b"c", 0), Ok(1));
    mtx_enter(&so_a.so_rcv.sb_mtx);
    assert!(filt_soread(&rd, 0));
    assert_eq!(rd.kn_data().get(), 3);
    mtx_leave(&so_a.so_rcv.sb_mtx);

    // The writer has room on a connected stream.
    let wr = knote_on(fb, EVFILT_WRITE, 0, 0);
    mtx_enter(&so_b.so_snd.sb_mtx);
    assert!(filt_sowrite(&wr, 0));
    assert!(wr.kn_data().get() > 0 && !wr.has_flags(EV_EOF));
    mtx_leave(&so_b.so_snd.sb_mtx);

    // b goes away: for poll, a reads EOF and a hang-up, and so does its except filter.
    close(p, b);
    let rd = knote_on(fa, EVFILT_READ, __EV_POLL, 0);
    let ex = knote_on(fa, EVFILT_EXCEPT, __EV_POLL, 0);
    mtx_enter(&so_a.so_rcv.sb_mtx);
    assert!(filt_soread(&rd, 0));
    assert!(rd.has_flags(EV_EOF) && rd.has_flags(__EV_HUP));
    assert!(filt_soexcept(&ex, 0) && ex.has_flags(__EV_HUP));
    mtx_leave(&so_a.so_rcv.sb_mtx);
    // Through kevent(2) (no __EV_POLL): EOF without the hang-up; no except event.
    let rd = knote_on(fa, EVFILT_READ, 0, 0);
    let ex = knote_on(fa, EVFILT_EXCEPT, 0, 0);
    mtx_enter(&so_a.so_rcv.sb_mtx);
    assert!(filt_soread(&rd, 0));
    assert!(rd.has_flags(EV_EOF) && !rd.has_flags(__EV_HUP));
    assert!(!filt_soexcept(&ex, 0));
    mtx_leave(&so_a.so_rcv.sb_mtx);

    close(p, a);
    teardown();
}
