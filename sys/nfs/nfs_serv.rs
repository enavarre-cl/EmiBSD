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
 *	@(#)nfs_serv.c	8.7 (Berkeley) 5/14/95
 */
/* </LICENSES> */

/* <CODE> */
//! `nfs/nfs_serv.c`: the NFS version 2 and 3 server calls to vnode operations, one function
//! per procedure (`nfsrv_getattr`, `nfsrv_lookup`, `nfsrv_read`, ...), all of type
//! [`NfsrvProc`]; `nfssvc_nfsd` dispatches through nfs_syscalls.c's `nfsrv3_procs[]`.
//!
//! Upstream: sys/nfs/nfs_serv.c @ 3ce1f3f79392
//!
//! These routines generally have 3 phases:
//! 1. break down and validate the rpc request in the mbuf list;
//! 2. do the vnode ops for the request (surprisingly many are very similar to the syscalls
//!    in vfs_syscalls.c);
//! 3. build the rpc reply in an mbuf list.
//!
//! Do not mix the phases, since a bad request makes the dissection fail without doing any
//! `vrele()` or `vput()`. `nfsm_reply()` generates an nfs rpc reply with the nfs error number
//! iff error != 0, whereas returning an error from the server function implies a fatal error
//! such as a badly constructed rpc request that should be dropped without a reply. For
//! version 3, we do not return after `nfsm_reply()` for the error case, since most version 3
//! rpcs return more than the status for error cases.
//!
//! The file is compiled only with `option NFSSERVER` (`sys/conf/files`), so the module is
//! behind the `nfsserver` feature.
//!
//! ## Deviations
//! - Every procedure returns `Result<(), Errno>`: `Ok(())` is the C's 0 (the reply is in
//!   `*mrq`), `Err` the C's non-zero error (the request is dropped). The status put in the
//!   reply stays the C's `int` (an errno or an NFS status, `NFSERR_*`), as `nfs_namei` and
//!   `nfsrv_fhtovp` return it (`nfs_srvsubs.rs`).
//! - `nfsm_reply` returns the build cursor (`Some(mb)`) when the procedure goes on, `None`
//!   where the C returns non-zero (its caller then returns 0). The reply header comes from
//!   `nfs_rephead`, whose `MGETHDR` can fail here: that drops the request (`ENOBUFS`, `*mrq`
//!   NULL) after releasing what the procedure holds. `nfsm_srvmtofh1`/`nfsm_srvmtofh2` say
//!   "replied" with `Ok(true)`/return the `nfsfh_t` by value; `nfsm_srvnamesiz` returns the
//!   C's `(*lenp, *errorp)` pair. `nfsm_srvmtofh` is the pair of them every procedure calls.
//! - The client's address (`nd_nam`) is an `Option`; a request without one fails the
//!   reserved port check (`NFSERR_AUTHERR | AUTH_TOOWEAK`), as an address too short for a
//!   `sockaddr_in` does in `nfsrv_fhtovp`.
//! - The `fhandle_t` written into a reply is built by `nfsrv_vptofh` (the C's repeated
//!   `memset(fhp, 0, sizeof(nfh)); fhp->fh_fsid = ...; VFS_VPTOFH()`).
//! - `cn_pnbuf` is given back with `pnbuf_put`, which clears the pointer; `ni_startdir` is
//!   released with `startdir_rele`, which takes it out of the `nameidata`.
//! - The C's `M_TEMP` buffers (the read and write iovec arrays, the readdir buffer, the
//!   symlink target) are `Vec`s (the kernel's `GlobalAlloc` over `malloc(9)`).
//! - `MGET`/`MCLGET` with `M_WAIT` can fail here: `nfsrv_readlink` and `nfsrv_read` then
//!   reply `ENOBUFS`. When the data mbufs of a read cannot be had after its reply was begun,
//!   that reply is freed before the error reply is built (the C overwrites `*mrq` with the
//!   second reply after a failed `VOP_READ` and leaks the first).
//! - Leaks of the C fixed (each on an error path): `nfsrv_create` leaves the existing file
//!   locked when a `GUARDED` create fails with `EEXIST`; `nfsrv_create`, `nfsrv_mknod`,
//!   `nfsrv_mkdir`, `nfsrv_rmdir` and `nfsrv_rename` release the directory of a failed name
//!   lookup only after the reply, so not when `nfsm_reply` returns non-zero (a version 3
//!   `EBADRPC`): here it is released first; `nfsrv_rename`'s `nfsmout:` keeps one reference
//!   of the source directory when it is the file handle's (`XXX` workaround) and the saved
//!   name buffer.
//! - `nfsrv_access` takes `rdonly` as a `bool` and its `override` as `override_` (a Rust
//!   keyword).
//! - The `goto nfsmout` after `nfsm_reply()` in `nfsrv_create`'s device path is version 2
//!   only, where `nfsm_reply()` with an error always ends the call: the function returns.
//!   After a failed `VOP_MKNOD` there the name buffer is not given back again: the file
//!   system has freed it (`ufs_makeinode`'s `bad:`; the C's second `pool_put` is a double
//!   free).
//! - Before the second `vfs_lookup` of `nfsrv_create`'s device path, `nfsrv_mknod` and
//!   `nfsrv_symlink`, `ni_pathlen` is set to the length of the name looked up again: the
//!   lookup subtracts it a second time, which the C's `size_t` survives by wrapping.
//! - Readdir: a directory entry with a zero `d_reclen`, or one that does not fit the buffer,
//!   ends the walk (the C would loop or read past the buffer).

use alloc::vec;
use alloc::vec::Vec;
use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::sync::atomic::Ordering::Relaxed;

use crate::kern::kern_prot::suser_ucred;
use crate::kern::kern_tc::microboottime;
use crate::kern::subr_pool::pool_put;
use crate::kern::subr_prf::panic;
use crate::kern::uipc_mbuf::{m_freem, m_get, m_trailingspace};
use crate::kern::vfs_init::NAMEI_POOL;
use crate::kern::vfs_lookup::{ndinit, vfs_lookup};
use crate::kern::vfs_subr::{vattr_null, vput, vrele};
use crate::kern::vfs_vnops::vn_lock;
use crate::kern::vfs_vops::{
    VOP_ABORTOP, VOP_ACCESS, VOP_CREATE, VOP_FSYNC, VOP_GETATTR, VOP_LINK, VOP_MKDIR, VOP_MKNOD,
    VOP_PATHCONF, VOP_READ, VOP_READDIR, VOP_READLINK, VOP_REMOVE, VOP_RENAME, VOP_RMDIR,
    VOP_SETATTR, VOP_SYMLINK, VOP_UNLOCK, VOP_WRITE,
};
use crate::nfs::nfs::{ND_NFSV3, NfsrvDescript, NfssvcSock, nfs_srvmaxdata};
use crate::nfs::nfs_socket::nfs_rephead;
use crate::nfs::nfs_srvsubs::{
    nfs_namei, nfsm_adj, nfsm_srvfattr, nfsm_srvpostop_attr, nfsm_srvsattr, nfsm_srvwcc,
    nfsrv_fhtovp,
};
use crate::nfs::nfs_subs::{
    NFSSTATS, nfs_false, nfs_true, nfs_xdrneg1, nfsm_buftombuf, nfsm_build, nfsm_srvfhtom,
    nfsm_strtombuf,
};
use crate::nfs::nfs_var::nfsm_padlen;
use crate::nfs::nfsm_subs::{
    XdrOut, nfsd_adv, nfsd_dissect, nfsd_mtouio, nfsd_strsiz, nfsm_avail, nfsm_rndup,
};
use crate::nfs::nfsproto::{
    NFS_FABLKSIZE, NFS_MAXDATA, NFS_MAXDGRAMDATA, NFS_MAXNAMLEN, NFS_MAXPATHLEN, NFSERR_AUTHERR,
    NFSERR_BADTYPE, NFSERR_NAMETOL, NFSERR_NOT_SYNC, NFSERR_RETVOID, NFSERR_TOOSMALL,
    NFSV3ACCESS_DELETE, NFSV3ACCESS_EXECUTE, NFSV3ACCESS_EXTEND, NFSV3ACCESS_LOOKUP,
    NFSV3ACCESS_MODIFY, NFSV3ACCESS_READ, NFSV3CREATE_EXCLUSIVE, NFSV3CREATE_GUARDED,
    NFSV3CREATE_UNCHECKED, NFSV3FSINFO_CANSETTIME, NFSV3FSINFO_HOMOGENEOUS, NFSV3FSINFO_LINK,
    NFSV3FSINFO_SYMLINK, NFSV3WRITE_FILESYNC, NFSV3WRITE_UNSTABLE, NFSX_UNSIGNED, NFSX_V2FATTR,
    NFSX_V2FH, NFSX_V2SATTR, NFSX_V3COOKIEVERF, NFSX_V3CREATEVERF, NFSX_V3FATTR, NFSX_V3FH,
    NFSX_V3FSINFO, NFSX_V3PATHCONF, NFSX_V3POSTOPATTR, NFSX_V3WCCDATA, NFSX_V3WRITEVERF, NfsFattr,
    NfsStatfs, Nfsfh, Nfsuint64, Nfsv2Sattr, Nfsv3Fsinfo, Nfsv3Pathconf, Nfsv3Time, nfstov_mode,
    nfsv3tov_type, nfsx_cookieverf, nfsx_fattr, nfsx_postopattr, nfsx_postoporfattr,
    nfsx_preopattr, nfsx_srvfh, nfsx_statfs, nfsx_wccdata, nfsx_wccorfattr, nfsx_writeverf,
};
use crate::nfs::rpcv2::AUTH_TOOWEAK;
use crate::nfs::xdr_subs::{
    fxdr_nfsv2time, fxdr_nfsv3time, fxdr_unsigned, txdr_hyper, txdr_unsigned, xdr_bytes,
};
use crate::sys::dirent::Dirent;
use crate::sys::errno::Errno;
use crate::sys::lock::{LK_EXCLUSIVE, LK_RETRY};
use crate::sys::mbuf::{M_EXT, M_WAIT, MINCLSIZE, MT_DATA, Mbuf, mclget, mtod};
use crate::sys::mount::{
    Fhandle, MNT_RDONLY, MNT_WAIT, Mount, Statfs, VFS_STATFS, VFS_VGET, VFS_VPTOFH,
};
use crate::sys::namei::{
    CREATE, Componentname, DELETE, FOLLOW, HASBUF, ISSYMLINK, LOCKLEAF, LOCKPARENT, LOOKUP,
    NOCACHE, NOFOLLOW, Nameidata, NiDirp, RDONLY, RENAME, SAVESTART, WANTPARENT,
};
use crate::sys::proc::Proc;
use crate::sys::socket::SOCK_DGRAM;
use crate::sys::time::Timespec;
use crate::sys::types::{Dev, Mode, Off, Register, makedev};
use crate::sys::ucred::Ucred;
use crate::sys::uio::{Iovec, Uio, UioRw, UioSeg};
use crate::sys::unistd::{_PC_CHOWN_RESTRICTED, _PC_LINK_MAX, _PC_NAME_MAX, _PC_NO_TRUNC};
use crate::sys::vnode::{
    IO_NODELOCKED, IO_SYNC, VA_UTIMES_NULL, VBLK, VCHR, VDIR, VEXEC, VFIFO, VLNK, VNON, VREAD,
    VREG, VROOT, VSOCK, VTEXT, VWRITE, Vattr, Vnode, iftovt,
};
use crate::ufs::ufs::dir::DIRBLKSIZ;
use crate::uvm::uvm_vnode::uvm_vnp_uncache;

/// A server procedure (`int (*)(struct nfsrv_descript *, struct nfssvc_sock *, struct proc *,
/// struct mbuf **)`): serves the request `nd` from the socket `slp` in the nfsd `procp` and
/// leaves the reply in `*mrq`. `Err` is the C's non-zero return: the request is dropped.
pub type NfsrvProc =
    fn(&mut NfsrvDescript, &NfssvcSock, &Proc, &mut Option<&'static Mbuf>) -> Result<(), Errno>;

/// The status of a vnode operation as the C's `int error` (0 or an errno).
fn status_of<T>(r: &Result<T, Errno>) -> i32 {
    match r {
        Ok(_) => 0,
        Err(e) => e.as_i32(),
    }
}

/// Whether `a` and `b` are the same vnode (the C's pointer comparison; NULL is no vnode).
fn same_vnode(a: Option<&Vnode>, b: Option<&Vnode>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => ptr::eq(a, b),
        (None, None) => true,
        _ => false,
    }
}

/// `vp->v_mount`, which a vnode an export or a lookup returned always has.
fn v_mount(vp: &Vnode) -> &'static Mount {
    match vp.v_mount.get() {
        Some(mp) => mp,
        None => panic(format_args!("nfs_serv: vnode {:p} without a mount", vp)),
    }
}

/// Whether `vp` and `wp` are on the same mount (`vp->v_mount == wp->v_mount`).
fn same_mount(vp: &Vnode, wp: &Vnode) -> bool {
    match (vp.v_mount.get(), wp.v_mount.get()) {
        (Some(a), Some(b)) => ptr::eq(a, b),
        (None, None) => true,
        _ => false,
    }
}

/// `vp->v_mount->mnt_flag & MNT_RDONLY`.
fn mnt_rdonly(vp: &Vnode) -> bool {
    v_mount(vp).mnt_flag.get() & MNT_RDONLY != 0
}

/// `nd.ni_vp` after a lookup that returned it.
fn ni_vp(nd: &Nameidata<'_>) -> &'static Vnode {
    match nd.ni_vp {
        Some(vp) => vp,
        None => panic(format_args!("nfs_serv: lookup without a vnode")),
    }
}

/// `nd.ni_dvp` after a lookup that returned the parent.
fn ni_dvp(nd: &Nameidata<'_>) -> &'static Vnode {
    match nd.ni_dvp {
        Some(dvp) => dvp,
        None => panic(format_args!("nfs_serv: lookup without a parent")),
    }
}

/// `VOP_ABORTOP(nd.ni_dvp, &nd.ni_cnd)`.
fn abortop(nd: &mut Nameidata<'_>) {
    if let Some(dvp) = nd.ni_dvp {
        let _ = VOP_ABORTOP(dvp, &mut nd.ni_cnd);
    }
}

/// `if (nd.ni_dvp == nd.ni_vp) vrele(nd.ni_dvp); else vput(nd.ni_dvp);`: releases the locked
/// parent of a lookup, which is only referenced when it is also the leaf.
fn dvp_release(nd: &Nameidata<'_>) {
    if let Some(dvp) = nd.ni_dvp {
        if same_vnode(Some(dvp), nd.ni_vp) {
            vrele(dvp);
        } else {
            vput(dvp);
        }
    }
}

/// `vrele(nd.ni_startdir)`: the starting directory a `SAVESTART` lookup kept referenced.
fn startdir_rele(nd: &mut Nameidata<'_>) {
    if let Some(sd) = nd.ni_startdir.take() {
        vrele(sd);
    }
}

/// `pool_put(&namei_pool, cnp->cn_pnbuf)`: gives a saved name buffer back.
fn pnbuf_put(cnp: &mut Componentname) {
    if let Some(buf) = NonNull::new(cnp.cn_pnbuf) {
        pool_put(&NAMEI_POOL, buf);
    }
    cnp.cn_pnbuf = ptr::null_mut();
}

/// `if (cn_flags & HASBUF) { pool_put(&namei_pool, cn_pnbuf); cn_flags &= ~HASBUF; }`.
fn pnbuf_put_hasbuf(cnp: &mut Componentname) {
    if cnp.cn_flags & HASBUF != 0 {
        pnbuf_put(cnp);
        cnp.cn_flags &= !HASBUF;
    }
}

/// `ni_pathlen` for the second `vfs_lookup` of the name the first one left in
/// `cn_nameptr` (after `VOP_MKNOD`/`VOP_SYMLINK`): the length of that name, which the lookup
/// consumes again (the C lets the unsigned count wrap below zero there).
fn nfsrv_relookup_pathlen(nd: &mut Nameidata<'_>) {
    nd.ni_pathlen = nd.ni_cnd.cn_namelen as usize;
}

/// The status of a request whose client cannot pass the reserved port check.
const AUTH_TOOWEAK_STATUS: i32 = NFSERR_AUTHERR | AUTH_TOOWEAK as i32;

/// `nfsrv_fhtovp(fhp, lockflag, &vp, &nfsd->nd_cr, slp, nfsd->nd_nam, &rdonly)`.
fn fhtovp(
    nfsd: &NfsrvDescript,
    fh: &Fhandle,
    lockflag: bool,
    slp: &NfssvcSock,
) -> Result<(&'static Vnode, bool), i32> {
    let Some(nam) = nfsd.nd_nam else {
        return Err(AUTH_TOOWEAK_STATUS);
    };
    nfsrv_fhtovp(fh, lockflag, &nfsd.nd_cr, slp, nam)
}

/// `nfs_namei(ndp, fhp, len, slp, nfsd->nd_nam, &nfsd->nd_md, &nfsd->nd_dpos, retdirp, p)`.
fn namei(
    nfsd: &mut NfsrvDescript,
    ndp: &mut Nameidata<'_>,
    fh: &Fhandle,
    len: usize,
    slp: &NfssvcSock,
    retdirp: &mut Option<&'static Vnode>,
    p: &Proc,
) -> Result<(), i32> {
    *retdirp = None;
    let Some(nam) = nfsd.nd_nam else {
        return Err(AUTH_TOOWEAK_STATUS);
    };
    nfs_namei(
        ndp,
        fh,
        len,
        slp,
        nam,
        &mut nfsd.nd_md,
        &mut nfsd.nd_dpos,
        retdirp,
        p,
    )
}

/// The file handle of `vp`, as the server writes it into a reply: `memset(fhp, 0,
/// sizeof(nfh)); fhp->fh_fsid = vp->v_mount->mnt_stat.f_fsid; VFS_VPTOFH(vp, &fhp->fh_fid)`.
fn nfsrv_vptofh(vp: &'static Vnode) -> Result<Nfsfh, Errno> {
    let mut fh = Fhandle {
        fh_fsid: v_mount(vp).mnt_stat.get().f_fsid,
        ..Fhandle::default()
    };
    VFS_VPTOFH(vp, &mut fh.fh_fid)?;
    let mut nfh = Nfsfh::new();
    nfh.set_fh_generic(&fh);
    Ok(nfh)
}

/// A write view of the `len` bytes at byte `off` of `m`'s data: a part of the reply built
/// earlier and filled in now (the C keeps the `tl`/`fp` pointer `nfsm_build` returned).
fn nfsm_rebuild(m: &Mbuf, off: usize, len: usize) -> XdrOut<'_> {
    let mlen = m.m_len().get() as usize;
    if off > mlen || len > mlen - off {
        panic(format_args!(
            "nfsm_rebuild: {} bytes at {} past the {} of mbuf {:p}",
            len, off, mlen, m
        ));
    }
    // SAFETY: the `len` bytes at `off` lie inside `m`'s data (checked above), which this
    // request's reply owns; nothing else reads or writes them while the view lives.
    unsafe { XdrOut::new(mtod::<u8>(m).add(off), len) }
}

/// `nfsm_reply(nfsd, slp, mrq, &mb, error, statuslen)`: builds the reply header for the
/// status `error` (with room for `statuslen` bytes after it; none for a version 2 error),
/// frees the request and puts the reply in `*mrq`. `Some(mb)` is the build cursor when the
/// procedure goes on; `None` where the C returns non-zero (a version 2 error, `EBADRPC`):
/// the procedure is done and returns 0.
fn nfsm_reply(
    nfsd: &mut NfsrvDescript,
    slp: &NfssvcSock,
    mrq: &mut Option<&'static Mbuf>,
    error: i32,
    statuslen: usize,
) -> Result<Option<&'static Mbuf>, Errno> {
    let v3 = nfsd.nd_flag & ND_NFSV3 != 0;

    nfsd.nd_repstat = error;
    let statuslen = if error != 0 && !v3 { 0 } else { statuslen };
    let r = nfs_rephead(statuslen, nfsd, Some(slp), error);
    m_freem(nfsd.nd_mrep.take());
    let (mreq, mb) = match r {
        Ok(x) => x,
        Err(e) => {
            *mrq = None;
            return Err(e);
        }
    };
    *mrq = Some(mreq);
    if error != 0 && (!v3 || error == Errno::EBADRPC.as_i32()) {
        return Ok(None);
    }
    Ok(Some(mb))
}

/// `nfsm_srvmtofh1(nfsd, slp, mrq, &mb, &error)`: checks the length of a version 3 file
/// handle. `Ok(true)`: it is wrong, and the `EBADRPC` reply is in `*mrq` (the C returns
/// non-zero).
fn nfsm_srvmtofh1(
    nfsd: &mut NfsrvDescript,
    slp: &NfssvcSock,
    mrq: &mut Option<&'static Mbuf>,
) -> Result<bool, Errno> {
    if nfsd.nd_flag & ND_NFSV3 != 0 {
        let len = fxdr_unsigned(nfsd_dissect(nfsd, NFSX_UNSIGNED)?.get(0)) as i32;
        if len != NFSX_V3FH as i32 {
            nfsm_reply(nfsd, slp, mrq, Errno::EBADRPC.as_i32(), 0)?;
            return Ok(true);
        }
    }
    Ok(false)
}

/// `nfsm_srvmtofh2(nfsd, fhp, &error)`: the file handle of the request (`NFSX_V3FH` bytes; a
/// version 2 handle is padded to `NFSX_V2FH`).
fn nfsm_srvmtofh2(nfsd: &mut NfsrvDescript) -> Result<Nfsfh, Errno> {
    let mut nfh = Nfsfh::new();
    nfh.fh_bytes[..NFSX_V3FH].copy_from_slice(nfsd_dissect(nfsd, NFSX_V3FH)?.bytes());
    if nfsd.nd_flag & ND_NFSV3 == 0 {
        nfsd_adv(nfsd, NFSX_V2FH - NFSX_V3FH)?;
    }
    Ok(nfh)
}

