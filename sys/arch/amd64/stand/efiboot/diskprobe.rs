/*	$OpenBSD: diskprobe.c,v 1.4 2026/05/03 13:10:46 stsp Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1997 Tobias Weingartner
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

/* We want the disk type names from disklabel.h */
/* </LICENSES> */

/* <CODE> */
//! The disk probe: name the EFI disks (`hd0`..., `cd0`), checksum their first blocks for
//! the kernel to match them, pass `BOOTARG_DISKINFO`, and find hibernation signatures.
//!
//! Upstream: sys/arch/amd64/stand/efiboot/diskprobe.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `disklist` is the boxed vector [`DISKLIST`] (`TAILQ` in C); `bootdev_dip` the atomic
//!   pointer [`BOOTDEV_DIP`] into it.
//! - `SOFTRAID`'s `srprobe()` is feature `softraid`, not ported: with it on, the probe says
//!   so (the C probes the softraid volumes there).

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::ptr;
use core::sync::atomic::{AtomicPtr, AtomicU32, Ordering};

use boot::bootarg::addbootarg;
use libkern::staticcell::StaticCell;
use libsa::hdr::disklabel::{
    DTYPE_ATAPI, DTYPE_ESDI, DTYPE_SCSI, DTYPE_ST506, FS_SWAP, RAW_PART, dl_getpoffset,
    dl_getpsize, dl_sectoblk,
};
use libsa::hdr::hibernate::HIBERNATE_MAGIC;
use libsa::hdr::param::{DEV_BSIZE, MAXBSIZE};
use libsa::hdr::reboot::makebootdev;
use libsa::printf;
use libsa::stand::F_READ;

use crate::biosvar::{
    BDI_BADLABEL, BDI_EL_TORITO, BDI_GOODLABEL, BDI_HIBVALID, BDI_INVALID, BOOTARG_CKSUMLEN,
    BOOTARG_DISKINFO, BiosDiskinfo,
};
use crate::disk::DiskInfo;
use crate::efiboot::EFI_DISKLIST;
use crate::efidev::efi_getdisklabel;

/// `MAX_CKSUMLEN`: max # of blks to cksum.
const MAX_CKSUMLEN: u32 = (MAXBSIZE / DEV_BSIZE) as u32;

/// `disklist`: the disks we found and probed.
#[allow(clippy::vec_box)] // the boxes keep the addresses f_devdata and bootdev_dip hold
pub static DISKLIST: StaticCell<Vec<Box<DiskInfo>>> = StaticCell::new(Vec::new());

/// `bootdev_dip`: the boot device's disk.
pub static BOOTDEV_DIP: AtomicPtr<DiskInfo> = AtomicPtr::new(ptr::null_mut());

/// `bios_cksumlen`: the blocks checksummed.
pub static BIOS_CKSUMLEN: AtomicU32 = AtomicU32::new(0);

/// `efi_hardprobe()`: label and name the EFI disks, in `efi_disklist`'s order.
fn efi_hardprobe() {
    let (mut scsi, mut ide, mut atapi) = (0u32, 0u32, 0u32);

    // SAFETY: single-threaded; the only references to the two lists.
    let (efi_list, list) = unsafe { (EFI_DISKLIST.get_mut(), DISKLIST.get_mut()) };
    for mut dip in efi_list.drain(..) {
        let mut n = scsi + ide;
        let type_;
        let bsdunit;

        // Try to find the label, to figure out device type
        let label = match dip.efi_info.as_deref() {
            Some(ed) => efi_getdisklabel(ed, &mut dip.disklabel),
            None => Err("no EFI block I/O"),
        };
        if label.is_err() {
            type_ = 0;
            printf!(" hd{}*", n);
            bsdunit = ide;
            ide += 1;
        } else {
            // Best guess
            match dip.disklabel.d_type {
                DTYPE_SCSI => {
                    type_ = 4;
                    bsdunit = scsi;
                    scsi += 1;
                    dip.bios_info.flags |= BDI_GOODLABEL;
                }
                DTYPE_ESDI | DTYPE_ST506 => {
                    type_ = 0;
                    bsdunit = ide;
                    ide += 1;
                    dip.bios_info.flags |= BDI_GOODLABEL;
                }
                DTYPE_ATAPI => {
                    type_ = 6;
                    n = atapi;
                    bsdunit = atapi;
                    atapi += 1;
                    dip.bios_info.flags |= BDI_GOODLABEL | BDI_EL_TORITO;
                }
                _ => {
                    dip.bios_info.flags |= BDI_BADLABEL;
                    type_ = 0; // XXX Suggest IDE
                    bsdunit = ide;
                    ide += 1;
                }
            }
            printf!(" {}d{}", if type_ == 6 { 'c' } else { 'h' }, n);
        }
        dip.bios_info.bios_number = if type_ != 6 {
            0x80 | n as i32
        } else {
            0xe0 | n as i32
        };

        dip.bios_info.checksum = 0; // just in case
        // Fill out best we can
        let dev = makebootdev(type_, 0, 0, bsdunit, RAW_PART as u32) as i32;
        dip.bsddev = dev;
        dip.bios_info.bsd_dev = dev;
        check_hibernate(&mut dip);

        // Add to queue of disks
        list.push(dip);
    }
}

