/* $OpenBSD: acpitimer.c,v 1.18 2025/09/16 12:18:10 hshoexer Exp $ */
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

//! acpitimer(4): `dev/acpi/acpitimer.c`, the ACPI power management timer. The FADT names a
//! free-running counter at 3.579545 MHz, 24 bits wide (32 with `FADT_TMR_VAL_EXT`), in I/O
//! space (or wherever `X_PM_TMR_BLK` says). acpitimer0 registers it as a timecounter of
//! quality 1000, makes `acpitimer_delay` the `delay(9)` implementation (quality 1000) and,
//! on amd64, offers it to `tsc.c` as the reference the TSC's frequency is measured against
//! (`cpu_recalibrate_tsc`).
//!
//! Upstream: sys/dev/acpi/acpitimer.c @ 3ce1f3f79392
//!
//! acpi0 attaches it by name once ACPI is enabled, when the FADT has a timer block and the
//! machine is not hardware-reduced (`acpi_attach_common`); amd64's ioconf has
//! `acpitimer* at acpi?`, as GENERIC.
//!
//! ## Deviations
//! - `acpi_timecounter.tc_name` is a `Cell` (`sys/timetc.rs`): the C points it at
//!   `sc_dev.dv_xname` after its static initialiser.
//! - `acpitimer_delay` treats a negative `usecs` as 0 (the C converts it to a huge
//!   `uint64_t` and spins for ages). `CPU_BUSY_CYCLE()` is `core::hint::spin_loop()`.
//! - `delay_init` and `cpu_recalibrate_tsc` come through `machine::acpi_machdep`; the latter
//!   does nothing but on amd64, where the C calls it (`#if defined(__amd64__)`).

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr;

use super::acpi::{aaa_name, acpi_map_address, fadt};
use super::acpireg::{ACPI_FREQUENCY, FADT_TMR_VAL_EXT};
use super::acpivar::{AcpiAttachArgs, AcpiSoftc};
use crate::kern::kern_tc::tc_init;
use crate::kprintf;
use crate::machine::acpi_machdep::{cpu_recalibrate_tsc, delay_init};
use crate::machine::bus::{BusSpaceHandle, BusSpaceTag, bus_space_read_4};
use crate::sys::device::{CD_COCOVM, CfMatch, Cfattach, Cfdriver, DV_DULL, Device, Softc};
use crate::sys::timetc::Timecounter;

/// `struct acpitimer_softc`.
#[repr(C)]
pub struct AcpitimerSoftc {
    /// `sc_dev`.
    pub sc_dev: Device,
    /// `sc_iot`.
    pub sc_iot: Cell<Option<BusSpaceTag>>,
    /// `sc_ioh`.
    pub sc_ioh: Cell<Option<BusSpaceHandle>>,
}

// SAFETY: `#[repr(C)]` with the device first; all-zero is no tag and no handle.
unsafe impl Softc for AcpitimerSoftc {}

/// `acpi_timecounter`: `tc_priv` is acpitimer0's softc once it attaches.
static ACPI_TIMECOUNTER: Timecounter = Timecounter::new(
    acpi_get_timecount,
    0x00ff_ffff, // 24 bits
    ACPI_FREQUENCY as u64,
    "",
    1000,
    0,
);

/// `acpitimer_ca`.
pub static ACPITIMER_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<AcpitimerSoftc>(),
    ca_match: Some(acpitimermatch),
    ca_attach: acpitimerattach,
    ca_detach: None,
    ca_activate: None,
};

/// `acpitimer_cd`.
pub static ACPITIMER_CD: Cfdriver = Cfdriver::new(b"acpitimer", DV_DULL, CD_COCOVM);

/// `acpitimermatch(parent, match, aux)`: acpi0's `"acpitimer"` attach arguments, which carry
/// no table.
pub fn acpitimermatch(_parent: Option<&Device>, match_: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: acpi0 hands its children `struct acpi_attach_args`.
    let aa = unsafe { &*aux.cast_const().cast::<AcpiAttachArgs>() };
    let cf = match_.cfdata();

    // sanity
    match aaa_name(aa) {
        Some(name) if name == cf.cf_driver.cd_name && aa.aaa_table.is_null() => 1,
        _ => 0,
    }
}

