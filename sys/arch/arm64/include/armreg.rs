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

/* <CODE> */
//! arm64 `<machine/armreg.h>`: the system registers' bit definitions.
//!
//! Upstream: sys/arch/arm64/include/armreg.h @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M4 ports `INSN_SIZE`, `READ_SPECIALREG`/`WRITE_SPECIALREG`, the
//! `ESR_ELx` exception classes and syndrome bits the kernel trap handler reads, the `PSR`
//! bits and the `MDSCR_EL1` debug bits. M11a (`cpu.c`) adds the cache registers (`CCSIDR`,
//! `CLIDR`, `CSSELR`, `CTR_IL1P`), every `ID_AA64*` field, `SCTLR_EL1`, `TCR_A1` and
//! `DBG_MDSCR_TDCC`. The GIC system registers come with the drivers that use them: M16f
//! (`agintc.c`) adds the GICv3 CPU interface's `ICC_*_EL1` fields.
//!
//! ## Deviations
//! - `READ_SPECIALREG`/`WRITE_SPECIALREG` are `macro_rules!` taking the register name as a
//!   string literal: `asm!` needs the name at compile time, as the C's `__STRING(reg)` does.
//! - `CNTKCTL_EVNTEN`, `CNTKCTL_EVNTDIR`, `CNTKCTL_EVNTI_SHIFT` and `CNTKCTL_EVNTI_MASK` are
//!   not in the C header: they are `CNTKCTL_EL1`'s event stream fields (the ARM ARM puts them
//!   where the header's `CNTHCTL_EVNT*` are for `CNTHCTL_EL2`), for the event stream
//!   `agtimer.rs` enables (its deviations say why).

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

/* CCSIDR_EL1 - Current Cache Size ID Register */

/// `CCSIDR_SETS_MASK`.
pub const CCSIDR_SETS_MASK: u64 = 0x0fff_e000;
/// `CCSIDR_SETS_SHIFT`.
pub const CCSIDR_SETS_SHIFT: u32 = 13;

/// `CCSIDR_SETS(reg)`: the number of sets (old 32-bit format).
pub const fn ccsidr_sets(reg: u64) -> u32 {
    (((reg & CCSIDR_SETS_MASK) >> CCSIDR_SETS_SHIFT) + 1) as u32
}

/// `CCSIDR_WAYS_MASK`.
pub const CCSIDR_WAYS_MASK: u64 = 0x0000_1ff8;
/// `CCSIDR_WAYS_SHIFT`.
pub const CCSIDR_WAYS_SHIFT: u32 = 3;

/// `CCSIDR_WAYS(reg)`: the associativity (old 32-bit format).
pub const fn ccsidr_ways(reg: u64) -> u32 {
    (((reg & CCSIDR_WAYS_MASK) >> CCSIDR_WAYS_SHIFT) + 1) as u32
}

/// `CCSIDR_LINE_MASK`.
pub const CCSIDR_LINE_MASK: u64 = 0x0000_0007;

/// `CCSIDR_LINE_SIZE(reg)`: the line size in bytes (old 32-bit format).
pub const fn ccsidr_line_size(reg: u64) -> u32 {
    1 << ((reg & CCSIDR_LINE_MASK) + 4)
}

/// `CCSIDR_CCIDX_SETS_MASK`.
pub const CCSIDR_CCIDX_SETS_MASK: u64 = 0x00ff_ffff_0000_0000;
/// `CCSIDR_CCIDX_SETS_SHIFT`.
pub const CCSIDR_CCIDX_SETS_SHIFT: u32 = 32;

/// `CCSIDR_CCIDX_SETS(reg)`: the number of sets (64-bit format, `FEAT_CCIDX`).
pub const fn ccsidr_ccidx_sets(reg: u64) -> u32 {
    (((reg & CCSIDR_CCIDX_SETS_MASK) >> CCSIDR_CCIDX_SETS_SHIFT) + 1) as u32
}

/// `CCSIDR_CCIDX_WAYS_MASK`.
pub const CCSIDR_CCIDX_WAYS_MASK: u64 = 0x0000_0000_00ff_fff8;
/// `CCSIDR_CCIDX_WAYS_SHIFT`.
pub const CCSIDR_CCIDX_WAYS_SHIFT: u32 = 3;

/// `CCSIDR_CCIDX_WAYS(reg)`: the associativity (64-bit format).
pub const fn ccsidr_ccidx_ways(reg: u64) -> u32 {
    (((reg & CCSIDR_CCIDX_WAYS_MASK) >> CCSIDR_CCIDX_WAYS_SHIFT) + 1) as u32
}

/// `CCSIDR_CCIDX_LINE_MASK`.
pub const CCSIDR_CCIDX_LINE_MASK: u64 = 0x0000_0000_0000_0007;

/// `CCSIDR_CCIDX_LINE_SIZE(reg)`: the line size in bytes (64-bit format).
pub const fn ccsidr_ccidx_line_size(reg: u64) -> u32 {
    1 << ((reg & CCSIDR_CCIDX_LINE_MASK) + 4)
}

/* CLIDR_EL1 - Cache Level ID Register */

/// `CLIDR_CTYPE_MASK`.
pub const CLIDR_CTYPE_MASK: u64 = 0x7;
/// `CLIDR_CTYPE_INSN`.
pub const CLIDR_CTYPE_INSN: u64 = 0x1;
/// `CLIDR_CTYPE_DATA`.
pub const CLIDR_CTYPE_DATA: u64 = 0x2;
/// `CLIDR_CTYPE_UNIFIED`.
pub const CLIDR_CTYPE_UNIFIED: u64 = 0x4;

/* CSSELR_EL1 - Cache Size Selection Register */

/// `CSSELR_IND`.
pub const CSSELR_IND: u64 = 1 << 0;
/// `CSSELR_LEVEL_SHIFT`.
pub const CSSELR_LEVEL_SHIFT: u32 = 1;

/* CTR_EL0 - Cache Type Register */

/// `CTR_IL1P_SHIFT`.
pub const CTR_IL1P_SHIFT: u32 = 14;
/// `CTR_IL1P_MASK`.
pub const CTR_IL1P_MASK: u64 = 0x3 << CTR_IL1P_SHIFT;
/// `CTR_IL1P_AIVIVT`.
pub const CTR_IL1P_AIVIVT: u64 = 0x1 << CTR_IL1P_SHIFT;
/// `CTR_IL1P_VIPT`.
pub const CTR_IL1P_VIPT: u64 = 0x2 << CTR_IL1P_SHIFT;
/// `CTR_IL1P_PIPT`.
pub const CTR_IL1P_PIPT: u64 = 0x3 << CTR_IL1P_SHIFT;

/* TCR_EL1 */

/// `TCR_A1`: the ASID comes from `TTBR1_EL1`.
pub const TCR_A1: u64 = 1 << 22;

/// `TCR_T0SZ_SHIFT`.
pub const TCR_T0SZ_SHIFT: u32 = 0;

/// `TCR_T0SZ(x)`: the size offset of the `TTBR0_EL1` region (64 minus its address bits).
pub const fn tcr_t0sz(x: u64) -> u64 {
    x << TCR_T0SZ_SHIFT
}

/// `TCR_AS`: 16-bit ASIDs.
pub const TCR_AS: u64 = 1 << 36;
/// `TCR_IPS_SHIFT`: the intermediate physical address size (`start_mmu` copies
/// `ID_AA64MMFR0_EL1.PARange` there).
pub const TCR_IPS_SHIFT: u32 = 32;
/// `TCR_TG1_4K`: 4 KiB granule for `TTBR1_EL1`.
pub const TCR_TG1_4K: u64 = 2 << 30;
/// `TCR_SH1_IS`: inner shareable `TTBR1_EL1` walks.
pub const TCR_SH1_IS: u64 = 0x3 << 28;
/// `TCR_ORGN1_WBWA`: outer write-back write-allocate `TTBR1_EL1` walks.
pub const TCR_ORGN1_WBWA: u64 = 0x1 << 26;
/// `TCR_IRGN1_WBWA`: inner write-back write-allocate `TTBR1_EL1` walks.
pub const TCR_IRGN1_WBWA: u64 = 0x1 << 24;
/// `TCR_TG0_4K`: 4 KiB granule for `TTBR0_EL1`.
pub const TCR_TG0_4K: u64 = 0 << 14;
/// `TCR_SH0_IS`: inner shareable `TTBR0_EL1` walks.
pub const TCR_SH0_IS: u64 = 0x3 << 12;
/// `TCR_ORGN0_WBWA`: outer write-back write-allocate `TTBR0_EL1` walks.
pub const TCR_ORGN0_WBWA: u64 = 0x1 << 10;
/// `TCR_IRGN0_WBWA`: inner write-back write-allocate `TTBR0_EL1` walks.
pub const TCR_IRGN0_WBWA: u64 = 0x1 << 8;
/// `TCR_CACHE_ATTRS`: cacheable walks for both halves.
pub const TCR_CACHE_ATTRS: u64 = TCR_IRGN0_WBWA | TCR_IRGN1_WBWA | TCR_ORGN0_WBWA | TCR_ORGN1_WBWA;
/// `TCR_SMP_ATTRS`: inner shareable walks for both halves.
pub const TCR_SMP_ATTRS: u64 = TCR_SH0_IS | TCR_SH1_IS;
/// `TCR_T1SZ(x)`: the size offset of the `TTBR1_EL1` region (64 minus its address bits).
pub const fn tcr_t1sz(x: u64) -> u64 {
    x << 16
}

/* CNTHCTL_EL2 (locore.S's drop_to_el1) */

/// `CNTHCTL_EL1PCEN`: EL0/EL1 may use the physical timer.
pub const CNTHCTL_EL1PCEN: u64 = 1 << 1;
/// `CNTHCTL_EL1PCTEN`: EL0/EL1 may read the physical counter.
pub const CNTHCTL_EL1PCTEN: u64 = 1 << 0;

/* ICC_CTLR_EL1 */

/// `ICC_CTLR_EL1_EOIMODE`: EOI only drops the priority; a write to `ICC_DIR_EL1` deactivates.
pub const ICC_CTLR_EL1_EOIMODE: u64 = 1 << 1;
/// `ICC_CTLR_EL1_PRIBITS_SHIFT`.
pub const ICC_CTLR_EL1_PRIBITS_SHIFT: u64 = 8;
/// `ICC_CTLR_EL1_PRIBITS_MASK`: the number of priority bits implemented, minus one.
pub const ICC_CTLR_EL1_PRIBITS_MASK: u64 = 0x7 << 8;
/// `ICC_CTLR_EL1_PRIBITS(reg)`.
pub const fn icc_ctlr_el1_pribits(reg: u64) -> u64 {
    (reg & ICC_CTLR_EL1_PRIBITS_MASK) >> ICC_CTLR_EL1_PRIBITS_SHIFT
}

/* ICC_IAR1_EL1 */

/// `ICC_IAR1_EL1_SPUR`: the spurious interrupt ID.
pub const ICC_IAR1_EL1_SPUR: u64 = 0x03ff;

/* ICC_IGRPEN0_EL1 */

/// `ICC_IGRPEN0_EL1_EN`.
pub const ICC_IGRPEN0_EL1_EN: u64 = 1 << 0;

/* ICC_PMR_EL1 */

/// `ICC_PMR_EL1_PRIO_MASK`.
pub const ICC_PMR_EL1_PRIO_MASK: u64 = 0xFF;

/* ICC_SGI1R_EL1 */

/// `ICC_SGI1R_EL1_TL_MASK`: the target list, one bit per affinity-0 value.
pub const ICC_SGI1R_EL1_TL_MASK: u64 = 0xffff;
/// `ICC_SGI1R_EL1_AFF1_SHIFT`.
pub const ICC_SGI1R_EL1_AFF1_SHIFT: u64 = 16;
/// `ICC_SGI1R_EL1_SGIID_SHIFT`.
pub const ICC_SGI1R_EL1_SGIID_SHIFT: u64 = 24;
/// `ICC_SGI1R_EL1_AFF2_SHIFT`.
pub const ICC_SGI1R_EL1_AFF2_SHIFT: u64 = 32;
/// `ICC_SGI1R_EL1_AFF3_SHIFT`.
pub const ICC_SGI1R_EL1_AFF3_SHIFT: u64 = 48;
/// `ICC_SGI1R_EL1_SGIID_MASK`.
pub const ICC_SGI1R_EL1_SGIID_MASK: u64 = 0xf;
/// `ICC_SGI1R_EL1_IRM`: to every processor but the sender.
pub const ICC_SGI1R_EL1_IRM: u64 = 0x1 << 40;

/* ICC_SRE_EL1 */

/// `ICC_SRE_EL1_SRE`: the system register interface at EL1.
pub const ICC_SRE_EL1_SRE: u64 = 1 << 0;

/* ICC_SRE_EL2 (locore.S's drop_to_el1) */

