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

/* <CODE> */
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
//!   `union mount_info` is its 160 bytes, 8-aligned: the per-filesystem views come with
//!   their file systems (`ufs_args` and the `export_args` it embeds came with ffs, `struct
//!   mfs_args` (`MfsArgs`) with MFS, `iso_args`, `msdosfs_args`, `udf_args` and `tmpfs_args`
//!   with M10c, `nfs_args` with NFS (M10e); each `*Args::from_bytes` reads them out of the kernel copy of the mount
//!   arguments). `ntfs_args` (`NtfsArgs`) and the two `NTFS_MFLAG_*` came with NTFS (M10d),
//!   under cfg `option_ntfs` (feature `ntfs`, amd64 only: `sys/build.rs`).
//! - `struct vfsconf`'s `vfc_refcount` is atomic (`atomic_inc_int` in C).
//! - `VFS_*` are functions with the macros' names (`#[allow(non_snake_case)]`).
//! - `struct netcred` and `struct netexport` are always defined, as the C header does, over
//!   the ported radix tree (`net/radix.rs`): their members are `Cell`s, the `ne_rtable_inet`
//!   pointer an `Option<&'static RadixNodeHead>`; a `Netcred` is `malloc`ed with the
//!   address and mask it is keyed by behind it, as in C (`kern/vfs_subr.rs`). `struct
//!   nfs_args` (`NfsArgs`) reads like the other `*Args`; its `addr`, `fh` and `hostname`
//!   are user addresses.
//! - The user-level prototypes (`mount`, `statfs`, ...) are not kernel material; the kernel
//!   ones are their functions in `vfs_subr.rs`, `vfs_init.rs` and `vfs_syscalls.rs`.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::AtomicU32;

use crate::kern::subr_prf::panic;
use crate::machine::copy::AbiPod;
use crate::net::radix::{RadixNode, RadixNodeHead};
use crate::queue_adapter;
use crate::sys::errno::Errno;
use crate::sys::mbuf::Mbuf;
use crate::sys::namei::Nameidata;
use crate::sys::proc::Proc;
use crate::sys::queue::{SlistEntry, TailqEntry, TailqHead};
use crate::sys::refcnt::Refcnt;
use crate::sys::rwlock::Rwlock;
use crate::sys::sysctl::Sysctlfn;
use crate::sys::types::{Gid, Ino, Mode, Off, Uid};
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

/// `struct export_args`: export arguments for local filesystem mount calls. The two
/// `struct sockaddr *` are user addresses (`usize`), which the kernel copies in when it
/// exports (`NFSSERVER`).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ExportArgs {
    /// `ex_flags`: export related flags.
    pub ex_flags: i32,
    /// `ex_root`: mapping for root uid.
    pub ex_root: Uid,
    /// `ex_anon`: mapping for anonymous user.
    pub ex_anon: crate::sys::ucred::Xucred,
    /// `ex_addr`: net address to which exported.
    pub ex_addr: usize,
    /// `ex_addrlen`: and the net address length.
    pub ex_addrlen: i32,
    /// `ex_mask`: mask of valid bits in saddr.
    pub ex_mask: usize,
    /// `ex_masklen`: and the smask length.
    pub ex_masklen: i32,
}

/// `struct ufs_args`: arguments to mount UFS-based filesystems. `fspec` is the user address
/// of the block special device's name.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct UfsArgs {
    /// `fspec`: block special device to mount.
    pub fspec: usize,
    /// `export_info`: network export information.
    pub export_info: ExportArgs,
}

impl UfsArgs {
    /// `sizeof(struct ufs_args)`: the `vfc_datasize` of the UFS file systems.
    pub const SIZE: usize = size_of::<UfsArgs>();

    /// The arguments in the kernel copy `sys_mount` made of them (at least `SIZE` bytes),
    /// `None` when there are none (the C's NULL `data`).
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < Self::SIZE {
            return None;
        }
        // SAFETY: `data` holds `SIZE` readable bytes (checked), and the structure is
        // integers, valid for any bit pattern (its padding bytes are padding); the read is
        // unaligned.
        Some(unsafe { ptr::read_unaligned(data.as_ptr().cast::<UfsArgs>()) })
    }
}

