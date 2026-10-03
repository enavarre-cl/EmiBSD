/*	$OpenBSD: tty.c,v 1.186 2026/08/19 22:56:24 daniel Exp $	*/
/*	$NetBSD: tty.c,v 1.68.4.2 1996/06/06 16:04:52 thorpej Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (c) 1982, 1986, 1990, 1991, 1993
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
 *	@(#)tty.c	8.8 (Berkeley) 1/21/94
 */
/* </LICENSES> */

//! The terminal line discipline: `kern/tty.c`.
//!
//! Upstream: sys/kern/tty.c @ 3ce1f3f79392
//!
//! A tty driver (`com(4)`, `pluart(4)`, the ptys) hands every received character to
//! [`ttyinput`] through its line discipline's `l_rint` and starts output with `t_oproc`;
//! readers and writers come in through `l_read`/`l_write` ([`ttread`], [`ttwrite`]) and the
//! terminal ioctls through [`ttioctl`]. This file is the termios discipline: canonical
//! (line) and raw input, echo and erase, the signal characters sent to the foreground
//! process group, flow control, output processing, job control of background readers and
//! writers, `^T` status (`ttyinfo`) and the global tty list behind `kern.tty`.
//!
//! ## Deviations
//! - Every function takes `&Tty` (the tty's members are `Cell`s, `sys/tty.rs`). Functions
//!   returning an errno return `Result`; [`ttioctl`] returns `Ok(false)` for the C's `-1`
//!   ("not a tty ioctl"). The C lets an `ERESTART` from a background sleep in `ttioctl`
//!   (`-1` too) look like "not handled"; here it propagates as `ERESTART`.
//! - `cc = tp->t_cc` (a pointer to the live array) is a copy taken where the C takes the
//!   pointer, re-read where a sleep could have changed it (`ttread` after `ttysleep`).
//! - `CCEQ` and `TTBREAKC` compare the `int` character, `TTY_QUOTE` included, as the C
//!   does ([`cceq`] on an `int`); `sys::termios::cceq` takes bytes and is for drivers.
//! - The kqueue filters (`ttkqfilter`, `filt_tty*`, the `filterops`) need `struct knote`
//!   (`kern_event.c`): `ttkqfilter` reports itself, and `ttyfree`'s `klist_invalidate` has
//!   nothing to invalidate.
//! - `ttread`'s `VMIN`/`VTIME` timer is a `struct timeout` on `ttread`'s stack instead of a
//!   `malloc(M_TEMP)` one; it is deleted on every way out, as the C frees it.
//! - `ttyinfo`'s resident set size is `vm_rssize` (`vm_resident_count` reads the pmap's
//!   count, which `kern_sysctl.rs` reads the same way).
//! - `TIOCCONS` checks that `/dev/console` can be opened through `namei`, as the C does;
//!   without a root file system that lookup fails with `ENOENT`.
//! - `ttymalloc` panics if `malloc(M_WAITOK)` fails (`malloc(9)` cannot sleep yet).
//! - `sysctl_tty` asks `sysctl_pty` for the names it does not know (`NPTY > 0`).

use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicI32, AtomicI64, Ordering};

use libkern::{explicit_bzero, scanc};

use crate::dev::cons::{cn_tab, constty, set_constty};
use crate::dev::rnd::enqueue_randomness;
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_proc::{pgfind, prfind};
use crate::kern::kern_prot::suser;
use crate::kern::kern_resource::{calctsru, tuagg_get_process};
use crate::kern::kern_rwlock::{rw_enter_write, rw_exit_write};
use crate::kern::kern_sig::{pgsignal, prsignal, sigismasked};
use crate::kern::kern_subr::{uiomove, ureadc};
use crate::kern::kern_synch::{nowake, tsleep_nsec, wakeup};
use crate::kern::kern_sysctl::{sysctl_rdquad, sysctl_rdstruct};
use crate::kern::kern_tc::microtime;
use crate::kern::kern_timeout::{timeout_add_msec, timeout_del, timeout_set};
use crate::kern::sched_bsd::AVERUNNABLE;
use crate::kern::subr_log::{consbufp, msgbuf_putchar};
use crate::kern::subr_prf::panic;
use crate::kern::subr_prf::{Str, printf, ttyprintf};
use crate::kern::sys_generic::selwakeup;
use crate::kern::tty_conf::{LINESW, NLINESW, linesw};
use crate::kern::tty_pty::sysctl_pty;
use crate::kern::tty_subr::{
    b_to_q, catq, clalloc, clfree, firstc, getc, ndflush, nextc, putc, unputc,
};
use crate::kern::vfs_lookup::{namei, ndinit};
use crate::kern::vfs_subr::vrele;
use crate::kern::vfs_vnops::vn_lock;
use crate::kern::vfs_vops::{VOP_ACCESS, VOP_UNLOCK};
use crate::machine::conf::cdevsw;
use crate::machine::cpu::curproc;
use crate::machine::intr::{IPL_TTY, splassert, spltty, splx};
use crate::sys::errno::Errno;
use crate::sys::event::Knote;
use crate::sys::fcntl::{FNONBLOCK, FREAD, FWRITE};
use crate::sys::filio::{FIOASYNC, FIOGETOWN, FIONREAD, FIOSETOWN};
use crate::sys::ioctl::{ioctl_arg, ioctl_ret};
use crate::sys::lock::{LK_EXCLUSIVE, LK_RETRY};
use crate::sys::malloc::{M_SYSCTL, M_TTYS, M_WAITOK, M_ZERO};
use crate::sys::namei::{FOLLOW, LOOKUP, NiDirp, UNVEIL_READ, UNVEIL_WRITE};
use crate::sys::param::{FSCALE, FSHIFT, PCATCH, PZERO};
use crate::sys::pledge::{PLEDGE_RPATH, PLEDGE_WPATH};
use crate::sys::proc::{
    NO_PID, P_WEXIT, PS_CONTROLT, PS_EMBRYO, PS_EXITING, PS_PPWAIT, Proc, ProcThrLink, Process,
    ProcessPglist, SONPROC, SRUN, Tusage, sess_leader, sesshold, sessrele,
};
use crate::sys::queue::{ListHead, TailqHead};
use crate::sys::rwlock::Rwlock;
use crate::sys::signal::{
    SIGHUP, SIGINFO, SIGINT, SIGIO, SIGQUIT, SIGTSTP, SIGTTIN, SIGTTOU, SIGWINCH,
};
use crate::sys::signalvar::sigpending;
use crate::sys::systm::INFSLP;
use crate::sys::termios::{
    _POSIX_VDISABLE, ALTWERASE, BRKINT, CHWFLOW, CIGNORE, CLOCAL, CREAD, Cc, ECHO, ECHOCTL, ECHOE,
    ECHOK, ECHOKE, ECHONL, ECHOPRT, EXTPROC, FLUSHO, ICANON, ICRNL, IEXTEN, IGNBRK, IGNCR, IGNPAR,
    IMAXBEL, INLCR, INPCK, ISIG, ISTRIP, IUCLC, IXANY, IXOFF, IXON, MDMBUF, NOFLSH, NOKERNINFO,
    OCRNL, OLCUC, ONLCR, ONLRET, ONOCR, ONOEOT, OPOST, OXTABS, PARMRK, PENDIN, TOSTOP, Termios,
    VDISCARD, VDSUSP, VEOF, VEOL, VEOL2, VERASE, VINTR, VKILL, VLNEXT, VMIN, VQUIT, VREPRINT,
    VSTART, VSTATUS, VSTOP, VSUSP, VTIME, VWERASE, XCASE,
};
use crate::sys::time::sec_to_nsec;
use crate::sys::timeout::{Timeout, timeout_triggered};
use crate::sys::tty::{
    BACKSPACE, CONTROL, Clist, Itty, KERN_TTY_INFO, KERN_TTY_TKCANCC, KERN_TTY_TKNIN,
    KERN_TTY_TKNOUT, KERN_TTY_TKRAWCC, NEWLINE, OBUFSIZ, ORDINARY, RETURN, Speedtab, TAB,
    TS_ASLEEP, TS_ASYNC, TS_BKSL, TS_BUSY, TS_CARR_ON, TS_CNTTB, TS_ERASE, TS_ISOPEN, TS_LNCH,
    TS_LOCAL, TS_TBLOCK, TS_TIMEOUT, TS_TSTAMPCTSCLR, TS_TSTAMPCTSSET, TS_TSTAMPDCDCLR,
    TS_TSTAMPDCDSET, TS_TTSTOP, TS_TYPEN, TS_WOPEN, TS_XCLUDE, TTHIWATMINSPACE, TTIPRI, TTMAXLOWAT,
    TTMINHIWAT, TTMINLOWAT, TTOPRI, TTY_CHARMASK, TTY_ERRORMASK, TTY_FE, TTY_PE, TTY_QUOTE, Tty,
    TtylistHead, VTAB, isbackground, isctty, ttyhog,
};
use crate::sys::ttycom::{
    TIOCCBRK, TIOCCONS, TIOCDRAIN, TIOCEXCL, TIOCFLUSH, TIOCGETA, TIOCGETD, TIOCGPGRP, TIOCGSID,
    TIOCGTSTAMP, TIOCGWINSZ, TIOCM_CAR, TIOCM_CTS, TIOCNXCL, TIOCOUTQ, TIOCSBRK, TIOCSCTTY,
    TIOCSETA, TIOCSETAF, TIOCSETAW, TIOCSETD, TIOCSPGRP, TIOCSTART, TIOCSTAT, TIOCSTOP,
    TIOCSTSTAMP, TIOCSWINSZ, Tstamps, Winsize,
};
use crate::sys::ttydefaults::{CEOT, TTYDEFCHARS, ctrl};
use crate::sys::types::{Dev, Fixpt, major};
use crate::sys::uio::Uio;
use crate::sys::vnode::{IO_NDELAY, VREAD};
use crate::unported;

// Symbolic sleep message strings.

/// `ttclos`.
pub const TTCLOS: &str = "ttycls";
/// `ttopen`.
pub const TTOPEN: &str = "ttyopn";
/// `ttybg`.
pub const TTYBG: &str = "ttybg";
/// `ttyin`.
pub const TTYIN: &str = "ttyin";
/// `ttyout`.
pub const TTYOUT: &str = "ttyout";

// Table with character classes and parity. The 8th bit indicates parity, the 7th bit
// indicates the character is an alphameric or underscore (for ALTWERASE), and the low 6 bits
// indicate delay type. If the low 6 bits are 0 then the character needs no special
// processing on output; classes other than 0 might be translated or (not currently) require
// delays.

/// Even parity.
const E: u8 = 0x00;
/// Odd parity.
const O: u8 = 0x80;
/// Alpha or underscore.
const ALPHA: u8 = 0x40;
/// The class bits of a `char_type` entry.
pub const CCLASSMASK: u8 = 0x3f;

const BS: u8 = BACKSPACE;
const CC: u8 = CONTROL;
const CR: u8 = RETURN;
const NA: u8 = ORDINARY | ALPHA;
const NL: u8 = NEWLINE;
const NO: u8 = ORDINARY;
const TB: u8 = TAB;
const VT: u8 = VTAB;

