/* $OpenBSD: acpihpet.c,v 1.32 2025/09/16 12:18:10 hshoexer Exp $ */
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

//! acpihpet(4): `dev/acpi/acpihpet.c`, the High Precision Event Timer the ACPI `HPET` table
//! describes. acpihpet0 maps its registers, starts the main counter, checks that it counts
//! and that its period is sane, and registers the low 32 bits of the counter as a
//! timecounter of quality 1000 and `acpihpet_delay` as `delay(9)` (quality 2000). On amd64 it
//! offers the counter to `tsc.c` as the TSC's reference (`cpu_recalibrate_tsc`). Its
//! comparators are not used: the HPET is a counter here, not an interrupt source. On
//! suspend it saves the registers and on resume it restores them.
//!
//! Upstream: sys/dev/acpi/acpihpet.c @ 3ce1f3f79392
//!
//! acpi0 offers it every table (`config_found_sm` with `acpi_submatch`); amd64's ioconf has
//! `acpihpet* at acpi?`, as GENERIC.
//!
//! ## Deviations
//! - `hpet_timecounter.tc_name` is a `Cell` (`sys/timetc.rs`): the C points it at
//!   `sc_dev.dv_xname` after its static initialiser.
//! - `acpihpet_delay` treats a negative `usecs` as 0 (the C converts it to a huge
//!   `uint64_t`). `CPU_BUSY_CYCLE()` is `core::hint::spin_loop()`.
//! - `struct hpet_regs` (`sc_save`) is a `Cell` of a plain copyable value; the suspend and
//!   resume paths walk `timers[]` in a loop, register by register in the C's order.
//! - `delay_init`, `delay_fini` and `cpu_recalibrate_tsc` come through
//!   `machine::acpi_machdep`; the last does nothing but on amd64, where the C calls it
//!   (`#if defined(__amd64__)`). Nothing suspends yet (`subr_suspend.c` is not ported), so
//!   `acpihpet_activate` is reached only through `config_suspend`.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::{AtomicI32, Ordering};

use super::acpi::acpi_map_address;
use super::acpidev::{
    HPET_CAPABILITIES, HPET_CONFIGURATION, HPET_INTERRUPT_STATUS, HPET_MAIN_COUNTER,
    HPET_MAX_PERIOD, HPET_REG_SIZE, HPET_TIMER0_COMPARE, HPET_TIMER0_CONFIG, HPET_TIMER0_INTERRUPT,
    HPET_TIMER1_COMPARE, HPET_TIMER1_CONFIG, HPET_TIMER1_INTERRUPT, HPET_TIMER2_COMPARE,
    HPET_TIMER2_CONFIG, HPET_TIMER2_INTERRUPT,
};
use super::acpireg::{AcpiHpet, AcpiTableHeader, HPET_SIG};
use super::acpivar::{AcpiAttachArgs, AcpiSoftc};
use crate::kern::kern_tc::tc_init;
use crate::kprintf;
use crate::machine::acpi_machdep::{cpu_recalibrate_tsc, delay_fini, delay_init};
use crate::machine::bus::{
    BusSize, BusSpaceHandle, BusSpaceTag, bus_space_read_4, bus_space_write_4,
};
use crate::machine::cpu::delay;
use crate::sys::device::{
    CD_COCOVM, CfMatch, Cfattach, Cfdriver, DV_DULL, DVACT_RESUME, DVACT_SUSPEND, Device, Softc,
};
use crate::sys::errno::Errno;
use crate::sys::timetc::Timecounter;

/// `HPET_TIMERS`: the comparators saved across a suspend.
const HPET_TIMERS: usize = 3;

/// The `HPET_TIMERn_{CONFIG,INTERRUPT,COMPARE}` offsets of each saved comparator.
const HPET_TIMER_REGS: [(BusSize, BusSize, BusSize); HPET_TIMERS] = [
    (
        HPET_TIMER0_CONFIG,
        HPET_TIMER0_INTERRUPT,
        HPET_TIMER0_COMPARE,
    ),
    (
        HPET_TIMER1_CONFIG,
        HPET_TIMER1_INTERRUPT,
        HPET_TIMER1_COMPARE,
    ),
    (
        HPET_TIMER2_CONFIG,
        HPET_TIMER2_INTERRUPT,
        HPET_TIMER2_COMPARE,
    ),
];

/// One comparator of `struct hpet_regs`.
#[derive(Clone, Copy, Default)]
pub struct HpetTimerRegs {
    /// `config`.
    pub config: u64,
    /// `compare`.
    pub compare: u64,
    /// `interrupt`.
    pub interrupt: u64,
}

