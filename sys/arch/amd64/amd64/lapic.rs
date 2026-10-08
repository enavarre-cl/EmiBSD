/*	$OpenBSD: lapic.c,v 1.77 2025/12/30 15:21:05 kettenis Exp $	*/
/* $NetBSD: lapic.c,v 1.2 2003/05/08 01:04:35 fvdl Exp $ */
/* <LICENSES> */
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
/* </LICENSES> */

/* <CODE> */
//! The local APIC: `arch/amd64/amd64/lapic.c`.
//!
//! Upstream: sys/arch/amd64/amd64/lapic.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M4 ports `local_pic`, the register accessors (`i82489_*`,
//! `x2apic_*`, `lapic_readreg`/`lapic_writereg`, `lapic_cpu_number`), `lapic_map`,
//! `lapic_enable`, `lapic_disable`, `lapic_set_lvt`, `lapic_boot_init`, `lapic_hwmask`,
//! `lapic_hwunmask` and `lapic_setup`; M5 adds the timer: `lapic_gettick`,
//! `lapic_timer_rearm/trigger/start/oneshot/periodic`, `lapic_timer_intrclock`,
//! `lapic_clockintr`, `lapic_startclock`, `lapic_initclocks`, `wait_next_cycle` and
//! `lapic_calibrate_timer`; M11a the `MULTIPROCESSOR` parts: `ipi_count`, the IPI vectors
//! of `lapic_boot_init`, `x86_ipi` with `i82489_icr_wait`, `i82489_ipi_init`, `i82489_ipi`,
//! `x2apic_writeicr`, `x2apic_ipi_init`, `x2apic_ipi` and `x86_ipi_init`.
//!
//! ## Deviations
//! - `lapic_map` maps the page with `pmap_kenter_pa` (`PMAP_NOCACHE`) instead of whapping the
//!   PTE by hand: there is no TLB shootdown to avoid yet. `pmap_enter_special` (the u-k
//!   mapping for the Meltdown trampoline) is M6. x2APIC mode is taken only when the firmware
//!   enabled it (`cpu_ecxfeature` arrives with CPU identification, M4-b), and the `CODEPATCH`
//!   of the EOI is not there: the timer stub's EOI is the MMIO write.
//! - `lapic_set_lvt`: with I/O APICs it masks ExtINT and programs the `mp_intrs` entries
//!   (the MADT's local APIC NMIs) as the C does (M13). Without any MP/ACPI interrupt table
//!   (no `mp_busses`: a kernel without ACPI) LINT0 is programmed as ExtINT and LINT1 as NMI,
//!   the MP specification's default configuration (what `mpbios` would record); the firmware
//!   leaves LINT0 masked, which would cut the 8259 off. The AMD C1E workaround needs
//!   `ci_vendor`/`ci_family` (M4-b).
//! - `lapic_clockintr` takes the interrupt frame by pointer (`vector.S` passes `%rsp`), not
//!   by value as the C does.
//! - `lapic_calibrate_timer`: `mp_verbose` is off and the CPU is named `cpu0`.
//! - Without a table, `lapic_set_lvt` on an application processor masks LINT0: the 8259's
//!   ExtINT goes to the boot processor only, which is what the C's `nioapics > 0` masking
//!   amounts to there. QEMU hands the 8259's output to every LAPIC whose LINT0 is unmasked,
//!   so leaving it open would deliver each legacy interrupt to every CPU.
//! - The IPI stubs' EOI is the MMIO write, as the timer's (no x2APIC `CODEPATCH`).

use core::cell::UnsafeCell;
use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use libkern::StaticCell;