/// `nfsm_srvmtofh1` then `nfsm_srvmtofh2`, as every procedure begins: the request's file
/// handle, or `None` when the `EBADRPC` reply is already in `*mrq`.
fn nfsm_srvmtofh(
    nfsd: &mut NfsrvDescript,
    slp: &NfssvcSock,
    mrq: &mut Option<&'static Mbuf>,
) -> Result<Option<Nfsfh>, Errno> {
    if nfsm_srvmtofh1(nfsd, slp, mrq)? {
        return Ok(None);
    }
    nfsm_srvmtofh2(nfsd).map(Some)
}

/// `nfsm_srvnamesiz(nfsd, &len, &error)`: the length of the name that follows: `(len, 0)`, or
/// `(0, NFSERR_NAMETOL)` past `NFS_MAXNAMLEN`, `(0, EBADRPC)` for an empty or negative one.
fn nfsm_srvnamesiz(nfsd: &mut NfsrvDescript) -> Result<(usize, i32), Errno> {
    let len = fxdr_unsigned(nfsd_dissect(nfsd, NFSX_UNSIGNED)?.get(0)) as i32;
    Ok(if len > NFS_MAXNAMLEN as i32 {
        (0, NFSERR_NAMETOL)
    } else if len <= 0 {
        (0, Errno::EBADRPC.as_i32())
    } else {
        (len as usize, 0)
    })
}

/// `nfs v3 access service`
pub fn nfsrv3_access(
    nfsd: &mut NfsrvDescript,
    slp: &NfssvcSock,
    procp: &Proc,
    mrq: &mut Option<&'static Mbuf>,
) -> Result<(), Errno> {
    let cred: *const Ucred = &nfsd.nd_cr;

    let Some(nfh) = nfsm_srvmtofh(nfsd, slp, mrq)? else {
        return Ok(());
    };
    let fh = nfh.fh_generic();
    let mut nfsmode = fxdr_unsigned(nfsd_dissect(nfsd, NFSX_UNSIGNED)?.get(0));
    let (vp, rdonly) = match fhtovp(nfsd, &fh, true, slp) {
        Ok(x) => x,
        Err(error) => {
            if let Some(mut mb) = nfsm_reply(nfsd, slp, mrq, error, NFSX_UNSIGNED)? {
                nfsm_srvpostop_attr(nfsd, None, &mut mb);
            }
            return Ok(());
        }
    };
    let isdir = vp.v_type.get() == VDIR;
    if nfsmode & NFSV3ACCESS_READ != 0
        && nfsrv_access(vp, VREAD, &nfsd.nd_cr, rdonly, procp, false).is_err()
    {
        nfsmode &= !NFSV3ACCESS_READ;
    }
    let testmode = if isdir {
        NFSV3ACCESS_MODIFY | NFSV3ACCESS_EXTEND | NFSV3ACCESS_DELETE
    } else {
        NFSV3ACCESS_MODIFY | NFSV3ACCESS_EXTEND
    };
    if nfsmode & testmode != 0
        && nfsrv_access(vp, VWRITE, &nfsd.nd_cr, rdonly, procp, false).is_err()
    {
        nfsmode &= !testmode;
    }
    let testmode = if isdir {
        NFSV3ACCESS_LOOKUP
    } else {
        NFSV3ACCESS_EXECUTE
    };
    if nfsmode & testmode != 0
        && nfsrv_access(vp, VEXEC, &nfsd.nd_cr, rdonly, procp, false).is_err()
    {
        nfsmode &= !testmode;
    }
    let mut va = Vattr::new();
    let getret = VOP_GETATTR(vp, &mut va, cred, procp).is_ok();
    vput(vp);
    let Some(mut mb) = nfsm_reply(nfsd, slp, mrq, 0, nfsx_postopattr(true) + NFSX_UNSIGNED)? else {
        return Ok(());
    };
    nfsm_srvpostop_attr(nfsd, getret.then_some(&va), &mut mb);
    nfsm_build(&mut mb, NFSX_UNSIGNED).set(0, txdr_unsigned(nfsmode));
    Ok(())
}

/// `nfs getattr service`
pub fn nfsrv_getattr(
    nfsd: &mut NfsrvDescript,
    slp: &NfssvcSock,
    procp: &Proc,
    mrq: &mut Option<&'static Mbuf>,
) -> Result<(), Errno> {
    let cred: *const Ucred = &nfsd.nd_cr;

    let Some(nfh) = nfsm_srvmtofh(nfsd, slp, mrq)? else {
        return Ok(());
    };
    let fh = nfh.fh_generic();
    let (vp, _rdonly) = match fhtovp(nfsd, &fh, true, slp) {
        Ok(x) => x,
        Err(error) => {
            nfsm_reply(nfsd, slp, mrq, error, 0)?;
            return Ok(());
        }
    };
    let mut va = Vattr::new();
    let error = status_of(&VOP_GETATTR(vp, &mut va, cred, procp));
    vput(vp);
    let v3 = nfsd.nd_flag & ND_NFSV3 != 0;
    let Some(mut mb) = nfsm_reply(nfsd, slp, mrq, error, nfsx_fattr(v3))? else {
        return Ok(());
    };
    if error != 0 {
        return Ok(());
    }
    let fp = nfsm_srvfattr(nfsd, &va);
    nfsm_build(&mut mb, nfsx_fattr(v3)).write(0, &fp);
    Ok(())
}

/// `nfs setattr service`
pub fn nfsrv_setattr(
    nfsd: &mut NfsrvDescript,
    slp: &NfssvcSock,
    procp: &Proc,
    mrq: &mut Option<&'static Mbuf>,
) -> Result<(), Errno> {
    let cred: *const Ucred = &nfsd.nd_cr;
    let v3 = nfsd.nd_flag & ND_NFSV3 != 0;

    let Some(nfh) = nfsm_srvmtofh(nfsd, slp, mrq)? else {
        return Ok(());
    };
    let fh = nfh.fh_generic();
    let mut va = Vattr::new();
    vattr_null(&mut va);
    let mut gcheck = false;
    let mut guard = Timespec::default();
    if v3 {
        va.va_vaflags |= VA_UTIMES_NULL;
        nfsm_srvsattr(nfsd, &mut va)?;
        gcheck = fxdr_unsigned(nfsd_dissect(nfsd, NFSX_UNSIGNED)?.get(0)) != 0;
        if gcheck {
            let t: Nfsv3Time = nfsd_dissect(nfsd, 2 * NFSX_UNSIGNED)?.read(0);
            guard = fxdr_nfsv3time(&t);
        }
    } else {
        let sp: Nfsv2Sattr = nfsd_dissect(nfsd, NFSX_V2SATTR)?.read(0);
        // Nah nah nah nah na nah
        // There is a bug in the Sun client that puts 0xffff in the mode field of sattr when
        // it should put in 0xffffffff. The u_short doesn't sign extend.
        // --> check the low order 2 bytes for 0xffff
        if fxdr_unsigned(sp.sa_mode) & 0xffff != 0xffff {
            va.va_mode = nfstov_mode(sp.sa_mode);
        }
        if sp.sa_uid != nfs_xdrneg1 {
            va.va_uid = fxdr_unsigned(sp.sa_uid);
        }
        if sp.sa_gid != nfs_xdrneg1 {
            va.va_gid = fxdr_unsigned(sp.sa_gid);
        }
        if sp.sa_size != nfs_xdrneg1 {
            va.va_size = u64::from(fxdr_unsigned(sp.sa_size));
        }
        if sp.sa_atime.nfsv2_sec != nfs_xdrneg1 {
            // `#ifdef notyet`: fxdr_nfsv2time(&sp->sa_atime, &va.va_atime);
            va.va_atime.tv_sec = i64::from(fxdr_unsigned(sp.sa_atime.nfsv2_sec));
            va.va_atime.tv_nsec = 0;
        }
        if sp.sa_mtime.nfsv2_sec != nfs_xdrneg1 {
            va.va_mtime = fxdr_nfsv2time(&sp.sa_mtime);
        }
    }

    // Now that we have all the fields, lets do it.
    let (vp, rdonly) = match fhtovp(nfsd, &fh, true, slp) {
        Ok(x) => x,
        Err(error) => {
            if let Some(mut mb) = nfsm_reply(nfsd, slp, mrq, error, 2 * NFSX_UNSIGNED)? {
                nfsm_srvwcc(nfsd, None, None, &mut mb);
            }
            return Ok(());
        }
    };
    let mut preat = Vattr::new();
    let mut preat_ok = false;
    if v3 {
        let r = VOP_GETATTR(vp, &mut preat, cred, procp);
        preat_ok = r.is_ok();
        let mut error = status_of(&r);
        if error == 0
            && gcheck
            && (preat.va_ctime.tv_sec != guard.tv_sec || preat.va_ctime.tv_nsec != guard.tv_nsec)
        {
            error = NFSERR_NOT_SYNC;
        }
        if error != 0 {
            vput(vp);
            if let Some(mut mb) = nfsm_reply(nfsd, slp, mrq, error, nfsx_wccdata(v3))? {
                nfsm_srvwcc(nfsd, preat_ok.then_some(&preat), None, &mut mb);
            }
            return Ok(());
        }
    }

    // If the size is being changed write access is required, otherwise just check for a read
    // only file system.
    let mut postat_ok = false;
    let error = 'out: {
        if va.va_size == VNOVAL_SIZE {
            if rdonly || mnt_rdonly(vp) {
                break 'out Errno::EROFS.as_i32();
            }
        } else if vp.v_type.get() == VDIR {
            break 'out Errno::EISDIR.as_i32();
        } else if let Err(e) = nfsrv_access(vp, VWRITE, &nfsd.nd_cr, rdonly, procp, true) {
            break 'out e.as_i32();
        }
        let error = status_of(&VOP_SETATTR(vp, &mut va, cred, procp));
        let postat = VOP_GETATTR(vp, &mut va, cred, procp);
        postat_ok = postat.is_ok();
        if error == 0 {
            status_of(&postat)
        } else {
            error
        }
    };
    // out:
    vput(vp);
    let Some(mut mb) = nfsm_reply(nfsd, slp, mrq, error, nfsx_wccorfattr(v3))? else {
        return Ok(());
    };
    if v3 {
        nfsm_srvwcc(
            nfsd,
            preat_ok.then_some(&preat),
            postat_ok.then_some(&va),
            &mut mb,
        );
    } else {
        let fp = nfsm_srvfattr(nfsd, &va);
        nfsm_build(&mut mb, NFSX_V2FATTR).write(0, &fp);
    }
    Ok(())
}

/// `va_size` of a `vattr_null`ed `struct vattr` (`(u_quad_t)VNOVAL`): no size given.
const VNOVAL_SIZE: u64 = u64::MAX;

/// `nfs lookup rpc`
pub fn nfsrv_lookup(
    nfsd: &mut NfsrvDescript,
    slp: &NfssvcSock,
    procp: &Proc,
    mrq: &mut Option<&'static Mbuf>,
) -> Result<(), Errno> {
    let cred: *const Ucred = &nfsd.nd_cr;
    let v3 = nfsd.nd_flag & ND_NFSV3 != 0;

    let Some(nfh) = nfsm_srvmtofh(nfsd, slp, mrq)? else {
        return Ok(());
    };
    let fh = nfh.fh_generic();
    let (len, error) = nfsm_srvnamesiz(nfsd)?;
    if error != 0 {
        // nfsm_reply would return zero if v3 and an error different from EBADRPC. But it
        // does not make sense to continue anyway if the error set in nfsm_srvnamesiz is
        // NFSERR_NAMETOL.
        nfsm_reply(nfsd, slp, mrq, error, 0)?;
        return Ok(());
    }

    let mut nd = ndinit(LOOKUP, LOCKLEAF | SAVESTART, NiDirp::Sys(&[]), procp);
    nd.ni_cnd.cn_cred = cred;
    let mut dirp = None;
    let r = namei(nfsd, &mut nd, &fh, len, slp, &mut dirp, procp);
    let mut dirattr = Vattr::new();
    let mut dirattr_ok = false;
    if let Some(dirp) = dirp {
        if v3 {
            dirattr_ok = VOP_GETATTR(dirp, &mut dirattr, cred, procp).is_ok();
        }
        vrele(dirp);
    }
    if let Err(error) = r {
        if let Some(mut mb) = nfsm_reply(nfsd, slp, mrq, error, nfsx_postopattr(v3))? {
            nfsm_srvpostop_attr(nfsd, dirattr_ok.then_some(&dirattr), &mut mb);
        }
        return Ok(());
    }
    startdir_rele(&mut nd);
    pnbuf_put(&mut nd.ni_cnd);
    let vp = ni_vp(&nd);
    let mut va = Vattr::new();
    let r = nfsrv_vptofh(vp).and_then(|nfh| VOP_GETATTR(vp, &mut va, cred, procp).map(|()| nfh));
    vput(vp);
    let Some(mut mb) = nfsm_reply(
        nfsd,
        slp,
        mrq,
        status_of(&r),
        nfsx_srvfh(v3) + nfsx_postoporfattr(v3) + nfsx_postopattr(v3),
    )?
    else {
        return Ok(());
    };
    let Ok(nfh) = r else {
        nfsm_srvpostop_attr(nfsd, dirattr_ok.then_some(&dirattr), &mut mb);
        return Ok(());
    };
    nfsm_srvfhtom(&mut mb, &nfh, v3);
    if v3 {
        nfsm_srvpostop_attr(nfsd, Some(&va), &mut mb);
        nfsm_srvpostop_attr(nfsd, dirattr_ok.then_some(&dirattr), &mut mb);
    } else {
        let fp = nfsm_srvfattr(nfsd, &va);
        nfsm_build(&mut mb, NFSX_V2FATTR).write(0, &fp);
    }
    Ok(())
}

/// `nfs readlink service`
pub fn nfsrv_readlink(
    nfsd: &mut NfsrvDescript,
    slp: &NfssvcSock,
    procp: &Proc,
    mrq: &mut Option<&'static Mbuf>,
) -> Result<(), Errno> {
    let cred: *const Ucred = &nfsd.nd_cr;
    let v3 = nfsd.nd_flag & ND_NFSV3 != 0;

    let Some(nfh) = nfsm_srvmtofh(nfsd, slp, mrq)? else {
        return Ok(());
    };
    let fh = nfh.fh_generic();
    let (vp, _rdonly) = match fhtovp(nfsd, &fh, true, slp) {
        Ok(x) => x,
        Err(error) => {
            if let Some(mut mb) = nfsm_reply(nfsd, slp, mrq, error, 2 * NFSX_UNSIGNED)? {
                nfsm_srvpostop_attr(nfsd, None, &mut mb);
            }
            return Ok(());
        }
    };
    let mut mp: Option<&'static Mbuf> = None;
    let mut len = 0usize;
    let mut resid = 0usize;
    let r: Result<(), Errno> = 'out: {
        if vp.v_type.get() != VLNK {
            break 'out Err(if v3 { Errno::EINVAL } else { Errno::ENXIO });
        }
        // MLEN < NFS_MAXPATHLEN < MCLBYTES
        let Some(m) = m_get(M_WAIT, MT_DATA) else {
            break 'out Err(Errno::ENOBUFS);
        };
        mclget(m, M_WAIT);
        if m.m_flags().get() & M_EXT == 0 {
            m_freem(m);
            break 'out Err(Errno::ENOBUFS);
        }
        mp = Some(m);
        m.m_len().set(NFS_MAXPATHLEN as u32);
        len = NFS_MAXPATHLEN;
        let mut iov = [Iovec {
            iov_base: mtod::<c_void>(m),
            iov_len: NFS_MAXPATHLEN,
        }];
        let mut uio = Uio {
            uio_iov: &mut iov,
            uio_offset: 0,
            uio_resid: NFS_MAXPATHLEN,
            uio_segflg: UioSeg::UIO_SYSSPACE,
            uio_rw: UioRw::UIO_READ,
            uio_procp: None,
        };
        let r = VOP_READLINK(vp, &mut uio, cred);
        resid = uio.uio_resid;
        r
    };
    // out:
    let mut attr = Vattr::new();
    let getret = VOP_GETATTR(vp, &mut attr, cred, procp).is_ok();
    vput(vp);
    if r.is_err() {
        m_freem(mp.take());
    }
    let reply = nfsm_reply(
        nfsd,
        slp,
        mrq,
        status_of(&r),
        nfsx_postopattr(v3) + NFSX_UNSIGNED,
    )
    .inspect_err(|_| {
        m_freem(mp);
    })?;
    let Some(mut mb) = reply else {
        return Ok(());
    };
    if v3 {
        nfsm_srvpostop_attr(nfsd, getret.then_some(&attr), &mut mb);
        if r.is_err() {
            return Ok(());
        }
    }
    if resid > 0 {
        len -= resid;
        let tlen = nfsm_rndup(len);
        if let Some(m) = mp {
            nfsm_adj(m, (NFS_MAXPATHLEN - tlen) as i32, (tlen - len) as i32);
        }
    }
    nfsm_build(&mut mb, NFSX_UNSIGNED).set(0, txdr_unsigned(len as u32));
    mb.m_next().set(mp);
    Ok(())
}

/// `bad:`/`vbad:` of `nfsrv_read`: the error reply with the attributes fetched so far.
fn nfsrv_read_bad(
    nfsd: &mut NfsrvDescript,
    slp: &NfssvcSock,
    mrq: &mut Option<&'static Mbuf>,
    error: i32,
    va: Option<&Vattr>,
) -> Result<(), Errno> {
    if let Some(mut mb) = nfsm_reply(nfsd, slp, mrq, error, 0)? {
        nfsm_srvpostop_attr(nfsd, va, &mut mb);
    }
    Ok(())
}

/// `nfs read service`
pub fn nfsrv_read(
    nfsd: &mut NfsrvDescript,
    slp: &NfssvcSock,
    procp: &Proc,
    mrq: &mut Option<&'static Mbuf>,
) -> Result<(), Errno> {
    let cred: *const Ucred = &nfsd.nd_cr;
    let v3 = nfsd.nd_flag & ND_NFSV3 != 0;

    let Some(nfh) = nfsm_srvmtofh(nfsd, slp, mrq)? else {
        return Ok(());
    };
    let fh = nfh.fh_generic();
    let off: Off = if v3 {
        nfsd_dissect(nfsd, 2 * NFSX_UNSIGNED)?.fxdr_hyper(0) as Off
    } else {
        Off::from(fxdr_unsigned(nfsd_dissect(nfsd, NFSX_UNSIGNED)?.get(0)))
    };
    let reqlen = fxdr_unsigned(nfsd_dissect(nfsd, NFSX_UNSIGNED)?.get(0)) as i32;
    if reqlen > nfs_srvmaxdata(nfsd) as i32 || reqlen <= 0 {
        // nfsm_reply returns EBADRPC: the C returns 0.
        nfsm_reply(nfsd, slp, mrq, Errno::EBADRPC.as_i32(), 0)?;
        return Ok(());
    }

    let (vp, rdonly) = match fhtovp(nfsd, &fh, true, slp) {
        Ok(x) => x,
        Err(error) => return nfsrv_read_bad(nfsd, slp, mrq, error, None),
    };

    let mut error = 0;
    if vp.v_type.get() != VREG {
        error = if v3 {
            Errno::EINVAL
        } else if vp.v_type.get() == VDIR {
            Errno::EISDIR
        } else {
            Errno::EACCES
        }
        .as_i32();
    }
    if error == 0 && nfsrv_access(vp, VREAD, &nfsd.nd_cr, rdonly, procp, true).is_err() {
        error = status_of(&nfsrv_access(vp, VEXEC, &nfsd.nd_cr, rdonly, procp, true));
    }
    let mut va = Vattr::new();
    let getret = VOP_GETATTR(vp, &mut va, cred, procp);
    let mut getret_ok = getret.is_ok();
    if error == 0 {
        error = status_of(&getret);
    }
    if error != 0 {
        vput(vp);
        return nfsrv_read_bad(nfsd, slp, mrq, error, getret_ok.then_some(&va));
    }

    let offu = off as u64;
    let mut cnt: usize = if offu >= va.va_size {
        0
    } else if offu.wrapping_add(reqlen as u64) > va.va_size {
        (va.va_size - offu) as usize
    } else {
        reqlen as usize
    };
    let mb = match nfsm_reply(
        nfsd,
        slp,
        mrq,
        0,
        nfsx_postoporfattr(v3) + 3 * NFSX_UNSIGNED + nfsm_rndup(cnt),
    ) {
        Ok(Some(mb)) => mb,
        Ok(None) => {
            vput(vp);
            return Ok(());
        }
        Err(e) => {
            vput(vp);
            return Err(e);
        }
    };
    // The attributes (and for version 3 the count and eof words) are filled in once the
    // data is read: remember where they go.
    let hdrlen = if v3 {
        NFSX_V3FATTR + 4 * NFSX_UNSIGNED
    } else {
        NFSX_V2FATTR + NFSX_UNSIGNED
    };
    let mut mb = mb;
    nfsm_build(&mut mb, hdrlen);
    let hdr = mb;
    let hdroff = hdr.m_len().get() as usize - hdrlen;

    let len = nfsm_rndup(cnt);
    let mut resid = 0usize;
    if cnt > 0 {
        // Generate the mbuf list with the uio_iov ref. to it.
        let mut left = len;
        let mut i = 0usize;
        let mut m = mb;
        let mut nomem = false;
        while left > 0 {
            let siz = (m_trailingspace(m).max(0) as usize).min(left);
            if siz > 0 {
                left -= siz;
                i += 1;
            }
            if left > 0 {
                let Some(n) = m_get(M_WAIT, MT_DATA) else {
                    nomem = true;
                    break;
                };
                if left >= MINCLSIZE {
                    mclget(n, M_WAIT);
                }
                n.m_len().set(0);
                m.m_next().set(Some(n));
                m = n;
            }
        }
        if nomem {
            // vbad: the reply begun above goes (the C would leak it).
            vput(vp);
            m_freem(mrq.take());
            return nfsrv_read_bad(
                nfsd,
                slp,
                mrq,
                Errno::ENOBUFS.as_i32(),
                getret_ok.then_some(&va),
            );
        }
        let mut iv: Vec<Iovec> = Vec::with_capacity(i);
        let mut m = Some(mb);
        let mut left = len;
        while left > 0 {
            let Some(mm) = m else {
                panic(format_args!("nfsrv_read iov"));
            };
            let siz = (m_trailingspace(mm).max(0) as usize).min(left);
            if siz > 0 {
                let mlen = mm.m_len().get();
                iv.push(Iovec {
                    iov_base: mtod::<u8>(mm).wrapping_add(mlen as usize).cast::<c_void>(),
                    iov_len: siz,
                });
                mm.m_len().set(mlen + siz as u32);
                left -= siz;
            }
            m = mm.m_next().get();
        }
        let mut uio = Uio {
            uio_iov: &mut iv,
            uio_offset: off,
            uio_resid: len,
            uio_segflg: UioSeg::UIO_SYSSPACE,
            uio_rw: UioRw::UIO_READ,
            uio_procp: None,
        };
        let r = VOP_READ(vp, &mut uio, IO_NODELOCKED, cred);
        resid = uio.uio_resid;
        drop(iv);
        let error = match r {
            Err(e) => e.as_i32(),
            Ok(()) => {
                let g = VOP_GETATTR(vp, &mut va, cred, procp);
                getret_ok = g.is_ok();
                status_of(&g)
            }
        };
        if error != 0 {
            // vbad: the C builds a second reply over the first and leaks it.
            vput(vp);
            m_freem(mrq.take());
            return nfsrv_read_bad(nfsd, slp, mrq, error, getret_ok.then_some(&va));
        }
    }
    vput(vp);
    let fp = nfsm_srvfattr(nfsd, &va);
    let tlen = len - resid;
    cnt = cnt.min(tlen);
    let tlen = nfsm_rndup(cnt);
    if len != tlen || tlen != cnt {
        nfsm_adj(hdr, (len - tlen) as i32, (tlen - cnt) as i32);
    }
    let mut tl = nfsm_rebuild(hdr, hdroff, hdrlen);
    if v3 {
        tl.set(0, nfs_true);
        tl.write(NFSX_UNSIGNED, &fp);
        let w = (NFSX_UNSIGNED + NFSX_V3FATTR) / NFSX_UNSIGNED;
        tl.set(w, txdr_unsigned(cnt as u32));
        tl.set(
            w + 1,
            if len < reqlen as usize {
                nfs_true
            } else {
                nfs_false
            },
        );
        tl.set(w + 2, txdr_unsigned(cnt as u32));
    } else {
        tl.write(0, &fp);
        tl.set(NFSX_V2FATTR / NFSX_UNSIGNED, txdr_unsigned(cnt as u32));
    }
    Ok(())
}

