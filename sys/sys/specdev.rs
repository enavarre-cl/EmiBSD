/* <LICENSES> */
/*
 * Copyright (c) 1990, 1993
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
 *	@(#)specdev.h	8.3 (Berkeley) 8/10/94
 */
/* </LICENSES> */

/* <CODE> */
//! `<sys/specdev.h>`: the information maintained about special devices (`struct specinfo`),
//! the hash of device vnodes (`speclisth`) and the cloning-device helpers.
//!
//! Upstream: sys/sys/specdev.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - `si_specnext` is the vnode's own `v_specnext` (`sys/vnode.rs` explains why); the other
//!   shorthands (`v_rdev`, `v_specmountpoint`, `v_specparent`) are `Vnode` accessors.
//! - The union `si_ci` is two side-by-side members, `si_ci_parent` and `si_ci_bitmap`: a
//!   clone uses the first, its parent the second.
//! - `SPECHASH` is a `const fn`; `speclisth[]` is `SPECLISTH` in `kern/spec_vnops.rs`, where
//!   the C defines it; the `spec_*` prototypes are its functions.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr;

use crate::sys::lockf::LockfStateSlot;
use crate::sys::mount::Mount;
use crate::sys::queue::SlistHead;
use crate::sys::types::{Daddr, Dev};
use crate::sys::vnode::{VSpecnext, Vnode};

/// `CLONE_SHIFT`: we use the upper 16 bits of the minor to record the clone instance. This
/// gives us 8 bits for encoding the real minor number.
pub const CLONE_SHIFT: u32 = 8;
/// `CLONE_MAPSZ`: the bytes of a cloning device's bitmap.
pub const CLONE_MAPSZ: usize = 128;

/// `SPECHSZ`: buckets of the special device hash.
pub const SPECHSZ: usize = 64;

/// `SLIST_HEAD(vnodechain, vnode)`: the vnodes of one `speclisth` bucket.
pub type Vnodechain = SlistHead<VSpecnext>;

/// `struct specinfo`: the information maintained about special devices. It is allocated in
/// `checkalias` and freed in `vgone`.
pub struct Specinfo {
    /// `si_hashchain`: the `speclisth` bucket the vnode is on.
    pub si_hashchain: Cell<Option<&'static Vnodechain>>,
    /// `si_mountpoint`: the file system mounted from this device.
    pub si_mountpoint: Cell<Option<&'static Mount>>,
    /// `si_rdev`: the device.
    pub si_rdev: Cell<Dev>,
    /// `si_lockf`: advisory locks.
    pub si_lockf: LockfStateSlot,
    /// `si_lastr`: last read block, for read-ahead.
    pub si_lastr: Cell<Daddr>,
    /// `si_ci.ci_parent`: pointer back to parent device (a clone).
    pub si_ci_parent: Cell<Option<&'static Vnode>>,
    /// `si_ci.ci_bitmap`: bitmap of devices cloned off us (`CLONE_MAPSZ` bytes).
    pub si_ci_bitmap: Cell<*mut u8>,
}

impl Specinfo {
    /// A zeroed `struct specinfo`.
    pub const fn new() -> Self {
        Self {
            si_hashchain: Cell::new(None),
            si_mountpoint: Cell::new(None),
            si_rdev: Cell::new(0),
            si_lockf: Cell::new(None),
            si_lastr: Cell::new(0),
            si_ci_parent: Cell::new(None),
            si_ci_bitmap: Cell::new(ptr::null_mut()),
        }
    }
}

impl Default for Specinfo {
    fn default() -> Self {
        Self::new()
    }
}

/// `struct cloneinfo`.
pub struct Cloneinfo {
    /// `ci_vp`: cloned vnode.
    pub ci_vp: &'static Vnode,
    /// `ci_data`: original vnode's `v_data`.
    pub ci_data: *mut c_void,
}

/// `SPECHASH(rdev)`: the `speclisth` bucket of a device.
pub const fn spechash(rdev: Dev) -> usize {
    ((rdev >> 5).wrapping_add(rdev) as usize) & (SPECHSZ - 1)
}

const _: () = assert!(SPECHSZ & (SPECHSZ - 1) == 0);
/* </CODE> */
