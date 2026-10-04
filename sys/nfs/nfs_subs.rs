/*	$OpenBSD: nfs_subs.c,v 1.153 2026/09/01 02:33:27 jsg Exp $	*/
/*	$NetBSD: nfs_subs.c,v 1.27.4.3 1996/07/08 20:34:24 jtc Exp $	*/
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
 *	@(#)nfs_subs.c	8.8 (Berkeley) 5/22/95
 */
/* </LICENSES> */

//! `nfs/nfs_subs.c`: the functions that support the `nfsm_subs.h` inline functions and help
//! fiddle mbuf chains for the NFS op functions: they create the RPC header, copy data between
//! mbuf chains and uio lists, load the attribute cache from a reply and map errors.
//!
//! Upstream: sys/nfs/nfs_subs.c @ 3ce1f3f79392
//!
//! Mbufs are `&'static Mbuf` (`docs/C_TO_RUST.md`). A build cursor (the C's `struct mbuf
//! **mb`, the last mbuf of the chain being built) is a `&mut &'static Mbuf`; a dissection
//! cursor is an `Option<&'static Mbuf>` with a raw position in its data, checked by
//! `nfsm_avail` (`nfsm_subs.rs`) before every use. Data comes back as the bounds-checked
//! views of `nfsm_subs.rs` ([`XdrIn`], [`XdrOut`]).
//!
//! ## Deviations
//! - The XDR words `nfs_init` computes at startup (`rpc_call`, `nfs_true`, `nfs_xdrneg1`,
//!   ...) are `const`s computed at compile time, with their C names (lowercase:
//!   `RPC_CALL` is already the `rpcv2.h` constant).
//! - `MGET(M_WAIT)` cannot sleep here (`subr_pool.rs`): where the C cannot fail
//!   (`nfsm_reqhead`, `nfsm_build`, `nfsm_uiotombuf`) an empty pool panics, the
//!   `vfs_subr.rs` precedent; `nfsm_disct`, which returns an error anyway, fails with
//!   `ENOBUFS`.
//! - `nfsm_disct` and `nfs_adv` compute the bytes left in the current mbuf (the C's `left`
//!   argument, always that value) themselves; `nfs_adv` advances from the position when it
//!   is already far enough, a case the C's callers never reach. `nfsm_disct` returns the
//!   view instead of storing a pointer through `cp2`.
//! - [`nfsm_nextbytes`] is a Rust helper for the cursor walks of other files (`nfs_namei`
//!   copies a name byte by byte across mbufs), so that they need not dereference `dpos`.
//! - `nfsm_mbuftouio` ignores `copyout`'s error, and `nfsm_uiotombuf` `uiomove`'s, as the C
//!   does.
//! - `nfsm_buftombuf`/`nfsm_strtombuf` take a byte slice (its length is the C's `len`);
//!   `nfsm_srvfhtom` takes the `nfsfh_t` the server's handle lives in (the C passes its
//!   `fh_generic` member and reads `NFSX_V2FH` bytes from it, past the `fhandle_t`).
//! - `nfs_loadattrcache`: `option FIFO` is in GENERIC but `miscfs/fifofs` is not ported, so a
//!   new fifo node fails with `EOPNOTSUPP`, the C's `!FIFO` path (the tmpfs precedent). A
//!   new device node does not get `nfs_specvops` yet: M10e wires it when `nfs_vnops.rs`
//!   lands (an `unported!("nfs_specvops")` marks the site); the alias check runs.
//! - `nfsstats` is `NFSSTATS` of atomics (`nfs.rs`); `nfs_ticks` an `AtomicI32`.
//! - `nfs_get_xid`'s function-local statics (`nfs_xid_ctx`, `called`) are file statics, the
//!   context a `StaticCell` under a private mutex (`nfs_xid_mtx`, not in C), as
//!   `ip6_randomid` does.
//! - `nfsm_rpchead` builds `RPCAUTH_UNIX` only, as the C does (`KASSERT(auth_type ==
//!   RPCAUTH_UNIX)`); its switches over the type are the one case.
//! - `nfs_init`: under `NFSSERVER` it calls `nfsrv_init(0)` (`nfs_syscalls.c`) and
//!   `nfsrv_initcache()` (`nfs_srvcache.c`); M10e wires them when those files land, until
//!   then each site is an `unported!` gap.
//! - `nfs_vfs_init` and `nfs_getattrcache` return `Result` (`ENOENT` for a cache miss).
//! - `nfs_clearcommit`'s `goto loop` (a vnode found on the list of another mount) restarts
//!   the walk with a labelled `continue`.
//! - `nfsrv_errmap` maps an `err` below 1 to `NFSERR_IO`; the C indexes the version 2 table
//!   at `err - 1` and would read before it (its callers never pass one).
//! - `nfsm_v3attrbuild` takes `full` as a `bool`.

use core::ffi::c_void;
use core::ptr;
use core::slice;
use core::sync::atomic::{AtomicBool, AtomicI32, Ordering};

use crate::conf::param::HZ;
use crate::crypto::idgen::{Idgen32Ctx, idgen32, idgen32_init};
use crate::kassert;
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_subr::uiomove;
use crate::kern::kern_tc::gettime;
#[cfg(feature = "nfsclient")]
use crate::kern::spec_vnops::SPEC_VOPS;
use crate::kern::subr_pool::pool_init;
use crate::kern::subr_prf::panic;
use crate::kern::uipc_mbuf::{m_align, m_get, m_trailingspace};
#[cfg(feature = "nfsclient")]
use crate::kern::vfs_cache::cache_purge;
#[cfg(feature = "nfsclient")]
use crate::kern::vfs_subr::{checkalias, vgone, vrele};
use crate::machine::copy::copyout;
use crate::machine::intr::{IPL_NONE, IPL_SOFTNET, splbio, splx};
#[cfg(feature = "nfsclient")]
use crate::nfs::nfs::NFS_NODE_POOL;
use crate::nfs::nfs::{ND_NFSV3, NFS_TICKINTVL, NfsReq, NfsrvDescript, Nfsstats};
use crate::nfs::nfs_var::{mb_offset, nfsm_padlen};
use crate::nfs::nfsm_subs::nfsm_rndup;
use crate::nfs::nfsm_subs::{XdrIn, XdrOut, nfsm_avail};
#[cfg(feature = "nfsclient")]
use crate::nfs::nfsmount::VFSTONFS;
#[cfg(feature = "nfsclient")]
use crate::nfs::nfsnode::{NACC, NCHG, NMODIFIED, NUPD};
#[cfg(feature = "nfsclient")]
use crate::nfs::nfsnode::{NFS_BUFQ, NFSTOV, NfsNode};
use crate::nfs::nfsnode::{NFS_COMMIT_PUSH_VALID, NFS_COMMIT_PUSHED_VALID, VTONFS};
use crate::nfs::nfsproto::{NFBLK, NFCHR, NFDIR, NFFIFO, NFLNK, NFNON, NFREG, NFSOCK};
#[cfg(feature = "nfsclient")]
use crate::nfs::nfsproto::{NFS_FABLKSIZE, NfsFattr, nfsv2tov_type, nfsv3tov_type, nfsx_fattr};
use crate::nfs::nfsproto::{
    NFS_NPROCS, NFS_PROG, NFSERR_ACCES, NFSERR_BAD_COOKIE, NFSERR_BADHANDLE, NFSERR_BADTYPE,
    NFSERR_DQUOT, NFSERR_EXIST, NFSERR_FBIG, NFSERR_INVAL, NFSERR_IO, NFSERR_ISDIR, NFSERR_MLINK,
    NFSERR_NAMETOL, NFSERR_NODEV, NFSERR_NOENT, NFSERR_NOSPC, NFSERR_NOT_SYNC, NFSERR_NOTDIR,
    NFSERR_NOTEMPTY, NFSERR_NOTSUPP, NFSERR_NXIO, NFSERR_PERM, NFSERR_ROFS, NFSERR_SERVERFAULT,
    NFSERR_STALE, NFSERR_TOOSMALL, NFSERR_XDEV, NFSPROC_CREATE, NFSPROC_FSSTAT, NFSPROC_GETATTR,
    NFSPROC_LINK, NFSPROC_LOOKUP, NFSPROC_MKDIR, NFSPROC_NOOP, NFSPROC_NULL, NFSPROC_READ,
    NFSPROC_READDIR, NFSPROC_READLINK, NFSPROC_REMOVE, NFSPROC_RENAME, NFSPROC_RMDIR,
    NFSPROC_SETATTR, NFSPROC_SYMLINK, NFSPROC_WRITE, NFSV2PROC_CREATE, NFSV2PROC_GETATTR,
    NFSV2PROC_LINK, NFSV2PROC_LOOKUP, NFSV2PROC_MKDIR, NFSV2PROC_NOOP, NFSV2PROC_NULL,
    NFSV2PROC_READ, NFSV2PROC_READDIR, NFSV2PROC_READLINK, NFSV2PROC_REMOVE, NFSV2PROC_RENAME,
    NFSV2PROC_RMDIR, NFSV2PROC_SETATTR, NFSV2PROC_STATFS, NFSV2PROC_SYMLINK, NFSV2PROC_WRITE,
    NFSX_V2FH, NFSX_V3FH, Nfsfh, Nfstype, Nfsv2Time,
};
use crate::nfs::nfsproto::{
    NFS_VER2, NFS_VER3, NFSPROC_COMMIT, NFSV3SATTRTIME_DONTCHANGE, NFSV3SATTRTIME_TOCLIENT,
    NFSV3SATTRTIME_TOSERVER, NFSX_UNSIGNED,
};
use crate::nfs::rpcv2::RPCAUTH_NULL;
use crate::nfs::rpcv2::{
    RPC_AUTHERR, RPC_CALL, RPC_MISMATCH, RPC_MSGACCEPTED, RPC_MSGDENIED, RPC_REPLY, RPC_VER2,
    RPCAUTH_UNIX,
};
#[cfg(feature = "nfsclient")]
use crate::nfs::xdr_subs::{fxdr_hyper, fxdr_nfsv2time, fxdr_nfsv3time};
use crate::nfs::xdr_subs::{fxdr_unsigned, txdr_nfsv3time, txdr_unsigned};
use crate::sys::buf::{B_BUSY, B_DELWRI, B_NEEDCOMMIT, Buf};
use crate::sys::errno::Errno;
use crate::sys::mbuf::{M_WAIT, MHLEN, MLEN, MT_DATA, Mbuf, mclget, mtod};
use crate::sys::mount::Mount;
use crate::sys::mount::NFSMNT_NFSV3;
#[cfg(feature = "nfsclient")]
use crate::sys::mount::Vfsconf;
use crate::sys::mutex::Mutex;
use crate::sys::param::DEV_BSIZE;
#[cfg(feature = "nfsclient")]
use crate::sys::param::{BLKDEV_IOSIZE, MAXBSIZE};
use crate::sys::pool::{PR_WAITOK, Pool};
use crate::sys::time::Timespec;
use crate::sys::types::{Gid, Mode, Off, Uid};
#[cfg(feature = "nfsclient")]
use crate::sys::types::{Time, makedev};
use crate::sys::ucred::Ucred;
use crate::sys::uio::{Iovec, Uio, UioRw, UioSeg};
use crate::sys::vnode::Vattr;
#[cfg(feature = "nfsclient")]
use crate::sys::vnode::iftovt;
use crate::sys::vnode::{VBLK, VCHR, VDIR, VFIFO, VLNK, VNON, VNOVAL, VREG, VSOCK, Vnode, Vtype};
#[cfg(feature = "nfsclient")]
use crate::uvm::uvm_extern::Voff;
#[cfg(feature = "nfsclient")]
use crate::uvm::uvm_vnode::uvm_vnp_setsize;
use libkern::StaticCell;

/// `nfs_xdrneg1`: data items converted to XDR at startup, since they are constant. This is
/// kinda hokey, but may save a little time doing byte swaps.
#[allow(non_upper_case_globals)] // the C name
pub const nfs_xdrneg1: u32 = txdr_unsigned(-1i32 as u32);
/// `rpc_call`.
#[allow(non_upper_case_globals)] // the C name
pub const rpc_call: u32 = txdr_unsigned(RPC_CALL);
/// `rpc_vers`.
#[allow(non_upper_case_globals)] // the C name
pub const rpc_vers: u32 = txdr_unsigned(RPC_VER2);
/// `rpc_reply`.
#[allow(non_upper_case_globals)] // the C name
pub const rpc_reply: u32 = txdr_unsigned(RPC_REPLY);
/// `rpc_msgdenied`.
#[allow(non_upper_case_globals)] // the C name
pub const rpc_msgdenied: u32 = txdr_unsigned(RPC_MSGDENIED);
/// `rpc_autherr`.
#[allow(non_upper_case_globals)] // the C name
pub const rpc_autherr: u32 = txdr_unsigned(RPC_AUTHERR);
/// `rpc_mismatch`.
#[allow(non_upper_case_globals)] // the C name
pub const rpc_mismatch: u32 = txdr_unsigned(RPC_MISMATCH);
/// `rpc_auth_unix`.
#[allow(non_upper_case_globals)] // the C name
pub const rpc_auth_unix: u32 = txdr_unsigned(RPCAUTH_UNIX);
/// `rpc_msgaccepted`.
#[allow(non_upper_case_globals)] // the C name
pub const rpc_msgaccepted: u32 = txdr_unsigned(RPC_MSGACCEPTED);
/// `nfs_prog`.
#[allow(non_upper_case_globals)] // the C name
pub const nfs_prog: u32 = txdr_unsigned(NFS_PROG);
/// `nfs_true`.
#[allow(non_upper_case_globals)] // the C name
pub const nfs_true: u32 = txdr_unsigned(1);
/// `nfs_false`.
#[allow(non_upper_case_globals)] // the C name
pub const nfs_false: u32 = txdr_unsigned(0);

/// `nfsv2_type`: the version 2 file type of each vnode type.
pub const NFSV2_TYPE: [Nfstype; 9] = [
    NFNON, NFREG, NFDIR, NFBLK, NFCHR, NFLNK, NFNON, NFCHR, NFNON,
];
/// `nfsv3_type`: the version 3 file type of each vnode type.
pub const NFSV3_TYPE: [Nfstype; 9] = [
    NFNON, NFREG, NFDIR, NFBLK, NFCHR, NFLNK, NFSOCK, NFFIFO, NFNON,
];
/// `nv2tov_type`: the vnode type of each version 2 file type.
pub const NV2TOV_TYPE: [Vtype; 8] = [VNON, VREG, VDIR, VBLK, VCHR, VLNK, VNON, VNON];
/// `nv3tov_type`: the vnode type of each version 3 file type.
pub const NV3TOV_TYPE: [Vtype; 8] = [VNON, VREG, VDIR, VBLK, VCHR, VLNK, VSOCK, VFIFO];

/// `nfsv3_procid`: mapping of old NFS version 2 RPC numbers to generic numbers.
pub const NFSV3_PROCID: [usize; NFS_NPROCS] = [
    NFSPROC_NULL,
    NFSPROC_GETATTR,
    NFSPROC_SETATTR,
    NFSPROC_NOOP,
    NFSPROC_LOOKUP,
    NFSPROC_READLINK,
    NFSPROC_READ,
    NFSPROC_NOOP,
    NFSPROC_WRITE,
    NFSPROC_CREATE,
    NFSPROC_REMOVE,
    NFSPROC_RENAME,
    NFSPROC_LINK,
    NFSPROC_SYMLINK,
    NFSPROC_MKDIR,
    NFSPROC_RMDIR,
    NFSPROC_READDIR,
    NFSPROC_FSSTAT,
    NFSPROC_NOOP,
    NFSPROC_NOOP,
    NFSPROC_NOOP,
    NFSPROC_NOOP,
    NFSPROC_NOOP,
];

/// `nfsv2_procid`: and the reverse mapping from generic to version 2 procedure numbers.
pub const NFSV2_PROCID: [usize; NFS_NPROCS] = [
    NFSV2PROC_NULL,
    NFSV2PROC_GETATTR,
    NFSV2PROC_SETATTR,
    NFSV2PROC_LOOKUP,
    NFSV2PROC_NOOP,
    NFSV2PROC_READLINK,
    NFSV2PROC_READ,
    NFSV2PROC_WRITE,
    NFSV2PROC_CREATE,
    NFSV2PROC_MKDIR,
    NFSV2PROC_SYMLINK,
    NFSV2PROC_CREATE,
    NFSV2PROC_REMOVE,
    NFSV2PROC_RMDIR,
    NFSV2PROC_RENAME,
    NFSV2PROC_LINK,
    NFSV2PROC_READDIR,
    NFSV2PROC_NOOP,
    NFSV2PROC_STATFS,
    NFSV2PROC_NOOP,
    NFSV2PROC_NOOP,
    NFSV2PROC_NOOP,
    NFSV2PROC_NOOP,
];

/// `nfsrv_v2errmap`: maps errno values to NFS error numbers. Use `NFSERR_IO` as the catch
/// all for ones not specifically defined in RFC 1094. Everything after the table maps to
/// `NFSERR_IO`, so far.
pub const NFSRV_V2ERRMAP: [u8; 70] = {
    const P: u8 = NFSERR_PERM as u8;
    const NOENT: u8 = NFSERR_NOENT as u8;
    const IO: u8 = NFSERR_IO as u8;
    const NXIO: u8 = NFSERR_NXIO as u8;
    const ACCES: u8 = NFSERR_ACCES as u8;
    const EXIST: u8 = NFSERR_EXIST as u8;
    const NODEV: u8 = NFSERR_NODEV as u8;
    const NOTDIR: u8 = NFSERR_NOTDIR as u8;
    const ISDIR: u8 = NFSERR_ISDIR as u8;
    const FBIG: u8 = NFSERR_FBIG as u8;
    const NOSPC: u8 = NFSERR_NOSPC as u8;
    const ROFS: u8 = NFSERR_ROFS as u8;
    const NAMETOL: u8 = NFSERR_NAMETOL as u8;
    const NOTEMPTY: u8 = NFSERR_NOTEMPTY as u8;
    const DQUOT: u8 = NFSERR_DQUOT as u8;
    const STALE: u8 = NFSERR_STALE as u8;
    [
        P, NOENT, IO, IO, IO, NXIO, IO, IO, IO, IO, //
        IO, IO, ACCES, IO, IO, IO, EXIST, IO, NODEV, NOTDIR, //
        ISDIR, IO, IO, IO, IO, IO, FBIG, NOSPC, IO, ROFS, //
        IO, IO, IO, IO, IO, IO, IO, IO, IO, IO, //
        IO, IO, IO, IO, IO, IO, IO, IO, IO, IO, //
        IO, IO, IO, IO, IO, IO, IO, IO, IO, IO, //
        IO, IO, NAMETOL, IO, IO, NOTEMPTY, IO, IO, DQUOT, STALE,
    ]
};

// Maps errno values to NFS error numbers. Although it is not obvious whether or not NFS
// clients really care if a returned error value is in the specified list for the procedure,
// the safest thing to do is filter them appropriately. For version 2, the X/Open XNFS
// document is the only specification that defines error values for each RPC (the RFC simply
// lists all possible error values for all RPCs), so I have decided to not do this for
// version 2. The first entry is the default error return and the rest are the valid errors
// for that RPC in increasing numeric order.

/// `nfsv3err_null`.
const NFSV3ERR_NULL: &[i16] = &[0, 0];

/// `nfsv3err_getattr`.
const NFSV3ERR_GETATTR: &[i16] = &[
    NFSERR_IO as i16,
    NFSERR_IO as i16,
    NFSERR_STALE as i16,
    NFSERR_BADHANDLE as i16,
    NFSERR_SERVERFAULT as i16,
    0,
];

/// `nfsv3err_setattr`.
const NFSV3ERR_SETATTR: &[i16] = &[
    NFSERR_IO as i16,
    NFSERR_PERM as i16,
    NFSERR_IO as i16,
    NFSERR_ACCES as i16,
    NFSERR_INVAL as i16,
    NFSERR_NOSPC as i16,
    NFSERR_ROFS as i16,
    NFSERR_DQUOT as i16,
    NFSERR_STALE as i16,
    NFSERR_BADHANDLE as i16,
    NFSERR_NOT_SYNC as i16,
    NFSERR_SERVERFAULT as i16,
    0,
];

/// `nfsv3err_lookup`.
const NFSV3ERR_LOOKUP: &[i16] = &[
    NFSERR_IO as i16,
    NFSERR_NOENT as i16,
    NFSERR_IO as i16,
    NFSERR_ACCES as i16,
    NFSERR_NOTDIR as i16,
    NFSERR_NAMETOL as i16,
    NFSERR_STALE as i16,
    NFSERR_BADHANDLE as i16,
    NFSERR_SERVERFAULT as i16,
    0,
];

/// `nfsv3err_access`.
const NFSV3ERR_ACCESS: &[i16] = &[
    NFSERR_IO as i16,
    NFSERR_IO as i16,
    NFSERR_STALE as i16,
    NFSERR_BADHANDLE as i16,
    NFSERR_SERVERFAULT as i16,
    0,
];

/// `nfsv3err_readlink`.
const NFSV3ERR_READLINK: &[i16] = &[
    NFSERR_IO as i16,
    NFSERR_IO as i16,
    NFSERR_ACCES as i16,
    NFSERR_INVAL as i16,
    NFSERR_STALE as i16,
    NFSERR_BADHANDLE as i16,
    NFSERR_NOTSUPP as i16,
    NFSERR_SERVERFAULT as i16,
    0,
];

/// `nfsv3err_read`.
const NFSV3ERR_READ: &[i16] = &[
    NFSERR_IO as i16,
    NFSERR_IO as i16,
    NFSERR_NXIO as i16,
    NFSERR_ACCES as i16,
    NFSERR_INVAL as i16,
    NFSERR_STALE as i16,
    NFSERR_BADHANDLE as i16,
    NFSERR_SERVERFAULT as i16,
    0,
];

/// `nfsv3err_write`.
const NFSV3ERR_WRITE: &[i16] = &[
    NFSERR_IO as i16,
    NFSERR_IO as i16,
    NFSERR_ACCES as i16,
    NFSERR_INVAL as i16,
    NFSERR_FBIG as i16,
    NFSERR_NOSPC as i16,
    NFSERR_ROFS as i16,
    NFSERR_DQUOT as i16,
    NFSERR_STALE as i16,
    NFSERR_BADHANDLE as i16,
    NFSERR_SERVERFAULT as i16,
    0,
];

/// `nfsv3err_create`.
const NFSV3ERR_CREATE: &[i16] = &[
    NFSERR_IO as i16,
    NFSERR_IO as i16,
    NFSERR_ACCES as i16,
    NFSERR_EXIST as i16,
    NFSERR_NOTDIR as i16,
    NFSERR_NOSPC as i16,
    NFSERR_ROFS as i16,
    NFSERR_NAMETOL as i16,
    NFSERR_DQUOT as i16,
    NFSERR_STALE as i16,
    NFSERR_BADHANDLE as i16,
    NFSERR_NOTSUPP as i16,
    NFSERR_SERVERFAULT as i16,
    0,
];

/// `nfsv3err_mkdir`.
const NFSV3ERR_MKDIR: &[i16] = &[
    NFSERR_IO as i16,
    NFSERR_IO as i16,
    NFSERR_ACCES as i16,
    NFSERR_EXIST as i16,
    NFSERR_NOTDIR as i16,
    NFSERR_NOSPC as i16,
    NFSERR_ROFS as i16,
    NFSERR_NAMETOL as i16,
    NFSERR_DQUOT as i16,
    NFSERR_STALE as i16,
    NFSERR_BADHANDLE as i16,
    NFSERR_NOTSUPP as i16,
    NFSERR_SERVERFAULT as i16,
    0,
];

/// `nfsv3err_symlink`.
const NFSV3ERR_SYMLINK: &[i16] = &[
    NFSERR_IO as i16,
    NFSERR_IO as i16,
    NFSERR_ACCES as i16,
    NFSERR_EXIST as i16,
    NFSERR_NOTDIR as i16,
    NFSERR_NOSPC as i16,
    NFSERR_ROFS as i16,
    NFSERR_NAMETOL as i16,
    NFSERR_DQUOT as i16,
    NFSERR_STALE as i16,
    NFSERR_BADHANDLE as i16,
    NFSERR_NOTSUPP as i16,
    NFSERR_SERVERFAULT as i16,
    0,
];

/// `nfsv3err_mknod`.
const NFSV3ERR_MKNOD: &[i16] = &[
    NFSERR_IO as i16,
    NFSERR_IO as i16,
    NFSERR_ACCES as i16,
    NFSERR_EXIST as i16,
    NFSERR_NOTDIR as i16,
    NFSERR_NOSPC as i16,
    NFSERR_ROFS as i16,
    NFSERR_NAMETOL as i16,
    NFSERR_DQUOT as i16,
    NFSERR_STALE as i16,
    NFSERR_BADHANDLE as i16,
    NFSERR_NOTSUPP as i16,
    NFSERR_SERVERFAULT as i16,
    NFSERR_BADTYPE as i16,
    0,
];

/// `nfsv3err_remove`.
const NFSV3ERR_REMOVE: &[i16] = &[
    NFSERR_IO as i16,
    NFSERR_NOENT as i16,
    NFSERR_IO as i16,
    NFSERR_ACCES as i16,
    NFSERR_NOTDIR as i16,
    NFSERR_ROFS as i16,
    NFSERR_NAMETOL as i16,
    NFSERR_STALE as i16,
    NFSERR_BADHANDLE as i16,
    NFSERR_SERVERFAULT as i16,
    0,
];

/// `nfsv3err_rmdir`.
const NFSV3ERR_RMDIR: &[i16] = &[
    NFSERR_IO as i16,
    NFSERR_NOENT as i16,
    NFSERR_IO as i16,
    NFSERR_ACCES as i16,
    NFSERR_EXIST as i16,
    NFSERR_NOTDIR as i16,
    NFSERR_INVAL as i16,
    NFSERR_ROFS as i16,
    NFSERR_NAMETOL as i16,
    NFSERR_NOTEMPTY as i16,
    NFSERR_STALE as i16,
    NFSERR_BADHANDLE as i16,
    NFSERR_NOTSUPP as i16,
    NFSERR_SERVERFAULT as i16,
    0,
];

/// `nfsv3err_rename`.
const NFSV3ERR_RENAME: &[i16] = &[
    NFSERR_IO as i16,
    NFSERR_NOENT as i16,
    NFSERR_IO as i16,
    NFSERR_ACCES as i16,
    NFSERR_EXIST as i16,
    NFSERR_XDEV as i16,
    NFSERR_NOTDIR as i16,
    NFSERR_ISDIR as i16,
    NFSERR_INVAL as i16,
    NFSERR_NOSPC as i16,
    NFSERR_ROFS as i16,
    NFSERR_MLINK as i16,
    NFSERR_NAMETOL as i16,
    NFSERR_NOTEMPTY as i16,
    NFSERR_DQUOT as i16,
    NFSERR_STALE as i16,
    NFSERR_BADHANDLE as i16,
    NFSERR_NOTSUPP as i16,
    NFSERR_SERVERFAULT as i16,
    0,
];

/// `nfsv3err_link`.
const NFSV3ERR_LINK: &[i16] = &[
    NFSERR_IO as i16,
    NFSERR_IO as i16,
    NFSERR_ACCES as i16,
    NFSERR_EXIST as i16,
    NFSERR_XDEV as i16,
    NFSERR_NOTDIR as i16,
    NFSERR_INVAL as i16,
    NFSERR_NOSPC as i16,
    NFSERR_ROFS as i16,
    NFSERR_MLINK as i16,
    NFSERR_NAMETOL as i16,
    NFSERR_DQUOT as i16,
    NFSERR_STALE as i16,
    NFSERR_BADHANDLE as i16,
    NFSERR_NOTSUPP as i16,
    NFSERR_SERVERFAULT as i16,
    0,
];

/// `nfsv3err_readdir`.
const NFSV3ERR_READDIR: &[i16] = &[
    NFSERR_IO as i16,
    NFSERR_IO as i16,
    NFSERR_ACCES as i16,
    NFSERR_NOTDIR as i16,
    NFSERR_STALE as i16,
    NFSERR_BADHANDLE as i16,
    NFSERR_BAD_COOKIE as i16,
    NFSERR_TOOSMALL as i16,
    NFSERR_SERVERFAULT as i16,
    0,
];

/// `nfsv3err_readdirplus`.
const NFSV3ERR_READDIRPLUS: &[i16] = &[
    NFSERR_IO as i16,
    NFSERR_IO as i16,
    NFSERR_ACCES as i16,
    NFSERR_NOTDIR as i16,
    NFSERR_STALE as i16,
    NFSERR_BADHANDLE as i16,
    NFSERR_BAD_COOKIE as i16,
    NFSERR_NOTSUPP as i16,
    NFSERR_TOOSMALL as i16,
    NFSERR_SERVERFAULT as i16,
    0,
];

/// `nfsv3err_fsstat`.
const NFSV3ERR_FSSTAT: &[i16] = &[
    NFSERR_IO as i16,
    NFSERR_IO as i16,
    NFSERR_STALE as i16,
    NFSERR_BADHANDLE as i16,
    NFSERR_SERVERFAULT as i16,
    0,
];

/// `nfsv3err_fsinfo`.
const NFSV3ERR_FSINFO: &[i16] = &[
    NFSERR_STALE as i16,
    NFSERR_STALE as i16,
    NFSERR_BADHANDLE as i16,
    NFSERR_SERVERFAULT as i16,
    0,
];

/// `nfsv3err_pathconf`.
const NFSV3ERR_PATHCONF: &[i16] = &[
    NFSERR_STALE as i16,
    NFSERR_STALE as i16,
    NFSERR_BADHANDLE as i16,
    NFSERR_SERVERFAULT as i16,
    0,
];

/// `nfsv3err_commit`.
const NFSV3ERR_COMMIT: &[i16] = &[
    NFSERR_IO as i16,
    NFSERR_IO as i16,
    NFSERR_STALE as i16,
    NFSERR_BADHANDLE as i16,
    NFSERR_SERVERFAULT as i16,
    0,
];

/// `nfsrv_v3errmap`: the version 3 error list of each procedure, `NFSPROC_NULL` to
/// `NFSPROC_COMMIT`.
pub const NFSRV_V3ERRMAP: [&[i16]; 22] = [
    NFSV3ERR_NULL,
    NFSV3ERR_GETATTR,
    NFSV3ERR_SETATTR,
    NFSV3ERR_LOOKUP,
    NFSV3ERR_ACCESS,
    NFSV3ERR_READLINK,
    NFSV3ERR_READ,
    NFSV3ERR_WRITE,
    NFSV3ERR_CREATE,
    NFSV3ERR_MKDIR,
    NFSV3ERR_SYMLINK,
    NFSV3ERR_MKNOD,
    NFSV3ERR_REMOVE,
    NFSV3ERR_RMDIR,
    NFSV3ERR_RENAME,
    NFSV3ERR_LINK,
    NFSV3ERR_READDIR,
    NFSV3ERR_READDIRPLUS,
    NFSV3ERR_FSSTAT,
    NFSV3ERR_FSINFO,
    NFSV3ERR_PATHCONF,
    NFSV3ERR_COMMIT,
];

/// `nfs_ticks`: NFS timer ticks per `hz` tick (`nfs_init`).
pub static NFS_TICKS: AtomicI32 = AtomicI32::new(0);

/// `nfsstats`.
pub static NFSSTATS: Nfsstats = Nfsstats::new();

/// `nfsreqpl`: the `struct nfsreq` pool.
pub static NFSREQPL: Pool = Pool::new();

/// `nfs_xid_mtx` (not in C, see the deviations): guards [`NFS_XID_CTX`].
static NFS_XID_MTX: Mutex = Mutex::new(IPL_SOFTNET);

/// `nfs_xid_ctx`: `nfs_get_xid`'s generator; touched only under [`NFS_XID_MTX`].
static NFS_XID_CTX: StaticCell<Idgen32Ctx> = StaticCell::new(Idgen32Ctx::zeroed());

/// `called`: `nfs_get_xid` has seeded [`NFS_XID_CTX`].
static NFS_XID_CALLED: AtomicBool = AtomicBool::new(false);

/// `MGET(m, M_WAIT, MT_DATA)` where the C cannot fail: panics when the pool cannot serve
/// (it cannot sleep here, `subr_pool.rs`).
fn nfsm_mget() -> &'static Mbuf {
    match m_get(M_WAIT, MT_DATA) {
        Some(m) => m,
        None => panic(format_args!("nfs: out of mbufs")),
    }
}

