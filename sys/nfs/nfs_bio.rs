/*	$OpenBSD: nfs_bio.c,v 1.87 2024/09/18 05:21:19 jsg Exp $	*/
/*	$NetBSD: nfs_bio.c,v 1.25.4.2 1996/07/08 20:47:04 jtc Exp $	*/
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
 *	@(#)nfs_bio.c	8.9 (Berkeley) 3/30/95
 */
/* </LICENSES> */

/* <CODE> */
//! The NFS client's block I/O through the buffer cache: `nfs_bioread` (the read side of
//! `nfs_read`/`nfs_readlink`, with read-ahead), `nfs_write` (`vop_write`), the cache block
//! helpers `nfs_getcacheblk` and `nfs_vinvalbuf`, `nfs_asyncio` (hand a buffer to the
//! nfsiods through `nfs_bufq`) and `nfs_doio` (do the RPCs of one buffer, from a reading or
//! writing thread or from an nfsiod).
//!
//! Upstream: sys/nfs/nfs_bio.c @ 3ce1f3f79392
//!
//! For nfs, cache consistency can only be maintained approximately: a file whose
//! modification time on the server changed since the last read, or that this client wrote,
//! has its cached data thrown away (`nfs_vinvalbuf`). Each buffer keeps the part of its data
//! that is valid (`b_validoff`..`b_validend`) and the part written but not yet sent to the
//! server (`b_dirtyoff`..`b_dirtyend`); NFSv3 unstable writes are remembered with
//! `B_NEEDCOMMIT` and the commit ranges of `nfs_subs.rs`.
//!
//! ## Deviations
//! - `nfs_bufq`, `nfs_bufqlen` and `nfs_bufqmax` are defined in `nfsnode.rs` (`NFS_BUFQ`,
//!   `NFS_BUFQLEN`, `NFS_BUFQMAX`), where the header declares them. The sleep channels are
//!   the addresses of those statics, as in C: `nfssvc_iod` sleeps on `&NFS_BUFQ` and wakes
//!   `&NFS_BUFQLEN`. A buffer on `nfs_bufq` is linked through `b_freelist` as in C, without
//!   `b_onfreelist` (that flag marks the cache's own queues).
//! - `nfs_bioread` and `nfs_write` pass `uio_procp` to `VOP_GETATTR`, which takes a thread:
//!   when the uio has none (the C would pass NULL) `curproc` stands in, and `proc0` before any
//!   thread runs.
//! - `nfs_getcacheblk` returns `None` where the C returns NULL (interrupted by a signal).
//! - `nfs_asyncio(bp, readahead)` takes `readahead` as a `bool`; `nfs_doio`'s `must_commit`
//!   is a `bool`.
//! - `nfs_doio` reads a symbolic link with `curproc`'s credential, as the C; with no thread at
//!   all (impossible after boot) it passes `NOCRED` where the C would dereference NULL.
//! - The block arithmetic (`lbn`, `on`, `bn`), the merge of a write into the dirty and valid
//!   ranges, and the zero fill after a short read are small functions
//!   (`nfs_bioblk`, `nfs_bufranges`, `nfs_shortread`) so that host tests can check them.
//! - The `DIAGNOSTIC` mode checks of `nfs_bioread`/`nfs_write` are behind feature
//!   `diagnostic`.
//! - `nfs_numasync` (defined in `nfs_vnops.c`) is `nfs_vnops.rs`'s atomic `NFS_NUMASYNC`,
//!   the count the nfsiods keep.

use core::cmp::{max, min};
use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::Ordering;

use crate::kern::init_main::PROC0;
use crate::kern::kern_rwlock::{rw_enter_write, rw_exit_write};
use crate::kern::kern_sig::psignal;
use crate::kern::kern_subr::uiomove;
use crate::kern::kern_synch::{tsleep_nsec, wakeup, wakeup_one};
use crate::kern::subr_prf::{panic, uprintf};
use crate::kern::vfs_bio::{BCSTATS, bdwrite, biodone, brelse, buf_dirty, getblk, incore};
use crate::kern::vfs_subr::vinvalbuf;
use crate::kern::vfs_vnops::vn_fsizechk;
use crate::kern::vfs_vops::{VOP_BWRITE, VOP_GETATTR};
use crate::machine::cpu::curproc;
use crate::machine::intr::{splbio, splx};
use crate::nfs::nfs::B_INVAFTERWRITE;
use crate::nfs::nfs::nfs_isv3;
use crate::nfs::nfs_node::{nfs_crfree, nfs_crhold, vfstonfs_vp};
use crate::nfs::nfs_socket::nfs_sigintr;
use crate::nfs::nfs_subs::{
    NFSSTATS, nfs_add_tobecommitted_range, nfs_clearcommit, nfs_del_committed_range,
    nfs_del_tobecommitted_range,
};
use crate::nfs::nfs_vfsops::nfs_fsinfo;
use crate::nfs::nfs_vnops::{
    NFS_NUMASYNC, nfs_readlinkrpc, nfs_readrpc, nfs_writebp, nfs_writerpc,
};
use crate::nfs::nfsnode::{
    NFLUSHINPROG, NFLUSHWANT, NFS_BUFQ, NFS_BUFQLEN, NFS_BUFQMAX, NMODIFIED, NWRITEERR, VTONFS,
    nfs_invalidate_attrcache,
};
use crate::nfs::nfsproto::{
    NFS_MAXPATHLEN, NFSV3WRITE_DATASYNC, NFSV3WRITE_FILESYNC, NFSV3WRITE_UNSTABLE,
};
use crate::sys::buf::{
    B_ASYNC, B_DELWRI, B_DONE, B_EINTR, B_ERROR, B_INVAL, B_NEEDCOMMIT, B_NOCACHE, B_PHYS, B_READ,
    B_WRITEINPROG, Buf,
};
use crate::sys::errno::Errno;
use crate::sys::event::{NOTE_EXTEND, NOTE_TRUNCATE, NOTE_WRITE};
use crate::sys::mount::{NFSMNT_GOTFSINFO, NFSMNT_INT, NFSMNT_NFSV3};
use crate::sys::param::{DEV_BSHIFT, DEV_BSIZE, PCATCH, PRIBIO};
use crate::sys::proc::Proc;
use crate::sys::signal::SIGKILL;
use crate::sys::systm::INFSLP;
use crate::sys::time::sec_to_nsec;
use crate::sys::types::{Daddr, Off};
use crate::sys::ucred::{NOCRED, Ucred};
use crate::sys::uio::{Iovec, Uio, UioRw, UioSeg};
use crate::sys::vnode::{
    IO_APPEND, IO_SYNC, V_SAVE, VLNK, VN_KNOTE, VREG, VROOT, VTEXT, Vattr, Vnode, VopWriteArgs,
};
use crate::uvm::uvm_vnode::{uvm_vnp_setsize, uvm_vnp_uncache};

/// `nfs_numasync`: the number of running nfsiods.
fn nfs_numasync() -> i32 {
    NFS_NUMASYNC.load(Ordering::Relaxed)
}

/// The thread `VOP_GETATTR` runs for: `p`, else `curproc`, else `proc0`.
fn getattr_proc(p: Option<&Proc>) -> &Proc {
    match p {
        Some(p) => p,
        None => curproc().unwrap_or(&PROC0),
    }
}

/// `bp->b_proc = p`.
fn set_b_proc(bp: &Buf, p: Option<&Proc>) {
    bp.b_proc.set(p.map_or(ptr::null(), ptr::from_ref));
}

/// The cache block of a file offset for blocks of `biosize` bytes (a power of two):
/// `(lbn, on, bn)`, the logical block, the offset in it and the block number in `DEV_BSIZE`
/// units that names the buffer.
fn nfs_bioblk(offset: Off, biosize: i32) -> (Daddr, i32, Daddr) {
    let lbn = offset / Off::from(biosize);
    let on = (offset & Off::from(biosize - 1)) as i32;
    let bn = lbn * Daddr::from(biosize / DEV_BSIZE as i32);
    (lbn, on, bn)
}

/// The dirty and valid ranges of a buffer after `n` bytes were copied in at `on`:
/// `(dirtyoff, dirtyend, validoff, validend)` from the old ones.
fn nfs_bufranges(old: (i32, i32, i32, i32), on: i32, n: i32) -> (i32, i32, i32, i32) {
    let (mut dirtyoff, mut dirtyend, mut validoff, mut validend) = old;
    if dirtyend > 0 {
        dirtyoff = min(on, dirtyoff);
        dirtyend = max(on + n, dirtyend);
    } else {
        dirtyoff = on;
        dirtyend = on + n;
    }
    if validend == 0 || validend < dirtyoff || validoff > dirtyend {
        validoff = dirtyoff;
        validend = dirtyend;
    } else {
        validoff = min(validoff, dirtyoff);
        validend = max(validend, dirtyend);
    }
    (dirtyoff, dirtyend, validoff, validend)
}

/// A read of a `bcount`-byte buffer at file offset `boff` that left `resid` bytes unread:
/// `(zero, validend)`, the bytes to zero after the data (a hole in the file, whose later
/// writes have not been pushed to the server yet) and the end of the valid data.
fn nfs_shortread(bcount: i64, resid: usize, n_size: u64, boff: Off) -> (usize, i32) {
    let diff = (bcount - resid as i64) as i32;
    // The C computes in u_quad_t and keeps the low int.
    let len = n_size.wrapping_sub(boff.wrapping_add(Off::from(diff)) as u64) as i32;
    if len > 0 {
        let len = min(len as usize, resid);
        (len, diff + len as i32)
    } else {
        (0, diff)
    }
}

/// `nfs_bioread(vp, uio, ioflag, cred)`: vnode op for read using bio. Any similarity to
/// `readip()` is purely coincidental.
pub fn nfs_bioread(
    vp: &'static Vnode,
    uio: &mut Uio<'_>,
    _ioflag: i32,
    cred: *const Ucred,
) -> Result<(), Errno> {
    let np = VTONFS(vp);
    let nmp = vfstonfs_vp(vp);

    #[cfg(feature = "diagnostic")]
    if uio.uio_rw != UioRw::UIO_READ {
        panic(format_args!("nfs_read mode"));
    }
    if uio.uio_resid == 0 {
        return Ok(());
    }
    if uio.uio_offset < 0 {
        return Err(Errno::EINVAL);
    }
    let p = uio.uio_procp;
    if nmp.nm_flag.get() & (NFSMNT_NFSV3 | NFSMNT_GOTFSINFO) == NFSMNT_NFSV3 {
        let _ = nfs_fsinfo(nmp, vp, cred, p);
    }
    let biosize = nmp.nm_rsize.get();
    // For nfs, cache consistency can only be maintained approximately. Although RFC1094
    // does not specify the criteria, the following is believed to be compatible with the
    // reference port.
    // For nfs:
    // If the file's modify time on the server has changed since the last read rpc or you
    // have written to the file, you may have lost data cache consistency with the server, so
    // flush all of the file's data out of the cache. Then force a getattr rpc to ensure that
    // you have up to date attributes.
    let mut vattr = Vattr::new();
    if np.isset(NMODIFIED) {
        nfs_invalidate_attrcache(np);
        VOP_GETATTR(vp, &mut vattr, cred, getattr_proc(p))?;
        np.n_mtime.set(vattr.va_mtime);
    } else {
        VOP_GETATTR(vp, &mut vattr, cred, getattr_proc(p))?;
        if np.n_mtime.get() != vattr.va_mtime {
            nfs_vinvalbuf(vp, V_SAVE, cred, p)?;
            np.n_mtime.set(vattr.va_mtime);
        }
    }

    // update the cache read creds for this vnode
    nfs_crfree(np.n_rcred.get());
    np.n_rcred.set(cred);
    nfs_crhold(cred);

    loop {
        if vp.v_flag.get() & VROOT != 0 && vp.v_type.get() == VLNK {
            return nfs_readlinkrpc(vp, uio, cred);
        }
        let (bp, on, mut n) = match vp.v_type.get() {
            VREG => {
                NFSSTATS.biocache_reads.fetch_add(1, Ordering::Relaxed);
                let (lbn, on, bn) = nfs_bioblk(uio.uio_offset, biosize);
                let mut not_readin = true;

                // Start the read ahead(s), as required.
                if nfs_numasync() > 0 && nmp.nm_readahead.get() > 0 {
                    let mut nra = 0;
                    while nra < nmp.nm_readahead.get()
                        && (((lbn + 1 + Daddr::from(nra)) * Daddr::from(biosize)) as u64)
                            < np.n_size.get()
                    {
                        let rabn =
                            (lbn + 1 + Daddr::from(nra)) * Daddr::from(biosize / DEV_BSIZE as i32);
                        if incore(vp, rabn).is_none() {
                            let Some(rabp) = nfs_getcacheblk(vp, rabn, biosize, p) else {
                                return Err(Errno::EINTR);
                            };
                            if !rabp.isset(B_DELWRI | B_DONE) {
                                rabp.set(B_READ | B_ASYNC);
                                if nfs_asyncio(rabp, true).is_err() {
                                    rabp.set(B_INVAL);
                                    brelse(rabp);
                                }
                            } else {
                                brelse(rabp);
                            }
                        }
                        nra += 1;
                    }
                }

                // again:
                let (bp, n) = loop {
                    let Some(bp) = nfs_getcacheblk(vp, bn, biosize, p) else {
                        return Err(Errno::EINTR);
                    };
                    if !bp.isset(B_DONE | B_DELWRI) {
                        bp.set(B_READ);
                        not_readin = false;
                        if let Err(error) = nfs_doio(bp, p) {
                            brelse(bp);
                            return Err(error);
                        }
                    }
                    let mut n = min((biosize - on) as usize, uio.uio_resid) as i32;
                    // The C subtracts in u_quad_t and keeps the off_t.
                    let offdiff = (np.n_size.get() as Off).wrapping_sub(uio.uio_offset);
                    if offdiff < Off::from(n) {
                        n = offdiff as i32;
                    }
                    if not_readin
                        && n > 0
                        && (on < bp.b_validoff.get() || (on + n) > bp.b_validend.get())
                    {
                        bp.set(B_INVAFTERWRITE);
                        if bp.b_dirtyend.get() > 0 {
                            if !bp.isset(B_DELWRI) {
                                panic(format_args!("nfsbioread"));
                            }
                            if VOP_BWRITE(bp) == Err(Errno::EINTR) {
                                return Err(Errno::EINTR);
                            }
                        } else {
                            brelse(bp);
                        }
                        continue;
                    }
                    let diff = if on >= bp.b_validend.get() {
                        0
                    } else {
                        bp.b_validend.get() - on
                    };
                    if diff < n {
                        n = diff;
                    }
                    break (bp, n);
                };
                (bp, on, n)
            }
            VLNK => {
                NFSSTATS.biocache_readlinks.fetch_add(1, Ordering::Relaxed);
                let Some(bp) = nfs_getcacheblk(vp, 0, NFS_MAXPATHLEN as i32, p) else {
                    return Err(Errno::EINTR);
                };
                if !bp.isset(B_DONE) {
                    bp.set(B_READ);
                    if let Err(error) = nfs_doio(bp, p) {
                        brelse(bp);
                        return Err(error);
                    }
                }
                let n = min(uio.uio_resid, NFS_MAXPATHLEN.wrapping_sub(bp.b_resid.get())) as i32;
                (bp, 0, n)
            }
            t => panic(format_args!("nfsbioread: type {:x} unexpected", t as i32)),
        };

        let mut error = Ok(());
        if n > 0 {
            // SAFETY: `nfs_getcacheblk` returned the buffer busy for this thread and mapped;
            // the slice is dropped before the buffer is released.
            let data = unsafe { bp.data() };
            error = uiomove(&mut data[on as usize..(on + n) as usize], uio);
        }

        if vp.v_type.get() == VLNK {
            n = 0;
        }

        brelse(bp);
        if !(error.is_ok() && uio.uio_resid > 0 && n > 0) {
            return error;
        }
    }
}

/// `nfs_write` (`vop_write`): vnode op for write using bio.
pub fn nfs_write(ap: &mut VopWriteArgs<'_, '_>) -> Result<(), Errno> {
    let uio = &mut *ap.a_uio;
    let p = uio.uio_procp;
    let vp = ap.a_vp;
    let np = VTONFS(vp);
    let cred = ap.a_cred;
    let ioflag = ap.a_ioflag;
    let nmp = vfstonfs_vp(vp);
    let (mut extended, mut truncated) = (false, false);
    let mut wrotedta;

    #[cfg(feature = "diagnostic")]
    {
        if uio.uio_rw != UioRw::UIO_WRITE {
            panic(format_args!("nfs_write mode"));
        }
        if uio.uio_segflg == UioSeg::UIO_USERSPACE
            && uio.uio_procp.map(ptr::from_ref) != curproc().map(ptr::from_ref)
        {
            panic(format_args!("nfs_write proc"));
        }
    }
    if vp.v_type.get() != VREG {
        return Err(Errno::EIO);
    }
    if np.isset(NWRITEERR) {
        np.clr(NWRITEERR);
        return match np.n_error.get() {
            0 => Ok(()),
            e => Err(Errno::from_raw(e).unwrap_or(Errno::EIO)),
        };
    }
    if nmp.nm_flag.get() & (NFSMNT_NFSV3 | NFSMNT_GOTFSINFO) == NFSMNT_NFSV3 {
        let _ = nfs_fsinfo(nmp, vp, cred, p);
    }
    if ioflag & (IO_APPEND | IO_SYNC) != 0 {
        if np.isset(NMODIFIED) {
            nfs_invalidate_attrcache(np);
            nfs_vinvalbuf(vp, V_SAVE, cred, p)?;
        }
        if ioflag & IO_APPEND != 0 {
            nfs_invalidate_attrcache(np);
            let mut vattr = Vattr::new();
            VOP_GETATTR(vp, &mut vattr, cred, getattr_proc(p))?;
            uio.uio_offset = np.n_size.get() as Off;
        }
    }
    if uio.uio_offset < 0 {
        return Err(Errno::EINVAL);
    }
    if uio.uio_resid == 0 {
        return Ok(());
    }

    // do the filesize rlimit check
    let overrun = vn_fsizechk(vp, uio, ioflag)?;

    // update the cache write creds for this node.
    nfs_crfree(np.n_wcred.get());
    np.n_wcred.set(cred);
    nfs_crhold(cred);

    // I use nm_rsize, not nm_wsize so that all buffer cache blocks will be the same size
    // within a filesystem. nfs_writerpc will still use nm_wsize when sizing the rpc's.
    let biosize = nmp.nm_rsize.get();
    let error: Result<(), Errno> = 'out: {
        loop {
            // XXX make sure we aren't cached in the VM page cache
            let _ = uvm_vnp_uncache(vp);

            NFSSTATS.biocache_writes.fetch_add(1, Ordering::Relaxed);
            let (_lbn, on, bn) = nfs_bioblk(uio.uio_offset, biosize);
            let n = min((biosize - on) as usize, uio.uio_resid) as i32;
            // again:
            let bp = loop {
                let Some(bp) = nfs_getcacheblk(vp, bn, biosize, p) else {
                    break 'out Err(Errno::EINTR);
                };
                np.set(NMODIFIED);
                let end = (uio.uio_offset + Off::from(n)) as u64;
                if end > np.n_size.get() {
                    np.n_size.set(end);
                    uvm_vnp_setsize(vp, np.n_size.get() as Off);
                    extended = true;
                } else if end < np.n_size.get() {
                    truncated = true;
                }

                // If the new write will leave a contiguous dirty area, just update the
                // b_dirtyoff and b_dirtyend, otherwise force a write rpc of the old dirty
                // area.
                if bp.b_dirtyend.get() > 0
                    && (on > bp.b_dirtyend.get() || (on + n) < bp.b_dirtyoff.get())
                {
                    set_b_proc(bp, p);
                    if VOP_BWRITE(bp) == Err(Errno::EINTR) {
                        break 'out Err(Errno::EINTR);
                    }
                    continue;
                }
                break bp;
            };

            {
                // SAFETY: `nfs_getcacheblk` returned the buffer busy for this thread and
                // mapped; the slice is dropped before the buffer is released or written.
                let data = unsafe { bp.data() };
                if let Err(e) = uiomove(&mut data[on as usize..(on + n) as usize], uio) {
                    bp.set(B_ERROR);
                    brelse(bp);
                    break 'out Err(e);
                }
            }
            let (dirtyoff, dirtyend, validoff, validend) = nfs_bufranges(
                (
                    bp.b_dirtyoff.get(),
                    bp.b_dirtyend.get(),
                    bp.b_validoff.get(),
                    bp.b_validend.get(),
                ),
                on,
                n,
            );
            bp.b_dirtyoff.set(dirtyoff);
            bp.b_dirtyend.set(dirtyend);
            bp.b_validoff.set(validoff);
            bp.b_validend.set(validend);

            wrotedta = true;

            // Since this block is being modified, it must be written again and not just
            // committed.
            if nfs_isv3(vp) {
                rw_enter_write(&np.n_commitlock);
                if bp.isset(B_NEEDCOMMIT) {
                    bp.clr(B_NEEDCOMMIT);
                    nfs_del_tobecommitted_range(vp, bp);
                }
                nfs_del_committed_range(vp, bp);
                rw_exit_write(&np.n_commitlock);
            } else {
                bp.clr(B_NEEDCOMMIT);
            }

            if ioflag & IO_SYNC != 0 {
                set_b_proc(bp, p);
                if let Err(e) = VOP_BWRITE(bp) {
                    break 'out Err(e);
                }
            } else if n + on == biosize {
                set_b_proc(bp, None);
                bp.set(B_ASYNC);
                let _ = nfs_writebp(bp, false);
            } else {
                bdwrite(bp);
            }
            if !(uio.uio_resid > 0 && n > 0) {
                break;
            }
        }

        // out: XXX belongs here???
        if wrotedta {
            VN_KNOTE(
                vp,
                NOTE_WRITE
                    | if extended { NOTE_EXTEND } else { 0 }
                    | if truncated { NOTE_TRUNCATE } else { 0 },
            );
        }
        Ok(())
    };

    // out: correct the result for writes clamped by vn_fsizechk()
    uio.uio_resid = (uio.uio_resid as isize + overrun) as usize;

    error
}

