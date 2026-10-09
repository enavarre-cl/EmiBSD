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
 * Copyright (c) 2015 Masao Uebayashi
 * Copyright (c) 2005 Jordan Hargrave
 * All rights reserved.
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
 * THIS SOFTWARE IS PROVIDED BY THE AUTHORS AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED. IN NO EVENT SHALL THE AUTHORS OR CONTRIBUTORS BE LIABLE FOR
 * ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 */
/*-
 * Copyright (c) 2006 IronPort Systems Inc. <ambrisko@ironport.com>
 * All rights reserved.
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
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 * $FreeBSD$
 */
/* </LICENSES> */

/* <CODE> */
//! ipmi(4): `dev/ipmi.c` with its user interface `dev/ipmi.h`. The Intelligent Platform
//! Management Interface driver talks to the machine's baseboard management controller
//! (BMC) over one of three system interfaces, polled: KCS (two byte-wide registers with a
//! handshake per byte), SMIC (three registers, a control-code protocol) and BT (a block
//! transfer with a sequence number). On attach it starts a thread that reads the BMC's
//! sensor data records (SDRs), turns the temperature, voltage, current, fan, power supply
//! and intrusion sensors into `sensors(9)` (`hw.sensors.ipmiN`) and rereads one of them every
//! `SENSOR_REFRESH_RATE` seconds. It registers the BMC's watchdog timer with
//! `kern_watchdog.c` (`kern.watchdog.period`), and serves `/dev/ipmi` (character major 96):
//! one request at a time, sent synchronously by `IPMICTL_SEND_COMMAND` and fetched by
//! `IPMICTL_RECEIVE_MSG`. Once the system is up, commands run on a task queue of their own,
//! one at a time; cold, they are polled where they are issued.
//!
//! Upstream: sys/dev/ipmi.c @ 3ce1f3f79392, sys/dev/ipmi.h @ 3ce1f3f79392
//!
//! The ACPI attachment is `dev/acpi/ipmi_acpi.rs`; on amd64 the mainbus one is here
//! (`ipmi_match`, `ipmi_attach`, `ipmi_probe`), as in the C's `#if defined(__amd64__)`
//! part. `ipmi.h` has no `$OpenBSD$` line; its licence block is the second one above.
//!
//! BT keeps the C's offsets: `bt_recvmsg` drops the length and sequence bytes from the
//! buffer, but `IPMI_BTMSG_DATARCV` (5) still counts them, so `ipmi_recvcmd` hands back a BT
//! response's data from its third byte on (a host test pins this down).
//!
//! ## Deviations
//! - The interface layer is generic over `ipmivar.rs`'s `IpmiBmc` (the softc's registers,
//!   buffer and interface), so that host tests drive the KCS, SMIC and BT state machines,
//!   the SDR reading and the sensors against a simulated BMC; the kernel instantiates it for
//!   `IpmiSoftc` only. A command does not carry its softc: the functions that took a `struct
//!   ipmi_cmd *` take the softc beside it, and the interface functions also take the
//!   transfer buffer (`IpmiXfer`, borrowed once per command from the softc's `RefCell`; a
//!   second, concurrent command fails as a send error instead of sharing the buffer).
//! - `ipmi_cmd_wait` sleeps with `msleep_nsec` on a flag the task sets under a mutex: the C
//!   sleeps with `tsleep_nsec` after queueing the task, so a command that finished first
//!   would never wake it. Without a command task queue (`taskq_create` failed, where the C
//!   would dereference NULL) the command is polled.
//! - Buffers are slices and every copy is bounded by both ends: `kcs_recvmsg` and
//!   `bt_recvmsg` stop at the end of `sc_buf`, `smic_recvmsg` too (the C's loop has no
//!   bound), `ipmi_recvcmd` copies at most `c_data`'s length, `get_sdr_partial` at most the
//!   caller's buffer, and a request longer than its `c_data` fails. `getbits` reads bits past
//!   a record's end as 0 (the 6-bit ASCII name decoder can ask for one more byte than the
//!   name has).
//! - SDR request and response fields are encoded little-endian explicitly (the wire order;
//!   the C stores them in host order through `u_int16_t *` casts, the same bytes on amd64
//!   and arm64).
//! - `IPMI_DEBUG` is not configured, so `dbg_printf` and `dbg_dump` compile to nothing as in
//!   C; `dumpb` is kept. The `ipmi_request`, `ipmi_response`, `ipmi_bmc_request` and
//!   `ipmi_bmc_response` structures and the `#if 0` IPMB code of `ipmi_sendcmd` are not
//!   used by the C and are left out.
//! - `ipmi_attach_common` stops with a message when the interface type is unknown or the
//!   registers cannot be mapped (the C goes on and dereferences the NULL interface).
//! - `ipmi_map_regs`'s failure message prints the base and size, not the tag and the
//!   handle's address.
//! - `ipmilookup` returns the softc; like the C, `ipmiopen`, `ipmiclose` and `ipmiioctl`
//!   never give back the device reference `device_lookup` took (`device_unref`).
//! - `ipmi_probe` (amd64's mainbus) exists where cfg `machine_x86` is set, as the C's
//!   `#if defined(__amd64__) || defined(__i386__)` part: it reads SMBIOS's IPMI device
//!   information with `bios.c`'s `smbios_find_table` through `machine::x86`. `struct
//!   smbios_ipmi` and `SMBIOS_TYPE_IPMIDEV` (`<machine/smbiosvar.h>`) are declared here too,
//!   because `ipmi_smbios_probe` and its host tests are built on every machine; the record is
//!   read as this file's `SmbiosIpmi` (the same packed layout). Without SMBIOS the probe goes
//!   on to the `IPMI` signature scan of the BIOS area; `scan_sig` maps that area through
//!   `bus_space_map` of the attach arguments' memory tag instead of `ISA_HOLE_VADDR`, and
//!   returns the signature's physical address.
//! - `ipmi_sensor_list` is a `Sync` wrapper over an `SlistHead`, touched by the sensor
//!   thread only (and by autoconfiguration's thread before it runs); `ipmi_enabled` and
//!   `maxsdrlen` are atomics.

use core::cell::RefCell;
use core::ffi::c_void;
use core::mem::offset_of;
use core::ptr;
use core::sync::atomic::{AtomicBool, AtomicI32, Ordering};

use crate::dev::ipmivar::{
    APP_GET_DEVICE_ID, APP_GET_WATCHDOG_TIMER, APP_NETFN, APP_RESET_WATCHDOG,
    APP_SET_WATCHDOG_TIMER, IPMI_GET_WDOG_MAX, IPMI_IF_BT, IPMI_IF_BT_NREGS, IPMI_IF_KCS,
    IPMI_IF_KCS_NREGS, IPMI_IF_SMIC, IPMI_IF_SMIC_NREGS, IPMI_MSG_CCODE, IPMI_MSG_CMD,
    IPMI_MSG_DATARCV, IPMI_MSG_DATASND, IPMI_MSG_NFLN, IPMI_SET_WDOG_ACTION, IPMI_SET_WDOG_MAX,
    IPMI_SET_WDOG_TIMER, IPMI_SET_WDOG_TIMOL, IPMI_WDOG_DISABLED, IPMI_WDOG_DONTSTOP,
    IPMI_WDOG_MASK, IPMI_WDOG_REBOOT, IpmiAttachArgs, IpmiBmc, IpmiCmd, IpmiIf, IpmiIoctlState,
    IpmiIowait, IpmiSoftc, IpmiThread, IpmiXfer, SDRTYPE1_NAME, SDRTYPE2_NAME,
    SE_GET_SENSOR_READING, SE_NETFN, STORAGE_GET_SDR, STORAGE_NETFN, STORAGE_RESERVE_SDR, Sdrhdr,
    Sdrtype1, Sdrtype2,
};
use crate::kern::kern_kthread::{kthread_create, kthread_create_deferred, kthread_exit};
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_rwlock::{rw_enter_write, rw_exit_write, rw_init};
use crate::kern::kern_sensors::{sensor_attach, sensordev_install};
use crate::kern::kern_synch::{msleep_nsec, tsleep_nsec, wakeup};
use crate::kern::kern_task::{task_add, task_add_local, task_del, task_set, taskq_create};
use crate::kern::kern_watchdog::{wdog_register, wdog_shutdown};
use crate::kern::subr_autoconf::device_lookup;
use crate::kern::subr_prf::{Str, panicstr, snprintf};
use crate::machine::bus::{
    BusAddr, BusSize, BusSpaceTag, bus_space_map, bus_space_read_1, bus_space_read_4,
    bus_space_unmap, bus_space_write_1, bus_space_write_4,
};
use crate::machine::copy::{copyin, copyout};
use crate::machine::intr::IPL_MPFLOOR;
use crate::machine::isa_machdep::{IST_EDGE, IST_LEVEL};
#[cfg(machine_x86)]
use crate::machine::x86::{Smbtable, smbios_find_table};
use crate::sys::device::{CfMatch, Cfattach, Cfdriver, DV_DULL, DVACT_POWERDOWN, Device};
use crate::sys::errno::Errno;
use crate::sys::ioccom::{_ior, _iow, _iowr};
use crate::sys::malloc::{M_DEVBUF, M_NOWAIT, M_TEMP, M_WAITOK, M_ZERO};
use crate::sys::mutex::Mutex;
use crate::sys::param::PWAIT;
use crate::sys::proc::Proc;
use crate::sys::queue::{SlistEntry, SlistHead};
use crate::sys::sensors::{
    Ksensor, Ksensordev, SENSOR_AMPS, SENSOR_FANRPM, SENSOR_FINVALID, SENSOR_INDICATOR,
    SENSOR_S_CRIT, SENSOR_S_OK, SENSOR_S_WARN, SENSOR_TEMP, SENSOR_VOLTS_AC, SENSOR_VOLTS_DC,
    SENSOR_WATTS, SensorStatus, SensorType,
};
use crate::sys::systm::{COLD, INFSLP};
use crate::sys::task::{SYSTQ, TASKQ_MPSAFE, Task};
use crate::sys::time::{msec_to_nsec, sec_to_nsec};
use crate::sys::types::{Dev, minor};
use crate::{kassert, kprintf};

// ---- ipmi.h ----

/// `IPMI_MAX_ADDR_SIZE`.
pub const IPMI_MAX_ADDR_SIZE: usize = 0x20;
/// `IPMI_MAX_RX`: the largest response.
pub const IPMI_MAX_RX: usize = 1024;
/// `IPMI_BMC_SLAVE_ADDR`: Linux default slave address.
pub const IPMI_BMC_SLAVE_ADDR: i32 = 0x20;
/// `IPMI_BMC_CHANNEL`: Linux BMC channel.
pub const IPMI_BMC_CHANNEL: i32 = 0x0f;

/// `IPMI_BMC_SMS_LUN`.
pub const IPMI_BMC_SMS_LUN: i32 = 0x02;

/// `IPMI_SYSTEM_INTERFACE_ADDR_TYPE`.
pub const IPMI_SYSTEM_INTERFACE_ADDR_TYPE: i32 = 0x0c;
/// `IPMI_IPMB_ADDR_TYPE`.
pub const IPMI_IPMB_ADDR_TYPE: i32 = 0x01;
/// `IPMI_IPMB_BROADCAST_ADDR_TYPE`.
pub const IPMI_IPMB_BROADCAST_ADDR_TYPE: i32 = 0x41;

/// `IPMI_IOC_MAGIC`.
pub const IPMI_IOC_MAGIC: u8 = b'i';
/// `IPMICTL_RECEIVE_MSG_TRUNC`.
pub const IPMICTL_RECEIVE_MSG_TRUNC: u64 = _iowr::<IpmiRecv>(IPMI_IOC_MAGIC, 11);
/// `IPMICTL_RECEIVE_MSG`.
pub const IPMICTL_RECEIVE_MSG: u64 = _iowr::<IpmiRecv>(IPMI_IOC_MAGIC, 12);
/// `IPMICTL_SEND_COMMAND`.
pub const IPMICTL_SEND_COMMAND: u64 = _iow::<IpmiReq>(IPMI_IOC_MAGIC, 13);
/// `IPMICTL_REGISTER_FOR_CMD`.
pub const IPMICTL_REGISTER_FOR_CMD: u64 = _iow::<IpmiCmdspec>(IPMI_IOC_MAGIC, 14);
/// `IPMICTL_UNREGISTER_FOR_CMD`.
pub const IPMICTL_UNREGISTER_FOR_CMD: u64 = _iow::<IpmiCmdspec>(IPMI_IOC_MAGIC, 15);
/// `IPMICTL_SET_GETS_EVENTS_CMD`.
pub const IPMICTL_SET_GETS_EVENTS_CMD: u64 = _iow::<i32>(IPMI_IOC_MAGIC, 16);
/// `IPMICTL_SET_MY_ADDRESS_CMD`.
pub const IPMICTL_SET_MY_ADDRESS_CMD: u64 = _iow::<u32>(IPMI_IOC_MAGIC, 17);
/// `IPMICTL_GET_MY_ADDRESS_CMD`.
pub const IPMICTL_GET_MY_ADDRESS_CMD: u64 = _ior::<u32>(IPMI_IOC_MAGIC, 18);
/// `IPMICTL_SET_MY_LUN_CMD`.
pub const IPMICTL_SET_MY_LUN_CMD: u64 = _iow::<u32>(IPMI_IOC_MAGIC, 19);
/// `IPMICTL_GET_MY_LUN_CMD`.
pub const IPMICTL_GET_MY_LUN_CMD: u64 = _ior::<u32>(IPMI_IOC_MAGIC, 20);

/// `IPMI_RESPONSE_RECV_TYPE`.
pub const IPMI_RESPONSE_RECV_TYPE: i32 = 1;
/// `IPMI_ASYNC_EVENT_RECV_TYPE`.
pub const IPMI_ASYNC_EVENT_RECV_TYPE: i32 = 2;
/// `IPMI_CMD_RECV_TYPE`.
pub const IPMI_CMD_RECV_TYPE: i32 = 3;

/// `IPMI_APP_REQUEST`.
pub const IPMI_APP_REQUEST: i32 = 0x06;
/// `IPMI_GET_DEVICE_ID`.
pub const IPMI_GET_DEVICE_ID: i32 = 0x01;
/// `IPMI_CLEAR_FLAGS`.
pub const IPMI_CLEAR_FLAGS: i32 = 0x30;
/// `IPMI_GET_MSG_FLAGS`.
pub const IPMI_GET_MSG_FLAGS: i32 = 0x31;
/// `IPMI_MSG_AVAILABLE`.
pub const IPMI_MSG_AVAILABLE: i32 = 0x01;
/// `IPMI_MSG_BUFFER_FULL`.
pub const IPMI_MSG_BUFFER_FULL: i32 = 0x02;
/// `IPMI_WDT_PRE_TIMEOUT`.
pub const IPMI_WDT_PRE_TIMEOUT: i32 = 0x08;
/// `IPMI_GET_MSG`.
pub const IPMI_GET_MSG: i32 = 0x33;
/// `IPMI_SEND_MSG`.
pub const IPMI_SEND_MSG: i32 = 0x34;
/// `IPMI_GET_CHANNEL_INFO`.
pub const IPMI_GET_CHANNEL_INFO: i32 = 0x42;
/// `IPMI_RESET_WDOG`.
pub const IPMI_RESET_WDOG: i32 = 0x22;
/// `IPMI_SET_WDOG`.
pub const IPMI_SET_WDOG: i32 = 0x24;
/// `IPMI_GET_WDOG`.
pub const IPMI_GET_WDOG: i32 = 0x25;

/// `IPMI_SET_WD_TIMER_SMS_OS`.
pub const IPMI_SET_WD_TIMER_SMS_OS: i32 = 0x04;
/// `IPMI_SET_WD_TIMER_DONT_STOP`.
pub const IPMI_SET_WD_TIMER_DONT_STOP: i32 = 0x40;
/// `IPMI_SET_WD_ACTION_RESET`.
pub const IPMI_SET_WD_ACTION_RESET: i32 = 0x01;

// ---- ipmi.c ----

/// `SENSOR_REFRESH_RATE`: seconds between two sensor reads.
const SENSOR_REFRESH_RATE: u64 = 5;

/// `IPMI_BTMSG_LEN`: BT message byte: the length.
const IPMI_BTMSG_LEN: usize = 0;
/// `IPMI_BTMSG_NFLN`.
const IPMI_BTMSG_NFLN: usize = 1;
/// `IPMI_BTMSG_SEQ`.
const IPMI_BTMSG_SEQ: usize = 2;
/// `IPMI_BTMSG_CMD`.
const IPMI_BTMSG_CMD: usize = 3;
/// `IPMI_BTMSG_CCODE`.
pub const IPMI_BTMSG_CCODE: usize = 4;
/// `IPMI_BTMSG_DATASND`.
const IPMI_BTMSG_DATASND: i32 = 4;
/// `IPMI_BTMSG_DATARCV`.
const IPMI_BTMSG_DATARCV: i32 = 5;

