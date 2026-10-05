/* $OpenBSD: acpimadt.c,v 1.41 2025/09/16 12:18:10 hshoexer Exp $ */
/* <LICENSES> */
/*
 * Copyright (c) 2006 Mark Kettenis <kettenis@openbsd.org>
 *
 * Permission to use, copy, modify, and distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
 * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
 * ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
 * OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 */
/* </LICENSES> */

//! acpimadt(4): the ACPI Multiple APIC Description Table, `dev/acpi/acpimadt.c`.
//!
//! Upstream: sys/dev/acpi/acpimadt.c @ 3ce1f3f79392
//!
//! The MADT lists the processors' local APICs and the I/O APICs, and how the ISA interrupts
//! are wired to the I/O APIC pins. `acpimadt0` tells the firmware the machine runs in APIC
//! mode (`\_PIC(1)`), installs the MP bus table (`mp_busses`, with the ISA bus), maps the
//! boot processor's local APIC at the table's address, attaches a `cpu` per enabled local
//! APIC and an `ioapic` per I/O APIC at mainbus, then records the interrupt source
//! overrides and the local APIC NMI pins and maps the other ISA interrupts one to one.
//!
//! This is x86 code (its C includes `<machine/i82093var.h>` and friends); here it is
//! machine-independent and reaches those headers through `machine::mpconfig`, whose
//! machines without an I/O APIC never configure it.
//!
//! ## Deviations
//! - The table is read as bytes with unaligned copies of the packed entry structures
//!   (`acpireg.rs`), after `acpimadt_validate` checked every entry's bounds, as the C walks
//!   it through pointers.
//! - The `struct cpu_attach_args` and `struct apic_attach_args` are built by the machine
//!   (`mp_attach_cpu`, `mp_attach_ioapic`), which also supplies `mp_cpu_funcs` with
//!   `MULTIPROCESSOR`; the `__i386__` signature/feature members do not exist on amd64.
//! - `mp_intrs` is filled in a vector of the `nlapic_nmis` capacity and published when the
//!   second pass ends (the C publishes the array first and counts `mp_nintrs` up); nothing
//!   reads it before `cpu_configure`'s `lapic_set_lvt`.
//! - An override or ISA pin whose global interrupt no I/O APIC covers is skipped: the C
//!   dereferences the NULL `ioapic_find_bybase` returns. The mappings are `Box`ed and never
//!   freed (the C's `M_DEVBUF` mallocs, freed only for a bogus override).
//! - `dprintf` (`ACPI_DEBUG`) is not configured.

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::{AtomicU8, Ordering};

use crate::dev::acpi::acpireg::*;
use crate::dev::acpi::acpivar::{AcpiAttachArgs, AcpiSoftc};
use crate::dev::acpi::amltypes::AmlValue;
use crate::dev::acpi::dsdt::aml_evalname;
use crate::kern::init_main::NCPUSFOUND;
use crate::kern::subr_prf::Str;
use crate::kprintf;
use crate::machine::mpconfig::*;
use crate::sys::device::{CD_COCOVM, CfMatch, Cfattach, Cfdriver, DV_DULL, Device, UNCONF};
use crate::sys::types::Paddr;

/// `acpimadt_ca`.
pub static ACPIMADT_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<Device>(),
    ca_match: Some(acpimadt_match),
    ca_attach: acpimadt_attach,
    ca_detach: None,
    ca_activate: None,
};

/// `acpimadt_cd`.
pub static ACPIMADT_CD: Cfdriver = Cfdriver::new(b"acpimadt", DV_DULL, CD_COCOVM);

/// `acpimadt_busses[256]`: the MP bus table the MADT stands for.
pub static ACPIMADT_BUSSES: [MpBus; 256] = [const { MpBus::new() }; 256];
/// `acpimadt_isa_bus`.
pub static ACPIMADT_ISA_BUS: MpBus = MpBus::new();

/// `lapic_map[256]`: the local APIC ID of each ACPI processor ID.
static LAPIC_MAP: [AtomicU8; 256] = [const { AtomicU8::new(0) }; 256];

/// The table `aaa_table` points to, as its `hdr.length` bytes.
///
/// # Safety
///
/// `table` is a table acpi0 mapped and copied whole (`acpi_maptable`), never freed.
unsafe fn table_bytes(table: *const c_void) -> &'static [u8] {
    // SAFETY: the caller's guarantee: a whole table starts with its header.
    let hdr = unsafe { ptr::read_unaligned(table.cast::<AcpiTableHeader>()) };
    // SAFETY: as above: the copy holds `length` bytes.
    unsafe { core::slice::from_raw_parts(table.cast::<u8>(), hdr.length as usize) }
}

