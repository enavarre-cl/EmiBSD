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

//! vnode op calls for Sun NFS version 2 and 3: the NFS client's vnode operations
//! (`nfs_vops`), the operations of special devices (`nfs_specvops`) and fifos
//! (`nfs_fifovops`) on an NFS mount, and the RPCs behind them.
//!
//! Upstream: sys/nfs/nfs_vnops.c @ 3ce1f3f79392
//!
//! Each operation builds its request with the `nfsm_*` helpers, sends it with
//! `nfs_request` and dissects the reply; the attribute cache (`nfs_loadattrcache`) and the
//! access cache in the nfsnode avoid RPCs where the C does. Reads and writes go through the
//! buffer cache (`nfs_bio.rs`), which calls back into `nfs_readrpc`, `nfs_writerpc`,
//! `nfs_readlinkrpc` and `nfs_writebp`.
//!
//! ## Deviations
//! - The C's `goto nfsmout` is the inner closure / labeled block of `CONTRACT.md`: the
//!   dissect helpers free the reply on failure, as the C's do; `nmi_errorp` is the `Result`.
//!   `nfsm_loadattr`, `nfsm_wcc_data`, `nfsm_getfh` and `nfsm_mtofh` (the C's `static
//!   inline` helpers) return `Result`; `nfsm_getfh` returns a copy of the handle.
//! - `nfs_request` hands a version 3 status that is no errno (`NFSERR_BADHANDLE` ...
//!   `NFSERR_JUKEBOX`, 10001 and up) up as `EIO` (`nfs_socket.rs`), so the C's tests for
//!   `NFSERR_NOTSUPP` (`nfs_create`'s retry without `O_EXCL`, `nfs_readdir`'s fallback from
//!   READDIRPLUS) and `NFSERR_BAD_COOKIE` (`nfs_readdir`'s `EINVAL`) are kept but cannot
//!   match until it passes those statuses on (`nfs_errno_is`).
//! - `nfs_commit` returns `Result<(), i32>`: the C int, an errno or the fake
//!   `NFSERR_STALEWRITEVERF` (30001), which no `Errno` holds.
//! - `nfs_lookitup(dvp, name, len, cred, procp, npp)` takes the name as a slice and `npp` as
//!   `Option<&mut Option<&NfsNode>>` (NULL, `*npp == NULL`, `*npp != NULL`);
//!   `nfs_removerpc`, `nfs_renamerpc` likewise take names as slices. `struct proc *`
//!   arguments that may be NULL are `Option<&Proc>`; `int *` flags (`must_commit`,
//!   `end_of_directory`) are `&mut bool`; `nfs_writebp`'s and `nfs_flush`'s `int` switches
//!   are `bool`s; the `iomode` words are the `u32` `NFSV3WRITE_*` constants.
//! - The readdir RPCs pack their `struct nfs_dirent` records into the caller's
//!   `UIO_SYSSPACE` buffer through `NfsDirPack` (byte offsets of the C structure, the same
//!   layout), and the uio is advanced over what was packed when the RPC returns, success or
//!   not (the C advances it record by record; both callers drop it on error).
//!   `nfs_readdir` fixes the records up with `nfs_dirent_fixup` before copying them out.
//! - The silly name is formatted by `nfs_sillyname` (`".nfs%08X%08X"`).
//! - `VOP_GETATTR`/`VOP_SETATTR` need a thread: where the C passes a pointer that may be
//!   NULL (`uio_procp`, a close's `a_p`) this file passes it, else `curproc`, else `proc0`.
//! - `option FIFO` is in GENERIC but `miscfs/fifofs` is not ported (the ffs, tmpfs and
//!   cd9660 precedent): `nfs_fifovops` is defined, but `nfs_loadattrcache` takes the C's
//!   `!FIFO` path and never installs it; the `fifo_*` operations it names (`fifo_open`,
//!   `fifo_close`, `fifo_read`, `fifo_write`, `fifo_ioctl`, `fifo_kqfilter`,
//!   `fifo_pathconf`, `fifo_advlock`, `fifo_reclaim`, `fifo_printinfo`) are visible gaps
//!   (`unported!`, `ENOSYS`).
//! - `nfs_numasync` (defined here in C, counted by `nfs_syscalls.c`'s `nfssvc_iod`) is the
//!   atomic `NFS_NUMASYNC`.
//! - `malloc(M_WAITOK)` cannot fail in C; here a failure panics.
//! - `nfs_print` prints under feature `debug` or `diagnostic`; the C's third condition,
//!   `VFSLCKDEBUG`, has no feature here. The `DIAGNOSTIC` checks and `printf`s are behind
//!   feature `diagnostic`.
//! - `nfs_lookitup`'s error path skips the `vput` of a node it never got (a version 3 LOOKUP
//!   that fails with `*npp == NULL`: the C would `vput(NULL)`).
//! - `nfs_lookup` leaks the vnode of a name cache hit when it cannot relock the directory,
//!   as the C does; the C's `#if 0` `vprint` in `nfs_flush` stays a comment.

use core::ffi::c_void;
use core::mem::offset_of;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicI32, Ordering};

use crate::kassert;
use crate::kern::init_main::PROC0;
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_prot::{crdup, crfree};
use crate::kern::kern_rwlock::{rrw_enter, rrw_exit, rrw_status, rw_enter_write, rw_exit_write};
use crate::kern::kern_subr::uiomove;
use crate::kern::kern_synch::tsleep_nsec;
use crate::kern::kern_tc::{getnanotime, gettime};
use crate::kern::spec_vnops::{
    spec_advlock, spec_close, spec_ioctl, spec_kqfilter, spec_open, spec_pathconf, spec_read,
    spec_strategy, spec_write,
};
use crate::kern::subr_pool::pool_put;
use crate::kern::subr_prf::{panic, printf};
use crate::kern::vfs_bio::{BCSTATS, biodone, biowait, brelse, buf_undirty, bufcache_take};
use crate::kern::vfs_biomem::buf_acquire;
use crate::kern::vfs_cache::{cache_enter, cache_lookup, cache_purge};
use crate::kern::vfs_default::{
    vop_generic_abortop, vop_generic_badop, vop_generic_bmap, vop_generic_bwrite,
    vop_generic_lookup, vop_generic_revoke,
};
use crate::kern::vfs_init::NAMEI_POOL;
use crate::kern::vfs_lockf::lf_advlock;
use crate::kern::vfs_subr::{vaccess, vattr_null, vput, vref, vrele, vwaitforio};
use crate::kern::vfs_vnops::vn_lock;
use crate::kern::vfs_vops::{
    VOP_ABORTOP, VOP_ACCESS, VOP_BWRITE, VOP_FSYNC, VOP_GETATTR, VOP_ISLOCKED, VOP_SETATTR,
    VOP_STRATEGY, VOP_UNLOCK,
};
use crate::machine::cpu::curproc;
use crate::machine::intr::{splbio, splx};
use crate::nfs::nfs::{NFS_DIRBLKSIZ, NFS_READDIRBLKSIZ, nfs_cmpfh, nfs_isv3};
use crate::nfs::nfs_bio::{nfs_asyncio, nfs_bioread, nfs_doio, nfs_vinvalbuf, nfs_write};
use crate::nfs::nfs_kq::nfs_kqfilter;
use crate::nfs::nfs_node::{
    nfs_crfree, nfs_crhold, nfs_inactive, nfs_nget, nfs_reclaim, vfstonfs_vp,
};
use crate::nfs::nfs_socket::{nfs_request, nfs_sigintr};
use crate::nfs::nfs_subs::nfs_attrtimeo;
use crate::nfs::nfs_subs::{
    NFSSTATS, nfs_add_committed_range, nfs_clearcommit, nfs_false, nfs_getattrcache,
    nfs_in_committed_range, nfs_in_tobecommitted_range, nfs_loadattrcache, nfs_merge_commit_ranges,
    nfs_true, nfs_xdrneg1, nfsm_build, nfsm_fhtom, nfsm_reqhead, nfsm_uiotombuf, nfsm_v3attrbuild,
    txdr_nfsv2time,
};
use crate::nfs::nfsm_subs::{
    NfsmInfo, nfsm_adv, nfsm_dissect, nfsm_mtouio, nfsm_postop_attr, nfsm_rndup, nfsm_strsiz,
    nfsm_strtom,
};
use crate::nfs::nfsnode::{
    NACC, NCHG, NFSTOV, NMODIFIED, NUPD, NWRITEERR, NfsNode, Sillyrename, VTONFS,
    nfs_invalidate_attrcache,
};
use crate::nfs::nfsproto::{
    NFS_FABLKSIZE, NFS_MAXNAMLEN, NFS_MAXPATHLEN, NFSERR_BAD_COOKIE, NFSERR_IO, NFSERR_NOTSUPP,
    NFSERR_STALEWRITEVERF, NFSPROC_ACCESS, NFSPROC_COMMIT, NFSPROC_CREATE, NFSPROC_GETATTR,
    NFSPROC_LINK, NFSPROC_LOOKUP, NFSPROC_MKDIR, NFSPROC_MKNOD, NFSPROC_READ, NFSPROC_READDIR,
    NFSPROC_READDIRPLUS, NFSPROC_READLINK, NFSPROC_REMOVE, NFSPROC_RENAME, NFSPROC_RMDIR,
    NFSPROC_SETATTR, NFSPROC_SYMLINK, NFSPROC_WRITE, NFSV3ACCESS_DELETE, NFSV3ACCESS_EXECUTE,
    NFSV3ACCESS_EXTEND, NFSV3ACCESS_LOOKUP, NFSV3ACCESS_MODIFY, NFSV3ACCESS_READ,
    NFSV3CREATE_EXCLUSIVE, NFSV3CREATE_UNCHECKED, NFSV3WRITE_DATASYNC, NFSV3WRITE_FILESYNC,
    NFSV3WRITE_UNSTABLE, NFSX_UNSIGNED, NFSX_V2FH, NFSX_V2SATTR, NFSX_V3CREATEVERF, NFSX_V3FATTR,
    NFSX_V3FHMAX, NFSX_V3WRITEVERF, Nfsuint64, Nfsv2Sattr, Nfsv3Time, nfsx_fh, nfsx_readdir,
    nfsx_sattr, vtonfsv2_mode, vtonfsv3_type,
};
use crate::nfs::xdr_subs::{fxdr_hyper, fxdr_nfsv3time, fxdr_unsigned, txdr_hyper, txdr_unsigned};
use crate::sys::buf::{
    B_ASYNC, B_BUSY, B_DELWRI, B_DONE, B_ERROR, B_NEEDCOMMIT, B_PHYS, B_RAW, B_READ, B_WANTED,
    B_WRITEINPROG, Buf,
};
use crate::sys::dirent::{DT_UNKNOWN, Dirent, MAXNAMLEN, dirent_recsize, iftodt};
use crate::sys::errno::Errno;
use crate::sys::event::{NOTE_ATTRIB, NOTE_DELETE, NOTE_LINK, NOTE_TRUNCATE, NOTE_WRITE};
use crate::sys::fcntl::{FREAD, FWRITE, O_EXCL};
use crate::sys::lock::{LK_EXCLUSIVE, LK_RETRY, LK_RWFLAGS};
use crate::sys::malloc::{M_NFSREQ, M_TEMP, M_WAITOK, M_ZERO};
use crate::sys::mount::{
    MNT_RDONLY, MNT_WAIT, NFSMNT_GOTFSINFO, NFSMNT_HASWRITEVERF, NFSMNT_INT, NFSMNT_NFSV3,
    NFSMNT_RDIRPLUS,
};
#[cfg(feature = "diagnostic")]
use crate::sys::namei::HASBUF;
use crate::sys::namei::{
    CREATE, Componentname, DELETE, ISDOTDOT, ISLASTCN, LOCKPARENT, LOOKUP, MAKEENTRY,
    NAMECACHE_MAXLEN, PDIRUNLOCK, RENAME, SAVENAME, WANTPARENT,
};
use crate::sys::param::{DEV_BSIZE, MAXPATHLEN, PAGE_SIZE, PCATCH, PRIBIO, btodb};
use crate::sys::proc::Proc;
use crate::sys::syslimits::{LINK_MAX, NAME_MAX};
use crate::sys::systm::INFSLP;
use crate::sys::time::sec_to_nsec;
use crate::sys::types::{Daddr, Off, Register, Time, Uid, major, minor};
use crate::sys::ucred::{FSCRED, NOCRED, Ucred};
use crate::sys::uio::{Iovec, Uio, UioRw, UioSeg};
use crate::sys::unistd::{
    _PC_2_SYMLINKS, _PC_ALLOC_SIZE_MIN, _PC_CHOWN_RESTRICTED, _PC_FILESIZEBITS, _PC_LINK_MAX,
    _PC_NAME_MAX, _PC_NO_TRUNC, _PC_REC_INCR_XFER_SIZE, _PC_REC_MAX_XFER_SIZE,
    _PC_REC_MIN_XFER_SIZE, _PC_REC_XFER_ALIGN, _PC_SYMLINK_MAX, _PC_TIMESTAMP_RESOLUTION,
};
use crate::sys::vnode::{
    V_SAVE, VA_EXCLUSIVE, VBAD, VBLK, VCHR, VDIR, VEXEC, VFIFO, VLNK, VN_KNOTE, VNOVAL, VREAD,
    VREG, VSOCK, VWRITE, Vattr, Vnode, VopAccessArgs, VopAdvlockArgs, VopBmapArgs, VopBwriteArgs,
    VopCloseArgs, VopCreateArgs, VopFsyncArgs, VopGetattrArgs, VopIoctlArgs, VopIslockedArgs,
    VopLinkArgs, VopLockArgs, VopLookupArgs, VopMkdirArgs, VopMknodArgs, VopOpenArgs,
    VopPathconfArgs, VopPrintArgs, VopReadArgs, VopReaddirArgs, VopReadlinkArgs, VopReclaimArgs,
    VopRemoveArgs, VopRenameArgs, VopRmdirArgs, VopSetattrArgs, VopStrategyArgs, VopSymlinkArgs,
    VopUnlockArgs, VopWriteArgs, Vops, Vtype, cred_ref, vttoif,
};
use crate::unported;
use crate::uvm::uvm_vnode::{uvm_vnp_setsize, uvm_vnp_uncache};

use crate::nfs::nfs_vfsops::nfs_fsinfo;

/// `if (... != 0) goto nfsmout;`: the value of a `Result`, or a `break` out of the labeled
/// block `$l` (the C's `nfsmout:`) with its error.
macro_rules! nfsm_try {
    ($l:lifetime, $e:expr) => {
        match $e {
            Ok(v) => v,
            Err(e) => break $l Err(e),
        }
    };
}

/// `NFSV3_WCCRATTR`: `*flagp` of `nfsm_wcc_data`: return the post-op attribute flag.
const NFSV3_WCCRATTR: i32 = 0;
/// `NFSV3_WCCCHK`: `*flagp` of `nfsm_wcc_data`: return whether the pre-op mtime changed.
const NFSV3_WCCCHK: i32 = 1;

/// `NFS_COMMITBVECSIZ`: the most buffers one `nfs_flush` commit pass gathers.
const NFS_COMMITBVECSIZ: usize = 20;

/// `struct nfs_dirent`: a `struct dirent` as the readdir RPCs pack it, preceded by the NFS
/// cookie of the entry (two raw XDR words). Only its layout is used: the records are written
/// and read at these offsets in byte buffers.
#[repr(C)]
#[allow(dead_code)] // a layout: the members are reached through `offset_of!`
struct NfsDirent {
    cookie: [u32; 2],
    dirent: Dirent,
}

/// `NFS_DIRHDSIZ`: the bytes of an `nfs_dirent` before the name.
const NFS_DIRHDSIZ: usize = size_of::<NfsDirent>() - (MAXNAMLEN + 1);
/// `NFS_DIRENT_OVERHEAD`: the bytes of an `nfs_dirent` before its `struct dirent`.
const NFS_DIRENT_OVERHEAD: usize = offset_of!(NfsDirent, dirent);

/// Offsets of the members of a packed `nfs_dirent`.
const ND_COOKIE: usize = offset_of!(NfsDirent, cookie);
const ND_FILENO: usize = NFS_DIRENT_OVERHEAD + offset_of!(Dirent, d_fileno);
const ND_OFF: usize = NFS_DIRENT_OVERHEAD + offset_of!(Dirent, d_off);
const ND_RECLEN: usize = NFS_DIRENT_OVERHEAD + offset_of!(Dirent, d_reclen);
const ND_TYPE: usize = NFS_DIRENT_OVERHEAD + offset_of!(Dirent, d_type);
const ND_NAMLEN: usize = NFS_DIRENT_OVERHEAD + offset_of!(Dirent, d_namlen);
const ND_NAME: usize = NFS_DIRENT_OVERHEAD + offset_of!(Dirent, d_name);

/// A file handle dissected from a reply (`nfsm_getfh`): a copy of its `size` bytes.
struct NfsFh {
    fh: [u8; NFSX_V3FHMAX],
    size: usize,
}

impl NfsFh {
    /// The handle's bytes.
    fn bytes(&self) -> &[u8] {
        &self.fh[..self.size]
    }
}

/// The packing of `struct nfs_dirent` records into the buffer of a readdir RPC's uio: the
/// C's `uiop` cursor (`pos`), `blksiz` and `dp` (the last record, an offset).
struct NfsDirPack<'b> {
    /// The uio's buffer, from where the RPC started.
    buf: &'b mut [u8],
    /// The bytes packed (the C's `uio_iov->iov_base` advance).
    pos: usize,
    /// `blksiz`: the bytes used in the current `NFS_READDIRBLKSIZ` block.
    blksiz: usize,
    /// `dp`: the offset of the last record, if any.
    dp: Option<usize>,
    /// `tlen` of the record being packed, less its header.
    tail: usize,
}

impl<'b> NfsDirPack<'b> {
    /// An empty packing into `buf`.
    fn new(buf: &'b mut [u8]) -> Self {
        Self {
            buf,
            pos: 0,
            blksiz: 0,
            dp: None,
            tail: 0,
        }
    }

    /// `uiop->uio_resid`: the room left.
    fn resid(&self) -> usize {
        self.buf.len() - self.pos
    }

    /// `dp->d_reclen += n` of the last record.
    fn grow_last(&mut self, n: usize) {
        if let Some(dp) = self.dp {
            let r = self.get_u16(dp + ND_RECLEN).wrapping_add(n as u16);
            self.put(dp + ND_RECLEN, &r.to_ne_bytes());
        }
    }

    /// Skip `n` bytes (`iov_base += n; iov_len -= n; uio_resid -= n`).
    fn skip(&mut self, n: usize) {
        self.pos += n.min(self.resid());
    }

    fn put(&mut self, off: usize, b: &[u8]) {
        self.buf[off..off + b.len()].copy_from_slice(b);
    }

    fn get_u16(&self, off: usize) -> u16 {
        u16::from_ne_bytes([self.buf[off], self.buf[off + 1]])
    }

    /// Start the record of an entry `fileno` with a name of `len` bytes: when the record
    /// does not fit in the current `NFS_READDIRBLKSIZ` block, the last record grows to fill
    /// the block; `false` (the C's `bigenough = 0`) when the buffer has no room for it.
    /// Otherwise the header is written and the cursor stands at the name.
    fn begin(&mut self, fileno: u64, len: usize) -> bool {
        let tlen = dirent_recsize(len) + NFS_DIRENT_OVERHEAD;
        let left = NFS_READDIRBLKSIZ as usize - self.blksiz;
        if tlen > left {
            self.grow_last(left);
            self.skip(left);
            self.blksiz = 0;
        }
        if tlen > self.resid() {
            return false;
        }
        let dp = self.pos;
        self.put(dp + ND_FILENO, &fileno.to_ne_bytes());
        self.buf[dp + ND_NAMLEN] = len as u8;
        self.put(dp + ND_RECLEN, &(tlen as u16).to_ne_bytes());
        self.buf[dp + ND_TYPE] = DT_UNKNOWN;
        self.dp = Some(dp);
        self.blksiz += tlen;
        if self.blksiz == NFS_READDIRBLKSIZ as usize {
            self.blksiz = 0;
        }
        self.pos += NFS_DIRHDSIZ;
        self.tail = tlen - NFS_DIRHDSIZ;
        true
    }

