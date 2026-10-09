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
/*-
 * Copyright (c) 1991 The Regents of the University of California.
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
 * 3. Neither the name of the University nor the names of its contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE REGENTS AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE REGENTS OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 *	@(#)comreg.h	7.2 (Berkeley) 5/9/91
 */
/* </LICENSES> */

/* <CODE> */
//! Register bits of the `com(4)` UARTs: `<dev/ic/comreg.h>`, which includes
//! `<dev/ic/ns16550reg.h>` (re-exported here, as the include does).
//!
//! Upstream: sys/dev/ic/comreg.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - `CPR_FIFO_MODE(x)` is the `const fn` [`cpr_fifo_mode`].
//! - `COM_FREQ` and `CONADDR` can be overridden from the kernel configuration in C; here they
//!   are the defaults (`CONADDR_OVERRIDE` does not exist).

pub use crate::dev::ic::ns16550reg::*;
use crate::machine::bus::{BusAddr, BusSize};

/// 16-bit baud rate divisor clock.
pub const COM_FREQ: i32 = 1_843_200;
/// Baud rate tolerance, in 0.1% units.
pub const COM_TOLERANCE: i32 = 30;

// interrupt enable register

/// Enable receiver interrupt.
pub const IER_ERXRDY: u8 = 0x1;
/// Enable transmitter empty interrupt.
pub const IER_ETXRDY: u8 = 0x2;
/// Enable line status interrupt.
pub const IER_ERLS: u8 = 0x4;
/// Enable modem status interrupt.
pub const IER_EMSC: u8 = 0x8;
/// Enable sleep mode.
pub const IER_SLEEP: u8 = 0x10;
// PXA2X0's ns16550 ports have extra bits in this register
/// Enable rx timeout interrupt.
pub const IER_ERXTOUT: u8 = 0x10;
/// Enable UART.
pub const IER_EUART: u8 = 0x40;

// interrupt identification register

/// Mask of the interrupt id.
pub const IIR_IMASK: u8 = 0xf;
/// Receiver timeout.
pub const IIR_RXTOUT: u8 = 0xc;
/// Line status change.
pub const IIR_RLS: u8 = 0x6;
/// Receiver ready.
pub const IIR_RXRDY: u8 = 0x4;
/// Transmitter ready.
pub const IIR_TXRDY: u8 = 0x2;
/// Modem status.
pub const IIR_MLSC: u8 = 0x0;
/// No pending interrupts.
pub const IIR_NOPEND: u8 = 0x1;
/// Set if FIFOs are enabled.
pub const IIR_FIFO_MASK: u8 = 0xc0;

// fifo control register

/// Turn the FIFO on.
pub const FIFO_ENABLE: u8 = 0x01;
/// Reset RX FIFO.
pub const FIFO_RCV_RST: u8 = 0x02;
/// Reset TX FIFO.
pub const FIFO_XMT_RST: u8 = 0x04;
/// DMA mode.
pub const FIFO_DMA_MODE: u8 = 0x08;
/// Trigger RXRDY intr on 1 character.
pub const FIFO_TRIGGER_1: u8 = 0x00;
/// Ibid 4.
pub const FIFO_TRIGGER_4: u8 = 0x40;
/// Ibid 8.
pub const FIFO_TRIGGER_8: u8 = 0x80;
/// Ibid 14.
pub const FIFO_TRIGGER_14: u8 = 0xc0;
// ST16650 fifo control register
/// ST16650: receive trigger 8.
pub const FIFO_RCV_TRIGGER_8: u8 = 0x00;
/// ST16650: receive trigger 16.
pub const FIFO_RCV_TRIGGER_16: u8 = 0x40;
/// ST16650: receive trigger 24.
pub const FIFO_RCV_TRIGGER_24: u8 = 0x80;
/// ST16650: receive trigger 28.
pub const FIFO_RCV_TRIGGER_28: u8 = 0xc0;
/// ST16650: transmit trigger 16.
pub const FIFO_XMT_TRIGGER_16: u8 = 0x00;
/// ST16650: transmit trigger 8.
pub const FIFO_XMT_TRIGGER_8: u8 = 0x10;
/// ST16650: transmit trigger 24.
pub const FIFO_XMT_TRIGGER_24: u8 = 0x20;
/// ST16650: transmit trigger 30.
pub const FIFO_XMT_TRIGGER_30: u8 = 0x30;
// XR16850 fifo control register
/// XR16850: receive trigger 8.
pub const FIFO_RCV3_TRIGGER_8: u8 = FIFO_RCV_TRIGGER_8;
/// XR16850: receive trigger 16.
pub const FIFO_RCV3_TRIGGER_16: u8 = FIFO_RCV_TRIGGER_16;
/// XR16850: receive trigger 56.
pub const FIFO_RCV3_TRIGGER_56: u8 = 0x80;
/// XR16850: receive trigger 60.
pub const FIFO_RCV3_TRIGGER_60: u8 = 0xc0;
/// XR16850: transmit trigger 8.
pub const FIFO_XMT3_TRIGGER_8: u8 = 0x00;
/// XR16850: transmit trigger 16.
pub const FIFO_XMT3_TRIGGER_16: u8 = 0x10;
/// XR16850: transmit trigger 32.
pub const FIFO_XMT3_TRIGGER_32: u8 = 0x20;
/// XR16850: transmit trigger 56.
pub const FIFO_XMT3_TRIGGER_56: u8 = 0x30;
// TI16750 fifo control register
/// TI16750: 64-byte FIFO.
pub const FIFO_ENABLE_64BYTE: u8 = 0x20;

