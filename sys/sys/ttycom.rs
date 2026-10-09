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

/*-
 * Copyright (c) 1982, 1986, 1990, 1993, 1994
 *	The Regents of the University of California.  All rights reserved.
 * (c) UNIX System Laboratories, Inc.
 * All or some portions of this file are derived from material licensed
 * to the University of California by American Telephone and Telegraph
 * Co. or Unix System Laboratories, Inc. and are reproduced herein with
 * the permission of UNIX System Laboratories, Inc.
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
 *	@(#)ttycom.h	8.1 (Berkeley) 3/28/94
 */
/* </LICENSES> */

/* <CODE> */
//! `<sys/ttycom.h>`: the tty `ioctl(2)` commands, `struct winsize` and `struct tstamps`, the
//! modem bits and the line discipline numbers.
//!
//! Upstream: sys/sys/ttycom.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - `UIOCCMD(n)` is the `const fn` [`uioccmd`]; the commands are `u64` (`u_long` in C).
//! - The modem, packet and flag bits are `i32`: they travel in the `int` argument of
//!   `TIOCMSET`, `TIOCPKT` and `TIOCSFLAGS`.

use crate::machine::copy::AbiPod;
use crate::sys::ioccom::{_io, _ior, _iow};
use crate::sys::termios::Termios;
use crate::sys::time::Timeval;

/// `struct winsize`: window/terminal size. This information is stored by the kernel in
/// order to provide a consistent interface, but is not used by the kernel.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Winsize {
    /// Rows, in characters.
    pub ws_row: u16,
    /// Columns, in characters.
    pub ws_col: u16,
    /// Horizontal size, pixels.
    pub ws_xpixel: u16,
    /// Vertical size, pixels.
    pub ws_ypixel: u16,
}

// SAFETY: four `u16`s, no padding, every bit pattern valid.
unsafe impl AbiPod for Winsize {}

/// `struct tstamps`: which modem transitions `TIOCGTSTAMP` records.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tstamps {
    /// `TIOCM_CAR` and/or `TIOCM_CTS`.
    pub ts_set: i32,
    /// Likewise, for the transitions that clear them.
    pub ts_clr: i32,
}

// SAFETY: two `i32`s, no padding, every bit pattern valid.
unsafe impl AbiPod for Tstamps {}

/// Line enable.
pub const TIOCM_LE: i32 = 0o001;
/// Data terminal ready.
pub const TIOCM_DTR: i32 = 0o002;
/// Request to send.
pub const TIOCM_RTS: i32 = 0o004;
/// Secondary transmit.
pub const TIOCM_ST: i32 = 0o010;
/// Secondary receive.
pub const TIOCM_SR: i32 = 0o020;
/// Clear to send.
pub const TIOCM_CTS: i32 = 0o040;
/// Carrier detect.
pub const TIOCM_CAR: i32 = 0o100;
/// Carrier detect.
pub const TIOCM_CD: i32 = TIOCM_CAR;
/// Ring.
pub const TIOCM_RNG: i32 = 0o200;
/// Ring.
pub const TIOCM_RI: i32 = TIOCM_RNG;
/// Data set ready.
pub const TIOCM_DSR: i32 = 0o400;

// 8-10 compat

