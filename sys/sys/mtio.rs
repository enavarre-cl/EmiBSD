/*	$OpenBSD: mtio.h,v 1.9 2007/06/01 18:44:48 krw Exp $	*/
/*	$NetBSD: mtio.h,v 1.14 1997/04/15 06:50:19 lukem Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1982, 1986, 1993
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
 *	@(#)mtio.h	8.1 (Berkeley) 6/2/93
 */
/* </LICENSES> */

/* <CODE> */
//! `<sys/mtio.h>`: structures and definitions for mag tape io control commands.
//!
//! Upstream: sys/sys/mtio.h @ 3ce1f3f79392
//!
//! The ioctl ABI of the tape devices (`st(4)`); `kern_pledge.c`'s `pledge_ioctl` lists the
//! `MTIOC*` commands for the `tape` promise.
//!
//! ## Deviations
//! - The C `short` and `int` fields are `i16` and `i32`; the `MT*` operation codes and the
//!   `MT_IS*`/`MT_DS_*` constants are `i16` because they are stored in `short` fields, the
//!   minor-number masks (`T_*`, the header's `_KERNEL` part) are `i32`.
//! - `u_int32_t` ioctl arguments are `u32`.

use crate::sys::ioccom::{_io, _ior, _iow};

/// `struct mtop`: the argument of `MTIOCTOP`, a mag tape op command.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mtop {
    /// `mt_op`: operations defined below.
    pub mt_op: i16,
    /// `mt_count`: how many of them.
    pub mt_count: i32,
}

// operations

/// `MTWEOF`: write an end-of-file record.
pub const MTWEOF: i16 = 0;
/// `MTFSF`: forward space file.
pub const MTFSF: i16 = 1;
/// `MTBSF`: backward space file.
pub const MTBSF: i16 = 2;
/// `MTFSR`: forward space record.
pub const MTFSR: i16 = 3;
/// `MTBSR`: backward space record.
pub const MTBSR: i16 = 4;
/// `MTREW`: rewind.
pub const MTREW: i16 = 5;
/// `MTOFFL`: rewind and put the drive offline.
pub const MTOFFL: i16 = 6;
/// `MTNOP`: no operation, sets status only.
pub const MTNOP: i16 = 7;
/// `MTRETEN`: retension.
pub const MTRETEN: i16 = 8;
/// `MTERASE`: erase entire tape.
pub const MTERASE: i16 = 9;
/// `MTEOM`: forward to end of media.
pub const MTEOM: i16 = 10;
/// `MTNBSF`: backward space to beginning of file.
pub const MTNBSF: i16 = 11;
/// `MTCACHE`: enable controller cache.
pub const MTCACHE: i16 = 12;
/// `MTNOCACHE`: disable controller cache.
pub const MTNOCACHE: i16 = 13;
/// `MTSETBSIZ`: set block size; 0 for variable.
pub const MTSETBSIZ: i16 = 14;
/// `MTSETDNSTY`: set density code for current mode.
pub const MTSETDNSTY: i16 = 15;

/// `struct mtget`: the argument of `MTIOCGET`, the mag tape get status command.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mtget {
    /// `mt_type`: type of magtape device.
    pub mt_type: i16,
    // the following two registers are grossly device dependent
    /// `mt_dsreg`: "drive status" register.
    pub mt_dsreg: i16,
    /// `mt_erreg`: "error" register.
    pub mt_erreg: i16,
    // end device-dependent registers
    /// `mt_resid`: residual count.
    pub mt_resid: i16,
    /// `mt_fileno`: current file number relative to BOT.
    pub mt_fileno: i32,
    /// `mt_blkno`: current block number relative to BOF.
    pub mt_blkno: i32,
    /// `mt_blksiz`: current block size.
    pub mt_blksiz: i32,
    /// `mt_density`: current density code.
    pub mt_density: i32,
    /// `mt_mblksiz`: default block size.
    pub mt_mblksiz: i32,
    /// `mt_mdensity`: default density code.
    pub mt_mdensity: i32,
}

// Constants for mt_type byte. These are the same for controllers compatible with the types
// listed.

