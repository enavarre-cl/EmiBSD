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

/* <CODE> */
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
//! - `bufinsvn`/`bufremvn` keep the buffer's `b_onvnbufs` flag in step with its vnode list
//!   (the C's `NOLIST`, `sys/buf.rs`); `vinvalbuf` panics if it finds dirty buffers with no
//!   thread to `VOP_FSYNC` them (the C always has `curproc`).
//! - The device switch (`cdevsw[].d_type`/`d_flags`, `nblkdev`) is each architecture's
//!   `conf.c`, through `machine::conf`.
//! - `copy_statfs_info` never receives the mount's own `mnt_stat` (the callers pass a copy,
//!   see `sys/mount.rs`), so the C's early return for that case is not needed; the copy has
//!   the same values, so the result is the same.
//! - `NFSSERVER` is feature `nfsserver`: `vfs_hang_addrlist`, `vfs_free_netcred`,
//!   `vfs_free_addrlist` and `vfs_vfsinit`'s `rn_init` exist only with it, and without it
//!   `vfs_export` answers `ENOTSUP` and `vfs_export_lookup` nothing, as in C. `vfs_export`
//!   takes the file system's `Netexport` and the kernel copy of `struct export_args`
//!   (`ExportArgs`, whose two addresses are user addresses for `copyin`) and returns the
//!   `Result`; `vfs_export_lookup` returns `Option<&'static Netcred>` and takes the
//!   client's address mbuf as an `Option`.
//! - `vfs_hang_addrlist` allocates `KEYLEN_PAD` bytes more than `sizeof(struct netcred) +
//!   ex_addrlen + ex_masklen` behind each entry (`netc_len` counts them), so that the radix
//!   code, which reads keys for `max_keylen` bytes whatever their length byte says, never
//!   leaves the allocation; an address shorter than two bytes (no family byte) is `EINVAL`.
//!   A failed `malloc(M_WAITOK)` panics, as `vfs_mount_alloc`'s does.
//! - `vprint` is compiled under feature `diagnostic` or `debug`, `printlockedvnodes` under
//!   `debug`, as in C. The DDB printers (`vfs_buf_print`, `vfs_vnode_print`,
//!   `vfs_mount_print`) wait for the ddb command loop (`db_command.c`).

use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicI32, AtomicI64, AtomicU32, Ordering};

use crate::kassert;
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_malloc::{free, malloc};
#[cfg(feature = "nfsserver")]
use crate::kern::kern_prot::crfromxucred;
use crate::kern::kern_prot::groupmember;
use crate::kern::kern_rwlock::{
    rw_enter, rw_enter_read, rw_enter_write, rw_exit, rw_exit_read, rw_exit_write, rw_init_flags,
    rw_status,
};
use crate::kern::kern_synch::{
    msleep_nsec, refcnt_init, refcnt_rele, refcnt_take, tsleep_nsec, wakeup,
};
use crate::kern::kern_sysctl::{sysctl_rdint, sysctl_rdstruct};
use crate::kern::sched_bsd::r#yield;
use crate::kern::spec_vnops::{SPEC_VOPS, SPECLISTH};
use crate::kern::subr_pool::{pool_get, pool_init};
use crate::kern::subr_prf::{panic, panicstr, tablefull};
use crate::kern::vfs_bio::{BCSTATS, BUFHEAD, bawrite, brelse, bufcache_take, bwrite};
use crate::kern::vfs_biomem::{buf_acquire, buf_acquire_nomap};
use crate::kern::vfs_cache::{cache_purge, cache_tree_init};
use crate::kern::vfs_init::{MAXVFSCONF, vfs_byname, vfs_bytypenum};
use crate::kern::vfs_lockf::lf_purgelocks;
use crate::kern::vfs_sync::{SYNCDELAY, vn_initialize_syncerd, vn_syncer_add_to_worklist};
use crate::kern::vfs_syscalls::{dounmount, sys_sync};
use crate::kern::vfs_vnops::vn_lock;
use crate::kern::vfs_vops::{
    VOP_BWRITE, VOP_CLOSE, VOP_FSYNC, VOP_INACTIVE, VOP_ISLOCKED, VOP_LOCK, VOP_RECLAIM,
    VOP_REVOKE, VOP_UNLOCK,
};
use crate::kprintf;
use crate::machine::conf::{cdevsw, nblkdev};
#[cfg(feature = "nfsserver")]
use crate::machine::copy::copyin;
use crate::machine::cpu::{curproc, delay};
use crate::machine::intr::{IPL_BIO, IPL_NONE, splassert, splbio, splx};
use crate::miscfs::deadfs::dead_vnops::DEAD_VOPS;
#[cfg(feature = "nfsserver")]
use crate::net::radix::{
    RadixNode, RadixNodeHead, rn_addroute, rn_delete, rn_init, rn_inithead, rn_match, rn_walktree,
};
#[cfg(feature = "nfsserver")]
use crate::netinet::in_::SockaddrIn;
use crate::sys::buf::{B_BUSY, B_DELWRI, B_DONE, B_INVAL, B_READ, B_WANTED, Buf};
use crate::sys::conf::{D_CLONE, D_TTY};
use crate::sys::errno::Errno;
use crate::sys::event::NOTE_REVOKE;
use crate::sys::fcntl::FNONBLOCK;
use crate::sys::lock::{LK_DRAIN, LK_EXCLUSIVE, LK_NOWAIT, LK_TYPE_MASK};
use crate::sys::malloc::{M_MOUNT, M_VNODE, M_WAITOK, M_ZERO};
#[cfg(feature = "nfsserver")]
use crate::sys::malloc::{M_NETADDR, M_RTABLE};
use crate::sys::mbuf::Mbuf;
#[cfg(feature = "nfsserver")]
use crate::sys::mbuf::{MLEN, mtod};
use crate::sys::mount::{
    ExportArgs, Fsid, MFSNAMELEN, MNAMELEN, MNT_NOPERM, MNT_RDONLY, MNT_STALLED, MNT_UNMOUNT,
    MNT_WAIT, MntList, Mount, Netcred, Netexport, Statfs, VB_DUPOK, VB_NOWAIT, VB_READ, VB_WAIT,
    VB_WRITE, VFS_BCACHESTAT, VFS_CONF, VFS_GENERIC, VFS_MAXTYPENUM, VFS_SYNC, Vfsconf,
};
#[cfg(feature = "nfsserver")]
use crate::sys::mount::{MNT_DEFEXPORTED, MNT_DELEXPORT, MNT_EXPORTED};
use crate::sys::mutex::Mutex;
use crate::sys::param::{NODEV, PINOD, PRIBIO};
use crate::sys::pool::{PR_WAITOK, PR_ZERO, Pool};
use crate::sys::proc::Proc;
use crate::sys::queue::{ListHead, TailqHead};
use crate::sys::rwlock::{RW_NOSLEEP, RW_READ, RW_WRITE, RWL_IS_VNODE, Rwlock};
use crate::sys::sched::sched_pause;
#[cfg(feature = "nfsserver")]
use crate::sys::socket::AF_INET;
use crate::sys::specdev::{CLONE_MAPSZ, CLONE_SHIFT, Specinfo, spechash};
use crate::sys::stat::{
    S_IFBLK, S_IFCHR, S_IFDIR, S_IFIFO, S_IFLNK, S_IFMT, S_IFREG, S_IFSOCK, S_IRGRP, S_IROTH,
    S_IRUSR, S_IWGRP, S_IWOTH, S_IWUSR, S_IXGRP, S_IXOTH, S_IXUSR,
};
use crate::sys::systm::{INFSLP, kernel_assert_locked};
use crate::sys::types::{Dev, Gid, Mode, Uid, major, makedev, minor};
use crate::sys::ucred::{NOCRED, Ucred};
use crate::sys::vnode::VN_KNOTE;
use crate::sys::vnode::{
    BVnbufs, Buflists, DOCLOSE, FORCECLOSE, IGNORECLEAN, REVOKEALL, SKIPSYSTEM, V_SAVE, V_SAVEMETA,
    VALIASED, VBAD, VBIOERROR, VBIOONFREELIST, VBIOONSYNCLIST, VBIOWAIT, VBLK, VCHR, VDIR, VEXEC,
    VFIFO, VFreelist, VISTTY, VLNK, VNON, VNOVAL, VREAD, VREG, VROOT, VSOCK, VSYSTEM, VSynclist,
    VT_NON, VWRITE, VXLOCK, VXWANT, Vattr, Vnode, VnodeUn, Vops, Vtagtype, Vtype, WRITECLOSE,
};
use crate::uvm::uvm_vnode::{uvm_vnp_sync, uvm_vnp_terminate};

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

// SAFETY: the lists are changed at `splbio` under the kernel lock, as in C.
unsafe impl Sync for Freelst {}

/// `struct mntlist`: the mounted file systems.
pub struct Mntlist(pub TailqHead<MntList>);

// SAFETY: the list is changed under the kernel lock, as in C.
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

/// `bufinsvn(bp, dp)`: puts a buffer on a vnode's clean or dirty list.
///
/// # Safety
///
/// `bp` is on no vnode list.
unsafe fn bufinsvn(bp: &'static Buf, dp: &Buflists) {
    // SAFETY: the caller's contract; the buffer stays in its pool item while linked.
    unsafe { dp.insert_head(bp) };
    bp.b_onvnbufs.set(true);
}

