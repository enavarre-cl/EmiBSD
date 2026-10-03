/*	$OpenBSD: subr_disk.c,v 1.287 2026/08/09 19:22:49 gnezdo Exp $	*/
/*	$NetBSD: subr_disk.c,v 1.17 1996/03/16 23:17:08 christos Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1995 Jason R. Thorpe.  All rights reserved.
 * Copyright (c) 1982, 1986, 1988, 1993
 *	The Regents of the University of California.  All rights reserved.
 * (c) UNIX System Laboratories, Inc.
 * All or some portions of this file are derived from material licensed
 * to the University of California by American Telephone and Telegraph
 * Co. or Unix System Laboratories, Inc. and are reproduced herein with
 * the permission of UNIX System Laboratories, Inc.
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
 *	@(#)ufs_disksubr.c	8.5 (Berkeley) 1/21/94
 */
/* </LICENSES> */

//! The disk layer: label checks and the DOS/MBR label spoofing (`dkcksum`,
//! `initdisklabel`, `checkdisklabel`, `readdoslabel`, `setdisklabel`), transfer bounds
//! (`bounds_check_with_label`), the list of attached disks (`disk_attach`, `disk_lookup`,
//! `disk_openpart`, ...) and mounting a disk root (`dk_mountroot`, `disk_readlabel`).
//!
//! Upstream: sys/kern/subr_disk.c @ 3ce1f3f79392
//!
//! Status: `wip`. Ported: everything a disk driver calls (rd(4) first), `dk_mountroot` with
//! `disk_readlabel`, and `setroot` with `getdisk`/`parsedisk` (the root/swap/dump choice).
//! Not yet ported: `disk_map` (the DUID lookup of `opendev(3)`'s `DIOCMAP`).
//!
//! ## Deviations
//! - `gpt_get_hdr` returns the header (zeroed when invalid) and `gpt_get_parts` the
//!   partition array as an `Option<Vec<u8>>` (the C's malloc'd `*gp`, NULL on a bad
//!   checksum); the array is read entry by entry with `GptPartition::from_bytes`. The
//!   checksums are `crc32` (`sys/lib/libz`).
//! - `checkdisklabel` takes the raw label and the in-core label as two `&mut`: the C's
//!   `lp != dlp` test is always true for its callers (the raw label sits in a sector buffer),
//!   so the copy is unconditional.
//! - `readdoslabel`'s `daddr_t *partoffp` is an `Option<&mut Daddr>`, its `spoofonly` a
//!   `bool`; the temporary label the C mallocs is a local copy.
//! - `bounds_check_with_label` returns `true` where the C returns 0 (go on with the
//!   transfer) and `false` for -1 (the buffer is finished: an error or end of partition).
//! - `disk_readlabel` returns `Err(DiskReadlabelError)`, whose `Display` is the C's message,
//!   where the C fills `errbuf` and returns it.
//! - `duid_format` returns the 16 hex digits by value instead of a static buffer.
//! - `softraid_disk_attach` and `sr_map_root` are `NSOFTRAID` (not configured); `DEBUG`'s
//!   `DPRINTF`s are not configured.
//! - `dk_mountroot`: `FFS` (feature `ffs`) is the only file system the kernel configuration
//!   names with a mountroot; `EXT2FS` and `CD9660` are not configured.
//! - `setroot`'s `RB_ASKNAME` dialogue (the "root device:" and "swap device:" prompts read
//!   with `getsn` under `cnpollc`) is reported and skipped: `getsn` is not ported. A kernel
//!   booted with `-a` goes on with its configured root. `NFSCLIENT` (the `nfs_mountroot`
//!   branches) is not configured.
//! - `parsedisk` returns the device and the `dev_t` as an `Option` pair instead of filling
//!   `*devp`; `getdisk` likewise.
//! - `disk_attach_callback`'s `struct disk_attach_task` is a malloc'd [`DiskAttachTask`], as
//!   in C.

use alloc::vec::Vec;
use core::ffi::c_void;
use core::fmt;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicI32, AtomicPtr, Ordering};

use libkern::StaticCell;
use libz::crc32;

use crate::dev::rnd::{arc4random_buf, enqueue_randomness};
use crate::kern::init_main::BOOTHOWTO;
use crate::kern::kern_lock::{mtx_enter, mtx_init, mtx_leave};
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_rwlock::{rw_enter, rw_enter_write, rw_exit_write, rw_init_flags};
use crate::kern::kern_synch::tsleep_nsec;
use crate::kern::kern_synch::wakeup;
use crate::kern::kern_task::{SYSTQ, task_add, task_set};
use crate::kern::kern_tc::microuptime;
use crate::kern::subr_autoconf::ALLDEVS;
use crate::kern::subr_autoconf::{device_lookup, device_ref, device_unref};
use crate::kern::subr_prf::Str;
use crate::kern::subr_prf::{addlog, log, panic, printf};
use crate::kern::subr_xxx::blktochr;
use crate::kern::vfs_bio::biowait;
use crate::kern::vfs_subr::{cdevvp, vdevgone, vput};
use crate::kern::vfs_vops::{VOP_CLOSE, VOP_IOCTL, VOP_OPEN};
use crate::machine::autoconf::nam2blk;
use crate::machine::conf::{bdevsw, cdevsw, nblkdev, nchrdev};
use crate::machine::cpu::curproc;
use crate::machine::intr::IPL_BIO;
use crate::net::if_::{if_addgroup, if_put, if_unit};
use crate::sys::buf::{B_BUSY, B_DONE, B_ERROR, B_RAW, B_READ, B_WRITE, Buf};
use crate::sys::conf::SWDEVT;
use crate::sys::conf::{DevTypeOpen, DevTypeStrategy};
use crate::sys::device::{Cfdriver, Device};
use crate::sys::device::{DV_DISK, DV_IFNET};
use crate::sys::disk::{DKF_CONSTRUCTED, DKF_NOLABELREAD, DKF_OPENED, Disk, DisklistHead};
use crate::sys::disklabel::{
    DISKLABEL_SIZE, DISKMAGIC, DOS_LABELSECTOR, DOS_MAXEBR, DOSBBSECTOR, DOSMBR_SIGNATURE,
    DOSMBR_SIGNATURE_OFF, DOSPTYP_EFI, DOSPTYP_EFISYS, DOSPTYP_EXTEND, DOSPTYP_EXTENDL,
    DOSPTYP_FAT12, DOSPTYP_FAT16B, DOSPTYP_FAT16L, DOSPTYP_FAT16S, DOSPTYP_FAT32, DOSPTYP_FAT32L,
    DOSPTYP_LINUX, DOSPTYP_NTFS, DOSPTYP_OPENBSD, DOSPTYP_UNUSED, Disklabel, DosPartition, FS_BOOT,
    FS_BSDFFS, FS_EXT2FS, FS_HFS, FS_MSDOS, FS_NTFS, FS_OTHER, FS_UNUSED, GPTMINHDRSIZE,
    GPTMINPARTSIZE, GPTREVISION, GPTSECTOR, GPTSIGNATURE, GptHeader, GptPartition, MAXDISKSIZE,
    MAXPARTITIONS, NDOSPART, NSPARE, Partition, RAW_PART, diskminor, diskpart, diskunit,
    dl_blkoffset, dl_blkspersec, dl_blktosec, dl_getbend, dl_getbstart, dl_getdsize, dl_getpoffset,
    dl_getpsize, dl_partname2num, dl_partnum2name, dl_sectoblk, dl_setbend, dl_setbstart,
    dl_setdsize, dl_setpoffset, dl_setpsize, makediskdev,
};
use crate::sys::dkio::DIOCGDINFO;
use crate::sys::errno::Errno;
use crate::sys::fcntl::FREAD;
use crate::sys::malloc::{M_DEVBUF, M_NOWAIT, M_TEMP, M_WAITOK, M_ZERO};
use crate::sys::param::{DEV_BSHIFT, DEV_BSIZE, MAXPHYS, NODEV};
use crate::sys::queue::TailqHead;
use crate::sys::reboot::RB_ASKNAME;
use crate::sys::rwlock::{RW_INTR, RW_WRITE, RWL_IS_VNODE};
use crate::sys::stat::{S_IFBLK, S_IFCHR};
use crate::sys::syslog::LOG_PRINTF;
use crate::sys::systm::{DUMPDEV, MOUNTROOT, ROOTDEV};
use crate::sys::task::Task;
use crate::sys::time::sec_to_nsec;
use crate::sys::time::{timeradd, timersub};
use crate::sys::types::{Daddr, Dev, major};
use crate::sys::ucred::NOCRED;
use crate::sys::uuid::Uuid;
use crate::sys::vnode::{VBLK, VCHR};
use crate::unported;

/// `DUID_SIZE`.
pub const DUID_SIZE: usize = 8;

/// `struct disk_attach_task`: the label read `disk_attach` queues on `systq`.
pub struct DiskAttachTask {
    /// `task`.
    pub task: Task,
    /// `dk`: the disk whose label to read.
    pub dk: NonNull<Disk>,
}

