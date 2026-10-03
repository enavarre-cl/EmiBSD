/*	$OpenBSD: dkio.h,v 1.15 2026/06/24 17:03:06 krw Exp $	*/
/*	$NetBSD: dkio.h,v 1.1 1996/01/30 18:21:48 thorpej Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1987, 1988, 1993
 *	The Regents of the University of California.  All rights reserved.
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
 */
/* </LICENSES> */

//! `<sys/dkio.h>`: disk-specific ioctls.
//!
//! Upstream: sys/sys/dkio.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - `struct dk_diskmap`'s `char *device` is a raw pointer (a user address, as in C).

use crate::sys::disklabel::{Disklabel, Partinfo};
use crate::sys::ioccom::{_io, _ior, _iow, _iowr};

// get and set disklabel; DIOCGPART used internally

/// `DIOCGDINFO`: get.
pub const DIOCGDINFO: u64 = _ior::<Disklabel>(b'd', 101);
/// `DIOCSDINFO`: set.
pub const DIOCSDINFO: u64 = _iow::<Disklabel>(b'd', 102);
/// `DIOCWDINFO`: set, update disk.
pub const DIOCWDINFO: u64 = _iow::<Disklabel>(b'd', 103);
/// `DIOCGPART`: get partition.
pub const DIOCGPART: u64 = _iow::<Partinfo>(b'd', 104);

/// `DIOCEJECT`: eject removable disk.
pub const DIOCEJECT: u64 = _io(b'd', 112);
/// `DIOCLOCK`: lock/unlock pack.
pub const DIOCLOCK: u64 = _iow::<i32>(b'd', 113);

/// `DIOCGPDINFO`: get physical.
pub const DIOCGPDINFO: u64 = _ior::<Disklabel>(b'd', 114);
/// `DIOCRLDINFO`: reload disklabel.
pub const DIOCRLDINFO: u64 = _io(b'd', 115);

/// `struct dk_inquiry`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct DkInquiry {
    /// `vendor`.
    pub vendor: [u8; 64],
    /// `product`.
    pub product: [u8; 128],
    /// `revision`.
    pub revision: [u8; 64],
    /// `serial`.
    pub serial: [u8; 64],
}

/// `DIOCINQ`.
pub const DIOCINQ: u64 = _ior::<DkInquiry>(b'd', 116);

/// `struct dk_cache`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct DkCache {
    /// `wrcache`.
    pub wrcache: u32,
    /// `rdcache`.
    pub rdcache: u32,
}

/// `DIOCGCACHE`: get cache enabled.
pub const DIOCGCACHE: u64 = _ior::<DkCache>(b'd', 117);
/// `DIOCSCACHE`: set cache enabled.
pub const DIOCSCACHE: u64 = _iow::<DkCache>(b'd', 118);

/// `struct dk_diskmap`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct DkDiskmap {
    /// `device`: a user address.
    pub device: *mut u8,
    /// `fd`.
    pub fd: i32,
    /// `flags`.
    pub flags: i32,
}

/// `DIOCMAP`.
pub const DIOCMAP: u64 = _iowr::<DkDiskmap>(b'd', 119);

/// `DIOCCACHESYNC`: sync cache (force?).
pub const DIOCCACHESYNC: u64 = _iow::<i32>(b'd', 120);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_ioctls_carry_the_label_size() {
        // _IOR('d', 101, struct disklabel): out, 1172 bytes, group 'd', number 101.
        assert_eq!(
            DIOCGDINFO,
            0x4000_0000 | (1172 << 16) | (u64::from(b'd') << 8) | 101
        );
        assert_eq!(DIOCRLDINFO, 0x2000_0000 | (u64::from(b'd') << 8) | 115);
        // A struct partinfo is two pointers.
        assert_eq!((DIOCGPART >> 16) & 0x1fff, 16);
    }
}
