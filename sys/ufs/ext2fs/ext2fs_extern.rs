/*	$OpenBSD: ext2fs_extern.h,v 1.40 2025/07/07 00:55:15 jsg Exp $	*/
/*	$NetBSD: ext2fs_extern.h,v 1.1 1997/06/11 09:33:55 bouyer Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (c) 1997 Manuel Bouyer.
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
 *	@(#)ffs_extern.h	8.3 (Berkeley) 4/16/94
 * Modified for ext2fs by Manuel Bouyer.
 */
/* </LICENSES> */

//! `<ufs/ext2fs/ext2fs_extern.h>`: the prototypes of the functions of `ufs/ext2fs`, the pools
//! and operation tables they share, and `IS_EXT2_VNODE`.
//!
//! Upstream: sys/ufs/ext2fs/ext2fs_extern.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - Like `ffs_extern.rs` and `ufs_extern.rs`, the header's prototypes are the functions of
//!   the `ext2fs_*.rs` files; the file that ports each of them adds its `pub use` here, so
//!   that `use crate::ufs::ext2fs::ext2fs_extern::*` reads like the C's `#include`. The items
//!   the header declares are: `ext2fs_inode_pool` and `ext2fs_dinode_pool` (the pools of
//!   `ext2fs_vfsops.c`), `ext2fs_vops`, `ext2fs_specvops` and `ext2fs_fifovops` (`ext2fs_vnops.c`;
//!   the last waits for `miscfs/fifofs`, `option FIFO`), and, by file: `ext2fs_alloc.c`
//!   (`ext2fs_alloc`, `ext2fs_inode_alloc`, `ext2fs_blkpref`, `ext2fs_blkfree`,
//!   `ext2fs_inode_free`), `ext2fs_balloc.c` (`ext2fs_buf_alloc`), `ext2fs_bmap.c`
//!   (`ext2fs_bmap`), `ext2fs_inode.c` (`ext2fs_size`, `ext2fs_init`, `ext2fs_setsize`,
//!   `ext2fs_update`, `ext2fs_truncate`, `ext2fs_inactive`), `ext2fs_lookup.c`
//!   (`ext2fs_readdir`, `ext2fs_lookup`, `ext2fs_direnter`, `ext2fs_dirremove`,
//!   `ext2fs_dirrewrite`, `ext2fs_dirempty`, `ext2fs_checkpath`), `ext2fs_subr.c`
//!   (`ext2fs_bufatoff`, `ext2fs_vinit`), `ext2fs_vfsops.c` (`ext2fs_mountroot`,
//!   `ext2fs_mount`, `ext2fs_reload`, `ext2fs_mountfs`, `ext2fs_unmount`,
//!   `ext2fs_flushfiles`, `ext2fs_statfs`, `ext2fs_sync`, `ext2fs_vget`, `ext2fs_fhtovp`,
//!   `ext2fs_vptofh`, `ext2fs_sbupdate`, `ext2fs_cgupdate`), `ext2fs_readwrite.c`
//!   (`ext2fs_read`, `ext2fs_write`) and `ext2fs_vnops.c` (`ext2fs_create` ...
//!   `ext2fs_makeinode`, `ext2fs_fsync`, `ext2fs_reclaim`, `ext2fsfifo_reclaim`).
//! - `IS_EXT2_VNODE(vp)` is [`is_ext2_vnode`].

use crate::sys::vnode::{VT_EXT2FS, Vnode};

/// `IS_EXT2_VNODE(vp)`: whether the vnode belongs to ext2fs.
pub fn is_ext2_vnode(vp: &Vnode) -> bool {
    vp.v_tag.get() == VT_EXT2FS
}
