/*	$OpenBSD: efidev.c,v 1.14 2026/09/04 17:48:11 mglocker Exp $	*/
/*	$OpenBSD: efidev.h,v 1.5 2026/09/04 17:48:11 mglocker Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 2015 YASUOKA Masahiko <yasuoka@yasuoka.net>
 * Copyright (c) 2016 Mark Kettenis
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
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR ``AS IS'' AND ANY EXPRESS OR
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED
 * WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE REGENTS OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 */

/*
 * Copyright (c) 1996 Michael Shalayeff
 * Copyright (c) 2015 YASUOKA Masahiko <yasuoka@yasuoka.net>
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
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR ``AS IS'' AND ANY EXPRESS OR
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED
 * WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE REGENTS OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 */
/* </LICENSES> */

//! efiboot's disks: EFI block I/O for libsa (`efiopen`, `efistrategy`), the search for the
//! OpenBSD partition (MBR, extended partitions, GPT) and its disklabel, an imaginary label
//! for ISO 9660 media, files on the EFI system partition (`esp0a:`), and the hibernation
//! signature on the swap partition.
//!
//! Upstream: sys/arch/arm64/stand/efiboot/efidev.c @ 3ce1f3f79392,
//! sys/arch/arm64/stand/efiboot/efidev.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - The bounce buffer of `efid_io` is the same `AllocatePages` buffer; the caller's buffer
//!   is a slice of whole sectors. `efistrategy` reads through a bounce buffer when the
//!   request is not a whole number of sectors (the C writes past the caller's size).
//! - The devices' open routines get the variadic `(unit, part)` of `devopen()` through
//!   libsa's `dv_open(f, &mut file)`: `devopen` (`efiboot.rs`) passes the device part of the
//!   name (`sd0a`), which [`devopen_args`] reads again.
//! - `BIOS_DEBUG` traces are not defined, as in the C; `debug` is read only by them.
//! - Messages the C returns as `const char *` are `Err(&'static str)`.
//! - A write to a read-only medium fails with `EFI_WRITE_PROTECTED` (the C's `goto on_eio`
//!   returns the `EFI_SUCCESS` of the page allocation).
//! - `esp_stat` keeps the C's return value: -1 when the file information needed a bigger
//!   buffer than `EFI_FILE_INFO` (`rv` is never set to 0 on that path), though `st_size` is
//!   filled in.

use alloc::boxed::Box;
use alloc::vec;
use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::Ordering;

use efi::include::efi::*;
use libsa::dev::set_errno;
use libsa::disklabel::getdisklabel;
use libsa::dkcksum::dkcksum;
use libsa::hdr::disklabel::{
    DISKMAGIC, DOS_LABELSECTOR, DOS_MAXEBR, DOSBBSECTOR, DOSPTYP_EFI, DOSPTYP_EXTEND,
    DOSPTYP_EXTENDL, DOSPTYP_OPENBSD, DOSPTYP_UNUSED, DTYPE_ATAPI, Disklabel, DosPartition,
    FS_SWAP, FS_UNUSED, GPT_UUID_OPENBSD, GPTMINHDRSIZE, GPTREVISION, GPTSECTOR, GPTSIGNATURE,
    GptHeader, GptPartition, MAXPARTITIONS, RAW_PART, dl_getpoffset, dl_getpsize, dl_sectoblk,
    dl_setdsize, dl_setpoffset, dl_setpsize,
};
use libsa::hdr::hibernate::HIBERNATE_MAGIC;
use libsa::hdr::iso::{ISO_STANDARD_ID, ISO_VD_PRIMARY};
use libsa::hdr::param::DEV_BSIZE;
use libsa::hdr::stat::Stat;
use libsa::hdr::types::{Daddr, Off};
use libsa::hdr::uuid::Uuid;
use libsa::saerrno::Errno;
use libsa::stand::{F_READ, F_WRITE, OpenFile, SEEK_CUR, SEEK_END, SEEK_SET};

