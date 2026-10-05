/* $OpenBSD: ampintc.c,v 1.35 2025/12/15 12:59:24 dlg Exp $ */
/* <LICENSES> */
/*
 * Copyright (c) 2007,2009,2011 Dale Rahn <drahn@openbsd.org>
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

//! The ARM generic interrupt controller, version 2: `arch/arm64/dev/ampintc.c`. This driver
//! implements the interrupt controller as specified in DDI0407E_cortex_a9_mpcore_r2p0_trm
//! with the IHI0048A_gic_architecture_spec_v1_0 underlying specification.
//!
//! Upstream: sys/arch/arm64/dev/ampintc.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M4 ports the registers, `ampintc_match`, `ampintc_attach` (from
//! the device-tree attach arguments; `ampintc_ca`/`ampintc_cd` attach them through mainbus
//! since M7b),
//! `ampintc_init`, `ampintc_set_priority`, `ampintc_setipl`, `ampintc_intr_enable`/`disable`/
//! `config`, `ampintc_calc_mask`/`calc_irq`, `ampintc_splx`/`spllower`/`splraise`,
//! `ampintc_iack`/`eoi`, `ampintc_route`, `ampintc_cpuinit`, `ampintc_route_irq`,
//! `ampintc_intr_barrier`, `ampintc_run_handler`, `ampintc_irq_handler`,
//! `ampintc_intr_establish_fdt`, `ampintc_intr_establish` and `ampintc_intr_disestablish`.
//! M11a adds the `MULTIPROCESSOR` side: the IPI's SGI found and established in
//! `ampintc_attach` (` ipi N`), `ampintc_ipi_ddb`, `ampintc_ipi_halt`, `ampintc_ipi_handler`,
//! `ampintc_send_ipi` (`intr_send_ipi_func`), the IPI EOI of `ampintc_cpuinit` (the
//! application processors' per-CPU init: their banked SGI/PPI registers, their target mask)
//! and the kernel lock in `ampintc_run_handler` (`IPL_MPSAFE` honoured as in C since M11e).
//! Device interrupts stay routed to the boot CPU (`ci` `NULL` is `cpu_info_primary`), as in
//! C. M12 adds, with PCI, the `simplebus_attach` of the GIC's children at the end of
//! `ampintc_attach` and the GICv2m MSI frame (`ampintcmsi* at fdt? early 1`: the `GICV2M_*`
//! registers, `ampintc_msi_match`, `ampintc_msi_attach`, `ampintc_intr_establish_msi`,
//! `ampintc_intr_disestablish_msi`, `ampintc_intr_barrier_msi`). `ampintc_activate`
//! (`DVACT_RESUME`) is not here.
//!
//! ## Deviations
//! - One static softc, `AMPINTC`, stands for the C's `ampintc` pointer to the attached
//!   device and for the rest of `struct ampintc_softc`: `ampintc_ca`'s `ca_devsize` is the
//!   `struct simplebus_softc` that heads the C's softc (`sc_sbus`, which `simplebus_attach`
//!   fills for the GIC's children), and mainbus attaches it from the device tree (`ampintc*
//!   at fdt? early 1`); `ampintc_activate` (`DVACT_RESUME`) is not in it yet.
//! - A GICv2m frame's `sc_ic` is an `UnsafeCell`, written once by `ampintc_msi_attach`
//!   before it registers it; `sc_spi`'s slots are `Cell`s, and an MSI's cookie is the
//!   address of its slot, as in C.
//! - `sc_cpu_mask` and `sc_ipi_reason` are atomics (each CPU writes its own mask in
//!   `ampintc_cpuinit` while others read it to send IPIs); `ampintc_send_ipi` fences before
//!   the `ICD_SGIR` write so the posted reason is visible to the target's handler.
//! - `ampintc_ipi_count` (feature `qemu`) reads the IPI handler's event counter for the
//!   self-check in `cpu_boot_secondary_processors`; not in the C.

use core::cell::{Cell, UnsafeCell};
use core::ffi::c_void;
use core::ptr::{self, NonNull};
#[cfg(feature = "multiprocessor")]
use core::sync::atomic::fence;
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicU8, AtomicU32, Ordering};

#[cfg(feature = "multiprocessor")]
use crate::arch::arm64::arm64::cpu::cpu_halt;
#[cfg(feature = "multiprocessor")]
use crate::arch::arm64::arm64::db_interface::db_enter;
#[cfg(feature = "multiprocessor")]
use crate::arch::arm64::arm64::intr::INTR_SEND_IPI_FUNC;
use crate::arch::arm64::arm64::intr::{
    arm_do_pending_intr, arm_init_smask, arm_intr_register_fdt, arm_set_intr_handler, arm_smask,
    interrupt_controllers, intr_barrier,
};
use crate::arch::arm64::arm64::pmap::{pmap_extract, pmap_kernel};
use crate::arch::arm64::dev::simplebus::simplebus_attach;
use crate::arch::arm64::include::cpu::{CpuInfo, cpu_info_primary, cpu_number, curcpu};
use crate::arch::arm64::include::cpu::{intr_disable, intr_enable, intr_restore};
use crate::arch::arm64::include::fdt::{FdtAttachArgs, fdt_intr_disestablish};
use crate::arch::arm64::include::frame::Trapframe;
#[cfg(feature = "multiprocessor")]
use crate::arch::arm64::include::intr::{ARM_IPI_DDB, ARM_IPI_HALT, ARM_IPI_NOP, IPL_IPI};
use crate::arch::arm64::include::intr::{
    IPL_FLAGMASK, IPL_HIGH, IPL_IRQMASK, IPL_MPSAFE, IPL_NONE, IPL_SCHED, IST_EDGE_RISING,
    IST_LEVEL_HIGH, InterruptController, IntrFn, MachineIntrHandle,
};
use crate::arch::arm64::include::simplebusvar::SimplebusSoftc;
use crate::dev::ofw::openfirm::{OF_getpropint, OF_is_compatible, OF_parent};
use crate::kassert;
use crate::kern::kern_malloc::{free, malloc, mallocarray};
use crate::kern::subr_evcount::{evcount_attach, evcount_detach};
use crate::kern::subr_prf::panic;
use crate::kprintf;
use crate::machine::bus::{
    BusSpaceHandle, BusSpaceTag, bus_space_map, bus_space_read_1, bus_space_read_4,
    bus_space_write_1, bus_space_write_4,
};
use crate::queue_adapter;
use crate::sys::device::{CfMatch, Cfattach, Cfdriver, DV_DULL, Device, Softc};
use crate::sys::evcount::Evcount;
use crate::sys::malloc::{M_DEVBUF, M_NOWAIT, M_WAITOK, M_ZERO};
use crate::sys::queue::{ListEntry, TailqEntry, TailqHead};
use crate::sys::systm::{kernel_lock, kernel_unlock};
use crate::sys::types::Vaddr;

// registers
/// `ICD_DCR`: the distributor control register.
pub const ICD_DCR: usize = 0x000;
/// `ICD_DCR_ES`.
pub const ICD_DCR_ES: u32 = 0x0000_0001;
/// `ICD_DCR_ENS`.
pub const ICD_DCR_ENS: u32 = 0x0000_0002;
/// `ICD_ICTR`: the interrupt controller type register.
pub const ICD_ICTR: usize = 0x004;
/// `ICD_ICTR_LSPI_SH`.
pub const ICD_ICTR_LSPI_SH: u32 = 11;
/// `ICD_ICTR_LSPI_M`.
pub const ICD_ICTR_LSPI_M: u32 = 0x1f;
/// `ICD_ICTR_CPU_SH`.
pub const ICD_ICTR_CPU_SH: u32 = 5;
/// `ICD_ICTR_CPU_M`.
pub const ICD_ICTR_CPU_M: u32 = 0x07;
/// `ICD_ICTR_ITL_SH`.
pub const ICD_ICTR_ITL_SH: u32 = 0;
/// `ICD_ICTR_ITL_M`.
pub const ICD_ICTR_ITL_M: u32 = 0x1f;
/// `ICD_IDIR`: the distributor implementer identification register.
pub const ICD_IDIR: usize = 0x008;

/// `IRQ_TO_REG32(i)`.
pub const fn irq_to_reg32(i: i32) -> usize {
    ((i >> 5) & 0x1f) as usize
}
/// `IRQ_TO_REG32BIT(i)`.
pub const fn irq_to_reg32bit(i: i32) -> u32 {
    (i & 0x1f) as u32
}
/// `IRQ_TO_REG16(i)`.
pub const fn irq_to_reg16(i: i32) -> usize {
    ((i >> 4) & 0x3f) as usize
}
/// `IRQ_TO_REG16BIT(i)`.
pub const fn irq_to_reg16bit(i: i32) -> u32 {
    (i & 0xf) as u32
}

/// `ICD_ISRn(i)`: interrupt security.
pub const fn icd_isrn(i: i32) -> usize {
    0x080 + irq_to_reg32(i) * 4
}
/// `ICD_ISERn(i)`: interrupt set-enable.
pub const fn icd_isern(i: i32) -> usize {
    0x100 + irq_to_reg32(i) * 4
}
/// `ICD_ICERn(i)`: interrupt clear-enable.
pub const fn icd_icern(i: i32) -> usize {
    0x180 + irq_to_reg32(i) * 4
}
/// `ICD_ISPRn(i)`: interrupt set-pending.
pub const fn icd_isprn(i: i32) -> usize {
    0x200 + irq_to_reg32(i) * 4
}
/// `ICD_ICPRn(i)`: interrupt clear-pending.
pub const fn icd_icprn(i: i32) -> usize {
    0x280 + irq_to_reg32(i) * 4
}
/// `ICD_ABRn(i)`: active bit.
pub const fn icd_abrn(i: i32) -> usize {
    0x300 + irq_to_reg32(i) * 4
}
/// `ICD_IPRn(i)`: interrupt priority, one byte per interrupt.
pub const fn icd_iprn(i: i32) -> usize {
    0x400 + i as usize
}
/// `ICD_IPTRn(i)`: interrupt processor targets, one byte per interrupt.
pub const fn icd_iptrn(i: i32) -> usize {
    0x800 + i as usize
}
/// `ICD_ICRn(i)`: interrupt configuration, two bits per interrupt.
pub const fn icd_icrn(i: i32) -> usize {
    0xc00 + irq_to_reg16(i) * 4
}
/// `ICD_ICR_TRIG_LEVEL(i)`.
pub const fn icd_icr_trig_level(i: i32) -> u32 {
    0x0 << (irq_to_reg16bit(i) * 2)
}
/// `ICD_ICR_TRIG_EDGE(i)`.
pub const fn icd_icr_trig_edge(i: i32) -> u32 {
    0x2 << (irq_to_reg16bit(i) * 2)
}
/// `ICD_ICR_TRIG_MASK(i)`.
pub const fn icd_icr_trig_mask(i: i32) -> u32 {
    0x2 << (irq_to_reg16bit(i) * 2)
}

/// `ICD_PPI`: the PPI status register.
pub const ICD_PPI: usize = 0xd00;
/// `ICD_SPI_BASE`: the SPI status registers.
pub const ICD_SPI_BASE: usize = 0xd04;
/// `ICD_SGIR`: the software generated interrupt register.
pub const ICD_SGIR: usize = 0xf00;

/// `ICPICR`: the CPU interface control register.
pub const ICPICR: usize = 0x00;
/// `ICPIPMR`: the interrupt priority mask register.
pub const ICPIPMR: usize = 0x04;
/// `ICMIPMR_SH`: XXX - must left justify bits to 0 - 7.
pub const ICMIPMR_SH: u32 = 4;
/// `ICPBPR`: the binary point register.
pub const ICPBPR: usize = 0x08;
/// `ICPIAR`: the interrupt acknowledge register.
pub const ICPIAR: usize = 0x0c;
/// `ICPIAR_IRQ_SH`.
pub const ICPIAR_IRQ_SH: u32 = 0;
/// `ICPIAR_IRQ_M`.
pub const ICPIAR_IRQ_M: u32 = 0x3ff;
/// `ICPIAR_CPUID_SH`.
pub const ICPIAR_CPUID_SH: u32 = 10;
/// `ICPIAR_CPUID_M`.
pub const ICPIAR_CPUID_M: u32 = 0x7;
/// `ICPIAR_NO_PENDING_IRQ`.
pub const ICPIAR_NO_PENDING_IRQ: u32 = ICPIAR_IRQ_M;
/// `ICPEOIR`: the end of interrupt register.
pub const ICPEOIR: usize = 0x10;
/// `ICPPRP`: the running priority register.
pub const ICPPRP: usize = 0x14;
/// `ICPHPIR`: the highest pending interrupt register.
pub const ICPHPIR: usize = 0x18;
/// `ICPIIR`: the CPU interface identification register.
pub const ICPIIR: usize = 0xfc;

/// `IRQ_ENABLE`.
pub const IRQ_ENABLE: bool = true;
/// `IRQ_DISABLE`.
pub const IRQ_DISABLE: bool = false;

// GICv2m frame controller for MSI interrupts.

/// `GICV2M_TYPER`.
pub const GICV2M_TYPER: usize = 0x008;

/// `GICV2M_TYPER_SPI_BASE(x)`: the frame's first SPI.
pub const fn gicv2m_typer_spi_base(x: u32) -> u32 {
    (x >> 16) & 0x3ff
}

/// `GICV2M_TYPER_SPI_COUNT(x)`: how many SPIs the frame has.
pub const fn gicv2m_typer_spi_count(x: u32) -> u32 {
    x & 0x3ff
}

/// `GICV2M_SETSPI_NS`: the doorbell a device writes the SPI number to.
pub const GICV2M_SETSPI_NS: usize = 0x040;

/// `struct intrhand`: one established handler.
pub struct Intrhand {
    /// `ih_list`: link on intrq list.
    pub ih_list: TailqEntry<Intrhand>,
    /// `ih_func`: handler.
    pub ih_func: IntrFn,
    /// `ih_arg`: arg for handler.
    pub ih_arg: *mut c_void,
    /// `ih_ipl`: `IPL_*`.
    pub ih_ipl: i32,
    /// `ih_flags`.
    pub ih_flags: i32,
    /// `ih_irq`: IRQ number.
    pub ih_irq: Cell<i32>,
    /// `ih_count`.
    pub ih_count: Evcount,
    /// `ih_name`.
    pub ih_name: Option<&'static str>,
    /// `ih_ci`: CPU the IRQ runs on.
    pub ih_ci: Cell<*const CpuInfo>,
}

queue_adapter!(
    /// `TAILQ_HEAD(, intrhand) iq_list`.
    pub IhList: Intrhand, ih_list => TailqEntry<Intrhand>
);

/// `struct intrq`: the handlers of one interrupt.
pub struct Intrq {
    /// `iq_list`: handler list.
    pub iq_list: TailqHead<IhList>,
    /// `iq_ci`: CPU the IRQ runs on.
    pub iq_ci: Cell<*const CpuInfo>,
    /// `iq_irq_max`: IRQ to mask while handling.
    pub iq_irq_max: Cell<i32>,
    /// `iq_irq_min`: lowest IRQ when shared.
    pub iq_irq_min: Cell<i32>,
    /// `iq_ist`: share type.
    pub iq_ist: Cell<i32>,
}

impl Intrq {
    const fn new() -> Self {
        Self {
            iq_list: TailqHead::new(),
            iq_ci: Cell::new(ptr::null()),
            iq_irq_max: Cell::new(0),
            iq_irq_min: Cell::new(0),
            iq_ist: Cell::new(0),
        }
    }
}

/// `struct ampintc_softc`.
pub struct AmpintcSoftc {
    // sc_sbus (struct simplebus_softc): M5.
    /// `sc_handler`: one `intrq` per interrupt, `sc_nintr` of them.
    pub sc_handler: Cell<*mut Intrq>,
    /// `sc_nintr`.
    pub sc_nintr: Cell<i32>,
    /// `sc_iot`.
    pub sc_iot: Cell<Option<BusSpaceTag>>,
    /// `sc_d_ioh`: the distributor.
    pub sc_d_ioh: Cell<Option<BusSpaceHandle>>,
    /// `sc_p_ioh`: the CPU interface.
    pub sc_p_ioh: Cell<Option<BusSpaceHandle>>,
    /// `sc_cpu_mask`: each CPU's bit in the target registers.
    pub sc_cpu_mask: [AtomicU8; ICD_ICTR_CPU_M as usize + 1],
    /// `sc_spur`: the spurious interrupt counter.
    pub sc_spur: Evcount,
    /// `sc_ic`: the registered interrupt controller.
    pub sc_ic: InterruptController,
    /// `sc_ipi_reason`: per CPU, the `1 << ARM_IPI_*` bits waiting for its IPI handler.
    pub sc_ipi_reason: [AtomicU32; ICD_ICTR_CPU_M as usize + 1],
    /// `sc_ipi_num`: the SGI the IPIs use (`MULTIPROCESSOR`; 0 without).
    pub sc_ipi_num: AtomicI32,
}

// SAFETY: the one controller, attached on the boot CPU before interrupts are enabled; the
// cells are written at attach and establish time with interrupts disabled.
unsafe impl Sync for AmpintcSoftc {}

/// `struct ampintc_msi_softc`: a GICv2m frame.
#[repr(C)]
pub struct AmpintcMsiSoftc {
    /// `sc_dev`.
    pub sc_dev: Device,
    /// `sc_iot`.
    pub sc_iot: Cell<Option<BusSpaceTag>>,
    /// `sc_ioh`.
    pub sc_ioh: Cell<Option<BusSpaceHandle>>,
    /// `sc_node`.
    pub sc_node: Cell<i32>,
    /// `sc_addr`: the frame's physical address, as the CPU sees it.
    pub sc_addr: Cell<usize>,
    /// `sc_bspi`: the first SPI.
    pub sc_bspi: Cell<i32>,
    /// `sc_nspi`: how many.
    pub sc_nspi: Cell<i32>,
    /// `sc_spi`: per SPI, the handle established on the parent GIC (NULL: free); `sc_nspi`
    /// slots, `malloc`ed zeroed. An MSI's cookie is the address of its slot.
    pub sc_spi: Cell<*mut Cell<*mut MachineIntrHandle>>,
    /// `sc_ic`: the registered controller; written once by `ampintc_msi_attach` before it
    /// registers it, read-only afterwards.
    pub sc_ic: UnsafeCell<InterruptController>,
}

impl AmpintcMsiSoftc {
    /// `&sc->sc_spi[i]`.
    fn spi(&self, i: i32) -> &Cell<*mut MachineIntrHandle> {
        let base = self.sc_spi.get();
        if base.is_null() || i < 0 || i >= self.sc_nspi.get() {
            panic(format_args!("ampintcmsi: bad spi index {i}"));
        }
        // SAFETY: attach allocated `sc_nspi` zeroed slots (valid all-zero) at `sc_spi`, never
        // freed.
        unsafe { &*base.add(i as usize) }
    }
}

// SAFETY: `#[repr(C)]` with the device first; the other members are `Cell`s of integers,
// raw pointers and `Option`s of references, and a controller whose members are `Cell`s,
// `Option`s of function pointers and a list entry of raw pointers: all valid as zero bits.
unsafe impl Softc for AmpintcMsiSoftc {}

/// `ampintc`: the attached controller.
pub static AMPINTC: AmpintcSoftc = AmpintcSoftc {
    sc_handler: Cell::new(ptr::null_mut()),
    sc_nintr: Cell::new(0),
    sc_iot: Cell::new(None),
    sc_d_ioh: Cell::new(None),
    sc_p_ioh: Cell::new(None),
    sc_cpu_mask: [const { AtomicU8::new(0) }; ICD_ICTR_CPU_M as usize + 1],
    sc_spur: Evcount::new(),
    sc_ic: InterruptController {
        ic_node: Cell::new(0),
        ic_cookie: Cell::new(ptr::null()),
        ic_establish: Some(ampintc_intr_establish_fdt),
        ic_establish_msi: None,
        ic_disestablish: Some(ampintc_intr_disestablish),
        ic_enable: None,
        ic_disable: None,
        ic_route: Some(ampintc_route_irq),
        ic_cpu_enable: Some(ampintc_cpuinit),
        ic_barrier: Some(ampintc_intr_barrier),
        ic_set_wakeup: None,
        ic_list: ListEntry::new(),
        ic_phandle: Cell::new(0),
        ic_cells: Cell::new(0),
        ic_gic_its_id: Cell::new(0),
    },
    sc_ipi_reason: [const { AtomicU32::new(0) }; ICD_ICTR_CPU_M as usize + 1],
    sc_ipi_num: AtomicI32::new(0),
};
/// `ampintc_ca`: the device is the `struct simplebus_softc` that heads the C's
/// `struct ampintc_softc` (`sc_sbus`), for the frames `simplebus_attach` attaches below it.
pub static AMPINTC_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<SimplebusSoftc>(),
    ca_match: Some(ampintc_match),
    ca_attach: ampintc_attach,
    ca_detach: None,
    ca_activate: None,
};

/// `ampintc_cd`.
pub static AMPINTC_CD: Cfdriver = Cfdriver::new(b"ampintc", DV_DULL, 0);

/// `ampintcmsi_ca`.
pub static AMPINTCMSI_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<AmpintcMsiSoftc>(),
    ca_match: Some(ampintc_msi_match),
    ca_attach: ampintc_msi_attach,
    ca_detach: None,
    ca_activate: None,
};

/// `ampintcmsi_cd`.
pub static AMPINTCMSI_CD: Cfdriver = Cfdriver::new(b"ampintcmsi", DV_DULL, 0);

/// Whether `ampintc_attach` has run (the C's `ampintc != NULL`).
static ATTACHED: AtomicBool = AtomicBool::new(false);

/// `ampintc_compatibles[]`.
static AMPINTC_COMPATIBLES: [&[u8]; 4] = [
    b"arm,cortex-a7-gic",
    b"arm,cortex-a9-gic",
    b"arm,cortex-a15-gic",
    b"arm,gic-400",
];

/// The mapped registers.
fn regs() -> (BusSpaceTag, BusSpaceHandle, BusSpaceHandle) {
    let sc = &AMPINTC;
    match (sc.sc_iot.get(), sc.sc_d_ioh.get(), sc.sc_p_ioh.get()) {
        (Some(iot), Some(d), Some(p)) => (iot, d, p),
        _ => panic(format_args!("ampintc: not attached")),
    }
}

/// `sc->sc_handler[irq]`.
fn handler(sc: &AmpintcSoftc, irq: i32) -> &'static Intrq {
    kassert!(irq >= 0 && irq < sc.sc_nintr.get());
    // SAFETY: `sc_handler` has `sc_nintr` entries, allocated by `ampintc_attach`.
    unsafe { &*sc.sc_handler.get().add(irq as usize) }
}

/// `ampintc_match`: whether the node is a GICv2 this driver drives.
pub fn ampintc_match(_parent: Option<&Device>, _cfdata: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: `ampintc` attaches at `fdt`, whose buses hand over a `FdtAttachArgs`.
    let faa = unsafe { &*aux.cast::<FdtAttachArgs<'_>>() };
    i32::from(
        AMPINTC_COMPATIBLES
            .iter()
            .any(|c| OF_is_compatible(faa.fa_node, c)),
    )
}

/// `ampintc_attach`: maps the distributor and CPU interface, resets the controller, takes
/// over `spl` and the IRQ dispatch, and registers with the device tree.
pub fn ampintc_attach(parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    let sc = &AMPINTC;
    // SAFETY: as in `ampintc_match`.
    let faa = unsafe { &*aux.cast::<FdtAttachArgs<'_>>() };

    ATTACHED.store(true, Ordering::Relaxed);
    arm_init_smask();

    sc.sc_iot.set(Some(faa.fa_iot));

    // First row: ICD
    // SAFETY: the device tree's registers of the GIC, which nothing else drives.
    let Ok(d_ioh) = (unsafe {
        bus_space_map(
            faa.fa_iot,
            faa.fa_reg[0].addr as usize,
            faa.fa_reg[0].size as usize,
            0,
        )
    }) else {
        panic(format_args!("ampintc_attach: ICD bus_space_map failed!"));
    };
    sc.sc_d_ioh.set(Some(d_ioh));
    // Second row: ICP
    // SAFETY: as above.
    let Ok(p_ioh) = (unsafe {
        bus_space_map(
            faa.fa_iot,
            faa.fa_reg[1].addr as usize,
            faa.fa_reg[1].size as usize,
            0,
        )
    }) else {
        panic(format_args!("ampintc_attach: ICP bus_space_map failed!"));
    };
    sc.sc_p_ioh.set(Some(p_ioh));

    evcount_attach(&sc.sc_spur, "irq1023/spur", ptr::null());

    let (iot, d, _) = regs();
    let ictr = bus_space_read_4(iot, d, ICD_ICTR);
    let mut nintr = 32 * ((ictr >> ICD_ICTR_ITL_SH) & ICD_ICTR_ITL_M) as i32;
    nintr += 32; // ICD_ICTR + 1, irq 0-31 is SGI, 32+ is PPI
    sc.sc_nintr.set(nintr);
    let ncpu = ((ictr >> ICD_ICTR_CPU_SH) & ICD_ICTR_CPU_M) + 1;
    kprintf!(" nirq {nintr}, ncpu {ncpu}");

    kassert!(curcpu().ci_cpuid.get() <= ICD_ICTR_CPU_M);
    sc.sc_cpu_mask[curcpu().ci_cpuid.get() as usize]
        .store(bus_space_read_1(iot, d, icd_iptrn(0)), Ordering::Relaxed);

    ampintc_init(sc);

    // software reset of the part?
    // set protection bit (kernel only)?
    // XXX - check power saving bit

    let Some(handlers) = malloc(
        nintr as usize * size_of::<Intrq>(),
        M_DEVBUF,
        M_ZERO | M_NOWAIT,
    ) else {
        panic(format_args!(
            "ampintc_attach: no memory for {nintr} handler queues"
        ));
    };
    let handlers = handlers.cast::<Intrq>();
    for i in 0..nintr as usize {
        // SAFETY: a fresh allocation of `nintr` entries, written before use.
        unsafe { handlers.add(i).write(Intrq::new()) };
    }
    sc.sc_handler.set(handlers.as_ptr());
    for i in 0..nintr {
        handler(sc, i).iq_list.init();
    }

    ampintc_setipl(IPL_HIGH); // XXX ???
    ampintc_calc_mask();

    // insert self as interrupt handler
    arm_set_intr_handler(
        ampintc_splraise,
        ampintc_spllower,
        ampintc_splx,
        ampintc_setipl,
        Some(ampintc_irq_handler),
        None,
        None,
        None,
    );

    // setup IPI interrupts
    #[cfg(feature = "multiprocessor")]
    {
        let (iot, d, _) = regs();
        let mut ipiirq = -1;
        for i in 0..16 {
            let oldreg = bus_space_read_1(iot, d, icd_iprn(i));
            bus_space_write_1(iot, d, icd_iprn(i), oldreg ^ 0x20);

            // if this interrupt is not usable, route will be zero
            let reg = bus_space_read_1(iot, d, icd_iprn(i));
            if reg == oldreg {
                continue;
            }

            // return to original value, will be set when used
            bus_space_write_1(iot, d, icd_iprn(i), oldreg);

            ipiirq = i;
            break;
        }

        if ipiirq == -1 {
            panic(format_args!("no irq available for IPI"));
        }

        kprintf!(" ipi {ipiirq}");

        let _ = ampintc_intr_establish(
            ipiirq,
            IST_EDGE_RISING,
            IPL_IPI | IPL_MPSAFE,
            None,
            ampintc_ipi_handler,
            ptr::from_ref(sc).cast_mut().cast(),
            Some("ipi"),
        );
        sc.sc_ipi_num.store(ipiirq, Ordering::Relaxed);
        // SAFETY: the boot CPU, before any application processor runs (the hook's
        // protocol in `arm64/intr.rs`).
        unsafe { INTR_SEND_IPI_FUNC.write(ampintc_send_ipi) };
    }

    // enable interrupts
    let (iot, d, p) = regs();
    bus_space_write_4(iot, d, ICD_DCR, 3);
    bus_space_write_4(iot, p, ICPICR, 1);
    // SAFETY: the controller is set up and every interrupt masked at the distributor.
    unsafe { intr_enable() };

    sc.sc_ic.ic_node.set(faa.fa_node);
    sc.sc_ic.ic_cookie.set(ptr::from_ref(sc).cast::<()>());
    arm_intr_register_fdt(&sc.sc_ic);

    // attach GICv2M frame controller
    simplebus_attach(parent, self_, aux);
}

// ampintc_activate (DVACT_RESUME): M5.

/// `ampintc_init`: disable all interrupts, clear all pending.
fn ampintc_init(sc: &AmpintcSoftc) {
    let (iot, d, _) = regs();
    let nintr = sc.sc_nintr.get();

    for i in 0..nintr / 32 {
        bus_space_write_4(iot, d, icd_icern(i * 32), !0);
        bus_space_write_4(iot, d, icd_icprn(i * 32), !0);
    }
    for i in 0..nintr {
        // lowest priority ??
        bus_space_write_1(iot, d, icd_iprn(i), 0xff);
        // target no cpus
        bus_space_write_1(iot, d, icd_iptrn(i), 0);
    }
    for i in 2..nintr / 16 {
        // irq 32 - N
        bus_space_write_4(iot, d, icd_icrn(i * 16), 0);
    }
}

/// `ampintc_set_priority`: we only use 16 (13 really) interrupt priorities, and a CPU is
/// only required to implement bit 4-7 of each field so shift into the top bits. Also low
/// values are higher priority thus `IPL_HIGH - pri`.
fn ampintc_set_priority(irq: i32, pri: i32) {
    let (iot, d, _) = regs();
    let prival = ((IPL_HIGH - pri) as u32) << ICMIPMR_SH;
    bus_space_write_1(iot, d, icd_iprn(irq), prival as u8);
}

/// `ampintc_setipl`: sets the level in `ci_cpl` and in the CPU interface's priority mask.
pub fn ampintc_setipl(new: i32) {
    let ci = curcpu();
    let (iot, _, p) = regs();

    // disable here is only to keep hardware in sync with ci->ci_cpl
    let psw = intr_disable();
    ci.ci_cpl.set(new as u32);

    // low values are higher priority thus IPL_HIGH - pri
    bus_space_write_4(iot, p, ICPIPMR, ((IPL_HIGH - new) as u32) << ICMIPMR_SH);
    // SAFETY: `psw` is this CPU's DAIF from `intr_disable`.
    unsafe { intr_restore(psw) };
}

/// `ampintc_intr_enable`.
fn ampintc_intr_enable(irq: i32) {
    let (iot, d, _) = regs();
    #[cfg(feature = "debug")]
    kprintf!(
        "enable irq {irq} register {:x} bitmask {:08x}\n",
        icd_isern(irq),
        1u32 << irq_to_reg32bit(irq)
    );
    bus_space_write_4(iot, d, icd_isern(irq), 1 << irq_to_reg32bit(irq));
}

/// `ampintc_intr_disable`.
fn ampintc_intr_disable(irq: i32) {
    let (iot, d, _) = regs();
    bus_space_write_4(iot, d, icd_icern(irq), 1 << irq_to_reg32bit(irq));
}

/// `ampintc_intr_config`: level or rising-edge trigger.
fn ampintc_intr_config(irqno: i32, type_: i32) {
    let (iot, d, _) = regs();

    let mut ctrl = bus_space_read_4(iot, d, icd_icrn(irqno));
    ctrl &= !icd_icr_trig_mask(irqno);
    if type_ == IST_EDGE_RISING {
        ctrl |= icd_icr_trig_edge(irqno);
    } else {
        ctrl |= icd_icr_trig_level(irqno);
    }
    bus_space_write_4(iot, d, icd_icrn(irqno), ctrl);
}

/// `ampintc_calc_mask`: recomputes every interrupt's priority and enable.
pub fn ampintc_calc_mask() {
    let sc = &AMPINTC;
    for irq in 0..sc.sc_nintr.get() {
        ampintc_calc_irq(sc, irq);
    }
}

/// `ampintc_calc_irq`: one interrupt's priority and enable from its handlers.
fn ampintc_calc_irq(sc: &AmpintcSoftc, irq: i32) {
    let iq = handler(sc, irq);
    let ci = iq.iq_ci.get();
    let mut max = IPL_NONE;
    let mut min = IPL_HIGH;

    for ih in iq.iq_list.iter() {
        max = max.max(ih.ih_ipl);
        min = min.min(ih.ih_ipl);
    }

    if max == IPL_NONE {
        min = IPL_NONE;
    }

    if iq.iq_irq_max.get() == max && iq.iq_irq_min.get() == min {
        return;
    }

    iq.iq_irq_max.set(max);
    iq.iq_irq_min.set(min);

    // Enable interrupts at lower levels, clear -> enable
    // Set interrupt priority/enable
    // SAFETY: a queue with handlers names its CPU, a static cpu_info.
    let ci = unsafe { ci.as_ref() };
    if min != IPL_NONE {
        ampintc_set_priority(irq, min);
        ampintc_intr_enable(irq);
        if let Some(ci) = ci {
            ampintc_route(irq, IRQ_ENABLE, ci);
        }
    } else {
        ampintc_intr_disable(irq);
        if let Some(ci) = ci {
            ampintc_route(irq, IRQ_DISABLE, ci);
        }
    }
}

/// `ampintc_splx`.
pub fn ampintc_splx(new: i32) {
    let ci = curcpu();

    if ci.ci_ipending.get() & arm_smask(new) != 0 {
        arm_do_pending_intr(new);
    }

    ampintc_setipl(new);
}

/// `ampintc_spllower`.
pub fn ampintc_spllower(new: i32) -> i32 {
    let ci = curcpu();
    let old = ci.ci_cpl.get() as i32;
    ampintc_splx(new);
    old
}

/// `ampintc_splraise`: `setipl` must always be called because there is a race window where
/// the variable is updated before the mask is set an interrupt occurs in that window
/// without the mask always being set, the hardware might not get updated on the next
/// `splraise` completely messing up spl protection.
pub fn ampintc_splraise(new: i32) -> i32 {
    let ci = curcpu();
    let old = ci.ci_cpl.get() as i32;

    let new = if old > new { old } else { new };
    ampintc_setipl(new);

    old
}

/// `ampintc_iack`: acknowledges the highest pending interrupt.
fn ampintc_iack() -> u32 {
    let (iot, _, p) = regs();
    bus_space_read_4(iot, p, ICPIAR)
}

/// `ampintc_eoi`.
fn ampintc_eoi(eoi: u32) {
    let (iot, _, p) = regs();
    bus_space_write_4(iot, p, ICPEOIR, eoi);
}

/// `ampintc_route`: targets (or untargets) `irq` at `ci`.
fn ampintc_route(irq: i32, enable: bool, ci: &CpuInfo) {
    let sc = &AMPINTC;
    let (iot, d, _) = regs();

    kassert!(ci.ci_cpuid.get() <= ICD_ICTR_CPU_M);
    let mask = sc.sc_cpu_mask[ci.ci_cpuid.get() as usize].load(Ordering::Relaxed);

    let mut val = bus_space_read_1(iot, d, icd_iptrn(irq));
    if enable == IRQ_ENABLE {
        val |= mask;
    } else {
        val &= !mask;
    }
    bus_space_write_1(iot, d, icd_iptrn(irq), val);
}

/// `ampintc_cpuinit`: a CPU learns its target mask and routes its interrupts.
pub fn ampintc_cpuinit() {
    let sc = &AMPINTC;
    let (iot, d, _) = regs();

    // XXX - this is the only cpu specific call to set this
    if sc.sc_cpu_mask[cpu_number() as usize].load(Ordering::Relaxed) == 0 {
        for i in 0..32 {
            let cpumask = bus_space_read_1(iot, d, icd_iptrn(i));
            if cpumask != 0 {
                sc.sc_cpu_mask[cpu_number() as usize].store(cpumask, Ordering::Relaxed);
                break;
            }
        }
    }

    if sc.sc_cpu_mask[cpu_number() as usize].load(Ordering::Relaxed) == 0 {
        panic(format_args!("could not determine cpu target mask"));
    }

    for irq in 0..sc.sc_nintr.get() {
        let iq = handler(sc, irq);
        if !ptr::eq(iq.iq_ci.get(), curcpu()) {
            continue;
        }
        if iq.iq_irq_min.get() != IPL_NONE {
            ampintc_route(irq, IRQ_ENABLE, curcpu());
        } else {
            ampintc_route(irq, IRQ_DISABLE, curcpu());
        }
    }

    // If a secondary CPU is turned off from an IPI handler and the GIC did not go through a
    // full reset (for example when we fail to suspend) the IPI might still be active. So
    // signal EOI here to make sure new interrupts will be serviced.
    ampintc_eoi(sc.sc_ipi_num.load(Ordering::Relaxed) as u32);
}

/// `ampintc_route_irq`: the `ic_route` hook.
fn ampintc_route_irq(v: *mut c_void, enable: bool, ci: &CpuInfo) {
    let sc = &AMPINTC;
    let (iot, d, p) = regs();
    // SAFETY: an established handler of this controller.
    let ih = unsafe { &*v.cast::<Intrhand>() };

    bus_space_write_4(iot, p, ICPICR, 1);
    bus_space_write_4(iot, d, icd_icrn(ih.ih_irq.get()), 0);
    if enable {
        ampintc_set_priority(
            ih.ih_irq.get(),
            handler(sc, ih.ih_irq.get()).iq_irq_min.get(),
        );
        ampintc_intr_enable(ih.ih_irq.get());
    }

    ampintc_route(ih.ih_irq.get(), enable, ci);
}

/// `ampintc_intr_barrier`: the `ic_barrier` hook.
fn ampintc_intr_barrier(cookie: *mut c_void) {
    // SAFETY: the cookie `ampintc_intr_establish` handed out: an established handler, never
    // freed while its driver can still call the barrier.
    let ih = unsafe { &*cookie.cast::<Intrhand>() };
    // SAFETY: `ih_ci` is the cpu_info the handler was established on, which lives forever.
    crate::kern::kern_sched::sched_barrier(unsafe { ih.ih_ci.get().as_ref() });
}

/// `ampintc_run_handler`: one handler, with its argument or the frame.
fn ampintc_run_handler(ih: &Intrhand, frame: *mut c_void, s: i32) {
    // MULTIPROCESSOR: the kernel lock unless the handler is IPL_MPSAFE or the interrupted
    // level is at least IPL_SCHED.
    let need_lock =
        cfg!(feature = "multiprocessor") && ih.ih_flags & IPL_MPSAFE == 0 && s < IPL_SCHED;
    if need_lock {
        kernel_lock();
    }

    let arg = if ih.ih_arg.is_null() {
        frame
    } else {
        ih.ih_arg
    };

    let handled = (ih.ih_func)(arg);
    if handled != 0 {
        ih.ih_count.ec_count.fetch_add(1, Ordering::Relaxed);
    }

    if need_lock {
        kernel_unlock();
    }
}

/// `ampintc_irq_handler`: the IRQ dispatcher `arm_cpu_irq` calls.
pub fn ampintc_irq_handler(frame: &mut Trapframe) {
    let sc = &AMPINTC;

    let iack_val = ampintc_iack();
    // DEBUG_INTC: not configured.
    let irq = (iack_val & ICPIAR_IRQ_M) as i32;
    if irq == 1023 {
        sc.sc_spur.ec_count.fetch_add(1, Ordering::Relaxed);
        return;
    }
    if irq >= sc.sc_nintr.get() {
        return;
    }

    let iq = handler(sc, irq);
    let pri = iq.iq_irq_max.get();
    let s = ampintc_splraise(pri);
    // SAFETY: the level is raised to the interrupt's, so only higher ones nest.
    unsafe { intr_enable() };
    for ih in iq.iq_list.iter() {
        ampintc_run_handler(ih, ptr::from_mut(frame).cast::<c_void>(), s);
    }
    intr_disable();
    ampintc_eoi(iack_val);

    ampintc_splx(s);
}

/// `ampintc_intr_establish_fdt`: the `ic_establish` hook: 1st cell contains type: 0 SPI
/// (32-X), 1 PPI (16-31); 2nd cell contains the interrupt number; 3rd the trigger.
fn ampintc_intr_establish_fdt(
    _cookie: *const (),
    cell: &[u32],
    level: i32,
    ci: Option<&'static CpuInfo>,
    func: IntrFn,
    arg: *mut c_void,
    name: &'static str,
) -> *mut c_void {
    let mut irq = cell[1] as i32;
    if cell[0] == 0 {
        irq += 32;
    } else if cell[0] == 1 {
        irq += 16;
    } else {
        panic(format_args!("ampintc0: bogus interrupt type"));
    }

    // SPIs are only active-high level or low-to-high edge
    let type_ = if cell[2] & 0x3 != 0 {
        IST_EDGE_RISING
    } else {
        IST_LEVEL_HIGH
    };

    match ampintc_intr_establish(irq, type_, level, ci, func, arg, Some(name)) {
        Some(ih) => ih.as_ptr().cast::<c_void>(),
        None => ptr::null_mut(),
    }
}

/// `ampintc_intr_establish`: registers `func(arg)` for `irqno` at `level`.
pub fn ampintc_intr_establish(
    irqno: i32,
    type_: i32,
    level: i32,
    ci: Option<&'static CpuInfo>,
    func: IntrFn,
    arg: *mut c_void,
    name: Option<&'static str>,
) -> Option<NonNull<Intrhand>> {
    let sc = &AMPINTC;

    if irqno < 0 || irqno >= sc.sc_nintr.get() {
        panic(format_args!(
            "ampintc_intr_establish: bogus irqnumber {irqno}: {}",
            name.unwrap_or("")
        ));
    }

    let ci = ci.unwrap_or_else(cpu_info_primary);

    let type_ = if irqno < 16 {
        // SGI are only EDGE
        IST_EDGE_RISING
    } else if irqno < 32 {
        // PPI are only LEVEL
        IST_LEVEL_HIGH
    } else {
        type_
    };

    let ih = malloc(size_of::<Intrhand>(), M_DEVBUF, M_WAITOK)?.cast::<Intrhand>();
    // SAFETY: a fresh allocation of the right size and alignment, written once before use.
    unsafe {
        ih.write(Intrhand {
            ih_list: TailqEntry::new(),
            ih_func: func,
            ih_arg: arg,
            ih_ipl: level & IPL_IRQMASK,
            ih_flags: level & IPL_FLAGMASK,
            ih_irq: Cell::new(irqno),
            ih_count: Evcount::new(),
            ih_name: name,
            ih_ci: Cell::new(ptr::from_ref(ci)),
        });
    }
    // SAFETY: as above; the handler lives until disestablished.
    let hand: &'static Intrhand = unsafe { ih.as_ref() };

    let psw = intr_disable();

    let iq = handler(sc, irqno);
    if !iq.iq_list.is_empty() && !ptr::eq(iq.iq_ci.get(), ci) {
        free(ih.cast::<u8>(), M_DEVBUF, size_of::<Intrhand>());
        // SAFETY: `psw` is this CPU's DAIF from `intr_disable`.
        unsafe { intr_restore(psw) };
        return None;
    }

    // SAFETY: a new handler, not on any queue; interrupts are disabled.
    unsafe { iq.iq_list.insert_tail(hand) };
    iq.iq_ci.set(ptr::from_ref(ci));

    if let Some(name) = name {
        evcount_attach(
            &hand.ih_count,
            name,
            ptr::from_ref(&hand.ih_irq).cast::<()>(),
        );
    }

    #[cfg(feature = "debug")]
    kprintf!(
        "ampintc_intr_establish irq {irqno} level {level} [{}]\n",
        name.unwrap_or("")
    );

    ampintc_intr_config(irqno, type_);
    ampintc_calc_mask();

    // SAFETY: as above.
    unsafe { intr_restore(psw) };
    Some(ih)
}

/// `ampintc_intr_disestablish`: the `ic_disestablish` hook.
///
/// The cookie must come from `ampintc_intr_establish` and is not used afterwards.
fn ampintc_intr_disestablish(cookie: *mut c_void) {
    let sc = &AMPINTC;
    let Some(ih) = NonNull::new(cookie.cast::<Intrhand>()) else {
        return;
    };
    // SAFETY: an established handler (the hook's contract), alive until freed below.
    let hand = unsafe { ih.as_ref() };

    #[cfg(feature = "debug")]
    kprintf!(
        "ampintc_intr_disestablish irq {} level {} [{}]\n",
        hand.ih_irq.get(),
        hand.ih_ipl,
        hand.ih_name.unwrap_or("")
    );

    let psw = intr_disable();

    // SAFETY: the handler is on its interrupt's queue; interrupts are disabled.
    unsafe { handler(sc, hand.ih_irq.get()).iq_list.remove(hand) };
    if hand.ih_name.is_some() {
        evcount_detach(&hand.ih_count);
    }
    ampintc_calc_mask();

    // SAFETY: `psw` is this CPU's DAIF from `intr_disable`.
    unsafe { intr_restore(psw) };

    free(ih.cast::<u8>(), M_DEVBUF, size_of::<Intrhand>());
}

/// `ampintc_msi_match`.
pub fn ampintc_msi_match(_parent: Option<&Device>, _cfdata: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: `ampintcmsi` attaches at `fdt`, whose buses hand over a `FdtAttachArgs`.
    let faa = unsafe { &*aux.cast::<FdtAttachArgs<'_>>() };
    i32::from(OF_is_compatible(faa.fa_node, b"arm,gic-v2m-frame"))
}

/// `ampintc_msi_attach`: maps the frame, finds its SPIs and registers it as an MSI
/// controller.
pub fn ampintc_msi_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: `ampintcmsi_ca` made the device, as an `AmpintcMsiSoftc`.
    let sc = unsafe { self_.softc::<AmpintcMsiSoftc>() };
    // SAFETY: as in `ampintc_msi_match`.
    let faa = unsafe { &*aux.cast::<FdtAttachArgs<'_>>() };

    sc.sc_node.set(faa.fa_node);
    sc.sc_iot.set(Some(faa.fa_iot));
    let Some(reg) = faa.fa_reg.first() else {
        panic(format_args!("ampintc_msi_attach: bus_space_map failed!"));
    };
    // SAFETY: the device tree's registers of the frame, which nothing else drives.
    let Ok(ioh) = (unsafe { bus_space_map(faa.fa_iot, reg.addr as usize, reg.size as usize, 0) })
    else {
        panic(format_args!("ampintc_msi_attach: bus_space_map failed!"));
    };
    sc.sc_ioh.set(Some(ioh));

    // XXX: Hack to retrieve the physical address (from a CPU PoV).
    let Some(pa) = pmap_extract(pmap_kernel(), Vaddr::new(ioh.0)) else {
        kprintf!(": cannot retrieve msi addr\n");
        return;
    };
    sc.sc_addr.set(pa.as_usize());

    let typer = bus_space_read_4(faa.fa_iot, ioh, GICV2M_TYPER);
    sc.sc_bspi.set(gicv2m_typer_spi_base(typer) as i32);
    sc.sc_nspi.set(gicv2m_typer_spi_count(typer) as i32);

    sc.sc_bspi
        .set(OF_getpropint(faa.fa_node, b"arm,msi-base-spi", sc.sc_bspi.get() as u32) as i32);
    sc.sc_nspi
        .set(OF_getpropint(faa.fa_node, b"arm,msi-num-spis", sc.sc_nspi.get() as u32) as i32);

    kprintf!(": nspi {}\n", sc.sc_nspi.get());

    let Some(spi) = mallocarray(
        sc.sc_nspi.get() as usize,
        size_of::<Cell<*mut MachineIntrHandle>>(),
        M_DEVBUF,
        M_WAITOK | M_ZERO,
    ) else {
        panic(format_args!("ampintc_msi_attach: out of memory"));
    };
    sc.sc_spi.set(spi.as_ptr().cast());

    // SAFETY: the controller is not registered yet, so nothing else reads it; it is written
    // once, here.
    let ic = unsafe { &mut *sc.sc_ic.get() };
    ic.ic_node.set(faa.fa_node);
    ic.ic_cookie.set(ptr::from_ref(sc).cast::<()>());
    ic.ic_establish_msi = Some(ampintc_intr_establish_msi);
    ic.ic_disestablish = Some(ampintc_intr_disestablish_msi);
    ic.ic_barrier = Some(ampintc_intr_barrier_msi);
    // SAFETY: the softc lives as long as the kernel (no detach); the controller is not
    // written again.
    arm_intr_register_fdt(unsafe { &*sc.sc_ic.get() });
}

/// `ampintc_intr_establish_msi`: the frame's `ic_establish_msi` hook: takes a free SPI,
/// establishes it on the parent GIC (edge-triggered) and hands back the doorbell and the
/// SPI number as the MSI's address and data. The cookie is the SPI's slot.
#[allow(clippy::too_many_arguments)] // the C's signature
fn ampintc_intr_establish_msi(
    self_: *const (),
    addr: &mut u64,
    data: &mut u64,
    level: i32,
    ci: Option<&'static CpuInfo>,
    func: IntrFn,
    arg: *mut c_void,
    name: &'static str,
) -> *mut c_void {
    // SAFETY: the cookie is the frame's softc (`ampintc_msi_attach`), which lives forever.
    let sc = unsafe { &*self_.cast::<AmpintcMsiSoftc>() };

    let Some(ic) = interrupt_controllers()
        .iter()
        .find(|ic| ic.ic_node.get() == OF_parent(sc.sc_node.get()))
    else {
        return ptr::null_mut();
    };
    let Some(establish) = ic.ic_establish else {
        return ptr::null_mut();
    };

    let mut cells = [0u32; 3];
    cells[0] = 0; // SPI
    cells[2] = 1; // Edge-Rising

    for i in 0..sc.sc_nspi.get() {
        let slot = sc.spi(i);
        if !slot.get().is_null() {
            continue;
        }

        cells[1] = (sc.sc_bspi.get() + i - 32) as u32;
        let cookie = establish(ic.ic_cookie.get(), &cells, level, ci, func, arg, name);
        if cookie.is_null() {
            return ptr::null_mut();
        }

        let Some(ih) = malloc(size_of::<MachineIntrHandle>(), M_DEVBUF, M_WAITOK) else {
            return ptr::null_mut();
        };
        let ih = ih.cast::<MachineIntrHandle>();
        // SAFETY: a fresh allocation, written once before use.
        unsafe {
            ih.write(MachineIntrHandle {
                ih_ic: ptr::from_ref(ic),
                ih_ih: cookie,
            });
        }

        *addr = (sc.sc_addr.get() + GICV2M_SETSPI_NS) as u64;
        *data = (sc.sc_bspi.get() + i) as u64;
        slot.set(ih.as_ptr());
        return ptr::from_ref(slot).cast_mut().cast::<c_void>();
    }

    ptr::null_mut()
}

/// The handle in an MSI's slot (`*(void **)cookie`).
fn msi_slot(cookie: *mut c_void) -> &'static Cell<*mut MachineIntrHandle> {
    // SAFETY: the cookie `ampintc_intr_establish_msi` handed out: a slot of a frame's
    // `sc_spi`, which is never freed.
    unsafe { &*cookie.cast::<Cell<*mut MachineIntrHandle>>() }
}

/// `ampintc_intr_disestablish_msi`: the frame's `ic_disestablish` hook.
fn ampintc_intr_disestablish_msi(cookie: *mut c_void) {
    let slot = msi_slot(cookie);
    if let Some(ih) = NonNull::new(slot.get()) {
        // SAFETY: the handle `ampintc_intr_establish_msi` made, not used after this.
        unsafe { fdt_intr_disestablish(ih) };
    }
    slot.set(ptr::null_mut());
}

/// `ampintc_intr_barrier_msi`: the frame's `ic_barrier` hook.
fn ampintc_intr_barrier_msi(cookie: *mut c_void) {
    if let Some(ih) = NonNull::new(msi_slot(cookie).get()) {
        intr_barrier(ih);
    }
}

/// `ampintc_ipi_ddb`: another CPU entered ddb.
#[cfg(feature = "multiprocessor")]
fn ampintc_ipi_ddb(_v: *mut c_void) -> i32 {
    // XXX
    db_enter();
    1
}

/// `ampintc_ipi_halt`: stop this CPU (`cpu_halt`).
#[cfg(feature = "multiprocessor")]
fn ampintc_ipi_halt(_v: *mut c_void) -> i32 {
    cpu_halt();
    1
}

/// `ampintc_ipi_handler`: the IPI's interrupt handler, at `IPL_IPI`, without the kernel
/// lock: runs the reasons posted for this CPU (`ARM_IPI_NOP` posts none; the interrupt
/// itself was the point).
#[cfg(feature = "multiprocessor")]
fn ampintc_ipi_handler(v: *mut c_void) -> i32 {
    let sc = &AMPINTC;
    let ci = curcpu();
    let reason = &sc.sc_ipi_reason[ci.ci_cpuid.get() as usize];

    let mut reasons = reason.load(Ordering::Relaxed);
    if reasons != 0 {
        reasons = reason.swap(0, Ordering::AcqRel);
        if reasons & (1 << ARM_IPI_DDB) != 0 {
            ampintc_ipi_ddb(v);
        }
        if reasons & (1 << ARM_IPI_HALT) != 0 {
            ampintc_ipi_halt(v);
        }
        // ARM_IPI_XCALL: NXCALL is 0 on arm64 (arm_cpu_xcall_dispatch).
    }

    1
}

/// `ampintc_send_ipi`: `intr_send_ipi_func`: posts `reason` for `ci` and raises the IPI's
/// SGI at it alone.
#[cfg(feature = "multiprocessor")]
fn ampintc_send_ipi(ci: &CpuInfo, reason: i32) {
    let sc = &AMPINTC;
    let (iot, d, _) = regs();

    if reason == ARM_IPI_NOP {
        if ptr::eq(ci, curcpu()) {
            return;
        }
    } else {
        sc.sc_ipi_reason[ci.ci_cpuid.get() as usize].fetch_or(1 << reason, Ordering::Release);
    }

    // currently will only send to one cpu
    let mut sendmask =
        u32::from(sc.sc_cpu_mask[ci.ci_cpuid.get() as usize].load(Ordering::Relaxed)) << 16;
    sendmask |= sc.sc_ipi_num.load(Ordering::Relaxed) as u32;

    // The reason (and whatever the sender prepared) before the interrupt.
    fence(Ordering::SeqCst);
    bus_space_write_4(iot, d, ICD_SGIR, sendmask);
}

/// The number of IPIs the IPI handler has run, on every CPU (its event counter): what the
/// `qemu` self-check of `cpu_boot_secondary_processors` watches.
#[cfg(all(feature = "multiprocessor", feature = "qemu"))]
pub fn ampintc_ipi_count() -> u64 {
    let sc = &AMPINTC;
    handler(sc, sc.sc_ipi_num.load(Ordering::Relaxed))
        .iq_list
        .iter()
        .map(|ih| ih.ih_count.ec_count.load(Ordering::Relaxed))
        .sum()
}

/// Whether the controller has attached.
pub fn ampintc_attached() -> bool {
    ATTACHED.load(Ordering::Relaxed)
}
