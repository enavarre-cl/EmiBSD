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

/*
 * Copyright (c) 1982, 1986, 1988, 1993, 1994
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
 *	@(#)reboot.h	8.2 (Berkeley) 7/10/94
 */
/* </LICENSES> */

/* <CODE> */
//! Arguments to `reboot(2)`, `boot(9)` and the boot loader: `<sys/reboot.h>`.
//!
//! Upstream: sys/sys/reboot.h @ 3ce1f3f79392
//!
//! `reboot()` lives in `kern/kern_xxx.rs`, `boot()` in each architecture's `machdep.rs`.
//!
//! ## Deviations
//! - The `B_*` and `MAKEBOOTDEV` macros are `const fn`s.

/// Flags for system auto-booting itself.
pub const RB_AUTOBOOT: i32 = 0;

/// Ask for file name to reboot from.
pub const RB_ASKNAME: i32 = 0x00001;
/// Reboot to single user only.
pub const RB_SINGLE: i32 = 0x00002;
/// Dont sync before reboot.
pub const RB_NOSYNC: i32 = 0x00004;
/// Don't reboot, just halt.
pub const RB_HALT: i32 = 0x00008;
/// Name given for /etc/init (unused).
pub const RB_INITNAME: i32 = 0x00010;
/// Use compiled-in rootdev.
pub const RB_DFLTROOT: i32 = 0x00020;
/// Give control to kernel debugger.
pub const RB_KDB: i32 = 0x00040;
/// Mount root fs read-only.
pub const RB_RDONLY: i32 = 0x00080;
/// Dump kernel memory before reboot.
pub const RB_DUMP: i32 = 0x00100;
/// Mini-root present in memory at boot time.
pub const RB_MINIROOT: i32 = 0x00200;
/// Change configured devices.
pub const RB_CONFIG: i32 = 0x00400;
/// Don't call resettodr() in boot().
pub const RB_TIMEBAD: i32 = 0x00800;
/// Attempt to power down machine.
pub const RB_POWERDOWN: i32 = 0x01000;
/// Use serial console if available.
pub const RB_SERCONS: i32 = 0x02000;
/// boot() called at user request (e.g. ddb).
pub const RB_USERREQ: i32 = 0x04000;
/// Just reset, no cleanup.
pub const RB_RESET: i32 = 0x08000;
/// Excellent random seed loaded.
pub const RB_GOODRANDOM: i32 = 0x10000;
/// Unhibernate.
pub const RB_UNHIBERNATE: i32 = 0x20000;
/// Booting confidential VM (e.g. SEV enabled).
pub const RB_COCOVM: i32 = 0x40000;

/*
 * Constants for converting boot-style device number to type,
 * adaptor (uba, mba, etc), unit number and partition number.
 * Type (== major device number) is in the low byte
 * for backward compatibility.  Except for that of the "magic
 * number", each mask applies to the shifted value.
 * Format:
 *	 (4) (4) (4) (4)  (8)     (8)
 *	--------------------------------
 *	|MA | AD| CT| UN| PART  | TYPE |
 *	--------------------------------
 */

/// Shift of the adaptor field.
pub const B_ADAPTORSHIFT: u32 = 24;
/// Mask of the adaptor field.
pub const B_ADAPTORMASK: u32 = 0x0f;
/// Shift of the controller field.
pub const B_CONTROLLERSHIFT: u32 = 20;
/// Mask of the controller field.
pub const B_CONTROLLERMASK: u32 = 0xf;
/// Shift of the unit field.
pub const B_UNITSHIFT: u32 = 16;
/// Mask of the unit field.
pub const B_UNITMASK: u32 = 0xf;
/// Shift of the partition field.
pub const B_PARTITIONSHIFT: u32 = 8;
/// Mask of the partition field.
pub const B_PARTITIONMASK: u32 = 0xff;
/// Shift of the type field.
pub const B_TYPESHIFT: u32 = 0;
/// Mask of the type field.
pub const B_TYPEMASK: u32 = 0xff;

/// Mask of the magic number.
pub const B_MAGICMASK: u32 = 0xf000_0000;
/// The magic number of a boot device number.
pub const B_DEVMAGIC: u32 = 0xa000_0000;

