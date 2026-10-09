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
 * Copyright (c) 1989, 1993
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
 *	@(#)vnode.h	8.11 (Berkeley) 11/21/94
 */
/* </LICENSES> */

/* <CODE> */
//! `<sys/vnode.h>`: the vnode, the focus of all file activity in UNIX. There is a unique vnode
//! allocated for each active file, each current directory, each mounted-on file, text file,
//! and the root. Also the vnode types and tags, `struct vattr`, the `IO_*` and `V*` flags,
//! `struct vops` (the operations vector a file system fills in) and the `struct vop_*_args`
//! structures its operations receive.
//!
//! Upstream: sys/sys/vnode.h @ 3ce1f3f79392
//!
//! A vnode is a `vnode_pool` item that is never freed: `getnewvnode` either allocates a new
//! one or recycles one from the free lists (`vfs_subr.rs`). It is therefore handed around as
//! `&'static Vnode`, with the C's `v_usecount` (`vref`/`vrele`/`vput`) saying who uses it. The
//! members are `Cell`s: the C mutates them through shared pointers under the kernel lock,
//! `vnode_mtx` (`[V]`) or `splbio` (`[B]`).
//!
//! ## How a file system plugs in
//! - It fills a `static` [`Vops`] whose slots are `Option<fn(&mut VopXxxArgs) -> Result<(),
//!   Errno>>` (`vop_islocked` returns the lock status as an `i32`); `None` is the C's NULL,
//!   which the `VOP_*` wrappers (`vfs_vops.rs`) answer with `EOPNOTSUPP`. Shared helpers
//!   that the C installs in many slots (`nullop`, `vop_generic_badop`, `eopnotsupp`) are
//!   written as closures, `Some(|_| nullop())`, so one helper serves every argument type.
//! - `getnewvnode(tag, mp, &FOO_VOPS)` returns the vnode; the file system hangs its own node
//!   from `v_data` (a `*mut c_void`, as in C) and reads it back with a cast at the one place
//!   it knows the type. The operations receive `&'static Vnode`s.
//! - The out-parameters of the C argument structures stay out-parameters: `a_vpp` is an
//!   `&mut Option<&'static Vnode>` the operation fills in, so a file system's body ports
//!   unchanged.
//!
//! ## Deviations
//! - `v_un` (the union of `v_mountedhere`, `v_socket`, `v_specinfo`, `v_fifoinfo`) is the
//!   enum [`VnodeUn`], with accessors named after the C's shorthand macros; a reader of the
//!   wrong member gets `None`, where the C would reinterpret the pointer.
//! - `si_specnext`, the link of the special-device alias chains, is the vnode's own
//!   `v_specnext` instead of a member of `struct specinfo`: the queue adapters need the link
//!   inside the element. `vp->v_specnext` reads the same in both.
//! - `v_uvm` is `Option<&'static UvmVnode>`: a `uvm_vnode_pool` item is never freed
//!   either.
//! - `struct buf *` (`a_bp`) is a `&'static Buf` (`sys/buf.rs`).
//! - `RBT_HEAD(buf_rb_bufs, buf)` is the [`BufRbBufs`] adapter, ordered by `vfs_subr.c`'s
//!   `rb_buf_compare`; `LIST_HEAD(buflists, buf)` is [`Buflists`] through `b_vnbufs`.
//! - `a_cred` is the C's `struct ucred *`, a raw pointer, since `NOCRED` and `FSCRED` are
//!   sentinel values; [`cred_ref`] turns a real one into a reference.
//! - `IFTOVT`, `VTTOIF` and `MAKEIMODE` are functions over the tables `vfs_subr.rs` defines.
//! - The macro `VN_KNOTE` is the function [`VN_KNOTE`]; the prototypes are their functions
//!   in `vfs_subr.rs`, `vfs_vnops.rs`, `vfs_default.rs`, `vfs_getcwd.rs` and
//!   `vfs_syscalls.rs`; `vfs_sync.c`'s and the uvm ones are not ported (stage 2).

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr;

use crate::kern::subr_prf::panic;
use crate::queue_adapter;
use crate::sys::buf::Buf;
use crate::sys::errno::Errno;
use crate::sys::event::{Klist, Knote};
use crate::sys::fcntl::Flock;
use crate::sys::mount::Mount;
use crate::sys::namei::{Componentname, NamecacheRbCache, NcMe};
use crate::sys::proc::Proc;
use crate::sys::queue::{ListEntry, ListHead, SlistEntry, TailqEntry, TailqHead};
use crate::sys::socketvar::Socket;
use crate::sys::specdev::Specinfo;
use crate::sys::time::Timespec;
use crate::sys::tree::{RbtEntry, RbtHead};
use crate::sys::types::{Daddr, Dev, Gid, Mode, Nlink, Register, Uid};
use crate::sys::ucred::{FSCRED, NOCRED, Ucred};
use crate::sys::uio::Uio;
use crate::tree_adapter;
use crate::uvm::uvm_vnode::UvmVnode;

/// `enum vtype`: vnode types. `VNON` means no type.
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(non_camel_case_types, clippy::upper_case_acronyms)] // OpenBSD names, verbatim
pub enum Vtype {
    /// No type.
    VNON,
    /// Regular file.
    VREG,
    /// Directory.
    VDIR,
    /// Block device.
    VBLK,
    /// Character device.
    VCHR,
    /// Symbolic link.
    VLNK,
    /// Socket.
    VSOCK,
    /// Fifo.
    VFIFO,
    /// A vnode that has been cleaned out (`vgone`).
    VBAD,
}

pub use Vtype::{VBAD, VBLK, VCHR, VDIR, VFIFO, VLNK, VNON, VREG, VSOCK};

/// `VTYPE_NAMES`.
pub const VTYPE_NAMES: [&str; 9] = [
    "VNON", "VREG", "VDIR", "VBLK", "VCHR", "VLNK", "VSOCK", "VFIFO", "VBAD",
];

