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

/* <CODE> */
//! `<tmpfs/tmpfs_vnops.h>` / `tmpfs_vnops.c`: the tmpfs vnode interface: the operations
//! vector used for files stored in a tmpfs file system (`tmpfs_vops`), its operations, the
//! rename machinery and the kqueue filters.
//!
//! Upstream: sys/tmpfs/tmpfs_vnops.h @ 3ce1f3f79392
//! Upstream: sys/tmpfs/tmpfs_vnops.c @ 3ce1f3f79392
//!
//! The declarations of `tmpfs_vnops.h` are the `pub` items here and in `tmpfs_specops.rs`
//! and `tmpfs_fifoops.rs`.
//!
//! # The vnode lock
//!
//! A tmpfs vnode's lock is its node's `tn_vlock`, a recursive rwlock (`rrwlock`), as ufs's
//! is its inode's `i_lock`: `tmpfs_lock` is `rrw_enter(&tn_vlock, a_flags & LK_RWFLAGS)`.
//! A thread that holds it exclusively and asks for it again gets it again (the count goes
//! up and `tmpfs_unlock` takes it down; the last unlock releases it), unless it passes
//! `LK_RECURSEFAIL`, which fails with `EDEADLK`. vnd(4) relies on this: `VNDIOCSET` reads
//! the backing file through `vn_rdwr` (which `vn_lock`s it) while the lock `vn_open`
//! returned it with is still held.
//!
//! ## Deviations
//! - `tmpfs_print`'s `fifo_printinfo` (`option FIFO`) waits for `miscfs/fifofs` (as fifo
//!   nodes do: `tmpfs_vnode_get` refuses them, see `tmpfs_subr.rs`).
//! - The helpers the C installs in several slots are the `vfs_default.rs` functions
//!   (`vop_generic_*`), as in `spec_vops`.
//! - `tmpfs_lookup` passes on an error of `cache_lookup` other than `ENOENT` (it failed to
//!   lock the directory again); the C returns 0 there without a vnode, which its caller
//!   would follow as a NULL pointer.
//! - Credentials the C dereferences (`cred->cr_uid`, the `tmpfs_ch*` calls of
//!   `tmpfs_setattr`) go through `ucred`, which panics on `NOCRED`/`FSCRED`, where the C
//!   would follow a bad pointer. `KASSERT`s on pointers the C then dereferences (an entry's
//!   node, a node's vnode, the source entry `tmpfs_rename_lock` found) are panics too.
//! - `tmpfs_readlink` copies the target to a `MAXPATHLEN` buffer on the stack for
//!   `uiomove`, which takes a mutable slice (a target is shorter than `MAXPATHLEN`).
//! - The rename helpers return what the C stores through out-parameters
//!   (`RenameEntered`, `RenameLocked`, the intermediate node); their `goto fail<n>`
//!   ladders are nested labelled blocks.
//! - `pool_put(&namei_pool, cnp->cn_pnbuf)` is `pnbuf_free`; `curproc` is `curp`,
//!   which panics without a thread (the C would follow NULL).
//! - The filters reach their vnode through `kn_hook` (`kn_vnode`), as ufs's do.

use core::ptr::{self, NonNull};

use crate::kassert;
use crate::kern::kern_event::{klist_insert_locked, klist_remove_locked};
use crate::kern::kern_rwlock::{rrw_enter, rrw_exit, rrw_status, rw_enter_write, rw_exit_write};
use crate::kern::kern_subr::uiomove;
use crate::kern::subr_pool::pool_put;
use crate::kern::subr_prf::panic;
use crate::kern::vfs_cache::{cache_enter, cache_lookup, cache_purge};
use crate::kern::vfs_default::{vop_generic_abortop, vop_generic_bmap, vop_generic_revoke};
use crate::kern::vfs_init::NAMEI_POOL;
use crate::kern::vfs_lockf::lf_advlock;
use crate::kern::vfs_subr::{vaccess, vattr_null, vput, vrecycle, vref, vrele};
use crate::kern::vfs_vnops::{vn_fsizechk, vn_lock};
use crate::kern::vfs_vops::{VOP_ABORTOP, VOP_ACCESS, VOP_ISLOCKED, VOP_UNLOCK};
use crate::machine::cpu::curproc;
use crate::sys::errno::Errno;
use crate::sys::event::{
    __EV_POLL, __EV_SELECT, EV_EOF, EV_ONESHOT, EVFILT_READ, EVFILT_VNODE, EVFILT_WRITE,
    FILTEROP_ISFD, Filterops, Knote, NOTE_EOF, NOTE_EXTEND, NOTE_LINK, NOTE_RENAME, NOTE_REVOKE,
    NOTE_WRITE,
};
use crate::sys::fcntl::{FWRITE, O_APPEND};
use crate::sys::file::foffset;
use crate::sys::lock::{LK_EXCLUSIVE, LK_RETRY, LK_RWFLAGS};
use crate::sys::mount::{MNT_NOATIME, MNT_RDONLY, Mount};
use crate::sys::namei::{
    CREATE, Componentname, DELETE, HASBUF, ISDOTDOT, ISLASTCN, LOCKPARENT, MAKEENTRY, PDIRUNLOCK,
    RENAME, SAVENAME,
};
use crate::sys::param::{MAXPATHLEN, PAGE_SIZE};
use crate::sys::proc::Proc;
use crate::sys::stat::{APPEND, IMMUTABLE, S_ISTXT};
use crate::sys::syslimits::LINK_MAX;
use crate::sys::time::Timespec;
use crate::sys::types::{Gid, Mode, Nlink, Off, Register, Uid};
use crate::sys::ucred::Ucred;
use crate::sys::unistd::{
    _PC_CHOWN_RESTRICTED, _PC_FILESIZEBITS, _PC_LINK_MAX, _PC_NAME_MAX, _PC_NO_TRUNC,
    _PC_TIMESTAMP_RESOLUTION,
};
use crate::sys::vnode::{
    IO_APPEND, VA_UTIMES_CHANGE, VBLK, VCHR, VDIR, VEXEC, VFIFO, VLNK, VN_KNOTE, VNON, VNOVAL,
    VREG, VSOCK, VWRITE, Vnode, VopAccessArgs, VopAdvlockArgs, VopBwriteArgs, VopCloseArgs,
    VopCreateArgs, VopFsyncArgs, VopGetattrArgs, VopInactiveArgs, VopIoctlArgs, VopIslockedArgs,
    VopKqfilterArgs, VopLinkArgs, VopLockArgs, VopLookupArgs, VopMkdirArgs, VopMknodArgs,
    VopOpenArgs, VopPathconfArgs, VopPrintArgs, VopReadArgs, VopReaddirArgs, VopReadlinkArgs,
    VopReclaimArgs, VopRemoveArgs, VopRenameArgs, VopRmdirArgs, VopSetattrArgs, VopStrategyArgs,
    VopSymlinkArgs, VopUnlockArgs, VopWriteArgs, Vops, cred_ref,
};
use crate::tmpfs::tmpfs::{
    TMPFS_DIRSEQ_EOF, TMPFS_MAXNAMLEN, TMPFS_NODE_ACCESSED, TMPFS_NODE_CHANGED,
    TMPFS_NODE_MODIFIED, TMPFS_NODE_STATUSALL, TmpfsDirent, TmpfsMount, TmpfsNode, VFS_TO_TMPFS,
    VP_TO_TMPFS_DIR, VP_TO_TMPFS_NODE, tmpfs_dirseq_full, tmpfs_node_gen, tmpfs_node_reclaiming,
};
use crate::tmpfs::tmpfs_mem::{tmpfs_strname_alloc, tmpfs_strname_free, tmpfs_strname_neqlen};
use crate::tmpfs::tmpfs_subr::{
    tmpfs_alloc_dirent, tmpfs_alloc_file, tmpfs_chflags, tmpfs_chmod, tmpfs_chown, tmpfs_chsize,
    tmpfs_chtimes, tmpfs_dir_attach, tmpfs_dir_cached, tmpfs_dir_detach, tmpfs_dir_getdents,
    tmpfs_dir_lookup, tmpfs_free_dirent, tmpfs_free_node, tmpfs_reg_resize, tmpfs_uio_cached,
    tmpfs_uio_uncache, tmpfs_uiomove, tmpfs_update, tmpfs_vnode_get,
};
use crate::uvm::uvm_param::round_page;
use crate::uvm::uvm_vnode::uvm_vnp_uncache;

/// What `tmpfs_rename_enter` looked up and locked: the C's `*fde_ret`, `*fvp_ret`,
/// `*tde_ret` and `*tvp_ret`.
struct RenameEntered {
    /// The source entry.
    fde: &'static TmpfsDirent,
    /// The source vnode, locked and referenced.
    fvp: &'static Vnode,
    /// The target entry, when the target exists.
    tde: Option<&'static TmpfsDirent>,
    /// The target vnode, locked and referenced, when the target exists.
    tvp: Option<&'static Vnode>,
}

/// What `tmpfs_rename_lock` looked up and locked: the C's `*a_dirent_ret`, `*a_vp_ret`,
/// `*b_dirent_ret` and `*b_vp_ret`.
struct RenameLocked {
    /// The entry of `a_cnp` in directory a, if any.
    a_dirent: Option<&'static TmpfsDirent>,
    /// Its vnode, locked and referenced.
    a_vp: Option<&'static Vnode>,
    /// The entry of `b_cnp` in directory b, if any.
    b_dirent: Option<&'static TmpfsDirent>,
    /// Its vnode, locked and referenced (`a_vp` once more when both name one node).
    b_vp: Option<&'static Vnode>,
}

/// `tmpfs_vops`: vnode operations vector used for files stored in a tmpfs file system.
pub static TMPFS_VOPS: Vops = Vops {
    vop_lookup: Some(tmpfs_lookup),
    vop_create: Some(tmpfs_create),
    vop_mknod: Some(tmpfs_mknod),
    vop_open: Some(tmpfs_open),
    vop_close: Some(tmpfs_close),
    vop_access: Some(tmpfs_access),
    vop_getattr: Some(tmpfs_getattr),
    vop_setattr: Some(tmpfs_setattr),
    vop_read: Some(tmpfs_read),
    vop_write: Some(tmpfs_write),
    vop_ioctl: Some(tmpfs_ioctl),
    vop_kqfilter: Some(tmpfs_kqfilter),
    vop_revoke: Some(vop_generic_revoke),
    vop_fsync: Some(tmpfs_fsync),
    vop_remove: Some(tmpfs_remove),
    vop_link: Some(tmpfs_link),
    vop_rename: Some(tmpfs_rename),
    vop_mkdir: Some(tmpfs_mkdir),
    vop_rmdir: Some(tmpfs_rmdir),
    vop_symlink: Some(tmpfs_symlink),
    vop_readdir: Some(tmpfs_readdir),
    vop_readlink: Some(tmpfs_readlink),
    vop_abortop: Some(vop_generic_abortop),
    vop_inactive: Some(tmpfs_inactive),
    vop_reclaim: Some(tmpfs_reclaim),
    vop_lock: Some(tmpfs_lock),
    vop_unlock: Some(tmpfs_unlock),
    vop_bmap: Some(vop_generic_bmap),
    vop_strategy: Some(tmpfs_strategy),
    vop_print: Some(tmpfs_print),
    vop_islocked: Some(tmpfs_islocked),
    vop_pathconf: Some(tmpfs_pathconf),
    vop_advlock: Some(tmpfs_advlock),
    vop_bwrite: Some(tmpfs_bwrite),
};

/// `tmpfsread_filtops`.
pub static TMPFSREAD_FILTOPS: Filterops = Filterops {
    f_flags: FILTEROP_ISFD,
    f_attach: None,
    f_detach: Some(filt_tmpfsdetach),
    f_event: Some(filt_tmpfsread),
    f_modify: None,
    f_process: None,
};

/// `tmpfswrite_filtops`.
pub static TMPFSWRITE_FILTOPS: Filterops = Filterops {
    f_flags: FILTEROP_ISFD,
    f_attach: None,
    f_detach: Some(filt_tmpfsdetach),
    f_event: Some(filt_tmpfswrite),
    f_modify: None,
    f_process: None,
};

/// `tmpfsvnode_filtops`.
pub static TMPFSVNODE_FILTOPS: Filterops = Filterops {
    f_flags: FILTEROP_ISFD,
    f_attach: None,
    f_detach: Some(filt_tmpfsdetach),
    f_event: Some(filt_tmpfsvnode),
    f_modify: None,
    f_process: None,
};

/// The vnode's mount, which a tmpfs vnode always has.
fn vmount(vp: &Vnode) -> &'static Mount {
    match vp.v_mount.get() {
        Some(mp) => mp,
        None => panic(format_args!("tmpfs: vnode {:p} without a mount", vp)),
    }
}

/// `curproc`: the thread a vnode operation runs in.
fn curp() -> &'static Proc {
    match curproc() {
        Some(p) => p,
        None => panic(format_args!("tmpfs: no curproc")),
    }
}

/// `*cred` of a credential the C dereferences: a real one (`NOCRED`/`FSCRED` panic).
fn ucred<'a>(cred: *const Ucred) -> &'a Ucred {
    // SAFETY: the credentials a vnode operation receives are held by its caller for the
    // operation's duration (`cred_ref`'s contract).
    match unsafe { cred_ref(cred) } {
        Some(c) => c,
        None => panic(format_args!(
            "tmpfs: credential {:p} is not a real one",
            cred
        )),
    }
}

/// `pool_put(&namei_pool, cnp->cn_pnbuf)`: give back the pathname buffer.
fn pnbuf_free(cnp: &Componentname) {
    if let Some(buf) = NonNull::new(cnp.cn_pnbuf) {
        pool_put(&NAMEI_POOL, buf);
    }
}

/// `de->td_node` where the C dereferences it: an entry in a directory has its node.
fn td_node(de: &TmpfsDirent) -> &'static TmpfsNode {
    match de.td_node.get() {
        Some(node) => node,
        None => panic(format_args!("tmpfs: entry {:p} without a node", de)),
    }
}

/// `node->tn_vnode` where the C dereferences it: a node the rename locked has its vnode.
fn tn_vnode(node: &TmpfsNode) -> &'static Vnode {
    match node.tn_vnode.get() {
        Some(vp) => vp,
        None => panic(format_args!("tmpfs: node {:p} without a vnode", node)),
    }
}

/// The C's `a == b` on two pointers that may be NULL.
fn same<T>(a: Option<&T>, b: Option<&T>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => ptr::eq(a, b),
        (None, None) => true,
        _ => false,
    }
}

/// Whether `cnp` names `.` or `..`.
fn is_dot_or_dotdot(cnp: &Componentname) -> bool {
    let name = cnp.name();
    name == b"." || name == b".."
}

