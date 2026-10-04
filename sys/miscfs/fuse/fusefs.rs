/* $OpenBSD: fusefs.h,v 1.17 2026/06/20 13:45:13 helg Exp $ */
/* <LICENSES> */
/*
 * Copyright (c) 2012-2013 Sylvestre Gallon <ccna.syl@gmail.com>
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
/* </LICENSES> */

//! `fusefs.h`: the FUSE file system's sysctl identifiers, its per-mount structure and the
//! operations a daemon turned out not to implement.
//!
//! Upstream: sys/miscfs/fuse/fusefs.h @ 3ce1f3f79392
//!
//! The prototypes of the header are functions of the files that define them:
//! `fusefs_vops` (`fuse_vnops.rs`, `FUSEFS_VOPS`), `fusefs_fbuf_pool` (`fuse_vfsops.rs`,
//! `FUSEFS_FBUF_POOL`), `fusefs_file_open`/`fusefs_file_close` (`fuse_file.rs`) and the
//! device helpers `fuse_device_cleanup`, `fuse_device_queue_fbuf`, `fuse_device_set_fmp`
//! (`fuse_device.rs`).
//!
//! ## Deviations
//! - `FUSE_DEBUG` is not defined (as in GENERIC), so `DPRINTF` expands to nothing and every
//!   `DPRINTF` of the `.c` files is a comment.
//! - `struct fusefs_mnt`'s members that change after the mount (`undef_op`, `max_write`,
//!   `sess_init`) are `Cell`s; the others are plain.

use core::cell::Cell;

use crate::kern::subr_prf::panic;
use crate::sys::mount::Mount;
use crate::sys::sysctl::{CTLTYPE_INT, Ctlname};
use crate::sys::types::Dev;

/// `FUSEFS_OPENDEVS`: # of fuse devices opened.
pub const FUSEFS_OPENDEVS: i32 = 1;
/// `FUSEFS_INFBUFS`: # of in fbufs.
pub const FUSEFS_INFBUFS: i32 = 2;
/// `FUSEFS_WAITFBUFS`: # of fbufs waiting for a response.
pub const FUSEFS_WAITFBUFS: i32 = 3;
/// `FUSEFS_POOL_NBPAGES`: # total fusefs size.
pub const FUSEFS_POOL_NBPAGES: i32 = 4;
/// `FUSEFS_MAXID`: number of valid fusefs ids.
pub const FUSEFS_MAXID: usize = 5;

/// `FUSEFS_NAMES`.
pub const FUSEFS_NAMES: [Ctlname; FUSEFS_MAXID] = [
    Ctlname::NONE,
    Ctlname::new(b"fusefs_open_devices", CTLTYPE_INT),
    Ctlname::new(b"fusefs_fbufs_in", CTLTYPE_INT),
    Ctlname::new(b"fusefs_fbufs_wait", CTLTYPE_INT),
    Ctlname::new(b"fusefs_pool_pages", CTLTYPE_INT),
];

/// `UNDEF_ACCESS`: the daemon does not implement access.
pub const UNDEF_ACCESS: u32 = 1 << 0;
/// `UNDEF_MKDIR`.
pub const UNDEF_MKDIR: u32 = 1 << 1;
/// `UNDEF_CREATE`.
pub const UNDEF_CREATE: u32 = 1 << 2;
/// `UNDEF_LINK`.
pub const UNDEF_LINK: u32 = 1 << 3;
/// `UNDEF_READLINK`.
pub const UNDEF_READLINK: u32 = 1 << 4;
/// `UNDEF_RMDIR`.
pub const UNDEF_RMDIR: u32 = 1 << 5;
/// `UNDEF_REMOVE`.
pub const UNDEF_REMOVE: u32 = 1 << 6;
/// `UNDEF_SETATTR`.
pub const UNDEF_SETATTR: u32 = 1 << 7;
/// `UNDEF_RENAME`.
pub const UNDEF_RENAME: u32 = 1 << 8;
/// `UNDEF_SYMLINK`.
pub const UNDEF_SYMLINK: u32 = 1 << 9;
/// `UNDEF_MKNOD`.
pub const UNDEF_MKNOD: u32 = 1 << 10;
/// `UNDEF_FLUSH`.
pub const UNDEF_FLUSH: u32 = 1 << 11;
/// `UNDEF_FSYNC`.
pub const UNDEF_FSYNC: u32 = 1 << 12;

/// `struct fusefs_mnt`: a mounted FUSE file system, `malloc(M_FUSEFS)`ed by `fusefs_mount`
/// and freed by `fusefs_unmount`.
pub struct FusefsMnt {
    /// `mp`: the mount.
    pub mp: &'static Mount,
    /// `undef_op`: the `UNDEF_*` operations the daemon answered `ENOSYS` to.
    pub undef_op: Cell<u32>,
    /// `max_read`: the largest read the daemon accepts (the mount's `max_read` option, at
    /// most `FUSEBUFMAXSIZE`).
    pub max_read: i32,
    /// `max_write`: the largest write sent to the daemon (its `FUSE_INIT` reply).
    pub max_write: Cell<i32>,
    /// `sess_init`: 0 before `fusefs_mount` and after the device closed or the unmount,
    /// `PENDING` until the daemon answered `FUSE_INIT`, 1 afterwards.
    pub sess_init: Cell<i32>,
    /// `allow_other`: whether users other than the one who mounted may use the file system.
    pub allow_other: i32,
    /// `dev`: the fuse(4) device (clone) the daemon talks through.
    pub dev: Dev,
}

// SAFETY: changed under the kernel lock (one CPU), as in C.
unsafe impl Sync for FusefsMnt {}

/// `VFSTOFUSEFS(mp)`: the FUSE mount of a mount.
#[allow(non_snake_case)] // the C name
pub fn VFSTOFUSEFS(mp: &Mount) -> &'static FusefsMnt {
    let fmp = mp.mnt_data.get();
    if fmp.is_null() {
        panic(format_args!(
            "VFSTOFUSEFS: mount {:p} has no fusefs_mnt",
            mp
        ));
    }
    // SAFETY: a FUSE mount's `mnt_data` is the `FusefsMnt` `fusefs_mount` allocated, which
    // lives until `fusefs_unmount` frees it and clears `mnt_data`; the caller holds the
    // mount busy or a vnode of it.
    unsafe { &*fmp.cast::<FusefsMnt>() }
}