/// `enum vtagtype`: vnode tag types. These are for the benefit of external programs only
/// (e.g., pstat) and should NEVER be inspected by the kernel. Note that `v_tag` is actually
/// used to tell MFS from FFS, and EXT2FS from the rest, so don't believe the above comment!
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(non_camel_case_types, clippy::upper_case_acronyms)] // OpenBSD names, verbatim
pub enum Vtagtype {
    /// No file system.
    VT_NON,
    /// `ufs`.
    VT_UFS,
    /// `nfs`.
    VT_NFS,
    /// `mfs`.
    VT_MFS,
    /// `msdosfs`.
    VT_MSDOSFS,
    /// Unused.
    VT_PORTAL,
    /// Unused.
    VT_PROCFS,
    /// Unused.
    VT_AFS,
    /// `cd9660`.
    VT_ISOFS,
    /// Unused.
    VT_ADOSFS,
    /// `ext2fs`.
    VT_EXT2FS,
    /// The syncer vnode.
    VT_VFS,
    /// `ntfs`.
    VT_NTFS,
    /// `udf`.
    VT_UDF,
    /// `fuse`.
    VT_FUSEFS,
    /// `tmpfs`.
    VT_TMPFS,
}

pub use Vtagtype::{
    VT_ADOSFS, VT_AFS, VT_EXT2FS, VT_FUSEFS, VT_ISOFS, VT_MFS, VT_MSDOSFS, VT_NFS, VT_NON, VT_NTFS,
    VT_PORTAL, VT_PROCFS, VT_TMPFS, VT_UDF, VT_UFS, VT_VFS,
};

/// `VTAG_NAMES`.
pub const VTAG_NAMES: [&str; 16] = [
    "NON", "UFS", "NFS", "MFS", "MSDOSFS", "unused", "unused", "unused", "ISOFS", "unused",
    "EXT2FS", "VFS", "NTFS", "UDF", "FUSEFS", "TMPFS",
];

/// `v_un`: what a vnode of a given type points at.
#[derive(Clone, Copy, Default)]
pub enum VnodeUn {
    /// Nothing (the C's NULL in every member).
    #[default]
    None,
    /// `vu_mountedhere`: ptr to mounted vfs (`VDIR`).
    Mountedhere(&'static Mount),
    /// `vu_socket`: unix ipc (`VSOCK`; the socket bound to the name, `uipc_usrreq.c`).
    Socket(&'static Socket),
    /// `vu_specinfo`: device (`VCHR`, `VBLK`).
    Specinfo(&'static Specinfo),
    /// `vu_fifoinfo`: fifo (`VFIFO`; `struct fifoinfo`, `fifo_vnops.c`).
    Fifoinfo(*mut c_void),
}

/// `struct vnode`.
///
/// Locks: \[a\] atomic, \[V\] `vnode_mtx`, \[B\] `IPL_BIO`; the rest the kernel lock.
pub struct Vnode {
    /// `v_uvm`: uvm data (the vnode pager's object, kept across recycling).
    pub v_uvm: Cell<Option<&'static UvmVnode>>,
    /// `v_op`: vnode operations vector.
    pub v_op: Cell<Option<&'static Vops>>,
    /// `v_type`: vnode type.
    pub v_type: Cell<Vtype>,
    /// `v_tag`: type of underlying data.
    pub v_tag: Cell<Vtagtype>,
    /// `v_flag`: vnode flags (see below).
    pub v_flag: Cell<u32>,
    /// \[V\] `v_lflag`: lock vnode flags.
    pub v_lflag: Cell<u32>,
    /// `v_usecount`: reference count of users.
    pub v_usecount: Cell<u32>,
    /// `v_uvcount`: unveil references.
    pub v_uvcount: Cell<u32>,
    /// `v_writecount`: reference count of writers.
    pub v_writecount: Cell<u32>,
    /// \[V\] `v_lockcount`: # threads waiting on lock.
    pub v_lockcount: Cell<u32>,
    /// \[B\] `v_bioflag`: flags accessed in interrupts.
    pub v_bioflag: Cell<u32>,
    /// \[B\] `v_holdcnt`: buffer references.
    pub v_holdcnt: Cell<u32>,
    /// `v_id`: capability identifier.
    pub v_id: Cell<u32>,
    /// `v_mount`: ptr to vfs we are in.
    pub v_mount: Cell<Option<&'static Mount>>,
    /// \[B\] `v_freelist`: vnode freelist.
    pub v_freelist: TailqEntry<Vnode>,
    /// `v_mntvnodes`: vnodes for mount point.
    pub v_mntvnodes: TailqEntry<Vnode>,
    /// \[B\] `v_bufs_tree`: lookup of all bufs.
    pub v_bufs_tree: RbtHead<BufRbBufs>,
    /// \[B\] `v_cleanblkhd`: clean blocklist head.
    pub v_cleanblkhd: Buflists,
    /// \[B\] `v_dirtyblkhd`: dirty blocklist head.
    pub v_dirtyblkhd: Buflists,
    /// \[B\] `v_numoutput`: num of writes in progress.
    pub v_numoutput: Cell<u32>,
    /// \[B\] `v_synclist`: vnode with dirty buffers.
    pub v_synclist: ListEntry<Vnode>,
    /// `v_un`: the union of `v_mountedhere`, `v_socket`, `v_specinfo`, `v_fifoinfo`.
    pub v_un: Cell<VnodeUn>,
    /// `si_specnext`: the special-device alias chain (see the module's deviations).
    pub v_specnext: SlistEntry<Vnode>,
    /// `v_nc_tree`: VFS namecache, the entries naming children of this directory.
    pub v_nc_tree: RbtHead<NamecacheRbCache>,
    /// `v_cache_dst`: cache entries to us.
    pub v_cache_dst: TailqHead<NcMe>,
    /// `v_data`: private data for fs.
    pub v_data: Cell<*mut c_void>,
    /// `v_klist`: identity of poller(s).
    pub v_klist: Klist,
}

// SAFETY: the members are mutated under the kernel lock, `vnode_mtx` or `splbio`, as in C;
// the kernel runs one CPU.
unsafe impl Sync for Vnode {}

impl Vnode {
    /// A zeroed vnode, as `pool_get(&vnode_pool, PR_ZERO)` returns it.
    pub const fn new() -> Self {
        Self {
            v_uvm: Cell::new(None),
            v_op: Cell::new(None),
            v_type: Cell::new(VNON),
            v_tag: Cell::new(VT_NON),
            v_flag: Cell::new(0),
            v_lflag: Cell::new(0),
            v_usecount: Cell::new(0),
            v_uvcount: Cell::new(0),
            v_writecount: Cell::new(0),
            v_lockcount: Cell::new(0),
            v_bioflag: Cell::new(0),
            v_holdcnt: Cell::new(0),
            v_id: Cell::new(0),
            v_mount: Cell::new(None),
            v_freelist: TailqEntry::new(),
            v_mntvnodes: TailqEntry::new(),
            v_bufs_tree: RbtHead::new(),
            v_cleanblkhd: ListHead::new(),
            v_dirtyblkhd: ListHead::new(),
            v_numoutput: Cell::new(0),
            v_synclist: ListEntry::new(),
            v_un: Cell::new(VnodeUn::None),
            v_specnext: SlistEntry::new(),
            v_nc_tree: RbtHead::new(),
            v_cache_dst: TailqHead::new(),
            v_data: Cell::new(ptr::null_mut()),
            v_klist: Klist::new(),
        }
    }

    /// `vp->v_op`, which every vnode out of `getnewvnode` has.
    pub fn op(&self) -> &'static Vops {
        match self.v_op.get() {
            Some(ops) => ops,
            None => panic(format_args!("vnode {:p}: no v_op", self)),
        }
    }

    /// `vp->v_mountedhere`.
    pub fn v_mountedhere(&self) -> Option<&'static Mount> {
        match self.v_un.get() {
            VnodeUn::Mountedhere(mp) => Some(mp),
            _ => None,
        }
    }

    /// `vp->v_mountedhere = mp`.
    pub fn set_v_mountedhere(&self, mp: Option<&'static Mount>) {
        self.v_un
            .set(mp.map_or(VnodeUn::None, VnodeUn::Mountedhere));
    }

    /// `vp->v_socket`: the socket bound to a `VSOCK` vnode's name.
    pub fn v_socket(&self) -> Option<&'static Socket> {
        match self.v_un.get() {
            VnodeUn::Socket(so) => Some(so),
            _ => None,
        }
    }