/// `tmpfs_lookup` (`vop_lookup`): path name traversal routine.
///
/// Arguments: `dvp` (directory being searched), `vpp` (result), `cnp` (component name -
/// path).
///
/// - Caller holds a reference and lock on `dvp`.
/// - We return looked-up vnode (`vpp`) locked, with a reference held.
pub fn tmpfs_lookup(ap: &mut VopLookupArgs<'_>) -> Result<(), Errno> {
    let dvp = ap.a_dvp;
    let vpp = &mut *ap.a_vpp;
    let cnp = &mut *ap.a_cnp;
    let cred = cnp.cn_cred;
    let lastcn = cnp.cn_flags & ISLASTCN != 0;
    let lockparent = cnp.cn_flags & LOCKPARENT != 0;

    kassert!(VOP_ISLOCKED(dvp) != 0);

    let dnode = VP_TO_TMPFS_DIR(dvp);
    cnp.cn_flags &= !PDIRUNLOCK;
    *vpp = None;

    let error: Result<(), Errno> = 'out: {
        // Check accessibility of directory.
        if let Err(e) = VOP_ACCESS(dvp, VEXEC, cred, curp()) {
            break 'out Err(e);
        }

        // If requesting the last path component on a read-only file system with a write
        // operation, deny it.
        if lastcn
            && vmount(dvp).mnt_flag.get() & MNT_RDONLY != 0
            && (cnp.cn_nameiop == DELETE || cnp.cn_nameiop == RENAME)
        {
            break 'out Err(Errno::EROFS);
        }

        // Avoid doing a linear scan of the directory if the requested directory/name couple
        // is already in the cache. A negative hit is ENOENT.
        if let Some(vp) = cache_lookup(dvp, cnp)? {
            *vpp = Some(vp);
            return Ok(()); // Found in cache.
        }

        let error: Result<(), Errno> = 'done: {
            if cnp.cn_flags & ISDOTDOT != 0 {
                // Lookup of ".." case.
                if lastcn {
                    if cnp.cn_nameiop == RENAME {
                        break 'out Err(Errno::EINVAL);
                    }
                    if cnp.cn_nameiop == DELETE {
                        // Keep the name for tmpfs_rmdir().
                        cnp.cn_flags |= SAVENAME;
                    }
                }
                kassert!(dnode.tn_type.get() == VDIR);
                let Some(pnode) = dnode.tn_spec.tn_dir.tn_parent.get() else {
                    break 'out Err(Errno::ENOENT);
                };

                // Lock the parent tn_nlock before releasing the vnode lock, and thus
                // prevents parent from disappearing.
                rw_enter_write(&pnode.tn_nlock);
                let _ = VOP_UNLOCK(dvp);

                // Get a vnode of the '..' entry and re-acquire the lock. Release the
                // tn_nlock.
                let error = tmpfs_vnode_get(vmount(dvp), pnode).map(|vp| *vpp = Some(vp));
                let _ = vn_lock(dvp, LK_EXCLUSIVE | LK_RETRY);
                break 'out error;
            } else if cnp.name() == b"." {
                // Lookup of "." case.
                if lastcn && cnp.cn_nameiop == RENAME {
                    break 'out Err(Errno::EISDIR);
                }
                vref(dvp);
                *vpp = Some(dvp);
                break 'done Ok(());
            }

            // Other lookup cases: perform directory scan.
            let Some(de) = tmpfs_dir_lookup(dnode, cnp) else {
                // The entry was not found in the directory. This is valid if we are
                // creating or renaming an entry and are working on the last component of
                // the path name.
                if lastcn && (cnp.cn_nameiop == CREATE || cnp.cn_nameiop == RENAME) {
                    if let Err(e) = VOP_ACCESS(dvp, VWRITE, cred, curp()) {
                        break 'out Err(e);
                    }
                    // We are creating an entry in the file system, so save its name for
                    // further use by tmpfs_create().
                    cnp.cn_flags |= SAVENAME;
                    break 'done Err(Errno::EJUSTRETURN);
                }
                break 'done Err(Errno::ENOENT);
            };

            let tnode = td_node(de);

            // If it is not the last path component and found a non-directory or non-link
            // entry (which may itself be pointing to a directory), raise an error.
            if !lastcn && tnode.tn_type.get() != VDIR && tnode.tn_type.get() != VLNK {
                break 'out Err(Errno::ENOTDIR);
            }

            // Check the permissions.
            if lastcn && (cnp.cn_nameiop == DELETE || cnp.cn_nameiop == RENAME) {
                if let Err(e) = VOP_ACCESS(dvp, VWRITE, cred, curp()) {
                    break 'out Err(e);
                }

                // If not root and directory is sticky, check for permission on directory or
                // on file. This implements append-only directories.
                if dnode.tn_mode.get() & S_ISTXT != 0 {
                    let uid = cnp.cred().cr_uid.get();
                    if uid != 0 && uid != dnode.tn_uid.get() && uid != tnode.tn_uid.get() {
                        break 'out Err(Errno::EPERM);
                    }
                }

                // XXX pedro: We might need cn_nameptr later in tmpfs_remove() or
                // tmpfs_rmdir() for a tmpfs_dir_lookup(). We should really get rid of
                // SAVENAME at some point.
                if cnp.cn_nameiop == DELETE {
                    cnp.cn_flags |= SAVENAME;
                }
            }

            // Get a vnode for the matching entry.
            rw_enter_write(&tnode.tn_nlock);
            tmpfs_vnode_get(vmount(dvp), tnode).map(|vp| *vpp = Some(vp))
        };
        // done:
        // Cache the result, unless request was for creation (as it does not improve the
        // performance).
        if cnp.cn_flags & MAKEENTRY != 0 && cnp.cn_nameiop != CREATE {
            cache_enter(dvp, *vpp, cnp);
        }
        error
    };

    // out:
    // If (1) we succeeded, (2) found a distinct vnode != .. to return and (3) were either
    // explicitly told to keep the parent locked or are in the middle of a lookup, unlock the
    // parent vnode.
    if matches!(error, Ok(()) | Err(Errno::EJUSTRETURN)) // (1)
        && (!same(*vpp, Some(dvp)) || cnp.cn_flags & ISDOTDOT != 0) // (2)
        && (!lockparent || !lastcn)
    // (3)
    {
        let _ = VOP_UNLOCK(dvp);
        cnp.cn_flags |= PDIRUNLOCK;
    } else {
        kassert!(VOP_ISLOCKED(dvp) != 0);
    }

    kassert!(vpp.is_some_and(|vp| VOP_ISLOCKED(vp) != 0) || error.is_err());

    error
}

/// `tmpfs_create` (`vop_create`): create a regular file (or a socket's node).
pub fn tmpfs_create(ap: &mut VopCreateArgs<'_>) -> Result<(), Errno> {
    let dvp = ap.a_dvp;
    let vap = &*ap.a_vap;

    kassert!(VOP_ISLOCKED(dvp) != 0);
    kassert!(ap.a_cnp.cn_flags & HASBUF != 0);
    kassert!(vap.va_type == VREG || vap.va_type == VSOCK);
    *ap.a_vpp = Some(tmpfs_alloc_file(dvp, vap, ap.a_cnp, None)?);
    Ok(())
}

/// `tmpfs_mknod` (`vop_mknod`): make a device special file or a fifo.
pub fn tmpfs_mknod(ap: &mut VopMknodArgs<'_>) -> Result<(), Errno> {
    let vt = ap.a_vap.va_type;

    if vt != VBLK && vt != VCHR && vt != VFIFO {
        return Err(Errno::EINVAL);
    }

    let vp = tmpfs_alloc_file(ap.a_dvp, ap.a_vap, ap.a_cnp, None)?;
    *ap.a_vpp = Some(vp);
    vput(vp);
    Ok(())
}

/// `tmpfs_open` (`vop_open`): refuse a file all of whose names are gone, and writes to an
/// append-only file that are not appends.
pub fn tmpfs_open(ap: &mut VopOpenArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let mode = ap.a_mode;

    kassert!(VOP_ISLOCKED(vp) != 0);

    let node = VP_TO_TMPFS_NODE(vp);
    if node.tn_links.get() < 1 {
        // The file is still active, but all its names have been removed (e.g. by a "rmdir
        // $(pwd)"). It cannot be opened any more, as it is about to be destroyed.
        return Err(Errno::ENOENT);
    }

    // If the file is marked append-only, deny write requests.
    if node.tn_flags.get() & APPEND != 0 && mode & (FWRITE | O_APPEND) == FWRITE {
        return Err(Errno::EPERM);
    }
    Ok(())
}

/// `tmpfs_close` (`vop_close`): nothing to do.
pub fn tmpfs_close(ap: &mut VopCloseArgs<'_>) -> Result<(), Errno> {
    // DIAGNOSTIC:
    kassert!(VOP_ISLOCKED(ap.a_vp) != 0);
    Ok(())
}

/// `tmpfs_access` (`vop_access`).
pub fn tmpfs_access(ap: &mut VopAccessArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let mode = ap.a_mode;
    let node = VP_TO_TMPFS_NODE(vp);
    let writing = mode & VWRITE != 0;

    kassert!(VOP_ISLOCKED(vp) != 0);

    // Possible?
    match vp.v_type.get() {
        VDIR | VLNK | VREG => {
            if writing && vmount(vp).mnt_flag.get() & MNT_RDONLY != 0 {
                return Err(Errno::EROFS);
            }
        }
        VBLK | VCHR | VSOCK | VFIFO => {}
        _ => return Err(Errno::EINVAL),
    }
    if writing && node.tn_flags.get() & IMMUTABLE != 0 {
        return Err(Errno::EPERM);
    }

    vaccess(
        vp.v_type.get(),
        node.tn_mode.get(),
        node.tn_uid.get(),
        node.tn_gid.get(),
        mode,
        ucred(ap.a_cred),
    )
}

/// `tmpfs_getattr` (`vop_getattr`).
pub fn tmpfs_getattr(ap: &mut VopGetattrArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let vap = &mut *ap.a_vap;
    let node = VP_TO_TMPFS_NODE(vp);

    vattr_null(vap);

    vap.va_type = vp.v_type.get();
    vap.va_mode = node.tn_mode.get();
    vap.va_nlink = node.tn_links.get();
    vap.va_uid = node.tn_uid.get();
    vap.va_gid = node.tn_gid.get();
    vap.va_fsid = i64::from(vmount(vp).mnt_stat.get().f_fsid.val[0]);
    vap.va_fileid = node.tn_id.get();
    vap.va_size = node.tn_size.get() as u64;
    vap.va_blocksize = PAGE_SIZE as i64;
    vap.va_atime = node.tn_atime.get();
    vap.va_mtime = node.tn_mtime.get();
    vap.va_ctime = node.tn_ctime.get();
    // vap->va_birthtime = node->tn_birthtime;
    vap.va_gen = tmpfs_node_gen(node);
    vap.va_flags = u64::from(node.tn_flags.get());
    vap.va_rdev = if vp.v_type.get() == VBLK || vp.v_type.get() == VCHR {
        node.tn_spec.tn_dev.tn_rdev.get()
    } else {
        VNOVAL
    };
    vap.va_bytes = round_page(node.tn_size.get() as usize) as u64;
    vap.va_filerev = VNOVAL as u64;
    vap.va_vaflags = 0;
    vap.va_spare = i64::from(VNOVAL); // XXX

    Ok(())
}

/// `GOODTIME(tv)`: whether a time of a `vattr` is set.
#[allow(non_snake_case)] // the C macro
fn GOODTIME(tv: &Timespec) -> bool {
    tv.tv_nsec != i64::from(VNOVAL)
}

/// `tmpfs_setattr` (`vop_setattr`).
///
/// XXX Should this operation be atomic? I think it should, but code in other places (e.g.,
/// ufs) doesn't seem to be...
pub fn tmpfs_setattr(ap: &mut VopSetattrArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let vap = &*ap.a_vap;
    let cred = ap.a_cred;
    let p = curp();

    kassert!(VOP_ISLOCKED(vp) != 0);

    // Abort if any unsettable attribute is given.
    if vap.va_type != VNON
        || vap.va_nlink != VNOVAL as Nlink
        || vap.va_fsid != i64::from(VNOVAL)
        || vap.va_fileid != VNOVAL as u64
        || vap.va_blocksize != i64::from(VNOVAL)
        || GOODTIME(&vap.va_ctime)
        || vap.va_gen != VNOVAL as u64
        || vap.va_rdev != VNOVAL
        || vap.va_bytes != VNOVAL as u64
    {
        return Err(Errno::EINVAL);
    }
    if vap.va_flags != VNOVAL as u64 {
        tmpfs_chflags(vp, vap.va_flags as u32, ucred(cred), p)?;
    }

    if vap.va_size != VNOVAL as u64 {
        tmpfs_chsize(vp, vap.va_size, ucred(cred), p)?;
    }

    if vap.va_uid != VNOVAL as Uid || vap.va_gid != VNOVAL as Gid {
        tmpfs_chown(vp, vap.va_uid, vap.va_gid, ucred(cred), p)?;
    }

    if vap.va_mode != VNOVAL as Mode {
        tmpfs_chmod(vp, vap.va_mode, ucred(cred), p)?;
    }

    if vap.va_vaflags & VA_UTIMES_CHANGE != 0 || GOODTIME(&vap.va_atime) || GOODTIME(&vap.va_mtime)
    {
        tmpfs_chtimes(
            vp,
            &vap.va_atime,
            &vap.va_mtime,
            vap.va_vaflags,
            ucred(cred),
            p,
        )?;
    }

    Ok(())
}

/// `tmpfs_read` (`vop_read`): read a regular file through its object.
pub fn tmpfs_read(ap: &mut VopReadArgs<'_, '_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let uio = &mut *ap.a_uio;
    // const int ioflag = ap->a_ioflag;

    kassert!(VOP_ISLOCKED(vp) != 0);

    if vp.v_type.get() != VREG {
        return Err(Errno::EISDIR);
    }
    if uio.uio_offset < 0 {
        return Err(Errno::EINVAL);
    }
    if uio.uio_resid == 0 {
        return Ok(());
    }

    let node = VP_TO_TMPFS_NODE(vp);
    let mut error = Ok(());

    while error.is_ok() && uio.uio_resid > 0 {
        if node.tn_size.get() <= uio.uio_offset {
            break;
        }
        let len = (node.tn_size.get() - uio.uio_offset).min(uio.uio_resid as Off);
        if len == 0 {
            break;
        }
        error = tmpfs_uiomove(node, uio, len as usize);
    }

    if vmount(vp).mnt_flag.get() & MNT_NOATIME == 0 {
        tmpfs_update(node, TMPFS_NODE_ACCESSED);
    }

    error
}