/// `ICC_SRE_EL2_SRE`: the GICv3 CPU interface's system registers at EL2.
pub const ICC_SRE_EL2_SRE: u64 = 1 << 0;
/// `ICC_SRE_EL2_EN`: lets EL1 use the system register interface.
pub const ICC_SRE_EL2_EN: u64 = 1 << 3;

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
/// `CNTKCTL_EVNTEN`: enable the event stream (not in the C header; see the deviations).
pub const CNTKCTL_EVNTEN: u64 = 1 << 2;
/// `CNTKCTL_EVNTDIR`: the transition of the trigger bit that makes an event (0: 0 to 1).
pub const CNTKCTL_EVNTDIR: u64 = 1 << 3;
/// `CNTKCTL_EVNTI_SHIFT`: the virtual counter bit that triggers the event stream.
pub const CNTKCTL_EVNTI_SHIFT: u64 = 4;
/// `CNTKCTL_EVNTI_MASK`.
pub const CNTKCTL_EVNTI_MASK: u64 = 0xf << CNTKCTL_EVNTI_SHIFT;

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
/// `ISS_MSR_DIR`: a read (`mrs`).
pub const ISS_MSR_DIR: u64 = 0x01;
/// `ISS_MSR_Rt_SHIFT`.
pub const ISS_MSR_RT_SHIFT: u32 = 5;
/// `ISS_MSR_Rt_MASK`.
pub const ISS_MSR_RT_MASK: u64 = 0x1f << ISS_MSR_RT_SHIFT;
/// `ISS_MSR_CRm_SHIFT`.
pub const ISS_MSR_CRM_SHIFT: u32 = 1;
/// `ISS_MSR_CRm_MASK`.
pub const ISS_MSR_CRM_MASK: u64 = 0xf << ISS_MSR_CRM_SHIFT;
/// `ISS_MSR_CRn_SHIFT`.
pub const ISS_MSR_CRN_SHIFT: u32 = 10;
/// `ISS_MSR_CRn_MASK`.
pub const ISS_MSR_CRN_MASK: u64 = 0xf << ISS_MSR_CRN_SHIFT;
/// `ISS_MSR_OP1_SHIFT`.
pub const ISS_MSR_OP1_SHIFT: u32 = 14;
/// `ISS_MSR_OP1_MASK`.
pub const ISS_MSR_OP1_MASK: u64 = 0x7 << ISS_MSR_OP1_SHIFT;
/// `ISS_MSR_OP2_SHIFT`.
pub const ISS_MSR_OP2_SHIFT: u32 = 17;
/// `ISS_MSR_OP2_MASK`.
pub const ISS_MSR_OP2_MASK: u64 = 0x7 << ISS_MSR_OP2_SHIFT;
/// `ISS_MSR_OP0_SHIFT`.
pub const ISS_MSR_OP0_SHIFT: u32 = 20;
/// `ISS_MSR_OP0_MASK`.
pub const ISS_MSR_OP0_MASK: u64 = 0x3 << ISS_MSR_OP0_SHIFT;

/// `ISS_MSR_Rt(x)`, `ISS_MSR_CRm(x)`, ...: the field `mask` (shifted by `shift`) of a
/// trapped `msr`/`mrs` syndrome.
pub const fn iss_msr_field(esr: u64, mask: u64, shift: u32) -> u64 {
    (esr & mask) >> shift
}

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
/// `DBG_MDSCR_TDCC`: traps EL0 accesses to the debug communication channel.
pub const DBG_MDSCR_TDCC: u64 = 0x1 << 12;
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
/// `PSR_BTYPE`: the branch type of the last indirect branch (BTI).
pub const PSR_BTYPE: u64 = 0x0000_0c00;
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

// ID_AA64DFR0_EL1

/// `ID_AA64DFR0_MASK`.
pub const ID_AA64DFR0_MASK: u64 = 0x00000000f0f0ffff;
/// `ID_AA64DFR0_DEBUG_VER_SHIFT`.
pub const ID_AA64DFR0_DEBUG_VER_SHIFT: u32 = 0;
/// `ID_AA64DFR0_DEBUG_VER_MASK`.
pub const ID_AA64DFR0_DEBUG_VER_MASK: u64 = 0xf << ID_AA64DFR0_DEBUG_VER_SHIFT;
/// `ID_AA64DFR0_DEBUG_VER_8`.
pub const ID_AA64DFR0_DEBUG_VER_8: u64 = 0x6 << ID_AA64DFR0_DEBUG_VER_SHIFT;
/// `ID_AA64DFR0_DEBUG_VER_8_VHE`.
pub const ID_AA64DFR0_DEBUG_VER_8_VHE: u64 = 0x7 << ID_AA64DFR0_DEBUG_VER_SHIFT;
/// `ID_AA64DFR0_TRACE_VER_SHIFT`.
pub const ID_AA64DFR0_TRACE_VER_SHIFT: u32 = 4;
/// `ID_AA64DFR0_TRACE_VER_MASK`.
pub const ID_AA64DFR0_TRACE_VER_MASK: u64 = 0xf << ID_AA64DFR0_TRACE_VER_SHIFT;
/// `ID_AA64DFR0_TRACE_VER_NONE`.
pub const ID_AA64DFR0_TRACE_VER_NONE: u64 = 0x0 << ID_AA64DFR0_TRACE_VER_SHIFT;
/// `ID_AA64DFR0_TRACE_VER_IMPL`.
pub const ID_AA64DFR0_TRACE_VER_IMPL: u64 = 0x1 << ID_AA64DFR0_TRACE_VER_SHIFT;
/// `ID_AA64DFR0_PMU_VER_SHIFT`.
pub const ID_AA64DFR0_PMU_VER_SHIFT: u32 = 8;
/// `ID_AA64DFR0_PMU_VER_MASK`.
pub const ID_AA64DFR0_PMU_VER_MASK: u64 = 0xf << ID_AA64DFR0_PMU_VER_SHIFT;
/// `ID_AA64DFR0_PMU_VER_NONE`.
pub const ID_AA64DFR0_PMU_VER_NONE: u64 = 0x0 << ID_AA64DFR0_PMU_VER_SHIFT;
/// `ID_AA64DFR0_PMU_VER_3`.
pub const ID_AA64DFR0_PMU_VER_3: u64 = 0x1 << ID_AA64DFR0_PMU_VER_SHIFT;
/// `ID_AA64DFR0_PMU_VER_3_1`.
pub const ID_AA64DFR0_PMU_VER_3_1: u64 = 0x4 << ID_AA64DFR0_PMU_VER_SHIFT;
/// `ID_AA64DFR0_PMU_VER_IMPL`.
pub const ID_AA64DFR0_PMU_VER_IMPL: u64 = 0xf << ID_AA64DFR0_PMU_VER_SHIFT;
/// `ID_AA64DFR0_BRPS_SHIFT`.
pub const ID_AA64DFR0_BRPS_SHIFT: u32 = 12;
/// `ID_AA64DFR0_BRPS_MASK`.
pub const ID_AA64DFR0_BRPS_MASK: u64 = 0xf << ID_AA64DFR0_BRPS_SHIFT;
/// `ID_AA64DFR0_WRPS_SHIFT`.
pub const ID_AA64DFR0_WRPS_SHIFT: u32 = 20;
/// `ID_AA64DFR0_WRPS_MASK`.
pub const ID_AA64DFR0_WRPS_MASK: u64 = 0xf << ID_AA64DFR0_WRPS_SHIFT;
/// `ID_AA64DFR0_CTX_CMPS_SHIFT`.
pub const ID_AA64DFR0_CTX_CMPS_SHIFT: u32 = 28;
/// `ID_AA64DFR0_CTX_CMPS_MASK`.
pub const ID_AA64DFR0_CTX_CMPS_MASK: u64 = 0xf << ID_AA64DFR0_CTX_CMPS_SHIFT;

// ID_AA64ISAR0_EL1

