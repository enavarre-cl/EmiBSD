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
 * Copyright (c) 2018 Patrick Wildt <patrick@blueri.se>
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
//! ipmi(4) at acpi: `dev/acpi/ipmi_acpi.c`. A device whose `_HID` is `IPI0001` is a BMC's
//! system interface: `_IFT` gives the interface type, `_SRV` the IPMI revision (BCD, in its
//! high byte) and `_CRS` the registers: the first I/O port or fixed 32-bit memory
//! descriptor is the base, a second one of the same kind sets the register spacing. The
//! interrupts are ignored (the driver polls). The rest is `ipmi_attach_common`.
//!
//! Upstream: sys/dev/acpi/ipmi_acpi.c @ 3ce1f3f79392
//!
//! amd64's GENERIC has `ipmi0 at acpi? disable` (enabled with `boot -c`), arm64's `ipmi* at
//! acpi?`. QEMU's `isa-ipmi-kcs` (amd64 `q35`) puts an `IPI0001` device in its DSDT, whose
//! `_CRS` is `IO(Decode16, 0xca2, 0xca3, 1, 2)`: a range whose `_MAX` is the last port, where
//! a fixed device's `_CRS` has `_MAX` equal to `_MIN`. The C takes `_MAX`, so there ipmi0
//! maps 0xca3 and every command fails ("sendcmd fails", "no SDRs IPMI disabled"); this port
//! does the same, as OpenBSD 8.0 does on the same machine (`smoke-ipmi`).
//!
//! ## Deviations
//! - The softc's own members are `Cell`s (autoconfiguration hands out zeroed softcs);
//!   `sc_iotype` is a byte (`b'i'`, `b'm'`, 0 for none).
//! - `_IFT` and `_SRV` are truncated to `int` as the C's assignments do.

use core::cell::{Cell, RefCell};
use core::ffi::c_void;
use core::ptr;

use crate::dev::acpi::acpi::acpi_matchhids;
use crate::dev::acpi::acpireg::ACPI_DEV_IPMI;
use crate::dev::acpi::acpivar::{AcpiAttachArgs, AcpiSoftc};
use crate::dev::acpi::amltypes::{AML_OBJTYPE_BUFFER, AmlNodeRef, AmlValue};
use crate::dev::acpi::dsdt::{
    AcpiResource, LR_EXTIRQ, LR_MEM32FIXED, SR_IOPORT, SR_IRQ, aml_crstype, aml_evalinteger,
    aml_evalname, aml_freevalue, aml_parse_resource,
};
use crate::dev::ipmi::{ipmi_activate, ipmi_attach_common};
use crate::dev::ipmivar::{IpmiAttachArgs, IpmiSoftc};
use crate::kprintf;
use crate::machine::bus::BusSize;
use crate::sys::device::{CfMatch, Cfattach, Device, Softc};

/// `struct ipmi_acpi_softc`.
#[repr(C)]
pub struct IpmiAcpiSoftc {
    /// `sc`: the generic softc, first.
    pub sc: IpmiSoftc,

    /// `sc_acpi`: acpi0.
    pub sc_acpi: Cell<*const AcpiSoftc>,
    /// `sc_devnode`.
    pub sc_devnode: RefCell<Option<AmlNodeRef>>,

    /// `sc_ift`: `_IFT`, the interface type.
    pub sc_ift: Cell<i32>,
    /// `sc_srv`: `_SRV`, the IPMI revision.
    pub sc_srv: Cell<i32>,

    /// `sc_iobase`: the first register.
    pub sc_iobase: Cell<BusSize>,
    /// `sc_iospacing`: the distance between two registers.
    pub sc_iospacing: Cell<i32>,
    /// `sc_iotype`: `b'i'` (I/O ports), `b'm'` (memory) or 0 (no usable resources).
    pub sc_iotype: Cell<u8>,
}

// SAFETY: `#[repr(C)]` with the device first (inside `sc`, a `Softc` itself); the other
// members are valid as zero bits: a null pointer, an unborrowed `RefCell` of `None`, zeros.
unsafe impl Softc for IpmiAcpiSoftc {}

/// `ipmi_acpi_ca`.
pub static IPMI_ACPI_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<IpmiAcpiSoftc>(),
    ca_match: Some(ipmi_acpi_match),
    ca_attach: ipmi_acpi_attach,
    ca_detach: None,
    ca_activate: Some(ipmi_activate),
};

/// `ipmi_acpi_hids[]`.
pub const IPMI_ACPI_HIDS: [&str; 1] = [ACPI_DEV_IPMI];

/// `DEVNAME(s)`: `s->sc.sc_dev.dv_xname`.
fn devname(sc: &IpmiAcpiSoftc) -> &str {
    sc.sc.sc_dev.xname()
}

