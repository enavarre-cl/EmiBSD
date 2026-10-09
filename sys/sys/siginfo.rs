/* <LICENSES> */
/*
 * Copyright (c) 1997 Theo de Raadt
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
/* </LICENSES> */

/* <CODE> */
//! `<sys/siginfo.h>`: `union sigval`, `siginfo_t` and the signal codes.
//!
//! Upstream: sys/sys/siginfo.h @ 3ce1f3f79392
//!
//! Status: `ported` (with `kern_sig.c`). `initsiginfo`, the one prototype, is in
//! `kern/kern_sig.rs`.
//!
//! ## Deviations
//! - `union sigval` is [`Sigval`], one pointer-sized word: `sival_ptr` is the word,
//!   `sival_int` its low 32 bits, which is where the C's `int` member lies on the two
//!   little-endian targets.
//! - `siginfo_t` is [`Siginfo`]: `si_signo`, `si_code`, `si_errno`, the alignment hole the C
//!   compiler leaves before the union (spelled out), and the 120-byte union as fifteen
//!   `u64` words, read and written through accessor methods named after the C's
//!   `si_*` macros (`si_addr()`, `set_si_addr()`, ...). The size (136 bytes on LP64) and
//!   every offset are the C's; the `#if 0` members (`_file`, `_prof`) have no accessors.

use crate::machine::copy::AbiPod;

/// `union sigval`: an integer or a pointer value (see the module's deviations).
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sigval(usize);

impl Sigval {
    /// A `sigval` whose `sival_ptr` is `ptr` (a user address).
    pub const fn from_ptr(ptr: usize) -> Self {
        Self(ptr)
    }

    /// A `sigval` whose `sival_int` is `v`.
    pub const fn from_int(v: i32) -> Self {
        Self(v as u32 as usize)
    }

    /// `sival_ptr`: pointer value.
    pub const fn sival_ptr(self) -> usize {
        self.0
    }

    /// `sival_int`: integer value.
    pub const fn sival_int(self) -> i32 {
        self.0 as u32 as i32
    }
}

/// `SI_FROMUSER(sip)`: negative signal codes are reserved for future use for user generated
/// signals.
pub const fn si_fromuser(si: &Siginfo) -> bool {
    si.si_code <= 0
}

/// `SI_FROMKERNEL(sip)`.
pub const fn si_fromkernel(si: &Siginfo) -> bool {
    si.si_code > 0
}

/// `SI_NOINFO`: no signal information.
pub const SI_NOINFO: i32 = 32767;
/// `SI_USER`: user generated signal via kill().
pub const SI_USER: i32 = 0;
/// `SI_LWP`: user generated signal via lwp_kill().
pub const SI_LWP: i32 = -1;
/// `SI_QUEUE`: user generated signal via sigqueue().
pub const SI_QUEUE: i32 = -2;
/// `SI_TIMER`: from timer expiration.
pub const SI_TIMER: i32 = -3;

// The machine dependent signal codes (SIGILL, SIGFPE, SIGSEGV, and SIGBUS).

/// `ILL_ILLOPC`: illegal opcode.
pub const ILL_ILLOPC: i32 = 1;
/// `ILL_ILLOPN`: illegal operand.
pub const ILL_ILLOPN: i32 = 2;
/// `ILL_ILLADR`: illegal addressing mode.
pub const ILL_ILLADR: i32 = 3;
/// `ILL_ILLTRP`: illegal trap.
pub const ILL_ILLTRP: i32 = 4;
/// `ILL_PRVOPC`: privileged opcode.
pub const ILL_PRVOPC: i32 = 5;
/// `ILL_PRVREG`: privileged register.
pub const ILL_PRVREG: i32 = 6;
/// `ILL_COPROC`: co-processor.
pub const ILL_COPROC: i32 = 7;
/// `ILL_BADSTK`: bad stack.
pub const ILL_BADSTK: i32 = 8;
/// `ILL_BTCFI`: IBT missing on indirect call.
pub const ILL_BTCFI: i32 = 9;
/// `NSIGILL`.
pub const NSIGILL: i32 = 9;

/// `EMT_TAGOVF`: tag overflow.
pub const EMT_TAGOVF: i32 = 1;
/// `NSIGEMT`.
pub const NSIGEMT: i32 = 1;

