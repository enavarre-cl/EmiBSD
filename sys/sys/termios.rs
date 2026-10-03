/*	$OpenBSD: termios.h,v 1.14 2022/12/30 23:41:45 millert Exp $	*/
/*	$NetBSD: termios.h,v 1.14 1996/04/09 20:55:41 cgd Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1988, 1989, 1993, 1994
 *	The Regents of the University of California.  All rights reserved.
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
 *	@(#)termios.h	8.3 (Berkeley) 3/28/94
 */
/* </LICENSES> */

//! Terminal attributes: `<sys/termios.h>`, as the kernel sees it.
//!
//! Upstream: sys/sys/termios.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - Every flag is visible (`__BSD_VISIBLE`); `CCEQ` is [`cceq`].
//! - The `tcsetattr` family of prototypes is userland and not here; the `TCIFLUSH`..`TCION`
//!   constants of the same `#ifndef _KERNEL` block are, so the header is complete for the
//!   userland that is built against it. `<sys/ttycom.h>` (the tty ioctls) and
//!   `<sys/ttydefaults.h>` are modules of their own.
//! - [`Termios`] is an [`AbiPod`]: `TIOCGETA`/`TIOCSETA` copy it whole.

use crate::machine::copy::AbiPod;

/// `tcflag_t`: a set of terminal flags.
pub type Tcflag = u32;
/// `cc_t`: a control character.
pub type Cc = u8;
/// `speed_t`: a line speed in bits per second.
pub type Speed = u32;

// Special Control Characters: index into c_cc[]; the comment says which flag enables each.

/// End of file (ICANON).
pub const VEOF: u32 = 0;
/// End of line (ICANON).
pub const VEOL: u32 = 1;
/// Second end of line (ICANON).
pub const VEOL2: u32 = 2;
/// Erase (ICANON).
pub const VERASE: u32 = 3;
/// Word erase (ICANON).
pub const VWERASE: u32 = 4;
/// Kill (ICANON).
pub const VKILL: u32 = 5;
/// Reprint (ICANON).
pub const VREPRINT: u32 = 6;
/// Interrupt (ISIG).
pub const VINTR: u32 = 8;
/// Quit (ISIG).
pub const VQUIT: u32 = 9;
/// Suspend (ISIG).
pub const VSUSP: u32 = 10;
/// Delayed suspend (ISIG).
pub const VDSUSP: u32 = 11;
/// Start (IXON, IXOFF).
pub const VSTART: u32 = 12;
/// Stop (IXON, IXOFF).
pub const VSTOP: u32 = 13;
/// Literal next (IEXTEN).
pub const VLNEXT: u32 = 14;
/// Discard (IEXTEN).
pub const VDISCARD: u32 = 15;
/// Minimum characters (!ICANON).
pub const VMIN: u32 = 16;
/// Timeout (!ICANON).
pub const VTIME: u32 = 17;
/// Status (ICANON).
pub const VSTATUS: u32 = 18;
/// Number of control characters.
pub const NCCS: usize = 20;

/// The control character value that disables its function.
pub const _POSIX_VDISABLE: Cc = 0o377;

// Input flags - software input processing

/// Ignore BREAK condition.
pub const IGNBRK: Tcflag = 0x0000_0001;
/// Map BREAK to SIGINT.
pub const BRKINT: Tcflag = 0x0000_0002;
/// Ignore (discard) parity errors.
pub const IGNPAR: Tcflag = 0x0000_0004;
/// Mark parity and framing errors.
pub const PARMRK: Tcflag = 0x0000_0008;
/// Enable checking of parity errors.
pub const INPCK: Tcflag = 0x0000_0010;
/// Strip 8th bit off chars.
pub const ISTRIP: Tcflag = 0x0000_0020;
/// Map NL into CR.
pub const INLCR: Tcflag = 0x0000_0040;
/// Ignore CR.
pub const IGNCR: Tcflag = 0x0000_0080;
/// Map CR to NL (ala CRMOD).
pub const ICRNL: Tcflag = 0x0000_0100;
/// Enable output flow control.
pub const IXON: Tcflag = 0x0000_0200;
/// Enable input flow control.
pub const IXOFF: Tcflag = 0x0000_0400;
/// Any char will restart after stop.
pub const IXANY: Tcflag = 0x0000_0800;
/// Translate upper to lower case.
pub const IUCLC: Tcflag = 0x0000_1000;
/// Ring bell on input queue full.
pub const IMAXBEL: Tcflag = 0x0000_2000;

// Output flags - software output processing

