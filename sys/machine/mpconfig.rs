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
/* </LICENSES> */

/* <CODE> */
//! `<machine/mpconfig.h>` as a trait: the interrupt configuration the firmware describes
//! (the MP specification's tables, or ACPI's MADT and `_PRT`), and what the x86 drivers of
//! `sys/dev/acpi` (`acpimadt`, `acpiprt`) need from the machine to record it.
//!
//! On OpenBSD those two drivers include the x86 headers `<machine/mpbiosvar.h>` (with
//! `mpbiosreg.h` and `mpconfig.h`), `<machine/i82093reg.h>`, `<machine/i82093var.h>`,
//! `<machine/apicvar.h>`, `<machine/i82489var.h>` and `<machine/i8259.h>`, and are configured
//! on amd64 and i386 only. Here they are machine-independent code that reaches those headers
//! through this contract:
//!
//! - `struct mp_bus` and `struct mp_intr_map` ([`MpBus`], [`MpIntrMap`]) are defined here,
//!   because the generic drivers build them; amd64's `include/mpconfig.rs` re-exports them.
//! - The MP specification's interrupt flags (`MPS_INTPO_*`, `MPS_INTTR_*`), the I/O APIC
//!   redirection bits (`IOAPIC_REDLO_*`), the interrupt handle encoding (`APIC_INT_*`) and
//!   `ICU_LEN` are associated constants, re-exported below under their C names.
//! - The functions are the ones those drivers call: `lapic_boot_init`, `lapic_cpu_number`,
//!   `ioapic_find_bybase` with the softc members they read or write, the `cpu` and `ioapic`
//!   attachments at mainbus (the x86 `struct cpu_attach_args` and `struct apic_attach_args`
//!   are built by the machine), and the `mp_busses`/`mp_intrs` globals of `mainbus.c`.
//!
//! A machine without an I/O APIC (arm64, and the host test double) has no `struct
//! ioapic_softc` ([`NoIoapic`]), answers "none" and records nothing; its constants are 0 and
//! never used, since it configures neither driver.

use core::cell::Cell;

use crate::machine::Machine;
use crate::machine::bus::{BusAddr, BusSpaceTag};
use crate::sys::device::{CfprintT, Device};
use crate::sys::types::Paddr;

/// `struct ioapic_softc` of the selected machine.
pub type Ioapic = <Machine as MpConfig>::Ioapic;

/// The `struct ioapic_softc` of a machine without an I/O APIC: no value exists.
pub enum NoIoapic {}

/// `struct mp_bus`: a bus of the MP configuration, with the interrupt mappings of its pins.
///
/// ## Deviations
/// - `mb_intr_print` and `mb_intr_cfg` (which take `mpbios.c`'s `struct mpbios_int`) are
///   left out until `mpbios.c` is ported; the ACPI drivers never set them.
pub struct MpBus {
    /// `mb_name`: XXX bus name.
    pub mb_name: Cell<&'static str>,
    /// `mb_idx`: XXX bus index.
    pub mb_idx: Cell<i32>,
    /// `mb_intrs`: the head of the bus's interrupt mappings.
    pub mb_intrs: Cell<Option<&'static MpIntrMap>>,
    /// `mb_data`: random bus-specific datum.
    pub mb_data: Cell<u32>,
}

impl MpBus {
    /// A zeroed `struct mp_bus` (the C's static arrays are in `.bss`).
    pub const fn new() -> Self {
        Self {
            mb_name: Cell::new(""),
            mb_idx: Cell::new(0),
            mb_intrs: Cell::new(None),
            mb_data: Cell::new(0),
        }
    }

    /// The bus's mappings, `mb_intrs` and on through `next`.
    pub fn intrs(&self) -> impl Iterator<Item = &'static MpIntrMap> {
        let mut mip = self.mb_intrs.get();
        core::iter::from_fn(move || {
            let m = mip?;
            mip = m.next.get();
            Some(m)
        })
    }

    /// Pushes `map` at the head of the bus's mappings (`map->next = mb_intrs; mb_intrs =
    /// map`).
    pub fn push(&self, map: &'static MpIntrMap) {
        map.next.set(self.mb_intrs.get());
        self.mb_intrs.set(Some(map));
    }
}

impl Default for MpBus {
    fn default() -> Self {
        Self::new()
    }
}

