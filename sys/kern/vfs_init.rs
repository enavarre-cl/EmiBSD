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
 * Copyright (c) 1989, 1993
 *	The Regents of the University of California.  All rights reserved.
 *
 * This code is derived from software contributed
 * to Berkeley by John Heidemann of the UCLA Ficus project.
 *
 * Source: * @(#)i405_init.c 2.10 92/04/27 UCLA Ficus project
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
 *	@(#)vfs_init.c	8.3 (Berkeley) 1/4/94
 */
/* </LICENSES> */

/* <CODE> */
//! File system initialisation: the configured file system types (`vfsconflist[]`), `vfsinit`
//! (the `namei` buffer pool, the vnode table, the name cache, each type's `vfs_init`), the
//! root vnode and the lookups by type name and number.
//!
//! Upstream: sys/kern/vfs_init.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `vfsconflist[]` holds the ported file systems in `vfs_init.c`'s order: `ffs` (feature `ffs`,
//!   `option FFS`), `mfs` (feature `mfs`, `option MFS`), `ext2fs` (feature `ext2fs`, `option
//!   EXT2FS`), `cd9660` (feature `cd9660`, `option CD9660`), `msdos` (feature `msdosfs`, `option
//!   MSDOSFS`), `nfs` (feature `nfsclient`, `option NFSCLIENT`), `ntfs` (feature `ntfs`, `option
//!   NTFS`, in amd64's GENERIC only: cfg `option_ntfs`, `sys/build.rs`), `udf` (feature `udf`,
//!   `option UDF`), `fuse` (feature `fuse`, `option FUSE`), `tmpfs` (feature `tmpfs`, `option
//!   TMPFS`, commented out in GENERIC). A new entry joins as a `Vfsconf::new(...)` line behind a
//!   cargo feature named after the `option(4)`.
//! - `rootvnode` is an `AtomicPtr` behind [`rootvnode`]/[`set_rootvnode`]; `maxvfsconf` is an
//!   `AtomicI32`.
//! - `vfs_byname` takes the name as bytes (`&[u8]`, NUL or slice end terminated).

use core::ptr;
use core::sync::atomic::{AtomicI32, AtomicPtr, Ordering};

use crate::kern::subr_pool::pool_init;
use crate::kern::vfs_cache::nchinit;
use crate::kern::vfs_subr::vntblinit;
use crate::machine::intr::IPL_NONE;
use crate::sys::mount::Vfsconf;
use crate::sys::param::MAXPATHLEN;
use crate::sys::pool::{PR_WAITOK, Pool};
use crate::sys::vnode::Vnode;

/// `namei_pool`: the `MAXPATHLEN` pathname buffers of `namei`.
pub static NAMEI_POOL: Pool = Pool::new();

/// `rootvnode`: this defines the root filesystem.
static ROOTVNODE: AtomicPtr<Vnode> = AtomicPtr::new(ptr::null_mut());

/// `vfsconflist[]`: the filesystem types this kernel is configured with (see the module's
/// deviations).
static VFSCONFLIST: [Vfsconf; NVFSCONF] = [
    #[cfg(feature = "ffs")]
    Vfsconf::new(
        &crate::ufs::ffs::ffs_vfsops::FFS_VFSOPS,
        crate::sys::mount::MOUNT_FFS,
        1,
        crate::sys::mount::MNT_LOCAL | crate::sys::mount::MNT_SWAPPABLE,
        crate::sys::mount::UfsArgs::SIZE,
    ),
    #[cfg(feature = "mfs")]
    Vfsconf::new(
        &crate::ufs::mfs::mfs_vfsops::MFS_VFSOPS,
        crate::sys::mount::MOUNT_MFS,
        3,
        crate::sys::mount::MNT_LOCAL,
        crate::sys::mount::MfsArgs::SIZE,
    ),
    #[cfg(feature = "ext2fs")]
    Vfsconf::new(
        &crate::ufs::ext2fs::ext2fs_vfsops::EXT2FS_VFSOPS,
        crate::sys::mount::MOUNT_EXT2FS,
        17,
        crate::sys::mount::MNT_LOCAL | crate::sys::mount::MNT_SWAPPABLE,
        crate::sys::mount::UfsArgs::SIZE,
    ),
    #[cfg(feature = "cd9660")]
    Vfsconf::new(
        &crate::isofs::cd9660::cd9660_vfsops::CD9660_VFSOPS,
        crate::sys::mount::MOUNT_CD9660,
        14,
        crate::sys::mount::MNT_LOCAL,
        crate::sys::mount::IsoArgs::SIZE,
    ),
    #[cfg(feature = "msdosfs")]
    Vfsconf::new(
        &crate::msdosfs::msdosfs_vfsops::MSDOSFS_VFSOPS,
        crate::sys::mount::MOUNT_MSDOS,
        4,
        crate::sys::mount::MNT_LOCAL | crate::sys::mount::MNT_SWAPPABLE,
        crate::sys::mount::MsdosfsArgs::SIZE,
    ),
    #[cfg(feature = "nfsclient")]
    Vfsconf::new(
        &crate::nfs::nfs_vfsops::NFS_VFSOPS,
        crate::sys::mount::MOUNT_NFS,
        2,
        crate::sys::mount::MNT_SWAPPABLE,
        crate::sys::mount::NfsArgs::SIZE,
    ),
    #[cfg(option_ntfs)]
    Vfsconf::new(
        &crate::ntfs::ntfs_vfsops::NTFS_VFSOPS,
        crate::sys::mount::MOUNT_NTFS,
        6,
        crate::sys::mount::MNT_LOCAL,
        crate::sys::mount::NtfsArgs::SIZE,
    ),
    #[cfg(feature = "udf")]
    Vfsconf::new(
        &crate::isofs::udf::udf_vfsops::UDF_VFSOPS,
        crate::sys::mount::MOUNT_UDF,
        13,
        crate::sys::mount::MNT_LOCAL,
        crate::sys::mount::UdfArgs::SIZE,
    ),
    #[cfg(feature = "fuse")]
    Vfsconf::new(
        &crate::miscfs::fuse::fuse_vfsops::FUSEFS_VFSOPS,
        crate::sys::mount::MOUNT_FUSEFS,
        18,
        0,
        crate::sys::mount::FusefsArgs::SIZE,
    ),
    #[cfg(feature = "tmpfs")]
    Vfsconf::new(
        &crate::tmpfs::tmpfs_vfsops::TMPFS_VFSOPS,
        crate::sys::mount::MOUNT_TMPFS,
        19,
        crate::sys::mount::MNT_LOCAL,
        crate::sys::mount::TmpfsArgs::SIZE,
    ),
];

/// The number of entries of `vfsconflist[]`: one per configured file system.
const NVFSCONF: usize = cfg!(feature = "ffs") as usize
    + cfg!(feature = "mfs") as usize
    + cfg!(feature = "ext2fs") as usize
    + cfg!(feature = "cd9660") as usize
    + cfg!(feature = "msdosfs") as usize
    + cfg!(feature = "nfsclient") as usize
    + cfg!(option_ntfs) as usize
    + cfg!(feature = "udf") as usize
    + cfg!(feature = "fuse") as usize
    + cfg!(feature = "tmpfs") as usize;

/// `maxvfsconf`: initially the size of the list, `vfsinit` will set it to the highest defined
/// type number.
pub static MAXVFSCONF: AtomicI32 = AtomicI32::new(VFSCONFLIST.len() as i32);

/// `rootvnode`: the root (i.e. "/") vnode, `None` until a root file system is mounted.
pub fn rootvnode() -> Option<&'static Vnode> {
    // SAFETY: only `set_rootvnode` stores here, and only vnodes, which are never freed.
    unsafe { ROOTVNODE.load(Ordering::Acquire).as_ref() }
}

/// `rootvnode = vp`.
pub fn set_rootvnode(vp: Option<&'static Vnode>) {
    let p = vp.map_or(ptr::null_mut(), |vp| ptr::from_ref(vp).cast_mut());
    ROOTVNODE.store(p, Ordering::Release);
}

/// Initialize the vnode structures and initialize each file system type.
pub fn vfsinit() {
    pool_init(
        &NAMEI_POOL,
        MAXPATHLEN,
        0,
        IPL_NONE,
        PR_WAITOK,
        "namei",
        None,
    );

    // Initialize the vnode table.
    vntblinit();

    // Initialize the vnode name cache.
    nchinit();

    let mut maxvfsconf = 0;
    for vfsp in VFSCONFLIST.iter() {
        if vfsp.vfc_typenum > maxvfsconf {
            maxvfsconf = vfsp.vfc_typenum;
        }
        if let Some(init) = vfsp.vfc_vfsops.vfs_init {
            let _ = init(vfsp);
        }
    }
    MAXVFSCONF.store(maxvfsconf, Ordering::Relaxed);
}

/// `vfs_byname(name)`: the configured type called `name`.
pub fn vfs_byname(name: &[u8]) -> Option<&'static Vfsconf> {
    let name = name.split(|&c| c == 0).next().unwrap_or(&[]);
    VFSCONFLIST.iter().find(|vfsp| vfsp.name() == name)
}

/// `vfs_bytypenum(typenum)`: the configured type numbered `typenum`.
pub fn vfs_bytypenum(typenum: i32) -> Option<&'static Vfsconf> {
    VFSCONFLIST.iter().find(|vfsp| vfsp.vfc_typenum == typenum)
}
/* </CODE> */
