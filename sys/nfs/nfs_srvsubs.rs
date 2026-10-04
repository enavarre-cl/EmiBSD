/*	$OpenBSD: nfs_srvsubs.c,v 1.5 2026/06/09 02:55:17 jsg Exp $	*/
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

//! `nfs/nfs_srvsubs.c`: the server's helpers for the NFS op functions of `nfs_serv.c`: the
//! lookup of a name from a request (`nfs_namei`), the trimming of a reply chain (`nfsm_adj`),
//! the reply builders for attributes and weak cache consistency data, the file handle to
//! vnode conversion with its export and privileged port checks (`nfsrv_fhtovp`), the
//! host address comparison (`netaddr_match`) and the NFSv3 `sattr3` decoder.
//!
//! Upstream: sys/nfs/nfs_srvsubs.c @ 3ce1f3f79392
//!
//! The file is compiled only with `option NFSSERVER` (`sys/conf/files`), so the module is
//! behind the `nfsserver` feature.
//!
//! ## Deviations
//! - `nfs_namei` and `nfsrv_fhtovp` return `Result<_, i32>`: the C's `int error` is an errno
//!   or an NFS status (`NFSERR_AUTHERR | AUTH_TOOWEAK` is not an [`Errno`]) and goes straight
//!   into `nd_repstat`. An `Errno` converts with [`Errno::as_i32`]. The vnode the C returns
//!   through `vpp`/`retdirp`, and `*rdonlyp`, are in the `Ok` value, except for `nfs_namei`'s
//!   `retdirp`: the C sets it before the lookup runs and the caller `vrele`s it on every path
//!   after, errors included, so it stays an out parameter (`&mut Option<&'static Vnode>`).
//! - `nfs_namei` copies the name into the `namei_pool` buffer a slice at a time with
//!   `nfsm_nextbytes` (`nfs_subs.rs`), so it never dereferences `*dposp`. A name that does not
//!   fit the buffer fails with `ENAMETOOLONG` (the C relies on its callers' `nfsm_srvnamesiz`
//!   limit of `NFS_MAXNAMLEN`). It gives the buffer back with a null `cn_pnbuf`; the C leaves
//!   the pointer dangling. `nfs_adv` computes the bytes left itself (the C's `rem` argument).
//! - `nfsm_srvwcc` and `nfsm_srvpostop_attr` take the attributes as `Option<&Vattr>`: `None`
//!   is the C's non-zero `before_ret`/`after_ret` (the `VOP_GETATTR` failed), so no caller
//!   passes an uninitialised `struct vattr` along with a flag.
//! - `nfsm_srvfattr` returns the [`NfsFattr`] instead of filling the one the caller has just
//!   `nfsm_build`t; the caller writes it with `XdrOut::write(0, &fp)` (a version 2 reply
//!   holds `NFSX_V2FATTR` bytes of it).
//! - `nfsrv_fhtovp` reads the client's `struct sockaddr_in` out of the address mbuf with an
//!   unaligned copy; an address mbuf shorter than a `sockaddr_in` fails the privileged port
//!   check (`NFSERR_AUTHERR | AUTH_TOOWEAK`) rather than being read past its end. It takes the
//!   exported file system's credentials (`VFS_CHECKEXP`'s `credanon`, a pointer into the
//!   export list) as the C does; a null one cannot happen with the file systems of this tree
//!   and fails with `EACCES` rather than being dereferenced.
//! - `nfsm_adj` clamps the null fill to the mbuf's data (the C would write before it if
//!   `nul > m_len`; its callers never ask for that).
//! - `netaddr_match` returns a `bool`.

use core::ptr::{self, NonNull};