/// `bad:`/`vbad:` of `nfsrv_write`: the error reply with the weak cache consistency data.
fn nfsrv_write_bad(
    nfsd: &mut NfsrvDescript,
    slp: &NfssvcSock,
    mrq: &mut Option<&'static Mbuf>,
    error: i32,
    forat: Option<&Vattr>,
) -> Result<(), Errno> {
    if let Some(mut mb) = nfsm_reply(nfsd, slp, mrq, error, 0)? {
        nfsm_srvwcc(nfsd, forat, None, &mut mb);
    }
    Ok(())
}

/// `nfs write service`
pub fn nfsrv_write(
    nfsd: &mut NfsrvDescript,
    slp: &NfssvcSock,
    procp: &Proc,
    mrq: &mut Option<&'static Mbuf>,
) -> Result<(), Errno> {
    let cred: *const Ucred = &nfsd.nd_cr;
    let v3 = nfsd.nd_flag & ND_NFSV3 != 0;

    if nfsd.nd_mrep.is_none() {
        *mrq = None;
        return Ok(());
    }
    let Some(nfh) = nfsm_srvmtofh(nfsd, slp, mrq)? else {
        return Ok(());
    };
    let fh = nfh.fh_generic();
    let (off, stable, len): (Off, u32, i32) = if v3 {
        let tl = nfsd_dissect(nfsd, 5 * NFSX_UNSIGNED)?;
        (
            tl.fxdr_hyper(0) as Off,
            fxdr_unsigned(tl.get(3)),
            fxdr_unsigned(tl.get(4)) as i32,
        )
    } else {
        let tl = nfsd_dissect(nfsd, 4 * NFSX_UNSIGNED)?;
        (
            Off::from(fxdr_unsigned(tl.get(1))),
            NFSV3WRITE_FILESYNC,
            fxdr_unsigned(tl.get(3)) as i32,
        )
    };
    let retlen = len;
    let mut cnt = 0usize;
    let mut i: i64 = 0;

    // For NFS Version 2, it is not obvious what a write of zero length should do, but I
    // might as well be consistent with Version 3, which is to return ok so long as there
    // are no permission problems.
    if len > 0 {
        let mut zeroing = true;
        let mut mp = nfsd.nd_mrep;
        while let Some(m) = mp {
            if nfsd.nd_md.is_some_and(|md| ptr::eq(md, m)) {
                zeroing = false;
                let mlen = m.m_len().get() as usize;
                let adjust = mlen - nfsm_avail(m, nfsd.nd_dpos);
                m.m_len().set((mlen - adjust) as u32);
                if m.m_len().get() > 0 && adjust > 0 {
                    m.m_data().set(m.m_data().get().wrapping_add(adjust));
                }
            }
            if zeroing {
                m.m_len().set(0);
            } else if m.m_len().get() > 0 {
                i += i64::from(m.m_len().get());
                if i > i64::from(len) {
                    let over = (i - i64::from(len)) as u32;
                    m.m_len().set(m.m_len().get() - over);
                    zeroing = true;
                }
                if m.m_len().get() > 0 {
                    cnt += 1;
                }
            }
            mp = m.m_next().get();
        }
    }
    if len > NFS_MAXDATA as i32 || len < 0 || i < i64::from(len) {
        return nfsrv_write_bad(nfsd, slp, mrq, Errno::EIO.as_i32(), None);
    }
    let (vp, rdonly) = match fhtovp(nfsd, &fh, true, slp) {
        Ok(x) => x,
        Err(error) => return nfsrv_write_bad(nfsd, slp, mrq, error, None),
    };
    let mut forat = Vattr::new();
    let forat_ok = v3 && VOP_GETATTR(vp, &mut forat, cred, procp).is_ok();
    if vp.v_type.get() != VREG {
        let error = if v3 {
            Errno::EINVAL
        } else if vp.v_type.get() == VDIR {
            Errno::EISDIR
        } else {
            Errno::EACCES
        };
        vput(vp);
        return nfsrv_write_bad(nfsd, slp, mrq, error.as_i32(), forat_ok.then_some(&forat));
    }
    if let Err(e) = nfsrv_access(vp, VWRITE, &nfsd.nd_cr, rdonly, procp, true) {
        vput(vp);
        return nfsrv_write_bad(nfsd, slp, mrq, e.as_i32(), forat_ok.then_some(&forat));
    }

    let mut r = Ok(());
    if len > 0 {
        let mut iv: Vec<Iovec> = Vec::with_capacity(cnt);
        let mut mp = nfsd.nd_mrep;
        while let Some(m) = mp {
            if m.m_len().get() > 0 {
                iv.push(Iovec {
                    iov_base: mtod::<c_void>(m),
                    iov_len: m.m_len().get() as usize,
                });
            }
            mp = m.m_next().get();
        }

        // NFSV3WRITE_DATASYNC and NFSV3WRITE_FILESYNC are both written synchronously.
        let ioflags = if stable == NFSV3WRITE_UNSTABLE {
            IO_NODELOCKED
        } else {
            IO_SYNC | IO_NODELOCKED
        };
        let mut uio = Uio {
            uio_iov: &mut iv,
            uio_offset: off,
            uio_resid: len as usize,
            uio_segflg: UioSeg::UIO_SYSSPACE,
            uio_rw: UioRw::UIO_WRITE,
            uio_procp: None,
        };
        r = VOP_WRITE(vp, &mut uio, ioflags, cred);
        NFSSTATS.srvvop_writes.fetch_add(1, Relaxed);
    }
    let mut va = Vattr::new();
    let aftat = VOP_GETATTR(vp, &mut va, cred, procp);
    vput(vp);
    let error = if r.is_ok() {
        status_of(&aftat)
    } else {
        status_of(&r)
    };
    let Some(mut mb) = nfsm_reply(
        nfsd,
        slp,
        mrq,
        error,
        nfsx_preopattr(v3) + nfsx_postoporfattr(v3) + 2 * NFSX_UNSIGNED + nfsx_writeverf(v3),
    )?
    else {
        return Ok(());
    };
    if v3 {
        nfsm_srvwcc(
            nfsd,
            forat_ok.then_some(&forat),
            aftat.is_ok().then_some(&va),
            &mut mb,
        );
        if error != 0 {
            return Ok(());
        }
        let mut tl = nfsm_build(&mut mb, 4 * NFSX_UNSIGNED);
        tl.put(txdr_unsigned(retlen as u32));
        if stable == NFSV3WRITE_UNSTABLE {
            tl.put(txdr_unsigned(stable));
        } else {
            tl.put(txdr_unsigned(NFSV3WRITE_FILESYNC));
        }
        // Actually, there is no need to txdr these fields, but it may make the values more
        // human readable, for debugging purposes.
        let boottime = microboottime();
        tl.put(txdr_unsigned(boottime.tv_sec as u32));
        tl.put(txdr_unsigned(boottime.tv_usec as u32));
    } else {
        let fp = nfsm_srvfattr(nfsd, &va);
        nfsm_build(&mut mb, NFSX_V2FATTR).write(0, &fp);
    }
    Ok(())
}

/// `nfsm_srvpostop_fh(mb, fhp)`: a version 3 `post_op_fh3` holding the file handle.
fn nfsm_srvpostop_fh(mb: &mut &'static Mbuf, nfh: &Nfsfh) {
    let mut tl = nfsm_build(mb, 2 * NFSX_UNSIGNED + NFSX_V3FH);
    tl.set(0, nfs_true);
    tl.set(1, txdr_unsigned(NFSX_V3FH as u32));
    tl.set_bytes(2 * NFSX_UNSIGNED, &nfh.fh_bytes[..NFSX_V3FH]);
}

/// The arguments of a CREATE after its name: the version 3 `createhow3` (`sattr3` or the
/// `createverf3`), or the version 2 `sattr`. `exists` is whether the name is there
/// (`nd.ni_vp`). Returns the C's `error` (`EEXIST` for a `GUARDED` create of an existing
/// file); `Err` is the `goto nfsmout` of a bad request.
fn nfsrv_create_args(
    nfsd: &mut NfsrvDescript,
    exists: bool,
    va: &mut Vattr,
    cverf: &mut [u8; NFSX_V3CREATEVERF],
    exclusive_flag: &mut bool,
    rdev: &mut u32,
) -> Result<i32, Errno> {
    let mut error = 0;
    if nfsd.nd_flag & ND_NFSV3 != 0 {
        let how = fxdr_unsigned(nfsd_dissect(nfsd, NFSX_UNSIGNED)?.get(0));
        match how {
            NFSV3CREATE_GUARDED if exists => error = Errno::EEXIST.as_i32(),
            NFSV3CREATE_GUARDED | NFSV3CREATE_UNCHECKED => nfsm_srvsattr(nfsd, va)?,
            NFSV3CREATE_EXCLUSIVE => {
                cverf.copy_from_slice(nfsd_dissect(nfsd, NFSX_V3CREATEVERF)?.bytes());
                *exclusive_flag = true;
                if !exists {
                    va.va_mode = 0;
                }
            }
            _ => {}
        }
        va.va_type = VREG;
    } else {
        let sp: Nfsv2Sattr = nfsd_dissect(nfsd, NFSX_V2SATTR)?.read(0);
        va.va_type = iftovt(fxdr_unsigned(sp.sa_mode) as Mode);
        if va.va_type == VNON {
            va.va_type = VREG;
        }
        va.va_mode = nfstov_mode(sp.sa_mode);
        match va.va_type {
            VREG => {
                let tsize = fxdr_unsigned(sp.sa_size) as i32;
                if tsize != -1 {
                    va.va_size = tsize as u64;
                }
            }
            VCHR | VBLK | VFIFO => {
                *rdev = fxdr_unsigned(sp.sa_size);
            }
            _ => {}
        }
    }
    Ok(error)
}

/// `nfsmout:` of `nfsrv_create` and `nfsrv_mknod`: releases what the name lookup returned
/// and passes the error of the bad request on.
fn nfsrv_create_nfsmout(
    nd: &mut Nameidata<'_>,
    dirp: Option<&'static Vnode>,
    error: Errno,
) -> Result<(), Errno> {
    if let Some(dirp) = dirp {
        vrele(dirp);
    }
    if nd.ni_cnd.cn_nameiop != LOOKUP {
        startdir_rele(nd);
        pnbuf_put_hasbuf(&mut nd.ni_cnd);
    }
    abortop(nd);
    dvp_release(nd);
    if let Some(vp) = nd.ni_vp {
        vput(vp);
    }
    Err(error)
}

/// `nfs create service`: now does a truncate to 0 length via setattr if it already exists.
pub fn nfsrv_create(
    nfsd: &mut NfsrvDescript,
    slp: &NfssvcSock,
    procp: &Proc,
    mrq: &mut Option<&'static Mbuf>,
) -> Result<(), Errno> {
    let cred: *const Ucred = &nfsd.nd_cr;
    let v3 = nfsd.nd_flag & ND_NFSV3 != 0;

    let Some(nfh) = nfsm_srvmtofh(nfsd, slp, mrq)? else {
        return Ok(());
    };
    let fh = nfh.fh_generic();
    let (len, error) = nfsm_srvnamesiz(nfsd)?;
    if error != 0 {
        // nfsm_reply would return zero if v3 and an error different from EBADRPC. But it
        // does not make sense to continue anyway if the error set in nfsm_srvnamesiz is
        // NFSERR_NAMETOL.
        nfsm_reply(nfsd, slp, mrq, error, 0)?;
        return Ok(());
    }

    let mut nd = ndinit(
        CREATE,
        LOCKPARENT | LOCKLEAF | SAVESTART,
        NiDirp::Sys(&[]),
        procp,
    );
    nd.ni_cnd.cn_cred = cred;
    let mut dirp = None;
    let r = namei(nfsd, &mut nd, &fh, len, slp, &mut dirp, procp);
    let mut dirfor = Vattr::new();
    let mut dirfor_ok = false;
    if let Some(d) = dirp {
        if v3 {
            dirfor_ok = VOP_GETATTR(d, &mut dirfor, cred, procp).is_ok();
        } else {
            vrele(d);
            dirp = None;
        }
    }
    if let Err(error) = r {
        let reply = nfsm_reply(nfsd, slp, mrq, error, nfsx_wccdata(v3));
        if let Some(d) = dirp {
            vrele(d);
        }
        if let Some(mut mb) = reply? {
            nfsm_srvwcc(nfsd, dirfor_ok.then_some(&dirfor), None, &mut mb);
        }
        return Ok(());
    }

    let mut va = Vattr::new();
    vattr_null(&mut va);
    let mut cverf = [0u8; NFSX_V3CREATEVERF];
    let mut exclusive_flag = false;
    let mut rdev: u32 = 0;
    let mut error = match nfsrv_create_args(
        nfsd,
        nd.ni_vp.is_some(),
        &mut va,
        &mut cverf,
        &mut exclusive_flag,
        &mut rdev,
    ) {
        Ok(error) => error,
        Err(e) => return nfsrv_create_nfsmout(&mut nd, dirp, e),
    };

    // Iff doesn't exist, create it
    // otherwise just truncate to 0 length
    //   should I set the mode too ??
    let vp: Option<&'static Vnode>;
    if nd.ni_vp.is_none() {
        let dvp = ni_dvp(&nd);
        if va.va_type == VREG || va.va_type == VSOCK {
            startdir_rele(&mut nd);
            error = status_of(&VOP_CREATE(dvp, &mut nd.ni_vp, &mut nd.ni_cnd, &mut va));
            vput(dvp);
            if error == 0 {
                pnbuf_put(&mut nd.ni_cnd);
                if exclusive_flag {
                    exclusive_flag = false;
                    vattr_null(&mut va);
                    // bcopy(cverf, &va.va_atime, NFSX_V3CREATEVERF)
                    va.va_atime.tv_sec = i64::from_ne_bytes(cverf);
                    error = status_of(&VOP_SETATTR(ni_vp(&nd), &mut va, cred, procp));
                }
            }
        } else if va.va_type == VCHR || va.va_type == VBLK || va.va_type == VFIFO {
            if va.va_type == VCHR && rdev == 0xffff_ffff {
                va.va_type = VFIFO;
            }
            if va.va_type != VFIFO
                && let Err(e) = suser_ucred(&nfsd.nd_cr)
            {
                startdir_rele(&mut nd);
                pnbuf_put_hasbuf(&mut nd.ni_cnd);
                let _ = VOP_ABORTOP(dvp, &mut nd.ni_cnd);
                vput(dvp);
                // Version 2 only: nfsm_reply ends the call.
                nfsm_reply(nfsd, slp, mrq, e.as_i32(), 0)?;
                return Ok(());
            }
            va.va_rdev = rdev as Dev;
            let r = VOP_MKNOD(dvp, &mut nd.ni_vp, &mut nd.ni_cnd, &mut va);
            vput(dvp);
            if let Err(e) = r {
                startdir_rele(&mut nd);
                // The file system gave the name buffer back (the C frees it again).
                nfsm_reply(nfsd, slp, mrq, e.as_i32(), 0)?;
                return Ok(());
            }
            nfsrv_relookup_pathlen(&mut nd);
            nd.ni_cnd.cn_nameiop = LOOKUP;
            nd.ni_cnd.cn_flags &= !(LOCKPARENT | SAVESTART);
            nd.ni_cnd.cn_proc = procp;
            nd.ni_cnd.cn_cred = cred;
            if let Err(e) = vfs_lookup(&mut nd) {
                pnbuf_put_hasbuf(&mut nd.ni_cnd);
                nfsm_reply(nfsd, slp, mrq, e.as_i32(), 0)?;
                return Ok(());
            }

            pnbuf_put(&mut nd.ni_cnd);
            if nd.ni_cnd.cn_flags & ISSYMLINK != 0 {
                if let Some(d) = nd.ni_dvp {
                    vrele(d);
                }
                vput(ni_vp(&nd));
                abortop(&mut nd);
                nfsm_reply(nfsd, slp, mrq, Errno::EINVAL.as_i32(), 0)?;
                return Ok(());
            }
        } else {
            startdir_rele(&mut nd);
            pnbuf_put(&mut nd.ni_cnd);
            nd.ni_cnd.cn_flags &= !HASBUF;
            let _ = VOP_ABORTOP(dvp, &mut nd.ni_cnd);
            vput(dvp);
            error = Errno::ENXIO.as_i32();
        }
        vp = nd.ni_vp;
    } else {
        startdir_rele(&mut nd);
        pnbuf_put(&mut nd.ni_cnd);
        nd.ni_cnd.cn_flags &= !HASBUF;
        let evp = ni_vp(&nd);
        vp = Some(evp);
        dvp_release(&nd);
        abortop(&mut nd);
        if va.va_size != VNOVAL_SIZE {
            let rdonly = nd.ni_cnd.cn_flags & RDONLY != 0;
            let r = nfsrv_access(evp, VWRITE, &nfsd.nd_cr, rdonly, procp, false).and_then(|()| {
                let tempsize = va.va_size;
                vattr_null(&mut va);
                va.va_size = tempsize;
                VOP_SETATTR(evp, &mut va, cred, procp)
            });
            error = status_of(&r);
            if error != 0 {
                vput(evp);
            }
        } else if error != 0 {
            // A GUARDED create of an existing file: the C leaves it locked.
            vput(evp);
        }
    }
    let mut nfh = Nfsfh::new();
    if error == 0 {
        let vp = match vp {
            Some(vp) => vp,
            None => panic(format_args!("nfsrv_create: created without a vnode")),
        };
        error = match nfsrv_vptofh(vp) {
            Ok(f) => {
                nfh = f;
                status_of(&VOP_GETATTR(vp, &mut va, cred, procp))
            }
            Err(e) => e.as_i32(),
        };
        vput(vp);
    }
    let mut diraft = Vattr::new();
    let mut diraft_ok = false;
    if v3 {
        if exclusive_flag && error == 0 && va.va_atime.tv_sec.to_ne_bytes() != cverf {
            error = Errno::EEXIST.as_i32();
        }
        if let Some(d) = dirp {
            diraft_ok = VOP_GETATTR(d, &mut diraft, cred, procp).is_ok();
            vrele(d);
        }
    }
    let Some(mut mb) = nfsm_reply(
        nfsd,
        slp,
        mrq,
        error,
        nfsx_srvfh(v3) + nfsx_fattr(v3) + nfsx_wccdata(v3),
    )?
    else {
        return Ok(());
    };
    if v3 {
        if error == 0 {
            nfsm_srvpostop_fh(&mut mb, &nfh);
            nfsm_srvpostop_attr(nfsd, Some(&va), &mut mb);
        }
        nfsm_srvwcc(
            nfsd,
            dirfor_ok.then_some(&dirfor),
            diraft_ok.then_some(&diraft),
            &mut mb,
        );
    } else {
        nfsm_srvfhtom(&mut mb, &nfh, v3);
        let fp = nfsm_srvfattr(nfsd, &va);
        nfsm_build(&mut mb, NFSX_V2FATTR).write(0, &fp);
    }
    Ok(())
}

