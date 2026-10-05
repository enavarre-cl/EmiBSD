/*	$OpenBSD: dev_i386.c,v 1.1 2019/05/10 21:20:42 mlarkin Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1996-1999 Michael Shalayeff
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
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR ``AS IS'' AND ANY EXPRESS OR
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES
 * OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED.
 * IN NO EVENT SHALL THE AUTHOR OR HIS RELATIVES BE LIABLE FOR ANY DIRECT,
 * INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES
 * (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
 * SERVICES; LOSS OF MIND, USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT,
 * STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING
 * IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF
 * THE POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

//! efiboot's devices: `devopen()` (the first device of `devsw[]` that claims the name), the
//! boot device's name (`devboot`), and the console names (`ttyname`, `ttydev`, `cnspeed`).
//!
//! Upstream: sys/arch/amd64/stand/efiboot/dev_i386.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `SOFTRAID`'s look for a bootable softraid volume holding the boot disk is feature
//!   `softraid`, not ported: `devboot` names the disk itself (the C does too when there is no
//!   such volume).
//! - `ttyname()` returns its buffer instead of a `static` one.

use core::sync::atomic::Ordering;

use boot::cmd::BOOTDEVLEN;
use libsa::cons::cn_tab;
use libsa::hdr::types::{Dev, NODEV, major, makedev, minor};
use libsa::saerrno::Errno;
use libsa::snprintf;
use libsa::stand::{F_NODEV, OpenFile, sa_conf};

use crate::efiboot::comspeed;
use crate::exec_i386::BOOTMAC;

/// `bdevs[]`: the BSD names of the block devices (XXX use slot for 'rd' for 'hd'
/// pseudo-device).
pub const BDEVS: [&str; 19] = [
    "wd", "", "fd", "", "sd", "st", "cd", "", "", "", "", "", "", "", "", "", "", "hd", "",
];
/// `nbdevs`.
pub const NBDEVS: usize = BDEVS.len();

/// `cdevs[]`: the console devices.
const CDEVS: [&str; 13] = ["cn", "", "", "", "", "", "", "", "com", "", "", "", "pc"];

/// `devopen(f, fname, &file)`: pass the name to every device's open routine; the rest of
/// the name, which is the file's.
pub fn devopen<'a>(f: &mut OpenFile, fname: &'a [u8]) -> Result<&'a [u8], Errno> {
    let devsw = sa_conf().devsw;
    let mut rc = Err(Errno(1));
    let mut file = fname;

    for dp in devsw {
        file = fname;
        rc = (dp.dv_open)(f, &mut file);
        if rc.is_ok() {
            f.f_dev = Some(dp);
            if dp.dv_name != "TFTP" {
                // Clear bootmac, to signal that we loaded this file from a non-network
                // device.
                BOOTMAC.store(core::ptr::null_mut(), Ordering::Relaxed);
            }
            return Ok(file);
        }
    }

    if (f.f_flags & F_NODEV) == 0 {
        // the C leaves f_dev past the table; no device was found
        f.f_dev = None;
    }

    rc.map(|()| file)
}

/// `devboot(bootdev, p)`: the name of the device we booted from (`hd0a`, `cd0a`, `tftp`).
pub fn devboot(bootdev: Dev, p: &mut [u8; BOOTDEVLEN]) {
    if bootdev == 0 {
        snprintf!(p, "tftp");
        return;
    }

    let mut q = 0;
    let mut put = |c: u8| {
        p[q] = c;
        q += 1;
    };
    if (bootdev & 0x100) != 0 {
        put(b'c');
        put(b'd');
        put(b'0');
    } else {
        put(if (bootdev & 0x80) != 0 { b'h' } else { b'f' });
        put(b'd');
        put(b'0' + (bootdev & 0x7f) as u8);
    }
    put(b'a');
    p[q] = 0;
}

/// `ttyname(fd)`: the console's name.
pub fn ttyname() -> [u8; 8] {
    let mut buf = [0u8; 8];
    if let Some(cn) = cn_tab() {
        let dev = cn.dev();
        snprintf!(
            &mut buf,
            "{}{}",
            CDEVS.get(major(dev) as usize).unwrap_or(&""),
            minor(dev)
        );
    }
    buf
}

/// `ttydev(name)`: the device of a console name (`com0`, `pc0`), `NODEV` if none.
pub fn ttydev(name: &[u8]) -> Dev {
    let name = &name[..name.iter().position(|&c| c == 0).unwrap_or(name.len())];
    let digits = name.iter().rev().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 || digits == name.len() {
        return NODEV;
    }
    let (prefix, num) = name.split_at(name.len() - digits);
    // the C reads the digits backwards: "12" gives unit 21
    let mut unit: i32 = -1;
    for &c in num.iter().rev() {
        unit = (if unit < 0 { 0 } else { unit * 10 }) + i32::from(c - b'0');
    }
    for (i, d) in CDEVS.iter().enumerate() {
        // strncmp(name, cdevs[i], no - name + 1)
        let d = d.as_bytes();
        if (0..prefix.len()).all(|k| d.get(k).copied().unwrap_or(0) == prefix[k]) {
            return makedev(i as u32, unit as u32);
        }
    }
    NODEV
}

/// `cnspeed(dev, sp)`.
pub fn cnspeed(dev: Dev, sp: i32) -> i32 {
    if major(dev) == 8 {
        // comN
        return comspeed(dev, sp);
    }

    // pc0 and anything else
    9600
}
