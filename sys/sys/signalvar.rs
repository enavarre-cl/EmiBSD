/*	$OpenBSD: signalvar.h,v 1.58 2025/03/10 09:28:57 claudio Exp $	*/
/*	$NetBSD: signalvar.h,v 1.17 1996/04/22 01:23:31 christos Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1991, 1993
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
 *	@(#)signalvar.h	8.3 (Berkeley) 1/4/94
 */
/* </LICENSES> */

/* <CODE> */
//! `<sys/signalvar.h>`: kernel signal definitions and data structures, not exported to user
//! programs.
//!
//! Upstream: sys/sys/signalvar.h @ 3ce1f3f79392
//!
//! Status: `ported` (with `kern_sig.c`): `struct sigacts`, `SAS_*`, `SIG_CATCH`/`SIG_HOLD`,
//! `SIGPENDING`, the `SA_*` signal properties, `sigcantmask`, `enum signal_type` and
//! `struct sigctx`. The machine-independent functions it declares are in
//! `kern/kern_sig.rs`; `sendsig`, the machine-dependent one, is `machine::signal`.
//!
//! ## Deviations
//! - The `[m]` members of `struct sigacts` are `Cell`s under the process's `ps_mtx`, the `[a]`
//!   `ps_sigflags` an atomic; [`Sigacts::copy_from`] is `sigactsinit`'s `memcpy`.
//! - `sigcantmask` is the `const fn` [`sigcantmask`]; `SIGPENDING(p)` is [`sigpending`].
//! - `struct sigctx`'s `int` flags are `bool`s.

use core::cell::Cell;
use core::sync::atomic::{AtomicI32, Ordering};

use crate::sys::proc::Proc;
use crate::sys::signal::{NSIG, SIG_DFL, SIGKILL, SIGSTOP, Sig, Sigset, sigmask};

/// `NSIG` as an array length.
const NSIG_LEN: usize = NSIG as usize;

/// `struct sigacts`: process signal actions and state, needed only within the process (not
/// necessarily resident).
///
/// Locks used to protect struct members in struct sigacts:
/// - a: atomic operations
/// - m: this process' `ps_mtx`
pub struct Sigacts {
    /// \[m\] `ps_sigact`: disposition of signals.
    pub ps_sigact: [Cell<Sig>; NSIG_LEN],
    /// \[m\] `ps_catchmask`: signals to be blocked.
    pub ps_catchmask: [Cell<Sigset>; NSIG_LEN],
    /// \[m\] `ps_sigonstack`: signals to take on sigstack.
    pub ps_sigonstack: Cell<Sigset>,
    /// \[m\] `ps_sigintr`: signals interrupt syscalls.
    pub ps_sigintr: Cell<Sigset>,
    /// \[m\] `ps_sigreset`: signals that reset when caught.
    pub ps_sigreset: Cell<Sigset>,
    /// \[m\] `ps_siginfo`: signals that provide siginfo.
    pub ps_siginfo: Cell<Sigset>,
    /// \[m\] `ps_sigignore`: signals being ignored.
    pub ps_sigignore: Cell<Sigset>,
    /// \[m\] `ps_sigcatch`: signals being caught by user.
    pub ps_sigcatch: Cell<Sigset>,
    /// \[a\] `ps_sigflags`: signal flags, below.
    pub ps_sigflags: AtomicI32,
}

// SAFETY: the `Cell`s are touched under the owning process's `ps_mtx`, `ps_sigflags` is
// atomic.
unsafe impl Sync for Sigacts {}

impl Sigacts {
    /// An all-zero `struct sigacts` (every action `SIG_DFL`), what `sigacts0` holds before
    /// `siginit`.
    pub const fn new() -> Self {
        Self {
            ps_sigact: [const { Cell::new(SIG_DFL) }; NSIG_LEN],
            ps_catchmask: [const { Cell::new(0) }; NSIG_LEN],
            ps_sigonstack: Cell::new(0),
            ps_sigintr: Cell::new(0),
            ps_sigreset: Cell::new(0),
            ps_siginfo: Cell::new(0),
            ps_sigignore: Cell::new(0),
            ps_sigcatch: Cell::new(0),
            ps_sigflags: AtomicI32::new(0),
        }
    }

    /// `memcpy(ps, other, sizeof(struct sigacts))`.
    pub fn copy_from(&self, other: &Sigacts) {
        for (to, from) in self.ps_sigact.iter().zip(&other.ps_sigact) {
            to.set(from.get());
        }
        for (to, from) in self.ps_catchmask.iter().zip(&other.ps_catchmask) {
            to.set(from.get());
        }
        self.ps_sigonstack.set(other.ps_sigonstack.get());
        self.ps_sigintr.set(other.ps_sigintr.get());
        self.ps_sigreset.set(other.ps_sigreset.get());
        self.ps_siginfo.set(other.ps_siginfo.get());
        self.ps_sigignore.set(other.ps_sigignore.get());
        self.ps_sigcatch.set(other.ps_sigcatch.get());
        self.ps_sigflags
            .store(other.ps_sigflags.load(Ordering::Relaxed), Ordering::Relaxed);
    }
}

