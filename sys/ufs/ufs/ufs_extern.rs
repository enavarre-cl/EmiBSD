/*	$OpenBSD: ufs_extern.h,v 1.41 2024/07/07 01:39:06 jsg Exp $	*/
/*	$NetBSD: ufs_extern.h,v 1.5 1996/02/09 22:36:03 christos Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (c) 1991, 1993, 1994
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
 *	@(#)ufs_extern.h	8.6 (Berkeley) 8/10/94
 */
/* </LICENSES> */

//! `<ufs/ufs/ufs_extern.h>`: the prototypes of the UFS layer's functions, which the file
//! systems on it (`ffs`, `mfs`, `ext2fs`) call and put in their operation tables.
//!
//! Upstream: sys/ufs/ufs/ufs_extern.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - The header declares no types; its prototypes are the functions of `ufs_bmap.rs`,
//!   `ufs_ihash.rs`, `ufs_inode.rs`, `ufs_lookup.rs`, `ufs_vfsops.rs` and `ufs_vnops.rs`,
//!   re-exported here so that `use crate::ufs::ufs::ufs_extern::*` reads like the C's
//!   `#include`. `ufs_init`, which the header lists under `ufs_inode.c`, is defined in
//!   `ufs_vfsops.c` (and `ufs_vfsops.rs`).
//! - The `FIFO` prototypes (`ufsfifo_read`, `ufsfifo_write`, `ufsfifo_close`) wait for
//!   `miscfs/fifofs` (`ufs_vnops.rs`).

pub use crate::ufs::ufs::quota::ufs_quotactl;
pub use crate::ufs::ufs::ufs_bmap::{ufs_bmap, ufs_bmaparray, ufs_getlbns};
pub use crate::ufs::ufs::ufs_ihash::{ufs_ihashget, ufs_ihashinit, ufs_ihashins, ufs_ihashrem};
pub use crate::ufs::ufs::ufs_inode::{ufs_inactive, ufs_reclaim};
pub use crate::ufs::ufs::ufs_lookup::{
    ufs_checkpath, ufs_dirbad, ufs_dirbadentry, ufs_dirempty, ufs_direnter, ufs_dirremove,
    ufs_dirrewrite, ufs_lookup, ufs_makedirentry,
};
pub use crate::ufs::ufs::ufs_vfsops::{
    ufs_check_export, ufs_fhtovp, ufs_init, ufs_root, ufs_start,
};
pub use crate::ufs::ufs::ufs_vnops::{
    ufs_access, ufs_advlock, ufs_close, ufs_create, ufs_getattr, ufs_ioctl, ufs_islocked,
    ufs_itimes, ufs_kqfilter, ufs_link, ufs_lock, ufs_makeinode, ufs_mkdir, ufs_mknod, ufs_open,
    ufs_pathconf, ufs_print, ufs_readdir, ufs_readlink, ufs_remove, ufs_rename, ufs_rmdir,
    ufs_setattr, ufs_strategy, ufs_symlink, ufs_unlock, ufsspec_close, ufsspec_read, ufsspec_write,
};