use crate::arch::amd64::amd64::ioapic::NIOAPICS;
use crate::arch::amd64::amd64::machdep::{
    IDT_ALLOCMAP, delay, delay_is_i8254, idt_vec_set, set_initclock_func, set_startclock_func,
};
use crate::arch::amd64::amd64::mainbus::{mp_busses, mp_intrs};
#[cfg(feature = "multiprocessor")]
use crate::arch::amd64::amd64::vector::{
    Xintr_lapic_ipi, Xipi_invlpg, Xipi_invlrange, Xipi_invltlb,
};
use crate::arch::amd64::amd64::vector::{Xintr_lapic_ltimer, Xintrspurious};
#[cfg(feature = "multiprocessor")]
use crate::arch::amd64::amd64::{apic::apic_format_redir, mainbus::MP_VERBOSE};
use crate::arch::amd64::include::cpu::{CpuInfo, curcpu};
use crate::arch::amd64::include::cpufunc::{intr_disable, intr_restore, rdmsr, wrmsr};
use crate::arch::amd64::include::frame::Intrframe;
use crate::arch::amd64::include::i82489reg::{
    LAPIC_CCR_TIMER, LAPIC_DCR_TIMER, LAPIC_DCRT_DIV1, LAPIC_DLMODE_EXTINT, LAPIC_DLMODE_NMI,
    LAPIC_ICR_TIMER, LAPIC_ID, LAPIC_ID_SHIFT, LAPIC_LVINT0, LAPIC_LVINT1, LAPIC_LVT_MASKED,
    LAPIC_LVTT, LAPIC_LVTT_M, LAPIC_LVTT_TM_ONESHOT, LAPIC_LVTT_TM_PERIODIC, LAPIC_SVR,
    LAPIC_SVR_ENABLE, MSR_X2APIC_BASE,
};
#[cfg(feature = "multiprocessor")]
use crate::arch::amd64::include::i82489reg::{
    LAPIC_DEST_MASK, LAPIC_DLMODE_INIT, LAPIC_DLSTAT_BUSY, LAPIC_ICRHI, LAPIC_ICRLO, LAPIC_LVERR,
    LAPIC_LVL_ASSERT, LAPIC_LVL_DEASSERT, LAPIC_LVL_TRIG, LAPIC_PCINT,
};
#[cfg(feature = "multiprocessor")]
use crate::arch::amd64::include::i82489var::{
    LAPIC_IPI_INVLPG, LAPIC_IPI_INVLRANGE, LAPIC_IPI_INVLTLB, LAPIC_IPI_VECTOR,
};
use crate::arch::amd64::include::i82489var::{LAPIC_SPURIOUS_VECTOR, LAPIC_TIMER_VECTOR};
use crate::arch::amd64::include::mpbiosreg::MPS_ALL_APICS;
use crate::arch::amd64::include::param::PAGE_SIZE;
use crate::arch::amd64::include::pic::{PIC_LAPIC, Pic};
use crate::arch::amd64::include::pmap::PMAP_NOCACHE;
use crate::arch::amd64::include::specialreg::{APICBASE_ENABLE_X2APIC, MSR_APICBASE};
use crate::arch::amd64::isa::clock::{RTCLOCK_TVAL, gettick, i8254_inittimecounter_simple};
use crate::conf::param::HZ;
use crate::dev::ic::i8253reg::TIMER_FREQ;
use crate::kern::kern_clock::{PROFHZ, STATCLOCK_IS_RANDOMIZED, STATHZ};
use crate::kern::kern_clockintr::{clockintr_cpu_init, clockintr_dispatch, clockintr_trigger};
use crate::kern::subr_evcount::{evcount_attach, evcount_inc, evcount_percpu};
use crate::kprintf;
use crate::machine::pmap::{pmap_kenter_pa, pmap_kernel, pmap_update};
use crate::sys::clockintr::Intrclock;
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
/// `ipi_count`: the inter-processor interrupt counter (`MULTIPROCESSOR`).
#[cfg(feature = "multiprocessor")]
pub static IPI_COUNT: Evcount = Evcount::new();
/// `ipi_irq`: the counter's user data.
#[cfg(feature = "multiprocessor")]
static IPI_IRQ: AtomicU64 = AtomicU64::new(0);

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
/// `x86_ipi`: how an IPI is sent (`vec`, the target APIC ID or `LAPIC_DEST_*`, the delivery
/// mode): `i82489_ipi`, or `x2apic_ipi` in x2APIC mode (`MULTIPROCESSOR`).
#[cfg(feature = "multiprocessor")]
static X86_IPI: StaticCell<fn(i32, u32, u32)> = StaticCell::new(i82489_ipi);

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
        #[cfg(feature = "multiprocessor")]
        // SAFETY: as above.
        unsafe {
            X86_IPI.write(x2apic_ipi)
        };
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
    let ci = curcpu();

    // MULTIPROCESSOR && mp_verbose: the "prelint" dumps (apic_format_redir).
    #[cfg(feature = "multiprocessor")]
    if MP_VERBOSE.load(Ordering::Relaxed) != 0 {
        let name = cpu_name(ci);
        apic_format_redir(name, "prelint", 0, 0, lapic_readreg(LAPIC_LVINT0));
        apic_format_redir(name, "prelint", 1, 0, lapic_readreg(LAPIC_LVINT1));
    }

    // NIOAPIC > 0: disable ExtINT by default when using I/O APICs.
    if NIOAPICS.load(Ordering::Relaxed) > 0 {
        let lint0 = lapic_readreg(LAPIC_LVINT0) | LAPIC_LVT_MASKED;
        lapic_writereg(LAPIC_LVINT0, lint0);
    } else if mp_busses().is_none() {
        // No MP/ACPI interrupt table (see the module's deviations): the MP default
        // configuration, LINT0 = ExtINT (the 8259's output), LINT1 = NMI. ExtINT goes to the
        // boot processor only.
        if crate::arch::amd64::include::cpu::cpu_is_primary(ci) {
            lapic_writereg(LAPIC_LVINT0, LAPIC_DLMODE_EXTINT);
        } else {
            lapic_writereg(LAPIC_LVINT0, LAPIC_DLMODE_EXTINT | LAPIC_LVT_MASKED);
        }
        lapic_writereg(LAPIC_LVINT1, LAPIC_DLMODE_NMI);
    }

    // ci_vendor == CPUV_AMD && family 0xf/0x10: the C1E workaround (M4-b).

    for mpi in mp_intrs() {
        if mpi.ioapic.is_none()
            && (mpi.cpu_id == MPS_ALL_APICS || mpi.cpu_id as u32 == ci.ci_apicid.get())
        {
            #[cfg(feature = "diagnostic")]
            if mpi.ioapic_pin > 1 {
                crate::kern::subr_prf::panic(format_args!(
                    "lapic_set_lvt: bad pin value {}",
                    mpi.ioapic_pin
                ));
            }
            if mpi.ioapic_pin == 0 {
                lapic_writereg(LAPIC_LVINT0, mpi.redir);
            } else {
                lapic_writereg(LAPIC_LVINT1, mpi.redir);
            }
        }
    }

    #[cfg(feature = "multiprocessor")]
    if MP_VERBOSE.load(Ordering::Relaxed) != 0 {
        let name = cpu_name(ci);
        apic_format_redir(name, "timer", 0, 0, lapic_readreg(LAPIC_LVTT));
        apic_format_redir(name, "pcint", 0, 0, lapic_readreg(LAPIC_PCINT));
        apic_format_redir(name, "lint", 0, 0, lapic_readreg(LAPIC_LVINT0));
        apic_format_redir(name, "lint", 1, 0, lapic_readreg(LAPIC_LVINT1));
        apic_format_redir(name, "err", 0, 0, lapic_readreg(LAPIC_LVERR));
    }
}