/// `nfs v3 mknod service`
pub fn nfsrv_mknod(
    nfsd: &mut NfsrvDescript,
    slp: &NfssvcSock,
    procp: &Proc,
    mrq: &mut Option<&'static Mbuf>,
) -> Result<(), Errno> {
    let cred: *const Ucred = &nfsd.nd_cr;

    let Some(nfh) = nfsm_srvmtofh(nfsd, slp, mrq)? else {
        return Ok(());
    };
    let fh = nfh.fh_generic();
    let (len, error) = nfsm_srvnamesiz(nfsd)?;
    if error != 0 {
        // nfsm_reply would return zero if v3 and an error different from EBADRPC. But it
        // does not make sense to continue anyway if the error set in nfsm_srvnamesiz is
        // NFSERR_NAMETOL.
        nfsm_reply(nfsd, slp, mrq, error, 0)?;
        return Ok(());
    }

    let mut nd = ndinit(
        CREATE,
        LOCKPARENT | LOCKLEAF | SAVESTART,
        NiDirp::Sys(&[]),
        procp,
    );
    nd.ni_cnd.cn_cred = cred;
    let mut dirp = None;
    let r = namei(nfsd, &mut nd, &fh, len, slp, &mut dirp, procp);
    let mut dirfor = Vattr::new();
    let mut dirfor_ok = false;
    if let Some(d) = dirp {
        dirfor_ok = VOP_GETATTR(d, &mut dirfor, cred, procp).is_ok();
    }
    if let Err(error) = r {
        let reply = nfsm_reply(nfsd, slp, mrq, error, nfsx_wccdata(true));
        if let Some(d) = dirp {
            vrele(d);
        }
        if let Some(mut mb) = reply? {
            nfsm_srvwcc(nfsd, dirfor_ok.then_some(&dirfor), None, &mut mb);
        }
        return Ok(());
    }

    let vtyp = match nfsd_dissect(nfsd, NFSX_UNSIGNED) {
        Ok(tl) => nfsv3tov_type(tl.get(0)),
        Err(e) => return nfsrv_create_nfsmout(&mut nd, dirp, e),
    };
    let mut va = Vattr::new();
    let error = 'out: {
        if vtyp != VCHR && vtyp != VBLK && vtyp != VSOCK && vtyp != VFIFO {
            startdir_rele(&mut nd);
            pnbuf_put(&mut nd.ni_cnd);
            abortop(&mut nd);
            dvp_release(&nd);
            if let Some(vp) = nd.ni_vp {
                vput(vp);
            }
            break 'out NFSERR_BADTYPE;
        }
        vattr_null(&mut va);
        if let Err(e) = nfsm_srvsattr(nfsd, &mut va) {
            return nfsrv_create_nfsmout(&mut nd, dirp, e);
        }
        if vtyp == VCHR || vtyp == VBLK {
            let (major, minor) = match nfsd_dissect(nfsd, 2 * NFSX_UNSIGNED) {
                Ok(tl) => (fxdr_unsigned(tl.get(0)), fxdr_unsigned(tl.get(1))),
                Err(e) => return nfsrv_create_nfsmout(&mut nd, dirp, e),
            };
            va.va_rdev = makedev(major, minor);
        }

        // Iff doesn't exist, create it.
        if let Some(vp) = nd.ni_vp {
            startdir_rele(&mut nd);
            pnbuf_put(&mut nd.ni_cnd);
            abortop(&mut nd);
            dvp_release(&nd);
            vput(vp);
            break 'out Errno::EEXIST.as_i32();
        }
        let dvp = ni_dvp(&nd);
        va.va_type = vtyp;
        if vtyp == VSOCK {
            startdir_rele(&mut nd);
            let r = VOP_CREATE(dvp, &mut nd.ni_vp, &mut nd.ni_cnd, &mut va);
            vput(dvp);
            if r.is_ok() {
                pnbuf_put(&mut nd.ni_cnd);
            }
            status_of(&r)
        } else {
            if va.va_type != VFIFO
                && let Err(e) = suser_ucred(&nfsd.nd_cr)
            {
                startdir_rele(&mut nd);
                pnbuf_put(&mut nd.ni_cnd);
                let _ = VOP_ABORTOP(dvp, &mut nd.ni_cnd);
                vput(dvp);
                break 'out e.as_i32();
            }
            let r = VOP_MKNOD(dvp, &mut nd.ni_vp, &mut nd.ni_cnd, &mut va);
            vput(dvp);
            if let Err(e) = r {
                startdir_rele(&mut nd);
                break 'out e.as_i32();
            }
            nfsrv_relookup_pathlen(&mut nd);
            nd.ni_cnd.cn_nameiop = LOOKUP;
            nd.ni_cnd.cn_flags &= !(LOCKPARENT | SAVESTART);
            nd.ni_cnd.cn_proc = procp;
            nd.ni_cnd.cn_cred = procp.p_ucred.get();
            let r = vfs_lookup(&mut nd);
            pnbuf_put(&mut nd.ni_cnd);
            if let Err(e) = r {
                break 'out e.as_i32();
            }
            if nd.ni_cnd.cn_flags & ISSYMLINK != 0 {
                if let Some(d) = nd.ni_dvp {
                    vrele(d);
                }
                vput(ni_vp(&nd));
                abortop(&mut nd);
                break 'out Errno::EINVAL.as_i32();
            }
            0
        }
    };
    // out:
    let mut error = error;
    let mut nfh = Nfsfh::new();
    if error == 0 {
        let vp = ni_vp(&nd);
        error = match nfsrv_vptofh(vp) {
            Ok(f) => {
                nfh = f;
                status_of(&VOP_GETATTR(vp, &mut va, cred, procp))
            }
            Err(e) => e.as_i32(),
        };
        vput(vp);
    }
    let mut diraft = Vattr::new();
    let mut diraft_ok = false;
    if let Some(d) = dirp {
        diraft_ok = VOP_GETATTR(d, &mut diraft, cred, procp).is_ok();
        vrele(d);
    }
    let Some(mut mb) = nfsm_reply(
        nfsd,
        slp,
        mrq,
        error,
        nfsx_srvfh(true) + nfsx_postopattr(true) + nfsx_wccdata(true),
    )?
    else {
        return Ok(());
    };
    if error == 0 {
        nfsm_srvpostop_fh(&mut mb, &nfh);
        nfsm_srvpostop_attr(nfsd, Some(&va), &mut mb);
    }
    nfsm_srvwcc(
        nfsd,
        dirfor_ok.then_some(&dirfor),
        diraft_ok.then_some(&diraft),
        &mut mb,
    );
    Ok(())
}

/// `nfs remove service`
pub fn nfsrv_remove(
    nfsd: &mut NfsrvDescript,
    slp: &NfssvcSock,
    procp: &Proc,
    mrq: &mut Option<&'static Mbuf>,
) -> Result<(), Errno> {
    let cred: *const Ucred = &nfsd.nd_cr;
    let v3 = nfsd.nd_flag & ND_NFSV3 != 0;

    let Some(nfh) = nfsm_srvmtofh(nfsd, slp, mrq)? else {
        return Ok(());
    };
    let fh = nfh.fh_generic();
    let (len, error) = nfsm_srvnamesiz(nfsd)?;
    if error != 0 {
        // nfsm_reply would return zero if v3 and an error different from EBADRPC. But it
        // does not make sense to continue anyway if the error set in nfsm_srvnamesiz is
        // NFSERR_NAMETOL.
        nfsm_reply(nfsd, slp, mrq, error, 0)?;
        return Ok(());
    }

    let mut nd = ndinit(DELETE, LOCKPARENT | LOCKLEAF, NiDirp::Sys(&[]), procp);
    nd.ni_cnd.cn_cred = cred;
    let mut dirp = None;
    let r = namei(nfsd, &mut nd, &fh, len, slp, &mut dirp, procp);
    let mut dirfor = Vattr::new();
    let mut dirfor_ok = false;
    if let Some(d) = dirp {
        if v3 {
            dirfor_ok = VOP_GETATTR(d, &mut dirfor, cred, procp).is_ok();
        } else {
            vrele(d);
            dirp = None;
        }
    }

    let error = match r {
        Err(error) => error,
        Ok(()) => {
            let vp = ni_vp(&nd);
            let error = 'out: {
                if vp.v_type.get() == VDIR
                    && let Err(e) = suser_ucred(&nfsd.nd_cr)
                {
                    break 'out e.as_i32();
                }
                // The root of a mounted filesystem cannot be deleted.
                if vp.v_flag.get() & VROOT != 0 {
                    break 'out Errno::EBUSY.as_i32();
                }
                if vp.v_flag.get() & VTEXT != 0 {
                    let _ = uvm_vnp_uncache(vp);
                }
                0
            };
            // out:
            if error == 0 {
                status_of(&VOP_REMOVE(ni_dvp(&nd), vp, &mut nd.ni_cnd))
            } else {
                abortop(&mut nd);
                dvp_release(&nd);
                vput(vp);
                error
            }
        }
    };
    let mut diraft = Vattr::new();
    let mut diraft_ok = false;
    if v3 && let Some(d) = dirp {
        diraft_ok = VOP_GETATTR(d, &mut diraft, cred, procp).is_ok();
        vrele(d);
    }
    let Some(mut mb) = nfsm_reply(nfsd, slp, mrq, error, nfsx_wccdata(v3))? else {
        return Ok(());
    };
    if v3 {
        nfsm_srvwcc(
            nfsd,
            dirfor_ok.then_some(&dirfor),
            diraft_ok.then_some(&diraft),
            &mut mb,
        );
    }
    Ok(())
}

/// `nfsmout:` of `nfsrv_rename`: releases what the lookup of the source returned and passes
/// `r` on (`Ok` after a reply is in `*mrq`, `Err` for a bad request).
fn nfsrv_rename_nfsmout(
    fromnd: &mut Nameidata<'_>,
    fdirp: Option<&'static Vnode>,
    fvp: Option<&'static Vnode>,
    r: Result<(), Errno>,
) -> Result<(), Errno> {
    if let Some(d) = fdirp {
        vrele(d);
    }
    if fromnd.ni_cnd.cn_nameiop != LOOKUP {
        startdir_rele(fromnd);
        abortop(fromnd);
        // The C skips this when ni_dvp is fdirp ("XXX: Workaround"), which leaks the
        // lookup's own reference: fdirp holds another one, released above.
        if let Some(dvp) = fromnd.ni_dvp {
            vrele(dvp);
        }
        if let Some(fvp) = fvp {
            vrele(fvp);
        }
        pnbuf_put(&mut fromnd.ni_cnd);
    }
    r
}

/// `nfs rename service`
pub fn nfsrv_rename(
    nfsd: &mut NfsrvDescript,
    slp: &NfssvcSock,
    procp: &Proc,
    mrq: &mut Option<&'static Mbuf>,
) -> Result<(), Errno> {
    let cred: *const Ucred = &nfsd.nd_cr;
    let v3 = nfsd.nd_flag & ND_NFSV3 != 0;

    let Some(fnfh) = nfsm_srvmtofh(nfsd, slp, mrq)? else {
        return Ok(());
    };
    let ffh = fnfh.fh_generic();
    let (len, error) = nfsm_srvnamesiz(nfsd)?;
    if error != 0 {
        // nfsm_reply would return zero if v3 and an error different from EBADRPC. But it
        // does not make sense to continue anyway if the error set in nfsm_srvnamesiz is
        // NFSERR_NAMETOL.
        nfsm_reply(nfsd, slp, mrq, error, 0)?;
        return Ok(());
    }

    // Remember our original uid so that we can reset cr_uid before the second nfs_namei()
    // call, in case it is remapped.
    let saved_uid = nfsd.nd_cr.cr_uid.get();

    let mut fromnd = ndinit(DELETE, WANTPARENT | SAVESTART, NiDirp::Sys(&[]), procp);
    fromnd.ni_cnd.cn_cred = cred;
    let mut fdirp = None;
    let r = namei(nfsd, &mut fromnd, &ffh, len, slp, &mut fdirp, procp);
    let mut fdirfor = Vattr::new();
    let mut fdirfor_ok = false;
    if let Some(d) = fdirp {
        if v3 {
            fdirfor_ok = VOP_GETATTR(d, &mut fdirfor, cred, procp).is_ok();
        } else {
            vrele(d);
            fdirp = None;
        }
    }
    if let Err(error) = r {
        let reply = nfsm_reply(nfsd, slp, mrq, error, 2 * nfsx_wccdata(v3));
        if let Some(d) = fdirp {
            vrele(d);
        }
        if let Some(mut mb) = reply? {
            nfsm_srvwcc(nfsd, fdirfor_ok.then_some(&fdirfor), None, &mut mb);
            nfsm_srvwcc(nfsd, None, None, &mut mb);
        }
        return Ok(());
    }

    let fvp = ni_vp(&fromnd);
    let tnfh = match nfsm_srvmtofh(nfsd, slp, mrq) {
        Ok(Some(f)) => f,
        Ok(None) => return nfsrv_rename_nfsmout(&mut fromnd, fdirp, Some(fvp), Ok(())),
        Err(e) => return nfsrv_rename_nfsmout(&mut fromnd, fdirp, Some(fvp), Err(e)),
    };
    let tfh = tnfh.fh_generic();
    let len2 = match nfsd_strsiz(nfsd, NFS_MAXNAMLEN) {
        Ok(l) => l,
        Err(e) => return nfsrv_rename_nfsmout(&mut fromnd, fdirp, Some(fvp), Err(e)),
    };
    nfsd.nd_cr.cr_uid.set(saved_uid);

    let mut tond = ndinit(
        RENAME,
        LOCKPARENT | LOCKLEAF | NOCACHE | SAVESTART,
        NiDirp::Sys(&[]),
        procp,
    );
    tond.ni_cnd.cn_cred = cred;
    let mut tdirp = None;
    let r = namei(nfsd, &mut tond, &tfh, len2, slp, &mut tdirp, procp);
    let mut tdirfor = Vattr::new();
    let mut tdirfor_ok = false;
    if let Some(d) = tdirp {
        if v3 {
            tdirfor_ok = VOP_GETATTR(d, &mut tdirfor, cred, procp).is_ok();
        } else {
            vrele(d);
            tdirp = None;
        }
    }
    let fdvp = ni_dvp(&fromnd);
    let error = match r {
        Err(error) => {
            let _ = VOP_ABORTOP(fdvp, &mut fromnd.ni_cnd);
            vrele(fdvp);
            vrele(fvp);
            error
        }
        Ok(()) => {
            let tdvp = ni_dvp(&tond);
            let tvp = tond.ni_vp;
            let pick = |e3: Errno, e2: Errno| (if v3 { e3 } else { e2 }).as_i32();
            let mut error = 'out: {
                if let Some(tvp) = tvp {
                    let fdir = fvp.v_type.get() == VDIR;
                    let tdir = tvp.v_type.get() == VDIR;
                    if fdir && !tdir {
                        break 'out pick(Errno::EEXIST, Errno::EISDIR);
                    } else if !fdir && tdir {
                        break 'out pick(Errno::EEXIST, Errno::ENOTDIR);
                    }
                    if tdir && tvp.v_mountedhere().is_some() {
                        break 'out pick(Errno::EXDEV, Errno::ENOTEMPTY);
                    }
                }
                if fvp.v_type.get() == VDIR && fvp.v_mountedhere().is_some() {
                    break 'out pick(Errno::EXDEV, Errno::ENOTEMPTY);
                }
                if !same_mount(fvp, tdvp) {
                    break 'out pick(Errno::EXDEV, Errno::ENOTEMPTY);
                }
                let mut error = 0;
                if ptr::eq(fvp, tdvp) {
                    error = pick(Errno::EINVAL, Errno::ENOTEMPTY);
                }
                // If source is the same as the destination (that is the same vnode with the
                // same name in the same directory), then there is nothing to do.
                if same_vnode(Some(fvp), tvp)
                    && ptr::eq(fdvp, tdvp)
                    && fromnd.ni_cnd.name() == tond.ni_cnd.name()
                {
                    error = -1;
                }
                error
            };
            // out:
            if error == 0 {
                if let Some(tvp) = tvp {
                    let _ = uvm_vnp_uncache(tvp);
                }
                error = status_of(&VOP_RENAME(
                    fdvp,
                    fvp,
                    &mut fromnd.ni_cnd,
                    tdvp,
                    tvp,
                    &mut tond.ni_cnd,
                ));
            } else {
                let _ = VOP_ABORTOP(tdvp, &mut tond.ni_cnd);
                if same_vnode(Some(tdvp), tvp) {
                    vrele(tdvp);
                } else {
                    vput(tdvp);
                }
                if let Some(tvp) = tvp {
                    vput(tvp);
                }
                let _ = VOP_ABORTOP(fdvp, &mut fromnd.ni_cnd);
                vrele(fdvp);
                vrele(fvp);
                if error == -1 {
                    error = 0;
                }
            }
            startdir_rele(&mut tond);
            pnbuf_put(&mut tond.ni_cnd);
            error
        }
    };
    // out1:
    let mut fdiraft = Vattr::new();
    let mut fdiraft_ok = false;
    if let Some(d) = fdirp {
        fdiraft_ok = VOP_GETATTR(d, &mut fdiraft, cred, procp).is_ok();
        vrele(d);
    }
    let mut tdiraft = Vattr::new();
    let mut tdiraft_ok = false;
    if let Some(d) = tdirp {
        tdiraft_ok = VOP_GETATTR(d, &mut tdiraft, cred, procp).is_ok();
        vrele(d);
    }
    startdir_rele(&mut fromnd);
    pnbuf_put(&mut fromnd.ni_cnd);
    let Some(mut mb) = nfsm_reply(nfsd, slp, mrq, error, 2 * nfsx_wccdata(v3))? else {
        return Ok(());
    };
    if v3 {
        nfsm_srvwcc(
            nfsd,
            fdirfor_ok.then_some(&fdirfor),
            fdiraft_ok.then_some(&fdiraft),
            &mut mb,
        );
        nfsm_srvwcc(
            nfsd,
            tdirfor_ok.then_some(&tdirfor),
            tdiraft_ok.then_some(&tdiraft),
            &mut mb,
        );
    }
    Ok(())
}

/// `nfs link service`
pub fn nfsrv_link(
    nfsd: &mut NfsrvDescript,
    slp: &NfssvcSock,
    procp: &Proc,
    mrq: &mut Option<&'static Mbuf>,
) -> Result<(), Errno> {
    let cred: *const Ucred = &nfsd.nd_cr;
    let v3 = nfsd.nd_flag & ND_NFSV3 != 0;

    let Some(nfh) = nfsm_srvmtofh(nfsd, slp, mrq)? else {
        return Ok(());
    };
    let fh = nfh.fh_generic();
    let Some(dnfh) = nfsm_srvmtofh(nfsd, slp, mrq)? else {
        return Ok(());
    };
    let dfh = dnfh.fh_generic();
    let (len, error) = nfsm_srvnamesiz(nfsd)?;
    if error != 0 {
        // nfsm_reply would return zero if v3 and an error different from EBADRPC. But it
        // does not make sense to continue anyway if the error set in nfsm_srvnamesiz is
        // NFSERR_NAMETOL.
        nfsm_reply(nfsd, slp, mrq, error, 0)?;
        return Ok(());
    }

    let (vp, _rdonly) = match fhtovp(nfsd, &fh, false, slp) {
        Ok(x) => x,
        Err(error) => {
            if let Some(mut mb) = nfsm_reply(
                nfsd,
                slp,
                mrq,
                error,
                nfsx_postopattr(v3) + nfsx_wccdata(v3),
            )? {
                nfsm_srvpostop_attr(nfsd, None, &mut mb);
                nfsm_srvwcc(nfsd, None, None, &mut mb);
            }
            return Ok(());
        }
    };
    let mut dirp = None;
    let mut dirfor = Vattr::new();
    let mut dirfor_ok = false;
    let mut error = 0;
    if vp.v_type.get() != VDIR {
        let mut nd = ndinit(CREATE, LOCKPARENT, NiDirp::Sys(&[]), procp);
        nd.ni_cnd.cn_cred = cred;
        let r = namei(nfsd, &mut nd, &dfh, len, slp, &mut dirp, procp);
        if let Some(d) = dirp {
            if v3 {
                dirfor_ok = VOP_GETATTR(d, &mut dirfor, cred, procp).is_ok();
            } else {
                vrele(d);
                dirp = None;
            }
        }
        error = match r {
            Err(error) => error,
            Ok(()) => {
                let dvp = ni_dvp(&nd);
                let error = if nd.ni_vp.is_some() {
                    Errno::EEXIST.as_i32()
                } else if !same_mount(vp, dvp) {
                    Errno::EXDEV.as_i32()
                } else {
                    0
                };
                // out:
                if error == 0 {
                    status_of(&VOP_LINK(dvp, vp, &mut nd.ni_cnd))
                } else {
                    abortop(&mut nd);
                    dvp_release(&nd);
                    if let Some(xp) = nd.ni_vp {
                        vrele(xp);
                    }
                    error
                }
            }
        };
    }
    // out1:
    let mut at = Vattr::new();
    let getret = v3 && VOP_GETATTR(vp, &mut at, cred, procp).is_ok();
    let mut diraft = Vattr::new();
    let mut diraft_ok = false;
    if let Some(d) = dirp {
        diraft_ok = VOP_GETATTR(d, &mut diraft, cred, procp).is_ok();
        vrele(d);
    }
    vrele(vp);
    let Some(mut mb) = nfsm_reply(
        nfsd,
        slp,
        mrq,
        error,
        nfsx_postopattr(v3) + nfsx_wccdata(v3),
    )?
    else {
        return Ok(());
    };
    if v3 {
        nfsm_srvpostop_attr(nfsd, getret.then_some(&at), &mut mb);
        nfsm_srvwcc(
            nfsd,
            dirfor_ok.then_some(&dirfor),
            diraft_ok.then_some(&diraft),
            &mut mb,
        );
    }
    Ok(())
}