/// `B_ADAPTOR(val)`.
pub const fn b_adaptor(val: u32) -> u32 {
    (val >> B_ADAPTORSHIFT) & B_ADAPTORMASK
}

/// `B_CONTROLLER(val)`.
pub const fn b_controller(val: u32) -> u32 {
    (val >> B_CONTROLLERSHIFT) & B_CONTROLLERMASK
}

/// `B_UNIT(val)`.
pub const fn b_unit(val: u32) -> u32 {
    (val >> B_UNITSHIFT) & B_UNITMASK
}

/// `B_PARTITION(val)`.
pub const fn b_partition(val: u32) -> u32 {
    (val >> B_PARTITIONSHIFT) & B_PARTITIONMASK
}

/// `B_TYPE(val)`.
pub const fn b_type(val: u32) -> u32 {
    (val >> B_TYPESHIFT) & B_TYPEMASK
}

/// `MAKEBOOTDEV(type, adaptor, controller, unit, partition)`.
pub const fn makebootdev(ty: u32, adaptor: u32, controller: u32, unit: u32, partition: u32) -> u32 {
    (ty << B_TYPESHIFT)
        | (adaptor << B_ADAPTORSHIFT)
        | (controller << B_CONTROLLERSHIFT)
        | (unit << B_UNITSHIFT)
        | (partition << B_PARTITIONSHIFT)
        | B_DEVMAGIC
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bootdev_round_trip() {
        let dev = makebootdev(4, 1, 2, 3, 5);
        assert_eq!(b_type(dev), 4);
        assert_eq!(b_adaptor(dev), 1);
        assert_eq!(b_controller(dev), 2);
        assert_eq!(b_unit(dev), 3);
        assert_eq!(b_partition(dev), 5);
        assert_eq!(dev & B_MAGICMASK, B_DEVMAGIC);
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/sys/reboot.h");
        let ours: &[(&str, i64)] = &[
            ("RB_AUTOBOOT", RB_AUTOBOOT as i64),
            ("RB_ASKNAME", RB_ASKNAME as i64),
            ("RB_SINGLE", RB_SINGLE as i64),
            ("RB_NOSYNC", RB_NOSYNC as i64),
            ("RB_HALT", RB_HALT as i64),
            ("RB_INITNAME", RB_INITNAME as i64),
            ("RB_DFLTROOT", RB_DFLTROOT as i64),
            ("RB_KDB", RB_KDB as i64),
            ("RB_RDONLY", RB_RDONLY as i64),
            ("RB_DUMP", RB_DUMP as i64),
            ("RB_MINIROOT", RB_MINIROOT as i64),
            ("RB_CONFIG", RB_CONFIG as i64),
            ("RB_TIMEBAD", RB_TIMEBAD as i64),
            ("RB_POWERDOWN", RB_POWERDOWN as i64),
            ("RB_SERCONS", RB_SERCONS as i64),
            ("RB_USERREQ", RB_USERREQ as i64),
            ("RB_RESET", RB_RESET as i64),
            ("RB_GOODRANDOM", RB_GOODRANDOM as i64),
            ("RB_UNHIBERNATE", RB_UNHIBERNATE as i64),
            ("RB_COCOVM", RB_COCOVM as i64),
            ("B_ADAPTORSHIFT", B_ADAPTORSHIFT as i64),
            ("B_ADAPTORMASK", B_ADAPTORMASK as i64),
            ("B_CONTROLLERSHIFT", B_CONTROLLERSHIFT as i64),
            ("B_CONTROLLERMASK", B_CONTROLLERMASK as i64),
            ("B_UNITSHIFT", B_UNITSHIFT as i64),
            ("B_UNITMASK", B_UNITMASK as i64),
            ("B_PARTITIONSHIFT", B_PARTITIONSHIFT as i64),
            ("B_PARTITIONMASK", B_PARTITIONMASK as i64),
            ("B_TYPESHIFT", B_TYPESHIFT as i64),
            ("B_TYPEMASK", B_TYPEMASK as i64),
            ("B_MAGICMASK", B_MAGICMASK as i64),
            ("B_DEVMAGIC", B_DEVMAGIC as i64),
        ];
        for (name, value) in ours {
            assert_eq!(crate::reftest::int(&defs, name), Some(*value), "{name}");
        }
    }
}
/* </TESTS> */
