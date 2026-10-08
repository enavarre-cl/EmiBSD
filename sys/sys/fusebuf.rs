/* $OpenBSD: fusebuf.h,v 1.17 2026/06/17 13:29:01 helg Exp $ */
/* <LICENSES> */
/*
 * Copyright (c) 2013 Sylvestre Gallon
 * Copyright (c) 2013 Martin Pieuchot
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
/* </LICENSES> */

/* <CODE> */
//! `<sys/fusebuf.h>`: the FUSE kernel interface. The `fuse_*` structures are the binary
//! protocol between fuse(4) and the userland file system daemon (libfuse), the Linux FUSE
//! kernel interface 7.19 that OpenBSD somewhat emulates; `struct fusebuf` is the kernel's
//! request and reply buffer.
//!
//! Upstream: sys/sys/fusebuf.h @ 3ce1f3f79392
//!
//! An operation is issued by the kernel through fuse(4) when the userland file system needs
//! to execute an action (mkdir(2), link(2), etc). The daemon reads it from `/dev/fuse0` as a
//! `struct fuse_in_header`, the operation's input structure (`op_in_len` bytes) and the data
//! (`fb_len` bytes), and answers with a `struct fuse_out_header` carrying the same ID
//! (`unique`), the operation's output structure (`op_out_len` bytes) and, for the operations
//! that expect it (`op_out_buf`), data.
//!
//! The protocol structures are `#[repr(C)]` with the C's members (every hole is a named
//! member already), their sizes and offsets pinned by compile-time checks, and marked
//! [`AbiPod`]: they are copied to and from the daemon as bytes.
//!
//! ## Deviations
//! - `struct fusebuf`'s two unions (`op.in` and `op.out`, which overlap) are one byte area,
//!   [`FusebufOp`], read and written as whole members (`fbuf.op_get::<FuseEntryOut>()`,
//!   `fbuf.op_set(&open_in)`); the area is zeroed by `fb_setup` as the C's `PR_ZERO`, so
//!   building a member from its `Default` and setting the members the C sets is the C's
//!   member-by-member stores.
//! - `struct fusebuf` is shared by the requesting thread and the device through pointers, as
//!   in C: its members are `Cell`s (the header a `Cell<FuseInHeader>`), the `fb_*` field
//!   macros are methods of the same names, and the data buffer stays a raw `dat` pointer
//!   with [`Fusebuf::fb_dat_slice`] (`unsafe`: one side at a time touches the bytes).
//! - `FUSE_NAME_OFFSET`, `FUSE_DIRENT_ALIGN` and `FUSE_DIRENT_SIZE` are a constant and
//!   `const fn`s; `struct fuse_dirent`'s flexible `name[]` is `name: [u8; 0]`, the name
//!   being read from the buffer past [`FUSE_NAME_OFFSET`].
//! - The `!_KERNEL` half of the header (libfuse's `struct fusebuf`, its `fb_*` macros and
//!   `fb_dat(in)`) is userland's view of the bytes `fuseread` produces and is not ported;
//!   the host tests check that `fuseread` produces exactly that layout.
//! - The prototypes `fb_setup`, `fb_queue` and `fb_delete` are the functions of
//!   `miscfs/fuse/fusebuf.rs`.

use core::cell::Cell;
use core::mem::offset_of;
use core::ptr;

use crate::machine::copy::AbiPod;
use crate::queue_adapter;
use crate::sys::queue::SimpleqEntry;
use crate::sys::types::Ino;

/// `FUSEBUFMAXSIZE`: maximum size of the read or write buffer sent from the kernel for VFS
/// syscalls: read, readdir, readlink, write.
pub const FUSEBUFMAXSIZE: usize = 4096 * 1024;

/// `FUSE_KERNEL_VERSION`: Linux FUSE kernel interface major version we somewhat emulate.
pub const FUSE_KERNEL_VERSION: u32 = 7;

/// `FUSE_KERNEL_MINOR_VERSION`: Linux FUSE kernel interface minor version we somewhat
/// emulate.
pub const FUSE_KERNEL_MINOR_VERSION: u32 = 19;

/// `FUSE_ROOT_ID`: the root inode is the root of the mounted FUSE file system. Also note
/// that inode 0 can't be used for normal purposes.
pub const FUSE_ROOT_ID: Ino = 1;

/// `FUSE_FATTR_MODE`: flag needed by setattr.
pub const FUSE_FATTR_MODE: u32 = 1 << 0;
/// `FUSE_FATTR_UID`.
pub const FUSE_FATTR_UID: u32 = 1 << 1;
/// `FUSE_FATTR_GID`.
pub const FUSE_FATTR_GID: u32 = 1 << 2;
/// `FUSE_FATTR_SIZE`.
pub const FUSE_FATTR_SIZE: u32 = 1 << 3;
/// `FUSE_FATTR_ATIME`.
pub const FUSE_FATTR_ATIME: u32 = 1 << 4;
/// `FUSE_FATTR_MTIME`.
pub const FUSE_FATTR_MTIME: u32 = 1 << 5;