/// `nfsm_reqhead(hsiz)`: creates the header for an RPC request packet. The `hsiz` is the
/// size of the rest of the NFS request header (just used to decide if a cluster is a good
/// idea).
pub fn nfsm_reqhead(hsiz: usize) -> &'static Mbuf {
    let mb = nfsm_mget();
    if hsiz > MLEN {
        mclget(mb, M_WAIT);
    }
    mb.m_len().set(0);

    // Finally, return values
    mb
}

/// `nfs_get_xid`: returns an unpredictable XID in XDR form.
pub fn nfs_get_xid() -> u32 {
    mtx_enter(&NFS_XID_MTX);
    // SAFETY: NFS_XID_CTX is touched only here, with NFS_XID_MTX held; no other reference to
    // it exists while this one lives.
    let ctx = unsafe { NFS_XID_CTX.get_mut() };
    if !NFS_XID_CALLED.load(Ordering::Relaxed) {
        NFS_XID_CALLED.store(true, Ordering::Relaxed);
        idgen32_init(ctx);
    }
    let xid = idgen32(ctx);
    mtx_leave(&NFS_XID_MTX);
    txdr_unsigned(xid)
}

/// `nfsm_rpchead(req, cr, auth_type)`: builds the RPC header and fills in the authorization
/// info in `req`'s request mbuf (`r_mreq`, a packet header mbuf with nothing in it yet), and
/// stores the new xid in `r_xid`. Right now we are pretty centric around `RPCAUTH_UNIX`; in
/// the future, this function will need some love to be able to handle other authorization
/// methods, such as Kerberos.
pub fn nfsm_rpchead(req: &NfsReq, cr: &Ucred, auth_type: u32) {
    kassert!(auth_type == RPCAUTH_UNIX);

    // RPCAUTH_UNIX fits in an hdr mbuf, in the future other authorization methods need to
    // figure out their own sizes and allocate and chain mbufs accordingly.
    let Some(mut mb) = req.r_mreq.get() else {
        panic(format_args!("nfsm_rpchead: no request mbuf"));
    };
    let Some(nmp) = req.r_nmp.get() else {
        panic(format_args!("nfsm_rpchead: no mount"));
    };

    // We need to start out by finding how big the authorization cred and verifier are for
    // the auth_type, to be able to correctly align the mbuf header/chain. In the
    // RPCAUTH_UNIX case, the size is the static part as shown in RFC1831 + the number of
    // groups, RPCAUTH_UNIX has a zero verifier.
    let ngroups = i32::from(cr.cr_ngroups.get())
        .min(nmp.nm_numgrps.get())
        .max(0) as usize;
    let auth_len = (ngroups << 2) + 5 * NFSX_UNSIGNED;
    let authsiz = nfsm_rndup(auth_len);
    // The authorization size + the size of the static part.
    m_align(mb, (authsiz + 10 * NFSX_UNSIGNED) as i32);

    mb.m_len().set(0);

    // First the RPC header.
    let mut tl = nfsm_build(&mut mb, 6 * NFSX_UNSIGNED);

    // Get a new (non-zero) xid.
    let xid = nfs_get_xid();
    req.r_xid.set(xid);
    tl.put(xid);
    tl.put(rpc_call);
    tl.put(rpc_vers);
    tl.put(nfs_prog);
    if nmp.nm_flag.get() & NFSMNT_NFSV3 != 0 {
        tl.put(txdr_unsigned(NFS_VER3));
        tl.put(txdr_unsigned(req.r_procnum.get() as u32));
    } else {
        tl.put(txdr_unsigned(NFS_VER2));
        tl.put(txdr_unsigned(NFSV2_PROCID[req.r_procnum.get()] as u32));
    }

    // The Authorization cred and its verifier.
    let mut tl = nfsm_build(&mut mb, auth_len + 4 * NFSX_UNSIGNED);
    tl.put(txdr_unsigned(RPCAUTH_UNIX));
    tl.put(txdr_unsigned(authsiz as u32));

    // The authorization cred.
    tl.put(0); // stamp
    tl.put(0); // NULL hostname
    tl.put(txdr_unsigned(cr.cr_uid.get()));
    tl.put(txdr_unsigned(cr.cr_gid.get()));
    tl.put(txdr_unsigned(ngroups as u32));
    for g in &cr.cr_groups[..ngroups] {
        tl.put(txdr_unsigned(g.get()));
    }
    // The authorization verifier.
    tl.put(txdr_unsigned(RPCAUTH_NULL));
    tl.put(0);

    let ph = mb.m_pkthdr();
    ph.len
        .set(ph.len.get() + (authsiz + 10 * NFSX_UNSIGNED) as i32);
    ph.ph_ifidx.set(0);
}