use crate::kern::kern_tc::getnanotime;
use crate::kern::subr_pool::{pool_get, pool_put};
use crate::kern::vfs_init::NAMEI_POOL;
use crate::kern::vfs_lookup::vfs_lookup;
use crate::kern::vfs_subr::{vfs_getvfs, vput, vref, vrele};
use crate::kern::vfs_vops::VOP_UNLOCK;
use crate::netinet::in_::{IPPORT_RESERVED, SockaddrIn};
use crate::nfs::nfs::{ND_NFSV3, Nethostaddr, NfsrvDescript, NfssvcSock};
use crate::nfs::nfs_subs::{nfs_adv, nfs_false, nfs_true, nfsm_build, nfsm_nextbytes};
use crate::nfs::nfs_var::nfsm_padlen;
use crate::nfs::nfsm_subs::nfsd_dissect;
use crate::nfs::nfsproto::{
    NFS_FABLKSIZE, NFSERR_AUTHERR, NFSV3SATTRTIME_TOCLIENT, NFSV3SATTRTIME_TOSERVER, NFSX_UNSIGNED,
    NFSX_V3FATTR, NfsFattr, Nfsuint64, Nfsv3Spec, Nfsv3Time, nfstov_mode, vtonfsv2_mode,
    vtonfsv2_type, vtonfsv3_mode, vtonfsv3_type,
};
use crate::nfs::rpcv2::AUTH_TOOWEAK;
use crate::nfs::xdr_subs::{
    fxdr_nfsv3time, fxdr_unsigned, txdr_hyper, txdr_nfsv2time, txdr_nfsv3time, txdr_unsigned,
};
use crate::sys::errno::Errno;
use crate::sys::mbuf::{Mbuf, mtod};
use crate::sys::mount::{Fhandle, MNT_EXPORTANON, MNT_EXRDONLY, VFS_CHECKEXP, VFS_FHTOVP};
use crate::sys::namei::{
    HASBUF, ISSYMLINK, LOCKPARENT, NOCROSSMOUNT, Nameidata, RDONLY, SAVENAME, SAVESTART,
};
use crate::sys::param::MAXPATHLEN;
use crate::sys::pool::PR_WAITOK;
use crate::sys::proc::Proc;
use crate::sys::socket::{AF_INET, SOCK_STREAM};
use crate::sys::syslimits::NGROUPS_MAX;
use crate::sys::time::Timespec;
use crate::sys::types::{SaFamily, major, minor};
use crate::sys::ucred::Ucred;
use crate::sys::vnode::{VA_UTIMES_CHANGE, VA_UTIMES_NULL, VFIFO, Vattr, Vnode};

/// `nfs_namei(ndp, fhp, len, slp, nam, mdp, dposp, retdirp, p)`: sets up the `nameidata` for
/// a `vfs_lookup` of the `len`-byte name at the dissection cursor (`*mdp`, `*dposp`) of the
/// request, relative to the directory `fhp` names, and does it.
///
/// The name is copied into a `namei_pool` buffer (a NUL or a `/` in it is `EACCES`), the
/// cursor moves past it and its XDR padding, and the directory is found with
/// [`nfsrv_fhtovp`] (`ENOTDIR` unless it is one). The directory goes to `*retdirp` (the
/// caller `vrele`s it, even when this fails). The lookup does not cross mount points, and is
/// read-only for a read-only export. A symbolic link at the end is `EINVAL`. The buffer is
/// given back, except when the caller asked for `SAVENAME` or `SAVESTART`: then it stays in
/// `cn_pnbuf` with `HASBUF` set.
///
/// `ndp.ni_cnd.cn_cred` is the credentials `nfsrv_fhtovp` maps for the export (the caller's
/// `nd_cr`). The error is the C's `int`: an errno or an NFS status.
#[allow(clippy::too_many_arguments)] // the C signature
pub fn nfs_namei(
    ndp: &mut Nameidata<'_>,
    fhp: &Fhandle,
    len: usize,
    slp: &NfssvcSock,
    nam: &Mbuf,
    mdp: &mut Option<&'static Mbuf>,
    dposp: &mut *mut u8,
    retdirp: &mut Option<&'static Vnode>,
    p: &Proc,
) -> Result<(), i32> {
    *retdirp = None;
    let Some(buf) = pool_get(&NAMEI_POOL, PR_WAITOK) else {
        // PR_WAITOK cannot sleep yet (subr_pool.rs): the C cannot fail here.
        return Err(Errno::ENOMEM.as_i32());
    };
    ndp.ni_cnd.cn_pnbuf = buf.as_ptr();

    let r = namei_lookup(ndp, buf, fhp, len, slp, nam, mdp, dposp, retdirp, p);

    // Check for saved name request
    if r.is_ok() && ndp.ni_cnd.cn_flags & (SAVENAME | SAVESTART) != 0 {
        ndp.ni_cnd.cn_flags |= HASBUF;
        return Ok(());
    }
    // out:
    pool_put(&NAMEI_POOL, buf);
    ndp.ni_cnd.cn_pnbuf = ptr::null_mut();
    r
}