/// `ipmi_acpi_match(parent, match, aux)`: an `IPI0001` device.
pub fn ipmi_acpi_match(_parent: Option<&Device>, match_: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: acpi0 hands its children `struct acpi_attach_args`.
    let aa = unsafe { &*aux.cast_const().cast::<AcpiAttachArgs>() };
    let cf = match_.cfdata();

    // sanity
    acpi_matchhids(
        aa,
        &IPMI_ACPI_HIDS,
        core::str::from_utf8(cf.cf_driver.cd_name).unwrap_or(""),
    )
}

/// `ipmi_acpi_attach(parent, self, aux)`: the interface from `_IFT`, `_SRV` and `_CRS`,
/// then `ipmi_attach_common`.
pub fn ipmi_acpi_attach(parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: `ipmi_acpi_ca` makes `IpmiAcpiSoftc`s, never detached: the softc lives as long
    // as the kernel.
    let sc: &'static IpmiAcpiSoftc = unsafe { &*ptr::from_ref(self_.softc::<IpmiAcpiSoftc>()) };
    // SAFETY: as in `ipmi_acpi_match`.
    let aa = unsafe { &*aux.cast_const().cast::<AcpiAttachArgs>() };
    let mut ift = 0i64;
    let mut srv = 0i64;

    let Some(parent) = parent else {
        return;
    };
    // SAFETY: ipmi attaches at acpi0, whose softc is an `AcpiSoftc`, never freed.
    let acpi: &'static AcpiSoftc = unsafe { &*ptr::from_ref(parent.softc::<AcpiSoftc>()) };
    sc.sc_acpi.set(ptr::from_ref(acpi));
    *sc.sc_devnode.borrow_mut() = aa.aaa_node.clone();
    let node = sc.sc_devnode.borrow().clone();

    let rc = aml_evalinteger(Some(acpi), node.as_ref(), b"_IFT", &[], &mut ift);
    if rc != 0 {
        kprintf!(": no _IFT\n");
        return;
    }
    sc.sc_ift.set(ift as i32);

    aml_evalinteger(Some(acpi), node.as_ref(), b"_SRV", &[], &mut srv);
    sc.sc_srv.set(srv as i32);

    let res = AmlValue::new();
    if aml_evalname(Some(acpi), node.as_ref(), b"_CRS", &[], Some(&res)) != 0 {
        kprintf!(": no _CRS method\n");
        return;
    }
    if res.r#type() != AML_OBJTYPE_BUFFER {
        kprintf!(
            ": invalid _CRS object (type {} len {})\n",
            res.r#type(),
            res.length()
        );
        aml_freevalue(Some(&res));
        return;
    }

    aml_parse_resource(&res, &mut |i, crs| ipmi_acpi_parse_crs(i, crs, sc));
    aml_freevalue(Some(&res));

    if sc.sc_iotype.get() == 0 {
        kprintf!(
            "{}: incomplete resources (ift {})\n",
            devname(sc),
            sc.sc_ift.get()
        );
        return;
    }

    let ia = IpmiAttachArgs {
        iaa_iot: acpi.sc_iot.get(),
        iaa_memt: acpi.sc_memt.get(),
        iaa_if_type: sc.sc_ift.get(),
        iaa_if_rev: sc.sc_srv.get() >> 4,
        iaa_if_irq: -1,
        iaa_if_irqlvl: 0,
        iaa_if_iosize: 1,
        iaa_if_iospacing: sc.sc_iospacing.get(),
        iaa_if_iobase: sc.sc_iobase.get(),
        iaa_if_iotype: sc.sc_iotype.get(),
        ..IpmiAttachArgs::zeroed()
    };

    ipmi_attach_common(&sc.sc, &ia);
}