    /// `vp->v_socket = so`.
    pub fn set_v_socket(&self, so: Option<&'static Socket>) {
        self.v_un.set(so.map_or(VnodeUn::None, VnodeUn::Socket));
    }

    /// `vp->v_specinfo`.
    pub fn v_specinfo(&self) -> Option<&'static Specinfo> {
        match self.v_un.get() {
            VnodeUn::Specinfo(si) => Some(si),
            _ => None,
        }
    }

    /// `vp->v_specinfo` of a device vnode, which `checkalias` gave one.
    fn specinfo(&self) -> &'static Specinfo {
        match self.v_specinfo() {
            Some(si) => si,
            None => panic(format_args!("vnode {:p}: no v_specinfo", self)),
        }
    }

    /// `vp->v_rdev` (`v_specinfo->si_rdev`).
    pub fn v_rdev(&self) -> Dev {
        self.specinfo().si_rdev.get()
    }

    /// `vp->v_specmountpoint` (`v_specinfo->si_mountpoint`).
    pub fn v_specmountpoint(&self) -> Option<&'static Mount> {
        self.specinfo().si_mountpoint.get()
    }

    /// `vp->v_specparent` (`v_specinfo->si_ci.ci_parent`).
    pub fn v_specparent(&self) -> Option<&'static Vnode> {
        self.specinfo().si_ci_parent.get()
    }

    /// `vp->v_data` as the file system's node type.
    ///
    /// # Safety
    ///
    /// `v_data` points at a live `T`: the caller is the file system that stored it (its `v_tag`
    /// and `v_op` say so) and the node is not reclaimed.
    pub unsafe fn data<T>(&self) -> &T {
        // SAFETY: the caller's contract.
        unsafe { &*self.v_data.get().cast::<T>() }
    }
}

impl Default for Vnode {
    fn default() -> Self {
        Self::new()
    }
}

queue_adapter!(
    /// `LIST_HEAD(buflists, buf)`: a vnode's clean or dirty buffers, through `b_vnbufs`.
    pub BVnbufs: Buf, b_vnbufs => ListEntry<Buf>
);

/// `struct buflists`.
pub type Buflists = ListHead<BVnbufs>;

tree_adapter!(
    /// `RBT_HEAD(buf_rb_bufs, buf)`: a vnode's buffers by logical block, through `b_rbbufs`.
    pub BufRbBufs: Buf, b_rbbufs => RbtEntry, crate::kern::vfs_subr::rb_buf_compare
);

queue_adapter!(
    /// `TAILQ_HEAD(freelst, vnode)`: the vnode free and hold lists, through `v_freelist`.
    pub VFreelist: Vnode, v_freelist => TailqEntry<Vnode>
);

queue_adapter!(
    /// `TAILQ_HEAD(, vnode) mnt_vnodelist`: the vnodes of a mount, through `v_mntvnodes`.
    pub VMntvnodes: Vnode, v_mntvnodes => TailqEntry<Vnode>
);

queue_adapter!(
    /// `SLIST_HEAD(vnodechain, vnode)`: a special-device alias chain, through `v_specnext`.
    pub VSpecnext: Vnode, v_specnext => SlistEntry<Vnode>
);

queue_adapter!(
    /// The syncer worklist, through `v_synclist` (`vfs_sync.c`).
    pub VSynclist: Vnode, v_synclist => ListEntry<Vnode>
);

/// `VROOT`: root of its file system.
pub const VROOT: u32 = 0x0001;
/// `VTEXT`: vnode is a pure text prototype.
pub const VTEXT: u32 = 0x0002;
/// `VSYSTEM`: vnode being used by kernel.
pub const VSYSTEM: u32 = 0x0004;
/// `VISTTY`: vnode represents a tty.
pub const VISTTY: u32 = 0x0008;
/// `VXLOCK`: vnode is locked to change underlying type.
pub const VXLOCK: u32 = 0x0100;
/// `VXWANT`: process is waiting for vnode.
pub const VXWANT: u32 = 0x0200;
/// `VCLONED`: vnode was cloned.
pub const VCLONED: u32 = 0x0400;
/// `VALIASED`: vnode has an alias.
pub const VALIASED: u32 = 0x0800;
/// `VLARVAL`: vnode data not yet set up by higher level.
pub const VLARVAL: u32 = 0x1000;
/// `VLOCKSWORK`: FS supports locking discipline.
pub const VLOCKSWORK: u32 = 0x4000;
/// `VCLONE`: vnode is a clone.
pub const VCLONE: u32 = 0x8000;