/// `nfsm_mbuftouio(mrep, uiop, siz, dpos)`: copies `siz` bytes of an mbuf chain, from the
/// cursor (`*mrep`, `*dpos`), to the uio scatter/gather list, then skips the XDR padding.
pub fn nfsm_mbuftouio(
    mrep: &mut Option<&'static Mbuf>,
    uiop: &mut Uio<'_>,
    mut siz: usize,
    dpos: &mut *mut u8,
) -> Result<(), Errno> {
    let Some(mut mp) = *mrep else {
        return Err(Errno::EBADRPC);
    };
    let mut mbufcp = *dpos;
    let mut len = nfsm_avail(mp, mbufcp);
    let rem = nfsm_padlen(siz);
    while siz > 0 {
        if uiop.uio_iov.is_empty() {
            return Err(Errno::EFBIG);
        }
        let mut left = uiop.uio_iov[0].iov_len;
        let mut uiocp = uiop.uio_iov[0].iov_base.cast::<u8>();
        if left > siz {
            left = siz;
        }
        let uiosiz = left;
        while left > 0 {
            while len == 0 {
                mp = mp.m_next().get().ok_or(Errno::EBADRPC)?;
                mbufcp = mtod::<u8>(mp);
                len = mp.m_len().get() as usize;
            }
            let xfer = left.min(len);
            if uiop.uio_segflg == UioSeg::UIO_SYSSPACE {
                // SAFETY: `xfer <= len` bytes at `mbufcp` are `mp`'s data; a UIO_SYSSPACE
                // iovec is a kernel buffer of at least `iov_len >= xfer` bytes past `uiocp`
                // that its builder vouches for (`struct uio`'s contract); the two are
                // distinct memory.
                unsafe { ptr::copy_nonoverlapping(mbufcp, uiocp, xfer) };
            } else {
                // SAFETY: `xfer <= len` bytes at `mbufcp` are `mp`'s data, initialised and
                // not written during the copy.
                let src = unsafe { slice::from_raw_parts(mbufcp, xfer) };
                // The C ignores copyout's error.
                let _ = copyout(src, uiocp.addr());
            }
            left -= xfer;
            len -= xfer;
            mbufcp = mbufcp.wrapping_add(xfer);
            uiocp = uiocp.wrapping_add(xfer);
            uiop.uio_offset += xfer as i64;
            uiop.uio_resid -= xfer;
        }
        if uiop.uio_iov[0].iov_len <= siz {
            let iov = core::mem::take(&mut uiop.uio_iov);
            uiop.uio_iov = &mut iov[1..];
        } else {
            let iov = &mut uiop.uio_iov[0];
            iov.iov_base = iov.iov_base.wrapping_byte_add(uiosiz);
            iov.iov_len -= uiosiz;
        }
        siz -= uiosiz;
    }
    *dpos = mbufcp;
    *mrep = Some(mp);
    if rem > 0 {
        if len < rem {
            nfs_adv(mrep, dpos, rem)?;
        } else {
            *dpos = dpos.wrapping_add(rem);
        }
    }
    Ok(())
}

