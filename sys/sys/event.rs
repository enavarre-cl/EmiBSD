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

/* <CODE> */
//! `<sys/event.h>`: `kqueue(2)`'s `struct kevent`, filters, flags and notes, and the kernel
//! half: `struct klist`, `struct knote`, `struct filterops`, `struct klistops`, `struct
//! kqueue_scan_state` and the inline helpers `knote_modify`, `knote_process` and
//! `klist_empty`.
//!
//! Upstream: sys/sys/event.h @ 3ce1f3f79392
//!
//! A knote is a pool item (`knote_pool`, `kern_event.rs`) that a kqueue owns from
//! `kqueue_register` until `knote_drop`; while it lives it is linked into its kqueue's
//! descriptor table or hash (`kn_link`), into the object's `struct klist` (`kn_selnext`)
//! and, while active, into the kqueue's pending queue (`kn_tqe`). Filters, objects and the
//! kqueue reach it as `&Knote`; the links are the `queue.h` adapters.
//!
//! ## Deviations
//! - The members a lock guards are `Cell`s (the knote is shared through pointers, as in C);
//!   `kn_kevent` is [`KnKevent`], a `struct kevent` of `Cell`s, so that the field-alias
//!   macros (`kn_id`, `kn_filter`, `kn_flags`, `kn_fflags`, `kn_data`, `kn_udata`, `kn_fp`)
//!   are methods returning the member's `&Cell` (the `m_next()` idiom of
//!   `docs/C_TO_RUST.md`).
//! - `kn_ptr`, a union of the file, the process and the user-event flag, is [`KnPtr`] with
//!   the three alternatives side by side (the union idiom); `kn_sfflags` is `u32` (it holds
//!   the `unsigned int` `fflags` of a `struct kevent`).
//! - `struct filterops` is a struct of optional `fn` pointers: `f_attach` returns
//!   `Result<(), Errno>`; `f_event`, `f_modify` and `f_process` return `bool` (the C's
//!   "non-zero if active"); `f_process`'s `struct kevent *` is `Option<&mut Kevent>` (NULL
//!   when only checking). `kn_fop` is `None` only for the scan markers.
//! - `SLIST_HEAD(knlist, knote)` serves C both for a klist (through `kn_selnext`) and for the
//!   kqueue's descriptor table and hash (through `kn_link`); the adapter fixes the link, so
//!   these are two types: `Klist::kl_list` lists through [`KnSelnext`] and [`Knlist`]
//!   through [`KnLink`].
//! - `struct klistops`' callbacks take the opaque `kl_arg` as `*const c_void` and are
//!   `unsafe fn`: `klist_init` (`kern_event.rs`) is where the caller vouches for it.
//! - `kqueue_scan_state`'s markers are knotes inside the state; the state must not move
//!   between `kqueue_scan_setup` and `kqueue_scan_finish`, which is `kqueue_scan_setup`'s
//!   safety contract.
//! - The prototypes are their functions in `kern/kern_event.rs`; `knote_modify_fn` and
//!   `knote_process_fn` take the event function as an `fn` pointer, as in C.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr;

use crate::kern::kern_event::{knote_assign, knote_submit};
use crate::kern::subr_prf::panic;
use crate::machine::copy::AbiPod;
use crate::queue_adapter;
use crate::sys::errno::Errno;
use crate::sys::eventvar::Kqueue;
use crate::sys::file::File;
use crate::sys::proc::Process;
use crate::sys::queue::{SlistEntry, SlistHead, TailqEntry};

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

// SAFETY: `#[repr(C)]` of integers without implicit padding (8 + 2 + 2 + 4 + 8 + 8 bytes, the
// size asserted below): every byte is initialised and every bit pattern is a valid event.
unsafe impl AbiPod for Kevent {}

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

/// `NOTE_SUBMIT`: initial knote submission (hint flag for in-kernel use; must not equal any
/// existing note).
pub const NOTE_SUBMIT: u32 = 0x0100_0000;

/// `KN_HASHSIZE`: buckets of a kqueue's hash of non-descriptor knotes (XXX should be
/// tunable).
pub const KN_HASHSIZE: i32 = 64;

