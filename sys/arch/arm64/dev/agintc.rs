/* $OpenBSD: agintc.c,v 1.66 2026/09/08 19:48:28 kettenis Exp $ */
/* <LICENSES> */
/*
 * Copyright (c) 2007, 2009, 2011, 2017 Dale Rahn <drahn@dalerahn.com>
 * Copyright (c) 2018 Mark Kettenis <kettenis@openbsd.org>
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
//! The ARM generic interrupt controller, versions 3 and 4: `arch/arm64/dev/agintc.c`. This
//! is a device driver for the GICv3/GICv4 IP from ARM as specified in IHI0069C, an example
//! of this hardware is the GIC 500.
//!
//! Upstream: sys/arch/arm64/dev/agintc.c @ 3ce1f3f79392
//!
//! The whole file (M16f): the distributor (`GICD_*`), the redistributors (`GICR_*`, one per
//! CPU, found by walking the `GICR_TYPER` frames of every redistributor region), the CPU
//! interface through its system registers (`ICC_*_EL1`: the priority mask is the `spl`
//! level, `ICC_IAR1`/`ICC_EOIR1` acknowledge and end an interrupt), the SGIs that carry the
//! IPIs (`ICC_SGI1R`), the LPIs with their configuration and pending tables, the message
//! based SPIs (`mbi-ranges`), `agintc_activate`/`agintc_restore` and the wakeup hooks; and
//! the ITS (`agintcmsi* at fdt? early 1`, `arm,gic-v3-its`): its command queue, its device
//! and collection tables (flat or indirect), one collection per CPU, and the MSIs it
//! translates into LPIs (`MAPD`, `MAPTI`, `INV`, `DISCARD`, `SYNC`).
//!
//! ## Deviations
//! - One static softc, `AGINTC`, stands for the C's `agintc_sc` and the rest of
//!   `struct agintc_softc`, as `ampintc.rs` does: `agintc_ca`'s `ca_devsize` is the
//!   `struct simplebus_softc` that heads the C's softc (`sc_sbus`, which `simplebus_attach`
//!   fills for the ITS below the GIC). Its `sc_ic` is an `UnsafeCell`, written once by
//!   `agintc_attach` before it registers it (whether `ic_establish_msi` is set depends on
//!   the MBI ranges). `sc_ncells` is not kept: nothing in the C reads or writes it.
//! - `sc_cpuremap`, `sc_ipi_reason`, `sc_ipi_num` and `sc_ipi_irq` are atomics (the
//!   application processors read them in `agintc_cpuinit` and their IPI handler while others
//!   send IPIs); the other members are written at attach, on the boot CPU, before any other
//!   CPU runs, or under the kernel lock with interrupts disabled, as in C.
//! - The arrays the C `mallocarray`s (`sc_rbase_ioh`, `sc_r_ioh`, `sc_processor`,
//!   `sc_handler`, `sc_lpi`, an MBI range's slots) are still `malloc(9)`ed, reached through
//!   accessors that check the index; an LPI's and an MBI's slot is a `Cell` of a raw
//!   pointer, and an MSI's cookie is the address of its slot, as in C.
//! - An ITS command is built as its four little-endian double words (`GitsCmd::words`) and
//!   written to the queue with volatile stores: the C `memcpy`s a `struct gits_cmd` whose
//!   padding its `memset` zeroed; the bytes are the same.
//! - `agintc_send_ipi` puts a `dsb ish` before the `ICC_SGI1R` write and an `isb` after it,
//!   so the posted reason is visible to the target before the SGI is (a system register
//!   write is not ordered by the C's atomic's `dmb`).
//! - `agintc_attach`'s failure path (`unmap`) restores the interrupt state it disabled; the
//!   C returns with interrupts still disabled.
//! - `agintc_msi_create_device` returns no device when its ITT cannot be allocated (the C
//!   would hand the ITS a NULL table); `agintc_intr_establish_msi`'s event ID check takes a
//!   shift past 31 as a free bit set (the C's `1U << eventid` is undefined there).
//! - `ffs`/`fls` (libkern) are local helpers until libkern ports them.
//! - `agintc_ipi_count` and `agintc_attached` (feature `qemu`, the self-check in
//!   `cpu_boot_secondary_processors`) are not in the C.

use core::cell::{Cell, UnsafeCell};
use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicPtr, AtomicU32, Ordering};

#[cfg(feature = "multiprocessor")]
use crate::arch::arm64::arm64::cpu::cpu_halt;
use crate::arch::arm64::arm64::cpufunc::cpu_dcache_wb_range;
#[cfg(feature = "multiprocessor")]
use crate::arch::arm64::arm64::db_interface::db_enter;
#[cfg(feature = "multiprocessor")]
use crate::arch::arm64::arm64::intr::INTR_SEND_IPI_FUNC;
use crate::arch::arm64::arm64::intr::{
    arm_do_pending_intr, arm_init_smask, arm_intr_register_fdt, arm_set_intr_handler, arm_smask,
    delay,
};
use crate::arch::arm64::arm64::machdep::cpu_info_list;
use crate::arch::arm64::dev::simplebus::simplebus_attach;
use crate::arch::arm64::include::armreg::{MPIDR_AFF, icc_ctlr_el1_pribits};
#[cfg(any(feature = "multiprocessor", test))]
use crate::arch::arm64::include::armreg::{MPIDR_AFF1, MPIDR_AFF2, MPIDR_AFF3};
use crate::arch::arm64::include::bus::{bus_space_read_8, bus_space_write_8};
use crate::arch::arm64::include::cpu::{
    CpuInfo, MAXCPUS, cpu_info_primary, cpu_number, curcpu, intr_disable, intr_enable, intr_restore,
};
use crate::arch::arm64::include::fdt::FdtAttachArgs;
use crate::arch::arm64::include::frame::Trapframe;
#[cfg(feature = "multiprocessor")]
use crate::arch::arm64::include::intr::{ARM_IPI_DDB, ARM_IPI_HALT, ARM_IPI_NOP, IPL_IPI};
use crate::arch::arm64::include::intr::{
    IPL_FLAGMASK, IPL_HIGH, IPL_IRQMASK, IPL_MPSAFE, IPL_NONE, IPL_SCHED, IPL_WAKEUP,
    IST_EDGE_RISING, IST_LEVEL_HIGH, InterruptController, IntrFn,
};
use crate::arch::arm64::include::simplebusvar::SimplebusSoftc;
use crate::dev::ofw::openfirm::{
    OF_getpropint, OF_getpropint64, OF_getpropintarray, OF_getproplen, OF_is_compatible, OF_peer,
};
use crate::kassert;
use crate::kern::kern_malloc::{free, malloc, mallocarray};
use crate::kern::subr_autoconf::config_activate_children;
use crate::kern::subr_evcount::{evcount_attach, evcount_detach};
use crate::kern::subr_prf::panic;
use crate::kprintf;
#[cfg(feature = "multiprocessor")]
use crate::machine::bus::bus_space_read_1;
use crate::machine::bus::{
    BUS_DMA_ALLOCNOW, BUS_DMA_NOCACHE, BUS_DMA_WAITOK, BUS_DMA_ZERO, BusDmaSegment, BusDmaTag,
    BusDmamap, BusSpaceHandle, BusSpaceTag, bus_dmamap_create, bus_dmamap_destroy,
    bus_dmamap_load_raw, bus_dmamem_alloc, bus_dmamem_free, bus_dmamem_map, bus_dmamem_unmap,
    bus_space_map, bus_space_read_4, bus_space_subregion, bus_space_unmap, bus_space_write_1,
    bus_space_write_4,
};
use crate::queue_adapter;
use crate::sys::device::{CfMatch, Cfattach, Cfdriver, DV_DULL, DVACT_RESUME, Device, Softc};
use crate::sys::errno::Errno;
use crate::sys::evcount::Evcount;
use crate::sys::malloc::{M_DEVBUF, M_TEMP, M_WAITOK, M_ZERO};
use crate::sys::param::PAGE_SIZE;
use crate::sys::queue::{ListEntry, ListHead, TailqEntry, TailqHead};
use crate::sys::systm::{kernel_lock, kernel_unlock};

/// The CPU interface's system registers, by the C's names (`s3_0_c4_c6_0` is `ICC_PMR_EL1`,
/// ...), as `mrs`/`msr` operands.
macro_rules! icc_reg {
    (ICC_PMR) => {
        "s3_0_c4_c6_0"
    };
    (ICC_IAR1) => {
        "s3_0_c12_c12_0"
    };
    (ICC_EOIR1) => {
        "s3_0_c12_c12_1"
    };
    (ICC_BPR1) => {
        "s3_0_c12_c12_3"
    };
    (ICC_CTLR_EL1) => {
        "s3_0_c12_c12_4"
    };
    (ICC_SRE_EL1) => {
        "s3_0_c12_c12_5"
    };
    (ICC_IGRPEN1) => {
        "s3_0_c12_c12_7"
    };
    (ICC_SGI1R) => {
        "s3_0_c12_c11_5"
    };
}

/// `ICC_SRE_EL1_EN`: `SRE`, `DFB` and `DIB`.
pub const ICC_SRE_EL1_EN: u64 = 0x7;

/* distributor registers */
/// `GICD_CTLR`.
pub const GICD_CTLR: usize = 0x0000;
/// `GICD_CTLR_RWP` (non-secure).
pub const GICD_CTLR_RWP: u32 = 1 << 31;
/// `GICD_CTLR_EnableGrp1`.
pub const GICD_CTLR_ENABLE_GRP1: u32 = 1 << 0;
/// `GICD_CTLR_EnableGrp1A`.
pub const GICD_CTLR_ENABLE_GRP1A: u32 = 1 << 1;
/// `GICD_CTLR_ARE_NS`.
pub const GICD_CTLR_ARE_NS: u32 = 1 << 4;
/// `GICD_CTLR_DS`: one security state.
pub const GICD_CTLR_DS: u32 = 1 << 6;
/// `GICD_TYPER`.
pub const GICD_TYPER: usize = 0x0004;
/// `GICD_TYPER_MBIS`: message based SPIs.
pub const GICD_TYPER_MBIS: u32 = 1 << 16;
/// `GICD_TYPER_LPIS`.
pub const GICD_TYPER_LPIS: u32 = 1 << 17;
/// `GICD_TYPER_ITLINE_M`.
pub const GICD_TYPER_ITLINE_M: u32 = 0x1f;
/// `GICD_IIDR`.
pub const GICD_IIDR: usize = 0x0008;
/// `GICD_SETSPI_NSR`.
pub const GICD_SETSPI_NSR: usize = 0x0040;
/// `GICD_CLRSPI_NSR`.
pub const GICD_CLRSPI_NSR: usize = 0x0048;

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

/// `GICD_IGROUPR(i)`.
pub const fn gicd_igroupr(i: i32) -> usize {
    0x0080 + irq_to_reg32(i) * 4
}

/// `GICD_ISENABLER(i)`.
pub const fn gicd_isenabler(i: i32) -> usize {
    0x0100 + irq_to_reg32(i) * 4
}

/// `GICD_ICENABLER(i)`.
pub const fn gicd_icenabler(i: i32) -> usize {
    0x0180 + irq_to_reg32(i) * 4
}

/// `GICD_ISPENDR(i)`.
pub const fn gicd_ispendr(i: i32) -> usize {
    0x0200 + irq_to_reg32(i) * 4
}

/// `GICD_ICPENDR(i)`.
pub const fn gicd_icpendr(i: i32) -> usize {
    0x0280 + irq_to_reg32(i) * 4
}

/// `GICD_ISACTIVER(i)`.
pub const fn gicd_isactiver(i: i32) -> usize {
    0x0300 + irq_to_reg32(i) * 4
}

/// `GICD_ICACTIVER(i)`.
pub const fn gicd_icactiver(i: i32) -> usize {
    0x0380 + irq_to_reg32(i) * 4
}

/// `GICD_IPRIORITYR(i)`: one byte per interrupt.
pub const fn gicd_ipriorityr(i: i32) -> usize {
    0x0400 + i as usize
}

/// `GICD_ICFGR(i)`.
pub const fn gicd_icfgr(i: i32) -> usize {
    0x0c00 + irq_to_reg16(i) * 4
}

/// `GICD_ICFGR_TRIG_LEVEL(i)`.
pub const fn gicd_icfgr_trig_level(i: i32) -> u32 {
    0x0 << (irq_to_reg16bit(i) * 2)
}

/// `GICD_ICFGR_TRIG_EDGE(i)`.
pub const fn gicd_icfgr_trig_edge(i: i32) -> u32 {
    0x2 << (irq_to_reg16bit(i) * 2)
}

/// `GICD_ICFGR_TRIG_MASK(i)`.
pub const fn gicd_icfgr_trig_mask(i: i32) -> u32 {
    0x2 << (irq_to_reg16bit(i) * 2)
}

/// `GICD_IGRPMODR(i)`.
pub const fn gicd_igrpmodr(i: i32) -> usize {
    0x0d00 + irq_to_reg32(i) * 4
}

/// `GICD_NSACR(i)`.
pub const fn gicd_nsacr(i: i32) -> usize {
    0x0e00 + irq_to_reg16(i) * 4
}

/// `GICD_IROUTER(i)`: the affinity an SPI is routed to.
pub const fn gicd_irouter(i: i32) -> usize {
    0x6000 + i as usize * 8
}

/* redistributor registers */
/// `GICR_CTLR`.
pub const GICR_CTLR: usize = 0x00000;
/// `GICR_CTLR_RWP`.
pub const GICR_CTLR_RWP: u32 = (1 << 31) | (1 << 3);
/// `GICR_CTLR_ENABLE_LPIS`.
pub const GICR_CTLR_ENABLE_LPIS: u32 = 1 << 0;
/// `GICR_IIDR`.
pub const GICR_IIDR: usize = 0x00004;
/// `GICR_TYPER`: 64 bits; the affinity in 63:32, the processor number in 23:8.
pub const GICR_TYPER: usize = 0x00008;
/// `GICR_TYPER_LAST`: the last redistributor of its region.
pub const GICR_TYPER_LAST: u64 = 1 << 4;
/// `GICR_TYPER_VLPIS`: two more 64 KB frames for virtual LPIs.
pub const GICR_TYPER_VLPIS: u64 = 1 << 1;
/// `GICR_WAKER`.
pub const GICR_WAKER: usize = 0x00014;
/// `GICR_WAKER_X31`.
pub const GICR_WAKER_X31: u32 = 1 << 31;
/// `GICR_WAKER_CHILDRENASLEEP`.
pub const GICR_WAKER_CHILDRENASLEEP: u32 = 1 << 2;
/// `GICR_WAKER_PROCESSORSLEEP`.
pub const GICR_WAKER_PROCESSORSLEEP: u32 = 1 << 1;
/// `GICR_WAKER_X0`.
pub const GICR_WAKER_X0: u32 = 1 << 0;
/// `GICR_PROPBASER`.
pub const GICR_PROPBASER: usize = 0x00070;
/// `GICR_PROPBASER_ISH`.
pub const GICR_PROPBASER_ISH: u64 = 1 << 10;
/// `GICR_PROPBASER_IC_NORM_NC`.
pub const GICR_PROPBASER_IC_NORM_NC: u64 = 1 << 7;
/// `GICR_PENDBASER`.
pub const GICR_PENDBASER: usize = 0x00078;
/// `GICR_PENDBASER_PTZ`: the pending table is zeroed.
pub const GICR_PENDBASER_PTZ: u64 = 1 << 62;
/// `GICR_PENDBASER_ISH`.
pub const GICR_PENDBASER_ISH: u64 = 1 << 10;
/// `GICR_PENDBASER_IC_NORM_NC`.
pub const GICR_PENDBASER_IC_NORM_NC: u64 = 1 << 7;
/// `GICR_IGROUPR0`.
pub const GICR_IGROUPR0: usize = 0x10080;
/// `GICR_ISENABLE0`.
pub const GICR_ISENABLE0: usize = 0x10100;
/// `GICR_ICENABLE0`.
pub const GICR_ICENABLE0: usize = 0x10180;
/// `GICR_ISPENDR0`.
pub const GICR_ISPENDR0: usize = 0x10200;
/// `GICR_ICPENDR0`.
pub const GICR_ICPENDR0: usize = 0x10280;
/// `GICR_ISACTIVE0`.
pub const GICR_ISACTIVE0: usize = 0x10300;
/// `GICR_ICACTIVE0`.
pub const GICR_ICACTIVE0: usize = 0x10380;

