/*	$OpenBSD: udf_vfsops.c,v 1.73 2025/09/20 13:53:36 mpi Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 2001, 2002 Scott Long <scottl@freebsd.org>
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
 * $FreeBSD: src/sys/fs/udf/udf_vfsops.c,v 1.25 2005/01/25 15:52:03 phk Exp $
 */
/* </LICENSES> */

//! UDF file-system-type operations: mount (the anchor, the volume descriptor sequence, the
//! partition maps, the file set descriptor and the root's file entry), unmount, root,
//! `statfs`, `vget` (a file entry into a vnode), file handles, and `udf_checktag`.
//!
//! Upstream: sys/isofs/udf/udf_vfsops.c @ 3ce1f3f79392
//!
//! Ported to OpenBSD by Pedro Martelletto in February 2005.
//!
//! Ok, here's how it goes. The UDF specs are pretty clear on how each data structure is
//! made up, but not very clear on how they relate to each other. Here is the skinny... This
//! demonstrates a filesystem with one file in the root directory. Subdirectories are treated
//! just as normal files, but they have File Id Descriptors of their children as their file
//! data. As for the Anchor Volume Descriptor Pointer, it can exist in two of the following
//! three places: sector 256, sector n (the max sector of the disk), or sector n - 256. It's
//! a pretty good bet that one will exist at sector 256 though. One caveat is unclosed CD
//! media. For that, sector 256 cannot be written, so the Anchor Volume Descriptor Pointer
//! can exist at sector 512 until the media is closed.
//!
//! ## Deviations
//! - `udf_mount` reads its `struct udf_args` out of the kernel copy of the mount arguments
//!   (`UdfArgs::from_bytes`), `EINVAL` when they are short; it looks the device up with a
//!   `nameidata` of its own (`ndinit`), as `ffs_mount` does, instead of reinitialising the
//!   caller's. The `UDF_DEBUG` message (no such option here) is left out.
//! - `vfsconflist[]` gives UDF `UdfArgs::SIZE` as `vfc_datasize` (the C writes
//!   `sizeof(struct iso_args)`, larger than the `struct udf_args` that `mount_udf(8)`
//!   passes, so `sys_mount` would copy in bytes past it).
//! - `udf_root`, `udf_vget` and `udf_fhtovp` return the vnode (`Vfsops`'s shape).
//! - The descriptors are read in place from the buffers (`ecma167_udf.rs`); a structure that
//!   does not fit in what was read (a partition map past the descriptor's buffer, the
//!   Metadata File's allocation descriptor, a sparing table entry) is `EINVAL`, or ends the
//!   count of sparing table entries, where the C reads past the buffer.
//!   `udf_find_partmaps` takes the bytes that follow the logical volume descriptor (the C's
//!   `lvd->maps[]`).
//! - `udf_vget` copies at most what the buffer holds of the file entry (the C copies
//!   `l_ea + l_ad` + the header size from a one-block buffer); the rest of the copy stays
//!   zero (`M_ZERO`).
//! - `udf_mountfs`'s error path also frees the sparing table and the VAT copy (the C leaves
//!   them), and `udf_get_spartmap` forgets the sparing table it frees (the C keeps the
//!   dangling `um_stbl`). When the scan of the volume descriptor sequence stops at a logical
//!   volume descriptor whose partition maps fail after both descriptors were found, its
//!   buffer is released (the C keeps it busy).
//! - `ffs(um_bsize) - 1` is computed with `trailing_zeros`; shifts by `um_bshift` wrap
//!   (`rdsector`), so a logical block size that is not a power of two reads the wrong
//!   blocks, as in C, without the C's undefined shift.
//! - `struct ifid` is [`Ifid`], copied in and out of a `struct fid` field by field.

use core::ffi::c_void;
use core::ptr::{self, NonNull};