/// `char_type[]`: the class and parity of every character.
pub static CHAR_TYPE: [u8; 256] = {
    const LOW: [u8; 128] = [
        E | CC,
        O | CC,
        O | CC,
        E | CC,
        O | CC,
        E | CC,
        E | CC,
        O | CC, // nul - bel
        O | BS,
        E | TB,
        E | NL,
        O | CC,
        E | VT,
        O | CR,
        O | CC,
        E | CC, // bs - si
        O | CC,
        E | CC,
        E | CC,
        O | CC,
        E | CC,
        O | CC,
        O | CC,
        E | CC, // dle - etb
        E | CC,
        O | CC,
        O | CC,
        E | CC,
        O | CC,
        E | CC,
        E | CC,
        O | CC, // can - us
        O | NO,
        E | NO,
        E | NO,
        O | NO,
        E | NO,
        O | NO,
        O | NO,
        E | NO, // sp - '
        E | NO,
        O | NO,
        O | NO,
        E | NO,
        O | NO,
        E | NO,
        E | NO,
        O | NO, // ( - /
        E | NA,
        O | NA,
        O | NA,
        E | NA,
        O | NA,
        E | NA,
        E | NA,
        O | NA, // 0 - 7
        O | NA,
        E | NA,
        E | NO,
        O | NO,
        E | NO,
        O | NO,
        O | NO,
        E | NO, // 8 - ?
        O | NO,
        E | NA,
        E | NA,
        O | NA,
        E | NA,
        O | NA,
        O | NA,
        E | NA, // @ - G
        E | NA,
        O | NA,
        O | NA,
        E | NA,
        O | NA,
        E | NA,
        E | NA,
        O | NA, // H - O
        E | NA,
        O | NA,
        O | NA,
        E | NA,
        O | NA,
        E | NA,
        E | NA,
        O | NA, // P - W
        O | NA,
        E | NA,
        E | NA,
        O | NO,
        E | NO,
        O | NO,
        O | NO,
        O | NA, // X - _
        E | NO,
        O | NA,
        O | NA,
        E | NA,
        O | NA,
        E | NA,
        E | NA,
        O | NA, // ` - g
        O | NA,
        E | NA,
        E | NA,
        O | NA,
        E | NA,
        O | NA,
        O | NA,
        E | NA, // h - o
        O | NA,
        E | NA,
        E | NA,
        O | NA,
        E | NA,
        O | NA,
        O | NA,
        E | NA, // p - w
        E | NA,
        O | NA,
        O | NA,
        E | NO,
        O | NO,
        E | NO,
        E | NO,
        O | CC, // x - del
    ];
    let mut t = [NA; 256];
    let mut i = 0;
    while i < 128 {
        t[i] = LOW[i];
        i += 1;
    }
    // Meta chars; should be settable per character set; for now, treat them all as normal
    // characters.
    t
};

/// `PARITY(c)`.
#[allow(dead_code)] // the C defines it next to the table; nothing uses it there either
fn parity(c: usize) -> u8 {
    CHAR_TYPE[c] & O
}

/// `ISALPHA(c)`.
fn isalpha(c: i32) -> u8 {
    CHAR_TYPE[(c & TTY_CHARMASK) as usize] & ALPHA
}

/// `CCLASS(c)`.
fn cclass(c: i32) -> u8 {
    CHAR_TYPE[(c & 0xff) as usize] & CCLASSMASK
}

/// `islower(c)`.
fn islower(c: i32) -> bool {
    (i32::from(b'a')..=i32::from(b'z')).contains(&c)
}

/// `isupper(c)`.
fn isupper(c: i32) -> bool {
    (i32::from(b'A')..=i32::from(b'Z')).contains(&c)
}

/// `tolower(c)`.
fn tolower(c: i32) -> i32 {
    c - i32::from(b'A') + i32::from(b'a')
}

/// `toupper(c)`.
fn toupper(c: i32) -> i32 {
    c - i32::from(b'a') + i32::from(b'A')
}

/// `ttylist`: every allocated tty (`TAILQ_HEAD`).
pub static TTYLIST: TtylistHead = TtylistHead(TailqHead::new());
/// `tty_count`: number of ttys in global ttylist.
pub static TTY_COUNT: AtomicI32 = AtomicI32::new(0);
/// `ttylist_lock`.
pub static TTYLIST_LOCK: Rwlock = Rwlock::new("ttylist");

/// `tk_cancc`: input characters in canonical mode.
pub static TK_CANCC: AtomicI64 = AtomicI64::new(0);
/// `tk_nin`: input characters.
pub static TK_NIN: AtomicI64 = AtomicI64::new(0);
/// `tk_nout`: output characters.
pub static TK_NOUT: AtomicI64 = AtomicI64::new(0);
/// `tk_rawcc`: input characters in raw mode.
pub static TK_RAWCC: AtomicI64 = AtomicI64::new(0);

/// `CCEQ(val, c)` on an `int` character: `c` equals the enabled control character `val`.
fn cceq(val: Cc, c: i32) -> bool {
    c == i32::from(val) && val != _POSIX_VDISABLE
}

/// `d_stop` of the tty's driver (`(*cdevsw[major(tp->t_dev)].d_stop)(tp, rw)`).
fn d_stop(tp: &Tty, rw: i32) {
    let _ = (cdevsw(major(tp.t_dev.get())).d_stop)(tp, rw);
}

/// `ttyopen`: initial open of tty, or (re)entry to standard tty line discipline.
pub fn ttyopen(device: Dev, tp: &Tty, _p: &Proc) -> Result<(), Errno> {
    let s = spltty();
    tp.t_dev.set(device);
    if !tp.t_state_isset(TS_ISOPEN) {
        tp.t_state_set(TS_ISOPEN);
        tp.t_winsize.set(Winsize::default());
        tp.t_column.set(0);
    }
    tp.t_state_clr(TS_WOPEN);
    splx(s);
    Ok(())
}

/// `ttyclose`: handle close() on a tty line: flush and set to initial state, bumping
/// generation number so that pending read/write calls can detect recycling of the tty.
pub fn ttyclose(tp: &Tty) -> Result<(), Errno> {
    if constty().is_some_and(|c| ptr::eq(c, tp)) {
        set_constty(None);
    }

    ttyflush(tp, FREAD | FWRITE);

    tp.t_gen.set(tp.t_gen.get().wrapping_add(1));
    tp.t_pgrp.set(ptr::null());
    if let Some(s) = tp.session() {
        sessrele(s);
    }
    tp.t_session.set(ptr::null());
    tp.t_state.set(0);
    Ok(())
}

/// `FLUSHQ(q)`.
fn flushq(q: &Clist) {
    if q.c_cc.get() != 0 {
        ndflush(q, q.c_cc.get());
    }
}

/// `TTBREAKC(c, lflag)`: is `c` a line delimiter ("break" character)?
fn ttbreakc(c: i32, cc: &[Cc], lflag: u32) -> bool {
    c == i32::from(b'\n')
        || ((c == i32::from(cc[VEOF as usize])
            || c == i32::from(cc[VEOL as usize])
            || (c == i32::from(cc[VEOL2 as usize]) && lflag & IEXTEN != 0))
            && c != i32::from(_POSIX_VDISABLE))
}

/// Where `ttyinput`'s special-character switch ends up.
enum Input {
    /// `goto endcase`.
    Endcase,
    /// `goto restartoutput`.
    Restartoutput,
    /// `goto startoutput`.
    Startoutput,
    /// `return (r)`.
    Return(i32),
}

/// `parmrk:` in `ttyinput`: the `\377 \0 c` marking of a bad character.
fn parmrk(tp: &Tty, c: i32, iflag: u32) {
    let _ = putc(0o377 | TTY_QUOTE, &tp.t_rawq);
    if iflag & ISTRIP != 0 || c != 0o377 {
        let _ = putc(TTY_QUOTE, &tp.t_rawq);
    }
    let _ = putc(c | TTY_QUOTE, &tp.t_rawq);
}

