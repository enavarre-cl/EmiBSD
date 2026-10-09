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
//! The user/kernel copy routines of `arch/amd64/amd64/copy.S`, pulled in from the `.S` file
//! next to this module (the file keeps OpenBSD's licence block and layout; `{NAME}`
//! placeholders are what `assym.h` provides in C), with the safe Rust entry points the
//! `machine::copy` contract names.
//!
//! Upstream: sys/arch/amd64/amd64/copy.S @ 3ce1f3f79392
//!
//! Status: `ported` (M6-a): `kcopy`, `copyout`, `_copyin`, `copy_fault`, `copyoutstr`,
//! `_copyinstr`, `copystr_fault`, and the `.nofault.0` table (`DECLARE_ONFAULT`) the page
//! fault handler checks `pcb_onfault` against.
//!
//! ## Deviations
//! - `SMAP_STAC`/`SMAP_CLAC` are code patches (`codepatch.c`) applied only on CPUs with
//!   SMAP; neither `codepatch` nor CPU identification (`CR4.SMAP`) is here, so the copies
//!   run without `stac`/`clac`. No retguard.
//! - The C's `copyin()`/`copyinstr()` are `_copyin`/`_copyinstr` here too; the Rust entry
//!   points take the kernel side as a slice, so only the user address can be wrong, and the
//!   assembly's fault handling turns that into `EFAULT`.

use core::arch::global_asm;
use core::mem::offset_of;

use crate::arch::amd64::include::cpu::CpuInfo;
use crate::arch::amd64::include::pcb::Pcb;
use crate::arch::amd64::include::vmparam::VM_MAXUSER_ADDRESS;
use crate::sys::errno::Errno;

global_asm!(
    include_str!("copy.S"),
    CI_CURPCB = const offset_of!(CpuInfo, ci_curpcb),
    PCB_ONFAULT = const offset_of!(Pcb, pcb_onfault),
    VM_MAXUSER_ADDRESS = const VM_MAXUSER_ADDRESS,
    EFAULT = const Errno::EFAULT as i32,
    ENAMETOOLONG = const Errno::ENAMETOOLONG as i32,
    options(att_syntax)
);

/// The C entry points (`int (*)(const void *, void *, size_t)` and the string forms with
/// `size_t *lencopied`); every one returns 0 or an errno.
mod sym {
    unsafe extern "C" {
        pub fn kcopy(src: *const u8, dst: *mut u8, len: usize) -> i32;
        pub fn copyout(kaddr: *const u8, uaddr: *mut u8, len: usize) -> i32;
        pub fn _copyin(uaddr: *const u8, kaddr: *mut u8, len: usize) -> i32;
        pub fn copyoutstr(kaddr: *const u8, uaddr: *mut u8, len: usize, done: *mut usize) -> i32;
        pub fn _copyinstr(uaddr: *const u8, kaddr: *mut u8, len: usize, done: *mut usize) -> i32;
    }
}

/// An assembly routine's `int` as a `Result`.
fn errno(rc: i32) -> Result<(), Errno> {
    if rc == 0 {
        Ok(())
    } else {
        Err(Errno::from_raw(rc).unwrap_or(Errno::EFAULT))
    }
}

/// `copyin`: see `machine::copy::UserCopy::copyin`.
pub fn copyin(uaddr: usize, kbuf: &mut [u8]) -> Result<(), Errno> {
    // SAFETY: the kernel side is `kbuf`, valid for its length; the user side is checked
    // against VM_MAXUSER_ADDRESS and any fault lands in `copy_fault` through `pcb_onfault`.
    errno(unsafe { sym::_copyin(uaddr as *const u8, kbuf.as_mut_ptr(), kbuf.len()) })
}

/// `copyout`: see `machine::copy::UserCopy::copyout`.
pub fn copyout(kbuf: &[u8], uaddr: usize) -> Result<(), Errno> {
    // SAFETY: as for `copyin`, with the kernel side read only.
    errno(unsafe { sym::copyout(kbuf.as_ptr(), uaddr as *mut u8, kbuf.len()) })
}

/// `copyinstr`: see `machine::copy::UserCopy::copyinstr`.
pub fn copyinstr(uaddr: usize, kbuf: &mut [u8]) -> Result<usize, Errno> {
    let mut done = 0usize;
    // SAFETY: as for `copyin`; `done` is a local the routine writes.
    errno(unsafe {
        sym::_copyinstr(uaddr as *const u8, kbuf.as_mut_ptr(), kbuf.len(), &mut done)
    })?;
    Ok(done)
}

/// `copyoutstr`: see `machine::copy::UserCopy::copyoutstr`.
pub fn copyoutstr(kbuf: &[u8], uaddr: usize) -> Result<usize, Errno> {
    let mut done = 0usize;
    // SAFETY: as for `copyout`; `done` is a local the routine writes.
    errno(unsafe { sym::copyoutstr(kbuf.as_ptr(), uaddr as *mut u8, kbuf.len(), &mut done) })?;
    Ok(done)
}

/// `kcopy`: see `machine::copy::UserCopy::kcopy`.
///
/// # Safety
///
/// As `machine::copy::UserCopy::kcopy`.
pub unsafe fn kcopy(src: *const u8, dst: *mut u8, len: usize) -> Result<(), Errno> {
    // SAFETY: the caller's guarantee; a fault on either side lands in `copy_fault`.
    errno(unsafe { sym::kcopy(src, dst, len) })
}
/* </CODE> */