use crate::crypto::siphash::SiphashKey;
use crate::dev::rnd::arc4random_buf;
use crate::isofs::udf::ecma167_udf::{
    AnchorVdp, DescTag, ExtfileEntry, FileEntry, FilesetDesc, LogvolDesc, LongAd, Packed, PartDesc,
    PartMap1, PartMap2, PartMapMeta, PartMapSpare, PartMapVirt, TAGID_ANCHOR, TAGID_EXTFENTRY,
    TAGID_FENTRY, TAGID_FSD, TAGID_LOGVOL, TAGID_PARTITION, UDF_EXTFENTRY_SIZE, UDF_FENTRY_SIZE,
    UDF_ICB_FILETYPE_BLOCKDEVICE, UDF_ICB_FILETYPE_CHARDEVICE, UDF_ICB_FILETYPE_DIRECTORY,
    UDF_ICB_FILETYPE_FIFO, UDF_ICB_FILETYPE_RANDOMACCESS, UDF_ICB_FILETYPE_REALTIME,
    UDF_ICB_FILETYPE_SOCKET, UDF_ICB_FILETYPE_SYMLINK, UDF_ICB_FILETYPE_UNKNOWN, UDF_REGID_ID_SIZE,
    UdfSparingTable,
};
use crate::isofs::udf::udf::{
    UDF_HASHTBLSIZE, UDF_MNT_FIND_VAT, UDF_MNT_USES_META, UDF_MNT_USES_VAT, UdfDirstream, Udfino,
    Umount, Unicode, Unode, UnodeHash, VFSTOUDFFS, VTOU, rdsector, udf_getid, udf_readlblks,
};
use crate::isofs::udf::udf_subr::{udf_vat_free, udf_vat_get, udf_vat_map};
use crate::isofs::udf::udf_vnops::{udf_allocv, udf_hashins, udf_hashlookup};
use crate::kern::init_main::rootvp;
use crate::kern::kern_lock::mtx_init;
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_rwlock::rrw_init_flags;
use crate::kern::kern_subr::{hashfree, hashinit};
use crate::kern::spec_vnops::SPEC_VOPS;
use crate::kern::subr_pool::{pool_get, pool_init, pool_put};
use crate::kern::subr_prf::{panic, printf};
use crate::kern::vfs_bio::{bread, brelse};
use crate::kern::vfs_lookup::{namei, ndinit};
use crate::kern::vfs_subr::{
    checkalias, copy_statfs_info, vcount, vflush, vfs_mountedon, vgone, vinvalbuf, vref, vrele,
};
use crate::kern::vfs_vnops::vn_lock;
use crate::kern::vfs_vops::{VOP_CLOSE, VOP_OPEN, VOP_UNLOCK};
use crate::machine::conf::nblkdev;
use crate::machine::copy::copyinstr;
use crate::machine::cpu::curproc;
use crate::machine::intr::IPL_NONE;
use crate::sys::buf::Buf;
use crate::sys::dirent::MAXNAMLEN;
use crate::sys::endian::{letoh16, letoh32};
use crate::sys::errno::Errno;
use crate::sys::fcntl::FREAD;
use crate::sys::lock::{LK_EXCLUSIVE, LK_RETRY};
use crate::sys::malloc::{M_NOWAIT, M_UDFFENTRY, M_UDFMOUNT, M_WAITOK, M_ZERO};
use crate::sys::mbuf::Mbuf;
use crate::sys::mount::{
    Fid, MNAMELEN, MNT_FORCE, MNT_LOCAL, MNT_RDONLY, MNT_ROOTFS, MNT_UPDATE, Mount, Statfs,
    UdfArgs, VFS_VGET, Vfsconf, Vfsops,
};
use crate::sys::namei::{FOLLOW, LOOKUP, Nameidata, NiDirp};
use crate::sys::param::btodb;
use crate::sys::pool::{PR_WAITOK, PR_ZERO, Pool};
use crate::sys::proc::Proc;
use crate::sys::rwlock::{RWL_DUPOK, RWL_IS_VNODE};
use crate::sys::syslimits::NAME_MAX;
use crate::sys::systm::INFSLP;
use crate::sys::types::{Daddr, Ino, Uid, major};
use crate::sys::ucred::{FSCRED, NOCRED, Ucred};
use crate::sys::vnode::{
    FORCECLOSE, V_SAVE, VBLK, VCHR, VDIR, VFIFO, VLNK, VREG, VROOT, VSOCK, Vnode,
};

/// `udf_trans_pool`: the buffers of `udf_transname` and `udf_cmpname`.
pub static UDF_TRANS_POOL: Pool = Pool::new();
/// `unode_pool`.
pub static UNODE_POOL: Pool = Pool::new();
/// `udf_ds_pool`: the directory streams.
pub static UDF_DS_POOL: Pool = Pool::new();

/// `udf_vfsops`.
pub static UDF_VFSOPS: Vfsops = Vfsops {
    vfs_mount: udf_mount,
    vfs_start: udf_start,
    vfs_unmount: udf_unmount,
    vfs_root: udf_root,
    vfs_quotactl: udf_quotactl,
    vfs_statfs: udf_statfs,
    vfs_sync: udf_sync,
    vfs_vget: udf_vget,
    vfs_fhtovp: udf_fhtovp,
    vfs_vptofh: udf_vptofh,
    vfs_init: Some(udf_init),
    vfs_sysctl: None,
    vfs_checkexp: udf_checkexp,
};

/// `udf_init` (`vfs_init`): the pools.
pub fn udf_init(_foo: &'static Vfsconf) -> Result<(), Errno> {
    pool_init(
        &UDF_TRANS_POOL,
        MAXNAMLEN * size_of::<Unicode>(),
        0,
        IPL_NONE,
        PR_WAITOK,
        "udftrpl",
        None,
    );
    pool_init(
        &UNODE_POOL,
        size_of::<Unode>(),
        0,
        IPL_NONE,
        PR_WAITOK,
        "udfndpl",
        None,
    );
    pool_init(
        &UDF_DS_POOL,
        size_of::<UdfDirstream>(),
        0,
        IPL_NONE,
        PR_WAITOK,
        "udfdspl",
        None,
    );

    Ok(())
}

/// `udf_start` (`vfs_start`).
pub fn udf_start(_mp: &'static Mount, _flags: i32, _p: &Proc) -> Result<(), Errno> {
    Ok(())
}

/// `memset(dst, 0, MNAMELEN); strlcpy(dst, src, MNAMELEN)`.
fn mname_copy(dst: &mut [u8; MNAMELEN], src: &[u8]) {
    *dst = [0; MNAMELEN];
    let src = src.split(|&c| c == 0).next().unwrap_or(&[]);
    let n = src.len().min(MNAMELEN - 1);
    dst[..n].copy_from_slice(&src[..n]);
}

