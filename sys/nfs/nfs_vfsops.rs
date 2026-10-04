/*	$OpenBSD: nfs_vfsops.c,v 1.136 2026/08/31 22:37:09 jsg Exp $	*/
/*	$NetBSD: nfs_vfsops.c,v 1.46.4.1 1996/05/25 22:40:35 fvdl Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1989, 1993, 1995
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
 *	@(#)nfs_vfsops.c	8.12 (Berkeley) 5/20/95
 */
/* </LICENSES> */

//! The NFS client's file system type: the `vfsops` (`nfs_vfsops`), mounting (`nfs_mount`,
//! `mountnfs`, `nfs_decode_args`, `nfs_unmount`), mounting the root file system and the swap
//! file over the network (`nfs_mountroot`, `nfs_mount_diskless`), `nfs_statfs` and
//! `nfs_fsinfo`, `nfs_sync`, and the `fs.nfs` sysctl (`nfs_sysctl`).
//!
//! Upstream: sys/nfs/nfs_vfsops.c @ 3ce1f3f79392
//!
//! A mount is an `nfsmount` hanging off `mnt_data` and the root vnode of the remote file
//! system (`nm_vnode`), referenced for as long as the mount lives. The sockets and the RPC
//! machinery are `nfs_socket.rs`'s; `nfs_mountroot` boots from the network with `nfs_boot.rs`
//! (RARP, bootparams, mountd) into the [`NFS_DISKLESS`] structure.
//!
//! ## Deviations
//! - `nfs_mount` reads its `struct nfs_args` out of the kernel copy of the mount arguments
//!   (`NfsArgs::from_bytes`), `EINVAL` when they are short (the C dereferences NULL); `args->fh`
//!   and `args->hostname` are user addresses copied in as the C does. `mountnfs` takes the
//!   file handle as a slice (the C reads it through `argp->fh`, which is a kernel pointer for
//!   `nfs_mount_diskless` and the local copy for `nfs_mount`), the host name and the mounted-on
//!   path as byte strings, and returns the root vnode (the C's `*vpp`); its `MNT_UPDATE`
//!   branch, which `nfs_mount` never reaches, returns `Ok(None)`.
//! - `mnt_stat.mount_info.nfs_args` is read and written as a whole `NfsArgs` (`nfsargs_bytes`
//!   builds the bytes field by field, padding zero): `nfs_decode_args` works on a copy.
//! - `nfs_mount_diskless` returns the mount and its root vnode, copies the server address
//!   from `ndm_saddr` (where `ndm_args.addr` points) and panics as the C when the mount fails.
//! - `nfs_mountroot` ignores the swap file's size (it is only printed under `DEBUG`) and the
//!   `#ifdef notyet` swap credentials; `nfs_boot_init` returns a `Result` that is a panic
//!   here, since the C's cannot fail either.
//! - `nfs_sysctl` reads `name[0]` only when there is one; `fs.nfs.nfsstats` is copied out of
//!   and into the counters as the 78 words of `struct nfsstats` (`Nfsstats::snapshot`,
//!   `restore`).
//! - `mountnfs`' failure path also clears `mnt_data`, which the C leaves pointing at the freed
//!   `nfsmount` of a mount about to be freed.
//! - `nfs_statfs` frees its credential on every path (the C does through `nfsmout`).

use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::Ordering::Relaxed;

use libkern::{StaticCell, strlcpy};

use crate::kern::init_main::{set_rootvp, set_swapdev_vp};
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_prot::{crfree, crget};
use crate::kern::kern_synch::{nowake, tsleep_nsec};
use crate::kern::kern_sysctl::sysctl_int;
use crate::kern::kern_time::inittodr;
use crate::kern::kern_timeout::{timeout_add, timeout_del, timeout_set_proc};
use crate::kern::subr_prf::{Str, panic, printf};
use crate::kern::uipc_mbuf::{m_freem, m_get};
use crate::kern::uipc_syscalls::sockargs;
use crate::kern::vfs_subr::{
    MOUNTLIST, bdevvp, copy_statfs_info, vflush, vfs_getnewfsid, vfs_rootmountalloc, vfs_unbusy,
    vget, vgone, vput, vref, vrele,
};
use crate::kern::vfs_vnops::vn_lock;
use crate::kern::vfs_vops::{VOP_FSYNC, VOP_GETATTR, VOP_ISLOCKED, VOP_UNLOCK};
use crate::machine::conf::swapdev;
use crate::machine::copy::{copyin, copyinstr, copyout};
use crate::machine::cpu::curproc;
use crate::machine::intr::{splbio, splx};
use crate::nfs::nfs::{
    NFS_DEFRAHEAD, NFS_DIRBLKSIZ, NFS_MAXATTRTIMO, NFS_MAXGRPS, NFS_MAXRAHEAD, NFS_MAXREXMIT,
    NFS_MINATTRTIMO, NFS_NFSSTATS, NFS_NIOTHREADS, NFS_READDIRSIZE, NFS_RETRANS, NFS_RSIZE,
    NFS_WSIZE, nfs_hz, nfs_maxtimeo, nfs_mintimeo, nfs_niothreads, nfs_timeo,
};
use crate::nfs::nfs_boot::{nfs_boot_getfh, nfs_boot_init};
use crate::nfs::nfs_node::{nfs_nget, nfs_ninit};
use crate::nfs::nfs_socket::{nfs_connect, nfs_disconnect, nfs_request, nfs_timer};
use crate::nfs::nfs_subs::{NFSSTATS, nfs_vfs_init, nfsm_fhtom, nfsm_reqhead};
use crate::nfs::nfs_syscalls::nfs_getset_niothreads;
use crate::nfs::nfsdiskless::{NfsDiskless, NfsDlmount};
use crate::nfs::nfsm_subs::{NfsmInfo, nfsm_dissect, nfsm_postop_attr};
use crate::nfs::nfsmount::{NfsMount, VFSTONFS};
use crate::nfs::nfsnode::NFSTOV;
use crate::nfs::nfsproto::{
    NFS_FABLKSIZE, NFS_MAXDATA, NFS_MAXDGRAMDATA, NFS_V2MAXDATA, NFSPROC_FSINFO, NFSPROC_FSSTAT,
    NFSX_V3FHMAX, NFSX_V3FSINFO, NfsStatfs, Nfsv3Fsinfo, nfsx_fh, nfsx_statfs,
};
use crate::nfs::xdr_subs::{fxdr_hyper, fxdr_unsigned};
use crate::sys::dirent::MAXNAMLEN;
use crate::sys::errno::Errno;
use crate::sys::lock::{LK_EXCLUSIVE, LK_RETRY};
use crate::sys::malloc::{M_NFSMNT, M_WAITOK, M_ZERO};
use crate::sys::mbuf::{M_WAIT, MT_SONAME, Mbuf, mtod};
use crate::sys::mount::{
    Fid, MNAMELEN, MNT_FORCE, MNT_LAZY, MNT_UPDATE, Mount, NFSMNT_ACDIRMAX, NFSMNT_ACDIRMIN,
    NFSMNT_ACREGMAX, NFSMNT_ACREGMIN, NFSMNT_GOTFSINFO, NFSMNT_INTERNAL, NFSMNT_MAXGRPS,
    NFSMNT_NFSV3, NFSMNT_NOCONN, NFSMNT_RDIRPLUS, NFSMNT_READAHEAD, NFSMNT_READDIRSIZE,
    NFSMNT_RETRANS, NFSMNT_RSIZE, NFSMNT_SOFT, NFSMNT_TIMEO, NFSMNT_WSIZE, NfsArgs, Statfs, Vfsops,
};
use crate::sys::namei::Nameidata;
use crate::sys::param::{DEV_BSHIFT, MAXBSIZE, NODEV, PSOCK};
use crate::sys::proc::Proc;
use crate::sys::socket::SOCK_DGRAM;
use crate::sys::swap::NETDEV;
use crate::sys::time::sec_to_nsec;
use crate::sys::types::{Ino, Uid};
use crate::sys::ucred::Ucred;
use crate::sys::vnode::{FORCECLOSE, VDIR, VNON, VREG, VROOT, Vattr, Vnode};

