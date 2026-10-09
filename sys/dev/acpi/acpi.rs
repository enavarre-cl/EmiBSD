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
 * Copyright (c) 2005 Thorsten Lockert <tholo@sigmasoft.com>
 * Copyright (c) 2005 Jordan Hargrave <jordan@openbsd.org>
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
//! acpi(4): `dev/acpi/acpi.c`, the ACPI core. acpi0 finds the firmware's tables (RSDP, then
//! XSDT or RSDT), copies them, maps the fixed power-management registers the FADT names,
//! loads the DSDT and the SSDTs into the AML interpreter (`dsdt.rs`), takes ACPI mode over
//! from the firmware (`SMI_CMD`), sets up the general-purpose events, discovers the sleep
//! states (`_S0_`..`_S5_`) and the devices that can wake the machine, attaches its children
//! (the PM timer, the drivers of the tables, and the namespace devices by `_HID`), and runs
//! its thread, which serves the queue of AML work (`acpi_addtask`) the SCI and notifications
//! fill. `acpi_powerdown` enters S5 (`halt -p`); `acpi_reset` writes the FADT's reset
//! register (`cpu_reset`).
//!
//! Upstream: sys/dev/acpi/acpi.c @ 3ce1f3f79392
//!
//! The machine's half (`acpi_map`, `acpi_attach_machdep`, the global lock, ...) is
//! `machine::acpi_machdep`, each architecture's `acpi_machdep.c`. acpi0 attaches at bios0 on
//! amd64 (`arch/amd64/amd64/bios.rs`); arm64 attaches no acpi0 until M14. Options as in
//! amd64 GENERIC: `SMALL_KERNEL` is not defined, `SUSPEND` is; `ACPI_DEBUG` is not
//! configured (`dnprintf` compiled out); `NACPIPWRRES` and `NWD` are 0 (neither
//! `acpipwrres` nor `wd` is configured).
//!
//! ## Deviations
//! - acpi0's children are matched against the `cfdata` table, as in C. `acpitimer` and
//!   `acpihpet` are ported (M13), `acpicpu` on amd64 (M16e, `acpicpu_x86.rs`); the drivers
//!   that are not (`acpiec`, `acpitz`, ...) and the devices found by `_HID` with no driver
//!   print "not configured" (or nothing, for the quiet ones), as an OpenBSD kernel without
//!   them does.
//! - `pool acpiwqpool` and the `SIMPLEQ` of tasks are a `VecDeque` of `struct acpi_taskq`
//!   values (their `next` link is unused); a failed allocation prints "unable to create
//!   task" as `pool_get(PR_NOWAIT)` failing does. The PCI lists (`acpi_pcidevs`,
//!   `acpi_pcirootdevs`) are `Vec`s of the `struct acpi_pci` they own (leaked, as the C
//!   never frees one that made it into a list). Both live in `AmlGlobal`s: every path is
//!   under the kernel lock.
//! - `acpi_maptable`'s copy (`q_data`) is its own allocation, zero-padded to at least
//!   `sizeof(struct acpi_fadt)` so that reading the FADT of an older revision stays in
//!   bounds (the C reads past a short copy). An entry kept in no list (`flag` 0) is never
//!   freed: `acpi_loadtables`, the C's only such caller, copies the XSDT/RSDT through the
//!   same code and drops it, so `q_id`s are numbered as in C. A table shorter than its
//!   header is refused.
//! - `acpi_gasio` with no acpi0 fails (-1); the C dereferences NULL. Accesses past the end
//!   of `buffer` are not made (the C overruns it). `GAS_EMBEDDED` reaches `acpiec_read`/
//!   `acpiec_write` only through `sc_ec`, which stays NULL until `acpiec.c` is ported, so
//!   the C's "EC not initialized" answer is what it gives.
//! - `acpi_attach_common` with no DSDT prints " !DSDT" and parses nothing (the C dereferences
//!   NULL).
//! - The `#ifdef __arm64__` (`ECTC`, `acpisectwo`) and `#if defined(__amd64__) ||
//!   defined(__i386__)` (`_PRT`, `acpiprt`) walks are chosen by the machine's
//!   `ACPI_SECTWO`/`ACPI_PRT` constants, not by `cfg(target_arch)`.
//! - `SUSPEND` paths of `subr_suspend.c`, which is not ported, are reported where reached:
//!   `sleep_state` (`acpi_sleep_task`, power button set to suspend) and
//!   `device_register_wakeup` (`acpi_attach_common` when a device can wake the machine);
//!   `resuming()` is false (the machine never resumes). The S3 machinery itself
//!   (`acpi_x86.c`, `acpi_wakecode.S`, `acpi_sleep_cpu`) is not ported.
//! - `sc_note`, `sc_ac`, `sc_bat`, `sc_sbs` wait for `acpi_apm.c`, `acpiac.c`, `acpibat.c`
//!   and `acpisbs.c` (`acpivar.rs`): `acpi_attach_common`'s walk of `alldevs` that fills
//!   the three lists, `acpi_batcount` and `acpi_apminfo` (whose callers are `acpibat.c` and
//!   `acpi_apm.c`, with `<machine/apmvar.h>`) come with them. `acpi_record_event` returns 1
//!   while `SCFLAG_OPEN` is never set (no `acpiopen`); its `knote_locked` is reported.
//! - `acpi_pciroots_attach` cannot reserve the bus numbers (`extent_alloc_region` on
//!   `pba_busex`: `subr_extent.c` is not ported) and reports it.
//! - `aml_intlen` stays 64: `acpi.c` at the pin never sets it to 32 for a revision-1 DSDT.
//! - The callbacks of `aml_find_node`, `aml_walknodes` and `aml_parse_resource` are
//!   closures; the C's `void *arg` (the softc, a bus number) is captured.

use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::format;
use alloc::rc::Rc;
use alloc::vec::Vec;
use core::cell::{Cell, RefCell};
use core::ffi::c_void;
use core::mem::{ManuallyDrop, size_of};
use core::ptr;
use core::slice;
use core::sync::atomic::{AtomicI32, AtomicU32, Ordering};

use super::acpidev::{STA_BATTERY, STA_DEV_OK, STA_ENABLED, STA_PRESENT, STA_SHOW_UI};
use super::acpireg::{
    ACPI_DEV_ECD, ACPI_DEV_MOUSE, ACPI_DEV_PCIB, ACPI_DEV_PCIEB, ACPI_DEV_SBD, ACPI_DEV_SBS,
    ACPI_OPREG_GPIO, ACPI_OPREG_GSB, ACPI_PM1_ALL_STS, ACPI_PM1_PWRBTN_EN, ACPI_PM1_PWRBTN_STS,
    ACPI_PM1_RTC_EN, ACPI_PM1_RTC_STS, ACPI_PM1_SCI_EN, ACPI_PM1_SLP_EN, ACPI_PM1_SLP_TYPX_MASK,
    ACPI_PM1_SLPBTN_EN, ACPI_PM1_SLPBTN_STS, ACPI_PM1_WAK_STS, ACPI_PM2_ARB_DIS, ACPI_STATE_D0,
    ACPI_STATE_S0, ACPI_STATE_S3, ACPI_STATE_S4, ACPI_STATE_S5, AcpiFacs, AcpiFadt, AcpiGas,
    AcpiRsdp, AcpiTableHeader, FADT_HW_REDUCED_ACPI, FADT_POWER_S0_IDLE_CAPABLE, FADT_PWR_BUTTON,
    FADT_RESET_REG_SUP, FADT_SIG, FADT_SLP_BUTTON, GAS_EMBEDDED, GAS_PCI_CFG_SPACE,
    GAS_SYSTEM_IOSPACE, GAS_SYSTEM_MEMORY, SSDT_SIG, acpi_adr_pcidev, acpi_adr_pcifun,
    acpi_pci_bus, acpi_pci_dev, acpi_pci_fn, acpi_pci_reg, acpi_pci_seg, acpi_pm1_slp_typx,
};
use super::acpiutil::acpi_checksum;
use super::acpivar::{
    ACPI_IOREAD, ACPI_IOWRITE, ACPI_SOFTC, ACPIREG_GPE_EN, ACPIREG_GPE_STS, ACPIREG_GPE0_EN,
    ACPIREG_GPE0_STS, ACPIREG_GPE1_EN, ACPIREG_GPE1_STS, ACPIREG_MAXREG, ACPIREG_PM1_CNT,
    ACPIREG_PM1_EN, ACPIREG_PM1_STS, ACPIREG_PM1A_CNT, ACPIREG_PM1A_EN, ACPIREG_PM1A_STS,
    ACPIREG_PM1B_CNT, ACPIREG_PM1B_EN, ACPIREG_PM1B_STS, ACPIREG_PM2_CNT, ACPIREG_SMICMD,
    AcpiAttachArgs, AcpiQ, AcpiSoftc, AcpiTaskq, AcpiThread, AcpiWakeq, GPE_EDGE, GPE_LEVEL,
    GpeBlock, SCFLAG_OPEN, WAKEGPE_GPIO, WAKEGPE_NONE, WAKEGPE_PWRBTN, WAKEGPE_RTC, WAKEGPE_SLPBTN,
    acpi_softc,
};
use super::amltypes::{
    AML_OBJTYPE_BUFFER, AML_OBJTYPE_DEVICE, AML_OBJTYPE_INTEGER, AML_OBJTYPE_NAMEREF,
    AML_OBJTYPE_OBJREF, AML_OBJTYPE_PACKAGE, AML_OBJTYPE_POWERRSRC, AML_OBJTYPE_PROCESSOR,
    AML_OBJTYPE_STRING, AML_OBJTYPE_THERMZONE, AcpiPci, AmlNodeRef, AmlValue, AmlValueRef,
};
use super::dsdt::{
    AML_BUSY, AML_WALK_PRE, AcpiResource, AmlGlobal, LR_DWORD, LR_EXTIRQ, LR_EXTIRQ_MODE,
    LR_EXTIRQ_POLARITY, LR_EXTIRQ_SHR, LR_GPIO, LR_GPIO_INT, LR_GPIO_LEVEL, LR_GPIO_MODE, LR_MEM24,
    LR_MEM32, LR_MEM32FIXED, LR_QWORD, LR_TYPE_IO, LR_TYPE_MEMORY, LR_WORD, SR_FIXEDPORT,
    SR_IOPORT, SR_IRQ, SR_IRQ_MODE, SR_IRQ_POLARITY, SR_IRQ_SHR, acpi_parse_aml, acpi_poll,
    aml_create_defaultobjects, aml_crslen, aml_crstype, aml_eisaid, aml_evalhid, aml_evalinteger,
    aml_evalname, aml_evalnode, aml_find_node, aml_freevalue, aml_getname, aml_hashopcodes,
    aml_node_setval, aml_notify_dev, aml_parse_resource, aml_postparse, aml_register_notify,
    aml_root, aml_searchname, aml_searchrel, aml_val2int, aml_walknodes, cstr,
};
use crate::dev::pci::pci::{PCI_DOPM, pci_get_capability, pci_get_powerstate};
use crate::dev::pci::pcidevs::PCI_VENDOR_INVALID;
use crate::dev::pci::pcireg::{
    PCI_CAP_PWRMGMT, PCI_CLASS_BRIDGE, PCI_CLASS_REG, PCI_ID_REG, PCI_PMCSR, PCI_PMCSR_STATE_D0,
    PCI_PMCSR_STATE_D3, PCI_SUBCLASS_BRIDGE_PCI, pci_class, pci_subclass, pci_vendor,
};
use crate::dev::pci::pcivar::{PciAttachArgs, PcibusAttachArgs, Pcireg};
use crate::dev::pci::ppbreg::{PPB_REG_BUSINFO, ppb_businfo_secondary};
use crate::kern::init_main::{BOOTHOWTO, NCPUS};
use crate::kern::kern_kthread::{kthread_create, kthread_create_deferred, kthread_exit};
use crate::kern::kern_rwlock::{rw_enter_write, rw_exit_write, rw_init};
use crate::kern::kern_sched::sched_peg_curproc;
use crate::kern::kern_synch::{tsleep_nsec, wakeup};
use crate::kern::kern_timeout::timeout_set;
use crate::kern::kern_xxx::powerbutton_event;
use crate::kern::subr_autoconf::{config_found, config_found_sm};
use crate::kern::subr_prf::{Str, panic};
use crate::machine::acpi_machdep::{
    ACPI_PRT, ACPI_SECTWO, acpi_attach_machdep, acpi_bus_space_map, acpi_bus_space_unmap,
    acpi_iommu_device_map, acpi_map, acpi_unmap, ci_acpi_proc_id, cpu_suspended, pwr_action,
};
use crate::machine::bus::{
    BusAddr, BusSize, BusSpaceHandle, BusSpaceTag, bus_space_map, bus_space_read_1,
    bus_space_read_2, bus_space_read_4, bus_space_write_1, bus_space_write_2, bus_space_write_4,
};
use crate::machine::cpu::{CpuInfo, cpu_info_foreach, delay, intr_disable};
use crate::machine::intr::{splbio, splhigh, splx};
use crate::machine::pci_machdep::{
    PciChipsetTag, Pcitag, pci_conf_read, pci_conf_write, pci_decompose_tag, pci_lookup_segment,
    pci_make_tag,
};
use crate::sys::device::{
    CD_COCOVM, CfMatch, Cfdriver, CfprintT, DV_DULL, Device, QUIET, SLEEP_SUSPEND, UNCONF,
};
use crate::sys::errno::Errno;
use crate::sys::param::PWAIT;
use crate::sys::reboot::RB_POWERDOWN;
use crate::sys::systm::{COLD, INFSLP};
use crate::sys::types::Paddr;
use crate::{kassert, kprintf, unported};

/// `ACPIEN_RETRIES`: how many times `acpi_enable` reads PM1 waiting for `SCI_EN`.
const ACPIEN_RETRIES: i32 = 15;

/// `APM_NORMAL_RESUME` (`<machine/apmvar.h>`, not ported): the event `acpi_record_event`
/// reports after a resume.
pub const APM_NORMAL_RESUME: u32 = 0x0003;
/// `APM_POWER_CHANGE` (`<machine/apmvar.h>`): the A/C or battery status may have changed.
pub const APM_POWER_CHANGE: u32 = 0x0006;

/// `struct acpi_gpio_event`: an event a GPIO pin signals (`_AEI`). (`struct idechnl`, the
/// IDE bay of `acpi_foundide`, is `NWD > 0`: `wd` is not configured, so it is compiled out
/// with `acpi_foundide` and `acpiide_notify`.)
struct AcpiGpioEvent {
    /// `node`: the device whose `_AEI` names the event.
    node: AmlNodeRef,
    /// `tflags`.
    tflags: u16,
    /// `pin`.
    pin: u16,
}

/// `acpi_poll_enabled`: some device asked to be polled (`ACPIDEV_POLL`) and the poll
/// timeout already runs.
pub static ACPI_POLL_ENABLED: AtomicI32 = AtomicI32::new(0);
/// `acpi_hasprocfvs`: a processor has `_PSS` frequencies (`acpicpu.c` sets it).
pub static ACPI_HASPROCFVS: AtomicI32 = AtomicI32::new(0);
/// `acpi_haspci`: ACPI found the PCI host bridges (`acpipci.c` sets it), so mainbus leaves
/// `pci0` to `acpipci_attach_busses`.
pub static ACPI_HASPCI: AtomicI32 = AtomicI32::new(0);
/// `acpi_legacy_free`: the machine has no legacy ISA devices.
pub static ACPI_LEGACY_FREE: AtomicI32 = AtomicI32::new(0);
/// `acpi_enabled`: acpi0 took the hardware over (`acpi_attach_common`).
pub static ACPI_ENABLED: AtomicI32 = AtomicI32::new(0);
/// `mouse_has_softbtn`: the touchpad is a Synaptics clickpad with a top button area.
pub static MOUSE_HAS_SOFTBTN: AtomicI32 = AtomicI32::new(0);
/// `acpi_force_bm`: or'ed into PM1 control on resume.
pub static ACPI_FORCE_BM: AtomicU32 = AtomicU32::new(0);
/// `acpi_evindex`: the index of the last event `acpi_record_event` composed.
pub static ACPI_EVINDEX: AtomicI32 = AtomicI32::new(0);

/// `sbtn_pnp[]`: the Synaptics devices with a 'top button area' (from Linux's list, which
/// Synaptics supplies). Clickpads with these PNP ids get a wscons mouse type of their own,
/// which defines trackpad regions that emulate mouse buttons.
static SBTN_PNP: [&str; 33] = [
    "LEN0017", "LEN0018", "LEN0019", "LEN0023", "LEN002A", "LEN002B", "LEN002C", "LEN002D",
    "LEN002E", "LEN0033", "LEN0034", "LEN0035", "LEN0036", "LEN0037", "LEN0038", "LEN0039",
    "LEN0041", "LEN0042", "LEN0045", "LEN0047", "LEN0049", "LEN2000", "LEN2001", "LEN2002",
    "LEN2003", "LEN2004", "LEN2005", "LEN2006", "LEN2007", "LEN2008", "LEN2009", "LEN200A",
    "LEN200B",
];

/// `acpi_cd`.
pub static ACPI_CD: Cfdriver = Cfdriver::new(b"acpi", DV_DULL, CD_COCOVM);

/// `acpi_pcidevs`: the PCI devices the namespace describes (`acpi_getpci`).
static ACPI_PCIDEVS: AmlGlobal<RefCell<Vec<&'static AcpiPci>>> =
    AmlGlobal::new(RefCell::new(Vec::new()));
/// `acpi_pcirootdevs`: the PCI host bridges.
static ACPI_PCIROOTDEVS: AmlGlobal<RefCell<Vec<&'static AcpiPci>>> =
    AmlGlobal::new(RefCell::new(Vec::new()));

/// `acpi_taskq`: the work queued for the acpi thread.
static ACPI_TASKQ: AmlGlobal<RefCell<VecDeque<AcpiTaskq>>> =
    AmlGlobal::new(RefCell::new(VecDeque::new()));

