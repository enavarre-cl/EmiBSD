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
 * Copyright (c) 2016 Dale Rahn <drahn@dalerahn.com>
 * Copyright (c) 2017 Mark Kettenis <kettenis@openbsd.org>
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
//! arm64 CPU identification, attachment and the application processors:
//! `arch/arm64/arm64/cpu.c`.
//!
//! Upstream: sys/arch/arm64/arm64/cpu.c @ 3ce1f3f79392
//!
//! Status: `ported` (M11a). The identification tables (`cpu_implementers[]` and the
//! `cpu_cores_*[]` lists), `cpu_rng`, the Spectre-V2/BHB/V4 and CVE-2025-10263 mitigation
//! choices, `cpu_identify` (the core name, the caches, the ID register features),
//! `cpu_identify_cleanup` (the userland view of the ID registers, `hwcap`/`hwcap2`),
//! `cpu_classify`, `cpu_match`/`cpu_attach` (`cpu0 at mainbus0 mpidr 0: ARM Cortex-A72
//! r0p3`), `cpu_init`, the branch-predictor and SError hooks, `cpu_clockspeed`; with
//! `MULTIPROCESSOR` `cpu_boot_secondary_processors`, `cpu_start_secondary`,
//! `cpu_boot_secondary`, `cpu_init_secondary` (with the application processor's entry from
//! the boot glue, `cpu_hatch_entry`), `cpu_halt`, `cpu_kick` and `cpu_unidle`; `SUSPEND`'s
//! `cpu_suspend_cycle`; the operating points (`cpu_opp_*`), `cpu_psci_init` and
//! `cpu_psci_idle_cycle`; the kstats.
//!
//! ## Deviations
//! - The application processors are started through the boot protocol (`BootMp`, kept by
//!   `initarm`), not PSCI `CPU_ON` or a spin table into `locore.S`'s `cpu_hatch_secondary`
//!   (`docs/ARCHITECTURE.md`). `cpu_start_secondary` still reads the `enable-method` and
//!   refuses a CPU without one, then releases the processor whose MPIDR affinity matches
//!   the node's `reg` with its `cpu_info`; `cpu_start_spin_table` and the `cpu_hatch_ci`
//!   physical address have nothing left to do. The processor arrives in `cpu_hatch_entry`
//!   with the MMU on and the bootloader's tables: it does what `cpu_hatch_secondary` does
//!   (`TPIDR_EL1`, then the MMU with the boot processor's `MAIR_EL1`, `TCR_EL1`,
//!   `SCTLR_EL1` and `ci_ttbr1`, then `ci_el1_stkend` as its stack) plus what `initarm` does
//!   on the boot processor (`SPSel`, `VBAR_EL1`, the FPU trapped as `fpu_drop` leaves it),
//!   then calls `cpu_init_secondary`. After a boot by boot(8) (M14) the `BootMp` is
//!   `machdep.rs`'s: psci(4)'s `psci_cpu_on` (`dev/fdt/psci.rs`, attached before the
//!   processors) at `locore.S`'s `cpu_hatch_secondary`, which
//!   brings the MMU up on `locore0.S`'s identity map and the kernel's `TTBR1_EL1` and
//!   enters `cpu_hatch_entry` on `ci_el1_stkend` (`locore.rs`, deviations).
//! - Not in the C: `cpu_hatch_entry` turns on the processor's generic timer event stream
//!   (`agtimer_evtstrm_enable`, a `wfe` wake-up every ~130 µs; `agtimer_startclock` keeps it
//!   on) before `cpu_init_secondary` waits in `wfe` for `CPUF_IDENTIFY` and `CPUF_GO`. Those
//!   waits otherwise end only on the boot processor's `sev` (the `ARM_IPI_NOP` it adds cannot
//!   wake a processor whose GIC CPU interface `arm_intr_cpu_enable` has not enabled yet),
//!   and QEMU's TCG (since it halts on `wfe`) can lose that `sev`: its `sev` helper kicks the
//!   halted vCPU without the BQL, racing the vCPU thread's idle check, so the vCPU sleeps on
//!   until an interrupt or a timer wakes it. A boot then stalled forever in
//!   `cpu_boot_secondary` (M11e). The event stream is QEMU's timer-driven, reliable wake-up
//!   and bounds every such lost event; Linux keeps it on for the same reason.
//! - `psci* at fdt?` is configured (M13), so `NPSCI` is 1: `cpu_flush_bp_psci`'s
//!   `psci_flush_bp`, `smccc_enable_arch_workaround_2` (Spectre-V4) and PSCI
//!   `CPU_OFF`/`CPU_SUSPEND` in `cpu_halt` are the C's. The firmware Spectre-BHB vectors
//!   (`smccc_needs_arch_workaround_3`, `trampoline_vectors_psci_{hvc,smc}`) wait for
//!   `trampoline.S`; `psci_features` (`cpu_psci_init`, only with `cpu-idle-states`) and
//!   `psci_cpu_suspend` (`cpu_psci_idle_cycle`) stay reported: no idle state is ever picked.
//!   `cpu_start_secondary` still starts the APs through Limine, never `psci_cpu_on`.
//! - `trampoline.S` is not ported (`exception.rs`): `ci_trampoline_vectors` records which
//!   `trampoline_vectors_*` table the C would pick, as a `TRAMPOLINE_VECTORS_*` number.
//! - `codepatch_nop(CPTAG_REPEAT_TLBI)` has nothing to patch: the TLB invalidations never
//!   repeat yet (`cpufunc.rs`, deviations), which is the patched state; the CPUs the C keeps
//!   the repeat for (CVE-2025-10263, not QEMU's Cortex-A72) go without it.
//! - `hw.model` is `CPU_MODEL` here; `kern_sysctl.rs` does not read it yet.
//! - `NKSTAT` is 0: `pseudo-device kstat` is not configured (`dev/kstat.c` is not ported);
//!   `cpu_kstat_attach`/`cpu_opp_kstat_attach` report it and their callers never run.
//! - `cpu_clockspeed`, `cpu_opp_mountroot` and `cpu_opp_dotask` report the clock and
//!   regulator framework (`ofw_clock.c`, `ofw_regulator.c`) and `cpu_setperf`
//!   (`sched_bsd.c`); `cpu_opp_init` reports `cooling_device_register` (`ofw_thermal.c`).
//!   QEMU's processors have neither `clocks` nor `operating-points-v2`.
//! - `SUSPEND` is not configured here (M14): `cpu_init_primary`, `cpu_suspend_primary` and
//!   `cpu_resume_secondary` are reported; `cpu_suspend_cycle` (which `cpu_halt` uses) is
//!   ported. `HIBERNATE`'s `cpu_park` is not configured either.
//! - `NXCALL` is 0 on arm64 (only `psp(4)` needs `xcall`): no `cpu_xcall_establish`.
//! - `cpu_info[]` holds `CiPtr`s (another CPU reads them); APs' `cpu_info`s come from
//!   `malloc(9)` and live forever, as in C.

use core::arch::asm;
use core::cell::Cell;
use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::{AtomicI32, AtomicU64, Ordering};

use libkern::StaticCell;

use crate::arch::arm64::arm64::cpufunc::{cpu_tlb_flush, cpu_wfi};
use crate::arch::arm64::arm64::machdep::cpu_info_list;
use crate::arch::arm64::include::armreg::*;
use crate::arch::arm64::include::cpu::cpu_is_primary;
#[cfg(feature = "multiprocessor")]
use crate::arch::arm64::include::cpu::{CPUF_IDENTIFIED, CPUF_IDENTIFY, CPUF_PRESENT};
#[cfg(feature = "multiprocessor")]
use crate::arch::arm64::include::cpu::{CPUF_PRIMARY, CPUF_RUNNING};
use crate::arch::arm64::include::cpu::{CpuInfo, curcpu};
use crate::arch::arm64::include::elf::*;
use crate::arch::arm64::include::fdt::FdtAttachArgs;
use crate::arch::arm64::include::vmparam::USER_SPACE_BITS;
#[cfg(feature = "multiprocessor")]
use crate::dev::fdt::psci::{psci_can_suspend, psci_cpu_off};
use crate::dev::fdt::psci::{psci_flush_bp, smccc_enable_arch_workaround_2};
use crate::dev::ofw::fdt::{
    OF_child, OF_getindex, OF_getnodebyphandle, OF_getprop, OF_getpropbool, OF_getpropint,
    OF_getpropint64, OF_getpropintarray, OF_getproplen, OF_is_compatible, OF_peer,
};
use crate::kern::exec_elf::{HWCAP, HWCAP2};
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_sysctl::{CPU_CPUSPEED, hw_prod, hw_vendor};
use crate::kern::kern_task::{SYSTQ, task_add, task_set};
use crate::kern::kern_tc::microuptime;
use crate::kern::kern_timeout::{timeout_add_msec, timeout_set};
use crate::kern::subr_autoconf::config_mountroot;
use crate::kern::subr_prf::{panic, snprintf};
use crate::kprintf;
use crate::sys::device::{CfMatch, Cfattach, Cfdriver, DV_DULL, Device};
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_DEVBUF, M_TEMP, M_WAITOK, M_ZERO};
use crate::sys::param::USPACE;
use crate::sys::queue::{ListEntry, ListHead};
use crate::sys::sched::{CPUTYP_E, CPUTYP_L, CPUTYP_P};
use crate::sys::task::Task;
use crate::sys::time::timersub;
use crate::sys::timeout::Timeout;
use crate::unported;
use crate::uvm::uvm_km::{KD_WAITOK, KP_ZERO, KV_ANY, km_alloc};
use crate::{kassert, queue_adapter};

#[cfg(feature = "multiprocessor")]
use crate::arch::arm64::arm64::cpuswitch::proc_trampoline;
#[cfg(feature = "multiprocessor")]
use crate::arch::arm64::arm64::exception::exception_vectors_addr;
#[cfg(feature = "multiprocessor")]
use crate::arch::arm64::arm64::intr::{arm_intr_cpu_enable, arm_send_ipi, cpu_startclock, delay};
#[cfg(feature = "multiprocessor")]
use crate::arch::arm64::arm64::machdep::{BOOT_MP, CPU_INFO};
#[cfg(feature = "multiprocessor")]
use crate::arch::arm64::include::cpu::{CPUF_AP, CPUF_GO, intr_disable, intr_restore};
#[cfg(feature = "multiprocessor")]
use crate::arch::arm64::include::frame::{Switchframe, Trapframe};
#[cfg(feature = "multiprocessor")]
use crate::arch::arm64::include::intr::{ARM_IPI_NOP, IPL_HIGH, IPL_NONE};
#[cfg(feature = "multiprocessor")]
use crate::arch::arm64::include::param::stackalign;
#[cfg(feature = "multiprocessor")]
use crate::dev::rnd::arc4random;
#[cfg(feature = "multiprocessor")]
use crate::kern::init_main::NCPUS;
#[cfg(feature = "multiprocessor")]
use crate::kern::kern_clockintr::clockqueue_init;
#[cfg(feature = "multiprocessor")]
use crate::kern::kern_sched::{sched_idle, sched_init_cpu, sched_toidle};
#[cfg(feature = "multiprocessor")]
use crate::kern::sched_bsd::sched_assert_unlocked;
#[cfg(feature = "multiprocessor")]
use crate::machine::intr::{spllower, splraise};
#[cfg(feature = "multiprocessor")]
use crate::sys::systm::kernel_assert_unlocked;
#[cfg(feature = "multiprocessor")]
use crate::sys::types::Register;

// CPU Identification

/// `CPU_IMPL_ARM`.
pub const CPU_IMPL_ARM: u64 = 0x41;
/// `CPU_IMPL_CAVIUM`.
pub const CPU_IMPL_CAVIUM: u64 = 0x43;
/// `CPU_IMPL_NVIDIA`.
pub const CPU_IMPL_NVIDIA: u64 = 0x4e;
/// `CPU_IMPL_AMCC`.
pub const CPU_IMPL_AMCC: u64 = 0x50;
/// `CPU_IMPL_QCOM`.
pub const CPU_IMPL_QCOM: u64 = 0x51;
/// `CPU_IMPL_APPLE`.
pub const CPU_IMPL_APPLE: u64 = 0x61;
/// `CPU_IMPL_MICROSOFT`.
pub const CPU_IMPL_MICROSOFT: u64 = 0x6d;
/// `CPU_IMPL_AMPERE`.
pub const CPU_IMPL_AMPERE: u64 = 0xc0;

/// ARM parts: `CPU_PART_*`.
pub mod part {
    /// `CPU_PART_CORTEX_A34`.
    pub const CORTEX_A34: u64 = 0xd02;
    /// `CPU_PART_CORTEX_A53`.
    pub const CORTEX_A53: u64 = 0xd03;
    /// `CPU_PART_CORTEX_A35`.
    pub const CORTEX_A35: u64 = 0xd04;
    /// `CPU_PART_CORTEX_A55`.
    pub const CORTEX_A55: u64 = 0xd05;
    /// `CPU_PART_CORTEX_A65`.
    pub const CORTEX_A65: u64 = 0xd06;
    /// `CPU_PART_CORTEX_A57`.
    pub const CORTEX_A57: u64 = 0xd07;
    /// `CPU_PART_CORTEX_A72`.
    pub const CORTEX_A72: u64 = 0xd08;
    /// `CPU_PART_CORTEX_A73`.
    pub const CORTEX_A73: u64 = 0xd09;
    /// `CPU_PART_CORTEX_A75`.
    pub const CORTEX_A75: u64 = 0xd0a;
    /// `CPU_PART_CORTEX_A76`.
    pub const CORTEX_A76: u64 = 0xd0b;
    /// `CPU_PART_NEOVERSE_N1`.
    pub const NEOVERSE_N1: u64 = 0xd0c;
    /// `CPU_PART_CORTEX_A77`.
    pub const CORTEX_A77: u64 = 0xd0d;
    /// `CPU_PART_CORTEX_A76AE`.
    pub const CORTEX_A76AE: u64 = 0xd0e;
    /// `CPU_PART_NEOVERSE_V1`.
    pub const NEOVERSE_V1: u64 = 0xd40;
    /// `CPU_PART_CORTEX_A78`.
    pub const CORTEX_A78: u64 = 0xd41;
    /// `CPU_PART_CORTEX_A78AE`.
    pub const CORTEX_A78AE: u64 = 0xd42;
    /// `CPU_PART_CORTEX_A65AE`.
    pub const CORTEX_A65AE: u64 = 0xd43;
    /// `CPU_PART_CORTEX_X1`.
    pub const CORTEX_X1: u64 = 0xd44;
    /// `CPU_PART_CORTEX_A510`.
    pub const CORTEX_A510: u64 = 0xd46;
    /// `CPU_PART_CORTEX_A710`.
    pub const CORTEX_A710: u64 = 0xd47;
    /// `CPU_PART_CORTEX_X2`.
    pub const CORTEX_X2: u64 = 0xd48;
    /// `CPU_PART_NEOVERSE_N2`.
    pub const NEOVERSE_N2: u64 = 0xd49;
    /// `CPU_PART_NEOVERSE_E1`.
    pub const NEOVERSE_E1: u64 = 0xd4a;
    /// `CPU_PART_CORTEX_A78C`.
    pub const CORTEX_A78C: u64 = 0xd4b;
    /// `CPU_PART_CORTEX_X1C`.
    pub const CORTEX_X1C: u64 = 0xd4c;
    /// `CPU_PART_CORTEX_A715`.
    pub const CORTEX_A715: u64 = 0xd4d;
    /// `CPU_PART_CORTEX_X3`.
    pub const CORTEX_X3: u64 = 0xd4e;
    /// `CPU_PART_NEOVERSE_V2`.
    pub const NEOVERSE_V2: u64 = 0xd4f;
    /// `CPU_PART_CORTEX_A520`.
    pub const CORTEX_A520: u64 = 0xd80;
    /// `CPU_PART_CORTEX_A720`.
    pub const CORTEX_A720: u64 = 0xd81;
    /// `CPU_PART_CORTEX_X4`.
    pub const CORTEX_X4: u64 = 0xd82;
    /// `CPU_PART_NEOVERSE_V3AE`.
    pub const NEOVERSE_V3AE: u64 = 0xd83;
    /// `CPU_PART_NEOVERSE_V3`.
    pub const NEOVERSE_V3: u64 = 0xd84;
    /// `CPU_PART_CORTEX_X925`.
    pub const CORTEX_X925: u64 = 0xd85;
    /// `CPU_PART_CORTEX_A725`.
    pub const CORTEX_A725: u64 = 0xd87;
    /// `CPU_PART_CORTEX_A520AE`.
    pub const CORTEX_A520AE: u64 = 0xd88;
    /// `CPU_PART_CORTEX_A720AE`.
    pub const CORTEX_A720AE: u64 = 0xd89;
    /// `CPU_PART_C1_NANO`.
    pub const C1_NANO: u64 = 0xd8a;
    /// `CPU_PART_C1_PRO`.
    pub const C1_PRO: u64 = 0xd8b;
    /// `CPU_PART_C1_ULTRA`.
    pub const C1_ULTRA: u64 = 0xd8c;
    /// `CPU_PART_NEOVERSE_N3`.
    pub const NEOVERSE_N3: u64 = 0xd8e;
    /// `CPU_PART_CORTEX_A320`.
    pub const CORTEX_A320: u64 = 0xd8f;
    /// `CPU_PART_C1_PREMIUM`.
    pub const C1_PREMIUM: u64 = 0xd90;
    /// `CPU_PART_C2_ULTRA`.
    pub const C2_ULTRA: u64 = 0xd96;

    // Cavium
    /// `CPU_PART_THUNDERX_T88`.
    pub const THUNDERX_T88: u64 = 0x0a1;
    /// `CPU_PART_THUNDERX_T81`.
    pub const THUNDERX_T81: u64 = 0x0a2;
    /// `CPU_PART_THUNDERX_T83`.
    pub const THUNDERX_T83: u64 = 0x0a3;
    /// `CPU_PART_THUNDERX2_T99`.
    pub const THUNDERX2_T99: u64 = 0x0af;

    // NVIDIA
    /// `CPU_PART_OLYMPUS`.
    pub const OLYMPUS: u64 = 0x010;

