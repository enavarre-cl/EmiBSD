/* <CODE> */
//! `<dev/usb/uftdireg.h>`: the FTDI USB single port serial converter's vendor requests and
//! data format.
//!
//! Upstream: sys/dev/usb/uftdireg.h @ 3ce1f3f79392
//!
//! Definitions for the FTDI USB Single Port Serial Converter, known as FTDI_SIO (Serial
//! Input/Output application of the chipset). The device is based on the FTDI FT8U100AX chip.
//! It has a DB25 on one side, USB on the other. Thanks to FTDI (<http://www.ftdi.co.uk>) for
//! providing details of the protocol required to talk to the device and ongoing assistance
//! during development. Bill Ryder (bryder@sgi.com) of Silicon Graphics, Inc. is the original
//! author of the header, modified by Lennart Augustsson. The C header carries no licence
//! text.
//!
//! All requests are vendor requests to the device (`UT_WRITE_VENDOR_DEVICE`, and
//! `UT_READ_VENDOR_DEVICE` for `FTDI_SIO_GET_STATUS`) with the port in `wIndex`:
//! - `FTDI_SIO_RESET`: `wValue` 0 resets the port (flow control off, DTR and RTS cleared,
//!   buffers purged; baud rate and data format kept), 1 purges the receive buffer, 2 the
//!   transmit buffer.
//! - `FTDI_SIO_SET_BAUD_RATE`: `wValue` is the divisor (the high bits of the divisor go in the
//!   high byte of `wIndex`; for the old SIO chip, the `ftdi_sio_b*` index of the rate).
//! - `FTDI_SIO_SET_DATA`: `wValue` bits 0 to 7 are the data bits, 8 to 10 the parity (none,
//!   odd, even, mark, space), 11 to 13 the stop bits (1, 1.5, 2), 14 sets a break.
//! - `FTDI_SIO_MODEM_CTRL`: `wValue` bit 0 is DTR, bit 1 RTS, bits 8 and 9 say which of the
//!   two the request is about (DTR and RTS take a request each).
//! - `FTDI_SIO_SET_FLOW_CTRL`: the high byte of `wIndex` is the protocol (RTS/CTS, DTR/DSR,
//!   XON/XOFF), the low byte the port; for XON/XOFF `wValue` has XOFF in its high byte and XON
//!   in its low one.
//! - `FTDI_SIO_GET_STATUS`: the modem status (CTS, DSR, RI and RLSD in bits 4 to 7).
//!
//! The bulk-in endpoint's packets begin with two status bytes (the modem status, whose upper
//! four bits are a 16550's MSR, and the line status, a 16550's LSR); the SIO chip's bulk-out
//! packets begin with a tag byte holding the length and the port.
//!
//! ## Deviations
//! - The anonymous baud rate `enum` (`ftdi_sio_b300` ... `ftdi_sio_b115200`) is constants of
//!   the same names. The function-like macros are `const fn`s of the same names.
//! - `FTDI_LSR_MASK` is `~0x60`, an `int`; it is only ever ANDed with a `u_char`, so it is
//!   the byte `0x9f` here.

/* Vendor Request Interface */
/// `FTDI_SIO_RESET`: Reset the port.
pub const FTDI_SIO_RESET: u8 = 0;
/// `FTDI_SIO_MODEM_CTRL`: Set the modem control register.
pub const FTDI_SIO_MODEM_CTRL: u8 = 1;
/// `FTDI_SIO_SET_FLOW_CTRL`: Set flow control register.
pub const FTDI_SIO_SET_FLOW_CTRL: u8 = 2;
/// `FTDI_SIO_SET_BAUD_RATE`: Set baud rate.
pub const FTDI_SIO_SET_BAUD_RATE: u8 = 3;
/// `FTDI_SIO_SET_DATA`: Set the data characteristics of the port.
pub const FTDI_SIO_SET_DATA: u8 = 4;
/// `FTDI_SIO_GET_STATUS`: Retrieve current value of status reg.
pub const FTDI_SIO_GET_STATUS: u8 = 5;
/// `FTDI_SIO_SET_EVENT_CHAR`: Set the event character.
pub const FTDI_SIO_SET_EVENT_CHAR: u8 = 6;
/// `FTDI_SIO_SET_ERROR_CHAR`: Set the error character.
pub const FTDI_SIO_SET_ERROR_CHAR: u8 = 7;

