/*	$OpenBSD: psci.c,v 1.17 2024/07/10 11:01:24 kettenis Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 2016 Jonathan Gray <jsg@openbsd.org>
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
//! ARM Power State Coordination Interface on the device tree: `dev/fdt/psci.c`.
//!
//! The firmware interface that powers the machine off (`SYSTEM_OFF`), resets it
//! (`SYSTEM_RESET`), starts and stops CPUs, and carries the SMCCC calls of the Spectre
//! workarounds. `psci* at fdt? early 1` attaches it from the `/psci` node (mainbus's early
//! pass); it registers `psci_powerdown` and `psci_reset` as `powerdownfn` and `cpuresetfn`,
//! which arm64's `boot(9)` calls.
//!
//! Upstream: sys/dev/fdt/psci.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `hvc_call` and `smc_call` (`support.S`) and the `cpuresetfn`/`powerdownfn` variables
//!   (`machdep.c`) are reached through `machine::fdt` (`hvc_call`, `smc_call`,
//!   `set_cpuresetfn`, `set_powerdownfn`); generic code never names the arch.
//! - `psci_flush_bp` takes the current CPU's `ci_flush_bp` cell as an argument instead of
//!   reading `curcpu()->ci_flush_bp`: `ci_flush_bp` is an arm64 member of `struct cpu_info`
//!   that `machine::CpuInfo` does not carry. `arm64/cpu.rs`'s `cpu_flush_bp_psci` passes it.
//! - Under Limine the application processors are started by Limine's MP protocol, not by
//!   `psci_cpu_on`; after a boot by boot(8) (M14) arm64's `machdep.rs` starts them through
//!   `psci_cpu_on`, as `cpu_start_secondary` does in C.
//! - The softc's call function is a `fn(u64, u64, u64, u64) -> u64`; the results are
//!   truncated to `i32`/`u32` as the C's return types do.
//! - `psci_sc` is an `AtomicPtr` set by `psci_attach` (the softc lives as long as the device,
//!   which is never detached).

use core::cell::Cell;
use core::ffi::c_void;
use core::sync::atomic::{AtomicPtr, Ordering};

use crate::dev::fdt::pscivar::{
    CPU_OFF, CPU_ON, CPU_SUSPEND, PSCI_FEATURES, PSCI_METHOD_HVC, PSCI_METHOD_NONE,
    PSCI_METHOD_SMC, PSCI_NOT_SUPPORTED, PSCI_SUCCESS, PSCI_VERSION, SYSTEM_OFF, SYSTEM_RESET,
    SYSTEM_SUSPEND,
};
use crate::dev::ofw::fdt::{OF_getprop, OF_getpropint, OF_is_compatible};
use crate::kassert;
use crate::kern::subr_prf::printf;
use crate::machine::fdt::{FdtAttachArgs, hvc_call, set_cpuresetfn, set_powerdownfn, smc_call};
use crate::sys::device::{CfMatch, Cfattach, Cfdriver, DV_DULL, Device, Softc};

/// `SMCCC_VERSION`.
const SMCCC_VERSION: u32 = 0x8000_0000;
/// `SMCCC_ARCH_FEATURES`.
const SMCCC_ARCH_FEATURES: u32 = 0x8000_0001;
/// `SMCCC_ARCH_WORKAROUND_1`.
const SMCCC_ARCH_WORKAROUND_1: u32 = 0x8000_8000;
/// `SMCCC_ARCH_WORKAROUND_2`.
const SMCCC_ARCH_WORKAROUND_2: u32 = 0x8000_7fff;
/// `SMCCC_ARCH_WORKAROUND_3`.
const SMCCC_ARCH_WORKAROUND_3: u32 = 0x8000_3fff;

/// `sc_callfn`'s type: `hvc_call` or `smc_call`.
type CallFn = fn(u64, u64, u64, u64) -> u64;

/// `struct psci_softc`.
#[repr(C)]
pub struct PsciSoftc {
    /// `sc_dev`.
    pub sc_dev: Device,
    /// `sc_callfn`.
    pub sc_callfn: Cell<Option<CallFn>>,
    /// `sc_psci_version`.
    pub sc_psci_version: Cell<u32>,
    /// `sc_system_off`.
    pub sc_system_off: Cell<u32>,
    /// `sc_system_reset`.
    pub sc_system_reset: Cell<u32>,
    /// `sc_system_suspend`.
    pub sc_system_suspend: Cell<u32>,
    /// `sc_cpu_on`.
    pub sc_cpu_on: Cell<u32>,
    /// `sc_cpu_off`.
    pub sc_cpu_off: Cell<u32>,
    /// `sc_cpu_suspend`.
    pub sc_cpu_suspend: Cell<u32>,
    /// `sc_smccc_version`.
    pub sc_smccc_version: Cell<u32>,
    /// `sc_method`.
    pub sc_method: Cell<i32>,
}

// SAFETY: `#[repr(C)]` with the `struct device` first, as `config_make_softc` requires.
unsafe impl Softc for PsciSoftc {}

/// `psci_sc`.
static PSCI_SC: AtomicPtr<PsciSoftc> = AtomicPtr::new(core::ptr::null_mut());

/// `psci_ca`.
pub static PSCI_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<PsciSoftc>(),
    ca_match: Some(psci_match),
    ca_attach: psci_attach,
    ca_detach: None,
    ca_activate: None,
};

/// `psci_cd`.
pub static PSCI_CD: Cfdriver = Cfdriver::new(b"psci", DV_DULL, 0);

/// `psci_sc`, if `psci_attach` has run.
fn psci_sc() -> Option<&'static PsciSoftc> {
    // SAFETY: the pointer is null or the attached softc, which is never freed.
    unsafe { PSCI_SC.load(Ordering::Acquire).as_ref() }
}

/// `psci_match`: `arm,psci`, `arm,psci-0.2` or `arm,psci-1.0`.
pub fn psci_match(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: the device-tree buses hand their children `struct fdt_attach_args`.
    let faa = unsafe { &*aux.cast::<FdtAttachArgs<'_>>() };

    i32::from(
        OF_is_compatible(faa.fa_node, b"arm,psci")
            || OF_is_compatible(faa.fa_node, b"arm,psci-0.2")
            || OF_is_compatible(faa.fa_node, b"arm,psci-1.0"),
    )
}

/// `psci_attach`: pick the conduit and the function ids, report the versions, and hook
/// `boot(9)`'s power-off and reset.
pub fn psci_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: as in `psci_match`.
    let faa = unsafe { &*aux.cast::<FdtAttachArgs<'_>>() };
    // SAFETY: `psci_ca` makes `PsciSoftc`s; `config_make_softc`'s allocation lives as long as
    // the device, which is never detached.
    let sc: &'static PsciSoftc = unsafe { &*core::ptr::from_ref(self_.softc::<PsciSoftc>()) };
    let mut method = [0u8; 128];

    // C: `if (OF_getprop(...))`, true for -1 as well; the zeroed buffer then matches nothing.
    if OF_getprop(faa.fa_node, b"method", &mut method) != 0 {
        let len = method.iter().position(|&b| b == 0).unwrap_or(method.len());
        match &method[..len] {
            b"hvc" => {
                sc.sc_callfn.set(Some(hvc_call));
                sc.sc_method.set(PSCI_METHOD_HVC);
            }
            b"smc" => {
                sc.sc_callfn.set(Some(smc_call));
                sc.sc_method.set(PSCI_METHOD_SMC);
            }
            _ => {}
        }
    }

    // The function IDs are only to be parsed for the old specification (as in version 0.1).
    // All newer implementations are supposed to use the specified values.
    if OF_is_compatible(faa.fa_node, b"arm,psci-0.2")
        || OF_is_compatible(faa.fa_node, b"arm,psci-1.0")
    {
        sc.sc_psci_version.set(PSCI_VERSION);
        sc.sc_system_off.set(SYSTEM_OFF);
        sc.sc_system_reset.set(SYSTEM_RESET);
        sc.sc_cpu_on.set(CPU_ON);
        sc.sc_cpu_off.set(CPU_OFF);
        sc.sc_cpu_suspend.set(CPU_SUSPEND);
    } else if OF_is_compatible(faa.fa_node, b"arm,psci") {
        sc.sc_system_off
            .set(OF_getpropint(faa.fa_node, b"system_off", 0));
        sc.sc_system_reset
            .set(OF_getpropint(faa.fa_node, b"system_reset", 0));
        sc.sc_cpu_on.set(OF_getpropint(faa.fa_node, b"cpu_on", 0));
        sc.sc_cpu_off.set(OF_getpropint(faa.fa_node, b"cpu_off", 0));
        sc.sc_cpu_suspend
            .set(OF_getpropint(faa.fa_node, b"cpu_suspend", 0));
    }

    PSCI_SC.store(core::ptr::from_ref(sc).cast_mut(), Ordering::Release);

    let version = psci_version();
    printf(format_args!(
        ": PSCI {}.{}",
        version >> 16,
        version & 0xffff
    ));

    if version >= 0x10000 {
        if psci_features(SMCCC_VERSION) == PSCI_SUCCESS {
            sc.sc_smccc_version.set(smccc_version() as u32);
            let v = sc.sc_smccc_version.get();
            printf(format_args!(", SMCCC {}.{}", v >> 16, v & 0xffff));
        }
        if psci_features(SYSTEM_SUSPEND) == PSCI_SUCCESS {
            sc.sc_system_suspend.set(SYSTEM_SUSPEND);
            printf(format_args!(", SYSTEM_SUSPEND"));
        }
    }

    printf(format_args!("\n"));

    if sc.sc_system_off.get() != 0 {
        set_powerdownfn(psci_powerdown);
    }
    if sc.sc_system_reset.get() != 0 {
        set_cpuresetfn(psci_reset);
    }
}

/// `psci_reset`: `SYSTEM_RESET`; returns only if the firmware did not reset the machine.
pub fn psci_reset() {
    let Some(sc) = psci_sc() else { return };

    if let Some(callfn) = sc.sc_callfn.get() {
        callfn(u64::from(sc.sc_system_reset.get()), 0, 0, 0);
    }
}

/// `psci_powerdown`: `SYSTEM_OFF`; returns only if the firmware did not power off.
pub fn psci_powerdown() {
    let Some(sc) = psci_sc() else { return };

    if let Some(callfn) = sc.sc_callfn.get() {
        callfn(u64::from(sc.sc_system_off.get()), 0, 0, 0);
    }
}

/*
 * Firmware-based workaround for CVE-2017-5715.  We determine whether
 * the workaround is actually implemented and needed the first time we
 * are invoked such that we only make the firmware call when appropriate.
 */