/// Enable following output processing.
pub const OPOST: Tcflag = 0x0000_0001;
/// Map NL to CR-NL (ala CRMOD).
pub const ONLCR: Tcflag = 0x0000_0002;
/// Horizontal tab delay mask.
pub const TABDLY: Tcflag = 0x0000_0004;
/// No tab delay or expansion.
pub const TAB0: Tcflag = 0x0000_0000;
/// Expand tabs to spaces.
pub const TAB3: Tcflag = 0x0000_0004;
/// BSD name for TAB3.
pub const OXTABS: Tcflag = TAB3;
/// Discard EOT's (^D) on output.
pub const ONOEOT: Tcflag = 0x0000_0008;
/// Map CR to NL.
pub const OCRNL: Tcflag = 0x0000_0010;
/// Translate lower case to upper case.
pub const OLCUC: Tcflag = 0x0000_0020;
/// No CR output at column 0.
pub const ONOCR: Tcflag = 0x0000_0040;
/// NL performs the CR function.
pub const ONLRET: Tcflag = 0x0000_0080;

// Control flags - hardware control of terminal

/// Ignore control flags.
pub const CIGNORE: Tcflag = 0x0000_0001;
/// Character size mask.
pub const CSIZE: Tcflag = 0x0000_0300;
/// 5 bits (pseudo).
pub const CS5: Tcflag = 0x0000_0000;
/// 6 bits.
pub const CS6: Tcflag = 0x0000_0100;
/// 7 bits.
pub const CS7: Tcflag = 0x0000_0200;
/// 8 bits.
pub const CS8: Tcflag = 0x0000_0300;
/// Send 2 stop bits.
pub const CSTOPB: Tcflag = 0x0000_0400;
/// Enable receiver.
pub const CREAD: Tcflag = 0x0000_0800;
/// Parity enable.
pub const PARENB: Tcflag = 0x0000_1000;
/// Odd parity, else even.
pub const PARODD: Tcflag = 0x0000_2000;
/// Hang up on last close.
pub const HUPCL: Tcflag = 0x0000_4000;
/// Ignore modem status lines.
pub const CLOCAL: Tcflag = 0x0000_8000;
/// RTS/CTS full-duplex flow control.
pub const CRTSCTS: Tcflag = 0x0001_0000;
/// Compat name for CRTSCTS.
pub const CRTS_IFLOW: Tcflag = CRTSCTS;
/// Compat name for CRTSCTS.
pub const CCTS_OFLOW: Tcflag = CRTSCTS;
/// DTR/DCD hardware flow control.
pub const MDMBUF: Tcflag = 0x0010_0000;
/// All types of hw flow control.
pub const CHWFLOW: Tcflag = MDMBUF | CRTSCTS;

// "Local" flags - dumping ground for other state. Warning: some flags in this structure begin
// with the letter "I" and look like they belong in the input flag.

/// Visual erase for line kill.
pub const ECHOKE: Tcflag = 0x0000_0001;
/// Visually erase chars.
pub const ECHOE: Tcflag = 0x0000_0002;
/// Echo NL after line kill.
pub const ECHOK: Tcflag = 0x0000_0004;
/// Enable echoing.
pub const ECHO: Tcflag = 0x0000_0008;
/// Echo NL even if ECHO is off.
pub const ECHONL: Tcflag = 0x0000_0010;
/// Visual erase mode for hardcopy.
pub const ECHOPRT: Tcflag = 0x0000_0020;
/// Echo control chars as ^(Char).
pub const ECHOCTL: Tcflag = 0x0000_0040;
/// Enable signals INTR, QUIT, [D]SUSP.
pub const ISIG: Tcflag = 0x0000_0080;
/// Canonicalize input lines.
pub const ICANON: Tcflag = 0x0000_0100;
/// Use alternate WERASE algorithm.
pub const ALTWERASE: Tcflag = 0x0000_0200;
/// Enable DISCARD and LNEXT.
pub const IEXTEN: Tcflag = 0x0000_0400;
/// External processing.
pub const EXTPROC: Tcflag = 0x0000_0800;
/// Stop background jobs from output.
pub const TOSTOP: Tcflag = 0x0040_0000;
/// Output being flushed (state).
pub const FLUSHO: Tcflag = 0x0080_0000;
/// Canonical upper/lower case.
pub const XCASE: Tcflag = 0x0100_0000;
/// No kernel output from VSTATUS.
pub const NOKERNINFO: Tcflag = 0x0200_0000;
/// XXX retype pending input (state).
pub const PENDIN: Tcflag = 0x2000_0000;
/// Don't flush after interrupt.
pub const NOFLSH: Tcflag = 0x8000_0000;

