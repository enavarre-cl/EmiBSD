/* $OpenBSD: ipmivar.h,v 1.34 2021/01/23 12:10:08 kettenis Exp $ */
/* <LICENSES> */
/*
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
 *
 */
/* </LICENSES> */

/* <CODE> */
//! ipmi(4)'s definitions: `dev/ipmivar.h`. The interface types (KCS, SMIC, BT and SSIF), the
//! attach arguments a bus hands the driver, the interface layer's function table, a command,
//! the softc, the watchdog bits and the BMC's network functions and commands, and the two
//! sensor data record layouts the driver reads.
//!
//! Upstream: sys/dev/ipmivar.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - `struct ipmi_if`'s function pointers take the softc beside the command: a command does
//!   not carry its softc (`c_sc`, see [`IpmiCmd`]), and the transfer state the interfaces
//!   work in (`sc_buf`, `sc_btseq`) is an [`IpmiXfer`] passed to them. The table is generic
//!   over [`IpmiBmc`], the softc's register access, so that the KCS, SMIC and BT state
//!   machines run on the host against a simulated BMC (`ipmi.rs`'s tests); the kernel uses
//!   `IpmiIf<IpmiSoftc>` only. `IpmiBmc` is not in the C.
//! - `struct ipmi_cmd` has no `c_sc` (see above); `c_data` is an optional byte slice (the
//!   C's `void *`, `NULL` being `None`) and `c_ccode` an `i32`: the C's `u_int` is set to and
//!   compared with -1 only.
//! - `struct ipmi_iowait`'s `v` member, never used by the C, is left out.
//! - `sc_ioctl`'s request, command and buffer are an [`IpmiIoctlState`] in an `UnsafeCell`
//!   under `sc_ioctl.lock`, as the C protects them; its command keeps the members that
//!   persist between `IPMICTL_SEND_COMMAND` and `IPMICTL_RECEIVE_MSG` (`c_data` is attached
//!   for each command).
//! - `struct sdrtype1`/`sdrtype2` are `#[repr(C, packed)]` as in C, read from a record's
//!   bytes with [`Sdrtype1::read`]/[`Sdrtype2::read`] (zero-filled past a short record, where
//!   the C would read past it). Their `name[1]` members are left out: the name bytes are the
//!   record's from `SDRTYPE1_NAME`/`SDRTYPE2_NAME` on (the C's `offsetof(..., name)`).
//! - `struct ipmi_thread`'s `running` is an `AtomicI32` (the C's `volatile int`).

use core::cell::{Cell, RefCell, UnsafeCell};
use core::sync::atomic::AtomicI32;

use crate::dev::ipmi::{IPMI_MAX_RX, IpmiReq, IpmiSensor};
use crate::machine::bus::{BusAddr, BusSpaceHandle, BusSpaceTag};
use crate::machine::cpu::delay;
use crate::sys::device::{Device, Softc};
use crate::sys::rwlock::Rwlock;
use crate::sys::sensors::Ksensordev;
use crate::sys::task::{Task, Taskq};

/// `IPMI_IF_KCS`: Keyboard Controller Style.
pub const IPMI_IF_KCS: i32 = 1;
/// `IPMI_IF_SMIC`: Server Management Interface Chip.
pub const IPMI_IF_SMIC: i32 = 2;
/// `IPMI_IF_BT`: Block Transfer.
pub const IPMI_IF_BT: i32 = 3;
/// `IPMI_IF_SSIF`: SMBus System Interface.
pub const IPMI_IF_SSIF: i32 = 4;

/// `IPMI_IF_KCS_NREGS`.
pub const IPMI_IF_KCS_NREGS: i32 = 2;
/// `IPMI_IF_SMIC_NREGS`.
pub const IPMI_IF_SMIC_NREGS: i32 = 3;
/// `IPMI_IF_BT_NREGS`.
pub const IPMI_IF_BT_NREGS: i32 = 3;

