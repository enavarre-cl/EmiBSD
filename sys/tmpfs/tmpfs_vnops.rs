/*	$OpenBSD: tmpfs_vnops.h,v 1.7 2022/06/26 05:20:42 visa Exp $	*/
/*	$NetBSD: tmpfs_vnops.h,v 1.13 2011/05/24 20:17:49 rmind Exp $	*/
/*	$OpenBSD: tmpfs_vnops.c,v 1.57 2025/09/20 13:53:36 mpi Exp $	*/
/*	$NetBSD: tmpfs_vnops.c,v 1.100 2012/11/05 17:27:39 dholland Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 2005, 2006 The NetBSD Foundation, Inc.
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

/*
 * Copyright (c) 2005, 2006, 2007, 2012 The NetBSD Foundation, Inc.
 * Copyright (c) 2013 Pedro Martelletto
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Julio M. Merino Vidal, developed as part of Google's Summer of Code
 * 2005 program, and by Taylor R Campbell.
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

//! `<tmpfs/tmpfs_vnops.h>` / `tmpfs_vnops.c`: the tmpfs vnode interface, the operations
//! vector used for files stored in a tmpfs file system (`tmpfs_vops`) and its operations.
//!
//! Upstream: sys/tmpfs/tmpfs_vnops.h @ 3ce1f3f79392
//! Upstream: sys/tmpfs/tmpfs_vnops.c @ 3ce1f3f79392 (in part, see below)
//!
//! M10C-PENDING: tmpfs_vnops.c. This file holds the header (the declarations are the
//! `pub` items here and in `tmpfs_specops.rs`/`tmpfs_fifoops.rs`) and the part of
//! `tmpfs_vnops.c` that the core of tmpfs needs to get, lock and drop vnodes:
//! `tmpfs_inactive`, `tmpfs_reclaim`, `tmpfs_lock`, `tmpfs_unlock`, `tmpfs_islocked`,
//! `tmpfs_print`, `tmpfs_bwrite`, `tmpfs_strategy` and `tmpfs_ioctl`, ported in full. The
//! rest of `tmpfs_vnops.c` is ported next, into this file:
//! - `tmpfs_lookup`, `tmpfs_create`, `tmpfs_mknod`, `tmpfs_open`, `tmpfs_close`,
//!   `tmpfs_read`, `tmpfs_write`, `tmpfs_fsync`, `tmpfs_remove`, `tmpfs_link`,
//!   `tmpfs_rename` (and its helpers), `tmpfs_mkdir`, `tmpfs_rmdir`, `tmpfs_symlink`,
//!   `tmpfs_readdir`, `tmpfs_readlink`, `tmpfs_pathconf`, `tmpfs_advlock`,
//!   `tmpfs_kqfilter`: their `tmpfs_vops` slots are `None` meanwhile (`EOPNOTSUPP` from
//!   the `VOP_*` wrappers);
//! - `tmpfs_access`, `tmpfs_getattr`, `tmpfs_setattr`: visible stubs (`unported!`,
//!   `ENOSYS`) marked `M10C-SHIM`, because the special-device and fifo tables of
//!   `tmpfs_specops.c` and `tmpfs_fifoops.c` name them.
//!
//! ## Deviations
//! - `tmpfs_print`'s `fifo_printinfo` (`option FIFO`) waits for `miscfs/fifofs`.
//! - The helpers the C installs in many slots are the `vfs_default.rs` functions
//!   (`vop_generic_*`), as in `spec_vops`.

use crate::kern::kern_rwlock::{rrw_enter, rrw_exit, rrw_status, rw_enter_write, rw_exit_write};
use crate::kern::vfs_cache::cache_purge;
use crate::kern::vfs_default::{vop_generic_abortop, vop_generic_bmap, vop_generic_revoke};
use crate::kern::vfs_subr::vrecycle;
use crate::kern::vfs_vops::{VOP_ISLOCKED, VOP_UNLOCK};
use crate::machine::cpu::curproc;
use crate::sys::errno::Errno;
use crate::sys::lock::LK_RWFLAGS;
use crate::sys::vnode::{
    VREG, VopAccessArgs, VopBwriteArgs, VopGetattrArgs, VopInactiveArgs, VopIoctlArgs,
    VopIslockedArgs, VopLockArgs, VopPrintArgs, VopReclaimArgs, VopSetattrArgs, VopStrategyArgs,
    VopUnlockArgs, Vops,
};
use crate::tmpfs::tmpfs::{VFS_TO_TMPFS, VP_TO_TMPFS_NODE, tmpfs_node_reclaiming};
use crate::tmpfs::tmpfs_subr::{tmpfs_free_node, tmpfs_uio_cached, tmpfs_uio_uncache};
use crate::{kassert, unported};

/// `tmpfs_vops`: vnode operations vector used for files stored in a tmpfs file system.
///
/// M10C-PENDING: tmpfs_vnops.c fills the `None` slots (see the module's documentation).
pub static TMPFS_VOPS: Vops = Vops {
    vop_lookup: None, // M10C-PENDING: tmpfs_lookup
    vop_create: None, // M10C-PENDING: tmpfs_create
    vop_mknod: None,  // M10C-PENDING: tmpfs_mknod
    vop_open: None,   // M10C-PENDING: tmpfs_open
    vop_close: None,  // M10C-PENDING: tmpfs_close
    vop_access: Some(tmpfs_access),
    vop_getattr: Some(tmpfs_getattr),
    vop_setattr: Some(tmpfs_setattr),
    vop_read: None,  // M10C-PENDING: tmpfs_read
    vop_write: None, // M10C-PENDING: tmpfs_write
    vop_ioctl: Some(tmpfs_ioctl),
    vop_kqfilter: None, // M10C-PENDING: tmpfs_kqfilter
    vop_revoke: Some(vop_generic_revoke),
    vop_fsync: None,    // M10C-PENDING: tmpfs_fsync
    vop_remove: None,   // M10C-PENDING: tmpfs_remove
    vop_link: None,     // M10C-PENDING: tmpfs_link
    vop_rename: None,   // M10C-PENDING: tmpfs_rename
    vop_mkdir: None,    // M10C-PENDING: tmpfs_mkdir
    vop_rmdir: None,    // M10C-PENDING: tmpfs_rmdir
    vop_symlink: None,  // M10C-PENDING: tmpfs_symlink
    vop_readdir: None,  // M10C-PENDING: tmpfs_readdir
    vop_readlink: None, // M10C-PENDING: tmpfs_readlink
    vop_abortop: Some(vop_generic_abortop),
    vop_inactive: Some(tmpfs_inactive),
    vop_reclaim: Some(tmpfs_reclaim),
    vop_lock: Some(tmpfs_lock),
    vop_unlock: Some(tmpfs_unlock),
    vop_bmap: Some(vop_generic_bmap),
    vop_strategy: Some(tmpfs_strategy),
    vop_print: Some(tmpfs_print),
    vop_islocked: Some(tmpfs_islocked),
    vop_pathconf: None, // M10C-PENDING: tmpfs_pathconf
    vop_advlock: None,  // M10C-PENDING: tmpfs_advlock
    vop_bwrite: Some(tmpfs_bwrite),
};

/// `tmpfs_access` (`vop_access`). M10C-PENDING: tmpfs_vnops.c.
// M10C-SHIM: replaced by the port of tmpfs_vnops.c.
pub fn tmpfs_access(_ap: &mut VopAccessArgs<'_>) -> Result<(), Errno> {
    Err(unported!("tmpfs_access (tmpfs_vnops.c)"))
}

/// `tmpfs_getattr` (`vop_getattr`). M10C-PENDING: tmpfs_vnops.c.
// M10C-SHIM: replaced by the port of tmpfs_vnops.c.
pub fn tmpfs_getattr(_ap: &mut VopGetattrArgs<'_>) -> Result<(), Errno> {
    Err(unported!("tmpfs_getattr (tmpfs_vnops.c)"))
}

/// `tmpfs_setattr` (`vop_setattr`). M10C-PENDING: tmpfs_vnops.c.
// M10C-SHIM: replaced by the port of tmpfs_vnops.c.
pub fn tmpfs_setattr(_ap: &mut VopSetattrArgs<'_>) -> Result<(), Errno> {
    Err(unported!("tmpfs_setattr (tmpfs_vnops.c)"))
}

/// `tmpfs_inactive` (`vop_inactive`): the last use of the vnode went; drop the cached
/// mapping of a regular file and, if the node has no links left, reclaim the vnode at once
/// so that the node can be freed and reused immediately.
pub fn tmpfs_inactive(ap: &mut VopInactiveArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;

    kassert!(VOP_ISLOCKED(vp) != 0);

    let node = VP_TO_TMPFS_NODE(vp);

    if vp.v_type.get() == VREG && tmpfs_uio_cached(node) {
        tmpfs_uio_uncache(node);
    }

    let _ = VOP_UNLOCK(vp);

    // If we are done with the node, reclaim it so that it can be reused immediately.
    if node.tn_links.get() == 0 {
        let _ = vrecycle(vp, curproc());
    }

    Ok(())
}

/// `tmpfs_reclaim` (`vop_reclaim`): disassociate the node from the vnode, and destroy the
/// node if it has no links (unless `tmpfs_vnode_get` is about to give it a new vnode).
pub fn tmpfs_reclaim(ap: &mut VopReclaimArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let Some(mp) = vp.v_mount.get() else {
        crate::kern::subr_prf::panic(format_args!("tmpfs_reclaim: vnode {:p} has no mount", vp));
    };
    let tmp = VFS_TO_TMPFS(mp);
    let node = VP_TO_TMPFS_NODE(vp);

    // Disassociate inode from vnode.
    rw_enter_write(&node.tn_nlock);
    node.tn_vnode.set(None);
    vp.v_data.set(core::ptr::null_mut());
    // Check if tmpfs_vnode_get() is racing with us.
    let racing = tmpfs_node_reclaiming(node);
    rw_exit_write(&node.tn_nlock);

    cache_purge(vp);

    // If inode is not referenced, i.e. no links, then destroy it. Note: if racing - inode
    // is about to get a new vnode, leave it.
    if node.tn_links.get() == 0 && !racing {
        tmpfs_free_node(tmp, node);
    }
    Ok(())
}

/// `tmpfs_print` (`vop_print`): describe the node (`DEBUG`/`DIAGNOSTIC` kernels only).
pub fn tmpfs_print(ap: &mut VopPrintArgs) -> Result<(), Errno> {
    #[cfg(any(feature = "debug", feature = "diagnostic"))]
    {
        let vp = ap.a_vp;
        let node = VP_TO_TMPFS_NODE(vp);

        crate::kern::subr_prf::printf(format_args!(
            "tag VT_TMPFS, tmpfs_node {:p}, flags 0x{:x}, links {}\n\tmode 0{:o}, owner {}, group {}, size {}",
            node,
            node.tn_flags.get(),
            node.tn_links.get(),
            node.tn_mode.get(),
            node.tn_uid.get(),
            node.tn_gid.get(),
            node.tn_size.get()
        ));
        // FIFO: fifo_printinfo(vp) for a VFIFO (miscfs/fifofs, not ported).
        crate::kern::subr_prf::printf(format_args!("\n"));
    }
    #[cfg(not(any(feature = "debug", feature = "diagnostic")))]
    let _ = ap;
    Ok(())
}

/// `tmpfs_bwrite` (`vop_bwrite`): a null op.
pub fn tmpfs_bwrite(_ap: &mut VopBwriteArgs) -> Result<(), Errno> {
    Ok(())
}

/// `tmpfs_strategy` (`vop_strategy`): tmpfs has no buffers.
pub fn tmpfs_strategy(_ap: &mut VopStrategyArgs) -> Result<(), Errno> {
    Err(Errno::EOPNOTSUPP)
}

/// `tmpfs_ioctl` (`vop_ioctl`).
pub fn tmpfs_ioctl(_ap: &mut VopIoctlArgs<'_>) -> Result<(), Errno> {
    Err(Errno::ENOTTY)
}

/// `tmpfs_lock` (`vop_lock`): take the node's vnode lock.
pub fn tmpfs_lock(ap: &mut VopLockArgs) -> Result<(), Errno> {
    let tnp = VP_TO_TMPFS_NODE(ap.a_vp);

    rrw_enter(&tnp.tn_vlock, ap.a_flags & LK_RWFLAGS)
}

/// `tmpfs_unlock` (`vop_unlock`).
pub fn tmpfs_unlock(ap: &mut VopUnlockArgs) -> Result<(), Errno> {
    let tnp = VP_TO_TMPFS_NODE(ap.a_vp);

    rrw_exit(&tnp.tn_vlock);
    Ok(())
}

/// `tmpfs_islocked` (`vop_islocked`).
pub fn tmpfs_islocked(ap: &mut VopIslockedArgs) -> i32 {
    let tnp = VP_TO_TMPFS_NODE(ap.a_vp);

    rrw_status(&tnp.tn_vlock)
}