    // Applied Micro
    /// `CPU_PART_X_GENE`.
    pub const X_GENE: u64 = 0x000;

    // Qualcomm
    /// `CPU_PART_ORYON`.
    pub const ORYON: u64 = 0x001;
    /// `CPU_PART_ORYON_V3`.
    pub const ORYON_V3: u64 = 0x002;
    /// `CPU_PART_KRYO400_GOLD`.
    pub const KRYO400_GOLD: u64 = 0x804;
    /// `CPU_PART_KRYO400_SILVER`.
    pub const KRYO400_SILVER: u64 = 0x805;

    // Apple
    /// `CPU_PART_ICESTORM`.
    pub const ICESTORM: u64 = 0x022;
    /// `CPU_PART_FIRESTORM`.
    pub const FIRESTORM: u64 = 0x023;
    /// `CPU_PART_ICESTORM_PRO`.
    pub const ICESTORM_PRO: u64 = 0x024;
    /// `CPU_PART_FIRESTORM_PRO`.
    pub const FIRESTORM_PRO: u64 = 0x025;
    /// `CPU_PART_ICESTORM_MAX`.
    pub const ICESTORM_MAX: u64 = 0x028;
    /// `CPU_PART_FIRESTORM_MAX`.
    pub const FIRESTORM_MAX: u64 = 0x029;
    /// `CPU_PART_BLIZZARD`.
    pub const BLIZZARD: u64 = 0x032;
    /// `CPU_PART_AVALANCHE`.
    pub const AVALANCHE: u64 = 0x033;
    /// `CPU_PART_BLIZZARD_PRO`.
    pub const BLIZZARD_PRO: u64 = 0x034;
    /// `CPU_PART_AVALANCHE_PRO`.
    pub const AVALANCHE_PRO: u64 = 0x035;
    /// `CPU_PART_BLIZZARD_MAX`.
    pub const BLIZZARD_MAX: u64 = 0x038;
    /// `CPU_PART_AVALANCHE_MAX`.
    pub const AVALANCHE_MAX: u64 = 0x039;

    // Ampere
    /// `CPU_PART_AMPERE1_AC03`.
    pub const AMPERE1_AC03: u64 = 0xac3;
    /// `CPU_PART_AMPERE1_AC04`.
    pub const AMPERE1_AC04: u64 = 0xac4;
}

use part::*;

/// `CPU_IMPL(midr)`.
pub const fn cpu_impl(midr: u64) -> u64 {
    (midr >> 24) & 0xff
}

/// `CPU_PART(midr)`.
pub const fn cpu_part(midr: u64) -> u64 {
    (midr >> 4) & 0xfff
}

/// `CPU_VAR(midr)`.
pub const fn cpu_var(midr: u64) -> u64 {
    (midr >> 20) & 0xf
}

/// `CPU_REV(midr)`.
pub const fn cpu_rev(midr: u64) -> u64 {
    midr & 0xf
}

/// `NPSCI`: `psci* at fdt?` is configured (see the module's deviations).
const NPSCI: i32 = 1;
/// `NKSTAT`: `pseudo-device kstat` is not configured.
const NKSTAT: i32 = 0;

/// The `trampoline_vectors_*` table `ci_trampoline_vectors` names (`trampoline.S`, not
/// ported: see the module's deviations): `trampoline_vectors_none`.
pub const TRAMPOLINE_VECTORS_NONE: u64 = 0;
/// `trampoline_vectors_loop_8`.
pub const TRAMPOLINE_VECTORS_LOOP_8: u64 = 1;
/// `trampoline_vectors_loop_11`.
pub const TRAMPOLINE_VECTORS_LOOP_11: u64 = 2;
/// `trampoline_vectors_loop_24`.
pub const TRAMPOLINE_VECTORS_LOOP_24: u64 = 3;
/// `trampoline_vectors_loop_32`.
pub const TRAMPOLINE_VECTORS_LOOP_32: u64 = 4;
/// `trampoline_vectors_loop_132`.
pub const TRAMPOLINE_VECTORS_LOOP_132: u64 = 5;
/// `trampoline_vectors_clrbhb`.
pub const TRAMPOLINE_VECTORS_CLRBHB: u64 = 6;

/// `struct cpu_cores`: a core of an implementer.
pub struct CpuCores {
    /// `id`: the `CPU_PART`.
    pub id: u64,
    /// `name`.
    pub name: &'static str,
}

/// `struct implementers`: arm cores makers.
pub struct Implementers {
    /// `id`: the `CPU_IMPL`.
    pub id: u64,
    /// `name`.
    pub name: &'static str,
    /// `corelist`.
    pub corelist: &'static [CpuCores],
}

/// `struct opp`: one operating point.
#[derive(Clone, Copy, Default)]
pub struct Opp {
    /// `opp_hz`.
    pub opp_hz: u64,
    /// `opp_microvolt`.
    pub opp_microvolt: u32,
}

/// `struct opp_table`: the operating points of one `operating-points-v2` node.
pub struct OppTable {
    /// `ot_list`: the `opp_tables` link.
    pub ot_list: ListEntry<OppTable>,
    /// `ot_phandle`.
    pub ot_phandle: u32,
    /// `ot_opp`: `ot_nopp` points, sorted by frequency (`malloc(9)`, never freed).
    pub ot_opp: &'static [Opp],
    /// `ot_opp_hz_min`.
    pub ot_opp_hz_min: u64,
    /// `ot_opp_hz_max`.
    pub ot_opp_hz_max: u64,
    /// `ot_master`: the CPU that drives a shared table, null when not shared.
    pub ot_master: Cell<*const CpuInfo>,
}

impl OppTable {
    /// `ot_nopp`.
    pub fn ot_nopp(&self) -> i32 {
        self.ot_opp.len() as i32
    }
}

queue_adapter!(
    /// `LIST_HEAD(, opp_table) opp_tables`.
    pub OppTableList: OppTable, ot_list => ListEntry<OppTable>
);

/// `opp_tables` behind a `Sync` wrapper: CPUs attach one at a time on the boot CPU.
struct OppTables(ListHead<OppTableList>);
// SAFETY: see the type's doc; nothing touches the list after autoconfiguration.
unsafe impl Sync for OppTables {}

/// `cpu_cores_none[]`.
static CPU_CORES_NONE: [CpuCores; 0] = [];

/// `cpu_cores_arm[]`.
static CPU_CORES_ARM: [CpuCores; 44] = [
    CpuCores {
        id: C1_NANO,
        name: "C1-Nano",
    },
    CpuCores {
        id: C1_PREMIUM,
        name: "C1-Premium",
    },
    CpuCores {
        id: C1_PRO,
        name: "C1-Pro",
    },
    CpuCores {
        id: C1_ULTRA,
        name: "C1-Ultra",
    },
    CpuCores {
        id: C2_ULTRA,
        name: "C2-Ultra",
    },
    CpuCores {
        id: CORTEX_A34,
        name: "Cortex-A34",
    },
    CpuCores {
        id: CORTEX_A35,
        name: "Cortex-A35",
    },
    CpuCores {
        id: CORTEX_A53,
        name: "Cortex-A53",
    },
    CpuCores {
        id: CORTEX_A55,
        name: "Cortex-A55",
    },
    CpuCores {
        id: CORTEX_A57,
        name: "Cortex-A57",
    },
    CpuCores {
        id: CORTEX_A65,
        name: "Cortex-A65",
    },
    CpuCores {
        id: CORTEX_A65AE,
        name: "Cortex-A65AE",
    },
    CpuCores {
        id: CORTEX_A72,
        name: "Cortex-A72",
    },
    CpuCores {
        id: CORTEX_A73,
        name: "Cortex-A73",
    },
    CpuCores {
        id: CORTEX_A75,
        name: "Cortex-A75",
    },
    CpuCores {
        id: CORTEX_A76,
        name: "Cortex-A76",
    },
    CpuCores {
        id: CORTEX_A76AE,
        name: "Cortex-A76AE",
    },
    CpuCores {
        id: CORTEX_A77,
        name: "Cortex-A77",
    },
    CpuCores {
        id: CORTEX_A78,
        name: "Cortex-A78",
    },
    CpuCores {
        id: CORTEX_A78AE,
        name: "Cortex-A78AE",
    },
    CpuCores {
        id: CORTEX_A78C,
        name: "Cortex-A78C",
    },
    CpuCores {
        id: CORTEX_A320,
        name: "Cortex-A320",
    },
    CpuCores {
        id: CORTEX_A510,
        name: "Cortex-A510",
    },
    CpuCores {
        id: CORTEX_A520,
        name: "Cortex-A520",
    },
    CpuCores {
        id: CORTEX_A520AE,
        name: "Cortex-A520AE",
    },
    CpuCores {
        id: CORTEX_A710,
        name: "Cortex-A710",
    },
    CpuCores {
        id: CORTEX_A715,
        name: "Cortex-A715",
    },
    CpuCores {
        id: CORTEX_A720,
        name: "Cortex-A720",
    },
    CpuCores {
        id: CORTEX_A720AE,
        name: "Cortex-A720AE",
    },
    CpuCores {
        id: CORTEX_A725,
        name: "Cortex-A725",
    },
    CpuCores {
        id: CORTEX_X1,
        name: "Cortex-X1",
    },
    CpuCores {
        id: CORTEX_X1C,
        name: "Cortex-X1C",
    },
    CpuCores {
        id: CORTEX_X2,
        name: "Cortex-X2",
    },
    CpuCores {
        id: CORTEX_X3,
        name: "Cortex-X3",
    },
    CpuCores {
        id: CORTEX_X4,
        name: "Cortex-X4",
    },
    CpuCores {
        id: CORTEX_X925,
        name: "Cortex-X925",
    },
    CpuCores {
        id: NEOVERSE_E1,
        name: "Neoverse E1",
    },
    CpuCores {
        id: NEOVERSE_N1,
        name: "Neoverse N1",
    },
    CpuCores {
        id: NEOVERSE_N2,
        name: "Neoverse N2",
    },
    CpuCores {
        id: NEOVERSE_N3,
        name: "Neoverse N3",
    },
    CpuCores {
        id: NEOVERSE_V1,
        name: "Neoverse V1",
    },
    CpuCores {
        id: NEOVERSE_V2,
        name: "Neoverse V2",
    },
    CpuCores {
        id: NEOVERSE_V3,
        name: "Neoverse V3",
    },
    CpuCores {
        id: NEOVERSE_V3AE,
        name: "Neoverse V3AE",
    },
];

/// `cpu_cores_cavium[]`.
static CPU_CORES_CAVIUM: [CpuCores; 4] = [
    CpuCores {
        id: THUNDERX_T88,
        name: "ThunderX T88",
    },
    CpuCores {
        id: THUNDERX_T81,
        name: "ThunderX T81",
    },
    CpuCores {
        id: THUNDERX_T83,
        name: "ThunderX T83",
    },
    CpuCores {
        id: THUNDERX2_T99,
        name: "ThunderX2 T99",
    },
];

/// `cpu_cores_nvidia[]`.
static CPU_CORES_NVIDIA: [CpuCores; 1] = [CpuCores {
    id: OLYMPUS,
    name: "Olympus",
}];

/// `cpu_cores_amcc[]`.
static CPU_CORES_AMCC: [CpuCores; 1] = [CpuCores {
    id: X_GENE,
    name: "X-Gene",
}];

/// `cpu_cores_qcom[]`.
static CPU_CORES_QCOM: [CpuCores; 4] = [
    CpuCores {
        id: KRYO400_GOLD,
        name: "Kryo 400 Gold",
    },
    CpuCores {
        id: KRYO400_SILVER,
        name: "Kryo 400 Silver",
    },
    CpuCores {
        id: ORYON,
        name: "Oryon",
    },
    CpuCores {
        id: ORYON_V3,
        name: "Oryon V3",
    },
];

/// `cpu_cores_apple[]`.
static CPU_CORES_APPLE: [CpuCores; 12] = [
    CpuCores {
        id: ICESTORM,
        name: "Icestorm",
    },
    CpuCores {
        id: FIRESTORM,
        name: "Firestorm",
    },
    CpuCores {
        id: ICESTORM_PRO,
        name: "Icestorm Pro",
    },
    CpuCores {
        id: FIRESTORM_PRO,
        name: "Firestorm Pro",
    },
    CpuCores {
        id: ICESTORM_MAX,
        name: "Icestorm Max",
    },
    CpuCores {
        id: FIRESTORM_MAX,
        name: "Firestorm Max",
    },
    CpuCores {
        id: BLIZZARD,
        name: "Blizzard",
    },
    CpuCores {
        id: AVALANCHE,
        name: "Avalanche",
    },
    CpuCores {
        id: BLIZZARD_PRO,
        name: "Blizzard Pro",
    },
    CpuCores {
        id: AVALANCHE_PRO,
        name: "Avalanche Pro",
    },
    CpuCores {
        id: BLIZZARD_MAX,
        name: "Blizzard Max",
    },
    CpuCores {
        id: AVALANCHE_MAX,
        name: "Avalanche Max",
    },
];

/// `cpu_cores_ampere[]`.
static CPU_CORES_AMPERE: [CpuCores; 2] = [
    CpuCores {
        id: AMPERE1_AC03,
        name: "AmpereOne AC03",
    },
    CpuCores {
        id: AMPERE1_AC04,
        name: "AmpereOne AC04",
    },
];

/// `cpu_cores_microsoft[]`.
static CPU_CORES_MICROSOFT: [CpuCores; 1] = [CpuCores {
    id: NEOVERSE_N2,
    name: "Azure Cobalt 100",
}];

/// `cpu_implementers[]`: arm cores makers.
pub static CPU_IMPLEMENTERS: [Implementers; 8] = [
    Implementers {
        id: CPU_IMPL_ARM,
        name: "ARM",
        corelist: &CPU_CORES_ARM,
    },
    Implementers {
        id: CPU_IMPL_CAVIUM,
        name: "Cavium",
        corelist: &CPU_CORES_CAVIUM,
    },
    Implementers {
        id: CPU_IMPL_NVIDIA,
        name: "NVIDIA",
        corelist: &CPU_CORES_NVIDIA,
    },
    Implementers {
        id: CPU_IMPL_AMCC,
        name: "Applied Micro",
        corelist: &CPU_CORES_AMCC,
    },
    Implementers {
        id: CPU_IMPL_QCOM,
        name: "Qualcomm",
        corelist: &CPU_CORES_QCOM,
    },
    Implementers {
        id: CPU_IMPL_APPLE,
        name: "Apple",
        corelist: &CPU_CORES_APPLE,
    },
    Implementers {
        id: CPU_IMPL_AMPERE,
        name: "Ampere",
        corelist: &CPU_CORES_AMPERE,
    },
    Implementers {
        id: CPU_IMPL_MICROSOFT,
        name: "Microsoft",
        corelist: &CPU_CORES_MICROSOFT,
    },
];

/// `cpu_model`: `hw.model`, written by the boot CPU's `cpu_identify` at attach.
pub static CPU_MODEL: StaticCell<[u8; 64]> = StaticCell::new([0; 64]);
/// `cpu_node`: the CPU node whose `clocks` `cpu_clockspeed` reads.
pub static CPU_NODE: AtomicI32 = AtomicI32::new(0);

/// `cpu_id_aa64isar0`: the userland view of `ID_AA64ISAR0_EL1` (`cpu_identify_cleanup`).
pub static CPU_ID_AA64ISAR0: AtomicU64 = AtomicU64::new(0);
/// `cpu_id_aa64isar1`.
pub static CPU_ID_AA64ISAR1: AtomicU64 = AtomicU64::new(0);
/// `cpu_id_aa64isar2`.
pub static CPU_ID_AA64ISAR2: AtomicU64 = AtomicU64::new(0);
/// `cpu_id_aa64mmfr0`.
pub static CPU_ID_AA64MMFR0: AtomicU64 = AtomicU64::new(0);
/// `cpu_id_aa64mmfr1`.
pub static CPU_ID_AA64MMFR1: AtomicU64 = AtomicU64::new(0);
/// `cpu_id_aa64mmfr2`.
pub static CPU_ID_AA64MMFR2: AtomicU64 = AtomicU64::new(0);
/// `cpu_id_aa64pfr0`.
pub static CPU_ID_AA64PFR0: AtomicU64 = AtomicU64::new(0);
/// `cpu_id_aa64pfr1`.
pub static CPU_ID_AA64PFR1: AtomicU64 = AtomicU64::new(0);
/// `cpu_id_aa64zfr0`.
pub static CPU_ID_AA64ZFR0: AtomicU64 = AtomicU64::new(0);

/// `arm64_has_lse`: the LSE atomics.
pub static ARM64_HAS_LSE: AtomicI32 = AtomicI32::new(0);
/// `arm64_has_rng`: `RNDR`.
pub static ARM64_HAS_RNG: AtomicI32 = AtomicI32::new(0);
/// `arm64_has_aes` (`CRYPTO`).
pub static ARM64_HAS_AES: AtomicI32 = AtomicI32::new(0);

/// `cpu_ca`.
pub static CPU_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<Device>(),
    ca_match: Some(cpu_match),
    ca_attach: cpu_attach,
    ca_detach: None,
    ca_activate: None,
};

/// `cpu_cd`.
pub static CPU_CD: Cfdriver = Cfdriver::new(b"cpu", DV_DULL, 0);

/// `cpu_rng_to`: feeds `RNDR` into the entropy pool every second.
static CPU_RNG_TO: Timeout = Timeout::new(cpu_rng, ptr::null_mut());

/// `opp_tables`.
static OPP_TABLES: OppTables = OppTables(ListHead::new());
/// `cpu_opp_task`: applies a new performance level from process context.
static CPU_OPP_TASK: Task = Task::new(cpu_opp_dotask, ptr::null_mut());