/// `VBIOWAIT`: waiting for output to complete.
pub const VBIOWAIT: u32 = 0x0001;
/// `VBIOONSYNCLIST`: Vnode is on syncer worklist.
pub const VBIOONSYNCLIST: u32 = 0x0002;
/// `VBIOONFREELIST`: Vnode is on a free list.
pub const VBIOONFREELIST: u32 = 0x0004;
/// `VBIOERROR`: A write failed.
pub const VBIOERROR: u32 = 0x0008;

/// `struct vattr`: vnode attributes. A field value of `VNOVAL` represents a field whose value
/// is unavailable (getattr) or which is not to be changed (setattr). For the timespec fields,
/// only the `tv_nsec` member needs to be set to `VNOVAL`: if `tv_nsec != VNOVAL` then both
/// `tv_sec` and `tv_nsec` are valid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Vattr {
    /// `va_type`: vnode type (for create).
    pub va_type: Vtype,
    /// `va_mode`: files access mode and type.
    pub va_mode: Mode,
    /// `va_nlink`: number of references to file.
    pub va_nlink: Nlink,
    /// `va_uid`: owner user id.
    pub va_uid: Uid,
    /// `va_gid`: owner group id.
    pub va_gid: Gid,
    /// `va_fsid`: file system id (dev for now).
    pub va_fsid: i64,
    /// `va_fileid`: file id.
    pub va_fileid: u64,
    /// `va_size`: file size in bytes.
    pub va_size: u64,
    /// `va_blocksize`: blocksize preferred for i/o.
    pub va_blocksize: i64,
    /// `va_atime`: time of last access.
    pub va_atime: Timespec,
    /// `va_mtime`: time of last modification.
    pub va_mtime: Timespec,
    /// `va_ctime`: time file changed.
    pub va_ctime: Timespec,
    /// `va_gen`: generation number of file.
    pub va_gen: u64,
    /// `va_flags`: flags defined for file.
    pub va_flags: u64,
    /// `va_rdev`: device the special file represents.
    pub va_rdev: Dev,
    /// `va_bytes`: bytes of disk space held by file.
    pub va_bytes: u64,
    /// `va_filerev`: file modification number.
    pub va_filerev: u64,
    /// `va_vaflags`: operations flags, see below.
    pub va_vaflags: u32,
    /// `va_spare`: remain quad aligned.
    pub va_spare: i64,
}

impl Vattr {
    /// An all-zero `struct vattr` (a stack variable before `vattr_null` or a `getattr`).
    pub const fn new() -> Self {
        Self {
            va_type: VNON,
            va_mode: 0,
            va_nlink: 0,
            va_uid: 0,
            va_gid: 0,
            va_fsid: 0,
            va_fileid: 0,
            va_size: 0,
            va_blocksize: 0,
            va_atime: Timespec::new(0, 0),
            va_mtime: Timespec::new(0, 0),
            va_ctime: Timespec::new(0, 0),
            va_gen: 0,
            va_flags: 0,
            va_rdev: 0,
            va_bytes: 0,
            va_filerev: 0,
            va_vaflags: 0,
            va_spare: 0,
        }
    }
}

impl Default for Vattr {
    fn default() -> Self {
        Self::new()
    }
}

/// `VA_UTIMES_NULL`: utimes argument was NULL.
pub const VA_UTIMES_NULL: u32 = 0x01;
/// `VA_EXCLUSIVE`: exclusive create request.
pub const VA_EXCLUSIVE: u32 = 0x02;
/// `VA_UTIMES_CHANGE`: ctime should be updated.
pub const VA_UTIMES_CHANGE: u32 = 0x04;

/// `IO_UNIT`: do I/O as atomic unit.
pub const IO_UNIT: i32 = 0x01;
/// `IO_APPEND`: append write to end.
pub const IO_APPEND: i32 = 0x02;
/// `IO_SYNC`: do I/O synchronously.
pub const IO_SYNC: i32 = 0x04;
/// `IO_NODELOCKED`: underlying node already locked.
pub const IO_NODELOCKED: i32 = 0x08;
/// `IO_NDELAY`: `FNDELAY` flag set in file table.
pub const IO_NDELAY: i32 = 0x10;
/// `IO_NOLIMIT`: don't enforce limits on i/o.
pub const IO_NOLIMIT: i32 = 0x20;
/// `IO_NOCACHE`: don't cache result of this i/o.
pub const IO_NOCACHE: i32 = 0x40;

/// `VSUID`: set user id on execution.
pub const VSUID: Mode = 0o4000;
/// `VSGID`: set group id on execution.
pub const VSGID: Mode = 0o2000;
/// `VSVTX`: save swapped text even after use.
pub const VSVTX: Mode = 0o1000;
/// `VREAD`: read permission.
pub const VREAD: i32 = 0o0400;
/// `VWRITE`: write permission.
pub const VWRITE: i32 = 0o0200;
/// `VEXEC`: execute permission.
pub const VEXEC: i32 = 0o0100;

/// `VNOVAL`: token indicating no attribute value yet assigned. Stored into the unsigned
/// fields as the C's conversion does (all bits set).
pub const VNOVAL: i32 = -1;

/// `IFTOVT(mode)`: the vnode type of an inode format.
pub fn iftovt(mode: Mode) -> Vtype {
    crate::kern::vfs_subr::IFTOVT_TAB[((mode & crate::sys::stat::S_IFMT) >> 12) as usize]
}

/// `VTTOIF(indx)`: the inode format of a vnode type.
pub fn vttoif(indx: Vtype) -> Mode {
    crate::kern::vfs_subr::VTTOIF_TAB[indx as usize]
}

