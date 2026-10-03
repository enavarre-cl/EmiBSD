/*	$OpenBSD: vfs_subr.c,v 1.335 2026/06/30 14:04:03 kirill Exp $	*/
/*	$NetBSD: vfs_subr.c,v 1.53 1996/04/22 01:39:13 christos Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1989, 1993
 *	The Regents of the University of California.  All rights reserved.
 * (c) UNIX System Laboratories, Inc.
 * All or some portions of this file are derived from material licensed
 * to the University of California by American Telephone and Telegraph
 * Co. or Unix System Laboratories, Inc. and are reproduced herein with
 * the permission of UNIX System Laboratories, Inc.
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
 *	@(#)vfs_subr.c	8.13 (Berkeley) 4/18/94
 */
/* </LICENSES> */

//! External virtual filesystem routines: the vnode table (`getnewvnode`, the free and hold
//! lists, `vget`/`vref`/`vput`/`vrele`, `vhold`/`vdrop`, `vclean`/`vgone`/`vflush`), the
//! special-device aliases (`bdevvp`, `cdevvp`, `checkalias`, `vfinddev`, `vcount`), mount
//! structures (`vfs_mount_alloc`, `vfs_busy`, `vfs_rootmountalloc`, `vfs_getvfs`,
//! `vfs_getnewfsid`, `vfs_unmountall`, `vfs_stall`, `vfs_shutdown`), `vattr_null`,
//! `vaccess`, `vfs_sysctl` and the vnode/buffer helpers.
//!
//! Upstream: sys/kern/vfs_subr.c @ 3ce1f3f79392
//!
//! Vnodes are never freed: a vnode whose last user goes (`vrele`, `vput`) is deactivated
//! (`VOP_INACTIVE`) and put on `vnode_free_list` (or `vnode_hold_list` while buffers
//! reference it), still carrying its file system's identity, so a later lookup can `vget` it
//! back. `getnewvnode` allocates new vnodes until `maxvnodes` and then recycles from the
//! free lists, `vgonel`ing the old identity away.
//!
//! ## Deviations
//! - Out-parameters are return values: `getnewvnode` and `vfs_rootmountalloc` return the
//!   vnode or mount, `bdevvp`/`cdevvp` return `Option` (the C's NULL for `NODEV`),
//!   `vfinddev` returns `Option`. `vrele` and `vrecycle` return `bool`.
//! - `getnewvnode` fails with `ENFILE` when `vnode_pool` cannot serve `PR_WAITOK` (the pool
//!   cannot sleep yet, `subr_pool.rs`); `vfs_mount_alloc` and `checkalias` panic when
//!   `malloc(M_WAITOK)` fails, since their callers cannot fail.
//! - No buffer cache yet (`vfs_bio.c`): `bcstats.numbufs` is 0 in `getnewvnode`; `vinvalbuf`
//!   and `vflushbuf` wait for output as the C does and report the buffer-list walk;
//!   `vfs_syncwait` reports the `bufhead` scan and finds nothing busy; `bgetvp`, `brelvp`,
//!   `buf_replacevnode`, `reassignbuf` and `rb_buf_compare` take a `struct buf *` as an
//!   opaque pointer and report themselves.
//! - Not here yet, reported with `unported!` where the C calls them: `vn_initialize_syncerd`
//!   and `vn_syncer_add_to_worklist` (`vfs_sync.c`), `uvm_vnp_terminate`/`uvm_vnp_sync`
//!   (`uvm_vnode.c`), `lf_purgelocks` (`vfs_lockf.c`, only when a device vnode has locks,
//!   which none can), the device switch (`cdevsw[].d_type`/`d_flags`, `nblkdev`: `conf.c`,
//!   through `spec_vnops.rs`), `bcstats` for `vfs.generic.bcachestat`. `VN_KNOTE(vp,
//!   NOTE_REVOKE)` has no knotes to post (`kern_event.c`).
//! - `copy_statfs_info` never receives the mount's own `mnt_stat` (the callers pass a copy,
//!   see `sys/mount.rs`), so the C's early return for that case is not needed; the copy has
//!   the same values, so the result is the same.
//! - `NFSSERVER` is not configured: `vfs_hang_addrlist`, `vfs_free_netcred` and
//!   `vfs_free_addrlist` are compiled out as in C, `vfs_export` answers `ENOTSUP` and
//!   `vfs_export_lookup` NULL. The export structures are opaque pointers.
//! - `vprint` is compiled under feature `diagnostic` or `debug`, `printlockedvnodes` under
//!   `debug`, as in C. The DDB printers (`vfs_buf_print`, `vfs_vnode_print`,
//!   `vfs_mount_print`) wait for the ddb command loop (`db_command.c`).
//! - `KERNEL_ASSERT_LOCKED()` is nothing without `MULTIPROCESSOR`.

use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicI32, AtomicI64, AtomicU32, Ordering};

use crate::kassert;
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_prot::groupmember;
use crate::kern::kern_rwlock::{
    rw_enter, rw_enter_read, rw_enter_write, rw_exit, rw_exit_read, rw_exit_write, rw_init_flags,
    rw_status,
};
use crate::kern::kern_synch::{
    msleep_nsec, refcnt_init, refcnt_rele, refcnt_take, tsleep_nsec, wakeup,
};
use crate::kern::kern_sysctl::{sysctl_rdint, sysctl_rdstruct};
use crate::kern::spec_vnops::{
    SPEC_VOPS, SPECLISTH, cdevsw_d_flags_clone, cdevsw_d_type_tty, nblkdev,
};
use crate::kern::subr_pool::{pool_get, pool_init};
use crate::kern::subr_prf::{panic, panicstr, tablefull};
use crate::kern::vfs_cache::{cache_purge, cache_tree_init};
use crate::kern::vfs_init::{MAXVFSCONF, vfs_byname, vfs_bytypenum};
use crate::kern::vfs_syscalls::{dounmount, sys_sync};
use crate::kern::vfs_vnops::vn_lock;
use crate::kern::vfs_vops::{
    VOP_CLOSE, VOP_INACTIVE, VOP_ISLOCKED, VOP_LOCK, VOP_RECLAIM, VOP_REVOKE, VOP_UNLOCK,
};
use crate::kprintf;
use crate::machine::cpu::curproc;
use crate::machine::intr::{IPL_BIO, IPL_NONE, splassert, splbio, splx};
use crate::miscfs::deadfs::dead_vnops::DEAD_VOPS;
use crate::sys::errno::Errno;
use crate::sys::fcntl::FNONBLOCK;
use crate::sys::lock::{LK_DRAIN, LK_EXCLUSIVE, LK_NOWAIT, LK_TYPE_MASK};
use crate::sys::malloc::{M_MOUNT, M_VNODE, M_WAITOK, M_ZERO};
use crate::sys::mount::{
    Fsid, MFSNAMELEN, MNAMELEN, MNT_NOPERM, MNT_RDONLY, MNT_STALLED, MNT_UNMOUNT, MNT_WAIT,
    MntList, Mount, Statfs, VB_DUPOK, VB_NOWAIT, VB_READ, VB_WAIT, VB_WRITE, VFS_BCACHESTAT,
    VFS_CONF, VFS_GENERIC, VFS_MAXTYPENUM, VFS_SYNC, Vfsconf,
};
use crate::sys::mutex::Mutex;
use crate::sys::param::{NODEV, PINOD, PRIBIO};
use crate::sys::pool::{PR_WAITOK, PR_ZERO, Pool};
use crate::sys::proc::Proc;
use crate::sys::queue::TailqHead;
use crate::sys::rwlock::{RW_NOSLEEP, RW_READ, RW_WRITE, RWL_IS_VNODE, Rwlock};
use crate::sys::specdev::{CLONE_MAPSZ, CLONE_SHIFT, Specinfo, spechash};
use crate::sys::stat::{
    S_IFBLK, S_IFCHR, S_IFDIR, S_IFIFO, S_IFLNK, S_IFMT, S_IFREG, S_IFSOCK, S_IRGRP, S_IROTH,
    S_IRUSR, S_IWGRP, S_IWOTH, S_IWUSR, S_IXGRP, S_IXOTH, S_IXUSR,
};
use crate::sys::systm::INFSLP;
use crate::sys::types::{Dev, Gid, Mode, Uid, major, makedev, minor};
use crate::sys::ucred::{NOCRED, Ucred};
use crate::sys::vnode::{
    DOCLOSE, FORCECLOSE, IGNORECLEAN, REVOKEALL, SKIPSYSTEM, V_SAVE, VALIASED, VBAD, VBIOERROR,
    VBIOONFREELIST, VBIOWAIT, VBLK, VCHR, VDIR, VEXEC, VFIFO, VFreelist, VISTTY, VLNK, VNON,
    VNOVAL, VREAD, VREG, VROOT, VSOCK, VSYSTEM, VT_NON, VWRITE, VXLOCK, VXWANT, Vattr, Vnode,
    VnodeUn, Vops, Vtagtype, Vtype, WRITECLOSE,
};
use crate::unported;