/// `FUSE_LOOKUP`: fusebuf type.
pub const FUSE_LOOKUP: u32 = 1;
/// `FUSE_GETATTR`.
pub const FUSE_GETATTR: u32 = 3;
/// `FUSE_SETATTR`.
pub const FUSE_SETATTR: u32 = 4;
/// `FUSE_READLINK`.
pub const FUSE_READLINK: u32 = 5;
/// `FUSE_SYMLINK`.
pub const FUSE_SYMLINK: u32 = 6;
/// `FUSE_MKNOD`.
pub const FUSE_MKNOD: u32 = 8;
/// `FUSE_MKDIR`.
pub const FUSE_MKDIR: u32 = 9;
/// `FUSE_UNLINK`.
pub const FUSE_UNLINK: u32 = 10;
/// `FUSE_RMDIR`.
pub const FUSE_RMDIR: u32 = 11;
/// `FUSE_RENAME`.
pub const FUSE_RENAME: u32 = 12;
/// `FUSE_LINK`.
pub const FUSE_LINK: u32 = 13;
/// `FUSE_OPEN`.
pub const FUSE_OPEN: u32 = 14;
/// `FUSE_READ`.
pub const FUSE_READ: u32 = 15;
/// `FUSE_WRITE`.
pub const FUSE_WRITE: u32 = 16;
/// `FUSE_STATFS`.
pub const FUSE_STATFS: u32 = 17;
/// `FUSE_RELEASE`.
pub const FUSE_RELEASE: u32 = 18;
/// `FUSE_FSYNC`.
pub const FUSE_FSYNC: u32 = 20;
/// `FUSE_FLUSH`.
pub const FUSE_FLUSH: u32 = 25;
/// `FUSE_INIT`.
pub const FUSE_INIT: u32 = 26;
/// `FUSE_OPENDIR`.
pub const FUSE_OPENDIR: u32 = 27;
/// `FUSE_READDIR`.
pub const FUSE_READDIR: u32 = 28;
/// `FUSE_RELEASEDIR`.
pub const FUSE_RELEASEDIR: u32 = 29;
/// `FUSE_DESTROY`.
pub const FUSE_DESTROY: u32 = 38;
/// `FUSE_FORGET`.
pub const FUSE_FORGET: u32 = 2;

/// Implements [`AbiPod`] for protocol structures.
macro_rules! fuse_abi {
    ($($t:ident),+ $(,)?) => {
        $(
            // SAFETY: `#[repr(C)]` integers (and arrays and structures of them) whose holes
            // are named members: no implicit padding (the sizes are pinned at the end of the
            // file), and any bit pattern is a valid value.
            unsafe impl AbiPod for $t {}
        )+
    };
}

/// `struct fuse_attr`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FuseAttr {
    /// `ino`.
    pub ino: u64,
    /// `size`.
    pub size: u64,
    /// `blocks`.
    pub blocks: u64,
    /// `atime`.
    pub atime: u64,
    /// `mtime`.
    pub mtime: u64,
    /// `ctime`.
    pub ctime: u64,
    /// `atimensec`.
    pub atimensec: u32,
    /// `mtimensec`.
    pub mtimensec: u32,
    /// `ctimensec`.
    pub ctimensec: u32,
    /// `mode`.
    pub mode: u32,
    /// `nlink`.
    pub nlink: u32,
    /// `uid`.
    pub uid: u32,
    /// `gid`.
    pub gid: u32,
    /// `rdev`.
    pub rdev: u32,
    /// `blksize`.
    pub blksize: u32,
    /// `padding`.
    pub padding: u32,
}

/// `struct fuse_entry_out`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FuseEntryOut {
    /// `nodeid`: inode number.
    pub nodeid: u64,
    /// `generation`: not implemented.
    pub generation: u64,
    /// `entry_valid`: not implemented.
    pub entry_valid: u64,
    /// `attr_valid`: not implemented.
    pub attr_valid: u64,
    /// `entry_valid_nsec`: not implemented.
    pub entry_valid_nsec: u32,
    /// `attr_valid_nsec`: not implemented.
    pub attr_valid_nsec: u32,
    /// `attr`.
    pub attr: FuseAttr,
}

/// `struct fuse_forget_in`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FuseForgetIn {
    /// `nlookup`.
    pub nlookup: u64,
}

/// `struct fuse_getattr_in`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FuseGetattrIn {
    /// `getattr_flags`: not implemented.
    pub getattr_flags: u32,
    /// `dummy`.
    pub dummy: u32,
    /// `fh`: not implemented.
    pub fh: u64,
}

/// `struct fuse_attr_out`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FuseAttrOut {
    /// `attr_valid`: not implemented.
    pub attr_valid: u64,
    /// `attr_valid_nsec`: not implemented.
    pub attr_valid_nsec: u32,
    /// `dummy`.
    pub dummy: u32,
    /// `attr`.
    pub attr: FuseAttr,
}

