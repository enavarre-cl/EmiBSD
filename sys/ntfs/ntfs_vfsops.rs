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

/*-
 * Copyright (c) 1998, 1999 Semen Ustimenko (semenu@FreeBSD.org)
 * All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 *	Id: ntfs_vfsops.h,v 1.3 1999/05/12 09:43:06 semenu Exp
 */
/*-
 * Copyright (c) 1998, 1999 Semen Ustimenko
 * All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 *	Id: ntfs_vfsops.c,v 1.7 1999/05/31 11:28:30 phk Exp
 */
/* </LICENSES> */

/* <CODE> */
//! NTFS file-system-type operations: mount (the boot file, the system vnodes of `$MFT`, the
//! root and `$Bitmap`, the upper-case table, the free cluster count, the attribute
//! definitions of `$AttrDef`), unmount, root, `statfs`, `vget` (an attribute of an MFT record
//! into a vnode) and file handles.
//!
//! Upstream: sys/ntfs/ntfs_vfsops.h @ 3ce1f3f79392, sys/ntfs/ntfs_vfsops.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - The header and the file share this module, as `.h`/`.c` pairs do (`siphash.rs`).
//! - `ntfs_mount` reads its `struct ntfs_args` out of the kernel copy of the mount arguments
//!   (`NtfsArgs::from_bytes`), `EINVAL` when a mount that is not an update has none (the C
//!   dereferences NULL); it looks the device up with a `nameidata` of its own (`ndinit`), as
//!   `msdosfs_mount` does, instead of reinitialising the caller's. `bcopy(args,
//!   &mp->mnt_stat.mount_info.ntfs_args, ...)` copies the argument bytes into `mount_info`.
//!   `VFS_STATFS(mp, &mp->mnt_stat, p)` works on a copy of `mnt_stat` stored back.
//! - `ntfs_root`, `ntfs_vget`, `ntfs_fhtovp` and `ntfs_vgetex` return the vnode (`Vfsops`'s
//!   shape); `ntfs_calccfree` returns the count.
//! - `ntfs_mountfs`: a boot file with a zero sector size is `EINVAL` (the C divides by it);
//!   `1 << -cpr` shifts modulo 32, as the machines do. When reading `$AttrDef` fails, its
//!   vnode is released before the error path (the C keeps it referenced, so `vflush` cannot
//!   recycle it, and it would outlive the `ntfsmount` it points at); the name copy stops at
//!   `NTFS_ATTRNAME_MAXLEN` characters.
//! - `ntfs_vgetex`'s `f_type` is `VNON` when the fnode is not validated (the C leaves it
//!   uninitialised; its only such caller, `ntfs_ntlookupfile`, sets `v_type` itself). When
//!   `getnewvnode` fails it releases the ntnode a second time, as the C does.
//! - `ntfs_vptofh` writes the length, the number and the attribute into the `struct fid`
//!   ([`Ntfid`]), as the C's stores through the cast do.
//! - `ntfs_sysctl` keeps the C's `EINVAL` for every name.
//! - The `DPRINTF`/`DDPRINTF`s (`NTFS_DEBUG`, off) are left out.

use core::ffi::c_void;
use core::ptr;