/// `ID_AA64ISAR0_MASK`.
pub const ID_AA64ISAR0_MASK: u64 = 0xfffffffff0fffff0;
/// `ID_AA64ISAR0_AES_SHIFT`.
pub const ID_AA64ISAR0_AES_SHIFT: u32 = 4;
/// `ID_AA64ISAR0_AES_MASK`.
pub const ID_AA64ISAR0_AES_MASK: u64 = 0xf << ID_AA64ISAR0_AES_SHIFT;
/// `ID_AA64ISAR0_AES_NONE`.
pub const ID_AA64ISAR0_AES_NONE: u64 = 0x0 << ID_AA64ISAR0_AES_SHIFT;
/// `ID_AA64ISAR0_AES_BASE`.
pub const ID_AA64ISAR0_AES_BASE: u64 = 0x1 << ID_AA64ISAR0_AES_SHIFT;
/// `ID_AA64ISAR0_AES_PMULL`.
pub const ID_AA64ISAR0_AES_PMULL: u64 = 0x2 << ID_AA64ISAR0_AES_SHIFT;
/// `ID_AA64ISAR0_SHA1_SHIFT`.
pub const ID_AA64ISAR0_SHA1_SHIFT: u32 = 8;
/// `ID_AA64ISAR0_SHA1_MASK`.
pub const ID_AA64ISAR0_SHA1_MASK: u64 = 0xf << ID_AA64ISAR0_SHA1_SHIFT;
/// `ID_AA64ISAR0_SHA1_NONE`.
pub const ID_AA64ISAR0_SHA1_NONE: u64 = 0x0 << ID_AA64ISAR0_SHA1_SHIFT;
/// `ID_AA64ISAR0_SHA1_BASE`.
pub const ID_AA64ISAR0_SHA1_BASE: u64 = 0x1 << ID_AA64ISAR0_SHA1_SHIFT;
/// `ID_AA64ISAR0_SHA2_SHIFT`.
pub const ID_AA64ISAR0_SHA2_SHIFT: u32 = 12;
/// `ID_AA64ISAR0_SHA2_MASK`.
pub const ID_AA64ISAR0_SHA2_MASK: u64 = 0xf << ID_AA64ISAR0_SHA2_SHIFT;
/// `ID_AA64ISAR0_SHA2_NONE`.
pub const ID_AA64ISAR0_SHA2_NONE: u64 = 0x0 << ID_AA64ISAR0_SHA2_SHIFT;
/// `ID_AA64ISAR0_SHA2_BASE`.
pub const ID_AA64ISAR0_SHA2_BASE: u64 = 0x1 << ID_AA64ISAR0_SHA2_SHIFT;
/// `ID_AA64ISAR0_SHA2_512`.
pub const ID_AA64ISAR0_SHA2_512: u64 = 0x2 << ID_AA64ISAR0_SHA2_SHIFT;
/// `ID_AA64ISAR0_CRC32_SHIFT`.
pub const ID_AA64ISAR0_CRC32_SHIFT: u32 = 16;
/// `ID_AA64ISAR0_CRC32_MASK`.
pub const ID_AA64ISAR0_CRC32_MASK: u64 = 0xf << ID_AA64ISAR0_CRC32_SHIFT;
/// `ID_AA64ISAR0_CRC32_NONE`.
pub const ID_AA64ISAR0_CRC32_NONE: u64 = 0x0 << ID_AA64ISAR0_CRC32_SHIFT;
/// `ID_AA64ISAR0_CRC32_BASE`.
pub const ID_AA64ISAR0_CRC32_BASE: u64 = 0x1 << ID_AA64ISAR0_CRC32_SHIFT;
/// `ID_AA64ISAR0_ATOMIC_SHIFT`.
pub const ID_AA64ISAR0_ATOMIC_SHIFT: u32 = 20;
/// `ID_AA64ISAR0_ATOMIC_MASK`.
pub const ID_AA64ISAR0_ATOMIC_MASK: u64 = 0xf << ID_AA64ISAR0_ATOMIC_SHIFT;
/// `ID_AA64ISAR0_ATOMIC_NONE`.
pub const ID_AA64ISAR0_ATOMIC_NONE: u64 = 0x0 << ID_AA64ISAR0_ATOMIC_SHIFT;
/// `ID_AA64ISAR0_ATOMIC_IMPL`.
pub const ID_AA64ISAR0_ATOMIC_IMPL: u64 = 0x2 << ID_AA64ISAR0_ATOMIC_SHIFT;
/// `ID_AA64ISAR0_RDM_SHIFT`.
pub const ID_AA64ISAR0_RDM_SHIFT: u32 = 28;
/// `ID_AA64ISAR0_RDM_MASK`.
pub const ID_AA64ISAR0_RDM_MASK: u64 = 0xf << ID_AA64ISAR0_RDM_SHIFT;
/// `ID_AA64ISAR0_RDM_NONE`.
pub const ID_AA64ISAR0_RDM_NONE: u64 = 0x0 << ID_AA64ISAR0_RDM_SHIFT;
/// `ID_AA64ISAR0_RDM_IMPL`.
pub const ID_AA64ISAR0_RDM_IMPL: u64 = 0x1 << ID_AA64ISAR0_RDM_SHIFT;
/// `ID_AA64ISAR0_SHA3_SHIFT`.
pub const ID_AA64ISAR0_SHA3_SHIFT: u32 = 32;
/// `ID_AA64ISAR0_SHA3_MASK`.
pub const ID_AA64ISAR0_SHA3_MASK: u64 = 0xf << ID_AA64ISAR0_SHA3_SHIFT;
/// `ID_AA64ISAR0_SHA3_NONE`.
pub const ID_AA64ISAR0_SHA3_NONE: u64 = 0x0 << ID_AA64ISAR0_SHA3_SHIFT;
/// `ID_AA64ISAR0_SHA3_IMPL`.
pub const ID_AA64ISAR0_SHA3_IMPL: u64 = 0x1 << ID_AA64ISAR0_SHA3_SHIFT;
/// `ID_AA64ISAR0_SM3_SHIFT`.
pub const ID_AA64ISAR0_SM3_SHIFT: u32 = 36;
/// `ID_AA64ISAR0_SM3_MASK`.
pub const ID_AA64ISAR0_SM3_MASK: u64 = 0xf << ID_AA64ISAR0_SM3_SHIFT;
/// `ID_AA64ISAR0_SM3_NONE`.
pub const ID_AA64ISAR0_SM3_NONE: u64 = 0x0 << ID_AA64ISAR0_SM3_SHIFT;
/// `ID_AA64ISAR0_SM3_IMPL`.
pub const ID_AA64ISAR0_SM3_IMPL: u64 = 0x1 << ID_AA64ISAR0_SM3_SHIFT;
/// `ID_AA64ISAR0_SM4_SHIFT`.
pub const ID_AA64ISAR0_SM4_SHIFT: u32 = 40;
/// `ID_AA64ISAR0_SM4_MASK`.
pub const ID_AA64ISAR0_SM4_MASK: u64 = 0xf << ID_AA64ISAR0_SM4_SHIFT;
/// `ID_AA64ISAR0_SM4_NONE`.
pub const ID_AA64ISAR0_SM4_NONE: u64 = 0x0 << ID_AA64ISAR0_SM4_SHIFT;
/// `ID_AA64ISAR0_SM4_IMPL`.
pub const ID_AA64ISAR0_SM4_IMPL: u64 = 0x1 << ID_AA64ISAR0_SM4_SHIFT;
/// `ID_AA64ISAR0_DP_SHIFT`.
pub const ID_AA64ISAR0_DP_SHIFT: u32 = 44;
/// `ID_AA64ISAR0_DP_MASK`.
pub const ID_AA64ISAR0_DP_MASK: u64 = 0xf << ID_AA64ISAR0_DP_SHIFT;
/// `ID_AA64ISAR0_DP_NONE`.
pub const ID_AA64ISAR0_DP_NONE: u64 = 0x0 << ID_AA64ISAR0_DP_SHIFT;
/// `ID_AA64ISAR0_DP_IMPL`.
pub const ID_AA64ISAR0_DP_IMPL: u64 = 0x1 << ID_AA64ISAR0_DP_SHIFT;
/// `ID_AA64ISAR0_FHM_SHIFT`.
pub const ID_AA64ISAR0_FHM_SHIFT: u32 = 48;
/// `ID_AA64ISAR0_FHM_MASK`.
pub const ID_AA64ISAR0_FHM_MASK: u64 = 0xf << ID_AA64ISAR0_FHM_SHIFT;
/// `ID_AA64ISAR0_FHM_NONE`.
pub const ID_AA64ISAR0_FHM_NONE: u64 = 0x0 << ID_AA64ISAR0_FHM_SHIFT;
/// `ID_AA64ISAR0_FHM_IMPL`.
pub const ID_AA64ISAR0_FHM_IMPL: u64 = 0x1 << ID_AA64ISAR0_FHM_SHIFT;
/// `ID_AA64ISAR0_TS_SHIFT`.
pub const ID_AA64ISAR0_TS_SHIFT: u32 = 52;
/// `ID_AA64ISAR0_TS_MASK`.
pub const ID_AA64ISAR0_TS_MASK: u64 = 0xf << ID_AA64ISAR0_TS_SHIFT;
/// `ID_AA64ISAR0_TS_NONE`.
pub const ID_AA64ISAR0_TS_NONE: u64 = 0x0 << ID_AA64ISAR0_TS_SHIFT;
/// `ID_AA64ISAR0_TS_BASE`.
pub const ID_AA64ISAR0_TS_BASE: u64 = 0x1 << ID_AA64ISAR0_TS_SHIFT;
/// `ID_AA64ISAR0_TS_AXFLAG`.
pub const ID_AA64ISAR0_TS_AXFLAG: u64 = 0x2 << ID_AA64ISAR0_TS_SHIFT;
/// `ID_AA64ISAR0_TLB_SHIFT`.
pub const ID_AA64ISAR0_TLB_SHIFT: u32 = 56;
/// `ID_AA64ISAR0_TLB_MASK`.
pub const ID_AA64ISAR0_TLB_MASK: u64 = 0xf << ID_AA64ISAR0_TLB_SHIFT;
/// `ID_AA64ISAR0_TLB_NONE`.
pub const ID_AA64ISAR0_TLB_NONE: u64 = 0x0 << ID_AA64ISAR0_TLB_SHIFT;
/// `ID_AA64ISAR0_TLB_IOS`.
pub const ID_AA64ISAR0_TLB_IOS: u64 = 0x1 << ID_AA64ISAR0_TLB_SHIFT;
/// `ID_AA64ISAR0_TLB_IRANGE`.
pub const ID_AA64ISAR0_TLB_IRANGE: u64 = 0x2 << ID_AA64ISAR0_TLB_SHIFT;
/// `ID_AA64ISAR0_RNDR_SHIFT`.
pub const ID_AA64ISAR0_RNDR_SHIFT: u32 = 60;
/// `ID_AA64ISAR0_RNDR_MASK`.
pub const ID_AA64ISAR0_RNDR_MASK: u64 = 0xf << ID_AA64ISAR0_RNDR_SHIFT;
/// `ID_AA64ISAR0_RNDR_NONE`.
pub const ID_AA64ISAR0_RNDR_NONE: u64 = 0x0 << ID_AA64ISAR0_RNDR_SHIFT;
/// `ID_AA64ISAR0_RNDR_IMPL`.
pub const ID_AA64ISAR0_RNDR_IMPL: u64 = 0x1 << ID_AA64ISAR0_RNDR_SHIFT;

// ID_AA64ISAR1_EL1