/// `iftovt_tab[]`: the vnode type of each inode format (`IFTOVT`).
pub const IFTOVT_TAB: [Vtype; 16] = [
    VNON, VFIFO, VCHR, VNON, VDIR, VNON, VBLK, VNON, VREG, VNON, VLNK, VNON, VSOCK, VNON, VNON,
    VBAD,
];

/// `vttoif_tab[]`: the inode format of each vnode type (`VTTOIF`).
pub const VTTOIF_TAB: [Mode; 9] = [
    0, S_IFREG, S_IFDIR, S_IFBLK, S_IFCHR, S_IFLNK, S_IFSOCK, S_IFIFO, S_IFMT,
];

/// A global list of vnodes (`struct freelst`): the free list or the hold list.
pub struct Freelst(pub TailqHead<VFreelist>);

// SAFETY: the lists are changed at `splbio`, as in C; the kernel runs one CPU.
unsafe impl Sync for Freelst {}

/// `struct mntlist`: the mounted file systems.
pub struct Mntlist(pub TailqHead<MntList>);

// SAFETY: the list is changed under the kernel lock, as in C; the kernel runs one CPU.
unsafe impl Sync for Mntlist {}

/// `prtactive`: 1 => print out reclaim of active vnodes.
pub static PRTACTIVE: AtomicI32 = AtomicI32::new(0);

/// `vnode_hold_list`: list of vnodes referencing buffers.
pub static VNODE_HOLD_LIST: Freelst = Freelst(TailqHead::new());
/// `vnode_free_list`: vnode free list.
pub static VNODE_FREE_LIST: Freelst = Freelst(TailqHead::new());

/// `mountlist`: mounted filesystem list.
pub static MOUNTLIST: Mntlist = Mntlist(TailqHead::new());

/// `maxvnodes`: the number of vnodes `getnewvnode` allocates before it recycles.
pub static MAXVNODES: AtomicI32 = AtomicI32::new(0);

/// `vnode_mtx`: protects the `[V]` members of every vnode (`v_lflag`, `v_lockcount`).
pub static VNODE_MTX: Mutex = Mutex::new(IPL_BIO);

/// `vnode_pool`.
pub static VNODE_POOL: Pool = Pool::new();

// rb_buf_compare and RBT_GENERATE(buf_rb_bufs): struct buf (vfs_bio.c).

/// Initialize the vnode management data structures.
pub fn vntblinit() {
    // buffer cache may need a vnode for each buffer
    MAXVNODES.store(
        2 * crate::conf::param::INITIALVNODES.load(Ordering::Relaxed),
        Ordering::Relaxed,
    );
    pool_init(
        &VNODE_POOL,
        size_of::<Vnode>(),
        0,
        IPL_NONE,
        PR_WAITOK,
        "vnodes",
        None,
    );
    VNODE_HOLD_LIST.0.init();
    VNODE_FREE_LIST.0.init();
    MOUNTLIST.0.init();

    // Initialize the filesystem syncer.
    let _ = unported!("vn_initialize_syncerd (vfs_sync.c)");

    // NFSSERVER: rn_init, not configured.
}

/// Allocate a mount point. The returned mount point is marked as busy.
pub fn vfs_mount_alloc(vp: Option<&'static Vnode>, vfsp: &'static Vfsconf) -> &'static Mount {
    let Some(mem) = malloc(size_of::<Mount>(), M_MOUNT, M_WAITOK | M_ZERO) else {
        panic(format_args!("vfs_mount_alloc: out of memory"));
    };
    let mp = mem.cast::<Mount>();
    // SAFETY: a fresh, suitably aligned allocation of `size_of::<Mount>()` bytes, written
    // once before anything else sees it.
    unsafe { mp.as_ptr().write(Mount::new()) };
    // SAFETY: as above; the mount stays allocated until its last reference goes
    // (`vfs_mount_rele`).
    let mp: &'static Mount = unsafe { mp.as_ref() };
    refcnt_init(&mp.mnt_refs);
    rw_init_flags(&mp.mnt_lock, "vfslock", RWL_IS_VNODE);
    let _ = vfs_busy(mp, VB_READ | VB_NOWAIT);

    mp.mnt_vnodelist.init();
    mp.mnt_vnodecovered.set(vp);

    vfsp.vfc_refcount.fetch_add(1, Ordering::SeqCst);
    mp.mnt_vfc.set(Some(vfsp));
    mp.mnt_op.set(Some(vfsp.vfc_vfsops));
    mp.mnt_flag.set(vfsp.vfc_flags);
    mp.update_stat(|sp| {
        let name = vfsp.name();
        sp.f_fstypename = [0; MFSNAMELEN];
        sp.f_fstypename[..name.len()].copy_from_slice(name);
    });

    mp
}

/// `vfs_mount_take(mp)`: another reference to `mp`.
pub fn vfs_mount_take(mp: &'static Mount) -> &'static Mount {
    refcnt_take(&mp.mnt_refs);
    mp
}

/// `vfs_mount_rele(mp)`: drops a reference; the last one frees the mount.
fn vfs_mount_rele(mp: &'static Mount) {
    if refcnt_rele(&mp.mnt_refs) {
        free(NonNull::from(mp).cast(), M_MOUNT, size_of::<Mount>());
    }
}

/// Release a mount point.
pub fn vfs_mount_free(mp: &'static Mount) {
    mp.mnt_flag.set(mp.mnt_flag.get() | MNT_UNMOUNT);
    mp.vfc().vfc_refcount.fetch_sub(1, Ordering::SeqCst);
    vfs_mount_rele(mp);
}

/// Mark a mount point as busy. Used to synchronize access and to delay unmounting.
///
/// Default behaviour is to attempt getting a READ lock and in case of an ongoing unmount, to
/// wait for it to finish and then return failure.
pub fn vfs_busy(mp: &'static Mount, flags: i32) -> Result<(), Errno> {
    let mut rwflags = if flags & VB_WRITE != 0 {
        RW_WRITE
    } else {
        RW_READ
    };
    let mut error = Ok(());

    if flags & VB_WAIT == 0 {
        rwflags |= RW_NOSLEEP;
    }

    // WITNESS: VB_DUPOK -> RW_DUPOK, not configured.

    vfs_mount_take(mp);
    if rw_enter(&mp.mnt_lock, rwflags).is_err() {
        error = Err(Errno::EBUSY);
    } else if mp.mnt_flag.get() & MNT_UNMOUNT != 0 {
        rw_exit(&mp.mnt_lock);
        error = Err(Errno::EBUSY);
    }
    vfs_mount_rele(mp);

    error
}

/// Free a busy file system.
pub fn vfs_unbusy(mp: &Mount) {
    rw_exit(&mp.mnt_lock);
}

/// `vfs_isbusy(mp)`: whether the mount is busied.
pub fn vfs_isbusy(mp: &Mount) -> bool {
    rw_status(&mp.mnt_lock) != 0
}

/// Lookup a filesystem type, and if found allocate and initialize a mount structure for it.
///
/// Devname is usually updated by mount(8) after booting.
pub fn vfs_rootmountalloc(fstypename: &[u8], devname: &[u8]) -> Result<&'static Mount, Errno> {
    let Some(vfsp) = vfs_byname(fstypename) else {
        return Err(Errno::ENODEV);
    };
    let mp = vfs_mount_alloc(None, vfsp);
    mp.mnt_flag.set(mp.mnt_flag.get() | MNT_RDONLY);
    mp.update_stat(|sp| {
        sp.f_mntonname[0] = b'/';
        strlcpy(&mut sp.f_mntfromname, devname);
        strlcpy(&mut sp.f_mntfromspec, devname);
    });
    Ok(mp)
}

/// `strlcpy(dst, src, sizeof(dst))` of a byte string into a fixed name buffer.
fn strlcpy(dst: &mut [u8; MNAMELEN], src: &[u8]) {
    let src = src.split(|&c| c == 0).next().unwrap_or(&[]);
    let n = src.len().min(MNAMELEN - 1);
    dst[..n].copy_from_slice(&src[..n]);
    dst[n] = 0;
}

/// Lookup a mount point by filesystem identifier.
pub fn vfs_getvfs(fsid: &Fsid) -> Option<&'static Mount> {
    MOUNTLIST
        .0
        .iter()
        .find(|mp| mp.mnt_stat.get().f_fsid.val == fsid.val)
}

/// Get a new unique fsid.
pub fn vfs_getnewfsid(mp: &'static Mount) {
    static XXXFS_MNTID: AtomicU32 = AtomicU32::new(0);

    let mtype = mp.vfc().vfc_typenum;
    let base = nblkdev() + mtype as u32;
    mp.update_stat(|sp| {
        sp.f_fsid.val[0] = makedev(base, 0);
        sp.f_fsid.val[1] = mtype;
    });
    if XXXFS_MNTID.load(Ordering::Relaxed) == 0 {
        XXXFS_MNTID.fetch_add(1, Ordering::Relaxed);
    }
    let mut tfsid = Fsid {
        val: [makedev(base, XXXFS_MNTID.load(Ordering::Relaxed)), mtype],
    };
    if !MOUNTLIST.0.is_empty() {
        while vfs_getvfs(&tfsid).is_some() {
            tfsid.val[0] = tfsid.val[0].wrapping_add(1);
            XXXFS_MNTID.fetch_add(1, Ordering::Relaxed);
        }
    }
    mp.update_stat(|sp| sp.f_fsid.val[0] = tfsid.val[0]);
}