/// `diskprobe()`: probe for all BIOS supported disks (the EFI ones) and pass them to the
/// kernel.
pub fn diskprobe() {
    // Init stuff
    // SAFETY: single-threaded; the only reference to the list.
    unsafe { DISKLIST.get_mut() }.clear();

    efi_hardprobe();

    #[cfg(feature = "softraid")]
    printf!(" (softraid: not ported)");

    // Checksumming of hard disks
    let mut i = 0;
    loop {
        let more = disksum(i);
        i += 1;
        if !(more && i < MAX_CKSUMLEN) {
            break;
        }
    }
    BIOS_CKSUMLEN.store(i, Ordering::Relaxed);

    // Get space for passing bios_diskinfo stuff to kernel, copy it out
    // SAFETY: single-threaded; a shared look at the list.
    let list = unsafe { DISKLIST.get() };
    let mut bytes = Vec::new();
    for dip in list.iter() {
        bytes.extend_from_slice(dip.bios_info.as_bytes());
    }
    let end = BiosDiskinfo {
        bios_number: -1,
        ..Default::default()
    };
    bytes.extend_from_slice(end.as_bytes());

    // Register for kernel use
    addbootarg(BOOTARG_CKSUMLEN, &i.to_ne_bytes());
    addbootarg(BOOTARG_DISKINFO, &bytes);
}

/// `dklookup(dev)`: the disk of BIOS number `dev`, null if none.
pub fn dklookup(dev: i32) -> *mut DiskInfo {
    // SAFETY: single-threaded; the pointer handed out points into a box of the list, which
    // is never dropped while the program runs.
    let list = unsafe { DISKLIST.get_mut() };
    for dip in list.iter_mut() {
        if dip.bios_info.bios_number == dev {
            return &mut **dip;
        }
    }
    ptr::null_mut()
}

/// `dump_diskinfo()`: the BIOS view of the disks.
#[allow(dead_code)] // as in the C, efiboot's `machine diskinfo` is efi_dump_diskinfo
pub fn dump_diskinfo() {
    printf!("Disk\tBIOS#\tType\tCyls\tHeads\tSecs\tFlags\tChecksum\n");
    // SAFETY: single-threaded; a shared look at the list.
    let list = unsafe { DISKLIST.get() };
    for dip in list.iter() {
        let bdi = dip.bios_info;
        let d = bdi.bios_number;
        let (flags, cyl, heads, secs, cksum) = (
            bdi.flags,
            bdi.bios_cylinders,
            bdi.bios_heads,
            bdi.bios_sectors,
            bdi.checksum,
        );
        let (c, u) = if flags & BDI_EL_TORITO != 0 {
            ('c', 0)
        } else {
            (if d & 0x80 != 0 { 'h' } else { 'f' }, d & 0x7f)
        };

        printf!(
            "{}d{}\t{:#x}\t{}\t{}\t{}\t{}\t{:#x}\t{:#x}\n",
            c,
            u,
            d,
            if flags & BDI_BADLABEL != 0 {
                "*none*"
            } else {
                "label"
            },
            cyl,
            heads,
            secs,
            flags,
            cksum
        );
    }
}

/// `disksum(blk)`: checksum one more block on all hard drives (with libz's `adler32`, quick,
/// small, and available); true if two drives still sum the same.
fn disksum(blk: u32) -> bool {
    let mut reprobe = false;
    let mut buf = [0u8; DEV_BSIZE];

    // SAFETY: single-threaded; the only reference to the list.
    let list = unsafe { DISKLIST.get_mut() };
    for k in 0..list.len() {
        let (before, rest) = list.split_at_mut(k);
        let dip = &mut rest[0];
        let bdi = &mut dip.bios_info;

        // Skip this disk if it is not a HD or has had an I/O error
        if (bdi.bios_number & 0x80) == 0 || (bdi.flags & BDI_INVALID) != 0 {
            continue;
        }

        // Adler32 checksum
        let st = (dip.diskio)(F_READ, dip, blk, 1, &mut buf);
        let bdi = &mut dip.bios_info;
        if st != 0 {
            bdi.flags |= BDI_INVALID;
            continue;
        }
        bdi.checksum = libz::adler32(bdi.checksum, Some(&buf));

        for dip2 in before.iter() {
            let bd = &dip2.bios_info;
            if (bd.bios_number & 0x80) != 0
                && (bd.flags & BDI_INVALID) == 0
                && bdi.checksum == bd.checksum
            {
                reprobe = true;
            }
        }
    }

    reprobe
}

/// `bootdev_has_hibernate()`.
pub fn bootdev_has_hibernate() -> bool {
    let dip = BOOTDEV_DIP.load(Ordering::Relaxed);
    if dip.is_null() {
        return false;
    }
    // SAFETY: `bootdev_dip` points into DISKLIST's boxes.
    (unsafe { (*dip).bios_info.flags } & BDI_HIBVALID) != 0
}

/// `check_hibernate(dip)`: is there a hibernation signature at the end of the swap
/// partition (`b`)?
pub fn check_hibernate(dip: &mut DiskInfo) {
    let mut buf = [0u8; DEV_BSIZE];

    // read hibernate
    let p = dip.disklabel.d_partitions[1];
    if p.p_fstype != FS_SWAP || dl_getpsize(&p) == 0 {
        return;
    }

    let sec = dl_getpoffset(&p) + dl_getpsize(&p) - 1;

    let blk = dl_sectoblk(&dip.disklabel, sec) as i64;
    let error = (dip.strategy)((dip as *mut DiskInfo).cast(), F_READ, blk, &mut buf, None);
    if error.is_ok() && u32::from_ne_bytes([buf[0], buf[1], buf[2], buf[3]]) == HIBERNATE_MAGIC {
        dip.bios_info.flags |= BDI_HIBVALID; // Hibernate present
    }
}
/* </CODE> */
