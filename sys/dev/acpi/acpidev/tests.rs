//! Tests for `acpidev`.

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