/// `bufremvn(bp)`: takes a buffer off its vnode list (`LIST_NEXT(bp, b_vnbufs) = NOLIST`).
///
/// # Safety
///
/// `bp` is on a vnode list.
unsafe fn bufremvn(bp: &'static Buf) {
    // SAFETY: the caller's contract.
    unsafe { ListHead::<BVnbufs>::remove(bp) };
    bp.b_onvnbufs.set(false);
}

/// `rb_buf_compare(b1, b2)`: orders a vnode's buffers by logical block (`RBT_GENERATE
/// (buf_rb_bufs, ...)`; the tree adapter is `sys/vnode.rs`'s `BufRbBufs`).
pub fn rb_buf_compare(b1: &Buf, b2: &Buf) -> core::cmp::Ordering {
    b1.b_lblkno.get().cmp(&b2.b_lblkno.get())
}

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
    vn_initialize_syncerd();

    #[cfg(feature = "nfsserver")]
    rn_init(size_of::<SockaddrIn>() as u32);
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
    let maxvnodes = MAXVNODES
        .load(Ordering::Relaxed)
        .max(BCSTATS.numbufs.load(Ordering::Relaxed) as i32);
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
        vp.v_bufs_tree.init();
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
    if vp.v_type.get() == VCHR && cdevsw(major(vp.v_rdev())).d_type == D_TTY {
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
            si.si_lockf.set(None);
            si.si_ci_bitmap.set(ptr::null_mut());
            nvp.v_un.set(VnodeUn::Specinfo(si));
            if nvp.v_type.get() == VCHR
                && cdevsw(major(nvp_rdev)).d_flags & D_CLONE != 0
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
    kernel_assert_locked();

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
    // already, right?
    let s = splbio();
    let empty = va.flags & IGNORECLEAN != 0 && vp.v_dirtyblkhd.is_empty();
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
    VN_KNOTE(vp, NOTE_REVOKE);
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
            && cdevsw(major(si.si_rdev.get())).d_flags & D_CLONE != 0
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
        if si.si_lockf.get().is_some() {
            lf_purgelocks(&si.si_lockf);
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
        VFS_BCACHESTAT => {
            // buffer cache statistics
            let bcstats = BCSTATS.snapshot();
            sysctl_rdstruct(oldp, oldlenp, newp, bcstats.as_bytes())
        }
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

/// `KEYLEN_PAD`: the bytes of slack behind an export entry's address and mask (the radix
/// code's `KEYLEN_LIMIT`, the longest key it reads).
#[cfg(feature = "nfsserver")]
const KEYLEN_PAD: usize = 64;

/// Build hash lists of net addresses and hang them off the mount point. Called by
/// `vfs_export()` to set up the lists of export addresses.
#[cfg(feature = "nfsserver")]
fn vfs_hang_addrlist(
    mp: &'static Mount,
    nep: &'static Netexport,
    argp: &ExportArgs,
) -> Result<(), Errno> {
    if argp.ex_addrlen == 0 {
        if mp.mnt_flag.get() & MNT_DEFEXPORTED != 0 {
            return Err(Errno::EPERM);
        }
        let np = &nep.ne_defexported;
        // fill in the kernel's ucred from userspace's xucred
        crfromxucred(&np.netc_anon, &argp.ex_anon)?;
        mp.mnt_flag.set(mp.mnt_flag.get() | MNT_DEFEXPORTED);
        np.netc_exflags.set(argp.ex_flags);
        return Ok(());
    }
    if argp.ex_addrlen > MLEN as i32
        || argp.ex_masklen > MLEN as i32
        || argp.ex_addrlen < 0
        || argp.ex_masklen < 0
    {
        return Err(Errno::EINVAL);
    }
    let addrlen = argp.ex_addrlen as usize;
    let masklen = argp.ex_masklen as usize;
    if addrlen < 2 {
        // No room for the family byte `saddr->sa_family` reads.
        return Err(Errno::EINVAL);
    }
    let nplen = size_of::<Netcred>() + addrlen + masklen + KEYLEN_PAD;
    let Some(mem) = malloc(nplen, M_NETADDR, M_WAITOK | M_ZERO) else {
        panic(format_args!("vfs_hang_addrlist: out of memory"));
    };
    let npp = mem.cast::<Netcred>();
    // SAFETY: a fresh, suitably aligned (malloc's chunks are) allocation of `nplen` bytes,
    // at least a `Netcred`'s; it is written before anything reads it, and stays allocated
    // until `vfs_free_netcred` (or the `free` below) returns it.
    let np: &'static Netcred = unsafe {
        npp.as_ptr().write(Netcred::new());
        &*npp.as_ptr()
    };
    np.netc_len.set(nplen as i32);
    let free_np = || free(mem, M_NETADDR, nplen);
    // The key (and mask) live right behind the entry, as in C: `saddr = (np + 1)`.
    // SAFETY: inside the allocation: `Netcred` bytes, then `addrlen + masklen + KEYLEN_PAD`.
    let saddr = unsafe { mem.as_ptr().add(size_of::<Netcred>()) };
    // SAFETY: as above; `saddr .. saddr + addrlen` is inside the allocation and nothing else
    // refers to it yet.
    let saddr_buf = unsafe { core::slice::from_raw_parts_mut(saddr, addrlen) };
    if let Err(e) = copyin(argp.ex_addr, saddr_buf) {
        free_np();
        return Err(e);
    }
    if usize::from(saddr_buf[0]) > addrlen {
        saddr_buf[0] = addrlen as u8;
    }
    let mut smask: *const u8 = core::ptr::null();
    if masklen != 0 {
        // SAFETY: inside the allocation, right behind the address.
        let m = unsafe { saddr.add(addrlen) };
        // SAFETY: `m .. m + masklen` is inside the allocation, unshared.
        let mask_buf = unsafe { core::slice::from_raw_parts_mut(m, masklen) };
        if let Err(e) = copyin(argp.ex_mask, mask_buf) {
            free_np();
            return Err(e);
        }
        if usize::from(mask_buf[0]) > masklen {
            mask_buf[0] = masklen as u8;
        }
        smask = m;
    }
    // fill in the kernel's ucred from userspace's xucred
    if let Err(e) = crfromxucred(&np.netc_anon, &argp.ex_anon) {
        free_np();
        return Err(e);
    }
    let rnh: &'static RadixNodeHead = match saddr_buf[1] {
        AF_INET => match nep.ne_rtable_inet.get() {
            Some(rnh) => rnh,
            None => {
                let mut head = None;
                if !rn_inithead(
                    &mut head,
                    core::mem::offset_of!(SockaddrIn, sin_addr) as i32,
                ) {
                    free_np();
                    return Err(Errno::ENOBUFS);
                }
                nep.ne_rtable_inet.set(head);
                let Some(rnh) = head else {
                    free_np();
                    return Err(Errno::ENOBUFS);
                };
                rnh
            }
        },
        _ => {
            free_np();
            return Err(Errno::EINVAL);
        }
    };
    // SAFETY: `rn_init` ran in `vfs_init`; the address and the mask are byte strings led by
    // their length, `addrlen`/`masklen` long and followed by `KEYLEN_PAD` readable bytes (so
    // readable for the radix code's `max_keylen`); both stay in place, with the node pair
    // `np.netc_rnodes` (zeroed), until `vfs_free_netcred` deletes the route.
    let rn = unsafe { rn_addroute(saddr, smask, rnh, &np.netc_rnodes, 0) };
    if !rn.is_some_and(|rn| core::ptr::eq(core::ptr::from_ref(rn).cast::<Netcred>(), np)) {
        // already exists
        free_np();
        return Err(Errno::EPERM);
    }
    np.netc_exflags.set(argp.ex_flags);
    Ok(())
}

/// `vfs_free_netcred`: delete an export entry from its tree and free it (the function
/// `rn_walktree` calls for every entry of `rnh`).
#[cfg(feature = "nfsserver")]
fn vfs_free_netcred(rn: &'static RadixNode, rnh: &RadixNodeHead) {
    let np = core::ptr::from_ref(rn).cast::<Netcred>();
    // SAFETY: `rn` is the leaf of an entry `vfs_hang_addrlist` added to `rnh`, visited by
    // `rn_walktree`, the one leaf that may be deleted inside the walk; its key and mask stay
    // allocated until the `free` below.
    let _ = unsafe { rn_delete(rn.rn_key.get(), rn.rn_mask.get(), rnh, None) };
    // SAFETY: the node pair is the first member of the `Netcred` that was `malloc`ed with
    // `M_NETADDR`; `rn_delete` has returned it, so nothing refers to it any more.
    unsafe {
        let len = (*np).netc_len.get() as usize;
        free(
            NonNull::new_unchecked(np.cast_mut().cast::<u8>()),
            M_NETADDR,
            len,
        );
    }
}

/// Free the net address hash lists that are hanging off the mount points.
#[cfg(feature = "nfsserver")]
fn vfs_free_addrlist(nep: &'static Netexport) {
    if let Some(rnh) = nep.ne_rtable_inet.get() {
        let _ = rn_walktree(rnh, |rn, _id| {
            vfs_free_netcred(rn, rnh);
            Ok(())
        });
        free(
            NonNull::from(rnh).cast::<u8>(),
            M_RTABLE,
            size_of::<RadixNodeHead>(),
        );
        nep.ne_rtable_inet.set(None);
    }
}