/// `nfs_getcacheblk(vp, bn, size, p)`: get an nfs cache block. Allocate a new one if the
/// block isn't currently in the cache and return the block marked busy. If the calling
/// process is interrupted by a signal for an interruptible mount point, return `None`.
pub fn nfs_getcacheblk(
    vp: &'static Vnode,
    bn: Daddr,
    size: i32,
    p: Option<&Proc>,
) -> Option<&'static Buf> {
    let nmp = vfstonfs_vp(vp);

    if nmp.nm_flag.get() & NFSMNT_INT != 0 {
        let mut bp = getblk(vp, bn, size, PCATCH, INFSLP);
        while bp.is_none() {
            if nfs_sigintr(nmp, None, p).is_err() {
                return None;
            }
            bp = getblk(vp, bn, size, 0, sec_to_nsec(2));
        }
        bp
    } else {
        getblk(vp, bn, size, 0, INFSLP)
    }
}

/// `nfs_vinvalbuf(vp, flags, cred, p)`: flush and invalidate all dirty buffers. If another
/// process is already doing the flush, just wait for completion.
pub fn nfs_vinvalbuf(
    vp: &'static Vnode,
    flags: i32,
    cred: *const Ucred,
    p: Option<&Proc>,
) -> Result<(), Errno> {
    let nmp = vfstonfs_vp(vp);
    let np = VTONFS(vp);
    let mut stimeo = INFSLP;
    let mut sintr = 0;

    if nmp.nm_flag.get() & NFSMNT_INT != 0 {
        sintr = PCATCH;
        stimeo = sec_to_nsec(2);
    }

    // First wait for any other process doing a flush to complete.
    while np.isset(NFLUSHINPROG) {
        np.set(NFLUSHWANT);
        let error = tsleep_nsec(
            ptr::from_ref(&np.n_flag),
            PRIBIO | sintr,
            "nfsvinval",
            stimeo,
        );
        if error.is_err() && sintr != 0 && nfs_sigintr(nmp, None, p).is_err() {
            return Err(Errno::EINTR);
        }
    }

    // Now, flush as required.
    np.set(NFLUSHINPROG);
    let mut error = vinvalbuf(vp, flags, cred, p, sintr, INFSLP);
    while error.is_err() {
        if sintr != 0 && nfs_sigintr(nmp, None, p).is_err() {
            np.clr(NFLUSHINPROG);
            if np.isset(NFLUSHWANT) {
                np.clr(NFLUSHWANT);
                wakeup(ptr::from_ref(&np.n_flag));
            }
            return Err(Errno::EINTR);
        }
        error = vinvalbuf(vp, flags, cred, p, 0, stimeo);
    }
    np.clr(NMODIFIED | NFLUSHINPROG);
    if np.isset(NFLUSHWANT) {
        np.clr(NFLUSHWANT);
        wakeup(ptr::from_ref(&np.n_flag));
    }
    Ok(())
}

