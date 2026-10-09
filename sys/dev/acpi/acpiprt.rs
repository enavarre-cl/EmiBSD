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

/* <CODE> */
//! acpiprt(4): ACPI PCI interrupt routing, `dev/acpi/acpiprt.c`.
//!
//! Upstream: sys/dev/acpi/acpiprt.c @ 3ce1f3f79392
//!
//! acpi0 attaches one `acpiprt` per `_PRT` (PCI Routing Table) of a present PCI bus. Each
//! entry names a device slot and interrupt pin (INTA# to INTD#) and either a global system
//! interrupt or a link device whose `_CRS` says which interrupt it is set to (and whose
//! `_PRS`/`_SRS` can change it). With I/O APICs every entry becomes an `mp_intr_map` on the
//! bus's `mp_busses` list, which `pci_intr_map` looks up; without them the interrupt line
//! register of the matching functions is rewritten. `acpiprt_route_interrupt` programs a
//! link device (`_SRS`) when a driver establishes the interrupt.
//!
//! The I/O APIC half is x86 code; here it reaches the I/O APICs and `mp_busses` through
//! `machine::mpconfig`, whose machines without an I/O APIC never configure the driver.
//!
//! ## Deviations
//! - `acpiprt_map_list` (`SIMPLEQ`) is a `Vec` in an `AmlGlobal` (kernel lock), as acpi.c's
//!   lists are; its entries keep the link device's node (`Rc`) and the softc.
//! - `struct acpiprt_irq` starts zeroed: the C leaves it uninitialised when a `_CRS` holds
//!   no interrupt descriptor.
//! - A `_PRT` entry whose name resolves to a node without a value, a bus number past
//!   `mp_nbusses` or an I/O APIC pin past the I/O APIC's are skipped where the C would
//!   dereference NULL or write past the array.
//! - `_SRS` gets a new buffer holding the edited `_CRS` bytes (the C edits `res` in place
//!   and passes it).
//! - `ACPI_DEBUG`'s mapping line is not configured.

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::cell::{Cell, RefCell};
use core::ffi::c_void;
use core::ptr;

use crate::dev::acpi::acpi::acpi_getsta;
use crate::dev::acpi::acpidev::{STA_ENABLED, STA_PRESENT};
use crate::dev::acpi::acpireg::acpi_pci_dev;
use crate::dev::acpi::acpivar::{AcpiAttachArgs, AcpiSoftc};
use crate::dev::acpi::amltypes::{
    AML_OBJTYPE_BUFFER, AML_OBJTYPE_DEVICE, AML_OBJTYPE_NAMEREF, AML_OBJTYPE_OBJREF,
    AML_OBJTYPE_PACKAGE, AmlNodeRef, AmlValue, AmlValueRef,
};
use crate::dev::acpi::dsdt::{
    AcpiResource, AmlGlobal, LR_EXTIRQ, LR_EXTIRQ_MODE, LR_EXTIRQ_POLARITY, LR_EXTIRQ_SHR, SR_IRQ,
    SR_IRQ_MODE, SR_IRQ_POLARITY, SR_IRQ_SHR, aml_crslen, aml_crstype, aml_evalname, aml_evalnode,
    aml_getname, aml_parse_resource, aml_searchrel, aml_val2int, cstr,
};
use crate::dev::pci::pcireg::{
    PCI_BHLC_REG, PCI_INTERRUPT_LINE_MASK, PCI_INTERRUPT_LINE_SHIFT, PCI_INTERRUPT_REG,
    pci_hdrtype_multifn, pci_interrupt_pin,
};
use crate::kern::subr_prf::Str;
use crate::machine::mpconfig::*;
use crate::machine::pci_machdep::{
    pci_conf_read, pci_conf_write, pci_lookup_segment, pci_make_tag,
};
use crate::sys::device::{CD_COCOVM, CfMatch, Cfattach, Cfdriver, DV_DULL, Device, Softc};
use crate::{kassert, kprintf};

/// `struct acpiprt_irq`.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct AcpiprtIrq {
    /// `_int`: the interrupt.
    pub _int: i32,
    /// `_shr`: shared.
    pub _shr: i32,
    /// `_ll`: active low.
    pub _ll: i32,
    /// `_he`: edge triggered.
    pub _he: i32,
}

