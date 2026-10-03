//! Host tests for the `select(2)`/`poll(2)` conversions of `sys_generic.c`: the kevents a
//! pollfd turns into and what a failed registration leaves in `revents`, the events turned
//! back into `revents` and fd set bits, and `pollout` copying only `revents` out.

use std::boxed::Box;
use std::vec::Vec;
use std::{assert, assert_eq, vec};

use super::*;

/// A thread for the conversions: they only read its name and id.
fn test_proc() -> &'static Proc {
    Box::leak(Box::new(Proc::new()))
}

fn pfd(fd: i32, events: i16) -> Pollfd {
    Pollfd {
        fd,
        events,
        revents: 0,
    }
}

#[test]
fn ppollregister_reports_failed_registrations() {
    let p = test_proc();
    // kqueue_register is not ported: every registration fails (ENOSYS), which poll(2)
    // reports as POLLERR; a negative fd is skipped.
    let mut pl = vec![pfd(0, POLLIN), pfd(-1, POLLIN), pfd(1, 0)];
    let (mut nregistered, mut ncollected) = (0, 0);
    ppollregister(p, &mut pl, &mut nregistered, &mut ncollected);
    assert_eq!(nregistered, 0);
    assert_eq!(ncollected, 2);
    assert_eq!(pl[0].revents, POLLERR);
    assert_eq!(pl[1].revents, 0);
    assert_eq!(pl[2].revents, POLLERR, "POLLHUP is always checked");
}

#[test]
fn ppollcollect_converts_events() {
    let p = test_proc();
    let mut pl = vec![pfd(3, POLLIN | POLLOUT), pfd(4, POLLPRI)];
    let read = ev_set(3, EVFILT_READ, 0, 0, 0, 0);
    assert_eq!(ppollcollect(p, &read, &mut pl), 1);
    assert_eq!(pl[0].revents, POLLIN);
    // a second event on the same pollfd is not counted again
    let write = ev_set(3, EVFILT_WRITE, 0, 0, 0, 0);
    assert_eq!(ppollcollect(p, &write, &mut pl), 0);
    assert_eq!(pl[0].revents, POLLIN | POLLOUT);
    // hang-up on the except filter
    let hup = ev_set(4, EVFILT_EXCEPT, __EV_HUP, 0, 0, 1);
    assert_eq!(ppollcollect(p, &hup, &mut pl), 1);
    assert_eq!(pl[1].revents, POLLHUP);
    // EBADF preempts everything
    let bad = ev_set(4, EVFILT_EXCEPT, EV_ERROR, 0, Errno::EBADF as i64, 1);
    assert_eq!(ppollcollect(p, &bad, &mut pl), 0);
    assert_eq!(pl[1].revents, POLLNVAL);
}

#[test]
fn pselcollect_sets_bits() {
    let p = test_proc();
    let words = howmany(40, NFDBITS);
    let mut pobits = vec![0 as FdMask; 3 * words];
    let mut n = 0;
    assert_eq!(
        pselcollect(
            p,
            &ev_set(33, EVFILT_WRITE, 0, 0, 0, 0),
            &mut pobits,
            words,
            &mut n
        ),
        Ok(())
    );
    assert_eq!(n, 1);
    assert_eq!(pobits[words + 1], 1 << 1);
    let err = ev_set(2, EVFILT_READ, EV_ERROR, 0, Errno::EBADF as i64, 0);
    assert_eq!(
        pselcollect(p, &err, &mut pobits, words, &mut n),
        Err(Errno::EBADF)
    );
}

#[test]
fn pollout_copies_only_revents() {
    let mut user: Vec<Pollfd> = vec![pfd(7, POLLIN), pfd(8, POLLOUT)];
    let mut kernel = user.clone();
    kernel[0].revents = POLLIN;
    kernel[1].revents = POLLHUP;
    kernel[1].fd = 99; // not copied
    assert_eq!(pollout(&kernel, user.as_mut_ptr() as usize), Ok(()));
    assert_eq!(
        user[0],
        Pollfd {
            fd: 7,
            events: POLLIN,
            revents: POLLIN
        }
    );
    assert_eq!(
        user[1],
        Pollfd {
            fd: 8,
            events: POLLOUT,
            revents: POLLHUP
        }
    );
    assert!(size_of::<Pollfd>() == 8);
}