/// `ttyinput`: process input of a single character received on a tty. Returns 0 normally,
/// 1 if a costly operation was reached.
pub fn ttyinput(c: i32, tp: &Tty) -> i32 {
    let mut c = c;
    let mut ret = 0;

    enqueue_randomness(((tp.t_dev.get() << 8) | c) as u32);
    // If receiver is not enabled, drop it.
    if tp.t_cflag() & CREAD == 0 {
        return 0;
    }

    // If input is pending take it first.
    let lflag = tp.t_lflag();
    let s = spltty();
    if lflag & PENDIN != 0 {
        ttypend(tp);
    }
    splx(s);
    // Gather stats.
    if lflag & ICANON != 0 {
        TK_CANCC.fetch_add(1, Ordering::Relaxed);
        tp.t_cancc.set(tp.t_cancc.get() + 1);
    } else {
        TK_RAWCC.fetch_add(1, Ordering::Relaxed);
        tp.t_rawcc.set(tp.t_rawcc.get() + 1);
    }
    TK_NIN.fetch_add(1, Ordering::Relaxed);

    let cc = tp.t_cc_all();
    let iflag = tp.t_iflag();
    let exit = 'switch: {
        // Handle exceptional conditions (break, parity, framing).
        let error = c & TTY_ERRORMASK;
        if error != 0 {
            c &= !TTY_ERRORMASK;
            if error & TTY_FE != 0 && c == 0 {
                // Break.
                if iflag & IGNBRK != 0 {
                    break 'switch Input::Return(0);
                }
                ttyflush(tp, FREAD | FWRITE);
                if iflag & BRKINT != 0 {
                    pgsignal(tp.pgrp(), SIGINT, true);
                    break 'switch Input::Endcase;
                } else if iflag & PARMRK != 0 {
                    parmrk(tp, c, iflag);
                    break 'switch Input::Endcase;
                }
            } else if (error & TTY_PE != 0 && iflag & INPCK != 0) || error & TTY_FE != 0 {
                if iflag & IGNPAR != 0 {
                    break 'switch Input::Endcase;
                } else if iflag & PARMRK != 0 {
                    parmrk(tp, c, iflag);
                    break 'switch Input::Endcase;
                } else {
                    c = 0;
                }
            }
        }
        if c == 0o377 && iflag & ISTRIP == 0 && iflag & PARMRK != 0 {
            parmrk(tp, c, iflag);
            break 'switch Input::Endcase;
        }

        // In tandem mode, check high water mark.
        if iflag & IXOFF != 0 || tp.t_cflag() & CHWFLOW != 0 {
            ttyblock(tp);
        }
        if !tp.t_state_isset(TS_TYPEN) && iflag & ISTRIP != 0 {
            c &= !0x80;
        }
        if lflag & EXTPROC == 0 {
            // Check for literal nexting very first.
            if tp.t_state_isset(TS_LNCH) {
                c |= TTY_QUOTE;
                tp.t_state_clr(TS_LNCH);
            }
            // Scan for special characters. This code is really just a big case statement
            // with non-constant cases. The bottom of the case statement is labeled
            // ``endcase'', so goto it after a case match, or similar.

            // Control chars which aren't controlled by ICANON, ISIG, or IXON.
            if lflag & IEXTEN != 0 {
                if cceq(cc[VLNEXT as usize], c) {
                    if lflag & ECHO != 0 {
                        if lflag & ECHOE != 0 {
                            let _ = ttyoutput(i32::from(b'^'), tp);
                            let _ = ttyoutput(i32::from(b'\x08'), tp);
                        } else {
                            ttyecho(c, tp);
                        }
                    }
                    tp.t_state_set(TS_LNCH);
                    break 'switch Input::Endcase;
                }
                if cceq(cc[VDISCARD as usize], c) {
                    if lflag & FLUSHO != 0 {
                        tp.t_lflag_clr(FLUSHO);
                    } else {
                        ttyflush(tp, FWRITE);
                        ttyecho(c, tp);
                        if tp.t_rawq.c_cc.get() + tp.t_canq.c_cc.get() != 0 {
                            ret = ttyretype(tp);
                        }
                        tp.t_lflag_set(FLUSHO);
                    }
                    break 'switch Input::Startoutput;
                }
            }
            // Signals.
            if lflag & ISIG != 0 {
                if cceq(cc[VINTR as usize], c) || cceq(cc[VQUIT as usize], c) {
                    if lflag & NOFLSH == 0 {
                        ttyflush(tp, FREAD | FWRITE);
                    }
                    ttyecho(c, tp);
                    pgsignal(
                        tp.pgrp(),
                        if cceq(cc[VINTR as usize], c) {
                            SIGINT
                        } else {
                            SIGQUIT
                        },
                        true,
                    );
                    break 'switch Input::Endcase;
                }
                if cceq(cc[VSUSP as usize], c) {
                    if lflag & NOFLSH == 0 {
                        ttyflush(tp, FREAD);
                    }
                    ttyecho(c, tp);
                    pgsignal(tp.pgrp(), SIGTSTP, true);
                    break 'switch Input::Endcase;
                }
            }
            // Handle start/stop characters.
            if iflag & IXON != 0 {
                if cceq(cc[VSTOP as usize], c) {
                    if !tp.t_state_isset(TS_TTSTOP) {
                        tp.t_state_set(TS_TTSTOP);
                        d_stop(tp, 0);
                        break 'switch Input::Return(0);
                    }
                    if !cceq(cc[VSTART as usize], c) {
                        break 'switch Input::Return(0);
                    }
                    // if VSTART == VSTOP then toggle
                    break 'switch Input::Endcase;
                }
                if cceq(cc[VSTART as usize], c) {
                    break 'switch Input::Restartoutput;
                }
            }
            // IGNCR, ICRNL, & INLCR
            if c == i32::from(b'\r') {
                if iflag & IGNCR != 0 {
                    break 'switch Input::Endcase;
                } else if iflag & ICRNL != 0 {
                    c = i32::from(b'\n');
                }
            } else if c == i32::from(b'\n') && iflag & INLCR != 0 {
                c = i32::from(b'\r');
            }
        }
        if tp.t_lflag() & EXTPROC == 0 && lflag & ICANON != 0 {
            // From here on down canonical mode character processing takes place.

            // upper case or specials with IUCLC and XCASE
            if lflag & XCASE != 0 && iflag & IUCLC != 0 {
                if tp.t_state_isset(TS_BKSL) {
                    tp.t_state_clr(TS_BKSL);
                    c = match u8::try_from(c) {
                        Ok(b'\'') => i32::from(b'`'),
                        Ok(b'!') => i32::from(b'|'),
                        Ok(b'^') => i32::from(b'~'),
                        Ok(b'(') => i32::from(b'{'),
                        Ok(b')') => i32::from(b'}'),
                        _ => c,
                    };
                } else if c == i32::from(b'\\') {
                    tp.t_state_set(TS_BKSL);
                    break 'switch Input::Endcase;
                } else if isupper(c) {
                    c = tolower(c);
                }
            } else if iflag & IUCLC != 0 && isupper(c) {
                c = tolower(c);
            }
            // erase (^H / ^?)
            if cceq(cc[VERASE as usize], c) {
                if tp.t_rawq.c_cc.get() != 0 {
                    ret = ttyrub(unputc(&tp.t_rawq), tp);
                }
                break 'switch Input::Endcase;
            }
            // kill (^U)
            if cceq(cc[VKILL as usize], c) {
                if lflag & ECHOKE != 0
                    && tp.t_rawq.c_cc.get() == i32::from(tp.t_rocount.get())
                    && lflag & ECHOPRT == 0
                {
                    while tp.t_rawq.c_cc.get() != 0 {
                        if ttyrub(unputc(&tp.t_rawq), tp) != 0 {
                            ret = 1;
                        }
                    }
                } else {
                    ttyecho(c, tp);
                    if lflag & ECHOK != 0 || lflag & ECHOKE != 0 {
                        ttyecho(i32::from(b'\n'), tp);
                    }
                    flushq(&tp.t_rawq);
                    tp.t_rocount.set(0);
                }
                tp.t_state_clr(TS_LOCAL);
                break 'switch Input::Endcase;
            }
            // word erase (^W)
            if cceq(cc[VWERASE as usize], c) && lflag & IEXTEN != 0 {
                let alt = lflag & ALTWERASE != 0;

                // erase whitespace
                loop {
                    c = unputc(&tp.t_rawq);
                    if c != i32::from(b' ') && c != i32::from(b'\t') {
                        break;
                    }
                    if ttyrub(c, tp) != 0 {
                        ret = 1;
                    }
                }
                if c == -1 {
                    break 'switch Input::Endcase;
                }
                // erase last char of word and remember the next chars type (for ALTWERASE)
                if ttyrub(c, tp) != 0 {
                    ret = 1;
                }
                c = unputc(&tp.t_rawq);
                if c == -1 {
                    break 'switch Input::Endcase;
                }
                if c == i32::from(b' ') || c == i32::from(b'\t') {
                    let _ = putc(c, &tp.t_rawq);
                    break 'switch Input::Endcase;
                }
                let ctype = isalpha(c);
                // erase rest of word
                loop {
                    if ttyrub(c, tp) != 0 {
                        ret = 1;
                    }
                    c = unputc(&tp.t_rawq);
                    if c == -1 {
                        break 'switch Input::Endcase;
                    }
                    if c == i32::from(b' ') || c == i32::from(b'\t') || (alt && isalpha(c) != ctype)
                    {
                        break;
                    }
                }
                let _ = putc(c, &tp.t_rawq);
                break 'switch Input::Endcase;
            }
            // reprint line (^R)
            if cceq(cc[VREPRINT as usize], c) && lflag & IEXTEN != 0 {
                ret = ttyretype(tp);
                break 'switch Input::Endcase;
            }
            // ^T - kernel info and generate SIGINFO
            if cceq(cc[VSTATUS as usize], c) && lflag & IEXTEN != 0 {
                if lflag & ISIG != 0 {
                    pgsignal(tp.pgrp(), SIGINFO, true);
                }
                if lflag & NOKERNINFO == 0 {
                    ttyinfo(tp);
                }
                break 'switch Input::Endcase;
            }
        }
        // Check for input buffer overflow
        if tp.t_rawq.c_cc.get() + tp.t_canq.c_cc.get() >= ttyhog(tp) {
            if iflag & IMAXBEL != 0 {
                if tp.t_outq.c_cc.get() < i32::from(tp.t_hiwat.get()) {
                    let _ = ttyoutput(i32::from(ctrl(b'g')), tp);
                }
            } else {
                ttyflush(tp, FREAD | FWRITE);
            }
            break 'switch Input::Endcase;
        }
        // Put data char in q for user and wakeup on seeing a line delimiter.
        if putc(c, &tp.t_rawq) >= 0 {
            if lflag & ICANON == 0 {
                ttwakeup(tp);
                ttyecho(c, tp);
                break 'switch Input::Endcase;
            }
            if ttbreakc(c, &cc, lflag) {
                tp.t_rocount.set(0);
                catq(&tp.t_rawq, &tp.t_canq);
                ttwakeup(tp);
            } else {
                let rocount = tp.t_rocount.get();
                tp.t_rocount.set(rocount.wrapping_add(1));
                if rocount == 0 {
                    tp.t_rocol.set(tp.t_column.get());
                }
            }
            if tp.t_state_isset(TS_ERASE) {
                // end of prterase \.../
                tp.t_state_clr(TS_ERASE);
                let _ = ttyoutput(i32::from(b'/'), tp);
            }
            let mut i = i32::from(tp.t_column.get());
            ttyecho(c, tp);
            if cceq(cc[VEOF as usize], c) && lflag & ECHO != 0 {
                // Place the cursor over the '^' of the ^D.
                i = 2.min(i32::from(tp.t_column.get()) - i);
                while i > 0 {
                    let _ = ttyoutput(i32::from(b'\x08'), tp);
                    i -= 1;
                }
            }
        }
        Input::Endcase
    };

    match exit {
        Input::Return(r) => return r,
        Input::Endcase => {
            // IXANY means allow any character to restart output.
            if tp.t_state_isset(TS_TTSTOP)
                && iflag & IXANY == 0
                && cc[VSTART as usize] != cc[VSTOP as usize]
            {
                return ret;
            }
            tp.t_lflag_clr(FLUSHO);
            tp.t_state_clr(TS_TTSTOP);
        }
        Input::Restartoutput => {
            tp.t_lflag_clr(FLUSHO);
            tp.t_state_clr(TS_TTSTOP);
        }
        Input::Startoutput => {}
    }
    ttstart(tp);
    ret
}

/// `ttyoutput`: output a single character on a tty, doing output processing as needed
/// (expanding tabs, newline processing, etc.). Returns < 0 if succeeds, otherwise returns
/// char to resend. Must be recursive.
pub fn ttyoutput(c: i32, tp: &Tty) -> i32 {
    let mut c = c;
    let oflag = tp.t_oflag();
    if oflag & OPOST == 0 {
        TK_NOUT.fetch_add(1, Ordering::Relaxed);
        tp.t_outcc.set(tp.t_outcc.get() + 1);
        if tp.t_lflag() & FLUSHO == 0 && putc(c, &tp.t_outq) != 0 {
            return c;
        }
        return -1;
    }
    // Do tab expansion if OXTABS is set. Special case if we external processing, we don't
    // do the tab expansion because we'll probably get it wrong. If tab expansion needs to
    // be done, let it happen externally.
    c &= TTY_CHARMASK;
    if c == i32::from(b'\t') && oflag & OXTABS != 0 && tp.t_lflag() & EXTPROC == 0 {
        let mut c = 8 - (i32::from(tp.t_column.get()) & 7);
        let notout = if tp.t_lflag() & FLUSHO != 0 {
            0
        } else {
            let s = spltty(); // Don't interrupt tabs.
            let notout = b_to_q(&b"        "[..c as usize], &tp.t_outq) as i32;
            c -= notout;
            TK_NOUT.fetch_add(i64::from(c), Ordering::Relaxed);
            tp.t_outcc.set(tp.t_outcc.get() + i64::from(c));
            splx(s);
            notout
        };
        tp.t_column.set(tp.t_column.get().wrapping_add(c as i16));
        return if notout != 0 { i32::from(b'\t') } else { -1 };
    }
    if c == i32::from(CEOT) && oflag & ONOEOT != 0 {
        return -1;
    }

    // Newline translation: if ONLCR is set, translate newline into "\r\n". If OCRNL is set,
    // translate '\r' into '\n'.
    if c == i32::from(b'\n') && tp.t_oflag() & ONLCR != 0 {
        TK_NOUT.fetch_add(1, Ordering::Relaxed);
        tp.t_outcc.set(tp.t_outcc.get() + 1);
        if tp.t_lflag() & FLUSHO == 0 && putc(i32::from(b'\r'), &tp.t_outq) != 0 {
            return c;
        }
        tp.t_column.set(0);
    } else if c == i32::from(b'\r') && tp.t_oflag() & OCRNL != 0 {
        c = i32::from(b'\n');
    }

    if tp.t_oflag() & OLCUC != 0 && islower(c) {
        c = toupper(c);
    } else if tp.t_oflag() & OLCUC != 0 && tp.t_lflag() & XCASE != 0 {
        let c2 = match u8::try_from(c) {
            Ok(b'`') => i32::from(b'\''),
            Ok(b'|') => i32::from(b'!'),
            Ok(b'~') => i32::from(b'^'),
            Ok(b'{') => i32::from(b'('),
            Ok(b'}') => i32::from(b')'),
            _ => c,
        };
        if c == i32::from(b'\\') || isupper(c) || c != c2 {
            TK_NOUT.fetch_add(1, Ordering::Relaxed);
            tp.t_outcc.set(tp.t_outcc.get() + 1);
            if putc(i32::from(b'\\'), &tp.t_outq) != 0 {
                return c;
            }
            c = c2;
        }
    }
    if tp.t_oflag() & ONOCR != 0 && c == i32::from(b'\r') && tp.t_column.get() == 0 {
        return -1;
    }

    TK_NOUT.fetch_add(1, Ordering::Relaxed);
    tp.t_outcc.set(tp.t_outcc.get() + 1);
    if tp.t_lflag() & FLUSHO == 0 && putc(c, &tp.t_outq) != 0 {
        return c;
    }

    let mut col = tp.t_column.get();
    match cclass(c) {
        BACKSPACE => {
            if col > 0 {
                col -= 1;
            }
        }
        CONTROL => {}
        NEWLINE => {
            if tp.t_oflag() & ONLRET != 0 || tp.t_oflag() & OCRNL != 0 {
                col = 0;
            }
        }
        RETURN => col = 0,
        ORDINARY => col = col.wrapping_add(1),
        TAB => col = col.wrapping_add(8) & !7,
        _ => {}
    }
    tp.t_column.set(col);
    -1
}