/// The part of `nfs_namei` before its `out:` label: everything that can fail, with the buffer
/// `buf` (`cn_pnbuf`) still the caller's to give back.
#[allow(clippy::too_many_arguments)] // nfs_namei's, split
fn namei_lookup(
    ndp: &mut Nameidata<'_>,
    buf: NonNull<u8>,
    fhp: &Fhandle,
    len: usize,
    slp: &NfssvcSock,
    nam: &Mbuf,
    mdp: &mut Option<&'static Mbuf>,
    dposp: &mut *mut u8,
    retdirp: &mut Option<&'static Vnode>,
    p: &Proc,
) -> Result<(), i32> {
    if len >= MAXPATHLEN {
        return Err(Errno::ENAMETOOLONG.as_i32());
    }
    // Copy the name from the mbuf list to ndp->ni_pnbuf and set the various ndp fields
    // appropriately.
    //
    // SAFETY: `buf` is a fresh `MAXPATHLEN`-byte `namei_pool` item that only this function
    // uses until `nfs_namei` gives it back.
    let pn = unsafe { core::slice::from_raw_parts_mut(buf.as_ptr(), MAXPATHLEN) };
    let mut n = 0;
    while n < len {
        let chunk = nfsm_nextbytes(mdp, dposp, len - n).map_err(Errno::as_i32)?;
        let bytes = chunk.bytes();
        if bytes.iter().any(|&c| c == 0 || c == b'/') {
            return Err(Errno::EACCES.as_i32());
        }
        pn[n..n + bytes.len()].copy_from_slice(bytes);
        n += bytes.len();
    }
    pn[len] = 0;
    let pad = nfsm_padlen(len);
    if pad > 0 {
        nfs_adv(mdp, dposp, pad).map_err(Errno::as_i32)?;
    }
    ndp.ni_pathlen = len;
    ndp.ni_cnd.cn_nameptr = buf.as_ptr();

    // Extract and set starting directory.
    let (dp, rdonly) = nfsrv_fhtovp(fhp, false, ndp.ni_cnd.cred(), slp, nam)?;
    if dp.v_type.get() != crate::sys::vnode::VDIR {
        vrele(dp);
        return Err(Errno::ENOTDIR.as_i32());
    }
    vref(dp);
    *retdirp = Some(dp);
    ndp.ni_startdir = Some(dp);
    if rdonly {
        ndp.ni_cnd.cn_flags |= NOCROSSMOUNT | RDONLY;
    } else {
        ndp.ni_cnd.cn_flags |= NOCROSSMOUNT;
    }

    // And call lookup() to do the real work
    ndp.ni_cnd.cn_proc = p;
    vfs_lookup(ndp).map_err(Errno::as_i32)?;

    // Check for encountering a symbolic link
    if ndp.ni_cnd.cn_flags & ISSYMLINK != 0 {
        if let Some(dvp) = ndp.ni_dvp {
            if ndp.ni_cnd.cn_flags & LOCKPARENT != 0 && ndp.ni_pathlen == 1 {
                vput(dvp);
            } else {
                vrele(dvp);
            }
        }
        if let Some(vp) = ndp.ni_vp.take() {
            vput(vp);
        }
        return Err(Errno::EINVAL.as_i32());
    }
    Ok(())
}

/// `nfsm_adj(mp, len, nul)`: a fiddled version of `m_adj()` that ensures null fill to a long
/// boundary and only trims off the back end: removes `len` bytes from the end of the chain
/// `mp` and zeroes the last `nul` bytes that are left.
pub fn nfsm_adj(mp: &Mbuf, len: i32, nul: i32) {
    // Trim from tail. Scan the mbuf chain, calculating its length and finding the last mbuf.
    // If the adjustment only affects this mbuf, then just adjust and return. Otherwise,
    // rescan and truncate after the remaining size.
    let mut count: i32 = 0;
    let mut m = mp;
    loop {
        count = count.wrapping_add(m.m_len().get() as i32);
        match m.m_next().get() {
            Some(next) => m = next,
            None => break,
        }
    }
    if m.m_len().get() as i32 > len {
        m.m_len().set((m.m_len().get() as i32 - len) as u32);
        nul_fill(m, nul);
        return;
    }
    count = count.wrapping_sub(len);
    if count < 0 {
        count = 0;
    }
    // Correct length for chain is "count". Find the mbuf with last data, adjust its length,
    // and toss data from remaining mbufs on chain.
    let mut cur = Some(mp);
    let mut last = mp;
    while let Some(m) = cur {
        last = m;
        if m.m_len().get() as i32 >= count {
            m.m_len().set(count as u32);
            nul_fill(m, nul);
            break;
        }
        count -= m.m_len().get() as i32;
        cur = m.m_next().get();
    }
    let mut rest = last.m_next().get();
    while let Some(m) = rest {
        m.m_len().set(0);
        rest = m.m_next().get();
    }
}