/// `nfs_vfsops`: nfs vfs operations.
pub static NFS_VFSOPS: Vfsops = Vfsops {
    vfs_mount: nfs_mount,
    vfs_start: nfs_start,
    vfs_unmount: nfs_unmount,
    vfs_root: nfs_root,
    vfs_quotactl: nfs_quotactl,
    vfs_statfs: nfs_statfs,
    vfs_sync: nfs_sync,
    vfs_vget: nfs_vget,
    vfs_fhtovp: nfs_fhtovp,
    vfs_vptofh: nfs_vptofh,
    vfs_init: Some(nfs_vfs_init),
    vfs_sysctl: Some(nfs_sysctl),
    vfs_checkexp: nfs_checkexp,
};

/// `nfs_diskless`: what `nfs_mountroot` boots from; `uvm_swap.c`'s `swapmount` reads the swap
/// vnode and the host name from it. `nfs_boot_getfh` points the arguments' `addr`, `fh` and
/// `hostname` at members of the structure, so it stays where it is.
pub static NFS_DISKLESS: StaticCell<NfsDiskless> = StaticCell::new(NfsDiskless::new());

/// The bytes of a `struct nfs_args`, field by field with the padding zero (what
/// `mnt_stat.mount_info` holds).
fn nfsargs_bytes(a: &NfsArgs) -> [u8; NfsArgs::SIZE] {
    use core::mem::offset_of;
    let mut b = [0u8; NfsArgs::SIZE];
    macro_rules! put {
        ($($f:ident),*) => {$(
            let o = offset_of!(NfsArgs, $f);
            let v = a.$f.to_ne_bytes();
            b[o..o + v.len()].copy_from_slice(&v);
        )*};
    }
    put!(
        version,
        addr,
        addrlen,
        sotype,
        proto,
        fh,
        fhsize,
        flags,
        wsize,
        rsize,
        readdirsize,
        timeo,
        retrans,
        maxgrouplist,
        readahead,
        leaseterm,
        deadthresh,
        hostname,
        acregmin,
        acregmax,
        acdirmin,
        acdirmax
    );
    b
}

/// Stores `args` as `mp->mnt_stat.mount_info.nfs_args`.
fn store_mount_args(mp: &Mount, args: &NfsArgs) {
    let b = nfsargs_bytes(args);
    mp.update_stat(|sp| sp.mount_info.__align[..NfsArgs::SIZE].copy_from_slice(&b));
}

/// `mp->mnt_stat.mount_info.nfs_args`.
fn mount_args(mp: &Mount) -> NfsArgs {
    let sp = mp.mnt_stat.get();
    match NfsArgs::from_bytes(&sp.mount_info.__align) {
        Some(a) => a,
        None => panic(format_args!("nfs: mount_info is shorter than nfs_args")),
    }
}

