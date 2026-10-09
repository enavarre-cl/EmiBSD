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
 * Copyright (c) 1982, 1986, 1993
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
 *	@(#)resource.h	8.2 (Berkeley) 1/4/94
 */
/* </LICENSES> */

/* <CODE> */
//! `<sys/resource.h>`: priorities, resource usage and limits.
//!
//! Upstream: sys/sys/resource.h @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M5 ports the constants, `struct rusage`, `struct rlimit` and
//! `struct loadavg`; `dosetrlimit`, `donice` and `dogetrusage` are `kern_resource.c` (M6).
//!
//! ## Deviations
//! - `struct rusage`'s counters are `Cell`s: the owning thread bumps them (`ru_nvcsw++`).

use core::cell::Cell;

use crate::sys::time::Timeval;
use crate::sys::types::{Fixpt, Rlim};

/// `PRIO_MIN`.
pub const PRIO_MIN: i32 = -20;
/// `PRIO_MAX`.
pub const PRIO_MAX: i32 = 20;

/// `PRIO_PROCESS`.
pub const PRIO_PROCESS: i32 = 0;
/// `PRIO_PGRP`.
pub const PRIO_PGRP: i32 = 1;
/// `PRIO_USER`.
pub const PRIO_USER: i32 = 2;

/// `RUSAGE_SELF`.
pub const RUSAGE_SELF: i32 = 0;
/// `RUSAGE_CHILDREN`.
pub const RUSAGE_CHILDREN: i32 = -1;
/// `RUSAGE_THREAD`.
pub const RUSAGE_THREAD: i32 = 1;

/// `struct rusage`: resource utilization information.
pub struct Rusage {
    /// `ru_utime`: user time used.
    pub ru_utime: Cell<Timeval>,
    /// `ru_stime`: system time used.
    pub ru_stime: Cell<Timeval>,
    /// `ru_maxrss`: max resident set size.
    pub ru_maxrss: Cell<i64>,
    /// `ru_ixrss`: integral shared text memory size.
    pub ru_ixrss: Cell<i64>,
    /// `ru_idrss`: integral unshared data.
    pub ru_idrss: Cell<i64>,
    /// `ru_isrss`: integral unshared stack.
    pub ru_isrss: Cell<i64>,
    /// `ru_minflt`: page reclaims.
    pub ru_minflt: Cell<i64>,
    /// `ru_majflt`: page faults.
    pub ru_majflt: Cell<i64>,
    /// `ru_nswap`: swaps.
    pub ru_nswap: Cell<i64>,
    /// `ru_inblock`: block input operations.
    pub ru_inblock: Cell<i64>,
    /// `ru_oublock`: block output operations.
    pub ru_oublock: Cell<i64>,
    /// `ru_msgsnd`: messages sent.
    pub ru_msgsnd: Cell<i64>,
    /// `ru_msgrcv`: messages received.
    pub ru_msgrcv: Cell<i64>,
    /// `ru_nsignals`: signals received.
    pub ru_nsignals: Cell<i64>,
    /// `ru_nvcsw`: voluntary context switches.
    pub ru_nvcsw: Cell<i64>,
    /// `ru_nivcsw`: involuntary context switches.
    pub ru_nivcsw: Cell<i64>,
}

// SAFETY: the owning thread's statistics, written by that thread.
unsafe impl Sync for Rusage {}

impl Rusage {
    /// All zero.
    pub const fn new() -> Self {
        Self {
            ru_utime: Cell::new(Timeval::new(0, 0)),
            ru_stime: Cell::new(Timeval::new(0, 0)),
            ru_maxrss: Cell::new(0),
            ru_ixrss: Cell::new(0),
            ru_idrss: Cell::new(0),
            ru_isrss: Cell::new(0),
            ru_minflt: Cell::new(0),
            ru_majflt: Cell::new(0),
            ru_nswap: Cell::new(0),
            ru_inblock: Cell::new(0),
            ru_oublock: Cell::new(0),
            ru_msgsnd: Cell::new(0),
            ru_msgrcv: Cell::new(0),
            ru_nsignals: Cell::new(0),
            ru_nvcsw: Cell::new(0),
            ru_nivcsw: Cell::new(0),
        }
    }
}

impl Default for Rusage {
    fn default() -> Self {
        Self::new()
    }
}

impl Rusage {
    /// `*self = *other`: the struct assignment of the C.
    pub fn copy_from(&self, other: &Rusage) {
        self.ru_utime.set(other.ru_utime.get());
        self.ru_stime.set(other.ru_stime.get());
        for (a, b) in self.longs().iter().zip(other.longs().iter()) {
            a.set(b.get());
        }
    }

    /// `ru_maxrss` through `ru_nivcsw` (`ru_first` is `ru_ixrss`), in declaration order.
    fn longs(&self) -> [&Cell<i64>; 14] {
        [
            &self.ru_maxrss,
            &self.ru_ixrss,
            &self.ru_idrss,
            &self.ru_isrss,
            &self.ru_minflt,
            &self.ru_majflt,
            &self.ru_nswap,
            &self.ru_inblock,
            &self.ru_oublock,
            &self.ru_msgsnd,
            &self.ru_msgrcv,
            &self.ru_nsignals,
            &self.ru_nvcsw,
            &self.ru_nivcsw,
        ]
    }

    /// The C layout of `struct rusage` (two `struct timeval`, then fourteen `long`s), native
    /// endian, for `copyout`.
    pub fn to_bytes(&self) -> [u8; RUSAGE_SIZE] {
        let mut b = [0u8; RUSAGE_SIZE];
        let mut off = 0;
        for tv in [self.ru_utime.get(), self.ru_stime.get()] {
            b[off..off + 8].copy_from_slice(&tv.tv_sec.to_ne_bytes());
            b[off + 8..off + 16].copy_from_slice(&tv.tv_usec.to_ne_bytes());
            off += 16;
        }
        for l in self.longs() {
            b[off..off + 8].copy_from_slice(&l.get().to_ne_bytes());
            off += 8;
        }
        b
    }
}

/// `sizeof(struct rusage)` on LP64: two `struct timeval` and fourteen `long`s.
pub const RUSAGE_SIZE: usize = 2 * 16 + 14 * 8;

/// `RLIMIT_CPU`: cpu time in milliseconds.
pub const RLIMIT_CPU: usize = 0;
/// `RLIMIT_FSIZE`: maximum file size.
pub const RLIMIT_FSIZE: usize = 1;
/// `RLIMIT_DATA`: data size.
pub const RLIMIT_DATA: usize = 2;
/// `RLIMIT_STACK`: stack size.
pub const RLIMIT_STACK: usize = 3;
/// `RLIMIT_CORE`: core file size.
pub const RLIMIT_CORE: usize = 4;
/// `RLIMIT_RSS`: resident set size.
pub const RLIMIT_RSS: usize = 5;
/// `RLIMIT_MEMLOCK`: locked-in-memory address space.
pub const RLIMIT_MEMLOCK: usize = 6;
/// `RLIMIT_NPROC`: number of processes.
pub const RLIMIT_NPROC: usize = 7;
/// `RLIMIT_NOFILE`: number of open files.
pub const RLIMIT_NOFILE: usize = 8;

/// `RLIM_NLIMITS`: number of resource limits.
pub const RLIM_NLIMITS: usize = 9;

/// `RLIM_INFINITY`.
pub const RLIM_INFINITY: Rlim = (1 << 63) - 1;
/// `RLIM_SAVED_MAX`.
pub const RLIM_SAVED_MAX: Rlim = RLIM_INFINITY;
/// `RLIM_SAVED_CUR`.
pub const RLIM_SAVED_CUR: Rlim = RLIM_INFINITY;

/// `struct rlimit`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rlimit {
    /// `rlim_cur`: current (soft) limit.
    pub rlim_cur: Rlim,
    /// `rlim_max`: maximum value for `rlim_cur`.
    pub rlim_max: Rlim,
}

/// `struct loadavg`: load average structure.
#[derive(Clone, Copy, Debug, Default)]
pub struct Loadavg {
    /// `ldavg`.
    pub ldavg: [Fixpt; 3],
    /// `fscale`.
    pub fscale: i64,
}
/* </CODE> */