/// `nfsm_uiotombuf(mp, uiop, len)`: copies `len` bytes of a uio scatter/gather list to the
/// end of the mbuf chain at the build cursor `*mp`, then the XDR padding.
pub fn nfsm_uiotombuf(mp: &mut &'static Mbuf, uiop: &mut Uio<'_>, mut len: usize) {
    let mut mb = *mp;

    let pad = nfsm_padlen(len);

    // XXX -- the following should be done by the caller
    uiop.uio_resid = len;
    uiop.uio_rw = UioRw::UIO_WRITE;

    while len > 0 {
        let xfer = len.min(m_trailingspace(mb) as usize);
        // SAFETY: the `xfer` bytes after `mb`'s data are its trailing space
        // (`m_trailingspace`), storage of the chain being built that nothing else uses.
        let dst = unsafe { slice::from_raw_parts_mut(mb_offset(mb), xfer) };
        // The C ignores uiomove's error.
        let _ = uiomove(dst, uiop);
        mb.m_len().set(mb.m_len().get() + xfer as u32);
        len -= xfer;
        if len > 0 {
            let mb2 = nfsm_mget();
            if len > MLEN {
                mclget(mb2, M_WAIT);
            }
            mb2.m_len().set(0);
            mb.m_next().set(Some(mb2));
            mb = mb2;
        }
    }

    if pad > 0 {
        if pad > m_trailingspace(mb) as usize {
            let mb2 = nfsm_mget();
            mb2.m_len().set(0);
            mb.m_next().set(Some(mb2));
            mb = mb2;
        }
        // SAFETY: the `pad` bytes after `mb`'s data are its trailing space (checked above, or
        // a fresh mbuf of `MLEN` bytes), storage of the chain being built.
        unsafe { ptr::write_bytes(mb_offset(mb), 0, pad) };
        mb.m_len().set(mb.m_len().get() + pad as u32);
    }

    *mp = mb;
}

