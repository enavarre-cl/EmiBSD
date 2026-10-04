/*	$OpenBSD: tmpfs_fifoops.c,v 1.9 2024/05/14 08:26:13 jsg Exp $	*/
/*	$NetBSD: tmpfs_fifoops.c,v 1.9 2011/05/24 20:17:49 rmind Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 2005 The NetBSD Foundation, Inc.
 * Copyright (c) 2013 Pedro Martelletto
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Julio M. Merino Vidal, developed as part of Google's Summer of Code
 * 2005 program.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 *
 * THIS SOFTWARE IS PROVIDED BY THE NETBSD FOUNDATION, INC. AND CONTRIBUTORS
 * ``AS IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
 * TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
 * PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL THE FOUNDATION OR CONTRIBUTORS
 * BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
 * CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
 * SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
 * INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
 * CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
 * ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
 * POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

//! tmpfs vnode interface for named pipes: the operations of a fifo stored in a tmpfs file
//! system (`tmpfs_fifovops`), which keep the node's times and leave the pipe to fifofs.
//!
//! Upstream: sys/tmpfs/tmpfs_fifoops.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `option FIFO` is in GENERIC but `miscfs/fifofs` is not ported (the ffs precedent):
//!   the `fifo_*` operations the table installs (`fifo_open`, `fifo_close`, `fifo_ioctl`,
//!   `fifo_kqfilter`, `fifo_pathconf`, `fifo_advlock`) and the `fifo_read`/`fifo_write`
//!   that `tmpfs_fifo_read`/`tmpfs_fifo_write` end in are visible gaps (`unported!`,
//!   `ENOSYS`). Nothing reaches them yet: `tmpfs_vnode_get` refuses a fifo node with
//!   `EOPNOTSUPP` and never installs this table.
//! - The helpers the C installs in many slots (`vop_generic_badop`) are closures, as in
//!   `spec_vops` (`docs/C_TO_RUST.md`).
//! - `tmpfs_access`, `tmpfs_getattr` and `tmpfs_setattr` come from `tmpfs_vnops.c`, which
//!   is ported after this file (M10C-PENDING: tmpfs_vnops.c): until then they are the
//!   visible stubs of `tmpfs_vnops.rs`.

use crate::kern::vfs_default::{
    vop_generic_badop, vop_generic_bmap, vop_generic_lookup, vop_generic_revoke,
};
use crate::sys::errno::Errno;
use crate::sys::vnode::{VopFsyncArgs, VopReadArgs, VopWriteArgs, Vops};
use crate::tmpfs::tmpfs::{TMPFS_NODE_ACCESSED, TMPFS_NODE_MODIFIED, VP_TO_TMPFS_NODE};
use crate::tmpfs::tmpfs_subr::tmpfs_update;
use crate::tmpfs::tmpfs_vnops::{
    tmpfs_access, tmpfs_bwrite, tmpfs_getattr, tmpfs_inactive, tmpfs_islocked, tmpfs_lock,
    tmpfs_print, tmpfs_reclaim, tmpfs_setattr, tmpfs_unlock,
};
use crate::unported;

/// `tmpfs_fifovops`: vnode operations vector used for fifos stored in a tmpfs file system.
pub static TMPFS_FIFOVOPS: Vops = Vops {
    vop_access: Some(tmpfs_access),
    vop_getattr: Some(tmpfs_getattr),
    vop_setattr: Some(tmpfs_setattr),
    vop_read: Some(tmpfs_fifo_read),
    vop_write: Some(tmpfs_fifo_write),
    vop_fsync: Some(tmpfs_fifo_fsync),
    vop_inactive: Some(tmpfs_inactive),
    vop_reclaim: Some(tmpfs_reclaim),
    vop_lock: Some(tmpfs_lock),
    vop_unlock: Some(tmpfs_unlock),
    vop_print: Some(tmpfs_print),
    vop_islocked: Some(tmpfs_islocked),
    vop_bwrite: Some(tmpfs_bwrite),

    // XXX: Keep in sync with fifo_vops.
    vop_lookup: Some(vop_generic_lookup),
    vop_create: Some(|_| vop_generic_badop()),
    vop_mknod: Some(|_| vop_generic_badop()),
    vop_open: Some(|_| Err(unported!("fifo_open (miscfs/fifofs)"))),
    vop_close: Some(|_| Err(unported!("fifo_close (miscfs/fifofs)"))),
    vop_ioctl: Some(|_| Err(unported!("fifo_ioctl (miscfs/fifofs)"))),
    vop_kqfilter: Some(|_| Err(unported!("fifo_kqfilter (miscfs/fifofs)"))),
    vop_revoke: Some(vop_generic_revoke),
    vop_remove: Some(|_| vop_generic_badop()),
    vop_link: Some(|_| vop_generic_badop()),
    vop_rename: Some(|_| vop_generic_badop()),
    vop_mkdir: Some(|_| vop_generic_badop()),
    vop_rmdir: Some(|_| vop_generic_badop()),
    vop_symlink: Some(|_| vop_generic_badop()),
    vop_readdir: Some(|_| vop_generic_badop()),
    vop_readlink: Some(|_| vop_generic_badop()),
    vop_abortop: Some(|_| vop_generic_badop()),
    vop_bmap: Some(vop_generic_bmap),
    vop_strategy: Some(|_| vop_generic_badop()),
    vop_pathconf: Some(|_| Err(unported!("fifo_pathconf (miscfs/fifofs)"))),
    vop_advlock: Some(|_| Err(unported!("fifo_advlock (miscfs/fifofs)"))),
};

/// `tmpfs_fifo_read` (`vop_read`): mark the node accessed, then read the pipe.
pub fn tmpfs_fifo_read(ap: &mut VopReadArgs<'_, '_>) -> Result<(), Errno> {
    let vp = ap.a_vp;

    tmpfs_update(VP_TO_TMPFS_NODE(vp), TMPFS_NODE_ACCESSED);
    // fifo_read(v): miscfs/fifofs, not ported.
    Err(unported!("fifo_read (miscfs/fifofs)"))
}

/// `tmpfs_fifo_write` (`vop_write`): mark the node modified, then write the pipe.
pub fn tmpfs_fifo_write(ap: &mut VopWriteArgs<'_, '_>) -> Result<(), Errno> {
    let vp = ap.a_vp;

    tmpfs_update(VP_TO_TMPFS_NODE(vp), TMPFS_NODE_MODIFIED);
    // fifo_write(v): miscfs/fifofs, not ported.
    Err(unported!("fifo_write (miscfs/fifofs)"))
}

/// `tmpfs_fifo_fsync` (`vop_fsync`): nothing to flush.
pub fn tmpfs_fifo_fsync(_ap: &mut VopFsyncArgs<'_>) -> Result<(), Errno> {
    Ok(())
}