use crate::disk::{DISKINFO_FLAG_GOODLABEL, DISKINFO_FLAG_HIBVALID, DiskInfo, EfiDiskinfo};
use crate::efiboot::{BOOTDEV_DIP, DISKLIST, IH, bs, devopen_args, handle_protocol};

/// The media of a disk.
fn media(ed: &EfiDiskinfo) -> &EfiBlockIoMedia {
    // SAFETY: the firmware's block I/O protocol and its media, valid until ExitBootServices.
    unsafe { &*(*ed.blkio).Media }
}

/// `EFI_BLKSPERSEC(ed)`: `DEV_BSIZE` blocks per sector.
fn efi_blkspersec(ed: &EfiDiskinfo) -> u32 {
    media(ed).BlockSize / DEV_BSIZE as u32
}

/// `EFI_SECTOBLK(ed, n)`.
fn efi_sectoblk(ed: &EfiDiskinfo, n: u64) -> u64 {
    n * u64::from(efi_blkspersec(ed))
}

/// `efid_init(dip, handle)`: a disk on the block I/O protocol `blkio`, with its label read.
pub fn efid_init(blkio: *mut EfiBlockIo) -> Box<DiskInfo> {
    // SAFETY: the firmware's block I/O protocol.
    let mediaid = unsafe { (*(*blkio).Media).MediaId };
    let mut dip = Box::new(DiskInfo {
        ed: EfiDiskinfo { blkio, mediaid },
        disklabel: Disklabel::zeroed(),
        sr_vol: ptr::null_mut(),
        part: 0,
        flags: 0,
        diskio: efid_diskio,
        strategy: efistrategy,
    });

    let mut label = Disklabel::zeroed();
    if efi_getdisklabel(&dip.ed, &mut label).is_ok() {
        dip.flags |= DISKINFO_FLAG_GOODLABEL;
    }
    dip.disklabel = label;
    dip
}

/// `efid_io(rw, ed, off, nsect, buf)`: transfer `nsect` `DEV_BSIZE` blocks at `off` through
/// a page-aligned buffer of whole device sectors.
fn efid_io(rw: i32, ed: &EfiDiskinfo, off: u32, nsect: i32, buf: &mut [u8]) -> EfiStatus {
    // block count of the intrinsic block size in DEV_BSIZE
    let blks = efi_blkspersec(ed);
    if blks == 0 {
        // block size < 512.  HP Stream 13 actually has such a disk.
        return EFI_UNSUPPORTED;
    }
    let nsect = nsect.max(0) as u32;
    let bsize = media(ed).BlockSize as usize;

    let start = off / blks;
    let end = (off + nsect).div_ceil(blks);
    let size = (end - start) as usize * bsize;
    let n = DEV_BSIZE * nsect as usize;
    let skip = DEV_BSIZE * (off - start * blks) as usize;

    let mut addr: EfiPhysicalAddress = 0;
    // SAFETY: a boot service with a valid pointer.
    let mut status = unsafe {
        (bs().AllocatePages)(
            AllocateAnyPages,
            EfiLoaderData,
            efi_size_to_pages(size),
            &mut addr,
        )
    };
    if !efi_error(status) {
        let data = addr as usize as *mut u8;
        // SAFETY: `data` is `size` bytes of pages just allocated; the protocol transfers
        // whole sectors into or out of them; at most `buf.len()` bytes of `buf` are copied.
        unsafe {
            let blkio = ed.blkio;
            let n = n.min(buf.len());
            if rw == F_READ {
                status =
                    ((*blkio).ReadBlocks)(blkio, ed.mediaid, u64::from(start), size, data.cast());
                if !efi_error(status) {
                    ptr::copy_nonoverlapping(data.add(skip), buf.as_mut_ptr(), n);
                }
            } else if rw == F_WRITE {
                if media(ed).ReadOnly != 0 {
                    status = EFI_WRITE_PROTECTED;
                } else {
                    if !off.is_multiple_of(blks) || !nsect.is_multiple_of(blks) {
                        status = ((*blkio).ReadBlocks)(
                            blkio,
                            ed.mediaid,
                            u64::from(start),
                            size,
                            data.cast(),
                        );
                    }
                    if !efi_error(status) {
                        ptr::copy_nonoverlapping(buf.as_ptr(), data.add(skip), n);
                        status = ((*blkio).WriteBlocks)(
                            blkio,
                            ed.mediaid,
                            u64::from(start),
                            size,
                            data.cast(),
                        );
                    }
                }
            }
            (bs().FreePages)(addr, efi_size_to_pages(size));
        }
    }

    status
}