/// `ID_AA64ISAR1_MASK`.
pub const ID_AA64ISAR1_MASK: u64 = 0xffffffffffffffff;
/// `ID_AA64ISAR1_DPB_SHIFT`.
pub const ID_AA64ISAR1_DPB_SHIFT: u32 = 0;
/// `ID_AA64ISAR1_DPB_MASK`.
pub const ID_AA64ISAR1_DPB_MASK: u64 = 0xf << ID_AA64ISAR1_DPB_SHIFT;
/// `ID_AA64ISAR1_DPB_NONE`.
pub const ID_AA64ISAR1_DPB_NONE: u64 = 0x0 << ID_AA64ISAR1_DPB_SHIFT;
/// `ID_AA64ISAR1_DPB_IMPL`.
pub const ID_AA64ISAR1_DPB_IMPL: u64 = 0x1 << ID_AA64ISAR1_DPB_SHIFT;
/// `ID_AA64ISAR1_DPB_DCCVADP`.
pub const ID_AA64ISAR1_DPB_DCCVADP: u64 = 0x2 << ID_AA64ISAR1_DPB_SHIFT;
/// `ID_AA64ISAR1_APA_SHIFT`.
pub const ID_AA64ISAR1_APA_SHIFT: u32 = 4;
/// `ID_AA64ISAR1_APA_MASK`.
pub const ID_AA64ISAR1_APA_MASK: u64 = 0xf << ID_AA64ISAR1_APA_SHIFT;
/// `ID_AA64ISAR1_APA_NONE`.
pub const ID_AA64ISAR1_APA_NONE: u64 = 0x0 << ID_AA64ISAR1_APA_SHIFT;
/// `ID_AA64ISAR1_APA_PAC`.
pub const ID_AA64ISAR1_APA_PAC: u64 = 0x1 << ID_AA64ISAR1_APA_SHIFT;
/// `ID_AA64ISAR1_APA_EPAC`.
pub const ID_AA64ISAR1_APA_EPAC: u64 = 0x2 << ID_AA64ISAR1_APA_SHIFT;
/// `ID_AA64ISAR1_APA_EPAC2`.
pub const ID_AA64ISAR1_APA_EPAC2: u64 = 0x3 << ID_AA64ISAR1_APA_SHIFT;
/// `ID_AA64ISAR1_APA_FPAC`.
pub const ID_AA64ISAR1_APA_FPAC: u64 = 0x4 << ID_AA64ISAR1_APA_SHIFT;
/// `ID_AA64ISAR1_APA_FPAC_COMBINED`.
pub const ID_AA64ISAR1_APA_FPAC_COMBINED: u64 = 0x5 << ID_AA64ISAR1_APA_SHIFT;
/// `ID_AA64ISAR1_API_SHIFT`.
pub const ID_AA64ISAR1_API_SHIFT: u32 = 8;
/// `ID_AA64ISAR1_API_MASK`.
pub const ID_AA64ISAR1_API_MASK: u64 = 0xf << ID_AA64ISAR1_API_SHIFT;
/// `ID_AA64ISAR1_API_NONE`.
pub const ID_AA64ISAR1_API_NONE: u64 = 0x0 << ID_AA64ISAR1_API_SHIFT;
/// `ID_AA64ISAR1_API_PAC`.
pub const ID_AA64ISAR1_API_PAC: u64 = 0x1 << ID_AA64ISAR1_API_SHIFT;
/// `ID_AA64ISAR1_API_EPAC`.
pub const ID_AA64ISAR1_API_EPAC: u64 = 0x2 << ID_AA64ISAR1_API_SHIFT;
/// `ID_AA64ISAR1_API_EPAC2`.
pub const ID_AA64ISAR1_API_EPAC2: u64 = 0x3 << ID_AA64ISAR1_API_SHIFT;
/// `ID_AA64ISAR1_API_FPAC`.
pub const ID_AA64ISAR1_API_FPAC: u64 = 0x4 << ID_AA64ISAR1_API_SHIFT;
/// `ID_AA64ISAR1_API_FPAC_COMBINED`.
pub const ID_AA64ISAR1_API_FPAC_COMBINED: u64 = 0x5 << ID_AA64ISAR1_API_SHIFT;
/// `ID_AA64ISAR1_JSCVT_SHIFT`.
pub const ID_AA64ISAR1_JSCVT_SHIFT: u32 = 12;
/// `ID_AA64ISAR1_JSCVT_MASK`.
pub const ID_AA64ISAR1_JSCVT_MASK: u64 = 0xf << ID_AA64ISAR1_JSCVT_SHIFT;
/// `ID_AA64ISAR1_JSCVT_NONE`.
pub const ID_AA64ISAR1_JSCVT_NONE: u64 = 0x0 << ID_AA64ISAR1_JSCVT_SHIFT;
/// `ID_AA64ISAR1_JSCVT_IMPL`.
pub const ID_AA64ISAR1_JSCVT_IMPL: u64 = 0x1 << ID_AA64ISAR1_JSCVT_SHIFT;
/// `ID_AA64ISAR1_FCMA_SHIFT`.
pub const ID_AA64ISAR1_FCMA_SHIFT: u32 = 16;
/// `ID_AA64ISAR1_FCMA_MASK`.
pub const ID_AA64ISAR1_FCMA_MASK: u64 = 0xf << ID_AA64ISAR1_FCMA_SHIFT;
/// `ID_AA64ISAR1_FCMA_NONE`.
pub const ID_AA64ISAR1_FCMA_NONE: u64 = 0x0 << ID_AA64ISAR1_FCMA_SHIFT;
/// `ID_AA64ISAR1_FCMA_IMPL`.
pub const ID_AA64ISAR1_FCMA_IMPL: u64 = 0x1 << ID_AA64ISAR1_FCMA_SHIFT;
/// `ID_AA64ISAR1_LRCPC_SHIFT`.
pub const ID_AA64ISAR1_LRCPC_SHIFT: u32 = 20;
/// `ID_AA64ISAR1_LRCPC_MASK`.
pub const ID_AA64ISAR1_LRCPC_MASK: u64 = 0xf << ID_AA64ISAR1_LRCPC_SHIFT;
/// `ID_AA64ISAR1_LRCPC_NONE`.
pub const ID_AA64ISAR1_LRCPC_NONE: u64 = 0x0 << ID_AA64ISAR1_LRCPC_SHIFT;
/// `ID_AA64ISAR1_LRCPC_BASE`.
pub const ID_AA64ISAR1_LRCPC_BASE: u64 = 0x1 << ID_AA64ISAR1_LRCPC_SHIFT;
/// `ID_AA64ISAR1_LRCPC_LDAPUR`.
pub const ID_AA64ISAR1_LRCPC_LDAPUR: u64 = 0x2 << ID_AA64ISAR1_LRCPC_SHIFT;
/// `ID_AA64ISAR1_GPA_SHIFT`.
pub const ID_AA64ISAR1_GPA_SHIFT: u32 = 24;
/// `ID_AA64ISAR1_GPA_MASK`.
pub const ID_AA64ISAR1_GPA_MASK: u64 = 0xf << ID_AA64ISAR1_GPA_SHIFT;
/// `ID_AA64ISAR1_GPA_NONE`.
pub const ID_AA64ISAR1_GPA_NONE: u64 = 0x0 << ID_AA64ISAR1_GPA_SHIFT;
/// `ID_AA64ISAR1_GPA_IMPL`.
pub const ID_AA64ISAR1_GPA_IMPL: u64 = 0x1 << ID_AA64ISAR1_GPA_SHIFT;
/// `ID_AA64ISAR1_GPI_SHIFT`.
pub const ID_AA64ISAR1_GPI_SHIFT: u32 = 28;
/// `ID_AA64ISAR1_GPI_MASK`.
pub const ID_AA64ISAR1_GPI_MASK: u64 = 0xf << ID_AA64ISAR1_GPI_SHIFT;
/// `ID_AA64ISAR1_GPI_NONE`.
pub const ID_AA64ISAR1_GPI_NONE: u64 = 0x0 << ID_AA64ISAR1_GPI_SHIFT;
/// `ID_AA64ISAR1_GPI_IMPL`.
pub const ID_AA64ISAR1_GPI_IMPL: u64 = 0x1 << ID_AA64ISAR1_GPI_SHIFT;
/// `ID_AA64ISAR1_FRINTTS_SHIFT`.
pub const ID_AA64ISAR1_FRINTTS_SHIFT: u32 = 32;
/// `ID_AA64ISAR1_FRINTTS_MASK`.
pub const ID_AA64ISAR1_FRINTTS_MASK: u64 = 0xf << ID_AA64ISAR1_FRINTTS_SHIFT;
/// `ID_AA64ISAR1_FRINTTS_NONE`.
pub const ID_AA64ISAR1_FRINTTS_NONE: u64 = 0x0 << ID_AA64ISAR1_FRINTTS_SHIFT;
/// `ID_AA64ISAR1_FRINTTS_IMPL`.
pub const ID_AA64ISAR1_FRINTTS_IMPL: u64 = 0x1 << ID_AA64ISAR1_FRINTTS_SHIFT;
/// `ID_AA64ISAR1_SB_SHIFT`.
pub const ID_AA64ISAR1_SB_SHIFT: u32 = 36;
/// `ID_AA64ISAR1_SB_MASK`.
pub const ID_AA64ISAR1_SB_MASK: u64 = 0xf << ID_AA64ISAR1_SB_SHIFT;
/// `ID_AA64ISAR1_SB_NONE`.
pub const ID_AA64ISAR1_SB_NONE: u64 = 0x0 << ID_AA64ISAR1_SB_SHIFT;
/// `ID_AA64ISAR1_SB_IMPL`.
pub const ID_AA64ISAR1_SB_IMPL: u64 = 0x1 << ID_AA64ISAR1_SB_SHIFT;
/// `ID_AA64ISAR1_SPECRES_SHIFT`.
pub const ID_AA64ISAR1_SPECRES_SHIFT: u32 = 40;
/// `ID_AA64ISAR1_SPECRES_MASK`.
pub const ID_AA64ISAR1_SPECRES_MASK: u64 = 0xf << ID_AA64ISAR1_SPECRES_SHIFT;
/// `ID_AA64ISAR1_SPECRES_NONE`.
pub const ID_AA64ISAR1_SPECRES_NONE: u64 = 0x0 << ID_AA64ISAR1_SPECRES_SHIFT;
/// `ID_AA64ISAR1_SPECRES_IMPL`.
pub const ID_AA64ISAR1_SPECRES_IMPL: u64 = 0x1 << ID_AA64ISAR1_SPECRES_SHIFT;
/// `ID_AA64ISAR1_BF16_SHIFT`.
pub const ID_AA64ISAR1_BF16_SHIFT: u32 = 44;
/// `ID_AA64ISAR1_BF16_MASK`.
pub const ID_AA64ISAR1_BF16_MASK: u64 = 0xf << ID_AA64ISAR1_BF16_SHIFT;
/// `ID_AA64ISAR1_BF16_NONE`.
pub const ID_AA64ISAR1_BF16_NONE: u64 = 0x0 << ID_AA64ISAR1_BF16_SHIFT;
/// `ID_AA64ISAR1_BF16_BASE`.
pub const ID_AA64ISAR1_BF16_BASE: u64 = 0x1 << ID_AA64ISAR1_BF16_SHIFT;
/// `ID_AA64ISAR1_BF16_EBF`.
pub const ID_AA64ISAR1_BF16_EBF: u64 = 0x2 << ID_AA64ISAR1_BF16_SHIFT;
/// `ID_AA64ISAR1_DGH_SHIFT`.
pub const ID_AA64ISAR1_DGH_SHIFT: u32 = 48;
/// `ID_AA64ISAR1_DGH_MASK`.
pub const ID_AA64ISAR1_DGH_MASK: u64 = 0xf << ID_AA64ISAR1_DGH_SHIFT;
/// `ID_AA64ISAR1_DGH_NONE`.
pub const ID_AA64ISAR1_DGH_NONE: u64 = 0x0 << ID_AA64ISAR1_DGH_SHIFT;
/// `ID_AA64ISAR1_DGH_IMPL`.
pub const ID_AA64ISAR1_DGH_IMPL: u64 = 0x1 << ID_AA64ISAR1_DGH_SHIFT;
/// `ID_AA64ISAR1_I8MM_SHIFT`.
pub const ID_AA64ISAR1_I8MM_SHIFT: u32 = 52;
/// `ID_AA64ISAR1_I8MM_MASK`.
pub const ID_AA64ISAR1_I8MM_MASK: u64 = 0xf << ID_AA64ISAR1_I8MM_SHIFT;
/// `ID_AA64ISAR1_I8MM_NONE`.
pub const ID_AA64ISAR1_I8MM_NONE: u64 = 0x0 << ID_AA64ISAR1_I8MM_SHIFT;
/// `ID_AA64ISAR1_I8MM_IMPL`.
pub const ID_AA64ISAR1_I8MM_IMPL: u64 = 0x1 << ID_AA64ISAR1_I8MM_SHIFT;
/// `ID_AA64ISAR1_XS_SHIFT`.
pub const ID_AA64ISAR1_XS_SHIFT: u32 = 56;
/// `ID_AA64ISAR1_XS_MASK`.
pub const ID_AA64ISAR1_XS_MASK: u64 = 0xf << ID_AA64ISAR1_XS_SHIFT;
/// `ID_AA64ISAR1_XS_NONE`.
pub const ID_AA64ISAR1_XS_NONE: u64 = 0x0 << ID_AA64ISAR1_XS_SHIFT;
/// `ID_AA64ISAR1_XS_IMPL`.
pub const ID_AA64ISAR1_XS_IMPL: u64 = 0x1 << ID_AA64ISAR1_XS_SHIFT;
/// `ID_AA64ISAR1_LS64_SHIFT`.
pub const ID_AA64ISAR1_LS64_SHIFT: u32 = 60;
/// `ID_AA64ISAR1_LS64_MASK`.
pub const ID_AA64ISAR1_LS64_MASK: u64 = 0xf << ID_AA64ISAR1_LS64_SHIFT;
/// `ID_AA64ISAR1_LS64_NONE`.
pub const ID_AA64ISAR1_LS64_NONE: u64 = 0x0 << ID_AA64ISAR1_LS64_SHIFT;
/// `ID_AA64ISAR1_LS64_BASE`.
pub const ID_AA64ISAR1_LS64_BASE: u64 = 0x1 << ID_AA64ISAR1_LS64_SHIFT;
/// `ID_AA64ISAR1_LS64_V`.
pub const ID_AA64ISAR1_LS64_V: u64 = 0x2 << ID_AA64ISAR1_LS64_SHIFT;
/// `ID_AA64ISAR1_LS64_ACCDATA`.
pub const ID_AA64ISAR1_LS64_ACCDATA: u64 = 0x3 << ID_AA64ISAR1_LS64_SHIFT;

// ID_AA64ISAR2_EL1

/// `ID_AA64ISAR2_MASK`.
pub const ID_AA64ISAR2_MASK: u64 = 0x00ff0000f0ffffff;
/// `ID_AA64ISAR2_WFXT_SHIFT`.
pub const ID_AA64ISAR2_WFXT_SHIFT: u32 = 0;
/// `ID_AA64ISAR2_WFXT_MASK`.
pub const ID_AA64ISAR2_WFXT_MASK: u64 = 0xf << ID_AA64ISAR2_WFXT_SHIFT;
/// `ID_AA64ISAR2_WFXT_NONE`.
pub const ID_AA64ISAR2_WFXT_NONE: u64 = 0x0 << ID_AA64ISAR2_WFXT_SHIFT;
/// `ID_AA64ISAR2_WFXT_IMPL`.
pub const ID_AA64ISAR2_WFXT_IMPL: u64 = 0x2 << ID_AA64ISAR2_WFXT_SHIFT;
/// `ID_AA64ISAR2_RPRES_SHIFT`.
pub const ID_AA64ISAR2_RPRES_SHIFT: u32 = 4;
/// `ID_AA64ISAR2_RPRES_MASK`.
pub const ID_AA64ISAR2_RPRES_MASK: u64 = 0xf << ID_AA64ISAR2_RPRES_SHIFT;
/// `ID_AA64ISAR2_RPRES_NONE`.
pub const ID_AA64ISAR2_RPRES_NONE: u64 = 0x0 << ID_AA64ISAR2_RPRES_SHIFT;
/// `ID_AA64ISAR2_RPRES_IMPL`.
pub const ID_AA64ISAR2_RPRES_IMPL: u64 = 0x1 << ID_AA64ISAR2_RPRES_SHIFT;
/// `ID_AA64ISAR2_GPA3_SHIFT`.
pub const ID_AA64ISAR2_GPA3_SHIFT: u32 = 8;
/// `ID_AA64ISAR2_GPA3_WIDTH`.
pub const ID_AA64ISAR2_GPA3_WIDTH: u64 = 4;
/// `ID_AA64ISAR2_GPA3_MASK`.
pub const ID_AA64ISAR2_GPA3_MASK: u64 = 0xf << ID_AA64ISAR2_GPA3_SHIFT;
/// `ID_AA64ISAR2_GPA3_NONE`.
pub const ID_AA64ISAR2_GPA3_NONE: u64 = 0x0 << ID_AA64ISAR2_GPA3_SHIFT;
/// `ID_AA64ISAR2_GPA3_IMPL`.
pub const ID_AA64ISAR2_GPA3_IMPL: u64 = 0x1 << ID_AA64ISAR2_GPA3_SHIFT;
/// `ID_AA64ISAR2_APA3_SHIFT`.
pub const ID_AA64ISAR2_APA3_SHIFT: u32 = 12;
/// `ID_AA64ISAR2_APA3_WIDTH`.
pub const ID_AA64ISAR2_APA3_WIDTH: u64 = 4;
/// `ID_AA64ISAR2_APA3_MASK`.
pub const ID_AA64ISAR2_APA3_MASK: u64 = 0xf << ID_AA64ISAR2_APA3_SHIFT;
/// `ID_AA64ISAR2_APA3_NONE`.
pub const ID_AA64ISAR2_APA3_NONE: u64 = 0x0 << ID_AA64ISAR2_APA3_SHIFT;
/// `ID_AA64ISAR2_APA3_PAC`.
pub const ID_AA64ISAR2_APA3_PAC: u64 = 0x1 << ID_AA64ISAR2_APA3_SHIFT;
/// `ID_AA64ISAR2_APA3_EPAC`.
pub const ID_AA64ISAR2_APA3_EPAC: u64 = 0x2 << ID_AA64ISAR2_APA3_SHIFT;
/// `ID_AA64ISAR2_APA3_EPAC2`.
pub const ID_AA64ISAR2_APA3_EPAC2: u64 = 0x3 << ID_AA64ISAR2_APA3_SHIFT;
/// `ID_AA64ISAR2_APA3_FPAC`.
pub const ID_AA64ISAR2_APA3_FPAC: u64 = 0x4 << ID_AA64ISAR2_APA3_SHIFT;
/// `ID_AA64ISAR2_APA3_FPAC_COMBINED`.
pub const ID_AA64ISAR2_APA3_FPAC_COMBINED: u64 = 0x5 << ID_AA64ISAR2_APA3_SHIFT;
/// `ID_AA64ISAR2_MOPS_SHIFT`.
pub const ID_AA64ISAR2_MOPS_SHIFT: u32 = 16;
/// `ID_AA64ISAR2_MOPS_MASK`.
pub const ID_AA64ISAR2_MOPS_MASK: u64 = 0xf << ID_AA64ISAR2_MOPS_SHIFT;
/// `ID_AA64ISAR2_MOPS_NONE`.
pub const ID_AA64ISAR2_MOPS_NONE: u64 = 0x0 << ID_AA64ISAR2_MOPS_SHIFT;
/// `ID_AA64ISAR2_MOPS_IMPL`.
pub const ID_AA64ISAR2_MOPS_IMPL: u64 = 0x1 << ID_AA64ISAR2_MOPS_SHIFT;
/// `ID_AA64ISAR2_BC_SHIFT`.
pub const ID_AA64ISAR2_BC_SHIFT: u32 = 20;
/// `ID_AA64ISAR2_BC_MASK`.
pub const ID_AA64ISAR2_BC_MASK: u64 = 0xf << ID_AA64ISAR2_BC_SHIFT;
/// `ID_AA64ISAR2_BC_NONE`.
pub const ID_AA64ISAR2_BC_NONE: u64 = 0x0 << ID_AA64ISAR2_BC_SHIFT;
/// `ID_AA64ISAR2_BC_IMPL`.
pub const ID_AA64ISAR2_BC_IMPL: u64 = 0x1 << ID_AA64ISAR2_BC_SHIFT;
/// `ID_AA64ISAR2_CLRBHB_SHIFT`.
pub const ID_AA64ISAR2_CLRBHB_SHIFT: u32 = 28;
/// `ID_AA64ISAR2_CLRBHB_MASK`.
pub const ID_AA64ISAR2_CLRBHB_MASK: u64 = 0xf << ID_AA64ISAR2_CLRBHB_SHIFT;
/// `ID_AA64ISAR2_CLRBHB_NONE`.
pub const ID_AA64ISAR2_CLRBHB_NONE: u64 = 0x0 << ID_AA64ISAR2_CLRBHB_SHIFT;
/// `ID_AA64ISAR2_CLRBHB_IMPL`.
pub const ID_AA64ISAR2_CLRBHB_IMPL: u64 = 0x1 << ID_AA64ISAR2_CLRBHB_SHIFT;
/// `ID_AA64ISAR2_RPRFM_SHIFT`.
pub const ID_AA64ISAR2_RPRFM_SHIFT: u32 = 48;
/// `ID_AA64ISAR2_RPRFM_MASK`.
pub const ID_AA64ISAR2_RPRFM_MASK: u64 = 0xf << ID_AA64ISAR2_RPRFM_SHIFT;
/// `ID_AA64ISAR2_RPRFM_NONE`.
pub const ID_AA64ISAR2_RPRFM_NONE: u64 = 0x0 << ID_AA64ISAR2_RPRFM_SHIFT;
/// `ID_AA64ISAR2_RPRFM_IMPL`.
pub const ID_AA64ISAR2_RPRFM_IMPL: u64 = 0x1 << ID_AA64ISAR2_RPRFM_SHIFT;
/// `ID_AA64ISAR2_CSSC_SHIFT`.
pub const ID_AA64ISAR2_CSSC_SHIFT: u32 = 52;
/// `ID_AA64ISAR2_CSSC_MASK`.
pub const ID_AA64ISAR2_CSSC_MASK: u64 = 0xf << ID_AA64ISAR2_CSSC_SHIFT;
/// `ID_AA64ISAR2_CSSC_NONE`.
pub const ID_AA64ISAR2_CSSC_NONE: u64 = 0x0 << ID_AA64ISAR2_CSSC_SHIFT;
/// `ID_AA64ISAR2_CSSC_IMPL`.
pub const ID_AA64ISAR2_CSSC_IMPL: u64 = 0x1 << ID_AA64ISAR2_CSSC_SHIFT;

