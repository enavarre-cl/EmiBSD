/*	$OpenBSD: lapic.c,v 1.77 2025/12/30 15:21:05 kettenis Exp $	*/
/* $NetBSD: lapic.c,v 1.2 2003/05/08 01:04:35 fvdl Exp $ */
/*-
 * Copyright (c) 2000 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by RedBack Networks Inc.
 *
 * Author: Bill Sommerfeld
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

//! The local APIC: `arch/amd64/amd64/lapic.c`.
//!
//! Upstream: sys/arch/amd64/amd64/lapic.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M4 ports `local_pic`, the register accessors (`i82489_*`,
//! `x2apic_*`, `lapic_readreg`/`lapic_writereg`, `lapic_cpu_number`), `lapic_map`,
//! `lapic_enable`, `lapic_disable`, `lapic_set_lvt`, `lapic_boot_init`, `lapic_hwmask`,
//! `lapic_hwunmask` and `lapic_setup`. The timer (`lapic_calibrate_timer`, `lapic_clockintr`,
//! `lapic_initclocks`, `lapic_timer_*`, `lapic_startclock`) and the IPIs (`x86_ipi*`,
//! `i82489_ipi`, `x2apic_ipi`) come with M5.
//!
//! ## Deviations
//! - `lapic_map` maps the page with `pmap_kenter_pa` (`PMAP_NOCACHE`) instead of whapping the
//!   PTE by hand: there is no TLB shootdown to avoid yet. `pmap_enter_special` (the u-k
//!   mapping for the Meltdown trampoline) is M6. x2APIC mode is taken only when the firmware
//!   enabled it (`cpu_ecxfeature` arrives with CPU identification, M4-b), and the `CODEPATCH`
//!   of the EOI is not there.
//! - `lapic_set_lvt`: the MP/ACPI interrupt tables (`mp_intrs`, M5) do not exist, so LINT0 is
//!   programmed as ExtINT and LINT1 as NMI, the MP specification's default configuration
//!   (what `mpbios` would record); the firmware leaves LINT0 masked, which would cut the
//!   8259 off. The AMD C1E workaround needs `ci_vendor`/`ci_family` (M4-b).
//! - `lapic_boot_init` reserves `LAPIC_TIMER_VECTOR` but points no gate at it until the
//!   timer stub exists (M5).

use core::cell::UnsafeCell;
use core::ptr;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use libkern::StaticCell;

use crate::arch::amd64::amd64::machdep::{IDT_ALLOCMAP, idt_vec_set};
use crate::arch::amd64::amd64::vector::Xintrspurious;
use crate::arch::amd64::include::cpu::CpuInfo;
use crate::arch::amd64::include::cpufunc::{intr_disable, intr_restore, rdmsr, wrmsr};
use crate::arch::amd64::include::i82489reg::{
    LAPIC_DLMODE_EXTINT, LAPIC_DLMODE_NMI, LAPIC_ID, LAPIC_ID_SHIFT, LAPIC_LVINT0, LAPIC_LVINT1,
    LAPIC_LVT_MASKED, LAPIC_LVTT, LAPIC_SVR, LAPIC_SVR_ENABLE, MSR_X2APIC_BASE,
};
use crate::arch::amd64::include::i82489var::{LAPIC_SPURIOUS_VECTOR, LAPIC_TIMER_VECTOR};
use crate::arch::amd64::include::param::PAGE_SIZE;
use crate::arch::amd64::include::pic::{PIC_LAPIC, Pic};
use crate::arch::amd64::include::pmap::PMAP_NOCACHE;
use crate::arch::amd64::include::specialreg::{APICBASE_ENABLE_X2APIC, MSR_APICBASE};
use crate::kern::subr_evcount::{evcount_attach, evcount_percpu};
use crate::machine::pmap::{pmap_kenter_pa, pmap_kernel, pmap_update};
use crate::sys::evcount::Evcount;
use crate::sys::mman::{PROT_READ, PROT_WRITE};
use crate::sys::types::{Paddr, Vaddr};
use crate::unported;

/// `local_apic`: the page the LAPIC's registers are mapped over (`locore.S` reserves it in
/// `.data`; here it is a page-aligned static the mapping replaces).
#[repr(C, align(4096))]
pub struct LocalApicPage(UnsafeCell<[u32; PAGE_SIZE / 4]>);

// SAFETY: the page is never read or written as memory: `lapic_map` points its PTE at the
// LAPIC's registers, which the accessors read and write with volatile accesses.
unsafe impl Sync for LocalApicPage {}

/// `local_apic`.
#[unsafe(export_name = "local_apic")]
pub static LOCAL_APIC: LocalApicPage = LocalApicPage(UnsafeCell::new([0; PAGE_SIZE / 4]));

/// `clk_count`: the clock interrupt counter.
pub static CLK_COUNT: Evcount = Evcount::new();
/// `clk_irq`: the counter's user data.
static CLK_IRQ: AtomicU64 = AtomicU64::new(0);

/// `local_pic`: the LAPIC as a `struct pic`.
pub static LOCAL_PIC: Pic = Pic {
    pic_name: "lapic",
    pic_type: PIC_LAPIC,
    pic_hwmask: Some(lapic_hwmask),
    pic_hwunmask: Some(lapic_hwunmask),
    pic_addroute: Some(lapic_setup),
    pic_delroute: Some(lapic_setup),
    pic_allocidtvec: None,
    pic_level_stubs: None,
    pic_edge_stubs: None,
};

/// `x2apic_enabled`.
pub static X2APIC_ENABLED: AtomicBool = AtomicBool::new(false);

/// The LAPIC register at `reg`, as an MMIO pointer.
fn lapic_reg_ptr(reg: i32) -> *mut u32 {
    LOCAL_APIC
        .0
        .get()
        .cast::<u8>()
        .wrapping_add(reg as usize)
        .cast::<u32>()
}

/// `i82489_readreg`.
pub fn i82489_readreg(reg: i32) -> u32 {
    // SAFETY: `lapic_map` has mapped the LAPIC's register page over `local_apic`; the
    // registers are read with volatile accesses.
    unsafe { ptr::read_volatile(lapic_reg_ptr(reg)) }
}

/// `i82489_cpu_number`.
pub fn i82489_cpu_number() -> u32 {
    i82489_readreg(LAPIC_ID) >> LAPIC_ID_SHIFT
}

/// `i82489_writereg`.
pub fn i82489_writereg(reg: i32, val: u32) {
    // SAFETY: as for `i82489_readreg`.
    unsafe { ptr::write_volatile(lapic_reg_ptr(reg), val) };
}

/// `x2apic_readreg`.
pub fn x2apic_readreg(reg: i32) -> u32 {
    // SAFETY: the x2APIC MSRs exist when x2APIC mode is enabled, which is when this is used.
    unsafe { rdmsr(MSR_X2APIC_BASE + (reg as u32 >> 4)) as u32 }
}

/// `x2apic_cpu_number`.
pub fn x2apic_cpu_number() -> u32 {
    x2apic_readreg(LAPIC_ID)
}

/// `x2apic_writereg`.
pub fn x2apic_writereg(reg: i32, val: u32) {
    // SAFETY: as for `x2apic_readreg`.
    unsafe { wrmsr(MSR_X2APIC_BASE + (reg as u32 >> 4), u64::from(val)) };
}

/// `lapic_readreg`: the accessor in use (MMIO or x2APIC).
static LAPIC_READREG: StaticCell<fn(i32) -> u32> = StaticCell::new(i82489_readreg);
/// `lapic_writereg`.
static LAPIC_WRITEREG: StaticCell<fn(i32, u32)> = StaticCell::new(i82489_writereg);

/// `lapic_readreg(reg)`.
pub fn lapic_readreg(reg: i32) -> u32 {
    // SAFETY: written once by `lapic_map` on the boot CPU before any read.
    (unsafe { LAPIC_READREG.read() })(reg)
}

/// `lapic_writereg(reg, val)`.
pub fn lapic_writereg(reg: i32, val: u32) {
    // SAFETY: as for `lapic_readreg`.
    (unsafe { LAPIC_WRITEREG.read() })(reg, val)
}

/// `lapic_cpu_number`.
pub fn lapic_cpu_number() -> u32 {
    if X2APIC_ENABLED.load(Ordering::Relaxed) {
        return x2apic_cpu_number();
    }
    i82489_cpu_number()
}

/// `lapic_map`: maps the LAPIC's registers at `lapic_base` over `local_apic`, or switches
/// to x2APIC mode (see the module's deviations).
pub fn lapic_map(lapic_base: Paddr) {
    let s = intr_disable();
    // SAFETY: MSR_APICBASE exists on every CPU with a local APIC.
    let msr = unsafe { rdmsr(MSR_APICBASE) };
    let va = Vaddr::new(ptr::addr_of!(LOCAL_APIC) as usize);
    if msr & APICBASE_ENABLE_X2APIC != 0 {
        // On real hardware, x2apic must only be enabled if interrupt remapping is also
        // enabled. See 10.12.7 of the SDM vol 3. On hypervisors, this is not necessary.
        // The hypervisor flag check (cpu_ecxfeature) waits for CPU identification.
        // SAFETY: once, on the boot CPU, before any LAPIC access.
        unsafe {
            LAPIC_READREG.write(x2apic_readreg);
            LAPIC_WRITEREG.write(x2apic_writereg);
        }
        // MULTIPROCESSOR: x86_ipi = x2apic_ipi.
        X2APIC_ENABLED.store(true, Ordering::Relaxed);
        let _ = unported!("codepatch_call(CPTAG_EOI, x2apic_eoi)");
    } else {
        // Map local apic.
        // SAFETY: `va` is the kernel's own page reserved for the LAPIC and `lapic_base` the
        // LAPIC's registers, mapped uncached.
        unsafe {
            pmap_kenter_pa(
                va,
                Paddr::new(lapic_base.as_usize() | PMAP_NOCACHE as usize),
                PROT_READ | PROT_WRITE,
            )
        };
        pmap_update(pmap_kernel());
    }
    // pmap_enter_special(va, lapic_base, PROT_READ | PROT_WRITE): the u-k page table (M6).
    // SAFETY: `s` is this CPU's saved flags.
    unsafe { intr_restore(s) };
}

/// `lapic_enable`: enable local apic.
pub fn lapic_enable() {
    lapic_writereg(LAPIC_SVR, LAPIC_SVR_ENABLE | LAPIC_SPURIOUS_VECTOR as u32);
}

/// `lapic_disable`.
pub fn lapic_disable() {
    lapic_writereg(LAPIC_SVR, 0);
}

/// `lapic_set_lvt`: programs the local interrupt pins (see the module's deviations).
pub fn lapic_set_lvt() {
    // mp_verbose: MULTIPROCESSOR. NIOAPIC > 0: ExtINT would be masked here.
    // ci_vendor == CPUV_AMD && family 0xf/0x10: the C1E workaround (M4-b).

    // for (i = 0; i < mp_nintrs; i++): no MP/ACPI interrupt table yet; the MP default
    // configuration is LINT0 = ExtINT (the 8259's output), LINT1 = NMI.
    lapic_writereg(LAPIC_LVINT0, LAPIC_DLMODE_EXTINT);
    lapic_writereg(LAPIC_LVINT1, LAPIC_DLMODE_NMI);
}

/// `lapic_boot_init`: initialize fixed idt vectors for use by local apic.
pub fn lapic_boot_init(lapic_base: Paddr) {
    lapic_map(lapic_base);

    // MULTIPROCESSOR: LAPIC_IPI_VECTOR and the invalidation IPIs (M5).

    IDT_ALLOCMAP[LAPIC_SPURIOUS_VECTOR as usize].store(true, Ordering::Relaxed);
    idt_vec_set(LAPIC_SPURIOUS_VECTOR, Xintrspurious as *const () as usize);
    IDT_ALLOCMAP[LAPIC_TIMER_VECTOR as usize].store(true, Ordering::Relaxed);
    // idt_vec_set(LAPIC_TIMER_VECTOR, Xintr_lapic_ltimer): the timer stub (M5).

    // NXEN, NHYPERV: not configured.

    evcount_attach(&CLK_COUNT, "clock", ptr::from_ref(&CLK_IRQ).cast::<()>());
    evcount_percpu(&CLK_COUNT);
    // MULTIPROCESSOR: ipi_count.
}

/// `lapic_hwmask`: masks LVT entry `pin`.
fn lapic_hwmask(_pic: &Pic, pin: i32) {
    let reg = LAPIC_LVTT + (pin << 4);
    let val = lapic_readreg(reg) | LAPIC_LVT_MASKED;
    lapic_writereg(reg, val);
}

/// `lapic_hwunmask`: unmasks LVT entry `pin`.
fn lapic_hwunmask(_pic: &Pic, pin: i32) {
    let reg = LAPIC_LVTT + (pin << 4);
    let val = lapic_readreg(reg) & !LAPIC_LVT_MASKED;
    lapic_writereg(reg, val);
}

/// `lapic_setup`: nothing to route.
fn lapic_setup(_pic: &Pic, _ci: &CpuInfo, _pin: i32, _idtvec: i32, _type: i32) {}