/// `efid_diskio(rw, dip, off, nsect, buf)`: 0 or -1.
fn efid_diskio(rw: i32, dip: &DiskInfo, off: u32, nsect: i32, buf: &mut [u8]) -> i32 {
    if efi_error(efid_io(rw, &dip.ed, off, nsect, buf)) {
        -1
    } else {
        0
    }
}

/// `gpt_chk_mbr(dp, dsize)`: is this the GPT protective MBR (one partition, of type EFI,
/// covering the disk or as much of it as 32 bits can)? Taken from kern/subr_disk.c. NOTE:
/// MS always uses a size of UINT32_MAX for the EFI partition!
fn gpt_chk_mbr(dp: &[DosPartition], dsize: u64) -> bool {
    let mut found = 0;
    let mut efi = 0;
    for dp2 in dp {
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
        if u64::from(psize) <= dsize - GPTSECTOR || psize == u32::MAX {
            efi += 1;
        }
    }
    found == 1 && efi == 1
}

/// `findopenbsd(ed, &err)`: the sector of the first MBR OpenBSD partition, following
/// extended partitions. N.B.: must boot from a partition within the first 2^32-1 sectors!
/// Called only if the MBR on sector 0 is *not* a protective MBR and *does* have a valid
/// signature.
fn findopenbsd(ed: &EfiDiskinfo) -> Result<u32, Option<&'static str>> {
    let mut mbroff = DOSBBSECTOR;
    let mut mbr_eoff = DOSBBSECTOR; // Offset of MBR extended partition.
    let mut maxebr = DOS_MAXEBR;

    loop {
        if maxebr == 0 {
            return Err(Some("too many extended partitions"));
        }
        maxebr -= 1;

        // Read MBR
        let mut mbr = [0u8; DEV_BSIZE];
        if efi_error(efid_io(F_READ, ed, mbroff, 1, &mut mbr)) {
            return Err(Some("Disk I/O Error"));
        }

        // Search for OpenBSD partition
        let mut nextebr: u32 = 0;
        for dp in DosPartition::table(&mbr) {
            if dp.dp_size == 0 {
                continue;
            }
            let start = dp.dp_start;
            if dp.dp_typ == DOSPTYP_OPENBSD {
                if start > start.wrapping_add(mbroff) {
                    continue;
                }
                return Ok(start + mbroff);
            }

            // Record location of next ebr if and only if this is the first extended
            // partition in this boot record!
            if nextebr == 0 && (dp.dp_typ == DOSPTYP_EXTEND || dp.dp_typ == DOSPTYP_EXTENDL) {
                nextebr = start.wrapping_add(mbr_eoff);
                if nextebr < start {
                    nextebr = u32::MAX;
                }
                if mbr_eoff == DOSBBSECTOR {
                    mbr_eoff = start;
                }
            }
        }

        if nextebr != 0 && nextebr != u32::MAX {
            mbroff = nextebr;
            continue;
        }

        return Err(None);
    }
}