// line control register

/// Divisor latch access enable.
pub const LCR_DLAB: u8 = 0x80;
/// Break Control.
pub const LCR_SBREAK: u8 = 0x40;
/// Space parity.
pub const LCR_PZERO: u8 = 0x38;
/// Mark parity.
pub const LCR_PONE: u8 = 0x28;
/// Even parity.
pub const LCR_PEVEN: u8 = 0x18;
/// Odd parity.
pub const LCR_PODD: u8 = 0x08;
/// No parity.
pub const LCR_PNONE: u8 = 0x00;
/// XXX - low order bit of all parity.
pub const LCR_PENAB: u8 = 0x08;
/// 2 stop bits per serial word.
pub const LCR_STOPB: u8 = 0x04;
/// 8 bits per serial word.
pub const LCR_8BITS: u8 = 0x03;
/// 7 bits.
pub const LCR_7BITS: u8 = 0x02;
/// 6 bits.
pub const LCR_6BITS: u8 = 0x01;
/// 5 bits.
pub const LCR_5BITS: u8 = 0x00;
/// ST16650/XR16850/OX16C950 EFR access enable.
pub const LCR_EFR: u8 = 0xbf;

// modem control register

/// Auto flow control.
pub const MCR_AFE: u8 = 0x20;
/// Loop test: echos from TX to RX.
pub const MCR_LOOPBACK: u8 = 0x10;
/// Out2: enables UART interrupts.
pub const MCR_IENABLE: u8 = 0x08;
/// Out1: resets some internal modems.
pub const MCR_DRS: u8 = 0x04;
/// Request To Send.
pub const MCR_RTS: u8 = 0x02;
/// Data Terminal Ready.
pub const MCR_DTR: u8 = 0x01;

// line status register

/// Error in receive FIFO.
pub const LSR_RCV_FIFO: u8 = 0x80;
/// Transmitter empty: byte sent.
pub const LSR_TSRE: u8 = 0x40;
/// Transmitter buffer empty.
pub const LSR_TXRDY: u8 = 0x20;
/// Break detected.
pub const LSR_BI: u8 = 0x10;
/// Framing error: bad stop bit.
pub const LSR_FE: u8 = 0x08;
/// Parity error.
pub const LSR_PE: u8 = 0x04;
/// Overrun, lost incoming byte.
pub const LSR_OE: u8 = 0x02;
/// Byte ready in Receive Buffer.
pub const LSR_RXRDY: u8 = 0x01;
/// Mask for incoming data or error.
pub const LSR_RCV_MASK: u8 = 0x1f;

// modem status register
// All deltas are from the last read of the MSR.

/// Current Data Carrier Detect.
pub const MSR_DCD: u8 = 0x80;
/// Current Ring Indicator.
pub const MSR_RI: u8 = 0x40;
/// Current Data Set Ready.
pub const MSR_DSR: u8 = 0x20;
/// Current Clear to Send.
pub const MSR_CTS: u8 = 0x10;
/// DCD has changed state.
pub const MSR_DDCD: u8 = 0x08;
/// RI has toggled low to high.
pub const MSR_TERI: u8 = 0x04;
/// DSR has changed state.
pub const MSR_DDSR: u8 = 0x02;
/// CTS has changed state.
pub const MSR_DCTS: u8 = 0x01;

// enhanced features register

/// Enhanced control bit.
pub const EFR_ECB: u8 = 0x10;
/// Special character detect.
pub const EFR_SCD: u8 = 0x20;
/// RTS flow control.
pub const EFR_RTS: u8 = 0x40;
/// CTS flow control.
pub const EFR_CTS: u8 = 0x80;

// enhanced FIFO control register

/// Mode.
pub const FCTL_MODE: u8 = 0x80;
/// Swap.
pub const FCTL_SWAP: u8 = 0x40;
/// RS485.
pub const FCTL_RS485: u8 = 0x08;
/// IrDA receive invert.
pub const FCTL_IRRXINV: u8 = 0x04;
/// Trigger 2.
pub const FCTL_TRIGGER2: u8 = 0x10;
/// Trigger 3.
pub const FCTL_TRIGGER3: u8 = 0x20;