/// `acpi_skip_hids[]`: devices for which we don't want to attach a driver.
static ACPI_SKIP_HIDS: [&str; 13] = [
    "INT0800", // Intel 82802Firmware Hub Device
    "PNP0000", // 8259-compatible Programmable Interrupt Controller
    "PNP0001", // EISA Interrupt Controller
    "PNP0100", // PC-class System Timer
    "PNP0103", // HPET System Timer
    "PNP0200", // PC-class DMA Controller
    "PNP0201", // EISA DMA Controller
    "PNP0800", // Microsoft Sound System Compatible Device
    "PNP0C01", // System Board
    "PNP0C02", // PNP Motherboard Resources
    "PNP0C04", // x87-compatible Floating Point Processing Unit
    "PNP0C09", // Embedded Controller Device
    "PNP0C0F", // PCI Interrupt Link Device
];

/// `acpi_isa_hids[]`: ISA devices for which we attach a driver later.
static ACPI_ISA_HIDS: [&str; 3] = [
    "PNP0400", // Standard LPT Parallel Port
    "PNP0401", // ECP Parallel Port
    "PNP0700", // PC-class Floppy Disk Controller
];

/// `acpi_quiet_hids[]`: overly abundant devices to avoid printing details for.
static ACPI_QUIET_HIDS: [&str; 1] = ["ACPI0007"];

/// `DEVNAME(sc)`: acpi0's name.
fn devname(sc: &AcpiSoftc) -> &str {
    sc.sc_dev.xname()
}

/// `sc->sc_fadt`, which `acpi_attach_common` sets before anything reads it (acpi0's
/// children read it too: `acpitimer`).
pub fn fadt(sc: &AcpiSoftc) -> &'static AcpiFadt {
    let p = sc.sc_fadt.get();
    kassert!(!p.is_null());
    // SAFETY: `sc_fadt` points at the FADT's copy, which `acpi_maptable` allocated with at
    // least `size_of::<AcpiFadt>()` bytes and never frees; the structure is packed
    // (alignment 1), so any address is aligned, and its members are read by value.
    unsafe { &*p }
}

/// The bytes of a table `acpi_maptable` copied, `hdr.length` of them.
pub fn q_table_bytes(entry: &AcpiQ) -> &'static [u8] {
    let hdr = entry.q_table.cast_const().cast::<u8>();
    // SAFETY: `q_table` is the start of a copy at least as long as its header says (and
    // than a header), never freed.
    let h = unsafe { ptr::read_unaligned(hdr.cast::<AcpiTableHeader>()) };
    // SAFETY: as above.
    unsafe { slice::from_raw_parts(hdr, h.length as usize) }
}

/// The NUL-terminated string of an attach argument (`aaa_dev`, `aaa_cdev`), without its NUL.
fn aaa_str<'a>(p: *const u8) -> Option<&'a [u8]> {
    if p.is_null() {
        return None;
    }
    let mut n = 0;
    // SAFETY: the attaching code hands NUL-terminated strings that live for the duration of
    // `config_found` (`acpivar.rs`, `struct acpi_attach_args`).
    while unsafe { *p.add(n) } != 0 {
        n += 1;
    }
    // SAFETY: the `n` bytes before the NUL, read above.
    Some(unsafe { slice::from_raw_parts(p, n) })
}

/// `aa->aaa_name` without its NUL, `None` when NULL: the name acpi0's children compare
/// against their driver's (`acpitimer`).
pub fn aaa_name(aa: &AcpiAttachArgs) -> Option<&[u8]> {
    aaa_str(aa.aaa_name)
}

/// `memset(&aaa, 0, sizeof(aaa))` with `aaa_iot` and `aaa_memt` from acpi0.
fn acpi_attach_args(sc: &AcpiSoftc) -> AcpiAttachArgs {
    AcpiAttachArgs {
        aaa_name: ptr::null(),
        aaa_iot: sc.sc_iot.get(),
        aaa_memt: sc.sc_memt.get(),
        aaa_dmat: None,
        aaa_table: ptr::null_mut(),
        aaa_node: None,
        aaa_dev: ptr::null(),
        aaa_cdev: ptr::null(),
        aaa_addr: [0; 8],
        aaa_size: [0; 8],
        aaa_bst: [None; 8],
        aaa_naddr: 0,
        aaa_irq: [0; 8],
        aaa_irq_flags: [0; 8],
        aaa_nirq: 0,
    }
}

/// `config_found(&sc->sc_dev, &aaa, print)`.
fn acpi_config_found(sc: &AcpiSoftc, aaa: &mut AcpiAttachArgs, print: CfprintT) {
    let _ = config_found(&sc.sc_dev, ptr::from_mut(aaa).cast(), Some(print));
}

/// `strlcpy(dst, src, sizeof(dst))`.
fn strlcpy(dst: &mut [u8], src: &[u8]) {
    let n = src.len().min(dst.len().saturating_sub(1));
    dst[..n].copy_from_slice(&src[..n]);
    if let Some(z) = dst.get_mut(n) {
        *z = 0;
    }
}

/// The value `v` refers to when it is an `ObjRef` (`v->v_objref.ref`), else `v`.
fn deref_objref(v: AmlValueRef) -> AmlValueRef {
    if v.r#type() == AML_OBJTYPE_OBJREF
        && let Some(r) = v.v_objref().and_then(|o| o.r#ref)
    {
        return r;
    }
    v
}

/// `acpi_pci_conf_read_1(pc, tag, reg)`.
pub fn acpi_pci_conf_read_1(pc: PciChipsetTag, tag: Pcitag, reg: i32) -> u8 {
    let val = pci_conf_read(pc, tag, reg & !0x3);
    (val >> ((reg & 0x3) << 3)) as u8
}

/// `acpi_pci_conf_read_2(pc, tag, reg)`.
pub fn acpi_pci_conf_read_2(pc: PciChipsetTag, tag: Pcitag, reg: i32) -> u16 {
    let val = pci_conf_read(pc, tag, reg & !0x2);
    (val >> ((reg & 0x2) << 3)) as u16
}

/// `acpi_pci_conf_read_4(pc, tag, reg)`.
pub fn acpi_pci_conf_read_4(pc: PciChipsetTag, tag: Pcitag, reg: i32) -> u32 {
    pci_conf_read(pc, tag, reg)
}

/// `acpi_pci_conf_write_1(pc, tag, reg, val)`.
pub fn acpi_pci_conf_write_1(pc: PciChipsetTag, tag: Pcitag, reg: i32, val: u8) {
    let mut tmp = pci_conf_read(pc, tag, reg & !0x3);
    tmp &= !(0xff << ((reg & 0x3) << 3));
    tmp |= u32::from(val) << ((reg & 0x3) << 3);
    pci_conf_write(pc, tag, reg & !0x3, tmp);
}

/// `acpi_pci_conf_write_2(pc, tag, reg, val)`.
pub fn acpi_pci_conf_write_2(pc: PciChipsetTag, tag: Pcitag, reg: i32, val: u16) {
    let mut tmp = pci_conf_read(pc, tag, reg & !0x2);
    tmp &= !(0xffff << ((reg & 0x2) << 3));
    tmp |= u32::from(val) << ((reg & 0x2) << 3);
    pci_conf_write(pc, tag, reg & !0x2, tmp);
}

/// `acpi_pci_conf_write_4(pc, tag, reg, val)`.
pub fn acpi_pci_conf_write_4(pc: PciChipsetTag, tag: Pcitag, reg: i32, val: u32) {
    pci_conf_write(pc, tag, reg, val);
}

/// The `n` bytes of `buffer` at `at`, or `None` past its end (the C writes past it).
fn chunk(buffer: &mut [u8], at: usize, n: usize) -> Option<&mut [u8]> {
    buffer.get_mut(at..at + n)
}

/// `acpi_gasio(sc, iodir, iospace, address, access_size, len, buffer)`: reads or writes
/// `len` bytes of address space `iospace` (`GAS_*`) at `address`, `access_size` bytes at a
/// time, into or from `buffer`; 0, or -1 when the space cannot be reached.
pub fn acpi_gasio(
    sc: Option<&AcpiSoftc>,
    iodir: i32,
    iospace: i32,
    address: u64,
    access_size: i32,
    len: i32,
    buffer: &mut [u8],
) -> i32 {
    let Some(sc) = sc else {
        return -1;
    };

    kassert!(access_size != 0 && len % access_size == 0);

    let step = access_size.max(1) as usize;
    let len_u = len.max(0) as usize;
    match iospace {
        GAS_SYSTEM_MEMORY | GAS_SYSTEM_IOSPACE => {
            let iot = if iospace == GAS_SYSTEM_MEMORY {
                sc.sc_memt.get()
            } else {
                sc.sc_iot.get()
            };
            let Some(iot) = iot else {
                kprintf!("{}: unable to map iospace\n", devname(sc));
                return -1;
            };

            // SAFETY: the address comes from the firmware's tables (an operation region, a
            // fixed register), which is what `acpi_bus_space_map` is for.
            let Ok(ioh) = (unsafe { acpi_bus_space_map(iot, address as BusAddr, len_u, 0) }) else {
                kprintf!("{}: unable to map iospace\n", devname(sc));
                return -1;
            };
            let mut reg = 0;
            while reg < len_u {
                if let Some(b) = chunk(buffer, reg, step) {
                    if iodir == ACPI_IOREAD {
                        match access_size {
                            1 => b[0] = bus_space_read_1(iot, ioh, reg),
                            2 => b.copy_from_slice(&bus_space_read_2(iot, ioh, reg).to_le_bytes()),
                            4 => b.copy_from_slice(&bus_space_read_4(iot, ioh, reg).to_le_bytes()),
                            _ => {
                                kprintf!("{}: rdio: invalid size {}\n", devname(sc), access_size);
                                return -1;
                            }
                        }
                    } else {
                        match access_size {
                            1 => bus_space_write_1(iot, ioh, reg, b[0]),
                            2 => bus_space_write_2(iot, ioh, reg, u16::from_le_bytes([b[0], b[1]])),
                            4 => bus_space_write_4(
                                iot,
                                ioh,
                                reg,
                                u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
                            ),
                            _ => {
                                kprintf!("{}: wrio: invalid size {}\n", devname(sc), access_size);
                                return -1;
                            }
                        }
                    }
                }
                reg += step;
            }
            acpi_bus_space_unmap(iot, ioh, len_u);
        }

        GAS_PCI_CFG_SPACE => {
            // The ACPI standard says that a function number of FFFF can be used to refer
            // to all functions on a device. This makes no sense though in the context of
            // accessing PCI config space. Yet there is AML out there that does this. We
            // simulate a read from a nonexistent device here. Writes will panic when we
            // try to construct the tag below.
            if acpi_pci_fn(address) == 0xffff && iodir == ACPI_IOREAD {
                let n = len_u.min(buffer.len());
                buffer[..n].fill(0xff);
                return 0;
            }

            let Some(pc) = pci_lookup_segment(
                i32::from(acpi_pci_seg(address)),
                i32::from(acpi_pci_bus(address)),
            ) else {
                return -1;
            };
            let tag = pci_make_tag(
                pc,
                i32::from(acpi_pci_bus(address)),
                i32::from(acpi_pci_dev(address)),
                i32::from(acpi_pci_fn(address)),
            );

            let reg = i32::from(acpi_pci_reg(address));
            let mut idx = 0;
            while idx < len_u {
                let r = reg + idx as i32;
                if let Some(b) = chunk(buffer, idx, step) {
                    if iodir == ACPI_IOREAD {
                        match access_size {
                            1 => b[0] = acpi_pci_conf_read_1(pc, tag, r),
                            2 => b.copy_from_slice(&acpi_pci_conf_read_2(pc, tag, r).to_le_bytes()),
                            4 => b.copy_from_slice(&acpi_pci_conf_read_4(pc, tag, r).to_le_bytes()),
                            _ => {
                                kprintf!("{}: rdcfg: invalid size {}\n", devname(sc), access_size);
                                return -1;
                            }
                        }
                    } else {
                        match access_size {
                            1 => acpi_pci_conf_write_1(pc, tag, r, b[0]),
                            2 => {
                                acpi_pci_conf_write_2(pc, tag, r, u16::from_le_bytes([b[0], b[1]]))
                            }
                            4 => acpi_pci_conf_write_4(
                                pc,
                                tag,
                                r,
                                u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
                            ),
                            _ => {
                                kprintf!("{}: wrcfg: invalid size {}\n", devname(sc), access_size);
                                return -1;
                            }
                        }
                    }
                }
                idx += step;
            }
        }

        GAS_EMBEDDED => {
            if sc.sc_ec.get().is_null() {
                kprintf!("{}: WARNING EC not initialized\n", devname(sc));
                return -1;
            }
            // acpiec_read/acpiec_write(sc->sc_ec, (uint8_t)address, len, buffer): sc_ec is
            // set by acpiec.c, not ported (unreachable while it stays NULL).
            return -1;
        }
        _ => {}
    }
    0
}

/// `acpi_inidev(node, sc)`: `aml_find_node`'s callback for `_INI`: runs it when the device
/// is present; nonzero stops the walk below a device that is neither present nor enabled.
fn acpi_inidev(node: &AmlNodeRef, sc: &AcpiSoftc) -> i32 {
    // Per the ACPI spec 6.5.1, only run _INI when device is there or when there is no
    // _STA. We terminate the tree walk (with return 1) early if necessary.

    // Evaluate _STA to decide _INI fate and walk fate
    let sta = acpi_getsta(sc, node.parent().as_ref());

    // Evaluate _INI if we are present
    if sta & i64::from(STA_PRESENT) != 0 {
        aml_evalnode(Some(sc), Some(node), &[], None);
    }

    // If we are functioning, we walk/search our children
    if sta & i64::from(STA_DEV_OK) != 0 {
        return 0;
    }

    // If we are not enabled, or not present, terminate search
    if sta & i64::from(STA_PRESENT | STA_ENABLED) == 0 {
        return 1;
    }

    // Default just continue search
    0
}

/// `acpi_foundprt(node, sc)`: attaches `acpiprt` for a `_PRT` (PCI interrupt routing).
fn acpi_foundprt(node: &AmlNodeRef, sc: &AcpiSoftc) -> i32 {
    // Evaluate _STA to decide _PRT fate and walk fate
    let sta = acpi_getsta(sc, node.parent().as_ref());
    if sta & i64::from(STA_PRESENT) != 0 {
        let mut aaa = acpi_attach_args(sc);
        aaa.aaa_node = Some(node.clone());
        aaa.aaa_name = c"acpiprt".as_ptr().cast();

        acpi_config_found(sc, &mut aaa, acpi_print);
    }

    // If we are functioning, we walk/search our children
    if sta & i64::from(STA_DEV_OK) != 0 {
        return 0;
    }

    // If we are not enabled, or not present, terminate search
    if sta & i64::from(STA_PRESENT | STA_ENABLED) == 0 {
        return 1;
    }

    // Default just continue search
    0
}

/// `acpi_getminbus(crsidx, crs, &bbn)`: a host bridge's first bus number, from the bus
/// range of its `_CRS`; -1 for an invalid range.
fn acpi_getminbus(_crsidx: i32, crs: &AcpiResource<'_>, bbn: &mut i32) -> i32 {
    let typ = aml_crstype(crs);

    // Check for embedded bus number
    if typ == LR_WORD && crs.lr_word_type() == 2 {
        // If _MIN > _MAX, the resource is considered to be invalid.
        if crs.lr_word__min() > crs.lr_word__max() {
            return -1;
        }
        *bbn = i32::from(crs.lr_word__min());
    }
    0
}

/// `acpi_matchcls(aaa, class, subclass, interface)`: the device's `_CLS` is that PCI
/// class code.
pub fn acpi_matchcls(aaa: &AcpiAttachArgs, class: i64, subclass: i64, interface: i64) -> i32 {
    let sc = acpi_softc();

    if aaa.aaa_dev.is_null() || aaa.aaa_node.is_none() {
        return 0;
    }

    let res = AmlValue::new();
    if aml_evalname(sc, aaa.aaa_node.as_ref(), b"_CLS", &[], Some(&res)) != 0 {
        return 0;
    }

    let int = |i: usize| {
        res.v_package(i)
            .filter(|v| v.r#type() == AML_OBJTYPE_INTEGER)
            .map(|v| v.v_integer())
    };
    if res.r#type() != AML_OBJTYPE_PACKAGE || res.length() != 3 {
        return 0;
    }
    let (Some(c), Some(s), Some(i)) = (int(0), int(1), int(2)) else {
        return 0;
    };

    i32::from(c == class && s == subclass && i == interface)
}

/// `_acpi_matchhids(hid, hids)`: `hid` is one of `hids`.
pub fn _acpi_matchhids(hid: &[u8], hids: &[&str]) -> i32 {
    i32::from(hids.iter().any(|h| h.as_bytes() == hid))
}

/// `acpi_matchhids(aa, hids, driver)`: 2 when the device's `_HID` is one of `hids`, 1 when
/// its `_CID` is, else 0.
pub fn acpi_matchhids(aa: &AcpiAttachArgs, hids: &[&str], _driver: &str) -> i32 {
    if aa.aaa_node.is_none() {
        return 0;
    }
    let Some(dev) = aaa_str(aa.aaa_dev) else {
        return 0;
    };

    if _acpi_matchhids(dev, hids) != 0 {
        return 2;
    }
    if let Some(cdev) = aaa_str(aa.aaa_cdev)
        && _acpi_matchhids(cdev, hids) != 0
    {
        return 1;
    }

    0
}

/// `acpi_getsta(sc, node)`: the device's `_STA`, or everything present and working when it
/// has none.
pub fn acpi_getsta(sc: &AcpiSoftc, node: Option<&AmlNodeRef>) -> i64 {
    let mut sta = 0;

    if aml_evalinteger(Some(sc), node, b"_STA", &[], &mut sta) != 0 {
        sta = i64::from(STA_PRESENT | STA_ENABLED | STA_SHOW_UI | STA_DEV_OK | STA_BATTERY);
    }

    sta
}

/// The UUID of `_DSD` device properties, daffd814-6eba-4d8c-8a91-bc9bbf4aa301.
const DSD_PROP_GUID: [u8; 16] = [
    0x14, 0xd8, 0xff, 0xda, 0xba, 0x6e, 0x8c, 0x4d, 0x8a, 0x91, 0xbc, 0x9b, 0xbf, 0x4a, 0xa3, 0x01,
];

/// The properties package of a `_DSD` result `dsd` whose UUID is `guid`.
fn dsd_properties(dsd: &AmlValue, guid: &[u8; 16]) -> Option<AmlValueRef> {
    if dsd.r#type() != AML_OBJTYPE_PACKAGE || dsd.length() != 2 {
        return None;
    }
    let uuid = dsd.v_package(0)?;
    let props = dsd.v_package(1)?;
    if uuid.r#type() != AML_OBJTYPE_BUFFER || props.r#type() != AML_OBJTYPE_PACKAGE {
        return None;
    }

    // Check UUID.
    if uuid.length() as usize != guid.len() || uuid.v_buffer()[..] != guid[..] {
        return None;
    }
    Some(props)
}