/// `IPMI_SENSOR_TYPE_TEMP`: IPMI 2.0, table 42-3, sensor type codes (event type << 8 | type).
const IPMI_SENSOR_TYPE_TEMP: i32 = 0x0101;
/// `IPMI_SENSOR_TYPE_VOLT`.
const IPMI_SENSOR_TYPE_VOLT: i32 = 0x0102;
/// `IPMI_SENSOR_TYPE_CURRENT`.
const IPMI_SENSOR_TYPE_CURRENT: i32 = 0x0103;
/// `IPMI_SENSOR_TYPE_FAN`.
const IPMI_SENSOR_TYPE_FAN: i32 = 0x0104;
/// `IPMI_SENSOR_TYPE_INTRUSION`.
const IPMI_SENSOR_TYPE_INTRUSION: i32 = 0x6F05;
/// `IPMI_SENSOR_TYPE_PWRSUPPLY`.
const IPMI_SENSOR_TYPE_PWRSUPPLY: i32 = 0x6F08;

/// `IPMI_UNIT_TYPE_DEGREE_C`: IPMI 2.0, table 43-15, sensor unit type codes.
pub const IPMI_UNIT_TYPE_DEGREE_C: i32 = 1;
/// `IPMI_UNIT_TYPE_DEGREE_F`.
pub const IPMI_UNIT_TYPE_DEGREE_F: i32 = 2;
/// `IPMI_UNIT_TYPE_DEGREE_K`.
pub const IPMI_UNIT_TYPE_DEGREE_K: i32 = 3;
/// `IPMI_UNIT_TYPE_VOLTS`.
const IPMI_UNIT_TYPE_VOLTS: i32 = 4;
/// `IPMI_UNIT_TYPE_AMPS`.
const IPMI_UNIT_TYPE_AMPS: i32 = 5;
/// `IPMI_UNIT_TYPE_WATTS`.
const IPMI_UNIT_TYPE_WATTS: i32 = 6;
/// `IPMI_UNIT_TYPE_RPM`.
const IPMI_UNIT_TYPE_RPM: i32 = 18;

/// `IPMI_NAME_UNICODE`: SDR name encodings (`typelen` bits 7-6).
const IPMI_NAME_UNICODE: u8 = 0x00;
/// `IPMI_NAME_BCDPLUS`.
const IPMI_NAME_BCDPLUS: u8 = 0x01;
/// `IPMI_NAME_ASCII6BIT`.
const IPMI_NAME_ASCII6BIT: u8 = 0x02;
/// `IPMI_NAME_ASCII8BIT`.
const IPMI_NAME_ASCII8BIT: u8 = 0x03;

/// `IPMI_ENTITY_PWRSUPPLY`.
const IPMI_ENTITY_PWRSUPPLY: i32 = 0x0A;

/// `IPMI_INVALID_SENSOR`: Get Sensor Reading byte 2: reading unavailable.
const IPMI_INVALID_SENSOR: u8 = 1 << 5;
/// `IPMI_DISABLED_SENSOR`: Get Sensor Reading byte 2: scanning enabled.
const IPMI_DISABLED_SENSOR: u8 = 1 << 6;

/// `IPMI_SDR_TYPEFULL`.
const IPMI_SDR_TYPEFULL: u8 = 1;
/// `IPMI_SDR_TYPECOMPACT`.
const IPMI_SDR_TYPECOMPACT: u8 = 2;

/// `RSSA_MASK`.
const RSSA_MASK: i32 = 0xff;
/// `LUN_MASK`.
const LUN_MASK: i32 = 0x3;

// BT interface registers.
/// `_BT_CTRL_REG`.
const _BT_CTRL_REG: i32 = 0;
/// `BT_CLR_WR_PTR`.
const BT_CLR_WR_PTR: u8 = 1 << 0;
/// `BT_CLR_RD_PTR`.
const BT_CLR_RD_PTR: u8 = 1 << 1;
/// `BT_HOST2BMC_ATN`.
const BT_HOST2BMC_ATN: u8 = 1 << 2;
/// `BT_BMC2HOST_ATN`.
const BT_BMC2HOST_ATN: u8 = 1 << 3;
/// `BT_EVT_ATN`.
pub const BT_EVT_ATN: u8 = 1 << 4;
/// `BT_HOST_BUSY`.
const BT_HOST_BUSY: u8 = 1 << 6;
/// `BT_BMC_BUSY`.
const BT_BMC_BUSY: u8 = 1 << 7;
/// `BT_READY`.
pub const BT_READY: u8 = BT_HOST_BUSY | BT_HOST2BMC_ATN | BT_BMC2HOST_ATN;
/// `_BT_DATAIN_REG`.
const _BT_DATAIN_REG: i32 = 1;
/// `_BT_DATAOUT_REG`.
const _BT_DATAOUT_REG: i32 = 1;
/// `_BT_INTMASK_REG`.
const _BT_INTMASK_REG: i32 = 2;
/// `BT_IM_HIRQ_PEND`.
const BT_IM_HIRQ_PEND: u8 = 1 << 1;
/// `BT_IM_SCI_EN`.
const BT_IM_SCI_EN: u8 = 1 << 2;
/// `BT_IM_SMI_EN`.
const BT_IM_SMI_EN: u8 = 1 << 3;
/// `BT_IM_NMI2SMI`.
const BT_IM_NMI2SMI: u8 = 1 << 4;

// SMIC interface registers.
/// `_SMIC_DATAIN_REG`.
const _SMIC_DATAIN_REG: i32 = 0;
/// `_SMIC_DATAOUT_REG`.
const _SMIC_DATAOUT_REG: i32 = 0;
/// `_SMIC_CTRL_REG`.
const _SMIC_CTRL_REG: i32 = 1;
/// `SMS_CC_GET_STATUS`.
pub const SMS_CC_GET_STATUS: u8 = 0x40;
/// `SMS_CC_START_TRANSFER`.
const SMS_CC_START_TRANSFER: u8 = 0x41;
/// `SMS_CC_NEXT_TRANSFER`.
const SMS_CC_NEXT_TRANSFER: u8 = 0x42;
/// `SMS_CC_END_TRANSFER`.
const SMS_CC_END_TRANSFER: u8 = 0x43;
/// `SMS_CC_START_RECEIVE`.
const SMS_CC_START_RECEIVE: u8 = 0x44;
/// `SMS_CC_NEXT_RECEIVE`.
const SMS_CC_NEXT_RECEIVE: u8 = 0x45;
/// `SMS_CC_END_RECEIVE`.
const SMS_CC_END_RECEIVE: u8 = 0x46;
/// `SMS_CC_TRANSFER_ABORT`.
pub const SMS_CC_TRANSFER_ABORT: u8 = 0x47;
/// `SMS_SC_READY`.
const SMS_SC_READY: i32 = 0xc0;
/// `SMS_SC_WRITE_START`.
const SMS_SC_WRITE_START: i32 = 0xc1;
/// `SMS_SC_WRITE_NEXT`.
const SMS_SC_WRITE_NEXT: i32 = 0xc2;
/// `SMS_SC_WRITE_END`.
const SMS_SC_WRITE_END: i32 = 0xc3;
/// `SMS_SC_READ_START`.
const SMS_SC_READ_START: i32 = 0xc4;
/// `SMS_SC_READ_NEXT`.
const SMS_SC_READ_NEXT: i32 = 0xc5;
/// `SMS_SC_READ_END`.
const SMS_SC_READ_END: i32 = 0xc6;
/// `_SMIC_FLAG_REG`.
const _SMIC_FLAG_REG: i32 = 2;
/// `SMIC_BUSY`.
const SMIC_BUSY: u8 = 1 << 0;
/// `SMIC_SMS_ATN`.
pub const SMIC_SMS_ATN: u8 = 1 << 2;
/// `SMIC_EVT_ATN`.
pub const SMIC_EVT_ATN: u8 = 1 << 3;
/// `SMIC_SMI`.
pub const SMIC_SMI: u8 = 1 << 4;
/// `SMIC_TX_DATA_RDY`.
const SMIC_TX_DATA_RDY: u8 = 1 << 6;
/// `SMIC_RX_DATA_RDY`.
const SMIC_RX_DATA_RDY: u8 = 1 << 7;

// KCS interface registers.
/// `_KCS_DATAIN_REGISTER`.
const _KCS_DATAIN_REGISTER: i32 = 0;
/// `_KCS_DATAOUT_REGISTER`.
const _KCS_DATAOUT_REGISTER: i32 = 0;
/// `KCS_READ_NEXT`.
const KCS_READ_NEXT: u8 = 0x68;
/// `_KCS_COMMAND_REGISTER`.
const _KCS_COMMAND_REGISTER: i32 = 1;
/// `KCS_GET_STATUS`.
const KCS_GET_STATUS: u8 = 0x60;
/// `KCS_WRITE_START`.
const KCS_WRITE_START: u8 = 0x61;
/// `KCS_WRITE_END`.
const KCS_WRITE_END: u8 = 0x62;
/// `_KCS_STATUS_REGISTER`.
const _KCS_STATUS_REGISTER: i32 = 1;
/// `KCS_OBF`: output buffer full.
const KCS_OBF: u8 = 1 << 0;
/// `KCS_IBF`: input buffer full.
const KCS_IBF: u8 = 1 << 1;
/// `KCS_SMS_ATN`.
pub const KCS_SMS_ATN: u8 = 1 << 2;
/// `KCS_CD`.
pub const KCS_CD: u8 = 1 << 3;
/// `KCS_OEM1`.
pub const KCS_OEM1: u8 = 1 << 4;
/// `KCS_OEM2`.
pub const KCS_OEM2: u8 = 1 << 5;
/// `KCS_STATE_MASK`.
const KCS_STATE_MASK: u8 = 0xc0;
/// `KCS_IDLE_STATE`.
const KCS_IDLE_STATE: i32 = 0x00;
/// `KCS_READ_STATE`.
const KCS_READ_STATE: i32 = 0x40;
/// `KCS_WRITE_STATE`.
const KCS_WRITE_STATE: i32 = 0x80;
/// `KCS_ERROR_STATE`.
const KCS_ERROR_STATE: i32 = 0xC0;

/// `READ_SMS_BUFFER`.
pub const READ_SMS_BUFFER: i32 = 0x37;
/// `WRITE_I2C`.
pub const WRITE_I2C: i32 = 0x50;
/// `GET_MESSAGE_CMD`.
pub const GET_MESSAGE_CMD: i32 = 0x33;
/// `SEND_MESSAGE_CMD`.
pub const SEND_MESSAGE_CMD: i32 = 0x34;
/// `IPMB_CHANNEL_NUMBER`.
pub const IPMB_CHANNEL_NUMBER: i32 = 0;
/// `PUBLIC_BUS`.
pub const PUBLIC_BUS: i32 = 0;
/// `MIN_I2C_PACKET_SIZE`.
pub const MIN_I2C_PACKET_SIZE: i32 = 3;
/// `MIN_IMB_PACKET_SIZE`: one byte for the checksum.
pub const MIN_IMB_PACKET_SIZE: i32 = 7;
/// `MIN_BTBMC_REQ_SIZE`.
pub const MIN_BTBMC_REQ_SIZE: i32 = 4;
/// `MIN_BTBMC_RSP_SIZE`.
pub const MIN_BTBMC_RSP_SIZE: i32 = 5;
/// `MIN_BMC_REQ_SIZE`.
pub const MIN_BMC_REQ_SIZE: i32 = 2;
/// `MIN_BMC_RSP_SIZE`.
pub const MIN_BMC_RSP_SIZE: i32 = 3;
/// `BMC_SA`: BMC/ESM3.
const BMC_SA: i32 = 0x20;
/// `FPC_SA`: front panel.
pub const FPC_SA: i32 = 0x22;
/// `BP_SA`: primary backplane.
pub const BP_SA: i32 = 0xC0;
/// `BP2_SA`: secondary backplane.
pub const BP2_SA: i32 = 0xC2;
/// `PBP_SA`: peripheral backplane.
pub const PBP_SA: i32 = 0xC4;
/// `DRAC_SA`: DRAC-III.
pub const DRAC_SA: i32 = 0x28;
/// `DRAC3_SA`: DRAC-III.
pub const DRAC3_SA: i32 = 0x30;
/// `BMC_LUN`.
const BMC_LUN: i32 = 0;
/// `SMS_LUN`.
pub const SMS_LUN: i32 = 2;

/// `MIN_PERIOD`: the shortest watchdog period the driver sets, in seconds.
const MIN_PERIOD: i32 = 10;

/// `SMIPMI_FLAG_IRQLVL`: SMBIOS IPMI flags: level-triggered interrupt.
const SMIPMI_FLAG_IRQLVL: u8 = 1 << 0;
/// `SMIPMI_FLAG_IRQEN`: the interrupt is specified.
const SMIPMI_FLAG_IRQEN: u8 = 1 << 3;
/// `SMIPMI_FLAG_ODDOFFSET`: the address's low bit.
const SMIPMI_FLAG_ODDOFFSET: u8 = 1 << 4;
/// `IPMI_IOSPACING_BYTE`.
const IPMI_IOSPACING_BYTE: u8 = 0;
/// `IPMI_IOSPACING_WORD`.
const IPMI_IOSPACING_WORD: u8 = 2;
/// `IPMI_IOSPACING_DWORD`.
const IPMI_IOSPACING_DWORD: u8 = 1;

/// `SMBIOS_TYPE_IPMIDEV` (`<machine/smbiosvar.h>`).
pub const SMBIOS_TYPE_IPMIDEV: u8 = 38;

/// `struct ipmi_msg`: a message to or from `/dev/ipmi`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct IpmiMsg {
    /// `netfn`.
    pub netfn: u8,
    /// `cmd`.
    pub cmd: u8,
    /// `data_len`.
    pub data_len: u16,
    /// `data`: a user address.
    pub data: usize,
}

/// `struct ipmi_req`: `IPMICTL_SEND_COMMAND`'s argument.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct IpmiReq {
    /// `addr`: a user address.
    pub addr: usize,
    /// `addr_len`.
    pub addr_len: u32,
    /// `msgid`.
    pub msgid: i64,
    /// `msg`.
    pub msg: IpmiMsg,
}

/// `struct ipmi_recv`: `IPMICTL_RECEIVE_MSG`'s argument.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct IpmiRecv {
    /// `recv_type`.
    pub recv_type: i32,
    /// `addr`: a user address.
    pub addr: usize,
    /// `addr_len`.
    pub addr_len: u32,
    /// `msgid`.
    pub msgid: i64,
    /// `msg`.
    pub msg: IpmiMsg,
}

/// `struct ipmi_cmdspec`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct IpmiCmdspec {
    /// `netfn`.
    pub netfn: u8,
    /// `cmd`.
    pub cmd: u8,
}

/// `struct ipmi_addr`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct IpmiAddr {
    /// `addr_type`.
    pub addr_type: i32,
    /// `channel`.
    pub channel: i16,
    /// `data`.
    pub data: [u8; IPMI_MAX_ADDR_SIZE],
}

/// `struct ipmi_system_interface_addr`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct IpmiSystemInterfaceAddr {
    /// `addr_type`.
    pub addr_type: i32,
    /// `channel`.
    pub channel: i16,
    /// `lun`.
    pub lun: u8,
}

/// `struct ipmi_ipmb_addr`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct IpmiIpmbAddr {
    /// `addr_type`.
    pub addr_type: i32,
    /// `channel`.
    pub channel: i16,
    /// `slave_addr`.
    pub slave_addr: u8,
    /// `lun`.
    pub lun: u8,
}

/// `struct ipmi_sensor`: a BMC sensor exported to `sensors(9)`.
pub struct IpmiSensor {
    /// `i_sdr`: its sensor data record (shared by the sensors of one compact record).
    pub i_sdr: &'static [u8],
    /// `i_num`: the sensor number.
    pub i_num: i32,
    /// `stype`: the sensor type.
    pub stype: i32,
    /// `etype`: the event/reading type.
    pub etype: i32,
    /// `i_sensor`.
    pub i_sensor: Ksensor,
    /// `list`.
    pub list: SlistEntry<IpmiSensor>,
}

crate::queue_adapter!(
    /// `SLIST_HEAD(ipmi_sensors_head, ipmi_sensor)`, through `list`.
    pub IpmiSensorsList: IpmiSensor, list => SlistEntry<IpmiSensor>
);

/// `struct ipmi_sensors_head`, made `Sync`: the sensor thread fills and walks it (see the
/// module's deviations).
pub struct IpmiSensorsHead(SlistHead<IpmiSensorsList>);

// SAFETY: see the type's doc: one thread at a time touches the list.
unsafe impl Sync for IpmiSensorsHead {}

/// `struct dmd_ipmi`: the BIOS area's IPMI signature block.
#[repr(C, packed)]
#[derive(Clone, Copy, Default)]
pub struct DmdIpmi {
    /// `dmd_sig`: "IPMI".
    pub dmd_sig: [u8; 4],
    /// `dmd_i2c_address`: the BMC's address.
    pub dmd_i2c_address: u8,
    /// `dmd_nvram_address`: the NVRAM's address.
    pub dmd_nvram_address: u8,
    /// `dmd_if_type`: the interface type.
    pub dmd_if_type: u8,
    /// `dmd_if_rev`: the interface revision.
    pub dmd_if_rev: u8,
}