/// `findopenbsd_gpt(ed, &err)`: the sector of the first GPT OpenBSD partition. Called only
/// if the MBR on sector 0 *is* a protective MBR with a valid signature and sector 1 is a
/// valid GPT header.
fn findopenbsd_gpt(ed: &EfiDiskinfo) -> Result<u32, Option<&'static str>> {
    // Prepare OpenBSD UUID (XXX: should be replaced by uuid_dec_be())
    let c = GPT_UUID_OPENBSD;
    let openbsd_uuid = Uuid {
        time_low: u32::from_be_bytes([c[0], c[1], c[2], c[3]]),
        time_mid: u16::from_be_bytes([c[4], c[5]]),
        time_hi_and_version: u16::from_be_bytes([c[6], c[7]]),
        clock_seq_hi_and_reserved: c[8],
        clock_seq_low: c[9],
        node: [c[10], c[11], c[12], c[13], c[14], c[15]],
    };

    if efi_blkspersec(ed) > 8 {
        return Err(Some("disk sector > 4096 bytes\n"));
    }

    let mut buf = vec![0u8; 4096];
    let spb = efi_blkspersec(ed) as i32;

    // GPT Header
    let mut lba = GPTSECTOR;
    if efi_error(efid_io(
        F_READ,
        ed,
        efi_sectoblk(ed, lba) as u32,
        spb,
        &mut buf,
    )) {
        return Err(Some("Disk I/O Error"));
    }
    // SAFETY: `buf` holds more than a header; read unaligned; integers only.
    let mut gh = unsafe { buf.as_ptr().cast::<GptHeader>().read_unaligned() };

    // Check signature
    if u64::from_le(gh.gh_sig) != GPTSIGNATURE {
        return Err(Some("bad GPT signature\n"));
    }

    if u32::from_le(gh.gh_rev) != GPTREVISION {
        return Err(Some("bad GPT revision\n"));
    }

    let ghsize = u32::from_le(gh.gh_size);
    if ghsize < GPTMINHDRSIZE || ghsize as usize > core::mem::size_of::<GptHeader>() {
        return Err(Some("bad GPT header size\n"));
    }

    // Check checksum
    let orig_csum = gh.gh_csum;
    gh.gh_csum = 0;
    // SAFETY: the header's first `ghsize` bytes (checked above to be within it).
    let hdr = unsafe { core::slice::from_raw_parts((&raw const gh).cast::<u8>(), ghsize as usize) };
    let new_csum = libz::crc32(0, hdr);
    gh.gh_csum = orig_csum;
    if u32::from_le(orig_csum) != new_csum {
        return Err(Some("bad GPT header checksum\n"));
    }

    lba = u64::from_le(gh.gh_part_lba);
    let ghpartsize = u32::from_le(gh.gh_part_size) as usize;
    if ghpartsize == 0 {
        return Err(Some("bad GPT header size\n"));
    }
    let ghpartspersec = media(ed).BlockSize as usize / ghpartsize;
    let mut ghpartnum = u32::from_le(gh.gh_part_num) as usize;
    let gpsectors = ghpartnum.div_ceil(ghpartspersec.max(1));
    let mut new_csum = libz::crc32(0, &[]);
    let mut found = None;
    let gpsize = core::mem::size_of::<GptPartition>();
    for _ in 0..gpsectors {
        if efi_error(efid_io(
            F_READ,
            ed,
            efi_sectoblk(ed, lba) as u32,
            spb,
            &mut buf,
        )) {
            return Err(Some("Disk I/O Error"));
        }
        for part in 0..ghpartspersec {
            if ghpartnum == 0 {
                break;
            }
            let e = &buf[part * gpsize..(part + 1) * gpsize];
            new_csum = libz::crc32(new_csum, e);
            ghpartnum -= 1;
            if found.is_some() {
                continue;
            }
            // SAFETY: `e` is one partition entry's bytes; read unaligned; integers only.
            let gp = unsafe { e.as_ptr().cast::<GptPartition>().read_unaligned() };
            if gp.gp_type == openbsd_uuid {
                found = Some(gp);
            }
        }
        lba += 1;
    }
    if new_csum != u32::from_le(gh.gh_part_csum) {
        return Err(Some("bad GPT entries checksum\n"));
    }
    if let Some(gp) = found {
        let lba = u64::from_le(gp.gp_lba_start);
        // Bootloaders do not current handle addresses > UINT_MAX!
        if lba > u64::from(u32::MAX) || efi_sectoblk(ed, lba) > u64::from(u32::MAX) {
            return Err(Some("OpenBSD Partition LBA > 2**32 - 1"));
        }
        return Ok(lba as u32);
    }

    Err(None)
}

