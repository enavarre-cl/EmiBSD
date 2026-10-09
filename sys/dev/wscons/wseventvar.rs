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
 *	@(#)event_var.h	8.1 (Berkeley) 6/11/93
 */
/* </LICENSES> */

/* <CODE> */
//! `<dev/wscons/wseventvar.h>`: the internal `wscons_event` queue interface for the keyboard
//! and mouse drivers.
//!
//! Upstream: sys/dev/wscons/wseventvar.h @ 3ce1f3f79392
//!
//! A [`Wseventvar`] is the ring of [`WsconsEvent`]s a reader of `/dev/wskbd*` or `/dev/wsmux*`
//! gets: the drivers put events at `ws_put` (from interrupt context, under `ws_mtx` at
//! `IPL_TTY`) and [`wsevent_wakeup`] the reader, who takes them from `ws_get` with
//! `wsevent_read` (`wsevent.rs`, with the other functions this header declares). The drivers
//! are expected not to place events in the queue above `spltty()`, i.e., are expected to run
//! off serial ports or similar devices.
//!
//! ## Deviations
//! - The members the C marks `[m]` are `Cell`s, touched only with `ws_mtx` held (the C's
//!   `volatile u_int ws_put` is one too: every access is under the mutex). `ws_q`, the
//!   `malloc`ed ring, is a raw pointer in a `Cell`, null while the queue is closed;
//!   [`Wseventvar::q_write`] and [`Wseventvar::q_events`] reach its slots.
//! - The structure is valid all-zero (it is a member of `M_ZERO` softcs), as in C;
//!   [`Wseventvar::new`] makes the same zeroes for a standalone one.
//! - The prototypes (`wsevent_init`, `wsevent_fini`, `wsevent_read`, `wsevent_kqfilter`)
//!   belong to `wsevent.c`; they are in `wsevent.rs`.

use core::cell::Cell;
use core::ptr;

use crate::dev::wscons::wsconsio::WsconsEvent;
use crate::kern::kern_event::knote;
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_sig::pgsigio;
use crate::kern::kern_synch::wakeup;
use crate::machine::intr::IPL_NONE;
use crate::sys::event::Klist;
use crate::sys::mutex::Mutex;
use crate::sys::sigio::SigioRef;
use crate::sys::signal::SIGIO;

/// `WSEVENT_QSIZE`: the ring's size in events; should be a power of two so that `%` is fast
/// (may need tuning).
pub const WSEVENT_QSIZE: u32 = 256;

/// `struct wseventvar`: an event queue.
///
/// Locks used to protect data: I immutable, m `ws_mtx`.
pub struct Wseventvar {
    /// \[m\] `ws_get`: get (read) index (modified synchronously).
    pub ws_get: Cell<u32>,
    /// \[m\] `ws_put`: put (write) index (modified by interrupt).
    pub ws_put: Cell<u32>,
    /// `ws_mtx`.
    pub ws_mtx: Mutex,
    /// \[m\] `ws_klist`: list of knotes.
    pub ws_klist: Klist,
    /// `ws_sigio`: async I/O registration.
    pub ws_sigio: SigioRef,
    /// \[m\] `ws_wanted`: wake up on input ready.
    pub ws_wanted: Cell<i32>,
    /// \[m\] `ws_async`: send SIGIO on input ready.
    pub ws_async: Cell<i32>,
    /// \[m\] `ws_q`: circular buffer (queue) of events, `WSEVENT_QSIZE` long; null while
    /// closed.
    pub ws_q: Cell<*mut WsconsEvent>,
}

impl Wseventvar {
    /// A closed queue, as the zeroed member of a softc is.
    pub const fn new() -> Self {
        Self {
            ws_get: Cell::new(0),
            ws_put: Cell::new(0),
            ws_mtx: Mutex::new(IPL_NONE),
            ws_klist: Klist::new(),
            ws_sigio: SigioRef::new(),
            ws_wanted: Cell::new(0),
            ws_async: Cell::new(0),
            ws_q: Cell::new(ptr::null_mut()),
        }
    }

    /// `ev->ws_q[idx] = e`.
    ///
    /// # Safety
    ///
    /// The queue is open (`ws_q` is the ring `wsevent_init` allocated), `idx <
    /// WSEVENT_QSIZE`, and `ws_mtx` is held.
    pub unsafe fn q_write(&self, idx: u32, e: WsconsEvent) {
        debug_assert!(idx < WSEVENT_QSIZE);
        // SAFETY: the caller's contract: an open ring of `WSEVENT_QSIZE` events, an index
        // inside it, and the mutex that serialises the slots.
        unsafe { self.ws_q.get().add(idx as usize).write(e) };
    }

    /// `ev->ws_q[idx]`.
    ///
    /// # Safety
    ///
    /// The queue is open and `idx < WSEVENT_QSIZE`; no writer touches that slot meanwhile
    /// (the reader's own slots, or slots a writer filled and has not published yet).
    pub unsafe fn q_read(&self, idx: u32) -> WsconsEvent {
        debug_assert!(idx < WSEVENT_QSIZE);
        // SAFETY: the caller's contract: an open ring of `WSEVENT_QSIZE` initialised
        // (`M_ZERO`ed, plain data) events and a slot nobody writes now.
        unsafe { self.ws_q.get().add(idx as usize).read() }
    }

    /// The bytes of the `n` events of the ring from `idx` (`(caddr_t)&ev->ws_q[idx]` and the
    /// length `uiomove` gets).
    ///
    /// # Safety
    ///
    /// The queue is open and `idx + n <= WSEVENT_QSIZE`; no writer touches those slots while
    /// the slice lives: they lie between `ws_get` and `ws_put`, which a writer does not
    /// reach (a full ring drops the event instead).
    #[allow(clippy::mut_from_ref)] // the ring is shared memory the protocol above partitions
    pub unsafe fn q_events(&self, idx: u32, n: u32) -> &mut [u8] {
        debug_assert!(idx + n <= WSEVENT_QSIZE);
        // SAFETY: the caller's contract: `n` initialised events (the ring is `M_ZERO`ed and
        // `WsconsEvent` is plain data) that nobody else writes meanwhile.
        unsafe {
            core::slice::from_raw_parts_mut(
                self.ws_q.get().add(idx as usize).cast::<u8>(),
                n as usize * size_of::<WsconsEvent>(),
            )
        }
    }
}

impl Default for Wseventvar {
    fn default() -> Self {
        Self::new()
    }
}

/// `PWSEVENT`: set just above `PSOCK`, which is just above `TTIPRI`, on the theory that mouse
/// and keyboard `user' input should be quick.
pub const PWSEVENT: i32 = 23;

/// `wsevent_wakeup`: tell the knotes and a sleeping reader that events are in the queue, and
/// send `SIGIO` if asked to.
pub fn wsevent_wakeup(ev: &Wseventvar) {
    let mut dosigio = false;

    knote(&ev.ws_klist, 0);

    mtx_enter(&ev.ws_mtx);
    if ev.ws_wanted.get() != 0 {
        ev.ws_wanted.set(0);
        wakeup(ptr::from_ref(ev));
    }
    if ev.ws_async.get() != 0 {
        dosigio = true;
    }
    mtx_leave(&ev.ws_mtx);

    if dosigio {
        pgsigio(&ev.ws_sigio, SIGIO, false);
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;
    use crate::reftest::assert_defines;

    #[test]
    fn a_new_queue_is_closed() {
        assert!(WSEVENT_QSIZE.is_power_of_two());
        let ev = Wseventvar::new();
        assert!(ev.ws_q.get().is_null());
        assert_eq!((ev.ws_get.get(), ev.ws_put.get()), (0, 0));
    }

    /// The constants against `<dev/wscons/wseventvar.h>`.
    #[test]
    #[ignore = "needs OPENBSD_SRC"]
    fn constants_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/wscons/wseventvar.h");
        let _ = assert_defines!(defs; WSEVENT_QSIZE, PWSEVENT);
    }
}
/* </TESTS> */