/// `GICR_IPRIORITYR(i)`: one byte per SGI or PPI.
pub const fn gicr_ipriorityr(i: i32) -> usize {
    0x10400 + i as usize
}

/// `GICR_ICFGR0`.
pub const GICR_ICFGR0: usize = 0x10c00;
/// `GICR_ICFGR1`.
pub const GICR_ICFGR1: usize = 0x10c04;
/// `GICR_IGRPMODR0`.
pub const GICR_IGRPMODR0: usize = 0x10d00;

/// `GICR_PROP_SIZE`: the LPI configuration table, one byte per LPI.
pub const GICR_PROP_SIZE: usize = 64 * 1024;
/// `GICR_PROP_GROUP1`.
pub const GICR_PROP_GROUP1: u8 = 1 << 1;
/// `GICR_PROP_ENABLE`.
pub const GICR_PROP_ENABLE: u8 = 1 << 0;
/// `GICR_PEND_SIZE`: the LPI pending table.
pub const GICR_PEND_SIZE: usize = 64 * 1024;

/// `PPI_BASE`.
pub const PPI_BASE: i32 = 16;
/// `SPI_BASE`.
pub const SPI_BASE: i32 = 32;
/// `LPI_BASE`.
pub const LPI_BASE: i32 = 8192;

/// `IRQ_ENABLE`.
pub const IRQ_ENABLE: bool = true;
/// `IRQ_DISABLE`.
pub const IRQ_DISABLE: bool = false;

/*
 * GICv3 ITS controller for MSI interrupts.
 */
/// `GITS_CTLR`.
pub const GITS_CTLR: usize = 0x0000;
/// `GITS_CTLR_ENABLED`.
pub const GITS_CTLR_ENABLED: u32 = 1 << 0;
/// `GITS_TYPER`.
pub const GITS_TYPER: usize = 0x0008;
/// `GITS_TYPER_CIL`: `CIDbits` is valid.
pub const GITS_TYPER_CIL: u64 = 1 << 36;

/// `GITS_TYPER_CIDBITS(x)`.
pub const fn gits_typer_cidbits(x: u64) -> u64 {
    (x >> 32) & 0xf
}

/// `GITS_TYPER_HCC(x)`.
pub const fn gits_typer_hcc(x: u64) -> u64 {
    (x >> 24) & 0xff
}

/// `GITS_TYPER_PTA`: collections target physical addresses.
pub const GITS_TYPER_PTA: u64 = 1 << 19;

/// `GITS_TYPER_DEVBITS(x)`.
pub const fn gits_typer_devbits(x: u64) -> u64 {
    (x >> 13) & 0x1f
}

/// `GITS_TYPER_ITE_SZ(x)`.
pub const fn gits_typer_ite_sz(x: u64) -> u64 {
    (x >> 4) & 0xf
}

/// `GITS_TYPER_PHYS`: physical LPIs.
pub const GITS_TYPER_PHYS: u64 = 1 << 0;
/// `GITS_CBASER`.
pub const GITS_CBASER: usize = 0x0080;
/// `GITS_CBASER_VALID`.
pub const GITS_CBASER_VALID: u64 = 1 << 63;
/// `GITS_CBASER_IC_NORM_NC`.
pub const GITS_CBASER_IC_NORM_NC: u64 = 1 << 59;
/// `GITS_CBASER_MASK`.
pub const GITS_CBASER_MASK: u64 = 0x1f_ffff_ffff_f000;
/// `GITS_CWRITER`.
pub const GITS_CWRITER: usize = 0x0088;
/// `GITS_CREADR`.
pub const GITS_CREADR: usize = 0x0090;

/// `GITS_BASER(i)`.
pub const fn gits_baser(i: usize) -> usize {
    0x0100 + i * 8
}

/// `GITS_BASER_VALID`.
pub const GITS_BASER_VALID: u64 = 1 << 63;
/// `GITS_BASER_INDIRECT`.
pub const GITS_BASER_INDIRECT: u64 = 1 << 62;
/// `GITS_BASER_IC_NORM_NC`.
pub const GITS_BASER_IC_NORM_NC: u64 = 1 << 59;
/// `GITS_BASER_TYPE_MASK`.
pub const GITS_BASER_TYPE_MASK: u64 = 7 << 56;
/// `GITS_BASER_TYPE_DEVICE`.
pub const GITS_BASER_TYPE_DEVICE: u64 = 1 << 56;
/// `GITS_BASER_TYPE_COLL`.
pub const GITS_BASER_TYPE_COLL: u64 = 4 << 56;

/// `GITS_BASER_TTE_SZ(x)`: the size of a table entry, minus one.
pub const fn gits_baser_tte_sz(x: u64) -> u64 {
    (x >> 48) & 0x1f
}

/// `GITS_BASER_PGSZ_MASK`.
pub const GITS_BASER_PGSZ_MASK: u64 = 3 << 8;
/// `GITS_BASER_PGSZ_4K`.
pub const GITS_BASER_PGSZ_4K: u64 = 0 << 8;
/// `GITS_BASER_PGSZ_16K`.
pub const GITS_BASER_PGSZ_16K: u64 = 1 << 8;
/// `GITS_BASER_PGSZ_64K`.
pub const GITS_BASER_PGSZ_64K: u64 = 2 << 8;
/// `GITS_BASER_SZ_MASK`: the number of pages, minus one.
pub const GITS_BASER_SZ_MASK: u64 = 0xff;
/// `GITS_BASER_PA_MASK`.
pub const GITS_BASER_PA_MASK: u64 = 0x7fff_ffff_f000;
/// `GITS_TRANSLATER`: the doorbell a device writes its event ID to.
pub const GITS_TRANSLATER: usize = 0x10040;

/// `GITS_NUM_BASER`.
pub const GITS_NUM_BASER: usize = 8;

/// `GITS_CMD_VALID`.
pub const GITS_CMD_VALID: u64 = 1 << 63;

/* ITS commands */
/// `SYNC`.
pub const SYNC: u8 = 0x05;
/// `MAPD`.
pub const MAPD: u8 = 0x08;
/// `MAPC`.
pub const MAPC: u8 = 0x09;
/// `MAPTI`.
pub const MAPTI: u8 = 0x0a;
/// `INV`.
pub const INV: u8 = 0x0c;
/// `INVALL`.
pub const INVALL: u8 = 0x0d;
/// `DISCARD`.
pub const DISCARD: u8 = 0x0f;

/// `GITS_CMDQ_SIZE`.
pub const GITS_CMDQ_SIZE: usize = 64 * 1024;
/// `GITS_CMDQ_NENTRIES`: the queue's 32-byte commands.
pub const GITS_CMDQ_NENTRIES: usize = GITS_CMDQ_SIZE / GITS_CMD_SIZE;
/// `sizeof(struct gits_cmd)`.
pub const GITS_CMD_SIZE: usize = 32;

/// `CPU_IMPL(midr)`.
pub const fn cpu_impl(midr: u64) -> u64 {
    (midr >> 24) & 0xff
}

/// `CPU_PART(midr)`.
pub const fn cpu_part(midr: u64) -> u64 {
    (midr >> 4) & 0xfff
}

/// `CPU_IMPL_QCOM`.
pub const CPU_IMPL_QCOM: u64 = 0x51;
/// `CPU_PART_ORYON`.
pub const CPU_PART_ORYON: u64 = 0x001;

/// `struct agintc_mbi_range`: a range of message based SPIs.
pub struct AgintcMbiRange {
    /// `mr_base`: the first SPI.
    pub mr_base: i32,
    /// `mr_span`: how many.
    pub mr_span: i32,
    /// `mr_mbi`: per SPI, the handler established on it (NULL: free); `mr_span` zeroed
    /// slots.
    pub mr_mbi: *mut Cell<*mut c_void>,
}

impl AgintcMbiRange {
    /// `&mr->mr_mbi[j]`.
    fn slot(&self, j: i32) -> &Cell<*mut c_void> {
        if self.mr_mbi.is_null() || j < 0 || j >= self.mr_span {
            panic(format_args!("agintc: bad mbi slot {j}"));
        }
        // SAFETY: `agintc_mbiinit` allocated `mr_span` zeroed slots (valid all-zero), never
        // freed.
        unsafe { &*self.mr_mbi.add(j as usize) }
    }
}

/// `struct agintc_lpi_info`: what an LPI an ITS translates an MSI into is for.
pub struct AgintcLpiInfo {
    /// `li_msic`: the ITS.
    pub li_msic: *const AgintcMsiSoftc,
    /// `li_ci`: the CPU it is routed to.
    pub li_ci: &'static CpuInfo,
    /// `li_deviceid`.
    pub li_deviceid: u32,
    /// `li_eventid`.
    pub li_eventid: u32,
    /// `li_ih`: its handler.
    pub li_ih: *mut Intrhand,
}

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
    /// `ih_flags`; `IPL_WAKEUP` is set later by `agintc_intr_set_wakeup`.
    pub ih_flags: Cell<i32>,
    /// `ih_type`: trigger type.
    pub ih_type: i32,
    /// `ih_irq`: IRQ number.
    pub ih_irq: i32,
    /// `ih_count`.
    pub ih_count: Evcount,
    /// `ih_name`.
    pub ih_name: Option<&'static str>,
    /// `ih_ci`: CPU the IRQ runs on.
    pub ih_ci: &'static CpuInfo,
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
    /// `iq_route`.
    pub iq_route: Cell<i32>,
}

impl Intrq {
    const fn new() -> Self {
        Self {
            iq_list: TailqHead::new(),
            iq_ci: Cell::new(ptr::null()),
            iq_irq_max: Cell::new(0),
            iq_irq_min: Cell::new(0),
            iq_ist: Cell::new(0),
            iq_route: Cell::new(0),
        }
    }
}

/// `struct agintc_dmamem`: a physically contiguous, uncached DMA area the GIC reads.
pub struct AgintcDmamem {
    /// `adm_map`.
    pub adm_map: &'static BusDmamap,
    /// `adm_seg`.
    pub adm_seg: BusDmaSegment,
    /// `adm_size`.
    pub adm_size: usize,
    /// `adm_kva`.
    pub adm_kva: NonNull<u8>,
}

impl AgintcDmamem {
    /// `AGINTC_DMA_DVA(_adm)`: the device address of the area.
    pub fn dva(&self) -> u64 {
        self.adm_map.dm_segs()[0].get().ds_addr as u64
    }

    /// `AGINTC_DMA_KVA(_adm)`.
    pub fn kva(&self) -> *mut u8 {
        self.adm_kva.as_ptr()
    }

    /// Byte `i` of the area, written so the GIC sees it once the cache is cleaned.
    fn set_u8(&self, i: usize, v: u8) {
        kassert!(i < self.adm_size);
        // SAFETY: `i` is inside the area `agintc_dmamem_alloc` mapped, which lives until
        // `agintc_dmamem_free`; the GIC reads it, so the store is volatile.
        unsafe { self.kva().add(i).write_volatile(v) };
    }

    /// Byte `i` of the area.
    fn get_u8(&self, i: usize) -> u8 {
        kassert!(i < self.adm_size);
        // SAFETY: as in `set_u8`.
        unsafe { self.kva().add(i).read_volatile() }
    }

    /// The 64-bit word `i` of the area.
    fn get_u64(&self, i: usize) -> u64 {
        kassert!((i + 1) * 8 <= self.adm_size);
        // SAFETY: as in `set_u8`; the area is page aligned, so the word is aligned.
        unsafe { self.kva().cast::<u64>().add(i).read_volatile() }
    }

    /// Writes the 64-bit word `i` of the area.
    fn set_u64(&self, i: usize, v: u64) {
        kassert!((i + 1) * 8 <= self.adm_size);
        // SAFETY: as in `get_u64`.
        unsafe { self.kva().cast::<u64>().add(i).write_volatile(v) };
    }
}

/// `struct agintc_softc`.
pub struct AgintcSoftc {
    // sc_sbus (struct simplebus_softc): the device `agintc_ca` makes.
    /// The device, for its name.
    pub sc_dev: Cell<*const Device>,
    /// `sc_handler`: one `intrq` per interrupt, `sc_nintr` of them.
    pub sc_handler: Cell<*mut Intrq>,
    /// `sc_lpi`: per LPI, what it is for (NULL: free); `sc_nlpi` slots.
    pub sc_lpi: Cell<*mut Cell<*mut AgintcLpiInfo>>,
    /// `sc_iot`.
    pub sc_iot: Cell<Option<BusSpaceTag>>,
    /// `sc_d_ioh`: the distributor.
    pub sc_d_ioh: Cell<Option<BusSpaceHandle>>,
    /// `sc_r_ioh`: each redistributor's frames, `sc_num_redist` of them.
    pub sc_r_ioh: Cell<*mut BusSpaceHandle>,
    /// `sc_rbase_ioh`: each redistributor region, `sc_num_redist_regions` of them.
    pub sc_rbase_ioh: Cell<*mut BusSpaceHandle>,
    /// `sc_dmat`.
    pub sc_dmat: Cell<Option<BusDmaTag>>,
    /// `sc_processor`: each redistributor's processor number, `sc_num_redist` of them.
    pub sc_processor: Cell<*mut u16>,
    /// `sc_cpuremap`: per CPU, the index of its redistributor.
    pub sc_cpuremap: [AtomicI32; MAXCPUS as usize],
    /// `sc_nintr`.
    pub sc_nintr: Cell<i32>,
    /// `sc_nlpi`.
    pub sc_nlpi: Cell<i32>,
    /// `sc_mbi_addr`: the doorbell of the message based SPIs.
    pub sc_mbi_addr: Cell<u64>,
    /// `sc_mbi_nranges`.
    pub sc_mbi_nranges: Cell<i32>,
    /// `sc_mbi_ranges`.
    pub sc_mbi_ranges: Cell<*mut AgintcMbiRange>,
    /// `sc_prio_shift`: how far an interrupt's priority is shifted.
    pub sc_prio_shift: Cell<i32>,
    /// `sc_pmr_shift`: how far the priority mask is shifted.
    pub sc_pmr_shift: Cell<i32>,
    /// `sc_rk3399_quirk`: the distributor is accessed as secure.
    pub sc_rk3399_quirk: Cell<bool>,
    /// `sc_spur`: the spurious interrupt counter.
    pub sc_spur: Evcount,
    /// `sc_num_redist`.
    pub sc_num_redist: Cell<i32>,
    /// `sc_num_redist_regions`.
    pub sc_num_redist_regions: Cell<i32>,
    /// `sc_prop`: the LPI configuration table.
    pub sc_prop: Cell<Option<&'static AgintcDmamem>>,
    /// `sc_pend`: the LPI pending table.
    pub sc_pend: Cell<Option<&'static AgintcDmamem>>,
    /// `sc_ic`: the registered interrupt controller; written once by `agintc_attach` before
    /// it registers it, read-only afterwards.
    pub sc_ic: UnsafeCell<InterruptController>,
    /// `sc_ipi_num`: id for ipi.
    pub sc_ipi_num: AtomicI32,
    /// `sc_ipi_reason`: cause of ipi, per CPU.
    pub sc_ipi_reason: [AtomicU32; MAXCPUS as usize],
    /// `sc_ipi_irq`: ipi irqhandle.
    pub sc_ipi_irq: AtomicPtr<Intrhand>,
}

// SAFETY: the one controller, attached on the boot CPU before any other CPU runs; the cells
// are written at attach time, or at establish time with interrupts disabled under the kernel
// lock, as in C; what the other CPUs touch while running is atomic.
unsafe impl Sync for AgintcSoftc {}

/// `struct gits_cmd`: one ITS command.
#[derive(Clone, Copy, Default)]
pub struct GitsCmd {
    /// `cmd`.
    pub cmd: u8,
    /// `deviceid`.
    pub deviceid: u32,
    /// `eventid`.
    pub eventid: u32,
    /// `intid`.
    pub intid: u32,
    /// `dw2`.
    pub dw2: u64,
    /// `dw3`.
    pub dw3: u64,
}

impl GitsCmd {
    /// The command's four double words, as the C's `struct gits_cmd` lays them out in
    /// memory: `cmd` in byte 0, `deviceid` in bytes 4-7, `eventid` and `intid` in the second
    /// double word.
    pub const fn words(&self) -> [u64; 4] {
        [
            self.cmd as u64 | ((self.deviceid as u64) << 32),
            self.eventid as u64 | ((self.intid as u64) << 32),
            self.dw2,
            self.dw3,
        ]
    }
}

