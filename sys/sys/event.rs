/*	$OpenBSD: event.h,v 1.74 2025/05/10 09:44:39 visa Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (c) 1999,2000,2001 Jonathan Lemon <jlemon@FreeBSD.org>
 * All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 *	$FreeBSD: src/sys/sys/event.h,v 1.11 2001/02/24 01:41:31 jlemon Exp $
 */
/* </LICENSES> */

//! `<sys/event.h>`: `kqueue(2)`'s `struct kevent`, filters, flags and notes.
//!
//! Upstream: sys/sys/event.h @ 3ce1f3f79392
//!
//! Status: `wip`. The part shared with user space (`EVFILT_*`, `EV_SET`, `struct kevent`,
//! `EV_*`, every `NOTE_*`) and the kernel-only `__EV_SELECT`/`__EV_POLL`/`__EV_HUP` flags and
//! `EVFILT_MARKER`, which `select(2)`/`poll(2)` (`sys_generic.c`) use. `struct klist`,
//! `struct knote`, `struct filterops` and the rest of the kernel half come with
//! `kern_event.c`.

/// `EVFILT_READ`.
pub const EVFILT_READ: i16 = -1;
/// `EVFILT_WRITE`.
pub const EVFILT_WRITE: i16 = -2;
/// `EVFILT_AIO`: attached to aio requests.
pub const EVFILT_AIO: i16 = -3;
/// `EVFILT_VNODE`: attached to vnodes.
pub const EVFILT_VNODE: i16 = -4;
/// `EVFILT_PROC`: attached to struct process.
pub const EVFILT_PROC: i16 = -5;
/// `EVFILT_SIGNAL`: attached to struct process.
pub const EVFILT_SIGNAL: i16 = -6;
/// `EVFILT_TIMER`: timers.
pub const EVFILT_TIMER: i16 = -7;
/// `EVFILT_DEVICE`: devices.
pub const EVFILT_DEVICE: i16 = -8;
/// `EVFILT_EXCEPT`: exceptional conditions.
pub const EVFILT_EXCEPT: i16 = -9;
/// `EVFILT_USER`: user event.
pub const EVFILT_USER: i16 = -10;
/// `EVFILT_SYSCOUNT`.
pub const EVFILT_SYSCOUNT: i32 = 10;

/// `struct kevent`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Kevent {
    /// `ident`: identifier for this event.
    pub ident: usize,
    /// `filter`: filter for event.
    pub filter: i16,
    /// `flags`: action flags for kqueue.
    pub flags: u16,
    /// `fflags`: filter flag value.
    pub fflags: u32,
    /// `data`: filter data value.
    pub data: i64,
    /// `udata`: opaque user data identifier (a user pointer the kernel never follows).
    pub udata: usize,
}

/// `EV_SET(kevp, a, b, c, d, e, f)`.
pub const fn ev_set(
    ident: usize,
    filter: i16,
    flags: u16,
    fflags: u32,
    data: i64,
    udata: usize,
) -> Kevent {
    Kevent {
        ident,
        filter,
        flags,
        fflags,
        data,
        udata,
    }
}

/// `EV_ADD`: add event to kq (implies enable).
pub const EV_ADD: u16 = 0x0001;
/// `EV_DELETE`: delete event from kq.
pub const EV_DELETE: u16 = 0x0002;
/// `EV_ENABLE`: enable event.
pub const EV_ENABLE: u16 = 0x0004;
/// `EV_DISABLE`: disable event (not reported).
pub const EV_DISABLE: u16 = 0x0008;
/// `EV_ONESHOT`: only report one occurrence.
pub const EV_ONESHOT: u16 = 0x0010;
/// `EV_CLEAR`: clear event state after reporting.
pub const EV_CLEAR: u16 = 0x0020;
/// `EV_RECEIPT`: force `EV_ERROR` on success, data=0.
pub const EV_RECEIPT: u16 = 0x0040;
/// `EV_DISPATCH`: disable event after reporting.
pub const EV_DISPATCH: u16 = 0x0080;
/// `EV_SYSFLAGS`: reserved by system.
pub const EV_SYSFLAGS: u16 = 0xf800;
/// `EV_FLAG1`: filter-specific flag.
pub const EV_FLAG1: u16 = 0x2000;
/// `EV_EOF`: EOF detected.
pub const EV_EOF: u16 = 0x8000;
/// `EV_ERROR`: error, data contains errno.
pub const EV_ERROR: u16 = 0x4000;