/// `udf_mount` (`vfs_mount`): mount system call. `data` is the kernel copy of the user's
/// `struct udf_args`.
pub fn udf_mount(
    mp: &'static Mount,
    path: &[u8],
    data: &mut [u8],
    _ndp: &mut Nameidata<'_>,
    p: &Proc,
) -> Result<(), Errno> {
    let mut fspec = [0u8; MNAMELEN];

    if mp.mnt_flag.get() & MNT_RDONLY == 0 {
        mp.mnt_flag.set(mp.mnt_flag.get() | MNT_RDONLY);
    }

    // No root filesystem support. Probably not a big deal, since the bootloader doesn't
    // understand UDF.
    if mp.mnt_flag.get() & MNT_ROOTFS != 0 {
        return Err(Errno::EOPNOTSUPP);
    }

    // If updating, check whether changing from read-only to read/write; if there is no
    // device name, that's all we do.
    if mp.mnt_flag.get() & MNT_UPDATE != 0 {
        return Ok(());
    }

    let Some(args) = UdfArgs::from_bytes(data) else {
        return Err(Errno::EINVAL);
    };

    copyinstr(args.fspec, &mut fspec)?;

    let flen = fspec.iter().position(|&c| c == 0).unwrap_or(MNAMELEN);
    let mut nd = ndinit(LOOKUP, FOLLOW, NiDirp::Sys(&fspec[..flen]), p);
    namei(&mut nd)?;
    let Some(devvp) = nd.ni_vp else {
        return Err(Errno::ENOENT);
    };

    if devvp.v_type.get() != VBLK {
        vrele(devvp);
        return Err(Errno::ENOTBLK);
    }

    if major(devvp.v_rdev()) >= nblkdev() {
        vrele(devvp);
        return Err(Errno::ENXIO);
    }

    if let Err(e) = udf_mountfs(devvp, mp, args.lastblock, p) {
        vrele(devvp);
        return Err(e);
    }

    // Keep a copy of the mount information.
    mp.update_stat(|sp| {
        mname_copy(&mut sp.f_mntonname, path);
        mname_copy(&mut sp.f_mntfromname, &fspec);
        mname_copy(&mut sp.f_mntfromspec, &fspec);
    });

    Ok(())
}

/// Check the descriptor tag for both the correct id and correct checksum. Return zero if
/// all is good, `EINVAL` if not.
pub fn udf_checktag(tag: &DescTag, id: u16) -> Result<(), Errno> {
    let itag = tag.as_bytes();

    if letoh16(tag.id) != id {
        return Err(Errno::EINVAL);
    }

    let mut cksum: u8 = 0;
    for &b in &itag[..15] {
        cksum = cksum.wrapping_add(b);
    }
    cksum = cksum.wrapping_sub(itag[4]);

    if cksum == tag.cksum {
        return Ok(());
    }

    Err(Errno::EINVAL)
}

/// `ffs(x)`: the 1-based index of the lowest bit set, 0 for 0.
fn ffs(x: i32) -> i32 {
    if x == 0 {
        0
    } else {
        x.trailing_zeros() as i32 + 1
    }
}

/// The buffer's data, as the C's `bp->b_data`.
///
/// # Safety
///
/// As [`Buf::data`]: the buffer is busy for the caller and mapped, and no other slice of it
/// is alive.
#[allow(clippy::mut_from_ref)] // the B_BUSY owner's view, as the C's b_data
unsafe fn bdata(bp: &Buf) -> &mut [u8] {
    // SAFETY: the caller's contract.
    unsafe { bp.data() }
}