    /// The slot of the name of the record begun (`len` bytes at the cursor).
    fn name_slot(&mut self, len: usize) -> &mut [u8] {
        &mut self.buf[self.pos..self.pos + len]
    }

    /// The name of `len` bytes is in place: NUL-terminate it and skip to the end of the
    /// record.
    fn end_name(&mut self, len: usize) {
        self.buf[self.pos + len] = 0;
        self.pos += self.tail;
    }

    /// `ndp->cookie[0] = c0; ndp->cookie[1] = c1` of the last record (raw XDR words).
    fn set_cookie(&mut self, c0: u32, c1: u32) {
        if let Some(dp) = self.dp {
            self.put(dp + ND_COOKIE, &c0.to_ne_bytes());
            self.put(dp + ND_COOKIE + 4, &c1.to_ne_bytes());
        }
    }

    /// `dp->d_type = t` of the last record.
    fn set_type(&mut self, t: u8) {
        if let Some(dp) = self.dp {
            self.buf[dp + ND_TYPE] = t;
        }
    }

    /// Fill the last record, iff any, out to a multiple of `NFS_READDIRBLKSIZ` by increasing
    /// its `d_reclen`.
    fn finish(&mut self) {
        if self.blksiz > 0 {
            let left = NFS_READDIRBLKSIZ as usize - self.blksiz;
            self.grow_last(left);
            self.skip(left);
        }
    }
}

/// `nfs_numasync`: the number of nfsiods running (`nfs_syscalls.c`'s `nfssvc_iod` counts
/// them; `nfs_bio.rs` hands I/O to them while it is not 0).
pub static NFS_NUMASYNC: AtomicI32 = AtomicI32::new(0);

/// `nfs_vops`: global vfs data structures for nfs.
pub static NFS_VOPS: Vops = Vops {
    vop_lookup: Some(nfs_lookup),
    vop_create: Some(nfs_create),
    vop_mknod: Some(nfs_mknod),
    vop_open: Some(nfs_open),
    vop_close: Some(nfs_close),
    vop_access: Some(nfs_access),
    vop_getattr: Some(nfs_getattr),
    vop_setattr: Some(nfs_setattr),
    vop_read: Some(nfs_read),
    vop_write: Some(nfs_write),
    vop_ioctl: Some(nfs_ioctl),
    vop_kqfilter: Some(nfs_kqfilter),
    vop_revoke: Some(vop_generic_revoke),
    vop_fsync: Some(nfs_fsync),
    vop_remove: Some(nfs_remove),
    vop_link: Some(nfs_link),
    vop_rename: Some(nfs_rename),
    vop_mkdir: Some(nfs_mkdir),
    vop_rmdir: Some(nfs_rmdir),
    vop_symlink: Some(nfs_symlink),
    vop_readdir: Some(nfs_readdir),
    vop_readlink: Some(nfs_readlink),
    vop_abortop: Some(vop_generic_abortop),
    vop_inactive: Some(nfs_inactive),
    vop_reclaim: Some(nfs_reclaim),
    vop_lock: Some(nfs_lock),
    vop_unlock: Some(nfs_unlock),
    vop_bmap: Some(nfs_bmap),
    vop_strategy: Some(nfs_strategy),
    vop_print: Some(nfs_print),
    vop_islocked: Some(nfs_islocked),
    vop_pathconf: Some(nfs_pathconf),
    vop_advlock: Some(nfs_advlock),
    vop_bwrite: Some(nfs_bwrite),
};

/// `nfs_specvops`: special device vnode ops.
pub static NFS_SPECVOPS: Vops = Vops {
    vop_close: Some(nfsspec_close),
    vop_access: Some(nfsspec_access),
    vop_getattr: Some(nfs_getattr),
    vop_setattr: Some(nfs_setattr),
    vop_read: Some(nfsspec_read),
    vop_write: Some(nfsspec_write),
    vop_fsync: Some(nfs_fsync),
    vop_inactive: Some(nfs_inactive),
    vop_reclaim: Some(nfs_reclaim),
    vop_lock: Some(nfs_lock),
    vop_unlock: Some(nfs_unlock),
    vop_print: Some(nfs_print),
    vop_islocked: Some(nfs_islocked),

    // XXX: Keep in sync with spec_vops.
    vop_lookup: Some(vop_generic_lookup),
    vop_create: Some(|_| vop_generic_badop()),
    vop_mknod: Some(|_| vop_generic_badop()),
    vop_open: Some(spec_open),
    vop_ioctl: Some(spec_ioctl),
    vop_kqfilter: Some(spec_kqfilter),
    vop_revoke: Some(vop_generic_revoke),
    vop_remove: Some(|_| vop_generic_badop()),
    vop_link: Some(|_| vop_generic_badop()),
    vop_rename: Some(|_| vop_generic_badop()),
    vop_mkdir: Some(|_| vop_generic_badop()),
    vop_rmdir: Some(|_| vop_generic_badop()),
    vop_symlink: Some(|_| vop_generic_badop()),
    vop_readdir: Some(|_| vop_generic_badop()),
    vop_readlink: Some(|_| vop_generic_badop()),
    vop_abortop: Some(|_| vop_generic_badop()),
    vop_bmap: Some(vop_generic_bmap),
    vop_strategy: Some(spec_strategy),
    vop_pathconf: Some(spec_pathconf),
    vop_advlock: Some(spec_advlock),
    vop_bwrite: Some(vop_generic_bwrite),
};

/// `nfs_fifovops`: fifo vnode ops (`option FIFO`; `miscfs/fifofs` is not ported, see the
/// module doc).
pub static NFS_FIFOVOPS: Vops = Vops {
    vop_close: Some(nfsfifo_close),
    vop_access: Some(nfsspec_access),
    vop_getattr: Some(nfs_getattr),
    vop_setattr: Some(nfs_setattr),
    vop_read: Some(nfsfifo_read),
    vop_write: Some(nfsfifo_write),
    vop_fsync: Some(nfs_fsync),
    vop_inactive: Some(nfs_inactive),
    vop_reclaim: Some(nfsfifo_reclaim),
    vop_lock: Some(nfs_lock),
    vop_unlock: Some(nfs_unlock),
    vop_print: Some(nfs_print),
    vop_islocked: Some(nfs_islocked),
    vop_bwrite: Some(vop_generic_bwrite),

    // XXX: Keep in sync with fifo_vops.
    vop_lookup: Some(vop_generic_lookup),
    vop_create: Some(|_| vop_generic_badop()),
    vop_mknod: Some(|_| vop_generic_badop()),
    vop_open: Some(|_| Err(unported!("fifo_open (miscfs/fifofs)"))),
    vop_ioctl: Some(|_| Err(unported!("fifo_ioctl (miscfs/fifofs)"))),
    vop_kqfilter: Some(|_| Err(unported!("fifo_kqfilter (miscfs/fifofs)"))),
    vop_revoke: Some(vop_generic_revoke),
    vop_remove: Some(|_| vop_generic_badop()),
    vop_link: Some(|_| vop_generic_badop()),
    vop_rename: Some(|_| vop_generic_badop()),
    vop_mkdir: Some(|_| vop_generic_badop()),
    vop_rmdir: Some(|_| vop_generic_badop()),
    vop_symlink: Some(|_| vop_generic_badop()),
    vop_readdir: Some(|_| vop_generic_badop()),
    vop_readlink: Some(|_| vop_generic_badop()),
    vop_abortop: Some(|_| vop_generic_badop()),
    vop_bmap: Some(vop_generic_bmap),
    vop_strategy: Some(|_| vop_generic_badop()),
    vop_pathconf: Some(|_| Err(unported!("fifo_pathconf (miscfs/fifofs)"))),
    vop_advlock: Some(|_| Err(unported!("fifo_advlock (miscfs/fifofs)"))),
};

/// `*cred` of a credential the C dereferences: a real one (`NOCRED`/`FSCRED` panic).
fn ucred<'a>(cred: *const Ucred) -> &'a Ucred {
    // SAFETY: the credentials an NFS operation receives are held by its caller for the
    // operation's duration (`cred_ref`'s contract).
    match unsafe { cred_ref(cred) } {
        Some(c) => c,
        None => panic(format_args!(
            "nfs: {} credential",
            if ptr::eq(cred, NOCRED) {
                "missing"
            } else if ptr::eq(cred, FSCRED) {
                "kernel"
            } else {
                "NULL"
            }
        )),
    }
}

/// The thread a `VOP_GETATTR`/`VOP_SETATTR` runs for where the C passes a pointer that may
/// be NULL: `p`, else `curproc`, else `proc0`.
fn nfs_proc(p: Option<&Proc>) -> &Proc {
    match p {
        Some(p) => p,
        None => curproc().unwrap_or(&PROC0),
    }
}

/// `cnp->cn_proc`, `None` when the component name has no thread. Not tied to the borrow of
/// `cnp`, whose flags the operations keep changing while the thread is in use.
fn cn_proc<'a>(cnp: &Componentname) -> Option<&'a Proc> {
    // SAFETY: `cn_proc` is NULL or the thread doing the lookup (`ndinitat`), which outlives
    // the vnode operation it is handed to.
    unsafe { cnp.cn_proc.as_ref() }
}

/// `pool_put(&namei_pool, cnp->cn_pnbuf)`: give back the pathname buffer.
fn pnbuf_free(cnp: &Componentname) {
    if let Some(buf) = NonNull::new(cnp.cn_pnbuf) {
        pool_put(&NAMEI_POOL, buf);
    }
}

/// `vp->v_mount->mnt_flag`.
fn mnt_flag(vp: &Vnode) -> i32 {
    match vp.v_mount.get() {
        Some(mp) => mp.mnt_flag.get(),
        None => panic(format_args!("nfs: vnode {:p} has no mount", vp)),
    }
}

/// `KASSERT(cnp->cn_proc == curproc)`.
fn assert_cn_curproc(cnp: &Componentname) {
    kassert!(curproc().is_some_and(|p| ptr::eq(p, cnp.cn_proc)));
}

/// Whether an error of `nfs_request` is the NFS status `nfserr` (the C's `error ==
/// NFSERR_*`): only statuses that are errno numbers come through (see the module doc).
fn nfs_errno_is(e: Errno, nfserr: i32) -> bool {
    e.as_i32() == nfserr
}

/// `vput(vp)` of a looked-up vnode, `vrele` when it is the directory itself.
fn vput_or_vrele(vp: &'static Vnode, dvp: &'static Vnode) {
    if ptr::eq(vp, dvp) {
        let _ = vrele(vp);
    } else {
        vput(vp);
    }
}

/// `nfs_cache_enter(dvp, vp, cnp)`: enter a name in the cache, remembering in the node the
/// change time (positive entry) or the directory's modification time (negative entry) it
/// was entered at.
pub fn nfs_cache_enter(dvp: &'static Vnode, vp: Option<&'static Vnode>, cnp: &Componentname) {
    match vp {
        Some(vp) => {
            let np = VTONFS(vp);
            np.n_ctime.set(np.n_vattr.get().va_ctime.tv_sec);
        }
        None => {
            let np = VTONFS(dvp);
            if np.n_ctime.get() == 0 {
                np.n_ctime.set(np.n_vattr.get().va_mtime.tv_sec);
            }
        }
    }

    cache_enter(dvp, vp, cnp);
}

/// Whether the access cache of `np` holds an entry for `uid` younger than `timeo` seconds at
/// `now` (`nfs_access`'s `cachevalid`).
fn nfs_access_cachevalid(np: &NfsNode, uid: Uid, now: Time, timeo: i32) -> bool {
    np.n_accstamp.get() != -1
        && now - np.n_accstamp.get() < Time::from(timeo)
        && np.n_accuid.get() == uid
}

/// The answer of a valid access cache entry for `mode`, if it has one: a success covers the
/// modes granted, a failure the modes refused.
fn nfs_access_cached(np: &NfsNode, mode: i32) -> Option<i32> {
    let accmode = np.n_accmode.get();
    let accerror = np.n_accerror.get();
    if accerror == 0 {
        if accmode & mode == mode {
            return Some(accerror);
        }
    } else if accmode & mode == accmode {
        return Some(accerror);
    }
    None
}

/// Record the result `error` (an errno number, 0 for success) of an access check of `mode`
/// for `uid` at `now`: if it is the same result as for a previous, different request, OR it
/// in, without updating the timestamp.
fn nfs_access_update(np: &NfsNode, cachevalid: bool, mode: i32, uid: Uid, error: i32, now: Time) {
    if error == 0 || error == Errno::EACCES.as_i32() {
        if cachevalid && np.n_accstamp.get() != -1 && error == np.n_accerror.get() {
            if error == 0 {
                np.n_accmode.set(np.n_accmode.get() | mode);
            } else if np.n_accmode.get() & mode == mode {
                np.n_accmode.set(mode);
            }
        } else {
            np.n_accstamp.set(now);
            np.n_accuid.set(uid);
            np.n_accmode.set(mode);
            np.n_accerror.set(error);
        }
    }
}

/// The `NFSV3ACCESS_*` bits asked for a `VREAD`/`VWRITE`/`VEXEC` check of a vnode of type
/// `vtype`.
fn nfs_access_mode(vtype: Vtype, a_mode: i32) -> u32 {
    let mut mode = if a_mode & VREAD != 0 {
        NFSV3ACCESS_READ
    } else {
        0
    };
    if vtype == VDIR {
        if a_mode & VWRITE != 0 {
            mode |= NFSV3ACCESS_MODIFY | NFSV3ACCESS_EXTEND | NFSV3ACCESS_DELETE;
        }
        if a_mode & VEXEC != 0 {
            mode |= NFSV3ACCESS_LOOKUP;
        }
    } else {
        if a_mode & VWRITE != 0 {
            mode |= NFSV3ACCESS_MODIFY | NFSV3ACCESS_EXTEND;
        }
        if a_mode & VEXEC != 0 {
            mode |= NFSV3ACCESS_EXECUTE;
        }
    }
    mode
}

/// `nfs_access` (`vop_access`): for nfs version 2, just return ok. File accesses may fail
/// later. For nfs version 3, use the access rpc to check accessibility. If file modes are
/// changed on the server, accesses might still fail later.
pub fn nfs_access(ap: &mut VopAccessArgs<'_>) -> Result<(), Errno> {
    let mut vp = ap.a_vp;
    let v3 = nfs_isv3(vp);
    let np = VTONFS(vp);

    // Disallow write attempts on filesystems mounted read-only; unless the file is a socket,
    // fifo, or a block or character device resident on the filesystem.
    if ap.a_mode & VWRITE != 0 && mnt_flag(vp) & MNT_RDONLY != 0 {
        match vp.v_type.get() {
            VREG | VDIR | VLNK => return Err(Errno::EROFS),
            _ => {}
        }
    }

    // Check access cache first. If a request has been made for this uid shortly before, use
    // the cached result.
    let uid = ucred(ap.a_cred).cr_uid.get();
    let cachevalid = nfs_access_cachevalid(np, uid, gettime(), nfs_attrtimeo(np));

    if cachevalid && let Some(e) = nfs_access_cached(np, ap.a_mode) {
        return errno_result(e);
    }

    // For nfs v3, do an access rpc, otherwise you are stuck emulating ufs_access() locally
    // using the vattr. This may not be correct, since the server may apply other access
    // criteria such as client uid-->server uid mapping that we do not know about, but this
    // is better than just returning anything that is lying about in the cache.
    if !v3 {
        return nfsspec_access(ap);
    }

    NFSSTATS.rpccnt[NFSPROC_ACCESS].fetch_add(1, Ordering::Relaxed);
    let mut info = NfsmInfo::new();
    info.nmi_v3 = v3;
    let req = nfsm_reqhead(nfsx_fh(v3) + NFSX_UNSIGNED);
    info.nmi_mreq = Some(req);
    let mut mb = req;
    nfsm_fhtom(&mut mb, vp, v3);
    let mode = nfs_access_mode(vp.v_type.get(), ap.a_mode);
    nfsm_build(&mut mb, NFSX_UNSIGNED).set(0, txdr_unsigned(mode));

    info.nmi_procp = Some(ap.a_p);
    info.nmi_cred = ap.a_cred;
    // The verdict of the server (0 or EACCES), or the RPC's failure (the C's `goto nfsmout`,
    // which skips the cache update).
    let r = (|| -> Result<i32, Errno> {
        let error = nfs_request(vp, NFSPROC_ACCESS, &mut info);

        let mut attrflag = 0;
        nfsm_postop_attr(&mut info, &mut vp, &mut attrflag)?;
        if let Err(e) = error {
            m_freem_reply(&mut info);
            return Err(e);
        }

        let rmode = fxdr_unsigned(nfsm_dissect(&mut info, NFSX_UNSIGNED)?.get(0));
        // The NFS V3 spec does not clarify whether or not the returned access bits can be
        // a superset of the ones requested, so...
        let error = if rmode & mode != mode {
            Errno::EACCES.as_i32()
        } else {
            0
        };

        m_freem_reply(&mut info);
        Ok(error)
    })();
    let error = r?;

    // If we got the same result as for a previous, different request, OR it in. Don't
    // update the timestamp in that case.
    nfs_access_update(np, cachevalid, ap.a_mode, uid, error, gettime());
    // nfsmout:
    errno_result(error)
}

/// An errno number as a result: 0 is success.
fn errno_result(e: i32) -> Result<(), Errno> {
    if e == 0 {
        Ok(())
    } else {
        Err(Errno::from_raw(e).unwrap_or(Errno::EIO))
    }
}

/// `m_freem(info.nmi_mrep)` at the end of an RPC (the reply is gone afterwards).
fn m_freem_reply(info: &mut NfsmInfo<'_>) {
    crate::kern::uipc_mbuf::m_freem(info.nmi_mrep.take());
}

/// `nfs_open` (`vop_open`): check to see if the type is ok and that deletion is not in
/// progress. For paged in text files, you will need to flush the page cache if consistency
/// is lost.
pub fn nfs_open(ap: &mut VopOpenArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let np = VTONFS(vp);
    let mut vattr = Vattr::new();

    let vt = vp.v_type.get();
    if vt != VREG && vt != VDIR && vt != VLNK {
        #[cfg(feature = "diagnostic")]
        printf(format_args!("open eacces vtyp={}\n", vt as i32));
        return Err(Errno::EACCES);
    }

    // Initialize read and write creds here, for swapfiles and other paths that don't set the
    // creds themselves.
    if ap.a_mode & FREAD != 0 {
        nfs_crfree(np.n_rcred.get());
        np.n_rcred.set(ap.a_cred);
        nfs_crhold(np.n_rcred.get());
    }
    if ap.a_mode & FWRITE != 0 {
        nfs_crfree(np.n_wcred.get());
        np.n_wcred.set(ap.a_cred);
        nfs_crhold(np.n_wcred.get());
    }

    if np.isset(NMODIFIED) {
        let error = nfs_vinvalbuf(vp, V_SAVE, ap.a_cred, Some(ap.a_p));
        if error == Err(Errno::EINTR) {
            return error;
        }
        let _ = uvm_vnp_uncache(vp);
        nfs_invalidate_attrcache(np);
        if vp.v_type.get() == VDIR {
            np.n_direofoffset.set(0);
        }
        VOP_GETATTR(vp, &mut vattr, ap.a_cred, ap.a_p)?;
        np.n_mtime.set(vattr.va_mtime);
    } else {
        VOP_GETATTR(vp, &mut vattr, ap.a_cred, ap.a_p)?;
        if np.n_mtime.get() != vattr.va_mtime {
            if vp.v_type.get() == VDIR {
                np.n_direofoffset.set(0);
            }
            let error = nfs_vinvalbuf(vp, V_SAVE, ap.a_cred, Some(ap.a_p));
            if error == Err(Errno::EINTR) {
                return error;
            }
            let _ = uvm_vnp_uncache(vp);
            np.n_mtime.set(vattr.va_mtime);
        }
    }
    // For open/close consistency.
    nfs_invalidate_attrcache(np);
    Ok(())
}

