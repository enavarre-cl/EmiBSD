/*	$OpenBSD: fpu.c,v 1.4 2025/02/18 09:18:57 kettenis Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 2022 Mark Kettenis <kettenis@openbsd.org>
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

//! arm64 `fpu.c`: the floating point and SVE state of a thread.
//!
//! Upstream: sys/arch/arm64/arm64/fpu.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M5 ports `fpu_drop`, what the context switch needs; `fpu_save`,
//! `fpu_load`, `fpu_kernel_enter` and `fpu_kernel_exit` (the `str q`/`ldr q` register
//! block moves, used once user threads run FP code) come with user mode (M6), reported.
//!
//! ## Deviations
//! - The kernel is built for `aarch64-unknown-none-softfloat`: the register moves of
//!   `fpu_save`/`fpu_load` are `.arch armv8-a+fp` inline assembly in C and will be the same
//!   here; until a thread has `PCB_FPU` set nothing calls them.

use crate::arch::arm64::include::armreg::{
    CPACR_FPEN_MASK, CPACR_FPEN_TRAP_ALL1, CPACR_ZEN_MASK, CPACR_ZEN_TRAP_ALL1, read_specialreg,
    write_specialreg,
};
use crate::sys::proc::Proc;
use crate::unported;

/// `fpu_save`: saves `p`'s FP (or SVE) registers into its pcb, if the FPU is enabled.
pub fn fpu_save(_p: &Proc) {
    // The `str q0..q31` block and sve_save: user FP state (M6).
    let _ = unported!("fpu_save: the FP register block (M6)");
}

/// `fpu_load`: loads `p`'s FP registers from its pcb and enables the FPU.
pub fn fpu_load(_p: &Proc) {
    let _ = unported!("fpu_load: the FP register block (M6)");
}

/// `fpu_drop`: disable FPU and SVE.
pub fn fpu_drop() {
    let mut cpacr = read_specialreg!("cpacr_el1");
    cpacr &= !(CPACR_FPEN_MASK | CPACR_ZEN_MASK);
    cpacr |= CPACR_FPEN_TRAP_ALL1 | CPACR_ZEN_TRAP_ALL1;
    // SAFETY: trapping FP/SVE use from EL0 and EL1 only changes what faults; the kernel is
    // built without floating point and no user thread runs yet.
    unsafe { write_specialreg!("cpacr_el1", cpacr) };

    // No ISB instruction needed here, as returning to EL0 is a context synchronization
    // event.
}

// fpu_kernel_enter, fpu_kernel_exit: kernel FP use (M6).
