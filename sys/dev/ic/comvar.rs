/*	$OpenBSD: comvar.h,v 1.62 2026/04/06 10:27:53 kettenis Exp $	*/
/*	$NetBSD: comvar.h,v 1.5 1996/05/05 19:50:47 christos Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1997 - 1998, Jason Downs.  All rights reserved.
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
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR(S) ``AS IS'' AND ANY EXPRESS
 * OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED
 * WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
 * DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR(S) BE LIABLE FOR ANY DIRECT,
 * INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES
 * (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
 * SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
 * CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 */

/*
 * Copyright (c) 1996 Christopher G. Demetriou.  All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. All advertising materials mentioning features or use of this software
 *    must display the following acknowledgement:
 *      This product includes software developed by Christopher G. Demetriou
 *	for the NetBSD Project.
 * 4. The name of the author may not be used to endorse or promote products
 *    derived from this software without specific prior written permission
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
//! The `com(4)` driver's private header: `<dev/ic/comvar.h>`, what `com.c` shares with its bus
//! front-ends (`com_isa`, `com_pci`, `com_acpi`, `com_fdt`, `commulti`).
//!
//! Upstream: sys/dev/ic/comvar.h @ 3ce1f3f79392
//!
//! The prototypes it declares are the functions of `com.rs`, and the `comcons*` externs are
//! the globals `com.rs` defines.
//!
//! ## Deviations
//! - [`ComSoftc`] is a softc (`docs/C_TO_RUST.md`): `#[repr(C)]`, the device first, every
//!   member a `Cell` (the driver, its interrupt handlers and the tty layer change them through
//!   `struct com_softc *`), all-zero valid for `config_make_softc`'s `M_ZERO`. The bus tag
//!   and handle are `Option`s (unset before the attachment fills them); `sc_ih`, `sc_si` and
//!   `sc_tty` are pointers in `Cell`s.
//! - The input ring's four pointers (`sc_ibuf`, `sc_ibufp`, `sc_ibufhigh`, `sc_ibufend`) are
//!   the index of the buffer being filled in `sc_ibufs` and its fill count
//!   ([`ComSoftc::sc_ibuf`], [`ComSoftc::sc_ibufp`]); the high-water mark and the end are the
//!   constants `COM_IHIGHWATER` and `COM_IBUFSIZE`. The buffers are `Cell<u8>`s, written by
//!   `comintr` and read by `comsoft`.
//! - The power-management hooks return `Result` where `enable` returns an `int` errno.
//! - `ca_noien` is a `bool`; `ca_iobase` is a `BusAddr` (an `int` in C).

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::NonNull;

use crate::kern::kern_softintr::SoftintrHand;
use crate::machine::bus::{BusAddr, BusSpaceHandle, BusSpaceTag};
use crate::sys::device::{Device, Softc};
use crate::sys::errno::Errno;
use crate::sys::timeout::Timeout;
use crate::sys::tty::Tty;

/// `struct commulti_attach_args`: how a multi-port board attaches each of its ports.
pub struct CommultiAttachArgs {
    /// Slave number.
    pub ca_slave: i32,
    /// The board's bus space.
    pub ca_iot: BusSpaceTag,
    /// The port's registers.
    pub ca_ioh: BusSpaceHandle,
    /// The port's base address.
    pub ca_iobase: BusAddr,
    /// Whether the port must not drive OUT2 (`COM_HW_NOIEN`).
    pub ca_noien: bool,
}

/// Size of one input ring buffer.
pub const COM_IBUFSIZE: usize = 32 * 512;
/// Fill level at which input flow control kicks in.
pub const COM_IHIGHWATER: usize = (3 * COM_IBUFSIZE) / 4;

// sc_uarttype

/// Unknown.
pub const COM_UART_UNKNOWN: u8 = 0x00;
/// No fifo.
pub const COM_UART_8250: u8 = 0x01;
/// No fifo.
pub const COM_UART_16450: u8 = 0x02;
/// No working fifo.
pub const COM_UART_16550: u8 = 0x03;
/// 16 byte fifo.
pub const COM_UART_16550A: u8 = 0x04;
/// No working fifo.
pub const COM_UART_ST16650: u8 = 0x05;
/// 32 byte fifo.
pub const COM_UART_ST16650V2: u8 = 0x06;
/// 64 byte fifo.
pub const COM_UART_TI16750: u8 = 0x07;
/// 64 bytes fifo.
pub const COM_UART_ST16C654: u8 = 0x08;
/// 128 byte fifo.
pub const COM_UART_XR16850: u8 = 0x10;
/// 128 byte fifo.
pub const COM_UART_OX16C950: u8 = 0x11;
/// 256 byte fifo.
pub const COM_UART_XR17V35X: u8 = 0x12;
/// Configurable.
pub const COM_UART_DW_APB: u8 = 0x13;
/// 32 byte fifo.
pub const COM_UART_PXA2X0: u8 = 0x14;

// sc_hwflags

/// Never enable the interrupt gate (OUT2).
pub const COM_HW_NOIEN: u8 = 0x01;
/// The FIFO works.
pub const COM_HW_FIFO: u8 = 0x02;
/// Infrared (SIR) port.
pub const COM_HW_SIR: u8 = 0x20;
/// This port is the console.
pub const COM_HW_CONSOLE: u8 = 0x40;

// sc_swflags

/// Soft carrier: ignore DCD.
pub const COM_SW_SOFTCAR: u8 = 0x01;
/// Local line: CLOCAL by default.
pub const COM_SW_CLOCAL: u8 = 0x02;
/// Hardware flow control by default.
pub const COM_SW_CRTSCTS: u8 = 0x04;
/// DTR/DCD flow control by default.
pub const COM_SW_MDMBUF: u8 = 0x08;
/// Pulse-per-second input.
pub const COM_SW_PPS: u8 = 0x10;
/// The port was lost (hot-unplugged).
pub const COM_SW_DEAD: u8 = 0x20;

/// The `enable` power-management hook: powers a port up, with an errno on failure.
pub type ComEnableFn = fn(&ComSoftc) -> Result<(), Errno>;
/// The `disable` power-management hook: powers a port down.
pub type ComDisableFn = fn(&ComSoftc);

/// `struct com_softc`: the state of one `com(4)` port.
#[repr(C)]
pub struct ComSoftc {
    /// `sc_dev`.
    pub sc_dev: Device,
    /// `sc_ih`: the interrupt handle.
    pub sc_ih: Cell<*mut c_void>,
    /// `sc_iot`: the port's bus space.
    pub sc_iot: Cell<Option<BusSpaceTag>>,
    /// `sc_tty`: NULL until the first open.
    pub sc_tty: Cell<*const Tty>,
    /// `sc_dtr_tmo`: raises DTR again after a close.
    pub sc_dtr_tmo: Timeout,
    /// `sc_diag_tmo`: reports overflows.
    pub sc_diag_tmo: Timeout,
    /// `sc_si`: the soft interrupt that drains the input ring.
    pub sc_si: Cell<Option<NonNull<SoftintrHand>>>,

    /// `sc_overflows`: input ring overflows seen.
    pub sc_overflows: Cell<i32>,
    /// `sc_floods`: input floods (high water reached) seen.
    pub sc_floods: Cell<i32>,
    /// `sc_errors`: line errors seen.
    pub sc_errors: Cell<i32>,

    /// `sc_halt`: output halted (`comparam` waiting for the transmitter).
    pub sc_halt: Cell<i32>,

    /// `sc_iobase`: the port's base address.
    pub sc_iobase: Cell<BusAddr>,
    /// `sc_frequency`: the UART's clock, in Hz.
    pub sc_frequency: Cell<i32>,

    /// `sc_ioh`: the port's registers.
    pub sc_ioh: Cell<Option<BusSpaceHandle>>,
    /// `sc_reg_width`: register width in bytes (4 for memory-mapped 32-bit registers).
    pub sc_reg_width: Cell<u8>,
    /// `sc_reg_shift`: register stride, as a shift count.
    pub sc_reg_shift: Cell<u8>,

    /// `sc_uarttype`: `COM_UART_*`, the chip `com_attach_subr` identified.
    pub sc_uarttype: Cell<u8>,
    /// `sc_uartrev`.
    pub sc_uartrev: Cell<u8>,
    /// `sc_hwflags`: `COM_HW_*`.
    pub sc_hwflags: Cell<u8>,
    /// `sc_swflags`: `COM_SW_*`.
    pub sc_swflags: Cell<u8>,
    /// `sc_fifolen`: depth of the FIFO, when it works.
    pub sc_fifolen: Cell<i32>,
    /// `sc_msr`: last modem status register value.
    pub sc_msr: Cell<u8>,
    /// `sc_mcr`: modem control register shadow.
    pub sc_mcr: Cell<u8>,
    /// `sc_lcr`: line control register shadow.
    pub sc_lcr: Cell<u8>,
    /// `sc_ier`: interrupt enable register shadow.
    pub sc_ier: Cell<u8>,
    /// `sc_dtr`: the MCR bits that are DTR on this port.
    pub sc_dtr: Cell<u8>,

    /// `sc_cua`: the port is open through its call-out (cua) device.
    pub sc_cua: Cell<u8>,

    /// `sc_initialize`: force initialization.
    pub sc_initialize: Cell<u8>,

    /// `sc_ibuf`: which of `sc_ibufs` is being filled (0 or 1).
    pub sc_ibuf: Cell<usize>,
    /// `sc_ibufp - sc_ibuf`: the bytes filled in it.
    pub sc_ibufp: Cell<usize>,
    /// `sc_ibufs`: the two input ring buffers (one fills while the other drains), data and
    /// line status bytes in pairs.
    pub sc_ibufs: [[Cell<u8>; COM_IBUFSIZE]; 2],

    // power management hooks
    /// `enable`: powers the port up.
    pub enable: Cell<Option<ComEnableFn>>,
    /// `disable`: powers the port down.
    pub disable: Cell<Option<ComDisableFn>>,
    /// `enabled`: whether the port is powered.
    pub enabled: Cell<i32>,
}

// SAFETY: `repr(C)` with the device first; every member is a `Cell` of an integer, a raw
// pointer, an `Option` of a reference, handle or function, or a `Timeout`, all of which are
// valid all-zero (the `Option`s are `None`, the tag an arbitrary but valid space).
unsafe impl Softc for ComSoftc {}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_sizes() {
        assert_eq!(COM_IBUFSIZE, 16 * 1024);
        assert_eq!(COM_IHIGHWATER, 12 * 1024);
        assert!(COM_IHIGHWATER < COM_IBUFSIZE);
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/ic/comvar.h");
        let ours: &[(&str, i64)] = &[
            ("COM_IBUFSIZE", COM_IBUFSIZE as i64),
            ("COM_UART_UNKNOWN", COM_UART_UNKNOWN as i64),
            ("COM_UART_8250", COM_UART_8250 as i64),
            ("COM_UART_16550A", COM_UART_16550A as i64),
            ("COM_UART_TI16750", COM_UART_TI16750 as i64),
            ("COM_UART_XR16850", COM_UART_XR16850 as i64),
            ("COM_UART_XR17V35X", COM_UART_XR17V35X as i64),
            ("COM_UART_DW_APB", COM_UART_DW_APB as i64),
            ("COM_UART_PXA2X0", COM_UART_PXA2X0 as i64),
            ("COM_HW_NOIEN", COM_HW_NOIEN as i64),
            ("COM_HW_FIFO", COM_HW_FIFO as i64),
            ("COM_HW_SIR", COM_HW_SIR as i64),
            ("COM_HW_CONSOLE", COM_HW_CONSOLE as i64),
            ("COM_SW_SOFTCAR", COM_SW_SOFTCAR as i64),
            ("COM_SW_CLOCAL", COM_SW_CLOCAL as i64),
            ("COM_SW_CRTSCTS", COM_SW_CRTSCTS as i64),
            ("COM_SW_MDMBUF", COM_SW_MDMBUF as i64),
            ("COM_SW_PPS", COM_SW_PPS as i64),
            ("COM_SW_DEAD", COM_SW_DEAD as i64),
        ];
        for (name, value) in ours {
            assert_eq!(crate::reftest::int(&defs, name), Some(*value), "{name}");
        }
    }
}
/* </TESTS> */