/// `struct smbios_ipmi` (`<machine/smbiosvar.h>`, DMTF DSP0134 section 3.3.39): SMBIOS type
/// 38, the IPMI device information.
#[repr(C, packed)]
#[derive(Clone, Copy, Default)]
pub struct SmbiosIpmi {
    /// `smipmi_if_type`: the interface type.
    pub smipmi_if_type: u8,
    /// `smipmi_if_rev`: the BCD IPMI revision.
    pub smipmi_if_rev: u8,
    /// `smipmi_i2c_address`: the BMC's I2C address.
    pub smipmi_i2c_address: u8,
    /// `smipmi_nvram_address`: the NVRAM's I2C address.
    pub smipmi_nvram_address: u8,
    /// `smipmi_base_address`: the BMC's base address (BAR format).
    pub smipmi_base_address: u64,
    /// `smipmi_base_flags`: register spacing (bits 7-6), low address bit (4), IRQ valid
    /// (3), interrupt polarity (1) and trigger (0).
    pub smipmi_base_flags: u8,
    /// `smipmi_irq`: the interrupt, if any.
    pub smipmi_irq: u8,
}

/// The command and its softc an `ipmi_cmd_wait` hands its task, and the flag the task sets.
struct IpmiCmdWait {
    /// The softc.
    sc: *const IpmiSoftc,
    /// The command.
    c: *mut IpmiCmd<'static>,
    /// Set by the task once the command is done; under `mtx`.
    done: AtomicBool,
    /// Orders `done` against the sleep.
    mtx: Mutex,
}

/// `NIPMI`: `config(8)`'s count of ipmi devices (`ipmi0` on amd64, `ipmi*` on arm64).
pub const NIPMI: i32 = 1;

/// `ipmi_enabled`: an IPMI controller attached (`i2c_scan.c` then skips the sensors it
/// would have found).
pub static IPMI_ENABLED: AtomicI32 = AtomicI32::new(0);

/// `ipmi_sensor_list`.
static IPMI_SENSOR_LIST: IpmiSensorsHead = IpmiSensorsHead(SlistHead::new());

/// `kcs_if`.
pub static KCS_IF: IpmiIf = IpmiIf::KCS;
/// `smic_if`.
pub static SMIC_IF: IpmiIf = IpmiIf::SMIC;
/// `bt_if`.
pub static BT_IF: IpmiIf = IpmiIf::BT;

/// `ipmi_cd`.
pub static IPMI_CD: Cfdriver = Cfdriver::new(b"ipmi", DV_DULL, 0);

/// `maxsdrlen`: the SDR bytes read per request.
pub static MAXSDRLEN: AtomicI32 = AtomicI32::new(0x10);

/// `ipmi_ca`: ipmi at mainbus (amd64).
pub static IPMI_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<IpmiSoftc>(),
    ca_match: Some(ipmi_match),
    ca_attach: ipmi_attach,
    ca_detach: None,
    ca_activate: Some(ipmi_activate),
};

impl<S: IpmiBmc> IpmiIf<S> {
    /// `kcs_if`'s members.
    pub const KCS: Self = Self {
        name: "KCS",
        nregs: IPMI_IF_KCS_NREGS,
        buildmsg: cmn_buildmsg,
        sendmsg: kcs_sendmsg::<S>,
        recvmsg: kcs_recvmsg::<S>,
        reset: kcs_reset::<S>,
        probe: kcs_probe::<S>,
        datasnd: IPMI_MSG_DATASND,
        datarcv: IPMI_MSG_DATARCV,
    };

    /// `smic_if`'s members.
    pub const SMIC: Self = Self {
        name: "SMIC",
        nregs: IPMI_IF_SMIC_NREGS,
        buildmsg: cmn_buildmsg,
        sendmsg: smic_sendmsg::<S>,
        recvmsg: smic_recvmsg::<S>,
        reset: smic_reset::<S>,
        probe: smic_probe::<S>,
        datasnd: IPMI_MSG_DATASND,
        datarcv: IPMI_MSG_DATARCV,
    };

    /// `bt_if`'s members.
    pub const BT: Self = Self {
        name: "BT",
        nregs: IPMI_IF_BT_NREGS,
        buildmsg: bt_buildmsg,
        sendmsg: bt_sendmsg::<S>,
        recvmsg: bt_recvmsg::<S>,
        reset: bt_reset::<S>,
        probe: bt_probe::<S>,
        datasnd: IPMI_BTMSG_DATASND,
        datarcv: IPMI_BTMSG_DATARCV,
    };
}

/// `ipmi_get_if(iftype)`: the interface layer of `iftype`, `None` for SSIF and unknown
/// types.
pub fn ipmi_get_if(iftype: i32) -> Option<&'static IpmiIf> {
    match iftype {
        IPMI_IF_KCS => Some(&KCS_IF),
        IPMI_IF_SMIC => Some(&SMIC_IF),
        IPMI_IF_BT => Some(&BT_IF),
        _ => None,
    }
}

/// The BMC helper functions on the softc: `bmc_read`, `bmc_write` through `bus_space(9)`.
impl IpmiBmc for IpmiSoftc {
    fn bmc_read(&self, offset: i32) -> u8 {
        let (Some(t), Some(h)) = (self.sc_iot.get(), self.sc_ioh.get()) else {
            return 0xff;
        };
        let off = (offset * self.sc_if_iospacing.get()) as BusSize;
        if self.sc_if_iosize.get() == 4 {
            bus_space_read_4(t, h, off) as u8
        } else {
            bus_space_read_1(t, h, off)
        }
    }

    fn bmc_write(&self, offset: i32, val: u8) {
        let (Some(t), Some(h)) = (self.sc_iot.get(), self.sc_ioh.get()) else {
            return;
        };
        let off = (offset * self.sc_if_iospacing.get()) as BusSize;
        if self.sc_if_iosize.get() == 4 {
            bus_space_write_4(t, h, off, u32::from(val));
        } else {
            bus_space_write_1(t, h, off, val);
        }
    }

    fn devname(&self) -> &str {
        self.sc_dev.xname()
    }

    fn sc_if(&self) -> Option<IpmiIf<Self>> {
        self.sc_if.get().copied()
    }

    fn sc_xfer(&self) -> &RefCell<IpmiXfer> {
        &self.sc_xfer
    }

    fn sc_sensordev(&self) -> &Ksensordev {
        &self.sc_sensordev
    }

    fn ipmi_cmd_wait(&self, c: &mut IpmiCmd<'_>) {
        ipmi_cmd_wait(self, c);
    }
}

/// `bmc_io_wait(sc, a)`: polls register `a.offset` until the bits of `a.mask` read
/// `a.value`, for up to five seconds. Returns the register, or -1.
pub fn bmc_io_wait<S: IpmiBmc>(sc: &S, a: &IpmiIowait) -> i32 {
    // == 5s XXX can be shorter
    for _ in 0..5_000_000 {
        let v = sc.bmc_read(a.offset);
        if v & a.mask == a.value {
            return i32::from(v);
        }

        sc.bmc_delay();
    }

    // dbg_printf(1, "bmc_io_wait fails ..."): IPMI_DEBUG is not configured.
    -1
}

/// `NETFN_LUN(nf, ln)`.
const fn netfn_lun(nf: i32, ln: i32) -> u8 {
    ((nf << 2) | (ln & LUN_MASK)) as u8
}

/// `bt_read(sc, reg)`.
pub fn bt_read<S: IpmiBmc>(sc: &S, reg: i32) -> i32 {
    i32::from(sc.bmc_read(reg))
}

/// `bt_write(sc, reg, data)`: writes once the BMC is not busy; -1 if it stays busy.
pub fn bt_write<S: IpmiBmc>(sc: &S, reg: i32, data: u8) -> i32 {
    let a = IpmiIowait {
        offset: _BT_CTRL_REG,
        mask: BT_BMC_BUSY,
        value: 0,
        lbl: "bt_write",
    };
    if bmc_io_wait(sc, &a) < 0 {
        return -1;
    }

    sc.bmc_write(reg, data);
    0
}

/// `bt_sendmsg(c)`: writes the message into the BMC's buffer and signals it.
pub fn bt_sendmsg<S: IpmiBmc>(sc: &S, x: &mut IpmiXfer, c: &mut IpmiCmd<'_>) -> i32 {
    bt_write(sc, _BT_CTRL_REG, BT_CLR_WR_PTR);
    let n = (c.c_txlen.max(0) as usize).min(x.sc_buf.len());
    for &b in &x.sc_buf[..n] {
        bt_write(sc, _BT_DATAOUT_REG, b);
    }

    bt_write(sc, _BT_CTRL_REG, BT_HOST2BMC_ATN);
    let a = IpmiIowait {
        offset: _BT_CTRL_REG,
        mask: BT_HOST2BMC_ATN | BT_BMC_BUSY,
        value: 0,
        lbl: "bt_sendwait",
    };
    if bmc_io_wait(sc, &a) < 0 {
        return -1;
    }

    0
}

/// `bt_recvmsg(c)`: waits for the BMC's response and reads it, leaving out the sequence
/// number.
pub fn bt_recvmsg<S: IpmiBmc>(sc: &S, x: &mut IpmiXfer, c: &mut IpmiCmd<'_>) -> i32 {
    let a = IpmiIowait {
        offset: _BT_CTRL_REG,
        mask: BT_BMC2HOST_ATN,
        value: BT_BMC2HOST_ATN,
        lbl: "bt_recvwait",
    };
    if bmc_io_wait(sc, &a) < 0 {
        return -1;
    }

    bt_write(sc, _BT_CTRL_REG, BT_HOST_BUSY);
    bt_write(sc, _BT_CTRL_REG, BT_BMC2HOST_ATN);
    bt_write(sc, _BT_CTRL_REG, BT_CLR_RD_PTR);
    let len = bt_read(sc, _BT_DATAIN_REG) as u8;
    let mut j = 0;
    for i in IPMI_BTMSG_NFLN..=usize::from(len) {
        let v = bt_read(sc, _BT_DATAIN_REG) as u8;
        if i != IPMI_BTMSG_SEQ
            && let Some(slot) = x.sc_buf.get_mut(j)
        {
            *slot = v;
            j += 1;
        }
    }
    bt_write(sc, _BT_CTRL_REG, BT_HOST_BUSY);
    c.c_rxlen = i32::from(len) - 1;

    0
}

/// `bt_reset(sc)`: not supported.
pub fn bt_reset<S: IpmiBmc>(_sc: &S) -> i32 {
    -1
}

/// `bt_probe(sc)`: clears the pointers and attention bits and masks the BMC's interrupt.
pub fn bt_probe<S: IpmiBmc>(sc: &S) -> i32 {
    let mut rv = sc.bmc_read(_BT_CTRL_REG);
    rv &= BT_HOST_BUSY;
    rv |= BT_CLR_WR_PTR | BT_CLR_RD_PTR | BT_BMC2HOST_ATN | BT_HOST2BMC_ATN;
    sc.bmc_write(_BT_CTRL_REG, rv);

    let mut rv = sc.bmc_read(_BT_INTMASK_REG);
    rv &= BT_IM_SCI_EN | BT_IM_SMI_EN | BT_IM_NMI2SMI;
    rv |= BT_IM_HIRQ_PEND;
    sc.bmc_write(_BT_INTMASK_REG, rv);

    // #if 0: the register dump.
    0
}

/// `smic_wait(sc, mask, val, lbl)`: waits for the flag bits, then returns the control
/// (status) register; -1 on timeout.
pub fn smic_wait<S: IpmiBmc>(sc: &S, mask: u8, val: u8, _lbl: &'static str) -> i32 {
    // Wait for expected flag bits
    let a = IpmiIowait {
        offset: _SMIC_FLAG_REG,
        mask,
        value: val,
        lbl: "smicwait",
    };
    if bmc_io_wait(sc, &a) < 0 {
        return -1;
    }

    // Return current status
    i32::from(sc.bmc_read(_SMIC_CTRL_REG))
}

/// `smic_write_cmd_data(sc, cmd, data)`: writes a control code and a data byte, toggles
/// BUSY and returns the status once the BMC took them.
pub fn smic_write_cmd_data<S: IpmiBmc>(sc: &S, cmd: u8, data: Option<u8>) -> i32 {
    let sts = smic_wait(
        sc,
        SMIC_TX_DATA_RDY | SMIC_BUSY,
        SMIC_TX_DATA_RDY,
        "smic_write_cmd_data ready",
    );
    if sts < 0 {
        return sts;
    }

    sc.bmc_write(_SMIC_CTRL_REG, cmd);
    if let Some(data) = data {
        sc.bmc_write(_SMIC_DATAOUT_REG, data);
    }

    // Toggle BUSY bit, then wait for busy bit to clear
    let v = sc.bmc_read(_SMIC_FLAG_REG);
    sc.bmc_write(_SMIC_FLAG_REG, v | SMIC_BUSY);

    smic_wait(sc, SMIC_BUSY, 0, "smic_write_cmd_data busy")
}

/// `smic_read_data(sc, data)`: reads a data byte once the BMC has one; the status.
pub fn smic_read_data<S: IpmiBmc>(sc: &S, data: &mut u8) -> i32 {
    let sts = smic_wait(
        sc,
        SMIC_RX_DATA_RDY | SMIC_BUSY,
        SMIC_RX_DATA_RDY,
        "smic_read_data",
    );
    if sts >= 0 {
        *data = sc.bmc_read(_SMIC_DATAIN_REG);
    }
    sts
}

/// `ErrStat(a, b)`: prints `b` when `a`.
fn err_stat(a: bool, b: &str) {
    if a {
        kprintf!("{}", b);
    }
}

/// `smic_sendmsg(c)`: START, NEXT for each middle byte, END.
pub fn smic_sendmsg<S: IpmiBmc>(sc: &S, x: &mut IpmiXfer, c: &mut IpmiCmd<'_>) -> i32 {
    let byte = |i: i32| x.sc_buf.get(i.max(0) as usize).copied().unwrap_or(0);

    let sts = smic_write_cmd_data(sc, SMS_CC_START_TRANSFER, Some(byte(0)));
    err_stat(sts != SMS_SC_WRITE_START, "wstart");
    let mut idx = 1;
    while idx < c.c_txlen - 1 {
        let sts = smic_write_cmd_data(sc, SMS_CC_NEXT_TRANSFER, Some(byte(idx)));
        err_stat(sts != SMS_SC_WRITE_NEXT, "write");
        idx += 1;
    }
    let sts = smic_write_cmd_data(sc, SMS_CC_END_TRANSFER, Some(byte(idx)));
    if sts != SMS_SC_WRITE_END {
        return -1;
    }

    0
}

/// `smic_recvmsg(c)`: START, then a byte and NEXT until the BMC says END, then END.
pub fn smic_recvmsg<S: IpmiBmc>(sc: &S, x: &mut IpmiXfer, c: &mut IpmiCmd<'_>) -> i32 {
    c.c_rxlen = 0;
    let sts = smic_wait(sc, SMIC_RX_DATA_RDY, SMIC_RX_DATA_RDY, "smic_recvmsg");
    if sts < 0 {
        return -1;
    }

    let sts = smic_write_cmd_data(sc, SMS_CC_START_RECEIVE, None);
    err_stat(sts != SMS_SC_READ_START, "rstart");
    let mut idx = 0;
    let sts = loop {
        let mut b = 0;
        let sts = smic_read_data(sc, &mut b);
        if let Some(slot) = x.sc_buf.get_mut(idx) {
            *slot = b;
        }
        idx += 1;
        if (sts != SMS_SC_READ_START && sts != SMS_SC_READ_NEXT) || idx >= x.sc_buf.len() {
            break sts;
        }
        smic_write_cmd_data(sc, SMS_CC_NEXT_RECEIVE, None);
    };
    err_stat(sts != SMS_SC_READ_END, "rend");

    c.c_rxlen = idx as i32;

    let sts = smic_write_cmd_data(sc, SMS_CC_END_RECEIVE, None);
    if sts != SMS_SC_READY {
        return -1;
    }

    0
}

/// `smic_reset(sc)`: not supported.
pub fn smic_reset<S: IpmiBmc>(_sc: &S) -> i32 {
    -1
}

/// `smic_probe(sc)`: the flag register should not be 0xFF on a good system.
pub fn smic_probe<S: IpmiBmc>(sc: &S) -> i32 {
    if sc.bmc_read(_SMIC_FLAG_REG) == 0xFF {
        return -1;
    }

    0
}

/// `kcs_wait(sc, mask, value, lbl)`: waits for the status bits, drops a stale output byte
/// in the write state, reports an error state; returns the state (`KCS_*_STATE`) or -1.
pub fn kcs_wait<S: IpmiBmc>(sc: &S, mask: u8, value: u8, lbl: &'static str) -> i32 {
    let a = IpmiIowait {
        offset: _KCS_STATUS_REGISTER,
        mask,
        value,
        lbl,
    };
    let v = bmc_io_wait(sc, &a);
    if v < 0 {
        return v;
    }
    let v = v as u8;

    // Check if output buffer full, read dummy byte
    if v & (KCS_OBF | KCS_STATE_MASK) == KCS_OBF | KCS_WRITE_STATE as u8 {
        sc.bmc_read(_KCS_DATAIN_REGISTER);
    }

    // Check for error state
    if i32::from(v & KCS_STATE_MASK) == KCS_ERROR_STATE {
        sc.bmc_write(_KCS_COMMAND_REGISTER, KCS_GET_STATUS);
        while sc.bmc_read(_KCS_STATUS_REGISTER) & KCS_IBF != 0 {
            core::hint::spin_loop();
        }
        kprintf!(
            "{}: error code: {:x}\n",
            sc.devname(),
            sc.bmc_read(_KCS_DATAIN_REGISTER)
        );
    }

    i32::from(v & KCS_STATE_MASK)
}