/// `FPE_INTDIV`: integer divide by zero.
pub const FPE_INTDIV: i32 = 1;
/// `FPE_INTOVF`: integer overflow.
pub const FPE_INTOVF: i32 = 2;
/// `FPE_FLTDIV`: floating point divide by zero.
pub const FPE_FLTDIV: i32 = 3;
/// `FPE_FLTOVF`: floating point overflow.
pub const FPE_FLTOVF: i32 = 4;
/// `FPE_FLTUND`: floating point underflow.
pub const FPE_FLTUND: i32 = 5;
/// `FPE_FLTRES`: floating point inexact result.
pub const FPE_FLTRES: i32 = 6;
/// `FPE_FLTINV`: invalid floating point operation.
pub const FPE_FLTINV: i32 = 7;
/// `FPE_FLTSUB`: subscript out of range.
pub const FPE_FLTSUB: i32 = 8;
/// `NSIGFPE`.
pub const NSIGFPE: i32 = 8;

/// `SEGV_MAPERR`: address not mapped to object.
pub const SEGV_MAPERR: i32 = 1;
/// `SEGV_ACCERR`: invalid permissions.
pub const SEGV_ACCERR: i32 = 2;
/// `NSIGSEGV`.
pub const NSIGSEGV: i32 = 2;

/// `BUS_ADRALN`: invalid address alignment.
pub const BUS_ADRALN: i32 = 1;
/// `BUS_ADRERR`: non-existent physical address.
pub const BUS_ADRERR: i32 = 2;
/// `BUS_OBJERR`: object specific hardware error.
pub const BUS_OBJERR: i32 = 3;
/// `NSIGBUS`.
pub const NSIGBUS: i32 = 3;

/// `TRAP_BRKPT`: breakpoint trap.
pub const TRAP_BRKPT: i32 = 1;
/// `TRAP_TRACE`: trace trap.
pub const TRAP_TRACE: i32 = 2;
/// `NSIGTRAP`.
pub const NSIGTRAP: i32 = 2;

/// `CLD_EXITED`: child has exited.
pub const CLD_EXITED: i32 = 1;
/// `CLD_KILLED`: child was killed.
pub const CLD_KILLED: i32 = 2;
/// `CLD_DUMPED`: child has coredumped.
pub const CLD_DUMPED: i32 = 3;
/// `CLD_TRAPPED`: traced child has stopped.
pub const CLD_TRAPPED: i32 = 4;
/// `CLD_STOPPED`: child has stopped on signal.
pub const CLD_STOPPED: i32 = 5;
/// `CLD_CONTINUED`: stopped child has continued.
pub const CLD_CONTINUED: i32 = 6;
/// `NSIGCLD`.
pub const NSIGCLD: i32 = 6;

// SIGPOLL and SIGPROF signal codes: not supported (`#if 0` in C).

/// `SI_MAXSZ`.
pub const SI_MAXSZ: usize = 128;
/// `SI_PAD`.
pub const SI_PAD: usize = (SI_MAXSZ / size_of::<i32>()) - 3;

/// The words of `siginfo_t`'s `_data` union: `SI_PAD` ints rounded up to the union's
/// pointer alignment.
const SI_DATA_WORDS: usize = (SI_PAD * size_of::<i32>()).div_ceil(size_of::<u64>());

/// `siginfo_t` (see the module's deviations for the union).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Siginfo {
    /// `si_signo`: signal from signal.h.
    pub si_signo: i32,
    /// `si_code`: code from above.
    pub si_code: i32,
    /// `si_errno`: error from errno.h.
    pub si_errno: i32,
    /// The C compiler's alignment hole before the pointer-aligned union.
    hole: i32,
    /// `_data`: the union of `_pad[SI_PAD]`, `_proc` and `_fault`.
    data: [u64; SI_DATA_WORDS],
}

impl Siginfo {
    /// An all-zero `siginfo_t` (`memset(si, 0, sizeof(*si))`).
    pub const fn zeroed() -> Self {
        Self {
            si_signo: 0,
            si_code: 0,
            si_errno: 0,
            hole: 0,
            data: [0; SI_DATA_WORDS],
        }
    }

    /// The low 32 bits of a union word, as the `int` the C stores there.
    const fn lo(&self, word: usize) -> i32 {
        self.data[word] as u32 as i32
    }

    /// The high 32 bits of a union word, as the `int` the C stores there.
    const fn hi(&self, word: usize) -> i32 {
        (self.data[word] >> 32) as u32 as i32
    }

    /// Stores `v` in the low 32 bits of a union word.
    const fn set_lo(&mut self, word: usize, v: i32) {
        self.data[word] = (self.data[word] & !0xffff_ffff) | (v as u32 as u64);
    }

    /// Stores `v` in the high 32 bits of a union word.
    const fn set_hi(&mut self, word: usize, v: i32) {
        self.data[word] = (self.data[word] & 0xffff_ffff) | ((v as u32 as u64) << 32);
    }

