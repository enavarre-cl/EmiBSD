/* <LICENSES> */
/*
 * Copyright (c) 1996 Michael Shalayeff
 * Copyright (c) 2003 Tobias Weingartner
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

/* efidev.c */
/* </LICENSES> */

/* <CODE> */
//! efiboot's disks: EFI block I/O for libsa (`efiopen`, `efistrategy`), the search for the
//! OpenBSD partition (MBR, extended partitions, GPT) and its disklabel, an imaginary label
//! for ISO 9660 media, `machine diskinfo`, and files on the EFI system partition (`esp:`).
//!
//! Upstream: sys/arch/amd64/stand/efiboot/efidev.c @ 3ce1f3f79392,
//! sys/arch/amd64/stand/efiboot/efidev.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - The bounce buffer of `efid_io` is the same `AllocatePages` buffer; the caller's buffer is
//!   a slice of whole sectors. `efistrategy` reads through a bounce buffer when the request
//!   is not a whole number of sectors (the C writes past the caller's size).
//! - `SOFTRAID` (`sr*` devices, `sr_strategy`): with feature `softraid`, as the C; the
//!   feature is not ported yet, so `efiopen` of an `sr` device says so and fails (`ENXIO`).
//! - `EFI_DEBUG`/`BIOS_DEBUG` traces are not defined, as in the C.
//! - Messages the C returns as `const char *` are `Err(&'static str)`.
//! - A write to a read-only medium fails with `EFI_WRITE_PROTECTED` (the C's `goto on_eio`
//!   returns the `EFI_SUCCESS` of the page allocation).

use alloc::boxed::Box;
use alloc::vec;
use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::{AtomicI32, Ordering};

use efi::include::efi::*;
use libsa::dev::set_errno;
use libsa::disklabel::getdisklabel;
use libsa::dkcksum::dkcksum;
use libsa::hdr::disklabel::{
    DISKMAGIC, DOS_LABELSECTOR, DOS_MAXEBR, DOSBBSECTOR, DOSPARTOFF, DOSPTYP_EFI, DOSPTYP_EXTEND,
    DOSPTYP_EXTENDL, DOSPTYP_OPENBSD, DOSPTYP_UNUSED, DTYPE_ATAPI, Disklabel, DosPartition,
    FS_UNUSED, GPT_UUID_OPENBSD, GPTMINHDRSIZE, GPTREVISION, GPTSECTOR, GPTSIGNATURE, GptHeader,
    MAXPARTITIONS, RAW_PART, dl_partname2num, dl_sectoblk, dl_setdsize, dl_setpoffset, dl_setpsize,
};
use libsa::hdr::iso::{ISO_STANDARD_ID, ISO_VD_PRIMARY};
use libsa::hdr::param::DEV_BSIZE;
use libsa::hdr::reboot::{b_adaptor, b_controller, b_partition, b_type, b_unit, makebootdev};
use libsa::hdr::stat::Stat;
use libsa::hdr::types::{Daddr, Off};
use libsa::hdr::uuid::Uuid;
use libsa::printf;
use libsa::putchar::putchar;
use libsa::saerrno::Errno;
use libsa::stand::{F_RAW, F_READ, F_WRITE, OpenFile, SEEK_CUR, SEEK_END, SEEK_SET};

use crate::biosvar::BiosDiskinfo;
use crate::dev_i386::{BDEVS, NBDEVS};
use crate::disk::DiskInfo;
use crate::diskprobe::{BOOTDEV_DIP, DISKLIST, dklookup};
use crate::efiboot::{IH, bs, handle_protocol};

/// `struct efi_diskinfo`: the block I/O protocol of a disk and its media id.
pub struct EfiDiskinfo {
    /// `blkio`.
    pub blkio: *mut EfiBlockIo,
    /// `mediaid`.
    pub mediaid: u32,
}

/// `bios_bootdev`: the BIOS-style number of the device we booted from.
pub static BIOS_BOOTDEV: AtomicI32 = AtomicI32::new(0);

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

/// `efid_init(dip, handle)`: a disk on the block I/O protocol `blkio`.
pub fn efid_init(blkio: *mut EfiBlockIo) -> Box<DiskInfo> {
    // SAFETY: the firmware's block I/O protocol.
    let mediaid = unsafe { (*(*blkio).Media).MediaId };
    Box::new(DiskInfo {
        efi_info: Some(Box::new(EfiDiskinfo { blkio, mediaid })),
        bios_info: BiosDiskinfo::default(),
        disklabel: Disklabel::zeroed(),
        sr_vol: ptr::null_mut(),
        diskio: efid_diskio,
        strategy: efistrategy,
        bsddev: 0,
        bootdev: 0,
    })
}

