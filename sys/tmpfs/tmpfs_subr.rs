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
 * Copyright (c) 2005-2011 The NetBSD Foundation, Inc.
 * Copyright (c) 2013 Pedro Martelletto
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Julio M. Merino Vidal, developed as part of Google's Summer of Code
 * 2005 program, and by Mindaugas Rasiukevicius.
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
//! Efficient memory file system: interfaces for inode and directory entry construction,
//! destruction and manipulation, the attribute changes `tmpfs_setattr` makes, and the data
//! path of regular files through their anonymous UVM object.
//!
//! Upstream: sys/tmpfs/tmpfs_subr.c @ 3ce1f3f79392
//!
//! # Reference counting
//!
//! The link count of inode (`tn_links`) is used as a reference counter. However, it has
//! slightly different semantics.
//!
//! For directories - link count represents directory entries, which refer to the
//! directories. In other words, it represents the count of sub-directories. It also takes
//! into account the virtual '.' entry (which has no real entry in the list). For files - link
//! count represents the hard links. Since only empty directories can be removed - link count
//! aligns the reference counting requirements enough. Note: to check whether directory is
//! not empty, the inode size (`tn_size`) can be used.
//!
//! The inode itself, as an object, gathers its first reference when directory entry is
//! attached via `tmpfs_dir_attach`. For instance, after regular `tmpfs_create()`, a file
//! would have a link count of 1, while directory after `tmpfs_mkdir()` would have 2 (due to
//! '.').
//!
//! # Reclamation
//!
//! It should be noted that tmpfs inodes rely on a combination of vnode reference counting
//! and link counting. That is, an inode can only be destroyed if its associated vnode is
//! inactive. The destruction is done on vnode reclamation i.e. `tmpfs_reclaim()`. It should
//! be noted that `tn_links` being 0 is a destruction criterion.
//!
//! If an inode has references within the file system (`tn_links > 0`) and its inactive vnode
//! gets reclaimed/recycled - then the association is broken in `tmpfs_reclaim()`. In such
//! case, an inode will always pass `tmpfs_lookup()` and thus `tmpfs_vnode_get()` to
//! associate a new vnode.
//!
//! # Lock order
//!
//! `tn_nlock` -> `v_vlock` (the node's `tn_vlock`) -> `v_interlock`.
//!
//! ## Deviations
//! - Functions that fill a `**` out-parameter return the object (`Result<&'static T,
//!   Errno>`); `tmpfs_dir_lookup`, `tmpfs_dir_cached` and `tmpfs_dir_lookupbyseq` return
//!   `Option` for the C's NULL; `tmpfs_uio_lookup` returns `None` for the C's NULL address
//!   and `tmpfs_uio_cached` a `bool`.
//! - `tmpfs_alloc_node`/`tmpfs_alloc_file` take the symbolic link's target as an optional
//!   byte slice (ending at its first NUL or at the slice's end), not a `char *`.
//! - The `tmpfs_ch*` functions take the credential as `&Ucred`; the vnode operations turn
//!   their `a_cred` into one. Their `struct proc *` is kept for the signature (only
//!   `tmpfs_chtimes` uses it).
//! - `option FIFO` is in GENERIC but `miscfs/fifofs` is not ported: `tmpfs_vnode_get`
//!   refuses a fifo node with `EOPNOTSUPP` before taking a vnode, as `ffs_vinit` does
//!   (`tmpfs_fifovops` waits in `tmpfs_fifoops.rs`). `mknod(2)`/`mkfifo(2)` answer
//!   `EOPNOTSUPP` for a fifo before reaching the file system meanwhile.
//! - `KASSERT`s on pointers the C then dereferences (`dvp` in `tmpfs_dir_attach`, the
//!   entry's node, the parent of `..`) are panics: Rust cannot follow a NULL.
//! - `pool_put(&namei_pool, cnp->cn_pnbuf)` in `tmpfs_alloc_file` gives the buffer back
//!   through `vfs_init.rs`'s `NAMEI_POOL`.

use core::ffi::c_void;
use core::ptr::{self, NonNull};

use crate::dev::rnd::arc4random;
use crate::kassert;
use crate::kern::kern_prot::{groupmember, suser_ucred};
use crate::kern::kern_rwlock::{
    rrw_init_flags, rw_enter, rw_enter_write, rw_exit, rw_exit_write, rw_init,
};
use crate::kern::kern_subr::uiomove;
use crate::kern::kern_sysctl::SECURELEVEL;
use crate::kern::kern_tc::{getnanotime, nanotime};
use crate::kern::spec_vnops::SPEC_VOPS;
use crate::kern::subr_pool::pool_put;
use crate::kern::subr_prf::panic;
use crate::kern::vfs_init::NAMEI_POOL;
use crate::kern::vfs_subr::{checkalias, getnewvnode, vget, vgone, vrele};
use crate::kern::vfs_vnops::vn_lock;
use crate::kern::vfs_vops::{VOP_ACCESS, VOP_ISLOCKED};
use crate::sys::dirent::{
    DT_BLK, DT_CHR, DT_DIR, DT_FIFO, DT_LNK, DT_REG, DT_SOCK, Dirent, dirent_size,
};
use crate::sys::errno::Errno;
use crate::sys::event::{NOTE_ATTRIB, NOTE_DELETE, NOTE_EXTEND, NOTE_LINK, NOTE_WRITE};
use crate::sys::lock::{LK_EXCLUSIVE, LK_RETRY};
use crate::sys::mman::{MADV_NORMAL, MADV_SEQUENTIAL, MAP_INHERIT_NONE, PROT_READ, PROT_WRITE};
use crate::sys::mount::{MNT_RDONLY, Mount};
use crate::sys::namei::{Componentname, SAVESTART};
use crate::sys::param::{MAXPATHLEN, PAGE_MASK, PAGE_SHIFT, PAGE_SIZE};
use crate::sys::proc::Proc;
use crate::sys::queue::{ListHead, TailqHead};
use crate::sys::rwlock::{RW_WRITE, RWL_DUPOK, RWL_IS_VNODE};
use crate::sys::stat::{
    ALLPERMS, APPEND, IMMUTABLE, S_ISGID, S_ISTXT, SF_APPEND, SF_IMMUTABLE, SF_SETTABLE,
    UF_SETTABLE,
};
use crate::sys::syslimits::LINK_MAX;
use crate::sys::time::Timespec;
use crate::sys::types::{Dev, Gid, Mode, Off, Uid, Vsize};
use crate::sys::ucred::Ucred;
use crate::sys::uio::Uio;
use crate::sys::vnode::{
    VA_UTIMES_CHANGE, VA_UTIMES_NULL, VBAD, VBLK, VCHR, VDIR, VFIFO, VLNK, VN_KNOTE, VNON, VNOVAL,
    VREG, VROOT, VSOCK, VT_TMPFS, VTEXT, VWRITE, Vattr, Vnode, Vtype,
};
use crate::tmpfs::tmpfs::{
    TMPFS_DIRSEQ_DOT, TMPFS_DIRSEQ_DOTDOT, TMPFS_DIRSEQ_END, TMPFS_DIRSEQ_EOF, TMPFS_DIRSEQ_NONE,
    TMPFS_DIRSEQ_START, TMPFS_NODE_ACCESSED, TMPFS_NODE_CHANGED, TMPFS_NODE_GEN_MASK,
    TMPFS_NODE_MODIFIED, TMPFS_NODE_STATUSALL, TMPFS_RECLAIMING_BIT, TmpfsDir, TmpfsDirent,
    TmpfsMount, TmpfsNode, TmpfsNodeList, VFS_TO_TMPFS, VP_TO_TMPFS_DIR, VP_TO_TMPFS_NODE,
    tmpfs_dirseq_full, tmpfs_node_reclaiming, tmpfs_validate_dir,
};
use crate::tmpfs::tmpfs_mem::{
    tmpfs_dirent_get, tmpfs_dirent_put, tmpfs_mem_decr, tmpfs_mem_incr, tmpfs_node_get,
    tmpfs_node_put, tmpfs_strname_alloc, tmpfs_strname_free,
};
use crate::tmpfs::tmpfs_specops::TMPFS_SPECVOPS;
use crate::tmpfs::tmpfs_vnops::TMPFS_VOPS;
use crate::uvm::uvm_aobj::{
    UAO_FLAG_CANFAIL, uao_create, uao_detach, uao_grow, uao_reference, uao_shrink,
};
use crate::uvm::uvm_extern::{Voff, uvm_mapflag};
use crate::uvm::uvm_km::kernel_map;
use crate::uvm::uvm_map::{uvm_map, uvm_unmap};
use crate::uvm::uvm_param::{round_page, trunc_page};
use crate::uvm::uvm_vnode::{uvm_vnp_setsize, uvm_vnp_uncache};