/// `kcs_write_cmd(sc, cmd)`: writes a control code; the state once the BMC took it.
pub fn kcs_write_cmd<S: IpmiBmc>(sc: &S, cmd: u8) -> i32 {
    // ASSERT: IBF and OBF are clear
    sc.bmc_write(_KCS_COMMAND_REGISTER, cmd);

    kcs_wait(sc, KCS_IBF, 0, "write_cmd")
}

/// `kcs_write_data(sc, data)`: writes a data byte; the state once the BMC took it.
pub fn kcs_write_data<S: IpmiBmc>(sc: &S, data: u8) -> i32 {
    // ASSERT: IBF and OBF are clear
    sc.bmc_write(_KCS_DATAOUT_REGISTER, data);

    kcs_wait(sc, KCS_IBF, 0, "write_data")
}

/// `kcs_read_data(sc, data)`: in the read state, reads a byte and asks for the next.
pub fn kcs_read_data<S: IpmiBmc>(sc: &S, data: &mut u8) -> i32 {
    let sts = kcs_wait(sc, KCS_IBF | KCS_OBF, KCS_OBF, "read_data");
    if sts != KCS_READ_STATE {
        return sts;
    }

    // ASSERT: OBF is set read data, request next byte
    *data = sc.bmc_read(_KCS_DATAIN_REGISTER);
    sc.bmc_write(_KCS_DATAOUT_REGISTER, KCS_READ_NEXT);

    sts
}

/// `kcs_sendmsg(c)`: WRITE_START, the bytes, WRITE_END before the last; the BMC must end in
/// the read state.
pub fn kcs_sendmsg<S: IpmiBmc>(sc: &S, x: &mut IpmiXfer, c: &mut IpmiCmd<'_>) -> i32 {
    // ASSERT: IBF is clear
    let mut sts = kcs_write_cmd(sc, KCS_WRITE_START);
    let mut idx = 0;
    while idx < c.c_txlen {
        if idx == c.c_txlen - 1 {
            sts = kcs_write_cmd(sc, KCS_WRITE_END);
        }

        if sts != KCS_WRITE_STATE {
            break;
        }

        let b = x.sc_buf.get(idx as usize).copied().unwrap_or(0);
        sts = kcs_write_data(sc, b);
        idx += 1;
    }
    if sts != KCS_READ_STATE {
        return -1;
    }

    0
}

/// `kcs_recvmsg(c)`: reads bytes while the BMC is in the read state, up to `c_maxrxlen`;
/// it must end idle.
pub fn kcs_recvmsg<S: IpmiBmc>(sc: &S, x: &mut IpmiXfer, c: &mut IpmiCmd<'_>) -> i32 {
    let max = (c.c_maxrxlen.max(0) as usize).min(x.sc_buf.len());
    let mut idx = 0;
    while idx < max {
        let sts = kcs_read_data(sc, &mut x.sc_buf[idx]);
        if sts != KCS_READ_STATE {
            break;
        }
        idx += 1;
    }
    let sts = kcs_wait(sc, KCS_IBF, 0, "recv");
    c.c_rxlen = idx as i32;
    if sts != KCS_IDLE_STATE {
        return -1;
    }

    0
}

/// `kcs_reset(sc)`: not supported.
pub fn kcs_reset<S: IpmiBmc>(_sc: &S) -> i32 {
    -1
}

/// `kcs_probe(sc)`: 1 if the interface is in its error state.
pub fn kcs_probe<S: IpmiBmc>(sc: &S) -> i32 {
    let v = sc.bmc_read(_KCS_STATUS_REGISTER);
    if i32::from(v & KCS_STATE_MASK) == KCS_ERROR_STATE {
        return 1;
    }
    // #if 0: the status dump.
    0
}

/// `dumpb(lbl, len, data)`: prints `data` in hex (the `IPMI_DEBUG` dumps).
pub fn dumpb(lbl: &str, data: &[u8]) {
    kprintf!("{}: ", lbl);
    for b in data {
        kprintf!("{:02x} ", b);
    }

    kprintf!("\n");
}

/// The request bytes `c` sends: its data, `None` when it has fewer than `c_txlen`.
fn cmd_txdata<'c>(c: &'c IpmiCmd<'_>) -> Option<&'c [u8]> {
    let n = c.c_txlen.max(0) as usize;
    if n == 0 {
        return Some(&[]);
    }
    c.c_data.as_deref()?.get(..n)
}

/// `bt_buildmsg(c)`: a BT message from the netfn/LUN, the command and the data. Used by
/// the BT protocol.
pub fn bt_buildmsg(x: &mut IpmiXfer, c: &IpmiCmd<'_>) {
    x.sc_buf[IPMI_BTMSG_LEN] = (c.c_txlen + (IPMI_BTMSG_DATASND - 1)) as u8;
    x.sc_buf[IPMI_BTMSG_NFLN] = netfn_lun(c.c_netfn, c.c_rslun);
    x.sc_buf[IPMI_BTMSG_SEQ] = x.sc_btseq as u8;
    x.sc_btseq = x.sc_btseq.wrapping_add(1);
    x.sc_buf[IPMI_BTMSG_CMD] = c.c_cmd as u8;
    if let Some(d) = cmd_txdata(c) {
        let off = IPMI_BTMSG_DATASND as usize;
        let n = d.len().min(x.sc_buf.len() - off);
        x.sc_buf[off..off + n].copy_from_slice(&d[..n]);
    }
}

/// `cmn_buildmsg(c)`: a message from the netfn/LUN, the command and the data. Used by
/// both the SMIC and KCS protocols.
pub fn cmn_buildmsg(x: &mut IpmiXfer, c: &IpmiCmd<'_>) {
    x.sc_buf[IPMI_MSG_NFLN] = netfn_lun(c.c_netfn, c.c_rslun);
    x.sc_buf[IPMI_MSG_CMD] = c.c_cmd as u8;
    if let Some(d) = cmd_txdata(c) {
        let off = IPMI_MSG_DATASND as usize;
        let n = d.len().min(x.sc_buf.len() - off);
        x.sc_buf[off..off + n].copy_from_slice(&d[..n]);
    }
}

/// `ipmi_sendcmd(c)`: sends an IPMI command; 0, or -1 (and -1 for any responder but the
/// BMC: the IPMB path is `#if 0` in the C).
pub fn ipmi_sendcmd<S: IpmiBmc>(sc: &S, c: &mut IpmiCmd<'_>) -> i32 {
    let Some(ifp) = sc.sc_if() else {
        return -1;
    };
    if c.c_rssa != BMC_SA {
        // #if 0: an IPMB request through the BMC.
        return -1;
    }
    if cmd_txdata(c).is_none() {
        return -1;
    }
    let Ok(mut x) = sc.sc_xfer().try_borrow_mut() else {
        return -1;
    };
    (ifp.buildmsg)(&mut x, c);

    c.c_txlen += ifp.datasnd;
    (ifp.sendmsg)(sc, &mut x, c)
}

/// `ipmi_recvcmd(c)`: receives the response and copies its data out; the completion code,
/// or -1.
pub fn ipmi_recvcmd<S: IpmiBmc>(sc: &S, c: &mut IpmiCmd<'_>) -> i32 {
    let Some(ifp) = sc.sc_if() else {
        return -1;
    };
    let Ok(mut x) = sc.sc_xfer().try_borrow_mut() else {
        return -1;
    };

    // Receive message from interface, copy out result data
    c.c_maxrxlen += ifp.datarcv;
    if (ifp.recvmsg)(sc, &mut x, c) != 0 || c.c_rxlen < ifp.datarcv {
        return -1;
    }

    c.c_rxlen -= ifp.datarcv;
    if c.c_rxlen > 0
        && let Some(data) = c.c_data.as_deref_mut()
    {
        let off = ifp.datarcv as usize;
        let n = (c.c_rxlen as usize)
            .min(data.len())
            .min(x.sc_buf.len() - off);
        data[..n].copy_from_slice(&x.sc_buf[off..off + n]);
    }

    // IPMI_DEBUG: the completion code and the response dump.
    i32::from(x.sc_buf[IPMI_MSG_CCODE])
}

/// `ipmi_cmd(c)`: runs a command; polled while cold or panicking, else on the command
/// task queue.
pub fn ipmi_cmd<S: IpmiBmc>(sc: &S, c: &mut IpmiCmd<'_>) {
    if COLD.load(Ordering::Relaxed) || panicstr() {
        ipmi_cmd_poll(sc, c);
    } else {
        sc.ipmi_cmd_wait(c);
    }
}

/// `ipmi_cmd_poll(c)`: sends the command and receives its response here.
pub fn ipmi_cmd_poll<S: IpmiBmc>(sc: &S, c: &mut IpmiCmd<'_>) {
    let rc = ipmi_sendcmd(sc, c);
    c.c_ccode = rc;
    if c.c_ccode != 0 {
        kprintf!("{}: sendcmd fails\n", sc.devname());
    } else {
        c.c_ccode = ipmi_recvcmd(sc, c);
    }
}

/// `ipmi_cmd_wait(c)`: runs the command on the softc's command task queue and sleeps until
/// it is done (see the module's deviations).
pub fn ipmi_cmd_wait(sc: &IpmiSoftc, c: &mut IpmiCmd<'_>) {
    let Some(tq) = sc.sc_cmd_taskq.get() else {
        ipmi_cmd_poll(sc, c);
        return;
    };

    let w = IpmiCmdWait {
        sc: ptr::from_ref(sc),
        c: ptr::from_mut(c).cast::<IpmiCmd<'static>>(),
        done: AtomicBool::new(false),
        mtx: Mutex::new(IPL_MPFLOOR),
    };
    let t = Task::new(ipmi_cmd_wait_cb, ptr::from_ref(&w).cast_mut().cast());
    // SAFETY: `t` and `w` live on this stack frame until the task has run (`done` is set
    // after its last use of them, under `w.mtx`, and this function waits for it) and the
    // `task_del` below found it off the worklist.
    let res = unsafe { task_add_local(tq, &t) };
    kassert!(res);

    mtx_enter(&w.mtx);
    while !w.done.load(Ordering::Relaxed) {
        let _ = msleep_nsec(ptr::from_ref(&w), &w.mtx, PWAIT, "ipmicmd", INFSLP);
    }
    mtx_leave(&w.mtx);

    let res = task_del(tq, &t);
    kassert!(!res);
}

/// `ipmi_cmd_wait_cb(arg)`: the command task: polls the command, wakes its issuer.
pub fn ipmi_cmd_wait_cb(arg: *mut c_void) {
    // SAFETY: `ipmi_cmd_wait` passed its `IpmiCmdWait`, alive until `done` is set below.
    let w = unsafe { &*arg.cast_const().cast::<IpmiCmdWait>() };
    // SAFETY: the softc is the device's, never freed; the command is the waiting caller's,
    // which does not touch it until `done` is set.
    let (sc, c) = unsafe { (&*w.sc, &mut *w.c) };

    ipmi_cmd_poll(sc, c);

    mtx_enter(&w.mtx);
    w.done.store(true, Ordering::Relaxed);
    wakeup(ptr::from_ref(w));
    mtx_leave(&w.mtx);
}

/// `get_sdr_partial(sc, recordId, reserveId, offset, length, buffer, nxtRecordId)`: reads
/// `length` bytes of record `record_id` from `offset` into `buffer`; 0, or 1 when nothing
/// came back.
pub fn get_sdr_partial<S: IpmiBmc>(
    sc: &S,
    record_id: u16,
    reserve_id: u16,
    offset: u8,
    length: u8,
    buffer: &mut [u8],
    nxt_record_id: Option<&mut u16>,
) -> i32 {
    // 8 + max of length
    let mut cmd = [0u8; IPMI_GET_WDOG_MAX + 255];

    cmd[0..2].copy_from_slice(&reserve_id.to_le_bytes());
    cmd[2..4].copy_from_slice(&record_id.to_le_bytes());
    cmd[4] = offset;
    cmd[5] = length;

    let mut c = IpmiCmd {
        c_rssa: BMC_SA,
        c_rslun: BMC_LUN,
        c_netfn: STORAGE_NETFN,
        c_cmd: STORAGE_GET_SDR,
        c_txlen: IPMI_SET_WDOG_MAX as i32,
        c_rxlen: 0,
        c_maxrxlen: 8 + i32::from(length),
        c_data: Some(&mut cmd),
        c_ccode: 0,
    };
    ipmi_cmd(sc, &mut c);
    let len = c.c_rxlen;

    if let Some(nxt) = nxt_record_id {
        *nxt = u16::from_le_bytes([cmd[0], cmd[1]]);
    }
    if len > 2 {
        let n = ((len - 2) as usize).min(buffer.len()).min(cmd.len() - 2);
        buffer[..n].copy_from_slice(&cmd[2..2 + n]);
    } else {
        return 1;
    }

    0
}

/// `get_sdr(sc, recid, nxtrec)`: reads a whole SDR and offers it to `add_sdr_sensor`;
/// 0, or 1 when it could not be read.
pub fn get_sdr<S: IpmiBmc>(sc: &S, recid: u16, nxtrec: &mut u16) -> i32 {
    // Reserve SDR
    let mut resid = [0u8; 2];
    let mut c = IpmiCmd {
        c_rssa: BMC_SA,
        c_rslun: BMC_LUN,
        c_netfn: STORAGE_NETFN,
        c_cmd: STORAGE_RESERVE_SDR,
        c_txlen: 0,
        c_maxrxlen: 2,
        c_rxlen: 0,
        c_data: Some(&mut resid),
        c_ccode: 0,
    };
    ipmi_cmd(sc, &mut c);
    let resid = u16::from_le_bytes(resid);

    // Get SDR Header
    let mut shdr = [0u8; size_of::<Sdrhdr>()];
    if get_sdr_partial(
        sc,
        recid,
        resid,
        0,
        shdr.len() as u8,
        &mut shdr,
        Some(nxtrec),
    ) != 0
    {
        kprintf!("{}: get header fails\n", sc.devname());
        return 1;
    }
    // Allocate space for entire SDR. Length of SDR in header does not include header length.
    let sdrlen = shdr.len() + usize::from(shdr[4]);
    let Some(mem) = malloc(sdrlen, M_DEVBUF, M_NOWAIT) else {
        return 1;
    };
    // SAFETY: a fresh allocation of `sdrlen` bytes, this function's alone until it is freed
    // below or handed to the sensors, which only read it.
    let psdr: &'static mut [u8] = unsafe { core::slice::from_raw_parts_mut(mem.as_ptr(), sdrlen) };

    psdr[..shdr.len()].copy_from_slice(&shdr);

    // Read SDR Data maxsdrlen bytes at a time
    let maxsdrlen = MAXSDRLEN.load(Ordering::Relaxed).max(1) as usize;
    let mut offset = shdr.len();
    while offset < sdrlen {
        let len = (sdrlen - offset).min(maxsdrlen);

        if get_sdr_partial(
            sc,
            recid,
            resid,
            offset as u8,
            len as u8,
            &mut psdr[offset..offset + len],
            None,
        ) != 0
        {
            kprintf!("{}: get chunk: {},{} fails\n", sc.devname(), offset, len);
            free(mem, M_DEVBUF, sdrlen);
            return 1;
        }
        offset += maxsdrlen;
    }

    // Add SDR to sensor list, if not wanted, free buffer
    if add_sdr_sensor(sc, psdr) == 0 {
        free(mem, M_DEVBUF, sdrlen);
    }

    0
}

/// `getbits(bytes, bitpos, bitlen)`: `bitlen` bits from bit `bitpos` on, the highest
/// first; bits past the end read 0.
pub fn getbits(bytes: &[u8], bitpos: i32, bitlen: i32) -> i32 {
    let mut bitpos = bitpos + bitlen - 1;
    let mut v = 0;
    for _ in 0..bitlen {
        v <<= 1;
        let mask = 1u8 << (bitpos & 7);
        if bytes
            .get((bitpos >> 3) as usize)
            .is_some_and(|&b| b & mask != 0)
        {
            v |= 1;
        }
        bitpos -= 1;
    }

    v
}

/// `ipmi_sensor_name(name, len, typelen, bits, bitslen)`: decodes an SDR's name into
/// `name` (its length is the C's `len`), NUL-terminated; 0 when the record is too short.
pub fn ipmi_sensor_name(name: &mut [u8], typelen: u8, bits: &[u8], bitslen: i32) -> i32 {
    const BCDPLUS: &[u8; 16] = b"0123456789 -.:,_";
    let len = name.len() as i32;
    let mut out = 0usize;
    let mut put = |name: &mut [u8], ch: u8| {
        if let Some(slot) = name.get_mut(out) {
            *slot = ch;
        }
        out += 1;
    };

    let mut slen = i32::from(typelen & 0x1F);
    match typelen >> 6 {
        IPMI_NAME_UNICODE => {
            // unicode
        }

        IPMI_NAME_BCDPLUS => {
            // Characters are encoded in 4-bit BCDPLUS
            if len < slen * 2 + 1 {
                slen = (len >> 1) - 1;
            }
            if slen > bitslen {
                return 0;
            }
            for i in 0..slen.max(0) as usize {
                let b = bits.get(i).copied().unwrap_or(0);
                put(name, BCDPLUS[usize::from(b >> 4)]);
                put(name, BCDPLUS[usize::from(b & 0xF)]);
            }
        }

        IPMI_NAME_ASCII6BIT => {
            // Characters are encoded in 6-bit ASCII: 0x00 - 0x3F maps to 0x20 - 0x5F.
            // XXX: need to calculate max len: slen = 3/4 * len
            if len < slen + 1 {
                slen = len - 1;
            }
            if slen * 6 / 8 > bitslen {
                return 0;
            }
            let mut i = 0;
            while i < slen * 8 {
                put(name, (getbits(bits, i, 6) + i32::from(b' ')) as u8);
                i += 6;
            }
        }

        IPMI_NAME_ASCII8BIT => {
            // Characters are 8-bit ascii
            if len < slen + 1 {
                slen = len - 1;
            }
            if slen > bitslen {
                return 0;
            }
            for i in 0..slen.max(0) as usize {
                put(name, bits.get(i).copied().unwrap_or(0));
            }
        }

        _ => {}
    }
    put(name, 0);

    1
}