/// `nfs_close` (`vop_close`). What an NFS client should do upon close after writing is a
/// debatable issue. Most NFS clients push delayed writes to the server upon close, basically
/// for two reasons:
/// 1. So that any write errors may be reported back to the client process doing the close
///    system call. By far the two most likely errors are `NFSERR_NOSPC` and `NFSERR_DQUOT`
///    to indicate space allocation failure.
/// 2. To put a worst case upper bound on cache inconsistency between multiple clients for
///    the file.
///
/// There is also a consistency problem for Version 2 of the protocol w.r.t. not being able
/// to tell if other clients are writing a file concurrently, since there is no way of
/// knowing if the changed modify time in the reply is only due to the write for this client.
/// (NFS Version 3 provides weak cache consistency data in the reply that should be
/// sufficient to detect and handle this case.)
///
/// The current code does the following: for NFS Version 2 - play it safe and
/// flush/invalidate all dirty buffers; for NFS Version 3 - flush dirty buffers to the server
/// but don't invalidate or commit them (this satisfies 1 and 2 except for the case where the
/// server crashes after this close but before the commit RPC, which is felt to be "good
/// enough". Changing the last argument to `nfs_flush()` to a 1 would force a commit
/// operation, if it is felt a commit is necessary now.
pub fn nfs_close(ap: &mut VopCloseArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let np = VTONFS(vp);
    let mut error = Ok(());

    if vp.v_type.get() == VREG {
        if np.isset(NMODIFIED) {
            if nfs_isv3(vp) {
                error = nfs_flush(vp, ap.a_cred, MNT_WAIT, ap.a_p, false);
                np.clr(NMODIFIED);
            } else {
                error = nfs_vinvalbuf(vp, V_SAVE, ap.a_cred, ap.a_p);
            }
            nfs_invalidate_attrcache(np);
        }
        if np.isset(NWRITEERR) {
            np.clr(NWRITEERR);
            error = errno_result(np.n_error.get());
        }
    }
    error
}

/// `nfsm_loadattr(infop, vpp, vap)`: the attributes of a reply loaded into the attribute
/// cache of `*vpp` (which may be replaced by an alias), and copied to `vap` when given; the
/// reply is freed on failure.
fn nfsm_loadattr(
    infop: &mut NfsmInfo<'_>,
    vpp: &mut &'static Vnode,
    vap: Option<&mut Vattr>,
) -> Result<(), Errno> {
    let mut ttvp = *vpp;

    if let Err(e) = nfs_loadattrcache(&mut ttvp, &mut infop.nmi_md, &mut infop.nmi_dpos, vap) {
        m_freem_reply(infop);
        return Err(e);
    }
    *vpp = ttvp;
    Ok(())
}

/// `nfs_getattr` (`vop_getattr`): nfs getattr call from vfs.
pub fn nfs_getattr(ap: &mut VopGetattrArgs<'_>) -> Result<(), Errno> {
    let mut vp = ap.a_vp;
    let np = VTONFS(vp);
    let mut info = NfsmInfo::new();

    info.nmi_v3 = nfs_isv3(vp);

    // Update local times for special files.
    if np.isset(NACC | NUPD) {
        np.set(NCHG);
    }
    // First look in the cache.
    if nfs_getattrcache(vp, ap.a_vap).is_ok() {
        return Ok(());
    }

    NFSSTATS.rpccnt[NFSPROC_GETATTR].fetch_add(1, Ordering::Relaxed);
    let req = nfsm_reqhead(nfsx_fh(info.nmi_v3));
    info.nmi_mreq = Some(req);
    let mut mb = req;
    nfsm_fhtom(&mut mb, vp, info.nmi_v3);
    info.nmi_procp = Some(ap.a_p);
    info.nmi_cred = ap.a_cred;
    let error = nfs_request(vp, NFSPROC_GETATTR, &mut info);
    if error.is_ok() {
        nfsm_loadattr(&mut info, &mut vp, Some(ap.a_vap))?;
    }
    m_freem_reply(&mut info);
    error
}

/// `nfs_setattr` (`vop_setattr`): nfs setattr call.
pub fn nfs_setattr(ap: &mut VopSetattrArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let np = VTONFS(vp);
    let vap = &mut *ap.a_vap;
    let mut hint = NOTE_ATTRIB;
    let mut tsize: u64 = 0;

    // Setting of flags is not supported.
    if vap.va_flags != VNOVAL as u64 {
        return Err(Errno::EOPNOTSUPP);
    }

    // Disallow write attempts if the filesystem is mounted read-only.
    if (vap.va_uid != VNOVAL as Uid
        || vap.va_gid != VNOVAL as u32
        || vap.va_atime.tv_nsec != i64::from(VNOVAL)
        || vap.va_mtime.tv_nsec != i64::from(VNOVAL)
        || vap.va_mode != VNOVAL as u32)
        && mnt_flag(vp) & MNT_RDONLY != 0
    {
        return Err(Errno::EROFS);
    }
    if vap.va_size != VNOVAL as u64 {
        match vp.v_type.get() {
            VDIR => return Err(Errno::EISDIR),
            VCHR | VBLK | VSOCK | VFIFO => {
                if vap.va_mtime.tv_nsec == i64::from(VNOVAL)
                    && vap.va_atime.tv_nsec == i64::from(VNOVAL)
                    && vap.va_mode == VNOVAL as u32
                    && vap.va_uid == VNOVAL as Uid
                    && vap.va_gid == VNOVAL as u32
                {
                    return Ok(());
                }
                vap.va_size = VNOVAL as u64;
            }
            _ => {
                // Disallow write attempts if the filesystem is mounted read-only.
                if mnt_flag(vp) & MNT_RDONLY != 0 {
                    return Err(Errno::EROFS);
                }
                if vap.va_size == 0 {
                    nfs_vinvalbuf(vp, 0, ap.a_cred, Some(ap.a_p))?;
                } else {
                    nfs_vinvalbuf(vp, V_SAVE, ap.a_cred, Some(ap.a_p))?;
                }
                tsize = np.n_size.get();
                np.n_size.set(vap.va_size);
                let mut va = np.n_vattr.get();
                va.va_size = vap.va_size;
                np.n_vattr.set(va);
                uvm_vnp_setsize(vp, np.n_size.get() as Off);
            }
        }
    } else if (vap.va_mtime.tv_nsec != i64::from(VNOVAL)
        || vap.va_atime.tv_nsec != i64::from(VNOVAL))
        && vp.v_type.get() == VREG
    {
        let error = nfs_vinvalbuf(vp, V_SAVE, ap.a_cred, Some(ap.a_p));
        if error == Err(Errno::EINTR) {
            return error;
        }
    }
    let error = nfs_setattrrpc(vp, vap, ap.a_cred, Some(ap.a_p));
    if error.is_err() && vap.va_size != VNOVAL as u64 {
        np.n_size.set(tsize);
        let mut va = np.n_vattr.get();
        va.va_size = tsize;
        np.n_vattr.set(va);
        uvm_vnp_setsize(vp, np.n_size.get() as Off);
    }

    if vap.va_size != VNOVAL as u64 && vap.va_size < tsize {
        hint |= NOTE_TRUNCATE;
    }

    VN_KNOTE(vp, hint); // XXX setattrrpc?

    error
}

/// `nfsm_wcc_data(infop, vpp, flagp)`: a version 3 `wcc_data`: the pre-op attributes (their
/// mtime compared with the node's when `*flagp` is `NFSV3_WCCCHK`) and the post-op ones.
/// `*flagp` becomes whether the mtime changed (`NFSV3_WCCCHK`) or the post-op attribute flag
/// (`NFSV3_WCCRATTR`). Nothing when the reply is already gone.
fn nfsm_wcc_data(
    infop: &mut NfsmInfo<'_>,
    vpp: &mut &'static Vnode,
    flagp: &mut i32,
) -> Result<(), Errno> {
    let mut ttattrf = 0;
    let mut ttretf = 0;

    if infop.nmi_mrep.is_none() {
        return Ok(());
    }

    if nfsm_dissect(infop, NFSX_UNSIGNED)?.get(0) == nfs_true {
        let tl = nfsm_dissect(infop, 6 * NFSX_UNSIGNED)?;
        let mtime = fxdr_nfsv3time(&tl.read::<Nfsv3Time>(2 * NFSX_UNSIGNED));
        if *flagp != NFSV3_WCCRATTR {
            ttretf = i32::from(VTONFS(vpp).n_mtime.get() != mtime);
        }
    }
    nfsm_postop_attr(infop, vpp, &mut ttattrf)?;
    if *flagp != NFSV3_WCCRATTR {
        *flagp = ttretf;
    } else {
        *flagp = ttattrf;
    }
    Ok(())
}

/// `nfs_setattrrpc(vp, vap, cred, procp)`: do an nfs setattr rpc.
pub fn nfs_setattrrpc(
    vp: &'static Vnode,
    vap: &Vattr,
    cred: *const Ucred,
    procp: Option<&Proc>,
) -> Result<(), Errno> {
    let mut vp = vp;
    let mut info = NfsmInfo::new();
    let mut wccflag = NFSV3_WCCRATTR;
    let v3 = nfs_isv3(vp);

    info.nmi_v3 = v3;

    NFSSTATS.rpccnt[NFSPROC_SETATTR].fetch_add(1, Ordering::Relaxed);
    let req = nfsm_reqhead(nfsx_fh(v3) + nfsx_sattr(v3));
    info.nmi_mreq = Some(req);
    let mut mb = req;
    nfsm_fhtom(&mut mb, vp, v3);

    if info.nmi_v3 {
        nfsm_v3attrbuild(&mut mb, vap, true);
        nfsm_build(&mut mb, NFSX_UNSIGNED).set(0, nfs_false);
    } else {
        let sp = nfs_v2sattr(
            if vap.va_mode == VNOVAL as u32 {
                nfs_xdrneg1
            } else {
                vtonfsv2_mode(vp.v_type.get(), vap.va_mode)
            },
            if vap.va_uid == VNOVAL as Uid {
                nfs_xdrneg1
            } else {
                txdr_unsigned(vap.va_uid)
            },
            if vap.va_gid == VNOVAL as u32 {
                nfs_xdrneg1
            } else {
                txdr_unsigned(vap.va_gid)
            },
            txdr_unsigned(vap.va_size as u32),
            vap,
        );
        nfsm_build(&mut mb, NFSX_V2SATTR).write(0, &sp);
    }

    info.nmi_procp = procp;
    info.nmi_cred = cred;
    let r = (|| -> Result<(), Errno> {
        let error = nfs_request(vp, NFSPROC_SETATTR, &mut info);

        if info.nmi_v3 {
            nfsm_wcc_data(&mut info, &mut vp, &mut wccflag)?;
        } else if error.is_ok() {
            nfsm_loadattr(&mut info, &mut vp, None)?;
        }

        m_freem_reply(&mut info);
        error
    })();
    // nfsmout:
    r
}

/// A version 2 `sattr` with the given raw words and the times of `vap`.
fn nfs_v2sattr(mode: u32, uid: u32, gid: u32, size: u32, vap: &Vattr) -> Nfsv2Sattr {
    Nfsv2Sattr {
        sa_mode: mode,
        sa_uid: uid,
        sa_gid: gid,
        sa_size: size,
        sa_atime: txdr_nfsv2time(&vap.va_atime),
        sa_mtime: txdr_nfsv2time(&vap.va_mtime),
    }
}

/// `nfsm_getfh(infop, sizep, v3)`: a file handle of a reply (version 3: its length word,
/// checked, first); the reply is freed on failure (`EBADRPC` for a bad length).
fn nfsm_getfh(infop: &mut NfsmInfo<'_>, v3: bool) -> Result<NfsFh, Errno> {
    let size = if v3 {
        let size = fxdr_unsigned(nfsm_dissect(infop, NFSX_UNSIGNED)?.get(0)) as i32;
        if size <= 0 || size as usize > NFSX_V3FHMAX {
            m_freem_reply(infop);
            return Err(Errno::EBADRPC);
        }
        size as usize
    } else {
        NFSX_V2FH
    };
    let tl = nfsm_dissect(infop, nfsm_rndup(size))?;
    let mut fh = NfsFh {
        fh: [0; NFSX_V3FHMAX],
        size,
    };
    fh.fh[..size].copy_from_slice(&tl.bytes()[..size]);
    Ok(fh)
}

/// `vp->v_mount` of an NFS vnode.
fn vmount(vp: &Vnode) -> &'static crate::sys::mount::Mount {
    match vp.v_mount.get() {
        Some(mp) => mp,
        None => panic(format_args!("nfs: vnode {:p} has no mount", vp)),
    }
}