/// `NOTE_SIGNAL`: flag indicating hint is a signal. Used by `EVFILT_SIGNAL`, and also shared
/// by `EVFILT_PROC` (all knotes attached to `ps_klist`).
pub const NOTE_SIGNAL: u32 = 0x0800_0000;

/// `FILTEROP_ISFD`: each knote of this filter is associated with a file descriptor
/// (`ident` == file descriptor).
pub const FILTEROP_ISFD: i32 = 0x0000_0001;
/// `FILTEROP_MPSAFE`: the kqueue subsystem can invoke `f_attach`, `f_detach`, `f_modify`
/// and `f_process` without the kernel lock.
pub const FILTEROP_MPSAFE: i32 = 0x0000_0002;

/// `KN_ACTIVE`: event has been triggered.
pub const KN_ACTIVE: i32 = 0x0001;
/// `KN_QUEUED`: event is on queue.
pub const KN_QUEUED: i32 = 0x0002;
/// `KN_DISABLED`: event is disabled.
pub const KN_DISABLED: i32 = 0x0004;
/// `KN_DETACHED`: knote is detached.
pub const KN_DETACHED: i32 = 0x0008;
/// `KN_PROCESSING`: knote is being processed.
pub const KN_PROCESSING: i32 = 0x0010;
/// `KN_WAITING`: waiting on processing.
pub const KN_WAITING: i32 = 0x0020;

/// `struct filterops`: the event filter interface.
///
/// `f_flags` defines properties of the event filter (`FILTEROP_ISFD`, `FILTEROP_MPSAFE`).
/// `f_attach` attaches the knote to the object. `f_detach` detaches the knote from the
/// object; the object must not use this knote for delivering events after this callback has
/// returned. `f_event` notifies the filter about an event (called through `knote()`).
/// `f_modify` modifies the knote with new state from the user and returns `true` if the
/// knote has become active. `f_process` checks if the event is active and returns `true` if
/// the event should be returned to the user; if `kev` is given and the event is active, the
/// callback should store the event's state in it for delivery to the user.
///
/// Concurrency control: the kqueue subsystem serializes calls of `f_attach`, `f_detach`,
/// `f_modify` and `f_process`.
pub struct Filterops {
    /// `f_flags`.
    pub f_flags: i32,
    /// `f_attach(kn)`.
    pub f_attach: Option<FAttach>,
    /// `f_detach(kn)`.
    pub f_detach: Option<FDetach>,
    /// `f_event(kn, hint)`.
    pub f_event: Option<FEvent>,
    /// `f_modify(kev, kn)`.
    pub f_modify: Option<FModify>,
    /// `f_process(kn, kev)`.
    pub f_process: Option<FProcess>,
}

/// The type of `f_attach`.
pub type FAttach = fn(&Knote) -> Result<(), Errno>;
/// The type of `f_detach`.
pub type FDetach = fn(&Knote);
/// The type of `f_event`: `true` when the event is active.
pub type FEvent = fn(&Knote, i64) -> bool;
/// The type of `f_modify`: `true` when the knote has become active.
pub type FModify = fn(&mut Kevent, &Knote) -> bool;
/// The type of `f_process`: `true` when the event should be returned to the user.
pub type FProcess = fn(&Knote, Option<&mut Kevent>) -> bool;

/// `kn_kevent`: the knote's `struct kevent`, one `Cell` per member (see the module's
/// deviations). `ident` and `filter` are \[I\]; the others \[o\].
pub struct KnKevent {
    /// `ident`.
    pub ident: Cell<usize>,
    /// `filter`.
    pub filter: Cell<i16>,
    /// `flags`.
    pub flags: Cell<u16>,
    /// `fflags`.
    pub fflags: Cell<u32>,
    /// `data`.
    pub data: Cell<i64>,
    /// `udata`.
    pub udata: Cell<usize>,
}

impl KnKevent {
    /// A zeroed event.
    pub const fn new() -> Self {
        Self {
            ident: Cell::new(0),
            filter: Cell::new(0),
            flags: Cell::new(0),
            fflags: Cell::new(0),
            data: Cell::new(0),
            udata: Cell::new(0),
        }
    }