/// `nfsmout:` of `nfsrv_symlink`: releases what the lookup returned and passes the error of
/// the bad request on (the target buffer goes with its owner).
fn nfsrv_symlink_nfsmout(
    nd: &mut Nameidata<'_>,
    dirp: Option<&'static Vnode>,
    error: Errno,
) -> Result<(), Errno> {
    if nd.ni_cnd.cn_nameiop != LOOKUP {
        startdir_rele(nd);
        pnbuf_put(&mut nd.ni_cnd);
    }
    if let Some(d) = dirp {
        vrele(d);
    }
    abortop(nd);
    dvp_release(nd);
    if let Some(vp) = nd.ni_vp {
        vrele(vp);
    }
    Err(error)
}

/// `nfs symbolic link service`
pub fn nfsrv_symlink(
    nfsd: &mut NfsrvDescript,
    slp: &NfssvcSock,
    procp: &Proc,
    mrq: &mut Option<&'static Mbuf>,
) -> Result<(), Errno> {
    let cred: *const Ucred = &nfsd.nd_cr;
    let v3 = nfsd.nd_flag & ND_NFSV3 != 0;

    let Some(nfh) = nfsm_srvmtofh(nfsd, slp, mrq)? else {
        return Ok(());
    };
    let fh = nfh.fh_generic();
    let (len, error) = nfsm_srvnamesiz(nfsd)?;
    if error != 0 {
        // nfsm_reply would return zero if v3 and an error different from EBADRPC. But it
        // does not make sense to continue anyway if the error set in nfsm_srvnamesiz is
        // NFSERR_NAMETOL.
        nfsm_reply(nfsd, slp, mrq, error, 0)?;
        return Ok(());
    }

    let mut nd = ndinit(CREATE, LOCKPARENT | SAVESTART, NiDirp::Sys(&[]), procp);
    nd.ni_cnd.cn_cred = cred;
    let mut dirp = None;
    let r = namei(nfsd, &mut nd, &fh, len, slp, &mut dirp, procp);
    let mut dirfor = Vattr::new();
    let mut dirfor_ok = false;
    if let Some(d) = dirp {
        if v3 {
            dirfor_ok = VOP_GETATTR(d, &mut dirfor, cred, procp).is_ok();
        } else {
            vrele(d);
            dirp = None;
        }
    }
    let mut va = Vattr::new();
    let mut nfh = Nfsfh::new();
    let error = 'out: {
        if let Err(error) = r {
            break 'out error;
        }
        vattr_null(&mut va);
        if v3 && let Err(e) = nfsm_srvsattr(nfsd, &mut va) {
            return nfsrv_symlink_nfsmout(&mut nd, dirp, e);
        }
        let len2 = match nfsd_strsiz(nfsd, NFS_MAXPATHLEN) {
            Ok(l) => l,
            Err(e) => return nfsrv_symlink_nfsmout(&mut nd, dirp, e),
        };
        let mut pathcp = vec![0u8; len2 + 1];
        let mut iv = [Iovec {
            iov_base: pathcp.as_mut_ptr().cast::<c_void>(),
            iov_len: len2,
        }];
        let mut io = Uio {
            uio_iov: &mut iv,
            uio_offset: 0,
            uio_resid: len2,
            uio_segflg: UioSeg::UIO_SYSSPACE,
            uio_rw: UioRw::UIO_READ,
            uio_procp: None,
        };
        if let Err(e) = nfsd_mtouio(nfsd, &mut io, len2 as i32) {
            return nfsrv_symlink_nfsmout(&mut nd, dirp, e);
        }
        if !v3 {
            match nfsd_dissect(nfsd, NFSX_V2SATTR) {
                Ok(tl) => {
                    let sp: Nfsv2Sattr = tl.read(0);
                    va.va_mode = nfstov_mode(sp.sa_mode);
                }
                Err(e) => return nfsrv_symlink_nfsmout(&mut nd, dirp, e),
            }
        }
        pathcp[len2] = 0;
        if let Some(vp) = nd.ni_vp {
            startdir_rele(&mut nd);
            pnbuf_put(&mut nd.ni_cnd);
            abortop(&mut nd);
            dvp_release(&nd);
            vrele(vp);
            break 'out Errno::EEXIST.as_i32();
        }
        let dvp = ni_dvp(&nd);
        let r = VOP_SYMLINK(dvp, &mut nd.ni_vp, &mut nd.ni_cnd, &mut va, &pathcp[..len2]);
        let mut error = status_of(&r);
        if error != 0 {
            startdir_rele(&mut nd);
        } else {
            if v3 {
                nfsrv_relookup_pathlen(&mut nd);
                nd.ni_cnd.cn_nameiop = LOOKUP;
                nd.ni_cnd.cn_flags &= !(LOCKPARENT | SAVESTART | FOLLOW);
                nd.ni_cnd.cn_flags |= NOFOLLOW | LOCKLEAF;
                nd.ni_cnd.cn_proc = procp;
                nd.ni_cnd.cn_cred = cred;
                error = status_of(&vfs_lookup(&mut nd));
                if error == 0 {
                    let vp = ni_vp(&nd);
                    error = match nfsrv_vptofh(vp) {
                        Ok(f) => {
                            nfh = f;
                            status_of(&VOP_GETATTR(vp, &mut va, cred, procp))
                        }
                        Err(e) => e.as_i32(),
                    };
                    vput(vp);
                }
            } else {
                startdir_rele(&mut nd);
            }
            pnbuf_put(&mut nd.ni_cnd);
        }
        error
    };
    // out:
    let mut diraft = Vattr::new();
    let mut diraft_ok = false;
    if let Some(d) = dirp {
        diraft_ok = VOP_GETATTR(d, &mut diraft, cred, procp).is_ok();
        vrele(d);
    }
    let Some(mut mb) = nfsm_reply(
        nfsd,
        slp,
        mrq,
        error,
        nfsx_srvfh(v3) + nfsx_postopattr(v3) + nfsx_wccdata(v3),
    )?
    else {
        return Ok(());
    };
    if v3 {
        if error == 0 {
            nfsm_srvpostop_fh(&mut mb, &nfh);
            nfsm_srvpostop_attr(nfsd, Some(&va), &mut mb);
        }
        nfsm_srvwcc(
            nfsd,
            dirfor_ok.then_some(&dirfor),
            diraft_ok.then_some(&diraft),
            &mut mb,
        );
    }
    Ok(())
}

/// `nfsmout:` of `nfsrv_mkdir`: releases what the lookup returned and passes the error of
/// the bad request on.
fn nfsrv_mkdir_nfsmout(
    nd: &mut Nameidata<'_>,
    dirp: Option<&'static Vnode>,
    error: Errno,
) -> Result<(), Errno> {
    if let Some(d) = dirp {
        vrele(d);
    }
    abortop(nd);
    dvp_release(nd);
    if let Some(vp) = nd.ni_vp {
        vrele(vp);
    }
    Err(error)
}

/// `nfs mkdir service`
pub fn nfsrv_mkdir(
    nfsd: &mut NfsrvDescript,
    slp: &NfssvcSock,
    procp: &Proc,
    mrq: &mut Option<&'static Mbuf>,
) -> Result<(), Errno> {
    let cred: *const Ucred = &nfsd.nd_cr;
    let v3 = nfsd.nd_flag & ND_NFSV3 != 0;

    let Some(nfh) = nfsm_srvmtofh(nfsd, slp, mrq)? else {
        return Ok(());
    };
    let fh = nfh.fh_generic();
    let (len, error) = nfsm_srvnamesiz(nfsd)?;
    if error != 0 {
        // nfsm_reply would return zero if v3 and an error different from EBADRPC. But it
        // does not make sense to continue anyway if the error set in nfsm_srvnamesiz is
        // NFSERR_NAMETOL.
        nfsm_reply(nfsd, slp, mrq, error, 0)?;
        return Ok(());
    }

    let mut nd = ndinit(CREATE, LOCKPARENT, NiDirp::Sys(&[]), procp);
    nd.ni_cnd.cn_cred = cred;
    let mut dirp = None;
    let r = namei(nfsd, &mut nd, &fh, len, slp, &mut dirp, procp);
    let mut dirfor = Vattr::new();
    let mut dirfor_ok = false;
    if let Some(d) = dirp {
        if v3 {
            dirfor_ok = VOP_GETATTR(d, &mut dirfor, cred, procp).is_ok();
        } else {
            vrele(d);
            dirp = None;
        }
    }
    if let Err(error) = r {
        let reply = nfsm_reply(nfsd, slp, mrq, error, nfsx_wccdata(v3));
        if let Some(d) = dirp {
            vrele(d);
        }
        if let Some(mut mb) = reply? {
            nfsm_srvwcc(nfsd, dirfor_ok.then_some(&dirfor), None, &mut mb);
        }
        return Ok(());
    }

    let mut va = Vattr::new();
    vattr_null(&mut va);
    if v3 {
        if let Err(e) = nfsm_srvsattr(nfsd, &mut va) {
            return nfsrv_mkdir_nfsmout(&mut nd, dirp, e);
        }
    } else {
        match nfsd_dissect(nfsd, NFSX_UNSIGNED) {
            Ok(tl) => va.va_mode = nfstov_mode(tl.get(0)),
            Err(e) => return nfsrv_mkdir_nfsmout(&mut nd, dirp, e),
        }
    }
    va.va_type = VDIR;
    let mut nfh = Nfsfh::new();
    let error = if let Some(vp) = nd.ni_vp {
        abortop(&mut nd);
        dvp_release(&nd);
        vrele(vp);
        Errno::EEXIST.as_i32()
    } else {
        match VOP_MKDIR(ni_dvp(&nd), &mut nd.ni_vp, &mut nd.ni_cnd, &mut va) {
            Ok(()) => {
                let vp = ni_vp(&nd);
                let error = match nfsrv_vptofh(vp) {
                    Ok(f) => {
                        nfh = f;
                        status_of(&VOP_GETATTR(vp, &mut va, cred, procp))
                    }
                    Err(e) => e.as_i32(),
                };
                vput(vp);
                error
            }
            Err(e) => e.as_i32(),
        }
    };
    // out:
    let mut diraft = Vattr::new();
    let mut diraft_ok = false;
    if let Some(d) = dirp {
        diraft_ok = VOP_GETATTR(d, &mut diraft, cred, procp).is_ok();
        vrele(d);
    }
    let Some(mut mb) = nfsm_reply(
        nfsd,
        slp,
        mrq,
        error,
        nfsx_srvfh(v3) + nfsx_postopattr(v3) + nfsx_wccdata(v3),
    )?
    else {
        return Ok(());
    };
    if v3 {
        if error == 0 {
            nfsm_srvpostop_fh(&mut mb, &nfh);
            nfsm_srvpostop_attr(nfsd, Some(&va), &mut mb);
        }
        nfsm_srvwcc(
            nfsd,
            dirfor_ok.then_some(&dirfor),
            diraft_ok.then_some(&diraft),
            &mut mb,
        );
    } else {
        nfsm_srvfhtom(&mut mb, &nfh, v3);
        let fp = nfsm_srvfattr(nfsd, &va);
        nfsm_build(&mut mb, NFSX_V2FATTR).write(0, &fp);
    }
    Ok(())
}

/// `nfs rmdir service`
pub fn nfsrv_rmdir(
    nfsd: &mut NfsrvDescript,
    slp: &NfssvcSock,
    procp: &Proc,
    mrq: &mut Option<&'static Mbuf>,
) -> Result<(), Errno> {
    let cred: *const Ucred = &nfsd.nd_cr;
    let v3 = nfsd.nd_flag & ND_NFSV3 != 0;

    let Some(nfh) = nfsm_srvmtofh(nfsd, slp, mrq)? else {
        return Ok(());
    };
    let fh = nfh.fh_generic();
    let (len, error) = nfsm_srvnamesiz(nfsd)?;
    if error != 0 {
        // nfsm_reply would return zero if v3 and an error different from EBADRPC. But it
        // does not make sense to continue anyway if the error set in nfsm_srvnamesiz is
        // NFSERR_NAMETOL.
        nfsm_reply(nfsd, slp, mrq, error, 0)?;
        return Ok(());
    }

    let mut nd = ndinit(DELETE, LOCKPARENT | LOCKLEAF, NiDirp::Sys(&[]), procp);
    nd.ni_cnd.cn_cred = cred;
    let mut dirp = None;
    let r = namei(nfsd, &mut nd, &fh, len, slp, &mut dirp, procp);
    let mut dirfor = Vattr::new();
    let mut dirfor_ok = false;
    if let Some(d) = dirp {
        if v3 {
            dirfor_ok = VOP_GETATTR(d, &mut dirfor, cred, procp).is_ok();
        } else {
            vrele(d);
            dirp = None;
        }
    }
    if let Err(error) = r {
        let reply = nfsm_reply(nfsd, slp, mrq, error, nfsx_wccdata(v3));
        if let Some(d) = dirp {
            vrele(d);
        }
        if let Some(mut mb) = reply? {
            nfsm_srvwcc(nfsd, dirfor_ok.then_some(&dirfor), None, &mut mb);
        }
        return Ok(());
    }
    let vp = ni_vp(&nd);
    let dvp = ni_dvp(&nd);
    let error = 'out: {
        if vp.v_type.get() != VDIR {
            break 'out Errno::ENOTDIR.as_i32();
        }
        // No rmdir "." please.
        if ptr::eq(dvp, vp) {
            break 'out Errno::EINVAL.as_i32();
        }
        // A mounted on directory cannot be deleted.
        if vp.v_mountedhere().is_some() {
            break 'out Errno::EBUSY.as_i32();
        }
        // The root of a mounted filesystem cannot be deleted.
        if vp.v_flag.get() & VROOT != 0 {
            break 'out Errno::EBUSY.as_i32();
        }
        0
    };
    // out:
    let error = if error == 0 {
        status_of(&VOP_RMDIR(dvp, vp, &mut nd.ni_cnd))
    } else {
        abortop(&mut nd);
        dvp_release(&nd);
        vput(vp);
        error
    };
    let mut diraft = Vattr::new();
    let mut diraft_ok = false;
    if let Some(d) = dirp {
        diraft_ok = VOP_GETATTR(d, &mut diraft, cred, procp).is_ok();
        vrele(d);
    }
    let Some(mut mb) = nfsm_reply(nfsd, slp, mrq, error, nfsx_wccdata(v3))? else {
        return Ok(());
    };
    if v3 {
        nfsm_srvwcc(
            nfsd,
            dirfor_ok.then_some(&dirfor),
            diraft_ok.then_some(&diraft),
            &mut mb,
        );
    }
    Ok(())
}

/// The directory entry at the start of `buf` and its name, or `None` when the buffer ends
/// before the entry does (or its `d_reclen` is zero, which would never move on).
fn nfsrv_dirent(buf: &[u8]) -> Option<(Dirent, &[u8])> {
    let dp = Dirent::from_bytes(buf)?;
    let reclen = usize::from(dp.d_reclen);
    let name_end = Dirent::NAME_OFFSET + usize::from(dp.d_namlen);
    if reclen == 0 || reclen > buf.len() || name_end > buf.len() {
        return None;
    }
    Some((dp, &buf[Dirent::NAME_OFFSET..name_end]))
}

/// The offset in `buf` (what `VOP_READDIR` read) of its first entry with a file number:
/// the C's walk past the entries with `d_fileno == 0` (`buf.len()` when there is none).
fn nfsrv_dirent_skip(buf: &[u8]) -> usize {
    let mut cpos = 0;
    while cpos < buf.len() {
        match nfsrv_dirent(&buf[cpos..]) {
            Some((dp, _)) if dp.d_fileno == 0 => cpos += usize::from(dp.d_reclen),
            Some(_) => return cpos,
            None => return buf.len(),
        }
    }
    cpos
}

/// The loop of `nfsrv_readdir` that builds the reply: packs the entries of `buf` (the
/// `struct dirent`s from `cpos` to `cend`) as READDIR `entry`s at `mb`, skipping the ones
/// without a file number, while the reply counted in `len` (`3 * NFSX_UNSIGNED` of
/// paranoia to start) stays within the client's `cnt` bytes. Returns `false` when an entry
/// did not fit (the C's `eofflag = 0; break`).
fn nfsrv_readdir_pack(
    mb: &mut &'static Mbuf,
    buf: &[u8],
    v3: bool,
    cnt: i32,
    mut len: i32,
) -> bool {
    let mut cpos = 0;
    while cpos < buf.len() {
        let Some((dp, name)) = nfsrv_dirent(&buf[cpos..]) else {
            break;
        };
        if dp.d_fileno != 0 {
            let nlen = name.len();
            let pad = nfsm_padlen(nlen);
            len += (4 * NFSX_UNSIGNED + nlen + pad) as i32;
            if v3 {
                len += (2 * NFSX_UNSIGNED) as i32;
            }
            if len > cnt {
                return false;
            }
            // Build the directory record xdr from the dirent entry.
            let mut tl = nfsm_build(mb, (if v3 { 3 } else { 2 }) * NFSX_UNSIGNED);
            tl.set(0, nfs_true);
            if v3 {
                tl.txdr_hyper(1, dp.d_fileno);
            } else {
                tl.set(1, txdr_unsigned(dp.d_fileno as u32));
            }

            // And copy the name
            nfsm_strtombuf(mb, name);

            // Finish off the record
            if v3 {
                nfsm_build(mb, 2 * NFSX_UNSIGNED).txdr_hyper(0, dp.d_off as u64);
            } else {
                nfsm_build(mb, NFSX_UNSIGNED).set(0, txdr_unsigned(dp.d_off as u32));
            }
        }
        cpos += usize::from(dp.d_reclen);
    }
    true
}

/// `if (cnt > xfer || cnt < 0) cnt = xfer;`: the client's byte count limited to
/// `NFS_SRVMAXDATA` (`xfer`).
fn nfsrv_readdir_cnt(cnt: i32, xfer: i32) -> i32 {
    if cnt > xfer || cnt < 0 { xfer } else { cnt }
}

/// The size of the buffer for `VOP_READDIR`: `siz` rounded up to a multiple of `DIRBLKSIZ`,
/// `xfer` when that is larger than `xfer` or not positive.
fn nfsrv_readdir_siz(siz: i32, xfer: i32) -> usize {
    let blk = DIRBLKSIZ as i32;
    let siz = siz.wrapping_add(blk - 1) & !(blk - 1);
    (if siz > xfer || siz <= 0 { xfer } else { siz }) as usize
}

