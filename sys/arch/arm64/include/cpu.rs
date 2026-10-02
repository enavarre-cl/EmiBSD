/* $OpenBSD: cpu.h,v 1.57 2026/09/06 20:02:12 kettenis Exp $ */
/*
 * Copyright (c) 2016 Dale Rahn <drahn@dalerahn.com>
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

//! arm64 `<machine/cpu.h>`: per-CPU state and the interrupt-mask helpers.
//!
//! Upstream: sys/arch/arm64/include/cpu.h @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M0 ports only the DAIF helpers (`restore_daif`, `enable_irq_daif`,
//! `disable_irq_daif`, `disable_irq_daif_ret`, `intr_enable`, `intr_disable`, `intr_restore`).
//! `struct cpu_info`, the `CTL_MACHDEP` names, `curcpu()` and the rest arrive with milestone M5.
//!
//! ## Deviations
//! - DAIF values are `u64`, the width of the register (`mrs`/`msr` move a full X register);
//!   C narrows them to `uint32_t` on the way out and widens them back.

use core::arch::asm;

/// `restore_daif`: writes `daif` back into `DAIF`.
///
/// # Safety
///
/// Changing the interrupt mask bits must be done by code that owns the current interrupt
/// level; use through `intr_restore` with a value from `intr_disable`.
#[inline]
pub unsafe fn restore_daif(daif: u64) {
    // SAFETY: a system register write with no memory effect; the caller owns the mask.
    unsafe { asm!("msr daif, {}", in(reg) daif, options(nomem, nostack, preserves_flags)) };
}

/// `enable_irq_daif`: unmasks IRQ and FIQ.
///
/// # Safety
///
/// As for [`restore_daif`].
#[inline]
pub unsafe fn enable_irq_daif() {
    // SAFETY: as for `restore_daif`.
    unsafe { asm!("msr daifclr, #3", options(nomem, nostack, preserves_flags)) };
}

/// `disable_irq_daif`: masks IRQ and FIQ.
#[inline]
pub fn disable_irq_daif() {
    // SAFETY: masking interrupts is always sound; it only delays their delivery.
    unsafe { asm!("msr daifset, #3", options(nomem, nostack, preserves_flags)) };
}

/// `disable_irq_daif_ret`: masks IRQ and FIQ and returns the previous `DAIF`.
#[inline]
pub fn disable_irq_daif_ret() -> u64 {
    let daif: u64;
    // SAFETY: as for `disable_irq_daif`; the read has no side effects.
    unsafe {
        asm!("mrs {}, daif", out(reg) daif, options(nomem, nostack, preserves_flags));
        asm!("msr daifset, #3", options(nomem, nostack, preserves_flags));
    }
    daif
}

/// `intr_enable`: enables interrupts.
///
/// # Safety
///
/// As for [`restore_daif`].
#[inline]
pub unsafe fn intr_enable() {
    // SAFETY: forwarded.
    unsafe { enable_irq_daif() };
}

/// `intr_disable`: disables interrupts and returns the state for [`intr_restore`].
#[inline]
pub fn intr_disable() -> u64 {
    disable_irq_daif_ret()
}

/// `intr_restore`: restores the interrupt state saved by [`intr_disable`].
///
/// # Safety
///
/// `daif` must come from [`intr_disable`] on this CPU.
#[inline]
pub unsafe fn intr_restore(daif: u64) {
    // SAFETY: forwarded.
    unsafe { restore_daif(daif) };
}
