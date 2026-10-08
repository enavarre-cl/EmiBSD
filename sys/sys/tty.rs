/*	$OpenBSD: tty.h,v 1.46 2026/08/06 20:38:02 claudio Exp $	*/
/*	$NetBSD: tty.h,v 1.30.4.1 1996/06/02 09:08:13 mrg Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (c) 1982, 1986, 1993
 *	The Regents of the University of California.  All rights reserved.
 * (c) UNIX System Laboratories, Inc.
 * All or some portions of this file are derived from material licensed
 * to the University of California by American Telephone and Telegraph
 * Co. or Unix System Laboratories, Inc. and are reproduced herein with
 * the permission of UNIX System Laboratories, Inc.
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
 *	@(#)tty.h	8.6 (Berkeley) 1/21/94
 */
/* </LICENSES> */

/* <CODE> */
//! `<sys/tty.h>`: the terminal: `struct clist` (the character queues), `struct tty`, the
//! `TS_*` state bits, the character classes, `struct ptmget` for `/dev/ptm` and the
//! `KERN_TTY_*` sysctl names.
//!
//! Upstream: sys/sys/tty.h @ 3ce1f3f79392
//!
//! A tty is shared by the process that reads or writes it, the driver's interrupt and soft
//! interrupt handlers, and the clock (`t_rstrt_to`), all of which change it through a
//! `struct tty *` at `spltty()`. Every member that changes after `ttymalloc` is therefore a
//! `Cell`, and the functions take `&Tty`.
//!
//! ## Deviations
//! - `struct clist` keeps the C's ring buffer and quote bitmap (`c_cs`, `c_cq`, allocated by
//!   `clalloc`), but its cursors `c_cf` and `c_cl` are indices into the ring instead of
//!   pointers, and `c_ce` is not stored (it is `c_cs + c_cn`). The C sets both cursors to
//!   NULL when the queue empties; here they are left as they are, and every user tests
//!   `c_cc` first as the C does. `firstc`/`nextc` hand out indices. The structure is
//!   `#[repr(C)]` so that `&q.c_cf` is a different address from `&q`: `tty_pty.c` sleeps
//!   on both.
//! - `t_termios` and `t_winsize` are `Cell`s of the whole structure, with accessor methods
//!   named after the `#define t_lflag t_termios.c_lflag` shorthands (`tp.t_lflag()`,
//!   `tp.set_t_lflag(..)`, `tp.t_cc(VERASE)`); `t_state` and `t_lflag` also have
//!   `SET`/`CLR`/`ISSET` helpers.
//! - `t_pgrp` and `t_session` are raw pointers in `Cell`s, as the process structures keep
//!   theirs (`Process::ps_pgrp`); `t_oproc`, `t_param` and `t_hwiflow` are `Option<fn>`s
//!   in `Cell`s. `t_param` returns `Result` and takes the requested termios by reference
//!   (the drivers' own termios is passed as a copy, which reads the same values).
//! - `struct itty` has its padding as named members (`_pad0`, `_pad1`), so `sysctl_rdstruct`
//!   copies no uninitialised byte (`docs/C_TO_RUST.md`).
//! - `isctty` and `isbackground` are functions; the sleep message strings, `tty_count`,
//!   `tk_*` and the prototypes are `kern/tty.rs`'s, `kern/tty_subr.rs`'s and
//!   `kern/tty_tty.rs`'s; the `ppp*`, `nmea*`, `msts*` and `endrun*` line disciplines are not
//!   configured.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::Ordering;

use crate::machine::copy::AbiPod;
use crate::queue_adapter;
use crate::sys::errno::Errno;
use crate::sys::ioccom::_ior;
use crate::sys::proc::{PS_CONTROLT, Pgrp, Process, Session};
use crate::sys::queue::{TailqEntry, TailqHead};
use crate::sys::selinfo::Selinfo;
use crate::sys::sysctl::{CTLTYPE_QUAD, CTLTYPE_STRUCT, Ctlname, SysctlPlain};
use crate::sys::termios::{Cc, Tcflag, Termios};
use crate::sys::time::Timeval;
use crate::sys::timeout::Timeout;
use crate::sys::ttycom::Winsize;
use crate::sys::types::{Dev, Gid, Pid};