/// `struct hpet_regs`: the registers saved across a suspend.
#[derive(Clone, Copy, Default)]
pub struct HpetRegs {
    /// `configuration`.
    pub configuration: u64,
    /// `interrupt_status`.
    pub interrupt_status: u64,
    /// `main_counter`.
    pub main_counter: u64,
    /// `timers`.
    pub timers: [HpetTimerRegs; HPET_TIMERS],
}

/// `struct acpihpet_softc`.
#[repr(C)]
pub struct AcpihpetSoftc {
    /// `sc_dev`.
    pub sc_dev: Device,
    /// `sc_iot`.
    pub sc_iot: Cell<Option<BusSpaceTag>>,
    /// `sc_ioh`.
    pub sc_ioh: Cell<Option<BusSpaceHandle>>,
    /// `sc_conf`: the configuration register with the enable bit clear.
    pub sc_conf: Cell<u32>,
    /// `sc_save`.
    pub sc_save: Cell<HpetRegs>,
}

// SAFETY: `#[repr(C)]` with the device first; all-zero is no tag, no handle and zeroed
// registers.
unsafe impl Softc for AcpihpetSoftc {}

/// `acpihpet_attached`: an HPET attached, so no other table is taken.
static ACPIHPET_ATTACHED: AtomicI32 = AtomicI32::new(0);

/// `hpet_timecounter`: `tc_frequency` from the period, `tc_priv` acpihpet0's softc.
static HPET_TIMECOUNTER: Timecounter =
    Timecounter::new(acpihpet_gettime, 0xffff_ffff, 0, "", 1000, 0);

/// `acpihpet_ca`.
pub static ACPIHPET_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<AcpihpetSoftc>(),
    ca_match: Some(acpihpet_match),
    ca_attach: acpihpet_attach,
    ca_detach: None,
    ca_activate: Some(acpihpet_activate),
};

/// `acpihpet_cd`.
pub static ACPIHPET_CD: Cfdriver = Cfdriver::new(b"acpihpet", DV_DULL, CD_COCOVM);

/// The softc's tag and handle, once `acpihpet_attach` mapped the registers.
fn regs(sc: &AcpihpetSoftc) -> Option<(BusSpaceTag, BusSpaceHandle)> {
    Some((sc.sc_iot.get()?, sc.sc_ioh.get()?))
}

/// `acpihpet_r(iot, ioh, ioa)`: a 64-bit register as two 32-bit reads, high word first.
pub fn acpihpet_r(iot: BusSpaceTag, ioh: BusSpaceHandle, ioa: BusSize) -> u64 {
    let hi = u64::from(bus_space_read_4(iot, ioh, ioa + 4));
    let lo = u64::from(bus_space_read_4(iot, ioh, ioa));
    (hi << 32) | lo
}

/// `acpihpet_w(iot, ioh, ioa, val)`: a 64-bit register as two 32-bit writes, high word first.
pub fn acpihpet_w(iot: BusSpaceTag, ioh: BusSpaceHandle, ioa: BusSize, val: u64) {
    bus_space_write_4(iot, ioh, ioa + 4, (val >> 32) as u32);
    bus_space_write_4(iot, ioh, ioa, val as u32);
}

/// `acpihpet_activate(self, act)`: on suspend gives up `delay(9)`, stops the counter and
/// saves the registers; on resume stops it, restores them, restarts it and takes `delay(9)`
/// back.
pub fn acpihpet_activate(self_: &Device, act: i32) -> Result<(), Errno> {
    // SAFETY: `acpihpet_ca`'s softc is an `AcpihpetSoftc` (`ca_devsize`).
    let sc: &AcpihpetSoftc = unsafe { self_.softc() };
    let Some((iot, ioh)) = regs(sc) else {
        return Ok(());
    };

    match act {
        DVACT_SUSPEND => {
            delay_fini(acpihpet_delay);

            // stop, then save
            bus_space_write_4(iot, ioh, HPET_CONFIGURATION, sc.sc_conf.get());

            let mut save = HpetRegs {
                configuration: acpihpet_r(iot, ioh, HPET_CONFIGURATION),
                interrupt_status: acpihpet_r(iot, ioh, HPET_INTERRUPT_STATUS),
                main_counter: acpihpet_r(iot, ioh, HPET_MAIN_COUNTER),
                ..HpetRegs::default()
            };
            for (t, &(config, interrupt, compare)) in
                save.timers.iter_mut().zip(HPET_TIMER_REGS.iter())
            {
                t.config = acpihpet_r(iot, ioh, config);
                t.interrupt = acpihpet_r(iot, ioh, interrupt);
                t.compare = acpihpet_r(iot, ioh, compare);
            }
            sc.sc_save.set(save);
        }
        DVACT_RESUME => {
            // stop, restore, then restart
            bus_space_write_4(iot, ioh, HPET_CONFIGURATION, sc.sc_conf.get());

            let save = sc.sc_save.get();
            acpihpet_w(iot, ioh, HPET_CONFIGURATION, save.configuration);
            acpihpet_w(iot, ioh, HPET_INTERRUPT_STATUS, save.interrupt_status);
            acpihpet_w(iot, ioh, HPET_MAIN_COUNTER, save.main_counter);
            for (t, &(config, interrupt, compare)) in save.timers.iter().zip(HPET_TIMER_REGS.iter())
            {
                acpihpet_w(iot, ioh, config, t.config);
                acpihpet_w(iot, ioh, interrupt, t.interrupt);
                acpihpet_w(iot, ioh, compare, t.compare);
            }
            bus_space_write_4(iot, ioh, HPET_CONFIGURATION, sc.sc_conf.get() | 1);

            delay_init(acpihpet_delay, 2000);
        }
        _ => {}
    }

    Ok(())
}