/// `struct acpiprt_map`: a link device's routing, for `acpiprt_route_interrupt`.
pub struct AcpiprtMap {
    /// `bus`.
    pub bus: i32,
    /// `dev`.
    pub dev: i32,
    /// `pin`: 0 for INTA#.
    pub pin: i32,
    /// `irq`: the interrupt chosen for the link.
    pub irq: i32,
    /// `sc`.
    pub sc: &'static AcpiprtSoftc,
    /// `node`: the link device.
    pub node: AmlNodeRef,
}

/// `struct acpiprt_softc`.
#[repr(C)]
pub struct AcpiprtSoftc {
    /// `sc_dev`.
    pub sc_dev: Device,
    /// `sc_acpi`: acpi0, set by `acpiprt_attach`.
    pub sc_acpi: Cell<*const AcpiSoftc>,
    /// `sc_devnode`: the `_PRT` node, set by `acpiprt_attach` (kernel lock).
    pub sc_devnode: RefCell<Option<AmlNodeRef>>,
    /// `sc_bus`.
    pub sc_bus: Cell<i32>,
}

impl AcpiprtSoftc {
    /// `sc->sc_acpi`.
    fn acpi(&self) -> &'static AcpiSoftc {
        // SAFETY: `acpiprt_attach` set it to acpi0's softc, which is never freed, before
        // anything else reads it.
        match unsafe { self.sc_acpi.get().as_ref() } {
            Some(sc) => sc,
            None => crate::kern::subr_prf::panic(format_args!("acpiprt: no acpi0")),
        }
    }

    /// `sc->sc_devnode`.
    fn devnode(&self) -> Option<AmlNodeRef> {
        self.sc_devnode.borrow().clone()
    }
}

// SAFETY: `#[repr(C)]` with the device first; zeroes are a null pointer, an unborrowed
// `RefCell` holding `None` (an `Rc` is never null) and 0.
unsafe impl Softc for AcpiprtSoftc {}

/// `acpiprt_map_list`.
static ACPIPRT_MAP_LIST: AmlGlobal<RefCell<Vec<AcpiprtMap>>> =
    AmlGlobal::new(RefCell::new(Vec::new()));

/// `acpiprt_ca`.
pub static ACPIPRT_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<AcpiprtSoftc>(),
    ca_match: Some(acpiprt_match),
    ca_attach: acpiprt_attach,
    ca_detach: None,
    ca_activate: None,
};

/// `acpiprt_cd`.
pub static ACPIPRT_CD: Cfdriver = Cfdriver::new(b"acpiprt", DV_DULL, CD_COCOVM);

/// `acpiprt_match(parent, match, aux)`: acpi0's `_PRT` arguments.
pub fn acpiprt_match(_parent: Option<&Device>, match_: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: acpi0 hands its children `struct acpi_attach_args`.
    let aa = unsafe { &*aux.cast_const().cast::<AcpiAttachArgs>() };
    let cf = match_.cfdata();

    // sanity
    if aa.aaa_name.is_null() || !aa.aaa_table.is_null() {
        return 0;
    }
    // SAFETY: a non-null `aaa_name` is a NUL-terminated C string acpi0 wrote.
    let name = unsafe { core::ffi::CStr::from_ptr(aa.aaa_name.cast()) };
    if name.to_bytes() != cf.cf_driver.cd_name {
        return 0;
    }

    1
}

/// `acpiprt_attach(parent, self, aux)`.
pub fn acpiprt_attach(parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: `self_` was made for `acpiprt_ca`; the softc is never freed.
    let sc: &'static AcpiprtSoftc = unsafe { &*ptr::from_ref(self_.softc::<AcpiprtSoftc>()) };
    // SAFETY: acpi0 hands its children `struct acpi_attach_args`.
    let aa = unsafe { &*aux.cast_const().cast::<AcpiAttachArgs>() };

    let Some(parent) = parent else {
        return;
    };
    // SAFETY: acpiprt attaches at acpi0, whose softc is an `AcpiSoftc`.
    sc.sc_acpi
        .set(ptr::from_ref(unsafe { parent.softc::<AcpiSoftc>() }));
    *sc.sc_devnode.borrow_mut() = aa.aaa_node.clone();
    let Some(devnode) = sc.devnode() else {
        return;
    };
    sc.sc_bus.set(acpiprt_getpcibus(sc, &devnode));
    let pname = devnode.parent().map(|p| p.name).unwrap_or_default();
    kprintf!(": bus {} ({})", sc.sc_bus.get(), Str(cstr(&pname)));

    if sc.sc_bus.get() == -1 {
        kprintf!("\n");
        return;
    }

    let res = AmlValue::new();
    if aml_evalnode(Some(sc.acpi()), Some(&devnode), &[], Some(&res)) != 0 {
        kprintf!(": no PCI interrupt routing table\n");
        return;
    }

    if res.r#type() != AML_OBJTYPE_PACKAGE {
        kprintf!(": _PRT is not a package\n");
        return;
    }

    kprintf!("\n");

    for i in 0..res.length().max(0) as usize {
        if let Some(v) = res.v_package(i) {
            acpiprt_prt_add(sc, &v);
        }
    }
}