// ID_AA64MMFR0_EL1

/// `ID_AA64MMFR0_MASK`.
pub const ID_AA64MMFR0_MASK: u64 = 0xf0000000ffffffff;
/// `ID_AA64MMFR0_PA_RANGE_SHIFT`.
pub const ID_AA64MMFR0_PA_RANGE_SHIFT: u32 = 0;
/// `ID_AA64MMFR0_PA_RANGE_MASK`.
pub const ID_AA64MMFR0_PA_RANGE_MASK: u64 = 0xf << ID_AA64MMFR0_PA_RANGE_SHIFT;
/// `ID_AA64MMFR0_PA_RANGE_4G`.
pub const ID_AA64MMFR0_PA_RANGE_4G: u64 = 0x0 << ID_AA64MMFR0_PA_RANGE_SHIFT;
/// `ID_AA64MMFR0_PA_RANGE_64G`.
pub const ID_AA64MMFR0_PA_RANGE_64G: u64 = 0x1 << ID_AA64MMFR0_PA_RANGE_SHIFT;
/// `ID_AA64MMFR0_PA_RANGE_1T`.
pub const ID_AA64MMFR0_PA_RANGE_1T: u64 = 0x2 << ID_AA64MMFR0_PA_RANGE_SHIFT;
/// `ID_AA64MMFR0_PA_RANGE_4T`.
pub const ID_AA64MMFR0_PA_RANGE_4T: u64 = 0x3 << ID_AA64MMFR0_PA_RANGE_SHIFT;
/// `ID_AA64MMFR0_PA_RANGE_16T`.
pub const ID_AA64MMFR0_PA_RANGE_16T: u64 = 0x4 << ID_AA64MMFR0_PA_RANGE_SHIFT;
/// `ID_AA64MMFR0_PA_RANGE_256T`.
pub const ID_AA64MMFR0_PA_RANGE_256T: u64 = 0x5 << ID_AA64MMFR0_PA_RANGE_SHIFT;
/// `ID_AA64MMFR0_ASID_BITS_SHIFT`.
pub const ID_AA64MMFR0_ASID_BITS_SHIFT: u32 = 4;
/// `ID_AA64MMFR0_ASID_BITS_MASK`.
pub const ID_AA64MMFR0_ASID_BITS_MASK: u64 = 0xf << ID_AA64MMFR0_ASID_BITS_SHIFT;
/// `ID_AA64MMFR0_ASID_BITS_8`.
pub const ID_AA64MMFR0_ASID_BITS_8: u64 = 0x0 << ID_AA64MMFR0_ASID_BITS_SHIFT;
/// `ID_AA64MMFR0_ASID_BITS_16`.
pub const ID_AA64MMFR0_ASID_BITS_16: u64 = 0x2 << ID_AA64MMFR0_ASID_BITS_SHIFT;
/// `ID_AA64MMFR0_BIGEND_SHIFT`.
pub const ID_AA64MMFR0_BIGEND_SHIFT: u32 = 8;
/// `ID_AA64MMFR0_BIGEND_MASK`.
pub const ID_AA64MMFR0_BIGEND_MASK: u64 = 0xf << ID_AA64MMFR0_BIGEND_SHIFT;
/// `ID_AA64MMFR0_BIGEND_FIXED`.
pub const ID_AA64MMFR0_BIGEND_FIXED: u64 = 0x0 << ID_AA64MMFR0_BIGEND_SHIFT;
/// `ID_AA64MMFR0_BIGEND_MIXED`.
pub const ID_AA64MMFR0_BIGEND_MIXED: u64 = 0x1 << ID_AA64MMFR0_BIGEND_SHIFT;
/// `ID_AA64MMFR0_S_NS_MEM_SHIFT`.
pub const ID_AA64MMFR0_S_NS_MEM_SHIFT: u32 = 12;
/// `ID_AA64MMFR0_S_NS_MEM_MASK`.
pub const ID_AA64MMFR0_S_NS_MEM_MASK: u64 = 0xf << ID_AA64MMFR0_S_NS_MEM_SHIFT;
/// `ID_AA64MMFR0_S_NS_MEM_NONE`.
pub const ID_AA64MMFR0_S_NS_MEM_NONE: u64 = 0x0 << ID_AA64MMFR0_S_NS_MEM_SHIFT;
/// `ID_AA64MMFR0_S_NS_MEM_DISTINCT`.
pub const ID_AA64MMFR0_S_NS_MEM_DISTINCT: u64 = 0x1 << ID_AA64MMFR0_S_NS_MEM_SHIFT;
/// `ID_AA64MMFR0_BIGEND_EL0_SHIFT`.
pub const ID_AA64MMFR0_BIGEND_EL0_SHIFT: u32 = 16;
/// `ID_AA64MMFR0_BIGEND_EL0_MASK`.
pub const ID_AA64MMFR0_BIGEND_EL0_MASK: u64 = 0xf << ID_AA64MMFR0_BIGEND_EL0_SHIFT;
/// `ID_AA64MMFR0_BIGEND_EL0_FIXED`.
pub const ID_AA64MMFR0_BIGEND_EL0_FIXED: u64 = 0x0 << ID_AA64MMFR0_BIGEND_EL0_SHIFT;
/// `ID_AA64MMFR0_BIGEND_EL0_MIXED`.
pub const ID_AA64MMFR0_BIGEND_EL0_MIXED: u64 = 0x1 << ID_AA64MMFR0_BIGEND_EL0_SHIFT;
/// `ID_AA64MMFR0_TGRAN16_SHIFT`.
pub const ID_AA64MMFR0_TGRAN16_SHIFT: u32 = 20;
/// `ID_AA64MMFR0_TGRAN16_MASK`.
pub const ID_AA64MMFR0_TGRAN16_MASK: u64 = 0xf << ID_AA64MMFR0_TGRAN16_SHIFT;
/// `ID_AA64MMFR0_TGRAN16_NONE`.
pub const ID_AA64MMFR0_TGRAN16_NONE: u64 = 0x0 << ID_AA64MMFR0_TGRAN16_SHIFT;
/// `ID_AA64MMFR0_TGRAN16_IMPL`.
pub const ID_AA64MMFR0_TGRAN16_IMPL: u64 = 0x1 << ID_AA64MMFR0_TGRAN16_SHIFT;
/// `ID_AA64MMFR0_TGRAN64_SHIFT`.
pub const ID_AA64MMFR0_TGRAN64_SHIFT: u32 = 24;
/// `ID_AA64MMFR0_TGRAN64_MASK`.
pub const ID_AA64MMFR0_TGRAN64_MASK: u64 = 0xf << ID_AA64MMFR0_TGRAN64_SHIFT;
/// `ID_AA64MMFR0_TGRAN64_IMPL`.
pub const ID_AA64MMFR0_TGRAN64_IMPL: u64 = 0x0 << ID_AA64MMFR0_TGRAN64_SHIFT;
/// `ID_AA64MMFR0_TGRAN64_NONE`.
pub const ID_AA64MMFR0_TGRAN64_NONE: u64 = 0xf << ID_AA64MMFR0_TGRAN64_SHIFT;
/// `ID_AA64MMFR0_TGRAN4_SHIFT`.
pub const ID_AA64MMFR0_TGRAN4_SHIFT: u32 = 28;
/// `ID_AA64MMFR0_TGRAN4_MASK`.
pub const ID_AA64MMFR0_TGRAN4_MASK: u64 = 0xf << ID_AA64MMFR0_TGRAN4_SHIFT;
/// `ID_AA64MMFR0_TGRAN4_IMPL`.
pub const ID_AA64MMFR0_TGRAN4_IMPL: u64 = 0x0 << ID_AA64MMFR0_TGRAN4_SHIFT;
/// `ID_AA64MMFR0_TGRAN4_NONE`.
pub const ID_AA64MMFR0_TGRAN4_NONE: u64 = 0xf << ID_AA64MMFR0_TGRAN4_SHIFT;
/// `ID_AA64MMFR0_ECV_SHIFT`.
pub const ID_AA64MMFR0_ECV_SHIFT: u32 = 60;
/// `ID_AA64MMFR0_ECV_MASK`.
pub const ID_AA64MMFR0_ECV_MASK: u64 = 0xf << ID_AA64MMFR0_ECV_SHIFT;
/// `ID_AA64MMFR0_ECV_NONE`.
pub const ID_AA64MMFR0_ECV_NONE: u64 = 0x0 << ID_AA64MMFR0_ECV_SHIFT;
/// `ID_AA64MMFR0_ECV_IMPL`.
pub const ID_AA64MMFR0_ECV_IMPL: u64 = 0x1 << ID_AA64MMFR0_ECV_SHIFT;
/// `ID_AA64MMFR0_ECV_CNTHCTL`.
pub const ID_AA64MMFR0_ECV_CNTHCTL: u64 = 0x2 << ID_AA64MMFR0_ECV_SHIFT;

// ID_AA64MMFR1_EL1