/// Set vnode attributes to `VNOVAL`.
pub fn vattr_null(vap: &mut Vattr) {
    vap.va_type = VNON;
    // Don't get fancy: u_quad_t = u_int = VNOVAL leaves the u_quad_t with 2^31-1 instead of
    // 2^64-1. Just write'm out and let the compiler do its job.
    vap.va_mode = VNOVAL as Mode;
    vap.va_nlink = VNOVAL as u32;
    vap.va_uid = VNOVAL as Uid;
    vap.va_gid = VNOVAL as Gid;
    vap.va_fsid = i64::from(VNOVAL);
    vap.va_fileid = VNOVAL as u64;
    vap.va_size = VNOVAL as u64;
    vap.va_blocksize = i64::from(VNOVAL);
    vap.va_atime.tv_sec = i64::from(VNOVAL);
    vap.va_atime.tv_nsec = i64::from(VNOVAL);
    vap.va_mtime.tv_sec = i64::from(VNOVAL);
    vap.va_mtime.tv_nsec = i64::from(VNOVAL);
    vap.va_ctime.tv_sec = i64::from(VNOVAL);
    vap.va_ctime.tv_nsec = i64::from(VNOVAL);
    vap.va_gen = VNOVAL as u64;
    vap.va_flags = VNOVAL as u64;
    vap.va_rdev = VNOVAL;
    vap.va_bytes = VNOVAL as u64;
    vap.va_filerev = VNOVAL as u64;
    vap.va_vaflags = 0;
}

/// `numvnodes`: the vnodes allocated so far.
pub static NUMVNODES: AtomicI64 = AtomicI64::new(0);

/// `bcstats.numbufs`: the buffer cache (`vfs_bio.c`) is not here, so there are no buffers.
fn bcstats_numbufs() -> i32 {
    0
}

/// Return the next vnode from the free list.
pub fn getnewvnode(
    tag: Vtagtype,
    mp: Option<&'static Mount>,
    vops: &'static Vops,
) -> Result<&'static Vnode, Errno> {
    static TOGGLE: AtomicI32 = AtomicI32::new(0);
    let p = curproc();

    // allow maxvnodes to increase if the buffer cache itself is big enough to justify it.
    // (we don't shrink it ever)
    let maxvnodes = MAXVNODES.load(Ordering::Relaxed).max(bcstats_numbufs());
    MAXVNODES.store(maxvnodes, Ordering::Relaxed);

    // We must choose whether to allocate a new vnode or recycle an existing one. The
    // criterion for allocating a new one is that the total number of vnodes is less than the
    // number desired or there are no vnodes on either free list. Generally we only want to
    // recycle vnodes that have no buffers associated with them, so we look first on the
    // vnode_free_list. If it is empty, we next consider vnodes with referencing buffers on
    // the vnode_hold_list. The toggle ensures that half the time we will use a buffer from
    // the vnode_hold_list, and half the time we will allocate a new one unless the list has
    // grown to twice the desired size. We are reticent to recycle vnodes from the
    // vnode_hold_list because we will lose the identity of all its referencing buffers.
    let mut toggle = TOGGLE.load(Ordering::Relaxed) ^ 1;
    let numvnodes = NUMVNODES.load(Ordering::Relaxed);
    if numvnodes / 2 > i64::from(maxvnodes) {
        toggle = 0;
    }
    TOGGLE.store(toggle, Ordering::Relaxed);

    let s = splbio();
    let listhd = if VNODE_FREE_LIST.0.is_empty() {
        &VNODE_HOLD_LIST
    } else {
        &VNODE_FREE_LIST
    };
    let vp: &'static Vnode = if numvnodes < i64::from(maxvnodes)
        || (VNODE_FREE_LIST.0.is_empty() && (VNODE_HOLD_LIST.0.is_empty() || toggle != 0))
    {
        splx(s);
        let Some(mem) = pool_get(&VNODE_POOL, PR_WAITOK | PR_ZERO) else {
            // PR_WAITOK cannot sleep yet (see the module's deviations).
            tablefull("vnode");
            return Err(Errno::ENFILE);
        };
        let vp = mem.cast::<Vnode>();
        // SAFETY: a fresh, suitably aligned pool item of `size_of::<Vnode>()` bytes, written
        // once before anything else sees it.
        unsafe { vp.as_ptr().write(Vnode::new()) };
        // SAFETY: as above; vnodes are never given back to the pool.
        let vp: &'static Vnode = unsafe { vp.as_ref() };
        // RBT_INIT(buf_rb_bufs, &vp->v_bufs_tree): struct buf (vfs_bio.c).
        cache_tree_init(&vp.v_nc_tree);
        vp.v_cache_dst.init();
        NUMVNODES.fetch_add(1, Ordering::Relaxed);
        vp
    } else {
        let found = listhd.0.iter().find(|vp| VOP_ISLOCKED(vp) == 0);
        // Unless this is a bad time of the month, at most the first NCPUS items on the free
        // list are locked, so this is close enough to being empty.
        let Some(vp) = found else {
            splx(s);
            tablefull("vnode");
            return Err(Errno::ENFILE);
        };

        #[cfg(feature = "diagnostic")]
        if vp.v_usecount.get() != 0 {
            vprint(Some("free vnode"), vp);
            panic(format_args!("free vnode isn't"));
        }

        // SAFETY: `vp` is on `listhd` (it was found there), at splbio.
        unsafe { listhd.0.remove(vp) };
        vp.v_bioflag.set(vp.v_bioflag.get() & !VBIOONFREELIST);
        splx(s);

        if vp.v_type.get() != VBAD {
            vgonel(vp, p);
        }
        #[cfg(feature = "diagnostic")]
        {
            if !vp.v_data.get().is_null() {
                vprint(Some("cleaned vnode"), vp);
                panic(format_args!("cleaned vnode isn't"));
            }
            let s = splbio();
            if vp.v_numoutput.get() != 0 {
                panic(format_args!("Clean vnode has pending I/O's"));
            }
            splx(s);
        }
        vp.v_flag.set(0);
        vp.v_un.set(VnodeUn::None);
        vp
    };
    cache_purge(vp);
    vp.v_type.set(VNON);
    vp.v_tag.set(tag);
    vp.v_op.set(Some(vops));
    insmntque(vp, mp);
    vp.v_usecount.set(1);
    vp.v_data.set(ptr::null_mut());
    Ok(vp)
}

/// Move a vnode from one mount queue to another.
pub fn insmntque(vp: &'static Vnode, mp: Option<&'static Mount>) {
    // Delete from old mount point vnode list, if on one.
    if let Some(old) = vp.v_mount.get() {
        // SAFETY: a vnode with a `v_mount` is on that mount's list (this function is the
        // only one that changes both).
        unsafe { old.mnt_vnodelist.remove(vp) };
    }
    // Insert into list of vnodes for the new mount point, if available.
    vp.v_mount.set(mp);
    if let Some(mp) = mp {
        // SAFETY: the vnode was just taken off every mount list; it never moves.
        unsafe { mp.mnt_vnodelist.insert_tail(vp) };
    }
}

/// Create a vnode for a block device. Used for root filesystem, argdev, and swap areas.
/// Also used for memory file system special devices.
pub fn bdevvp(dev: Dev) -> Result<Option<&'static Vnode>, Errno> {
    getdevvp(dev, VBLK)
}

/// Create a vnode for a character device. Used for console handling.
pub fn cdevvp(dev: Dev) -> Result<Option<&'static Vnode>, Errno> {
    getdevvp(dev, VCHR)
}

/// Create a vnode for a device. Used by bdevvp (block device) for root file system etc., and
/// by cdevvp (character device) for console.
pub fn getdevvp(dev: Dev, type_: Vtype) -> Result<Option<&'static Vnode>, Errno> {
    if dev == NODEV {
        return Ok(None);
    }
    let nvp = getnewvnode(VT_NON, None, &SPEC_VOPS)?;
    let mut vp = nvp;
    vp.v_type.set(type_);
    if let Some(alias) = checkalias(vp, dev, None) {
        vput(vp);
        vp = alias;
    }
    if vp.v_type.get() == VCHR && cdevsw_d_type_tty(major(vp.v_rdev())) {
        vp.v_flag.set(vp.v_flag.get() | VISTTY);
    }
    Ok(Some(vp))
}