/// The value of property `prop` in a `_DSD` properties package, an `ObjRef` followed.
fn dsd_property(props: &AmlValue, prop: &[u8]) -> Option<AmlValueRef> {
    // Check properties.
    for i in 0..props.length().max(0) as usize {
        let Some(res) = props.v_package(i) else {
            continue;
        };
        let Some(name) = res.v_package(0) else {
            continue;
        };
        if res.r#type() != AML_OBJTYPE_PACKAGE
            || res.length() != 2
            || name.r#type() != AML_OBJTYPE_STRING
            || name.v_string() != cstr(prop)
        {
            continue;
        }
        return res.v_package(1).map(deref_objref);
    }
    None
}

/// `acpi_storaged3enable(sc, node)`: the device's `_DSD` sets `StorageD3Enable`.
pub fn acpi_storaged3enable(sc: &AcpiSoftc, node: &AmlNodeRef) -> i32 {
    // 5025030f-842f-4ab4-a561-99a5189762d0
    const PROP_GUID: [u8; 16] = [
        0x0f, 0x03, 0x25, 0x50, 0x2f, 0x84, 0xb4, 0x4a, 0xa5, 0x61, 0x99, 0xa5, 0x18, 0x97, 0x62,
        0xd0,
    ];

    let dsd = AmlValue::new();
    if aml_evalname(Some(sc), Some(node), b"_DSD", &[], Some(&dsd)) != 0 {
        return 0;
    }
    let Some(props) = dsd_properties(&dsd, &PROP_GUID) else {
        return 0;
    };
    match dsd_property(&props, b"StorageD3Enable") {
        Some(val) if val.r#type() == AML_OBJTYPE_INTEGER => val.v_integer() as i32,
        _ => 0,
    }
}

/// A new `struct acpi_pci`, zeroed (`M_ZERO`).
fn acpi_pci_new() -> AcpiPci {
    AcpiPci {
        next: Default::default(),
        node: RefCell::new(None),
        device: Cell::new(None),
        sub: Cell::new(0),
        seg: Cell::new(0),
        bus: Cell::new(0),
        dev: Cell::new(0),
        fun: Cell::new(0),
        _s0w: Cell::new(0),
        _s3d: Cell::new(0),
        _s3w: Cell::new(0),
        _s4d: Cell::new(0),
        _s4w: Cell::new(0),
        d3cold: Cell::new(0),
    }
}

/// An integer method's value, or -1 when there is none (`_S0W`, `_S3D`, ...).
fn eval_or_minus1(sc: &AcpiSoftc, node: &AmlNodeRef, name: &[u8]) -> i32 {
    let mut val = 0;
    if aml_evalinteger(Some(sc), Some(node), name, &[], &mut val) == 0 {
        val as i32
    } else {
        -1
    }
}

/// `acpi_getpci(node, sc)`: maps an ACPI device node to PCI: host bridges go to
/// `acpi_pcirootdevs`, devices with an `_ADR` below one to `acpi_pcidevs`; nonzero stops the
/// walk below a device without PCI children.
fn acpi_getpci(node: &AmlNodeRef, sc: &AcpiSoftc) -> i32 {
    let pcihid: [&str; 3] = [ACPI_DEV_PCIB, ACPI_DEV_PCIEB, "HWP0002"];

    let sta = acpi_getsta(sc, Some(node));
    if sta & i64::from(STA_PRESENT) == 0 {
        return 0;
    }

    if node
        .value()
        .is_none_or(|v| v.r#type() != AML_OBJTYPE_DEVICE)
    {
        return 0;
    }
    let res = AmlValue::new();
    if aml_evalhid(node, &res) == 0 {
        // Check if this is a PCI Root node
        if _acpi_matchhids(&res.v_string(), &pcihid) != 0 {
            aml_freevalue(Some(&res));

            let pci = Box::leak(Box::new(acpi_pci_new()));

            pci.bus.set(-1);
            let mut val = 0;
            if aml_evalinteger(Some(sc), Some(node), b"_SEG", &[], &mut val) == 0 {
                pci.seg.set(val as i32);
            }
            if aml_evalname(Some(sc), Some(node), b"_CRS", &[], Some(&res)) == 0 {
                let mut bus = pci.bus.get();
                aml_parse_resource(&res, &mut |i, crs| acpi_getminbus(i, crs, &mut bus));
                pci.bus.set(bus);
            }
            if aml_evalinteger(Some(sc), Some(node), b"_BBN", &[], &mut val) == 0
                && pci.bus.get() == -1
            {
                pci.bus.set(val as i32);
            }
            pci.sub.set(pci.bus.get());
            node.pci.set(Some(pci));
            ACPI_PCIROOTDEVS.get().borrow_mut().push(pci);
        }
        aml_freevalue(Some(&res));
        return 0;
    }

    // If parent is not PCI, or device does not have _ADR, return
    let Some(ppci) = node.parent().and_then(|p| p.pci.get()) else {
        return 0;
    };
    let mut val = 0;
    if aml_evalinteger(Some(sc), Some(node), b"_ADR", &[], &mut val) != 0 {
        return 0;
    }

    let pci = acpi_pci_new();
    pci.seg.set(ppci.seg.get());
    pci.bus.set(ppci.sub.get());
    pci.dev.set(i32::from(acpi_adr_pcidev(val as u64)));
    pci.fun.set(i32::from(acpi_adr_pcifun(val as u64)));
    *pci.node.borrow_mut() = Some(node.clone());
    pci.sub.set(-1);

    // Collect device power state information.
    pci._s0w.set(eval_or_minus1(sc, node, b"_S0W"));
    pci._s3d.set(eval_or_minus1(sc, node, b"_S3D"));
    pci._s3w.set(eval_or_minus1(sc, node, b"_S3W"));
    pci._s4d.set(eval_or_minus1(sc, node, b"_S4D"));
    pci._s4w.set(eval_or_minus1(sc, node, b"_S4W"));
    pci.d3cold.set(acpi_storaged3enable(sc, node));

    // Check if PCI device exists
    if pci.dev.get() > 0x1F || pci.fun.get() > 7 {
        return 1;
    }
    let Some(pc) = pci_lookup_segment(pci.seg.get(), pci.bus.get()) else {
        return 1;
    };
    let tag = pci_make_tag(pc, pci.bus.get(), pci.dev.get(), pci.fun.get());
    let reg = pci_conf_read(pc, tag, PCI_ID_REG);
    if pci_vendor(reg) == PCI_VENDOR_INVALID {
        return 1;
    }
    let pci: &'static AcpiPci = Box::leak(Box::new(pci));
    node.pci.set(Some(pci));

    ACPI_PCIDEVS.get().borrow_mut().push(pci);

    // Check if this is a PCI bridge
    let reg = pci_conf_read(pc, tag, PCI_CLASS_REG);
    if pci_class(reg) == PCI_CLASS_BRIDGE && pci_subclass(reg) == PCI_SUBCLASS_BRIDGE_PCI {
        let reg = pci_conf_read(pc, tag, PPB_REG_BUSINFO);
        pci.sub.set(ppb_businfo_secondary(reg) as i32);

        // Continue scanning
        return 0;
    }

    // Device does not have children, stop scanning
    1
}

/// The `acpi_pcidevs` entry of PCI device `tag`.
fn acpi_pcidev(pc: PciChipsetTag, tag: Pcitag) -> Option<&'static AcpiPci> {
    let (bus, dev, fun) = pci_decompose_tag(pc, tag);
    ACPI_PCIDEVS
        .get()
        .borrow()
        .iter()
        .copied()
        .find(|p| p.bus.get() == bus && p.dev.get() == dev && p.fun.get() == fun)
}

/// `acpi_find_pci(pc, tag)`: the namespace node of PCI device `tag`.
pub fn acpi_find_pci(pc: PciChipsetTag, tag: Pcitag) -> Option<AmlNodeRef> {
    acpi_pcidev(pc, tag).and_then(|p| p.node.borrow().clone())
}

/// `acpi_pci_match(dev, pa)`: ties a PCI device that attached to its namespace node, sets
/// up the power resources it depends on, and listens to its wake notifications.
pub fn acpi_pci_match(dev: &'static Device, pa: &PciAttachArgs) -> Option<AmlNodeRef> {
    let pdev = ACPI_PCIDEVS.get().borrow().iter().copied().find(|p| {
        p.bus.get() == pa.pa_bus as i32
            && p.dev.get() == pa.pa_device as i32
            && p.fun.get() == pa.pa_function as i32
    })?;

    pdev.device.set(Some(dev));

    // If some Power Resources are dependent on this device initialize them.
    let state = pci_get_powerstate(pa.pa_pc, pa.pa_tag);
    acpi_pci_set_powerstate(pa.pa_pc, pa.pa_tag, state, 1);
    acpi_pci_set_powerstate(pa.pa_pc, pa.pa_tag, state, 0);

    let node = pdev.node.borrow().clone()?;
    aml_register_notify(
        &node,
        None,
        acpi_pci_notify,
        ptr::from_ref(pdev).cast_mut().cast(),
        0,
    );

    Some(node)
}

/// `acpi_pci_min_powerstate(pc, tag)`: the lowest power state the device may enter in the
/// sleep state the machine goes to.
pub fn acpi_pci_min_powerstate(pc: PciChipsetTag, tag: Pcitag) -> Pcireg {
    let mut defaultstate = pci_get_powerstate(pc, tag);

    let (bus, dev, fun) = pci_decompose_tag(pc, tag);
    let Some(sc) = acpi_softc() else {
        return defaultstate as Pcireg;
    };
    for pdev in ACPI_PCIDEVS.get().borrow().iter() {
        if pdev.bus.get() == bus && pdev.dev.get() == dev && pdev.fun.get() == fun {
            let mut state = -1;
            match sc.sc_state.get() {
                s if s == i32::from(ACPI_STATE_S0) => {
                    if BOOTHOWTO.load(Ordering::Relaxed) & RB_POWERDOWN != 0 {
                        defaultstate = PCI_PMCSR_STATE_D3 as i32;
                        state = pdev._s0w.get();
                    }
                }
                s if s == i32::from(ACPI_STATE_S3) => {
                    defaultstate = PCI_PMCSR_STATE_D3 as i32;
                    state = pdev._s3d.get().max(pdev._s3w.get());
                }
                s if s == i32::from(ACPI_STATE_S4) => {
                    state = pdev._s4d.get().max(pdev._s4w.get());
                }
                _ => {}
            }

            if state >= PCI_PMCSR_STATE_D0 as i32 && state <= PCI_PMCSR_STATE_D3 as i32 {
                return state as Pcireg;
            }
        }
    }

    defaultstate as Pcireg
}

/// `acpi_pci_set_powerstate(pc, tag, state, pre)`: runs the device's `_PSx` around a
/// power-state change (before it with `pre`, after it otherwise).
pub fn acpi_pci_set_powerstate(pc: PciChipsetTag, tag: Pcitag, state: i32, pre: i32) {
    let sc = acpi_softc();

    let Some(pdev) = acpi_pcidev(pc, tag) else {
        return;
    };
    let node = pdev.node.borrow().clone();

    if state != i32::from(ACPI_STATE_D0) && pre == 0 {
        let name = format!("_PS{state}");
        aml_evalname(sc, node.as_ref(), name.as_bytes(), &[], None);
    }

    // NACPIPWRRES > 0: the power resources of sc_pwrresdevs (acpipwrres.c, not
    // configured).

    if state == i32::from(ACPI_STATE_D0) && pre != 0 {
        aml_evalname(sc, node.as_ref(), b"_PS0", &[], None);
    }
}

/// `acpi_pci_notify(node, ntype, pdev)`: clears the PME status of a device that signalled
/// a wake (notification 2).
fn acpi_pci_notify(_node: &AmlNodeRef, ntype: i32, arg: *mut c_void) -> i32 {
    // SAFETY: `acpi_pci_match` registered a leaked `struct acpi_pci` as the argument.
    let pdev = unsafe { &*arg.cast_const().cast::<AcpiPci>() };

    // We're only interested in Device Wake notifications.
    if ntype != 2 {
        return 0;
    }

    let Some(pc) = pci_lookup_segment(pdev.seg.get(), pdev.bus.get()) else {
        return 0;
    };
    let tag = pci_make_tag(pc, pdev.bus.get(), pdev.dev.get(), pdev.fun.get());
    if let Some((offset, _)) = pci_get_capability(pc, tag, PCI_CAP_PWRMGMT) {
        // Clear the PME Status bit if it is set.
        let reg = pci_conf_read(pc, tag, offset + PCI_PMCSR);
        pci_conf_write(pc, tag, offset + PCI_PMCSR, reg);
    }

    0
}

/// `acpi_pciroots_attach(dev, pba, pr)`: attaches a `pci` bus below `dev` for every host
/// bridge the namespace describes.
pub fn acpi_pciroots_attach(dev: &Device, pba: &mut PcibusAttachArgs, pr: CfprintT) {
    // KASSERT(pba->pba_busex != NULL): extents (subr_extent.c) are not ported, so the bus
    // numbers cannot be reserved with extent_alloc_region.
    let _ = unported!("acpi_pciroots_attach: extent_alloc_region (pba_busex, subr_extent.c)");

    let roots: Vec<&'static AcpiPci> = ACPI_PCIROOTDEVS.get().borrow().clone();
    for pdev in roots {
        pba.pba_bus = pdev.bus.get();
        let _ = config_found(dev, ptr::from_mut(pba).cast(), Some(pr));
    }
}

// GPIO support

/// `acpi_gpio_event_task(ev, pin)`: runs the `_Lxx`/`_Exx` method (or `_EVT`) of a
/// GPIO-signalled event, then unmasks a level-triggered pin.
fn acpi_gpio_event_task(arg0: *mut c_void, arg1: i32) {
    let sc = acpi_softc();
    // SAFETY: `acpi_gpio_parse_events` leaked the event, which stays for good.
    let ev = unsafe { &*arg0.cast_const().cast::<AcpiGpioEvent>() };
    let gpio = ev.node.gpio.get();
    let pin = arg1 as u16;

    let mut done = false;
    if pin < 256 {
        let name = if ev.tflags & LR_GPIO_MODE == LR_GPIO_LEVEL {
            format!("_L{pin:02X}")
        } else {
            format!("_E{pin:02X}")
        };
        done = aml_evalname(sc, Some(&ev.node), name.as_bytes(), &[], None) == 0;
    }

    if !done {
        let evt = [AmlValue::integer(i64::from(pin))];
        aml_evalname(sc, Some(&ev.node), b"_EVT", &evt, None);
    }

    // intr_enable:
    if ev.tflags & LR_GPIO_MODE == LR_GPIO_LEVEL
        && let Some(gpio) = gpio
    {
        (gpio.intr_enable)(gpio.cookie, i32::from(pin));
    }
}

/// `acpi_gpio_event(ev)`: the interrupt handler of a GPIO-signalled event: masks a
/// level-triggered pin and queues the event's method for the acpi thread.
fn acpi_gpio_event(arg: *mut c_void) -> i32 {
    let Some(sc) = acpi_softc() else {
        return 0;
    };
    // SAFETY: as in `acpi_gpio_event_task`.
    let ev = unsafe { &*arg.cast_const().cast::<AcpiGpioEvent>() };

    if ev.tflags & LR_GPIO_MODE == LR_GPIO_LEVEL
        && let Some(gpio) = ev.node.gpio.get()
    {
        (gpio.intr_disable)(gpio.cookie, i32::from(ev.pin));
    }

    if cpu_suspended().load(Ordering::Relaxed) != 0 {
        cpu_suspended().store(0, Ordering::Relaxed);
    }
    if sc.sc_wakegpe.get() == WAKEGPE_NONE {
        sc.sc_wakegpe.set(WAKEGPE_GPIO);
        sc.sc_wakegpio.set(i32::from(ev.pin));
    }

    acpi_addtask(sc, acpi_gpio_event_task, arg, i32::from(ev.pin));
    acpi_wakeup(sc);

    1
}

/// `acpi_gpio_parse_events(crsidx, crs, devnode)`: establishes the interrupt of each GPIO
/// interrupt resource of an `_AEI`.
fn acpi_gpio_parse_events(_crsidx: i32, crs: &AcpiResource<'_>, devnode: &AmlNodeRef) -> i32 {
    match aml_crstype(crs) {
        LR_GPIO => {
            let res_off = usize::from(crs.lr_gpio_res_off());
            let name: Vec<u8> = crs.bytes().get(res_off..).unwrap_or(&[]).to_vec();
            let node = aml_searchname(Some(devnode), &name);
            let pin_off = usize::from(crs.lr_gpio_pin_off());
            let pin = u16::from_le_bytes([crs.byte(pin_off), crs.byte(pin_off + 1)]);
            if crs.lr_gpio_type() == LR_GPIO_INT
                && let Some(gpio) = node.as_ref().and_then(|n| n.gpio.get())
            {
                let ev = Box::into_raw(Box::new(AcpiGpioEvent {
                    node: devnode.clone(),
                    tflags: crs.lr_gpio_tflags(),
                    pin,
                }));
                (gpio.intr_establish)(
                    gpio.cookie,
                    i32::from(pin),
                    i32::from(crs.lr_gpio_tflags()),
                    crate::machine::intr::IPL_BIO | crate::machine::intr::IPL_WAKEUP,
                    acpi_gpio_event,
                    ev.cast(),
                );
            }
        }
        t => {
            kprintf!("acpi_gpio_parse_events: unknown resource type {}\n", t);
        }
    }

    0
}