/// `ID_AA64MMFR1_MASK`.
pub const ID_AA64MMFR1_MASK: u64 = 0xf000f000ffffffff;
/// `ID_AA64MMFR1_HAFDBS_SHIFT`.
pub const ID_AA64MMFR1_HAFDBS_SHIFT: u32 = 0;
/// `ID_AA64MMFR1_HAFDBS_MASK`.
pub const ID_AA64MMFR1_HAFDBS_MASK: u64 = 0xf << ID_AA64MMFR1_HAFDBS_SHIFT;
/// `ID_AA64MMFR1_HAFDBS_NONE`.
pub const ID_AA64MMFR1_HAFDBS_NONE: u64 = 0x0 << ID_AA64MMFR1_HAFDBS_SHIFT;
/// `ID_AA64MMFR1_HAFDBS_AF`.
pub const ID_AA64MMFR1_HAFDBS_AF: u64 = 0x1 << ID_AA64MMFR1_HAFDBS_SHIFT;
/// `ID_AA64MMFR1_HAFDBS_AF_DBS`.
pub const ID_AA64MMFR1_HAFDBS_AF_DBS: u64 = 0x2 << ID_AA64MMFR1_HAFDBS_SHIFT;
/// `ID_AA64MMFR1_VMIDBITS_SHIFT`.
pub const ID_AA64MMFR1_VMIDBITS_SHIFT: u32 = 4;
/// `ID_AA64MMFR1_VMIDBITS_MASK`.
pub const ID_AA64MMFR1_VMIDBITS_MASK: u64 = 0xf << ID_AA64MMFR1_VMIDBITS_SHIFT;
/// `ID_AA64MMFR1_VMIDBITS_8`.
pub const ID_AA64MMFR1_VMIDBITS_8: u64 = 0x0 << ID_AA64MMFR1_VMIDBITS_SHIFT;
/// `ID_AA64MMFR1_VMIDBITS_16`.
pub const ID_AA64MMFR1_VMIDBITS_16: u64 = 0x2 << ID_AA64MMFR1_VMIDBITS_SHIFT;
/// `ID_AA64MMFR1_VH_SHIFT`.
pub const ID_AA64MMFR1_VH_SHIFT: u32 = 8;
/// `ID_AA64MMFR1_VH_MASK`.
pub const ID_AA64MMFR1_VH_MASK: u64 = 0xf << ID_AA64MMFR1_VH_SHIFT;
/// `ID_AA64MMFR1_VH_NONE`.
pub const ID_AA64MMFR1_VH_NONE: u64 = 0x0 << ID_AA64MMFR1_VH_SHIFT;
/// `ID_AA64MMFR1_VH_IMPL`.
pub const ID_AA64MMFR1_VH_IMPL: u64 = 0x1 << ID_AA64MMFR1_VH_SHIFT;
/// `ID_AA64MMFR1_HPDS_SHIFT`.
pub const ID_AA64MMFR1_HPDS_SHIFT: u32 = 12;
/// `ID_AA64MMFR1_HPDS_MASK`.
pub const ID_AA64MMFR1_HPDS_MASK: u64 = 0xf << ID_AA64MMFR1_HPDS_SHIFT;
/// `ID_AA64MMFR1_HPDS_NONE`.
pub const ID_AA64MMFR1_HPDS_NONE: u64 = 0x0 << ID_AA64MMFR1_HPDS_SHIFT;
/// `ID_AA64MMFR1_HPDS_IMPL`.
pub const ID_AA64MMFR1_HPDS_IMPL: u64 = 0x1 << ID_AA64MMFR1_HPDS_SHIFT;
/// `ID_AA64MMFR1_LO_SHIFT`.
pub const ID_AA64MMFR1_LO_SHIFT: u32 = 16;
/// `ID_AA64MMFR1_LO_MASK`.
pub const ID_AA64MMFR1_LO_MASK: u64 = 0xf << ID_AA64MMFR1_LO_SHIFT;
/// `ID_AA64MMFR1_LO_NONE`.
pub const ID_AA64MMFR1_LO_NONE: u64 = 0x0 << ID_AA64MMFR1_LO_SHIFT;
/// `ID_AA64MMFR1_LO_IMPL`.
pub const ID_AA64MMFR1_LO_IMPL: u64 = 0x1 << ID_AA64MMFR1_LO_SHIFT;
/// `ID_AA64MMFR1_PAN_SHIFT`.
pub const ID_AA64MMFR1_PAN_SHIFT: u32 = 20;
/// `ID_AA64MMFR1_PAN_MASK`.
pub const ID_AA64MMFR1_PAN_MASK: u64 = 0xf << ID_AA64MMFR1_PAN_SHIFT;
/// `ID_AA64MMFR1_PAN_NONE`.
pub const ID_AA64MMFR1_PAN_NONE: u64 = 0x0 << ID_AA64MMFR1_PAN_SHIFT;
/// `ID_AA64MMFR1_PAN_IMPL`.
pub const ID_AA64MMFR1_PAN_IMPL: u64 = 0x1 << ID_AA64MMFR1_PAN_SHIFT;
/// `ID_AA64MMFR1_PAN_ATS1E1`.
pub const ID_AA64MMFR1_PAN_ATS1E1: u64 = 0x2 << ID_AA64MMFR1_PAN_SHIFT;
/// `ID_AA64MMFR1_PAN_EPAN`.
pub const ID_AA64MMFR1_PAN_EPAN: u64 = 0x3 << ID_AA64MMFR1_PAN_SHIFT;
/// `ID_AA64MMFR1_SPECSEI_SHIFT`.
pub const ID_AA64MMFR1_SPECSEI_SHIFT: u32 = 24;
/// `ID_AA64MMFR1_SPECSEI_MASK`.
pub const ID_AA64MMFR1_SPECSEI_MASK: u64 = 0xf << ID_AA64MMFR1_SPECSEI_SHIFT;
/// `ID_AA64MMFR1_SPECSEI_NONE`.
pub const ID_AA64MMFR1_SPECSEI_NONE: u64 = 0x0 << ID_AA64MMFR1_SPECSEI_SHIFT;
/// `ID_AA64MMFR1_SPECSEI_IMPL`.
pub const ID_AA64MMFR1_SPECSEI_IMPL: u64 = 0x1 << ID_AA64MMFR1_SPECSEI_SHIFT;
/// `ID_AA64MMFR1_XNX_SHIFT`.
pub const ID_AA64MMFR1_XNX_SHIFT: u32 = 28;
/// `ID_AA64MMFR1_XNX_MASK`.
pub const ID_AA64MMFR1_XNX_MASK: u64 = 0xf << ID_AA64MMFR1_XNX_SHIFT;
/// `ID_AA64MMFR1_XNX_NONE`.
pub const ID_AA64MMFR1_XNX_NONE: u64 = 0x0 << ID_AA64MMFR1_XNX_SHIFT;
/// `ID_AA64MMFR1_XNX_IMPL`.
pub const ID_AA64MMFR1_XNX_IMPL: u64 = 0x1 << ID_AA64MMFR1_XNX_SHIFT;
/// `ID_AA64MMFR1_AFP_SHIFT`.
pub const ID_AA64MMFR1_AFP_SHIFT: u32 = 44;
/// `ID_AA64MMFR1_AFP_MASK`.
pub const ID_AA64MMFR1_AFP_MASK: u64 = 0xf << ID_AA64MMFR1_AFP_SHIFT;
/// `ID_AA64MMFR1_AFP_NONE`.
pub const ID_AA64MMFR1_AFP_NONE: u64 = 0x0 << ID_AA64MMFR1_AFP_SHIFT;
/// `ID_AA64MMFR1_AFP_IMPL`.
pub const ID_AA64MMFR1_AFP_IMPL: u64 = 0x1 << ID_AA64MMFR1_AFP_SHIFT;
/// `ID_AA64MMFR1_ECBHB_SHIFT`.
pub const ID_AA64MMFR1_ECBHB_SHIFT: u32 = 60;
/// `ID_AA64MMFR1_ECBHB_MASK`.
pub const ID_AA64MMFR1_ECBHB_MASK: u64 = 0xf << ID_AA64MMFR1_ECBHB_SHIFT;
/// `ID_AA64MMFR1_ECBHB_NONE`.
pub const ID_AA64MMFR1_ECBHB_NONE: u64 = 0x0 << ID_AA64MMFR1_ECBHB_SHIFT;
/// `ID_AA64MMFR1_ECBHB_IMPL`.
pub const ID_AA64MMFR1_ECBHB_IMPL: u64 = 0x1 << ID_AA64MMFR1_ECBHB_SHIFT;

// ID_AA64MMFR2_EL1

/// `ID_AA64MMFR2_MASK`.
pub const ID_AA64MMFR2_MASK: u64 = 0xffff0fffffffffff;
/// `ID_AA64MMFR2_CCIDX_SHIFT`.
pub const ID_AA64MMFR2_CCIDX_SHIFT: u32 = 20;
/// `ID_AA64MMFR2_CCIDX_MASK`.
pub const ID_AA64MMFR2_CCIDX_MASK: u64 = 0xf << ID_AA64MMFR2_CCIDX_SHIFT;
/// `ID_AA64MMFR2_CCIDX_IMPL`.
pub const ID_AA64MMFR2_CCIDX_IMPL: u64 = 0x1 << ID_AA64MMFR2_CCIDX_SHIFT;
/// `ID_AA64MMFR2_AT_SHIFT`.
pub const ID_AA64MMFR2_AT_SHIFT: u32 = 32;
/// `ID_AA64MMFR2_AT_MASK`.
pub const ID_AA64MMFR2_AT_MASK: u64 = 0xf << ID_AA64MMFR2_AT_SHIFT;
/// `ID_AA64MMFR2_AT_NONE`.
pub const ID_AA64MMFR2_AT_NONE: u64 = 0x0 << ID_AA64MMFR2_AT_SHIFT;
/// `ID_AA64MMFR2_AT_IMPL`.
pub const ID_AA64MMFR2_AT_IMPL: u64 = 0x1 << ID_AA64MMFR2_AT_SHIFT;
/// `ID_AA64MMFR2_IDS_SHIFT`.
pub const ID_AA64MMFR2_IDS_SHIFT: u32 = 36;
/// `ID_AA64MMFR2_IDS_MASK`.
pub const ID_AA64MMFR2_IDS_MASK: u64 = 0xf << ID_AA64MMFR2_IDS_SHIFT;
/// `ID_AA64MMFR2_IDS_NONE`.
pub const ID_AA64MMFR2_IDS_NONE: u64 = 0x0 << ID_AA64MMFR2_IDS_SHIFT;
/// `ID_AA64MMFR2_IDS_IMPL`.
pub const ID_AA64MMFR2_IDS_IMPL: u64 = 0x1 << ID_AA64MMFR2_IDS_SHIFT;

// ID_AA64PFR0_EL1