/* Port Identifier Table */
/// `FTDI_PIT_DEFAULT`: SIOA.
pub const FTDI_PIT_DEFAULT: i32 = 0;
/// `FTDI_PIT_SIOA`: SIOA.
pub const FTDI_PIT_SIOA: i32 = 1;
/// `FTDI_PIT_SIOB`: SIOB.
pub const FTDI_PIT_SIOB: i32 = 2;
/// `FTDI_PIT_PARALLEL`: Parallel.
pub const FTDI_PIT_PARALLEL: i32 = 3;

/// `enum uftdi_type`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(non_camel_case_types)] // the C names
pub enum UftdiType {
    /// `UFTDI_TYPE_SIO`.
    #[default]
    UFTDI_TYPE_SIO,
    /// `UFTDI_TYPE_8U232AM`.
    UFTDI_TYPE_8U232AM,
    /// `UFTDI_TYPE_2232H`.
    UFTDI_TYPE_2232H,
    /// `UFTDI_TYPE_232R`.
    UFTDI_TYPE_232R,
}

/* FTDI_SIO_RESET */
/// `FTDI_SIO_RESET_SIO`.
pub const FTDI_SIO_RESET_SIO: u16 = 0;
/// `FTDI_SIO_RESET_PURGE_RX`.
pub const FTDI_SIO_RESET_PURGE_RX: u16 = 1;
/// `FTDI_SIO_RESET_PURGE_TX`.
pub const FTDI_SIO_RESET_PURGE_TX: u16 = 2;

/* FTDI_SIO_SET_BAUDRATE */
/// `ftdi_sio_b300`.
#[allow(non_upper_case_globals)] // the C name
pub const ftdi_sio_b300: i32 = 0;
/// `ftdi_sio_b600`.
#[allow(non_upper_case_globals)] // the C name
pub const ftdi_sio_b600: i32 = 1;
/// `ftdi_sio_b1200`.
#[allow(non_upper_case_globals)] // the C name
pub const ftdi_sio_b1200: i32 = 2;
/// `ftdi_sio_b2400`.
#[allow(non_upper_case_globals)] // the C name
pub const ftdi_sio_b2400: i32 = 3;
/// `ftdi_sio_b4800`.
#[allow(non_upper_case_globals)] // the C name
pub const ftdi_sio_b4800: i32 = 4;
/// `ftdi_sio_b9600`.
#[allow(non_upper_case_globals)] // the C name
pub const ftdi_sio_b9600: i32 = 5;
/// `ftdi_sio_b19200`.
#[allow(non_upper_case_globals)] // the C name
pub const ftdi_sio_b19200: i32 = 6;
/// `ftdi_sio_b38400`.
#[allow(non_upper_case_globals)] // the C name
pub const ftdi_sio_b38400: i32 = 7;
/// `ftdi_sio_b57600`.
#[allow(non_upper_case_globals)] // the C name
pub const ftdi_sio_b57600: i32 = 8;
/// `ftdi_sio_b115200`.
#[allow(non_upper_case_globals)] // the C name
pub const ftdi_sio_b115200: i32 = 9;

/// `FTDI_8U232AM_FREQ`: (48MHz / 16).
pub const FTDI_8U232AM_FREQ: u32 = 3_000_000;
/// `FTDI_2232H_FREQ`: (120MHz / 10).
pub const FTDI_2232H_FREQ: u32 = 12_000_000;

/* Bounds for normal divisors as 4-bit fixed precision ints. */
/// `FTDI_8U232AM_MIN_DIV`.
pub const FTDI_8U232AM_MIN_DIV: u32 = 0x20;
/// `FTDI_8U232AM_MAX_DIV`.
pub const FTDI_8U232AM_MAX_DIV: u32 = 0x3fff8;