/// `nfs_asyncio(bp, readahead)`: initiate asynchronous I/O. Return an error if no nfsiods
/// are available. This is mainly to avoid queueing async I/O requests when the nfsiods are
/// all hung on a dead server.
pub fn nfs_asyncio(bp: &'static Buf, readahead: bool) -> Result<(), Errno> {
    'out: {
        if nfs_numasync() == 0 {
            break 'out;
        }

        while NFS_BUFQLEN.load(Ordering::Relaxed) > NFS_BUFQMAX.load(Ordering::Relaxed) {
            if readahead {
                break 'out;
            }
            let _ = tsleep_nsec(ptr::from_ref(&NFS_BUFQLEN), PRIBIO, "nfs_bufq", INFSLP);
        }

        if !bp.isset(B_READ) {
            bp.set(B_WRITEINPROG);
        }

        // SAFETY: the caller owns the busy buffer, so it is on none of the cache's free-list
        // queues (`bufcache_take` took it off) nor on `nfs_bufq`; an nfsiod unlinks it before
        // `nfs_doio` completes it, and buffers are pool items that stay in place.
        unsafe { NFS_BUFQ.0.insert_tail(bp) };
        NFS_BUFQLEN.fetch_add(1, Ordering::Relaxed);

        wakeup_one(ptr::from_ref(&NFS_BUFQ));
        return Ok(());
    }

    // out:
    NFSSTATS.forcedsync.fetch_add(1, Ordering::Relaxed);
    Err(Errno::EIO)
}