/// `ID_AA64PFR0_MASK`.
pub const ID_AA64PFR0_MASK: u64 = 0xff0fffffffffffff;
/// `ID_AA64PFR0_EL0_SHIFT`.
pub const ID_AA64PFR0_EL0_SHIFT: u32 = 0;
/// `ID_AA64PFR0_EL0_MASK`.
pub const ID_AA64PFR0_EL0_MASK: u64 = 0xf << ID_AA64PFR0_EL0_SHIFT;
/// `ID_AA64PFR0_EL0_64`.
pub const ID_AA64PFR0_EL0_64: u64 = 0x1 << ID_AA64PFR0_EL0_SHIFT;
/// `ID_AA64PFR0_EL0_64_32`.
pub const ID_AA64PFR0_EL0_64_32: u64 = 0x2 << ID_AA64PFR0_EL0_SHIFT;
/// `ID_AA64PFR0_EL1_SHIFT`.
pub const ID_AA64PFR0_EL1_SHIFT: u32 = 4;
/// `ID_AA64PFR0_EL1_MASK`.
pub const ID_AA64PFR0_EL1_MASK: u64 = 0xf << ID_AA64PFR0_EL1_SHIFT;
/// `ID_AA64PFR0_EL1_64`.
pub const ID_AA64PFR0_EL1_64: u64 = 0x1 << ID_AA64PFR0_EL1_SHIFT;
/// `ID_AA64PFR0_EL1_64_32`.
pub const ID_AA64PFR0_EL1_64_32: u64 = 0x2 << ID_AA64PFR0_EL1_SHIFT;
/// `ID_AA64PFR0_EL2_SHIFT`.
pub const ID_AA64PFR0_EL2_SHIFT: u32 = 8;
/// `ID_AA64PFR0_EL2_MASK`.
pub const ID_AA64PFR0_EL2_MASK: u64 = 0xf << ID_AA64PFR0_EL2_SHIFT;
/// `ID_AA64PFR0_EL2_NONE`.
pub const ID_AA64PFR0_EL2_NONE: u64 = 0x0 << ID_AA64PFR0_EL2_SHIFT;
/// `ID_AA64PFR0_EL2_64`.
pub const ID_AA64PFR0_EL2_64: u64 = 0x1 << ID_AA64PFR0_EL2_SHIFT;
/// `ID_AA64PFR0_EL2_64_32`.
pub const ID_AA64PFR0_EL2_64_32: u64 = 0x2 << ID_AA64PFR0_EL2_SHIFT;
/// `ID_AA64PFR0_EL3_SHIFT`.
pub const ID_AA64PFR0_EL3_SHIFT: u32 = 12;
/// `ID_AA64PFR0_EL3_MASK`.
pub const ID_AA64PFR0_EL3_MASK: u64 = 0xf << ID_AA64PFR0_EL3_SHIFT;
/// `ID_AA64PFR0_EL3_NONE`.
pub const ID_AA64PFR0_EL3_NONE: u64 = 0x0 << ID_AA64PFR0_EL3_SHIFT;
/// `ID_AA64PFR0_EL3_64`.
pub const ID_AA64PFR0_EL3_64: u64 = 0x1 << ID_AA64PFR0_EL3_SHIFT;
/// `ID_AA64PFR0_EL3_64_32`.
pub const ID_AA64PFR0_EL3_64_32: u64 = 0x2 << ID_AA64PFR0_EL3_SHIFT;
/// `ID_AA64PFR0_FP_SHIFT`.
pub const ID_AA64PFR0_FP_SHIFT: u32 = 16;
/// `ID_AA64PFR0_FP_MASK`.
pub const ID_AA64PFR0_FP_MASK: u64 = 0xf << ID_AA64PFR0_FP_SHIFT;
/// `ID_AA64PFR0_FP_IMPL`.
pub const ID_AA64PFR0_FP_IMPL: u64 = 0x0 << ID_AA64PFR0_FP_SHIFT;
/// `ID_AA64PFR0_FP_HP`.
pub const ID_AA64PFR0_FP_HP: u64 = 0x1 << ID_AA64PFR0_FP_SHIFT;
/// `ID_AA64PFR0_FP_NONE`.
pub const ID_AA64PFR0_FP_NONE: u64 = 0xf << ID_AA64PFR0_FP_SHIFT;
/// `ID_AA64PFR0_ADV_SIMD_SHIFT`.
pub const ID_AA64PFR0_ADV_SIMD_SHIFT: u32 = 20;
/// `ID_AA64PFR0_ADV_SIMD_MASK`.
pub const ID_AA64PFR0_ADV_SIMD_MASK: u64 = 0xf << ID_AA64PFR0_ADV_SIMD_SHIFT;
/// `ID_AA64PFR0_ADV_SIMD_IMPL`.
pub const ID_AA64PFR0_ADV_SIMD_IMPL: u64 = 0x0 << ID_AA64PFR0_ADV_SIMD_SHIFT;
/// `ID_AA64PFR0_ADV_SIMD_HP`.
pub const ID_AA64PFR0_ADV_SIMD_HP: u64 = 0x1 << ID_AA64PFR0_ADV_SIMD_SHIFT;
/// `ID_AA64PFR0_ADV_SIMD_NONE`.
pub const ID_AA64PFR0_ADV_SIMD_NONE: u64 = 0xf << ID_AA64PFR0_ADV_SIMD_SHIFT;
/// `ID_AA64PFR0_GIC_BITS`.
pub const ID_AA64PFR0_GIC_BITS: u64 = 0x4;
/// `ID_AA64PFR0_GIC_SHIFT`.
pub const ID_AA64PFR0_GIC_SHIFT: u32 = 24;
/// `ID_AA64PFR0_GIC_MASK`.
pub const ID_AA64PFR0_GIC_MASK: u64 = 0xf << ID_AA64PFR0_GIC_SHIFT;
/// `ID_AA64PFR0_GIC_CPUIF_NONE`.
pub const ID_AA64PFR0_GIC_CPUIF_NONE: u64 = 0x0 << ID_AA64PFR0_GIC_SHIFT;
/// `ID_AA64PFR0_GIC_CPUIF_EN`.
pub const ID_AA64PFR0_GIC_CPUIF_EN: u64 = 0x1 << ID_AA64PFR0_GIC_SHIFT;
/// `ID_AA64PFR0_RAS_SHIFT`.
pub const ID_AA64PFR0_RAS_SHIFT: u32 = 28;
/// `ID_AA64PFR0_RAS_MASK`.
pub const ID_AA64PFR0_RAS_MASK: u64 = 0xf << ID_AA64PFR0_RAS_SHIFT;
/// `ID_AA64PFR0_RAS_NONE`.
pub const ID_AA64PFR0_RAS_NONE: u64 = 0x0 << ID_AA64PFR0_RAS_SHIFT;
/// `ID_AA64PFR0_RAS_IMPL`.
pub const ID_AA64PFR0_RAS_IMPL: u64 = 0x1 << ID_AA64PFR0_RAS_SHIFT;
/// `ID_AA64PFR0_RAS_IMPL_V1P1`.
pub const ID_AA64PFR0_RAS_IMPL_V1P1: u64 = 0x2 << ID_AA64PFR0_RAS_SHIFT;
/// `ID_AA64PFR0_SVE_SHIFT`.
pub const ID_AA64PFR0_SVE_SHIFT: u32 = 32;
/// `ID_AA64PFR0_SVE_MASK`.
pub const ID_AA64PFR0_SVE_MASK: u64 = 0xf << ID_AA64PFR0_SVE_SHIFT;
/// `ID_AA64PFR0_SVE_NONE`.
pub const ID_AA64PFR0_SVE_NONE: u64 = 0x0 << ID_AA64PFR0_SVE_SHIFT;
/// `ID_AA64PFR0_SVE_IMPL`.
pub const ID_AA64PFR0_SVE_IMPL: u64 = 0x1 << ID_AA64PFR0_SVE_SHIFT;
/// `ID_AA64PFR0_SEL2_SHIFT`.
pub const ID_AA64PFR0_SEL2_SHIFT: u32 = 36;
/// `ID_AA64PFR0_SEL2_MASK`.
pub const ID_AA64PFR0_SEL2_MASK: u64 = 0xf << ID_AA64PFR0_SEL2_SHIFT;
/// `ID_AA64PFR0_SEL2_NONE`.
pub const ID_AA64PFR0_SEL2_NONE: u64 = 0x0 << ID_AA64PFR0_SEL2_SHIFT;
/// `ID_AA64PFR0_SEL2_IMPL`.
pub const ID_AA64PFR0_SEL2_IMPL: u64 = 0x1 << ID_AA64PFR0_SEL2_SHIFT;
/// `ID_AA64PFR0_MPAM_SHIFT`.
pub const ID_AA64PFR0_MPAM_SHIFT: u32 = 40;
/// `ID_AA64PFR0_MPAM_MASK`.
pub const ID_AA64PFR0_MPAM_MASK: u64 = 0xf << ID_AA64PFR0_MPAM_SHIFT;
/// `ID_AA64PFR0_MPAM_NONE`.
pub const ID_AA64PFR0_MPAM_NONE: u64 = 0x0 << ID_AA64PFR0_MPAM_SHIFT;
/// `ID_AA64PFR0_MPAM_IMPL`.
pub const ID_AA64PFR0_MPAM_IMPL: u64 = 0x1 << ID_AA64PFR0_MPAM_SHIFT;
/// `ID_AA64PFR0_AMU_SHIFT`.
pub const ID_AA64PFR0_AMU_SHIFT: u32 = 44;
/// `ID_AA64PFR0_AMU_MASK`.
pub const ID_AA64PFR0_AMU_MASK: u64 = 0xf << ID_AA64PFR0_AMU_SHIFT;
/// `ID_AA64PFR0_AMU_NONE`.
pub const ID_AA64PFR0_AMU_NONE: u64 = 0x0 << ID_AA64PFR0_AMU_SHIFT;
/// `ID_AA64PFR0_AMU_IMPL`.
pub const ID_AA64PFR0_AMU_IMPL: u64 = 0x1 << ID_AA64PFR0_AMU_SHIFT;
/// `ID_AA64PFR0_AMU_IMPL_V1P1`.
pub const ID_AA64PFR0_AMU_IMPL_V1P1: u64 = 0x2 << ID_AA64PFR0_AMU_SHIFT;
/// `ID_AA64PFR0_DIT_SHIFT`.
pub const ID_AA64PFR0_DIT_SHIFT: u32 = 48;
/// `ID_AA64PFR0_DIT_MASK`.
pub const ID_AA64PFR0_DIT_MASK: u64 = 0xf << ID_AA64PFR0_DIT_SHIFT;
/// `ID_AA64PFR0_DIT_UNKNOWN`.
pub const ID_AA64PFR0_DIT_UNKNOWN: u64 = 0x0 << ID_AA64PFR0_DIT_SHIFT;
/// `ID_AA64PFR0_DIT_IMPL`.
pub const ID_AA64PFR0_DIT_IMPL: u64 = 0x1 << ID_AA64PFR0_DIT_SHIFT;
/// `ID_AA64PFR0_CSV2_SHIFT`.
pub const ID_AA64PFR0_CSV2_SHIFT: u32 = 56;
/// `ID_AA64PFR0_CSV2_MASK`.
pub const ID_AA64PFR0_CSV2_MASK: u64 = 0xf << ID_AA64PFR0_CSV2_SHIFT;
/// `ID_AA64PFR0_CSV2_UNKNOWN`.
pub const ID_AA64PFR0_CSV2_UNKNOWN: u64 = 0x0 << ID_AA64PFR0_CSV2_SHIFT;
/// `ID_AA64PFR0_CSV2_IMPL`.
pub const ID_AA64PFR0_CSV2_IMPL: u64 = 0x1 << ID_AA64PFR0_CSV2_SHIFT;
/// `ID_AA64PFR0_CSV2_SCXT`.
pub const ID_AA64PFR0_CSV2_SCXT: u64 = 0x2 << ID_AA64PFR0_CSV2_SHIFT;
/// `ID_AA64PFR0_CSV2_HCXT`.
pub const ID_AA64PFR0_CSV2_HCXT: u64 = 0x3 << ID_AA64PFR0_CSV2_SHIFT;
/// `ID_AA64PFR0_CSV3_SHIFT`.
pub const ID_AA64PFR0_CSV3_SHIFT: u32 = 60;
/// `ID_AA64PFR0_CSV3_MASK`.
pub const ID_AA64PFR0_CSV3_MASK: u64 = 0xf << ID_AA64PFR0_CSV3_SHIFT;
/// `ID_AA64PFR0_CSV3_UNKNOWN`.
pub const ID_AA64PFR0_CSV3_UNKNOWN: u64 = 0x0 << ID_AA64PFR0_CSV3_SHIFT;
/// `ID_AA64PFR0_CSV3_IMPL`.
pub const ID_AA64PFR0_CSV3_IMPL: u64 = 0x1 << ID_AA64PFR0_CSV3_SHIFT;

// ID_AA64PFR1_EL1

/// `ID_AA64PFR1_MASK`.
pub const ID_AA64PFR1_MASK: u64 = 0x000000000000ffff;
/// `ID_AA64PFR1_BT_SHIFT`.
pub const ID_AA64PFR1_BT_SHIFT: u32 = 0;
/// `ID_AA64PFR1_BT_MASK`.
pub const ID_AA64PFR1_BT_MASK: u64 = 0xf << ID_AA64PFR1_BT_SHIFT;
/// `ID_AA64PFR1_BT_NONE`.
pub const ID_AA64PFR1_BT_NONE: u64 = 0x0 << ID_AA64PFR1_BT_SHIFT;
/// `ID_AA64PFR1_BT_IMPL`.
pub const ID_AA64PFR1_BT_IMPL: u64 = 0x1 << ID_AA64PFR1_BT_SHIFT;
/// `ID_AA64PFR1_SSBS_SHIFT`.
pub const ID_AA64PFR1_SSBS_SHIFT: u32 = 4;
/// `ID_AA64PFR1_SSBS_MASK`.
pub const ID_AA64PFR1_SSBS_MASK: u64 = 0xf << ID_AA64PFR1_SSBS_SHIFT;
/// `ID_AA64PFR1_SSBS_NONE`.
pub const ID_AA64PFR1_SSBS_NONE: u64 = 0x0 << ID_AA64PFR1_SSBS_SHIFT;
/// `ID_AA64PFR1_SSBS_PSTATE`.
pub const ID_AA64PFR1_SSBS_PSTATE: u64 = 0x1 << ID_AA64PFR1_SSBS_SHIFT;
/// `ID_AA64PFR1_SSBS_PSTATE_MSR`.
pub const ID_AA64PFR1_SSBS_PSTATE_MSR: u64 = 0x2 << ID_AA64PFR1_SSBS_SHIFT;
/// `ID_AA64PFR1_MTE_SHIFT`.
pub const ID_AA64PFR1_MTE_SHIFT: u32 = 8;
/// `ID_AA64PFR1_MTE_MASK`.
pub const ID_AA64PFR1_MTE_MASK: u64 = 0xf << ID_AA64PFR1_MTE_SHIFT;
/// `ID_AA64PFR1_MTE_NONE`.
pub const ID_AA64PFR1_MTE_NONE: u64 = 0x0 << ID_AA64PFR1_MTE_SHIFT;
/// `ID_AA64PFR1_MTE_IMPL`.
pub const ID_AA64PFR1_MTE_IMPL: u64 = 0x1 << ID_AA64PFR1_MTE_SHIFT;
/// `ID_AA64PFR1_RAS_FRAC_SHIFT`.
pub const ID_AA64PFR1_RAS_FRAC_SHIFT: u32 = 12;
/// `ID_AA64PFR1_RAS_FRAC_MASK`.
pub const ID_AA64PFR1_RAS_FRAC_MASK: u64 = 0xf << ID_AA64PFR1_RAS_FRAC_SHIFT;
/// `ID_AA64PFR1_RAS_FRAC_NONE`.
pub const ID_AA64PFR1_RAS_FRAC_NONE: u64 = 0x0 << ID_AA64PFR1_RAS_FRAC_SHIFT;
/// `ID_AA64PFR1_RAS_FRAC_IMPL`.
pub const ID_AA64PFR1_RAS_FRAC_IMPL: u64 = 0x1 << ID_AA64PFR1_RAS_FRAC_SHIFT;

// ID_AA64ZFR0_EL1