/// `IPMI_WDOG_DONTSTOP`: don't stop the timer when setting it.
pub const IPMI_WDOG_DONTSTOP: u8 = 0x40;

/// `IPMI_WDOG_MASK`: the timeout action bits.
pub const IPMI_WDOG_MASK: u8 = 0x03;
/// `IPMI_WDOG_DISABLED`.
pub const IPMI_WDOG_DISABLED: u8 = 0x00;
/// `IPMI_WDOG_REBOOT`.
pub const IPMI_WDOG_REBOOT: u8 = 0x01;
/// `IPMI_WDOG_PWROFF`.
pub const IPMI_WDOG_PWROFF: u8 = 0x02;
/// `IPMI_WDOG_PWRCYCLE`.
pub const IPMI_WDOG_PWRCYCLE: u8 = 0x03;

/// `IPMI_WDOG_PRE_DISABLED`.
pub const IPMI_WDOG_PRE_DISABLED: u8 = 0x00;
/// `IPMI_WDOG_PRE_SMI`.
pub const IPMI_WDOG_PRE_SMI: u8 = 0x01;
/// `IPMI_WDOG_PRE_NMI`.
pub const IPMI_WDOG_PRE_NMI: u8 = 0x02;
/// `IPMI_WDOG_PRE_INTERRUPT`.
pub const IPMI_WDOG_PRE_INTERRUPT: u8 = 0x03;

/// `IPMI_SET_WDOG_TIMER`: byte of Set Watchdog Timer: timer use.
pub const IPMI_SET_WDOG_TIMER: usize = 0;
/// `IPMI_SET_WDOG_ACTION`: timer actions.
pub const IPMI_SET_WDOG_ACTION: usize = 1;
/// `IPMI_SET_WDOG_PRETIMO`: pre-timeout interval.
pub const IPMI_SET_WDOG_PRETIMO: usize = 2;
/// `IPMI_SET_WDOG_FLAGS`: timer use expiration flags clear.
pub const IPMI_SET_WDOG_FLAGS: usize = 3;
/// `IPMI_SET_WDOG_TIMOL`: initial countdown, low byte.
pub const IPMI_SET_WDOG_TIMOL: usize = 4;
/// `IPMI_SET_WDOG_TIMOM`: initial countdown, high byte.
pub const IPMI_SET_WDOG_TIMOM: usize = 5;
/// `IPMI_SET_WDOG_MAX`: the request's length.
pub const IPMI_SET_WDOG_MAX: usize = 6;

/// `IPMI_GET_WDOG_TIMER`.
pub const IPMI_GET_WDOG_TIMER: usize = IPMI_SET_WDOG_TIMER;
/// `IPMI_GET_WDOG_ACTION`.
pub const IPMI_GET_WDOG_ACTION: usize = IPMI_SET_WDOG_ACTION;
/// `IPMI_GET_WDOG_PRETIMO`.
pub const IPMI_GET_WDOG_PRETIMO: usize = IPMI_SET_WDOG_PRETIMO;
/// `IPMI_GET_WDOG_FLAGS`.
pub const IPMI_GET_WDOG_FLAGS: usize = IPMI_SET_WDOG_FLAGS;
/// `IPMI_GET_WDOG_TIMOL`.
pub const IPMI_GET_WDOG_TIMOL: usize = IPMI_SET_WDOG_TIMOL;
/// `IPMI_GET_WDOG_TIMOM`.
pub const IPMI_GET_WDOG_TIMOM: usize = IPMI_SET_WDOG_TIMOM;
/// `IPMI_GET_WDOG_PRECDL`: present countdown, low byte.
pub const IPMI_GET_WDOG_PRECDL: usize = 6;
/// `IPMI_GET_WDOG_PRECDM`: present countdown, high byte.
pub const IPMI_GET_WDOG_PRECDM: usize = 7;
/// `IPMI_GET_WDOG_MAX`: the response's length.
pub const IPMI_GET_WDOG_MAX: usize = 8;

