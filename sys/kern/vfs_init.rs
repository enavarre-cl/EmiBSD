/*	$OpenBSD: vfs_init.c,v 1.44 2024/05/20 09:11:21 mvs Exp $	*/
/*	$NetBSD: vfs_init.c,v 1.6 1996/02/09 19:00:58 christos Exp $	*/
/* <LICENSES> */
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

//! File system initialisation: the configured file system types (`vfsconflist[]`), `vfsinit`
//! (the `namei` buffer pool, the vnode table, the name cache, each type's `vfs_init`), the
//! root vnode and the lookups by type name and number.
//!
//! Upstream: sys/kern/vfs_init.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `vfsconflist[]` is empty: no file system is ported yet (the buffer cache and the vnode
//!   pager are there for the first). Each GENERIC entry (`FFS`, `MFS`, `EXT2FS`,
//!   `CD9660`, `MSDOSFS`, `NFSCLIENT`, `NTFS`, `UDF`, `FUSE`, `TMPFS`) joins as a
//!   `Vfsconf::new(...)` line when its file system does, behind a cargo feature named after
//!   the `option(4)`.
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
static VFSCONFLIST: [Vfsconf; 0] = [];

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