    /// `*kev = kn->kn_kevent`.
    pub fn get(&self) -> Kevent {
        Kevent {
            ident: self.ident.get(),
            filter: self.filter.get(),
            flags: self.flags.get(),
            fflags: self.fflags.get(),
            data: self.data.get(),
            udata: self.udata.get(),
        }
    }

    /// `kn->kn_kevent = *kev`.
    pub fn set(&self, kev: &Kevent) {
        self.ident.set(kev.ident);
        self.filter.set(kev.filter);
        self.flags.set(kev.flags);
        self.fflags.set(kev.fflags);
        self.data.set(kev.data);
        self.udata.set(kev.udata);
    }
}

impl Default for KnKevent {
    fn default() -> Self {
        Self::new()
    }
}

/// `kn_ptr`: what a knote points at, by filter (see the module's deviations).
pub struct KnPtr {
    /// \[o\] `p_fp`: file data pointer (`kn_fp`), for the `FILTEROP_ISFD` filters; the knote
    /// holds a reference to the file.
    pub p_fp: Cell<Option<&'static File>>,
    /// `p_process`: process pointer (`EVFILT_PROC`, `EVFILT_SIGNAL`).
    pub p_process: Cell<*const Process>,
    /// `p_useract`: user event active (`EVFILT_USER`).
    pub p_useract: Cell<i32>,
}

impl KnPtr {
    /// Nothing pointed at.
    pub const fn new() -> Self {
        Self {
            p_fp: Cell::new(None),
            p_process: Cell::new(ptr::null()),
            p_useract: Cell::new(0),
        }
    }
}

impl Default for KnPtr {
    fn default() -> Self {
        Self::new()
    }
}

/// `struct knote`.
///
/// Locking: \[I\] immutable after creation, \[o\] object lock, \[q\] `kn_kq->kq_lock`.
pub struct Knote {
    /// `kn_link`: for fd (the kqueue's `kq_knlist`/`kq_knhash`).
    pub kn_link: SlistEntry<Knote>,
    /// `kn_selnext`: for `struct selinfo` (the object's klist).
    pub kn_selnext: SlistEntry<Knote>,
    /// `kn_tqe`: the kqueue's pending queue (`kq_head`).
    pub kn_tqe: TailqEntry<Knote>,
    /// \[I\] `kn_kq`: which queue we are on.
    pub kn_kq: Cell<*const Kqueue>,
    /// `kn_kevent`.
    pub kn_kevent: KnKevent,
    /// \[q\] `kn_status`: `KN_*`.
    pub kn_status: Cell<i32>,
    /// \[o\] `kn_sfflags`: saved filter flags.
    pub kn_sfflags: Cell<u32>,
    /// \[o\] `kn_sdata`: saved data field.
    pub kn_sdata: Cell<i64>,
    /// `kn_ptr`.
    pub kn_ptr: KnPtr,
    /// `kn_fop`: the filter.
    pub kn_fop: Cell<Option<&'static Filterops>>,
    /// \[o\] `kn_hook`: the object, for the filter.
    pub kn_hook: Cell<*mut c_void>,
    /// \[I\] `kn_pollid`.
    pub kn_pollid: Cell<u32>,
}

impl Knote {
    /// A zeroed knote, as `pool_get(&knote_pool, PR_ZERO)` returns it (and the scan
    /// markers).
    pub const fn new() -> Self {
        Self {
            kn_link: SlistEntry::new(),
            kn_selnext: SlistEntry::new(),
            kn_tqe: TailqEntry::new(),
            kn_kq: Cell::new(ptr::null()),
            kn_kevent: KnKevent::new(),
            kn_status: Cell::new(0),
            kn_sfflags: Cell::new(0),
            kn_sdata: Cell::new(0),
            kn_ptr: KnPtr::new(),
            kn_fop: Cell::new(None),
            kn_hook: Cell::new(ptr::null_mut()),
            kn_pollid: Cell::new(0),
        }
    }

    /// `kn_id`: `kn_kevent.ident` \[I\].
    pub fn kn_id(&self) -> &Cell<usize> {
        &self.kn_kevent.ident
    }