/// Quad: input chars.
pub const KERN_TTY_TKNIN: i32 = 1;
/// Quad: output chars.
pub const KERN_TTY_TKNOUT: i32 = 2;
/// Quad: input chars, raw mode.
pub const KERN_TTY_TKRAWCC: i32 = 3;
/// Quad: input char, cooked mode.
pub const KERN_TTY_TKCANCC: i32 = 4;
/// Struct: tty stats.
pub const KERN_TTY_INFO: i32 = 5;
// was KERN_TTY_MAXPTYS 6, was KERN_TTY_NPTYS 7
/// Number of `kern.tty` names.
pub const KERN_TTY_MAXID: usize = 8;

/// `CTL_KERN_TTY_NAMES`.
pub const CTL_KERN_TTY_NAMES: [Ctlname; KERN_TTY_MAXID] = [
    Ctlname::NONE,
    Ctlname::new(b"tk_nin", CTLTYPE_QUAD),
    Ctlname::new(b"tk_nout", CTLTYPE_QUAD),
    Ctlname::new(b"tk_rawcc", CTLTYPE_QUAD),
    Ctlname::new(b"tk_cancc", CTLTYPE_QUAD),
    Ctlname::new(b"ttyinfo", CTLTYPE_STRUCT),
    Ctlname::new(b"gap", 0),
    Ctlname::new(b"gap", 0),
];

/// `struct ptmget`, for `/dev/ptm` pty getting ioctl `PTMGET`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ptmget {
    /// The controlling (master) side's descriptor.
    pub cfd: i32,
    /// The slave side's descriptor.
    pub sfd: i32,
    /// The master's path, NUL-terminated.
    pub cn: [u8; 16],
    /// The slave's path, NUL-terminated.
    pub sn: [u8; 16],
}

// SAFETY: two `i32`s and two byte arrays, 40 bytes without padding; any bit pattern is valid.
unsafe impl AbiPod for Ptmget {}

/// Get ptys.
pub const PTMGET: u64 = _ior::<Ptmget>(b't', 1);
/// The cloning pty device.
pub const PATH_PTMDEV: &[u8] = b"/dev/ptm";
/// XXX evil hardcoding of tty gid.
pub const TTY_GID: Gid = 4;

/// `struct clist`: a ring buffer of characters, with an optional bit per character for
/// `TTY_QUOTE` (see the module's deviations for the cursors).
///
/// Protected by: `spltty()`. *Don't* play with `c_cs`, `c_cq`, `c_cf` or `c_cl` outside
/// `tty_subr.c`.
#[repr(C)]
pub struct Clist {
    /// `c_cc`: count of characters in queue.
    pub c_cc: Cell<i32>,
    /// `c_cn`: total ring buffer length.
    pub c_cn: Cell<i32>,
    /// `c_cf`: index of the first character (meaningful while `c_cc > 0`).
    pub c_cf: Cell<usize>,
    /// `c_cl`: index of the next open character (meaningful while `c_cc > 0`).
    pub c_cl: Cell<usize>,
    /// `c_cs`: start of ring buffer (`c_cn` bytes), NULL before `clalloc`.
    pub c_cs: Cell<*mut u8>,
    /// `c_cq`: the quote bitmap (`QMEM(c_cn)` bytes), NULL when the queue has none.
    pub c_cq: Cell<*mut u8>,
}

impl Clist {
    /// An unallocated queue.
    pub const fn new() -> Self {
        Self {
            c_cc: Cell::new(0),
            c_cn: Cell::new(0),
            c_cf: Cell::new(0),
            c_cl: Cell::new(0),
            c_cs: Cell::new(ptr::null_mut()),
            c_cq: Cell::new(ptr::null_mut()),
        }
    }