/// A one-iovec kernel-space uio over `len` bytes at `base`.
fn nfs_sysuio<'a>(
    io: &'a mut [Iovec; 1],
    base: *mut u8,
    len: usize,
    offset: Off,
    rw: UioRw,
    p: Option<&'a Proc>,
) -> Uio<'a> {
    io[0] = Iovec {
        iov_base: base.cast::<c_void>(),
        iov_len: len,
    };
    Uio {
        uio_iov: io,
        uio_offset: offset,
        uio_resid: len,
        uio_segflg: UioSeg::UIO_SYSSPACE,
        uio_rw: rw,
        uio_procp: p,
    }
}

/// `nfs_doio(bp, p)`: do an I/O operation to/from a cache block. This may be called
/// synchronously or from an nfsiod (`p` is then `None`).
pub fn nfs_doio(bp: &'static Buf, p: Option<&Proc>) -> Result<(), Errno> {
    let Some(vp) = bp.b_vp.get() else {
        panic(format_args!("nfs_doio: buffer {:p} without a vnode", bp));
    };
    let np = VTONFS(vp);
    let error: Result<(), Errno>;
    let mut must_commit = false;
    let mut io = [Iovec {
        iov_base: ptr::null_mut(),
        iov_len: 0,
    }];
    let blkoff = bp.b_blkno.get() << DEV_BSHIFT;

    // Historically, paging was done with physio, but no more.
    let resid = if bp.isset(B_PHYS) {
        let len = bp.b_bcount.get() as usize;
        // mapping was done by vmapbuf()
        let rw = if bp.isset(B_READ) {
            UioRw::UIO_READ
        } else {
            UioRw::UIO_WRITE
        };
        let mut uio = nfs_sysuio(&mut io, bp.b_data.get(), len, blkoff, rw, p);
        if bp.isset(B_READ) {
            NFSSTATS.read_physios.fetch_add(1, Ordering::Relaxed);
            error = nfs_readrpc(vp, &mut uio);
        } else {
            let mut iomode = NFSV3WRITE_DATASYNC;
            NFSSTATS.write_physios.fetch_add(1, Ordering::Relaxed);
            error = nfs_writerpc(vp, &mut uio, &mut iomode, &mut must_commit);
        }
        if let Err(e) = error {
            bp.set(B_ERROR);
            bp.b_error.set(Some(e));
        }
        uio.uio_resid
    } else if bp.isset(B_READ) {
        let len = bp.b_bcount.get() as usize;
        let mut uio = nfs_sysuio(&mut io, bp.b_data.get(), len, 0, UioRw::UIO_READ, p);
        match vp.v_type.get() {
            VREG => {
                uio.uio_offset = blkoff;
                NFSSTATS.read_bios.fetch_add(1, Ordering::Relaxed);
                BCSTATS.pendingreads.fetch_add(1, Ordering::Relaxed);
                BCSTATS.numreads.fetch_add(1, Ordering::Relaxed);
                error = nfs_readrpc(vp, &mut uio);
                if error.is_ok() {
                    bp.b_validoff.set(0);
                    if uio.uio_resid != 0 {
                        // If len > 0, there is a hole in the file and no writes after the
                        // hole have been pushed to the server yet. Just zero fill the rest
                        // of the valid area.
                        let (zero, validend) = nfs_shortread(
                            bp.b_bcount.get(),
                            uio.uio_resid,
                            np.n_size.get(),
                            blkoff,
                        );
                        if zero > 0 {
                            let diff = validend as usize - zero;
                            // SAFETY: the buffer is busy for this I/O and mapped; the read
                            // into it is over and no other slice of it is alive.
                            let data = unsafe { bp.data() };
                            data[diff..diff + zero].fill(0);
                        }
                        bp.b_validend.set(validend);
                    } else {
                        bp.b_validend.set(bp.b_bcount.get() as i32);
                    }
                }
                if let Some(p) = p
                    && vp.v_flag.get() & VTEXT != 0
                    && np.n_mtime.get() != np.n_vattr.get().va_mtime
                {
                    uprintf(format_args!(
                        "Process killed due to text file modification\n"
                    ));
                    psignal(p, SIGKILL);
                }
            }
            VLNK => {
                uio.uio_offset = 0;
                NFSSTATS.readlink_bios.fetch_add(1, Ordering::Relaxed);
                BCSTATS.pendingreads.fetch_add(1, Ordering::Relaxed);
                BCSTATS.numreads.fetch_add(1, Ordering::Relaxed);
                let cred = curproc().map_or(NOCRED, |cp| cp.p_ucred.get());
                error = nfs_readlinkrpc(vp, &mut uio, cred);
            }
            t => panic(format_args!("nfs_doio:  type {:x} unexpected", t as i32)),
        }
        if let Err(e) = error {
            bp.set(B_ERROR);
            bp.b_error.set(Some(e));
        }
        uio.uio_resid
    } else {
        let dirtyoff = bp.b_dirtyoff.get();
        let len = (bp.b_dirtyend.get() - dirtyoff) as usize;
        let offset = bp.b_blkno.get() * DEV_BSIZE as Daddr + Off::from(dirtyoff);
        let base = bp.b_data.get().wrapping_add(dirtyoff as usize);
        let mut uio = nfs_sysuio(&mut io, base, len, offset, UioRw::UIO_WRITE, p);
        NFSSTATS.write_bios.fetch_add(1, Ordering::Relaxed);
        BCSTATS.pendingwrites.fetch_add(1, Ordering::Relaxed);
        BCSTATS.numwrites.fetch_add(1, Ordering::Relaxed);
        let mut iomode = if bp.b_flags.get() & (B_ASYNC | B_NEEDCOMMIT | B_NOCACHE) == B_ASYNC {
            NFSV3WRITE_UNSTABLE
        } else {
            NFSV3WRITE_FILESYNC
        };
        bp.set(B_WRITEINPROG);
        error = nfs_writerpc(vp, &mut uio, &mut iomode, &mut must_commit);

        rw_enter_write(&np.n_commitlock);
        if error.is_ok() && iomode == NFSV3WRITE_UNSTABLE {
            bp.set(B_NEEDCOMMIT);
            nfs_add_tobecommitted_range(vp, bp);
        } else {
            bp.clr(B_NEEDCOMMIT);
            nfs_del_committed_range(vp, bp);
        }
        rw_exit_write(&np.n_commitlock);

        bp.clr(B_WRITEINPROG);

        // For an interrupted write, the buffer is still valid and the write hasn't been
        // pushed to the server yet, so we can't set B_ERROR and report the interruption by
        // setting B_EINTR. For the B_ASYNC case, B_EINTR is not relevant, so the rpc attempt
        // is essentially a noop.
        // For the case of a V3 write rpc not being committed to stable storage, the block is
        // still dirty and requires either a commit rpc or another write rpc with iomode ==
        // NFSV3WRITE_FILESYNC before the block is reused. This is indicated by setting the
        // B_DELWRI and B_NEEDCOMMIT flags.
        if error == Err(Errno::EINTR) || (error.is_ok() && bp.isset(B_NEEDCOMMIT)) {
            let s = splbio();
            buf_dirty(bp);
            splx(s);

            if !bp.isset(B_ASYNC) && error.is_err() {
                bp.set(B_EINTR);
            }
        } else {
            if let Err(e) = error {
                bp.set(B_ERROR);
                bp.b_error.set(Some(e));
                np.n_error.set(e as i32);
                np.set(NWRITEERR);
            }
            bp.b_dirtyoff.set(0);
            bp.b_dirtyend.set(0);
        }
        uio.uio_resid
    };
    bp.b_resid.set(resid);
    if must_commit && let Some(mp) = vp.v_mount.get() {
        nfs_clearcommit(mp);
    }
    let s = splbio();
    biodone(bp);
    splx(s);
    error
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_arithmetic() {
        // 8k blocks: 16 DEV_BSIZE sectors each.
        assert_eq!(nfs_bioblk(0, 8192), (0, 0, 0));
        assert_eq!(nfs_bioblk(8191, 8192), (0, 8191, 0));
        assert_eq!(nfs_bioblk(8192, 8192), (1, 0, 16));
        assert_eq!(nfs_bioblk(3 * 8192 + 100, 8192), (3, 100, 48));
        assert_eq!(nfs_bioblk(1 << 40, 32768), (1 << 25, 0, (1 << 25) * 64));
    }

    #[test]
    fn write_ranges_merge() {
        // A clean buffer: the write is the dirty and the valid range.
        assert_eq!(nfs_bufranges((0, 0, 0, 0), 100, 50), (100, 150, 100, 150));
        // Contiguous with the dirty range: it grows; the valid range covers it.
        assert_eq!(
            nfs_bufranges((100, 150, 0, 8192), 150, 10),
            (100, 160, 0, 8192)
        );
        assert_eq!(
            nfs_bufranges((100, 150, 100, 150), 50, 50),
            (50, 150, 50, 150)
        );
        // Valid data apart from the dirty range is forgotten.
        assert_eq!(nfs_bufranges((0, 0, 0, 10), 100, 10), (100, 110, 100, 110));
        // Overlapping valid range: the union.
        assert_eq!(nfs_bufranges((0, 0, 50, 120), 100, 50), (100, 150, 50, 150));
    }

    #[test]
    fn short_read_zero_fills_to_the_file_size() {
        // 8k buffer at offset 8192, 3000 bytes read, file 8192 + 5000 long: zero 2000 more.
        assert_eq!(
            nfs_shortread(8192, 8192 - 3000, 8192 + 5000, 8192),
            (2000, 5000)
        );
        // The file ends where the data ends: nothing to zero.
        assert_eq!(
            nfs_shortread(8192, 8192 - 3000, 8192 + 3000, 8192),
            (0, 3000)
        );
        // The file is longer than the buffer: zero to the end of the buffer.
        assert_eq!(nfs_shortread(8192, 192, 1 << 20, 0), (192, 8192));
    }
}
/* </TESTS> */