/// `struct mfs_args`: arguments to mount MFS. `fspec` is the user address of the name to
/// export for `statfs`, `base` the user address of the file system in the memory of the
/// process that mounts it.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct MfsArgs {
    /// `fspec`: name to export for statfs.
    pub fspec: usize,
    /// `export_info`: if exported MFSes are supported.
    pub export_info: ExportArgs,
    /// `base`: base of file system in memory.
    pub base: usize,
    /// `size`: size of file system.
    pub size: u64,
}

impl MfsArgs {
    /// `sizeof(struct mfs_args)`: the `vfc_datasize` of MFS.
    pub const SIZE: usize = size_of::<MfsArgs>();

    /// The arguments in the kernel copy `sys_mount` made of them (at least `SIZE` bytes),
    /// `None` when there are none (the C's NULL `data`).
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < Self::SIZE {
            return None;
        }
        // SAFETY: `data` holds `SIZE` readable bytes (checked), and the structure is
        // integers, valid for any bit pattern (its padding bytes are padding); the read is
        // unaligned.
        Some(unsafe { ptr::read_unaligned(data.as_ptr().cast::<MfsArgs>()) })
    }
}

/// The `SIZE` and `from_bytes` of a file system's mount arguments, as [`UfsArgs`] has them:
/// `sys_mount` copies `vfc_datasize` bytes in, and the file system reads its structure out.
macro_rules! mount_args_from_bytes {
    ($t:ident, $c:literal) => {
        impl $t {
            #[doc = concat!("`sizeof(struct ", $c, ")`: the `vfc_datasize` of its file system.")]
            pub const SIZE: usize = size_of::<$t>();

            /// The arguments in the kernel copy `sys_mount` made of them (at least `SIZE`
            /// bytes), `None` when there are none (the C's NULL `data`).
            pub fn from_bytes(data: &[u8]) -> Option<Self> {
                if data.len() < Self::SIZE {
                    return None;
                }
                // SAFETY: `data` holds `SIZE` readable bytes (checked), and the structure is
                // integers, valid for any bit pattern (its padding bytes are padding); the
                // read is unaligned.
                Some(unsafe { ptr::read_unaligned(data.as_ptr().cast::<$t>()) })
            }
        }
    };
}

/// `struct iso_args`: arguments to mount ISO 9660 filesystems. `fspec` is a user address.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct IsoArgs {
    /// `fspec`: block special device to mount.
    pub fspec: usize,
    /// `export_info`: network export info.
    pub export_info: ExportArgs,
    /// `flags`: mounting flags, the `ISOFSMNT_*` below.
    pub flags: i32,
    /// `sess`: start sector of session.
    pub sess: i32,
}

mount_args_from_bytes!(IsoArgs, "iso_args");

/// `ISOFSMNT_NORRIP`: disable Rock Ridge Ext.
pub const ISOFSMNT_NORRIP: i32 = 0x0000_0001;
/// `ISOFSMNT_GENS`: enable generation numbers.
pub const ISOFSMNT_GENS: i32 = 0x0000_0002;
/// `ISOFSMNT_EXTATT`: enable extended attr.
pub const ISOFSMNT_EXTATT: i32 = 0x0000_0004;
/// `ISOFSMNT_NOJOLIET`: disable Joliet Ext.
pub const ISOFSMNT_NOJOLIET: i32 = 0x0000_0008;
/// `ISOFSMNT_SESS`: use `iso_args.sess`.
pub const ISOFSMNT_SESS: i32 = 0x0000_0010;

/// `NFS_ARGSVERSION`: change when `nfs_args` changes.
pub const NFS_ARGSVERSION: i32 = 4;

