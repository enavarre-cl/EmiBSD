/*	$OpenBSD: hid.c,v 1.10 2025/11/03 01:41:22 jmatthew Exp $ */
/*	$NetBSD: hid.c,v 1.23 2002/07/11 21:14:25 augustss Exp $	*/
/*	$FreeBSD: src/sys/dev/usb/hid.c,v 1.11 1999/11/17 22:33:39 n_hibma Exp $ */
/*	$OpenBSD: hid.h,v 1.13 2025/10/28 15:36:46 jcs Exp $ */
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

#[cfg(test)]
mod tests;