/// `alloc(sizeof(struct diskinfo))` and `memset(.., 0, ..)`: a disk with no EFI device
/// (`efi_pxeprobe`'s fake one for the TFTP case).
pub fn efid_init_none() -> Box<DiskInfo> {
    Box::new(DiskInfo {
        efi_info: None,
        bios_info: BiosDiskinfo::default(),
        disklabel: Disklabel::zeroed(),
        sr_vol: ptr::null_mut(),
        diskio: efid_diskio,
        strategy: efistrategy,
        bsddev: 0,
        bootdev: 0,
    })
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
        // whole sectors into or out of them; `buf` holds `n` bytes (checked by the callers'
        // slices: at most `buf.len()` bytes are copied).
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
    let Some(ed) = dip.efi_info.as_deref() else {
        return -1;
    };
    if efi_error(efid_io(rw, ed, off, nsect, buf)) {
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

/// `findopenbsd_gpt(ed, &err)`: the sector of the first GPT OpenBSD partition.
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
    let gpsize = core::mem::size_of::<libsa::hdr::disklabel::GptPartition>();
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
            let gp = unsafe {
                e.as_ptr()
                    .cast::<libsa::hdr::disklabel::GptPartition>()
                    .read_unaligned()
            };
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
    let _ = DOSPARTOFF;

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

/// `efiopen(f, &file)`: open `hd0a:path`-style names: the BIOS-style disk and its partition.
pub fn efiopen(f: &mut OpenFile, file: &mut &[u8]) -> Result<(), Errno> {
    let cp = *file;
    let at = |i: usize| cp.get(i).copied().unwrap_or(0);

    f.f_devdata = ptr::null_mut();

    // Search for device specification.
    let devlen = if at(4) == b':' {
        2
    } else if at(5) == b':' {
        3
    } else {
        return Err(Errno::ENOENT);
    };
    let dev = &cp[..devlen];
    let mut p = devlen;

    // Get unit.
    let unit = if at(p).is_ascii_digit() {
        p += 1;
        u32::from(at(p - 1) - b'0')
    } else {
        printf!("Bad unit number\n");
        return Err(Errno::EUNIT);
    };

    // Get partition.
    let Some(part) = dl_partname2num(at(p)) else {
        printf!("Bad partition\n");
        return Err(Errno::EPART);
    };
    p += 1;

    // Get filename.
    p += 1; // skip ':'
    if at(p) != 0 {
        *file = &cp[p.min(cp.len())..];
    } else {
        f.f_flags |= F_RAW;
    }

    // Intercept softraid disks.
    if dev == b"sr" {
        printf!("efiopen: softraid volumes are not supported (feature softraid)\n");
        return Err(Errno::ENXIO);
    }

    let Some(maj) = BDEVS[..NBDEVS].iter().position(|d| {
        // strncmp(dev, bdevs[maj], devlen)
        let name = d.as_bytes();
        (0..devlen).all(|i| name.get(i).copied().unwrap_or(0) == dev[i])
    }) else {
        printf!("Unknown device: ");
        for &c in cp.iter().take_while(|&&c| c != b':') {
            putchar(i32::from(c));
        }
        putchar(i32::from(b'\n'));
        return Err(Errno::EADAPT);
    };

    let mut biosdev = unit as i32;
    match maj {
        0 | 4 | 17 => biosdev |= 0x80, // wd, sd, hd
        2 => {}                        // fd
        6 => biosdev |= 0xe0,          // cd
        _ => return Err(Errno::ENXIO),
    }

    // Find device
    let dip = dklookup(biosdev);
    if dip.is_null() {
        return Err(Errno::ENXIO);
    }
    BOOTDEV_DIP.store(dip, Ordering::Relaxed);

    // Fix up bootdev
    // SAFETY: `dip` points into DISKLIST (a box that lives as long as the program).
    let d = unsafe { &mut *dip };
    let bsd_dev = d.bios_info.bsd_dev as u32;
    d.bsddev = makebootdev(
        b_type(bsd_dev),
        b_adaptor(bsd_dev),
        b_controller(bsd_dev),
        unit,
        part as u32,
    ) as i32;
    d.bootdev = makebootdev(
        b_type(bsd_dev),
        b_adaptor(bsd_dev),
        b_controller(bsd_dev),
        b_unit(bsd_dev),
        part as u32,
    ) as i32;

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
    // SAFETY: libsa passes the `f_devdata` efiopen set, a disk of DISKLIST.
    let dip = unsafe { &*devdata.cast::<DiskInfo>() };
    let nsect = buf.len().div_ceil(DEV_BSIZE);
    let part = &dip.disklabel.d_partitions[b_partition(dip.bsddev as u32) as usize & 63];
    let blk = blk + dl_sectoblk(&dip.disklabel, u64::from(part.p_offset)) as Daddr;

    let error = if blk < 0 {
        Errno::EINVAL.0
    } else if buf.len() == nsect * DEV_BSIZE {
        (dip.diskio)(rw, dip, blk as u32, nsect as i32, buf)
    } else {
        let mut bounce = vec![0u8; nsect * DEV_BSIZE];
        if rw == F_WRITE {
            bounce[..buf.len()].copy_from_slice(buf);
        }
        let e = (dip.diskio)(rw, dip, blk as u32, nsect as i32, &mut bounce);
        if rw == F_READ {
            let n = buf.len();
            buf.copy_from_slice(&bounce[..n]);
        }
        e
    };

    if let Some(r) = rsize {
        *r = nsect * DEV_BSIZE;
    }

    // the C returns diskio's -1 (or EINVAL) as the error number
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

/// `efi_dump_diskinfo()`: `machine diskinfo`.
pub fn efi_dump_diskinfo() {
    printf!("Disk\tBlkSiz\tIoAlign\tSize\tFlags\tChecksum\n");
    // SAFETY: single-threaded; a shared look at the list.
    let list = unsafe { DISKLIST.get() };
    for dip in list.iter() {
        let bdi = dip.bios_info;
        let Some(ed) = dip.efi_info.as_deref() else {
            continue;
        };
        let m = media(ed);

        let mut siz = (m.LastBlock + 1) * u64::from(m.BlockSize);
        siz /= 1024 * 1024;
        let sizu = if siz < 10000 {
            "MB"
        } else {
            siz /= 1024;
            "GB"
        };

        let (bsd_dev, number, flags, checksum) =
            (bdi.bsd_dev, bdi.bios_number, bdi.flags, bdi.checksum);
        printf!(
            "{}d{}\t{}\t{}\t{}{}\t{:#x}\t{:#x}\t{}\n",
            if b_type(bsd_dev as u32) == 6 {
                'c'
            } else {
                'h'
            },
            number & 0x1f,
            m.BlockSize,
            m.IoAlign,
            siz as u32,
            sizu,
            flags,
            checksum,
            if m.RemovableMedia != 0 {
                "Removable"
            } else {
                ""
            }
        );
    }
}

/// A file opened on the EFI system partition: its EFI file handle.
struct EspFile(*mut EfiFile);

// SAFETY: efiboot is single-threaded; the handle is the firmware's.
unsafe impl Send for EspFile {}

/// The EFI file handle of an `esp:` open file.
fn esp_handle(f: &mut OpenFile) -> Result<*mut EfiFile, Errno> {
    f.fsdata::<EspFile>().map(|h| h.0).ok_or(Errno::EBADF)
}

/// `esp_open(path, f)`: load a file from the EFI System Partition.
pub fn esp_open(path: &[u8], f: &mut OpenFile) -> Result<(), Errno> {
    if f.f_dev.is_none_or(|d| d.dv_name != "ESP") {
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
            EFI_FILE_READ_ONLY | EFI_FILE_SYSTEM,
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

    let mut guid = EFI_FILE_INFO_ID;
    let mut filen: UINTN = 0;
    // SAFETY: the file handle; a null buffer asks for the size.
    let status = unsafe { ((*fh).GetInfo)(fh, &mut guid, &mut filen, ptr::null_mut()) };
    match status {
        EFI_SUCCESS | EFI_BUFFER_TOO_SMALL => {}
        _ => return Err(Errno::EIO),
    }

    let mut fi = vec![0u64; filen.div_ceil(8)];
    // SAFETY: the file handle and a buffer of at least `filen` bytes.
    let status = unsafe { ((*fh).GetInfo)(fh, &mut guid, &mut filen, fi.as_mut_ptr().cast()) };
    if status != EFI_SUCCESS {
        return Err(Errno(-1));
    }
    // SAFETY: the buffer holds an EFI_FILE_INFO (8-byte aligned).
    sb.st_size = unsafe { (*fi.as_ptr().cast::<EfiFileInfo>()).FileSize } as Off;
    Ok(())
}

/// `esp_readdir`.
pub fn esp_readdir(_f: &mut OpenFile, _name: Option<&mut [u8]>) -> Result<(), Errno> {
    Err(Errno::EOPNOTSUPP)
}

/// `espopen(f, &file)`: claim `esp:` (and arm64's `esp0a:`) names.
pub fn espopen(_f: &mut OpenFile, file: &mut &[u8]) -> Result<(), Errno> {
    const ESPDEV: &[u8] = b"esp:";
    const ESP0ADEV: &[u8] = b"esp0a:";

    let fname = *file;
    let esplen = if fname.starts_with(ESPDEV) {
        ESPDEV.len()
    } else if fname.starts_with(ESP0ADEV) {
        // provide compat for arm64 (esp0a:)
        ESP0ADEV.len()
    } else {
        return Err(Errno(1));
    };

    // remove esp: (or esp0a:) prefix
    *file = &fname[esplen..];

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
/* </CODE> */