/// `struct nfs_args`: arguments to mount NFS. `addr`, `fh` and `hostname` are user addresses
/// (the C's `struct sockaddr *`, `u_char *` and `char *`), which the NFS mount copies in.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct NfsArgs {
    /// `version`: args structure version number (`NFS_ARGSVERSION`).
    pub version: i32,
    /// `addr`: file server address.
    pub addr: usize,
    /// `addrlen`: length of address.
    pub addrlen: i32,
    /// `sotype`: socket type.
    pub sotype: i32,
    /// `proto`: and protocol.
    pub proto: i32,
    /// `fh`: file handle to be mounted.
    pub fh: usize,
    /// `fhsize`: size, in bytes, of `fh`.
    pub fhsize: i32,
    /// `flags`: the `NFSMNT_*` below.
    pub flags: i32,
    /// `wsize`: write size in bytes.
    pub wsize: i32,
    /// `rsize`: read size in bytes.
    pub rsize: i32,
    /// `readdirsize`: readdir size in bytes.
    pub readdirsize: i32,
    /// `timeo`: initial timeout in .1 secs.
    pub timeo: i32,
    /// `retrans`: times to retry send.
    pub retrans: i32,
    /// `maxgrouplist`: max. size of group list.
    pub maxgrouplist: i32,
    /// `readahead`: number of blocks to readahead.
    pub readahead: i32,
    /// `leaseterm`: term (sec) of lease.
    pub leaseterm: i32,
    /// `deadthresh`: retrans threshold.
    pub deadthresh: i32,
    /// `hostname`: server's name.
    pub hostname: usize,
    /// `acregmin`: attr cache file recently modified.
    pub acregmin: i32,
    /// `acregmax`: ac file not recently modified.
    pub acregmax: i32,
    /// `acdirmin`: ac for dir recently modified.
    pub acdirmin: i32,
    /// `acdirmax`: ac for dir not recently modified.
    pub acdirmax: i32,
}

mount_args_from_bytes!(NfsArgs, "nfs_args");

/// `NFSMNT_RESVPORT`: always use reserved ports.
pub const NFSMNT_RESVPORT: i32 = 0x0000_0000;
/// `NFSMNT_SOFT`: soft mount (hard is default).
pub const NFSMNT_SOFT: i32 = 0x0000_0001;
/// `NFSMNT_WSIZE`: set write size.
pub const NFSMNT_WSIZE: i32 = 0x0000_0002;
/// `NFSMNT_RSIZE`: set read size.
pub const NFSMNT_RSIZE: i32 = 0x0000_0004;
/// `NFSMNT_TIMEO`: set initial timeout.
pub const NFSMNT_TIMEO: i32 = 0x0000_0008;
/// `NFSMNT_RETRANS`: set number of request retries.
pub const NFSMNT_RETRANS: i32 = 0x0000_0010;
/// `NFSMNT_MAXGRPS`: set maximum grouplist size.
pub const NFSMNT_MAXGRPS: i32 = 0x0000_0020;
/// `NFSMNT_INT`: allow interrupts on hard mount.
pub const NFSMNT_INT: i32 = 0x0000_0040;
/// `NFSMNT_NOCONN`: don't connect the socket.
pub const NFSMNT_NOCONN: i32 = 0x0000_0080;
/// `NFSMNT_NQNFS`: use Nqnfs protocol.
pub const NFSMNT_NQNFS: i32 = 0x0000_0100;
/// `NFSMNT_NFSV3`: use NFS version 3 protocol.
pub const NFSMNT_NFSV3: i32 = 0x0000_0200;
/// `NFSMNT_KERB`: use Kerberos authentication.
pub const NFSMNT_KERB: i32 = 0x0000_0400;
/// `NFSMNT_DUMBTIMR`: don't estimate rtt dynamically.
pub const NFSMNT_DUMBTIMR: i32 = 0x0000_0800;
/// `NFSMNT_LEASETERM`: set lease term (nqnfs).
pub const NFSMNT_LEASETERM: i32 = 0x0000_1000;
/// `NFSMNT_READAHEAD`: set read ahead.
pub const NFSMNT_READAHEAD: i32 = 0x0000_2000;
/// `NFSMNT_DEADTHRESH`: set dead server retry thresh.
pub const NFSMNT_DEADTHRESH: i32 = 0x0000_4000;
/// `NFSMNT_NOAC`: disable attribute cache.
pub const NFSMNT_NOAC: i32 = 0x0000_8000;
/// `NFSMNT_RDIRPLUS`: use Readdirplus for V3.
pub const NFSMNT_RDIRPLUS: i32 = 0x0001_0000;
/// `NFSMNT_READDIRSIZE`: set readdir size.
pub const NFSMNT_READDIRSIZE: i32 = 0x0002_0000;
/// `NFSMNT_ACREGMIN`: `acregmin` field valid.
pub const NFSMNT_ACREGMIN: i32 = 0x0004_0000;
/// `NFSMNT_ACREGMAX`: `acregmax` field valid.
pub const NFSMNT_ACREGMAX: i32 = 0x0008_0000;
/// `NFSMNT_ACDIRMIN`: `acdirmin` field valid.
pub const NFSMNT_ACDIRMIN: i32 = 0x0010_0000;
/// `NFSMNT_ACDIRMAX`: `acdirmax` field valid.
pub const NFSMNT_ACDIRMAX: i32 = 0x0020_0000;