/// Why `disk_readlabel` failed; `Display` writes the C's `errbuf` message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiskReadlabelError {
    /// `cannot obtain vnode for 0x%x/0x%x`.
    NoVnode {
        /// The device asked for.
        dev: Dev,
        /// Its raw character partition.
        rawdev: Dev,
    },
    /// `cannot open disk, 0x%x/0x%x, error %d`.
    Open {
        /// The device asked for.
        dev: Dev,
        /// Its raw character partition.
        rawdev: Dev,
        /// The open's error.
        error: Errno,
    },
    /// `cannot read disk label, 0x%x/0x%x, error %d`.
    Ioctl {
        /// The device asked for.
        dev: Dev,
        /// Its raw character partition.
        rawdev: Dev,
        /// The `DIOCGDINFO` error.
        error: Errno,
    },
}

impl fmt::Display for DiskReadlabelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::NoVnode { dev, rawdev } => {
                write!(f, "cannot obtain vnode for {dev:#x}/{rawdev:#x}")
            }
            Self::Open { dev, rawdev, error } => write!(
                f,
                "cannot open disk, {dev:#x}/{rawdev:#x}, error {}",
                error as i32
            ),
            Self::Ioctl { dev, rawdev, error } => write!(
                f,
                "cannot read disk label, {dev:#x}/{rawdev:#x}, error {}",
                error as i32
            ),
        }
    }
}

/// `disklist`: a global list of all disks attached to the system. May grow or shrink over
/// time.
pub static DISKLIST: DisklistHead = DisklistHead(TailqHead::new());
/// `disk_count`: number of drives in global disklist.
pub static DISK_COUNT: AtomicI32 = AtomicI32::new(0);
/// `disk_change`: set if a disk has been attached/detached since last we looked at this
/// variable. This is reset by `hw_sysctl()`.
pub static DISK_CHANGE: AtomicI32 = AtomicI32::new(0);

/// `bootduid`: DUID of boot disk. Written by the machine's boot glue before autoconf.
pub static BOOTDUID: StaticCell<[u8; DUID_SIZE]> = StaticCell::new([0; DUID_SIZE]);
/// `rootduid`: DUID of root disk. Written by `setroot`.
pub static ROOTDUID: StaticCell<[u8; DUID_SIZE]> = StaticCell::new([0; DUID_SIZE]);

/// `rootdv`: the root device, chosen by `setroot`.
pub static ROOTDV: AtomicPtr<Device> = AtomicPtr::new(ptr::null_mut());

/// Compute checksum for disk label.
pub fn dkcksum(lp: &Disklabel) -> u16 {
    lp.cksum_words(usize::from(lp.d_npartitions))
        .fold(0, |sum, w| sum ^ w)
}

/// `initdisklabel`: the minimal requirements for an archetypal disk label: the raw
/// partition covers the disk, the others are empty.
pub fn initdisklabel(lp: &mut Disklabel) -> Result<(), Errno> {
    // minimal requirements for archetypal disk label
    if lp.d_secsize as usize > MAXPHYS {
        return Err(Errno::ERANGE);
    }
    if (lp.d_secsize as usize) < DEV_BSIZE {
        lp.d_secsize = DEV_BSIZE as u32;
    }
    if dl_getdsize(lp) == 0 {
        dl_setdsize(lp, MAXDISKSIZE);
    }
    if lp.d_secpercyl == 0 {
        return Err(Errno::ERANGE);
    }
    lp.d_npartitions = MAXPARTITIONS as u16;
    for p in lp.d_partitions.iter_mut().take(RAW_PART as usize) {
        dl_setpsize(p, 0);
        dl_setpoffset(p, 0);
    }
    let dsize = dl_getdsize(lp);
    let raw = &mut lp.d_partitions[RAW_PART as usize];
    if dl_getpsize(raw) == 0 {
        dl_setpsize(raw, dsize);
    }
    dl_setpoffset(raw, 0);
    dl_setbstart(lp, 0);
    dl_setbend(lp, dsize);
    lp.d_version = 1;
    Ok(())
}

/// Check an incoming block to make sure it is a disklabel, convert it to a newer version
/// if needed, etc etc. `dlp` is the label as read from the disk, `lp` the in-core label,
/// which comes in holding the real disk size and leaves holding the checked label.
pub fn checkdisklabel(
    _dev: Dev,
    dlp: &mut Disklabel,
    lp: &mut Disklabel,
    boundstart: u64,
    boundend: u64,
) -> Result<(), Errno> {
    // These fields may not be 0, no point trying a byteswap
    if dlp.d_secpercyl == 0 || dlp.d_nsectors == 0 || dlp.d_version == 0 {
        return Err(Errno::EINVAL); // invalid label
    } else if dlp.d_secsize == 0 {
        return Err(Errno::ENOSPC); // disk too small
    }

    let error = if dlp.d_magic != DISKMAGIC || dlp.d_magic2 != DISKMAGIC {
        Some(Errno::ENOENT) // no disk label
    } else if usize::from(dlp.d_npartitions) > MAXPARTITIONS {
        Some(Errno::E2BIG) // too many partitions
    } else if dkcksum(dlp) != 0 {
        Some(Errno::EINVAL) // incorrect checksum
    } else {
        None
    };

    if let Some(error) = error {
        // If it is byte-swapped, attempt to convert it
        if dlp.d_magic.swap_bytes() != DISKMAGIC
            || dlp.d_magic2.swap_bytes() != DISKMAGIC
            || usize::from(dlp.d_npartitions.swap_bytes()) > MAXPARTITIONS
        {
            return Err(error);
        }

        // Need a byte-swap aware dkcksum variant inlined, because dkcksum uses a sub-field
        let sum = dlp
            .cksum_words(usize::from(dlp.d_npartitions.swap_bytes()))
            .fold(0u16, |sum, w| sum ^ w);
        if sum != 0 {
            return Err(error);
        }

        dlp.d_magic = dlp.d_magic.swap_bytes();
        dlp.d_type = dlp.d_type.swap_bytes();

        // d_typename and d_packname are strings

        dlp.d_secsize = dlp.d_secsize.swap_bytes();
        dlp.d_nsectors = dlp.d_nsectors.swap_bytes();
        dlp.d_ntracks = dlp.d_ntracks.swap_bytes();
        dlp.d_ncylinders = dlp.d_ncylinders.swap_bytes();
        dlp.d_secpercyl = dlp.d_secpercyl.swap_bytes();
        dlp.d_secperunit = dlp.d_secperunit.swap_bytes();

        // d_uid is a string

        dlp.d_acylinders = dlp.d_acylinders.swap_bytes();

        dlp.d_flags = dlp.d_flags.swap_bytes();

        dlp.d_secperunith = dlp.d_secperunith.swap_bytes();
        dlp.d_version = dlp.d_version.swap_bytes();

        for s in dlp.d_spare.iter_mut().take(NSPARE) {
            *s = s.swap_bytes();
        }

        dlp.d_magic2 = dlp.d_magic2.swap_bytes();

        dlp.d_npartitions = dlp.d_npartitions.swap_bytes();

        for pp in dlp.d_partitions.iter_mut().take(MAXPARTITIONS) {
            pp.p_size = pp.p_size.swap_bytes();
            pp.p_offset = pp.p_offset.swap_bytes();
            pp.p_offseth = pp.p_offseth.swap_bytes();
            pp.p_sizeh = pp.p_sizeh.swap_bytes();
            pp.p_cpg = pp.p_cpg.swap_bytes();
        }

        dlp.d_checksum = 0;
        dlp.d_checksum = dkcksum(dlp);
    }

    // XXX should verify lots of other fields and whine a lot

    // Initial passed in lp contains the real disk size.
    let disksize = dl_getdsize(lp);

    *lp = *dlp;

    dl_setdsize(lp, disksize);
    let raw = &mut lp.d_partitions[RAW_PART as usize];
    dl_setpsize(raw, disksize);
    dl_setpoffset(raw, 0);
    dl_setbstart(lp, boundstart);
    dl_setbend(lp, boundend.min(dl_getdsize(lp)));

    lp.d_checksum = 0;
    lp.d_checksum = dkcksum(lp);
    Ok(())
}

/// Read a disk sector.
pub fn readdisksector(
    bp: &'static Buf,
    strat: DevTypeStrategy,
    lp: &Disklabel,
    sector: u64,
) -> Result<(), Errno> {
    bp.b_blkno.set(dl_sectoblk(lp, sector) as Daddr);
    bp.b_bcount.set(i64::from(lp.d_secsize));
    bp.b_error.set(None);
    bp.clr(B_READ | B_WRITE | B_DONE | B_ERROR);
    bp.set(B_BUSY | B_READ | B_RAW);

    strat(bp);

    biowait(bp)
}

