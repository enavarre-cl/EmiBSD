/*	$OpenBSD: systm.h,v 1.179 2026/04/22 01:51:37 jsg Exp $	*/
/*	$NetBSD: systm.h,v 1.50 1996/06/09 04:55:09 briggs Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (c) 1982, 1988, 1991, 1993
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
 *	@(#)systm.h	8.4 (Berkeley) 2/23/94
 */
/* </LICENSES> */

//! `<sys/systm.h>`: the kernel's global declarations.
//!
//! Upstream: sys/sys/systm.h @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M3 ports `physmem`; M5 adds `cold`, `safepri` and the sleep
//! limits `INFSLP`/`MAXTSLP`; M6 `struct sysent`, `sy_call_t`, `SY_NOLOCK` and `SCARG`; M7b
//! the net lock macros (`NET_LOCK` .. `NET_ASSERT_LOCKED_EXCLUSIVE`) over `netlock`. The
//! hostname and boot-time globals, the `panic`/`printf` prototypes (already in
//! `kern/subr_prf.rs`) and the rest arrive with their files. `tsleep`/`wakeup` are in
//! `kern/kern_synch.rs`; the `copyin`/`copyout` family is `machine::copy`.
//!
//! ## Deviations
//! - `physmem`, `cold` and `safepri` are defined here (the C defines each in every
//!   `machdep.c`/`autoconf.c` and declares them here), so generic code names them without an
//!   architecture path; the `machdep`s and `cpu_configure` fill them.
//! - `NET_LOCK()` and friends are functions (`net_lock()`); the assertions take the caller's
//!   name, which the C's macros take from `__func__`. `netlock` itself is defined in
//!   `net/if_.rs`, where `net/if.c` defines it.
//! - `sy_call_t` returns `Result<(), Errno>` with the two return registers as an out
//!   parameter; `SCARG(uap, k)` is `sysargs::<T>(v).k.get()` (`sys/syscallargs.rs`).

use core::ptr;
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicUsize};

use crate::kern::kern_rwlock::{
    rw_enter_read, rw_enter_write, rw_exit_read, rw_exit_write, rw_status,
};
use crate::net::if_::NETLOCK;
use crate::sys::errno::Errno;
use crate::sys::proc::Proc;
use crate::sys::types::Register;

/// `physmem`: physical memory, in pages (an `int` in C).
pub static PHYSMEM: AtomicUsize = AtomicUsize::new(0);

/// `cold`: cold start flag, set in locore, cleared by `cpu_configure` once the devices are
/// attached and interrupts can be taken.
pub static COLD: AtomicBool = AtomicBool::new(true);

/// `safepri`: the IPL `tsleep` lowers to while cold or after a panic, to give interrupts a
/// chance (`int safepri = 0` in each `machdep.c`).
pub static SAFEPRI: AtomicI32 = AtomicI32::new(0);

/// `INFSLP`: sleep forever (`tsleep_nsec` and friends).
pub const INFSLP: u64 = u64::MAX;
/// `MAXTSLP`: the longest finite sleep.
pub const MAXTSLP: u64 = u64::MAX - 1;

/// The argument block of a system call: the six argument registers, in the C ABI's order,
/// as the machine-dependent entry finds them in the trap frame.
pub type SysArgs = [Register; 6];

/// `sy_call_t`: every system call: the calling thread, the argument block (see `sysargs`)
/// and the two return registers (`retval[0]` is what the user sees in its return register).
pub type SyCall = fn(&Proc, &SysArgs, &mut [Register; 2]) -> Result<(), Errno>;

/// `struct sysent`: system call table entry.
#[derive(Clone, Copy)]
pub struct Sysent {
    /// `sy_narg`: number of args.
    pub sy_narg: i16,
    /// `sy_argsize`: total size of arguments.
    pub sy_argsize: i16,
    /// `sy_flags`: `SY_*`.
    pub sy_flags: i32,
    /// `sy_call`: implementing function.
    pub sy_call: SyCall,
}

impl Sysent {
    /// One table entry (`init_sysent.rs` is generated with these).
    pub const fn new(narg: i16, argsize: usize, flags: i32, call: SyCall) -> Self {
        Self {
            sy_narg: narg,
            sy_argsize: argsize as i16,
            sy_flags: flags,
            sy_call: call,
        }
    }
}

