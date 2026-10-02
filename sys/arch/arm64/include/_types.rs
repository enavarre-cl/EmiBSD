/* $OpenBSD: _types.h,v 1.7 2026/08/31 10:58:08 tb Exp $ */

/*-
 * Copyright (c) 1990, 1993
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
 *	@(#)types.h	8.3 (Berkeley) 1/5/94
 *	@(#)ansi.h	8.2 (Berkeley) 1/4/94
 */

//! arm64 `<machine/_types.h>`: alignment rules and the `label_t` register save area.
//!
//! Upstream: sys/arch/arm64/include/_types.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - The exact-width, minimum-width, fast, pointer-sized and greatest-width integer typedefs,
//!   `__size_t`, `__ssize_t`, `__ptrdiff_t`, `__va_list`, the floating-point and the
//!   wide-character typedefs are Rust primitives (`i8`..`u64`, `usize`, `isize`); they are not
//!   re-declared. `__register_t` is `sys::types::Register`.
//! - `__vaddr_t`, `__paddr_t`, `__vsize_t`, `__psize_t` are the `Vaddr`/`Paddr`/`Vsize`/`Psize`
//!   newtypes in `sys::types`.
//! - `_ALIGN(p)` and `_ALIGNED_POINTER(p, t)` are the functions [`_align`] and
//!   [`_aligned_pointer`].

/// `_ALIGNBYTES`: rounding mask that aligns an address for every data type (`sizeof(long) - 1`).
pub const _ALIGNBYTES: usize = core::mem::size_of::<usize>() - 1;
/// `_STACKALIGNBYTES`: the stack pointer is kept 16-byte aligned.
pub const _STACKALIGNBYTES: usize = 15;
/// `_MAX_PAGE_SHIFT`: same as `PAGE_SHIFT`.
pub const _MAX_PAGE_SHIFT: usize = 12;

/// `_ALIGN(p)`: rounds `p` (a pointer or byte index) up to a value correctly aligned for all data
/// types (`int`, `long`, ...).
pub const fn _align(p: usize) -> usize {
    (p + _ALIGNBYTES) & !_ALIGNBYTES
}

/// `_ALIGNED_POINTER(p, t)`: whether a `T` may be fetched from address `p`: `p` must be a
/// multiple of `size_of::<T>()`. The C macro uses `sizeof(t) - 1` as the mask, so a zero-sized
/// `T` wraps the mask to all ones, exactly as it does here.
pub const fn _aligned_pointer<T>(p: usize) -> bool {
    p & core::mem::size_of::<T>().wrapping_sub(1) == 0
}

/// `label_t`: the register save area of the kernel's `setjmp`/`longjmp` (used by ddb).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Label {
    /// Saved callee-preserved registers, stack pointer and return address; the layout is
    /// `locore.S`'s.
    pub val: [isize; 13],
}