/// `ttioctl`: ioctls for all tty devices. Called after line-discipline specific ioctl has
/// been called to do discipline-specific functions and/or reject any of these ioctl
/// commands. `Ok(false)` where the C returns `-1`: not a tty ioctl.
pub fn ttioctl(tp: &Tty, cmd: u64, data: &mut [u8], flag: i32, p: &Proc) -> Result<bool, Errno> {
    let pr = p.process();

    // If the ioctl involves modification, hang if in the background.
    if matches!(
        cmd,
        FIOSETOWN
            | TIOCFLUSH
            | TIOCDRAIN
            | TIOCSBRK
            | TIOCCBRK
            | TIOCSETA
            | TIOCSETD
            | TIOCSETAF
            | TIOCSETAW
            | TIOCSPGRP
            | TIOCSTAT
            | TIOCSWINSZ
    ) {
        while isbackground(pr, tp)
            && pr.ps_flags.load(Ordering::Relaxed) & PS_PPWAIT == 0
            && !sigismasked(p, SIGTTOU)
        {
            // SAFETY: a process in the background of a tty is in a process group.
            let pgrp = unsafe { &*pr.ps_pgrp.get() };
            if pgrp.pg_jobc.get() == 0 {
                return Err(Errno::EIO);
            }
            pgsignal(Some(pgrp), SIGTTOU, true);
            ttysleep_nsec(tp, nowake(), TTOPRI | PCATCH, TTYBG, sec_to_nsec(1))?;
        }
    }

    match cmd {
        // Process the ioctl.
        FIOASYNC => {
            // set/clear async i/o
            let s = spltty();
            if ioctl_arg::<i32>(data) != 0 {
                tp.t_state_set(TS_ASYNC);
            } else {
                tp.t_state_clr(TS_ASYNC);
            }
            splx(s);
        }
        FIONREAD => {
            // get # bytes to read
            let s = spltty();
            ioctl_ret(data, &ttnread(tp));
            splx(s);
        }
        TIOCEXCL => {
            // set exclusive use of tty
            let s = spltty();
            tp.t_state_set(TS_XCLUDE);
            splx(s);
        }
        TIOCFLUSH => {
            // flush buffers
            let mut flags = ioctl_arg::<i32>(data);
            if flags == 0 {
                flags = FREAD | FWRITE;
            } else {
                flags &= FREAD | FWRITE;
            }
            ttyflush(tp, flags);
        }
        TIOCCONS => {
            // become virtual console
            if ioctl_arg::<i32>(data) != 0 {
                if let Some(cons) = constty()
                    && !ptr::eq(cons, tp)
                    && cons.t_state.get() & (TS_CARR_ON | TS_ISOPEN) == (TS_CARR_ON | TS_ISOPEN)
                {
                    return Err(Errno::EBUSY);
                }

                // ensure user can open the real console
                let mut nid = ndinit(LOOKUP, FOLLOW, NiDirp::Sys(b"/dev/console"), p);
                nid.ni_pledge = PLEDGE_RPATH | PLEDGE_WPATH;
                nid.ni_unveil = UNVEIL_READ | UNVEIL_WRITE;
                namei(&mut nid)?;
                let Some(vp) = nid.ni_vp else {
                    return Err(Errno::ENOENT);
                };
                let _ = vn_lock(vp, LK_EXCLUSIVE | LK_RETRY);
                let error = VOP_ACCESS(vp, VREAD, p.p_ucred.get(), p);
                let _ = VOP_UNLOCK(vp);
                vrele(vp);
                error?;

                set_constty(Some(tp));
            } else if constty().is_some_and(|c| ptr::eq(c, tp)) {
                set_constty(None);
            }
        }
        TIOCDRAIN => {
            // wait till output drained
            ttywait(tp)?;
        }
        TIOCGETA => {
            // get termios struct
            ioctl_ret(data, &tp.t_termios.get());
        }
        TIOCGETD => {
            // get line discipline
            ioctl_ret(data, &i32::from(tp.t_line.get()));
        }
        TIOCGWINSZ => {
            // get window size
            ioctl_ret(data, &tp.t_winsize.get());
        }
        TIOCGTSTAMP => {
            let s = spltty();
            ioctl_ret(data, &tp.t_tv.get());
            splx(s);
        }
        FIOGETOWN => {
            // get pgrp of tty
            if !isctty(pr, tp) && suser(p).is_err() {
                return Err(Errno::ENOTTY);
            }
            ioctl_ret(data, &tp.pgrp().map_or(0, |pg| -pg.pg_id.get()));
        }
        TIOCGPGRP => {
            // get pgrp of tty
            if !isctty(pr, tp) && suser(p).is_err() {
                return Err(Errno::ENOTTY);
            }
            ioctl_ret(data, &tp.pgrp().map_or(NO_PID, |pg| pg.pg_id.get()));
        }
        TIOCGSID => {
            // get sid of tty
            if !isctty(pr, tp) {
                return Err(Errno::ENOTTY);
            }
            // SAFETY: a session leader stays allocated while it leads the session.
            let leader = tp
                .session()
                .and_then(|s| unsafe { s.s_leader.get().as_ref() });
            let Some(leader) = leader else {
                // XXX session stored wrong
                return Err(Errno::ENOTTY);
            };
            ioctl_ret(data, &leader.ps_pid.get());
        }
        TIOCNXCL => {
            // reset exclusive use of tty
            let s = spltty();
            tp.t_state_clr(TS_XCLUDE);
            splx(s);
        }
        TIOCOUTQ => {
            // output queue size
            ioctl_ret(data, &tp.t_outq.c_cc.get());
        }
        TIOCSETA | TIOCSETAW | TIOCSETAF => {
            // set termios struct / drain output, set / drn out, fls in, set
            let mut t: Termios = ioctl_arg(data);

            let s = spltty();
            if cmd == TIOCSETAW || cmd == TIOCSETAF {
                if let Err(e) = ttywait(tp) {
                    splx(s);
                    return Err(e);
                }
                if cmd == TIOCSETAF {
                    ttyflush(tp, FREAD);
                }
            }
            if t.c_cflag & CIGNORE == 0 {
                // Some minor validation is necessary.
                if t.c_ispeed < 0 || t.c_ospeed < 0 {
                    splx(s);
                    return Err(Errno::EINVAL);
                }
                // Set device hardware.
                let error = match tp.t_param.get() {
                    Some(param) => param(tp, &t),
                    None => Ok(()),
                };
                if let Err(e) = error {
                    splx(s);
                    return Err(e);
                }
                if !tp.t_state_isset(TS_CARR_ON)
                    && tp.t_cflag() & CLOCAL != 0
                    && t.c_cflag & CLOCAL == 0
                {
                    tp.t_state_clr(TS_ISOPEN);
                    tp.t_state_set(TS_WOPEN);
                    ttwakeup(tp);
                }
                tp.set_t_cflag(t.c_cflag);
                tp.set_t_ispeed(t.c_ispeed);
                tp.set_t_ospeed(t.c_ospeed);
                if t.c_ospeed == 0
                    && let Some(sess) = tp.session()
                    // SAFETY: a session leader stays allocated while it leads the session.
                    && let Some(leader) = unsafe { sess.s_leader.get().as_ref() }
                {
                    prsignal(leader, SIGHUP);
                }
                ttsetwater(tp);
            }
            if cmd != TIOCSETAF && t.c_lflag & ICANON != tp.t_lflag() & ICANON {
                if t.c_lflag & ICANON != 0 {
                    tp.t_lflag_set(PENDIN);
                    ttwakeup(tp);
                } else {
                    catq(&tp.t_rawq, &tp.t_canq);
                    tp.t_rawq.swap(&tp.t_canq);
                    tp.t_lflag_clr(PENDIN);
                }
            }
            tp.set_t_iflag(t.c_iflag);
            tp.set_t_oflag(t.c_oflag);
            // Make the EXTPROC bit read only.
            if tp.t_lflag() & EXTPROC != 0 {
                t.c_lflag |= EXTPROC;
            } else {
                t.c_lflag &= !EXTPROC;
            }
            tp.set_t_lflag(t.c_lflag | (tp.t_lflag() & PENDIN));
            tp.set_t_cc_all(t.c_cc);
            splx(s);
        }
        TIOCSETD => {
            // set line discipline
            let t = ioctl_arg::<i32>(data);
            let device = tp.t_dev.get();

            if t as u32 >= NLINESW as u32 {
                return Err(Errno::ENXIO);
            }
            if t != i32::from(tp.t_line.get()) {
                let s = spltty();
                let _ = (linesw(tp).l_close)(tp, flag, Some(p));
                if let Err(e) = (LINESW[t as usize].l_open)(device, tp, p) {
                    let _ = (linesw(tp).l_open)(device, tp, p);
                    splx(s);
                    return Err(e);
                }
                tp.t_line.set(t as u8);
                splx(s);
            }
        }
        TIOCSTART => {
            // start output, like ^Q
            let s = spltty();
            if tp.t_state_isset(TS_TTSTOP) || tp.t_lflag() & FLUSHO != 0 {
                tp.t_lflag_clr(FLUSHO);
                tp.t_state_clr(TS_TTSTOP);
                ttstart(tp);
            }
            splx(s);
        }
        TIOCSTOP => {
            // stop output, like ^S
            let s = spltty();
            if !tp.t_state_isset(TS_TTSTOP) {
                tp.t_state_set(TS_TTSTOP);
                d_stop(tp, 0);
            }
            splx(s);
        }
        TIOCSCTTY => {
            // become controlling tty
            // Session ctty vnode pointer set in vnode layer.
            // SAFETY: a process's session lives while the process is in it.
            let Some(sess) = (unsafe { pr.session().as_ref() }) else {
                return Err(Errno::EPERM);
            };
            if !sess_leader(pr)
                || ((!sess.s_ttyvp.get().is_null() || !tp.t_session.get().is_null())
                    && !ptr::eq(tp.t_session.get(), sess))
            {
                return Err(Errno::EPERM);
            }
            if let Some(old) = tp.session() {
                sessrele(old);
            }
            sesshold(sess);
            tp.t_session.set(sess);
            tp.t_pgrp.set(pr.ps_pgrp.get());
            sess.s_ttyp.set(tp);
            pr.ps_flags.fetch_or(PS_CONTROLT, Ordering::SeqCst);
        }
        FIOSETOWN => {
            // set pgrp of tty
            let pgid = ioctl_arg::<i32>(data);

            if !isctty(pr, tp) {
                return Err(Errno::ENOTTY);
            }
            let pgrp = if pgid < 0 {
                pgfind(-pgid)
            } else {
                let Some(pr1) = prfind(pgid) else {
                    return Err(Errno::ESRCH);
                };
                // SAFETY: a live process is in a live process group.
                unsafe { pr1.ps_pgrp.get().as_ref() }
            };
            match pgrp {
                None => return Err(Errno::EINVAL),
                Some(pgrp) if !ptr::eq(pgrp.pg_session.get(), pr.session()) => {
                    return Err(Errno::EPERM);
                }
                Some(pgrp) => tp.t_pgrp.set(pgrp),
            }
        }
        TIOCSPGRP => {
            // set pgrp of tty
            let pgrp = pgfind(ioctl_arg::<i32>(data));

            if !isctty(pr, tp) {
                return Err(Errno::ENOTTY);
            }
            match pgrp {
                None => return Err(Errno::EINVAL),
                Some(pgrp) if !ptr::eq(pgrp.pg_session.get(), pr.session()) => {
                    return Err(Errno::EPERM);
                }
                Some(pgrp) => tp.t_pgrp.set(pgrp),
            }
        }
        TIOCSTAT => {
            // get load avg stats
            ttyinfo(tp);
        }
        TIOCSWINSZ => {
            // set window size
            let ws: Winsize = ioctl_arg(data);
            if tp.t_winsize.get() != ws {
                tp.t_winsize.set(ws);
                pgsignal(tp.pgrp(), SIGWINCH, true);
            }
        }
        TIOCSTSTAMP => {
            let ts: Tstamps = ioctl_arg(data);

            let s = spltty();
            let mut flags = tp.t_flags.get();
            flags &= !(TS_TSTAMPDCDSET | TS_TSTAMPCTSSET | TS_TSTAMPDCDCLR | TS_TSTAMPCTSCLR);
            if ts.ts_set & TIOCM_CAR != 0 {
                flags |= TS_TSTAMPDCDSET;
            }
            if ts.ts_set & TIOCM_CTS != 0 {
                flags |= TS_TSTAMPCTSSET;
            }
            if ts.ts_clr & TIOCM_CAR != 0 {
                flags |= TS_TSTAMPDCDCLR;
            }
            if ts.ts_clr & TIOCM_CTS != 0 {
                flags |= TS_TSTAMPCTSCLR;
            }
            tp.t_flags.set(flags);
            splx(s);
        }
        _ => return Ok(false),
    }
    Ok(true)
}