/// `TMPFS_UIO_MAXBYTES`: be gentle to kernel_map, don't allow more than 4MB in a single
/// transaction.
const TMPFS_UIO_MAXBYTES: usize = (1 << 22) - PAGE_SIZE;

/// The node a directory entry refers to (`de->td_node`, which the C dereferences).
fn td_node(de: &TmpfsDirent) -> &'static TmpfsNode {
    match de.td_node.get() {
        Some(node) => node,
        None => panic(format_args!(
            "tmpfs: directory entry {:p} without a node",
            de
        )),
    }
}

/// Whether the node's vnode, if it has one, is locked (`VOP_ISLOCKED(node->tn_vnode)`).
fn vnode_islocked(node: &TmpfsNode) -> bool {
    node.tn_vnode.get().is_some_and(|vp| VOP_ISLOCKED(vp) != 0)
}

/// `tmpfs_alloc_node`: allocate a new inode of a specified type and insert it into the list
/// of specified mount point.
#[allow(clippy::too_many_arguments)] // the C's signature
pub fn tmpfs_alloc_node(
    tmp: &TmpfsMount,
    type_: Vtype,
    uid: Uid,
    gid: Gid,
    mode: Mode,
    target: Option<&[u8]>,
    rdev: Dev,
) -> Result<&'static TmpfsNode, Errno> {
    let Some(nnode) = tmpfs_node_get(tmp) else {
        return Err(Errno::ENOSPC);
    };

    // Initially, no references and no associations.
    nnode.tn_links.set(0);
    nnode.tn_vnode.set(None);
    nnode.tn_dirent_hint.set(None);

    rw_enter_write(&tmp.tm_acc_lock);
    tmp.tm_highest_inode
        .set(tmp.tm_highest_inode.get().wrapping_add(1));
    nnode.tn_id.set(tmp.tm_highest_inode.get());
    if nnode.tn_id.get() == 0 {
        tmp.tm_highest_inode
            .set(tmp.tm_highest_inode.get().wrapping_sub(1));
        rw_exit_write(&tmp.tm_acc_lock);
        tmpfs_node_put(tmp, nnode);
        return Err(Errno::ENOSPC);
    }
    rw_exit_write(&tmp.tm_acc_lock);

    // Generic initialization.
    nnode.tn_type.set(type_);
    nnode.tn_size.set(0);
    nnode.tn_flags.set(0);
    nnode.tn_lockf.set(None);
    nnode
        .tn_gen
        .set(TMPFS_NODE_GEN_MASK & u64::from(arc4random()));

    let now = nanotime();
    nnode.tn_atime.set(now);
    nnode.tn_birthtime.set(now);
    nnode.tn_ctime.set(now);
    nnode.tn_mtime.set(now);

    kassert!(uid != VNOVAL as Uid && gid != VNOVAL as Gid && mode != VNOVAL as Mode);

    nnode.tn_uid.set(uid);
    nnode.tn_gid.set(gid);
    nnode.tn_mode.set(mode);

    // Type-specific initialization.
    match type_ {
        VBLK | VCHR => {
            // Character/block special device.
            kassert!(rdev != VNOVAL);
            nnode.tn_spec.tn_dev.tn_rdev.set(rdev);
        }
        VDIR => {
            // Directory.
            nnode.tn_spec.tn_dir.tn_dir.init();
            nnode.tn_spec.tn_dir.tn_parent.set(None);
            nnode.tn_spec.tn_dir.tn_next_seq.set(TMPFS_DIRSEQ_START);
            nnode.tn_spec.tn_dir.tn_readdir_lastp.set(None);

            // Extra link count for the virtual '.' entry.
            nnode.tn_links.set(nnode.tn_links.get() + 1);
        }
        VFIFO | VSOCK => {}
        VLNK => {
            // Symbolic link. Target specifies the file name.
            kassert!(target.is_some());
            let target = target.unwrap_or(&[]);
            let len = target.iter().position(|&c| c == 0).unwrap_or(target.len());
            kassert!(len < MAXPATHLEN);

            nnode.tn_size.set(len as Off);
            if len == 0 {
                nnode.tn_spec.tn_lnk.tn_link.set(ptr::null_mut());
            } else {
                let Some(link) = tmpfs_strname_alloc(tmp, len) else {
                    tmpfs_node_put(tmp, nnode);
                    return Err(Errno::ENOSPC);
                };
                // SAFETY: `link` is a fresh buffer of at least `len` bytes (rounded up to
                // `TMPFS_NAME_QUANTUM`) and `target` holds `len` bytes; they do not overlap.
                unsafe { ptr::copy_nonoverlapping(target.as_ptr(), link.as_ptr(), len) };
                nnode.tn_spec.tn_lnk.tn_link.set(link.as_ptr());
            }
        }
        VREG => {
            // Regular file. Create an underlying UVM object.
            let Some(uobj) = uao_create(Vsize::new(0), UAO_FLAG_CANFAIL) else {
                tmpfs_node_put(tmp, nnode);
                return Err(Errno::ENOSPC);
            };
            nnode.tn_spec.tn_reg.tn_aobj.set(Some(uobj));
            nnode.tn_spec.tn_reg.tn_aobj_pages.set(0);
            nnode.tn_spec.tn_reg.tn_aobj_pgptr.set(0);
            nnode.tn_spec.tn_reg.tn_aobj_pgnum.set(-1);
        }
        VNON | VBAD => {
            kassert!(false);
        }
    }

    rw_init(&nnode.tn_nlock, "tvlk");

    rw_enter_write(&tmp.tm_lock);
    // SAFETY: a fresh node, on no list; it stays in place until `tmpfs_free_node` takes it
    // off, under `tm_lock` as here.
    unsafe { tmp.tm_nodes.insert_head(nnode) };
    rw_exit_write(&tmp.tm_lock);

    Ok(nnode)
}

/// `tmpfs_free_node`: remove the inode from a list in the mount point and destroy the inode
/// structures.
pub fn tmpfs_free_node(tmp: &TmpfsMount, node: &'static TmpfsNode) {
    rw_enter_write(&tmp.tm_lock);
    // SAFETY: the node is on the mount's list (`tmpfs_alloc_node`), under `tm_lock`.
    unsafe { ListHead::<TmpfsNodeList>::remove(node) };
    rw_exit_write(&tmp.tm_lock);

    match node.tn_type.get() {
        VLNK => {
            if node.tn_size.get() > 0 {
                kassert!(node.tn_size.get() as u64 <= usize::MAX as u64);
                if let Some(link) = NonNull::new(node.tn_spec.tn_lnk.tn_link.get()) {
                    tmpfs_strname_free(tmp, link, node.tn_size.get() as usize);
                }
            }
        }
        VREG => {
            // Calculate the size of inode data, decrease the used-memory counter, and
            // destroy the underlying UVM object (if any).
            let objsz = PAGE_SIZE * node.tn_spec.tn_reg.tn_aobj_pages.get();
            if objsz != 0 {
                tmpfs_mem_decr(tmp, objsz);
            }
            if let Some(uobj) = node.tn_spec.tn_reg.tn_aobj.take() {
                uao_detach(uobj);
            }
        }
        VDIR => {
            kassert!(node.tn_spec.tn_dir.tn_dir.is_empty());
            kassert!(
                node.tn_spec.tn_dir.tn_parent.get().is_none()
                    || tmp.tm_root.get().is_some_and(|root| ptr::eq(node, root))
            );
        }
        _ => {}
    }

    rw_enter_write(&tmp.tm_acc_lock);
    if node.tn_id.get() == tmp.tm_highest_inode.get() {
        tmp.tm_highest_inode.set(tmp.tm_highest_inode.get() - 1);
    }
    rw_exit_write(&tmp.tm_acc_lock);

    // mutex_destroy(&node->tn_nlock): nothing to do for an rwlock.
    tmpfs_node_put(tmp, node);
}