    /// `tq = *clp`: a second header over the same ring and bitmap (`ttypend`).
    pub fn copy_header(&self) -> Self {
        Self {
            c_cc: Cell::new(self.c_cc.get()),
            c_cn: Cell::new(self.c_cn.get()),
            c_cf: Cell::new(self.c_cf.get()),
            c_cl: Cell::new(self.c_cl.get()),
            c_cs: Cell::new(self.c_cs.get()),
            c_cq: Cell::new(self.c_cq.get()),
        }
    }

    /// `tmp = *a; *a = *b; *b = tmp`: exchanges two queue headers (`catq`, `TIOCSETA`).
    pub fn swap(&self, other: &Self) {
        self.c_cc.swap(&other.c_cc);
        self.c_cn.swap(&other.c_cn);
        self.c_cf.swap(&other.c_cf);
        self.c_cl.swap(&other.c_cl);
        self.c_cs.swap(&other.c_cs);
        self.c_cq.swap(&other.c_cq);
    }
}

impl Default for Clist {
    fn default() -> Self {
        Self::new()
    }
}

/// `t_oproc`: start output.
pub type TOproc = fn(&Tty);
/// `t_param`: set hardware state.
pub type TParam = fn(&Tty, &Termios) -> Result<(), Errno>;
/// `t_hwiflow`: set hardware flow control; nonzero when it took effect.
pub type THwiflow = fn(&Tty, i32) -> i32;

/// `struct tty`: per-tty structure.
///
/// Should be split in two, into device and tty drivers. Glue could be masks of what to echo
/// and circular buffer (low, high, timeout).
///
/// Protected by: `spltty()` (and the kernel lock, which one CPU without preemption is).
pub struct Tty {
    /// `tty_link`: link in global tty list.
    pub tty_link: TailqEntry<Tty>,
    /// `t_rawq`: device raw input queue.
    pub t_rawq: Clist,
    /// `t_rawcc`: raw input queue statistics.
    pub t_rawcc: Cell<i64>,
    /// `t_canq`: device canonical queue.
    pub t_canq: Clist,
    /// `t_cancc`: canonical queue statistics.
    pub t_cancc: Cell<i64>,
    /// `t_outq`: device output queue.
    pub t_outq: Clist,
    /// `t_outcc`: output queue statistics.
    pub t_outcc: Cell<i64>,
    /// `t_qlen`: length of above queues.
    pub t_qlen: Cell<i32>,
    /// `t_line`: interface to device drivers (the `linesw[]` index).
    pub t_line: Cell<u8>,
    /// `t_dev`: device.
    pub t_dev: Cell<Dev>,
    /// `t_state`: device and driver (`TS_*`) state.
    pub t_state: Cell<i32>,
    /// `t_flags`: tty flags (the `TS_TSTAMP*` bits).
    pub t_flags: Cell<i32>,
    /// `t_pgrp`: foreground process group.
    pub t_pgrp: Cell<*const Pgrp>,
    /// `t_session`: enclosing session.
    pub t_session: Cell<*const Session>,
    /// `t_rsel`: tty read/oob select.
    pub t_rsel: Selinfo,
    /// `t_wsel`: tty write select.
    pub t_wsel: Selinfo,
    /// `t_termios`: termios state.
    pub t_termios: Cell<Termios>,
    /// `t_winsize`: window size.
    pub t_winsize: Cell<Winsize>,
    /// `t_oproc`: start output.
    pub t_oproc: Cell<Option<TOproc>>,
    /// `t_param`: set hardware state.
    pub t_param: Cell<Option<TParam>>,
    /// `t_hwiflow`: set hardware flow control.
    pub t_hwiflow: Cell<Option<THwiflow>>,
    /// `t_sc`: XXX: net/if_sl.c:sl_softc.
    pub t_sc: Cell<*mut c_void>,
    /// `t_column`: tty output column.
    pub t_column: Cell<i16>,
    /// `t_rocount`: tty.
    pub t_rocount: Cell<i16>,
    /// `t_rocol`: tty.
    pub t_rocol: Cell<i16>,
    /// `t_hiwat`: high water mark.
    pub t_hiwat: Cell<i16>,
    /// `t_lowat`: low water mark.
    pub t_lowat: Cell<i16>,
    /// `t_gen`: generation number.
    pub t_gen: Cell<i16>,
    /// `t_rstrt_to`: restart timeout.
    pub t_rstrt_to: Timeout,
    /// `t_tv`: timestamp.
    pub t_tv: Cell<Timeval>,
}