/// `nfsm_buftombuf(mp, buf, len)`: copies a buffer to the end of an mbuf chain.
pub fn nfsm_buftombuf(mp: &mut &'static Mbuf, buf: &[u8]) {
    let mut iov = [Iovec {
        iov_base: buf.as_ptr().cast_mut().cast::<c_void>(),
        iov_len: buf.len(),
    }];

    // A UIO_WRITE uio is only read from: `buf` is not written.
    let mut io = Uio {
        uio_iov: &mut iov,
        uio_offset: 0,
        uio_resid: buf.len(),
        uio_segflg: UioSeg::UIO_SYSSPACE,
        uio_rw: UioRw::UIO_WRITE,
        uio_procp: None,
    };

    nfsm_uiotombuf(mp, &mut io, buf.len());
}

/// `nfsm_strtombuf(mp, str, len)`: copies a string to the end of an mbuf chain, as XDR: the
/// length word, the bytes, the padding.
pub fn nfsm_strtombuf(mp: &mut &'static Mbuf, str: &[u8]) {
    let strlen = txdr_unsigned(str.len() as u32);

    let mut iov = [
        Iovec {
            iov_base: ptr::from_ref(&strlen).cast_mut().cast::<c_void>(),
            iov_len: size_of::<u32>(),
        },
        Iovec {
            iov_base: str.as_ptr().cast_mut().cast::<c_void>(),
            iov_len: str.len(),
        },
    ];

    // A UIO_WRITE uio is only read from: neither buffer is written.
    let resid = size_of::<u32>() + str.len();
    let mut io = Uio {
        uio_iov: &mut iov,
        uio_offset: 0,
        uio_resid: resid,
        uio_segflg: UioSeg::UIO_SYSSPACE,
        uio_rw: UioRw::UIO_WRITE,
        uio_procp: None,
    };

    nfsm_uiotombuf(mp, &mut io, resid);
}

/// `nfsm_disct(mdp, dposp, siz, left, cp2)`: helps break down an mbuf chain by making the
/// next `siz` bytes at the cursor contiguous, returned as a view. This is used by
/// `nfsm_dissect` for tough cases: when they straddle mbufs, a new mbuf is inserted that
/// holds them.
pub fn nfsm_disct<'a>(
    mdp: &'a mut Option<&'static Mbuf>,
    dposp: &'a mut *mut u8,
    siz: usize,
) -> Result<XdrIn<'a>, Errno> {
    let Some(mut mp) = *mdp else {
        return Err(Errno::EBADRPC);
    };
    let mut left = nfsm_avail(mp, *dposp);
    while left == 0 {
        *mdp = mp.m_next().get();
        let Some(m) = *mdp else {
            return Err(Errno::EBADRPC);
        };
        mp = m;
        left = mp.m_len().get() as usize;
        *dposp = mtod::<u8>(mp);
    }
    if left >= siz {
        let cp2 = *dposp;
        *dposp = cp2.wrapping_add(siz);
        // SAFETY: `siz <= left` bytes at `cp2` are `mp`'s data, which the chain's owner does
        // not write while the view borrows its cursor.
        return Ok(unsafe { XdrIn::new(cp2, siz) });
    }
    if mp.m_next().get().is_none() {
        return Err(Errno::EBADRPC);
    }
    if siz > MHLEN {
        panic(format_args!("nfs S too big"));
    }

    let Some(mp2) = m_get(M_WAIT, MT_DATA) else {
        return Err(Errno::ENOBUFS);
    };
    mp2.m_next().set(mp.m_next().get());
    mp.m_next().set(Some(mp2));
    mp.m_len().set(mp.m_len().get() - left as u32);
    let mp = mp2;
    let cp2 = mtod::<u8>(mp);
    // Copy what was left.
    // SAFETY: the `left` bytes at the cursor are the old mbuf's data (`nfsm_avail`, before
    // its length was cut); `cp2` is the fresh mbuf's data area of `MLEN >= MHLEN >= siz`
    // bytes; the two do not overlap.
    unsafe { ptr::copy_nonoverlapping(*dposp, cp2, left) };
    let mut siz2 = siz - left;
    let mut p = left;
    let mut next = mp.m_next().get();
    // Loop around copying up the siz2 bytes.
    let last = loop {
        let Some(m2) = next else {
            return Err(Errno::EBADRPC);
        };
        let xfer = siz2.min(m2.m_len().get() as usize);
        if xfer > 0 {
            // SAFETY: `xfer` bytes at `m2`'s data are its data; `p + xfer <= siz` bytes of
            // the fresh mbuf's data area (checked above); distinct mbufs.
            unsafe { ptr::copy_nonoverlapping(mtod::<u8>(m2), cp2.add(p), xfer) };
            m2.m_data().set(m2.m_data().get().wrapping_add(xfer));
            m2.m_len().set(m2.m_len().get() - xfer as u32);
            p += xfer;
            siz2 -= xfer;
        }
        if siz2 == 0 {
            break m2;
        }
        next = m2.m_next().get();
    };
    mp.m_len().set(siz as u32);
    *mdp = Some(last);
    *dposp = mtod::<u8>(last);
    // SAFETY: the `siz` bytes at `cp2` were just copied into the inserted mbuf, whose data
    // they now are; the chain's owner does not write them while the view borrows its
    // cursor.
    Ok(unsafe { XdrIn::new(cp2, siz) })
}