/// `tmpfs_vnode_get`: allocate or reclaim a vnode for a specified inode.
///
/// Must be called with `tn_nlock` held, which it releases. Returns the vnode locked.
pub fn tmpfs_vnode_get(
    mp: &'static Mount,
    node: &'static TmpfsNode,
) -> Result<&'static Vnode, Errno> {
    // again:
    // If there is already a vnode, try to reclaim it.
    while let Some(vp) = node.tn_vnode.get() {
        // atomic_or_ulong(&node->tn_gen, TMPFS_RECLAIMING_BIT);
        node.tn_gen.set(node.tn_gen.get() | TMPFS_RECLAIMING_BIT);
        rw_exit_write(&node.tn_nlock);
        let error = vget(vp, LK_EXCLUSIVE);
        if error == Err(Errno::ENOENT) {
            rw_enter_write(&node.tn_nlock);
            continue; // goto again
        }
        // atomic_and_ulong(&node->tn_gen, ~TMPFS_RECLAIMING_BIT);
        node.tn_gen.set(node.tn_gen.get() & !TMPFS_RECLAIMING_BIT);
        return error.map(|()| vp);
    }
    if tmpfs_node_reclaiming(node) {
        // atomic_and_ulong(&node->tn_gen, ~TMPFS_RECLAIMING_BIT);
        node.tn_gen.set(node.tn_gen.get() & !TMPFS_RECLAIMING_BIT);
    }

    // FIFO: vp->v_op = &tmpfs_fifovops below (miscfs/fifofs, not ported); refused here, as
    // ffs_vinit refuses one, before a vnode is taken.
    if node.tn_type.get() == VFIFO {
        rw_exit_write(&node.tn_nlock);
        return Err(Errno::EOPNOTSUPP);
    }

    // Get a new vnode and associate it with our inode. Share the lock with underlying UVM
    // object, if there is one (VREG case): `#if 0` in the C.
    let mut vp = match getnewvnode(VT_TMPFS, Some(mp), &TMPFS_VOPS) {
        Ok(vp) => vp,
        Err(e) => {
            rw_exit_write(&node.tn_nlock);
            return Err(e);
        }
    };

    rrw_init_flags(&node.tn_vlock, "tnode", RWL_DUPOK | RWL_IS_VNODE);
    vp.v_type.set(node.tn_type.get());

    // Type-specific initialization.
    match node.tn_type.get() {
        VBLK | VCHR => {
            vp.v_op.set(Some(&TMPFS_SPECVOPS));
            if let Some(nvp) = checkalias(vp, node.tn_spec.tn_dev.tn_rdev.get(), Some(mp)) {
                nvp.v_data.set(vp.v_data.get());
                vp.v_data.set(ptr::null_mut());
                vp.v_op.set(Some(&SPEC_VOPS));
                vrele(vp);
                vgone(vp);
                vp = nvp;
                node.tn_vnode.set(Some(vp));
            }
        }
        VDIR => {
            if node
                .tn_spec
                .tn_dir
                .tn_parent
                .get()
                .is_some_and(|p| ptr::eq(p, node))
            {
                vp.v_flag.set(vp.v_flag.get() | VROOT);
            }
        }
        VLNK | VREG | VSOCK | VFIFO => {}
        VNON | VBAD => {
            kassert!(false);
        }
    }

    uvm_vnp_setsize(vp, node.tn_size.get());
    vp.v_data
        .set(ptr::from_ref(node).cast_mut().cast::<c_void>());
    node.tn_vnode.set(Some(vp));
    let _ = vn_lock(vp, LK_EXCLUSIVE | LK_RETRY);
    rw_exit_write(&node.tn_nlock);

    kassert!(VOP_ISLOCKED(vp) != 0);
    Ok(vp)
}

/// `tmpfs_alloc_file`: allocate a new file of specified type and adds it into the parent
/// directory. Credentials of the caller are used. Returns the new file's vnode, locked.
pub fn tmpfs_alloc_file(
    dvp: &'static Vnode,
    vap: &Vattr,
    cnp: &mut Componentname,
    target: Option<&[u8]>,
) -> Result<&'static Vnode, Errno> {
    let Some(mp) = dvp.v_mount.get() else {
        panic(format_args!(
            "tmpfs_alloc_file: vnode {:p} has no mount",
            dvp
        ));
    };
    let tmp = VFS_TO_TMPFS(mp);
    let dnode = VP_TO_TMPFS_DIR(dvp);

    kassert!(VOP_ISLOCKED(dvp) != 0);

    let error = 'out: {
        // Check for the maximum number of links limit.
        if vap.va_type == VDIR {
            // Check for maximum links limit.
            if dnode.tn_links.get() == LINK_MAX {
                break 'out Err(Errno::EMLINK);
            }
            kassert!(dnode.tn_links.get() < LINK_MAX);
        }

        if tmpfs_dirseq_full(dnode) {
            break 'out Err(Errno::ENOSPC);
        }

        if dnode.tn_links.get() == 0 {
            break 'out Err(Errno::ENOENT);
        }

        // Allocate a node that represents the new file.
        let node = match tmpfs_alloc_node(
            tmp,
            vap.va_type,
            cnp.cred().cr_uid.get(),
            dnode.tn_gid.get(),
            vap.va_mode,
            target,
            vap.va_rdev,
        ) {
            Ok(node) => node,
            Err(e) => break 'out Err(e),
        };

        // Allocate a directory entry that points to the new file.
        let de = match tmpfs_alloc_dirent(tmp, cnp.name()) {
            Ok(de) => de,
            Err(e) => {
                tmpfs_free_node(tmp, node);
                break 'out Err(e);
            }
        };

        // Get a vnode for the new file.
        rw_enter_write(&node.tn_nlock);
        let vp = match tmpfs_vnode_get(mp, node) {
            Ok(vp) => vp,
            Err(e) => {
                tmpfs_free_dirent(tmp, de);
                tmpfs_free_node(tmp, node);
                break 'out Err(e);
            }
        };

        // Associate inode and attach the entry into the directory.
        tmpfs_dir_attach(dnode, de, node);
        Ok(vp)
    };

    // out:
    if error.is_ok()
        && cnp.cn_flags & SAVESTART == 0
        && let Some(buf) = NonNull::new(cnp.cn_pnbuf)
    {
        pool_put(&NAMEI_POOL, buf);
    }
    error
}

/// `tmpfs_alloc_dirent`: allocates a new directory entry for the inode. The directory entry
/// contains a path name component, `name`.
pub fn tmpfs_alloc_dirent(tmp: &TmpfsMount, name: &[u8]) -> Result<&'static TmpfsDirent, Errno> {
    let len = name.len() as u16;

    let Some(nde) = tmpfs_dirent_get(tmp) else {
        return Err(Errno::ENOSPC);
    };

    let Some(buf) = tmpfs_strname_alloc(tmp, usize::from(len)) else {
        tmpfs_dirent_put(tmp, nde);
        return Err(Errno::ENOSPC);
    };
    // SAFETY: `buf` is a fresh buffer of at least `len` bytes and `name` holds them; they
    // do not overlap.
    unsafe { ptr::copy_nonoverlapping(name.as_ptr(), buf.as_ptr(), usize::from(len)) };
    nde.td_name.set(buf.as_ptr());
    nde.td_namelen.set(len);
    nde.td_seq.set(TMPFS_DIRSEQ_NONE);

    Ok(nde)
}