// SAFETY: the buses are written while cold, by autoconfiguration under the kernel lock
// (acpimadt, acpiprt), and only read once interrupts are established.
unsafe impl Sync for MpBus {}

/// `struct mp_intr_map`: how one bus pin (or local APIC pin) is wired.
pub struct MpIntrMap {
    /// `next`.
    pub next: Cell<Option<&'static MpIntrMap>>,
    /// `bus`.
    pub bus: Option<&'static MpBus>,
    /// `bus_pin`.
    pub bus_pin: i32,
    /// `ioapic`: the I/O APIC the pin is on; `None` for a local APIC pin.
    pub ioapic: Option<&'static Ioapic>,
    /// `ioapic_pin`.
    pub ioapic_pin: i32,
    /// `ioapic_ih`: int handle, for apic_intr_est.
    pub ioapic_ih: i32,
    /// `type`: from mp spec intr record.
    pub type_: i32,
    /// `flags`: from mp spec intr record.
    pub flags: i32,
    /// `redir`.
    pub redir: u32,
    /// `cpu_id`.
    pub cpu_id: i32,
}

impl MpIntrMap {
    /// `memset(map, 0, sizeof *map)`.
    pub const fn new() -> Self {
        Self {
            next: Cell::new(None),
            bus: None,
            bus_pin: 0,
            ioapic: None,
            ioapic_pin: 0,
            ioapic_ih: 0,
            type_: 0,
            flags: 0,
            redir: 0,
            cpu_id: 0,
        }
    }
}

impl Default for MpIntrMap {
    fn default() -> Self {
        Self::new()
    }
}

// SAFETY: a mapping is filled in before it is published (pushed on a bus, stored in an I/O
// APIC pin or in `mp_intrs`) and read only afterwards; `next` changes only while the lists
// are built, by autoconfiguration under the kernel lock.
unsafe impl Sync for MpIntrMap {}

/// What the x86 ACPI drivers need from the machine (see the module's documentation).
pub trait MpConfig {
    /// `struct ioapic_softc`.
    type Ioapic: 'static;

    /// `MPS_INTPO_DEF`: polarity conforms to the bus.
    const MPS_INTPO_DEF: i32;
    /// `MPS_INTPO_ACTHI`.
    const MPS_INTPO_ACTHI: i32;
    /// `MPS_INTPO_ACTLO`.
    const MPS_INTPO_ACTLO: i32;
    /// `MPS_INTPO_SHIFT`.
    const MPS_INTPO_SHIFT: i32;
    /// `MPS_INTPO_MASK`.
    const MPS_INTPO_MASK: i32;
    /// `MPS_INTTR_DEF`: trigger mode conforms to the bus.
    const MPS_INTTR_DEF: i32;
    /// `MPS_INTTR_EDGE`.
    const MPS_INTTR_EDGE: i32;
    /// `MPS_INTTR_LEVEL`.
    const MPS_INTTR_LEVEL: i32;
    /// `MPS_INTTR_SHIFT`.
    const MPS_INTTR_SHIFT: i32;
    /// `MPS_INTTR_MASK`.
    const MPS_INTTR_MASK: i32;
    /// `IOAPIC_REDLO_DEL_MASK`.
    const IOAPIC_REDLO_DEL_MASK: u32;
    /// `IOAPIC_REDLO_DEL_SHIFT`.
    const IOAPIC_REDLO_DEL_SHIFT: u32;
    /// `IOAPIC_REDLO_DEL_LOPRI`.
    const IOAPIC_REDLO_DEL_LOPRI: u32;
    /// `IOAPIC_REDLO_DEL_NMI`.
    const IOAPIC_REDLO_DEL_NMI: u32;
    /// `IOAPIC_REDLO_ACTLO`.
    const IOAPIC_REDLO_ACTLO: u32;
    /// `IOAPIC_REDLO_LEVEL`.
    const IOAPIC_REDLO_LEVEL: u32;
    /// `APIC_INT_VIA_APIC`.
    const APIC_INT_VIA_APIC: i32;
    /// `APIC_INT_APIC_SHIFT`.
    const APIC_INT_APIC_SHIFT: i32;
    /// `APIC_INT_PIN_SHIFT`.
    const APIC_INT_PIN_SHIFT: i32;
    /// `ICU_LEN`: the 8259 pairs' sixteen inputs.
    const ICU_LEN: i32;
    /// `NIOAPIC > 0`: the kernel configures `ioapic*`.
    const NIOAPIC: bool;