/// `udf_mountfs`: read the volume's structures from `devvp` and set up its umount.
pub fn udf_mountfs(
    devvp: &'static Vnode,
    mp: &'static Mount,
    lb: u32,
    p: &Proc,
) -> Result<(), Errno> {
    let mut fsd_offset: u32 = 0;
    let mut part_num: u16 = 0;
    let mut fsd_part: u16 = 0;
    let mut logvol_found = false;
    let mut part_found = false;
    let mut fsd_found = false;

    // Disallow multiple mounts of the same device. Disallow mounting of a device that is
    // currently in use (except for root, which might share swap device for miniroot). Flush
    // out any old buffers remaining from a previous use.
    vfs_mountedon(devvp)?;
    if vcount(devvp) > 1 && !rootvp().is_some_and(|r| ptr::eq(r, devvp)) {
        return Err(Errno::EBUSY);
    }
    let _ = vn_lock(devvp, LK_EXCLUSIVE | LK_RETRY);
    let error = vinvalbuf(devvp, V_SAVE, p.p_ucred.get(), Some(p), 0, INFSLP);
    let _ = VOP_UNLOCK(devvp);
    error?;

    VOP_OPEN(devvp, FREAD, FSCRED, p)?;

    let Some(mem) = malloc(size_of::<Umount>(), M_UDFMOUNT, M_WAITOK | M_ZERO) else {
        panic(format_args!("udf_mountfs: no memory"));
    };
    let ump = mem.cast::<Umount>();
    // SAFETY: a fresh allocation of a umount's size (malloc aligns to its bucket, at least
    // 16), written once here; `udf_unmount` or the error path below frees it.
    let ump: &'static Umount = unsafe {
        ump.as_ptr().write(Umount::new());
        ump.as_ref()
    };

    mp.mnt_data
        .set(ptr::from_ref(ump).cast_mut().cast::<c_void>());
    let typenum = mp.vfc().vfc_typenum;
    mp.update_stat(|sp| {
        sp.f_fsid.val[0] = devvp.v_rdev();
        sp.f_fsid.val[1] = typenum;
        sp.f_namemax = NAME_MAX as u32;
    });
    mp.mnt_flag.set(mp.mnt_flag.get() | MNT_LOCAL);

    ump.um_mountp.set(Some(mp));
    ump.um_dev.set(devvp.v_rdev());
    ump.um_devvp.set(Some(devvp));

    let bsize: usize = 2048; // Should probe the media for its size.

    let mut bp: Option<&'static Buf>;

    let error: Errno = 'bail: {
        // Get the Anchor Volume Descriptor Pointer from sector 256. Should also check
        // sector n - 256, n, and 512.
        let mut sector: u32 = 256;
        let (b, e) = bread(
            devvp,
            Daddr::from(sector) * btodb(bsize) as Daddr,
            bsize as i32,
        );
        bp = Some(b);
        if let Err(e) = e {
            break 'bail e;
        }
        let avdp = {
            // SAFETY: the buffer is ours (busy from bread) and mapped; the slice dies in
            // this block.
            let data = unsafe { bdata(b) };
            let Some(tag) = DescTag::at(data, 0) else {
                break 'bail Errno::EINVAL;
            };
            if let Err(e) = udf_checktag(tag, TAGID_ANCHOR) {
                break 'bail e;
            }
            match AnchorVdp::at(data, 0) {
                Some(avdp) => *avdp,
                None => break 'bail Errno::EINVAL,
            }
        };
        brelse(b);
        bp = None;

        // Extract the Partition Descriptor and Logical Volume Descriptor from the Volume
        // Descriptor Sequence. Should we care about the partition type right now? What
        // about multiple partitions?
        let mvds_start = letoh32(avdp.main_vds_ex.loc);
        let mvds_end =
            mvds_start.wrapping_add(letoh32(avdp.main_vds_ex.len).wrapping_sub(1) / bsize as u32);
        sector = mvds_start;
        while sector < mvds_end {
            let (b, e) = bread(
                devvp,
                Daddr::from(sector) * btodb(bsize) as Daddr,
                bsize as i32,
            );
            bp = Some(b);
            if let Err(e) = e {
                printf(format_args!("Can't read sector {} of VDS\n", sector));
                break 'bail e;
            }
            // SAFETY: as above; the slice dies before the buffer is released.
            let data = unsafe { bdata(b) };
            if let Some(lvd) = LogvolDesc::at(data, 0)
                && udf_checktag(&lvd.tag, TAGID_LOGVOL).is_ok()
            {
                let bs = letoh32(lvd.lb_size) as i32;
                ump.um_bsize.set(bs);
                ump.um_bmask.set(bs.wrapping_sub(1));
                ump.um_bshift.set(ffs(bs) - 1);
                let fsd_loc = lvd.lv_fsd_loc();
                fsd_part = letoh16(fsd_loc.loc.part_num);
                fsd_offset = letoh32(fsd_loc.loc.lb_num);
                if udf_find_partmaps(ump, lvd, &data[LogvolDesc::SIZE..]).is_err() {
                    break;
                }
                logvol_found = true;
            }
            if let Some(pd) = PartDesc::at(data, 0)
                && udf_checktag(&pd.tag, TAGID_PARTITION).is_ok()
            {
                part_found = true;
                part_num = letoh16(pd.part_num);
                let len = letoh32(pd.part_len);
                ump.um_len.set(len);
                ump.um_reallen.set(len);
                let start = letoh32(pd.start_loc);
                ump.um_start.set(start);
                ump.um_realstart.set(start);
            }

            brelse(b);
            bp = None;
            if part_found && logvol_found {
                break;
            }
            sector += 1;
        }

        if !part_found || !logvol_found {
            break 'bail Errno::EINVAL;
        }
        // A descriptor whose partition maps failed ended the scan with its buffer (the C
        // keeps it busy for ever when both descriptors were found before).
        if let Some(b) = bp.take() {
            brelse(b);
        }

        if ump.um_flags.get() & UDF_MNT_USES_META != 0 {
            // Read Metadata File 'File Entry' to find Metadata file.
            sector = ump.um_start.get().wrapping_add(ump.um_meta_start.get()); // Set in udf_get_mpartmap()
            let (b, e) = rdsector(ump, devvp, Daddr::from(sector), ump.um_bsize.get());
            bp = Some(b);
            if e.is_err() {
                printf(format_args!(
                    "Cannot read sector {} for Metadata File Entry\n",
                    sector
                ));
                break 'bail Errno::EINVAL;
            }
            let la = {
                // SAFETY: as above.
                let data = unsafe { bdata(b) };
                let la_at = |hdr: usize, l_ea: u32| {
                    hdr.checked_add(letoh32(l_ea) as usize)
                        .and_then(|off| LongAd::at(data, off).copied())
                };
                match (ExtfileEntry::at(data, 0), FileEntry::at(data, 0)) {
                    (Some(xfentry), _) if udf_checktag(&xfentry.tag, TAGID_EXTFENTRY).is_ok() => {
                        la_at(UDF_EXTFENTRY_SIZE, xfentry.l_ea)
                    }
                    (_, Some(fentry)) if udf_checktag(&fentry.tag, TAGID_FENTRY).is_ok() => {
                        la_at(UDF_FENTRY_SIZE, fentry.l_ea)
                    }
                    (_, fentry) => {
                        printf(format_args!(
                            "Invalid Metadata File FE @ sector {}! (tag.id {})\n",
                            sector,
                            fentry.map_or(0, |f| { f.tag.id })
                        ));
                        break 'bail Errno::EINVAL;
                    }
                }
            };
            let Some(la) = la else {
                break 'bail Errno::EINVAL;
            };
            ump.um_meta_start.set(letoh32(la.loc.lb_num));
            ump.um_meta_len.set(letoh32(la.len));
            brelse(b);
            bp = None;
        } else if fsd_part != part_num {
            printf(format_args!("FSD does not lie within the partition!\n"));
            break 'bail Errno::EINVAL;
        }

        mtx_init(&ump.um_hashmtx, IPL_NONE);
        let Some(tbl) = hashinit::<UnodeHash>(UDF_HASHTBLSIZE, M_UDFMOUNT, M_WAITOK) else {
            panic(format_args!("udf_mountfs: no hash table"));
        };
        ump.um_hashtbl.set(Some(tbl));
        ump.um_hashsz.set(tbl.len() as u64 - 1);
        let mut key = [0u8; 16];
        arc4random_buf(&mut key);
        let (k0, k1) = key.split_at(8);
        ump.um_hashkey.set(SiphashKey {
            k0: u64::from_ne_bytes(k0.try_into().unwrap_or([0; 8])),
            k1: u64::from_ne_bytes(k1.try_into().unwrap_or([0; 8])),
        });

        // Get the VAT, if needed
        if ump.um_flags.get() & UDF_MNT_FIND_VAT != 0
            && let Err(e) = udf_vat_get(ump, lb)
        {
            break 'bail e;
        }

        // Grab the Fileset Descriptor. Thanks to Chuck McCrobie <mccrobie@cablespeed.com>
        // for pointing me in the right direction here.
        sector = if ump.um_flags.get() & UDF_MNT_USES_META != 0 {
            ump.um_meta_start.get()
        } else {
            fsd_offset
        };
        let _ = udf_vat_map(ump, &mut sector);
        let (b, e) = rdsector(ump, devvp, Daddr::from(sector), ump.um_bsize.get());
        bp = Some(b);
        if let Err(e) = e {
            printf(format_args!("Cannot read sector {} of FSD\n", sector));
            break 'bail e;
        }
        {
            // SAFETY: as above.
            let data = unsafe { bdata(b) };
            if let Some(fsd) = FilesetDesc::at(data, 0)
                && udf_checktag(&fsd.tag, TAGID_FSD).is_ok()
            {
                fsd_found = true;
                let mut icb = fsd.rootdir_icb;
                if ump.um_flags.get() & UDF_MNT_USES_META != 0 {
                    icb.loc.lb_num = { icb.loc.lb_num }.wrapping_add(ump.um_meta_start.get());
                    icb.loc.part_num = part_num;
                }
                ump.um_root_icb.set(icb);
            }
        }

        brelse(b);
        bp = None;

        if !fsd_found {
            printf(format_args!("Couldn't find the fsd\n"));
            break 'bail Errno::EINVAL;
        }

        // Find the file entry for the root directory.
        let root_icb = ump.um_root_icb.get();
        sector = letoh32(root_icb.loc.lb_num);
        let size = letoh32(root_icb.len);
        let _ = udf_vat_map(ump, &mut sector);
        let (b, e) = udf_readlblks(ump, sector as i32, size as i32);
        bp = Some(b);
        if let Err(e) = e {
            printf(format_args!("Cannot read sector {}\n", sector));
            break 'bail e;
        }

        {
            // SAFETY: as above.
            let data = unsafe { bdata(b) };
            let efe = ExtfileEntry::at(data, 0).map_or(Err(Errno::EINVAL), |x| {
                udf_checktag(&x.tag, TAGID_EXTFENTRY)
            });
            if efe.is_err() {
                let fe = FileEntry::at(data, 0)
                    .map_or(Err(Errno::EINVAL), |f| udf_checktag(&f.tag, TAGID_FENTRY));
                if let Err(e) = fe {
                    printf(format_args!("Invalid root file entry!\n"));
                    break 'bail e;
                }
            }
        }

        brelse(b);

        if let Some(si) = devvp.v_specinfo() {
            si.si_mountpoint.set(Some(mp));
        }

        return Ok(());
    };

    // bail:
    if let Some(tbl) = ump.um_hashtbl.take() {
        // SAFETY: the table came from `hashinit` with these arguments just above; no unode
        // of this failed mount is used any more.
        unsafe { hashfree(tbl, UDF_HASHTBLSIZE, M_UDFMOUNT) };
    }
    udf_vat_free(ump);
    if let Some(stbl) = ump.um_stbl.take() {
        free(stbl, M_UDFMOUNT, ump.um_stbl_size.get());
    }
    free(
        NonNull::from(ump).cast::<u8>(),
        M_UDFMOUNT,
        size_of::<Umount>(),
    );
    mp.mnt_data.set(ptr::null_mut());
    mp.mnt_flag.set(mp.mnt_flag.get() & !MNT_LOCAL);
    if let Some(si) = devvp.v_specinfo() {
        si.si_mountpoint.set(None);
    }
    if let Some(b) = bp {
        brelse(b);
    }

    let _ = vn_lock(devvp, LK_EXCLUSIVE | LK_RETRY);
    let _ = VOP_CLOSE(devvp, FREAD, FSCRED, Some(p));
    let _ = VOP_UNLOCK(devvp);

    Err(error)
}

