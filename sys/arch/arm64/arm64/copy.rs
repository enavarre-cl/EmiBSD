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
/* </LICENSES> */

/* <CODE> */
//! The user/kernel copy routines of `arch/arm64/arm64/copy.S`, pulled in from the `.S` file
//! next to this module (the file keeps OpenBSD's licence block and layout; `{NAME}`
//! placeholders are what `assym.h` provides in C), with the safe Rust entry points the
//! `machine::copy` contract names. The string copies are `copystr.rs`.
//!
//! Upstream: sys/arch/arm64/arm64/copy.S @ 3ce1f3f79392
//!
//! Status: `ported` (M6-a): `copyin`, `copyin32`, `copyout`, `kcopy`.
//!
//! ## Deviations
//! - No retguard.
//! - The Rust entry points take the kernel side as a slice, so only the user address can be
//!   wrong, and the assembly's `pcb_onfault` handling turns that into `EFAULT`.

use core::arch::global_asm;
use core::mem::offset_of;

use crate::arch::arm64::include::cpu::CpuInfo;
use crate::arch::arm64::include::pcb::Pcb;
use crate::sys::errno::Errno;

global_asm!(
    include_str!("copy.S"),
    CI_CURPCB = const offset_of!(CpuInfo, ci_curpcb),
    PCB_ONFAULT = const offset_of!(Pcb, pcb_onfault),
    EFAULT = const Errno::EFAULT as i32,
);

/// The C entry points; every one returns 0 or an errno.
mod sym {
    unsafe extern "C" {
        pub fn copyin(uaddr: *const u8, kaddr: *mut u8, len: usize) -> i32;
        pub fn copyin32(uaddr: *const u32, kaddr: *mut u32) -> i32;
        pub fn copyout(kaddr: *const u8, uaddr: *mut u8, len: usize) -> i32;
        pub fn kcopy(src: *const u8, dst: *mut u8, len: usize) -> i32;
    }
}

/// An assembly routine's `int` as a `Result`.
pub(super) fn errno(rc: i32) -> Result<(), Errno> {
    if rc == 0 {
        Ok(())
    } else {
        Err(Errno::from_raw(rc).unwrap_or(Errno::EFAULT))
    }
}

/// `copyin`: see `machine::copy::UserCopy::copyin`.
pub fn copyin(uaddr: usize, kbuf: &mut [u8]) -> Result<(), Errno> {
    // SAFETY: the kernel side is `kbuf`, valid for its length; the user side is checked to
    // be a user address and any fault lands in `.Lcopyfault` through `pcb_onfault`.
    errno(unsafe { sym::copyin(uaddr as *const u8, kbuf.as_mut_ptr(), kbuf.len()) })
}

/// `copyin32`: atomically copies a 32-bit word from user space.
pub fn copyin32(uaddr: usize) -> Result<u32, Errno> {
    let mut word = 0u32;
    // SAFETY: as for `copyin`; `word` is a local the routine writes.
    errno(unsafe { sym::copyin32(uaddr as *const u32, &mut word) })?;
    Ok(word)
}

/// `copyout`: see `machine::copy::UserCopy::copyout`.
pub fn copyout(kbuf: &[u8], uaddr: usize) -> Result<(), Errno> {
    // SAFETY: as for `copyin`, with the kernel side read only.
    errno(unsafe { sym::copyout(kbuf.as_ptr(), uaddr as *mut u8, kbuf.len()) })
}

/// `kcopy`: see `machine::copy::UserCopy::kcopy`.
///
/// # Safety
///
/// As `machine::copy::UserCopy::kcopy`.
pub unsafe fn kcopy(src: *const u8, dst: *mut u8, len: usize) -> Result<(), Errno> {
    // SAFETY: the caller's guarantee; a fault on either side lands in `.Lcopyfault`.
    errno(unsafe { sym::kcopy(src, dst, len) })
}
/* </CODE> */
