/* <LICENSES> */
/*-
 * Copyright (c) 1994
 *	The Regents of the University of California.  All rights reserved.
 *
 * This code is derived from software contributed to Berkeley
 * by Pace Willisson (pace@blitz.com).  The Rock Ridge Extension
 * Support code is derived from software contributed to Berkeley
 * by Atsushi Murai (amurai@spec.co.jp).
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
 *	@(#)iso.h	8.4 (Berkeley) 12/5/94
 */
/* </LICENSES> */

/* <CODE> */
//! `<isofs/cd9660/cd9660_extern.h>`: definitions used in the kernel for cd9660 file system
//! support: the CD-ROM format type, the per-mount data (`struct iso_mnt`) and the block
//! arithmetic macros.
//!
//! Upstream: sys/isofs/cd9660/cd9660_extern.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - `im_export` (`struct netexport`) is kept whether or not `nfsserver` is configured, as the
//!   C does (`vfs_export` answers `ENOTSUP` without it), as `ufsmount.rs` does for UFS.
//! - `root_extent` and `root_size` are `u32`, as `isonum_733` reads them (the C's `int` only
//!   differs past 2^31 blocks).
//! - `struct iso_mnt` is built whole by `iso_mountfs` and not changed afterwards, so its
//!   members are plain fields.
//! - `blkoff`, `lblktosize`, `lblkno` and `blksize` are functions over `i64` offsets and
//!   blocks (the C's `off_t`/`daddr_t`).
//! - The prototypes are the functions of `cd9660_vfsops.rs`, `cd9660_util.rs`,
//!   `cd9660_node.rs` and `cd9660_vnops.rs`.

use crate::isofs::cd9660::iso::isodcl;
use crate::kern::subr_prf::panic;
use crate::sys::mount::{Mount, Netexport};
use crate::sys::types::Dev;
use crate::sys::vnode::Vnode;

/// `enum ISO_FTYPE`: CD-ROM format type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(non_camel_case_types, clippy::upper_case_acronyms)] // OpenBSD names, verbatim
pub enum IsoFtype {
    /// Plain ISO 9660 with version numbers stripped and associated files merged.
    ISO_FTYPE_DEFAULT,
    /// Plain ISO 9660 names as they are (`ISOFSMNT_GENS`).
    ISO_FTYPE_9660,
    /// Rock Ridge.
    ISO_FTYPE_RRIP,
    /// ECMA-168.
    ISO_FTYPE_ECMA,
}

pub use IsoFtype::{ISO_FTYPE_9660, ISO_FTYPE_DEFAULT, ISO_FTYPE_ECMA, ISO_FTYPE_RRIP};

/// `ISOFSMNT_ROOT`.
pub const ISOFSMNT_ROOT: i32 = 0;

/// `struct iso_mnt`: a mounted ISO 9660 file system.
pub struct IsoMnt {
    /// `im_flags`: the `ISOFSMNT_*` mount flags in effect.
    pub im_flags: i32,
    /// `im_mountp`.
    pub im_mountp: &'static Mount,
    /// `im_dev`.
    pub im_dev: Dev,
    /// `im_devvp`.
    pub im_devvp: &'static Vnode,
    /// `logical_block_size`.
    pub logical_block_size: i32,
    /// `im_bshift`.
    pub im_bshift: i32,
    /// `im_bmask`.
    pub im_bmask: i32,
    /// `volume_space_size`.
    pub volume_space_size: i32,
    /// `im_export`: export information.
    pub im_export: Netexport,
    /// `root`: a copy of the root directory record.
    pub root: [u8; isodcl(157, 190)],
    /// `root_extent`.
    pub root_extent: u32,
    /// `root_size`.
    pub root_size: u32,
    /// `iso_ftype`.
    pub iso_ftype: IsoFtype,
    /// `rr_skip`: the bytes to skip before the SUSP entries of a record.
    pub rr_skip: i32,
    /// `rr_skip0`: the same, for the root directory's `.` entry.
    pub rr_skip0: i32,
    /// `joliet_level`.
    pub joliet_level: i32,
}

/// `VFSTOISOFS(mp)`: the ISO 9660 data of a mounted cd9660 file system.
pub fn vfstoisofs(mp: &Mount) -> &'static IsoMnt {
    let data = mp.mnt_data.get();
    let cd9660 = mp.mnt_vfc.get().map(|vfc| vfc.name()) == Some(b"cd9660".as_slice());
    if data.is_null() || !cd9660 {
        panic(format_args!(
            "VFSTOISOFS: mount {:p} is not a mounted cd9660",
            mp
        ));
    }
    // SAFETY: a cd9660 mount's `mnt_data` (checked above) is the `struct iso_mnt` that
    // `iso_mountfs` allocated, which lives until `cd9660_unmount` frees it and clears
    // `mnt_data`; the caller holds the mount busy or a vnode of it.
    unsafe { &*data.cast::<IsoMnt>() }
}

/// `blkoff(imp, loc)`.
pub fn blkoff(imp: &IsoMnt, loc: i64) -> i64 {
    loc & i64::from(imp.im_bmask)
}

/// `lblktosize(imp, blk)`.
pub fn lblktosize(imp: &IsoMnt, blk: i64) -> i64 {
    blk << imp.im_bshift
}

/// `lblkno(imp, loc)`.
pub fn lblkno(imp: &IsoMnt, loc: i64) -> i64 {
    loc >> imp.im_bshift
}

/// `blksize(imp, ip, lbn)`: every block is a logical block.
pub fn blksize(imp: &IsoMnt) -> i32 {
    imp.logical_block_size
}
/* </CODE> */
