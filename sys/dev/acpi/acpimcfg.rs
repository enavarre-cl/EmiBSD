/* $OpenBSD: acpimcfg.c,v 1.6 2025/09/16 12:18:10 hshoexer Exp $ */
/* <LICENSES> */
/*
 * Copyright (c) 2010 Mark Kettenis <kettenis@openbsd.org>
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
//! acpimcfg(4): the MCFG table, the PCI Express configuration space windows (ECAM) of the
//! PCI segments: `dev/acpi/acpimcfg.c`. Each entry goes to the machine's `pci_mcfg_init`.
//!
//! Upstream: sys/dev/acpi/acpimcfg.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - The entries are read from the table's bytes (unaligned reads, within `hdr.length`;
//!   the C walks `struct acpi_mcfg_entry` pointers up to the same end).

use core::ffi::c_void;
use core::ptr;

use crate::dev::acpi::acpireg::{AcpiMcfg, AcpiMcfgEntry, AcpiTableHeader, MCFG_SIG};
use crate::dev::acpi::acpivar::AcpiAttachArgs;
use crate::kern::subr_prf::printf;
use crate::machine::bus::BusAddr;
use crate::machine::pci_machdep::pci_mcfg_init;
use crate::sys::device::{CD_COCOVM, CfMatch, Cfattach, Cfdriver, DV_DULL, Device};

/// `acpimcfg_ca`.
pub static ACPIMCFG_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<Device>(),
    ca_match: Some(acpimcfg_match),
    ca_attach: acpimcfg_attach,
    ca_detach: None,
    ca_activate: None,
};

/// `acpimcfg_cd`.
pub static ACPIMCFG_CD: Cfdriver = Cfdriver::new(b"acpimcfg", DV_DULL, CD_COCOVM);

/// `acpimcfg_match(parent, match, aux)`: the MCFG table.
pub fn acpimcfg_match(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: acpi0 hands its children `struct acpi_attach_args`.
    let aaa = unsafe { &*aux.cast_const().cast::<AcpiAttachArgs>() };

    // If we do not have a table, it is not us
    if aaa.aaa_table.is_null() {
        return 0;
    }

    // If it is an MCFG table, we can attach
    // SAFETY: a table argument points at a table acpi0 loaded, at least a header long.
    let hdr = unsafe { ptr::read_unaligned(aaa.aaa_table.cast_const().cast::<AcpiTableHeader>()) };
    i32::from(&hdr.signature == MCFG_SIG)
}

/// `acpimcfg_attach(parent, self, aux)`: prints and registers each window.
pub fn acpimcfg_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: as in `acpimcfg_match`.
    let aaa = unsafe { &*aux.cast_const().cast::<AcpiAttachArgs>() };
    let base = aaa.aaa_table.cast_const().cast::<u8>();
    // SAFETY: the MCFG acpi0 loaded (`acpimcfg_match`), `hdr.length` bytes, never freed.
    let len = unsafe { ptr::read_unaligned(base.cast::<AcpiTableHeader>()) }.length as usize;
    // SAFETY: as above.
    let table = unsafe { core::slice::from_raw_parts(base, len) };

    printf(format_args!("\n"));

    let Some(memt) = aaa.aaa_memt else {
        return;
    };

    let mut addr = size_of::<AcpiMcfg>();
    while addr + size_of::<AcpiMcfgEntry>() <= table.len() {
        // SAFETY: a whole entry lies within the table (checked above); it is packed integers.
        let entry = unsafe { ptr::read_unaligned(table[addr..].as_ptr().cast::<AcpiMcfgEntry>()) };

        printf(format_args!(
            "{}: addr 0x{:x}, bus {}-{}\n",
            self_.xname(),
            { entry.base_address },
            entry.min_bus_number,
            entry.max_bus_number
        ));

        pci_mcfg_init(
            memt,
            entry.base_address as BusAddr,
            i32::from(entry.segment),
            i32::from(entry.min_bus_number),
            i32::from(entry.max_bus_number),
        );
        addr += size_of::<AcpiMcfgEntry>();
    }
}
/* </CODE> */