/// `IPMI_MSG_NFLN`: KCS/SMIC message byte: network function and LUN.
pub const IPMI_MSG_NFLN: usize = 0;
/// `IPMI_MSG_CMD`: the command.
pub const IPMI_MSG_CMD: usize = 1;
/// `IPMI_MSG_CCODE`: a response's completion code.
pub const IPMI_MSG_CCODE: usize = 2;
/// `IPMI_MSG_DATASND`: where a request's data starts.
pub const IPMI_MSG_DATASND: i32 = 2;
/// `IPMI_MSG_DATARCV`: where a response's data starts.
pub const IPMI_MSG_DATARCV: i32 = 3;

/// `APP_NETFN`: application requests.
pub const APP_NETFN: i32 = 0x06;
/// `APP_GET_DEVICE_ID`.
pub const APP_GET_DEVICE_ID: i32 = 0x01;
/// `APP_RESET_WATCHDOG`.
pub const APP_RESET_WATCHDOG: i32 = 0x22;
/// `APP_SET_WATCHDOG_TIMER`.
pub const APP_SET_WATCHDOG_TIMER: i32 = 0x24;
/// `APP_GET_WATCHDOG_TIMER`.
pub const APP_GET_WATCHDOG_TIMER: i32 = 0x25;
/// `APP_GET_SYSTEM_INTERFACE_CAPS`.
pub const APP_GET_SYSTEM_INTERFACE_CAPS: i32 = 0x57;

/// `TRANSPORT_NETFN`.
pub const TRANSPORT_NETFN: i32 = 0xC;
/// `BRIDGE_NETFN`.
pub const BRIDGE_NETFN: i32 = 0x2;

/// `STORAGE_NETFN`: storage requests.
pub const STORAGE_NETFN: i32 = 0x0A;
/// `STORAGE_GET_FRU_INV_AREA`.
pub const STORAGE_GET_FRU_INV_AREA: i32 = 0x10;
/// `STORAGE_READ_FRU_DATA`.
pub const STORAGE_READ_FRU_DATA: i32 = 0x11;
/// `STORAGE_RESERVE_SDR`.
pub const STORAGE_RESERVE_SDR: i32 = 0x22;
/// `STORAGE_GET_SDR`.
pub const STORAGE_GET_SDR: i32 = 0x23;
/// `STORAGE_ADD_SDR`.
pub const STORAGE_ADD_SDR: i32 = 0x24;
/// `STORAGE_ADD_PARTIAL_SDR`.
pub const STORAGE_ADD_PARTIAL_SDR: i32 = 0x25;
/// `STORAGE_DELETE_SDR`.
pub const STORAGE_DELETE_SDR: i32 = 0x26;
/// `STORAGE_RESERVE_SEL`.
pub const STORAGE_RESERVE_SEL: i32 = 0x42;
/// `STORAGE_GET_SEL`.
pub const STORAGE_GET_SEL: i32 = 0x43;
/// `STORAGE_ADD_SEL`.
pub const STORAGE_ADD_SEL: i32 = 0x44;
/// `STORAGE_ADD_PARTIAL_SEL`.
pub const STORAGE_ADD_PARTIAL_SEL: i32 = 0x45;
/// `STORAGE_DELETE_SEL`.
pub const STORAGE_DELETE_SEL: i32 = 0x46;

