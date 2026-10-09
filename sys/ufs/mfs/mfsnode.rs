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
 *	@(#)mfsnode.h	8.2 (Berkeley) 8/11/93
 */
/* </LICENSES> */

/* <CODE> */
//! `<ufs/mfs/mfsnode.h>`: the control data of the memory based file system.
//!
//! Upstream: sys/ufs/mfs/mfsnode.h @ 3ce1f3f79392
//!
//! A `struct mfsnode` is the `v_data` of the vnode `mfs_mount` makes for the file system's
//! "device": it is `malloc(M_MFSNODE)`ed there, handed around as `&'static Mfsnode` (the way
//! `Ufsmount` is) and freed by `mfs_reclaim`.
//!
//! ## Deviations
//! - The members are `Cell`s, except `mfs_shutdown`, which is atomic: `mfs_close` (any
//!   process) sets it and `mfs_start` (the file system's process) polls it.
//! - `mfs_baseoff` is a user address (`usize`): the base of the file system in the memory of
//!   the process that called `mount(2)`, which `mfs_doio` reaches with `copyin`/`copyout`.
//!   `mfs_size` is the C's `long` (`i64`).
//! - `VTOMFS` is [`vtomfs`], which checks the vnode's tag and that it has data; `MFSTOV` is
//!   [`mfstov`], which asserts the vnode is set.

use core::cell::Cell;
use core::sync::atomic::AtomicI32;

use crate::kern::subr_prf::panic;
use crate::sys::buf::{Buf, Bufq};
use crate::sys::types::Pid;
use crate::sys::vnode::{VT_MFS, Vnode};

/// `struct mfsnode`: this structure defines the control data for the memory based file system.
pub struct Mfsnode {
    /// `mfs_vnode`: vnode associated with this mfsnode.
    pub mfs_vnode: Cell<Option<&'static Vnode>>,
    /// `mfs_bufq`: bufq for MFS I/O.
    pub mfs_bufq: Bufq,
    /// `mfs_baseoff`: base of file system in memory (a user address, see the deviations).
    pub mfs_baseoff: Cell<usize>,
    /// `mfs_size`: size of memory file system.
    pub mfs_size: Cell<i64>,
    /// `mfs_tid`: supporting thread's tid.
    pub mfs_tid: Cell<Pid>,
    /// `mfs_buflist`: list of I/O requests (unused by the C as well).
    pub mfs_buflist: Cell<Option<&'static Buf>>,
    /// `mfs_shutdown`: shutdown request.
    pub mfs_shutdown: AtomicI32,
}

impl Mfsnode {
    /// A zeroed mfsnode, as `malloc(.., M_ZERO)` leaves one, for `mfs_mount` to fill in.
    pub const fn new() -> Self {
        Self {
            mfs_vnode: Cell::new(None),
            mfs_bufq: Bufq::new(),
            mfs_baseoff: Cell::new(0),
            mfs_size: Cell::new(0),
            mfs_tid: Cell::new(0),
            mfs_buflist: Cell::new(None),
            mfs_shutdown: AtomicI32::new(0),
        }
    }
}

impl Default for Mfsnode {
    fn default() -> Self {
        Self::new()
    }
}

/// `VTOMFS(vp)`: convert a vnode pointer to an mfsnode pointer.
pub fn vtomfs(vp: &'static Vnode) -> &'static Mfsnode {
    let data = vp.v_data.get();
    if vp.v_tag.get() != VT_MFS || data.is_null() {
        panic(format_args!("VTOMFS: vnode {:p} is not an mfs node", vp));
    }
    // SAFETY: a `VT_MFS` vnode's `v_data` (checked above) is the `struct mfsnode` that
    // `mfs_mount` allocated and initialised; it stays until `mfs_reclaim` clears `v_data`.
    unsafe { &*data.cast::<Mfsnode>() }
}

/// `MFSTOV(mfsp)`: convert an mfsnode pointer to a vnode pointer.
pub fn mfstov(mfsp: &Mfsnode) -> &'static Vnode {
    match mfsp.mfs_vnode.get() {
        Some(vp) => vp,
        None => panic(format_args!("MFSTOV: mfsnode has no vnode")),
    }
}
/* </CODE> */