/// `vfs_export(mp, nep, argp)`: process mount export info. `nep` is the file system's export
/// list (`um_export`, ...), `argp` the kernel copy of the `struct export_args`.
pub fn vfs_export(
    mp: &'static Mount,
    nep: &'static Netexport,
    argp: &ExportArgs,
) -> Result<(), Errno> {
    #[cfg(feature = "nfsserver")]
    {
        if argp.ex_flags & MNT_DELEXPORT != 0 {
            vfs_free_addrlist(nep);
            mp.mnt_flag
                .set(mp.mnt_flag.get() & !(MNT_EXPORTED | MNT_DEFEXPORTED));
        }
        if argp.ex_flags & MNT_EXPORTED != 0 {
            vfs_hang_addrlist(mp, nep, argp)?;
            mp.mnt_flag.set(mp.mnt_flag.get() | MNT_EXPORTED);
        }
        Ok(())
    }
    #[cfg(not(feature = "nfsserver"))]
    {
        let _ = (mp, nep, argp);
        Err(Errno::ENOTSUP)
    }
}

/// `vfs_export_lookup(mp, nep, nam)`: lookup host in fs export list. `nam` is the mbuf with
/// the client's `struct sockaddr` (none: only the default export can match). Without
/// `NFSSERVER` there is never an entry.
pub fn vfs_export_lookup(
    mp: &'static Mount,
    nep: &'static Netexport,
    nam: Option<&Mbuf>,
) -> Option<&'static Netcred> {
    #[cfg(feature = "nfsserver")]
    {
        // SAFETY: an address mbuf holds a `struct sockaddr`, readable for `max_keylen` bytes
        // (the longest key), as every mbuf's data area is.
        unsafe { export_lookup(mp, nep, nam.map(|m| mtod::<u8>(m).cast_const())) }
    }
    #[cfg(not(feature = "nfsserver"))]
    {
        let _ = (mp, nep, nam);
        None
    }
}

/// The body of `vfs_export_lookup`, over the client's `struct sockaddr` as a byte pointer.
///
/// # Safety
///
/// `saddr`, when given, points at a `struct sockaddr` readable for its length byte and for
/// the radix code's `max_keylen` bytes (an mbuf's data area is), and `rn_init` has run.
#[cfg(feature = "nfsserver")]
unsafe fn export_lookup(
    mp: &'static Mount,
    nep: &'static Netexport,
    saddr: Option<*const u8>,
) -> Option<&'static Netcred> {
    let mut np: Option<&'static Netcred> = None;
    if mp.mnt_flag.get() & MNT_EXPORTED != 0 {
        // Lookup in the export list first.
        if let Some(saddr) = saddr {
            // SAFETY: the caller's contract: the family byte is readable.
            let family = unsafe { saddr.add(1).read() };
            let rnh = match family {
                AF_INET => nep.ne_rtable_inet.get(),
                _ => None,
            };
            if let Some(rnh) = rnh {
                // SAFETY: the tree holds only entries `vfs_hang_addrlist` made, live until
                // `vfs_free_addrlist`; `saddr` is a key by the caller's contract; the leaf
                // is the first member of its `Netcred`.
                np = unsafe {
                    rn_match(saddr, rnh).map(|rn| &*core::ptr::from_ref(rn).cast::<Netcred>())
                };
            }
        }
        // If no address match, use the default if it exists.
        if np.is_none() && mp.mnt_flag.get() & MNT_DEFEXPORTED != 0 {
            np = Some(&nep.ne_defexported);
        }
    }
    np
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
            uvm_vnp_sync(Some(mp));
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
    crate::kern::kern_acct::acct_shutdown();

    kprintf!("syncing disks...");

    if !panicstr() {
        // Sync before unmount, in case we hang on something.
        let mut retval = [0; 2];
        let _ = sys_sync(p, &[0; 6], &mut retval);
        vfs_unmountall();
    }

    // NSOFTRAID > 0
    crate::dev::softraid::sr_quiesce();

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

    // Wait for sync to finish.
    let mut dcount = 10000;
    let mut nbusy = 0;
    for iter in 0..20u32 {
        nbusy = 0;
        for bp in BUFHEAD.0.iter() {
            if bp.b_flags.get() & (B_BUSY | B_INVAL | B_READ) == B_BUSY {
                nbusy += 1;
            }
            // With soft updates, some buffers that are written will be remarked as dirty
            // until other buffers are written.
            //
            // XXX here be dragons. this should really go away but should be carefully made to
            // go away on it's own with testing.. XXX
            if bp.isset(B_DELWRI) {
                let s = splbio();
                bufcache_take(bp);
                buf_acquire(bp);
                splx(s);
                nbusy += 1;
                bawrite(bp);
                dcount -= 1;
                if dcount <= 0 {
                    if verbose {
                        kprintf!("softdep ");
                    }
                    return 1;
                }
            }
        }
        if nbusy == 0 {
            break;
        }
        if verbose {
            kprintf!("{} ", nbusy);
        }
        #[cfg(feature = "multiprocessor")]
        let hold_count = if crate::kern::kern_lock::_kernel_lock_held() {
            crate::kern::kern_lock::__mp_release_all(&crate::kern::kern_lock::KERNEL_LOCK)
        } else {
            0
        };
        delay(40000 * iter);
        #[cfg(feature = "multiprocessor")]
        if hold_count != 0 {
            crate::kern::kern_lock::__mp_acquire_count(
                &crate::kern::kern_lock::KERNEL_LOCK,
                hold_count,
            );
        }
    }

    nbusy
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
    slpflag: i32,
    slptimeo: u64,
) -> Result<(), Errno> {
    // VFSLCKDEBUG: not configured.

    if flags & V_SAVE != 0 {
        let s = splbio();
        let _ = vwaitforio(vp, 0, "vinvalbuf", INFSLP);
        if !vp.v_dirtyblkhd.is_empty() {
            splx(s);
            let Some(p) = p else {
                panic(format_args!(
                    "vinvalbuf: dirty buffers and no thread to sync them"
                ));
            };
            VOP_FSYNC(vp, cred, MNT_WAIT, p)?;
            let s = splbio();
            if vp.v_numoutput.get() > 0 || !vp.v_dirtyblkhd.is_empty() {
                panic(format_args!("vinvalbuf: dirty bufs, vp {:p}", vp));
            }
            splx(s);
        } else {
            splx(s);
        }
    }
    // The first buffer of a list to look at, skipping the metadata (negative blocks) for
    // V_SAVEMETA.
    let skipmeta = |mut b: Option<&'static Buf>| {
        if flags & V_SAVEMETA != 0 {
            while let Some(bp) = b
                && bp.b_lblkno.get() < 0
            {
                b = ListHead::<BVnbufs>::next(bp);
            }
        }
        b
    };
    'restart: loop {
        // loop:
        let s = splbio();
        loop {
            let mut count = 0;
            let Some(blist) =
                skipmeta(vp.v_cleanblkhd.first()).or_else(|| skipmeta(vp.v_dirtyblkhd.first()))
            else {
                break;
            };

            let mut next = Some(blist);
            while let Some(bp) = next {
                next = ListHead::<BVnbufs>::next(bp);
                if flags & V_SAVEMETA != 0 && bp.b_lblkno.get() < 0 {
                    continue;
                }
                if bp.isset(B_BUSY) {
                    bp.set(B_WANTED);
                    if let Err(error) = tsleep_nsec(
                        ptr::from_ref(bp),
                        slpflag | (PRIBIO + 1),
                        "vinvalbuf",
                        slptimeo,
                    ) {
                        splx(s);
                        return Err(error);
                    }
                    break;
                }
                bufcache_take(bp);
                // XXX Since there are no node locks for NFS, I believe there is a slight
                // chance that a delayed write will occur while sleeping just above, so check
                // for it.
                if bp.isset(B_DELWRI) && flags & V_SAVE != 0 {
                    buf_acquire(bp);
                    splx(s);
                    let _ = VOP_BWRITE(bp);
                    continue 'restart;
                }
                buf_acquire_nomap(bp);
                bp.set(B_INVAL);
                brelse(bp);
                count += 1;
                // XXX Temporary workaround XXX
                //
                // If this is a gigantisch vnode and we are trashing a ton of buffers, drop the
                // lock and yield every so often. The longer term fix is to add a separate list
                // for these invalid buffers so we don't have to do the work to free these here.
                if count > 100 {
                    splx(s);
                    sched_pause(r#yield);
                    continue 'restart;
                }
            }
        }
        if flags & V_SAVEMETA == 0 && (!vp.v_dirtyblkhd.is_empty() || !vp.v_cleanblkhd.is_empty()) {
            panic(format_args!("vinvalbuf: flush failed, vp {:p}", vp));
        }
        splx(s);
        return Ok(());
    }
}

/// `vflushbuf(vp, sync)`: writes the vnode's dirty buffers out, and with `sync` waits for
/// them.
pub fn vflushbuf(vp: &'static Vnode, sync: bool) {
    'restart: loop {
        // loop:
        let s = splbio();
        for bp in vp.v_dirtyblkhd.iter() {
            if bp.isset(B_BUSY) {
                continue;
            }
            if !bp.isset(B_DELWRI) {
                panic(format_args!("vflushbuf: not dirty"));
            }
            bufcache_take(bp);
            buf_acquire(bp);
            splx(s);
            // Wait for I/O associated with indirect blocks to complete, since there is no way
            // to quickly wait for them below.
            if bp.b_vp.get().is_some_and(|bvp| ptr::eq(bvp, vp)) || !sync {
                bawrite(bp);
            } else {
                let _ = bwrite(bp);
            }
            continue 'restart;
        }
        if !sync {
            splx(s);
            return;
        }
        let _ = vwaitforio(vp, 0, "vflushbuf", INFSLP);
        if !vp.v_dirtyblkhd.is_empty() {
            splx(s);
            #[cfg(feature = "diagnostic")]
            vprint(Some("vflushbuf: dirty"), vp);
            continue 'restart;
        }
        splx(s);
        return;
    }
}