/// `psci_flush_bp_none`.
pub fn psci_flush_bp_none() {}

/// `psci_flush_bp_smccc_arch_workaround_1`.
pub fn psci_flush_bp_smccc_arch_workaround_1() {
    if let Some(callfn) = psci_sc().and_then(|sc| sc.sc_callfn.get()) {
        callfn(u64::from(SMCCC_ARCH_WORKAROUND_1), 0, 0, 0);
    }
}

/// `psci_flush_bp`: decide, once, what the current CPU's `ci_flush_bp` is, and flush now
/// (`ci_flush_bp` is the cell of `curcpu()`; see the module's deviations).
pub fn psci_flush_bp(ci_flush_bp: &Cell<Option<fn()>>) {
    // SMCCC 1.1 allows us to detect if the workaround is implemented and needed.
    if let Some(sc) = psci_sc()
        && sc.sc_smccc_version.get() >= 0x10001
        && smccc_arch_features(SMCCC_ARCH_WORKAROUND_1) == 0
    {
        // Workaround implemented and needed.
        ci_flush_bp.set(Some(psci_flush_bp_smccc_arch_workaround_1));
        psci_flush_bp_smccc_arch_workaround_1();
    } else {
        // Workaround isn't implemented or isn't needed.
        ci_flush_bp.set(Some(psci_flush_bp_none));
    }
}