/// The first `n` bytes of a busy buffer's data, copied out.
fn buf_bytes<const N: usize>(bp: &Buf, off: usize) -> [u8; N] {
    let mut out = [0u8; N];
    // SAFETY: the caller holds the buffer busy (`B_BUSY`, from `geteblk`), so nobody else
    // touches its data; `data()` is the mapped `b_bcount` bytes.
    let data = unsafe { bp.data() };
    if let Some(src) = data.get(off..) {
        let n = src.len().min(N);
        out[..n].copy_from_slice(&src[..n]);
    }
    out
}

/// `readdoslabel`: reads the DOS boot block and spoofs a label from its MBR (or GPT, or
/// FAT boot sector), then reads the disklabel where the spoofed label says it is. With
/// `partoffp`, only finds the label's block (for `writedisklabel`) and leaves `lp` alone.
pub fn readdoslabel(
    bp: &'static Buf,
    strat: DevTypeStrategy,
    lp: &mut Disklabel,
    partoffp: Option<&mut Daddr>,
    spoofonly: bool,
) -> Result<(), Errno> {
    readdisksector(bp, strat, lp, DOSBBSECTOR)?;
    let dosbb: [u8; DEV_BSIZE] = buf_bytes(bp, 0);

    let mut nlp = *lp;
    nlp.d_partitions = [Partition::new(); _];
    nlp.d_partitions[RAW_PART as usize] = lp.d_partitions[RAW_PART as usize];
    nlp.d_magic = 0;

    let mut partoff: Daddr = 0;
    spoofgpt(bp, strat, &dosbb, &mut nlp, &mut partoff)?;
    if nlp.d_magic != DISKMAGIC {
        spoofmbr(bp, strat, &dosbb, &mut nlp, &mut partoff);
    }
    if nlp.d_magic != DISKMAGIC {
        spooffat(&dosbb, &mut nlp, &mut partoff);
    }
    if nlp.d_magic != DISKMAGIC {
        // readdoslabel: N/A -- label partition @ daddr_t 0 (default)
        partoff = 0;
    }

    if let Some(partoffp) = partoffp {
        // If a non-zero value is returned writedisklabel() exits with EIO. If 0 is
        // returned the label sector is read from disk and lp is copied into it. So leave
        // lp alone!
        if partoff == -1 {
            return Err(Errno::ENXIO);
        }
        *partoffp = partoff;
        return Ok(());
    }

    nlp.d_magic = lp.d_magic;
    *lp = nlp;

    lp.d_checksum = 0;
    lp.d_checksum = dkcksum(lp);

    if spoofonly || partoff == -1 {
        return Ok(());
    }

    partoff += DOS_LABELSECTOR;
    if readdisksector(bp, strat, lp, dl_blktosec(lp, partoff as u64)).is_err() {
        return Err(bp.b_error.get().unwrap_or(Errno::EIO));
    }

    let mut rlp = Disklabel::from_bytes(&buf_bytes::<DISKLABEL_SIZE>(
        bp,
        dl_blkoffset(lp, partoff as u64) as usize,
    ));
    let (bstart, bend) = (dl_getbstart(&rlp), dl_getbend(&rlp));
    checkdisklabel(bp.b_dev.get(), &mut rlp, lp, bstart, bend)
}

/// Return the index into `dp[]` of the EFI GPT (0xEE) partition, or `None` if no such
/// partition exists.
pub fn gpt_chk_mbr(dp: &[DosPartition; NDOSPART], dsize: u64) -> Option<usize> {
    let mut found = 0;
    let mut efi = 0;
    let mut eficnt = 0;
    for (i, dp2) in dp.iter().enumerate() {
        if dp2.dp_typ == DOSPTYP_UNUSED {
            continue;
        }
        found += 1;
        if dp2.dp_typ != DOSPTYP_EFI {
            continue;
        }
        if u64::from(u32::from_le(dp2.dp_start)) != GPTSECTOR {
            continue;
        }
        let psize = u32::from_le(dp2.dp_size);
        if u64::from(psize) <= dsize.wrapping_sub(GPTSECTOR) || psize == u32::MAX {
            efi = i;
            eficnt += 1;
        }
    }
    if found == 1 && eficnt == 1 {
        Some(efi)
    } else {
        None
    }
}

/// `gpt_get_hdr`: reads the GPT header at `sector` and returns it, or a zeroed header when
/// it is not a valid one (wrong signature, revision, sizes or checksum).
pub fn gpt_get_hdr(
    bp: &'static Buf,
    strat: DevTypeStrategy,
    lp: &Disklabel,
    sector: u64,
) -> Result<GptHeader, Errno> {
    readdisksector(bp, strat, lp, sector)?;

    let mut raw: [u8; GPTMINHDRSIZE as usize] = buf_bytes(bp, 0);
    let ngh = GptHeader::from_bytes(&raw);

    let size = u32::from_le(ngh.gh_size);
    let partsize = u32::from_le(ngh.gh_part_size);
    let lbaend = u64::from_le(ngh.gh_lba_end);
    let lbastart = u64::from_le(ngh.gh_lba_start);

    let csum = ngh.gh_csum;
    raw[GptHeader::CSUM_OFF..GptHeader::CSUM_OFF + 4].fill(0);
    let ncsum = crc32(0, &raw).to_le();

    if u64::from_le(ngh.gh_sig) == GPTSIGNATURE
        && u32::from_le(ngh.gh_rev) == GPTREVISION
        && size == GPTMINHDRSIZE
        && lbastart <= lbaend
        && partsize == GPTMINPARTSIZE
        && lp.d_secsize.is_multiple_of(partsize)
        && csum == ncsum
    {
        Ok(GptHeader {
            gh_csum: ncsum,
            ..ngh
        })
    } else {
        Ok(GptHeader::default())
    }
}

/// `gpt_get_parts`: reads and checks the partition array `gh` points at; `None` (the C's
/// NULL `*gp`) when its checksum does not match.
pub fn gpt_get_parts(
    bp: &'static Buf,
    strat: DevTypeStrategy,
    lp: &Disklabel,
    gh: &GptHeader,
) -> Result<Option<Vec<u8>>, Errno> {
    let partlba = u64::from_le(gh.gh_part_lba);
    let partnum = u32::from_le(gh.gh_part_num);
    let partsize = u32::from_le(gh.gh_part_size);
    let secsize = u64::from(lp.d_secsize);

    let sectors = (u64::from(partnum) * u64::from(partsize)).div_ceil(secsize);

    // mallocarray(sectors, d_secsize, M_DEVBUF, M_NOWAIT | M_ZERO): a fallible Vec.
    let bytes = sectors
        .checked_mul(secsize)
        .and_then(|b| usize::try_from(b).ok())
        .ok_or(Errno::ENOMEM)?;
    let mut ngp: Vec<u8> = Vec::new();
    ngp.try_reserve_exact(bytes).map_err(|_| Errno::ENOMEM)?;
    ngp.resize(bytes, 0);

    for i in 0..sectors {
        readdisksector(bp, strat, lp, partlba + i)?;
        let off = (i * secsize) as usize;
        // SAFETY: the buffer is busy (geteblk), so its data is ours; `data()` is the
        // `b_bcount` (one sector) bytes just read.
        let data = unsafe { bp.data() };
        let n = data.len().min(secsize as usize);
        ngp[off..off + n].copy_from_slice(&data[..n]);
    }

    let len = (partnum as usize) * (partsize as usize);
    let partcsum = crc32(0, &ngp[..len.min(bytes)]).to_le();
    if partcsum != gh.gh_part_csum {
        // DEBUG: DPRINTF("invalid %s GPT partition array @ %llu\n", ...): not configured.
        return Ok(None);
    }
    Ok(Some(ngp))
}

/// The GPT partition type GUIDs `gpt_get_fstype` knows, in memory order (LE format!), with
/// their file system types.
const KNOWNFS: [([u8; 16], u8); 8] = [
    // GPT_UUID_UNUSED
    ([0; 16], FS_UNUSED),
    // GPT_LEUUID_OPENBSD
    (
        [
            0xa0, 0xc7, 0x4c, 0x82, 0xa8, 0x36, 0xe3, 0x11, 0x89, 0x0a, 0x95, 0x25, 0x19, 0xad,
            0x3f, 0x61,
        ],
        FS_BSDFFS,
    ),
    // GPT_UUID_MICROSOFT_BASIC_DATA
    (
        [
            0xa2, 0xa0, 0xd0, 0xeb, 0xe5, 0xb9, 0x33, 0x44, 0x87, 0xc0, 0x68, 0xb6, 0xb7, 0x26,
            0x99, 0xc7,
        ],
        FS_MSDOS,
    ),
    // GPT_UUID_CHROMEOS_ROOTFS
    (
        [
            0x02, 0xe2, 0xb8, 0x3c, 0x7e, 0x3b, 0xdd, 0x47, 0x8a, 0x3c, 0x7f, 0xf2, 0xa1, 0x3c,
            0xfc, 0xec,
        ],
        FS_EXT2FS,
    ),
    // GPT_UUID_LINUX_FILES
    (
        [
            0xaf, 0x3d, 0xc6, 0x0f, 0x83, 0x84, 0x72, 0x47, 0x8e, 0x79, 0x3d, 0x69, 0xd8, 0x47,
            0x7d, 0xe4,
        ],
        FS_EXT2FS,
    ),
    // GPT_UUID_MAC_OS_X_HFS
    (
        [
            0x00, 0x53, 0x46, 0x48, 0x00, 0x00, 0xaa, 0x11, 0xaa, 0x11, 0x00, 0x30, 0x65, 0x43,
            0xec, 0xac,
        ],
        FS_HFS,
    ),
    // GPT_LEUUID_EFI_SYSTEM
    (
        [
            0x28, 0x73, 0x2a, 0xc1, 0x1f, 0xf8, 0xd2, 0x11, 0xba, 0x4b, 0x00, 0xa0, 0xc9, 0x3e,
            0xc9, 0x3b,
        ],
        FS_MSDOS,
    ),
    // GPT_UUID_BIOS_BOOT
    (
        [
            0x48, 0x61, 0x68, 0x21, 0x49, 0x64, 0x6f, 0x6e, 0x74, 0x4e, 0x65, 0x65, 0x64, 0x45,
            0x46, 0x49,
        ],
        FS_BOOT,
    ),
];