/// The null fill of `nfsm_adj`: the last `nul` bytes of `m`'s data, clamped to the data.
fn nul_fill(m: &Mbuf, nul: i32) {
    if nul <= 0 {
        return;
    }
    let mlen = m.m_len().get() as usize;
    let nul = (nul as usize).min(mlen);
    // SAFETY: `[mlen - nul, mlen)` is inside the `m_len` bytes of data of `m`, which the
    // caller owns (it is a reply being built), and nothing else reads them meanwhile.
    unsafe { mtod::<u8>(m).add(mlen - nul).write_bytes(0, nul) };
}

/// `nfsm_srvwcc(nfsd, before_ret, before_vap, after_ret, after_vap, mb)`: appends the NFSv3
/// weak cache consistency data (`wcc_data`) to the reply at the build cursor `mb`: the
/// pre-operation attributes (size, mtime, ctime; `None` when they could not be fetched) and
/// the post-operation attributes. Non-inline, so that the kernel text size does not get too
/// big.
pub fn nfsm_srvwcc(
    nfsd: &NfsrvDescript,
    before: Option<&Vattr>,
    after: Option<&Vattr>,
    mb: &mut &'static Mbuf,
) {
    match before {
        None => {
            nfsm_build(mb, NFSX_UNSIGNED).set(0, nfs_false);
        }
        Some(before_vap) => {
            let mut tl = nfsm_build(mb, 7 * NFSX_UNSIGNED);
            tl.set(0, nfs_true);
            tl.txdr_hyper(1, before_vap.va_size);
            tl.write(12, &txdr_nfsv3time(&before_vap.va_mtime));
            tl.write(20, &txdr_nfsv3time(&before_vap.va_ctime));
        }
    }
    nfsm_srvpostop_attr(nfsd, after, mb);
}

/// `nfsm_srvpostop_attr(nfsd, after_ret, after_vap, mb)`: appends an NFSv3 `post_op_attr`:
/// the attributes when `after` has them (`None` is the C's non-zero `after_ret`).
pub fn nfsm_srvpostop_attr(nfsd: &NfsrvDescript, after: Option<&Vattr>, mb: &mut &'static Mbuf) {
    match after {
        None => {
            nfsm_build(mb, NFSX_UNSIGNED).set(0, nfs_false);
        }
        Some(after_vap) => {
            let mut tl = nfsm_build(mb, NFSX_UNSIGNED + NFSX_V3FATTR);
            tl.set(0, nfs_true);
            tl.write(NFSX_UNSIGNED, &nfsm_srvfattr(nfsd, after_vap));
        }
    }
}

/// `nfsm_srvfattr(nfsd, vap, fp)`: the `fattr` (version 3 when the request is) of `vap`, in
/// wire order. Version 2 only fills the words of its shorter structure.
pub fn nfsm_srvfattr(nfsd: &NfsrvDescript, vap: &Vattr) -> NfsFattr {
    let mut fp = NfsFattr {
        fa_nlink: txdr_unsigned(vap.va_nlink),
        fa_uid: txdr_unsigned(vap.va_uid),
        fa_gid: txdr_unsigned(vap.va_gid),
        ..NfsFattr::default()
    };
    if nfsd.nd_flag & ND_NFSV3 != 0 {
        fp.fa_type = vtonfsv3_type(vap.va_type);
        fp.fa_mode = vtonfsv3_mode(vap.va_mode);
        fp.set_fa3_size(Nfsuint64::from_words(txdr_hyper(vap.va_size)));
        fp.set_fa3_used(Nfsuint64::from_words(txdr_hyper(vap.va_bytes)));
        fp.set_fa3_rdev(Nfsv3Spec {
            specdata1: txdr_unsigned(major(vap.va_rdev)),
            specdata2: txdr_unsigned(minor(vap.va_rdev)),
        });
        fp.set_fa3_fsid(Nfsuint64::from_words([
            0,
            txdr_unsigned(vap.va_fsid as u32),
        ]));
        fp.set_fa3_fileid(Nfsuint64::from_words(txdr_hyper(vap.va_fileid)));
        fp.set_fa3_atime(txdr_nfsv3time(&vap.va_atime));
        fp.set_fa3_mtime(txdr_nfsv3time(&vap.va_mtime));
        fp.set_fa3_ctime(txdr_nfsv3time(&vap.va_ctime));
    } else {
        fp.fa_type = vtonfsv2_type(vap.va_type);
        fp.fa_mode = vtonfsv2_mode(vap.va_type, vap.va_mode);
        fp.set_fa2_size(txdr_unsigned(vap.va_size as u32));
        fp.set_fa2_blocksize(txdr_unsigned(vap.va_blocksize as u32));
        fp.set_fa2_rdev(if vap.va_type == VFIFO {
            0xffff_ffff
        } else {
            txdr_unsigned(vap.va_rdev as u32)
        });
        fp.set_fa2_blocks(txdr_unsigned((vap.va_bytes / NFS_FABLKSIZE) as u32));
        fp.set_fa2_fsid(txdr_unsigned(vap.va_fsid as u32));
        fp.set_fa2_fileid(txdr_unsigned(vap.va_fileid as u32));
        fp.set_fa2_atime(txdr_nfsv2time(&vap.va_atime));
        fp.set_fa2_mtime(txdr_nfsv2time(&vap.va_mtime));
        fp.set_fa2_ctime(txdr_nfsv2time(&vap.va_ctime));
    }
    fp
}

