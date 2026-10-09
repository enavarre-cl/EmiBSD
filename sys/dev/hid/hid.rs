/* <LICENSES> */
/*
 * Copyright (c) 1998 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Lennart Augustsson (lennart@augustsson.net) at
 * Carlstedt Research & Technology.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 *
 * THIS SOFTWARE IS PROVIDED BY THE NETBSD FOUNDATION, INC. AND CONTRIBUTORS
 * ``AS IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
 * TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
 * PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL THE FOUNDATION OR CONTRIBUTORS
 * BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
 * CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
 * SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
 * INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
 * CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
 * ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
 * POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! `hid(4)`: the HID report descriptor parser shared by the USB and I2C HID drivers
//! (`uhidev(4)`, `ihidev(4)`) and by the drivers on top of them (`ukbd(4)`, `hidkbd`,
//! `hidms`, `hidmt`, `hidcc`), with `<dev/hid/hid.h>`: the usage pages and usages, the item
//! structures and the parser's entry points.
//!
//! Upstream: sys/dev/hid/hid.c @ 3ce1f3f79392, sys/dev/hid/hid.h @ 3ce1f3f79392
//!
//! The parser walks a report descriptor, a stream of short and long items. Global items
//! (usage page, logical and physical ranges, report size, report count, report ID) and local
//! items (usages, usage ranges, designators, strings) accumulate into a [`HidItem`]; a Main
//! item (Input, Output, Feature, Collection, End Collection) hands the accumulated item out
//! through [`hid_get_item`], once per variable field (a `Report Count` of 8 on a variable
//! item yields eight items of count 1, each with its own usage) or once for an array. The
//! bit position of every field in its report is tracked per report ID, so that
//! [`hid_locate`] can answer where a usage lives and [`hid_get_data`] can extract it from a
//! report.
//!
//! ## Deviations
//! - `struct hid_data` is [`HidData`], borrowing the descriptor as a slice (`start`, `end`
//!   and `p` become the slice and an index into it). [`hid_start_parse`] returns a `Box`
//!   (the C `malloc`s it with `M_TEMP`, `M_WAITOK|M_ZERO`) and [`hid_end_parse`] drops it, so
//!   the NULL checks of `hid_end_parse` and `hid_get_item` have no counterpart (a `Box` is
//!   never null; `hid_get_collection_data` returns an `Option`).
//! - The descriptor is a `&[u8]` (the C's `const void *` and `int len`); the report buffer of
//!   [`hid_get_data`] likewise.
//! - The cursor `c` of `hid_get_item` (always `&s->cur[s->pushlevel]`) is the index
//!   `s.pushlevel`; `goto top` is a loop label.
//! - `hid_find_report` and `hid_get_id_of_collection` return `Option<u32>` (the report ID)
//!   where the C returns the report ID or `-1`; `hid_find_report` takes `usages` as a slice
//!   (`n_usages` is its length) and `coll_usages` as an `Option` of a slice that is
//!   0-terminated as in the C (a slice that ends before a 0 ends the list just the same).
//!   Its match bitmap is a `u64`, so that 32 usages, which the C documents as valid, do not
//!   shift an `int` by 32.
//! - `struct hid_item`'s `next` member, which no code in the tree uses, is not carried.
//! - `hid_get_data_sub` (non-static in the C, not declared in the header) is public and takes
//!   `is_signed` as a `bool`.
//! - The `USBHID_DEBUG` `DPRINTF`s are not carried; `DIAGNOSTIC`'s `lo != 0` warning in
//!   `hid_report_size` is behind the `diagnostic` feature.
//! - `hid_report_size` and the position arithmetic wrap in 32 bits, as the C's unsigned
//!   arithmetic does, instead of panicking on a hostile descriptor.

use alloc::boxed::Box;

/// `MAXUSAGE`: the most usages (or usage ranges) one Main item can collect.
const MAXUSAGE: usize = 64;
/// `MAXPUSH`: the depth of the Push/Pop item stack.
const MAXPUSH: usize = 4;
/// `MAXID`: the most report IDs whose positions are tracked, ID 0 included.
const MAXID: usize = 16;
/// `MAXLOCCNT`: the most items one variable Main item expands to.
const MAXLOCCNT: u32 = 2048;

/// `HUP_UNDEFINED`.
pub const HUP_UNDEFINED: u32 = 0x0000;
/// `HUP_GENERIC_DESKTOP`.
pub const HUP_GENERIC_DESKTOP: u32 = 0x0001;
/// `HUP_SIMULATION`.
pub const HUP_SIMULATION: u32 = 0x0002;
/// `HUP_VR_CONTROLS`.
pub const HUP_VR_CONTROLS: u32 = 0x0003;
/// `HUP_SPORTS_CONTROLS`.
pub const HUP_SPORTS_CONTROLS: u32 = 0x0004;
/// `HUP_GAMING_CONTROLS`.
pub const HUP_GAMING_CONTROLS: u32 = 0x0005;
/// `HUP_KEYBOARD`.
pub const HUP_KEYBOARD: u32 = 0x0007;
/// `HUP_LED`.
pub const HUP_LED: u32 = 0x0008;
/// `HUP_BUTTON`.
pub const HUP_BUTTON: u32 = 0x0009;
/// `HUP_ORDINALS`.
pub const HUP_ORDINALS: u32 = 0x000a;
/// `HUP_TELEPHONY`.
pub const HUP_TELEPHONY: u32 = 0x000b;
/// `HUP_CONSUMER`.
pub const HUP_CONSUMER: u32 = 0x000c;
/// `HUP_DIGITIZERS`.
pub const HUP_DIGITIZERS: u32 = 0x000d;
/// `HUP_PHYSICAL_IFACE`.
pub const HUP_PHYSICAL_IFACE: u32 = 0x000e;
/// `HUP_UNICODE`.
pub const HUP_UNICODE: u32 = 0x0010;
/// `HUP_ALPHANUM_DISPLAY`.
pub const HUP_ALPHANUM_DISPLAY: u32 = 0x0014;
/// `HUP_MONITOR`.
pub const HUP_MONITOR: u32 = 0x0080;
/// `HUP_MONITOR_ENUM_VAL`.
pub const HUP_MONITOR_ENUM_VAL: u32 = 0x0081;
/// `HUP_VESA_VC`.
pub const HUP_VESA_VC: u32 = 0x0082;
/// `HUP_VESA_CMD`.
pub const HUP_VESA_CMD: u32 = 0x0083;
/// `HUP_POWER`.
pub const HUP_POWER: u32 = 0x0084;
/// `HUP_BATTERY`.
pub const HUP_BATTERY: u32 = 0x0085;
/// `HUP_BARCODE_SCANNER`.
pub const HUP_BARCODE_SCANNER: u32 = 0x008b;
/// `HUP_SCALE`.
pub const HUP_SCALE: u32 = 0x008c;
/// `HUP_CAMERA_CONTROL`.
pub const HUP_CAMERA_CONTROL: u32 = 0x0090;
/// `HUP_ARCADE`.
pub const HUP_ARCADE: u32 = 0x0091;
/// `HUP_VENDOR`.
pub const HUP_VENDOR: u32 = 0x00ff;
/// `HUP_FIDO`.
pub const HUP_FIDO: u32 = 0xf1d0;
/// `HUP_MICROSOFT`.
pub const HUP_MICROSOFT: u32 = 0xff00;
/// `HUP_APPLE`.
pub const HUP_APPLE: u32 = 0x00ff;
/// `HUP_WACOM`.
pub const HUP_WACOM: u32 = 0xff00;
/// `HUP_INAME`.
pub const HUP_INAME: u32 = 0x0001;
/// `HUP_PRESENT_STATUS`.
pub const HUP_PRESENT_STATUS: u32 = 0x0002;
/// `HUP_CHANGED_STATUS`.
pub const HUP_CHANGED_STATUS: u32 = 0x0003;
/// `HUP_UPS`.
pub const HUP_UPS: u32 = 0x0004;
/// `HUP_POWER_SUPPLY`.
pub const HUP_POWER_SUPPLY: u32 = 0x0005;
/// `HUP_BATTERY_SYSTEM`.
pub const HUP_BATTERY_SYSTEM: u32 = 0x0010;
/// `HUP_BATTERY_SYSTEM_ID`.
pub const HUP_BATTERY_SYSTEM_ID: u32 = 0x0011;
/// `HUP_PD_BATTERY`.
pub const HUP_PD_BATTERY: u32 = 0x0012;
/// `HUP_BATTERY_ID`.
pub const HUP_BATTERY_ID: u32 = 0x0013;
/// `HUP_CHARGER`.
pub const HUP_CHARGER: u32 = 0x0014;
/// `HUP_CHARGER_ID`.
pub const HUP_CHARGER_ID: u32 = 0x0015;
/// `HUP_POWER_CONVERTER`.
pub const HUP_POWER_CONVERTER: u32 = 0x0016;
/// `HUP_POWER_CONVERTER_ID`.
pub const HUP_POWER_CONVERTER_ID: u32 = 0x0017;
/// `HUP_OUTLET_SYSTEM`.
pub const HUP_OUTLET_SYSTEM: u32 = 0x0018;
/// `HUP_OUTLET_SYSTEM_ID`.
pub const HUP_OUTLET_SYSTEM_ID: u32 = 0x0019;
/// `HUP_INPUT`.
pub const HUP_INPUT: u32 = 0x001a;
/// `HUP_INPUT_ID`.
pub const HUP_INPUT_ID: u32 = 0x001b;
/// `HUP_OUTPUT`.
pub const HUP_OUTPUT: u32 = 0x001c;
/// `HUP_OUTPUT_ID`.
pub const HUP_OUTPUT_ID: u32 = 0x001d;
/// `HUP_FLOW`.
pub const HUP_FLOW: u32 = 0x001e;
/// `HUP_FLOW_ID`.
pub const HUP_FLOW_ID: u32 = 0x001f;
/// `HUP_OUTLET`.
pub const HUP_OUTLET: u32 = 0x0020;
/// `HUP_OUTLET_ID`.
pub const HUP_OUTLET_ID: u32 = 0x0021;
/// `HUP_GANG`.
pub const HUP_GANG: u32 = 0x0022;
/// `HUP_GANG_ID`.
pub const HUP_GANG_ID: u32 = 0x0023;
/// `HUP_POWER_SUMMARY`.
pub const HUP_POWER_SUMMARY: u32 = 0x0024;
/// `HUP_POWER_SUMMARY_ID`.
pub const HUP_POWER_SUMMARY_ID: u32 = 0x0025;
/// `HUP_VOLTAGE`.
pub const HUP_VOLTAGE: u32 = 0x0030;
/// `HUP_CURRENT`.
pub const HUP_CURRENT: u32 = 0x0031;
/// `HUP_FREQUENCY`.
pub const HUP_FREQUENCY: u32 = 0x0032;
/// `HUP_APPARENT_POWER`.
pub const HUP_APPARENT_POWER: u32 = 0x0033;
/// `HUP_ACTIVE_POWER`.
pub const HUP_ACTIVE_POWER: u32 = 0x0034;
/// `HUP_PERCENT_LOAD`.
pub const HUP_PERCENT_LOAD: u32 = 0x0035;
/// `HUP_TEMPERATURE`.
pub const HUP_TEMPERATURE: u32 = 0x0036;
/// `HUP_HUMIDITY`.
pub const HUP_HUMIDITY: u32 = 0x0037;
/// `HUP_BADCOUNT`.
pub const HUP_BADCOUNT: u32 = 0x0038;
/// `HUP_CONFIG_VOLTAGE`.
pub const HUP_CONFIG_VOLTAGE: u32 = 0x0040;
/// `HUP_CONFIG_CURRENT`.
pub const HUP_CONFIG_CURRENT: u32 = 0x0041;
/// `HUP_CONFIG_FREQUENCY`.
pub const HUP_CONFIG_FREQUENCY: u32 = 0x0042;
/// `HUP_CONFIG_APP_POWER`.
pub const HUP_CONFIG_APP_POWER: u32 = 0x0043;
/// `HUP_CONFIG_ACT_POWER`.
pub const HUP_CONFIG_ACT_POWER: u32 = 0x0044;
/// `HUP_CONFIG_PERCENT_LOAD`.
pub const HUP_CONFIG_PERCENT_LOAD: u32 = 0x0045;
/// `HUP_CONFIG_TEMPERATURE`.
pub const HUP_CONFIG_TEMPERATURE: u32 = 0x0046;
/// `HUP_CONFIG_HUMIDITY`.
pub const HUP_CONFIG_HUMIDITY: u32 = 0x0047;
/// `HUP_SWITCHON_CONTROL`.
pub const HUP_SWITCHON_CONTROL: u32 = 0x0050;
/// `HUP_SWITCHOFF_CONTROL`.
pub const HUP_SWITCHOFF_CONTROL: u32 = 0x0051;
/// `HUP_TOGGLE_CONTROL`.
pub const HUP_TOGGLE_CONTROL: u32 = 0x0052;
/// `HUP_LOW_VOLT_TRANSF`.
pub const HUP_LOW_VOLT_TRANSF: u32 = 0x0053;
/// `HUP_HIGH_VOLT_TRANSF`.
pub const HUP_HIGH_VOLT_TRANSF: u32 = 0x0054;
/// `HUP_DELAYBEFORE_REBOOT`.
pub const HUP_DELAYBEFORE_REBOOT: u32 = 0x0055;
/// `HUP_DELAYBEFORE_STARTUP`.
pub const HUP_DELAYBEFORE_STARTUP: u32 = 0x0056;
/// `HUP_DELAYBEFORE_SHUTDWN`.
pub const HUP_DELAYBEFORE_SHUTDWN: u32 = 0x0057;
/// `HUP_TEST`.
pub const HUP_TEST: u32 = 0x0058;
/// `HUP_MODULE_RESET`.
pub const HUP_MODULE_RESET: u32 = 0x0059;
/// `HUP_AUDIBLE_ALRM_CTL`.
pub const HUP_AUDIBLE_ALRM_CTL: u32 = 0x005a;
/// `HUP_PRESENT`.
pub const HUP_PRESENT: u32 = 0x0060;
/// `HUP_GOOD`.
pub const HUP_GOOD: u32 = 0x0061;
/// `HUP_INTERNAL_FAILURE`.
pub const HUP_INTERNAL_FAILURE: u32 = 0x0062;
/// `HUP_PD_VOLT_OUTOF_RANGE`.
pub const HUP_PD_VOLT_OUTOF_RANGE: u32 = 0x0063;
/// `HUP_FREQ_OUTOFRANGE`.
pub const HUP_FREQ_OUTOFRANGE: u32 = 0x0064;
/// `HUP_OVERLOAD`.
pub const HUP_OVERLOAD: u32 = 0x0065;
/// `HUP_OVERCHARGED`.
pub const HUP_OVERCHARGED: u32 = 0x0066;
/// `HUP_OVERTEMPERATURE`.
pub const HUP_OVERTEMPERATURE: u32 = 0x0067;
/// `HUP_SHUTDOWN_REQUESTED`.
pub const HUP_SHUTDOWN_REQUESTED: u32 = 0x0068;
/// `HUP_SHUTDOWN_IMMINENT`.
pub const HUP_SHUTDOWN_IMMINENT: u32 = 0x0069;
/// `HUP_SWITCH_ON_OFF`.
pub const HUP_SWITCH_ON_OFF: u32 = 0x006b;
/// `HUP_SWITCHABLE`.
pub const HUP_SWITCHABLE: u32 = 0x006c;
/// `HUP_USED`.
pub const HUP_USED: u32 = 0x006d;
/// `HUP_BOOST`.
pub const HUP_BOOST: u32 = 0x006e;
/// `HUP_BUCK`.
pub const HUP_BUCK: u32 = 0x006f;
/// `HUP_INITIALIZED`.
pub const HUP_INITIALIZED: u32 = 0x0070;
/// `HUP_TESTED`.
pub const HUP_TESTED: u32 = 0x0071;
/// `HUP_AWAITING_POWER`.
pub const HUP_AWAITING_POWER: u32 = 0x0072;
/// `HUP_COMMUNICATION_LOST`.
pub const HUP_COMMUNICATION_LOST: u32 = 0x0073;
/// `HUP_IMANUFACTURER`.
pub const HUP_IMANUFACTURER: u32 = 0x00fd;
/// `HUP_IPRODUCT`.
pub const HUP_IPRODUCT: u32 = 0x00fe;
/// `HUP_ISERIALNUMBER`.
pub const HUP_ISERIALNUMBER: u32 = 0x00ff;
/// `HUB_SMB_BATTERY_MODE`.
pub const HUB_SMB_BATTERY_MODE: u32 = 0x0001;
/// `HUB_SMB_BATTERY_STATUS`.
pub const HUB_SMB_BATTERY_STATUS: u32 = 0x0002;
/// `HUB_SMB_ALARM_WARNING`.
pub const HUB_SMB_ALARM_WARNING: u32 = 0x0003;
/// `HUB_SMB_CHARGER_MODE`.
pub const HUB_SMB_CHARGER_MODE: u32 = 0x0004;
/// `HUB_SMB_CHARGER_STATUS`.
pub const HUB_SMB_CHARGER_STATUS: u32 = 0x0005;
/// `HUB_SMB_CHARGER_SPECINF`.
pub const HUB_SMB_CHARGER_SPECINF: u32 = 0x0006;
/// `HUB_SMB_SELECTR_STATE`.
pub const HUB_SMB_SELECTR_STATE: u32 = 0x0007;
/// `HUB_SMB_SELECTR_PRESETS`.
pub const HUB_SMB_SELECTR_PRESETS: u32 = 0x0008;
/// `HUB_SMB_SELECTR_INFO`.
pub const HUB_SMB_SELECTR_INFO: u32 = 0x0009;
/// `HUB_SMB_OPT_MFGFUNC1`.
pub const HUB_SMB_OPT_MFGFUNC1: u32 = 0x0010;
/// `HUB_SMB_OPT_MFGFUNC2`.
pub const HUB_SMB_OPT_MFGFUNC2: u32 = 0x0011;
/// `HUB_SMB_OPT_MFGFUNC3`.
pub const HUB_SMB_OPT_MFGFUNC3: u32 = 0x0012;
/// `HUB_SMB_OPT_MFGFUNC4`.
pub const HUB_SMB_OPT_MFGFUNC4: u32 = 0x0013;
/// `HUB_SMB_OPT_MFGFUNC5`.
pub const HUB_SMB_OPT_MFGFUNC5: u32 = 0x0014;
/// `HUB_CONNECTIONTOSMBUS`.
pub const HUB_CONNECTIONTOSMBUS: u32 = 0x0015;
/// `HUB_OUTPUT_CONNECTION`.
pub const HUB_OUTPUT_CONNECTION: u32 = 0x0016;
/// `HUB_CHARGER_CONNECTION`.
pub const HUB_CHARGER_CONNECTION: u32 = 0x0017;
/// `HUB_BATTERY_INSERTION`.
pub const HUB_BATTERY_INSERTION: u32 = 0x0018;
/// `HUB_USENEXT`.
pub const HUB_USENEXT: u32 = 0x0019;
/// `HUB_OKTOUSE`.
pub const HUB_OKTOUSE: u32 = 0x001a;
/// `HUB_BATTERY_SUPPORTED`.
pub const HUB_BATTERY_SUPPORTED: u32 = 0x001b;
/// `HUB_SELECTOR_REVISION`.
pub const HUB_SELECTOR_REVISION: u32 = 0x001c;
/// `HUB_CHARGING_INDICATOR`.
pub const HUB_CHARGING_INDICATOR: u32 = 0x001d;
/// `HUB_MANUFACTURER_ACCESS`.
pub const HUB_MANUFACTURER_ACCESS: u32 = 0x0028;
/// `HUB_REM_CAPACITY_LIM`.
pub const HUB_REM_CAPACITY_LIM: u32 = 0x0029;
/// `HUB_REM_TIME_LIM`.
pub const HUB_REM_TIME_LIM: u32 = 0x002a;
/// `HUB_ATRATE`.
pub const HUB_ATRATE: u32 = 0x002b;
/// `HUB_CAPACITY_MODE`.
pub const HUB_CAPACITY_MODE: u32 = 0x002c;
/// `HUB_BCAST_TO_CHARGER`.
pub const HUB_BCAST_TO_CHARGER: u32 = 0x002d;
/// `HUB_PRIMARY_BATTERY`.
pub const HUB_PRIMARY_BATTERY: u32 = 0x002e;
/// `HUB_CHANGE_CONTROLLER`.
pub const HUB_CHANGE_CONTROLLER: u32 = 0x002f;
/// `HUB_TERMINATE_CHARGE`.
pub const HUB_TERMINATE_CHARGE: u32 = 0x0040;
/// `HUB_TERMINATE_DISCHARGE`.
pub const HUB_TERMINATE_DISCHARGE: u32 = 0x0041;
/// `HUB_BELOW_REM_CAP_LIM`.
pub const HUB_BELOW_REM_CAP_LIM: u32 = 0x0042;
/// `HUB_REM_TIME_LIM_EXP`.
pub const HUB_REM_TIME_LIM_EXP: u32 = 0x0043;
/// `HUB_CHARGING`.
pub const HUB_CHARGING: u32 = 0x0044;
/// `HUB_DISCHARGING`.
pub const HUB_DISCHARGING: u32 = 0x0045;
/// `HUB_FULLY_CHARGED`.
pub const HUB_FULLY_CHARGED: u32 = 0x0046;
/// `HUB_FULLY_DISCHARGED`.
pub const HUB_FULLY_DISCHARGED: u32 = 0x0047;
/// `HUB_CONDITIONING_FLAG`.
pub const HUB_CONDITIONING_FLAG: u32 = 0x0048;
/// `HUB_ATRATE_OK`.
pub const HUB_ATRATE_OK: u32 = 0x0049;
/// `HUB_SMB_ERROR_CODE`.
pub const HUB_SMB_ERROR_CODE: u32 = 0x004a;
/// `HUB_NEED_REPLACEMENT`.
pub const HUB_NEED_REPLACEMENT: u32 = 0x004b;
/// `HUB_ATRATE_TIMETOFULL`.
pub const HUB_ATRATE_TIMETOFULL: u32 = 0x0060;
/// `HUB_ATRATE_TIMETOEMPTY`.
pub const HUB_ATRATE_TIMETOEMPTY: u32 = 0x0061;
/// `HUB_AVERAGE_CURRENT`.
pub const HUB_AVERAGE_CURRENT: u32 = 0x0062;
/// `HUB_MAXERROR`.
pub const HUB_MAXERROR: u32 = 0x0063;
/// `HUB_REL_STATEOF_CHARGE`.
pub const HUB_REL_STATEOF_CHARGE: u32 = 0x0064;
/// `HUB_ABS_STATEOF_CHARGE`.
pub const HUB_ABS_STATEOF_CHARGE: u32 = 0x0065;
/// `HUB_REM_CAPACITY`.
pub const HUB_REM_CAPACITY: u32 = 0x0066;
/// `HUB_FULLCHARGE_CAPACITY`.
pub const HUB_FULLCHARGE_CAPACITY: u32 = 0x0067;
/// `HUB_RUNTIMETO_EMPTY`.
pub const HUB_RUNTIMETO_EMPTY: u32 = 0x0068;
/// `HUB_AVERAGETIMETO_EMPTY`.
pub const HUB_AVERAGETIMETO_EMPTY: u32 = 0x0069;
/// `HUB_AVERAGETIMETO_FULL`.
pub const HUB_AVERAGETIMETO_FULL: u32 = 0x006a;
/// `HUB_CYCLECOUNT`.
pub const HUB_CYCLECOUNT: u32 = 0x006b;
/// `HUB_BATTPACKMODEL_LEVEL`.
pub const HUB_BATTPACKMODEL_LEVEL: u32 = 0x0080;
/// `HUB_INTERNAL_CHARGE_CTL`.
pub const HUB_INTERNAL_CHARGE_CTL: u32 = 0x0081;
/// `HUB_PRIMARY_BATTERY_SUP`.
pub const HUB_PRIMARY_BATTERY_SUP: u32 = 0x0082;
/// `HUB_DESIGN_CAPACITY`.
pub const HUB_DESIGN_CAPACITY: u32 = 0x0083;
/// `HUB_SPECIFICATION_INFO`.
pub const HUB_SPECIFICATION_INFO: u32 = 0x0084;
/// `HUB_MANUFACTURER_DATE`.
pub const HUB_MANUFACTURER_DATE: u32 = 0x0085;
/// `HUB_SERIAL_NUMBER`.
pub const HUB_SERIAL_NUMBER: u32 = 0x0086;
/// `HUB_IMANUFACTURERNAME`.
pub const HUB_IMANUFACTURERNAME: u32 = 0x0087;
/// `HUB_IDEVICENAME`.
pub const HUB_IDEVICENAME: u32 = 0x0088;
/// `HUB_IDEVICECHEMISTERY`.
pub const HUB_IDEVICECHEMISTERY: u32 = 0x0089;
/// `HUB_MANUFACTURERDATA`.
pub const HUB_MANUFACTURERDATA: u32 = 0x008a;
/// `HUB_RECHARGABLE`.
pub const HUB_RECHARGABLE: u32 = 0x008b;
/// `HUB_WARN_CAPACITY_LIM`.
pub const HUB_WARN_CAPACITY_LIM: u32 = 0x008c;
/// `HUB_CAPACITY_GRANUL1`.
pub const HUB_CAPACITY_GRANUL1: u32 = 0x008d;
/// `HUB_CAPACITY_GRANUL2`.
pub const HUB_CAPACITY_GRANUL2: u32 = 0x008e;
/// `HUB_IOEM_INFORMATION`.
pub const HUB_IOEM_INFORMATION: u32 = 0x008f;
/// `HUB_INHIBIT_CHARGE`.
pub const HUB_INHIBIT_CHARGE: u32 = 0x00c0;
/// `HUB_ENABLE_POLLING`.
pub const HUB_ENABLE_POLLING: u32 = 0x00c1;
/// `HUB_RESTORE_TO_ZERO`.
pub const HUB_RESTORE_TO_ZERO: u32 = 0x00c2;
/// `HUB_AC_PRESENT`.
pub const HUB_AC_PRESENT: u32 = 0x00d0;
/// `HUB_BATTERY_PRESENT`.
pub const HUB_BATTERY_PRESENT: u32 = 0x00d1;
/// `HUB_POWER_FAIL`.
pub const HUB_POWER_FAIL: u32 = 0x00d2;
/// `HUB_ALARM_INHIBITED`.
pub const HUB_ALARM_INHIBITED: u32 = 0x00d3;
/// `HUB_THERMISTOR_UNDRANGE`.
pub const HUB_THERMISTOR_UNDRANGE: u32 = 0x00d4;
/// `HUB_THERMISTOR_HOT`.
pub const HUB_THERMISTOR_HOT: u32 = 0x00d5;
/// `HUB_THERMISTOR_COLD`.
pub const HUB_THERMISTOR_COLD: u32 = 0x00d6;
/// `HUB_THERMISTOR_OVERANGE`.
pub const HUB_THERMISTOR_OVERANGE: u32 = 0x00d7;
/// `HUB_BS_VOLT_OUTOF_RANGE`.
pub const HUB_BS_VOLT_OUTOF_RANGE: u32 = 0x00d8;
/// `HUB_BS_CURR_OUTOF_RANGE`.
pub const HUB_BS_CURR_OUTOF_RANGE: u32 = 0x00d9;
/// `HUB_BS_CURR_NOT_REGULTD`.
pub const HUB_BS_CURR_NOT_REGULTD: u32 = 0x00da;
/// `HUB_BS_VOLT_NOT_REGULTD`.
pub const HUB_BS_VOLT_NOT_REGULTD: u32 = 0x00db;
/// `HUB_MASTER_MODE`.
pub const HUB_MASTER_MODE: u32 = 0x00dc;
/// `HUB_CHARGER_SELECTR_SUP`.
pub const HUB_CHARGER_SELECTR_SUP: u32 = 0x00f0;
/// `HUB_CHARGER_SPEC`.
pub const HUB_CHARGER_SPEC: u32 = 0x00f1;
/// `HUB_LEVEL2`.
pub const HUB_LEVEL2: u32 = 0x00f2;
/// `HUB_LEVEL3`.
pub const HUB_LEVEL3: u32 = 0x00f3;
/// `HUG_POINTER`.
pub const HUG_POINTER: u32 = 0x0001;
/// `HUG_MOUSE`.
pub const HUG_MOUSE: u32 = 0x0002;
/// `HUG_FN_KEY`.
pub const HUG_FN_KEY: u32 = 0x0003;
/// `HUG_JOYSTICK`.
pub const HUG_JOYSTICK: u32 = 0x0004;
/// `HUG_GAME_PAD`.
pub const HUG_GAME_PAD: u32 = 0x0005;
/// `HUG_KEYBOARD`.
pub const HUG_KEYBOARD: u32 = 0x0006;
/// `HUG_KEYPAD`.
pub const HUG_KEYPAD: u32 = 0x0007;
/// `HUG_X`.
pub const HUG_X: u32 = 0x0030;
/// `HUG_Y`.
pub const HUG_Y: u32 = 0x0031;
/// `HUG_Z`.
pub const HUG_Z: u32 = 0x0032;
/// `HUG_RX`.
pub const HUG_RX: u32 = 0x0033;
/// `HUG_RY`.
pub const HUG_RY: u32 = 0x0034;
/// `HUG_RZ`.
pub const HUG_RZ: u32 = 0x0035;
/// `HUG_SLIDER`.
pub const HUG_SLIDER: u32 = 0x0036;
/// `HUG_DIAL`.
pub const HUG_DIAL: u32 = 0x0037;
/// `HUG_WHEEL`.
pub const HUG_WHEEL: u32 = 0x0038;
/// `HUG_HAT_SWITCH`.
pub const HUG_HAT_SWITCH: u32 = 0x0039;
/// `HUG_COUNTED_BUFFER`.
pub const HUG_COUNTED_BUFFER: u32 = 0x003a;
/// `HUG_BYTE_COUNT`.
pub const HUG_BYTE_COUNT: u32 = 0x003b;
/// `HUG_MOTION_WAKEUP`.
pub const HUG_MOTION_WAKEUP: u32 = 0x003c;
/// `HUG_VX`.
pub const HUG_VX: u32 = 0x0040;
/// `HUG_VY`.
pub const HUG_VY: u32 = 0x0041;
/// `HUG_VZ`.
pub const HUG_VZ: u32 = 0x0042;
/// `HUG_VBRX`.
pub const HUG_VBRX: u32 = 0x0043;
/// `HUG_VBRY`.
pub const HUG_VBRY: u32 = 0x0044;
/// `HUG_VBRZ`.
pub const HUG_VBRZ: u32 = 0x0045;
/// `HUG_VNO`.
pub const HUG_VNO: u32 = 0x0046;
/// `HUG_TWHEEL`.
pub const HUG_TWHEEL: u32 = 0x0048;
/// `HUG_SYSTEM_CONTROL`.
pub const HUG_SYSTEM_CONTROL: u32 = 0x0080;
/// `HUG_SYSTEM_POWER_DOWN`.
pub const HUG_SYSTEM_POWER_DOWN: u32 = 0x0081;
/// `HUG_SYSTEM_SLEEP`.
pub const HUG_SYSTEM_SLEEP: u32 = 0x0082;
/// `HUG_SYSTEM_WAKEUP`.
pub const HUG_SYSTEM_WAKEUP: u32 = 0x0083;
/// `HUG_SYSTEM_CONTEXT_MENU`.
pub const HUG_SYSTEM_CONTEXT_MENU: u32 = 0x0084;
/// `HUG_SYSTEM_MAIN_MENU`.
pub const HUG_SYSTEM_MAIN_MENU: u32 = 0x0085;
/// `HUG_SYSTEM_APP_MENU`.
pub const HUG_SYSTEM_APP_MENU: u32 = 0x0086;
/// `HUG_SYSTEM_MENU_HELP`.
pub const HUG_SYSTEM_MENU_HELP: u32 = 0x0087;
/// `HUG_SYSTEM_MENU_EXIT`.
pub const HUG_SYSTEM_MENU_EXIT: u32 = 0x0088;
/// `HUG_SYSTEM_MENU_SELECT`.
pub const HUG_SYSTEM_MENU_SELECT: u32 = 0x0089;
/// `HUG_SYSTEM_MENU_RIGHT`.
pub const HUG_SYSTEM_MENU_RIGHT: u32 = 0x008a;
/// `HUG_SYSTEM_MENU_LEFT`.
pub const HUG_SYSTEM_MENU_LEFT: u32 = 0x008b;
/// `HUG_SYSTEM_MENU_UP`.
pub const HUG_SYSTEM_MENU_UP: u32 = 0x008c;
/// `HUG_SYSTEM_MENU_DOWN`.
pub const HUG_SYSTEM_MENU_DOWN: u32 = 0x008d;
/// `HUD_UNDEFINED`.
pub const HUD_UNDEFINED: u32 = 0x0000;
/// `HUD_DIGITIZER`.
pub const HUD_DIGITIZER: u32 = 0x0001;
/// `HUD_PEN`.
pub const HUD_PEN: u32 = 0x0002;
/// `HUD_TOUCHSCREEN`.
pub const HUD_TOUCHSCREEN: u32 = 0x0004;
/// `HUD_TOUCHPAD`.
pub const HUD_TOUCHPAD: u32 = 0x0005;
/// `HUD_CONFIG`.
pub const HUD_CONFIG: u32 = 0x000e;
/// `HUD_STYLUS`.
pub const HUD_STYLUS: u32 = 0x0020;
/// `HUD_FINGER`.
pub const HUD_FINGER: u32 = 0x0022;
/// `HUD_TIP_PRESSURE`.
pub const HUD_TIP_PRESSURE: u32 = 0x0030;
/// `HUD_BARREL_PRESSURE`.
pub const HUD_BARREL_PRESSURE: u32 = 0x0031;
/// `HUD_IN_RANGE`.
pub const HUD_IN_RANGE: u32 = 0x0032;
/// `HUD_TOUCH`.
pub const HUD_TOUCH: u32 = 0x0033;
/// `HUD_UNTOUCH`.
pub const HUD_UNTOUCH: u32 = 0x0034;
/// `HUD_TAP`.
pub const HUD_TAP: u32 = 0x0035;
/// `HUD_QUALITY`.
pub const HUD_QUALITY: u32 = 0x0036;
/// `HUD_DATA_VALID`.
pub const HUD_DATA_VALID: u32 = 0x0037;
/// `HUD_TRANSDUCER_INDEX`.
pub const HUD_TRANSDUCER_INDEX: u32 = 0x0038;
/// `HUD_TABLET_FKEYS`.
pub const HUD_TABLET_FKEYS: u32 = 0x0039;
/// `HUD_PROGRAM_CHANGE_KEYS`.
pub const HUD_PROGRAM_CHANGE_KEYS: u32 = 0x003a;
/// `HUD_BATTERY_STRENGTH`.
pub const HUD_BATTERY_STRENGTH: u32 = 0x003b;
/// `HUD_INVERT`.
pub const HUD_INVERT: u32 = 0x003c;
/// `HUD_X_TILT`.
pub const HUD_X_TILT: u32 = 0x003d;
/// `HUD_Y_TILT`.
pub const HUD_Y_TILT: u32 = 0x003e;
/// `HUD_AZIMUTH`.
pub const HUD_AZIMUTH: u32 = 0x003f;
/// `HUD_ALTITUDE`.
pub const HUD_ALTITUDE: u32 = 0x0040;
/// `HUD_TWIST`.
pub const HUD_TWIST: u32 = 0x0041;
/// `HUD_TIP_SWITCH`.
pub const HUD_TIP_SWITCH: u32 = 0x0042;
/// `HUD_SEC_TIP_SWITCH`.
pub const HUD_SEC_TIP_SWITCH: u32 = 0x0043;
/// `HUD_BARREL_SWITCH`.
pub const HUD_BARREL_SWITCH: u32 = 0x0044;
/// `HUD_ERASER`.
pub const HUD_ERASER: u32 = 0x0045;
/// `HUD_TABLET_PICK`.
pub const HUD_TABLET_PICK: u32 = 0x0046;
/// `HUD_CONFIDENCE`.
pub const HUD_CONFIDENCE: u32 = 0x0047;
/// `HUD_WIDTH`.
pub const HUD_WIDTH: u32 = 0x0048;
/// `HUD_HEIGHT`.
pub const HUD_HEIGHT: u32 = 0x0049;
/// `HUD_CONTACTID`.
pub const HUD_CONTACTID: u32 = 0x0051;
/// `HUD_INPUT_MODE`.
pub const HUD_INPUT_MODE: u32 = 0x0052;
/// `HUD_DEVICE_INDEX`.
pub const HUD_DEVICE_INDEX: u32 = 0x0053;
/// `HUD_CONTACTCOUNT`.
pub const HUD_CONTACTCOUNT: u32 = 0x0054;
/// `HUD_CONTACT_MAX`.
pub const HUD_CONTACT_MAX: u32 = 0x0055;
/// `HUD_SCAN_TIME`.
pub const HUD_SCAN_TIME: u32 = 0x0056;
/// `HUD_BUTTON_TYPE`.
pub const HUD_BUTTON_TYPE: u32 = 0x0059;
/// `HUD_SECONDARY_BARREL_SWITCH`.
pub const HUD_SECONDARY_BARREL_SWITCH: u32 = 0x005A;
/// `HUD_WACOM_X`.
pub const HUD_WACOM_X: u32 = 0x0130;
/// `HUD_WACOM_Y`.
pub const HUD_WACOM_Y: u32 = 0x0131;
/// `HUD_WACOM_DISTANCE`.
pub const HUD_WACOM_DISTANCE: u32 = 0x0132;
/// `HUD_WACOM_PAD_BUTTONS00`.
pub const HUD_WACOM_PAD_BUTTONS00: u32 = 0x0910;
/// `HUD_WACOM_BATTERY`.
pub const HUD_WACOM_BATTERY: u32 = 0x1013;
/// `HUL_NUM_LOCK`.
pub const HUL_NUM_LOCK: u32 = 0x0001;
/// `HUL_CAPS_LOCK`.
pub const HUL_CAPS_LOCK: u32 = 0x0002;
/// `HUL_SCROLL_LOCK`.
pub const HUL_SCROLL_LOCK: u32 = 0x0003;
/// `HUL_COMPOSE`.
pub const HUL_COMPOSE: u32 = 0x0004;
/// `HUL_KANA`.
pub const HUL_KANA: u32 = 0x0005;
/// `HUC_CONTROL`.
pub const HUC_CONTROL: u32 = 0x0001;
/// `HUC_TRACK_NEXT`.
pub const HUC_TRACK_NEXT: u32 = 0x00b5;
/// `HUC_TRACK_PREV`.
pub const HUC_TRACK_PREV: u32 = 0x00b6;
/// `HUC_STOP`.
pub const HUC_STOP: u32 = 0x00b7;
/// `HUC_PLAY_PAUSE`.
pub const HUC_PLAY_PAUSE: u32 = 0x00cd;
/// `HUC_VOLUME`.
pub const HUC_VOLUME: u32 = 0x00e0;
/// `HUC_MUTE`.
pub const HUC_MUTE: u32 = 0x00e2;
/// `HUC_VOL_INC`.
pub const HUC_VOL_INC: u32 = 0x00e9;
/// `HUC_VOL_DEC`.
pub const HUC_VOL_DEC: u32 = 0x00ea;
/// `HUC_AC_PAN`.
pub const HUC_AC_PAN: u32 = 0x0238;
/// `HUF_U2FHID`.
pub const HUF_U2FHID: u32 = 0x0001;
/// `HUF_RAW_IN_DATA_REPORT`.
pub const HUF_RAW_IN_DATA_REPORT: u32 = 0x0020;
/// `HUF_RAW_OUT_DATA_REPORT`.
pub const HUF_RAW_OUT_DATA_REPORT: u32 = 0x0021;
/// `HCOLL_PHYSICAL`.
pub const HCOLL_PHYSICAL: u32 = 0;
/// `HCOLL_APPLICATION`.
pub const HCOLL_APPLICATION: u32 = 1;
/// `HCOLL_LOGICAL`.
pub const HCOLL_LOGICAL: u32 = 2;
/// `HIO_CONST`.
pub const HIO_CONST: u32 = 0x001;
/// `HIO_VARIABLE`.
pub const HIO_VARIABLE: u32 = 0x002;
/// `HIO_RELATIVE`.
pub const HIO_RELATIVE: u32 = 0x004;
/// `HIO_WRAP`.
pub const HIO_WRAP: u32 = 0x008;
/// `HIO_NONLINEAR`.
pub const HIO_NONLINEAR: u32 = 0x010;
/// `HIO_NOPREF`.
pub const HIO_NOPREF: u32 = 0x020;
/// `HIO_NULLSTATE`.
pub const HIO_NULLSTATE: u32 = 0x040;
/// `HIO_VOLATILE`.
pub const HIO_VOLATILE: u32 = 0x080;
/// `HIO_BUFBYTES`.
pub const HIO_BUFBYTES: u32 = 0x100;
/// `HCC_UNDEFINED`.
pub const HCC_UNDEFINED: u32 = 0x00;
/// `HCC_MAX`.
pub const HCC_MAX: u32 = 0x23;

/// `HID_USAGE2(p, u)`: a usage with its usage page in the high half.
pub const fn hid_usage2(p: u32, u: u32) -> u32 {
    (p << 16) | u
}

/// `HID_GET_USAGE(u)`: the usage within its page.
pub const fn hid_get_usage(u: u32) -> u32 {
    u & 0xffff
}

/// `HID_GET_USAGE_PAGE(u)`: the usage page of a usage.
pub const fn hid_get_usage_page(u: u32) -> u32 {
    (u >> 16) & 0xffff
}

/// `enum hid_kind`: what a parse hands out (`hid_all` is every kind).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(non_camel_case_types)] // OpenBSD names, verbatim, for grep-ability
pub enum HidKind {
    /// `hid_input`.
    #[default]
    hid_input,
    /// `hid_output`.
    hid_output,
    /// `hid_feature`.
    hid_feature,
    /// `hid_collection`.
    hid_collection,
    /// `hid_endcollection`.
    hid_endcollection,
    /// `hid_all`: no filter.
    hid_all,
}

pub use HidKind::*;

/// `struct hid_location`: where a field lives in its report, in bits.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HidLocation {
    /// `size`: bits per element.
    pub size: u32,
    /// `count`: number of elements.
    pub count: u32,
    /// `pos`: bit offset in the report (after the report ID byte, which is not counted).
    pub pos: u32,
}

/// `struct hid_item`: one parsed item: the global and local state in force, the kind, the
/// flags of the Main item and its location.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(non_snake_case)] // `report_ID`, as OpenBSD names it
pub struct HidItem {
    /// `_usage_page`: the usage page, already shifted into the high half.
    pub _usage_page: u32,
    /// `logical_minimum`.
    pub logical_minimum: i32,
    /// `logical_maximum`.
    pub logical_maximum: i32,
    /// `physical_minimum`.
    pub physical_minimum: i32,
    /// `physical_maximum`.
    pub physical_maximum: i32,
    /// `unit_exponent`.
    pub unit_exponent: u32,
    /// `unit`.
    pub unit: u32,
    /// `report_ID`.
    pub report_ID: u32,
    /// `usage`: page and usage (`HID_USAGE2`).
    pub usage: u32,
    /// `usage_minimum`.
    pub usage_minimum: u32,
    /// `usage_maximum`.
    pub usage_maximum: u32,
    /// `designator_index`.
    pub designator_index: u32,
    /// `designator_minimum`.
    pub designator_minimum: u32,
    /// `designator_maximum`.
    pub designator_maximum: u32,
    /// `string_index`.
    pub string_index: u32,
    /// `string_minimum`.
    pub string_minimum: u32,
    /// `string_maximum`.
    pub string_maximum: u32,
    /// `set_delimiter`.
    pub set_delimiter: u32,
    /// `collection`: the collection type of a `hid_collection` item (`HCOLL_*`).
    pub collection: u32,
    /// `collevel`: collection nesting depth.
    pub collevel: i32,
    /// `kind`.
    pub kind: HidKind,
    /// `flags`: the data bits of the Main item (`HIO_*`).
    pub flags: u32,
    /// `loc`: where the field is.
    pub loc: HidLocation,
}

/// `struct hid_pos_data`: the next free bit position of one report ID.
#[derive(Clone, Copy, Default)]
struct HidPosData {
    rid: i32,
    pos: u32,
}

/// `struct hid_data`: the state of one parse over a report descriptor.
pub struct HidData<'a> {
    /// `start` and `end`: the descriptor.
    desc: &'a [u8],
    /// `p`: the next byte to read, an index into `desc`.
    p: usize,
    /// `cur`: the item under construction, one per Push level.
    cur: [HidItem; MAXPUSH],
    /// `last_pos`: where each report ID's data ended.
    last_pos: [HidPosData; MAXID],
    /// `usages_min` and `usages_max`: the usages of the Main item being built.
    usages_min: [u32; MAXUSAGE],
    usages_max: [u32; MAXUSAGE],
    /// `usage_last`: last seen usage.
    usage_last: u32,
    /// `loc_size`: last seen size.
    loc_size: u32,
    /// `loc_count`: last seen count.
    loc_count: u32,
    /// `ncount`: end usage item count.
    ncount: u32,
    /// `icount`: current usage item count.
    icount: u32,
    /// `kind`: the kind this parse hands out.
    kind: HidKind,
    /// `pushlevel`: current push level.
    pushlevel: u8,
    /// `nusage`: end "usages_min/max" index.
    nusage: u8,
    /// `iusage`: current "usages_min/max" index.
    iusage: u8,
    /// `ousage`: current "usages_min/max" offset.
    ousage: u8,
    /// `susage`: usage set flags.
    susage: u8,
}

impl HidData<'_> {
    /// `hid_switch_rid`: make `nextid` the current report ID of item `ci`: save the position
    /// reached in the old ID, restore the one reached in the new ID.
    fn hid_switch_rid(&mut self, ci: usize, nextid: i32) {
        let cur_id = self.cur[ci].report_ID;
        if cur_id == nextid as u32 {
            return;
        }

        // save current position for current rID
        let i = self.find_pos_slot(cur_id as i32);
        if i != MAXID {
            self.last_pos[i].rid = cur_id as i32;
            self.last_pos[i].pos = self.cur[ci].loc.pos;
        }

        // store next report ID
        self.cur[ci].report_ID = nextid as u32;

        // lookup last position for next rID
        let i = self.find_pos_slot(nextid);
        if i != MAXID {
            self.last_pos[i].rid = nextid;
            self.cur[ci].loc.pos = self.last_pos[i].pos;
        } else {
            // out of RID entries, position is set to zero
            self.cur[ci].loc.pos = 0;
        }
    }

    /// The slot of `last_pos` for report ID `id`: 0 for ID 0, else the entry holding `id`
    /// or the first free one, `MAXID` when the table is full.
    fn find_pos_slot(&self, id: i32) -> usize {
        if id == 0 {
            return 0;
        }
        let mut i = 1;
        while i != MAXID {
            if self.last_pos[i].rid == id || self.last_pos[i].rid == 0 {
                break;
            }
            i += 1;
        }
        i
    }

    /// `hid_get_byte`: the next byte, advancing `wSize` bytes (to the end at most); 0 once
    /// the end is reached.
    fn hid_get_byte(&mut self, w_size: u16) -> u8 {
        let end = self.desc.len();
        if self.p >= end {
            return 0;
        }
        let retval = self.desc[self.p];
        if end - self.p < usize::from(w_size) {
            self.p = end;
        } else {
            self.p += usize::from(w_size);
        }
        retval
    }
}

/// `hid_clear_local`: forget the local items of `c` (the usages, designators and strings
/// of the Main item just handed out).
fn hid_clear_local(c: &mut HidItem) {
    c.loc.count = 0;
    c.loc.size = 0;
    c.usage = 0;
    c.usage_minimum = 0;
    c.usage_maximum = 0;
    c.designator_index = 0;
    c.designator_minimum = 0;
    c.designator_maximum = 0;
    c.string_index = 0;
    c.string_minimum = 0;
    c.string_maximum = 0;
    c.set_delimiter = 0;
}

/// `hid_start_parse`: start parsing the descriptor `d`, handing out the items of `kind`.
pub fn hid_start_parse(d: &[u8], kind: HidKind) -> Box<HidData<'_>> {
    Box::new(HidData {
        desc: d,
        p: 0,
        cur: [HidItem::default(); MAXPUSH],
        last_pos: [HidPosData::default(); MAXID],
        usages_min: [0; MAXUSAGE],
        usages_max: [0; MAXUSAGE],
        usage_last: 0,
        loc_size: 0,
        loc_count: 0,
        ncount: 0,
        icount: 0,
        kind,
        pushlevel: 0,
        nusage: 0,
        iusage: 0,
        ousage: 0,
        susage: 0,
    })
}

/// `hid_end_parse`: finish a parse (the C frees the `M_TEMP` block; here the box drops).
pub fn hid_end_parse(s: Box<HidData<'_>>) {
    drop(s);
}

/// `hid_get_item`: the next item of the parse's kind into `h`; `false` at the end of the
/// descriptor, on an invalid End Collection, or once the Push stack has overflowed.
pub fn hid_get_item(s: &mut HidData<'_>, h: &mut HidItem) -> bool {
    if usize::from(s.pushlevel) >= MAXPUSH {
        return false;
    }

    'top: loop {
        let ci = usize::from(s.pushlevel);

        // check if there is an array of items
        if s.icount < s.ncount {
            // get current usage
            if s.iusage < s.nusage {
                let iu = usize::from(s.iusage);
                let dval = s.usages_min[iu].wrapping_add(u32::from(s.ousage));
                s.cur[ci].usage = dval;
                s.usage_last = dval;
                if dval == s.usages_max[iu] {
                    s.iusage += 1;
                    s.ousage = 0;
                } else {
                    s.ousage += 1;
                }
            }
            // else: the last usage stays in force
            s.icount += 1;
            // Only copy HID item, increment position and return if correct kind!
            if s.kind == hid_all || s.kind == s.cur[ci].kind {
                *h = s.cur[ci];
                let c = &mut s.cur[ci];
                c.loc.pos = c.loc.pos.wrapping_add(c.loc.size.wrapping_mul(c.loc.count));
                return true;
            }
        }

        // reset state variables
        s.icount = 0;
        s.ncount = 0;
        s.iusage = 0;
        s.nusage = 0;
        s.susage = 0;
        s.ousage = 0;
        hid_clear_local(&mut s.cur[ci]);

        // get next item
        while s.p != s.desc.len() {
            let ci = usize::from(s.pushlevel);
            let mut b_size = u32::from(s.hid_get_byte(1));
            let b_tag;
            let b_type;
            if b_size == 0xfe {
                // long item
                b_size = u32::from(s.hid_get_byte(1));
                b_size |= u32::from(s.hid_get_byte(1)) << 8;
                b_tag = u32::from(s.hid_get_byte(1));
                b_type = 0xff; // XXX what should it be
            } else {
                // short item
                b_tag = b_size >> 4;
                b_type = (b_size >> 2) & 3;
                b_size &= 3;
                if b_size == 3 {
                    b_size = 4;
                }
            }
            let (mut uval, dval): (u32, i32) = match b_size {
                0 => (0, 0),
                1 => {
                    let uval = u32::from(s.hid_get_byte(1));
                    (uval, i32::from(uval as u8 as i8))
                }
                2 => {
                    let mut uval = u32::from(s.hid_get_byte(1));
                    uval |= u32::from(s.hid_get_byte(1)) << 8;
                    (uval, i32::from(uval as u16 as i16))
                }
                4 => {
                    let mut uval = u32::from(s.hid_get_byte(1));
                    uval |= u32::from(s.hid_get_byte(1)) << 8;
                    uval |= u32::from(s.hid_get_byte(1)) << 16;
                    uval |= u32::from(s.hid_get_byte(1)) << 24;
                    (uval, uval as i32)
                }
                _ => {
                    // bad length: skip the data
                    let _ = s.hid_get_byte(b_size as u16);
                    continue;
                }
            };

            match b_type {
                0 => {
                    // Main
                    match b_tag {
                        8 | 9 | 11 => {
                            // Input, Output, Feature
                            let c = &mut s.cur[ci];
                            c.kind = match b_tag {
                                8 => hid_input,
                                9 => hid_output,
                                _ => hid_feature,
                            };
                            c.flags = dval as u32;
                            c.loc.count = s.loc_count;
                            c.loc.size = s.loc_size;

                            if c.flags & HIO_VARIABLE != 0 {
                                // range check usage count
                                if c.loc.count > MAXLOCCNT {
                                    s.ncount = MAXLOCCNT;
                                } else {
                                    s.ncount = c.loc.count;
                                }

                                // The "top" loop will return one and one item:
                                c.loc.count = 1;
                            } else {
                                s.ncount = 1;
                            }
                            continue 'top;
                        }
                        10 => {
                            // Collection
                            let c = &mut s.cur[ci];
                            c.kind = hid_collection;
                            c.collection = uval;
                            c.collevel += 1;
                            c.usage = s.usage_last;
                            *h = *c;
                            return true;
                        }
                        12 => {
                            // End collection
                            let c = &mut s.cur[ci];
                            c.kind = hid_endcollection;
                            if c.collevel == 0 {
                                // invalid end collection
                                return false;
                            }
                            c.collevel -= 1;
                            *h = *c;
                            return true;
                        }
                        _ => {}
                    }
                }
                1 => {
                    // Global
                    match b_tag {
                        0 => s.cur[ci]._usage_page = uval << 16,
                        1 => s.cur[ci].logical_minimum = dval,
                        2 => s.cur[ci].logical_maximum = dval,
                        3 => s.cur[ci].physical_minimum = dval,
                        4 => s.cur[ci].physical_maximum = dval,
                        5 => s.cur[ci].unit_exponent = uval,
                        6 => s.cur[ci].unit = uval,
                        7 => s.loc_size = uval,
                        8 => s.hid_switch_rid(ci, dval),
                        9 => s.loc_count = uval,
                        10 if usize::from(s.pushlevel) < MAXPUSH - 1 => {
                            // Push (else: cannot push)
                            s.pushlevel += 1;
                            let ni = usize::from(s.pushlevel);
                            s.cur[ni] = s.cur[ci];
                            // store size and count
                            s.cur[ci].loc.size = s.loc_size;
                            s.cur[ci].loc.count = s.loc_count;
                            // the update of the current item pointer is `pushlevel`
                        }
                        11 if s.pushlevel > 0 => {
                            // Pop (else: cannot pop)
                            s.pushlevel -= 1;
                            let ni = usize::from(s.pushlevel);
                            // preserve position
                            let oldpos = s.cur[ci].loc.pos;
                            // restore size and count
                            s.loc_size = s.cur[ni].loc.size;
                            s.loc_count = s.cur[ni].loc.count;
                            // set default item location
                            s.cur[ni].loc.pos = oldpos;
                            s.cur[ni].loc.size = 0;
                            s.cur[ni].loc.count = 0;
                        }
                        _ => {}
                    }
                }
                2 => {
                    // Local
                    match b_tag {
                        0 => {
                            if b_size != 4 {
                                uval |= s.cur[ci]._usage_page;
                            }

                            // set last usage, in case of a collection
                            s.usage_last = uval;

                            if usize::from(s.nusage) < MAXUSAGE {
                                let n = usize::from(s.nusage);
                                s.usages_min[n] = uval;
                                s.usages_max[n] = uval;
                                s.nusage += 1;
                            }
                            // else: max usage reached

                            // clear any pending usage sets
                            s.susage = 0;
                        }
                        1 | 2 => {
                            if b_tag == 1 {
                                s.susage |= 1;
                                if b_size != 4 {
                                    uval |= s.cur[ci]._usage_page;
                                }
                                s.cur[ci].usage_minimum = uval;
                            } else {
                                s.susage |= 2;
                                if b_size != 4 {
                                    uval |= s.cur[ci]._usage_page;
                                }
                                s.cur[ci].usage_maximum = uval;
                            }

                            // check_set
                            if s.susage != 3 {
                                continue;
                            }

                            // sanity check
                            if usize::from(s.nusage) < MAXUSAGE
                                && s.cur[ci].usage_minimum <= s.cur[ci].usage_maximum
                            {
                                // add usage range
                                let n = usize::from(s.nusage);
                                s.usages_min[n] = s.cur[ci].usage_minimum;
                                s.usages_max[n] = s.cur[ci].usage_maximum;
                                s.nusage += 1;
                            }
                            // else: usage set dropped
                            s.susage = 0;
                        }
                        3 => s.cur[ci].designator_index = uval,
                        4 => s.cur[ci].designator_minimum = uval,
                        5 => s.cur[ci].designator_maximum = uval,
                        7 => s.cur[ci].string_index = uval,
                        8 => s.cur[ci].string_minimum = uval,
                        9 => s.cur[ci].string_maximum = uval,
                        10 => s.cur[ci].set_delimiter = uval,
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        return false;
    }
}

/// `hid_report_size`: the size in bytes of the report with ID `id` of kind `k` (without
/// the report ID byte).
pub fn hid_report_size(buf: &[u8], k: HidKind, id: u8) -> i32 {
    let mut h = HidItem::default();
    let mut lo: i32 = -1;
    let mut hi: i32 = -1;
    let mut d = hid_start_parse(buf, k);
    while hid_get_item(&mut d, &mut h) {
        if h.report_ID == u32::from(id) && h.kind == k {
            if lo < 0 {
                lo = h.loc.pos as i32;
                #[cfg(feature = "diagnostic")]
                if lo != 0 {
                    crate::kprintf!("hid_report_size: lo != 0\n");
                }
            }
            hi = h.loc.pos.wrapping_add(h.loc.size.wrapping_mul(h.loc.count)) as i32;
        }
    }
    hid_end_parse(d);
    (hi.wrapping_sub(lo).wrapping_add(7)) / 8
}

/// `hid_locate`: find the first non-constant item of kind `k`, usage `u` and report ID
/// `id`; its location and flags go to `loc` and `flags` (when given). When there is none,
/// `loc.size` and `flags` are set to 0 and the result is `false`.
pub fn hid_locate(
    desc: &[u8],
    u: u32,
    id: u8,
    k: HidKind,
    loc: Option<&mut HidLocation>,
    flags: Option<&mut u32>,
) -> bool {
    let mut h = HidItem::default();
    let mut d = hid_start_parse(desc, k);
    while hid_get_item(&mut d, &mut h) {
        if h.kind == k && h.flags & HIO_CONST == 0 && h.usage == u && h.report_ID == u32::from(id) {
            if let Some(loc) = loc {
                *loc = h.loc;
            }
            if let Some(flags) = flags {
                *flags = h.flags;
            }
            hid_end_parse(d);
            return true;
        }
    }
    hid_end_parse(d);
    if let Some(loc) = loc {
        loc.size = 0;
    }
    if let Some(flags) = flags {
        *flags = 0;
    }
    false
}

/// `hid_get_data_sub`: the field at `loc` out of the report `buf`, sign-extended when
/// `is_signed`. Bytes past the end of the report read as 0; a field wider than 32 bits is
/// cut to 32.
pub fn hid_get_data_sub(buf: &[u8], loc: &HidLocation, is_signed: bool) -> u32 {
    let hpos = loc.pos;
    let mut hsize = loc.size;

    // Range check and limit
    if hsize == 0 {
        return 0;
    }
    if hsize > 32 {
        hsize = 32;
    }

    // Get data in a safe way
    let mut data: u32 = 0;
    let mut rpos = hpos / 8;
    let mut n = hsize.div_ceil(8) as u8;
    rpos = rpos.wrapping_add(u32::from(n));
    while n != 0 {
        n -= 1;
        rpos = rpos.wrapping_sub(1);
        if let Some(&b) = buf.get(rpos as usize) {
            data |= u32::from(b) << (8 * u32::from(n));
        }
    }

    // Correctly shift down data
    data >>= hpos % 8;
    let n = 32 - hsize;

    // Mask and sign extend in one
    if is_signed {
        (((data << n) as i32) >> n) as u32
    } else {
        (data << n) >> n
    }
}

/// `hid_get_data`: the signed field at `loc` out of the report `buf`.
pub fn hid_get_data(buf: &[u8], loc: &HidLocation) -> i32 {
    hid_get_data_sub(buf, loc, true) as i32
}

/// `hid_get_udata`: the unsigned field at `loc` out of the report `buf`.
pub fn hid_get_udata(buf: &[u8], loc: &HidLocation) -> u32 {
    hid_get_data_sub(buf, loc, false)
}

/// `hid_is_collection`: whether the application collection of `usage` ends within report
/// ID `id`.
pub fn hid_is_collection(desc: &[u8], id: u8, usage: i32) -> bool {
    let mut hi = HidItem::default();
    let mut coll_usage: u32 = !0;
    let mut hd = hid_start_parse(desc, hid_all);

    while hid_get_item(&mut hd, &mut hi) {
        if hi.kind == hid_collection && hi.collection == HCOLL_APPLICATION {
            coll_usage = hi.usage;
        }
        if hi.kind == hid_endcollection
            && coll_usage == usage as u32
            && hi.report_ID == u32::from(id)
        {
            hid_end_parse(hd);
            return true;
        }
    }
    hid_end_parse(hd);
    false
}

/// `hid_get_collection_data`: a parse of `desc` positioned just after the collection of
/// type `collection` and usage `usage` (so that the next item is inside it), or `None`.
pub fn hid_get_collection_data(
    desc: &[u8],
    usage: i32,
    collection: u32,
) -> Option<Box<HidData<'_>>> {
    let mut hi = HidItem::default();
    let mut hd = hid_start_parse(desc, hid_all);

    while hid_get_item(&mut hd, &mut hi) {
        if hi.kind == hid_collection && hi.collection == collection && hi.usage == usage as u32 {
            return Some(hd);
        }
    }
    hid_end_parse(hd);
    None
}

/// `hid_get_id_of_collection`: the report ID of the collection of type `collection` and
/// usage `usage`; `None` where the C returns -1.
pub fn hid_get_id_of_collection(desc: &[u8], usage: u32, collection: u32) -> Option<u32> {
    let mut hi = HidItem::default();
    let mut hd = hid_start_parse(desc, hid_all);

    while hid_get_item(&mut hd, &mut hi) {
        if hi.kind == hid_collection && hi.collection == collection && hi.usage == usage {
            hid_end_parse(hd);
            return Some(hi.report_ID);
        }
    }
    hid_end_parse(hd);
    None
}

/// `hid_find_report`: find the first report of kind `kind` that contains each of the given
/// `usages` (1 to 32 of them) and belongs to an application collection with the `app_usage`
/// type.
///
/// If `coll_usages` is `None`, the search skips collections with usages from vendor pages
/// (0xFF00 to 0xFFFF). If it is `Some`, it is a 0-terminated sequence of collection usages,
/// and the search skips collections with usages not present in this set (it is not necessary
/// to include the usage of the application collection here).
///
/// The result is the report ID (0 for a single report without an ID), `None` for no match.
pub fn hid_find_report(
    desc: &[u8],
    kind: HidKind,
    app_usage: i32,
    usages: &[i32],
    coll_usages: Option<&[i32]>,
) -> Option<u32> {
    let mut h = HidItem::default();
    let mut matches: u64 = 0;
    let mut cur_id: Option<u32> = None;
    let mut skip: i32 = 0;
    let n_usages = usages.len().min(32);
    let mut hd = hid_start_parse(desc, hid_all);

    while hid_get_item(&mut hd, &mut h) {
        if cur_id != Some(h.report_ID) {
            matches = 0;
            cur_id = Some(h.report_ID);
        }
        if h.kind == hid_collection {
            if skip != 0 {
                continue;
            }
            if h.collevel == 1 {
                if h.usage != app_usage as u32 {
                    skip = 1;
                }
            } else if let Some(coll_usages) = coll_usages {
                let mut found = false;
                for &cu in coll_usages {
                    if cu as u32 == h.usage {
                        found = true;
                        break;
                    }
                    if cu == 0 {
                        break;
                    }
                }
                if !found {
                    skip = h.collevel;
                }
            } else if hid_get_usage_page(h.usage) >= 0xff00 {
                skip = h.collevel;
            }
        } else if h.kind == hid_endcollection && h.collevel < skip {
            skip = 0;
        }
        if h.kind != kind || skip != 0 {
            continue;
        }
        for (i, &u) in usages.iter().enumerate().take(n_usages) {
            if h.usage == u as u32 && matches & (1 << i) == 0 {
                matches |= 1 << i;
                if matches != (1u64 << n_usages) - 1 {
                    break;
                }
                hid_end_parse(hd);
                return Some(h.report_ID);
            }
        }
    }
    hid_end_parse(hd);
    None
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for the HID report descriptor parser, over the descriptors of real devices:
    // QEMU's `usb-kbd`, the boot-protocol mouse of the HID 1.11 specification (appendix E.10),
    // and hand-made ones for report IDs, Push/Pop, long items, usage tables and truncation.

    use std::vec::Vec;
    use std::{assert, assert_eq};

    use super::*;

    /// `qemu_keyboard_hid_report_desc` (hw/usb/dev-hid.c): the 8 modifier bits, a constant byte,
    /// 5 LEDs plus 3 bits of padding out, and an array of 6 key codes.
    const QEMU_KBD: &[u8] = &[
        0x05, 0x01, 0x09, 0x06, 0xa1, 0x01, 0x75, 0x01, 0x95, 0x08, 0x05, 0x07, 0x19, 0xe0, 0x29,
        0xe7, 0x15, 0x00, 0x25, 0x01, 0x81, 0x02, 0x95, 0x01, 0x75, 0x08, 0x81, 0x01, 0x95, 0x05,
        0x75, 0x01, 0x05, 0x08, 0x19, 0x01, 0x29, 0x05, 0x91, 0x02, 0x95, 0x01, 0x75, 0x03, 0x91,
        0x01, 0x95, 0x06, 0x75, 0x08, 0x15, 0x00, 0x25, 0xff, 0x05, 0x07, 0x19, 0x00, 0x29, 0xff,
        0x81, 0x00, 0xc0,
    ];

    /// The boot protocol mouse (HID 1.11, E.10): three buttons, five bits of padding, X and Y.
    const BOOT_MOUSE: &[u8] = &[
        0x05, 0x01, 0x09, 0x02, 0xa1, 0x01, 0x09, 0x01, 0xa1, 0x00, 0x05, 0x09, 0x19, 0x01, 0x29,
        0x03, 0x15, 0x00, 0x25, 0x01, 0x95, 0x03, 0x75, 0x01, 0x81, 0x02, 0x95, 0x01, 0x75, 0x05,
        0x81, 0x01, 0x05, 0x01, 0x09, 0x30, 0x09, 0x31, 0x15, 0x81, 0x25, 0x7f, 0x75, 0x08, 0x95,
        0x02, 0x81, 0x06, 0xc0, 0xc0,
    ];

    fn items(desc: &[u8], kind: HidKind) -> Vec<HidItem> {
        let mut d = hid_start_parse(desc, kind);
        let mut h = HidItem::default();
        let mut v = Vec::new();
        while hid_get_item(&mut d, &mut h) {
            // collections come out whatever the kind asked for, as in the C
            if kind == hid_all || h.kind == kind {
                v.push(h);
            }
        }
        hid_end_parse(d);
        v
    }

    #[test]
    fn qemu_kbd_inputs_are_eight_modifiers_a_constant_and_an_array() {
        let v = items(QEMU_KBD, hid_input);
        assert_eq!(v.len(), 10);
        for (i, h) in v[..8].iter().enumerate() {
            assert_eq!(h.usage, hid_usage2(HUP_KEYBOARD, 0xe0 + i as u32));
            assert_eq!(h.flags, HIO_VARIABLE);
            assert_eq!(
                h.loc,
                HidLocation {
                    size: 1,
                    count: 1,
                    pos: i as u32
                }
            );
            assert_eq!(h.logical_minimum, 0);
            assert_eq!(h.logical_maximum, 1);
        }
        assert_eq!(v[8].flags, HIO_CONST);
        assert_eq!(
            v[8].loc,
            HidLocation {
                size: 8,
                count: 1,
                pos: 8
            }
        );
        // the array: not variable, 6 elements of 8 bits at bit 16; its usage is the start of the
        // range
        assert_eq!(v[9].flags, 0);
        assert_eq!(
            v[9].loc,
            HidLocation {
                size: 8,
                count: 6,
                pos: 16
            }
        );
        // `Logical Maximum (255)` is a one-byte item and reads signed, as in the C
        assert_eq!(v[9].logical_maximum, -1);
    }

    #[test]
    fn qemu_kbd_outputs_are_five_leds_and_padding() {
        let v = items(QEMU_KBD, hid_output);
        assert_eq!(v.len(), 6);
        for (i, h) in v[..5].iter().enumerate() {
            assert_eq!(h.usage, hid_usage2(HUP_LED, 1 + i as u32));
            assert_eq!(
                h.loc,
                HidLocation {
                    size: 1,
                    count: 1,
                    pos: i as u32
                }
            );
        }
        assert_eq!(v[5].flags, HIO_CONST);
        assert_eq!(
            v[5].loc,
            HidLocation {
                size: 3,
                count: 1,
                pos: 5
            }
        );
    }

    #[test]
    fn qemu_kbd_collections_nest_and_balance() {
        let v = items(QEMU_KBD, hid_all);
        let colls: Vec<_> = v
            .iter()
            .filter(|h| h.kind == hid_collection || h.kind == hid_endcollection)
            .collect();
        assert_eq!(colls.len(), 2);
        assert_eq!(colls[0].kind, hid_collection);
        assert_eq!(colls[0].collection, HCOLL_APPLICATION);
        assert_eq!(
            colls[0].usage,
            hid_usage2(HUP_GENERIC_DESKTOP, HUG_KEYBOARD)
        );
        assert_eq!(colls[0].collevel, 1);
        assert_eq!(colls[1].kind, hid_endcollection);
        assert_eq!(colls[1].collevel, 0);
    }

    #[test]
    fn qemu_kbd_report_sizes() {
        assert_eq!(hid_report_size(QEMU_KBD, hid_input, 0), 8);
        assert_eq!(hid_report_size(QEMU_KBD, hid_output, 0), 1);
        assert_eq!(hid_report_size(QEMU_KBD, hid_feature, 0), 0);
        // no such report ID
        assert_eq!(hid_report_size(QEMU_KBD, hid_input, 3), 0);
    }

    #[test]
    fn qemu_kbd_locate_modifiers_and_keys() {
        let mut loc = HidLocation::default();
        let mut flags = 0;
        // the left shift modifier is bit 1 of the first byte
        assert!(hid_locate(
            QEMU_KBD,
            hid_usage2(HUP_KEYBOARD, 0xe1),
            0,
            hid_input,
            Some(&mut loc),
            Some(&mut flags)
        ));
        assert_eq!(
            loc,
            HidLocation {
                size: 1,
                count: 1,
                pos: 1
            }
        );
        assert_eq!(flags, HIO_VARIABLE);
        // the key array is found by the first usage of its range
        assert!(hid_locate(
            QEMU_KBD,
            hid_usage2(HUP_KEYBOARD, 0),
            0,
            hid_input,
            Some(&mut loc),
            None
        ));
        assert_eq!(
            loc,
            HidLocation {
                size: 8,
                count: 6,
                pos: 16
            }
        );
        // LEDs
        assert!(hid_locate(
            QEMU_KBD,
            hid_usage2(HUP_LED, HUL_CAPS_LOCK),
            0,
            hid_output,
            Some(&mut loc),
            None
        ));
        assert_eq!(
            loc,
            HidLocation {
                size: 1,
                count: 1,
                pos: 1
            }
        );
        // the constant padding is never located; a miss zeroes the size and the flags
        loc.size = 77;
        flags = 77;
        assert!(!hid_locate(
            QEMU_KBD,
            hid_usage2(HUP_LED, HUL_KANA + 1),
            0,
            hid_output,
            Some(&mut loc),
            Some(&mut flags)
        ));
        assert_eq!(loc.size, 0);
        assert_eq!(flags, 0);
        // wrong kind, wrong report ID
        assert!(!hid_locate(
            QEMU_KBD,
            hid_usage2(HUP_LED, HUL_NUM_LOCK),
            0,
            hid_input,
            None,
            None
        ));
        assert!(!hid_locate(
            QEMU_KBD,
            hid_usage2(HUP_LED, HUL_NUM_LOCK),
            1,
            hid_output,
            None,
            None
        ));
    }

    #[test]
    fn qemu_kbd_is_a_keyboard_collection() {
        assert!(hid_is_collection(
            QEMU_KBD,
            0,
            hid_usage2(HUP_GENERIC_DESKTOP, HUG_KEYBOARD) as i32
        ));
        assert!(!hid_is_collection(
            QEMU_KBD,
            0,
            hid_usage2(HUP_GENERIC_DESKTOP, HUG_MOUSE) as i32
        ));
        assert!(!hid_is_collection(
            QEMU_KBD,
            1,
            hid_usage2(HUP_GENERIC_DESKTOP, HUG_KEYBOARD) as i32
        ));
    }

    #[test]
    fn boot_mouse_locations() {
        let mut loc = HidLocation::default();
        assert!(hid_locate(
            BOOT_MOUSE,
            hid_usage2(HUP_GENERIC_DESKTOP, HUG_X),
            0,
            hid_input,
            Some(&mut loc),
            None
        ));
        assert_eq!(
            loc,
            HidLocation {
                size: 8,
                count: 1,
                pos: 8
            }
        );
        let mut yloc = HidLocation::default();
        let mut flags = 0;
        assert!(hid_locate(
            BOOT_MOUSE,
            hid_usage2(HUP_GENERIC_DESKTOP, HUG_Y),
            0,
            hid_input,
            Some(&mut yloc),
            Some(&mut flags)
        ));
        assert_eq!(
            yloc,
            HidLocation {
                size: 8,
                count: 1,
                pos: 16
            }
        );
        assert_eq!(flags, HIO_VARIABLE | HIO_RELATIVE);
        let mut b = [HidLocation::default(); 3];
        for (i, l) in b.iter_mut().enumerate() {
            assert!(hid_locate(
                BOOT_MOUSE,
                hid_usage2(HUP_BUTTON, 1 + i as u32),
                0,
                hid_input,
                Some(l),
                None
            ));
            assert_eq!(
                *l,
                HidLocation {
                    size: 1,
                    count: 1,
                    pos: i as u32
                }
            );
        }
        assert_eq!(hid_report_size(BOOT_MOUSE, hid_input, 0), 3);
        assert!(hid_is_collection(
            BOOT_MOUSE,
            0,
            hid_usage2(HUP_GENERIC_DESKTOP, HUG_MOUSE) as i32
        ));
    }

    #[test]
    fn boot_mouse_data_is_signed() {
        let xloc = HidLocation {
            size: 8,
            count: 1,
            pos: 8,
        };
        let yloc = HidLocation {
            size: 8,
            count: 1,
            pos: 16,
        };
        let button = HidLocation {
            size: 1,
            count: 1,
            pos: 2,
        };
        let report = [0x05u8, 0xfe, 0x7f];
        assert_eq!(hid_get_data(&report, &xloc), -2);
        assert_eq!(hid_get_data(&report, &yloc), 127);
        assert_eq!(hid_get_udata(&report, &xloc), 0xfe);
        assert_eq!(
            hid_get_data(&report, &button),
            -1,
            "a set signed bit extends to -1"
        );
        assert_eq!(hid_get_udata(&report, &button), 1);
    }

    #[test]
    fn get_data_crosses_bytes_and_clamps() {
        // 12 bits at bit 4: low nibble of byte 0 is skipped, the data is 0x321 for 0x12 0x34 0x56?
        let report = [0x10u8, 0x32, 0x54, 0x76];
        let l = HidLocation {
            size: 12,
            count: 1,
            pos: 4,
        };
        assert_eq!(hid_get_udata(&report, &l), 0x321);
        // sign extension of a 12-bit field
        let report = [0x00u8, 0xf8, 0xff];
        let l = HidLocation {
            size: 12,
            count: 1,
            pos: 4,
        };
        assert_eq!(hid_get_data(&report, &l), -128);
        // a field wider than 32 bits is cut to 32
        let report = [0x78u8, 0x56, 0x34, 0x12, 0xff, 0xff];
        let l = HidLocation {
            size: 48,
            count: 1,
            pos: 0,
        };
        assert_eq!(hid_get_udata(&report, &l), 0x1234_5678);
        // a zero-size field is 0
        assert_eq!(hid_get_udata(&report, &HidLocation::default()), 0);
        // bytes past the end of the report read as 0
        let l = HidLocation {
            size: 16,
            count: 1,
            pos: 8,
        };
        assert_eq!(hid_get_udata(&[0xaa, 0xbb], &l), 0xbb);
        assert_eq!(hid_get_udata(&[], &l), 0);
        // a 32-bit field at an unaligned position loses its top bits, as in the C
        let l = HidLocation {
            size: 32,
            count: 1,
            pos: 4,
        };
        assert_eq!(hid_get_udata(&[0x10, 0, 0, 0x80, 0xff], &l), 0x0800_0001);
    }

    /// Two reports: ID 1 has a 16-bit variable input and an 8-bit output, ID 2 a 24-bit input;
    /// positions are tracked per ID.
    const REPORT_IDS: &[u8] = &[
        0x05, 0x01, 0x09, 0x04, 0xa1,
        0x01, // Generic Desktop, Joystick, Application collection
        0x85, 0x01, // Report ID 1
        0x09, 0x30, 0x75, 0x10, 0x95, 0x01, 0x81, 0x02, // X: 16 bits
        0x05, 0x08, 0x09, 0x01, 0x75, 0x08, 0x95, 0x01, 0x91, 0x02, // an LED: 8 bits out
        0x85, 0x02, // Report ID 2
        0x05, 0x01, 0x09, 0x31, 0x75, 0x18, 0x95, 0x01, 0x81, 0x02, // Y: 24 bits
        0x85, 0x01, // back to ID 1
        0x05, 0x01, 0x09, 0x32, 0x75, 0x08, 0x95, 0x01, 0x81, 0x02, // Z: 8 bits, follows X
        0xc0,
    ];

    #[test]
    fn report_ids_keep_their_own_positions() {
        let mut loc = HidLocation::default();
        assert!(hid_locate(
            REPORT_IDS,
            hid_usage2(HUP_GENERIC_DESKTOP, HUG_X),
            1,
            hid_input,
            Some(&mut loc),
            None
        ));
        assert_eq!(
            loc,
            HidLocation {
                size: 16,
                count: 1,
                pos: 0
            }
        );
        assert!(hid_locate(
            REPORT_IDS,
            hid_usage2(HUP_GENERIC_DESKTOP, HUG_Y),
            2,
            hid_input,
            Some(&mut loc),
            None
        ));
        assert_eq!(
            loc,
            HidLocation {
                size: 24,
                count: 1,
                pos: 0
            }
        );
        assert!(hid_locate(
            REPORT_IDS,
            hid_usage2(HUP_GENERIC_DESKTOP, HUG_Z),
            1,
            hid_input,
            Some(&mut loc),
            None
        ));
        assert_eq!(
            loc,
            HidLocation {
                size: 8,
                count: 1,
                pos: 16
            },
            "resumes after X"
        );
        // an item is only found under its own report ID
        assert!(!hid_locate(
            REPORT_IDS,
            hid_usage2(HUP_GENERIC_DESKTOP, HUG_Y),
            1,
            hid_input,
            None,
            None
        ));
        assert_eq!(hid_report_size(REPORT_IDS, hid_input, 1), 3);
        assert_eq!(hid_report_size(REPORT_IDS, hid_input, 2), 3);
        assert_eq!(hid_report_size(REPORT_IDS, hid_output, 1), 1);
        assert_eq!(hid_report_size(REPORT_IDS, hid_output, 2), 0);
        assert_eq!(
            hid_get_id_of_collection(
                REPORT_IDS,
                hid_usage2(HUP_GENERIC_DESKTOP, HUG_JOYSTICK),
                HCOLL_APPLICATION
            ),
            Some(0),
            "the collection opens before any report ID"
        );
        assert_eq!(
            hid_get_id_of_collection(
                REPORT_IDS,
                hid_usage2(HUP_GENERIC_DESKTOP, HUG_MOUSE),
                HCOLL_APPLICATION
            ),
            None
        );
    }

    #[test]
    fn more_report_ids_than_slots_restart_at_zero() {
        // 20 distinct IDs: the table holds MAXID - 1 non-zero ones, the rest restart at pos 0
        let mut d: Vec<u8> = std::vec![0x05, 0x01, 0x09, 0x04, 0xa1, 0x01, 0x75, 0x08, 0x95, 0x01];
        for id in 1..=20u8 {
            d.extend_from_slice(&[0x85, id, 0x09, 0x30, 0x81, 0x02]);
        }
        d.push(0xc0);
        let v = items(&d, hid_input);
        assert_eq!(v.len(), 20);
        assert!(v.iter().all(|h| h.loc.pos == 0 && h.loc.size == 8));
    }

    #[test]
    fn find_report_by_usages_and_collections() {
        let x = hid_usage2(HUP_GENERIC_DESKTOP, HUG_X) as i32;
        let z = hid_usage2(HUP_GENERIC_DESKTOP, HUG_Z) as i32;
        let y = hid_usage2(HUP_GENERIC_DESKTOP, HUG_Y) as i32;
        let app = hid_usage2(HUP_GENERIC_DESKTOP, HUG_JOYSTICK) as i32;
        assert_eq!(
            hid_find_report(REPORT_IDS, hid_input, app, &[x], None),
            Some(1)
        );
        // Z comes after report 2 interleaved: the matches restart with every change of ID, as
        // in the C, which expects the fields of a report to be together
        assert_eq!(
            hid_find_report(REPORT_IDS, hid_input, app, &[x, z], None),
            None
        );
        let m = hid_usage2(HUP_GENERIC_DESKTOP, HUG_MOUSE) as i32;
        assert_eq!(
            hid_find_report(BOOT_MOUSE, hid_input, m, &[x, y], None),
            Some(0)
        );
        assert_eq!(
            hid_find_report(BOOT_MOUSE, hid_input, m, &[y, x], None),
            Some(0)
        );
        assert_eq!(
            hid_find_report(REPORT_IDS, hid_input, app, &[y], None),
            Some(2)
        );
        // X and Y are in different reports
        assert_eq!(
            hid_find_report(REPORT_IDS, hid_input, app, &[x, y], None),
            None
        );
        // wrong application collection
        let mouse = hid_usage2(HUP_GENERIC_DESKTOP, HUG_MOUSE) as i32;
        assert_eq!(
            hid_find_report(REPORT_IDS, hid_input, mouse, &[x], None),
            None
        );
        // wrong kind
        assert_eq!(
            hid_find_report(REPORT_IDS, hid_output, app, &[x], None),
            None
        );
        // a single report without an ID answers 0
        let btn1 = hid_usage2(HUP_BUTTON, 1) as i32;
        let m = hid_usage2(HUP_GENERIC_DESKTOP, HUG_MOUSE) as i32;
        assert_eq!(
            hid_find_report(BOOT_MOUSE, hid_input, m, &[btn1], None),
            Some(0)
        );
        // the mouse's pointer collection is physical; with a collection list that omits it the
        // buttons inside are skipped, with one that names it they are found
        let ptr = hid_usage2(HUP_GENERIC_DESKTOP, HUG_POINTER) as i32;
        assert_eq!(
            hid_find_report(BOOT_MOUSE, hid_input, m, &[btn1], Some(&[0])),
            None
        );
        assert_eq!(
            hid_find_report(BOOT_MOUSE, hid_input, m, &[btn1], Some(&[ptr, 0])),
            Some(0)
        );
    }

    #[test]
    fn find_report_skips_vendor_collections() {
        // Application (Generic Desktop, Mouse) { Collection (vendor page 0xff00) { Input (X) } }
        let d: &[u8] = &[
            0x05, 0x01, 0x09, 0x02, 0xa1, 0x01, 0x06, 0x00, 0xff, 0x09, 0x01, 0xa1, 0x00, 0x05,
            0x01, 0x09, 0x30, 0x75, 0x08, 0x95, 0x01, 0x81, 0x02, 0xc0, 0xc0,
        ];
        let m = hid_usage2(HUP_GENERIC_DESKTOP, HUG_MOUSE) as i32;
        let x = hid_usage2(HUP_GENERIC_DESKTOP, HUG_X) as i32;
        assert_eq!(hid_find_report(d, hid_input, m, &[x], None), None);
        let vend = hid_usage2(0xff00, 1) as i32;
        assert_eq!(
            hid_find_report(d, hid_input, m, &[x], Some(&[vend, 0])),
            Some(0)
        );
    }

    #[test]
    fn push_and_pop_restore_the_globals() {
        // Report size 8, count 1; Push; size 16; Input; Pop; Input again uses size 8.
        let d: &[u8] = &[
            0x05, 0x01, 0x09, 0x04, 0xa1, 0x01, 0x75, 0x08, 0x95, 0x01, 0xa4, // Push
            0x75, 0x10, 0x09, 0x30, 0x81, 0x02, 0xb4, // Pop
            0x09, 0x31, 0x81, 0x02, 0xc0,
        ];
        let v = items(d, hid_input);
        assert_eq!(v.len(), 2);
        assert_eq!(
            v[0].loc,
            HidLocation {
                size: 16,
                count: 1,
                pos: 0
            }
        );
        assert_eq!(
            v[1].loc,
            HidLocation {
                size: 8,
                count: 1,
                pos: 16
            }
        );
    }

    #[test]
    fn more_than_maxpush_pushes_are_ignored() {
        let mut d: Vec<u8> = std::vec![0x75, 0x08, 0x95, 0x01];
        d.extend(std::iter::repeat_n(0xa4u8, 10));
        d.extend_from_slice(&[0x09, 0x30, 0x81, 0x02]);
        d.extend(std::iter::repeat_n(0xb4u8, 10));
        d.extend_from_slice(&[0x09, 0x31, 0x81, 0x02]);
        let v = items(&d, hid_input);
        assert_eq!(v.len(), 2);
    }

    #[test]
    fn usage_ranges_and_usage_lists_distribute_over_a_count() {
        // Usage (X), Usage (Y), Report Count 4: the third and fourth items repeat the last usage
        let d: &[u8] = &[
            0x05, 0x01, 0x09, 0x30, 0x09, 0x31, 0x75, 0x08, 0x95, 0x04, 0x81, 0x02,
        ];
        let v = items(d, hid_input);
        let u: Vec<u32> = v.iter().map(|h| h.usage).collect();
        let (x, y) = (
            hid_usage2(HUP_GENERIC_DESKTOP, HUG_X),
            hid_usage2(HUP_GENERIC_DESKTOP, HUG_Y),
        );
        assert_eq!(u, std::vec![x, y, y, y]);
        let p: Vec<u32> = v.iter().map(|h| h.loc.pos).collect();
        assert_eq!(p, std::vec![0, 8, 16, 24]);
        // minimum without a maximum is dropped
        let d: &[u8] = &[0x05, 0x09, 0x19, 0x01, 0x75, 0x01, 0x95, 0x02, 0x81, 0x02];
        let v = items(d, hid_input);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].usage, 0);
        // an inverted range is dropped as well
        let d: &[u8] = &[
            0x05, 0x09, 0x19, 0x05, 0x29, 0x01, 0x75, 0x01, 0x95, 0x02, 0x81, 0x02,
        ];
        let v = items(d, hid_input);
        assert_eq!(v[0].usage, 0);
    }

    #[test]
    fn full_width_usages_ignore_the_page() {
        // a 4-byte usage item carries its own page: 0x000c0001 (Consumer Control)
        let d: &[u8] = &[
            0x05, 0x01, 0x0b, 0x01, 0x00, 0x0c, 0x00, 0x75, 0x08, 0x95, 0x01, 0x81, 0x02,
        ];
        let v = items(d, hid_input);
        assert_eq!(v[0].usage, hid_usage2(HUP_CONSUMER, HUC_CONTROL));
    }

    #[test]
    fn long_items_are_skipped() {
        // 0xfe: long item, 3 bytes of data, tag 0x10; then a normal Input
        let d: &[u8] = &[
            0x05, 0x01, 0x09, 0x30, 0x75, 0x08, 0x95, 0x01, 0xfe, 0x03, 0x00, 0x10, 0xaa, 0xbb,
            0xcc, 0x81, 0x02,
        ];
        let v = items(d, hid_input);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].usage, hid_usage2(HUP_GENERIC_DESKTOP, HUG_X));
    }

    #[test]
    fn a_variable_count_is_capped() {
        // 5000 one-bit variable items are cut to MAXLOCCNT
        let d: &[u8] = &[0x75, 0x01, 0x96, 0x88, 0x13, 0x81, 0x02];
        assert_eq!(items(d, hid_input).len(), MAXLOCCNT as usize);
    }

    #[test]
    fn an_unbalanced_end_collection_ends_the_parse() {
        let d: &[u8] = &[
            0x75, 0x08, 0x95, 0x01, 0x09, 0x30, 0x81, 0x02, 0xc0, 0x09, 0x31, 0x81, 0x02,
        ];
        assert_eq!(items(d, hid_input).len(), 1);
    }

    #[test]
    fn a_truncated_descriptor_ends_cleanly() {
        // the last item claims 4 data bytes and the descriptor ends after 1
        let d: &[u8] = &[0x75, 0x08, 0x95, 0x01, 0x09, 0x30, 0x81, 0x02, 0x17, 0x01];
        assert_eq!(items(d, hid_input).len(), 1);
        assert_eq!(items(&[], hid_all).len(), 0);
        assert_eq!(hid_report_size(&[], hid_input, 0), 0);
    }

    #[test]
    fn collection_data_resumes_after_the_collection() {
        let mut d = hid_get_collection_data(
            BOOT_MOUSE,
            hid_usage2(HUP_GENERIC_DESKTOP, HUG_POINTER) as i32,
            HCOLL_PHYSICAL,
        )
        .expect("the pointer collection is there");
        let mut h = HidItem::default();
        assert!(hid_get_item(&mut d, &mut h));
        assert_eq!(h.kind, hid_input);
        assert_eq!(h.usage, hid_usage2(HUP_BUTTON, 1));
        hid_end_parse(d);
        assert!(
            hid_get_collection_data(BOOT_MOUSE, hid_usage2(HUP_LED, 1) as i32, HCOLL_PHYSICAL)
                .is_none()
        );
    }

    #[test]
    fn usage_helpers_split_and_join() {
        let u = hid_usage2(HUP_GENERIC_DESKTOP, HUG_WHEEL);
        assert_eq!(u, 0x0001_0038);
        assert_eq!(hid_get_usage_page(u), HUP_GENERIC_DESKTOP);
        assert_eq!(hid_get_usage(u), HUG_WHEEL);
    }

    /// Every constant of `<dev/hid/hid.h>` against the C header.
    #[test]
    #[ignore = "needs OPENBSD_SRC"]
    fn constants_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/hid/hid.h");
        let names = crate::reftest::assert_defines!(defs;
            HUP_UNDEFINED,
            HUP_GENERIC_DESKTOP,
            HUP_SIMULATION,
            HUP_VR_CONTROLS,
            HUP_SPORTS_CONTROLS,
            HUP_GAMING_CONTROLS,
            HUP_KEYBOARD,
            HUP_LED,
            HUP_BUTTON,
            HUP_ORDINALS,
            HUP_TELEPHONY,
            HUP_CONSUMER,
            HUP_DIGITIZERS,
            HUP_PHYSICAL_IFACE,
            HUP_UNICODE,
            HUP_ALPHANUM_DISPLAY,
            HUP_MONITOR,
            HUP_MONITOR_ENUM_VAL,
            HUP_VESA_VC,
            HUP_VESA_CMD,
            HUP_POWER,
            HUP_BATTERY,
            HUP_BARCODE_SCANNER,
            HUP_SCALE,
            HUP_CAMERA_CONTROL,
            HUP_ARCADE,
            HUP_VENDOR,
            HUP_FIDO,
            HUP_MICROSOFT,
            HUP_APPLE,
            HUP_WACOM,
            HUP_INAME,
            HUP_PRESENT_STATUS,
            HUP_CHANGED_STATUS,
            HUP_UPS,
            HUP_POWER_SUPPLY,
            HUP_BATTERY_SYSTEM,
            HUP_BATTERY_SYSTEM_ID,
            HUP_PD_BATTERY,
            HUP_BATTERY_ID,
            HUP_CHARGER,
            HUP_CHARGER_ID,
            HUP_POWER_CONVERTER,
            HUP_POWER_CONVERTER_ID,
            HUP_OUTLET_SYSTEM,
            HUP_OUTLET_SYSTEM_ID,
            HUP_INPUT,
            HUP_INPUT_ID,
            HUP_OUTPUT,
            HUP_OUTPUT_ID,
            HUP_FLOW,
            HUP_FLOW_ID,
            HUP_OUTLET,
            HUP_OUTLET_ID,
            HUP_GANG,
            HUP_GANG_ID,
            HUP_POWER_SUMMARY,
            HUP_POWER_SUMMARY_ID,
            HUP_VOLTAGE,
            HUP_CURRENT,
            HUP_FREQUENCY,
            HUP_APPARENT_POWER,
            HUP_ACTIVE_POWER,
            HUP_PERCENT_LOAD,
            HUP_TEMPERATURE,
            HUP_HUMIDITY,
            HUP_BADCOUNT,
            HUP_CONFIG_VOLTAGE,
            HUP_CONFIG_CURRENT,
            HUP_CONFIG_FREQUENCY,
            HUP_CONFIG_APP_POWER,
            HUP_CONFIG_ACT_POWER,
            HUP_CONFIG_PERCENT_LOAD,
            HUP_CONFIG_TEMPERATURE,
            HUP_CONFIG_HUMIDITY,
            HUP_SWITCHON_CONTROL,
            HUP_SWITCHOFF_CONTROL,
            HUP_TOGGLE_CONTROL,
            HUP_LOW_VOLT_TRANSF,
            HUP_HIGH_VOLT_TRANSF,
            HUP_DELAYBEFORE_REBOOT,
            HUP_DELAYBEFORE_STARTUP,
            HUP_DELAYBEFORE_SHUTDWN,
            HUP_TEST,
            HUP_MODULE_RESET,
            HUP_AUDIBLE_ALRM_CTL,
            HUP_PRESENT,
            HUP_GOOD,
            HUP_INTERNAL_FAILURE,
            HUP_PD_VOLT_OUTOF_RANGE,
            HUP_FREQ_OUTOFRANGE,
            HUP_OVERLOAD,
            HUP_OVERCHARGED,
            HUP_OVERTEMPERATURE,
            HUP_SHUTDOWN_REQUESTED,
            HUP_SHUTDOWN_IMMINENT,
            HUP_SWITCH_ON_OFF,
            HUP_SWITCHABLE,
            HUP_USED,
            HUP_BOOST,
            HUP_BUCK,
            HUP_INITIALIZED,
            HUP_TESTED,
            HUP_AWAITING_POWER,
            HUP_COMMUNICATION_LOST,
            HUP_IMANUFACTURER,
            HUP_IPRODUCT,
            HUP_ISERIALNUMBER,
            HUB_SMB_BATTERY_MODE,
            HUB_SMB_BATTERY_STATUS,
            HUB_SMB_ALARM_WARNING,
            HUB_SMB_CHARGER_MODE,
            HUB_SMB_CHARGER_STATUS,
            HUB_SMB_CHARGER_SPECINF,
            HUB_SMB_SELECTR_STATE,
            HUB_SMB_SELECTR_PRESETS,
            HUB_SMB_SELECTR_INFO,
            HUB_SMB_OPT_MFGFUNC1,
            HUB_SMB_OPT_MFGFUNC2,
            HUB_SMB_OPT_MFGFUNC3,
            HUB_SMB_OPT_MFGFUNC4,
            HUB_SMB_OPT_MFGFUNC5,
            HUB_CONNECTIONTOSMBUS,
            HUB_OUTPUT_CONNECTION,
            HUB_CHARGER_CONNECTION,
            HUB_BATTERY_INSERTION,
            HUB_USENEXT,
            HUB_OKTOUSE,
            HUB_BATTERY_SUPPORTED,
            HUB_SELECTOR_REVISION,
            HUB_CHARGING_INDICATOR,
            HUB_MANUFACTURER_ACCESS,
            HUB_REM_CAPACITY_LIM,
            HUB_REM_TIME_LIM,
            HUB_ATRATE,
            HUB_CAPACITY_MODE,
            HUB_BCAST_TO_CHARGER,
            HUB_PRIMARY_BATTERY,
            HUB_CHANGE_CONTROLLER,
            HUB_TERMINATE_CHARGE,
            HUB_TERMINATE_DISCHARGE,
            HUB_BELOW_REM_CAP_LIM,
            HUB_REM_TIME_LIM_EXP,
            HUB_CHARGING,
            HUB_DISCHARGING,
            HUB_FULLY_CHARGED,
            HUB_FULLY_DISCHARGED,
            HUB_CONDITIONING_FLAG,
            HUB_ATRATE_OK,
            HUB_SMB_ERROR_CODE,
            HUB_NEED_REPLACEMENT,
            HUB_ATRATE_TIMETOFULL,
            HUB_ATRATE_TIMETOEMPTY,
            HUB_AVERAGE_CURRENT,
            HUB_MAXERROR,
            HUB_REL_STATEOF_CHARGE,
            HUB_ABS_STATEOF_CHARGE,
            HUB_REM_CAPACITY,
            HUB_FULLCHARGE_CAPACITY,
            HUB_RUNTIMETO_EMPTY,
            HUB_AVERAGETIMETO_EMPTY,
            HUB_AVERAGETIMETO_FULL,
            HUB_CYCLECOUNT,
            HUB_BATTPACKMODEL_LEVEL,
            HUB_INTERNAL_CHARGE_CTL,
            HUB_PRIMARY_BATTERY_SUP,
            HUB_DESIGN_CAPACITY,
            HUB_SPECIFICATION_INFO,
            HUB_MANUFACTURER_DATE,
            HUB_SERIAL_NUMBER,
            HUB_IMANUFACTURERNAME,
            HUB_IDEVICENAME,
            HUB_IDEVICECHEMISTERY,
            HUB_MANUFACTURERDATA,
            HUB_RECHARGABLE,
            HUB_WARN_CAPACITY_LIM,
            HUB_CAPACITY_GRANUL1,
            HUB_CAPACITY_GRANUL2,
            HUB_IOEM_INFORMATION,
            HUB_INHIBIT_CHARGE,
            HUB_ENABLE_POLLING,
            HUB_RESTORE_TO_ZERO,
            HUB_AC_PRESENT,
            HUB_BATTERY_PRESENT,
            HUB_POWER_FAIL,
            HUB_ALARM_INHIBITED,
            HUB_THERMISTOR_UNDRANGE,
            HUB_THERMISTOR_HOT,
            HUB_THERMISTOR_COLD,
            HUB_THERMISTOR_OVERANGE,
            HUB_BS_VOLT_OUTOF_RANGE,
            HUB_BS_CURR_OUTOF_RANGE,
            HUB_BS_CURR_NOT_REGULTD,
            HUB_BS_VOLT_NOT_REGULTD,
            HUB_MASTER_MODE,
            HUB_CHARGER_SELECTR_SUP,
            HUB_CHARGER_SPEC,
            HUB_LEVEL2,
            HUB_LEVEL3,
            HUG_POINTER,
            HUG_MOUSE,
            HUG_FN_KEY,
            HUG_JOYSTICK,
            HUG_GAME_PAD,
            HUG_KEYBOARD,
            HUG_KEYPAD,
            HUG_X,
            HUG_Y,
            HUG_Z,
            HUG_RX,
            HUG_RY,
            HUG_RZ,
            HUG_SLIDER,
            HUG_DIAL,
            HUG_WHEEL,
            HUG_HAT_SWITCH,
            HUG_COUNTED_BUFFER,
            HUG_BYTE_COUNT,
            HUG_MOTION_WAKEUP,
            HUG_VX,
            HUG_VY,
            HUG_VZ,
            HUG_VBRX,
            HUG_VBRY,
            HUG_VBRZ,
            HUG_VNO,
            HUG_TWHEEL,
            HUG_SYSTEM_CONTROL,
            HUG_SYSTEM_POWER_DOWN,
            HUG_SYSTEM_SLEEP,
            HUG_SYSTEM_WAKEUP,
            HUG_SYSTEM_CONTEXT_MENU,
            HUG_SYSTEM_MAIN_MENU,
            HUG_SYSTEM_APP_MENU,
            HUG_SYSTEM_MENU_HELP,
            HUG_SYSTEM_MENU_EXIT,
            HUG_SYSTEM_MENU_SELECT,
            HUG_SYSTEM_MENU_RIGHT,
            HUG_SYSTEM_MENU_LEFT,
            HUG_SYSTEM_MENU_UP,
            HUG_SYSTEM_MENU_DOWN,
            HUD_UNDEFINED,
            HUD_DIGITIZER,
            HUD_PEN,
            HUD_TOUCHSCREEN,
            HUD_TOUCHPAD,
            HUD_CONFIG,
            HUD_STYLUS,
            HUD_FINGER,
            HUD_TIP_PRESSURE,
            HUD_BARREL_PRESSURE,
            HUD_IN_RANGE,
            HUD_TOUCH,
            HUD_UNTOUCH,
            HUD_TAP,
            HUD_QUALITY,
            HUD_DATA_VALID,
            HUD_TRANSDUCER_INDEX,
            HUD_TABLET_FKEYS,
            HUD_PROGRAM_CHANGE_KEYS,
            HUD_BATTERY_STRENGTH,
            HUD_INVERT,
            HUD_X_TILT,
            HUD_Y_TILT,
            HUD_AZIMUTH,
            HUD_ALTITUDE,
            HUD_TWIST,
            HUD_TIP_SWITCH,
            HUD_SEC_TIP_SWITCH,
            HUD_BARREL_SWITCH,
            HUD_ERASER,
            HUD_TABLET_PICK,
            HUD_CONFIDENCE,
            HUD_WIDTH,
            HUD_HEIGHT,
            HUD_CONTACTID,
            HUD_INPUT_MODE,
            HUD_DEVICE_INDEX,
            HUD_CONTACTCOUNT,
            HUD_CONTACT_MAX,
            HUD_SCAN_TIME,
            HUD_BUTTON_TYPE,
            HUD_SECONDARY_BARREL_SWITCH,
            HUD_WACOM_X,
            HUD_WACOM_Y,
            HUD_WACOM_DISTANCE,
            HUD_WACOM_PAD_BUTTONS00,
            HUD_WACOM_BATTERY,
            HUL_NUM_LOCK,
            HUL_CAPS_LOCK,
            HUL_SCROLL_LOCK,
            HUL_COMPOSE,
            HUL_KANA,
            HUC_CONTROL,
            HUC_TRACK_NEXT,
            HUC_TRACK_PREV,
            HUC_STOP,
            HUC_PLAY_PAUSE,
            HUC_VOLUME,
            HUC_MUTE,
            HUC_VOL_INC,
            HUC_VOL_DEC,
            HUC_AC_PAN,
            HUF_U2FHID,
            HUF_RAW_IN_DATA_REPORT,
            HUF_RAW_OUT_DATA_REPORT,
            HCOLL_PHYSICAL,
            HCOLL_APPLICATION,
            HCOLL_LOGICAL,
            HIO_CONST,
            HIO_VARIABLE,
            HIO_RELATIVE,
            HIO_WRAP,
            HIO_NONLINEAR,
            HIO_NOPREF,
            HIO_NULLSTATE,
            HIO_VOLATILE,
            HIO_BUFBYTES,
            HCC_UNDEFINED,
            HCC_MAX,
        );
        crate::reftest::assert_complete(&defs, "H", &names);
    }
}
/* </TESTS> */