/// `nfs_adv(mdp, dposp, offs, left)`: advances the position in the mbuf chain by `offs`
/// bytes.
pub fn nfs_adv(
    mdp: &mut Option<&'static Mbuf>,
    dposp: &mut *mut u8,
    mut offs: usize,
) -> Result<(), Errno> {
    let Some(mut m) = *mdp else {
        return Err(Errno::EBADRPC);
    };
    let mut s = nfsm_avail(m, *dposp);
    if s >= offs {
        *dposp = dposp.wrapping_add(offs);
        return Ok(());
    }
    while s < offs {
        offs -= s;
        m = m.m_next().get().ok_or(Errno::EBADRPC)?;
        s = m.m_len().get() as usize;
    }
    *mdp = Some(m);
    *dposp = mtod::<u8>(m).wrapping_add(offs);
    Ok(())
}

/// The cursor walk of `nfs_namei` (`nfs_srvsubs.c`), which reads a name byte by byte across
/// mbufs without making it contiguous: the bytes at the cursor that are in its current mbuf,
/// at most `max` of them, after skipping exhausted mbufs (`EBADRPC` at the end of the
/// chain); the cursor moves past them. A Rust helper, so that no caller dereferences
/// `dpos`.
pub fn nfsm_nextbytes<'a>(
    mdp: &'a mut Option<&'static Mbuf>,
    dposp: &'a mut *mut u8,
    max: usize,
) -> Result<XdrIn<'a>, Errno> {
    let Some(mut md) = *mdp else {
        return Err(Errno::EBADRPC);
    };
    let mut rem = nfsm_avail(md, *dposp);
    while rem == 0 && max > 0 {
        md = md.m_next().get().ok_or(Errno::EBADRPC)?;
        *mdp = Some(md);
        *dposp = mtod::<u8>(md);
        rem = md.m_len().get() as usize;
    }
    let n = rem.min(max);
    let p = *dposp;
    *dposp = p.wrapping_add(n);
    // SAFETY: `n <= rem` bytes at `p` are `md`'s data, which the chain's owner does not write
    // while the view borrows its cursor.
    Ok(unsafe { XdrIn::new(p, n) })
}

/// `nfs_init`: called once to initialize data structures...
pub fn nfs_init() {
    // The XDR words of the C (`rpc_vers = txdr_unsigned(RPC_VER2)`, ...) are constants.
    let ticks = (HZ.load(Ordering::Relaxed) * NFS_TICKINTVL + 500) / 1000;
    NFS_TICKS.store(ticks.max(1), Ordering::Relaxed);
    #[cfg(feature = "nfsserver")]
    {
        // M10e: `nfsrv_init(0)` (nfs_syscalls.rs) and `nfsrv_initcache()` (nfs_srvcache.rs)
        // once those files land: init server data structures and the server request cache.
        let _ = crate::unported!("nfsrv_init");
        let _ = crate::unported!("nfsrv_initcache");
    }

    pool_init(
        &NFSREQPL,
        size_of::<NfsReq>(),
        0,
        IPL_NONE,
        PR_WAITOK,
        "nfsreqpl",
        None,
    );
}

/// `nfs_vfs_init` (`vfs_init`): the nfsiod buffer queue and the nfsnode pool.
#[cfg(feature = "nfsclient")]
pub fn nfs_vfs_init(_vfsp: &'static Vfsconf) -> Result<(), Errno> {
    NFS_BUFQ.0.init();

    pool_init(
        &NFS_NODE_POOL,
        size_of::<NfsNode>(),
        0,
        IPL_NONE,
        PR_WAITOK,
        "nfsnodepl",
        None,
    );

    Ok(())
}

/// `nfs_loadattrcache(vpp, mdp, dposp, vaper)`: loads the attribute cache (that lives in the
/// nfsnode entry) with the values on the mbuf list and, iff `vaper` is given, copies the
/// attributes to it. A new device node may be replaced by an alias (`*vpp`).
#[cfg(feature = "nfsclient")]
pub fn nfs_loadattrcache(
    vpp: &mut &'static Vnode,
    mdp: &mut Option<&'static Mbuf>,
    dposp: &mut *mut u8,
    vaper: Option<&mut Vattr>,
) -> Result<(), Errno> {
    let mut vp = *vpp;
    let v3 = crate::nfs::nfs::nfs_isv3(vp);

    let fp: NfsFattr = nfsm_disct(mdp, dposp, nfsx_fattr(v3))?.read(0);
    let mut vtyp;
    let vmode;
    let rdev: i32;
    let mtime;
    if v3 {
        vtyp = nfsv3tov_type(fp.fa_type);
        vmode = fxdr_unsigned(fp.fa_mode);
        let spec = fp.fa3_rdev();
        rdev = makedev(fxdr_unsigned(spec.specdata1), fxdr_unsigned(spec.specdata2));
        mtime = fxdr_nfsv3time(&fp.fa3_mtime());
    } else {
        vtyp = nfsv2tov_type(fp.fa_type);
        vmode = fxdr_unsigned(fp.fa_mode);
        if vtyp == VNON || vtyp == VREG {
            vtyp = iftovt(vmode);
        }
        rdev = fxdr_unsigned(fp.fa2_rdev()) as i32;
        mtime = fxdr_nfsv2time(&fp.fa2_mtime());

        // Really ugly NFSv2 kludge.
        if vtyp == VCHR && rdev == -1 {
            vtyp = VFIFO;
        }
    }

    // If v_type == VNON it is a new node, so fill in the v_type, n_mtime fields. Check to see
    // if it represents a special device, and if so, check for a possible alias. Once the
    // correct vnode has been obtained, fill in the rest of the information.
    let np = VTONFS(vp);
    if vp.v_type.get() == VNON {
        cache_purge(vp);
        vp.v_type.set(vtyp);
        if vtyp == VFIFO {
            // `option FIFO` is in GENERIC but miscfs/fifofs is not ported: the C's !FIFO
            // path.
            return Err(Errno::EOPNOTSUPP);
        }
        if vtyp == VCHR || vtyp == VBLK {
            // M10e: `vp->v_op = &nfs_specvops` once nfs_vnops.rs (agent vnops) lands.
            let _ = crate::unported!("nfs_specvops");
            if let Some(nvp) = checkalias(vp, rdev, vp.v_mount.get()) {
                // Discard unneeded vnode, but save its nfsnode. Since the nfsnode does not
                // have a lock, its vnode lock has to be carried over.
                nvp.v_data.set(vp.v_data.get());
                vp.v_data.set(ptr::null_mut());
                vp.v_op.set(Some(&SPEC_VOPS));
                vrele(vp);
                vgone(vp);
                // Reinitialize aliased node.
                np.n_vnode.set(Some(nvp));
                vp = nvp;
                *vpp = nvp;
            }
        }
        np.n_mtime.set(mtime);
    }
    let Some(mp) = vp.v_mount.get() else {
        panic(format_args!(
            "nfs_loadattrcache: vnode {:p} has no mount",
            vp
        ));
    };
    let mut vap = np.n_vattr.get();
    vap.va_type = vtyp;
    vap.va_rdev = rdev;
    vap.va_mtime = mtime;
    vap.va_fsid = i64::from(mp.mnt_stat.get().f_fsid.val[0]);

    let uid = fxdr_unsigned(fp.fa_uid);
    let gid = fxdr_unsigned(fp.fa_gid);
    // Invalidate access cache if uid, gid or mode changed.
    if np.n_accstamp.get() != -1
        && (gid != vap.va_gid || uid != vap.va_uid || (vmode & 0o7777) != vap.va_mode)
    {
        np.n_accstamp.set(-1);
    }

    vap.va_mode = vmode & 0o7777;

    vap.va_blocksize = match vtyp {
        VBLK => BLKDEV_IOSIZE as i64,
        VCHR => MAXBSIZE as i64,
        _ => {
            if v3 {
                i64::from(mp.mnt_stat.get().f_iosize)
            } else {
                i64::from(fxdr_unsigned(fp.fa2_blocksize()) as i32)
            }
        }
    };
    vap.va_nlink = fxdr_unsigned(fp.fa_nlink);
    vap.va_uid = uid;
    vap.va_gid = gid;
    if v3 {
        vap.va_size = fxdr_hyper(fp.fa3_size().words());
        vap.va_bytes = fxdr_hyper(fp.fa3_used().words());
        vap.va_fileid = fxdr_hyper(fp.fa3_fileid().words());
        vap.va_atime = fxdr_nfsv3time(&fp.fa3_atime());
        vap.va_ctime = fxdr_nfsv3time(&fp.fa3_ctime());
    } else {
        vap.va_size = u64::from(fxdr_unsigned(fp.fa2_size()));
        // `(u_quad_t)int32_t`: sign-extended, then multiplied modulo 2^64.
        vap.va_bytes = (fxdr_unsigned(fp.fa2_blocks()) as i32 as u64).wrapping_mul(NFS_FABLKSIZE);
        vap.va_fileid = fxdr_unsigned(fp.fa2_fileid()) as i32 as u64;
        vap.va_atime = fxdr_nfsv2time(&fp.fa2_atime());
        vap.va_ctime = Timespec {
            tv_sec: fxdr_unsigned(fp.fa2_ctime().nfsv2_sec) as Time,
            tv_nsec: 0,
        };
        vap.va_gen = u64::from(fxdr_unsigned(fp.fa2_ctime().nfsv2_usec));
    }
    vap.va_flags = 0;
    vap.va_filerev = 0;

    if vap.va_size != np.n_size.get() {
        if vap.va_type == VREG {
            if np.n_flag.get() & NMODIFIED != 0 {
                if vap.va_size < np.n_size.get() {
                    vap.va_size = np.n_size.get();
                } else {
                    np.n_size.set(vap.va_size);
                }
            } else {
                np.n_size.set(vap.va_size);
            }
            np.n_vattr.set(vap);
            uvm_vnp_setsize(vp, np.n_size.get() as Voff);
        } else {
            np.n_size.set(vap.va_size);
        }
    }
    np.n_vattr.set(vap);
    np.n_attrstamp.set(gettime());
    if let Some(vaper) = vaper {
        *vaper = vap;
        if np.n_flag.get() & NCHG != 0 {
            if np.n_flag.get() & NACC != 0 {
                vaper.va_atime = np.n_atim.get();
            }
            if np.n_flag.get() & NUPD != 0 {
                vaper.va_mtime = np.n_mtim.get();
            }
        }
    }
    Ok(())
}