// SAFETY: see the type's doc: every member is changed at spltty() on the one CPU.
unsafe impl Sync for Tty {}

impl Tty {
    /// A zeroed tty, as `malloc(M_ZERO)` returns it in `ttymalloc`.
    pub const fn new() -> Self {
        Self {
            tty_link: TailqEntry::new(),
            t_rawq: Clist::new(),
            t_rawcc: Cell::new(0),
            t_canq: Clist::new(),
            t_cancc: Cell::new(0),
            t_outq: Clist::new(),
            t_outcc: Cell::new(0),
            t_qlen: Cell::new(0),
            t_line: Cell::new(0),
            t_dev: Cell::new(0),
            t_state: Cell::new(0),
            t_flags: Cell::new(0),
            t_pgrp: Cell::new(ptr::null()),
            t_session: Cell::new(ptr::null()),
            t_rsel: Selinfo::new(),
            t_wsel: Selinfo::new(),
            t_termios: Cell::new(Termios::zeroed()),
            t_winsize: Cell::new(Winsize {
                ws_row: 0,
                ws_col: 0,
                ws_xpixel: 0,
                ws_ypixel: 0,
            }),
            t_oproc: Cell::new(None),
            t_param: Cell::new(None),
            t_hwiflow: Cell::new(None),
            t_sc: Cell::new(ptr::null_mut()),
            t_column: Cell::new(0),
            t_rocount: Cell::new(0),
            t_rocol: Cell::new(0),
            t_hiwat: Cell::new(0),
            t_lowat: Cell::new(0),
            t_gen: Cell::new(0),
            t_rstrt_to: Timeout::zeroed(),
            t_tv: Cell::new(Timeval::new(0, 0)),
        }
    }

    /// Applies `f` to a copy of the termios and stores it back.
    fn termios_update(&self, f: impl FnOnce(&mut Termios)) {
        let mut t = self.t_termios.get();
        f(&mut t);
        self.t_termios.set(t);
    }

    /// `t_cc[i]` (`#define t_cc t_termios.c_cc`).
    pub fn t_cc(&self, i: u32) -> Cc {
        self.t_termios.get().c_cc[i as usize]
    }

    /// `t_cc`: the whole control character array.
    pub fn t_cc_all(&self) -> [Cc; crate::sys::termios::NCCS] {
        self.t_termios.get().c_cc
    }

    /// `t_cc[..] = cc`.
    pub fn set_t_cc_all(&self, cc: [Cc; crate::sys::termios::NCCS]) {
        self.termios_update(|t| t.c_cc = cc);
    }

    /// `t_cflag`.
    pub fn t_cflag(&self) -> Tcflag {
        self.t_termios.get().c_cflag
    }

    /// `t_cflag = v`.
    pub fn set_t_cflag(&self, v: Tcflag) {
        self.termios_update(|t| t.c_cflag = v);
    }

    /// `t_iflag`.
    pub fn t_iflag(&self) -> Tcflag {
        self.t_termios.get().c_iflag
    }

    /// `t_iflag = v`.
    pub fn set_t_iflag(&self, v: Tcflag) {
        self.termios_update(|t| t.c_iflag = v);
    }