/// `nfsrv_fhtovp(fhp, lockflag, vpp, cred, slp, nam, rdonlyp)`: converts a file handle to a
/// vnode (locked when `lockflag`) and says whether its export is read-only.
///
/// - looks the file system id up in the mount list (`ESTALE` when it is not there);
/// - gets the export rights by calling `VFS_CHECKEXP()` and the vnode by `VFS_FHTOVP()`;
/// - refuses clients that do not come from a reserved port (`NFSERR_AUTHERR |
///   AUTH_TOOWEAK`), and on a stream socket the ftp-data port 20;
/// - if `cred`'s uid is 0 or the export maps everyone (`MNT_EXPORTANON`), sets `cred`'s uid,
///   gid and groups to the export's anonymous credentials;
/// - unlocks the vnode unless `lockflag`.
///
/// The error is the C's `int`: an errno or an NFS status.
pub fn nfsrv_fhtovp(
    fhp: &Fhandle,
    lockflag: bool,
    cred: &Ucred,
    slp: &NfssvcSock,
    nam: &Mbuf,
) -> Result<(&'static Vnode, bool), i32> {
    let Some(mp) = vfs_getvfs(&fhp.fh_fsid) else {
        return Err(Errno::ESTALE.as_i32());
    };
    let mut exflags = 0;
    let mut credanon: *const Ucred = ptr::null();
    VFS_CHECKEXP(mp, nam, &mut exflags, &mut credanon).map_err(Errno::as_i32)?;
    let vp = VFS_FHTOVP(mp, &fhp.fh_fid).map_err(Errno::as_i32)?;

    let saddr = sockaddr_in_of(nam);
    if let Some(saddr) = saddr {
        let port = u16::from_be(saddr.sin_port);
        let stream = slp
            .ns_so
            .get()
            .is_some_and(|so| so.so_type.get() == SOCK_STREAM);
        if saddr.sin_family == AF_INET
            && (i32::from(port) >= IPPORT_RESERVED || (stream && port == 20))
        {
            vput(vp);
            return Err(NFSERR_AUTHERR | AUTH_TOOWEAK as i32);
        }
    } else {
        // Shorter than a sockaddr_in: not an address of a client we can vouch for.
        vput(vp);
        return Err(NFSERR_AUTHERR | AUTH_TOOWEAK as i32);
    }

    // Check/setup credentials.
    if cred.cr_uid.get() == 0 || exflags & MNT_EXPORTANON != 0 {
        // SAFETY: `VFS_CHECKEXP` stored the address of the `netc_anon` of the export entry it
        // matched (`ufs_check_export` & co.), which lives as long as the export list does;
        // the list is not changed while this request is served (single kernel lock).
        let Some(anon) = (unsafe { credanon.as_ref() }) else {
            vput(vp);
            return Err(Errno::EACCES.as_i32());
        };
        cred.cr_uid.set(anon.cr_uid.get());
        cred.cr_gid.set(anon.cr_gid.get());
        let mut i = 0;
        while i < anon.cr_ngroups.get() as usize && i < NGROUPS_MAX {
            cred.cr_groups[i].set(anon.cr_groups[i].get());
            i += 1;
        }
        cred.cr_ngroups.set(i as i16);
    }
    let rdonly = exflags & MNT_EXRDONLY != 0;
    if !lockflag {
        let _ = VOP_UNLOCK(vp);
    }

    Ok((vp, rdonly))
}