/// `acpihpet_match(parent, match, aux)`: the first `HPET` table acpi0 offers.
pub fn acpihpet_match(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: acpi0 hands its children `struct acpi_attach_args`.
    let aaa = unsafe { &*aux.cast_const().cast::<AcpiAttachArgs>() };

    // If we do not have a table, it is not us; attach only once
    if ACPIHPET_ATTACHED.load(Ordering::Relaxed) != 0 || aaa.aaa_table.is_null() {
        return 0;
    }

    // If it is an HPET table, we can attach
    // SAFETY: `aaa_table` is a table `acpi_maptable` copied, at least a header long.
    let hdr = unsafe { ptr::read_unaligned(aaa.aaa_table.cast_const().cast::<AcpiTableHeader>()) };
    if hdr.signature != *HPET_SIG {
        return 0;
    }

    1
}

/// `acpihpet_attach(parent, self, aux)`: maps and enables the HPET, checks it, and registers
/// its timecounter and delay function.
pub fn acpihpet_attach(parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: `acpihpet_ca`'s softc is an `AcpihpetSoftc` (`ca_devsize`).
    let sc: &AcpihpetSoftc = unsafe { self_.softc() };
    // SAFETY: a softc `config_attach` allocated is never freed (acpihpet does not detach).
    let sc: &'static AcpihpetSoftc = unsafe { &*ptr::from_ref(sc) };
    // SAFETY: acpi0 hands its children `struct acpi_attach_args`.
    let aaa = unsafe { &*aux.cast_const().cast::<AcpiAttachArgs>() };
    let Some(parent) = parent else {
        return;
    };
    // SAFETY: acpihpet attaches only at acpi0 (`acpihpet* at acpi?`), whose softc is an
    // `AcpiSoftc`.
    let psc: &AcpiSoftc = unsafe { parent.softc() };
    // SAFETY: `acpihpet_match` took this table for an HPET table; `acpi_maptable` zero-pads
    // every copy to at least `sizeof(struct acpi_fadt)`, longer than `struct acpi_hpet`.
    let hpet = unsafe { ptr::read_unaligned(aaa.aaa_table.cast_const().cast::<AcpiHpet>()) };
    let gas = { hpet.base_address };

    let Ok((ioh, iot)) = acpi_map_address(psc, Some(&gas), 0, HPET_REG_SIZE) else {
        kprintf!(": can't map i/o space\n");
        return;
    };
    sc.sc_ioh.set(Some(ioh));
    sc.sc_iot.set(Some(iot));

    // Revisions 0x30 through 0x3a of the AMD SB700, with spread spectrum enabled, have an
    // SMM based HPET emulation that's subtly broken. The hardware is initialized upon first
    // access of the configuration register. Initialization takes some time during which the
    // configuration register returns 0xffffffff.
    let mut timeout = 1000;
    loop {
        if bus_space_read_4(iot, ioh, HPET_CONFIGURATION) != 0xffff_ffff {
            break;
        }
        timeout -= 1;
        if timeout <= 0 {
            break;
        }
    }

    if timeout == 0 {
        kprintf!(": disabled\n");
        return;
    }

    // enable hpet
    let conf = bus_space_read_4(iot, ioh, HPET_CONFIGURATION) & !1;
    sc.sc_conf.set(conf);
    bus_space_write_4(iot, ioh, HPET_CONFIGURATION, conf | 1);

    // make sure hpet is working
    let v1 = bus_space_read_4(iot, ioh, HPET_MAIN_COUNTER);
    delay(1);
    let v2 = bus_space_read_4(iot, ioh, HPET_MAIN_COUNTER);
    if v1 == v2 {
        kprintf!(": counter not incrementing\n");
        bus_space_write_4(iot, ioh, HPET_CONFIGURATION, conf);
        return;
    }

    // timer period in femtoseconds (10^-15)
    let period = u64::from(bus_space_read_4(
        iot,
        ioh,
        HPET_CAPABILITIES + size_of::<u32>(),
    ));

    // Period must be > 0 and less than 100ns (10^8 fs)
    let Some(freq) = acpihpet_freq(period) else {
        kprintf!(": invalid period\n");
        bus_space_write_4(iot, ioh, HPET_CONFIGURATION, conf);
        return;
    };
    kprintf!(": {} Hz\n", freq);

    HPET_TIMECOUNTER.tc_frequency.set(freq);
    HPET_TIMECOUNTER.tc_priv.set(ptr::from_ref(sc).cast());
    let name: &'static str = sc.sc_dev.xname();
    HPET_TIMECOUNTER.tc_name.set(name);
    tc_init(&HPET_TIMECOUNTER);

    delay_init(acpihpet_delay, 2000);

    // #if defined(__amd64__)
    cpu_recalibrate_tsc(&HPET_TIMECOUNTER);

    ACPIHPET_ATTACHED.fetch_add(1, Ordering::Relaxed);
}

