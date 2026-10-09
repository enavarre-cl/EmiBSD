/* <LICENSES> */
/*
 * Copyright (c) 1989, 1993
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
 *	@(#)ucred.h	8.2 (Berkeley) 1/4/94
 */
/* </LICENSES> */

/* <CODE> */
//! `<sys/ucred.h>`: credentials.
//!
//! Upstream: sys/sys/ucred.h @ 3ce1f3f79392
//!
//! Status: `ported`. Milestone M5 ports `struct ucred` and `struct xucred` (the process
//! structures point at one); `NOCRED` and `FSCRED` come with `kern_prot.c`, whose module
//! (`kern/kern_prot.rs`) holds the functions this header declares (`crget`, `crhold`,
//! `crfree`, `crset`, `crcopy`, `crdup`, `crfromxucred`, `suser`, `suser_ucred`).
//!
//! ## Deviations
//! - The members after `cr_refcnt` are `Cell`s: a credential is filled in (`crset`, exec's
//!   `crcopy`) through the same shared pointer the processes hold, while it is unshared.
//! - `cr_startcopy` (the `#define` naming where `crset` starts copying) is a comment: `crset`
//!   copies the members by name.

use core::cell::Cell;
use core::ptr;

use crate::sys::refcnt::Refcnt;
use crate::sys::syslimits::NGROUPS_MAX;
use crate::sys::types::{Gid, Uid};

/// `NOCRED`: no credential available.
pub const NOCRED: *const Ucred = ptr::without_provenance(usize::MAX);
/// `FSCRED`: filesystem credential.
pub const FSCRED: *const Ucred = ptr::without_provenance(usize::MAX - 1);

/// `struct ucred`.
pub struct Ucred {
    /// `cr_refcnt`: reference count.
    pub cr_refcnt: Refcnt,
    // The following fields are all copied by crset() (cr_startcopy = cr_uid).
    /// `cr_uid`: effective user id.
    pub cr_uid: Cell<Uid>,
    /// `cr_ruid`: real user id.
    pub cr_ruid: Cell<Uid>,
    /// `cr_svuid`: saved effective user id.
    pub cr_svuid: Cell<Uid>,
    /// `cr_gid`: effective group id.
    pub cr_gid: Cell<Gid>,
    /// `cr_rgid`: real group id.
    pub cr_rgid: Cell<Gid>,
    /// `cr_svgid`: saved effective group id.
    pub cr_svgid: Cell<Gid>,
    /// `cr_ngroups`: number of groups.
    pub cr_ngroups: Cell<i16>,
    /// `cr_groups`: groups.
    pub cr_groups: [Cell<Gid>; NGROUPS_MAX],
}

// SAFETY: credentials are written once (`crset`) before they are shared; the count is atomic.
unsafe impl Sync for Ucred {}

impl Ucred {
    /// Root's credentials with one reference (what `crget` returns zeroed).
    pub const fn new() -> Self {
        Self {
            cr_refcnt: Refcnt::new(),
            cr_uid: Cell::new(0),
            cr_ruid: Cell::new(0),
            cr_svuid: Cell::new(0),
            cr_gid: Cell::new(0),
            cr_rgid: Cell::new(0),
            cr_svgid: Cell::new(0),
            cr_ngroups: Cell::new(0),
            cr_groups: [const { Cell::new(0) }; NGROUPS_MAX],
        }
    }
}

impl Default for Ucred {
    fn default() -> Self {
        Self::new()
    }
}

/// `struct xucred`: userspace version, for use in syscalls arguments.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Xucred {
    /// `cr_uid`: user id.
    pub cr_uid: Uid,
    /// `cr_gid`: group id.
    pub cr_gid: Gid,
    /// `cr_ngroups`: number of groups.
    pub cr_ngroups: i16,
    /// `cr_groups`: groups.
    pub cr_groups: [Gid; NGROUPS_MAX],
}
/* </CODE> */