/// The boot processor's `MAIR_EL1`, `TCR_EL1` and `SCTLR_EL1`, which `cpu_start_secondary`
/// hands to the application processors (what `locore.S`'s `start_mmu` programs).
#[cfg(feature = "multiprocessor")]
static AP_MAIR: AtomicU64 = AtomicU64::new(0);
/// See `AP_MAIR`.
#[cfg(feature = "multiprocessor")]
static AP_TCR: AtomicU64 = AtomicU64::new(0);
/// See `AP_MAIR`.
#[cfg(feature = "multiprocessor")]
static AP_SCTLR: AtomicU64 = AtomicU64::new(0);
/// See `AP_MAIR`: the kernel's `TTBR1_EL1` (`ci_ttbr1`, which the processor cannot read yet).
/// `locore.S`'s `cpu_hatch_secondary` reads it with the MMU off (boot(8)'s entry), so it
/// exists without `MULTIPROCESSOR` too (the assembly names it either way).
pub(crate) static AP_TTBR1: AtomicU64 = AtomicU64::new(0);

/// `cpu_suspended`: drivers clear it to wake a suspended machine.
pub static CPU_SUSPENDED: AtomicI32 = AtomicI32::new(0);
/// `cpu_suspend_cycle_fcn` (`SUSPEND`): how a halted or suspended CPU waits.
pub static CPU_SUSPEND_CYCLE_FCN: StaticCell<fn()> = StaticCell::new(cpu_wfi);

/// A C string in a buffer, up to its first NUL.
fn cstr(s: &[u8]) -> &[u8] {
    &s[..s.iter().position(|&c| c == 0).unwrap_or(s.len())]
}

/// `__builtin_arm_rndrrs`/`__builtin_arm_rndr`: a random number, `None` when the hardware
/// has none to give right now.
fn rndr(reseeded: bool) -> Option<u64> {
    let val: u64;
    let ok: u64;
    // SAFETY: `RNDRRS`/`RNDR` (by encoding, the assembler may not know FEAT_RNG) only read
    // a random number and set the flags; `cpu_attach` arms this only when ID_AA64ISAR0 says
    // the registers exist.
    unsafe {
        if reseeded {
            asm!("mrs {v}, s3_3_c2_c4_1", "cset {ok}, ne", v = out(reg) val, ok = out(reg) ok,
                options(nomem, nostack));
        } else {
            asm!("mrs {v}, s3_3_c2_c4_0", "cset {ok}, ne", v = out(reg) val, ok = out(reg) ok,
                options(nomem, nostack));
        }
    }
    (ok != 0).then_some(val)
}

/// `cpu_rng`: two 32-bit samples of `RNDR` into the entropy pool, again in a second when
/// `arg` is the timeout.
pub fn cpu_rng(arg: *mut c_void) {
    if let Some(rndr) = rndr(true).or_else(|| rndr(false)) {
        crate::dev::rnd::enqueue_randomness((rndr & 0xffff_ffff) as u32);
        crate::dev::rnd::enqueue_randomness((rndr >> 32) as u32);
    }

    // SAFETY: `cpu_attach` passes `cpu_rng_to` itself (a static) or nothing.
    if let Some(to) = unsafe { arg.cast::<Timeout>().as_ref() } {
        timeout_add_msec(to, 1000);
    }
}

/// `cpu_mitigate_spectre_v2`: enable mitigation for Spectre-V2 branch target injection
/// vulnerabilities (CVE-2017-5715).
pub fn cpu_mitigate_spectre_v2(ci: &CpuInfo) {
    // By default we let the firmware decide what mitigation is necessary.
    ci.ci_flush_bp.set(Some(cpu_flush_bp_psci));

    // Some specific CPUs are known not to be vulnerable.
    let midr = ci.ci_midr.get();
    match (cpu_impl(midr), cpu_part(midr)) {
        (CPU_IMPL_ARM, CORTEX_A35 | CORTEX_A53 | CORTEX_A55) | (CPU_IMPL_QCOM, KRYO400_SILVER) => {
            // Not vulnerable.
            ci.ci_flush_bp.set(Some(cpu_flush_bp_noop));
        }
        _ => {}
    }

    // The architecture has been updated to explicitly tell us if we're not vulnerable to
    // Spectre-V2.
    let id = read_specialreg!("id_aa64pfr0_el1");
    if id & ID_AA64PFR0_CSV2_MASK >= ID_AA64PFR0_CSV2_IMPL {
        ci.ci_flush_bp.set(Some(cpu_flush_bp_noop));
    }
}

/// `cpu_mitigate_spectre_bhb`: enable mitigation for Spectre-BHB branch history injection
/// vulnerabilities (CVE-2022-23960).
pub fn cpu_mitigate_spectre_bhb(ci: &CpuInfo) {
    // If we know the CPU, we can add a branchy loop that cleans the BHB.
    let midr = ci.ci_midr.get();
    match (cpu_impl(midr), cpu_part(midr)) {
        (CPU_IMPL_ARM, CORTEX_A57 | CORTEX_A72) => {
            ci.ci_trampoline_vectors.set(TRAMPOLINE_VECTORS_LOOP_8);
        }
        (CPU_IMPL_ARM, CORTEX_A76 | CORTEX_A76AE | CORTEX_A77 | NEOVERSE_N1) => {
            ci.ci_trampoline_vectors.set(TRAMPOLINE_VECTORS_LOOP_24);
        }
        (
            CPU_IMPL_ARM,
            CORTEX_A78 | CORTEX_A78AE | CORTEX_A78C | CORTEX_X1 | CORTEX_X1C | CORTEX_X2
            | CORTEX_A710 | NEOVERSE_N2 | NEOVERSE_V1,
        ) => {
            ci.ci_trampoline_vectors.set(TRAMPOLINE_VECTORS_LOOP_32);
        }
        (
            CPU_IMPL_ARM,
            CORTEX_X3 | CORTEX_X4 | CORTEX_X925 | NEOVERSE_V2 | NEOVERSE_V3 | NEOVERSE_V3AE,
        ) => {
            ci.ci_trampoline_vectors.set(TRAMPOLINE_VECTORS_LOOP_132);
        }
        (CPU_IMPL_AMPERE, AMPERE1_AC03) => {
            ci.ci_trampoline_vectors.set(TRAMPOLINE_VECTORS_LOOP_11);
        }
        _ => {}
    }

    // If we're not using a loop, let firmware decide. This also covers the original
    // Spectre-V2 in addition to Spectre-BHB: smccc_needs_arch_workaround_3() and the
    // trampoline_vectors_psci_{hvc,smc} tables, NPSCI > 0 only.

    // Prefer CLRBHB to mitigate Spectre-BHB.
    let id = read_specialreg!("id_aa64isar2_el1");
    if id & ID_AA64ISAR2_CLRBHB_MASK >= ID_AA64ISAR2_CLRBHB_IMPL {
        ci.ci_trampoline_vectors.set(TRAMPOLINE_VECTORS_CLRBHB);
    }

    // ECBHB tells us Spectre-BHB is mitigated.
    let id = read_specialreg!("id_aa64mmfr1_el1");
    if id & ID_AA64MMFR1_ECBHB_MASK >= ID_AA64MMFR1_ECBHB_IMPL {
        ci.ci_trampoline_vectors.set(TRAMPOLINE_VECTORS_NONE);
    }

    // The architecture has been updated to explicitly tell us if we're not vulnerable to
    // Spectre-BHB.
    let id = read_specialreg!("id_aa64pfr0_el1");
    if id & ID_AA64PFR0_CSV2_MASK >= ID_AA64PFR0_CSV2_HCXT {
        ci.ci_trampoline_vectors.set(TRAMPOLINE_VECTORS_NONE);
    }
}

/// `cpu_mitigate_spectre_v4`: enable mitigation for Spectre-V4 speculative store bypass
/// vulnerabilities (CVE-2018-3639).
pub fn cpu_mitigate_spectre_v4(ci: &CpuInfo) {
    let midr = ci.ci_midr.get();
    match (cpu_impl(midr), cpu_part(midr)) {
        (CPU_IMPL_ARM, CORTEX_A35 | CORTEX_A53 | CORTEX_A55) | (CPU_IMPL_QCOM, KRYO400_SILVER) => {
            // Not vulnerable.
            return;
        }
        _ => {}
    }

    // SSBS tells us Spectre-V4 is mitigated.
    let id = read_specialreg!("id_aa64pfr1_el1");
    if id & ID_AA64PFR1_SSBS_MASK >= ID_AA64PFR1_SSBS_PSTATE {
        return;
    }

    // Enable firmware workaround if required.
    smccc_enable_arch_workaround_2();
}

/// `cpu_mitigate_cve_2025_10263`: enable mitigation for TLB invalidation vulnerabilities
/// (CVE-2025-10263). The workaround for this vulnerability needs to be NOP-ed out on
/// hardware that isn't vulnerable since the cost is too high.
pub fn cpu_mitigate_cve_2025_10263(ci: &CpuInfo) {
    let midr = ci.ci_midr.get();

    let keep = match (cpu_impl(midr), cpu_part(midr)) {
        // Vulnerable.
        (
            CPU_IMPL_ARM,
            C1_PREMIUM | C1_ULTRA | CORTEX_A76 | CORTEX_A76AE | CORTEX_A77 | CORTEX_A78
            | CORTEX_A78AE | CORTEX_A78C | CORTEX_X1 | CORTEX_X1C | CORTEX_X2 | CORTEX_X3
            | CORTEX_X4 | CORTEX_X925 | NEOVERSE_N1 | NEOVERSE_N2 | NEOVERSE_V1 | NEOVERSE_V2
            | NEOVERSE_V3 | NEOVERSE_V3AE,
        ) => true,
        // Not vulnerable, but mitigation works around ARM erratum #2441007.
        (CPU_IMPL_ARM, CORTEX_A55) => true,
        // Not vulnerable, but mitigation works around ARM erratum #2441009 (fixed in r1p2).
        (CPU_IMPL_ARM, CORTEX_A510) => {
            cpu_var(midr) == 0 || (cpu_var(midr) == 1 && cpu_rev(midr) < 2)
        }
        // Vulnerable.
        (CPU_IMPL_NVIDIA, OLYMPUS) => true,
        // Cortex-A76 derived, so probably vulnerable; Cortex-A55 derived, so ARM erratum
        // #2441007 probably applies.
        (CPU_IMPL_QCOM, KRYO400_GOLD | KRYO400_SILVER) => true,
        // Vulnerable.
        (CPU_IMPL_MICROSOFT, NEOVERSE_N2) => true,
        _ => false,
    };

    // When not `keep`: codepatch_nop(CPTAG_REPEAT_TLBI). The TLBIs are not repeated yet
    // (cpufunc.rs), which is what the patch makes of them; a vulnerable CPU keeps that gap
    // (see the module's deviations).
    let _ = keep;
}

/// One line of ID register features: `sep` before a new feature, nothing before a `+`
/// refinement, as the C's `printf("%sNAME", sep); sep = ",";` pairs do.
struct FeatureLine {
    sep: &'static str,
}

impl FeatureLine {
    /// A new feature `name` when `cond`; returns `cond`.
    fn feat(&mut self, cond: bool, name: &str) -> bool {
        if cond {
            kprintf!("{}{}", self.sep, name);
            self.sep = ",";
        }
        cond
    }

    /// A refinement of the previous feature (`"+NAME"`) when `cond`.
    fn plus(&self, cond: bool, name: &str) {
        if cond {
            kprintf!("{}", name);
        }
    }
}

/// `id_aa64zfr0_el1` by its encoding: the assembler wants SVE enabled for the name.
fn read_id_aa64zfr0() -> u64 {
    read_specialreg!("s3_0_c0_c4_4")
}

/// The ID registers `cpu_identify` compares with the previous CPU's.
fn id_regs() -> [u64; 9] {
    [
        read_specialreg!("id_aa64isar0_el1"),
        read_specialreg!("id_aa64isar1_el1"),
        read_specialreg!("id_aa64isar2_el1"),
        read_specialreg!("id_aa64mmfr0_el1"),
        read_specialreg!("id_aa64mmfr1_el1"),
        read_specialreg!("id_aa64mmfr2_el1"),
        read_specialreg!("id_aa64pfr0_el1"),
        read_specialreg!("id_aa64pfr1_el1"),
        read_id_aa64zfr0(),
    ]
}

/// `CCSIDR_EL1` of cache level `i` (the instruction side with `insn`).
fn read_ccsidr(i: u64, insn: bool) -> u64 {
    let csselr = (i << CSSELR_LEVEL_SHIFT) | if insn { CSSELR_IND } else { 0 };
    // SAFETY: selecting the cache whose size registers we read changes nothing else; the
    // `isb` makes the selection visible to the read.
    unsafe {
        write_specialreg!("csselr_el1", csselr);
        asm!("isb", options(nomem, nostack, preserves_flags));
    }
    read_specialreg!("ccsidr_el1")
}

/// `(sets, ways, line)` of a `CCSIDR_EL1` value, in the 64-bit format when `ccidx`.
fn cache_geometry(ccsidr: u64, ccidx: bool) -> (u32, u32, u32) {
    if ccidx {
        (
            ccsidr_ccidx_sets(ccsidr),
            ccsidr_ccidx_ways(ccsidr),
            ccsidr_ccidx_line_size(ccsidr),
        )
    } else {
        (
            ccsidr_sets(ccsidr),
            ccsidr_ways(ccsidr),
            ccsidr_line_size(ccsidr),
        )
    }
}