/// `nfs_lookup` (`vop_lookup`): nfs lookup call, one step at a time... First look in cache.
/// If not found, unlock the directory nfsnode and do the rpc.
pub fn nfs_lookup(ap: &mut VopLookupArgs<'_>) -> Result<(), Errno> {
    let cnp = &mut *ap.a_cnp;
    let mut dvp = ap.a_dvp;
    let vpp = &mut *ap.a_vpp;
    let mut info = NfsmInfo::new();
    let mut attrflag = 0;

    info.nmi_v3 = nfs_isv3(dvp);
    let v3 = info.nmi_v3;

    cnp.cn_flags &= !PDIRUNLOCK;
    let flags = cnp.cn_flags;

    *vpp = None;
    if flags & ISLASTCN != 0
        && mnt_flag(dvp) & MNT_RDONLY != 0
        && (cnp.cn_nameiop == DELETE || cnp.cn_nameiop == RENAME)
    {
        return Err(Errno::EROFS);
    }
    if dvp.v_type.get() != VDIR {
        return Err(Errno::ENOTDIR);
    }
    let lockparent = flags & LOCKPARENT != 0;
    let wantparent = flags & (LOCKPARENT | WANTPARENT) != 0;
    let np = VTONFS(dvp);

    // Before tediously performing a linear scan of the directory, check the name cache to
    // see if the directory/name pair we are looking for is known already. If the
    // directory/name pair is found in the name cache, we have to ensure the directory has
    // not changed from the time the cache entry has been created. If it has, the cache entry
    // has to be ignored.
    match cache_lookup(dvp, cnp) {
        // A miss (the C's -1).
        Ok(None) => {}
        Err(e) if e != Errno::ENOENT => {
            *vpp = None;
            return Err(e);
        }
        hit => {
            // Some(vp): a hit, the vnode locked; None: a negative hit (ENOENT).
            let hitvp = hit.ok().flatten();
            let mut vattr = Vattr::new();

            if cnp.cn_flags & PDIRUNLOCK != 0 {
                if let Err(err2) = vn_lock(dvp, LK_EXCLUSIVE | LK_RETRY) {
                    *vpp = None;
                    return Err(err2);
                }
                cnp.cn_flags &= !PDIRUNLOCK;
            }

            if let Err(err2) = VOP_ACCESS(dvp, VEXEC, cnp.cn_cred, cnp.proc()) {
                if let Some(hvp) = hitvp {
                    vput_or_vrele(hvp, dvp);
                }
                *vpp = None;
                return Err(err2);
            }

            match hitvp {
                None => {
                    if VOP_GETATTR(dvp, &mut vattr, cnp.cn_cred, cnp.proc()).is_ok()
                        && vattr.va_mtime.tv_sec == VTONFS(dvp).n_ctime.get()
                    {
                        return Err(Errno::ENOENT);
                    }
                    cache_purge(dvp);
                    np.n_ctime.set(0);
                    // goto dorpc
                }
                Some(newvp) => {
                    if VOP_GETATTR(newvp, &mut vattr, cnp.cn_cred, cnp.proc()).is_ok()
                        && vattr.va_ctime.tv_sec == VTONFS(newvp).n_ctime.get()
                    {
                        NFSSTATS.lookupcache_hits.fetch_add(1, Ordering::Relaxed);
                        if cnp.cn_nameiop != LOOKUP && flags & ISLASTCN != 0 {
                            cnp.cn_flags |= SAVENAME;
                        }
                        if (!lockparent || flags & ISLASTCN == 0) && !ptr::eq(newvp, dvp) {
                            let _ = VOP_UNLOCK(dvp);
                            cnp.cn_flags |= PDIRUNLOCK;
                        }
                        *vpp = Some(newvp);
                        return Ok(());
                    }
                    cache_purge(newvp);
                    vput_or_vrele(newvp, dvp);
                    *vpp = None;
                }
            }
        }
    }

    // dorpc:
    let mut newvp: Option<&'static Vnode> = None;
    NFSSTATS.lookupcache_misses.fetch_add(1, Ordering::Relaxed);
    NFSSTATS.rpccnt[NFSPROC_LOOKUP].fetch_add(1, Ordering::Relaxed);
    let len = cnp.name().len();
    let req = nfsm_reqhead(nfsx_fh(v3) + NFSX_UNSIGNED + nfsm_rndup(len));
    info.nmi_mreq = Some(req);
    let mut mb = req;
    nfsm_fhtom(&mut mb, dvp, v3);
    let error: Result<(), Errno> = 'nfsmout: {
        nfsm_try!('nfsmout, nfsm_strtom(&mut info, &mut mb, cnp.name(), NFS_MAXNAMLEN));

        info.nmi_procp = cn_proc(cnp);
        info.nmi_cred = cnp.cn_cred;
        if let Err(e) = nfs_request(dvp, NFSPROC_LOOKUP, &mut info) {
            if v3 {
                nfsm_try!('nfsmout, nfsm_postop_attr(&mut info, &mut dvp, &mut attrflag));
            }
            m_freem_reply(&mut info);
            break 'nfsmout Err(e);
        }

        let fh = nfsm_try!('nfsmout, nfsm_getfh(&mut info, v3));

        // Handle RENAME case...
        if cnp.cn_nameiop == RENAME && wantparent && flags & ISLASTCN != 0 {
            if nfs_cmpfh(np, fh.bytes()) {
                m_freem_reply(&mut info);
                return Err(Errno::EISDIR);
            }
            let nnp = match nfs_nget(vmount(dvp), fh.bytes()) {
                Ok(nnp) => nnp,
                Err(e) => {
                    m_freem_reply(&mut info);
                    return Err(e);
                }
            };
            let mut nvp = NFSTOV(nnp);
            newvp = Some(nvp);
            if v3 {
                nfsm_try!('nfsmout, nfsm_postop_attr(&mut info, &mut nvp, &mut attrflag));
                newvp = Some(nvp);
                nfsm_try!('nfsmout, nfsm_postop_attr(&mut info, &mut dvp, &mut attrflag));
            } else {
                nfsm_try!('nfsmout, nfsm_loadattr(&mut info, &mut nvp, None));
            }
            *vpp = Some(nvp);
            m_freem_reply(&mut info);
            cnp.cn_flags |= SAVENAME;
            if !lockparent {
                let _ = VOP_UNLOCK(dvp);
                cnp.cn_flags |= PDIRUNLOCK;
            }
            return Ok(());
        }

        // The postop attr handling is duplicated for each if case, because it should be done
        // while dvp is locked (unlocking dvp is different for each case).

        let mut nvp;
        if nfs_cmpfh(np, fh.bytes()) {
            vref(dvp);
            nvp = dvp;
            newvp = Some(nvp);
            if v3 {
                nfsm_try!('nfsmout, nfsm_postop_attr(&mut info, &mut nvp, &mut attrflag));
                newvp = Some(nvp);
                nfsm_try!('nfsmout, nfsm_postop_attr(&mut info, &mut dvp, &mut attrflag));
            } else {
                nfsm_try!('nfsmout, nfsm_loadattr(&mut info, &mut nvp, None));
                newvp = Some(nvp);
            }
        } else if flags & ISDOTDOT != 0 {
            let _ = VOP_UNLOCK(dvp);
            cnp.cn_flags |= PDIRUNLOCK;

            let nnp = match nfs_nget(vmount(dvp), fh.bytes()) {
                Ok(nnp) => nnp,
                Err(e) => {
                    if vn_lock(dvp, LK_EXCLUSIVE | LK_RETRY).is_ok() {
                        cnp.cn_flags &= !PDIRUNLOCK;
                    }
                    m_freem_reply(&mut info);
                    return Err(e);
                }
            };
            nvp = NFSTOV(nnp);
            newvp = Some(nvp);

            if v3 {
                nfsm_try!('nfsmout, nfsm_postop_attr(&mut info, &mut nvp, &mut attrflag));
                newvp = Some(nvp);
                nfsm_try!('nfsmout, nfsm_postop_attr(&mut info, &mut dvp, &mut attrflag));
            } else {
                nfsm_try!('nfsmout, nfsm_loadattr(&mut info, &mut nvp, None));
                newvp = Some(nvp);
            }

            if lockparent && flags & ISLASTCN != 0 {
                if let Err(e) = vn_lock(dvp, LK_EXCLUSIVE) {
                    m_freem_reply(&mut info);
                    vput(nvp);
                    return Err(e);
                }
                cnp.cn_flags &= !PDIRUNLOCK;
            }
        } else {
            let nnp = match nfs_nget(vmount(dvp), fh.bytes()) {
                Ok(nnp) => nnp,
                Err(e) => {
                    m_freem_reply(&mut info);
                    return Err(e);
                }
            };
            nvp = NFSTOV(nnp);
            newvp = Some(nvp);
            if v3 {
                nfsm_try!('nfsmout, nfsm_postop_attr(&mut info, &mut nvp, &mut attrflag));
                newvp = Some(nvp);
                nfsm_try!('nfsmout, nfsm_postop_attr(&mut info, &mut dvp, &mut attrflag));
            } else {
                nfsm_try!('nfsmout, nfsm_loadattr(&mut info, &mut nvp, None));
                newvp = Some(nvp);
            }
            if !lockparent || flags & ISLASTCN == 0 {
                let _ = VOP_UNLOCK(dvp);
                cnp.cn_flags |= PDIRUNLOCK;
            }
        }

        if cnp.cn_nameiop != LOOKUP && flags & ISLASTCN != 0 {
            cnp.cn_flags |= SAVENAME;
        }
        if cnp.cn_flags & MAKEENTRY != 0 && (cnp.cn_nameiop != DELETE || flags & ISLASTCN == 0) {
            nfs_cache_enter(dvp, Some(nvp), cnp);
        }

        *vpp = Some(nvp);
        m_freem_reply(&mut info);
        Ok(())
    };

    // nfsmout:
    if let Err(mut e) = error {
        // We get here only because of errors returned by the RPC. Otherwise we'd already
        // have returned.
        if e == Errno::ENOENT && cnp.cn_flags & MAKEENTRY != 0 && cnp.cn_nameiop != CREATE {
            nfs_cache_enter(dvp, None, cnp);
        }
        if let Some(nvp) = newvp {
            vput_or_vrele(nvp, dvp);
        }
        if (cnp.cn_nameiop == CREATE || cnp.cn_nameiop == RENAME)
            && flags & ISLASTCN != 0
            && e == Errno::ENOENT
        {
            e = if mnt_flag(dvp) & MNT_RDONLY != 0 {
                Errno::EROFS
            } else {
                Errno::EJUSTRETURN
            };
        }
        if cnp.cn_nameiop != LOOKUP && flags & ISLASTCN != 0 {
            cnp.cn_flags |= SAVENAME;
        }
        *vpp = None;
        return Err(e);
    }
    Ok(())
}

/// `nfs_read` (`vop_read`): nfs read call. Just call `nfs_bioread()` to do the work.
pub fn nfs_read(ap: &mut VopReadArgs<'_, '_>) -> Result<(), Errno> {
    let vp = ap.a_vp;

    if vp.v_type.get() != VREG {
        return Err(Errno::EPERM);
    }
    nfs_bioread(vp, ap.a_uio, ap.a_ioflag, ap.a_cred)
}

/// `nfs_readlink` (`vop_readlink`): nfs readlink call.
pub fn nfs_readlink(ap: &mut VopReadlinkArgs<'_, '_>) -> Result<(), Errno> {
    let vp = ap.a_vp;

    if vp.v_type.get() != VLNK {
        return Err(Errno::EPERM);
    }
    nfs_bioread(vp, ap.a_uio, 0, ap.a_cred)
}

/// `nfs_lock` (`vop_lock`): lock an inode.
pub fn nfs_lock(ap: &mut VopLockArgs) -> Result<(), Errno> {
    let vp = ap.a_vp;

    rrw_enter(&VTONFS(vp).n_lock, ap.a_flags & LK_RWFLAGS)
}

/// `nfs_unlock` (`vop_unlock`): unlock an inode.
pub fn nfs_unlock(ap: &mut VopUnlockArgs) -> Result<(), Errno> {
    let vp = ap.a_vp;

    rrw_exit(&VTONFS(vp).n_lock);
    Ok(())
}

/// `nfs_islocked` (`vop_islocked`): check for a locked inode.
pub fn nfs_islocked(ap: &mut VopIslockedArgs) -> i32 {
    rrw_status(&VTONFS(ap.a_vp).n_lock)
}

/// `nfs_readlinkrpc(vp, uiop, cred)`: do a readlink rpc. Called by `nfs_doio()` from below
/// the buffer cache.
pub fn nfs_readlinkrpc(
    vp: &'static Vnode,
    uiop: &mut Uio<'_>,
    cred: *const Ucred,
) -> Result<(), Errno> {
    let mut vp = vp;
    let mut info = NfsmInfo::new();
    let mut attrflag = 0;

    info.nmi_v3 = nfs_isv3(vp);

    NFSSTATS.rpccnt[NFSPROC_READLINK].fetch_add(1, Ordering::Relaxed);
    let req = nfsm_reqhead(nfsx_fh(info.nmi_v3));
    info.nmi_mreq = Some(req);
    let mut mb = req;
    nfsm_fhtom(&mut mb, vp, info.nmi_v3);

    info.nmi_procp = curproc();
    info.nmi_cred = cred;
    let error: Result<(), Errno> = 'nfsmout: {
        let error = nfs_request(vp, NFSPROC_READLINK, &mut info);

        if info.nmi_v3 {
            nfsm_try!('nfsmout, nfsm_postop_attr(&mut info, &mut vp, &mut attrflag));
        }
        if error.is_ok() {
            let len = nfsm_try!('nfsmout, nfsm_strsiz(&mut info, NFS_MAXPATHLEN));
            nfsm_try!('nfsmout, nfsm_mtouio(&mut info, uiop, len as i32));
        }

        m_freem_reply(&mut info);
        error
    };

    // nfsmout:
    error
}

/// `nfs_readrpc(vp, uiop)`: nfs read rpc call: read `uiop.uio_resid` bytes at
/// `uiop.uio_offset` of `vp` from the server, `nm_rsize` bytes per RPC.
pub fn nfs_readrpc(vp: &'static Vnode, uiop: &mut Uio<'_>) -> Result<(), Errno> {
    let mut vp = vp;
    let v3 = nfs_isv3(vp);
    let mut attrflag = 0;

    let nmp = vfstonfs_vp(vp);
    let mut tsiz = uiop.uio_resid as i64;
    if uiop.uio_offset + tsiz > 0xffff_ffff && !v3 {
        return Err(Errno::EFBIG);
    }
    while tsiz > 0 {
        let mut info = NfsmInfo::new();
        info.nmi_v3 = v3;

        NFSSTATS.rpccnt[NFSPROC_READ].fetch_add(1, Ordering::Relaxed);
        let len = tsiz.min(i64::from(nmp.nm_rsize.get()));
        let req = nfsm_reqhead(nfsx_fh(v3) + NFSX_UNSIGNED * 3);
        info.nmi_mreq = Some(req);
        let mut mb = req;
        nfsm_fhtom(&mut mb, vp, v3);
        let mut tl = nfsm_build(&mut mb, NFSX_UNSIGNED * 3);
        if v3 {
            tl.txdr_hyper(0, uiop.uio_offset as u64);
            tl.set(2, txdr_unsigned(len as u32));
        } else {
            tl.set(0, txdr_unsigned(uiop.uio_offset as u32));
            tl.set(1, txdr_unsigned(len as u32));
            tl.set(2, 0);
        }

        info.nmi_procp = curproc();
        info.nmi_cred = VTONFS(vp).n_rcred.get();
        let error = nfs_request(vp, NFSPROC_READ, &mut info);
        if v3 {
            nfsm_postop_attr(&mut info, &mut vp, &mut attrflag)?;
        }
        if let Err(e) = error {
            m_freem_reply(&mut info);
            return Err(e);
        }

        let eof = if v3 {
            fxdr_unsigned(nfsm_dissect(&mut info, 2 * NFSX_UNSIGNED)?.get(1)) as i32
        } else {
            nfsm_loadattr(&mut info, &mut vp, None)?;
            0
        };

        let retlen = nfsm_strsiz(&mut info, nmp.nm_rsize.get() as usize)? as i64;
        nfsm_mtouio(&mut info, uiop, retlen as i32)?;
        m_freem_reply(&mut info);
        tsiz -= retlen;
        if v3 {
            if eof != 0 || retlen == 0 {
                tsiz = 0;
            }
        } else if retlen < len {
            tsiz = 0;
        }
    }

    // nfsmout:
    Ok(())
}

/// `nfs_writerpc(vp, uiop, iomode, must_commit)`: nfs write call: write `uiop` to the
/// server, `nm_wsize` bytes per RPC. `iomode` is the `NFSV3WRITE_*` asked for and returns
/// the lowest commitment level any RPC got; `must_commit` is set when the server's write
/// verifier changed.
pub fn nfs_writerpc(
    vp: &'static Vnode,
    uiop: &mut Uio<'_>,
    iomode: &mut u32,
    must_commit: &mut bool,
) -> Result<(), Errno> {
    let mut vp = vp;
    let nmp = vfstonfs_vp(vp);
    let v3 = nfs_isv3(vp);
    let mut wccflag;
    let mut committed = NFSV3WRITE_FILESYNC;
    let mut info = NfsmInfo::new();

    #[cfg(feature = "diagnostic")]
    if uiop.uio_iovcnt() != 1 {
        panic(format_args!("nfs: writerpc iovcnt > 1"));
    }
    *must_commit = false;
    let mut tsiz = uiop.uio_resid as i64;
    if uiop.uio_offset + tsiz > 0xffff_ffff && !v3 {
        return Err(Errno::EFBIG);
    }
    let error: Result<(), Errno> = 'nfsmout: {
        while tsiz > 0 {
            info = NfsmInfo::new();
            info.nmi_v3 = v3;

            NFSSTATS.rpccnt[NFSPROC_WRITE].fetch_add(1, Ordering::Relaxed);
            let mut len = tsiz.min(i64::from(nmp.nm_wsize.get()));
            let req = nfsm_reqhead(nfsx_fh(v3) + 5 * NFSX_UNSIGNED + nfsm_rndup(len as usize));
            info.nmi_mreq = Some(req);
            let mut mb = req;
            nfsm_fhtom(&mut mb, vp, v3);
            if v3 {
                let mut tl = nfsm_build(&mut mb, 5 * NFSX_UNSIGNED);
                tl.txdr_hyper(0, uiop.uio_offset as u64);
                tl.set(2, txdr_unsigned(len as u32));
                tl.set(3, txdr_unsigned(*iomode));
                tl.set(4, txdr_unsigned(len as u32));
            } else {
                let mut tl = nfsm_build(&mut mb, 4 * NFSX_UNSIGNED);
                // Set both "begin" and "current" to non-garbage.
                let x = txdr_unsigned(uiop.uio_offset as u32);
                tl.set(0, x); // "begin offset"
                tl.set(1, x); // "current offset"
                let x = txdr_unsigned(len as u32);
                tl.set(2, x); // total to this offset
                tl.set(3, x); // size of this write
            }
            nfsm_uiotombuf(&mut mb, uiop, len as usize);

            info.nmi_procp = curproc();
            info.nmi_cred = VTONFS(vp).n_wcred.get();
            let error = nfs_request(vp, NFSPROC_WRITE, &mut info);
            wccflag = NFSV3_WCCRATTR;
            if v3 {
                wccflag = NFSV3_WCCCHK;
                nfsm_try!('nfsmout, nfsm_wcc_data(&mut info, &mut vp, &mut wccflag));
            }

            if let Err(e) = error {
                break 'nfsmout Err(e);
            }

            if v3 {
                wccflag = NFSV3_WCCCHK;
                let tl = nfsm_try!(
                    'nfsmout,
                    nfsm_dissect(&mut info, 2 * NFSX_UNSIGNED + NFSX_V3WRITEVERF)
                );
                let rlen = i64::from(fxdr_unsigned(tl.get(0)) as i32);
                let commit = fxdr_unsigned(tl.get(1));
                let mut verf = [0u8; NFSX_V3WRITEVERF];
                verf.copy_from_slice(&tl.bytes()[2 * NFSX_UNSIGNED..][..NFSX_V3WRITEVERF]);
                if rlen <= 0 {
                    // info.nmi_mrep free'd after the loop
                    break 'nfsmout Err(errno_of(NFSERR_IO));
                } else if rlen < len {
                    let backup = (len - rlen) as usize;
                    if let Some(iov) = uiop.uio_iov.first_mut() {
                        iov.iov_base = iov
                            .iov_base
                            .cast::<u8>()
                            .wrapping_sub(backup)
                            .cast::<c_void>();
                        iov.iov_len += backup;
                    }
                    uiop.uio_offset -= backup as Off;
                    uiop.uio_resid += backup;
                    len = rlen;
                }

                // Return the lowest commitment level obtained by any of the RPCs.
                committed = nfs_lowest_commit(committed, commit);
                if nmp.nm_flag.get() & NFSMNT_HASWRITEVERF == 0 {
                    nmp.nm_verf.set(verf);
                    nmp.nm_flag.set(nmp.nm_flag.get() | NFSMNT_HASWRITEVERF);
                } else if nmp.nm_verf.get() != verf {
                    *must_commit = true;
                    nmp.nm_verf.set(verf);
                }
            } else {
                nfsm_try!('nfsmout, nfsm_loadattr(&mut info, &mut vp, None));
            }
            if wccflag != 0 {
                let np = VTONFS(vp);
                np.n_mtime.set(np.n_vattr.get().va_mtime);
            }
            m_freem_reply(&mut info);
            tsiz -= len;
        }
        Ok(())
    };

    // nfsmout:
    m_freem_reply(&mut info);
    *iomode = committed;
    if error.is_err() {
        uiop.uio_resid = tsiz as usize;
    }
    error
}

/// The commitment level of a series of writes: the lowest of `committed` (so far) and
/// `commit` (the last RPC's), `NFSV3WRITE_FILESYNC` > `NFSV3WRITE_DATASYNC` >
/// `NFSV3WRITE_UNSTABLE`.
fn nfs_lowest_commit(committed: u32, commit: u32) -> u32 {
    if committed == NFSV3WRITE_FILESYNC
        || (committed == NFSV3WRITE_DATASYNC && commit == NFSV3WRITE_UNSTABLE)
    {
        commit
    } else {
        committed
    }
}

/// An NFS status that is an errno number (`NFSERR_IO` ...) as an `Errno`.
fn errno_of(nfserr: i32) -> Errno {
    Errno::from_raw(nfserr).unwrap_or(Errno::EIO)
}

/// `nfsm_mtofh(infop, dvp, vpp, flagp)`: the optional file handle and attributes of a new
/// object in a create-style reply: the node of the handle (`*vpp`, referenced and locked)
/// with its attributes loaded. `*flagp` says whether a handle came (always for version 2).
fn nfsm_mtofh(
    infop: &mut NfsmInfo<'_>,
    dvp: &'static Vnode,
    vpp: &mut Option<&'static Vnode>,
    flagp: &mut i32,
) -> Result<(), Errno> {
    let mut flag = if infop.nmi_v3 {
        fxdr_unsigned(nfsm_dissect(infop, NFSX_UNSIGNED)?.get(0)) as i32
    } else {
        1
    };
    if flag != 0 {
        let fh = nfsm_getfh(infop, infop.nmi_v3)?;
        match nfs_nget(vmount(dvp), fh.bytes()) {
            Ok(ttnp) => *vpp = Some(NFSTOV(ttnp)),
            Err(e) => {
                m_freem_reply(infop);
                return Err(e);
            }
        }
    }
    if infop.nmi_v3 {
        let w = fxdr_unsigned(nfsm_dissect(infop, NFSX_UNSIGNED)?.get(0)) as i32;
        if flag != 0 {
            flag = w;
        } else if w != 0 {
            nfsm_adv(infop, NFSX_V3FATTR)?;
        }
    }
    if flag != 0
        && let Some(mut vp) = *vpp
    {
        let r = nfsm_loadattr(infop, &mut vp, None);
        *vpp = Some(vp);
        r?;
    }
    *flagp = flag;
    Ok(())
}

/// `nfs_mknodrpc(dvp, vpp, cnp, vap)`: nfs mknod rpc. For NFS v2 this is a kludge. Use a
/// create rpc but with the IFMT bits of the mode set to specify the file type and the size
/// field for rdev.
pub fn nfs_mknodrpc(
    dvp: &'static Vnode,
    vpp: &mut Option<&'static Vnode>,
    cnp: &mut Componentname,
    vap: &Vattr,
) -> Result<(), Errno> {
    let mut wdvp = dvp;
    let mut info = NfsmInfo::new();
    let mut newvp: Option<&'static Vnode> = None;
    let mut wccflag = NFSV3_WCCRATTR;
    let mut gotvp = 0;

    info.nmi_v3 = nfs_isv3(dvp);
    let v3 = info.nmi_v3;

    let rdev = match vap.va_type {
        VCHR | VBLK => txdr_unsigned(vap.va_rdev as u32),
        VFIFO | VSOCK => nfs_xdrneg1,
        _ => {
            let _ = VOP_ABORTOP(dvp, cnp);
            return Err(Errno::EOPNOTSUPP);
        }
    };
    NFSSTATS.rpccnt[NFSPROC_MKNOD].fetch_add(1, Ordering::Relaxed);
    let req = nfsm_reqhead(
        nfsx_fh(v3) + 4 * NFSX_UNSIGNED + nfsm_rndup(cnp.name().len()) + nfsx_sattr(v3),
    );
    info.nmi_mreq = Some(req);
    let mut mb = req;
    nfsm_fhtom(&mut mb, dvp, v3);
    let error: Result<(), Errno> = 'nfsmout: {
        nfsm_try!('nfsmout, nfsm_strtom(&mut info, &mut mb, cnp.name(), NFS_MAXNAMLEN));

        if v3 {
            nfsm_build(&mut mb, NFSX_UNSIGNED).set(0, vtonfsv3_type(vap.va_type));
            nfsm_v3attrbuild(&mut mb, vap, false);
            if vap.va_type == VCHR || vap.va_type == VBLK {
                let mut tl = nfsm_build(&mut mb, 2 * NFSX_UNSIGNED);
                tl.set(0, txdr_unsigned(major(vap.va_rdev)));
                tl.set(1, txdr_unsigned(minor(vap.va_rdev)));
            }
        } else {
            let sp = nfs_v2sattr(
                vtonfsv2_mode(vap.va_type, vap.va_mode),
                nfs_xdrneg1,
                nfs_xdrneg1,
                rdev,
                vap,
            );
            nfsm_build(&mut mb, NFSX_V2SATTR).write(0, &sp);
        }

        assert_cn_curproc(cnp);
        info.nmi_procp = cn_proc(cnp);
        info.nmi_cred = cnp.cn_cred;
        let mut error = nfs_request(dvp, NFSPROC_MKNOD, &mut info);
        if error.is_ok() {
            nfsm_try!('nfsmout, nfsm_mtofh(&mut info, dvp, &mut newvp, &mut gotvp));
            if gotvp == 0 {
                let mut np = None;
                error = nfs_lookitup(dvp, cnp.name(), cnp.cn_cred, cn_proc(cnp), Some(&mut np));
                if error.is_ok() {
                    newvp = np.map(NFSTOV);
                }
            }
        }
        if v3 {
            nfsm_try!('nfsmout, nfsm_wcc_data(&mut info, &mut wdvp, &mut wccflag));
        }
        m_freem_reply(&mut info);
        error
    };

    // nfsmout:
    if error.is_err() {
        if let Some(nvp) = newvp {
            vput(nvp);
        }
    } else {
        if cnp.cn_flags & MAKEENTRY != 0 {
            nfs_cache_enter(dvp, newvp, cnp);
        }
        *vpp = newvp;
    }
    pnbuf_free(cnp);
    VTONFS(dvp).set(NMODIFIED);
    if wccflag == 0 {
        nfs_invalidate_attrcache(VTONFS(dvp));
    }
    error
}

/// `nfs_mknod` (`vop_mknod`): nfs mknod vop. Just call `nfs_mknodrpc()` to do the work.
pub fn nfs_mknod(ap: &mut VopMknodArgs<'_>) -> Result<(), Errno> {
    let mut newvp = None;

    let error = nfs_mknodrpc(ap.a_dvp, &mut newvp, ap.a_cnp, ap.a_vap);
    if error.is_ok()
        && let Some(nvp) = newvp
    {
        vput(nvp);
    }

    VN_KNOTE(ap.a_dvp, NOTE_WRITE);

    error
}

/// `nfs_create` (`vop_create`): nfs file create call.
pub fn nfs_create(ap: &mut VopCreateArgs<'_>) -> Result<(), Errno> {
    let dvp = ap.a_dvp;
    let vap = &mut *ap.a_vap;
    let cnp = &mut *ap.a_cnp;
    let mut wdvp = dvp;
    let mut newvp: Option<&'static Vnode>;
    let mut wccflag = NFSV3_WCCRATTR;
    let mut gotvp = 0;
    let mut fmode = 0;
    let v3 = nfs_isv3(dvp);

    // Oops, not for me..
    if vap.va_type == VSOCK {
        return nfs_mknodrpc(dvp, ap.a_vpp, cnp, vap);
    }

    if vap.va_vaflags & VA_EXCLUSIVE != 0 {
        fmode |= O_EXCL;
    }

    let mut error;
    loop {
        // again:
        let mut info = NfsmInfo::new();
        info.nmi_v3 = v3;
        newvp = None;

        NFSSTATS.rpccnt[NFSPROC_CREATE].fetch_add(1, Ordering::Relaxed);
        let req = nfsm_reqhead(
            nfsx_fh(v3) + 2 * NFSX_UNSIGNED + nfsm_rndup(cnp.name().len()) + nfsx_sattr(v3),
        );
        info.nmi_mreq = Some(req);
        let mut mb = req;
        nfsm_fhtom(&mut mb, dvp, v3);
        error = 'nfsmout: {
            nfsm_try!('nfsmout, nfsm_strtom(&mut info, &mut mb, cnp.name(), NFS_MAXNAMLEN));
            if v3 {
                if fmode & O_EXCL != 0 {
                    nfsm_build(&mut mb, NFSX_UNSIGNED).set(0, txdr_unsigned(NFSV3CREATE_EXCLUSIVE));
                    let mut verf = [0u8; NFSX_V3CREATEVERF];
                    crate::dev::rnd::arc4random_buf(&mut verf);
                    nfsm_build(&mut mb, NFSX_V3CREATEVERF).set_bytes(0, &verf);
                } else {
                    nfsm_build(&mut mb, NFSX_UNSIGNED).set(0, txdr_unsigned(NFSV3CREATE_UNCHECKED));
                    nfsm_v3attrbuild(&mut mb, vap, false);
                }
            } else {
                let sp = nfs_v2sattr(
                    vtonfsv2_mode(vap.va_type, vap.va_mode),
                    nfs_xdrneg1,
                    nfs_xdrneg1,
                    0,
                    vap,
                );
                nfsm_build(&mut mb, NFSX_V2SATTR).write(0, &sp);
            }

            assert_cn_curproc(cnp);
            info.nmi_procp = cn_proc(cnp);
            info.nmi_cred = cnp.cn_cred;
            let mut error = nfs_request(dvp, NFSPROC_CREATE, &mut info);
            if error.is_ok() {
                nfsm_try!('nfsmout, nfsm_mtofh(&mut info, dvp, &mut newvp, &mut gotvp));
                if gotvp == 0 {
                    let mut np = None;
                    error = nfs_lookitup(dvp, cnp.name(), cnp.cn_cred, cn_proc(cnp), Some(&mut np));
                    if error.is_ok() {
                        newvp = np.map(NFSTOV);
                    }
                }
            }
            if v3 {
                nfsm_try!('nfsmout, nfsm_wcc_data(&mut info, &mut wdvp, &mut wccflag));
            }
            m_freem_reply(&mut info);
            error
        };

        // nfsmout:
        if let Err(e) = error {
            if let Some(nvp) = newvp.take() {
                vput(nvp);
            }
            if v3 && fmode & O_EXCL != 0 && nfs_errno_is(e, NFSERR_NOTSUPP) {
                fmode &= !O_EXCL;
                continue; // goto again
            }
        } else if v3 && fmode & O_EXCL != 0 {
            let ts = getnanotime();
            if vap.va_atime.tv_nsec == i64::from(VNOVAL) {
                vap.va_atime = ts;
            }
            if vap.va_mtime.tv_nsec == i64::from(VNOVAL) {
                vap.va_mtime = ts;
            }
            if let Some(nvp) = newvp {
                error = nfs_setattrrpc(nvp, vap, cnp.cn_cred, cn_proc(cnp));
            }
        }
        break;
    }
    if error.is_ok() {
        if cnp.cn_flags & MAKEENTRY != 0 {
            nfs_cache_enter(dvp, newvp, cnp);
        }
        *ap.a_vpp = newvp;
    }
    pnbuf_free(cnp);
    VTONFS(dvp).set(NMODIFIED);
    if wccflag == 0 {
        nfs_invalidate_attrcache(VTONFS(dvp));
    }
    VN_KNOTE(ap.a_dvp, NOTE_WRITE);
    error
}

/// `nfs_remove` (`vop_remove`): nfs file remove call. To try and make nfs semantics closer
/// to ufs semantics, a file that has other processes using the vnode is renamed instead of
/// removed and then removed later on the last close.
/// - If `v_usecount > 1`: if a rename is not already in the works, call `nfs_sillyrename()`
///   to set it up;
/// - else do the remove rpc.
pub fn nfs_remove(ap: &mut VopRemoveArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let dvp = ap.a_dvp;
    let cnp = &mut *ap.a_cnp;
    let np = VTONFS(vp);
    let mut vattr = Vattr::new();

    #[cfg(feature = "diagnostic")]
    {
        if cnp.cn_flags & HASBUF == 0 {
            panic(format_args!("nfs_remove: no name"));
        }
        if vp.v_usecount.get() < 1 {
            panic(format_args!("nfs_remove: bad v_usecount"));
        }
    }
    let error = if vp.v_type.get() == VDIR {
        Err(Errno::EPERM)
    } else if vp.v_usecount.get() == 1
        || (np.n_sillyrename.get().is_some()
            && VOP_GETATTR(vp, &mut vattr, cnp.cn_cred, cnp.proc()).is_ok()
            && vattr.va_nlink > 1)
    {
        // Purge the name cache so that the chance of a lookup for the name succeeding while
        // the remove is in progress is minimized. Without node locking it can still happen,
        // such that an I/O op returns ESTALE, but since you get this if another host removes
        // the file..
        cache_purge(vp);
        // throw away biocache buffers, mainly to avoid unnecessary delayed writes later.
        let mut error = nfs_vinvalbuf(vp, 0, cnp.cn_cred, cn_proc(cnp));
        // Do the rpc
        if error != Err(Errno::EINTR) {
            error = nfs_removerpc(dvp, cnp.name(), cnp.cn_cred, cn_proc(cnp));
        }
        // Kludge City: If the first reply to the remove rpc is lost.. the reply to the
        // retransmitted request will be ENOENT since the file was in fact removed. Therefore,
        // we cheat and return success.
        if error == Err(Errno::ENOENT) {
            error = Ok(());
        }
        error
    } else if np.n_sillyrename.get().is_none() {
        nfs_sillyrename(dvp, vp, cnp)
    } else {
        Ok(())
    };
    pnbuf_free(cnp);
    nfs_invalidate_attrcache(np);
    VN_KNOTE(vp, NOTE_DELETE);
    VN_KNOTE(dvp, NOTE_WRITE);
    error
}

/// `nfs_removeit(sp)`: nfs file remove rpc called from `nfs_inactive`.
pub fn nfs_removeit(sp: &Sillyrename) -> Result<(), Errno> {
    kassert!(VOP_ISLOCKED(sp.s_dvp) != 0);
    // Make sure that the directory vnode is still valid.
    //
    // NFS can potentially try to nuke a silly *after* the directory has already been pushed
    // out on a forced unmount. Since the silly is going to go away anyway, this is fine.
    if sp.s_dvp.v_type.get() == VBAD {
        return Ok(());
    }
    nfs_removerpc(
        sp.s_dvp,
        &sp.s_name[..sp.s_namlen as usize],
        ptr::from_ref(sp.s_cred),
        None,
    )
}

/// `nfs_removerpc(dvp, name, namelen, cred, proc)`: nfs remove rpc, called from
/// `nfs_remove()` and `nfs_removeit()`.
pub fn nfs_removerpc(
    dvp: &'static Vnode,
    name: &[u8],
    cred: *const Ucred,
    proc: Option<&Proc>,
) -> Result<(), Errno> {
    let mut wdvp = dvp;
    let mut info = NfsmInfo::new();
    let mut wccflag = NFSV3_WCCRATTR;

    info.nmi_v3 = nfs_isv3(dvp);

    NFSSTATS.rpccnt[NFSPROC_REMOVE].fetch_add(1, Ordering::Relaxed);
    let req = nfsm_reqhead(nfsx_fh(info.nmi_v3) + NFSX_UNSIGNED + nfsm_rndup(name.len()));
    info.nmi_mreq = Some(req);
    let mut mb = req;
    nfsm_fhtom(&mut mb, dvp, info.nmi_v3);
    let error: Result<(), Errno> = 'nfsmout: {
        nfsm_try!('nfsmout, nfsm_strtom(&mut info, &mut mb, name, NFS_MAXNAMLEN));

        info.nmi_procp = proc;
        info.nmi_cred = cred;
        let error = nfs_request(dvp, NFSPROC_REMOVE, &mut info);
        if info.nmi_v3 {
            nfsm_try!('nfsmout, nfsm_wcc_data(&mut info, &mut wdvp, &mut wccflag));
        }
        m_freem_reply(&mut info);
        error
    };

    // nfsmout:
    VTONFS(dvp).set(NMODIFIED);
    if wccflag == 0 {
        nfs_invalidate_attrcache(VTONFS(dvp));
    }
    error
}

/// Whether two vnodes are on the same mount (`a->v_mount == b->v_mount`).
fn same_mount(a: &Vnode, b: &Vnode) -> bool {
    match (a.v_mount.get(), b.v_mount.get()) {
        (Some(x), Some(y)) => ptr::eq(x, y),
        (None, None) => true,
        _ => false,
    }
}

/// `nfs_rename` (`vop_rename`): nfs file rename call.
pub fn nfs_rename(ap: &mut VopRenameArgs<'_>) -> Result<(), Errno> {
    let fvp = ap.a_fvp;
    let mut tvp = ap.a_tvp;
    let fdvp = ap.a_fdvp;
    let tdvp = ap.a_tdvp;
    let tcnp = &mut *ap.a_tcnp;
    let fcnp = &mut *ap.a_fcnp;

    #[cfg(feature = "diagnostic")]
    if tcnp.cn_flags & HASBUF == 0 || fcnp.cn_flags & HASBUF == 0 {
        panic(format_args!("nfs_rename: no name"));
    }
    let error = 'out: {
        // Check for cross-device rename
        if !same_mount(fvp, tdvp) || tvp.is_some_and(|tvp| !same_mount(fvp, tvp)) {
            break 'out Err(Errno::EXDEV);
        }

        // If the tvp exists and is in use, sillyrename it before doing the rename of the new
        // file over it.
        if let Some(t) = tvp
            && t.v_usecount.get() > 1
            && VTONFS(t).n_sillyrename.get().is_none()
            && t.v_type.get() != VDIR
            && nfs_sillyrename(tdvp, t, tcnp).is_ok()
        {
            VN_KNOTE(t, NOTE_DELETE);
            vput(t);
            tvp = None;
        }

        let error = nfs_renamerpc(
            fdvp,
            fcnp.name(),
            tdvp,
            tcnp.name(),
            tcnp.cn_cred,
            cn_proc(tcnp),
        );

        VN_KNOTE(fdvp, NOTE_WRITE);
        VN_KNOTE(tdvp, NOTE_WRITE);

        if fvp.v_type.get() == VDIR {
            if tvp.is_some_and(|t| t.v_type.get() == VDIR) {
                cache_purge(tdvp);
            }
            cache_purge(fdvp);
        }
        error
    };

    // out:
    if tvp.is_some_and(|t| ptr::eq(tdvp, t)) {
        let _ = vrele(tdvp);
    } else {
        vput(tdvp);
    }
    if let Some(t) = tvp {
        vput(t);
    }
    let _ = vrele(fdvp);
    let _ = vrele(fvp);
    // Kludge: Map ENOENT => 0 assuming that it is a reply to a retry.
    if error == Err(Errno::ENOENT) {
        return Ok(());
    }
    error
}

/// `nfs_renameit(sdvp, scnp, sp)`: nfs file rename rpc called from `nfs_remove()` above
/// (through `nfs_sillyrename`).
pub fn nfs_renameit(
    sdvp: &'static Vnode,
    scnp: &Componentname,
    sp: &Sillyrename,
) -> Result<(), Errno> {
    nfs_renamerpc(
        sdvp,
        scnp.name(),
        sdvp,
        &sp.s_name[..sp.s_namlen as usize],
        scnp.cn_cred,
        curproc(),
    )
}

/// `nfs_renamerpc(fdvp, fnameptr, fnamelen, tdvp, tnameptr, tnamelen, cred, proc)`: do an
/// nfs rename rpc. Called from `nfs_rename()` and `nfs_renameit()`.
pub fn nfs_renamerpc(
    fdvp: &'static Vnode,
    fname: &[u8],
    tdvp: &'static Vnode,
    tname: &[u8],
    cred: *const Ucred,
    proc: Option<&Proc>,
) -> Result<(), Errno> {
    let (mut wfdvp, mut wtdvp) = (fdvp, tdvp);
    let mut info = NfsmInfo::new();
    let mut fwccflag = NFSV3_WCCRATTR;
    let mut twccflag = NFSV3_WCCRATTR;

    info.nmi_v3 = nfs_isv3(fdvp);
    let v3 = info.nmi_v3;

    NFSSTATS.rpccnt[NFSPROC_RENAME].fetch_add(1, Ordering::Relaxed);
    let req = nfsm_reqhead(
        (nfsx_fh(v3) + NFSX_UNSIGNED) * 2 + nfsm_rndup(fname.len()) + nfsm_rndup(tname.len()),
    );
    info.nmi_mreq = Some(req);
    let mut mb = req;
    nfsm_fhtom(&mut mb, fdvp, v3);
    let error: Result<(), Errno> = 'nfsmout: {
        nfsm_try!('nfsmout, nfsm_strtom(&mut info, &mut mb, fname, NFS_MAXNAMLEN));
        nfsm_fhtom(&mut mb, tdvp, v3);
        nfsm_try!('nfsmout, nfsm_strtom(&mut info, &mut mb, tname, NFS_MAXNAMLEN));

        info.nmi_procp = proc;
        info.nmi_cred = cred;
        let error = nfs_request(fdvp, NFSPROC_RENAME, &mut info);
        if v3 {
            nfsm_try!('nfsmout, nfsm_wcc_data(&mut info, &mut wfdvp, &mut fwccflag));
            nfsm_try!('nfsmout, nfsm_wcc_data(&mut info, &mut wtdvp, &mut twccflag));
        }
        m_freem_reply(&mut info);
        error
    };

    // nfsmout:
    VTONFS(fdvp).set(NMODIFIED);
    VTONFS(tdvp).set(NMODIFIED);
    if fwccflag == 0 {
        nfs_invalidate_attrcache(VTONFS(fdvp));
    }
    if twccflag == 0 {
        nfs_invalidate_attrcache(VTONFS(tdvp));
    }
    error
}

/// `nfs_link` (`vop_link`): nfs hard link create call.
pub fn nfs_link(ap: &mut VopLinkArgs<'_>) -> Result<(), Errno> {
    let mut vp = ap.a_vp;
    let mut dvp = ap.a_dvp;
    let cnp = &mut *ap.a_cnp;
    let mut info = NfsmInfo::new();
    let mut wccflag = NFSV3_WCCRATTR;
    let mut attrflag = 0;

    info.nmi_v3 = nfs_isv3(vp);
    let v3 = info.nmi_v3;

    if let Err(e) = vn_lock(vp, LK_EXCLUSIVE) {
        let _ = VOP_ABORTOP(dvp, cnp);
        vput(dvp);
        return Err(e);
    }

    // Push all writes to the server, so that the attribute cache doesn't get "out of sync"
    // with the server. XXX There should be a better way!
    let _ = VOP_FSYNC(vp, cnp.cn_cred, MNT_WAIT, cnp.proc());

    NFSSTATS.rpccnt[NFSPROC_LINK].fetch_add(1, Ordering::Relaxed);
    let req = nfsm_reqhead(2 * nfsx_fh(v3) + NFSX_UNSIGNED + nfsm_rndup(cnp.name().len()));
    info.nmi_mreq = Some(req);
    let mut mb = req;
    nfsm_fhtom(&mut mb, vp, v3);
    nfsm_fhtom(&mut mb, dvp, v3);
    let error: Result<(), Errno> = 'nfsmout: {
        nfsm_try!('nfsmout, nfsm_strtom(&mut info, &mut mb, cnp.name(), NFS_MAXNAMLEN));

        info.nmi_procp = cn_proc(cnp);
        info.nmi_cred = cnp.cn_cred;
        let error = nfs_request(vp, NFSPROC_LINK, &mut info);
        if v3 {
            nfsm_try!('nfsmout, nfsm_postop_attr(&mut info, &mut vp, &mut attrflag));
            nfsm_try!('nfsmout, nfsm_wcc_data(&mut info, &mut dvp, &mut wccflag));
        }
        m_freem_reply(&mut info);
        error
    };

    // nfsmout:
    pnbuf_free(cnp);
    VTONFS(dvp).set(NMODIFIED);
    if attrflag == 0 {
        nfs_invalidate_attrcache(VTONFS(vp));
    }
    if wccflag == 0 {
        nfs_invalidate_attrcache(VTONFS(dvp));
    }

    VN_KNOTE(vp, NOTE_LINK);
    VN_KNOTE(dvp, NOTE_WRITE);
    let _ = VOP_UNLOCK(vp);
    vput(dvp);
    error
}

/// `nfs_symlink` (`vop_symlink`): nfs symbolic link create call.
pub fn nfs_symlink(ap: &mut VopSymlinkArgs<'_>) -> Result<(), Errno> {
    let mut dvp = ap.a_dvp;
    let vap = &*ap.a_vap;
    let cnp = &mut *ap.a_cnp;
    let target = ap.a_target;
    let mut info = NfsmInfo::new();
    let mut wccflag = NFSV3_WCCRATTR;
    let mut gotvp = 0;
    let mut newvp: Option<&'static Vnode> = None;

    info.nmi_v3 = nfs_isv3(dvp);
    let v3 = info.nmi_v3;

    NFSSTATS.rpccnt[NFSPROC_SYMLINK].fetch_add(1, Ordering::Relaxed);
    let slen = target.len();
    let req = nfsm_reqhead(
        nfsx_fh(v3)
            + 2 * NFSX_UNSIGNED
            + nfsm_rndup(cnp.name().len())
            + nfsm_rndup(slen)
            + nfsx_sattr(v3),
    );
    info.nmi_mreq = Some(req);
    let mut mb = req;
    nfsm_fhtom(&mut mb, dvp, v3);
    let error: Result<(), Errno> = 'nfsmout: {
        nfsm_try!('nfsmout, nfsm_strtom(&mut info, &mut mb, cnp.name(), NFS_MAXNAMLEN));
        if v3 {
            nfsm_v3attrbuild(&mut mb, vap, false);
        }
        nfsm_try!('nfsmout, nfsm_strtom(&mut info, &mut mb, target, NFS_MAXPATHLEN));
        if !v3 {
            let sp = nfs_v2sattr(
                vtonfsv2_mode(VLNK, vap.va_mode),
                nfs_xdrneg1,
                nfs_xdrneg1,
                nfs_xdrneg1,
                vap,
            );
            nfsm_build(&mut mb, NFSX_V2SATTR).write(0, &sp);
        }

        info.nmi_procp = cn_proc(cnp);
        info.nmi_cred = cnp.cn_cred;
        let error = nfs_request(dvp, NFSPROC_SYMLINK, &mut info);
        if v3 {
            if error.is_ok() {
                nfsm_try!('nfsmout, nfsm_mtofh(&mut info, dvp, &mut newvp, &mut gotvp));
            }
            nfsm_try!('nfsmout, nfsm_wcc_data(&mut info, &mut dvp, &mut wccflag));
        }
        m_freem_reply(&mut info);
        error
    };

    // nfsmout:
    if let Some(nvp) = newvp {
        vput(nvp);
    }
    pnbuf_free(cnp);
    VTONFS(dvp).set(NMODIFIED);
    if wccflag == 0 {
        nfs_invalidate_attrcache(VTONFS(dvp));
    }
    VN_KNOTE(dvp, NOTE_WRITE);
    vput(dvp);
    error
}

/// `nfs_mkdir` (`vop_mkdir`): nfs make dir call.
pub fn nfs_mkdir(ap: &mut VopMkdirArgs<'_>) -> Result<(), Errno> {
    let mut dvp = ap.a_dvp;
    let vap = &*ap.a_vap;
    let cnp = &mut *ap.a_cnp;
    let mut info = NfsmInfo::new();
    let mut newvp: Option<&'static Vnode> = None;
    let mut wccflag = NFSV3_WCCRATTR;
    let mut gotvp = 0;

    info.nmi_v3 = nfs_isv3(dvp);
    let v3 = info.nmi_v3;

    let len = cnp.name().len();
    NFSSTATS.rpccnt[NFSPROC_MKDIR].fetch_add(1, Ordering::Relaxed);
    let req = nfsm_reqhead(nfsx_fh(v3) + NFSX_UNSIGNED + nfsm_rndup(len) + nfsx_sattr(v3));
    info.nmi_mreq = Some(req);
    let mut mb = req;
    nfsm_fhtom(&mut mb, dvp, v3);
    let mut error: Result<(), Errno> = 'nfsmout: {
        nfsm_try!('nfsmout, nfsm_strtom(&mut info, &mut mb, cnp.name(), NFS_MAXNAMLEN));

        if v3 {
            nfsm_v3attrbuild(&mut mb, vap, false);
        } else {
            let sp = nfs_v2sattr(
                vtonfsv2_mode(VDIR, vap.va_mode),
                nfs_xdrneg1,
                nfs_xdrneg1,
                nfs_xdrneg1,
                vap,
            );
            nfsm_build(&mut mb, NFSX_V2SATTR).write(0, &sp);
        }

        info.nmi_procp = cn_proc(cnp);
        info.nmi_cred = cnp.cn_cred;
        let error = nfs_request(dvp, NFSPROC_MKDIR, &mut info);
        if error.is_ok() {
            nfsm_try!('nfsmout, nfsm_mtofh(&mut info, dvp, &mut newvp, &mut gotvp));
        }
        if v3 {
            nfsm_try!('nfsmout, nfsm_wcc_data(&mut info, &mut dvp, &mut wccflag));
        }
        m_freem_reply(&mut info);
        error
    };

    // nfsmout:
    VTONFS(dvp).set(NMODIFIED);
    if wccflag == 0 {
        nfs_invalidate_attrcache(VTONFS(dvp));
    }

    if error.is_ok() && newvp.is_none() {
        let mut np = None;
        error = nfs_lookitup(dvp, cnp.name(), cnp.cn_cred, cn_proc(cnp), Some(&mut np));
        if error.is_ok()
            && let Some(np) = np
        {
            let nvp = NFSTOV(np);
            newvp = Some(nvp);
            if nvp.v_type.get() != VDIR {
                error = Err(Errno::EEXIST);
            }
        }
    }
    if error.is_err() {
        if let Some(nvp) = newvp {
            vput(nvp);
        }
    } else {
        VN_KNOTE(dvp, NOTE_WRITE | NOTE_LINK);
        if cnp.cn_flags & MAKEENTRY != 0 {
            nfs_cache_enter(dvp, newvp, cnp);
        }
        *ap.a_vpp = newvp;
    }
    pnbuf_free(cnp);
    vput(dvp);
    error
}

/// `nfs_rmdir` (`vop_rmdir`): nfs remove directory call.
pub fn nfs_rmdir(ap: &mut VopRmdirArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let mut dvp = ap.a_dvp;
    let cnp = &mut *ap.a_cnp;
    let mut info = NfsmInfo::new();
    let mut wccflag = NFSV3_WCCRATTR;

    info.nmi_v3 = nfs_isv3(dvp);

    NFSSTATS.rpccnt[NFSPROC_RMDIR].fetch_add(1, Ordering::Relaxed);
    let req = nfsm_reqhead(nfsx_fh(info.nmi_v3) + NFSX_UNSIGNED + nfsm_rndup(cnp.name().len()));
    info.nmi_mreq = Some(req);
    let mut mb = req;
    nfsm_fhtom(&mut mb, dvp, info.nmi_v3);
    let error: Result<(), Errno> = 'nfsmout: {
        nfsm_try!('nfsmout, nfsm_strtom(&mut info, &mut mb, cnp.name(), NFS_MAXNAMLEN));

        info.nmi_procp = cn_proc(cnp);
        info.nmi_cred = cnp.cn_cred;
        let error = nfs_request(dvp, NFSPROC_RMDIR, &mut info);
        if info.nmi_v3 {
            nfsm_try!('nfsmout, nfsm_wcc_data(&mut info, &mut dvp, &mut wccflag));
        }
        m_freem_reply(&mut info);
        error
    };

    // nfsmout:
    pnbuf_free(cnp);
    VTONFS(dvp).set(NMODIFIED);
    if wccflag == 0 {
        nfs_invalidate_attrcache(VTONFS(dvp));
    }

    VN_KNOTE(dvp, NOTE_WRITE | NOTE_LINK);
    VN_KNOTE(vp, NOTE_DELETE);

    cache_purge(vp);
    vput(vp);
    vput(dvp);
    // Kludge: Map ENOENT => 0 assuming that you have a reply to a retry.
    if error == Err(Errno::ENOENT) {
        return Ok(());
    }
    error
}

// The readdir logic below has a big design bug. It stores the NFS cookie in the returned
// uio->uio_offset but does not store the verifier (it cannot). Instead, the code stores the
// verifier in the nfsnode and applies that verifies to all cookies, no matter what verifier
// was originally with the cookie.
//
// From a practical standpoint, this is not a problem since almost all NFS servers do not
// change the validity of cookies across deletes and inserts.

/// The fix-up `nfs_readdir` applies to a packed `nfs_dirent` record `rec` before it copies
/// its `struct dirent` out: `d_reclen` loses the cookie overhead and `d_off` becomes the
/// cookie. Returns the cookie, the record's packed length and its new `d_reclen`; `EBADRPC`
/// when the name holds a `/`.
fn nfs_dirent_fixup(rec: &mut [u8]) -> Result<(u64, usize, usize), Errno> {
    let reclen = usize::from(u16::from_ne_bytes([rec[ND_RECLEN], rec[ND_RECLEN + 1]]));
    let d_reclen = reclen.wrapping_sub(NFS_DIRENT_OVERHEAD);
    rec[ND_RECLEN..ND_RECLEN + 2].copy_from_slice(&(d_reclen as u16).to_ne_bytes());
    let word =
        |off: usize| u32::from_ne_bytes([rec[off], rec[off + 1], rec[off + 2], rec[off + 3]]);
    let cookie = fxdr_hyper([word(ND_COOKIE), word(ND_COOKIE + 4)]);
    rec[ND_OFF..ND_OFF + 8].copy_from_slice(&(cookie as Off).to_ne_bytes());

    let namlen = usize::from(rec[ND_NAMLEN]);
    if rec[ND_NAME..ND_NAME + namlen].contains(&b'/') {
        return Err(Errno::EBADRPC);
    }
    Ok((cookie, reclen, d_reclen))
}

/// `nfs_readdir` (`vop_readdir`): nfs readdir call.
pub fn nfs_readdir(ap: &mut VopReaddirArgs<'_, '_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let np = VTONFS(vp);
    let uio = &mut *ap.a_uio;
    let mut newoff = uio.uio_offset as u64;
    let nmp = vfstonfs_vp(vp);
    let p = uio.uio_procp;
    let mut done = false;
    let mut eof = false;
    let cred = ap.a_cred;

    if vp.v_type.get() != VDIR {
        return Err(Errno::EPERM);
    }
    // First, check for hit on the EOF offset cache
    if np.n_direofoffset.get() != 0 && uio.uio_offset == np.n_direofoffset.get() {
        let mut vattr = Vattr::new();
        if VOP_GETATTR(vp, &mut vattr, ap.a_cred, nfs_proc(uio.uio_procp)).is_ok()
            && np.n_mtime.get() == vattr.va_mtime
        {
            NFSSTATS.direofcache_hits.fetch_add(1, Ordering::Relaxed);
            *ap.a_eofflag = 1;
            return Ok(());
        }
    }

    if uio.uio_resid < NFS_FABLKSIZE as usize {
        return Err(Errno::EINVAL);
    }

    let tresid = uio.uio_resid;

    if uio.uio_rw != UioRw::UIO_READ {
        return Err(Errno::EINVAL);
    }

    if nmp.nm_flag.get() & (NFSMNT_NFSV3 | NFSMNT_GOTFSINFO) == NFSMNT_NFSV3 {
        let _ = nfs_fsinfo(nmp, vp, cred, p);
    }

    let mut cnt = 5;

    // M_ZERO to avoid leaking kernel data in dirent padding
    let dirblksiz = NFS_DIRBLKSIZ as usize;
    let Some(data) = malloc(dirblksiz, M_TEMP, M_WAITOK | M_ZERO) else {
        panic(format_args!("nfs_readdir: malloc"));
    };
    // SAFETY: a fresh, zeroed `malloc(9)` block of `NFS_DIRBLKSIZ` bytes that this function
    // owns until the `free` below; only this slice and the uio built over it reach it.
    let buf = unsafe { core::slice::from_raw_parts_mut(data.as_ptr(), dirblksiz) };
    let mut error = Ok(());
    loop {
        let mut readdir_iovec = [Iovec {
            iov_base: buf.as_mut_ptr().cast::<c_void>(),
            iov_len: dirblksiz,
        }];
        let mut readdir_uio = Uio {
            uio_iov: &mut readdir_iovec,
            uio_offset: newoff as Off,
            uio_resid: dirblksiz,
            uio_segflg: UioSeg::UIO_SYSSPACE,
            uio_rw: UioRw::UIO_READ,
            uio_procp: curproc(),
        };

        if nmp.nm_flag.get() & NFSMNT_RDIRPLUS != 0 {
            error = nfs_readdirplusrpc(vp, &mut readdir_uio, cred, &mut eof, p);
            if let Err(e) = error
                && nfs_errno_is(e, NFSERR_NOTSUPP)
            {
                nmp.nm_flag.set(nmp.nm_flag.get() & !NFSMNT_RDIRPLUS);
            }
        }
        if nmp.nm_flag.get() & NFSMNT_RDIRPLUS == 0 {
            error = nfs_readdirrpc(vp, &mut readdir_uio, cred, &mut eof);
        }

        if let Err(e) = error
            && nfs_errno_is(e, NFSERR_BAD_COOKIE)
        {
            error = Err(Errno::EINVAL);
        }

        // The records end where the RPC left the uio's iovec.
        let filled = dirblksiz - readdir_uio.uio_resid;
        let mut ndp = 0;
        while error.is_ok() && ndp < filled {
            let (cookie, reclen, d_reclen) = match nfs_dirent_fixup(&mut buf[ndp..]) {
                Ok(r) => r,
                Err(e) => {
                    error = Err(e);
                    break;
                }
            };

            if uio.uio_resid < d_reclen {
                eof = false;
                done = true;
                break;
            }

            let dp = ndp + NFS_DIRENT_OVERHEAD;
            if let Err(e) = uiomove(&mut buf[dp..dp + d_reclen], uio) {
                error = Err(e);
                break;
            }

            newoff = cookie;

            ndp += reclen;
        }
        // while (!error && !done && !eof && cnt--)
        if error.is_err() || done || eof || cnt == 0 {
            break;
        }
        cnt -= 1;
    }

    free(data, M_TEMP, dirblksiz);

    uio.uio_offset = newoff as Off;

    if error.is_ok() && (eof || uio.uio_resid == tresid) {
        NFSSTATS.direofcache_misses.fetch_add(1, Ordering::Relaxed);
        *ap.a_eofflag = 1;
        return Ok(());
    }

    *ap.a_eofflag = 0;
    error
}

/// The kernel buffer of a readdir RPC's uio: its first iovec, at most `uio_resid` bytes.
/// The readdir RPCs write the records there directly, as the C does.
///
/// # Safety
///
/// `uio` is a `UIO_SYSSPACE` uio whose first iovec is a kernel buffer of `iov_len` writable
/// bytes (a `UIO_SYSSPACE` uio's builder vouches for that, `sys/uio.rs`), and nothing else
/// reads or writes that buffer while the returned slice is in use.
unsafe fn nfs_uio_sysbuf<'b>(uio: &Uio<'_>) -> &'b mut [u8] {
    kassert!(uio.uio_segflg == UioSeg::UIO_SYSSPACE);
    let Some(iov) = uio.uio_iov.first() else {
        return &mut [];
    };
    let len = iov.iov_len.min(uio.uio_resid);
    if len == 0 || iov.iov_base.is_null() {
        return &mut [];
    }
    // SAFETY: the caller's contract: `len` writable bytes at `iov_base`, used only through
    // this slice.
    unsafe { core::slice::from_raw_parts_mut(iov.iov_base.cast::<u8>(), len) }
}

/// Advance a uio over `n` bytes of its first iovec (`iov_base += n; iov_len -= n;
/// uio_resid -= n`), what the readdir RPCs packed.
fn nfs_uio_advance(uio: &mut Uio<'_>, n: usize) {
    if n == 0 {
        return;
    }
    if let Some(iov) = uio.uio_iov.first_mut() {
        iov.iov_base = iov.iov_base.cast::<u8>().wrapping_add(n).cast::<c_void>();
        iov.iov_len -= n;
    }
    uio.uio_resid -= n;
}

/// `nfsm_mtouio` of a name into the bytes `dst`: copies `dst.len()` bytes of the reply and
/// skips their XDR padding.
fn nfsm_mtobuf(info: &mut NfsmInfo<'_>, dst: &mut [u8]) -> Result<(), Errno> {
    let len = dst.len();
    let mut iov = [Iovec {
        iov_base: dst.as_mut_ptr().cast::<c_void>(),
        iov_len: len,
    }];
    let mut uio = Uio {
        uio_iov: &mut iov,
        uio_offset: 0,
        uio_resid: len,
        uio_segflg: UioSeg::UIO_SYSSPACE,
        uio_rw: UioRw::UIO_READ,
        uio_procp: None,
    };
    nfsm_mtouio(info, &mut uio, len as i32)
}

/// `nfs_readdirrpc(vp, uiop, cred, end_of_directory)`: readdir rpc call. Loops doing
/// readdir rpc's of `nm_readdirsize` and packs their entries into `uiop`'s buffer as
/// `nfs_dirent` records (the cookies stuffed in before each `struct dirent`).
pub fn nfs_readdirrpc(
    vp: &'static Vnode,
    uiop: &mut Uio<'_>,
    cred: *const Ucred,
    end_of_directory: &mut bool,
) -> Result<(), Errno> {
    let mut wvp = vp;
    let nmp = vfstonfs_vp(vp);
    let dnp = VTONFS(vp);
    let v3 = nfs_isv3(vp);
    let mut more_dirs = true;
    let mut bigenough = true;
    let mut attrflag = 0;

    #[cfg(feature = "diagnostic")]
    if uiop.uio_iovcnt() != 1 || uiop.uio_resid & (NFS_DIRBLKSIZ as usize - 1) != 0 {
        panic(format_args!("nfs readdirrpc bad uio"));
    }

    let mut cookie = txdr_hyper(uiop.uio_offset as u64);
    let procp = uiop.uio_procp;
    // SAFETY: a `UIO_SYSSPACE` uio's iovecs are kernel buffers its builder vouches for
    // (`sys/uio.rs`, kasserted by `nfs_uio_sysbuf`); the caller (`nfs_readdir`) hands over a
    // buffer of its own that nothing else touches until this RPC returns, and the uio itself
    // is only advanced once the packing is over.
    let mut pack = NfsDirPack::new(unsafe { nfs_uio_sysbuf(uiop) });

    // Loop around doing readdir rpc's of size nm_readdirsize truncated to a multiple of
    // NFS_READDIRBLKSIZ. The stopping criteria is EOF or buffer full.
    let error: Result<(), Errno> = 'nfsmout: {
        while more_dirs && bigenough {
            let mut info = NfsmInfo::new();
            info.nmi_v3 = v3;

            NFSSTATS.rpccnt[NFSPROC_READDIR].fetch_add(1, Ordering::Relaxed);
            let req = nfsm_reqhead(nfsx_fh(v3) + nfsx_readdir(v3));
            info.nmi_mreq = Some(req);
            let mut mb = req;
            nfsm_fhtom(&mut mb, vp, v3);
            if v3 {
                let mut tl = nfsm_build(&mut mb, 5 * NFSX_UNSIGNED);
                tl.set(0, cookie[0]);
                tl.set(1, cookie[1]);
                let verf = if cookie == [0, 0] {
                    [0, 0]
                } else {
                    dnp.n_cookieverf.get().nfsuquad
                };
                tl.set(2, verf[0]);
                tl.set(3, verf[1]);
                tl.set(4, txdr_unsigned(nmp.nm_readdirsize.get() as u32));
            } else {
                let mut tl = nfsm_build(&mut mb, 2 * NFSX_UNSIGNED);
                tl.set(0, cookie[1]);
                tl.set(1, txdr_unsigned(nmp.nm_readdirsize.get() as u32));
            }

            info.nmi_procp = procp;
            info.nmi_cred = cred;
            let error = nfs_request(vp, NFSPROC_READDIR, &mut info);
            if v3 {
                nfsm_try!('nfsmout, nfsm_postop_attr(&mut info, &mut wvp, &mut attrflag));
            }

            if let Err(e) = error {
                m_freem_reply(&mut info);
                break 'nfsmout Err(e);
            }

            if v3 {
                let tl = nfsm_try!('nfsmout, nfsm_dissect(&mut info, 2 * NFSX_UNSIGNED));
                dnp.n_cookieverf.set(Nfsuint64 {
                    nfsuquad: [tl.get(0), tl.get(1)],
                });
            }

            let tl = nfsm_try!('nfsmout, nfsm_dissect(&mut info, NFSX_UNSIGNED));
            more_dirs = fxdr_unsigned(tl.get(0)) != 0;

            // loop thru the dir entries, doctoring them to dirent form
            while more_dirs && bigenough {
                let (fileno, len) = if v3 {
                    let tl = nfsm_try!('nfsmout, nfsm_dissect(&mut info, 3 * NFSX_UNSIGNED));
                    (tl.fxdr_hyper(0), fxdr_unsigned(tl.get(2)) as i32)
                } else {
                    let tl = nfsm_try!('nfsmout, nfsm_dissect(&mut info, 2 * NFSX_UNSIGNED));
                    (
                        u64::from(fxdr_unsigned(tl.get(0))),
                        fxdr_unsigned(tl.get(1)) as i32,
                    )
                };
                if len <= 0 || len as usize > NFS_MAXNAMLEN {
                    m_freem_reply(&mut info);
                    break 'nfsmout Err(Errno::EBADRPC);
                }
                let len = len as usize;
                bigenough = pack.begin(fileno, len);
                if bigenough {
                    nfsm_try!('nfsmout, nfsm_mtobuf(&mut info, pack.name_slot(len)));
                    pack.end_name(len); // null terminate
                } else {
                    nfsm_try!('nfsmout, nfsm_adv(&mut info, nfsm_rndup(len)));
                }
                let words = if v3 { 3 } else { 2 };
                let tl = nfsm_try!('nfsmout, nfsm_dissect(&mut info, words * NFSX_UNSIGNED));
                if bigenough {
                    let c0 = if v3 {
                        cookie[0] = tl.get(0);
                        cookie[0]
                    } else {
                        0
                    };
                    cookie[1] = tl.get(words - 2);
                    pack.set_cookie(c0, cookie[1]);
                }
                more_dirs = fxdr_unsigned(tl.get(words - 1)) != 0;
            }
            // If at end of rpc data, get the eof boolean
            if !more_dirs {
                let tl = nfsm_try!('nfsmout, nfsm_dissect(&mut info, NFSX_UNSIGNED));
                more_dirs = fxdr_unsigned(tl.get(0)) == 0;
            }
            m_freem_reply(&mut info);
        }
        // Fill last record, iff any, out to a multiple of NFS_READDIRBLKSIZ by increasing
        // d_reclen for the last record.
        pack.finish();

        // We are now either at the end of the directory or have filled the block.
        if bigenough {
            dnp.n_direofoffset.set(fxdr_hyper(cookie) as Off);
            *end_of_directory = true;
        } else if pack.resid() > 0 {
            printf(format_args!("EEK! readdirrpc resid > 0\n"));
        }
        Ok(())
    };

    // nfsmout:
    let packed = pack.pos;
    nfs_uio_advance(uiop, packed);
    error
}

/// `nfs_readdirplusrpc(vp, uiop, cred, end_of_directory, p)`: NFS V3 readdir plus RPC.
/// Used in place of `nfs_readdirrpc()`: the entries come with their attributes and file
/// handles, which fill the attribute and name caches.
pub fn nfs_readdirplusrpc(
    vp: &'static Vnode,
    uiop: &mut Uio<'_>,
    cred: *const Ucred,
    end_of_directory: &mut bool,
    p: Option<&Proc>,
) -> Result<(), Errno> {
    let mut wvp = vp;
    let nmp = vfstonfs_vp(vp);
    let dnp = VTONFS(vp);
    let mut newvp: Option<&'static Vnode> = None;
    let mut more_dirs = true;
    let mut bigenough = true;
    let mut attrflag = 0;

    #[cfg(feature = "diagnostic")]
    if uiop.uio_iovcnt() != 1 || uiop.uio_resid & (NFS_DIRBLKSIZ as usize - 1) != 0 {
        panic(format_args!("nfs readdirplusrpc bad uio"));
    }
    // NDINIT(ndp, 0, 0, UIO_SYSSPACE, NULL, p); ndp->ni_dvp = vp: the component name the
    // entries are entered in the name cache with.
    let mut cn = Componentname::new();
    cn.cn_proc = p.map_or(ptr::null(), ptr::from_ref);
    let ni_dvp = vp;

    let mut cookie = txdr_hyper(uiop.uio_offset as u64);
    let procp = uiop.uio_procp;
    // SAFETY: a `UIO_SYSSPACE` uio's iovecs are kernel buffers its builder vouches for
    // (`sys/uio.rs`, kasserted by `nfs_uio_sysbuf`); the caller (`nfs_readdir`) hands over a
    // buffer of its own that nothing else touches until this RPC returns, and the uio itself
    // is only advanced once the packing is over.
    let mut pack = NfsDirPack::new(unsafe { nfs_uio_sysbuf(uiop) });

    // Loop around doing readdir rpc's of size nm_readdirsize truncated to a multiple of
    // NFS_READDIRBLKSIZ. The stopping criteria is EOF or buffer full.
    let error: Result<(), Errno> = 'nfsmout: {
        let mut error = Ok(());
        while more_dirs && bigenough {
            let mut info = NfsmInfo::new();
            info.nmi_v3 = true;

            NFSSTATS.rpccnt[NFSPROC_READDIRPLUS].fetch_add(1, Ordering::Relaxed);
            let req = nfsm_reqhead(nfsx_fh(true) + 6 * NFSX_UNSIGNED);
            info.nmi_mreq = Some(req);
            let mut mb = req;
            nfsm_fhtom(&mut mb, vp, true);
            let mut tl = nfsm_build(&mut mb, 6 * NFSX_UNSIGNED);
            tl.set(0, cookie[0]);
            tl.set(1, cookie[1]);
            let verf = if cookie == [0, 0] {
                [0, 0]
            } else {
                dnp.n_cookieverf.get().nfsuquad
            };
            tl.set(2, verf[0]);
            tl.set(3, verf[1]);
            tl.set(4, txdr_unsigned(nmp.nm_readdirsize.get() as u32));
            tl.set(5, txdr_unsigned(nmp.nm_rsize.get() as u32));

            info.nmi_procp = procp;
            info.nmi_cred = cred;
            error = nfs_request(vp, NFSPROC_READDIRPLUS, &mut info);
            nfsm_try!('nfsmout, nfsm_postop_attr(&mut info, &mut wvp, &mut attrflag));
            if let Err(e) = error {
                m_freem_reply(&mut info);
                break 'nfsmout Err(e);
            }

            let tl = nfsm_try!('nfsmout, nfsm_dissect(&mut info, 3 * NFSX_UNSIGNED));
            dnp.n_cookieverf.set(Nfsuint64 {
                nfsuquad: [tl.get(0), tl.get(1)],
            });
            more_dirs = fxdr_unsigned(tl.get(2)) != 0;

            // loop thru the dir entries, doctoring them to 4bsd form
            while more_dirs && bigenough {
                let tl = nfsm_try!('nfsmout, nfsm_dissect(&mut info, 3 * NFSX_UNSIGNED));
                let fileno = tl.fxdr_hyper(0);
                let len = fxdr_unsigned(tl.get(2)) as i32;
                if len <= 0 || len as usize > NFS_MAXNAMLEN {
                    m_freem_reply(&mut info);
                    break 'nfsmout Err(Errno::EBADRPC);
                }
                let len = len as usize;
                bigenough = pack.begin(fileno, len);
                if bigenough {
                    let slot = pack.name_slot(len);
                    cn.cn_nameptr = slot.as_ptr();
                    cn.cn_namelen = len as i64;
                    nfsm_try!('nfsmout, nfsm_mtobuf(&mut info, slot));
                    pack.end_name(len);
                } else {
                    nfsm_try!('nfsmout, nfsm_adv(&mut info, nfsm_rndup(len)));
                }
                let tl = nfsm_try!('nfsmout, nfsm_dissect(&mut info, 3 * NFSX_UNSIGNED));
                if bigenough {
                    cookie = [tl.get(0), tl.get(1)];
                    pack.set_cookie(cookie[0], cookie[1]);
                }

                // Since the attributes are before the file handle (sigh), we must skip over
                // the attributes and then come back and get them.
                let attrflag = fxdr_unsigned(tl.get(2)) as i32;
                if attrflag != 0 {
                    let dpossav1 = info.nmi_dpos;
                    let mdsav1 = info.nmi_md;
                    nfsm_try!('nfsmout, nfsm_adv(&mut info, NFSX_V3FATTR));
                    let tl = nfsm_try!('nfsmout, nfsm_dissect(&mut info, NFSX_UNSIGNED));
                    let mut doit = fxdr_unsigned(tl.get(0)) != 0;
                    let mut np = dnp;
                    if doit {
                        let fh = nfsm_try!('nfsmout, nfsm_getfh(&mut info, true));
                        if nfs_cmpfh(dnp, fh.bytes()) {
                            vref(vp);
                            newvp = Some(vp);
                            np = dnp;
                        } else {
                            match nfs_nget(vmount(vp), fh.bytes()) {
                                Err(e) => {
                                    error = Err(e);
                                    doit = false;
                                }
                                Ok(n) => {
                                    newvp = Some(NFSTOV(n));
                                    np = n;
                                }
                            }
                        }
                    }
                    if doit
                        && bigenough
                        && let Some(mut nvp) = newvp
                    {
                        let dpossav2 = info.nmi_dpos;
                        info.nmi_dpos = dpossav1;
                        let mdsav2 = info.nmi_md;
                        info.nmi_md = mdsav1;
                        nfsm_try!('nfsmout, nfsm_loadattr(&mut info, &mut nvp, None));
                        newvp = Some(nvp);
                        info.nmi_dpos = dpossav2;
                        info.nmi_md = mdsav2;
                        pack.set_type(iftodt(vttoif(np.n_vattr.get().va_type)));
                        if cn.cn_namelen as usize <= NAMECACHE_MAXLEN {
                            // ndp->ni_vp = newvp
                            cache_purge(ni_dvp);
                            nfs_cache_enter(ni_dvp, newvp, &cn);
                        }
                    }
                } else {
                    // Just skip over the file handle
                    let tl = nfsm_try!('nfsmout, nfsm_dissect(&mut info, NFSX_UNSIGNED));
                    let i = fxdr_unsigned(tl.get(0)) as i32;
                    if i > 0 {
                        nfsm_try!('nfsmout, nfsm_adv(&mut info, nfsm_rndup(i as usize)));
                    }
                }
                if let Some(nvp) = newvp.take() {
                    vput_or_vrele(nvp, vp);
                }
                let tl = nfsm_try!('nfsmout, nfsm_dissect(&mut info, NFSX_UNSIGNED));
                more_dirs = fxdr_unsigned(tl.get(0)) != 0;
            }
            // If at end of rpc data, get the eof boolean
            if !more_dirs {
                let tl = nfsm_try!('nfsmout, nfsm_dissect(&mut info, NFSX_UNSIGNED));
                more_dirs = fxdr_unsigned(tl.get(0)) == 0;
            }
            m_freem_reply(&mut info);
        }
        // Fill last record, iff any, out to a multiple of NFS_READDIRBLKSIZ by increasing
        // d_reclen for the last record.
        pack.finish();

        // We are now either at the end of the directory or have filled the block.
        if bigenough {
            dnp.n_direofoffset.set(fxdr_hyper(cookie) as Off);
            *end_of_directory = true;
        } else if pack.resid() > 0 {
            printf(format_args!("EEK! readdirplusrpc resid > 0\n"));
        }
        error
    };

    // nfsmout:
    if let Some(nvp) = newvp {
        vput_or_vrele(nvp, vp);
    }
    let packed = pack.pos;
    nfs_uio_advance(uiop, packed);
    error
}

/// `nfs_sillyname`: the funny name `".nfs%08X%08X"` of a silly rename, made of two random
/// words, NUL-terminated in a `s_name` buffer; with its length.
fn nfs_sillyname(rnd: [u32; 2]) -> ([u8; 24], usize) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut name = [0u8; 24];
    name[..4].copy_from_slice(b".nfs");
    let mut n = 4;
    for w in rnd {
        for shift in (0..8).rev() {
            name[n] = HEX[((w >> (shift * 4)) & 0xf) as usize];
            n += 1;
        }
    }
    (name, n)
}

/// `nfs_sillyrename(dvp, vp, cnp)`: silly rename. To make the NFS filesystem that is
/// stateless look a little more like the "ufs" a remove of an active vnode is translated to
/// a rename to a funny looking filename that is removed by `nfs_inactive` on the nfsnode.
/// There is the potential for another process on a different client to create the same
/// funny name between the `nfs_lookitup()` fails and the `nfs_rename()` completes, but...
pub fn nfs_sillyrename(
    dvp: &'static Vnode,
    vp: &'static Vnode,
    cnp: &Componentname,
) -> Result<(), Errno> {
    cache_purge(dvp);
    let np = VTONFS(vp);
    let Some(mem) = malloc(size_of::<Sillyrename>(), M_NFSREQ, M_WAITOK) else {
        panic(format_args!("nfs_sillyrename: malloc"));
    };
    let mut sp = Sillyrename {
        s_cred: crdup(cnp.cred()),
        s_dvp: dvp,
        s_namlen: 0,
        s_name: [0; 24],
    };
    vref(dvp);

    let error = 'bad: {
        if vp.v_type.get() == VDIR {
            #[cfg(feature = "diagnostic")]
            printf(format_args!("nfs: sillyrename dir\n"));
            break 'bad Err(Errno::EINVAL);
        }

        // Try lookitups until we get one that isn't there
        loop {
            // Fudge together a funny name
            let mut rnd = [0u8; 8];
            crate::dev::rnd::arc4random_buf(&mut rnd);
            let (name, len) = nfs_sillyname([
                u32::from_ne_bytes([rnd[0], rnd[1], rnd[2], rnd[3]]),
                u32::from_ne_bytes([rnd[4], rnd[5], rnd[6], rnd[7]]),
            ]);
            sp.s_name = name;
            sp.s_namlen = len as i64;

            if nfs_lookitup(
                dvp,
                &sp.s_name[..len],
                ptr::from_ref(sp.s_cred),
                cn_proc(cnp),
                None,
            )
            .is_err()
            {
                break;
            }
        }

        if let Err(e) = nfs_renameit(dvp, cnp, &sp) {
            break 'bad Err(e);
        }
        let mut npp = Some(np);
        let _ = nfs_lookitup(
            dvp,
            &sp.s_name[..sp.s_namlen as usize],
            ptr::from_ref(sp.s_cred),
            cn_proc(cnp),
            Some(&mut npp),
        );
        let np = npp.unwrap_or(np);
        let spp = mem.cast::<Sillyrename>();
        // SAFETY: a fresh `malloc(9)` block of `size_of::<Sillyrename>()` bytes, aligned by
        // malloc, written once before the node publishes it; `nfs_inactive` frees it.
        unsafe { spp.as_ptr().write(sp) };
        np.n_sillyrename.set(Some(spp));
        return Ok(());
    };

    // bad:
    let _ = vrele(sp.s_dvp);
    crfree(sp.s_cred);
    free(mem, M_NFSREQ, size_of::<Sillyrename>());
    error
}

/// `nfs_lookitup(dvp, name, len, cred, procp, npp)`: look up a file name and optionally
/// either update the file handle or allocate an nfsnode, depending on the value of `npp`:
/// - `npp == NULL` (`None`): just do the lookup;
/// - `*npp == NULL`: allocate a new nfsnode and make sure attributes are handled too;
/// - `*npp != NULL`: update the file handle in the vnode.
pub fn nfs_lookitup(
    dvp: &'static Vnode,
    name: &[u8],
    cred: *const Ucred,
    procp: Option<&Proc>,
    npp: Option<&mut Option<&'static NfsNode>>,
) -> Result<(), Errno> {
    let mut info = NfsmInfo::new();
    let mut newvp: Option<&'static Vnode> = None;
    let mut np: Option<&'static NfsNode> = None;
    let dnp = VTONFS(dvp);
    let mut attrflag = 0;

    info.nmi_v3 = nfs_isv3(dvp);
    let v3 = info.nmi_v3;
    // npp, and *npp when there is one.
    let want = npp.is_some();
    let given: Option<&'static NfsNode> = npp.as_ref().and_then(|n| **n);

    NFSSTATS.rpccnt[NFSPROC_LOOKUP].fetch_add(1, Ordering::Relaxed);
    let req = nfsm_reqhead(nfsx_fh(v3) + NFSX_UNSIGNED + nfsm_rndup(name.len()));
    info.nmi_mreq = Some(req);
    let mut mb = req;
    nfsm_fhtom(&mut mb, dvp, v3);
    let error: Result<(), Errno> = 'nfsmout: {
        nfsm_try!('nfsmout, nfsm_strtom(&mut info, &mut mb, name, NFS_MAXNAMLEN));

        info.nmi_procp = procp;
        info.nmi_cred = cred;
        let error = nfs_request(dvp, NFSPROC_LOOKUP, &mut info);
        if error.is_err() && !v3 {
            m_freem_reply(&mut info);
            break 'nfsmout error;
        }

        if want && error.is_ok() {
            let nfhp = nfsm_try!('nfsmout, nfsm_getfh(&mut info, v3));
            let mut nvp;
            if let Some(n) = given {
                n.set_fh(nfhp.bytes());
                np = Some(n);
                nvp = NFSTOV(n);
            } else if nfs_cmpfh(dnp, nfhp.bytes()) {
                vref(dvp);
                nvp = dvp;
                np = Some(dnp);
            } else {
                match nfs_nget(vmount(dvp), nfhp.bytes()) {
                    Ok(n) => {
                        np = Some(n);
                        nvp = NFSTOV(n);
                    }
                    Err(e) => {
                        m_freem_reply(&mut info);
                        return Err(e);
                    }
                }
            }
            newvp = Some(nvp);
            if v3 {
                nfsm_try!('nfsmout, nfsm_postop_attr(&mut info, &mut nvp, &mut attrflag));
                newvp = Some(nvp);
                if attrflag == 0 && given.is_none() {
                    m_freem_reply(&mut info);
                    vput_or_vrele(nvp, dvp);
                    return Err(Errno::ENOENT);
                }
            } else {
                nfsm_try!('nfsmout, nfsm_loadattr(&mut info, &mut nvp, None));
                newvp = Some(nvp);
            }
        }
        m_freem_reply(&mut info);
        error
    };

    // nfsmout:
    if let Some(npp) = npp
        && npp.is_none()
    {
        if error.is_err() {
            if let Some(nvp) = newvp {
                vput_or_vrele(nvp, dvp);
            }
        } else {
            *npp = np;
        }
    }
    error
}

/// `nfs_commit(vp, offset, cnt, procp)`: Nfs Version 3 commit rpc: commit `cnt` bytes at
/// `offset` to stable storage. The error is the C's int: an errno number, or
/// `NFSERR_STALEWRITEVERF` when the server's write verifier changed.
pub fn nfs_commit(
    vp: &'static Vnode,
    offset: u64,
    cnt: i32,
    procp: Option<&Proc>,
) -> Result<(), i32> {
    let mut wvp = vp;
    let nmp = vfstonfs_vp(vp);
    let mut info = NfsmInfo::new();
    let mut wccflag = NFSV3_WCCRATTR;

    if nmp.nm_flag.get() & NFSMNT_HASWRITEVERF == 0 {
        return Ok(());
    }
    NFSSTATS.rpccnt[NFSPROC_COMMIT].fetch_add(1, Ordering::Relaxed);
    info.nmi_v3 = true;
    let req = nfsm_reqhead(nfsx_fh(true));
    info.nmi_mreq = Some(req);
    let mut mb = req;
    nfsm_fhtom(&mut mb, vp, true);

    let mut tl = nfsm_build(&mut mb, 3 * NFSX_UNSIGNED);
    tl.txdr_hyper(0, offset);
    tl.set(2, txdr_unsigned(cnt as u32));

    info.nmi_procp = procp;
    info.nmi_cred = VTONFS(vp).n_wcred.get();
    // Ok(true): the verifier changed.
    let r: Result<bool, Errno> = 'nfsmout: {
        let error = nfs_request(vp, NFSPROC_COMMIT, &mut info);
        nfsm_try!('nfsmout, nfsm_wcc_data(&mut info, &mut wvp, &mut wccflag));

        let mut stale = false;
        if error.is_ok() {
            let tl = nfsm_try!('nfsmout, nfsm_dissect(&mut info, NFSX_V3WRITEVERF));
            let mut verf = [0u8; NFSX_V3WRITEVERF];
            verf.copy_from_slice(&tl.bytes()[..NFSX_V3WRITEVERF]);
            if nmp.nm_verf.get() != verf {
                nmp.nm_verf.set(verf);
                stale = true;
            }
        }
        m_freem_reply(&mut info);
        error.map(|()| stale)
    };

    // nfsmout:
    match r {
        Ok(false) => Ok(()),
        Ok(true) => Err(NFSERR_STALEWRITEVERF),
        Err(e) => Err(e.as_i32()),
    }
}

// Kludge City..
// - make nfs_bmap() essentially a no-op that does no translation
// - do nfs_strategy() by doing I/O with nfs_readrpc/nfs_writerpc
//   (Maybe I could use the process's page mapping, but I was concerned that Kernel Write
//    might not be enabled and also figured copyout() would do a lot more work than bcopy()
//    and also it currently happens in the context of the swapper process (2).

/// `nfs_bmap` (`vop_bmap`): no translation: the vnode itself, the logical block in
/// `DEV_BSIZE` units.
pub fn nfs_bmap(ap: &mut VopBmapArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;

    if let Some(vpp) = ap.a_vpp.as_deref_mut() {
        *vpp = Some(vp);
    }
    if let Some(bnp) = ap.a_bnp.as_deref_mut() {
        let iosize = vmount(vp).mnt_stat.get().f_iosize as usize;
        *bnp = ap.a_bn * btodb(iosize) as Daddr;
    }
    Ok(())
}

/// `nfs_strategy` (`vop_strategy`): strategy routine. For async requests when nfsiod(s) are
/// running, queue the request by calling `nfs_asyncio()`, otherwise just call `nfs_doio()`
/// to do the request.
pub fn nfs_strategy(ap: &mut VopStrategyArgs) -> Result<(), Errno> {
    let bp = ap.a_bp;
    let mut error = Ok(());

    if bp.b_flags.get() & (B_PHYS | B_ASYNC) == (B_PHYS | B_ASYNC) {
        panic(format_args!("nfs physio/async"));
    }
    let p = if bp.isset(B_ASYNC) {
        None
    } else {
        curproc() // XXX
    };
    // If the op is asynchronous and an i/o daemon is waiting queue the request, wake it up
    // and wait for completion otherwise just do it ourselves.
    if !bp.isset(B_ASYNC) || nfs_asyncio(bp, false).is_err() {
        error = nfs_doio(bp, p);
    }
    error
}

/// `nfs_ioctl` (`vop_ioctl`): no ioctls.
pub fn nfs_ioctl(_ap: &mut VopIoctlArgs<'_>) -> Result<(), Errno> {
    Err(Errno::ENOTTY)
}

/// `nfs_fsync` (`vop_fsync`): fsync vnode op. Just call `nfs_flush()` with commit == 1.
pub fn nfs_fsync(ap: &mut VopFsyncArgs<'_>) -> Result<(), Errno> {
    nfs_flush(ap.a_vp, ap.a_cred, ap.a_waitfor, Some(ap.a_p), true)
}

/// `nfs_flush(vp, cred, waitfor, p, commit)`: flush all the blocks associated with a vnode.
/// Walk through the buffer pool and push any dirty pages associated with the vnode.
pub fn nfs_flush(
    vp: &'static Vnode,
    _cred: *const Ucred,
    waitfor: i32,
    p: Option<&Proc>,
    commit: bool,
) -> Result<(), Errno> {
    let np = VTONFS(vp);
    let nmp = vfstonfs_vp(vp);
    let mut slptimeo = INFSLP;
    let mut slpflag = 0;
    let mut passone = true;
    let mut off = u64::MAX;
    let mut endoff = 0u64;
    let mut bvec: [Option<&'static Buf>; NFS_COMMITBVECSIZ] = [None; NFS_COMMITBVECSIZ];

    if nmp.nm_flag.get() & NFSMNT_INT != 0 {
        slpflag = PCATCH;
    }
    if !commit {
        passone = false;
    }
    // A b_flags == (B_DELWRI | B_NEEDCOMMIT) block has been written to the server, but nas
    // not been committed to stable storage on the server yet. On the first pass, the byte
    // range is worked out and the commit rpc is done. On the second pass, nfs_writebp() is
    // called to do the job.
    'again: loop {
        let mut bvecpos = 0;
        if nfs_isv3(vp) && commit {
            let s = splbio();
            for bp in vp.v_dirtyblkhd.iter() {
                if bvecpos >= NFS_COMMITBVECSIZ {
                    break;
                }
                if bp.b_flags.get() & (B_BUSY | B_DELWRI | B_NEEDCOMMIT)
                    != (B_DELWRI | B_NEEDCOMMIT)
                {
                    continue;
                }
                bufcache_take(bp);
                bp.set(B_WRITEINPROG);
                buf_acquire(bp);

                // A list of these buffers is kept so that the second loop knows which
                // buffers have actually been committed. This is necessary, since there may
                // be a race between the commit rpc and new uncommitted writes on the file.
                bvec[bvecpos] = Some(bp);
                bvecpos += 1;
                let mut toff = (bp.b_blkno.get() as u64)
                    .wrapping_mul(DEV_BSIZE as u64)
                    .wrapping_add(bp.b_dirtyoff.get() as u64);
                if toff < off {
                    off = toff;
                }
                toff = toff.wrapping_add((bp.b_dirtyend.get() - bp.b_dirtyoff.get()) as u64);
                if toff > endoff {
                    endoff = toff;
                }
            }
            splx(s);
        }
        if bvecpos > 0 {
            // Commit data on the server, as required.
            BCSTATS.pendingwrites.fetch_add(1, Ordering::Relaxed);
            BCSTATS.numwrites.fetch_add(1, Ordering::Relaxed);
            let retv = nfs_commit(vp, off, endoff.wrapping_sub(off) as i32, p);
            if retv == Err(NFSERR_STALEWRITEVERF) {
                nfs_clearcommit(vmount(vp));
            }
            // Now, either mark the blocks I/O done or mark the blocks dirty, depending on
            // whether the commit succeeded.
            for (i, bp) in bvec[..bvecpos].iter().flatten().enumerate() {
                bp.clr(B_NEEDCOMMIT | B_WRITEINPROG);
                if retv.is_err() {
                    if i == 0 {
                        BCSTATS.pendingwrites.fetch_sub(1, Ordering::Relaxed);
                    }
                    brelse(bp);
                } else {
                    if i > 0 {
                        BCSTATS.pendingwrites.fetch_add(1, Ordering::Relaxed);
                    }
                    let s = splbio();
                    buf_undirty(bp);
                    vp.v_numoutput.set(vp.v_numoutput.get() + 1);
                    bp.set(B_ASYNC);
                    bp.clr(B_READ | B_DONE | B_ERROR);
                    bp.b_dirtyoff.set(0);
                    bp.b_dirtyend.set(0);
                    biodone(bp);
                    splx(s);
                }
            }
        }

        // Start/do any write(s) that are required.
        'loop_: loop {
            let s = splbio();
            for bp in vp.v_dirtyblkhd.iter() {
                if bp.isset(B_BUSY) {
                    if waitfor != MNT_WAIT || passone {
                        continue;
                    }
                    bp.set(B_WANTED);
                    let error = tsleep_nsec(
                        ptr::from_ref(bp),
                        slpflag | (PRIBIO + 1),
                        "nfsfsync",
                        slptimeo,
                    );
                    splx(s);
                    if error.is_err() {
                        if nfs_sigintr(nmp, None, p).is_err() {
                            return Err(Errno::EINTR);
                        }
                        if slpflag == PCATCH {
                            slpflag = 0;
                            slptimeo = sec_to_nsec(2);
                        }
                    }
                    continue 'loop_;
                }
                if !bp.isset(B_DELWRI) {
                    panic(format_args!("nfs_fsync: not dirty"));
                }
                if (passone || !commit) && bp.isset(B_NEEDCOMMIT) {
                    continue;
                }
                bufcache_take(bp);
                if passone || !commit {
                    bp.set(B_ASYNC);
                } else {
                    bp.set(B_ASYNC | B_WRITEINPROG | B_NEEDCOMMIT);
                }
                buf_acquire(bp);
                splx(s);
                let _ = VOP_BWRITE(bp);
                continue 'loop_;
            }
            splx(s);
            if passone {
                passone = false;
                continue 'again;
            }
            if waitfor == MNT_WAIT {
                // loop2:
                let dirty = loop {
                    let s = splbio();
                    if vwaitforio(vp, slpflag, "nfs_fsync", slptimeo).is_err() {
                        splx(s);
                        if nfs_sigintr(nmp, None, p).is_err() {
                            return Err(Errno::EINTR);
                        }
                        if slpflag == PCATCH {
                            slpflag = 0;
                            slptimeo = sec_to_nsec(2);
                        }
                        continue; // goto loop2
                    }
                    let dirty = !vp.v_dirtyblkhd.is_empty() && commit;
                    splx(s);
                    break dirty;
                };
                if dirty {
                    // #if 0: vprint("nfs_fsync: dirty", vp);
                    continue 'loop_;
                }
            }
            break 'again;
        }
    }
    let mut error = Ok(());
    if np.isset(NWRITEERR) {
        error = errno_result(np.n_error.get());
        np.clr(NWRITEERR);
    }
    error
}

/// `nfs_pathconf` (`vop_pathconf`): return POSIX pathconf information applicable to nfs.
/// Fake it. For v3 we could ask the server, but such code hasn't been written yet.
pub fn nfs_pathconf(ap: &mut VopPathconfArgs<'_>) -> Result<(), Errno> {
    let nmp = vfstonfs_vp(ap.a_vp);
    let xfer = nmp.nm_rsize.get().min(nmp.nm_wsize.get()) as Register;

    *ap.a_retval = match ap.a_name {
        _PC_LINK_MAX => LINK_MAX as Register,
        _PC_NAME_MAX => NAME_MAX as Register,
        _PC_CHOWN_RESTRICTED => 1,
        _PC_NO_TRUNC => 1,
        _PC_ALLOC_SIZE_MIN => NFS_FABLKSIZE as Register,
        _PC_FILESIZEBITS => 64,
        _PC_REC_INCR_XFER_SIZE => xfer,
        _PC_REC_MAX_XFER_SIZE => -1, // means ``unlimited''
        _PC_REC_MIN_XFER_SIZE => xfer,
        _PC_REC_XFER_ALIGN => PAGE_SIZE as Register,
        _PC_SYMLINK_MAX => MAXPATHLEN as Register,
        _PC_2_SYMLINKS => 1,
        _PC_TIMESTAMP_RESOLUTION => {
            if nfs_isv3(ap.a_vp) {
                1
            } else {
                1000
            }
        }
        _ => return Err(Errno::EINVAL),
    };

    Ok(())
}

/// `nfs_advlock` (`vop_advlock`): NFS advisory byte-level locks.
pub fn nfs_advlock(ap: &mut VopAdvlockArgs<'_>) -> Result<(), Errno> {
    let np = VTONFS(ap.a_vp);

    lf_advlock(
        &np.n_lockf,
        np.n_size.get() as Off,
        ap.a_id,
        ap.a_op,
        ap.a_fl,
        ap.a_flags,
    )
}

/// `nfs_print` (`vop_print`): print out the contents of an nfsnode.
pub fn nfs_print(ap: &mut VopPrintArgs) -> Result<(), Errno> {
    #[cfg(any(feature = "debug", feature = "diagnostic"))]
    {
        let vp = ap.a_vp;
        let np = VTONFS(vp);
        let va = np.n_vattr.get();

        printf(format_args!(
            "tag VT_NFS, fileid {} fsid 0x{:x}",
            va.va_fileid as i64, va.va_fsid
        ));
        if vp.v_type.get() == VFIFO {
            // fifo_printinfo(vp): miscfs/fifofs, not ported.
            let _ = unported!("fifo_printinfo (miscfs/fifofs)");
        }
        printf(format_args!("\n"));
    }
    #[cfg(not(any(feature = "debug", feature = "diagnostic")))]
    let _ = ap;
    Ok(())
}

/// `nfs_bwrite` (`vop_bwrite`): just call `nfs_writebp()` with the force argument set to 1.
pub fn nfs_bwrite(ap: &mut VopBwriteArgs) -> Result<(), Errno> {
    nfs_writebp(ap.a_bp, true)
}

/// `nfs_writebp(bp, force)`: this is a clone of `vop_generic_bwrite()`, except that
/// `B_WRITEINPROG` isn't set unless the force flag is one and it also handles the
/// `B_NEEDCOMMIT` flag.
pub fn nfs_writebp(bp: &'static Buf, force: bool) -> Result<(), Errno> {
    let oldflags = bp.b_flags.get();
    let mut retv: i32 = 1;
    let p = curproc(); // XXX

    if !bp.isset(B_BUSY) {
        panic(format_args!("bwrite: buffer is not busy???"));
    }

    let Some(vp) = bp.b_vp.get() else {
        panic(format_args!("nfs_writebp: buffer {:p} without a vnode", bp));
    };
    let np = VTONFS(vp);

    bp.clr(B_READ | B_DONE | B_ERROR);

    let s = splbio();
    buf_undirty(bp);

    if oldflags & B_ASYNC != 0
        && oldflags & B_DELWRI == 0
        && let Some(p) = p
    {
        p.p_ru.ru_oublock.set(p.p_ru.ru_oublock.get() + 1);
    }

    vp.v_numoutput.set(vp.v_numoutput.get() + 1);
    splx(s);

    // If B_NEEDCOMMIT is set, a commit rpc may do the trick. If not an actual write will
    // have to be scheduled via. VOP_STRATEGY(). If B_WRITEINPROG is already set, then push
    // it with a write anyhow.
    if oldflags & (B_NEEDCOMMIT | B_WRITEINPROG) == B_NEEDCOMMIT {
        let mut off = (bp.b_blkno.get() as u64)
            .wrapping_mul(DEV_BSIZE as u64)
            .wrapping_add(bp.b_dirtyoff.get() as u64) as Off;
        let mut cnt = (bp.b_dirtyend.get() - bp.b_dirtyoff.get()) as usize;

        rw_enter_write(&np.n_commitlock);
        if !bp.isset(B_NEEDCOMMIT) {
            rw_exit_write(&np.n_commitlock);
            return Ok(());
        }

        // If it's already been committed by somebody else, bail.
        if !nfs_in_committed_range(vp, bp) {
            let mut pushedrange = false;
            // Since we're going to do this, push as much as we can.

            if nfs_in_tobecommitted_range(vp, bp) {
                pushedrange = true;
                off = np.n_pushlo.get();
                cnt = (np.n_pushhi.get() - np.n_pushlo.get()) as usize;
            }

            bp.set(B_WRITEINPROG);
            BCSTATS.pendingwrites.fetch_add(1, Ordering::Relaxed);
            BCSTATS.numwrites.fetch_add(1, Ordering::Relaxed);
            retv = match nfs_commit(vp, off as u64, cnt as i32, curproc()) {
                Ok(()) => 0,
                Err(e) => e,
            };
            bp.clr(B_WRITEINPROG);

            if retv == 0 {
                if pushedrange {
                    nfs_merge_commit_ranges(vp);
                } else {
                    nfs_add_committed_range(vp, bp);
                }
            } else {
                BCSTATS.pendingwrites.fetch_sub(1, Ordering::Relaxed);
            }
        } else {
            retv = 0; // It has already been committed.
        }

        rw_exit_write(&np.n_commitlock);
        if retv == 0 {
            bp.b_dirtyoff.set(0);
            bp.b_dirtyend.set(0);
            bp.clr(B_NEEDCOMMIT);
            let s = splbio();
            biodone(bp);
            splx(s);
        } else if retv == NFSERR_STALEWRITEVERF {
            nfs_clearcommit(vmount(vp));
        }
    }
    if retv != 0 {
        let s = splbio();
        if force {
            bp.set(B_WRITEINPROG);
        }
        splx(s);
        let _ = VOP_STRATEGY(vp, bp);
    }

    if oldflags & B_ASYNC == 0 {
        bp.set(B_RAW);
        let rtval = biowait(bp);
        if oldflags & B_DELWRI == 0
            && let Some(p) = p
        {
            p.p_ru.ru_oublock.set(p.p_ru.ru_oublock.get() + 1);
        }
        brelse(bp);
        return rtval;
    }

    Ok(())
}

/// `nfsspec_access` (`vop_access` of special files and fifos): essentially just get vattr
/// and then imitate iaccess() since the device is local to the client.
pub fn nfsspec_access(ap: &mut VopAccessArgs<'_>) -> Result<(), Errno> {
    let mut va = Vattr::new();
    let vp = ap.a_vp;

    // Disallow write attempts on filesystems mounted read-only; unless the file is a socket,
    // fifo, or a block or character device resident on the filesystem.
    if ap.a_mode & VWRITE != 0 && mnt_flag(vp) & MNT_RDONLY != 0 {
        match vp.v_type.get() {
            VREG | VDIR | VLNK => return Err(Errno::EROFS),
            _ => {}
        }
    }

    VOP_GETATTR(vp, &mut va, ap.a_cred, ap.a_p)?;

    vaccess(
        vp.v_type.get(),
        va.va_mode,
        va.va_uid,
        va.va_gid,
        ap.a_mode,
        ucred(ap.a_cred),
    )
}

/// `nfsspec_read` (`vop_read`): read wrapper for special devices.
pub fn nfsspec_read(ap: &mut VopReadArgs<'_, '_>) -> Result<(), Errno> {
    let np = VTONFS(ap.a_vp);

    // Set access flag.
    np.set(NACC);
    np.n_atim.set(getnanotime());
    spec_read(ap)
}

/// `nfsspec_write` (`vop_write`): write wrapper for special devices.
pub fn nfsspec_write(ap: &mut VopWriteArgs<'_, '_>) -> Result<(), Errno> {
    let np = VTONFS(ap.a_vp);

    // Set update flag.
    np.set(NUPD);
    np.n_mtim.set(getnanotime());
    spec_write(ap)
}

/// The times of a special file or fifo noted by its read/write wrappers, pushed to the
/// server with a setattr on its last close (a writable mount): the common part of
/// `nfsspec_close` and `nfsfifo_close`.
fn nfs_settimes_on_close(ap: &mut VopCloseArgs<'_>, np: &NfsNode) {
    let vp = ap.a_vp;

    np.set(NCHG);
    if vp.v_usecount.get() == 1 && mnt_flag(vp) & MNT_RDONLY == 0 {
        let mut vattr = Vattr::new();
        vattr_null(&mut vattr);
        if np.isset(NACC) {
            vattr.va_atime = np.n_atim.get();
        }
        if np.isset(NUPD) {
            vattr.va_mtime = np.n_mtim.get();
        }
        let _ = VOP_SETATTR(vp, &mut vattr, ap.a_cred, nfs_proc(ap.a_p));
    }
}

/// `nfsspec_close` (`vop_close`): close wrapper for special devices. Update the times on
/// the nfsnode then do device close.
pub fn nfsspec_close(ap: &mut VopCloseArgs<'_>) -> Result<(), Errno> {
    let np = VTONFS(ap.a_vp);

    if np.isset(NACC | NUPD) {
        nfs_settimes_on_close(ap, np);
    }
    spec_close(ap)
}

/// `nfsfifo_read` (`vop_read`): read wrapper for fifos.
pub fn nfsfifo_read(ap: &mut VopReadArgs<'_, '_>) -> Result<(), Errno> {
    let np = VTONFS(ap.a_vp);

    // Set access flag.
    np.set(NACC);
    np.n_atim.set(getnanotime());
    // fifo_read(ap): miscfs/fifofs, not ported.
    Err(unported!("fifo_read (miscfs/fifofs)"))
}

/// `nfsfifo_write` (`vop_write`): write wrapper for fifos.
pub fn nfsfifo_write(ap: &mut VopWriteArgs<'_, '_>) -> Result<(), Errno> {
    let np = VTONFS(ap.a_vp);

    // Set update flag.
    np.set(NUPD);
    np.n_mtim.set(getnanotime());
    // fifo_write(ap): miscfs/fifofs, not ported.
    Err(unported!("fifo_write (miscfs/fifofs)"))
}

/// `nfsfifo_close` (`vop_close`): close wrapper for fifos. Update the times on the nfsnode
/// then do fifo close.
pub fn nfsfifo_close(ap: &mut VopCloseArgs<'_>) -> Result<(), Errno> {
    let np = VTONFS(ap.a_vp);

    if np.isset(NACC | NUPD) {
        if np.isset(NACC) {
            np.n_atim.set(getnanotime());
        }
        if np.isset(NUPD) {
            np.n_mtim.set(getnanotime());
        }
        nfs_settimes_on_close(ap, np);
    }
    // fifo_close(ap): miscfs/fifofs, not ported.
    Err(unported!("fifo_close (miscfs/fifofs)"))
}

/// `nfsfifo_reclaim` (`vop_reclaim`): the fifo's, then the node's.
pub fn nfsfifo_reclaim(ap: &mut VopReclaimArgs<'_>) -> Result<(), Errno> {
    // fifo_reclaim(v): miscfs/fifofs, not ported.
    let _ = unported!("fifo_reclaim (miscfs/fifofs)");
    nfs_reclaim(ap)
}

#[cfg(test)]
mod tests;