use crate::kern::init_main::rootvp;
use crate::kern::kern_malloc::{free, mallocarray};
use crate::kern::subr_disk::disk_map;
use crate::kern::subr_prf::{panic, printf};
use crate::kern::vfs_bio::{bread, brelse};
use crate::kern::vfs_lookup::{namei, ndinit};
use crate::kern::vfs_subr::{
    copy_statfs_info, getnewvnode, vcount, vflush, vfs_export, vfs_export_lookup, vfs_mountedon,
    vget, vinvalbuf, vput, vref, vrele,
};
use crate::kern::vfs_vnops::vn_lock;
use crate::kern::vfs_vops::{VOP_CLOSE, VOP_OPEN, VOP_UNLOCK};
use crate::machine::conf::nblkdev;
use crate::machine::copy::copyinstr;
use crate::ntfs::ntfs::{
    Attrdef, BBLOCK, BBSIZE, Bootfile, Cn, NTFS_A_DATA, NTFS_ATTRDEFINO, NTFS_ATTRNAME_MAXLEN,
    NTFS_BBID, NTFS_BBIDLEN, NTFS_BITMAPINO, NTFS_FRFLAG_DIR, NTFS_MAXFILENAME, NTFS_MFTINO,
    NTFS_ROOTINO, Ntfsino, Ntfsmount, Ntvattrdef, Packed, VFSTONTFS, VTOF, VTONT,
};
use crate::ntfs::ntfs_conv::{ntfs_utf8_wcmp, ntfs_utf8_wget, ntfs_utf8_wput};
use crate::ntfs::ntfs_ihash::ntfs_nthashinit;
use crate::ntfs::ntfs_inode::{AttrNameBuf, FN_VALID, IN_LOADED, Ntfid};
use crate::ntfs::ntfs_subr::{
    Rdata, ntfs_alloc, ntfs_dealloc, ntfs_fget, ntfs_filesize, ntfs_frele, ntfs_loadntnode,
    ntfs_ntlookup, ntfs_ntput, ntfs_readattr, ntfs_toupper_unuse, ntfs_toupper_use,
};
use crate::ntfs::ntfs_vnops::NTFS_VOPS;
use crate::sys::disk::DM_OPENBLCK;
use crate::sys::errno::Errno;
use crate::sys::fcntl::FREAD;
use crate::sys::lock::{LK_EXCLUSIVE, LK_RETRY, LK_TYPE_MASK};
use crate::sys::malloc::{M_NTFSMNT, M_TEMP, M_WAITOK, M_ZERO};
use crate::sys::mbuf::Mbuf;
use crate::sys::mount::{
    Fid, MNAMELEN, MNT_FORCE, MNT_LOCAL, MNT_UPDATE, Mount, NtfsArgs, Statfs, VFS_STATFS, VFS_VGET,
    Vfsconf, Vfsops,
};
use crate::sys::namei::{FOLLOW, LOOKUP, Nameidata, NiDirp};
use crate::sys::proc::Proc;
use crate::sys::systm::INFSLP;
use crate::sys::types::{Ino, Uid, major};
use crate::sys::ucred::{FSCRED, NOCRED, Ucred};
use crate::sys::vnode::{
    FORCECLOSE, SKIPSYSTEM, V_SAVE, VBAD, VBLK, VDIR, VNON, VREG, VROOT, VSYSTEM, VT_NTFS, Vnode,
    Vtype,
};

/// `VG_DONTLOADIN`: tells `ntfs_vgetex` to do not call `ntfs_loadntnode()` on ntnode, even if
/// ntnode not loaded.
pub const VG_DONTLOADIN: u64 = 0x0001;
/// `VG_DONTVALIDFN`: tells `ntfs_vgetex` to do not validate fnode.
pub const VG_DONTVALIDFN: u64 = 0x0002;
/// `VG_EXT`: this is not main record.
pub const VG_EXT: u64 = 0x0004;

/// `ntfs_vfsops`.
pub static NTFS_VFSOPS: Vfsops = Vfsops {
    vfs_mount: ntfs_mount,
    vfs_start: ntfs_start,
    vfs_unmount: ntfs_unmount,
    vfs_root: ntfs_root,
    vfs_quotactl: ntfs_quotactl,
    vfs_statfs: ntfs_statfs,
    vfs_sync: ntfs_sync,
    vfs_vget: ntfs_vget,
    vfs_fhtovp: ntfs_fhtovp,
    vfs_vptofh: ntfs_vptofh,
    vfs_init: Some(ntfs_init),
    vfs_sysctl: Some(ntfs_sysctl),
    vfs_checkexp: ntfs_checkexp,
};

/// `ntfs_checkexp` (`vfs_checkexp`): verify a remote client has export rights and return
/// these rights via `exflagsp` and `credanonp`.
pub fn ntfs_checkexp(
    mp: &'static Mount,
    nam: &Mbuf,
    exflagsp: &mut i32,
    credanonp: &mut *const Ucred,
) -> Result<(), Errno> {
    let ntm = VFSTONTFS(mp);

    // Get the export permission structure for this <mp, client> tuple.
    let Some(np) = vfs_export_lookup(mp, &ntm.ntm_export, Some(nam)) else {
        return Err(Errno::EACCES);
    };

    *exflagsp = np.netc_exflags.get();
    *credanonp = ptr::from_ref(&np.netc_anon);
    Ok(())
}