/// `NOTE_LOWAT`: low water mark.
pub const NOTE_LOWAT: u32 = 0x0001;
/// `NOTE_EOF`: return on EOF.
pub const NOTE_EOF: u32 = 0x0002;
/// `NOTE_OOB`: OOB data on a socket.
pub const NOTE_OOB: u32 = 0x0004;
/// `NOTE_DELETE`: vnode was removed.
pub const NOTE_DELETE: u32 = 0x0001;
/// `NOTE_WRITE`: data contents changed.
pub const NOTE_WRITE: u32 = 0x0002;
/// `NOTE_EXTEND`: size increased.
pub const NOTE_EXTEND: u32 = 0x0004;
/// `NOTE_ATTRIB`: attributes changed.
pub const NOTE_ATTRIB: u32 = 0x0008;
/// `NOTE_LINK`: link count changed.
pub const NOTE_LINK: u32 = 0x0010;
/// `NOTE_RENAME`: vnode was renamed.
pub const NOTE_RENAME: u32 = 0x0020;
/// `NOTE_REVOKE`: vnode access was revoked.
pub const NOTE_REVOKE: u32 = 0x0040;
/// `NOTE_TRUNCATE`: vnode was truncated.
pub const NOTE_TRUNCATE: u32 = 0x0080;
/// `NOTE_EXIT`: process exited.
pub const NOTE_EXIT: u32 = 0x8000_0000;
/// `NOTE_FORK`: process forked.
pub const NOTE_FORK: u32 = 0x4000_0000;
/// `NOTE_EXEC`: process exec'd.
pub const NOTE_EXEC: u32 = 0x2000_0000;
/// `NOTE_PCTRLMASK`: mask for hint bits.
pub const NOTE_PCTRLMASK: u32 = 0xf000_0000;
/// `NOTE_PDATAMASK`: mask for pid.
pub const NOTE_PDATAMASK: u32 = 0x000f_ffff;
/// `NOTE_TRACK`: follow across forks.
pub const NOTE_TRACK: u32 = 0x0000_0001;
/// `NOTE_TRACKERR`: could not track child.
pub const NOTE_TRACKERR: u32 = 0x0000_0002;
/// `NOTE_CHILD`: am a child process.
pub const NOTE_CHILD: u32 = 0x0000_0004;
/// `NOTE_CHANGE`: device change event.
pub const NOTE_CHANGE: u32 = 0x0000_0001;
/// `NOTE_MSECONDS`: data is milliseconds.
pub const NOTE_MSECONDS: u32 = 0x0000_0000;
/// `NOTE_SECONDS`: data is seconds.
pub const NOTE_SECONDS: u32 = 0x0000_0001;
/// `NOTE_USECONDS`: data is microseconds.
pub const NOTE_USECONDS: u32 = 0x0000_0002;
/// `NOTE_NSECONDS`: data is nanoseconds.
pub const NOTE_NSECONDS: u32 = 0x0000_0003;
/// `NOTE_ABSTIME`: timeout is absolute.
pub const NOTE_ABSTIME: u32 = 0x0000_0010;
/// `NOTE_FFNOP`: ignore input fflags.
pub const NOTE_FFNOP: u32 = 0x0000_0000;
/// `NOTE_FFAND`: AND fflags.
pub const NOTE_FFAND: u32 = 0x4000_0000;
/// `NOTE_FFOR`: OR fflags.
pub const NOTE_FFOR: u32 = 0x8000_0000;
/// `NOTE_FFCOPY`: copy fflags.
pub const NOTE_FFCOPY: u32 = 0xc000_0000;
/// `NOTE_FFCTRLMASK`: masks for operations.
pub const NOTE_FFCTRLMASK: u32 = 0xc000_0000;
/// `NOTE_FFLAGSMASK`.
pub const NOTE_FFLAGSMASK: u32 = 0x00ff_ffff;
/// `NOTE_TRIGGER`: trigger the event.
pub const NOTE_TRIGGER: u32 = 0x0100_0000;

/// `__EV_SELECT`: match behavior of select (kernel only).
pub const __EV_SELECT: u16 = 0x0800;
/// `__EV_POLL`: match behavior of poll (kernel only).
pub const __EV_POLL: u16 = 0x1000;
/// `__EV_HUP`: device or socket disconnected (kernel only).
pub const __EV_HUP: u16 = EV_FLAG1;
/// `EVFILT_MARKER`: placemarker for tailq.
pub const EVFILT_MARKER: i16 = 0xf;

const _: () = {
    assert!(size_of::<Kevent>() == 32);
};