/// Check to see if the new vnode represents a special device for which we already have a
/// vnode (either because of bdevvp() or because of a different vnode representing the same
/// block device). If such an alias exists, deallocate the existing contents and return the
/// aliased vnode. The caller is responsible for filling it with its new contents.
pub fn checkalias(
    nvp: &'static Vnode,
    nvp_rdev: Dev,
    mp: Option<&'static Mount>,
) -> Option<&'static Vnode> {
    let p = curproc();

    if nvp.v_type.get() != VBLK && nvp.v_type.get() != VCHR {
        return None;
    }

    let vchain = &SPECLISTH.0[spechash(nvp_rdev)];
    let vp = 'search: loop {
        for vp in vchain.iter() {
            if nvp_rdev != vp.v_rdev() || nvp.v_type.get() != vp.v_type.get() {
                continue;
            }
            // Alias, but not in use, so flush it out.
            if vp.v_usecount.get() == 0 {
                vgonel(vp, p);
                continue 'search;
            }
            let vpid = vp.v_id.get();
            if vget(vp, LK_EXCLUSIVE).is_err() {
                continue 'search;
            }
            if vpid != vp.v_id.get() {
                vput(vp);
                continue 'search;
            }
            break 'search Some(vp);
        }
        break None;
    };

    // Common case is actually in the if statement
    match vp {
        Some(vp) if vp.v_tag.get() == VT_NON && vp.v_type.get() == VBLK => {
            // This code is the uncommon case. It is called in case we found an alias that
            // was VT_NON && vtype of VBLK. This means we found a block device that was
            // created using bdevvp. An example of such a vnode is the root partition device
            // vnode created in ffs_mountroot.
            //
            // The vnodes created by bdevvp should not be aliased (why?).
            let _ = VOP_UNLOCK(vp);
            vclean(vp, 0, p);
            vp.v_op.set(nvp.v_op.get());
            vp.v_tag.set(nvp.v_tag.get());
            nvp.v_type.set(VNON);
            insmntque(vp, mp);
            Some(vp)
        }
        vp => {
            let Some(mem) = malloc(size_of::<Specinfo>(), M_VNODE, M_WAITOK) else {
                panic(format_args!("checkalias: out of memory"));
            };
            let si = mem.cast::<Specinfo>();
            // SAFETY: a fresh, suitably aligned allocation of `size_of::<Specinfo>()` bytes,
            // written once before anything else sees it.
            unsafe { si.as_ptr().write(Specinfo::new()) };
            // SAFETY: as above; `vgonel` frees it after unlinking the vnode.
            let si: &'static Specinfo = unsafe { si.as_ref() };
            si.si_rdev.set(nvp_rdev);
            si.si_hashchain.set(Some(vchain));
            si.si_mountpoint.set(None);
            si.si_lockf.set(ptr::null_mut());
            si.si_ci_bitmap.set(ptr::null_mut());
            nvp.v_un.set(VnodeUn::Specinfo(si));
            if nvp.v_type.get() == VCHR
                && cdevsw_d_flags_clone(major(nvp_rdev))
                && minor(nvp_rdev) >> CLONE_SHIFT == 0
            {
                if let Some(vp) = vp {
                    si.si_ci_bitmap.set(
                        vp.v_specinfo()
                            .map_or(ptr::null_mut(), |s| s.si_ci_bitmap.get()),
                    );
                } else {
                    let Some(map) = malloc(CLONE_MAPSZ, M_VNODE, M_WAITOK | M_ZERO) else {
                        panic(format_args!("checkalias: out of memory"));
                    };
                    si.si_ci_bitmap.set(map.as_ptr());
                }
            }
            // SAFETY: `nvp` is a new device vnode on no chain; vnodes never move.
            unsafe { vchain.insert_head(nvp) };
            if let Some(vp) = vp {
                nvp.v_flag.set(nvp.v_flag.get() | VALIASED);
                vp.v_flag.set(vp.v_flag.get() | VALIASED);
                vput(vp);
            }
            None
        }
    }
}

/// Grab a particular vnode from the free list, increment its reference count and lock it.
/// If the vnode lock bit is set, the vnode is being eliminated in vgone. In that case, we
/// cannot grab it, so the process is awakened when the transition is completed, and an error
/// code is returned to indicate that the vnode is no longer usable, possibly having been
/// changed to a new file system type.
pub fn vget(vp: &'static Vnode, flags: i32) -> Result<(), Errno> {
    // If the vnode is in the process of being cleaned out for another use, we wait for the
    // cleaning to finish and then return failure. Cleaning is determined by checking that the
    // VXLOCK flag is set.
    mtx_enter(&VNODE_MTX);
    if vp.v_lflag.get() & VXLOCK != 0 {
        if flags & LK_NOWAIT != 0 {
            mtx_leave(&VNODE_MTX);
            return Err(Errno::EBUSY);
        }

        vp.v_lflag.set(vp.v_lflag.get() | VXWANT);
        let _ = msleep_nsec(ptr::from_ref(vp), &VNODE_MTX, PINOD, "vget", INFSLP);
        mtx_leave(&VNODE_MTX);
        return Err(Errno::ENOENT);
    }
    mtx_leave(&VNODE_MTX);

    let s = splbio();
    let onfreelist = vp.v_bioflag.get() & VBIOONFREELIST != 0;
    if vp.v_usecount.get() == 0 && onfreelist {
        // SAFETY: a vnode marked `VBIOONFREELIST` is on the hold list while it has holds and
        // on the free list otherwise (`vputonfreelist`, `vhold`, `vdrop`), at splbio.
        unsafe {
            if vp.v_holdcnt.get() > 0 {
                VNODE_HOLD_LIST.0.remove(vp);
            } else {
                VNODE_FREE_LIST.0.remove(vp);
            }
        }
        vp.v_bioflag.set(vp.v_bioflag.get() & !VBIOONFREELIST);
    }
    splx(s);

    vp.v_usecount.set(vp.v_usecount.get() + 1);
    if flags & LK_TYPE_MASK != 0 {
        let error = vn_lock(vp, flags);
        if error.is_err() {
            vp.v_usecount.set(vp.v_usecount.get() - 1);
            if vp.v_usecount.get() == 0 && onfreelist {
                vputonfreelist(vp);
            }
        }
        return error;
    }

    Ok(())
}

/// Vnode reference.
pub fn vref(vp: &'static Vnode) {
    // KERNEL_ASSERT_LOCKED(): nothing without MULTIPROCESSOR.

    #[cfg(feature = "diagnostic")]
    {
        if vp.v_usecount.get() == 0 {
            panic(format_args!("vref used where vget required"));
        }
        if vp.v_type.get() == VNON {
            panic(format_args!("vref on a VNON vnode"));
        }
    }
    vp.v_usecount.set(vp.v_usecount.get() + 1);
}

/// `vputonfreelist(vp)`: puts an unused vnode on the hold list (if buffers reference it) or
/// the free list; a dead vnode (`VBAD`) goes to the head, to be recycled first.
pub fn vputonfreelist(vp: &'static Vnode) {
    let s = splbio();

    #[cfg(feature = "diagnostic")]
    {
        if vp.v_usecount.get() != 0 {
            panic(format_args!("Use count is not zero!"));
        }

        // If the hold count is still positive, one or many threads could still be waiting
        // on the vnode lock inside uvn_io().
        if vp.v_holdcnt.get() == 0 && vp.v_lockcount.get() != 0 {
            panic(format_args!("vputonfreelist: lock count is not zero"));
        }

        if vp.v_bioflag.get() & VBIOONFREELIST != 0 {
            vprint(Some("vnode already on free list: "), vp);
            panic(format_args!("vnode already on free list"));
        }
    }

    vp.v_bioflag
        .set((vp.v_bioflag.get() | VBIOONFREELIST) & !VBIOERROR);

    let lst = if vp.v_holdcnt.get() > 0 {
        &VNODE_HOLD_LIST
    } else {
        &VNODE_FREE_LIST
    };

    // SAFETY: the vnode is on no free list (`VBIOONFREELIST` was clear); vnodes never move.
    unsafe {
        if vp.v_type.get() == VBAD {
            lst.0.insert_head(vp);
        } else {
            lst.0.insert_tail(vp);
        }
    }

    splx(s);
}

/// `vput()`, just unlock and `vrele()`.
pub fn vput(vp: &'static Vnode) {
    let p = curproc();

    #[cfg(feature = "diagnostic")]
    if vp.v_usecount.get() == 0 {
        vprint(Some("vput: bad ref count"), vp);
        panic(format_args!("vput: ref cnt"));
    }
    vp.v_usecount.set(vp.v_usecount.get() - 1);
    kassert!(vp.v_usecount.get() > 0 || vp.v_uvcount.get() == 0);
    if vp.v_usecount.get() > 0 {
        let _ = VOP_UNLOCK(vp);
        return;
    }

    #[cfg(feature = "diagnostic")]
    if vp.v_writecount.get() != 0 {
        vprint(Some("vput: bad writecount"), vp);
        panic(format_args!("vput: v_writecount != 0"));
    }

    let _ = VOP_INACTIVE(vp, p);

    let s = splbio();
    if vp.v_usecount.get() == 0 && vp.v_bioflag.get() & VBIOONFREELIST == 0 {
        vputonfreelist(vp);
    }
    splx(s);
}