/// `_REG(space, 1)`: tells the device's AML that address space `space` is available.
fn acpi_reg_space(sc: &AcpiSoftc, devnode: &AmlNodeRef, space: i32) {
    let arg = [AmlValue::integer(i64::from(space)), AmlValue::integer(1)];
    if let Some(node) = aml_searchname(Some(devnode), b"_REG")
        && aml_evalnode(Some(sc), Some(&node), &arg, None) != 0
    {
        kprintf!("{}: _REG failed\n", Str(node.name()));
    }
}

/// `acpi_register_gpio(sc, devnode)`: a GPIO controller's driver attached: registers the
/// GeneralPurposeIO address space and the events its `_AEI` signals.
pub fn acpi_register_gpio(sc: &AcpiSoftc, devnode: &AmlNodeRef) {
    // Register GeneralPurposeIO address space.
    acpi_reg_space(sc, devnode, ACPI_OPREG_GPIO);

    // Register GPIO signaled ACPI events.
    let res = AmlValue::new();
    if aml_evalname(Some(sc), Some(devnode), b"_AEI", &[], Some(&res)) != 0 {
        return;
    }
    aml_parse_resource(&res, &mut |i, crs| acpi_gpio_parse_events(i, crs, devnode));
}

/// `acpi_register_gsb(sc, devnode)`: registers the GenericSerialBus address space.
pub fn acpi_register_gsb(sc: &AcpiSoftc, devnode: &AmlNodeRef) {
    acpi_reg_space(sc, devnode, ACPI_OPREG_GSB);
}

/// `acpi_attach_common(sc, base)`: acpi0's attachment, from the RSDP at physical address
/// `base`.
pub fn acpi_attach_common(sc: &'static AcpiSoftc, base: Paddr) {
    rw_init(&sc.sc_lck, "acpilk");

    ACPI_SOFTC.store(ptr::from_ref(sc).cast_mut(), Ordering::Release);
    let root = aml_root();
    *sc.sc_root.borrow_mut() = Some(root.clone());

    let Ok(handle) = acpi_map(base, size_of::<AcpiRsdp>()) else {
        kprintf!(": can't map memory\n");
        return;
    };
    // SAFETY: `acpi_map` mapped the RSDP's bytes at `va`; the structure is packed.
    let rsdp = unsafe { ptr::read_unaligned(handle.va as *const AcpiRsdp) };

    // pool_init(&acpiwqpool, ...): the task queue is a VecDeque (see the deviations).

    sc.sc_tables.init();
    sc.sc_wakedevs.init();
    // NACPIPWRRES > 0: SIMPLEQ_INIT(&sc->sc_pwrresdevs).

    if acpi_loadtables(sc, &rsdp) != 0 {
        kprintf!(": can't load tables\n");
        acpi_unmap(&handle);
        return;
    }

    acpi_unmap(&handle);

    // Find the FADT
    for entry in sc.sc_tables.iter() {
        if q_table_bytes(entry).starts_with(FADT_SIG) {
            sc.sc_fadt.set(entry.q_table.cast_const().cast());
            break;
        }
    }
    if sc.sc_fadt.get().is_null() {
        kprintf!(": no FADT\n");
        return;
    }
    let f = fadt(sc);

    sc.sc_major.set(i32::from(f.hdr.revision));
    if sc.sc_major.get() > 4 {
        sc.sc_minor.set(i32::from(f.fadt_minor));
    }
    kprintf!(": ACPI {}.{}", sc.sc_major.get(), sc.sc_minor.get());

    // A bunch of things need to be done differently for Hardware-reduced ACPI.
    if f.hdr.revision >= 5 && { f.flags } & FADT_HW_REDUCED_ACPI != 0 {
        sc.sc_hw_reduced.set(1);
    }

    // Map Power Management registers
    acpi_map_pmregs(sc);

    // Check if we can and need to enable ACPI control.
    let pm1 = acpi_read_pmreg(sc, ACPIREG_PM1_CNT, 0) as u16;
    if pm1 & ACPI_PM1_SCI_EN == 0 && { f.smi_cmd } != 0 && f.acpi_enable == 0 && f.acpi_disable == 0
    {
        kprintf!(", ACPI control unavailable\n");
        acpi_unmap_pmregs(sc);
        return;
    }

    // Set up a pointer to the firmware control structure
    let facspa = if f.hdr.revision < 3 || { f.x_firmware_ctl } == 0 {
        u64::from(f.firmware_ctl)
    } else {
        f.x_firmware_ctl
    };

    match acpi_map(Paddr::new(facspa as usize), size_of::<AcpiFacs>()) {
        Err(_) => {
            kprintf!(" !FACS");
        }
        Ok(handle) => sc.sc_facs.set(handle.va as *mut AcpiFacs),
    }

    // Create opcode hashtable
    aml_hashopcodes();

    // Create Default AML objects
    aml_create_defaultobjects();

    // Load the DSDT from the FADT pointer -- use the extended (64-bit) pointer if it exists
    let entry = if f.hdr.revision < 3 || { f.x_dsdt } == 0 {
        acpi_maptable(sc, Paddr::new({ f.dsdt } as usize), None, None, None, -1)
    } else {
        acpi_maptable(sc, Paddr::new({ f.x_dsdt } as usize), None, None, None, -1)
    };

    match entry {
        None => {
            kprintf!(" !DSDT");
        }
        Some(entry) => {
            acpi_parse_aml(Some(sc), None, table_aml(entry));
        }
    }

    // Load SSDT's
    for entry in sc.sc_tables.iter() {
        if q_table_bytes(entry).starts_with(SSDT_SIG) {
            acpi_parse_aml(Some(sc), None, table_aml(entry));
        }
    }

    // Perform post-parsing fixups
    aml_postparse();

    // #if 0: acpi_legacy_free from FADT_LEGACY_DEVICES.

    // Find available sleeping states
    acpi_init_states(sc);

    // Find available sleep/resume related methods.
    acpi_init_pm(sc);

    // Initialize GPE handlers
    let s = splbio();
    acpi_init_gpes(sc);
    splx(s);

    // some devices require periodic polling
    timeout_set(
        &sc.sc_dev_timeout,
        acpi_poll,
        ptr::from_ref(sc).cast_mut().cast(),
    );

    ACPI_ENABLED.store(1, Ordering::Relaxed);

    // Take over ACPI control. Note that once we do this, we effectively tell the system
    // that we have ownership of the ACPI hardware registers, and that SMI should leave
    // them alone
    //
    // This may prevent thermal control on some systems where that actually does work
    if pm1 & ACPI_PM1_SCI_EN == 0 && { f.smi_cmd } != 0 && acpi_enable(sc).is_err() {
        kprintf!(", can't enable ACPI\n");
        return;
    }

    kprintf!("\n{}: tables", devname(sc));
    for entry in sc.sc_tables.iter() {
        kprintf!(" {}", Str(&q_table_bytes(entry)[..4]));
    }
    kprintf!("\n");

    // Display wakeup devices and lowest S-state
    let mut wakeup_dev_ct = 0;
    kprintf!("{}: wakeup devices", devname(sc));
    for wentry in sc.sc_wakedevs.iter() {
        if wakeup_dev_ct < 16 {
            let name = wentry
                .q_node
                .borrow()
                .as_ref()
                .map(|n| n.name.to_vec())
                .unwrap_or_default();
            kprintf!(
                " {}(S{})",
                Str(&name[..name.len().min(4)]),
                wentry.q_state.get()
            );
        } else if wakeup_dev_ct == 16 {
            kprintf!(" [...]");
        }
        wakeup_dev_ct += 1;
    }
    kprintf!("\n");

    // SUSPEND
    if wakeup_dev_ct > 0 {
        let _ = unported!("device_register_wakeup (subr_suspend.c)");
    }

    // ACPI is enabled now -- attach timer
    if sc.sc_hw_reduced.get() == 0 && ({ f.pm_tmr_blk } != 0 || { f.x_pm_tmr_blk.address } != 0) {
        let mut aaa = acpi_attach_args(sc);
        aaa.aaa_name = c"acpitimer".as_ptr().cast();
        acpi_config_found(sc, &mut aaa, acpi_print);
    }

    // Attach table-defined devices
    for entry in sc.sc_tables.iter() {
        let mut aaa = acpi_attach_args(sc);
        aaa.aaa_dmat = sc.sc_ci_dmat.get();
        aaa.aaa_table = entry.q_table;
        let _ = config_found_sm(
            &sc.sc_dev,
            ptr::from_mut(&mut aaa).cast(),
            Some(acpi_print),
            Some(acpi_submatch),
        );
    }

    // initialize runtime environment
    aml_find_node(&root, b"_INI", &mut |n| acpi_inidev(n, sc));

    if ACPI_SECTWO {
        aml_find_node(&root, b"ECTC", &mut |n| acpi_foundsectwo(n, sc));
    }

    // Get PCI mapping
    aml_walknodes(Some(&root), AML_WALK_PRE, &mut |n| acpi_getpci(n, sc));

    // attach pci interrupt routing tables (amd64, i386)
    if ACPI_PRT {
        aml_find_node(&root, b"_PRT", &mut |n| acpi_foundprt(n, sc));
    }

    aml_find_node(&root, b"_HID", &mut |n| acpi_foundec(n, sc));

    // check if we're running on a sony
    aml_find_node(&root, b"GBRT", &mut |n| acpi_foundsony(n, sc));

    // try to find smart battery first
    aml_find_node(&root, b"_HID", &mut |n| acpi_foundsbs(n, sc));

    // attach battery, power supply and button devices
    aml_find_node(&root, b"_HID", &mut |n| acpi_foundhid(n, sc));

    aml_walknodes(Some(&root), AML_WALK_PRE, &mut |n| acpi_add_device(n, sc));

    // NWD > 0: attach IDE bay (acpi_foundide): wd is not configured.

    // attach docks
    aml_find_node(&root, b"_DCK", &mut |n| acpi_founddock(n, sc));

    // attach video
    aml_find_node(&root, b"_DOS", &mut |n| acpi_foundvideo(n, sc));

    // create list of devices we want to query when APM comes in: sc_ac, sc_bat and sc_sbs
    // from the acpiac, acpibat and acpisbs devices in alldevs (see the deviations: none of
    // those drivers exists, so the lists would stay empty).

    // Setup threads
    let thread = Box::into_raw(Box::new(AcpiThread {
        sc: Cell::new(Some(sc)),
        running: AtomicI32::new(1),
    }));
    sc.sc_thread.set(thread);

    // Enable PCI Power Management.
    PCI_DOPM.store(1, Ordering::Relaxed);

    acpi_attach_machdep(sc);

    kthread_create_deferred(acpi_create_thread, ptr::from_ref(sc).cast_mut().cast());
}

/// The byte code of a definition block: the bytes after its header.
fn table_aml(entry: &AcpiQ) -> &'static [u8] {
    q_table_bytes(entry)
        .get(size_of::<AcpiTableHeader>()..)
        .unwrap_or(&[])
}

/// `acpi_submatch(parent, match, aux)`: only attachment arguments that carry a table are
/// offered to the drivers (`config_found_sm` for the tables).
pub fn acpi_submatch(parent: Option<&Device>, match_: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: acpi0 hands its children `struct acpi_attach_args`.
    let aaa = unsafe { &*aux.cast_const().cast::<AcpiAttachArgs>() };
    let cf = match_.cfdata();

    if aaa.aaa_table.is_null() {
        return 0;
    }
    cf.cf_attach
        .ca_match
        .map_or(0, |ca_match| ca_match(parent, match_, aux))
}

/// `acpi_noprint(aux, pnp)`: prints nothing.
pub fn acpi_noprint(_aux: *mut c_void, _pnp: Option<&[u8]>) -> i32 {
    QUIET
}

/// `acpi_print(aux, pnp)`: names a child that found no driver.
pub fn acpi_print(aux: *mut c_void, pnp: Option<&[u8]>) -> i32 {
    // SAFETY: acpi0 hands its children `struct acpi_attach_args`.
    let aa = unsafe { &*aux.cast_const().cast::<AcpiAttachArgs>() };

    if let Some(pnp) = pnp {
        if let Some(name) = aaa_str(aa.aaa_name) {
            kprintf!("{} at {}", Str(name), Str(pnp));
        } else if let Some(dev) = aaa_str(aa.aaa_dev) {
            kprintf!("\"{}\" at {}", Str(dev), Str(pnp));
        } else {
            return QUIET;
        }
    }

    UNCONF
}

/// A table `acpi_maptable` validated and copied: its id and its zero-padded copy.
struct TableCopy {
    /// `q_id`.
    id: i32,
    /// `q_data`.
    data: Vec<u8>,
}

/// `tblid`: the id of the last table `acpi_maptable` copied.
static TBLID: AtomicI32 = AtomicI32::new(0);

/// `acpi_maptable`'s mapping, validation and copy (see the deviations).
fn acpi_maptable_copy(
    sc: &AcpiSoftc,
    addr: Paddr,
    sig: Option<&[u8]>,
    oem: Option<&[u8]>,
    tbl: Option<&[u8]>,
) -> Option<TableCopy> {
    let hlen = size_of::<AcpiTableHeader>();

    // Check if we can map address
    if addr.as_usize() == 0 {
        return None;
    }
    let handle = acpi_map(addr, hlen).ok()?;
    // SAFETY: `acpi_map` mapped `hlen` bytes at `va`; the header is packed.
    let hdr = unsafe { ptr::read_unaligned(handle.va as *const AcpiTableHeader) };
    let len = hdr.length as usize;
    acpi_unmap(&handle);
    if len < hlen {
        return None;
    }

    // Validate length/checksum
    let handle = acpi_map(addr, len).ok()?;
    // SAFETY: `acpi_map` mapped `len` bytes at `va`, until `acpi_unmap` below.
    let bytes = unsafe { slice::from_raw_parts(handle.va as *const u8, len) };
    if acpi_checksum(bytes) != 0 {
        kprintf!("\n{}: {} checksum error", devname(sc), Str(&hdr.signature));
    }

    if sig.is_some_and(|s| s.get(..4) != Some(&hdr.signature[..]))
        || oem.is_some_and(|o| o.get(..6) != Some(&hdr.oemid[..]))
        || tbl.is_some_and(|t| t.get(..8) != Some(&hdr.oemtableid[..]))
    {
        acpi_unmap(&handle);
        return None;
    }

    // Allocate copy
    let mut data = Vec::new();
    let copy = if data
        .try_reserve_exact(len.max(size_of::<AcpiFadt>()))
        .is_ok()
    {
        data.extend_from_slice(bytes);
        data.resize(len.max(size_of::<AcpiFadt>()), 0);
        Some(TableCopy {
            id: TBLID.fetch_add(1, Ordering::Relaxed) + 1,
            data,
        })
    } else {
        None
    };
    acpi_unmap(&handle);
    copy
}

/// `acpi_maptable(sc, addr, sig, oem, tbl, flag)`: maps the table at physical address
/// `addr` and, if its signature (and OEM ids, when given) match, copies it into an entry
/// that goes at the head of `sc_tables` (`flag` < 0), its tail (`flag` > 0), or nowhere.
pub fn acpi_maptable(
    sc: &AcpiSoftc,
    addr: Paddr,
    sig: Option<&[u8]>,
    oem: Option<&[u8]>,
    tbl: Option<&[u8]>,
    flag: i32,
) -> Option<&'static AcpiQ> {
    let copy = acpi_maptable_copy(sc, addr, sig, oem, tbl)?;
    let data = copy.data.leak();
    let entry: &'static AcpiQ = Box::leak(Box::new(AcpiQ {
        q_next: Default::default(),
        q_id: copy.id,
        q_table: data.as_mut_ptr().cast(),
        q_data: [],
    }));

    if flag < 0 {
        // SAFETY: a new entry, in no list, leaked: it stays in place for good.
        unsafe { sc.sc_tables.insert_head(entry) };
    } else if flag > 0 {
        // SAFETY: as above.
        unsafe { sc.sc_tables.insert_tail(entry) };
    }
    Some(entry)
}

/// `acpi_loadtables(sc, rsdp)`: copies every table the XSDT (or, for an ACPI 1.0 RSDP, the
/// RSDT) lists into `sc_tables`; 0 or `ENOMEM`.
pub fn acpi_loadtables(sc: &AcpiSoftc, rsdp: &AcpiRsdp) -> i32 {
    let hlen = size_of::<AcpiTableHeader>();
    let xsdt = { rsdp.rsdp_xsdt };
    let (sdt_addr, width, what) = if rsdp.rsdp1.revision == 2 && xsdt != 0 {
        (Paddr::new(xsdt as usize), 8, "xsdt")
    } else {
        (Paddr::new({ rsdp.rsdp1.rsdt } as usize), 4, "rsdt")
    };

    let Some(sdt) = acpi_maptable_copy(sc, sdt_addr, None, None, None) else {
        kprintf!("couldn't map {}\n", what);
        return Errno::ENOMEM as i32;
    };

    // SAFETY: the copy holds at least a header; it is packed.
    let hdr = unsafe { ptr::read_unaligned(sdt.data.as_ptr().cast::<AcpiTableHeader>()) };
    let len = hdr.length as usize;
    let ntables = (len - hlen) / width;

    for i in 0..ntables {
        let at = hlen + i * width;
        let addr = if width == 8 {
            let mut b = [0u8; 8];
            b.copy_from_slice(&sdt.data[at..at + 8]);
            u64::from_le_bytes(b)
        } else {
            let mut b = [0u8; 4];
            b.copy_from_slice(&sdt.data[at..at + 4]);
            u64::from(u32::from_le_bytes(b))
        };
        acpi_maptable(sc, Paddr::new(addr as usize), None, None, None, 1);
    }

    0
}

/// The mapped register `reg` (`sc_pmregs[reg]`): its handle and the width of an access.
fn pmreg_access(sc: &AcpiSoftc, reg: i32) -> Option<(BusSpaceTag, BusSpaceHandle, i32)> {
    let r = sc.sc_pmregs.get(usize::try_from(reg).ok()?)?;
    if r.size.get() == 0 {
        return None;
    }
    let size = r.size.get().min(r.access.get());
    Some((sc.sc_iot.get()?, r.ioh.get()?, size))
}