/// Associate a buffer with a vnode.
///
/// Manipulates buffer vnode queues. Must be called at splbio().
pub fn bgetvp(vp: &'static Vnode, bp: &'static Buf) {
    splassert(IPL_BIO, "bgetvp");

    if bp.b_vp.get().is_some() {
        panic(format_args!("bgetvp: not free"));
    }
    vhold(vp);
    bp.b_vp.set(Some(vp));
    if vp.v_type.get() == VBLK || vp.v_type.get() == VCHR {
        bp.b_dev.set(vp.v_rdev());
    } else {
        bp.b_dev.set(NODEV);
    }
    // Insert onto list for new vnode.
    // SAFETY: a buffer without a vnode is on no vnode list.
    unsafe { bufinsvn(bp, &vp.v_cleanblkhd) };
}

/// Disassociate a buffer from a vnode.
///
/// Manipulates vnode buffer queues. Must be called at splbio().
pub fn brelvp(bp: &'static Buf) {
    splassert(IPL_BIO, "brelvp");

    let Some(vp) = bp.b_vp.get() else {
        panic(format_args!("brelvp: NULL"));
    };
    // Delete from old vnode list, if on one.
    if bp.b_onvnbufs.get() {
        // SAFETY: `b_onvnbufs` says the buffer is on a vnode list.
        unsafe { bufremvn(bp) };
    }
    if vp.v_bioflag.get() & VBIOONSYNCLIST != 0 && vp.v_dirtyblkhd.is_empty() {
        vp.v_bioflag.set(vp.v_bioflag.get() & !VBIOONSYNCLIST);
        // SAFETY: a vnode marked `VBIOONSYNCLIST` is on the syncer's wheel.
        unsafe { ListHead::<VSynclist>::remove(vp) };
    }
    bp.b_vp.set(None);

    vdrop(vp);
}

/// Replaces the current vnode associated with the buffer, if any, with a new vnode.
///
/// If an output I/O is pending on the buffer, the old vnode I/O count is adjusted.
///
/// Ignores vnode buffer queues. Must be called at splbio().
pub fn buf_replacevnode(bp: &'static Buf, newvp: &'static Vnode) {
    let oldvp = bp.b_vp.get();

    splassert(IPL_BIO, "buf_replacevnode");

    if oldvp.is_some() {
        brelvp(bp);
    }

    if bp.b_flags.get() & (B_READ | B_DONE) == 0 {
        newvp.v_numoutput.set(newvp.v_numoutput.get() + 1); // put it on swapdev
        vwakeup(oldvp);
    }

    bgetvp(newvp, bp);
    // SAFETY: `bgetvp` just put the buffer on the new vnode's clean list.
    unsafe { bufremvn(bp) };
}

