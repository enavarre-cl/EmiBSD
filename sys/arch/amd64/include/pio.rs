/*	$OpenBSD: pio.h,v 1.5 2015/04/25 21:31:24 guenther Exp $	*/
/*	$NetBSD: pio.h,v 1.2 2003/02/27 11:22:46 fvdl Exp $	*/

/*-
 * Copyright (c) 1998 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Charles M. Hannum.
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
 * THIS SOFTWARE IS PROVIDED BY THE NETBSD FOUNDATION, INC. AND CONTRIBUTORS
 * ``AS IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
 * TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
 * PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL THE FOUNDATION OR CONTRIBUTORS
 * BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
 * CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
 * SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
 * INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
 * CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
 * ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
 * POSSIBILITY OF SUCH DAMAGE.
 */

//! Programmed I/O: the x86 `in` and `out` instructions, `<machine/pio.h>`.
//!
//! Upstream: sys/arch/amd64/include/pio.h @ 3ce1f3f79392
//!
//! Every function is `unsafe`: a port access has whatever effect the device behind the port
//! defines, and only the caller knows which device that is.
//!
//! ## Deviations
//! - The immediate-port variants (`__inbc`, `__outbc`, ...) exist so GCC can emit the short
//!   `in al, imm8` encoding when the port is a constant below 256. rustc makes the same choice
//!   on its own, so there is one function per width.
//! - The string forms take slices instead of a pointer and a count.

use core::arch::asm;

/// `inb`: reads a byte from `port`.
///
/// # Safety
///
/// The caller must know what device answers at `port` and that reading it is appropriate.
#[inline]
pub unsafe fn inb(port: u16) -> u8 {
    let data: u8;
    // SAFETY: `in` has no effect on memory or the stack; the caller vouches for the port.
    unsafe {
        asm!("in al, dx", in("dx") port, out("al") data, options(nomem, nostack, preserves_flags));
    }
    data
}

/// `inw`: reads a 16-bit word from `port`.
///
/// # Safety
///
/// As for [`inb`].
#[inline]
pub unsafe fn inw(port: u16) -> u16 {
    let data: u16;
    // SAFETY: as for `inb`.
    unsafe {
        asm!("in ax, dx", in("dx") port, out("ax") data, options(nomem, nostack, preserves_flags));
    }
    data
}

/// `inl`: reads a 32-bit word from `port`.
///
/// # Safety
///
/// As for [`inb`].
#[inline]
pub unsafe fn inl(port: u16) -> u32 {
    let data: u32;
    // SAFETY: as for `inb`.
    unsafe {
        asm!("in eax, dx", in("dx") port, out("eax") data, options(nomem, nostack, preserves_flags));
    }
    data
}

/// `insb`: reads `buf.len()` bytes from `port` into `buf`.
///
/// # Safety
///
/// As for [`inb`].
#[inline]
pub unsafe fn insb(port: u16, buf: &mut [u8]) {
    // SAFETY: `rep insb` writes exactly `rcx` bytes at `rdi`, which is the slice; the direction
    // flag is clear by ABI; the caller vouches for the port.
    unsafe {
        asm!(
            "rep insb",
            in("dx") port,
            inout("rdi") buf.as_mut_ptr() => _,
            inout("rcx") buf.len() => _,
            options(nostack, preserves_flags)
        );
    }
}

/// `insw`: reads `buf.len()` 16-bit words from `port` into `buf`.
///
/// # Safety
///
/// As for [`inb`].
#[inline]
pub unsafe fn insw(port: u16, buf: &mut [u16]) {
    // SAFETY: as for `insb`, with 16-bit elements.
    unsafe {
        asm!(
            "rep insw",
            in("dx") port,
            inout("rdi") buf.as_mut_ptr() => _,
            inout("rcx") buf.len() => _,
            options(nostack, preserves_flags)
        );
    }
}

/// `insl`: reads `buf.len()` 32-bit words from `port` into `buf`.
///
/// # Safety
///
/// As for [`inb`].
#[inline]
pub unsafe fn insl(port: u16, buf: &mut [u32]) {
    // SAFETY: as for `insb`, with 32-bit elements.
    unsafe {
        asm!(
            "rep insd",
            in("dx") port,
            inout("rdi") buf.as_mut_ptr() => _,
            inout("rcx") buf.len() => _,
            options(nostack, preserves_flags)
        );
    }
}

/// `outb`: writes a byte to `port`.
///
/// # Safety
///
/// The caller must know what device answers at `port` and that writing `data` is appropriate.
#[inline]
pub unsafe fn outb(port: u16, data: u8) {
    // SAFETY: `out` has no effect on memory or the stack; the caller vouches for the port.
    unsafe {
        asm!("out dx, al", in("dx") port, in("al") data, options(nomem, nostack, preserves_flags));
    }
}

/// `outw`: writes a 16-bit word to `port`.
///
/// # Safety
///
/// As for [`outb`].
#[inline]
pub unsafe fn outw(port: u16, data: u16) {
    // SAFETY: as for `outb`.
    unsafe {
        asm!("out dx, ax", in("dx") port, in("ax") data, options(nomem, nostack, preserves_flags));
    }
}

/// `outl`: writes a 32-bit word to `port`.
///
/// # Safety
///
/// As for [`outb`].
#[inline]
pub unsafe fn outl(port: u16, data: u32) {
    // SAFETY: as for `outb`.
    unsafe {
        asm!("out dx, eax", in("dx") port, in("eax") data, options(nomem, nostack, preserves_flags));
    }
}

/// `outsb`: writes the bytes of `buf` to `port`.
///
/// # Safety
///
/// As for [`outb`].
#[inline]
pub unsafe fn outsb(port: u16, buf: &[u8]) {
    // SAFETY: `rep outsb` reads exactly `rcx` bytes at `rsi`, which is the slice; the direction
    // flag is clear by ABI; the caller vouches for the port.
    unsafe {
        asm!(
            "rep outsb",
            in("dx") port,
            inout("rsi") buf.as_ptr() => _,
            inout("rcx") buf.len() => _,
            options(nostack, preserves_flags, readonly)
        );
    }
}

/// `outsw`: writes the 16-bit words of `buf` to `port`.
///
/// # Safety
///
/// As for [`outb`].
#[inline]
pub unsafe fn outsw(port: u16, buf: &[u16]) {
    // SAFETY: as for `outsb`, with 16-bit elements.
    unsafe {
        asm!(
            "rep outsw",
            in("dx") port,
            inout("rsi") buf.as_ptr() => _,
            inout("rcx") buf.len() => _,
            options(nostack, preserves_flags, readonly)
        );
    }
}

/// `outsl`: writes the 32-bit words of `buf` to `port`.
///
/// # Safety
///
/// As for [`outb`].
#[inline]
pub unsafe fn outsl(port: u16, buf: &[u32]) {
    // SAFETY: as for `outsb`, with 32-bit elements.
    unsafe {
        asm!(
            "rep outsd",
            in("dx") port,
            inout("rsi") buf.as_ptr() => _,
            inout("rcx") buf.len() => _,
            options(nostack, preserves_flags, readonly)
        );
    }
}
