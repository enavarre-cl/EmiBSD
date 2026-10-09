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
 * Copyright (c) 2003, 2004 Alexander Yurchenko <grange@openbsd.org>
 * Copyright (c) 2006 Constantine A. Murenin <cnst+openbsd@bugmail.mojo.ru>
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
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR ``AS IS'' AND ANY EXPRESS OR
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES
 * OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED.
 * IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY DIRECT, INDIRECT,
 * INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT
 * NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
 * DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
 * THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
 * (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF
 * THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! `<sys/sensors.h>`: hardware sensors (`hw.sensors`): the user-visible `struct sensor` and
//! `struct sensordev` that `sysctl(2)` copies out, and the kernel's `struct ksensor` and
//! `struct ksensordev` that drivers attach. The functions live in `kern/kern_sensors.rs`.
//!
//! Upstream: sys/sys/sensors.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - `enum sensor_type` and `enum sensor_status` are `#[repr(i32)]` Rust enums with the C's
//!   names and values ([`SensorType`], [`SensorStatus`]); the user structures keep plain `i32`
//!   members (`r#type`, `status`), as `sysctl_rdstruct` needs a type valid for every bit
//!   pattern ([`SysctlPlain`]).
//! - `sensor_type_s[]` is `#ifndef _KERNEL` (userland only) and is not part of the kernel.
//! - The kernel structures are shared through pointers at `splhigh`, so their members are
//!   `Cell`s; every one of them is valid as all-zero bytes (a `ksensordev` lives in an
//!   `M_ZERO` softc, `softraid`'s).
//! - `struct sensor_task` is opaque here, as in C; the type is `kern_sensors.rs`'s and is
//!   re-exported.

use core::cell::Cell;

pub use crate::kern::kern_sensors::SensorTask;
use crate::queue_adapter;
use crate::sys::queue::{SlistEntry, SlistHead};
use crate::sys::sysctl::SysctlPlain;
use crate::sys::time::Timeval;

/// `enum sensor_type`: sensor types.
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(non_camel_case_types)] // OpenBSD names, verbatim, for grep-ability
pub enum SensorType {
    /// temperature (uK)
    SENSOR_TEMP = 0,
    /// fan revolution speed
    SENSOR_FANRPM,
    /// voltage (uV DC)
    SENSOR_VOLTS_DC,
    /// voltage (uV AC)
    SENSOR_VOLTS_AC,
    /// resistance
    SENSOR_OHMS,
    /// power (uW)
    SENSOR_WATTS,
    /// current (uA)
    SENSOR_AMPS,
    /// power capacity (uWh)
    SENSOR_WATTHOUR,
    /// power capacity (uAh)
    SENSOR_AMPHOUR,
    /// boolean indicator
    SENSOR_INDICATOR,
    /// generic integer value
    SENSOR_INTEGER,
    /// percent (m%)
    SENSOR_PERCENT,
    /// illuminance (ulx)
    SENSOR_LUX,
    /// disk
    SENSOR_DRIVE,
    /// system time error (nSec)
    SENSOR_TIMEDELTA,
    /// humidity (m%RH)
    SENSOR_HUMIDITY,
    /// frequency (uHz)
    SENSOR_FREQ,
    /// angle (uDegrees)
    SENSOR_ANGLE,
    /// distance (uMeter)
    SENSOR_DISTANCE,
    /// pressure (mPa)
    SENSOR_PRESSURE,
    /// acceleration (u m/s^2)
    SENSOR_ACCEL,
    /// velocity (u m/s)
    SENSOR_VELOCITY,
    /// energy (uJ)
    SENSOR_ENERGY,
}

pub use SensorType::*;

/// `SENSOR_MAX_TYPES`: the number of sensor types.
pub const SENSOR_MAX_TYPES: usize = SENSOR_ENERGY as usize + 1;

impl SensorType {
    /// Every type, in the C's order (index = value).
    const ALL: [SensorType; SENSOR_MAX_TYPES] = [
        SENSOR_TEMP,
        SENSOR_FANRPM,
        SENSOR_VOLTS_DC,
        SENSOR_VOLTS_AC,
        SENSOR_OHMS,
        SENSOR_WATTS,
        SENSOR_AMPS,
        SENSOR_WATTHOUR,
        SENSOR_AMPHOUR,
        SENSOR_INDICATOR,
        SENSOR_INTEGER,
        SENSOR_PERCENT,
        SENSOR_LUX,
        SENSOR_DRIVE,
        SENSOR_TIMEDELTA,
        SENSOR_HUMIDITY,
        SENSOR_FREQ,
        SENSOR_ANGLE,
        SENSOR_DISTANCE,
        SENSOR_PRESSURE,
        SENSOR_ACCEL,
        SENSOR_VELOCITY,
        SENSOR_ENERGY,
    ];

