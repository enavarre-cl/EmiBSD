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

/* <CODE> */
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
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for `nfs_srvsubs.c`: `nfsm_adj`, the reply builders (`nfsm_srvwcc`,
    // `nfsm_srvpostop_attr`, `nfsm_srvfattr`), `nfsm_srvsattr`, `netaddr_match`, and
    // `nfs_namei`/`nfsrv_fhtovp` against a real exported ffs mount.

    use std::vec::Vec;

    use super::*;
    use crate::kern::uipc_mbuf::tests::setup as mbuf_setup;
    use crate::kern::uipc_mbuf::{m_freem, m_get};
    use crate::nfs::nfs_subs::nfsm_reqhead;
    use crate::nfs::nfs_subs::tests::{bytes, chain, lens};
    use crate::nfs::nfsm_subs::nfsm_avail;
    use crate::nfs::nfsproto::NFSV3SATTRTIME_DONTCHANGE;
    use crate::nfs::xdr_subs::{fxdr_hyper, xdr_bytes};
    use crate::sys::mbuf::{M_DONTWAIT, MT_SONAME};
    use crate::sys::types::makedev;
    use crate::sys::vnode::{VCHR, VDIR, VLNK, VREG, Vtype};

    /// The words of `b` (a reply), as raw XDR words in network order.
    fn words(b: &[u8]) -> Vec<u32> {
        b.chunks(4)
            .map(|c| u32::from_ne_bytes([c[0], c[1], c[2], c[3]]))
            .collect()
    }

    /// The value of the raw word `w`.
    fn v(w: u32) -> u32 {
        fxdr_unsigned(w)
    }

    /// A request descriptor positioned at the start of `m`.
    fn request(m: &'static Mbuf, v3: bool) -> NfsrvDescript {
        let mut nd = NfsrvDescript::new();
        nd.nd_mrep = Some(m);
        nd.nd_md = Some(m);
        nd.nd_dpos = mtod::<u8>(m);
        if v3 {
            nd.nd_flag |= ND_NFSV3;
        }
        nd
    }

    /// An `MT_SONAME` mbuf holding the `AF_INET` address `a`, port `port` (host order).
    fn nam(a: [u8; 4], port: u16) -> &'static Mbuf {
        let m = m_get(M_DONTWAIT, MT_SONAME).expect("an mbuf");
        let mut sin = [0u8; 16];
        sin[0] = 16;
        sin[1] = AF_INET;
        sin[2..4].copy_from_slice(&port.to_be_bytes());
        sin[4..8].copy_from_slice(&a);
        // SAFETY: an mbuf's data area holds 16 bytes; `mtod` points at it.
        unsafe { ptr::copy_nonoverlapping(sin.as_ptr(), mtod::<u8>(m), 16) };
        m.m_len().set(16);
        m
    }

    #[test]
    fn adj_trims_inside_the_last_mbuf_and_zero_fills() {
        let _g = mbuf_setup();
        let head = chain(&[&[1, 2, 3, 4], &[5, 6, 7, 8, 9, 10]]);
        nfsm_adj(head, 2, 0);
        assert_eq!(lens(head), [4, 4]);
        assert_eq!(bytes(head), [1, 2, 3, 4, 5, 6, 7, 8]);
        // The last `nul` bytes that are left are zeroed.
        nfsm_adj(head, 1, 2);
        assert_eq!(bytes(head), [1, 2, 3, 4, 5, 0, 0]);
        // Nothing to fill.
        nfsm_adj(head, 1, -1);
        assert_eq!(bytes(head), [1, 2, 3, 4, 5, 0]);
        m_freem(head);
    }

    #[test]
    fn adj_trims_across_mbufs() {
        let _g = mbuf_setup();
        let head = chain(&[&[1, 2, 3, 4], &[5, 6], &[7, 8, 9]]);
        // 9 bytes, trim 5: 4 are left, all in the first mbuf; the others are emptied.
        nfsm_adj(head, 5, 0);
        assert_eq!(lens(head), [4, 0, 0]);
        assert_eq!(bytes(head), [1, 2, 3, 4]);
        m_freem(head);

        let head = chain(&[&[1, 2, 3, 4], &[5, 6], &[7, 8, 9]]);
        // Trim 4: 5 are left, ending inside the second mbuf; the null fill lands in it.
        nfsm_adj(head, 4, 1);
        assert_eq!(lens(head), [4, 1, 0]);
        assert_eq!(bytes(head), [1, 2, 3, 4, 0]);
        m_freem(head);

        // Trimming everything or more leaves an empty chain.
        let head = chain(&[&[1, 2], &[3]]);
        nfsm_adj(head, 10, 2);
        assert_eq!(lens(head), [0, 0]);
        m_freem(head);

        // A fill larger than the data is clamped to it.
        let head = chain(&[&[1, 2, 3, 4]]);
        nfsm_adj(head, 2, 8);
        assert_eq!(bytes(head), [0, 0]);
        m_freem(head);
    }

    /// A `Vattr` with distinct values everywhere.
    fn attrs(t: Vtype) -> Vattr {
        let mut va = Vattr::new();
        va.va_type = t;
        va.va_mode = 0o644;
        va.va_nlink = 2;
        va.va_uid = 3;
        va.va_gid = 4;
        va.va_fsid = 7;
        va.va_fileid = 0x1_0000_0063;
        va.va_size = 0x1_0000_0002;
        va.va_blocksize = 16384;
        va.va_atime = Timespec::new(100, 1_000);
        va.va_mtime = Timespec::new(200, 2_000);
        va.va_ctime = Timespec::new(300, 3_000);
        va.va_rdev = makedev(5, 6);
        va.va_bytes = 8192;
        va
    }

    #[test]
    fn srvfattr_version_3() {
        let nd = NfsrvDescript {
            nd_flag: ND_NFSV3,
            ..NfsrvDescript::new()
        };
        let fp = nfsm_srvfattr(&nd, &attrs(VCHR));
        assert_eq!(v(fp.fa_type), 4, "NFCHR");
        assert_eq!(v(fp.fa_mode), 0o644);
        assert_eq!((v(fp.fa_nlink), v(fp.fa_uid), v(fp.fa_gid)), (2, 3, 4));
        assert_eq!(fxdr_hyper(fp.fa3_size().words()), 0x1_0000_0002);
        assert_eq!(fxdr_hyper(fp.fa3_used().words()), 8192);
        assert_eq!(
            (v(fp.fa3_rdev().specdata1), v(fp.fa3_rdev().specdata2)),
            (5, 6)
        );
        assert_eq!(fp.fa3_fsid().words(), [0, txdr_unsigned(7)]);
        assert_eq!(fxdr_hyper(fp.fa3_fileid().words()), 0x1_0000_0063);
        let (a, m, c) = (fp.fa3_atime(), fp.fa3_mtime(), fp.fa3_ctime());
        assert_eq!((v(a.nfsv3_sec), v(a.nfsv3_nsec)), (100, 1_000));
        assert_eq!((v(m.nfsv3_sec), v(m.nfsv3_nsec)), (200, 2_000));
        assert_eq!((v(c.nfsv3_sec), v(c.nfsv3_nsec)), (300, 3_000));
    }

    #[test]
    fn srvfattr_version_2() {
        let nd = NfsrvDescript::new();
        let fp = nfsm_srvfattr(&nd, &attrs(VREG));
        assert_eq!(v(fp.fa_type), 1, "NFREG");
        assert_eq!(v(fp.fa_mode), 0o100644, "a version 2 mode carries the type");
        assert_eq!(v(fp.fa2_size()), 2, "the size is truncated to 32 bits");
        assert_eq!(v(fp.fa2_blocksize()), 16384);
        assert_eq!(v(fp.fa2_rdev()), makedev(5, 6) as u32);
        assert_eq!(v(fp.fa2_blocks()), 8192 / 512);
        assert_eq!((v(fp.fa2_fsid()), v(fp.fa2_fileid())), (7, 0x63));
        assert_eq!(v(fp.fa2_mtime().nfsv2_sec), 200);
        assert_eq!(v(fp.fa2_mtime().nfsv2_usec), 2);
        // A fifo's rdev is all ones.
        let fp = nfsm_srvfattr(&nd, &attrs(VFIFO));
        assert_eq!(fp.fa2_rdev(), 0xffff_ffff);
        assert_eq!(v(fp.fa_type), 4, "a version 2 fifo is NFCHR");
    }

    #[test]
    fn srvpostop_attr_and_wcc_layouts() {
        let _g = mbuf_setup();
        let nd = NfsrvDescript {
            nd_flag: ND_NFSV3,
            ..NfsrvDescript::new()
        };

        // No attributes: a false word.
        let head = nfsm_reqhead(0);
        let mut mb = head;
        nfsm_srvpostop_attr(&nd, None, &mut mb);
        assert_eq!(words(&bytes(head)), [nfs_false]);
        m_freem(head);

        // Attributes: a true word and the 84-byte fattr.
        let head = nfsm_reqhead(0);
        let mut mb = head;
        let va = attrs(VREG);
        nfsm_srvpostop_attr(&nd, Some(&va), &mut mb);
        let b = bytes(head);
        assert_eq!(b.len(), NFSX_UNSIGNED + NFSX_V3FATTR);
        assert_eq!(words(&b)[0], nfs_true);
        assert_eq!(&b[4..], xdr_bytes(&nfsm_srvfattr(&nd, &va)));
        m_freem(head);

        // wcc_data with both halves: 7 words of pre-op attributes, then the post-op attributes.
        let head = nfsm_reqhead(0);
        let mut mb = head;
        let mut before = Vattr::new();
        before.va_size = 0x5_0000_0006;
        before.va_mtime = Timespec::new(11, 12);
        before.va_ctime = Timespec::new(13, 14);
        nfsm_srvwcc(&nd, Some(&before), Some(&va), &mut mb);
        let b = bytes(head);
        assert_eq!(b.len(), 7 * 4 + NFSX_UNSIGNED + NFSX_V3FATTR);
        let w = words(&b);
        assert_eq!(w[0], nfs_true);
        assert_eq!(fxdr_hyper([w[1], w[2]]), 0x5_0000_0006);
        assert_eq!([v(w[3]), v(w[4]), v(w[5]), v(w[6])], [11, 12, 13, 14]);
        assert_eq!(w[7], nfs_true);
        m_freem(head);

        // Neither: two false words.
        let head = nfsm_reqhead(0);
        let mut mb = head;
        nfsm_srvwcc(&nd, None, None, &mut mb);
        assert_eq!(words(&bytes(head)), [nfs_false, nfs_false]);
        m_freem(head);
    }

    /// An `sattr3` request: every combination of the optional members.
    #[test]
    fn srvsattr_decodes_every_member() {
        let _g = mbuf_setup();
        let t = txdr_unsigned;
        let mut b: Vec<u8> = Vec::new();
        for w in [
            nfs_true,
            t(0o10640), // mode (the type bits are masked off)
            nfs_false,  // uid not set
            nfs_true,
            t(5), // gid
            nfs_true,
            t(1),
            t(0x2_0000), // size 0x1_0002_0000: high word 1, low word 0x20000
            t(NFSV3SATTRTIME_TOCLIENT),
            t(100),
            t(5), // atime to the client's value
            t(NFSV3SATTRTIME_TOSERVER),
        ] {
            b.extend_from_slice(&w.to_ne_bytes());
        }
        // Split in the middle of a word and of a time to exercise the straddling dissects.
        let (a, c) = b.split_at(30);
        let head = chain(&[a, c]);
        let mut nd = request(head, true);
        let mut va = Vattr::new();
        va.va_vaflags = VA_UTIMES_NULL;
        va.va_uid = 77;
        nfsm_srvsattr(&mut nd, &mut va).expect("decoded");
        assert_eq!(va.va_mode, 0o640);
        assert_eq!(va.va_uid, 77, "left alone");
        assert_eq!(va.va_gid, 5);
        assert_eq!(va.va_size, 0x1_0002_0000);
        assert_eq!(va.va_atime, Timespec::new(100, 5));
        assert_eq!(va.va_vaflags, VA_UTIMES_CHANGE, "NULL cleared, CHANGE set");
        assert!(nd.nd_mrep.is_some());
        m_freem(head);

        // DONTCHANGE for both times: nothing is touched.
        let mut b: Vec<u8> = Vec::new();
        for w in [nfs_false, nfs_false, nfs_false, nfs_false] {
            b.extend_from_slice(&w.to_ne_bytes());
        }
        for _ in 0..2 {
            b.extend_from_slice(&txdr_unsigned(NFSV3SATTRTIME_DONTCHANGE).to_ne_bytes());
        }
        let head = chain(&[&b]);
        let mut nd = request(head, true);
        let mut va = Vattr::new();
        va.va_vaflags = VA_UTIMES_NULL;
        nfsm_srvsattr(&mut nd, &mut va).expect("decoded");
        assert_eq!(va.va_vaflags, VA_UTIMES_NULL);
        assert_eq!(va.va_atime, Timespec::new(0, 0));
        m_freem(head);
    }

    #[test]
    fn srvsattr_frees_a_short_request() {
        let _g = mbuf_setup();
        let t = txdr_unsigned;
        let mut b: Vec<u8> = Vec::new();
        for w in [nfs_true, t(0o644), nfs_false] {
            b.extend_from_slice(&w.to_ne_bytes());
        }
        let head = chain(&[&b]);
        let mut nd = request(head, true);
        let mut va = Vattr::new();
        assert_eq!(nfsm_srvsattr(&mut nd, &mut va), Err(Errno::EBADRPC));
        assert!(nd.nd_mrep.is_none(), "the request is freed");
        assert_eq!(
            va.va_mode, 0o644,
            "what was decoded before the failure stays"
        );
    }

    #[test]
    fn netaddr_match_compares_inet_addresses() {
        let _g = mbuf_setup();
        let m = nam([10, 0, 2, 9], 700);
        let host = |a: [u8; 4]| Nethostaddr {
            had_inetaddr: u32::from_ne_bytes(a),
            had_nam: None,
        };
        assert!(netaddr_match(AF_INET, &host([10, 0, 2, 9]), m));
        assert!(!netaddr_match(AF_INET, &host([10, 0, 2, 8]), m));
        // Any other family is "if there is any doubt, 0".
        assert!(!netaddr_match(0, &host([10, 0, 2, 9]), m));
        assert!(!netaddr_match(24, &host([10, 0, 2, 9]), m));
        // An address mbuf of another family never matches.
        // SAFETY: byte 1 of the address mbuf is `sin_family`.
        unsafe { mtod::<u8>(m).add(1).write(24) };
        assert!(!netaddr_match(AF_INET, &host([10, 0, 2, 9]), m));
        m_freem(m);
    }

    /// `nfs_namei` and `nfsrv_fhtovp` over an exported ffs: the name is copied out of a request
    /// chain (across mbufs), the directory comes from the file handle, the lookup stays inside
    /// it, the export maps root to the anonymous user, and every refusal has the C's error.
    #[test]
    fn namei_and_fhtovp_over_an_exported_ffs() {
        use crate::kern::kern_descrip::sys_close;
        use crate::kern::uipc_mbuf::tests::mbinit_again;
        use crate::kern::vfs_lookup::{namei, ndinit};
        use crate::kern::vfs_subr::tests::exports::{args, sin};
        use crate::kern::vfs_syscalls::{sys_mkdir, sys_mount, sys_open, sys_symlink};
        use crate::sys::fcntl::{O_CREAT, O_RDWR};
        use crate::sys::mount::{MNT_EXPORTED, MNT_EXRDONLY, MNT_UPDATE, UfsArgs, VFS_VPTOFH};
        use crate::sys::namei::{FOLLOW, LOCKLEAF, LOOKUP, NiDirp};
        use crate::ufs::ffs::ffs_vfsops::tests::{
            mount_root, newfs, path, setup, sys, teardown, unmount_root,
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
        sys(sys_close, p, &[fd as usize]).unwrap();
        let fd = sys(
            sys_open,
            p,
            &[path(b"/file\0"), (O_CREAT | O_RDWR) as usize, 0o644],
        )
        .unwrap();
        sys(sys_close, p, &[fd as usize]).unwrap();
        sys(sys_symlink, p, &[path(b"f\0"), path(b"/dir/ln\0")]).unwrap();

        // The file handle of `name`.
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
        let filefh = fh(b"/file");

        // mountd(8): export it read-only to 10.0.2.0/24, everyone else as user 32767.
        let net = sin(2, [10, 0, 2, 0]);
        let mask = sin(2, [255, 255, 255, 0]);
        let mut ua = UfsArgs {
            fspec: 0,
            export_info: args(MNT_EXPORTED | MNT_EXRDONLY, 32767, Some(net), Some(mask)),
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

        let slp = NfssvcSock::new();
        let client = nam([10, 0, 2, 9], 700);
        let cred = nd_cred_root();
        let weak = AUTH_TOOWEAK as i32;

        // Looks `name` up in `fh` as the client at `from`, the name in `parts` (a request chain).
        let run = |fhp: &Fhandle,
                   name_len: usize,
                   parts: &[&[u8]],
                   from: &Mbuf,
                   flags: u64|
         -> (Result<(), i32>, Nameidata<'static>, Option<&'static Vnode>) {
            let head = chain(parts);
            let mut md = Some(head);
            let mut dpos = mtod::<u8>(head);
            let mut nd = ndinit(LOOKUP, flags, NiDirp::Sys(b""), p);
            nd.ni_cnd.cn_cred = cred;
            let mut retdir = None;
            let r = nfs_namei(
                &mut nd,
                fhp,
                name_len,
                &slp,
                from,
                &mut md,
                &mut dpos,
                &mut retdir,
                p,
            );
            if r.is_ok() {
                // The cursor is past the name and its padding.
                let m = md.expect("cursor");
                assert_eq!(nfsm_avail(m, dpos), parts_after(name_len, m, head));
            }
            m_freem(head);
            (r, nd, retdir)
        };

        // The name starts in the second mbuf (the first is empty), its padding runs into the
        // third: the lookup finds /dir/f, does not cross mounts, is read-only, and root became
        // user 32767.
        assert_eq!(cred.cr_uid.get(), 0);
        let (r, nd, retdir) = run(&dirfh, 1, &[b"", b"f", b"\0\0\0z"], client, LOCKLEAF);
        assert_eq!(r, Ok(()));
        assert_eq!(cred.cr_uid.get(), 32767);
        assert_eq!(cred.cr_gid.get(), 32767);
        assert_eq!(cred.cr_ngroups.get(), 0);
        let vp = nd.ni_vp.expect("the file");
        assert_eq!(vp.v_type.get(), VREG);
        assert!(nd.ni_cnd.cn_flags & (NOCROSSMOUNT | RDONLY) == (NOCROSSMOUNT | RDONLY));
        assert!(
            nd.ni_cnd.cn_pnbuf.is_null(),
            "the buffer went back to the pool"
        );
        let dir = retdir.expect("the directory");
        assert_eq!(dir.v_type.get(), VDIR);
        vput(vp);
        vrele(dir);

        // A name copied in two chunks, across mbufs: "ln" is a symbolic link. Without FOLLOW
        // (what the server's lookups ask for) the lookup returns the link itself ...
        cred.cr_uid.set(0);
        let (r, nd, retdir) = run(&dirfh, 2, &[b"l", b"n\0\0"], client, LOCKLEAF);
        assert_eq!(r, Ok(()));
        let vp = nd.ni_vp.expect("the link");
        assert_eq!(vp.v_type.get(), VLNK);
        vput(vp);
        vrele(retdir.expect("the directory"));
        // ... and a request that would follow it is EINVAL, nothing left locked or referenced.
        let (r, nd, retdir) = run(&dirfh, 2, &[b"l", b"n\0\0"], client, LOCKLEAF | FOLLOW);
        assert_eq!(r, Err(Errno::EINVAL.as_i32()));
        assert!(nd.ni_vp.is_none());
        vrele(retdir.expect("the directory is still the caller's"));

        // A NUL or a slash in the name is EACCES; the directory has not been looked up yet, so
        // retdir stays None.
        for bad in [&b"a/b\0"[..], &b"a\0b\0"[..]] {
            let (r, nd, retdir) = run(&dirfh, 3, &[bad], client, LOCKLEAF);
            assert_eq!(r, Err(Errno::EACCES.as_i32()));
            assert!(retdir.is_none());
            assert!(nd.ni_cnd.cn_pnbuf.is_null());
        }

        // A chain that ends inside the name: EBADRPC.
        let (r, _, retdir) = run(&dirfh, 8, &[b"ab", b"cd"], client, LOCKLEAF);
        assert_eq!(r, Err(Errno::EBADRPC.as_i32()));
        assert!(retdir.is_none());

        // A name that does not exist: ENOENT, the directory returned for the caller to release.
        let (r, _, retdir) = run(&dirfh, 3, &[b"abc\0"], client, LOCKLEAF);
        assert_eq!(r, Err(Errno::ENOENT.as_i32()));
        vrele(retdir.expect("the directory"));

        // The handle of a regular file is not a directory.
        let (r, _, retdir) = run(&filefh, 1, &[b"x\0\0\0"], client, LOCKLEAF);
        assert_eq!(r, Err(Errno::ENOTDIR.as_i32()));
        assert!(retdir.is_none());

        // A file system that is not mounted: ESTALE.
        let mut stale = dirfh;
        stale.fh_fsid.val[0] ^= 0x5a5a;
        let (r, _, _) = run(&stale, 1, &[b"f\0\0\0"], client, LOCKLEAF);
        assert_eq!(r, Err(Errno::ESTALE.as_i32()));

        // A client outside the export list: EACCES from VFS_CHECKEXP.
        let outsider = nam([10, 0, 3, 9], 700);
        let (r, _, _) = run(&dirfh, 1, &[b"f\0\0\0"], outsider, LOCKLEAF);
        assert_eq!(r, Err(Errno::EACCES.as_i32()));

        // A client on an unprivileged port, and on a stream socket from port 20.
        let high = nam([10, 0, 2, 9], 1024);
        let (r, _, _) = run(&dirfh, 1, &[b"f\0\0\0"], high, LOCKLEAF);
        assert_eq!(r, Err(NFSERR_AUTHERR | weak));
        // (A privileged port is the 700 of every success above.)
        let ftp = nam([10, 0, 2, 9], 20);
        let (r, nd, retdir) = run(&dirfh, 1, &[b"f\0\0\0"], ftp, LOCKLEAF);
        assert_eq!(r, Ok(()), "datagram sockets may use port 20");
        vput(nd.ni_vp.expect("the file"));
        vrele(retdir.expect("the directory"));

        // SAVENAME keeps the pathname buffer for the caller.
        let (r, nd, retdir) = run(&dirfh, 1, &[b"f\0\0\0"], client, LOCKLEAF | SAVENAME);
        assert_eq!(r, Ok(()));
        assert!(nd.ni_cnd.cn_flags & HASBUF != 0);
        assert!(!nd.ni_cnd.cn_pnbuf.is_null());
        // The name's length (no NUL, unlike namei's), less what the lookup consumed.
        assert_eq!(nd.ni_pathlen, 0);
        pool_put(
            &NAMEI_POOL,
            NonNull::new(nd.ni_cnd.cn_pnbuf).expect("a buffer"),
        );
        vput(nd.ni_vp.expect("the file"));
        vrele(retdir.expect("the directory"));

        // nfsrv_fhtovp alone: a non-root caller keeps its credentials; lockflag keeps the lock.
        let user = Ucred::new();
        user.cr_uid.set(1000);
        user.cr_gid.set(1000);
        let (vp, rdonly) = nfsrv_fhtovp(&filefh, true, &user, &slp, client).expect("a vnode");
        assert!(rdonly, "exported read-only");
        assert_eq!((user.cr_uid.get(), user.cr_gid.get()), (1000, 1000));
        vput(vp);
        let (vp, _) = nfsrv_fhtovp(&filefh, false, &user, &slp, client).expect("a vnode");
        vrele(vp);

        // A sockaddr shorter than a sockaddr_in is not vouched for.
        let short = nam([10, 0, 2, 9], 700);
        short.m_len().set(8);
        assert_eq!(
            nfsrv_fhtovp(&filefh, false, &user, &slp, short).err(),
            Some(NFSERR_AUTHERR | weak),
            "the export lookup reads the key from the mbuf's data area, the port check refuses"
        );

        mbinit_again();
        unmount_root(p, mp);
        teardown();
    }

    /// What is left in the mbuf `m` of the request `head` after the name and its padding: the
    /// cursor is `name_len` rounded up to a word bytes from the start of the chain.
    fn parts_after(name_len: usize, m: &Mbuf, head: &Mbuf) -> usize {
        let consumed = name_len + (4 - name_len % 4) % 4;
        // Offset of `m` from the start of the chain.
        let mut off = 0;
        let mut cur = Some(head);
        while let Some(c) = cur {
            if ptr::eq(c, m) {
                break;
            }
            off += c.m_len().get() as usize;
            cur = c.m_next().get();
        }
        off + m.m_len().get() as usize - consumed
    }

    /// Root's credentials for a request: uid 0, gid 0, no groups.
    fn nd_cred_root() -> &'static Ucred {
        std::boxed::Box::leak(std::boxed::Box::new(Ucred::new()))
    }
}
/* </TESTS> */