/// nfs readdir service
/// - mallocs what it thinks is enough to read count rounded up to a multiple of
///   `NFS_DIRBLKSIZ <= NFS_MAXREADDIR`
/// - calls `VOP_READDIR()`
/// - loops around building the reply; if the output generated exceeds count break out of
///   loop
/// - it only knows that it has encountered eof when the `VOP_READDIR()` reads nothing
/// - as such one readdir rpc will return eof false although you are there and then the
///   next will return eof
/// - it trims out records with `d_fileno == 0`; this doesn't matter for Unix clients, but
///   they might confuse clients for other os'.
///
/// NB: It is tempting to set eof to true if the `VOP_READDIR()` reads less than requested,
/// but this may not apply to all filesystems. For example, client NFS does not { although
/// it is never remote mounted anyhow }. The alternate call `nfsrv_readdirplus()` does
/// lookups as well.
///
/// PS: The NFS protocol spec. does not clarify what the "count" byte argument is a count
/// of.. just name strings and file id's or the entire reply rpc or ... I tried just file
/// name and id sizes and it confused the Sun client, so I am using the full rpc size now.
/// The "paranoia.." comment refers to including the status longwords that are not a part
/// of the dir. "entry" structures, but are in the rpc.
pub fn nfsrv_readdir(
    nfsd: &mut NfsrvDescript,
    slp: &NfssvcSock,
    procp: &Proc,
    mrq: &mut Option<&'static Mbuf>,
) -> Result<(), Errno> {
    let cred: *const Ucred = &nfsd.nd_cr;
    let v3 = nfsd.nd_flag & ND_NFSV3 != 0;

    let Some(nfh) = nfsm_srvmtofh(nfsd, slp, mrq)? else {
        return Ok(());
    };
    let fh = nfh.fh_generic();
    // The cookie verifier (`verf`) is not checked.
    let (toff, cnt) = if v3 {
        let tl = nfsd_dissect(nfsd, 5 * NFSX_UNSIGNED)?;
        (tl.fxdr_hyper(0), fxdr_unsigned(tl.get(4)) as i32)
    } else {
        let tl = nfsd_dissect(nfsd, 2 * NFSX_UNSIGNED)?;
        (
            u64::from(fxdr_unsigned(tl.get(0))),
            fxdr_unsigned(tl.get(1)) as i32,
        )
    };
    let mut off = toff;
    let mut at = Vattr::new();
    let r = if cnt == 0 {
        Err(if v3 {
            NFSERR_TOOSMALL
        } else {
            Errno::EBADRPC.as_i32()
        })
    } else {
        let xfer = nfs_srvmaxdata(nfsd) as i32;
        let cnt = nfsrv_readdir_cnt(cnt, xfer);
        let siz = nfsrv_readdir_siz(cnt, xfer);
        fhtovp(nfsd, &fh, true, slp).and_then(|(vp, rdonly)| {
            if vp.v_type.get() != VDIR {
                vput(vp);
                Err(Errno::ENOTDIR.as_i32())
            } else {
                Ok((vp, rdonly, cnt, siz))
            }
        })
    };
    let (vp, rdonly, cnt, fullsiz) = match r {
        Ok(x) => x,
        Err(error) => {
            if let Some(mut mb) = nfsm_reply(nfsd, slp, mrq, error, NFSX_UNSIGNED)? {
                nfsm_srvpostop_attr(nfsd, None, &mut mb);
            }
            return Ok(());
        }
    };
    let mut getret = false;
    let mut error = 0;
    if v3 {
        let g = VOP_GETATTR(vp, &mut at, cred, procp);
        getret = g.is_ok();
        error = status_of(&g);
    }
    if error == 0 {
        error = status_of(&nfsrv_access(vp, VEXEC, &nfsd.nd_cr, rdonly, procp, false));
    }
    if error != 0 {
        vput(vp);
        if let Some(mut mb) = nfsm_reply(nfsd, slp, mrq, error, nfsx_postopattr(v3))? {
            nfsm_srvpostop_attr(nfsd, getret.then_some(&at), &mut mb);
        }
        return Ok(());
    }
    let _ = VOP_UNLOCK(vp);
    let mut rbuf = vec![0u8; fullsiz];
    // again:
    let (cpos, siz, eofflag) = loop {
        let mut iv = [Iovec {
            iov_base: rbuf.as_mut_ptr().cast::<c_void>(),
            iov_len: fullsiz,
        }];
        let mut io = Uio {
            uio_iov: &mut iv,
            uio_offset: off as Off,
            uio_resid: fullsiz,
            uio_segflg: UioSeg::UIO_SYSSPACE,
            uio_rw: UioRw::UIO_READ,
            uio_procp: None,
        };
        let mut eofflag = 0;

        let _ = vn_lock(vp, LK_EXCLUSIVE | LK_RETRY);
        let r = VOP_READDIR(vp, &mut io, cred, &mut eofflag);

        off = io.uio_offset as u64;
        let resid = io.uio_resid;
        let mut error = status_of(&r);
        if v3 {
            let g = VOP_GETATTR(vp, &mut at, cred, procp);
            getret = g.is_ok();
            if error == 0 {
                error = status_of(&g);
            }
        }

        let _ = VOP_UNLOCK(vp);
        if error != 0 {
            vrele(vp);
            drop(rbuf);
            if let Some(mut mb) = nfsm_reply(nfsd, slp, mrq, error, nfsx_postopattr(v3))? {
                nfsm_srvpostop_attr(nfsd, getret.then_some(&at), &mut mb);
            }
            return Ok(());
        }
        let siz = fullsiz - resid;
        // If nothing read, return eof rpc reply
        if siz == 0 {
            vrele(vp);
            let Some(mut mb) = nfsm_reply(
                nfsd,
                slp,
                mrq,
                0,
                nfsx_postopattr(v3) + nfsx_cookieverf(v3) + 2 * NFSX_UNSIGNED,
            )?
            else {
                return Ok(());
            };
            if v3 {
                nfsm_srvpostop_attr(nfsd, getret.then_some(&at), &mut mb);
                let mut tl = nfsm_build(&mut mb, 4 * NFSX_UNSIGNED);
                tl.txdr_hyper(0, at.va_filerev);
                tl.set(2, nfs_false);
                tl.set(3, nfs_true);
            } else {
                let mut tl = nfsm_build(&mut mb, 2 * NFSX_UNSIGNED);
                tl.set(0, nfs_false);
                tl.set(1, nfs_true);
            }
            return Ok(());
        }

        // Check for degenerate cases of nothing useful read. If so go try again
        let cpos = nfsrv_dirent_skip(&rbuf[..siz]);
        if cpos >= siz {
            continue;
        }
        break (cpos, siz, eofflag);
    };

    let len = (3 * NFSX_UNSIGNED) as i32; // paranoia, probably can be 0
    let mut mb = match nfsm_reply(
        nfsd,
        slp,
        mrq,
        0,
        nfsx_postopattr(v3) + nfsx_cookieverf(v3) + siz,
    ) {
        Ok(Some(mb)) => mb,
        Ok(None) => {
            vrele(vp);
            return Ok(());
        }
        Err(e) => {
            vrele(vp);
            return Err(e);
        }
    };
    if v3 {
        nfsm_srvpostop_attr(nfsd, getret.then_some(&at), &mut mb);
        nfsm_build(&mut mb, 2 * NFSX_UNSIGNED).txdr_hyper(0, at.va_filerev);
    }

    // Loop through the records and build reply
    let all = nfsrv_readdir_pack(&mut mb, &rbuf[cpos..siz], v3, cnt, len);
    let eofflag = if all { eofflag } else { 0 };
    vrele(vp);
    let mut tl = nfsm_build(&mut mb, 2 * NFSX_UNSIGNED);
    tl.set(0, nfs_false);
    tl.set(1, if eofflag != 0 { nfs_true } else { nfs_false });
    Ok(())
}

/// `struct flrep`'s size: `fl_off` (2 words), `fl_postopok`, `fl_fattr`, `fl_fhok`,
/// `fl_fhsize`, `fl_nfh`.
const FLREP_SIZE: usize =
    2 * NFSX_UNSIGNED + NFSX_UNSIGNED + NFSX_V3FATTR + 2 * NFSX_UNSIGNED + NFSX_V3FH;

/// `struct flrep`: the part of a READDIRPLUS `entryplus3` after the name (the cookie, the
/// `post_op_attr` and the `post_op_fh3`), as the bytes `nfsm_buftombuf` copies out.
fn nfsrv_flrep(off: u64, fattr: &NfsFattr, nfh: &Nfsfh) -> [u8; FLREP_SIZE] {
    let mut fl = [0u8; FLREP_SIZE];
    let off = txdr_hyper(off);
    fl[0..4].copy_from_slice(&off[0].to_ne_bytes());
    fl[4..8].copy_from_slice(&off[1].to_ne_bytes());
    fl[8..12].copy_from_slice(&nfs_true.to_ne_bytes());
    let fa = 12;
    fl[fa..fa + NFSX_V3FATTR].copy_from_slice(&xdr_bytes(fattr)[..NFSX_V3FATTR]);
    let fhok = fa + NFSX_V3FATTR;
    fl[fhok..fhok + 4].copy_from_slice(&nfs_true.to_ne_bytes());
    fl[fhok + 4..fhok + 8].copy_from_slice(&txdr_unsigned(NFSX_V3FH as u32).to_ne_bytes());
    fl[fhok + 8..].copy_from_slice(&nfh.fh_bytes[..NFSX_V3FH]);
    fl
}

/// `nfs readdirplus service`: `nfsrv_readdir` with the attributes and file handle of each
/// entry (version 3 only).
pub fn nfsrv_readdirplus(
    nfsd: &mut NfsrvDescript,
    slp: &NfssvcSock,
    procp: &Proc,
    mrq: &mut Option<&'static Mbuf>,
) -> Result<(), Errno> {
    let cred: *const Ucred = &nfsd.nd_cr;

    let Some(nfh) = nfsm_srvmtofh(nfsd, slp, mrq)? else {
        return Ok(());
    };
    let fh = nfh.fh_generic();
    // The cookie verifier (`verf`) is not checked.
    let (toff, siz, cnt) = {
        let tl = nfsd_dissect(nfsd, 6 * NFSX_UNSIGNED)?;
        (
            tl.fxdr_hyper(0),
            fxdr_unsigned(tl.get(4)) as i32,
            fxdr_unsigned(tl.get(5)) as i32,
        )
    };
    let mut off = toff;
    let mut at = Vattr::new();
    let r = if siz == 0 || cnt == 0 {
        Err(NFSERR_TOOSMALL)
    } else {
        let xfer = nfs_srvmaxdata(nfsd) as i32;
        let cnt = nfsrv_readdir_cnt(cnt, xfer);
        let siz = nfsrv_readdir_siz(siz, xfer);
        fhtovp(nfsd, &fh, true, slp).and_then(|(vp, rdonly)| {
            if vp.v_type.get() != VDIR {
                vput(vp);
                Err(Errno::ENOTDIR.as_i32())
            } else {
                Ok((vp, rdonly, cnt, siz))
            }
        })
    };
    let (vp, rdonly, cnt, fullsiz) = match r {
        Ok(x) => x,
        Err(error) => {
            if let Some(mut mb) = nfsm_reply(nfsd, slp, mrq, error, NFSX_UNSIGNED)? {
                nfsm_srvpostop_attr(nfsd, None, &mut mb);
            }
            return Ok(());
        }
    };
    let g = VOP_GETATTR(vp, &mut at, cred, procp);
    let mut getret = g.is_ok();
    let mut error = status_of(&g);
    if error == 0 {
        error = status_of(&nfsrv_access(vp, VEXEC, &nfsd.nd_cr, rdonly, procp, false));
    }
    if error != 0 {
        vput(vp);
        if let Some(mut mb) = nfsm_reply(nfsd, slp, mrq, error, NFSX_V3POSTOPATTR)? {
            nfsm_srvpostop_attr(nfsd, getret.then_some(&at), &mut mb);
        }
        return Ok(());
    }
    let _ = VOP_UNLOCK(vp);

    let mut rbuf = vec![0u8; fullsiz];
    // again:
    let (cpos, siz, eofflag) = loop {
        let mut iv = [Iovec {
            iov_base: rbuf.as_mut_ptr().cast::<c_void>(),
            iov_len: fullsiz,
        }];
        let mut io = Uio {
            uio_iov: &mut iv,
            uio_offset: off as Off,
            uio_resid: fullsiz,
            uio_segflg: UioSeg::UIO_SYSSPACE,
            uio_rw: UioRw::UIO_READ,
            uio_procp: None,
        };
        let mut eofflag = 0;

        let _ = vn_lock(vp, LK_EXCLUSIVE | LK_RETRY);
        let r = VOP_READDIR(vp, &mut io, cred, &mut eofflag);

        off = io.uio_offset as u64;
        let resid = io.uio_resid;
        let g = VOP_GETATTR(vp, &mut at, cred, procp);
        getret = g.is_ok();

        let _ = VOP_UNLOCK(vp);

        let error = if r.is_ok() {
            status_of(&g)
        } else {
            status_of(&r)
        };
        if error != 0 {
            vrele(vp);
            drop(rbuf);
            if let Some(mut mb) = nfsm_reply(nfsd, slp, mrq, error, NFSX_V3POSTOPATTR)? {
                nfsm_srvpostop_attr(nfsd, getret.then_some(&at), &mut mb);
            }
            return Ok(());
        }
        let siz = fullsiz - resid;
        // If nothing read, return eof rpc reply
        if siz == 0 {
            vrele(vp);
            let Some(mut mb) = nfsm_reply(
                nfsd,
                slp,
                mrq,
                0,
                NFSX_V3POSTOPATTR + NFSX_V3COOKIEVERF + 2 * NFSX_UNSIGNED,
            )?
            else {
                return Ok(());
            };
            nfsm_srvpostop_attr(nfsd, getret.then_some(&at), &mut mb);
            let mut tl = nfsm_build(&mut mb, 4 * NFSX_UNSIGNED);
            tl.txdr_hyper(0, at.va_filerev);
            tl.set(2, nfs_false);
            tl.set(3, nfs_true);
            return Ok(());
        }

        // Check for degenerate cases of nothing useful read. If so go try again
        let cpos = nfsrv_dirent_skip(&rbuf[..siz]);
        if cpos >= siz {
            continue;
        }
        break (cpos, siz, eofflag);
    };

    // struct READDIRPLUS3resok {
    //     postop_attr dir_attributes;
    //     cookieverf3 cookieverf;
    //     dirlistplus3 reply;
    // }
    //
    // struct dirlistplus3 {
    //     entryplus3  *entries;
    //     bool eof;
    //  }
    let mut len = NFSX_V3POSTOPATTR + NFSX_V3COOKIEVERF + 2 * NFSX_UNSIGNED;
    let mut dirlen = len;
    let mut mb = match nfsm_reply(nfsd, slp, mrq, 0, cnt as usize) {
        Ok(Some(mb)) => mb,
        Ok(None) => {
            vrele(vp);
            return Ok(());
        }
        Err(e) => {
            vrele(vp);
            return Err(e);
        }
    };
    nfsm_srvpostop_attr(nfsd, getret.then_some(&at), &mut mb);
    nfsm_build(&mut mb, 2 * NFSX_UNSIGNED).txdr_hyper(0, at.va_filerev);

    // Loop through the records and build reply
    let mut eofflag = eofflag;
    let buf = &rbuf[cpos..siz];
    let mut cpos = 0;
    let mut va = Vattr::new();
    'entries: while cpos < buf.len() {
        let Some((dp, name)) = nfsrv_dirent(&buf[cpos..]) else {
            break;
        };
        'invalid: {
            if dp.d_fileno == 0 {
                break 'invalid;
            }
            let nlen = name.len();
            let pad = nfsm_padlen(nlen);

            // For readdir_and_lookup get the vnode using the file number.
            let Ok(nvp) = VFS_VGET(v_mount(vp), dp.d_fileno) else {
                break 'invalid;
            };
            let Ok(nfhp) = nfsrv_vptofh(nvp) else {
                vput(nvp);
                break 'invalid;
            };
            if VOP_GETATTR(nvp, &mut va, cred, procp).is_err() {
                vput(nvp);
                break 'invalid;
            }
            vput(nvp);

            // If either the dircount or maxcount will be exceeded, get out now. Both of
            // these lengths are calculated conservatively, including all XDR overheads.
            //
            // Each entry:
            // 2 * NFSX_UNSIGNED for fileid3
            // 1 * NFSX_UNSIGNED for length of name
            // nlen + pad == space the name takes up
            // 2 * NFSX_UNSIGNED for the cookie
            // 1 * NFSX_UNSIGNED to indicate if file handle present
            // 1 * NFSX_UNSIGNED for the file handle length
            // NFSX_V3FH == space our file handle takes up
            // NFSX_V3POSTOPATTR == space the attributes take up
            // 1 * NFSX_UNSIGNED for next pointer
            len += 8 * NFSX_UNSIGNED + nlen + pad + NFSX_V3FH + NFSX_V3POSTOPATTR;
            dirlen += 6 * NFSX_UNSIGNED + nlen + pad;
            if len > cnt as usize || dirlen > fullsiz {
                eofflag = 0;
                break 'entries;
            }

            let mut tl = nfsm_build(&mut mb, 3 * NFSX_UNSIGNED);
            tl.set(0, nfs_true);
            tl.txdr_hyper(1, dp.d_fileno);

            // And copy the name
            nfsm_strtombuf(&mut mb, name);

            // Build the directory record xdr from the dirent entry.
            let fp = nfsm_srvfattr(nfsd, &va);
            let fl = nfsrv_flrep(dp.d_off as u64, &fp, &nfhp);

            // Now copy the flrep structure out.
            nfsm_buftombuf(&mut mb, &fl);
        }
        // invalid:
        cpos += usize::from(dp.d_reclen);
    }
    vrele(vp);
    let mut tl = nfsm_build(&mut mb, 2 * NFSX_UNSIGNED);
    tl.set(0, nfs_false);
    tl.set(1, if eofflag != 0 { nfs_true } else { nfs_false });
    Ok(())
}

/// `nfs commit service`
pub fn nfsrv_commit(
    nfsd: &mut NfsrvDescript,
    slp: &NfssvcSock,
    procp: &Proc,
    mrq: &mut Option<&'static Mbuf>,
) -> Result<(), Errno> {
    let cred: *const Ucred = &nfsd.nd_cr;

    let Some(nfh) = nfsm_srvmtofh(nfsd, slp, mrq)? else {
        return Ok(());
    };
    let fh = nfh.fh_generic();
    // XXX At this time VOP_FSYNC() does not accept offset and byte count parameters, so
    // these arguments are useless (someday maybe).
    let (_off, cnt) = {
        let tl = nfsd_dissect(nfsd, 3 * NFSX_UNSIGNED)?;
        (tl.fxdr_hyper(0), fxdr_unsigned(tl.get(2)) as i32)
    };
    let _cnt = cnt.max(0);
    let (vp, _rdonly) = match fhtovp(nfsd, &fh, true, slp) {
        Ok(x) => x,
        Err(error) => {
            if let Some(mut mb) = nfsm_reply(nfsd, slp, mrq, error, 2 * NFSX_UNSIGNED)? {
                nfsm_srvwcc(nfsd, None, None, &mut mb);
            }
            return Ok(());
        }
    };
    let mut bfor = Vattr::new();
    let for_ret = VOP_GETATTR(vp, &mut bfor, cred, procp).is_ok();
    let error = status_of(&VOP_FSYNC(vp, cred, MNT_WAIT, procp));
    let mut aft = Vattr::new();
    let aft_ret = VOP_GETATTR(vp, &mut aft, cred, procp).is_ok();
    vput(vp);
    let Some(mut mb) = nfsm_reply(nfsd, slp, mrq, error, NFSX_V3WCCDATA + NFSX_V3WRITEVERF)? else {
        return Ok(());
    };
    nfsm_srvwcc(
        nfsd,
        for_ret.then_some(&bfor),
        aft_ret.then_some(&aft),
        &mut mb,
    );
    if error == 0 {
        let mut tl = nfsm_build(&mut mb, NFSX_V3WRITEVERF);
        let boottime = microboottime();
        tl.set(0, txdr_unsigned(boottime.tv_sec as u32));
        tl.set(1, txdr_unsigned(boottime.tv_usec as u32));
    }
    Ok(())
}

/// `nfs statfs service`
pub fn nfsrv_statfs(
    nfsd: &mut NfsrvDescript,
    slp: &NfssvcSock,
    procp: &Proc,
    mrq: &mut Option<&'static Mbuf>,
) -> Result<(), Errno> {
    let cred: *const Ucred = &nfsd.nd_cr;
    let v3 = nfsd.nd_flag & ND_NFSV3 != 0;

    let Some(nfh) = nfsm_srvmtofh(nfsd, slp, mrq)? else {
        return Ok(());
    };
    let fh = nfh.fh_generic();
    let (vp, _rdonly) = match fhtovp(nfsd, &fh, true, slp) {
        Ok(x) => x,
        Err(error) => {
            if let Some(mut mb) = nfsm_reply(nfsd, slp, mrq, error, NFSX_UNSIGNED)? {
                nfsm_srvpostop_attr(nfsd, None, &mut mb);
            }
            return Ok(());
        }
    };
    let mut sf = Statfs::new();
    let error = status_of(&VFS_STATFS(v_mount(vp), &mut sf, procp));
    let mut at = Vattr::new();
    let getret = VOP_GETATTR(vp, &mut at, cred, procp).is_ok();
    vput(vp);
    let Some(mut mb) = nfsm_reply(nfsd, slp, mrq, error, nfsx_postopattr(v3) + nfsx_statfs(v3))?
    else {
        return Ok(());
    };
    if v3 {
        nfsm_srvpostop_attr(nfsd, getret.then_some(&at), &mut mb);
    }
    if error != 0 {
        return Ok(());
    }
    let mut sfp = NfsStatfs::default();
    if v3 {
        let bsize = u64::from(sf.f_bsize);
        let hyper = |v: u64| Nfsuint64::from_words(txdr_hyper(v));
        sfp.set_sf_tbytes(hyper(sf.f_blocks.wrapping_mul(bsize)));
        sfp.set_sf_fbytes(hyper(sf.f_bfree.wrapping_mul(bsize)));
        sfp.set_sf_abytes(hyper((sf.f_bavail as u64).wrapping_mul(bsize)));
        sfp.set_sf_tfiles(hyper(sf.f_files));
        let tval = sf.f_ffree;
        sfp.set_sf_ffiles(hyper(tval));
        sfp.set_sf_afiles(hyper(tval));
        sfp.set_sf_invarsec(0);
    } else {
        sfp.set_sf_tsize(txdr_unsigned(NFS_MAXDGRAMDATA as u32));
        sfp.set_sf_bsize(txdr_unsigned(sf.f_bsize));
        sfp.set_sf_blocks(txdr_unsigned(sf.f_blocks as u32));
        sfp.set_sf_bfree(txdr_unsigned(sf.f_bfree as u32));
        sfp.set_sf_bavail(txdr_unsigned(sf.f_bavail as u32));
    }
    nfsm_build(&mut mb, nfsx_statfs(v3)).write(0, &sfp);
    Ok(())
}

/// `nfs fsinfo service`
pub fn nfsrv_fsinfo(
    nfsd: &mut NfsrvDescript,
    slp: &NfssvcSock,
    procp: &Proc,
    mrq: &mut Option<&'static Mbuf>,
) -> Result<(), Errno> {
    let cred: *const Ucred = &nfsd.nd_cr;

    let Some(nfh) = nfsm_srvmtofh(nfsd, slp, mrq)? else {
        return Ok(());
    };
    let fh = nfh.fh_generic();
    let (vp, _rdonly) = match fhtovp(nfsd, &fh, true, slp) {
        Ok(x) => x,
        Err(error) => {
            if let Some(mut mb) = nfsm_reply(nfsd, slp, mrq, error, NFSX_UNSIGNED)? {
                nfsm_srvpostop_attr(nfsd, None, &mut mb);
            }
            return Ok(());
        }
    };
    let mut at = Vattr::new();
    let getret = VOP_GETATTR(vp, &mut at, cred, procp).is_ok();
    vput(vp);
    let Some(mut mb) = nfsm_reply(nfsd, slp, mrq, 0, NFSX_V3POSTOPATTR + NFSX_V3FSINFO)? else {
        return Ok(());
    };
    nfsm_srvpostop_attr(nfsd, getret.then_some(&at), &mut mb);

    // XXX
    // There should be file system VFS OP(s) to get this information. For now, assume ufs.
    let pref = if slp
        .ns_so
        .get()
        .is_some_and(|so| so.so_type.get() == SOCK_DGRAM)
    {
        NFS_MAXDGRAMDATA
    } else {
        NFS_MAXDATA
    } as u32;
    let sip = Nfsv3Fsinfo {
        fs_rtmax: txdr_unsigned(NFS_MAXDATA as u32),
        fs_rtpref: txdr_unsigned(pref),
        fs_rtmult: txdr_unsigned(NFS_FABLKSIZE as u32),
        fs_wtmax: txdr_unsigned(NFS_MAXDATA as u32),
        fs_wtpref: txdr_unsigned(pref),
        fs_wtmult: txdr_unsigned(NFS_FABLKSIZE as u32),
        fs_dtpref: txdr_unsigned(pref),
        fs_maxfilesize: Nfsuint64::from_words([0xffff_ffff, 0xffff_ffff]),
        fs_timedelta: Nfsv3Time {
            nfsv3_sec: 0,
            nfsv3_nsec: txdr_unsigned(1),
        },
        fs_properties: txdr_unsigned(
            NFSV3FSINFO_LINK
                | NFSV3FSINFO_SYMLINK
                | NFSV3FSINFO_HOMOGENEOUS
                | NFSV3FSINFO_CANSETTIME,
        ),
    };
    nfsm_build(&mut mb, NFSX_V3FSINFO).write(0, &sip);
    Ok(())
}

