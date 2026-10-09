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
/* </LICENSES> */

/* <CODE> */
//! The i8042 keyboard controller's ports, commands and command byte: `<dev/ic/i8042reg.h>`.
//!
//! Upstream: sys/dev/ic/i8042reg.h @ 3ce1f3f79392
//!
//! `pckbc(4)` drives the controller through two ports: the data port ([`KBDATAP`],
//! [`KBOUTP`]) at its base and the status/command port ([`KBSTATP`], [`KBCMDP`]) four bytes
//! above it. The `KBS_*` bits are the status register, the `KBC_*` values the controller
//! commands, the `KC8_*` bits its command byte.
//!
//! ## Deviations
//! - None: the constants keep their names and values; the port offsets are `usize`, the
//!   register values `u8`.

/// `KBSTATP`: kbd controller status port (I).
pub const KBSTATP: usize = 4;
/// `KBS_DIB`: kbd data in buffer.
pub const KBS_DIB: u8 = 0x01;
/// `KBS_IBF`: kbd input buffer low.
pub const KBS_IBF: u8 = 0x02;
/// `KBS_WARM`: kbd input buffer low.
pub const KBS_WARM: u8 = 0x04;
/// `KBS_OCMD`: kbd output buffer has command.
pub const KBS_OCMD: u8 = 0x08;
/// `KBS_NOSEC`: kbd security lock not engaged.
pub const KBS_NOSEC: u8 = 0x10;
/// `KBS_AUXDATA`: kbd data in buffer from aux port.
pub const KBS_AUXDATA: u8 = 0x20;
/// `KBS_RERR`: kbd receive error.
pub const KBS_RERR: u8 = 0x40;
/// `KBS_PERR`: kbd parity error.
pub const KBS_PERR: u8 = 0x80;

/// `KBCMDP`: kbd controller port (O).
pub const KBCMDP: usize = 4;
/// `KBC_RAMREAD`: read from RAM.
pub const KBC_RAMREAD: u8 = 0x20;
/// `KBC_RAMWRITE`: write to RAM.
pub const KBC_RAMWRITE: u8 = 0x60;
/// `KBC_AUXDISABLE`: disable auxiliary port.
pub const KBC_AUXDISABLE: u8 = 0xa7;
/// `KBC_AUXENABLE`: enable auxiliary port.
pub const KBC_AUXENABLE: u8 = 0xa8;
/// `KBC_AUXTEST`: test auxiliary port.
pub const KBC_AUXTEST: u8 = 0xa9;
/// `KBC_CMDWOUT`: write output port.
pub const KBC_CMDWOUT: u8 = 0xd1;
/// `KBC_KBDECHO`: echo to keyboard port.
pub const KBC_KBDECHO: u8 = 0xd2;
/// `KBC_AUXECHO`: echo to auxiliary port.
pub const KBC_AUXECHO: u8 = 0xd3;
/// `KBC_AUXWRITE`: write to auxiliary port.
pub const KBC_AUXWRITE: u8 = 0xd4;
/// `KBC_SELFTEST`: start self-test.
pub const KBC_SELFTEST: u8 = 0xaa;
/// `KBC_KBDTEST`: test keyboard port.
pub const KBC_KBDTEST: u8 = 0xab;
/// `KBC_KBDDISABLE`: disable keyboard port.
pub const KBC_KBDDISABLE: u8 = 0xad;
/// `KBC_KBDENABLE`: enable keyboard port.
pub const KBC_KBDENABLE: u8 = 0xae;
/// `KBC_READID`: read device id.
pub const KBC_READID: u8 = 0xf2;
/// `KBC_PULSE0`: pulse output bit 0.
pub const KBC_PULSE0: u8 = 0xfe;
/// `KBC_PULSE1`: pulse output bit 1.
pub const KBC_PULSE1: u8 = 0xfd;
/// `KBC_PULSE2`: pulse output bit 2.
pub const KBC_PULSE2: u8 = 0xfb;
/// `KBC_PULSE3`: pulse output bit 3.
pub const KBC_PULSE3: u8 = 0xf7;

/// `KBDATAP`: kbd data port (I).
pub const KBDATAP: usize = 0;
/// `KBOUTP`: kbd data port (O).
pub const KBOUTP: usize = 0;

/// `K_RDCMDBYTE`: read the command byte.
pub const K_RDCMDBYTE: u8 = 0x20;
/// `K_LDCMDBYTE`: load the command byte.
pub const K_LDCMDBYTE: u8 = 0x60;