/// `MAKEIMODE(indx, mode)`.
pub fn makeimode(indx: Vtype, mode: Mode) -> Mode {
    vttoif(indx) | mode
}

/// `VN_KNOTE(vp, b)`: posts the vnode event `b` (`NOTE_*`) to the vnode's knotes.
#[allow(non_snake_case)] // the C name
pub fn VN_KNOTE(vp: &Vnode, b: u32) {
    crate::kern::kern_event::knote_locked(&vp.v_klist, i64::from(b));
}

/// `SKIPSYSTEM`: vflush: skip vnodes marked `VSYSTEM`.
pub const SKIPSYSTEM: i32 = 0x0001;
/// `FORCECLOSE`: vflush: force file closure.
pub const FORCECLOSE: i32 = 0x0002;
/// `WRITECLOSE`: vflush: only close writeable files.
pub const WRITECLOSE: i32 = 0x0004;
/// `DOCLOSE`: vclean: close active files.
pub const DOCLOSE: i32 = 0x0008;
/// `IGNORECLEAN`: vflush: ignore clean vnodes.
pub const IGNORECLEAN: i32 = 0x0010;
/// `V_SAVE`: vinvalbuf: sync file first.
pub const V_SAVE: i32 = 0x0001;
/// `V_SAVEMETA`: vinvalbuf: leave indirect blocks.
pub const V_SAVEMETA: i32 = 0x0002;

/// `REVOKEALL`: vop_revoke: revoke all aliases.
pub const REVOKEALL: i32 = 0x0001;

/// `GETCWD_CHECK_ACCESS`: `vfs_getcwd_common` checks search (and then read) access on the
/// way up.
pub const GETCWD_CHECK_ACCESS: i32 = 0x0001;

/// The credentials of a `VOP_*` argument (`a_cred`) when they are real ones: `None` for NULL,
/// `NOCRED` and `FSCRED`.
///
/// # Safety
///
/// `cred` is NULL, `NOCRED`, `FSCRED` or a credential its holder keeps alive for `'a`: the
/// `a_cred` a vnode operation receives qualifies for the operation's duration (the caller
/// holds the thread's `p_ucred` or the file's `f_cred`).
pub unsafe fn cred_ref<'a>(cred: *const Ucred) -> Option<&'a Ucred> {
    if cred.is_null() || ptr::eq(cred, NOCRED) || ptr::eq(cred, FSCRED) {
        None
    } else {
        // SAFETY: the caller's contract.
        Some(unsafe { &*cred })
    }
}

/// `struct vop_generic_args`.
pub struct VopGenericArgs {
    /// `a_garbage`: other data probably follows.
    pub a_garbage: *mut c_void,
}

/// `struct vop_islocked_args`.
pub struct VopIslockedArgs {
    /// `a_vp`.
    pub a_vp: &'static Vnode,
}

/// `struct vop_lookup_args`.
pub struct VopLookupArgs<'a> {
    /// `a_dvp`: the directory searched.
    pub a_dvp: &'static Vnode,
    /// `a_vpp`: the vnode found, NULL when there is none.
    pub a_vpp: &'a mut Option<&'static Vnode>,
    /// `a_cnp`: the component looked up.
    pub a_cnp: &'a mut Componentname,
}

/// `struct vop_create_args`.
pub struct VopCreateArgs<'a> {
    /// `a_dvp`.
    pub a_dvp: &'static Vnode,
    /// `a_vpp`.
    pub a_vpp: &'a mut Option<&'static Vnode>,
    /// `a_cnp`.
    pub a_cnp: &'a mut Componentname,
    /// `a_vap`.
    pub a_vap: &'a mut Vattr,
}

/// `struct vop_mknod_args`.
pub struct VopMknodArgs<'a> {
    /// `a_dvp`.
    pub a_dvp: &'static Vnode,
    /// `a_vpp`.
    pub a_vpp: &'a mut Option<&'static Vnode>,
    /// `a_cnp`.
    pub a_cnp: &'a mut Componentname,
    /// `a_vap`.
    pub a_vap: &'a mut Vattr,
}

/// `struct vop_open_args`.
pub struct VopOpenArgs<'a> {
    /// `a_vp`.
    pub a_vp: &'static Vnode,
    /// `a_mode`.
    pub a_mode: i32,
    /// `a_cred`.
    pub a_cred: *const Ucred,
    /// `a_p`.
    pub a_p: &'a Proc,
}

/// `struct vop_close_args`.
pub struct VopCloseArgs<'a> {
    /// `a_vp`.
    pub a_vp: &'static Vnode,
    /// `a_fflag`.
    pub a_fflag: i32,
    /// `a_cred`.
    pub a_cred: *const Ucred,
    /// `a_p`: NULL when no thread closes (a file in a message).
    pub a_p: Option<&'a Proc>,
}

/// `struct vop_access_args`.
pub struct VopAccessArgs<'a> {
    /// `a_vp`.
    pub a_vp: &'static Vnode,
    /// `a_mode`.
    pub a_mode: i32,
    /// `a_cred`.
    pub a_cred: *const Ucred,
    /// `a_p`.
    pub a_p: &'a Proc,
}

/// `struct vop_getattr_args`.
pub struct VopGetattrArgs<'a> {
    /// `a_vp`.
    pub a_vp: &'static Vnode,
    /// `a_vap`.
    pub a_vap: &'a mut Vattr,
    /// `a_cred`.
    pub a_cred: *const Ucred,
    /// `a_p`.
    pub a_p: &'a Proc,
}

/// `struct vop_setattr_args`.
pub struct VopSetattrArgs<'a> {
    /// `a_vp`.
    pub a_vp: &'static Vnode,
    /// `a_vap`.
    pub a_vap: &'a mut Vattr,
    /// `a_cred`.
    pub a_cred: *const Ucred,
    /// `a_p`.
    pub a_p: &'a Proc,
}

/// `struct vop_read_args`.
pub struct VopReadArgs<'a, 'b> {
    /// `a_vp`.
    pub a_vp: &'static Vnode,
    /// `a_uio`.
    pub a_uio: &'a mut Uio<'b>,
    /// `a_ioflag`.
    pub a_ioflag: i32,
    /// `a_cred`.
    pub a_cred: *const Ucred,
}