/// A copy of the packed structure `T` at `off` in `b`, or `None` past the end.
fn read_at<T: Copy>(b: &[u8], off: usize) -> Option<T> {
    let end = off.checked_add(size_of::<T>())?;
    let bytes = b.get(off..end)?;
    // SAFETY: `bytes` holds `size_of::<T>()` bytes; `T` is a packed table structure of
    // integers, valid for any bits, read without alignment.
    Some(unsafe { ptr::read_unaligned(bytes.as_ptr().cast::<T>()) })
}

/// The MADT's entries, as their offset, type and length, from after the fixed part to
/// `hdr.length` (the C's `addr` walk), stopping at a zero length.
fn madt_entries(b: &[u8]) -> impl Iterator<Item = (usize, u8, u8)> + '_ {
    let mut addr = size_of::<AcpiMadt>();
    core::iter::from_fn(move || {
        if addr >= b.len() {
            return None;
        }
        let apic_type = *b.get(addr)?;
        let length = *b.get(addr + 1)?;
        if length == 0 {
            return None;
        }
        let at = addr;
        addr += usize::from(length);
        Some((at, apic_type, length))
    })
}

/// `acpimadt_match(parent, match, aux)`: an MADT table.
pub fn acpimadt_match(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: acpi0 hands its children `struct acpi_attach_args`.
    let aaa = unsafe { &*aux.cast_const().cast::<AcpiAttachArgs>() };

    // If we do not have a table, it is not us
    if aaa.aaa_table.is_null() {
        return 0;
    }

    // If it is an MADT table, we can attach
    // SAFETY: acpi0's table arguments point at a whole table.
    let hdr = unsafe { ptr::read_unaligned(aaa.aaa_table.cast_const().cast::<AcpiTableHeader>()) };
    if hdr.signature != *MADT_SIG {
        return 0;
    }

    1
}

/// `acpimadt_validate(madt)`: every entry fits the table and has its type's length.
pub fn acpimadt_validate(b: &[u8]) -> bool {
    let len = b.len();
    let mut addr = size_of::<AcpiMadt>();

    while addr < len {
        let (Some(&apic_type), Some(&length)) = (b.get(addr), b.get(addr + 1)) else {
            return false;
        };
        let length = usize::from(length);

        if length < 2 {
            return false;
        }

        if addr + length > len {
            return false;
        }

        let want = match apic_type {
            ACPI_MADT_LAPIC => Some(size_of::<AcpiMadtLapic>()),
            ACPI_MADT_IOAPIC => Some(size_of::<AcpiMadtIoapic>()),
            ACPI_MADT_OVERRIDE => Some(size_of::<AcpiMadtOverride>()),
            ACPI_MADT_NMI => Some(size_of::<AcpiMadtNmi>()),
            ACPI_MADT_LAPIC_NMI => Some(size_of::<AcpiMadtLapicNmi>()),
            ACPI_MADT_LAPIC_OVERRIDE => Some(size_of::<AcpiMadtLapicOverride>()),
            ACPI_MADT_IO_SAPIC => Some(size_of::<AcpiMadtIoSapic>()),
            ACPI_MADT_LOCAL_SAPIC => Some(size_of::<AcpiMadtLocalSapic>()),
            ACPI_MADT_PLATFORM_INT => Some(size_of::<AcpiMadtPlatformInt>()),
            ACPI_MADT_X2APIC => Some(size_of::<AcpiMadtX2apic>()),
            ACPI_MADT_X2APIC_NMI => Some(size_of::<AcpiMadtX2apicNmi>()),
            _ => None,
        };
        if want.is_some_and(|w| w != length) {
            return false;
        }

        addr += length;
    }

    true
}

/// `acpimadt_cfg_intr(flags, redir)`: the redirection bits of an MP-spec polarity and
/// trigger mode; `false` for a reserved encoding.
pub fn acpimadt_cfg_intr(flags: i32, redir: &mut u32) -> bool {
    let mpspo = (flags >> MPS_INTPO_SHIFT) & MPS_INTPO_MASK;
    let mpstrig = (flags >> MPS_INTTR_SHIFT) & MPS_INTTR_MASK;

    *redir &= !IOAPIC_REDLO_DEL_MASK;
    if mpspo == MPS_INTPO_DEF || mpspo == MPS_INTPO_ACTHI {
        *redir &= !IOAPIC_REDLO_ACTLO;
    } else if mpspo == MPS_INTPO_ACTLO {
        *redir |= IOAPIC_REDLO_ACTLO;
    } else {
        return false;
    }

    *redir |= IOAPIC_REDLO_DEL_LOPRI << IOAPIC_REDLO_DEL_SHIFT;

    if mpstrig == MPS_INTTR_LEVEL {
        *redir |= IOAPIC_REDLO_LEVEL;
    } else if mpstrig == MPS_INTTR_DEF || mpstrig == MPS_INTTR_EDGE {
        *redir &= !IOAPIC_REDLO_LEVEL;
    } else {
        return false;
    }

    true
}