/// `smccc_enable_arch_workaround_2`: Spectre-V4's firmware workaround, when implemented and
/// needed.
pub fn smccc_enable_arch_workaround_2() {
    // SMCCC 1.1 allows us to detect if the workaround is implemented and needed.
    if let Some(sc) = psci_sc()
        && sc.sc_smccc_version.get() >= 0x10001
        && smccc_arch_features(SMCCC_ARCH_WORKAROUND_2) == 0
        && let Some(callfn) = sc.sc_callfn.get()
    {
        // Workaround implemented and needed.
        callfn(u64::from(SMCCC_ARCH_WORKAROUND_2), 1, 0, 0);
    }
}

/// `smccc_needs_arch_workaround_3`: whether Spectre-BHB's firmware workaround is
/// implemented and needed.
pub fn smccc_needs_arch_workaround_3() -> i32 {
    // SMCCC 1.1 allows us to detect if the workaround is implemented and needed.
    if let Some(sc) = psci_sc()
        && sc.sc_smccc_version.get() >= 0x10001
        && smccc_arch_features(SMCCC_ARCH_WORKAROUND_3) == 0
    {
        // Workaround implemented and needed.
        return 1;
    }

    0
}

/// `KASSERT(sc && sc->sc_callfn)`, and the call function.
fn callfn_asserted() -> CallFn {
    let callfn = psci_sc().and_then(|sc| sc.sc_callfn.get());
    kassert!(callfn.is_some());
    callfn.unwrap_or(|_, _, _, _| PSCI_NOT_SUPPORTED as u64)
}

/// `smccc_version`.
pub fn smccc_version() -> i32 {
    let version = callfn_asserted()(u64::from(SMCCC_VERSION), 0, 0, 0) as i32;
    if version != PSCI_NOT_SUPPORTED {
        return version;
    }

    // Treat NOT_SUPPORTED as 1.0
    0x10000
}