/// `struct fuse_mknod_in`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FuseMknodIn {
    /// `mode`.
    pub mode: u32,
    /// `rdev`.
    pub rdev: u32,
    /// `umask`.
    pub umask: u32,
    /// `padding`.
    pub padding: u32,
}

/// `struct fuse_mkdir_in`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FuseMkdirIn {
    /// `mode`.
    pub mode: u32,
    /// `umask`.
    pub umask: u32,
}

/// `struct fuse_rename_in`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FuseRenameIn {
    /// `newdir`.
    pub newdir: u64,
}

/// `struct fuse_link_in`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FuseLinkIn {
    /// `oldnodeid`.
    pub oldnodeid: u64,
}

/// `struct fuse_setattr_in`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FuseSetattrIn {
    /// `valid`: the `FUSE_FATTR_*` set.
    pub valid: u32,
    /// `padding`.
    pub padding: u32,
    /// `fh`.
    pub fh: u64,
    /// `size`.
    pub size: u64,
    /// `lock_owner`.
    pub lock_owner: u64,
    /// `atime`.
    pub atime: u64,
    /// `mtime`.
    pub mtime: u64,
    /// `unused2`.
    pub unused2: u64,
    /// `atimensec`.
    pub atimensec: u32,
    /// `mtimensec`.
    pub mtimensec: u32,
    /// `unused3`.
    pub unused3: u32,
    /// `mode`.
    pub mode: u32,
    /// `unused4`.
    pub unused4: u32,
    /// `uid`.
    pub uid: u32,
    /// `gid`.
    pub gid: u32,
    /// `unused5`.
    pub unused5: u32,
}

/// `struct fuse_open_in`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FuseOpenIn {
    /// `flags`.
    pub flags: u32,
    /// `unused`.
    pub unused: u32,
}

/// `struct fuse_open_out`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FuseOpenOut {
    /// `fh`.
    pub fh: u64,
    /// `open_flags`.
    pub open_flags: u32,
    /// `padding`.
    pub padding: u32,
}

/// `struct fuse_release_in`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FuseReleaseIn {
    /// `fh`.
    pub fh: u64,
    /// `flags`.
    pub flags: u32,
    /// `release_flags`.
    pub release_flags: u32,
    /// `lock_owner`.
    pub lock_owner: u64,
}

/// `struct fuse_flush_in`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FuseFlushIn {
    /// `fh`.
    pub fh: u64,
    /// `unused`.
    pub unused: u32,
    /// `padding`.
    pub padding: u32,
    /// `lock_owner`.
    pub lock_owner: u64,
}

/// `struct fuse_read_in`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FuseReadIn {
    /// `fh`.
    pub fh: u64,
    /// `offset`.
    pub offset: u64,
    /// `size`.
    pub size: u32,
    /// `read_flags`.
    pub read_flags: u32,
    /// `lock_owner`.
    pub lock_owner: u64,
    /// `flags`.
    pub flags: u32,
    /// `padding`.
    pub padding: u32,
}

/// `struct fuse_write_in`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FuseWriteIn {
    /// `fh`.
    pub fh: u64,
    /// `offset`.
    pub offset: u64,
    /// `size`.
    pub size: u32,
    /// `write_flags`.
    pub write_flags: u32,
    /// `lock_owner`.
    pub lock_owner: u64,
    /// `flags`.
    pub flags: u32,
    /// `padding`.
    pub padding: u32,
}

/// `struct fuse_write_out`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FuseWriteOut {
    /// `size`.
    pub size: u32,
    /// `padding`.
    pub padding: u32,
}

/// `struct fuse_kstatfs`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FuseKstatfs {
    /// `blocks`.
    pub blocks: u64,
    /// `bfree`.
    pub bfree: u64,
    /// `bavail`.
    pub bavail: u64,
    /// `files`.
    pub files: u64,
    /// `ffree`.
    pub ffree: u64,
    /// `bsize`.
    pub bsize: u32,
    /// `namelen`.
    pub namelen: u32,
    /// `frsize`.
    pub frsize: u32,
    /// `padding`.
    pub padding: u32,
    /// `spare`.
    pub spare: [u32; 6],
}

/// `struct fuse_statfs_out`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FuseStatfsOut {
    /// `st`.
    pub st: FuseKstatfs,
}

/// `struct fuse_fsync_in`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FuseFsyncIn {
    /// `fh`.
    pub fh: u64,
    /// `fsync_flags` (the C marks it XXX).
    pub fsync_flags: u32,
    /// `padding`.
    pub padding: u32,
}

/// `struct fuse_init_in`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FuseInitIn {
    /// `major`.
    pub major: u32,
    /// `minor`.
    pub minor: u32,
    /// `max_readahead`.
    pub max_readahead: u32,
    /// `flags`.
    pub flags: u32,
}