/// `struct agintc_msi_device`: a device ID the ITS has a translation table for.
pub struct AgintcMsiDevice {
    /// `md_list`.
    pub md_list: ListEntry<AgintcMsiDevice>,
    /// `md_deviceid`.
    pub md_deviceid: u32,
    /// `md_events`: the event IDs in use, one bit each.
    pub md_events: Cell<u32>,
    /// `md_itt`: its interrupt translation table.
    pub md_itt: &'static AgintcDmamem,
}

queue_adapter!(
    /// `LIST_HEAD(, agintc_msi_device) sc_msi_devices`.
    pub MdList: AgintcMsiDevice, md_list => ListEntry<AgintcMsiDevice>
);

/// `struct agintc_msi_softc`: an ITS.
#[repr(C)]
pub struct AgintcMsiSoftc {
    /// `sc_dev`.
    pub sc_dev: Device,
    /// `sc_iot`.
    pub sc_iot: Cell<Option<BusSpaceTag>>,
    /// `sc_ioh`.
    pub sc_ioh: Cell<Option<BusSpaceHandle>>,
    /// `sc_dmat`.
    pub sc_dmat: Cell<Option<BusDmaTag>>,
    /// `sc_msi_addr`: the doorbell.
    pub sc_msi_addr: Cell<u64>,
    /// `sc_msi_delta`: per device ID, how far the doorbell moves (Synquacer's pre-ITS).
    pub sc_msi_delta: Cell<i32>,
    /// `sc_cmdq`.
    pub sc_cmdq: Cell<Option<&'static AgintcDmamem>>,
    /// `sc_cmdidx`.
    pub sc_cmdidx: Cell<u16>,
    /// `sc_devbits`.
    pub sc_devbits: Cell<i32>,
    /// `sc_deviceid_max`.
    pub sc_deviceid_max: Cell<u32>,
    /// `sc_dtt`: the device translation table.
    pub sc_dtt: Cell<Option<&'static AgintcDmamem>>,
    /// `sc_dtt_pgsz`.
    pub sc_dtt_pgsz: Cell<usize>,
    /// `sc_dte_sz`.
    pub sc_dte_sz: Cell<u8>,
    /// `sc_dtt_indirect`.
    pub sc_dtt_indirect: Cell<bool>,
    /// `sc_cidbits`.
    pub sc_cidbits: Cell<i32>,
    /// `sc_ctt`: the collection translation table.
    pub sc_ctt: Cell<Option<&'static AgintcDmamem>>,
    /// `sc_ctt_pgsz`.
    pub sc_ctt_pgsz: Cell<usize>,
    /// `sc_cte_sz`.
    pub sc_cte_sz: Cell<u8>,
    /// `sc_ite_sz`.
    pub sc_ite_sz: Cell<u8>,
    /// `sc_msi_devices`.
    pub sc_msi_devices: ListHead<MdList>,
    /// `sc_ic`: the registered controller; written once by `agintc_msi_attach` before it
    /// registers it, read-only afterwards.
    pub sc_ic: UnsafeCell<InterruptController>,
}

impl AgintcMsiSoftc {
    /// The mapped registers.
    fn regs(&self) -> (BusSpaceTag, BusSpaceHandle) {
        match (self.sc_iot.get(), self.sc_ioh.get()) {
            (Some(iot), Some(ioh)) => (iot, ioh),
            _ => panic(format_args!("agintcmsi: not attached")),
        }
    }

    /// `sc_dmat`.
    fn dmat(&self) -> BusDmaTag {
        match self.sc_dmat.get() {
            Some(t) => t,
            None => panic(format_args!("agintcmsi: no dma tag")),
        }
    }

    /// `sc_cmdq`.
    fn cmdq(&self) -> &'static AgintcDmamem {
        match self.sc_cmdq.get() {
            Some(q) => q,
            None => panic(format_args!("agintcmsi: no command queue")),
        }
    }
}

// SAFETY: `#[repr(C)]` with the device first; the other members are `Cell`s of integers,
// booleans, raw pointers and `Option`s of references and handles, a list head of raw
// pointers and a controller whose members are `Cell`s, `Option`s of function pointers and a
// list entry: all valid as zero bits.
unsafe impl Softc for AgintcMsiSoftc {}

/// `agintc_sc`: the attached controller.
pub static AGINTC: AgintcSoftc = AgintcSoftc {
    sc_dev: Cell::new(ptr::null()),
    sc_handler: Cell::new(ptr::null_mut()),
    sc_lpi: Cell::new(ptr::null_mut()),
    sc_iot: Cell::new(None),
    sc_d_ioh: Cell::new(None),
    sc_r_ioh: Cell::new(ptr::null_mut()),
    sc_rbase_ioh: Cell::new(ptr::null_mut()),
    sc_dmat: Cell::new(None),
    sc_processor: Cell::new(ptr::null_mut()),
    sc_cpuremap: [const { AtomicI32::new(0) }; MAXCPUS as usize],
    sc_nintr: Cell::new(0),
    sc_nlpi: Cell::new(0),
    sc_mbi_addr: Cell::new(0),
    sc_mbi_nranges: Cell::new(0),
    sc_mbi_ranges: Cell::new(ptr::null_mut()),
    sc_prio_shift: Cell::new(0),
    sc_pmr_shift: Cell::new(0),
    sc_rk3399_quirk: Cell::new(false),
    sc_spur: Evcount::new(),
    sc_num_redist: Cell::new(0),
    sc_num_redist_regions: Cell::new(0),
    sc_prop: Cell::new(None),
    sc_pend: Cell::new(None),
    sc_ic: UnsafeCell::new(InterruptController {
        ic_node: Cell::new(0),
        ic_cookie: Cell::new(ptr::null()),
        ic_establish: None,
        ic_establish_msi: None,
        ic_disestablish: None,
        ic_enable: None,
        ic_disable: None,
        ic_route: None,
        ic_cpu_enable: None,
        ic_barrier: None,
        ic_set_wakeup: None,
        ic_list: ListEntry::new(),
        ic_phandle: Cell::new(0),
        ic_cells: Cell::new(0),
        ic_gic_its_id: Cell::new(0),
    }),
    sc_ipi_num: AtomicI32::new(0),
    sc_ipi_reason: [const { AtomicU32::new(0) }; MAXCPUS as usize],
    sc_ipi_irq: AtomicPtr::new(ptr::null_mut()),
};

/// `agintc_ca`: the device is the `struct simplebus_softc` that heads the C's
/// `struct agintc_softc` (`sc_sbus`), for the ITS `simplebus_attach` attaches below it.
pub static AGINTC_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<SimplebusSoftc>(),
    ca_match: Some(agintc_match),
    ca_attach: agintc_attach,
    ca_detach: None,
    ca_activate: Some(agintc_activate),
};

/// `agintc_cd`.
pub static AGINTC_CD: Cfdriver = Cfdriver::new(b"agintc", DV_DULL, 0);

/// `agintcmsi_ca`.
pub static AGINTCMSI_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<AgintcMsiSoftc>(),
    ca_match: Some(agintc_msi_match),
    ca_attach: agintc_msi_attach,
    ca_detach: None,
    ca_activate: None,
};

/// `agintcmsi_cd`.
pub static AGINTCMSI_CD: Cfdriver = Cfdriver::new(b"agintcmsi", DV_DULL, 0);

/// Whether `agintc_attach` got as far as the C's `agintc_sc = sc`.
static ATTACHED: AtomicBool = AtomicBool::new(false);

/// `agintc_compatibles[]`.
static AGINTC_COMPATIBLES: [&[u8]; 2] = [b"arm,gic-v3", b"arm,gic-v4"];

/// `ffs(3)`: the 1-based index of the lowest set bit, 0 for none.
const fn ffs(x: u64) -> i32 {
    if x == 0 {
        0
    } else {
        x.trailing_zeros() as i32 + 1
    }
}

/// `fls(3)`: the 1-based index of the highest set bit, 0 for none.
const fn fls(x: u32) -> i32 {
    32 - x.leading_zeros() as i32
}

/// The priority byte an interrupt or the mask gets for `ipl`: `((0xff - ipl) << shift) &
/// 0xff`.
const fn agintc_prival(ipl: i32, shift: i32) -> u8 {
    (((0xff - ipl) << shift) & 0xff) as u8
}

/// `GICR_PROPBASER`'s value: the table, inner shareable, non-cacheable, and the ID bits for
/// `nlpi` LPIs above `LPI_BASE`.
const fn agintc_propbaser(pa: u64, nlpi: i32) -> u64 {
    pa | GICR_PROPBASER_ISH
        | GICR_PROPBASER_IC_NORM_NC
        | (fls((LPI_BASE + nlpi - 1) as u32) - 1) as u64
}

/// `ICC_SGI1R`'s value that raises SGI `sgi` at the CPU whose affinity is `mpidr` alone.
#[cfg(any(feature = "multiprocessor", test))]
const fn agintc_sgi1r(mpidr: u64, sgi: i32) -> u64 {
    let mut sendmask = (mpidr & MPIDR_AFF3) << 16;
    sendmask |= (mpidr & MPIDR_AFF2) << 16;
    sendmask |= (mpidr & MPIDR_AFF1) << 8;
    sendmask |= 1 << (mpidr & 0x0f);
    sendmask |= (sgi as u64) << 24;
    sendmask
}

/// The redistributor frames' size for a `GICR_TYPER`: two 64 KB frames, two more with
/// virtual LPIs, unless the device tree gives a stride.
const fn agintc_redist_size(typer: u64, redist_stride: u64) -> usize {
    if redist_stride == 0 {
        let mut sz = 64 * 1024 * 2;
        if typer & GICR_TYPER_VLPIS != 0 {
            sz += 64 * 1024 * 2;
        }
        sz
    } else {
        redist_stride as usize
    }
}

/// The affinity `GICR_TYPER` reports for the CPU whose `MPIDR_EL1` is `mpidr`: Aff3 in the
/// top byte, then Aff2..Aff0.
const fn agintc_mpidr_affinity(mpidr: u64) -> u32 {
    (((mpidr >> 8) & 0xff00_0000) | (mpidr & 0x00ff_ffff)) as u32
}

/// `mrs` of a CPU interface register.
macro_rules! icc_read {
    ($reg:ident) => {{
        let val: u64;
        // SAFETY: a GICv3 CPU interface register of this CPU, which `agintc_attach` enabled
        // (`ICC_SRE_EL1`); reading `ICC_IAR1` acknowledges an interrupt, which is what its
        // callers want. No `nomem`: the read must stay ordered with the memory accesses
        // around it.
        unsafe {
            ::core::arch::asm!(
                concat!("mrs {}, ", icc_reg!($reg)),
                out(reg) val,
                options(nostack, preserves_flags)
            )
        };
        val
    }};
}

/// `msr` of a CPU interface register.
macro_rules! icc_write {
    ($reg:ident, $val:expr) => {{
        let val: u64 = $val;
        // SAFETY: a GICv3 CPU interface register of this CPU; what the write changes (the
        // priority mask, the group enable, an EOI, an SGI) is the driver's to change. No
        // `nomem`, as in `icc_read!`.
        unsafe {
            ::core::arch::asm!(
                concat!("msr ", icc_reg!($reg), ", {}"),
                in(reg) val,
                options(nostack, preserves_flags)
            )
        };
    }};
}

/// `__isb()`.
fn isb() {
    // SAFETY: a barrier.
    unsafe { core::arch::asm!("isb", options(nostack, preserves_flags)) };
}

/// `__asm volatile("dsb sy")`.
fn dsb_sy() {
    // SAFETY: a barrier.
    unsafe { core::arch::asm!("dsb sy", options(nostack, preserves_flags)) };
}

/// The distributor.
fn dregs() -> (BusSpaceTag, BusSpaceHandle) {
    match (AGINTC.sc_iot.get(), AGINTC.sc_d_ioh.get()) {
        (Some(iot), Some(d)) => (iot, d),
        _ => panic(format_args!("agintc: not attached")),
    }
}

/// `sc->sc_r_ioh[hwcpu]`.
fn r_ioh(sc: &AgintcSoftc, hwcpu: i32) -> BusSpaceHandle {
    let base = sc.sc_r_ioh.get();
    if base.is_null() || hwcpu < 0 || hwcpu >= sc.sc_num_redist.get() {
        panic(format_args!("agintc: bad redistributor {hwcpu}"));
    }
    // SAFETY: `agintc_attach` filled `sc_num_redist` handles at `sc_r_ioh`, never freed.
    unsafe { base.add(hwcpu as usize).read() }
}

/// `sc->sc_rbase_ioh[idx]`.
fn rbase_ioh(sc: &AgintcSoftc, idx: i32) -> BusSpaceHandle {
    let base = sc.sc_rbase_ioh.get();
    if base.is_null() || idx < 0 || idx >= sc.sc_num_redist_regions.get() {
        panic(format_args!("agintc: bad redistributor region {idx}"));
    }
    // SAFETY: `agintc_attach` filled `sc_num_redist_regions` handles at `sc_rbase_ioh`.
    unsafe { base.add(idx as usize).read() }
}

/// `sc->sc_processor[hwcpu]`.
fn processor(sc: &AgintcSoftc, hwcpu: i32) -> u16 {
    let base = sc.sc_processor.get();
    if base.is_null() || hwcpu < 0 || hwcpu >= sc.sc_num_redist.get() {
        panic(format_args!("agintc: bad redistributor {hwcpu}"));
    }
    // SAFETY: `agintc_attach` filled `sc_num_redist` entries at `sc_processor`.
    unsafe { base.add(hwcpu as usize).read() }
}

/// `sc->sc_cpuremap[ci->ci_cpuid]`.
fn cpuremap(sc: &AgintcSoftc, ci: &CpuInfo) -> i32 {
    sc.sc_cpuremap[ci.ci_cpuid.get() as usize].load(Ordering::Relaxed)
}

/// `&sc->sc_handler[irq]`.
fn handler(sc: &AgintcSoftc, irq: i32) -> &'static Intrq {
    kassert!(irq >= 0 && irq < sc.sc_nintr.get());
    // SAFETY: `sc_handler` has `sc_nintr` entries, allocated by `agintc_attach`.
    unsafe { &*sc.sc_handler.get().add(irq as usize) }
}

/// `&sc->sc_lpi[i]`.
fn lpi_slot(sc: &AgintcSoftc, i: i32) -> &'static Cell<*mut AgintcLpiInfo> {
    kassert!(i >= 0 && i < sc.sc_nlpi.get());
    // SAFETY: `sc_lpi` has `sc_nlpi` zeroed slots, allocated by `agintc_attach`, never freed.
    unsafe { &*sc.sc_lpi.get().add(i as usize) }
}

/// `sc->sc_lpi[i]`, if one is established.
fn lpi(sc: &AgintcSoftc, i: i32) -> Option<&'static AgintcLpiInfo> {
    // SAFETY: a non-null slot holds an `agintc_lpi_info` `agintc_intr_establish_msi`
    // allocated, freed only after the slot is cleared.
    unsafe { lpi_slot(sc, i).get().as_ref() }
}

/// `&sc->sc_mbi_ranges[i]`.
fn mbi_range(sc: &AgintcSoftc, i: i32) -> &'static AgintcMbiRange {
    kassert!(i >= 0 && i < sc.sc_mbi_nranges.get());
    // SAFETY: `agintc_mbiinit` filled `sc_mbi_nranges` ranges, never freed.
    unsafe { &*sc.sc_mbi_ranges.get().add(i as usize) }
}

/// `sc->sc_prop`.
fn prop(sc: &AgintcSoftc) -> &'static AgintcDmamem {
    match sc.sc_prop.get() {
        Some(p) => p,
        None => panic(format_args!("agintc: no LPI configuration table")),
    }
}

/// Makes byte `i` of the LPI configuration table globally visible: `cpu_dcache_wb_range`
/// and `dsb sy`.
fn prop_sync(p: &AgintcDmamem, i: usize) {
    cpu_dcache_wb_range(p.kva() as usize + i, 1);
    dsb_sy();
}