/// `struct termios`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Termios {
    /// Input flags.
    pub c_iflag: Tcflag,
    /// Output flags.
    pub c_oflag: Tcflag,
    /// Control flags.
    pub c_cflag: Tcflag,
    /// Local flags.
    pub c_lflag: Tcflag,
    /// Control chars.
    pub c_cc: [Cc; NCCS],
    /// Input speed.
    pub c_ispeed: i32,
    /// Output speed.
    pub c_ospeed: i32,
}

impl Termios {
    /// An all-zero termios, what a freshly allocated `struct tty` holds.
    pub const fn zeroed() -> Self {
        Self {
            c_iflag: 0,
            c_oflag: 0,
            c_cflag: 0,
            c_lflag: 0,
            c_cc: [0; NCCS],
            c_ispeed: 0,
            c_ospeed: 0,
        }
    }
}

// SAFETY: `repr(C)`: four `u32`s, twenty bytes and two `i32`s, 44 bytes without padding; every
// bit pattern is a valid value.
unsafe impl AbiPod for Termios {}

// Commands passed to tcsetattr() for setting the termios structure.

/// Make change immediate.
pub const TCSANOW: i32 = 0;
/// Drain output, then change.
pub const TCSADRAIN: i32 = 1;
/// Drain output, flush input.
pub const TCSAFLUSH: i32 = 2;
/// Flag: don't alter h.w. state.
pub const TCSASOFT: i32 = 0x10;

// Standard speeds

/// 0 baud (hang up).
pub const B0: Speed = 0;
/// 50 baud.
pub const B50: Speed = 50;
/// 75 baud.
pub const B75: Speed = 75;
/// 110 baud.
pub const B110: Speed = 110;
/// 134 baud.
pub const B134: Speed = 134;
/// 150 baud.
pub const B150: Speed = 150;
/// 200 baud.
pub const B200: Speed = 200;
/// 300 baud.
pub const B300: Speed = 300;
/// 600 baud.
pub const B600: Speed = 600;
/// 1200 baud.
pub const B1200: Speed = 1200;
/// 1800 baud.
pub const B1800: Speed = 1800;
/// 2400 baud.
pub const B2400: Speed = 2400;
/// 4800 baud.
pub const B4800: Speed = 4800;
/// 9600 baud.
pub const B9600: Speed = 9600;
/// 19200 baud.
pub const B19200: Speed = 19200;
/// 38400 baud.
pub const B38400: Speed = 38400;
/// 7200 baud.
pub const B7200: Speed = 7200;
/// 14400 baud.
pub const B14400: Speed = 14400;
/// 28800 baud.
pub const B28800: Speed = 28800;
/// 57600 baud.
pub const B57600: Speed = 57600;
/// 76800 baud.
pub const B76800: Speed = 76800;
/// 115200 baud.
pub const B115200: Speed = 115200;
/// 230400 baud.
pub const B230400: Speed = 230400;
/// External A clock (19200).
pub const EXTA: Speed = 19200;
/// External B clock (38400).
pub const EXTB: Speed = 38400;

// tcflush(3) queue selectors and tcflow(3) actions (userland, `#ifndef _KERNEL`).

/// Flush the input queue.
pub const TCIFLUSH: i32 = 1;
/// Flush the output queue.
pub const TCOFLUSH: i32 = 2;
/// Flush both queues.
pub const TCIOFLUSH: i32 = 3;
/// Suspend output.
pub const TCOOFF: i32 = 1;
/// Restart output.
pub const TCOON: i32 = 2;
/// Send a STOP character.
pub const TCIOFF: i32 = 3;
/// Send a START character.
pub const TCION: i32 = 4;