/// `nfs_statfs(mp, sbp, p)`: nfs statfs call.
pub fn nfs_statfs(mp: &'static Mount, sbp: &mut Statfs, p: &Proc) -> Result<(), Errno> {
    let nmp = VFSTONFS(mp);
    let v3 = nmp.nm_flag.get() & NFSMNT_NFSV3 != 0;

    let vp = nfs_root(mp)?;
    let cred = crget();
    cred.cr_ngroups.set(0);
    if v3 && nmp.nm_flag.get() & NFSMNT_GOTFSINFO == 0 {
        let _ = nfs_fsinfo(nmp, vp, ptr::from_ref(cred), Some(p));
    }
    NFSSTATS.rpccnt[NFSPROC_FSSTAT].fetch_add(1, Relaxed);
    let mut info = NfsmInfo::new();
    info.nmi_v3 = v3;
    let req = nfsm_reqhead(nfsx_fh(v3));
    info.nmi_mreq = Some(req);
    let mut mb = req;
    nfsm_fhtom(&mut mb, vp, v3);

    info.nmi_procp = Some(p);
    info.nmi_cred = ptr::from_ref(cred);
    let r = (|| -> Result<(), Errno> {
        let rq = nfs_request(vp, NFSPROC_FSSTAT, &mut info);
        if v3 {
            let mut vpv = vp;
            let mut retattr = 0;
            nfsm_postop_attr(&mut info, &mut vpv, &mut retattr)?;
        }
        rq?;

        let sfp: NfsStatfs = nfsm_dissect(&mut info, nfsx_statfs(v3))?.read(0);
        sbp.f_iosize = nmp.nm_rsize.get().min(nmp.nm_wsize.get()) as u32;
        if v3 {
            let blk = NFS_FABLKSIZE;
            sbp.f_bsize = NFS_FABLKSIZE as u32;
            let tquad = fxdr_hyper(sfp.sf_tbytes().words());
            sbp.f_blocks = tquad / blk;
            let tquad = fxdr_hyper(sfp.sf_fbytes().words());
            sbp.f_bfree = tquad / blk;
            let tquad = fxdr_hyper(sfp.sf_abytes().words());
            sbp.f_bavail = (tquad as i64) / (blk as i64);

            let tquad = fxdr_hyper(sfp.sf_tfiles().words());
            sbp.f_files = tquad;
            let tquad = fxdr_hyper(sfp.sf_ffiles().words());
            sbp.f_ffree = tquad;
            sbp.f_favail = tquad as i64;
        } else {
            sbp.f_bsize = fxdr_unsigned(sfp.sf_bsize());
            sbp.f_blocks = i64::from(fxdr_unsigned(sfp.sf_blocks()) as i32) as u64;
            sbp.f_bfree = i64::from(fxdr_unsigned(sfp.sf_bfree()) as i32) as u64;
            sbp.f_bavail = i64::from(fxdr_unsigned(sfp.sf_bavail()) as i32);
            sbp.f_files = 0;
            sbp.f_ffree = 0;
            sbp.f_favail = 0;
        }
        copy_statfs_info(sbp, mp);
        Ok(())
    })();
    // nfsmout:
    m_freem(info.nmi_mrep.take());
    vput(vp);
    crfree(cred);
    r
}

/// What `nfs_fsinfo` does with the server's answer: shrinks the mount's `nm_wsize`, `nm_rsize`
/// and `nm_readdirsize` to the preferred and maximum transfer sizes the server gave (rounded
/// as the C does) and marks the mount `NFSMNT_GOTFSINFO`.
fn fsinfo_apply(nmp: &NfsMount, fsp: &Nfsv3Fsinfo) {
    let fab = NFS_FABLKSIZE as u32;
    let dirblk = NFS_DIRBLKSIZ as u32;
    let pref = fxdr_unsigned(fsp.fs_wtpref);
    if pref < nmp.nm_wsize.get() as u32 {
        nmp.nm_wsize.set(((pref + fab - 1) & !(fab - 1)) as i32);
    }
    let mut max = fxdr_unsigned(fsp.fs_wtmax);
    if max < nmp.nm_wsize.get() as u32 {
        nmp.nm_wsize.set((max & !(fab - 1)) as i32);
        if nmp.nm_wsize.get() == 0 {
            nmp.nm_wsize.set(max as i32);
        }
    }
    let pref = fxdr_unsigned(fsp.fs_rtpref);
    if pref < nmp.nm_rsize.get() as u32 {
        nmp.nm_rsize.set(((pref + fab - 1) & !(fab - 1)) as i32);
    }
    max = fxdr_unsigned(fsp.fs_rtmax);
    if max < nmp.nm_rsize.get() as u32 {
        nmp.nm_rsize.set((max & !(fab - 1)) as i32);
        if nmp.nm_rsize.get() == 0 {
            nmp.nm_rsize.set(max as i32);
        }
    }
    let pref = fxdr_unsigned(fsp.fs_dtpref);
    if pref < nmp.nm_readdirsize.get() as u32 {
        nmp.nm_readdirsize
            .set(((pref + dirblk - 1) & !(dirblk - 1)) as i32);
    }
    // (`max` is still the read maximum here, as in the C.)
    if max < nmp.nm_readdirsize.get() as u32 {
        nmp.nm_readdirsize.set((max & !(dirblk - 1)) as i32);
        if nmp.nm_readdirsize.get() == 0 {
            nmp.nm_readdirsize.set(max as i32);
        }
    }
    nmp.nm_flag.set(nmp.nm_flag.get() | NFSMNT_GOTFSINFO);
}

/// `nfs_fsinfo(nmp, vp, cred, p)`: nfs version 3 fsinfo rpc call.
pub fn nfs_fsinfo(
    nmp: &NfsMount,
    vp: &'static Vnode,
    cred: *const Ucred,
    p: Option<&Proc>,
) -> Result<(), Errno> {
    NFSSTATS.rpccnt[NFSPROC_FSINFO].fetch_add(1, Relaxed);
    let mut info = NfsmInfo::new();
    info.nmi_v3 = true;
    let req = nfsm_reqhead(nfsx_fh(true));
    info.nmi_mreq = Some(req);
    let mut mb = req;
    nfsm_fhtom(&mut mb, vp, true);

    info.nmi_procp = p;
    info.nmi_cred = cred;
    let r = (|| -> Result<(), Errno> {
        let rq = nfs_request(vp, NFSPROC_FSINFO, &mut info);

        let mut vpv = vp;
        let mut retattr = 0;
        nfsm_postop_attr(&mut info, &mut vpv, &mut retattr)?;
        rq?;

        let fsp: Nfsv3Fsinfo = nfsm_dissect(&mut info, NFSX_V3FSINFO)?.read(0);
        fsinfo_apply(nmp, &fsp);
        Ok(())
    })();
    // nfsmout:
    m_freem(info.nmi_mrep.take());
    r
}

