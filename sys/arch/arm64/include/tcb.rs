/*	$OpenBSD: tcb.h,v 1.2 2017/01/10 13:13:12 patrick Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 2011 Philip Guenther <guenther@openbsd.org>
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
//! arm64 `<machine/tcb.h>`: the thread control block, which user space reaches through
//! `TPIDR_EL0`.
//!
//! Upstream: sys/arch/arm64/include/tcb.h @ 3ce1f3f79392
//!
//! Status: `ported` (the kernel half). `cpu_switchto` (`cpuswitch.S`) loads `pcb_tcb` into
//! `TPIDR_EL0` for the thread it switches to; `TCB_SET` also loads it at once, for the
//! running thread.
//!
//! ## Deviations
//! - The macros `TCB_GET(p)`/`TCB_SET(p, addr)` are the functions `tcb_get`/`tcb_set`; the
//!   TCB is a `usize` (a user address the kernel never dereferences) instead of a `void *`.
//! - The userland half (`TLS_VARIANT`, `__aarch64_read_tcb`, the userland `TCB_GET()`) is not
//!   kernel code and is not here.

use core::arch::asm;
use core::ptr;

use crate::sys::proc::Proc;

/// `__aarch64_set_tcb(tcb)`: loads the user thread pointer register.
#[inline]
pub fn __aarch64_set_tcb(tcb: usize) {
    // SAFETY: TPIDR_EL0 is the user's thread pointer; the kernel never reads it (it uses
    // TPIDR_EL1 for curcpu), so writing any value only changes what user space sees.
    unsafe { asm!("msr tpidr_el0, {}", in(reg) tcb, options(nomem, nostack, preserves_flags)) };
}

/// `TCB_GET(p)`: `p`'s saved TCB address.
#[inline]
pub fn tcb_get(p: &Proc) -> usize {
    p.pcb().pcb_tcb.get() as usize
}

/// `TCB_SET(p, addr)`: records `addr` in `p`'s pcb and loads it into `TPIDR_EL0` (`p` is the
/// running thread).
#[inline]
pub fn tcb_set(p: &Proc, addr: usize) {
    p.pcb().pcb_tcb.set(ptr::with_exposed_provenance_mut(addr));
    __aarch64_set_tcb(addr);
}
/* </CODE> */
