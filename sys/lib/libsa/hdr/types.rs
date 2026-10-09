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
//! `<sys/types.h>` for libsa: the scalar types and the `dev_t` macros.

/// `dev_t`: a device number.
pub type Dev = i32;
/// `daddr_t`: a 64-bit disk address.
pub type Daddr = i64;
/// `daddr32_t`: the 32-bit disk addresses of FFS1.
pub type Daddr32 = i32;
/// `off_t`: a file offset.
pub type Off = i64;
/// `mode_t`: file type and permissions.
pub type Mode = u32;
/// `time_t`: seconds since the Epoch.
pub type Time = i64;
/// `uid_t`.
pub type Uid = u32;
/// `gid_t`.
pub type Gid = u32;
/// `ufsino_t`: an FFS inode number.
pub type Ufsino = u32;

/// `NODEV`: no device.
pub const NODEV: Dev = -1;

/// `major(x)`: the major number of a device.
pub const fn major(x: Dev) -> u32 {
    ((x as u32) >> 8) & 0xff
}

/// `minor(x)`: the minor number of a device.
pub const fn minor(x: Dev) -> u32 {
    let x = x as u32;
    (x & 0xff) | ((x & 0xffff_0000) >> 8)
}

/// `makedev(x, y)`: a device from its major and minor numbers.
pub const fn makedev(x: u32, y: u32) -> Dev {
    (((x & 0xff) << 8) | (y & 0xff) | ((y & 0x00ff_ff00) << 8)) as Dev
}
/* </CODE> */
