/*	$OpenBSD: specialreg.h,v 1.129 2026/09/19 16:11:07 mlarkin Exp $	*/
/*	$NetBSD: specialreg.h,v 1.1 2003/04/26 18:39:48 fvdl Exp $	*/
/*	$NetBSD: x86/specialreg.h,v 1.2 2003/04/25 21:54:30 fvdl Exp $	*/

/*-
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
 *
 *	@(#)specialreg.h	7.1 (Berkeley) 5/9/91
 */

//! amd64 `<machine/specialreg.h>`: control registers, MSRs and CPUID bits.
//!
//! Upstream: sys/arch/amd64/include/specialreg.h @ 3ce1f3f79392
//!
//! Status: `wip`. Milestones M3 and M4 port the `CR0`/`CR3` bits, `MSR_EFER` and the
//! `syscall`/segment-base MSRs; the CPUID feature words, the remaining MSRs and the MTRR/PAT
//! definitions arrive with CPU identification.

// Bits in 386 special registers:

/// `CR0_PE`: Protected mode Enable.
pub const CR0_PE: u64 = 0x0000_0001;
/// `CR0_MP`: "Math" Present (NPX or NPX emulator).
pub const CR0_MP: u64 = 0x0000_0002;
/// `CR0_EM`: EMulate non-NPX coproc. (trap ESC only).
pub const CR0_EM: u64 = 0x0000_0004;
/// `CR0_TS`: Task Switched (if MP, trap ESC and WAIT).
pub const CR0_TS: u64 = 0x0000_0008;
/// `CR0_ET`: Extension Type (387 (if set) vs 287).
pub const CR0_ET: u64 = 0x0000_0010;
/// `CR0_PG`: PaGing enable.
pub const CR0_PG: u64 = 0x8000_0000;

/// `CR3_REUSE_PCID`: do not flush the PCID's TLB entries on load.
pub const CR3_REUSE_PCID: u64 = 1 << 63;
/// `CR3_PADDR`: the page-table address bits of `CR3`.
pub const CR3_PADDR: u64 = 0x7fff_ffff_ffff_f000;

/// `MSR_APICBASE`: the local APIC's base address and mode.
pub const MSR_APICBASE: u32 = 0x01b;
/// `APICBASE_BSP`.
pub const APICBASE_BSP: u64 = 0x100;
/// `APICBASE_ENABLE_X2APIC`.
pub const APICBASE_ENABLE_X2APIC: u64 = 0x400;
/// `APICBASE_GLOBAL_ENABLE`.
pub const APICBASE_GLOBAL_ENABLE: u64 = 0x800;
/// `APICBASE_ADDRESS_MASK`.
pub const APICBASE_ADDRESS_MASK: u64 = 0x000f_ffff_ffff_f000;
/// `MSR_EFER`: Extended feature enable.
pub const MSR_EFER: u32 = 0xc000_0080;
/// `MSR_STAR`: the `syscall`/`sysret` segment selectors.
pub const MSR_STAR: u32 = 0xc000_0081;
/// `MSR_LSTAR`: the 64-bit `syscall` entry point.
pub const MSR_LSTAR: u32 = 0xc000_0082;
/// `MSR_CSTAR`: the compatibility-mode `syscall` entry point.
pub const MSR_CSTAR: u32 = 0xc000_0083;
/// `MSR_SFMASK`: the `RFLAGS` bits `syscall` clears.
pub const MSR_SFMASK: u32 = 0xc000_0084;
/// `MSR_FSBASE`: the `FS` segment base.
pub const MSR_FSBASE: u32 = 0xc000_0100;
/// `MSR_GSBASE`: the `GS` segment base.
pub const MSR_GSBASE: u32 = 0xc000_0101;
/// `MSR_KERNELGSBASE`: the `GS` base `swapgs` swaps in.
pub const MSR_KERNELGSBASE: u32 = 0xc000_0102;
/// `EFER_SCE`: SYSCALL extension.
pub const EFER_SCE: u64 = 0x0000_0001;
/// `EFER_LME`: Long Mode Enabled.
pub const EFER_LME: u64 = 0x0000_0100;
/// `EFER_LMA`: Long Mode Active.
pub const EFER_LMA: u64 = 0x0000_0400;
/// `EFER_NXE`: No-Execute Enabled.
pub const EFER_NXE: u64 = 0x0000_0800;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/arch/amd64/include/specialreg.h");
        let ours: &[(&str, i64)] = &[
            ("CR0_PE", CR0_PE as i64),
            ("CR0_PG", CR0_PG as i64),
            ("CR3_PADDR", CR3_PADDR as i64),
            ("MSR_EFER", i64::from(MSR_EFER)),
            ("EFER_SCE", EFER_SCE as i64),
            ("EFER_LME", EFER_LME as i64),
            ("EFER_LMA", EFER_LMA as i64),
            ("EFER_NXE", EFER_NXE as i64),
            ("MSR_APICBASE", i64::from(MSR_APICBASE)),
            ("APICBASE_ENABLE_X2APIC", APICBASE_ENABLE_X2APIC as i64),
            ("MSR_STAR", i64::from(MSR_STAR)),
            ("MSR_LSTAR", i64::from(MSR_LSTAR)),
            ("MSR_FSBASE", i64::from(MSR_FSBASE)),
            ("MSR_GSBASE", i64::from(MSR_GSBASE)),
            ("MSR_KERNELGSBASE", i64::from(MSR_KERNELGSBASE)),
        ];
        for (name, value) in ours {
            assert_eq!(crate::reftest::int(&defs, name), Some(*value), "{name}");
        }
    }
}