/* FTDI_SIO_SET_DATA */
/// `FTDI_SIO_SET_DATA_BITS(n)`.
#[allow(non_snake_case)] // the C macro's name
pub const fn FTDI_SIO_SET_DATA_BITS(n: u16) -> u16 {
    n
}
/// `FTDI_SIO_SET_DATA_PARITY_NONE`.
pub const FTDI_SIO_SET_DATA_PARITY_NONE: u16 = 0x0 << 8;
/// `FTDI_SIO_SET_DATA_PARITY_ODD`.
pub const FTDI_SIO_SET_DATA_PARITY_ODD: u16 = 0x1 << 8;
/// `FTDI_SIO_SET_DATA_PARITY_EVEN`.
pub const FTDI_SIO_SET_DATA_PARITY_EVEN: u16 = 0x2 << 8;
/// `FTDI_SIO_SET_DATA_PARITY_MARK`.
pub const FTDI_SIO_SET_DATA_PARITY_MARK: u16 = 0x3 << 8;
/// `FTDI_SIO_SET_DATA_PARITY_SPACE`.
pub const FTDI_SIO_SET_DATA_PARITY_SPACE: u16 = 0x4 << 8;
/// `FTDI_SIO_SET_DATA_STOP_BITS_1`.
pub const FTDI_SIO_SET_DATA_STOP_BITS_1: u16 = 0x0 << 11;
/// `FTDI_SIO_SET_DATA_STOP_BITS_15`.
pub const FTDI_SIO_SET_DATA_STOP_BITS_15: u16 = 0x1 << 11;
/// `FTDI_SIO_SET_DATA_STOP_BITS_2`.
pub const FTDI_SIO_SET_DATA_STOP_BITS_2: u16 = 0x2 << 11;
/// `FTDI_SIO_SET_BREAK`.
pub const FTDI_SIO_SET_BREAK: u16 = 0x1 << 14;

/* FTDI_SIO_MODEM_CTRL */
/// `FTDI_SIO_SET_DTR_MASK`.
pub const FTDI_SIO_SET_DTR_MASK: u16 = 0x1;
/// `FTDI_SIO_SET_DTR_HIGH`.
pub const FTDI_SIO_SET_DTR_HIGH: u16 = 1 | (FTDI_SIO_SET_DTR_MASK << 8);
/// `FTDI_SIO_SET_DTR_LOW`.
pub const FTDI_SIO_SET_DTR_LOW: u16 = FTDI_SIO_SET_DTR_MASK << 8;
/// `FTDI_SIO_SET_RTS_MASK`.
pub const FTDI_SIO_SET_RTS_MASK: u16 = 0x2;
/// `FTDI_SIO_SET_RTS_HIGH`.
pub const FTDI_SIO_SET_RTS_HIGH: u16 = 2 | (FTDI_SIO_SET_RTS_MASK << 8);
/// `FTDI_SIO_SET_RTS_LOW`.
pub const FTDI_SIO_SET_RTS_LOW: u16 = FTDI_SIO_SET_RTS_MASK << 8;

/* FTDI_SIO_SET_FLOW_CTRL */
/// `FTDI_SIO_DISABLE_FLOW_CTRL`.
pub const FTDI_SIO_DISABLE_FLOW_CTRL: u8 = 0x0;
/// `FTDI_SIO_RTS_CTS_HS`.
pub const FTDI_SIO_RTS_CTS_HS: u8 = 0x1;
/// `FTDI_SIO_DTR_DSR_HS`.
pub const FTDI_SIO_DTR_DSR_HS: u8 = 0x2;
/// `FTDI_SIO_XON_XOFF_HS`.
pub const FTDI_SIO_XON_XOFF_HS: u8 = 0x4;

/// `FTDI_SIO_CTS_MASK`.
pub const FTDI_SIO_CTS_MASK: u8 = 0x10;
/// `FTDI_SIO_DSR_MASK`.
pub const FTDI_SIO_DSR_MASK: u8 = 0x20;
/// `FTDI_SIO_RI_MASK`.
pub const FTDI_SIO_RI_MASK: u8 = 0x40;
/// `FTDI_SIO_RLSD_MASK`.
pub const FTDI_SIO_RLSD_MASK: u8 = 0x80;

