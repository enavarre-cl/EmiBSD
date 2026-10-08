/* $OpenBSD: acpidev.h,v 1.45 2024/08/06 17:38:56 kettenis Exp $ */
/* <LICENSES> */
/*
 * Copyright (c) 2005 Marco Peereboom <marco@openbsd.org>
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
//! `<dev/acpi/acpidev.h>`: structures and constants of the ACPI device drivers (battery,
//! AC adapter, HPET, processor performance states, dock, embedded controller, smart battery).
//!
//! Upstream: sys/dev/acpi/acpidev.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - The plain data structures are here (`AcpibatBix`, `AcpibatBst`, `AcpibatBmd`,
//!   `AcpicpuPss`, `AcpiGrd`, `AcpicpuPct`, `AcpisbsBattery`); `char` members are `u8`.
//!   `AcpiGrd` is `__packed` in C and `#[repr(C, packed)]`; the others are not and are plain
//!   `#[repr(C)]` (the C sizes are asserted at compile time).
//! - The driver softcs wait for their drivers' ports, because they hold unported types
//!   (`struct aml_node`, `struct acpi_softc`, `struct ksensor`, `struct ksensordev`,
//!   `struct sensor_task`, `struct timeval` handles, bus_space tags): `acpiac_softc`
//!   (acpiac.c), `acpibat_softc` (acpibat.c), `acpidock_softc` and the `aml_nodelisth` list
//!   head (acpidock.c), `acpiec_event` and `acpiec_softc` (acpiec.c), `acpisbs_softc`
//!   (acpisbs.c). `ACPIDOCK_STATUS_*`, `ACPIDOCK_EVENT_*` and `ACPIEC_MAX_EVENTS` are here.
//! - The prototypes `acpicpu_fetch_pss`, `acpicpu_set_notify` (acpicpu.c) and
//!   `acpibtn_disable_psw`, `acpibtn_enable_psw`, `acpibtn_numopenlids` (acpibtn.c) are
//!   declarations of functions in those files; the ports of those files define them.
//! - `DEVNAME(s)` (`(s)->sc_dev.dv_xname`) is a macro over a softc; it waits for the softcs.
//! - `SMBUS_DATA_SIZE` belongs to `smbus.h` (not ported); it is defined here, valued as there
//!   (32), until that header is ported and this one imports it.
//! - Constants are as wide as what they describe: the `_BIF`/`_BST`/... dwords are `u32`, the
//!   HPET register offsets `usize` (`bus_size_t`), `ACPISBS_UNITS_*` `i32` (`int units`).

use core::mem::size_of;

use super::acpireg::AcpiGas;

/// `ACPIDEV_NOPOLL`.
pub const ACPIDEV_NOPOLL: i32 = 0x0000;
/// `ACPIDEV_POLL`.
pub const ACPIDEV_POLL: i32 = 0x0001;
/// `ACPIDEV_WAKEUP`.
pub const ACPIDEV_WAKEUP: i32 = 0x0002;
/// `BIX_POWER_MW`.
pub const BIX_POWER_MW: u32 = 0x00;
/// `BIX_POWER_MA`.
pub const BIX_POWER_MA: u32 = 0x01;
/// `BIX_UNKNOWN`.
pub const BIX_UNKNOWN: u32 = 0xffffffff;
/// `BIX_TECH_PRIMARY`.
pub const BIX_TECH_PRIMARY: u32 = 0x00;
/// `BIX_TECH_SECONDARY`.
pub const BIX_TECH_SECONDARY: u32 = 0x01;
/// `CMB_OSC_UUID`.
pub const CMB_OSC_UUID: &str = "f18fc78b-0f15-4978-b793-53f833a1d35b";
/// `CMB_OSC_GRANULARITY`.
pub const CMB_OSC_GRANULARITY: u32 = 0x01;
/// `CMB_OSC_WAKE_ON_LOW`.
pub const CMB_OSC_WAKE_ON_LOW: u32 = 0x02;
/// `BST_DISCHARGE`.
pub const BST_DISCHARGE: u32 = 0x01;
/// `BST_CHARGE`.
pub const BST_CHARGE: u32 = 0x02;
/// `BST_CRITICAL`.
pub const BST_CRITICAL: u32 = 0x04;
/// `BST_UNKNOWN`.
pub const BST_UNKNOWN: u32 = 0xffffffff;
/// `BTP_CLEAR_TRIP_POINT`.
pub const BTP_CLEAR_TRIP_POINT: u32 = 0x00;
/// `BTM_CURRENT_RATE`.
pub const BTM_CURRENT_RATE: u32 = 0x00;
/// `BTM_RATE_TOO_LARGE`.
pub const BTM_RATE_TOO_LARGE: u32 = 0x00;
/// `BTM_CRITICAL`.
pub const BTM_CRITICAL: u32 = 0x00;
/// `BTM_UNKNOWN`.
pub const BTM_UNKNOWN: u32 = 0xffffffff;
/// `BMD_AML_CALIBRATE_CYCLE`.
pub const BMD_AML_CALIBRATE_CYCLE: u32 = 0x01;
/// `BMD_CHARGING_DISABLED`.
pub const BMD_CHARGING_DISABLED: u32 = 0x02;
/// `BMD_DISCHARGE_WHILE_AC`.
pub const BMD_DISCHARGE_WHILE_AC: u32 = 0x04;
/// `BMD_RECALIBRATE_BAT`.
pub const BMD_RECALIBRATE_BAT: u32 = 0x08;
/// `BMD_GOTO_STANDBY_SPEED`.
pub const BMD_GOTO_STANDBY_SPEED: u32 = 0x10;
/// `BMD_CB_AML_CALIBRATION`.
pub const BMD_CB_AML_CALIBRATION: u32 = 0x01;
/// `BMD_CB_DISABLE_CHARGER`.
pub const BMD_CB_DISABLE_CHARGER: u32 = 0x02;
/// `BMD_CB_DISCH_WHILE_AC`.
pub const BMD_CB_DISCH_WHILE_AC: u32 = 0x04;
/// `BMD_CB_AFFECT_ALL_BATT`.
pub const BMD_CB_AFFECT_ALL_BATT: u32 = 0x08;
/// `BMD_CB_FULL_CHRG_FIRST`.
pub const BMD_CB_FULL_CHRG_FIRST: u32 = 0x10;
/// `BMD_ONLY_CALIB_IF_ST3`: only recal when status bit 3 set.
pub const BMD_ONLY_CALIB_IF_ST3: u32 = 0x00;
/// `BMD_UNKNOWN`.
pub const BMD_UNKNOWN: u32 = 0xffffffff;
/// `BMC_AML_CALIBRATE`.
pub const BMC_AML_CALIBRATE: u32 = 0x01;
/// `BMC_DISABLE_CHARGING`.
pub const BMC_DISABLE_CHARGING: u32 = 0x02;
/// `BMC_ALLOW_AC_DISCHARGE`.
pub const BMC_ALLOW_AC_DISCHARGE: u32 = 0x04;
/// `PSR_OFFLINE`.
pub const PSR_OFFLINE: u32 = 0x00;
/// `PSR_ONLINE`.
pub const PSR_ONLINE: u32 = 0x01;
/// `HPET_REG_SIZE`.
pub const HPET_REG_SIZE: usize = 1024;
/// `HPET_CAPABILITIES`.
pub const HPET_CAPABILITIES: usize = 0x000;
/// `HPET_CONFIGURATION`.
pub const HPET_CONFIGURATION: usize = 0x010;
/// `HPET_INTERRUPT_STATUS`.
pub const HPET_INTERRUPT_STATUS: usize = 0x020;
/// `HPET_MAIN_COUNTER`.
pub const HPET_MAIN_COUNTER: usize = 0x0F0;
/// `HPET_TIMER0_CONFIG`.
pub const HPET_TIMER0_CONFIG: usize = 0x100;
/// `HPET_TIMER0_COMPARE`.
pub const HPET_TIMER0_COMPARE: usize = 0x108;
/// `HPET_TIMER0_INTERRUPT`.
pub const HPET_TIMER0_INTERRUPT: usize = 0x110;
/// `HPET_TIMER1_CONFIG`.
pub const HPET_TIMER1_CONFIG: usize = 0x20 + HPET_TIMER0_CONFIG;
/// `HPET_TIMER1_COMPARE`.
pub const HPET_TIMER1_COMPARE: usize = 0x20 + HPET_TIMER0_COMPARE;
/// `HPET_TIMER1_INTERRUPT`.
pub const HPET_TIMER1_INTERRUPT: usize = 0x20 + HPET_TIMER0_INTERRUPT;
/// `HPET_TIMER2_CONFIG`.
pub const HPET_TIMER2_CONFIG: usize = (0x20 * 2) + HPET_TIMER0_CONFIG;
/// `HPET_TIMER2_COMPARE`.
pub const HPET_TIMER2_COMPARE: usize = (0x20 * 2) + HPET_TIMER0_COMPARE;
/// `HPET_TIMER2_INTERRUPT`.
pub const HPET_TIMER2_INTERRUPT: usize = (0x20 * 2) + HPET_TIMER0_INTERRUPT;
/// `HPET_MAX_PERIOD`.
pub const HPET_MAX_PERIOD: u32 = 0x5F5E100;
/// `STA_PRESENT`.
pub const STA_PRESENT: u32 = 1 << 0;
/// `STA_ENABLED`.
pub const STA_ENABLED: u32 = 1 << 1;
/// `STA_SHOW_UI`.
pub const STA_SHOW_UI: u32 = 1 << 2;
/// `STA_DEV_OK`.
pub const STA_DEV_OK: u32 = 1 << 3;
/// `STA_BATTERY`.
pub const STA_BATTERY: u32 = 1 << 4;
/// `ACPIDOCK_STATUS_UNKNOWN`.
pub const ACPIDOCK_STATUS_UNKNOWN: i32 = -1;
/// `ACPIDOCK_STATUS_UNDOCKED`.
pub const ACPIDOCK_STATUS_UNDOCKED: i32 = 0;
/// `ACPIDOCK_STATUS_DOCKED`.
pub const ACPIDOCK_STATUS_DOCKED: i32 = 1;
/// `ACPIDOCK_EVENT_INSERT`.
pub const ACPIDOCK_EVENT_INSERT: i32 = 0;
/// `ACPIDOCK_EVENT_DEVCHECK`.
pub const ACPIDOCK_EVENT_DEVCHECK: i32 = 1;
/// `ACPIDOCK_EVENT_EJECT`.
pub const ACPIDOCK_EVENT_EJECT: i32 = 3;
/// `ACPIEC_MAX_EVENTS`.
pub const ACPIEC_MAX_EVENTS: usize = 256;
/// `ACPISBS_UNITS_MW`.
pub const ACPISBS_UNITS_MW: i32 = 0;
/// `ACPISBS_UNITS_MA`.
pub const ACPISBS_UNITS_MA: i32 = 1;
/// `ACPISBS_VALUE_UNKNOWN`.
pub const ACPISBS_VALUE_UNKNOWN: u16 = 65535;
/// `SMBUS_DATA_SIZE` (`smbus.h`): the size of a Smart Battery string; see the Deviations.
pub const SMBUS_DATA_SIZE: usize = 32;

/// `struct acpibat_bix`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct AcpibatBix {
    /// `bix_revision`.
    pub bix_revision: u8,
    /// `bix_power_unit`.
    pub bix_power_unit: u32,
    /// `bix_capacity`.
    pub bix_capacity: u32,
    /// `bix_last_capacity`.
    pub bix_last_capacity: u32,
    /// `bix_technology`.
    pub bix_technology: u32,
    /// `bix_voltage`.
    pub bix_voltage: u32,
    /// `bix_warning`.
    pub bix_warning: u32,
    /// `bix_low`.
    pub bix_low: u32,
    /// `bix_cycle_count`.
    pub bix_cycle_count: u32,
    /// `bix_accuracy`.
    pub bix_accuracy: u32,
    /// `bix_max_sample`.
    pub bix_max_sample: u32,
    /// `bix_min_sample`.
    pub bix_min_sample: u32,
    /// `bix_max_avg`.
    pub bix_max_avg: u32,
    /// `bix_min_avg`.
    pub bix_min_avg: u32,
    /// `bix_cap_granu1`.
    pub bix_cap_granu1: u32,
    /// `bix_cap_granu2`.
    pub bix_cap_granu2: u32,
    /// `bix_model`.
    pub bix_model: [u8; 20],
    /// `bix_serial`.
    pub bix_serial: [u8; 20],
    /// `bix_type`.
    pub bix_type: [u8; 20],
    /// `bix_oem`.
    pub bix_oem: [u8; 20],
}

/// `struct acpibat_bst`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct AcpibatBst {
    /// `bst_state`.
    pub bst_state: u32,
    /// `bst_rate`.
    pub bst_rate: u32,
    /// `bst_capacity`.
    pub bst_capacity: u32,
    /// `bst_voltage`.
    pub bst_voltage: u32,
}

/// `struct acpibat_bmd`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct AcpibatBmd {
    /// `bmd_status`.
    pub bmd_status: u32,
    /// `bmd_capability`.
    pub bmd_capability: u32,
    /// `bmd_recalibrate_count`.
    pub bmd_recalibrate_count: u32,
    /// `bmd_quick_recalibrate_time`.
    pub bmd_quick_recalibrate_time: u32,
    /// `bmd_slow_recalibrate_time`.
    pub bmd_slow_recalibrate_time: u32,
}

/// `struct acpicpu_pss`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct AcpicpuPss {
    /// `pss_core_freq`.
    pub pss_core_freq: u32,
    /// `pss_power`.
    pub pss_power: u32,
    /// `pss_trans_latency`.
    pub pss_trans_latency: u32,
    /// `pss_bus_latency`.
    pub pss_bus_latency: u32,
    /// `pss_ctrl`.
    pub pss_ctrl: u32,
    /// `pss_status`.
    pub pss_status: u32,
}

/// `struct acpi_grd`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct AcpiGrd {
    /// `grd_descriptor`.
    pub grd_descriptor: u8,
    /// `grd_length`.
    pub grd_length: u16,
    /// `grd_gas`.
    pub grd_gas: AcpiGas,
}

/// `struct acpicpu_pct`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct AcpicpuPct {
    /// `pct_ctrl`.
    pub pct_ctrl: AcpiGrd,
    /// `pct_status`.
    pub pct_status: AcpiGrd,
}

/// `struct acpisbs_battery`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct AcpisbsBattery {
    /// `mode`: bit flags.
    pub mode: u16,
    /// `units`.
    pub units: i32,
    /// `at_rate`: mAh or mWh.
    pub at_rate: u16,
    /// `temperature`: 0.1 degK.
    pub temperature: u16,
    /// `voltage`: mV.
    pub voltage: u16,
    /// `current`: mA.
    pub current: u16,
    /// `avg_current`: mA.
    pub avg_current: u16,
    /// `rel_charge`: percent of last_capacity.
    pub rel_charge: u16,
    /// `abs_charge`: percent of design_capacity.
    pub abs_charge: u16,
    /// `capacity`: mAh.
    pub capacity: u16,
    /// `full_capacity`: mAh, when fully charged.
    pub full_capacity: u16,
    /// `run_time`: minutes.
    pub run_time: u16,
    /// `avg_empty_time`: minutes.
    pub avg_empty_time: u16,
    /// `avg_full_time`: minutes until full.
    pub avg_full_time: u16,
    /// `charge_current`: mA.
    pub charge_current: u16,
    /// `charge_voltage`: mV.
    pub charge_voltage: u16,
    /// `status`: bit flags.
    pub status: u16,
    /// `cycle_count`: cycles.
    pub cycle_count: u16,
    /// `design_capacity`: mAh.
    pub design_capacity: u16,
    /// `design_voltage`: mV.
    pub design_voltage: u16,
    /// `spec`: formatted.
    pub spec: u16,
    /// `manufacture_date`: formatted.
    pub manufacture_date: u16,
    /// `serial`: number.
    pub serial: u16,
    /// `manufacturer`.
    pub manufacturer: [u8; SMBUS_DATA_SIZE],
    /// `device_name`.
    pub device_name: [u8; SMBUS_DATA_SIZE],
    /// `device_chemistry`.
    pub device_chemistry: [u8; SMBUS_DATA_SIZE],
    /// `oem_data`.
    pub oem_data: [u8; SMBUS_DATA_SIZE],
}

const _: () = {
    assert!(size_of::<AcpibatBix>() == 144);
    assert!(size_of::<AcpibatBst>() == 16);
    assert!(size_of::<AcpibatBmd>() == 20);
    assert!(size_of::<AcpicpuPss>() == 24);
    assert!(size_of::<AcpiGrd>() == 15);
    assert!(size_of::<AcpicpuPct>() == 30);
    assert!(size_of::<AcpisbsBattery>() == 180);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Tests for `acpidev`.

    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/acpi/acpidev.h");
        // Not compared: `CMB_OSC_UUID` (a string) and `DEVNAME(s)` (a macro over a softc).
        crate::reftest::assert_defines!(defs;
        ACPIDEV_NOPOLL, ACPIDEV_POLL, ACPIDEV_WAKEUP, BIX_POWER_MW, BIX_POWER_MA, BIX_UNKNOWN,
        BIX_TECH_PRIMARY, BIX_TECH_SECONDARY, CMB_OSC_GRANULARITY, CMB_OSC_WAKE_ON_LOW,
        BST_DISCHARGE, BST_CHARGE, BST_CRITICAL, BST_UNKNOWN, BTP_CLEAR_TRIP_POINT,
        BTM_CURRENT_RATE, BTM_RATE_TOO_LARGE, BTM_CRITICAL, BTM_UNKNOWN, BMD_AML_CALIBRATE_CYCLE,
        BMD_CHARGING_DISABLED, BMD_DISCHARGE_WHILE_AC, BMD_RECALIBRATE_BAT, BMD_GOTO_STANDBY_SPEED,
        BMD_CB_AML_CALIBRATION, BMD_CB_DISABLE_CHARGER, BMD_CB_DISCH_WHILE_AC,
        BMD_CB_AFFECT_ALL_BATT, BMD_CB_FULL_CHRG_FIRST, BMD_ONLY_CALIB_IF_ST3, BMD_UNKNOWN,
        BMC_AML_CALIBRATE, BMC_DISABLE_CHARGING, BMC_ALLOW_AC_DISCHARGE, PSR_OFFLINE, PSR_ONLINE,
        HPET_REG_SIZE, HPET_CAPABILITIES, HPET_CONFIGURATION, HPET_INTERRUPT_STATUS,
        HPET_MAIN_COUNTER, HPET_TIMER0_CONFIG, HPET_TIMER0_COMPARE, HPET_TIMER0_INTERRUPT,
        HPET_TIMER1_CONFIG, HPET_TIMER1_COMPARE, HPET_TIMER1_INTERRUPT, HPET_TIMER2_CONFIG,
        HPET_TIMER2_COMPARE, HPET_TIMER2_INTERRUPT, HPET_MAX_PERIOD, STA_PRESENT, STA_ENABLED,
        STA_SHOW_UI, STA_DEV_OK, STA_BATTERY, ACPIDOCK_STATUS_UNKNOWN, ACPIDOCK_STATUS_UNDOCKED,
        ACPIDOCK_STATUS_DOCKED, ACPIDOCK_EVENT_INSERT, ACPIDOCK_EVENT_DEVCHECK,
        ACPIDOCK_EVENT_EJECT, ACPIEC_MAX_EVENTS, ACPISBS_UNITS_MW, ACPISBS_UNITS_MA,
        ACPISBS_VALUE_UNKNOWN,
        );
    }

    #[test]
    fn structure_sizes_are_the_c_sizes() {
        assert_eq!(size_of::<AcpibatBix>(), 144);
        assert_eq!(size_of::<AcpibatBst>(), 16);
        assert_eq!(size_of::<AcpibatBmd>(), 20);
        assert_eq!(size_of::<AcpicpuPss>(), 24);
        assert_eq!(size_of::<AcpiGrd>(), 15);
        assert_eq!(size_of::<AcpicpuPct>(), 30);
        assert_eq!(size_of::<AcpisbsBattery>(), 180);
    }

    #[test]
    fn field_offsets() {
        use core::mem::offset_of;
        assert_eq!(offset_of!(AcpibatBix, bix_power_unit), 4);
        assert_eq!(offset_of!(AcpibatBix, bix_model), 64);
        assert_eq!(offset_of!(AcpisbsBattery, units), 4);
        assert_eq!(offset_of!(AcpisbsBattery, manufacturer), 50);
        assert_eq!(offset_of!(AcpiGrd, grd_gas), 3);
    }
}
/* </TESTS> */
