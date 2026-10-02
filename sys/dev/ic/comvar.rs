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

//! The `com(4)` driver's private header: `<dev/ic/comvar.h>`, what `com.c` shares with its bus
//! front-ends (`com_isa`, `com_pci`, `com_acpi`, `com_fdt`, `commulti`).
//!
//! Upstream: sys/dev/ic/comvar.h @ 3ce1f3f79392
//!
//! Status: `wip`. The constants, `struct commulti_attach_args` and the fields of
//! `struct com_softc` whose types exist are here. The prototypes it declares are the functions of
//! `com.rs` (the console ones are there; `comintr`, `comparam`, `comstart`, `com_attach_subr` and
//! the rest arrive with the tty side, M7), and the `comcons*` externs are the globals `com.rs`
//! defines.
//!
//! ## Deviations
//! - `sc_dev` (`struct device`, autoconf M4), `sc_ih` and `sc_si` (interrupt and soft-interrupt
//!   handles, M4), `sc_tty` (M7) and the two `struct timeout`s (`sc_dtr_tmo`, `sc_diag_tmo`, M5)
//!   are not in [`ComSoftc`] yet; they are added when their types are ported.
//! - The input ring's four pointers (`sc_ibuf`, `sc_ibufp`, `sc_ibufhigh`, `sc_ibufend`) are an
//!   index into `sc_ibufs` and a fill count ([`ComSoftc::sc_ibuf`], [`ComSoftc::sc_ibufp`]); the
//!   high-water mark and the end are the constants `COM_IHIGHWATER` and `COM_IBUFSIZE`.
//! - The power-management hooks return `Result` where `enable` returns an `int` errno.
//! - `ca_noien` is a `bool`; `ca_iobase` is a `BusAddr` (an `int` in C).

use crate::machine::bus::{BusAddr, BusSpaceHandle, BusSpaceTag};
use crate::sys::errno::Errno;

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
pub type ComEnableFn = fn(&mut ComSoftc) -> Result<(), Errno>;
/// The `disable` power-management hook: powers a port down.
pub type ComDisableFn = fn(&mut ComSoftc);

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

/// `struct com_softc`: the state of one `com(4)` port (see the module's deviations for the
/// fields that are not here yet).
pub struct ComSoftc {
    /// The port's bus space.
    pub sc_iot: BusSpaceTag,
    /// Input ring overflows seen.
    pub sc_overflows: i32,
    /// Input floods (high water reached) seen.
    pub sc_floods: i32,
    /// Line errors seen.
    pub sc_errors: i32,
    /// Output halted (`comstop`).
    pub sc_halt: i32,
    /// The port's base address.
    pub sc_iobase: BusAddr,
    /// The UART's clock, in Hz.
    pub sc_frequency: i32,
    /// The port's registers.
    pub sc_ioh: BusSpaceHandle,
    /// Register width in bytes (4 for memory-mapped 32-bit registers).
    pub sc_reg_width: u8,
    /// Register stride, as a shift count.
    pub sc_reg_shift: u8,
    /// `COM_UART_*`: the chip `com_attach_subr` identified.
    pub sc_uarttype: u8,
    /// `COM_HW_*`.
    pub sc_hwflags: u8,
    /// `COM_SW_*`.
    pub sc_swflags: u8,
    /// Depth of the FIFO, when it works.
    pub sc_fifolen: i32,
    /// Last modem status register value.
    pub sc_msr: u8,
    /// Modem control register shadow.
    pub sc_mcr: u8,
    /// Line control register shadow.
    pub sc_lcr: u8,
    /// Interrupt enable register shadow.
    pub sc_ier: u8,
    /// The MCR bit that is DTR on this port.
    pub sc_dtr: u8,
    /// The port is open through its call-out (cua) device.
    pub sc_cua: u8,
    /// Force initialization.
    pub sc_initialize: u8,
    /// Which of `sc_ibufs` is being filled.
    pub sc_ibuf: usize,
    /// Fill count of the current input buffer.
    pub sc_ibufp: usize,
    /// The two input ring buffers (one fills while the other drains).
    pub sc_ibufs: [[u8; COM_IBUFSIZE]; 2],
    // power management hooks
    /// Powers the port up.
    pub enable: Option<ComEnableFn>,
    /// Powers the port down.
    pub disable: Option<ComDisableFn>,
    /// Whether the port is powered.
    pub enabled: i32,
}

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
