/*	$OpenBSD: ucomvar.h,v 1.20 2023/10/01 15:58:11 krw Exp $ */
/*	$NetBSD: ucomvar.h,v 1.10 2001/12/31 12:15:21 augustss Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1999 The NetBSD Foundation, Inc.
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
//! `<dev/usb/ucomvar.h>`: what a USB serial driver (`uftdi(4)`, ...) hands `ucom(4)`: the
//! attach arguments, the methods table and the modem and line status bits.
//!
//! Upstream: sys/dev/usb/ucomvar.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - `struct ucom_methods`' entries are `Option`s of Rust functions (the C's NULLs are
//!   `None`). The `void *sc` they get back is `*mut c_void` as in C (`ucom_attach_args.arg`).
//!   `ucom_get_status` takes the two status bytes as `&mut u8`s (ucom always passes both; the C
//!   drivers test for NULL); `ucom_param` and `ucom_ioctl` return `Result<(), Errno>` (the C's
//!   errno or 0; `ucom_ioctl` answers `ENOTTY` for a command it does not handle);
//!   `ucom_read` gets the received bytes as a slice it advances past the header (the C's
//!   `u_char **ptr` and `u_int32_t *count`); `ucom_write` gets the destination and the data
//!   as slices and answers the length of the packet it made (the C's `to`, `from` and
//!   `u_int32_t *count`).
//! - The prototypes at the end of the C header (`ucomsubmatch`, `sysctl_ucominit`,
//!   `ucomprint`, `ucom_status_change`) are the functions of `ucom.rs`.

use core::ffi::c_void;

use crate::dev::usb::uhidev::UhidevSoftc;
use crate::dev::usb::usbdivar::{UsbdDevice, UsbdInterface};
use crate::sys::errno::Errno;
use crate::sys::proc::Proc;
use crate::sys::termios::Termios;

/// `UCOMBUSCF_PORTNO`: the index of the `portno` locator.
pub const UCOMBUSCF_PORTNO: usize = 0;
/// `UCOMBUSCF_PORTNO_DEFAULT`.
pub const UCOMBUSCF_PORTNO_DEFAULT: i32 = -1;

/// `UCOM_UNK_PORTNO`.
pub const UCOM_UNK_PORTNO: i32 = UCOMBUSCF_PORTNO_DEFAULT;

/// `UCOM_SET_DTR`: `ucom_set`'s register for the DTR line.
pub const UCOM_SET_DTR: i32 = 1;
/// `UCOM_SET_RTS`.
pub const UCOM_SET_RTS: i32 = 2;
/// `UCOM_SET_BREAK`.
pub const UCOM_SET_BREAK: i32 = 3;

/// `ucom_get_status`.
pub type UcomGetStatusFn = fn(sc: *mut c_void, portno: i32, lsr: &mut u8, msr: &mut u8);
/// `ucom_set`.
pub type UcomSetFn = fn(sc: *mut c_void, portno: i32, reg: i32, onoff: i32);
/// `ucom_param`.
pub type UcomParamFn = fn(sc: *mut c_void, portno: i32, t: &Termios) -> Result<(), Errno>;
/// `ucom_ioctl`.
pub type UcomIoctlFn = fn(
    sc: *mut c_void,
    portno: i32,
    cmd: u64,
    data: &mut [u8],
    flag: i32,
    p: &Proc,
) -> Result<(), Errno>;
/// `ucom_open`.
pub type UcomOpenFn = fn(sc: *mut c_void, portno: i32) -> Result<(), Errno>;
/// `ucom_close`.
pub type UcomCloseFn = fn(sc: *mut c_void, portno: i32);
/// `ucom_read`: `data` is what was received; the function moves it past the packet header.
pub type UcomReadFn = fn(sc: *mut c_void, portno: i32, data: &mut &[u8]);
/// `ucom_write`: puts the packet for `from` (the tty's bytes) in `to`, and answers its length.
pub type UcomWriteFn = fn(sc: *mut c_void, portno: i32, to: &mut [u8], from: &[u8]) -> usize;

/// `struct ucom_methods`.
pub struct UcomMethods {
    /// `ucom_get_status`.
    pub ucom_get_status: Option<UcomGetStatusFn>,
    /// `ucom_set`.
    pub ucom_set: Option<UcomSetFn>,
    /// `ucom_param`.
    pub ucom_param: Option<UcomParamFn>,
    /// `ucom_ioctl`.
    pub ucom_ioctl: Option<UcomIoctlFn>,
    /// `ucom_open`.
    pub ucom_open: Option<UcomOpenFn>,
    /// `ucom_close`.
    pub ucom_close: Option<UcomCloseFn>,
    /// `ucom_read`.
    pub ucom_read: Option<UcomReadFn>,
    /// `ucom_write`.
    pub ucom_write: Option<UcomWriteFn>,
}

/* modem control register */
/// `UMCR_RTS`: Request To Send.
pub const UMCR_RTS: u8 = 0x02;
/// `UMCR_DTR`: Data Terminal Ready.
pub const UMCR_DTR: u8 = 0x01;