/// `SE_NETFN`: sensor/event requests.
pub const SE_NETFN: i32 = 0x04;
/// `SE_GET_SDR_INFO`.
pub const SE_GET_SDR_INFO: i32 = 0x20;
/// `SE_GET_SDR`.
pub const SE_GET_SDR: i32 = 0x21;
/// `SE_RESERVE_SDR`.
pub const SE_RESERVE_SDR: i32 = 0x22;
/// `SE_GET_SENSOR_FACTOR`.
pub const SE_GET_SENSOR_FACTOR: i32 = 0x23;
/// `SE_SET_SENSOR_HYSTERESIS`.
pub const SE_SET_SENSOR_HYSTERESIS: i32 = 0x24;
/// `SE_GET_SENSOR_HYSTERESIS`.
pub const SE_GET_SENSOR_HYSTERESIS: i32 = 0x25;
/// `SE_SET_SENSOR_THRESHOLD`.
pub const SE_SET_SENSOR_THRESHOLD: i32 = 0x26;
/// `SE_GET_SENSOR_THRESHOLD`.
pub const SE_GET_SENSOR_THRESHOLD: i32 = 0x27;
/// `SE_SET_SENSOR_EVENT_ENABLE`.
pub const SE_SET_SENSOR_EVENT_ENABLE: i32 = 0x28;
/// `SE_GET_SENSOR_EVENT_ENABLE`.
pub const SE_GET_SENSOR_EVENT_ENABLE: i32 = 0x29;
/// `SE_REARM_SENSOR_EVENTS`.
pub const SE_REARM_SENSOR_EVENTS: i32 = 0x2A;
/// `SE_GET_SENSOR_EVENT_STATUS`.
pub const SE_GET_SENSOR_EVENT_STATUS: i32 = 0x2B;
/// `SE_GET_SENSOR_READING`.
pub const SE_GET_SENSOR_READING: i32 = 0x2D;
/// `SE_SET_SENSOR_TYPE`.
pub const SE_SET_SENSOR_TYPE: i32 = 0x2E;
/// `SE_GET_SENSOR_TYPE`.
pub const SE_GET_SENSOR_TYPE: i32 = 0x2F;

/// `offsetof(struct sdrtype1, name)`: where a full sensor record's name starts.
pub const SDRTYPE1_NAME: usize = 48;
/// `offsetof(struct sdrtype2, name)`: where a compact sensor record's name starts.
pub const SDRTYPE2_NAME: usize = 32;

/// `struct ipmi_iowait`: what `bmc_io_wait` polls for.
#[derive(Clone, Copy)]
pub struct IpmiIowait {
    /// `offset`: the register.
    pub offset: i32,
    /// `mask`: the bits that matter.
    pub mask: u8,
    /// `value`: what they must read.
    pub value: u8,
    /// `lbl`: who waits, for the debug message.
    pub lbl: &'static str,
}

/// `struct ipmi_attach_args`: what a bus hands ipmi(4). `#[repr(C)]` with the name first:
/// mainbus prints it through `mba_busname`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct IpmiAttachArgs {
    /// `iaa_name`: "ipmi".
    pub iaa_name: &'static [u8],
    /// `iaa_iot`: the I/O space tag.
    pub iaa_iot: Option<BusSpaceTag>,
    /// `iaa_memt`: the memory space tag.
    pub iaa_memt: Option<BusSpaceTag>,

    /// `iaa_if_type`: `IPMI_IF_*`.
    pub iaa_if_type: i32,
    /// `iaa_if_rev`: the IPMI revision, BCD major.minor.
    pub iaa_if_rev: i32,
    /// `iaa_if_iotype`: `b'i'` for I/O ports, `b'm'` for memory.
    pub iaa_if_iotype: u8,
    /// `iaa_if_iobase`: the first register.
    pub iaa_if_iobase: BusAddr,
    /// `iaa_if_iosize`: the register width in bytes (1 or 4).
    pub iaa_if_iosize: i32,
    /// `iaa_if_iospacing`: the distance between two registers.
    pub iaa_if_iospacing: i32,
    /// `iaa_if_irq`: the interrupt, -1 for none.
    pub iaa_if_irq: i32,
    /// `iaa_if_irqlvl`: `IST_LEVEL` or `IST_EDGE`.
    pub iaa_if_irqlvl: i32,
}

