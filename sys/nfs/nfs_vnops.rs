/*	$OpenBSD: nfs_vnops.c,v 1.215 2026/07/02 03:14:52 jsg Exp $	*/
/*	$NetBSD: nfs_vnops.c,v 1.62.4.1 1996/07/08 20:26:52 jtc Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1989, 1993
 *	The Regents of the University of California.  All rights reserved.
 *
 * This code is derived from software contributed to Berkeley by
 * Rick Macklem at The University of Guelph.
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
 *	@(#)nfs_vnops.c	8.16 (Berkeley) 5/27/95
 */
/* </LICENSES> */

//! vnode op calls for Sun NFS version 2 and 3 (`nfs_vnops.c`): TEMPORARY STUBS.
//!
//! Upstream: sys/nfs/nfs_vnops.c @ 3ce1f3f79392
//!
//! This file holds only what `nfs_bio.rs`, `nfs_node.rs` and `nfs_kq.rs` reach in
//! `nfs_vnops.c`: the RPC helpers they call and the three vnode operation vectors that
//! `nfs_nget` (`getnewvnode`) and `nfs_loadattrcache` install. The M10e vnops agent replaces
//! the whole file with the port of `nfs_vnops.c`, keeping these signatures.
//!
//! ## Deviations
//! - Temporary: every RPC helper below is a visible gap (`unported!`, `ENOSYS`) until
//!   `nfs_vnops.c` is ported.
//! - Temporary: the vectors carry only the operations ported so far (`nfs_write`,
//!   `nfs_kqfilter`, `nfs_inactive`, `nfs_reclaim`); every other slot is NULL
//!   (`EOPNOTSUPP`), where the C has the operations of `nfs_vnops.c`, `spec_vnops.c` and
//!   `fifo_vnops.c`.
//! - `int *` flags (`must_commit`, `end_of_directory`) are `&mut bool`; `nfs_writebp`'s
//!   `int force` is a `bool`; the `iomode` words are the `u32` `NFSV3WRITE_*` constants;
//!   `struct proc *` arguments that may be NULL are `Option<&Proc>`.

use crate::nfs::nfs_bio::nfs_write;
use crate::nfs::nfs_kq::nfs_kqfilter;
use crate::nfs::nfs_node::{nfs_inactive, nfs_reclaim};
use crate::nfs::nfsnode::Sillyrename;
use crate::sys::buf::Buf;
use crate::sys::errno::Errno;
use crate::sys::proc::Proc;
use crate::sys::ucred::Ucred;
use crate::sys::uio::Uio;
use crate::sys::vnode::{Vnode, Vops};
use crate::unported;

/// `nfs_vops`: the vnode operations of an NFS file (temporary: see the module doc).
pub static NFS_VOPS: Vops = Vops {
    vop_write: Some(nfs_write),
    vop_kqfilter: Some(nfs_kqfilter),
    vop_inactive: Some(nfs_inactive),
    vop_reclaim: Some(nfs_reclaim),
    ..Vops::EMPTY
};

/// `nfs_specvops`: the vnode operations of a special device on NFS (temporary).
pub static NFS_SPECVOPS: Vops = Vops {
    vop_inactive: Some(nfs_inactive),
    vop_reclaim: Some(nfs_reclaim),
    ..Vops::EMPTY
};

/// `nfs_fifovops`: the vnode operations of a fifo on NFS (temporary).
pub static NFS_FIFOVOPS: Vops = Vops {
    vop_inactive: Some(nfs_inactive),
    ..Vops::EMPTY
};

/// `nfs_readlinkrpc(vp, uiop, cred)`: read the target of the symbolic link `vp` into
/// `uiop` (temporary stub).
pub fn nfs_readlinkrpc(
    _vp: &'static Vnode,
    _uiop: &mut Uio<'_>,
    _cred: *const Ucred,
) -> Result<(), Errno> {
    Err(unported!("nfs_readlinkrpc (nfs_vnops.c)"))
}

/// `nfs_readrpc(vp, uiop)`: read `uiop.uio_resid` bytes at `uiop.uio_offset` of `vp` from
/// the server (temporary stub).
pub fn nfs_readrpc(_vp: &'static Vnode, _uiop: &mut Uio<'_>) -> Result<(), Errno> {
    Err(unported!("nfs_readrpc (nfs_vnops.c)"))
}

/// `nfs_writerpc(vp, uiop, iomode, must_commit)`: write `uiop` to the server; `iomode` is
/// the `NFSV3WRITE_*` asked for and returns the one the server gave, `must_commit` is set
/// when the server's write verifier changed (temporary stub).
pub fn nfs_writerpc(
    _vp: &'static Vnode,
    _uiop: &mut Uio<'_>,
    _iomode: &mut u32,
    _must_commit: &mut bool,
) -> Result<(), Errno> {
    Err(unported!("nfs_writerpc (nfs_vnops.c)"))
}

/// `nfs_readdirrpc(vp, uiop, cred, end_of_directory)`: read directory entries (temporary
/// stub).
pub fn nfs_readdirrpc(
    _vp: &'static Vnode,
    _uiop: &mut Uio<'_>,
    _cred: *const Ucred,
    _end_of_directory: &mut bool,
) -> Result<(), Errno> {
    Err(unported!("nfs_readdirrpc (nfs_vnops.c)"))
}

/// `nfs_readdirplusrpc(vp, uiop, cred, end_of_directory, p)`: read directory entries with
/// their attributes and handles (NFSv3; temporary stub).
pub fn nfs_readdirplusrpc(
    _vp: &'static Vnode,
    _uiop: &mut Uio<'_>,
    _cred: *const Ucred,
    _end_of_directory: &mut bool,
    _p: Option<&Proc>,
) -> Result<(), Errno> {
    Err(unported!("nfs_readdirplusrpc (nfs_vnops.c)"))
}

/// `nfs_commit(vp, offset, cnt, procp)`: commit `cnt` bytes at `offset` to stable storage
/// (NFSv3; temporary stub).
pub fn nfs_commit(
    _vp: &'static Vnode,
    _offset: u64,
    _cnt: i32,
    _procp: Option<&Proc>,
) -> Result<(), Errno> {
    Err(unported!("nfs_commit (nfs_vnops.c)"))
}

/// `nfs_removeit(sp)`: remove the silly-renamed file of `sp` (temporary stub).
pub fn nfs_removeit(_sp: &Sillyrename) -> Result<(), Errno> {
    Err(unported!("nfs_removeit (nfs_vnops.c)"))
}

/// `nfs_writebp(bp, force)`: start the write of a buffer, asynchronous when `B_ASYNC`, and
/// wait for it otherwise (temporary stub).
pub fn nfs_writebp(_bp: &'static Buf, _force: bool) -> Result<(), Errno> {
    Err(unported!("nfs_writebp (nfs_vnops.c)"))
}
