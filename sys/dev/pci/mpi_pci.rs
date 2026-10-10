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
 * Copyright (c) 2005 David Gwynne <dlg@openbsd.org>
 * Copyright (c) 2005 Marco Peereboom <marco@openbsd.org>
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
//! `mpi* at pci?`: the PCI front-end of mpi(4): maps the system interface registers,
//! establishes the interrupt (MSI, else INTx) and attaches the adapter.
//!
//! Upstream: sys/dev/pci/mpi_pci.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - The softc begins with the `struct mpi_softc`, as in C; `psc_pc` is an `Option` (zero is
//!   `None`), set by attach; `psc_tag` is the function's tag; `psc_ih` is the interrupt
//!   handle or null.
//! - The `__sparc64__` Open Firmware `scsi-initiator-id` walk is not ported (no such
//!   machine here).
//! - The interrupt is established with the device's name (`DEVNAME(sc)`), which lives as
//!   long as the softc.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::{self, NonNull};

use crate::dev::ic::mpi::{mpi_attach, mpi_detach, mpi_intr};
use crate::dev::ic::mpivar::{MPI_F_SPI, MpiSoftc};
use crate::dev::pci::pci::pci_matchbyid;
use crate::dev::pci::pci_map::{pci_mapreg_map, pci_mapreg_type};
use crate::dev::pci::pcidevs::{
    PCI_PRODUCT_SYMBIOS_1030, PCI_PRODUCT_SYMBIOS_FC909, PCI_PRODUCT_SYMBIOS_FC909A,
    PCI_PRODUCT_SYMBIOS_FC919, PCI_PRODUCT_SYMBIOS_FC919_1, PCI_PRODUCT_SYMBIOS_FC919X,
    PCI_PRODUCT_SYMBIOS_FC929, PCI_PRODUCT_SYMBIOS_FC929_1, PCI_PRODUCT_SYMBIOS_FC929X,
    PCI_PRODUCT_SYMBIOS_FC939X, PCI_PRODUCT_SYMBIOS_FC949E, PCI_PRODUCT_SYMBIOS_FC949X,
    PCI_PRODUCT_SYMBIOS_SAS1064, PCI_PRODUCT_SYMBIOS_SAS1064A, PCI_PRODUCT_SYMBIOS_SAS1064E,
    PCI_PRODUCT_SYMBIOS_SAS1064E_2, PCI_PRODUCT_SYMBIOS_SAS1066, PCI_PRODUCT_SYMBIOS_SAS1066E,
    PCI_PRODUCT_SYMBIOS_SAS1068, PCI_PRODUCT_SYMBIOS_SAS1068_2, PCI_PRODUCT_SYMBIOS_SAS1068E,
    PCI_PRODUCT_SYMBIOS_SAS1068E_2, PCI_VENDOR_SYMBIOS,
};
use crate::dev::pci::pcireg::{
    PCI_ID_REG, PCI_MAPREG_END, PCI_MAPREG_START, PCI_MAPREG_TYPE_MASK, PCI_MAPREG_TYPE_MEM,
    PCI_ROM_ENABLE, PCI_ROM_REG, pci_id_code,
};
use crate::dev::pci::pcivar::{PciAttachArgs, PciMatchid};
use crate::kern::subr_prf::printf;
use crate::machine::bus::bus_space_unmap;
use crate::machine::intr::{IPL_BIO, IPL_MPSAFE};
use crate::machine::pci_machdep::{
    PciChipsetTag, Pcitag, pci_conf_read, pci_conf_write, pci_intr_disestablish,
    pci_intr_establish, pci_intr_map, pci_intr_map_msi, pci_intr_string,
};
use crate::sys::device::{CfMatch, Cfattach, Device, Softc};
use crate::sys::errno::Errno;

/// `struct mpi_pci_softc`.
#[repr(C)]
pub struct MpiPciSoftc {
    /// `psc_mpi`.
    pub psc_mpi: MpiSoftc,

    /// `psc_pc`.
    pub psc_pc: Cell<Option<PciChipsetTag>>,
    /// `psc_tag`.
    pub psc_tag: Cell<Pcitag>,