/// `KC8_TRANS`: convert to old scan codes.
pub const KC8_TRANS: u8 = 0x40;
/// `KC8_MDISABLE`: disable mouse.
pub const KC8_MDISABLE: u8 = 0x20;
/// `KC8_KDISABLE`: disable keyboard.
pub const KC8_KDISABLE: u8 = 0x10;
/// `KC8_IGNSEC`: ignore security lock.
pub const KC8_IGNSEC: u8 = 0x08;
/// `KC8_CPU`: exit from protected mode reset.
pub const KC8_CPU: u8 = 0x04;
/// `KC8_MENABLE`: enable mouse interrupt.
pub const KC8_MENABLE: u8 = 0x02;
/// `KC8_KENABLE`: enable keyboard interrupt.
pub const KC8_KENABLE: u8 = 0x01;
/// `CMDBYTE`: translation, protected mode, both interrupts.
pub const CMDBYTE: u8 = KC8_TRANS | KC8_CPU | KC8_MENABLE | KC8_KENABLE;

/// `KCID_KBD1`: first byte of a keyboard's id.
pub const KCID_KBD1: u8 = 0xAB;
/// `KCID_KBD2`: second byte of a keyboard's id.
pub const KCID_KBD2: u8 = 0x83;
/// `KCID_MOUSE`: a mouse's id.
pub const KCID_MOUSE: u8 = 0x00;
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_command_byte() {
        assert_eq!(CMDBYTE, 0x47);
        assert_eq!(KBSTATP, KBCMDP);
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/ic/i8042reg.h");
        let ours: &[(&str, i64)] = &[
            ("KBSTATP", KBSTATP as i64),
            ("KBS_DIB", KBS_DIB.into()),
            ("KBS_IBF", KBS_IBF.into()),
            ("KBS_WARM", KBS_WARM.into()),
            ("KBS_OCMD", KBS_OCMD.into()),
            ("KBS_NOSEC", KBS_NOSEC.into()),
            ("KBS_AUXDATA", KBS_AUXDATA.into()),
            ("KBS_RERR", KBS_RERR.into()),
            ("KBS_PERR", KBS_PERR.into()),
            ("KBCMDP", KBCMDP as i64),
            ("KBC_RAMREAD", KBC_RAMREAD.into()),
            ("KBC_RAMWRITE", KBC_RAMWRITE.into()),
            ("KBC_AUXDISABLE", KBC_AUXDISABLE.into()),
            ("KBC_AUXENABLE", KBC_AUXENABLE.into()),
            ("KBC_AUXTEST", KBC_AUXTEST.into()),
            ("KBC_CMDWOUT", KBC_CMDWOUT.into()),
            ("KBC_KBDECHO", KBC_KBDECHO.into()),
            ("KBC_AUXECHO", KBC_AUXECHO.into()),
            ("KBC_AUXWRITE", KBC_AUXWRITE.into()),
            ("KBC_SELFTEST", KBC_SELFTEST.into()),
            ("KBC_KBDTEST", KBC_KBDTEST.into()),
            ("KBC_KBDDISABLE", KBC_KBDDISABLE.into()),
            ("KBC_KBDENABLE", KBC_KBDENABLE.into()),
            ("KBC_READID", KBC_READID.into()),
            ("KBC_PULSE0", KBC_PULSE0.into()),
            ("KBC_PULSE1", KBC_PULSE1.into()),
            ("KBC_PULSE2", KBC_PULSE2.into()),
            ("KBC_PULSE3", KBC_PULSE3.into()),
            ("KBDATAP", KBDATAP as i64),
            ("KBOUTP", KBOUTP as i64),
            ("K_RDCMDBYTE", K_RDCMDBYTE.into()),
            ("K_LDCMDBYTE", K_LDCMDBYTE.into()),
            ("KC8_TRANS", KC8_TRANS.into()),
            ("KC8_MDISABLE", KC8_MDISABLE.into()),
            ("KC8_KDISABLE", KC8_KDISABLE.into()),
            ("KC8_IGNSEC", KC8_IGNSEC.into()),
            ("KC8_CPU", KC8_CPU.into()),
            ("KC8_MENABLE", KC8_MENABLE.into()),
            ("KC8_KENABLE", KC8_KENABLE.into()),
            ("KCID_KBD1", KCID_KBD1.into()),
            ("KCID_KBD2", KCID_KBD2.into()),
            ("KCID_MOUSE", KCID_MOUSE.into()),
        ];
        for (name, value) in ours {
            assert_eq!(crate::reftest::int(&defs, name), Some(*value), "{name}");
        }
    }
}
/* </TESTS> */