/// `acpi_read_pmreg(sc, reg, offset)`: reads fixed register `reg` (`ACPIREG_*`); the
/// `ACPIREG_PM1_*` pseudo-registers read both blocks or'ed together.
pub fn acpi_read_pmreg(sc: &AcpiSoftc, reg: i32, offset: i32) -> i32 {
    let mut reg = reg;

    // For Hardware-reduced ACPI we emulate PM1B_CNT to reflect that the system is always
    // in ACPI mode.
    if sc.sc_hw_reduced.get() != 0 && reg == ACPIREG_PM1B_CNT {
        kassert!(offset == 0);
        return i32::from(ACPI_PM1_SCI_EN);
    }

    // For Hardware-reduced ACPI we also emulate PM1A_STS using SLEEP_STATUS_REG.
    if sc.sc_hw_reduced.get() != 0 && reg == ACPIREG_PM1A_STS {
        let g = { fadt(sc).sleep_status_reg };
        if g.register_bit_width > 0 {
            let mut value = [0u8; 1];

            kassert!(offset == 0);
            acpi_gasio(
                Some(sc),
                ACPI_IOREAD,
                i32::from(g.address_space_id),
                g.address,
                i32::from(g.register_bit_width / 8),
                i32::from(g.access_size),
                &mut value,
            );
            return i32::from(value[0]) << 8;
        }
    }

    // Special cases: 1A/1B blocks can be OR'ed together
    match reg {
        ACPIREG_PM1_EN => {
            return acpi_read_pmreg(sc, ACPIREG_PM1A_EN, offset)
                | acpi_read_pmreg(sc, ACPIREG_PM1B_EN, offset);
        }
        ACPIREG_PM1_STS => {
            return acpi_read_pmreg(sc, ACPIREG_PM1A_STS, offset)
                | acpi_read_pmreg(sc, ACPIREG_PM1B_STS, offset);
        }
        ACPIREG_PM1_CNT => {
            return acpi_read_pmreg(sc, ACPIREG_PM1A_CNT, offset)
                | acpi_read_pmreg(sc, ACPIREG_PM1B_CNT, offset);
        }
        ACPIREG_GPE_STS if offset < i32::from(fadt(sc).gpe0_blk_len >> 1) => {
            reg = ACPIREG_GPE0_STS;
        }
        ACPIREG_GPE_EN if offset < i32::from(fadt(sc).gpe0_blk_len >> 1) => {
            reg = ACPIREG_GPE0_EN;
        }
        _ => {}
    }

    if reg as usize >= ACPIREG_MAXREG {
        return 0;
    }
    let Some((iot, ioh, size)) = pmreg_access(sc, reg) else {
        return 0;
    };

    let o = offset as BusSize;
    match size {
        1 => i32::from(bus_space_read_1(iot, ioh, o)),
        2 => i32::from(bus_space_read_2(iot, ioh, o)),
        4 => bus_space_read_4(iot, ioh, o) as i32,
        _ => 0,
    }
}

/// `acpi_write_pmreg(sc, reg, offset, regval)`: writes fixed register `reg`; the
/// `ACPIREG_PM1_*` pseudo-registers write both blocks.
pub fn acpi_write_pmreg(sc: &AcpiSoftc, reg: i32, offset: i32, regval: i32) {
    let mut reg = reg;

    // For Hardware-reduced ACPI we also emulate PM1A_STS using SLEEP_STATUS_REG, and
    // PM1A_CNT using SLEEP_CONTROL_REG.
    if sc.sc_hw_reduced.get() != 0 && (reg == ACPIREG_PM1A_STS || reg == ACPIREG_PM1A_CNT) {
        let g = if reg == ACPIREG_PM1A_STS {
            fadt(sc).sleep_status_reg
        } else {
            fadt(sc).sleep_control_reg
        };
        if g.register_bit_width > 0 {
            let mut value = [(regval >> 8) as u8];

            kassert!(offset == 0);
            acpi_gasio(
                Some(sc),
                ACPI_IOWRITE,
                i32::from(g.address_space_id),
                g.address,
                i32::from(g.register_bit_width / 8),
                i32::from(g.access_size),
                &mut value,
            );
            return;
        }
    }

    // Special cases: 1A/1B blocks can be written with same value
    match reg {
        ACPIREG_PM1_EN => {
            acpi_write_pmreg(sc, ACPIREG_PM1A_EN, offset, regval);
            acpi_write_pmreg(sc, ACPIREG_PM1B_EN, offset, regval);
        }
        ACPIREG_PM1_STS => {
            acpi_write_pmreg(sc, ACPIREG_PM1A_STS, offset, regval);
            acpi_write_pmreg(sc, ACPIREG_PM1B_STS, offset, regval);
        }
        ACPIREG_PM1_CNT => {
            acpi_write_pmreg(sc, ACPIREG_PM1A_CNT, offset, regval);
            acpi_write_pmreg(sc, ACPIREG_PM1B_CNT, offset, regval);
        }
        ACPIREG_GPE_STS if offset < i32::from(fadt(sc).gpe0_blk_len >> 1) => {
            reg = ACPIREG_GPE0_STS;
        }
        ACPIREG_GPE_EN if offset < i32::from(fadt(sc).gpe0_blk_len >> 1) => {
            reg = ACPIREG_GPE0_EN;
        }
        _ => {}
    }

    // All special case return here
    if reg as usize >= ACPIREG_MAXREG {
        return;
    }
    let Some((iot, ioh, size)) = pmreg_access(sc, reg) else {
        return;
    };

    let o = offset as BusSize;
    match size {
        1 => bus_space_write_1(iot, ioh, o, regval as u8),
        2 => bus_space_write_2(iot, ioh, o, regval as u16),
        4 => bus_space_write_4(iot, ioh, o, regval as u32),
        _ => {}
    }
}

/// An `x_*_blk` register of the FADT: its address and access width (`1 << access_size`).
fn gas_reg(g: AcpiGas) -> (u64, i32) {
    ({ g.address }, 1 << g.access_size)
}

/// `acpi_map_pmregs(sc)`: maps the power-management registers the FADT names.
pub fn acpi_map_pmregs(sc: &AcpiSoftc) {
    let f = fadt(sc);
    let rev3 = f.hdr.revision >= 3;

    for reg in 0..ACPIREG_MAXREG as i32 {
        let mut size: i32 = 0;
        let mut access: i32 = 0;
        let mut addr: u64 = 0;
        let mut name = "";
        // The legacy block when there is one, else the extended one (revision 3 and later).
        let blk = |legacy: u32, x: AcpiGas, legacy_access: i32| -> (u64, i32) {
            if legacy != 0 {
                (u64::from(legacy), legacy_access)
            } else if rev3 {
                gas_reg(x)
            } else {
                (0, 0)
            }
        };
        match reg {
            ACPIREG_SMICMD => {
                name = "smi";
                size = 1;
                access = 1;
                addr = u64::from(f.smi_cmd);
            }
            ACPIREG_PM1A_STS | ACPIREG_PM1A_EN => {
                name = "pm1a_sts";
                size = i32::from(f.pm1_evt_len >> 1);
                (addr, access) = blk(f.pm1a_evt_blk, f.x_pm1a_evt_blk, 2);
                if reg == ACPIREG_PM1A_EN && addr != 0 {
                    addr += size as u64;
                    name = "pm1a_en";
                }
            }
            ACPIREG_PM1A_CNT => {
                name = "pm1a_cnt";
                size = i32::from(f.pm1_cnt_len);
                (addr, access) = blk(f.pm1a_cnt_blk, f.x_pm1a_cnt_blk, 2);
            }
            ACPIREG_PM1B_STS | ACPIREG_PM1B_EN => {
                name = "pm1b_sts";
                size = i32::from(f.pm1_evt_len >> 1);
                (addr, access) = blk(f.pm1b_evt_blk, f.x_pm1b_evt_blk, 2);
                if reg == ACPIREG_PM1B_EN && addr != 0 {
                    addr += size as u64;
                    name = "pm1b_en";
                }
            }
            ACPIREG_PM1B_CNT => {
                name = "pm1b_cnt";
                size = i32::from(f.pm1_cnt_len);
                (addr, access) = blk(f.pm1b_cnt_blk, f.x_pm1b_cnt_blk, 2);
            }
            ACPIREG_PM2_CNT => {
                name = "pm2_cnt";
                size = i32::from(f.pm2_cnt_len);
                (addr, access) = blk(f.pm2_cnt_blk, f.x_pm2_cnt_blk, size);
            }
            // #if 0: ACPIREG_PM_TMR is allocated in acpitimer.
            ACPIREG_GPE0_STS | ACPIREG_GPE0_EN => {
                name = "gpe0_sts";
                size = i32::from(f.gpe0_blk_len >> 1);
                (addr, access) = blk(f.gpe0_blk, f.x_gpe0_blk, 1);
                if reg == ACPIREG_GPE0_EN && addr != 0 {
                    addr += size as u64;
                    name = "gpe0_en";
                }
            }
            ACPIREG_GPE1_STS | ACPIREG_GPE1_EN => {
                name = "gpe1_sts";
                size = i32::from(f.gpe1_blk_len >> 1);
                (addr, access) = blk(f.gpe1_blk, f.x_gpe1_blk, 1);
                if reg == ACPIREG_GPE1_EN && addr != 0 {
                    addr += size as u64;
                    name = "gpe1_en";
                }
            }
            _ => {}
        }
        if size != 0 && addr != 0 {
            let r = &sc.sc_pmregs[reg as usize];
            // Size and address exist; map register space
            if let Some(iot) = sc.sc_iot.get() {
                // SAFETY: the FADT names these registers as the machine's.
                let ioh = unsafe { bus_space_map(iot, addr as BusAddr, size as BusSize, 0) };
                if let Ok(ioh) = ioh {
                    r.ioh.set(Some(ioh));
                }
            }

            r.name.set(Some(name));
            r.size.set(size);
            r.addr.set(addr as i32);
            r.access.set(access.min(4));
        }
    }
}

/// `acpi_unmap_pmregs(sc)`.
pub fn acpi_unmap_pmregs(sc: &AcpiSoftc) {
    for r in &sc.sc_pmregs {
        if r.size.get() != 0
            && r.addr.get() != 0
            && let (Some(iot), Some(ioh)) = (sc.sc_iot.get(), r.ioh.get())
        {
            crate::machine::bus::bus_space_unmap(iot, ioh, r.size.get() as BusSize);
        }
    }
}

/// `acpi_enable(sc)`: asks the firmware for ACPI mode through `SMI_CMD` and waits for
/// `SCI_EN`.
pub fn acpi_enable(sc: &AcpiSoftc) -> Result<(), Errno> {
    acpi_write_pmreg(sc, ACPIREG_SMICMD, 0, i32::from(fadt(sc).acpi_enable));
    let mut idx = 0;
    loop {
        if idx > ACPIEN_RETRIES {
            return Err(Errno::ETIMEDOUT);
        }
        idx += 1;
        if acpi_read_pmreg(sc, ACPIREG_PM1_CNT, 0) & i32::from(ACPI_PM1_SCI_EN) != 0 {
            return Ok(());
        }
    }
}

// ACPI Workqueue support

/// `acpi_addtask(sc, handler, arg0, arg1)`: queues `handler(arg0, arg1)` for the acpi
/// thread. The handler runs exactly once, so an `arg0` that carries ownership (an
/// `Rc::into_raw`) is taken back by the handler.
pub fn acpi_addtask(_sc: &AcpiSoftc, handler: fn(*mut c_void, i32), arg0: *mut c_void, arg1: i32) {
    let s = splbio();
    {
        let mut q = ACPI_TASKQ.get().borrow_mut();
        if q.try_reserve(1).is_err() {
            drop(q);
            splx(s);
            kprintf!("unable to create task");
            return;
        }
        q.push_back(AcpiTaskq {
            next: Default::default(),
            handler,
            arg0,
            arg1,
        });
    }
    splx(s);
}

/// `acpi_dotask(sc)`: runs the first queued task; nonzero if there was one.
pub fn acpi_dotask(_sc: &AcpiSoftc) -> i32 {
    let s = splbio();
    let wq = ACPI_TASKQ.get().borrow_mut().pop_front();
    splx(s);

    let Some(wq) = wq else {
        // we don't have anything to do
        return 0;
    };

    (wq.handler)(wq.arg0, wq.arg1);

    // We did something
    1
}

/// `is_ata(node)`: the node has the methods of an ATA channel or drive.
pub fn is_ata(node: &AmlNodeRef) -> bool {
    [b"_GTM", b"_GTF", b"_STM", b"_SDD"]
        .iter()
        .any(|n| aml_searchname(Some(node), *n).is_some())
}

/// `is_ejectable(node)`.
pub fn is_ejectable(node: &AmlNodeRef) -> bool {
    aml_searchname(Some(node), b"_EJ0").is_some()
}

/// `is_ejectable_bay(node)`: an ejectable ATA drive (an IDE bay).
pub fn is_ejectable_bay(node: &AmlNodeRef) -> bool {
    (is_ata(node) || node.parent().is_some_and(|p| is_ata(&p))) && is_ejectable(node)
}

/// `acpi_sleep_task(sc, sleepmode)`: the power button asked to suspend.
fn acpi_sleep_task(arg0: *mut c_void, sleepmode: i32) {
    // SAFETY: `acpi_pbtn_task` queues acpi0's softc, which is never freed.
    let sc = unsafe { &*arg0.cast_const().cast::<AcpiSoftc>() };

    // SUSPEND
    let _ = sleepmode;
    let _ = unported!("sleep_state (subr_suspend.c, acpi_x86.c: S3 suspend)");

    // Tell userland to recheck A/C and battery status
    acpi_record_event(sc, APM_POWER_CHANGE);
}

/// `acpi_reset()`: resets the machine through the FADT's reset register; returns when
/// there is none (`cpu_reset` then tries the keyboard controller).
pub fn acpi_reset() {
    if ACPI_ENABLED.load(Ordering::Relaxed) == 0 {
        return;
    }
    let Some(sc) = acpi_softc() else {
        return;
    };
    let f = fadt(sc);

    // RESET_REG_SUP is not properly set in some implementations, but not testing against
    // it breaks more machines than it fixes
    let reset_reg = { f.reset_reg };
    if f.hdr.revision <= 1 || { f.flags } & FADT_RESET_REG_SUP == 0 || { reset_reg.address } == 0 {
        return;
    }

    let mut value = u32::from(f.reset_value).to_le_bytes();

    let mut reset_as = i32::from(reset_reg.register_bit_width / 8);
    if reset_as == 0 {
        reset_as = 1;
    }

    let mut reset_len = i32::from(reset_reg.access_size);
    if reset_len == 0 {
        reset_len = reset_as;
    }

    acpi_gasio(
        Some(sc),
        ACPI_IOWRITE,
        i32::from(reset_reg.address_space_id),
        reset_reg.address,
        reset_as,
        reset_len,
        &mut value,
    );

    delay(100000);
}

/// The general-purpose event blocks of acpi0 (`sc->gpe_table`, `sc_lastgpe` of them).
fn gpe_table(sc: &AcpiSoftc) -> &'static [Cell<GpeBlock>] {
    let p = sc.gpe_table.get();
    if p.is_null() {
        return &[];
    }
    // SAFETY: `acpi_init_gpes` leaked `sc_lastgpe` blocks there and never changes either;
    // `Cell<GpeBlock>` has the layout of `GpeBlock`, and the blocks are touched under the
    // kernel lock at splbio, as in C.
    unsafe {
        slice::from_raw_parts(
            p.cast_const().cast::<Cell<GpeBlock>>(),
            sc.sc_lastgpe.get().max(0) as usize,
        )
    }
}

/// `acpi_gpe_task(arg0, gpe)`: runs the handler of an event the SCI saw.
fn acpi_gpe_task(_arg0: *mut c_void, gpe: i32) {
    let Some(sc) = acpi_softc() else {
        return;
    };
    let Some(pgpe) = gpe_table(sc).get(gpe.max(0) as usize) else {
        return;
    };

    let mut b = pgpe.get();
    if let Some(handler) = b.handler
        && b.active != 0
    {
        b.active = 0;
        pgpe.set(b);
        handler(sc, gpe, b.arg);
    }
}

/// `acpi_pbtn_task(sc, dummy)`: the power button was pressed.
fn acpi_pbtn_task(arg0: *mut c_void, _dummy: i32) {
    // SAFETY: `acpi_interrupt` queues acpi0's softc, which is never freed.
    let sc = unsafe { &*arg0.cast_const().cast::<AcpiSoftc>() };

    // Reset the latch and re-enable the GPE
    let s = splbio();
    let en = acpi_read_pmreg(sc, ACPIREG_PM1_EN, 0) as u16;
    acpi_write_pmreg(sc, ACPIREG_PM1_EN, 0, i32::from(en | ACPI_PM1_PWRBTN_EN));
    splx(s);

    // SUSPEND: ignore button events if we're resuming; resuming() (subr_suspend.c) is
    // false, as the machine never suspends.

    match pwr_action() {
        1 => powerbutton_event(),
        2 => acpi_addtask(sc, acpi_sleep_task, arg0, SLEEP_SUSPEND),
        _ => {}
    }
}

/// `acpi_sbtn_task(sc, dummy)`: the sleep button was pressed.
fn acpi_sbtn_task(arg0: *mut c_void, _dummy: i32) {
    // SAFETY: as in `acpi_pbtn_task`.
    let sc = unsafe { &*arg0.cast_const().cast::<AcpiSoftc>() };

    aml_notify_dev(Some(ACPI_DEV_SBD.as_bytes()), 0x80);

    // Reset the latch and re-enable the GPE
    let s = splbio();
    let en = acpi_read_pmreg(sc, ACPIREG_PM1_EN, 0) as u16;
    acpi_write_pmreg(sc, ACPIREG_PM1_EN, 0, i32::from(en | ACPI_PM1_SLPBTN_EN));
    splx(s);
}