// ttyread_filtops, ttywrite_filtops, ttyexcept_filtops and filt_ttyrdetach, filt_ttyread,
// filt_ttywdetach, filt_ttywrite, filt_ttyexcept: struct knote (kern_event.c).

/// `ttkqfilter`: attaches a kqueue filter to a tty (`kern_event.c` is not ported).
pub fn ttkqfilter(dev: Dev, _kn: &Knote) -> Result<(), Errno> {
    let _tp = cdevsw(major(dev)).d_tty.and_then(|t| t(dev));
    Err(unported!("ttkqfilter: klist_insert_locked (kern_event.c)"))
}

/// `ttnread`: the characters a read would return now.
fn ttnread(tp: &Tty) -> i32 {
    splassert(IPL_TTY, "ttnread");

    if tp.t_lflag() & PENDIN != 0 {
        ttypend(tp);
    }
    let mut nread = tp.t_canq.c_cc.get();
    if tp.t_lflag() & ICANON == 0 {
        nread += tp.t_rawq.c_cc.get();
        if nread < i32::from(tp.t_cc(VMIN)) && tp.t_cc(VTIME) == 0 {
            nread = 0;
        }
    }
    nread
}

/// `ttywait_nsec`: wait for output to drain, or if this times out, flush it.
pub fn ttywait_nsec(tp: &Tty, nsecs: u64) -> Result<(), Errno> {
    let mut error = Ok(());
    let s = spltty();
    let draining = |tp: &Tty| {
        (tp.t_outq.c_cc.get() != 0 || tp.t_state_isset(TS_BUSY))
            && (tp.t_state_isset(TS_CARR_ON) || tp.t_cflag() & CLOCAL != 0)
            && tp.t_oproc.get().is_some()
    };
    while draining(tp) {
        if let Some(oproc) = tp.t_oproc.get() {
            oproc(tp);
        }
        if draining(tp) {
            tp.t_state_set(TS_ASLEEP);
            error = ttysleep_nsec(
                tp,
                ptr::from_ref(&tp.t_outq).cast(),
                TTOPRI | PCATCH,
                TTYOUT,
                nsecs,
            );
            if error == Err(Errno::EWOULDBLOCK) {
                ttyflush(tp, FWRITE);
            }
            if error.is_err() {
                break;
            }
        } else {
            break;
        }
    }
    splx(s);
    error
}

/// `ttywait`: wait for output to drain.
pub fn ttywait(tp: &Tty) -> Result<(), Errno> {
    ttywait_nsec(tp, INFSLP)
}

/// `ttywflush`: flush if successfully wait.
pub fn ttywflush(tp: &Tty) -> Result<(), Errno> {
    let error = ttywait_nsec(tp, sec_to_nsec(5));
    if error.is_ok() || error == Err(Errno::EWOULDBLOCK) {
        ttyflush(tp, FREAD);
    }
    error
}

/// `ttyflush`: flush tty read and/or write queues, notifying anyone waiting.
pub fn ttyflush(tp: &Tty, rw: i32) {
    let s = spltty();
    if rw & FREAD != 0 {
        flushq(&tp.t_canq);
        flushq(&tp.t_rawq);
        tp.t_rocount.set(0);
        tp.t_rocol.set(0);
        tp.t_state_clr(TS_LOCAL);
        ttyunblock(tp);
        ttwakeup(tp);
    }
    if rw & FWRITE != 0 {
        tp.t_state_clr(TS_TTSTOP);
        d_stop(tp, rw);
        flushq(&tp.t_outq);
        wakeup(ptr::from_ref(&tp.t_outq));
        selwakeup(&tp.t_wsel);
    }
    splx(s);
}

/// `ttychars`: copy in the default termios characters.
pub fn ttychars(tp: &Tty) {
    tp.set_t_cc_all(TTYDEFCHARS);
}

/// `ttyblock`: send stop character on input overflow.
fn ttyblock(tp: &Tty) {
    let total = tp.t_rawq.c_cc.get() + tp.t_canq.c_cc.get();
    if tp.t_rawq.c_cc.get() > ttyhog(tp) {
        ttyflush(tp, FREAD | FWRITE);
        tp.t_state_clr(TS_TBLOCK);
    }
    // Block further input iff: current input > threshold AND input is available to user
    // program.
    if (total >= ttyhog(tp) / 2 && !tp.t_state_isset(TS_TBLOCK) && tp.t_lflag() & ICANON == 0)
        || tp.t_canq.c_cc.get() > 0
    {
        if tp.t_iflag() & IXOFF != 0
            && tp.t_cc(VSTOP) != _POSIX_VDISABLE
            && putc(i32::from(tp.t_cc(VSTOP)), &tp.t_outq) == 0
        {
            tp.t_state_set(TS_TBLOCK);
            ttstart(tp);
        }
        // Try to block remote output via hardware flow control.
        if tp.t_cflag() & CHWFLOW != 0
            && let Some(hwiflow) = tp.t_hwiflow.get()
            && hwiflow(tp, 1) != 0
        {
            tp.t_state_set(TS_TBLOCK);
        }
    }
}

/// `ttrstrt`: the `t_rstrt_to` timeout: restart output after a delay.
pub fn ttrstrt(arg: *mut c_void) {
    #[cfg(feature = "diagnostic")]
    if arg.is_null() {
        panic(format_args!("ttrstrt"));
    }
    // SAFETY: `ttymalloc` armed the timeout with its tty, which lives until `ttyfree`
    // (which runs after the timeout can no longer fire).
    let tp = unsafe { &*arg.cast::<Tty>() };
    let s = spltty();
    tp.t_state_clr(TS_TIMEOUT);
    ttstart(tp);
    splx(s);
}

/// `ttstart`: start output through the driver's `t_oproc`.
pub fn ttstart(tp: &Tty) -> i32 {
    if let Some(oproc) = tp.t_oproc.get() {
        // XXX: Kludge for pty.
        oproc(tp);
    }
    0
}

/// `ttylclose`: "close" a line discipline.
pub fn ttylclose(tp: &Tty, flag: i32, _p: Option<&Proc>) -> Result<(), Errno> {
    if flag & FNONBLOCK != 0 {
        ttyflush(tp, FREAD | FWRITE);
    } else {
        let _ = ttywflush(tp);
    }
    Ok(())
}

/// `ttymodem`: handle modem control transition on a tty. Flag indicates new state of
/// carrier. Returns 0 if the line should be turned off, otherwise 1.
pub fn ttymodem(tp: &Tty, flag: i32) -> i32 {
    if !tp.t_state_isset(TS_WOPEN) && tp.t_cflag() & MDMBUF != 0 {
        // MDMBUF: do flow control according to carrier flag
        if flag != 0 {
            tp.t_state_clr(TS_TTSTOP);
            ttstart(tp);
        } else if !tp.t_state_isset(TS_TTSTOP) {
            tp.t_state_set(TS_TTSTOP);
            d_stop(tp, 0);
        }
    } else if flag == 0 {
        // Lost carrier.
        tp.t_state_clr(TS_CARR_ON);
        if tp.t_state_isset(TS_ISOPEN) && tp.t_cflag() & CLOCAL == 0 {
            hangup_leader(tp);
            ttyflush(tp, FREAD | FWRITE);
            return 0;
        }
    } else {
        // Carrier now on.
        tp.t_state_set(TS_CARR_ON);
        ttwakeup(tp);
    }
    1
}

/// `if (tp->t_session && tp->t_session->s_leader) prsignal(tp->t_session->s_leader,
/// SIGHUP)`.
fn hangup_leader(tp: &Tty) {
    if let Some(sess) = tp.session()
        // SAFETY: a session leader stays allocated while it leads the session.
        && let Some(leader) = unsafe { sess.s_leader.get().as_ref() }
    {
        prsignal(leader, SIGHUP);
    }
}

/// `nullmodem`: default modem control routine (for other line disciplines). Return argument
/// flag, to turn off device on carrier drop.
pub fn nullmodem(tp: &Tty, flag: i32) -> i32 {
    if flag != 0 {
        tp.t_state_set(TS_CARR_ON);
    } else {
        tp.t_state_clr(TS_CARR_ON);
        if tp.t_state_isset(TS_ISOPEN) && tp.t_cflag() & CLOCAL == 0 {
            hangup_leader(tp);
            ttyflush(tp, FREAD | FWRITE);
            return 0;
        }
    }
    1
}

/// `ttypend`: reinput pending characters after state switch; call at `spltty()`.
pub fn ttypend(tp: &Tty) {
    splassert(IPL_TTY, "ttypend");

    tp.t_lflag_clr(PENDIN);
    tp.t_state_set(TS_TYPEN);
    let tq = tp.t_rawq.copy_header();
    tp.t_rawq.c_cc.set(0);
    tp.t_rawq.c_cf.set(0);
    tp.t_rawq.c_cl.set(0);
    loop {
        let c = getc(&tq);
        if c < 0 {
            break;
        }
        ttyinput(c, tp);
    }
    tp.t_state_clr(TS_TYPEN);
}

/// `ttvtimeout`: the `VTIME` timer of a non-canonical read.
fn ttvtimeout(arg: *mut c_void) {
    // SAFETY: `ttread` armed the timeout with its tty and deletes it before returning.
    let tp = unsafe { &*arg.cast::<Tty>() };
    wakeup(ptr::from_ref(&tp.t_rawq));
}

/// What `ttread`'s queue checks decided.
enum Wait<'q> {
    /// `goto read` with this queue.
    Read(&'q Clist),
    /// `goto sleep`.
    Sleep,
}