/// `cpu_identify`: prints the core, its caches and (when they differ from the previous
/// CPU's) the features of its ID registers; picks the mitigations.
pub fn cpu_identify(ci: &CpuInfo) {
    /// `prev_id_aa64*`: the previous CPU's ID registers (CPUs identify one at a time).
    static PREV_ID: [AtomicU64; 9] = [const { AtomicU64::new(0) }; 9];

    // SAFETY: `cpu_attach` set `ci_dev` to the CPU's device before identifying it.
    let xname = unsafe { ci.ci_dev.get().as_ref() }.map_or("cpu", |dev| dev.xname());

    let midr = read_specialreg!("midr_el1");
    let impl_ = cpu_impl(midr);
    let part = cpu_part(midr);
    ci.ci_midr.set(midr);

    let implementer = CPU_IMPLEMENTERS.iter().find(|i| i.id == impl_);
    let impl_name = implementer.map(|i| i.name);
    let coreselecter: &[CpuCores] = implementer.map_or(&CPU_CORES_NONE, |i| i.corelist);
    let part_name = coreselecter.iter().find(|c| c.id == part).map(|c| c.name);

    match (impl_name, part_name) {
        (Some(impl_name), Some(part_name)) => {
            kprintf!(
                " {} {} r{}p{}",
                impl_name,
                part_name,
                cpu_var(midr),
                cpu_rev(midr)
            );

            if cpu_is_primary(ci) {
                // SAFETY: the boot CPU identifies itself once, at attach, before anything
                // reads `cpu_model`.
                let model = unsafe { CPU_MODEL.get_mut() };
                snprintf(
                    model,
                    format_args!(
                        "{} {} r{}p{}",
                        impl_name,
                        part_name,
                        cpu_var(midr),
                        cpu_rev(midr)
                    ),
                );
            }
        }
        _ => {
            kprintf!(" Unknown, MIDR 0x{:x}", midr);

            if cpu_is_primary(ci) {
                // SAFETY: as above.
                let model = unsafe { CPU_MODEL.get_mut() };
                snprintf(model, format_args!("Unknown"));
            }
        }
    }

    // Print cache information.

    let ctr = read_specialreg!("ctr_el0");
    let mut il1p_name = match ctr & CTR_IL1P_MASK {
        CTR_IL1P_AIVIVT => "AIVIVT ",
        CTR_IL1P_VIPT => "VIPT ",
        CTR_IL1P_PIPT => "PIPT ",
        _ => "",
    };

    let id = read_specialreg!("id_aa64mmfr2_el1");
    let mut clidr = read_specialreg!("clidr_el1");
    let ccidx = id & ID_AA64MMFR2_CCIDX_MASK == ID_AA64MMFR2_CCIDX_IMPL;
    if id & ID_AA64MMFR2_CCIDX_MASK > ID_AA64MMFR2_CCIDX_IMPL {
        // Reserved value. Don't print cache information.
        clidr = 0;
    }
    for i in 0..7u64 {
        if clidr & CLIDR_CTYPE_MASK == 0 {
            break;
        }
        kprintf!("\n{}:", xname);
        let mut sep = "";
        if clidr & CLIDR_CTYPE_INSN != 0 {
            let (sets, ways, line) = cache_geometry(read_ccsidr(i, true), ccidx);
            kprintf!(
                "{} {}KB {}b/line {}-way L{} {}I-cache",
                sep,
                (sets * ways * line) / 1024,
                line,
                ways,
                i + 1,
                il1p_name
            );
            il1p_name = "";
            sep = ",";
        }
        if clidr & CLIDR_CTYPE_DATA != 0 {
            let (sets, ways, line) = cache_geometry(read_ccsidr(i, false), ccidx);
            kprintf!(
                "{} {}KB {}b/line {}-way L{} D-cache",
                sep,
                (sets * ways * line) / 1024,
                line,
                ways,
                i + 1
            );
            sep = ",";
        }
        if clidr & CLIDR_CTYPE_UNIFIED != 0 {
            let (sets, ways, line) = cache_geometry(read_ccsidr(i, false), ccidx);
            kprintf!(
                "{} {}KB {}b/line {}-way L{} cache",
                sep,
                (sets * ways * line) / 1024,
                line,
                ways,
                i + 1
            );
        }
        clidr >>= 3;
    }

    cpu_mitigate_spectre_v2(ci);
    cpu_mitigate_spectre_bhb(ci);
    cpu_mitigate_spectre_v4(ci);
    cpu_mitigate_cve_2025_10263(ci);

    // Apple CPUs provide detailed information for SError.
    if impl_ == CPU_IMPL_APPLE {
        ci.ci_serror.set(Some(cpu_serror_apple));
    }

    // Skip printing CPU features if they are identical to the previous CPU.
    let regs = id_regs();
    if regs
        .iter()
        .zip(PREV_ID.iter())
        .all(|(r, p)| *r == p.load(Ordering::Relaxed))
    {
        return;
    }

    // Print CPU features encoded in the ID registers.

    let mismatch = |cur: u64, saved: &AtomicU64, name: &str| {
        if cur != saved.load(Ordering::Relaxed) {
            kprintf!("\n{}: mismatched {}", xname, name);
        }
    };
    mismatch(regs[0], &CPU_ID_AA64ISAR0, "ID_AA64ISAR0_EL1");
    mismatch(regs[1], &CPU_ID_AA64ISAR1, "ID_AA64ISAR1_EL1");
    mismatch(regs[2], &CPU_ID_AA64ISAR2, "ID_AA64ISAR2_EL1");
    mismatch(regs[3], &CPU_ID_AA64MMFR0, "ID_AA64MMFR0_EL1");
    // Allow SpecSEI to be different.
    mismatch(
        regs[4] & !ID_AA64MMFR1_SPECSEI_MASK,
        &CPU_ID_AA64MMFR1,
        "ID_AA64MMFR1_EL1",
    );
    mismatch(regs[5], &CPU_ID_AA64MMFR2, "ID_AA64MMFR2_EL1");
    // Allow CSV2/CVS3 to be different. Ignore 32-bit support in all exception levels.
    mismatch(
        regs[6]
            & !(ID_AA64PFR0_CSV2_MASK
                | ID_AA64PFR0_CSV3_MASK
                | ID_AA64PFR0_EL0_MASK
                | ID_AA64PFR0_EL1_MASK
                | ID_AA64PFR0_EL2_MASK
                | ID_AA64PFR0_EL3_MASK),
        &CPU_ID_AA64PFR0,
        "ID_AA64PFR0_EL1",
    );
    mismatch(regs[7], &CPU_ID_AA64PFR1, "ID_AA64PFR1_EL1");

    kprintf!("\n{}: ", xname);

    let mut p = FeatureLine { sep: "" };

    // ID_AA64ISAR0
    let id = regs[0];
    let f = |mask: u64| id & mask;

    if p.feat(f(ID_AA64ISAR0_RNDR_MASK) >= ID_AA64ISAR0_RNDR_IMPL, "RNDR") {
        ARM64_HAS_RNG.store(1, Ordering::Relaxed);
    }
    p.feat(f(ID_AA64ISAR0_TLB_MASK) >= ID_AA64ISAR0_TLB_IOS, "TLBIOS");
    p.plus(
        f(ID_AA64ISAR0_TLB_MASK) >= ID_AA64ISAR0_TLB_IRANGE,
        "+IRANGE",
    );
    p.feat(f(ID_AA64ISAR0_TS_MASK) >= ID_AA64ISAR0_TS_BASE, "TS");
    p.plus(f(ID_AA64ISAR0_TS_MASK) >= ID_AA64ISAR0_TS_AXFLAG, "+AXFLAG");
    p.feat(f(ID_AA64ISAR0_FHM_MASK) >= ID_AA64ISAR0_FHM_IMPL, "FHM");
    p.feat(f(ID_AA64ISAR0_DP_MASK) >= ID_AA64ISAR0_DP_IMPL, "DP");
    p.feat(f(ID_AA64ISAR0_SM4_MASK) >= ID_AA64ISAR0_SM4_IMPL, "SM4");
    p.feat(f(ID_AA64ISAR0_SM3_MASK) >= ID_AA64ISAR0_SM3_IMPL, "SM3");
    p.feat(f(ID_AA64ISAR0_SHA3_MASK) >= ID_AA64ISAR0_SHA3_IMPL, "SHA3");
    p.feat(f(ID_AA64ISAR0_RDM_MASK) >= ID_AA64ISAR0_RDM_IMPL, "RDM");
    if p.feat(
        f(ID_AA64ISAR0_ATOMIC_MASK) >= ID_AA64ISAR0_ATOMIC_IMPL,
        "Atomic",
    ) {
        ARM64_HAS_LSE.store(1, Ordering::Relaxed);
    }
    p.feat(
        f(ID_AA64ISAR0_CRC32_MASK) >= ID_AA64ISAR0_CRC32_BASE,
        "CRC32",
    );
    p.feat(f(ID_AA64ISAR0_SHA2_MASK) >= ID_AA64ISAR0_SHA2_BASE, "SHA2");
    p.plus(
        f(ID_AA64ISAR0_SHA2_MASK) >= ID_AA64ISAR0_SHA2_512,
        "+SHA512",
    );
    p.feat(f(ID_AA64ISAR0_SHA1_MASK) >= ID_AA64ISAR0_SHA1_BASE, "SHA1");
    if p.feat(f(ID_AA64ISAR0_AES_MASK) >= ID_AA64ISAR0_AES_BASE, "AES") {
        // CRYPTO
        ARM64_HAS_AES.store(1, Ordering::Relaxed);
    }
    p.plus(f(ID_AA64ISAR0_AES_MASK) >= ID_AA64ISAR0_AES_PMULL, "+PMULL");

    // ID_AA64ISAR1
    let id = regs[1];
    let f = |mask: u64| id & mask;

    p.feat(f(ID_AA64ISAR1_LS64_MASK) >= ID_AA64ISAR1_LS64_BASE, "LS64");
    p.plus(f(ID_AA64ISAR1_LS64_MASK) >= ID_AA64ISAR1_LS64_V, "+V");
    p.plus(
        f(ID_AA64ISAR1_LS64_MASK) >= ID_AA64ISAR1_LS64_ACCDATA,
        "+ACCDATA",
    );
    p.feat(f(ID_AA64ISAR1_XS_MASK) >= ID_AA64ISAR1_XS_IMPL, "XS");
    p.feat(f(ID_AA64ISAR1_I8MM_MASK) >= ID_AA64ISAR1_I8MM_IMPL, "I8MM");
    p.feat(f(ID_AA64ISAR1_DGH_MASK) >= ID_AA64ISAR1_DGH_IMPL, "DGH");
    p.feat(f(ID_AA64ISAR1_BF16_MASK) >= ID_AA64ISAR1_BF16_BASE, "BF16");
    p.plus(f(ID_AA64ISAR1_BF16_MASK) >= ID_AA64ISAR1_BF16_EBF, "+EBF");
    p.feat(
        f(ID_AA64ISAR1_SPECRES_MASK) >= ID_AA64ISAR1_SPECRES_IMPL,
        "SPECRES",
    );
    p.feat(f(ID_AA64ISAR1_SB_MASK) >= ID_AA64ISAR1_SB_IMPL, "SB");
    p.feat(
        f(ID_AA64ISAR1_FRINTTS_MASK) >= ID_AA64ISAR1_FRINTTS_IMPL,
        "FRINTTS",
    );
    p.feat(f(ID_AA64ISAR1_GPI_MASK) >= ID_AA64ISAR1_GPI_IMPL, "GPI");
    p.feat(f(ID_AA64ISAR1_GPA_MASK) >= ID_AA64ISAR1_GPA_IMPL, "GPA");
    p.feat(
        f(ID_AA64ISAR1_LRCPC_MASK) >= ID_AA64ISAR1_LRCPC_BASE,
        "LRCPC",
    );
    p.plus(
        f(ID_AA64ISAR1_LRCPC_MASK) >= ID_AA64ISAR1_LRCPC_LDAPUR,
        "+LDAPUR",
    );
    p.feat(f(ID_AA64ISAR1_FCMA_MASK) >= ID_AA64ISAR1_FCMA_IMPL, "FCMA");
    p.feat(
        f(ID_AA64ISAR1_JSCVT_MASK) >= ID_AA64ISAR1_JSCVT_IMPL,
        "JSCVT",
    );
    pac_features(
        &mut p,
        "API",
        f(ID_AA64ISAR1_API_MASK),
        [
            ID_AA64ISAR1_API_PAC,
            ID_AA64ISAR1_API_EPAC,
            ID_AA64ISAR1_API_EPAC2,
            ID_AA64ISAR1_API_FPAC,
            ID_AA64ISAR1_API_FPAC_COMBINED,
        ],
    );
    pac_features(
        &mut p,
        "APA",
        f(ID_AA64ISAR1_APA_MASK),
        [
            ID_AA64ISAR1_APA_PAC,
            ID_AA64ISAR1_APA_EPAC,
            ID_AA64ISAR1_APA_EPAC2,
            ID_AA64ISAR1_APA_FPAC,
            ID_AA64ISAR1_APA_FPAC_COMBINED,
        ],
    );
    p.feat(f(ID_AA64ISAR1_DPB_MASK) >= ID_AA64ISAR1_DPB_IMPL, "DPB");
    p.plus(
        f(ID_AA64ISAR1_DPB_MASK) >= ID_AA64ISAR1_DPB_DCCVADP,
        "+DCCVADP",
    );

    // ID_AA64ISAR2
    let id = regs[2];
    let f = |mask: u64| id & mask;

    p.feat(f(ID_AA64ISAR2_CSSC_MASK) >= ID_AA64ISAR2_CSSC_IMPL, "CSSC");
    p.feat(
        f(ID_AA64ISAR2_RPRFM_MASK) >= ID_AA64ISAR2_RPRFM_IMPL,
        "RPRFM",
    );
    p.feat(
        f(ID_AA64ISAR2_CLRBHB_MASK) >= ID_AA64ISAR2_CLRBHB_IMPL,
        "CLRBHB",
    );
    p.feat(f(ID_AA64ISAR2_BC_MASK) >= ID_AA64ISAR2_BC_IMPL, "BC");
    p.feat(f(ID_AA64ISAR2_MOPS_MASK) >= ID_AA64ISAR2_MOPS_IMPL, "MOPS");
    p.feat(f(ID_AA64ISAR2_GPA3_MASK) >= ID_AA64ISAR2_GPA3_IMPL, "GPA3");
    pac_features(
        &mut p,
        "APA3",
        f(ID_AA64ISAR2_APA3_MASK),
        [
            ID_AA64ISAR2_APA3_PAC,
            ID_AA64ISAR2_APA3_EPAC,
            ID_AA64ISAR2_APA3_EPAC2,
            ID_AA64ISAR2_APA3_FPAC,
            ID_AA64ISAR2_APA3_FPAC_COMBINED,
        ],
    );
    p.feat(
        f(ID_AA64ISAR2_RPRES_MASK) >= ID_AA64ISAR2_RPRES_IMPL,
        "RPRES",
    );
    p.feat(f(ID_AA64ISAR2_WFXT_MASK) >= ID_AA64ISAR2_WFXT_IMPL, "WFXT");

    // ID_AA64MMFR0
    //
    // We only print ASIDBits for now.
    let id = regs[3];
    let f = |mask: u64| id & mask;

    p.feat(f(ID_AA64MMFR0_ECV_MASK) >= ID_AA64MMFR0_ECV_IMPL, "ECV");
    p.plus(
        f(ID_AA64MMFR0_ECV_MASK) >= ID_AA64MMFR0_ECV_CNTHCTL,
        "+CNTHCTL",
    );
    p.feat(
        f(ID_AA64MMFR0_ASID_BITS_MASK) == ID_AA64MMFR0_ASID_BITS_16,
        "ASID16",
    );

    // ID_AA64MMFR1
    //
    // We omit printing most virtualization related fields for now.
    let id = regs[4];
    let f = |mask: u64| id & mask;

    p.feat(f(ID_AA64MMFR1_AFP_MASK) >= ID_AA64MMFR1_AFP_IMPL, "AFP");
    p.feat(
        f(ID_AA64MMFR1_SPECSEI_MASK) >= ID_AA64MMFR1_SPECSEI_IMPL,
        "SpecSEI",
    );
    p.feat(f(ID_AA64MMFR1_PAN_MASK) >= ID_AA64MMFR1_PAN_IMPL, "PAN");
    p.plus(
        f(ID_AA64MMFR1_PAN_MASK) >= ID_AA64MMFR1_PAN_ATS1E1,
        "+ATS1E1",
    );
    p.plus(f(ID_AA64MMFR1_PAN_MASK) >= ID_AA64MMFR1_PAN_EPAN, "+EPAN");
    p.feat(f(ID_AA64MMFR1_LO_MASK) >= ID_AA64MMFR1_LO_IMPL, "LO");
    p.feat(f(ID_AA64MMFR1_HPDS_MASK) >= ID_AA64MMFR1_HPDS_IMPL, "HPDS");
    p.feat(f(ID_AA64MMFR1_VH_MASK) >= ID_AA64MMFR1_VH_IMPL, "VH");
    p.feat(f(ID_AA64MMFR1_HAFDBS_MASK) >= ID_AA64MMFR1_HAFDBS_AF, "HAF");
    p.plus(
        f(ID_AA64MMFR1_HAFDBS_MASK) >= ID_AA64MMFR1_HAFDBS_AF_DBS,
        "DBS",
    );
    p.feat(
        f(ID_AA64MMFR1_ECBHB_MASK) >= ID_AA64MMFR1_ECBHB_IMPL,
        "ECBHB",
    );

    // ID_AA64MMFR2
    let id = regs[5];
    let f = |mask: u64| id & mask;

    p.feat(f(ID_AA64MMFR2_IDS_MASK) >= ID_AA64MMFR2_IDS_IMPL, "IDS");
    p.feat(f(ID_AA64MMFR2_AT_MASK) >= ID_AA64MMFR2_AT_IMPL, "AT");

    // ID_AA64PFR0
    let id = regs[6];
    let f = |mask: u64| id & mask;

    p.feat(f(ID_AA64PFR0_CSV3_MASK) >= ID_AA64PFR0_CSV3_IMPL, "CSV3");
    p.feat(f(ID_AA64PFR0_CSV2_MASK) >= ID_AA64PFR0_CSV2_IMPL, "CSV2");
    p.plus(f(ID_AA64PFR0_CSV2_MASK) >= ID_AA64PFR0_CSV2_SCXT, "+SCXT");
    p.plus(f(ID_AA64PFR0_CSV2_MASK) >= ID_AA64PFR0_CSV2_HCXT, "+HCXT");
    p.feat(f(ID_AA64PFR0_DIT_MASK) >= ID_AA64PFR0_DIT_IMPL, "DIT");
    if p.feat(f(ID_AA64PFR0_AMU_MASK) >= ID_AA64PFR0_AMU_IMPL, "AMU") {
        p.plus(f(ID_AA64PFR0_AMU_MASK) >= ID_AA64PFR0_AMU_IMPL_V1P1, "v1p1");
    }
    if p.feat(f(ID_AA64PFR0_RAS_MASK) >= ID_AA64PFR0_RAS_IMPL, "RAS") {
        p.plus(f(ID_AA64PFR0_RAS_MASK) >= ID_AA64PFR0_RAS_IMPL_V1P1, "v1p1");
    }
    p.feat(f(ID_AA64PFR0_SVE_MASK) >= ID_AA64PFR0_SVE_IMPL, "SVE");
    p.feat(
        f(ID_AA64PFR0_ADV_SIMD_MASK) != ID_AA64PFR0_ADV_SIMD_NONE
            && f(ID_AA64PFR0_ADV_SIMD_MASK) >= ID_AA64PFR0_ADV_SIMD_HP,
        "AdvSIMD+HP",
    );
    p.feat(
        f(ID_AA64PFR0_FP_MASK) != ID_AA64PFR0_FP_NONE
            && f(ID_AA64PFR0_FP_MASK) >= ID_AA64PFR0_FP_HP,
        "FP+HP",
    );

    // ID_AA64PFR1
    let id = regs[7];
    let f = |mask: u64| id & mask;

    p.feat(f(ID_AA64PFR1_BT_MASK) >= ID_AA64PFR1_BT_IMPL, "BT");
    p.feat(f(ID_AA64PFR1_SSBS_MASK) >= ID_AA64PFR1_SSBS_PSTATE, "SSBS");
    p.plus(
        f(ID_AA64PFR1_SSBS_MASK) >= ID_AA64PFR1_SSBS_PSTATE_MSR,
        "+MSR",
    );
    p.feat(f(ID_AA64PFR1_MTE_MASK) >= ID_AA64PFR1_MTE_IMPL, "MTE");

    // ID_AA64ZFR0
    let id = regs[8];
    let f = |mask: u64| id & mask;

    if id & ID_AA64ZFR0_MASK != 0 {
        kprintf!("\n{}: SVE", xname);
        let z = FeatureLine { sep: "" };
        z.plus(f(ID_AA64ZFR0_SVEVER_MASK) >= ID_AA64ZFR0_SVEVER_SVE2, "2");
        z.plus(
            f(ID_AA64ZFR0_SVEVER_MASK) >= ID_AA64ZFR0_SVEVER_SVE2P1,
            "p1",
        );
        z.plus(
            f(ID_AA64ZFR0_F64MM_MASK) >= ID_AA64ZFR0_F64MM_IMPL,
            ",F64MM",
        );
        z.plus(
            f(ID_AA64ZFR0_F32MM_MASK) >= ID_AA64ZFR0_F32MM_IMPL,
            ",F32MM",
        );
        z.plus(f(ID_AA64ZFR0_I8MM_MASK) >= ID_AA64ZFR0_I8MM_IMPL, ",I8MM");
        z.plus(f(ID_AA64ZFR0_SM4_MASK) >= ID_AA64ZFR0_SM4_IMPL, ",SM4");
        z.plus(f(ID_AA64ZFR0_SHA3_MASK) >= ID_AA64ZFR0_SHA3_IMPL, ",SHA3");
        z.plus(f(ID_AA64ZFR0_BF16_MASK) >= ID_AA64ZFR0_BF16_BASE, ",BF16");
        z.plus(f(ID_AA64ZFR0_BF16_MASK) >= ID_AA64ZFR0_BF16_EBF, "+EBF");
        z.plus(
            f(ID_AA64ZFR0_BITPERM_MASK) >= ID_AA64ZFR0_BITPERM_IMPL,
            ",BitPerm",
        );
        z.plus(f(ID_AA64ZFR0_AES_MASK) >= ID_AA64ZFR0_AES_BASE, ",AES");
        z.plus(f(ID_AA64ZFR0_AES_MASK) >= ID_AA64ZFR0_AES_PMULL, "+PMULL");
    }

    for (prev, r) in PREV_ID.iter().zip(regs) {
        prev.store(r, Ordering::Relaxed);
    }

    // CPU_DEBUG (the raw ID registers): not configured.
}