/// `nfs_mountroot()`: mount a remote root fs via NFS. It goes like this:
/// - Call `nfs_boot_init()` to fill in the `nfs_diskless` struct (using RARP, bootparam RPC,
///   mountd RPC)
/// - hand craft the swap nfs vnode hanging off a fake mount point if `swdevt[0] == NODEV`
/// - build the rootfs mount point and call `mountnfs()` to do the rest.
pub fn nfs_mountroot() -> Result<(), Errno> {
    let Some(procp) = curproc() else {
        panic(format_args!("nfs_mountroot: no curproc"));
    };
    // SAFETY: `nfs_mountroot` runs once, from `main` on the boot thread, and nothing else
    // refers to the structure until `swapmount` reads it afterwards.
    let nd = unsafe { NFS_DISKLESS.get_mut() };

    // Call nfs_boot_init() to fill in the nfs_diskless struct. Side effect: finds and
    // configures a network interface.
    if let Err(e) = nfs_boot_init(nd, procp) {
        panic(format_args!(
            "nfs_mountroot: nfs_boot_init: error {}",
            e.as_i32()
        ));
    }

    // Create the root mount point.
    let boot = nd.nd_boot;
    if nfs_boot_getfh(&boot, b"root", &mut nd.nd_root, -1).is_err() {
        panic(format_args!("nfs_mountroot: root"));
    }
    let (mp, vp) = nfs_mount_diskless(&nd.nd_root, b"/", 0, procp);
    let _ = printf(format_args!("root on {}\n", Str(&nd.nd_root.ndm_host)));

    // Link it into the mount list.
    // SAFETY: a new mount on no list, under the kernel lock; mounts never move.
    unsafe { MOUNTLIST.0.insert_tail(mp) };
    set_rootvp(Some(vp));
    vfs_unbusy(mp);

    // Get root attributes (for the time).
    let mut attr = Vattr::new();
    if VOP_GETATTR(vp, &mut attr, procp.p_ucred.get(), procp).is_err() {
        panic(format_args!("nfs_mountroot: getattr for root"));
    }
    inittodr(attr.va_atime.tv_sec);

    // "Mount" the swap device.
    //
    // On a "dataless" configuration (swap on disk) we will have: (swdevt[0] != NODEV)
    // identifying the swap device.
    let sw0 = crate::sys::conf::SWDEVT[0].load(Relaxed);
    if sw0 != NODEV {
        match bdevvp(swapdev()) {
            Ok(Some(svp)) => set_swapdev_vp(Some(svp)),
            _ => panic(format_args!("nfs_mountroot: can't setup swap vp")),
        }
        let _ = printf(format_args!("swap on device {:#x}\n", sw0));
        return Ok(());
    }

    // If swapping to an nfs node: (swdevt[0] == NODEV)
    // Create a fake mount point just for the swap vnode so that the swap file can be on a
    // different server from the rootfs.
    //
    // Wait 5 retries, finally no swap is cool. -mickey
    let boot = nd.nd_boot;
    if nfs_boot_getfh(&boot, b"swap", &mut nd.nd_swap, 5).is_ok() {
        let (mp, vp) = nfs_mount_diskless(&nd.nd_swap, b"/swap", 0, procp);
        vfs_unbusy(mp);

        // Since the swap file is not the root dir of a file system, hack it to a regular
        // file.
        vp.v_type.set(VREG);
        vp.v_flag.set(0);

        // Next line is a hack to make swapmount() work on NFS swap files.
        crate::sys::conf::SWDEVT[0].store(NETDEV, Relaxed);
        // end hack
        nd.sw_vp = Some(vp);

        // Find out how large the swap file is.
        if VOP_GETATTR(vp, &mut attr, procp.p_ucred.get(), procp).is_err() {
            let _ = printf(format_args!("nfs_mountroot: getattr for swap\n"));
        }
        let _swap_blocks = attr.va_size >> DEV_BSHIFT;

        let _ = printf(format_args!("swap on {}\n", Str(&nd.nd_swap.ndm_host)));
        return Ok(());
    }

    let _ = printf(format_args!("WARNING: no swap\n"));
    crate::sys::conf::SWDEVT[0].store(NODEV, Relaxed);
    Ok(())
}

/// `nfs_mount_diskless(ndmntp, mntname, mntflag, vpp, p)`: internal version of mount system
/// call for diskless setup. Returns the mount and its root vnode.
pub fn nfs_mount_diskless(
    ndmntp: &NfsDlmount,
    mntname: &[u8],
    mntflag: i32,
    p: &Proc,
) -> (&'static Mount, &'static Vnode) {
    let mp = match vfs_rootmountalloc(b"nfs", mntname) {
        Ok(mp) => mp,
        Err(_) => panic(format_args!(
            "nfs_mount_diskless: vfs_rootmountalloc failed"
        )),
    };
    mp.mnt_flag.set(mp.mnt_flag.get() | mntflag);

    // Get mbuf for server sockaddr.
    let Some(m) = m_get(M_WAIT, MT_SONAME) else {
        panic(format_args!(
            "nfs_mount_diskless: no mbuf for the server address"
        ));
    };
    let sin = &ndmntp.ndm_saddr;
    let len = usize::from(sin.sin_len).min(size_of_val(sin));
    // SAFETY: the mbuf's data area holds at least `MLEN` (more than a `sockaddr_in`) bytes; the
    // source is `len` bytes of the plain-integer structure.
    unsafe { ptr::copy_nonoverlapping(ptr::from_ref(sin).cast::<u8>(), mtod::<u8>(m), len) };
    m.m_len().set(len as u32);

    let fhsize = (ndmntp.ndm_args.fhsize.max(0) as usize).min(NFSX_V3FHMAX);
    match mountnfs(
        &ndmntp.ndm_args,
        mp,
        m,
        mntname,
        &ndmntp.ndm_host,
        &ndmntp.ndm_fh[..fhsize],
        p,
    ) {
        Ok(Some(vp)) => (mp, vp),
        Ok(None) => panic(format_args!(
            "nfs_mountroot: mount {} gave no root vnode",
            Str(mntname)
        )),
        Err(e) => panic(format_args!(
            "nfs_mountroot: mount {} failed: {}",
            Str(mntname),
            e.as_i32()
        )),
    }
}