/// `smccc`: an SMCCC call, or `PSCI_NOT_SUPPORTED` without a conduit.
pub fn smccc(func_id: u32, arg0: u64, arg1: u64, arg2: u64) -> i32 {
    if let Some(callfn) = psci_sc().and_then(|sc| sc.sc_callfn.get()) {
        return callfn(u64::from(func_id), arg0, arg1, arg2) as i32;
    }

    PSCI_NOT_SUPPORTED
}

/// `smccc_arch_features`.
pub fn smccc_arch_features(arch_func_id: u32) -> i32 {
    callfn_asserted()(
        u64::from(SMCCC_ARCH_FEATURES),
        u64::from(arch_func_id),
        0,
        0,
    ) as i32
}

/// `psci_version`: the firmware's version, 0.0 without support.
pub fn psci_version() -> u32 {
    if let Some(sc) = psci_sc()
        && let Some(callfn) = sc.sc_callfn.get()
        && sc.sc_psci_version.get() != 0
    {
        return callfn(u64::from(sc.sc_psci_version.get()), 0, 0, 0) as u32;
    }

    // No version support; return 0.0.
    0
}

/// A PSCI call of the softc's function `select`, or `PSCI_NOT_SUPPORTED` when there is no
/// conduit or the id is 0.
fn psci_call(select: fn(&PsciSoftc) -> u32, a0: u64, a1: u64, a2: u64) -> i32 {
    if let Some(sc) = psci_sc()
        && let Some(callfn) = sc.sc_callfn.get()
        && select(sc) != 0
    {
        return callfn(u64::from(select(sc)), a0, a1, a2) as i32;
    }

    PSCI_NOT_SUPPORTED
}

/// `psci_system_suspend`.
pub fn psci_system_suspend(entry_point_address: u64, context_id: u64) -> i32 {
    psci_call(
        |sc| sc.sc_system_suspend.get(),
        entry_point_address,
        context_id,
        0,
    )
}

/// `psci_cpu_off`.
pub fn psci_cpu_off() -> i32 {
    psci_call(|sc| sc.sc_cpu_off.get(), 0, 0, 0)
}

/// `psci_cpu_on`: start the CPU `target_cpu` at `entry_point_address` (after a boot by
/// boot(8); under Limine, Limine starts the application processors).
pub fn psci_cpu_on(target_cpu: u64, entry_point_address: u64, context_id: u64) -> i32 {
    psci_call(
        |sc| sc.sc_cpu_on.get(),
        target_cpu,
        entry_point_address,
        context_id,
    )
}

/// `psci_cpu_suspend`.
pub fn psci_cpu_suspend(power_state: u64, entry_point_address: u64, context_id: u64) -> i32 {
    psci_call(
        |sc| sc.sc_cpu_suspend.get(),
        power_state,
        entry_point_address,
        context_id,
    )
}

/// `psci_features`: `PSCI_FEATURES` for `psci_func_id`.
pub fn psci_features(psci_func_id: u32) -> i32 {
    if let Some(callfn) = psci_sc().and_then(|sc| sc.sc_callfn.get()) {
        return callfn(u64::from(PSCI_FEATURES), u64::from(psci_func_id), 0, 0) as i32;
    }

    PSCI_NOT_SUPPORTED
}

/// `psci_can_suspend`: whether `SYSTEM_SUSPEND` is implemented.
pub fn psci_can_suspend() -> i32 {
    i32::from(psci_sc().is_some_and(|sc| sc.sc_system_suspend.get() != 0))
}

/// `psci_method`: `PSCI_METHOD_HVC`, `PSCI_METHOD_SMC` or `PSCI_METHOD_NONE`.
pub fn psci_method() -> i32 {
    psci_sc().map_or(PSCI_METHOD_NONE, |sc| sc.sc_method.get())
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_a_psci_node_every_call_is_unsupported() {
        // psci_attach never ran on the host: no softc, no conduit.
        assert_eq!(psci_version(), 0);
        assert_eq!(psci_features(SYSTEM_OFF), PSCI_NOT_SUPPORTED);
        assert_eq!(psci_cpu_on(1, 0x1000, 0), PSCI_NOT_SUPPORTED);
        assert_eq!(psci_cpu_off(), PSCI_NOT_SUPPORTED);
        assert_eq!(psci_system_suspend(0, 0), PSCI_NOT_SUPPORTED);
        assert_eq!(smccc(SMCCC_VERSION, 0, 0, 0), PSCI_NOT_SUPPORTED);
        assert_eq!(psci_can_suspend(), 0);
        assert_eq!(psci_method(), PSCI_METHOD_NONE);
        assert_eq!(smccc_needs_arch_workaround_3(), 0);
    }
}
/* </TESTS> */
