/* <LICENSES> */
/*
 * Copyright (c) 2002 Marc Espie.
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
 * THIS SOFTWARE IS PROVIDED BY THE OPENBSD PROJECT AND CONTRIBUTORS
 * ``AS IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
 * LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
 * A PARTICULAR PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL THE OPENBSD
 * PROJECT OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
 * SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
 * LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
 * DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
 * THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
 * (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
 * OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! `<sys/limits.h>`: common definitions for limits.h.
//!
//! Upstream: sys/sys/limits.h @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M6 (part b) needs `SSIZE_MAX` (`sys_write`); the integer
//! limits come along. The floating point legacy values (`FLT_DIG` and friends) and the
//! `machine/_float.h` include are not kernel material.
//!
//! ## Deviations
//! - `<machine/limits.h>` of amd64 and arm64 both define `SSIZE_MAX` as `LONG_MAX` and
//!   `SIZE_MAX` as `ULONG_MAX`; with every target 64-bit (`__LP64__`) they are given here
//!   instead of through a per-architecture module.

/// `CHAR_BIT`: number of bits in a char.
pub const CHAR_BIT: u32 = 8;

/// `SCHAR_MAX`: max value for a signed char.
pub const SCHAR_MAX: i8 = 0x7f;
/// `SCHAR_MIN`: min value for a signed char.
pub const SCHAR_MIN: i8 = -0x7f - 1;
/// `UCHAR_MAX`: max value for an unsigned char.
pub const UCHAR_MAX: u8 = 0xff;
/// `CHAR_MAX` (`char` is signed).
pub const CHAR_MAX: i8 = 0x7f;
/// `CHAR_MIN`.
pub const CHAR_MIN: i8 = -0x7f - 1;

/// `MB_LEN_MAX`: allow UTF-8 (RFC 3629).
pub const MB_LEN_MAX: usize = 4;

/// `USHRT_MAX`: max value for an unsigned short.
pub const USHRT_MAX: u16 = 0xffff;
/// `SHRT_MAX`: max value for a short.
pub const SHRT_MAX: i16 = 0x7fff;
/// `SHRT_MIN`: min value for a short.
pub const SHRT_MIN: i16 = -0x7fff - 1;

/// `UINT_MAX`: max value for an unsigned int.
pub const UINT_MAX: u32 = 0xffff_ffff;
/// `INT_MAX`: max value for an int.
pub const INT_MAX: i32 = 0x7fff_ffff;
/// `INT_MIN`: min value for an int.
pub const INT_MIN: i32 = -0x7fff_ffff - 1;

/// `ULONG_MAX`: max value for unsigned long (`__LP64__`).
pub const ULONG_MAX: u64 = 0xffff_ffff_ffff_ffff;
/// `LONG_MAX`: max value for a signed long.
pub const LONG_MAX: i64 = 0x7fff_ffff_ffff_ffff;
/// `LONG_MIN`: min value for a signed long.
pub const LONG_MIN: i64 = -0x7fff_ffff_ffff_ffff - 1;

/// `ULLONG_MAX`: max value for unsigned long long.
pub const ULLONG_MAX: u64 = 0xffff_ffff_ffff_ffff;
/// `LLONG_MAX`: max value for a signed long long.
pub const LLONG_MAX: i64 = 0x7fff_ffff_ffff_ffff;
/// `LLONG_MIN`: min value for a signed long long.
pub const LLONG_MIN: i64 = -0x7fff_ffff_ffff_ffff - 1;

/// `LONG_BIT`.
pub const LONG_BIT: u32 = 64;
/// `WORD_BIT`.
pub const WORD_BIT: u32 = 32;

/// `SSIZE_MAX` (`<machine/limits.h>`): max value for a ssize_t.
pub const SSIZE_MAX: isize = LONG_MAX as isize;
/// `SIZE_MAX` (`<machine/limits.h>`): max value for a size_t.
pub const SIZE_MAX: usize = ULONG_MAX as usize;

const _: () = {
    assert!(size_of::<usize>() == 8);
};
/* </CODE> */
