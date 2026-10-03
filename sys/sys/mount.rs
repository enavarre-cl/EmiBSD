/*	$OpenBSD: mount.h,v 1.154 2026/06/10 00:04:38 beck Exp $	*/
/*	$NetBSD: mount.h,v 1.48 1996/02/18 11:55:47 fvdl Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1989, 1991, 1993
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
 *	@(#)mount.h	8.15 (Berkeley) 7/14/94
 */
/* </LICENSES> */

//! `<sys/mount.h>`: the mounted file system (`struct mount`), the operations a file system
//! type provides (`struct vfsops`, called through the `VFS_*` macros), its configuration
//! entry (`struct vfsconf`), `struct statfs` and the `MNT_*` flags, file handles, the
//! `CTL_VFS` names and the buffer cache statistics.
//!
//! Upstream: sys/sys/mount.h @ 3ce1f3f79392
//!
//! A `struct mount` is `malloc(M_MOUNT)`ed by `vfs_mount_alloc` and freed when its last
//! reference (`mnt_refs`) goes; it is handed around as `&'static Mount`, the `crget`/`crfree`
//! idiom of `docs/C_TO_RUST.md`.
//!
//! ## How a file system plugs in
//! - It fills a `static` [`Vfsops`] and an entry of `vfsconflist[]` (`kern/vfs_init.rs`).
//!   `vfs_root`, `vfs_vget` and `vfs_fhtovp` return the vnode where the C fills `*vpp`;
//!   `vfs_mount` receives the kernel copy of the user's arguments as a byte slice of
//!   `vfc_datasize` bytes, which the file system reads as its own `*_args` structure.
//! - Its per-mount data hangs from `mnt_data` (`*mut c_void`, as in C).
//!
//! ## Deviations
//! - `mnt_op` and `mnt_vfc` are `Option`s (a zeroed allocation has neither) with accessors
//!   that assert them; `mnt_stat` is a `Cell<Statfs>`, read with `get` and changed with
//!   [`Mount::update_stat`].
//! - `struct statfs` names the holes the C compiler leaves (`_pad0` after `f_iosize`, `_pad1`
//!   before `mount_info`), so the structure is plain data that `copyout` may read whole;
//!   `union mount_info` is its 160 bytes, 8-aligned: the per-filesystem views (`ufs_args`,
//!   `mfs_args`, ..., the `export_args` they embed) come with their file systems.
//! - `struct vfsconf`'s `vfc_refcount` is atomic (`atomic_inc_int` in C).
//! - `VFS_*` are functions with the macros' names (`#[allow(non_snake_case)]`).
//! - `struct netcred`/`struct netexport` need `net/radix.h` and `NFSSERVER`, neither of
//!   which is configured; `vfs_export` takes the export table as an opaque pointer.
//! - The user-level prototypes (`mount`, `statfs`, ...) are not kernel material; the kernel
//!   ones are their functions in `vfs_subr.rs`, `vfs_init.rs` and `vfs_syscalls.rs`.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::AtomicU32;

use crate::kern::subr_prf::panic;
use crate::machine::copy::AbiPod;
use crate::queue_adapter;
use crate::sys::errno::Errno;
use crate::sys::mbuf::Mbuf;
use crate::sys::namei::Nameidata;
use crate::sys::proc::Proc;
use crate::sys::queue::{SlistEntry, TailqEntry, TailqHead};
use crate::sys::refcnt::Refcnt;
use crate::sys::rwlock::Rwlock;
use crate::sys::sysctl::Sysctlfn;
use crate::sys::types::{Ino, Uid};
use crate::sys::ucred::Ucred;
use crate::sys::vnode::{VMntvnodes, Vnode};

/// `fsid_t`: file system id type.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Fsid {
    /// `val`.
    pub val: [i32; 2],
}

/// `MAXFIDSZ`.
pub const MAXFIDSZ: usize = 16;

/// `struct fid`: file identifier. These are unique per filesystem on a single machine.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Fid {
    /// `fid_len`: length of data in bytes.
    pub fid_len: u16,
    /// `fid_reserved`: force longword alignment.
    pub fid_reserved: u16,
    /// `fid_data`: data (variable length).
    pub fid_data: [u8; MAXFIDSZ],
}