/// `SY_NOLOCK`: the syscall does not take the kernel lock.
pub const SY_NOLOCK: i32 = 0x01;

/// `SCARG`'s view of a system call's argument block as its `struct sys_*_args`: `T` is one
/// of `sys/syscallargs.rs`'s structs, register-wide `Syscallarg` slots in a row that any bit
/// pattern fills validly, no larger than the six registers.
pub fn sysargs<T>(args: &SysArgs) -> &T {
    const {
        assert!(size_of::<T>() <= size_of::<SysArgs>());
        assert!(align_of::<T>() <= align_of::<SysArgs>());
    }
    // SAFETY: `T` fits in the block (asserted above), shares its alignment, and is made of
    // `Syscallarg` unions, for which every register value is a valid datum.
    unsafe { &*ptr::from_ref(args).cast::<T>() }
}

/// `NET_LOCK()`: network stack data structures are, unless stated otherwise, protected by the
/// net lock. It's a single non-recursive lock for the whole subsystem.
pub fn net_lock() {
    rw_enter_write(&NETLOCK);
}

/// `NET_UNLOCK()`.
pub fn net_unlock() {
    rw_exit_write(&NETLOCK);
}

/// `NET_LOCK_SHARED()`: reader version of `NET_LOCK()`. The "softnet" thread should be the
/// only thread processing packets without holding an exclusive lock. This is done to allow
/// read-only ioctl(2) to not block. Shared lock can be grabbed instead of the exclusive
/// version if no field protected by the `NET_LOCK()` is modified by the ioctl/sysctl. Socket
/// system call can use shared netlock if it has additional locks to protect socket and pcb
/// data structures.
pub fn net_lock_shared() {
    rw_enter_read(&NETLOCK);
}

/// `NET_UNLOCK_SHARED()`.
pub fn net_unlock_shared() {
    rw_exit_read(&NETLOCK);
}

/// `NET_ASSERT_UNLOCKED()` (`DIAGNOSTIC`): this thread does not hold the net lock exclusively.
pub fn net_assert_unlocked(func: &str) {
    #[cfg(feature = "diagnostic")]
    {
        let s = rw_status(&NETLOCK);
        if crate::kern::subr_prf::SPLASSERT_CTL.load(core::sync::atomic::Ordering::Relaxed) > 0
            && s == crate::sys::rwlock::RW_WRITE
        {
            crate::kern::subr_prf::splassert_fail(0, crate::sys::rwlock::RW_WRITE, func);
        }
    }
    #[cfg(not(feature = "diagnostic"))]
    let _ = (func, rw_status);
}

/// `NET_ASSERT_LOCKED()` (`DIAGNOSTIC`): the net lock is held, shared or exclusive.
pub fn net_assert_locked(func: &str) {
    #[cfg(feature = "diagnostic")]
    {
        use crate::sys::rwlock::{RW_READ, RW_WRITE};
        let s = rw_status(&NETLOCK);
        if crate::kern::subr_prf::SPLASSERT_CTL.load(core::sync::atomic::Ordering::Relaxed) > 0
            && s != RW_WRITE
            && s != RW_READ
        {
            crate::kern::subr_prf::splassert_fail(RW_READ, s, func);
        }
    }
    #[cfg(not(feature = "diagnostic"))]
    let _ = func;
}

/// `NET_ASSERT_LOCKED_EXCLUSIVE()` (`DIAGNOSTIC`): this thread holds the net lock exclusively.
pub fn net_assert_locked_exclusive(func: &str) {
    #[cfg(feature = "diagnostic")]
    {
        use crate::sys::rwlock::RW_WRITE;
        let s = rw_status(&NETLOCK);
        if crate::kern::subr_prf::SPLASSERT_CTL.load(core::sync::atomic::Ordering::Relaxed) > 0
            && s != RW_WRITE
        {
            crate::kern::subr_prf::splassert_fail(RW_WRITE, s, func);
        }
    }
    #[cfg(not(feature = "diagnostic"))]
    let _ = func;
}
