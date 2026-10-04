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
//! Status: `wip`. Milestone M5 ports `fpu_drop`, what the context switch needs; with
//! `kern_sig.c` (whose trampoline saves the `q` registers, so the first signal delivered to
//! a process traps on the FPU) come `fpu_save` and `fpu_load`, the `str q`/`ldr q` register
//! block moves; with efi(4) `fpu_kernel_enter`/`fpu_kernel_exit` (the softfloat kernel never
//! uses the FPU itself, but the UEFI runtime services may). `sve_save` and `sve_load` are
//! reported.
//!
//! ## Deviations
//! - The kernel is built for `aarch64-unknown-none-softfloat`: the register moves are
//!   inline assembly bracketed by `.arch_extension fp`/`nofp`, as the C's `.arch
//!   armv8-a+fp`, and `fpsr`/`fpcr` are reached by their encodings (`s3_3_c4_c4_1`,
//!   `s3_3_c4_c4_0`), as the C's `#define`s do.
//! - SVE is never enabled (`CPACR_ZEN` stays trapped), so `sve_save` is reached only through
//!   a state this kernel cannot create; it is reported, and so is `sve_load` (the
//!   `EXCP_SVE` trap).

use core::arch::asm;
use core::ptr;

use crate::arch::arm64::include::armreg::{
    CPACR_FPEN_MASK, CPACR_FPEN_TRAP_ALL1, CPACR_FPEN_TRAP_EL0, CPACR_FPEN_TRAP_NONE,
    CPACR_ZEN_MASK, CPACR_ZEN_TRAP_ALL1, CPACR_ZEN_TRAP_NONE, read_specialreg, write_specialreg,
};
use crate::arch::arm64::include::pcb::PCB_FPU;
use crate::arch::arm64::include::reg::Fpreg;
use crate::kassert;
use crate::machine::cpu::curproc;
use crate::sys::proc::Proc;
use crate::unported;

/// `fpu_save`: saves `p`'s FP (or SVE) registers into its pcb, if the FPU is enabled.
pub fn fpu_save(p: &Proc) {
    let pcb = p.pcb();
    let fp = pcb.pcb_fpstate.get();

    let cpacr = read_specialreg!("cpacr_el1");
    if cpacr & CPACR_FPEN_MASK == CPACR_FPEN_TRAP_ALL1 {
        return;
    }

    kassert!(cpacr & CPACR_FPEN_MASK == CPACR_FPEN_TRAP_NONE);

    if cpacr & CPACR_ZEN_MASK == CPACR_ZEN_TRAP_NONE {
        sve_save(p);
        return;
    }

    // SAFETY: the FPU is enabled (checked above); `fp_reg` is 32 16-byte slots of the
    // thread's own pcb, which only this thread (or the context switch away from it) writes.
    // The stores read the vector registers and write only that memory.
    unsafe {
        asm!(
            ".arch_extension fp",
            "str q0, [{fp}, #0]",
            "str q1, [{fp}, #16]",
            "str q2, [{fp}, #32]",
            "str q3, [{fp}, #48]",
            "str q4, [{fp}, #64]",
            "str q5, [{fp}, #80]",
            "str q6, [{fp}, #96]",
            "str q7, [{fp}, #112]",
            "str q8, [{fp}, #128]",
            "str q9, [{fp}, #144]",
            "str q10, [{fp}, #160]",
            "str q11, [{fp}, #176]",
            "str q12, [{fp}, #192]",
            "str q13, [{fp}, #208]",
            "str q14, [{fp}, #224]",
            "str q15, [{fp}, #240]",
            "str q16, [{fp}, #256]",
            "str q17, [{fp}, #272]",
            "str q18, [{fp}, #288]",
            "str q19, [{fp}, #304]",
            "str q20, [{fp}, #320]",
            "str q21, [{fp}, #336]",
            "str q22, [{fp}, #352]",
            "str q23, [{fp}, #368]",
            "str q24, [{fp}, #384]",
            "str q25, [{fp}, #400]",
            "str q26, [{fp}, #416]",
            "str q27, [{fp}, #432]",
            "str q28, [{fp}, #448]",
            "str q29, [{fp}, #464]",
            "str q30, [{fp}, #480]",
            "str q31, [{fp}, #496]",
            ".arch_extension nofp",
            fp = in(reg) ptr::addr_of_mut!((*fp).fp_reg),
            options(nostack, preserves_flags)
        );
    }
    let fpsr = read_specialreg!("s3_3_c4_c4_1") as u32;
    let fpcr = read_specialreg!("s3_3_c4_c4_0") as u32;
    // SAFETY: as above.
    unsafe {
        (*fp).fp_sr = fpsr;
        (*fp).fp_cr = fpcr;
    }
}

