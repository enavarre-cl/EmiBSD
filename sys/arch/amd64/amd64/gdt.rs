/*	$OpenBSD: gdt.c,v 1.26 2018/02/21 19:24:15 guenther Exp $	*/
/*	$NetBSD: gdt.c,v 1.1 2003/04/26 18:39:28 fvdl Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (c) 1996, 1997 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by John T. Kohl and Charles M. Hannum.
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
/* </LICENSES> */

/* <CODE> */
//! Per-CPU global descriptor tables: `arch/amd64/amd64/gdt.c`.
//!
//! Upstream: sys/arch/amd64/amd64/gdt.c @ 3ce1f3f79392
//!
//! `gdt_init_cpu` points the CPU's own GDT (`ci_gdt`, a copy of the boot CPU's that
//! `cpu_attach` made) at its own TSS and loads both. The boot CPU's GDT is built by
//! `init_x86_64` (`machdep.rs`); an application processor calls this from `cpu_hatch`.
//!
//! ## Deviations
//! - `lgdt` (`locore.S`) also reloads the data and code selectors, which the C's `lgdt` does
//!   too; an application processor arrives with the bootloader's GDT (`stand`), so this is
//!   what moves it onto the kernel's selectors.

use core::ptr;

use crate::arch::amd64::amd64::locore::lgdt;
use crate::arch::amd64::amd64::machdep::{set_sys_segment, setregion};
use crate::arch::amd64::include::cpu::CpuInfo;
use crate::arch::amd64::include::cpufunc::ltr;
use crate::arch::amd64::include::segments::{
    GDT_SIZE, GPROC0_SEL, RegionDescriptor, SDT_SYS386TSS, SEL_KPL, gdt_addr_sys, gsyssel,
};
use crate::arch::amd64::include::tss::X86_64Tss;

/// `gdt_init_cpu`: load appropriate gdt descriptor; we better be running on `*ci`.
///
/// # Safety
///
/// Call on `ci`'s own CPU, with `ci_gdt` a copy of the boot CPU's GDT and `ci_tss` its TSS,
/// both living as long as the CPU runs, and the TSS not loaded on any other CPU.
pub unsafe fn gdt_init_cpu(ci: &CpuInfo) {
    let gdt = ci.ci_gdt.get().cast_mut().cast::<u64>();
    let sys = gdt_addr_sys(GPROC0_SEL);
    // SAFETY: the caller's guarantee: `ci_gdt` is this CPU's own GDT of `GDT_SIZE` bytes, not
    // loaded yet, so nothing else reads or writes its TSS descriptor.
    let words = unsafe { core::slice::from_raw_parts_mut(gdt, GDT_SIZE / 8) };
    set_sys_segment(
        &mut words[sys..sys + 2],
        ci.ci_tss.get() as usize,
        size_of::<X86_64Tss>() - 1,
        SDT_SYS386TSS,
        SEL_KPL,
        false,
    );

    let mut region = RegionDescriptor {
        rd_limit: 0,
        rd_base: 0,
    };
    setregion(&mut region, gdt as usize, (GDT_SIZE - 1) as u16);
    // SAFETY: the GDT is a copy of the boot CPU's, with valid 64-bit kernel code and data
    // segments at GCODE_SEL/GDATA_SEL, and it lives as long as the CPU (the caller's
    // guarantee). The TSS descriptor just written is "available", as `ltr` needs.
    unsafe {
        lgdt(ptr::from_ref(&region));
        ltr(gsyssel(GPROC0_SEL, SEL_KPL));
    }
}
/* </CODE> */
