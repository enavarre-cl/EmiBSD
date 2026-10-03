/* $OpenBSD: armreg.h,v 1.45 2026/05/04 20:43:42 kettenis Exp $ */
/* <LICENSES> */
/*-
 * Copyright (c) 2013, 2014 Andrew Turner
 * Copyright (c) 2015 The FreeBSD Foundation
 * All rights reserved.
 *
 * This software was developed by Andrew Turner under
 * sponsorship from the FreeBSD Foundation.
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
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 * $FreeBSD: head/sys/arm64/include/armreg.h 309248 2016-11-28 14:24:07Z andrew $
 */
/* </LICENSES> */

//! arm64 `<machine/armreg.h>`: the system registers' bit definitions.
//!
//! Upstream: sys/arch/arm64/include/armreg.h @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M4 ports `INSN_SIZE`, `READ_SPECIALREG`/`WRITE_SPECIALREG`, the
//! `ESR_ELx` exception classes and syndrome bits the kernel trap handler reads, the `PSR`
//! bits and the `MDSCR_EL1` debug bits. The ID register fields (`ID_AA64*`), `SCTLR`, `TCR`,
//! `CPACR`, the cache and GIC system registers come with CPU identification (M4-b) and the
//! subsystems that use them.
//!
//! ## Deviations
//! - `READ_SPECIALREG`/`WRITE_SPECIALREG` are `macro_rules!` taking the register name as a
//!   string literal: `asm!` needs the name at compile time, as the C's `__STRING(reg)` does.

/// `INSN_SIZE`: every A64 instruction is four bytes.
pub const INSN_SIZE: usize = 4;

/// `READ_SPECIALREG(reg)`: `mrs` of the named system register.
macro_rules! read_specialreg {
    ($reg:literal) => {{
        let val: u64;
        // SAFETY: a system register read with no side effects.
        unsafe {
            ::core::arch::asm!(
                concat!("mrs {}, ", $reg),
                out(reg) val,
                options(nomem, nostack, preserves_flags)
            )
        };
        val
    }};
}
pub(crate) use read_specialreg;

/// `WRITE_SPECIALREG(reg, val)`: `msr` of the named system register.
///
/// # Safety
///
/// The expansion is an `unsafe` block the caller must wrap: writing a system register changes
/// the machine's state; the caller guarantees that is sound here.
macro_rules! write_specialreg {
    ($reg:literal, $val:expr) => {{
        let val: u64 = $val;
        ::core::arch::asm!(
            concat!("msr ", $reg, ", {}"),
            in(reg) val,
            options(nomem, nostack, preserves_flags)
        )
    }};
}
pub(crate) use write_specialreg;

/* CPACR_EL1 */

/// `CPACR_ZEN_MASK`.
pub const CPACR_ZEN_MASK: u64 = 0x3 << 16;
/// `CPACR_ZEN_TRAP_ALL1`: traps from EL0 and EL1.
pub const CPACR_ZEN_TRAP_ALL1: u64 = 0x0 << 16;
/// `CPACR_ZEN_TRAP_EL0`: traps from EL0.
pub const CPACR_ZEN_TRAP_EL0: u64 = 0x1 << 16;
/// `CPACR_ZEN_TRAP_ALL2`: traps from EL0 and EL1.
pub const CPACR_ZEN_TRAP_ALL2: u64 = 0x2 << 16;
/// `CPACR_ZEN_TRAP_NONE`: no traps.
pub const CPACR_ZEN_TRAP_NONE: u64 = 0x3 << 16;
/// `CPACR_FPEN_MASK`.
pub const CPACR_FPEN_MASK: u64 = 0x3 << 20;
/// `CPACR_FPEN_TRAP_ALL1`: traps from EL0 and EL1.
pub const CPACR_FPEN_TRAP_ALL1: u64 = 0x0 << 20;
/// `CPACR_FPEN_TRAP_EL0`: traps from EL0.
pub const CPACR_FPEN_TRAP_EL0: u64 = 0x1 << 20;
/// `CPACR_FPEN_TRAP_ALL2`: traps from EL0 and EL1.
pub const CPACR_FPEN_TRAP_ALL2: u64 = 0x2 << 20;
/// `CPACR_FPEN_TRAP_NONE`: no traps.
pub const CPACR_FPEN_TRAP_NONE: u64 = 0x3 << 20;
/// `CPACR_TTA`.
pub const CPACR_TTA: u64 = 0x1 << 28;