    /// `lapic_boot_init(lapic_base)`: maps the boot processor's local APIC and sets up its
    /// fixed vectors.
    fn lapic_boot_init(lapic_base: Paddr);

    /// `lapic_cpu_number()`: the running processor's local APIC ID.
    fn lapic_cpu_number() -> u32;

    /// `config_found(parent, &caa, print)` for a processor: a `struct cpu_attach_args` named
    /// `"cpu"` with `apic_id`, `acpi_proc_id`, `CPU_ROLE_BP` when `bp` (else
    /// `CPU_ROLE_AP`) and, with `MULTIPROCESSOR`, `mp_cpu_funcs`.
    fn mp_attach_cpu(parent: &Device, apic_id: u32, acpi_proc_id: u32, bp: bool, print: CfprintT);

    /// `config_found(parent, &aaa, print)` for an I/O APIC: a `struct apic_attach_args`
    /// named `"ioapic"`.
    fn mp_attach_ioapic(
        parent: &Device,
        memt: BusSpaceTag,
        apic_id: i32,
        address: BusAddr,
        vecbase: i32,
        print: CfprintT,
    );

    /// `ioapic_find_bybase(vec)`: the I/O APIC whose global interrupts include `vec`.
    fn ioapic_find_bybase(vec: i32) -> Option<&'static Self::Ioapic>;

    /// `apic->sc_apicid`.
    fn ioapic_apicid(apic: &Self::Ioapic) -> i32;

    /// `apic->sc_apic_vecbase`.
    fn ioapic_vecbase(apic: &Self::Ioapic) -> i32;