/// `agintc_match`: whether the node is a GICv3 or GICv4.
pub fn agintc_match(_parent: Option<&Device>, _cfdata: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: `agintc` attaches at `fdt`, whose buses hand over a `FdtAttachArgs`.
    let faa = unsafe { &*aux.cast::<FdtAttachArgs<'_>>() };
    i32::from(
        AGINTC_COMPATIBLES
            .iter()
            .any(|c| OF_is_compatible(faa.fa_node, c)),
    )
}

/// `agintc_attach`: maps the distributor and the redistributor regions, finds each CPU's
/// redistributor, sets up the LPI tables, resets the controller, takes over `spl` and the
/// IRQ dispatch, picks the IPI's SGI, registers with the device tree and attaches the ITS.
pub fn agintc_attach(parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    let sc = &AGINTC;
    // SAFETY: as in `agintc_match`.
    let faa = unsafe { &*aux.cast::<FdtAttachArgs<'_>>() };

    let psw = intr_disable();
    arm_init_smask();

    sc.sc_dev.set(ptr::from_ref(self_));
    sc.sc_iot.set(Some(faa.fa_iot));
    sc.sc_dmat.set(Some(faa.fa_dmat));
    let iot = faa.fa_iot;

    let nregions = OF_getpropint(faa.fa_node, b"#redistributor-regions", 1) as i32;
    sc.sc_num_redist_regions.set(nregions);

    if nregions < 1 || faa.fa_reg.len() < nregions as usize + 1 {
        panic(format_args!("agintc_attach: missing registers"));
    }

    // SAFETY: the device tree's registers of the GIC, which nothing else drives.
    let Ok(d_ioh) = (unsafe {
        bus_space_map(
            iot,
            faa.fa_reg[0].addr as usize,
            faa.fa_reg[0].size as usize,
            0,
        )
    }) else {
        panic(format_args!("agintc_attach: GICD bus_space_map failed"));
    };
    sc.sc_d_ioh.set(Some(d_ioh));

    let Some(rbase) = mallocarray(
        nregions as usize,
        size_of::<BusSpaceHandle>(),
        M_DEVBUF,
        M_WAITOK,
    ) else {
        panic(format_args!("agintc_attach: out of memory"));
    };
    let rbase = rbase.cast::<BusSpaceHandle>();
    for idx in 0..nregions as usize {
        let reg = &faa.fa_reg[1 + idx];
        // SAFETY: as above.
        let Ok(h) = (unsafe { bus_space_map(iot, reg.addr as usize, reg.size as usize, 0) }) else {
            panic(format_args!("agintc_attach: GICR bus_space_map failed"));
        };
        // SAFETY: slot `idx` of the `nregions` just allocated.
        unsafe { rbase.as_ptr().add(idx).write(h) };
    }
    sc.sc_rbase_ioh.set(rbase.as_ptr());

    let typer = bus_space_read_4(iot, d_ioh, GICD_TYPER);

    if typer & GICD_TYPER_LPIS != 0 {
        // Allocate redistributor tables
        let Some(p) = agintc_dmamem_alloc(faa.fa_dmat, GICR_PROP_SIZE, GICR_PROP_SIZE) else {
            kprintf!(": can't alloc LPI config table\n");
            agintc_attach_unmap(sc, faa, psw);
            return;
        };
        sc.sc_prop.set(Some(p));
        let Some(p) = agintc_dmamem_alloc(faa.fa_dmat, GICR_PEND_SIZE, GICR_PEND_SIZE) else {
            kprintf!(": can't alloc LPI pending table\n");
            agintc_attach_unmap(sc, faa, psw);
            return;
        };
        sc.sc_pend.set(Some(p));

        // Minimum number of LPIs supported by any implementation.
        sc.sc_nlpi.set(8192);
    }

    if typer & GICD_TYPER_MBIS != 0 {
        agintc_mbiinit(sc, faa.fa_node, faa.fa_reg[0].addr);
    }

    // We are guaranteed to have at least 16 priority levels, so in principle we just want to
    // use the top 4 bits of the (non-secure) priority field.
    sc.sc_prio_shift.set(4);
    sc.sc_pmr_shift.set(4);

    // If the system supports two security states and SCR_EL3.FIQ is zero, the non-secure
    // shifted view applies. We detect this by checking whether the number of writable bits
    // matches the number of implemented priority bits. If that is the case we will need to
    // adjust the priorities that we write into ICC_PMR_EL1 accordingly.
    //
    // On Ampere eMAG it appears as if there are five writable bits when we write 0xff. But
    // for higher priorities (smaller values) only the top 4 bits stick. So we use 0xbf
    // instead to determine the number of writable bits.
    let ctrl = bus_space_read_4(iot, d_ioh, GICD_CTLR);
    if ctrl & GICD_CTLR_DS == 0 {
        let icc_ctlr = icc_read!(ICC_CTLR_EL1);
        let nbits = icc_ctlr_el1_pribits(icc_ctlr) as i32 + 1;
        let oldpmr = icc_read!(ICC_PMR);
        icc_write!(ICC_PMR, 0xbf);
        let pmr = icc_read!(ICC_PMR);
        icc_write!(ICC_PMR, oldpmr);
        if nbits == 8 - (ffs(pmr) - 1) {
            sc.sc_pmr_shift.set(sc.sc_pmr_shift.get() - 1);
        }
    }

    // The Rockchip RK3399 is busted. Its GIC-500 treats all access to its memory mapped
    // registers as "secure". As a result, several registers don't behave as expected. For
    // example, the GICD_IPRIORITYRn and GICR_IPRIORITYRn registers expose the full priority
    // range available to secure interrupts. We need to be aware of this and write an
    // adjusted priority value into these registers. We also need to be careful not to touch
    // any bits that shouldn't be writable in non-secure mode.
    //
    // We check whether we have secure mode access to these registers by attempting to write
    // to the GICD_NSACR register and check whether its contents actually change. In that
    // case we need to adjust the priorities we write into GICD_IPRIORITYRn and
    // GICRIPRIORITYRn accordingly.
    let oldnsacr = bus_space_read_4(iot, d_ioh, gicd_nsacr(32));
    bus_space_write_4(iot, d_ioh, gicd_nsacr(32), oldnsacr ^ 0xffff_ffff);
    let nsacr = bus_space_read_4(iot, d_ioh, gicd_nsacr(32));
    if nsacr != oldnsacr {
        bus_space_write_4(iot, d_ioh, gicd_nsacr(32), oldnsacr);
        sc.sc_rk3399_quirk.set(true);
        sc.sc_prio_shift.set(sc.sc_prio_shift.get() - 1);
        kprintf!(" sec");
    }

    kprintf!(
        " shift {}:{}",
        sc.sc_prio_shift.get(),
        sc.sc_pmr_shift.get()
    );

    evcount_attach(&sc.sc_spur, "irq1023/spur", ptr::null());

    icc_write!(ICC_SRE_EL1, ICC_SRE_EL1_EN);
    isb();

    let mut nintr = 32 * (typer & GICD_TYPER_ITLINE_M) as i32;
    nintr += 32; // ICD_ICTR + 1, irq 0-31 is SGI, 32+ is PPI
    sc.sc_nintr.set(nintr);

    ATTACHED.store(true, Ordering::Release); // save this for global access

    // find the redistributors.
    let redist_stride = OF_getpropint64(faa.fa_node, b"redistributor-stride", 0);
    let mut idx = 0;
    let mut offset = 0usize;
    let mut nredist = 0;
    while idx < nregions {
        let rtyper = bus_space_read_8(iot, rbase_ioh(sc, idx), offset + GICR_TYPER);
        let sz = agintc_redist_size(rtyper, redist_stride);

        #[cfg(feature = "debug")]
        kprintf!("probing redistributor {nredist} {offset:x}\n");

        offset += sz;
        if offset >= faa.fa_reg[1 + idx as usize].size as usize || rtyper & GICR_TYPER_LAST != 0 {
            offset = 0;
            idx += 1;
        }
        nredist += 1;
    }

    sc.sc_num_redist.set(nredist);
    kprintf!(" nirq {nintr} nredist {nredist}");

    let (Some(rioh), Some(procs)) = (
        mallocarray(
            nredist as usize,
            size_of::<BusSpaceHandle>(),
            M_DEVBUF,
            M_WAITOK,
        ),
        mallocarray(nredist as usize, size_of::<u16>(), M_DEVBUF, M_WAITOK),
    ) else {
        panic(format_args!("agintc_attach: out of memory"));
    };
    let rioh = rioh.cast::<BusSpaceHandle>();
    let procs = procs.cast::<u16>();

    // submap and configure the redistributors.
    idx = 0;
    offset = 0;
    for nredist in 0..nredist {
        let rb = rbase_ioh(sc, idx);
        let rtyper = bus_space_read_8(iot, rb, offset + GICR_TYPER);
        let sz = agintc_redist_size(rtyper, redist_stride);

        let affinity = (bus_space_read_8(iot, rb, offset + GICR_TYPER) >> 32) as u32;
        let mut ci: *const CpuInfo = cpu_info_list();
        // SAFETY: `cpu_info_list` links static cpu_infos.
        while let Some(c) = unsafe { ci.as_ref() } {
            if affinity == agintc_mpidr_affinity(c.ci_mpidr.get()) {
                sc.sc_cpuremap[c.ci_cpuid.get() as usize].store(nredist, Ordering::Relaxed);
                break;
            }
            ci = c.ci_next.get();
        }

        let p = (bus_space_read_8(iot, rb, offset + GICR_TYPER) >> 8) as u16;
        // SAFETY: slot `nredist` of the `nredist` entries just allocated.
        unsafe { procs.as_ptr().add(nredist as usize).write(p) };

        let Ok(sub) = bus_space_subregion(iot, rb, offset, sz) else {
            panic(format_args!(
                "agintc_attach: GICR bus_space_subregion failed"
            ));
        };
        // SAFETY: as above.
        unsafe { rioh.as_ptr().add(nredist as usize).write(sub) };

        if sc.sc_nlpi.get() > 0 {
            let (prop, pend) = (prop(sc), sc.sc_pend.get());
            bus_space_write_8(
                iot,
                rb,
                offset + GICR_PROPBASER,
                agintc_propbaser(prop.dva(), sc.sc_nlpi.get()),
            );
            if let Some(pend) = pend {
                bus_space_write_8(
                    iot,
                    rb,
                    offset + GICR_PENDBASER,
                    pend.dva()
                        | GICR_PENDBASER_ISH
                        | GICR_PENDBASER_IC_NORM_NC
                        | GICR_PENDBASER_PTZ,
                );
            }
            bus_space_write_4(iot, rb, offset + GICR_CTLR, GICR_CTLR_ENABLE_LPIS);
        }

        offset += sz;
        if offset >= faa.fa_reg[1 + idx as usize].size as usize || rtyper & GICR_TYPER_LAST != 0 {
            offset = 0;
            idx += 1;
        }
    }
    sc.sc_r_ioh.set(rioh.as_ptr());
    sc.sc_processor.set(procs.as_ptr());

    // Disable all interrupts, clear all pending
    for i in 1..nintr / 32 {
        bus_space_write_4(iot, d_ioh, gicd_icactiver(i * 32), !0);
        bus_space_write_4(iot, d_ioh, gicd_icenabler(i * 32), !0);
    }

    for i in (4..nintr).step_by(4) {
        // lowest priority ??
        bus_space_write_4(iot, d_ioh, gicd_ipriorityr(i), 0xffff_ffff);
    }

    // Set all interrupts to G1NS
    for i in 1..nintr / 32 {
        bus_space_write_4(iot, d_ioh, gicd_igroupr(i * 32), !0);
        bus_space_write_4(iot, d_ioh, gicd_igrpmodr(i * 32), 0);
    }

    for i in 2..nintr / 16 {
        // irq 32 - N
        bus_space_write_4(iot, d_ioh, gicd_icfgr(i * 16), 0);
    }

    agintc_cpuinit();

    let Some(handlers) = mallocarray(
        nintr as usize,
        size_of::<Intrq>(),
        M_DEVBUF,
        M_ZERO | M_WAITOK,
    ) else {
        panic(format_args!(
            "agintc_attach: no memory for {nintr} handler queues"
        ));
    };
    let handlers = handlers.cast::<Intrq>();
    for i in 0..nintr as usize {
        // SAFETY: a fresh allocation of `nintr` entries, written before use.
        unsafe { handlers.as_ptr().add(i).write(Intrq::new()) };
    }
    sc.sc_handler.set(handlers.as_ptr());
    for i in 0..nintr {
        handler(sc, i).iq_list.init();
    }
    if sc.sc_nlpi.get() > 0 {
        let Some(lpis) = mallocarray(
            sc.sc_nlpi.get() as usize,
            size_of::<Cell<*mut AgintcLpiInfo>>(),
            M_DEVBUF,
            M_ZERO | M_WAITOK,
        ) else {
            panic(format_args!("agintc_attach: no memory for the LPIs"));
        };
        sc.sc_lpi.set(lpis.as_ptr().cast());
    }

    // set priority to IPL_HIGH until configure lowers to desired IPL
    agintc_setipl(IPL_HIGH);

    // initialize all interrupts as disabled
    agintc_calc_mask();

    // insert self as interrupt handler
    arm_set_intr_handler(
        agintc_splraise,
        agintc_spllower,
        agintc_splx,
        agintc_setipl,
        Some(agintc_irq_handler),
        None,
        Some(agintc_enable_wakeup),
        Some(agintc_disable_wakeup),
    );

    // enable interrupts
    let ctrl = bus_space_read_4(iot, d_ioh, GICD_CTLR);
    let mut bits = GICD_CTLR_ARE_NS | GICD_CTLR_ENABLE_GRP1A | GICD_CTLR_ENABLE_GRP1;
    if sc.sc_rk3399_quirk.get() {
        bits &= !GICD_CTLR_ENABLE_GRP1A;
        bits <<= 1;
    }
    bus_space_write_4(iot, d_ioh, GICD_CTLR, ctrl | bits);

    icc_write!(ICC_PMR, 0xff);
    icc_write!(ICC_BPR1, 0);
    icc_write!(ICC_IGRPEN1, 1);

    // setup IPI interrupts
    #[cfg(feature = "multiprocessor")]
    {
        let mut ipiirq = -1;
        for i in 0..16 {
            let hwcpu = sc.sc_cpuremap[cpu_number() as usize].load(Ordering::Relaxed);
            let r = r_ioh(sc, hwcpu);

            let oldreg = bus_space_read_1(iot, r, gicr_ipriorityr(i));
            bus_space_write_1(iot, r, gicr_ipriorityr(i), oldreg ^ 0x20);

            // if this interrupt is not usable, pri will be unmodified
            let reg = bus_space_read_1(iot, r, gicr_ipriorityr(i));
            if reg == oldreg {
                continue;
            }

            // return to original value, will be set when used
            bus_space_write_1(iot, r, gicr_ipriorityr(i), oldreg);
            ipiirq = i;
            break;
        }

        if ipiirq == -1 {
            panic(format_args!("no irq available for IPI"));
        }

        kprintf!(" ipi {ipiirq}");

        let ih = agintc_intr_establish(
            ipiirq,
            IST_EDGE_RISING,
            IPL_IPI | IPL_MPSAFE,
            None,
            agintc_ipi_handler,
            ptr::from_ref(sc).cast_mut().cast(),
            Some("ipi"),
        );
        sc.sc_ipi_irq.store(
            ih.map_or(ptr::null_mut(), NonNull::as_ptr),
            Ordering::Release,
        );
        sc.sc_ipi_num.store(ipiirq, Ordering::Release);

        // SAFETY: the boot CPU, before any application processor runs (the hook's protocol
        // in `arm64/intr.rs`).
        unsafe { INTR_SEND_IPI_FUNC.write(agintc_send_ipi) };
    }

    // SAFETY: the controller is not registered yet, so nothing else reads it; it is written
    // once, here.
    let ic = unsafe { &mut *sc.sc_ic.get() };
    ic.ic_node.set(faa.fa_node);
    ic.ic_cookie.set(ptr::from_ref(sc).cast::<()>());
    ic.ic_establish = Some(agintc_intr_establish_fdt);
    ic.ic_disestablish = Some(agintc_intr_disestablish);
    ic.ic_route = Some(agintc_route_irq);
    ic.ic_cpu_enable = Some(agintc_cpuinit);
    ic.ic_barrier = Some(agintc_intr_barrier);
    if sc.sc_mbi_nranges.get() > 0 {
        ic.ic_establish_msi = Some(agintc_intr_establish_mbi);
    }
    ic.ic_set_wakeup = Some(agintc_intr_set_wakeup);
    // SAFETY: the softc is static; the controller is not written again.
    arm_intr_register_fdt(unsafe { &*sc.sc_ic.get() });

    // SAFETY: `psw` is this CPU's DAIF from `intr_disable` above.
    unsafe { intr_restore(psw) };

    // Attach ITS.
    simplebus_attach(parent, self_, aux);
}

/// `agintc_attach`'s `unmap:` path: frees what it allocated and unmaps the registers.
fn agintc_attach_unmap(sc: &AgintcSoftc, faa: &FdtAttachArgs<'_>, psw: u64) {
    let (iot, d_ioh) = dregs();
    let dmat = faa.fa_dmat;

    let r = sc.sc_r_ioh.replace(ptr::null_mut());
    if let Some(r) = NonNull::new(r) {
        free(
            r.cast(),
            M_DEVBUF,
            sc.sc_num_redist.get() as usize * size_of::<BusSpaceHandle>(),
        );
    }
    let p = sc.sc_processor.replace(ptr::null_mut());
    if let Some(p) = NonNull::new(p) {
        free(
            p.cast(),
            M_DEVBUF,
            sc.sc_num_redist.get() as usize * size_of::<u16>(),
        );
    }

    if let Some(pend) = sc.sc_pend.take() {
        agintc_dmamem_free(dmat, pend);
    }
    if let Some(prop) = sc.sc_prop.take() {
        agintc_dmamem_free(dmat, prop);
    }

    let nregions = sc.sc_num_redist_regions.get();
    for idx in 0..nregions {
        bus_space_unmap(
            iot,
            rbase_ioh(sc, idx),
            faa.fa_reg[1 + idx as usize].size as usize,
        );
    }
    if let Some(rb) = NonNull::new(sc.sc_rbase_ioh.replace(ptr::null_mut())) {
        free(
            rb.cast(),
            M_DEVBUF,
            nregions as usize * size_of::<BusSpaceHandle>(),
        );
    }

    bus_space_unmap(iot, d_ioh, faa.fa_reg[0].size as usize);
    sc.sc_d_ioh.set(None);

    // SAFETY: `psw` is this CPU's DAIF from `agintc_attach`'s `intr_disable`.
    unsafe { intr_restore(psw) };
}

/// `agintc_activate`: on resume, the controller's state comes back before the children's.
pub fn agintc_activate(self_: &Device, act: i32) -> Result<(), Errno> {
    let sc = &AGINTC;

    if act == DVACT_RESUME {
        agintc_restore(sc);
    }

    config_activate_children(self_, act)
}

/// `agintc_restore`: reprograms every established SPI, PPI and LPI.
pub fn agintc_restore(sc: &AgintcSoftc) {
    for irq in 0..sc.sc_nintr.get() {
        let iq = handler(sc, irq);
        let Some(ih) = iq.iq_list.first() else {
            continue;
        };

        agintc_intr_config(sc, irq, ih.ih_type);
        agintc_set_priority(sc, irq, iq.iq_irq_min.get());
        agintc_route(sc, irq, IRQ_ENABLE, Some(ih.ih_ci));
        agintc_intr_enable(sc, irq);
    }

    for irq in 0..sc.sc_nlpi.get() {
        let Some(li) = lpi(sc, irq) else {
            continue;
        };
        kassert!(!li.li_ih.is_null());
        let p = prop(sc);
        p.set_u8(irq as usize, p.get_u8(irq as usize) | GICR_PROP_ENABLE);
        // Make globally visible.
        prop_sync(p, irq as usize);
        // Invalidate cache
        agintc_msi_inv(li);
    }
}

/// `agintc_mbiinit`: the message based SPIs of `mbi-ranges`, when the distributor is itself
/// an MSI controller.
pub fn agintc_mbiinit(sc: &AgintcSoftc, node: i32, addr: u64) {
    if OF_getproplen(node, b"msi-controller") != 0 {
        return;
    }

    let len = OF_getproplen(node, b"mbi-ranges");
    // The C's `len % 2 * sizeof(uint32_t) != 0`: `(len % 2) * 4`, an odd length.
    if len <= 0 || (len % 2) * 4 != 0 {
        return;
    }
    let len = len as usize;

    let Some(ranges) = malloc(len, M_TEMP, M_WAITOK) else {
        return;
    };
    let ranges = ranges.cast::<u32>();
    // SAFETY: a fresh allocation of `len` bytes, freed below.
    let cells = unsafe { core::slice::from_raw_parts_mut(ranges.as_ptr(), len / 4) };
    OF_getpropintarray(node, b"mbi-ranges", cells);

    let nranges = len / (2 * size_of::<u32>());
    let Some(mrs) = mallocarray(nranges, size_of::<AgintcMbiRange>(), M_DEVBUF, M_WAITOK) else {
        panic(format_args!("agintc_mbiinit: out of memory"));
    };
    let mrs = mrs.cast::<AgintcMbiRange>();

    for i in 0..nranges {
        let base = cells[2 * i] as i32;
        let span = cells[2 * i + 1] as i32;
        let Some(slots) = mallocarray(
            span.max(0) as usize,
            size_of::<Cell<*mut c_void>>(),
            M_DEVBUF,
            M_WAITOK | M_ZERO,
        ) else {
            panic(format_args!("agintc_mbiinit: out of memory"));
        };
        // SAFETY: slot `i` of the `nranges` just allocated, written before use.
        unsafe {
            mrs.as_ptr().add(i).write(AgintcMbiRange {
                mr_base: base,
                mr_span: span,
                mr_mbi: slots.as_ptr().cast(),
            });
        }
    }
    sc.sc_mbi_ranges.set(mrs.as_ptr());
    sc.sc_mbi_nranges.set(nranges as i32);

    free(ranges.cast(), M_TEMP, len);

    let addr = OF_getpropint64(node, b"mbi-alias", addr);
    sc.sc_mbi_addr.set(addr + GICD_SETSPI_NSR as u64);

    kprintf!(" mbi");
}

/// `agintc_cpuinit`: initialize redistributors on each core.
pub fn agintc_cpuinit() {
    let sc = &AGINTC;
    let iot = dregs().0;
    let hwcpu = sc.sc_cpuremap[cpu_number() as usize].load(Ordering::Relaxed);
    let r = r_ioh(sc, hwcpu);

    let mut waker = bus_space_read_4(iot, r, GICR_WAKER);
    waker &= !GICR_WAKER_PROCESSORSLEEP;
    bus_space_write_4(iot, r, GICR_WAKER, waker);

    let mut timeout = 100_000;
    loop {
        waker = bus_space_read_4(iot, r, GICR_WAKER);
        timeout -= 1;
        if timeout == 0 || waker & GICR_WAKER_CHILDRENASLEEP == 0 {
            break;
        }
    }
    if timeout == 0 {
        kprintf!("agintc_cpuinit: waker timed out\n");
    }

    bus_space_write_4(iot, r, GICR_ICENABLE0, !0);
    bus_space_write_4(iot, r, GICR_ICPENDR0, !0);
    bus_space_write_4(iot, r, GICR_ICACTIVE0, !0);
    for i in (0..32).step_by(4) {
        bus_space_write_4(iot, r, gicr_ipriorityr(i), !0);
    }
    bus_space_write_4(iot, r, GICR_IGROUPR0, !0);
    bus_space_write_4(iot, r, GICR_IGRPMODR0, 0);

    let ipi = sc.sc_ipi_irq.load(Ordering::Acquire);
    if !ipi.is_null() {
        agintc_route_irq(ipi.cast(), IRQ_ENABLE, curcpu());
    }

    icc_write!(ICC_PMR, 0xff);
    icc_write!(ICC_BPR1, 0);
    icc_write!(ICC_IGRPEN1, 1);
    // SAFETY: the CPU interface and this CPU's redistributor are set up; its interrupts are
    // masked at the redistributor until established.
    unsafe { intr_enable() };
}

/// `agintc_set_priority`: an SPI's priority at the distributor, an SGI's or PPI's at the
/// local redistributor.
pub fn agintc_set_priority(sc: &AgintcSoftc, irq: i32, ipl: i32) {
    let (iot, d) = dregs();
    let prival = agintc_prival(ipl, sc.sc_prio_shift.get());

    if irq >= SPI_BASE {
        bus_space_write_1(iot, d, gicd_ipriorityr(irq), prival);
    } else {
        // only sets local redistributor
        let hwcpu = cpuremap(sc, curcpu());
        bus_space_write_1(iot, r_ioh(sc, hwcpu), gicr_ipriorityr(irq), prival);
    }
}

/// `agintc_setipl`: sets the level in `ci_cpl` and in the CPU interface's priority mask.
pub fn agintc_setipl(ipl: i32) {
    let sc = &AGINTC;
    let ci = curcpu();

    // disable here is only to keep hardware in sync with ci->ci_cpl
    let psw = intr_disable();
    ci.ci_cpl.set(ipl as u32);

    let prival = agintc_prival(ipl, sc.sc_pmr_shift.get());
    icc_write!(ICC_PMR, u64::from(prival));
    isb();

    // SAFETY: `psw` is this CPU's DAIF from `intr_disable`.
    unsafe { intr_restore(psw) };
}

/// `agintc_enable_wakeup`: before suspending, disables every interrupt whose handlers are
/// not `IPL_WAKEUP`.
pub fn agintc_enable_wakeup() {
    let sc = &AGINTC;

    for irq in 0..sc.sc_nintr.get() {
        let iq = handler(sc, irq);
        // No handler? Disabled already.
        if iq.iq_list.is_empty() {
            continue;
        }
        // Unless we're WAKEUP, disable.
        let wakeup = iq
            .iq_list
            .iter()
            .any(|ih| ih.ih_flags.get() & IPL_WAKEUP != 0);
        if !wakeup {
            agintc_intr_disable(sc, irq);
        }
    }

    for irq in 0..sc.sc_nlpi.get() {
        let Some(li) = lpi(sc, irq) else {
            continue;
        };
        // SAFETY: an established LPI's handler, freed only after its slot is cleared.
        let Some(ih) = (unsafe { li.li_ih.as_ref() }) else {
            panic(format_args!(
                "agintc_enable_wakeup: lpi {irq} without handler"
            ));
        };
        if ih.ih_flags.get() & IPL_WAKEUP != 0 {
            continue;
        }
        let p = prop(sc);
        p.set_u8(irq as usize, p.get_u8(irq as usize) & !GICR_PROP_ENABLE);
        // Make globally visible.
        prop_sync(p, irq as usize);
        // Invalidate cache
        agintc_msi_inv(li);
    }
}

/// `agintc_disable_wakeup`: all interrupts have already been enabled.
pub fn agintc_disable_wakeup() {}

/// `agintc_intr_enable`.
pub fn agintc_intr_enable(sc: &AgintcSoftc, irq: i32) {
    let (iot, d) = dregs();
    let bit = 1u32 << irq_to_reg32bit(irq);

    if irq >= 32 {
        bus_space_write_4(iot, d, gicd_isenabler(irq), bit);
    } else {
        let hwcpu = cpuremap(sc, curcpu());
        bus_space_write_4(iot, r_ioh(sc, hwcpu), GICR_ISENABLE0, bit);
    }
}

/// `agintc_intr_disable`.
pub fn agintc_intr_disable(sc: &AgintcSoftc, irq: i32) {
    let (iot, d) = dregs();
    let bit = 1u32 << irq_to_reg32bit(irq);

    if irq >= 32 {
        bus_space_write_4(iot, d, gicd_icenabler(irq), bit);
    } else {
        let hwcpu = cpuremap(sc, curcpu());
        bus_space_write_4(iot, r_ioh(sc, hwcpu), GICR_ICENABLE0, bit);
    }
}

/// `agintc_intr_config`: an SPI's trigger, level or rising edge.
pub fn agintc_intr_config(_sc: &AgintcSoftc, irq: i32, type_: i32) {
    // Don't dare to change SGIs or PPIs (yet)
    if irq < 32 {
        return;
    }

    let (iot, d) = dregs();
    let mut reg = bus_space_read_4(iot, d, gicd_icfgr(irq));
    reg &= !gicd_icfgr_trig_mask(irq);
    if type_ == IST_EDGE_RISING {
        reg |= gicd_icfgr_trig_edge(irq);
    } else {
        reg |= gicd_icfgr_trig_level(irq);
    }
    bus_space_write_4(iot, d, gicd_icfgr(irq), reg);
}

/// `agintc_calc_mask`: recomputes every interrupt's priority and enable.
pub fn agintc_calc_mask() {
    let sc = &AGINTC;
    for irq in 0..sc.sc_nintr.get() {
        agintc_calc_irq(sc, irq);
    }
}

/// `agintc_calc_irq`: one interrupt's priority, route and enable from its handlers.
pub fn agintc_calc_irq(sc: &AgintcSoftc, irq: i32) {
    let iq = handler(sc, irq);
    // SAFETY: a queue with handlers names its CPU, a static cpu_info.
    let ci = unsafe { iq.iq_ci.get().as_ref() };
    let mut max = IPL_NONE;
    let mut min = IPL_HIGH;

    for ih in iq.iq_list.iter() {
        if ih.ih_ipl > max {
            max = ih.ih_ipl;
        }
        if ih.ih_ipl < min {
            min = ih.ih_ipl;
        }
    }

    if max == IPL_NONE {
        min = IPL_NONE;
    }

    if iq.iq_irq_max.get() == max && iq.iq_irq_min.get() == min {
        return;
    }

    iq.iq_irq_max.set(max);
    iq.iq_irq_min.set(min);

    #[cfg(feature = "debug")]
    if min != IPL_NONE {
        kprintf!("irq {irq} to block at {max} {min} \n");
    }
    // Enable interrupts at lower levels, clear -> enable
    // Set interrupt priority/enable
    if min != IPL_NONE {
        agintc_set_priority(sc, irq, min);
        agintc_route(sc, irq, IRQ_ENABLE, ci);
        agintc_intr_enable(sc, irq);
    } else {
        agintc_intr_disable(sc, irq);
        agintc_route(sc, irq, IRQ_DISABLE, ci);
    }
}

/// `agintc_splx`.
pub fn agintc_splx(new: i32) {
    let ci = curcpu();

    if ci.ci_ipending.get() & arm_smask(new) != 0 {
        arm_do_pending_intr(new);
    }

    agintc_setipl(new);
}

/// `agintc_spllower`.
pub fn agintc_spllower(new: i32) -> i32 {
    let ci = curcpu();
    let old = ci.ci_cpl.get() as i32;

    agintc_splx(new);
    old
}

/// `agintc_splraise`: `setipl` must always be called because there is a race window where
/// the variable is updated before the mask is set an interrupt occurs in that window
/// without the mask always being set, the hardware might not get updated on the next
/// `splraise` completely messing up spl protection.
pub fn agintc_splraise(new: i32) -> i32 {
    let ci = curcpu();
    let old = ci.ci_cpl.get() as i32;

    let new = if old > new { old } else { new };

    agintc_setipl(new);
    old
}

/// `agintc_iack`: acknowledges the highest pending group 1 interrupt.
pub fn agintc_iack() -> u32 {
    let irq = icc_read!(ICC_IAR1);
    dsb_sy();
    irq as u32
}

/// `agintc_route_irq`: the `ic_route` hook.
pub fn agintc_route_irq(v: *mut c_void, enable: bool, ci: &CpuInfo) {
    let sc = &AGINTC;
    // SAFETY: an established handler of this controller (the hook's contract).
    let ih = unsafe { &*v.cast::<Intrhand>() };

    if enable {
        agintc_set_priority(sc, ih.ih_irq, handler(sc, ih.ih_irq).iq_irq_min.get());
        agintc_route(sc, ih.ih_irq, IRQ_ENABLE, Some(ci));
        agintc_intr_enable(sc, ih.ih_irq);
    }
}

/// `agintc_route`: an SPI's `GICD_IROUTER` names `ci`'s affinity.
pub fn agintc_route(_sc: &AgintcSoftc, irq: i32, _enable: bool, ci: Option<&CpuInfo>) {
    // XXX does not yet support 'participating node'
    if irq >= 32 {
        let Some(ci) = ci else {
            return;
        };
        #[cfg(feature = "debug")]
        kprintf!(
            "router {:x} irq {irq} val {:016x}\n",
            gicd_irouter(irq),
            ci.ci_mpidr.get() & MPIDR_AFF
        );
        let (iot, d) = dregs();
        bus_space_write_8(iot, d, gicd_irouter(irq), ci.ci_mpidr.get() & MPIDR_AFF);
    }
}

/// `agintc_intr_barrier`: the `ic_barrier` hook.
pub fn agintc_intr_barrier(cookie: *mut c_void) {
    // SAFETY: the cookie `agintc_intr_establish` handed out: an established handler, never
    // freed while its driver can still call the barrier.
    let ih = unsafe { &*cookie.cast::<Intrhand>() };
    crate::kern::kern_sched::sched_barrier(Some(ih.ih_ci));
}

/// `agintc_run_handler`: one handler, with its argument or the frame, under the kernel lock
/// unless it is `IPL_MPSAFE` or the interrupted level is at least `IPL_SCHED`.
pub fn agintc_run_handler(ih: &Intrhand, frame: *mut c_void, s: i32) {
    let need_lock =
        cfg!(feature = "multiprocessor") && ih.ih_flags.get() & IPL_MPSAFE == 0 && s < IPL_SCHED;
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

/// `agintc_irq_handler`: the IRQ dispatcher `arm_cpu_irq` calls.
pub fn agintc_irq_handler(frame: &mut Trapframe) {
    let sc = &AGINTC;

    let irq = agintc_iack() as i32;

    // DEBUG_AGINTC: not configured.

    if irq == 1023 {
        sc.sc_spur.ec_count.fetch_add(1, Ordering::Relaxed);
        return;
    }

    if (irq >= sc.sc_nintr.get() && irq < LPI_BASE) || irq >= LPI_BASE + sc.sc_nlpi.get() {
        return;
    }

    let frame = ptr::from_mut(frame).cast::<c_void>();

    if irq >= LPI_BASE {
        let Some(li) = lpi(sc, irq - LPI_BASE) else {
            return;
        };
        // SAFETY: an established LPI's handler, freed only after its slot is cleared.
        let Some(ih) = (unsafe { li.li_ih.as_ref() }) else {
            panic(format_args!(
                "agintc_irq_handler: lpi {irq} without handler"
            ));
        };

        let s = agintc_splraise(ih.ih_ipl);
        // SAFETY: the level is raised to the interrupt's, so only higher ones nest.
        unsafe { intr_enable() };
        agintc_run_handler(ih, frame, s);
        intr_disable();
        agintc_eoi(irq as u32);

        agintc_splx(s);
        return;
    }

    let iq = handler(sc, irq);
    let pri = iq.iq_irq_max.get();
    let s = agintc_splraise(pri);
    // SAFETY: as above.
    unsafe { intr_enable() };
    for ih in iq.iq_list.iter() {
        agintc_run_handler(ih, frame, s);
    }
    intr_disable();
    agintc_eoi(irq as u32);

    agintc_splx(s);
}

/// `agintc_intr_establish_fdt`: the `ic_establish` hook: 1st cell contains type: 0 SPI
/// (32-X), 1 PPI (16-31); 2nd cell contains the interrupt number; 3rd the trigger.
pub fn agintc_intr_establish_fdt(
    _cookie: *const (),
    cell: &[u32],
    level: i32,
    ci: Option<&'static CpuInfo>,
    func: IntrFn,
    arg: *mut c_void,
    name: &'static str,
) -> *mut c_void {
    let sc = &AGINTC;

    // 2nd cell contains the interrupt number
    let mut irq = cell[1] as i32;

    // 1st cell contains type: 0 SPI (32-X), 1 PPI (16-31)
    if cell[0] == 0 {
        irq += SPI_BASE;
    } else if cell[0] == 1 {
        irq += PPI_BASE;
    } else {
        // SAFETY: the device `agintc_attach` recorded, which lives forever.
        let name = unsafe { sc.sc_dev.get().as_ref() }.map_or("agintc", Device::xname);
        panic(format_args!("{name}: bogus interrupt type"));
    }

    // SPIs are only active-high level or low-to-high edge
    let type_ = if cell[2] & 0x3 != 0 {
        IST_EDGE_RISING
    } else {
        IST_LEVEL_HIGH
    };

    match agintc_intr_establish(irq, type_, level, ci, func, arg, Some(name)) {
        Some(ih) => ih.as_ptr().cast::<c_void>(),
        None => ptr::null_mut(),
    }
}

/// `agintc_intr_establish`: registers `func(arg)` for an SGI, PPI, SPI or LPI at `level`.
pub fn agintc_intr_establish(
    irqno: i32,
    type_: i32,
    level: i32,
    ci: Option<&'static CpuInfo>,
    func: IntrFn,
    arg: *mut c_void,
    name: Option<&'static str>,
) -> Option<NonNull<Intrhand>> {
    let sc = &AGINTC;

    if irqno < 0
        || (irqno >= sc.sc_nintr.get() && irqno < LPI_BASE)
        || irqno >= LPI_BASE + sc.sc_nlpi.get()
    {
        panic(format_args!(
            "agintc_intr_establish: bogus irqnumber {irqno}: {}",
            name.unwrap_or("")
        ));
    }

    let ci = ci.unwrap_or_else(cpu_info_primary);

    let ih = malloc(size_of::<Intrhand>(), M_DEVBUF, M_WAITOK)?.cast::<Intrhand>();
    // SAFETY: a fresh allocation of the right size and alignment, written once before use.
    unsafe {
        ih.write(Intrhand {
            ih_list: TailqEntry::new(),
            ih_func: func,
            ih_arg: arg,
            ih_ipl: level & IPL_IRQMASK,
            ih_flags: Cell::new(level & IPL_FLAGMASK),
            ih_type: type_,
            ih_irq: irqno,
            ih_count: Evcount::new(),
            ih_name: name,
            ih_ci: ci,
        });
    }
    // SAFETY: as above; the handler lives until disestablished.
    let hand: &'static Intrhand = unsafe { ih.as_ref() };

    let psw = intr_disable();

    if irqno < LPI_BASE {
        let iq = handler(sc, irqno);
        if !iq.iq_list.is_empty() && !ptr::eq(iq.iq_ci.get(), ci) {
            // SAFETY: `psw` is this CPU's DAIF from `intr_disable`.
            unsafe { intr_restore(psw) };
            free(ih.cast::<u8>(), M_DEVBUF, size_of::<Intrhand>());
            return None;
        }
        // SAFETY: a new handler, not on any queue; interrupts are disabled.
        unsafe { iq.iq_list.insert_tail(hand) };
        iq.iq_ci.set(ptr::from_ref(ci));
    }

    if let Some(name) = name {
        evcount_attach(
            &hand.ih_count,
            name,
            ptr::from_ref(&hand.ih_irq).cast::<()>(),
        );
    }

    #[cfg(feature = "debug")]
    kprintf!(
        "agintc_intr_establish: irq {irqno} level {level} [{}]\n",
        name.unwrap_or("")
    );

    if irqno < LPI_BASE {
        agintc_intr_config(sc, irqno, type_);
        agintc_calc_irq(sc, irqno);
    } else {
        let p = prop(sc);
        let i = (irqno - LPI_BASE) as usize;
        p.set_u8(
            i,
            agintc_prival(hand.ih_ipl, 4) | GICR_PROP_GROUP1 | GICR_PROP_ENABLE,
        );
        // Make globally visible.
        prop_sync(p, i);
    }

    // SAFETY: as above.
    unsafe { intr_restore(psw) };
    Some(ih)
}

/// `agintc_intr_disestablish`: the `ic_disestablish` hook.
pub fn agintc_intr_disestablish(cookie: *mut c_void) {
    let sc = &AGINTC;
    let Some(ih) = NonNull::new(cookie.cast::<Intrhand>()) else {
        return;
    };
    // SAFETY: an established handler (the hook's contract), alive until freed below.
    let hand = unsafe { ih.as_ref() };
    let irqno = hand.ih_irq;

    let psw = intr_disable();

    if irqno < LPI_BASE {
        // SAFETY: the handler is on its interrupt's queue; interrupts are disabled.
        unsafe { handler(sc, irqno).iq_list.remove(hand) };
        agintc_calc_irq(sc, irqno);

        // In case this is an MBI, free it
        for i in 0..sc.sc_mbi_nranges.get() {
            let mr = mbi_range(sc, i);
            if irqno < mr.mr_base {
                continue;
            }
            if irqno >= mr.mr_base + mr.mr_span {
                break;
            }
            let slot = mr.slot(irqno - mr.mr_base);
            if !slot.get().is_null() {
                slot.set(ptr::null_mut());
            }
        }
    } else {
        let p = prop(sc);
        let i = (irqno - LPI_BASE) as usize;
        p.set_u8(i, 0);

        // Make globally visible.
        prop_sync(p, i);
    }

    if hand.ih_name.is_some() {
        evcount_detach(&hand.ih_count);
    }

    // SAFETY: `psw` is this CPU's DAIF from `intr_disable`.
    unsafe { intr_restore(psw) };

    free(ih.cast::<u8>(), M_DEVBUF, 0);
}

/// `agintc_intr_set_wakeup`: the `ic_set_wakeup` hook.
pub fn agintc_intr_set_wakeup(cookie: *mut c_void) {
    // SAFETY: an established handler (the hook's contract).
    let ih = unsafe { &*cookie.cast::<Intrhand>() };
    ih.ih_flags.set(ih.ih_flags.get() | IPL_WAKEUP);
}

/// `agintc_intr_establish_mbi`: the GIC's `ic_establish_msi` hook when it has MBI ranges:
/// takes a free message based SPI, establishes it (edge-triggered) and hands back the
/// doorbell and the SPI number as the MSI's address and data.
#[allow(clippy::too_many_arguments)] // the C's signature
pub fn agintc_intr_establish_mbi(
    _self: *const (),
    addr: &mut u64,
    data: &mut u64,
    level: i32,
    ci: Option<&'static CpuInfo>,
    func: IntrFn,
    arg: *mut c_void,
    name: &'static str,
) -> *mut c_void {
    let sc = &AGINTC;
    let ci = ci.unwrap_or_else(cpu_info_primary);

    for i in 0..sc.sc_mbi_nranges.get() {
        let mr = mbi_range(sc, i);
        for j in 0..mr.mr_span {
            let slot = mr.slot(j);
            if !slot.get().is_null() {
                continue;
            }

            let Some(cookie) = agintc_intr_establish(
                mr.mr_base + j,
                IST_EDGE_RISING,
                level,
                Some(ci),
                func,
                arg,
                Some(name),
            ) else {
                return ptr::null_mut();
            };

            *addr = sc.sc_mbi_addr.get();
            *data = (mr.mr_base + j) as u64;

            slot.set(cookie.as_ptr().cast());
            return cookie.as_ptr().cast();
        }
    }

    ptr::null_mut()
}

/// `agintc_eoi`: ends the interrupt (priority drop and deactivation).
pub fn agintc_eoi(eoi: u32) {
    icc_write!(ICC_EOIR1, u64::from(eoi));
    isb();
}

/// `agintc_d_wait_rwp`: waits for a distributor register write to take effect.
pub fn agintc_d_wait_rwp(_sc: &AgintcSoftc) {
    let (iot, d) = dregs();
    let mut count = 100_000;
    let mut v;

    loop {
        v = bus_space_read_4(iot, d, GICD_CTLR);
        count -= 1;
        if count == 0 || v & GICD_CTLR_RWP == 0 {
            break;
        }
    }

    if count == 0 {
        panic(format_args!("agintc_d_wait_rwp: RWP timed out 0x08{v:x}"));
    }
}

/// `agintc_r_wait_rwp`: waits for a write to this CPU's redistributor to take effect.
pub fn agintc_r_wait_rwp(sc: &AgintcSoftc) {
    let iot = dregs().0;
    let hwcpu = cpuremap(sc, curcpu());
    let r = r_ioh(sc, hwcpu);
    let mut count = 100_000;
    let mut v;

    loop {
        v = bus_space_read_4(iot, r, GICR_CTLR);
        count -= 1;
        if count == 0 || v & GICR_CTLR_RWP == 0 {
            break;
        }
    }

    if count == 0 {
        panic(format_args!("agintc_r_wait_rwp: RWP timed out 0x08{v:x}"));
    }
}

/// `agintc_ipi_ddb`: another CPU entered ddb.
#[cfg(feature = "multiprocessor")]
pub fn agintc_ipi_ddb(_v: *mut c_void) -> i32 {
    // XXX
    db_enter();
    1
}

/// `agintc_ipi_halt`: ends the IPI and stops this CPU (`cpu_halt`) at `IPL_NONE`.
#[cfg(feature = "multiprocessor")]
pub fn agintc_ipi_halt(v: *mut c_void) -> i32 {
    // SAFETY: the IPI handler's argument, the static softc.
    let sc = unsafe { &*v.cast::<AgintcSoftc>() };
    let old = curcpu().ci_cpl.get() as i32;

    intr_disable();
    agintc_eoi(sc.sc_ipi_num.load(Ordering::Relaxed) as u32);
    agintc_setipl(IPL_NONE);

    cpu_halt();

    agintc_setipl(old);
    // SAFETY: back from `cpu_halt`, at the level the IPI interrupted.
    unsafe { intr_enable() };
    1
}

/// `agintc_ipi_handler`: the IPI's interrupt handler, at `IPL_IPI`, without the kernel
/// lock: runs the reasons posted for this CPU (`ARM_IPI_NOP` posts none).
#[cfg(feature = "multiprocessor")]
pub fn agintc_ipi_handler(v: *mut c_void) -> i32 {
    // SAFETY: as in `agintc_ipi_halt`.
    let sc = unsafe { &*v.cast::<AgintcSoftc>() };
    let ci = curcpu();
    let reason = &sc.sc_ipi_reason[ci.ci_cpuid.get() as usize];

    let mut reasons = reason.load(Ordering::Relaxed);
    if reasons != 0 {
        reasons = reason.swap(0, Ordering::AcqRel);
        if reasons & (1 << ARM_IPI_DDB) != 0 {
            agintc_ipi_ddb(v);
        }
        if reasons & (1 << ARM_IPI_HALT) != 0 {
            agintc_ipi_halt(v);
        }
        // ARM_IPI_XCALL: NXCALL is 0 on arm64 (arm_cpu_xcall_dispatch).
    }

    1
}

/// `agintc_send_ipi`: `intr_send_ipi_func`: posts `reason` for `ci` and raises the IPI's
/// SGI at it alone.
#[cfg(feature = "multiprocessor")]
pub fn agintc_send_ipi(ci: &CpuInfo, reason: i32) {
    let sc = &AGINTC;

    if reason == ARM_IPI_NOP {
        if ptr::eq(ci, curcpu()) {
            return;
        }
    } else {
        sc.sc_ipi_reason[ci.ci_cpuid.get() as usize].fetch_or(1 << reason, Ordering::Release);
    }

    // will only send 1 cpu
    let sendmask = agintc_sgi1r(ci.ci_mpidr.get(), sc.sc_ipi_num.load(Ordering::Relaxed));

    // The reason (and whatever the sender prepared) before the interrupt.
    // SAFETY: a barrier.
    unsafe { core::arch::asm!("dsb ish", options(nostack, preserves_flags)) };
    icc_write!(ICC_SGI1R, sendmask);
    isb();
}

/// `agintc_msi_match`: an ITS, unless ACPI describes it on a Qualcomm Oryon (X1E), where
/// MSIs don't work in ACPI mode.
pub fn agintc_msi_match(_parent: Option<&Device>, _cfdata: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: `agintcmsi` attaches at `fdt`, whose buses hand over a `FdtAttachArgs`.
    let faa = unsafe { &*aux.cast::<FdtAttachArgs<'_>>() };

    // XXX For some reason MSIs don't work on Qualcomm X1E SoCs in ACPI mode. So skip
    // attaching the ITS in that case. MSIs work fine when booting with a DTB.
    let midr = curcpu().ci_midr.get();
    if OF_is_compatible(OF_peer(0), b"openbsd,acpi")
        && cpu_impl(midr) == CPU_IMPL_QCOM
        && cpu_part(midr) == CPU_PART_ORYON
    {
        return 0;
    }

    i32::from(OF_is_compatible(faa.fa_node, b"arm,gic-v3-its"))
}

/// The page size a `GITS_BASER`'s `Page_Size` field names, in bytes.
const fn gits_baser_pgsz(baser: u64) -> usize {
    match baser & GITS_BASER_PGSZ_MASK {
        GITS_BASER_PGSZ_16K => 4 * PAGE_SIZE,
        GITS_BASER_PGSZ_64K => 16 * PAGE_SIZE,
        _ => PAGE_SIZE,
    }
}

/// `roundup(x, y)`.
const fn roundup(x: usize, y: usize) -> usize {
    x.div_ceil(y) * y
}

/// The device table's layout: its size in bytes and the highest device ID it can map, for
/// `devbits` device ID bits, `dte_sz`-byte entries and `pgsz`-byte pages, flat or
/// `indirect` (a level of 8-byte pointers to pages of entries); clamped to the 256 pages a
/// `GITS_BASER` can describe.
const fn agintc_msi_dtt_layout(
    devbits: i32,
    dte_sz: usize,
    pgsz: usize,
    indirect: bool,
) -> (usize, u32) {
    let mut size;
    if indirect {
        size = 1usize << devbits;
        size /= pgsz / dte_sz;
        size *= size_of::<u64>();
        size = roundup(size, pgsz);
    } else {
        size = roundup((1usize << devbits) * dte_sz, pgsz);
    }

    // Clamp down to maximum configurable num pages
    if size / pgsz > GITS_BASER_SZ_MASK as usize + 1 {
        size = (GITS_BASER_SZ_MASK as usize + 1) * pgsz;
    }

    // Calculate max deviceid based off configured size
    let max = if indirect {
        (size / size_of::<u64>()) * (pgsz / dte_sz) - 1
    } else {
        size / dte_sz - 1
    };
    (size, max as u32)
}

/// Probes the largest page size `GITS_BASER(i)` takes (64 KB, 16 KB, 4 KB) and returns the
/// register as it reads back.
fn agintc_msi_baser_pgsz(iot: BusSpaceTag, ioh: BusSpaceHandle, i: usize) -> u64 {
    // Determine the maximum supported page size.
    for pgsz in [GITS_BASER_PGSZ_64K, GITS_BASER_PGSZ_16K] {
        let cur = bus_space_read_8(iot, ioh, gits_baser(i));
        bus_space_write_8(
            iot,
            ioh,
            gits_baser(i),
            (cur & !GITS_BASER_PGSZ_MASK) | pgsz,
        );
        let got = bus_space_read_8(iot, ioh, gits_baser(i));
        if got & GITS_BASER_PGSZ_MASK == pgsz {
            return got;
        }
    }

    let cur = bus_space_read_8(iot, ioh, gits_baser(i));
    bus_space_write_8(
        iot,
        ioh,
        gits_baser(i),
        (cur & !GITS_BASER_PGSZ_MASK) | GITS_BASER_PGSZ_4K,
    );
    bus_space_read_8(iot, ioh, gits_baser(i))
}

/// `agintc_msi_attach`: maps the ITS, sets up its command queue and its device and
/// collection tables, enables it, maps one collection per CPU and registers it as an MSI
/// controller.
pub fn agintc_msi_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: `agintcmsi_ca` made the device, as an `AgintcMsiSoftc`.
    let sc = unsafe { self_.softc::<AgintcMsiSoftc>() };
    // SAFETY: as in `agintc_msi_match`.
    let faa = unsafe { &*aux.cast::<FdtAttachArgs<'_>>() };
    let gic = &AGINTC;

    let Some(reg) = faa.fa_reg.first() else {
        kprintf!(": no registers\n");
        return;
    };

    sc.sc_iot.set(Some(faa.fa_iot));
    // SAFETY: the device tree's registers of the ITS, which nothing else drives.
    let Ok(ioh) = (unsafe { bus_space_map(faa.fa_iot, reg.addr as usize, reg.size as usize, 0) })
    else {
        kprintf!(": can't map registers\n");
        return;
    };
    sc.sc_ioh.set(Some(ioh));
    sc.sc_dmat.set(Some(faa.fa_dmat));
    let iot = faa.fa_iot;

    sc.sc_msi_addr.set(reg.addr + GITS_TRANSLATER as u64);
    let mut pre_its = [0u32; 2];
    if OF_getpropintarray(faa.fa_node, b"socionext,synquacer-pre-its", &mut pre_its)
        == size_of::<[u32; 2]>() as i32
    {
        sc.sc_msi_addr.set(u64::from(pre_its[0]));
        sc.sc_msi_delta.set(4);
    }

    let typer = bus_space_read_8(iot, ioh, GITS_TYPER);
    if typer & GITS_TYPER_PHYS == 0 || typer & GITS_TYPER_PTA != 0 {
        kprintf!(": unsupported type 0x{typer:016x}\n");
        agintc_msi_attach_unmap(sc, reg.size as usize);
        return;
    }
    sc.sc_ite_sz.set(gits_typer_ite_sz(typer) as u8 + 1);
    sc.sc_devbits.set(gits_typer_devbits(typer) as i32 + 1);
    if typer & GITS_TYPER_CIL != 0 {
        sc.sc_cidbits.set(gits_typer_cidbits(typer) as i32 + 1);
    } else {
        sc.sc_cidbits.set(16);
    }

    // Set up command queue.
    let Some(cmdq) = agintc_dmamem_alloc(sc.dmat(), GITS_CMDQ_SIZE, GITS_CMDQ_SIZE) else {
        kprintf!(": can't alloc command queue\n");
        agintc_msi_attach_unmap(sc, reg.size as usize);
        return;
    };
    sc.sc_cmdq.set(Some(cmdq));
    bus_space_write_8(
        iot,
        ioh,
        GITS_CBASER,
        cmdq.dva()
            | GITS_CBASER_IC_NORM_NC
            | ((GITS_CMDQ_SIZE / PAGE_SIZE) as u64 - 1)
            | GITS_CBASER_VALID,
    );

    // Set up device translation table.
    for i in 0..GITS_NUM_BASER {
        let baser = bus_space_read_8(iot, ioh, gits_baser(i));
        if baser & GITS_BASER_TYPE_MASK != GITS_BASER_TYPE_DEVICE {
            continue;
        }

        let baser = agintc_msi_baser_pgsz(iot, ioh, i);
        sc.sc_dtt_pgsz.set(gits_baser_pgsz(baser));
        let pgsz = sc.sc_dtt_pgsz.get();

        // Calculate table size.
        sc.sc_dte_sz.set(gits_baser_tte_sz(baser) as u8 + 1);
        let dte_sz = usize::from(sc.sc_dte_sz.get());
        let (flat, _) = agintc_msi_dtt_layout(sc.sc_devbits.get(), dte_sz, pgsz, false);

        // Might make sense to go indirect
        if flat > 2 * pgsz {
            bus_space_write_8(iot, ioh, gits_baser(i), baser | GITS_BASER_INDIRECT);
            if bus_space_read_8(iot, ioh, gits_baser(i)) & GITS_BASER_INDIRECT != 0 {
                sc.sc_dtt_indirect.set(true);
            }
        }
        let (size, max) =
            agintc_msi_dtt_layout(sc.sc_devbits.get(), dte_sz, pgsz, sc.sc_dtt_indirect.get());
        sc.sc_deviceid_max.set(max);

        // Allocate table.
        let Some(dtt) = agintc_dmamem_alloc(sc.dmat(), size, pgsz) else {
            kprintf!(": can't alloc translation table\n");
            agintc_msi_attach_unmap(sc, reg.size as usize);
            return;
        };
        sc.sc_dtt.set(Some(dtt));

        // Configure table.
        let dtt_pa = dtt.dva();
        kassert!(dtt_pa & GITS_BASER_PA_MASK == dtt_pa);
        bus_space_write_8(
            iot,
            ioh,
            gits_baser(i),
            GITS_BASER_IC_NORM_NC
                | (baser & GITS_BASER_PGSZ_MASK)
                | dtt_pa
                | ((size / pgsz) as u64 - 1)
                | if sc.sc_dtt_indirect.get() {
                    GITS_BASER_INDIRECT
                } else {
                    0
                }
                | GITS_BASER_VALID,
        );
    }

    // Set up collection translation table.
    for i in 0..GITS_NUM_BASER {
        let baser = bus_space_read_8(iot, ioh, gits_baser(i));
        if baser & GITS_BASER_TYPE_MASK != GITS_BASER_TYPE_COLL {
            continue;
        }

        let baser = agintc_msi_baser_pgsz(iot, ioh, i);
        sc.sc_ctt_pgsz.set(gits_baser_pgsz(baser));
        let pgsz = sc.sc_ctt_pgsz.get();

        // Calculate table size.
        sc.sc_cte_sz.set(gits_baser_tte_sz(baser) as u8 + 1);
        let size = roundup(
            (1usize << sc.sc_cidbits.get()) * usize::from(sc.sc_cte_sz.get()),
            pgsz,
        );

        // Allocate table.
        let Some(ctt) = agintc_dmamem_alloc(sc.dmat(), size, pgsz) else {
            kprintf!(": can't alloc translation table\n");
            agintc_msi_attach_unmap(sc, reg.size as usize);
            return;
        };
        sc.sc_ctt.set(Some(ctt));

        // Configure table.
        let ctt_pa = ctt.dva();
        kassert!(ctt_pa & GITS_BASER_PA_MASK == ctt_pa);
        bus_space_write_8(
            iot,
            ioh,
            gits_baser(i),
            GITS_BASER_IC_NORM_NC
                | (baser & GITS_BASER_PGSZ_MASK)
                | ctt_pa
                | ((size / pgsz) as u64 - 1)
                | GITS_BASER_VALID,
        );
    }

    // Enable ITS.
    bus_space_write_4(iot, ioh, GITS_CTLR, GITS_CTLR_ENABLED);

    sc.sc_msi_devices.init();

    // Create one collection per core.
    let ncpus = crate::kern::init_main::NCPUS.load(Ordering::Relaxed);
    kassert!(ncpus <= gic.sc_num_redist.get());
    for i in 0..ncpus {
        let hwcpu = gic.sc_cpuremap[i as usize].load(Ordering::Relaxed);
        let cmd = GitsCmd {
            cmd: MAPC,
            dw2: GITS_CMD_VALID | (u64::from(processor(gic, hwcpu)) << 16) | i as u64,
            ..GitsCmd::default()
        };
        agintc_msi_send_cmd(sc, &cmd);
        agintc_msi_wait_cmd(sc);
    }

    kprintf!("\n");

    // SAFETY: the controller is not registered yet, so nothing else reads it; it is written
    // once, here.
    let ic = unsafe { &mut *sc.sc_ic.get() };
    ic.ic_node.set(faa.fa_node);
    ic.ic_cookie.set(ptr::from_ref(sc).cast::<()>());
    ic.ic_establish_msi = Some(agintc_intr_establish_msi);
    ic.ic_disestablish = Some(agintc_intr_disestablish_msi);
    ic.ic_barrier = Some(agintc_intr_barrier_msi);
    ic.ic_gic_its_id
        .set(OF_getpropint(faa.fa_node, b"openbsd,gic-its-id", 0));
    // SAFETY: the softc lives as long as the kernel (no detach); the controller is not
    // written again.
    arm_intr_register_fdt(unsafe { &*sc.sc_ic.get() });
}

/// `agintc_msi_attach`'s `unmap:` path.
fn agintc_msi_attach_unmap(sc: &AgintcMsiSoftc, size: usize) {
    let (iot, ioh) = sc.regs();
    if let Some(dtt) = sc.sc_dtt.take() {
        agintc_dmamem_free(sc.dmat(), dtt);
    }
    if let Some(cmdq) = sc.sc_cmdq.take() {
        agintc_dmamem_free(sc.dmat(), cmdq);
    }

    bus_space_unmap(iot, ioh, size);
    sc.sc_ioh.set(None);
}

/// `agintc_msi_send_cmd`: queues a command and moves `GITS_CWRITER` past it.
pub fn agintc_msi_send_cmd(sc: &AgintcMsiSoftc, cmd: &GitsCmd) {
    let (iot, ioh) = sc.regs();
    let queue = sc.cmdq();
    let idx = usize::from(sc.sc_cmdidx.get());

    for (w, v) in cmd.words().into_iter().enumerate() {
        queue.set_u64(idx * 4 + w, v);
    }

    // Make globally visible.
    cpu_dcache_wb_range(queue.kva() as usize + idx * GITS_CMD_SIZE, GITS_CMD_SIZE);
    dsb_sy();

    let next = (idx + 1) % GITS_CMDQ_NENTRIES;
    sc.sc_cmdidx.set(next as u16);
    bus_space_write_8(iot, ioh, GITS_CWRITER, (next * GITS_CMD_SIZE) as u64);
}

/// `agintc_msi_wait_cmd`: waits (about a millisecond) for the ITS to read up to
/// `GITS_CWRITER`.
pub fn agintc_msi_wait_cmd(sc: &AgintcMsiSoftc) {
    let (iot, ioh) = sc.regs();
    let want = usize::from(sc.sc_cmdidx.get()) * GITS_CMD_SIZE;

    let mut timo = 1000;
    while timo > 0 {
        let creadr = bus_space_read_8(iot, ioh, GITS_CREADR);
        if creadr == want as u64 {
            break;
        }
        delay(1);
        timo -= 1;
    }
    if timo == 0 {
        kprintf!("{}: command queue timeout\n", sc.sc_dev.xname());
    }
}

/// `agintc_msi_create_device_table`: with an indirect device table, the page of entries for
/// `deviceid`'s group, allocated on first use.
pub fn agintc_msi_create_device_table(sc: &AgintcMsiSoftc, deviceid: u32) -> Result<(), Errno> {
    let Some(table) = sc.sc_dtt.get() else {
        return Err(Errno::ENXIO);
    };
    let pgsz = sc.sc_dtt_pgsz.get();
    let idx = deviceid as usize / (pgsz / usize::from(sc.sc_dte_sz.get()));

    // Out of bounds
    if deviceid > sc.sc_deviceid_max.get() {
        return Err(Errno::ENXIO);
    }

    // No need to adjust
    if !sc.sc_dtt_indirect.get() {
        return Ok(());
    }

    // Table already allocated
    if table.get_u64(idx) != 0 {
        return Ok(());
    }

    // FIXME: leaks
    let Some(dtt) = agintc_dmamem_alloc(sc.dmat(), pgsz, pgsz) else {
        return Err(Errno::ENOMEM);
    };

    let dtt_pa = dtt.dva();
    kassert!(dtt_pa & GITS_BASER_PA_MASK == dtt_pa);
    table.set_u64(idx, dtt_pa | GITS_BASER_VALID);
    cpu_dcache_wb_range(table.kva() as usize + idx * 8, 8);
    dsb_sy();
    Ok(())
}

/// `agintc_msi_create_device`: maps `deviceid` to a new interrupt translation table (32
/// events).
pub fn agintc_msi_create_device(
    sc: &AgintcMsiSoftc,
    deviceid: u32,
) -> Option<&'static AgintcMsiDevice> {
    if deviceid > sc.sc_deviceid_max.get() {
        return None;
    }

    if agintc_msi_create_device_table(sc, deviceid).is_err() {
        return None;
    }

    let itt = agintc_dmamem_alloc(sc.dmat(), 32 * usize::from(sc.sc_ite_sz.get()), PAGE_SIZE)?;
    let md = malloc(size_of::<AgintcMsiDevice>(), M_DEVBUF, M_ZERO | M_WAITOK)?
        .cast::<AgintcMsiDevice>();
    // SAFETY: a fresh allocation of the right size and alignment, written once before use;
    // it lives as long as the ITS (devices are never removed).
    let md: &'static AgintcMsiDevice = unsafe {
        md.write(AgintcMsiDevice {
            md_list: ListEntry::new(),
            md_deviceid: deviceid,
            md_events: Cell::new(0),
            md_itt: itt,
        });
        md.as_ref()
    };
    // SAFETY: a new device, on no list; under the kernel lock, as every establish.
    unsafe { sc.sc_msi_devices.insert_head(md) };

    let cmd = GitsCmd {
        cmd: MAPD,
        deviceid,
        eventid: 4, // size
        dw2: itt.dva() | GITS_CMD_VALID,
        ..GitsCmd::default()
    };
    agintc_msi_send_cmd(sc, &cmd);
    agintc_msi_wait_cmd(sc);

    Some(md)
}

/// `agintc_msi_find_device`: the ITS's device for `deviceid`, made if new.
pub fn agintc_msi_find_device(
    sc: &AgintcMsiSoftc,
    deviceid: u32,
) -> Option<&'static AgintcMsiDevice> {
    if let Some(md) = sc
        .sc_msi_devices
        .iter()
        .find(|md| md.md_deviceid == deviceid)
    {
        // SAFETY: the list's devices are never freed.
        return Some(unsafe { &*ptr::from_ref(md) });
    }

    agintc_msi_create_device(sc, deviceid)
}

/// The ITS and the redistributor a translation targets, for its `SYNC`.
fn agintc_msi_li(li: &AgintcLpiInfo) -> (&'static AgintcMsiSoftc, u64) {
    let gic = &AGINTC;
    // SAFETY: an established LPI names the ITS that made it, which lives forever.
    let sc = unsafe { &*li.li_msic };
    let hwcpu = cpuremap(gic, li.li_ci);
    (sc, u64::from(processor(gic, hwcpu)) << 16)
}

/// `agintc_msi_discard`: drops the translation of an MSI.
pub fn agintc_msi_discard(li: &AgintcLpiInfo) {
    let (sc, target) = agintc_msi_li(li);

    let cmd = GitsCmd {
        cmd: DISCARD,
        deviceid: li.li_deviceid,
        eventid: li.li_eventid,
        ..GitsCmd::default()
    };
    agintc_msi_send_cmd(sc, &cmd);

    let cmd = GitsCmd {
        cmd: SYNC,
        dw2: target,
        ..GitsCmd::default()
    };
    agintc_msi_send_cmd(sc, &cmd);
    agintc_msi_wait_cmd(sc);
}

/// `agintc_msi_inv`: has the redistributor reread an LPI's configuration.
pub fn agintc_msi_inv(li: &AgintcLpiInfo) {
    let (sc, target) = agintc_msi_li(li);

    let cmd = GitsCmd {
        cmd: INV,
        deviceid: li.li_deviceid,
        eventid: li.li_eventid,
        ..GitsCmd::default()
    };
    agintc_msi_send_cmd(sc, &cmd);

    let cmd = GitsCmd {
        cmd: SYNC,
        dw2: target,
        ..GitsCmd::default()
    };
    agintc_msi_send_cmd(sc, &cmd);
    agintc_msi_wait_cmd(sc);
}

/// `agintc_intr_establish_msi`: the ITS's `ic_establish_msi` hook: `data` comes in as the
/// device ID, `addr` as the wanted event ID (0: any); takes a free event and a free LPI,
/// establishes the LPI and maps the event to it (`MAPTI` to `ci`'s collection), and hands
/// back the doorbell and the event ID. The cookie is the LPI's slot.
#[allow(clippy::too_many_arguments)] // the C's signature
pub fn agintc_intr_establish_msi(
    self_: *const (),
    addr: &mut u64,
    data: &mut u64,
    level: i32,
    ci: Option<&'static CpuInfo>,
    func: IntrFn,
    arg: *mut c_void,
    name: &'static str,
) -> *mut c_void {
    // SAFETY: the cookie is the ITS's softc (`agintc_msi_attach`), which lives forever.
    let sc = unsafe { &*self_.cast::<AgintcMsiSoftc>() };
    let gic = &AGINTC;
    let deviceid = *data as u32;

    let ci = ci.unwrap_or_else(cpu_info_primary);
    let hwcpu = cpuremap(gic, ci);

    let Some(md) = agintc_msi_find_device(sc, deviceid) else {
        return ptr::null_mut();
    };

    let bit = |e: u32| 1u32.checked_shl(e).unwrap_or(0);
    let mut eventid = *addr as u32;
    if eventid > 0 && md.md_events.get() & bit(eventid) != 0 {
        return ptr::null_mut();
    }
    while eventid < 32 {
        if md.md_events.get() & bit(eventid) == 0 {
            md.md_events.set(md.md_events.get() | bit(eventid));
            break;
        }
        eventid += 1;
    }
    if eventid >= 32 {
        return ptr::null_mut();
    }

    for i in 0..gic.sc_nlpi.get() {
        let slot = lpi_slot(gic, i);
        if !slot.get().is_null() {
            continue;
        }

        let Some(li) = malloc(size_of::<AgintcLpiInfo>(), M_DEVBUF, M_WAITOK | M_ZERO) else {
            return ptr::null_mut();
        };
        let li = li.cast::<AgintcLpiInfo>();
        // SAFETY: a fresh allocation, written once before use.
        unsafe {
            li.write(AgintcLpiInfo {
                li_msic: ptr::from_ref(sc),
                li_ci: ci,
                li_deviceid: deviceid,
                li_eventid: eventid,
                li_ih: ptr::null_mut(),
            });
        }
        slot.set(li.as_ptr());
        let ih = agintc_intr_establish(
            LPI_BASE + i,
            IST_EDGE_RISING,
            level,
            Some(ci),
            func,
            arg,
            Some(name),
        );
        let Some(ih) = ih else {
            slot.set(ptr::null_mut());
            free(li.cast(), M_DEVBUF, size_of::<AgintcLpiInfo>());
            return ptr::null_mut();
        };
        // SAFETY: the info just written, which only this establish has seen.
        unsafe { (*li.as_ptr()).li_ih = ih.as_ptr() };

        let cmd = GitsCmd {
            cmd: MAPTI,
            deviceid,
            eventid,
            intid: (LPI_BASE + i) as u32,
            dw2: u64::from(ci.ci_cpuid.get()),
            ..GitsCmd::default()
        };
        agintc_msi_send_cmd(sc, &cmd);

        let cmd = GitsCmd {
            cmd: SYNC,
            dw2: u64::from(processor(gic, hwcpu)) << 16,
            ..GitsCmd::default()
        };
        agintc_msi_send_cmd(sc, &cmd);
        agintc_msi_wait_cmd(sc);

        *addr = sc.sc_msi_addr.get() + u64::from(deviceid) * sc.sc_msi_delta.get() as u64;
        *data = u64::from(eventid);
        return ptr::from_ref(slot).cast_mut().cast::<c_void>();
    }

    ptr::null_mut()
}

/// The LPI in an MSI's slot (`*(void **)cookie`).
fn msi_slot(cookie: *mut c_void) -> &'static Cell<*mut AgintcLpiInfo> {
    // SAFETY: the cookie `agintc_intr_establish_msi` handed out: a slot of `sc_lpi`, which
    // is never freed.
    unsafe { &*cookie.cast::<Cell<*mut AgintcLpiInfo>>() }
}

