/*	$OpenBSD: tmpfs_vfsops.c,v 1.21 2025/11/21 09:49:33 mvs Exp $	*/
/*	$NetBSD: tmpfs_vfsops.c,v 1.52 2011/09/27 01:10:43 christos Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 2005, 2006, 2007 The NetBSD Foundation, Inc.
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

//! Efficient memory file system: the file-system-type operations (mount, unmount, root,
//! file handles, statfs, sync, init).
//!
//! tmpfs is a file system that uses NetBSD's virtual memory sub-system (the well-known UVM)
//! to store file data and metadata in an efficient way. This means that it does not follow
//! the structure of an on-disk file system because it simply does not need to. Instead, it
//! uses memory-specific data structures and algorithms to automatically allocate and
//! release resources.
//!
//! Upstream: sys/tmpfs/tmpfs_vfsops.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `tmpfs_mount` reads its `struct tmpfs_args` out of the kernel copy of the mount
//!   arguments (`TmpfsArgs::from_bytes`), `EINVAL` when they are short; the C dereferences
//!   the pointer it is given.
//! - `tmpfs_mount_update` panics when the root node has no vnode, where the C would follow
//!   the NULL `tn_vnode`.
//! - `tmpfs_root`, `tmpfs_vget` and `tmpfs_fhtovp` return the vnode (`Vfsops`'s shape).
//! - `(void *)eopnotsupp` in `vfs_quotactl`, `vfs_sysctl` and `vfs_checkexp` are closures
//!   calling `eopnotsupp`.
//! - The `struct tmpfs_mount` is `malloc(M_MISCFSMNT)`ed and a fresh [`TmpfsMount::new`]
//!   written into it.
//! - `TMPFS_VFSOPS` is not in `vfsconflist[]` yet: it joins with the vnode operations of
//!   `tmpfs_vnops.c` (M10C-PENDING: tmpfs_vnops.c).

use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::sync::atomic::Ordering;

use libkern::strlcpy;

use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_rwlock::{
    rw_enter_read, rw_enter_write, rw_exit_read, rw_exit_write, rw_init,
};
use crate::kern::subr_pool::pool_init;
use crate::kern::subr_prf::{panic, printf};
use crate::kern::subr_xxx::eopnotsupp;
use crate::kern::vfs_subr::{copy_statfs_info, vflush, vfs_getnewfsid};
use crate::kern::vfs_vnops::vn_lock;
use crate::kern::vfs_vops::VOP_UNLOCK;
use crate::machine::intr::IPL_NONE;
use crate::sys::errno::Errno;
use crate::sys::limits::INT_MAX;
use crate::sys::lock::{LK_EXCLUSIVE, LK_RETRY};
use crate::sys::malloc::{M_MISCFSMNT, M_WAITOK};
use crate::sys::mount::{
    Fid, MNAMELEN, MNT_FORCE, MNT_LOCAL, MNT_RDONLY, MNT_UPDATE, MNT_WANTRDWR, Mount, Statfs,
    TmpfsArgs, Vfsconf, Vfsops,
};
use crate::sys::namei::Nameidata;
use crate::sys::param::{PAGE_SHIFT, PAGE_SIZE};
use crate::sys::pool::{PR_WAITOK, Pool};
use crate::sys::proc::Proc;
use crate::sys::stat::ALLPERMS;
use crate::sys::types::{Gid, Ino, Mode, Uid};
use crate::sys::ucred::Ucred;
use crate::sys::vnode::{FORCECLOSE, VDIR, VNOVAL, Vnode, WRITECLOSE};
use crate::tmpfs::tmpfs::{
    TMPFS_MAXNAMLEN, TmpfsDirent, TmpfsFid, TmpfsMount, TmpfsNode, VFS_TO_TMPFS, VP_TO_TMPFS_NODE,
    tmpfs_node_gen,
};
use crate::tmpfs::tmpfs_mem::{
    TMPFS_BYTES_LIMIT, TMPFS_BYTES_USED, tmpfs_mntmem_destroy, tmpfs_mntmem_init,
    tmpfs_pages_avail, tmpfs_pages_total,
};
use crate::tmpfs::tmpfs_subr::{
    tmpfs_alloc_node, tmpfs_dir_detach, tmpfs_free_dirent, tmpfs_free_node, tmpfs_vnode_get,
};
use crate::uvm::uvm_init::UVMEXP;

/// `tmpfs_dirent_pool`.
pub static TMPFS_DIRENT_POOL: Pool = Pool::new();
/// `tmpfs_node_pool`.
pub static TMPFS_NODE_POOL: Pool = Pool::new();

/// `tmpfs_vfsops`: tmpfs vfs operations.
pub static TMPFS_VFSOPS: Vfsops = Vfsops {
    vfs_mount: tmpfs_mount,
    vfs_start: tmpfs_start,
    vfs_unmount: tmpfs_unmount,
    vfs_root: tmpfs_root,
    vfs_quotactl: |_, _, _, _, _| eopnotsupp(),
    vfs_statfs: tmpfs_statfs,
    vfs_sync: tmpfs_sync,
    vfs_vget: tmpfs_vget,
    vfs_fhtovp: tmpfs_fhtovp,
    vfs_vptofh: tmpfs_vptofh,
    vfs_init: Some(tmpfs_init),
    vfs_sysctl: Some(|_, _, _, _, _, _| eopnotsupp()),
    vfs_checkexp: |_, _, _, _| eopnotsupp(),
};

/// `tmpfs_init` (`vfs_init`): the global memory limit (half of the managed pages) and the
/// node and directory entry pools.
pub fn tmpfs_init(_vfsp: &'static Vfsconf) -> Result<(), Errno> {
    let npages = UVMEXP.npages.load(Ordering::Relaxed);
    TMPFS_BYTES_LIMIT.store(((npages / 2) as u64) << PAGE_SHIFT, Ordering::Relaxed);

    pool_init(
        &TMPFS_DIRENT_POOL,
        size_of::<TmpfsDirent>(),
        0,
        IPL_NONE,
        PR_WAITOK,
        "tmpfs_dirent",
        None,
    );
    pool_init(
        &TMPFS_NODE_POOL,
        size_of::<TmpfsNode>(),
        0,
        IPL_NONE,
        PR_WAITOK,
        "tmpfs_node",
        None,
    );

    Ok(())
}

/// `tmpfs_mount_update`: `mount -u`. Only a read-write to read-only change is supported:
/// it flushes the files opened for writing.
pub fn tmpfs_mount_update(mp: &'static Mount) -> Result<(), Errno> {
    if mp.mnt_flag.get() & MNT_RDONLY == 0 {
        return Err(Errno::EOPNOTSUPP);
    }

    // ro->rw transition: nothing to do?
    if mp.mnt_flag.get() & MNT_WANTRDWR != 0 {
        return Ok(());
    }

    let tmp = VFS_TO_TMPFS(mp);
    let Some(rootvp) = tmp.root().tn_vnode.get() else {
        panic(format_args!("tmpfs_mount_update: the root has no vnode"));
    };

    // Lock root to prevent lookups.
    vn_lock(rootvp, LK_EXCLUSIVE | LK_RETRY)?;

    // Lock mount point to prevent nodes from being added/removed.
    rw_enter_write(&tmp.tm_lock);

    // Flush files opened for writing; skip rootvp.
    let error = vflush(mp, Some(rootvp), WRITECLOSE);

    rw_exit_write(&tmp.tm_lock);
    let _ = VOP_UNLOCK(rootvp);

    error
}

/// `roundup(x, PAGE_SIZE)` over the C's `off_t` (a negative size wraps as in C).
fn roundup_page(x: i64) -> i64 {
    let y = PAGE_SIZE as i64;
    (x.wrapping_add(y - 1) / y).wrapping_mul(y)
}

/// `tmpfs_mount` (`vfs_mount`): mount system call. `data` is the kernel copy of the user's
/// `struct tmpfs_args`.
pub fn tmpfs_mount(
    mp: &'static Mount,
    path: &[u8],
    data: &mut [u8],
    _ndp: &mut Nameidata<'_>,
    _p: &Proc,
) -> Result<(), Errno> {
    if mp.mnt_flag.get() & MNT_UPDATE != 0 {
        return tmpfs_mount_update(mp);
    }

    let Some(args) = TmpfsArgs::from_bytes(data) else {
        return Err(Errno::EINVAL);
    };

    if args.ta_root_uid == VNOVAL as Uid
        || args.ta_root_gid == VNOVAL as Gid
        || args.ta_root_mode == VNOVAL as Mode
    {
        return Err(Errno::EINVAL);
    }

    // Get the memory usage limit for this file-system.
    let mut memlimit: u64 = 0;
    if args.ta_size_max != 0 {
        memlimit = roundup_page(args.ta_size_max) as u64;

        let avail = TMPFS_BYTES_LIMIT
            .load(Ordering::Relaxed)
            .wrapping_sub(TMPFS_BYTES_USED.load(Ordering::Relaxed));
        if avail < memlimit {
            return Err(Errno::EINVAL); // historic error
        }
        TMPFS_BYTES_USED.fetch_add(memlimit, Ordering::Relaxed);
    }

    let mut nodes: u64 = if args.ta_nodes_max <= 3 {
        3 + (if memlimit != 0 { memlimit } else { u64::MAX }) / 1024
    } else {
        args.ta_nodes_max
    };
    nodes = nodes.min(INT_MAX as u64);
    crate::kassert!(nodes >= 3);

    // Allocate the tmpfs mount structure and fill it.
    let Some(mem) = malloc(size_of::<TmpfsMount>(), M_MISCFSMNT, M_WAITOK) else {
        panic(format_args!("tmpfs_mount: malloc failed"));
    };
    let tmp = mem.cast::<TmpfsMount>();
    // SAFETY: a fresh allocation of `size_of::<TmpfsMount>()` bytes (malloc aligns to the
    // bucket size, at least 16), written once; it lives until `tmpfs_unmount` frees it.
    let tmp: &'static TmpfsMount = unsafe {
        tmp.as_ptr().write(TmpfsMount::new());
        tmp.as_ref()
    };

    tmp.tm_nodes_max.set(nodes as u32);
    tmp.tm_nodes_cnt.set(0);
    tmp.tm_highest_inode.set(1);
    tmp.tm_nodes.init();

    rw_init(&tmp.tm_lock, "tmplk");
    tmpfs_mntmem_init(tmp, memlimit);

    // Allocate the root node.
    let root = tmpfs_alloc_node(
        tmp,
        VDIR,
        args.ta_root_uid,
        args.ta_root_gid,
        args.ta_root_mode & ALLPERMS,
        None,
        VNOVAL,
    );
    crate::kassert!(root.is_ok());
    let root = match root {
        Ok(root) => root,
        Err(e) => panic(format_args!(
            "tmpfs_mount: no root node (error {})",
            e as i32
        )),
    };

    // Parent of the root inode is itself. Also, root inode has no directory entry (i.e. is
    // never attached), thus hold an extra reference (link) for it.
    root.tn_links.set(root.tn_links.get() + 1);
    root.tn_spec.tn_dir.tn_parent.set(Some(root));
    tmp.tm_root.set(Some(root));

    mp.mnt_data
        .set(ptr::from_ref(tmp).cast_mut().cast::<c_void>());
    mp.mnt_flag.set(mp.mnt_flag.get() | MNT_LOCAL);
    mp.update_stat(|sp| sp.f_namemax = TMPFS_MAXNAMLEN as u32);
    vfs_getnewfsid(mp);

    mp.update_stat(|sp| {
        sp.mount_info.__align[..TmpfsArgs::SIZE].copy_from_slice(&data[..TmpfsArgs::SIZE]);

        sp.f_mntonname = [0; MNAMELEN];
        sp.f_mntfromname = [0; MNAMELEN];
        sp.f_mntfromspec = [0; MNAMELEN];

        strlcpy(&mut sp.f_mntonname[..MNAMELEN - 1], path);
        strlcpy(&mut sp.f_mntfromname[..MNAMELEN - 1], b"tmpfs");
        strlcpy(&mut sp.f_mntfromspec[..MNAMELEN - 1], b"tmpfs");
    });

    Ok(())
}

/// `tmpfs_start` (`vfs_start`).
pub fn tmpfs_start(_mp: &'static Mount, _flags: i32, _p: &Proc) -> Result<(), Errno> {
    Ok(())
}

/// `tmpfs_unmount` (`vfs_unmount`): flush the vnodes, then destroy every directory entry
/// and node and the mount structure.
pub fn tmpfs_unmount(mp: &'static Mount, mntflags: i32, _p: &Proc) -> Result<(), Errno> {
    let tmp = VFS_TO_TMPFS(mp);
    let mut flags = 0;

    // Handle forced unmounts.
    if mntflags & MNT_FORCE != 0 {
        flags |= FORCECLOSE;
    }

    // Finalize all pending I/O.
    vflush(mp, None, flags)?;

    // First round, detach and destroy all directory entries. Also, clear the pointers to
    // the vnodes - they are gone.
    for node in tmp.tm_nodes.iter() {
        node.tn_vnode.set(None);
        if node.tn_type.get() != VDIR {
            continue;
        }
        while let Some(de) = node.tn_spec.tn_dir.tn_dir.first() {
            if let Some(cnode) = de.td_node.get() {
                cnode.tn_vnode.set(None);
            }
            tmpfs_dir_detach(node, de);
            tmpfs_free_dirent(tmp, de);
        }
    }

    // Second round, destroy all inodes.
    while let Some(node) = tmp.tm_nodes.first() {
        tmpfs_free_node(tmp, node);
    }

    if tmp.tm_mem_limit.get() != 0 {
        TMPFS_BYTES_USED.fetch_sub(tmp.tm_mem_limit.get(), Ordering::Relaxed);
    }

    // Throw away the tmpfs_mount structure.
    tmpfs_mntmem_destroy(tmp);
    // mutex_destroy(&tmp->tm_lock); kmem_free(tmp, sizeof(*tmp));
    free(
        NonNull::from(tmp).cast::<u8>(),
        M_MISCFSMNT,
        size_of::<TmpfsMount>(),
    );
    mp.mnt_data.set(ptr::null_mut());

    Ok(())
}

/// `tmpfs_root` (`vfs_root`): the root directory's vnode, locked.
pub fn tmpfs_root(mp: &'static Mount) -> Result<&'static Vnode, Errno> {
    let node = VFS_TO_TMPFS(mp).root();

    rw_enter_write(&node.tn_nlock);
    tmpfs_vnode_get(mp, node)
}

/// `tmpfs_vget` (`vfs_vget`).
pub fn tmpfs_vget(_mp: &'static Mount, _ino: Ino) -> Result<&'static Vnode, Errno> {
    printf(format_args!("tmpfs_vget called; need for it unknown yet\n"));
    Err(Errno::EOPNOTSUPP)
}

/// `tmpfs_fhtovp` (`vfs_fhtovp`): the vnode of the node a file handle names, locked.
pub fn tmpfs_fhtovp(mp: &'static Mount, fhp: &Fid) -> Result<&'static Vnode, Errno> {
    let tmp = VFS_TO_TMPFS(mp);

    if usize::from(fhp.fid_len) != TmpfsFid::SIZE {
        return Err(Errno::EINVAL);
    }
    let tfh = TmpfsFid::from_fid(fhp);

    rw_enter_write(&tmp.tm_lock);
    let node = tmp.tm_nodes.iter().find(|node| {
        node.tn_id.get() == tfh.tf_id && tmpfs_node_gen(node) == u64::from(tfh.tf_gen)
    });
    if let Some(node) = node {
        rw_enter_write(&node.tn_nlock);
    }
    rw_exit_write(&tmp.tm_lock);

    // Will release the tn_nlock.
    match node {
        Some(node) => tmpfs_vnode_get(mp, node),
        None => Err(Errno::ESTALE),
    }
}

/// `tmpfs_vptofh` (`vfs_vptofh`): the file handle of a vnode's node.
pub fn tmpfs_vptofh(vp: &'static Vnode, fhp: &mut Fid) -> Result<(), Errno> {
    let node = VP_TO_TMPFS_NODE(vp);

    let tfh = TmpfsFid {
        tf_len: TmpfsFid::SIZE as u16,
        tf_pad: 0,
        tf_gen: tmpfs_node_gen(node) as u32,
        tf_id: node.tn_id.get(),
    };
    tfh.to_fid(fhp);

    Ok(())
}

/// `tmpfs_statfs` (`vfs_statfs`).
pub fn tmpfs_statfs(mp: &'static Mount, sbp: &mut Statfs, _p: &Proc) -> Result<(), Errno> {
    let tmp = VFS_TO_TMPFS(mp);

    sbp.f_bsize = PAGE_SIZE as u32;
    sbp.f_iosize = PAGE_SIZE as u32;

    rw_enter_read(&tmp.tm_acc_lock);
    let avail = tmpfs_pages_avail(tmp);
    sbp.f_blocks = tmpfs_pages_total(tmp);
    sbp.f_bfree = avail;
    sbp.f_bavail = (avail & i64::MAX as u64) as i64; // f_bavail is int64_t

    let freenodes = u64::from(tmp.tm_nodes_max.get().wrapping_sub(tmp.tm_nodes_cnt.get()))
        .min(avail * PAGE_SIZE as u64 / size_of::<TmpfsNode>() as u64);

    sbp.f_files = u64::from(tmp.tm_nodes_cnt.get()) + freenodes;
    sbp.f_ffree = freenodes;
    sbp.f_favail = (freenodes & i64::MAX as u64) as i64; // f_favail is int64_t
    rw_exit_read(&tmp.tm_acc_lock);

    copy_statfs_info(sbp, mp);

    Ok(())
}

/// `tmpfs_sync` (`vfs_sync`): nothing to write back.
pub fn tmpfs_sync(
    _mp: &'static Mount,
    _waitfor: i32,
    _stall: i32,
    _cred: *const Ucred,
    _p: &Proc,
) -> Result<(), Errno> {
    Ok(())
}

#[cfg(test)]
mod tests;