/// `CCEQ(val, c)`: whether `c` equals the control character `val`, which must be enabled.
pub const fn cceq(val: Cc, c: Cc) -> bool {
    if c == val {
        val != _POSIX_VDISABLE
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_and_helpers() {
        assert_eq!(core::mem::size_of::<Termios>(), 4 * 4 + NCCS + 2 * 4);
        assert!(cceq(3, 3));
        assert!(!cceq(3, 4));
        assert!(!cceq(_POSIX_VDISABLE, _POSIX_VDISABLE));
        assert_eq!(CS8 & CSIZE, CS8);
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/sys/termios.h");
        let ours: &[(&str, i64)] = &[
            ("VEOF", VEOF as i64),
            ("VEOL", VEOL as i64),
            ("VEOL2", VEOL2 as i64),
            ("VERASE", VERASE as i64),
            ("VWERASE", VWERASE as i64),
            ("VKILL", VKILL as i64),
            ("VREPRINT", VREPRINT as i64),
            ("VINTR", VINTR as i64),
            ("VQUIT", VQUIT as i64),
            ("VSUSP", VSUSP as i64),
            ("VDSUSP", VDSUSP as i64),
            ("VSTART", VSTART as i64),
            ("VSTOP", VSTOP as i64),
            ("VLNEXT", VLNEXT as i64),
            ("VDISCARD", VDISCARD as i64),
            ("VMIN", VMIN as i64),
            ("VTIME", VTIME as i64),
            ("VSTATUS", VSTATUS as i64),
            ("NCCS", NCCS as i64),
            ("IGNBRK", IGNBRK as i64),
            ("BRKINT", BRKINT as i64),
            ("IGNPAR", IGNPAR as i64),
            ("PARMRK", PARMRK as i64),
            ("INPCK", INPCK as i64),
            ("ISTRIP", ISTRIP as i64),
            ("INLCR", INLCR as i64),
            ("IGNCR", IGNCR as i64),
            ("ICRNL", ICRNL as i64),
            ("IXON", IXON as i64),
            ("IXOFF", IXOFF as i64),
            ("IXANY", IXANY as i64),
            ("IUCLC", IUCLC as i64),
            ("IMAXBEL", IMAXBEL as i64),
            ("OPOST", OPOST as i64),
            ("ONLCR", ONLCR as i64),
            ("TABDLY", TABDLY as i64),
            ("TAB0", TAB0 as i64),
            ("TAB3", TAB3 as i64),
            ("ONOEOT", ONOEOT as i64),
            ("OCRNL", OCRNL as i64),
            ("OLCUC", OLCUC as i64),
            ("ONOCR", ONOCR as i64),
            ("ONLRET", ONLRET as i64),
            ("CIGNORE", CIGNORE as i64),
            ("CSIZE", CSIZE as i64),
            ("CS5", CS5 as i64),
            ("CS6", CS6 as i64),
            ("CS7", CS7 as i64),
            ("CS8", CS8 as i64),
            ("CSTOPB", CSTOPB as i64),
            ("CREAD", CREAD as i64),
            ("PARENB", PARENB as i64),
            ("PARODD", PARODD as i64),
            ("HUPCL", HUPCL as i64),
            ("CLOCAL", CLOCAL as i64),
            ("CRTSCTS", CRTSCTS as i64),
            ("MDMBUF", MDMBUF as i64),
            ("ECHOKE", ECHOKE as i64),
            ("ECHOE", ECHOE as i64),
            ("ECHOK", ECHOK as i64),
            ("ECHO", ECHO as i64),
            ("ECHONL", ECHONL as i64),
            ("ECHOPRT", ECHOPRT as i64),
            ("ECHOCTL", ECHOCTL as i64),
            ("ISIG", ISIG as i64),
            ("ICANON", ICANON as i64),
            ("ALTWERASE", ALTWERASE as i64),
            ("IEXTEN", IEXTEN as i64),
            ("EXTPROC", EXTPROC as i64),
            ("TOSTOP", TOSTOP as i64),
            ("FLUSHO", FLUSHO as i64),
            ("XCASE", XCASE as i64),
            ("NOKERNINFO", NOKERNINFO as i64),
            ("PENDIN", PENDIN as i64),
            ("NOFLSH", NOFLSH as i64),
            ("TCSANOW", TCSANOW as i64),
            ("TCSADRAIN", TCSADRAIN as i64),
            ("TCSAFLUSH", TCSAFLUSH as i64),
            ("TCSASOFT", TCSASOFT as i64),
            ("B0", B0 as i64),
            ("B300", B300 as i64),
            ("B9600", B9600 as i64),
            ("B38400", B38400 as i64),
            ("B115200", B115200 as i64),
            ("B230400", B230400 as i64),
            ("EXTA", EXTA as i64),
            ("EXTB", EXTB as i64),
            ("TCIFLUSH", TCIFLUSH as i64),
            ("TCOFLUSH", TCOFLUSH as i64),
            ("TCIOFLUSH", TCIOFLUSH as i64),
            ("TCOOFF", TCOOFF as i64),
            ("TCOON", TCOON as i64),
            ("TCIOFF", TCIOFF as i64),
            ("TCION", TCION as i64),
        ];
        for (name, value) in ours {
            assert_eq!(crate::reftest::int(&defs, name), Some(*value), "{name}");
        }
    }
}