/// `ipow(val, exp)`: `val * 10^exp`, integer division for negative exponents.
pub fn ipow(mut val: i64, mut exp: i32) -> i64 {
    while exp > 0 {
        val = val.wrapping_mul(10);
        exp -= 1;
    }

    while exp < 0 {
        val /= 10;
        exp += 1;
    }

    val
}

/// `signextend(val, bits)`: sign-extends the `bits`-bit value `val`.
pub fn signextend(val: u64, bits: i32) -> i64 {
    let msk = (1i64 << (bits - 1)) - 1;

    ((val & !(msk as u64)) as i64).wrapping_neg() | val as i64
}

/// `ipmi_convert(v, s1, adj)`: a raw reading through the record's linear factors,
/// `y = (M * v + B * 10^K1) * 10^(K2 + adj)` (the linearization function is not applied).
pub fn ipmi_convert(v: u8, s1: &Sdrtype1, adj: i32) -> i64 {
    // Calculate linear reading variables
    let m = signextend(
        (u64::from(s1.m_tolerance & 0xC0) << 2) + u64::from(s1.m),
        10,
    ) as i16;
    let b = signextend((u64::from(s1.b_accuracy & 0xC0) << 2) + u64::from(s1.b), 10) as i16;
    let k1 = signextend(u64::from(s1.rbexp & 0xF), 4) as i8;
    let k2 = signextend(u64::from(s1.rbexp >> 4), 4) as i8;

    // Calculate sensor reading:
    //  y = L((M * v + (B * 10^K1)) * 10^(K2+adj)
    //
    // This commutes out to:
    //  y = L(M*v * 10^(K2+adj) + B * 10^(K1+K2+adj));
    // Linearization function: y = f(x) 0 : y = x 1 : y = ln(x) 2 : y = log10(x) 3 : y =
    // log2(x) 4 : y = e^x 5 : y = 10^x 6 : y = 2^x 7 : y = 1/x 8 : y = x^2 9 : y = x^3 10 :
    // y = square root(x) 11 : y = cube root(x)
    ipow(i64::from(m) * i64::from(v), i32::from(k2) + adj)
        + ipow(i64::from(b), i32::from(k1) + i32::from(k2) + adj)
}

/// `ipmi_sensor_status(sc, psensor, reading)`: converts the reading into the sensor's value
/// and returns its status.
pub fn ipmi_sensor_status(psensor: &IpmiSensor, reading: &[u8; 8]) -> SensorStatus {
    let s1 = Sdrtype1::read(psensor.i_sdr);
    let sensor = &psensor.i_sensor;

    // Get reading of sensor
    match sensor.r#type.get() {
        SENSOR_TEMP => {
            sensor
                .value
                .set(ipmi_convert(reading[0], &s1, 6) + 273_150_000);
        }

        SENSOR_VOLTS_DC | SENSOR_VOLTS_AC | SENSOR_AMPS | SENSOR_WATTS => {
            sensor.value.set(ipmi_convert(reading[0], &s1, 6));
        }

        SENSOR_FANRPM => {
            sensor.value.set(ipmi_convert(reading[0], &s1, 0));
            if (s1.units1 >> 3) & 0x7 == 0x3 {
                // RPS -> RPM
                sensor.value.set(sensor.value.get() * 60);
            }
        }
        _ => {}
    }

    // Return Sensor Status
    let etype = (psensor.etype << 8) + psensor.stype;
    match etype {
        IPMI_SENSOR_TYPE_TEMP
        | IPMI_SENSOR_TYPE_VOLT
        | IPMI_SENSOR_TYPE_CURRENT
        | IPMI_SENSOR_TYPE_FAN => {
            // non-recoverable threshold
            if reading[2] & ((1 << 5) | (1 << 2)) != 0 {
                return SENSOR_S_CRIT;
            // critical threshold
            } else if reading[2] & ((1 << 4) | (1 << 1)) != 0 {
                return SENSOR_S_CRIT;
            // non-critical threshold
            } else if reading[2] & ((1 << 3) | (1 << 0)) != 0 {
                return SENSOR_S_WARN;
            }
        }

        IPMI_SENSOR_TYPE_INTRUSION => {
            sensor.value.set(i64::from(reading[2] & 1));
            if reading[2] & 0x1 != 0 {
                return SENSOR_S_CRIT;
            }
        }

        IPMI_SENSOR_TYPE_PWRSUPPLY => {
            // Reading: 1 = present+powered, 0 = otherwise
            sensor.value.set(i64::from(reading[2] & 1));
            if reading[2] & 0x10 != 0 {
                // XXX: Need sysctl type for Power Supply types
                //   ok: power supply installed && powered
                // warn: power supply installed && !powered
                // crit: power supply !installed
                return SENSOR_S_CRIT;
            }
            if reading[2] & 0x08 != 0 {
                // Power supply AC lost
                return SENSOR_S_WARN;
            }
        }
        _ => {}
    }

    SENSOR_S_OK
}

/// `read_sensor(sc, psensor)`: reads the sensor from its owner; 0, or -1.
pub fn read_sensor<S: IpmiBmc>(sc: &S, psensor: &IpmiSensor) -> i32 {
    let s1 = Sdrtype1::read(psensor.i_sdr);
    let mut data = [0u8; 8];

    data[0] = psensor.i_num as u8;

    let mut c = IpmiCmd {
        c_rssa: i32::from(s1.owner_id),
        c_rslun: i32::from(s1.owner_lun),
        c_netfn: SE_NETFN,
        c_cmd: SE_GET_SENSOR_READING,
        c_txlen: 1,
        c_maxrxlen: data.len() as i32,
        c_rxlen: 0,
        c_data: Some(&mut data),
        c_ccode: 0,
    };
    ipmi_cmd(sc, &mut c);

    if c.c_ccode != 0 {
        return -1;
    }
    let sensor = &psensor.i_sensor;
    sensor.flags.set(sensor.flags.get() & !SENSOR_FINVALID);
    if data[1] & IPMI_INVALID_SENSOR != 0 || (data[1] & IPMI_DISABLED_SENSOR == 0 && data[0] == 0) {
        sensor.flags.set(sensor.flags.get() | SENSOR_FINVALID);
    }
    sensor.status.set(ipmi_sensor_status(psensor, &data));
    0
}

/// `ipmi_sensor_type(type, ext_type, units2, entity)`: the `sensors(9)` type of an IPMI
/// sensor, `None` for one the driver does not export.
pub fn ipmi_sensor_type(
    r#type: i32,
    ext_type: i32,
    units2: i32,
    entity: i32,
) -> Option<SensorType> {
    match units2 {
        IPMI_UNIT_TYPE_AMPS => return Some(SENSOR_AMPS),

        IPMI_UNIT_TYPE_RPM => return Some(SENSOR_FANRPM),

        // XXX sensors framework distinguishes AC/DC but ipmi does not
        IPMI_UNIT_TYPE_VOLTS => return Some(SENSOR_VOLTS_DC),

        IPMI_UNIT_TYPE_WATTS => return Some(SENSOR_WATTS),
        _ => {}
    }

    match (ext_type << 8) | r#type {
        IPMI_SENSOR_TYPE_TEMP => return Some(SENSOR_TEMP),

        IPMI_SENSOR_TYPE_PWRSUPPLY => {
            if entity == IPMI_ENTITY_PWRSUPPLY {
                return Some(SENSOR_INDICATOR);
            }
        }

        IPMI_SENSOR_TYPE_INTRUSION => return Some(SENSOR_INDICATOR),
        _ => {}
    }

    None
}

/// `add_sdr_sensor(sc, psdr, sdrlen)`: exports the sensors of a full or compact record; 1
/// if the record is kept (a sensor uses it), 0 if the caller may free it.
pub fn add_sdr_sensor<S: IpmiBmc>(sc: &S, psdr: &'static [u8]) -> i32 {
    let sdrlen = psdr.len() as i32;
    let mut name = [0u8; 64];

    let Some(&record_type) = psdr.get(3) else {
        return 0;
    };
    match record_type {
        IPMI_SDR_TYPEFULL => {
            let s1 = Sdrtype1::read(psdr);
            let bits = psdr.get(SDRTYPE1_NAME..).unwrap_or(&[]);
            if ipmi_sensor_name(&mut name, s1.typelen, bits, sdrlen - SDRTYPE1_NAME as i32) == 0 {
                return 0;
            }
            add_child_sensors(
                sc,
                psdr,
                1,
                i32::from(s1.sensor_num),
                i32::from(s1.sensor_type),
                i32::from(s1.event_code),
                0,
                i32::from(s1.entity_id),
                &name,
            )
        }

        IPMI_SDR_TYPECOMPACT => {
            let s2 = Sdrtype2::read(psdr);
            let bits = psdr.get(SDRTYPE2_NAME..).unwrap_or(&[]);
            if ipmi_sensor_name(&mut name, s2.typelen, bits, sdrlen - SDRTYPE2_NAME as i32) == 0 {
                return 0;
            }
            add_child_sensors(
                sc,
                psdr,
                i32::from(s2.share1 & 0xF),
                i32::from(s2.sensor_num),
                i32::from(s2.sensor_type),
                i32::from(s2.event_code),
                i32::from(s2.share2 & 0x7F),
                i32::from(s2.entity_id),
                &name,
            )
        }

        _ => 0,
    }
}

/// `add_child_sensors(sc, psdr, count, sensor_num, sensor_type, ext_type, sensor_base,
/// entity, name)`: one `sensors(9)` sensor per sensor of the record that answers a reading;
/// 1 if any was attached.
#[allow(clippy::too_many_arguments)] // the C function's nine parameters
pub fn add_child_sensors<S: IpmiBmc>(
    sc: &S,
    psdr: &'static [u8],
    count: i32,
    sensor_num: i32,
    sensor_type: i32,
    ext_type: i32,
    sensor_base: i32,
    entity: i32,
    name: &[u8],
) -> i32 {
    let s1 = Sdrtype1::read(psdr);
    let mut rc = 0;

    let Some(typ) = ipmi_sensor_type(sensor_type, ext_type, i32::from(s1.units2), entity) else {
        // dbg_printf(5, "Unknown sensor type ..."): IPMI_DEBUG is not configured.
        return 0;
    };
    let name = &name[..name.iter().position(|&b| b == 0).unwrap_or(name.len())];
    for idx in 0..count {
        let Some(mem) = malloc(size_of::<IpmiSensor>(), M_DEVBUF, M_NOWAIT | M_ZERO) else {
            break;
        };
        let mem = mem.cast::<IpmiSensor>();
        // SAFETY: a fresh, suitably aligned allocation of an `IpmiSensor`, written whole
        // before any use.
        unsafe {
            mem.as_ptr().write(IpmiSensor {
                i_sdr: psdr,
                i_num: sensor_num + idx,
                stype: sensor_type,
                etype: ext_type,
                i_sensor: Ksensor::new(),
                list: SlistEntry::new(),
            });
        }
        // SAFETY: initialised above; it is freed below or kept for good on the list.
        let psensor: &'static IpmiSensor = unsafe { mem.as_ref() };

        // Initialize BSD Sensor info
        psensor.i_sensor.r#type.set(typ);
        let mut desc = [0u8; 32];
        if count > 1 {
            snprintf(
                &mut desc,
                format_args!("{} - {}", Str(name), sensor_base + idx),
            );
        } else {
            libkern::strlcpy(&mut desc, name);
        }
        psensor.i_sensor.desc.set(desc);

        if read_sensor(sc, psensor) == 0 {
            // SAFETY: a new element, on no list; the sensor thread owns the list.
            unsafe { IPMI_SENSOR_LIST.0.insert_head(psensor) };
            sensor_attach(sc.sc_sensordev(), &psensor.i_sensor);
            rc = 1;
        } else {
            free(mem.cast::<u8>(), M_DEVBUF, size_of::<IpmiSensor>());
        }
    }

    rc
}

/// `ipmi_refresh_sensors(sc)`: rereads the next sensor of the list.
pub fn ipmi_refresh_sensors(sc: &IpmiSoftc) {
    let Some(first) = IPMI_SENSOR_LIST.0.first() else {
        return;
    };

    let next = sc
        .current_sensor
        .get()
        .and_then(|s| SlistHead::<IpmiSensorsList>::next(s))
        .unwrap_or(first);
    // SAFETY: list elements are never freed.
    let next: &'static IpmiSensor = unsafe { &*ptr::from_ref(next) };
    sc.current_sensor.set(Some(next));

    if read_sensor(sc, next) != 0 {
        // dbg_printf(1, "error reading ..."): IPMI_DEBUG is not configured.
    }
}

/// `ipmi_map_regs(sc, ia)`: picks the interface and maps its registers; 0, or -1.
pub fn ipmi_map_regs(sc: &IpmiSoftc, ia: &IpmiAttachArgs) -> i32 {
    if sc.sc_if.get().is_some_and(|i| i.nregs == 0) {
        return 0;
    }

    sc.sc_if.set(ipmi_get_if(ia.iaa_if_type));
    let Some(ifp) = sc.sc_if.get() else {
        return -1;
    };

    let iot = if ia.iaa_if_iotype == b'i' {
        ia.iaa_iot
    } else {
        ia.iaa_memt
    };
    sc.sc_iot.set(iot);

    sc.sc_if_rev.set(ia.iaa_if_rev);
    sc.sc_if_iosize.set(ia.iaa_if_iosize);
    sc.sc_if_iospacing.set(ia.iaa_if_iospacing);
    let size = (ifp.nregs * sc.sc_if_iospacing.get()).max(0) as BusSize;
    let mapped = iot.and_then(|t| {
        // SAFETY: the BMC's register window, from the firmware's description, driven by this
        // driver alone.
        unsafe { bus_space_map(t, ia.iaa_if_iobase, size, 0) }.ok()
    });
    let Some(ioh) = mapped else {
        kprintf!(
            "{}: bus_space_map({:x} {:x} 0) failed\n",
            sc.devname(),
            ia.iaa_if_iobase,
            size
        );
        return -1;
    };
    sc.sc_ioh.set(Some(ioh));
    0
}

/// `ipmi_unmap_regs(sc)`.
pub fn ipmi_unmap_regs(sc: &IpmiSoftc) {
    let Some(ifp) = sc.sc_if.get() else {
        return;
    };
    if ifp.nregs > 0
        && let (Some(t), Some(h)) = (sc.sc_iot.get(), sc.sc_ioh.take())
    {
        bus_space_unmap(t, h, (ifp.nregs * sc.sc_if_iospacing.get()) as BusSize);
    }
}

/// `ipmi_poll_thread(arg)`: reads the SDRs and adds the sensors, then rereads one every
/// `SENSOR_REFRESH_RATE` seconds while running.
pub fn ipmi_poll_thread(arg: *mut c_void) {
    // SAFETY: `ipmi_create_thread` passes the softc's `IpmiThread`, never freed.
    let thread = unsafe { &*arg.cast_const().cast::<IpmiThread>() };
    let sc = thread.sc;

    // Scan SDRs, add sensors
    let mut rec: u16 = 0;
    while rec != 0xFFFF {
        if get_sdr(sc, rec, &mut rec) != 0 {
            ipmi_unmap_regs(sc);
            kprintf!("{}: no SDRs IPMI disabled\n", sc.devname());
            kthread_exit(0);
        }
        let _ = tsleep_nsec(ptr::from_ref(sc), PWAIT, "ipmirun", msec_to_nsec(1));
    }

    // initialize sensor list for thread
    let Some(first) = IPMI_SENSOR_LIST.0.first() else {
        kthread_exit(0);
    };
    // SAFETY: list elements are never freed.
    sc.current_sensor
        .set(Some(unsafe { &*ptr::from_ref(first) }));

    let mut xname = [0u8; 16];
    libkern::strlcpy(&mut xname, sc.devname().as_bytes());
    sc.sc_sensordev.xname.set(xname);
    // SAFETY: the softc is the device's, never detached.
    sensordev_install(unsafe { &*ptr::from_ref(&sc.sc_sensordev) });

    while thread.running.load(Ordering::Relaxed) != 0 {
        ipmi_refresh_sensors(sc);
        let _ = tsleep_nsec(
            ptr::from_ref(thread),
            PWAIT,
            "ipmi_poll",
            sec_to_nsec(SENSOR_REFRESH_RATE),
        );
    }

    kthread_exit(0);
}