/// `gpt_get_fstype`: the file system type of a GPT partition type GUID.
pub fn gpt_get_fstype(uuid_part: &Uuid) -> u8 {
    let b = uuid_part.as_bytes();
    KNOWNFS
        .iter()
        .find(|(gptype, _)| *gptype == b)
        .map_or(FS_OTHER, |&(_, fstype)| fstype)
}

/// `spoofgpt`: spoof a label from a GPT: the OpenBSD partition's place (where the real
/// label is) and the other known partitions as `i`, `j`, ...
pub fn spoofgpt(
    bp: &'static Buf,
    strat: DevTypeStrategy,
    dosbb: &[u8; DEV_BSIZE],
    lp: &mut Disklabel,
    partoffp: &mut Daddr,
) -> Result<(), Errno> {
    let dp = DosPartition::table(dosbb);
    let sig = u16::from_ne_bytes([dosbb[DOSMBR_SIGNATURE_OFF], dosbb[DOSMBR_SIGNATURE_OFF + 1]]);

    if u16::from_le(sig) != DOSMBR_SIGNATURE || gpt_chk_mbr(&dp, dl_getdsize(lp)).is_none() {
        return Ok(());
    }

    let mut gp = None;
    let mut gh = gpt_get_hdr(bp, strat, lp, GPTSECTOR);
    if let Ok(h) = &gh
        && u64::from_le(h.gh_sig) == GPTSIGNATURE
    {
        match gpt_get_parts(bp, strat, lp, h) {
            Ok(parts) => gp = parts,
            Err(e) => gh = Err(e),
        }
    }

    let primary_bad = match &gh {
        Err(_) => true,
        Ok(h) => u64::from_le(h.gh_sig) != GPTSIGNATURE || gp.is_none(),
    };
    if primary_bad {
        gh = gpt_get_hdr(bp, strat, lp, dl_getdsize(lp) - 1);
        if let Ok(h) = &gh
            && u64::from_le(h.gh_sig) == GPTSIGNATURE
        {
            match gpt_get_parts(bp, strat, lp, h) {
                Ok(parts) => gp = parts,
                Err(e) => gh = Err(e),
            }
        }
    }

    let gh = gh?;
    let Some(gp) = gp else {
        return Err(Errno::ENXIO);
    };

    let lbastart = u64::from_le(gh.gh_lba_start);
    let lbaend = u64::from_le(gh.gh_lba_end);
    let partnum = u32::from_le(gh.gh_part_num);

    let mut n = (b'i' - b'a') as usize; // Start spoofing at 'i', a.k.a. 8.

    dl_setbstart(lp, lbastart);
    dl_setbend(lp, lbaend + 1);
    let mut partoff = dl_sectoblk(lp, lbastart) as Daddr;
    let mut obsdfound = false;
    for i in 0..partnum as usize {
        let off = i * GPTMINPARTSIZE as usize;
        let Some(entry) = gp.get(off..off + GPTMINPARTSIZE as usize) else {
            break;
        };
        let mut e = [0u8; GPTMINPARTSIZE as usize];
        e.copy_from_slice(entry);
        let part = GptPartition::from_bytes(&e);

        let fstype = gpt_get_fstype(&part.gp_type);
        if fstype == FS_UNUSED {
            continue;
        }
        if fstype == FS_OTHER {
            // DEBUG: "spoofgpt: Skipping partition %u (unknown filesystem)".
            continue;
        }

        let start = u64::from_le(part.gp_lba_start);
        if start > lbaend || start < lbastart {
            continue;
        }

        let end = u64::from_le(part.gp_lba_end);
        if start > end {
            continue;
        }

        if obsdfound && fstype == FS_BSDFFS {
            continue;
        }

        if fstype == FS_BSDFFS {
            obsdfound = true;
            partoff = dl_sectoblk(lp, start) as Daddr;
            let labelsec = dl_blktosec(lp, (partoff + DOS_LABELSECTOR) as u64);
            if labelsec > end.min(lbaend) {
                partoff = -1;
            }
            dl_setbstart(lp, start);
            dl_setbend(lp, end + 1);
            continue;
        }

        if partoff != -1 {
            let labelsec = dl_blktosec(lp, (partoff + DOS_LABELSECTOR) as u64);
            if labelsec >= start && labelsec <= end {
                partoff = -1;
            }
        }

        if n < MAXPARTITIONS && end <= lbaend {
            let pp = &mut lp.d_partitions[n];
            n += 1;
            pp.p_fstype = fstype;
            dl_setpoffset(pp, start);
            dl_setpsize(pp, end - start + 1);
        }
    }

    lp.d_magic = DISKMAGIC;
    *partoffp = partoff;
    // free(gp, M_DEVBUF, gpbytes): the Vec is dropped here.

    // DEBUG: the "readdoslabel: GPT -- ..." print is not configured.
    Ok(())
}

/// `mbr_get_fstype`: the file system type of an MBR partition type.
pub fn mbr_get_fstype(dp_typ: u8) -> u8 {
    match dp_typ {
        DOSPTYP_OPENBSD => FS_BSDFFS,
        DOSPTYP_UNUSED => FS_UNUSED,
        DOSPTYP_LINUX => FS_EXT2FS,
        DOSPTYP_NTFS => FS_NTFS,
        DOSPTYP_EFISYS | DOSPTYP_FAT12 | DOSPTYP_FAT16S | DOSPTYP_FAT16B | DOSPTYP_FAT16L
        | DOSPTYP_FAT32 | DOSPTYP_FAT32L => FS_MSDOS,
        // DOSPTYP_EFI, DOSPTYP_EXTEND, DOSPTYP_EXTENDL and the rest
        _ => FS_OTHER,
    }
}

/// `spoofmbr`: spoofs partitions `i`.. from an MBR and its extended boot records, and
/// finds where the OpenBSD label lives (the A6 partition, else free space at sector 1).
pub fn spoofmbr(
    bp: &'static Buf,
    strat: DevTypeStrategy,
    dosbb: &[u8; DEV_BSIZE],
    lp: &mut Disklabel,
    partoffp: &mut Daddr,
) {
    let sig = u16::from_ne_bytes([dosbb[DOSMBR_SIGNATURE_OFF], dosbb[DOSMBR_SIGNATURE_OFF + 1]]);
    if u16::from_le(sig) != DOSMBR_SIGNATURE {
        return;
    }
    let mut dp = DosPartition::table(dosbb);

    let mut sector: u64 = DOSBBSECTOR;
    let mut obsdfound = false;
    let mut partoff: Daddr = 0;
    let mut parts = 0u32;
    let mut n = usize::from(b'i' - b'a');
    let mut wander = true;
    let mut ebr = 0;
    let mut extoff: u32 = 0;
    while wander && ebr < DOS_MAXEBR {
        ebr += 1;
        wander = false;
        if sector < u64::from(extoff) {
            sector = u64::from(extoff);
        }

        if sector != DOSBBSECTOR {
            if readdisksector(bp, strat, lp, sector).is_err() {
                break;
            }
            let ebrbb: [u8; DEV_BSIZE] = buf_bytes(bp, 0);
            let sig =
                u16::from_ne_bytes([ebrbb[DOSMBR_SIGNATURE_OFF], ebrbb[DOSMBR_SIGNATURE_OFF + 1]]);
            if u16::from_le(sig) != DOSMBR_SIGNATURE {
                break;
            }
            dp = DosPartition::table(&ebrbb);
        }

        for d in dp {
            if u32::from_le(d.dp_size) == 0 {
                continue;
            }
            if obsdfound && d.dp_typ == DOSPTYP_OPENBSD {
                continue;
            }

            if d.dp_typ != DOSPTYP_OPENBSD {
                if u64::from(u32::from_le(d.dp_start)) > dl_getdsize(lp) {
                    continue;
                }
                if u64::from(u32::from_le(d.dp_size)) > dl_getdsize(lp) {
                    continue;
                }
            }

            let start = sector + u64::from(u32::from_le(d.dp_start));
            let end = start + u64::from(u32::from_le(d.dp_size));

            parts += 1;
            if !obsdfound {
                let labeloff = partoff + DOS_LABELSECTOR;
                if labeloff >= dl_sectoblk(lp, start) as Daddr
                    && labeloff < dl_sectoblk(lp, end) as Daddr
                {
                    partoff = -1;
                }
            }

            match d.dp_typ {
                DOSPTYP_OPENBSD => {
                    obsdfound = true;
                    partoff = dl_sectoblk(lp, start) as Daddr;
                    let labeloff = partoff + DOS_LABELSECTOR;
                    if labeloff >= dl_sectoblk(lp, end) as Daddr {
                        partoff = -1;
                    }
                    dl_setbstart(lp, start);
                    dl_setbend(lp, end);
                    continue;
                }
                DOSPTYP_EFI => continue,
                DOSPTYP_EXTEND | DOSPTYP_EXTENDL => {
                    sector = start + u64::from(extoff);
                    if extoff == 0 {
                        extoff = start as u32;
                        sector = 0;
                    }
                    wander = true;
                    continue;
                }
                _ => {}
            }

            let fstype = mbr_get_fstype(d.dp_typ);
            if n < MAXPARTITIONS {
                let pp = &mut lp.d_partitions[n];
                n += 1;
                pp.p_fstype = fstype;
                if start != 0 {
                    dl_setpoffset(pp, start);
                }
                dl_setpsize(pp, end - start);
            }
        }
    }

    if parts > 0 {
        lp.d_magic = DISKMAGIC;
        *partoffp = partoff;
    }
}