    /// `kn_filter`: `kn_kevent.filter` \[I\].
    pub fn kn_filter(&self) -> &Cell<i16> {
        &self.kn_kevent.filter
    }

    /// `kn_flags`: `kn_kevent.flags` \[o\].
    pub fn kn_flags(&self) -> &Cell<u16> {
        &self.kn_kevent.flags
    }

    /// `kn_fflags`: `kn_kevent.fflags` \[o\].
    pub fn kn_fflags(&self) -> &Cell<u32> {
        &self.kn_kevent.fflags
    }

    /// `kn_data`: `kn_kevent.data` \[o\].
    pub fn kn_data(&self) -> &Cell<i64> {
        &self.kn_kevent.data
    }

    /// `kn_udata`: `kn_kevent.udata` \[o\].
    pub fn kn_udata(&self) -> &Cell<usize> {
        &self.kn_kevent.udata
    }

    /// `kn_fp`: `kn_ptr.p_fp` \[o\].
    pub fn kn_fp(&self) -> &Cell<Option<&'static File>> {
        &self.kn_ptr.p_fp
    }

    /// `kn->kn_fp`, which every attached `FILTEROP_ISFD` knote has.
    pub fn fp(&self) -> &'static File {
        match self.kn_ptr.p_fp.get() {
            Some(fp) => fp,
            None => panic(format_args!("knote {:p}: no kn_fp", self)),
        }
    }

    /// `kn->kn_fop`, which every knote but a scan marker has.
    pub fn fop(&self) -> &'static Filterops {
        match self.kn_fop.get() {
            Some(fop) => fop,
            None => panic(format_args!("knote {:p}: no kn_fop", self)),
        }
    }

    /// `kn->kn_kq`.
    pub fn kq(&self) -> &Kqueue {
        // SAFETY: `kqueue_register` sets `kn_kq` before the knote is linked anywhere, and a
        // kqueue outlives its knotes (`KQRELE` asserts `kq_nknotes == 0`).
        match unsafe { self.kn_kq.get().as_ref() } {
            Some(kq) => kq,
            None => panic(format_args!("knote {:p}: no kn_kq", self)),
        }
    }

    /// `kn->kn_flags |= flags`.
    pub fn set_flags(&self, flags: u16) {
        self.kn_flags().set(self.kn_flags().get() | flags);
    }

    /// `kn->kn_flags &= ~flags`.
    pub fn clear_flags(&self, flags: u16) {
        self.kn_flags().set(self.kn_flags().get() & !flags);
    }

    /// `kn->kn_flags & flags`, as a test.
    pub fn has_flags(&self, flags: u16) -> bool {
        self.kn_flags().get() & flags != 0
    }
}

impl Default for Knote {
    fn default() -> Self {
        Self::new()
    }
}

queue_adapter!(
    /// A kqueue's descriptor table and hash list knotes through `kn_link`.
    pub KnLink: Knote, kn_link => SlistEntry<Knote>
);
queue_adapter!(
    /// A klist lists knotes through `kn_selnext`.
    pub KnSelnext: Knote, kn_selnext => SlistEntry<Knote>
);
queue_adapter!(
    /// A kqueue's pending queue (`kq_head`) links knotes through `kn_tqe`.
    pub KnTqe: Knote, kn_tqe => TailqEntry<Knote>
);

/// `SLIST_HEAD(knlist, knote)` over `kn_link`: one slot of a kqueue's descriptor table or
/// hash.
pub type Knlist = SlistHead<KnLink>;

/// `struct klist`: the knotes an object delivers events to, and how to lock them.
///
/// This is currently visible to userland to work around broken programs which pull in
/// `<sys/proc.h>` or `<sys/selinfo.h>`.
pub struct Klist {
    /// `kl_list`, through `kn_selnext`.
    pub kl_list: SlistHead<KnSelnext>,
    /// `kl_ops`: the lock operations; `None` for the kernel lock at `splhigh`.
    pub kl_ops: Cell<Option<&'static Klistops>>,
    /// `kl_arg`: the lock, for `kl_ops`.
    pub kl_arg: Cell<*const c_void>,
}