/// `tmpfs_write` (`vop_write`): write a regular file through its object, growing it first
/// when the write goes past its end.
pub fn tmpfs_write(ap: &mut VopWriteArgs<'_, '_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let uio = &mut *ap.a_uio;
    let ioflag = ap.a_ioflag;

    kassert!(VOP_ISLOCKED(vp) != 0);

    let node = VP_TO_TMPFS_NODE(vp);
    let oldsize = node.tn_size.get();

    if vp.v_type.get() != VREG {
        return Err(Errno::EINVAL);
    }

    if uio.uio_resid == 0 {
        return Ok(());
    }

    if ioflag & IO_APPEND != 0 {
        uio.uio_offset = node.tn_size.get();
    }

    if uio.uio_offset < 0 || uio.uio_offset as u64 + uio.uio_resid as u64 > i64::MAX as u64 {
        return Err(Errno::EFBIG);
    }

    // do the filesize rlimit check
    let overrun = vn_fsizechk(vp, uio, ioflag)?;

    let extended = uio.uio_offset + uio.uio_resid as Off > node.tn_size.get();
    let error: Result<(), Errno> = 'out: {
        if extended && let Err(e) = tmpfs_reg_resize(vp, uio.uio_offset + uio.uio_resid as Off) {
            break 'out Err(e);
        }

        let mut error = Ok(());
        while error.is_ok() && uio.uio_resid > 0 {
            let _ = uvm_vnp_uncache(vp);
            let len = (node.tn_size.get() - uio.uio_offset).min(uio.uio_resid as Off);
            if len == 0 {
                break;
            }
            error = tmpfs_uiomove(node, uio, len as usize);
        }
        if error.is_err() {
            let _ = tmpfs_reg_resize(vp, oldsize);
        }

        tmpfs_update(node, TMPFS_NODE_MODIFIED | TMPFS_NODE_CHANGED);
        if extended {
            VN_KNOTE(vp, NOTE_WRITE | NOTE_EXTEND);
        } else {
            VN_KNOTE(vp, NOTE_WRITE);
        }
        error
    };
    // out:
    if error.is_err() {
        kassert!(oldsize == node.tn_size.get());
    } else {
        kassert!(uio.uio_resid == 0);

        // correct the result for writes clamped by vn_fsizechk()
        uio.uio_resid += overrun as usize;
    }
    error
}

/// `tmpfs_fsync` (`vop_fsync`): nothing to do.
pub fn tmpfs_fsync(ap: &mut VopFsyncArgs<'_>) -> Result<(), Errno> {
    // DIAGNOSTIC: Nothing to do. Just update.
    kassert!(VOP_ISLOCKED(ap.a_vp) != 0);
    Ok(())
}

/// `tmpfs_remove` (`vop_remove`): unlink a file.
///
/// - Both directory (`dvp`) and file (`vp`) are locked.
/// - `VOP_REMOVE` unlocks and drops the reference on both.
pub fn tmpfs_remove(ap: &mut VopRemoveArgs<'_>) -> Result<(), Errno> {
    let dvp = ap.a_dvp;
    let vp = ap.a_vp;
    let cnp = &*ap.a_cnp;

    kassert!(cnp.cn_flags & HASBUF != 0);

    let error: Result<(), Errno> = 'out: {
        if vp.v_type.get() == VDIR {
            break 'out Err(Errno::EPERM);
        }

        let dnode = VP_TO_TMPFS_NODE(dvp);
        let node = VP_TO_TMPFS_NODE(vp);

        // Files marked as immutable or append-only cannot be deleted.
        if node.tn_flags.get() & (IMMUTABLE | APPEND) != 0 {
            break 'out Err(Errno::EPERM);
        }

        // Likewise, files residing on directories marked as append-only cannot be deleted.
        if dnode.tn_flags.get() & APPEND != 0 {
            break 'out Err(Errno::EPERM);
        }

        // Lookup the directory entry (check the cached hint first).
        let de = tmpfs_dir_cached(node).or_else(|| tmpfs_dir_lookup(dnode, cnp));

        kassert!(de.is_some_and(|de| same(de.td_node.get(), Some(node))));
        let Some(de) = de else {
            panic(format_args!("tmpfs_remove: no entry for node {:p}", node));
        };

        // Remove the entry from the directory (drops the link count) and destroy it.
        // Note: the inode referred by it will not be destroyed until the vnode is
        // reclaimed/recycled.
        tmpfs_dir_detach(dnode, de);
        tmpfs_free_dirent(VFS_TO_TMPFS(vmount(vp)), de);
        if node.tn_links.get() > 0 {
            // We removed a hard link.
            tmpfs_update(node, TMPFS_NODE_CHANGED);
        }
        Ok(())
    };
    // out:
    pnbuf_free(cnp);
    error
}

/// `tmpfs_link` (`vop_link`): create a hard link.
pub fn tmpfs_link(ap: &mut VopLinkArgs<'_>) -> Result<(), Errno> {
    let dvp = ap.a_dvp;
    let vp = ap.a_vp;
    let cnp = &*ap.a_cnp;

    kassert!(VOP_ISLOCKED(dvp) != 0);
    kassert!(!ptr::eq(dvp, vp));

    let dnode = VP_TO_TMPFS_DIR(dvp);
    let node = VP_TO_TMPFS_NODE(vp);

    let _ = vn_lock(vp, LK_EXCLUSIVE | LK_RETRY);

    let error: Result<(), Errno> = 'out: {
        // Check for maximum number of links limit.
        if node.tn_links.get() == LINK_MAX {
            break 'out Err(Errno::EMLINK);
        }
        kassert!(node.tn_links.get() < LINK_MAX);

        // We cannot create links of files marked immutable or append-only.
        if node.tn_flags.get() & (IMMUTABLE | APPEND) != 0 {
            break 'out Err(Errno::EPERM);
        }

        if tmpfs_dirseq_full(dnode) {
            break 'out Err(Errno::ENOSPC);
        }

        // Allocate a new directory entry to represent the inode.
        let de = match tmpfs_alloc_dirent(VFS_TO_TMPFS(vmount(vp)), cnp.name()) {
            Ok(de) => de,
            Err(e) => break 'out Err(e),
        };

        // Insert the entry into the directory. It will increase the inode link count.
        tmpfs_dir_attach(dnode, de, node);

        // Update the timestamps and trigger the event.
        if let Some(nvp) = node.tn_vnode.get() {
            VN_KNOTE(nvp, NOTE_LINK);
        }
        tmpfs_update(node, TMPFS_NODE_CHANGED);
        Ok(())
    };
    // out:
    pnbuf_free(cnp);
    let _ = VOP_UNLOCK(vp);
    vput(dvp);
    error
}

/// `tmpfs_mkdir` (`vop_mkdir`).
pub fn tmpfs_mkdir(ap: &mut VopMkdirArgs<'_>) -> Result<(), Errno> {
    let dvp = ap.a_dvp;

    kassert!(ap.a_vap.va_type == VDIR);
    let error = tmpfs_alloc_file(dvp, ap.a_vap, ap.a_cnp, None).map(|vp| *ap.a_vpp = Some(vp));
    vput(dvp);
    error
}

/// `tmpfs_rmdir` (`vop_rmdir`): remove an empty directory.
pub fn tmpfs_rmdir(ap: &mut VopRmdirArgs<'_>) -> Result<(), Errno> {
    let dvp = ap.a_dvp;
    let vp = ap.a_vp;
    let cnp = &*ap.a_cnp;
    let tmp = VFS_TO_TMPFS(vmount(dvp));
    let dnode = VP_TO_TMPFS_DIR(dvp);
    let node = VP_TO_TMPFS_DIR(vp);

    kassert!(VOP_ISLOCKED(dvp) != 0);
    kassert!(VOP_ISLOCKED(vp) != 0);
    kassert!(cnp.cn_flags & HASBUF != 0);

    let error: Result<(), Errno> = 'out: {
        if cnp.name() == b".." {
            break 'out Err(Errno::ENOTEMPTY);
        }

        kassert!(same(node.tn_spec.tn_dir.tn_parent.get(), Some(dnode)));

        // Directories with more than two entries ('.' and '..') cannot be removed.
        if node.tn_size.get() > 0 && node.tn_spec.tn_dir.tn_dir.first().is_some() {
            break 'out Err(Errno::ENOTEMPTY);
        }

        // Lookup the directory entry (check the cached hint first).
        let de = tmpfs_dir_cached(node).or_else(|| tmpfs_dir_lookup(dnode, cnp));

        kassert!(de.is_some_and(|de| same(de.td_node.get(), Some(node))));
        let Some(de) = de else {
            panic(format_args!("tmpfs_rmdir: no entry for node {:p}", node));
        };

        // Check flags to see if we are allowed to remove the directory.
        if dnode.tn_flags.get() & APPEND != 0 || node.tn_flags.get() & (IMMUTABLE | APPEND) != 0 {
            break 'out Err(Errno::EPERM);
        }

        // Decrement the link count for the virtual '.' entry.
        node.tn_links.set(node.tn_links.get() - 1);
        tmpfs_update(node, TMPFS_NODE_STATUSALL);

        // Detach the directory entry from the directory.
        tmpfs_dir_detach(dnode, de);

        // Purge the cache for parent.
        cache_purge(dvp);

        // Destroy the directory entry. Note: the inode referred by it will not be destroyed
        // until the vnode is reclaimed.
        tmpfs_free_dirent(tmp, de);
        kassert!(node.tn_spec.tn_dir.tn_dir.first().is_none());

        kassert!(node.tn_links.get() == 0);
        Ok(())
    };
    // out:
    pnbuf_free(cnp);
    // Release the nodes.
    vput(dvp);
    vput(vp);
    error
}

/// `tmpfs_symlink` (`vop_symlink`).
pub fn tmpfs_symlink(ap: &mut VopSymlinkArgs<'_>) -> Result<(), Errno> {
    let dvp = ap.a_dvp;
    let vap = &mut *ap.a_vap;

    kassert!(vap.va_type == VNON);
    vap.va_type = VLNK;

    let error = tmpfs_alloc_file(dvp, vap, ap.a_cnp, Some(ap.a_target));
    vput(dvp);
    let vp = error?;
    *ap.a_vpp = Some(vp);
    vput(vp);

    Ok(())
}

/// `tmpfs_readdir` (`vop_readdir`).
pub fn tmpfs_readdir(ap: &mut VopReaddirArgs<'_, '_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let uio = &mut *ap.a_uio;

    kassert!(VOP_ISLOCKED(vp) != 0);

    // This operation only makes sense on directory nodes.
    if vp.v_type.get() != VDIR {
        return Err(Errno::ENOTDIR);
    }
    let node = VP_TO_TMPFS_DIR(vp);
    // Retrieve the directory entries, unless it is being destroyed.
    let error = if node.tn_links.get() != 0 {
        tmpfs_dir_getdents(node, uio)
    } else {
        Ok(())
    };

    *ap.a_eofflag = i32::from(error.is_ok() && uio.uio_offset == TMPFS_DIRSEQ_EOF as Off);
    error
}