/// `acpitimerattach(parent, self, aux)`: maps the timer block, registers the timecounter and
/// the delay function, and offers the timer as the TSC's reference.
pub fn acpitimerattach(parent: Option<&Device>, self_: &Device, _aux: *mut c_void) {
    // SAFETY: `acpitimer_ca`'s softc is an `AcpitimerSoftc` (`ca_devsize`).
    let sc: &AcpitimerSoftc = unsafe { self_.softc() };
    // SAFETY: a softc `config_attach` allocated is never freed (acpitimer does not detach).
    let sc: &'static AcpitimerSoftc = unsafe { &*ptr::from_ref(sc) };
    let Some(parent) = parent else {
        return;
    };
    // SAFETY: acpitimer attaches only at acpi0 (`acpitimer* at acpi?`), whose softc is an
    // `AcpiSoftc`.
    let psc: &AcpiSoftc = unsafe { parent.softc() };
    let f = fadt(psc);

    let gas = { f.x_pm_tmr_blk };
    let rc = if f.hdr.revision >= 3 && { gas.address } != 0 {
        acpi_map_address(psc, Some(&gas), 0, usize::from(f.pm_tmr_len))
    } else {
        acpi_map_address(
            psc,
            None,
            { f.pm_tmr_blk } as usize,
            usize::from(f.pm_tmr_len),
        )
    };
    let Ok((ioh, iot)) = rc else {
        kprintf!(": can't map i/o space\n");
        return;
    };
    sc.sc_ioh.set(Some(ioh));
    sc.sc_iot.set(Some(iot));

    let ext = { f.flags } & FADT_TMR_VAL_EXT != 0;
    kprintf!(
        ": {} Hz, {} bits\n",
        ACPI_FREQUENCY,
        if ext { 32 } else { 24 }
    );

    if ext {
        ACPI_TIMECOUNTER.tc_counter_mask.set(0xffff_ffff);
    }
    ACPI_TIMECOUNTER.tc_priv.set(ptr::from_ref(sc).cast());
    let name: &'static str = sc.sc_dev.xname();
    ACPI_TIMECOUNTER.tc_name.set(name);
    tc_init(&ACPI_TIMECOUNTER);

    delay_init(acpitimer_delay, 1000);

    // #if defined(__amd64__)
    cpu_recalibrate_tsc(&ACPI_TIMECOUNTER);
}

/// `acpitimer_delay(usecs)`: busy-waits `usecs` microseconds on the PM timer.
pub fn acpitimer_delay(usecs: i32) {
    let Some(sc) = acpitimer_softc(&ACPI_TIMECOUNTER) else {
        return;
    };
    let mask = ACPI_TIMECOUNTER.tc_counter_mask.get();
    let mut count: u64 = 0;

    let mut val2 = acpitimer_read(sc);
    let cycles = acpitimer_cycles(usecs, ACPI_TIMECOUNTER.tc_frequency.get());
    while count < cycles {
        core::hint::spin_loop(); // CPU_BUSY_CYCLE()
        let val1 = val2;
        val2 = acpitimer_read(sc);
        count += u64::from(val2.wrapping_sub(val1) & mask);
    }
}

/// `usecs * tc_frequency / 1000000`: the counter ticks in `usecs` microseconds (0 for a
/// negative count).
fn acpitimer_cycles(usecs: i32, frequency: u64) -> u64 {
    u64::from(usecs.max(0) as u32) * frequency / 1_000_000
}

/// `acpi_get_timecount(tc)`.
pub fn acpi_get_timecount(tc: &Timecounter) -> u32 {
    acpitimer_softc(tc).map_or(0, acpitimer_read)
}

/// `tc->tc_priv`: acpitimer0's softc, once it has attached.
fn acpitimer_softc(tc: &Timecounter) -> Option<&'static AcpitimerSoftc> {
    // SAFETY: `tc_priv` is null or the `&'static AcpitimerSoftc` `acpitimerattach` stored.
    unsafe { tc.tc_priv.get().cast::<AcpitimerSoftc>().as_ref() }
}

/// `acpitimer_read(sc)`: the counter, read until three readings in a row are in order (the
/// timer may be read while it ripples).
pub fn acpitimer_read(sc: &AcpitimerSoftc) -> u32 {
    let (Some(iot), Some(ioh)) = (sc.sc_iot.get(), sc.sc_ioh.get()) else {
        return 0;
    };

    let mut u2 = bus_space_read_4(iot, ioh, 0);
    let mut u3 = bus_space_read_4(iot, ioh, 0);
    loop {
        let u1 = u2;
        u2 = u3;
        u3 = bus_space_read_4(iot, ioh, 0);
        if !(u1 > u2 || u2 > u3) {
            break;
        }
    }

    u2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delay_cycles() {
        assert_eq!(
            acpitimer_cycles(1_000_000, u64::from(ACPI_FREQUENCY)),
            3_579_545
        );
        assert_eq!(acpitimer_cycles(1000, u64::from(ACPI_FREQUENCY)), 3579);
        assert_eq!(acpitimer_cycles(-5, u64::from(ACPI_FREQUENCY)), 0);
    }

    #[test]
    fn no_softc_reads_zero() {
        assert!(acpitimer_softc(&ACPI_TIMECOUNTER).is_none());
        assert_eq!(acpi_get_timecount(&ACPI_TIMECOUNTER), 0);
        assert_eq!(ACPI_TIMECOUNTER.tc_counter_mask.get(), 0x00ff_ffff);
    }
}