/// `nfs pathconf service`
pub fn nfsrv_pathconf(
    nfsd: &mut NfsrvDescript,
    slp: &NfssvcSock,
    procp: &Proc,
    mrq: &mut Option<&'static Mbuf>,
) -> Result<(), Errno> {
    let cred: *const Ucred = &nfsd.nd_cr;

    let Some(nfh) = nfsm_srvmtofh(nfsd, slp, mrq)? else {
        return Ok(());
    };
    let fh = nfh.fh_generic();
    let (vp, _rdonly) = match fhtovp(nfsd, &fh, true, slp) {
        Ok(x) => x,
        Err(error) => {
            if let Some(mut mb) = nfsm_reply(nfsd, slp, mrq, error, NFSX_UNSIGNED)? {
                nfsm_srvpostop_attr(nfsd, None, &mut mb);
            }
            return Ok(());
        }
    };
    let mut linkmax: Register = 0;
    let mut namemax: Register = 0;
    let mut chownres: Register = 0;
    let mut notrunc: Register = 0;
    let r = VOP_PATHCONF(vp, _PC_LINK_MAX, &mut linkmax)
        .and_then(|()| VOP_PATHCONF(vp, _PC_NAME_MAX, &mut namemax))
        .and_then(|()| VOP_PATHCONF(vp, _PC_CHOWN_RESTRICTED, &mut chownres))
        .and_then(|()| VOP_PATHCONF(vp, _PC_NO_TRUNC, &mut notrunc));
    let error = status_of(&r);
    let mut at = Vattr::new();
    let getret = VOP_GETATTR(vp, &mut at, cred, procp).is_ok();
    vput(vp);
    let Some(mut mb) = nfsm_reply(nfsd, slp, mrq, error, NFSX_V3POSTOPATTR + NFSX_V3PATHCONF)?
    else {
        return Ok(());
    };
    nfsm_srvpostop_attr(nfsd, getret.then_some(&at), &mut mb);
    if error != 0 {
        return Ok(());
    }
    let pc = Nfsv3Pathconf {
        pc_linkmax: txdr_unsigned(linkmax as u32),
        pc_namemax: txdr_unsigned(namemax as u32),
        pc_notrunc: txdr_unsigned(notrunc as u32),
        pc_chownrestricted: txdr_unsigned(chownres as u32),
        // These should probably be supported by VOP_PATHCONF(), but until msdosfs is
        // exportable (why would you want to?), the Unix defaults should be ok.
        pc_caseinsensitive: nfs_false,
        pc_casepreserving: nfs_true,
    };
    nfsm_build(&mut mb, NFSX_V3PATHCONF).write(0, &pc);
    Ok(())
}

/// Null operation, used by clients to ping server
pub fn nfsrv_null(
    nfsd: &mut NfsrvDescript,
    slp: &NfssvcSock,
    _procp: &Proc,
    mrq: &mut Option<&'static Mbuf>,
) -> Result<(), Errno> {
    nfsm_reply(nfsd, slp, mrq, NFSERR_RETVOID, 0)?;
    Ok(())
}

/// No operation, used for obsolete procedures
pub fn nfsrv_noop(
    nfsd: &mut NfsrvDescript,
    slp: &NfssvcSock,
    _procp: &Proc,
    mrq: &mut Option<&'static Mbuf>,
) -> Result<(), Errno> {
    let error = if nfsd.nd_repstat != 0 {
        nfsd.nd_repstat
    } else {
        Errno::EPROCUNAVAIL.as_i32()
    };
    nfsm_reply(nfsd, slp, mrq, error, 0)?;
    Ok(())
}