    /// The type whose C value is `v` (`(enum sensor_type)v`); `None` outside the enum.
    pub fn from_i32(v: i32) -> Option<Self> {
        usize::try_from(v)
            .ok()
            .and_then(|i| Self::ALL.get(i).copied())
    }
}

/// `SENSOR_DRIVE_EMPTY`.
pub const SENSOR_DRIVE_EMPTY: i64 = 1;
/// `SENSOR_DRIVE_READY`.
pub const SENSOR_DRIVE_READY: i64 = 2;
/// `SENSOR_DRIVE_POWERUP`.
pub const SENSOR_DRIVE_POWERUP: i64 = 3;
/// `SENSOR_DRIVE_ONLINE`.
pub const SENSOR_DRIVE_ONLINE: i64 = 4;
/// `SENSOR_DRIVE_IDLE`.
pub const SENSOR_DRIVE_IDLE: i64 = 5;
/// `SENSOR_DRIVE_ACTIVE`.
pub const SENSOR_DRIVE_ACTIVE: i64 = 6;
/// `SENSOR_DRIVE_REBUILD`.
pub const SENSOR_DRIVE_REBUILD: i64 = 7;
/// `SENSOR_DRIVE_POWERDOWN`.
pub const SENSOR_DRIVE_POWERDOWN: i64 = 8;
/// `SENSOR_DRIVE_FAIL`.
pub const SENSOR_DRIVE_FAIL: i64 = 9;
/// `SENSOR_DRIVE_PFAIL`.
pub const SENSOR_DRIVE_PFAIL: i64 = 10;

/// `enum sensor_status`: sensor states.
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(non_camel_case_types)] // OpenBSD names, verbatim, for grep-ability
pub enum SensorStatus {
    /// status is unspecified
    SENSOR_S_UNSPEC = 0,
    /// status is ok
    SENSOR_S_OK,
    /// status is warning
    SENSOR_S_WARN,
    /// status is critical
    SENSOR_S_CRIT,
    /// status is unknown
    SENSOR_S_UNKNOWN,
}

pub use SensorStatus::*;

/// `SENSOR_FINVALID`: sensor is invalid.
pub const SENSOR_FINVALID: i32 = 0x0001;
/// `SENSOR_FUNKNOWN`: sensor value is unknown.
pub const SENSOR_FUNKNOWN: i32 = 0x0002;

/// `struct sensor`: sensor data as `sysctl(2)` returns it. New fields should be added at the
/// end to encourage backwards compat.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Sensor {
    /// `desc`: sensor description, may be empty.
    pub desc: [u8; 32],
    /// `tv`: sensor value last change time.
    pub tv: Timeval,
    /// `value`: current value.
    pub value: i64,
    /// `type`: sensor type (an `enum sensor_type` value).
    pub r#type: i32,
    /// `status`: sensor status (an `enum sensor_status` value).
    pub status: i32,
    /// `numt`: sensor number of `.type` type.
    pub numt: i32,
    /// `flags`: sensor flags.
    pub flags: i32,
}

// SAFETY: `#[repr(C)]` of a byte array, a `Timeval` (two `i64`s) and integers, laid out
// without holes (the compile-time checks below), every bit pattern valid.
unsafe impl SysctlPlain for Sensor {}

/// `struct sensordev`: sensor device data as `sysctl(2)` returns it. New fields should be
/// added at the end to encourage backwards compat.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Sensordev {
    /// `num`: sensordev number.
    pub num: i32,
    /// `xname`: unix device name.
    pub xname: [u8; 16],
    /// `maxnumt`: per type, one past the highest `numt` attached.
    pub maxnumt: [i32; SENSOR_MAX_TYPES],
    /// `sensors_count`.
    pub sensors_count: i32,
}

// SAFETY: `#[repr(C)]` of `i32`s and a byte array, without holes (the compile-time checks
// below), every bit pattern valid.
unsafe impl SysctlPlain for Sensordev {}

