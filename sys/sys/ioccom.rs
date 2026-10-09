/* <LICENSES> */
/*-
 * Copyright (c) 1982, 1986, 1990, 1993, 1994
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
 *
 *	@(#)ioccom.h	8.2 (Berkeley) 3/28/94
 */
/* </LICENSES> */

/* <CODE> */
//! `ioctl(2)` command encoding: `<sys/ioccom.h>`.
//!
//! Upstream: sys/sys/ioccom.h @ 3ce1f3f79392
//!
//! An ioctl command keeps its number and group in the low 16 bits, the size of its argument in
//! the next 13 and the direction of the copy (`IOC_IN`, `IOC_OUT`, `IOC_VOID`) in the top
//! three bits of the low 32. Commands are `u_long` in C, `u64` here.
//!
//! Status: `ported`.
//!
//! ## Deviations
//! - `_IOR(g, n, t)`, `_IOW` and `_IOWR` take the argument type as a type parameter
//!   (`_iow::<Ifreq>(b'i', 12)`), and `size_of::<T>()` stands for `sizeof(t)`. The group is the
//!   C's character constant as a byte, the number a `u8`: the encoding has eight bits for each.
//! - The function-like macros are lowercase `const fn`s (`docs/C_TO_RUST.md`): `_ioc`, `_io`,
//!   `_ior`, `_iow`, `_iowr`, `iocparm_len`, `iocbasecmd`, `iocgroup`.

use core::mem::size_of;

use crate::sys::param::PAGE_SIZE;

/// Parameter length, at most 13 bits.
pub const IOCPARM_MASK: u64 = 0x1fff;

/// Max size of ioctl args.
pub const IOCPARM_MAX: usize = PAGE_SIZE;
/// No parameters.
pub const IOC_VOID: u64 = 0x2000_0000;
/// Copy parameters out.
pub const IOC_OUT: u64 = 0x4000_0000;
/// Copy parameters in.
pub const IOC_IN: u64 = 0x8000_0000;
/// Copy parameters in and out.
pub const IOC_INOUT: u64 = IOC_IN | IOC_OUT;
/// Mask for IN/OUT/VOID.
pub const IOC_DIRMASK: u64 = 0xe000_0000;

/// `IOCPARM_LEN(x)`: the size of the command's argument.
pub const fn iocparm_len(x: u64) -> u64 {
    (x >> 16) & IOCPARM_MASK
}

/// `IOCBASECMD(x)`: the command without its argument size.
pub const fn iocbasecmd(x: u64) -> u64 {
    x & !(IOCPARM_MASK << 16)
}

/// `IOCGROUP(x)`: the command's group (the character of `_IOW('i', ...)`).
pub const fn iocgroup(x: u64) -> u64 {
    (x >> 8) & 0xff
}

/// `_IOC(inout, group, num, len)`: encodes a command.
pub const fn _ioc(inout: u64, group: u8, num: u8, len: usize) -> u64 {
    inout | ((len as u64 & IOCPARM_MASK) << 16) | ((group as u64) << 8) | num as u64
}

/// `_IO(g, n)`: a command without an argument.
pub const fn _io(g: u8, n: u8) -> u64 {
    _ioc(IOC_VOID, g, n, 0)
}

/// `_IOR(g, n, t)`: a command whose `T` argument is copied out to the caller.
pub const fn _ior<T>(g: u8, n: u8) -> u64 {
    _ioc(IOC_OUT, g, n, size_of::<T>())
}

/// `_IOW(g, n, t)`: a command whose `T` argument is copied in from the caller.
pub const fn _iow<T>(g: u8, n: u8) -> u64 {
    _ioc(IOC_IN, g, n, size_of::<T>())
}

/// `_IOWR(g, n, t)`: a command whose `T` argument is copied in and back out (`_IORW` in
/// spirit; stdio took that name first).
pub const fn _iowr<T>(g: u8, n: u8) -> u64 {
    _ioc(IOC_INOUT, g, n, size_of::<T>())
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoding() {
        // SIOCSPGRP, _IOW('s', 8, int), and SIOCGPGRP, _IOR('s', 9, int).
        assert_eq!(_iow::<i32>(b's', 8), 0x8004_7308);
        assert_eq!(_ior::<i32>(b's', 9), 0x4004_7309);
        let c = _iowr::<[u8; 32]>(b'i', 17);
        assert_eq!(c, 0xc020_6911);
        assert_eq!(iocparm_len(c), 32);
        assert_eq!(iocgroup(c), u64::from(b'i'));
        assert_eq!(iocbasecmd(c), 0xc000_6911);
        assert_eq!(_io(b'f', 1), 0x2000_6601);
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/sys/ioccom.h");
        let ours = crate::reftest::assert_defines!(defs;
            IOCPARM_MASK, IOC_VOID, IOC_OUT, IOC_IN, IOC_INOUT, IOC_DIRMASK);
        crate::reftest::assert_complete(&defs, "IOC", &[&ours[..], &["IOCPARM_MAX"]].concat());
        assert_eq!(defs["IOCPARM_MAX"], "PAGE_SIZE");
    }
}
/* </TESTS> */
