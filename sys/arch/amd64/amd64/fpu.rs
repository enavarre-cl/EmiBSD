/* <LICENSES> */
/*-
 * Copyright (c) 1994, 1995, 1998 Charles M. Hannum.  All rights reserved.
 * Copyright (c) 1990 William Jolitz.
 * Copyright (c) 1991 The Regents of the University of California.
 * All rights reserved.
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
 */
/* </LICENSES> */

/* <CODE> */
//! The amd64 floating-point/"extended state" unit: `arch/amd64/amd64/fpu.c`.
//!
//! Upstream: sys/arch/amd64/amd64/fpu.c @ 3ce1f3f79392
//!
//! The FPU state is switched eagerly: `CPUPF_USERXSTATE` says the CPU holds curproc's user
//! state; `cpu_switchto` saves it into the pcb and loads the clean state, and the way back to
//! user mode (`intr_user_exit`, the `syscall` return) reloads it from the pcb.
//!
//! ## Deviations
//! - No XSAVE: `xsave_mask` stays 0 and every save/restore is `fxsave64`/`fxrstor64`, the
//!   C's code before `cpu_init` patches in `xsave`/`xsaveopt`/`xrstor`/`xsaves`
//!   (`replacexsave`, `codepatch.c`, not ported). So there is no AVX/AVX-512 state (`ymm`
//!   upper halves are not preserved across a context switch); `cpu_init` reports the XSAVE
//!   branch when the CPU has XSAVE.
//! - `fpu_kernel_enter`/`fpu_kernel_exit` are ported but have no caller: the kernel is built
//!   for a soft-float target and never touches the FPU.

use core::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};

use crate::arch::amd64::include::cpu::{CPUPF_USERXSTATE, curcpu};
use crate::arch::amd64::include::fpu::{
    Fxsave64, INITIAL_MXCSR_MASK, Savefpu, fldcw, fninit, fpureset, fpusavereset, fwait, fxsave,
    ldmxcsr,
};
use crate::arch::amd64::include::trap::T_XMM;
use crate::machine::cpu::curproc;
use crate::sys::siginfo::{FPE_FLTDIV, FPE_FLTINV, FPE_FLTOVF, FPE_FLTRES, FPE_FLTUND};

/// `xsave_mask`: the mask of enabled XSAVE features (0: XSAVE is not used, see the
/// deviations).
pub static XSAVE_MASK: AtomicU64 = AtomicU64::new(0);

/// `fpu_save_len`: size of the area needed to save the FPU state and other XSAVE-supported
/// state components.
pub static FPU_SAVE_LEN: AtomicUsize = AtomicUsize::new(size_of::<Fxsave64>());

/// `fpu_mxcsr_mask`: the mxcsr_mask for this host, taken from `fxsave()` on the primary CPU.
pub static FPU_MXCSR_MASK: AtomicU32 = AtomicU32::new(0);

/// `fpuinit`: init the FPU.
pub fn fpuinit() {
    fninit();
    if FPU_MXCSR_MASK.load(Ordering::Relaxed) == 0 {
        let mut fx = Savefpu::zeroed();
        fxsave(&mut fx);
        let mask = fx.fp_fxsave.fx_mxcsr_mask;
        FPU_MXCSR_MASK.store(
            if mask != 0 { mask } else { INITIAL_MXCSR_MASK },
            Ordering::Relaxed,
        );
    }
}

/// `fputrap`: record the FPU state and reinitialize it all except for the control word.
/// Returns the code to include in an SIGFPE.
///
/// Reinitializing the state allows naive SIGFPE handlers to longjmp without doing any
/// fixups.
pub fn fputrap(r#type: i32) -> i32 {
    let ci = curcpu();
    let Some(p) = curproc() else {
        return FPE_FLTINV;
    };
    let sfp = p.pcb().pcb_savefpu.get();

    crate::kassert!(ci.ci_pflags.get() & CPUPF_USERXSTATE != 0);
    ci.ci_pflags.set(ci.ci_pflags.get() & !CPUPF_USERXSTATE);
    // SAFETY: curproc's own save area; the CPU holds its state (CPUPF_USERXSTATE).
    unsafe { fpusavereset(sfp) };
    // SAFETY: only this thread reads or writes its save area.
    let fx = unsafe { (*sfp).fp_fxsave };

    let statbits = if r#type == T_XMM {
        let mxcsr = fx.fx_mxcsr;
        ldmxcsr(&(mxcsr & !0x3f));
        mxcsr
    } else {
        fninit();
        fwait();
        let cw = fx.fx_fcw;
        fldcw(&cw);
        fwait();
        u32::from(fx.fx_fsw)
    };
    x86fpflags_to_siginfo(statbits)
}

/// `x86fpflags_to_siginfo`: the `FPE_*` code of the lowest exception flag set.
fn x86fpflags_to_siginfo(flags: u32) -> i32 {
    const X86FP_SIGINFO_TABLE: [i32; 7] = [
        FPE_FLTINV, // bit 0 - invalid operation
        FPE_FLTRES, // bit 1 - denormal operand
        FPE_FLTDIV, // bit 2 - divide by zero
        FPE_FLTOVF, // bit 3 - fp overflow
        FPE_FLTUND, // bit 4 - fp underflow
        FPE_FLTRES, // bit 5 - fp precision
        FPE_FLTINV, // bit 6 - stack fault
    ];
    X86FP_SIGINFO_TABLE
        .iter()
        .enumerate()
        .find(|(i, _)| flags & (1 << i) != 0)
        .map_or(FPE_FLTINV, |(_, &code)| code) // punt if flags not set
}

/// `fpu_kernel_enter`: save curproc's FPU state, if the CPU holds it, before the kernel
/// uses the FPU.
pub fn fpu_kernel_enter() {
    let ci = curcpu();

    // splassert(IPL_NONE): no DIAGNOSTIC splassert here.

    // save curproc's FPU state if we haven't already
    if ci.ci_pflags.get() & CPUPF_USERXSTATE != 0 {
        ci.ci_pflags.set(ci.ci_pflags.get() & !CPUPF_USERXSTATE);
        if let Some(p) = curproc() {
            // SAFETY: curproc's own save area; the CPU holds its state.
            unsafe { fpusavereset(p.pcb().pcb_savefpu.get()) };
        }
    } else {
        fpureset();
    }
}

/// `fpu_kernel_exit`: make sure we don't leave anything in the registers.
pub fn fpu_kernel_exit() {
    fpureset();
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fp_flags_map_to_the_lowest_set_bit() {
        assert_eq!(x86fpflags_to_siginfo(0), FPE_FLTINV);
        assert_eq!(x86fpflags_to_siginfo(1 << 2), FPE_FLTDIV);
        assert_eq!(x86fpflags_to_siginfo((1 << 3) | (1 << 4)), FPE_FLTOVF);
        assert_eq!(x86fpflags_to_siginfo(1 << 4), FPE_FLTUND);
        assert_eq!(x86fpflags_to_siginfo(1 << 6), FPE_FLTINV);
    }
}
/* </TESTS> */
