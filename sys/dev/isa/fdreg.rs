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
 *	@(#)fdreg.h	7.1 (Berkeley) 5/9/91
 */
/* </LICENSES> */

/* <CODE> */
//! AT floppy controller registers and bitfields: `<dev/isa/fdreg.h>`.
//!
//! Upstream: sys/dev/isa/fdreg.h @ 3ce1f3f79392
//!
//! The controller is a NEC 765 (`<dev/ic/nec765reg.h>`, re-exported here as the C header
//! includes it) behind the PC's digital output, status, data and control registers.
//!
//! ## Deviations
//! - The register offsets keep their lower-case C names (`fdout`, `fdsts`, `fddata`,
//!   `fdctl`, `fdin`).
//! - `FDO_MOEN(n)`, `FDUNIT(dev)`, `FDTYPE(dev)` and `FDPART(dev)` are the `const fn`s
//!   `fdo_moen`, `fdunit`, `fdtype` and `fdpart`; `FD_BSIZE(fd)` (`128 <<
//!   fd->sc_type->secsize`) is `fd_bsize` of the type's size code.

#![allow(non_upper_case_globals)] // the C's lower-case register names

pub use crate::dev::ic::nec765reg::*;
use crate::sys::disklabel::MAXPARTITIONSUNIT;
use crate::sys::param::NBPG;
use crate::sys::types::{Dev, minor};

// registers

/// `fdout`: Digital Output Register (W).
pub const fdout: usize = 2;
/// `FDO_FDSEL`: floppy device select.
pub const FDO_FDSEL: u8 = 0x03;
/// `FDO_FRST`: floppy controller reset.
pub const FDO_FRST: u8 = 0x04;
/// `FDO_FDMAEN`: enable floppy DMA and Interrupt.
pub const FDO_FDMAEN: u8 = 0x08;

/// `FDO_MOEN(n)`: motor enable.
pub const fn fdo_moen(n: i32) -> u8 {
    ((1 << n) * 0x10) as u8
}

/// `fdsts`: NEC 765 Main Status Register (R).
pub const fdsts: usize = 4;
/// `fddata`: NEC 765 Data Register (R/W).
pub const fddata: usize = 5;

/// `FDCTL_OFFSET`: Offset from the other registers.
pub const FDCTL_OFFSET: usize = 7;
/// `fdctl`: Control Register (W).
pub const fdctl: usize = 0;
/// `FDC_500KBPS`: 500KBPS MFM drive transfer rate.
pub const FDC_500KBPS: i32 = 0x00;
/// `FDC_300KBPS`: 300KBPS MFM drive transfer rate.
pub const FDC_300KBPS: i32 = 0x01;
/// `FDC_250KBPS`: 250KBPS MFM drive transfer rate.
pub const FDC_250KBPS: i32 = 0x02;
/// `FDC_125KBPS`: 125KBPS FM drive transfer rate.
pub const FDC_125KBPS: i32 = 0x03;

/// `fdin`: Digital Input Register (R).
pub const fdin: usize = 0;
/// `FDI_DCHG`: diskette has been changed.
pub const FDI_DCHG: u8 = 0x80;

/// `FDC_NPORT`.
pub const FDC_NPORT: usize = 6;
/// `FDCTL_NPORT`.
pub const FDCTL_NPORT: usize = 1;
/// `FDC_MAXIOSIZE`: XXX should be MAXBSIZE.
pub const FDC_MAXIOSIZE: i32 = NBPG as i32;

/// `FD_BSIZE(fd)`: the sector size of a type whose size code is `secsize`.
pub const fn fd_bsize(secsize: i32) -> i32 {
    128 << secsize
}

/// `FDUNIT(dev)`.
pub const fn fdunit(dev: Dev) -> u32 {
    (minor(dev) / MAXPARTITIONSUNIT) / 8
}

/// `FDTYPE(dev)`.
pub const fn fdtype(dev: Dev) -> u32 {
    (minor(dev) / MAXPARTITIONSUNIT) % 8
}

/// `FDPART(dev)`.
pub const fn fdpart(dev: Dev) -> u32 {
    minor(dev) % MAXPARTITIONSUNIT
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;
    use crate::sys::types::makedev;

    #[test]
    fn minor_numbers() {
        // MAKEDEV's fd0c (block 2, minor 2), and unit 1 type B partition a (minor 576).
        let fd0c = makedev(2, 2);
        assert_eq!((fdunit(fd0c), fdtype(fd0c), fdpart(fd0c)), (0, 0, 2));
        let fd1b_a = makedev(2, (8 + 1) * 64);
        assert_eq!((fdunit(fd1b_a), fdtype(fd1b_a), fdpart(fd1b_a)), (1, 1, 0));
        assert_eq!(fdo_moen(1), 0x20);
        assert_eq!(fd_bsize(2), 512);
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/isa/fdreg.h");
        for (name, v) in [
            ("fdout", fdout as i64),
            ("FDO_FDSEL", FDO_FDSEL.into()),
            ("FDO_FRST", FDO_FRST.into()),
            ("FDO_FDMAEN", FDO_FDMAEN.into()),
            ("fdsts", fdsts as i64),
            ("fddata", fddata as i64),
            ("FDCTL_OFFSET", FDCTL_OFFSET as i64),
            ("fdctl", fdctl as i64),
            ("FDC_500KBPS", FDC_500KBPS.into()),
            ("FDC_300KBPS", FDC_300KBPS.into()),
            ("FDC_250KBPS", FDC_250KBPS.into()),
            ("FDC_125KBPS", FDC_125KBPS.into()),
            ("fdin", fdin as i64),
            ("FDI_DCHG", FDI_DCHG.into()),
            ("FDC_NPORT", FDC_NPORT as i64),
            ("FDCTL_NPORT", FDCTL_NPORT as i64),
        ] {
            assert_eq!(crate::reftest::int(&defs, name), Some(v), "{name}");
        }
    }
}
/* </TESTS> */