/// `ci->ci_dev->dv_xname`, for the `mp_verbose` dumps.
#[cfg(feature = "multiprocessor")]
fn cpu_name(ci: &CpuInfo) -> &str {
    // SAFETY: an attached CPU's device is never freed.
    unsafe { ci.ci_dev.get().as_ref() }.map_or("cpu", |d| d.xname())
}

/// `lapic_boot_init`: initialize fixed idt vectors for use by local apic.
pub fn lapic_boot_init(lapic_base: Paddr) {
    lapic_map(lapic_base);

    #[cfg(feature = "multiprocessor")]
    {
        IDT_ALLOCMAP[LAPIC_IPI_VECTOR as usize].store(true, Ordering::Relaxed);
        idt_vec_set(LAPIC_IPI_VECTOR, Xintr_lapic_ipi as *const () as usize);
        IDT_ALLOCMAP[LAPIC_IPI_INVLTLB as usize].store(true, Ordering::Relaxed);
        IDT_ALLOCMAP[LAPIC_IPI_INVLPG as usize].store(true, Ordering::Relaxed);
        IDT_ALLOCMAP[LAPIC_IPI_INVLRANGE as usize].store(true, Ordering::Relaxed);
        // !pmap_use_pcid: PCID is never enabled (pmap.rs), so the _pcid stubs are not there.
        idt_vec_set(LAPIC_IPI_INVLTLB, Xipi_invltlb as *const () as usize);
        idt_vec_set(LAPIC_IPI_INVLPG, Xipi_invlpg as *const () as usize);
        idt_vec_set(LAPIC_IPI_INVLRANGE, Xipi_invlrange as *const () as usize);
        // NVMM > 0 (LAPIC_IPI_INVEPT): not configured.
    }

    IDT_ALLOCMAP[LAPIC_SPURIOUS_VECTOR as usize].store(true, Ordering::Relaxed);
    idt_vec_set(LAPIC_SPURIOUS_VECTOR, Xintrspurious as *const () as usize);
    IDT_ALLOCMAP[LAPIC_TIMER_VECTOR as usize].store(true, Ordering::Relaxed);
    idt_vec_set(LAPIC_TIMER_VECTOR, Xintr_lapic_ltimer as *const () as usize);

    // NXEN, NHYPERV: not configured.

    evcount_attach(&CLK_COUNT, "clock", ptr::from_ref(&CLK_IRQ).cast::<()>());
    evcount_percpu(&CLK_COUNT);
    #[cfg(feature = "multiprocessor")]
    {
        evcount_attach(&IPI_COUNT, "ipi", ptr::from_ref(&IPI_IRQ).cast::<()>());
        evcount_percpu(&IPI_COUNT);
    }
}