/// `acpiprt_getirq(crsidx, crs, arg)`: the interrupt a `_CRS` descriptor is set to.
pub fn acpiprt_getirq(_crsidx: i32, crs: &AcpiResource<'_>, irq: &mut AcpiprtIrq) -> i32 {
    irq._shr = 0;
    irq._ll = 0;
    irq._he = 1;

    let typ = aml_crstype(crs);
    let len = aml_crslen(crs);
    match typ {
        SR_IRQ => {
            // ffs(irq_mask) - 1
            let mask = crs.sr_irq_irq_mask();
            irq._int = if mask == 0 {
                -1
            } else {
                mask.trailing_zeros() as i32
            };
            if len > 2 {
                let flags = crs.sr_irq_irq_flags();
                irq._shr = i32::from(flags & SR_IRQ_SHR);
                irq._ll = i32::from(flags & SR_IRQ_POLARITY);
                irq._he = i32::from(flags & SR_IRQ_MODE);
            }
        }
        LR_EXTIRQ => {
            irq._int = crs.lr_extirq_irq(0) as i32;
            let flags = crs.lr_extirq_flags();
            irq._shr = i32::from(flags & LR_EXTIRQ_SHR);
            irq._ll = i32::from(flags & LR_EXTIRQ_POLARITY);
            irq._he = i32::from(flags & LR_EXTIRQ_MODE);
        }
        _ => {
            kprintf!("unknown interrupt: {:x}\n", typ);
        }
    }
    0
}

/// `acpiprt_pri[16]`: how much each ISA interrupt is worth picking for a link.
pub const ACPIPRT_PRI: [i32; 16] = [
    0, // 8254 Counter 0
    1, // Keyboard
    0, // 8259 Slave
    2, // Serial Port A
    2, // Serial Port B
    5, // Parallel Port / Generic
    2, // Floppy Disk
    4, // Parallel Port / Generic
    1, // RTC
    6, // Generic
    7, // Generic
    7, // Generic
    1, // Mouse
    0, // FPU
    2, // Primary IDE
    3, // Secondary IDE
];

/// `acpiprt_chooseirq(crsidx, crs, arg)`: the best interrupt a `_PRS` descriptor allows.
pub fn acpiprt_chooseirq(_crsidx: i32, crs: &AcpiResource<'_>, irq: &mut AcpiprtIrq) -> i32 {
    let mut pri = -1;

    irq._shr = 0;
    irq._ll = 0;
    irq._he = 1;

    let typ = aml_crstype(crs);
    let len = aml_crslen(crs);
    match typ {
        SR_IRQ => {
            let mask = crs.sr_irq_irq_mask();
            for (i, &p) in ACPIPRT_PRI.iter().enumerate() {
                if mask & (1 << i) != 0 && p > pri {
                    irq._int = i as i32;
                    pri = p;
                }
            }
            if len > 2 {
                let flags = crs.sr_irq_irq_flags();
                irq._shr = i32::from(flags & SR_IRQ_SHR);
                irq._ll = i32::from(flags & SR_IRQ_POLARITY);
                irq._he = i32::from(flags & SR_IRQ_MODE);
            }
        }
        LR_EXTIRQ => {
            let count = usize::from(crs.lr_extirq_irq_count());

            // First try non-8259 interrupts.
            for i in 0..count {
                if crs.lr_extirq_irq(i) > 15 {
                    irq._int = crs.lr_extirq_irq(i) as i32;
                    return 0;
                }
            }

            for i in 0..count {
                let n = crs.lr_extirq_irq(i) as usize;
                if ACPIPRT_PRI.get(n).is_some_and(|&p| p > pri) {
                    irq._int = n as i32;
                    pri = ACPIPRT_PRI[n];
                }
            }
            let flags = crs.lr_extirq_flags();
            irq._shr = i32::from(flags & LR_EXTIRQ_SHR);
            irq._ll = i32::from(flags & LR_EXTIRQ_POLARITY);
            irq._he = i32::from(flags & LR_EXTIRQ_MODE);
        }
        _ => {
            kprintf!("unknown interrupt: {:x}\n", typ);
        }
    }
    0
}