/// The initial timeout of a mount from `timeo` (tenths of a second): `(timeo * NFS_HZ + 5) /
/// 10` ticks, clamped to `NFS_MINTIMEO`..`NFS_MAXTIMEO` (`nfs_decode_args`).
fn decode_timeo(timeo: i32, hz: i32, mintimeo: i32, maxtimeo: i32) -> i32 {
    let t = (timeo * hz + 5) / 10;
    if t < mintimeo {
        mintimeo
    } else if t > maxtimeo {
        maxtimeo
    } else {
        t
    }
}

/// `nfs_decode_args(nmp, argp, nargp)`: applies the mount arguments `argp` to the mount
/// `nmp` and returns what the mount ended up with in `nargp`.
pub fn nfs_decode_args(nmp: &'static NfsMount, argp: &NfsArgs, nargp: &mut NfsArgs) {
    let mut adjsock = false;

    // Also re-bind if we're switching to/from a connected UDP socket.
    adjsock |= (nmp.nm_flag.get() & NFSMNT_NOCONN) != (argp.flags & NFSMNT_NOCONN);

    nmp.nm_flag
        .set((argp.flags & !NFSMNT_INTERNAL) | (nmp.nm_flag.get() & NFSMNT_INTERNAL));

    if argp.flags & NFSMNT_TIMEO != 0 && argp.timeo > 0 {
        nmp.nm_timeo.set(decode_timeo(
            argp.timeo,
            nfs_hz(),
            nfs_mintimeo(),
            nfs_maxtimeo(),
        ));
    }

    if argp.flags & NFSMNT_RETRANS != 0 && argp.retrans > 1 {
        nmp.nm_retry.set(argp.retrans.min(NFS_MAXREXMIT));
    }
    if nmp.nm_flag.get() & NFSMNT_SOFT == 0 {
        nmp.nm_retry.set(NFS_MAXREXMIT + 1); // past clip limit
    }

    let maxio = if argp.flags & NFSMNT_NFSV3 != 0 {
        if argp.sotype == SOCK_DGRAM {
            NFS_MAXDGRAMDATA as i32
        } else {
            NFS_MAXDATA as i32
        }
    } else {
        NFS_V2MAXDATA as i32
    };
    let fab = NFS_FABLKSIZE as i32;

    if argp.flags & NFSMNT_WSIZE != 0 && argp.wsize > 0 {
        let osize = nmp.nm_wsize.get();
        nmp.nm_wsize.set(argp.wsize);
        // Round down to multiple of blocksize.
        nmp.nm_wsize.set(nmp.nm_wsize.get() & !(fab - 1));
        if nmp.nm_wsize.get() <= 0 {
            nmp.nm_wsize.set(fab);
        }
        adjsock |= nmp.nm_wsize.get() != osize;
    }
    if nmp.nm_wsize.get() > maxio {
        nmp.nm_wsize.set(maxio);
    }
    if nmp.nm_wsize.get() > MAXBSIZE as i32 {
        nmp.nm_wsize.set(MAXBSIZE as i32);
    }

    if argp.flags & NFSMNT_RSIZE != 0 && argp.rsize > 0 {
        let osize = nmp.nm_rsize.get();
        nmp.nm_rsize.set(argp.rsize);
        // Round down to multiple of blocksize.
        nmp.nm_rsize.set(nmp.nm_rsize.get() & !(fab - 1));
        if nmp.nm_rsize.get() <= 0 {
            nmp.nm_rsize.set(fab);
        }
        adjsock |= nmp.nm_rsize.get() != osize;
    }
    if nmp.nm_rsize.get() > maxio {
        nmp.nm_rsize.set(maxio);
    }
    if nmp.nm_rsize.get() > MAXBSIZE as i32 {
        nmp.nm_rsize.set(MAXBSIZE as i32);
    }

    let dirblk = NFS_DIRBLKSIZ;
    if argp.flags & NFSMNT_READDIRSIZE != 0 && argp.readdirsize > 0 {
        nmp.nm_readdirsize.set(argp.readdirsize);
        // Round down to multiple of blocksize.
        nmp.nm_readdirsize
            .set(nmp.nm_readdirsize.get() & !(dirblk - 1));
        if nmp.nm_readdirsize.get() < dirblk {
            nmp.nm_readdirsize.set(dirblk);
        }
    } else if argp.flags & NFSMNT_RSIZE != 0 {
        nmp.nm_readdirsize.set(nmp.nm_rsize.get());
    }

    if nmp.nm_readdirsize.get() > maxio {
        nmp.nm_readdirsize.set(maxio);
    }

    if argp.flags & NFSMNT_MAXGRPS != 0
        && argp.maxgrouplist >= 0
        && argp.maxgrouplist <= NFS_MAXGRPS
    {
        nmp.nm_numgrps.set(argp.maxgrouplist);
    }
    if argp.flags & NFSMNT_READAHEAD != 0 && argp.readahead >= 0 && argp.readahead <= NFS_MAXRAHEAD
    {
        nmp.nm_readahead.set(argp.readahead);
    }
    let clip = |v: i32| -> u16 { if v > 0xffff { 0xffff } else { v as u16 } };
    if argp.flags & NFSMNT_ACREGMIN != 0 && argp.acregmin >= 0 {
        nmp.nm_acregmin.set(clip(argp.acregmin));
    }
    if argp.flags & NFSMNT_ACREGMAX != 0 && argp.acregmax >= 0 {
        nmp.nm_acregmax.set(clip(argp.acregmax));
    }
    if nmp.nm_acregmin.get() > nmp.nm_acregmax.get() {
        nmp.nm_acregmin.set(nmp.nm_acregmax.get());
    }

    if argp.flags & NFSMNT_ACDIRMIN != 0 && argp.acdirmin >= 0 {
        nmp.nm_acdirmin.set(clip(argp.acdirmin));
    }
    if argp.flags & NFSMNT_ACDIRMAX != 0 && argp.acdirmax >= 0 {
        nmp.nm_acdirmax.set(clip(argp.acdirmax));
    }
    if nmp.nm_acdirmin.get() > nmp.nm_acdirmax.get() {
        nmp.nm_acdirmin.set(nmp.nm_acdirmax.get());
    }

    if nmp.nm_so.get().is_some() && adjsock {
        nfs_disconnect(nmp);
        if nmp.nm_sotype.get() == SOCK_DGRAM {
            while nfs_connect(nmp, None).is_err() {
                let _ = printf(format_args!("nfs_args: retrying connect\n"));
                let _ = tsleep_nsec(nowake(), PSOCK, "nfscon", sec_to_nsec(1));
            }
        }
    }

    // Update nargp based on nmp.
    nargp.wsize = nmp.nm_wsize.get();
    nargp.rsize = nmp.nm_rsize.get();
    nargp.readdirsize = nmp.nm_readdirsize.get();
    nargp.timeo = nmp.nm_timeo.get();
    nargp.retrans = nmp.nm_retry.get();
    nargp.maxgrouplist = nmp.nm_numgrps.get();
    nargp.readahead = nmp.nm_readahead.get();
    nargp.acregmin = i32::from(nmp.nm_acregmin.get());
    nargp.acregmax = i32::from(nmp.nm_acregmax.get());
    nargp.acdirmin = i32::from(nmp.nm_acdirmin.get());
    nargp.acdirmax = i32::from(nmp.nm_acdirmax.get());
}