// infrared selection register

/// Transmitter SIR enable.
pub const ISR_XMITIR: u8 = 0x01;
/// Receiver SIR enable.
pub const ISR_RCVEIR: u8 = 0x02;
/// 1.6us transmit pulse width.
pub const ISR_XMODE: u8 = 0x04;
/// Negative transmit data polarity.
pub const ISR_TXPL: u8 = 0x08;
/// Negative receive data polarity.
pub const ISR_RXPL: u8 = 0x10;

/// `CPR_FIFO_MODE(x)`: the FIFO mode field of the component parameter register (Synopsys
/// DesignWare APB UART).
pub const fn cpr_fifo_mode(x: u32) -> u32 {
    (x >> 16) & 0xff
}

/// Number of I/O ports a `com(4)` occupies.
pub const COM_NPORTS: BusSize = 8;

// Exar XR17V35X

/// Interrupt register 0.
pub const UART_EXAR_INT0: BusSize = 0x80;
/// Sleep mode.
pub const UART_EXAR_SLEEP: BusSize = 0x8b;
/// Device identification.
pub const UART_EXAR_DVID: BusSize = 0x8d;

/// WARNING: Serial console is assumed to be at COM1 address.
pub const CONADDR: BusAddr = 0x3f8;
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_headers() {
        let defs = crate::reftest::defines("sys/dev/ic/comreg.h");
        let ours: &[(&str, i64)] = &[
            ("COM_FREQ", COM_FREQ as i64),
            ("COM_TOLERANCE", COM_TOLERANCE as i64),
            ("IER_ERXRDY", IER_ERXRDY as i64),
            ("IER_ETXRDY", IER_ETXRDY as i64),
            ("IIR_IMASK", IIR_IMASK as i64),
            ("IIR_NOPEND", IIR_NOPEND as i64),
            ("IIR_FIFO_MASK", IIR_FIFO_MASK as i64),
            ("FIFO_ENABLE", FIFO_ENABLE as i64),
            ("FIFO_RCV_RST", FIFO_RCV_RST as i64),
            ("FIFO_XMT_RST", FIFO_XMT_RST as i64),
            ("FIFO_TRIGGER_1", FIFO_TRIGGER_1 as i64),
            ("FIFO_TRIGGER_14", FIFO_TRIGGER_14 as i64),
            ("LCR_DLAB", LCR_DLAB as i64),
            ("LCR_8BITS", LCR_8BITS as i64),
            ("LCR_EFR", LCR_EFR as i64),
            ("MCR_IENABLE", MCR_IENABLE as i64),
            ("MCR_RTS", MCR_RTS as i64),
            ("MCR_DTR", MCR_DTR as i64),
            ("LSR_TXRDY", LSR_TXRDY as i64),
            ("LSR_RXRDY", LSR_RXRDY as i64),
            ("LSR_RCV_MASK", LSR_RCV_MASK as i64),
            ("MSR_DCD", MSR_DCD as i64),
            ("MSR_DCTS", MSR_DCTS as i64),
            ("EFR_ECB", EFR_ECB as i64),
            ("COM_NPORTS", COM_NPORTS as i64),
            ("UART_EXAR_DVID", UART_EXAR_DVID as i64),
            ("CONADDR", CONADDR as i64),
        ];
        for (name, value) in ours {
            assert_eq!(crate::reftest::int(&defs, name), Some(*value), "{name}");
        }
        let regs = crate::reftest::defines("sys/dev/ic/ns16550reg.h");
        let ours: &[(&str, i64)] = &[
            ("com_data", COM_DATA as i64),
            ("com_dlbl", COM_DLBL as i64),
            ("com_dlbh", COM_DLBH as i64),
            ("com_ier", COM_IER as i64),
            ("com_iir", COM_IIR as i64),
            ("com_fifo", COM_FIFO as i64),
            ("com_fctl", COM_FCTL as i64),
            ("com_efr", COM_EFR as i64),
            ("com_lctl", COM_LCTL as i64),
            ("com_cfcr", COM_CFCR as i64),
            ("com_mcr", COM_MCR as i64),
            ("com_lsr", COM_LSR as i64),
            ("com_msr", COM_MSR as i64),
            ("com_scratch", COM_SCRATCH as i64),
            ("com_usr", COM_USR as i64),
            ("com_cpr", COM_CPR as i64),
        ];
        for (name, value) in ours {
            assert_eq!(crate::reftest::int(&regs, name), Some(*value), "{name}");
        }
    }
}
/* </TESTS> */