/// `tmpfs_readlink` (`vop_readlink`).
pub fn tmpfs_readlink(ap: &mut VopReadlinkArgs<'_, '_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let uio = &mut *ap.a_uio;

    kassert!(VOP_ISLOCKED(vp) != 0);
    kassert!(uio.uio_offset == 0);
    kassert!(vp.v_type.get() == VLNK);

    let node = VP_TO_TMPFS_NODE(vp);
    let link = node.link();
    let len = link.len().min(uio.uio_resid);
    let mut buf = [0u8; MAXPATHLEN];
    buf[..len].copy_from_slice(&link[..len]);
    let error = uiomove(&mut buf[..len], uio);

    if vmount(vp).mnt_flag.get() & MNT_NOATIME == 0 {
        tmpfs_update(node, TMPFS_NODE_ACCESSED);
    }

    error
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

/// `tmpfs_pathconf` (`vop_pathconf`).
pub fn tmpfs_pathconf(ap: &mut VopPathconfArgs<'_>) -> Result<(), Errno> {
    *ap.a_retval = match ap.a_name {
        _PC_LINK_MAX => LINK_MAX as Register,
        _PC_NAME_MAX => TMPFS_MAXNAMLEN as Register,
        _PC_CHOWN_RESTRICTED => 1,
        _PC_NO_TRUNC => 1,
        _PC_FILESIZEBITS => 64,
        _PC_TIMESTAMP_RESOLUTION => 1,
        _ => return Err(Errno::EINVAL),
    };
    Ok(())
}

/// `tmpfs_advlock` (`vop_advlock`): advisory record locking on the node.
pub fn tmpfs_advlock(ap: &mut VopAdvlockArgs<'_>) -> Result<(), Errno> {
    let node = VP_TO_TMPFS_NODE(ap.a_vp);

    lf_advlock(
        &node.tn_lockf,
        node.tn_size.get(),
        ap.a_id,
        ap.a_op,
        ap.a_fl,
        ap.a_flags,
    )
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

/// `tmpfs_rename` (`vop_rename`): rename routine, the hairiest system call, with the insane
/// API.
///
/// Arguments: `fdvp` (from-parent vnode), `fvp` (from-leaf), `tdvp` (to-parent) and `tvp`
/// (to-leaf), if exists (NULL if not).
///
/// - Caller holds a reference on `fdvp` and `fvp`, they are unlocked. Note: `fdvp` and
///   `fvp` can refer to the same object (i.e. when it is root).
/// - Both `tdvp` and `tvp` are referenced and locked. It is our responsibility to release
///   the references and unlock them (or destroy).
pub fn tmpfs_rename(ap: &mut VopRenameArgs<'_>) -> Result<(), Errno> {
    let fdvp = ap.a_fdvp;
    let fvp = ap.a_fvp;
    let tdvp = ap.a_tdvp;
    let tvp = ap.a_tvp;

    kassert!(!ap.a_fcnp.cn_nameptr.is_null());
    // KASSERT(VOP_ISLOCKED(fdvp) != LK_EXCLUSIVE);
    // KASSERT(VOP_ISLOCKED(fvp) != LK_EXCLUSIVE);
    kassert!(fdvp.v_type.get() == VDIR);
    kassert!(tdvp.v_type.get() == VDIR);
    kassert!(ap.a_fcnp.cn_flags & HASBUF != 0);
    kassert!(ap.a_tcnp.cn_flags & HASBUF != 0);

    let cred = ap.a_fcnp.cn_cred;
    kassert!(ptr::eq(ap.a_tcnp.cn_cred, cred));

    // Check for cross-device rename. Also don't allow renames of mount points.
    if !same(fvp.v_mount.get(), tdvp.v_mount.get())
        || !same(fdvp.v_mount.get(), fvp.v_mount.get())
        || tvp.is_some_and(|tvp| !same(fvp.v_mount.get(), tvp.v_mount.get()))
    {
        tmpfs_rename_abort(ap);
        return Err(Errno::EXDEV);
    }

    // Can't check the locks on these until we know they're on the same FS, as not all FS do
    // locking the same way.
    kassert!(VOP_ISLOCKED(tdvp) == LK_EXCLUSIVE);
    kassert!(tvp.is_none_or(|tvp| VOP_ISLOCKED(tvp) == LK_EXCLUSIVE));

    // Reject renaming '.' and '..'.
    if is_dot_or_dotdot(ap.a_fcnp) {
        tmpfs_rename_abort(ap);
        return Err(Errno::EINVAL);
    }

    // Sanitize our world from the VFS insanity. Unlock the target directory and node,
    // which are locked. Release the children, which are referenced. Check for
    // rename("x", "y/."), which it is our responsibility to reject, not the caller's. (But
    // the caller does reject rename("x/.", "y"). Go figure.)
    let _ = VOP_UNLOCK(tdvp);
    if let Some(tvp) = tvp
        && !ptr::eq(tvp, tdvp)
    {
        let _ = VOP_UNLOCK(tvp);
    }

    vrele(fvp);
    if let Some(tvp) = tvp {
        vrele(tvp);
    }

    let error = if same(tvp, Some(tdvp)) {
        Err(Errno::EINVAL)
    } else {
        tmpfs_sane_rename(fdvp, ap.a_fcnp, tdvp, ap.a_tcnp, cred, false)
    };

    // out: All done, whether with success or failure. Release the directory nodes now, as
    // the caller expects from the VFS protocol.
    vrele(fdvp);
    vrele(tdvp);

    error
}

/// `tmpfs_sane_rename`: rename routine, the hairiest system call, with the sane API.
///
/// Arguments: `fdvp` (from directory vnode), `fcnp` (from component name), `tdvp` (to
/// directory vnode), and `tcnp` (to component name).
///
/// `fdvp` and `tdvp` must be referenced and unlocked.
fn tmpfs_sane_rename(
    fdvp: &'static Vnode,
    fcnp: &Componentname,
    tdvp: &'static Vnode,
    tcnp: &Componentname,
    cred: *const Ucred,
    posixly_correct: bool,
) -> Result<(), Errno> {
    // KASSERT(VOP_ISLOCKED(fdvp) != LK_EXCLUSIVE);
    // KASSERT(VOP_ISLOCKED(tdvp) != LK_EXCLUSIVE);
    kassert!(fdvp.v_type.get() == VDIR);
    kassert!(tdvp.v_type.get() == VDIR);
    kassert!(same(fdvp.v_mount.get(), tdvp.v_mount.get()));
    kassert!(fcnp.cn_flags & ISDOTDOT == 0);
    kassert!(tcnp.cn_flags & ISDOTDOT == 0);
    kassert!(!is_dot_or_dotdot(fcnp));
    kassert!(!is_dot_or_dotdot(tcnp));

    // Pull out the tmpfs data structures.
    let fdnode = VP_TO_TMPFS_NODE(fdvp);
    let tdnode = VP_TO_TMPFS_NODE(tdvp);
    kassert!(same(fdnode.tn_vnode.get(), Some(fdvp)));
    kassert!(same(tdnode.tn_vnode.get(), Some(tdvp)));
    kassert!(fdnode.tn_type.get() == VDIR);
    kassert!(tdnode.tn_type.get() == VDIR);

    let mount = vmount(fdvp);
    kassert!(same(Some(mount), tdvp.v_mount.get()));
    // XXX How can we be sure this stays true? (Not that you're likely to mount a tmpfs
    // read-only...)
    kassert!(mount.mnt_flag.get() & MNT_RDONLY == 0);
    let tmpfs = VFS_TO_TMPFS(mount);

    // Decide whether we need a new name, and allocate memory for it if so. Do this before
    // locking anything or taking destructive actions so that we can back out safely and
    // sleep safely. XXX Is sleeping an issue here? Can this just be moved into
    // tmpfs_rename_attachdetach?
    let tlen = tcnp.cn_namelen as usize;
    let mut newname = if tmpfs_strname_neqlen(fcnp, tcnp) {
        match tmpfs_strname_alloc(tmpfs, tlen) {
            Some(name) => Some(name),
            None => return Err(Errno::ENOSPC), // out_unlocked, with nothing to free
        }
    } else {
        None
    };

    let error: Result<(), Errno> = 'out_unlocked: {
        // Lock and look up everything. GCC is not very clever.
        let RenameEntered { fde, fvp, tde, tvp } =
            match tmpfs_rename_enter(mount, tmpfs, cred, fdvp, fdnode, fcnp, tdvp, tdnode, tcnp) {
                Ok(entered) => entered,
                Err(e) => break 'out_unlocked Err(e),
            };

        // Check that everything is locked and looks right.
        let fnode = td_node(fde);
        kassert!(same(fnode.tn_vnode.get(), Some(fvp)));
        kassert!(fnode.tn_type.get() == fvp.v_type.get());
        kassert!(tde.is_none() == tvp.is_none());
        kassert!(tde.is_none_or(|tde| tde.td_node.get().is_some()));
        kassert!(tde.is_none_or(|tde| same(td_node(tde).tn_vnode.get(), tvp)));
        kassert!(tde.is_none_or(|tde| {
            tvp.is_some_and(|tvp| td_node(tde).tn_type.get() == tvp.v_type.get())
        }));
        kassert!(VOP_ISLOCKED(fdvp) == LK_EXCLUSIVE);
        kassert!(VOP_ISLOCKED(tdvp) == LK_EXCLUSIVE);
        kassert!(VOP_ISLOCKED(fvp) == LK_EXCLUSIVE);
        kassert!(tvp.is_none_or(|tvp| VOP_ISLOCKED(tvp) == LK_EXCLUSIVE));

        let error: Result<(), Errno> = 'out_locked: {
            if same(Some(fvp), tvp) {
                // If the source and destination are the same object, we need only at most
                // delete the source entry.
                kassert!(tvp.is_some());
                if fnode.tn_type.get() == VDIR {
                    // XXX How can this possibly happen?
                    break 'out_locked Err(Errno::EINVAL);
                }
                if !posixly_correct && !same(Some(fde), tde) {
                    // XXX Doesn't work because of locking.
                    // error = VOP_REMOVE(fdvp, fvp);
                    if let Err(e) = tmpfs_do_remove(tmpfs, fdvp, fdnode, fde, fvp, cred) {
                        break 'out_locked Err(e);
                    }
                }
                // goto success
            } else {
                kassert!(!same(Some(fde), tde));
                kassert!(!same(Some(fvp), tvp));

                // If the target exists, refuse to rename a directory over a non-directory
                // or vice versa, or to clobber a non-empty directory.
                if let Some(tvp) = tvp {
                    kassert!(tde.is_some_and(|tde| tde.td_node.get().is_some()));
                    let fdir = fvp.v_type.get() == VDIR;
                    let tdir = tvp.v_type.get() == VDIR;
                    let error = match (fdir, tdir) {
                        (true, true) if tde.is_some_and(|tde| td_node(tde).tn_size.get() > 0) => {
                            Err(Errno::ENOTEMPTY)
                        }
                        (true, true) | (false, false) => Ok(()),
                        (true, false) => Err(Errno::ENOTDIR),
                        (false, true) => Err(Errno::EISDIR),
                    };
                    if let Err(e) = error {
                        break 'out_locked Err(e);
                    }
                    kassert!(fdir == tdir);
                }

                // Authorize the rename.
                let tnode = tde.map(td_node);
                if let Err(e) = tmpfs_rename_check_possible(fdnode, fnode, tdnode, tnode) {
                    break 'out_locked Err(e);
                }
                if let Err(e) = tmpfs_rename_check_permitted(cred, fdnode, fnode, tdnode, tnode) {
                    break 'out_locked Err(e);
                }

                // Everything is hunky-dory. Shuffle the directory entries.
                tmpfs_rename_attachdetach(tmpfs, fdvp, fde, fvp, tdvp, tde, tvp);

                // Update the directory entry's name necessary, and flag metadata updates. A
                // memory allocation failure here is not OK because we've already committed
                // some changes that we can't back out at this point, and we have things
                // locked so we can't sleep, hence the early allocation above.
                if let Some(name) = newname.take() {
                    kassert!(tlen <= TMPFS_MAXNAMLEN);

                    if let Some(old) = NonNull::new(fde.td_name.get()) {
                        tmpfs_strname_free(tmpfs, old, usize::from(fde.td_namelen.get()));
                    }
                    fde.td_namelen.set(tlen as u16);
                    let tname = tcnp.name();
                    // SAFETY: `name` is a fresh `tmpfs_strname_alloc` buffer of `tlen` bytes,
                    // the length of `tname`, and nothing else points at it yet.
                    unsafe { ptr::copy_nonoverlapping(tname.as_ptr(), name.as_ptr(), tname.len()) };
                    // Commit newname and don't free it on the way out.
                    fde.td_name.set(name.as_ptr());

                    tmpfs_update(td_node(fde), TMPFS_NODE_CHANGED);
                    tmpfs_update(tdnode, TMPFS_NODE_MODIFIED);
                }
            }

            // success:
            VN_KNOTE(fvp, NOTE_RENAME);
            tmpfs_rename_cache_purge(fdvp, fvp, tdvp, tvp);
            Ok(())
        };

        // out_locked:
        tmpfs_rename_exit(tmpfs, fdvp, fvp, tdvp, tvp);
        error
    };

    // out_unlocked:
    // KASSERT(VOP_ISLOCKED(fdvp) != LK_EXCLUSIVE);
    // KASSERT(VOP_ISLOCKED(tdvp) != LK_EXCLUSIVE);
    // KASSERT((fvp == NULL) || (VOP_ISLOCKED(fvp) != LK_EXCLUSIVE));
    // KASSERT((tvp == NULL) || (VOP_ISLOCKED(tvp) != LK_EXCLUSIVE));

    if let Some(name) = newname {
        tmpfs_strname_free(tmpfs, name, tlen);
    }

    error
}

/// `tmpfs_rename_enter`: look up `fcnp` in `fdnode`/`fdvp` and return its directory entry
/// and the associated vnode; fail if not found. Look up `tcnp` in `tdnode`/`tdvp` and
/// return its directory entry and the associated vnode, or none if not found. Fail if
/// anything has been mounted on any of the nodes involved.
///
/// `fdvp` and `tdvp` must be referenced.
///
/// On entry, nothing is locked.
///
/// On success, everything is locked, and `fvp`, and `tvp` if any, are referenced. The only
/// pairs of vnodes that may be identical are {`fdvp`, `tdvp`} and {`fvp`, `tvp`}.
///
/// On failure, everything remains as was.
///
/// Locking everything including the source and target nodes is necessary to make sure that,
/// e.g., link count updates are OK. The locking order is, in general, ancestor-first,
/// matching the order you need to use to look up a descendant anyway.
#[allow(clippy::too_many_arguments)] // the C's parameters
fn tmpfs_rename_enter(
    mount: &'static Mount,
    tmpfs: &TmpfsMount,
    cred: *const Ucred,
    fdvp: &'static Vnode,
    fdnode: &'static TmpfsNode,
    fcnp: &Componentname,
    tdvp: &'static Vnode,
    tdnode: &'static TmpfsNode,
    tcnp: &Componentname,
) -> Result<RenameEntered, Errno> {
    kassert!(same(fdnode.tn_vnode.get(), Some(fdvp)));
    kassert!(same(tdnode.tn_vnode.get(), Some(tdvp)));
    kassert!(fdnode.tn_type.get() == VDIR);
    kassert!(tdnode.tn_type.get() == VDIR);

    let entered = if ptr::eq(fdvp, tdvp) {
        kassert!(ptr::eq(fdnode, tdnode));
        tmpfs_rename_enter_common(mount, tmpfs, cred, fdvp, fdnode, fcnp, tcnp)?
    } else {
        kassert!(!ptr::eq(fdnode, tdnode));
        tmpfs_rename_enter_separate(mount, tmpfs, cred, fdvp, fdnode, fcnp, tdvp, tdnode, tcnp)?
    };

    let RenameEntered { fde, fvp, tde, tvp } = &entered;
    kassert!(tde.is_none() == tvp.is_none());
    kassert!(tde.is_none_or(|tde| tde.td_node.get().is_some()));
    kassert!(tde.is_none_or(|tde| same(td_node(tde).tn_vnode.get(), *tvp)));
    kassert!(fde.td_node.get().is_some());
    kassert!(VOP_ISLOCKED(fdvp) == LK_EXCLUSIVE);
    kassert!(VOP_ISLOCKED(fvp) == LK_EXCLUSIVE);
    kassert!(VOP_ISLOCKED(tdvp) == LK_EXCLUSIVE);
    kassert!(tvp.is_none_or(|tvp| VOP_ISLOCKED(tvp) == LK_EXCLUSIVE));
    kassert!(!ptr::eq(*fvp, fdvp));
    kassert!(!ptr::eq(*fvp, tdvp));
    kassert!(!same(*tvp, Some(fdvp)));
    kassert!(!same(*tvp, Some(tdvp)));
    Ok(entered)
}

/// `tmpfs_rename_enter_common`: lock and look up with a common source/target directory.
fn tmpfs_rename_enter_common(
    mount: &'static Mount,
    _tmpfs: &TmpfsMount,
    cred: *const Ucred,
    dvp: &'static Vnode,
    dnode: &'static TmpfsNode,
    fcnp: &Componentname,
    tcnp: &Componentname,
) -> Result<RenameEntered, Errno> {
    tmpfs_rename_lock_directory(dvp, dnode)?; // fail0

    let parent = dnode.tn_spec.tn_dir.tn_parent.get();
    let entered: Result<RenameEntered, Errno> = 'fail1: {
        // Did we lose a race with mount?
        if dvp.v_mountedhere().is_some() {
            break 'fail1 Err(Errno::EBUSY);
        }

        // Make sure the caller may read the directory.
        if let Err(e) = VOP_ACCESS(dvp, VEXEC, cred, curp()) {
            break 'fail1 Err(e);
        }

        // The order in which we lock the source and target nodes is irrelevant because
        // there can only be one rename on this directory in flight at a time, and we have
        // it locked.

        let Some(fde) = tmpfs_dir_lookup(dnode, fcnp) else {
            break 'fail1 Err(Errno::ENOENT);
        };

        let fnode = td_node(fde);
        // We ruled out `.' earlier.
        kassert!(!ptr::eq(fnode, dnode));
        // We ruled out `..' earlier.
        kassert!(!same(Some(fnode), parent));
        rw_enter_write(&fnode.tn_nlock);
        let fvp = match tmpfs_vnode_get(mount, fnode) {
            Ok(fvp) => fvp,
            Err(e) => break 'fail1 Err(e),
        };
        kassert!(VOP_ISLOCKED(fvp) == LK_EXCLUSIVE);
        kassert!(!ptr::eq(fvp, dvp));
        kassert!(same(fvp.v_mount.get(), Some(mount)));

        let entered: Result<RenameEntered, Errno> = 'fail2: {
            // Refuse to rename a mount point.
            if fvp.v_type.get() == VDIR && fvp.v_mountedhere().is_some() {
                break 'fail2 Err(Errno::EBUSY);
            }

            let tde = tmpfs_dir_lookup(dnode, tcnp);
            let tvp = match tde {
                None => None,
                Some(tde) => {
                    let tnode = td_node(tde);
                    // We ruled out `.' earlier.
                    kassert!(!ptr::eq(tnode, dnode));
                    // We ruled out `..' earlier.
                    kassert!(!same(Some(tnode), parent));
                    let tvp = if !ptr::eq(tnode, fnode) {
                        rw_enter_write(&tnode.tn_nlock);
                        let tvp = match tmpfs_vnode_get(mount, tnode) {
                            Ok(tvp) => tvp,
                            Err(e) => break 'fail2 Err(e),
                        };
                        kassert!(same(tvp.v_mount.get(), Some(mount)));
                        // Refuse to rename over a mount point.
                        if tvp.v_type.get() == VDIR && tvp.v_mountedhere().is_some() {
                            // fail3: tvp is not fvp here.
                            vput(tvp);
                            break 'fail2 Err(Errno::EBUSY);
                        }
                        tvp
                    } else {
                        vref(fvp);
                        fvp
                    };
                    kassert!(VOP_ISLOCKED(tvp) == LK_EXCLUSIVE);
                    Some(tvp)
                }
            };
            kassert!(!same(tvp, Some(dvp)));

            Ok(RenameEntered { fde, fvp, tde, tvp })
        };
        if entered.is_err() {
            // fail2:
            vput(fvp);
        }
        entered
    };
    if entered.is_err() {
        // fail1:
        let _ = VOP_UNLOCK(dvp);
    }
    entered
}