/// Vnode release - use for active VNODES. If count drops to zero, call inactive routine and
/// return to freelist. Returns `false` if it did not sleep.
pub fn vrele(vp: &'static Vnode) -> bool {
    let p = curproc();

    #[cfg(feature = "diagnostic")]
    if vp.v_usecount.get() == 0 {
        vprint(Some("vrele: bad ref count"), vp);
        panic(format_args!("vrele: ref cnt"));
    }
    vp.v_usecount.set(vp.v_usecount.get() - 1);
    if vp.v_usecount.get() > 0 {
        return false;
    }

    #[cfg(feature = "diagnostic")]
    if vp.v_writecount.get() != 0 {
        vprint(Some("vrele: bad writecount"), vp);
        panic(format_args!("vrele: v_writecount != 0"));
    }

    if vn_lock(vp, LK_EXCLUSIVE).is_err() {
        #[cfg(feature = "diagnostic")]
        vprint(Some("vrele: cannot lock"), vp);
        return true;
    }

    let _ = VOP_INACTIVE(vp, p);

    let s = splbio();
    if vp.v_usecount.get() == 0 && vp.v_bioflag.get() & VBIOONFREELIST == 0 {
        vputonfreelist(vp);
    }
    splx(s);
    true
}

/// Page or buffer structure gets a reference.
pub fn vhold(vp: &'static Vnode) {
    let s = splbio();

    // If it is on the freelist and the hold count is currently zero, move it to the hold
    // list.
    if vp.v_bioflag.get() & VBIOONFREELIST != 0
        && vp.v_holdcnt.get() == 0
        && vp.v_usecount.get() == 0
    {
        // SAFETY: an unheld vnode marked `VBIOONFREELIST` is on the free list, at splbio.
        unsafe {
            VNODE_FREE_LIST.0.remove(vp);
            VNODE_HOLD_LIST.0.insert_tail(vp);
        }
    }
    vp.v_holdcnt.set(vp.v_holdcnt.get() + 1);

    splx(s);
}

/// Lose interest in a vnode.
pub fn vdrop(vp: &'static Vnode) {
    let s = splbio();

    #[cfg(feature = "diagnostic")]
    if vp.v_holdcnt.get() == 0 {
        panic(format_args!("vdrop: zero holdcnt"));
    }

    vp.v_holdcnt.set(vp.v_holdcnt.get() - 1);

    // If it is on the holdlist and the hold count drops to zero, move it to the free list.
    if vp.v_bioflag.get() & VBIOONFREELIST != 0
        && vp.v_holdcnt.get() == 0
        && vp.v_usecount.get() == 0
    {
        // SAFETY: a held vnode marked `VBIOONFREELIST` was on the hold list, at splbio.
        unsafe {
            VNODE_HOLD_LIST.0.remove(vp);
            VNODE_FREE_LIST.0.insert_tail(vp);
        }
    }

    splx(s);
}

/// `vfs_mount_foreach_vnode(mp, func, arg)`: calls `func` on every vnode of `mp`, stopping
/// at the first error; restarts when a vnode left the mount under it.
pub fn vfs_mount_foreach_vnode(
    mp: &'static Mount,
    func: &mut dyn FnMut(&'static Vnode) -> Result<(), Errno>,
) -> Result<(), Errno> {
    'restart: loop {
        for vp in mp.mnt_vnodelist.iter() {
            if !vp.v_mount.get().is_some_and(|m| ptr::eq(m, mp)) {
                continue 'restart;
            }

            func(vp)?;
        }
        return Ok(());
    }
}

/// `struct vflush_args`.
struct VflushArgs {
    skipvp: Option<&'static Vnode>,
    busy: i32,
    flags: i32,
}

/// `vflush_vnode(vp, arg)`: one vnode of `vflush`.
fn vflush_vnode(vp: &'static Vnode, va: &mut VflushArgs) -> Result<(), Errno> {
    let p = curproc();

    if va.skipvp.is_some_and(|skip| ptr::eq(skip, vp)) {
        return Ok(());
    }

    if va.flags & SKIPSYSTEM != 0 && vp.v_flag.get() & VSYSTEM != 0 {
        return Ok(());
    }

    // If WRITECLOSE is set, only flush out regular file vnodes open for writing.
    if va.flags & WRITECLOSE != 0 && (vp.v_writecount.get() == 0 || vp.v_type.get() != VREG) {
        return Ok(());
    }

    // With v_usecount == 0, all we need to do is clear out the vnode data structures and we
    // are done.
    if vp.v_usecount.get() == 0 {
        vgonel(vp, p);
        return Ok(());
    }

    // If FORCECLOSE is set, forcibly close the vnode. For block or character devices,
    // revert to an anonymous device. For all other files, just kill them.
    if va.flags & FORCECLOSE != 0 {
        if vp.v_type.get() != VBLK && vp.v_type.get() != VCHR {
            vgonel(vp, p);
        } else {
            vclean(vp, 0, p);
            vp.v_op.set(Some(&SPEC_VOPS));
            insmntque(vp, None);
        }
        return Ok(());
    }

    // If set, this is allowed to ignore vnodes which don't have changes pending to disk.
    // XXX Might be nice to check per-fs "inode" flags, but generally the filesystem is sync'd
    // already, right? (LIST_EMPTY(&vp->v_dirtyblkhd): no buffers before vfs_bio.c.)
    let s = splbio();
    let empty = va.flags & IGNORECLEAN != 0;
    splx(s);

    if empty {
        return Ok(());
    }

    // DEBUG_SYSCTL busyprt: not configured.
    va.busy += 1;
    Ok(())
}

/// Remove any vnodes in the vnode table belonging to mount point `mp`.
///
/// If `MNT_NOFORCE` is specified, there should not be any active ones, return error if any
/// are found (nb: this is a user error, not a system error). If `MNT_FORCE` is specified,
/// detach any active vnodes that are found.
pub fn vflush(mp: &'static Mount, skipvp: Option<&'static Vnode>, flags: i32) -> Result<(), Errno> {
    let mut va = VflushArgs {
        skipvp,
        busy: 0,
        flags,
    };

    let _ = vfs_mount_foreach_vnode(mp, &mut |vp| vflush_vnode(vp, &mut va));

    if va.busy != 0 {
        return Err(Errno::EBUSY);
    }
    Ok(())
}

/// Disassociate the underlying file system from a vnode.
pub fn vclean(vp: &'static Vnode, flags: i32, p: Option<&Proc>) {
    let mut do_wakeup = false;

    // Check to see if the vnode is in use. If so we have to reference it before we clean it
    // out so that its count cannot fall to zero and generate a race against ourselves to
    // recycle it.
    let active = vp.v_usecount.get();
    if active != 0 {
        vp.v_usecount.set(active + 1);
    }

    // Prevent the vnode from being recycled or brought into use while we clean it out.
    mtx_enter(&VNODE_MTX);
    if vp.v_lflag.get() & VXLOCK != 0 {
        panic(format_args!("vclean: deadlock"));
    }
    vp.v_lflag.set(vp.v_lflag.get() | VXLOCK);

    if vp.v_lockcount.get() > 0 {
        // Ensure that any thread currently waiting on the same lock has observed that the
        // vnode is about to be exclusively locked before continuing.
        let _ = msleep_nsec(
            ptr::from_ref(&vp.v_lockcount),
            &VNODE_MTX,
            PINOD,
            "vop_lock",
            INFSLP,
        );
        kassert!(vp.v_lockcount.get() == 0);
    }
    mtx_leave(&VNODE_MTX);

    // Even if the count is zero, the VOP_INACTIVE routine may still have the object locked
    // while it cleans it out. The VOP_LOCK ensures that the VOP_INACTIVE routine is done with
    // its work. For active vnodes, it ensures that no other activity can occur while the
    // underlying object is being cleaned out.
    let _ = VOP_LOCK(vp, LK_EXCLUSIVE | LK_DRAIN);

    // Clean out any VM data associated with the vnode.
    uvm_vnp_terminate(vp);
    // Clean out any buffers associated with the vnode.
    if flags & DOCLOSE != 0
        && let Err(error) = vinvalbuf(vp, V_SAVE, NOCRED, p, 0, INFSLP)
    {
        kprintf!(
            "vclean: failed to flush buffers, error {}; discarding dirty buffers",
            error as i32
        );
        if let Some(mp) = vp.v_mount.get() {
            let (name, len) = mp.mntonname();
            kprintf!("; mounted on: {}", crate::kern::subr_prf::Str(&name[..len]));
        }
        kprintf!("\n");
        let _ = vinvalbuf(vp, 0, NOCRED, p, 0, INFSLP);
    }
    // If purging an active vnode, it must be closed and deactivated before being reclaimed.
    // Note that the VOP_INACTIVE will unlock the vnode
    if active != 0 {
        if flags & DOCLOSE != 0 {
            let _ = VOP_CLOSE(vp, FNONBLOCK, NOCRED, p);
        }
        let _ = VOP_INACTIVE(vp, p);
    } else {
        // Any other processes trying to obtain this lock must first wait for VXLOCK to
        // clear, then call the new lock operation.
        let _ = VOP_UNLOCK(vp);
    }

    // Reclaim the vnode.
    if VOP_RECLAIM(vp, p).is_err() {
        panic(format_args!("vclean: cannot reclaim"));
    }
    if active != 0 {
        vp.v_usecount.set(vp.v_usecount.get() - 1);
        if vp.v_usecount.get() == 0 {
            let s = splbio();
            if vp.v_holdcnt.get() > 0 {
                panic(format_args!("vclean: not clean"));
            }
            vputonfreelist(vp);
            splx(s);
        }
    }
    cache_purge(vp);

    // Done with purge, notify sleepers of the grim news.
    vp.v_op.set(Some(&DEAD_VOPS));
    // VN_KNOTE(vp, NOTE_REVOKE): no knotes before kern_event.c.
    vp.v_tag.set(VT_NON);
    // VFSLCKDEBUG: not configured.
    mtx_enter(&VNODE_MTX);
    vp.v_lflag.set(vp.v_lflag.get() & !VXLOCK);
    if vp.v_lflag.get() & VXWANT != 0 {
        vp.v_lflag.set(vp.v_lflag.get() & !VXWANT);
        do_wakeup = true;
    }
    mtx_leave(&VNODE_MTX);
    if do_wakeup {
        wakeup(ptr::from_ref(vp));
    }
}