/// Clears `cpu_suspended` and records the first wake source.
fn acpi_wake_source(sc: &AcpiSoftc, which: i32) {
    if cpu_suspended().load(Ordering::Relaxed) != 0 {
        cpu_suspended().store(0, Ordering::Relaxed);
    }
    if sc.sc_wakegpe.get() == WAKEGPE_NONE {
        sc.sc_wakegpe.set(which);
    }
}

/// `acpi_interrupt(sc)`: the SCI: masks and queues the general-purpose events that fired,
/// and handles the fixed power, sleep and RTC events.
pub fn acpi_interrupt(arg: *mut c_void) -> i32 {
    // SAFETY: `acpi_attach_machdep` established the SCI with acpi0's softc, never freed.
    let sc = unsafe { &*arg.cast_const().cast::<AcpiSoftc>() };
    let mut processed = 0;

    let table = gpe_table(sc);
    let mut idx = 0;
    while idx < sc.sc_lastgpe.get() {
        let sts = acpi_read_pmreg(sc, ACPIREG_GPE_STS, idx >> 3) as u16;
        let en = acpi_read_pmreg(sc, ACPIREG_GPE_EN, idx >> 3) as u16;
        if en & sts != 0 {
            // Mask the GPE until it is serviced
            acpi_write_pmreg(sc, ACPIREG_GPE_EN, idx >> 3, i32::from(en & !sts));
            for jdx in 0..8 {
                if en & sts & (1 << jdx) == 0 {
                    continue;
                }

                acpi_wake_source(sc, idx + jdx);

                // Signal this GPE
                let gpe = idx + jdx;
                let mut flags = 0;
                if let Some(b) = table.get(gpe as usize) {
                    let mut v = b.get();
                    v.active = 1;
                    flags = v.flags;
                    b.set(v);
                }
                acpi_addtask(sc, acpi_gpe_task, ptr::null_mut(), gpe);

                // Edge interrupts need their STS bits cleared now. Level interrupts will
                // have their STS bits cleared just before they are re-enabled.
                if flags & GPE_EDGE != 0 {
                    acpi_write_pmreg(sc, ACPIREG_GPE_STS, idx >> 3, 1 << jdx);
                }

                processed = 1;
            }
        }
        idx += 8;
    }

    let mut sts = acpi_read_pmreg(sc, ACPIREG_PM1_STS, 0) as u16;
    let mut en = acpi_read_pmreg(sc, ACPIREG_PM1_EN, 0) as u16;
    if sts & en != 0 {
        sts &= en;
        let arg0 = ptr::from_ref(sc).cast_mut().cast();
        if sts & ACPI_PM1_PWRBTN_STS != 0 {
            // Mask and acknowledge
            en &= !ACPI_PM1_PWRBTN_EN;
            acpi_write_pmreg(sc, ACPIREG_PM1_EN, 0, i32::from(en));
            acpi_write_pmreg(sc, ACPIREG_PM1_STS, 0, i32::from(ACPI_PM1_PWRBTN_STS));
            sts &= !ACPI_PM1_PWRBTN_STS;

            acpi_wake_source(sc, WAKEGPE_PWRBTN);

            acpi_addtask(sc, acpi_pbtn_task, arg0, 0);
        }
        if sts & ACPI_PM1_SLPBTN_STS != 0 {
            // Mask and acknowledge
            en &= !ACPI_PM1_SLPBTN_EN;
            acpi_write_pmreg(sc, ACPIREG_PM1_EN, 0, i32::from(en));
            acpi_write_pmreg(sc, ACPIREG_PM1_STS, 0, i32::from(ACPI_PM1_SLPBTN_STS));
            sts &= !ACPI_PM1_SLPBTN_STS;

            acpi_wake_source(sc, WAKEGPE_SLPBTN);

            acpi_addtask(sc, acpi_sbtn_task, arg0, 0);
        }
        if sts & ACPI_PM1_RTC_STS != 0 {
            // Mask and acknowledge
            en &= !ACPI_PM1_RTC_EN;
            acpi_write_pmreg(sc, ACPIREG_PM1_EN, 0, i32::from(en));
            acpi_write_pmreg(sc, ACPIREG_PM1_STS, 0, i32::from(ACPI_PM1_RTC_STS));
            sts &= !ACPI_PM1_RTC_STS;

            acpi_wake_source(sc, WAKEGPE_RTC);
        }

        if sts != 0 {
            kprintf!(
                "{}: PM1 stuck (en 0x{:x} st 0x{:x}), clearing\n",
                devname(sc),
                en,
                sts
            );
            acpi_write_pmreg(sc, ACPIREG_PM1_EN, 0, i32::from(en & !sts));
            acpi_write_pmreg(sc, ACPIREG_PM1_STS, 0, i32::from(sts));
        }
        processed = 1;
    }

    if processed != 0 {
        acpi_wakeup(sc);
    }

    processed
}

/// `nacpicpus`: the `acpicpu` children `acpi_add_device` made.
static NACPICPUS: AtomicI32 = AtomicI32::new(0);

/// `acpi_add_device(node, sc)`: attaches `acpicpu`, `acpitz` and `acpipwrres` for the
/// processors, thermal zones and power resources of the namespace.
fn acpi_add_device(node: &AmlNodeRef, sc: &AcpiSoftc) -> i32 {
    let mut aaa = acpi_attach_args(sc);
    aaa.aaa_node = Some(node.clone());
    let Some(value) = node.value() else {
        return 0;
    };

    match value.r#type() {
        AML_OBJTYPE_PROCESSOR => {
            if sc.sc_skip_processor.get() != 0 {
                return 0;
            }
            if NACPICPUS.load(Ordering::Relaxed) >= NCPUS.load(Ordering::Relaxed) {
                return 0;
            }
            let mut proc_id: i32 = -1;
            let res = AmlValue::new();
            if aml_evalnode(Some(sc), Some(node), &[], Some(&res)) == 0 {
                if let Some(p) = res.v_processor() {
                    proc_id = i32::from(p.proc_id);
                }
                aml_freevalue(Some(&res));
            }
            let mut found: Option<&'static CpuInfo> = None;
            cpu_info_foreach(&mut |ci| {
                if found.is_none() && ci_acpi_proc_id(ci) == proc_id as u32 {
                    found = Some(ci);
                }
            });
            if found.is_none() {
                return 0;
            }
            NACPICPUS.fetch_add(1, Ordering::Relaxed);

            aaa.aaa_name = c"acpicpu".as_ptr().cast();
        }
        AML_OBJTYPE_THERMZONE => {
            let sta = acpi_getsta(sc, Some(node));
            if sta & i64::from(STA_PRESENT) == 0 {
                return 0;
            }

            aaa.aaa_name = c"acpitz".as_ptr().cast();
        }
        AML_OBJTYPE_POWERRSRC => {
            aaa.aaa_name = c"acpipwrres".as_ptr().cast();
        }
        _ => return 0,
    }
    acpi_config_found(sc, &mut aaa, acpi_print);
    0
}

/// `acpi_enable_onegpe(sc, gpe)`.
pub fn acpi_enable_onegpe(sc: &AcpiSoftc, gpe: i32) {
    // Read enabled register
    let mask = 1u8 << (gpe & 7);
    let en = acpi_read_pmreg(sc, ACPIREG_GPE_EN, gpe >> 3) as u8;
    acpi_write_pmreg(sc, ACPIREG_GPE_EN, gpe >> 3, i32::from(en | mask));
}

/// `acpi_disable_allgpes(sc)`: clears all GPEs.
pub fn acpi_disable_allgpes(sc: &AcpiSoftc) {
    let mut idx = 0;
    while idx < sc.sc_lastgpe.get() {
        acpi_write_pmreg(sc, ACPIREG_GPE_EN, idx >> 3, 0);
        acpi_write_pmreg(sc, ACPIREG_GPE_STS, idx >> 3, -1);
        idx += 8;
    }
}

/// `acpi_enable_rungpes(sc)`: enables the runtime GPEs.
pub fn acpi_enable_rungpes(sc: &AcpiSoftc) {
    for (idx, b) in gpe_table(sc).iter().enumerate() {
        if b.get().handler.is_some() {
            acpi_enable_onegpe(sc, idx as i32);
        }
    }
}

/// `acpi_enable_wakegpes(sc, state)`: enables the wakeup GPEs that may wake the machine
/// from `state`.
pub fn acpi_enable_wakegpes(sc: &AcpiSoftc, state: i32) {
    for wentry in sc.sc_wakedevs.iter() {
        if wentry.q_enabled.get() != 0 && state <= wentry.q_state.get() {
            acpi_enable_onegpe(sc, wentry.q_gpe.get());
        }
    }
}

/// `acpi_set_gpehandler(sc, gpe, handler, arg, flags)`: 0, or `-EINVAL`.
pub fn acpi_set_gpehandler(
    sc: &AcpiSoftc,
    gpe: i32,
    handler: Option<fn(&AcpiSoftc, i32, *mut c_void) -> i32>,
    arg: *mut c_void,
    flags: i32,
) -> i32 {
    let Some(ptbl) = acpi_find_gpe(sc, gpe) else {
        return -(Errno::EINVAL as i32);
    };
    if handler.is_none() {
        return -(Errno::EINVAL as i32);
    }
    if flags & GPE_LEVEL != 0 && flags & GPE_EDGE != 0 {
        return -(Errno::EINVAL as i32);
    }
    if flags & (GPE_LEVEL | GPE_EDGE) == 0 {
        return -(Errno::EINVAL as i32);
    }
    let mut b = ptbl.get();
    if b.handler.is_some() {
        kprintf!("{}: GPE 0x{:02x} already enabled\n", devname(sc), gpe);
    }

    b.handler = handler;
    b.arg = arg;
    b.flags = flags;
    ptbl.set(b);

    0
}

/// `acpi_gpe(sc, gpe, node)`: the handler of a `\_GPE._Lxx`/`_Exx` method: runs it, then
/// acknowledges a level-triggered event and unmasks it.
fn acpi_gpe(sc: &AcpiSoftc, gpe: i32, arg: *mut c_void) -> i32 {
    // SAFETY: `acpi_init_gpes` registered an `Rc::into_raw` of the method's node, kept for
    // good; this borrows it without taking that reference over.
    let node = ManuallyDrop::new(unsafe { Rc::from_raw(arg.cast_const().cast()) });
    let node: &AmlNodeRef = &node;

    aml_evalnode(Some(sc), Some(node), &[], None);

    let mask = 1u8 << (gpe & 7);
    let flags = gpe_table(sc)
        .get(gpe.max(0) as usize)
        .map_or(0, |b| b.get().flags);
    if flags & GPE_LEVEL != 0 {
        acpi_write_pmreg(sc, ACPIREG_GPE_STS, gpe >> 3, i32::from(mask));
    }
    let en = acpi_read_pmreg(sc, ACPIREG_GPE_EN, gpe >> 3) as u8;
    acpi_write_pmreg(sc, ACPIREG_GPE_EN, gpe >> 3, i32::from(en | mask));
    0
}

/// `acpi_foundprw(node, sc)`: records a device that can wake the system. `_PRW` returns a
/// package: `pkg[0]` the GPE bit (an integer, or a GPE block and bit), `pkg[1]` the lowest
/// sleep state, `pkg[2+]` its power resources. To enable it, `_ON` of each power resource
/// and `_PSW` are evaluated.
fn acpi_foundprw(node: &AmlNodeRef, sc: &AcpiSoftc) -> i32 {
    let parent = node.parent();
    let sta = acpi_getsta(sc, parent.as_ref());
    if sta & i64::from(STA_PRESENT) == 0 {
        return 0;
    }

    let pkg: AmlValueRef = Rc::new(AmlValue::new());
    aml_evalnode(Some(sc), Some(node), &[], Some(&pkg));
    let wq: &'static AcpiWakeq = Box::leak(Box::new(AcpiWakeq {
        q_next: Default::default(),
        q_node: RefCell::new(parent),
        q_wakepkg: RefCell::new(Some(pkg.clone())),
        q_gpe: Cell::new(-1),
        q_state: Cell::new(0),
        q_enabled: Cell::new(0),
    }));

    // Get GPE of wakeup device, and lowest sleep level
    if pkg.r#type() == AML_OBJTYPE_PACKAGE && pkg.length() >= 2 {
        if let Some(v) = pkg.v_package(0)
            && v.r#type() == AML_OBJTYPE_INTEGER
        {
            wq.q_gpe.set(v.v_integer() as i32);
        }
        if let Some(v) = pkg.v_package(1)
            && v.r#type() == AML_OBJTYPE_INTEGER
        {
            wq.q_state.set(v.v_integer() as i32);
        }
        wq.q_enabled.set(0);
    }
    // SAFETY: a new entry, in no list, leaked: it stays in place for good.
    unsafe { sc.sc_wakedevs.insert_tail(wq) };
    0
}

/// `acpi_toggle_wakedev(sc, node, enable)`: lets device `node` wake the machine, or not; 0,
/// or -1 when it cannot wake it.
pub fn acpi_toggle_wakedev(sc: &AcpiSoftc, node: &AmlNodeRef, enable: i32) -> i32 {
    for wentry in sc.sc_wakedevs.iter() {
        if wentry
            .q_node
            .borrow()
            .as_ref()
            .is_some_and(|n| Rc::ptr_eq(n, node))
        {
            wentry.q_enabled.set(i32::from(enable != 0));
            return 0;
        }
    }

    -1
}

/// `acpi_find_gpe(sc, gpe)`: the block of event `gpe`, if acpi0 has one.
pub fn acpi_find_gpe(sc: &AcpiSoftc, gpe: i32) -> Option<&'static Cell<GpeBlock>> {
    if gpe >= sc.sc_lastgpe.get() {
        return None;
    }
    gpe_table(sc).get(usize::try_from(gpe).ok()?)
}

/// `acpi_init_gpes(sc)`: allocates the GPE table, clears the events, gives each
/// `\_GPE._Lxx` (level) or `_Exx` (edge) method its event, and records the wakeup devices.
pub fn acpi_init_gpes(sc: &AcpiSoftc) {
    sc.sc_lastgpe.set(i32::from(fadt(sc).gpe0_blk_len) << 2);

    // Allocate GPE table
    let blocks: Vec<GpeBlock> = (0..sc.sc_lastgpe.get())
        .map(|_| GpeBlock {
            handler: None,
            arg: ptr::null_mut(),
            active: 0,
            flags: 0,
        })
        .collect();
    sc.gpe_table.set(blocks.leak().as_mut_ptr());

    let root = sc.sc_root.borrow().clone();

    // Clear GPE status
    acpi_disable_allgpes(sc);
    for idx in 0..sc.sc_lastgpe.get() {
        // Search Level-sensitive GPES
        let name = format!("\\_GPE._L{idx:02X}");
        let mut gpe = aml_searchname(root.as_ref(), name.as_bytes());
        if let Some(g) = &gpe {
            let arg = Rc::into_raw(g.clone()).cast_mut().cast();
            acpi_set_gpehandler(sc, idx, Some(acpi_gpe), arg, GPE_LEVEL);
        }
        if gpe.is_none() {
            // Search Edge-sensitive GPES
            let name = format!("\\_GPE._E{idx:02X}");
            gpe = aml_searchname(root.as_ref(), name.as_bytes());
            if let Some(g) = &gpe {
                let arg = Rc::into_raw(g.clone()).cast_mut().cast();
                acpi_set_gpehandler(sc, idx, Some(acpi_gpe), arg, GPE_EDGE);
            }
        }
    }
    if let Some(root) = root {
        aml_find_node(&root, b"_PRW", &mut |n| acpi_foundprw(n, sc));
    }
}

/// `acpi_init_pm(sc)`: finds the sleep and resume methods.
pub fn acpi_init_pm(sc: &AcpiSoftc) {
    let root = sc.sc_root.borrow().clone();
    let find = |name: &[u8]| aml_searchname(root.as_ref(), name);
    *sc.sc_tts.borrow_mut() = find(b"_TTS");
    *sc.sc_pts.borrow_mut() = find(b"_PTS");
    *sc.sc_wak.borrow_mut() = find(b"_WAK");
    *sc.sc_bfs.borrow_mut() = find(b"_BFS");
    *sc.sc_gts.borrow_mut() = find(b"_GTS");
    *sc.sc_sst.borrow_mut() = find(b"_SI_._SST");
}

/// `acpi_init_states(sc)`: reads the `SLP_TYP` values of each sleep state the firmware
/// has (`\_S0_` .. `\_S5_`) and prints them.
pub fn acpi_init_states(sc: &AcpiSoftc) {
    let root = sc.sc_root.borrow().clone();

    kprintf!("\n{}: sleep states", devname(sc));
    for i in ACPI_STATE_S0..=ACPI_STATE_S5 {
        let st = &sc.sc_sleeptype[usize::from(i)];
        let name = format!("_S{i}_");
        st.slp_typa.set(-1);
        st.slp_typb.set(-1);
        let res = AmlValue::new();
        if aml_evalname(Some(sc), root.as_ref(), name.as_bytes(), &[], Some(&res)) != 0 {
            continue;
        }
        if res.r#type() != AML_OBJTYPE_PACKAGE || res.length() < 2 {
            aml_freevalue(Some(&res));
            continue;
        }
        st.slp_typa
            .set(aml_val2int(res.v_package(0).as_deref()) as i32);
        st.slp_typb
            .set(aml_val2int(res.v_package(1).as_deref()) as i32);
        aml_freevalue(Some(&res));

        kprintf!(" S{}", i);
        if i == 0 && { fadt(sc).flags } & FADT_POWER_S0_IDLE_CAPABLE != 0 {
            kprintf!("ix");
        }
    }
}

/// The PM1 control value of `reg` with sleep type `typ` (`SLP_EN` clear).
fn pm1_slp_typ(sc: &AcpiSoftc, reg: i32, typ: i32) -> u16 {
    let mut v = acpi_read_pmreg(sc, reg, 0) as u16;
    v &= !(ACPI_PM1_SLP_TYPX_MASK | ACPI_PM1_SLP_EN);
    v | acpi_pm1_slp_typx(typ as u16)
}

