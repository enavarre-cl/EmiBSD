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
 * Copyright (c) 2001 Constantine Sapuntzakis
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Neither the name of the author nor the names of any co-contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR
 * BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
 * CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
 * SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
 * INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
 * CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
 * ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF
 * THE POSSIBILITY OF SUCH DAMAGE.
 *
 */
/* </LICENSES> */

/* <CODE> */
//! The wdc(4) event trace: the record types of the ring buffer `wdc_log` keeps (`wdc.rs`)
//! when the kernel has `option WDCDEBUG`, and the `WDC_LOG_*` helpers the channel code calls
//! at each register access. `ATAIOGETTRACE` reads the ring back.
//!
//! Upstream: sys/dev/ic/wdcevent.h @ 3ce1f3f79392
//!
//! A record is the type byte, a byte holding the channel's log index (3 bits) and the
//! payload's length (5 bits), then the payload.
//!
//! ## Deviations
//! - `option WDCDEBUG` is the cargo feature `wdcdebug`: with it the helpers record through
//!   `wdc_log`; without it they are empty functions, as the C's empty macros.
//! - `enum wdcevent_type` is [`WdceventType`], `#[repr(u8)]` (its value is the type byte).
//! - The helpers' `char record[]` arrays are byte arrays of the payload's length (the C's
//!   `WDC_LOG_ATA_CMDLONG` declares eight bytes and logs seven); `WDC_LOG_ATAPI_CMD` takes
//!   the command bytes as a slice (`len` is its length) and logs at most the 18 that fit its
//!   20-byte record, where the C would overrun it.

#![allow(non_snake_case)] // the C's WDC_LOG_* names

use crate::dev::ic::wdcvar::{ChannelSoftc, WdcRegs};

/// `enum wdcevent_type`.
#[repr(u8)]
#[allow(non_camel_case_types)] // the C names
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WdceventType {
    /// `WDCEVENT_STATUS`.
    WDCEVENT_STATUS = 1,
    /// `WDCEVENT_ERROR`.
    WDCEVENT_ERROR,
    /// `WDCEVENT_ATAPI_CMD`.
    WDCEVENT_ATAPI_CMD,
    /// `WDCEVENT_ATAPI_DONE`.
    WDCEVENT_ATAPI_DONE,
    /// `WDCEVENT_ATA_SHORT`.
    WDCEVENT_ATA_SHORT,
    /// `WDCEVENT_ATA_LONG`.
    WDCEVENT_ATA_LONG,
    /// `WDCEVENT_SET_DRIVE1`.
    WDCEVENT_SET_DRIVE1,
    /// `WDCEVENT_SET_DRIVE0`.
    WDCEVENT_SET_DRIVE0,
    /// `WDCEVENT_REG`.
    WDCEVENT_REG,
    /// `WDCEVENT_ATA_EXT`.
    WDCEVENT_ATA_EXT,
}

pub use WdceventType::*;

/// `WDC_LOG_STATUS`: records a status register value that differs from the last one.
#[cfg(feature = "wdcdebug")]
pub fn WDC_LOG_STATUS(chp: &ChannelSoftc, status: u8) {
    if chp.ch_prev_log_status.get() == status {
        return;
    }

    chp.ch_prev_log_status.set(status);
    crate::dev::ic::wdc::wdc_log(chp, WDCEVENT_STATUS, &[status]);
}

/// `WDC_LOG_ERROR`.
#[cfg(feature = "wdcdebug")]
pub fn WDC_LOG_ERROR(chp: &ChannelSoftc, error: u8) {
    crate::dev::ic::wdc::wdc_log(chp, WDCEVENT_ERROR, &[error]);
}

/// `WDC_LOG_ATAPI_CMD`: the transfer's flags and the packet command.
#[cfg(feature = "wdcdebug")]
pub fn WDC_LOG_ATAPI_CMD(chp: &ChannelSoftc, _drive: i32, flags: i32, cmd: &[u8]) {
    let mut record = [0u8; 20];

    record[0] = (flags >> 8) as u8;
    record[1] = (flags & 0xff) as u8;
    let len = cmd.len().min(record.len() - 2);
    record[2..2 + len].copy_from_slice(&cmd[..len]);

    crate::dev::ic::wdc::wdc_log(chp, WDCEVENT_ATAPI_CMD, &record[..len + 2]);
}

/// `WDC_LOG_ATAPI_DONE`.
#[cfg(feature = "wdcdebug")]
pub fn WDC_LOG_ATAPI_DONE(chp: &ChannelSoftc, _drive: i32, flags: i32, error: u8) {
    let record = [(flags >> 8) as u8, (flags & 0xff) as u8, error];
    crate::dev::ic::wdc::wdc_log(chp, WDCEVENT_ATAPI_DONE, &record);
}

/// `WDC_LOG_ATA_CMDSHORT`.
#[cfg(feature = "wdcdebug")]
pub fn WDC_LOG_ATA_CMDSHORT(chp: &ChannelSoftc, cmd: u8) {
    crate::dev::ic::wdc::wdc_log(chp, WDCEVENT_ATA_SHORT, &[cmd]);
}