/// `lapic_gettick`: the timer's current count.
pub fn lapic_gettick() -> u32 {
    lapic_readreg(LAPIC_CCR_TIMER)
}

/*
 * this gets us up to a 4GHz busclock....
 */

/// `lapic_per_second`: the timer's clock, in Hz.
pub static LAPIC_PER_SECOND: AtomicU32 = AtomicU32::new(0);
/// `lapic_timer_nsec_cycle_ratio`: cycles per nanosecond, as a 32.32 fixed-point number.
pub static LAPIC_TIMER_NSEC_CYCLE_RATIO: AtomicU64 = AtomicU64::new(0);
/// `lapic_timer_nsec_max`: the longest interval the timer can be programmed for.
pub static LAPIC_TIMER_NSEC_MAX: AtomicU64 = AtomicU64::new(0);

/// `lapic_timer_intrclock`.
pub const LAPIC_TIMER_INTRCLOCK: Intrclock = Intrclock {
    ic_cookie: ptr::null_mut(),
    ic_rearm: lapic_timer_rearm,
    ic_trigger: lapic_timer_trigger,
};

/// `lapic_timer_rearm`: fires the timer in `nsecs`.
pub fn lapic_timer_rearm(_unused: *mut c_void, nsecs: u64) {
    let nsecs = nsecs.min(LAPIC_TIMER_NSEC_MAX.load(Ordering::Relaxed));
    let mut cycles =
        ((nsecs.wrapping_mul(LAPIC_TIMER_NSEC_CYCLE_RATIO.load(Ordering::Relaxed))) >> 32) as u32;
    if cycles == 0 {
        cycles = 1;
    }
    lapic_writereg(LAPIC_ICR_TIMER, cycles);
}