/// `tmpfs_rename_enter_separate`: lock and look up with separate source and target
/// directories.
#[allow(clippy::too_many_arguments)] // the C's parameters
fn tmpfs_rename_enter_separate(
    mount: &'static Mount,
    tmpfs: &TmpfsMount,
    cred: *const Ucred,
    fdvp: &'static Vnode,
    fdnode: &'static TmpfsNode,
    fcnp: &Componentname,
    tdvp: &'static Vnode,
    tdnode: &'static TmpfsNode,
    tcnp: &Componentname,
) -> Result<RenameEntered, Errno> {
    kassert!(!ptr::eq(fdvp, tdvp));
    kassert!(!ptr::eq(fdnode, tdnode));

    // #if 0 XXX: mutex_enter(&tmpfs->tm_rename_lock);

    let intermediate_node = tmpfs_rename_genealogy(fdnode, tdnode)?;

    // intermediate_node == NULL means fdnode is not an ancestor of tdnode.
    let (fde, fvp, tde, tvp) = if intermediate_node.is_none() {
        let l = tmpfs_rename_lock(
            mount,
            cred,
            Errno::ENOTEMPTY,
            tdvp,
            tdnode,
            tcnp,
            true,
            fdvp,
            fdnode,
            fcnp,
            false,
        )?;
        (l.b_dirent, l.b_vp, l.a_dirent, l.a_vp)
    } else {
        let l = tmpfs_rename_lock(
            mount,
            cred,
            Errno::EINVAL,
            fdvp,
            fdnode,
            fcnp,
            false,
            tdvp,
            tdnode,
            tcnp,
            true,
        )?;
        (l.a_dirent, l.a_vp, l.b_dirent, l.b_vp)
    };

    // The source was not missing-ok: tmpfs_rename_lock found it, with its vnode.
    let (Some(fde), Some(fvp)) = (fde, fvp) else {
        panic(format_args!("tmpfs_rename_enter_separate: no source"));
    };
    kassert!(fde.td_node.get().is_some());

    // Reject rename("foo/bar", "foo/bar/baz/quux/zot").
    if same(fde.td_node.get(), intermediate_node) {
        tmpfs_rename_exit(tmpfs, fdvp, fvp, tdvp, tvp);
        return Err(Errno::EINVAL);
    }

    Ok(RenameEntered { fde, fvp, tde, tvp })

    // fail: #if 0 XXX: mutex_exit(&tmpfs->tm_rename_lock);
}

/// `tmpfs_rename_exit`: unlock everything we locked for rename.
///
/// `fdvp` and `tdvp` must be referenced.
///
/// On entry, everything is locked, and `fvp` and `tvp` referenced.
///
/// On exit, everything is unlocked, and `fvp` and `tvp` are released.
fn tmpfs_rename_exit(
    _tmpfs: &TmpfsMount,
    fdvp: &'static Vnode,
    fvp: &'static Vnode,
    tdvp: &'static Vnode,
    tvp: Option<&'static Vnode>,
) {
    kassert!(!ptr::eq(fdvp, fvp));
    kassert!(!same(Some(fdvp), tvp));
    kassert!(!same(Some(tdvp), tvp));
    kassert!(!ptr::eq(tdvp, fvp));
    kassert!(VOP_ISLOCKED(fdvp) == LK_EXCLUSIVE);
    kassert!(VOP_ISLOCKED(tdvp) == LK_EXCLUSIVE);
    kassert!(VOP_ISLOCKED(fvp) == LK_EXCLUSIVE);
    kassert!(tvp.is_none_or(|tvp| VOP_ISLOCKED(tvp) == LK_EXCLUSIVE));

    if let Some(tvp) = tvp {
        if !ptr::eq(tvp, fvp) {
            vput(tvp);
        } else {
            vrele(tvp);
        }
    }
    let _ = VOP_UNLOCK(tdvp);
    vput(fvp);
    if !ptr::eq(fdvp, tdvp) {
        let _ = VOP_UNLOCK(fdvp);
    }

    // #if 0 XXX: if (fdvp != tdvp) mutex_exit(&tmpfs->tm_rename_lock);
}

/// `tmpfs_rename_lock_directory`: lock a directory, but fail if it has been rmdir'd.
///
/// `vp` must be referenced.
fn tmpfs_rename_lock_directory(vp: &'static Vnode, node: &'static TmpfsNode) -> Result<(), Errno> {
    kassert!(same(node.tn_vnode.get(), Some(vp)));
    kassert!(node.tn_type.get() == VDIR);

    let _ = vn_lock(vp, LK_EXCLUSIVE | LK_RETRY);
    if node.tn_spec.tn_dir.tn_parent.get().is_none() {
        let _ = VOP_UNLOCK(vp);
        return Err(Errno::ENOENT);
    }

    Ok(())
}

/// `tmpfs_rename_genealogy`: analyze the genealogy of the source and target nodes.
///
/// On success, returns either the child of `fdnode` of which `tdnode` is a descendant, or
/// none if `tdnode` is not a descendant of `fdnode` at all.
///
/// `fdnode` and `tdnode` must be unlocked and referenced. The file system's rename lock
/// must also be held, to exclude concurrent changes to the file system's genealogy other
/// than rmdir.
///
/// XXX This causes an extra lock/unlock of `tdnode` in the case when we're just about to
/// lock it again before locking anything else. However, changing that requires
/// reorganizing the code to make it even more horrifically obscure.
fn tmpfs_rename_genealogy(
    fdnode: &'static TmpfsNode,
    tdnode: &'static TmpfsNode,
) -> Result<Option<&'static TmpfsNode>, Errno> {
    kassert!(!ptr::eq(fdnode, tdnode));
    kassert!(fdnode.tn_vnode.get().is_some());
    kassert!(tdnode.tn_vnode.get().is_some());
    kassert!(fdnode.tn_type.get() == VDIR);
    kassert!(tdnode.tn_type.get() == VDIR);

    // We need to provisionally lock tdnode->tn_vnode to keep rmdir from deleting it -- or
    // any ancestor -- at an inopportune moment.
    tmpfs_rename_lock_directory(tn_vnode(tdnode), tdnode)?;

    let mut node = tdnode;
    let intermediate_node = loop {
        let Some(parent) = node.tn_spec.tn_dir.tn_parent.get() else {
            panic(format_args!(
                "tmpfs_rename_genealogy: directory {:p} without a parent",
                node
            ));
        };
        kassert!(parent.tn_type.get() == VDIR);

        // Did we hit the root without finding fdnode?
        if ptr::eq(parent, node) {
            break None;
        }

        // Did we find that fdnode is an ancestor?
        if ptr::eq(parent, fdnode) {
            break Some(node);
        }

        // Neither -- keep ascending the family tree.
        node = parent;
    };

    let _ = VOP_UNLOCK(tn_vnode(tdnode));
    Ok(intermediate_node)
}

/// `tmpfs_rename_lock`: lock directories a and b, which must be distinct, and look up and
/// lock nodes a and b. Do a first and then b. Directory b may not be an ancestor of
/// directory a, although directory a may be an ancestor of directory b. Fail with
/// `overlap_error` if node a is directory b. Neither componentname may be `.` or `..`.
///
/// `a_dvp` and `b_dvp` must be referenced.
///
/// On entry, `a_dvp` and `b_dvp` are unlocked.
///
/// On success,
/// - `a_dvp` and `b_dvp` are locked,
/// - `a_dirent` is either none or a directory entry whose node is locked and referenced,
/// - `a_vp` is the corresponding vnode,
/// - `b_dirent` is either none or a directory entry whose node is locked and referenced,
/// - `b_vp` is either none or the corresponding vnode, and
/// - the only pair of vnodes that may be identical is `a_vp` and `b_vp`.
///
/// On failure, `a_dvp` and `b_dvp` are left unlocked and nothing is returned.
#[allow(clippy::too_many_arguments)] // the C's parameters
fn tmpfs_rename_lock(
    mount: &'static Mount,
    cred: *const Ucred,
    overlap_error: Errno,
    a_dvp: &'static Vnode,
    a_dnode: &'static TmpfsNode,
    a_cnp: &Componentname,
    a_missing_ok: bool,
    b_dvp: &'static Vnode,
    b_dnode: &'static TmpfsNode,
    b_cnp: &Componentname,
    b_missing_ok: bool,
) -> Result<RenameLocked, Errno> {
    kassert!(!ptr::eq(a_dvp, b_dvp));
    kassert!(!ptr::eq(a_dnode, b_dnode));
    kassert!(same(a_dnode.tn_vnode.get(), Some(a_dvp)));
    kassert!(same(b_dnode.tn_vnode.get(), Some(b_dvp)));
    kassert!(a_dnode.tn_type.get() == VDIR);
    kassert!(b_dnode.tn_type.get() == VDIR);
    kassert!(a_missing_ok != b_missing_ok);

    tmpfs_rename_lock_directory(a_dvp, a_dnode)?; // fail0

    let locked: Result<RenameLocked, Errno> = 'fail1: {
        // Did we lose a race with mount?
        if a_dvp.v_mountedhere().is_some() {
            break 'fail1 Err(Errno::EBUSY);
        }

        // Make sure the caller may read the directory.
        if let Err(e) = VOP_ACCESS(a_dvp, VEXEC, cred, curp()) {
            break 'fail1 Err(e);
        }

        let a_dirent = tmpfs_dir_lookup(a_dnode, a_cnp);
        let a_vp = match a_dirent {
            Some(a_dirent) => {
                let a_node = td_node(a_dirent);
                // We ruled out `.' earlier.
                kassert!(!ptr::eq(a_node, a_dnode));
                // We ruled out `..' earlier.
                kassert!(!same(Some(a_node), a_dnode.tn_spec.tn_dir.tn_parent.get()));
                if ptr::eq(a_node, b_dnode) {
                    break 'fail1 Err(overlap_error);
                }
                rw_enter_write(&a_node.tn_nlock);
                let a_vp = match tmpfs_vnode_get(mount, a_node) {
                    Ok(a_vp) => a_vp,
                    Err(e) => break 'fail1 Err(e),
                };
                kassert!(same(a_vp.v_mount.get(), Some(mount)));
                // Refuse to rename (over) a mount point.
                if a_vp.v_type.get() == VDIR && a_vp.v_mountedhere().is_some() {
                    // fail2:
                    kassert!(VOP_ISLOCKED(a_vp) == LK_EXCLUSIVE);
                    vput(a_vp);
                    break 'fail1 Err(Errno::EBUSY);
                }
                Some(a_vp)
            }
            None if !a_missing_ok => break 'fail1 Err(Errno::ENOENT),
            None => None,
        };
        kassert!(!same(a_vp, Some(a_dvp)));
        kassert!(!same(a_vp, Some(b_dvp)));

        let locked: Result<RenameLocked, Errno> = 'fail2: {
            if let Err(e) = tmpfs_rename_lock_directory(b_dvp, b_dnode) {
                break 'fail2 Err(e);
            }

            let locked: Result<RenameLocked, Errno> = 'fail3: {
                // Did we lose a race with mount?
                if b_dvp.v_mountedhere().is_some() {
                    break 'fail3 Err(Errno::EBUSY);
                }

                // Make sure the caller may read the directory.
                if let Err(e) = VOP_ACCESS(b_dvp, VEXEC, cred, curp()) {
                    break 'fail3 Err(e);
                }

                let b_dirent = tmpfs_dir_lookup(b_dnode, b_cnp);
                let b_vp = match b_dirent {
                    Some(b_dirent) => {
                        let b_node = td_node(b_dirent);
                        // We ruled out `.' earlier.
                        kassert!(!ptr::eq(b_node, b_dnode));
                        // We ruled out `..' earlier.
                        kassert!(!same(Some(b_node), b_dnode.tn_spec.tn_dir.tn_parent.get()));
                        // b is not an ancestor of a.
                        kassert!(!ptr::eq(b_node, a_dnode));
                        // But the source and target nodes might be the same.
                        if a_dirent.is_none_or(|a_dirent| !ptr::eq(td_node(a_dirent), b_node)) {
                            rw_enter_write(&b_node.tn_nlock);
                            let b_vp = match tmpfs_vnode_get(mount, b_node) {
                                Ok(b_vp) => b_vp,
                                Err(e) => break 'fail3 Err(e),
                            };
                            kassert!(same(b_vp.v_mount.get(), Some(mount)));
                            kassert!(!same(a_vp, Some(b_vp)));
                            // Refuse to rename (over) a mount point.
                            if b_vp.v_type.get() == VDIR && b_vp.v_mountedhere().is_some() {
                                // fail4: b_vp is not a_vp here.
                                kassert!(VOP_ISLOCKED(b_vp) == LK_EXCLUSIVE);
                                vput(b_vp);
                                break 'fail3 Err(Errno::EBUSY);
                            }
                            Some(b_vp)
                        } else {
                            // a_dirent names the node, which tmpfs_vnode_get gave a vnode.
                            let Some(a_vp) = a_vp else {
                                panic(format_args!("tmpfs_rename_lock: no vnode for a"));
                            };
                            vref(a_vp);
                            Some(a_vp)
                        }
                    }
                    None if !b_missing_ok => break 'fail3 Err(Errno::ENOENT),
                    None => None,
                };
                kassert!(!same(b_vp, Some(a_dvp)));
                kassert!(!same(b_vp, Some(b_dvp)));

                kassert!(VOP_ISLOCKED(a_dvp) == LK_EXCLUSIVE);
                kassert!(VOP_ISLOCKED(b_dvp) == LK_EXCLUSIVE);
                kassert!(a_missing_ok || a_dirent.is_some());
                kassert!(b_missing_ok || b_dirent.is_some());
                kassert!(a_dirent.is_none_or(|de| de.td_node.get().is_some()));
                kassert!(a_dirent.is_none_or(|de| same(td_node(de).tn_vnode.get(), a_vp)));
                kassert!(b_dirent.is_none_or(|de| de.td_node.get().is_some()));
                kassert!(b_dirent.is_none_or(|de| same(td_node(de).tn_vnode.get(), b_vp)));
                kassert!(a_vp.is_none_or(|vp| VOP_ISLOCKED(vp) == LK_EXCLUSIVE));
                kassert!(b_vp.is_none_or(|vp| VOP_ISLOCKED(vp) == LK_EXCLUSIVE));

                Ok(RenameLocked {
                    a_dirent,
                    a_vp,
                    b_dirent,
                    b_vp,
                })
            };
            if locked.is_err() {
                // fail3:
                kassert!(VOP_ISLOCKED(b_dvp) == LK_EXCLUSIVE);
                let _ = VOP_UNLOCK(b_dvp);
            }
            locked
        };
        if locked.is_err()
            && let Some(a_vp) = a_vp
        {
            // fail2:
            kassert!(VOP_ISLOCKED(a_vp) == LK_EXCLUSIVE);
            vput(a_vp);
        }
        locked
    };
    if locked.is_err() {
        // fail1:
        kassert!(VOP_ISLOCKED(a_dvp) == LK_EXCLUSIVE);
        let _ = VOP_UNLOCK(a_dvp);
    }

    // fail0:
    // KASSERT(VOP_ISLOCKED(a_dvp) != LK_EXCLUSIVE);
    // KASSERT(VOP_ISLOCKED(b_dvp) != LK_EXCLUSIVE);
    // KASSERT((a_vp == NULL) || (VOP_ISLOCKED(a_vp) != LK_EXCLUSIVE));
    // KASSERT((b_vp == NULL) || (VOP_ISLOCKED(b_vp) != LK_EXCLUSIVE));
    locked
}