/// `efi_getdisklabel(ed, label)`: the disklabel of the OpenBSD partition, through the MBR
/// or the GPT; or an imaginary one for ISO 9660 media; or why there is none.
pub fn efi_getdisklabel(ed: &EfiDiskinfo, label: &mut Disklabel) -> Result<(), &'static str> {
    // Read sector 0. Ensure it has a valid MBR signature. If it's a protective MBR then try
    // to find the disklabel via GPT. If it's not a protective MBR, try to find the disklabel
    // via MBR.
    let mut buf = [0u8; DEV_BSIZE];
    if efi_error(efid_io(F_READ, ed, DOSBBSECTOR, 1, &mut buf)) {
        return Err("Disk I/O Error");
    }

    // Check MBR signature.
    if buf[510] != 0x55 || buf[511] != 0xaa {
        if efi_getdisklabel_cd9660(ed, label) {
            return Ok(());
        }
        return Err("invalid MBR signature");
    }

    let dosparts = DosPartition::table(&buf);

    // check for GPT protective MBR.
    let start = if gpt_chk_mbr(&dosparts, media(ed).LastBlock + 1) {
        findopenbsd_gpt(ed).map_err(|e| e.unwrap_or("no OpenBSD GPT partition"))?
    } else {
        findopenbsd(ed).map_err(|e| e.unwrap_or("no OpenBSD MBR partition\n"))?
    };

    // Load BSD disklabel
    if efi_error(efid_io(
        F_READ,
        ed,
        efi_sectoblk(ed, u64::from(start)) as u32 + DOS_LABELSECTOR,
        1,
        &mut buf,
    )) {
        return Err("failed to read disklabel");
    }

    // Fill in disklabel
    getdisklabel(&buf, label)
}

/// `efi_getdisklabel_cd9660(ed, label)`: an imaginary disk label for an ISO 9660 medium;
/// false if it is not one.
fn efi_getdisklabel_cd9660(ed: &EfiDiskinfo, label: &mut Disklabel) -> bool {
    let mut buf = [0u8; DEV_BSIZE];
    if efi_error(efid_io(F_READ, ed, 64, 1, &mut buf)) {
        return false;
    }
    if buf[0] != ISO_VD_PRIMARY || buf[1..6] != ISO_STANDARD_ID[..] {
        return false;
    }

    // Create an imaginary disk label
    label.d_secsize = 2048;
    label.d_ntracks = 1;
    label.d_nsectors = 100;
    label.d_ncylinders = 1;
    label.d_secpercyl = label.d_ntracks * label.d_nsectors;

    label.d_typename = [0; 16];
    label.d_typename[..12].copy_from_slice(b"ATAPI CD-ROM");
    label.d_type = DTYPE_ATAPI;

    label.d_packname = [0; 16];
    label.d_packname[..10].copy_from_slice(b"fictitious");
    dl_setdsize(label, 100);

    // 'a' partition covering the "whole" disk
    dl_setpoffset(&mut label.d_partitions[0], 0);
    dl_setpsize(&mut label.d_partitions[0], 100);
    label.d_partitions[0].p_fstype = FS_UNUSED;

    // The raw partition is special
    dl_setpoffset(&mut label.d_partitions[RAW_PART], 0);
    dl_setpsize(&mut label.d_partitions[RAW_PART], 100);
    label.d_partitions[RAW_PART].p_fstype = FS_UNUSED;

    label.d_npartitions = MAXPARTITIONS as u16;

    label.d_magic = DISKMAGIC;
    label.d_magic2 = DISKMAGIC;
    label.d_checksum = dkcksum(label);

    true
}

/// `efiopen(f, unit, part)`: disk `unit` of the disk list (`sd<unit>`), partition `part`.
pub fn efiopen(f: &mut OpenFile, file: &mut &[u8]) -> Result<(), Errno> {
    let (unit, part) = devopen_args(file);

    if part as usize >= MAXPARTITIONS {
        return Err(Errno::ENXIO);
    }

    // SAFETY: single-threaded; the list is not changed while efiboot runs boot(8).
    let list = unsafe { DISKLIST.get_mut() };
    let Some(dip) = list.get_mut(unit as usize) else {
        return Err(Errno::ENXIO);
    };

    if (dip.flags & DISKINFO_FLAG_GOODLABEL) == 0 {
        return Err(Errno::ENXIO);
    }

    dip.part = part;
    let dip: *mut DiskInfo = &mut **dip;
    BOOTDEV_DIP.store(dip, Ordering::Relaxed);
    f.f_devdata = dip.cast();

    Ok(())
}

