/* <LICENSES> */
/*
 * Copyright (c) 1988 University of Utah.
 * Copyright (c) 1990, 1993
 *	The Regents of the University of California.  All rights reserved.
 *
 * This code is derived from software contributed to Berkeley by
 * the Systems Programming Group of the University of Utah Computer
 * Science Department.
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
 * from: Utah $Hdr: fdioctl.h 1.1 90/07/09$
 *
 *	@(#)vnioctl.h	8.1 (Berkeley) 6/10/93
 */
/* </LICENSES> */

/* <CODE> */
//! `<dev/vndioctl.h>`: the ioctls of vnd(4), the vnode disk: configure a unit over a file
//! (`VNDIOCSET`), unconfigure it (`VNDIOCCLR`) and ask what a unit covers (`VNDIOCGET`), as
//! vnconfig(8) and mount_vnd(8) issue them on `/dev/rvndNc`.
//!
//! Upstream: sys/dev/vndioctl.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - The structures are `#[repr(C)]` ABI types with LP64 sizes and offsets checked at
//!   compile time. `struct vnd_ioctl`'s two bytes of tail padding after `vnd_type` are the
//!   named `_pad0`, so that every byte is a field and the structure can be read and written
//!   as plain bytes ([`AbiPod`]).
//! - `char *vnd_file` and `u_char *vnd_key` are `usize`s: user addresses, only handed to
//!   `copyinstr`/`copyin` (`docs/C_TO_RUST.md`). `size_t` is `usize`, `off_t` [`Off`],
//!   `dev_t` [`Dev`], `ino_t` [`Ino`].

use core::mem::{offset_of, size_of};

use crate::machine::copy::AbiPod;
use crate::sys::ioccom::{_iow, _iowr};
use crate::sys::types::{Dev, Ino, Off};

/// `VNDNLEN`: the longest path a unit records (`PATH_MAX`).
pub const VNDNLEN: usize = 1024;

/// `struct vnd_ioctl`: ioctl definitions for file (vnode) disk pseudo-device.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VndIoctl {
    /// `vnd_file`: pathname of file to mount (a user address).
    pub vnd_file: usize,
    /// `vnd_secsize`: sector size in bytes.
    pub vnd_secsize: usize,
    /// `vnd_nsectors`: number of sectors in a track.
    pub vnd_nsectors: usize,
    /// `vnd_ntracks`: number of tracks per cylinder.
    pub vnd_ntracks: usize,
    /// `vnd_size`: (returned) size of disk.
    pub vnd_size: Off,
    /// `vnd_key`: the Blowfish key (a user address).
    pub vnd_key: usize,
    /// `vnd_keylen`: the key's length in bytes; 0 for no encryption.
    pub vnd_keylen: i32,
    /// `vnd_type`: DTYPE being emulated.
    pub vnd_type: u16,
    /// The two bytes of padding the C compiler puts at the end.
    pub _pad0: [u8; 2],
}

// SAFETY: `#[repr(C)]`, integers only, the tail padding a named field (size checked below):
// every byte is initialised and any pattern is a valid value.
unsafe impl AbiPod for VndIoctl {}

/// `struct vnd_user`: a simple structure used by userland to query about a specific vnd.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VndUser {
    /// `vnu_file`: vnd file.
    pub vnu_file: [u8; VNDNLEN],
    /// `vnu_unit`: vnd unit.
    pub vnu_unit: i32,
    /// `vnu_dev`: vnd device.
    pub vnu_dev: Dev,
    /// `vnu_ino`: vnd inode.
    pub vnu_ino: Ino,
}

impl VndUser {
    /// All zeros.
    pub const fn zeroed() -> Self {
        Self {
            vnu_file: [0; VNDNLEN],
            vnu_unit: 0,
            vnu_dev: 0,
            vnu_ino: 0,
        }
    }
}