/// `udf_unmount` (`vfs_unmount`).
pub fn udf_unmount(mp: &'static Mount, mntflags: i32, p: &Proc) -> Result<(), Errno> {
    let ump = VFSTOUDFFS(mp);
    let devvp = ump.devvp();
    let mut flags = 0;

    if mntflags & MNT_FORCE != 0 {
        flags |= FORCECLOSE;
    }

    vflush(mp, None, flags)?;

    let _ = vn_lock(devvp, LK_EXCLUSIVE | LK_RETRY);
    let _ = vinvalbuf(devvp, V_SAVE, NOCRED, Some(p), 0, INFSLP);
    let _ = VOP_CLOSE(devvp, FREAD, NOCRED, Some(p));
    let _ = VOP_UNLOCK(devvp);

    if let Some(si) = devvp.v_specinfo() {
        si.si_mountpoint.set(None);
    }
    vrele(devvp);

    if ump.um_flags.get() & UDF_MNT_USES_VAT != 0 {
        udf_vat_free(ump);
    }

    if let Some(stbl) = ump.um_stbl.take() {
        free(stbl, M_UDFMOUNT, ump.um_stbl_size.get());
    }

    if let Some(tbl) = ump.um_hashtbl.take() {
        // SAFETY: the table came from `hashinit` in `udf_mountfs` with these arguments; the
        // vnodes were flushed, so every unode left its chain in `udf_reclaim`.
        unsafe { hashfree(tbl, UDF_HASHTBLSIZE, M_UDFMOUNT) };
    }
    free(
        NonNull::from(ump).cast::<u8>(),
        M_UDFMOUNT,
        size_of::<Umount>(),
    );

    mp.mnt_data.set(ptr::null_mut());
    mp.mnt_flag.set(mp.mnt_flag.get() & !MNT_LOCAL);

    Ok(())
}