/// `MT_ISTS`: TS-11.
pub const MT_ISTS: i16 = 0x01;
/// `MT_ISHT`: TM03 Massbus: TE16, TU45, TU77.
pub const MT_ISHT: i16 = 0x02;
/// `MT_ISTM`: TM11/TE10 Unibus.
pub const MT_ISTM: i16 = 0x03;
/// `MT_ISMT`: TM78/TU78 Massbus.
pub const MT_ISMT: i16 = 0x04;
/// `MT_ISUT`: SI TU-45 emulation on Unibus.
pub const MT_ISUT: i16 = 0x05;
/// `MT_ISCPC`: SUN.
pub const MT_ISCPC: i16 = 0x06;
/// `MT_ISAR`: SUN.
pub const MT_ISAR: i16 = 0x07;
/// `MT_ISTMSCP`: DEC TMSCP protocol (TU81, TK50).
pub const MT_ISTMSCP: i16 = 0x08;
/// `MT_ISCY`: CCI Cipher.
pub const MT_ISCY: i16 = 0x09;
/// `MT_ISCT`: HP 1/4 tape.
pub const MT_ISCT: i16 = 0x0a;
/// `MT_ISFHP`: HP 7980 1/2 tape.
pub const MT_ISFHP: i16 = 0x0b;
/// `MT_ISEXABYTE`: Exabyte.
pub const MT_ISEXABYTE: i16 = 0x0c;
/// `MT_ISEXA8200`: Exabyte EXB-8200.
pub const MT_ISEXA8200: i16 = 0x0c;
/// `MT_ISEXA8500`: Exabyte EXB-8500.
pub const MT_ISEXA8500: i16 = 0x0d;
/// `MT_ISVIPER1`: Archive Viper-150.
pub const MT_ISVIPER1: i16 = 0x0e;
/// `MT_ISPYTHON`: Archive Python (DAT).
pub const MT_ISPYTHON: i16 = 0x0f;
/// `MT_ISHPDAT`: HP 35450A DAT drive.
pub const MT_ISHPDAT: i16 = 0x10;
/// `MT_ISWANGTEK`: WANGTEK 5150ES.
pub const MT_ISWANGTEK: i16 = 0x11;
/// `MT_ISCALIPER`: Caliper CP150.
pub const MT_ISCALIPER: i16 = 0x12;
/// `MT_ISWTEK5099`: WANGTEK 5099ES.
pub const MT_ISWTEK5099: i16 = 0x13;
/// `MT_ISVIPER2525`: Archive Viper 2525.
pub const MT_ISVIPER2525: i16 = 0x14;
/// `MT_ISMFOUR`: M4 Data 1/2 9track drive.
pub const MT_ISMFOUR: i16 = 0x11;
/// `MT_ISTK50`: DEC SCSI TK50.
pub const MT_ISTK50: i16 = 0x12;
/// `MT_ISMT02`: Emulex MT02 SCSI tape controller.
pub const MT_ISMT02: i16 = 0x13;

// bits defined for the mt_dsreg field

/// `MT_DS_RDONLY`: tape mounted readonly.
pub const MT_DS_RDONLY: i16 = 0x10;
/// `MT_DS_MOUNTED`: tape mounted (for control opens).
pub const MT_DS_MOUNTED: i16 = 0x03;

// mag tape io control commands

/// `MTIOCTOP`: do a mag tape op.
pub const MTIOCTOP: u64 = _iow::<Mtop>(b'm', 1);
/// `MTIOCGET`: get tape status.
pub const MTIOCGET: u64 = _ior::<Mtget>(b'm', 2);
/// `MTIOCIEOT`: ignore EOT error.
pub const MTIOCIEOT: u64 = _io(b'm', 3);
/// `MTIOCEEOT`: enable EOT error.
pub const MTIOCEEOT: u64 = _io(b'm', 4);

// When more SCSI-3 SSC (streaming device) devices are out there that support the full 32 byte
// type 2 structure, these ioctls will have to be rethought to support all the entities they
// haul into the picture (64 bit blocks, logical file record numbers, etc.).

/// `MTIOCRDSPOS`: get logical blk addr.
pub const MTIOCRDSPOS: u64 = _ior::<u32>(b'm', 5);
/// `MTIOCRDHPOS`: get hardware blk addr.
pub const MTIOCRDHPOS: u64 = _ior::<u32>(b'm', 6);
/// `MTIOCSLOCATE`: seek to logical blk addr.
pub const MTIOCSLOCATE: u64 = _iow::<u32>(b'm', 5);
/// `MTIOCHLOCATE`: seek to hardware blk addr.
pub const MTIOCHLOCATE: u64 = _iow::<u32>(b'm', 6);