/// `MFSNAMELEN`: length of fs type name, including nul.
pub const MFSNAMELEN: usize = 16;
/// `MNAMELEN`: length of buffer for returned name.
pub const MNAMELEN: usize = 90;

/// `union mount_info`: per-filesystem mount options (see the module's deviations).
#[repr(C, align(8))]
#[derive(Clone, Copy)]
pub struct MountInfo {
    /// `__align`: 64-bit alignment and room to grow.
    pub __align: [u8; 160],
}

/// `struct statfs`: file system statistics, with mount options and statvfs fields.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Statfs {
    /// `f_flags`: copy of mount flags.
    pub f_flags: u32,
    /// `f_bsize`: file system block size.
    pub f_bsize: u32,
    /// `f_iosize`: optimal transfer block size.
    pub f_iosize: u32,
    /// The hole before `f_blocks`.
    pub _pad0: u32,
    /// `f_blocks`: total data blocks in file system (unit is `f_bsize`).
    pub f_blocks: u64,
    /// `f_bfree`: free blocks in fs.
    pub f_bfree: u64,
    /// `f_bavail`: free blocks avail to non-superuser.
    pub f_bavail: i64,
    /// `f_files`: total file nodes in file system.
    pub f_files: u64,
    /// `f_ffree`: free file nodes in fs.
    pub f_ffree: u64,
    /// `f_favail`: free file nodes avail to non-root.
    pub f_favail: i64,
    /// `f_syncwrites`: count of sync writes since mount.
    pub f_syncwrites: u64,
    /// `f_syncreads`: count of sync reads since mount.
    pub f_syncreads: u64,
    /// `f_asyncwrites`: count of async writes since mount.
    pub f_asyncwrites: u64,
    /// `f_asyncreads`: count of async reads since mount.
    pub f_asyncreads: u64,
    /// `f_fsid`: file system id.
    pub f_fsid: Fsid,
    /// `f_namemax`: maximum filename length.
    pub f_namemax: u32,
    /// `f_owner`: user that mounted the file system.
    pub f_owner: Uid,
    /// `f_ctime`: last mount \[-u\] time.
    pub f_ctime: u64,
    /// `f_fstypename`: fs type name.
    pub f_fstypename: [u8; MFSNAMELEN],
    /// `f_mntonname`: directory on which mounted.
    pub f_mntonname: [u8; MNAMELEN],
    /// `f_mntfromname`: mounted file system.
    pub f_mntfromname: [u8; MNAMELEN],
    /// `f_mntfromspec`: special for mount request.
    pub f_mntfromspec: [u8; MNAMELEN],
    /// The hole before `mount_info`.
    pub _pad1: [u8; 2],
    /// `mount_info`: per-filesystem mount options.
    pub mount_info: MountInfo,
}

impl Statfs {
    /// A zeroed `struct statfs`.
    pub const fn new() -> Self {
        Self {
            f_flags: 0,
            f_bsize: 0,
            f_iosize: 0,
            _pad0: 0,
            f_blocks: 0,
            f_bfree: 0,
            f_bavail: 0,
            f_files: 0,
            f_ffree: 0,
            f_favail: 0,
            f_syncwrites: 0,
            f_syncreads: 0,
            f_asyncwrites: 0,
            f_asyncreads: 0,
            f_fsid: Fsid { val: [0; 2] },
            f_namemax: 0,
            f_owner: 0,
            f_ctime: 0,
            f_fstypename: [0; MFSNAMELEN],
            f_mntonname: [0; MNAMELEN],
            f_mntfromname: [0; MNAMELEN],
            f_mntfromspec: [0; MNAMELEN],
            _pad1: [0; 2],
            mount_info: MountInfo { __align: [0; 160] },
        }
    }
}

impl Default for Statfs {
    fn default() -> Self {
        Self::new()
    }
}

// SAFETY: `#[repr(C)]` integers and byte arrays, the C compiler's holes named (`_pad0`,
// `_pad1`), no implicit padding (the compile-time checks below pin the layout).
unsafe impl AbiPod for Statfs {}