    /// `si_pid` (`_data._proc._pid`): process ID.
    pub const fn si_pid(&self) -> i32 {
        self.lo(0)
    }

    /// Sets `si_pid`.
    pub const fn set_si_pid(&mut self, pid: i32) {
        self.set_lo(0, pid);
    }

    /// `si_uid` (`_data._proc._uid`).
    pub const fn si_uid(&self) -> u32 {
        self.hi(0) as u32
    }

    /// Sets `si_uid`.
    pub const fn set_si_uid(&mut self, uid: u32) {
        self.set_hi(0, uid as i32);
    }

    /// `si_value` (`_data._proc._pdata._kill._value`).
    pub const fn si_value(&self) -> Sigval {
        Sigval::from_ptr(self.data[1] as usize)
    }

    /// Sets `si_value`.
    pub const fn set_si_value(&mut self, v: Sigval) {
        self.data[1] = v.sival_ptr() as u64;
    }

    /// `si_utime` (`_data._proc._pdata._cld._utime`).
    pub const fn si_utime(&self) -> i64 {
        self.data[1] as i64
    }

    /// Sets `si_utime`.
    pub const fn set_si_utime(&mut self, t: i64) {
        self.data[1] = t as u64;
    }

    /// `si_stime` (`_data._proc._pdata._cld._stime`).
    pub const fn si_stime(&self) -> i64 {
        self.data[2] as i64
    }

    /// Sets `si_stime`.
    pub const fn set_si_stime(&mut self, t: i64) {
        self.data[2] = t as u64;
    }

    /// `si_status` (`_data._proc._pdata._cld._status`).
    pub const fn si_status(&self) -> i32 {
        self.lo(3)
    }

    /// Sets `si_status`.
    pub const fn set_si_status(&mut self, status: i32) {
        self.set_lo(3, status);
    }

    /// `si_addr` (`_data._fault._addr`): faulting address.
    pub const fn si_addr(&self) -> usize {
        self.data[0] as usize
    }

    /// Sets `si_addr`.
    pub const fn set_si_addr(&mut self, addr: usize) {
        self.data[0] = addr as u64;
    }

    /// `si_trapno` (`_data._fault._trapno`): illegal trap number.
    pub const fn si_trapno(&self) -> i32 {
        self.lo(1)
    }

    /// Sets `si_trapno`.
    pub const fn set_si_trapno(&mut self, trapno: i32) {
        self.set_lo(1, trapno);
    }
}

impl Default for Siginfo {
    fn default() -> Self {
        Self::zeroed()
    }
}

// SAFETY: `repr(C)`: four `i32`s and fifteen `u64`s, no implicit padding (the hole is a
// field); every bit pattern is a valid value.
unsafe impl AbiPod for Siginfo {}

const _: () = {
    assert!(cfg!(target_endian = "little"));
    assert!(size_of::<Siginfo>() == 136);
    assert!(core::mem::offset_of!(Siginfo, data) == 16);
    assert!(size_of::<Sigval>() == size_of::<usize>());
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn union_members_share_the_words_as_in_c() {
        let mut si = Siginfo::zeroed();
        si.set_si_pid(42);
        si.set_si_uid(1000);
        assert_eq!(si.si_pid(), 42);
        assert_eq!(si.si_uid(), 1000);
        // _fault._addr overlays _proc._pid/_uid.
        assert_eq!(si.si_addr(), (1000usize << 32) | 42);
        si.set_si_addr(0xdead_beef_0000);
        si.set_si_trapno(14);
        assert_eq!(si.si_trapno(), 14);
        // _fault._trapno overlays the low half of _kill._value.
        assert_eq!(si.si_value().sival_int(), 14);
        si.set_si_status(-1);
        assert_eq!(si.si_status(), -1);
        assert_eq!(Sigval::from_int(-5).sival_int(), -5);
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/sys/siginfo.h");
        let ours: &[(&str, i64)] = &[
            ("SI_NOINFO", SI_NOINFO as i64),
            ("SI_USER", SI_USER as i64),
            ("ILL_BTCFI", ILL_BTCFI as i64),
            ("FPE_FLTSUB", FPE_FLTSUB as i64),
            ("SEGV_ACCERR", SEGV_ACCERR as i64),
            ("BUS_OBJERR", BUS_OBJERR as i64),
            ("TRAP_TRACE", TRAP_TRACE as i64),
            ("CLD_CONTINUED", CLD_CONTINUED as i64),
            ("SI_MAXSZ", SI_MAXSZ as i64),
        ];
        for (name, value) in ours {
            assert_eq!(crate::reftest::int(&defs, name), Some(*value), "{name}");
        }
    }
}
/* </TESTS> */