// minor device number (the header's `_KERNEL` part)

/// `T_UNIT`: unit selection.
pub const T_UNIT: i32 = 0o003;
/// `T_NOREWIND`: no rewind on close.
pub const T_NOREWIND: i32 = 0o004;
/// `T_DENSEL`: density select.
pub const T_DENSEL: i32 = 0o030;
/// `T_800BPI`: select 800 bpi.
pub const T_800BPI: i32 = 0o000;
/// `T_1600BPI`: select 1600 bpi.
pub const T_1600BPI: i32 = 0o010;
/// `T_6250BPI`: select 6250 bpi.
pub const T_6250BPI: i32 = 0o020;
/// `T_BADBPI`: undefined selection.
pub const T_BADBPI: i32 = 0o030;

const _: () = {
    assert!(size_of::<Mtop>() == 8);
    assert!(size_of::<Mtget>() == 32);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_have_openbsd_values() {
        // The values the C compiler computes for _IOW('m', 1, struct mtop) and friends.
        assert_eq!(MTIOCTOP, 0x8008_6d01);
        assert_eq!(MTIOCGET, 0x4020_6d02);
        assert_eq!(MTIOCIEOT, 0x2000_6d03);
        assert_eq!(MTIOCEEOT, 0x2000_6d04);
        assert_eq!(MTIOCRDSPOS, 0x4004_6d05);
        assert_eq!(MTIOCRDHPOS, 0x4004_6d06);
        assert_eq!(MTIOCSLOCATE, 0x8004_6d05);
        assert_eq!(MTIOCHLOCATE, 0x8004_6d06);
    }

    #[test]
    fn layouts_match_the_c_structs() {
        assert_eq!(core::mem::offset_of!(Mtop, mt_count), 4);
        assert_eq!(core::mem::offset_of!(Mtget, mt_fileno), 8);
        assert_eq!(core::mem::offset_of!(Mtget, mt_mdensity), 28);
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/sys/mtio.h");
        let mut ours = crate::reftest::assert_defines!(defs;
        MTWEOF, MTFSF, MTBSF, MTFSR, MTBSR, MTREW, MTOFFL, MTNOP, MTRETEN, MTERASE, MTEOM,
        MTNBSF, MTCACHE, MTNOCACHE, MTSETBSIZ, MTSETDNSTY);
        ours.extend(crate::reftest::assert_defines!(defs;
        MT_ISTS, MT_ISHT, MT_ISTM, MT_ISMT, MT_ISUT, MT_ISCPC, MT_ISAR, MT_ISTMSCP, MT_ISCY,
        MT_ISCT, MT_ISFHP, MT_ISEXABYTE, MT_ISEXA8200, MT_ISEXA8500, MT_ISVIPER1,
        MT_ISPYTHON, MT_ISHPDAT, MT_ISWANGTEK, MT_ISCALIPER, MT_ISWTEK5099, MT_ISVIPER2525,
        MT_ISMFOUR, MT_ISTK50, MT_ISMT02, MT_DS_RDONLY, MT_DS_MOUNTED));
        ours.extend(crate::reftest::assert_defines!(defs;
        T_UNIT, T_NOREWIND, T_DENSEL, T_800BPI, T_1600BPI, T_6250BPI, T_BADBPI));
        // The ioctl numbers are _IO* expressions, which the helper does not evaluate:
        // `commands_have_openbsd_values` pins their values, this pins the expressions.
        let ioctls = [
            ("MTIOCTOP", "_IOW('m', 1, struct mtop)"),
            ("MTIOCGET", "_IOR('m', 2, struct mtget)"),
            ("MTIOCIEOT", "_IO('m', 3)"),
            ("MTIOCEEOT", "_IO('m', 4)"),
            ("MTIOCRDSPOS", "_IOR('m', 5, u_int32_t)"),
            ("MTIOCRDHPOS", "_IOR('m', 6, u_int32_t)"),
            ("MTIOCSLOCATE", "_IOW('m', 5, u_int32_t)"),
            ("MTIOCHLOCATE", "_IOW('m', 6, u_int32_t)"),
        ];
        for (name, text) in ioctls {
            assert_eq!(defs.get(name).map(|s| s.as_str()), Some(text), "{name}");
            ours.push(name);
        }
        crate::reftest::assert_complete(&defs, "MT", &ours);
        crate::reftest::assert_complete(&defs, "T_", &ours);
    }
}
/* </TESTS> */