impl IpmiAttachArgs {
    /// `memset(&ia, 0, sizeof(ia))`.
    pub const fn zeroed() -> Self {
        Self {
            iaa_name: b"",
            iaa_iot: None,
            iaa_memt: None,
            iaa_if_type: 0,
            iaa_if_rev: 0,
            iaa_if_iotype: 0,
            iaa_if_iobase: 0,
            iaa_if_iosize: 0,
            iaa_if_iospacing: 0,
            iaa_if_irq: 0,
            iaa_if_irqlvl: 0,
        }
    }
}

/// `sc_buf` and `sc_btseq`: the message buffer the interface layer builds requests and
/// receives responses in, and BT's sequence number. Commands run one at a time (the
/// command task queue has one thread; cold or panicking, the caller polls), which the
/// `RefCell` in the softc checks.
pub struct IpmiXfer {
    /// `sc_btseq`.
    pub sc_btseq: i32,
    /// `sc_buf`.
    pub sc_buf: [u8; IPMI_MAX_RX + 16],
}

impl IpmiXfer {
    /// A zeroed buffer.
    pub const fn new() -> Self {
        Self {
            sc_btseq: 0,
            sc_buf: [0; IPMI_MAX_RX + 16],
        }
    }
}

impl Default for IpmiXfer {
    fn default() -> Self {
        Self::new()
    }
}

