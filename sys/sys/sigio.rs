/*	$OpenBSD: sigio.h,v 1.4 2020/01/08 16:27:42 visa Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (c) 1990, 1993
 *	The Regents of the University of California.  All rights reserved.
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
 *	@(#)filedesc.h	8.1 (Berkeley) 6/2/93
 * $FreeBSD: head/sys/sys/sigio.h 326023 2017-11-20 19:43:44Z pfg $
 */
/* </LICENSES> */

/* <CODE> */
//! `<sys/sigio.h>`: the registration of a process or process group to receive `SIGIO`/
//! `SIGURG` for a file.
//!
//! Upstream: sys/sys/sigio.h @ 3ce1f3f79392
//!
//! Status: `ported` (with `kern_sig.c`, which implements the functions it declares).
//!
//! ## Deviations
//! - The union `sio_u` (`sio_proc` or `sio_pgrp`, chosen by the sign of `sio_pgid`) is two
//!   side-by-side pointer fields; the one the sign does not select stays null.

use core::cell::Cell;
use core::ptr;

use crate::queue_adapter;
use crate::sys::proc::{Pgrp, Process};
use crate::sys::queue::{ListEntry, ListHead};
use crate::sys::types::Pid;
use crate::sys::ucred::Ucred;

/// `struct sigio_ref`: sigio registration.
///
/// Locking: S: `sigio_lock`.
pub struct SigioRef {
    /// \[S\] `sir_sigio`: associated sigio struct.
    pub sir_sigio: Cell<*const Sigio>,
}

// SAFETY: `sir_sigio` is written under `sigio_lock`.
unsafe impl Sync for SigioRef {}

impl SigioRef {
    /// `sigio_init`: an empty registration.
    pub const fn new() -> Self {
        Self {
            sir_sigio: Cell::new(ptr::null()),
        }
    }
}

impl Default for SigioRef {
    fn default() -> Self {
        Self::new()
    }
}

/// `struct sigio`: this structure holds the information needed to send a SIGIO or a SIGURG
/// signal to a process or process group when new data arrives on a device or socket. The
/// structure is placed on an LIST belonging to the proc or pgrp so that the entire list may
/// be revoked when the process exits or the process group disappears.
///
/// Locking: I: immutable after creation; S: `sigio_lock`.
pub struct Sigio {
    /// \[I\] `sio_proc` (`sio_u.siu_proc`): process to receive SIGIO/SIGURG.
    pub sio_proc: Cell<*const Process>,
    /// \[I\] `sio_pgrp` (`sio_u.siu_pgrp`): process group to receive ...
    pub sio_pgrp: Cell<*const Pgrp>,
    /// \[S\] `sio_pgsigio`: sigio's for process or group.
    pub sio_pgsigio: ListEntry<Sigio>,
    /// \[I\] `sio_myref`: location of the pointer that holds the reference to this structure.
    pub sio_myref: Cell<*const SigioRef>,
    /// \[I\] `sio_ucred`: current credentials.
    pub sio_ucred: Cell<*const Ucred>,
    /// \[I\] `sio_pgid`: pgid for signals.
    pub sio_pgid: Cell<Pid>,
}

// SAFETY: the links are touched under `sigio_lock`, the rest is immutable after creation.
unsafe impl Sync for Sigio {}

impl Sigio {
    /// An unlinked, empty `struct sigio`.
    pub const fn new() -> Self {
        Self {
            sio_proc: Cell::new(ptr::null()),
            sio_pgrp: Cell::new(ptr::null()),
            sio_pgsigio: ListEntry::new(),
            sio_myref: Cell::new(ptr::null()),
            sio_ucred: Cell::new(ptr::null()),
            sio_pgid: Cell::new(0),
        }
    }
}

impl Default for Sigio {
    fn default() -> Self {
        Self::new()
    }
}

queue_adapter!(
    /// `LIST_ENTRY(sigio) sio_pgsigio`: a process's or process group's sigios.
    pub SigioPgsigio: Sigio, sio_pgsigio => ListEntry<Sigio>
);

/// `LIST_HEAD(sigiolst, sigio)`.
pub type Sigiolst = ListHead<SigioPgsigio>;

/// `sigio_init(sir)`.
pub fn sigio_init(sir: &SigioRef) {
    sir.sir_sigio.set(ptr::null());
}
/* </CODE> */
