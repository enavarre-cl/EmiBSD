/*	$OpenBSD: disk.h,v 1.7 2020/12/09 18:10:18 krw Exp $	*/
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
/* </LICENSES> */

//! `disk.h` of amd64's boot programs: `struct diskinfo`, everything efiboot knows of a disk.
//!
//! Upstream: sys/arch/amd64/stand/libsa/disk.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - The disks are boxed in vectors (`disklist`, `efi_disklist` in `diskprobe.rs` and
//!   `efiboot.rs`) instead of a `TAILQ`; a box keeps its address, which `f_devdata` and
//!   `bootdev_dip` hold, as the C's list entries do.
//! - `diskio` takes the disk by reference and the buffer as a slice of `nsect` sectors.
//! - `sr_vol` (`struct sr_boot_volume *`, `SOFTRAID`) is an untyped pointer, always null:
//!   softraid is not ported (feature `softraid`).

use alloc::boxed::Box;
use core::ffi::c_void;

use libsa::hdr::disklabel::Disklabel;
use libsa::stand::StrategyFn;

use crate::biosvar::BiosDiskinfo;
use crate::efidev::EfiDiskinfo;

/// `diskio`: read or write `nsect` sectors at `off` (in `DEV_BSIZE` units); 0 or -1.
pub type DiskioFn = fn(rw: i32, dip: &DiskInfo, off: u32, nsect: i32, buf: &mut [u8]) -> i32;

/// `struct diskinfo`: all the info on a disk we've found.
pub struct DiskInfo {
    /// `efi_info`: the EFI block I/O of the disk.
    pub efi_info: Option<Box<EfiDiskinfo>>,
    /// `bios_info`: what the kernel gets of it.
    pub bios_info: BiosDiskinfo,
    /// `disklabel`.
    pub disklabel: Disklabel,
    /// `sr_vol`: the softraid volume (`SOFTRAID`), null.
    #[allow(dead_code)] // read by the softraid code (feature softraid), not ported yet
    pub sr_vol: *mut c_void,
    /// `diskio`.
    pub diskio: DiskioFn,
    /// `strategy`: libsa's device strategy for it.
    pub strategy: StrategyFn,
    /// `bsddev`.
    pub bsddev: i32,
    /// `bootdev`.
    pub bootdev: i32,
}

// SAFETY: efiboot runs on one CPU without threads; the EFI pointers inside are only used
// from it, before ExitBootServices.
unsafe impl Send for DiskInfo {}