/// `struct fuse_init_out`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FuseInitOut {
    /// `major`.
    pub major: u32,
    /// `minor`.
    pub minor: u32,
    /// `max_readahead`: not implemented.
    pub max_readahead: u32,
    /// `flags`: not implemented.
    pub flags: u32,
    /// `max_background`: not implemented.
    pub max_background: u16,
    /// `congestion_threshold`: not implemented.
    pub congestion_threshold: u16,
    /// `max_write`.
    pub max_write: u32,
}

/// `struct fuse_in_header`: the header of every request the daemon reads.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FuseInHeader {
    /// `len`: the request's length, header included.
    pub len: u32,
    /// `opcode`: the `FUSE_*` operation.
    pub opcode: u32,
    /// `unique`: the request's ID, which the reply carries back.
    pub unique: u64,
    /// `nodeid`: the inode the operation is about.
    pub nodeid: u64,
    /// `uid`.
    pub uid: u32,
    /// `gid`.
    pub gid: u32,
    /// `pid`: the thread ID of the requester.
    pub pid: u32,
    /// `padding`.
    pub padding: u32,
}

/// `struct fuse_out_header`: the header of every reply the daemon writes.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FuseOutHeader {
    /// `len`: the reply's length, header included.
    pub len: u32,
    /// `error`: the negated errno, 0 for success.
    pub error: i32,
    /// `unique`: the ID of the request answered; 0 for a notification.
    pub unique: u64,
}

/// `struct fuse_dirent`: one directory entry of a `FUSE_READDIR` reply; the unterminated
/// name follows at [`FUSE_NAME_OFFSET`].
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FuseDirent {
    /// `ino`.
    pub ino: u64,
    /// `off`.
    pub off: u64,
    /// `namelen`.
    pub namelen: u32,
    /// `type`.
    pub r#type: u32,
    /// `name[]`: unterminated string.
    pub name: [u8; 0],
}

impl FuseDirent {
    /// The entry's fixed part at the start of `buf` (`(struct fuse_dirent *)p`), `None` when
    /// `buf` is shorter than [`FUSE_NAME_OFFSET`].
    pub fn from_bytes(buf: &[u8]) -> Option<FuseDirent> {
        if buf.len() < FUSE_NAME_OFFSET {
            return None;
        }
        // SAFETY: `buf` holds `FUSE_NAME_OFFSET` == `size_of::<FuseDirent>()` readable bytes
        // (checked; the compile-time checks pin the size), and the structure is integers,
        // valid for any bit pattern; the read is unaligned.
        Some(unsafe { ptr::read_unaligned(buf.as_ptr().cast::<FuseDirent>()) })
    }
}

fuse_abi!(
    FuseAttr,
    FuseEntryOut,
    FuseForgetIn,
    FuseGetattrIn,
    FuseAttrOut,
    FuseMknodIn,
    FuseMkdirIn,
    FuseRenameIn,
    FuseLinkIn,
    FuseSetattrIn,
    FuseOpenIn,
    FuseOpenOut,
    FuseReleaseIn,
    FuseFlushIn,
    FuseReadIn,
    FuseWriteIn,
    FuseWriteOut,
    FuseKstatfs,
    FuseStatfsOut,
    FuseFsyncIn,
    FuseInitIn,
    FuseInitOut,
    FuseInHeader,
    FuseOutHeader,
    FuseDirent,
);

/// `FUSE_NAME_OFFSET`: `offsetof(struct fuse_dirent, name)`.
pub const FUSE_NAME_OFFSET: usize = offset_of!(FuseDirent, name);

/// `FUSE_DIRENT_ALIGN(x)`: `x` rounded up to a multiple of 8.
pub const fn fuse_dirent_align(x: usize) -> usize {
    (x + size_of::<u64>() - 1) & !(size_of::<u64>() - 1)
}

/// `FUSE_DIRENT_SIZE(d)`: the record length of a FUSE dirent.
pub const fn fuse_dirent_size(d: &FuseDirent) -> usize {
    fuse_dirent_align(FUSE_NAME_OFFSET + d.namelen as usize)
}

/// The size of `struct fusebuf`'s `op` union: its largest member (`struct fuse_entry_out`).
pub const FUSEBUF_OP_SIZE: usize = {
    let mut n = 0;
    let sizes = [
        size_of::<FuseForgetIn>(),
        size_of::<FuseGetattrIn>(),
        size_of::<FuseSetattrIn>(),
        size_of::<FuseMknodIn>(),
        size_of::<FuseMkdirIn>(),
        size_of::<FuseRenameIn>(),
        size_of::<FuseLinkIn>(),
        size_of::<FuseOpenIn>(),
        size_of::<FuseReadIn>(),
        size_of::<FuseWriteIn>(),
        size_of::<FuseReleaseIn>(),
        size_of::<FuseFsyncIn>(),
        size_of::<FuseFlushIn>(),
        size_of::<FuseInitIn>(),
        size_of::<FuseEntryOut>(),
        size_of::<FuseAttrOut>(),
        size_of::<FuseOpenOut>(),
        size_of::<FuseWriteOut>(),
        size_of::<FuseStatfsOut>(),
        size_of::<FuseInitOut>(),
    ];
    let mut i = 0;
    while i < sizes.len() {
        if sizes[i] > n {
            n = sizes[i];
        }
        i += 1;
    }
    n
};