/// `struct vop_write_args`.
pub struct VopWriteArgs<'a, 'b> {
    /// `a_vp`.
    pub a_vp: &'static Vnode,
    /// `a_uio`.
    pub a_uio: &'a mut Uio<'b>,
    /// `a_ioflag`.
    pub a_ioflag: i32,
    /// `a_cred`.
    pub a_cred: *const Ucred,
}

/// `struct vop_ioctl_args`.
pub struct VopIoctlArgs<'a> {
    /// `a_vp`.
    pub a_vp: &'static Vnode,
    /// `a_command`.
    pub a_command: u64,
    /// `a_data`: the kernel copy of the argument.
    pub a_data: &'a mut [u8],
    /// `a_fflag`.
    pub a_fflag: i32,
    /// `a_cred`.
    pub a_cred: *const Ucred,
    /// `a_p`.
    pub a_p: &'a Proc,
}

/// `struct vop_kqfilter_args`.
pub struct VopKqfilterArgs<'a> {
    /// `a_vp`.
    pub a_vp: &'static Vnode,
    /// `a_fflag`.
    pub a_fflag: i32,
    /// `a_kn`.
    pub a_kn: &'a Knote,
}

/// `struct vop_revoke_args`.
pub struct VopRevokeArgs {
    /// `a_vp`.
    pub a_vp: &'static Vnode,
    /// `a_flags`.
    pub a_flags: i32,
}

/// `struct vop_fsync_args`.
pub struct VopFsyncArgs<'a> {
    /// `a_vp`.
    pub a_vp: &'static Vnode,
    /// `a_cred`.
    pub a_cred: *const Ucred,
    /// `a_waitfor`.
    pub a_waitfor: i32,
    /// `a_p`.
    pub a_p: &'a Proc,
}

/// `struct vop_remove_args`.
pub struct VopRemoveArgs<'a> {
    /// `a_dvp`.
    pub a_dvp: &'static Vnode,
    /// `a_vp`.
    pub a_vp: &'static Vnode,
    /// `a_cnp`.
    pub a_cnp: &'a mut Componentname,
}

/// `struct vop_link_args`.
pub struct VopLinkArgs<'a> {
    /// `a_dvp`.
    pub a_dvp: &'static Vnode,
    /// `a_vp`.
    pub a_vp: &'static Vnode,
    /// `a_cnp`.
    pub a_cnp: &'a mut Componentname,
}

/// `struct vop_rename_args`.
pub struct VopRenameArgs<'a> {
    /// `a_fdvp`.
    pub a_fdvp: &'static Vnode,
    /// `a_fvp`.
    pub a_fvp: &'static Vnode,
    /// `a_fcnp`.
    pub a_fcnp: &'a mut Componentname,
    /// `a_tdvp`.
    pub a_tdvp: &'static Vnode,
    /// `a_tvp`: NULL when the target does not exist.
    pub a_tvp: Option<&'static Vnode>,
    /// `a_tcnp`.
    pub a_tcnp: &'a mut Componentname,
}

/// `struct vop_mkdir_args`.
pub struct VopMkdirArgs<'a> {
    /// `a_dvp`.
    pub a_dvp: &'static Vnode,
    /// `a_vpp`.
    pub a_vpp: &'a mut Option<&'static Vnode>,
    /// `a_cnp`.
    pub a_cnp: &'a mut Componentname,
    /// `a_vap`.
    pub a_vap: &'a mut Vattr,
}

/// `struct vop_rmdir_args`.
pub struct VopRmdirArgs<'a> {
    /// `a_dvp`.
    pub a_dvp: &'static Vnode,
    /// `a_vp`.
    pub a_vp: &'static Vnode,
    /// `a_cnp`.
    pub a_cnp: &'a mut Componentname,
}

/// `struct vop_symlink_args`.
pub struct VopSymlinkArgs<'a> {
    /// `a_dvp`.
    pub a_dvp: &'static Vnode,
    /// `a_vpp`.
    pub a_vpp: &'a mut Option<&'static Vnode>,
    /// `a_cnp`.
    pub a_cnp: &'a mut Componentname,
    /// `a_vap`.
    pub a_vap: &'a mut Vattr,
    /// `a_target`: the link's contents, without the NUL.
    pub a_target: &'a [u8],
}

/// `struct vop_readdir_args`.
pub struct VopReaddirArgs<'a, 'b> {
    /// `a_vp`.
    pub a_vp: &'static Vnode,
    /// `a_uio`.
    pub a_uio: &'a mut Uio<'b>,
    /// `a_cred`.
    pub a_cred: *const Ucred,
    /// `a_eofflag`.
    pub a_eofflag: &'a mut i32,
}

/// `struct vop_readlink_args`.
pub struct VopReadlinkArgs<'a, 'b> {
    /// `a_vp`.
    pub a_vp: &'static Vnode,
    /// `a_uio`.
    pub a_uio: &'a mut Uio<'b>,
    /// `a_cred`.
    pub a_cred: *const Ucred,
}

/// `struct vop_abortop_args`.
pub struct VopAbortopArgs<'a> {
    /// `a_dvp`.
    pub a_dvp: &'static Vnode,
    /// `a_cnp`.
    pub a_cnp: &'a mut Componentname,
}

/// `struct vop_inactive_args`.
pub struct VopInactiveArgs<'a> {
    /// `a_vp`.
    pub a_vp: &'static Vnode,
    /// `a_p`: `curproc`, NULL before process 0 exists.
    pub a_p: Option<&'a Proc>,
}

/// `struct vop_reclaim_args`.
pub struct VopReclaimArgs<'a> {
    /// `a_vp`.
    pub a_vp: &'static Vnode,
    /// `a_p`: `curproc`, NULL before process 0 exists.
    pub a_p: Option<&'a Proc>,
}

/// `struct vop_lock_args`.
pub struct VopLockArgs {
    /// `a_vp`.
    pub a_vp: &'static Vnode,
    /// `a_flags`: `LK_*`.
    pub a_flags: i32,
}

/// `struct vop_unlock_args`.
pub struct VopUnlockArgs {
    /// `a_vp`.
    pub a_vp: &'static Vnode,
}