/// `agintc_intr_disestablish_msi`: the ITS's `ic_disestablish` hook.
pub fn agintc_intr_disestablish_msi(cookie: *mut c_void) {
    let slot = msi_slot(cookie);
    let Some(li) = NonNull::new(slot.get()) else {
        return;
    };
    // SAFETY: the info `agintc_intr_establish_msi` made, freed below.
    let info = unsafe { li.as_ref() };

    agintc_intr_disestablish(info.li_ih.cast());
    agintc_msi_discard(info);
    agintc_msi_inv(info);

    free(li.cast(), M_DEVBUF, size_of::<AgintcLpiInfo>());
    slot.set(ptr::null_mut());
}

/// `agintc_intr_barrier_msi`: the ITS's `ic_barrier` hook.
pub fn agintc_intr_barrier_msi(cookie: *mut c_void) {
    // SAFETY: as in `agintc_intr_disestablish_msi`; the info lives while established.
    if let Some(li) = unsafe { msi_slot(cookie).get().as_ref() } {
        agintc_intr_barrier(li.li_ih.cast());
    }
}

/// `agintc_dmamem_alloc`: `size` bytes of zeroed, physically contiguous memory aligned to
/// `align`, mapped uncached and loaded in a map of one segment.
pub fn agintc_dmamem_alloc(
    dmat: BusDmaTag,
    size: usize,
    align: usize,
) -> Option<&'static AgintcDmamem> {
    let adm = malloc(size_of::<AgintcDmamem>(), M_DEVBUF, M_WAITOK | M_ZERO)?;

    let Ok(map) = bus_dmamap_create(dmat, size, 1, size, 0, BUS_DMA_WAITOK | BUS_DMA_ALLOCNOW)
    else {
        // admfree:
        free(adm, M_DEVBUF, size_of::<AgintcDmamem>());
        return None;
    };
    let destroy = |adm: NonNull<u8>| {
        // SAFETY: the map created above, unused.
        unsafe { bus_dmamap_destroy(dmat, NonNull::from(map)) };
        free(adm, M_DEVBUF, size_of::<AgintcDmamem>());
    };

    let mut segs = [BusDmaSegment::default(); 1];
    let Ok(nsegs) = bus_dmamem_alloc(
        dmat,
        size,
        align,
        0,
        &mut segs,
        BUS_DMA_WAITOK | BUS_DMA_ZERO,
    ) else {
        destroy(adm);
        return None;
    };

    let Ok(kva) = bus_dmamem_map(
        dmat,
        &mut segs[..nsegs],
        size,
        BUS_DMA_WAITOK | BUS_DMA_NOCACHE,
    ) else {
        // free:
        // SAFETY: the segment allocated above, not mapped.
        unsafe { bus_dmamem_free(dmat, &segs[..nsegs]) };
        destroy(adm);
        return None;
    };

    // SAFETY: the segment allocated above, which stays until `agintc_dmamem_free`.
    if unsafe { bus_dmamap_load_raw(dmat, map, &segs[..nsegs], size, BUS_DMA_WAITOK) }.is_err() {
        // unmap:
        // SAFETY: the mapping and the segment made above, unused.
        unsafe {
            bus_dmamem_unmap(dmat, kva, size);
            bus_dmamem_free(dmat, &segs[..nsegs]);
        }
        destroy(adm);
        return None;
    }

    // Make globally visible.
    cpu_dcache_wb_range(kva.as_ptr() as usize, size);
    dsb_sy();

    let adm = adm.cast::<AgintcDmamem>();
    // SAFETY: a fresh allocation of `size_of::<AgintcDmamem>()` bytes (malloc aligns it),
    // written whole before any use; it lives until `agintc_dmamem_free`.
    unsafe {
        adm.as_ptr().write(AgintcDmamem {
            adm_map: map,
            adm_seg: segs[0],
            adm_size: size,
            adm_kva: kva,
        });
        Some(&*adm.as_ptr())
    }
}