/// `lapic_timer_trigger`: fires the timer as soon as possible.
pub fn lapic_timer_trigger(_unused: *mut c_void) {
    let s = intr_disable();
    lapic_timer_oneshot(0, 1);
    // SAFETY: `s` is this CPU's saved flags.
    unsafe { intr_restore(s) };
}

/// `lapic_timer_start`: start the local apic countdown timer.
///
/// First set the mode, mask, and vector. Then set the divisor. Last, set the cycle count:
/// this restarts the countdown.
#[inline]
fn lapic_timer_start(mode: u32, mask: u32, cycles: u32) {
    lapic_writereg(LAPIC_LVTT, mode | mask | LAPIC_TIMER_VECTOR as u32);
    lapic_writereg(LAPIC_DCR_TIMER, LAPIC_DCRT_DIV1);
    lapic_writereg(LAPIC_ICR_TIMER, cycles);
}

/// `lapic_timer_oneshot`.
pub fn lapic_timer_oneshot(mask: u32, cycles: u32) {
    lapic_timer_start(LAPIC_LVTT_TM_ONESHOT, mask, cycles);
}

/// `lapic_timer_periodic`.
pub fn lapic_timer_periodic(mask: u32, cycles: u32) {
    lapic_timer_start(LAPIC_LVTT_TM_PERIODIC, mask, cycles);
}

/// `lapic_clockintr`: the timer interrupt, from `Xintr_lapic_ltimer` (see the module's
/// deviations).
#[unsafe(no_mangle)]
pub extern "C" fn lapic_clockintr(_arg: *mut c_void, frame: &mut Intrframe) {
    let ci = curcpu();

    let floor = ci.ci_handled_intr_level.get();
    ci.ci_handled_intr_level.set(ci.ci_ilevel.get());
    clockintr_dispatch(ptr::from_mut(frame).cast::<c_void>());
    ci.ci_handled_intr_level.set(floor);

    evcount_inc(&CLK_COUNT);
}

/// `lapic_startclock`.
pub fn lapic_startclock() {
    clockintr_cpu_init(Some(&LAPIC_TIMER_INTRCLOCK));
    clockintr_trigger();
}

/// `lapic_initclocks`.
pub fn lapic_initclocks() {
    i8254_inittimecounter_simple();

    let hz = HZ.load(Ordering::Relaxed);
    STATHZ.store(hz, Ordering::Relaxed);
    PROFHZ.store(hz * 10, Ordering::Relaxed);
    STATCLOCK_IS_RANDOMIZED.store(true, Ordering::Relaxed);
}

/// `wait_next_cycle`: spins until the i8254 reloads.
#[inline]
fn wait_next_cycle() {
    let mut tlast: u32 = 1 << 16; // i8254 counter has 16 bits at most
    loop {
        let tick = gettick() as u32;
        if tick > tlast {
            return;
        }
        tlast = tick;
    }
}