/// `tmpfs_free_dirent`: free a directory entry.
pub fn tmpfs_free_dirent(tmp: &TmpfsMount, de: &'static TmpfsDirent) {
    kassert!(de.td_node.get().is_none());
    kassert!(de.td_seq.get() == TMPFS_DIRSEQ_NONE);
    if let Some(name) = NonNull::new(de.td_name.get()) {
        tmpfs_strname_free(tmp, name, usize::from(de.td_namelen.get()));
    }
    tmpfs_dirent_put(tmp, de);
}

/// `tmpfs_dir_attach`: associate directory entry with a specified inode, and attach the
/// entry into the directory, specified by vnode.
///
/// Increases link count on the associated node. Increases link count on directory node, if
/// our node is VDIR. It is caller's responsibility to check for the `LINK_MAX` limit.
/// Triggers kqueue events here.
pub fn tmpfs_dir_attach(
    dnode: &'static TmpfsNode,
    de: &'static TmpfsDirent,
    node: &'static TmpfsNode,
) {
    let Some(dvp) = dnode.tn_vnode.get() else {
        panic(format_args!(
            "tmpfs_dir_attach: directory {:p} has no vnode",
            dnode
        ));
    };
    let mut events = NOTE_WRITE;

    kassert!(VOP_ISLOCKED(dvp) != 0);

    // Get a new sequence number.
    kassert!(de.td_seq.get() == TMPFS_DIRSEQ_NONE);
    de.td_seq.set(tmpfs_dir_getseq(dnode, de));

    // Associate directory entry and the inode.
    de.td_node.set(Some(node));
    kassert!(node.tn_links.get() < LINK_MAX);
    node.tn_links.set(node.tn_links.get() + 1);

    // Save the hint (might overwrite).
    node.tn_dirent_hint.set(Some(de));

    // Insert the entry to the directory (parent of inode).
    // SAFETY: a fresh entry, on no list (its sequence number was NONE); it stays in place
    // until `tmpfs_dir_detach` takes it off, under the directory's vnode lock as here.
    unsafe { dnode.tn_spec.tn_dir.tn_dir.insert_tail(de) };
    dnode
        .tn_size
        .set(dnode.tn_size.get() + size_of::<TmpfsDirent>() as Off);
    tmpfs_update(dnode, TMPFS_NODE_STATUSALL);
    uvm_vnp_setsize(dvp, dnode.tn_size.get());

    if node.tn_type.get() == VDIR {
        // Set parent.
        kassert!(node.tn_spec.tn_dir.tn_parent.get().is_none());
        node.tn_spec.tn_dir.tn_parent.set(Some(dnode));

        // Increase the link count of parent.
        kassert!(dnode.tn_links.get() < LINK_MAX);
        dnode.tn_links.set(dnode.tn_links.get() + 1);
        events |= NOTE_LINK;

        tmpfs_validate_dir(node);
    }
    VN_KNOTE(dvp, events);
}

/// `tmpfs_dir_detach`: disassociate directory entry and its inode, and detach the entry
/// from the directory, specified by vnode.
///
/// Decreases link count on the associated node. Decreases the link count on directory node,
/// if our node is VDIR. Triggers kqueue events here.
pub fn tmpfs_dir_detach(dnode: &'static TmpfsNode, de: &'static TmpfsDirent) {
    let node = td_node(de);
    let dvp = dnode.tn_vnode.get();
    let mut events = NOTE_WRITE;

    kassert!(dvp.is_none_or(|dvp| VOP_ISLOCKED(dvp) != 0));

    // Deassociate the inode and entry.
    de.td_node.set(None);
    node.tn_dirent_hint.set(None);

    kassert!(node.tn_links.get() > 0);
    node.tn_links.set(node.tn_links.get() - 1);
    if let Some(vp) = node.tn_vnode.get() {
        kassert!(VOP_ISLOCKED(vp) != 0);
        VN_KNOTE(
            vp,
            if node.tn_links.get() != 0 {
                NOTE_LINK
            } else {
                NOTE_DELETE
            },
        );
    }

    // If directory - decrease the link count of parent.
    if node.tn_type.get() == VDIR {
        kassert!(
            node.tn_spec
                .tn_dir
                .tn_parent
                .get()
                .is_some_and(|p| ptr::eq(p, dnode))
        );
        node.tn_spec.tn_dir.tn_parent.set(None);

        kassert!(dnode.tn_links.get() > 0);
        dnode.tn_links.set(dnode.tn_links.get() - 1);
        events |= NOTE_LINK;
    }

    // Remove the entry from the directory.
    if dnode
        .tn_spec
        .tn_dir
        .tn_readdir_lastp
        .get()
        .is_some_and(|lastp| ptr::eq(lastp, de))
    {
        dnode.tn_spec.tn_dir.tn_readdir_lastp.set(None);
    }
    // SAFETY: the entry is on this directory's list (`tmpfs_dir_attach`), under the
    // directory's vnode lock.
    unsafe { dnode.tn_spec.tn_dir.tn_dir.remove(de) };

    dnode
        .tn_size
        .set(dnode.tn_size.get() - size_of::<TmpfsDirent>() as Off);
    tmpfs_update(dnode, TMPFS_NODE_MODIFIED | TMPFS_NODE_CHANGED);
    tmpfs_dir_putseq(dnode, de);
    if let Some(dvp) = dvp {
        tmpfs_update(dnode, 0);
        uvm_vnp_setsize(dvp, dnode.tn_size.get());
        VN_KNOTE(dvp, events);
    }
}

/// `tmpfs_dir_lookup`: find a directory entry in the specified inode.
///
/// Note that the `.` and `..` components are not allowed as they do not physically exist
/// within directories.
pub fn tmpfs_dir_lookup(
    node: &'static TmpfsNode,
    cnp: &Componentname,
) -> Option<&'static TmpfsDirent> {
    let name = cnp.name();
    let nlen = name.len();

    kassert!(vnode_islocked(node));
    kassert!(nlen != 1 || name[0] != b'.');
    kassert!(nlen != 2 || !(name[0] == b'.' && name[1] == b'.'));
    tmpfs_validate_dir(node);

    let de = node
        .tn_spec
        .tn_dir
        .tn_dir
        .iter()
        .find(|de| usize::from(de.td_namelen.get()) == nlen && de.name() == name);
    tmpfs_update(node, TMPFS_NODE_ACCESSED);
    de
}