// The bits the kernel sets in `nm_flag` for itself; `mount_nfs(8)` never passes them.

/// `NFSMNT_INTERNAL`: bits set internally.
pub const NFSMNT_INTERNAL: i32 = 0xfffc_0000_u32 as i32;
/// `NFSMNT_HASWRITEVERF`: has write verifier for V3.
pub const NFSMNT_HASWRITEVERF: i32 = 0x0004_0000;
/// `NFSMNT_GOTPATHCONF`: got the V3 pathconf info.
pub const NFSMNT_GOTPATHCONF: i32 = 0x0008_0000;
/// `NFSMNT_GOTFSINFO`: got the V3 fsinfo.
pub const NFSMNT_GOTFSINFO: i32 = 0x0010_0000;
/// `NFSMNT_MNTD`: mnt server for mnt point.
pub const NFSMNT_MNTD: i32 = 0x0020_0000;
/// `NFSMNT_DISMINPROG`: dismount in progress.
pub const NFSMNT_DISMINPROG: i32 = 0x0040_0000;
/// `NFSMNT_DISMNT`: dismounted.
pub const NFSMNT_DISMNT: i32 = 0x0080_0000;
/// `NFSMNT_SNDLOCK`: send socket lock.
pub const NFSMNT_SNDLOCK: i32 = 0x0100_0000;
/// `NFSMNT_WANTSND`: want above.
pub const NFSMNT_WANTSND: i32 = 0x0200_0000;
/// `NFSMNT_RCVLOCK`: rcv socket lock.
pub const NFSMNT_RCVLOCK: i32 = 0x0400_0000;
/// `NFSMNT_WANTRCV`: want above.
pub const NFSMNT_WANTRCV: i32 = 0x0800_0000;
/// `NFSMNT_WAITAUTH`: wait for authentication.
pub const NFSMNT_WAITAUTH: i32 = 0x1000_0000;
/// `NFSMNT_HASAUTH`: has authenticator.
pub const NFSMNT_HASAUTH: i32 = 0x2000_0000;
/// `NFSMNT_WANTAUTH`: wants an authenticator.
pub const NFSMNT_WANTAUTH: i32 = 0x4000_0000;
/// `NFSMNT_AUTHERR`: authentication error.
pub const NFSMNT_AUTHERR: i32 = 0x8000_0000_u32 as i32;

/// `struct msdosfs_args`: arguments to mount MSDOS filesystems. `fspec` is a user address.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct MsdosfsArgs {
    /// `fspec`: blocks special holding the fs to mount.
    pub fspec: usize,
    /// `export_info`: network export information.
    pub export_info: ExportArgs,
    /// `uid`: uid that owns msdosfs files.
    pub uid: Uid,
    /// `gid`: gid that owns msdosfs files.
    pub gid: Gid,
    /// `mask`: mask to be applied for msdosfs perms.
    pub mask: Mode,
    /// `flags`: the `MSDOSFSMNT_*` below.
    pub flags: i32,
}

mount_args_from_bytes!(MsdosfsArgs, "msdosfs_args");

/// `MSDOSFSMNT_SHORTNAME`: force old DOS short names only.
pub const MSDOSFSMNT_SHORTNAME: i32 = 0x01;
/// `MSDOSFSMNT_LONGNAME`: force Win'95 long names.
pub const MSDOSFSMNT_LONGNAME: i32 = 0x02;
/// `MSDOSFSMNT_NOWIN95`: completely ignore Win95 entries.
pub const MSDOSFSMNT_NOWIN95: i32 = 0x04;