/// `efistrategy(devdata, rw, blk, size, buf, rsize)`: transfer at block `blk` of the
/// partition the open named.
pub fn efistrategy(
    devdata: *mut c_void,
    rw: i32,
    blk: Daddr,
    buf: &mut [u8],
    rsize: Option<&mut usize>,
) -> Result<(), Errno> {
    // SAFETY: libsa passes the `f_devdata` efiopen set, a disk of the disk list.
    let dip = unsafe { &*devdata.cast::<DiskInfo>() };
    let nsect = buf.len().div_ceil(DEV_BSIZE);
    let part = &dip.disklabel.d_partitions[dip.part as usize];
    let blk = blk + dl_sectoblk(&dip.disklabel, u64::from(part.p_offset)) as Daddr;

    let error = if blk < 0 {
        Errno::EINVAL.0
    } else if buf.len() == nsect * DEV_BSIZE {
        efid_diskio(rw, dip, blk as u32, nsect as i32, buf)
    } else {
        let mut bounce = vec![0u8; nsect * DEV_BSIZE];
        if rw == F_WRITE {
            bounce[..buf.len()].copy_from_slice(buf);
        }
        let e = efid_diskio(rw, dip, blk as u32, nsect as i32, &mut bounce);
        if rw == F_READ {
            let n = buf.len();
            buf.copy_from_slice(&bounce[..n]);
        }
        e
    };

    if let Some(r) = rsize {
        *r = nsect * DEV_BSIZE;
    }

    // the C returns efid_diskio's -1 (or EINVAL) as the error number
    if error == 0 {
        Ok(())
    } else {
        Err(Errno(error))
    }
}

/// `eficlose(f)`.
pub fn eficlose(f: &mut OpenFile) -> Result<(), Errno> {
    f.f_devdata = ptr::null_mut();
    Ok(())
}

/// `efiioctl(f, cmd, data)`.
pub fn efiioctl(_f: &mut OpenFile, _cmd: u64, _data: *mut c_void) -> Result<(), Errno> {
    Ok(())
}

/// A file opened on the EFI system partition: its EFI file handle.
struct EspFile(*mut EfiFile);

// SAFETY: efiboot is single-threaded; the handle is the firmware's.
unsafe impl Send for EspFile {}

/// The EFI file handle of an `esp0a:` open file.
fn esp_handle(f: &mut OpenFile) -> Result<*mut EfiFile, Errno> {
    f.fsdata::<EspFile>().map(|h| h.0).ok_or(Errno::EBADF)
}

/// `esp_open(path, f)`: load a file from the EFI System Partition.
pub fn esp_open(path: &[u8], f: &mut OpenFile) -> Result<(), Errno> {
    if f.f_dev.is_none_or(|d| d.dv_name != "esp") {
        return Err(Errno::ENXIO);
    }

    let ih = IH.load(Ordering::Relaxed);
    if ih.is_null() {
        return Err(Errno::ENXIO);
    }

    // get the loaded image protocol interface
    let li =
        handle_protocol::<EfiLoadedImage>(ih, LOADED_IMAGE_PROTOCOL).map_err(|_| Errno::ENXIO)?;

    // get a fs handle
    // SAFETY: the firmware's loaded image protocol.
    let dev = unsafe { (*li).DeviceHandle };
    let vol = handle_protocol::<EfiFileIoInterface>(dev, SIMPLE_FILE_SYSTEM_PROTOCOL)
        .map_err(|_| Errno::ENXIO)?;

    let mut vh: *mut EfiFile = ptr::null_mut();
    // SAFETY: the firmware's simple file system protocol.
    if unsafe { ((*vol).OpenVolume)(vol, &mut vh) } != EFI_SUCCESS {
        return Err(Errno::ENOENT);
    }

    // No AsciiStrToUnicodeStrS
    let path = &path[..path.iter().position(|&c| c == 0).unwrap_or(path.len())];
    let mut fname: alloc::vec::Vec<u16> = path.iter().map(|&c| u16::from(c)).collect();
    fname.push(0);

    let mut fh: *mut EfiFile = ptr::null_mut();
    // SAFETY: the volume's file protocol and a NUL-terminated name.
    let status = unsafe {
        ((*vh).Open)(
            vh,
            &mut fh,
            fname.as_mut_ptr(),
            EFI_FILE_MODE_READ,
            EFI_FILE_READ_ONLY /*| EFI_FILE_HIDDEN*/ | EFI_FILE_SYSTEM,
        )
    };
    if status != EFI_SUCCESS {
        return Err(Errno::ENOENT);
    }

    f.f_fsdata = Some(Box::new(EspFile(fh)));
    Ok(())
}

