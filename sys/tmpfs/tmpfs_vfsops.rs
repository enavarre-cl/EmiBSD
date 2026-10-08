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

/* <CODE> */
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
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
pub(crate) mod tests {
    // Host tests for tmpfs over a real mount: `tmpfs_mount`, the root vnode, files,
    // directories and symbolic links made with `tmpfs_alloc_file`, lookups, `getdents`,
    // resizing a file's object, attribute changes, file handles, `statfs` and `tmpfs_unmount`
    // (the vfs setup is `vfs_subr.rs`'s). The data path (`tmpfs_uiomove`,
    // `tmpfs_zeropg`) maps the object into `kernel_map` and needs the kernel's faults, so it
    // runs in the kernel only.

    use core::mem::offset_of;
    use std::sync::MutexGuard;
    use std::vec::Vec;
    use std::{assert, assert_eq};

    use super::*;
    use crate::kern::vfs_lookup::ndinit;
    use crate::kern::vfs_subr::{vfs_mount_alloc, vput, vrele};
    use crate::machine::Machine;
    use crate::machine::cpu::Cpu;
    use crate::sys::dirent::Dirent;
    use crate::sys::mount::TMPFS_ARGS_VERSION;
    use crate::sys::namei::{Componentname, LOOKUP, NiDirp};
    use crate::sys::types::Off;
    use crate::sys::uio::{Iovec, Uio, UioRw, UioSeg};
    use crate::sys::vnode::{VLNK, VREG, VROOT, Vattr};
    use crate::tmpfs::tmpfs::{TMPFS_DIRSEQ_EOF, TMPFS_DIRSEQ_START, VP_TO_TMPFS_DIR};
    use crate::tmpfs::tmpfs_subr::{
        tmpfs_alloc_file, tmpfs_chmod, tmpfs_chown, tmpfs_dir_cached, tmpfs_dir_getdents,
        tmpfs_dir_lookup, tmpfs_reg_resize, tmpfs_truncate,
    };
    use crate::uvm::uvm_aobj::uao_init;