/// `ipmi_acpi_parse_crs(crsidx, crs, arg)`: the base from the first I/O port or fixed
/// memory descriptor, the spacing from a second one; interrupts are skipped. -1 (and no
/// usable resources) on anything else.
pub fn ipmi_acpi_parse_crs(crsidx: i32, crs: &AcpiResource<'_>, sc: &IpmiAcpiSoftc) -> i32 {
    let r#type = aml_crstype(crs);

    let (addr, iotype): (BusSize, u8) = match r#type {
        // Ignore for now.
        SR_IRQ => return 0,
        SR_IOPORT => (BusSize::from(crs.sr_ioport__max()), b'i'),
        LR_MEM32FIXED => (crs.lr_m32fixed__bas() as BusSize, b'm'),
        // Ignore for now.
        LR_EXTIRQ => return 0,
        _ => {
            kprintf!(
                "\n{}: unexpected resource #{} type {}",
                devname(sc),
                crsidx,
                r#type
            );
            sc.sc_iotype.set(0);
            return -1;
        }
    };

    match crsidx {
        0 => {
            sc.sc_iobase.set(addr);
            sc.sc_iospacing.set(1);
            sc.sc_iotype.set(iotype);
        }
        1 => {
            if sc.sc_iotype.get() != iotype {
                kprintf!(
                    "\n{}: unexpected resource #{} type {}\n",
                    devname(sc),
                    crsidx,
                    r#type
                );
                sc.sc_iotype.set(0);
                return -1;
            }
            if addr <= sc.sc_iobase.get() {
                sc.sc_iotype.set(0);
                return -1;
            }
            sc.sc_iospacing.set((addr - sc.sc_iobase.get()) as i32);
        }
        _ => {
            kprintf!(
                "\n{}: invalid resource #{} type {} (ift {})",
                devname(sc),
                crsidx,
                r#type,
                sc.sc_ift.get()
            );
            sc.sc_iotype.set(0);
            return -1;
        }
    }

    0
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;
    use std::boxed::Box;
    use std::vec::Vec;

    fn softc() -> &'static IpmiAcpiSoftc {
        // SAFETY: all-zero is a valid `IpmiAcpiSoftc` (its `Softc` contract).
        Box::leak(Box::new(unsafe { core::mem::zeroed::<IpmiAcpiSoftc>() }))
    }

    /// `IO(Decode16, min, max, aln, len)`.
    fn io(min: u16, max: u16, len: u8) -> Vec<u8> {
        let mut d = std::vec![0x47, 1];
        d.extend_from_slice(&min.to_le_bytes());
        d.extend_from_slice(&max.to_le_bytes());
        d.extend_from_slice(&[1, len]);
        d
    }

    /// `Memory32Fixed(ReadWrite, bas, len)`.
    fn mem32(bas: u32, len: u32) -> Vec<u8> {
        let mut d = std::vec![0x86, 9, 0, 1];
        d.extend_from_slice(&bas.to_le_bytes());
        d.extend_from_slice(&len.to_le_bytes());
        d
    }

    /// `IRQNoFlags() {n}`.
    fn irq(n: u8) -> Vec<u8> {
        let m = 1u16 << n;
        let mut d = std::vec![0x22];
        d.extend_from_slice(&m.to_le_bytes());
        d
    }

    fn parse(descs: &[Vec<u8>]) -> (&'static IpmiAcpiSoftc, Vec<i32>) {
        let sc = softc();
        let rcs = descs
            .iter()
            .enumerate()
            .map(|(i, d)| ipmi_acpi_parse_crs(i as i32, &AcpiResource::new(d), sc))
            .collect();
        (sc, rcs)
    }

    #[test]
    fn one_io_descriptor_gives_the_base() {
        // QEMU's isa-ipmi-kcs: a range of two ports; the C takes its maximum (module docs).
        let (sc, rcs) = parse(&[io(0xca2, 0xca3, 2)]);
        assert_eq!(rcs, [0]);
        assert_eq!((sc.sc_iobase.get(), sc.sc_iospacing.get()), (0xca3, 1));

        let (sc, rcs) = parse(&[io(0xca2, 0xca2, 2)]);
        assert_eq!(rcs, [0]);
        assert_eq!(
            (
                sc.sc_iotype.get(),
                sc.sc_iobase.get(),
                sc.sc_iospacing.get()
            ),
            (b'i', 0xca2, 1)
        );
    }

    #[test]
    fn two_registers_set_the_spacing() {
        let (sc, rcs) = parse(&[io(0xca2, 0xca2, 1), io(0xca6, 0xca6, 1)]);
        assert_eq!(rcs, [0, 0]);
        assert_eq!((sc.sc_iobase.get(), sc.sc_iospacing.get()), (0xca2, 4));

        let (sc, rcs) = parse(&[mem32(0xfe00_0000, 1), mem32(0xfe00_0004, 1)]);
        assert_eq!(rcs, [0, 0]);
        assert_eq!(
            (
                sc.sc_iotype.get(),
                sc.sc_iobase.get(),
                sc.sc_iospacing.get()
            ),
            (b'm', 0xfe00_0000, 4)
        );
    }

    #[test]
    fn interrupts_are_skipped_and_mismatches_refused() {
        let (sc, rcs) = parse(&[irq(10)]);
        assert_eq!(rcs, [0]);
        assert_eq!(sc.sc_iotype.get(), 0);

        // An I/O port then memory, a second register below the first, a third one.
        let (sc, rcs) = parse(&[io(0xca2, 0xca2, 1), mem32(0xfe00_0000, 1)]);
        assert_eq!((rcs[1], sc.sc_iotype.get()), (-1, 0));
        let (sc, rcs) = parse(&[io(0xca3, 0xca3, 1), io(0xca2, 0xca2, 1)]);
        assert_eq!((rcs[1], sc.sc_iotype.get()), (-1, 0));
        let (sc, rcs) = parse(&[io(1, 1, 1), io(2, 2, 1), io(3, 3, 1)]);
        assert_eq!((rcs[2], sc.sc_iotype.get()), (-1, 0));
    }
}
/* </TESTS> */