/// `nfs_mount(mp, path, data, ndp, p)`: VFS operations: mount system call. It seems a bit
/// dumb to copyinstr() the host here and then bcopy() it in mountnfs(), but I wanted to
/// detect errors before doing the sockargs() call because sockargs() allocates an mbuf and
/// an error after that means that I have to release the mbuf.
pub fn nfs_mount(
    mp: &'static Mount,
    path: &[u8],
    data: &mut [u8],
    _ndp: &mut Nameidata<'_>,
    p: &Proc,
) -> Result<(), Errno> {
    let args = NfsArgs::from_bytes(data);

    if let Some(a) = &args
        && a.flags & (NFSMNT_NFSV3 | NFSMNT_RDIRPLUS) == NFSMNT_RDIRPLUS
    {
        return Err(Errno::EINVAL);
    }

    if nfs_niothreads.load(Relaxed) < 0 {
        nfs_niothreads.store(4, Relaxed);
        nfs_getset_niothreads(true);
    }

    if mp.mnt_flag.get() & MNT_UPDATE != 0 {
        if mp.mnt_data.get().is_null() {
            return Err(Errno::EIO);
        }
        let nmp = VFSTONFS(mp);

        // When doing an update, we can't change from or to v3.
        if let Some(mut a) = args {
            a.flags = (a.flags & !NFSMNT_NFSV3) | (nmp.nm_flag.get() & NFSMNT_NFSV3);
            let mut nargs = mount_args(mp);
            nfs_decode_args(nmp, &a, &mut nargs);
            store_mount_args(mp, &nargs);
        }
        return Ok(());
    }
    let Some(args) = args else {
        return Err(Errno::EINVAL);
    };
    if args.fhsize < 0 || args.fhsize as usize > NFSX_V3FHMAX {
        return Err(Errno::EINVAL);
    }
    let fhsize = args.fhsize as usize;
    let mut nfh = [0u8; NFSX_V3FHMAX];
    copyin(args.fh, &mut nfh[..fhsize])?;
    let mut hst = [0u8; MNAMELEN];
    copyinstr(args.hostname, &mut hst[..MNAMELEN - 1])?;
    // sockargs() call must be after above copyin() calls.
    let nam = sockargs(args.addr, args.addrlen as usize, MT_SONAME)?;
    mountnfs(&args, mp, nam, path, &hst, &nfh[..fhsize], p)?;
    Ok(())
}