/// `ttread`: process a read call on a tty device.
pub fn ttread(tp: &Tty, uio: &mut Uio<'_>, flag: i32) -> Result<(), Errno> {
    let stime_storage = Timeout::zeroed();
    let mut stime: Option<&Timeout> = None;
    let p = curproc();
    let mut last_cc = 0;

    let error = 'out: loop {
        // loop:
        let lflag = tp.t_lflag();
        let s = spltty();
        // take pending input first
        if lflag & PENDIN != 0 {
            ttypend(tp);
        }
        splx(s);

        // Hang process if it's in the background.
        if let Some(p) = p
            && isbackground(p.process(), tp)
        {
            let pr = p.process();
            // SAFETY: a process in the background of a tty is in a process group.
            let pgrp = unsafe { &*pr.ps_pgrp.get() };
            if sigismasked(p, SIGTTIN)
                || pr.ps_flags.load(Ordering::Relaxed) & PS_PPWAIT != 0
                || pgrp.pg_jobc.get() == 0
            {
                break 'out Err(Errno::EIO);
            }
            pgsignal(Some(pgrp), SIGTTIN, true);
            if let Err(e) = ttysleep_nsec(tp, nowake(), TTIPRI | PCATCH, TTYBG, sec_to_nsec(1)) {
                break 'out Err(e);
            }
            continue 'out;
        }

        let s = spltty();
        let cc = tp.t_cc_all();
        let wait = if lflag & ICANON == 0 {
            let min = i32::from(cc[VMIN as usize]);
            let time = u64::from(cc[VTIME as usize]) * 100; // tenths of a second (ms)

            let qp = &tp.t_rawq;
            // Check each of the four combinations. (min > 0 && time == 0) is the normal
            // read case. It should be fairly efficient, so we check that and its companion
            // case (min == 0 && time == 0) first.
            'nc: {
                if time == 0 {
                    if qp.c_cc.get() < min {
                        break 'nc Wait::Sleep;
                    }
                    break 'nc Wait::Read(qp);
                }
                if min > 0 {
                    if qp.c_cc.get() <= 0 {
                        break 'nc Wait::Sleep;
                    }
                    if qp.c_cc.get() >= min {
                        break 'nc Wait::Read(qp);
                    }
                    match stime {
                        None => {
                            // alloc_timer:
                            timeout_set(
                                &stime_storage,
                                ttvtimeout,
                                ptr::from_ref(tp).cast_mut().cast(),
                            );
                            timeout_add_msec(&stime_storage, time);
                            stime = Some(&stime_storage);
                        }
                        Some(st) => {
                            if qp.c_cc.get() > last_cc {
                                // got a character, restart timer
                                timeout_add_msec(st, time);
                            }
                        }
                    }
                } else {
                    // min == 0
                    if qp.c_cc.get() > 0 {
                        break 'nc Wait::Read(qp);
                    }
                    if stime.is_none() {
                        timeout_set(
                            &stime_storage,
                            ttvtimeout,
                            ptr::from_ref(tp).cast_mut().cast(),
                        );
                        timeout_add_msec(&stime_storage, time);
                        stime = Some(&stime_storage);
                    }
                }
                last_cc = qp.c_cc.get();
                if stime.is_some_and(|st| !timeout_triggered(st)) {
                    break 'nc Wait::Sleep;
                }
                Wait::Read(qp)
            }
        } else if tp.t_canq.c_cc.get() <= 0 {
            Wait::Sleep
        } else {
            Wait::Read(&tp.t_canq)
        };

        let qp = match wait {
            Wait::Sleep => {
                // If there is no input, sleep on rawq awaiting hardware receipt and
                // notification. If we have data, we don't need to check for carrier.
                let carrier = tp.t_state_isset(TS_CARR_ON) || tp.t_cflag() & CLOCAL != 0;
                if !carrier && tp.t_state_isset(TS_ISOPEN) {
                    splx(s);
                    break 'out Ok(());
                }
                if flag & IO_NDELAY != 0 {
                    splx(s);
                    break 'out Err(Errno::EWOULDBLOCK);
                }
                let mut error = ttysleep(
                    tp,
                    ptr::from_ref(&tp.t_rawq).cast(),
                    TTIPRI | PCATCH,
                    if carrier { TTYIN } else { TTOPEN },
                );
                splx(s);
                if stime.is_some_and(timeout_triggered) {
                    error = Err(Errno::EWOULDBLOCK);
                }
                if tp.t_cc(VMIN) == 0 && error == Err(Errno::EWOULDBLOCK) {
                    break 'out Ok(());
                }
                if let Err(e) = error
                    && e != Errno::EWOULDBLOCK
                {
                    break 'out Err(e);
                }
                continue 'out;
            }
            Wait::Read(qp) => qp,
        };

        // read:
        splx(s);

        // Input present, check for input mapping and processing.
        let mut first = true;
        let mut error = Ok(());
        let mut again = false;
        loop {
            let c = getc(qp);
            if c < 0 {
                break;
            }
            // delayed suspend (^Y)
            if cceq(cc[VDSUSP as usize], c) && lflag & (IEXTEN | ISIG) == (IEXTEN | ISIG) {
                pgsignal(tp.pgrp(), SIGTSTP, true);
                if first {
                    error = ttysleep_nsec(tp, nowake(), TTIPRI | PCATCH, TTYBG, sec_to_nsec(1));
                    if error.is_err() {
                        break;
                    }
                    again = true;
                }
                break;
            }
            // Interpret EOF only in canonical mode.
            if cceq(cc[VEOF as usize], c) && lflag & ICANON != 0 {
                break;
            }
            // Give user character.
            error = ureadc(c, uio);
            if error.is_err() {
                break;
            }
            if uio.uio_resid == 0 {
                break;
            }
            // In canonical mode check for a "break character" marking the end of a "line
            // of input".
            if lflag & ICANON != 0 && ttbreakc(c, &cc, lflag) {
                break;
            }
            first = false;
        }
        if again {
            continue 'out;
        }
        // Look to unblock output now that (presumably) the input queue has gone down.
        let s = spltty();
        if tp.t_rawq.c_cc.get() < ttyhog(tp) / 5 {
            ttyunblock(tp);
        }
        splx(s);
        break 'out error;
    };

    // out:
    if let Some(st) = stime {
        timeout_del(st);
    }
    error
}

/// `ttyunblock`: call at `spltty`.
pub fn ttyunblock(tp: &Tty) {
    splassert(IPL_TTY, "ttyunblock");

    if tp.t_state_isset(TS_TBLOCK) {
        if tp.t_iflag() & IXOFF != 0
            && tp.t_cc(VSTART) != _POSIX_VDISABLE
            && putc(i32::from(tp.t_cc(VSTART)), &tp.t_outq) == 0
        {
            tp.t_state_clr(TS_TBLOCK);
            ttstart(tp);
        }
        // Try to unblock remote output via hardware flow control.
        if tp.t_cflag() & CHWFLOW != 0
            && let Some(hwiflow) = tp.t_hwiflow.get()
            && hwiflow(tp, 0) != 0
        {
            tp.t_state_clr(TS_TBLOCK);
        }
    }
}

/// `ttycheckoutq`: check the output queue on `tp` for space for a kernel message (from
/// `uprintf` or `tprintf`). Allow some space over the normal hiwater mark so we don't lose
/// messages due to normal flow control, but don't let the tty run amok. Sleeps here are not
/// interruptible, but we return prematurely if new signals arrive.
pub fn ttycheckoutq(tp: &Tty, wait: bool) -> bool {
    let hiwat = i32::from(tp.t_hiwat.get());
    let s = spltty();
    let p = curproc();
    let oldsig = match p {
        Some(p) if wait => sigpending(p),
        _ => 0,
    };
    if tp.t_outq.c_cc.get() > hiwat + TTHIWATMINSPACE {
        while tp.t_outq.c_cc.get() > hiwat {
            ttstart(tp);
            if !wait || p.is_some_and(|p| sigpending(p) != oldsig) {
                splx(s);
                return false;
            }
            tp.t_state_set(TS_ASLEEP);
            let _ = tsleep_nsec(
                ptr::from_ref(&tp.t_outq),
                PZERO - 1,
                "ttckoutq",
                sec_to_nsec(1),
            );
        }
    }
    splx(s);
    true
}

/// How `ttwrite`'s main loop ended.
enum WriteExit {
    /// `goto out`: give back what was copied in but not queued.
    Out(Result<(), Errno>),
    /// `goto done`.
    Done(Result<(), Errno>),
}

/// `ttwrite`: process a write call on a tty device.
pub fn ttwrite(tp: &Tty, uio: &mut Uio<'_>, flag: i32) -> Result<(), Errno> {
    let mut obuf = [0u8; OBUFSIZ];
    let mut cp = 0usize;
    let mut cc = 0usize;
    let mut obufcc = 0usize;

    let hiwat = i32::from(tp.t_hiwat.get());
    let cnt = uio.uio_resid;

    let exit = 'top: loop {
        // loop:
        let s = spltty();
        if !tp.t_state_isset(TS_CARR_ON) && tp.t_cflag() & CLOCAL == 0 {
            if tp.t_state_isset(TS_ISOPEN) {
                splx(s);
                break 'top WriteExit::Done(Err(Errno::EIO));
            } else if flag & IO_NDELAY != 0 {
                splx(s);
                break 'top WriteExit::Out(Err(Errno::EWOULDBLOCK));
            } else {
                // Sleep awaiting carrier.
                let error = ttysleep(
                    tp,
                    ptr::from_ref(&tp.t_rawq).cast(),
                    TTIPRI | PCATCH,
                    TTOPEN,
                );
                splx(s);
                if let Err(e) = error {
                    break 'top WriteExit::Out(Err(e));
                }
                continue 'top;
            }
        }
        splx(s);
        // Hang the process if it's in the background.
        if let Some(p) = curproc() {
            let pr = p.process();
            if isbackground(pr, tp)
                && tp.t_lflag() & TOSTOP != 0
                && pr.ps_flags.load(Ordering::Relaxed) & PS_PPWAIT == 0
                && !sigismasked(p, SIGTTOU)
            {
                // SAFETY: a process in the background of a tty is in a process group.
                let pgrp = unsafe { &*pr.ps_pgrp.get() };
                if pgrp.pg_jobc.get() == 0 {
                    break 'top WriteExit::Out(Err(Errno::EIO));
                }
                pgsignal(Some(pgrp), SIGTTOU, true);
                if let Err(e) = ttysleep_nsec(tp, nowake(), TTIPRI | PCATCH, TTYBG, sec_to_nsec(1))
                {
                    break 'top WriteExit::Out(Err(e));
                }
                continue 'top;
            }
        }
        // Process the user's data in at most OBUFSIZ chunks. Perform any output
        // translation. Keep track of high water mark, sleep on overflow awaiting device aid
        // in acquiring new space.
        let mut error = Ok(());
        let mut ovhiwat = false;
        'chunks: while uio.uio_resid > 0 || cc > 0 {
            if tp.t_lflag() & FLUSHO != 0 {
                uio.uio_resid = 0;
                break 'top WriteExit::Done(Ok(()));
            }
            if tp.t_outq.c_cc.get() > hiwat {
                ovhiwat = true;
                break 'chunks;
            }
            // Grab a hunk of data from the user, unless we have some leftover from last
            // time.
            if cc == 0 {
                cc = uio.uio_resid.min(OBUFSIZ);
                cp = 0;
                if let Err(e) = uiomove(&mut obuf[..cc], uio) {
                    error = Err(e);
                    cc = 0;
                    break 'chunks;
                }
                if cc > obufcc {
                    obufcc = cc;
                }

                // duplicate /dev/console output into console buffer
                if let Some(cbp) = consbufp()
                    && cn_tab().is_some_and(|cn| cn.cn_dev.get() == tp.t_dev.get())
                    && tp.t_gen.get() == 0
                {
                    for &c in &obuf[..cc] {
                        if c != b'\0' && c != b'\r' && c != 0o177 {
                            msgbuf_putchar(cbp, c);
                        }
                    }
                }
            }
            // If nothing fancy need be done, grab those characters we can handle without
            // any of ttyoutput's processing and just transfer them to the output q. For
            // those chars which require special processing (as indicated by the bits in
            // char_type), call ttyoutput. After processing a hunk of data, look for FLUSHO
            // so ^O's will take effect immediately.
            while cc > 0 {
                let mut ce = if tp.t_oflag() & OPOST == 0 {
                    cc
                } else {
                    let ce = cc - scanc(&obuf[cp..cp + cc], &CHAR_TYPE, CCLASSMASK);
                    // If ce is zero, then we're processing a special character through
                    // ttyoutput.
                    if ce == 0 {
                        tp.t_rocount.set(0);
                        if ttyoutput(i32::from(obuf[cp]), tp) >= 0 {
                            // out of space
                            ovhiwat = true;
                            break 'chunks;
                        }
                        cp += 1;
                        cc -= 1;
                        if tp.t_lflag() & FLUSHO != 0 || tp.t_outq.c_cc.get() > hiwat {
                            ovhiwat = true;
                            break 'chunks;
                        }
                        continue;
                    }
                    ce
                };
                // A bunch of normal characters have been found. Transfer them en masse to
                // the output queue and continue processing at the top of the loop. If there
                // are any further characters in this <= OBUFSIZ chunk, the first should be a
                // character requiring special handling by ttyoutput.
                tp.t_rocount.set(0);
                let i = b_to_q(&obuf[cp..cp + ce], &tp.t_outq);
                ce -= i;
                tp.t_column.set(tp.t_column.get().wrapping_add(ce as i16));
                cp += ce;
                cc -= ce;
                TK_NOUT.fetch_add(ce as i64, Ordering::Relaxed);
                tp.t_outcc.set(tp.t_outcc.get() + ce as i64);
                if i > 0 {
                    // out of space
                    ovhiwat = true;
                    break 'chunks;
                }
                if tp.t_lflag() & FLUSHO != 0 || tp.t_outq.c_cc.get() > hiwat {
                    break;
                }
            }
            ttstart(tp);
        }
        if !ovhiwat {
            break 'top WriteExit::Out(error);
        }

        // ovhiwat:
        ttstart(tp);
        let s = spltty();
        // This can only occur if FLUSHO is set in t_lflag, or if ttstart/oproc is
        // synchronous (or very fast).
        if tp.t_outq.c_cc.get() <= hiwat {
            splx(s);
            continue 'top;
        }
        if flag & IO_NDELAY != 0 {
            splx(s);
            uio.uio_resid += cc;
            if obufcc != 0 {
                explicit_bzero(&mut obuf[..obufcc]);
            }
            return if uio.uio_resid == cnt {
                Err(Errno::EWOULDBLOCK)
            } else {
                Ok(())
            };
        }
        tp.t_state_set(TS_ASLEEP);
        let error = ttysleep(
            tp,
            ptr::from_ref(&tp.t_outq).cast(),
            TTOPRI | PCATCH,
            TTYOUT,
        );
        splx(s);
        if let Err(e) = error {
            break 'top WriteExit::Out(Err(e));
        }
    };

    let error = match exit {
        WriteExit::Out(error) => {
            // If cc is nonzero, we leave the uio structure inconsistent, as the offset and
            // iov pointers have moved forward, but it doesn't matter (the call will either
            // return short or restart with a new uio).
            uio.uio_resid += cc;
            error
        }
        WriteExit::Done(error) => error,
    };
    // done:
    if obufcc != 0 {
        explicit_bzero(&mut obuf[..obufcc]);
    }
    error
}