/// The BMC as the interface layer reaches it: the softc's registers, its message buffer and
/// its interface. Not in the C (see the module's deviations).
pub trait IpmiBmc: Sized {
    /// `bmc_read(sc, offset)`: register `offset`.
    fn bmc_read(&self, offset: i32) -> u8;
    /// `bmc_write(sc, offset, val)`.
    fn bmc_write(&self, offset: i32, val: u8);
    /// `delay(1)` between two reads of `bmc_io_wait`.
    fn bmc_delay(&self) {
        delay(1);
    }
    /// `DEVNAME(sc)`.
    fn devname(&self) -> &str;
    /// `sc->sc_if`.
    fn sc_if(&self) -> Option<IpmiIf<Self>>;
    /// `sc->sc_buf`, `sc->sc_btseq`.
    fn sc_xfer(&self) -> &RefCell<IpmiXfer>;
    /// `&sc->sc_sensordev`.
    fn sc_sensordev(&self) -> &Ksensordev;
    /// `ipmi_cmd_wait(c)`: runs a command once the system is up (the softc's command task
    /// queue).
    fn ipmi_cmd_wait(&self, c: &mut IpmiCmd<'_>);
}

/// `struct ipmi_if`: an interface layer.
pub struct IpmiIf<S: IpmiBmc = IpmiSoftc> {
    /// `name`.
    pub name: &'static str,
    /// `nregs`: the registers it maps.
    pub nregs: i32,
    /// `buildmsg`: puts the command's header and data in the buffer.
    pub buildmsg: fn(&mut IpmiXfer, &IpmiCmd<'_>),
    /// `sendmsg`: sends the `c_txlen` bytes of the buffer; 0 or -1.
    pub sendmsg: fn(&S, &mut IpmiXfer, &mut IpmiCmd<'_>) -> i32,
    /// `recvmsg`: receives at most `c_maxrxlen` bytes into the buffer, sets `c_rxlen`; 0 or
    /// -1.
    pub recvmsg: fn(&S, &mut IpmiXfer, &mut IpmiCmd<'_>) -> i32,
    /// `reset`.
    pub reset: fn(&S) -> i32,
    /// `probe`.
    pub probe: fn(&S) -> i32,
    /// `datasnd`: the header bytes before a request's data.
    pub datasnd: i32,
    /// `datarcv`: the header bytes before a response's data.
    pub datarcv: i32,
}

impl<S: IpmiBmc> Clone for IpmiIf<S> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<S: IpmiBmc> Copy for IpmiIf<S> {}

/// `struct ipmi_cmd`: one command to the BMC and its response.
#[derive(Default)]
pub struct IpmiCmd<'a> {
    /// `c_rssa`: the responder's slave address.
    pub c_rssa: i32,
    /// `c_rslun`: the responder's LUN.
    pub c_rslun: i32,
    /// `c_netfn`: the network function.
    pub c_netfn: i32,
    /// `c_cmd`: the command.
    pub c_cmd: i32,

    /// `c_txlen`: the request's data bytes (header included once sent).
    pub c_txlen: i32,
    /// `c_maxrxlen`: the most response bytes wanted.
    pub c_maxrxlen: i32,
    /// `c_rxlen`: the response's data bytes.
    pub c_rxlen: i32,

    /// `c_data`: the request's data, overwritten by the response's; `None` for `NULL`.
    pub c_data: Option<&'a mut [u8]>,
    /// `c_ccode`: the completion code, -1 until known.
    pub c_ccode: i32,
}

/// `struct ipmi_ioctl`'s members other than the lock.
pub struct IpmiIoctlState {
    /// `req`: the request being served; `msgid` -1 when none.
    pub req: IpmiReq,
    /// `cmd`: its command (without `c_data`, see the module's deviations).
    pub cmd: IpmiCmd<'static>,
    /// `buf`: its data.
    pub buf: [u8; IPMI_MAX_RX],
}

/// `struct ipmi_ioctl`: the one request `/dev/ipmi` serves at a time.
pub struct IpmiIoctl {
    /// `lock`.
    pub lock: Rwlock,
    /// The rest. Protected by: `lock`, held for writing.
    pub st: UnsafeCell<IpmiIoctlState>,
}

/// `struct ipmi_softc`.
#[repr(C)]
pub struct IpmiSoftc {
    /// `sc_dev`.
    pub sc_dev: Device,

    /// `sc_if`: the interface layer.
    pub sc_if: Cell<Option<&'static IpmiIf>>,
    /// `sc_if_iosize`: the size of the I/O ports.
    pub sc_if_iosize: Cell<i32>,
    /// `sc_if_iospacing`: the spacing of the I/O ports.
    pub sc_if_iospacing: Cell<i32>,
    /// `sc_if_rev`: the IPMI revision.
    pub sc_if_rev: Cell<i32>,

    /// `sc_ih`: interrupt handle (never established: the driver polls).
    pub sc_ih: Cell<*mut core::ffi::c_void>,
    /// `sc_iot`.
    pub sc_iot: Cell<Option<BusSpaceTag>>,
    /// `sc_ioh`.
    pub sc_ioh: Cell<Option<BusSpaceHandle>>,

    /// `sc_btseq` and `sc_buf`.
    pub sc_xfer: RefCell<IpmiXfer>,
    /// `sc_cmd_taskq`: where commands run once the system is up.
    pub sc_cmd_taskq: Cell<Option<&'static Taskq>>,

    /// `sc_ioctl`.
    pub sc_ioctl: IpmiIoctl,

    /// `sc_wdog_period`: the watchdog's period in seconds, 0 when off.
    pub sc_wdog_period: Cell<i32>,
    /// `sc_wdog_tickle_task`.
    pub sc_wdog_tickle_task: Task,

    /// `sc_thread`: the sensor thread's state.
    pub sc_thread: Cell<Option<&'static IpmiThread>>,

    /// `current_sensor`: the sensor the next refresh reads.
    pub current_sensor: Cell<Option<&'static IpmiSensor>>,
    /// `sc_sensordev`.
    pub sc_sensordev: Ksensordev,
}

// SAFETY: `#[repr(C)]` with the device first; all-zero is valid for every member: empty
// cells and options, a null pointer, an unborrowed `RefCell` of zeroed bytes, an unnamed
// unlocked `Rwlock`, a request of zeros, a command without data, an unset `Task` and an
// empty sensor device.
unsafe impl Softc for IpmiSoftc {}

/// `struct ipmi_thread`: the sensor thread's state.
pub struct IpmiThread {
    /// `sc`.
    pub sc: &'static IpmiSoftc,
    /// `running`: cleared to stop the thread.
    pub running: AtomicI32,
}

/// `struct sdrhdr`: a sensor data record's header.
#[repr(C, packed)]
#[derive(Clone, Copy, Default)]
pub struct Sdrhdr {
    /// `record_id`: SDR record ID.
    pub record_id: u16,
    /// `sdr_version`: SDR version.
    pub sdr_version: u8,
    /// `record_type`: SDR record type.
    pub record_type: u8,
    /// `record_length`: SDR record length (without this header).
    pub record_length: u8,
}

/// `struct sdrtype1`: SDR record type 1, a full sensor record (without `name[1]`).
#[repr(C, packed)]
#[derive(Clone, Copy, Default)]
pub struct Sdrtype1 {
    /// `sdrhdr`.
    pub sdrhdr: Sdrhdr,

    /// `owner_id`.
    pub owner_id: u8,
    /// `owner_lun`.
    pub owner_lun: u8,
    /// `sensor_num`.
    pub sensor_num: u8,

    /// `entity_id`.
    pub entity_id: u8,
    /// `entity_instance`.
    pub entity_instance: u8,
    /// `sensor_init`.
    pub sensor_init: u8,
    /// `sensor_caps`.
    pub sensor_caps: u8,
    /// `sensor_type`.
    pub sensor_type: u8,
    /// `event_code`.
    pub event_code: u8,
    /// `trigger_mask`.
    pub trigger_mask: u16,
    /// `reading_mask`.
    pub reading_mask: u16,
    /// `settable_mask`.
    pub settable_mask: u16,
    /// `units1`.
    pub units1: u8,
    /// `units2`.
    pub units2: u8,
    /// `units3`.
    pub units3: u8,
    /// `linear`.
    pub linear: u8,
    /// `m`.
    pub m: u8,
    /// `m_tolerance`.
    pub m_tolerance: u8,
    /// `b`.
    pub b: u8,
    /// `b_accuracy`.
    pub b_accuracy: u8,
    /// `accuracyexp`.
    pub accuracyexp: u8,
    /// `rbexp`.
    pub rbexp: u8,
    /// `analogchars`.
    pub analogchars: u8,
    /// `nominalreading`.
    pub nominalreading: u8,
    /// `normalmax`.
    pub normalmax: u8,
    /// `normalmin`.
    pub normalmin: u8,
    /// `sensormax`.
    pub sensormax: u8,
    /// `sensormin`.
    pub sensormin: u8,
    /// `uppernr`.
    pub uppernr: u8,
    /// `upperc`.
    pub upperc: u8,
    /// `uppernc`.
    pub uppernc: u8,
    /// `lowernr`.
    pub lowernr: u8,
    /// `lowerc`.
    pub lowerc: u8,
    /// `lowernc`.
    pub lowernc: u8,
    /// `physt`.
    pub physt: u8,
    /// `nhyst`.
    pub nhyst: u8,
    /// `resvd`.
    pub resvd: [u8; 2],
    /// `oem`.
    pub oem: u8,
    /// `typelen`: the name's encoding (bits 7-6) and length (bits 4-0).
    pub typelen: u8,
}

impl Sdrtype1 {
    /// `(struct sdrtype1 *)psdr`: the record's first bytes, zeros past its end.
    pub fn read(sdr: &[u8]) -> Self {
        read_packed(sdr)
    }
}

/// `struct sdrtype2`: SDR record type 2, a compact sensor record (without `name[1]`).
#[repr(C, packed)]
#[derive(Clone, Copy, Default)]
pub struct Sdrtype2 {
    /// `sdrhdr`.
    pub sdrhdr: Sdrhdr,

    /// `owner_id`.
    pub owner_id: u8,
    /// `owner_lun`.
    pub owner_lun: u8,
    /// `sensor_num`.
    pub sensor_num: u8,

    /// `entity_id`.
    pub entity_id: u8,
    /// `entity_instance`.
    pub entity_instance: u8,
    /// `sensor_init`.
    pub sensor_init: u8,
    /// `sensor_caps`.
    pub sensor_caps: u8,
    /// `sensor_type`.
    pub sensor_type: u8,
    /// `event_code`.
    pub event_code: u8,
    /// `trigger_mask`.
    pub trigger_mask: u16,
    /// `reading_mask`.
    pub reading_mask: u16,
    /// `set_mask`.
    pub set_mask: u16,
    /// `units1`.
    pub units1: u8,
    /// `units2`.
    pub units2: u8,
    /// `units3`.
    pub units3: u8,
    /// `share1`: the record's sensor count in bits 3-0.
    pub share1: u8,
    /// `share2`: the first instance number in bits 6-0.
    pub share2: u8,
    /// `physt`.
    pub physt: u8,
    /// `nhyst`.
    pub nhyst: u8,
    /// `resvd`.
    pub resvd: [u8; 3],
    /// `oem`.
    pub oem: u8,
    /// `typelen`.
    pub typelen: u8,
}

impl Sdrtype2 {
    /// `(struct sdrtype2 *)psdr`: the record's first bytes, zeros past its end.
    pub fn read(sdr: &[u8]) -> Self {
        read_packed(sdr)
    }
}

/// A packed record of plain bytes read from the start of `b`, zero past its end.
fn read_packed<T: Default + Copy>(b: &[u8]) -> T {
    let mut v = T::default();
    let n = b.len().min(size_of::<T>());
    // SAFETY: `T` is one of the `#[repr(C, packed)]` records above, made of integers only,
    // so every byte pattern is a valid value; at most `size_of::<T>()` bytes are written
    // into it, from a slice that holds at least `n`.
    unsafe {
        core::ptr::copy_nonoverlapping(b.as_ptr(), (&raw mut v).cast::<u8>(), n);
    }
    v
}

const _: () = {
    assert!(size_of::<Sdrhdr>() == 5);
    assert!(size_of::<Sdrtype1>() == SDRTYPE1_NAME);
    assert!(size_of::<Sdrtype2>() == SDRTYPE2_NAME);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_read_little_endian_fields_at_the_c_offsets() {
        let mut b = [0u8; 60];
        b[0] = 0x34;
        b[1] = 0x12; // record_id
        b[3] = 1; // record_type
        b[4] = 55; // record_length
        b[7] = 7; // sensor_num
        b[12] = 0x01; // sensor_type
        b[21] = 0x01; // units2
        b[24] = 0x80; // m
        b[29] = 0xf2; // rbexp
        b[47] = 0xc4; // typelen
        let s1 = Sdrtype1::read(&b);
        assert_eq!({ s1.sdrhdr.record_id }, 0x1234);
        assert_eq!(s1.sdrhdr.record_type, 1);
        assert_eq!(s1.sensor_num, 7);
        assert_eq!(s1.sensor_type, 1);
        assert_eq!(s1.units2, 1);
        assert_eq!(s1.m, 0x80);
        assert_eq!(s1.rbexp, 0xf2);
        assert_eq!(s1.typelen, 0xc4);

        let mut c = [0u8; 40];
        c[23] = 0x43; // share1
        c[24] = 0x85; // share2
        c[31] = 0xc8; // typelen
        let s2 = Sdrtype2::read(&c);
        assert_eq!(s2.share1, 0x43);
        assert_eq!(s2.share2, 0x85);
        assert_eq!(s2.typelen, 0xc8);
    }

    #[test]
    fn a_short_record_reads_zeros_past_its_end() {
        let s1 = Sdrtype1::read(&[1, 0, 0x51, 2, 9]);
        assert_eq!({ s1.sdrhdr.record_id }, 1);
        assert_eq!(s1.sdrhdr.record_type, 2);
        assert_eq!(s1.typelen, 0);
    }
}
/* </TESTS> */