/// `struct fusebuf`'s `op`: the union of the operations' input structures (`in`) and output
/// structures (`out`), which overlap, as the bytes of the largest member.
#[repr(C, align(8))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FusebufOp {
    /// The union's bytes.
    pub bytes: [u8; FUSEBUF_OP_SIZE],
}

impl FusebufOp {
    /// A zeroed union.
    pub const fn new() -> Self {
        Self {
            bytes: [0; FUSEBUF_OP_SIZE],
        }
    }

    /// Reads the member `T` (`fbuf->op.in.read`, `fbuf->op.out.entry`, ...).
    pub fn get<T: FusebufOpMember>(&self) -> T {
        const { assert!(size_of::<T>() <= FUSEBUF_OP_SIZE) };
        // SAFETY: `T` is no larger than the area (checked at compile time) and, being
        // `AbiPod`, valid for any bytes; the read is unaligned.
        unsafe { ptr::read_unaligned(self.bytes.as_ptr().cast::<T>()) }
    }

    /// Writes the member `T`; the bytes past it keep their value, as the C's stores into one
    /// member of the union do.
    pub fn set<T: FusebufOpMember>(&mut self, v: &T) {
        self.bytes[..size_of::<T>()].copy_from_slice(abi_bytes(v));
    }
}

impl Default for FusebufOp {
    fn default() -> Self {
        Self::new()
    }
}

/// `struct fusebuf` (`_KERNEL`): a request issued by the kernel through fuse(4) and, once the
/// daemon answered it with the same ID (`fb_uuid`), its reply.
///
/// The requesting thread fills it and sleeps on it (`fb_queue`); the device moves it from
/// its input queue to its wait queue when the daemon reads it and off the wait queue when
/// the reply is written, then wakes the thread. The members are `Cell`s: both sides reach it
/// through shared pointers, one at a time (under the kernel lock; the device's queues under
/// its `fd_lock`).
pub struct Fusebuf {
    /// `next`: next buffer in chain.
    pub next: SimpleqEntry<Fusebuf>,
    /// `hdr`: the request's header.
    pub hdr: Cell<FuseInHeader>,
    /// `error`: error returned by daemon.
    pub error: Cell<i32>,
    /// `op_in_len`: size of input.
    pub op_in_len: Cell<usize>,
    /// `op_out_len`: size of output.
    pub op_out_len: Cell<usize>,
    /// `op_out_buf`: whether to expect data.
    pub op_out_buf: Cell<u8>,
    /// `op`: the operation's input and output structures.
    pub op: Cell<FusebufOp>,
    /// `dat_len`: the length of `dat`.
    pub dat_len: Cell<u64>,
    /// `dat`: the data, `malloc(M_FUSEFS)`ed; NULL when there is none.
    pub dat: Cell<*mut u8>,
}

// SAFETY: changed under the kernel lock by the requesting thread or the device (one CPU),
// as in C; the device's queues are protected by its `fd_lock`.
unsafe impl Sync for Fusebuf {}

queue_adapter!(
    /// `fb_next`: the link of a fusebuf in a device's queues (`struct fusebuf_head`).
    pub FbNext: Fusebuf, next => SimpleqEntry<Fusebuf>
);

impl Fusebuf {
    /// A zeroed fusebuf, as `pool_get(PR_ZERO)` returns it.
    pub const fn new() -> Self {
        Self {
            next: SimpleqEntry::new(),
            hdr: Cell::new(FuseInHeader {
                len: 0,
                opcode: 0,
                unique: 0,
                nodeid: 0,
                uid: 0,
                gid: 0,
                pid: 0,
                padding: 0,
            }),
            error: Cell::new(0),
            op_in_len: Cell::new(0),
            op_out_len: Cell::new(0),
            op_out_buf: Cell::new(0),
            op: Cell::new(FusebufOp::new()),
            dat_len: Cell::new(0),
            dat: Cell::new(ptr::null_mut()),
        }
    }

    /// Changes the header in place.
    pub fn update_hdr(&self, f: impl FnOnce(&mut FuseInHeader)) {
        let mut h = self.hdr.get();
        f(&mut h);
        self.hdr.set(h);
    }

    /// `fb_type` (`hdr.opcode`).
    pub fn fb_type(&self) -> u32 {
        self.hdr.get().opcode
    }

    /// `fb_uuid` (`hdr.unique`).
    pub fn fb_uuid(&self) -> u64 {
        self.hdr.get().unique
    }

    /// `fb_ino` (`hdr.nodeid`).
    pub fn fb_ino(&self) -> u64 {
        self.hdr.get().nodeid
    }

    /// `fb_tid` (`hdr.pid`).
    pub fn fb_tid(&self) -> u32 {
        self.hdr.get().pid
    }