/// `ttyrub`: rubout one character from the rawq of tp as cleanly as possible.
pub fn ttyrub(c: i32, tp: &Tty) -> i32 {
    let mut c = c;
    if tp.t_lflag() & ECHO == 0 || tp.t_lflag() & EXTPROC != 0 {
        return 0;
    }
    tp.t_lflag_clr(FLUSHO);
    if tp.t_lflag() & ECHOE != 0 {
        if tp.t_rocount.get() == 0 {
            // Screwed by ttwrite; retype
            return ttyretype(tp);
        }
        if c == (i32::from(b'\t') | TTY_QUOTE) || c == (i32::from(b'\n') | TTY_QUOTE) {
            ttyrubo(tp, 2);
        } else {
            c &= TTY_CHARMASK;
            match cclass(c) {
                ORDINARY => ttyrubo(tp, 1),
                BACKSPACE | CONTROL | NEWLINE | RETURN | VTAB => {
                    if tp.t_lflag() & ECHOCTL != 0 {
                        ttyrubo(tp, 2);
                    }
                }
                TAB => {
                    if i32::from(tp.t_rocount.get()) < tp.t_rawq.c_cc.get() {
                        return ttyretype(tp);
                    }
                    let s = spltty();
                    let mut savecol = tp.t_column.get();
                    tp.t_state_set(TS_CNTTB);
                    tp.t_lflag_set(FLUSHO);
                    tp.t_column.set(tp.t_rocol.get());
                    let (mut tabc, mut cc) = (0, 0);
                    let mut cp = firstc(&tp.t_rawq, &mut tabc, &mut cc);
                    while cp.is_some() {
                        ttyecho(tabc, tp);
                        cp = nextc(&tp.t_rawq, cp, &mut tabc, &mut cc);
                    }
                    tp.t_lflag_clr(FLUSHO);
                    tp.t_state_clr(TS_CNTTB);
                    splx(s);

                    // savecol will now be length of the tab.
                    savecol = savecol.wrapping_sub(tp.t_column.get());
                    tp.t_column.set(tp.t_column.get().wrapping_add(savecol));
                    if savecol > 8 {
                        savecol = 8; // overflow screw
                    }
                    loop {
                        savecol -= 1;
                        if savecol < 0 {
                            break;
                        }
                        let _ = ttyoutput(i32::from(b'\x08'), tp);
                    }
                }
                _ => {
                    // XXX
                    printf(format_args!(
                        "ttyrub: would panic c = {}, val = {}\n",
                        c,
                        cclass(c)
                    ));
                }
            }
        }
    } else if tp.t_lflag() & ECHOPRT != 0 {
        if !tp.t_state_isset(TS_ERASE) {
            tp.t_state_set(TS_ERASE);
            let _ = ttyoutput(i32::from(b'\\'), tp);
        }
        ttyecho(c, tp);
    } else {
        ttyecho(i32::from(tp.t_cc(VERASE)), tp);
    }
    tp.t_rocount.set(tp.t_rocount.get().wrapping_sub(1));
    0
}

/// `ttyrubo`: back over `cnt` characters, erasing them.
fn ttyrubo(tp: &Tty, cnt: i32) {
    for _ in 0..cnt {
        let _ = ttyoutput(i32::from(b'\x08'), tp);
        let _ = ttyoutput(i32::from(b' '), tp);
        let _ = ttyoutput(i32::from(b'\x08'), tp);
    }
}

/// `ttyretype`: reprint the rawq line. Note, it is assumed that `c_cc` has already been
/// checked.
pub fn ttyretype(tp: &Tty) -> i32 {
    // Echo the reprint character.
    if tp.t_cc(VREPRINT) != _POSIX_VDISABLE {
        ttyecho(i32::from(tp.t_cc(VREPRINT)), tp);
    }

    let _ = ttyoutput(i32::from(b'\n'), tp);

    let s = spltty();
    for q in [&tp.t_canq, &tp.t_rawq] {
        let (mut c, mut cc) = (0, 0);
        let mut cp = firstc(q, &mut c, &mut cc);
        while cp.is_some() {
            ttyecho(c, tp);
            cp = nextc(q, cp, &mut c, &mut cc);
        }
    }
    tp.t_state_clr(TS_ERASE);
    splx(s);

    tp.t_rocount.set(tp.t_rawq.c_cc.get() as i16);
    tp.t_rocol.set(0);
    1
}

/// `ttyecho`: echo a typed character to the terminal.
fn ttyecho(c: i32, tp: &Tty) {
    let mut c = c;
    if !tp.t_state_isset(TS_CNTTB) {
        tp.t_lflag_clr(FLUSHO);
    }
    let lflag = tp.t_lflag();
    if (lflag & ECHO == 0 && (lflag & ECHONL == 0 || c != i32::from(b'\n'))) || lflag & EXTPROC != 0
    {
        return;
    }
    if (lflag & ECHOCTL != 0
        && (c & TTY_CHARMASK) <= 0o37
        && c != i32::from(b'\t')
        && c != i32::from(b'\n'))
        || (c & TTY_CHARMASK) == 0o177
    {
        let _ = ttyoutput(i32::from(b'^'), tp);
        c &= TTY_CHARMASK;
        if c == 0o177 {
            c = i32::from(b'?');
        } else {
            c += i32::from(b'A') - 1;
        }
    }
    let _ = ttyoutput(c, tp);
}

/// `ttwakeupwr`: wakeup any writers if necessary.
pub fn ttwakeupwr(tp: &Tty) {
    if tp.t_outq.c_cc.get() <= i32::from(tp.t_lowat.get()) {
        if tp.t_state_isset(TS_ASLEEP) {
            tp.t_state_clr(TS_ASLEEP);
            wakeup(ptr::from_ref(&tp.t_outq));
        }
        selwakeup(&tp.t_wsel);
    }
}

/// `ttwakeup`: wake up any readers on a tty.
pub fn ttwakeup(tp: &Tty) {
    selwakeup(&tp.t_rsel);
    if tp.t_state_isset(TS_ASYNC) {
        pgsignal(tp.pgrp(), SIGIO, true);
    }
    wakeup(ptr::from_ref(&tp.t_rawq));
}

/// `ttspeedtab`: look up a code for a specified speed in a conversion table; used by
/// drivers to map software speed values to hardware parameters. -1 when it is not there.
pub fn ttspeedtab(speed: i32, table: &[Speedtab]) -> i32 {
    for entry in table {
        if entry.sp_speed == -1 {
            break;
        }
        if entry.sp_speed == speed {
            return entry.sp_code;
        }
    }
    -1
}

/// `ttsetwater`: set tty hi and low water marks.
///
/// Try to arrange the dynamics so there's about one second from hi to low water.
pub fn ttsetwater(tp: &Tty) {
    /// `CLAMP(x, h, l)`.
    fn clamp(x: i32, h: i32, l: i32) -> i32 {
        if x > h {
            h
        } else if x < l {
            l
        } else {
            x
        }
    }

    let cps = tp.t_ospeed() / 10;
    let mut x = clamp(cps / 2, TTMAXLOWAT, TTMINLOWAT);
    tp.t_lowat.set(x as i16);
    x += cps;
    tp.t_hiwat
        .set(clamp(x, tp.t_outq.c_cn.get() - TTHIWATMINSPACE, TTMINHIWAT) as i16);
}

/// `process_sum`: get the total estcpu for a process, summing across threads. Returns
/// true if at least one thread is runnable/running.
fn process_sum(pr: &Process) -> (bool, Fixpt) {
    let mut ret = false;
    let mut pctcpu: Fixpt = 0;
    for p in pr.ps_threads.iter() {
        if p.p_stat.get() == SRUN || p.p_stat.get() == SONPROC {
            ret = true;
        }
        pctcpu = pctcpu.wrapping_add(p.p_pctcpu.load(Ordering::Relaxed));
    }
    (ret, pctcpu)
}

/// `ttyinfo`: report on state of foreground process group.
pub fn ttyinfo(tp: &Tty) {
    if !ttycheckoutq(tp, false) {
        return;
    }

    // Print load average.
    let tmp =
        (AVERUNNABLE.ldavg[0].load(Ordering::Relaxed) as i64 * 100 + FSCALE as i64 / 2) >> FSHIFT;
    ttyprintf(tp, format_args!("load: {}.{:02} ", tmp / 100, tmp % 100));

    let first = tp.pgrp().and_then(|pg| pg.pg_members.first());
    if tp.t_session.get().is_null() {
        ttyprintf(tp, format_args!("not a controlling terminal\n"));
    } else if tp.t_pgrp.get().is_null() {
        ttyprintf(tp, format_args!("no foreground process group\n"));
    } else if let Some(pr) = first {
        // Pick the most active process:
        //  - prefer at least one running/runnable thread
        //  - prefer higher total pctcpu
        //  - prefer non-zombie
        // Otherwise take the most recently added to this process group
        let mut pickpr = pr;
        let (mut run, mut pctcpu) = process_sum(pickpr);
        let mut next = ListHead::<ProcessPglist>::next(pr);
        while let Some(pr) = next {
            next = ListHead::<ProcessPglist>::next(pr);
            let (run2, pctcpu2) = process_sum(pr);
            if run {
                // pick is running; is p running w/same or more cpu?
                if !(run2 && pctcpu2 >= pctcpu) {
                    continue;
                }
            } else if !(run2 || pctcpu2 > pctcpu) {
                // pick isn't running; is p running *or* w/more cpu? if p has less cpu or is
                // exiting, then it's worse
                if pctcpu2 < pctcpu || pr.ps_flags.load(Ordering::Relaxed) & PS_EXITING != 0 {
                    continue;
                }
            }
            // update_pickpr:
            pickpr = pr;
            run = run2;
            pctcpu = pctcpu2;
        }

        // Calculate percentage cpu, resident set size.
        let calc_pctcpu = ((u64::from(pctcpu) * 10000 + FSCALE as u64 / 2) >> FSHIFT) as i64;
        let mut rss: i64 = 0;
        if pickpr.ps_flags.load(Ordering::Relaxed) & (PS_EMBRYO | PS_EXITING) == 0
            // SAFETY: a live process holds its vmspace.
            && let Some(vm) = unsafe { pickpr.ps_vmspace.get().as_ref() }
        {
            rss = i64::from(vm.vm_rssize.get());
        }

        let tu = Tusage::new();
        tuagg_get_process(&tu, pickpr);
        let (mut utime, mut stime, _) = calctsru(&tu);

        // Round up and print user time.
        utime.tv_nsec += 5_000_000;
        if utime.tv_nsec >= 1_000_000_000 {
            utime.tv_sec += 1;
            utime.tv_nsec -= 1_000_000_000;
        }

        // Round up and print system time.
        stime.tv_nsec += 5_000_000;
        if stime.tv_nsec >= 1_000_000_000 {
            stime.tv_sec += 1;
            stime.tv_nsec -= 1_000_000_000;
        }

        // Find the most active thread:
        //  - prefer runnable
        //  - prefer higher pctcpu
        //  - prefer living
        // Otherwise take the newest thread
        match pickpr.ps_threads.first() {
            None => {
                ttyprintf(tp, format_args!("empty foreground process group\n"));
            }
            Some(p) => {
                let mut pick = p;
                let mut run = p.p_stat.get() == SRUN || p.p_stat.get() == SONPROC;
                let mut pctcpu = p.p_pctcpu.load(Ordering::Relaxed);
                let mut next = TailqHead::<ProcThrLink>::next(p);
                while let Some(p) = next {
                    next = TailqHead::<ProcThrLink>::next(p);
                    let run2 = p.p_stat.get() == SRUN || p.p_stat.get() == SONPROC;
                    let pctcpu2 = p.p_pctcpu.load(Ordering::Relaxed);
                    if run {
                        // pick is running; is p running w/same or more cpu?
                        if !(run2 && pctcpu2 >= pctcpu) {
                            continue;
                        }
                    } else if !(run2 || pctcpu2 > pctcpu)
                        && (pctcpu2 < pctcpu || p.p_flag.load(Ordering::Relaxed) & P_WEXIT != 0)
                    {
                        // pick isn't running; is p running *or* w/more cpu? if p has less
                        // cpu or is exiting, then it's worse
                        continue;
                    }
                    // update_pick:
                    pick = p;
                    run = run2;
                    pctcpu = p.p_pctcpu.load(Ordering::Relaxed);
                }
                let state = if pick.p_stat.get() == SONPROC {
                    "running"
                } else if pick.p_stat.get() == SRUN {
                    "runnable"
                } else {
                    pick.p_wmesg.get().unwrap_or("iowait")
                };

                ttyprintf(
                    tp,
                    format_args!(
                        " cmd: {} {} [{}] {}.{:02}u {}.{:02}s {}% {}k\n",
                        Str(pickpr.comm()),
                        pickpr.ps_pid.get(),
                        state,
                        utime.tv_sec,
                        utime.tv_nsec / 10_000_000,
                        stime.tv_sec,
                        stime.tv_nsec / 10_000_000,
                        calc_pctcpu / 100,
                        rss
                    ),
                );
            }
        }
    } else {
        ttyprintf(tp, format_args!("empty foreground process group\n"));
    }
    tp.t_rocount.set(0); // so pending input will be retyped if BS
}