/// `udf_root` (`vfs_root`): the root directory's vnode, locked.
pub fn udf_root(mp: &'static Mount) -> Result<&'static Vnode, Errno> {
    let ump = VFSTOUDFFS(mp);

    let id = udf_getid(&ump.um_root_icb.get());

    let vp = udf_vget(mp, Ino::from(id))?;

    vp.v_flag.set(vp.v_flag.get() | VROOT);

    Ok(vp)
}

/// `udf_quotactl` (`vfs_quotactl`).
pub fn udf_quotactl(
    _mp: &'static Mount,
    _cmds: i32,
    _uid: Uid,
    _arg: usize,
    _p: &Proc,
) -> Result<(), Errno> {
    Err(Errno::EOPNOTSUPP)
}

/// `udf_statfs` (`vfs_statfs`).
pub fn udf_statfs(mp: &'static Mount, sbp: &mut Statfs, _p: &Proc) -> Result<(), Errno> {
    let ump = VFSTOUDFFS(mp);

    sbp.f_bsize = ump.um_bsize.get() as u32;
    sbp.f_iosize = ump.um_bsize.get() as u32;
    sbp.f_blocks = u64::from(ump.um_len.get());
    sbp.f_bfree = 0;
    sbp.f_bavail = 0;
    sbp.f_files = 0;
    sbp.f_ffree = 0;
    sbp.f_favail = 0;
    copy_statfs_info(sbp, mp);

    Ok(())
}

/// `udf_sync` (`vfs_sync`).
pub fn udf_sync(
    _mp: &'static Mount,
    _waitfor: i32,
    _stall: i32,
    _cred: *const Ucred,
    _p: &Proc,
) -> Result<(), Errno> {
    Ok(())
}

/// `udf_vget` (`vfs_vget`): the vnode of the file entry at logical block `ino`, locked.
pub fn udf_vget(mp: &'static Mount, ino: Ino) -> Result<&'static Vnode, Errno> {
    if ino > Ino::from(Udfino::MAX) {
        panic(format_args!("udf_vget: alien ino_t {}", ino));
    }
    let ino = ino as Udfino;

    let p = curproc();
    let ump = VFSTOUDFFS(mp);

    // See if we already have this in the cache
    if let Some(vp) = udf_hashlookup(ump, ino, LK_EXCLUSIVE)? {
        return Ok(vp);
    }

    // Allocate memory and check the tag id's before grabbing a new vnode, since it's hard to
    // roll back if there is a problem.
    let Some(mem) = pool_get(&UNODE_POOL, PR_WAITOK | PR_ZERO) else {
        panic(format_args!("udf_vget: no unode"));
    };
    let upp = mem.cast::<Unode>();
    // SAFETY: a fresh `unode_pool` item, sized and aligned for a unode, written once; it is
    // given back by the error paths below or by `udf_reclaim`.
    let up: &'static Unode = unsafe {
        upp.as_ptr().write(Unode::new());
        upp.as_ref()
    };

    // Copy in the file entry. Per the spec, the size can only be 1 block.
    let mut sector = ino;
    let devvp = ump.devvp();
    let _ = udf_vat_map(ump, &mut sector);
    let (bp, error) = rdsector(ump, devvp, Daddr::from(sector), ump.um_bsize.get());
    if let Err(e) = error {
        printf(format_args!("Cannot read sector {}\n", sector));
        pool_put(&UNODE_POOL, mem);
        brelse(bp);
        return Err(e);
    }

    let copied = {
        // SAFETY: the buffer is ours (busy from bread) and mapped; the slice dies in this
        // block, before the buffer is released.
        let data = unsafe { bdata(bp) };
        let xfe =
            ExtfileEntry::at(data, 0).filter(|xfe| udf_checktag(&xfe.tag, TAGID_EXTFENTRY).is_ok());
        let size_hdr = match xfe {
            Some(xfe) => Some((
                letoh32(xfe.l_ea) as usize + letoh32(xfe.l_ad) as usize,
                UDF_EXTFENTRY_SIZE,
            )),
            None => FileEntry::at(data, 0)
                .filter(|fe| udf_checktag(&fe.tag, TAGID_FENTRY).is_ok())
                .map(|fe| {
                    (
                        letoh32(fe.l_ea) as usize + letoh32(fe.l_ad) as usize,
                        UDF_FENTRY_SIZE,
                    )
                }),
        };
        match size_hdr {
            None => {
                printf(format_args!("Invalid file entry!\n"));
                None
            }
            Some((size, hdr)) => {
                // Allocate max size of FE/XFE.
                let len = size + UDF_EXTFENTRY_SIZE;
                match malloc(len, M_UDFFENTRY, M_NOWAIT | M_ZERO) {
                    None => Some(Err(())),
                    Some(fentry) => {
                        let n = (size + hdr).min(data.len());
                        // SAFETY: `fentry` is a fresh allocation of `len` >= `n` bytes
                        // (`hdr` <= `UDF_EXTFENTRY_SIZE`), which cannot overlap the buffer.
                        unsafe {
                            ptr::copy_nonoverlapping(data.as_ptr(), fentry.as_ptr(), n);
                        }
                        Some(Ok((fentry, len)))
                    }
                }
            }
        }
    };

    brelse(bp);

    let (fentry, len) = match copied {
        Some(Ok(f)) => f,
        // Cannot allocate file entry block, or not a file entry.
        None | Some(Err(())) => {
            pool_put(&UNODE_POOL, mem);
            return Err(Errno::ENOMEM);
        }
    };
    up.u_fentry.set(Some(fentry));
    up.u_fentry_len.set(len);

    let vp = match udf_allocv(mp, p) {
        Ok(vp) => vp,
        Err(e) => {
            free(fentry, M_UDFFENTRY, len);
            pool_put(&UNODE_POOL, mem);
            return Err(e); // Error from udf_allocv()
        }
    };

    up.u_vnode.set(Some(vp));
    up.u_ino.set(ino);
    up.u_devvp.set(Some(devvp));
    up.u_dev.set(ump.um_dev.get());
    up.u_ump.set(Some(ump));
    vp.v_data.set(upp.as_ptr().cast());
    vref(devvp);

    rrw_init_flags(&up.u_lock, "unode", RWL_DUPOK | RWL_IS_VNODE);

    // udf_hashins() will lock the vnode for us.
    let _ = udf_hashins(up);

    let vtype = match up.fentry().icbtag.file_type {
        UDF_ICB_FILETYPE_DIRECTORY => VDIR,
        UDF_ICB_FILETYPE_BLOCKDEVICE => VBLK,
        UDF_ICB_FILETYPE_CHARDEVICE => VCHR,
        UDF_ICB_FILETYPE_FIFO => VFIFO,
        UDF_ICB_FILETYPE_SOCKET => VSOCK,
        UDF_ICB_FILETYPE_SYMLINK => VLNK,
        UDF_ICB_FILETYPE_RANDOMACCESS | UDF_ICB_FILETYPE_REALTIME | UDF_ICB_FILETYPE_UNKNOWN => {
            VREG
        }
        _ => {
            printf(format_args!(
                "Unrecognized file type ({})\n",
                vp.v_type.get() as i32
            ));
            VREG
        }
    };
    vp.v_type.set(vtype);

    // check if this is a vnode alias
    let mut vp = vp;
    if let Some(nvp) = checkalias(vp, up.u_dev.get(), ump.um_mountp.get()) {
        printf(format_args!("found a vnode alias\n"));
        // Discard unneeded vnode, but save its udf_node. Note that the lock is carried over
        // in the udf_node
        nvp.v_data.set(vp.v_data.get());
        vp.v_data.set(ptr::null_mut());
        vp.v_op.set(Some(&SPEC_VOPS));
        vrele(vp);
        vgone(vp);
        // Reinitialize aliased inode.
        vp = nvp;
        ump.um_devvp.set(Some(vp));
    }

    Ok(vp)
}

/// `struct ifid`: a UDF file handle, overlaid on a `struct fid`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ifid {
    /// `ifid_len`.
    pub ifid_len: u16,
    /// `ifid_pad`.
    pub ifid_pad: u16,
    /// `ifid_ino`.
    pub ifid_ino: i32,
    /// `ifid_start`.
    pub ifid_start: i64,
}