/// `acpiprt_prt_add(sc, v)`: records one `_PRT` entry.
pub fn acpiprt_prt_add(sc: &'static AcpiprtSoftc, v: &AmlValue) {
    let mut irq = AcpiprtIrq::default();

    if v.r#type() != AML_OBJTYPE_PACKAGE || v.length() != 4 {
        kprintf!("invalid mapping object\n");
        return;
    }

    let elem = |i: usize| v.v_package(i);
    let addr = aml_val2int(elem(0).as_deref()) as u64;
    let pin = aml_val2int(elem(1).as_deref()) as i32;
    if pin > 3 {
        return;
    }

    let Some(mut pp): Option<AmlValueRef> = elem(2) else {
        return;
    };
    if pp.r#type() == AML_OBJTYPE_NAMEREF {
        let name = pp
            .v_nameref()
            .map(|p| aml_getname(p.tail()))
            .unwrap_or_default();
        let Some(node) = aml_searchrel(sc.devnode().as_ref(), &name) else {
            kprintf!("Invalid device\n");
            return;
        };
        let Some(value) = node.value() else {
            return;
        };
        pp = value;
    }
    if pp.r#type() == AML_OBJTYPE_OBJREF {
        let Some(r) = pp.v_objref().and_then(|o| o.r#ref) else {
            return;
        };
        pp = r;
    }
    if pp.r#type() == AML_OBJTYPE_DEVICE {
        let Some(node) = pp.node() else {
            return;
        };

        let sta = acpi_getsta(sc.acpi(), Some(&node));
        if sta & i64::from(STA_PRESENT) == 0 {
            return;
        }

        let res = AmlValue::new();
        if aml_evalname(Some(sc.acpi()), Some(&node), b"_CRS", &[], Some(&res)) != 0 {
            kprintf!("no _CRS method\n");
            return;
        }

        if res.r#type() != AML_OBJTYPE_BUFFER || res.length() < 5 {
            kprintf!("invalid _CRS object\n");
            return;
        }
        aml_parse_resource(&res, &mut |i, crs| acpiprt_getirq(i, crs, &mut irq));

        // Pick a new IRQ if necessary.
        if irq._int == 0 || irq._int == 2 || irq._int == 13 {
            let res = AmlValue::new();
            if aml_evalname(Some(sc.acpi()), Some(&node), b"_PRS", &[], Some(&res)) == 0 {
                aml_parse_resource(&res, &mut |i, crs| acpiprt_chooseirq(i, crs, &mut irq));
            }
        }

        ACPIPRT_MAP_LIST.get().borrow_mut().push(AcpiprtMap {
            bus: sc.sc_bus.get(),
            dev: i32::from(acpi_pci_dev(addr << 16)),
            pin,
            irq: irq._int,
            sc,
            node,
        });
    } else {
        irq._int = aml_val2int(elem(3).as_deref()) as i32;
        irq._shr = 1;
        irq._ll = 1;
        irq._he = 0;
    }

    // ACPI_DEBUG: the "addr 0x.. pin .. irq .." line.

    // NIOAPIC > 0
    if NIOAPIC && nioapics() > 0 {
        let Some(apic) = ioapic_find_bybase(irq._int) else {
            kprintf!(
                "{}: no apic found for irq {}\n",
                sc.sc_dev.xname(),
                irq._int
            );
            return;
        };

        let mut map = MpIntrMap::new();
        map.ioapic = Some(apic);
        map.ioapic_pin = irq._int - ioapic_vecbase(apic);
        map.bus_pin = ((addr >> 14) & 0x7c) as i32 | (pin & 0x3);
        if irq._ll != 0 {
            map.flags |= MPS_INTPO_ACTLO << MPS_INTPO_SHIFT;
        } else {
            map.flags |= MPS_INTPO_ACTHI << MPS_INTPO_SHIFT;
        }
        if irq._he != 0 {
            map.flags |= MPS_INTTR_EDGE << MPS_INTTR_SHIFT;
        } else {
            map.flags |= MPS_INTTR_LEVEL << MPS_INTTR_SHIFT;
        }

        map.redir = IOAPIC_REDLO_DEL_LOPRI << IOAPIC_REDLO_DEL_SHIFT;
        let po = (map.flags >> MPS_INTPO_SHIFT) & MPS_INTPO_MASK;
        if po == MPS_INTPO_DEF || po == MPS_INTPO_ACTLO {
            map.redir |= IOAPIC_REDLO_ACTLO;
        }
        let tr = (map.flags >> MPS_INTTR_SHIFT) & MPS_INTTR_MASK;
        if tr == MPS_INTTR_DEF || tr == MPS_INTTR_LEVEL {
            map.redir |= IOAPIC_REDLO_LEVEL;
        }

        map.ioapic_ih = APIC_INT_VIA_APIC
            | (ioapic_apicid(apic) << APIC_INT_APIC_SHIFT)
            | (map.ioapic_pin << APIC_INT_PIN_SHIFT);

        let Some(bus) = mp_busses().and_then(|b| b.get(sc.sc_bus.get() as usize)) else {
            return;
        };
        let ioapic_pin = map.ioapic_pin;
        let map: &'static MpIntrMap = Box::leak(Box::new(map));
        ioapic_set_ip_map(apic, ioapic_pin, map);

        bus.push(map);

        return;
    }

    let bus = sc.sc_bus.get();
    let dev = i32::from(acpi_pci_dev(addr << 16));
    let Some(pc) = pci_lookup_segment(0, bus) else {
        return;
    };
    let tag = pci_make_tag(pc, bus, dev, 0);

    let reg = pci_conf_read(pc, tag, PCI_BHLC_REG);
    let nfuncs = if pci_hdrtype_multifn(reg) { 8 } else { 1 };

    for func in 0..nfuncs {
        let tag = pci_make_tag(pc, bus, dev, func);
        let mut reg = pci_conf_read(pc, tag, PCI_INTERRUPT_REG);
        if pci_interrupt_pin(reg) as i32 == pin + 1 {
            reg &= !(PCI_INTERRUPT_LINE_MASK << PCI_INTERRUPT_LINE_SHIFT);
            reg |= (irq._int as u32) << PCI_INTERRUPT_LINE_SHIFT;
            pci_conf_write(pc, tag, PCI_INTERRUPT_REG, reg);
        }
    }
}