/// `MOUNT_FFS`: UNIX "Fast" Filesystem.
pub const MOUNT_FFS: &[u8] = b"ffs";
/// `MOUNT_UFS`: for compatibility.
pub const MOUNT_UFS: &[u8] = MOUNT_FFS;
/// `MOUNT_NFS`: Network Filesystem.
pub const MOUNT_NFS: &[u8] = b"nfs";
/// `MOUNT_MFS`: Memory Filesystem.
pub const MOUNT_MFS: &[u8] = b"mfs";
/// `MOUNT_MSDOS`: MSDOS Filesystem.
pub const MOUNT_MSDOS: &[u8] = b"msdos";
/// `MOUNT_AFS`: Andrew Filesystem.
pub const MOUNT_AFS: &[u8] = b"afs";
/// `MOUNT_CD9660`: ISO9660 (aka CDROM) Filesystem.
pub const MOUNT_CD9660: &[u8] = b"cd9660";
/// `MOUNT_EXT2FS`: Second Extended Filesystem.
pub const MOUNT_EXT2FS: &[u8] = b"ext2fs";
/// `MOUNT_NCPFS`: NetWare Network File System.
pub const MOUNT_NCPFS: &[u8] = b"ncpfs";
/// `MOUNT_NTFS`: NTFS.
pub const MOUNT_NTFS: &[u8] = b"ntfs";
/// `MOUNT_UDF`: UDF.
pub const MOUNT_UDF: &[u8] = b"udf";
/// `MOUNT_TMPFS`: tmpfs.
pub const MOUNT_TMPFS: &[u8] = b"tmpfs";
/// `MOUNT_FUSEFS`: FUSE.
pub const MOUNT_FUSEFS: &[u8] = b"fuse";

/// `struct mount`: structure per mounted file system. Each mounted file system has an array of
/// operations and an instance record. The file systems are put on a doubly linked list.
pub struct Mount {
    /// `mnt_list`: mount list.
    pub mnt_list: TailqEntry<Mount>,
    /// `mnt_dounmount`: unmount work queue.
    pub mnt_dounmount: SlistEntry<Mount>,
    /// `mnt_op`: operations on fs.
    pub mnt_op: Cell<Option<&'static Vfsops>>,
    /// `mnt_vfc`: configuration info.
    pub mnt_vfc: Cell<Option<&'static Vfsconf>>,
    /// `mnt_vnodecovered`: vnode we mounted on.
    pub mnt_vnodecovered: Cell<Option<&'static Vnode>>,
    /// `mnt_syncer`: syncer vnode.
    pub mnt_syncer: Cell<Option<&'static Vnode>>,
    /// `mnt_vnodelist`: list of vnodes this mount.
    pub mnt_vnodelist: TailqHead<VMntvnodes>,
    /// `mnt_lock`: mount structure lock.
    pub mnt_lock: Rwlock,
    /// `mnt_refs`.
    pub mnt_refs: Refcnt,
    /// `mnt_flag`: flags.
    pub mnt_flag: Cell<i32>,
    /// `mnt_stat`: cache of filesystem stats.
    pub mnt_stat: Cell<Statfs>,
    /// `mnt_data`: private data.
    pub mnt_data: Cell<*mut c_void>,
}

// SAFETY: the members are changed under the kernel lock or `mnt_lock` (`vfs_busy`), as in
// C; the kernel runs one CPU.
unsafe impl Sync for Mount {}

impl Mount {
    /// A zeroed mount, as `malloc(M_MOUNT, M_ZERO)` returns it.
    pub const fn new() -> Self {
        Self {
            mnt_list: TailqEntry::new(),
            mnt_dounmount: SlistEntry::new(),
            mnt_op: Cell::new(None),
            mnt_vfc: Cell::new(None),
            mnt_vnodecovered: Cell::new(None),
            mnt_syncer: Cell::new(None),
            mnt_vnodelist: TailqHead::new(),
            mnt_lock: Rwlock::new("vfslock"),
            mnt_refs: Refcnt::new(),
            mnt_flag: Cell::new(0),
            mnt_stat: Cell::new(Statfs::new()),
            mnt_data: Cell::new(ptr::null_mut()),
        }
    }