/// `agintc_dmamem_free`.
pub fn agintc_dmamem_free(dmat: BusDmaTag, adm: &'static AgintcDmamem) {
    // SAFETY: the area `agintc_dmamem_alloc` made, which the GIC no longer uses (its
    // registers were never pointed at it, or are being unmapped).
    unsafe {
        bus_dmamem_unmap(dmat, adm.adm_kva, adm.adm_size);
        bus_dmamem_free(dmat, core::slice::from_ref(&adm.adm_seg));
        bus_dmamap_destroy(dmat, NonNull::from(adm.adm_map));
    }
    free(
        NonNull::from(adm).cast(),
        M_DEVBUF,
        size_of::<AgintcDmamem>(),
    );
}

/// The number of IPIs the IPI handler has run, on every CPU (its event counter): what the
/// `qemu` self-check of `cpu_boot_secondary_processors` watches.
#[cfg(all(feature = "multiprocessor", feature = "qemu"))]
pub fn agintc_ipi_count() -> u64 {
    // SAFETY: the IPI's handler, established at attach and never freed.
    unsafe { AGINTC.sc_ipi_irq.load(Ordering::Acquire).as_ref() }
        .map_or(0, |ih| ih.ih_count.ec_count.load(Ordering::Relaxed))
}

/// Whether the controller has attached (the C's `agintc_sc != NULL`).
pub fn agintc_attached() -> bool {
    ATTACHED.load(Ordering::Acquire)
}