/// Used to assign buffers to the appropriate clean or dirty list on the vnode and to add
/// newly dirty vnodes to the appropriate filesystem syncer list.
///
/// Manipulates vnode buffer queues. Must be called at splbio().
pub fn reassignbuf(bp: &'static Buf) {
    let Some(vp) = bp.b_vp.get() else {
        panic(format_args!("reassignbuf: NULL"));
    };

    splassert(IPL_BIO, "reassignbuf");

    // Delete from old vnode list, if on one.
    if bp.b_onvnbufs.get() {
        // SAFETY: `b_onvnbufs` says the buffer is on a vnode list.
        unsafe { bufremvn(bp) };
    }

    // If dirty, put on list of dirty buffers; otherwise insert onto list of clean buffers.
    let listheadp = if !bp.isset(B_DELWRI) {
        if vp.v_bioflag.get() & VBIOONSYNCLIST != 0 && vp.v_dirtyblkhd.is_empty() {
            vp.v_bioflag.set(vp.v_bioflag.get() & !VBIOONSYNCLIST);
            // SAFETY: a vnode marked `VBIOONSYNCLIST` is on the syncer's wheel.
            unsafe { ListHead::<VSynclist>::remove(vp) };
        }
        &vp.v_cleanblkhd
    } else {
        if vp.v_bioflag.get() & VBIOONSYNCLIST == 0 {
            let syncdelay = SYNCDELAY.load(Ordering::Relaxed);
            let delay = match vp.v_type.get() {
                VDIR => syncdelay / 2,
                VBLK if vp.v_specmountpoint().is_some() => syncdelay / 3,
                _ => syncdelay,
            };
            vn_syncer_add_to_worklist(vp, delay);
        }
        &vp.v_dirtyblkhd
    };
    // SAFETY: the buffer was just taken off its vnode list, if it was on one.
    unsafe { bufinsvn(bp, listheadp) };
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
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
pub(crate) mod tests {
    // Host tests for the vnode table (`getnewvnode`, the use counts and free lists of
    // `vref`/`vrele`/`vput`/`vget`, `vgone`, `insmntque`, `vflush`), `vattr_null`, `vaccess` and
    // the mount helpers; and `testfs`, a small in-memory file system (a fixed tree of
    // directories, a file and symbolic links, with the lock discipline of a real one) that the
    // `namei`, name cache and `getcwd` tests share.

    use std::sync::MutexGuard;
    use std::{assert, assert_eq, boxed::Box};

    use super::*;
    use crate::kern::kern_descrip::{fdinit, filedesc_init};
    use crate::kern::kern_proc::procinit;
    use crate::kern::kern_prot::{crget, crhold};
    use crate::kern::subr_pool::tests::setup_real_memory;
    use crate::kern::vfs_cache::{NUMCACHE, NUMNEG};
    use crate::kern::vfs_init::{set_rootvnode, vfsinit};
    use crate::sys::proc::Process;

    /// `testfs`: the file system the vfs tests mount as root.
    pub(crate) mod testfs {
        use core::ffi::c_void;
        use core::ptr;
        use core::sync::atomic::{AtomicPtr, AtomicU32, AtomicUsize, Ordering};
        use std::boxed::Box;
        use std::sync::MutexGuard;

        use super::setup;
        use crate::kern::kern_subr::uiomove;
        use crate::kern::subr_xxx::eopnotsupp;
        use crate::kern::vfs_cache::{cache_enter, cache_lookup};
        use crate::kern::vfs_default::vop_generic_abortop;
        use crate::kern::vfs_init::set_rootvnode;
        use crate::kern::vfs_subr::{
            MOUNTLIST, getnewvnode, vfs_mount_alloc, vfs_unbusy, vget, vref,
        };
        use crate::kern::vfs_vnops::vn_lock;
        use crate::kern::vfs_vops::VOP_UNLOCK;
        use crate::sys::dirent::{DT_DIR, DT_LNK, DT_REG, Dirent, dirent_recsize};
        use crate::sys::errno::Errno;
        use crate::sys::lock::{LK_EXCLUSIVE, LK_NOWAIT};
        use crate::sys::mount::{MNT_LOCAL, MNT_ROOTFS, Mount, VFS_ROOT, Vfsconf, Vfsops};
        use crate::sys::namei::{
            CREATE, ISDOTDOT, ISLASTCN, LOCKPARENT, MAKEENTRY, PDIRUNLOCK, RENAME,
        };
        use crate::sys::proc::Proc;
        use crate::sys::vnode::{
            VDIR, VLNK, VREG, VROOT, VT_TMPFS, Vnode, VopAccessArgs, VopGetattrArgs,
            VopInactiveArgs, VopIslockedArgs, VopLockArgs, VopLookupArgs, VopReaddirArgs,
            VopReadlinkArgs, VopReclaimArgs, VopUnlockArgs, Vops,
        };

        /// What a node is.
        pub(crate) enum Kind {
            /// A directory.
            Dir,
            /// A regular file.
            Reg,
            /// A symbolic link to the path.
            Lnk(&'static [u8]),
        }

        /// One node of the tree: its name, its parent's index and its kind.
        pub(crate) struct Node {
            pub(crate) name: &'static [u8],
            pub(crate) parent: usize,
            pub(crate) kind: Kind,
        }

        /// `/`.
        pub(crate) const ROOT: usize = 0;
        /// `/a`.
        pub(crate) const A: usize = 1;
        /// `/a/b`, a file.
        pub(crate) const B: usize = 2;
        /// `/a/c`.
        pub(crate) const C: usize = 3;
        /// `/l -> a/c`.
        pub(crate) const L: usize = 4;
        /// `/a/abs -> /a/b`.
        pub(crate) const ABS: usize = 5;
        /// `/loop -> loop`.
        pub(crate) const LOOP: usize = 6;
        /// `/a/xxx...x` (40 bytes, longer than `NAMECACHE_MAXLEN`).
        pub(crate) const LONG: usize = 7;
        /// The number of nodes.
        pub(crate) const N: usize = 8;

        /// The tree.
        pub(crate) static NODES: [Node; N] = [
            Node {
                name: b"/",
                parent: ROOT,
                kind: Kind::Dir,
            },
            Node {
                name: b"a",
                parent: ROOT,
                kind: Kind::Dir,
            },
            Node {
                name: b"b",
                parent: A,
                kind: Kind::Reg,
            },
            Node {
                name: b"c",
                parent: A,
                kind: Kind::Dir,
            },
            Node {
                name: b"l",
                parent: ROOT,
                kind: Kind::Lnk(b"a/c"),
            },
            Node {
                name: b"abs",
                parent: A,
                kind: Kind::Lnk(b"/a/b"),
            },
            Node {
                name: b"loop",
                parent: ROOT,
                kind: Kind::Lnk(b"loop"),
            },
            Node {
                name: b"xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
                parent: A,
                kind: Kind::Reg,
            },
        ];

        /// Each node's vnode, while it has one (the "inode hash").
        static VTAB: [AtomicPtr<Vnode>; N] = [const { AtomicPtr::new(ptr::null_mut()) }; N];
        /// Each node's lock: 1 while held.
        static LOCKED: [AtomicU32; N] = [const { AtomicU32::new(0) }; N];
        /// How many times `VOP_INACTIVE` ran.
        pub(crate) static INACTIVE: AtomicUsize = AtomicUsize::new(0);
        /// How many times `VOP_RECLAIM` ran.
        pub(crate) static RECLAIM: AtomicUsize = AtomicUsize::new(0);
        /// The mount.
        static MP: AtomicPtr<Mount> = AtomicPtr::new(ptr::null_mut());

        /// Forgets every vnode and lock (the pools behind them are new after `setup`).
        pub(crate) fn reset() {
            for i in 0..N {
                VTAB[i].store(ptr::null_mut(), Ordering::SeqCst);
                LOCKED[i].store(0, Ordering::SeqCst);
            }
            INACTIVE.store(0, Ordering::SeqCst);
            RECLAIM.store(0, Ordering::SeqCst);
            MP.store(ptr::null_mut(), Ordering::SeqCst);
        }

        /// The node a testfs vnode stands for.
        pub(crate) fn node_of(vp: &Vnode) -> usize {
            let idx = vp.v_data.get() as usize;
            assert!(idx > 0, "not a testfs vnode");
            idx - 1
        }

        /// The node's vnode, if it has one.
        pub(crate) fn vnode_of(idx: usize) -> Option<&'static Vnode> {
            // SAFETY: only `vget_node` stores here, and only vnodes, which are never freed.
            unsafe { VTAB[idx].load(Ordering::SeqCst).as_ref() }
        }

        /// The use count of every node's vnode (0 for none).
        pub(crate) fn usecounts() -> [u32; N] {
            core::array::from_fn(|i| vnode_of(i).map_or(0, |vp| vp.v_usecount.get()))
        }

        /// Whether any node's lock is held.
        pub(crate) fn any_locked() -> bool {
            LOCKED.iter().any(|l| l.load(Ordering::SeqCst) != 0)
        }

        /// The vnode of node `idx`, referenced and locked (`VFS_VGET`).
        pub(crate) fn vget_node(mp: &'static Mount, idx: usize) -> Result<&'static Vnode, Errno> {
            if let Some(vp) = vnode_of(idx) {
                vget(vp, LK_EXCLUSIVE)?;
                return Ok(vp);
            }
            let vp = getnewvnode(VT_TMPFS, Some(mp), &TESTFS_VOPS)?;
            vp.v_type.set(match NODES[idx].kind {
                Kind::Dir => VDIR,
                Kind::Reg => VREG,
                Kind::Lnk(_) => VLNK,
            });
            vp.v_data
                .set(ptr::without_provenance_mut::<c_void>(idx + 1));
            if idx == ROOT {
                vp.v_flag.set(VROOT);
            }
            vn_lock(vp, LK_EXCLUSIVE)?;
            VTAB[idx].store(ptr::from_ref(vp).cast_mut(), Ordering::SeqCst);
            Ok(vp)
        }

        fn the_mount() -> &'static Mount {
            // SAFETY: `mount_root` stores the mount, which stays allocated while mounted.
            unsafe { &*MP.load(Ordering::SeqCst) }
        }

        fn testfs_mount(
            _mp: &'static Mount,
            _path: &[u8],
            _data: &mut [u8],
            _ndp: &mut crate::sys::namei::Nameidata<'_>,
            _p: &Proc,
        ) -> Result<(), Errno> {
            Ok(())
        }

        fn testfs_root(mp: &'static Mount) -> Result<&'static Vnode, Errno> {
            vget_node(mp, ROOT)
        }

        fn testfs_vget(mp: &'static Mount, ino: u64) -> Result<&'static Vnode, Errno> {
            vget_node(mp, ino as usize)
        }

        /// `vfsops` of testfs: only the root and the inode lookup do anything.
        pub(crate) static TESTFS_VFSOPS: Vfsops = Vfsops {
            vfs_mount: testfs_mount,
            vfs_start: |_, _, _| Ok(()),
            vfs_unmount: |_, _, _| Ok(()),
            vfs_root: testfs_root,
            vfs_quotactl: |_, _, _, _, _| eopnotsupp(),
            vfs_statfs: |_, _, _| Ok(()),
            vfs_sync: |_, _, _, _, _| Ok(()),
            vfs_vget: testfs_vget,
            vfs_fhtovp: |_, _| Err(Errno::EOPNOTSUPP),
            vfs_vptofh: |_, _| eopnotsupp(),
            vfs_init: None,
            vfs_sysctl: None,
            vfs_checkexp: |_, _, _, _| eopnotsupp(),
        };

        /// The configuration entry of testfs.
        pub(crate) static TESTFS_CONF: Vfsconf =
            Vfsconf::new(&TESTFS_VFSOPS, b"testfs", 99, MNT_LOCAL, 0);

        fn testfs_lock(ap: &mut VopLockArgs) -> Result<(), Errno> {
            let l = &LOCKED[node_of(ap.a_vp)];
            if l.load(Ordering::SeqCst) != 0 {
                if ap.a_flags & LK_NOWAIT != 0 {
                    return Err(Errno::EBUSY);
                }
                std::panic!("testfs: node {} locked twice", node_of(ap.a_vp));
            }
            l.store(1, Ordering::SeqCst);
            Ok(())
        }

        fn testfs_unlock(ap: &mut VopUnlockArgs) -> Result<(), Errno> {
            let l = &LOCKED[node_of(ap.a_vp)];
            assert_eq!(
                l.load(Ordering::SeqCst),
                1,
                "testfs: unlock of an unlocked node"
            );
            l.store(0, Ordering::SeqCst);
            Ok(())
        }

        fn testfs_islocked(ap: &mut VopIslockedArgs) -> i32 {
            LOCKED[node_of(ap.a_vp)].load(Ordering::SeqCst) as i32
        }

        fn testfs_inactive(ap: &mut VopInactiveArgs<'_>) -> Result<(), Errno> {
            INACTIVE.fetch_add(1, Ordering::SeqCst);
            VOP_UNLOCK(ap.a_vp)
        }

        fn testfs_reclaim(ap: &mut VopReclaimArgs<'_>) -> Result<(), Errno> {
            RECLAIM.fetch_add(1, Ordering::SeqCst);
            let idx = node_of(ap.a_vp);
            VTAB[idx].store(ptr::null_mut(), Ordering::SeqCst);
            LOCKED[idx].store(0, Ordering::SeqCst);
            ap.a_vp.v_data.set(ptr::null_mut());
            Ok(())
        }

        /// The lookup of `ufs_lookup`, over the fixed tree: the name cache first, then the
        /// directory; the parent stays locked only for `LOCKPARENT` on the last component.
        fn testfs_lookup(ap: &mut VopLookupArgs<'_>) -> Result<(), Errno> {
            let dvp = ap.a_dvp;
            let cnp = &mut *ap.a_cnp;
            *ap.a_vpp = None;
            if dvp.v_type.get() != VDIR {
                return Err(Errno::ENOTDIR);
            }
            let lockparent = cnp.cn_flags & LOCKPARENT != 0;
            let islast = cnp.cn_flags & ISLASTCN != 0;

            match cache_lookup(dvp, cnp) {
                Ok(Some(vp)) => {
                    *ap.a_vpp = Some(vp);
                    return Ok(());
                }
                Ok(None) => {}
                Err(e) => return Err(e),
            }

            let dir = node_of(dvp);
            let name = cnp.name();
            let found = if name == b"." {
                Some(dir)
            } else if cnp.cn_flags & ISDOTDOT != 0 {
                Some(NODES[dir].parent)
            } else {
                (1..N).find(|&i| NODES[i].parent == dir && NODES[i].name == name)
            };
            let Some(idx) = found else {
                if (cnp.cn_nameiop == CREATE || cnp.cn_nameiop == RENAME) && islast {
                    if !lockparent {
                        let _ = VOP_UNLOCK(dvp);
                        cnp.cn_flags |= PDIRUNLOCK;
                    }
                    return Err(Errno::EJUSTRETURN);
                }
                if cnp.cn_flags & MAKEENTRY != 0 && cnp.cn_nameiop != CREATE {
                    cache_enter(dvp, None, cnp);
                }
                return Err(Errno::ENOENT);
            };

            let mp = the_mount();
            let vp = if idx == dir {
                vref(dvp);
                dvp
            } else if cnp.cn_flags & ISDOTDOT != 0 {
                let _ = VOP_UNLOCK(dvp);
                cnp.cn_flags |= PDIRUNLOCK;
                let vp = vget_node(mp, idx)?;
                if lockparent && islast {
                    vn_lock(dvp, LK_EXCLUSIVE)?;
                    cnp.cn_flags &= !PDIRUNLOCK;
                }
                vp
            } else {
                let vp = vget_node(mp, idx)?;
                if !lockparent || !islast {
                    let _ = VOP_UNLOCK(dvp);
                    cnp.cn_flags |= PDIRUNLOCK;
                }
                vp
            };
            if cnp.cn_flags & MAKEENTRY != 0 {
                cache_enter(dvp, Some(vp), cnp);
            }
            *ap.a_vpp = Some(vp);
            Ok(())
        }

        fn testfs_getattr(ap: &mut VopGetattrArgs<'_>) -> Result<(), Errno> {
            let idx = node_of(ap.a_vp);
            let va = &mut *ap.a_vap;
            *va = crate::sys::vnode::Vattr::new();
            va.va_type = ap.a_vp.v_type.get();
            va.va_mode = 0o755;
            va.va_nlink = 1;
            va.va_fsid = 99;
            va.va_fileid = 100 + idx as u64;
            va.va_size = match NODES[idx].kind {
                Kind::Lnk(target) => target.len() as u64,
                _ => 512,
            };
            va.va_blocksize = 512;
            va.va_bytes = 512;
            Ok(())
        }

        fn testfs_readlink(ap: &mut VopReadlinkArgs<'_, '_>) -> Result<(), Errno> {
            let Kind::Lnk(target) = NODES[node_of(ap.a_vp)].kind else {
                return Err(Errno::EINVAL);
            };
            let mut buf = target.to_vec();
            uiomove(&mut buf, ap.a_uio)
        }

        /// One `struct dirent` record for `name`.
        fn dirent(fileno: u64, type_: u8, name: &[u8]) -> std::vec::Vec<u8> {
            let reclen = dirent_recsize(name.len());
            let mut rec = std::vec![0u8; reclen];
            rec[..8].copy_from_slice(&fileno.to_ne_bytes());
            rec[16..18].copy_from_slice(&(reclen as u16).to_ne_bytes());
            rec[18] = type_;
            rec[19] = name.len() as u8;
            rec[Dirent::NAME_OFFSET..Dirent::NAME_OFFSET + name.len()].copy_from_slice(name);
            rec
        }

        /// The whole directory in one call: `.`, `..` and the children.
        fn testfs_readdir(ap: &mut VopReaddirArgs<'_, '_>) -> Result<(), Errno> {
            let dir = node_of(ap.a_vp);
            if ap.a_uio.uio_offset != 0 {
                *ap.a_eofflag = 1;
                return Ok(());
            }
            let mut buf = dirent(100 + dir as u64, DT_DIR, b".");
            buf.extend(dirent(100 + NODES[dir].parent as u64, DT_DIR, b".."));
            for i in (1..N).filter(|&i| NODES[i].parent == dir) {
                let type_ = match NODES[i].kind {
                    Kind::Dir => DT_DIR,
                    Kind::Reg => DT_REG,
                    Kind::Lnk(_) => DT_LNK,
                };
                buf.extend(dirent(100 + i as u64, type_, NODES[i].name));
            }
            *ap.a_eofflag = 1;
            uiomove(&mut buf, ap.a_uio)
        }

        fn testfs_access(_ap: &mut VopAccessArgs<'_>) -> Result<(), Errno> {
            Ok(())
        }

        /// `vops` of testfs.
        pub(crate) static TESTFS_VOPS: Vops = Vops {
            vop_lock: Some(testfs_lock),
            vop_unlock: Some(testfs_unlock),
            vop_islocked: Some(testfs_islocked),
            vop_abortop: Some(vop_generic_abortop),
            vop_access: Some(testfs_access),
            vop_close: Some(|_| Ok(())),
            vop_getattr: Some(testfs_getattr),
            vop_inactive: Some(testfs_inactive),
            vop_lookup: Some(testfs_lookup),
            vop_open: Some(|_| Ok(())),
            vop_readdir: Some(testfs_readdir),
            vop_readlink: Some(testfs_readlink),
            vop_reclaim: Some(testfs_reclaim),
            vop_print: Some(|_| Ok(())),
            ..Vops::EMPTY
        };

        /// Mounts testfs as the root file system the way `main` does after `mountroot`: on the
        /// mount list with `MNT_ROOTFS`, its root as `rootvnode` and `p`'s current directory.
        pub(crate) fn mount_root(p: &Proc) -> &'static Mount {
            let mp = vfs_mount_alloc(None, &TESTFS_CONF);
            vfs_unbusy(mp);
            MP.store(ptr::from_ref(mp).cast_mut(), Ordering::SeqCst);
            // SAFETY: a new mount on no list.
            unsafe { MOUNTLIST.0.insert_tail(mp) };
            mp.mnt_flag.set(mp.mnt_flag.get() | MNT_ROOTFS);
            let Ok(root) = VFS_ROOT(mp) else {
                std::panic!("testfs: no root");
            };
            set_rootvnode(Some(root));
            p.fd().fd_cdir.set(Some(root));
            vref(root);
            let _ = VOP_UNLOCK(root);
            mp
        }

        /// `setup` plus testfs mounted as root: the guard, the thread and the mount.
        pub(crate) fn setup_root() -> (MutexGuard<'static, ()>, &'static Proc, &'static Mount) {
            let (guard, p) = setup();
            let mp = mount_root(p);
            (guard, p, mp)
        }

        /// A leaked fresh `Box`, for tests that want a `'static` buffer.
        pub(crate) fn leak<T>(t: T) -> &'static mut T {
            Box::leak(Box::new(t))
        }
    }

    /// Real memory, the process, file and vfs pools, empty vnode lists; returns the guard and a
    /// thread with credentials and a descriptor table (no current directory).
    pub(crate) fn setup() -> (MutexGuard<'static, ()>, &'static Proc) {
        let guard = setup_real_memory();
        crate::machine::cons::consinit();
        procinit();
        filedesc_init();
        NUMVNODES.store(0, Ordering::SeqCst);
        NUMCACHE.store(0, Ordering::SeqCst);
        NUMNEG.store(0, Ordering::SeqCst);
        // Device vnodes of an earlier test lived in the memory just reset.
        for chain in crate::kern::spec_vnops::SPECLISTH.0.iter() {
            chain.init();
        }
        // The file systems' pools and tables live in that memory too: let vfsinit's vfs_init
        // calls set them up again.
        #[cfg(feature = "ffs")]
        {
            crate::ufs::ffs::ffs_vfsops::FFS_INIT_DONE.store(false, Ordering::SeqCst);
            crate::ufs::ufs::ufs_vfsops::UFS_INIT_DONE.store(false, Ordering::SeqCst);
        }
        vfsinit();
        testfs::reset();
        set_rootvnode(None);

        let pr: &'static Process = Box::leak(Box::new(Process::new()));
        let p: &'static Proc = Box::leak(Box::new(Proc::new()));
        p.p_p.set(pr);
        pr.ps_mainproc.set(p);
        let cr = crget();
        p.p_ucred.set(cr);
        pr.ps_ucred.set(crhold(cr));
        let fdp = fdinit();
        pr.ps_fd.set(fdp);
        p.p_fd.set(fdp);
        (guard, p)
    }

    #[test]
    fn vattr_null_sets_every_field_to_vnoval() {
        let mut va = Vattr::new();
        va.va_type = VREG;
        va.va_vaflags = 7;
        vattr_null(&mut va);
        assert_eq!(va.va_type, VNON);
        assert_eq!(va.va_mode, Mode::MAX);
        assert_eq!(va.va_uid, Uid::MAX);
        assert_eq!(va.va_size, u64::MAX);
        assert_eq!(va.va_fsid, -1);
        assert_eq!(va.va_atime.tv_nsec, -1);
        assert_eq!(va.va_ctime.tv_sec, -1);
        assert_eq!(va.va_rdev, -1);
        assert_eq!(va.va_flags, u64::MAX);
        assert_eq!(va.va_vaflags, 0);
    }

    #[test]
    fn vaccess_checks_owner_then_group_then_others() {
        let _g = setup();
        let cr = crget();
        cr.cr_uid.set(1000);
        cr.cr_gid.set(10);
        cr.cr_ngroups.set(1);
        cr.cr_groups[0].set(10);
        // The owner's bits decide for the owner, even when others may.
        assert_eq!(
            vaccess(VREG, 0o077, 1000, 20, VREAD, cr),
            Err(Errno::EACCES)
        );
        assert_eq!(vaccess(VREG, 0o600, 1000, 20, VREAD | VWRITE, cr), Ok(()));
        // Then the group.
        assert_eq!(vaccess(VREG, 0o040, 0, 10, VREAD, cr), Ok(()));
        assert_eq!(vaccess(VREG, 0o040, 0, 10, VWRITE, cr), Err(Errno::EACCES));
        // Then everyone else.
        assert_eq!(vaccess(VDIR, 0o001, 0, 0, VEXEC, cr), Ok(()));
        // Root reads anything, but executes only something executable (directories search).
        cr.cr_uid.set(0);
        assert_eq!(vaccess(VREG, 0o000, 1, 1, VREAD | VWRITE, cr), Ok(()));
        assert_eq!(vaccess(VREG, 0o644, 1, 1, VEXEC, cr), Err(Errno::EACCES));
        assert_eq!(vaccess(VDIR, 0o000, 1, 1, VEXEC, cr), Ok(()));
    }

    #[test]
    fn getnewvnode_puts_the_vnode_on_its_mount() {
        let (_g, _p, mp) = testfs::setup_root();
        let before = NUMVNODES.load(Ordering::SeqCst);
        let vp = getnewvnode(VT_NON, Some(mp), &testfs::TESTFS_VOPS).unwrap();
        assert_eq!(vp.v_usecount.get(), 1);
        assert_eq!(vp.v_type.get(), VNON);
        assert!(vp.v_mount.get().is_some_and(|m| ptr::eq(m, mp)));
        assert!(mp.mnt_vnodelist.iter().any(|v| ptr::eq(v, vp)));
        assert_eq!(NUMVNODES.load(Ordering::SeqCst), before + 1);

        insmntque(vp, None);
        assert!(vp.v_mount.get().is_none());
        assert!(!mp.mnt_vnodelist.iter().any(|v| ptr::eq(v, vp)));
    }

    #[test]
    fn use_counts_and_the_free_list() {
        let (_g, _p, mp) = testfs::setup_root();
        let vp = testfs::vget_node(mp, testfs::B).unwrap();
        assert_eq!(vp.v_usecount.get(), 1);
        assert_eq!(VOP_ISLOCKED(vp), 1);

        vref(vp);
        assert_eq!(vp.v_usecount.get(), 2);
        // vput of a vnode still referenced only unlocks it.
        vput(vp);
        assert_eq!(vp.v_usecount.get(), 1);
        assert_eq!(VOP_ISLOCKED(vp), 0);
        assert_eq!(testfs::INACTIVE.load(Ordering::SeqCst), 0);

        // The last vrele locks, deactivates (which unlocks) and frees the vnode, which keeps its
        // identity on the free list.
        assert!(vrele(vp));
        assert_eq!(vp.v_usecount.get(), 0);
        assert_eq!(testfs::INACTIVE.load(Ordering::SeqCst), 1);
        assert!(!testfs::any_locked());
        assert!(vp.v_bioflag.get() & VBIOONFREELIST != 0);
        assert!(VNODE_FREE_LIST.0.iter().any(|v| ptr::eq(v, vp)));
        assert_eq!(testfs::node_of(vp), testfs::B);

        // vget takes it back off the free list, referenced and locked.
        let again = testfs::vget_node(mp, testfs::B).unwrap();
        assert!(ptr::eq(again, vp));
        assert_eq!(vp.v_usecount.get(), 1);
        assert!(vp.v_bioflag.get() & VBIOONFREELIST == 0);
        assert!(!VNODE_FREE_LIST.0.iter().any(|v| ptr::eq(v, vp)));
        vput(vp);
        assert_eq!(vp.v_usecount.get(), 0);
        assert_eq!(testfs::INACTIVE.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn vhold_moves_a_free_vnode_to_the_hold_list() {
        let (_g, _p, mp) = testfs::setup_root();
        let vp = testfs::vget_node(mp, testfs::B).unwrap();
        vput(vp);
        assert!(VNODE_FREE_LIST.0.iter().any(|v| ptr::eq(v, vp)));
        vhold(vp);
        assert!(VNODE_HOLD_LIST.0.iter().any(|v| ptr::eq(v, vp)));
        assert!(!VNODE_FREE_LIST.0.iter().any(|v| ptr::eq(v, vp)));
        vdrop(vp);
        assert!(VNODE_FREE_LIST.0.iter().any(|v| ptr::eq(v, vp)));
        assert!(!VNODE_HOLD_LIST.0.iter().any(|v| ptr::eq(v, vp)));
    }

    #[test]
    fn vgone_reclaims_and_leaves_a_dead_vnode() {
        let (_g, _p, mp) = testfs::setup_root();
        let vp = testfs::vget_node(mp, testfs::C).unwrap();
        let _ = VOP_UNLOCK(vp);
        let id = vp.v_id.get();

        // An active vnode: closed, deactivated, reclaimed; the reference survives.
        vgone(vp);
        assert_eq!(testfs::RECLAIM.load(Ordering::SeqCst), 1);
        assert_eq!(testfs::INACTIVE.load(Ordering::SeqCst), 1);
        assert_eq!(vp.v_type.get(), VBAD);
        assert!(ptr::eq(vp.op(), &DEAD_VOPS));
        assert!(vp.v_mount.get().is_none());
        assert!(vp.v_data.get().is_null());
        assert_ne!(vp.v_id.get(), id, "cache_purge gives a new capability");
        assert!(testfs::vnode_of(testfs::C).is_none());
        assert_eq!(vp.v_usecount.get(), 1);

        // The dead vnode's last release puts it at the head of the free list, to be recycled
        // first.
        assert!(vrele(vp));
        assert!(VNODE_FREE_LIST.0.first().is_some_and(|v| ptr::eq(v, vp)));
    }

    #[test]
    fn getnewvnode_recycles_from_the_free_list_past_maxvnodes() {
        let (_g, _p, mp) = testfs::setup_root();
        let vp = testfs::vget_node(mp, testfs::B).unwrap();
        vput(vp);
        MAXVNODES.store(1, Ordering::SeqCst);
        let before = NUMVNODES.load(Ordering::SeqCst);
        let nvp = getnewvnode(VT_NON, None, &testfs::TESTFS_VOPS).unwrap();
        assert!(ptr::eq(nvp, vp), "the free vnode is recycled");
        assert_eq!(NUMVNODES.load(Ordering::SeqCst), before);
        assert_eq!(testfs::RECLAIM.load(Ordering::SeqCst), 1);
        assert!(testfs::vnode_of(testfs::B).is_none());
        assert_eq!(nvp.v_usecount.get(), 1);
        assert!(nvp.v_mount.get().is_none());
    }

    #[test]
    fn vflush_counts_busy_vnodes_unless_forced() {
        let (_g, _p, mp) = testfs::setup_root();
        let busy = testfs::vget_node(mp, testfs::B).unwrap();
        let _ = VOP_UNLOCK(busy);
        let idle = testfs::vget_node(mp, testfs::C).unwrap();
        vput(idle);

        // The root (two references) and B are busy; C is idle and goes.
        let root = crate::kern::vfs_init::rootvnode();
        assert_eq!(vflush(mp, root, 0), Err(Errno::EBUSY));
        assert!(testfs::vnode_of(testfs::C).is_none());
        assert!(testfs::vnode_of(testfs::B).is_some());

        assert_eq!(vflush(mp, root, FORCECLOSE), Ok(()));
        assert!(testfs::vnode_of(testfs::B).is_none());
        assert_eq!(busy.v_type.get(), VBAD);
        // Only the skipped root is left on the mount.
        assert_eq!(mp.mnt_vnodelist.iter().count(), 1);
    }

    #[test]
    fn vfs_busy_fails_on_an_unmounting_file_system() {
        let (_g, _p, mp) = testfs::setup_root();
        assert_eq!(vfs_busy(mp, VB_READ | VB_NOWAIT), Ok(()));
        assert!(vfs_isbusy(mp));
        vfs_unbusy(mp);
        assert!(!vfs_isbusy(mp));
        mp.mnt_flag.set(mp.mnt_flag.get() | MNT_UNMOUNT);
        assert_eq!(vfs_busy(mp, VB_READ | VB_NOWAIT), Err(Errno::EBUSY));
        assert!(!vfs_isbusy(mp));
        mp.mnt_flag.set(mp.mnt_flag.get() & !MNT_UNMOUNT);
    }

    #[test]
    fn vfs_getnewfsid_is_unique_among_mounts() {
        let (_g, _p, mp) = testfs::setup_root();
        vfs_getnewfsid(mp);
        let fsid = mp.mnt_stat.get().f_fsid;
        assert!(vfs_getvfs(&fsid).is_some_and(|m| ptr::eq(m, mp)));
        let mp2 = vfs_mount_alloc(None, &testfs::TESTFS_CONF);
        vfs_unbusy(mp2);
        // SAFETY: a new mount on no list.
        unsafe { MOUNTLIST.0.insert_tail(mp2) };
        vfs_getnewfsid(mp2);
        assert_ne!(mp2.mnt_stat.get().f_fsid, fsid);
        assert_eq!(&mp2.mnt_stat.get().f_fstypename[..7], b"testfs\0");
    }

    #[test]
    fn checkalias_is_not_reached_for_regular_vnodes() {
        let (_g, _p, mp) = testfs::setup_root();
        let vp = testfs::vget_node(mp, testfs::B).unwrap();
        assert!(checkalias(vp, 0, Some(mp)).is_none());
        assert!(vp.v_specinfo().is_none());
        vput(vp);
    }

    /// What `vfs_export` and `vfs_export_lookup` do with the export list (`NFSSERVER`): the entries
    /// of the radix tree, their masks, the default export and `MNT_DELEXPORT`.
    #[cfg(feature = "nfsserver")]
    pub(crate) mod exports {
        use std::boxed::Box;
        use std::{assert, assert_eq};

        use super::testfs;
        use super::*;
        use crate::sys::mount::{
            ExportArgs, MNT_DEFEXPORTED, MNT_DELEXPORT, MNT_EXPORTED, MNT_EXRDONLY, Netexport,
        };
        use crate::sys::syslimits::NGROUPS_MAX;
        use crate::sys::ucred::Xucred;

        /// A `sockaddr_in` for `a` in a 32-byte buffer (the radix code reads keys for its
        /// `max_keylen`), at a stable address: a "user" address for `copyin`.
        pub(crate) fn sin(family: u8, a: [u8; 4]) -> &'static [u8; 32] {
            let mut k = [0u8; 32];
            k[0] = 16;
            k[1] = family;
            k[4..8].copy_from_slice(&a);
            Box::leak(Box::new(k))
        }

        /// `struct export_args` for the address and mask (`None`: an empty one).
        pub(crate) fn args(
            flags: i32,
            anon: u32,
            addr: Option<&'static [u8; 32]>,
            mask: Option<&'static [u8; 32]>,
        ) -> ExportArgs {
            ExportArgs {
                ex_flags: flags,
                ex_root: 0,
                ex_anon: Xucred {
                    cr_uid: anon,
                    cr_gid: anon,
                    cr_ngroups: 0,
                    cr_groups: [0; NGROUPS_MAX],
                },
                ex_addr: addr.map_or(0, |a| a.as_ptr() as usize),
                ex_addrlen: if addr.is_some() { 16 } else { 0 },
                ex_mask: mask.map_or(0, |a| a.as_ptr() as usize),
                ex_masklen: if mask.is_some() { 16 } else { 0 },
            }
        }

        /// What `VFS_CHECKEXP` answers a client at `a` (an `AF_INET` address mbuf; the caller has
        /// run `mbinit_again`): the export flags and the anonymous credentials' uid, or the error.
        pub(crate) fn check_export(mp: &'static Mount, a: [u8; 4]) -> Result<(i32, u32), Errno> {
            use crate::kern::uipc_mbuf::m_get;
            use crate::sys::mbuf::{M_DONTWAIT, MT_SONAME, mtod};
            use crate::sys::mount::VFS_CHECKEXP;

            let m = m_get(M_DONTWAIT, MT_SONAME).ok_or(Errno::ENOBUFS)?;
            let k = sin(2, a);
            // SAFETY: an mbuf's data area holds a `sockaddr_in`; `mtod` points at it.
            unsafe { ptr::copy_nonoverlapping(k.as_ptr(), mtod::<u8>(m), 16) };
            m.m_len().set(16);
            let mut exflags = 0;
            let mut anon: *const Ucred = ptr::null();
            VFS_CHECKEXP(mp, m, &mut exflags, &mut anon)?;
            // SAFETY: `anon` is the entry's `netc_anon`, which lives as long as the list.
            Ok((exflags, unsafe { (*anon).cr_uid.get() }))
        }

        /// The entry the client `a` matches, as `(exflags, anon uid)`.
        fn lookup(mp: &'static Mount, nep: &'static Netexport, a: [u8; 4]) -> Option<(i32, u32)> {
            let key = sin(2, a);
            // SAFETY: a `sockaddr_in` in a buffer longer than `max_keylen`; `rn_init` ran in
            // `vfs_init`.
            let np = unsafe { export_lookup(mp, nep, Some(key.as_ptr())) }?;
            Some((np.netc_exflags.get(), np.netc_anon.cr_uid.get()))
        }

        fn fresh() -> (
            std::sync::MutexGuard<'static, ()>,
            &'static Mount,
            &'static Netexport,
        ) {
            let (g, _p) = setup();
            let mp = vfs_mount_alloc(None, &testfs::TESTFS_CONF);
            (g, mp, Box::leak(Box::new(Netexport::new())))
        }

        #[test]
        fn an_exported_network_matches_its_hosts_only() {
            let (_g, mp, nep) = fresh();
            let net = sin(2, [10, 1, 0, 0]);
            let mask = sin(2, [255, 255, 0, 0]);
            let ro = MNT_EXPORTED | MNT_EXRDONLY;
            assert_eq!(
                vfs_export(mp, nep, &args(ro, 32767, Some(net), Some(mask))),
                Ok(())
            );
            assert!(mp.mnt_flag.get() & MNT_EXPORTED != 0);
            assert!(nep.ne_rtable_inet.get().is_some());

            assert_eq!(lookup(mp, nep, [10, 1, 2, 3]), Some((ro, 32767)));
            assert_eq!(lookup(mp, nep, [10, 1, 255, 255]), Some((ro, 32767)));
            assert_eq!(lookup(mp, nep, [10, 2, 0, 1]), None);
            assert_eq!(lookup(mp, nep, [192, 168, 0, 1]), None);
        }

        #[test]
        fn the_most_specific_entry_wins() {
            let (_g, mp, nep) = fresh();
            let net = sin(2, [10, 0, 0, 0]);
            let mask = sin(2, [255, 0, 0, 0]);
            let host = sin(2, [10, 0, 0, 7]);
            let ro = MNT_EXPORTED | MNT_EXRDONLY;
            vfs_export(mp, nep, &args(ro, 100, Some(net), Some(mask))).unwrap();
            vfs_export(mp, nep, &args(MNT_EXPORTED, 200, Some(host), None)).unwrap();
            assert_eq!(lookup(mp, nep, [10, 0, 0, 7]), Some((MNT_EXPORTED, 200)));
            assert_eq!(lookup(mp, nep, [10, 0, 0, 8]), Some((ro, 100)));
        }

        #[test]
        fn a_duplicate_entry_and_a_bad_address_are_refused() {
            let (_g, mp, nep) = fresh();
            let host = sin(2, [10, 0, 0, 7]);
            vfs_export(mp, nep, &args(MNT_EXPORTED, 1, Some(host), None)).unwrap();
            assert_eq!(
                vfs_export(mp, nep, &args(MNT_EXPORTED, 2, Some(host), None)),
                Err(Errno::EPERM)
            );
            // The first one is still the entry.
            assert_eq!(lookup(mp, nep, [10, 0, 0, 7]), Some((MNT_EXPORTED, 1)));

            // AF_UNSPEC is not an address family the export list knows.
            let other = sin(0, [10, 0, 0, 9]);
            assert_eq!(
                vfs_export(mp, nep, &args(MNT_EXPORTED, 1, Some(other), None)),
                Err(Errno::EINVAL)
            );
            // Lengths past an mbuf, or negative.
            let mut a = args(MNT_EXPORTED, 1, Some(host), None);
            a.ex_addrlen = MLEN as i32 + 1;
            assert_eq!(vfs_export(mp, nep, &a), Err(Errno::EINVAL));
            a.ex_addrlen = -1;
            assert_eq!(vfs_export(mp, nep, &a), Err(Errno::EINVAL));
            a.ex_addrlen = 16;
            a.ex_masklen = -1;
            assert_eq!(vfs_export(mp, nep, &a), Err(Errno::EINVAL));
            // A bad address pointer is the copyin's EFAULT.
            let mut a = args(MNT_EXPORTED, 1, None, None);
            a.ex_addrlen = 16;
            assert_eq!(vfs_export(mp, nep, &a), Err(Errno::EFAULT));
        }

        #[test]
        fn the_default_export_serves_everyone_else() {
            let (_g, mp, nep) = fresh();
            // Not exported at all: nothing, whatever the lists hold.
            assert_eq!(lookup(mp, nep, [10, 0, 0, 1]), None);
            let host = sin(2, [10, 0, 0, 7]);
            vfs_export(mp, nep, &args(MNT_EXPORTED, 1, Some(host), None)).unwrap();
            assert_eq!(lookup(mp, nep, [172, 16, 0, 1]), None);

            let ro = MNT_EXPORTED | MNT_EXRDONLY;
            vfs_export(mp, nep, &args(ro, 77, None, None)).unwrap();
            assert!(mp.mnt_flag.get() & MNT_DEFEXPORTED != 0);
            assert_eq!(lookup(mp, nep, [172, 16, 0, 1]), Some((ro, 77)));
            assert_eq!(lookup(mp, nep, [10, 0, 0, 7]), Some((MNT_EXPORTED, 1)));
            // No address at all (a NULL `nam`): only the default.
            assert!(vfs_export_lookup(mp, nep, None).is_some_and(|np| np.netc_exflags.get() == ro));
            // Only one default.
            assert_eq!(
                vfs_export(mp, nep, &args(ro, 78, None, None)),
                Err(Errno::EPERM)
            );
        }

        #[test]
        fn mnt_delexport_empties_the_list() {
            let (_g, mp, nep) = fresh();
            let net = sin(2, [10, 1, 0, 0]);
            let mask = sin(2, [255, 255, 0, 0]);
            vfs_export(mp, nep, &args(MNT_EXPORTED, 5, Some(net), Some(mask))).unwrap();
            vfs_export(mp, nep, &args(MNT_EXPORTED, 6, None, None)).unwrap();
            assert!(lookup(mp, nep, [10, 1, 0, 9]).is_some());

            vfs_export(mp, nep, &args(MNT_DELEXPORT, 0, None, None)).unwrap();
            assert_eq!(mp.mnt_flag.get() & (MNT_EXPORTED | MNT_DEFEXPORTED), 0);
            assert!(nep.ne_rtable_inet.get().is_none());
            assert_eq!(lookup(mp, nep, [10, 1, 0, 9]), None);
            assert!(vfs_export_lookup(mp, nep, None).is_none());

            // And the list can be built again.
            vfs_export(mp, nep, &args(MNT_EXPORTED, 8, Some(net), Some(mask))).unwrap();
            assert_eq!(lookup(mp, nep, [10, 1, 0, 9]), Some((MNT_EXPORTED, 8)));
            // Delete and add in one call, as `mountd` reloading its exports does.
            vfs_export(
                mp,
                nep,
                &args(MNT_DELEXPORT | MNT_EXPORTED, 9, Some(net), Some(mask)),
            )
            .unwrap();
            assert_eq!(
                lookup(mp, nep, [10, 1, 0, 9]),
                Some((MNT_DELEXPORT | MNT_EXPORTED, 9))
            );
        }
    }
}
/* </TESTS> */