/// `spooffat`: a disk that is one FAT file system gets it as partition `i`, and no label.
pub fn spooffat(dosbb: &[u8; DEV_BSIZE], lp: &mut Disklabel, partoffp: &mut Daddr) {
    let secsize = u16::from_le_bytes([dosbb[11], dosbb[12]]);

    let valid_jmp = (dosbb[0] == 0xeb && dosbb[2] == 0x90) || dosbb[0] == 0xe9;
    let valid_fat = dosbb[16] == 1 || dosbb[16] == 2;
    let valid_sec =
        usize::from(secsize) >= DEV_BSIZE && secsize <= 4096 && secsize.is_multiple_of(512);

    if valid_jmp && valid_sec && valid_fat {
        let i = usize::from(b'i' - b'a');
        lp.d_partitions[i] = lp.d_partitions[RAW_PART as usize];
        lp.d_partitions[i].p_fstype = FS_MSDOS;
        *partoffp = -1;
        lp.d_magic = DISKMAGIC;
    }
}

/// Check new disk label for sensibility before setting it.
pub fn setdisklabel(olp: &mut Disklabel, nlp: &mut Disklabel, openmask: u64) -> Result<(), Errno> {
    // sanity clause
    if nlp.d_secpercyl == 0
        || nlp.d_secsize == 0
        || nlp.d_secsize as usize > MAXPHYS
        || !(nlp.d_secsize as usize).is_multiple_of(DEV_BSIZE)
    {
        return Err(Errno::EINVAL);
    }

    // special case to allow disklabel to be invalidated
    if nlp.d_magic == 0xffff_ffff {
        *olp = *nlp;
        return Ok(());
    }

    if nlp.d_magic != DISKMAGIC || nlp.d_magic2 != DISKMAGIC {
        return Err(Errno::ENOENT);
    } else if usize::from(nlp.d_npartitions) > MAXPARTITIONS {
        return Err(Errno::E2BIG);
    } else if dkcksum(nlp) != 0 {
        return Err(Errno::EINVAL);
    }

    // XXX missing check if other dos partitions will be overwritten

    for i in 0..MAXPARTITIONS {
        let opp = olp.d_partitions[i];
        let npp = &mut nlp.d_partitions[i];
        if openmask & (1u64 << i) != 0
            && (dl_getpoffset(npp) != dl_getpoffset(&opp) || dl_getpsize(npp) < dl_getpsize(&opp))
        {
            return Err(Errno::EBUSY);
        }
        // Copy internally-set partition information if new label doesn't include it. XXX
        if npp.p_fstype == FS_UNUSED && opp.p_fstype != FS_UNUSED {
            npp.p_fragblock = opp.p_fragblock;
            npp.p_cpg = opp.p_cpg;
        }
    }

    // Generate a UID if the disklabel does not already have one.
    if duid_iszero(&nlp.d_uid) {
        loop {
            arc4random_buf(&mut nlp.d_uid);
            let taken = DISKLIST
                .0
                .iter()
                .any(|dk| dk.label().is_some_and(|l| duid_equal(&l.d_uid, &nlp.d_uid)));
            if !taken && !duid_iszero(&nlp.d_uid) {
                break;
            }
        }
    }

    // Preserve the disk size and RAW_PART values.
    dl_setdsize(nlp, dl_getdsize(olp));
    let dsize = dl_getdsize(nlp);
    let npp = &mut nlp.d_partitions[RAW_PART as usize];
    dl_setpoffset(npp, 0);
    dl_setpsize(npp, dsize);

    nlp.d_checksum = 0;
    nlp.d_checksum = dkcksum(nlp);
    *olp = *nlp;

    DISK_CHANGE.store(1, Ordering::Relaxed);

    Ok(())
}

/// Determine the size of the transfer, and make sure it is within the boundaries of the
/// partition. Adjust transfer if needed, and signal errors or early completion: `false`
/// means the buffer is finished (`b_resid` set, and `B_ERROR` on an error).
pub fn bounds_check_with_label(bp: &Buf, lp: &Disklabel) -> bool {
    let p = &lp.d_partitions[diskpart(bp.b_dev.get()) as usize];
    let blkno = bp.b_blkno.get();
    let bcount = bp.b_bcount.get();

    let ok = 'check: {
        // Avoid division by zero, negative offsets, and negative sizes.
        if lp.d_secpercyl == 0 || blkno < 0 || bcount < 0 {
            break 'check Err(());
        }

        // Ensure transfer is a whole number of aligned sectors.
        if !(blkno as u64).is_multiple_of(dl_blkspersec(lp))
            || bcount % i64::from(lp.d_secsize) != 0
        {
            break 'check Err(());
        }

        // Ensure transfer starts within partition boundary.
        let partblocks = dl_sectoblk(lp, dl_getpsize(p)) as Daddr;
        if blkno > partblocks {
            break 'check Err(());
        }

        // If exactly at end of partition or null transfer, return EOF.
        if blkno == partblocks || bcount == 0 {
            break 'check Ok(false);
        }

        // Truncate request if it extends past the end of the partition.
        let mut sz = bcount >> DEV_BSHIFT;
        if sz > partblocks - blkno {
            sz = partblocks - blkno;
            bp.b_bcount.set(sz << DEV_BSHIFT);
        }

        Ok(true)
    };

    match ok {
        Ok(true) => return true,
        Ok(false) => {}
        Err(()) => {
            bp.b_error.set(Some(Errno::EINVAL));
            bp.set(B_ERROR);
        }
    }
    bp.b_resid
        .set(usize::try_from(bp.b_bcount.get()).unwrap_or(0));
    false
}

/// Disk error is the preface to plaintive error messages about failing disk transfers. It
/// prints messages of the form
///
/// `hp0g: hard error reading fsbn 12345 of 12344-12347 (hp0 bn %d cn %d tn %d sn %d)`
///
/// if the offset of the error in the transfer and a disk label are both available.
/// `blkdone` should be -1 if the position of the error is unknown; the disklabel may be
/// absent from drivers that have not been converted to use them. The message is printed
/// with printf if `pri` is `LOG_PRINTF`, otherwise it uses log at the specified priority.
/// The message should be completed (with at least a newline) with printf or addlog,
/// respectively. There is no trailing space.
pub fn diskerr(bp: &Buf, dname: &str, what: &str, pri: i32, blkdone: i32, lp: Option<&Disklabel>) {
    let unit = diskunit(bp.b_dev.get());
    let part = diskpart(bp.b_dev.get());
    let partname = dl_partnum2name(part as usize).map_or('?', char::from);
    let pr = |args: fmt::Arguments<'_>| {
        if pri != LOG_PRINTF {
            addlog(args);
        } else {
            let _ = printf(args);
        }
    };

    if pri != LOG_PRINTF {
        log(pri, format_args!(""));
    }
    pr(format_args!(
        "{dname}{unit}{partname}: {what} {}ing fsbn ",
        if bp.isset(B_READ) { "read" } else { "writ" }
    ));
    let bcount = bp.b_bcount.get();
    let mut sn = bp.b_blkno.get();
    if bcount <= DEV_BSIZE as i64 {
        pr(format_args!("{sn}"));
    } else {
        if blkdone >= 0 {
            sn += Daddr::from(blkdone);
            pr(format_args!("{sn} of "));
        }
        pr(format_args!(
            "{}-{}",
            bp.b_blkno.get(),
            bp.b_blkno.get() + (bcount - 1) / DEV_BSIZE as i64
        ));
    }
    if let Some(lp) = lp
        && (blkdone >= 0 || bcount <= i64::from(lp.d_secsize))
    {
        sn += dl_sectoblk(lp, dl_getpoffset(&lp.d_partitions[part as usize])) as Daddr;
        let cyl = dl_sectoblk(lp, u64::from(lp.d_secpercyl)) as Daddr;
        let trk = dl_sectoblk(lp, u64::from(lp.d_nsectors)) as Daddr;
        pr(format_args!(
            " ({dname}{unit} bn {sn}; cn {}",
            sn / cyl.max(1)
        ));
        sn %= cyl.max(1);
        pr(format_args!(
            " tn {} sn {})",
            sn / trk.max(1),
            sn % trk.max(1)
        ));
    }
}

