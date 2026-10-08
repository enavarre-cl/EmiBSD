/*	$OpenBSD: ttydefaults.h,v 1.7 2019/03/12 11:01:25 nicm Exp $	*/
/*	$NetBSD: ttydefaults.h,v 1.8 1996/04/09 20:55:45 cgd Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (c) 1982, 1986, 1993
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
 *	@(#)ttydefaults.h	8.4 (Berkeley) 1/21/94
 */
/* </LICENSES> */

/* <CODE> */
//! System wide defaults for terminal state: `<sys/ttydefaults.h>`.
//!
//! Upstream: sys/sys/ttydefaults.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - `ttydefchars[]` (behind `TTYDEFCHARS` in C) is always defined, as [`TTYDEFCHARS`].

use crate::sys::termios::{
    _POSIX_VDISABLE, B9600, BRKINT, CREAD, CS8, Cc, ECHO, ECHOCTL, ECHOE, ECHOKE, HUPCL, ICANON,
    ICRNL, IEXTEN, IMAXBEL, ISIG, IXANY, IXON, NCCS, ONLCR, OPOST, Speed, Tcflag,
};

// Defaults on "first" open.

/// Default input flags.
pub const TTYDEF_IFLAG: Tcflag = BRKINT | ICRNL | IMAXBEL | IXON | IXANY;
/// Default output flags.
pub const TTYDEF_OFLAG: Tcflag = OPOST | ONLCR;
/// Default local flags.
pub const TTYDEF_LFLAG: Tcflag = ECHO | ICANON | ISIG | IEXTEN | ECHOE | ECHOKE | ECHOCTL;
/// Default control flags.
pub const TTYDEF_CFLAG: Tcflag = CREAD | CS8 | HUPCL;
/// Default speed.
pub const TTYDEF_SPEED: Speed = B9600;

// Control Character Defaults

/// `CTRL(x)`: the control character of `x`.
pub const fn ctrl(x: u8) -> Cc {
    x & 0o37
}

/// End of file.
pub const CEOF: Cc = ctrl(b'd');
/// End of line (avoids `_POSIX_VDISABLE`).
pub const CEOL: Cc = 0o377;
/// Erase.
pub const CERASE: Cc = 0o177;
/// Interrupt.
pub const CINTR: Cc = ctrl(b'c');
/// Status (avoids `_POSIX_VDISABLE`).
pub const CSTATUS: Cc = 0o377;
/// Kill.
pub const CKILL: Cc = ctrl(b'u');
/// Minimum characters for a non-canonical read.
pub const CMIN: Cc = 1;
/// Quit (FS, `^\`).
pub const CQUIT: Cc = 0o34;
/// Suspend.
pub const CSUSP: Cc = ctrl(b'z');
/// Timeout for a non-canonical read.
pub const CTIME: Cc = 0;
/// Delayed suspend.
pub const CDSUSP: Cc = ctrl(b'y');
/// Start output.
pub const CSTART: Cc = ctrl(b'q');
/// Stop output.
pub const CSTOP: Cc = ctrl(b's');
/// Literal next.
pub const CLNEXT: Cc = ctrl(b'v');
/// Discard output.
pub const CDISCARD: Cc = ctrl(b'o');
/// Word erase.
pub const CWERASE: Cc = ctrl(b'w');
/// Reprint line.
pub const CREPRINT: Cc = ctrl(b'r');
/// End of transmission.
pub const CEOT: Cc = CEOF;
// compat
/// Break (compat).
pub const CBRK: Cc = CEOL;
/// Reprint (compat).
pub const CRPRNT: Cc = CREPRINT;
/// Flush (compat).
pub const CFLUSH: Cc = CDISCARD;

/// `ttydefchars[NCCS]`: the default control characters, indexed by `V*`.
pub const TTYDEFCHARS: [Cc; NCCS] = [
    CEOF,
    CEOL,
    CEOL,
    CERASE,
    CWERASE,
    CKILL,
    CREPRINT,
    _POSIX_VDISABLE,
    CINTR,
    CQUIT,
    CSUSP,
    CDSUSP,
    CSTART,
    CSTOP,
    CLNEXT,
    CDISCARD,
    CMIN,
    CTIME,
    CSTATUS,
    _POSIX_VDISABLE,
];
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;
    use crate::sys::termios::{VEOF, VEOL, VEOL2, VERASE};

    #[test]
    fn control_characters() {
        assert_eq!(ctrl(b'd'), 4);
        assert_eq!(CEOF, 4);
        assert_eq!(CINTR, 3);
        assert_eq!(CQUIT, 0o34);
        assert_eq!(TTYDEFCHARS[VEOF as usize], CEOF);
        assert_eq!(TTYDEFCHARS[VEOL as usize], CEOL);
        assert_eq!(TTYDEFCHARS[VEOL2 as usize], CEOL);
        assert_eq!(TTYDEFCHARS[VERASE as usize], CERASE);
        assert_eq!(TTYDEFCHARS[7], 0o377);
        assert_eq!(TTYDEFCHARS[19], 0o377);
        assert_eq!(TTYDEF_CFLAG, 0x0800 | 0x0300 | 0x4000);
        assert_eq!(TTYDEF_SPEED, 9600);
    }
}
/* </TESTS> */