    /// `fb_uid` (`hdr.uid`).
    pub fn fb_uid(&self) -> u32 {
        self.hdr.get().uid
    }

    /// `fb_gid` (`hdr.gid`).
    pub fn fb_gid(&self) -> u32 {
        self.hdr.get().gid
    }

    /// `fb_err` (`error`).
    pub fn fb_err(&self) -> i32 {
        self.error.get()
    }

    /// `fb_err = e`.
    pub fn set_fb_err(&self, e: i32) {
        self.error.set(e);
    }

    /// `fb_len` (`dat_len`).
    pub fn fb_len(&self) -> u64 {
        self.dat_len.get()
    }

    /// `fb_len = len`.
    pub fn set_fb_len(&self, len: u64) {
        self.dat_len.set(len);
    }

    /// `fb_dat` (`dat`): the data buffer, NULL when there is none.
    pub fn fb_dat(&self) -> *mut u8 {
        self.dat.get()
    }

    /// `fb_dat = p`.
    pub fn set_fb_dat(&self, p: *mut u8) {
        self.dat.set(p);
    }

    /// The `fb_len` bytes at `fb_dat` (empty when there is no buffer).
    ///
    /// # Safety
    ///
    /// `fb_dat` is NULL or the buffer of `fb_len` bytes allocated with it (`fb_setup`,
    /// `fusewrite`), and the caller is the one side that touches it now: the requesting
    /// thread before `fb_queue` and after it returns, or the device while the fusebuf is on
    /// its queues. No other slice of the buffer is live while this one is.
    #[allow(clippy::mut_from_ref)] // the C's shared pointer; the contract is the caller's
    pub unsafe fn fb_dat_slice(&self) -> &mut [u8] {
        let p = self.dat.get();
        if p.is_null() {
            return &mut [];
        }
        // SAFETY: the caller's contract: `p` is a live buffer of `dat_len` bytes, used by
        // this side alone while the slice lives.
        unsafe { core::slice::from_raw_parts_mut(p, self.dat_len.get() as usize) }
    }

    /// Reads the `op` union's member `T` (`fbuf->op.out.open`, ...).
    pub fn op_get<T: FusebufOpMember>(&self) -> T {
        self.op.get().get()
    }

    /// Writes the `op` union's member `T` (`fbuf->op.in.open = ...`).
    pub fn op_set<T: FusebufOpMember>(&self, v: &T) {
        let mut op = self.op.get();
        op.set(v);
        self.op.set(op);
    }
}

impl Default for Fusebuf {
    fn default() -> Self {
        Self::new()
    }
}

/// A member of `struct fusebuf`'s `op` union (`op.in.*` or `op.out.*`).
pub trait FusebufOpMember: AbiPod {}

macro_rules! fusebuf_op_members {
    ($($t:ident),+ $(,)?) => {
        $( impl FusebufOpMember for $t {} )+
    };
}

fusebuf_op_members!(
    FuseForgetIn,
    FuseGetattrIn,
    FuseSetattrIn,
    FuseMknodIn,
    FuseMkdirIn,
    FuseRenameIn,
    FuseLinkIn,
    FuseOpenIn,
    FuseReadIn,
    FuseWriteIn,
    FuseReleaseIn,
    FuseFsyncIn,
    FuseFlushIn,
    FuseInitIn,
    FuseEntryOut,
    FuseAttrOut,
    FuseOpenOut,
    FuseWriteOut,
    FuseStatfsOut,
    FuseInitOut,
);

/// The bytes of a protocol structure, as `uiomove(&hdr, sizeof(hdr), uio)` copies them.
pub fn abi_bytes<T: AbiPod>(v: &T) -> &[u8] {
    // SAFETY: `T: AbiPod` has no padding, so all `size_of::<T>()` bytes behind the reference
    // are initialised and may be read as `u8`s while `v` is borrowed.
    unsafe { core::slice::from_raw_parts(ptr::from_ref(v).cast::<u8>(), size_of::<T>()) }
}

/// The bytes of a protocol structure, writable, as `uiomove(&hdr, sizeof(hdr), uio)` fills
/// them.
pub fn abi_bytes_mut<T: AbiPod>(v: &mut T) -> &mut [u8] {
    // SAFETY: `T: AbiPod`: every byte is initialised and any bytes written make a valid `T`;
    // the slice covers exactly the object and borrows it mutably.
    unsafe { core::slice::from_raw_parts_mut(ptr::from_mut(v).cast::<u8>(), size_of::<T>()) }
}