/* CNTKCTL_EL1 - Counter-timer Kernel Control Register */

/// `CNTKCTL_EL0VCTEN`: allow EL0 virtual counter access.
pub const CNTKCTL_EL0VCTEN: u64 = 1 << 1;

/* CNTV_CTL_EL0 */

/// `CNTV_CTL_ENABLE`.
pub const CNTV_CTL_ENABLE: u32 = 1 << 0;
/// `CNTV_CTL_IMASK`.
pub const CNTV_CTL_IMASK: u32 = 1 << 1;
/// `CNTV_CTL_ISTATUS`.
pub const CNTV_CTL_ISTATUS: u32 = 1 << 2;

/* CurrentEL - Current Exception Level */

/// `CURRENTEL_EL_SHIFT`.
pub const CURRENTEL_EL_SHIFT: u64 = 2;
/// `CURRENTEL_EL_MASK`.
pub const CURRENTEL_EL_MASK: u64 = 0x3 << CURRENTEL_EL_SHIFT;
/// `CURRENTEL_EL_EL0`.
pub const CURRENTEL_EL_EL0: u64 = 0x0 << CURRENTEL_EL_SHIFT;
/// `CURRENTEL_EL_EL1`.
pub const CURRENTEL_EL_EL1: u64 = 0x1 << CURRENTEL_EL_SHIFT;
/// `CURRENTEL_EL_EL2`.
pub const CURRENTEL_EL_EL2: u64 = 0x2 << CURRENTEL_EL_SHIFT;
/// `CURRENTEL_EL_EL3`.
pub const CURRENTEL_EL_EL3: u64 = 0x3 << CURRENTEL_EL_SHIFT;

// MPIDR_EL1 - Multiprocessor Affinity Register
/// `MPIDR_AFF3`.
pub const MPIDR_AFF3: u64 = 0xFF << 32;
/// `MPIDR_AFF2`.
pub const MPIDR_AFF2: u64 = 0xFF << 16;
/// `MPIDR_AFF1`.
pub const MPIDR_AFF1: u64 = 0xFF << 8;
/// `MPIDR_AFF0`.
pub const MPIDR_AFF0: u64 = 0xFF;
/// `MPIDR_AFF`: the four affinity levels, a CPU's address in the device tree.
pub const MPIDR_AFF: u64 = MPIDR_AFF3 | MPIDR_AFF2 | MPIDR_AFF1 | MPIDR_AFF0;

/// `ESR_ELx_ISS_MASK`: the instruction specific syndrome.
pub const ESR_ELX_ISS_MASK: u64 = 0x00ff_ffff;
/// `ISS_INSN_FnV`.
pub const ISS_INSN_FNV: u64 = 0x01 << 10;
/// `ISS_INSN_EA`.
pub const ISS_INSN_EA: u64 = 0x01 << 9;
/// `ISS_INSN_S1PTW`.
pub const ISS_INSN_S1PTW: u64 = 0x01 << 7;
/// `ISS_INSN_IFSC_MASK`.
pub const ISS_INSN_IFSC_MASK: u64 = 0x1f;
/// `ISS_DATA_ISV`.
pub const ISS_DATA_ISV: u64 = 0x01 << 24;
/// `ISS_DATA_SAS_MASK`.
pub const ISS_DATA_SAS_MASK: u64 = 0x03 << 22;
/// `ISS_DATA_SSE`.
pub const ISS_DATA_SSE: u64 = 0x01 << 21;
/// `ISS_DATA_SRT_MASK`.
pub const ISS_DATA_SRT_MASK: u64 = 0x1f << 16;
/// `ISS_DATA_SF`.
pub const ISS_DATA_SF: u64 = 0x01 << 15;
/// `ISS_DATA_AR`.
pub const ISS_DATA_AR: u64 = 0x01 << 14;
/// `ISS_DATA_FnV`.
pub const ISS_DATA_FNV: u64 = 0x01 << 10;
/// `ISS_DATA_EA`.
pub const ISS_DATA_EA: u64 = 0x01 << 9;
/// `ISS_DATA_CM`: a cache maintenance instruction.
pub const ISS_DATA_CM: u64 = 0x01 << 8;
/// `ISS_DATA_S1PTW`.
pub const ISS_DATA_S1PTW: u64 = 0x01 << 7;
/// `ISS_DATA_WnR`: the access was a write.
pub const ISS_DATA_WNR: u64 = 0x01 << 6;
/// `ISS_DATA_DFSC_MASK`: the data fault status code.
pub const ISS_DATA_DFSC_MASK: u64 = 0x3f;
/// `ISS_DATA_DFSC_TF_L0`: translation fault, level 0 (`_L1` to `_L3` follow).
pub const ISS_DATA_DFSC_TF_L0: u64 = 0x04;
/// `ISS_DATA_DFSC_TF_L3`.
pub const ISS_DATA_DFSC_TF_L3: u64 = 0x07;
/// `ISS_DATA_DFSC_AFF_L1`: access flag fault, level 1.
pub const ISS_DATA_DFSC_AFF_L1: u64 = 0x09;
/// `ISS_DATA_DFSC_PF_L1`: permission fault, level 1.
pub const ISS_DATA_DFSC_PF_L1: u64 = 0x0d;
/// `ISS_DATA_DFSC_PF_L3`.
pub const ISS_DATA_DFSC_PF_L3: u64 = 0x0f;
/// `ISS_DATA_DFSC_ALIGN`: alignment fault.
pub const ISS_DATA_DFSC_ALIGN: u64 = 0x21;