    /// `mp->mnt_op`, which `vfs_mount_alloc` set.
    pub fn op(&self) -> &'static Vfsops {
        match self.mnt_op.get() {
            Some(op) => op,
            None => panic(format_args!("mount {:p}: no mnt_op", self)),
        }
    }

    /// `mp->mnt_vfc`, which `vfs_mount_alloc` set.
    pub fn vfc(&self) -> &'static Vfsconf {
        match self.mnt_vfc.get() {
            Some(vfc) => vfc,
            None => panic(format_args!("mount {:p}: no mnt_vfc", self)),
        }
    }

    /// Changes `mp->mnt_stat` in place.
    pub fn update_stat(&self, f: impl FnOnce(&mut Statfs)) {
        let mut sp = self.mnt_stat.get();
        f(&mut sp);
        self.mnt_stat.set(sp);
    }

    /// `mp->mnt_stat.f_mntonname`, without the NUL padding.
    pub fn mntonname(&self) -> ([u8; MNAMELEN], usize) {
        let name = self.mnt_stat.get().f_mntonname;
        let len = name.iter().position(|&c| c == 0).unwrap_or(MNAMELEN);
        (name, len)
    }
}

impl Default for Mount {
    fn default() -> Self {
        Self::new()
    }
}

queue_adapter!(
    /// `TAILQ_HEAD(mntlist, mount)`: the mounted file systems, through `mnt_list`.
    pub MntList: Mount, mnt_list => TailqEntry<Mount>
);

queue_adapter!(
    /// `SLIST_HEAD(, mount)`: `dounmount`'s work queue, through `mnt_dounmount`.
    pub MntDounmount: Mount, mnt_dounmount => SlistEntry<Mount>
);

/// `MNT_RDONLY`: read only filesystem.
pub const MNT_RDONLY: i32 = 0x0000_0001;
/// `MNT_SYNCHRONOUS`: file system written synchronously.
pub const MNT_SYNCHRONOUS: i32 = 0x0000_0002;
/// `MNT_NOEXEC`: can't exec from filesystem.
pub const MNT_NOEXEC: i32 = 0x0000_0004;
/// `MNT_NOSUID`: don't honor setuid bits on fs.
pub const MNT_NOSUID: i32 = 0x0000_0008;
/// `MNT_NODEV`: don't interpret special files.
pub const MNT_NODEV: i32 = 0x0000_0010;
/// `MNT_NOPERM`: don't enforce permission checks.
pub const MNT_NOPERM: i32 = 0x0000_0020;
/// `MNT_ASYNC`: file system written asynchronously.
pub const MNT_ASYNC: i32 = 0x0000_0040;
/// `MNT_WXALLOWED`: filesystem allows W|X mappings.
pub const MNT_WXALLOWED: i32 = 0x0000_0800;

/// `MNT_EXRDONLY`: exported read only.
pub const MNT_EXRDONLY: i32 = 0x0000_0080;
/// `MNT_EXPORTED`: file system is exported.
pub const MNT_EXPORTED: i32 = 0x0000_0100;
/// `MNT_DEFEXPORTED`: exported to the world.
pub const MNT_DEFEXPORTED: i32 = 0x0000_0200;
/// `MNT_EXPORTANON`: use anon uid mapping for everyone.
pub const MNT_EXPORTANON: i32 = 0x0000_0400;

/// `MNT_LOCAL`: filesystem is stored locally.
pub const MNT_LOCAL: i32 = 0x0000_1000;
/// `MNT_QUOTA`: quotas are enabled on filesystem.
pub const MNT_QUOTA: i32 = 0x0000_2000;
/// `MNT_ROOTFS`: identifies the root filesystem.
pub const MNT_ROOTFS: i32 = 0x0000_4000;

/// `MNT_NOATIME`: don't update access times on fs.
pub const MNT_NOATIME: i32 = 0x0000_8000;

/// `MNT_VISFLAGMASK`: mask of flags that are visible to statfs().
pub const MNT_VISFLAGMASK: i32 = 0x0400_ffff;