/// `tmpfs_rename_attachdetach`: shuffle the directory entries to move `fvp` from the
/// directory `fdvp` into the directory `tdvp`. `fde` is `fvp`'s directory entry in `fdvp`.
/// If we are overwriting a target node, it is `tvp`, and `tde` is its directory entry in
/// `tdvp`.
///
/// `fdvp`, `fvp`, `tdvp`, and `tvp` must all be locked and referenced.
fn tmpfs_rename_attachdetach(
    tmpfs: &TmpfsMount,
    fdvp: &'static Vnode,
    fde: &'static TmpfsDirent,
    fvp: &'static Vnode,
    tdvp: &'static Vnode,
    tde: Option<&'static TmpfsDirent>,
    tvp: Option<&'static Vnode>,
) {
    kassert!(fde.td_node.get().is_some());
    kassert!(same(td_node(fde).tn_vnode.get(), Some(fvp)));
    kassert!(tde.is_none() == tvp.is_none());
    kassert!(tde.is_none_or(|tde| tde.td_node.get().is_some()));
    kassert!(tde.is_none_or(|tde| same(td_node(tde).tn_vnode.get(), tvp)));
    kassert!(VOP_ISLOCKED(fdvp) == LK_EXCLUSIVE);
    kassert!(VOP_ISLOCKED(tdvp) == LK_EXCLUSIVE);
    kassert!(VOP_ISLOCKED(fvp) == LK_EXCLUSIVE);
    kassert!(tvp.is_none_or(|tvp| VOP_ISLOCKED(tvp) == LK_EXCLUSIVE));

    // If we are moving from one directory to another, detach the source entry and reattach
    // it to the target directory.
    if !ptr::eq(fdvp, tdvp) {
        // tmpfs_dir_detach clobbers fde->td_node, so save it.
        let fnode = td_node(fde);
        let fdnode = VP_TO_TMPFS_DIR(fdvp);
        let tdnode = VP_TO_TMPFS_DIR(tdvp);
        tmpfs_dir_detach(fdnode, fde);
        tmpfs_dir_attach(tdnode, fde, fnode);
    } else if tvp.is_none() {
        // We are changing the directory. tmpfs_dir_attach and tmpfs_dir_detach note the
        // events for us, but for this case we don't call them, so we must note the event
        // explicitly.
        VN_KNOTE(fdvp, NOTE_WRITE);
    }

    // If we are replacing an existing target entry, delete it.
    if let Some(tde) = tde {
        let tdnode = VP_TO_TMPFS_DIR(tdvp);
        let tnode = td_node(tde);
        kassert!(tvp.is_some_and(|tvp| (fvp.v_type.get() == VDIR) == (tvp.v_type.get() == VDIR)));
        if tnode.tn_type.get() == VDIR {
            kassert!(tnode.tn_size.get() == 0);
            kassert!(tnode.tn_links.get() == 2);
            // Decrement the extra link count for `.' so the vnode will be recycled when
            // released.
            tnode.tn_links.set(tnode.tn_links.get() - 1);
        }
        tmpfs_dir_detach(tdnode, tde);
        tmpfs_free_dirent(tmpfs, tde);
    }
}

/// `tmpfs_do_remove`: remove the entry `de` for the non-directory `vp` from the directory
/// `dvp`.
///
/// Everything must be locked and referenced.
fn tmpfs_do_remove(
    tmpfs: &TmpfsMount,
    dvp: &'static Vnode,
    dnode: &'static TmpfsNode,
    de: &'static TmpfsDirent,
    vp: &'static Vnode,
    cred: *const Ucred,
) -> Result<(), Errno> {
    kassert!(same(dnode.tn_vnode.get(), Some(dvp)));
    kassert!(de.td_node.get().is_some());
    kassert!(same(td_node(de).tn_vnode.get(), Some(vp)));
    kassert!(VOP_ISLOCKED(dvp) == LK_EXCLUSIVE);
    kassert!(VOP_ISLOCKED(vp) == LK_EXCLUSIVE);

    let node = td_node(de);
    tmpfs_remove_check_possible(dnode, node)?;

    tmpfs_remove_check_permitted(cred, dnode, node)?;

    // If not root and directory is sticky, check for permission on directory or on file.
    // This implements append-only directories.
    if dnode.tn_mode.get() & S_ISTXT != 0 {
        let uid = ucred(cred).cr_uid.get();
        if uid != 0 && uid != dnode.tn_uid.get() && uid != node.tn_uid.get() {
            return Err(Errno::EPERM);
        }
    }

    tmpfs_dir_detach(dnode, de);
    tmpfs_free_dirent(tmpfs, de);

    Ok(())
}

/// `tmpfs_rename_check_possible`: check whether a rename is possible independent of
/// credentials.
///
/// Everything must be locked and referenced.
fn tmpfs_rename_check_possible(
    fdnode: &'static TmpfsNode,
    fnode: &'static TmpfsNode,
    tdnode: &'static TmpfsNode,
    tnode: Option<&'static TmpfsNode>,
) -> Result<(), Errno> {
    kassert!(!ptr::eq(fdnode, fnode));
    kassert!(!same(Some(tdnode), tnode));
    kassert!(!same(Some(fnode), tnode));
    kassert!(fdnode.tn_vnode.get().is_some());
    kassert!(fnode.tn_vnode.get().is_some());
    kassert!(tdnode.tn_vnode.get().is_some());
    kassert!(tnode.is_none_or(|tnode| tnode.tn_vnode.get().is_some()));
    kassert!(VOP_ISLOCKED(tn_vnode(fdnode)) == LK_EXCLUSIVE);
    kassert!(VOP_ISLOCKED(tn_vnode(fnode)) == LK_EXCLUSIVE);
    kassert!(VOP_ISLOCKED(tn_vnode(tdnode)) == LK_EXCLUSIVE);
    kassert!(tnode.is_none_or(|tnode| VOP_ISLOCKED(tn_vnode(tnode)) == LK_EXCLUSIVE));

    // If fdnode is immutable, we can't write to it. If fdnode is append-only, the only
    // change we can make is to add entries to it. If fnode is immutable, we can't change
    // the links to it. If fnode is append-only...well, this is what UFS does.
    if (fdnode.tn_flags.get() | fnode.tn_flags.get()) & (IMMUTABLE | APPEND) != 0 {
        return Err(Errno::EPERM);
    }

    // If tdnode is immutable, we can't write to it. If tdnode is append-only, we can add
    // entries, but we can't change existing entries.
    let append = if tnode.is_some() { APPEND } else { 0 };
    if tdnode.tn_flags.get() & (IMMUTABLE | append) != 0 {
        return Err(Errno::EPERM);
    }

    // If tnode is immutable, we can't replace links to it. If tnode is append-only...well,
    // this is what UFS does.
    if let Some(tnode) = tnode
        && tnode.tn_flags.get() & (IMMUTABLE | APPEND) != 0
    {
        return Err(Errno::EPERM);
    }

    Ok(())
}

/// `tmpfs_rename_check_permitted`: check whether a rename is permitted given our
/// credentials.
///
/// Everything must be locked and referenced.
fn tmpfs_rename_check_permitted(
    cred: *const Ucred,
    fdnode: &'static TmpfsNode,
    fnode: &'static TmpfsNode,
    tdnode: &'static TmpfsNode,
    tnode: Option<&'static TmpfsNode>,
) -> Result<(), Errno> {
    kassert!(!ptr::eq(fdnode, fnode));
    kassert!(!same(Some(tdnode), tnode));
    kassert!(!same(Some(fnode), tnode));
    kassert!(fdnode.tn_vnode.get().is_some());
    kassert!(fnode.tn_vnode.get().is_some());
    kassert!(tdnode.tn_vnode.get().is_some());
    kassert!(tnode.is_none_or(|tnode| tnode.tn_vnode.get().is_some()));
    kassert!(VOP_ISLOCKED(tn_vnode(fdnode)) == LK_EXCLUSIVE);
    kassert!(VOP_ISLOCKED(tn_vnode(fnode)) == LK_EXCLUSIVE);
    kassert!(VOP_ISLOCKED(tn_vnode(tdnode)) == LK_EXCLUSIVE);
    kassert!(tnode.is_none_or(|tnode| VOP_ISLOCKED(tn_vnode(tnode)) == LK_EXCLUSIVE));

    // We need to remove or change an entry in the source directory.
    VOP_ACCESS(tn_vnode(fdnode), VWRITE, cred, curp())?;

    // If we are changing directories, then we need to write to the target directory to add
    // or change an entry. Also, if fnode is a directory, we need to write to it to change
    // its `..' entry.
    if !ptr::eq(fdnode, tdnode) {
        VOP_ACCESS(tn_vnode(tdnode), VWRITE, cred, curp())?;
        if fnode.tn_type.get() == VDIR {
            VOP_ACCESS(tn_vnode(fnode), VWRITE, cred, curp())?;
        }
    }

    tmpfs_check_sticky(cred, fdnode, Some(fnode))?;

    if tmpfs_dirseq_full(tdnode) {
        return Err(Errno::ENOSPC);
    }

    tmpfs_check_sticky(cred, tdnode, tnode)?;

    Ok(())
}

/// `tmpfs_remove_check_possible`: check whether removing `node`'s entry in `dnode` is
/// possible independent of credentials.
///
/// Everything must be locked and referenced.
fn tmpfs_remove_check_possible(
    dnode: &'static TmpfsNode,
    node: &'static TmpfsNode,
) -> Result<(), Errno> {
    kassert!(dnode.tn_vnode.get().is_some());
    kassert!(!ptr::eq(dnode, node));
    kassert!(VOP_ISLOCKED(tn_vnode(dnode)) == LK_EXCLUSIVE);
    kassert!(VOP_ISLOCKED(tn_vnode(node)) == LK_EXCLUSIVE);

    // We want to delete the entry. If dnode is immutable, we can't write to it to delete
    // the entry. If dnode is append-only, the only change we can make is to add entries, so
    // we can't delete entries. If node is immutable, we can't change the links to it, so we
    // can't delete the entry. If node is append-only...well, this is what UFS does.
    if (dnode.tn_flags.get() | node.tn_flags.get()) & (IMMUTABLE | APPEND) != 0 {
        return Err(Errno::EPERM);
    }

    Ok(())
}

/// `tmpfs_remove_check_permitted`: check whether removing `node`'s entry in `dnode` is
/// permitted given our credentials.
///
/// Everything must be locked and referenced.
fn tmpfs_remove_check_permitted(
    cred: *const Ucred,
    dnode: &'static TmpfsNode,
    node: &'static TmpfsNode,
) -> Result<(), Errno> {
    kassert!(dnode.tn_vnode.get().is_some());
    kassert!(!ptr::eq(dnode, node));
    kassert!(VOP_ISLOCKED(tn_vnode(dnode)) == LK_EXCLUSIVE);
    kassert!(VOP_ISLOCKED(tn_vnode(node)) == LK_EXCLUSIVE);

    // Check whether we are permitted to write to the source directory in order to delete an
    // entry from it.
    VOP_ACCESS(tn_vnode(dnode), VWRITE, cred, curp())?;

    tmpfs_check_sticky(cred, dnode, Some(node))?;

    Ok(())
}

/// `tmpfs_check_sticky`: check whether we may change an entry in a sticky directory. If the
/// directory is sticky, the user must own either the directory or, if it exists, the node,
/// in order to change the entry.
///
/// Everything must be locked and referenced.
fn tmpfs_check_sticky(
    cred: *const Ucred,
    dnode: &'static TmpfsNode,
    node: Option<&'static TmpfsNode>,
) -> Result<(), Errno> {
    kassert!(dnode.tn_vnode.get().is_some());
    kassert!(VOP_ISLOCKED(tn_vnode(dnode)) == LK_EXCLUSIVE);
    kassert!(node.is_none_or(|node| node.tn_vnode.get().is_some()));
    kassert!(node.is_none() || VOP_ISLOCKED(tn_vnode(dnode)) == LK_EXCLUSIVE);

    let Some(node) = node else {
        return Ok(());
    };

    if dnode.tn_mode.get() & S_ISTXT != 0 {
        let uid = ucred(cred).cr_uid.get();
        if uid != 0 && uid != dnode.tn_uid.get() && uid != node.tn_uid.get() {
            return Err(Errno::EPERM);
        }
    }

    Ok(())
}