/// The pointer authentication field `name` of an ID register (`API`, `APA`, `APA3`): the
/// feature, `+EPAC` or `+EPAC2`, `+FPAC` and `+COMBINED`. `levels` are the field's `PAC`,
/// `EPAC`, `EPAC2`, `FPAC` and `FPAC_COMBINED` values.
fn pac_features(p: &mut FeatureLine, name: &str, field: u64, levels: [u64; 5]) {
    let [pac, epac, epac2, fpac, combined] = levels;
    p.feat(field >= pac, name);
    if field == epac {
        p.plus(true, "+EPAC");
    } else {
        p.plus(field >= epac2, "+EPAC2");
    }
    p.plus(field >= fpac, "+FPAC");
    p.plus(field >= combined, "+COMBINED");
}

/// `cpu_identify_cleanup`: reduces the `cpu_id_aa64*` copies to what userland may see
/// (`emulate_msr`, `machdep.id_aa64*`) and computes `hwcap`/`hwcap2` from them; called once
/// every CPU has attached.
pub fn cpu_identify_cleanup() {
    let load = |a: &AtomicU64| a.load(Ordering::Relaxed);

    // ID_AA64ISAR0_EL1
    let mut value = load(&CPU_ID_AA64ISAR0) & ID_AA64ISAR0_MASK;
    value &= !ID_AA64ISAR0_TLB_MASK;
    CPU_ID_AA64ISAR0.store(value, Ordering::Relaxed);

    // ID_AA64ISAR1_EL1
    let mut value = load(&CPU_ID_AA64ISAR1) & ID_AA64ISAR1_MASK;
    value &= !ID_AA64ISAR1_SPECRES_MASK;
    CPU_ID_AA64ISAR1.store(value, Ordering::Relaxed);

    // ID_AA64ISAR2_EL1
    let mut value = load(&CPU_ID_AA64ISAR2) & ID_AA64ISAR2_MASK;
    value &= !ID_AA64ISAR2_CLRBHB_MASK;
    CPU_ID_AA64ISAR2.store(value, Ordering::Relaxed);

    // ID_AA64MMFR0_EL1
    let value = load(&CPU_ID_AA64MMFR0) & ID_AA64MMFR0_ECV_MASK;
    CPU_ID_AA64MMFR0.store(value, Ordering::Relaxed);

    // ID_AA64MMFR1_EL1
    let value = load(&CPU_ID_AA64MMFR1) & ID_AA64MMFR1_AFP_MASK;
    CPU_ID_AA64MMFR1.store(value, Ordering::Relaxed);

    // ID_AA64MMFR2_EL1
    let value = load(&CPU_ID_AA64MMFR2) & ID_AA64MMFR2_AT_MASK;
    CPU_ID_AA64MMFR2.store(value, Ordering::Relaxed);

    // ID_AA64PFR0_EL1
    let value = load(&CPU_ID_AA64PFR0)
        & (ID_AA64PFR0_FP_MASK
            | ID_AA64PFR0_ADV_SIMD_MASK
            | ID_AA64PFR0_SVE_MASK
            | ID_AA64PFR0_DIT_MASK);
    CPU_ID_AA64PFR0.store(value, Ordering::Relaxed);

    // ID_AA64PFR1_EL1
    let value = load(&CPU_ID_AA64PFR1) & (ID_AA64PFR1_BT_MASK | ID_AA64PFR1_SSBS_MASK);
    CPU_ID_AA64PFR1.store(value, Ordering::Relaxed);

    // ID_AA64ZFR0_EL1
    let value = load(&CPU_ID_AA64ZFR0) & ID_AA64ZFR0_MASK;
    CPU_ID_AA64ZFR0.store(value, Ordering::Relaxed);

    let isar0 = load(&CPU_ID_AA64ISAR0);
    let isar1 = load(&CPU_ID_AA64ISAR1);
    let isar2 = load(&CPU_ID_AA64ISAR2);
    let mmfr0 = load(&CPU_ID_AA64MMFR0);
    let mmfr1 = load(&CPU_ID_AA64MMFR1);
    let mmfr2 = load(&CPU_ID_AA64MMFR2);
    let pfr0 = load(&CPU_ID_AA64PFR0);
    let pfr1 = load(&CPU_ID_AA64PFR1);
    let zfr0 = load(&CPU_ID_AA64ZFR0);
    let id_aa64mmfr2 = read_specialreg!("id_aa64mmfr2_el1");

    // HWCAP
    let mut hwcap = HWCAP.load(Ordering::Relaxed);
    let mut cap = |cond: bool, bit: u64| {
        if cond {
            hwcap |= bit;
        }
    };
    cap(true, HWCAP_FP); // OpenBSD assumes Floating-point support
    cap(true, HWCAP_ASIMD); // OpenBSD assumes Advanced SIMD support
    // HWCAP_EVTSTRM: OpenBSD kernel doesn't configure event stream
    cap(
        isar0 & ID_AA64ISAR0_AES_MASK >= ID_AA64ISAR0_AES_BASE,
        HWCAP_AES,
    );
    cap(
        isar0 & ID_AA64ISAR0_AES_MASK >= ID_AA64ISAR0_AES_PMULL,
        HWCAP_PMULL,
    );
    cap(
        isar0 & ID_AA64ISAR0_SHA1_MASK >= ID_AA64ISAR0_SHA1_BASE,
        HWCAP_SHA1,
    );
    cap(
        isar0 & ID_AA64ISAR0_SHA2_MASK >= ID_AA64ISAR0_SHA2_BASE,
        HWCAP_SHA2,
    );
    cap(
        isar0 & ID_AA64ISAR0_CRC32_MASK >= ID_AA64ISAR0_CRC32_BASE,
        HWCAP_CRC32,
    );
    cap(
        isar0 & ID_AA64ISAR0_ATOMIC_MASK >= ID_AA64ISAR0_ATOMIC_IMPL,
        HWCAP_ATOMICS,
    );
    cap(
        pfr0 & ID_AA64PFR0_FP_MASK != ID_AA64PFR0_FP_NONE
            && pfr0 & ID_AA64PFR0_FP_MASK >= ID_AA64PFR0_FP_HP,
        HWCAP_FPHP,
    );
    cap(
        pfr0 & ID_AA64PFR0_ADV_SIMD_MASK != ID_AA64PFR0_ADV_SIMD_NONE
            && pfr0 & ID_AA64PFR0_ADV_SIMD_MASK >= ID_AA64PFR0_ADV_SIMD_HP,
        HWCAP_ASIMDHP,
    );
    cap(
        id_aa64mmfr2 & ID_AA64MMFR2_IDS_MASK >= ID_AA64MMFR2_IDS_IMPL,
        HWCAP_CPUID,
    );
    cap(
        isar0 & ID_AA64ISAR0_RDM_MASK >= ID_AA64ISAR0_RDM_IMPL,
        HWCAP_ASIMDRDM,
    );
    cap(
        isar1 & ID_AA64ISAR1_JSCVT_MASK >= ID_AA64ISAR1_JSCVT_IMPL,
        HWCAP_JSCVT,
    );
    cap(
        isar1 & ID_AA64ISAR1_FCMA_MASK >= ID_AA64ISAR1_FCMA_IMPL,
        HWCAP_FCMA,
    );
    cap(
        isar1 & ID_AA64ISAR1_LRCPC_MASK >= ID_AA64ISAR1_LRCPC_BASE,
        HWCAP_LRCPC,
    );
    cap(
        isar1 & ID_AA64ISAR1_DPB_MASK >= ID_AA64ISAR1_DPB_IMPL,
        HWCAP_DCPOP,
    );
    cap(
        isar0 & ID_AA64ISAR0_SHA3_MASK >= ID_AA64ISAR0_SHA3_IMPL,
        HWCAP_SHA3,
    );
    cap(
        isar0 & ID_AA64ISAR0_SM3_MASK >= ID_AA64ISAR0_SM3_IMPL,
        HWCAP_SM3,
    );
    cap(
        isar0 & ID_AA64ISAR0_SM4_MASK >= ID_AA64ISAR0_SM4_IMPL,
        HWCAP_SM4,
    );
    cap(
        isar0 & ID_AA64ISAR0_DP_MASK >= ID_AA64ISAR0_DP_IMPL,
        HWCAP_ASIMDDP,
    );
    cap(
        isar0 & ID_AA64ISAR0_SHA2_MASK >= ID_AA64ISAR0_SHA2_512,
        HWCAP_SHA512,
    );
    cap(
        pfr0 & ID_AA64PFR0_SVE_MASK >= ID_AA64PFR0_SVE_IMPL,
        HWCAP_SVE,
    );
    cap(
        isar0 & ID_AA64ISAR0_FHM_MASK >= ID_AA64ISAR0_FHM_IMPL,
        HWCAP_ASIMDFHM,
    );
    cap(
        pfr0 & ID_AA64PFR0_DIT_MASK >= ID_AA64PFR0_DIT_IMPL,
        HWCAP_DIT,
    );
    cap(
        mmfr2 & ID_AA64MMFR2_AT_MASK >= ID_AA64MMFR2_AT_IMPL,
        HWCAP_USCAT,
    );
    cap(
        isar1 & ID_AA64ISAR1_LRCPC_MASK >= ID_AA64ISAR1_LRCPC_LDAPUR,
        HWCAP_ILRCPC,
    );
    cap(
        isar0 & ID_AA64ISAR0_TS_MASK >= ID_AA64ISAR0_TS_BASE,
        HWCAP_FLAGM,
    );
    cap(
        pfr1 & ID_AA64PFR1_SSBS_MASK >= ID_AA64PFR1_SSBS_PSTATE_MSR,
        HWCAP_SSBS,
    );
    cap(
        isar1 & ID_AA64ISAR1_SB_MASK >= ID_AA64ISAR1_SB_IMPL,
        HWCAP_SB,
    );
    cap(
        isar1 & ID_AA64ISAR1_APA_MASK >= ID_AA64ISAR1_APA_PAC
            || isar1 & ID_AA64ISAR1_API_MASK >= ID_AA64ISAR1_API_PAC
            || isar2 & ID_AA64ISAR2_APA3_MASK >= ID_AA64ISAR2_APA3_PAC,
        HWCAP_PACA,
    );
    cap(
        isar1 & ID_AA64ISAR1_GPA_MASK >= ID_AA64ISAR1_GPA_IMPL
            || isar1 & ID_AA64ISAR1_GPI_MASK >= ID_AA64ISAR1_GPI_IMPL
            || isar2 & ID_AA64ISAR2_GPA3_MASK >= ID_AA64ISAR2_GPA3_IMPL,
        HWCAP_PACG,
    );
    HWCAP.store(hwcap, Ordering::Relaxed);

    // HWCAP2
    let sve = hwcap & HWCAP_SVE != 0;
    let mut hwcap2 = HWCAP2.load(Ordering::Relaxed);
    let mut cap2 = |cond: bool, bit: u64| {
        if cond {
            hwcap2 |= bit;
        }
    };
    cap2(
        isar1 & ID_AA64ISAR1_DPB_MASK >= ID_AA64ISAR1_DPB_DCCVADP,
        HWCAP2_DCPODP,
    );
    cap2(
        sve && zfr0 & ID_AA64ZFR0_SVEVER_MASK >= ID_AA64ZFR0_SVEVER_SVE2,
        HWCAP2_SVE2,
    );
    cap2(
        sve && zfr0 & ID_AA64ZFR0_AES_MASK >= ID_AA64ZFR0_AES_BASE,
        HWCAP2_SVEAES,
    );
    cap2(
        sve && zfr0 & ID_AA64ZFR0_AES_MASK >= ID_AA64ZFR0_AES_PMULL,
        HWCAP2_SVEPMULL,
    );
    cap2(
        sve && zfr0 & ID_AA64ZFR0_BITPERM_MASK >= ID_AA64ZFR0_BITPERM_IMPL,
        HWCAP2_SVEBITPERM,
    );
    cap2(
        sve && zfr0 & ID_AA64ZFR0_SHA3_MASK >= ID_AA64ZFR0_SHA3_IMPL,
        HWCAP2_SVESHA3,
    );
    cap2(
        sve && zfr0 & ID_AA64ZFR0_SM4_MASK >= ID_AA64ZFR0_SM4_IMPL,
        HWCAP2_SVESM4,
    );
    cap2(
        isar0 & ID_AA64ISAR0_TS_MASK >= ID_AA64ISAR0_TS_AXFLAG,
        HWCAP2_FLAGM2,
    );
    cap2(
        isar1 & ID_AA64ISAR1_FRINTTS_MASK >= ID_AA64ISAR1_FRINTTS_IMPL,
        HWCAP2_FRINT,
    );
    cap2(
        sve && zfr0 & ID_AA64ZFR0_I8MM_MASK >= ID_AA64ZFR0_I8MM_IMPL,
        HWCAP2_SVEI8MM,
    );
    cap2(
        sve && zfr0 & ID_AA64ZFR0_F32MM_MASK >= ID_AA64ZFR0_F32MM_IMPL,
        HWCAP2_SVEF32MM,
    );
    cap2(
        sve && zfr0 & ID_AA64ZFR0_F64MM_MASK >= ID_AA64ZFR0_F64MM_IMPL,
        HWCAP2_SVEF64MM,
    );
    cap2(
        sve && zfr0 & ID_AA64ZFR0_BF16_MASK >= ID_AA64ZFR0_BF16_BASE,
        HWCAP2_SVEBF16,
    );
    cap2(
        isar1 & ID_AA64ISAR1_I8MM_MASK >= ID_AA64ISAR1_I8MM_IMPL,
        HWCAP2_I8MM,
    );
    cap2(
        isar1 & ID_AA64ISAR1_BF16_MASK >= ID_AA64ISAR1_BF16_BASE,
        HWCAP2_BF16,
    );
    cap2(
        isar1 & ID_AA64ISAR1_DGH_MASK >= ID_AA64ISAR1_DGH_IMPL,
        HWCAP2_DGH,
    );
    cap2(
        isar0 & ID_AA64ISAR0_RNDR_MASK >= ID_AA64ISAR0_RNDR_IMPL,
        HWCAP2_RNG,
    );
    cap2(
        pfr1 & ID_AA64PFR1_BT_MASK >= ID_AA64PFR1_BT_IMPL,
        HWCAP2_BTI,
    );
    // HWCAP2_MTE: OpenBSD kernel doesn't provide MTE support
    cap2(
        mmfr0 & ID_AA64MMFR0_ECV_MASK >= ID_AA64MMFR0_ECV_IMPL,
        HWCAP2_ECV,
    );
    cap2(
        mmfr1 & ID_AA64MMFR1_AFP_MASK >= ID_AA64MMFR1_AFP_IMPL,
        HWCAP2_AFP,
    );
    cap2(
        isar2 & ID_AA64ISAR2_RPRES_MASK >= ID_AA64ISAR2_RPRES_IMPL,
        HWCAP2_RPRES,
    );
    // HWCAP2_MTE3: OpenBSD kernel doesn't provide MTE support
    // HWCAP2_SME and its variants: OpenBSD kernel doesn't provide SME support
    cap2(
        isar2 & ID_AA64ISAR2_WFXT_MASK >= ID_AA64ISAR2_WFXT_IMPL,
        HWCAP2_WFXT,
    );
    cap2(
        isar1 & ID_AA64ISAR1_BF16_MASK >= ID_AA64ISAR1_BF16_EBF,
        HWCAP2_EBF16,
    );
    cap2(
        sve && zfr0 & ID_AA64ZFR0_BF16_MASK >= ID_AA64ZFR0_BF16_EBF,
        HWCAP2_SVE_EBF16,
    );
    cap2(
        isar2 & ID_AA64ISAR2_CSSC_MASK >= ID_AA64ISAR2_CSSC_IMPL,
        HWCAP2_CSSC,
    );
    cap2(
        isar2 & ID_AA64ISAR2_RPRFM_MASK >= ID_AA64ISAR2_RPRFM_IMPL,
        HWCAP2_RPRFM,
    );
    cap2(
        sve && zfr0 & ID_AA64ZFR0_SVEVER_MASK >= ID_AA64ZFR0_SVEVER_SVE2P1,
        HWCAP2_SVE2P1,
    );
    // HWCAP2_SME2 and its variants: OpenBSD kernel doesn't provide SME support
    cap2(
        isar2 & ID_AA64ISAR2_MOPS_MASK >= ID_AA64ISAR2_MOPS_IMPL,
        HWCAP2_MOPS,
    );
    cap2(
        isar2 & ID_AA64ISAR2_BC_MASK >= ID_AA64ISAR2_BC_IMPL,
        HWCAP2_HBC,
    );
    HWCAP2.store(hwcap2, Ordering::Relaxed);
}

/// `cpu_classify`: performance, efficiency or lethargic cores from their relative
/// capacities.
pub fn cpu_classify() {
    let mut max_capacity = 0u64;

    let mut ci: *const CpuInfo = cpu_info_list();
    // SAFETY: `cpu_info_list` links `cpu_info`s that live forever.
    while let Some(c) = unsafe { ci.as_ref() } {
        max_capacity = max_capacity.max(c.ci_capacity.get());
        ci = c.ci_next.get();
    }

    let mut ci: *const CpuInfo = cpu_info_list();
    // SAFETY: as above.
    while let Some(c) = unsafe { ci.as_ref() } {
        let cap = c.ci_capacity.get();
        let typ = if cap == 0 || 100 * cap > 80 * max_capacity {
            CPUTYP_P
        } else if 100 * cap > 30 * max_capacity {
            CPUTYP_E
        } else {
            CPUTYP_L
        };
        c.ci_cputype.set(typ as u32);
        ci = c.ci_next.get();
    }
}