/// `struct ntfs_args`: arguments to mount ntfs filesystems. `fspec` is a user address.
#[cfg(option_ntfs)]
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct NtfsArgs {
    /// `fspec`: block special device to mount.
    pub fspec: usize,
    /// `export_info`: network export information.
    pub export_info: ExportArgs,
    /// `uid`: uid that owns ntfs files.
    pub uid: Uid,
    /// `gid`: gid that owns ntfs files.
    pub gid: Gid,
    /// `mode`: mask to be applied for ntfs perms.
    pub mode: Mode,
    /// `flag`: additional flags, the `NTFS_MFLAG_*` below.
    pub flag: u64,
}

#[cfg(option_ntfs)]
mount_args_from_bytes!(NtfsArgs, "ntfs_args");

/// `NTFS_MFLAG_CASEINS`: ntfs mount option, case-insensitive lookups.
#[cfg(option_ntfs)]
pub const NTFS_MFLAG_CASEINS: u64 = 0x0000_0001;
/// `NTFS_MFLAG_ALLNAMES`: ntfs mount option, list the DOS names too.
#[cfg(option_ntfs)]
pub const NTFS_MFLAG_ALLNAMES: u64 = 0x0000_0002;

/// `struct udf_args`: arguments to mount UDF file systems. `fspec` is a user address.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct UdfArgs {
    /// `fspec`: block special device to mount.
    pub fspec: usize,
    /// `lastblock`: special device last block.
    pub lastblock: u32,
}

mount_args_from_bytes!(UdfArgs, "udf_args");

/// `TMPFS_ARGS_VERSION`.
pub const TMPFS_ARGS_VERSION: i32 = 1;

/// `struct tmpfs_args`: arguments to mount tmpfs file systems.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct TmpfsArgs {
    /// `ta_version`: `TMPFS_ARGS_VERSION`.
    pub ta_version: i32,
    /// `ta_nodes_max`: size counter, the most nodes.
    pub ta_nodes_max: Ino,
    /// `ta_size_max`: size counter, the most bytes.
    pub ta_size_max: Off,
    /// `ta_root_uid`: root node attribute.
    pub ta_root_uid: Uid,
    /// `ta_root_gid`: root node attribute.
    pub ta_root_gid: Gid,
    /// `ta_root_mode`: root node attribute.
    pub ta_root_mode: Mode,
}

mount_args_from_bytes!(TmpfsArgs, "tmpfs_args");

/// `struct fusefs_args`: arguments to mount fusefs filesystems. `name` is a user address.
#[cfg(feature = "fuse")]
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct FusefsArgs {
    /// `name`.
    pub name: usize,
    /// `fd`: the daemon's open fuse(4) device.
    pub fd: i32,
    /// `max_read`.
    pub max_read: i32,
    /// `allow_other`: FUSE does not allow the file system to be accessed by other users
    /// unless this option is specified. This is to prevent unintentional denial of service
    /// to other users if the file system is not responding. e.g. user executes df(1) or
    /// cron job that scans mounted file systems.
    pub allow_other: i32,
}

#[cfg(feature = "fuse")]
mount_args_from_bytes!(FusefsArgs, "fusefs_args");

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

/// `struct netcred`: network address lookup element. The radix tree of a [`Netexport`] holds
/// these `malloc(M_NETADDR)`ed with the address and mask they are keyed by right behind
/// them (`netc_len` is the size of the whole allocation); the first member must stay
/// `netc_rnodes`, as `vfs_export_lookup` turns the leaf `rn_match` finds back into its
/// `Netcred`.
#[repr(C)]
pub struct Netcred {
    /// `netc_rnodes`: the radix tree's leaf and internal node for this entry.
    pub netc_rnodes: [RadixNode; 2],
    /// `netc_exflags`: the `export_args`'s `ex_flags` (`MNT_EXRDONLY`, ...).
    pub netc_exflags: Cell<i32>,
    /// `netc_len`: size of the allocation.
    pub netc_len: Cell<i32>,
    /// `netc_anon`: the credentials anonymous clients map to.
    pub netc_anon: Ucred,
}