/// Initialize the disklist. Called by main() before autoconfiguration.
pub fn disk_init() {
    DISKLIST.0.init();
    DISK_COUNT.store(0, Ordering::Relaxed);
    DISK_CHANGE.store(0, Ordering::Relaxed);
}

/// `disk_construct`: initialises a disk's lock and mutex.
pub fn disk_construct(diskp: &Disk) {
    rw_init_flags(&diskp.dk_lock, "dklk", RWL_IS_VNODE);
    mtx_init(&diskp.dk_mtx, IPL_BIO);

    diskp.dk_flags.set(diskp.dk_flags.get() | DKF_CONSTRUCTED);
}

/// Attach a disk.
pub fn disk_attach(dv: Option<&Device>, diskp: &'static Disk) {
    // KERNEL_ASSERT_LOCKED(): nothing without MULTIPROCESSOR.

    if diskp.dk_flags.get() & DKF_CONSTRUCTED == 0 {
        disk_construct(diskp);
    }

    // Allocate and initialize the disklabel structures. Note that it's not safe to sleep
    // here, since we're probably going to be called during autoconfiguration.
    let Some(label) = malloc(DISKLABEL_SIZE, M_DEVBUF, M_NOWAIT | M_ZERO) else {
        panic(format_args!(
            "disk_attach: can't allocate storage for disklabel"
        ));
    };
    diskp.dk_label.set(Some(label.cast::<Disklabel>()));

    // Set the attached timestamp.
    diskp.dk_attachtime.set(microuptime());

    // Link into the disklist.
    // SAFETY: the disk is in no list (not yet attached) and lives in its driver's softc,
    // which stays in place until `disk_detach` unlinks it.
    unsafe { DISKLIST.0.insert_tail(diskp) };
    DISK_COUNT.fetch_add(1, Ordering::Relaxed);
    DISK_CHANGE.store(1, Ordering::Relaxed);

    // Store device structure and number for later use.
    diskp.dk_device.set(dv.map(NonNull::from));
    diskp.dk_devno.set(NODEV);
    if let Some(dv) = dv {
        let majdev = findblkmajor(dv);
        if majdev >= 0 {
            diskp.dk_devno.set(makediskdev(
                majdev as u32,
                dv.dv_unit.get() as u32,
                RAW_PART,
            ));
        }

        if diskp.dk_devno.get() != NODEV {
            let size = core::mem::size_of::<DiskAttachTask>();
            let Some(mem) = malloc(size, M_TEMP, M_WAITOK) else {
                panic(format_args!("disk_attach: out of memory"));
            };
            let dat = mem.cast::<DiskAttachTask>();

            // XXX: Assumes dk is part of the device softc.
            device_ref(dv);
            // SAFETY: a fresh allocation of `size` bytes, aligned for the task (malloc's
            // chunks are aligned to their power-of-two size).
            unsafe {
                dat.as_ptr().write(DiskAttachTask {
                    task: Task::zeroed(),
                    dk: NonNull::from(diskp),
                })
            };
            // SAFETY: just written; freed by `disk_attach_callback` only, after the task
            // ran, so the task lives as long as `systq` holds it.
            let task: &'static Task = unsafe { &(*dat.as_ptr()).task };
            task_set(task, disk_attach_callback, dat.as_ptr().cast());
            task_add(SYSTQ, task);
        }
    }

    // softraid_disk_attach: NSOFTRAID not configured.
}

/// `disk_attach_callback`: reads the label of a newly attached disk (from `systq`), so
/// that its `d_checksum` feeds the entropy pool and `setroot` can wait for it.
pub fn disk_attach_callback(xdat: *mut c_void) {
    let dat = xdat.cast::<DiskAttachTask>();
    // SAFETY: `disk_attach` passed its live `DiskAttachTask`; the task queue copied the
    // function and argument out before calling us, so it may go now.
    let dk = unsafe { (*dat).dk };
    if let Some(dat) = NonNull::new(dat) {
        free(dat.cast(), M_TEMP, core::mem::size_of::<DiskAttachTask>());
    }
    // SAFETY: the disk lives in its device's softc, which `disk_attach`'s `device_ref`
    // keeps until the `device_unref` below.
    let dk: &Disk = unsafe { dk.as_ref() };

    if dk.dk_flags.get() & (DKF_OPENED | DKF_NOLABELREAD) == 0 {
        // Read disklabel.
        let mut dl = Disklabel::zeroed();
        if disk_readlabel(&mut dl, dk.dk_devno.get()).is_ok() {
            enqueue_randomness(u32::from(dl.d_checksum));
        }
    }

    dk.dk_flags.set(dk.dk_flags.get() | DKF_OPENED);
    if let Some(dv) = dk.dk_device.get() {
        // SAFETY: the reference `disk_attach` took for this task.
        unsafe { device_unref(dv) };
    }
    wakeup(ptr::from_ref(dk));
}

/// Detach a disk.
pub fn disk_detach(diskp: &Disk) {
    // KERNEL_ASSERT_LOCKED(): nothing without MULTIPROCESSOR.

    // softraid_disk_attach: NSOFTRAID not configured.

    // Free the space used by the disklabel structures.
    if let Some(label) = diskp.dk_label.take() {
        free(label.cast(), M_DEVBUF, DISKLABEL_SIZE);
    }

    // Remove from the disklist.
    // SAFETY: `disk_attach` linked the disk on `disklist`.
    unsafe { DISKLIST.0.remove(diskp) };
    DISK_CHANGE.store(1, Ordering::Relaxed);
    if DISK_COUNT.fetch_sub(1, Ordering::Relaxed) - 1 < 0 {
        panic(format_args!("disk_detach: disk_count < 0"));
    }
}

/// `disk_openpart`: records an open of a partition, after checking that it exists (unless
/// it is the raw partition).
pub fn disk_openpart(dk: &Disk, part: u32, fmt: i32, haslabel: bool) -> Result<(), Errno> {
    // Unless opening the raw partition, check that the partition exists.
    if part != RAW_PART {
        let exists = haslabel
            && dk.label().is_some_and(|l| {
                part < u32::from(l.d_npartitions)
                    && l.d_partitions[part as usize].p_fstype != FS_UNUSED
            });
        if !exists {
            return Err(Errno::ENXIO);
        }
    }

    // Ensure the partition doesn't get changed under our feet.
    match fmt as u32 {
        S_IFCHR => dk.dk_copenmask.set(dk.dk_copenmask.get() | (1u64 << part)),
        S_IFBLK => dk.dk_bopenmask.set(dk.dk_bopenmask.get() | (1u64 << part)),
        _ => {}
    }
    dk.dk_openmask
        .set(dk.dk_copenmask.get() | dk.dk_bopenmask.get());

    Ok(())
}

/// `disk_closepart`: records a close of a partition.
pub fn disk_closepart(dk: &Disk, part: u32, fmt: i32) {
    match fmt as u32 {
        S_IFCHR => dk.dk_copenmask.set(dk.dk_copenmask.get() & !(1u64 << part)),
        S_IFBLK => dk.dk_bopenmask.set(dk.dk_bopenmask.get() & !(1u64 << part)),
        _ => {}
    }
    dk.dk_openmask
        .set(dk.dk_copenmask.get() | dk.dk_bopenmask.get());
}

/// `disk_gone`: revokes the vnodes of every partition of a disk that is going away.
pub fn disk_gone(open: DevTypeOpen, unit: u32) {
    // Locate the lowest minor number to be detached.
    let mn = diskminor(unit, 0);
    let mx = mn + MAXPARTITIONS as u32 - 1;

    for bmaj in 0..nblkdev() {
        if ptr::fn_addr_eq(bdevsw(bmaj).d_open, open) {
            vdevgone(bmaj, mn, mx, VBLK);
        }
    }
    for cmaj in 0..nchrdev() {
        if ptr::fn_addr_eq(cdevsw(cmaj).d_open, open) {
            vdevgone(cmaj, mn, mx, VCHR);
        }
    }
}