/// `APIC_INT_VIA_APIC | (apicid << APIC_INT_APIC_SHIFT) | (pin << APIC_INT_PIN_SHIFT)`.
fn ioapic_ih(apic: &Ioapic, pin: i32) -> i32 {
    APIC_INT_VIA_APIC | (ioapic_apicid(apic) << APIC_INT_APIC_SHIFT) | (pin << APIC_INT_PIN_SHIFT)
}

/// `acpimadt_attach(parent, self, aux)`.
pub fn acpimadt_attach(parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    let Some(parent) = parent else {
        return;
    };
    // SAFETY: acpimadt attaches at acpi0, whose softc is an `AcpiSoftc`.
    let acpi_sc = unsafe { parent.softc::<AcpiSoftc>() };
    let Some(mainbus) = parent.parent().and_then(Device::parent) else {
        return;
    };
    // SAFETY: acpi0 hands its children `struct acpi_attach_args`.
    let aaa = unsafe { &*aux.cast_const().cast::<AcpiAttachArgs>() };
    // SAFETY: `acpimadt_match` accepted the table acpi0 copied.
    let b = unsafe { table_bytes(aaa.aaa_table.cast_const()) };
    let Some(madt) = read_at::<AcpiMadt>(b, 0) else {
        kprintf!(": invalid, skipping\n");
        return;
    };
    let mut nlapic_nmis = 0;

    // Do some sanity checks before committing to run in APIC mode.
    if !acpimadt_validate(b) {
        kprintf!(": invalid, skipping\n");
        return;
    }

    let local_apic_address = madt.local_apic_address;
    kprintf!(" addr 0x{:x}", local_apic_address);
    if madt.flags & ACPI_APIC_PCAT_COMPAT != 0 {
        kprintf!(": PC-AT compat");
    }
    kprintf!("\n");

    // Tell the BIOS we will be using APIC mode.
    let arg = [AmlValue::integer(1)];
    aml_evalname(Some(acpi_sc), None, b"\\_PIC", &arg, None);

    mp_set_busses(&ACPIMADT_BUSSES, &ACPIMADT_ISA_BUS);

    lapic_boot_init(Paddr::new(local_apic_address as usize));

    // 1st pass, get CPUs and IOAPICs
    for (addr, apic_type, _) in madt_entries(b) {
        match apic_type {
            ACPI_MADT_LAPIC_OVERRIDE => {
                let Some(e) = read_at::<AcpiMadtLapicOverride>(b, addr) else {
                    continue;
                };
                let lapic_address = e.lapic_address;
                if lapic_address != u64::from(local_apic_address) {
                    kprintf!(
                        "{}: ignored LAPIC override 0x{:x}\n",
                        self_.xname(),
                        lapic_address
                    );
                }
            }
            ACPI_MADT_LAPIC => {
                let Some(e) = read_at::<AcpiMadtLapic>(b, addr) else {
                    continue;
                };
                if e.flags & ACPI_PROC_ENABLE == 0 {
                    continue;
                }

                LAPIC_MAP[usize::from(e.acpi_proc_id)].store(e.apic_id, Ordering::Relaxed);

                let bp = lapic_cpu_number() == u32::from(e.apic_id);
                if !bp {
                    NCPUSFOUND.fetch_add(1, Ordering::Relaxed);
                }
                mp_attach_cpu(
                    mainbus,
                    u32::from(e.apic_id),
                    u32::from(e.acpi_proc_id),
                    bp,
                    acpimadt_print,
                );
            }
            ACPI_MADT_IOAPIC => {
                let Some(e) = read_at::<AcpiMadtIoapic>(b, addr) else {
                    continue;
                };
                let Some(memt) = acpi_sc.sc_memt.get() else {
                    continue;
                };
                mp_attach_ioapic(
                    mainbus,
                    memt,
                    i32::from(e.acpi_ioapic_id),
                    e.address as usize,
                    e.global_int_base as i32,
                    acpimadt_print,
                );
            }
            ACPI_MADT_LAPIC_NMI => nlapic_nmis += 1,
            ACPI_MADT_X2APIC => {
                let Some(e) = read_at::<AcpiMadtX2apic>(b, addr) else {
                    continue;
                };
                let (apic_id, flags) = (e.apic_id, e.flags);
                if apic_id > 255 || flags & ACPI_PROC_ENABLE == 0 {
                    continue;
                }

                let bp = lapic_cpu_number() == apic_id;
                if !bp {
                    NCPUSFOUND.fetch_add(1, Ordering::Relaxed);
                }
                mp_attach_cpu(mainbus, apic_id, e.acpi_proc_uid, bp, acpimadt_print);
            }
            _ => {}
        }
    }

    let mut intrs: Vec<MpIntrMap> = Vec::with_capacity(nlapic_nmis);

    // 2nd pass, get interrupt overrides
    for (addr, apic_type, _) in madt_entries(b) {
        match apic_type {
            ACPI_MADT_LAPIC | ACPI_MADT_IOAPIC => {}

            ACPI_MADT_OVERRIDE => {
                let Some(e) = read_at::<AcpiMadtOverride>(b, addr) else {
                    continue;
                };
                let pin = e.global_int as i32;
                let Some(apic) = ioapic_find_bybase(pin) else {
                    continue;
                };

                let mut map = MpIntrMap::new();
                map.ioapic = Some(apic);
                map.ioapic_pin = pin - ioapic_vecbase(apic);
                map.bus_pin = i32::from(e.source);
                map.flags = i32::from(e.flags);

                if !acpimadt_cfg_intr(i32::from(e.flags), &mut map.redir) {
                    kprintf!("{}: bogus override for pin {}\n", self_.xname(), pin);
                    continue;
                }

                map.ioapic_ih = ioapic_ih(apic, pin);

                let map: &'static MpIntrMap = Box::leak(Box::new(map));
                ioapic_set_ip_map(apic, pin, map);

                ACPIMADT_ISA_BUS.push(map);
            }

            ACPI_MADT_LAPIC_NMI => {
                let Some(e) = read_at::<AcpiMadtLapicNmi>(b, addr) else {
                    continue;
                };
                let pin = i32::from(e.local_apic_lint);

                let mut map = MpIntrMap::new();
                map.cpu_id =
                    i32::from(LAPIC_MAP[usize::from(e.acpi_proc_id)].load(Ordering::Relaxed));
                map.ioapic_pin = pin;
                map.flags = i32::from(e.flags);

                if (pin != 0 && pin != 1) || !acpimadt_cfg_intr(map.flags, &mut map.redir) {
                    kprintf!("{}: bogus nmi for apid {}\n", self_.xname(), map.cpu_id);
                    continue;
                }

                map.redir &= !IOAPIC_REDLO_DEL_MASK;
                map.redir |= IOAPIC_REDLO_DEL_NMI << IOAPIC_REDLO_DEL_SHIFT;
                if intrs.len() < nlapic_nmis {
                    intrs.push(map);
                }
            }

            ACPI_MADT_X2APIC | ACPI_MADT_X2APIC_NMI => {}

            _ => {
                if apic_type < ACPI_MADT_OEM_RSVD {
                    kprintf!(
                        "{}: unknown apic structure type {:x}\n",
                        self_.xname(),
                        apic_type
                    );
                }
            }
        }
    }

    mp_set_intrs(Box::leak(intrs.into_boxed_slice()));

    // ISA interrupts are supposed to be identity mapped unless there is an override, in
    // which case we will already have a mapping for the interrupt.
    for pin in (0..).take(ICU_LEN as usize) {
        // Skip if we already have a mapping for this interrupt.
        if ACPIMADT_ISA_BUS.intrs().any(|map| map.bus_pin == pin) {
            continue;
        }

        let Some(apic) = ioapic_find_bybase(pin) else {
            continue;
        };

        let mut map = MpIntrMap::new();
        map.ioapic = Some(apic);
        map.ioapic_pin = pin;
        map.bus_pin = pin;
        map.redir = IOAPIC_REDLO_DEL_LOPRI << IOAPIC_REDLO_DEL_SHIFT;

        map.ioapic_ih = ioapic_ih(apic, pin);

        let map: &'static MpIntrMap = Box::leak(Box::new(map));
        ioapic_set_ip_map(apic, pin, map);

        ACPIMADT_ISA_BUS.push(map);
    }
}

/// `acpimadt_print(aux, pnp)`: names a `cpu` or `ioapic` that found no driver.
pub fn acpimadt_print(aux: *mut c_void, pnp: Option<&[u8]>) -> i32 {
    // SAFETY: the `struct cpu_attach_args` and `struct apic_attach_args` acpimadt hands
    // mainbus both start with their name, which is all this reads.
    let name = unsafe { *aux.cast_const().cast::<&'static [u8]>() };

    if let Some(pnp) = pnp {
        kprintf!("{} at {}:", Str(name), Str(pnp));
    }

    UNCONF
}

#[cfg(test)]
mod tests;