impl Default for VndUser {
    fn default() -> Self {
        Self::zeroed()
    }
}

// SAFETY: `#[repr(C)]`, integers and a byte array with no padding (size checked below):
// every byte is initialised and any pattern is a valid value.
unsafe impl AbiPod for VndUser {}

// Before you can use a unit, it must be configured with VNDIOCSET. The configuration
// persists across opens and closes of the device; an VNDIOCCLR must be used to reset a
// configuration. An attempt to VNDIOCSET an already active unit will return EBUSY.

/// `VNDIOCSET`: enable disk.
pub const VNDIOCSET: u64 = _iowr::<VndIoctl>(b'F', 0);
/// `VNDIOCCLR`: disable disk.
pub const VNDIOCCLR: u64 = _iow::<VndIoctl>(b'F', 1);
/// `VNDIOCGET`: get disk info.
pub const VNDIOCGET: u64 = _iowr::<VndUser>(b'F', 3);

const _: () = {
    assert!(size_of::<VndIoctl>() == 56);
    assert!(offset_of!(VndIoctl, vnd_size) == 32);
    assert!(offset_of!(VndIoctl, vnd_key) == 40);
    assert!(offset_of!(VndIoctl, vnd_keylen) == 48);
    assert!(offset_of!(VndIoctl, vnd_type) == 52);
    assert!(size_of::<VndUser>() == 1040);
    assert!(offset_of!(VndUser, vnu_unit) == 1024);
    assert!(offset_of!(VndUser, vnu_dev) == 1028);
    assert!(offset_of!(VndUser, vnu_ino) == 1032);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;
    use crate::sys::ioccom::{IOC_IN, IOC_INOUT, IOCPARM_MASK};

    #[test]
    fn commands_have_openbsd_values() {
        // The values the C compiler computes on LP64 for _IOWR('F', 0, struct vnd_ioctl),
        // _IOW('F', 1, struct vnd_ioctl) and _IOWR('F', 3, struct vnd_user).
        assert_eq!(VNDIOCSET, 0xc038_4600);
        assert_eq!(VNDIOCCLR, 0x8038_4601);
        assert_eq!(VNDIOCGET, 0xc410_4603);
    }

    /// Every `#define VNDIOC... _IO*('F', n, type)` of the C: the direction, group, number
    /// and argument type it names must give our value.
    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/vndioctl.h");
        assert_eq!(crate::reftest::int(&defs, "VNDNLEN"), Some(VNDNLEN as i64));
        let ours = [
            ("VNDIOCSET", VNDIOCSET),
            ("VNDIOCCLR", VNDIOCCLR),
            ("VNDIOCGET", VNDIOCGET),
        ];
        let mut seen = 0;
        for (name, text) in defs.iter().filter(|(n, _)| n.starts_with("VNDIOC")) {
            let (mac, args) = text.split_once('(').expect(name);
            let args = args.trim_end().strip_suffix(')').expect(name);
            let parts: std::vec::Vec<&str> = args.split(',').map(str::trim).collect();
            let dir = match mac.trim() {
                "_IOW" => IOC_IN,
                "_IOWR" => IOC_INOUT,
                m => panic!("{name}: {m}"),
            };
            let group = u64::from(parts[0].as_bytes()[1]);
            let num: u64 = parts[1].parse().expect(name);
            let len = match parts[2] {
                "struct vnd_ioctl" => size_of::<VndIoctl>(),
                "struct vnd_user" => size_of::<VndUser>(),
                t => panic!("{name}: unknown argument type {t}"),
            } as u64;
            let want = dir | ((len & IOCPARM_MASK) << 16) | (group << 8) | num;
            let Some((_, value)) = ours.iter().find(|(n, _)| n == name) else {
                panic!("{name} is not ported");
            };
            assert_eq!(*value, want, "{name}");
            seen += 1;
        }
        assert_eq!(seen, ours.len());
    }
}
/* </TESTS> */