/// `cpu_match`: a `cpu` node, as long as there is room for it (the CPU we run on always
/// fits).
pub fn cpu_match(_parent: Option<&Device>, _cfdata: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: `cpu` attaches at `mainbus`, which hands over a `FdtAttachArgs`.
    let faa = unsafe { &*aux.cast::<FdtAttachArgs<'_>>() };
    let mpidr = read_specialreg!("mpidr_el1");
    let mut buf = [0u8; 32];

    if OF_getprop(faa.fa_node, b"device_type", &mut buf) <= 0 || cstr(&buf) != b"cpu" {
        return 0;
    }

    let ncpus = crate::kern::init_main::NCPUS.load(Ordering::Relaxed);
    if (ncpus as u32) < crate::arch::arm64::include::cpu::MAXCPUS
        || faa
            .fa_reg
            .first()
            .is_some_and(|r| r.addr == mpidr & MPIDR_AFF)
    {
        return 1;
    }

    0
}

/// `cpu_attach`: sets up the CPU's `cpu_info`; the boot CPU identifies and initialises
/// itself, an application processor (`MULTIPROCESSOR`) is started and identifies itself
/// while this waits.
pub fn cpu_attach(_parent: Option<&Device>, dev: &Device, aux: *mut c_void) {
    // SAFETY: as in `cpu_match`.
    let faa = unsafe { &*aux.cast::<FdtAttachArgs<'_>>() };

    kassert!(!faa.fa_reg.is_empty());

    #[cfg(feature = "multiprocessor")]
    let ci: &'static CpuInfo = {
        let mpidr = read_specialreg!("mpidr_el1");
        if faa.fa_reg[0].addr == mpidr & MPIDR_AFF {
            let ci = cpu_info_list();
            ci.ci_flags.fetch_or(
                CPUF_RUNNING | CPUF_PRESENT | CPUF_PRIMARY,
                Ordering::Relaxed,
            );
            ci
        } else {
            let ci = cpu_info_alloc();
            CPU_INFO[dev.dv_unit.get() as usize].set(ci);
            let mut last = cpu_info_list();
            // SAFETY: the list links `cpu_info`s that live forever.
            while let Some(next) = unsafe { last.ci_next.get().as_ref() } {
                last = next;
            }
            last.ci_next.set(ci);
            ci.ci_flags.fetch_or(CPUF_AP, Ordering::Relaxed);
            NCPUS.fetch_add(1, Ordering::Relaxed);
            ci
        }
    };
    #[cfg(not(feature = "multiprocessor"))]
    let ci: &'static CpuInfo = cpu_info_list();

    ci.ci_dev.set(ptr::from_ref(dev));
    ci.ci_cpuid.set(dev.dv_unit.get() as u32);
    ci.ci_mpidr.set(faa.fa_reg[0].addr);
    ci.ci_node.set(faa.fa_node);
    ci.ci_self.set(ptr::from_ref(ci));

    kprintf!(" mpidr {:x}:", ci.ci_mpidr.get());

    let Some(kstack) = km_alloc(USPACE, &KV_ANY, &KP_ZERO, &KD_WAITOK) else {
        panic(format_args!("cpu_attach: no kernel stack"));
    };
    ci.ci_el1_stkend.set(kstack.as_ptr() as usize + USPACE - 16);
    ci.ci_trampoline_vectors.set(TRAMPOLINE_VECTORS_NONE);

    #[cfg(feature = "multiprocessor")]
    let primary = ci.ci_flags.load(Ordering::Relaxed) & CPUF_AP == 0;
    #[cfg(not(feature = "multiprocessor"))]
    let primary = true;

    #[cfg(feature = "multiprocessor")]
    if !primary {
        cpu_attach_secondary(ci);
    }

    if primary {
        CPU_ID_AA64ISAR0.store(read_specialreg!("id_aa64isar0_el1"), Ordering::Relaxed);
        CPU_ID_AA64ISAR1.store(read_specialreg!("id_aa64isar1_el1"), Ordering::Relaxed);
        CPU_ID_AA64ISAR2.store(read_specialreg!("id_aa64isar2_el1"), Ordering::Relaxed);
        CPU_ID_AA64MMFR0.store(read_specialreg!("id_aa64mmfr0_el1"), Ordering::Relaxed);
        CPU_ID_AA64MMFR2.store(read_specialreg!("id_aa64mmfr2_el1"), Ordering::Relaxed);
        CPU_ID_AA64PFR1.store(read_specialreg!("id_aa64pfr1_el1"), Ordering::Relaxed);
        CPU_ID_AA64ZFR0.store(read_id_aa64zfr0(), Ordering::Relaxed);

        // The SpecSEI "feature" isn't relevant for userland. So it is fine if this field
        // differs between CPU cores. Mask off this field to prevent exporting it to
        // userland.
        CPU_ID_AA64MMFR1.store(
            read_specialreg!("id_aa64mmfr1_el1") & !ID_AA64MMFR1_SPECSEI_MASK,
            Ordering::Relaxed,
        );

        // The CSV2/CSV3 "features" are handled on a per-processor basis. So it is fine if
        // these fields differ between CPU cores. Mask off these fields to prevent exporting
        // these to userland.
        //
        // We only support 64-bit mode, so we don't care about differences in support for
        // 32-bit mode between cores. Mask off these fields as well.
        CPU_ID_AA64PFR0.store(
            read_specialreg!("id_aa64pfr0_el1")
                & !(ID_AA64PFR0_CSV2_MASK
                    | ID_AA64PFR0_CSV3_MASK
                    | ID_AA64PFR0_EL0_MASK
                    | ID_AA64PFR0_EL1_MASK
                    | ID_AA64PFR0_EL2_MASK
                    | ID_AA64PFR0_EL3_MASK),
            Ordering::Relaxed,
        );

        // Lenovo X13s ships with broken EL2 firmware that hangs the machine if we enable
        // PAuth.
        // SAFETY: `hw_vendor`/`hw_prod` are written by mainbus's attach, which precedes the
        // CPUs', and read-only afterwards.
        let (vendor, prod) = unsafe { (hw_vendor.read(), hw_prod.read()) };
        if let (Some(vendor), Some(prod)) = (vendor, prod)
            && cstr(vendor) == b"LENOVO"
            && (prod.starts_with(b"21BX") || prod.starts_with(b"21BY"))
        {
            CPU_ID_AA64ISAR1.fetch_and(
                !(ID_AA64ISAR1_APA_MASK | ID_AA64ISAR1_GPA_MASK),
                Ordering::Relaxed,
            );
        }

        cpu_identify(ci);

        if OF_getproplen(ci.ci_node.get(), b"clocks") > 0 {
            CPU_NODE.store(ci.ci_node.get(), Ordering::Relaxed);
            // SAFETY: autoconfiguration on the boot CPU, before sysctl(2) can read it.
            unsafe { CPU_CPUSPEED.write(Some(cpu_clockspeed)) };
        }

        cpu_init();

        if ARM64_HAS_RNG.load(Ordering::Relaxed) != 0 {
            let to = ptr::from_ref(&CPU_RNG_TO).cast_mut().cast::<c_void>();
            timeout_set(&CPU_RNG_TO, cpu_rng, to);
            cpu_rng(to);
        }
    }

    // NXCALL > 0: cpu_xcall_establish(ci) (MULTIPROCESSOR); NXCALL is 0 on arm64.

    if NKSTAT > 0 {
        cpu_kstat_attach(ci);
    }

    let opp = OF_getpropint(ci.ci_node.get(), b"operating-points-v2", 0);
    if opp != 0 {
        cpu_opp_init(ci, opp);
    }

    ci.ci_capacity.set(u64::from(OF_getpropint(
        ci.ci_node.get(),
        b"capacity-dmips-mhz",
        0,
    )));
    // SAFETY: `cpu_opp_init` points `ci_opp_table` at a table that lives forever.
    if let Some(ot) = unsafe { ci.ci_opp_table.get().as_ref() } {
        ci.ci_capacity
            .set(ci.ci_capacity.get() * (ot.ot_opp_hz_max / 1_000_000));
    }
    cpu_classify();

    cpu_psci_init(ci);

    kprintf!("\n");
}

/// A zeroed `cpu_info` for an application processor (`malloc(9)`, never freed).
#[cfg(feature = "multiprocessor")]
fn cpu_info_alloc() -> &'static CpuInfo {
    let Some(p) = malloc(size_of::<CpuInfo>(), M_DEVBUF, M_WAITOK | M_ZERO) else {
        panic(format_args!("cpu_attach: no memory for cpu_info"));
    };
    let ci = p.cast::<CpuInfo>();
    kassert!(ci.as_ptr().is_aligned());
    // SAFETY: a fresh allocation of the right size and alignment, written once before use
    // and never freed (the C's `cpu_info`s live forever).
    unsafe {
        ci.write(CpuInfo::new());
        ci.as_ref()
    }
}

/// The `CPUF_AP` half of `cpu_attach`: start the processor and wait (ten seconds at most)
/// until it has identified itself.
#[cfg(feature = "multiprocessor")]
fn cpu_attach_secondary(ci: &'static CpuInfo) {
    let mut buf = [0u8; 32];
    let mut spinup_data = 0u64;
    let mut spinup_method = 0;
    let mut timeout = 10000;

    let _len = OF_getprop(ci.ci_node.get(), b"enable-method", &mut buf);
    if cstr(&buf) == b"psci" {
        spinup_method = 1;
    } else if cstr(&buf) == b"spin-table" {
        spinup_method = 2;
        spinup_data = OF_getpropint64(ci.ci_node.get(), b"cpu-release-addr", 0);
    }

    clockqueue_init(&ci.ci_queue);
    sched_init_cpu(ci);
    if cpu_start_secondary(ci, spinup_method, spinup_data) {
        ci.ci_flags.fetch_or(CPUF_IDENTIFY, Ordering::Relaxed);
        dsb_sev();

        loop {
            if ci.ci_flags.load(Ordering::Acquire) & CPUF_IDENTIFIED != 0 {
                break;
            }
            timeout -= 1;
            if timeout == 0 {
                break;
            }
            delay(1000);
        }
        if timeout == 0 {
            kprintf!(" failed to identify");
            ci.ci_flags.store(0, Ordering::Relaxed);
        }
    } else {
        kprintf!(" failed to spin up");
        ci.ci_flags.store(0, Ordering::Relaxed);
    }
}

/// `dsb sy; sev`: publish the stores and wake the CPUs waiting in `wfe`.
#[cfg(feature = "multiprocessor")]
fn dsb_sev() {
    // SAFETY: a barrier and an event; no other architectural effect.
    unsafe { asm!("dsb sy", "sev", options(nostack, preserves_flags)) };
}

/// `dsb sy`.
#[cfg(feature = "multiprocessor")]
fn dsb_sy() {
    // SAFETY: a barrier.
    unsafe { asm!("dsb sy", options(nostack, preserves_flags)) };
}

/// `wfe`: wait for an event (or an interrupt).
#[cfg(feature = "multiprocessor")]
fn wfe() {
    // SAFETY: waits; no other architectural effect.
    unsafe { asm!("wfe", options(nomem, nostack, preserves_flags)) };
}

/// `cpu_init`: this CPU's MMU and control registers: the user half of the address space,
/// PAN, DIT, pointer authentication, BTI, SVE's default vector length and the debug
/// registers.
pub fn cpu_init() {
    let pt0pa = crate::arch::arm64::arm64::pmap::pmap_kernel()
        .pm_pt0pa
        .get();

    let mut tcr = read_specialreg!("tcr_el1");
    tcr &= !tcr_t0sz(0x3f);
    tcr |= tcr_t0sz(64 - u64::from(USER_SPACE_BITS));
    tcr |= TCR_A1;
    // SAFETY: the kernel runs in the upper half; the lower half gets the kernel's empty
    // table and the user address space width (with the ASID taken from TTBR1), then the TLB
    // is flushed.
    unsafe {
        write_specialreg!("ttbr0_el1", pt0pa);
        asm!("isb", options(nomem, nostack, preserves_flags));
        write_specialreg!("tcr_el1", tcr);
    }
    cpu_tlb_flush();

    // Enable PAN.
    let id_aa64mmfr1 = read_specialreg!("id_aa64mmfr1_el1");
    if id_aa64mmfr1 & ID_AA64MMFR1_PAN_MASK >= ID_AA64MMFR1_PAN_IMPL {
        let mut sctlr = read_specialreg!("sctlr_el1");
        sctlr &= !SCTLR_SPAN;
        if id_aa64mmfr1 & ID_AA64MMFR1_PAN_MASK >= ID_AA64MMFR1_PAN_EPAN {
            sctlr |= SCTLR_EPAN;
        }
        // SAFETY: PSTATE.PAN set on exception entry: the kernel reaches user memory only
        // through the unprivileged `ldtr`/`sttr` of `copy.S`.
        unsafe { write_specialreg!("sctlr_el1", sctlr) };
    }

    // Enable DIT.
    let id_aa64pfr0 = read_specialreg!("id_aa64pfr0_el1");
    if id_aa64pfr0 & ID_AA64PFR0_DIT_MASK >= ID_AA64PFR0_DIT_IMPL {
        // SAFETY: data independent timing changes only instruction timing.
        unsafe { asm!(".arch armv8.4-a", "msr dit, #1", options(nomem, nostack)) };
    }

    // Enable PAuth.
    let isar1 = CPU_ID_AA64ISAR1.load(Ordering::Relaxed);
    let isar2 = CPU_ID_AA64ISAR2.load(Ordering::Relaxed);
    if isar1 & ID_AA64ISAR1_APA_MASK >= ID_AA64ISAR1_APA_PAC
        || isar1 & ID_AA64ISAR1_API_MASK >= ID_AA64ISAR1_API_PAC
        || isar2 & ID_AA64ISAR2_APA3_MASK >= ID_AA64ISAR2_APA3_PAC
    {
        let mut sctlr = read_specialreg!("sctlr_el1");
        sctlr |= SCTLR_EnIA | SCTLR_EnDA;
        sctlr |= SCTLR_EnIB | SCTLR_EnDB;
        // SAFETY: the kernel is built without pointer authentication; the keys matter to
        // userland only.
        unsafe { write_specialreg!("sctlr_el1", sctlr) };
    }

    // Enable strict BTI compatibility for PACIASP and PACIBSP.
    if CPU_ID_AA64PFR1.load(Ordering::Relaxed) & ID_AA64PFR1_BT_MASK >= ID_AA64PFR1_BT_IMPL {
        let mut sctlr = read_specialreg!("sctlr_el1");
        sctlr |= SCTLR_BT0 | SCTLR_BT1;
        // SAFETY: only changes how PACIxSP behave as branch targets.
        unsafe { write_specialreg!("sctlr_el1", sctlr) };
    }

    // Setup SVE with the default 128-bit vector length.
    if CPU_ID_AA64PFR0.load(Ordering::Relaxed) & ID_AA64PFR0_SVE_MASK >= ID_AA64PFR0_SVE_IMPL {
        let mut cpacr = read_specialreg!("cpacr_el1");
        cpacr &= !CPACR_ZEN_MASK;
        cpacr |= CPACR_ZEN_TRAP_EL0;
        // SAFETY: SVE is enabled at EL1 just long enough to write ZCR_EL1 (by encoding,
        // the assembler wants SVE for the name), then trapped again.
        unsafe {
            write_specialreg!("cpacr_el1", cpacr);
            asm!("isb", options(nomem, nostack, preserves_flags));
            write_specialreg!("s3_0_c1_c2_0", 0);
            cpacr &= !CPACR_ZEN_MASK;
            cpacr |= CPACR_ZEN_TRAP_ALL1;
            write_specialreg!("cpacr_el1", cpacr);
            asm!("isb", options(nomem, nostack, preserves_flags));
        }
    }

    // Initialize debug registers.
    // SAFETY: the monitor debug registers' reset state, EL0 DCC accesses trapped, and the
    // OS lock released.
    unsafe {
        write_specialreg!("mdscr_el1", DBG_MDSCR_TDCC);
        write_specialreg!("oslar_el1", 0);
    }
}

/// `cpu_flush_bp_noop`: the CPU needs no branch predictor flush.
pub fn cpu_flush_bp_noop() {}

/// `cpu_flush_bp_psci`: the firmware's branch predictor flush (`psci_flush_bp`, `NPSCI`
/// only: nothing here).
pub fn cpu_flush_bp_psci() {
    if NPSCI > 0 {
        psci_flush_bp(&curcpu().ci_flush_bp);
    }
}

/// `cpu_serror_apple`: Apple's L2 cache error registers.
pub fn cpu_serror_apple() {
    // SAFETY: barriers before reading the implementation-defined error registers.
    unsafe { asm!("dsb sy", "isb", options(nostack, preserves_flags)) };
    kprintf!("l2c_err_sts 0x{:x}\n", read_specialreg!("s3_3_c15_c8_0"));
    kprintf!("l2c_err_adr 0x{:x}\n", read_specialreg!("s3_3_c15_c9_0"));
    kprintf!("l2c_err_inf 0x{:x}\n", read_specialreg!("s3_3_c15_c10_0"));
}

/// `cpu_clockspeed`: `cpu_cpuspeed`: the frequency of `cpu_node`'s clock in MHz.
pub fn cpu_clockspeed(freq: &mut i32) -> Result<(), Errno> {
    // *freq = clock_get_frequency(cpu_node, NULL) / 1000000;
    let _ = freq;
    Err(unported!("clock_get_frequency (dev/ofw/ofw_clock.c)"))
}

/// `cpu_boot_secondary_processors`: lets every attached application processor run, one at
/// a time, each with its own random seed. Without `MULTIPROCESSOR` there are none.
pub fn cpu_boot_secondary_processors() {
    #[cfg(feature = "multiprocessor")]
    {
        let mut ci: *const CpuInfo = cpu_info_list();
        // SAFETY: the list links `cpu_info`s that live forever.
        while let Some(c) = unsafe { ci.as_ref() } {
            ci = c.ci_next.get();
            let flags = c.ci_flags.load(Ordering::Relaxed);
            if flags & CPUF_AP == 0 || flags & CPUF_PRIMARY != 0 {
                continue;
            }

            c.ci_randseed.set((arc4random() & 0x7fff_ffff) + 1);
            cpu_boot_secondary(c);
        }

        #[cfg(feature = "qemu")]
        selfcheck::ipis();
    }
}

