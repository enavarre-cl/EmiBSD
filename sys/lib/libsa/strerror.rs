/*	$OpenBSD: strerror.c,v 1.10 2014/11/19 20:28:56 miod Exp $	*/
/*	$NetBSD: strerror.c,v 1.11 1996/10/13 02:29:08 christos Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (c) 1993
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
 */
/* </LICENSES> */

//! `strerror()`: the message of an error number.
//!
//! Upstream: sys/lib/libsa/strerror.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - The C formats an unknown error into a `static char ebuf[64]`; here the result is a
//!   [`StrError`] that prints the same text.

use core::fmt;

use crate::saerrno::Errno;

/// What `strerror()` returns: a fixed message, or `Unknown error: code N`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StrError {
    /// A known error.
    Msg(&'static str),
    /// Any other number.
    Unknown(i32),
}

impl fmt::Display for StrError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Msg(m) => f.write_str(m),
            Self::Unknown(code) => write!(f, "Unknown error: code {code}"),
        }
    }
}

/// `strerror(err)`.
pub fn strerror(err: Errno) -> StrError {
    StrError::Msg(match err {
        Errno::EADAPT => "bad adaptor number",
        Errno::ECTLR => "bad controller number",
        Errno::EUNIT => "bad drive number",
        Errno::EPART => "bad partition",
        Errno::ERDLAB => "can't read disk label",
        Errno::ENXIO => "Device not configured",
        Errno::EPERM => "Operation not permitted",
        Errno::ENOENT => "No such file or directory",
        Errno::ESTALE => "Stale NFS file handle",
        Errno::EFTYPE => "Inappropriate file type or format",
        Errno::ENOEXEC => "Exec format error",
        Errno::EIO => "Input/output error",
        Errno::EINVAL => "Invalid argument",
        Errno(code) => return StrError::Unknown(code),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;

    #[test]
    fn messages() {
        assert_eq!(
            strerror(Errno::ENOENT).to_string(),
            "No such file or directory"
        );
        assert_eq!(strerror(Errno::EPART).to_string(), "bad partition");
        assert_eq!(strerror(Errno(42)).to_string(), "Unknown error: code 42");
    }
}