/// `ipmi_create_thread(arg)`: starts the sensor thread.
pub fn ipmi_create_thread(arg: *mut c_void) {
    // SAFETY: `ipmi_attach_common` passes the softc, never freed.
    let sc = unsafe { &*arg.cast_const().cast::<IpmiSoftc>() };
    let Some(thread) = sc.sc_thread.get() else {
        return;
    };

    if kthread_create(
        ipmi_poll_thread,
        ptr::from_ref(thread).cast_mut().cast(),
        sc.devname().as_bytes(),
    )
    .is_err()
    {
        kprintf!(
            "{}: unable to create run thread, ipmi disabled\n",
            sc.devname()
        );
    }
}

/// `ipmi_attach_common(sc, ia)`: maps the registers, starts the sensor thread, prints the
/// interface, registers the watchdog and readies `/dev/ipmi` and the command task queue.
pub fn ipmi_attach_common(sc: &'static IpmiSoftc, ia: &IpmiAttachArgs) {
    // Map registers
    if ipmi_map_regs(sc, ia) != 0 {
        // See the module's deviations: the C carries on.
        kprintf!(": unable to map interface type {}\n", ia.iaa_if_type);
        return;
    }
    let Some(ifp) = sc.sc_if.get() else {
        return;
    };

    let Some(mem) = malloc(size_of::<IpmiThread>(), M_DEVBUF, M_NOWAIT) else {
        kprintf!(": unable to allocate thread\n");
        return;
    };
    let mem = mem.cast::<IpmiThread>();
    // SAFETY: a fresh, suitably aligned allocation of an `IpmiThread`, written whole.
    unsafe {
        mem.as_ptr().write(IpmiThread {
            sc,
            running: AtomicI32::new(1),
        });
    }
    // SAFETY: initialised above and never freed.
    sc.sc_thread.set(Some(unsafe { mem.as_ref() }));

    // Setup threads
    kthread_create_deferred(ipmi_create_thread, ptr::from_ref(sc).cast_mut().cast());

    kprintf!(
        ": version {}.{} interface {}",
        ia.iaa_if_rev >> 4,
        ia.iaa_if_rev & 0xF,
        ifp.name
    );
    if ifp.nregs > 0 {
        kprintf!(
            " {}base 0x{:x}/{:x} spacing {}",
            if ia.iaa_if_iotype == b'i' {
                "io"
            } else {
                "mem"
            },
            ia.iaa_if_iobase,
            ia.iaa_if_iospacing * ifp.nregs,
            ia.iaa_if_iospacing
        );
    }
    if ia.iaa_if_irq != -1 {
        kprintf!(" irq {}", ia.iaa_if_irq);
    }
    kprintf!("\n");

    // setup flag to exclude iic
    IPMI_ENABLED.store(1, Ordering::Relaxed);

    // Setup Watchdog timer
    sc.sc_wdog_period.set(0);
    task_set(
        &sc.sc_wdog_tickle_task,
        ipmi_watchdog_tickle,
        ptr::from_ref(sc).cast_mut().cast(),
    );
    wdog_register(ipmi_watchdog, ptr::from_ref(sc).cast_mut().cast());

    // SAFETY: the softc is the device's, never detached, and `dv_xname` is not written
    // after attach.
    let name: &'static str = unsafe { &*ptr::from_ref(sc.devname()) };
    rw_init(&sc.sc_ioctl.lock, name);
    // SAFETY: nothing can reach `/dev/ipmi` before the attach returns.
    let st = unsafe { &mut *sc.sc_ioctl.st.get() };
    st.req.msgid = -1;
    st.cmd.c_ccode = -1;

    sc.sc_cmd_taskq
        .set(taskq_create(b"ipmicmd", 1, IPL_MPFLOOR, TASKQ_MPSAFE));
}

/// `ipmi_activate(self, act)`: disarms the watchdog at powerdown.
pub fn ipmi_activate(self_: &Device, act: i32) -> Result<(), Errno> {
    if act == DVACT_POWERDOWN {
        wdog_shutdown(ptr::from_ref(self_).cast_mut().cast());
    }

    Ok(())
}

/// `ipmilookup(dev)`: the softc of `/dev/ipmiN`.
pub fn ipmilookup(dev: Dev) -> Option<&'static IpmiSoftc> {
    let dv = device_lookup(&IPMI_CD, minor(dev) as i32)?;
    // SAFETY: every ipmi device is an `IpmiSoftc` or starts with one (`IpmiAcpiSoftc`),
    // attached for good.
    Some(unsafe { &*dv.as_ptr().cast_const().cast::<IpmiSoftc>() })
}

/// `ipmiopen(dev, flags, mode, p)`.
pub fn ipmiopen(dev: Dev, _flags: i32, _mode: i32, _p: &Proc) -> Result<(), Errno> {
    if ipmilookup(dev).is_none() {
        return Err(Errno::ENXIO);
    }
    Ok(())
}

/// `ipmiclose(dev, flags, mode, p)`.
pub fn ipmiclose(dev: Dev, _flags: i32, _mode: i32, _p: Option<&Proc>) -> Result<(), Errno> {
    if ipmilookup(dev).is_none() {
        return Err(Errno::ENXIO);
    }
    Ok(())
}

/// A `T` read from the start of an ioctl's argument buffer.
fn ioctl_arg<T: Copy + Default>(data: &[u8]) -> Result<T, Errno> {
    if data.len() < size_of::<T>() {
        return Err(Errno::EINVAL);
    }
    // SAFETY: `T` is one of the `#[repr(C)]` argument structures above or an integer, made
    // of integers (user addresses are `usize`), so any bytes are a valid value; the buffer
    // holds `size_of::<T>()` of them (checked above), read unaligned.
    Ok(unsafe { ptr::read_unaligned(data.as_ptr().cast::<T>()) })
}

/// Writes `bytes` at `off` in an ioctl's argument buffer.
fn ioctl_put(data: &mut [u8], off: usize, bytes: &[u8]) -> Result<(), Errno> {
    data.get_mut(off..off + bytes.len())
        .ok_or(Errno::EINVAL)?
        .copy_from_slice(bytes);
    Ok(())
}

/// `ipmiioctl(dev, cmd, data, flag, proc)`: `/dev/ipmi`'s requests.
pub fn ipmiioctl(
    dev: Dev,
    cmd: u64,
    data: &mut [u8],
    _flag: i32,
    _proc: &Proc,
) -> Result<(), Errno> {
    let Some(sc) = ipmilookup(dev) else {
        return Err(Errno::ENXIO);
    };

    rw_enter_write(&sc.sc_ioctl.lock);
    // SAFETY: `sc_ioctl.lock` is held for writing until the end of this function.
    let st = unsafe { &mut *sc.sc_ioctl.st.get() };

    let (rc, reset) = ipmiioctl_locked(sc, st, cmd, data);
    if reset {
        // reset:
        st.req.msgid = -1;
        st.cmd.c_ccode = -1;
    }
    // done:
    rw_exit_write(&sc.sc_ioctl.lock);
    rc
}

/// `ipmiioctl`'s switch under the lock: the result, and whether to reset the request (the
/// C's `goto reset`).
fn ipmiioctl_locked(
    sc: &IpmiSoftc,
    st: &mut IpmiIoctlState,
    cmd: u64,
    data: &mut [u8],
) -> (Result<(), Errno>, bool) {
    st.cmd.c_maxrxlen = st.buf.len() as i32;

    match cmd {
        IPMICTL_SEND_COMMAND => {
            let req: IpmiReq = match ioctl_arg(data) {
                Ok(req) => req,
                Err(e) => return (Err(e), true),
            };
            if req.msgid == -1 {
                return (Err(Errno::EINVAL), true);
            }
            if st.req.msgid != -1 {
                return (Err(Errno::EBUSY), true);
            }
            let len = i32::from(req.msg.data_len);
            if len > st.cmd.c_maxrxlen {
                return (Err(Errno::E2BIG), true);
            }
            st.req = req;
            st.cmd.c_ccode = -1;
            if let Err(e) = copyin(req.msg.data, &mut st.buf[..len as usize]) {
                return (Err(e), true);
            }
            kassert!(st.cmd.c_ccode == -1);

            // Execute a command synchronously.
            st.cmd.c_netfn = i32::from(req.msg.netfn);
            st.cmd.c_cmd = i32::from(req.msg.cmd);
            st.cmd.c_txlen = i32::from(req.msg.data_len);
            st.cmd.c_rxlen = 0;
            let mut c = IpmiCmd {
                c_rssa: st.cmd.c_rssa,
                c_rslun: st.cmd.c_rslun,
                c_netfn: st.cmd.c_netfn,
                c_cmd: st.cmd.c_cmd,
                c_txlen: st.cmd.c_txlen,
                c_maxrxlen: st.cmd.c_maxrxlen,
                c_rxlen: st.cmd.c_rxlen,
                c_data: Some(&mut st.buf),
                c_ccode: st.cmd.c_ccode,
            };
            ipmi_cmd(sc, &mut c);
            let (txlen, maxrxlen, rxlen, ccode) = (c.c_txlen, c.c_maxrxlen, c.c_rxlen, c.c_ccode);
            st.cmd.c_txlen = txlen;
            st.cmd.c_maxrxlen = maxrxlen;
            st.cmd.c_rxlen = rxlen;
            st.cmd.c_ccode = ccode;
            (Ok(()), false)
        }
        IPMICTL_RECEIVE_MSG_TRUNC | IPMICTL_RECEIVE_MSG => {
            let recv: IpmiRecv = match ioctl_arg(data) {
                Ok(recv) => recv,
                Err(e) => return (Err(e), true),
            };
            if st.req.msgid == -1 {
                return (Err(Errno::EINVAL), true);
            }
            if st.cmd.c_ccode == -1 {
                return (Err(Errno::EAGAIN), true);
            }
            let ccode = (st.cmd.c_ccode & 0xff) as u8;
            if let Err(e) = copyout(&[ccode], recv.msg.data) {
                return (Err(e), true);
            }

            // Return a command result.
            let msg = offset_of!(IpmiRecv, msg);
            let fields: [(usize, &[u8]); 5] = [
                (
                    offset_of!(IpmiRecv, recv_type),
                    &IPMI_RESPONSE_RECV_TYPE.to_ne_bytes(),
                ),
                (offset_of!(IpmiRecv, msgid), &st.req.msgid.to_ne_bytes()),
                (msg + offset_of!(IpmiMsg, netfn), &[st.req.msg.netfn]),
                (msg + offset_of!(IpmiMsg, cmd), &[st.req.msg.cmd]),
                (
                    msg + offset_of!(IpmiMsg, data_len),
                    &((st.cmd.c_rxlen + 1) as u16).to_ne_bytes(),
                ),
            ];
            for (off, bytes) in fields {
                if let Err(e) = ioctl_put(data, off, bytes) {
                    return (Err(e), true);
                }
            }

            let n = (st.cmd.c_rxlen.max(0) as usize).min(st.buf.len());
            // Always reset state after command completion.
            (copyout(&st.buf[..n], recv.msg.data + 1), true)
        }
        IPMICTL_SET_MY_ADDRESS_CMD => match ioctl_arg::<i32>(data) {
            Ok(iv) if (0..=RSSA_MASK).contains(&iv) => {
                st.cmd.c_rssa = iv;
                (Ok(()), false)
            }
            Ok(_) => (Err(Errno::EINVAL), true),
            Err(e) => (Err(e), true),
        },
        IPMICTL_GET_MY_ADDRESS_CMD => (ioctl_put(data, 0, &st.cmd.c_rssa.to_ne_bytes()), false),
        IPMICTL_SET_MY_LUN_CMD => match ioctl_arg::<i32>(data) {
            Ok(iv) if (0..=LUN_MASK).contains(&iv) => {
                st.cmd.c_rslun = iv;
                (Ok(()), false)
            }
            Ok(_) => (Err(Errno::EINVAL), true),
            Err(e) => (Err(e), true),
        },
        IPMICTL_GET_MY_LUN_CMD => (ioctl_put(data, 0, &st.cmd.c_rslun.to_ne_bytes()), false),
        // IPMICTL_SET_GETS_EVENTS_CMD, IPMICTL_REGISTER_FOR_CMD,
        // IPMICTL_UNREGISTER_FOR_CMD and the rest: nothing to do.
        _ => (Ok(()), false),
    }
}

/// `ipmi_watchdog(arg, period)`: `kern_watchdog.c`'s callback: sets the period (at least
/// `MIN_PERIOD` seconds, 0 disarms) or, unchanged, tickles the BMC's timer from `systq`.
pub fn ipmi_watchdog(arg: *mut c_void, period: i32) -> i32 {
    // SAFETY: `ipmi_attach_common` registered the softc, never freed.
    let sc = unsafe { &*arg.cast_const().cast::<IpmiSoftc>() };
    let mut period = period;

    if sc.sc_wdog_period.get() == period {
        if period != 0 {
            // SAFETY: the softc lives for good, so does its task.
            let t: &'static Task = unsafe { &*ptr::from_ref(&sc.sc_wdog_tickle_task) };
            let _ = task_del(SYSTQ, t);
            let res = task_add(SYSTQ, t);
            kassert!(res);
        }
        return period;
    }

    if period < MIN_PERIOD && period > 0 {
        period = MIN_PERIOD;
    }
    sc.sc_wdog_period.set(period);
    ipmi_watchdog_set(arg);
    kprintf!(
        "{}: watchdog {}abled\n",
        sc.devname(),
        if period == 0 { "dis" } else { "en" }
    );
    period
}

/// `ipmi_watchdog_tickle(arg)`: Reset Watchdog Timer.
pub fn ipmi_watchdog_tickle(arg: *mut c_void) {
    // SAFETY: the task's argument is the softc, never freed.
    let sc = unsafe { &*arg.cast_const().cast::<IpmiSoftc>() };

    let mut c = IpmiCmd {
        c_rssa: BMC_SA,
        c_rslun: BMC_LUN,
        c_netfn: APP_NETFN,
        c_cmd: APP_RESET_WATCHDOG,
        c_txlen: 0,
        c_maxrxlen: 0,
        c_rxlen: 0,
        c_data: None,
        c_ccode: 0,
    };
    ipmi_cmd(sc, &mut c);
}

/// `ipmi_watchdog_set(arg)`: Get Watchdog Timer, then Set Watchdog Timer with the softc's
/// period, "don't stop" and the reboot action (both cleared for 0).
pub fn ipmi_watchdog_set(arg: *mut c_void) {
    // SAFETY: the softc, never freed.
    let sc = unsafe { &*arg.cast_const().cast::<IpmiSoftc>() };
    let mut wdog = [0u8; IPMI_GET_WDOG_MAX];

    let mut c = IpmiCmd {
        c_rssa: BMC_SA,
        c_rslun: BMC_LUN,
        c_netfn: APP_NETFN,
        c_cmd: APP_GET_WATCHDOG_TIMER,
        c_txlen: 0,
        c_maxrxlen: IPMI_GET_WDOG_MAX as i32,
        c_rxlen: 0,
        c_data: Some(&mut wdog),
        c_ccode: 0,
    };
    ipmi_cmd(sc, &mut c);

    // Period is 10ths/sec
    let period = sc.sc_wdog_period.get();
    let timo = (period.wrapping_mul(10) as u16).to_le_bytes();

    wdog[IPMI_SET_WDOG_TIMOL..IPMI_SET_WDOG_TIMOL + 2].copy_from_slice(&timo);
    wdog[IPMI_SET_WDOG_TIMER] &= !IPMI_WDOG_DONTSTOP;
    wdog[IPMI_SET_WDOG_TIMER] |= if period == 0 { 0 } else { IPMI_WDOG_DONTSTOP };
    wdog[IPMI_SET_WDOG_ACTION] &= !IPMI_WDOG_MASK;
    wdog[IPMI_SET_WDOG_ACTION] |= if period == 0 {
        IPMI_WDOG_DISABLED
    } else {
        IPMI_WDOG_REBOOT
    };

    let mut c = IpmiCmd {
        c_rssa: BMC_SA,
        c_rslun: BMC_LUN,
        c_netfn: APP_NETFN,
        c_cmd: APP_SET_WATCHDOG_TIMER,
        c_txlen: IPMI_SET_WDOG_MAX as i32,
        c_maxrxlen: 0,
        c_rxlen: 0,
        c_data: Some(&mut wdog),
        c_ccode: 0,
    };
    ipmi_cmd(sc, &mut c);
}

/// `ipmi_match(parent, match, aux)` (amd64's mainbus): the BMC answers Get Device ID through
/// the interface the probe found.
pub fn ipmi_match(_parent: Option<&Device>, match_: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: mainbus hands ipmi its `IpmiAttachArgs` (`mba_iaa`).
    let ia = unsafe { &*aux.cast_const().cast::<IpmiAttachArgs>() };
    let cf = match_.cfdata();
    let mut cmd = [0u8; 32];
    let mut rv = 0;

    if ia.iaa_name != cf.cf_driver.cd_name {
        return 0;
    }

    // XXX local softc is wrong wrong wrong
    let Some(mem) = malloc(size_of::<IpmiSoftc>(), M_TEMP, M_WAITOK | M_ZERO) else {
        return 0;
    };
    // SAFETY: a zeroed allocation of an `IpmiSoftc`, a valid value (`Softc`'s contract).
    let sc = unsafe { mem.cast::<IpmiSoftc>().as_ref() };
    let mut xname = [0u8; 16];
    libkern::strlcpy(&mut xname, b"ipmi0");
    sc.sc_dev.dv_xname.set(xname);

    // Map registers
    if ipmi_map_regs(sc, ia) == 0 {
        if let Some(ifp) = sc.sc_if.get() {
            (ifp.probe)(sc);
        }

        // Identify BMC device early to detect lying bios
        let mut c = IpmiCmd {
            c_rssa: BMC_SA,
            c_rslun: BMC_LUN,
            c_netfn: APP_NETFN,
            c_cmd: APP_GET_DEVICE_ID,
            c_txlen: 0,
            c_maxrxlen: cmd.len() as i32,
            c_rxlen: 0,
            c_data: Some(&mut cmd),
            c_ccode: 0,
        };
        ipmi_cmd(sc, &mut c);

        // dbg_dump(1, "bmc data", ...): IPMI_DEBUG is not configured.
        rv = 1; // GETID worked, we got IPMI
        ipmi_unmap_regs(sc);
    }

    free(mem, M_TEMP, size_of::<IpmiSoftc>());

    rv
}

