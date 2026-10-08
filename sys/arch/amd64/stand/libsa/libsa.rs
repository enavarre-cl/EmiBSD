/*	$OpenBSD: libsa.h,v 1.8 2023/02/23 19:48:22 miod Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1996-1999 Michael Shalayeff
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
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR ``AS IS'' AND ANY EXPRESS OR
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES
 * OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED.
 * IN NO EVENT SHALL THE AUTHOR OR HIS RELATIVES BE LIABLE FOR ANY DIRECT,
 * INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES
 * (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
 * SERVICES; LOSS OF MIND, USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT,
 * STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING
 * IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF
 * THE POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! `libsa.h` of amd64's boot programs: the probe list type and what efiboot's files share;
//! and the `CPUID` macros of `<machine/specialreg.h>` they use.
//!
//! Upstream: sys/arch/amd64/stand/libsa/libsa.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - The prototypes are the definitions' own (`machdep.rs`, `memprobe.rs`, `diskprobe.rs`,
//!   `dev_i386.rs`); the BIOS-only ones (`gateA20`, `smpprobe`, `pciprobe`, `apmprobe`,
//!   `getSYSCONFaddr`, `getEBDAaddr`, `ps2model`) belong to the BIOS boot programs, which
//!   are not ported. `MACHINE_CMD` and `CHECK_SKIP_CONF` are boot(8)'s `BootMd` members.
//! - `CPUID(code, eax, ebx, ecx, edx)` returns the four registers ([`cpuid`]),
//!   `docs/C_TO_RUST.md`'s row for output-lvalue macros.

use core::arch::x86_64::__cpuid_count;

/// `struct i386_boot_probes`: a named list of probe routines `machdep()` runs.
pub struct I386BootProbes {
    /// `name`.
    pub name: &'static str,
    /// `probes`.
    pub probes: &'static [fn()],
}

/// `CPUIDECX_HV` (`<machine/specialreg.h>`): running under a hypervisor.
pub const CPUIDECX_HV: u32 = 0x8000_0000;
/// `CPUIDECX_RDRAND`.
pub const CPUIDECX_RDRAND: u32 = 0x4000_0000;
/// `SEFF0EBX_RDSEED`.
pub const SEFF0EBX_RDSEED: u32 = 0x0004_0000;

/// `CPUID_LEAF(code, leaf, ...)`: the four registers.
pub fn cpuid_leaf(code: u32, leaf: u32) -> (u32, u32, u32, u32) {
    let r = __cpuid_count(code, leaf);
    (r.eax, r.ebx, r.ecx, r.edx)
}

/// `CPUID(code, ...)`.
pub fn cpuid(code: u32) -> (u32, u32, u32, u32) {
    cpuid_leaf(code, 0)
}
/* </CODE> */