    /// `t_lflag`.
    pub fn t_lflag(&self) -> Tcflag {
        self.t_termios.get().c_lflag
    }

    /// `t_lflag = v`.
    pub fn set_t_lflag(&self, v: Tcflag) {
        self.termios_update(|t| t.c_lflag = v);
    }

    /// `SET(tp->t_lflag, bits)`.
    pub fn t_lflag_set(&self, bits: Tcflag) {
        self.termios_update(|t| t.c_lflag |= bits);
    }

    /// `CLR(tp->t_lflag, bits)`.
    pub fn t_lflag_clr(&self, bits: Tcflag) {
        self.termios_update(|t| t.c_lflag &= !bits);
    }

    /// `t_oflag`.
    pub fn t_oflag(&self) -> Tcflag {
        self.t_termios.get().c_oflag
    }

    /// `t_oflag = v`.
    pub fn set_t_oflag(&self, v: Tcflag) {
        self.termios_update(|t| t.c_oflag = v);
    }

    /// `t_ispeed`.
    pub fn t_ispeed(&self) -> i32 {
        self.t_termios.get().c_ispeed
    }

    /// `t_ispeed = v`.
    pub fn set_t_ispeed(&self, v: i32) {
        self.termios_update(|t| t.c_ispeed = v);
    }

    /// `t_ospeed`.
    pub fn t_ospeed(&self) -> i32 {
        self.t_termios.get().c_ospeed
    }

    /// `t_ospeed = v`.
    pub fn set_t_ospeed(&self, v: i32) {
        self.termios_update(|t| t.c_ospeed = v);
    }

    /// `ISSET(tp->t_state, bits)`.
    pub fn t_state_isset(&self, bits: i32) -> bool {
        self.t_state.get() & bits != 0
    }

    /// `SET(tp->t_state, bits)`.
    pub fn t_state_set(&self, bits: i32) {
        self.t_state.set(self.t_state.get() | bits);
    }

    /// `CLR(tp->t_state, bits)`.
    pub fn t_state_clr(&self, bits: i32) {
        self.t_state.set(self.t_state.get() & !bits);
    }

    /// `tp->t_pgrp`, if any.
    pub fn pgrp(&self) -> Option<&'static Pgrp> {
        // SAFETY: a foreground group stays allocated while it is a tty's: `pgdelete` clears
        // the pointer of its session's terminal before it frees the group.
        unsafe { self.t_pgrp.get().as_ref() }
    }

    /// `tp->t_session`, if any.
    pub fn session(&self) -> Option<&'static Session> {
        // SAFETY: the tty holds a reference on its session (`SESSHOLD` in `TIOCSCTTY`),
        // released with `SESSRELE` when the pointer is cleared.
        unsafe { self.t_session.get().as_ref() }
    }
}

impl Default for Tty {
    fn default() -> Self {
        Self::new()
    }
}

queue_adapter!(
    /// `TAILQ_ENTRY(tty) tty_link`: the global tty list.
    pub TtyLink: Tty, tty_link => TailqEntry<Tty>
);

/// `struct itty`: small version of `struct tty` exported via sysctl `KERN_TTY_INFO`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Itty {
    /// `t_dev`.
    pub t_dev: Dev,
    /// `t_rawq_c_cc`.
    pub t_rawq_c_cc: i32,
    /// `t_canq_c_cc`.
    pub t_canq_c_cc: i32,
    /// `t_outq_c_cc`.
    pub t_outq_c_cc: i32,
    /// `t_hiwat`.
    pub t_hiwat: i16,
    /// `t_lowat`.
    pub t_lowat: i16,
    /// `t_column`.
    pub t_column: i16,
    /// The C compiler's padding before `t_state`, named so it is copied as zeros.
    pub _pad0: i16,
    /// `t_state`.
    pub t_state: i32,
    /// `t_session_id`.
    pub t_session_id: Pid,
    /// `t_pgrp_pg_id`.
    pub t_pgrp_pg_id: Pid,
    /// `t_line`.
    pub t_line: u8,
    /// The C compiler's tail padding, named so it is copied as zeros.
    pub _pad1: [u8; 3],
}