/// `mountnfs(argp, mp, nam, pth, hst, vpp, p)`: common code for mount and mountroot. `fh` is
/// the root file handle (`argp->fh`, `argp->fhsize` bytes); returns the root vnode, unlocked.
pub fn mountnfs(
    argp: &NfsArgs,
    mp: &'static Mount,
    nam: &'static Mbuf,
    pth: &[u8],
    hst: &[u8],
    fh: &[u8],
    p: &Proc,
) -> Result<Option<&'static Vnode>, Errno> {
    if mp.mnt_flag.get() & MNT_UPDATE != 0 {
        // update paths, file handles, etc, here	XXX
        m_freem(Some(nam));
        return Ok(None);
    }
    let Some(mem) = malloc(size_of::<NfsMount>(), M_NFSMNT, M_WAITOK | M_ZERO) else {
        m_freem(Some(nam));
        return Err(Errno::ENOMEM);
    };
    // SAFETY: a fresh allocation of `size_of::<NfsMount>()` bytes (malloc aligns to the bucket
    // size, at least 16), written once; it lives until `nfs_reaper` (or the failure path
    // below) frees it.
    let nmp: &'static NfsMount = unsafe {
        mem.cast::<NfsMount>().as_ptr().write(NfsMount::new());
        &*mem.cast::<NfsMount>().as_ptr()
    };
    mp.mnt_data
        .set(ptr::from_ref(nmp).cast_mut().cast::<c_void>());

    vfs_getnewfsid(mp);
    nmp.nm_mountp.set(Some(mp));
    nmp.nm_timeo.set(nfs_timeo());
    nmp.nm_retry.set(NFS_RETRANS);
    nmp.nm_wsize.set(NFS_WSIZE);
    nmp.nm_rsize.set(NFS_RSIZE);
    nmp.nm_readdirsize.set(NFS_READDIRSIZE);
    nmp.nm_numgrps.set(NFS_MAXGRPS);
    nmp.nm_readahead.set(NFS_DEFRAHEAD);
    nmp.nm_acregmin.set(NFS_MINATTRTIMO as u16);
    nmp.nm_acregmax.set(NFS_MAXATTRTIMO as u16);
    nmp.nm_acdirmin.set(NFS_MINATTRTIMO as u16);
    nmp.nm_acdirmax.set(NFS_MAXATTRTIMO as u16);
    mp.update_stat(|sp| {
        sp.f_namemax = MAXNAMLEN as u32;
        sp.f_mntonname = [0; MNAMELEN];
        strlcpy(&mut sp.f_mntonname[..MNAMELEN - 1], pth);
        sp.f_mntfromname = [0; MNAMELEN];
        strlcpy(&mut sp.f_mntfromname[..MNAMELEN - 1], hst);
        sp.f_mntfromspec = [0; MNAMELEN];
        strlcpy(&mut sp.f_mntfromspec[..MNAMELEN - 1], hst);
    });
    let mut nargs = *argp;
    nargs.addr = 0;
    nargs.fh = 0;
    nargs.hostname = 0;
    nmp.nm_nam.set(Some(nam));
    nfs_decode_args(nmp, argp, &mut nargs);
    store_mount_args(mp, &nargs);

    nfs_ninit(nmp);
    nmp.nm_reqsq.init();
    timeout_set_proc(
        &nmp.nm_rtimeout,
        nfs_timer,
        ptr::from_ref(nmp).cast_mut().cast::<c_void>(),
    );

    // Set up the sockets and per-host congestion.
    nmp.nm_sotype.set(argp.sotype);
    nmp.nm_soproto.set(argp.proto);

    let bad = |error: Errno| -> Result<Option<&'static Vnode>, Errno> {
        nfs_disconnect(nmp);
        mp.mnt_data.set(ptr::null_mut());
        free(
            ptr::NonNull::from(nmp).cast::<u8>(),
            M_NFSMNT,
            size_of::<NfsMount>(),
        );
        m_freem(Some(nam));
        Err(error)
    };

    // For Connection based sockets (TCP,...) defer the connect until the first request, in
    // case the server is not responding.
    if nmp.nm_sotype.get() == SOCK_DGRAM
        && let Err(e) = nfs_connect(nmp, None)
    {
        return bad(e);
    }

    // This is silly, but it has to be set so that vinifod() works. We do not want to do an
    // nfs_statfs() here since we can get stuck on a dead server and we are holding a lock on
    // the mount point.
    mp.update_stat(|sp| sp.f_iosize = NFS_MAXDGRAMDATA as u32);
    let np = match nfs_nget(mp, fh) {
        Ok(np) => np,
        Err(e) => return bad(e),
    };
    let vp = NFSTOV(np);
    let mut attr = Vattr::new();
    if let Err(e) = VOP_GETATTR(vp, &mut attr, p.p_ucred.get(), p) {
        vput(vp);
        return bad(e);
    }

    // A reference count is needed on the nfsnode representing the remote root. If this object
    // is not persistent, then backward traversals of the mount point (i.e. "..") will not
    // work if the nfsnode gets flushed out of the cache. Ufs does not have this problem,
    // because one can identify root inodes by their number == ROOTINO (2). So, just unlock,
    // but no rele.
    nmp.nm_vnode.set(Some(vp));
    if vp.v_type.get() == VNON {
        vp.v_type.set(VDIR);
    }
    vp.v_flag.set(VROOT);
    let _ = VOP_UNLOCK(vp);

    Ok(Some(vp))
}

/// `nfs_unmount(mp, mntflags, p)`: unmount system call.
pub fn nfs_unmount(mp: &'static Mount, mntflags: i32, _p: &Proc) -> Result<(), Errno> {
    let nmp = VFSTONFS(mp);
    let mut flags = 0;

    let vp = nfs_root(mp)?;

    if mntflags & MNT_FORCE == 0 && vp.v_usecount.get() > 2 {
        vput(vp);
        return Err(Errno::EBUSY);
    }

    if mntflags & MNT_FORCE != 0 {
        flags |= FORCECLOSE;
    }

    if let Err(e) = vflush(mp, Some(vp), flags) {
        vput(vp);
        return Err(e);
    }

    // There are two references count to get rid of here: one from mountnfs() and one from
    // nfs_root() above.
    vrele(vp);
    vput(vp);
    vgone(vp);
    nfs_disconnect(nmp);
    m_freem(nmp.nm_nam.get());
    timeout_del(&nmp.nm_rtimeout);
    timeout_set_proc(
        &nmp.nm_rtimeout,
        nfs_reaper,
        ptr::from_ref(nmp).cast_mut().cast::<c_void>(),
    );
    timeout_add(&nmp.nm_rtimeout, 0);
    mp.mnt_data.set(ptr::null_mut());
    Ok(())
}

/// `nfs_reaper(arg)`: delay nfs mount point free until pending or sleeping timeouts have
/// finished.
pub fn nfs_reaper(arg: *mut c_void) {
    let Some(nmp) = ptr::NonNull::new(arg.cast::<u8>()) else {
        panic(format_args!("nfs_reaper: no mount"));
    };
    free(nmp, M_NFSMNT, size_of::<NfsMount>());
}