/// `struct ksensor`: the kernel's sensor data.
///
/// Protected by: `splhigh` for the list link and `numt`; the driver owns the value members.
pub struct Ksensor {
    /// `list`: device-scope list.
    pub list: SlistEntry<Ksensor>,
    /// `desc`: sensor description, may be empty.
    pub desc: Cell<[u8; 32]>,
    /// `tv`: sensor value last change time.
    pub tv: Cell<Timeval>,
    /// `value`: current value.
    pub value: Cell<i64>,
    /// `type`: sensor type.
    pub r#type: Cell<SensorType>,
    /// `status`: sensor status.
    pub status: Cell<SensorStatus>,
    /// `numt`: sensor number of `.type` type.
    pub numt: Cell<i32>,
    /// `flags`: sensor flags, ie. `SENSOR_FINVALID`.
    pub flags: Cell<i32>,
}

// SAFETY: the link and `numt` change at `splhigh` (`sensor_attach`, `sensor_detach`), the
// values are written by the owning driver; the kernel runs one CPU.
unsafe impl Sync for Ksensor {}

impl Ksensor {
    /// A zeroed sensor (the C's `M_ZERO`/`bzero`ed `struct ksensor`).
    pub const fn new() -> Self {
        Self {
            list: SlistEntry::new(),
            desc: Cell::new([0; 32]),
            tv: Cell::new(Timeval {
                tv_sec: 0,
                tv_usec: 0,
            }),
            value: Cell::new(0),
            r#type: Cell::new(SENSOR_TEMP),
            status: Cell::new(SENSOR_S_UNSPEC),
            numt: Cell::new(0),
            flags: Cell::new(0),
        }
    }
}

impl Default for Ksensor {
    fn default() -> Self {
        Self::new()
    }
}

queue_adapter!(
    /// `SLIST_HEAD(ksensors_head, ksensor)`: a device's sensors, through `list`.
    pub KsensorList: Ksensor, list => SlistEntry<Ksensor>
);

/// `struct ksensors_head`.
pub type KsensorsHead = SlistHead<KsensorList>;

/// `struct ksensordev`: the kernel's sensor device data.
///
/// Protected by: `splhigh`.
pub struct Ksensordev {
    /// `list`: the link in `sensordev_list`.
    pub list: SlistEntry<Ksensordev>,
    /// `num`: sensordev number.
    pub num: Cell<i32>,
    /// `xname`: unix device name, NUL-terminated.
    pub xname: Cell<[u8; 16]>,
    /// `maxnumt`: per type, one past the highest `numt` attached.
    pub maxnumt: Cell<[i32; SENSOR_MAX_TYPES]>,
    /// `sensors_count`.
    pub sensors_count: Cell<i32>,
    /// `sensors_list`.
    pub sensors_list: KsensorsHead,
}

// SAFETY: the members change at `splhigh` in `kern_sensors.rs`; one CPU.
unsafe impl Sync for Ksensordev {}

impl Ksensordev {
    /// A zeroed sensor device.
    pub const fn new() -> Self {
        Self {
            list: SlistEntry::new(),
            num: Cell::new(0),
            xname: Cell::new([0; 16]),
            maxnumt: Cell::new([0; SENSOR_MAX_TYPES]),
            sensors_count: Cell::new(0),
            sensors_list: SlistHead::new(),
        }
    }
}

impl Default for Ksensordev {
    fn default() -> Self {
        Self::new()
    }
}

queue_adapter!(
    /// `SLIST_HEAD(, ksensordev) sensordev_list`: the installed sensor devices, through
    /// `list`.
    pub KsensordevList: Ksensordev, list => SlistEntry<Ksensordev>
);

const _: () = {
    assert!(core::mem::size_of::<Sensor>() == 32 + 16 + 8 + 4 * 4);
    assert!(core::mem::offset_of!(Sensor, tv) == 32);
    assert!(core::mem::offset_of!(Sensor, value) == 48);
    assert!(core::mem::size_of::<Sensordev>() == 4 + 16 + 4 * SENSOR_MAX_TYPES + 4);
    assert!(SENSOR_MAX_TYPES == 23);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sensor_type_round_trip() {
        for (i, t) in SensorType::ALL.iter().enumerate() {
            assert_eq!(*t as usize, i);
            assert_eq!(SensorType::from_i32(i as i32), Some(*t));
        }
        assert_eq!(SensorType::from_i32(-1), None);
        assert_eq!(SensorType::from_i32(SENSOR_MAX_TYPES as i32), None);
    }

    #[test]
    fn user_layouts() {
        assert_eq!(core::mem::size_of::<Sensor>(), 72);
        assert_eq!(core::mem::size_of::<Sensordev>(), 116);
    }
}
/* </TESTS> */