    /// `apic->sc_pins[pin].ip_map = map`.
    fn ioapic_set_ip_map(apic: &Self::Ioapic, pin: i32, map: &'static MpIntrMap);

    /// `nioapics`: the I/O APICs attached.
    fn nioapics() -> i32;

    /// `mp_busses = busses; mp_nbusses = nitems(busses); mp_isa_bus = isa`.
    fn mp_set_busses(busses: &'static [MpBus], isa: &'static MpBus);

    /// `mp_intrs = intrs; mp_nintrs = nitems(intrs)`.
    fn mp_set_intrs(intrs: &'static [MpIntrMap]);

    /// `mp_busses` (with `mp_nbusses`), `None` while no table set it.
    fn mp_busses() -> Option<&'static [MpBus]>;
}

/// `MPS_INTPO_DEF`.
pub const MPS_INTPO_DEF: i32 = <Machine as MpConfig>::MPS_INTPO_DEF;
/// `MPS_INTPO_ACTHI`.
pub const MPS_INTPO_ACTHI: i32 = <Machine as MpConfig>::MPS_INTPO_ACTHI;
/// `MPS_INTPO_ACTLO`.
pub const MPS_INTPO_ACTLO: i32 = <Machine as MpConfig>::MPS_INTPO_ACTLO;
/// `MPS_INTPO_SHIFT`.
pub const MPS_INTPO_SHIFT: i32 = <Machine as MpConfig>::MPS_INTPO_SHIFT;
/// `MPS_INTPO_MASK`.
pub const MPS_INTPO_MASK: i32 = <Machine as MpConfig>::MPS_INTPO_MASK;
/// `MPS_INTTR_DEF`.
pub const MPS_INTTR_DEF: i32 = <Machine as MpConfig>::MPS_INTTR_DEF;
/// `MPS_INTTR_EDGE`.
pub const MPS_INTTR_EDGE: i32 = <Machine as MpConfig>::MPS_INTTR_EDGE;
/// `MPS_INTTR_LEVEL`.
pub const MPS_INTTR_LEVEL: i32 = <Machine as MpConfig>::MPS_INTTR_LEVEL;
/// `MPS_INTTR_SHIFT`.
pub const MPS_INTTR_SHIFT: i32 = <Machine as MpConfig>::MPS_INTTR_SHIFT;
/// `MPS_INTTR_MASK`.
pub const MPS_INTTR_MASK: i32 = <Machine as MpConfig>::MPS_INTTR_MASK;
/// `IOAPIC_REDLO_DEL_MASK`.
pub const IOAPIC_REDLO_DEL_MASK: u32 = <Machine as MpConfig>::IOAPIC_REDLO_DEL_MASK;
/// `IOAPIC_REDLO_DEL_SHIFT`.
pub const IOAPIC_REDLO_DEL_SHIFT: u32 = <Machine as MpConfig>::IOAPIC_REDLO_DEL_SHIFT;
/// `IOAPIC_REDLO_DEL_LOPRI`.
pub const IOAPIC_REDLO_DEL_LOPRI: u32 = <Machine as MpConfig>::IOAPIC_REDLO_DEL_LOPRI;
/// `IOAPIC_REDLO_DEL_NMI`.
pub const IOAPIC_REDLO_DEL_NMI: u32 = <Machine as MpConfig>::IOAPIC_REDLO_DEL_NMI;
/// `IOAPIC_REDLO_ACTLO`.
pub const IOAPIC_REDLO_ACTLO: u32 = <Machine as MpConfig>::IOAPIC_REDLO_ACTLO;
/// `IOAPIC_REDLO_LEVEL`.
pub const IOAPIC_REDLO_LEVEL: u32 = <Machine as MpConfig>::IOAPIC_REDLO_LEVEL;
/// `APIC_INT_VIA_APIC`.
pub const APIC_INT_VIA_APIC: i32 = <Machine as MpConfig>::APIC_INT_VIA_APIC;
/// `APIC_INT_APIC_SHIFT`.
pub const APIC_INT_APIC_SHIFT: i32 = <Machine as MpConfig>::APIC_INT_APIC_SHIFT;
/// `APIC_INT_PIN_SHIFT`.
pub const APIC_INT_PIN_SHIFT: i32 = <Machine as MpConfig>::APIC_INT_PIN_SHIFT;
/// `ICU_LEN`.
pub const ICU_LEN: i32 = <Machine as MpConfig>::ICU_LEN;
/// `NIOAPIC > 0`.
pub const NIOAPIC: bool = <Machine as MpConfig>::NIOAPIC;

/// `lapic_boot_init(lapic_base)`.
pub fn lapic_boot_init(lapic_base: Paddr) {
    Machine::lapic_boot_init(lapic_base)
}

/// `lapic_cpu_number()`.
pub fn lapic_cpu_number() -> u32 {
    Machine::lapic_cpu_number()
}

/// A processor's `config_found` at `parent` (see [`MpConfig::mp_attach_cpu`]).
pub fn mp_attach_cpu(parent: &Device, apic_id: u32, acpi_proc_id: u32, bp: bool, print: CfprintT) {
    Machine::mp_attach_cpu(parent, apic_id, acpi_proc_id, bp, print)
}

/// An I/O APIC's `config_found` at `parent` (see [`MpConfig::mp_attach_ioapic`]).
pub fn mp_attach_ioapic(
    parent: &Device,
    memt: BusSpaceTag,
    apic_id: i32,
    address: BusAddr,
    vecbase: i32,
    print: CfprintT,
) {
    Machine::mp_attach_ioapic(parent, memt, apic_id, address, vecbase, print)
}

/// `ioapic_find_bybase(vec)`.
pub fn ioapic_find_bybase(vec: i32) -> Option<&'static Ioapic> {
    Machine::ioapic_find_bybase(vec)
}

/// `apic->sc_apicid`.
pub fn ioapic_apicid(apic: &Ioapic) -> i32 {
    Machine::ioapic_apicid(apic)
}

/// `apic->sc_apic_vecbase`.
pub fn ioapic_vecbase(apic: &Ioapic) -> i32 {
    Machine::ioapic_vecbase(apic)
}

/// `apic->sc_pins[pin].ip_map = map`.
pub fn ioapic_set_ip_map(apic: &Ioapic, pin: i32, map: &'static MpIntrMap) {
    Machine::ioapic_set_ip_map(apic, pin, map)
}

/// `nioapics`.
pub fn nioapics() -> i32 {
    Machine::nioapics()
}

/// `mp_busses = busses; mp_nbusses = nitems(busses); mp_isa_bus = isa`.
pub fn mp_set_busses(busses: &'static [MpBus], isa: &'static MpBus) {
    Machine::mp_set_busses(busses, isa)
}

/// `mp_intrs = intrs; mp_nintrs = nitems(intrs)`.
pub fn mp_set_intrs(intrs: &'static [MpIntrMap]) {
    Machine::mp_set_intrs(intrs)
}

/// `mp_busses`, `None` while it is NULL.
pub fn mp_busses() -> Option<&'static [MpBus]> {
    Machine::mp_busses()
}
/* </CODE> */