/// `uvm_vnp_terminate(vp)`: the vnode pager (`uvm_vnode.c`) is not ported, so no vnode has
/// VM data to clean out.
fn uvm_vnp_terminate(vp: &Vnode) {
    if !vp.v_uvm.get().is_null() {
        let _ = unported!("uvm_vnp_terminate (uvm_vnode.c)");
    }
}

/// Recycle an unused vnode to the front of the free list.
pub fn vrecycle(vp: &'static Vnode, p: Option<&Proc>) -> bool {
    if vp.v_usecount.get() == 0 {
        vgonel(vp, p);
        return true;
    }
    false
}

/// Eliminate all activity associated with a vnode in preparation for reuse.
pub fn vgone(vp: &'static Vnode) {
    let p = curproc();
    vgonel(vp, p);
}

/// vgone, with struct proc.
pub fn vgonel(vp: &'static Vnode, p: Option<&Proc>) {
    kassert!(vp.v_uvcount.get() == 0);

    // If a vgone (or vclean) is already in progress, wait until it is done and return.
    mtx_enter(&VNODE_MTX);
    if vp.v_lflag.get() & VXLOCK != 0 {
        vp.v_lflag.set(vp.v_lflag.get() | VXWANT);
        let _ = msleep_nsec(ptr::from_ref(vp), &VNODE_MTX, PINOD, "vgone", INFSLP);
        mtx_leave(&VNODE_MTX);
        return;
    }
    mtx_leave(&VNODE_MTX);

    // Clean out the filesystem specific data.
    vclean(vp, DOCLOSE, p);
    // Delete from old mount point vnode list, if on one.
    if vp.v_mount.get().is_some() {
        insmntque(vp, None);
    }
    // If special device, remove it from special device alias list if it is on one.
    if (vp.v_type.get() == VBLK || vp.v_type.get() == VCHR)
        && let Some(si) = vp.v_specinfo()
    {
        if vp.v_flag.get() & VALIASED == 0
            && vp.v_type.get() == VCHR
            && cdevsw_d_flags_clone(major(si.si_rdev.get()))
            && minor(si.si_rdev.get()) >> CLONE_SHIFT == 0
            && let Some(map) = NonNull::new(si.si_ci_bitmap.get())
        {
            free(map, M_VNODE, CLONE_MAPSZ);
        }
        if let Some(chain) = si.si_hashchain.get() {
            // SAFETY: a device vnode with a specinfo is on its hash chain (`checkalias`).
            unsafe { chain.remove(vp) };
            if vp.v_flag.get() & VALIASED != 0 {
                let mut vx: Option<&'static Vnode> = None;
                let mut more = false;
                for vq in chain.iter() {
                    if vq.v_rdev() != si.si_rdev.get() || vq.v_type.get() != vp.v_type.get() {
                        continue;
                    }
                    if vx.is_some() {
                        more = true;
                        break;
                    }
                    vx = Some(vq);
                }
                let Some(vx) = vx else {
                    panic(format_args!("missing alias"));
                };
                if !more {
                    vx.v_flag.set(vx.v_flag.get() & !VALIASED);
                }
                vp.v_flag.set(vp.v_flag.get() & !VALIASED);
            }
        }
        if !si.si_lockf.get().is_null() {
            let _ = unported!("lf_purgelocks (vfs_lockf.c)");
        }
        vp.v_un.set(VnodeUn::None);
        free(NonNull::from(si).cast(), M_VNODE, size_of::<Specinfo>());
    }
    // If it is on the freelist and not already at the head, move it to the head of the list.
    vp.v_type.set(VBAD);

    // Move onto the free list, unless we were called from getnewvnode and we're not on any
    // free list
    let s = splbio();
    if vp.v_usecount.get() == 0 && vp.v_bioflag.get() & VBIOONFREELIST != 0 {
        if vp.v_holdcnt.get() > 0 {
            panic(format_args!("vgonel: not clean"));
        }

        if !VNODE_FREE_LIST.0.first().is_some_and(|f| ptr::eq(f, vp)) {
            // SAFETY: an unheld, unused vnode marked `VBIOONFREELIST` is on the free list.
            unsafe {
                VNODE_FREE_LIST.0.remove(vp);
                VNODE_FREE_LIST.0.insert_head(vp);
            }
        }
    }
    splx(s);
}

/// Lookup a vnode by device number.
pub fn vfinddev(dev: Dev, type_: Vtype) -> Option<&'static Vnode> {
    SPECLISTH.0[spechash(dev)]
        .iter()
        .find(|vp| dev == vp.v_rdev() && type_ == vp.v_type.get())
}

/// Revoke all the vnodes corresponding to the specified minor number range (endpoints
/// inclusive) of the specified major.
pub fn vdevgone(maj: u32, minl: u32, minh: u32, type_: Vtype) {
    for mn in minl..=minh {
        if let Some(vp) = vfinddev(makedev(maj, mn), type_) {
            let _ = VOP_REVOKE(vp, REVOKEALL);
        }
    }
}

/// Calculate the total number of references to a special device.
pub fn vcount(vp: &'static Vnode) -> i32 {
    'restart: loop {
        if vp.v_flag.get() & VALIASED == 0 {
            return vp.v_usecount.get() as i32;
        }
        let mut count = 0;
        let Some(chain) = vp.v_specinfo().and_then(|si| si.si_hashchain.get()) else {
            return vp.v_usecount.get() as i32;
        };
        for vq in chain.iter() {
            if vq.v_rdev() != vp.v_rdev() || vq.v_type.get() != vp.v_type.get() {
                continue;
            }
            // Alias, but not in use, so flush it out.
            if vq.v_usecount.get() == 0 && !ptr::eq(vq, vp) {
                vgone(vq);
                continue 'restart;
            }
            count += vq.v_usecount.get() as i32;
        }
        return count;
    }
}

/// Print out a description of a vnode.
#[cfg(any(feature = "debug", feature = "diagnostic"))]
pub fn vprint(label: Option<&str>, vp: &'static Vnode) {
    use crate::sys::vnode::{VBIOONSYNCLIST, VTEXT, VTYPE_NAMES};

    if let Some(label) = label {
        kprintf!("{}: ", label);
    }
    kprintf!(
        "{:p}, type {}, use {}, write {}, hold {},",
        vp,
        VTYPE_NAMES[vp.v_type.get() as usize],
        vp.v_usecount.get(),
        vp.v_writecount.get(),
        vp.v_holdcnt.get()
    );
    let flags: [(bool, &str); 9] = [
        (vp.v_flag.get() & VROOT != 0, "VROOT"),
        (vp.v_flag.get() & VTEXT != 0, "VTEXT"),
        (vp.v_flag.get() & VSYSTEM != 0, "VSYSTEM"),
        (vp.v_lflag.get() & VXLOCK != 0, "VXLOCK"),
        (vp.v_lflag.get() & VXWANT != 0, "VXWANT"),
        (vp.v_bioflag.get() & VBIOWAIT != 0, "VBIOWAIT"),
        (vp.v_bioflag.get() & VBIOONFREELIST != 0, "VBIOONFREELIST"),
        (vp.v_bioflag.get() & VBIOONSYNCLIST != 0, "VBIOONSYNCLIST"),
        (vp.v_flag.get() & VALIASED != 0, "VALIASED"),
    ];
    let mut first = true;
    for (set, name) in flags {
        if set {
            kprintf!("{}{}", if first { " flags (" } else { "|" }, name);
            first = false;
        }
    }
    if !first {
        kprintf!(")");
    }
    if vp.v_data.get().is_null() {
        kprintf!("\n");
    } else {
        kprintf!("\n\t");
        let _ = crate::kern::vfs_vops::VOP_PRINT(vp);
    }
}