// SAFETY: `repr(C)` integers with the C's padding as explicit members: 40 bytes, every byte
// initialised.
unsafe impl SysctlPlain for Itty {}

/// Sleep priority for tty reads.
pub const TTIPRI: i32 = 25;
/// Sleep priority for tty writes.
pub const TTOPRI: i32 = 26;

/// `OBUFSIZ`.
pub const OBUFSIZ: usize = 512;

/// `TTYHOG(tp)`: the input queue limit.
pub fn ttyhog(tp: &Tty) -> i32 {
    tp.t_qlen.get()
}

/// `TTMAXLOWAT`.
pub const TTMAXLOWAT: i32 = 256;
/// `TTMINLOWAT`.
pub const TTMINLOWAT: i32 = 32;
/// `TTMINHIWAT`.
pub const TTMINHIWAT: i32 = 100;
/// Min space above hiwat.
pub const TTHIWATMINSPACE: i32 = 200;

// These flags are kept in t_state.

/// Process waiting for tty.
pub const TS_ASLEEP: i32 = 0x00001;
/// Tty in async I/O mode.
pub const TS_ASYNC: i32 = 0x00002;
/// Draining output.
pub const TS_BUSY: i32 = 0x00004;
/// Carrier is present.
pub const TS_CARR_ON: i32 = 0x00008;
/// Outq has been flushed during DMA.
pub const TS_FLUSH: i32 = 0x00010;
/// Open has completed.
pub const TS_ISOPEN: i32 = 0x00020;
/// Further input blocked.
pub const TS_TBLOCK: i32 = 0x00040;
/// Wait for output char processing.
pub const TS_TIMEOUT: i32 = 0x00080;
/// Output paused.
pub const TS_TTSTOP: i32 = 0x00100;
/// Open in progress.
pub const TS_WOPEN: i32 = 0x00200;
/// Tty requires exclusivity.
pub const TS_XCLUDE: i32 = 0x00400;

// State for intra-line fancy editing work.

/// State for lowercase \ work.
pub const TS_BKSL: i32 = 0x00800;
/// Counting tab width, ignore FLUSHO.
pub const TS_CNTTB: i32 = 0x01000;
/// Within a \.../ for PRTRUB.
pub const TS_ERASE: i32 = 0x02000;
/// Next character is literal.
pub const TS_LNCH: i32 = 0x04000;
/// Retyping suspended input (PENDIN).
pub const TS_TYPEN: i32 = 0x08000;
/// The editing state.
pub const TS_LOCAL: i32 = TS_BKSL | TS_CNTTB | TS_ERASE | TS_LNCH | TS_TYPEN;

/// Update timestamp on DCD set.
pub const TS_TSTAMPDCDSET: i32 = 0x10000;
/// Update timestamp on DCD clr.
pub const TS_TSTAMPDCDCLR: i32 = 0x20000;
/// Update timestamp on CTS set.
pub const TS_TSTAMPCTSSET: i32 = 0x40000;
/// Update timestamp on CTS clr.
pub const TS_TSTAMPCTSCLR: i32 = 0x80000;

// Character type information.

/// Ordinary character.
pub const ORDINARY: u8 = 0;
/// Control character.
pub const CONTROL: u8 = 1;
/// Backspace.
pub const BACKSPACE: u8 = 2;
/// Newline.
pub const NEWLINE: u8 = 3;
/// Horizontal tab.
pub const TAB: u8 = 4;
/// Vertical tab.
pub const VTAB: u8 = 5;
/// Carriage return.
pub const RETURN: u8 = 6;

/// `struct speedtab`: a speed and the driver's code for it; a table ends with `sp_speed`
/// -1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Speedtab {
    /// Speed.
    pub sp_speed: i32,
    /// Code.
    pub sp_code: i32,
}