/// `lapic_calibrate_timer`: calibrate the local apic count-down timer (which is running at
/// bus-clock speed) vs. the i8254 counter/timer (which is running at a fixed rate).
///
/// The Intel MP spec says: "An MP operating system may use the IRQ8 real-time clock as a
/// reference to determine the actual APIC timer clock speed."
///
/// We're actually using the IRQ0 timer. Hmm.
pub fn lapic_calibrate_timer(_ci: &CpuInfo) {
    if LAPIC_PER_SECOND.load(Ordering::Relaxed) == 0 {
        // mp_verbose: "cpu0: calibrating local timer".

        // Configure timer to one-shot, interrupt masked, large positive number.
        lapic_timer_oneshot(LAPIC_LVTT_M, 0x8000_0000);

        if delay_is_i8254() {
            let s = intr_disable();

            // wait for current cycle to finish
            wait_next_cycle();

            let startapic = lapic_gettick();

            // wait the next hz cycles
            let hz = HZ.load(Ordering::Relaxed);
            for _ in 0..hz {
                wait_next_cycle();
            }

            let endapic = lapic_gettick();

            // SAFETY: `s` is this CPU's saved flags.
            unsafe { intr_restore(s) };

            let dtick = u64::from(hz as u32) * RTCLOCK_TVAL.load(Ordering::Relaxed);
            let dapic = u64::from(startapic.wrapping_sub(endapic));

            // there are TIMER_FREQ ticks per second. in dtick ticks, there are dapic bus clocks.
            let tmp = (TIMER_FREQ as u64 * dapic) / dtick;

            LAPIC_PER_SECOND.store(tmp as u32, Ordering::Relaxed);
        } else {
            let s = intr_disable();
            let startapic = lapic_gettick();
            delay(1000 * 1000);
            let endapic = lapic_gettick();
            // SAFETY: `s` is this CPU's saved flags.
            unsafe { intr_restore(s) };
            LAPIC_PER_SECOND.store(startapic.wrapping_sub(endapic), Ordering::Relaxed);
        }
    }

    let per_second = LAPIC_PER_SECOND.load(Ordering::Relaxed);
    kprintf!(
        "cpu0: apic clock running at {}MHz\n",
        per_second / (1000 * 1000)
    );

    // XXX What do we do if this is zero?
    if per_second == 0 {
        return;
    }

    let ratio = u64::from(per_second) * (1u64 << 32) / 1_000_000_000;
    LAPIC_TIMER_NSEC_CYCLE_RATIO.store(ratio, Ordering::Relaxed);
    LAPIC_TIMER_NSEC_MAX.store(u64::MAX / ratio, Ordering::Relaxed);
    set_initclock_func(lapic_initclocks);
    set_startclock_func(lapic_startclock);
}

// XXX the following belong mostly or partly elsewhere..

/// `i82489_icr_wait`: spins until the LAPIC has sent the last command (`DIAGNOSTIC`: panics
/// after 100000 rounds).
#[cfg(feature = "multiprocessor")]
#[inline]
fn i82489_icr_wait() {
    #[cfg(feature = "diagnostic")]
    let mut j: u32 = 100_000;

    while i82489_readreg(LAPIC_ICRLO) & LAPIC_DLSTAT_BUSY != 0 {
        core::hint::spin_loop();
        #[cfg(feature = "diagnostic")]
        {
            j -= 1;
            if j == 0 {
                crate::kern::subr_prf::panic(format_args!("i82489_icr_wait: busy"));
            }
        }
    }
}

/// `i82489_ipi_init`: the INIT assert/deassert pair to `target` (MMIO mode).
#[cfg(feature = "multiprocessor")]
pub fn i82489_ipi_init(target: u32) {
    if target & LAPIC_DEST_MASK == 0 {
        i82489_writereg(LAPIC_ICRHI, target << LAPIC_ID_SHIFT);
    }

    i82489_writereg(
        LAPIC_ICRLO,
        (target & LAPIC_DEST_MASK) | LAPIC_DLMODE_INIT | LAPIC_LVL_ASSERT,
    );

    i82489_icr_wait();

    delay(10000);

    i82489_writereg(
        LAPIC_ICRLO,
        (target & LAPIC_DEST_MASK) | LAPIC_DLMODE_INIT | LAPIC_LVL_TRIG | LAPIC_LVL_DEASSERT,
    );

    i82489_icr_wait();
}

