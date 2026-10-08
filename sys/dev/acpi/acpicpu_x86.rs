/* $OpenBSD: acpicpu_x86.c,v 1.2 2025/09/16 12:18:10 hshoexer Exp $ */
/* <LICENSES> */
/*
 * Copyright (c) 2005 Marco Peereboom <marco@openbsd.org>
 * Copyright (c) 2015 Philip Guenther <guenther@openbsd.org>
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
//! acpicpu(4) on x86: `dev/acpi/acpicpu_x86.c`, the processor's ACPI power management.
//! One acpicpu attaches per CPU, to the namespace's `Processor()` object or `ACPI0007`
//! device whose ACPI id is the CPU's (`ci_acpi_proc_id`, from the MADT). It tells the
//! firmware what the OS handles (`_OSC`, or `_PDC`), reads the CPU's idle states (`_CST`,
//! or the FADT's `P_LVL2`/`P_LVL3` with the processor block), makes `acpicpu_idle` the
//! idle loop's wait (`cpu_idle_cycle_fcn`) and `acpicpu_suspend` the halted CPU's
//! (`cpu_suspend_cycle_fcn`), and reads the performance states (`_PSS`, `_PPC`, `_PCT`),
//! installing `acpicpu_setperf` as `cpu_setperf` when it can drive them. The notify
//! handler re-reads `_PPC`/`_PSS` (0x80) or `_CST` (0x81).
//!
//! Upstream: sys/dev/acpi/acpicpu_x86.c @ 3ce1f3f79392
//!
//! The idle loop then picks, every time, the deepest state whose latency fits three times
//! into the recent sleep length (`sc_prev_sleep`, a decaying average) and is not marked
//! skippable (C2 and deeper without an always-running APIC timer, `TPM_ARAT`), waits in it
//! (`sti; hlt`, an I/O read of the state's port, or `monitor`/`mwait` on `ci_mwait`, which
//! `cpu_kick` and `cpu_unidle` clear to wake the CPU without an IPI) and folds the time
//! spent into the average. amd64's ioconf has `acpicpu* at acpi?`, as its GENERIC; arm64's
//! GENERIC line is `dev/acpi/acpicpu_arm64.c`'s, not this file's. Under QEMU (q35, `-cpu
//! qemu64`) the CPUs are `ACPI0007` devices with no `_CST`, `_PSS` or processor block, and
//! the FADT's `P_LVL2_UP` is clear: each acpicpu keeps the C1 `hlt` fallback only
//! (`acpicpu0 at acpi0: C1(@1 halt!)`), and the idle loop runs `acpicpu_idle`.
//!
//! The driver is x86's: it reaches `struct cpu_info` (`ci_acpicpudev`, `ci_mwait`,
//! `ci_feature_tpmflags`), the machine's idle hooks and `inb`/`monitor`/`mwait` through
//! `machine::x86`, so those items exist where cfg `machine_x86` is set (amd64). The
//! `_CST`/`_CSD`/`_PSS` parsing, the state list, its printing and the choice of a state are
//! plain functions on every target, with host tests.
//!
//! ## Deviations
//! - `sc_cstates` (the C's `SLIST`, head first) is a `Vec` of `AcpiCstate` values behind a
//!   mutex of its own (`sc_cst_mtx`): `acpicpu_notify` replaces the list while the CPU may
//!   be idling on it, where the C frees the entries under the idle loop's feet. The list is
//!   built aside (`acpicpu_getcst`) and swapped in; the idle and suspend paths copy the
//!   state they chose out under the mutex and wait after leaving it. The `Vec` is written
//!   once by the attach before anything reads it (`UnsafeCell<MaybeUninit<_>>`, as a softc
//!   member the attach fills, docs/C_TO_RUST.md).
//! - `acpicpu_add_cstate`, `acpicpu_add_cstatepkg`, `acpicpu_add_cdeppkg` and the parts of
//!   `acpicpu_getcst`/`acpicpu_getcst_from_fadt` that edit the list take the list; what the
//!   C prints from inside them is returned as text and printed by the caller, at the same
//!   place in the line. `check_mwait_hints` takes `cpu_mwait_size`/`cpu_mwait_states`.
//! - `sc_flags`, `sc_prev_sleep` and `sc_last_itime` are atomics (the idle loop and the
//!   notify path both touch them); the other members are `Cell`s written by the attach or
//!   the notify task, under the kernel lock.
//! - Out-of-range values the C would trip on are clamped: `cst_stats[best->state]` for a
//!   `_CST` C4 (the array has 4 entries) is not counted; `acpicpu_setperf`'s index is kept
//!   below `sc_pss_len` (the C's `idx > sc_pss_len` lets `sc_pss_len` through) and a
//!   `_PSS` of more than 100 states (`100 / len` is 0) picks the first; the `_PCT` status
//!   and control reads and writes move at most the 4 bytes of the `uint32_t` they target;
//!   the bus-master back-off that finds no shallower state keeps the last state of the
//!   list (the C's `best` becomes NULL). A missing `_PSS` or `_PCT` sub-object reads as 0
//!   (`aml_val2int(NULL)`), or a short buffer as zeroes, where the C reads past it.
//! - `_OSC`'s capabilities buffer is the C's 12 bytes (`uint32_t buf[3]`) with the third
//!   word 0, where the C leaves it uninitialised (`_OSC` reads two words: its count is 2).
//! - The `#if 0` `acpicpu_set_throttle` and `acpicpu_find_cstate` (and `CPU_THT_EN`,
//!   `CPU_STATE`, `CPU_STATEMASK`, which only they use) are not compiled in C and not here;
//!   `ACPI_DEBUG` (`dnprintf`, `aml_showvalue` of each `_CST` package, the `_PSS` dump) is
//!   not configured. `acpicpu_add_cdeppkg`'s `#if 1 ||` `aml_showvalue` is kept.
//! - `cpu_setperf` (`kern/sched_bsd.c`) is installed but nothing calls it yet: `hw.setperf`
//!   and `perfpolicy` are not ported (`sched_bsd.rs`); `est`/`k1x` (`acpicpu_fetch_pss`,
//!   `acpicpu_set_notify`) are not ported either. Under QEMU no `_PSS` exists.
//! - `cpu_suspend_cycle_fcn` is used by `x86_64_ipi_halt` (the CPUs `boot(9)` halts); the
//!   S0 suspend loop (`cpu_suspend_primary`, `SUSPEND`) is not ported, so `acpicpu_suspend`
//!   runs only there.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
#[cfg(machine_x86)]
use core::cell::{Cell, RefCell, UnsafeCell};
#[cfg(machine_x86)]
use core::ffi::c_void;
#[cfg(machine_x86)]
use core::mem::MaybeUninit;
use core::ptr;
use core::sync::atomic::AtomicU64;
#[cfg(machine_x86)]
use core::sync::atomic::{AtomicI32, Ordering};

#[cfg(machine_x86)]
use super::acpi::{
    ACPI_FORCE_BM, ACPI_HASPROCFVS, aaa_name, acpi_gasio, acpi_matchhids, acpi_read_pmreg,
    acpi_write_pmreg, fadt,
};
#[cfg(machine_x86)]
use super::acpidev::ACPIDEV_NOPOLL;
use super::acpidev::{AcpiGrd, AcpicpuPct, AcpicpuPss};
#[cfg(machine_x86)]
use super::acpireg::{ACPI_PM1_BM_RLD, ACPI_PM1_BM_STS};
use super::acpireg::{AcpiGas, FADT_P_LVL2_UP, GAS_FUNCTIONAL_FIXED, GAS_SYSTEM_IOSPACE};
#[cfg(machine_x86)]
use super::acpivar::{
    ACPI_IOREAD, ACPI_IOWRITE, ACPIREG_PM1_CNT, ACPIREG_PM1_STS, ACPIREG_SMICMD, AcpiAttachArgs,
    AcpiSoftc, acpi_softc,
};
use super::amltypes::{AML_OBJTYPE_BUFFER, AML_OBJTYPE_PACKAGE, AmlValue};
#[cfg(machine_x86)]
use super::amltypes::{AML_OBJTYPE_DEVICE, AML_OBJTYPE_PROCESSOR, AmlNodeRef};
use super::dsdt::{LR_GENREGISTER, SRT_ENDTAG, aml_val2int};
#[cfg(machine_x86)]
use super::dsdt::{
    aml_evalinteger, aml_evalname, aml_evalnode, aml_foreachpkg, aml_freevalue,
    aml_register_notify, aml_searchname, aml_showvalue, cstr,
};
#[cfg(machine_x86)]
use crate::kern::init_main::NCPUS;
#[cfg(machine_x86)]
use crate::kern::kern_lock::{mtx_enter, mtx_init, mtx_leave};
#[cfg(machine_x86)]
use crate::kern::kern_tc::microuptime;
#[cfg(machine_x86)]
use crate::kern::sched_bsd::CPU_SETPERF;
#[cfg(machine_x86)]
use crate::kern::subr_prf::{Str, panic, printf};
#[cfg(machine_x86)]
use crate::machine::acpi_machdep::{ci_acpi_proc_id, cpu_suspended};
#[cfg(machine_x86)]
use crate::machine::cpu::{cpu_info_foreach, curcpu};
#[cfg(machine_x86)]
use crate::machine::intr::IPL_NONE;
#[cfg(machine_x86)]
use crate::machine::x86::{
    CPU_IDLE_CYCLE_FCN, CPU_MWAIT_SIZE, CPU_MWAIT_STATES, CPU_SUSPEND_CYCLE_FCN, CPU_VENDOR,
    CPUSPEED, CpuInfo, MWAIT_IDLING, MWAIT_ONLY, PSL_I, SETPERF_PRIO, TPM_ARAT, clflush,
    cpu_idle_cycle_hlt, cpu_info_primary, cpu_is_primary, inb, intr_enable, monitor, mwait,
    read_rflags,
};
#[cfg(machine_x86)]
use crate::sys::device::{CD_COCOVM, CfMatch, Cfattach, Cfdriver, DV_DULL, Device, Softc};
#[cfg(machine_x86)]
use crate::sys::mutex::Mutex;
#[cfg(machine_x86)]
use crate::sys::sched::cpu_is_idle;

/// `ACPI_STATE_C0`.
pub const ACPI_STATE_C0: u16 = 0x00;
/// `ACPI_STATE_C1`.
pub const ACPI_STATE_C1: u16 = 0x01;
/// `ACPI_STATE_C2`.
pub const ACPI_STATE_C2: u16 = 0x02;
/// `ACPI_STATE_C3`.
pub const ACPI_STATE_C3: u16 = 0x03;

/// `ACPI_PDC_REVID`.
pub const ACPI_PDC_REVID: u32 = 0x1;
/// `ACPI_PDC_SMP`.
pub const ACPI_PDC_SMP: u32 = 0xa;
/// `ACPI_PDC_MSR`.
pub const ACPI_PDC_MSR: u32 = 0x1;

// _PDC/_OSC Intel capabilities flags
/// `ACPI_PDC_P_FFH`.
pub const ACPI_PDC_P_FFH: u32 = 0x0001;
/// `ACPI_PDC_C_C1_HALT`.
pub const ACPI_PDC_C_C1_HALT: u32 = 0x0002;
/// `ACPI_PDC_T_FFH`.
pub const ACPI_PDC_T_FFH: u32 = 0x0004;
/// `ACPI_PDC_SMP_C1PT`.
pub const ACPI_PDC_SMP_C1PT: u32 = 0x0008;
/// `ACPI_PDC_SMP_C2C3`.
pub const ACPI_PDC_SMP_C2C3: u32 = 0x0010;
/// `ACPI_PDC_SMP_P_SWCOORD`.
pub const ACPI_PDC_SMP_P_SWCOORD: u32 = 0x0020;
/// `ACPI_PDC_SMP_C_SWCOORD`.
pub const ACPI_PDC_SMP_C_SWCOORD: u32 = 0x0040;
/// `ACPI_PDC_SMP_T_SWCOORD`.
pub const ACPI_PDC_SMP_T_SWCOORD: u32 = 0x0080;
/// `ACPI_PDC_C_C1_FFH`.
pub const ACPI_PDC_C_C1_FFH: u32 = 0x0100;
/// `ACPI_PDC_C_C2C3_FFH`.
pub const ACPI_PDC_C_C2C3_FFH: u32 = 0x0200;
// reserved			0x0400
/// `ACPI_PDC_P_HWCOORD`.
pub const ACPI_PDC_P_HWCOORD: u32 = 0x0800;
/// `ACPI_PDC_PPC_NOTIFY`.
pub const ACPI_PDC_PPC_NOTIFY: u32 = 0x1000;

/// `CST_METH_HALT`.
pub const CST_METH_HALT: i16 = 0;
/// `CST_METH_IO_HALT`.
pub const CST_METH_IO_HALT: i16 = 1;
/// `CST_METH_MWAIT`.
pub const CST_METH_MWAIT: i16 = 2;
/// `CST_METH_GAS_IO`.
pub const CST_METH_GAS_IO: i16 = 3;

// flags on Intel's FFH mwait method
/// `CST_FLAG_MWAIT_HW_COORD`.
pub const CST_FLAG_MWAIT_HW_COORD: u16 = 0x1;
/// `CST_FLAG_MWAIT_BM_AVOIDANCE`.
pub const CST_FLAG_MWAIT_BM_AVOIDANCE: u16 = 0x2;
/// `CST_FLAG_FALLBACK`: fallback for broken `_CST`.
pub const CST_FLAG_FALLBACK: u16 = 0x4000;
/// `CST_FLAG_SKIP`: state is worse choice.
pub const CST_FLAG_SKIP: u16 = 0x8000;

/// `FLAGS_MWAIT_ONLY`.
pub const FLAGS_MWAIT_ONLY: i32 = 0x02;
/// `FLAGS_BMCHECK`.
pub const FLAGS_BMCHECK: i32 = 0x04;
/// `FLAGS_NOTHROTTLE`.
pub const FLAGS_NOTHROTTLE: i32 = 0x08;
/// `FLAGS_NOPSS`.
pub const FLAGS_NOPSS: i32 = 0x10;
/// `FLAGS_NOPCT`.
pub const FLAGS_NOPCT: i32 = 0x20;

/// `ACPI_MAX_C2_LATENCY`.
pub const ACPI_MAX_C2_LATENCY: u16 = 100;
/// `ACPI_MAX_C3_LATENCY`.
pub const ACPI_MAX_C3_LATENCY: u16 = 1000;

/// `CSD_COORD_SW_ALL`.
pub const CSD_COORD_SW_ALL: i64 = 0xFC;
/// `CSD_COORD_SW_ANY`.
pub const CSD_COORD_SW_ANY: i64 = 0xFD;
/// `CSD_COORD_HW_ALL`.
pub const CSD_COORD_HW_ALL: i64 = 0xFE;

/// The `_OSC` UUID of the processor capabilities, 4077A616-290C-47BE-9EBD-D87058713953.
pub const CPU_OSCUUID: [u8; 16] = [
    0x16, 0xA6, 0x77, 0x40, 0x0C, 0x29, 0xBE, 0x47, 0x9E, 0xBD, 0xD8, 0x70, 0x58, 0x71, 0x39, 0x53,
];

/// `struct acpi_cstate`: one idle state (the `SLIST` link is the list's order).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AcpiCstate {
    /// `state`: C1, C2, C3 (C4 accepted).
    pub state: u16,
    /// `method`: `CST_METH_*`.
    pub method: i16,
    /// `flags`: `CST_FLAG_*`.
    pub flags: u16,
    /// `latency`: microseconds.
    pub latency: u16,
    /// `power`: milliwatts, -1 when unknown.
    pub power: i32,
    /// `address`: the I/O port, or the mwait hint.
    pub address: u64,
}

/// What [`acpicpu_add_cstatepkg`] makes of one `_CST` package.
#[derive(Debug, PartialEq, Eq)]
pub enum CstPkg {
    /// Ignored without a word (not a 4-element package, a state out of 0..=4, or mwait
    /// hints refused silently).
    Skip,
    /// Refused, with the text the C prints.
    Bad(String),
    /// A state to add, after the text the C printed on the way (`check_mwait_hints`'s).
    State(AcpiCstate, String),
}

/// What [`acpicpu_add_cdeppkg`] makes of one `_CSD` package.
#[derive(Debug, PartialEq, Eq)]
pub enum CsdPkg {
    /// Malformed: the C dumps it (`aml_showvalue`) and prints "bogus CSD".
    Bogus,
    /// Nothing to say (one CPU, or the hardware coordinates).
    Quiet,
    /// The dependency, as the C prints it.
    Dep(String),
}

/// `cst_stats[]`: how many times the idle loop chose each state.
pub static CST_STATS: [AtomicU64; 4] = [const { AtomicU64::new(0) }; 4];

/// `void (*)(struct acpicpu_pss *, int)`: a `_PPC` change listener (`sc_notify`).
pub type AcpicpuNotifyFn = fn(&[AcpicpuPss], i32);

/// `struct acpicpu_softc`.
#[cfg(machine_x86)]
#[repr(C)]
pub struct AcpicpuSoftc {
    /// `sc_dev`.
    pub sc_dev: Device,
    /// `sc_cpu`: the ACPI processor id.
    pub sc_cpu: Cell<i32>,
    /// `sc_duty_wid`.
    pub sc_duty_wid: Cell<i32>,
    /// `sc_duty_off`.
    pub sc_duty_off: Cell<i32>,
    /// `sc_pblk_addr`: the processor block (`Processor()` objects only).
    pub sc_pblk_addr: Cell<u32>,
    /// `sc_pblk_len`.
    pub sc_pblk_len: Cell<i32>,
    /// `sc_flags`: `FLAGS_*`.
    pub sc_flags: AtomicI32,
    /// `sc_prev_sleep`: the decaying average of the idle time, in microseconds.
    pub sc_prev_sleep: AtomicU64,
    /// `sc_last_itime`: the last idle time.
    pub sc_last_itime: AtomicU64,
    /// `sc_ci`: the CPU.
    pub sc_ci: Cell<*const CpuInfo>,
    /// Guards `sc_cstates` (not in the C, see the module's deviations).
    pub sc_cst_mtx: Mutex,
    /// `sc_cstates`, head first; written once by the attach, then only under `sc_cst_mtx`.
    pub sc_cstates: UnsafeCell<MaybeUninit<Vec<AcpiCstate>>>,
    /// `sc_acpi`.
    pub sc_acpi: Cell<*const AcpiSoftc>,
    /// `sc_devnode`.
    pub sc_devnode: RefCell<Option<AmlNodeRef>>,
    /// `sc_pss_len`.
    pub sc_pss_len: Cell<i32>,
    /// `sc_ppc`.
    pub sc_ppc: Cell<i32>,
    /// `sc_level`: the performance level in percent.
    pub sc_level: Cell<i32>,
    /// `sc_pss` (with `sc_pssfulllen`): the performance states; written once by the attach
    /// (as `sc_cstates`), then used under the kernel lock ([`AcpicpuSoftc::pss`]).
    pub sc_pss: UnsafeCell<MaybeUninit<RefCell<Vec<AcpicpuPss>>>>,
    /// `sc_pct`.
    pub sc_pct: Cell<AcpicpuPct>,
    /// `sc_pct_stat_as`: save compensation for pct access for lying bios'.
    pub sc_pct_stat_as: Cell<u32>,
    /// `sc_pct_ctrl_as`.
    pub sc_pct_ctrl_as: Cell<u32>,
    /// `sc_pct_stat_len`.
    pub sc_pct_stat_len: Cell<u32>,
    /// `sc_pct_ctrl_len`.
    pub sc_pct_ctrl_len: Cell<u32>,
    /// `sc_notify`: the `_PPC` change listener (one only, as in C).
    pub sc_notify: Cell<Option<AcpicpuNotifyFn>>,
}

#[cfg(machine_x86)]
impl AcpicpuSoftc {
    /// `sc->sc_acpi`.
    fn acpi(&self) -> Option<&'static AcpiSoftc> {
        // SAFETY: null or acpi0's softc, which is never freed.
        unsafe { self.sc_acpi.get().as_ref() }
    }

    /// `sc->sc_dev.dv_xname` (`DEVNAME(sc)`).
    fn xname(&self) -> &str {
        self.sc_dev.xname()
    }

    /// `sc->sc_devnode`.
    fn node(&self) -> Option<AmlNodeRef> {
        self.sc_devnode.borrow().clone()
    }

    /// `sc->sc_devnode->name`.
    fn node_name(&self) -> [u8; 5] {
        self.sc_devnode.borrow().as_ref().map_or([0; 5], |n| n.name)
    }

    /// `sc_pss`.
    fn pss(&self) -> &RefCell<Vec<AcpicpuPss>> {
        // SAFETY: the attach initialised it first thing, before the softc is linked to its
        // CPU or registered anywhere, and it is never written again.
        unsafe { (*self.sc_pss.get()).assume_init_ref() }
    }

    /// Runs `f` on `sc_cstates` under `sc_cst_mtx`.
    fn with_cstates<R>(&self, f: impl FnOnce(&[AcpiCstate]) -> R) -> R {
        mtx_enter(&self.sc_cst_mtx);
        // SAFETY: the attach initialised the list before anything reads it, and
        // `sc_cst_mtx` is held, so nobody replaces it meanwhile.
        let r = f(unsafe { (*self.sc_cstates.get()).assume_init_ref() });
        mtx_leave(&self.sc_cst_mtx);
        r
    }

    /// Replaces `sc_cstates` with `list` under `sc_cst_mtx`; the old list is freed after.
    fn set_cstates(&self, list: Vec<AcpiCstate>) {
        mtx_enter(&self.sc_cst_mtx);
        // SAFETY: as for `with_cstates`; the mutex makes this the only access.
        let old = core::mem::replace(unsafe { (*self.sc_cstates.get()).assume_init_mut() }, list);
        mtx_leave(&self.sc_cst_mtx);
        drop(old);
    }
}

// SAFETY: `#[repr(C)]` with the device first; every other member is valid as zero bits:
// zeros, null pointers, an unlocked mutex, `MaybeUninit`s (the lists, which the attach writes
// before any use), an unborrowed `RefCell` of `None`, a `None` function pointer.
#[cfg(machine_x86)]
unsafe impl Softc for AcpicpuSoftc {}

/// `acpicpu_ca`.
#[cfg(machine_x86)]
pub static ACPICPU_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<AcpicpuSoftc>(),
    ca_match: Some(acpicpu_match),
    ca_attach: acpicpu_attach,
    ca_detach: None,
    ca_activate: None,
};

/// `acpicpu_cd`.
#[cfg(machine_x86)]
pub static ACPICPU_CD: Cfdriver = Cfdriver::new(b"acpicpu", DV_DULL, CD_COCOVM);

/// `acpicpu_hids[]`.
pub static ACPICPU_HIDS: [&str; 1] = ["ACPI0007"];

/// `valid_throttle(o, w, a)`: the FADT's duty cycle field fits the processor block's
/// control register (offset `o`, width `w`, block address `a`).
pub fn valid_throttle(o: i32, w: i32, a: u32) -> bool {
    a != 0 && w != 0 && (o + w) <= 31 && (o > 4 || (o + w) <= 4)
}

/// `CPU_MAXSTATE(sc)`: the number of throttling states of a duty width.
pub const fn cpu_maxstate(duty_wid: i32) -> i64 {
    1i64 << duty_wid
}

/// The capabilities acpicpu reports to `_OSC`/`_PDC`.
pub const ACPICPU_PDC_CAP: u32 = ACPI_PDC_C_C1_HALT
    | ACPI_PDC_P_FFH
    | ACPI_PDC_C_C1_FFH
    | ACPI_PDC_C_C2C3_FFH
    | ACPI_PDC_SMP_P_SWCOORD
    | ACPI_PDC_SMP_C2C3
    | ACPI_PDC_SMP_C1PT;

/// The bytes of a buffer of 32-bit words, as the C passes `uint32_t buf[]`.
pub fn acpicpu_pdc_words(w: &[u32]) -> Vec<u8> {
    w.iter().flat_map(|x| x.to_le_bytes()).collect()
}

/// `acpicpu_set_pdc(sc)`: tells the firmware the OS's power management capabilities:
/// through `_OSC` (a query, then the capabilities the firmware granted) when the processor
/// has one, otherwise through `_PDC`.
#[cfg(machine_x86)]
pub fn acpicpu_set_pdc(sc: &AcpicpuSoftc) {
    let acpi = sc.acpi();
    let node = sc.node();
    let cap = ACPICPU_PDC_CAP;

    if aml_searchname(node.as_ref(), b"_OSC").is_some() {
        // Query _OSC
        let osc_cmd = [
            AmlValue::buffer(&CPU_OSCUUID),
            AmlValue::integer(1),
            AmlValue::integer(2),
            AmlValue::buffer(&acpicpu_pdc_words(&[1, cap, 0])),
        ];
        let res = AmlValue::new();
        aml_evalname(acpi, node.as_ref(), b"_OSC", &osc_cmd, Some(&res));

        let buf = res.v_buffer();
        if res.r#type() != AML_OBJTYPE_BUFFER || res.length() < 8 || buf.len() < 8 {
            printf(format_args!(": unable to query capabilities\n"));
            aml_freevalue(Some(&res));
            return;
        }

        // Evaluate _OSC
        let granted = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]) & cap;
        aml_freevalue(Some(&res));
        let osc_cmd = [
            AmlValue::buffer(&CPU_OSCUUID),
            AmlValue::integer(1),
            AmlValue::integer(2),
            AmlValue::buffer(&acpicpu_pdc_words(&[0, granted, 0])),
        ];
        aml_evalname(acpi, node.as_ref(), b"_OSC", &osc_cmd, None);
    } else {
        // Evaluate _PDC
        let cmd = [AmlValue::buffer(&acpicpu_pdc_words(&[
            ACPI_PDC_REVID,
            1,
            cap,
        ]))];
        aml_evalname(acpi, node.as_ref(), b"_PDC", &cmd, None);
    }
}

/// `check_mwait_hints(state, hints)`: sanity checks mwait hints against what cpuid told us
/// (`cpu_mwait_size`, `cpu_mwait_states`)... but because intel screwed up, just checks
/// whether cpuid says the given state has _any_ substates. Whether the hints are accepted,
/// and what the C prints when it refuses them with a reason.
pub fn check_mwait_hints(
    state: i32,
    hints: u64,
    mwait_size: u32,
    mwait_states: u32,
) -> (bool, String) {
    if mwait_size == 0 {
        return (false, String::new());
    }
    let hints = hints as i32;
    let mut cstate = ((hints >> 4) & 0xf) + 1;
    if cstate == 16 {
        cstate = 0;
    } else if cstate > 7 {
        // out of range of test against CPUID; just trust'em
        return (true, String::new());
    }
    let num_substates = (mwait_states >> (4 * cstate)) & 0xf;
    if num_substates == 0 {
        return (
            false,
            format!(": C{state} bad (state {cstate} has no substates)"),
        );
    }
    (true, String::new())
}

/// `acpicpu_add_cstate(sc, state, method, flags, latency, power, address)`: adds a state at
/// the head of `list`, or overwrites the fallback C1 at the head with the real C1.
pub fn acpicpu_add_cstate(list: &mut Vec<AcpiCstate>, cx: AcpiCstate) {
    // add a new state, or overwrite the fallback C1 state?
    match list.first_mut() {
        Some(head) if cx.state == ACPI_STATE_C1 && head.flags & CST_FLAG_FALLBACK != 0 => {
            *head = cx;
        }
        _ => list.insert(0, cx),
    }
}

/// `printf(" %#x")` of a byte: `0` for zero, `0x..` otherwise, as C's `#` flag does.
fn alt_hex(b: u64) -> String {
    if b == 0 {
        String::from("0")
    } else {
        format!("{b:#x}")
    }
}

/// `acpicpu_add_cstatepkg(val, arg)`: the state one `_CST` package `{ register, type,
/// latency, power }` describes, with the checks the C makes (the register is a Generic
/// Register descriptor: functional fixed hardware for `hlt`, `inb; hlt` or `mwait`, or an
/// 8-bit I/O port).
pub fn acpicpu_add_cstatepkg(val: &AmlValue, mwait_size: u32, mwait_states: u32) -> CstPkg {
    if val.r#type() != AML_OBJTYPE_PACKAGE || val.length() != 4 {
        return CstPkg::Skip;
    }
    let int = |i: usize| val.v_package(i).map_or(0, |v| v.v_integer());

    // range and sanity checks
    let state = int(1) as i32;
    if !(0..=4).contains(&state) {
        return CstPkg::Skip;
    }
    let Some(reg) = val.v_package(0) else {
        return CstPkg::Skip;
    };
    if reg.r#type() != AML_OBJTYPE_BUFFER {
        return CstPkg::Bad(format!(
            ": C{state} (unexpected ACPI object type {})",
            reg.r#type()
        ));
    }
    let buf = reg.v_buffer();
    let grd_size = size_of::<AcpiGrd>();
    if reg.length() as usize != grd_size + 2
        || buf.len() < grd_size + 1
        || i32::from(buf[0]) != LR_GENREGISTER
        || usize::from(u16::from_le_bytes([buf[1], buf[2]])) != size_of::<AcpiGas>()
        || buf[grd_size] != SRT_ENDTAG
    {
        return CstPkg::Bad(format!(": C{state} (bogo buffer)"));
    }
    // struct acpi_gas, after the descriptor and its length.
    let gas = &buf[3..3 + size_of::<AcpiGas>()];
    let (space_id, bit_width, bit_offset, access_size) = (gas[0], gas[1], gas[2], gas[3]);
    let address = u64::from_le_bytes([
        gas[4], gas[5], gas[6], gas[7], gas[8], gas[9], gas[10], gas[11],
    ]);

    let mut flags = 0u16;
    let mut said = String::new();
    let (method, addr) = match i32::from(space_id) {
        GAS_FUNCTIONAL_FIXED => {
            if bit_width == 0 {
                (CST_METH_HALT, 0)
            } else {
                // In theory we should only do this for vendor 1 == Intel but other values
                // crop up, presumably due to the normal ACPI spec confusion.
                match bit_offset {
                    0x1 => {
                        // i386 and amd64 I/O space is 16bits
                        if address > 0xffff {
                            return CstPkg::Bad(format!(": C{state} (bogo I/O addr {address:x})"));
                        }
                        (CST_METH_IO_HALT, address)
                    }
                    0x2 => {
                        let (ok, msg) = check_mwait_hints(state, address, mwait_size, mwait_states);
                        if !ok {
                            return if msg.is_empty() {
                                CstPkg::Skip
                            } else {
                                CstPkg::Bad(msg)
                            };
                        }
                        said = msg;
                        flags = u16::from(access_size);
                        (CST_METH_MWAIT, address)
                    }
                    _ => {
                        return CstPkg::Bad(format!(": C{state} (unknown FFH class {bit_offset})"));
                    }
                }
            }
        }
        GAS_SYSTEM_IOSPACE => {
            if bit_width != 8 || bit_offset != 0 {
                return CstPkg::Bad(format!(
                    ": C{state} (unhandled I/O spec: {bit_width}/{bit_offset})"
                ));
            }
            (CST_METH_GAS_IO, address)
        }
        _ => {
            // dump the GAS for analysis
            let mut s = format!(": C{state} (unhandled GAS:");
            for &b in gas {
                s.push(' ');
                s.push_str(&alt_hex(u64::from(b)));
            }
            s.push(')');
            return CstPkg::Bad(s);
        }
    };

    CstPkg::State(
        AcpiCstate {
            state: state as u16,
            method,
            flags,
            latency: int(2) as u16,
            power: int(3) as i32,
            address: addr,
        },
        said,
    )
}

/// `acpicpu_add_cdeppkg(val, arg)`: a `_CSD` package `{ count, revision, domain,
/// coordination, processors, index }`, printed when it says something the hardware does not
/// coordinate itself.
pub fn acpicpu_add_cdeppkg(val: &AmlValue) -> CsdPkg {
    let int = |i: usize| val.v_package(i).map_or(0, |v| v.v_integer());

    // errors: unexpected object type, bad length, mismatched length, and bad CSD revision
    if val.r#type() != AML_OBJTYPE_PACKAGE
        || val.length() < 6
        || i64::from(val.length()) != int(0)
        || int(1) != 0
    {
        return CsdPkg::Bogus;
    }

    // coordinating 'among' one CPU is trivial, ignore
    let num_proc = int(4);
    if num_proc == 1 {
        return CsdPkg::Quiet;
    }

    // we practically assume the hardware will coordinate, so ignore
    let coord_type = int(3);
    if coord_type == CSD_COORD_HW_ALL {
        return CsdPkg::Quiet;
    }

    let domain = int(2);
    let cindex = int(5);
    CsdPkg::Dep(format!(
        ": CSD (c={} d={domain} n={num_proc} i={cindex})",
        alt_hex(coord_type as u64)
    ))
}

/// The C1 `hlt` state `acpicpu_getcst` starts from, in case `_CST`'s C1 is bogus.
pub const ACPICPU_FALLBACK_C1: AcpiCstate = AcpiCstate {
    state: ACPI_STATE_C1,
    method: CST_METH_HALT,
    flags: CST_FLAG_FALLBACK,
    latency: 1,
    power: -1,
    address: 0,
};

/// `acpicpu_getcst`'s pass over the list it built: marks C2 and deeper skippable when the
/// CPU's LAPIC timer stops in deep states (no `TPM_ARAT`), and says whether every state it
/// looked at uses `mwait` (`FLAGS_MWAIT_ONLY`). As in C, the last state of the list is not
/// looked at.
pub fn acpicpu_cst_skip(list: &mut [AcpiCstate], arat: bool) -> bool {
    let mut use_nonmwait = false;
    let n = list.len();
    for cx in list.iter_mut().take(n.saturating_sub(1)) {
        if cx.state > 1 && !arat {
            cx.flags |= CST_FLAG_SKIP;
        } else if cx.method != CST_METH_MWAIT {
            use_nonmwait = true;
        }
    }
    !use_nonmwait
}

/// `acpicpu_getcst(sc)`: re-reads the idle states from `_CST` into a new list (starting
/// from the C1 fallback); 1 when there is no `_CST` or none of its states was understood
/// (the list then holds the fallback alone), 0 otherwise. The list replaces `sc_cstates`
/// either way, as the C rebuilds it in place.
#[cfg(machine_x86)]
pub fn acpicpu_getcst(sc: &AcpicpuSoftc) -> i32 {
    let acpi = sc.acpi();
    let node = sc.node();
    let mut list = Vec::new();

    // provide a fallback C1-via-halt in case _CST's C1 is bogus
    acpicpu_add_cstate(&mut list, ACPICPU_FALLBACK_C1);

    let res = AmlValue::new();
    if aml_evalname(acpi, node.as_ref(), b"_CST", &[], Some(&res)) != 0 {
        sc.set_cstates(list);
        return 1;
    }

    let size = CPU_MWAIT_SIZE.load(Ordering::Relaxed);
    let states = CPU_MWAIT_STATES.load(Ordering::Relaxed);
    aml_foreachpkg(
        &res,
        1,
        &mut |val| match acpicpu_add_cstatepkg(val, size, states) {
            CstPkg::Skip => {}
            CstPkg::Bad(msg) => {
                printf(format_args!("{msg}"));
            }
            CstPkg::State(cx, msg) => {
                printf(format_args!("{msg}"));
                acpicpu_add_cstate(&mut list, cx);
            }
        },
    );
    aml_freevalue(Some(&res));

    // only have fallback state?  then no _CST objects were understood
    if list
        .first()
        .is_some_and(|cx| cx.flags & CST_FLAG_FALLBACK != 0)
    {
        sc.set_cstates(list);
        return 1;
    }

    // Skip states >= C2 if the CPU's LAPIC timer stops in deep states (i.e., it doesn't
    // have the 'ARAT' bit set). Also keep track if all the states we'll use use mwait.
    // SAFETY: `sc_ci` is set by the attach before it calls this, to a CPU's `cpu_info`,
    // never freed.
    let arat = unsafe { sc.sc_ci.get().as_ref() }
        .is_some_and(|ci| ci.ci_feature_tpmflags.get() & TPM_ARAT != 0);
    if acpicpu_cst_skip(&mut list, arat) {
        sc.sc_flags.fetch_or(FLAGS_MWAIT_ONLY, Ordering::Relaxed);
    } else {
        sc.sc_flags.fetch_and(!FLAGS_MWAIT_ONLY, Ordering::Relaxed);
    }
    sc.set_cstates(list);

    let res = AmlValue::new();
    if aml_evalname(acpi, node.as_ref(), b"_CSD", &[], Some(&res)) == 0 {
        aml_foreachpkg(&res, 1, &mut |val| match acpicpu_add_cdeppkg(val) {
            CsdPkg::Bogus => {
                aml_showvalue(Some(val));
                printf(format_args!("bogus CSD\n"));
            }
            CsdPkg::Quiet => {}
            CsdPkg::Dep(msg) => {
                printf(format_args!("{msg}"));
            }
        });
        aml_freevalue(Some(&res));
    }

    0
}

/// `acpicpu_getcst_from_fadt(sc)`'s states: the old-style fixed C2 and C3 of the FADT
/// (`P_LVL2_LAT`, `P_LVL3_LAT`) through the processor block's `P_LVL2`/`P_LVL3` ports, with
/// the extra restrictions the FADT puts on them.
#[allow(clippy::too_many_arguments)] // the FADT's and the processor block's values
pub fn acpicpu_fadt_cstates(
    list: &mut Vec<AcpiCstate>,
    fadt_flags: u32,
    p_lvl2_lat: u16,
    p_lvl3_lat: u16,
    ncpus: i32,
    arat: bool,
    pblk_addr: u32,
    pblk_len: i32,
) {
    // FADT has to set flag to do C2 and higher on MP
    if fadt_flags & FADT_P_LVL2_UP == 0 && ncpus > 1 {
        return;
    }

    // skip these C2 and C3 states if the CPU doesn't have ARAT
    let flags = if arat { 0 } else { CST_FLAG_SKIP };

    // Some systems don't export a full PBLK; reduce functionality
    if pblk_len >= 5 && p_lvl2_lat <= ACPI_MAX_C2_LATENCY {
        acpicpu_add_cstate(
            list,
            AcpiCstate {
                state: ACPI_STATE_C2,
                method: CST_METH_GAS_IO,
                flags,
                latency: p_lvl2_lat,
                power: -1,
                address: u64::from(pblk_addr) + 4,
            },
        );
    }
    if pblk_len >= 6 && p_lvl3_lat <= ACPI_MAX_C3_LATENCY {
        acpicpu_add_cstate(
            list,
            AcpiCstate {
                state: ACPI_STATE_C3,
                method: CST_METH_GAS_IO,
                flags,
                latency: p_lvl3_lat,
                power: -1,
                address: u64::from(pblk_addr) + 5,
            },
        );
    }
}

/// `acpicpu_getcst_from_fadt(sc)`: old-style fixed C-state info in the FADT, added to the
/// current list.
#[cfg(machine_x86)]
pub fn acpicpu_getcst_from_fadt(sc: &AcpicpuSoftc) {
    let Some(acpi) = sc.acpi() else {
        return;
    };
    let f = fadt(acpi);
    // SAFETY: as in `acpicpu_getcst`.
    let arat = unsafe { sc.sc_ci.get().as_ref() }
        .is_some_and(|ci| ci.ci_feature_tpmflags.get() & TPM_ARAT != 0);
    let mut list = sc.with_cstates(<[AcpiCstate]>::to_vec);
    acpicpu_fadt_cstates(
        &mut list,
        f.flags,
        f.p_lvl2_lat,
        f.p_lvl3_lat,
        NCPUS.load(Ordering::Relaxed),
        arat,
        sc.sc_pblk_addr.get(),
        sc.sc_pblk_len.get(),
    );
    sc.set_cstates(list);
}

/// `acpicpu_print_one_cst(cx)`: `" C1(@1 halt!)"`, `" !C3(350@100 mwait.1@0x20)"`: the
/// state (`!` when skipped), the power if known, the latency, the method, the flags (`!`
/// for the fallback) and the address when it matters.
pub fn acpicpu_print_one_cst(cx: &AcpiCstate) -> String {
    let (meth, show_addr) = match cx.method {
        CST_METH_IO_HALT => (" halt", true),
        CST_METH_HALT => (" halt", false),
        CST_METH_MWAIT => (" mwait", cx.address != 0),
        CST_METH_GAS_IO => (" io", true),
        _ => ("", false),
    };

    let mut s = format!(
        " {}C{}(",
        if cx.flags & CST_FLAG_SKIP != 0 {
            "!"
        } else {
            ""
        },
        cx.state
    );
    if cx.power != -1 {
        s.push_str(&format!("{}", cx.power));
    }
    s.push_str(&format!("@{}{}", cx.latency, meth));
    if cx.flags & !CST_FLAG_SKIP != 0 {
        if cx.flags & CST_FLAG_FALLBACK != 0 {
            s.push('!');
        } else {
            s.push_str(&format!(".{:x}", cx.flags & !CST_FLAG_SKIP));
        }
    }
    if show_addr {
        s.push_str(&format!("@0x{:x}", cx.address));
    }
    s.push(')');
    s
}

/// `acpicpu_print_cst(sc)`: `":"` and the states, separated by commas; nothing for an
/// empty list.
pub fn acpicpu_print_cst(list: &[AcpiCstate]) -> String {
    let mut s = String::new();
    if !list.is_empty() {
        s.push(':');
        for (i, cx) in list.iter().enumerate() {
            if i != 0 {
                s.push(',');
            }
            s.push_str(&acpicpu_print_one_cst(cx));
        }
    }
    s
}

/// `acpicpu_match(parent, match, aux)`: an `ACPI0007` device whose `_UID` is a CPU's ACPI
/// id (and then no `Processor()` object is attached, `sc_skip_processor`), or a
/// `Processor()` object acpi0 offers by name (`acpi_add_device`).
#[cfg(machine_x86)]
pub fn acpicpu_match(parent: Option<&Device>, match_: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: acpi0 hands its children `struct acpi_attach_args`.
    let aa = unsafe { &*aux.cast_const().cast::<AcpiAttachArgs>() };
    let cf = match_.cfdata();
    let Some(parent) = parent else {
        return 0;
    };
    // SAFETY: acpicpu attaches at acpi0 only (`acpicpu* at acpi?`), whose softc is an
    // `AcpiSoftc`.
    let acpi: &AcpiSoftc = unsafe { parent.softc() };

    if acpi_matchhids(aa, &ACPICPU_HIDS, "acpicpu") != 0
        && aa
            .aaa_node
            .as_ref()
            .and_then(|n| n.value())
            .is_some_and(|v| v.r#type() == AML_OBJTYPE_DEVICE)
    {
        // Record that we've seen a Device() CPU object, so we won't attach any
        // Processor() nodes.
        acpi.sc_skip_processor.set(1);

        // Only match if we can find a CPU with the right ID
        let mut uid = 0i64;
        let mut found = false;
        if aml_evalinteger(Some(acpi), aa.aaa_node.as_ref(), b"_UID", &[], &mut uid) == 0 {
            cpu_info_foreach(&mut |ci| {
                if i64::from(ci_acpi_proc_id(ci)) == uid {
                    found = true;
                }
            });
        }
        return i32::from(found);
    }

    // sanity
    if aaa_name(aa) != Some(cf.cf_driver.cd_name) || !aa.aaa_table.is_null() {
        return 0;
    }

    1
}

/// `acpicpu_attach(parent, self, aux)`: links the device to its CPU, tells the firmware
/// what the OS handles, reads the C-states (and installs the idle and suspend waits) and
/// the P-states, and prints what it found.
#[cfg(machine_x86)]
pub fn acpicpu_attach(parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: `acpicpu_ca` makes `AcpicpuSoftc`s, never detached: the softc lives as long as
    // the kernel.
    let sc: &'static AcpicpuSoftc = unsafe { &*ptr::from_ref(self_.softc::<AcpicpuSoftc>()) };
    // SAFETY: as in `acpicpu_match`.
    let aa = unsafe { &*aux.cast_const().cast::<AcpiAttachArgs>() };
    let Some(parent) = parent else {
        return;
    };
    // SAFETY: as in `acpicpu_match`.
    let acpi: &'static AcpiSoftc = unsafe { &*ptr::from_ref(parent.softc::<AcpiSoftc>()) };

    sc.sc_acpi.set(ptr::from_ref(acpi));
    *sc.sc_devnode.borrow_mut() = aa.aaa_node.clone();
    let node = sc.node();

    // SLIST_INIT(&sc->sc_cstates)
    mtx_init(&sc.sc_cst_mtx, IPL_NONE);
    // SAFETY: the softc is fresh and not yet linked to its CPU: nothing else reaches the
    // lists.
    unsafe {
        (*sc.sc_cstates.get()).write(Vec::new());
        (*sc.sc_pss.get()).write(RefCell::new(Vec::new()));
    }

    let mut uid = 0i64;
    if aml_evalinteger(Some(acpi), node.as_ref(), b"_UID", &[], &mut uid) == 0 {
        sc.sc_cpu.set(uid as i32);
    }

    let res = AmlValue::new();
    if aml_evalnode(Some(acpi), node.as_ref(), &[], Some(&res)) == 0 {
        if res.r#type() == AML_OBJTYPE_PROCESSOR
            && let Some(p) = res.v_processor()
        {
            sc.sc_cpu.set(i32::from(p.proc_id));
            sc.sc_pblk_addr.set(p.proc_addr);
            sc.sc_pblk_len.set(i32::from(p.proc_len));
        }
        aml_freevalue(Some(&res));
    }
    let f = fadt(acpi);
    sc.sc_duty_off.set(i32::from(f.duty_offset));
    sc.sc_duty_wid.set(i32::from(f.duty_width));

    // link in the matching cpu_info
    let mut found: Option<&'static CpuInfo> = None;
    cpu_info_foreach(&mut |ci| {
        if found.is_none() && ci_acpi_proc_id(ci) == sc.sc_cpu.get() as u32 {
            ci.ci_acpicpudev
                .store(ptr::from_ref(self_).cast_mut(), Ordering::Release);
            found = Some(ci);
        }
    });
    let Some(ci) = found else {
        printf(format_args!(
            ": no cpu matching ACPI ID {}\n",
            sc.sc_cpu.get()
        ));
        return;
    };
    sc.sc_ci.set(ptr::from_ref(ci));

    sc.sc_prev_sleep.store(1_000_000, Ordering::Relaxed);

    acpicpu_set_pdc(sc);

    if !valid_throttle(
        sc.sc_duty_off.get(),
        sc.sc_duty_wid.get(),
        sc.sc_pblk_addr.get(),
    ) {
        sc.sc_flags.fetch_or(FLAGS_NOTHROTTLE, Ordering::Relaxed);
    }

    // Get C-States from _CST or FADT
    if acpicpu_getcst(sc) != 0 || sc.with_cstates(<[AcpiCstate]>::is_empty) {
        acpicpu_getcst_from_fadt(sc);
    } else {
        // Notify BIOS we use _CST objects
        let cst_cnt = { f.cst_cnt };
        if cst_cnt != 0 {
            acpi_write_pmreg(acpi, ACPIREG_SMICMD, 0, i32::from(cst_cnt));
        }
    }
    if let Some(first) = sc.with_cstates(|l| l.first().copied()) {
        // SAFETY: while cold on the boot CPU, before any idle loop runs (the machine's
        // `cpu_idle_cycle_fcn` contract).
        unsafe {
            CPU_IDLE_CYCLE_FCN.write(acpicpu_idle);
            CPU_SUSPEND_CYCLE_FCN.write(Some(acpicpu_suspend));
        }

        // C3 (and maybe C2?) needs BM_RLD to be set to wake the system
        if first.state > 1 && ACPI_FORCE_BM.load(Ordering::Relaxed) == 0 {
            let en = acpi_read_pmreg(acpi, ACPIREG_PM1_CNT, 0) as u16;
            if en & ACPI_PM1_BM_RLD == 0 {
                acpi_write_pmreg(acpi, ACPIREG_PM1_CNT, 0, i32::from(en | ACPI_PM1_BM_RLD));
                ACPI_FORCE_BM.store(u32::from(ACPI_PM1_BM_RLD), Ordering::Relaxed);
            }
        }
    }

    if acpicpu_getpss(sc) != 0 {
        sc.sc_flags.fetch_or(FLAGS_NOPSS, Ordering::Relaxed);
    } else {
        if sc.sc_pss_len.get() == 0 {
            // this should never happen
            printf(format_args!("{}: invalid _PSS length\n", sc.xname()));
            sc.sc_flags.fetch_or(FLAGS_NOPSS, Ordering::Relaxed);
        }

        acpicpu_getppc(sc);
        if acpicpu_getpct(sc) != 0 {
            sc.sc_flags.fetch_or(FLAGS_NOPCT, Ordering::Relaxed);
        } else if sc.sc_pss_len.get() > 0 {
            // Notify BIOS we are handling p-states
            let pstate_cnt = { f.pstate_cnt };
            if pstate_cnt != 0 {
                acpi_write_pmreg(acpi, ACPIREG_SMICMD, 0, i32::from(pstate_cnt));
            }

            if let Some(node) = node.as_ref() {
                aml_register_notify(
                    node,
                    None,
                    acpicpu_notify,
                    ptr::from_ref(sc).cast_mut().cast(),
                    ACPIDEV_NOPOLL,
                );
            }

            let status = acpicpu_pct_read(sc, sc.sc_pct_stat_as.get());
            let len = sc.sc_pss_len.get();
            sc.sc_level.set((100 / len) * (len - status as i32));
            if SETPERF_PRIO.load(Ordering::Relaxed) < 30 {
                // SAFETY: while cold on the boot CPU, before anything calls it.
                unsafe { CPU_SETPERF.write(Some(acpicpu_setperf)) };
                acpicpu_set_notify(acpicpu_setperf_ppc_change);
                SETPERF_PRIO.store(30, Ordering::Relaxed);
                ACPI_HASPROCFVS.store(1, Ordering::Relaxed);
            }
        }
    }

    // Nicely enumerate what power management capabilities ACPI CPU provides.
    let (line, empty) = sc.with_cstates(|l| (acpicpu_print_cst(l), l.is_empty()));
    printf(format_args!("{line}"));
    let flags = sc.sc_flags.load(Ordering::Relaxed);
    if flags & FLAGS_NOPSS == 0 {
        printf(format_args!("{} ", if empty { ':' } else { ',' }));

        // If acpicpu is itself providing the capability to transition states, enumerate
        // them in the fashion that est and powernow would.
        if flags & (FLAGS_NOPSS | FLAGS_NOPCT) == 0 {
            printf(format_args!("{}", acpicpu_fvs(&sc.pss().borrow())));
        } else {
            printf(format_args!("PSS"));
        }
    }

    printf(format_args!("\n"));
}

/// `"FVS, 2400, 1800, 800 MHz"`: the frequencies of the performance states, as est and
/// powernow print them.
pub fn acpicpu_fvs(pss: &[AcpicpuPss]) -> String {
    let mut s = String::from("FVS, ");
    if let Some((last, rest)) = pss.split_last() {
        for p in rest {
            s.push_str(&format!("{}, ", p.pss_core_freq));
        }
        s.push_str(&format!("{} MHz", last.pss_core_freq));
    }
    s
}

/// `acpicpu_getppc(sc)`: the highest performance state the platform allows now (`_PPC`); 1
/// when there is none (0 is then assumed).
#[cfg(machine_x86)]
pub fn acpicpu_getppc(sc: &AcpicpuSoftc) -> i32 {
    sc.sc_ppc.set(0);

    let res = AmlValue::new();
    if aml_evalname(sc.acpi(), sc.node().as_ref(), b"_PPC", &[], Some(&res)) != 0 {
        // dnprintf: no _PPC
        return 1;
    }

    sc.sc_ppc.set(aml_val2int(Some(&res)) as i32);
    aml_freevalue(Some(&res));

    0
}

/// `struct acpi_grd` from a `_PCT` buffer, zero-filled past a short buffer.
pub fn acpicpu_grd(buf: &[u8]) -> AcpiGrd {
    let mut b = [0u8; size_of::<AcpiGrd>()];
    let n = buf.len().min(b.len());
    b[..n].copy_from_slice(&buf[..n]);
    // SAFETY: `AcpiGrd` is a packed structure of integers (any bytes are a value) exactly
    // `b.len()` bytes long; it is read unaligned.
    unsafe { ptr::read_unaligned(b.as_ptr().cast::<AcpiGrd>()) }
}

/// The access widths and lengths `acpicpu_getpct` derives from the `_PCT` registers:
/// `(stat_as, ctrl_as, stat_len, ctrl_len)`; a register width of 0 is a single 32-bit
/// access, an access size of 0 the access width.
pub fn acpicpu_pct_sizes(pct: &AcpicpuPct) -> (u32, u32, u32, u32) {
    let (stat, ctrl) = (pct.pct_status.grd_gas, pct.pct_ctrl.grd_gas);
    // if not set assume single 32 bit access
    let stat_as = match u32::from(stat.register_bit_width) / 8 {
        0 => 4,
        n => n,
    };
    let ctrl_as = match u32::from(ctrl.register_bit_width) / 8 {
        0 => 4,
        n => n,
    };
    let stat_len = match u32::from(stat.access_size) {
        0 => stat_as,
        n => n,
    };
    let ctrl_len = match u32::from(ctrl.access_size) {
        0 => ctrl_as,
        n => n,
    };
    (stat_as, ctrl_as, stat_len, ctrl_len)
}

/// `acpicpu_getpct(sc)`: the performance control and status registers (`_PCT`); 1 when
/// there are none or they are functional fixed hardware (MSRs, which est and powernow drive).
#[cfg(machine_x86)]
pub fn acpicpu_getpct(sc: &AcpicpuSoftc) -> i32 {
    let res = AmlValue::new();
    if aml_evalname(sc.acpi(), sc.node().as_ref(), b"_PCT", &[], Some(&res)) != 0 {
        // dnprintf: no _PCT
        return 1;
    }

    if res.length() != 2 {
        // dnprintf: invalid _PCT length
        return 1;
    }

    let mut pct = sc.sc_pct.get();
    pct.pct_ctrl = acpicpu_grd(&res.v_package(0).map(|v| v.v_buffer()).unwrap_or_default());
    let rv = if i32::from(pct.pct_ctrl.grd_gas.address_space_id) == GAS_FUNCTIONAL_FIXED {
        // dnprintf: CTRL GASIO is functional fixed hardware.
        1
    } else {
        pct.pct_status = acpicpu_grd(&res.v_package(1).map(|v| v.v_buffer()).unwrap_or_default());
        if i32::from(pct.pct_status.grd_gas.address_space_id) == GAS_FUNCTIONAL_FIXED {
            // dnprintf: CTRL GASIO is functional fixed hardware.
            1
        } else {
            let (stat_as, ctrl_as, stat_len, ctrl_len) = acpicpu_pct_sizes(&pct);
            sc.sc_pct_stat_as.set(stat_as);
            sc.sc_pct_ctrl_as.set(ctrl_as);
            sc.sc_pct_stat_len.set(stat_len);
            sc.sc_pct_ctrl_len.set(ctrl_len);
            0
        }
    };
    sc.sc_pct.set(pct);

    aml_freevalue(Some(&res));
    rv
}

/// What `acpicpu_getpss` keeps of `_PSS` (each entry `{ frequency, power, transition
/// latency, bus master latency, control, status }`), and the lines it prints for the
/// entries it strikes. This heuristic comes from FreeBSD's `dev/acpica/acpi_perf.c`, to
/// weed out invalid entries; as in C, a frequency is compared with the zeroed slot it would
/// fill, so "equals last" catches a 0.
pub fn acpicpu_parse_pss(res: &AmlValue, xname: &str) -> (Vec<AcpicpuPss>, Vec<String>) {
    let n = res.length().max(0) as usize;
    let zero = AcpicpuPss {
        pss_core_freq: 0,
        pss_power: 0,
        pss_trans_latency: 0,
        pss_bus_latency: 0,
        pss_ctrl: 0,
        pss_status: 0,
    };
    let mut pss = alloc::vec![zero; n];
    let mut said = Vec::new();
    let mut c = 0;
    for i in 0..n {
        let e = res.v_package(i);
        let field = |k: usize| aml_val2int(e.as_ref().and_then(|e| e.v_package(k)).as_deref());
        let cf = field(0) as i32;

        if cf == pss[c].pss_core_freq as i32 {
            said.push(format!(
                "{xname}: struck PSS entry, core frequency equals  last\n"
            ));
            continue;
        }

        if cf == 0xFFFF || cf == 0x9999 || cf == 99999 || cf == 0 {
            said.push(format!(
                "{xname}: struck PSS entry, inappropriate core frequency value\n"
            ));
            continue;
        }

        pss[c] = AcpicpuPss {
            pss_core_freq: cf as u32,
            pss_power: field(1) as u32,
            pss_trans_latency: field(2) as u32,
            pss_bus_latency: field(3) as u32,
            pss_ctrl: field(4) as u32,
            pss_status: field(5) as u32,
        };
        c += 1;
    }
    pss.truncate(c);
    (pss, said)
}

/// `acpicpu_getpss(sc)`: the performance states (`_PSS`); 1 when there are none.
#[cfg(machine_x86)]
pub fn acpicpu_getpss(sc: &AcpicpuSoftc) -> i32 {
    let res = AmlValue::new();
    if aml_evalname(sc.acpi(), sc.node().as_ref(), b"_PSS", &[], Some(&res)) != 0 {
        // dprintf: no _PSS
        return 1;
    }

    let (pss, said) = acpicpu_parse_pss(&res, sc.xname());
    for line in &said {
        printf(format_args!("{line}"));
    }
    sc.sc_pss_len.set(pss.len() as i32);
    *sc.pss().borrow_mut() = pss;

    aml_freevalue(Some(&res));

    0
}

/// `acpicpu_fetch_pss(&pss)`: the primary CPU's performance states, for est and powernow.
/// According to the ACPI spec all processors of an SMP system support the same states; for
/// now we pray the bios ensures this...
#[cfg(machine_x86)]
pub fn acpicpu_fetch_pss() -> Vec<AcpicpuPss> {
    let Some(sc) = acpicpu_of(cpu_info_primary()) else {
        return Vec::new();
    };
    sc.pss().borrow().clone()
}

/// `acpicpu_notify(node, notify_type, arg)`: `_PPC` changed (0x80): re-read the performance
/// states and tell the listener; `_CST` changed (0x81): re-read and print the idle states.
#[cfg(machine_x86)]
pub fn acpicpu_notify(_node: &AmlNodeRef, notify_type: i32, arg: *mut c_void) -> i32 {
    // SAFETY: `arg` is the softc `acpicpu_attach` registered, never freed.
    let Some(sc) = (unsafe { arg.cast_const().cast::<AcpicpuSoftc>().as_ref() }) else {
        return 0;
    };

    // dnprintf: "acpicpu_notify: %.2x %s\n"

    match notify_type {
        0x80 => {
            // _PPC changed, retrieve new values
            acpicpu_getppc(sc);
            acpicpu_getpss(sc);
            if let Some(f) = sc.sc_notify.get() {
                let pss = sc.pss().borrow().clone();
                f(&pss, sc.sc_pss_len.get());
            }
        }
        0x81 => {
            // _CST changed, retrieve new values
            acpicpu_getcst(sc);
            let line = sc.with_cstates(acpicpu_print_cst);
            printf(format_args!("{}: notify{}\n", sc.xname(), line));
        }
        _ => {
            printf(format_args!(
                "{}: unhandled cpu event {:x}\n",
                sc.xname(),
                notify_type
            ));
        }
    }

    0
}

/// `acpicpu_set_notify(func)`: installs the primary CPU's `_PPC` change listener.
#[cfg(machine_x86)]
pub fn acpicpu_set_notify(func: AcpicpuNotifyFn) {
    if let Some(sc) = acpicpu_of(cpu_info_primary()) {
        sc.sc_notify.set(Some(func));
    }
}

/// `acpicpu_setperf_ppc_change(pss, npss)`: re-applies the primary CPU's level after a
/// `_PPC` change.
#[cfg(machine_x86)]
pub fn acpicpu_setperf_ppc_change(_pss: &[AcpicpuPss], _npss: i32) {
    if let Some(sc) = acpicpu_of(cpu_info_primary()) {
        // SAFETY: written while cold by the attach; this runs from the acpi thread.
        if let Some(setperf) = unsafe { CPU_SETPERF.read() } {
            setperf(sc.sc_level.get());
        }
    }
}

/// `acpicpu_setperf`'s index: the performance state for `level` percent among `pss_len`
/// states of which the first `pss_len - ppc` are forbidden when `ppc` is set (see the
/// module's deviations for the clamps).
pub fn acpicpu_setperf_idx(level: i32, pss_len: i32, ppc: i32) -> usize {
    let len = if ppc != 0 { ppc } else { pss_len };
    if len <= 0 || 100 / len == 0 {
        return 0;
    }
    let mut idx = (len - 1) - (level / (100 / len));
    if idx < 0 {
        idx = 0;
    }

    if ppc != 0 {
        idx += pss_len - ppc;
    }

    if idx >= pss_len {
        idx = pss_len - 1;
    }
    idx.max(0) as usize
}

/// Reads the `_PCT` status register, `len` bytes (at most 4).
#[cfg(machine_x86)]
fn acpicpu_pct_read(sc: &AcpicpuSoftc, len: u32) -> u32 {
    let pct = sc.sc_pct.get();
    let gas = pct.pct_status.grd_gas;
    let mut buf = [0u8; 4];
    acpi_gasio(
        sc.acpi(),
        ACPI_IOREAD,
        i32::from(gas.address_space_id),
        gas.address,
        sc.sc_pct_stat_as.get() as i32,
        len.min(4) as i32,
        &mut buf,
    );
    u32::from_le_bytes(buf)
}

/// `acpicpu_setperf(level)`: moves this CPU to the performance state for `level` percent
/// through the `_PCT` control register and checks the status register.
#[cfg(machine_x86)]
pub fn acpicpu_setperf(level: i32) {
    let Some(sc) = acpicpu_of(curcpu()) else {
        return;
    };

    // dnprintf: "%s: acpicpu setperf level %d\n"

    if !(0..=100).contains(&level) {
        // dnprintf: acpicpu setperf illegal percentage
        return;
    }

    // XXX this should be handled more gracefully and it needs to also do the duty cycle
    // method instead of pss exclusively
    let flags = sc.sc_flags.load(Ordering::Relaxed);
    if flags & FLAGS_NOPSS != 0 || flags & FLAGS_NOPCT != 0 {
        // dnprintf: acpicpu no _PSS or _PCT
        return;
    }

    let idx = acpicpu_setperf_idx(level, sc.sc_pss_len.get(), sc.sc_ppc.get());

    // dnprintf: "%s: acpicpu setperf index %d pss_len %d ppc %d\n"

    let Some(pss) = sc.pss().borrow().get(idx).copied() else {
        return;
    };

    let status = acpicpu_pct_read(sc, sc.sc_pct_stat_len.get());

    // Are we already at the requested frequency?
    if status == pss.pss_status {
        return;
    }

    let pct = sc.sc_pct.get();
    let ctrl = pct.pct_ctrl.grd_gas;
    let mut buf = pss.pss_ctrl.to_le_bytes();
    acpi_gasio(
        sc.acpi(),
        ACPI_IOWRITE,
        i32::from(ctrl.address_space_id),
        ctrl.address,
        sc.sc_pct_ctrl_as.get() as i32,
        sc.sc_pct_ctrl_len.get().min(4) as i32,
        &mut buf,
    );

    let status = acpicpu_pct_read(sc, sc.sc_pct_stat_as.get());

    // Did the transition succeed?
    if status == pss.pss_status {
        CPUSPEED.store(pss.pss_core_freq as i32, Ordering::Relaxed);
        sc.sc_level.set(level);
    } else {
        printf(format_args!(
            "{}: acpicpu setperf failed to alter frequency\n",
            Str(cstr(&sc.node_name()))
        ));
    }
}

/// The idle loop's choice (`acpicpu_idle`): the index of the first state that is not
/// skipped and whose latency fits three times into `prev_sleep`, or of the last state.
pub fn acpicpu_choose(list: &[AcpiCstate], prev_sleep: u64) -> Option<usize> {
    if list.is_empty() {
        return None;
    }
    let mut best = 0;
    let mut cx = 0;
    while list[cx].flags & CST_FLAG_SKIP != 0 || u64::from(list[cx].latency) * 3 > prev_sleep {
        cx += 1;
        if cx == list.len() {
            break;
        }
        best = cx;
    }
    Some(best)
}

/// Whether the chosen state needs the bus-master check: C3 or deeper with
/// `CST_FLAG_MWAIT_BM_AVOIDANCE`.
pub fn acpicpu_bm_check(cx: &AcpiCstate) -> bool {
    cx.state >= 3 && cx.flags & CST_FLAG_MWAIT_BM_AVOIDANCE != 0
}

/// The bus-master back-off: from the chosen state `from`, the next state that is not
/// skipped and is shallower than C3 or needs no bus-master avoidance; the last state when
/// none is (see the module's deviations).
pub fn acpicpu_bm_backoff(list: &[AcpiCstate], from: usize) -> usize {
    list.iter()
        .enumerate()
        .skip(from + 1)
        .filter(|(_, cx)| cx.flags & CST_FLAG_SKIP == 0)
        .find(|(_, cx)| cx.state < 3 || cx.flags & CST_FLAG_MWAIT_BM_AVOIDANCE == 0)
        .map_or(list.len().saturating_sub(1), |(i, _)| i)
}

/// The suspend loop's choice (`acpicpu_suspend`): the first state that is not skipped, or
/// the last one.
pub fn acpicpu_suspend_choose(list: &[AcpiCstate]) -> Option<usize> {
    if list.is_empty() {
        return None;
    }
    Some(
        list.iter()
            .position(|cx| cx.flags & CST_FLAG_SKIP == 0)
            .unwrap_or(list.len() - 1),
    )
}

/// `sc_prev_sleep`'s update after an idle of `itime` microseconds: the average of the old
/// value weighted 3/4 and half the idle time.
pub fn acpicpu_prev_sleep(prev_sleep: u64, itime: u64) -> u64 {
    let itime = itime >> 1;
    (prev_sleep + (prev_sleep >> 1) + itime) >> 1
}

/// `ci->ci_acpicpudev` as the softc it is.
#[cfg(machine_x86)]
fn acpicpu_of(ci: &CpuInfo) -> Option<&'static AcpicpuSoftc> {
    // SAFETY: null, or an acpicpu device `acpicpu_attach` linked, never detached.
    let dev = unsafe { ci.ci_acpicpudev.load(Ordering::Acquire).as_ref() }?;
    // SAFETY: only acpicpu devices are stored there; their softc is an `AcpicpuSoftc`.
    Some(unsafe { &*ptr::from_ref(dev.softc::<AcpicpuSoftc>()) })
}

/// `sti` and `panic("null acpicpu")`: the idle or suspend wait installed on a CPU acpicpu
/// did not attach to.
#[cfg(machine_x86)]
fn acpicpu_null() -> ! {
    // SAFETY: about to panic; interrupts on, as the C's `sti`, for the panic path.
    unsafe { intr_enable() };
    panic(format_args!("null acpicpu"));
}

/// The intel errata AAI65 workaround: `clflush` before `monitor` on Intel CPUs with
/// `clflush`.
#[cfg(machine_x86)]
fn acpicpu_aai65(ci: &CpuInfo, addr: *const u8) {
    // SAFETY: `cpu_vendor` is written once by `init_x86_64` before anything else runs.
    let vendor = unsafe { CPU_VENDOR.get() };
    if ci.ci_cflushsz.get() != 0 && vendor.starts_with(b"GenuineIntel\0") {
        core::sync::atomic::fence(Ordering::SeqCst); // membar_sync
        clflush(addr as u64);
        core::sync::atomic::fence(Ordering::SeqCst); // membar_sync
    }
}

/// `acpicpu_idle()`: `cpu_idle_cycle_fcn`: waits in the deepest acceptable idle state and
/// updates the sleep length average.
#[cfg(machine_x86)]
pub fn acpicpu_idle() {
    let ci = curcpu();
    let Some(sc) = acpicpu_of(ci) else {
        acpicpu_null();
    };

    // possibly update the MWAIT_ONLY flag in cpu_info
    if sc.sc_flags.load(Ordering::Relaxed) & FLAGS_MWAIT_ONLY != 0 {
        if ci.ci_mwait.load(Ordering::Relaxed) & MWAIT_ONLY == 0 {
            ci.ci_mwait.fetch_or(MWAIT_ONLY, Ordering::SeqCst);
        }
    } else if ci.ci_mwait.load(Ordering::Relaxed) & MWAIT_ONLY != 0 {
        ci.ci_mwait.fetch_and(!MWAIT_ONLY, Ordering::SeqCst);
    }

    // Find the first state with a latency we'll accept, ignoring states marked skippable
    let prev_sleep = sc.sc_prev_sleep.load(Ordering::Relaxed);
    let best = sc.with_cstates(|list| {
        let mut best = acpicpu_choose(list, prev_sleep)?;
        if acpicpu_bm_check(&list[best])
            && let Some(acpi) = acpi_softc()
            && acpi_read_pmreg(acpi, ACPIREG_PM1_STS, 0) & i32::from(ACPI_PM1_BM_STS) != 0
        {
            // clear it and back off
            acpi_write_pmreg(acpi, ACPIREG_PM1_STS, 0, i32::from(ACPI_PM1_BM_STS));
            best = acpicpu_bm_backoff(list, best);
        }
        Some(list[best])
    });
    let Some(best) = best else {
        cpu_idle_cycle_hlt();
        return;
    };

    if let Some(n) = CST_STATS.get(usize::from(best.state)) {
        n.fetch_add(1, Ordering::Relaxed);
    }

    let mut itime = (crate::conf::param::TICK.load(Ordering::Relaxed) / 2) as u64;
    match best.method {
        CST_METH_IO_HALT => {
            // SAFETY: the state's port from `_CST`, which the firmware says to read to
            // enter it.
            let _ = unsafe { inb(best.address as u16) };
            cpu_idle_cycle_hlt();
        }
        CST_METH_MWAIT => {
            if read_rflags() & PSL_I == 0 {
                panic(format_args!("idle with interrupts blocked!"));
            }

            // something already queued?
            if !cpu_is_idle(ci) {
                return;
            }

            // About to idle; setting the MWAIT_IN_IDLE bit tells cpu_unidle() that it
            // can't be a no-op and tells cpu_kick() that it doesn't need to use an IPI. We
            // also set the MWAIT_KEEP_IDLING bit: those routines clear it to stop the mwait.
            // Once they're set, we do a final check of the queue, in case another cpu
            // called setrunqueue() and added something to the queue and called
            // cpu_unidle() between the check in sched_idle() and here.
            let hints = best.address as u32;
            let start = microuptime();
            ci.ci_mwait.fetch_or(MWAIT_IDLING, Ordering::SeqCst);
            if cpu_is_idle(ci) {
                // intel errata AAI65: cflush before monitor
                acpicpu_aai65(ci, ci.ci_mwait.as_ptr().cast());

                monitor(ci.ci_mwait.as_ptr(), 0, 0);
                if ci.ci_mwait.load(Ordering::SeqCst) & MWAIT_IDLING == MWAIT_IDLING {
                    mwait(0, hints);
                }
            }

            let stop = microuptime();
            let usecs = (stop.tv_sec - start.tv_sec) * 1_000_000 + stop.tv_usec as i64
                - start.tv_usec as i64;
            itime = usecs.max(0) as u64;

            // done idling; let cpu_kick() know that an IPI is required
            ci.ci_mwait.fetch_and(!MWAIT_IDLING, Ordering::SeqCst);
        }
        CST_METH_GAS_IO => {
            // SAFETY: as for `CST_METH_IO_HALT`: the port that enters the state.
            let _ = unsafe { inb(best.address as u16) };
            // something harmless to give system time to change state
            if let Some(acpi) = acpi_softc() {
                acpi_read_pmreg(acpi, ACPIREG_PM1_STS, 0);
            }
        }
        _ => cpu_idle_cycle_hlt(), // CST_METH_HALT and default
    }

    sc.sc_last_itime.store(itime, Ordering::Relaxed);
    sc.sc_prev_sleep
        .store(acpicpu_prev_sleep(prev_sleep, itime), Ordering::Relaxed);
}

/// `acpicpu_suspend()`: `cpu_suspend_cycle_fcn`: waits in the lowest usable idle state
/// (`x86_64_ipi_halt`'s CPUs); `mwait` on `cpu_suspended`, unless this is the primary CPU
/// and the machine is not suspended.
#[cfg(machine_x86)]
pub fn acpicpu_suspend() {
    let ci = curcpu();
    let Some(sc) = acpicpu_of(ci) else {
        acpicpu_null();
    };

    // Find the lowest usable state.
    let Some(best) = sc.with_cstates(|list| acpicpu_suspend_choose(list).map(|i| list[i])) else {
        cpu_idle_cycle_hlt();
        return;
    };

    match best.method {
        CST_METH_IO_HALT => {
            // SAFETY: as in `acpicpu_idle`.
            let _ = unsafe { inb(best.address as u16) };
            cpu_idle_cycle_hlt();
        }
        CST_METH_MWAIT => {
            let hints = best.address as u32;
            let suspended = cpu_suspended();
            // intel errata AAI65: cflush before monitor
            acpicpu_aai65(ci, suspended.as_ptr().cast());

            monitor(suspended.as_ptr().cast(), 0, 0);
            if suspended.load(Ordering::SeqCst) != 0 || !cpu_is_primary(ci) {
                mwait(0, hints);
            }
        }
        CST_METH_GAS_IO => {
            // SAFETY: as in `acpicpu_idle`.
            let _ = unsafe { inb(best.address as u16) };
            // something harmless to give system time to change state
            if let Some(acpi) = acpi_softc() {
                acpi_read_pmreg(acpi, ACPIREG_PM1_STS, 0);
            }
        }
        _ => cpu_idle_cycle_hlt(), // CST_METH_HALT and default
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;
    use crate::dev::acpi::amltypes::{AmlObj, AmlValueRef};
    use alloc::rc::Rc;
    use alloc::vec;

    fn int(x: i64) -> AmlValueRef {
        Rc::new(AmlValue::integer(x))
    }

    fn pkg(v: Vec<AmlValueRef>) -> AmlValue {
        AmlValue::from_obj(AmlObj::Package(v))
    }

    /// A `ResourceTemplate () { Register (space, width, offset, address, access) }`.
    fn grd(space: u8, width: u8, offset: u8, address: u64, access: u8) -> AmlValueRef {
        let mut b = vec![0x82, 12, 0, space, width, offset, access];
        b.extend_from_slice(&address.to_le_bytes());
        b.extend_from_slice(&[0x79, 0]);
        Rc::new(AmlValue::buffer(&b))
    }

    fn cst(reg: AmlValueRef, state: i64, latency: i64, power: i64) -> AmlValue {
        pkg(vec![reg, int(state), int(latency), int(power)])
    }

    fn cx(state: u16, method: i16, flags: u16, latency: u16) -> AcpiCstate {
        AcpiCstate {
            state,
            method,
            flags,
            latency,
            power: -1,
            address: 0,
        }
    }

    #[test]
    fn throttle_and_constants() {
        assert!(!valid_throttle(0, 0, 0)); // QEMU: no processor block, no duty cycle
        assert!(valid_throttle(1, 3, 0x410));
        assert!(!valid_throttle(1, 4, 0x410)); // crosses bit 4, the THT_EN bit
        assert!(valid_throttle(5, 3, 0x410));
        assert!(!valid_throttle(1, 3, 0));
        assert_eq!(cpu_maxstate(3), 8);
        assert_eq!(ACPICPU_PDC_CAP, 0x033b);
        assert_eq!(
            acpicpu_pdc_words(&[ACPI_PDC_REVID, 1, ACPICPU_PDC_CAP]),
            [1, 0, 0, 0, 1, 0, 0, 0, 0x3b, 0x03, 0, 0]
        );
        assert_eq!(CPU_OSCUUID[..4], [0x16, 0xa6, 0x77, 0x40]); // 4077A616-...
    }

    #[test]
    fn add_cstate_overwrites_the_fallback_c1() {
        let mut l = Vec::new();
        acpicpu_add_cstate(&mut l, ACPICPU_FALLBACK_C1);
        assert_eq!(l, [ACPICPU_FALLBACK_C1]);
        // _CST's C1 replaces the fallback; C2 and C3 go in front.
        acpicpu_add_cstate(&mut l, cx(1, CST_METH_MWAIT, 1, 1));
        acpicpu_add_cstate(&mut l, cx(2, CST_METH_MWAIT, 1, 50));
        acpicpu_add_cstate(&mut l, cx(3, CST_METH_MWAIT, 3, 100));
        assert_eq!(l.iter().map(|c| c.state).collect::<Vec<_>>(), [3, 2, 1]);
        // A second C1 is a new entry once the head is not the fallback.
        acpicpu_add_cstate(&mut l, cx(1, CST_METH_HALT, 0, 1));
        assert_eq!(l.len(), 4);
    }

    #[test]
    fn cst_packages() {
        // FFH, width 0: hlt.
        let p = cst(grd(0x7f, 0, 0, 0, 0), 1, 1, 1000);
        assert_eq!(
            acpicpu_add_cstatepkg(&p, 0, 0),
            CstPkg::State(
                AcpiCstate {
                    state: 1,
                    method: CST_METH_HALT,
                    flags: 0,
                    latency: 1,
                    power: 1000,
                    address: 0
                },
                String::new()
            )
        );
        // FFH class 1: inb then hlt.
        let p = cst(grd(0x7f, 8, 1, 0x415, 0), 2, 50, 500);
        assert!(matches!(
            acpicpu_add_cstatepkg(&p, 0, 0),
            CstPkg::State(
                AcpiCstate {
                    method: CST_METH_IO_HALT,
                    address: 0x415,
                    ..
                },
                _
            )
        ));
        let p = cst(grd(0x7f, 8, 1, 0x1_0000, 0), 2, 50, 500);
        assert_eq!(
            acpicpu_add_cstatepkg(&p, 0, 0),
            CstPkg::Bad(String::from(": C2 (bogo I/O addr 10000)"))
        );
        // FFH class 2: mwait hint 0x20 (C3), access size 1 = HW coordinated.
        let p = cst(grd(0x7f, 1, 2, 0x20, 1), 3, 100, 350);
        assert_eq!(acpicpu_add_cstatepkg(&p, 0, 0), CstPkg::Skip); // no mwait
        assert_eq!(
            acpicpu_add_cstatepkg(&p, 64, 0x0000_0120),
            CstPkg::Bad(String::from(": C3 bad (state 3 has no substates)"))
        );
        assert_eq!(
            acpicpu_add_cstatepkg(&p, 64, 0x0000_2120),
            CstPkg::State(
                AcpiCstate {
                    state: 3,
                    method: CST_METH_MWAIT,
                    flags: 1,
                    latency: 100,
                    power: 350,
                    address: 0x20
                },
                String::new()
            )
        );
        let p = cst(grd(0x7f, 1, 3, 0, 0), 2, 1, 1);
        assert_eq!(
            acpicpu_add_cstatepkg(&p, 0, 0),
            CstPkg::Bad(String::from(": C2 (unknown FFH class 3)"))
        );
        // System I/O: one byte at offset 0, else refused.
        let p = cst(grd(1, 8, 0, 0x414, 0), 2, 100, 500);
        assert!(matches!(
            acpicpu_add_cstatepkg(&p, 0, 0),
            CstPkg::State(
                AcpiCstate {
                    method: CST_METH_GAS_IO,
                    address: 0x414,
                    ..
                },
                _
            )
        ));
        let p = cst(grd(1, 16, 0, 0x414, 0), 2, 100, 500);
        assert_eq!(
            acpicpu_add_cstatepkg(&p, 0, 0),
            CstPkg::Bad(String::from(": C2 (unhandled I/O spec: 16/0)"))
        );
        // Memory space: the GAS dumped, with C's %#x (0 is "0").
        let p = cst(grd(0, 8, 0, 0x1000, 1), 2, 100, 500);
        assert_eq!(
            acpicpu_add_cstatepkg(&p, 0, 0),
            CstPkg::Bad(String::from(
                ": C2 (unhandled GAS: 0 0x8 0 0x1 0 0x10 0 0 0 0 0 0)"
            ))
        );
        // Not a buffer, a bad buffer, a state out of range, a short package.
        let p = cst(int(0), 2, 1, 1);
        assert_eq!(
            acpicpu_add_cstatepkg(&p, 0, 0),
            CstPkg::Bad(String::from(": C2 (unexpected ACPI object type 1)"))
        );
        let p = cst(Rc::new(AmlValue::buffer(&[0x82, 12, 0])), 2, 1, 1);
        assert_eq!(
            acpicpu_add_cstatepkg(&p, 0, 0),
            CstPkg::Bad(String::from(": C2 (bogo buffer)"))
        );
        assert_eq!(
            acpicpu_add_cstatepkg(&cst(grd(0x7f, 0, 0, 0, 0), 5, 1, 1), 0, 0),
            CstPkg::Skip
        );
        assert_eq!(
            acpicpu_add_cstatepkg(&pkg(vec![int(1), int(2)]), 0, 0),
            CstPkg::Skip
        );
    }

    #[test]
    fn mwait_hints() {
        // Hint 0x10 is C2 (cpuid 5's nibble 2), 0xf0 wraps to C0, 0x70 is beyond the test.
        assert!(check_mwait_hints(2, 0x10, 64, 0x0000_0220).0);
        assert!(!check_mwait_hints(2, 0x10, 64, 0x0000_0020).0);
        assert!(check_mwait_hints(1, 0xf0, 64, 0x0000_0001).0);
        assert!(check_mwait_hints(7, 0x70, 64, 0).0);
        assert_eq!(
            check_mwait_hints(2, 0x10, 0, 0x0000_0220),
            (false, String::new())
        );
    }

    #[test]
    fn csd_packages() {
        let csd = |v: [i64; 6]| pkg(v.iter().map(|&x| int(x)).collect());
        assert_eq!(
            acpicpu_add_cdeppkg(&csd([6, 0, 0, 0xfe, 4, 0])),
            CsdPkg::Quiet
        );
        assert_eq!(
            acpicpu_add_cdeppkg(&csd([6, 0, 0, 0xfc, 1, 0])),
            CsdPkg::Quiet
        );
        assert_eq!(
            acpicpu_add_cdeppkg(&csd([6, 0, 3, 0xfd, 2, 1])),
            CsdPkg::Dep(String::from(": CSD (c=0xfd d=3 n=2 i=1)"))
        );
        assert_eq!(
            acpicpu_add_cdeppkg(&csd([5, 0, 0, 0xfe, 4, 0])),
            CsdPkg::Bogus
        );
        assert_eq!(
            acpicpu_add_cdeppkg(&csd([6, 1, 0, 0xfe, 4, 0])),
            CsdPkg::Bogus
        );
        assert_eq!(acpicpu_add_cdeppkg(&AmlValue::integer(6)), CsdPkg::Bogus);
    }

    #[test]
    fn deep_states_need_arat() {
        let mut l = vec![
            cx(3, CST_METH_MWAIT, 1, 100),
            cx(2, CST_METH_MWAIT, 1, 50),
            cx(1, CST_METH_HALT, 0, 1),
        ];
        // No ARAT: C3 and C2 skipped; the last (C1) is not looked at, so mwait only.
        assert!(acpicpu_cst_skip(&mut l, false));
        assert_eq!(l[0].flags, 1 | CST_FLAG_SKIP);
        assert_eq!(l[1].flags, 1 | CST_FLAG_SKIP);
        assert_eq!(l[2].flags, 0);
        let mut l = vec![
            cx(3, CST_METH_MWAIT, 1, 100),
            cx(2, CST_METH_GAS_IO, 0, 50),
            cx(1, CST_METH_HALT, 0, 1),
        ];
        assert!(!acpicpu_cst_skip(&mut l, true));
        assert_eq!(l[0].flags, 1);
    }

    #[test]
    fn fadt_states() {
        let mut l = vec![ACPICPU_FALLBACK_C1];
        // MP without P_LVL2_UP: nothing (QEMU's FADT).
        acpicpu_fadt_cstates(&mut l, 0, 0xfff, 0xfff, 2, false, 0x410, 6);
        assert_eq!(l.len(), 1);
        acpicpu_fadt_cstates(&mut l, FADT_P_LVL2_UP, 50, 500, 2, false, 0x410, 6);
        assert_eq!(l.len(), 3);
        assert_eq!((l[0].state, l[0].address), (3, 0x415));
        assert_eq!((l[1].state, l[1].address), (2, 0x414));
        assert_eq!(l[1].flags, CST_FLAG_SKIP);
        // A latency over the limit, or a short processor block.
        let mut l = Vec::new();
        acpicpu_fadt_cstates(&mut l, 0, 101, 500, 1, true, 0x410, 6);
        assert_eq!(l.iter().map(|c| c.state).collect::<Vec<_>>(), [3]);
        let mut l = Vec::new();
        acpicpu_fadt_cstates(&mut l, 0, 50, 500, 1, true, 0x410, 4);
        assert!(l.is_empty());
    }

    #[test]
    fn printing() {
        // What QEMU's CPUs get: the fallback alone.
        assert_eq!(acpicpu_print_cst(&[ACPICPU_FALLBACK_C1]), ": C1(@1 halt!)");
        assert_eq!(acpicpu_print_cst(&[]), "");
        let l = [
            AcpiCstate {
                state: 3,
                method: CST_METH_MWAIT,
                flags: 1 | CST_FLAG_SKIP,
                latency: 100,
                power: 350,
                address: 0x20,
            },
            AcpiCstate {
                state: 2,
                method: CST_METH_GAS_IO,
                flags: 0,
                latency: 50,
                power: -1,
                address: 0x414,
            },
            AcpiCstate {
                state: 1,
                method: CST_METH_IO_HALT,
                flags: 0,
                latency: 1,
                power: 1000,
                address: 0x415,
            },
        ];
        assert_eq!(
            acpicpu_print_cst(&l),
            ": !C3(350@100 mwait.1@0x20), C2(@50 io@0x414), C1(1000@1 halt@0x415)"
        );
        let pss = |f| AcpicpuPss {
            pss_core_freq: f,
            pss_power: 0,
            pss_trans_latency: 0,
            pss_bus_latency: 0,
            pss_ctrl: 0,
            pss_status: 0,
        };
        assert_eq!(
            acpicpu_fvs(&[pss(2400), pss(1800), pss(800)]),
            "FVS, 2400, 1800, 800 MHz"
        );
    }

    #[test]
    fn idle_choice() {
        let l = [
            cx(3, CST_METH_MWAIT, CST_FLAG_MWAIT_BM_AVOIDANCE, 100),
            cx(2, CST_METH_MWAIT, CST_FLAG_SKIP, 50),
            cx(1, CST_METH_HALT, 0, 1),
        ];
        // Long sleeps take C3; shorter ones skip C3 (latency) and C2 (skipped).
        assert_eq!(acpicpu_choose(&l, 1_000_000), Some(0));
        assert_eq!(acpicpu_choose(&l, 299), Some(2));
        assert_eq!(acpicpu_choose(&l, 0), Some(2)); // none fits: the last
        assert_eq!(acpicpu_choose(&[], 0), None);
        assert!(acpicpu_bm_check(&l[0]));
        assert!(!acpicpu_bm_check(&l[2]));
        assert_eq!(acpicpu_bm_backoff(&l, 0), 2);
        let deep = [
            cx(3, CST_METH_MWAIT, CST_FLAG_MWAIT_BM_AVOIDANCE, 100),
            cx(4, CST_METH_MWAIT, CST_FLAG_MWAIT_BM_AVOIDANCE, 200),
        ];
        assert_eq!(acpicpu_bm_backoff(&deep, 0), 1); // none shallower: the last
        assert_eq!(acpicpu_suspend_choose(&l), Some(0));
        assert_eq!(
            acpicpu_suspend_choose(&[cx(2, 0, CST_FLAG_SKIP, 1), cx(1, 0, CST_FLAG_SKIP, 1)]),
            Some(1)
        );
        // The average: 3/4 of the old value plus a quarter of the idle time.
        assert_eq!(acpicpu_prev_sleep(1_000_000, 0), 750_000);
        assert_eq!(acpicpu_prev_sleep(1000, 1000), 1000);
        assert_eq!(acpicpu_prev_sleep(0, 5000), 1250);
    }

    #[test]
    fn performance_states() {
        // 4 states: 100% is the first, 0% the last.
        assert_eq!(acpicpu_setperf_idx(100, 4, 0), 0);
        assert_eq!(acpicpu_setperf_idx(0, 4, 0), 3);
        assert_eq!(acpicpu_setperf_idx(50, 4, 0), 1);
        // _PPC 2: only the last two states.
        assert_eq!(acpicpu_setperf_idx(100, 4, 2), 2);
        assert_eq!(acpicpu_setperf_idx(0, 4, 2), 3);
        // The C's divide by zero and off-by-one are clamped.
        assert_eq!(acpicpu_setperf_idx(50, 101, 0), 0);
        assert_eq!(acpicpu_setperf_idx(0, 0, 0), 0);

        let e = |f: i64, ctl: i64| {
            Rc::new(pkg(vec![
                int(f),
                int(10),
                int(10),
                int(10),
                int(ctl),
                int(ctl),
            ]))
        };
        let res = pkg(vec![e(2400, 0x1a), e(0, 0), e(0xffff, 1), e(800, 0x08)]);
        let (pss, said) = acpicpu_parse_pss(&res, "acpicpu0");
        assert_eq!(
            pss.iter()
                .map(|p| (p.pss_core_freq, p.pss_ctrl))
                .collect::<Vec<_>>(),
            [(2400, 0x1a), (800, 0x08)]
        );
        assert_eq!(
            said,
            [
                "acpicpu0: struck PSS entry, core frequency equals  last\n",
                "acpicpu0: struck PSS entry, inappropriate core frequency value\n",
            ]
        );

        // _PCT: a 16-bit status register with no access size, a control register without
        // a width and an access size of 2.
        let mut st = vec![0x82, 12, 0, 1, 16, 0, 0];
        st.extend_from_slice(&0x0880u64.to_le_bytes());
        let mut ct = vec![0x82, 12, 0, 1, 0, 0, 2];
        ct.extend_from_slice(&0x0882u64.to_le_bytes());
        let pct = AcpicpuPct {
            pct_ctrl: acpicpu_grd(&ct),
            pct_status: acpicpu_grd(&st),
        };
        assert_eq!(acpicpu_pct_sizes(&pct), (2, 4, 2, 2));
        let g = acpicpu_grd(&st[..5]); // short: zero-filled
        assert_eq!({ g.grd_gas.register_bit_width }, 16);
        assert_eq!({ g.grd_gas.address }, 0);
    }
}
/* </TESTS> */