const _: () = {
    assert!(GITS_CMDQ_NENTRIES == 2048);
    assert!(size_of::<[u64; 4]>() == GITS_CMD_SIZE);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distributor_register_offsets() {
        assert_eq!(gicd_isenabler(32), 0x104);
        assert_eq!(gicd_icenabler(64), 0x188);
        assert_eq!(gicd_ipriorityr(33), 0x421);
        assert_eq!(gicd_icfgr(32), 0xc08);
        assert_eq!(gicd_icfgr_trig_edge(33), 0x2 << 2);
        assert_eq!(gicd_nsacr(32), 0xe08);
        assert_eq!(gicd_irouter(32), 0x6100);
        assert_eq!(gicr_ipriorityr(3), 0x10403);
    }

    #[test]
    fn priorities() {
        // The IPI's level stays above IPL_HIGH's mask.
        assert_eq!(agintc_prival(IPL_HIGH, 4), 0x40);
        assert_eq!(agintc_prival(IPL_NONE, 4), 0xf0);
        assert_eq!(agintc_prival(12, 4), 0x30);
        assert_eq!(agintc_prival(IPL_NONE, 3), 0xf8);
        assert_eq!(ffs(0xb0), 5);
        assert_eq!(ffs(0), 0);
        assert_eq!(fls(16383), 14);
    }

    #[test]
    fn propbaser_id_bits() {
        // 8192 LPIs above 8192: 14 ID bits, encoded as 13.
        assert_eq!(
            agintc_propbaser(0x4000_0000, 8192),
            0x4000_0000 | 0x400 | 0x80 | 13
        );
    }

    #[test]
    fn sgi_target() {
        // Aff0 3, Aff1 1, Aff2 2, Aff3 4.
        let mpidr = (4u64 << 32) | (2 << 16) | (1 << 8) | 3;
        let v = agintc_sgi1r(mpidr, 5);
        assert_eq!(v & 0xffff, 1 << 3);
        assert_eq!((v >> 16) & 0xff, 1);
        assert_eq!((v >> 24) & 0xf, 5);
        assert_eq!((v >> 32) & 0xff, 2);
        assert_eq!((v >> 48) & 0xff, 4);
        assert_eq!(agintc_mpidr_affinity(mpidr | (1 << 31)), 0x0402_0103);
    }

    #[test]
    fn redistributor_frames() {
        assert_eq!(agintc_redist_size(0, 0), 0x20000);
        assert_eq!(agintc_redist_size(GICR_TYPER_VLPIS, 0), 0x40000);
        assert_eq!(agintc_redist_size(GICR_TYPER_VLPIS, 0x30000), 0x30000);
    }

    #[test]
    fn its_commands() {
        let cmd = GitsCmd {
            cmd: MAPTI,
            deviceid: 0x10,
            eventid: 2,
            intid: 8193,
            dw2: 1,
            ..GitsCmd::default()
        };
        assert_eq!(cmd.words(), [0x10_0000_000a, (8193 << 32) | 2, 1, 0]);
        let mapd = GitsCmd {
            cmd: MAPD,
            deviceid: 8,
            eventid: 4,
            dw2: 0x8000_0000 | GITS_CMD_VALID,
            ..GitsCmd::default()
        };
        assert_eq!(mapd.words()[0], (8 << 32) | 0x08);
        assert_eq!(mapd.words()[1], 4);
    }

    #[test]
    fn its_tables() {
        assert_eq!(gits_baser_pgsz(GITS_BASER_PGSZ_64K), 65536);
        assert_eq!(gits_baser_pgsz(GITS_BASER_PGSZ_16K), 16384);
        assert_eq!(gits_baser_pgsz(GITS_BASER_PGSZ_4K), PAGE_SIZE);
        // 16 device ID bits, 8-byte entries, 64 KB pages: 512 KB flat; indirect, one page
        // of pointers covers every ID.
        assert_eq!(
            agintc_msi_dtt_layout(16, 8, 65536, false),
            (512 * 1024, 65535)
        );
        assert_eq!(
            agintc_msi_dtt_layout(16, 8, 65536, true),
            (65536, 8192 * 8192 - 1)
        );
        // 32 bits flat clamps at 256 pages.
        assert_eq!(agintc_msi_dtt_layout(32, 8, 4096, false).0, 256 * 4096);
        assert_eq!(gits_typer_devbits(0xf << 13), 15);
        assert_eq!(gits_typer_ite_sz(0x7 << 4), 7);
        assert_eq!(gits_baser_tte_sz(7 << 48), 7);
    }

    #[test]
    fn oryon_quirk_ids() {
        let midr = (0x51u64 << 24) | (0x001 << 4);
        assert_eq!(cpu_impl(midr), CPU_IMPL_QCOM);
        assert_eq!(cpu_part(midr), CPU_PART_ORYON);
    }
}
/* </TESTS> */