/// `MNT_BITS`: the `%b` description of the mount flags.
pub const MNT_BITS: &[u8] = b"\x10\x01RDONLY\x02SYNCHRONOUS\x03NOEXEC\x04NOSUID\x05NODEV\x06NOPERM\
\x07ASYNC\x08EXRDONLY\x09EXPORTED\x0aDEFEXPORTED\x0bEXPORTANON\
\x0cWXALLOWED\x0dLOCAL\x0eQUOTA\x0fROOTFS\x10NOATIME\x11UPDATE\
\x12DELEXPORT\x13RELOAD\x14FORCE\x15STALLED\x16SWAPPABLE\x19UNMOUNT\
\x1aWANTRDWR\x1bSOFTDEP\x1cDOOMED";

/// `MNT_UPDATE`: not a real mount, just an update.
pub const MNT_UPDATE: i32 = 0x0001_0000;
/// `MNT_DELEXPORT`: delete export host lists.
pub const MNT_DELEXPORT: i32 = 0x0002_0000;
/// `MNT_RELOAD`: reload filesystem data.
pub const MNT_RELOAD: i32 = 0x0004_0000;
/// `MNT_FORCE`: force unmount or readonly change.
pub const MNT_FORCE: i32 = 0x0008_0000;
/// `MNT_STALLED`: filesystem stalled.
pub const MNT_STALLED: i32 = 0x0010_0000;
/// `MNT_SWAPPABLE`: filesystem can be used for swap.
pub const MNT_SWAPPABLE: i32 = 0x0020_0000;
/// `MNT_UNMOUNT`: unmount in progress.
pub const MNT_UNMOUNT: i32 = 0x0100_0000;
/// `MNT_WANTRDWR`: want upgrade to read/write.
pub const MNT_WANTRDWR: i32 = 0x0200_0000;
/// `MNT_SOFTDEP`: soft dependencies being done - now ignored.
pub const MNT_SOFTDEP: i32 = 0x0400_0000;
/// `MNT_DOOMED`: device behind filesystem is gone.
pub const MNT_DOOMED: i32 = 0x0800_0000;

/// `MNT_OP_FLAGS`.
pub const MNT_OP_FLAGS: i32 = MNT_UPDATE | MNT_RELOAD | MNT_FORCE | MNT_WANTRDWR;

/// `MNT_WAIT`: synchronously wait for I/O to complete.
pub const MNT_WAIT: i32 = 1;
/// `MNT_NOWAIT`: start all I/O, but do not wait for it.
pub const MNT_NOWAIT: i32 = 2;
/// `MNT_LAZY`: push data not written by filesystem syncer.
pub const MNT_LAZY: i32 = 3;

/// `struct fhandle` (`fhandle_t`): generic file handle.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Fhandle {
    /// `fh_fsid`: file system id of mount point.
    pub fh_fsid: Fsid,
    /// `fh_fid`: file sys specific id.
    pub fh_fid: Fid,
}

// SAFETY: `#[repr(C)]` integers only: two `i32`, two `u16` and 16 bytes, no padding.
unsafe impl AbiPod for Fhandle {}

/// `VFS_GENERIC`: generic filesystem information.
pub const VFS_GENERIC: i32 = 0;
/// `VFS_MAXTYPENUM`: int: highest defined filesystem type.
pub const VFS_MAXTYPENUM: i32 = 1;
/// `VFS_CONF`: struct: vfsconf for filesystem given as next argument.
pub const VFS_CONF: i32 = 2;
/// `VFS_BCACHESTAT`: struct: buffer cache statistics given as next argument.
pub const VFS_BCACHESTAT: i32 = 3;

/// `struct vfsconf`: filesystem configuration information. One of these exists for each type
/// of filesystem supported by the kernel. These are searched at mount time to identify the
/// requested filesystem.
pub struct Vfsconf {
    /// `vfc_vfsops`: filesystem operations vector.
    pub vfc_vfsops: &'static Vfsops,
    /// `vfc_name`: filesystem type name.
    pub vfc_name: [u8; MFSNAMELEN],
    /// `vfc_typenum`: historic filesystem type number.
    pub vfc_typenum: i32,
    /// `vfc_refcount`: number mounted of this type.
    pub vfc_refcount: AtomicU32,
    /// `vfc_flags`: permanent flags.
    pub vfc_flags: i32,
    /// `vfc_datasize`: size of data args.
    pub vfc_datasize: usize,
}

