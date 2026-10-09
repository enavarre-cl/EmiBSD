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
 * Copyright (c) 1996, 1997 Christopher G. Demetriou.  All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. All advertising materials mentioning features or use of this software
 *    must display the following acknowledgement:
 *      This product includes software developed by Christopher G. Demetriou
 *	for the NetBSD Project.
 * 4. The name of the author may not be used to endorse or promote products
 *    derived from this software without specific prior written permission
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR ``AS IS'' AND ANY EXPRESS OR
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES
 * OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED.
 * IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY DIRECT, INDIRECT,
 * INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT
 * NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
 * DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
 * THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
 * (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF
 * THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */

/*
 * Copyright (c) 1992, 1993
 *	The Regents of the University of California.  All rights reserved.
 *
 * This software was developed by the Computer Systems Engineering group
 * at Lawrence Berkeley Laboratory under DARPA contract BG 91-66 and
 * contributed to Berkeley.
 *
 * All advertising materials mentioning features or use of this software
 * must display the following acknowledgement:
 *	This product includes software developed by the University of
 *	California, Lawrence Berkeley Laboratory.
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
 *	@(#)event.c	8.1 (Berkeley) 6/11/93
 */
/* </LICENSES> */

/* <CODE> */
//! Internal "wscons_event" queue interface for the keyboard and mouse drivers: opening and
//! closing a queue, and the user-level interface to it, `read(2)` and `kqueue(2)` (a user
//! cannot write an event queue).
//!
//! Upstream: sys/dev/wscons/wsevent.c @ 3ce1f3f79392
//!
//! The queue itself and `wsevent_wakeup` are `wseventvar.rs`'s. [`wsevent_read`] copies out
//! as many whole events as the reader asked for and the ring holds, sleeping (`PWSEVENT`,
//! interruptible) while it is empty unless the descriptor is non-blocking. The knote of
//! [`wsevent_kqfilter`] is readable while the ring is not empty, with the count of events
//! as its data.
//!
//! ## Deviations
//! - [`wsevent_init`]'s 1 (another thread initialised the queue while the allocation slept) is
//!   `Err(EBUSY)`, the error its callers turn it into. An `M_WAITOK` allocation does not fail;
//!   if it did, the result would be `ENOMEM`.
//! - The `DIAGNOSTIC` message of a second [`wsevent_fini`] is behind the `diagnostic`
//!   feature.

use core::ptr;

use crate::dev::wscons::wsconsio::WsconsEvent;
use crate::dev::wscons::wseventvar::{PWSEVENT, WSEVENT_QSIZE, Wseventvar};
use crate::kern::kern_event::{klist_init_mutex, klist_insert, klist_invalidate, klist_remove};
use crate::kern::kern_lock::{mtx_enter, mtx_init_flags, mtx_leave};
use crate::kern::kern_malloc::{free, mallocarray};
use crate::kern::kern_sig::sigio_free;
use crate::kern::kern_subr::uiomove;
use crate::kern::kern_synch::msleep_nsec;
use crate::machine::intr::IPL_TTY;
use crate::sys::errno::Errno;
use crate::sys::event::{
    EVFILT_READ, FILTEROP_ISFD, FILTEROP_MPSAFE, Filterops, Kevent, Knote, knote_modify,
    knote_process,
};
use crate::sys::malloc::{M_DEVBUF, M_WAITOK, M_ZERO};
use crate::sys::mutex::mutex_assert_locked;
use crate::sys::param::{PCATCH, howmany};
use crate::sys::sigio::sigio_init;
use crate::sys::systm::INFSLP;
use crate::sys::uio::Uio;
use crate::sys::vnode::IO_NDELAY;

/// `wsevent_filtops`: `EVFILT_READ` on an event queue.
pub static WSEVENT_FILTOPS: Filterops = Filterops {
    f_flags: FILTEROP_ISFD | FILTEROP_MPSAFE,
    f_attach: None,
    f_detach: Some(filt_wseventdetach),
    f_event: Some(filt_wseventread),
    f_modify: Some(filt_wseventmodify),
    f_process: Some(filt_wseventprocess),
};

/// The bytes of the ring.
const QBYTES: usize = WSEVENT_QSIZE as usize * size_of::<WsconsEvent>();

/// `wsevent_init`: initialize a wscons_event queue. `Err(EBUSY)` where the C returns 1: the
/// queue was opened by someone else while we slept.
pub fn wsevent_init(ev: &Wseventvar) -> Result<(), Errno> {
    if !ev.ws_q.get().is_null() {
        return Ok(());
    }

    let Some(queue) = mallocarray(
        WSEVENT_QSIZE as usize,
        size_of::<WsconsEvent>(),
        M_DEVBUF,
        M_WAITOK | M_ZERO,
    ) else {
        return Err(Errno::ENOMEM);
    };
    if !ev.ws_q.get().is_null() {
        free(queue, M_DEVBUF, QBYTES);
        return Err(Errno::EBUSY);
    }

    mtx_init_flags(&ev.ws_mtx, IPL_TTY, Some("wsmtx"), 0);
    // SAFETY: `ws_mtx` is a member of the same queue, which outlives its klist.
    unsafe { klist_init_mutex(&ev.ws_klist, &ev.ws_mtx) };

    ev.ws_q.set(queue.as_ptr().cast());
    ev.ws_get.set(0);
    ev.ws_put.set(0);

    sigio_init(&ev.ws_sigio);

    Ok(())
}

/// `wsevent_fini`: tear down a wscons_event queue.
pub fn wsevent_fini(ev: &Wseventvar) {
    let Some(q) = ptr::NonNull::new(ev.ws_q.get()) else {
        #[cfg(feature = "diagnostic")]
        crate::kprintf!("wsevent_fini: already invoked\n");
        return;
    };
    free(q.cast(), M_DEVBUF, QBYTES);
    ev.ws_q.set(ptr::null_mut());

    klist_invalidate(&ev.ws_klist);

    sigio_free(&ev.ws_sigio);
}

/// `wsevent_read`: user-level interface: read whole events (at least one: `EMSGSIZE` for a
/// shorter buffer), sleeping while there are none unless `flags` has `IO_NDELAY`
/// (`EWOULDBLOCK` then).
pub fn wsevent_read(ev: &Wseventvar, uio: &mut Uio<'_>, flags: i32) -> Result<(), Errno> {
    let evsize = size_of::<WsconsEvent>();

    // Make sure we can return at least 1.
    if uio.uio_resid < evsize {
        return Err(Errno::EMSGSIZE); // ???
    }
    let mut n = howmany(uio.uio_resid, evsize);

    mtx_enter(&ev.ws_mtx);

    while ev.ws_get.get() == ev.ws_put.get() {
        if flags & IO_NDELAY != 0 {
            mtx_leave(&ev.ws_mtx);
            return Err(Errno::EWOULDBLOCK);
        }
        ev.ws_wanted.set(1);
        if let Err(error) = msleep_nsec(
            ptr::from_ref(ev),
            &ev.ws_mtx,
            PWSEVENT | PCATCH,
            "wsevent_read",
            INFSLP,
        ) {
            mtx_leave(&ev.ws_mtx);
            return Err(error);
        }
    }

    // Move wscons_event from tail end of queue (there is at least one there).
    let mut cnt = if ev.ws_put.get() < ev.ws_get.get() {
        WSEVENT_QSIZE - ev.ws_get.get() // events in [get..QSIZE)
    } else {
        ev.ws_put.get() - ev.ws_get.get() // events in [get..put)
    } as usize;

    if cnt > n {
        cnt = n;
    }

    let get = ev.ws_get.get();
    let mut tcnt = ev.ws_put.get() as usize;
    n -= cnt;

    ev.ws_get.set((get + cnt as u32) % WSEVENT_QSIZE);
    let wrap = !(ev.ws_get.get() != 0 || n == 0 || tcnt == 0);
    if wrap {
        if tcnt > n {
            tcnt = n;
        }
        ev.ws_get.set(tcnt as u32);
    }

    mtx_leave(&ev.ws_mtx);

    // SAFETY: the events in [get, get + cnt) are below `ws_put` (or wrap past the end), so no
    // writer reaches them; the ring stays allocated while its device is open for this read.
    let mut error = uiomove(unsafe { ev.q_events(get, cnt as u32) }, uio);

    // If we do wrap to 0, move from front of queue to put index, if there is anything there
    // to move.
    if wrap && error.is_ok() {
        // SAFETY: as above, for [0, tcnt), with `tcnt` at most `ws_put`.
        error = uiomove(unsafe { ev.q_events(0, tcnt as u32) }, uio);
    }

    error
}

/// `wsevent_kqfilter`: attach a knote to the queue (`EVFILT_READ` only).
pub fn wsevent_kqfilter(ev: &Wseventvar, kn: &Knote) -> Result<(), Errno> {
    match kn.kn_filter().get() {
        EVFILT_READ => kn.kn_fop.set(Some(&WSEVENT_FILTOPS)),
        _ => return Err(Errno::EINVAL),
    }

    kn.kn_hook.set(ptr::from_ref(ev).cast_mut().cast());
    klist_insert(&ev.ws_klist, kn);

    Ok(())
}

/// The queue a knote hangs on (`kn->kn_hook`).
fn kn_wsevent(kn: &Knote) -> &Wseventvar {
    // SAFETY: `wsevent_kqfilter` set `kn_hook` to the queue, and `wsevent_fini` invalidates
    // the klist (detaching every knote) before the queue goes.
    unsafe { &*kn.kn_hook.get().cast::<Wseventvar>() }
}

/// `filt_wseventdetach`.
pub fn filt_wseventdetach(kn: &Knote) {
    let ev = kn_wsevent(kn);

    klist_remove(&ev.ws_klist, kn);
}

/// `filt_wseventread`: readable when the ring holds events; their number is the data.
/// Called with `ws_mtx` held.
pub fn filt_wseventread(kn: &Knote, _hint: i64) -> bool {
    let ev = kn_wsevent(kn);

    mutex_assert_locked(&ev.ws_mtx, "filt_wseventread");

    let (get, put) = (ev.ws_get.get(), ev.ws_put.get());
    if get == put {
        return false;
    }

    if get < put {
        kn.kn_data().set(i64::from(put - get));
    } else {
        kn.kn_data().set(i64::from((WSEVENT_QSIZE - get) + put));
    }

    true
}

/// `filt_wseventmodify`.
pub fn filt_wseventmodify(kev: &mut Kevent, kn: &Knote) -> bool {
    let ev = kn_wsevent(kn);

    mtx_enter(&ev.ws_mtx);
    let active = knote_modify(kev, kn);
    mtx_leave(&ev.ws_mtx);

    active
}

/// `filt_wseventprocess`.
pub fn filt_wseventprocess(kn: &Knote, kev: Option<&mut Kevent>) -> bool {
    let ev = kn_wsevent(kn);

    mtx_enter(&ev.ws_mtx);
    let active = knote_process(kn, kev);
    mtx_leave(&ev.ws_mtx);

    active
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests of the event queue: opening, reading whole events (with the ring's wrap),
    // the non-blocking and short reads, the knote's count, closing.

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
}
/* </TESTS> */