/// `FTDI_PORT_MASK`.
pub const FTDI_PORT_MASK: u8 = 0x0f;
/// `FTDI_MSR_MASK`.
pub const FTDI_MSR_MASK: u8 = 0xf0;
/// `FTDI_GET_MSR(p)`: the modem status byte of an IN packet.
#[allow(non_snake_case)] // the C macro's name
pub fn FTDI_GET_MSR(p: &[u8]) -> u8 {
    p.first().copied().unwrap_or(0) & FTDI_MSR_MASK
}
/// `FTDI_GET_LSR(p)`: the line status byte of an IN packet.
#[allow(non_snake_case)] // the C macro's name
pub fn FTDI_GET_LSR(p: &[u8]) -> u8 {
    p.get(1).copied().unwrap_or(0)
}
/// `FTDI_LSR_MASK`: interesting bits.
pub const FTDI_LSR_MASK: u8 = !0x60;
/// `FTDI_OUT_TAG(len, port)`: the first byte of an OUT packet.
#[allow(non_snake_case)] // the C macro's name
pub const fn FTDI_OUT_TAG(len: u32, port: i32) -> u8 {
    ((len << 2) | port as u32) as u8
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modem_control_values() {
        assert_eq!(FTDI_SIO_SET_DTR_HIGH, 0x0101);
        assert_eq!(FTDI_SIO_SET_DTR_LOW, 0x0100);
        assert_eq!(FTDI_SIO_SET_RTS_HIGH, 0x0202);
        assert_eq!(FTDI_SIO_SET_RTS_LOW, 0x0200);
    }

    #[test]
    fn data_characteristics() {
        let data = FTDI_SIO_SET_DATA_STOP_BITS_2
            | FTDI_SIO_SET_DATA_PARITY_EVEN
            | FTDI_SIO_SET_DATA_BITS(8);
        assert_eq!(data, (2 << 11) | (2 << 8) | 8);
        assert_eq!(FTDI_SIO_SET_BREAK, 0x4000);
    }

    #[test]
    fn in_packet_status_and_out_tag() {
        let p = [0xb1, 0x60, b'x'];
        assert_eq!(FTDI_GET_MSR(&p), 0xb0);
        assert_eq!(FTDI_GET_LSR(&p), 0x60);
        assert_eq!(FTDI_LSR_MASK & 0x60, 0);
        assert_eq!(FTDI_OUT_TAG(5, 1), (5 << 2) | 1);
        assert_eq!(FTDI_GET_MSR(&[]), 0);
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn constants_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/usb/uftdireg.h");
        let ours = crate::reftest::assert_defines!(defs;
            FTDI_SIO_RESET, FTDI_SIO_MODEM_CTRL, FTDI_SIO_SET_FLOW_CTRL,
            FTDI_SIO_SET_BAUD_RATE, FTDI_SIO_SET_DATA, FTDI_SIO_GET_STATUS,
            FTDI_SIO_SET_EVENT_CHAR, FTDI_SIO_SET_ERROR_CHAR, FTDI_PIT_DEFAULT, FTDI_PIT_SIOA,
            FTDI_PIT_SIOB, FTDI_PIT_PARALLEL, FTDI_SIO_RESET_SIO, FTDI_SIO_RESET_PURGE_RX,
            FTDI_SIO_RESET_PURGE_TX, FTDI_8U232AM_FREQ, FTDI_2232H_FREQ, FTDI_8U232AM_MIN_DIV,
            FTDI_8U232AM_MAX_DIV, FTDI_SIO_SET_DTR_MASK, FTDI_SIO_SET_RTS_MASK,
            FTDI_SIO_DISABLE_FLOW_CTRL, FTDI_SIO_RTS_CTS_HS, FTDI_SIO_DTR_DSR_HS,
            FTDI_SIO_XON_XOFF_HS, FTDI_SIO_CTS_MASK, FTDI_SIO_DSR_MASK, FTDI_SIO_RI_MASK,
            FTDI_SIO_RLSD_MASK, FTDI_PORT_MASK, FTDI_MSR_MASK,
        );
        crate::reftest::assert_complete(&defs, "FTDI_SIO_RESET", &ours);
    }
}
/* </TESTS> */