/// `tputchar`: output char to tty; console putchar style.
pub fn tputchar(c: i32, tp: &Tty) -> i32 {
    let s = spltty();
    if !tp.t_state_isset(TS_ISOPEN) || !(tp.t_state_isset(TS_CARR_ON) || tp.t_cflag() & CLOCAL != 0)
    {
        splx(s);
        return -1;
    }
    if c == i32::from(b'\n') {
        let _ = ttyoutput(i32::from(b'\r'), tp);
    }
    let _ = ttyoutput(c, tp);
    ttstart(tp);
    splx(s);
    0
}

/// `ttysleep`: sleep on `chan`, returning `ERESTART` if tty changed while we napped and
/// returning any errors (e.g. `EINTR`/`ETIMEDOUT`) reported by `tsleep`. If the tty is
/// revoked, restarting a pending call will redo validation done at the start of the call.
pub fn ttysleep(tp: &Tty, chan: *const c_void, pri: i32, wmesg: &'static str) -> Result<(), Errno> {
    ttysleep_nsec(tp, chan, pri, wmesg, INFSLP)
}

/// `ttysleep_nsec`: [`ttysleep`] with a timeout.
pub fn ttysleep_nsec(
    tp: &Tty,
    chan: *const c_void,
    pri: i32,
    wmesg: &'static str,
    nsecs: u64,
) -> Result<(), Errno> {
    let gen_ = tp.t_gen.get();
    tsleep_nsec(chan, pri, wmesg, nsecs)?;
    if tp.t_gen.get() == gen_ {
        Ok(())
    } else {
        Err(Errno::ERESTART)
    }
}

/// `tty_init`: initialise the global tty list.
pub fn tty_init() {
    TTYLIST.0.init();
    TTY_COUNT.store(0, Ordering::Relaxed);
}

/// `ttymalloc`: allocate a tty structure and its associated buffers, and attach it to the
/// tty list.
pub fn ttymalloc(baud: i32) -> &'static Tty {
    let Some(mem) = malloc(size_of::<Tty>(), M_TTYS, M_WAITOK | M_ZERO) else {
        panic(format_args!("ttymalloc: out of memory"));
    };
    let tpp = mem.cast::<Tty>();
    // SAFETY: a fresh allocation of `size_of::<Tty>()` bytes, aligned by `malloc(9)` for any
    // kernel structure; written once before anything else sees it.
    unsafe { tpp.as_ptr().write(Tty::new()) };
    // SAFETY: as above; the tty lives until `ttyfree` gives it back.
    let tp: &'static Tty = unsafe { tpp.as_ref() };

    let baud = if baud == 0 { 115200 } else { baud };

    tp.t_qlen.set(if baud <= 9600 {
        1024
    } else if baud <= 115200 {
        4096
    } else {
        8192
    });
    clalloc(&tp.t_rawq, tp.t_qlen.get(), true);
    clalloc(&tp.t_canq, tp.t_qlen.get(), true);
    // output queue doesn't need quoting
    clalloc(&tp.t_outq, tp.t_qlen.get(), false);

    rw_enter_write(&TTYLIST_LOCK);
    // SAFETY: a new tty in no list; the list is ttylist_lock's.
    unsafe { TTYLIST.0.insert_tail(tp) };
    TTY_COUNT.fetch_add(1, Ordering::Relaxed);
    rw_exit_write(&TTYLIST_LOCK);

    timeout_set(&tp.t_rstrt_to, ttrstrt, ptr::from_ref(tp).cast_mut().cast());

    tp
}

/// `ttyfree`: free a tty structure and its buffers, after removing it from the tty list.
///
/// # Safety
///
/// `tp` came from [`ttymalloc`], its driver no longer uses it, no thread sleeps on it and
/// its restart timeout is not pending; nothing uses it afterwards.
pub unsafe fn ttyfree(tp: NonNull<Tty>) {
    // SAFETY: the caller's contract: a live tty from ttymalloc.
    let t = unsafe { tp.as_ref() };

    rw_enter_write(&TTYLIST_LOCK);
    let count = TTY_COUNT.fetch_sub(1, Ordering::Relaxed) - 1;
    #[cfg(feature = "diagnostic")]
    if count < 0 {
        panic(format_args!("ttyfree: tty_count < 0"));
    }
    let _ = count;
    // SAFETY: the tty is on ttylist since ttymalloc; the list is ttylist_lock's.
    unsafe { TTYLIST.0.remove(t) };
    rw_exit_write(&TTYLIST_LOCK);

    // klist_invalidate(&tp->t_rsel.si_note), (&tp->t_wsel.si_note): no knote can be on
    // them (kern_event.c is not ported).

    clfree(&t.t_rawq);
    clfree(&t.t_canq);
    clfree(&t.t_outq);
    free(tp.cast::<u8>(), M_TTYS, size_of::<Tty>());
}

/// `ttystats_init`: the `struct itty` of every tty, in a `malloc(M_SYSCTL)`ed array the
/// caller frees: the array, the number filled and its size in bytes.
fn ttystats_init() -> (Option<NonNull<Itty>>, usize, usize) {
    let ttyc = TTY_COUNT.load(Ordering::Relaxed).max(0) as usize;
    let ttystatssiz = ttyc * size_of::<Itty>();
    if ttyc == 0 {
        return (None, 0, 0);
    }
    let Some(mem) = malloc(ttystatssiz, M_SYSCTL, M_WAITOK | M_ZERO) else {
        panic(format_args!("ttystats_init: out of memory"));
    };
    let ttystats = mem.cast::<Itty>();

    let mut ntty = 0;
    rw_enter_write(&TTYLIST_LOCK);
    for tp in TTYLIST.0.iter() {
        if ntty >= ttyc {
            break;
        }
        let itp = Itty {
            t_dev: tp.t_dev.get(),
            t_rawq_c_cc: tp.t_rawq.c_cc.get(),
            t_canq_c_cc: tp.t_canq.c_cc.get(),
            t_outq_c_cc: tp.t_outq.c_cc.get(),
            t_hiwat: tp.t_hiwat.get(),
            t_lowat: tp.t_lowat.get(),
            t_column: if tp.t_oflag() & OPOST != 0 {
                tp.t_column.get()
            } else {
                0
            },
            _pad0: 0,
            t_state: tp.t_state.get(),
            t_session_id: tp
                .session()
                // SAFETY: a session leader stays allocated while it leads the session.
                .and_then(|s| unsafe { s.s_leader.get().as_ref() })
                .map_or(0, |l| l.ps_pid.get()),
            t_pgrp_pg_id: tp.pgrp().map_or(0, |pg| pg.pg_id.get()),
            t_line: tp.t_line.get(),
            _pad1: [0; 3],
        };
        // SAFETY: `ntty < ttyc`: inside the array allocated above.
        unsafe { ttystats.as_ptr().add(ntty).write(itp) };
        ntty += 1;
    }
    rw_exit_write(&TTYLIST_LOCK);
    (Some(ttystats), ntty, ttystatssiz)
}

/// `sysctl_tty`: return tty-related information (`kern.tty`).
pub fn sysctl_tty(
    name: &[i32],
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    newlen: usize,
) -> Result<(), Errno> {
    if name.len() != 1 {
        return Err(Errno::ENOTDIR);
    }

    match name[0] {
        KERN_TTY_TKNIN => sysctl_rdquad(oldp, oldlenp, newp, TK_NIN.load(Ordering::Relaxed)),
        KERN_TTY_TKNOUT => sysctl_rdquad(oldp, oldlenp, newp, TK_NOUT.load(Ordering::Relaxed)),
        KERN_TTY_TKRAWCC => sysctl_rdquad(oldp, oldlenp, newp, TK_RAWCC.load(Ordering::Relaxed)),
        KERN_TTY_TKCANCC => sysctl_rdquad(oldp, oldlenp, newp, TK_CANCC.load(Ordering::Relaxed)),
        KERN_TTY_INFO => {
            let (ttystats, ttyc, ttystatssiz) = ttystats_init();
            let bytes: &[u8] = match ttystats {
                // SAFETY: `ttyc` initialised `Itty`s (no padding, `SysctlPlain`) in the
                // allocation, read as bytes until the free below.
                Some(p) => unsafe {
                    core::slice::from_raw_parts(p.as_ptr().cast::<u8>(), ttyc * size_of::<Itty>())
                },
                None => &[],
            };
            let err = sysctl_rdstruct(oldp, oldlenp, newp, bytes);
            if let Some(p) = ttystats {
                free(p.cast::<u8>(), M_SYSCTL, ttystatssiz);
            }
            err
        }
        // NPTY > 0
        _ => sysctl_pty(name, oldp, oldlenp, newp, newlen),
    }
}

/// `ttytstamp`: record the time of a modem line transition the user asked for
/// (`TIOCSTSTAMP`).
pub fn ttytstamp(tp: &Tty, octs: i32, ncts: i32, odcd: i32, ndcd: i32) {
    let mut doit = 0;

    if ncts ^ octs != 0 {
        doit |= if ncts != 0 {
            tp.t_flags.get() & TS_TSTAMPCTSSET
        } else {
            tp.t_flags.get() & TS_TSTAMPCTSCLR
        };
    }
    if ndcd ^ odcd != 0 {
        doit |= if ndcd != 0 {
            tp.t_flags.get() & TS_TSTAMPDCDSET
        } else {
            tp.t_flags.get() & TS_TSTAMPDCDCLR
        };
    }

    if doit != 0 {
        tp.t_tv.set(microtime());
    }
}

#[cfg(test)]
mod tests;