/// `ipmi_attach(parent, self, aux)`.
pub fn ipmi_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: `ipmi_ca` makes `IpmiSoftc`s, never detached.
    let sc: &'static IpmiSoftc = unsafe { &*ptr::from_ref(self_.softc::<IpmiSoftc>()) };
    // SAFETY: as in `ipmi_match`.
    let ia = unsafe { &*aux.cast_const().cast::<IpmiAttachArgs>() };
    ipmi_attach_common(sc, ia);
}

/// `scan_sig(start, end, skip, len, data)`: the physical address of the first `data` at a
/// multiple of `skip` from `start` below `end` in memory space `memt`, if any (see the
/// module's deviations).
pub fn scan_sig(
    memt: BusSpaceTag,
    start: BusAddr,
    end: BusAddr,
    skip: usize,
    data: &[u8],
) -> Option<BusAddr> {
    if end <= start {
        return None;
    }
    let size = end - start + data.len();
    // SAFETY: the BIOS area (ROM and reserved memory), read only.
    let h = unsafe { bus_space_map(memt, start, size, 0) }.ok()?;

    let mut found = None;
    let mut off = 0;
    while start + off < end {
        if (0..data.len()).all(|i| bus_space_read_1(memt, h, off + i) == data[i]) {
            found = Some(start + off);
            break;
        }

        off += skip;
    }

    bus_space_unmap(memt, h, size);
    found
}

/// `ipmi_smbios_probe(pipmi, ia)`: the attach arguments from SMBIOS's IPMI record.
pub fn ipmi_smbios_probe(pipmi: &SmbiosIpmi, ia: &mut IpmiAttachArgs) {
    let flags = pipmi.smipmi_base_flags;
    let base = pipmi.smipmi_base_address;

    ia.iaa_if_type = i32::from(pipmi.smipmi_if_type);
    ia.iaa_if_rev = i32::from(pipmi.smipmi_if_rev);
    ia.iaa_if_irq = if flags & SMIPMI_FLAG_IRQEN != 0 {
        i32::from(pipmi.smipmi_irq)
    } else {
        -1
    };
    ia.iaa_if_irqlvl = if flags & SMIPMI_FLAG_IRQLVL != 0 {
        IST_LEVEL
    } else {
        IST_EDGE
    };
    ia.iaa_if_iosize = 1;

    // SMIPMI_FLAG_IFSPACING
    match (flags >> 6) & 0x3 {
        IPMI_IOSPACING_BYTE => ia.iaa_if_iospacing = 1,

        IPMI_IOSPACING_DWORD => ia.iaa_if_iospacing = 4,

        IPMI_IOSPACING_WORD => ia.iaa_if_iospacing = 2,

        _ => {
            ia.iaa_if_iospacing = 1;
            kprintf!("ipmi: unknown register spacing\n");
        }
    }

    // Calculate base address (PCI BAR format)
    if base & 0x1 != 0 {
        ia.iaa_if_iotype = b'i';
        ia.iaa_if_iobase = (base & !0x1) as BusAddr;
    } else {
        ia.iaa_if_iotype = b'm';
        ia.iaa_if_iobase = (base & !0xF) as BusAddr;
    }
    if flags & SMIPMI_FLAG_ODDOFFSET != 0 {
        ia.iaa_if_iobase += 1;
    }

    if flags == 0x7f {
        // IBM 325 eServer workaround
        ia.iaa_if_iospacing = 1;
        ia.iaa_if_iobase = base as BusAddr;
        ia.iaa_if_iotype = b'i';
    }
}

/// `ipmi_probe(aux)` (amd64's mainbus): 1 when SMBIOS or a BIOS signature describes a BMC,
/// with the attach arguments filled in.
#[cfg(machine_x86)]
pub fn ipmi_probe(ia: &mut IpmiAttachArgs) -> i32 {
    let mut tbl = Smbtable::default();
    if smbios_find_table(SMBIOS_TYPE_IPMIDEV, &mut tbl) != 0 {
        // SAFETY: `smbios_find_table` points `tblhdr` at the formatted area of a type 38
        // structure in the table bios0 mapped read-only for good; it is read as the C reads
        // it (a record shorter than `struct smbios_ipmi` reads on into its strings).
        let pipmi = unsafe { tbl.tblhdr.cast::<SmbiosIpmi>().read_unaligned() };
        ipmi_smbios_probe(&pipmi, ia);
    } else {
        let Some(memt) = ia.iaa_memt else {
            return 0;
        };
        // XXX hack to find Dell PowerEdge 8450
        let Some(pa) = scan_sig(memt, 0xC0000, 0xFFFFF, 16, b"IPMI") else {
            // no IPMI found
            return 0;
        };

        // we have an IPMI signature, fill in attach arg structure
        // SAFETY: as in `scan_sig`: the BIOS area, read only.
        let Ok(h) = (unsafe { bus_space_map(memt, pa, size_of::<DmdIpmi>(), 0) }) else {
            return 0;
        };
        ia.iaa_if_type = i32::from(bus_space_read_1(memt, h, offset_of!(DmdIpmi, dmd_if_type)));
        ia.iaa_if_rev = i32::from(bus_space_read_1(memt, h, offset_of!(DmdIpmi, dmd_if_rev)));
        bus_space_unmap(memt, h, size_of::<DmdIpmi>());
    }

    1
}