/// `ntfs_sysctl` (`vfs_sysctl`).
pub fn ntfs_sysctl(
    _name: &[i32],
    _oldp: usize,
    _oldlenp: &mut usize,
    _newp: usize,
    _newlen: usize,
    _p: &Proc,
) -> Result<(), Errno> {
    Err(Errno::EINVAL)
}

/// `ntfs_init` (`vfs_init`).
pub fn ntfs_init(_vcp: &'static Vfsconf) -> Result<(), Errno> {
    Ok(())
}

/// `bzero(dst, MNAMELEN); strlcpy(dst, src, MNAMELEN)`.
fn mname_copy(dst: &mut [u8; MNAMELEN], src: &[u8]) {
    *dst = [0; MNAMELEN];
    let src = src.split(|&c| c == 0).next().unwrap_or(&[]);
    let n = src.len().min(MNAMELEN - 1);
    dst[..n].copy_from_slice(&src[..n]);
}

/// `ntfs_mount` (`vfs_mount`): mount an NTFS volume at `path`. `data` is the kernel copy of
/// the user's `struct ntfs_args`.
pub fn ntfs_mount(
    mp: &'static Mount,
    path: &[u8],
    data: &mut [u8],
    _ndp: &mut Nameidata<'_>,
    p: &Proc,
) -> Result<(), Errno> {
    let args = NtfsArgs::from_bytes(data);
    let mut fname = [0u8; MNAMELEN];
    let mut fspec = [0u8; MNAMELEN];

    ntfs_nthashinit();

    // Mounting non-root file system or updating a file system

    // If updating, check whether changing from read-only to read/write; if there is no
    // device name, that's all we do.
    if mp.mnt_flag.get() & MNT_UPDATE != 0 {
        // if not updating name...
        if let Some(a) = args
            && a.fspec == 0
        {
            // Process export requests. Jumping to "success" will return the vfs_export()
            // error code.
            let ntm = VFSTONTFS(mp);
            return vfs_export(mp, &ntm.ntm_export, &a.export_info);
        }

        printf(format_args!("ntfs_mount(): MNT_UPDATE not supported\n"));
        return Err(Errno::EINVAL);
    }

    // Not an update, or updating the name: look up the name and verify that it refers to a
    // sensible block device.
    let Some(args) = args else {
        return Err(Errno::EINVAL);
    };
    copyinstr(args.fspec, &mut fspec)?;

    if !disk_map(&fspec, &mut fname, DM_OPENBLCK) {
        fname = fspec;
    }

    let flen = fname.iter().position(|&c| c == 0).unwrap_or(MNAMELEN);
    let mut nd = ndinit(LOOKUP, FOLLOW, NiDirp::Sys(&fname[..flen]), p);
    // can't get devvp!
    namei(&mut nd)?;
    let Some(devvp) = nd.ni_vp else {
        return Err(Errno::ENOENT);
    };

    let error: Result<(), Errno> = 'error_2: {
        if devvp.v_type.get() != VBLK {
            break 'error_2 Err(Errno::ENOTBLK);
        }

        if major(devvp.v_rdev()) >= nblkdev() {
            break 'error_2 Err(Errno::ENXIO);
        }

        // UPDATE: `#if 0` in the C (an update returned above).

        // NEW MOUNT

        // Since this is a new mount, we want the names for the device and the mount point
        // copied in. If an error occurs, the mountpoint is discarded by the upper level
        // code. Save "last mounted on" info for mount point (NULL pad).
        mp.update_stat(|sp| {
            mname_copy(&mut sp.f_mntonname, path);
            mname_copy(&mut sp.f_mntfromname, &fname);
            mname_copy(&mut sp.f_mntfromspec, &fspec);
            sp.mount_info.__align[..NtfsArgs::SIZE].copy_from_slice(&data[..NtfsArgs::SIZE]);
        });
        ntfs_mountfs(devvp, mp, &args, p)
    };
    if let Err(e) = error {
        // error_2: error with devvp held; release devvp before failing
        vrele(devvp);
        return Err(e);
    }

    // Initialize FS stat information in mount struct; uses both mp->mnt_stat.f_mntonname
    // and mp->mnt_stat.f_mntfromname
    //
    // This code is common to root and non-root mounts
    let mut sp = mp.mnt_stat.get();
    let _ = VFS_STATFS(mp, &mut sp, p);
    mp.mnt_stat.set(sp);

    Ok(())
}