/// `acpi_sleep_pm(sc, state)`: writes the sleep type of `state` and `SLP_EN` until the
/// machine wakes (`WAK_STS`); for S5 it does not come back.
pub fn acpi_sleep_pm(sc: &AcpiSoftc, state: i32) {
    let mut retry = 0;
    let st = &sc.sc_sleeptype[state as usize];

    let _ = intr_disable();

    // Clear WAK_STS bit
    acpi_write_pmreg(sc, ACPIREG_PM1_STS, 0, i32::from(ACPI_PM1_WAK_STS));

    // Disable BM arbitration at deep sleep and beyond
    let f = fadt(sc);
    if state >= i32::from(ACPI_STATE_S3) && { f.pm2_cnt_blk } != 0 && f.pm2_cnt_len != 0 {
        acpi_write_pmreg(sc, ACPIREG_PM2_CNT, 0, i32::from(ACPI_PM2_ARB_DIS));
    }

    // Write SLP_TYPx values
    let mut rega = pm1_slp_typ(sc, ACPIREG_PM1A_CNT, st.slp_typa.get());
    let mut regb = pm1_slp_typ(sc, ACPIREG_PM1B_CNT, st.slp_typb.get());
    acpi_write_pmreg(sc, ACPIREG_PM1A_CNT, 0, i32::from(rega));
    acpi_write_pmreg(sc, ACPIREG_PM1B_CNT, 0, i32::from(regb));

    // Loop on WAK_STS, setting the SLP_EN bits once in a while
    rega |= ACPI_PM1_SLP_EN;
    regb |= ACPI_PM1_SLP_EN;
    loop {
        if retry == 0 {
            acpi_write_pmreg(sc, ACPIREG_PM1A_CNT, 0, i32::from(rega));
            acpi_write_pmreg(sc, ACPIREG_PM1B_CNT, 0, i32::from(regb));
        }
        retry = (retry + 1) % 100000;

        let regra = acpi_read_pmreg(sc, ACPIREG_PM1A_STS, 0) as u16;
        let regrb = acpi_read_pmreg(sc, ACPIREG_PM1B_STS, 0) as u16;
        if regra & ACPI_PM1_WAK_STS != 0 || regrb & ACPI_PM1_WAK_STS != 0 {
            break;
        }
    }
}

/// `acpi_resume_pm(sc, fromstate)`: puts PM1 back in S0 after a wake and runs `_BFS` and
/// `_WAK`.
pub fn acpi_resume_pm(sc: &AcpiSoftc, fromstate: i32) {
    let s0 = &sc.sc_sleeptype[usize::from(ACPI_STATE_S0)];

    // Write SLP_TYPx values
    let rega = pm1_slp_typ(sc, ACPIREG_PM1A_CNT, s0.slp_typa.get());
    let regb = pm1_slp_typ(sc, ACPIREG_PM1B_CNT, s0.slp_typb.get());
    acpi_write_pmreg(sc, ACPIREG_PM1A_CNT, 0, i32::from(rega));
    acpi_write_pmreg(sc, ACPIREG_PM1B_CNT, 0, i32::from(regb));

    // Force SCI_EN on resume to fix horribly broken machines
    acpi_write_pmreg(
        sc,
        ACPIREG_PM1_CNT,
        0,
        (u32::from(ACPI_PM1_SCI_EN) | ACPI_FORCE_BM.load(Ordering::Relaxed)) as i32,
    );

    // Clear fixed event status
    acpi_write_pmreg(sc, ACPIREG_PM1_STS, 0, i32::from(ACPI_PM1_ALL_STS));

    // acpica-reference.pdf page 148 says do not call _BFS
    // 1st resume AML step: _BFS(fromstate)
    aml_node_setval(
        Some(sc),
        sc.sc_bfs.borrow().clone().as_ref(),
        i64::from(fromstate),
    );

    // Enable runtime GPEs
    acpi_disable_allgpes(sc);
    acpi_enable_rungpes(sc);

    // 2nd resume AML step: _WAK(fromstate)
    aml_node_setval(
        Some(sc),
        sc.sc_wak.borrow().clone().as_ref(),
        i64::from(fromstate),
    );

    // Clear WAK_STS bit
    acpi_write_pmreg(sc, ACPIREG_PM1_STS, 0, i32::from(ACPI_PM1_WAK_STS));

    let f = fadt(sc);
    let mut en = acpi_read_pmreg(sc, ACPIREG_PM1_EN, 0) as u16;
    if { f.flags } & FADT_PWR_BUTTON == 0 {
        en |= ACPI_PM1_PWRBTN_EN;
    }
    if { f.flags } & FADT_SLP_BUTTON == 0 {
        en |= ACPI_PM1_SLPBTN_EN;
    }
    acpi_write_pmreg(sc, ACPIREG_PM1_EN, 0, i32::from(en));

    // If PM2 exists, re-enable BM arbitration (reportedly some BIOS forget to)
    if { f.pm2_cnt_blk } != 0 && f.pm2_cnt_len != 0 {
        let mut rega = acpi_read_pmreg(sc, ACPIREG_PM2_CNT, 0) as u16;
        rega &= !ACPI_PM2_ARB_DIS;
        acpi_write_pmreg(sc, ACPIREG_PM2_CNT, 0, i32::from(rega));
    }
}

/// `save_led_state`: the last state `acpi_indicator` set.
static SAVE_LED_STATE: AtomicI32 = AtomicI32::new(-1);

/// `acpi_indicator(sc, led_state)`: sets the indicator light to some state (`_SST`).
pub fn acpi_indicator(sc: &AcpiSoftc, led_state: i32) {
    if SAVE_LED_STATE.load(Ordering::Relaxed) != led_state {
        aml_node_setval(
            Some(sc),
            sc.sc_sst.borrow().clone().as_ref(),
            i64::from(led_state),
        );
        SAVE_LED_STATE.store(led_state, Ordering::Relaxed);
    }
}

/// `acpi_powerdown()`: enters S5 (`boot(RB_POWERDOWN)`); returns only when acpi0 did not
/// take the machine over.
///
/// XXX We are going to do AML execution but are not in the acpi thread. We do not know if
/// the acpi thread is sleeping on acpiec in some intermediate context. Wish us luck.
pub fn acpi_powerdown() {
    let state = i32::from(ACPI_STATE_S5);

    if ACPI_ENABLED.load(Ordering::Relaxed) == 0 {
        return;
    }
    let Some(sc) = acpi_softc() else {
        return;
    };

    let _s = splhigh();
    let _ = intr_disable();
    COLD.store(true, Ordering::Relaxed);

    // 1st powerdown AML step: _PTS(tostate)
    aml_node_setval(
        Some(sc),
        sc.sc_pts.borrow().clone().as_ref(),
        i64::from(state),
    );

    acpi_disable_allgpes(sc);
    acpi_enable_wakegpes(sc, state);

    // 2nd powerdown AML step: _GTS(tostate)
    aml_node_setval(
        Some(sc),
        sc.sc_gts.borrow().clone().as_ref(),
        i64::from(state),
    );

    acpi_sleep_pm(sc, state);
    panic(format_args!("acpi S5 transition did not happen"));
}

/// `acpi_map_address(sc, gas, base, size, &ioh, &iot)`: maps `size` bytes at `base` (plus
/// the generic address `gas`, port space without one); the C's -1 is an `Err`.
pub fn acpi_map_address(
    sc: &AcpiSoftc,
    gas: Option<&AcpiGas>,
    base: BusAddr,
    size: BusSize,
) -> Result<(BusSpaceHandle, BusSpaceTag), Errno> {
    let mut base = base;
    let mut iospace = GAS_SYSTEM_IOSPACE;

    // No GAS structure, default to I/O space
    if let Some(gas) = gas {
        base = base.wrapping_add({ gas.address } as BusAddr);
        iospace = i32::from(gas.address_space_id);
    }
    let iot = match iospace {
        GAS_SYSTEM_MEMORY => sc.sc_memt.get(),
        GAS_SYSTEM_IOSPACE => sc.sc_iot.get(),
        _ => None,
    }
    .ok_or(Errno::EINVAL)?;
    // SAFETY: the firmware's tables name the range.
    let ioh = unsafe { bus_space_map(iot, base, size, 0) }?;

    Ok((ioh, iot))
}

/// `acpi_wakeup(sc)`: wakes the acpi thread.
pub fn acpi_wakeup(sc: &AcpiSoftc) {
    sc.sc_threadwaiting.set(0);
    wakeup(ptr::from_ref(sc));
}

/// `acpi_thread(thread)`: the acpi thread: enables the fixed button events and the runtime
/// GPEs, then runs the task queue whenever it is woken.
fn acpi_thread(arg: *mut c_void) {
    let thread = arg.cast::<AcpiThread>();
    // SAFETY: `acpi_attach_common` made the thread structure with `Box::into_raw`; only
    // this thread frees it, when it ends.
    let th = unsafe { &*thread };
    let Some(sc) = th.sc.get() else {
        kthread_exit(0);
    };

    // AML/SMI cannot be trusted -- only run on the BSP
    let mut primary: Option<&'static CpuInfo> = None;
    cpu_info_foreach(&mut |ci| {
        if primary.is_none() {
            primary = Some(ci);
        }
    });
    if let Some(ci) = primary {
        sched_peg_curproc(ci);
    }

    rw_enter_write(&sc.sc_lck);

    // If we have an interrupt handler, we can get notification when certain status bits
    // changes in the ACPI registers, so let us enable some events we can forward to
    // userland
    if !sc.sc_interrupt.get().is_null() {
        sc.sc_threadwaiting.set(1);

        // Enable Sleep/Power buttons if they exist
        let s = splbio();
        let f = fadt(sc);
        let mut en = acpi_read_pmreg(sc, ACPIREG_PM1_EN, 0) as u16;
        if { f.flags } & FADT_PWR_BUTTON == 0 {
            en |= ACPI_PM1_PWRBTN_EN;
        }
        if { f.flags } & FADT_SLP_BUTTON == 0 {
            en |= ACPI_PM1_SLPBTN_EN;
        }
        acpi_write_pmreg(sc, ACPIREG_PM1_EN, 0, i32::from(en));

        // Enable handled GPEs here
        acpi_enable_rungpes(sc);
        splx(s);
    }

    while th.running.load(Ordering::Relaxed) != 0 {
        let s = splbio();
        while sc.sc_threadwaiting.get() != 0 {
            rw_exit_write(&sc.sc_lck);
            let _ = tsleep_nsec(ptr::from_ref(sc), PWAIT, "acpi0", INFSLP);
            rw_enter_write(&sc.sc_lck);
        }
        sc.sc_threadwaiting.set(1);
        splx(s);
        if AML_BUSY.load(Ordering::Relaxed) != 0 {
            panic(format_args!("thread woke up to find aml was busy"));
        }

        // Run ACPI taskqueue
        while acpi_dotask(sc) != 0 {}
    }
    // SAFETY: see above; nothing else holds the structure any more.
    drop(unsafe { Box::from_raw(thread) });

    kthread_exit(0);
}

/// `acpi_create_thread(sc)`: starts the acpi thread, once kernel threads can be made.
fn acpi_create_thread(arg: *mut c_void) {
    // SAFETY: `acpi_attach_common` deferred this with acpi0's softc, never freed.
    let sc = unsafe { &*arg.cast_const().cast::<AcpiSoftc>() };

    if kthread_create(
        acpi_thread,
        sc.sc_thread.get().cast(),
        devname(sc).as_bytes(),
    )
    .is_err()
    {
        kprintf!(
            "{}: unable to create isr thread, GPEs disabled\n",
            devname(sc)
        );
    }
}

/// `acpi_foundsectwo(node, sc)` (`__arm64__`): attaches `acpisectwo`, the Surface Pro X's
/// embedded controller.
fn acpi_foundsectwo(node: &AmlNodeRef, sc: &AcpiSoftc) -> i32 {
    let mut aaa = acpi_attach_args(sc);
    aaa.aaa_node = node.parent();
    aaa.aaa_name = c"acpisectwo".as_ptr().cast();

    acpi_config_found(sc, &mut aaa, acpi_print);

    0
}

/// The device id a `_HID`/`_CID` value names: its string, or the EISA id of its integer.
fn hid_string(v: &AmlValue) -> Vec<u8> {
    match v.r#type() {
        AML_OBJTYPE_STRING => v.v_string(),
        AML_OBJTYPE_INTEGER => aml_eisaid(aml_val2int(Some(v)) as u32).to_vec(),
        _ => b"unknown".to_vec(),
    }
}

/// `acpi_foundec(node, sc)`: attaches `acpiec` for the embedded controller (`PNP0C09`).
fn acpi_foundec(node: &AmlNodeRef, sc: &AcpiSoftc) -> i32 {
    let res = AmlValue::new();
    if aml_evalnode(Some(sc), Some(node), &[], Some(&res)) != 0 {
        return 0;
    }

    let mut dev = hid_string(&res);
    if dev != ACPI_DEV_ECD.as_bytes() {
        return 0;
    }

    // Check if we're already attached: sc->sc_ec->sc_devnode == node->parent (sc_ec stays
    // NULL until acpiec.c is ported).

    let mut aaa = acpi_attach_args(sc);
    aaa.aaa_node = node.parent();
    dev.push(0);
    aaa.aaa_dev = dev.as_ptr();
    aaa.aaa_name = c"acpiec".as_ptr().cast();
    acpi_config_found(sc, &mut aaa, acpi_print);
    aml_freevalue(Some(&res));

    0
}

/// `acpi_foundsony(node, sc)`: attaches `acpisony`.
fn acpi_foundsony(node: &AmlNodeRef, sc: &AcpiSoftc) -> i32 {
    let mut aaa = acpi_attach_args(sc);
    aaa.aaa_node = node.parent();
    aaa.aaa_name = c"acpisony".as_ptr().cast();

    acpi_config_found(sc, &mut aaa, acpi_print);

    0
}

// Support for _DSD Device Properties.

/// `acpi_getprop(node, prop, buf, buflen)`: copies the buffer or string property `prop`
/// of the device's `_DSD` into `buf`; its length, or -1.
pub fn acpi_getprop(node: &AmlNodeRef, prop: &[u8], buf: &mut [u8]) -> i32 {
    let dsd = AmlValue::new();
    if aml_evalname(acpi_softc(), Some(node), b"_DSD", &[], Some(&dsd)) != 0 {
        return -1;
    }
    let Some(props) = dsd_properties(&dsd, &DSD_PROP_GUID) else {
        return -1;
    };

    // Check properties.
    if let Some(val) = dsd_property(&props, prop) {
        let len = val.length();
        match val.r#type() {
            AML_OBJTYPE_BUFFER | AML_OBJTYPE_STRING => {
                let b = val.v_buffer();
                let n = (len.max(0) as usize).min(buf.len()).min(b.len());
                buf[..n].copy_from_slice(&b[..n]);
                return len;
            }
            _ => {}
        }
    }

    -1
}

/// `acpi_getpropint(node, prop, defval)`: the integer property `prop` of the device's
/// `_DSD`, or `defval`.
pub fn acpi_getpropint(node: &AmlNodeRef, prop: &[u8], defval: u64) -> u64 {
    let dsd = AmlValue::new();
    if aml_evalname(acpi_softc(), Some(node), b"_DSD", &[], Some(&dsd)) != 0 {
        return defval;
    }
    let Some(props) = dsd_properties(&dsd, &DSD_PROP_GUID) else {
        return defval;
    };

    match dsd_property(&props, prop) {
        Some(val) if val.r#type() == AML_OBJTYPE_INTEGER => val.v_integer() as u64,
        _ => defval,
    }
}

/// `acpi_parsehid(node, sc, outcdev, outdev, devlen)`: the device ids of the device whose
/// `_HID` is `node`: its `_CID` (the first, for a package) into `outcdev`, its `_HID` into
/// `outdev`, NUL-terminated; nonzero when `_HID` cannot be evaluated.
pub fn acpi_parsehid(
    node: &AmlNodeRef,
    sc: &AcpiSoftc,
    outcdev: &mut [u8],
    outdev: &mut [u8],
) -> i32 {
    let res = AmlValue::new();
    let parent = node.parent();
    if aml_evalname(acpi_softc(), parent.as_ref(), b"_CID", &[], Some(&res)) == 0 {
        let cid = if res.r#type() == AML_OBJTYPE_PACKAGE && res.length() >= 1 {
            res.v_package(0)
        } else {
            None
        };
        let dev = match &cid {
            Some(c) => hid_string(c),
            None => hid_string(&res),
        };
        strlcpy(outcdev, &dev);
        aml_freevalue(Some(&res));
    } else if let Some(z) = outcdev.first_mut() {
        *z = 0;
    }

    if aml_evalnode(Some(sc), Some(node), &[], Some(&res)) != 0 {
        return 1;
    }

    let dev = hid_string(&res);
    strlcpy(outdev, &dev);

    aml_freevalue(Some(&res));

    0
}

/// `acpi_attach_deps(sc, node)`: attaches first the devices the device depends on
/// (`_DEP`).
pub fn acpi_attach_deps(sc: &AcpiSoftc, node: &AmlNodeRef) {
    let res = AmlValue::new();
    if aml_evalname(Some(sc), Some(node), b"_DEP", &[], Some(&res)) != 0 {
        return;
    }

    if res.r#type() != AML_OBJTYPE_PACKAGE {
        return;
    }

    let mut node = Some(node.clone());
    for i in 0..res.length().max(0) as usize {
        let Some(mut val) = res.v_package(i) else {
            continue;
        };
        if val.r#type() == AML_OBJTYPE_NAMEREF {
            let name = val
                .v_nameref()
                .map(|p| aml_getname(p.tail()))
                .unwrap_or_default();
            node = aml_searchrel(node.as_ref(), &name);
            if let Some(v) = node.as_ref().and_then(|n| n.value()) {
                val = v;
            }
        }
        let val = deref_objref(val);
        if val.r#type() != AML_OBJTYPE_DEVICE {
            continue;
        }
        let Some(dep) = val.node() else {
            continue;
        };
        if dep.attached.get() != 0 {
            continue;
        }
        if let Some(hid) = aml_searchname(Some(&dep), b"_HID") {
            acpi_foundhid(&hid, sc);
        }
    }

    aml_freevalue(Some(&res));
}