/// `cpu_start_secondary`: hands the boot processor's MMU setup to `ci` and releases it into
/// `cpu_hatch_entry` (see the module's deviations); false when it cannot be started.
#[cfg(feature = "multiprocessor")]
pub fn cpu_start_secondary(ci: &'static CpuInfo, method: i32, data: u64) -> bool {
    let _ = data; // the spin table's release address: the bootloader's business here
    ci.ci_ttbr1.set(read_specialreg!("ttbr1_el1"));
    AP_MAIR.store(read_specialreg!("mair_el1"), Ordering::Relaxed);
    AP_TCR.store(read_specialreg!("tcr_el1"), Ordering::Relaxed);
    AP_SCTLR.store(read_specialreg!("sctlr_el1"), Ordering::Relaxed);
    AP_TTBR1.store(ci.ci_ttbr1.get(), Ordering::Relaxed);
    crate::arch::arm64::arm64::cpufunc::cpu_dcache_wb_range(
        ptr::from_ref(ci) as usize,
        size_of::<CpuInfo>(),
    );

    // 1: psci, 2: spin-table; both through the boot protocol, which knows them.
    if method != 1 && method != 2 {
        return false;
    }

    // SAFETY: `initarm` copies `BootMp` once, before any CPU but the boot one runs.
    let Some(mp) = (unsafe { BOOT_MP.read() }) else {
        return false;
    };
    let Some(index) = mp
        .cpus()
        .position(|c| c.hwid & MPIDR_AFF == ci.ci_mpidr.get())
    else {
        return false;
    };
    if (mp.cpu)(index).hwid & MPIDR_AFF == mp.bsp_hwid & MPIDR_AFF {
        return false;
    }

    // SAFETY: `index` is a processor of the boot protocol other than this one, started
    // once (each `cpu` node attaches once), and `ci` is what `cpu_hatch_entry` expects: a
    // `cpu_info` that lives forever, published by the protocol's release store.
    unsafe { (mp.start)(index, ptr::from_ref(ci) as usize) };
    true
}

/// `cpu_boot_secondary`: `CPUF_GO` for `ci`, an IPI in case it sleeps in `wfi`, then wait
/// until it runs.
#[cfg(feature = "multiprocessor")]
pub fn cpu_boot_secondary(ci: &CpuInfo) {
    #[cfg(feature = "qemu")]
    let window = selfcheck::prepare();

    ci.ci_flags.fetch_or(CPUF_GO, Ordering::Release);
    dsb_sev();

    // Send an interrupt as well to make sure the CPU wakes up regardless of whether it is in
    // a WFE or a WFI loop.
    arm_send_ipi(ci, ARM_IPI_NOP);

    #[cfg(feature = "qemu")]
    if let Some(w) = &window {
        selfcheck::remap(w);
    }

    while ci.ci_flags.load(Ordering::Acquire) & CPUF_RUNNING == 0 {
        wfe();
    }

    #[cfg(feature = "qemu")]
    if let Some(w) = &window {
        selfcheck::finish(w);
    }
}

/// The application processor's entry from the boot glue (`Cpu::cpu_hatch`); `arg` is its
/// `struct cpu_info`. Without `MULTIPROCESSOR` nothing starts one.
///
/// # Safety
///
/// Called once per application processor by the boot glue, with the `arg` the boot processor
/// passed to `BootMp::start` (`cpu_start_secondary`).
pub unsafe fn cpu_hatch_entry(arg: usize) -> ! {
    #[cfg(feature = "multiprocessor")]
    {
        let ci = arg as *const CpuInfo;
        let pt0pa = crate::arch::arm64::arm64::pmap::pmap_kernel()
            .pm_pt0pa
            .get();
        let mut cpacr = read_specialreg!("cpacr_el1");
        cpacr &= !(CPACR_FPEN_MASK | CPACR_ZEN_MASK);
        cpacr |= CPACR_FPEN_TRAP_ALL1 | CPACR_ZEN_TRAP_ALL1;

        // SAFETY: the processor's first kernel instructions, interrupts masked, on the
        // bootloader's stack and tables, touching only the kernel image (which both sets of
        // tables map). The kernel runs on SP_EL1 (the "EL1h" vectors, as initarm does on the
        // boot processor). The kernel's TTBR1 tables map the upper half as the bootloader's
        // do (`pmap_bootstrap` adopted them, the bootloader's stack in the direct map
        // included) plus what the kernel mapped since (`ci`, the kernel stacks); MAIR keeps
        // the bootloader's attributes 0 and 1. The lower half gets the kernel's empty table,
        // then this CPU's TLB is flushed. FP/SIMD and SVE trap at EL0 and EL1 (what
        // `fpu_drop` leaves). Then the per-CPU pointer and the kernel's vectors, so curcpu()
        // and a fault work from here on.
        unsafe {
            asm!(
                "mov {tmp}, sp",
                "msr spsel, #1",
                "mov sp, {tmp}",
                "isb",
                tmp = out(reg) _,
                options(nomem, nostack, preserves_flags)
            );
            write_specialreg!("mair_el1", AP_MAIR.load(Ordering::Relaxed));
            write_specialreg!("ttbr0_el1", pt0pa);
            asm!("isb", options(nomem, nostack, preserves_flags));
            write_specialreg!("tcr_el1", AP_TCR.load(Ordering::Relaxed));
            asm!("isb", options(nomem, nostack, preserves_flags));
            write_specialreg!("ttbr1_el1", AP_TTBR1.load(Ordering::Relaxed));
            asm!(
                "isb",
                "tlbi vmalle1",
                "dsb nsh",
                "isb",
                options(nostack, preserves_flags)
            );
            write_specialreg!("sctlr_el1", AP_SCTLR.load(Ordering::Relaxed));
            write_specialreg!("cpacr_el1", cpacr);
            asm!("isb", options(nomem, nostack, preserves_flags));
            write_specialreg!("tpidr_el1", ci as u64);
            write_specialreg!("vbar_el1", exception_vectors_addr());
            asm!("isb", options(nomem, nostack, preserves_flags));
        }

        // Not in the C: the event stream bounds the `wfe` waits of `cpu_init_secondary`
        // (see the module's deviations).
        crate::arch::arm64::dev::agtimer::agtimer_evtstrm_enable();

        // SAFETY: `cpu_start_secondary` passed a `cpu_info` that lives forever, mapped now.
        let stkend = unsafe { (*ci).ci_el1_stkend.get() };
        let next: extern "C" fn(*const CpuInfo) -> ! = cpu_hatch_secondary;
        // SAFETY: the processor leaves the bootloader's stack for its own kernel stack
        // (`ci_el1_stkend`, as locore.S's cpu_hatch_secondary does) and never comes back;
        // a null frame record ends backtraces there.
        unsafe {
            asm!(
                "mov sp, {stk}",
                "mov x29, xzr",
                "mov x30, xzr",
                "br {next}",
                stk = in(reg) stkend,
                next = in(reg) next,
                in("x0") ci,
                options(noreturn)
            )
        }
    }
    #[cfg(not(feature = "multiprocessor"))]
    {
        let _ = arg;
        <crate::machine::Machine as crate::machine::Cpu>::halt()
    }
}

/// `cpu_hatch_secondary` on the kernel stack: `cpu_init_secondary`.
#[cfg(feature = "multiprocessor")]
extern "C" fn cpu_hatch_secondary(ci: *const CpuInfo) -> ! {
    // SAFETY: `cpu_hatch_entry` passes the `cpu_info` the boot processor started us with.
    let ci: &'static CpuInfo = unsafe { &*ci };
    kassert!(ci.ci_ttbr1.get() == read_specialreg!("ttbr1_el1"));
    cpu_init_secondary(ci)
}

/// `cpu_init_secondary`: the application processor reports itself present, identifies
/// itself when the boot processor asks, waits for `CPUF_GO`, initialises itself, starts
/// its interrupts and clock and enters the scheduler as its idle thread.
#[cfg(feature = "multiprocessor")]
pub fn cpu_init_secondary(ci: &'static CpuInfo) -> ! {
    ci.ci_flags.fetch_or(CPUF_PRESENT, Ordering::Release);
    dsb_sy();

    if ci.ci_flags.load(Ordering::Acquire) & CPUF_IDENTIFIED == 0 {
        while ci.ci_flags.load(Ordering::Acquire) & CPUF_IDENTIFY == 0 {
            wfe();
        }

        cpu_identify(ci);
        ci.ci_flags.fetch_or(CPUF_IDENTIFIED, Ordering::Release);
        dsb_sy();
    }

    while ci.ci_flags.load(Ordering::Acquire) & CPUF_GO == 0 {
        wfe();
    }
    dsb_sy();

    // HIBERNATE (CPUF_PARK, cpu_park): not configured.

    cpu_init();

    #[cfg(feature = "qemu")]
    selfcheck::hatch();

    // Start from a clean slate regardless of whether this is the initial power up or a
    // wakeup of a suspended CPU.
    ci.ci_curproc.set(ptr::null());
    ci.ci_curpcb.set(ptr::null());
    ci.ci_curpm.set(ptr::null());
    ci.ci_cpl.set(IPL_NONE as u32);
    ci.ci_ipending.set(0);
    ci.ci_idepth.set(0);

    // DIAGNOSTIC
    ci.ci_mutex_level.set(0);

    // Re-create the switchframe for this CPUs idle process.
    let spc = &ci.ci_schedstate;
    // SAFETY: `sched_kthreads_create` forked this CPU's idle thread before the boot CPU
    // set CPUF_GO; it never exits.
    let Some(p) = (unsafe { spc.spc_idleproc.get().as_ref() }) else {
        panic(format_args!("cpu_init_secondary: no idle thread"));
    };
    let pcb = p.pcb();

    let tf = stackalign(p.p_addr.get() as usize + USPACE - size_of::<Trapframe>() - 0x10)
        as *mut Trapframe;
    pcb.pcb_tf.set(tf);

    let sf = (tf as *mut Switchframe).wrapping_sub(1);
    let idle: fn(*mut c_void) = sched_idle;
    // SAFETY: `sf` is right below the trap frame, inside the idle thread's u-area, which
    // nothing runs on yet.
    unsafe {
        sf.write(Switchframe {
            sf_x19: idle as usize as Register,
            sf_x20: ptr::from_ref(ci) as usize as Register,
            sf_lr: proc_trampoline as *const () as usize as Register,
            ..Switchframe::default()
        });
    }
    pcb.pcb_sp.set(sf as u64);

    let _s = splraise(IPL_HIGH);
    arm_intr_cpu_enable();
    cpu_startclock();

    ci.ci_flags.fetch_or(CPUF_RUNNING, Ordering::Release);
    dsb_sev();

    spllower(IPL_NONE);

    sched_toidle()
}

/// `cpu_halt`: this CPU stops (`ARM_IPI_HALT`) until `CPUF_GO` again.
#[cfg(feature = "multiprocessor")]
pub fn cpu_halt() {
    let ci = curcpu();

    kernel_assert_unlocked();
    sched_assert_unlocked();

    let psw = intr_disable();

    ci.ci_flags
        .fetch_and(!(CPUF_RUNNING | CPUF_PRESENT | CPUF_GO), Ordering::AcqRel);

    if NPSCI > 0 && psci_can_suspend() != 0 {
        psci_cpu_off();
    }

    // If we failed to turn ourselves off using PSCI, declare that we're still present and
    // spin in a low power state until we're told to wake up again by the primary CPU.

    ci.ci_flags.fetch_or(CPUF_PRESENT, Ordering::Release);

    // Mask clock interrupts.
    let ctl = read_specialreg!("cntv_ctl_el0") | u64::from(CNTV_CTL_IMASK);
    // SAFETY: this CPU's virtual timer; masking it only delays its interrupt.
    unsafe { write_specialreg!("cntv_ctl_el0", ctl) };

    let mut _count = 0u64;
    while ci.ci_flags.load(Ordering::Acquire) & CPUF_GO == 0 {
        if NPSCI > 0 && ci.ci_psci_suspend_param.get() != 0 {
            // psci_cpu_suspend(ci_psci_suspend_param, start_pa, ci_pa): the resume entry
            // point is M14's (SUSPEND), so the state is dropped as on a failed call.
            ci.ci_psci_suspend_param.set(0);
        }
        cpu_suspend_cycle();
        _count += 1;
    }

    ci.ci_flags.fetch_or(CPUF_RUNNING, Ordering::Release);
    dsb_sev();

    // SAFETY: `psw` is this CPU's DAIF from `intr_disable` above.
    unsafe { intr_restore(psw) };

    // Unmask clock interrupts.
    let ctl = read_specialreg!("cntv_ctl_el0") & !u64::from(CNTV_CTL_IMASK);
    // SAFETY: as above.
    unsafe { write_specialreg!("cntv_ctl_el0", ctl) };
}

/// `cpu_kick`: force `ci` to enter the kernel.
pub fn cpu_kick(ci: &CpuInfo) {
    #[cfg(feature = "multiprocessor")]
    if !ptr::eq(ci, curcpu()) {
        arm_send_ipi(ci, ARM_IPI_NOP);
    }
    #[cfg(not(feature = "multiprocessor"))]
    let _ = ci;
}

/// `cpu_unidle`: wake `ci` from its idle loop. This could send IPI or SEV depending on if
/// the other processor is sleeping (WFI or WFE), in userland, or if the cpu is in other
/// possible wait states? Without `MULTIPROCESSOR` the idle loop sees the run queue itself.
pub fn cpu_unidle(ci: &CpuInfo) {
    #[cfg(feature = "multiprocessor")]
    if !ptr::eq(ci, curcpu()) {
        arm_send_ipi(ci, ARM_IPI_NOP);
    }
    #[cfg(not(feature = "multiprocessor"))]
    let _ = ci;
}

/// `cpu_suspend_cycle` (`SUSPEND`): `(*cpu_suspend_cycle_fcn)()`.
pub fn cpu_suspend_cycle() {
    // SAFETY: a driver installs its own at attach time, before any CPU halts or suspends.
    let f = unsafe { CPU_SUSPEND_CYCLE_FCN.read() };
    f();
}

/// `cpu_init_primary` (`SUSPEND`): the boot processor's way back from `SYSTEM_SUSPEND`.
pub fn cpu_init_primary() {
    // cpu_init(); cpu_startclock(); longjmp(&cpu_suspend_jmpbuf);
    let _ = unported!("cpu_init_primary: SUSPEND (setjmp/longjmp, M14)");
}

/// `cpu_suspend_primary` (`SUSPEND`): suspend the machine on the boot processor.
pub fn cpu_suspend_primary() -> Result<(), Errno> {
    Err(unported!(
        "cpu_suspend_primary: SUSPEND (psci_system_suspend, setjmp, M14)"
    ))
}

/// `cpu_resume_secondary` (`SUSPEND`, `MULTIPROCESSOR`): restart `ci` after a resume.
#[cfg(feature = "multiprocessor")]
pub fn cpu_resume_secondary(ci: &CpuInfo) {
    let _ = ci;
    let _ = unported!("cpu_resume_secondary: SUSPEND (M14)");
}

// Dynamic voltage and frequency scaling implementation.

/// `cpu_opp_init`: reads the `operating-points-v2` table `phandle` (shared tables once)
/// and registers the CPU as a cooling device.
pub fn cpu_opp_init(ci: &CpuInfo, phandle: u32) {
    if let Some(ot) = OPP_TABLES.0.iter().find(|ot| ot.ot_phandle == phandle) {
        ci.ci_opp_table.set(ptr::from_ref(ot));
        return;
    }

    let node = OF_getnodebyphandle(phandle);
    if node == 0 {
        return;
    }

    if !OF_is_compatible(node, b"operating-points-v2") {
        return;
    }

    let children = || {
        core::iter::successors(Some(OF_child(node)), |&c| Some(OF_peer(c)))
            .take_while(|&c| c != 0)
            .filter(|&c| OF_getproplen(c, b"turbo-mode") != 0)
    };
    let count = children().count();
    if count == 0 {
        return;
    }

    let Some(opps) = malloc(count * size_of::<Opp>(), M_DEVBUF, M_ZERO | M_WAITOK) else {
        return;
    };
    // SAFETY: `count` zeroed `Opp`s (plain integers), never freed.
    let opp: &'static mut [Opp] =
        unsafe { core::slice::from_raw_parts_mut(opps.as_ptr().cast::<Opp>(), count) };

    for (n, child) in children().enumerate() {
        let opp_hz = OF_getpropint64(child, b"opp-hz", 0);
        let mut values = [0u32; 3];
        let len = OF_getpropintarray(child, b"opp-microvolt", &mut values);
        let mut opp_microvolt = 0;
        if len == 4 || len == 3 * 4 {
            opp_microvolt = values[0];
        }

        // Insert into the array, keeping things sorted.
        let i = opp[..n].iter().position(|o| opp_hz < o.opp_hz).unwrap_or(n);
        opp.copy_within(i..n, i + 1);
        opp[i] = Opp {
            opp_hz,
            opp_microvolt,
        };
    }

    let Some(otp) = malloc(size_of::<OppTable>(), M_DEVBUF, M_ZERO | M_WAITOK) else {
        return;
    };
    let otp = otp.cast::<OppTable>();
    let master = if OF_getproplen(node, b"opp-shared") == 0 {
        ptr::from_ref(ci)
    } else {
        ptr::null()
    };
    // SAFETY: a fresh allocation of the right size and alignment, written once, never
    // freed (the C keeps its tables forever).
    let ot: &'static OppTable = unsafe {
        otp.write(OppTable {
            ot_list: ListEntry::new(),
            ot_phandle: phandle,
            ot_opp: opp,
            ot_opp_hz_min: opp[0].opp_hz,
            ot_opp_hz_max: opp[count - 1].opp_hz,
            ot_master: Cell::new(master),
        });
        otp.as_ref()
    };

    // SAFETY: CPUs attach one at a time on the boot CPU; the table is new.
    unsafe { OPP_TABLES.0.insert_head(ot) };

    ci.ci_opp_table.set(ptr::from_ref(ot));
    ci.ci_opp_max.store(ot.ot_nopp() - 1, Ordering::Relaxed);
    ci.ci_cpu_supply
        .set(OF_getpropint(ci.ci_node.get(), b"cpu-supply", 0));

    // cd = malloc(sizeof(struct cooling_device)); cd_node, cd_cookie = ci,
    // cd_get_level = cpu_opp_get_cooling_level, cd_set_level = cpu_opp_set_cooling_level;
    // cooling_device_register(cd);
    let _ = unported!("cooling_device_register (dev/ofw/ofw_thermal.c)");

    // Do additional checks at mountroot when all the clocks and regulators are available.
    // SAFETY: `cpu_attach` pointed `ci_dev` at the CPU's device, which lives forever.
    if let Some(dev) = unsafe { ci.ci_dev.get().as_ref() } {
        config_mountroot(dev, cpu_opp_mountroot);
    }
}