/// `esp_close(f)`.
pub fn esp_close(f: &mut OpenFile) -> Result<(), Errno> {
    let fh = esp_handle(f)?;
    // SAFETY: the file handle esp_open got.
    unsafe { ((*fh).Close)(fh) };
    f.f_fsdata = None;
    Ok(())
}

/// `esp_read(f, addr, size, &resid)`.
pub fn esp_read(f: &mut OpenFile, addr: &mut [u8], resid: &mut usize) -> Result<(), Errno> {
    let fh = esp_handle(f)?;
    let mut readlen: UINTN = addr.len();
    // SAFETY: the file handle and a buffer of `readlen` bytes.
    let status = unsafe { ((*fh).Read)(fh, &mut readlen, addr.as_mut_ptr().cast()) };
    if status != EFI_SUCCESS {
        return Err(Errno::EIO);
    }

    *resid = addr.len() - readlen;
    Ok(())
}

/// `esp_write`.
pub fn esp_write(_f: &mut OpenFile, _start: &[u8], _resid: &mut usize) -> Result<(), Errno> {
    Err(Errno::EROFS)
}

/// `esp_seek(f, offset, where)`: returns 0 on success, as the C.
pub fn esp_seek(f: &mut OpenFile, offset: Off, whence: i32) -> Result<Off, Errno> {
    let fh = esp_handle(f)?;
    let mut position: u64 = 0;

    // SAFETY: the file handle esp_open got.
    unsafe {
        match whence {
            SEEK_CUR => {
                if ((*fh).GetPosition)(fh, &mut position) != EFI_SUCCESS {
                    set_errno(Errno::EIO);
                    return Err(Errno::EIO);
                }
                position = position.wrapping_add(offset as u64);
            }
            SEEK_SET => position = offset as u64,
            SEEK_END => position = 0xFFFF_FFFF_FFFF_FFFF,
            _ => {
                set_errno(Errno::EINVAL);
                return Err(Errno::EINVAL);
            }
        }

        if ((*fh).SetPosition)(fh, position) != EFI_SUCCESS {
            set_errno(Errno::EIO);
            return Err(Errno::EIO);
        }
    }

    Ok(0)
}