/// `nfs_root(mp, vpp)`: return root of a filesystem.
pub fn nfs_root(mp: &'static Mount) -> Result<&'static Vnode, Errno> {
    let nmp = VFSTONFS(mp);
    let Some(vp) = nmp.nm_vnode.get() else {
        panic(format_args!("nfs_root: mount without a root vnode"));
    };
    vref(vp);
    if let Err(e) = vn_lock(vp, LK_EXCLUSIVE | LK_RETRY) {
        vrele(vp);
        return Err(e);
    }
    Ok(vp)
}

/// `nfs_sync(mp, waitfor, stall, cred, p)`: flush out the buffer cache.
pub fn nfs_sync(
    mp: &'static Mount,
    waitfor: i32,
    _stall: i32,
    cred: *const Ucred,
    p: &Proc,
) -> Result<(), Errno> {
    let mut allerror = Ok(());

    // Don't traverse the vnode list if we want to skip all of them.
    if waitfor == MNT_LAZY {
        return allerror;
    }

    // Force stale buffer cache information to be flushed.
    'restart: loop {
        for vp in mp.mnt_vnodelist.iter() {
            // If the vnode that we are about to sync is no longer associated with this mount
            // point, start over.
            if !vp.v_mount.get().is_some_and(|m| ptr::eq(m, mp)) {
                continue 'restart;
            }
            if VOP_ISLOCKED(vp) != 0 {
                continue;
            }
            let s = splbio();
            let empty = vp.v_dirtyblkhd.is_empty();
            splx(s);
            if empty {
                continue;
            }
            if vget(vp, LK_EXCLUSIVE).is_err() {
                continue 'restart;
            }
            if let Err(e) = VOP_FSYNC(vp, cred, waitfor, p) {
                allerror = Err(e);
            }
            vput(vp);
        }
        break;
    }

    allerror
}

/// `nfs_vget(mp, ino, vpp)`: NFS flat namespace lookup. Currently unsupported.
pub fn nfs_vget(_mp: &'static Mount, _ino: Ino) -> Result<&'static Vnode, Errno> {
    Err(Errno::EOPNOTSUPP)
}

/// `nfs_sysctl(name, namelen, oldp, oldlenp, newp, newlen, p)`: do that sysctl thang...
pub fn nfs_sysctl(
    name: &[i32],
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    newlen: usize,
    _p: &Proc,
) -> Result<(), Errno> {
    // All names at this level are terminal.
    if name.len() > 1 {
        return Err(Errno::ENOTDIR); // overloaded
    }
    let Some(&first) = name.first() else {
        return Err(Errno::EOPNOTSUPP);
    };

    const STATS_SIZE: usize = crate::nfs::nfs::Nfsstats::NWORDS * size_of::<u64>();
    match first {
        NFS_NFSSTATS => {
            if oldp == 0 {
                *oldlenp = STATS_SIZE;
                return Ok(());
            }

            if *oldlenp < STATS_SIZE {
                *oldlenp = STATS_SIZE;
                return Err(Errno::ENOMEM);
            }

            let words = NFSSTATS.snapshot();
            let mut bytes = [0u8; STATS_SIZE];
            let (chunks, _) = bytes.as_chunks_mut::<{ size_of::<u64>() }>();
            for (chunk, w) in chunks.iter_mut().zip(words) {
                *chunk = w.to_ne_bytes();
            }
            copyout(&bytes, oldp)?;

            if newp != 0 && newlen != STATS_SIZE {
                return Err(Errno::EINVAL);
            }

            if newp != 0 {
                copyin(newp, &mut bytes)?;
                let mut words = [0u64; crate::nfs::nfs::Nfsstats::NWORDS];
                let (chunks, _) = bytes.as_chunks::<{ size_of::<u64>() }>();
                for (w, chunk) in words.iter_mut().zip(chunks) {
                    *w = u64::from_ne_bytes(*chunk);
                }
                NFSSTATS.restore(&words);
            }
            Ok(())
        }
        NFS_NIOTHREADS => {
            nfs_getset_niothreads(false);

            let rv = sysctl_int(oldp, oldlenp, newp, newlen, &nfs_niothreads);
            if newp != 0 {
                nfs_getset_niothreads(true);
            }

            rv
        }
        _ => Err(Errno::EOPNOTSUPP),
    }
}

/// `nfs_fhtovp(mp, fhp, vpp)`: at this point, this should never happen.
pub fn nfs_fhtovp(_mp: &'static Mount, _fhp: &Fid) -> Result<&'static Vnode, Errno> {
    Err(Errno::EINVAL)
}

/// `nfs_vptofh(vp, fhp)`: vnode pointer to file handle, should never happen either.
pub fn nfs_vptofh(_vp: &'static Vnode, _fhp: &mut Fid) -> Result<(), Errno> {
    Err(Errno::EINVAL)
}

/// `nfs_start(mp, flags, p)`: vfs start routine, a no-op.
pub fn nfs_start(_mp: &'static Mount, _flags: i32, _p: &Proc) -> Result<(), Errno> {
    Ok(())
}

/// `nfs_quotactl(mp, cmd, uid, arg, p)`: do operations associated with quotas, not supported.
pub fn nfs_quotactl(
    _mp: &'static Mount,
    _cmd: i32,
    _uid: Uid,
    _arg: usize,
    _p: &Proc,
) -> Result<(), Errno> {
    Err(Errno::EOPNOTSUPP)
}

/// `nfs_checkexp(mp, nam, exflagsp, credanonp)`: check export permission, not supported.
pub fn nfs_checkexp(
    _mp: &'static Mount,
    _nam: &Mbuf,
    _exflagsp: &mut i32,
    _credanonp: &mut *const Ucred,
) -> Result<(), Errno> {
    Err(Errno::EOPNOTSUPP)
}

#[cfg(test)]
mod tests;