const _: () = {
    assert!(size_of::<IpmiReq>() == 40);
    assert!(size_of::<IpmiRecv>() == 48);
    assert!(size_of::<SmbiosIpmi>() == 14);
    assert!(size_of::<DmdIpmi>() == 8);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;
    use core::cell::{Cell, RefCell};
    use std::boxed::Box;
    use std::vec;
    use std::vec::Vec;

    /// A BMC's command processor: Get Device ID, the SDR repository, sensor readings and the
    /// watchdog commands. Requests and responses are `[netfn << 2 | lun, cmd, ...]`.
    #[derive(Default)]
    struct Bmc {
        sdrs: Vec<Vec<u8>>,
        readings: Vec<(u8, [u8; 3])>,
        log: RefCell<Vec<Vec<u8>>>,
        wdog: RefCell<Vec<u8>>,
    }

    impl Bmc {
        fn respond(&self, req: &[u8]) -> Vec<u8> {
            self.log.borrow_mut().push(req.to_vec());
            let netfn = i32::from(req[0] >> 2);
            let cmd = i32::from(req[1]);
            let data = &req[2..];
            let mut rsp = vec![(((netfn | 1) << 2) as u8) | (req[0] & 3), req[1]];
            match (netfn, cmd) {
                (APP_NETFN, APP_GET_DEVICE_ID) => {
                    rsp.extend_from_slice(&[0, 0x20, 0x01, 0x02, 0x00, 0x02, 0x3f])
                }
                (STORAGE_NETFN, STORAGE_RESERVE_SDR) => rsp.extend_from_slice(&[0, 0x01, 0x00]),
                (STORAGE_NETFN, STORAGE_GET_SDR) => {
                    assert_eq!(data.len(), 6, "Get SDR takes six bytes");
                    assert_eq!(u16::from_le_bytes([data[0], data[1]]), 1, "the reservation");
                    let id = usize::from(u16::from_le_bytes([data[2], data[3]]));
                    let idx = id.saturating_sub(1);
                    let rec = &self.sdrs[idx];
                    let next: u16 = if idx + 1 < self.sdrs.len() {
                        (idx + 2) as u16
                    } else {
                        0xFFFF
                    };
                    let (off, len) = (usize::from(data[4]), usize::from(data[5]));
                    rsp.push(0);
                    rsp.extend_from_slice(&next.to_le_bytes());
                    rsp.extend_from_slice(&rec[off..(off + len).min(rec.len())]);
                }
                (SE_NETFN, SE_GET_SENSOR_READING) => {
                    match self.readings.iter().find(|r| r.0 == data[0]) {
                        Some((_, r)) => {
                            rsp.push(0);
                            rsp.extend_from_slice(r);
                        }
                        None => rsp.push(0xcb),
                    }
                }
                (APP_NETFN, APP_GET_WATCHDOG_TIMER) => {
                    rsp.extend_from_slice(&[0, 0x44, 0, 0, 0, 0x2c, 0x01, 0, 0])
                }
                (APP_NETFN, APP_SET_WATCHDOG_TIMER) => {
                    *self.wdog.borrow_mut() = data.to_vec();
                    rsp.push(0);
                }
                _ => rsp.push(0xc1),
            }
            rsp
        }
    }

    /// The system interface a fake speaks, with its registers' state.
    enum Wire {
        Kcs {
            state: u8,
            obf: bool,
            out: u8,
            end: bool,
            req: Vec<u8>,
            rsp: Vec<u8>,
            pos: usize,
        },
        Smic {
            ctrl: u8,
            din: u8,
            status: u8,
            out: u8,
            rx: bool,
            req: Vec<u8>,
            rsp: Vec<u8>,
            pos: usize,
        },
        Bt {
            b2h: bool,
            hbusy: bool,
            mask: u8,
            wbuf: Vec<u8>,
            rbuf: Vec<u8>,
            pos: usize,
        },
    }

    /// A softc whose registers are a simulated BMC.
    struct Fake {
        bmc: Bmc,
        wire: RefCell<Wire>,
        xfer: RefCell<IpmiXfer>,
        dev: Ksensordev,
        polls: Cell<u32>,
    }

    impl Fake {
        fn new(iftype: i32, bmc: Bmc) -> &'static Fake {
            let wire = match iftype {
                IPMI_IF_KCS => Wire::Kcs {
                    state: 0,
                    obf: false,
                    out: 0,
                    end: false,
                    req: Vec::new(),
                    rsp: Vec::new(),
                    pos: 0,
                },
                IPMI_IF_SMIC => Wire::Smic {
                    ctrl: 0,
                    din: 0,
                    status: 0,
                    out: 0,
                    rx: false,
                    req: Vec::new(),
                    rsp: Vec::new(),
                    pos: 0,
                },
                _ => Wire::Bt {
                    b2h: false,
                    hbusy: false,
                    mask: 0,
                    wbuf: Vec::new(),
                    rbuf: Vec::new(),
                    pos: 0,
                },
            };
            Box::leak(Box::new(Fake {
                bmc,
                wire: RefCell::new(wire),
                xfer: RefCell::new(IpmiXfer::new()),
                dev: Ksensordev::new(),
                polls: Cell::new(0),
            }))
        }
    }

    impl IpmiBmc for Fake {
        fn bmc_read(&self, offset: i32) -> u8 {
            self.polls.set(self.polls.get() + 1);
            match &mut *self.wire.borrow_mut() {
                Wire::Kcs {
                    state, obf, out, ..
                } => match offset {
                    0 => {
                        *obf = false;
                        *out
                    }
                    _ => *state | if *obf { KCS_OBF } else { 0 },
                },
                Wire::Smic {
                    status, out, rx, ..
                } => match offset {
                    0 => *out,
                    1 => *status,
                    _ => SMIC_TX_DATA_RDY | if *rx { SMIC_RX_DATA_RDY } else { 0 },
                },
                Wire::Bt {
                    b2h,
                    hbusy,
                    mask,
                    rbuf,
                    pos,
                    ..
                } => match offset {
                    0 => {
                        (if *b2h { BT_BMC2HOST_ATN } else { 0 })
                            | (if *hbusy { BT_HOST_BUSY } else { 0 })
                    }
                    1 => {
                        let v = rbuf.get(*pos).copied().unwrap_or(0);
                        *pos += 1;
                        v
                    }
                    _ => *mask,
                },
            }
        }

        fn bmc_write(&self, offset: i32, val: u8) {
            match &mut *self.wire.borrow_mut() {
                Wire::Kcs {
                    state,
                    obf,
                    out,
                    end,
                    req,
                    rsp,
                    pos,
                } => match (offset, val) {
                    (1, KCS_WRITE_START) => {
                        req.clear();
                        *state = KCS_WRITE_STATE as u8;
                        *end = false;
                    }
                    (1, KCS_WRITE_END) => *end = true,
                    (1, _) => panic!("unexpected KCS control code {val:#x}"),
                    (_, v) if *state == KCS_WRITE_STATE as u8 => {
                        req.push(v);
                        if *end {
                            *rsp = self.bmc.respond(req);
                            *pos = 0;
                            *state = KCS_READ_STATE as u8;
                            *out = rsp[0];
                            *obf = true;
                        }
                    }
                    (_, KCS_READ_NEXT) if *state == KCS_READ_STATE as u8 => {
                        *pos += 1;
                        if *pos < rsp.len() {
                            *out = rsp[*pos];
                        } else {
                            *state = KCS_IDLE_STATE as u8;
                            *out = 0;
                        }
                        *obf = true;
                    }
                    _ => panic!("unexpected KCS data write {val:#x}"),
                },
                Wire::Smic {
                    ctrl,
                    din,
                    status,
                    out,
                    rx,
                    req,
                    rsp,
                    pos,
                } => match offset {
                    0 => *din = val,
                    1 => *ctrl = val,
                    _ if val & SMIC_BUSY != 0 => {
                        let last = |p: usize, n: usize| p + 1 >= n;
                        *status = match *ctrl {
                            SMS_CC_START_TRANSFER => {
                                *req = vec![*din];
                                SMS_SC_WRITE_START as u8
                            }
                            SMS_CC_NEXT_TRANSFER => {
                                req.push(*din);
                                SMS_SC_WRITE_NEXT as u8
                            }
                            SMS_CC_END_TRANSFER => {
                                req.push(*din);
                                *rsp = self.bmc.respond(req);
                                *rx = true;
                                SMS_SC_WRITE_END as u8
                            }
                            SMS_CC_START_RECEIVE => {
                                *pos = 0;
                                *out = rsp[0];
                                if last(0, rsp.len()) {
                                    SMS_SC_READ_END as u8
                                } else {
                                    SMS_SC_READ_START as u8
                                }
                            }
                            SMS_CC_NEXT_RECEIVE => {
                                *pos += 1;
                                *out = rsp[*pos];
                                if last(*pos, rsp.len()) {
                                    SMS_SC_READ_END as u8
                                } else {
                                    SMS_SC_READ_NEXT as u8
                                }
                            }
                            SMS_CC_END_RECEIVE => {
                                *rx = false;
                                SMS_SC_READY as u8
                            }
                            c => panic!("unexpected SMIC control code {c:#x}"),
                        };
                    }
                    _ => {}
                },
                Wire::Bt {
                    b2h,
                    hbusy,
                    mask,
                    wbuf,
                    rbuf,
                    pos,
                } => match offset {
                    0 => {
                        if val & BT_CLR_WR_PTR != 0 {
                            wbuf.clear();
                        }
                        if val & BT_CLR_RD_PTR != 0 {
                            *pos = 0;
                        }
                        if val & BT_HOST_BUSY != 0 {
                            *hbusy = !*hbusy;
                        }
                        if val & BT_BMC2HOST_ATN != 0 {
                            *b2h = false;
                        }
                        if val & BT_HOST2BMC_ATN != 0 && !wbuf.is_empty() {
                            // [len, nfln, seq, cmd, data..] -> [len, nfln, seq, cmd, cc, ..]
                            assert_eq!(usize::from(wbuf[0]), wbuf.len() - 1, "the length");
                            let mut inner = vec![wbuf[1], wbuf[3]];
                            inner.extend_from_slice(&wbuf[4..]);
                            let r = self.bmc.respond(&inner);
                            let mut out = vec![0, r[0], wbuf[2]];
                            out.extend_from_slice(&r[1..]);
                            out[0] = (out.len() - 1) as u8;
                            *rbuf = out;
                            *b2h = true;
                        }
                    }
                    1 => wbuf.push(val),
                    _ => *mask = val,
                },
            }
        }

        fn bmc_delay(&self) {}

        fn devname(&self) -> &str {
            "ipmi0"
        }

        fn sc_if(&self) -> Option<IpmiIf<Self>> {
            Some(match &*self.wire.borrow() {
                Wire::Kcs { .. } => IpmiIf::KCS,
                Wire::Smic { .. } => IpmiIf::SMIC,
                Wire::Bt { .. } => IpmiIf::BT,
            })
        }

        fn sc_xfer(&self) -> &RefCell<IpmiXfer> {
            &self.xfer
        }

        fn sc_sensordev(&self) -> &Ksensordev {
            &self.dev
        }

        fn ipmi_cmd_wait(&self, c: &mut IpmiCmd<'_>) {
            ipmi_cmd_poll(self, c);
        }
    }

    fn get_device_id(sc: &Fake) -> (i32, Vec<u8>) {
        let mut buf = [0u8; 32];
        let mut c = IpmiCmd {
            c_rssa: BMC_SA,
            c_rslun: BMC_LUN,
            c_netfn: APP_NETFN,
            c_cmd: APP_GET_DEVICE_ID,
            c_maxrxlen: buf.len() as i32,
            c_data: Some(&mut buf),
            ..IpmiCmd::default()
        };
        ipmi_cmd_poll(sc, &mut c);
        let (cc, n) = (c.c_ccode, c.c_rxlen.max(0) as usize);
        (cc, buf[..n].to_vec())
    }

    #[test]
    fn kcs_sends_and_receives_through_the_state_machine() {
        let sc = Fake::new(IPMI_IF_KCS, Bmc::default());
        assert_eq!(kcs_probe(sc), 0);
        for _ in 0..2 {
            let (cc, data) = get_device_id(sc);
            assert_eq!(cc, 0);
            assert_eq!(data, [0x20, 0x01, 0x02, 0x00, 0x02, 0x3f]);
        }
        assert_eq!(*sc.bmc.log.borrow(), [vec![0x18, 0x01], vec![0x18, 0x01]]);

        // An unknown command: the completion code comes back, no data.
        let mut c = IpmiCmd {
            c_rssa: BMC_SA,
            c_netfn: APP_NETFN,
            c_cmd: 0x7f,
            ..IpmiCmd::default()
        };
        ipmi_cmd_poll(sc, &mut c);
        assert_eq!((c.c_ccode, c.c_rxlen), (0xc1, 0));
    }

    #[test]
    fn kcs_error_state_and_refused_requests() {
        let sc = Fake::new(IPMI_IF_KCS, Bmc::default());
        if let Wire::Kcs { state, .. } = &mut *sc.wire.borrow_mut() {
            *state = KCS_ERROR_STATE as u8;
        }
        assert_eq!(kcs_probe(sc), 1);

        // A responder other than the BMC is the C's #if 0 IPMB path: a send error.
        let sc = Fake::new(IPMI_IF_KCS, Bmc::default());
        let mut c = IpmiCmd {
            c_rssa: 0x22,
            ..IpmiCmd::default()
        };
        ipmi_cmd_poll(sc, &mut c);
        assert_eq!(c.c_ccode, -1);
        // So is a request longer than its data.
        let mut short = [0u8; 1];
        let mut c = IpmiCmd {
            c_rssa: BMC_SA,
            c_txlen: 4,
            c_data: Some(&mut short),
            ..IpmiCmd::default()
        };
        ipmi_cmd_poll(sc, &mut c);
        assert_eq!(c.c_ccode, -1);
        assert!(sc.bmc.log.borrow().is_empty());
        assert_eq!(sc.polls.get(), 0);
    }

    #[test]
    fn smic_sends_and_receives_with_control_codes() {
        let sc = Fake::new(IPMI_IF_SMIC, Bmc::default());
        assert_eq!(smic_probe(sc), 0);
        let (cc, data) = get_device_id(sc);
        assert_eq!(cc, 0);
        assert_eq!(data, [0x20, 0x01, 0x02, 0x00, 0x02, 0x3f]);

        // A request with data goes START, NEXT.., END.
        let mut d = [7u8, 8, 9];
        let mut c = IpmiCmd {
            c_rssa: BMC_SA,
            c_netfn: APP_NETFN,
            c_cmd: APP_SET_WATCHDOG_TIMER,
            c_txlen: 3,
            c_data: Some(&mut d),
            ..IpmiCmd::default()
        };
        ipmi_cmd_poll(sc, &mut c);
        assert_eq!((c.c_ccode, c.c_rxlen), (0, 0));
        assert_eq!(*sc.bmc.wdog.borrow(), [7, 8, 9]);
    }

    #[test]
    fn bt_numbers_its_messages_and_keeps_the_c_data_offset() {
        let sc = Fake::new(IPMI_IF_BT, Bmc::default());
        assert_eq!(bt_probe(sc), 0);
        let (cc, data) = get_device_id(sc);
        assert_eq!(cc, 0);
        // bt_recvmsg drops the length and the sequence number but IPMI_BTMSG_DATARCV counts
        // them, so the data comes out two bytes late and two bytes short, as in C.
        assert_eq!(data, [0x02, 0x00, 0x02, 0x3f]);
        let _ = get_device_id(sc);
        assert_eq!(sc.xfer.borrow().sc_btseq, 2);
        assert_eq!(*sc.bmc.log.borrow(), [vec![0x18, 0x01], vec![0x18, 0x01]]);
    }

    /// A full record (type 1): a threshold temperature sensor in degrees C, `y = v`.
    fn full_temp(num: u8, name: &[u8]) -> Vec<u8> {
        let mut r = vec![0u8; SDRTYPE1_NAME];
        r[2] = 0x51;
        r[3] = IPMI_SDR_TYPEFULL;
        r[5] = BMC_SA as u8; // owner_id
        r[7] = num; // sensor_num
        r[12] = 0x01; // sensor_type: temperature
        r[13] = 0x01; // event_code: threshold
        r[21] = 1; // units2: degrees C
        r[24] = 1; // m
        r[47] = 0xc0 | name.len() as u8; // typelen: 8-bit ASCII
        r.extend_from_slice(name);
        r[4] = (r.len() - 5) as u8;
        r
    }

    /// A compact record (type 2): `count` sensors of type `stype` from `num`, instances
    /// from 1.
    fn compact(num: u8, count: u8, stype: u8, event: u8, name: &[u8]) -> Vec<u8> {
        let mut r = vec![0u8; SDRTYPE2_NAME];
        r[3] = IPMI_SDR_TYPECOMPACT;
        r[5] = BMC_SA as u8;
        r[7] = num;
        r[12] = stype;
        r[13] = event;
        r[23] = count; // share1
        r[24] = 1; // share2: instance base
        r[31] = 0xc0 | name.len() as u8;
        r.extend_from_slice(name);
        r[4] = (r.len() - 5) as u8;
        r
    }

    #[test]
    fn the_sdr_repository_becomes_sensors() {
        let _g = crate::kern::subr_pool::tests::setup_real_memory();
        let bmc = Bmc {
            sdrs: vec![
                full_temp(1, b"CPU Temp"),
                // QEMU's default record: a watchdog sensor, which ipmi(4) does not export.
                compact(2, 1, 0x23, 0x6f, b"Watchdog"),
                compact(3, 2, 0x05, 0x6f, b"Lid"),
                // A sensor that does not answer its reading is not attached.
                full_temp(9, b"Gone"),
            ],
            readings: vec![(1, [25, 0x40, 0]), (3, [0, 0x40, 1]), (4, [0, 0x40, 0])],
            ..Bmc::default()
        };
        let sc = Fake::new(IPMI_IF_KCS, bmc);

        let mut rec = 0u16;
        let mut n = 0;
        while rec != 0xFFFF {
            assert_eq!(get_sdr(sc, rec, &mut rec), 0);
            n += 1;
        }
        assert_eq!(n, 4);

        let sensors: Vec<&IpmiSensor> = IPMI_SENSOR_LIST.0.iter().collect();
        let desc = |s: &IpmiSensor| {
            let d = s.i_sensor.desc.get();
            let l = d.iter().position(|&b| b == 0).unwrap_or(d.len());
            d[..l].to_vec()
        };
        // SLIST_INSERT_HEAD: the newest first.
        assert_eq!(
            sensors.iter().map(|s| desc(s)).collect::<Vec<_>>(),
            [
                b"Lid - 2".to_vec(),
                b"Lid - 1".to_vec(),
                b"CPU Temp".to_vec()
            ]
        );
        let temp = sensors[2];
        assert_eq!(temp.i_sensor.r#type.get(), SENSOR_TEMP);
        assert_eq!(temp.i_sensor.value.get(), 25_000_000 + 273_150_000);
        assert_eq!(temp.i_sensor.status.get(), SENSOR_S_OK);
        assert_eq!(temp.i_sensor.flags.get() & SENSOR_FINVALID, 0);
        assert_eq!(sensors[1].i_sensor.r#type.get(), SENSOR_INDICATOR);
        assert_eq!(sensors[1].i_sensor.value.get(), 1);
        assert_eq!(sensors[1].i_sensor.status.get(), SENSOR_S_CRIT);
        assert_eq!(sensors[0].i_sensor.value.get(), 0);
        assert_eq!(sensors[0].i_sensor.status.get(), SENSOR_S_OK);
        assert_eq!(sc.dev.sensors_count.get(), 3);

        // The first record: its header, then MAXSDRLEN pieces.
        let gets = sc
            .bmc
            .log
            .borrow()
            .iter()
            .filter(|r| r[1] == STORAGE_GET_SDR as u8)
            .map(|r| (r[6], r[7]))
            .take(4)
            .collect::<Vec<_>>();
        assert_eq!(gets, [(0, 5), (5, 16), (21, 16), (37, 16)]);
    }

    #[test]
    fn sign_extension_and_powers_of_ten() {
        assert_eq!(signextend(0x1ff, 10), 0x1ff);
        assert_eq!(signextend(0x200, 10), -512);
        assert_eq!(signextend(0x3ff, 10), -1);
        assert_eq!(signextend(0x7, 4), 7);
        assert_eq!(signextend(0x8, 4), -8);
        assert_eq!(signextend(0xf, 4), -1);
        assert_eq!(ipow(25, 0), 25);
        assert_eq!(ipow(25, 3), 25_000);
        assert_eq!(ipow(12_345, -2), 123);
        assert_eq!(ipow(-12_345, -2), -123);
    }

    #[test]
    fn readings_convert_through_the_linear_factors() {
        let s1 = |m: u16, b: u16, k1: u8, k2: u8| Sdrtype1 {
            m: m as u8,
            m_tolerance: ((m >> 2) & 0xc0) as u8,
            b: b as u8,
            b_accuracy: ((b >> 2) & 0xc0) as u8,
            rbexp: (k2 << 4) | k1,
            ..Sdrtype1::default()
        };
        // y = v
        assert_eq!(ipmi_convert(40, &s1(1, 0, 0, 0), 6), 40_000_000);
        // A 12 V rail: M = 6, K2 = -2 (0.06 V per count), B = 0.
        assert_eq!(ipmi_convert(200, &s1(6, 0, 0, 0xe), 6), 12_000_000);
        // M = -1 (10 bits), B = 100 with K1 = 1: y = -v + 1000.
        assert_eq!(ipmi_convert(10, &s1(0x3ff, 100, 1, 0), 0), 990);
        // A fan in units of 30 RPM.
        assert_eq!(ipmi_convert(100, &s1(30, 0, 0, 0), 0), 3000);
    }

    #[test]
    fn names_decode_from_every_encoding() {
        let mut name = [0xffu8; 64];
        let cstr = |n: &[u8]| n[..n.iter().position(|&b| b == 0).unwrap()].to_vec();

        assert_eq!(ipmi_sensor_name(&mut name, 0xc3, b"Fan", 3), 1);
        assert_eq!(cstr(&name), b"Fan");
        // BCD plus: two characters a byte.
        assert_eq!(ipmi_sensor_name(&mut name, 0x42, &[0x12, 0xa3], 2), 1);
        assert_eq!(cstr(&name), b"12 3");
        // 6-bit ASCII: "IPMI" packed little-endian in three bytes.
        let packed = [0x29, 0xdc, 0xa6];
        assert_eq!(ipmi_sensor_name(&mut name, 0x83, &packed, 3), 1);
        assert_eq!(cstr(&name), b"IPMI");
        // Unicode is not decoded: an empty name.
        assert_eq!(ipmi_sensor_name(&mut name, 0x05, b"abcde", 5), 1);
        assert_eq!(cstr(&name), b"");
        // Too short a record.
        assert_eq!(ipmi_sensor_name(&mut name, 0xc8, b"Short", 5), 0);
        // A small buffer truncates.
        let mut small = [0u8; 4];
        assert_eq!(ipmi_sensor_name(&mut small, 0xc8, b"LongName", 8), 1);
        assert_eq!(cstr(&small), b"Lon");
        assert_eq!(getbits(&[0b1010_0000], 5, 3), 0b101);
        assert_eq!(getbits(&[0xff], 6, 6), 0b11, "bits past the end read 0");
    }

    #[test]
    fn sensor_types_and_statuses() {
        assert_eq!(ipmi_sensor_type(0x01, 0x01, 1, 0), Some(SENSOR_TEMP));
        assert_eq!(ipmi_sensor_type(0x02, 0x01, 4, 0), Some(SENSOR_VOLTS_DC));
        assert_eq!(ipmi_sensor_type(0x04, 0x01, 18, 0), Some(SENSOR_FANRPM));
        assert_eq!(ipmi_sensor_type(0x03, 0x01, 5, 0), Some(SENSOR_AMPS));
        assert_eq!(ipmi_sensor_type(0x08, 0x01, 6, 0), Some(SENSOR_WATTS));
        assert_eq!(ipmi_sensor_type(0x05, 0x6f, 0, 0), Some(SENSOR_INDICATOR));
        assert_eq!(
            ipmi_sensor_type(0x08, 0x6f, 0, 0x0a),
            Some(SENSOR_INDICATOR)
        );
        assert_eq!(ipmi_sensor_type(0x08, 0x6f, 0, 0x03), None);
        assert_eq!(ipmi_sensor_type(0x23, 0x6f, 0, 0), None);

        let sdr: &'static [u8] = vec![0u8; 64].leak();
        let s = |stype: i32, etype: i32, typ| {
            let s = IpmiSensor {
                i_sdr: sdr,
                i_num: 0,
                stype,
                etype,
                i_sensor: Ksensor::new(),
                list: SlistEntry::new(),
            };
            s.i_sensor.r#type.set(typ);
            s
        };
        let st = |s: &IpmiSensor, b2: u8| ipmi_sensor_status(s, &[0, 0, b2, 0, 0, 0, 0, 0]);
        let t = s(1, 1, SENSOR_TEMP);
        assert_eq!(st(&t, 0), SENSOR_S_OK);
        assert_eq!(st(&t, 1 << 0), SENSOR_S_WARN);
        assert_eq!(st(&t, 1 << 1), SENSOR_S_CRIT);
        assert_eq!(st(&t, 1 << 5), SENSOR_S_CRIT);
        assert_eq!(t.i_sensor.value.get(), 273_150_000);
        let p = s(8, 0x6f, SENSOR_INDICATOR);
        assert_eq!(st(&p, 0x01), SENSOR_S_OK);
        assert_eq!(p.i_sensor.value.get(), 1);
        assert_eq!(st(&p, 0x09), SENSOR_S_WARN);
        assert_eq!(st(&p, 0x10), SENSOR_S_CRIT);
        assert_eq!(p.i_sensor.value.get(), 0);
    }

    #[test]
    fn smbios_records_become_attach_arguments() {
        let mut ia = IpmiAttachArgs::zeroed();
        // QEMU's isa-ipmi-kcs: KCS 2.0 at I/O 0xca2, byte spacing, no interrupt.
        let kcs = SmbiosIpmi {
            smipmi_if_type: 1,
            smipmi_if_rev: 0x20,
            smipmi_i2c_address: 0x20,
            smipmi_nvram_address: 0,
            smipmi_base_address: 0xca2 | 1,
            smipmi_base_flags: 0,
            smipmi_irq: 0,
        };
        ipmi_smbios_probe(&kcs, &mut ia);
        assert_eq!(
            (
                ia.iaa_if_type,
                ia.iaa_if_rev,
                ia.iaa_if_iotype,
                ia.iaa_if_iobase
            ),
            (1, 0x20, b'i', 0xca2)
        );
        assert_eq!(
            (ia.iaa_if_iospacing, ia.iaa_if_irq, ia.iaa_if_irqlvl),
            (1, -1, IST_EDGE)
        );

        let bt = SmbiosIpmi {
            smipmi_if_type: 3,
            smipmi_base_address: 0xfed4_0000,
            smipmi_base_flags: 0x40 | SMIPMI_FLAG_IRQEN | SMIPMI_FLAG_ODDOFFSET | 1,
            smipmi_irq: 10,
            ..kcs
        };
        ipmi_smbios_probe(&bt, &mut ia);
        assert_eq!(
            (ia.iaa_if_iotype, ia.iaa_if_iobase, ia.iaa_if_iospacing),
            (b'm', 0xfed4_0001, 4)
        );
        assert_eq!((ia.iaa_if_irq, ia.iaa_if_irqlvl), (10, IST_LEVEL));

        let ibm = SmbiosIpmi {
            smipmi_base_address: 0xe4,
            smipmi_base_flags: 0x7f,
            ..kcs
        };
        ipmi_smbios_probe(&ibm, &mut ia);
        assert_eq!(
            (ia.iaa_if_iotype, ia.iaa_if_iobase, ia.iaa_if_iospacing),
            (b'i', 0xe4, 1)
        );
    }

    #[test]
    fn ioctl_numbers_match_the_c() {
        // _IOWR('i', 12, struct ipmi_recv): 48 bytes on LP64.
        assert_eq!(IPMICTL_RECEIVE_MSG, 0xc030_690c);
        assert_eq!(IPMICTL_SEND_COMMAND, 0x8028_690d);
        assert_eq!(IPMICTL_SET_MY_ADDRESS_CMD, 0x8004_6911);
        assert_eq!(IPMICTL_GET_MY_LUN_CMD, 0x4004_6914);
    }
}
/* </TESTS> */
