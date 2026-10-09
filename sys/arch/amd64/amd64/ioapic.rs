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

/*
 * Copyright (c) 1999 Stefan Grefen
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
 *      This product includes software developed by the NetBSD
 *      Foundation, Inc. and its contributors.
 * 4. Neither the name of The NetBSD Foundation nor the names of its
 *    contributors may be used to endorse or promote products derived
 *    from this software without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY AUTHOR AND CONTRIBUTORS ``AS IS'' AND ANY
 * EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR AND CONTRIBUTORS BE LIABLE
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
//! The I/O APIC (Intel 82093AA): `arch/amd64/amd64/ioapic.c`.
//!
//! Upstream: sys/arch/amd64/amd64/ioapic.c @ 3ce1f3f79392
//!
//! `acpimadt` (or `mpbios`) attaches one `ioapic` per I/O APIC at mainbus. Each is a `struct
//! pic` whose pins `intr_establish` routes: while `ioapic_cold`, `ioapic_addroute` only
//! records the pin's vector, type and CPU; `ioapic_enable` (`cpu_configure`, after mainbus)
//! then programs every recorded pin, after which routes are written as they are added. The
//! interrupt stubs (`ioapic_edge_stubs`, `ioapic_level_stubs`, `vector.S`) acknowledge at
//! the local APIC and mask a level-triggered pin while its handler is deferred.
//!
//! ## Deviations
//! - The softc's `struct pic` is reached by offset (`i82093var.rs`): `ioapic_from_pic` is the
//!   C's `(struct ioapic_softc *)pic` cast. Its name is the device's `dv_xname`.
//! - `ioapic_hwmask`/`ioapic_hwunmask` are `extern "C"` under their C names, because the
//!   level stubs call them (`ioapic_mask`/`ioapic_unmask` in `vector.S`); the pic's
//!   `pic_hwmask`/`pic_hwunmask` are Rust-ABI shims around them.
//! - `ioapic_find(MPS_ALL_APICS)` with more than one I/O APIC panics, as the C does.
//! - `ioapic_activate` returns `Ok(())` (`ca_activate`'s `Result`), the C's 0.
//! - `ioapic_attach` adds the softc to `ioapics` once its registers are mapped and its pic
//!   is set up (the C adds it first): `ioapic_find` hands out only usable I/O APICs, and a
//!   failed map leaves none behind.
//! - The pins are `mallocarray`ed with `M_WAITOK` as in C; a failed allocation panics.

use core::ffi::c_void;
use core::mem::{MaybeUninit, offset_of};
use core::ptr;
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicPtr, Ordering};

use crate::arch::amd64::amd64::apic::apic_format_redir;
use crate::arch::amd64::amd64::bus_space::{bus_space_map, bus_space_read_4, bus_space_write_4};
use crate::arch::amd64::amd64::mainbus::MP_VERBOSE;
use crate::arch::amd64::amd64::vector::{ioapic_edge_stubs, ioapic_level_stubs};
use crate::arch::amd64::include::apicvar::{ApicAttachArgs, IOAPIC_PICMODE};
use crate::arch::amd64::include::cpu::CpuInfo;
use crate::arch::amd64::include::cpufunc::{intr_disable, intr_restore};
use crate::arch::amd64::include::i82093reg::*;
use crate::arch::amd64::include::i82093var::{IoapicPin, IoapicSoftc};
use crate::arch::amd64::include::intr::Intrstub;
use crate::arch::amd64::include::intrdefs::{IST_LEVEL, IST_NONE};
use crate::arch::amd64::include::mpbiosreg::{MPS_ALL_APICS, MPS_INTPO_DEF};
use crate::arch::amd64::include::param::PAGE_SIZE;
use crate::arch::amd64::include::pic::{PIC_IOAPIC, Pic};
use crate::arch::amd64::include::pio::outb;
#[cfg(feature = "multiprocessor")]
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_malloc::mallocarray;
use crate::kern::subr_prf::panic;
use crate::kprintf;
use crate::sys::device::{CD_COCOVM, CfMatch, Cfattach, Cfdriver, DV_DULL, DVACT_RESUME, Device};
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_DEVBUF, M_WAITOK};

/// `ioapic_bsp_id`: the boot processor's APIC ID (`cpu_attach`).
pub static IOAPIC_BSP_ID: AtomicI32 = AtomicI32::new(0);
/// `ioapic_cold`: routes are only recorded until `ioapic_enable`.
pub static IOAPIC_COLD: AtomicBool = AtomicBool::new(true);

/// `ioapics`: head of linked list.
pub static IOAPICS: AtomicPtr<IoapicSoftc> = AtomicPtr::new(ptr::null_mut());
/// `nioapics`: number attached.
pub static NIOAPICS: AtomicI32 = AtomicI32::new(0);
/// `ioapic_vecbase`: the next global interrupt base for a table that gives none.
static IOAPIC_VECBASE: AtomicI32 = AtomicI32::new(0);

/// The softc whose `sc_pic` is `pic`: the C's `(struct ioapic_softc *)pic`.
///
/// # Safety
///
/// `pic` is the `sc_pic` of an `IoapicSoftc` (`ioapic_attach` hands out no other).
unsafe fn ioapic_from_pic(pic: *const Pic) -> &'static IoapicSoftc {
    // SAFETY: the caller's guarantee: going back by the field's offset lands on the softc,
    // which is never freed.
    unsafe {
        &*pic
            .cast::<u8>()
            .sub(offset_of!(IoapicSoftc, sc_pic))
            .cast::<IoapicSoftc>()
    }
}

/// The attached I/O APICs, from `ioapics` on through `sc_next`.
fn ioapics() -> impl Iterator<Item = &'static IoapicSoftc> {
    let mut sc = IOAPICS.load(Ordering::Acquire).cast_const();
    core::iter::from_fn(move || {
        // SAFETY: the list links softcs that are never freed.
        let s = unsafe { sc.as_ref() }?;
        sc = s.sc_next.get();
        Some(s)
    })
}

/// `ioapic_lock`: interrupts off and, with `MULTIPROCESSOR`, the pic's mutex.
fn ioapic_lock(sc: &IoapicSoftc) -> u64 {
    let flags = intr_disable();
    #[cfg(feature = "multiprocessor")]
    mtx_enter(&sc.sc_mutex);
    #[cfg(not(feature = "multiprocessor"))]
    let _ = sc;
    flags
}

/// `ioapic_unlock`.
fn ioapic_unlock(sc: &IoapicSoftc, flags: u64) {
    #[cfg(feature = "multiprocessor")]
    mtx_leave(&sc.sc_mutex);
    #[cfg(not(feature = "multiprocessor"))]
    let _ = sc;
    // SAFETY: `flags` came from the matching `ioapic_lock`.
    unsafe { intr_restore(flags) };
}

/// `ioapic_read_ul`: register read, the caller holding the lock.
fn ioapic_read_ul(sc: &IoapicSoftc, regid: i32) -> u32 {
    let (t, h) = (sc.sc_memt.get(), sc.sc_memh.get());
    bus_space_write_4(t, h, IOAPIC_REG, regid as u32);
    bus_space_read_4(t, h, IOAPIC_DATA)
}

/// `ioapic_write_ul`: register write, the caller holding the lock.
fn ioapic_write_ul(sc: &IoapicSoftc, regid: i32, val: u32) {
    let (t, h) = (sc.sc_memt.get(), sc.sc_memh.get());
    bus_space_write_4(t, h, IOAPIC_REG, regid as u32);
    bus_space_write_4(t, h, IOAPIC_DATA, val);
}

/// `ioapic_read`.
fn ioapic_read(sc: &IoapicSoftc, regid: i32) -> u32 {
    let flags = ioapic_lock(sc);
    let val = ioapic_read_ul(sc, regid);
    ioapic_unlock(sc, flags);
    val
}

/// `ioapic_write`.
fn ioapic_write(sc: &IoapicSoftc, regid: i32, val: u32) {
    let flags = ioapic_lock(sc);
    ioapic_write_ul(sc, regid, val);
    ioapic_unlock(sc, flags);
}

/// `ioapic_find(apicid)`: the I/O APIC with `apicid`.
pub fn ioapic_find(apicid: i32) -> Option<&'static IoapicSoftc> {
    if apicid == MPS_ALL_APICS {
        // XXX mpbios-specific; XXX kludge for all-ioapics interrupt support on single
        // ioapic systems
        if NIOAPICS.load(Ordering::Relaxed) <= 1 {
            return ioapics().next();
        }
        panic(format_args!(
            "unsupported: all-ioapics interrupt with >1 ioapic"
        ));
    }

    ioapics().find(|sc| sc.sc_apicid.get() == apicid)
}

/// `ioapic_find_bybase(vec)`: for the case the I/O APICs were configured using ACPI, there
/// must be an option to match global ACPI interrupts with APICs.
pub fn ioapic_find_bybase(vec: i32) -> Option<&'static IoapicSoftc> {
    ioapics().find(|sc| {
        vec >= sc.sc_apic_vecbase.get() && vec < sc.sc_apic_vecbase.get() + sc.sc_apic_sz.get()
    })
}

/// `ioapic_add`: links `sc` at the head of `ioapics`.
fn ioapic_add(sc: &'static IoapicSoftc) {
    sc.sc_next.set(IOAPICS.load(Ordering::Relaxed));
    IOAPICS.store(ptr::from_ref(sc).cast_mut(), Ordering::Release);
    NIOAPICS.fetch_add(1, Ordering::Relaxed);
}

/// `ioapic_print_redir(sc, why, pin)`.
pub fn ioapic_print_redir(sc: &IoapicSoftc, why: &str, pin: i32) {
    let redirlo = ioapic_read(sc, ioapic_redlo(pin));
    let redirhi = ioapic_read(sc, ioapic_redhi(pin));

    apic_format_redir(sc.sc_dev.xname(), why, pin, redirhi, redirlo);
}

/// `ioapic_ca`.
pub static IOAPIC_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<IoapicSoftc>(),
    ca_match: Some(ioapic_match),
    ca_attach: ioapic_attach,
    ca_detach: None,
    ca_activate: Some(ioapic_activate),
};

/// `ioapic_cd`.
pub static IOAPIC_CD: Cfdriver = Cfdriver::new(b"ioapic", DV_DULL, CD_COCOVM);

/// `ioapic_match(parent, match, aux)`: the attachment arguments name `ioapic`.
pub fn ioapic_match(_parent: Option<&Device>, match_: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: mainbus's children get arguments that start with the bus name; an "ioapic"
    // one is a `struct apic_attach_args` (`acpimadt`), which is all `aaa_name` reads.
    let aaa = unsafe { &*aux.cast_const().cast::<ApicAttachArgs>() };

    i32::from(aaa.aaa_name == match_.cfdata().cf_driver.cd_name)
}

/// `ioapic_set_id`: reprogram the APIC ID, and check that it actually got set.
fn ioapic_set_id(sc: &IoapicSoftc) {
    let apicid = sc.sc_apicid.get() as u32;
    ioapic_write(
        sc,
        IOAPIC_ID,
        (ioapic_read(sc, IOAPIC_ID) & !IOAPIC_ID_MASK) | (apicid << IOAPIC_ID_SHIFT),
    );

    let apic_id = (ioapic_read(sc, IOAPIC_ID) & IOAPIC_ID_MASK) >> IOAPIC_ID_SHIFT;

    if apic_id != apicid {
        kprintf!(", can't remap");
    } else {
        kprintf!(", remapped");
    }
}

/// `ioapic_attach(parent, self, aux)`.
pub fn ioapic_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: `self_` was made for `ioapic_ca`, whose softc is an `IoapicSoftc`, which is
    // never freed (an I/O APIC does not detach).
    let sc: &'static IoapicSoftc = unsafe { &*ptr::from_ref(self_.softc::<IoapicSoftc>()) };
    // SAFETY: matched by `ioapic_match`: a `struct apic_attach_args`.
    let aaa = unsafe { &*aux.cast_const().cast::<ApicAttachArgs>() };

    sc.sc_flags.set(aaa.flags);
    sc.sc_apicid.set(aaa.apic_id);

    kprintf!(": apid {}", aaa.apic_id);

    if ioapic_find(aaa.apic_id).is_some() {
        kprintf!(", duplicate apic id (ignored)\n");
        return;
    }

    kprintf!(" pa 0x{:x}", aaa.apic_address);

    sc.sc_memt.set(aaa.apic_memt);
    // SAFETY: the firmware's table names this page as the I/O APIC's registers.
    match unsafe { bus_space_map(aaa.apic_memt, aaa.apic_address, PAGE_SIZE, 0) } {
        Ok(h) => sc.sc_memh.set(h),
        Err(_) => {
            kprintf!(", map failed\n");
            return;
        }
    }

    // SAFETY: the softc's name is written by `config_attach` before attach and never again,
    // and the softc is never freed, so the name lives as long as the pic.
    let name: &'static str = unsafe { &*ptr::from_ref(sc.sc_dev.xname()) };
    // SAFETY: nothing has seen the pic yet: this is its one write.
    unsafe {
        (*sc.sc_pic.get()) = MaybeUninit::new(Pic {
            pic_name: name,
            pic_type: PIC_IOAPIC,
            pic_hwmask: Some(ioapic_pic_hwmask),
            pic_hwunmask: Some(ioapic_pic_hwunmask),
            pic_addroute: Some(ioapic_addroute),
            pic_delroute: Some(ioapic_delroute),
            pic_allocidtvec: None,
            pic_edge_stubs: Some(ioapic_edge_stubs_table),
            pic_level_stubs: Some(ioapic_level_stubs_table),
        });
    }
    // MULTIPROCESSOR: mtx_init(&sc->sc_pic.pic_mutex, IPL_NONE): the zeroed sc_mutex is a
    // free IPL_NONE mutex.

    // The C adds the softc to `ioapics` before mapping; it is added here once its registers
    // and pic are set up, which nothing between the two reads.
    ioapic_add(sc);

    let ver_sz = ioapic_read(sc, IOAPIC_VER);
    sc.sc_apic_vers
        .set(((ver_sz & IOAPIC_VER_MASK) >> IOAPIC_VER_SHIFT) as i32);
    sc.sc_apic_sz
        .set(((ver_sz & IOAPIC_MAX_MASK) >> IOAPIC_MAX_SHIFT) as i32 + 1);

    if aaa.apic_vecbase != -1 {
        sc.sc_apic_vecbase.set(aaa.apic_vecbase);
    } else {
        // XXX this assumes ordering of ioapics in the table. Only needed for broken BIOS
        // workaround (see mpbios.c)
        sc.sc_apic_vecbase
            .set(IOAPIC_VECBASE.fetch_add(sc.sc_apic_sz.get(), Ordering::Relaxed));
    }

    if MP_VERBOSE.load(Ordering::Relaxed) != 0 {
        kprintf!(
            ", {} mode",
            if aaa.flags & IOAPIC_PICMODE != 0 {
                "PIC"
            } else {
                "virtual wire"
            }
        );
    }

    kprintf!(
        ", version {:x}, {} pins",
        sc.sc_apic_vers.get(),
        sc.sc_apic_sz.get()
    );

    let apic_id = ((ioapic_read(sc, IOAPIC_ID) & IOAPIC_ID_MASK) >> IOAPIC_ID_SHIFT) as i32;

    let n = sc.sc_apic_sz.get() as usize;
    let Some(pins) = mallocarray(n, size_of::<IoapicPin>(), M_DEVBUF, M_WAITOK) else {
        panic(format_args!("ioapic_attach: mallocarray"));
    };
    let pins = pins.cast::<IoapicPin>();
    for i in 0..n {
        // SAFETY: `i` is inside the fresh allocation of `n` pins, each written once.
        unsafe { pins.add(i).write(IoapicPin::new()) };
    }
    sc.sc_pins.set(pins.as_ptr().cast_const());

    // In case the APIC is not initialized to the correct ID do it now. Maybe we should record
    // the original ID for interrupt mapping later ...
    if apic_id != sc.sc_apicid.get() {
        if MP_VERBOSE.load(Ordering::Relaxed) != 0 {
            kprintf!("\n{}: misconfigured as apic {}", name, apic_id);
        }
        ioapic_set_id(sc);
    }

    kprintf!("\n");
}

/// `ioapic_activate(self, act)`: on resume, reset the APIC id, like we do on boot.
pub fn ioapic_activate(self_: &Device, act: i32) -> Result<(), Errno> {
    // SAFETY: `self_` was made for `ioapic_ca`.
    let sc = unsafe { self_.softc::<IoapicSoftc>() };

    if act == DVACT_RESUME {
        ioapic_write(
            sc,
            IOAPIC_ID,
            (ioapic_read(sc, IOAPIC_ID) & !IOAPIC_ID_MASK)
                | ((sc.sc_apicid.get() as u32) << IOAPIC_ID_SHIFT),
        );
    }

    Ok(())
}

/// `apic_set_redir(sc, pin, idt_vec, ci)`: writes pin `pin`'s redirection entry.
pub fn apic_set_redir(sc: &IoapicSoftc, pin: i32, idt_vec: i32, ci: Option<&CpuInfo>) {
    let mut redhi: u32 = 0;
    let pp = &sc.pins()[pin as usize];
    let map = pp.ip_map.get();
    let mut redlo = map.map_or(IOAPIC_REDLO_MASK, |m| m.redir);
    let delmode = (redlo & IOAPIC_REDLO_DEL_MASK) >> IOAPIC_REDLO_DEL_SHIFT;

    // XXX magic numbers
    if delmode != 0 && delmode != 1 {
        // leave it as the table says
    } else if pp.ip_type.get() == IST_NONE {
        redlo |= IOAPIC_REDLO_MASK;
    } else {
        redlo |= idt_vec as u32 & 0xff;
        redlo &= !IOAPIC_REDLO_DEL_MASK;
        redlo |= IOAPIC_REDLO_DEL_FIXED << IOAPIC_REDLO_DEL_SHIFT;
        redlo &= !IOAPIC_REDLO_DSTMOD;

        // Destination: BSP CPU
        //
        // XXX will want to distribute interrupts across cpu's eventually. most likely,
        // we'll want to vector each interrupt to a specific CPU and load-balance across
        // cpu's. but there's no point in doing that until after most interrupts run
        // without the kernel lock.
        let apicid = ci.map_or(0, |ci| ci.ci_apicid.get());
        redhi |= apicid << IOAPIC_REDHI_DEST_SHIFT;

        // XXX derive this bit from BIOS info
        if pp.ip_type.get() == IST_LEVEL {
            redlo |= IOAPIC_REDLO_LEVEL;
        } else {
            redlo &= !IOAPIC_REDLO_LEVEL;
        }
        if map.is_some_and(|m| m.flags & 3 == MPS_INTPO_DEF) {
            if pp.ip_type.get() == IST_LEVEL {
                redlo |= IOAPIC_REDLO_ACTLO;
            } else {
                redlo &= !IOAPIC_REDLO_ACTLO;
            }
        }
    }
    // Do atomic write
    ioapic_write(sc, ioapic_redlo(pin), IOAPIC_REDLO_MASK);
    ioapic_write(sc, ioapic_redhi(pin), redhi);
    ioapic_write(sc, ioapic_redlo(pin), redlo);
    if MP_VERBOSE.load(Ordering::Relaxed) != 0 {
        ioapic_print_redir(sc, "int", pin);
    }
}

/// `ioapic_enable`: throw the switch and enable interrupts.
pub fn ioapic_enable() {
    IOAPIC_COLD.store(false, Ordering::Relaxed);

    let Some(first) = ioapics().next() else {
        return;
    };

    if first.sc_flags.get() & IOAPIC_PICMODE != 0 {
        kprintf!(
            "{}: writing to IMCR to disable pics\n",
            first.sc_dev.xname()
        );
        // SAFETY: the IMCR ports, which an MP table in PIC mode says exist.
        unsafe {
            outb(IMCR_ADDR, IMCR_REGISTER);
            outb(IMCR_DATA, IMCR_APIC);
        }
    }

    for sc in ioapics() {
        if MP_VERBOSE.load(Ordering::Relaxed) != 0 {
            kprintf!("{}: enabling\n", sc.sc_dev.xname());
        }

        for (p, ip) in sc.pins().iter().enumerate() {
            if ip.ip_type.get() != IST_NONE {
                // SAFETY: a routed pin names the static `cpu_info` `intr_establish` chose.
                let ci = unsafe { ip.ip_cpu.get().as_ref() };
                apic_set_redir(sc, p as i32, ip.ip_vector.get(), ci);
            }
        }
    }
}

/// `ioapic_hwmask(pic, pin)`: masks `pin`; also called by the level stubs (`vector.S`).
///
/// # Safety
///
/// `pic` is the `sc_pic` of an attached I/O APIC (what the stubs pass: `is_pic` of an I/O
/// APIC source).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ioapic_hwmask(pic: *const Pic, pin: i32) {
    // SAFETY: the caller's guarantee.
    let sc = unsafe { ioapic_from_pic(pic) };

    if IOAPIC_COLD.load(Ordering::Relaxed) {
        return;
    }
    let flags = ioapic_lock(sc);
    let mut redlo = ioapic_read_ul(sc, ioapic_redlo(pin));
    redlo |= IOAPIC_REDLO_MASK;
    redlo &= !IOAPIC_REDLO_RIRR;
    ioapic_write_ul(sc, ioapic_redlo(pin), redlo);
    ioapic_unlock(sc, flags);
}

/// `ioapic_hwunmask(pic, pin)`; also called by the level stubs (`vector.S`).
///
/// # Safety
///
/// As for [`ioapic_hwmask`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ioapic_hwunmask(pic: *const Pic, pin: i32) {
    // SAFETY: the caller's guarantee.
    let sc = unsafe { ioapic_from_pic(pic) };

    if IOAPIC_COLD.load(Ordering::Relaxed) {
        return;
    }
    let flags = ioapic_lock(sc);
    let mut redlo = ioapic_read_ul(sc, ioapic_redlo(pin));
    redlo &= !IOAPIC_REDLO_MASK;
    redlo &= !IOAPIC_REDLO_RIRR;
    ioapic_write_ul(sc, ioapic_redlo(pin), redlo);
    ioapic_unlock(sc, flags);
}

/// `pic_hwmask` of an I/O APIC.
fn ioapic_pic_hwmask(pic: &Pic, pin: i32) {
    // SAFETY: only an I/O APIC's pic has this function.
    unsafe { ioapic_hwmask(pic, pin) }
}

/// `pic_hwunmask` of an I/O APIC.
fn ioapic_pic_hwunmask(pic: &Pic, pin: i32) {
    // SAFETY: only an I/O APIC's pic has this function.
    unsafe { ioapic_hwunmask(pic, pin) }
}

/// `ioapic_addroute(pic, ci, pin, idtvec, type)`.
pub fn ioapic_addroute(pic: &Pic, ci: &CpuInfo, pin: i32, idtvec: i32, type_: i32) {
    // SAFETY: only an I/O APIC's pic has this function.
    let sc = unsafe { ioapic_from_pic(pic) };

    let pp = &sc.pins()[pin as usize];
    pp.ip_type.set(type_);
    pp.ip_vector.set(idtvec);
    pp.ip_cpu.set(ptr::from_ref(ci));
    if IOAPIC_COLD.load(Ordering::Relaxed) {
        return;
    }
    apic_set_redir(sc, pin, idtvec, Some(ci));
}

/// `ioapic_delroute(pic, ci, pin, idtvec, type)`.
pub fn ioapic_delroute(pic: &Pic, _ci: &CpuInfo, pin: i32, _idtvec: i32, _type: i32) {
    // SAFETY: only an I/O APIC's pic has this function.
    let sc = unsafe { ioapic_from_pic(pic) };

    if IOAPIC_COLD.load(Ordering::Relaxed) {
        sc.pins()[pin as usize].ip_type.set(IST_NONE);
        return;
    }
    ioapic_pic_hwmask(pic, pin);
}

/// `ioapic_edge_stubs[]` as a slice.
pub fn ioapic_edge_stubs_table() -> &'static [Intrstub] {
    // SAFETY: a read-only table `vector.S` defines.
    unsafe { &*ptr::addr_of!(ioapic_edge_stubs) }
}

/// `ioapic_level_stubs[]` as a slice.
pub fn ioapic_level_stubs_table() -> &'static [Intrstub] {
    // SAFETY: a read-only table `vector.S` defines.
    unsafe { &*ptr::addr_of!(ioapic_level_stubs) }
}

/// `ioapic_dump` (`DDB`): prints the redirection entry of every routed pin.
pub fn ioapic_dump() {
    for sc in ioapics() {
        for (p, ip) in sc.pins().iter().enumerate() {
            if ip.ip_type.get() != IST_NONE {
                ioapic_print_redir(sc, "dump", p as i32);
            }
        }
    }
}
/* </CODE> */
