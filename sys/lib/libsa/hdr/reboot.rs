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
//! `<sys/reboot.h>` for libsa and the boot programs: the `boothowto` flags and the
//! `bootdev` encoding.

/// `RB_ASKNAME`: ask for the root device.
pub const RB_ASKNAME: i32 = 0x00001;
/// `RB_SINGLE`: boot to single user.
pub const RB_SINGLE: i32 = 0x00002;
/// `RB_KDB`: enter the kernel debugger.
pub const RB_KDB: i32 = 0x00040;
/// `RB_CONFIG`: run the kernel configuration editor.
pub const RB_CONFIG: i32 = 0x00400;
/// `RB_GOODRANDOM`: the boot loader passed an excellent random seed.
pub const RB_GOODRANDOM: i32 = 0x10000;
/// `RB_UNHIBERNATE`: resume from hibernation.
pub const RB_UNHIBERNATE: i32 = 0x20000;

/// `B_ADAPTORSHIFT`.
pub const B_ADAPTORSHIFT: u32 = 24;
/// `B_ADAPTORMASK`.
pub const B_ADAPTORMASK: u32 = 0x0f;
/// `B_CONTROLLERSHIFT`.
pub const B_CONTROLLERSHIFT: u32 = 20;
/// `B_CONTROLLERMASK`.
pub const B_CONTROLLERMASK: u32 = 0xf;
/// `B_UNITSHIFT`.
pub const B_UNITSHIFT: u32 = 16;
/// `B_UNITMASK`.
pub const B_UNITMASK: u32 = 0xf;
/// `B_PARTITIONSHIFT`.
pub const B_PARTITIONSHIFT: u32 = 8;
/// `B_PARTITIONMASK`.
pub const B_PARTITIONMASK: u32 = 0xff;
/// `B_TYPESHIFT`.
pub const B_TYPESHIFT: u32 = 0;
/// `B_TYPEMASK`.
pub const B_TYPEMASK: u32 = 0xff;
/// `B_MAGICMASK`.
pub const B_MAGICMASK: u32 = 0xf000_0000;
/// `B_DEVMAGIC`: the magic of a valid `bootdev`.
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