/// Increment a disk's busy counter. If the counter is going from 0 to 1, set the
/// timestamp.
pub fn disk_busy(diskp: &Disk) {
    // XXX We'd like to use something as accurate as microtime(), but that doesn't depend
    // on the system TOD clock.
    mtx_enter(&diskp.dk_mtx);
    let busy = diskp.dk_busy.get();
    diskp.dk_busy.set(busy + 1);
    if busy == 0 {
        diskp.dk_timestamp.set(microuptime());
    }
    mtx_leave(&diskp.dk_mtx);
}

/// Decrement a disk's busy counter, increment the byte count, total busy time, and reset
/// the timestamp.
pub fn disk_unbusy(diskp: &Disk, bcount: i64, blkno: Daddr, read: bool) {
    mtx_enter(&diskp.dk_mtx);

    let busy = diskp.dk_busy.get();
    diskp.dk_busy.set(busy - 1);
    if busy == 0 {
        let _ = printf(format_args!("disk_unbusy: {}: dk_busy < 0\n", diskp.name()));
    }

    let dv_time = microuptime();

    let diff_time = timersub(&dv_time, &diskp.dk_timestamp.get());
    diskp
        .dk_time
        .set(timeradd(&diskp.dk_time.get(), &diff_time));

    diskp.dk_timestamp.set(dv_time);
    if bcount > 0 {
        if read {
            diskp.dk_rbytes.set(diskp.dk_rbytes.get() + bcount as u64);
            diskp.dk_rxfer.set(diskp.dk_rxfer.get() + 1);
        } else {
            diskp.dk_wbytes.set(diskp.dk_wbytes.get() + bcount as u64);
            diskp.dk_wxfer.set(diskp.dk_wxfer.get() + 1);
        }
    } else {
        diskp.dk_seek.set(diskp.dk_seek.get() + 1);
    }

    mtx_leave(&diskp.dk_mtx);

    enqueue_randomness(
        (bcount ^ diff_time.tv_usec as i64 ^ (blkno >> 32) ^ (blkno & 0xffff_ffff)) as u32,
    );
}

/// `disk_lock`: takes the disk lock, interruptibly.
pub fn disk_lock(dk: &Disk) -> Result<(), Errno> {
    rw_enter(&dk.dk_lock, RW_WRITE | RW_INTR)
}

/// `disk_lock_nointr`: takes the disk lock.
pub fn disk_lock_nointr(dk: &Disk) {
    rw_enter_write(&dk.dk_lock);
}

/// `disk_unlock`: releases the disk lock.
pub fn disk_unlock(dk: &Disk) {
    rw_exit_write(&dk.dk_lock);
}

/// `dk_mountroot`: the `mountroot` of a disk root: reads `rootdev`'s label and mounts the
/// root partition with the mountroot of its file system type.
pub fn dk_mountroot() -> Result<(), Errno> {
    let rootdev = ROOTDEV.load(Ordering::Relaxed);
    let part = diskpart(rootdev) as usize;

    let mut dl = Disklabel::zeroed();
    if let Err(error) = disk_readlabel(&mut dl, rootdev) {
        panic(format_args!("{error}"));
    }

    if dl_getpsize(&dl.d_partitions[part]) == 0 {
        panic(format_args!("root filesystem has size 0"));
    }
    // EXT2FS, CD9660: not configured.
    let fstype = dl.d_partitions[part].p_fstype;
    #[cfg(feature = "ffs")]
    {
        if fstype != FS_BSDFFS {
            let _ = printf(format_args!(
                "filesystem type {fstype} not known.. assuming ffs\n"
            ));
        }
        crate::ufs::ffs::ffs_vfsops::ffs_mountroot()
    }
    #[cfg(not(feature = "ffs"))]
    panic(format_args!(
        "disk {rootdev:#x} filesystem type {fstype} not known"
    ));
}

/// `getdisk`: `parsedisk`, listing the choices when `str` names no disk.
pub fn getdisk(str: &[u8], defpart: u32) -> Option<(&'static Device, Dev)> {
    let found = parsedisk(str, defpart);
    if found.is_none() {
        let _ = printf(format_args!("use one of: exit"));
        for dv in ALLDEVS.0.iter() {
            if dv.dv_class.get() == DV_DISK {
                let _ = printf(format_args!(" {}[a-p]", dv.xname()));
            }
            // NFSCLIENT: not configured (DV_IFNET choices).
        }
        let _ = printf(format_args!("\n"));
    }
    found
}

/// `parsedisk`: the disk device `str` names (`wd0`, `rd0a`, ...) and its block device for
/// the partition the name ends in (`defpart` without one).
pub fn parsedisk(str: &[u8], defpart: u32) -> Option<(&'static Device, Dev)> {
    let mut len = str.len();
    if len == 0 {
        return None;
    }
    let part = match dl_partname2num(str[len - 1]) {
        Some(n) if n < MAXPARTITIONS => {
            len -= 1;
            n as u32
        }
        _ => defpart,
    };

    for dv in ALLDEVS.0.iter() {
        if dv.dv_class.get() == DV_DISK && dv.xname().as_bytes() == &str[..len] {
            let majdev = findblkmajor(dv);
            if majdev < 0 {
                return None;
            }
            return Some((
                dv,
                makediskdev(majdev as u32, dv.dv_unit.get() as u32, part),
            ));
        }
        // NFSCLIENT: not configured (an interface name gives NODEV).
    }
    None
}