const _: () = {
    assert!(size_of::<FuseAttr>() == 88);
    assert!(size_of::<FuseEntryOut>() == 128);
    assert!(offset_of!(FuseEntryOut, attr) == 40);
    assert!(size_of::<FuseForgetIn>() == 8);
    assert!(size_of::<FuseGetattrIn>() == 16);
    assert!(size_of::<FuseAttrOut>() == 104);
    assert!(offset_of!(FuseAttrOut, attr) == 16);
    assert!(size_of::<FuseMknodIn>() == 16);
    assert!(size_of::<FuseMkdirIn>() == 8);
    assert!(size_of::<FuseRenameIn>() == 8);
    assert!(size_of::<FuseLinkIn>() == 8);
    assert!(size_of::<FuseSetattrIn>() == 88);
    assert!(offset_of!(FuseSetattrIn, atimensec) == 56);
    assert!(offset_of!(FuseSetattrIn, mode) == 68);
    assert!(offset_of!(FuseSetattrIn, uid) == 76);
    assert!(size_of::<FuseOpenIn>() == 8);
    assert!(size_of::<FuseOpenOut>() == 16);
    assert!(size_of::<FuseReleaseIn>() == 24);
    assert!(size_of::<FuseFlushIn>() == 24);
    assert!(size_of::<FuseReadIn>() == 40);
    assert!(size_of::<FuseWriteIn>() == 40);
    assert!(size_of::<FuseWriteOut>() == 8);
    assert!(size_of::<FuseKstatfs>() == 80);
    assert!(size_of::<FuseStatfsOut>() == 80);
    assert!(size_of::<FuseFsyncIn>() == 16);
    assert!(size_of::<FuseInitIn>() == 16);
    assert!(size_of::<FuseInitOut>() == 24);
    assert!(offset_of!(FuseInitOut, max_write) == 20);
    assert!(size_of::<FuseInHeader>() == 40);
    assert!(offset_of!(FuseInHeader, unique) == 8);
    assert!(offset_of!(FuseInHeader, nodeid) == 16);
    assert!(offset_of!(FuseInHeader, pid) == 32);
    assert!(size_of::<FuseOutHeader>() == 16);
    assert!(size_of::<FuseDirent>() == 24);
    assert!(FUSE_NAME_OFFSET == 24);
    assert!(FUSEBUF_OP_SIZE == 128);
    assert!(align_of::<FusebufOp>() == 8);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for `<sys/fusebuf.h>`: the protocol structures' layout (the bytes libfuse
    // reads and writes), the `op` union's overlapping members, the dirent helpers, and the
    // constants against the C header (`just test-ref`).

    use std::{assert, assert_eq};

    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/sys/fusebuf.h");
        for (name, value) in [
            ("FUSEBUFMAXSIZE", FUSEBUFMAXSIZE as i64),
            ("FUSE_KERNEL_VERSION", i64::from(FUSE_KERNEL_VERSION)),
            (
                "FUSE_KERNEL_MINOR_VERSION",
                i64::from(FUSE_KERNEL_MINOR_VERSION),
            ),
            ("FUSE_FATTR_MODE", i64::from(FUSE_FATTR_MODE)),
            ("FUSE_FATTR_UID", i64::from(FUSE_FATTR_UID)),
            ("FUSE_FATTR_GID", i64::from(FUSE_FATTR_GID)),
            ("FUSE_FATTR_SIZE", i64::from(FUSE_FATTR_SIZE)),
            ("FUSE_FATTR_ATIME", i64::from(FUSE_FATTR_ATIME)),
            ("FUSE_FATTR_MTIME", i64::from(FUSE_FATTR_MTIME)),
            ("FUSE_LOOKUP", i64::from(FUSE_LOOKUP)),
            ("FUSE_GETATTR", i64::from(FUSE_GETATTR)),
            ("FUSE_SETATTR", i64::from(FUSE_SETATTR)),
            ("FUSE_READLINK", i64::from(FUSE_READLINK)),
            ("FUSE_SYMLINK", i64::from(FUSE_SYMLINK)),
            ("FUSE_MKNOD", i64::from(FUSE_MKNOD)),
            ("FUSE_MKDIR", i64::from(FUSE_MKDIR)),
            ("FUSE_UNLINK", i64::from(FUSE_UNLINK)),
            ("FUSE_RMDIR", i64::from(FUSE_RMDIR)),
            ("FUSE_RENAME", i64::from(FUSE_RENAME)),
            ("FUSE_LINK", i64::from(FUSE_LINK)),
            ("FUSE_OPEN", i64::from(FUSE_OPEN)),
            ("FUSE_READ", i64::from(FUSE_READ)),
            ("FUSE_WRITE", i64::from(FUSE_WRITE)),
            ("FUSE_STATFS", i64::from(FUSE_STATFS)),
            ("FUSE_RELEASE", i64::from(FUSE_RELEASE)),
            ("FUSE_FSYNC", i64::from(FUSE_FSYNC)),
            ("FUSE_FLUSH", i64::from(FUSE_FLUSH)),
            ("FUSE_INIT", i64::from(FUSE_INIT)),
            ("FUSE_OPENDIR", i64::from(FUSE_OPENDIR)),
            ("FUSE_READDIR", i64::from(FUSE_READDIR)),
            ("FUSE_RELEASEDIR", i64::from(FUSE_RELEASEDIR)),
            ("FUSE_DESTROY", i64::from(FUSE_DESTROY)),
            ("FUSE_FORGET", i64::from(FUSE_FORGET)),
        ] {
            assert_eq!(crate::reftest::int(&defs, name), Some(value), "{name}");
        }
    }

    #[test]
    fn header_bytes_are_the_c_layout() {
        let h = FuseInHeader {
            len: 0x0102_0304,
            opcode: FUSE_LOOKUP,
            unique: 0x1122_3344_5566_7788,
            nodeid: 9,
            uid: 1000,
            gid: 1001,
            pid: 100_005,
            padding: 0,
        };
        let b = abi_bytes(&h);
        assert_eq!(b.len(), 40);
        assert_eq!(&b[0..4], &0x0102_0304u32.to_ne_bytes());
        assert_eq!(&b[4..8], &FUSE_LOOKUP.to_ne_bytes());
        assert_eq!(&b[8..16], &0x1122_3344_5566_7788u64.to_ne_bytes());
        assert_eq!(&b[16..24], &9u64.to_ne_bytes());
        assert_eq!(&b[24..28], &1000u32.to_ne_bytes());
        assert_eq!(&b[28..32], &1001u32.to_ne_bytes());
        assert_eq!(&b[32..36], &100_005u32.to_ne_bytes());

        let mut o = FuseOutHeader::default();
        abi_bytes_mut(&mut o)[4..8].copy_from_slice(&(-2i32).to_ne_bytes());
        assert_eq!(o.error, -2);
    }

    #[test]
    fn op_members_overlap_like_the_c_union() {
        let fb = Fusebuf::new();
        fb.op_set(&FuseOpenIn {
            flags: 0x0202,
            unused: 0,
        });
        // The input occupies the first bytes of the union; the reply overwrites the same bytes.
        assert_eq!(fb.op_get::<FuseOpenIn>().flags, 0x0202);
        assert_eq!(&fb.op.get().bytes[0..4], &0x0202u32.to_ne_bytes());
        fb.op_set(&FuseOpenOut {
            fh: 77,
            open_flags: 1,
            padding: 0,
        });
        assert_eq!(fb.op_get::<FuseOpenOut>().fh, 77);
        assert_eq!(fb.op_get::<FuseOpenIn>().flags, 77);

        // Writing a small member leaves the bytes past it alone.
        let mut op = FusebufOp::new();
        op.bytes[8] = 0xaa;
        op.set(&FuseMkdirIn { mode: 1, umask: 2 });
        assert_eq!(op.bytes[8], 0xaa);
        assert_eq!(op.get::<FuseMkdirIn>(), FuseMkdirIn { mode: 1, umask: 2 });
    }

    #[test]
    fn fusebuf_aliases_read_the_header() {
        let fb = Fusebuf::new();
        fb.update_hdr(|h| {
            h.opcode = FUSE_READ;
            h.unique = 42;
            h.nodeid = FUSE_ROOT_ID;
            h.uid = 3;
            h.gid = 4;
            h.pid = 5;
        });
        fb.set_fb_err(2);
        fb.set_fb_len(16);
        assert_eq!(fb.fb_type(), FUSE_READ);
        assert_eq!(fb.fb_uuid(), 42);
        assert_eq!(fb.fb_ino(), 1);
        assert_eq!((fb.fb_uid(), fb.fb_gid(), fb.fb_tid()), (3, 4, 5));
        assert_eq!(fb.fb_err(), 2);
        assert_eq!(fb.fb_len(), 16);
        assert!(fb.fb_dat().is_null());
        // SAFETY: no buffer: the slice is empty.
        assert!(unsafe { fb.fb_dat_slice() }.is_empty());
    }

    #[test]
    fn dirent_size_rounds_to_eight() {
        assert_eq!(fuse_dirent_align(0), 0);
        assert_eq!(fuse_dirent_align(1), 8);
        assert_eq!(fuse_dirent_align(8), 8);
        assert_eq!(fuse_dirent_align(25), 32);
        let mut d = FuseDirent {
            namelen: 1,
            ..FuseDirent::default()
        };
        assert_eq!(fuse_dirent_size(&d), 32);
        d.namelen = 8;
        assert_eq!(fuse_dirent_size(&d), 32);
        d.namelen = 9;
        assert_eq!(fuse_dirent_size(&d), 40);
    }

    #[test]
    fn dirent_from_bytes_reads_the_fixed_part() {
        let mut b = [0u8; 32];
        b[0..8].copy_from_slice(&7u64.to_ne_bytes());
        b[8..16].copy_from_slice(&3u64.to_ne_bytes());
        b[16..20].copy_from_slice(&5u32.to_ne_bytes());
        b[20..24].copy_from_slice(&4u32.to_ne_bytes());
        b[24..29].copy_from_slice(b"hello");
        let d = FuseDirent::from_bytes(&b).unwrap();
        assert_eq!((d.ino, d.off, d.namelen, d.r#type), (7, 3, 5, 4));
        assert!(FuseDirent::from_bytes(&b[..23]).is_none());
    }
}
/* </TESTS> */