/// Set exclusive use of tty.
pub const TIOCEXCL: u64 = _io(b't', 13);
/// Reset exclusive use of tty.
pub const TIOCNXCL: u64 = _io(b't', 14);
// 15 unused
/// Flush buffers.
pub const TIOCFLUSH: u64 = _iow::<i32>(b't', 16);
// 17-18 compat
/// Get termios struct.
pub const TIOCGETA: u64 = _ior::<Termios>(b't', 19);
/// Set termios struct.
pub const TIOCSETA: u64 = _iow::<Termios>(b't', 20);
/// Drain output, set.
pub const TIOCSETAW: u64 = _iow::<Termios>(b't', 21);
/// Drain output, flush input, set.
pub const TIOCSETAF: u64 = _iow::<Termios>(b't', 22);
/// Get line discipline.
pub const TIOCGETD: u64 = _ior::<i32>(b't', 26);
/// Set line discipline.
pub const TIOCSETD: u64 = _iow::<i32>(b't', 27);
/// Set verified auth.
pub const TIOCSETVERAUTH: u64 = _iow::<i32>(b't', 28);
/// Clear verified auth.
pub const TIOCCLRVERAUTH: u64 = _io(b't', 29);
/// Check verified auth.
pub const TIOCCHKVERAUTH: u64 = _io(b't', 30);
// 127-124 compat
/// Set break bit.
pub const TIOCSBRK: u64 = _io(b't', 123);
/// Clear break bit.
pub const TIOCCBRK: u64 = _io(b't', 122);
/// Set data terminal ready.
pub const TIOCSDTR: u64 = _io(b't', 121);
/// Clear data terminal ready.
pub const TIOCCDTR: u64 = _io(b't', 120);
/// Get pgrp of tty.
pub const TIOCGPGRP: u64 = _ior::<i32>(b't', 119);
/// Set pgrp of tty.
pub const TIOCSPGRP: u64 = _iow::<i32>(b't', 118);
// 117-116 compat
/// Output queue size.
pub const TIOCOUTQ: u64 = _ior::<i32>(b't', 115);
/// Void tty association.
pub const TIOCNOTTY: u64 = _io(b't', 113);
/// pty: set/clear packet mode.
pub const TIOCPKT: u64 = _iow::<i32>(b't', 112);
/// Data packet.
pub const TIOCPKT_DATA: i32 = 0x00;
/// Flush packet.
pub const TIOCPKT_FLUSHREAD: i32 = 0x01;
/// Flush packet.
pub const TIOCPKT_FLUSHWRITE: i32 = 0x02;
/// Stop output.
pub const TIOCPKT_STOP: i32 = 0x04;
/// Start output.
pub const TIOCPKT_START: i32 = 0x08;
/// No more ^S, ^Q.
pub const TIOCPKT_NOSTOP: i32 = 0x10;
/// Now do ^S ^Q.
pub const TIOCPKT_DOSTOP: i32 = 0x20;
/// State change of pty driver.
pub const TIOCPKT_IOCTL: i32 = 0x40;
/// Stop output, like ^S.
pub const TIOCSTOP: u64 = _io(b't', 111);
/// Start output, like ^Q.
pub const TIOCSTART: u64 = _io(b't', 110);
/// Set all modem bits.
pub const TIOCMSET: u64 = _iow::<i32>(b't', 109);
/// Bis modem bits.
pub const TIOCMBIS: u64 = _iow::<i32>(b't', 108);
/// Bic modem bits.
pub const TIOCMBIC: u64 = _iow::<i32>(b't', 107);
/// Get all modem bits.
pub const TIOCMGET: u64 = _ior::<i32>(b't', 106);
/// Remote input editing.
pub const TIOCREMOTE: u64 = _iow::<i32>(b't', 105);
/// Get window size.
pub const TIOCGWINSZ: u64 = _ior::<Winsize>(b't', 104);
/// Set window size.
pub const TIOCSWINSZ: u64 = _iow::<Winsize>(b't', 103);
/// pty: set/clr usr cntl mode.
pub const TIOCUCNTL: u64 = _iow::<i32>(b't', 102);

/// `UIOCCMD(n)`: user control op "n".
pub const fn uioccmd(n: u8) -> u64 {
    _io(b'u', n)
}

/// Set break bit, user control.
pub const TIOCUCNTL_SBRK: u64 = TIOCSBRK & 0xff;
/// Clear break bit, user control.
pub const TIOCUCNTL_CBRK: u64 = TIOCCBRK & 0xff;
/// Generate status message.
pub const TIOCSTAT: u64 = _io(b't', 101);
/// Get sid of tty.
pub const TIOCGSID: u64 = _ior::<i32>(b't', 99);
/// Become virtual console.
pub const TIOCCONS: u64 = _iow::<i32>(b't', 98);
/// Become controlling tty.
pub const TIOCSCTTY: u64 = _io(b't', 97);
/// pty: external processing.
pub const TIOCEXT: u64 = _iow::<i32>(b't', 96);
/// pty: generate signal.
pub const TIOCSIG: u64 = _iow::<i32>(b't', 95);
/// Wait till output drained.
pub const TIOCDRAIN: u64 = _io(b't', 94);
/// Get device flags.
pub const TIOCGFLAGS: u64 = _ior::<i32>(b't', 93);
/// Set device flags.
pub const TIOCSFLAGS: u64 = _iow::<i32>(b't', 92);
/// Ignore hardware carrier.
pub const TIOCFLAG_SOFTCAR: i32 = 0x01;
/// Set clocal on open.
pub const TIOCFLAG_CLOCAL: i32 = 0x02;
/// Set crtscts on open.
pub const TIOCFLAG_CRTSCTS: i32 = 0x04;
/// Set mdmbuf on open.
pub const TIOCFLAG_MDMBUF: i32 = 0x08;
/// Call hardpps on carrier up.
pub const TIOCFLAG_PPS: i32 = 0x10;
/// Get timestamp.
pub const TIOCGTSTAMP: u64 = _ior::<Timeval>(b't', 91);
/// Timestamp reasons.
pub const TIOCSTSTAMP: u64 = _iow::<Tstamps>(b't', 90);