/// `acpi_parse_resources(crsidx, crs, aaa)`: records the address ranges and interrupts of
/// a device's `_CRS` in its attach arguments.
pub fn acpi_parse_resources(_crsidx: i32, crs: &AcpiResource<'_>, aaa: &mut AcpiAttachArgs) -> i32 {
    let typ = aml_crstype(crs);
    let na = aaa.aaa_naddr as usize;
    let ni = aaa.aaa_nirq as usize;

    match typ {
        SR_IOPORT | SR_FIXEDPORT | LR_MEM24 | LR_MEM32 | LR_MEM32FIXED | LR_WORD | LR_DWORD
        | LR_QWORD
            if na >= aaa.aaa_addr.len() =>
        {
            return 0;
        }
        SR_IRQ | LR_EXTIRQ if ni >= aaa.aaa_irq.len() => return 0,
        _ => {}
    }

    match typ {
        SR_IOPORT | SR_FIXEDPORT => aaa.aaa_bst[na] = aaa.aaa_iot,
        LR_MEM24 | LR_MEM32 | LR_MEM32FIXED => aaa.aaa_bst[na] = aaa.aaa_memt,
        LR_WORD | LR_DWORD | LR_QWORD => match crs.lr_word_type() {
            LR_TYPE_MEMORY => aaa.aaa_bst[na] = aaa.aaa_memt,
            LR_TYPE_IO => aaa.aaa_bst[na] = aaa.aaa_iot,
            // Bus number range or something else; skip.
            _ => return 0,
        },
        _ => {}
    }

    let range = match typ {
        SR_IOPORT => Some((
            u64::from(crs.sr_ioport__min()),
            u64::from(crs.sr_ioport__len()),
        )),
        SR_FIXEDPORT => Some((
            u64::from(crs.sr_fioport__bas()),
            u64::from(crs.sr_fioport__len()),
        )),
        LR_MEM24 => Some((u64::from(crs.lr_m24__min()), u64::from(crs.lr_m24__len()))),
        LR_MEM32 => Some((u64::from(crs.lr_m32__min()), u64::from(crs.lr_m32__len()))),
        LR_MEM32FIXED => Some((
            u64::from(crs.lr_m32fixed__bas()),
            u64::from(crs.lr_m32fixed__len()),
        )),
        LR_WORD => Some((u64::from(crs.lr_word__min()), u64::from(crs.lr_word__len()))),
        LR_DWORD => Some((
            u64::from(crs.lr_dword__min()),
            u64::from(crs.lr_dword__len()),
        )),
        LR_QWORD => Some((crs.lr_qword__min(), crs.lr_qword__len())),
        _ => None,
    };
    if let Some((a, s)) = range {
        aaa.aaa_addr[na] = a;
        aaa.aaa_size[na] = s;
        aaa.aaa_naddr += 1;
    }

    match typ {
        SR_IRQ => {
            let mask = crs.sr_irq_irq_mask();
            aaa.aaa_irq[ni] = if mask == 0 {
                u32::MAX
            } else {
                mask.trailing_zeros()
            };
            // Default is exclusive, active-high, edge triggered.
            let flags = if aml_crslen(crs) < 4 {
                SR_IRQ_MODE
            } else {
                crs.sr_irq_irq_flags()
            };
            // Map flags to those of the extended interrupt descriptor.
            if flags & SR_IRQ_SHR != 0 {
                aaa.aaa_irq_flags[ni] |= u32::from(LR_EXTIRQ_SHR);
            }
            if flags & SR_IRQ_POLARITY != 0 {
                aaa.aaa_irq_flags[ni] |= u32::from(LR_EXTIRQ_POLARITY);
            }
            if flags & SR_IRQ_MODE != 0 {
                aaa.aaa_irq_flags[ni] |= u32::from(LR_EXTIRQ_MODE);
            }
            aaa.aaa_nirq += 1;
        }
        LR_EXTIRQ => {
            aaa.aaa_irq[ni] = crs.lr_extirq_irq(0);
            aaa.aaa_irq_flags[ni] = u32::from(crs.lr_extirq_flags());
            aaa.aaa_nirq += 1;
        }
        _ => {}
    }

    0
}

/// `acpi_parse_crs(sc, aaa)`: the device's `_CRS` into its attach arguments.
pub fn acpi_parse_crs(sc: &AcpiSoftc, aaa: &mut AcpiAttachArgs) {
    let res = AmlValue::new();
    if aml_evalname(Some(sc), aaa.aaa_node.as_ref(), b"_CRS", &[], Some(&res)) != 0 {
        return;
    }

    aml_parse_resource(&res, &mut |i, crs| acpi_parse_resources(i, crs, aaa));
}

/// `acpi_foundhid(node, sc)`: attaches a driver for the device whose `_HID` is `node`
/// (unless it is skipped or left to ISA), with the resources of its `_CRS`.
fn acpi_foundhid(node: &AmlNodeRef, sc: &AcpiSoftc) -> i32 {
    let mut cdev = [0u8; 32];
    let mut dev = [0u8; 32];

    if acpi_parsehid(node, sc, &mut cdev, &mut dev) != 0 {
        return 0;
    }
    let Some(parent) = node.parent() else {
        return 0;
    };

    let sta = acpi_getsta(sc, Some(&parent));
    if sta & i64::from(STA_PRESENT) == 0 && sta & i64::from(STA_DEV_OK) == 0 {
        return 1;
    }
    if sta & i64::from(STA_ENABLED) == 0 {
        return 0;
    }

    let mut cca = 0;
    if aml_evalinteger(Some(sc), Some(&parent), b"_CCA", &[], &mut cca) != 0 {
        cca = 1;
    }

    acpi_attach_deps(sc, &parent);

    let mut aaa = acpi_attach_args(sc);
    aaa.aaa_dmat = if cca != 0 {
        sc.sc_cc_dmat.get()
    } else {
        sc.sc_ci_dmat.get()
    };
    aaa.aaa_node = Some(parent.clone());
    aaa.aaa_dev = dev.as_ptr();
    aaa.aaa_cdev = cdev.as_ptr();

    if cstr(&cdev) == ACPI_DEV_MOUSE.as_bytes()
        && SBTN_PNP.iter().any(|p| p.as_bytes() == cstr(&dev))
    {
        MOUSE_HAS_SOFTBTN.store(1, Ordering::Relaxed);
    }

    if acpi_matchhids(&aaa, &ACPI_SKIP_HIDS, "none") != 0
        || acpi_matchhids(&aaa, &ACPI_ISA_HIDS, "none") != 0
    {
        return 0;
    }

    acpi_parse_crs(sc, &mut aaa);

    aaa.aaa_dmat = acpi_iommu_device_map(&parent, aaa.aaa_dmat);

    if parent.attached.get() == 0 {
        parent.attached.set(1);
        if acpi_matchhids(&aaa, &ACPI_QUIET_HIDS, "none") != 0 {
            acpi_config_found(sc, &mut aaa, acpi_noprint);
        } else {
            acpi_config_found(sc, &mut aaa, acpi_print);
        }
    }

    0
}

/// `acpi_founddock(node, sc)`: attaches `acpidock`.
fn acpi_founddock(node: &AmlNodeRef, sc: &AcpiSoftc) -> i32 {
    let mut aaa = acpi_attach_args(sc);
    aaa.aaa_node = node.parent();
    aaa.aaa_name = c"acpidock".as_ptr().cast();

    acpi_config_found(sc, &mut aaa, acpi_print);

    0
}

/// `acpi_foundvideo(node, sc)`: attaches `acpivideo`.
fn acpi_foundvideo(node: &AmlNodeRef, sc: &AcpiSoftc) -> i32 {
    let mut aaa = acpi_attach_args(sc);
    aaa.aaa_node = node.parent();
    aaa.aaa_name = c"acpivideo".as_ptr().cast();

    acpi_config_found(sc, &mut aaa, acpi_print);

    0
}

/// `acpi_foundsbs(node, sc)`: attaches `acpisbs` for a smart battery subsystem.
fn acpi_foundsbs(node: &AmlNodeRef, sc: &AcpiSoftc) -> i32 {
    let mut cdev = [0u8; 32];
    let mut dev = [0u8; 32];

    if acpi_parsehid(node, sc, &mut cdev, &mut dev) != 0 {
        return 0;
    }
    let Some(parent) = node.parent() else {
        return 0;
    };

    let sta = acpi_getsta(sc, Some(&parent));
    if sta & i64::from(STA_PRESENT) == 0 {
        return 0;
    }

    acpi_attach_deps(sc, &parent);

    if cstr(&dev) != ACPI_DEV_SBS.as_bytes() {
        return 0;
    }

    if parent.attached.get() != 0 {
        return 0;
    }

    let mut aaa = acpi_attach_args(sc);
    aaa.aaa_node = Some(parent.clone());
    aaa.aaa_dev = dev.as_ptr();
    aaa.aaa_cdev = cdev.as_ptr();

    acpi_config_found(sc, &mut aaa, acpi_print);
    parent.attached.set(1);

    0
}

/// `acpi_record_event(sc, type)`: tells the processes that opened acpi(4) about a power
/// event; 1 when none did.
pub fn acpi_record_event(sc: &AcpiSoftc, r#type: u32) -> i32 {
    if sc.sc_flags.get() & SCFLAG_OPEN == 0 {
        return 1;
    }

    ACPI_EVINDEX.fetch_add(1, Ordering::Relaxed);
    let _ = r#type;
    let _ = unported!("acpi_record_event: knote_locked(sc_note) (acpi_apm.c)");
    0
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests of acpi(4)'s core that need no firmware and no namespace: the id matching, the
    // `_CRS` and `_DSD` parsing, the task queue and the fixed registers of an unmapped acpi0.
    // (The namespace walks run in the QEMU smokes; `dsdt.rs` owns the namespace on the
    // host.)

    use std::boxed::Box;
    use std::sync::Mutex;
    use std::vec;
    use std::vec::Vec;

    use super::*;
    use crate::dev::acpi::amltypes::AmlObj;

    /// The task queue is global: the tests that use it take this lock.
    static TASKQ_LOCK: Mutex<()> = Mutex::new(());

    /// An acpi0 softc as autoconfiguration allocates it: all zero (the `Softc` contract).
    fn zeroed_softc() -> Box<AcpiSoftc> {
        // SAFETY: all-zero is a valid `AcpiSoftc` (`unsafe impl Softc`, acpivar.rs).
        unsafe { Box::<AcpiSoftc>::new_zeroed().assume_init() }
    }

    /// `memset(&aaa, 0, sizeof(aaa))` without a softc.
    fn aaa() -> AcpiAttachArgs {
        AcpiAttachArgs {
            aaa_name: ptr::null(),
            aaa_iot: None,
            aaa_memt: None,
            aaa_dmat: None,
            aaa_table: ptr::null_mut(),
            aaa_node: None,
            aaa_dev: ptr::null(),
            aaa_cdev: ptr::null(),
            aaa_addr: [0; 8],
            aaa_size: [0; 8],
            aaa_bst: [None; 8],
            aaa_naddr: 0,
            aaa_irq: [0; 8],
            aaa_irq_flags: [0; 8],
            aaa_nirq: 0,
        }
    }

    #[test]
    fn hids_match_as_in_c() {
        assert_eq!(_acpi_matchhids(b"PNP0C0F", &ACPI_SKIP_HIDS), 1);
        assert_eq!(_acpi_matchhids(b"PNP0501", &ACPI_SKIP_HIDS), 0);
        assert_eq!(_acpi_matchhids(b"ACPI0007", &ACPI_QUIET_HIDS), 1);

        let node: AmlNodeRef = Rc::new(crate::dev::acpi::amltypes::AmlNode::new(None, *b"COM1\0"));
        let mut a = aaa();
        let dev = *b"PNP0700\0";
        let cdev = *b"PNP0C02\0";
        // No node: no match.
        a.aaa_dev = dev.as_ptr();
        assert_eq!(acpi_matchhids(&a, &ACPI_ISA_HIDS, "fdc"), 0);
        a.aaa_node = Some(node);
        assert_eq!(acpi_matchhids(&a, &ACPI_ISA_HIDS, "fdc"), 2);
        // The _CID matches: 1.
        a.aaa_cdev = cdev.as_ptr();
        assert_eq!(acpi_matchhids(&a, &ACPI_SKIP_HIDS, "none"), 1);
    }

    #[test]
    fn hid_strings() {
        // EISAID("PNP0303") is 0x0303d041.
        assert_eq!(hid_string(&AmlValue::integer(0x0303_d041)), b"PNP0303");
        assert_eq!(hid_string(&AmlValue::string(b"QEMU0002")), b"QEMU0002");
        assert_eq!(hid_string(&AmlValue::buffer(&[1, 2])), b"unknown");

        let mut out = [0xffu8; 8];
        strlcpy(&mut out, b"ACPI0010-long");
        assert_eq!(&out, b"ACPI001\0");
    }

    /// A `_CRS` buffer: an I/O port range 0x3f8..0x3ff (8 bytes), IRQ 4 without flags (so
    /// edge, exclusive, active high), a fixed 32-bit memory range, a bus-number word range, and
    /// the end tag.
    fn crs() -> AmlValue {
        let mut b: Vec<u8> = vec![];
        b.extend_from_slice(&[0x47, 0x01, 0xf8, 0x03, 0xf8, 0x03, 0x01, 0x08]); // IO
        b.extend_from_slice(&[0x22, 0x10, 0x00]); // IRQNoFlags {4}
        b.extend_from_slice(&[0x86, 0x09, 0x00, 0x01]); // Memory32Fixed, read-write
        b.extend_from_slice(&0xfed0_0000u32.to_le_bytes());
        b.extend_from_slice(&0x400u32.to_le_bytes());
        // WordBusNumber: type 2, _MIN 0, _MAX 0xff, _LEN 0x100.
        b.extend_from_slice(&[0x88, 0x0d, 0x00, 0x02, 0x0c, 0x00]);
        for w in [0u16, 0, 0xff, 0, 0x100] {
            b.extend_from_slice(&w.to_le_bytes());
        }
        b.extend_from_slice(&[0x79, 0x00]); // EndTag
        AmlValue::buffer(&b)
    }

    #[test]
    fn crs_resources_fill_the_attach_args() {
        let mut a = aaa();
        aml_parse_resource(&crs(), &mut |i, r| acpi_parse_resources(i, r, &mut a));
        // The bus range is skipped; the port and the memory are recorded.
        assert_eq!(a.aaa_naddr, 2);
        assert_eq!((a.aaa_addr[0], a.aaa_size[0]), (0x3f8, 8));
        assert_eq!((a.aaa_addr[1], a.aaa_size[1]), (0xfed0_0000, 0x400));
        assert_eq!(a.aaa_nirq, 1);
        assert_eq!(a.aaa_irq[0], 4);
        assert_eq!(a.aaa_irq_flags[0], u32::from(LR_EXTIRQ_MODE));

        let mut bus = -1;
        aml_parse_resource(&crs(), &mut |i, r| acpi_getminbus(i, r, &mut bus));
        assert_eq!(bus, 0);
    }

    #[test]
    fn dsd_properties_are_found_by_name() {
        let pkg = |v: Vec<AmlValueRef>| AmlValue::from_obj(AmlObj::Package(v));
        let prop = |name: &[u8], v: AmlValue| {
            Rc::new(pkg(vec![Rc::new(AmlValue::string(name)), Rc::new(v)]))
        };
        let dsd = pkg(vec![
            Rc::new(AmlValue::buffer(&DSD_PROP_GUID)),
            Rc::new(pkg(vec![
                prop(b"clock-frequency", AmlValue::integer(1_843_200)),
                prop(b"compatible", AmlValue::string(b"snps,dw-apb-uart")),
            ])),
        ]);
        let props = dsd_properties(&dsd, &DSD_PROP_GUID).unwrap();
        assert_eq!(
            dsd_property(&props, b"clock-frequency")
                .unwrap()
                .v_integer(),
            1_843_200
        );
        assert_eq!(
            dsd_property(&props, b"compatible").unwrap().v_string(),
            b"snps,dw-apb-uart"
        );
        assert!(dsd_property(&props, b"missing").is_none());
        // Another UUID: not device properties.
        assert!(dsd_properties(&dsd, &[0; 16]).is_none());
    }

    static RAN: Mutex<Vec<(usize, i32)>> = Mutex::new(Vec::new());

    fn record(arg0: *mut c_void, arg1: i32) {
        RAN.lock().unwrap().push((arg0 as usize, arg1));
    }

    #[test]
    fn tasks_run_once_in_order() {
        let _g = TASKQ_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let sc = zeroed_softc();
        RAN.lock().unwrap().clear();

        assert_eq!(acpi_dotask(&sc), 0);
        acpi_addtask(&sc, record, 1 as *mut c_void, 10);
        acpi_addtask(&sc, record, 2 as *mut c_void, 20);
        while acpi_dotask(&sc) != 0 {}
        assert_eq!(*RAN.lock().unwrap(), vec![(1, 10), (2, 20)]);
        assert_eq!(acpi_dotask(&sc), 0);
    }

    #[test]
    fn an_unmapped_acpi0() {
        let sc = zeroed_softc();
        // No register is mapped: reads give 0, writes go nowhere.
        assert_eq!(acpi_read_pmreg(&sc, ACPIREG_PM1A_CNT, 0), 0);
        acpi_write_pmreg(&sc, ACPIREG_PM1A_CNT, 0, 0x2000);
        // Hardware-reduced ACPI is always in ACPI mode.
        sc.sc_hw_reduced.set(1);
        assert_eq!(
            acpi_read_pmreg(&sc, ACPIREG_PM1B_CNT, 0),
            i32::from(ACPI_PM1_SCI_EN)
        );
        // No acpi0 at all: the access fails.
        let mut b = [0u8; 4];
        assert_eq!(
            acpi_gasio(None, ACPI_IOREAD, GAS_SYSTEM_IOSPACE, 0xcf9, 1, 1, &mut b),
            -1
        );
        // The host double maps no firmware table.
        assert!(acpi_maptable(&sc, Paddr::new(0xe0000), None, None, None, 1).is_none());
        assert!(sc.sc_tables.iter().next().is_none());
    }
}
/* </TESTS> */
