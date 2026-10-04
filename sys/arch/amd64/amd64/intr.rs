/*	$OpenBSD: intr.c,v 1.64 2025/11/10 12:34:52 dlg Exp $	*/
/*	$NetBSD: intr.c,v 1.3 2003/03/03 22:16:20 fvdl Exp $	*/
/* <LICENSES> */
/*
 * Copyright 2002 (c) Wasabi Systems, Inc.
 * All rights reserved.
 *
 * Written by Frank van der Linden for Wasabi Systems, Inc.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. All advertising materials mentioning features or use of this software
 *    must display the following acknowledgement:
 *      This product includes software developed for the NetBSD Project by
 *      Wasabi Systems, Inc.
 * 4. The name of Wasabi Systems, Inc. may not be used to endorse
 *    or promote products derived from this software without specific prior
 *    written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY WASABI SYSTEMS, INC. ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
 * TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
 * PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL WASABI SYSTEMS, INC
 * BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
 * CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
 * SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
 * INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
 * CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
 * ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
 * POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

//! amd64 interrupt sources and `spl(9)`: `arch/amd64/amd64/intr.c`.
//!
//! Upstream: sys/arch/amd64/amd64/intr.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M4 ports `softintr_pic`, `softintr_to_ssir`,
//! `intr_default_setup`, `x86_nmi`, `intr_calculatemasks`, `intr_allocate_slot_cpu`,
//! `intr_allocate_slot`, `intr_shared_edge`, `intr_establish`, `intr_disestablish`,
//! `intr_handler`, the fake soft handlers, `cpu_intr_init`, `intr_printconfig`,
//! `intr_barrier`, `intr_set_wakeup`, `splraise`, `spllower`, `softintr` and `dosoftint`.
//! M5 adds the LAPIC timer source of `cpu_intr_init` (`LIR_TIMER`); M11a the
//! `MULTIPROCESSOR` paths: the kernel lock in `intr_handler` and the `LIR_IPI` source
//! (`fake_ipi_intrhand`). Xen, Hyper-V, `SIR_XCALL` and the `SUSPEND` wakeup masking come with
//! their subsystems.
//!
//! ## Deviations
//! - `CPU_INFO_FOREACH` walks `ci_next` from `cpu_info_primary`; without `MULTIPROCESSOR`
//!   the primary is the only CPU.
//! - `MULTIPROCESSOR`: device interrupts all go to the boot CPU (no MADT/IOAPIC routing to
//!   the APs until M13). `intr_handler` honours `IPL_MPSAFE` as the C does (M11e).
//! - `intr_printconfig` is the `INTRDEBUG` body behind feature `debug`.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicPtr, Ordering};

use crate::arch::amd64::amd64::autoconf::COLD;
use crate::arch::amd64::amd64::i8259::{I8259_PIC, i8259_default_setup, i8259_stubs_table};
use crate::arch::amd64::amd64::lapic::LOCAL_PIC;
use crate::arch::amd64::amd64::machdep::{IDT, IDT_ALLOCMAP, idt_vec_alloc, idt_vec_free, setgate};
use crate::arch::amd64::amd64::spl::Xspllower;
#[cfg(feature = "multiprocessor")]
use crate::arch::amd64::amd64::vector::{Xrecurse_lapic_ipi, Xresume_lapic_ipi};
use crate::arch::amd64::amd64::vector::{
    Xrecurse_lapic_ltimer, Xresume_lapic_ltimer, Xsoftclock, Xsoftnet, Xsofttty,
};
use crate::arch::amd64::include::cpu::{CpuInfo, cpu_info_primary, cpu_is_primary, curcpu};
use crate::arch::amd64::include::cpufunc::{intr_disable, intr_restore};
use crate::arch::amd64::include::frame::Intrframe;
use crate::arch::amd64::include::i8259::ICU_OFFSET;
use crate::arch::amd64::include::intr::{IntrFn, Intrhand, Intrsource, Intrstub, apic_level};
use crate::arch::amd64::include::intrdefs::{
    IDT_INTR_HIGH, IPL_CLOCK, IPL_HIGH, IPL_MPSAFE, IPL_NONE, IPL_SOFTCLOCK, IPL_SOFTNET,
    IPL_SOFTTTY, IPL_TTY, IPL_WAKEUP, IST_EDGE, IST_LEVEL, IST_NONE, IST_PULSE, LIR_TIMER,
    MAX_INTR_SOURCES, NIPL, NUM_LEGACY_IRQS, SIR_CLOCK, SIR_NET, SIR_TTY,
};
#[cfg(feature = "multiprocessor")]
use crate::arch::amd64::include::intrdefs::{IPL_IPI, LIR_IPI};
use crate::arch::amd64::include::pic::{PIC_SOFT, Pic};
use crate::arch::amd64::include::pio::inb;
use crate::arch::amd64::include::segments::{GCODE_SEL, SDT_SYS386IGT, SEL_KPL, gsel};
use crate::kassert;
#[cfg(feature = "multiprocessor")]
use crate::kern::kern_lock::{__mp_lock, __mp_unlock, KERNEL_LOCK};
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_sched::sched_barrier;
use crate::kern::kern_softintr::softintr_dispatch;
use crate::kern::subr_evcount::{evcount_attach, evcount_detach};
use crate::kern::subr_prf::{log, panic, printf, snprintf};
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_DEVBUF, M_NOWAIT, M_WAITOK, M_ZERO};
use crate::sys::softintr::NSOFTINTR;
use crate::sys::syslog::LOG_CRIT;

/// `softintr_pic`.
pub static SOFTINTR_PIC: Pic = Pic {
    pic_name: "softintr_pic0",
    pic_type: PIC_SOFT,
    pic_hwmask: None,
    pic_hwunmask: None,
    pic_addroute: None,
    pic_delroute: None,
    pic_allocidtvec: None,
    pic_level_stubs: None,
    pic_edge_stubs: None,
};

/// `softintr_to_ssir[NSOFTINTR]`: the `ci_ipending` bit of each soft interrupt level.
pub const SOFTINTR_TO_SSIR: [u32; NSOFTINTR] = [SIR_CLOCK, SIR_NET, SIR_TTY];

/// `intr_suspended`.
pub static INTR_SUSPENDED: AtomicBool = AtomicBool::new(false);
/// `intr_nowake`: the last non-wakeup handler that fired while suspended.
static INTR_NOWAKE: AtomicPtr<Intrhand> = AtomicPtr::new(ptr::null_mut());

/// `intr_default_setup`: fill in default interrupt table (in case of spurious interrupt
/// during configuration of kernel), setup interrupt control unit.
pub fn intr_default_setup() {
    let stubs = i8259_stubs_table();
    // SAFETY: the boot CPU's IDT, written before the legacy gates are unmasked.
    let idt = unsafe { IDT.get_mut() };

    // icu vectors
    for (i, stub) in stubs.iter().enumerate().take(NUM_LEGACY_IRQS) {
        let vec = ICU_OFFSET as usize + i;
        IDT_ALLOCMAP[vec].store(true, Ordering::Relaxed);
        setgate(
            &mut idt.0[vec],
            stub.ist_entry,
            0,
            SDT_SYS386IGT,
            SEL_KPL,
            gsel(GCODE_SEL, SEL_KPL),
        );
    }

    // Eventually might want to check if it's actually there.
    i8259_default_setup();
}

/// `x86_nmi`: handle a NMI, possibly a machine check. Returns `true` to panic the system,
/// `false` to ignore.
pub fn x86_nmi() -> bool {
    // SAFETY: the PC platform's system control port and the RTC index port, read only.
    let (p61, p70) = unsafe { (inb(0x61), inb(0x70)) };
    log(
        LOG_CRIT,
        format_args!("NMI port 61 {p61:x}, port 70 {p70:x}\n"),
    );
    false
}

/// The source at `slot` of `ci`, if one is allocated.
fn source(ci: &CpuInfo, slot: usize) -> Option<&'static Intrsource> {
    // SAFETY: a non-null entry is a malloc'd source that stays until intr_disestablish frees
    // it, which also clears the entry.
    unsafe { ci.ci_isources[slot].get().as_ref() }
}

/// The handlers of a source, first to last.
fn handlers(src: &Intrsource) -> impl Iterator<Item = &'static Intrhand> {
    let mut q = src.is_handlers.get();
    core::iter::from_fn(move || {
        // SAFETY: the chain's handlers are established ones, alive until disestablished.
        let ih = unsafe { q.as_ref() }?;
        q = ih.ih_next.get();
        Some(ih)
    })
}

/// `intr_calculatemasks`: recalculate the interrupt masks from scratch.
pub fn intr_calculatemasks(ci: &CpuInfo) {
    let mut intrlevel = [0u32; MAX_INTR_SOURCES];

    // First, figure out which levels each IRQ uses.
    let mut unusedirqs: u64 = u64::MAX;
    for (irq, level) in intrlevel.iter_mut().enumerate() {
        let Some(src) = source(ci, irq) else {
            *level = 0;
            continue;
        };
        let mut levels = 0u32;
        for q in handlers(src) {
            levels |= 1 << q.ih_level.get();
        }
        *level = levels;
        if levels != 0 {
            unusedirqs &= !(1u64 << irq);
        }
    }

    // Then figure out which IRQs use each level.
    for level in 0..NIPL {
        let mut irqs: u64 = 0;
        for (irq, &l) in intrlevel.iter().enumerate() {
            if l & (1 << level) != 0 {
                irqs |= 1u64 << irq;
            }
        }
        ci.ci_imask[level].set(irqs | unusedirqs);
    }

    for level in 0..(NIPL - 1) {
        ci.ci_imask[level + 1].set(ci.ci_imask[level + 1].get() | ci.ci_imask[level].get());
    }

    for irq in 0..MAX_INTR_SOURCES {
        let Some(src) = source(ci, irq) else {
            continue;
        };
        let mut maxlevel = IPL_NONE;
        let mut minlevel = IPL_HIGH;
        for q in handlers(src) {
            minlevel = minlevel.min(q.ih_level.get());
            maxlevel = maxlevel.max(q.ih_level.get());
        }
        src.is_maxlevel.set(maxlevel);
        src.is_minlevel.set(minlevel);
    }

    for level in 0..NIPL {
        ci.ci_iunmask[level].set(!ci.ci_imask[level].get());
    }
}

/// Allocates a zeroed `intrsource` named "pin N".
fn new_source(pin: i32) -> Option<&'static Intrsource> {
    let isp = malloc(size_of::<Intrsource>(), M_DEVBUF, M_NOWAIT | M_ZERO)?.cast::<Intrsource>();
    // SAFETY: a fresh allocation of the right size and alignment, written once before use.
    unsafe { isp.write(Intrsource::new()) };
    // SAFETY: as above; the source is not shared yet.
    let src: &'static Intrsource = unsafe { isp.as_ref() };
    // SAFETY: the name is written once, before the source is published.
    snprintf(
        unsafe { &mut *src.is_evname.get() },
        format_args!("pin {pin}"),
    );
    Some(src)
}

/// `intr_allocate_slot_cpu`: finds or allocates the slot of (`pic`, `pin`) on `ci`.
pub fn intr_allocate_slot_cpu(
    ci: &'static CpuInfo,
    pic: &'static Pic,
    pin: i32,
    index: &mut i32,
) -> Result<(), Errno> {
    let mut start = if cpu_is_primary(ci) {
        NUM_LEGACY_IRQS
    } else {
        0
    };
    let mut slot: i32 = -1;

    for i in 0..start {
        if let Some(isp) = source(ci, i)
            && ptr::eq(isp.is_pic.get(), pic)
            && isp.is_pin.get() == pin
        {
            slot = i as i32;
            start = MAX_INTR_SOURCES;
            break;
        }
    }
    for i in start..MAX_INTR_SOURCES {
        match source(ci, i) {
            Some(isp) if ptr::eq(isp.is_pic.get(), pic) && isp.is_pin.get() == pin => {
                slot = i as i32;
                break;
            }
            None if slot == -1 => {
                slot = i as i32;
                continue;
            }
            _ => {}
        }
    }
    if slot == -1 {
        return Err(Errno::EBUSY);
    }

    if source(ci, slot as usize).is_none() {
        let isp = new_source(pin).ok_or(Errno::ENOMEM)?;
        ci.ci_isources[slot as usize].set(ptr::from_ref(isp));
    }

    *index = slot;
    Ok(())
}

/// `intr_allocate_slot`: a simple round-robin allocator to assign interrupts to CPUs.
#[allow(clippy::too_many_arguments)] // the C's signature
pub fn intr_allocate_slot(
    pic: &'static Pic,
    legacy_irq: i32,
    pin: i32,
    level: i32,
    cip: &mut Option<&'static CpuInfo>,
    index: &mut i32,
    idt_slot: &mut i32,
) -> Result<(), Errno> {
    let ci: &'static CpuInfo;
    let slot: i32;
    let idtvec: i32;

    // If a legacy IRQ is wanted, try to use a fixed slot pointing at the primary CPU. In the
    // case of IO APICs, multiple pins may map to one legacy IRQ, but they should not be shared
    // in that case, so the first one gets the legacy slot, but a subsequent allocation with a
    // different pin will get a different slot.
    let mut other = legacy_irq == -1;
    let mut legacy: Option<(&'static CpuInfo, i32, Option<&'static Intrsource>)> = None;
    if !other {
        let primary = cpu_info_primary();
        // must check for duplicate pic + pin first
        let duplicate = (0..MAX_INTR_SOURCES).find(|&s| {
            source(primary, s)
                .is_some_and(|isp| ptr::eq(isp.is_pic.get(), pic) && isp.is_pin.get() == pin)
        });
        match duplicate {
            Some(s) => legacy = Some((primary, s as i32, source(primary, s))),
            None => {
                let s = legacy_irq;
                match source(primary, s as usize) {
                    None => {
                        let isp = new_source(pin).ok_or(Errno::ENOMEM)?;
                        primary.ci_isources[s as usize].set(ptr::from_ref(isp));
                        legacy = Some((primary, s, Some(isp)));
                    }
                    Some(isp) => {
                        if !ptr::eq(isp.is_pic.get(), pic) || isp.is_pin.get() != pin {
                            if ptr::eq(pic, &I8259_PIC) {
                                return Err(Errno::EINVAL);
                            }
                            other = true;
                        } else {
                            legacy = Some((primary, s, Some(isp)));
                        }
                    }
                }
            }
        }
    }

    if let (false, Some((lci, lslot, isp))) = (other, legacy) {
        // duplicate:
        ci = lci;
        slot = lslot;
        if ptr::eq(pic, &I8259_PIC) {
            idtvec = ICU_OFFSET + legacy_irq;
        } else {
            let isp = isp.ok_or(Errno::EBUSY)?;
            // IOAPIC_HWMASK is not defined: a lower level than the source's gets a new vector.
            if isp.is_minlevel.get() == 0 || level < isp.is_minlevel.get() {
                idtvec = idt_vec_alloc(apic_level(level), IDT_INTR_HIGH);
                if idtvec == 0 {
                    return Err(Errno::EBUSY);
                }
            } else {
                idtvec = isp.is_idtvec.get();
            }
        }
    } else {
        // other: look for a free slot elsewhere. If cip is null, it means try primary cpu but
        // accept secondary, otherwise we need a slot on the requested cpu.
        let first = cip.unwrap_or_else(cpu_info_primary);
        let mut found: Option<(&'static CpuInfo, i32)> = None;
        let mut s = 0;
        if intr_allocate_slot_cpu(first, pic, pin, &mut s).is_ok() {
            found = Some((first, s));
        } else {
            // Can't alloc on the requested cpu, fail.
            if cip.is_some() {
                return Err(Errno::EBUSY);
            }
            // ..now try the others.
            let mut next = cpu_info_primary().ci_next.get();
            // SAFETY: the CPU list links static cpu_info structures.
            while let Some(c) = unsafe { next.as_ref() } {
                if !cpu_is_primary(c) && intr_allocate_slot_cpu(c, pic, pin, &mut s).is_ok() {
                    found = Some((c, s));
                    break;
                }
                next = c.ci_next.get();
            }
        }
        let Some((fci, fslot)) = found else {
            return Err(Errno::EBUSY);
        };
        // found:
        ci = fci;
        slot = fslot;
        idtvec = match pic.pic_allocidtvec {
            Some(alloc) => alloc(pic, pin, apic_level(level), IDT_INTR_HIGH),
            None => idt_vec_alloc(apic_level(level), IDT_INTR_HIGH),
        };
        if idtvec == 0 {
            if let Some(isp) = source(ci, slot as usize) {
                free(
                    NonNull::from(isp).cast::<u8>(),
                    M_DEVBUF,
                    size_of::<Intrsource>(),
                );
            }
            ci.ci_isources[slot as usize].set(ptr::null());
            return Err(Errno::EBUSY);
        }
    }

    *idt_slot = idtvec;
    *index = slot;
    *cip = Some(ci);
    Ok(())
}

/// `intr_shared_edge`: true if the system has any non-level interrupts which are shared on
/// the same pin. The interrupt stubs read it.
#[unsafe(no_mangle)]
pub static intr_shared_edge: AtomicI32 = AtomicI32::new(0);

/// `intr_establish`: registers `handler(arg)` for `pin` of `pic` at `level`; `legacy_irq` is
/// the ISA IRQ or -1. Returns the handle `intr_disestablish` takes.
#[allow(clippy::too_many_arguments)] // the C's signature
pub fn intr_establish(
    legacy_irq: i32,
    pic: &'static Pic,
    pin: i32,
    type_: i32,
    level: i32,
    ci: Option<&'static CpuInfo>,
    handler: IntrFn,
    arg: *mut c_void,
    what: &'static str,
) -> Option<NonNull<Intrhand>> {
    #[cfg(feature = "diagnostic")]
    {
        if legacy_irq != -1 && !(0..=15).contains(&legacy_irq) {
            panic(format_args!("intr_establish: bad legacy IRQ value"));
        }
        if legacy_irq == -1 && ptr::eq(pic, &I8259_PIC) {
            panic(format_args!("intr_establish: non-legacy IRQ on i8259"));
        }
    }

    let flags = level & (IPL_MPSAFE | IPL_WAKEUP);
    let level = level & !(IPL_MPSAFE | IPL_WAKEUP);

    kassert!(level <= IPL_TTY || level >= IPL_CLOCK || flags & IPL_MPSAFE != 0);

    let mut ci = ci;
    let mut slot = 0;
    let mut idt_vec = 0;
    if intr_allocate_slot(
        pic,
        legacy_irq,
        pin,
        level,
        &mut ci,
        &mut slot,
        &mut idt_vec,
    )
    .is_err()
    {
        printf(format_args!(
            "failed to allocate interrupt slot for PIC {} pin {pin}\n",
            pic.pic_name
        ));
        return None;
    }
    let ci = ci?;

    // no point in sleeping unless someone can free memory.
    let flags_m = if COLD.load(Ordering::Relaxed) {
        M_NOWAIT
    } else {
        M_WAITOK
    };
    let Some(ih) = malloc(size_of::<Intrhand>(), M_DEVBUF, flags_m) else {
        printf(format_args!(
            "intr_establish: can't allocate handler info\n"
        ));
        return None;
    };
    let ih = ih.cast::<Intrhand>();
    // SAFETY: a fresh allocation of the right size and alignment, written once before use.
    unsafe { ih.write(Intrhand::new()) };
    // SAFETY: as above; the handler lives until intr_disestablish frees it.
    let hand: &'static Intrhand = unsafe { ih.as_ref() };
    let free_ih = || free(ih.cast::<u8>(), M_DEVBUF, size_of::<Intrhand>());

    let src = source(ci, slot as usize)?;
    if !src.is_handlers.get().is_null() {
        // SAFETY: a source with handlers has its pic set to a static Pic.
        let spic = unsafe { &*src.is_pic.get() };
        if spic.pic_type != pic.pic_type {
            free_ih();
            printf(format_args!(
                "intr_establish: can't share intr source between different PIC types (legacy_irq {legacy_irq} pin {pin} slot {slot})\n"
            ));
            return None;
        }
    }

    src.is_pin.set(pin);
    src.is_pic.set(ptr::from_ref(pic));

    let is_type = src.is_type.get();
    if is_type == IST_NONE {
        src.is_type.set(type_);
    } else if (is_type == IST_EDGE || is_type == IST_LEVEL) && is_type == type_ {
        if is_type == IST_EDGE {
            intr_shared_edge.store(1, Ordering::Relaxed);
        }
    } else if is_type == IST_EDGE || is_type == IST_LEVEL || is_type == IST_PULSE {
        if is_type == IST_EDGE {
            intr_shared_edge.store(1, Ordering::Relaxed);
        }
        if type_ != IST_NONE {
            printf(format_args!(
                "intr_establish: pic {} pin {pin}: can't share type {is_type} with {type_}\n",
                pic.pic_name
            ));
            free_ih();
            return None;
        }
    } else {
        panic(format_args!(
            "intr_establish: bad intr type {is_type} for pic {} pin {pin}",
            pic.pic_name
        ));
    }

    if !COLD.load(Ordering::Relaxed)
        && let Some(hwmask) = pic.pic_hwmask
    {
        hwmask(pic, pin);
    }

    // Figure out where to put the handler. This is O(N^2), but we want to preserve the
    // order, and N is generally small.
    let mut p: &Cell<*const Intrhand> = &src.is_handlers;
    // SAFETY: as for `handlers`.
    while let Some(q) = unsafe { p.get().as_ref() } {
        if q.ih_level.get() > level {
            p = &q.ih_next;
        } else {
            break;
        }
    }

    hand.ih_fun.set(Some(handler));
    hand.ih_arg.set(arg);
    hand.ih_next.set(p.get());
    hand.ih_level.set(level);
    hand.ih_flags.set(flags);
    hand.ih_pin.set(pin);
    hand.ih_cpu.set(ptr::from_ref(ci));
    hand.ih_slot.set(slot);
    evcount_attach(
        &hand.ih_count,
        what,
        ptr::from_ref(&src.is_idtvec).cast::<()>(),
    );
    p.set(ptr::from_ref(hand));

    intr_calculatemasks(ci);

    if src.is_resume.get() == 0 || src.is_idtvec.get() != idt_vec {
        if src.is_idtvec.get() != 0 && src.is_idtvec.get() != idt_vec {
            idt_vec_free(src.is_idtvec.get());
        }
        src.is_idtvec.set(idt_vec);
        let table = if type_ == IST_LEVEL {
            pic.pic_level_stubs
        } else {
            pic.pic_edge_stubs
        };
        let stubp: &Intrstub = &table.map(|t| t())?[slot as usize];
        src.is_resume.set(stubp.ist_resume);
        src.is_recurse.set(stubp.ist_recurse);
        // SAFETY: the vector is allocated to this source; the gate is written while the pin
        // is masked.
        let idt = unsafe { IDT.get_mut() };
        setgate(
            &mut idt.0[idt_vec as usize],
            stubp.ist_entry,
            0,
            SDT_SYS386IGT,
            SEL_KPL,
            gsel(GCODE_SEL, SEL_KPL),
        );
    }

    if let Some(addroute) = pic.pic_addroute {
        addroute(pic, ci, pin, idt_vec, type_);
    }

    if !COLD.load(Ordering::Relaxed)
        && let Some(hwunmask) = pic.pic_hwunmask
    {
        hwunmask(pic, pin);
    }

    #[cfg(feature = "debug")]
    printf(format_args!(
        "allocated pic {} type {} pin {pin} level {level} to cpu{} slot {slot} idt entry {idt_vec}\n",
        pic.pic_name,
        if type_ == IST_EDGE { "edge" } else { "level" },
        ci.ci_apicid.get()
    ));

    Some(ih)
}

/// `intr_disestablish`: deregister an interrupt handler.
///
/// # Safety
///
/// `ih` must come from `intr_establish` and not be used afterwards.
pub unsafe fn intr_disestablish(ih: NonNull<Intrhand>) {
    // SAFETY: the caller's guarantee.
    let hand: &'static Intrhand = unsafe { ih.as_ref() };
    // SAFETY: an established handler names its CPU, source and pic, all alive.
    let ci = unsafe { &*hand.ih_cpu.get() };
    let slot = hand.ih_slot.get() as usize;
    let Some(src) = source(ci, slot) else {
        panic(format_args!("intr_disestablish: handler not registered"));
    };
    // SAFETY: as above.
    let pic = unsafe { &*src.is_pic.get() };
    let idtvec = src.is_idtvec.get();

    if let Some(hwmask) = pic.pic_hwmask {
        hwmask(pic, hand.ih_pin.get());
    }
    ci.ci_ipending.fetch_and(!(1u64 << slot), Ordering::Relaxed);

    // Remove the handler from the chain.
    let mut p: &Cell<*const Intrhand> = &src.is_handlers;
    loop {
        // SAFETY: as for `handlers`.
        let Some(q) = (unsafe { p.get().as_ref() }) else {
            panic(format_args!("intr_disestablish: handler not registered"));
        };
        if ptr::eq(q, hand) {
            break;
        }
        p = &q.ih_next;
    }
    p.set(hand.ih_next.get());

    intr_calculatemasks(ci);

    if src.is_handlers.get().is_null() {
        if let Some(delroute) = pic.pic_delroute {
            delroute(pic, ci, hand.ih_pin.get(), idtvec, src.is_type.get());
        }
    } else if let Some(hwunmask) = pic.pic_hwunmask {
        hwunmask(pic, hand.ih_pin.get());
    }

    #[cfg(feature = "debug")]
    printf(format_args!(
        "cpu{}: remove slot {slot} (pic {} pin {} vec {idtvec})\n",
        ci.ci_apicid.get(),
        pic.pic_name,
        hand.ih_pin.get()
    ));

    if src.is_handlers.get().is_null() {
        free(
            NonNull::from(src).cast::<u8>(),
            M_DEVBUF,
            size_of::<Intrsource>(),
        );
        ci.ci_isources[slot].set(ptr::null());
        if !ptr::eq(pic, &I8259_PIC) {
            idt_vec_free(idtvec);
        }
    }

    evcount_detach(&hand.ih_count);
    free(ih.cast::<u8>(), M_DEVBUF, size_of::<Intrhand>());
}

/// `intr_handler`: runs one handler from the interrupt stub; the handler gets its argument,
/// or the frame when it has none.
///
/// # Safety
///
/// `ih` must be an established handler (the stubs pass one from a source's chain).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn intr_handler(frame: *mut Intrframe, ih: *const Intrhand) -> i32 {
    let ci = curcpu();
    // SAFETY: the caller's guarantee.
    let ih = unsafe { &*ih };

    // We may not be able to mask MSIs, so block non-wakeup interrupts while we're suspended.
    if INTR_SUSPENDED.load(Ordering::Relaxed) && ih.ih_flags.get() & IPL_WAKEUP == 0 {
        INTR_NOWAKE.store(ptr::from_ref(ih).cast_mut(), Ordering::Relaxed);
        return 0;
    }

    // MULTIPROCESSOR: the kernel lock unless IPL_MPSAFE.
    #[cfg(feature = "multiprocessor")]
    let need_lock = ih.ih_flags.get() & IPL_MPSAFE == 0;
    #[cfg(feature = "multiprocessor")]
    if need_lock {
        __mp_lock(&KERNEL_LOCK);
    }

    let floor = ci.ci_handled_intr_level.get();
    ci.ci_handled_intr_level.set(ih.ih_level.get());
    let arg = ih.ih_arg.get();
    let rc = match ih.ih_fun.get() {
        Some(fun) => fun(if arg.is_null() { frame.cast() } else { arg }),
        None => 0,
    };
    ci.ci_handled_intr_level.set(floor);
    #[cfg(feature = "multiprocessor")]
    if need_lock {
        __mp_unlock(&KERNEL_LOCK);
    }

    rc
}

/// `fake_softclock_intrhand`: fake interrupt handler structures for the benefit of symmetry
/// with other interrupt sources, and the benefit of `intr_calculatemasks()`.
static FAKE_SOFTCLOCK_INTRHAND: Intrhand = Intrhand::new();
/// `fake_softnet_intrhand`.
static FAKE_SOFTNET_INTRHAND: Intrhand = Intrhand::new();
/// `fake_softtty_intrhand`.
static FAKE_SOFTTTY_INTRHAND: Intrhand = Intrhand::new();
/// `fake_timer_intrhand`.
static FAKE_TIMER_INTRHAND: Intrhand = Intrhand::new();
/// `fake_ipi_intrhand` (`MULTIPROCESSOR`).
#[cfg(feature = "multiprocessor")]
static FAKE_IPI_INTRHAND: Intrhand = Intrhand::new();

/// A fixed source for `cpu_intr_init`: `recurse` and `resume` entries, the fake handler at
/// `level`, on `pic`.
fn fixed_source(
    recurse: usize,
    resume: usize,
    fake: &'static Intrhand,
    level: i32,
    pic: &'static Pic,
) -> &'static Intrsource {
    let Some(isp) = malloc(size_of::<Intrsource>(), M_DEVBUF, M_NOWAIT | M_ZERO) else {
        panic(format_args!("can't allocate fixed interrupt source"));
    };
    let isp = isp.cast::<Intrsource>();
    // SAFETY: a fresh allocation, written once before use; the source lives forever.
    unsafe { isp.write(Intrsource::new()) };
    // SAFETY: as above.
    let src: &'static Intrsource = unsafe { isp.as_ref() };
    src.is_recurse.set(recurse);
    src.is_resume.set(resume);
    fake.ih_level.set(level);
    src.is_handlers.set(ptr::from_ref(fake));
    src.is_pic.set(pic);
    src
}

/// `cpu_intr_init`: initialize all handlers that aren't dynamically allocated, and exist for
/// each CPU.
pub fn cpu_intr_init(ci: &CpuInfo) {
    ci.ci_isources[SIR_CLOCK as usize].set(fixed_source(
        Xsoftclock as *const () as usize,
        Xsoftclock as *const () as usize,
        &FAKE_SOFTCLOCK_INTRHAND,
        IPL_SOFTCLOCK,
        &SOFTINTR_PIC,
    ));
    ci.ci_isources[SIR_NET as usize].set(fixed_source(
        Xsoftnet as *const () as usize,
        Xsoftnet as *const () as usize,
        &FAKE_SOFTNET_INTRHAND,
        IPL_SOFTNET,
        &SOFTINTR_PIC,
    ));
    ci.ci_isources[SIR_TTY as usize].set(fixed_source(
        Xsofttty as *const () as usize,
        Xsofttty as *const () as usize,
        &FAKE_SOFTTTY_INTRHAND,
        IPL_SOFTTTY,
        &SOFTINTR_PIC,
    ));
    // NLAPIC > 0
    ci.ci_isources[LIR_TIMER as usize].set(fixed_source(
        Xrecurse_lapic_ltimer as *const () as usize,
        Xresume_lapic_ltimer as *const () as usize,
        &FAKE_TIMER_INTRHAND,
        IPL_CLOCK,
        &LOCAL_PIC,
    ));
    #[cfg(feature = "multiprocessor")]
    {
        ci.ci_isources[LIR_IPI as usize].set(fixed_source(
            Xrecurse_lapic_ipi as *const () as usize,
            Xresume_lapic_ipi as *const () as usize,
            &FAKE_IPI_INTRHAND,
            IPL_IPI,
            &LOCAL_PIC,
        ));
        // NXCALL > 0: the SIR_XCALL source (Xxcallintr): kern_xcall.c is not ported.
    }
    // NXEN, NHYPERV: not configured.

    intr_calculatemasks(ci);
}

/// `intr_printconfig`: the `INTRDEBUG` dump of every CPU's masks and sources.
pub fn intr_printconfig() {
    #[cfg(feature = "debug")]
    {
        let mut next: *const CpuInfo = cpu_info_primary();
        // SAFETY: the CPU list links static cpu_info structures.
        while let Some(ci) = unsafe { next.as_ref() } {
            printf(format_args!(
                "cpu{}: interrupt masks:\n",
                ci.ci_apicid.get()
            ));
            for i in 0..NIPL {
                printf(format_args!(
                    "IPL {i} mask {:x} unmask {:x}\n",
                    ci.ci_imask[i].get(),
                    ci.ci_iunmask[i].get()
                ));
            }
            for i in 0..MAX_INTR_SOURCES {
                let Some(isp) = source(ci, i) else {
                    continue;
                };
                // SAFETY: a source's pic is a static Pic.
                let pic = unsafe { &*isp.is_pic.get() };
                printf(format_args!(
                    "cpu{} source {i} is pin {} from pic {} maxlevel {}\n",
                    ci.ci_apicid.get(),
                    isp.is_pin.get(),
                    pic.pic_name,
                    isp.is_maxlevel.get()
                ));
                for ih in handlers(isp) {
                    printf(format_args!(
                        "\thandler {:?} level {}\n",
                        ih.ih_fun.get().map(|f| f as usize),
                        ih.ih_level.get()
                    ));
                }
            }
            next = ci.ci_next.get();
        }
    }
}

/// `intr_barrier`: waits until no CPU runs the handler.
pub fn intr_barrier(cookie: NonNull<Intrhand>) {
    // SAFETY: an established handler.
    let ih = unsafe { cookie.as_ref() };
    // SAFETY: `ih_cpu` is null or the static cpu_info `intr_establish` recorded.
    sched_barrier(unsafe { ih.ih_cpu.get().as_ref() });
}

/// `intr_set_wakeup`: marks the handler as one that may fire while suspended.
pub fn intr_set_wakeup(cookie: NonNull<Intrhand>) {
    // SAFETY: an established handler.
    let ih = unsafe { cookie.as_ref() };
    ih.ih_flags.set(ih.ih_flags.get() | IPL_WAKEUP);
}

// intr_enable_wakeup / intr_disable_wakeup: option SUSPEND, not configured.

/// `splraise`: add a mask to cpl, and return the old value of cpl.
pub fn splraise(nlevel: i32) -> i32 {
    let ci = curcpu();

    kassert!(nlevel >= IPL_NONE);

    let olevel = ci.ci_ilevel.get();
    ci.ci_ilevel.set(ci.ci_ilevel.get().max(nlevel));
    olevel
}

/// `spllower`: restore a value to cpl (unmasking interrupts). If any unmasked interrupts
/// are pending, call `Xspllower()` to process them.
pub fn spllower(nlevel: i32) -> i32 {
    let ci = curcpu();

    let imask = ci.ci_iunmask[nlevel as usize].get();
    let olevel = ci.ci_ilevel.get();

    let flags = intr_disable();
    if ci.ci_ipending.load(Ordering::Relaxed) & imask != 0 {
        // SAFETY: interrupts are disabled, this is kernel context and `nlevel` is an IPL.
        unsafe { Xspllower(nlevel) };
    } else {
        ci.ci_ilevel.set(nlevel);
        // SAFETY: `flags` is this CPU's saved state.
        unsafe { intr_restore(flags) };
    }
    olevel
}

/// `softintr`: software interrupt registration. We hand-code this to ensure that it's
/// atomic.
pub fn softintr(si_level: i32) {
    let ci = curcpu();
    let sir = SOFTINTR_TO_SSIR[si_level as usize];
    ci.ci_ipending.fetch_or(1u64 << sir, Ordering::Relaxed);
}

/// `dosoftint`: runs the soft interrupt handlers of `si_level`, from `Xsoft*`.
#[unsafe(no_mangle)]
pub extern "C" fn dosoftint(si_level: i32) {
    let ci = curcpu();

    let floor = ci.ci_handled_intr_level.get();
    ci.ci_handled_intr_level.set(ci.ci_ilevel.get());
    softintr_dispatch(si_level);
    ci.ci_handled_intr_level.set(floor);
}