// Backwards compatibility

/// `TIOCMODG`: `TIOCMGET`.
pub const TIOCMODG: u64 = TIOCMGET;
/// `TIOCMODS`: `TIOCMSET`.
pub const TIOCMODS: u64 = TIOCMSET;

/// Termios tty line discipline.
pub const TTYDISC: i32 = 0;
/// Tablet discipline.
pub const TABLDISC: i32 = 3;
/// Serial IP discipline.
pub const SLIPDISC: i32 = 4;
/// ppp discipline.
pub const PPPDISC: i32 = 5;
/// Metricom wireless IP discipline.
pub const STRIPDISC: i32 = 6;
/// NMEA0183 discipline.
pub const NMEADISC: i32 = 7;
/// Meinberg time string discipline.
pub const MSTSDISC: i32 = 8;
/// EndRun time format discipline.
pub const ENDRUNDISC: i32 = 9;
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_encode_as_on_openbsd() {
        // The values OpenBSD's userland compiles in (LP64): direction, size, group, number.
        assert_eq!(TIOCGETA, 0x402c_7413);
        assert_eq!(TIOCSETA, 0x802c_7414);
        assert_eq!(TIOCGWINSZ, 0x4008_7468);
        assert_eq!(TIOCSWINSZ, 0x8008_7467);
        assert_eq!(TIOCSCTTY, 0x2000_7461);
        assert_eq!(TIOCGPGRP, 0x4004_7477);
        assert_eq!(TIOCSPGRP, 0x8004_7476);
        assert_eq!(TIOCGTSTAMP, 0x4010_745b);
        assert_eq!(TIOCUCNTL_SBRK, 123);
        assert_eq!(uioccmd(5), 0x2000_7505);
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/sys/ttycom.h");
        let ours: &[(&str, i32)] = &[
            ("TIOCM_LE", TIOCM_LE),
            ("TIOCM_DTR", TIOCM_DTR),
            ("TIOCM_RTS", TIOCM_RTS),
            ("TIOCM_ST", TIOCM_ST),
            ("TIOCM_SR", TIOCM_SR),
            ("TIOCM_CTS", TIOCM_CTS),
            ("TIOCM_CAR", TIOCM_CAR),
            ("TIOCM_RNG", TIOCM_RNG),
            ("TIOCM_DSR", TIOCM_DSR),
            ("TIOCPKT_DATA", TIOCPKT_DATA),
            ("TIOCPKT_FLUSHREAD", TIOCPKT_FLUSHREAD),
            ("TIOCPKT_FLUSHWRITE", TIOCPKT_FLUSHWRITE),
            ("TIOCPKT_STOP", TIOCPKT_STOP),
            ("TIOCPKT_START", TIOCPKT_START),
            ("TIOCPKT_NOSTOP", TIOCPKT_NOSTOP),
            ("TIOCPKT_DOSTOP", TIOCPKT_DOSTOP),
            ("TIOCPKT_IOCTL", TIOCPKT_IOCTL),
            ("TIOCFLAG_SOFTCAR", TIOCFLAG_SOFTCAR),
            ("TIOCFLAG_CLOCAL", TIOCFLAG_CLOCAL),
            ("TIOCFLAG_CRTSCTS", TIOCFLAG_CRTSCTS),
            ("TIOCFLAG_MDMBUF", TIOCFLAG_MDMBUF),
            ("TIOCFLAG_PPS", TIOCFLAG_PPS),
            ("TTYDISC", TTYDISC),
            ("TABLDISC", TABLDISC),
            ("SLIPDISC", SLIPDISC),
            ("PPPDISC", PPPDISC),
            ("STRIPDISC", STRIPDISC),
            ("NMEADISC", NMEADISC),
            ("MSTSDISC", MSTSDISC),
            ("ENDRUNDISC", ENDRUNDISC),
        ];
        for (name, value) in ours {
            assert_eq!(
                crate::reftest::int(&defs, name),
                Some(i64::from(*value)),
                "{name}"
            );
        }
    }
}
/* </TESTS> */