/// `tmpfs_rename_cache_purge`: purge the name cache of the vnodes a rename changed.
fn tmpfs_rename_cache_purge(
    fdvp: &'static Vnode,
    fvp: &'static Vnode,
    tdvp: &'static Vnode,
    tvp: Option<&'static Vnode>,
) {
    kassert!(!ptr::eq(fdvp, fvp));
    kassert!(!same(Some(fdvp), tvp));
    kassert!(!ptr::eq(tdvp, fvp));
    kassert!(!same(Some(tdvp), tvp));
    kassert!(!same(Some(fvp), tvp));
    kassert!(fdvp.v_type.get() == VDIR);
    kassert!(tdvp.v_type.get() == VDIR);

    // XXX What actually needs to be purged?

    cache_purge(fdvp);

    if fvp.v_type.get() == VDIR {
        cache_purge(fvp);
    }

    if !ptr::eq(tdvp, fdvp) {
        cache_purge(tdvp);
    }

    if let Some(tvp) = tvp
        && tvp.v_type.get() == VDIR
    {
        cache_purge(tvp);
    }
}

/// `tmpfs_rename_abort`: abort both lookups of a rename and release every vnode.
fn tmpfs_rename_abort(ap: &mut VopRenameArgs<'_>) {
    let fdvp = ap.a_fdvp;
    let fvp = ap.a_fvp;
    let tdvp = ap.a_tdvp;
    let tvp = ap.a_tvp;

    let _ = VOP_ABORTOP(tdvp, ap.a_tcnp);
    if same(Some(tdvp), tvp) {
        vrele(tdvp);
    } else {
        vput(tdvp);
    }
    if let Some(tvp) = tvp {
        vput(tvp);
    }
    let _ = VOP_ABORTOP(fdvp, ap.a_fcnp);
    vrele(fdvp);
    vrele(fvp);
}

/// `tmpfs_kqfilter` (`vop_kqfilter`): attach a knote to the vnode.
pub fn tmpfs_kqfilter(ap: &mut VopKqfilterArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let kn = ap.a_kn;

    match kn.kn_filter().get() {
        EVFILT_READ => kn.kn_fop.set(Some(&TMPFSREAD_FILTOPS)),
        EVFILT_WRITE => kn.kn_fop.set(Some(&TMPFSWRITE_FILTOPS)),
        EVFILT_VNODE => kn.kn_fop.set(Some(&TMPFSVNODE_FILTOPS)),
        _ => return Err(Errno::EINVAL),
    }

    kn.kn_hook.set(ptr::from_ref(vp).cast_mut().cast());

    klist_insert_locked(&vp.v_klist, kn);

    Ok(())
}

/// `kn->kn_hook` of a tmpfs knote: its vnode.
fn kn_vnode(kn: &Knote) -> &'static Vnode {
    // SAFETY: `tmpfs_kqfilter` points `kn_hook` at the vnode, a `vnode_pool` item that is
    // never freed.
    match unsafe { kn.kn_hook.get().cast::<Vnode>().as_ref() } {
        Some(vp) => vp,
        None => panic(format_args!("knote {:p}: no vnode", kn)),
    }
}

/// `filt_tmpfsdetach`: unhooks the knote from the vnode.
pub fn filt_tmpfsdetach(kn: &Knote) {
    let vp = kn_vnode(kn);

    klist_remove_locked(&vp.v_klist, kn);
}

/// `filt_tmpfsread`: the bytes past the file offset; always ready for poll and select.
pub fn filt_tmpfsread(kn: &Knote, hint: i64) -> bool {
    let vp = kn_vnode(kn);
    let node = VP_TO_TMPFS_NODE(vp);

    // filesystem is gone, so set the EOF flag and schedule the knote for deletion.
    if hint == i64::from(NOTE_REVOKE) {
        kn.set_flags(EV_EOF | EV_ONESHOT);
        return true;
    }

    kn.kn_data().set(node.tn_size.get() - foffset(kn.fp()));
    if kn.kn_data().get() == 0 && kn.kn_sfflags.get() & NOTE_EOF != 0 {
        kn.kn_fflags().set(kn.kn_fflags().get() | NOTE_EOF);
        return true;
    }

    if kn.has_flags(__EV_POLL | __EV_SELECT) {
        return true;
    }

    kn.kn_data().get() != 0
}

/// `filt_tmpfswrite`: a file is always writable.
pub fn filt_tmpfswrite(kn: &Knote, hint: i64) -> bool {
    // filesystem is gone, so set the EOF flag and schedule the knote for deletion.
    if hint == i64::from(NOTE_REVOKE) {
        kn.set_flags(EV_EOF | EV_ONESHOT);
        return true;
    }

    kn.kn_data().set(0);
    true
}