// SAFETY: a klist is touched under its lock (`kl_ops`) or the kernel lock at `splhigh`, as in
// C; the kernel runs one CPU.
unsafe impl Sync for Klist {}

impl Klist {
    /// A zeroed klist: empty, locked by the kernel lock.
    pub const fn new() -> Self {
        Self {
            kl_list: SlistHead::new(),
            kl_ops: Cell::new(None),
            kl_arg: Cell::new(ptr::null()),
        }
    }
}

impl Default for Klist {
    fn default() -> Self {
        Self::new()
    }
}

/// `struct klistops`.
pub struct Klistops {
    /// `klo_assertlk(arg)`.
    pub klo_assertlk: unsafe fn(*const c_void),
    /// `klo_lock(arg)`: returns the state `klo_unlock` restores.
    pub klo_lock: unsafe fn(*const c_void) -> i32,
    /// `klo_unlock(arg, ls)`.
    pub klo_unlock: unsafe fn(*const c_void, i32),
}

/// `struct kqueue_scan_state`.
pub struct KqueueScanState {
    /// `kqs_kq`: kqueue of this scan.
    pub kqs_kq: Cell<*const Kqueue>,
    /// `kqs_start`: start marker.
    pub kqs_start: Knote,
    /// `kqs_end`: end marker.
    pub kqs_end: Knote,
    /// `kqs_nevent`: number of events collected.
    pub kqs_nevent: Cell<i32>,
    /// `kqs_queued`: if set, end marker is in queue.
    pub kqs_queued: Cell<bool>,
}

impl KqueueScanState {
    /// A zeroed state (`memset(scan, 0, sizeof(*scan))`).
    pub const fn new() -> Self {
        Self {
            kqs_kq: Cell::new(ptr::null()),
            kqs_start: Knote::new(),
            kqs_end: Knote::new(),
            kqs_nevent: Cell::new(0),
            kqs_queued: Cell::new(false),
        }
    }

    /// `scan->kqs_kq`.
    pub fn kq(&self) -> &Kqueue {
        // SAFETY: `kqueue_scan_setup` sets it and holds a reference (`KQREF`) until
        // `kqueue_scan_finish`.
        match unsafe { self.kqs_kq.get().as_ref() } {
            Some(kq) => kq,
            None => panic(format_args!("kqueue scan {:p}: not set up", self)),
        }
    }
}

impl Default for KqueueScanState {
    fn default() -> Self {
        Self::new()
    }
}

/// `knote_modify_fn(kev, kn, f_event)`.
pub fn knote_modify_fn(kev: &Kevent, kn: &Knote, f_event: FEvent) -> bool {
    knote_assign(kev, kn);
    f_event(kn, 0)
}

/// `knote_modify(kev, kn)`: assigns the user's parameters and re-runs the event function.
pub fn knote_modify(kev: &Kevent, kn: &Knote) -> bool {
    match kn.fop().f_event {
        Some(f_event) => knote_modify_fn(kev, kn, f_event),
        None => panic(format_args!("knote_modify: knote {:p} has no f_event", kn)),
    }
}

/// `knote_process_fn(kn, kev, f_event)`.
pub fn knote_process_fn(kn: &Knote, kev: Option<&mut Kevent>, f_event: FEvent) -> bool {
    // If called from kqueue_scan(), skip f_event when EV_ONESHOT is set, to preserve old
    // behaviour.
    let active = if kev.is_some() && kn.has_flags(EV_ONESHOT) {
        true
    } else {
        f_event(kn, 0)
    };
    if active {
        knote_submit(kn, kev);
    }
    active
}

/// `knote_process(kn, kev)`: whether the event is active, its state stored in `kev` if so.
pub fn knote_process(kn: &Knote, kev: Option<&mut Kevent>) -> bool {
    match kn.fop().f_event {
        Some(f_event) => knote_process_fn(kn, kev, f_event),
        None => panic(format_args!("knote_process: knote {:p} has no f_event", kn)),
    }
}

/// `klist_empty(klist)`.
pub fn klist_empty(klist: &Klist) -> bool {
    klist.kl_list.is_empty()
}

const _: () = {
    assert!(size_of::<Kevent>() == 32);
};
/* </CODE> */