/// `ntfs_mountfs`: common code for mount and mountroot.
pub fn ntfs_mountfs(
    devvp: &'static Vnode,
    mp: &'static Mount,
    argsp: &NtfsArgs,
    p: &Proc,
) -> Result<(), Errno> {
    let dev = devvp.v_rdev();

    // Disallow multiple mounts of the same device. Disallow mounting of a device that is
    // currently in use (except for root, which might share swap device for miniroot). Flush
    // out any old buffers remaining from a previous use.
    vfs_mountedon(devvp)?;
    let ncount = vcount(devvp);
    if ncount > 1 && !rootvp().is_some_and(|r| ptr::eq(r, devvp)) {
        return Err(Errno::EBUSY);
    }
    let _ = vn_lock(devvp, LK_EXCLUSIVE | LK_RETRY);
    let error = vinvalbuf(devvp, V_SAVE, p.p_ucred.get(), Some(p), 0, INFSLP);
    let _ = VOP_UNLOCK(devvp);
    error?;

    VOP_OPEN(devvp, FREAD, FSCRED, p)?;

    let mut ntmp: Option<&'static Ntfsmount> = None;

    let error: Result<(), Errno> = 'out: {
        let (bp, e) = bread(devvp, BBLOCK, BBSIZE);
        if let Err(e) = e {
            brelse(bp);
            break 'out Err(e);
        }
        let nt = ntfs_alloc(Ntfsmount::new(), M_NTFSMNT);
        ntmp = Some(nt);
        {
            // SAFETY: the buffer is ours (busy from bread) and mapped; the slice dies before
            // the release below.
            let b = unsafe { bp.data() };
            nt.ntm_bootfile.set(Bootfile::read(b, 0));
        }
        brelse(bp);

        if nt.ntm_bootfile.get().bf_sysid[..NTFS_BBIDLEN] != NTFS_BBID[..] {
            break 'out Err(Errno::EINVAL);
        }
        if nt.ntm_bps() == 0 {
            break 'out Err(Errno::EINVAL);
        }

        {
            let cpr = nt.ntm_mftrecsz() as i8;
            if cpr > 0 {
                nt.ntm_bpmftrec.set(u32::from(nt.ntm_spc()) * cpr as u32);
            } else {
                nt.ntm_bpmftrec.set(
                    (1i32.wrapping_shl(-i32::from(cpr) as u32) / i32::from(nt.ntm_bps())) as u32,
                );
            }
        }

        nt.ntm_mountp.set(Some(mp));
        nt.ntm_dev.set(dev);
        nt.ntm_devvp.set(Some(devvp));
        nt.ntm_uid.set(argsp.uid);
        nt.ntm_gid.set(argsp.gid);
        nt.ntm_mode.set(argsp.mode);
        nt.ntm_flag.set(argsp.flag);
        mp.mnt_data.set(ptr::from_ref(nt).cast_mut().cast());
        nt.ntm_ntnodeq.init();

        // set file name encode/decode hooks XXX utf-8 only for now
        nt.ntm_wget.set(Some(ntfs_utf8_wget));
        nt.ntm_wput.set(Some(ntfs_utf8_wput));
        nt.ntm_wcmp.set(Some(ntfs_utf8_wcmp));

        let error: Result<(), Errno> = 'out1: {
            // We read in some system nodes to do not allow reclaim them and to have every
            // time access to them.
            for pi in [NTFS_MFTINO, NTFS_ROOTINO, NTFS_BITMAPINO] {
                let vp = match VFS_VGET(mp, Ino::from(pi)) {
                    Ok(vp) => vp,
                    Err(e) => break 'out1 Err(e),
                };
                nt.ntm_sysvn[pi as usize].set(Some(vp));
                vp.v_flag.set(vp.v_flag.get() | VSYSTEM);
                vref(vp);
                vput(vp);
            }

            // read the Unicode lowercase --> uppercase translation table, if necessary
            if let Err(e) = ntfs_toupper_use(mp, nt, p) {
                break 'out1 Err(e);
            }

            // Scan $BitMap and count free clusters
            match ntfs_calccfree(nt) {
                Ok(cfree) => nt.ntm_cfree.set(cfree),
                Err(e) => break 'out1 Err(e),
            }

            // Read and translate to internal format attribute definition file.
            {
                let mut ad = [0u8; size_of::<Attrdef>()];

                // Open $AttrDef
                let vp = match VFS_VGET(mp, Ino::from(NTFS_ATTRDEFINO)) {
                    Ok(vp) => vp,
                    Err(e) => break 'out1 Err(e),
                };

                // Count valid entries
                let mut num = 0usize;
                loop {
                    if let Err(e) = ntfs_readattr(
                        nt,
                        VTONT(vp),
                        NTFS_A_DATA,
                        None,
                        (num * size_of::<Attrdef>()) as i64,
                        size_of::<Attrdef>(),
                        &mut Rdata::Mem(&mut ad),
                    ) {
                        vput(vp);
                        break 'out1 Err(e);
                    }
                    if Attrdef::read(&ad, 0).ad_name[0] == 0 {
                        break;
                    }
                    num += 1;
                }

                // Alloc memory for attribute definitions
                let Some(mem) =
                    mallocarray(num, size_of::<Ntvattrdef>(), M_NTFSMNT, M_WAITOK | M_ZERO)
                else {
                    panic(format_args!("ntfs_mountfs: no memory"));
                };
                let adp = mem.cast::<Ntvattrdef>();
                nt.ntm_ad.set(Some(adp));
                nt.ntm_adnum.set(num as i32);
                // SAFETY: a fresh, zeroed allocation of `num` attribute definitions
                // (alignment 1), only reached through this slice until the loop is done.
                let ads = unsafe { core::slice::from_raw_parts_mut(adp.as_ptr(), num) };

                // Read them and translate
                for (i, d) in ads.iter_mut().enumerate() {
                    if let Err(e) = ntfs_readattr(
                        nt,
                        VTONT(vp),
                        NTFS_A_DATA,
                        None,
                        (i * size_of::<Attrdef>()) as i64,
                        size_of::<Attrdef>(),
                        &mut Rdata::Mem(&mut ad),
                    ) {
                        vput(vp);
                        break 'out1 Err(e);
                    }
                    let a = Attrdef::read(&ad, 0);
                    let name = a.ad_name;
                    let mut j = 0;
                    let mut adname = [0u8; 0x40];
                    while j < NTFS_ATTRNAME_MAXLEN {
                        adname[j] = name[j] as u8;
                        if name[j] == 0 {
                            break;
                        }
                        j += 1;
                    }
                    d.ad_name = adname;
                    d.ad_namelen = j as i32;
                    d.ad_type = a.ad_type;
                }

                vput(vp);
            }

            mp.update_stat(|sp| {
                sp.f_fsid.val[0] = dev;
                sp.f_fsid.val[1] = mp.vfc().vfc_typenum;
                sp.f_namemax = NTFS_MAXFILENAME as u32;
            });
            mp.mnt_flag.set(mp.mnt_flag.get() | MNT_LOCAL);
            if let Some(si) = devvp.v_specinfo() {
                si.si_mountpoint.set(Some(mp));
            }
            return Ok(());
        };

        // out1:
        for sv in nt.ntm_sysvn.iter() {
            if let Some(vp) = sv.get() {
                vrele(vp);
            }
        }

        let _ = vflush(mp, None, 0);

        error
    };

    // out:
    if let Some(si) = devvp.v_specinfo() {
        si.si_mountpoint.set(None);
    }

    if let Some(nt) = ntmp {
        if let Some(ad) = nt.ntm_ad.take() {
            free(
                ad.cast(),
                M_NTFSMNT,
                nt.ntm_adnum.get() as usize * size_of::<Ntvattrdef>(),
            );
        }
        // SAFETY: the ntfsmount came from `ntfs_alloc(M_NTFSMNT)` above; its vnodes were
        // flushed, and `mnt_data` is cleared right after.
        unsafe { ntfs_dealloc(nt, M_NTFSMNT) };
        mp.mnt_data.set(ptr::null_mut());
    }

    // lock the device vnode before calling VOP_CLOSE()
    let _ = vn_lock(devvp, LK_EXCLUSIVE | LK_RETRY);
    let _ = VOP_CLOSE(devvp, FREAD, NOCRED, Some(p));
    let _ = VOP_UNLOCK(devvp);

    error
}