/// `esp_stat(f, sb)`.
pub fn esp_stat(f: &mut OpenFile, sb: &mut Stat) -> Result<(), Errno> {
    let fh = esp_handle(f)?;

    sb.st_mode = 0o444;
    sb.st_nlink = 1;
    sb.st_uid = 0;
    sb.st_gid = 0;

    // `EFI_FILE_INFO fi` on the stack: its size, 8-byte aligned.
    let mut fi = [0u64; core::mem::size_of::<EfiFileInfo>().div_ceil(8)];
    let mut guid = EFI_FILE_INFO_ID;
    let mut filen: UINTN = core::mem::size_of::<EfiFileInfo>();
    // SAFETY: the file handle and a buffer of `filen` bytes.
    let status = unsafe { ((*fh).GetInfo)(fh, &mut guid, &mut filen, fi.as_mut_ptr().cast()) };
    match status {
        EFI_SUCCESS => {
            // SAFETY: the firmware filled in an EFI_FILE_INFO (8-byte aligned buffer).
            sb.st_size = unsafe { (*fi.as_ptr().cast::<EfiFileInfo>()).FileSize } as Off;
            return Ok(());
        }
        EFI_BUFFER_TOO_SMALL => {}
        _ => return Err(Errno::EIO),
    }

    let mut fip = vec![0u64; filen.div_ceil(8)];
    // SAFETY: the file handle and a buffer of at least `filen` bytes.
    let status = unsafe { ((*fh).GetInfo)(fh, &mut guid, &mut filen, fip.as_mut_ptr().cast()) };
    if status == EFI_SUCCESS {
        // SAFETY: the buffer holds an EFI_FILE_INFO (8-byte aligned).
        sb.st_size = unsafe { (*fip.as_ptr().cast::<EfiFileInfo>()).FileSize } as Off;
    }

    // the C's `rv`, still -1 here
    Err(Errno(-1))
}

/// `esp_readdir`.
pub fn esp_readdir(_f: &mut OpenFile, _name: Option<&mut [u8]>) -> Result<(), Errno> {
    Err(Errno::EOPNOTSUPP)
}

/// `espopen(f, unit, part)`: `esp0`, the EFI system partition we were loaded from.
pub fn espopen(_f: &mut OpenFile, file: &mut &[u8]) -> Result<(), Errno> {
    let (unit, _) = devopen_args(file);

    if unit != 0 {
        return Err(Errno(1));
    }

    Ok(())
}

/// `espclose(f)`.
pub fn espclose(_f: &mut OpenFile) -> Result<(), Errno> {
    Ok(())
}

/// `espioctl(f, cmd, data)`.
pub fn espioctl(_f: &mut OpenFile, _cmd: u64, _data: *mut c_void) -> Result<(), Errno> {
    Err(Errno::EOPNOTSUPP)
}

/// `espstrategy`.
pub fn espstrategy(
    _devdata: *mut c_void,
    _rw: i32,
    _blk: Daddr,
    _buf: &mut [u8],
    _rsize: Option<&mut usize>,
) -> Result<(), Errno> {
    Err(Errno::EOPNOTSUPP)
}

/// `check_hibernate(dip)`: mark the disk whose swap ends with a hibernation signature.
pub fn check_hibernate(dip: &mut DiskInfo) {
    let mut buf = [0u8; DEV_BSIZE];

    // Check that the b partition is swap.
    let p = dip.disklabel.d_partitions[1];
    if p.p_fstype != FS_SWAP || dl_getpsize(&p) == 0 {
        return;
    }

    // Last sector of swap holds the hibernate signature.
    let sec = dl_getpoffset(&p) + dl_getpsize(&p) - 1;

    // Use diskio (raw, absolute LBA) instead of strategy. efistrategy implicitly adds
    // dip->disklabel.d_partitions[dip->part].p_offset to the incoming block number,
    // treating it as partition-relative -- on amd64 this works because dip->bsddev's
    // partition is RAW_PART (c, with p_offset == 0), but here dip->part defaults to 0
    // (= a), whose p_offset is non-zero.
    let error = (dip.diskio)(
        F_READ,
        dip,
        dl_sectoblk(&dip.disklabel, sec) as u32,
        1,
        &mut buf,
    );
    if error == 0 && u32::from_ne_bytes([buf[0], buf[1], buf[2], buf[3]]) == HIBERNATE_MAGIC {
        dip.flags |= DISKINFO_FLAG_HIBVALID;
    }
}

/// `bootdev_has_hibernate()`.
pub fn bootdev_has_hibernate() -> bool {
    let dip = BOOTDEV_DIP.load(Ordering::Relaxed);
    if dip.is_null() {
        return false;
    }

    // SAFETY: bootdev_dip points into the disk list (efiopen or efi_diskprobe set it).
    unsafe { ((*dip).flags & DISKINFO_FLAG_HIBVALID) != 0 }
}