    /// `psc_ih`: the interrupt handle, or null.
    pub psc_ih: Cell<*mut c_void>,
}

// SAFETY: `#[repr(C)]` with the mpi softc (itself headed by the device, all-zero valid)
// first, then `Cell`s of an `Option` of the chipset tag, the PCI tag (an integer) and a raw
// pointer, valid as zero bits.
unsafe impl Softc for MpiPciSoftc {}

/// `mpi_pci_ca`.
pub static MPI_PCI_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<MpiPciSoftc>(),
    ca_match: Some(mpi_pci_match),
    ca_attach: mpi_pci_attach,
    ca_detach: Some(mpi_pci_detach),
    ca_activate: None,
};

const fn mpi_device(pid: u32) -> PciMatchid {
    PciMatchid {
        pm_vid: PCI_VENDOR_SYMBIOS as _,
        pm_pid: pid as _,
    }
}

/// `mpi_devices`.
static MPI_DEVICES: [PciMatchid; 22] = [
    mpi_device(PCI_PRODUCT_SYMBIOS_1030),
    mpi_device(PCI_PRODUCT_SYMBIOS_FC909),
    mpi_device(PCI_PRODUCT_SYMBIOS_FC909A),
    mpi_device(PCI_PRODUCT_SYMBIOS_FC919),
    mpi_device(PCI_PRODUCT_SYMBIOS_FC919_1),
    mpi_device(PCI_PRODUCT_SYMBIOS_FC919X),
    mpi_device(PCI_PRODUCT_SYMBIOS_FC929),
    mpi_device(PCI_PRODUCT_SYMBIOS_FC929_1),
    mpi_device(PCI_PRODUCT_SYMBIOS_FC929X),
    mpi_device(PCI_PRODUCT_SYMBIOS_FC939X),
    mpi_device(PCI_PRODUCT_SYMBIOS_FC949E),
    mpi_device(PCI_PRODUCT_SYMBIOS_FC949X),
    mpi_device(PCI_PRODUCT_SYMBIOS_SAS1064),
    mpi_device(PCI_PRODUCT_SYMBIOS_SAS1064A),
    mpi_device(PCI_PRODUCT_SYMBIOS_SAS1064E_2),
    mpi_device(PCI_PRODUCT_SYMBIOS_SAS1064E),
    mpi_device(PCI_PRODUCT_SYMBIOS_SAS1066),
    mpi_device(PCI_PRODUCT_SYMBIOS_SAS1066E),
    mpi_device(PCI_PRODUCT_SYMBIOS_SAS1068),
    mpi_device(PCI_PRODUCT_SYMBIOS_SAS1068_2),
    mpi_device(PCI_PRODUCT_SYMBIOS_SAS1068E),
    mpi_device(PCI_PRODUCT_SYMBIOS_SAS1068E_2),
];

/// `mpi_pci_match`.
pub fn mpi_pci_match(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: pci attaches its children with a `pci_attach_args`.
    let pa = unsafe { &*aux.cast::<PciAttachArgs>() };

    pci_matchbyid(pa, &MPI_DEVICES)
}