// SAFETY: the members are changed under the kernel lock while the file system's export
// list is updated (`vfs_export`), as in C; the kernel serialises them like the rest of the
// mount's state.
unsafe impl Sync for Netcred {}

impl Netcred {
    /// A zeroed entry, as `malloc(M_ZERO)` returns it.
    pub const fn new() -> Self {
        Self {
            netc_rnodes: [RadixNode::new(), RadixNode::new()],
            netc_exflags: Cell::new(0),
            netc_len: Cell::new(0),
            netc_anon: Ucred::new(),
        }
    }
}

impl Default for Netcred {
    fn default() -> Self {
        Self::new()
    }
}

/// `struct netexport`: network export information of one file system.
pub struct Netexport {
    /// `ne_defexported`: default export.
    pub ne_defexported: Netcred,
    /// `ne_rtable_inet`: individual exports.
    pub ne_rtable_inet: Cell<Option<&'static RadixNodeHead>>,
}

// SAFETY: as for `Netcred`.
unsafe impl Sync for Netexport {}

impl Netexport {
    /// An empty export list, as `malloc(M_ZERO)` returns it.
    pub const fn new() -> Self {
        Self {
            ne_defexported: Netcred::new(),
            ne_rtable_inet: Cell::new(None),
        }
    }
}

impl Default for Netexport {
    fn default() -> Self {
        Self::new()
    }
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
    assert!(size_of::<ExportArgs>() == 120);
    assert!(UfsArgs::SIZE == 128);
    assert!(MfsArgs::SIZE == 144);
    assert!(offset_of!(MfsArgs, base) == 128);
    assert!(offset_of!(MfsArgs, size) == 136);
    assert!(MfsArgs::SIZE <= size_of::<MountInfo>());
    assert!(IsoArgs::SIZE == 136);
    assert!(NfsArgs::SIZE == 112);
    assert!(offset_of!(NfsArgs, fh) == 32);
    assert!(offset_of!(NfsArgs, hostname) == 88);
    assert!(NfsArgs::SIZE <= size_of::<MountInfo>());
    assert!(MsdosfsArgs::SIZE == 144);
    assert!(UdfArgs::SIZE == 16);
    assert!(TmpfsArgs::SIZE == 40);
};

#[cfg(option_ntfs)]
const _: () = {
    assert!(NtfsArgs::SIZE == 152);
    assert!(core::mem::offset_of!(NtfsArgs, uid) == 128);
    assert!(core::mem::offset_of!(NtfsArgs, flag) == 144);
    assert!(NtfsArgs::SIZE <= size_of::<MountInfo>());
};

#[cfg(feature = "fuse")]
const _: () = {
    assert!(FusefsArgs::SIZE == 24);
    assert!(core::mem::offset_of!(FusefsArgs, fd) == 8);
    assert!(core::mem::offset_of!(FusefsArgs, allow_other) == 16);
};
/* </CODE> */