/// `ntfs_start` (`vfs_start`).
pub fn ntfs_start(_mp: &'static Mount, _flags: i32, _p: &Proc) -> Result<(), Errno> {
    Ok(())
}

/// `ntfs_unmount` (`vfs_unmount`).
pub fn ntfs_unmount(mp: &'static Mount, mntflags: i32, p: &Proc) -> Result<(), Errno> {
    let ntmp = VFSTONTFS(mp);

    let mut flags = 0;
    if mntflags & MNT_FORCE != 0 {
        flags |= FORCECLOSE;
    }

    vflush(mp, None, flags | SKIPSYSTEM)?;

    // Check if system vnodes are still referenced
    for sv in ntmp.ntm_sysvn.iter() {
        if mntflags & MNT_FORCE == 0 && sv.get().is_some_and(|vp| vp.v_usecount.get() > 1) {
            return Err(Errno::EBUSY);
        }
    }

    // Dereference all system vnodes
    for sv in ntmp.ntm_sysvn.iter() {
        if let Some(vp) = sv.get() {
            vrele(vp);
        }
    }

    // vflush system vnodes
    if let Err(e) = vflush(mp, None, flags) {
        // XXX should this be panic() ?
        printf(format_args!(
            "ntfs_unmount: vflush failed(sysnodes): {}\n",
            e as i32
        ));
    }

    // Check if the type of device node isn't VBAD before touching v_specinfo. If the device
    // vnode is revoked, the field is NULL and touching it causes null pointer dereference.
    let devvp = ntmp.devvp();
    if devvp.v_type.get() != VBAD
        && let Some(si) = devvp.v_specinfo()
    {
        si.si_mountpoint.set(None);
    }

    // lock the device vnode before calling VOP_CLOSE()
    let _ = vn_lock(devvp, LK_EXCLUSIVE | LK_RETRY);
    let _ = vinvalbuf(devvp, V_SAVE, NOCRED, Some(p), 0, INFSLP);
    let _ = VOP_CLOSE(devvp, FREAD, NOCRED, Some(p));
    vput(devvp);

    // free the toupper table, if this has been last mounted ntfs volume
    ntfs_toupper_unuse(Some(p));

    if let Some(ad) = ntmp.ntm_ad.take() {
        free(
            ad.cast(),
            M_NTFSMNT,
            ntmp.ntm_adnum.get() as usize * size_of::<Ntvattrdef>(),
        );
    }
    // SAFETY: the ntfsmount came from `ntfs_alloc(M_NTFSMNT)` in `ntfs_mountfs`; every vnode
    // of the mount was flushed, so no ntnode points at it, and `mnt_data` is cleared below.
    unsafe { ntfs_dealloc(ntmp, M_NTFSMNT) };
    mp.mnt_data.set(ptr::null_mut());
    mp.mnt_flag.set(mp.mnt_flag.get() & !MNT_LOCAL);
    Ok(())
}