/// `ISS_BRK_COMMENT_MASK`: the immediate of a `brk` instruction.
pub const ISS_BRK_COMMENT_MASK: u64 = 0xffff;

/// `ESR_ELx_EC_SHIFT`: the exception class field.
pub const ESR_ELX_EC_SHIFT: u32 = 26;
/// `ESR_ELx_EC_MASK`.
pub const ESR_ELX_EC_MASK: u64 = 0x3f << 26;

/// `ESR_ELx_EXCEPTION(esr)`: the exception class of a syndrome.
pub const fn esr_elx_exception(esr: u64) -> u32 {
    ((esr & ESR_ELX_EC_MASK) >> ESR_ELX_EC_SHIFT) as u32
}

/// `EXCP_UNKNOWN`: Unkwn exception.
pub const EXCP_UNKNOWN: u32 = 0x00;
/// `EXCP_FP_SIMD`: FP/SIMD trap.
pub const EXCP_FP_SIMD: u32 = 0x07;
/// `EXCP_BRANCH_TGT`: Branch target exception.
pub const EXCP_BRANCH_TGT: u32 = 0x0d;
/// `EXCP_ILL_STATE`: Illegal execution state.
pub const EXCP_ILL_STATE: u32 = 0x0e;
/// `EXCP_SVC`: SVC trap.
pub const EXCP_SVC: u32 = 0x15;
/// `EXCP_MSR`: MSR/MRS trap.
pub const EXCP_MSR: u32 = 0x18;
/// `EXCP_SVE`: SVE trap.
pub const EXCP_SVE: u32 = 0x19;
/// `EXCP_FPAC`: Faulting PAC trap.
pub const EXCP_FPAC: u32 = 0x1c;
/// `EXCP_INSN_ABORT_L`: Instruction abort, from lower EL.
pub const EXCP_INSN_ABORT_L: u32 = 0x20;
/// `EXCP_INSN_ABORT`: Instruction abort, from same EL.
pub const EXCP_INSN_ABORT: u32 = 0x21;
/// `EXCP_PC_ALIGN`: PC alignment fault.
pub const EXCP_PC_ALIGN: u32 = 0x22;
/// `EXCP_DATA_ABORT_L`: Data abort, from lower EL.
pub const EXCP_DATA_ABORT_L: u32 = 0x24;
/// `EXCP_DATA_ABORT`: Data abort, from same EL.
pub const EXCP_DATA_ABORT: u32 = 0x25;
/// `EXCP_SP_ALIGN`: SP alignment fault.
pub const EXCP_SP_ALIGN: u32 = 0x26;
/// `EXCP_TRAP_FP`: Trapped FP exception.
pub const EXCP_TRAP_FP: u32 = 0x2c;
/// `EXCP_SERROR`: SError interrupt.
pub const EXCP_SERROR: u32 = 0x2f;
/// `EXCP_SOFTSTP_EL0`: Software Step, from lower EL.
pub const EXCP_SOFTSTP_EL0: u32 = 0x32;
/// `EXCP_SOFTSTP_EL1`: Software Step, from same EL.
pub const EXCP_SOFTSTP_EL1: u32 = 0x33;
/// `EXCP_WATCHPT_EL1`: Watchpoint, from same EL.
pub const EXCP_WATCHPT_EL1: u32 = 0x35;
/// `EXCP_BRK`: Breakpoint.
pub const EXCP_BRK: u32 = 0x3c;