/// `nfs_attrtimeo(np)`: the attribute cache timeout of a node, in seconds: a tenth of the
/// age of its last modification, clamped to the mount's `acregmin..acregmax` (directories:
/// `acdirmin..acdirmax`); the minimum while it has local modifications.
#[cfg(feature = "nfsclient")]
pub fn nfs_attrtimeo(np: &NfsNode) -> i32 {
    let vp = NFSTOV(np);
    let Some(mp) = vp.v_mount.get() else {
        panic(format_args!("nfs_attrtimeo: vnode {:p} has no mount", vp));
    };
    let nmp = VFSTONFS(mp);
    let tenthage = ((gettime() - np.n_mtime.get().tv_sec) / 10) as i32;

    let (minto, maxto) = if vp.v_type.get() == VDIR {
        (
            i32::from(nmp.nm_acdirmin.get()),
            i32::from(nmp.nm_acdirmax.get()),
        )
    } else {
        (
            i32::from(nmp.nm_acregmin.get()),
            i32::from(nmp.nm_acregmax.get()),
        )
    };

    if np.n_flag.get() & NMODIFIED != 0 || tenthage < minto {
        minto
    } else if tenthage < maxto {
        tenthage
    } else {
        maxto
    }
}

/// `nfs_getattrcache(vp, vaper)`: checks the time stamp; if the cache is valid, copies its
/// contents to `vaper`, otherwise fails with `ENOENT`.
#[cfg(feature = "nfsclient")]
pub fn nfs_getattrcache(vp: &'static Vnode, vaper: &mut Vattr) -> Result<(), Errno> {
    let np = VTONFS(vp);

    if np.n_attrstamp.get() == 0 || gettime() - np.n_attrstamp.get() >= i64::from(nfs_attrtimeo(np))
    {
        NFSSTATS.attrcache_misses.fetch_add(1, Ordering::Relaxed);
        return Err(Errno::ENOENT);
    }
    NFSSTATS.attrcache_hits.fetch_add(1, Ordering::Relaxed);
    let mut vap = np.n_vattr.get();
    if vap.va_size != np.n_size.get() {
        if vap.va_type == VREG {
            if np.n_flag.get() & NMODIFIED != 0 {
                if vap.va_size < np.n_size.get() {
                    vap.va_size = np.n_size.get();
                } else {
                    np.n_size.set(vap.va_size);
                }
            } else {
                np.n_size.set(vap.va_size);
            }
            np.n_vattr.set(vap);
            uvm_vnp_setsize(vp, np.n_size.get() as Voff);
        } else {
            np.n_size.set(vap.va_size);
        }
    }
    np.n_vattr.set(vap);
    *vaper = vap;
    if np.n_flag.get() & NCHG != 0 {
        if np.n_flag.get() & NACC != 0 {
            vaper.va_atime = np.n_atim.get();
        }
        if np.n_flag.get() & NUPD != 0 {
            vaper.va_mtime = np.n_mtim.get();
        }
    }
    Ok(())
}

/// `nfs_clearcommit(mp)`: the write verifier has changed (probably due to a server reboot),
/// so all `B_NEEDCOMMIT` blocks will have to be written again. Since they are on the dirty
/// block list as `B_DELWRI`, all this takes is clearing the `B_NEEDCOMMIT` flag. Once done
/// the new write verifier can be set for the mount point.
pub fn nfs_clearcommit(mp: &Mount) {
    let s = splbio();
    'restart: loop {
        for vp in mp.mnt_vnodelist.iter() {
            if !vp.v_mount.get().is_some_and(|m| ptr::eq(m, mp)) {
                // Paranoia.
                continue 'restart;
            }
            for bp in vp.v_dirtyblkhd.iter() {
                let flags = bp.b_flags.get();
                if flags & (B_BUSY | B_DELWRI | B_NEEDCOMMIT) == B_DELWRI | B_NEEDCOMMIT {
                    bp.b_flags.set(flags & !B_NEEDCOMMIT);
                }
            }
        }
        break;
    }
    splx(s);
}

/// `nfs_merge_commit_ranges(vp)`: folds the to-be-committed range into the committed one.
pub fn nfs_merge_commit_ranges(vp: &Vnode) {
    let np = VTONFS(vp);

    if np.n_commitflags.get() & NFS_COMMIT_PUSHED_VALID == 0 {
        np.n_pushedlo.set(np.n_pushlo.get());
        np.n_pushedhi.set(np.n_pushhi.get());
        np.n_commitflags
            .set(np.n_commitflags.get() | NFS_COMMIT_PUSHED_VALID);
    } else {
        if np.n_pushlo.get() < np.n_pushedlo.get() {
            np.n_pushedlo.set(np.n_pushlo.get());
        }
        if np.n_pushhi.get() > np.n_pushedhi.get() {
            np.n_pushedhi.set(np.n_pushhi.get());
        }
    }

    np.n_pushlo.set(0);
    np.n_pushhi.set(0);
    np.n_commitflags
        .set(np.n_commitflags.get() & !NFS_COMMIT_PUSH_VALID);
}

/// The byte range of a buffer's dirty data: `b_blkno * DEV_BSIZE` to that plus
/// `b_dirtyend`.
fn commit_range(bp: &Buf) -> (Off, Off) {
    let lo = bp.b_blkno.get() * DEV_BSIZE as Off;
    (lo, lo + Off::from(bp.b_dirtyend.get()))
}

/// `nfs_in_committed_range(vp, bp)`: whether the buffer's dirty data was committed.
pub fn nfs_in_committed_range(vp: &Vnode, bp: &Buf) -> bool {
    let np = VTONFS(vp);

    if np.n_commitflags.get() & NFS_COMMIT_PUSHED_VALID == 0 {
        return false;
    }
    let (lo, hi) = commit_range(bp);

    lo >= np.n_pushedlo.get() && hi <= np.n_pushedhi.get()
}

/// `nfs_in_tobecommitted_range(vp, bp)`: whether the buffer's dirty data is in the range
/// to be committed.
pub fn nfs_in_tobecommitted_range(vp: &Vnode, bp: &Buf) -> bool {
    let np = VTONFS(vp);

    if np.n_commitflags.get() & NFS_COMMIT_PUSH_VALID == 0 {
        return false;
    }
    let (lo, hi) = commit_range(bp);

    lo >= np.n_pushlo.get() && hi <= np.n_pushhi.get()
}

/// `nfs_add_committed_range(vp, bp)`: widens the committed range to the buffer.
pub fn nfs_add_committed_range(vp: &Vnode, bp: &Buf) {
    let np = VTONFS(vp);
    let (lo, hi) = commit_range(bp);

    if np.n_commitflags.get() & NFS_COMMIT_PUSHED_VALID == 0 {
        np.n_pushedlo.set(lo);
        np.n_pushedhi.set(hi);
        np.n_commitflags
            .set(np.n_commitflags.get() | NFS_COMMIT_PUSHED_VALID);
    } else {
        if hi > np.n_pushedhi.get() {
            np.n_pushedhi.set(hi);
        }
        if lo < np.n_pushedlo.get() {
            np.n_pushedlo.set(lo);
        }
    }
}