/// `ID_AA64ZFR0_MASK`.
pub const ID_AA64ZFR0_MASK: u64 = 0x0ff0ff0f00ff00ff;
/// `ID_AA64ZFR0_SVEVER_SHIFT`.
pub const ID_AA64ZFR0_SVEVER_SHIFT: u32 = 0;
/// `ID_AA64ZFR0_SVEVER_MASK`.
pub const ID_AA64ZFR0_SVEVER_MASK: u64 = 0xf << ID_AA64ZFR0_SVEVER_SHIFT;
/// `ID_AA64ZFR0_SVEVER_SVE1`.
pub const ID_AA64ZFR0_SVEVER_SVE1: u64 = 0x0 << ID_AA64ZFR0_SVEVER_SHIFT;
/// `ID_AA64ZFR0_SVEVER_SVE2`.
pub const ID_AA64ZFR0_SVEVER_SVE2: u64 = 0x1 << ID_AA64ZFR0_SVEVER_SHIFT;
/// `ID_AA64ZFR0_SVEVER_SVE2P1`.
pub const ID_AA64ZFR0_SVEVER_SVE2P1: u64 = 0x2 << ID_AA64ZFR0_SVEVER_SHIFT;
/// `ID_AA64ZFR0_AES_SHIFT`.
pub const ID_AA64ZFR0_AES_SHIFT: u32 = 4;
/// `ID_AA64ZFR0_AES_MASK`.
pub const ID_AA64ZFR0_AES_MASK: u64 = 0xf << ID_AA64ZFR0_AES_SHIFT;
/// `ID_AA64ZFR0_AES_NONE`.
pub const ID_AA64ZFR0_AES_NONE: u64 = 0x0 << ID_AA64ZFR0_AES_SHIFT;
/// `ID_AA64ZFR0_AES_BASE`.
pub const ID_AA64ZFR0_AES_BASE: u64 = 0x1 << ID_AA64ZFR0_AES_SHIFT;
/// `ID_AA64ZFR0_AES_PMULL`.
pub const ID_AA64ZFR0_AES_PMULL: u64 = 0x2 << ID_AA64ZFR0_AES_SHIFT;
/// `ID_AA64ZFR0_BITPERM_SHIFT`.
pub const ID_AA64ZFR0_BITPERM_SHIFT: u32 = 16;
/// `ID_AA64ZFR0_BITPERM_MASK`.
pub const ID_AA64ZFR0_BITPERM_MASK: u64 = 0xf << ID_AA64ZFR0_BITPERM_SHIFT;
/// `ID_AA64ZFR0_BITPERM_NONE`.
pub const ID_AA64ZFR0_BITPERM_NONE: u64 = 0x0 << ID_AA64ZFR0_BITPERM_SHIFT;
/// `ID_AA64ZFR0_BITPERM_IMPL`.
pub const ID_AA64ZFR0_BITPERM_IMPL: u64 = 0x1 << ID_AA64ZFR0_BITPERM_SHIFT;
/// `ID_AA64ZFR0_BF16_SHIFT`.
pub const ID_AA64ZFR0_BF16_SHIFT: u32 = 20;
/// `ID_AA64ZFR0_BF16_MASK`.
pub const ID_AA64ZFR0_BF16_MASK: u64 = 0xf << ID_AA64ZFR0_BF16_SHIFT;
/// `ID_AA64ZFR0_BF16_NONE`.
pub const ID_AA64ZFR0_BF16_NONE: u64 = 0x0 << ID_AA64ZFR0_BF16_SHIFT;
/// `ID_AA64ZFR0_BF16_BASE`.
pub const ID_AA64ZFR0_BF16_BASE: u64 = 0x1 << ID_AA64ZFR0_BF16_SHIFT;
/// `ID_AA64ZFR0_BF16_EBF`.
pub const ID_AA64ZFR0_BF16_EBF: u64 = 0x2 << ID_AA64ZFR0_BF16_SHIFT;
/// `ID_AA64ZFR0_SHA3_SHIFT`.
pub const ID_AA64ZFR0_SHA3_SHIFT: u32 = 32;
/// `ID_AA64ZFR0_SHA3_MASK`.
pub const ID_AA64ZFR0_SHA3_MASK: u64 = 0xf << ID_AA64ZFR0_SHA3_SHIFT;
/// `ID_AA64ZFR0_SHA3_NONE`.
pub const ID_AA64ZFR0_SHA3_NONE: u64 = 0x0 << ID_AA64ZFR0_SHA3_SHIFT;
/// `ID_AA64ZFR0_SHA3_IMPL`.
pub const ID_AA64ZFR0_SHA3_IMPL: u64 = 0x1 << ID_AA64ZFR0_SHA3_SHIFT;
/// `ID_AA64ZFR0_SM4_SHIFT`.
pub const ID_AA64ZFR0_SM4_SHIFT: u32 = 40;
/// `ID_AA64ZFR0_SM4_MASK`.
pub const ID_AA64ZFR0_SM4_MASK: u64 = 0xf << ID_AA64ZFR0_SM4_SHIFT;
/// `ID_AA64ZFR0_SM4_NONE`.
pub const ID_AA64ZFR0_SM4_NONE: u64 = 0x0 << ID_AA64ZFR0_SM4_SHIFT;
/// `ID_AA64ZFR0_SM4_IMPL`.
pub const ID_AA64ZFR0_SM4_IMPL: u64 = 0x1 << ID_AA64ZFR0_SM4_SHIFT;
/// `ID_AA64ZFR0_I8MM_SHIFT`.
pub const ID_AA64ZFR0_I8MM_SHIFT: u32 = 44;
/// `ID_AA64ZFR0_I8MM_MASK`.
pub const ID_AA64ZFR0_I8MM_MASK: u64 = 0xf << ID_AA64ZFR0_I8MM_SHIFT;
/// `ID_AA64ZFR0_I8MM_NONE`.
pub const ID_AA64ZFR0_I8MM_NONE: u64 = 0x0 << ID_AA64ZFR0_I8MM_SHIFT;
/// `ID_AA64ZFR0_I8MM_IMPL`.
pub const ID_AA64ZFR0_I8MM_IMPL: u64 = 0x1 << ID_AA64ZFR0_I8MM_SHIFT;
/// `ID_AA64ZFR0_F32MM_SHIFT`.
pub const ID_AA64ZFR0_F32MM_SHIFT: u32 = 52;
/// `ID_AA64ZFR0_F32MM_MASK`.
pub const ID_AA64ZFR0_F32MM_MASK: u64 = 0xf << ID_AA64ZFR0_F32MM_SHIFT;
/// `ID_AA64ZFR0_F32MM_NONE`.
pub const ID_AA64ZFR0_F32MM_NONE: u64 = 0x0 << ID_AA64ZFR0_F32MM_SHIFT;
/// `ID_AA64ZFR0_F32MM_IMPL`.
pub const ID_AA64ZFR0_F32MM_IMPL: u64 = 0x1 << ID_AA64ZFR0_F32MM_SHIFT;
/// `ID_AA64ZFR0_F64MM_SHIFT`.
pub const ID_AA64ZFR0_F64MM_SHIFT: u32 = 56;
/// `ID_AA64ZFR0_F64MM_MASK`.
pub const ID_AA64ZFR0_F64MM_MASK: u64 = 0xf << ID_AA64ZFR0_F64MM_SHIFT;
/// `ID_AA64ZFR0_F64MM_NONE`.
pub const ID_AA64ZFR0_F64MM_NONE: u64 = 0x0 << ID_AA64ZFR0_F64MM_SHIFT;
/// `ID_AA64ZFR0_F64MM_IMPL`.
pub const ID_AA64ZFR0_F64MM_IMPL: u64 = 0x1 << ID_AA64ZFR0_F64MM_SHIFT;

/* SCTLR_EL1 - System Control Register */

/// `SCTLR_RES0`: reserved, write 0.
pub const SCTLR_RES0: u64 = 0xffff_ffff_c822_2400;
/// `SCTLR_RES1`: reserved, write 1.
pub const SCTLR_RES1: u64 = 0x0000_0000_30d0_0800;
/// `SCTLR_M`: the MMU.
pub const SCTLR_M: u64 = 0x0000_0000_0000_0001;
/// `SCTLR_A`: alignment checks.
pub const SCTLR_A: u64 = 0x0000_0000_0000_0002;
/// `SCTLR_C`: the data cache.
pub const SCTLR_C: u64 = 0x0000_0000_0000_0004;
/// `SCTLR_SA`: stack alignment checks at EL1.
pub const SCTLR_SA: u64 = 0x0000_0000_0000_0008;
/// `SCTLR_SA0`: stack alignment checks at EL0.
pub const SCTLR_SA0: u64 = 0x0000_0000_0000_0010;
/// `SCTLR_CP15BEN`.
pub const SCTLR_CP15BEN: u64 = 0x0000_0000_0000_0020;
/// `SCTLR_THEE`.
pub const SCTLR_THEE: u64 = 0x0000_0000_0000_0040;
/// `SCTLR_ITD`.
pub const SCTLR_ITD: u64 = 0x0000_0000_0000_0080;
/// `SCTLR_SED`.
pub const SCTLR_SED: u64 = 0x0000_0000_0000_0100;
/// `SCTLR_UMA`.
pub const SCTLR_UMA: u64 = 0x0000_0000_0000_0200;
/// `SCTLR_I`: the instruction cache.
pub const SCTLR_I: u64 = 0x0000_0000_0000_1000;
/// `SCTLR_EnDB`: pointer authentication with the DB key.
#[allow(non_upper_case_globals)]
pub const SCTLR_EnDB: u64 = 0x0000_0000_0000_2000;
/// `SCTLR_DZE`.
pub const SCTLR_DZE: u64 = 0x0000_0000_0000_4000;
/// `SCTLR_UCT`.
pub const SCTLR_UCT: u64 = 0x0000_0000_0000_8000;
/// `SCTLR_nTWI`.
#[allow(non_upper_case_globals)]
pub const SCTLR_nTWI: u64 = 0x0000_0000_0001_0000;
/// `SCTLR_nTWE`.
#[allow(non_upper_case_globals)]
pub const SCTLR_nTWE: u64 = 0x0000_0000_0004_0000;
/// `SCTLR_WXN`.
pub const SCTLR_WXN: u64 = 0x0000_0000_0008_0000;
/// `SCTLR_SPAN`: set PAN on exception entry when clear.
pub const SCTLR_SPAN: u64 = 0x0000_0000_0080_0000;
/// `SCTLR_EOE`.
pub const SCTLR_EOE: u64 = 0x0000_0000_0100_0000;
/// `SCTLR_EE`.
pub const SCTLR_EE: u64 = 0x0000_0000_0200_0000;
/// `SCTLR_UCI`.
pub const SCTLR_UCI: u64 = 0x0000_0000_0400_0000;
/// `SCTLR_EnDA`: pointer authentication with the DA key.
#[allow(non_upper_case_globals)]
pub const SCTLR_EnDA: u64 = 0x0000_0000_0800_0000;
/// `SCTLR_EnIB`: pointer authentication with the IB key.
#[allow(non_upper_case_globals)]
pub const SCTLR_EnIB: u64 = 0x0000_0000_4000_0000;
/// `SCTLR_EnIA`: pointer authentication with the IA key.
#[allow(non_upper_case_globals)]
pub const SCTLR_EnIA: u64 = 0x0000_0000_8000_0000;
/// `SCTLR_BT0`: PACIxSP are not compatible with BTI at EL0.
pub const SCTLR_BT0: u64 = 0x0000_0008_0000_0000;
/// `SCTLR_BT1`: the same at EL1.
pub const SCTLR_BT1: u64 = 0x0000_0010_0000_0000;
/// `SCTLR_EPAN`: enhanced PAN.
pub const SCTLR_EPAN: u64 = 0x0200_0000_0000_0000;
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icc_ctlr_pribits() {
        // Five priority bits: PRIbits reads 4.
        assert_eq!(icc_ctlr_el1_pribits(0x0000_0400), 4);
        assert_eq!(icc_ctlr_el1_pribits(!ICC_CTLR_EL1_PRIBITS_MASK), 0);
    }

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
            ("ICC_CTLR_EL1_EOIMODE", ICC_CTLR_EL1_EOIMODE as i64),
            (
                "ICC_CTLR_EL1_PRIBITS_SHIFT",
                ICC_CTLR_EL1_PRIBITS_SHIFT as i64,
            ),
            ("ICC_IAR1_EL1_SPUR", ICC_IAR1_EL1_SPUR as i64),
            ("ICC_SGI1R_EL1_TL_MASK", ICC_SGI1R_EL1_TL_MASK as i64),
            ("ICC_SGI1R_EL1_AFF1_SHIFT", ICC_SGI1R_EL1_AFF1_SHIFT as i64),
            (
                "ICC_SGI1R_EL1_SGIID_SHIFT",
                ICC_SGI1R_EL1_SGIID_SHIFT as i64,
            ),
            ("ICC_SGI1R_EL1_AFF2_SHIFT", ICC_SGI1R_EL1_AFF2_SHIFT as i64),
            ("ICC_SGI1R_EL1_AFF3_SHIFT", ICC_SGI1R_EL1_AFF3_SHIFT as i64),
        ];
        let defs = crate::reftest::defines("sys/arch/arm64/include/armreg.h");
        for (name, value) in want {
            assert_eq!(crate::reftest::int(&defs, name), Some(value), "{name}");
        }
    }
}
/* </TESTS> */
