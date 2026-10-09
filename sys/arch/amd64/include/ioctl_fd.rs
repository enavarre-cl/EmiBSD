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

/*
 * Copyright (C) 1992-1994 by Joerg Wunsch, Dresden
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
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR(S) ``AS IS'' AND ANY
 * EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
 * PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR(S) BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
 * CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT
 * OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR
 * BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF
 * LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
 * (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
 * USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH
 * DAMAGE.
 *
 * From: Id: ioctl_fd.h,v 1.7 1994/10/30 19:17:39 joerg Exp
 */
/* </LICENSES> */

/* <CODE> */
//! amd64 `<machine/ioctl_fd.h>`: the floppy's format request, its drive types and their
//! ioctls.
//!
//! Upstream: sys/arch/amd64/include/ioctl_fd.h @ 3ce1f3f79392
//!
//! `struct fd_formb` is what `fdformat(1)` hands `FD_FORM`; `struct fd_type` describes a
//! density (fd(4)'s `fd_types[]`), `FD_GTYPE` copies one out. Both are ABI: `repr(C)`.
//!
//! ## Deviations
//! - `struct fd_formb`'s `format_info` union of the structured fields and `raw[1]` is the
//!   structured member alone (`raw` is only a view of its first bytes); the
//!   `fd_formb_*` macros are its fields' names.
//! - `struct fd_type`'s `char *name` is a pointer to a NUL-terminated static string;
//!   [`FdType::name`] reads it.
//! - The `FDC_*KBPS` duplicates of `fdreg.h` are that header's constants, re-exported (the
//!   C defines them only when `fdreg.h` has not).

use core::ffi::{CStr, c_char};
use core::mem::size_of;

pub use crate::dev::isa::fdreg::{FDC_125KBPS, FDC_250KBPS, FDC_300KBPS, FDC_500KBPS};
use crate::sys::ioccom::{_ior, _iow};

/// `FD_FORMAT_VERSION`: used to validate before formatting.
pub const FD_FORMAT_VERSION: i32 = 110;
/// `FD_MAX_NSEC`: highest known number of spt - allow for 2.88 MB drives.
pub const FD_MAX_NSEC: usize = 36;

/// `struct fd_idfield_data`: data to write into id fields; for obscure formats, they
/// mustn't match the real values (but mostly do).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct FdIdfieldData {
    /// `cylno`: 0 thru 79 (or 39).
    pub cylno: u8,
    /// `headno`: 0, or 1.
    pub headno: u8,
    /// `secno`: starting at 1!
    pub secno: u8,
    /// `secsize`: usually 2.
    pub secsize: u8,
}

/// `struct fd_form_data`: DO NOT CHANGE THE LAYOUT OF THIS STRUCTS; it is hardware-dependant
/// since it exactly matches the byte sequence to write to FDC during its `format track'
/// operation.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct FdFormData {
    /// `secshift` (`fd_formb_secshift`): 0 -> 128, ...; usually 2 -> 512.
    pub secshift: u8,
    /// `nsecs` (`fd_formb_nsecs`): must be <= FD_MAX_NSEC.
    pub nsecs: u8,
    /// `gaplen` (`fd_formb_gaplen`): GAP 3 length; usually 84.
    pub gaplen: u8,
    /// `fillbyte` (`fd_formb_fillbyte`): usually 0xf6.
    pub fillbyte: u8,
    /// `idfields` (`fd_formb_cylno(i)` ...): 0 <= idx < nsecs used.
    pub idfields: [FdIdfieldData; FD_MAX_NSEC],
}

/// `struct fd_formb`: the `FD_FORM` request.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct FdFormb {
    /// `format_version`: == FD_FORMAT_VERSION.
    pub format_version: i32,
    /// `cyl`.
    pub cyl: i32,
    /// `head`.
    pub head: i32,
    /// `transfer_rate`: fdreg.h: FDC_???KBPS.
    pub transfer_rate: i32,
    /// `format_info.structured`.
    pub format_info: FdFormData,
}

/// `struct fd_type`: floppies come in various flavors, e.g., 1.2MB vs 1.44MB; here is how we
/// tell them apart.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct FdType {
    /// `sectrac`: sectors per track.
    pub sectrac: i32,
    /// `heads`: number of heads.
    pub heads: i32,
    /// `seccyl`: sectors per cylinder.
    pub seccyl: i32,
    /// `secsize`: size code for sectors.
    pub secsize: i32,
    /// `datalen`: data len when secsize = 0.
    pub datalen: i32,
    /// `steprate`: step rate and head unload time.
    pub steprate: i32,
    /// `gap1`: gap len between sectors.
    pub gap1: i32,
    /// `gap2`: formatting gap.
    pub gap2: i32,
    /// `tracks`: total num of tracks.
    pub tracks: i32,
    /// `size`: size of disk in sectors.
    pub size: i32,
    /// `step`: steps per cylinder.
    pub step: i32,
    /// `rate`: transfer speed code.
    pub rate: i32,
    /// `name`: a NUL-terminated static string.
    pub name: *const c_char,
}

impl FdType {
    /// `type->name` as text.
    pub fn name(&self) -> &'static str {
        // SAFETY: every `FdType` names a NUL-terminated static string (fd(4)'s table).
        unsafe { CStr::from_ptr(self.name) }.to_str().unwrap_or("?")
    }
}

// SAFETY: `name` points at a static, read-only string; the rest is plain integers.
unsafe impl Sync for FdType {}

/// `FD_FORM`: format a track.
pub const FD_FORM: u64 = _iow::<FdFormb>(b'F', 61);
/// `FD_GTYPE`: get drive type.
pub const FD_GTYPE: u64 = _ior::<FdType>(b'F', 62);
/// `FD_STYPE`: set drive type.
pub const FD_STYPE: u64 = _iow::<FdType>(b'F', 63);

/// `FD_GOPTS`: drive options, see below.
pub const FD_GOPTS: u64 = _ior::<i32>(b'F', 64);
/// `FD_SOPTS`.
pub const FD_SOPTS: u64 = _iow::<i32>(b'F', 65);

/// `FDOPT_NORETRY`: no retries on failure (cleared on close).
pub const FDOPT_NORETRY: i32 = 0x0001;

const _: () = assert!(size_of::<FdFormb>() == 16 + 4 + 4 * FD_MAX_NSEC);
const _: () = assert!(size_of::<FdType>() == 12 * 4 + 8);
/* </CODE> */