/* <TESTS> */
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
            ("ISOFSMNT_NORRIP", ISOFSMNT_NORRIP),
            ("ISOFSMNT_GENS", ISOFSMNT_GENS),
            ("ISOFSMNT_EXTATT", ISOFSMNT_EXTATT),
            ("ISOFSMNT_NOJOLIET", ISOFSMNT_NOJOLIET),
            ("ISOFSMNT_SESS", ISOFSMNT_SESS),
            ("MSDOSFSMNT_SHORTNAME", MSDOSFSMNT_SHORTNAME),
            ("MSDOSFSMNT_LONGNAME", MSDOSFSMNT_LONGNAME),
            ("MSDOSFSMNT_NOWIN95", MSDOSFSMNT_NOWIN95),
            ("TMPFS_ARGS_VERSION", TMPFS_ARGS_VERSION),
        ] {
            assert_eq!(
                crate::reftest::int(&defs, name),
                Some(i64::from(value)),
                "{name}"
            );
        }
        // The NFSMNT_* bits fill all 32 bits; the C's `int` constants are the unsigned values.
        for (name, value) in [
            ("NFS_ARGSVERSION", NFS_ARGSVERSION),
            ("NFSMNT_RESVPORT", NFSMNT_RESVPORT),
            ("NFSMNT_SOFT", NFSMNT_SOFT),
            ("NFSMNT_WSIZE", NFSMNT_WSIZE),
            ("NFSMNT_RSIZE", NFSMNT_RSIZE),
            ("NFSMNT_TIMEO", NFSMNT_TIMEO),
            ("NFSMNT_RETRANS", NFSMNT_RETRANS),
            ("NFSMNT_MAXGRPS", NFSMNT_MAXGRPS),
            ("NFSMNT_INT", NFSMNT_INT),
            ("NFSMNT_NOCONN", NFSMNT_NOCONN),
            ("NFSMNT_NQNFS", NFSMNT_NQNFS),
            ("NFSMNT_NFSV3", NFSMNT_NFSV3),
            ("NFSMNT_KERB", NFSMNT_KERB),
            ("NFSMNT_DUMBTIMR", NFSMNT_DUMBTIMR),
            ("NFSMNT_LEASETERM", NFSMNT_LEASETERM),
            ("NFSMNT_READAHEAD", NFSMNT_READAHEAD),
            ("NFSMNT_DEADTHRESH", NFSMNT_DEADTHRESH),
            ("NFSMNT_NOAC", NFSMNT_NOAC),
            ("NFSMNT_RDIRPLUS", NFSMNT_RDIRPLUS),
            ("NFSMNT_READDIRSIZE", NFSMNT_READDIRSIZE),
            ("NFSMNT_ACREGMIN", NFSMNT_ACREGMIN),
            ("NFSMNT_ACREGMAX", NFSMNT_ACREGMAX),
            ("NFSMNT_ACDIRMIN", NFSMNT_ACDIRMIN),
            ("NFSMNT_ACDIRMAX", NFSMNT_ACDIRMAX),
            ("NFSMNT_INTERNAL", NFSMNT_INTERNAL),
            ("NFSMNT_HASWRITEVERF", NFSMNT_HASWRITEVERF),
            ("NFSMNT_GOTPATHCONF", NFSMNT_GOTPATHCONF),
            ("NFSMNT_GOTFSINFO", NFSMNT_GOTFSINFO),
            ("NFSMNT_MNTD", NFSMNT_MNTD),
            ("NFSMNT_DISMINPROG", NFSMNT_DISMINPROG),
            ("NFSMNT_DISMNT", NFSMNT_DISMNT),
            ("NFSMNT_SNDLOCK", NFSMNT_SNDLOCK),
            ("NFSMNT_WANTSND", NFSMNT_WANTSND),
            ("NFSMNT_RCVLOCK", NFSMNT_RCVLOCK),
            ("NFSMNT_WANTRCV", NFSMNT_WANTRCV),
            ("NFSMNT_WAITAUTH", NFSMNT_WAITAUTH),
            ("NFSMNT_HASAUTH", NFSMNT_HASAUTH),
            ("NFSMNT_WANTAUTH", NFSMNT_WANTAUTH),
            ("NFSMNT_AUTHERR", NFSMNT_AUTHERR),
        ] {
            assert_eq!(
                crate::reftest::int(&defs, name),
                Some(i64::from(value as u32)),
                "{name}"
            );
        }
    }

    #[test]
    fn nfs_args_read_out_of_the_kernel_copy() {
        let mut bytes = [0u8; NfsArgs::SIZE];
        bytes[..4].copy_from_slice(&NFS_ARGSVERSION.to_ne_bytes());
        bytes[44..48].copy_from_slice(&(NFSMNT_NFSV3 | NFSMNT_RSIZE).to_ne_bytes());
        bytes[96..100].copy_from_slice(&7i32.to_ne_bytes());
        let args = NfsArgs::from_bytes(&bytes).expect("arguments");
        assert_eq!(args.version, NFS_ARGSVERSION);
        assert_eq!(args.flags, NFSMNT_NFSV3 | NFSMNT_RSIZE);
        assert_eq!(args.acregmin, 7);
        assert!(NfsArgs::from_bytes(&bytes[..NfsArgs::SIZE - 1]).is_none());
    }
}
/* </TESTS> */