/// `nfs_del_committed_range(vp, bp)`: takes the buffer out of the committed range.
pub fn nfs_del_committed_range(vp: &Vnode, bp: &Buf) {
    let np = VTONFS(vp);

    if np.n_commitflags.get() & NFS_COMMIT_PUSHED_VALID == 0 {
        return;
    }

    let (lo, hi) = commit_range(bp);

    if lo > np.n_pushedhi.get() || hi < np.n_pushedlo.get() {
        return;
    }
    if lo <= np.n_pushedlo.get() {
        np.n_pushedlo.set(hi);
    } else if hi >= np.n_pushedhi.get() {
        np.n_pushedhi.set(lo);
    } else {
        // XXX There's only one range. If the deleted range is in the middle, pick the
        // largest of the contiguous ranges that it leaves.
        if np.n_pushedlo.get() - lo > hi - np.n_pushedhi.get() {
            np.n_pushedhi.set(lo);
        } else {
            np.n_pushedlo.set(hi);
        }
    }
}

/// `nfs_add_tobecommitted_range(vp, bp)`: widens the range to be committed to the buffer.
pub fn nfs_add_tobecommitted_range(vp: &Vnode, bp: &Buf) {
    let np = VTONFS(vp);
    let (lo, hi) = commit_range(bp);

    if np.n_commitflags.get() & NFS_COMMIT_PUSH_VALID == 0 {
        np.n_pushlo.set(lo);
        np.n_pushhi.set(hi);
        np.n_commitflags
            .set(np.n_commitflags.get() | NFS_COMMIT_PUSH_VALID);
    } else {
        if lo < np.n_pushlo.get() {
            np.n_pushlo.set(lo);
        }
        if hi > np.n_pushhi.get() {
            np.n_pushhi.set(hi);
        }
    }
}

/// `nfs_del_tobecommitted_range(vp, bp)`: takes the buffer out of the range to be
/// committed.
pub fn nfs_del_tobecommitted_range(vp: &Vnode, bp: &Buf) {
    let np = VTONFS(vp);

    if np.n_commitflags.get() & NFS_COMMIT_PUSH_VALID == 0 {
        return;
    }

    let (lo, hi) = commit_range(bp);

    if lo > np.n_pushhi.get() || hi < np.n_pushlo.get() {
        return;
    }

    if lo <= np.n_pushlo.get() {
        np.n_pushlo.set(hi);
    } else if hi >= np.n_pushhi.get() {
        np.n_pushhi.set(lo);
    } else {
        // XXX There's only one range. If the deleted range is in the middle, pick the
        // largest of the contiguous ranges that it leaves.
        if np.n_pushlo.get() - lo > hi - np.n_pushhi.get() {
            np.n_pushhi.set(lo);
        } else {
            np.n_pushlo.set(hi);
        }
    }
}

/// `nfsrv_errmap(nd, err)`: maps an errno to an NFS error number. For version 3 also
/// filters out error numbers not specified for the associated procedure.
pub fn nfsrv_errmap(nd: &NfsrvDescript, err: i32) -> i32 {
    if nd.nd_flag & ND_NFSV3 != 0 {
        if nd.nd_procnum <= NFSPROC_COMMIT {
            let errs = NFSRV_V3ERRMAP[nd.nd_procnum];
            for &e in &errs[1..] {
                if e == 0 {
                    break;
                }
                let e = i32::from(e);
                if e == err {
                    return err;
                } else if e > err {
                    break;
                }
            }
            return i32::from(errs[0]);
        } else {
            return err & 0xffff;
        }
    }
    if err >= 1 && err as usize <= NFSRV_V2ERRMAP.len() {
        return i32::from(NFSRV_V2ERRMAP[err as usize - 1]);
    }
    NFSERR_IO
}

/// `nfsm_v3attrbuild(mp, a, full)`: appends a version 3 `sattr3` built from `a`: if `full`,
/// all fields that are set, otherwise just the mode and time fields.
pub fn nfsm_v3attrbuild(mp: &mut &'static Mbuf, a: &Vattr, full: bool) {
    let mut mb = *mp;

    if a.va_mode != VNOVAL as Mode {
        let mut tl = nfsm_build(&mut mb, 2 * NFSX_UNSIGNED);
        tl.put(nfs_true);
        tl.put(txdr_unsigned(a.va_mode));
    } else {
        nfsm_build(&mut mb, NFSX_UNSIGNED).put(nfs_false);
    }
    if full && a.va_uid != VNOVAL as Uid {
        let mut tl = nfsm_build(&mut mb, 2 * NFSX_UNSIGNED);
        tl.put(nfs_true);
        tl.put(txdr_unsigned(a.va_uid));
    } else {
        nfsm_build(&mut mb, NFSX_UNSIGNED).put(nfs_false);
    }
    if full && a.va_gid != VNOVAL as Gid {
        let mut tl = nfsm_build(&mut mb, 2 * NFSX_UNSIGNED);
        tl.put(nfs_true);
        tl.put(txdr_unsigned(a.va_gid));
    } else {
        nfsm_build(&mut mb, NFSX_UNSIGNED).put(nfs_false);
    }
    if full && a.va_size != VNOVAL as u64 {
        let mut tl = nfsm_build(&mut mb, 3 * NFSX_UNSIGNED);
        tl.put(nfs_true);
        tl.put_hyper(a.va_size);
    } else {
        nfsm_build(&mut mb, NFSX_UNSIGNED).put(nfs_false);
    }
    for t in [&a.va_atime, &a.va_mtime] {
        if t.tv_nsec != i64::from(VNOVAL) {
            if t.tv_sec != gettime() {
                let mut tl = nfsm_build(&mut mb, 3 * NFSX_UNSIGNED);
                tl.put(txdr_unsigned(NFSV3SATTRTIME_TOCLIENT));
                tl.write(NFSX_UNSIGNED, &txdr_nfsv3time(t));
            } else {
                nfsm_build(&mut mb, NFSX_UNSIGNED).put(txdr_unsigned(NFSV3SATTRTIME_TOSERVER));
            }
        } else {
            nfsm_build(&mut mb, NFSX_UNSIGNED).put(txdr_unsigned(NFSV3SATTRTIME_DONTCHANGE));
        }
    }

    *mp = mb;
}

/// `nfsm_build(mp, len)`: ensures a contiguous buffer `len` bytes long at the end of the
/// chain at the build cursor `*mp` (a new mbuf when the last one has no room) and returns it
/// as a view; the cursor moves to the mbuf that holds it.
pub fn nfsm_build<'a>(mp: &'a mut &'static Mbuf, len: usize) -> XdrOut<'a> {
    let mut mb = *mp;
    let mut bpos = mb_offset(mb);

    if len > m_trailingspace(mb) as usize {
        let mb2 = nfsm_mget();
        if len > MLEN {
            panic(format_args!("build > MLEN"));
        }
        mb.m_next().set(Some(mb2));
        mb = mb2;
        mb.m_len().set(0);
        bpos = mtod::<u8>(mb);
    }
    mb.m_len().set(mb.m_len().get() + len as u32);

    *mp = mb;

    // SAFETY: the `len` bytes at `bpos` were `mb`'s trailing space (checked above, or a fresh
    // mbuf of `MLEN >= len` bytes) and are now the end of its data: storage of the chain
    // being built, which the view borrows through the cursor.
    unsafe { XdrOut::new(bpos, len) }
}

/// `nfsm_fhtom(mb, v, v3)`: appends the file handle of the NFS vnode `v` to the request.
pub fn nfsm_fhtom(mb: &mut &'static Mbuf, v: &Vnode, v3: bool) {
    let n = VTONFS(v);

    if v3 {
        nfsm_strtombuf(mb, &n.n_fhp());
    } else {
        nfsm_buftombuf(mb, &n.n_fh.get().fh_bytes[..NFSX_V2FH]);
    }
}

/// `nfsm_srvfhtom(mp, f, v3)`: appends the server's file handle (in its `nfsfh_t`) to the
/// reply.
pub fn nfsm_srvfhtom(mp: &mut &'static Mbuf, f: &Nfsfh, v3: bool) {
    if v3 {
        nfsm_strtombuf(mp, &f.fh_bytes[..NFSX_V3FH]);
    } else {
        nfsm_buftombuf(mp, &f.fh_bytes[..NFSX_V2FH]);
    }
}

/// `txdr_nfsv2time(from, to)`: the version 2 time of `from`; `VNOVAL` nanoseconds are sent
/// as -1, a time of -1 seconds as -1.000001.
pub fn txdr_nfsv2time(from: &Timespec) -> Nfsv2Time {
    if from.tv_nsec == i64::from(VNOVAL) {
        Nfsv2Time {
            nfsv2_sec: nfs_xdrneg1,
            nfsv2_usec: nfs_xdrneg1,
        }
    } else if from.tv_sec == -1 {
        // can't request a time of -1; send -1.000001 == {-2,999999} instead
        Nfsv2Time {
            nfsv2_sec: txdr_unsigned(-2i32 as u32),
            nfsv2_usec: txdr_unsigned(999_999),
        }
    } else {
        Nfsv2Time {
            nfsv2_sec: txdr_unsigned(from.tv_sec as u32),
            nfsv2_usec: txdr_unsigned((from.tv_nsec / 1000) as u32),
        }
    }
}

const _: () = {
    assert!(fxdr_unsigned(nfs_xdrneg1) == u32::MAX);
    assert!(NFSV3_TYPE.len() == 9 && NV3TOV_TYPE.len() == 8);
    assert!(MHLEN <= MLEN);
};

#[cfg(test)]
pub(crate) mod tests;