    /// The configuration entry of tmpfs in `vfsconflist[]`.
    pub(crate) fn tmpfs_conf() -> &'static Vfsconf {
        crate::kern::vfs_init::vfs_byname(b"tmpfs").expect("tmpfs in vfsconflist[]")
    }

    /// Memory, the vfs (whose `vfsinit` runs `tmpfs_init`), the aobj pools, the thread as
    /// `curproc`.
    pub(crate) fn setup() -> (MutexGuard<'static, ()>, &'static Proc) {
        let (g, p) = crate::kern::vfs_subr::tests::setup();
        Machine::set_curproc(Machine::curcpu(), p);
        crate::kern::kern_rwlock::rw_obj_init();
        uao_init();
        TMPFS_BYTES_USED.store(0, Ordering::Relaxed);
        (g, p)
    }

    /// The bytes of a `struct tmpfs_args` as `sys_mount` copies them in.
    pub(crate) fn args(
        size_max: i64,
        nodes_max: u64,
        uid: Uid,
        mode: Mode,
    ) -> [u8; TmpfsArgs::SIZE] {
        let mut b = [0u8; TmpfsArgs::SIZE];
        let mut put = |off: usize, v: &[u8]| b[off..off + v.len()].copy_from_slice(v);
        put(
            offset_of!(TmpfsArgs, ta_version),
            &TMPFS_ARGS_VERSION.to_ne_bytes(),
        );
        put(
            offset_of!(TmpfsArgs, ta_nodes_max),
            &nodes_max.to_ne_bytes(),
        );
        put(offset_of!(TmpfsArgs, ta_size_max), &size_max.to_ne_bytes());
        put(offset_of!(TmpfsArgs, ta_root_uid), &uid.to_ne_bytes());
        put(offset_of!(TmpfsArgs, ta_root_gid), &0u32.to_ne_bytes());
        put(offset_of!(TmpfsArgs, ta_root_mode), &mode.to_ne_bytes());
        b
    }

    /// `mount -t tmpfs` on "/tmp" with these arguments.
    fn mount(p: &'static Proc, data: &mut [u8]) -> Result<&'static Mount, Errno> {
        let mp = vfs_mount_alloc(None, tmpfs_conf());
        let mut nd = ndinit(LOOKUP, 0, NiDirp::Sys(b"/tmp"), p);
        tmpfs_mount(mp, b"/tmp", data, &mut nd, p).map(|()| mp)
    }

    /// A component name for `name`, with the thread's credentials and no pathname buffer.
    fn cn(p: &'static Proc, name: &'static [u8]) -> Componentname {
        let mut cnp = Componentname::new();
        cnp.cn_proc = p;
        cnp.cn_cred = p.ucred();
        cnp.cn_nameptr = name.as_ptr();
        cnp.cn_namelen = name.len() as i64;
        cnp
    }

    /// The attributes `VOP_CREATE`/`VOP_MKDIR`/`VOP_SYMLINK` pass for a new node.
    fn vattr(type_: crate::sys::vnode::Vtype, mode: Mode) -> Vattr {
        let mut va = Vattr::new();
        va.va_type = type_;
        va.va_mode = mode;
        va.va_rdev = VNOVAL;
        va
    }

    /// `getdents` of a directory from `offset` into a buffer of `len` bytes: the names read
    /// and the offset the read ended at.
    fn getdents(node: &'static TmpfsNode, offset: Off, len: usize) -> (Vec<Vec<u8>>, Off) {
        let mut buf = std::vec![0u8; len];
        let mut iov = [Iovec {
            iov_base: buf.as_mut_ptr().cast(),
            iov_len: len,
        }];
        let mut uio = Uio {
            uio_iov: &mut iov,
            uio_offset: offset,
            uio_resid: len,
            uio_segflg: UioSeg::UIO_SYSSPACE,
            uio_rw: UioRw::UIO_READ,
            uio_procp: None,
        };
        tmpfs_dir_getdents(node, &mut uio).expect("getdents");
        let used = len - uio.uio_resid;
        let end = uio.uio_offset;

        let mut names = Vec::new();
        let mut off = 0;
        while off < used {
            let d = Dirent::from_bytes(&buf[off..]).expect("a dirent");
            let name = &buf[off + Dirent::NAME_OFFSET..][..usize::from(d.d_namlen)];
            names.push(name.to_vec());
            off += usize::from(d.d_reclen);
        }
        (names, end)
    }

    #[test]
    fn mount_rejects_unset_owners() {
        let (_g, p) = setup();
        let mut data = args(0, 0, VNOVAL as Uid, 0o755);
        assert_eq!(mount(p, &mut data).err(), Some(Errno::EINVAL));
        let mut short = [0u8; 8];
        assert_eq!(mount(p, &mut short).err(), Some(Errno::EINVAL));
    }

    #[test]
    fn files_directories_and_links_live_and_die_with_the_mount() {
        let (_g, p) = setup();
        let mut data = args(64 * 1024, 0, 0, 0o1777);
        let mp = mount(p, &mut data).expect("mount");
        let tmp = VFS_TO_TMPFS(mp);
        assert_eq!(TMPFS_BYTES_USED.load(Ordering::Relaxed), 64 * 1024);
        assert_eq!(tmp.tm_nodes_max.get(), 3 + 64);
        assert_eq!(&mp.mnt_stat.get().f_mntonname[..5], b"/tmp\0");
        assert_eq!(&mp.mnt_stat.get().f_mntfromname[..6], b"tmpfs\0");
        assert!(mp.mnt_flag.get() & MNT_LOCAL != 0);

        // the root: its own parent, mode as given, a vnode flagged VROOT
        let root = tmp.root();
        assert!(
            root.tn_spec
                .tn_dir
                .tn_parent
                .get()
                .is_some_and(|r| ptr::eq(r, root))
        );
        assert_eq!(root.tn_links.get(), 2);
        assert_eq!(root.tn_mode.get(), 0o1777);
        let dvp = tmpfs_root(mp).expect("root vnode");
        assert!(dvp.v_flag.get() & VROOT != 0);
        assert!(ptr::eq(VP_TO_TMPFS_DIR(dvp), root));

        // a file, a directory and a symbolic link
        let fvp =
            tmpfs_alloc_file(dvp, &vattr(VREG, 0o644), &mut cn(p, b"file"), None).expect("create");
        let ddvp =
            tmpfs_alloc_file(dvp, &vattr(VDIR, 0o755), &mut cn(p, b"dir"), None).expect("mkdir");
        let lvp = tmpfs_alloc_file(
            dvp,
            &vattr(VLNK, 0o777),
            &mut cn(p, b"link"),
            Some(b"file\0"),
        )
        .expect("symlink");
        let (file, sub, link) = (
            VP_TO_TMPFS_NODE(fvp),
            VP_TO_TMPFS_NODE(ddvp),
            VP_TO_TMPFS_NODE(lvp),
        );
        assert_eq!(root.tn_size.get(), 3 * size_of::<TmpfsDirent>() as Off);
        assert_eq!(root.tn_links.get(), 3, "'.', the root's own and dir's '..'");
        assert_eq!(file.tn_links.get(), 1);
        assert_eq!(sub.tn_links.get(), 2);
        assert!(
            sub.tn_spec
                .tn_dir
                .tn_parent
                .get()
                .is_some_and(|r| ptr::eq(r, root))
        );
        assert_eq!(link.link(), b"file");
        assert_eq!(link.tn_size.get(), 4);
        assert_eq!(tmp.tm_nodes_cnt.get(), 4);

        // lookups
        let de = tmpfs_dir_lookup(root, &cn(p, b"dir")).expect("dir's entry");
        assert!(de.td_node.get().is_some_and(|n| ptr::eq(n, sub)));
        assert_eq!(de.td_seq.get(), TMPFS_DIRSEQ_START + 1);
        assert!(tmpfs_dir_lookup(root, &cn(p, b"none")).is_none());
        assert!(tmpfs_dir_cached(file).is_some_and(|de| de.name() == b"file"));

        // getdents: everything at once, then two entries at a time
        let (names, end) = getdents(root, 0, 1024);
        assert_eq!(names, [&b"."[..], b"..", b"file", b"dir", b"link"]);
        assert_eq!(end as u64, TMPFS_DIRSEQ_EOF);
        let (names, end) = getdents(root, 0, 64);
        assert_eq!(names, [&b"."[..], b".."]);
        assert_eq!(end as u64, TMPFS_DIRSEQ_START);
        let (names, end) = getdents(root, end, 64);
        assert_eq!(names, [&b"file"[..], b"dir"]);
        let (names, end) = getdents(root, end, 64);
        assert_eq!(names, [&b"link"[..]]);
        assert_eq!(end as u64, TMPFS_DIRSEQ_EOF);
        let (names, _) = getdents(sub, 0, 1024);
        assert_eq!(names, [&b"."[..], b".."]);

        // the file's object grows and shrinks a page at a time, accounted to the mount
        let used = tmp.tm_bytes_used.get();
        tmpfs_reg_resize(fvp, 3 * PAGE_SIZE as Off).expect("grow");
        assert_eq!(file.tn_spec.tn_reg.tn_aobj_pages.get(), 3);
        assert_eq!(tmp.tm_bytes_used.get(), used + 3 * PAGE_SIZE as u64);
        tmpfs_truncate(fvp, PAGE_SIZE as Off).expect("shrink");
        assert_eq!(file.tn_spec.tn_reg.tn_aobj_pages.get(), 1);
        assert_eq!(file.tn_size.get(), PAGE_SIZE as Off);
        assert_eq!(tmpfs_truncate(fvp, -1), Err(Errno::EINVAL));
        assert_eq!(
            tmpfs_reg_resize(fvp, 1 << 20).err(),
            Some(Errno::ENOSPC),
            "past the 64 KB limit"
        );
        assert_eq!(file.tn_size.get(), PAGE_SIZE as Off);

        // attributes, as root
        let cred = p.ucred();
        tmpfs_chmod(fvp, 0o4600, cred, p).expect("chmod");
        assert_eq!(file.tn_mode.get(), 0o4600);
        tmpfs_chown(fvp, 7, VNOVAL as Gid, cred, p).expect("chown");
        assert_eq!((file.tn_uid.get(), file.tn_gid.get()), (7, 0));

        // file handles
        let mut fid = Fid::default();
        tmpfs_vptofh(fvp, &mut fid).expect("vptofh");
        assert_eq!(usize::from(fid.fid_len), TmpfsFid::SIZE);
        let _ = crate::kern::vfs_vops::VOP_UNLOCK(fvp);
        let again = tmpfs_fhtovp(mp, &fid).expect("fhtovp");
        assert!(ptr::eq(again, fvp));
        vput(again);
        let mut stale = fid;
        stale.fid_data[4] ^= 0xff;
        assert_eq!(tmpfs_fhtovp(mp, &stale).err(), Some(Errno::ESTALE));

        // statfs
        let mut sb = Statfs::new();
        tmpfs_statfs(mp, &mut sb, p).expect("statfs");
        assert_eq!(sb.f_blocks, 16);
        assert_eq!(sb.f_files, 4 + sb.f_ffree);

        // drop the vnodes (fvp is unlocked already), then unmount
        vrele(fvp);
        vput(ddvp);
        vput(lvp);
        vput(dvp);
        tmpfs_unmount(mp, 0, p).expect("unmount");
        assert!(mp.mnt_data.get().is_null());
        assert_eq!(TMPFS_BYTES_USED.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn the_node_limit_is_enospc() {
        let (_g, p) = setup();
        let mut data = args(0, 4, 0, 0o755);
        let mp = mount(p, &mut data).expect("mount");
        let tmp = VFS_TO_TMPFS(mp);
        assert_eq!(tmp.tm_nodes_max.get(), 4);
        let dvp = tmpfs_root(mp).expect("root vnode");

        let mut vps = Vec::new();
        for name in [&b"a"[..], b"b", b"c"] {
            let name: &'static [u8] = std::boxed::Box::leak(name.to_vec().into_boxed_slice());
            vps.push(
                tmpfs_alloc_file(dvp, &vattr(VREG, 0o644), &mut cn(p, name), None).expect("create"),
            );
        }
        assert_eq!(
            tmpfs_alloc_file(dvp, &vattr(VREG, 0o644), &mut cn(p, b"d"), None).err(),
            Some(Errno::ENOSPC)
        );
        assert_eq!(tmp.tm_nodes_cnt.get(), 4);

        for vp in vps {
            vput(vp);
        }
        vput(dvp);
        tmpfs_unmount(mp, 0, p).expect("unmount");
    }
}
/* </TESTS> */