/// The frequency of a counter that ticks every `period` femtoseconds, if the period is valid
/// (`0 < period <= HPET_MAX_PERIOD`).
fn acpihpet_freq(period: u64) -> Option<u64> {
    if period == 0 || period > u64::from(HPET_MAX_PERIOD) {
        return None;
    }
    Some(1_000_000_000_000_000u64 / period)
}

/// `acpihpet_delay(usecs)`: busy-waits `usecs` microseconds on the main counter.
pub fn acpihpet_delay(usecs: i32) {
    let Some(sc) = acpihpet_softc(&HPET_TIMECOUNTER) else {
        return;
    };
    let Some((iot, ioh)) = regs(sc) else {
        return;
    };
    let mut count: u64 = 0;

    let mut val2 = bus_space_read_4(iot, ioh, HPET_MAIN_COUNTER);
    let cycles = u64::from(usecs.max(0) as u32) * HPET_TIMECOUNTER.tc_frequency.get() / 1_000_000;
    while count < cycles {
        core::hint::spin_loop(); // CPU_BUSY_CYCLE()
        let val1 = val2;
        val2 = bus_space_read_4(iot, ioh, HPET_MAIN_COUNTER);
        count += u64::from(val2.wrapping_sub(val1));
    }
}

/// `tc->tc_priv`: acpihpet0's softc, once it has attached.
fn acpihpet_softc(tc: &Timecounter) -> Option<&'static AcpihpetSoftc> {
    // SAFETY: `tc_priv` is null or the `&'static AcpihpetSoftc` `acpihpet_attach` stored.
    unsafe { tc.tc_priv.get().cast::<AcpihpetSoftc>().as_ref() }
}

/// `acpihpet_gettime(tc)`: the low 32 bits of the main counter.
pub fn acpihpet_gettime(tc: &Timecounter) -> u32 {
    acpihpet_softc(tc).and_then(regs).map_or(0, |(iot, ioh)| {
        bus_space_read_4(iot, ioh, HPET_MAIN_COUNTER)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn period_to_frequency() {
        // QEMU's HPET: 10 ns.
        assert_eq!(acpihpet_freq(10_000_000), Some(100_000_000));
        // A 14.318 MHz HPET (69.841279 ns).
        assert_eq!(acpihpet_freq(69_841_279), Some(14_318_179)); // truncated, as in C
        assert_eq!(acpihpet_freq(u64::from(HPET_MAX_PERIOD)), Some(10_000_000));
        assert_eq!(acpihpet_freq(u64::from(HPET_MAX_PERIOD) + 1), None);
        assert_eq!(acpihpet_freq(0), None);
    }

    #[test]
    fn saved_timer_offsets() {
        assert_eq!(HPET_TIMER_REGS[1].0, HPET_TIMER0_CONFIG + 0x20);
        assert_eq!(HPET_TIMER_REGS[2].2, HPET_TIMER0_COMPARE + 0x40);
        assert_eq!(acpihpet_gettime(&HPET_TIMECOUNTER), 0);
    }
}
