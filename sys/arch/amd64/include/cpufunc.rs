/*	$OpenBSD: cpufunc.h,v 1.48 2026/07/28 15:08:06 hshoexer Exp $	*/
/*	$NetBSD: cpufunc.h,v 1.3 2003/05/08 10:27:43 fvdl Exp $	*/

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

//! amd64 `<machine/cpufunc.h>`: access to the x86 instructions the kernel needs.
//!
//! Upstream: sys/arch/amd64/include/cpufunc.h @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M0 ports the interrupt-flag helpers only (`read_rflags`,
//! `write_rflags`, `intr_enable`, `intr_disable`, `intr_restore`). Descriptor tables, control
//! registers, MSRs, TLB and cache helpers arrive with milestones M3 and M4.

use core::arch::asm;

/// `read_rflags`: the current `RFLAGS`.
#[inline]
pub fn read_rflags() -> u64 {
    let ef: u64;
    // SAFETY: `pushfq; pop` reads the flags through the stack and leaves it balanced.
    unsafe { asm!("pushfq", "pop {}", out(reg) ef, options(nomem, preserves_flags)) };
    ef
}

/// `write_rflags`: loads `RFLAGS` from `ef`.
///
/// # Safety
///
/// `ef` must be a value obtained from [`read_rflags`] on this CPU; it carries the interrupt
/// enable flag among others.
#[inline]
pub unsafe fn write_rflags(ef: u64) {
    // SAFETY: `push; popfq` loads the flags through the stack and leaves it balanced; the
    // caller vouches for the value.
    unsafe { asm!("push {}", "popfq", in(reg) ef, options(nomem)) };
}

/// `intr_enable`: enables interrupts.
///
/// # Safety
///
/// Only code that owns the current interrupt level may enable interrupts.
#[inline]
pub unsafe fn intr_enable() {
    // SAFETY: `sti` touches only the interrupt flag; the caller owns the level.
    unsafe { asm!("sti", options(nomem, nostack)) };
}

/// `intr_disable`: disables interrupts and returns the previous `RFLAGS` for
/// [`intr_restore`].
#[inline]
pub fn intr_disable() -> u64 {
    let ef = read_rflags();
    // SAFETY: masking interrupts is always sound; it only delays their delivery.
    unsafe { asm!("cli", options(nomem, nostack)) };
    ef
}

/// `intr_restore`: restores the interrupt state saved by [`intr_disable`].
///
/// # Safety
///
/// `ef` must come from [`intr_disable`] on this CPU.
#[inline]
pub unsafe fn intr_restore(ef: u64) {
    // SAFETY: forwarded.
    unsafe { write_rflags(ef) };
}

/// `rcr3`: reads `CR3`, the physical address of the current PML4 (with the PCID bits).
#[inline]
pub fn rcr3() -> u64 {
    let val: u64;
    // SAFETY: reading CR3 has no side effects; the kernel runs at CPL 0, where it is allowed.
    unsafe { asm!("mov {}, cr3", out(reg) val, options(nomem, nostack, preserves_flags)) };
    val
}