impl Default for Sigacts {
    fn default() -> Self {
        Self::new()
    }
}

/// `SAS_NOCLDSTOP`: No SIGCHLD when children stop.
pub const SAS_NOCLDSTOP: i32 = 0x01;
/// `SAS_NOCLDWAIT`: No zombies if child dies.
pub const SAS_NOCLDWAIT: i32 = 0x02;

/// `SIG_CATCH`: additional signal action value, used only temporarily/internally.
pub const SIG_CATCH: Sig = 2;
/// `SIG_HOLD`: additional signal action value, used only temporarily/internally.
pub const SIG_HOLD: Sig = 3;

/// `SIGPENDING(p)`: check if process p has an unmasked signal pending. Return mask of
/// pending signals.
pub fn sigpending(p: &Proc) -> Sigset {
    (p.p_siglist.load(Ordering::Relaxed) | p.process().ps_siglist.load(Ordering::Relaxed))
        & !p.p_sigmask.get()
}

/// `SA_KILL`: terminates process by default.
pub const SA_KILL: i32 = 0x01;
/// `SA_CORE`: ditto and coredumps.
pub const SA_CORE: i32 = 0x02;
/// `SA_STOP`: suspend process.
pub const SA_STOP: i32 = 0x04;
/// `SA_TTYSTOP`: ditto, from tty.
pub const SA_TTYSTOP: i32 = 0x08;
/// `SA_IGNORE`: ignore by default.
pub const SA_IGNORE: i32 = 0x10;
/// `SA_CONT`: continue if suspended.
pub const SA_CONT: i32 = 0x20;
/// `SA_CANTMASK`: non-maskable, catchable.
pub const SA_CANTMASK: i32 = 0x40;

/// `sigcantmask`: the signals that can be neither masked nor caught.
pub const fn sigcantmask() -> Sigset {
    sigmask(SIGKILL) | sigmask(SIGSTOP)
}

/// `enum signal_type`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignalType {
    /// `SPROCESS`: process signal, can be diverted (sigwait()).
    SPROCESS,
    /// `STHREAD`: thread signal, but should be propagated if unhandled.
    STHREAD,
}

/// `struct sigctx`: the signal context `cursig` fills for `postsig` and `issignal`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sigctx {
    /// `sig_action`.
    pub sig_action: Sig,
    /// `sig_catchmask`.
    pub sig_catchmask: Sigset,
    /// `sig_onstack`.
    pub sig_onstack: bool,
    /// `sig_intr`.
    pub sig_intr: bool,
    /// `sig_reset`.
    pub sig_reset: bool,
    /// `sig_info`.
    pub sig_info: bool,
    /// `sig_ignore`.
    pub sig_ignore: bool,
    /// `sig_catch`.
    pub sig_catch: bool,
    /// `sig_stop`.
    pub sig_stop: bool,
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_from_copies_every_member() {
        let a = Sigacts::new();
        a.ps_sigact[5].set(0x1234);
        a.ps_catchmask[5].set(0x10);
        a.ps_sigignore.set(0x8000);
        a.ps_sigflags.store(SAS_NOCLDWAIT, Ordering::Relaxed);
        let b = Sigacts::new();
        b.copy_from(&a);
        assert_eq!(b.ps_sigact[5].get(), 0x1234);
        assert_eq!(b.ps_catchmask[5].get(), 0x10);
        assert_eq!(b.ps_sigignore.get(), 0x8000);
        assert_eq!(b.ps_sigflags.load(Ordering::Relaxed), SAS_NOCLDWAIT);
        assert_eq!(sigcantmask(), 0x1_0100);
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/sys/signalvar.h");
        let ours: &[(&str, i64)] = &[
            ("SAS_NOCLDSTOP", SAS_NOCLDSTOP as i64),
            ("SAS_NOCLDWAIT", SAS_NOCLDWAIT as i64),
            ("SA_KILL", SA_KILL as i64),
            ("SA_CORE", SA_CORE as i64),
            ("SA_STOP", SA_STOP as i64),
            ("SA_TTYSTOP", SA_TTYSTOP as i64),
            ("SA_IGNORE", SA_IGNORE as i64),
            ("SA_CONT", SA_CONT as i64),
            ("SA_CANTMASK", SA_CANTMASK as i64),
        ];
        for (name, value) in ours {
            assert_eq!(crate::reftest::int(&defs, name), Some(*value), "{name}");
        }
    }
}
/* </TESTS> */
