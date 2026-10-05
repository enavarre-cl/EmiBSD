/*	$OpenBSD: bios.c,v 1.48 2025/09/16 12:18:10 hshoexer Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 2006 Gordon Willem Klok <gklok@cogeco.ca>
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

//! bios0: `arch/amd64/amd64/bios.c`, the firmware's node below mainbus. It reads the
//! SMBIOS tables (the machine's vendor and product) and attaches the firmware's other
//! interfaces: `efi0`, `acpi0` and `mpbios0`.
//!
//! Upstream: sys/arch/amd64/amd64/bios.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - The SMBIOS half (`smbios_find`, `smbios_find_table`, `smbios_get_string`,
//!   `fixstring`, `smbios_info`, `smbios_entry` and the `smbios_*` strings, over
//!   `<machine/smbiosvar.h>`) and the Soekris comBIOS scan that runs without SMBIOS are not
//!   ported: `bios_attach` reports them and prints the bare line, as the C does on a
//!   firmware without SMBIOS (the `hw.vendor`/`hw.product` sysctls stay unset).
//! - `bios_efiinfo` is Limine's (`machdep.rs`, `BIOS_EFIINFO_CONFIG_ACPI`): acpi0 gets the
//!   RSDP the firmware gave, as on an EFI boot. `efi0` (`efi_machdep.c`) and `mpbios0`
//!   (`mpbios.c`) are not ported and are reported where the C would attach them.
//! - `struct bios_attach_args` (`<machine/biosvar.h>`) is [`BiosAttachArgs`] here, not in
//!   `include/biosvar.rs`: that module is shared by path with efiboot (M14), which must
//!   build it without the kernel's bus_space types.

use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::Ordering;

use crate::arch::amd64::amd64::bus_space::{X86_BUS_SPACE_IO, X86_BUS_SPACE_MEM, X86BusSpace};
use crate::arch::amd64::amd64::machdep::BIOS_EFIINFO_CONFIG_ACPI;
use crate::kern::subr_autoconf::config_found;
use crate::kern::subr_prf::Str;
use crate::sys::device::{CD_COCOVM, CfMatch, Cfattach, Cfdriver, DV_DULL, Device, UNCONF};
use crate::{kprintf, unported};

/// `struct bios_attach_args` (`<machine/biosvar.h>`): what bios0 hands `acpi0`, `efi0` and
/// `mpbios0`.
#[repr(C)]
pub struct BiosAttachArgs {
    /// `ba_name`: the driver to match; first, as every amd64 attach argument starts with
    /// the name (`mainbus_print`).
    pub ba_name: &'static [u8],
    /// `ba_func`.
    pub ba_func: u32,
    /// `ba_iot`.
    pub ba_iot: X86BusSpace,
    /// `ba_memt`.
    pub ba_memt: X86BusSpace,
    /// `ba_acpipbase`: the RSDP's physical address, 0 when unknown.
    pub ba_acpipbase: usize,
}

/// `struct bios_softc`: bios0 is its device alone.
pub type BiosSoftc = Device;

/// `bios_ca`.
pub static BIOS_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<BiosSoftc>(),
    ca_match: Some(bios_match),
    ca_attach: bios_attach,
    ca_detach: None,
    ca_activate: None,
};

/// `bios_cd`.
pub static BIOS_CD: Cfdriver = Cfdriver::new(b"bios", DV_DULL, CD_COCOVM);

/// `bios_match(parent, match, aux)`: only one.
pub fn bios_match(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: mainbus hands bios0 its `mba_bios`, a `struct bios_attach_args`.
    let bia = unsafe { &*aux.cast_const().cast::<BiosAttachArgs>() };

    // only one
    if BIOS_CD.cd_ndevs.get() != 0 || bia.ba_name != BIOS_CD.cd_name {
        return 0;
    }
    1
}

/// `bios_attach(parent, self, aux)`: SMBIOS, then the firmware interfaces below bios0.
pub fn bios_attach(_parent: Option<&Device>, self_: &Device, _aux: *mut c_void) {
    // bios_efiinfo->config_smbios, or the ISA hole from SMBIOS_START to SMBIOS_END:
    // smbios_find, the "SMBIOS rev." line, smbios_info (see the deviations; reported after
    // the line ends, so that bios0's line stays whole).

    // out:
    kprintf!("\n");
    let _ = unported!("bios_attach: SMBIOS (smbios_find, smbios_info; smbiosvar.h)");

    // No SMBIOS extensions, go looking for Soekris comBIOS (see the deviations).
    let _ = unported!("bios_attach: the Soekris comBIOS scan (ISA_HOLE_VADDR)");

    // NEFI > 0
    let _ = unported!("efi0 at bios0 (efi_machdep.c)");

    // NACPI > 0
    {
        let mut ba = BiosAttachArgs {
            ba_name: b"acpi",
            ba_func: 0,
            ba_iot: X86_BUS_SPACE_IO,
            ba_memt: X86_BUS_SPACE_MEM,
            ba_acpipbase: BIOS_EFIINFO_CONFIG_ACPI.load(Ordering::Relaxed) as usize,
        };

        let _ = config_found(self_, ptr::from_mut(&mut ba).cast(), Some(bios_print));
    }

    // NMPBIOS > 0: if (mpbios_probe(self)) config_found(self, &ba "mpbios", bios_print).
    let _ = unported!("mpbios_probe (mpbios0 at bios0, mpbios.c)");
}

/// `bios_print(aux, pnp)`: names a child that found no driver.
pub fn bios_print(aux: *mut c_void, pnp: Option<&[u8]>) -> i32 {
    // SAFETY: bios0 hands its children `struct bios_attach_args`.
    let ba = unsafe { &*aux.cast_const().cast::<BiosAttachArgs>() };

    if let Some(pnp) = pnp {
        kprintf!("{} at {}", Str(ba.ba_name), Str(pnp));
    }
    UNCONF
}