// Modem control commands (driver).

/// Set the modem lines.
pub const DMSET: i32 = 0;
/// Set bits.
pub const DMBIS: i32 = 1;
/// Clear bits.
pub const DMBIC: i32 = 2;
/// Get the modem lines.
pub const DMGET: i32 = 3;

// Flags on a character passed to ttyinput.

/// Character mask.
pub const TTY_CHARMASK: i32 = 0x0000_00ff;
/// Character quoted.
pub const TTY_QUOTE: i32 = 0x0000_0100;
/// Error mask.
pub const TTY_ERRORMASK: i32 = 0xff00_0000_u32 as i32;
/// Framing error or BREAK condition.
pub const TTY_FE: i32 = 0x0100_0000;
/// Parity error.
pub const TTY_PE: i32 = 0x0200_0000;

/// `isctty(pr, tp)`: is `tp` controlling terminal for `pr`?
pub fn isctty(pr: &Process, tp: &Tty) -> bool {
    ptr::eq(pr.session(), tp.t_session.get())
        && pr.ps_flags.load(Ordering::Relaxed) & PS_CONTROLT != 0
}

/// `isbackground(pr, tp)`: is `pr` in background of `tp`?
pub fn isbackground(pr: &Process, tp: &Tty) -> bool {
    isctty(pr, tp) && !ptr::eq(pr.ps_pgrp.get(), tp.t_pgrp.get())
}

/// `TAILQ_HEAD(ttylist_head, tty)`: the ttylist is a TAILQ; made `Sync`, as the other
/// global list heads, under `ttylist_lock`.
pub struct TtylistHead(pub TailqHead<TtyLink>);

// SAFETY: the list is changed under `ttylist_lock` (and spltty), as in C.
unsafe impl Sync for TtylistHead {}

const _: () = assert!(size_of::<Itty>() == 40);
const _: () = assert!(size_of::<Ptmget>() == 40);
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/sys/tty.h");
        let ours: &[(&str, i64)] = &[
            ("KERN_TTY_TKNIN", KERN_TTY_TKNIN.into()),
            ("KERN_TTY_INFO", KERN_TTY_INFO.into()),
            ("KERN_TTY_MAXID", KERN_TTY_MAXID as i64),
            ("TTY_GID", TTY_GID.into()),
            ("TTIPRI", TTIPRI.into()),
            ("TTOPRI", TTOPRI.into()),
            ("OBUFSIZ", OBUFSIZ as i64),
            ("TTMAXLOWAT", TTMAXLOWAT.into()),
            ("TTMINLOWAT", TTMINLOWAT.into()),
            ("TTMINHIWAT", TTMINHIWAT.into()),
            ("TTHIWATMINSPACE", TTHIWATMINSPACE.into()),
            ("TS_ASLEEP", TS_ASLEEP.into()),
            ("TS_BUSY", TS_BUSY.into()),
            ("TS_CARR_ON", TS_CARR_ON.into()),
            ("TS_ISOPEN", TS_ISOPEN.into()),
            ("TS_TTSTOP", TS_TTSTOP.into()),
            ("TS_XCLUDE", TS_XCLUDE.into()),
            ("TS_BKSL", TS_BKSL.into()),
            ("TS_TYPEN", TS_TYPEN.into()),
            ("TS_TSTAMPCTSCLR", TS_TSTAMPCTSCLR.into()),
            ("ORDINARY", ORDINARY.into()),
            ("RETURN", RETURN.into()),
            ("DMGET", DMGET.into()),
            ("TTY_CHARMASK", TTY_CHARMASK.into()),
            ("TTY_QUOTE", TTY_QUOTE.into()),
            ("TTY_FE", TTY_FE.into()),
            ("TTY_PE", TTY_PE.into()),
        ];
        for (name, value) in ours {
            assert_eq!(crate::reftest::int(&defs, name), Some(*value), "{name}");
        }
    }
}
/* </TESTS> */