impl Ifid {
    /// `sizeof(struct ifid)`.
    pub const SIZE: usize = 16;

    /// `(struct ifid *)fhp`: the handle a `struct fid` holds.
    pub fn from_fid(fid: &Fid) -> Self {
        let d = &fid.fid_data;
        Self {
            ifid_len: fid.fid_len,
            ifid_pad: fid.fid_reserved,
            ifid_ino: i32::from_ne_bytes([d[0], d[1], d[2], d[3]]),
            ifid_start: i64::from_ne_bytes([d[4], d[5], d[6], d[7], d[8], d[9], d[10], d[11]]),
        }
    }
}

/// `udf_fhtovp` (`vfs_fhtovp`): the vnode a file handle names, locked.
pub fn udf_fhtovp(mp: &'static Mount, fhp: &Fid) -> Result<&'static Vnode, Errno> {
    let ifhp = Ifid::from_fid(fhp);

    // The C passes the `int` to an `ino_t`: a negative one sign-extends.
    VFS_VGET(mp, i64::from(ifhp.ifid_ino) as Ino)
}

/// `udf_vptofh` (`vfs_vptofh`): sets the length and the file number of `fhp` (the rest of it
/// is left as it is, as in C).
pub fn udf_vptofh(vp: &'static Vnode, fhp: &mut Fid) -> Result<(), Errno> {
    let up = VTOU(vp);
    fhp.fid_len = Ifid::SIZE as u16;
    fhp.fid_data[0..4].copy_from_slice(&(up.u_ino.get() as i32).to_ne_bytes());

    Ok(())
}

/// `udf_checkexp` (`vfs_checkexp`): no exports, for the time being.
pub fn udf_checkexp(
    _mp: &'static Mount,
    _nam: &Mbuf,
    _exflagsp: &mut i32,
    _credanonp: &mut *const Ucred,
) -> Result<(), Errno> {
    Err(Errno::EACCES) // For the time being
}

/// Handle a virtual partition map.
pub fn udf_get_vpartmap(ump: &Umount, _pmv: &PartMapVirt) -> Result<(), Errno> {
    ump.um_flags.set(ump.um_flags.get() | UDF_MNT_FIND_VAT); // Should do more than this
    Ok(())
}

/// Handle a sparable partition map. `st_loc` is the bytes that follow the map's fixed part
/// (the C's `pms->st_loc[]`).
pub fn udf_get_spartmap(ump: &Umount, pms: &PartMapSpare, st_loc: &[u8]) -> Result<(), Errno> {
    let st_size = letoh32(pms.st_size) as usize;
    let Some(stbl) = malloc(st_size, M_UDFMOUNT, M_NOWAIT) else {
        return Err(Errno::ENOMEM);
    };
    // SAFETY: `stbl` is a fresh allocation of `st_size` bytes, ours alone.
    unsafe { ptr::write_bytes(stbl.as_ptr(), 0, st_size) };
    let forget = || free(stbl, M_UDFMOUNT, st_size);

    // Calculate the number of sectors per packet
    ump.um_psecs.set(
        i32::from(letoh16(pms.packet_len))
            .checked_div(ump.um_bsize.get())
            .unwrap_or(0),
    );

    let Some(loc0) = st_loc.get(..4) else {
        forget();
        return Err(Errno::EINVAL);
    };
    let loc0 = letoh32(u32::from_ne_bytes([loc0[0], loc0[1], loc0[2], loc0[3]]));
    let (bp, error) = udf_readlblks(ump, loc0 as i32, st_size as i32);

    if let Err(e) = error {
        brelse(bp);
        forget();
        return Err(e); // Failed to read sparing table
    }

    {
        // SAFETY: the buffer is ours (busy from bread) and mapped; the slice dies in this
        // block.
        let data = unsafe { bdata(bp) };
        let n = st_size.min(data.len());
        // SAFETY: `stbl` is our fresh allocation of `st_size` >= `n` bytes, apart from the
        // buffer.
        unsafe { ptr::copy_nonoverlapping(data.as_ptr(), stbl.as_ptr(), n) };
    }
    brelse(bp);

    ump.um_stbl.set(Some(stbl));
    ump.um_stbl_size.set(st_size);

    let st = ump.stbl_bytes();
    let tag_ok = UdfSparingTable::at(st, 0).is_some_and(|t| udf_checktag(&t.tag, 0).is_ok());
    if !tag_ok {
        ump.um_stbl.set(None);
        forget();
        return Err(Errno::EINVAL); // Invalid sparing table found
    }
    let rt_l = UdfSparingTable::at(st, 0).map_or(0, |t| letoh16(t.rt_l));

    // See how many valid entries there are here. The list is supposed to be sorted,
    // 0xfffffff0 and higher are not valid.
    for i in 0..usize::from(rt_l) {
        ump.um_stbl_len.set(i as i32);
        let off =
            UdfSparingTable::SIZE + i * size_of::<crate::isofs::udf::ecma167_udf::SpareMapEntry>();
        let Some(e) = crate::isofs::udf::ecma167_udf::SpareMapEntry::at(st, off) else {
            break;
        };
        if letoh32(e.org) >= 0xffff_fff0 {
            break;
        }
    }

    Ok(())
}

/// Handle a metadata partition map.
pub fn udf_get_mpartmap(ump: &Umount, pmm: &PartMapMeta) -> Result<(), Errno> {
    ump.um_flags.set(ump.um_flags.get() | UDF_MNT_USES_META);
    ump.um_meta_start.set(pmm.meta_file_lbn);
    Ok(())
}

/// Scan the partition maps. `maps` is the bytes that follow the logical volume descriptor
/// (its `maps[]`).
pub fn udf_find_partmaps(ump: &Umount, lvd: &LogvolDesc, maps: &[u8]) -> Result<(), Errno> {
    let mut pmap = 0usize;

    for _ in 0..letoh32(lvd.n_pm) {
        let (Some(&ptype), Some(&psize)) = (maps.get(pmap), maps.get(pmap + 1)) else {
            return Err(Errno::EINVAL);
        };
        let psize = usize::from(psize);

        if ptype != 1 && ptype != 2 {
            return Err(Errno::EINVAL); // Invalid partition map type
        }

        if psize != PartMap1::SIZE && psize != PartMap2::SIZE {
            return Err(Errno::EINVAL); // Invalid partition map size
        }

        if ptype == 1 {
            pmap += PartMap1::SIZE;
            continue;
        }

        // Type 2 map. Find out the details
        let Some(pm2) = PartMap2::at(maps, pmap) else {
            return Err(Errno::EINVAL);
        };
        let regid_id: [u8; UDF_REGID_ID_SIZE] = pm2.part_id.id;

        // The literals compared as the C's bcmp of UDF_REGID_ID_SIZE bytes does: "*UDF
        // Virtual Partition" is one byte shorter, its NUL takes part.
        let error = if &regid_id == b"*UDF Virtual Partition\0" {
            match PartMapVirt::at(maps, pmap) {
                Some(pmv) => udf_get_vpartmap(ump, pmv),
                None => Err(Errno::EINVAL),
            }
        } else if &regid_id == b"*UDF Sparable Partition" {
            match PartMapSpare::at(maps, pmap) {
                Some(pms) => udf_get_spartmap(ump, pms, &maps[pmap + PartMapSpare::SIZE..]),
                None => Err(Errno::EINVAL),
            }
        } else if &regid_id == b"*UDF Metadata Partition" {
            match PartMapMeta::at(maps, pmap) {
                Some(pmm) => udf_get_mpartmap(ump, pmm),
                None => Err(Errno::EINVAL),
            }
        } else {
            return Err(Errno::EINVAL); // Unsupported partition map
        };

        error?; // Error getting partition

        pmap += PartMap2::SIZE;
    }

    Ok(())
}