/// List all of the locked vnodes in the system. Called when debugging the kernel.
#[cfg(feature = "debug")]
pub fn printlockedvnodes() {
    kprintf!("Locked vnodes\n");

    for mp in MOUNTLIST.0.iter() {
        if vfs_busy(mp, VB_READ | VB_NOWAIT).is_err() {
            continue;
        }
        for vp in mp.mnt_vnodelist.iter() {
            if VOP_ISLOCKED(vp) != 0 {
                vprint(None, vp);
            }
        }
        vfs_unbusy(mp);
    }
}

/// `struct vfsconf` as `vfs.generic.conf` copies it out: the operations pointer cleared, the
/// hole after `vfc_flags` named.
#[repr(C)]
struct VfsconfAbi {
    vfc_vfsops: u64,
    vfc_name: [u8; MFSNAMELEN],
    vfc_typenum: i32,
    vfc_refcount: u32,
    vfc_flags: i32,
    _pad: u32,
    vfc_datasize: u64,
}

// SAFETY: `#[repr(C)]` integers and bytes with the one hole named; no padding.
unsafe impl crate::sys::sysctl::SysctlPlain for VfsconfAbi {}

/// Top level filesystem related information gathering.
pub fn vfs_sysctl(
    name: &[i32],
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    newlen: usize,
    p: &Proc,
) -> Result<(), Errno> {
    use crate::sys::sysctl::SysctlPlain;

    // all sysctl names at this level are at least name and field
    if name.len() < 2 {
        return Err(Errno::ENOTDIR); // overloaded
    }

    if name[0] != VFS_GENERIC {
        let Some(vfsp) = vfs_bytypenum(name[0]) else {
            return Err(Errno::EOPNOTSUPP);
        };
        let Some(f) = vfsp.vfc_vfsops.vfs_sysctl else {
            return Err(Errno::EOPNOTSUPP);
        };

        return f(&name[1..], oldp, oldlenp, newp, newlen, p);
    }

    match name[1] {
        VFS_MAXTYPENUM => sysctl_rdint(oldp, oldlenp, newp, MAXVFSCONF.load(Ordering::Relaxed)),
        VFS_CONF => {
            if name.len() < 3 {
                return Err(Errno::ENOTDIR); // overloaded
            }

            let Some(vfsp) = vfs_bytypenum(name[2]) else {
                return Err(Errno::EOPNOTSUPP);
            };

            // Make a copy, clear out kernel pointers
            let tmpvfsp = VfsconfAbi {
                vfc_vfsops: 0,
                vfc_name: vfsp.vfc_name,
                vfc_typenum: vfsp.vfc_typenum,
                vfc_refcount: vfsp.vfc_refcount.load(Ordering::Relaxed),
                vfc_flags: vfsp.vfc_flags,
                _pad: 0,
                vfc_datasize: vfsp.vfc_datasize as u64,
            };

            sysctl_rdstruct(oldp, oldlenp, newp, tmpvfsp.as_bytes())
        }
        // buffer cache statistics
        VFS_BCACHESTAT => Err(unported!("vfs.generic.bcachestat: bcstats (vfs_bio.c)")),
        _ => Err(Errno::EOPNOTSUPP),
    }
}

/// Check to see if a filesystem is mounted on a block device.
pub fn vfs_mountedon(vp: &'static Vnode) -> Result<(), Errno> {
    if vp.v_specmountpoint().is_some() {
        return Err(Errno::EBUSY);
    }
    if vp.v_flag.get() & VALIASED != 0
        && let Some(chain) = vp.v_specinfo().and_then(|si| si.si_hashchain.get())
    {
        for vq in chain.iter() {
            if vq.v_rdev() != vp.v_rdev() || vq.v_type.get() != vp.v_type.get() {
                continue;
            }
            if vq.v_specmountpoint().is_some() {
                return Err(Errno::EBUSY);
            }
        }
    }
    Ok(())
}

// NFSSERVER (vfs_hang_addrlist, vfs_free_netcred, vfs_free_addrlist): not configured.

/// `vfs_export(mp, nep, argp)`: process mount export info. `nep` is a `struct netexport *`
/// and `argp` a `struct export_args *`; without `NFSSERVER` there is nothing to export.
pub fn vfs_export(
    _mp: &'static Mount,
    _nep: *mut c_void,
    _argp: *const c_void,
) -> Result<(), Errno> {
    Err(Errno::ENOTSUP)
}

/// `vfs_export_lookup(mp, nep, nam)`: lookup host in fs export list; NULL without
/// `NFSSERVER`.
pub fn vfs_export_lookup(
    _mp: &'static Mount,
    _nep: *mut c_void,
    _nam: *const c_void,
) -> *mut c_void {
    ptr::null_mut()
}

/// Do the usual access checking. `file_mode`, `uid` and `gid` are from the vnode in question,
/// while `acc_mode` and `cred` are from the `VOP_ACCESS` parameter list.
pub fn vaccess(
    type_: Vtype,
    file_mode: Mode,
    uid: Uid,
    gid: Gid,
    acc_mode: i32,
    cred: &Ucred,
) -> Result<(), Errno> {
    // User id 0 always gets read/write access.
    if cred.cr_uid.get() == 0 {
        // For VEXEC, at least one of the execute bits must be set.
        if acc_mode & VEXEC != 0 && type_ != VDIR && file_mode & (S_IXUSR | S_IXGRP | S_IXOTH) == 0
        {
            return Err(Errno::EACCES);
        }
        return Ok(());
    }

    let mut mask: Mode = 0;
    let check = |mask: Mode| {
        if file_mode & mask == mask {
            Ok(())
        } else {
            Err(Errno::EACCES)
        }
    };

    // Otherwise, check the owner.
    if cred.cr_uid.get() == uid {
        if acc_mode & VEXEC != 0 {
            mask |= S_IXUSR;
        }
        if acc_mode & VREAD != 0 {
            mask |= S_IRUSR;
        }
        if acc_mode & VWRITE != 0 {
            mask |= S_IWUSR;
        }
        return check(mask);
    }

    // Otherwise, check the groups.
    if groupmember(gid, cred) {
        if acc_mode & VEXEC != 0 {
            mask |= S_IXGRP;
        }
        if acc_mode & VREAD != 0 {
            mask |= S_IRGRP;
        }
        if acc_mode & VWRITE != 0 {
            mask |= S_IWGRP;
        }
        return check(mask);
    }

    // Otherwise, check everyone else.
    if acc_mode & VEXEC != 0 {
        mask |= S_IXOTH;
    }
    if acc_mode & VREAD != 0 {
        mask |= S_IROTH;
    }
    if acc_mode & VWRITE != 0 {
        mask |= S_IWOTH;
    }
    check(mask)
}

/// `vnoperm(vp)`: whether permission checks are off for the vnode's file system
/// (`MNT_NOPERM`); never for a file system root or a vnode without a mount.
pub fn vnoperm(vp: &'static Vnode) -> bool {
    if vp.v_flag.get() & VROOT != 0 {
        return false;
    }
    match vp.v_mount.get() {
        None => false,
        Some(mp) => mp.mnt_flag.get() & MNT_NOPERM != 0,
    }
}

/// `vfs_stall_lock`.
pub static VFS_STALL_LOCK: Rwlock = Rwlock::new("vfs_stall");
/// `vfs_stalling`.
pub static VFS_STALLING: AtomicU32 = AtomicU32::new(0);

/// `vfs_stall(p, stall)`: syncs and busies every file system (suspend), or releases them.
pub fn vfs_stall(p: &Proc, stall: bool) -> Result<(), Errno> {
    let mut allerror = Ok(());

    if stall {
        VFS_STALLING.fetch_add(1, Ordering::SeqCst);
        rw_enter_write(&VFS_STALL_LOCK);
    }

    // The loop variable mp is protected by vfs_busy() so that it cannot be unmounted while
    // VFS_SYNC() sleeps. Traverse forward to keep the lock order consistent with dounmount().
    for mp in MOUNTLIST.0.iter() {
        let (name, len) = mp.mntonname();
        let name = crate::kern::subr_prf::Str(&name[..len]);
        if stall {
            if let Err(error) = vfs_busy(mp, VB_WRITE | VB_WAIT | VB_DUPOK) {
                kprintf!("{}: busy\n", name);
                allerror = Err(error);
                continue;
            }
            uvm_vnp_sync(mp);
            if let Err(error) = VFS_SYNC(mp, MNT_WAIT, 1, p.p_ucred.get(), p) {
                kprintf!("{}: failed to sync\n", name);
                vfs_unbusy(mp);
                allerror = Err(error);
                continue;
            }
            mp.mnt_flag.set(mp.mnt_flag.get() | MNT_STALLED);
        } else if mp.mnt_flag.get() & MNT_STALLED != 0 {
            vfs_unbusy(mp);
            mp.mnt_flag.set(mp.mnt_flag.get() & !MNT_STALLED);
        }
    }

    if !stall {
        rw_exit_write(&VFS_STALL_LOCK);
        VFS_STALLING.fetch_sub(1, Ordering::SeqCst);
    }

    allerror
}

