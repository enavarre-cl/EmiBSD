/*	$OpenBSD: acpivar.h,v 1.141 2026/03/11 16:18:42 kettenis Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 2005 Thorsten Lockert <tholo@sigmasoft.com>
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
//! `<dev/acpi/acpivar.h>`: the state `acpi(4)` keeps (`struct acpi_softc`), the arguments it
//! attaches its children with, its table and task queues, and the register indices of the
//! fixed hardware.
//!
//! Upstream: sys/dev/acpi/acpivar.h @ 3ce1f3f79392
//!
//! The functions this header declares live in the files that define them: `acpi.c` (most of
//! them; `dev/acpi/acpi.rs` holds the ones the AML interpreter calls until that file is
//! ported), `acpi_machdep.c`, `acpiec.c`, `acpipwrres.c` and `dsdt.c` (`acpi_poll`,
//! `acpi_sleep`).
//!
//! ## Deviations
//! - `ACPI_DEBUG` is not configured: `dprintf`/`dnprintf` are compiled out, as in C.
//! - `extern struct acpi_softc *acpi_softc` (defined by `acpi.c`) is [`ACPI_SOFTC`] here, an
//!   `AtomicPtr` set once when acpi0 attaches, read with [`acpi_softc`].
//! - `struct acpi_softc` is a [`Softc`]: autoconfiguration allocates acpi0's zero-filled, as
//!   the C's `config_attach` does (`acpi_ca`'s `ca_devsize`), and it is never freed.
//!   `acpi_reg_map`'s `name` is an `Option` so that all-zero is a valid softc. Members whose
//!   types are not ported are left out and wait for their files: `sc_note` (the `klist` of `acpi.c`'s kqueue filter),
//!   `sc_pwrresdevs` (`acpipwrres.c`; `NACPIPWRRES` is 0 until then, so `struct acpi_pwrres`
//!   and `acpi_pwrreshead_t` are also left out), `sc_ac`, `sc_bat`, `sc_sbs` and their list
//!   types (`acpiac.c`, `acpibat.c`, `acpisbs.c`). `sc_ec` is a `*mut c_void` until
//!   `acpiec.c` gives `struct acpiec_softc`.
//! - Pointers into firmware memory (`sc_fadt`, `sc_facs`, `q_table`) stay raw pointers:
//!   the firmware shares `struct acpi_facs` with the kernel.
//! - `struct acpi_attach_args`' strings (`aaa_name`, `aaa_dev`, `aaa_cdev`) are
//!   NUL-terminated `*const u8` the attaching code owns for the duration of `config_found`.
//! - `TAILQ_HEAD(acpi_devlist_head, acpi_devlist)` is a `Vec` of [`AcpiDevlist`]: the lists
//!   are built by `acpi_getdevlist` and dropped by `acpi_freedevlist`, never shared.
//! - `struct acpivideo_softc` waits for `acpivideo.c`.

use alloc::vec::Vec;
use core::cell::{Cell, RefCell};
use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::{AtomicPtr, Ordering};

use super::acpireg::{AcpiFacs, AcpiFadt};
use super::amltypes::{AmlNodeRef, AmlValueRef};
use crate::machine::bus::{BusDmaTag, BusSpaceHandle, BusSpaceTag};
use crate::sys::device::{Device, Softc};
use crate::sys::param::NBPG;
use crate::sys::queue::{SimpleqEntry, SimpleqHead};
use crate::sys::rwlock::Rwlock;
use crate::sys::timeout::Timeout;

/// `ACPI_TRAMPOLINE`: the page the wakeup code runs from.
pub const ACPI_TRAMPOLINE: usize = 19 * NBPG;
/// `ACPI_TRAMP_DATA`: the page after it, for the wakeup code's data.
pub const ACPI_TRAMP_DATA: usize = 20 * NBPG;

/// `ACPI_UUID(a, b, c, d, e)`: the 16 bytes of a UUID as ACPI stores it (the first three
/// fields little-endian, the last two big-endian).
pub const fn acpi_uuid(a: u32, b: u16, c: u16, d: u16, e: u64) -> [u8; 16] {
    [
        a as u8,
        (a >> 8) as u8,
        (a >> 16) as u8,
        (a >> 24) as u8,
        b as u8,
        (b >> 8) as u8,
        c as u8,
        (c >> 8) as u8,
        (d >> 8) as u8,
        d as u8,
        (e >> 40) as u8,
        (e >> 32) as u8,
        (e >> 24) as u8,
        (e >> 16) as u8,
        (e >> 8) as u8,
        e as u8,
    ]
}

/// `ACPIDEVCF_ADDR`: the locator index of an ACPI device's address.
pub const ACPIDEVCF_ADDR: usize = 0;
/// `ACPIDEVCF_ADDR_UNK`: an unknown address.
pub const ACPIDEVCF_ADDR_UNK: i32 = -1;

/// `struct acpi_attach_args`: what acpi0 hands the drivers it attaches.
pub struct AcpiAttachArgs {
    /// `aaa_name`: the driver name to match (`"acpitimer"`, ...), NUL-terminated.
    pub aaa_name: *const u8,
    /// `aaa_iot`.
    pub aaa_iot: Option<BusSpaceTag>,
    /// `aaa_memt`.
    pub aaa_memt: Option<BusSpaceTag>,
    /// `aaa_dmat`.
    pub aaa_dmat: Option<BusDmaTag>,
    /// `aaa_table`: the table the device is described by, if not the namespace.
    pub aaa_table: *mut c_void,
    /// `aaa_node`: the device's namespace node.
    pub aaa_node: Option<AmlNodeRef>,
    /// `aaa_dev`: its `_HID`, NUL-terminated.
    pub aaa_dev: *const u8,
    /// `aaa_cdev`: its `_CID`, NUL-terminated.
    pub aaa_cdev: *const u8,
    /// `aaa_addr`: the addresses `_CRS` gives.
    pub aaa_addr: [u64; 8],
    /// `aaa_size`.
    pub aaa_size: [u64; 8],
    /// `aaa_bst`: the bus space of each address.
    pub aaa_bst: [Option<BusSpaceTag>; 8],
    /// `aaa_naddr`.
    pub aaa_naddr: i32,
    /// `aaa_irq`.
    pub aaa_irq: [u32; 8],
    /// `aaa_irq_flags`.
    pub aaa_irq_flags: [u32; 8],
    /// `aaa_nirq`.
    pub aaa_nirq: i32,
}

/// `struct acpi_mem_map`: a mapping `acpi_map` made of physical memory.
#[derive(Clone, Copy, Default)]
pub struct AcpiMemMap {
    /// `baseva`: the page-aligned start of the mapping.
    pub baseva: usize,
    /// `va`: the virtual address of the requested physical address.
    pub va: usize,
    /// `vsize`.
    pub vsize: usize,
    /// `pa`.
    pub pa: usize,
}

/// `struct acpi_q`: a table `acpi_maptable` loaded, in `sc_tables`.
#[repr(C)]
pub struct AcpiQ {
    /// `q_next`.
    pub q_next: SimpleqEntry<AcpiQ>,
    /// `q_id`: the table's index, also the DDB handle `Load` returns.
    pub q_id: i32,
    /// `q_table`: the table (`struct acpi_table_header` first), mapped for good.
    pub q_table: *mut c_void,
    /// `q_data[0]`: the copy of the table that follows the structure in its allocation.
    pub q_data: [u8; 0],
}

crate::queue_adapter!(
    /// `acpi_qhead_t`'s adapter: `struct acpi_q` through `q_next`.
    pub AcpiQList: AcpiQ, q_next => SimpleqEntry<AcpiQ>
);

/// `acpi_qhead_t`.
pub type AcpiQhead = SimpleqHead<AcpiQList>;

/// `struct acpi_taskq`: work for the acpi thread (`acpi_addtask`).
#[repr(C)]
pub struct AcpiTaskq {
    /// `next`.
    pub next: SimpleqEntry<AcpiTaskq>,
    /// `handler(arg0, arg1)`.
    pub handler: fn(*mut c_void, i32),
    /// `arg0`.
    pub arg0: *mut c_void,
    /// `arg1`.
    pub arg1: i32,
}

/// `struct acpi_wakeq`: a device that can wake the machine (`_PRW`).
#[repr(C)]
pub struct AcpiWakeq {
    /// `q_next`.
    pub q_next: SimpleqEntry<AcpiWakeq>,
    /// `q_node`.
    pub q_node: RefCell<Option<AmlNodeRef>>,
    /// `q_wakepkg`: its `_PRW` package.
    pub q_wakepkg: RefCell<Option<AmlValueRef>>,
    /// `q_gpe`.
    pub q_gpe: Cell<i32>,
    /// `q_state`.
    pub q_state: Cell<i32>,
    /// `q_enabled`.
    pub q_enabled: Cell<i32>,
}

crate::queue_adapter!(
    /// `acpi_wakeqhead_t`'s adapter: `struct acpi_wakeq` through `q_next`.
    pub AcpiWakeqList: AcpiWakeq, q_next => SimpleqEntry<AcpiWakeq>
);

/// `acpi_wakeqhead_t`.
pub type AcpiWakeqhead = SimpleqHead<AcpiWakeqList>;

/// `ACPIREG_PM1A_STS`: the index of a fixed register in `sc_pmregs`.
pub const ACPIREG_PM1A_STS: i32 = 0x00;
/// `ACPIREG_PM1A_EN`.
pub const ACPIREG_PM1A_EN: i32 = 0x01;
/// `ACPIREG_PM1A_CNT`.
pub const ACPIREG_PM1A_CNT: i32 = 0x02;
/// `ACPIREG_PM1B_STS`.
pub const ACPIREG_PM1B_STS: i32 = 0x03;
/// `ACPIREG_PM1B_EN`.
pub const ACPIREG_PM1B_EN: i32 = 0x04;
/// `ACPIREG_PM1B_CNT`.
pub const ACPIREG_PM1B_CNT: i32 = 0x05;
/// `ACPIREG_PM2_CNT`.
pub const ACPIREG_PM2_CNT: i32 = 0x06;
/// `ACPIREG_PM_TMR`.
pub const ACPIREG_PM_TMR: i32 = 0x07;
/// `ACPIREG_GPE0_STS`.
pub const ACPIREG_GPE0_STS: i32 = 0x08;
/// `ACPIREG_GPE0_EN`.
pub const ACPIREG_GPE0_EN: i32 = 0x09;
/// `ACPIREG_GPE1_STS`.
pub const ACPIREG_GPE1_STS: i32 = 0x0A;
/// `ACPIREG_GPE1_EN`.
pub const ACPIREG_GPE1_EN: i32 = 0x0B;
/// `ACPIREG_SMICMD`.
pub const ACPIREG_SMICMD: i32 = 0x0C;
/// `ACPIREG_MAXREG`: the number of mapped registers.
pub const ACPIREG_MAXREG: usize = 0x0D;

/// `ACPIREG_PM1_STS`: a special register, both PM1 status blocks.
pub const ACPIREG_PM1_STS: i32 = 0x0E;
/// `ACPIREG_PM1_EN`.
pub const ACPIREG_PM1_EN: i32 = 0x0F;
/// `ACPIREG_PM1_CNT`.
pub const ACPIREG_PM1_CNT: i32 = 0x10;
/// `ACPIREG_GPE_STS`.
pub const ACPIREG_GPE_STS: i32 = 0x11;
/// `ACPIREG_GPE_EN`.
pub const ACPIREG_GPE_EN: i32 = 0x12;

/// `ACPI_SST_INDICATOR_OFF`: a system status (`_SST`) code.
pub const ACPI_SST_INDICATOR_OFF: i32 = 0;
/// `ACPI_SST_WORKING`.
pub const ACPI_SST_WORKING: i32 = 1;
/// `ACPI_SST_WAKING`.
pub const ACPI_SST_WAKING: i32 = 2;
/// `ACPI_SST_SLEEPING`.
pub const ACPI_SST_SLEEPING: i32 = 3;
/// `ACPI_SST_SLEEP_CONTEXT`.
pub const ACPI_SST_SLEEP_CONTEXT: i32 = 4;

/// `struct acpi_reg_map`: one mapped fixed register.
#[derive(Default)]
pub struct AcpiRegMap {
    /// `ioh`.
    pub ioh: Cell<Option<BusSpaceHandle>>,
    /// `addr`.
    pub addr: Cell<i32>,
    /// `size`.
    pub size: Cell<i32>,
    /// `access`.
    pub access: Cell<i32>,
    /// `name`: `None` (the C's NULL) until `acpi_map_pmregs` maps the register.
    pub name: Cell<Option<&'static str>>,
}

/// `struct acpi_thread`.
pub struct AcpiThread {
    /// `sc`.
    pub sc: Cell<Option<&'static AcpiSoftc>>,
    /// `running` (`volatile int`).
    pub running: core::sync::atomic::AtomicI32,
}

/// `ACPI_MTX_MAXNAME`.
pub const ACPI_MTX_MAXNAME: usize = 5;

/// `struct acpi_mutex`.
pub struct AcpiMutex {
    /// `amt_lock`.
    pub amt_lock: Rwlock,
    /// `amt_name`: only four characters are used.
    pub amt_name: [u8; ACPI_MTX_MAXNAME + 3],
    /// `amt_ref_count`.
    pub amt_ref_count: Cell<i32>,
    /// `amt_timeout`.
    pub amt_timeout: Cell<i32>,
    /// `amt_synclevel`.
    pub amt_synclevel: Cell<i32>,
}

/// `struct gpe_block`: the handler of one general-purpose event.
#[derive(Clone, Copy)]
pub struct GpeBlock {
    /// `handler(sc, gpe, arg)`.
    pub handler: Option<fn(&AcpiSoftc, i32, *mut c_void) -> i32>,
    /// `arg`.
    pub arg: *mut c_void,
    /// `active`.
    pub active: i32,
    /// `flags`: `GPE_LEVEL` or `GPE_EDGE`.
    pub flags: i32,
}

/// `struct acpi_devlist`: one device of a `_EDL`/`_EJD`-style package.
pub struct AcpiDevlist {
    /// `dev_node`.
    pub dev_node: AmlNodeRef,
}

/// `struct acpi_devlist_head`.
pub type AcpiDevlistHead = Vec<AcpiDevlist>;

/// One `sc_sleeptype[]` entry: the `SLP_TYP` values of a sleep state.
#[derive(Default)]
pub struct AcpiSleeptype {
    /// `slp_typa`.
    pub slp_typa: Cell<i32>,
    /// `slp_typb`.
    pub slp_typb: Cell<i32>,
}

/// `struct acpi_softc`: acpi0.
#[repr(C)]
pub struct AcpiSoftc {
    /// `sc_dev`.
    pub sc_dev: Device,
    /// `sc_iot`.
    pub sc_iot: Cell<Option<BusSpaceTag>>,
    /// `sc_memt`.
    pub sc_memt: Cell<Option<BusSpaceTag>>,
    /// `sc_cc_dmat`: DMA tag for cache-coherent devices.
    pub sc_cc_dmat: Cell<Option<BusDmaTag>>,
    /// `sc_ci_dmat`: DMA tag for the others.
    pub sc_ci_dmat: Cell<Option<BusDmaTag>>,
    /// `sc_fadt`.
    pub sc_fadt: Cell<*const AcpiFadt>,
    /// `sc_tables`: every table loaded.
    pub sc_tables: AcpiQhead,
    /// `sc_wakedevs`.
    pub sc_wakedevs: AcpiWakeqhead,
    /// `sc_hw_reduced`.
    pub sc_hw_reduced: Cell<i32>,
    /// `sc_facs`: shared with the firmware.
    pub sc_facs: Cell<*mut AcpiFacs>,
    /// `sc_pmregs`.
    pub sc_pmregs: [AcpiRegMap; ACPIREG_MAXREG],
    /// `sc_ioh_pm1a_evt`.
    pub sc_ioh_pm1a_evt: Cell<Option<BusSpaceHandle>>,
    /// `sc_interrupt`: the SCI's handle.
    pub sc_interrupt: Cell<*mut c_void>,
    /// `sc_lck`: serialises the AML interpreter's sleepers with the acpi thread.
    pub sc_lck: Rwlock,
    /// `sc_sleeptype`.
    pub sc_sleeptype: [AcpiSleeptype; 6],
    /// `sc_lastgpe`.
    pub sc_lastgpe: Cell<i32>,
    /// `sc_wakegpe`.
    pub sc_wakegpe: Cell<i32>,
    /// `sc_wakegpio`.
    pub sc_wakegpio: Cell<i32>,
    /// `gpe_table`.
    pub gpe_table: Cell<*mut GpeBlock>,
    /// `sc_threadwaiting`.
    pub sc_threadwaiting: Cell<i32>,
    /// `sc_gpe_sts`.
    pub sc_gpe_sts: Cell<u32>,
    /// `sc_gpe_en`.
    pub sc_gpe_en: Cell<u32>,
    /// `sc_thread`.
    pub sc_thread: Cell<*mut AcpiThread>,
    /// `sc_root`: `\_SB_`.
    pub sc_root: RefCell<Option<AmlNodeRef>>,
    /// `sc_tts`.
    pub sc_tts: RefCell<Option<AmlNodeRef>>,
    /// `sc_pts`.
    pub sc_pts: RefCell<Option<AmlNodeRef>>,
    /// `sc_bfs`.
    pub sc_bfs: RefCell<Option<AmlNodeRef>>,
    /// `sc_gts`.
    pub sc_gts: RefCell<Option<AmlNodeRef>>,
    /// `sc_sst`.
    pub sc_sst: RefCell<Option<AmlNodeRef>>,
    /// `sc_wak`.
    pub sc_wak: RefCell<Option<AmlNodeRef>>,
    /// `sc_state`.
    pub sc_state: Cell<i32>,
    /// `sc_wakeup`.
    pub sc_wakeup: Cell<i32>,
    /// `sc_wakeups`.
    pub sc_wakeups: Cell<i32>,
    /// `sc_ec`: the embedded controller (`struct acpiec_softc *` once `acpiec.c` is ported).
    pub sc_ec: Cell<*mut c_void>,
    /// `sc_havesbs`.
    pub sc_havesbs: Cell<i32>,
    /// `sc_dev_timeout`: polls the `ACPIDEV_POLL` devices (`acpi_poll`).
    pub sc_dev_timeout: Timeout,
    /// `sc_major`.
    pub sc_major: Cell<i32>,
    /// `sc_minor`.
    pub sc_minor: Cell<i32>,
    /// `sc_pse`: passive cooling enabled.
    pub sc_pse: Cell<i32>,
    /// `sc_flags`.
    pub sc_flags: Cell<i32>,
    /// `sc_skip_processor`.
    pub sc_skip_processor: Cell<i32>,
    /// `sc_pmc_suspend`.
    pub sc_pmc_suspend: Cell<Option<fn(*mut c_void)>>,
    /// `sc_pmc_resume`.
    pub sc_pmc_resume: Cell<Option<fn(*mut c_void)>>,
    /// `sc_pmc_cookie`.
    pub sc_pmc_cookie: Cell<*mut c_void>,
}

// SAFETY: `#[repr(C)]` with the `Device` first; every member is valid all-zero: `Cell`s of
// integers, raw pointers and `Option`s (of bus tags and handles, nodes, functions, names),
// `RefCell<Option<_>>`s (an unborrowed `None`), the queue heads (`SIMPLEQ_INIT` runs in
// `acpi_attach_common`), the register maps, the rwlock (named by `rw_init`) and the timeout
// (set by `timeout_set`), as `struct acpi_softc` is in C after `M_ZERO`.
unsafe impl Softc for AcpiSoftc {}

/// `WAKEGPE_NONE`.
pub const WAKEGPE_NONE: i32 = -1;
/// `WAKEGPE_PWRBTN`.
pub const WAKEGPE_PWRBTN: i32 = -2;
/// `WAKEGPE_SLPBTN`.
pub const WAKEGPE_SLPBTN: i32 = -3;
/// `WAKEGPE_RTC`.
pub const WAKEGPE_RTC: i32 = -4;
/// `WAKEGPE_GPIO`.
pub const WAKEGPE_GPIO: i32 = -5;

/// `acpi_softc`: acpi0, once it attached; null before (and on machines without ACPI).
pub static ACPI_SOFTC: AtomicPtr<AcpiSoftc> = AtomicPtr::new(ptr::null_mut());

/// `SCFLAG_OREAD`.
pub const SCFLAG_OREAD: i32 = 0x0000001;
/// `SCFLAG_OWRITE`.
pub const SCFLAG_OWRITE: i32 = 0x0000002;
/// `SCFLAG_OPEN`.
pub const SCFLAG_OPEN: i32 = SCFLAG_OREAD | SCFLAG_OWRITE;

/// `GPE_NONE`.
pub const GPE_NONE: i32 = 0x00;
/// `GPE_LEVEL`.
pub const GPE_LEVEL: i32 = 0x01;
/// `GPE_EDGE`.
pub const GPE_EDGE: i32 = 0x02;

/// `ACPI_IOREAD`: the direction of `acpi_gasio`/`acpi_genio` and of field accesses.
pub const ACPI_IOREAD: i32 = 0;
/// `ACPI_IOWRITE`.
pub const ACPI_IOWRITE: i32 = 1;

/// `GL_BIT_PENDING`: the firmware waits for the global lock (section 5.2.10.1).
pub const GL_BIT_PENDING: u32 = 0x01;
/// `GL_BIT_OWNED`.
pub const GL_BIT_OWNED: u32 = 0x02;

/// `acpi_softc` read: acpi0's softc, `None` before it attached.
pub fn acpi_softc() -> Option<&'static AcpiSoftc> {
    let p = ACPI_SOFTC.load(Ordering::Acquire);
    // SAFETY: `ACPI_SOFTC` is null or points at acpi0's softc, which `acpi.c` stores once
    // the softc is fully set up and never frees (acpi0 does not detach).
    unsafe { p.as_ref() }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuid_layout() {
        // The _OSC UUID of the PCI host bridge, 33db4d5b-1ff7-401c-9657-7441c03dd766.
        let u = acpi_uuid(0x33db4d5b, 0x1ff7, 0x401c, 0x9657, 0x7441c03dd766);
        assert_eq!(&u[..4], &[0x5b, 0x4d, 0xdb, 0x33]);
        assert_eq!(&u[8..10], &[0x96, 0x57]);
        assert_eq!(u[15], 0x66);
        assert!(acpi_softc().is_none());
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/acpi/acpivar.h");
        crate::reftest::assert_defines!(defs;
            ACPIDEVCF_ADDR, ACPIDEVCF_ADDR_UNK, ACPIREG_PM1A_STS, ACPIREG_PM1A_EN,
            ACPIREG_PM1A_CNT, ACPIREG_PM1B_STS, ACPIREG_PM1B_EN, ACPIREG_PM1B_CNT,
            ACPIREG_PM2_CNT, ACPIREG_PM_TMR, ACPIREG_GPE0_STS, ACPIREG_GPE0_EN,
            ACPIREG_GPE1_STS, ACPIREG_GPE1_EN, ACPIREG_SMICMD, ACPIREG_MAXREG,
            ACPIREG_PM1_STS, ACPIREG_PM1_EN, ACPIREG_PM1_CNT, ACPIREG_GPE_STS, ACPIREG_GPE_EN,
            ACPI_SST_INDICATOR_OFF, ACPI_SST_WORKING, ACPI_SST_WAKING, ACPI_SST_SLEEPING,
            ACPI_SST_SLEEP_CONTEXT, ACPI_MTX_MAXNAME, WAKEGPE_NONE, WAKEGPE_PWRBTN,
            WAKEGPE_SLPBTN, WAKEGPE_RTC, WAKEGPE_GPIO, SCFLAG_OREAD, SCFLAG_OWRITE,
            SCFLAG_OPEN, GPE_NONE, GPE_LEVEL, GPE_EDGE, ACPI_IOREAD, ACPI_IOWRITE,
            GL_BIT_PENDING, GL_BIT_OWNED,
        );
    }
}
/* </TESTS> */