/// `DBG_MDSCR_SS`: software step enable.
pub const DBG_MDSCR_SS: u64 = 0x1;
/// `DBG_MDSCR_KDE`: kernel debug enable.
pub const DBG_MDSCR_KDE: u64 = 0x1 << 13;
/// `DBG_MDSCR_MDE`: monitor debug events enable.
pub const DBG_MDSCR_MDE: u64 = 0x1 << 15;

/// `PSR_M_EL0t`.
pub const PSR_M_EL0T: u64 = 0x0000_0000;
/// `PSR_M_EL1t`.
pub const PSR_M_EL1T: u64 = 0x0000_0004;
/// `PSR_M_EL1h`.
pub const PSR_M_EL1H: u64 = 0x0000_0005;
/// `PSR_M_EL2t`.
pub const PSR_M_EL2T: u64 = 0x0000_0008;
/// `PSR_M_EL2h`.
pub const PSR_M_EL2H: u64 = 0x0000_0009;
/// `PSR_M_MASK`.
pub const PSR_M_MASK: u64 = 0x0000_001f;
/// `PSR_F`: FIQ masked.
pub const PSR_F: u64 = 0x0000_0040;
/// `PSR_I`: IRQ masked.
pub const PSR_I: u64 = 0x0000_0080;
/// `PSR_M_EL0t`: the EL0 mode (`SPSR_EL1.M`).
#[allow(non_upper_case_globals)]
pub const PSR_M_EL0t: u64 = 0x0000_0000;
/// `PSR_DIT`: data independent timing.
pub const PSR_DIT: u64 = 0x0100_0000;
/// `PSR_A`: SError masked.
pub const PSR_A: u64 = 0x0000_0100;
/// `PSR_D`: debug exceptions masked.
pub const PSR_D: u64 = 0x0000_0200;
/// `PSR_SS`: software step.
pub const PSR_SS: u64 = 0x0020_0000;
/// `PSR_V`.
pub const PSR_V: u64 = 0x1000_0000;
/// `PSR_C`.
pub const PSR_C: u64 = 0x2000_0000;
/// `PSR_Z`.
pub const PSR_Z: u64 = 0x4000_0000;
/// `PSR_N`.
pub const PSR_N: u64 = 0x8000_0000;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exception_class_extraction() {
        assert_eq!(esr_elx_exception(0x9600_0045), EXCP_DATA_ABORT);
        assert_eq!(esr_elx_exception(0xf200_0000), EXCP_BRK);
        assert_eq!(esr_elx_exception(0), EXCP_UNKNOWN);
    }

    /// The C header's values, read from `$OPENBSD_SRC` (`just test-ref`).
    #[test]
    #[ignore]
    fn matches_reference() {
        let want = [
            ("INSN_SIZE", INSN_SIZE as i64),
            ("ISS_DATA_CM", ISS_DATA_CM as i64),
            ("ISS_DATA_WnR", ISS_DATA_WNR as i64),
            ("ISS_DATA_DFSC_MASK", ISS_DATA_DFSC_MASK as i64),
            ("ISS_BRK_COMMENT_MASK", ISS_BRK_COMMENT_MASK as i64),
            ("ESR_ELx_EC_SHIFT", ESR_ELX_EC_SHIFT as i64),
            ("EXCP_INSN_ABORT", EXCP_INSN_ABORT as i64),
            ("EXCP_DATA_ABORT", EXCP_DATA_ABORT as i64),
            ("EXCP_BRK", EXCP_BRK as i64),
            ("EXCP_WATCHPT_EL1", EXCP_WATCHPT_EL1 as i64),
            ("EXCP_SOFTSTP_EL1", EXCP_SOFTSTP_EL1 as i64),
            ("PSR_D", PSR_D as i64),
            ("PSR_SS", PSR_SS as i64),
            ("DBG_MDSCR_KDE", DBG_MDSCR_KDE as i64),
        ];
        let defs = crate::reftest::defines("sys/arch/arm64/include/armreg.h");
        for (name, value) in want {
            assert_eq!(crate::reftest::int(&defs, name), Some(value), "{name}");
        }
    }
}