/// `uvm_vnp_sync(mp)`: the vnode pager (`uvm_vnode.c`) is not ported.
pub fn uvm_vnp_sync(_mp: &'static Mount) {
    let _ = unported!("uvm_vnp_sync (uvm_vnode.c)");
}

/// `uvm_vnp_uncache(vp)`: drops the vnode's cached pages, true when nothing maps it. The
/// vnode pager (`uvm_vnode.c`) is not ported, so no vnode has pages to drop.
pub fn uvm_vnp_uncache(_vp: &'static Vnode) -> bool {
    let _ = unported!("uvm_vnp_uncache (uvm_vnode.c)");
    true
}

/// `vfs_stall_barrier()`: waits while the file systems are stalled.
pub fn vfs_stall_barrier() {
    if VFS_STALLING.load(Ordering::Relaxed) != 0 {
        rw_enter_read(&VFS_STALL_LOCK);
        rw_exit_read(&VFS_STALL_LOCK);
    }
}

/// Unmount all file systems. We traverse the list in reverse order under the assumption that
/// doing so will avoid needing to worry about dependencies.
pub fn vfs_unmountall() {
    let mut again = true;

    loop {
        let mut allerror = false;
        for mp in MOUNTLIST.0.iter_reverse() {
            if vfs_busy(mp, VB_WRITE | VB_NOWAIT).is_err() {
                continue;
            }
            // XXX Here is a race, the next pointer is not locked.
            let Some(p) = curproc() else {
                vfs_unbusy(mp);
                continue;
            };
            if let Err(error) = dounmount(mp, crate::sys::mount::MNT_FORCE, p) {
                let (name, len) = mp.mntonname();
                kprintf!(
                    "unmount of {} failed with error {}\n",
                    crate::kern::subr_prf::Str(&name[..len]),
                    error as i32
                );
                allerror = true;
            }
        }

        if allerror {
            kprintf!("WARNING: some file systems would not unmount\n");
            if again {
                kprintf!("retrying\n");
                again = false;
                continue;
            }
        }
        break;
    }
}

/// Sync and unmount file systems before shutting down.
pub fn vfs_shutdown(p: &Proc) {
    // ACCOUNTING: not configured.

    kprintf!("syncing disks...");

    if !panicstr() {
        // Sync before unmount, in case we hang on something.
        let mut retval = [0; 2];
        let _ = sys_sync(p, &[0; 6], &mut retval);
        vfs_unmountall();
    }

    // NSOFTRAID: not configured.

    if vfs_syncwait(p, true) != 0 {
        kprintf!(" giving up\n");
    } else {
        kprintf!(" done\n");
    }
}

/// Perform sync() operation and wait for buffers to flush; returns the buffers still busy.
pub fn vfs_syncwait(p: &Proc, verbose: bool) -> i32 {
    let mut retval = [0; 2];
    let _ = sys_sync(p, &[0; 6], &mut retval);

    // Wait for sync to finish: the scan of bufhead for busy and delayed-write buffers needs
    // struct buf (vfs_bio.c); without a buffer cache nothing is busy.
    let _ = unported!("vfs_syncwait: bufhead (vfs_bio.c)");
    let _ = verbose;
    0
}

/// Wait for all outstanding I/Os to complete.
///
/// Manipulates `v_numoutput`. Must be called at `splbio()`.
pub fn vwaitforio(
    vp: &'static Vnode,
    slpflag: i32,
    wmesg: &'static str,
    timeo: u64,
) -> Result<(), Errno> {
    splassert(IPL_BIO, "vwaitforio");

    while vp.v_numoutput.get() != 0 {
        vp.v_bioflag.set(vp.v_bioflag.get() | VBIOWAIT);
        tsleep_nsec(
            ptr::from_ref(&vp.v_numoutput),
            slpflag | (PRIBIO + 1),
            wmesg,
            timeo,
        )?;
    }

    Ok(())
}

/// Update outstanding I/O count and do wakeup if requested.
///
/// Manipulates `v_numoutput`. Must be called at `splbio()`.
pub fn vwakeup(vp: Option<&'static Vnode>) {
    splassert(IPL_BIO, "vwakeup");

    if let Some(vp) = vp {
        if vp.v_numoutput.get() == 0 {
            panic(format_args!("vwakeup: neg numoutput"));
        }
        vp.v_numoutput.set(vp.v_numoutput.get() - 1);
        if vp.v_bioflag.get() & VBIOWAIT != 0 && vp.v_numoutput.get() == 0 {
            vp.v_bioflag.set(vp.v_bioflag.get() & !VBIOWAIT);
            wakeup(ptr::from_ref(&vp.v_numoutput));
        }
    }
}

/// Flush out and invalidate all buffers associated with a vnode. Called with the underlying
/// object locked.
pub fn vinvalbuf(
    vp: &'static Vnode,
    flags: i32,
    cred: *const Ucred,
    p: Option<&Proc>,
    _slpflag: i32,
    _slptimeo: u64,
) -> Result<(), Errno> {
    // VFSLCKDEBUG: not configured.

    if flags & V_SAVE != 0 {
        let s = splbio();
        let _ = vwaitforio(vp, 0, "vinvalbuf", INFSLP);
        // !LIST_EMPTY(&vp->v_dirtyblkhd) -> VOP_FSYNC(vp, cred, MNT_WAIT, p): no buffer
        // lists before vfs_bio.c.
        let _ = (cred, p);
        splx(s);
    }
    // The loop that invalidates the clean and dirty buffers needs struct buf (vfs_bio.c).
    let _ = unported!("vinvalbuf: the vnode's buffer lists (vfs_bio.c)");
    Ok(())
}

/// `vflushbuf(vp, sync)`: writes the vnode's dirty buffers out.
pub fn vflushbuf(vp: &'static Vnode, sync: bool) {
    // The dirty list walk needs struct buf (vfs_bio.c).
    let _ = unported!("vflushbuf: v_dirtyblkhd (vfs_bio.c)");
    if !sync {
        return;
    }
    let s = splbio();
    let _ = vwaitforio(vp, 0, "vflushbuf", INFSLP);
    splx(s);
}

/// Associate a buffer with a vnode (`struct buf`, `vfs_bio.c`).
pub fn bgetvp(_vp: &'static Vnode, _bp: *mut c_void) {
    let _ = unported!("bgetvp: struct buf (vfs_bio.c)");
}

/// Disassociate a buffer from a vnode (`struct buf`, `vfs_bio.c`).
pub fn brelvp(_bp: *mut c_void) {
    let _ = unported!("brelvp: struct buf (vfs_bio.c)");
}

/// Replaces the current vnode associated with the buffer, if any, with a new vnode
/// (`struct buf`, `vfs_bio.c`).
pub fn buf_replacevnode(_bp: *mut c_void, _newvp: &'static Vnode) {
    let _ = unported!("buf_replacevnode: struct buf (vfs_bio.c)");
}

/// Used to assign buffers to the appropriate clean or dirty list on the vnode and to add
/// newly dirty vnodes to the appropriate filesystem syncer list (`struct buf`, `vfs_bio.c`;
/// `vn_syncer_add_to_worklist`, `vfs_sync.c`).
pub fn reassignbuf(_bp: *mut c_void) {
    let _ = unported!("reassignbuf: struct buf (vfs_bio.c), the syncer (vfs_sync.c)");
}

// DDB: vfs_buf_print, vfs_vnode_print, vfs_mount_print wait for the ddb command loop.

/// `copy_statfs_info(sbp, mp)`: fills the mount-wide members of a file system's `statfs`
/// answer from the mount.
pub fn copy_statfs_info(sbp: &mut Statfs, mp: &Mount) {
    let name = mp.vfc().name();
    sbp.f_fstypename = [0; MFSNAMELEN];
    sbp.f_fstypename[..name.len()].copy_from_slice(name);

    // sbp == &mp->mnt_stat never happens here (see the module's deviations).
    let mbp = mp.mnt_stat.get();
    sbp.f_fsid = mbp.f_fsid;
    sbp.f_owner = mbp.f_owner;
    sbp.f_flags = mbp.f_flags;
    sbp.f_syncwrites = mbp.f_syncwrites;
    sbp.f_asyncwrites = mbp.f_asyncwrites;
    sbp.f_syncreads = mbp.f_syncreads;
    sbp.f_asyncreads = mbp.f_asyncreads;
    sbp.f_namemax = mbp.f_namemax;
    sbp.f_mntonname = mbp.f_mntonname;
    sbp.f_mntfromname = mbp.f_mntfromname;
    sbp.f_mntfromspec = mbp.f_mntfromspec;
    sbp.mount_info = mbp.mount_info;
}

#[cfg(test)]
pub(crate) mod tests;