/// `struct vop_bmap_args`.
pub struct VopBmapArgs<'a> {
    /// `a_vp`.
    pub a_vp: &'static Vnode,
    /// `a_bn`.
    pub a_bn: Daddr,
    /// `a_vpp`: NULL when the caller does not want it.
    pub a_vpp: Option<&'a mut Option<&'static Vnode>>,
    /// `a_bnp`: NULL when the caller does not want it.
    pub a_bnp: Option<&'a mut Daddr>,
    /// `a_runp`: NULL when the caller does not want it.
    pub a_runp: Option<&'a mut i32>,
}

/// `struct vop_print_args`.
pub struct VopPrintArgs {
    /// `a_vp`.
    pub a_vp: &'static Vnode,
}

/// `struct vop_pathconf_args`.
pub struct VopPathconfArgs<'a> {
    /// `a_vp`.
    pub a_vp: &'static Vnode,
    /// `a_name`.
    pub a_name: i32,
    /// `a_retval`.
    pub a_retval: &'a mut Register,
}

/// `struct vop_advlock_args`.
pub struct VopAdvlockArgs<'a> {
    /// `a_vp`.
    pub a_vp: &'static Vnode,
    /// `a_id`: the lock's owner (a `struct file *` or a `struct process *`).
    pub a_id: *const c_void,
    /// `a_op`.
    pub a_op: i32,
    /// `a_fl`.
    pub a_fl: &'a mut Flock,
    /// `a_flags`.
    pub a_flags: i32,
}

/// `struct vop_strategy_args`.
pub struct VopStrategyArgs {
    /// `a_vp`.
    pub a_vp: &'static Vnode,
    /// `a_bp`.
    pub a_bp: &'static Buf,
}

/// `struct vop_bwrite_args`: a special case.
pub struct VopBwriteArgs {
    /// `a_bp`.
    pub a_bp: &'static Buf,
}