/// The `struct sockaddr_in` at the start of the address mbuf `nam` (`mtod(nam, struct
/// sockaddr_in *)`), copied out; `None` when the mbuf is shorter.
pub(crate) fn sockaddr_in_of(nam: &Mbuf) -> Option<SockaddrIn> {
    if (nam.m_len().get() as usize) < size_of::<SockaddrIn>() {
        return None;
    }
    // SAFETY: the mbuf holds at least `size_of::<SockaddrIn>()` bytes of data (checked), and
    // `SockaddrIn` is plain integers, valid for any bit pattern; the copy is unaligned.
    Some(unsafe { ptr::read_unaligned(mtod::<SockaddrIn>(nam)) })
}

/// `netaddr_match(family, haddr, nam)`: compares two net addresses by family and returns
/// whether they are the same host, or false if there is any doubt. The `AF_INET` family is
/// handled as a special case so that address mbufs do not need to be saved to store a
/// `struct in_addr`, which is only 4 bytes.
pub fn netaddr_match(family: SaFamily, haddr: &Nethostaddr, nam: &Mbuf) -> bool {
    match family {
        AF_INET => sockaddr_in_of(nam).is_some_and(|inetaddr| {
            inetaddr.sin_family == AF_INET && inetaddr.sin_addr.s_addr == haddr.had_inetaddr
        }),
        _ => false,
    }
}

/// `nfsm_srvsattr(nfsd, va)`: decodes an NFSv3 `sattr3` of the request into `va`: the mode,
/// uid, gid and size when the request sets them, then the access and modification times
/// (set to the client's value, to the server's, or left alone). Frees the request on
/// failure, as every `nfsd_dissect`.
pub fn nfsm_srvsattr(nfsd: &mut NfsrvDescript, va: &mut Vattr) -> Result<(), Errno> {
    if nfsd_dissect(nfsd, NFSX_UNSIGNED)?.get(0) == nfs_true {
        let tl = nfsd_dissect(nfsd, NFSX_UNSIGNED)?;
        va.va_mode = nfstov_mode(tl.get(0));
    }

    if nfsd_dissect(nfsd, NFSX_UNSIGNED)?.get(0) == nfs_true {
        let tl = nfsd_dissect(nfsd, NFSX_UNSIGNED)?;
        va.va_uid = fxdr_unsigned(tl.get(0));
    }

    if nfsd_dissect(nfsd, NFSX_UNSIGNED)?.get(0) == nfs_true {
        let tl = nfsd_dissect(nfsd, NFSX_UNSIGNED)?;
        va.va_gid = fxdr_unsigned(tl.get(0));
    }

    if nfsd_dissect(nfsd, NFSX_UNSIGNED)?.get(0) == nfs_true {
        let tl = nfsd_dissect(nfsd, 2 * NFSX_UNSIGNED)?;
        va.va_size = tl.fxdr_hyper(0);
    }

    srvsattr_time(nfsd, va, true)?;
    srvsattr_time(nfsd, va, false)?;

    Ok(())
}

/// One of the two `set_atime`/`set_mtime` discriminated unions of `sattr3`: the access time
/// (`atime`) or the modification time.
fn srvsattr_time(nfsd: &mut NfsrvDescript, va: &mut Vattr, atime: bool) -> Result<(), Errno> {
    let how = fxdr_unsigned(nfsd_dissect(nfsd, NFSX_UNSIGNED)?.get(0));
    let slot: &mut Timespec = if atime {
        &mut va.va_atime
    } else {
        &mut va.va_mtime
    };
    match how {
        NFSV3SATTRTIME_TOCLIENT => {
            va.va_vaflags |= VA_UTIMES_CHANGE;
            va.va_vaflags &= !VA_UTIMES_NULL;
            let tl = nfsd_dissect(nfsd, 2 * NFSX_UNSIGNED)?;
            let t: Nfsv3Time = tl.read(0);
            *slot = fxdr_nfsv3time(&t);
        }
        NFSV3SATTRTIME_TOSERVER => {
            va.va_vaflags |= VA_UTIMES_CHANGE;
            *slot = getnanotime();
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests;