/// `mpi_pci_attach`.
pub fn mpi_pci_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: `self_` was made for `mpi_pci_ca`, whose softc is an `MpiPciSoftc`; softcs are
    // never freed while the device exists, so the softc may be borrowed for 'static.
    let psc: &'static MpiPciSoftc = unsafe { &*ptr::from_ref(self_.softc::<MpiPciSoftc>()) };
    let sc = &psc.psc_mpi;
    // SAFETY: pci attaches its children with a `pci_attach_args` it owns for the duration of
    // the attach.
    let pa = unsafe { &*aux.cast::<PciAttachArgs>() };

    psc.psc_pc.set(Some(pa.pa_pc));
    psc.psc_tag.set(pa.pa_tag);
    psc.psc_ih.set(ptr::null_mut());
    sc.sc_dmat.set(Some(pa.pa_dmat));
    sc.sc_ios.set(0);
    sc.sc_target.set(-1);

    // find the appropriate memory base
    let mut r = PCI_MAPREG_START;
    let mut memtype = 0;
    while r < PCI_MAPREG_END {
        memtype = pci_mapreg_type(pa.pa_pc, pa.pa_tag, r);
        if memtype & PCI_MAPREG_TYPE_MASK == PCI_MAPREG_TYPE_MEM {
            break;
        }
        r += size_of::<u32>() as i32;
    }
    if r >= PCI_MAPREG_END {
        printf(format_args!(
            ": unable to locate system interface registers\n"
        ));
        return;
    }

    let Ok((iot, ioh, _base, ios)) = pci_mapreg_map(pa, r, memtype, 0, 0) else {
        printf(format_args!(": unable to map system interface registers\n"));
        return;
    };
    sc.sc_iot.set(Some(iot));
    sc.sc_ioh.set(Some(ioh));
    sc.sc_ios.set(ios);

    // disable the expansion rom
    pci_conf_write(
        pa.pa_pc,
        pa.pa_tag,
        PCI_ROM_REG,
        pci_conf_read(pa.pa_pc, pa.pa_tag, PCI_ROM_REG) & !PCI_ROM_ENABLE,
    );

    let unmap = 'unmap: {
        // hook up the interrupt
        let Some(ih) = pci_intr_map_msi(pa).or_else(|| pci_intr_map(pa)) else {
            printf(format_args!(": unable to map interrupt\n"));
            break 'unmap true;
        };
        let intrstr = pci_intr_string(pa.pa_pc, ih);
        let Some(cookie) = pci_intr_establish(
            pa.pa_pc,
            ih,
            IPL_BIO | IPL_MPSAFE,
            mpi_intr,
            ptr::from_ref(sc).cast_mut().cast(),
            sc.sc_dev.xname(),
        ) else {
            printf(format_args!(": unable to map interrupt at {}\n", intrstr));
            break 'unmap true;
        };
        psc.psc_ih.set(cookie.as_ptr());
        printf(format_args!(": {}", intrstr));

        if pci_conf_read(pa.pa_pc, pa.pa_tag, PCI_ID_REG)
            == pci_id_code(PCI_VENDOR_SYMBIOS, PCI_PRODUCT_SYMBIOS_1030)
        {
            sc.sc_flags.set(sc.sc_flags.get() | MPI_F_SPI);
        }

        if mpi_attach(sc).is_err() {
            // error printed by mpi_attach
            // deintr:
            if let Some(ih) = NonNull::new(psc.psc_ih.replace(ptr::null_mut())) {
                // SAFETY: the handle established above, dropped here.
                unsafe { pci_intr_disestablish(pa.pa_pc, ih) };
            }
            break 'unmap true;
        }

        false
    };

    if unmap {
        bus_space_unmap(iot, ioh, sc.sc_ios.get());
        sc.sc_ios.set(0);
    }
}

/// `mpi_pci_detach`.
pub fn mpi_pci_detach(self_: &Device, _flags: i32) -> Result<(), Errno> {
    // SAFETY: `self_` was made for `mpi_pci_ca`; softcs are never freed while the device
    // exists.
    let psc: &'static MpiPciSoftc = unsafe { &*ptr::from_ref(self_.softc::<MpiPciSoftc>()) };
    let sc = &psc.psc_mpi;

    mpi_detach(sc);

    if let Some(ih) = NonNull::new(psc.psc_ih.replace(ptr::null_mut()))
        && let Some(pc) = psc.psc_pc.get()
    {
        // SAFETY: the handle established by mpi_pci_attach, dropped here.
        unsafe { pci_intr_disestablish(pc, ih) };
    }
    if sc.sc_ios.get() != 0
        && let (Some(iot), Some(ioh)) = (sc.sc_iot.get(), sc.sc_ioh.get())
    {
        bus_space_unmap(iot, ioh, sc.sc_ios.get());
        sc.sc_ios.set(0);
    }

    Ok(())
}
/* </CODE> */