/* line status register */
/// `ULSR_RCV_FIFO`.
pub const ULSR_RCV_FIFO: u8 = 0x80;
/// `ULSR_TSRE`: Transmitter empty: byte sent.
pub const ULSR_TSRE: u8 = 0x40;
/// `ULSR_TXRDY`: Transmitter buffer empty.
pub const ULSR_TXRDY: u8 = 0x20;
/// `ULSR_BI`: Break detected.
pub const ULSR_BI: u8 = 0x10;
/// `ULSR_FE`: Framing error: bad stop bit.
pub const ULSR_FE: u8 = 0x08;
/// `ULSR_PE`: Parity error.
pub const ULSR_PE: u8 = 0x04;
/// `ULSR_OE`: Overrun, lost incoming byte.
pub const ULSR_OE: u8 = 0x02;
/// `ULSR_RXRDY`: Byte ready in Receive Buffer.
pub const ULSR_RXRDY: u8 = 0x01;
/// `ULSR_RCV_MASK`: Mask for incoming data or error.
pub const ULSR_RCV_MASK: u8 = 0x1f;

/* modem status register */
/* All deltas are from the last read of the MSR. */
/// `UMSR_DCD`: Current Data Carrier Detect.
pub const UMSR_DCD: u8 = 0x80;
/// `UMSR_RI`: Current Ring Indicator.
pub const UMSR_RI: u8 = 0x40;
/// `UMSR_DSR`: Current Data Set Ready.
pub const UMSR_DSR: u8 = 0x20;
/// `UMSR_CTS`: Current Clear to Send.
pub const UMSR_CTS: u8 = 0x10;
/// `UMSR_DDCD`: DCD has changed state.
pub const UMSR_DDCD: u8 = 0x08;
/// `UMSR_TERI`: RI has toggled low to high.
pub const UMSR_TERI: u8 = 0x04;
/// `UMSR_DDSR`: DSR has changed state.
pub const UMSR_DDSR: u8 = 0x02;
/// `UMSR_DCTS`: CTS has changed state.
pub const UMSR_DCTS: u8 = 0x01;

/// `struct ucom_attach_args`.
pub struct UcomAttachArgs {
    /// `portno`.
    pub portno: i32,
    /// `bulkin`: the bulk-in endpoint's address, -1 for none (a HID-based driver).
    pub bulkin: i32,
    /// `bulkout`: the bulk-out endpoint's address, -1 for none.
    pub bulkout: i32,
    /// `uhidev`: the HID device that carries the port, for the HID-based drivers.
    pub uhidev: Option<&'static UhidevSoftc>,
    /// `ibufsize`.
    pub ibufsize: u32,
    /// `ibufsizepad`.
    pub ibufsizepad: u32,
    /// `obufsize`.
    pub obufsize: u32,
    /// `opkthdrlen`.
    pub opkthdrlen: u32,
    /// `info`: attach message.
    pub info: Option<&'static str>,
    /// `device`.
    pub device: Option<&'static UsbdDevice>,
    /// `iface`.
    pub iface: Option<&'static UsbdInterface>,
    /// `methods`.
    pub methods: &'static UcomMethods,
    /// `arg`: what the methods get as their `sc`.
    pub arg: *mut c_void,
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modem_bits_do_not_overlap_the_line_bits() {
        assert_eq!(UMCR_DTR | UMCR_RTS, 0x03);
        assert_eq!(
            ULSR_RCV_MASK,
            ULSR_BI | ULSR_FE | ULSR_PE | ULSR_OE | ULSR_RXRDY
        );
        assert_eq!(UMSR_DCD | UMSR_RI | UMSR_DSR | UMSR_CTS, 0xf0);
        assert_eq!(UCOM_UNK_PORTNO, -1);
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn constants_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/usb/ucomvar.h");
        let ours = crate::reftest::assert_defines!(defs;
            UCOMBUSCF_PORTNO_DEFAULT, UCOM_SET_DTR, UCOM_SET_RTS, UCOM_SET_BREAK,
            UMCR_RTS, UMCR_DTR, ULSR_RCV_FIFO, ULSR_TSRE, ULSR_TXRDY, ULSR_BI, ULSR_FE,
            ULSR_PE, ULSR_OE, ULSR_RXRDY, ULSR_RCV_MASK, UMSR_DCD, UMSR_RI, UMSR_DSR,
            UMSR_CTS, UMSR_DDCD, UMSR_TERI, UMSR_DDSR, UMSR_DCTS,
        );
        crate::reftest::assert_complete(&defs, "UMCR_", &ours);
        crate::reftest::assert_complete(&defs, "ULSR_", &ours);
        crate::reftest::assert_complete(&defs, "UMSR_", &ours);
    }
}
/* </TESTS> */