/// `ntfs_root` (`vfs_root`): the root directory's vnode, locked.
pub fn ntfs_root(mp: &'static Mount) -> Result<&'static Vnode, Errno> {
    match VFS_VGET(mp, Ino::from(NTFS_ROOTINO)) {
        Ok(nvp) => Ok(nvp),
        Err(e) => {
            printf(format_args!("ntfs_root: VFS_VGET failed: {}\n", e as i32));
            Err(e)
        }
    }
}

/// `ntfs_quotactl` (`vfs_quotactl`): do operations associated with quotas, not supported.
pub fn ntfs_quotactl(
    _mp: &'static Mount,
    _cmds: i32,
    _uid: Uid,
    _arg: usize,
    _p: &Proc,
) -> Result<(), Errno> {
    Err(Errno::EOPNOTSUPP)
}

/// `ntfs_calccfree(ntmp, &cfree)`: count the free clusters of `$Bitmap`.
pub fn ntfs_calccfree(ntmp: &Ntfsmount) -> Result<Cn, Errno> {
    let vp = ntmp.sysvn(NTFS_BITMAPINO);
    let mut cfree: Cn = 0;

    let bmsize = VTOF(vp).f_size.get();

    let mut chunksize = if bmsize > 1024 * 1024 {
        1024 * 1024
    } else {
        bmsize as usize
    };

    let mut tmp = crate::ntfs::ntfs_subr::KBuf::new(chunksize, M_TEMP);

    let mut offset: u64 = 0;
    while offset < bmsize {
        if chunksize as u64 > bmsize - offset {
            chunksize = (bmsize - offset) as usize;
        }

        ntfs_readattr(
            ntmp,
            VTONT(vp),
            NTFS_A_DATA,
            None,
            offset as i64,
            chunksize,
            &mut Rdata::Mem(&mut tmp.as_mut_slice()[..chunksize]),
        )?;

        for &b in &tmp.as_slice()[..chunksize] {
            cfree += Cn::from((!b).count_ones());
        }

        offset += chunksize as u64;
    }

    Ok(cfree)
}

/// `ntfs_statfs` (`vfs_statfs`).
pub fn ntfs_statfs(mp: &'static Mount, sbp: &mut Statfs, _p: &Proc) -> Result<(), Errno> {
    let ntmp = VFSTONTFS(mp);

    let mftallocated = VTOF(ntmp.sysvn(NTFS_MFTINO)).f_allocated.get();

    sbp.f_bsize = u32::from(ntmp.ntm_bps());
    sbp.f_iosize = u32::from(ntmp.ntm_bps()) * u32::from(ntmp.ntm_spc());
    sbp.f_blocks = ntmp.ntm_bootfile.get().bf_spv;
    sbp.f_bavail = ntmp.ntfs_cntobn(ntmp.ntm_cfree.get());
    sbp.f_bfree = sbp.f_bavail as u64;
    sbp.f_favail = sbp
        .f_bfree
        .checked_div(u64::from(ntmp.ntm_bpmftrec.get()))
        .unwrap_or(0) as i64;
    sbp.f_ffree = sbp.f_favail as u64;
    sbp.f_files = mftallocated
        .checked_div(ntmp.ntfs_bntob(ntmp.ntm_bpmftrec.get()) as u64)
        .unwrap_or(0)
        .wrapping_add(sbp.f_ffree);
    copy_statfs_info(sbp, mp);

    Ok(())
}

/// `ntfs_sync` (`vfs_sync`).
pub fn ntfs_sync(
    _mp: &'static Mount,
    _waitfor: i32,
    _stall: i32,
    _cred: *const Ucred,
    _p: &Proc,
) -> Result<(), Errno> {
    Ok(())
}

/// `ntfs_fhtovp` (`vfs_fhtovp`): the vnode a file handle names, locked.
pub fn ntfs_fhtovp(mp: &'static Mount, fhp: &Fid) -> Result<&'static Vnode, Errno> {
    let ntfhp = Ntfid::from_fid(fhp);

    // XXX as unlink/rmdir/mkdir/creat are not currently possible with NTFS, we don't need
    // to check anything else for now
    ntfs_vgetex(
        mp,
        ntfhp.ntfid_ino,
        u32::from(ntfhp.ntfid_attr),
        None,
        LK_EXCLUSIVE | LK_RETRY,
        0,
    )
}

/// `ntfs_vptofh` (`vfs_vptofh`).
pub fn ntfs_vptofh(vp: &'static Vnode, fhp: &mut Fid) -> Result<(), Errno> {
    let fn_ = VTOF(vp);
    let ntp = VTONT(vp);
    let ntfhp = Ntfid {
        ntfid_len: Ntfid::SIZE as u16,
        ntfid_pad: fhp.fid_reserved,
        ntfid_ino: ntp.i_number.get(),
        ntfid_attr: fn_.f_attrtype.get() as u8,
    };
    ntfhp.to_fid(fhp);
    Ok(())
}

/// `ntfs_vgetex(mp, ino, attrtype, attrname, lkflags, flags, &vp)`: the vnode of attribute
/// `attrtype`/`attrname` of MFT record `ino`, locked with `lkflags` (when it holds a lock
/// type). The `VG_*` flags skip loading the ntnode or validating the fnode.
pub fn ntfs_vgetex(
    mp: &'static Mount,
    ino: Ntfsino,
    attrtype: u32,
    attrname: Option<AttrNameBuf>,
    lkflags: i32,
    flags: u64,
) -> Result<&'static Vnode, Errno> {
    let ntmp = VFSTONTFS(mp);

    loop {
        // retry:
        // Get ntnode
        let ip = match ntfs_ntlookup(ntmp, ino) {
            Ok(ip) => ip,
            Err(e) => {
                printf(format_args!("ntfs_vget: ntfs_ntget failed\n"));
                return Err(e);
            }
        };

        // It may be not initialized fully, so force load it
        if flags & VG_DONTLOADIN == 0
            && ip.i_flag.get() & IN_LOADED == 0
            && let Err(e) = ntfs_loadntnode(ntmp, ip)
        {
            printf(format_args!(
                "ntfs_vget: CAN'T LOAD ATTRIBUTES FOR INO: {}\n",
                ip.i_number.get()
            ));
            ntfs_ntput(ip);

            return Err(e);
        }

        let fp = match ntfs_fget(ntmp, ip, attrtype, attrname) {
            Ok(fp) => fp,
            Err(e) => {
                printf(format_args!("ntfs_vget: ntfs_fget failed\n"));
                ntfs_ntput(ip);

                return Err(e);
            }
        };

        let mut f_type: Vtype = VNON;
        if flags & VG_DONTVALIDFN == 0 && fp.f_flag.get() & FN_VALID == 0 {
            if ip.i_frflag.get() & NTFS_FRFLAG_DIR != 0
                && fp.f_attrtype.get() == NTFS_A_DATA
                && fp.f_attrname.get().is_none()
            {
                f_type = VDIR;
            } else if flags & VG_EXT != 0 {
                f_type = VNON;
                fp.f_size.set(0);
                fp.f_allocated.set(0);
            } else {
                f_type = VREG;

                match ntfs_filesize(ntmp, fp) {
                    Ok((size, bytes)) => {
                        fp.f_size.set(size);
                        fp.f_allocated.set(bytes);
                    }
                    Err(e) => {
                        ntfs_ntput(ip);

                        return Err(e);
                    }
                }
            }

            fp.f_flag.set(fp.f_flag.get() | FN_VALID);
        }

        // We may be calling vget() now. To avoid potential deadlock, we need to release
        // ntnode lock, since due to locking order vnode lock has to be acquired first.
        // ntfs_fget() bumped ntnode usecount, so ntnode won't be recycled prematurely.
        let vp = fp.f_vp.get();
        let vpid = vp.map_or(0, |vp| vp.v_id.get());
        ntfs_ntput(ip);

        if let Some(vp) = vp {
            // vget() returns error if the vnode has been recycled
            if vget(vp, lkflags).is_err() {
                continue;
            }
            if vpid == vp.v_id.get() {
                return Ok(vp);
            }
            vput(vp);
            continue;
        }

        let vp = match getnewvnode(VT_NTFS, Some(ntmp.mountp()), &NTFS_VOPS) {
            Ok(vp) => vp,
            Err(e) => {
                ntfs_frele(fp);
                // The C releases the ntnode a second time here (the module's deviations).
                ntfs_ntput(ip);

                return Err(e);
            }
        };

        fp.f_vp.set(Some(vp));
        vp.v_data.set(ptr::from_ref(fp).cast_mut().cast::<c_void>());
        vp.v_type.set(f_type);

        if ino == NTFS_ROOTINO {
            vp.v_flag.set(vp.v_flag.get() | VROOT);
        }

        if lkflags & LK_TYPE_MASK != 0
            && let Err(e) = vn_lock(vp, lkflags)
        {
            vput(vp);
            return Err(e);
        }

        return Ok(vp);
    }
}

/// `ntfs_vget` (`vfs_vget`): the vnode of the unnamed `$DATA` of MFT record `ino`, locked.
pub fn ntfs_vget(mp: &'static Mount, ino: Ino) -> Result<&'static Vnode, Errno> {
    if ino > Ino::from(Ntfsino::MAX) {
        panic(format_args!("ntfs_vget: alien ino_t {}", ino));
    }
    ntfs_vgetex(
        mp,
        ino as Ntfsino,
        NTFS_A_DATA,
        None,
        LK_EXCLUSIVE | LK_RETRY,
        0,
    ) // XXX
}

// `struct ntfid` must fit in a `struct fid`.
const _: () = assert!(Ntfid::SIZE <= crate::sys::mount::MAXFIDSZ + 4);
/* </CODE> */