impl Vfsconf {
    /// A configuration entry: `{ &ops, name, typenum, 0, flags, datasize }`.
    pub const fn new(
        vfsops: &'static Vfsops,
        name: &[u8],
        typenum: i32,
        flags: i32,
        datasize: usize,
    ) -> Self {
        let mut vfc_name = [0u8; MFSNAMELEN];
        let mut i = 0;
        while i < name.len() && i < MFSNAMELEN - 1 {
            vfc_name[i] = name[i];
            i += 1;
        }
        Self {
            vfc_vfsops: vfsops,
            vfc_name,
            vfc_typenum: typenum,
            vfc_refcount: AtomicU32::new(0),
            vfc_flags: flags,
            vfc_datasize: datasize,
        }
    }

    /// `vfc_name` without the NUL padding.
    pub fn name(&self) -> &[u8] {
        let len = self
            .vfc_name
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(MFSNAMELEN);
        &self.vfc_name[..len]
    }
}

/// `struct bcachestats`: buffer cache statistics.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Bcachestats {
    /// `numbufs`: number of buffers allocated.
    pub numbufs: i64,
    /// `numbufpages`: number of pages in buffer cache.
    pub numbufpages: i64,
    /// `numdirtypages`: number of dirty free pages.
    pub numdirtypages: i64,
    /// `numcleanpages`: number of clean free pages.
    pub numcleanpages: i64,
    /// `pendingwrites`: number of pending writes.
    pub pendingwrites: i64,
    /// `pendingreads`: number of pending reads.
    pub pendingreads: i64,
    /// `numwrites`: total writes started.
    pub numwrites: i64,
    /// `numreads`: total reads started.
    pub numreads: i64,
    /// `cachehits`: total reads found in cache.
    pub cachehits: i64,
    /// `busymapped`: number of busy and mapped buffers.
    pub busymapped: i64,
    /// `delwribufs`: delayed write buffers.
    pub delwribufs: i64,
    /// `kvaslots`: kva slots total.
    pub kvaslots: i64,
    /// `kvaslots_avail`: available kva slots.
    pub kvaslots_avail: i64,
}

/// `BUFPAGES_DEFICIT`: how far the buffer cache is below its low water mark, in pages.
pub fn bufpages_deficit() -> i64 {
    use crate::kern::vfs_bio::{BCSTATS, BUFLOWPAGES};
    let d = BUFLOWPAGES.load(core::sync::atomic::Ordering::Relaxed)
        - BCSTATS
            .numbufpages
            .load(core::sync::atomic::Ordering::Relaxed);
    d.max(0)
}

/// `BUFPAGES_INACT`: the buffer cache's clean pages above its low water mark.
pub fn bufpages_inact() -> i64 {
    use crate::kern::vfs_bio::{BCSTATS, BUFLOWPAGES};
    let d = BCSTATS
        .numcleanpages
        .load(core::sync::atomic::Ordering::Relaxed)
        - BUFLOWPAGES.load(core::sync::atomic::Ordering::Relaxed);
    d.max(0)
}

/// The type of `vfs_mount(mp, path, data, ndp, p)`.
pub type VfsMountFn =
    fn(&'static Mount, &[u8], &mut [u8], &mut Nameidata<'_>, &Proc) -> Result<(), Errno>;

