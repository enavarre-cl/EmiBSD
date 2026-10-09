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
 *	@(#)resourcevar.h	8.3 (Berkeley) 2/22/94
 */
/* </LICENSES> */

/* <CODE> */
//! `<sys/resourcevar.h>`: the kernel's shareable process resource limits.
//!
//! Upstream: sys/sys/resourcevar.h @ 3ce1f3f79392
//!
//! Status: `ported` except `ADDUPROF`, which waits for `addupc_task` (`subr_prof.c`, with
//! the profiling AST). The prototypes are the functions of `kern/kern_resource.rs`.
//!
//! ## Deviations
//! - `pl_rlimit` is an array of `Cell<Rlimit>`: a `plimit` is shared copy-on-write and only
//!   written by `lim_write_begin`'s exclusive copy, under `rlimit_lock`.

use core::cell::Cell;

use crate::kassert;
use crate::kern::kern_resource::lim_read_enter;
use crate::sys::refcnt::Refcnt;
use crate::sys::resource::{RLIM_NLIMITS, Rlimit};
use crate::sys::types::Rlim;

/// `struct plimit`: kernel shareable process resource limits. Because this structure is
/// moderately large but changes infrequently, it is shared copy-on-write after forks.
pub struct Plimit {
    /// `pl_rlimit`.
    pub pl_rlimit: [Cell<Rlimit>; RLIM_NLIMITS],
    /// `pl_refcnt`.
    pub pl_refcnt: Refcnt,
}

// SAFETY: a plimit is written only through the exclusive copy `lim_write_begin` hands out
// under `rlimit_lock`, before `lim_write_commit` publishes it; readers hold a reference.
unsafe impl Sync for Plimit {}

impl Plimit {
    /// A `plimit` with every limit zero (what `limit0` holds before `lim_startup`).
    pub const fn new() -> Self {
        Self {
            pl_rlimit: [const {
                Cell::new(Rlimit {
                    rlim_cur: 0,
                    rlim_max: 0,
                })
            }; RLIM_NLIMITS],
            pl_refcnt: Refcnt::new(),
        }
    }
}

impl Default for Plimit {
    fn default() -> Self {
        Self::new()
    }
}

/// `lim_read_leave`: finish read access to resource limits.
pub fn lim_read_leave(_limit: &Plimit) {
    // nothing
}

/// `lim_cur`: get the value of the resource limit in current process.
pub fn lim_cur(which: usize) -> Rlim {
    kassert!(which < RLIM_NLIMITS);

    let limit = lim_read_enter();
    let val = limit.pl_rlimit[which].get().rlim_cur;
    lim_read_leave(limit);
    val
}
/* </CODE> */
