/* <LICENSES> */
/*
 * Copyright (c) 2026 Emilio Navarrete Lineros <enavarre@outlook.com>
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

/*
 * Copyright (c) 2018 Mark Kettenis
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

/* <CODE> */
//! The PL011 on ACPI (`ARMH0011`, the SBSA UART of arm64 servers and QEMU `virt` with
//! `acpi=on`): `dev/acpi/pluart_acpi.c`. It maps the registers `_CRS` gives, establishes the
//! interrupt through `acpi_intr_establish` and attaches the generic `pluart(4)`, as the
//! console when the SPCR table names its address.
//!
//! Upstream: sys/dev/acpi/pluart_acpi.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `struct pluart_acpi_softc` keeps `sc_acpi`, `sc_node` and `sc_ih` in `Cell`s
//!   (autoconfiguration hands out zeroed softcs).

use core::cell::{Cell, RefCell};
use core::ffi::c_void;
use core::ptr;

use crate::dev::acpi::acpi::{acpi_matchhids, q_table_bytes};
use crate::dev::acpi::acpireg::{AcpiSpcr, GAS_SYSTEM_MEMORY, SPCR_SIG};
use crate::dev::acpi::acpivar::{AcpiAttachArgs, AcpiSoftc};
use crate::dev::acpi::amltypes::AmlNodeRef;
use crate::dev::acpi::dsdt::cstr;
use crate::dev::ic::pluart::{COM_HW_SBSA, PluartSoftc, pluart_attach_common, pluart_intr};
use crate::kern::subr_prf::{Str, printf};
use crate::machine::acpi_machdep::acpi_intr_establish;
use crate::machine::bus::{BusAddr, bus_space_map};
use crate::machine::intr::IPL_TTY;
use crate::sys::device::{CfMatch, Cfattach, Device, Softc};

/// `struct pluart_acpi_softc`.
#[repr(C)]
pub struct PluartAcpiSoftc {
    /// `sc`: the generic PL011 softc, first.
    pub sc: PluartSoftc,
    /// `sc_acpi`: acpi0.
    pub sc_acpi: Cell<*const AcpiSoftc>,
    /// `sc_node`.
    pub sc_node: RefCell<Option<AmlNodeRef>>,
    /// `sc_addr`: the registers' address, compared with the SPCR's.
    pub sc_addr: Cell<BusAddr>,
    /// `sc_ih`.
    pub sc_ih: Cell<*mut c_void>,
}

// SAFETY: `#[repr(C)]` with the device first (inside `sc`, a `Softc` itself); the other
// members are valid as zero bits: null pointers, an unborrowed `RefCell` of `None`, 0.
unsafe impl Softc for PluartAcpiSoftc {}

/// `pluart_acpi_ca`.
pub static PLUART_ACPI_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<PluartAcpiSoftc>(),
    ca_match: Some(pluart_acpi_match),
    ca_attach: pluart_acpi_attach,
    ca_detach: None,
    ca_activate: None,
};

/// `pluart_hids[]`.
pub const PLUART_HIDS: [&str; 1] = ["ARMH0011"];

/// `pluart_acpi_match(parent, match, aux)`: an `ARMH0011` with registers and an interrupt.
pub fn pluart_acpi_match(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: acpi0 hands its children `struct acpi_attach_args`.
    let aaa = unsafe { &*aux.cast_const().cast::<AcpiAttachArgs>() };

    if aaa.aaa_naddr < 1 || aaa.aaa_nirq < 1 {
        return 0;
    }
    acpi_matchhids(aaa, &PLUART_HIDS, "pluart")
}

/// `pluart_acpi_attach(parent, self, aux)`.
pub fn pluart_acpi_attach(parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: `pluart_acpi_ca` makes `PluartAcpiSoftc`s, never detached: the softc lives as
    // long as the kernel.
    let sc: &'static PluartAcpiSoftc = unsafe { &*ptr::from_ref(self_.softc::<PluartAcpiSoftc>()) };
    // SAFETY: as in `pluart_acpi_match`.
    let aaa = unsafe { &*aux.cast_const().cast::<AcpiAttachArgs>() };

    if let Some(parent) = parent {
        // SAFETY: pluart attaches at acpi0, whose softc is an `AcpiSoftc`.
        sc.sc_acpi
            .set(ptr::from_ref(unsafe { parent.softc::<AcpiSoftc>() }));
    }
    *sc.sc_node.borrow_mut() = aaa.aaa_node.clone();
    if let Some(node) = aaa.aaa_node.as_ref() {
        printf(format_args!(" {}", Str(cstr(&node.name))));
    }

    printf(format_args!(
        " addr 0x{:x}/0x{:x}",
        aaa.aaa_addr[0], aaa.aaa_size[0]
    ));
    printf(format_args!(" irq {}", aaa.aaa_irq[0]));

    let Some(iot) = aaa.aaa_bst[0] else {
        printf(format_args!(": can't map registers\n"));
        return;
    };
    sc.sc.sc_iot.set(Some(iot));
    sc.sc_addr.set(aaa.aaa_addr[0] as BusAddr);
    // SAFETY: the device's register window from its `_CRS`, which only this driver drives.
    match unsafe { bus_space_map(iot, aaa.aaa_addr[0] as BusAddr, aaa.aaa_size[0] as usize, 0) } {
        Ok(ioh) => sc.sc.sc_ioh.set(Some(ioh)),
        Err(_) => {
            printf(format_args!(": can't map registers\n"));
            return;
        }
    }

    // SAFETY: the softc is the device, alive while attached (see above).
    let name: &'static str = unsafe { &*ptr::from_ref(sc.sc.sc_dev.xname()) };
    let Some(ih) = acpi_intr_establish(
        aaa.aaa_irq[0] as i32,
        aaa.aaa_irq_flags[0] as i32,
        IPL_TTY,
        pluart_intr,
        ptr::from_ref(sc).cast_mut().cast::<c_void>(),
        name,
    ) else {
        printf(format_args!(": can't establish interrupt\n"));
        return;
    };
    sc.sc_ih.set(ih.as_ptr());
    sc.sc.sc_irq.set(ih.as_ptr());

    sc.sc.sc_hwflags.set(sc.sc.sc_hwflags.get() | COM_HW_SBSA);

    pluart_attach_common(&sc.sc, pluart_acpi_is_console(sc));
}

/// `pluart_acpi_is_console(sc)`: whether the SPCR names this UART's registers.
pub fn pluart_acpi_is_console(sc: &PluartAcpiSoftc) -> bool {
    // SAFETY: set by the attach to acpi0's softc, never freed.
    let Some(acpi) = (unsafe { sc.sc_acpi.get().as_ref() }) else {
        return false;
    };

    for entry in acpi.sc_tables.iter() {
        let table = q_table_bytes(entry);
        if !table.starts_with(SPCR_SIG) || table.len() < size_of::<AcpiSpcr>() {
            continue;
        }
        // SAFETY: the table holds a whole SPCR (checked above); it is packed integers.
        let spcr = unsafe { ptr::read_unaligned(table.as_ptr().cast::<AcpiSpcr>()) };
        let base = spcr.base_address;
        if i32::from(base.address_space_id) == GAS_SYSTEM_MEMORY && { base.address }
            == sc.sc_addr.get() as u64
        {
            return true;
        }
    }

    false
}
/* </CODE> */