/// `setroot`: chooses the root, swap and dump devices (`rootdev`, `swdevt[0]`, `dumpdev`)
/// from the configuration, the boot device and the boot disk's DUID, and sets `mountroot`.
pub fn setroot(bootdv: Option<&'static Device>, part: u32, exitflags: i32) {
    let _ = exitflags; // reboot(exitflags) answers "exit" at the prompt (RB_ASKNAME, below).
    let mut bootdv = bootdv;
    let mut part = part;

    // Ensure that all disk attach callbacks have completed.
    let mut slept = 0;
    loop {
        let pending = DISKLIST
            .0
            .iter()
            .find(|dk| dk.dk_devno.get() != NODEV && dk.dk_flags.get() & DKF_OPENED == 0);
        let Some(dk) = pending else { break };
        let _ = tsleep_nsec(ptr::from_ref(dk), 0, "dkopen", sec_to_nsec(1));
        slept += 1;
        if slept >= 5 {
            break;
        }
    }

    if slept == 5 {
        let _ = printf(format_args!("disklabels not read:"));
        for dk in DISKLIST.0.iter() {
            if dk.dk_devno.get() != NODEV && dk.dk_flags.get() & DKF_OPENED == 0 {
                let _ = printf(format_args!(" {}", Str(&dk.dk_name.get())));
            }
        }
        let _ = printf(format_args!("\n"));
    }

    // SAFETY: bootduid and rootduid are written only here, by the one thread running main.
    let bootduid = unsafe { BOOTDUID.get_mut() };
    if duid_iszero(bootduid) {
        // Locate DUID for boot disk since it was not provided.
        let dk = DISKLIST.0.iter().find(|dk| {
            dk.dk_device.get().map(NonNull::as_ptr) == bootdv.map(|d| ptr::from_ref(d).cast_mut())
        });
        if let Some(lp) = dk.and_then(|dk| dk.dk_label.get()) {
            // SAFETY: an attached disk's label lives as long as the disk.
            *bootduid = unsafe { lp.as_ref() }.d_uid;
        }
    } else if bootdv.is_none() {
        // Locate boot disk based on the provided DUID.
        let dk = DISKLIST.0.iter().find(|dk| {
            dk.dk_label.get().is_some_and(|lp| {
                // SAFETY: as above.
                duid_equal(&unsafe { lp.as_ref() }.d_uid, bootduid)
            })
        });
        if let Some(dv) = dk.and_then(|dk| dk.dk_device.get()) {
            // SAFETY: attached devices are never freed while their disk is on the list.
            bootdv = Some(unsafe { &*dv.as_ptr() });
        }
    }
    // SAFETY: as for bootduid.
    let rootduid = unsafe { ROOTDUID.get_mut() };
    *rootduid = *bootduid;

    // NSOFTRAID: not configured (sr_map_root).

    // If `swap generic' and we couldn't determine boot device, ask the user.
    let mut dk_found: Option<&Disk> = None;
    // SAFETY: mountroot is written only by the boot path and here, by the thread running main.
    let generic = unsafe { MOUNTROOT.read() }.is_none();
    if generic && bootdv.is_none() {
        BOOTHOWTO.fetch_or(RB_ASKNAME, Ordering::Relaxed);
    }
    let rootdv: &'static Device;
    if BOOTHOWTO.load(Ordering::Relaxed) & RB_ASKNAME != 0 {
        // The "root device" and "swap device" prompts (getsn under cnpollc).
        let _ = unported!("setroot: RB_ASKNAME prompts (getsn)");
        if generic && bootdv.is_none() {
            let _ = printf(format_args!("root device: none configured\n"));
            return;
        }
    }
    let rootdev = ROOTDEV.load(Ordering::Relaxed);
    if generic && rootdev == NODEV {
        // `swap generic'
        let Some(mut dv) = bootdv else { return };

        if dv.dv_class.get() == DV_DISK && !duid_iszero(rootduid) {
            let dk = DISKLIST.0.iter().find(|dk| {
                dk.dk_label.get().is_some_and(|lp| {
                    // SAFETY: as above.
                    duid_equal(&unsafe { lp.as_ref() }.d_uid, rootduid)
                })
            });
            let Some(dk) = dk else {
                panic(format_args!(
                    "root device ({}) not found",
                    Str(&duid_format(rootduid))
                ));
            };
            dk_found = Some(dk);
            if let Some(d) = dk.dk_device.get() {
                // SAFETY: as above.
                dv = unsafe { &*d.as_ptr() };
            }
        }
        rootdv = dv;

        let majdev = findblkmajor(rootdv);
        let nswapdev = if majdev >= 0 {
            // Root and swap are on the disk. Assume swap is on partition b.
            let unit = rootdv.dv_unit.get() as u32;
            ROOTDEV.store(makediskdev(majdev as u32, unit, part), Ordering::Relaxed);
            makediskdev(majdev as u32, unit, 1)
        } else {
            // Root and swap are on a net.
            NODEV
        };
        DUMPDEV.store(nswapdev, Ordering::Relaxed);
        SWDEVT[0].store(nswapdev, Ordering::Relaxed);
    } else {
        // Completely pre-configured, but we want rootdv ..
        let majdev = major(rootdev) as i32;
        let Some(name) = findblkname(majdev) else {
            return;
        };
        let unit = diskunit(rootdev);
        part = diskpart(rootdev);
        let mut buf = [0u8; 128];
        let len = bfmt(
            &mut buf,
            format_args!(
                "{}{}{}",
                Str(name),
                unit,
                char::from(dl_partnum2name(part as usize).unwrap_or(b'?'))
            ),
        );
        let Some((dv, _)) = parsedisk(&buf[..len], 0) else {
            panic(format_args!("root device ({}) not found", Str(&buf[..len])));
        };
        rootdv = dv;
    }
    ROOTDV.store(ptr::from_ref(rootdv).cast_mut(), Ordering::Relaxed);

    if let Some(bdv) = bootdv
        && bdv.dv_class.get() == DV_IFNET
        && let Some(ifp) = if_unit(bdv.xname().as_bytes())
    {
        let _ = if_addgroup(ifp, b"netboot");
        if_put(ifp);
    }

    if rootdv.dv_class.get() == DV_DISK {
        // SAFETY: as for MOUNTROOT above.
        unsafe { MOUNTROOT.write(Some(dk_mountroot)) };
        part = diskpart(ROOTDEV.load(Ordering::Relaxed));
    } else {
        // NFSCLIENT: not configured (DV_IFNET: nfs_mountroot).
        let _ = printf(format_args!(
            "can't figure root, hope your kernel is right\n"
        ));
        return;
    }

    let partname = char::from(dl_partnum2name(part as usize).unwrap_or(b'?'));
    let _ = printf(format_args!("root on {}{partname}", rootdv.xname()));

    if let Some(dk) = dk_found
        && dk.dk_device.get().map(NonNull::as_ptr) == Some(ptr::from_ref(rootdv).cast_mut())
    {
        let _ = printf(format_args!(
            " ({}.{partname})",
            Str(&duid_format(rootduid))
        ));
    }

    // Make the swap partition on the root drive the primary swap.
    let rootdev = ROOTDEV.load(Ordering::Relaxed);
    let mut temp = NODEV;
    let mut found = None;
    for (i, sw) in SWDEVT.iter().enumerate() {
        let d = sw.load(Ordering::Relaxed);
        if d == NODEV {
            break;
        }
        if major(rootdev) == major(d) && diskunit(rootdev) == diskunit(d) {
            temp = SWDEVT[0].load(Ordering::Relaxed);
            SWDEVT[0].store(d, Ordering::Relaxed);
            sw.store(temp, Ordering::Relaxed);
            found = Some(i);
            break;
        }
    }
    if found.is_some() {
        // If dumpdev was the same as the old primary swap device, move it to the new
        // primary swap device.
        if temp == DUMPDEV.load(Ordering::Relaxed) {
            DUMPDEV.store(SWDEVT[0].load(Ordering::Relaxed), Ordering::Relaxed);
        }
    }
    let print_dev = |what: &str, d: Dev| {
        let name = findblkname(major(d) as i32).unwrap_or(b"??");
        let p = char::from(dl_partnum2name(diskpart(d) as usize).unwrap_or(b'?'));
        let _ = printf(format_args!(" {what} on {}{}{p}", Str(name), diskunit(d)));
    };
    let sw0 = SWDEVT[0].load(Ordering::Relaxed);
    if sw0 != NODEV {
        print_dev("swap", sw0);
    }
    let dumpdev = DUMPDEV.load(Ordering::Relaxed);
    if dumpdev != NODEV {
        print_dev("dump", dumpdev);
    }
    let _ = printf(format_args!("\n"));
}

/// Formats into `buf` (the C's `snprintf`), truncating; returns the length written.
fn bfmt(buf: &mut [u8], args: fmt::Arguments<'_>) -> usize {
    struct W<'a>(&'a mut [u8], usize);
    impl fmt::Write for W<'_> {
        fn write_str(&mut self, s: &str) -> fmt::Result {
            let n = s.len().min(self.0.len() - self.1);
            self.0[self.1..self.1 + n].copy_from_slice(&s.as_bytes()[..n]);
            self.1 += n;
            Ok(())
        }
    }
    let mut w = W(buf, 0);
    let _ = fmt::write(&mut w, args);
    w.1
}

/// `findblkmajor`: the block major of a disk device, from its name (`nam2blk[]`), or -1.
pub fn findblkmajor(dv: &Device) -> i32 {
    let name = dv.xname().as_bytes();
    let len = name
        .iter()
        .position(u8::is_ascii_digit)
        .unwrap_or(name.len());
    let name = &name[..len];

    nam2blk()
        .iter()
        .find(|n| n.name == name)
        .map_or(-1, |n| n.maj)
}

/// `findblkname`: the driver name of a block major.
pub fn findblkname(maj: i32) -> Option<&'static [u8]> {
    nam2blk().iter().find(|n| n.maj == maj).map(|n| n.name)
}

/// `disk_readlabel`: reads the label of the disk `dev` is on through its raw character
/// partition (`DIOCGDINFO`).
pub fn disk_readlabel(dl: &mut Disklabel, dev: Dev) -> Result<(), DiskReadlabelError> {
    let chrdev = blktochr(dev);
    let rawdev = makediskdev(major(chrdev), diskunit(chrdev), RAW_PART);

    let vn = match cdevvp(rawdev) {
        Ok(Some(vn)) => vn,
        _ => return Err(DiskReadlabelError::NoVnode { dev, rawdev }),
    };

    let Some(p) = curproc() else {
        // Every caller runs in a thread (proc0, a kernel thread or a process).
        vput(vn);
        return Err(DiskReadlabelError::Open {
            dev,
            rawdev,
            error: Errno::ENXIO,
        });
    };

    let result = match VOP_OPEN(vn, FREAD, NOCRED, p) {
        Err(error) => Err(DiskReadlabelError::Open { dev, rawdev, error }),
        Ok(()) => VOP_IOCTL(vn, DIOCGDINFO, dl.as_bytes_mut(), FREAD, NOCRED, p)
            .map_err(|error| DiskReadlabelError::Ioctl { dev, rawdev, error }),
    };
    let _ = VOP_CLOSE(vn, FREAD, NOCRED, Some(p));
    vput(vn);
    result
}

/// Lookup a disk device and verify that it has completed attaching. The device comes back
/// referenced (`device_lookup`); the caller gives it back with `device_unref`.
pub fn disk_lookup(cd: &Cfdriver, unit: i32) -> Option<NonNull<Device>> {
    let dv = device_lookup(cd, unit)?;

    if DISKLIST.0.iter().any(|dk| dk.dk_device.get() == Some(dv)) {
        return Some(dv);
    }

    // SAFETY: the reference `device_lookup` just took.
    unsafe { device_unref(dv) };
    None
}

/// `duid_equal`.
pub fn duid_equal(duid1: &[u8; DUID_SIZE], duid2: &[u8; DUID_SIZE]) -> bool {
    duid1 == duid2
}

/// `duid_iszero`.
pub fn duid_iszero(duid: &[u8; DUID_SIZE]) -> bool {
    duid_equal(duid, &[0; DUID_SIZE])
}

/// `duid_format`: the DUID as 16 hex digits.
pub fn duid_format(duid: &[u8; DUID_SIZE]) -> [u8; 2 * DUID_SIZE] {
    // KERNEL_ASSERT_LOCKED(): nothing without MULTIPROCESSOR.
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = [0u8; 2 * DUID_SIZE];
    for (i, b) in duid.iter().enumerate() {
        s[2 * i] = HEX[usize::from(b >> 4)];
        s[2 * i + 1] = HEX[usize::from(b & 0xf)];
    }
    s
}

#[cfg(test)]
mod tests;