/// `fpu_load`: loads `p`'s FP registers from its pcb and enables the FPU (the
/// `EXCP_FP_SIMD`/`EXCP_TRAP_FP` trap of a thread's first FP instruction since its last
/// switch).
pub fn fpu_load(p: &Proc) {
    let pcb = p.pcb();
    let fp = pcb.pcb_fpstate.get();

    let mut cpacr = read_specialreg!("cpacr_el1");
    kassert!(cpacr & CPACR_FPEN_MASK == CPACR_FPEN_TRAP_ALL1);
    kassert!(cpacr & CPACR_ZEN_MASK == CPACR_ZEN_TRAP_ALL1);

    if pcb.pcb_flags.get() & PCB_FPU == 0 {
        // SAFETY: the thread's own FPU state, which nothing else touches while it runs.
        unsafe { fp.write(Fpreg::zeroed()) };
        pcb.pcb_flags.set(pcb.pcb_flags.get() | PCB_FPU);
    }

    // Enable FPU.
    cpacr &= !CPACR_FPEN_MASK;
    cpacr |= CPACR_FPEN_TRAP_NONE;
    // SAFETY: lets this CPU (EL0 and EL1) use the FPU; the kernel itself is built without
    // floating point, so only the thread's own user code and the moves below use it.
    unsafe { write_specialreg!("cpacr_el1", cpacr) };
    // SAFETY: an instruction synchronisation barrier, so the moves below see the enabled
    // FPU; no memory or flags are touched.
    unsafe { asm!("isb", options(nomem, nostack, preserves_flags)) };

    // SAFETY: the FPU is now enabled; `fp_reg` is the thread's own 32 16-byte slots. The
    // loads overwrite the vector registers, which no kernel code uses (softfloat target), so
    // nothing the compiler keeps is clobbered.
    unsafe {
        asm!(
            ".arch_extension fp",
            "ldr q0, [{fp}, #0]",
            "ldr q1, [{fp}, #16]",
            "ldr q2, [{fp}, #32]",
            "ldr q3, [{fp}, #48]",
            "ldr q4, [{fp}, #64]",
            "ldr q5, [{fp}, #80]",
            "ldr q6, [{fp}, #96]",
            "ldr q7, [{fp}, #112]",
            "ldr q8, [{fp}, #128]",
            "ldr q9, [{fp}, #144]",
            "ldr q10, [{fp}, #160]",
            "ldr q11, [{fp}, #176]",
            "ldr q12, [{fp}, #192]",
            "ldr q13, [{fp}, #208]",
            "ldr q14, [{fp}, #224]",
            "ldr q15, [{fp}, #240]",
            "ldr q16, [{fp}, #256]",
            "ldr q17, [{fp}, #272]",
            "ldr q18, [{fp}, #288]",
            "ldr q19, [{fp}, #304]",
            "ldr q20, [{fp}, #320]",
            "ldr q21, [{fp}, #336]",
            "ldr q22, [{fp}, #352]",
            "ldr q23, [{fp}, #368]",
            "ldr q24, [{fp}, #384]",
            "ldr q25, [{fp}, #400]",
            "ldr q26, [{fp}, #416]",
            "ldr q27, [{fp}, #432]",
            "ldr q28, [{fp}, #448]",
            "ldr q29, [{fp}, #464]",
            "ldr q30, [{fp}, #480]",
            "ldr q31, [{fp}, #496]",
            ".arch_extension nofp",
            fp = in(reg) ptr::addr_of!((*fp).fp_reg),
            options(nostack, readonly, preserves_flags)
        );
        write_specialreg!("s3_3_c4_c4_1", u64::from((*fp).fp_sr)); // fpsr
        write_specialreg!("s3_3_c4_c4_0", u64::from((*fp).fp_cr)); // fpcr
    }
}

/// `fpu_drop`: disable FPU and SVE.
pub fn fpu_drop() {
    let mut cpacr = read_specialreg!("cpacr_el1");
    cpacr &= !(CPACR_FPEN_MASK | CPACR_ZEN_MASK);
    cpacr |= CPACR_FPEN_TRAP_ALL1 | CPACR_ZEN_TRAP_ALL1;
    // SAFETY: trapping FP/SVE use from EL0 and EL1 only changes what faults; the kernel is
    // built without floating point.
    unsafe { write_specialreg!("cpacr_el1", cpacr) };

    // No ISB instruction needed here, as returning to EL0 is a context synchronization
    // event.
}

/// `fpu_kernel_enter`: lets EL1 use the FPU (for the UEFI runtime services, which may),
/// saving the current thread's FP state first.
pub fn fpu_kernel_enter() {
    if let Some(p) = curproc()
        && p.pcb().pcb_flags.get() & PCB_FPU != 0
    {
        fpu_save(p);
    }

    // Enable FPU (kernel only).
    let mut cpacr = read_specialreg!("cpacr_el1");
    cpacr &= !(CPACR_FPEN_MASK | CPACR_ZEN_MASK);
    cpacr |= CPACR_FPEN_TRAP_EL0 | CPACR_ZEN_TRAP_ALL1;
    // SAFETY: lets EL1 use the FP registers (EL0 still traps); this kernel's own code never
    // does, so only the firmware called between this and `fpu_kernel_exit` sees it.
    unsafe {
        write_specialreg!("cpacr_el1", cpacr);
        asm!("isb", options(nomem, nostack, preserves_flags));
    }
}

/// `fpu_kernel_exit`: traps FP use from EL0 and EL1 again.
pub fn fpu_kernel_exit() {
    // Disable FPU.
    let mut cpacr = read_specialreg!("cpacr_el1");
    cpacr &= !(CPACR_FPEN_MASK | CPACR_ZEN_MASK);
    cpacr |= CPACR_FPEN_TRAP_ALL1 | CPACR_ZEN_TRAP_ALL1;
    // SAFETY: as in `fpu_drop`.
    unsafe { write_specialreg!("cpacr_el1", cpacr) };

    // No ISB instruction needed here, as returning to EL0 is a context synchronization
    // event.
}

/// `sve_save`: the SVE `z`/`p`/`ffr` register block (see the module's deviations).
pub fn sve_save(_p: &Proc) {
    let _ = unported!("sve_save: the SVE register block");
}

/// `sve_load`: the `EXCP_SVE` trap's handler (see the module's deviations).
pub fn sve_load(_p: &Proc) {
    let _ = unported!("sve_load: the SVE register block");
}
