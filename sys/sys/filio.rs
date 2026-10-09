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
 *	@(#)filio.h	8.1 (Berkeley) 3/28/94
 */
/* </LICENSES> */

/* <CODE> */
//! `<sys/filio.h>`: generic file-descriptor ioctl's.
//!
//! Upstream: sys/sys/filio.h @ 3ce1f3f79392

use crate::sys::ioccom::{_io, _ior, _iow};

/// `FIOCLEX`: set close on exec on fd.
pub const FIOCLEX: u64 = _io(b'f', 1);
/// `FIONCLEX`: remove close on exec.
pub const FIONCLEX: u64 = _io(b'f', 2);
/// `FIONREAD`: get # bytes to read.
pub const FIONREAD: u64 = _ior::<i32>(b'f', 127);
/// `FIONBIO`: set/clear non-blocking i/o.
pub const FIONBIO: u64 = _iow::<i32>(b'f', 126);
/// `FIOASYNC`: set/clear async i/o.
pub const FIOASYNC: u64 = _iow::<i32>(b'f', 125);
/// `FIOSETOWN`: set owner.
pub const FIOSETOWN: u64 = _iow::<i32>(b'f', 124);
/// `FIOGETOWN`: get owner.
pub const FIOGETOWN: u64 = _ior::<i32>(b'f', 123);
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_have_openbsd_values() {
        // The values OpenBSD's ioctl(2) users compile in (amd64 and arm64 alike).
        assert_eq!(FIOCLEX, 0x2000_6601);
        assert_eq!(FIONCLEX, 0x2000_6602);
        assert_eq!(FIONREAD, 0x4004_667f);
        assert_eq!(FIONBIO, 0x8004_667e);
        assert_eq!(FIOASYNC, 0x8004_667d);
        assert_eq!(FIOSETOWN, 0x8004_667c);
        assert_eq!(FIOGETOWN, 0x4004_667b);
    }
}
/* </TESTS> */
