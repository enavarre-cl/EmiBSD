/*	$OpenBSD: mplock.h,v 1.14 2024/07/03 01:36:50 jsg Exp $	*/

/* <LICENSES> */
/*
 * Copyright (c) 2004 Niklas Hallqvist.  All rights reserved.
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

//! `<sys/mplock.h>`: the machine-independent ticket lock behind the kernel lock.
//!
//! Upstream: sys/sys/mplock.h @ 3ce1f3f79392
//!
//! Both architectures' `<machine/mplock.h>` say `__USE_MI_MPLOCK`, so this is the only
//! `struct __mp_lock`. The functions (`__mp_lock`, `__mp_unlock`, `__mp_release_all`,
//! `__mp_acquire_count`, `__mp_lock_held`) and `kernel_lock` itself live in
//! `kern/kern_lock.rs`, which defines them only with `MULTIPROCESSOR`, as the C does.
//!
//! ## Deviations
//! - `mpl_cpus[]`, `mpl_ticket` and `mpl_users` are atomics: each CPU writes its own
//!   `__mp_lock_cpu` but `__mp_lock_held` reads every CPU's, and the ticket is the C's
//!   `volatile` word. `WITNESS` (`mpl_lock_obj`) is not configured.

use core::sync::atomic::AtomicU32;

use crate::machine::cpu::MAXCPUS;

/// `struct __mp_lock_cpu`: one CPU's hold on an [`MpLock`].
pub struct MpLockCpu {
    /// `mplc_ticket`: the ticket this CPU drew when it first asked for the lock.
    pub mplc_ticket: AtomicU32,
    /// `mplc_depth`: how many times this CPU holds the lock (it is recursive).
    pub mplc_depth: AtomicU32,
}

impl MpLockCpu {
    /// A CPU that holds nothing.
    pub const fn new() -> Self {
        Self {
            mplc_ticket: AtomicU32::new(0),
            mplc_depth: AtomicU32::new(0),
        }
    }
}

impl Default for MpLockCpu {
    fn default() -> Self {
        Self::new()
    }
}

/// `struct __mp_lock`: a recursive ticket lock, one [`MpLockCpu`] per possible CPU.
pub struct MpLock {
    /// `mpl_cpus[MAXCPUS]`, indexed by `cpu_number()`.
    pub mpl_cpus: [MpLockCpu; MAXCPUS as usize],
    /// `mpl_ticket` (volatile): the ticket now being served.
    pub mpl_ticket: AtomicU32,
    /// `mpl_users`: the last ticket handed out.
    pub mpl_users: AtomicU32,
}

impl MpLock {
    /// What `___mp_lock_init` leaves: nobody holds it and ticket 1 is served next.
    pub const fn new() -> Self {
        Self {
            mpl_cpus: [const { MpLockCpu::new() }; MAXCPUS as usize],
            mpl_ticket: AtomicU32::new(1),
            mpl_users: AtomicU32::new(0),
        }
    }
}

impl Default for MpLock {
    fn default() -> Self {
        Self::new()
    }
}
