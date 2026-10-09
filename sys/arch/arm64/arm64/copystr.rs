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
//! The user/kernel string copies of `arch/arm64/arm64/copystr.S`, pulled in from the `.S`
//! file next to this module (the file keeps OpenBSD's licence block and layout; `{NAME}`
//! placeholders are what `assym.h` provides in C).
//!
//! Upstream: sys/arch/arm64/arm64/copystr.S @ 3ce1f3f79392
//!
//! Status: `ported` (M6-a): `copyinstr`, `copyoutstr`.
//!
//! ## Deviations
//! - No retguard. The Rust entry points take the kernel side as a slice.

use core::arch::global_asm;
use core::mem::offset_of;

use crate::arch::arm64::arm64::copy::errno;
use crate::arch::arm64::include::cpu::CpuInfo;
use crate::arch::arm64::include::pcb::Pcb;
use crate::sys::errno::Errno;

global_asm!(
    include_str!("copystr.S"),
    CI_CURPCB = const offset_of!(CpuInfo, ci_curpcb),
    PCB_ONFAULT = const offset_of!(Pcb, pcb_onfault),
    EFAULT = const Errno::EFAULT as i32,
    ENAMETOOLONG = const Errno::ENAMETOOLONG as i32,
);

/// The C entry points; every one returns 0 or an errno.
mod sym {
    unsafe extern "C" {
        pub fn copyinstr(uaddr: *const u8, kaddr: *mut u8, len: usize, done: *mut usize) -> i32;
        pub fn copyoutstr(kaddr: *const u8, uaddr: *mut u8, len: usize, done: *mut usize) -> i32;
    }
}

/// `copyinstr`: see `machine::copy::UserCopy::copyinstr`.
pub fn copyinstr(uaddr: usize, kbuf: &mut [u8]) -> Result<usize, Errno> {
    let mut done = 0usize;
    // SAFETY: the kernel side is `kbuf`, valid for its length; the user side is checked to
    // be a user address and any fault lands in `.Lcopystrfault` through `pcb_onfault`;
    // `done` is a local the routine writes.
    errno(unsafe { sym::copyinstr(uaddr as *const u8, kbuf.as_mut_ptr(), kbuf.len(), &mut done) })?;
    Ok(done)
}

/// `copyoutstr`: see `machine::copy::UserCopy::copyoutstr`.
pub fn copyoutstr(kbuf: &[u8], uaddr: usize) -> Result<usize, Errno> {
    let mut done = 0usize;
    // SAFETY: as for `copyinstr`, with the kernel side read only.
    errno(unsafe { sym::copyoutstr(kbuf.as_ptr(), uaddr as *mut u8, kbuf.len(), &mut done) })?;
    Ok(done)
}
/* </CODE> */