/// The type of `vfs_checkexp(mp, nam, extflagsp, credanonp)`.
pub type VfsCheckexpFn =
    fn(&'static Mount, &Mbuf, &mut i32, &mut *const Ucred) -> Result<(), Errno>;

/// The type of `vfs_init(vfsconf)`.
pub type VfsInitFn = fn(&'static Vfsconf) -> Result<(), Errno>;

/// `struct vfsops`: operations supported on mounted file system.
pub struct Vfsops {
    /// `vfs_mount(mp, path, data, ndp, p)`: `data` is the kernel copy of the user's
    /// arguments.
    pub vfs_mount: VfsMountFn,
    /// `vfs_start(mp, flags, p)`.
    pub vfs_start: fn(&'static Mount, i32, &Proc) -> Result<(), Errno>,
    /// `vfs_unmount(mp, mntflags, p)`.
    pub vfs_unmount: fn(&'static Mount, i32, &Proc) -> Result<(), Errno>,
    /// `vfs_root(mp, vpp)`: the root vnode, locked.
    pub vfs_root: fn(&'static Mount) -> Result<&'static Vnode, Errno>,
    /// `vfs_quotactl(mp, cmds, uid, arg, p)`: `arg` is a user address.
    pub vfs_quotactl: fn(&'static Mount, i32, Uid, usize, &Proc) -> Result<(), Errno>,
    /// `vfs_statfs(mp, sbp, p)`.
    pub vfs_statfs: fn(&'static Mount, &mut Statfs, &Proc) -> Result<(), Errno>,
    /// `vfs_sync(mp, waitfor, stall, cred, p)`.
    pub vfs_sync: fn(&'static Mount, i32, i32, *const Ucred, &Proc) -> Result<(), Errno>,
    /// `vfs_vget(mp, ino, vpp)`.
    pub vfs_vget: fn(&'static Mount, Ino) -> Result<&'static Vnode, Errno>,
    /// `vfs_fhtovp(mp, fhp, vpp)`.
    pub vfs_fhtovp: fn(&'static Mount, &Fid) -> Result<&'static Vnode, Errno>,
    /// `vfs_vptofh(vp, fhp)`.
    pub vfs_vptofh: fn(&'static Vnode, &mut Fid) -> Result<(), Errno>,
    /// `vfs_init(vfsconf)`, NULL when the type needs no initialisation.
    pub vfs_init: Option<VfsInitFn>,
    /// `vfs_sysctl(name, namelen, oldp, oldlenp, newp, newlen, p)`, NULL when the type has no
    /// `vfs.<type>` node.
    pub vfs_sysctl: Option<Sysctlfn>,
    /// `vfs_checkexp(mp, nam, extflagsp, credanonp)`.
    pub vfs_checkexp: VfsCheckexpFn,
}

/// `VFS_MOUNT(MP, PATH, DATA, NDP, P)`.
#[allow(non_snake_case)] // the C macro's name
pub fn VFS_MOUNT(
    mp: &'static Mount,
    path: &[u8],
    data: &mut [u8],
    ndp: &mut Nameidata<'_>,
    p: &Proc,
) -> Result<(), Errno> {
    (mp.op().vfs_mount)(mp, path, data, ndp, p)
}

/// `VFS_START(MP, FLAGS, P)`.
#[allow(non_snake_case)] // the C macro's name
pub fn VFS_START(mp: &'static Mount, flags: i32, p: &Proc) -> Result<(), Errno> {
    (mp.op().vfs_start)(mp, flags, p)
}

/// `VFS_UNMOUNT(MP, FORCE, P)`.
#[allow(non_snake_case)] // the C macro's name
pub fn VFS_UNMOUNT(mp: &'static Mount, force: i32, p: &Proc) -> Result<(), Errno> {
    (mp.op().vfs_unmount)(mp, force, p)
}

/// `VFS_ROOT(MP, VPP)`.
#[allow(non_snake_case)] // the C macro's name
pub fn VFS_ROOT(mp: &'static Mount) -> Result<&'static Vnode, Errno> {
    (mp.op().vfs_root)(mp)
}

/// `VFS_QUOTACTL(MP, C, U, A, P)`.
#[allow(non_snake_case)] // the C macro's name
pub fn VFS_QUOTACTL(mp: &'static Mount, c: i32, u: Uid, a: usize, p: &Proc) -> Result<(), Errno> {
    (mp.op().vfs_quotactl)(mp, c, u, a, p)
}

/// `VFS_STATFS(MP, SBP, P)`.
#[allow(non_snake_case)] // the C macro's name
pub fn VFS_STATFS(mp: &'static Mount, sbp: &mut Statfs, p: &Proc) -> Result<(), Errno> {
    (mp.op().vfs_statfs)(mp, sbp, p)
}

/// `VFS_SYNC(MP, W, S, C, P)`.
#[allow(non_snake_case)] // the C macro's name
pub fn VFS_SYNC(
    mp: &'static Mount,
    w: i32,
    s: i32,
    c: *const Ucred,
    p: &Proc,
) -> Result<(), Errno> {
    (mp.op().vfs_sync)(mp, w, s, c, p)
}

/// `VFS_VGET(MP, INO, VPP)`.
#[allow(non_snake_case)] // the C macro's name
pub fn VFS_VGET(mp: &'static Mount, ino: Ino) -> Result<&'static Vnode, Errno> {
    (mp.op().vfs_vget)(mp, ino)
}

/// `VFS_FHTOVP(MP, FIDP, VPP)`.
#[allow(non_snake_case)] // the C macro's name
pub fn VFS_FHTOVP(mp: &'static Mount, fidp: &Fid) -> Result<&'static Vnode, Errno> {
    (mp.op().vfs_fhtovp)(mp, fidp)
}

/// `VFS_VPTOFH(VP, FIDP)`.
#[allow(non_snake_case)] // the C macro's name
pub fn VFS_VPTOFH(vp: &'static Vnode, fidp: &mut Fid) -> Result<(), Errno> {
    let Some(mp) = vp.v_mount.get() else {
        panic(format_args!("VFS_VPTOFH: vnode {:p} has no mount", vp));
    };
    (mp.op().vfs_vptofh)(vp, fidp)
}

/// `VFS_CHECKEXP(MP, NAM, EXFLG, CRED)`.
#[allow(non_snake_case)] // the C macro's name
pub fn VFS_CHECKEXP(
    mp: &'static Mount,
    nam: &Mbuf,
    exflg: &mut i32,
    cred: &mut *const Ucred,
) -> Result<(), Errno> {
    (mp.op().vfs_checkexp)(mp, nam, exflg, cred)
}

/// `VB_READ`.
pub const VB_READ: i32 = 0x01;
/// `VB_WRITE`.
pub const VB_WRITE: i32 = 0x02;
/// `VB_NOWAIT`: immediately fail on busy lock.
pub const VB_NOWAIT: i32 = 0x04;
/// `VB_WAIT`: sleep fail on busy lock.
pub const VB_WAIT: i32 = 0x08;
/// `VB_DUPOK`: permit duplicate mount busying.
pub const VB_DUPOK: i32 = 0x10;

// The amd64/arm64 layout of `struct statfs` (both LP64).
const _: () = {
    use core::mem::offset_of;
    assert!(offset_of!(Statfs, f_blocks) == 16);
    assert!(offset_of!(Statfs, f_fsid) == 96);
    assert!(offset_of!(Statfs, f_ctime) == 112);
    assert!(offset_of!(Statfs, f_fstypename) == 120);
    assert!(offset_of!(Statfs, f_mntfromspec) == 316);
    assert!(offset_of!(Statfs, mount_info) == 408);
    assert!(size_of::<Statfs>() == 568);
    assert!(size_of::<Fhandle>() == 28);
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/sys/mount.h");
        for (name, value) in [
            ("MNT_RDONLY", MNT_RDONLY),
            ("MNT_NOPERM", MNT_NOPERM),
            ("MNT_WXALLOWED", MNT_WXALLOWED),
            ("MNT_ROOTFS", MNT_ROOTFS),
            ("MNT_VISFLAGMASK", MNT_VISFLAGMASK),
            ("MNT_UPDATE", MNT_UPDATE),
            ("MNT_STALLED", MNT_STALLED),
            ("MNT_UNMOUNT", MNT_UNMOUNT),
            ("MNT_DOOMED", MNT_DOOMED),
            ("MNT_LAZY", MNT_LAZY),
            ("MFSNAMELEN", MFSNAMELEN as i32),
            ("MNAMELEN", MNAMELEN as i32),
            ("MAXFIDSZ", MAXFIDSZ as i32),
            ("VFS_BCACHESTAT", VFS_BCACHESTAT),
            ("VB_DUPOK", VB_DUPOK),
        ] {
            assert_eq!(
                crate::reftest::int(&defs, name),
                Some(i64::from(value)),
                "{name}"
            );
        }
    }
}