/// `acpiprt_getpcibus(sc, node)`: the PCI bus of the `_PRT`'s device, -1 when it has none.
pub fn acpiprt_getpcibus(_sc: &AcpiprtSoftc, node: &AmlNodeRef) -> i32 {
    // Check if parent device has PCI mapping
    node.parent()
        .and_then(|p| p.pci.get())
        .map_or(-1, |pci| pci.sub.get())
}

/// `acpiprt_route_interrupt(bus, dev, pin)`: sets the link device of (`bus`, `dev`, `pin`)
/// to the interrupt `acpiprt_prt_add` chose, when it is not already.
pub fn acpiprt_route_interrupt(bus: i32, dev: i32, pin: i32) {
    let found = ACPIPRT_MAP_LIST
        .get()
        .borrow()
        .iter()
        .find(|p| p.bus == bus && p.dev == dev && p.pin == pin - 1)
        .map(|p| (p.irq, p.sc, p.node.clone()));
    let Some((newirq, sc, node)) = found else {
        return;
    };

    let sta = acpi_getsta(sc.acpi(), Some(&node));
    kassert!(sta & i64::from(STA_PRESENT) != 0);

    let res = AmlValue::new();
    if aml_evalname(Some(sc.acpi()), Some(&node), b"_CRS", &[], Some(&res)) != 0 {
        kprintf!("no _CRS method\n");
        return;
    }
    if res.r#type() != AML_OBJTYPE_BUFFER || res.length() < 5 {
        kprintf!("invalid _CRS object\n");
        return;
    }
    let mut irq = AcpiprtIrq::default();
    aml_parse_resource(&res, &mut |i, crs| acpiprt_getirq(i, crs, &mut irq));

    // Only re-route interrupts when necessary.
    if sta & i64::from(STA_ENABLED) != 0 && irq._int == newirq {
        return;
    }

    let mut crs = res.v_buffer();
    match aml_crstype(&AcpiResource::new(&crs)) {
        SR_IRQ => {
            let mask = (1u32 << newirq) as u16;
            crs[1..3].copy_from_slice(&mask.to_le_bytes());
        }
        LR_EXTIRQ => {
            if let Some(irq0) = crs.get_mut(5..9) {
                irq0.copy_from_slice(&(newirq as u32).to_le_bytes());
            }
        }
        _ => {}
    }

    let arg = [AmlValue::buffer(&crs)];
    let res2 = AmlValue::new();
    if aml_evalname(Some(sc.acpi()), Some(&node), b"_SRS", &arg, Some(&res2)) != 0 {
        kprintf!("no _SRS method\n");
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests of `acpiprt.rs`: reading and choosing a link device's interrupt from its
    // `_CRS`/`_PRS` descriptors.

    use super::*;

    /// An IRQ descriptor (`SR_IRQ`, three bytes of payload) for `mask` with `flags`.
    fn sr_irq(mask: u16, flags: u8) -> [u8; 4] {
        let m = mask.to_le_bytes();
        [0x23, m[0], m[1], flags]
    }

    /// An extended interrupt descriptor (`LR_EXTIRQ`) listing `irqs` with `flags`.
    fn lr_extirq(irqs: &[u32], flags: u8) -> std::vec::Vec<u8> {
        let len = (2 + 4 * irqs.len()) as u16;
        let mut b = std::vec![0x89, len.to_le_bytes()[0], len.to_le_bytes()[1], flags];
        b.push(irqs.len() as u8);
        for i in irqs {
            b.extend_from_slice(&i.to_le_bytes());
        }
        b
    }

    #[test]
    fn getirq_reads_the_set_interrupt() {
        let mut irq = AcpiprtIrq::default();
        // IRQ 11, level, active low, shared (the PIIX links' _CRS).
        let d = sr_irq(1 << 11, SR_IRQ_SHR | SR_IRQ_POLARITY);
        acpiprt_getirq(0, &AcpiResource::new(&d), &mut irq);
        assert_eq!(irq._int, 11);
        assert_ne!(irq._shr, 0);
        assert_ne!(irq._ll, 0);
        assert_eq!(irq._he, 0);

        // GSI 16, level, active high, shared (Q35's GSIx links).
        let d = lr_extirq(&[16], LR_EXTIRQ_SHR | 0x1);
        acpiprt_getirq(0, &AcpiResource::new(&d), &mut irq);
        assert_eq!(
            irq,
            AcpiprtIrq {
                _int: 16,
                _shr: i32::from(LR_EXTIRQ_SHR),
                _ll: 0,
                _he: 0
            }
        );

        // No interrupt set: ffs(0) - 1.
        let d = sr_irq(0, 0);
        acpiprt_getirq(0, &AcpiResource::new(&d), &mut irq);
        assert_eq!(irq._int, -1);
    }

    #[test]
    fn chooseirq_prefers_the_generic_lines_and_the_ioapic() {
        let mut irq = AcpiprtIrq::default();
        // 5, 10 and 11 allowed: 10 and 11 weigh 7, the first of them wins.
        let d = sr_irq((1 << 5) | (1 << 10) | (1 << 11), 0);
        acpiprt_chooseirq(0, &AcpiResource::new(&d), &mut irq);
        assert_eq!(irq._int, 10);

        // An I/O APIC input (> 15) is taken before any 8259 line.
        let d = lr_extirq(&[5, 10, 20], 0);
        acpiprt_chooseirq(0, &AcpiResource::new(&d), &mut irq);
        assert_eq!(irq._int, 20);

        let d = lr_extirq(&[3, 9], 0);
        acpiprt_chooseirq(0, &AcpiResource::new(&d), &mut irq);
        assert_eq!(irq._int, 9);
    }
}
/* </TESTS> */