/// `WDC_LOG_ATA_CMDLONG`.
#[cfg(feature = "wdcdebug")]
#[allow(clippy::too_many_arguments)] // the C helper's eight arguments
pub fn WDC_LOG_ATA_CMDLONG(
    chp: &ChannelSoftc,
    head: u8,
    features: u8,
    cylinhi: u8,
    cylinlo: u8,
    sector: u8,
    count: u8,
    command: u8,
) {
    let record = [head, features, cylinhi, cylinlo, sector, count, command];

    crate::dev::ic::wdc::wdc_log(chp, WDCEVENT_ATA_LONG, &record);
}

/// `WDC_LOG_SET_DRIVE`.
#[cfg(feature = "wdcdebug")]
pub fn WDC_LOG_SET_DRIVE(chp: &ChannelSoftc, drive: u8) {
    let r#type = if drive != 0 {
        WDCEVENT_SET_DRIVE1
    } else {
        WDCEVENT_SET_DRIVE0
    };
    crate::dev::ic::wdc::wdc_log(chp, r#type, &[]);
}

/// `WDC_LOG_REG`.
#[cfg(feature = "wdcdebug")]
pub fn WDC_LOG_REG(chp: &ChannelSoftc, reg: WdcRegs, val: u16) {
    let record = [reg.0 as u8, (val >> 8) as u8, (val & 0xff) as u8];

    crate::dev::ic::wdc::wdc_log(chp, WDCEVENT_REG, &record);
}

/// `WDC_LOG_ATA_CMDEXT`.
#[cfg(feature = "wdcdebug")]
#[allow(clippy::too_many_arguments)] // the C helper's ten arguments
pub fn WDC_LOG_ATA_CMDEXT(
    chp: &ChannelSoftc,
    lba_hi1: u8,
    lba_hi2: u8,
    lba_mi1: u8,
    lba_mi2: u8,
    lba_lo1: u8,
    lba_lo2: u8,
    count1: u8,
    count2: u8,
    command: u8,
) {
    let record = [
        lba_hi1, lba_hi2, lba_mi1, lba_mi2, lba_lo1, lba_lo2, count1, count2, command,
    ];

    crate::dev::ic::wdc::wdc_log(chp, WDCEVENT_ATA_EXT, &record);
}

/// `WDC_LOG_STATUS(chp, status)`: nothing without `WDCDEBUG`.
#[cfg(not(feature = "wdcdebug"))]
#[inline(always)]
pub fn WDC_LOG_STATUS(_chp: &ChannelSoftc, _status: u8) {}

/// `WDC_LOG_ERROR(chp, error)`: nothing without `WDCDEBUG`.
#[cfg(not(feature = "wdcdebug"))]
#[inline(always)]
pub fn WDC_LOG_ERROR(_chp: &ChannelSoftc, _error: u8) {}

/// `WDC_LOG_ATAPI_CMD(chp, drive, flags, len, cmd)`: nothing without `WDCDEBUG`.
#[cfg(not(feature = "wdcdebug"))]
#[inline(always)]
pub fn WDC_LOG_ATAPI_CMD(_chp: &ChannelSoftc, _drive: i32, _flags: i32, _cmd: &[u8]) {}

/// `WDC_LOG_ATAPI_DONE(chp, drive, flags, error)`: nothing without `WDCDEBUG`.
#[cfg(not(feature = "wdcdebug"))]
#[inline(always)]
pub fn WDC_LOG_ATAPI_DONE(_chp: &ChannelSoftc, _drive: i32, _flags: i32, _error: u8) {}

/// `WDC_LOG_ATA_CMDSHORT(chp, cmd)`: nothing without `WDCDEBUG`.
#[cfg(not(feature = "wdcdebug"))]
#[inline(always)]
pub fn WDC_LOG_ATA_CMDSHORT(_chp: &ChannelSoftc, _cmd: u8) {}

/// `WDC_LOG_ATA_CMDLONG(...)`: nothing without `WDCDEBUG`.
#[cfg(not(feature = "wdcdebug"))]
#[inline(always)]
#[allow(clippy::too_many_arguments)] // the C helper's eight arguments
pub fn WDC_LOG_ATA_CMDLONG(
    _chp: &ChannelSoftc,
    _head: u8,
    _features: u8,
    _cylinhi: u8,
    _cylinlo: u8,
    _sector: u8,
    _count: u8,
    _command: u8,
) {
}

/// `WDC_LOG_SET_DRIVE(chp, drive)`: nothing without `WDCDEBUG`.
#[cfg(not(feature = "wdcdebug"))]
#[inline(always)]
pub fn WDC_LOG_SET_DRIVE(_chp: &ChannelSoftc, _drive: u8) {}

/// `WDC_LOG_REG(chp, reg, val)`: nothing without `WDCDEBUG`.
#[cfg(not(feature = "wdcdebug"))]
#[inline(always)]
pub fn WDC_LOG_REG(_chp: &ChannelSoftc, _reg: WdcRegs, _val: u16) {}

/// `WDC_LOG_ATA_CMDEXT(...)`: nothing without `WDCDEBUG`.
#[cfg(not(feature = "wdcdebug"))]
#[inline(always)]
#[allow(clippy::too_many_arguments)] // the C helper's ten arguments
pub fn WDC_LOG_ATA_CMDEXT(
    _chp: &ChannelSoftc,
    _lba_hi1: u8,
    _lba_hi2: u8,
    _lba_mi1: u8,
    _lba_mi2: u8,
    _lba_lo1: u8,
    _lba_lo2: u8,
    _count1: u8,
    _count2: u8,
    _command: u8,
) {
}
/* </CODE> */