/// The type of `vop_lock`.
pub type VopLockFn = fn(&mut VopLockArgs) -> Result<(), Errno>;
/// The type of `vop_unlock`.
pub type VopUnlockFn = fn(&mut VopUnlockArgs) -> Result<(), Errno>;
/// The type of `vop_islocked`: the lock status, as the C returns it.
pub type VopIslockedFn = fn(&mut VopIslockedArgs) -> i32;
/// The type of `vop_abortop`.
pub type VopAbortopFn = fn(&mut VopAbortopArgs<'_>) -> Result<(), Errno>;
/// The type of `vop_access`.
pub type VopAccessFn = fn(&mut VopAccessArgs<'_>) -> Result<(), Errno>;
/// The type of `vop_advlock`.
pub type VopAdvlockFn = fn(&mut VopAdvlockArgs<'_>) -> Result<(), Errno>;
/// The type of `vop_bmap`.
pub type VopBmapFn = fn(&mut VopBmapArgs<'_>) -> Result<(), Errno>;
/// The type of `vop_bwrite`.
pub type VopBwriteFn = fn(&mut VopBwriteArgs) -> Result<(), Errno>;
/// The type of `vop_close`.
pub type VopCloseFn = fn(&mut VopCloseArgs<'_>) -> Result<(), Errno>;
/// The type of `vop_create`.
pub type VopCreateFn = fn(&mut VopCreateArgs<'_>) -> Result<(), Errno>;
/// The type of `vop_fsync`.
pub type VopFsyncFn = fn(&mut VopFsyncArgs<'_>) -> Result<(), Errno>;
/// The type of `vop_getattr`.
pub type VopGetattrFn = fn(&mut VopGetattrArgs<'_>) -> Result<(), Errno>;
/// The type of `vop_inactive`.
pub type VopInactiveFn = fn(&mut VopInactiveArgs<'_>) -> Result<(), Errno>;
/// The type of `vop_ioctl`.
pub type VopIoctlFn = fn(&mut VopIoctlArgs<'_>) -> Result<(), Errno>;
/// The type of `vop_link`.
pub type VopLinkFn = fn(&mut VopLinkArgs<'_>) -> Result<(), Errno>;
/// The type of `vop_lookup`.
pub type VopLookupFn = fn(&mut VopLookupArgs<'_>) -> Result<(), Errno>;
/// The type of `vop_mknod`.
pub type VopMknodFn = fn(&mut VopMknodArgs<'_>) -> Result<(), Errno>;
/// The type of `vop_open`.
pub type VopOpenFn = fn(&mut VopOpenArgs<'_>) -> Result<(), Errno>;
/// The type of `vop_pathconf`.
pub type VopPathconfFn = fn(&mut VopPathconfArgs<'_>) -> Result<(), Errno>;
/// The type of `vop_print`.
pub type VopPrintFn = fn(&mut VopPrintArgs) -> Result<(), Errno>;
/// The type of `vop_read`.
pub type VopReadFn = fn(&mut VopReadArgs<'_, '_>) -> Result<(), Errno>;
/// The type of `vop_readdir`.
pub type VopReaddirFn = fn(&mut VopReaddirArgs<'_, '_>) -> Result<(), Errno>;
/// The type of `vop_readlink`.
pub type VopReadlinkFn = fn(&mut VopReadlinkArgs<'_, '_>) -> Result<(), Errno>;
/// The type of `vop_reclaim`.
pub type VopReclaimFn = fn(&mut VopReclaimArgs<'_>) -> Result<(), Errno>;
/// The type of `vop_remove`.
pub type VopRemoveFn = fn(&mut VopRemoveArgs<'_>) -> Result<(), Errno>;
/// The type of `vop_rename`.
pub type VopRenameFn = fn(&mut VopRenameArgs<'_>) -> Result<(), Errno>;
/// The type of `vop_revoke`.
pub type VopRevokeFn = fn(&mut VopRevokeArgs) -> Result<(), Errno>;
/// The type of `vop_mkdir`.
pub type VopMkdirFn = fn(&mut VopMkdirArgs<'_>) -> Result<(), Errno>;
/// The type of `vop_rmdir`.
pub type VopRmdirFn = fn(&mut VopRmdirArgs<'_>) -> Result<(), Errno>;
/// The type of `vop_setattr`.
pub type VopSetattrFn = fn(&mut VopSetattrArgs<'_>) -> Result<(), Errno>;
/// The type of `vop_strategy`.
pub type VopStrategyFn = fn(&mut VopStrategyArgs) -> Result<(), Errno>;
/// The type of `vop_symlink`.
pub type VopSymlinkFn = fn(&mut VopSymlinkArgs<'_>) -> Result<(), Errno>;
/// The type of `vop_write`.
pub type VopWriteFn = fn(&mut VopWriteArgs<'_, '_>) -> Result<(), Errno>;
/// The type of `vop_kqfilter`.
pub type VopKqfilterFn = fn(&mut VopKqfilterArgs<'_>) -> Result<(), Errno>;

/// `struct vops`: vnode operations. `None` is the C's NULL: the `VOP_*` wrapper answers
/// `EOPNOTSUPP` without calling anything.
pub struct Vops {
    /// `vop_lock`.
    pub vop_lock: Option<VopLockFn>,
    /// `vop_unlock`.
    pub vop_unlock: Option<VopUnlockFn>,
    /// `vop_islocked`.
    pub vop_islocked: Option<VopIslockedFn>,
    /// `vop_abortop`.
    pub vop_abortop: Option<VopAbortopFn>,
    /// `vop_access`.
    pub vop_access: Option<VopAccessFn>,
    /// `vop_advlock`.
    pub vop_advlock: Option<VopAdvlockFn>,
    /// `vop_bmap`.
    pub vop_bmap: Option<VopBmapFn>,
    /// `vop_bwrite`.
    pub vop_bwrite: Option<VopBwriteFn>,
    /// `vop_close`.
    pub vop_close: Option<VopCloseFn>,
    /// `vop_create`.
    pub vop_create: Option<VopCreateFn>,
    /// `vop_fsync`.
    pub vop_fsync: Option<VopFsyncFn>,
    /// `vop_getattr`.
    pub vop_getattr: Option<VopGetattrFn>,
    /// `vop_inactive`.
    pub vop_inactive: Option<VopInactiveFn>,
    /// `vop_ioctl`.
    pub vop_ioctl: Option<VopIoctlFn>,
    /// `vop_link`.
    pub vop_link: Option<VopLinkFn>,
    /// `vop_lookup`.
    pub vop_lookup: Option<VopLookupFn>,
    /// `vop_mknod`.
    pub vop_mknod: Option<VopMknodFn>,
    /// `vop_open`.
    pub vop_open: Option<VopOpenFn>,
    /// `vop_pathconf`.
    pub vop_pathconf: Option<VopPathconfFn>,
    /// `vop_print`.
    pub vop_print: Option<VopPrintFn>,
    /// `vop_read`.
    pub vop_read: Option<VopReadFn>,
    /// `vop_readdir`.
    pub vop_readdir: Option<VopReaddirFn>,
    /// `vop_readlink`.
    pub vop_readlink: Option<VopReadlinkFn>,
    /// `vop_reclaim`.
    pub vop_reclaim: Option<VopReclaimFn>,
    /// `vop_remove`.
    pub vop_remove: Option<VopRemoveFn>,
    /// `vop_rename`.
    pub vop_rename: Option<VopRenameFn>,
    /// `vop_revoke`.
    pub vop_revoke: Option<VopRevokeFn>,
    /// `vop_mkdir`.
    pub vop_mkdir: Option<VopMkdirFn>,
    /// `vop_rmdir`.
    pub vop_rmdir: Option<VopRmdirFn>,
    /// `vop_setattr`.
    pub vop_setattr: Option<VopSetattrFn>,
    /// `vop_strategy`.
    pub vop_strategy: Option<VopStrategyFn>,
    /// `vop_symlink`.
    pub vop_symlink: Option<VopSymlinkFn>,
    /// `vop_write`.
    pub vop_write: Option<VopWriteFn>,
    /// `vop_kqfilter`.
    pub vop_kqfilter: Option<VopKqfilterFn>,
}

impl Vops {
    /// A vector with every slot NULL, for `..Vops::EMPTY` in a file system's table.
    pub const EMPTY: Vops = Vops {
        vop_lock: None,
        vop_unlock: None,
        vop_islocked: None,
        vop_abortop: None,
        vop_access: None,
        vop_advlock: None,
        vop_bmap: None,
        vop_bwrite: None,
        vop_close: None,
        vop_create: None,
        vop_fsync: None,
        vop_getattr: None,
        vop_inactive: None,
        vop_ioctl: None,
        vop_link: None,
        vop_lookup: None,
        vop_mknod: None,
        vop_open: None,
        vop_pathconf: None,
        vop_print: None,
        vop_read: None,
        vop_readdir: None,
        vop_readlink: None,
        vop_reclaim: None,
        vop_remove: None,
        vop_rename: None,
        vop_revoke: None,
        vop_mkdir: None,
        vop_rmdir: None,
        vop_setattr: None,
        vop_strategy: None,
        vop_symlink: None,
        vop_write: None,
        vop_kqfilter: None,
    };
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/sys/vnode.h");
        for (name, value) in [
            ("VROOT", i64::from(VROOT)),
            ("VTEXT", i64::from(VTEXT)),
            ("VISTTY", i64::from(VISTTY)),
            ("VXLOCK", i64::from(VXLOCK)),
            ("VXWANT", i64::from(VXWANT)),
            ("VALIASED", i64::from(VALIASED)),
            ("VCLONE", i64::from(VCLONE)),
            ("VBIOONFREELIST", i64::from(VBIOONFREELIST)),
            ("VA_UTIMES_CHANGE", i64::from(VA_UTIMES_CHANGE)),
            ("IO_NOCACHE", i64::from(IO_NOCACHE)),
            ("VEXEC", i64::from(VEXEC)),
            ("VSUID", i64::from(VSUID)),
            ("IGNORECLEAN", i64::from(IGNORECLEAN)),
            ("REVOKEALL", i64::from(REVOKEALL)),
        ] {
            assert_eq!(crate::reftest::int(&defs, name), Some(value), "{name}");
        }
    }
}
/* </TESTS> */