/// `tmpfs_dir_cached`: get a cached directory entry if it is valid. Used to avoid
/// unnecessary `tmpfs_dir_lookup()`. The vnode must be locked.
pub fn tmpfs_dir_cached(node: &'static TmpfsNode) -> Option<&'static TmpfsDirent> {
    let de = node.tn_dirent_hint.get();

    kassert!(vnode_islocked(node));

    let de = de?;
    kassert!(de.td_node.get().is_some_and(|n| ptr::eq(n, node)));

    // Directories always have a valid hint. For files, check if there are any hard links.
    // If there are - hint might be invalid.
    if node.tn_type.get() != VDIR && node.tn_links.get() > 1 {
        None
    } else {
        Some(de)
    }
}

/// `tmpfs_dir_getseq`: get a per-directory sequence number for the entry.
pub fn tmpfs_dir_getseq(dnode: &TmpfsNode, de: &TmpfsDirent) -> u64 {
    let seq = de.td_seq.get();

    tmpfs_validate_dir(dnode);

    if seq != TMPFS_DIRSEQ_NONE {
        // Already set.
        kassert!(seq >= TMPFS_DIRSEQ_START);
        return seq;
    }

    // The "." and ".." and the end-of-directory have reserved numbers. The other sequence
    // numbers are allocated incrementally.

    let seq = dnode.tn_spec.tn_dir.tn_next_seq.get();
    kassert!(seq >= TMPFS_DIRSEQ_START);
    kassert!(seq < TMPFS_DIRSEQ_END);
    dnode.tn_spec.tn_dir.tn_next_seq.set(seq + 1);
    seq
}

/// `tmpfs_dir_putseq`: give back the entry's sequence number; the directory's counter
/// steps back when it was the last one given, and restarts when the directory is empty.
pub fn tmpfs_dir_putseq(dnode: &TmpfsNode, de: &TmpfsDirent) {
    let seq = de.td_seq.get();

    tmpfs_validate_dir(dnode);
    kassert!(seq == TMPFS_DIRSEQ_NONE || seq >= TMPFS_DIRSEQ_START);
    kassert!(seq == TMPFS_DIRSEQ_NONE || seq < TMPFS_DIRSEQ_END);

    de.td_seq.set(TMPFS_DIRSEQ_NONE);

    // Empty?  We can reset.
    let next_seq = &dnode.tn_spec.tn_dir.tn_next_seq;
    if dnode.tn_size.get() == 0 {
        next_seq.set(TMPFS_DIRSEQ_START);
    } else if seq != TMPFS_DIRSEQ_NONE && seq == next_seq.get() - 1 {
        next_seq.set(next_seq.get() - 1);
    }
}

/// `tmpfs_dir_lookupbyseq`: lookup a directory entry by the sequence number.
pub fn tmpfs_dir_lookupbyseq(node: &'static TmpfsNode, seq: Off) -> Option<&'static TmpfsDirent> {
    let seq = seq as u64;
    let de = node.tn_spec.tn_dir.tn_readdir_lastp.get();

    tmpfs_validate_dir(node);

    // First, check the cache. If does not match - perform a lookup.
    if let Some(de) = de
        && de.td_seq.get() == seq
    {
        kassert!(de.td_seq.get() >= TMPFS_DIRSEQ_START);
        kassert!(de.td_seq.get() != TMPFS_DIRSEQ_NONE);
        return Some(de);
    }
    node.tn_spec.tn_dir.tn_dir.iter().find(|de| {
        kassert!(de.td_seq.get() >= TMPFS_DIRSEQ_START);
        kassert!(de.td_seq.get() != TMPFS_DIRSEQ_NONE);
        de.td_seq.get() == seq
    })
}

/// The first `d_reclen` bytes of a `struct dirent`, as `uiomove(dp, dp->d_reclen, uio)`
/// copies them out.
fn dirent_bytes(dp: &Dirent) -> ([u8; size_of::<Dirent>()], usize) {
    let mut b = [0u8; size_of::<Dirent>()];
    let reclen = usize::from(dp.d_reclen).min(b.len());
    b[0..8].copy_from_slice(&dp.d_fileno.to_ne_bytes());
    b[8..16].copy_from_slice(&dp.d_off.to_ne_bytes());
    b[16..18].copy_from_slice(&dp.d_reclen.to_ne_bytes());
    b[18] = dp.d_type;
    b[19] = dp.d_namlen;
    let namlen = usize::from(dp.d_namlen);
    b[Dirent::NAME_OFFSET..Dirent::NAME_OFFSET + namlen].copy_from_slice(&dp.d_name[..namlen]);
    (b, reclen)
}

/// Sets `dp`'s name (NUL-terminated) and `d_namlen`.
fn dirent_setname(dp: &mut Dirent, name: &[u8]) {
    dp.d_name[..name.len()].copy_from_slice(name);
    dp.d_name[name.len()] = 0;
    dp.d_namlen = name.len() as u8;
}

/// `tmpfs_dir_getdotents`: helper function for `tmpfs_readdir()` to get the dot meta
/// entries, that is, "." or "..". Copy it to the UIO space.
///
/// `Err(EJUSTRETURN)` when the entry does not fit in what is left of `uio`.
pub fn tmpfs_dir_getdotents(
    node: &'static TmpfsNode,
    dp: &mut Dirent,
    uio: &mut Uio<'_>,
) -> Result<(), Errno> {
    let next = match uio.uio_offset as u64 {
        TMPFS_DIRSEQ_DOT => {
            dp.d_fileno = node.tn_id.get();
            dirent_setname(dp, b".");
            TMPFS_DIRSEQ_DOTDOT
        }
        TMPFS_DIRSEQ_DOTDOT => {
            let Some(parent) = node.tn_spec.tn_dir.tn_parent.get() else {
                panic(format_args!(
                    "tmpfs_dir_getdotents: directory {:p} has no parent",
                    node
                ));
            };
            dp.d_fileno = parent.tn_id.get();
            dirent_setname(dp, b"..");
            match node.tn_spec.tn_dir.tn_dir.first() {
                Some(de) => tmpfs_dir_getseq(node, de),
                None => TMPFS_DIRSEQ_EOF,
            }
        }
        off => panic(format_args!("tmpfs_dir_getdotents: offset {}", off)),
    };
    dp.d_type = DT_DIR;
    dp.d_reclen = dirent_size(dp) as u16;
    dp.d_off = next as Off;

    if usize::from(dp.d_reclen) > uio.uio_resid {
        return Err(Errno::EJUSTRETURN);
    }

    let (mut bytes, reclen) = dirent_bytes(dp);
    uiomove(&mut bytes[..reclen], uio)?;

    uio.uio_offset = next as Off;
    Ok(())
}

/// `tmpfs_dir_getdents`: helper function for `tmpfs_readdir`.
///
/// Returns as much directory entries as can fit in the uio space. The read starts at
/// `uio.uio_offset`.
pub fn tmpfs_dir_getdents(node: &'static TmpfsNode, uio: &mut Uio<'_>) -> Result<(), Errno> {
    kassert!(vnode_islocked(node));
    tmpfs_validate_dir(node);
    let mut dent = Dirent {
        d_fileno: 0,
        d_off: 0,
        d_reclen: 0,
        d_type: 0,
        d_namlen: 0,
        __d_padding: [0; 4],
        d_name: [0; crate::sys::dirent::MAXNAMLEN + 1],
    };

    let error = 'done: {
        if uio.uio_offset as u64 == TMPFS_DIRSEQ_DOT
            && let Err(e) = tmpfs_dir_getdotents(node, &mut dent, uio)
        {
            break 'done Err(e);
        }
        if uio.uio_offset as u64 == TMPFS_DIRSEQ_DOTDOT
            && let Err(e) = tmpfs_dir_getdotents(node, &mut dent, uio)
        {
            break 'done Err(e);
        }
        // Done if we reached the end.
        if uio.uio_offset as u64 == TMPFS_DIRSEQ_EOF {
            break 'done Ok(());
        }

        // Locate the directory entry given by the given sequence number.
        let Some(mut de) = tmpfs_dir_lookupbyseq(node, uio.uio_offset) else {
            break 'done Err(Errno::EINVAL);
        };

        // Read as many entries as possible; i.e., until we reach the end of the directory
        // or we exhaust UIO space.
        let mut error = Ok(());
        let cur = loop {
            let dnode = td_node(de);
            dent.d_fileno = dnode.tn_id.get();
            dent.d_type = match dnode.tn_type.get() {
                VBLK => DT_BLK,
                VCHR => DT_CHR,
                VDIR => DT_DIR,
                VFIFO => DT_FIFO,
                VLNK => DT_LNK,
                VREG => DT_REG,
                VSOCK => DT_SOCK,
                VNON | VBAD => {
                    kassert!(false);
                    dent.d_type
                }
            };
            kassert!(usize::from(de.td_namelen.get()) < dent.d_name.len());
            dirent_setname(&mut dent, de.name());
            dent.d_reclen = dirent_size(&dent) as u16;

            if de.name().contains(&b'/') {
                error = Err(Errno::EINVAL);
                break Some(de);
            }

            let next_de = TailqHead::<TmpfsDir>::next(de);
            dent.d_off = match next_de {
                None => TMPFS_DIRSEQ_EOF as Off,
                Some(next) => tmpfs_dir_getseq(node, next) as Off,
            };

            if usize::from(dent.d_reclen) > uio.uio_resid {
                // Exhausted UIO space.
                error = Err(Errno::EJUSTRETURN);
                break Some(de);
            }

            // Copy out the directory entry and continue.
            let (mut bytes, reclen) = dirent_bytes(&dent);
            if let Err(e) = uiomove(&mut bytes[..reclen], uio) {
                error = Err(e);
                break Some(de);
            }
            match TailqHead::<TmpfsDir>::next(de) {
                Some(next) if uio.uio_resid > 0 => de = next,
                next => break next,
            }
        };

        // Cache the last entry or clear and mark EOF.
        uio.uio_offset = match cur {
            Some(de) => tmpfs_dir_getseq(node, de) as Off,
            None => TMPFS_DIRSEQ_EOF as Off,
        };
        node.tn_spec.tn_dir.tn_readdir_lastp.set(cur);
        error
    };

    // done:
    tmpfs_update(node, TMPFS_NODE_ACCESSED);

    match error {
        // Exhausted UIO space - just return.
        Err(Errno::EJUSTRETURN) => Ok(()),
        error => error,
    }
}

/// `tmpfs_reg_resize`: resize the underlying UVM object associated with the specified
/// regular file.
pub fn tmpfs_reg_resize(vp: &'static Vnode, newsize: Off) -> Result<(), Errno> {
    let Some(mp) = vp.v_mount.get() else {
        panic(format_args!(
            "tmpfs_reg_resize: vnode {:p} has no mount",
            vp
        ));
    };
    let tmp = VFS_TO_TMPFS(mp);
    let node = VP_TO_TMPFS_NODE(vp);
    let uobj = node.tn_uobj();

    kassert!(vp.v_type.get() == VREG);
    kassert!(newsize >= 0);

    let oldsize = node.tn_size.get();
    let oldpages = round_page(oldsize as usize) >> PAGE_SHIFT;
    let newpages = round_page(newsize as usize) >> PAGE_SHIFT;
    kassert!(oldpages == node.tn_spec.tn_reg.tn_aobj_pages.get());

    if newpages > oldpages {
        // Increase the used-memory counter if getting extra pages.
        let bytes = (newpages - oldpages) << PAGE_SHIFT;
        if !tmpfs_mem_incr(tmp, bytes) {
            return Err(Errno::ENOSPC);
        }
        let _ = rw_enter(uobj.vmobjlock(), RW_WRITE);
        let error = uao_grow(uobj, newpages as i32);
        rw_exit(uobj.vmobjlock());
        if error.is_err() {
            tmpfs_mem_decr(tmp, bytes);
            return Err(Errno::ENOSPC);
        }
    }

    node.tn_spec.tn_reg.tn_aobj_pages.set(newpages);
    node.tn_size.set(newsize);
    uvm_vnp_setsize(vp, newsize);
    let _ = uvm_vnp_uncache(vp);

    // Free "backing store".
    if newpages < oldpages {
        if tmpfs_uio_cached(node) {
            tmpfs_uio_uncache(node);
        }
        let _ = rw_enter(uobj.vmobjlock(), RW_WRITE);
        if uao_shrink(uobj, newpages as i32).is_err() {
            panic(format_args!("shrink failed"));
        }
        rw_exit(uobj.vmobjlock());
        // Decrease the used-memory counter.
        tmpfs_mem_decr(tmp, (oldpages - newpages) << PAGE_SHIFT);
    }
    if newsize > oldsize {
        if tmpfs_uio_cached(node) {
            tmpfs_uio_uncache(node);
        }
        let pgoff = oldsize as usize & PAGE_MASK;
        if pgoff != 0 {
            // Growing from an offset which is not at a page boundary; zero out unused bytes
            // in current page.
            if let Err(error) = tmpfs_zeropg(node, trunc_page(oldsize as usize) as Voff, pgoff) {
                panic(format_args!("tmpfs_zeropg: error {}", error as i32));
            }
        }
        VN_KNOTE(vp, NOTE_EXTEND);
    }
    Ok(())
}

/// `tmpfs_chflags`: change flags of the given vnode.
pub fn tmpfs_chflags(vp: &'static Vnode, flags: u32, cred: &Ucred, _p: &Proc) -> Result<(), Errno> {
    let node = VP_TO_TMPFS_NODE(vp);

    kassert!(VOP_ISLOCKED(vp) != 0);

    // Disallow this operation if the file system is mounted read-only.
    if vp
        .v_mount
        .get()
        .is_some_and(|mp| mp.mnt_flag.get() & MNT_RDONLY != 0)
    {
        return Err(Errno::EROFS);
    }

    if cred.cr_uid.get() != node.tn_uid.get() {
        suser_ucred(cred)?;
    }

    if cred.cr_uid.get() == 0 {
        if node.tn_flags.get() & (SF_IMMUTABLE | SF_APPEND) != 0
            && SECURELEVEL.load(core::sync::atomic::Ordering::Relaxed) > 0
        {
            return Err(Errno::EPERM);
        }
        node.tn_flags.set(flags);
    } else {
        if node.tn_flags.get() & (SF_IMMUTABLE | SF_APPEND) != 0 || flags & UF_SETTABLE != flags {
            return Err(Errno::EPERM);
        }
        node.tn_flags.set(node.tn_flags.get() & SF_SETTABLE);
        node.tn_flags
            .set(node.tn_flags.get() | (flags & UF_SETTABLE));
    }

    tmpfs_update(node, TMPFS_NODE_CHANGED);
    VN_KNOTE(vp, NOTE_ATTRIB);
    Ok(())
}

/// `tmpfs_chmod`: change access mode on the given vnode.
pub fn tmpfs_chmod(vp: &'static Vnode, mode: Mode, cred: &Ucred, _p: &Proc) -> Result<(), Errno> {
    let node = VP_TO_TMPFS_NODE(vp);

    kassert!(VOP_ISLOCKED(vp) != 0);

    // Disallow this operation if the file system is mounted read-only.
    if vp
        .v_mount
        .get()
        .is_some_and(|mp| mp.mnt_flag.get() & MNT_RDONLY != 0)
    {
        return Err(Errno::EROFS);
    }

    // Immutable or append-only files cannot be modified, either.
    if node.tn_flags.get() & (IMMUTABLE | APPEND) != 0 {
        return Err(Errno::EPERM);
    }

    if cred.cr_uid.get() != node.tn_uid.get() {
        suser_ucred(cred)?;
    }
    if cred.cr_uid.get() != 0 {
        if vp.v_type.get() != VDIR && mode & S_ISTXT != 0 {
            return Err(Errno::EFTYPE);
        }
        if !groupmember(node.tn_gid.get(), cred) && mode & S_ISGID != 0 {
            return Err(Errno::EPERM);
        }
    }

    node.tn_mode.set(mode & ALLPERMS);
    tmpfs_update(node, TMPFS_NODE_CHANGED);
    if vp.v_flag.get() & VTEXT != 0 && node.tn_mode.get() & S_ISTXT == 0 {
        let _ = uvm_vnp_uncache(vp);
    }
    VN_KNOTE(vp, NOTE_ATTRIB);
    Ok(())
}

/// `tmpfs_chown`: change ownership of the given vnode.
///
/// At least one of uid or gid must be different than `VNOVAL`. Attribute is unchanged for
/// `VNOVAL` case.
pub fn tmpfs_chown(
    vp: &'static Vnode,
    uid: Uid,
    gid: Gid,
    cred: &Ucred,
    _p: &Proc,
) -> Result<(), Errno> {
    let node = VP_TO_TMPFS_NODE(vp);

    kassert!(VOP_ISLOCKED(vp) != 0);

    // Assign default values if they are unknown.
    kassert!(uid != VNOVAL as Uid || gid != VNOVAL as Gid);
    let uid = if uid == VNOVAL as Uid {
        node.tn_uid.get()
    } else {
        uid
    };
    let gid = if gid == VNOVAL as Gid {
        node.tn_gid.get()
    } else {
        gid
    };

    // Disallow this operation if the file system is mounted read-only.
    if vp
        .v_mount
        .get()
        .is_some_and(|mp| mp.mnt_flag.get() & MNT_RDONLY != 0)
    {
        return Err(Errno::EROFS);
    }

    // Immutable or append-only files cannot be modified, either.
    if node.tn_flags.get() & (IMMUTABLE | APPEND) != 0 {
        return Err(Errno::EPERM);
    }

    if cred.cr_uid.get() != node.tn_uid.get()
        || uid != node.tn_uid.get()
        || (gid != node.tn_gid.get() && !groupmember(gid, cred))
    {
        suser_ucred(cred)?;
    }

    node.tn_uid.set(uid);
    node.tn_gid.set(gid);
    tmpfs_update(node, TMPFS_NODE_CHANGED);
    VN_KNOTE(vp, NOTE_ATTRIB);
    Ok(())
}

/// `tmpfs_chsize`: change size of the given vnode.
pub fn tmpfs_chsize(vp: &'static Vnode, size: u64, _cred: &Ucred, _p: &Proc) -> Result<(), Errno> {
    let node = VP_TO_TMPFS_NODE(vp);

    kassert!(VOP_ISLOCKED(vp) != 0);

    // Decide whether this is a valid operation based on the file type.
    match vp.v_type.get() {
        VDIR => return Err(Errno::EISDIR),
        VREG => {
            if vp
                .v_mount
                .get()
                .is_some_and(|mp| mp.mnt_flag.get() & MNT_RDONLY != 0)
            {
                return Err(Errno::EROFS);
            }
        }
        VBLK | VCHR | VFIFO => {
            // Allow modifications of special files even if in the file system is mounted
            // read-only (we are not modifying the files themselves, but the objects they
            // represent).
            return Ok(());
        }
        _ => return Err(Errno::EOPNOTSUPP),
    }

    // Immutable or append-only files cannot be modified, either.
    if node.tn_flags.get() & (IMMUTABLE | APPEND) != 0 {
        return Err(Errno::EPERM);
    }

    // Note: tmpfs_truncate() will raise NOTE_EXTEND and NOTE_ATTRIB.
    tmpfs_truncate(vp, size as Off)
}

/// `tmpfs_chtimes`: change access and modification times for vnode.
pub fn tmpfs_chtimes(
    vp: &'static Vnode,
    atime: &Timespec,
    mtime: &Timespec,
    vaflags: u32,
    cred: &Ucred,
    p: &Proc,
) -> Result<(), Errno> {
    let node = VP_TO_TMPFS_NODE(vp);

    kassert!(VOP_ISLOCKED(vp) != 0);

    // Disallow this operation if the file system is mounted read-only.
    if vp
        .v_mount
        .get()
        .is_some_and(|mp| mp.mnt_flag.get() & MNT_RDONLY != 0)
    {
        return Err(Errno::EROFS);
    }

    // Immutable or append-only files cannot be modified, either.
    if node.tn_flags.get() & (IMMUTABLE | APPEND) != 0 {
        return Err(Errno::EPERM);
    }

    if cred.cr_uid.get() != node.tn_uid.get()
        && let Err(error) = suser_ucred(cred)
    {
        if vaflags & VA_UTIMES_NULL == 0 {
            return Err(error);
        }
        VOP_ACCESS(vp, VWRITE, ptr::from_ref(cred), p)?;
    }

    if atime.tv_nsec != i64::from(VNOVAL) {
        node.tn_atime.set(*atime);
    }

    if mtime.tv_nsec != i64::from(VNOVAL) {
        node.tn_mtime.set(*mtime);
    }

    if mtime.tv_nsec != i64::from(VNOVAL) || vaflags & VA_UTIMES_CHANGE != 0 {
        tmpfs_update(VP_TO_TMPFS_NODE(vp), TMPFS_NODE_CHANGED);
    }

    VN_KNOTE(vp, NOTE_ATTRIB);

    Ok(())
}

/// `tmpfs_update`: update timestamps, et al.
pub fn tmpfs_update(node: &TmpfsNode, flags: i32) {
    let nowtm = getnanotime();

    if flags & TMPFS_NODE_ACCESSED != 0 {
        node.tn_atime.set(nowtm);
    }
    if flags & TMPFS_NODE_MODIFIED != 0 {
        node.tn_mtime.set(nowtm);
    }
    if flags & TMPFS_NODE_CHANGED != 0 {
        node.tn_ctime.set(nowtm);
    }
}

/// `tmpfs_truncate`: set a regular file's size.
pub fn tmpfs_truncate(vp: &'static Vnode, length: Off) -> Result<(), Errno> {
    let node = VP_TO_TMPFS_NODE(vp);

    if length < 0 {
        return Err(Errno::EINVAL);
    }
    if node.tn_size.get() == length {
        return Ok(());
    }
    let error = tmpfs_reg_resize(vp, length);
    if error.is_ok() {
        tmpfs_update(node, TMPFS_NODE_CHANGED | TMPFS_NODE_MODIFIED);
    }
    error
}

/// `tmpfs_uio_cached`: whether the node keeps one page of its object mapped.
pub fn tmpfs_uio_cached(node: &TmpfsNode) -> bool {
    let pgnum_valid = node.tn_pgnum().get() != -1;
    let pgptr_valid = node.tn_pgptr().get() != 0;
    kassert!(pgnum_valid == pgptr_valid);
    pgnum_valid && pgptr_valid
}

/// `tmpfs_uio_lookup`: the kernel address of the cached mapping of the page at `pgnum`, if
/// that is the page cached.
pub fn tmpfs_uio_lookup(node: &TmpfsNode, pgnum: Voff) -> Option<usize> {
    if tmpfs_uio_cached(node) && node.tn_pgnum().get() == pgnum {
        return Some(node.tn_pgptr().get());
    }

    None
}

/// `tmpfs_uio_uncache`: drop the cached mapping.
pub fn tmpfs_uio_uncache(node: &TmpfsNode) {
    kassert!(node.tn_pgnum().get() != -1);
    kassert!(node.tn_pgptr().get() != 0);
    uvm_unmap(
        kernel_map(),
        node.tn_pgptr().get(),
        node.tn_pgptr().get() + PAGE_SIZE,
    );
    node.tn_pgnum().set(-1);
    node.tn_pgptr().set(0);
}

/// `tmpfs_uio_cache`: remember the mapping of the page at `pgnum`.
pub fn tmpfs_uio_cache(node: &TmpfsNode, pgnum: Voff, pgptr: usize) {
    kassert!(node.tn_pgnum().get() == -1);
    kassert!(node.tn_pgptr().get() == 0);
    node.tn_pgnum().set(pgnum);
    node.tn_pgptr().set(pgptr);
}

/// The `len` bytes at the kernel address `va`, for `uiomove`.
///
/// # Safety
///
/// `[va, va + len)` lies in a live, readable and writable kernel mapping of the node's
/// object (pages fault in on first touch), which nothing else accesses while the slice
/// lives.
unsafe fn mapped_bytes<'a>(va: usize, len: usize) -> &'a mut [u8] {
    // SAFETY: the caller's contract.
    unsafe { core::slice::from_raw_parts_mut(va as *mut u8, len) }
}

/// `tmpfs_uiomove`: move up to `len` bytes between the file's object and `uio`, at
/// `uio.uio_offset`, through a temporary kernel mapping of the object (one page of it is
/// kept mapped for the next small transfer).
pub fn tmpfs_uiomove(node: &TmpfsNode, uio: &mut Uio<'_>, len: usize) -> Result<(), Errno> {
    let pgnum = trunc_page(uio.uio_offset as usize) as Voff;
    let pgoff = uio.uio_offset as usize & PAGE_MASK;

    if pgoff + len < PAGE_SIZE
        && let Some(va) = tmpfs_uio_lookup(node, pgnum)
    {
        // SAFETY: the cached mapping covers the page at `pgnum` and `pgoff + len` stays in
        // it; the node's vnode lock (held by the caller) serialises its users.
        return uiomove(unsafe { mapped_bytes(va + pgoff, len) }, uio);
    }

    let (sz, adv) = if len >= TMPFS_UIO_MAXBYTES {
        (TMPFS_UIO_MAXBYTES, MADV_NORMAL)
    } else {
        (len, MADV_SEQUENTIAL)
    };

    if tmpfs_uio_cached(node) {
        tmpfs_uio_uncache(node);
    }

    let uobj = node.tn_uobj();
    uao_reference(uobj);

    let mut va = 0usize;
    if let Err(error) = uvm_map(
        kernel_map(),
        &mut va,
        round_page(pgoff + sz),
        Some(uobj),
        trunc_page(uio.uio_offset as usize) as Voff,
        0,
        uvm_mapflag(
            PROT_READ | PROT_WRITE,
            PROT_READ | PROT_WRITE,
            MAP_INHERIT_NONE,
            adv,
            0,
        ),
    ) {
        uao_detach(uobj); // Drop reference.
        return Err(error);
    }

    // SAFETY: `[va, va + round_page(pgoff + sz))` is the fresh kernel mapping of the object
    // made above, readable and writable; `pgoff + sz` stays inside it.
    let error = uiomove(unsafe { mapped_bytes(va + pgoff, sz) }, uio);
    if error.is_ok() && pgoff + sz < PAGE_SIZE {
        tmpfs_uio_cache(node, pgnum, va);
    } else {
        uvm_unmap(kernel_map(), va, va + round_page(pgoff + sz));
    }

    error
}

/// `tmpfs_zeropg`: zero the bytes of the page at `pgnum` from `pgoff` to its end.
pub fn tmpfs_zeropg(node: &TmpfsNode, pgnum: Voff, pgoff: usize) -> Result<(), Errno> {
    kassert!(!tmpfs_uio_cached(node));

    let uobj = node.tn_uobj();
    uao_reference(uobj);

    let mut va = 0usize;
    if let Err(error) = uvm_map(
        kernel_map(),
        &mut va,
        PAGE_SIZE,
        Some(uobj),
        pgnum,
        0,
        uvm_mapflag(
            PROT_READ | PROT_WRITE,
            PROT_READ | PROT_WRITE,
            MAP_INHERIT_NONE,
            MADV_NORMAL,
            0,
        ),
    ) {
        uao_detach(uobj); // Drop reference.
        return Err(error);
    }

    // SAFETY: `[va, va + PAGE_SIZE)` is the fresh kernel mapping of the object's page made
    // above, writable; `pgoff` is below `PAGE_SIZE`.
    unsafe { mapped_bytes(va + pgoff, PAGE_SIZE - pgoff) }.fill(0);
    uvm_unmap(kernel_map(), va, va + PAGE_SIZE);

    Ok(())
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for the tmpfs directory sequence numbers on a bare directory node (no vnode,
    // no pools: the entries are leaked boxes linked by hand, as `tmpfs_dir_attach` links them).
    // The node, entry and vnode paths run through a real mount in `tmpfs_vfsops.rs`.

    use std::boxed::Box;
    use std::{assert, assert_eq, assert_ne};

    use super::*;

    /// A fresh directory node, as `tmpfs_alloc_node` leaves one.
    fn dir() -> &'static TmpfsNode {
        let node: &'static TmpfsNode = Box::leak(Box::new(TmpfsNode::new()));
        node.tn_type.set(VDIR);
        node.tn_spec.tn_dir.tn_next_seq.set(TMPFS_DIRSEQ_START);
        node
    }

    /// Links a new entry at the end of `dnode`, numbered as `tmpfs_dir_attach` numbers it.
    fn link(dnode: &'static TmpfsNode) -> &'static TmpfsDirent {
        let de: &'static TmpfsDirent = Box::leak(Box::new(TmpfsDirent::new()));
        de.td_seq.set(TMPFS_DIRSEQ_NONE);
        de.td_seq.set(tmpfs_dir_getseq(dnode, de));
        // SAFETY: a fresh, leaked entry on no list.
        unsafe { dnode.tn_spec.tn_dir.tn_dir.insert_tail(de) };
        dnode
            .tn_size
            .set(dnode.tn_size.get() + size_of::<TmpfsDirent>() as Off);
        de
    }

    /// Unlinks an entry as `tmpfs_dir_detach` does.
    fn unlink(dnode: &'static TmpfsNode, de: &'static TmpfsDirent) {
        // SAFETY: the entry is on the directory's list.
        unsafe { dnode.tn_spec.tn_dir.tn_dir.remove(de) };
        dnode
            .tn_size
            .set(dnode.tn_size.get() - size_of::<TmpfsDirent>() as Off);
        tmpfs_dir_putseq(dnode, de);
    }

    #[test]
    fn sequence_numbers_start_after_the_reserved_ones_and_step_back() {
        let d = dir();
        let a = link(d);
        let b = link(d);
        let c = link(d);
        assert_eq!([a.td_seq.get(), b.td_seq.get(), c.td_seq.get()], [3, 4, 5]);
        assert_eq!(tmpfs_dir_getseq(d, b), 4, "a number once given stays");

        // removing the last one given steps the counter back; a hole in the middle does not
        unlink(d, c);
        assert_eq!(c.td_seq.get(), TMPFS_DIRSEQ_NONE);
        assert_eq!(d.tn_spec.tn_dir.tn_next_seq.get(), 5);
        unlink(d, a);
        assert_eq!(d.tn_spec.tn_dir.tn_next_seq.get(), 5);
        assert_eq!(link(d).td_seq.get(), 5);

        // lookups by number: the readdir cache first, then the list
        assert!(tmpfs_dir_lookupbyseq(d, 4).is_some_and(|de| ptr::eq(de, b)));
        d.tn_spec.tn_dir.tn_readdir_lastp.set(Some(b));
        assert!(tmpfs_dir_lookupbyseq(d, 4).is_some_and(|de| ptr::eq(de, b)));
        assert!(tmpfs_dir_lookupbyseq(d, 3).is_none());
        assert!(!tmpfs_dirseq_full(d));
    }

    #[test]
    fn an_emptied_directory_restarts_its_numbers() {
        let d = dir();
        let a = link(d);
        let b = link(d);
        unlink(d, a);
        unlink(d, b);
        assert_eq!(d.tn_size.get(), 0);
        assert_eq!(d.tn_spec.tn_dir.tn_next_seq.get(), TMPFS_DIRSEQ_START);
    }

    #[test]
    fn a_full_directory_takes_no_more_entries() {
        let d = dir();
        d.tn_spec.tn_dir.tn_next_seq.set(TMPFS_DIRSEQ_END);
        assert!(tmpfs_dirseq_full(d));
    }

    #[test]
    fn update_sets_the_requested_times() {
        let node = TmpfsNode::new();
        let never = Timespec::new(-1, 0);
        node.tn_atime.set(never);
        node.tn_mtime.set(never);
        node.tn_ctime.set(never);
        tmpfs_update(&node, TMPFS_NODE_MODIFIED);
        assert_eq!(node.tn_atime.get(), never);
        assert_eq!(node.tn_ctime.get(), never);
        assert_ne!(node.tn_mtime.get(), never);
        tmpfs_update(&node, TMPFS_NODE_STATUSALL);
        assert_eq!(node.tn_atime.get(), node.tn_ctime.get());
        assert_eq!(node.tn_atime.get(), node.tn_mtime.get());
    }

    #[test]
    fn the_page_cache_bookkeeping_pairs_number_and_address() {
        let node = TmpfsNode::new();
        assert!(!tmpfs_uio_cached(&node));
        assert_eq!(tmpfs_uio_lookup(&node, 0), None);
        tmpfs_uio_cache(&node, PAGE_SIZE as Voff, 0x1000_0000);
        assert!(tmpfs_uio_cached(&node));
        assert_eq!(
            tmpfs_uio_lookup(&node, PAGE_SIZE as Voff),
            Some(0x1000_0000)
        );
        assert_eq!(tmpfs_uio_lookup(&node, 0), None);
    }
}
/* </TESTS> */