/// `cpu_opp_mountroot`: checks each table's clock and regulator, picks the initial
/// performance level and installs `cpu_opp_setperf` as `cpu_setperf`.
pub fn cpu_opp_mountroot(_self: &Device) {
    // if (cpu_setperf) return; then, per CPU with a table: cpu_opp_kstat_attach (NKSTAT),
    // regulator_enable, clock_get_frequency, clock_set_frequency, regulator_get_voltage and
    // regulator_set_voltage decide whether DVFS works; the first CPU's frequency gives the
    // level; task_set(&cpu_opp_task), cpu_setperf = cpu_opp_setperf, cpu_setperf(perflevel).
    let _ =
        unported!("cpu_opp_mountroot: cpu_setperf (sched_bsd.c), ofw_clock.c and ofw_regulator.c");
    task_set(&CPU_OPP_TASK, cpu_opp_dotask, ptr::null_mut());
}

/// `cpu_opp_dotask`: moves every DVFS-capable CPU to its chosen operating point.
pub fn cpu_opp_dotask(_arg: *mut c_void) {
    // Per CPU with a table (the master of a shared one): the clock down before the voltage,
    // or up after it; "DVFS failed" on an error.
    let _ = unported!("cpu_opp_dotask: clock_set_frequency, regulator_set_voltage");
}

/// `cpu_opp_setperf`: picks each table's operating point for `level` (percent) and lets
/// the task apply them.
pub fn cpu_opp_setperf(level: i32) {
    let mut ci: *const CpuInfo = cpu_info_list();
    // SAFETY: the list links `cpu_info`s that live forever.
    while let Some(c) = unsafe { ci.as_ref() } {
        ci = c.ci_next.get();
        // SAFETY: `cpu_opp_init` tables live forever.
        let Some(ot) = (unsafe { c.ci_opp_table.get().as_ref() }) else {
            continue;
        };

        // Skip if this table is shared and we're not the master.
        let master = ot.ot_master.get();
        if !master.is_null() && !ptr::eq(master, c) {
            continue;
        }

        let min = ot.ot_opp_hz_min;
        let max = ot.ot_opp_hz_max;
        let level_hz = min + (level as u64 * (max - min)) / 100;
        let mut opp_hz = min;
        for o in ot.ot_opp {
            if o.opp_hz <= level_hz && o.opp_hz >= opp_hz {
                opp_hz = o.opp_hz;
            }
        }

        // Find index of selected operating point.
        let opp_idx = ot.ot_opp.iter().position(|o| o.opp_hz == opp_hz);
        kassert!(opp_idx.is_some());

        c.ci_opp_idx
            .store(opp_idx.unwrap_or(0) as i32, Ordering::Relaxed);
    }

    // Update the hardware from a task since setting the regulators might need process
    // context.
    task_add(SYSTQ, &CPU_OPP_TASK);
}

/// `cpu_opp_get_cooling_level`: the cooling device's current level.
pub fn cpu_opp_get_cooling_level(cookie: *mut c_void, _cells: &[u32]) -> u32 {
    // SAFETY: the cooling device was registered with its CPU as the cookie.
    let ci = unsafe { &*cookie.cast::<CpuInfo>() };
    // SAFETY: a CPU registers as a cooling device only with a table, which lives forever.
    let ot = unsafe { &*ci.ci_opp_table.get() };

    (ot.ot_nopp() - ci.ci_opp_max.load(Ordering::Relaxed) - 1) as u32
}

/// `cpu_opp_set_cooling_level`: limits the CPU's operating points to cool down.
pub fn cpu_opp_set_cooling_level(cookie: *mut c_void, _cells: &[u32], level: u32) {
    // SAFETY: as in `cpu_opp_get_cooling_level`.
    let ci = unsafe { &*cookie.cast::<CpuInfo>() };
    // SAFETY: as in `cpu_opp_get_cooling_level`.
    let ot = unsafe { &*ci.ci_opp_table.get() };

    let level = level.min((ot.ot_nopp() - 1) as u32);

    let opp_max = ot.ot_nopp() - level as i32 - 1;
    if ci.ci_opp_max.load(Ordering::Relaxed) != opp_max {
        ci.ci_opp_max.store(opp_max, Ordering::Relaxed);
        task_add(SYSTQ, &CPU_OPP_TASK);
    }
}

/// `cpu_psci_init`: the shallowest idle state for the idle loop and the deepest one for
/// suspend, from the device tree.
pub fn cpu_psci_init(ci: &CpuInfo) {
    let node0 = ci.ci_node.get();

    // Find the shallowest (for now) idle state for this CPU. This should be the first one
    // that is listed. We'll use it in the idle loop.

    let len = OF_getproplen(node0, b"cpu-idle-states");
    if len < 4 {
        return;
    }

    let states = prop_cells(node0, b"cpu-idle-states", len);
    let node = OF_getnodebyphandle(states.first().copied().unwrap_or(0));
    free_cells(states);
    if node != 0 {
        let param = OF_getpropint(node, b"arm,psci-suspend-param", 0);
        let entry = OF_getpropint(node, b"entry-latency-us", 0);
        let exit = OF_getpropint(node, b"exit-latency-us", 0);
        let residency = OF_getpropint(node, b"min-residency-us", 0);
        ci.ci_psci_idle_latency
            .set(ci.ci_psci_idle_latency.get() + entry + exit + 2 * residency);

        // Skip states that stop the local timer.
        if OF_getpropbool(node, b"local-timer-stop") {
            ci.ci_psci_idle_param.set(0);
        }

        // Skip powerdown states: psci_features(CPU_SUSPEND) says which bit marks them; if
        // a state is left, it becomes ci_psci_idle_param and cpu_psci_idle_cycle the idle
        // function.
        let _ = param;
        let _ = unported!("psci_features (dev/fdt/psci.c): cpu-idle-states ignored");
    }

    // Hunt for the deepest idle state for this CPU. This is fairly complicated as it
    // requires traversing quite a few nodes in the device tree. The first step is to look
    // up the "psci" power domain for this CPU.

    let mut idx = OF_getindex(node0, Some(b"psci"), b"power-domain-names");
    if idx < 0 {
        return;
    }

    let len = OF_getproplen(node0, b"power-domains");
    if len <= 0 {
        return;
    }

    let domains = prop_cells(node0, b"power-domains", len);
    let mut at = 0;
    while at < domains.len() {
        if idx == 0 {
            break;
        }

        let node = OF_getnodebyphandle(domains[at]);
        if node == 0 {
            break;
        }

        let ncells = OF_getpropint(node, b"#power-domain-cells", 0) as usize;
        at += ncells + 1;
        idx -= 1;
    }

    let node = if idx == 0 {
        OF_getnodebyphandle(domains.get(at).copied().unwrap_or(0))
    } else {
        0
    };
    free_cells(domains);
    if node == 0 {
        return;
    }

    // We found the "psci" power domain. If this power domain has a parent power domain,
    // stash its phandle away for later.

    let cluster = OF_getpropint(node, b"power-domains", 0);

    // Get the deepest idle state for the CPU; this should be the last one that is listed.

    let len = OF_getproplen(node, b"domain-idle-states");
    if len < 4 {
        return;
    }

    let states = prop_cells(node, b"domain-idle-states", len);
    let node = OF_getnodebyphandle(states.last().copied().unwrap_or(0));
    free_cells(states);
    if node == 0 {
        return;
    }

    ci.ci_psci_suspend_param
        .set(OF_getpropint(node, b"arm,psci-suspend-param", 0));

    // Qualcomm Snapdragon always seem to operate in OS Initiated mode. This means that the
    // last CPU to suspend can pick the idle state that powers off the entire cluster. In
    // our case that will always be the primary CPU.

    #[cfg(feature = "multiprocessor")]
    if ci.ci_flags.load(Ordering::Relaxed) & CPUF_AP != 0 {
        return;
    }

    let node = OF_getnodebyphandle(cluster);
    if node == 0 {
        return;
    }

    // Get the deepest idle state for the cluster; this should be the last one that is
    // listed. (The C reuses the CPU's `len` for the cluster's property, as here.)

    let states = prop_cells(node, b"domain-idle-states", len);
    let node = OF_getnodebyphandle(states.last().copied().unwrap_or(0));
    free_cells(states);
    if node == 0 {
        return;
    }

    ci.ci_psci_suspend_param
        .set(OF_getpropint(node, b"arm,psci-suspend-param", 0));
}

/// `malloc(len, M_TEMP, M_WAITOK)` and `OF_getpropintarray` into it: the cells of `prop`.
fn prop_cells(node: i32, prop: &[u8], len: i32) -> &'static mut [u32] {
    let n = len.max(0) as usize / 4;
    let Some(p) = malloc(n.max(1) * 4, M_TEMP, M_WAITOK | M_ZERO) else {
        panic(format_args!("cpu_psci_init: malloc(M_WAITOK) failed"));
    };
    // SAFETY: `n` zeroed cells, freed by `free_cells` before the caller returns.
    let cells = unsafe { core::slice::from_raw_parts_mut(p.as_ptr().cast::<u32>(), n) };
    OF_getpropintarray(node, prop, cells);
    cells
}

/// `free(cells, M_TEMP, len)` of [`prop_cells`].
fn free_cells(cells: &'static mut [u32]) {
    let n = cells.len();
    if let Some(p) = ptr::NonNull::new(cells.as_mut_ptr().cast::<u8>()) {
        free(p, M_TEMP, n.max(1) * 4);
    }
}

/// `cpu_psci_idle_cycle`: `cpu_idle_cycle_fcn` with a PSCI idle state: suspend when the
/// last sleeps were long enough to pay for it, `wfi` otherwise; tracks the sleep time.
pub fn cpu_psci_idle_cycle() {
    let ci = curcpu();

    let start = microuptime();

    if ci.ci_prev_sleep.get() > u64::from(ci.ci_psci_idle_latency.get()) {
        // psci_cpu_suspend(ci->ci_psci_idle_param, 0, 0);
        let _ = unported!("psci_cpu_suspend (dev/fdt/psci.c)");
        cpu_wfi();
    } else {
        cpu_wfi();
    }

    let stop = microuptime();
    let d = timersub(&stop, &start);
    let mut itime = (d.tv_sec as u64) * 1_000_000 + d.tv_usec as u64;

    ci.ci_last_itime.set(itime);
    itime >>= 1;
    let prev = ci.ci_prev_sleep.get();
    ci.ci_prev_sleep.set((prev + (prev >> 1) + itime) >> 1);
}

/// `cpu_kstat_attach` (`NKSTAT`): the `mach` kstat (implementer, part, revision,
/// capacity).
pub fn cpu_kstat_attach(ci: &CpuInfo) {
    let _ = ci;
    let _ = unported!("cpu_kstat_attach: kstat_create (dev/kstat.c)");
}

/// `cpu_opp_kstat_attach` (`NKSTAT`): the `dt-opp` kstat (frequency, supply voltage).
pub fn cpu_opp_kstat_attach(ci: &CpuInfo) {
    let _ = ci;
    let _ = unported!("cpu_opp_kstat_attach: kstat_create (dev/kstat.c)");
}

/// The `qemu` self-check of the application processors (M11a's exit criterion): while an AP
/// hatches it reads a kernel window the boot processor then remaps under it (the TLB
/// invalidation must be broadcast to reach it); once all run, each takes an `ARM_IPI_NOP`.
#[cfg(all(feature = "multiprocessor", feature = "qemu"))]
mod selfcheck {
    use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

    use super::*;
    use crate::arch::arm64::dev::agintc::{agintc_attached, agintc_ipi_count};
    use crate::arch::arm64::dev::ampintc::ampintc_ipi_count;
    use crate::machine::pmap::{pmap_extract, pmap_kenter_pa, pmap_kernel, pmap_kremove};
    use crate::sys::mman::{PROT_READ, PROT_WRITE};
    use crate::sys::param::PAGE_SIZE;
    use crate::sys::types::{Paddr, Vaddr, Vsize};
    use crate::uvm::uvm_km::KP_NONE;

    /// The value of the page the window maps first.
    const OLD: u32 = 0x1111_1111;
    /// The value of the page the window maps after the remap.
    const NEW: u32 = 0x2222_2222;

    /// The test window while an AP hatches, 0 otherwise.
    static VA: AtomicUsize = AtomicUsize::new(0);
    /// 1: the AP read through the window; 2: the boot processor remapped it.
    static STAGE: AtomicU32 = AtomicU32::new(0);
    /// What the AP read after the remap.
    static SEEN: AtomicU32 = AtomicU32::new(0);
    /// How many APs saw the remap.
    static TLB_OK: AtomicU32 = AtomicU32::new(0);

    /// A window being tested: its address and the page it moves to.
    pub struct Window {
        va: Vaddr,
        new: Paddr,
    }

    /// One page of the kernel, filled with `val`: its physical address.
    fn page(val: u32) -> Option<Paddr> {
        let p = km_alloc(PAGE_SIZE, &KV_ANY, &KP_ZERO, &KD_WAITOK)?;
        // SAFETY: a fresh, mapped page of our own (leaked: a boot-time test).
        unsafe { p.cast::<u32>().write_volatile(val) };
        pmap_extract(pmap_kernel(), Vaddr::new(p.as_ptr() as usize))
    }

    /// Before `CPUF_GO`: a fresh window mapped to a page holding `OLD`.
    pub fn prepare() -> Option<Window> {
        let old = page(OLD)?;
        let new = page(NEW)?;
        let win = km_alloc(PAGE_SIZE, &KV_ANY, &KP_NONE, &KD_WAITOK)?;
        let va = Vaddr::new(win.as_ptr() as usize);
        // SAFETY: `va` is a kernel window of ours with nothing mapped, `old` a page of ours.
        unsafe { pmap_kenter_pa(va, old, PROT_READ | PROT_WRITE) };
        STAGE.store(0, Ordering::Relaxed);
        SEEN.store(0, Ordering::Relaxed);
        VA.store(va.as_usize(), Ordering::Release);
        Some(Window { va, new })
    }

    /// The AP's half, in `cpu_init_secondary` before `CPUF_RUNNING`: read, wait for the
    /// remap, read again.
    pub fn hatch() {
        let va = VA.load(Ordering::Acquire);
        if va == 0 {
            return;
        }
        // SAFETY: the boot processor keeps the window mapped (to one page or the other)
        // until this CPU runs; volatile, so each read goes through the translation.
        let _ = unsafe { (va as *const u32).read_volatile() };
        STAGE.store(1, Ordering::Release);
        while STAGE.load(Ordering::Acquire) != 2 {
            core::hint::spin_loop();
        }
        // SAFETY: as above.
        SEEN.store(
            unsafe { (va as *const u32).read_volatile() },
            Ordering::Release,
        );
    }

    /// After `CPUF_GO`: once the AP has read through the window, move it to the new page
    /// (`pmap_kremove` broadcasts the invalidation) and let the AP read again. There is no
    /// timeout: `cpu_boot_secondary` waits for `CPUF_RUNNING` without one anyway, and
    /// remapping before the AP's first read (as a 10 s timeout once did, while the AP slept
    /// in `wfe` after QEMU lost its `sev`, before the event stream) unmaps the window under it.
    pub fn remap(w: &Window) {
        while STAGE.load(Ordering::Acquire) != 1 {
            delay(100);
        }
        // SAFETY: our window; the AP is between its two reads, waiting for stage 2.
        unsafe {
            pmap_kremove(w.va, Vsize::new(PAGE_SIZE));
            pmap_kenter_pa(w.va, w.new, PROT_READ | PROT_WRITE);
        }
        STAGE.store(2, Ordering::Release);
    }

    /// After `CPUF_RUNNING`: what the AP saw. `hatch` runs before the AP sets `CPUF_RUNNING`,
    /// so the AP is done with the window.
    pub fn finish(w: &Window) {
        if SEEN.load(Ordering::Acquire) == NEW {
            TLB_OK.fetch_add(1, Ordering::Relaxed);
        }
        VA.store(0, Ordering::Release);
        // SAFETY: our window, unused from now on.
        unsafe { pmap_kremove(w.va, Vsize::new(PAGE_SIZE)) };
    }

    /// Once every AP runs: an `ARM_IPI_NOP` to each, seen by its handler; the summary.
    ///
    /// `main` holds the kernel lock here, and each AP's idle thread first takes it in
    /// `proc_trampoline_mi` with its interrupts still masked, so the boot CPU lets go of the
    /// lock while it waits for the handlers and takes it back afterwards.
    pub fn ipis() {
        use crate::kern::kern_lock::{__mp_acquire_count, __mp_release_all, KERNEL_LOCK};
        let hold = __mp_release_all(&KERNEL_LOCK);
        ipis_unlocked();
        __mp_acquire_count(&KERNEL_LOCK, hold);
    }

    /// The IPI handler's count of whichever GIC attached: agintc(4) on a GICv3 (M16f),
    /// ampintc(4) otherwise.
    fn ipi_count() -> u64 {
        if agintc_attached() {
            agintc_ipi_count()
        } else {
            ampintc_ipi_count()
        }
    }

    /// [`ipis`] without the kernel lock.
    fn ipis_unlocked() {
        let mut aps = 0;
        let mut running = 0;
        let mut ipi_ok = 0;

        let mut ci: *const CpuInfo = cpu_info_list();
        // SAFETY: the list links `cpu_info`s that live forever.
        while let Some(c) = unsafe { ci.as_ref() } {
            ci = c.ci_next.get();
            let flags = c.ci_flags.load(Ordering::Acquire);
            if flags & CPUF_AP == 0 {
                continue;
            }
            aps += 1;
            if flags & CPUF_RUNNING != 0 {
                running += 1;
            }
            let before = ipi_count();
            arm_send_ipi(c, ARM_IPI_NOP);
            let mut timeout = 1000;
            while ipi_count() == before && timeout > 0 {
                delay(1000);
                timeout -= 1;
            }
            if ipi_count() != before {
                ipi_ok += 1;
            }
        }

        kprintf!(
            "cpu: {} of {} application processors running, tlb shootdown seen by {}, ipi nop seen by {}\n",
            running,
            aps,
            TLB_OK.load(Ordering::Relaxed),
            ipi_ok
        );
    }
}
/* </CODE> */