/// Perform access checking for vnodes obtained from file handles that would refer to files
/// already opened by a Unix client. You cannot just use `vn_writechk()` and `VOP_ACCESS()`
/// for two reasons:
/// 1. You must check for exported rdonly as well as `MNT_RDONLY` for the write case.
/// 2. The owner is to be given access irrespective of mode bits for some operations
///    (`override_`), so that processes that chmod after opening a file don't break. I don't
///    like this because it opens a security hole, but since the nfs server opens a security
///    hole the size of a barn door anyhow, what the heck. A notable exception to this rule
///    is when `VOP_ACCESS()` returns `EPERM` (e.g. when a file is immutable) which is always
///    an error.
pub fn nfsrv_access(
    vp: &'static Vnode,
    flags: i32,
    cred: &Ucred,
    rdonly: bool,
    p: &Proc,
    override_: bool,
) -> Result<(), Errno> {
    if flags & VWRITE != 0 {
        // Just vn_writechk() changed to check rdonly
        //
        // Disallow write attempts on read-only file systems; unless the file is a socket or
        // a block or character device resident on the file system.
        if (rdonly || mnt_rdonly(vp)) && matches!(vp.v_type.get(), VREG | VDIR | VLNK) {
            return Err(Errno::EROFS);
        }
        // If there's shared text associated with the inode, try to free it up once. If we
        // fail, we can't allow writing.
        if vp.v_flag.get() & VTEXT != 0 && !uvm_vnp_uncache(vp) {
            return Err(Errno::ETXTBSY);
        }
    }
    let r = VOP_ACCESS(vp, flags, cred, p);
    // Allow certain operations for the owner (reads and writes on files that are already
    // open).
    if override_ && r == Err(Errno::EACCES) {
        let mut vattr = Vattr::new();
        if VOP_GETATTR(vp, &mut vattr, cred, p).is_ok() && cred.cr_uid.get() == vattr.va_uid {
            return Ok(());
        }
    }
    r
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for `nfs_serv.c`: the readdir packing and size rules, the `struct flrep`
    // layout, the request checks every procedure starts with, the null and no-op procedures,
    // and every procedure of the table against a real exported ffs mount (requests and replies
    // in XDR, version 2 and 3, reference counts checked by a clean unmount at the end).

    use core::mem::offset_of;
    use std::vec::Vec;

    use super::*;
    use crate::kern::uipc_mbuf::tests::setup as mbuf_setup;
    use crate::nfs::nfs_subs::tests::{bytes, chain};
    use crate::nfs::nfs_syscalls::NFSRV3_PROCS;
    use crate::nfs::nfsproto::NFS_NPROCS;
    use crate::nfs::nfsproto::{
        NFSERR_EXIST, NFSERR_NOENT, NFSPROC_GETATTR, NFSV3SATTRTIME_DONTCHANGE,
    };
    use crate::nfs::rpcv2::{RPC_GARBAGE, RPC_PROCUNAVAIL};
    use crate::nfs::xdr_subs::xdr_get;

    /// The raw XDR words of `b` as host values.
    fn words(b: &[u8]) -> Vec<u32> {
        b.chunks(4)
            .map(|c| u32::from_be_bytes([c[0], c[1], c[2], c[3]]))
            .collect()
    }

    /// The words of a reply chain, freed.
    fn reply_words(m: &'static Mbuf) -> Vec<u32> {
        let w = words(&bytes(m));
        m_freem(m);
        w
    }

    /// An XDR request body.
    #[derive(Default)]
    struct Req(Vec<u8>);

    impl Req {
        fn u(mut self, v: u32) -> Self {
            self.0.extend_from_slice(&v.to_be_bytes());
            self
        }

        fn hyper(self, v: u64) -> Self {
            self.u((v >> 32) as u32).u(v as u32)
        }

        fn raw(mut self, b: &[u8]) -> Self {
            self.0.extend_from_slice(b);
            self
        }

        /// A file handle: version 3 `nfs_fh3` (length and bytes), version 2 32 bytes.
        fn fh(self, fh: &Fhandle, v3: bool) -> Self {
            let b = xdr_bytes(fh).to_vec();
            if v3 {
                self.u(NFSX_V3FH as u32).raw(&b)
            } else {
                self.raw(&b).raw(&[0; NFSX_V2FH - NFSX_V3FH])
            }
        }

        /// A string or opaque: length, bytes and padding.
        fn name(self, s: &[u8]) -> Self {
            let pad = nfsm_padlen(s.len());
            self.u(s.len() as u32).raw(s).raw(&[0; 3][..pad])
        }

        /// A version 3 `sattr3` that sets only the mode (or nothing).
        fn sattr3(self, mode: Option<u32>) -> Self {
            let s = match mode {
                Some(m) => self.u(1).u(m),
                None => self.u(0),
            };
            s.u(0)
                .u(0)
                .u(0)
                .u(NFSV3SATTRTIME_DONTCHANGE)
                .u(NFSV3SATTRTIME_DONTCHANGE)
        }

        /// A version 2 `sattr` that sets only the mode.
        fn sattr2(self, mode: u32) -> Self {
            let mut s = self.u(mode);
            for _ in 0..7 {
                s = s.u(u32::MAX);
            }
            s
        }
    }

    /// A request descriptor for `body`, split over mbufs of 50 bytes (so that words straddle
    /// them), from `client`.
    fn descript(body: &[u8], v3: bool, client: Option<&'static Mbuf>) -> NfsrvDescript {
        let parts: Vec<&[u8]> = if body.is_empty() {
            std::vec![&[][..]]
        } else {
            body.chunks(50).collect()
        };
        let head = chain(&parts);
        let mut nd = NfsrvDescript::new();
        nd.nd_mrep = Some(head);
        nd.nd_md = Some(head);
        nd.nd_dpos = mtod::<u8>(head);
        if v3 {
            nd.nd_flag |= ND_NFSV3;
        }
        nd.nd_nam = client;
        nd.nd_retxid = 0x1234;
        nd
    }

    /// A reply being read back.
    struct Rd {
        w: Vec<u32>,
        i: usize,
    }

    impl Rd {
        /// The reply `w`: an accepted RPC reply whose `accept_stat` is `stat`.
        fn new(w: Vec<u32>, stat: u32) -> Rd {
            assert!(w.len() >= 6, "short reply {w:x?}");
            assert_eq!(&w[..6], &[0x1234, 1, 0, 0, 0, stat], "RPC reply header");
            Rd { w, i: 6 }
        }

        fn u(&mut self) -> u32 {
            self.i += 1;
            self.w[self.i - 1]
        }

        fn hyper(&mut self) -> u64 {
            (u64::from(self.u()) << 32) | u64::from(self.u())
        }

        fn n(&mut self, n: usize) -> Vec<u32> {
            self.i += n;
            self.w[self.i - n..self.i].to_vec()
        }

        /// A string or opaque.
        fn opaque(&mut self) -> Vec<u8> {
            let len = self.u() as usize;
            let n = nfsm_rndup(len) / 4;
            let mut b: Vec<u8> = self.n(n).iter().flat_map(|w| w.to_be_bytes()).collect();
            b.truncate(len);
            b
        }

        /// A `post_op_attr`: the 21 words of the attributes, if there.
        fn postop(&mut self) -> Option<Vec<u32>> {
            (self.u() == 1).then(|| self.n(NFSX_V3FATTR / 4))
        }

        /// A `wcc_data`: whether the pre-op attributes are there, and the post-op ones.
        fn wcc(&mut self) -> (bool, Option<Vec<u32>>) {
            let pre = self.u() == 1;
            if pre {
                self.n(6);
            }
            (pre, self.postop())
        }

        /// A `post_op_fh3`.
        fn postop_fh(&mut self) -> Option<Vec<u8>> {
            (self.u() == 1).then(|| self.opaque())
        }

        fn done(&self) {
            assert_eq!(self.i, self.w.len(), "the whole reply was read");
        }
    }

    /// Runs `f` on the request `body` and returns its result and the reply's words.
    fn run(
        f: NfsrvProc,
        body: &[u8],
        v3: bool,
        client: Option<&'static Mbuf>,
        p: &Proc,
    ) -> (Result<(), Errno>, Option<Vec<u32>>) {
        let mut nd = descript(body, v3, client);
        // nfsrv_errmap picks the version 3 status by procedure.
        nd.nd_procnum = NFSRV3_PROCS
            .iter()
            .position(|&g| core::ptr::fn_addr_eq(g, f))
            .expect("a procedure of the table");
        let slp = NfssvcSock::new();
        let mut mrq = None;
        let r = f(&mut nd, &slp, p, &mut mrq);
        assert!(nd.nd_mrep.is_none(), "the request was freed");
        (r, mrq.map(reply_words))
    }

    /// A directory entry as `VOP_READDIR` writes it.
    fn dirent(fileno: u64, off: i64, name: &[u8]) -> Vec<u8> {
        let reclen = (Dirent::NAME_OFFSET + name.len() + 1 + 7) & !7;
        let mut b = std::vec![0u8; reclen];
        b[offset_of!(Dirent, d_fileno)..][..8].copy_from_slice(&fileno.to_ne_bytes());
        b[offset_of!(Dirent, d_off)..][..8].copy_from_slice(&off.to_ne_bytes());
        b[offset_of!(Dirent, d_reclen)..][..2].copy_from_slice(&(reclen as u16).to_ne_bytes());
        b[offset_of!(Dirent, d_namlen)] = name.len() as u8;
        b[Dirent::NAME_OFFSET..][..name.len()].copy_from_slice(name);
        b
    }

    #[test]
    fn readdir_packs_entries_with_a_file_number_while_they_fit() {
        let _g = mbuf_setup();
        let mut buf = dirent(5, 100, b"a");
        buf.extend(dirent(0, 200, b"gone"));
        buf.extend(dirent(7, 300, b"bcdef"));

        // Version 3: true, fileid (hyper), name, cookie (hyper).
        let m = m_get(M_WAIT, MT_DATA).expect("an mbuf");
        let mut mb = m;
        assert!(nfsrv_readdir_pack(&mut mb, &buf, true, 1000, 12));
        let w = reply_words(m);
        assert_eq!(
            w,
            [
                1,
                0,
                5,
                1,
                u32::from_be_bytes(*b"a\0\0\0"),
                0,
                100, //
                1,
                0,
                7,
                5,
                u32::from_be_bytes(*b"bcde"),
                u32::from_be_bytes(*b"f\0\0\0"),
                0,
                300,
            ]
        );

        // Version 2: true, fileid, name, cookie; the second entry does not fit the count.
        let m = m_get(M_WAIT, MT_DATA).expect("an mbuf");
        let mut mb = m;
        // 12 + (16 + 4) = 32 for "a", then 32 + (16 + 8) = 56 > 40.
        assert!(!nfsrv_readdir_pack(&mut mb, &buf, false, 40, 12));
        let w = reply_words(m);
        assert_eq!(w, [1, 5, 1, u32::from_be_bytes(*b"a\0\0\0"), 100]);

        // A truncated entry or one with no length ends the walk.
        let m = m_get(M_WAIT, MT_DATA).expect("an mbuf");
        let mut mb = m;
        let mut bad = dirent(9, 1, b"x");
        bad[offset_of!(Dirent, d_reclen)..][..2].copy_from_slice(&0u16.to_ne_bytes());
        assert!(nfsrv_readdir_pack(&mut mb, &bad, true, 1000, 12));
        assert!(reply_words(m).is_empty());
        assert_eq!(nfsrv_dirent_skip(&bad), bad.len());

        // The degenerate-case walk stops at the first entry with a file number.
        let mut skip = dirent(0, 1, b"x");
        let first = skip.len();
        skip.extend(dirent(3, 2, b"y"));
        assert_eq!(nfsrv_dirent_skip(&skip), first);
        assert_eq!(
            nfsrv_dirent_skip(&dirent(0, 1, b"x")),
            dirent(0, 1, b"x").len()
        );
    }

    #[test]
    fn readdir_sizes_follow_the_c() {
        assert_eq!(nfsrv_readdir_cnt(100, 8192), 100);
        assert_eq!(nfsrv_readdir_cnt(-1, 8192), 8192);
        assert_eq!(nfsrv_readdir_cnt(9000, 8192), 8192);
        assert_eq!(nfsrv_readdir_siz(100, 8192), 512);
        assert_eq!(nfsrv_readdir_siz(512, 8192), 512);
        assert_eq!(nfsrv_readdir_siz(513, 8192), 1024);
        assert_eq!(nfsrv_readdir_siz(9000, 8192), 8192);
        assert_eq!(
            nfsrv_readdir_siz(i32::MAX, 8192),
            8192,
            "wraps negative: xfer"
        );
    }

    #[test]
    fn flrep_is_cookie_attributes_and_handle() {
        let fattr = NfsFattr {
            fa_type: txdr_unsigned(1),
            ..NfsFattr::default()
        };
        let mut nfh = Nfsfh::new();
        nfh.fh_bytes[0] = 0xaa;
        nfh.fh_bytes[NFSX_V3FH - 1] = 0xbb;
        nfh.fh_bytes[NFSX_V3FH] = 0xcc; // past the handle: not copied
        let fl = nfsrv_flrep(0x1_0000_0002, &fattr, &nfh);
        assert_eq!(FLREP_SIZE, 33 * 4);
        let w = words(&fl);
        assert_eq!(&w[..4], &[1, 2, 1, 1], "cookie, post_op_attr true, fa_type");
        assert_eq!(
            &w[24..26],
            &[1, NFSX_V3FH as u32],
            "post_op_fh3 true, length"
        );
        assert_eq!(fl[26 * 4], 0xaa);
        assert_eq!(fl[FLREP_SIZE - 1], 0xbb);
    }

    #[test]
    fn null_noop_and_bad_requests() {
        let (_g, p) = crate::kern::vfs_subr::tests::setup();
        crate::kern::uipc_mbuf::tests::mbinit_again();

        // NULL: an accepted reply with no status.
        let (r, w) = run(nfsrv_null, &[], true, None, p);
        assert_eq!(r, Ok(()));
        Rd::new(w.expect("a reply"), 0).done();

        // An obsolete procedure: PROC_UNAVAIL ...
        let (r, w) = run(nfsrv_noop, &[], false, None, p);
        assert_eq!(r, Ok(()));
        Rd::new(w.expect("a reply"), RPC_PROCUNAVAIL).done();
        // ... or the status nfs_getreq left.
        let mut nd = descript(&[], true, None);
        nd.nd_repstat = Errno::EBADRPC.as_i32();
        let slp = NfssvcSock::new();
        let mut mrq = None;
        assert_eq!(nfsrv_noop(&mut nd, &slp, p, &mut mrq), Ok(()));
        Rd::new(reply_words(mrq.expect("a reply")), RPC_GARBAGE).done();

        // A version 3 file handle of the wrong length: GARBAGE_ARGS, no NFS status.
        let body = Req::default().u(12).raw(&[0; 12]).0;
        let (r, w) = run(nfsrv_getattr, &body, true, None, p);
        assert_eq!(r, Ok(()));
        Rd::new(w.expect("a reply"), RPC_GARBAGE).done();

        // A request that ends inside the file handle is dropped without a reply.
        let body = Req::default().u(NFSX_V3FH as u32).raw(&[0; 8]).0;
        let (r, w) = run(nfsrv_getattr, &body, true, None, p);
        assert_eq!(r, Err(Errno::EBADRPC));
        assert!(w.is_none());

        // A request without a client address does not pass the port check.
        let body = Req::default().fh(&Fhandle::default(), true).0;
        let (r, w) = run(nfsrv_getattr, &body, true, None, p);
        assert_eq!(r, Ok(()));
        // MSG_DENIED, AUTH_ERROR, AUTH_TOOWEAK.
        assert_eq!(w.expect("a reply"), [0x1234, 1, 1, 1, AUTH_TOOWEAK]);

        // The table is in procedure number order.
        assert!(core::ptr::fn_addr_eq(
            NFSRV3_PROCS[NFSPROC_GETATTR],
            nfsrv_getattr as NfsrvProc
        ));
        assert!(core::ptr::fn_addr_eq(
            NFSRV3_PROCS[NFS_NPROCS - 1],
            nfsrv_noop as NfsrvProc
        ));
    }

    /// An `MT_SONAME` mbuf holding the `AF_INET` address `a`, port `port`.
    fn nam(a: [u8; 4], port: u16) -> &'static Mbuf {
        use crate::sys::mbuf::{M_DONTWAIT, MT_SONAME};
        let m = m_get(M_DONTWAIT, MT_SONAME).expect("an mbuf");
        let mut sin = [0u8; 16];
        sin[0] = 16;
        sin[1] = crate::sys::socket::AF_INET;
        sin[2..4].copy_from_slice(&port.to_be_bytes());
        sin[4..8].copy_from_slice(&a);
        // SAFETY: an mbuf's data area holds 16 bytes; `mtod` points at it.
        unsafe { ptr::copy_nonoverlapping(sin.as_ptr(), mtod::<u8>(m), 16) };
        m.m_len().set(16);
        m
    }

    #[test]
    fn procedures_over_an_exported_ffs() {
        use crate::kern::kern_descrip::sys_close;
        use crate::kern::sys_generic::sys_write;
        use crate::kern::uipc_mbuf::tests::mbinit_again;
        use crate::kern::vfs_lookup::namei;
        use crate::kern::vfs_subr::tests::exports::{args, sin};
        use crate::kern::vfs_syscalls::{sys_mkdir, sys_mount, sys_open};
        use crate::nfs::nfsproto::NFSV3CREATE_EXCLUSIVE;
        use crate::sys::fcntl::{O_CREAT, O_RDWR};
        use crate::sys::mount::{MNT_EXPORTED, MNT_UPDATE, UfsArgs};
        use crate::ufs::ffs::ffs_vfsops::tests::{
            mount_root, newfs, path, read_file, setup, sys, teardown, unmount_root,
        };

        let img = newfs::Image::new(newfs::FFS2_4M);
        let (_g, p) = setup(img.finish());
        mbinit_again();
        let mp = mount_root(p, false);

        sys(sys_mkdir, p, &[path(b"/dir\0"), 0o755]).unwrap();
        let fd = sys(
            sys_open,
            p,
            &[path(b"/dir/f\0"), (O_CREAT | O_RDWR) as usize, 0o644],
        )
        .unwrap();
        let hello = b"hello";
        sys(
            sys_write,
            p,
            &[fd as usize, hello.as_ptr() as usize, hello.len()],
        )
        .unwrap();
        sys(sys_close, p, &[fd as usize]).unwrap();

        let fh = |name: &'static [u8]| -> Fhandle {
            let mut nd = ndinit(LOOKUP, FOLLOW, NiDirp::Sys(name), p);
            namei(&mut nd).unwrap();
            let vp = nd.ni_vp.unwrap();
            let mut fid = Default::default();
            VFS_VPTOFH(vp, &mut fid).unwrap();
            vrele(vp);
            Fhandle {
                fh_fsid: mp.mnt_stat.get().f_fsid,
                fh_fid: fid,
            }
        };
        let dirfh = fh(b"/dir");
        let filefh = fh(b"/dir/f");

        // mountd(8): export it read-write to 10.0.2.0/24, root mapped to root.
        let net = sin(2, [10, 0, 2, 0]);
        let mask = sin(2, [255, 255, 255, 0]);
        let mut ua = UfsArgs {
            fspec: 0,
            export_info: args(MNT_EXPORTED, 0, Some(net), Some(mask)),
        };
        sys(
            sys_mount,
            p,
            &[
                b"ffs\0".as_ptr() as usize,
                path(b"/\0"),
                MNT_UPDATE as usize,
                ptr::from_mut(&mut ua) as usize,
            ],
        )
        .unwrap();
        let client = Some(nam([10, 0, 2, 9], 700));
        let call = |f: NfsrvProc, body: Req, v3: bool| -> Rd {
            let (r, w) = run(f, &body.0, v3, client, p);
            assert_eq!(r, Ok(()));
            Rd::new(w.expect("a reply"), 0)
        };

        // GETATTR, both versions.
        let mut rd = call(nfsrv_getattr, Req::default().fh(&dirfh, true), true);
        assert_eq!(rd.u(), 0);
        let fa = rd.n(21);
        assert_eq!((fa[0], fa[1] & 0o7777), (2, 0o755), "NF3DIR, mode");
        rd.done();
        let mut rd = call(nfsrv_getattr, Req::default().fh(&filefh, false), false);
        assert_eq!(rd.u(), 0);
        let fa = rd.n(17);
        assert_eq!((fa[0], fa[5]), (1, 5), "NFREG, size");
        rd.done();

        // LOOKUP: the handle and attributes of the file, the directory's attributes.
        let mut rd = call(
            nfsrv_lookup,
            Req::default().fh(&dirfh, true).name(b"f"),
            true,
        );
        assert_eq!(rd.u(), 0);
        assert_eq!(rd.opaque(), xdr_bytes(&filefh));
        assert!(rd.postop().is_some());
        assert!(rd.postop().is_some());
        rd.done();
        let mut rd = call(
            nfsrv_lookup,
            Req::default().fh(&dirfh, true).name(b"nope"),
            true,
        );
        assert_eq!(rd.u() as i32, NFSERR_NOENT);
        assert!(rd.postop().is_some(), "the directory's attributes");
        rd.done();
        // A name too long for NFS: NAMETOOLONG, nothing more.
        let long = [b'x'; NFS_MAXNAMLEN + 1];
        let mut rd = call(
            nfsrv_lookup,
            Req::default().fh(&dirfh, true).name(&long),
            true,
        );
        assert_eq!(rd.u() as i32, NFSERR_NAMETOL);
        rd.done();

        // ACCESS: root may read and modify a 0644 file, not execute it.
        let mut rd = call(
            nfsrv3_access,
            Req::default()
                .fh(&filefh, true)
                .u(NFSV3ACCESS_READ | NFSV3ACCESS_MODIFY | NFSV3ACCESS_EXECUTE),
            true,
        );
        assert_eq!(rd.u(), 0);
        assert!(rd.postop().is_some());
        assert_eq!(rd.u(), NFSV3ACCESS_READ | NFSV3ACCESS_MODIFY);
        rd.done();

        // READ: all of it, eof.
        let mut rd = call(
            nfsrv_read,
            Req::default().fh(&filefh, true).hyper(0).u(100),
            true,
        );
        assert_eq!(rd.u(), 0);
        assert!(rd.postop().is_some());
        assert_eq!((rd.u(), rd.u()), (5, 1), "count, eof");
        assert_eq!(rd.opaque(), hello);
        rd.done();

        // WRITE (the data spread over several mbufs), then a version 2 READ of the result.
        let data = b" world, from far away";
        let mut rd = call(
            nfsrv_write,
            Req::default()
                .fh(&filefh, true)
                .hyper(5)
                .u(data.len() as u32)
                .u(NFSV3WRITE_FILESYNC)
                .name(data),
            true,
        );
        assert_eq!(rd.u(), 0);
        assert_eq!(rd.wcc().0, true);
        assert_eq!((rd.u(), rd.u()), (data.len() as u32, NFSV3WRITE_FILESYNC));
        rd.n(2); // the write verifier
        rd.done();
        let mut rd = call(
            nfsrv_read,
            Req::default().fh(&filefh, false).u(0).u(100).u(0),
            false,
        );
        assert_eq!(rd.u(), 0);
        let fa = rd.n(17);
        assert_eq!(fa[5] as usize, 5 + data.len());
        assert_eq!(rd.opaque(), b"hello world, from far away");
        rd.done();
        assert_eq!(
            read_file(p, b"/dir/f\0").unwrap(),
            b"hello world, from far away"
        );
        // A read of 3 bytes: the data padded to a word (nfsm_adj), not eof.
        let mut rd = call(
            nfsrv_read,
            Req::default().fh(&filefh, true).hyper(1).u(3),
            true,
        );
        assert_eq!(rd.u(), 0);
        assert!(rd.postop().is_some());
        assert_eq!((rd.u(), rd.u()), (3, 0), "count, eof");
        let n = rd.u();
        assert_eq!(n, 3);
        assert_eq!(rd.u().to_be_bytes(), *b"ell\0");
        rd.done();
        // A read past the end: nothing, eof.
        let mut rd = call(
            nfsrv_read,
            Req::default().fh(&filefh, true).hyper(1000).u(10),
            true,
        );
        assert_eq!(rd.u(), 0);
        assert!(rd.postop().is_some());
        assert_eq!((rd.u(), rd.u()), (0, 1));
        assert_eq!(rd.opaque(), b"");
        rd.done();

        // CREATE: unchecked, then guarded over it (EEXIST, the file not left locked).
        let mut rd = call(
            nfsrv_create,
            Req::default()
                .fh(&dirfh, true)
                .name(b"new")
                .u(NFSV3CREATE_UNCHECKED)
                .sattr3(Some(0o600)),
            true,
        );
        assert_eq!(rd.u(), 0);
        let newfh = rd.postop_fh().expect("the handle");
        let fa = rd.postop().expect("the attributes");
        assert_eq!((fa[0], fa[1] & 0o7777), (1, 0o600));
        let (pre, post) = rd.wcc();
        assert!(pre && post.is_some());
        rd.done();
        let mut rd = call(
            nfsrv_create,
            Req::default()
                .fh(&dirfh, true)
                .name(b"new")
                .u(NFSV3CREATE_GUARDED)
                .sattr3(None),
            true,
        );
        assert_eq!(rd.u() as i32, NFSERR_EXIST);
        let (pre, post) = rd.wcc();
        assert!(pre && post.is_some());
        rd.done();
        // Exclusive: the verifier goes into va_atime.tv_sec, but with tv_nsec left VNOVAL
        // ufs_setattr does not set the times, so the file's atime never matches it and even a
        // retransmission with the same verifier is EEXIST: the C's behaviour with a 64-bit
        // time_t, kept.
        let excl = |verf: &[u8; 8]| {
            call(
                nfsrv_create,
                Req::default()
                    .fh(&dirfh, true)
                    .name(b"excl")
                    .u(NFSV3CREATE_EXCLUSIVE)
                    .raw(verf),
                true,
            )
            .u()
        };
        assert_eq!(excl(b"verifier"), 0);
        assert_eq!(excl(b"verifier") as i32, NFSERR_EXIST);
        assert_eq!(excl(b"other!!!") as i32, NFSERR_EXIST);
        // Version 2: a file with the sattr's mode, its handle and attributes.
        let mut rd = call(
            nfsrv_create,
            Req::default()
                .fh(&dirfh, false)
                .name(b"v2file")
                .sattr2(0o100640),
            false,
        );
        assert_eq!(rd.u(), 0);
        rd.n(8); // the handle
        let fa = rd.n(17);
        assert_eq!((fa[0], fa[1] & 0o7777), (1, 0o640));
        rd.done();

        // MKDIR, MKNOD (a fifo), SYMLINK and READLINK.
        let mut rd = call(
            nfsrv_mkdir,
            Req::default()
                .fh(&dirfh, true)
                .name(b"sub")
                .sattr3(Some(0o700)),
            true,
        );
        assert_eq!(rd.u(), 0);
        assert!(rd.postop_fh().is_some());
        assert_eq!(rd.postop().expect("attributes")[0], 2);
        rd.wcc();
        rd.done();
        // A character device (VOP_MKNOD, then the lookup again), a socket (VOP_CREATE), and a
        // fifo, which this ffs cannot load (no fifofs): VOP_MKNOD makes the entry, the lookup
        // after it fails with EOPNOTSUPP, mapped to NFSERR_IO.
        let mut rd = call(
            nfsrv_mknod,
            Req::default()
                .fh(&dirfh, true)
                .name(b"chr")
                .u(4)
                .sattr3(Some(0o600))
                .u(2)
                .u(3),
            true,
        );
        assert_eq!(rd.u(), 0);
        assert!(rd.postop_fh().is_some());
        let fa = rd.postop().expect("attributes");
        assert_eq!((fa[0], fa[9], fa[10]), (4, 2, 3), "NF3CHR, major, minor");
        rd.wcc();
        rd.done();
        let mut rd = call(
            nfsrv_mknod,
            Req::default()
                .fh(&dirfh, true)
                .name(b"sock")
                .u(6)
                .sattr3(Some(0o644)),
            true,
        );
        assert_eq!(rd.u(), 0);
        assert!(rd.postop_fh().is_some());
        assert_eq!(rd.postop().expect("attributes")[0], 6, "NF3SOCK");
        rd.wcc();
        rd.done();
        let mut rd = call(
            nfsrv_mknod,
            Req::default()
                .fh(&dirfh, true)
                .name(b"fifo")
                .u(7)
                .sattr3(Some(0o644)),
            true,
        );
        assert_eq!(rd.u(), 5, "NFSERR_IO");
        let (pre, post) = rd.wcc();
        assert!(pre && post.is_some());
        rd.done();
        // A regular file is not a type for MKNOD.
        let mut rd = call(
            nfsrv_mknod,
            Req::default().fh(&dirfh, true).name(b"reg").u(1),
            true,
        );
        assert_eq!(rd.u() as i32, NFSERR_BADTYPE);
        rd.wcc();
        rd.done();
        let mut rd = call(
            nfsrv_symlink,
            Req::default()
                .fh(&dirfh, true)
                .name(b"ln")
                .sattr3(Some(0o755)) // the mode clients send (VNOVAL would make a bad inode, as in C)
                .name(b"f"),
            true,
        );
        assert_eq!(rd.u(), 0);
        let lnfh = rd.postop_fh().expect("the link's handle");
        assert_eq!(rd.postop().expect("attributes")[0], 5, "NF3LNK");
        rd.wcc();
        rd.done();
        let lnfh: Fhandle = xdr_get(&lnfh);
        let mut rd = call(nfsrv_readlink, Req::default().fh(&lnfh, true), true);
        assert_eq!(rd.u(), 0);
        assert!(rd.postop().is_some());
        assert_eq!(rd.opaque(), b"f");
        rd.done();
        // READLINK of a file that is not a link: version 2 ENXIO.
        let mut rd = call(nfsrv_readlink, Req::default().fh(&filefh, false), false);
        assert_eq!(rd.u() as i32, Errno::ENXIO.as_i32());
        rd.done();

        // LINK, RENAME, REMOVE.
        let mut rd = call(
            nfsrv_link,
            Req::default()
                .fh(&filefh, true)
                .fh(&dirfh, true)
                .name(b"hard"),
            true,
        );
        assert_eq!(rd.u(), 0);
        assert_eq!(rd.postop().expect("attributes")[2], 2, "two links");
        rd.wcc();
        rd.done();
        let mut rd = call(
            nfsrv_rename,
            Req::default()
                .fh(&dirfh, true)
                .name(b"hard")
                .fh(&dirfh, true)
                .name(b"hard2"),
            true,
        );
        assert_eq!(rd.u(), 0);
        rd.wcc();
        rd.wcc();
        rd.done();
        // A second file handle of the wrong length: GARBAGE_ARGS after the source was looked
        // up (nfsmout releases it), and a request that ends after the source name is dropped.
        let (r, w) = run(
            nfsrv_rename,
            &Req::default()
                .fh(&dirfh, true)
                .name(b"f")
                .u(12)
                .raw(&[0; 12])
                .0,
            true,
            client,
            p,
        );
        assert_eq!(r, Ok(()));
        Rd::new(w.expect("a reply"), RPC_GARBAGE).done();
        let (r, w) = run(
            nfsrv_rename,
            &Req::default().fh(&dirfh, true).name(b"f").0,
            true,
            client,
            p,
        );
        assert_eq!(r, Err(Errno::EBADRPC));
        assert!(w.is_none());
        // Renaming a name to itself does nothing.
        let mut rd = call(
            nfsrv_rename,
            Req::default()
                .fh(&dirfh, false)
                .name(b"hard2")
                .fh(&dirfh, false)
                .name(b"hard2"),
            false,
        );
        assert_eq!(rd.u(), 0);
        rd.done();
        let mut rd = call(
            nfsrv_remove,
            Req::default().fh(&dirfh, true).name(b"hard2"),
            true,
        );
        assert_eq!(rd.u(), 0);
        rd.wcc();
        rd.done();
        let mut rd = call(
            nfsrv_remove,
            Req::default().fh(&dirfh, true).name(b"hard2"),
            true,
        );
        assert_eq!(rd.u() as i32, NFSERR_NOENT);
        rd.wcc();
        rd.done();
        // RMDIR of a file: ENOTDIR; of the directory: done.
        let mut rd = call(
            nfsrv_rmdir,
            Req::default().fh(&dirfh, true).name(b"f"),
            true,
        );
        assert_eq!(rd.u() as i32, Errno::ENOTDIR.as_i32());
        rd.wcc();
        rd.done();
        let mut rd = call(
            nfsrv_rmdir,
            Req::default().fh(&dirfh, true).name(b"sub"),
            true,
        );
        assert_eq!(rd.u(), 0);
        rd.wcc();
        rd.done();

        // READDIR and READDIRPLUS: every name, eof.
        let expect: Vec<&[u8]> = std::vec![
            b".", b"..", b"f", b"new", b"excl", b"v2file", b"chr", b"sock", b"ln", b"fifo"
        ];
        let mut rd = call(
            nfsrv_readdir,
            Req::default().fh(&dirfh, true).hyper(0).hyper(0).u(4096),
            true,
        );
        assert_eq!(rd.u(), 0);
        assert!(rd.postop().is_some());
        rd.n(2); // the cookie verifier
        let mut names = Vec::new();
        while rd.u() == 1 {
            rd.hyper();
            names.push(rd.opaque());
            rd.hyper();
        }
        assert_eq!(rd.u(), 1, "eof");
        rd.done();
        let mut sorted = names.clone();
        sorted.sort();
        let mut want: Vec<Vec<u8>> = expect.iter().map(|n| n.to_vec()).collect();
        want.sort();
        assert_eq!(sorted, want);
        let mut rd = call(
            nfsrv_readdirplus,
            Req::default()
                .fh(&dirfh, true)
                .hyper(0)
                .hyper(0)
                .u(4096)
                .u(8192),
            true,
        );
        assert_eq!(rd.u(), 0);
        assert!(rd.postop().is_some());
        rd.n(2);
        let mut plus = Vec::new();
        while rd.u() == 1 {
            rd.hyper();
            let name = rd.opaque();
            rd.hyper();
            assert!(rd.postop().is_some());
            let h = rd.postop_fh().expect("a handle");
            if name == b"f" {
                assert_eq!(h, xdr_bytes(&filefh));
            }
            plus.push(name);
        }
        assert_eq!(rd.u(), 1, "eof");
        rd.done();
        assert_eq!(plus, names);
        // A count too small for all of them: the first few, not eof; the C's paranoia words
        // (12) and one version 3 entry of a short name (32) fit 64 bytes, two do not.
        let mut rd = call(
            nfsrv_readdir,
            Req::default().fh(&dirfh, true).hyper(0).hyper(0).u(64),
            true,
        );
        assert_eq!(rd.u(), 0);
        assert!(rd.postop().is_some());
        rd.n(2);
        let mut few = 0;
        while rd.u() == 1 {
            rd.hyper();
            rd.opaque();
            rd.hyper();
            few += 1;
        }
        assert_eq!((few, rd.u()), (1, 0), "one entry, not eof");
        rd.done();
        // A count of 0: TOOSMALL.
        let mut rd = call(
            nfsrv_readdir,
            Req::default().fh(&dirfh, true).hyper(0).hyper(0).u(0),
            true,
        );
        assert_eq!(rd.u() as i32, NFSERR_TOOSMALL);
        assert!(rd.postop().is_none());
        rd.done();

        // SETATTR: truncate the new file (guard off), then GETATTR sees it.
        let newfh: Fhandle = xdr_get(&newfh);
        let mut rd = call(
            nfsrv_setattr,
            Req::default()
                .fh(&newfh, true)
                .u(0)
                .u(0)
                .u(0)
                .u(1)
                .hyper(1234)
                .u(0)
                .u(0)
                .u(0),
            true,
        );
        assert_eq!(rd.u(), 0);
        let (pre, post) = rd.wcc();
        assert!(pre);
        let post = post.expect("post-op attributes");
        assert_eq!(((u64::from(post[5]) << 32) | u64::from(post[6])), 1234);
        rd.done();

        // STATFS, FSINFO, PATHCONF, COMMIT.
        let mut rd = call(nfsrv_statfs, Req::default().fh(&dirfh, true), true);
        assert_eq!(rd.u(), 0);
        assert!(rd.postop().is_some());
        let sf = rd.n(13);
        assert!(sf[0] > 0 || sf[1] > 0, "total bytes");
        rd.done();
        let mut rd = call(nfsrv_statfs, Req::default().fh(&dirfh, false), false);
        assert_eq!(rd.u(), 0);
        let sf = rd.n(5);
        assert_eq!(sf[0], NFS_MAXDGRAMDATA as u32);
        rd.done();
        let mut rd = call(nfsrv_fsinfo, Req::default().fh(&dirfh, true), true);
        assert_eq!(rd.u(), 0);
        assert!(rd.postop().is_some());
        let fs = rd.n(12);
        assert_eq!((fs[0], fs[1]), (NFS_MAXDATA as u32, NFS_MAXDATA as u32));
        assert_eq!(fs[11], 0x1b, "LINK | SYMLINK | HOMOGENEOUS | CANSETTIME");
        rd.done();
        let mut rd = call(nfsrv_pathconf, Req::default().fh(&dirfh, true), true);
        assert_eq!(rd.u(), 0);
        assert!(rd.postop().is_some());
        let pc = rd.n(6);
        assert_eq!(pc[1], 255, "name max");
        assert_eq!((pc[4], pc[5]), (0, 1), "case sensitive, preserving");
        rd.done();
        let mut rd = call(
            nfsrv_commit,
            Req::default().fh(&filefh, true).hyper(0).u(0),
            true,
        );
        assert_eq!(rd.u(), 0);
        let (pre, post) = rd.wcc();
        assert!(pre && post.is_some());
        rd.n(2);
        rd.done();

        // A client outside the export list: EACCES (version 2; version 3 GETATTR maps it to
        // NFSERR_IO).
        let (r, w) = run(
            nfsrv_getattr,
            &Req::default().fh(&dirfh, false).0,
            false,
            Some(nam([10, 0, 3, 9], 700)),
            p,
        );
        assert_eq!(r, Ok(()));
        let mut rd = Rd::new(w.expect("a reply"), 0);
        assert_eq!(rd.u() as i32, Errno::EACCES.as_i32());
        rd.done();

        // Nothing is left referenced or locked: the file system unmounts.
        mbinit_again();
        unmount_root(p, mp);
        teardown();
    }
}
/* </TESTS> */