/// `filt_tmpfsvnode`: records the vnode events (`NOTE_*`) the user asked for.
pub fn filt_tmpfsvnode(kn: &Knote, hint: i64) -> bool {
    let hint32 = hint as u32;
    if kn.kn_sfflags.get() & hint32 != 0 {
        kn.kn_fflags().set(kn.kn_fflags().get() | hint32);
    }
    if hint == i64::from(NOTE_REVOKE) {
        kn.set_flags(EV_EOF);
        return true;
    }
    kn.kn_fflags().get() != 0
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for the tmpfs vnode operations, through the system calls' own paths
    // (`namei`, `vn_open`, `domkdirat`, `dorenameat`, ...) over a tmpfs mounted as the root
    // (the vfs setup is `vfs_subr.rs`'s, the mount `tmpfs_vfsops.rs`'s). The data
    // path of `tmpfs_read`/`tmpfs_write` (`tmpfs_uiomove`) maps the file's object into
    // `kernel_map` and needs the kernel's faults, so reads and writes of bytes run in the
    // kernel only (the smoke); here a file is resized with `VOP_SETATTR` and read empty.

    use std::boxed::Box;
    use std::sync::MutexGuard;
    use std::vec::Vec;
    use std::{assert, assert_eq};

    use super::*;
    use crate::kern::vfs_init::set_rootvnode;
    use crate::kern::vfs_lookup::{namei, ndinit};
    use crate::kern::vfs_subr::{vfs_mount_alloc, vfs_unbusy};
    use crate::kern::vfs_syscalls::{
        dolinkat, domkdirat, doreadlinkat, dorenameat, dosymlinkat, dounlinkat,
    };
    use crate::kern::vfs_vnops::{vn_close, vn_open, vn_rdwr};
    use crate::kern::vfs_vops::{
        VOP_GETATTR, VOP_KQFILTER, VOP_OPEN, VOP_PATHCONF, VOP_READDIR, VOP_SETATTR,
    };
    use crate::sys::dirent::Dirent;
    use crate::sys::fcntl::{AT_FDCWD, AT_REMOVEDIR, FREAD, O_CREAT};
    use crate::sys::lock::LK_RECURSEFAIL;
    use crate::sys::namei::{FOLLOW, LOCKLEAF, LOOKUP, NiDirp};
    use crate::sys::param::DEV_BSIZE;
    use crate::sys::uio::{Iovec, Uio, UioRw, UioSeg};
    use crate::sys::vnode::Vattr;
    use crate::tmpfs::tmpfs_vfsops::tests::{args, setup, tmpfs_conf};
    use crate::tmpfs::tmpfs_vfsops::{tmpfs_mount, tmpfs_root};

    /// The test setup plus a tmpfs of 1 MB mounted as "/" (mode 0755, owned by root) and made
    /// the thread's current directory.
    fn setup_root() -> (MutexGuard<'static, ()>, &'static Proc) {
        let (g, p) = setup();
        let mp = vfs_mount_alloc(None, tmpfs_conf());
        let mut data = args(1 << 20, 0, 0, 0o755);
        let mut nd = ndinit(LOOKUP, 0, NiDirp::Sys(b"/"), p);
        tmpfs_mount(mp, b"/", &mut data, &mut nd, p).expect("mount");
        vfs_unbusy(mp);
        let root = tmpfs_root(mp).expect("root");
        set_rootvnode(Some(root));
        p.fd().fd_cdir.set(Some(root));
        vref(root);
        let _ = VOP_UNLOCK(root);
        (g, p)
    }

    /// A user-space path for the `do*at` functions: the bytes and a NUL.
    fn c(path: &str) -> Vec<u8> {
        let mut v = path.as_bytes().to_vec();
        v.push(0);
        v
    }

    /// `open(path, O_RDWR | O_CREAT, mode)`: the vnode, unlocked and referenced.
    fn create(p: &'static Proc, path: &[u8], mode: Mode) -> &'static Vnode {
        let mut nd = ndinit(0, 0, NiDirp::Sys(path), p);
        vn_open(&mut nd, FREAD | FWRITE | O_CREAT, mode).expect("create");
        let vp = nd.ni_vp.expect("a vnode");
        let _ = VOP_UNLOCK(vp);
        vp
    }

    /// `close` of a vnode `create` gave.
    fn close(p: &'static Proc, vp: &'static Vnode) {
        vn_close(vp, FREAD | FWRITE, p.ucred(), Some(p)).expect("close");
    }

    /// `lstat(path)`: the attributes of the node `path` names.
    fn stat(p: &'static Proc, path: &[u8]) -> Result<Vattr, Errno> {
        let mut nd = ndinit(LOOKUP, LOCKLEAF, NiDirp::Sys(path), p);
        namei(&mut nd)?;
        let vp = nd.ni_vp.expect("a vnode");
        let mut va = Vattr::new();
        let error = VOP_GETATTR(vp, &mut va, p.ucred(), p);
        vput(vp);
        error.map(|()| va)
    }

    /// The names `getdents` lists in the directory `path`, read in one call.
    fn names(p: &'static Proc, path: &[u8]) -> Vec<Vec<u8>> {
        let mut nd = ndinit(LOOKUP, LOCKLEAF | FOLLOW, NiDirp::Sys(path), p);
        namei(&mut nd).expect("lookup");
        let vp = nd.ni_vp.expect("a vnode");
        let mut buf = std::vec![0u8; 4096];
        let mut iov = [Iovec {
            iov_base: buf.as_mut_ptr().cast(),
            iov_len: buf.len(),
        }];
        let mut uio = Uio {
            uio_iov: &mut iov,
            uio_offset: 0,
            uio_resid: 4096,
            uio_segflg: UioSeg::UIO_SYSSPACE,
            uio_rw: UioRw::UIO_READ,
            uio_procp: None,
        };
        let mut eof = 0;
        VOP_READDIR(vp, &mut uio, p.ucred(), &mut eof).expect("readdir");
        assert_eq!(eof, 1, "one call reads a small directory to its end");
        let used = 4096 - uio.uio_resid;
        vput(vp);

        let mut names = Vec::new();
        let mut off = 0;
        while off < used {
            let d = Dirent::from_bytes(&buf[off..]).expect("a dirent");
            let name = &buf[off + Dirent::NAME_OFFSET..][..usize::from(d.d_namlen)];
            names.push(name.to_vec());
            off += usize::from(d.d_reclen);
        }
        names
    }

    /// `truncate(vp, size)` through `VOP_SETATTR`.
    fn truncate(p: &'static Proc, vp: &'static Vnode, size: u64) -> Result<(), Errno> {
        let mut va = Vattr::new();
        vattr_null(&mut va);
        va.va_size = size;
        let _ = vn_lock(vp, LK_EXCLUSIVE | LK_RETRY);
        let error = VOP_SETATTR(vp, &mut va, p.ucred(), p);
        let _ = VOP_UNLOCK(vp);
        error
    }

    #[test]
    fn a_file_is_created_found_resized_and_removed() {
        let (_g, p) = setup_root();
        let vp = create(p, b"/f", 0o640);

        let va = stat(p, b"/f").expect("stat");
        assert_eq!(va.va_type, VREG);
        assert_eq!(va.va_mode, 0o640);
        assert_eq!((va.va_nlink, va.va_size, va.va_bytes), (1, 0, 0));
        assert_eq!(va.va_fileid, VP_TO_TMPFS_NODE(vp).tn_id.get());
        assert_eq!(va.va_blocksize, PAGE_SIZE as i64);
        assert_eq!(va.va_rdev, VNOVAL);
        assert_eq!(
            stat(p, b"/").expect("stat /").va_size,
            size_of::<TmpfsDirent>() as u64,
            "one entry"
        );

        // grow and shrink by whole pages (a partial page is zeroed through a mapping)
        truncate(p, vp, 3 * PAGE_SIZE as u64).expect("grow");
        let va = stat(p, b"/f").expect("stat");
        assert_eq!(va.va_size, 3 * PAGE_SIZE as u64);
        assert_eq!(va.va_bytes, 3 * PAGE_SIZE as u64);
        truncate(p, vp, PAGE_SIZE as u64).expect("shrink");
        assert_eq!(stat(p, b"/f").expect("stat").va_size, PAGE_SIZE as u64);

        // unsettable attributes
        let mut va = Vattr::new();
        vattr_null(&mut va);
        va.va_nlink = 3;
        let _ = vn_lock(vp, LK_EXCLUSIVE | LK_RETRY);
        assert_eq!(VOP_SETATTR(vp, &mut va, p.ucred(), p), Err(Errno::EINVAL));
        let _ = VOP_UNLOCK(vp);

        // a second open finds the same vnode; O_CREAT on an existing file opens it
        let again = create(p, b"/f", 0o600);
        assert!(ptr::eq(again, vp));
        close(p, again);

        assert_eq!(names(p, b"/"), [&b"."[..], b"..", b"f"]);
        dounlinkat(p, AT_FDCWD, c("/f").as_ptr(), 0).expect("unlink");
        assert_eq!(stat(p, b"/f").err(), Some(Errno::ENOENT));
        assert_eq!(names(p, b"/"), [&b"."[..], b".."]);
        let node = VP_TO_TMPFS_NODE(vp);
        assert_eq!(node.tn_links.get(), 0, "open but nameless");
        let _ = vn_lock(vp, LK_EXCLUSIVE | LK_RETRY);
        assert_eq!(
            VOP_OPEN(vp, FREAD, p.ucred(), p),
            Err(Errno::ENOENT),
            "a file without names cannot be opened again"
        );
        let _ = VOP_UNLOCK(vp);
        close(p, vp);
    }

    #[test]
    fn reads_and_writes_check_their_arguments() {
        let (_g, p) = setup_root();
        let vp = create(p, b"/f", 0o644);
        let mut byte = [0u8; 1];
        let mut resid = 0;

        // an empty file reads nothing; a write of nothing writes nothing
        vn_rdwr(
            UioRw::UIO_READ,
            vp,
            byte.as_mut_ptr().cast(),
            1,
            0,
            UioSeg::UIO_SYSSPACE,
            0,
            p.ucred(),
            Some(&mut resid),
            Some(p),
        )
        .expect("read");
        assert_eq!(resid, 1);
        vn_rdwr(
            UioRw::UIO_WRITE,
            vp,
            byte.as_mut_ptr().cast(),
            0,
            0,
            UioSeg::UIO_SYSSPACE,
            0,
            p.ucred(),
            None,
            Some(p),
        )
        .expect("empty write");
        // past the largest offset
        assert_eq!(
            vn_rdwr(
                UioRw::UIO_WRITE,
                vp,
                byte.as_mut_ptr().cast(),
                1,
                i64::MAX,
                UioSeg::UIO_SYSSPACE,
                0,
                p.ucred(),
                None,
                Some(p),
            ),
            Err(Errno::EFBIG)
        );
        assert_eq!(VP_TO_TMPFS_NODE(vp).tn_size.get(), 0);
        close(p, vp);

        // a directory is neither read nor written as a file
        let root = tmpfs_root_vnode(p);
        for (rw, want) in [
            (UioRw::UIO_READ, Errno::EISDIR),
            (UioRw::UIO_WRITE, Errno::EINVAL),
        ] {
            assert_eq!(
                vn_rdwr(
                    rw,
                    root,
                    byte.as_mut_ptr().cast(),
                    1,
                    0,
                    UioSeg::UIO_SYSSPACE,
                    0,
                    p.ucred(),
                    None,
                    Some(p),
                ),
                Err(want)
            );
        }
        vrele(root);
    }

    /// The root vnode, referenced and unlocked.
    fn tmpfs_root_vnode(p: &'static Proc) -> &'static Vnode {
        let mut nd = ndinit(LOOKUP, 0, NiDirp::Sys(b"/"), p);
        namei(&mut nd).expect("lookup /");
        nd.ni_vp.expect("a vnode")
    }

    #[test]
    fn directories_are_made_listed_and_removed() {
        let (_g, p) = setup_root();
        assert_eq!(stat(p, b"/").expect("stat /").va_nlink, 2);

        domkdirat(p, AT_FDCWD, c("/d").as_ptr(), 0o755).expect("mkdir /d");
        domkdirat(p, AT_FDCWD, c("/d/e").as_ptr(), 0o700).expect("mkdir /d/e");
        assert_eq!(
            domkdirat(p, AT_FDCWD, c("/d").as_ptr(), 0o755),
            Err(Errno::EEXIST)
        );
        let d = stat(p, b"/d").expect("stat /d");
        assert_eq!((d.va_type, d.va_mode, d.va_nlink), (VDIR, 0o755, 3));
        assert_eq!(stat(p, b"/").expect("stat /").va_nlink, 3);
        assert_eq!(names(p, b"/d"), [&b"."[..], b"..", b"e"]);
        assert_eq!(
            stat(p, b"/d/e/..").expect("..").va_fileid,
            d.va_fileid,
            "`..` is the parent"
        );

        assert_eq!(
            dounlinkat(p, AT_FDCWD, c("/d").as_ptr(), AT_REMOVEDIR),
            Err(Errno::ENOTEMPTY)
        );
        assert_eq!(
            dounlinkat(p, AT_FDCWD, c("/d").as_ptr(), 0),
            Err(Errno::EPERM),
            "unlink(2) of a directory"
        );
        dounlinkat(p, AT_FDCWD, c("/d/e").as_ptr(), AT_REMOVEDIR).expect("rmdir /d/e");
        assert_eq!(stat(p, b"/d").expect("stat /d").va_nlink, 2);
        dounlinkat(p, AT_FDCWD, c("/d").as_ptr(), AT_REMOVEDIR).expect("rmdir /d");
        assert_eq!(stat(p, b"/d").err(), Some(Errno::ENOENT));
        assert_eq!(stat(p, b"/").expect("stat /").va_nlink, 2);
        assert_eq!(names(p, b"/"), [&b"."[..], b".."]);
    }

    #[test]
    fn renames_move_entries_within_and_across_directories() {
        let (_g, p) = setup_root();
        let vp = create(p, b"/a", 0o644);
        close(p, vp);
        let id = stat(p, b"/a").expect("stat /a").va_fileid;
        domkdirat(p, AT_FDCWD, c("/d").as_ptr(), 0o755).expect("mkdir /d");
        domkdirat(p, AT_FDCWD, c("/d2").as_ptr(), 0o755).expect("mkdir /d2");

        // within a directory: a new name, a longer one
        dorenameat(p, AT_FDCWD, c("/a").as_ptr(), AT_FDCWD, c("/bee").as_ptr()).expect("rename");
        assert_eq!(stat(p, b"/a").err(), Some(Errno::ENOENT));
        assert_eq!(stat(p, b"/bee").expect("stat /bee").va_fileid, id);

        // across directories
        dorenameat(
            p,
            AT_FDCWD,
            c("/bee").as_ptr(),
            AT_FDCWD,
            c("/d/c").as_ptr(),
        )
        .expect("rename");
        assert_eq!(stat(p, b"/d/c").expect("stat /d/c").va_fileid, id);
        assert_eq!(names(p, b"/"), [&b"."[..], b"..", b"d", b"d2"]);
        assert_eq!(names(p, b"/d"), [&b"."[..], b"..", b"c"]);

        // over an existing file, which goes
        let vp = create(p, b"/d/x", 0o644);
        close(p, vp);
        dorenameat(
            p,
            AT_FDCWD,
            c("/d/c").as_ptr(),
            AT_FDCWD,
            c("/d/x").as_ptr(),
        )
        .expect("rename");
        assert_eq!(stat(p, b"/d/x").expect("stat /d/x").va_fileid, id);
        assert_eq!(names(p, b"/d"), [&b"."[..], b"..", b"x"]);

        // a directory into another one: the link counts and `..` follow it
        dorenameat(p, AT_FDCWD, c("/d").as_ptr(), AT_FDCWD, c("/d2/d").as_ptr()).expect("rename");
        let d2 = stat(p, b"/d2").expect("stat /d2");
        assert_eq!(d2.va_nlink, 3);
        assert_eq!(stat(p, b"/").expect("stat /").va_nlink, 3);
        assert_eq!(stat(p, b"/d2/d/..").expect("..").va_fileid, d2.va_fileid);
        assert_eq!(stat(p, b"/d2/d/x").expect("stat").va_fileid, id);

        // a directory over a non-empty one, a file over a directory, a directory into itself
        domkdirat(p, AT_FDCWD, c("/e").as_ptr(), 0o755).expect("mkdir /e");
        assert_eq!(
            dorenameat(p, AT_FDCWD, c("/e").as_ptr(), AT_FDCWD, c("/d2").as_ptr()),
            Err(Errno::ENOTEMPTY)
        );
        assert_eq!(
            dorenameat(
                p,
                AT_FDCWD,
                c("/d2/d/x").as_ptr(),
                AT_FDCWD,
                c("/e").as_ptr()
            ),
            Err(Errno::EISDIR)
        );
        assert_eq!(
            dorenameat(
                p,
                AT_FDCWD,
                c("/d2").as_ptr(),
                AT_FDCWD,
                c("/d2/d/z").as_ptr()
            ),
            Err(Errno::EINVAL)
        );
        // an empty directory over an empty one
        domkdirat(p, AT_FDCWD, c("/f").as_ptr(), 0o755).expect("mkdir /f");
        dorenameat(p, AT_FDCWD, c("/e").as_ptr(), AT_FDCWD, c("/f").as_ptr()).expect("rename");
        assert_eq!(names(p, b"/"), [&b"."[..], b"..", b"d2", b"f"]);
        assert_eq!(stat(p, b"/").expect("stat /").va_nlink, 4);
    }

    #[test]
    fn hard_links_count_names() {
        let (_g, p) = setup_root();
        let vp = create(p, b"/f", 0o644);
        close(p, vp);
        domkdirat(p, AT_FDCWD, c("/d").as_ptr(), 0o755).expect("mkdir /d");

        dolinkat(
            p,
            AT_FDCWD,
            c("/f").as_ptr(),
            AT_FDCWD,
            c("/d/g").as_ptr(),
            0,
        )
        .expect("link");
        let f = stat(p, b"/f").expect("stat /f");
        assert_eq!(f.va_nlink, 2);
        assert_eq!(stat(p, b"/d/g").expect("stat /d/g").va_fileid, f.va_fileid);
        assert_eq!(
            dolinkat(
                p,
                AT_FDCWD,
                c("/f").as_ptr(),
                AT_FDCWD,
                c("/d/g").as_ptr(),
                0
            ),
            Err(Errno::EEXIST)
        );
        assert_eq!(
            dolinkat(p, AT_FDCWD, c("/d").as_ptr(), AT_FDCWD, c("/h").as_ptr(), 0),
            Err(Errno::EPERM),
            "no links to directories"
        );

        dounlinkat(p, AT_FDCWD, c("/f").as_ptr(), 0).expect("unlink /f");
        assert_eq!(stat(p, b"/d/g").expect("stat /d/g").va_nlink, 1);
        dounlinkat(p, AT_FDCWD, c("/d/g").as_ptr(), 0).expect("unlink /d/g");
        assert_eq!(names(p, b"/d"), [&b"."[..], b".."]);
    }

    #[test]
    fn symbolic_links_read_back_and_are_followed() {
        let (_g, p) = setup_root();
        domkdirat(p, AT_FDCWD, c("/d").as_ptr(), 0o755).expect("mkdir /d");
        dosymlinkat(p, c("d").as_ptr(), AT_FDCWD, c("/l").as_ptr()).expect("symlink");

        let l = stat(p, b"/l").expect("lstat /l");
        assert_eq!((l.va_type, l.va_size), (VLNK, 1));
        let mut buf = [0u8; 16];
        let mut retval = [0; 2];
        doreadlinkat(
            p,
            AT_FDCWD,
            c("/l").as_ptr(),
            buf.as_mut_ptr(),
            buf.len(),
            &mut retval,
        )
        .expect("readlink");
        assert_eq!(&buf[..retval[0] as usize], b"d");

        // followed in the middle of a path
        let d = stat(p, b"/d").expect("stat /d").va_fileid;
        assert_eq!(stat(p, b"/l/.").expect("stat /l/.").va_fileid, d);

        // a long target
        let target = [b'x'; 200];
        let mut t = target.to_vec();
        t.push(0);
        dosymlinkat(p, t.as_ptr(), AT_FDCWD, c("/long").as_ptr()).expect("symlink");
        let mut buf = [0u8; 300];
        doreadlinkat(
            p,
            AT_FDCWD,
            c("/long").as_ptr(),
            buf.as_mut_ptr(),
            buf.len(),
            &mut retval,
        )
        .expect("readlink");
        assert_eq!(&buf[..retval[0] as usize], &target[..]);
        dounlinkat(p, AT_FDCWD, c("/long").as_ptr(), 0).expect("unlink");
    }

    #[test]
    fn the_vnode_lock_recurses_as_ufs_does_and_vnd_reads_under_it() {
        let (_g, p) = setup_root();
        let mut nd = ndinit(0, 0, NiDirp::Sys(b"/new.img"), p);
        vn_open(&mut nd, FREAD | FWRITE | O_CREAT, 0o600).expect("open");
        let vp = nd.ni_vp.expect("a vnode");
        let lock = &VP_TO_TMPFS_NODE(vp).tn_vlock;

        // vn_open returns the vnode locked by this thread
        assert_eq!(VOP_ISLOCKED(vp), LK_EXCLUSIVE);
        assert_eq!(lock.rrwl_wcnt.get(), 1);

        // the same thread takes it again: rrw_enter counts, as ufs_lock does
        vn_lock(vp, LK_EXCLUSIVE | LK_RETRY).expect("recursive lock");
        assert_eq!(lock.rrwl_wcnt.get(), 2);
        assert_eq!(
            vn_lock(vp, LK_EXCLUSIVE | LK_RECURSEFAIL),
            Err(Errno::EDEADLK)
        );
        let _ = VOP_UNLOCK(vp);
        assert_eq!(lock.rrwl_wcnt.get(), 1);
        assert_eq!(VOP_ISLOCKED(vp), LK_EXCLUSIVE, "still held once");

        // vnd(4)'s VNDIOCSET: vndsetcred reads the file through vn_rdwr (which locks it
        // again) while vn_open's lock is held, and the lock is back to one level afterwards
        let mut buf = [0u8; DEV_BSIZE];
        let mut resid = 0;
        vn_rdwr(
            UioRw::UIO_READ,
            vp,
            buf.as_mut_ptr().cast(),
            DEV_BSIZE,
            0,
            UioSeg::UIO_SYSSPACE,
            0,
            p.ucred(),
            Some(&mut resid),
            Some(p),
        )
        .expect("read under the open lock");
        assert_eq!(resid, DEV_BSIZE, "an empty file reads nothing");
        assert_eq!(lock.rrwl_wcnt.get(), 1);
        assert_eq!(VOP_ISLOCKED(vp), LK_EXCLUSIVE);

        let _ = VOP_UNLOCK(vp);
        assert_eq!(VOP_ISLOCKED(vp), 0);
        close(p, vp);
    }

    #[test]
    fn pathconf_and_kqueue_filters() {
        let (_g, p) = setup_root();
        let vp = create(p, b"/f", 0o644);
        let mut v: Register = 0;
        VOP_PATHCONF(vp, _PC_NAME_MAX, &mut v).expect("pathconf");
        assert_eq!(v, TMPFS_MAXNAMLEN as Register);
        VOP_PATHCONF(vp, _PC_LINK_MAX, &mut v).expect("pathconf");
        assert_eq!(v, LINK_MAX as Register);
        VOP_PATHCONF(vp, _PC_FILESIZEBITS, &mut v).expect("pathconf");
        assert_eq!(v, 64);
        assert_eq!(VOP_PATHCONF(vp, 9999, &mut v), Err(Errno::EINVAL));

        // a vnode filter hooks onto the vnode's list and comes off it
        let kn: &'static Knote = Box::leak(Box::new(Knote::new()));
        kn.kn_filter().set(EVFILT_VNODE);
        VOP_KQFILTER(vp, 0, kn).expect("kqfilter");
        assert!(
            kn.kn_fop
                .get()
                .is_some_and(|f| ptr::eq(f, &TMPFSVNODE_FILTOPS))
        );
        assert!(ptr::eq(kn_vnode(kn), vp));
        assert!(vp.v_klist.kl_list.first().is_some());
        filt_tmpfsdetach(kn);
        assert!(vp.v_klist.kl_list.first().is_none());
        let bad: &'static Knote = Box::leak(Box::new(Knote::new()));
        bad.kn_filter().set(-100);
        assert_eq!(VOP_KQFILTER(vp, 0, bad), Err(Errno::EINVAL));

        // the events: only the subscribed notes are recorded; revocation ends it
        let kn = Knote::new();
        kn.kn_sfflags.set(NOTE_WRITE | NOTE_RENAME);
        assert!(!filt_tmpfsvnode(&kn, i64::from(NOTE_LINK)));
        assert!(filt_tmpfsvnode(&kn, i64::from(NOTE_RENAME)));
        assert_eq!(kn.kn_fflags().get(), NOTE_RENAME);
        assert!(filt_tmpfsvnode(&kn, i64::from(NOTE_REVOKE)) && kn.has_flags(EV_EOF));
        let kn = Knote::new();
        assert!(filt_tmpfswrite(&kn, 0) && kn.kn_data().get() == 0);
        assert!(filt_tmpfswrite(&kn, i64::from(NOTE_REVOKE)));
        assert!(kn.has_flags(EV_EOF) && kn.has_flags(EV_ONESHOT));
        close(p, vp);
    }
}
/* </TESTS> */