/// `i82489_ipi`: sends vector `vec` to `target` with delivery mode `dl` (MMIO mode).
#[cfg(feature = "multiprocessor")]
pub fn i82489_ipi(vec: i32, target: u32, dl: u32) {
    let s = crate::machine::intr::splhigh();

    i82489_icr_wait();

    if target & LAPIC_DEST_MASK == 0 {
        i82489_writereg(LAPIC_ICRHI, target << LAPIC_ID_SHIFT);
    }

    // What the receiver reads (ci_ipis, the shootdown globals) is stored before the command:
    // the volatile ICR write must not move above those stores.
    core::sync::atomic::compiler_fence(Ordering::SeqCst);
    i82489_writereg(
        LAPIC_ICRLO,
        (target & LAPIC_DEST_MASK) | vec as u32 | dl | LAPIC_LVL_ASSERT,
    );

    i82489_icr_wait();

    crate::machine::intr::splx(s);
}

/// `x2apic_writeicr`: the 64-bit ICR write of x2APIC mode.
#[cfg(feature = "multiprocessor")]
#[inline]
fn x2apic_writeicr(hi: u32, lo: u32) {
    let msr = MSR_X2APIC_BASE + (LAPIC_ICRLO as u32 >> 4);
    // SAFETY: the x2APIC ICR MSR exists in x2APIC mode, the only mode this is used in; the
    // write sends an interrupt command and touches no memory.
    unsafe {
        core::arch::asm!("wrmsr", in("eax") lo, in("edx") hi, in("ecx") msr, options(nostack, preserves_flags))
    };
}

/// `x2apic_ipi_init`: the INIT assert/deassert pair to `target` (x2APIC mode).
#[cfg(feature = "multiprocessor")]
pub fn x2apic_ipi_init(target: u32) {
    let mut hi = 0;

    if target & LAPIC_DEST_MASK == 0 {
        hi = target & 0xff;
    }

    x2apic_writeicr(
        hi,
        (target & LAPIC_DEST_MASK) | LAPIC_DLMODE_INIT | LAPIC_LVL_ASSERT,
    );

    delay(10000);

    x2apic_writeicr(
        0,
        (target & LAPIC_DEST_MASK) | LAPIC_DLMODE_INIT | LAPIC_LVL_TRIG | LAPIC_LVL_DEASSERT,
    );
}

/// `x2apic_ipi`: sends vector `vec` to `target` with delivery mode `dl` (x2APIC mode).
#[cfg(feature = "multiprocessor")]
pub fn x2apic_ipi(vec: i32, target: u32, dl: u32) {
    let mut hi = 0;

    if target & LAPIC_DEST_MASK == 0 {
        hi = target & 0xff;
    }

    let lo = (target & LAPIC_DEST_MASK) | vec as u32 | dl | LAPIC_LVL_ASSERT;

    // SAFETY: `mfence; lfence` only orders memory accesses and instructions: the ICR write
    // is not serializing, and the receiver must see what was stored before it.
    unsafe { core::arch::asm!("mfence", "lfence", options(nostack, preserves_flags)) };
    x2apic_writeicr(hi, lo);
}

/// `x86_ipi_init`: the INIT IPI pair, in the mode the LAPIC runs in.
#[cfg(feature = "multiprocessor")]
pub fn x86_ipi_init(target: u32) {
    if X2APIC_ENABLED.load(Ordering::Relaxed) {
        x2apic_ipi_init(target);
    } else {
        i82489_ipi_init(target);
    }
}

/// `(*x86_ipi)(vec, target, dl)`: sends an IPI through the accessor `lapic_map` chose.
#[cfg(feature = "multiprocessor")]
pub fn x86_ipi(vec: i32, target: u32, dl: u32) {
    // SAFETY: written once by `lapic_map` on the boot CPU before any CPU sends an IPI.
    (unsafe { X86_IPI.read() })(vec, target, dl)
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
/* </CODE> */
