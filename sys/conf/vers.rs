/*	$OpenBSD: newvers.sh,v 1.219 2026/09/30 20:44:06 deraadt Exp $	*/
/*	$NetBSD: newvers.sh,v 1.17.2.1 1995/10/12 05:17:11 jtc Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1984, 1986, 1990, 1993
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
 *	@(#)newvers.sh	8.1 (Berkeley) 4/20/94
 */
/* </LICENSES> */

//! The system's identity: what `conf/newvers.sh` writes into the generated `vers.c` of every
//! OpenBSD kernel build (`ostype`, `osrelease`, `osversion`, `sccs`, `version`), and what
//! `kern.ostype`, `kern.osrelease`, `kern.osversion`, `kern.version` and the boot banner
//! report.
//!
//! Upstream: sys/conf/newvers.sh @ 3ce1f3f79392 (replaced: see below)
//!
//! The system is **EmiBSD**, release **7.8** (the user's decision, 2026-10-03: the release
//! number tracks the OpenBSD release the reference pin follows). The strings keep
//! `newvers.sh`'s layout:
//!
//! ```text
//! version   = "EmiBSD 7.8 (GENERIC) #<n>: <date>\n    <user>@<host>:<dir>\n"
//! osversion = "GENERIC#<n>"
//! ```
//!
//! ## Deviations
//! - `newvers.sh` is replaced by `sys/build.rs`, which hands the build's facts to this file
//!   as compile-time environment variables (`EMIBSD_VERS_*`). Nothing is read from a clock or
//!   a counter file, so a build is reproducible: `<n>` is `$EMIBSD_BUILD` (the justfile passes
//!   the number of commits, `git rev-list --count HEAD`; 0 without it), `<date>` is
//!   `$SOURCE_DATE_EPOCH` in `date(1)`'s format in UTC (the justfile passes the last commit's
//!   time; the epoch without it), `<user>` is `$USER`, `<host>` is `$EMIBSD_BUILD_HOST` (the
//!   justfile passes `hostname -s`; `localhost` without it) and `<dir>` is the `sys/`
//!   directory that was compiled, where OpenBSD prints its compile directory.
//! - The kernel configuration name is always `GENERIC`: `config(8)` is not ported and there is
//!   one configuration per architecture.
//! - `STATUS` is the release one (`""`), not `-current`: the system reports itself as the
//!   release, as the user decided.
//! - The strings are `&str` (their bytes are ASCII and NUL-free); `sysctl_rdstring` and the
//!   console take their bytes.

/// The operating system's name; a macro so `concat!` can build the other strings from it.
macro_rules! ost {
    () => {
        "EmiBSD"
    };
}

/// The release; a macro for the same reason as [`ost`].
macro_rules! osr {
    () => {
        "7.8"
    };
}

/// `STATUS`: empty for a release (`-current`, `-beta` and `-stable` are the others).
macro_rules! status {
    () => {
        ""
    };
}

/// The kernel configuration name, `${id}` in `newvers.sh`.
macro_rules! id {
    () => {
        "GENERIC"
    };
}

/// `ostype`.
pub const OSTYPE: &str = ost!();

/// `osrelease`.
pub const OSRELEASE: &str = osr!();

/// `osversion`: `${id}#${v}`.
pub const OSVERSION: &str = concat!(id!(), "#", env!("EMIBSD_VERS_BUILD"));

/// `sccs`: the `what(1)` string.
pub const SCCS: &str = concat!(
    "    @(#)",
    ost!(),
    " ",
    osr!(),
    status!(),
    " (",
    id!(),
    ") #",
    env!("EMIBSD_VERS_BUILD"),
    ": ",
    env!("EMIBSD_VERS_DATE"),
    "\n"
);

/// `version`: printed at boot and reported by `kern.version`.
pub const VERSION: &str = concat!(
    ost!(),
    " ",
    osr!(),
    status!(),
    " (",
    id!(),
    ") #",
    env!("EMIBSD_VERS_BUILD"),
    ": ",
    env!("EMIBSD_VERS_DATE"),
    "\n    ",
    env!("EMIBSD_VERS_USER"),
    "@",
    env!("EMIBSD_VERS_HOST"),
    ":",
    env!("EMIBSD_VERS_DIR"),
    "\n"
);

const _: () = {
    // `const char version[512]` in C.
    assert!(VERSION.len() < 512);
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_identity_is_emibsd_7_8() {
        assert_eq!(OSTYPE, "EmiBSD");
        assert_eq!(OSRELEASE, "7.8");
        assert!(VERSION.starts_with("EmiBSD 7.8 (GENERIC) #"));
        assert!(VERSION.ends_with('\n'));
        assert!(VERSION.contains("\n    "));
        assert!(OSVERSION.starts_with("GENERIC#"));
        assert!(SCCS.starts_with("    @(#)EmiBSD 7.8 (GENERIC) #"));
        assert!(!VERSION.contains('\0'));
    }
}
