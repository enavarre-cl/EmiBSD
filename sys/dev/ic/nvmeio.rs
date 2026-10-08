/* $OpenBSD: nvmeio.h,v 1.1 2024/05/24 12:04:07 krw Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 2023 Kenneth R Westerback <krw@openbsd.org>
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
//! `<dev/ic/nvmeio.h>`: the NVMe passthrough ioctl (`NVME_PASSTHROUGH_CMD`), through a
//! namespace's `sd(4)` raw device or bio(4).
//!
//! Upstream: sys/dev/ic/nvmeio.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - `caddr_t pt_status` and `pt_databuf` are user addresses, `usize`, as for the other
//!   ioctl structures (`docs/C_TO_RUST.md`); the holes of the LP64 layout are named
//!   `_pad*` members, the sizes and offsets asserted at compile time.

use core::mem::{offset_of, size_of};

use crate::dev::biovar::Bio;
use crate::sys::ioccom::_iowr;

/// `NVME_PASSTHROUGH_CMD`: `_IOWR('n', 0, struct nvme_pt_cmd)`.
pub const NVME_PASSTHROUGH_CMD: u64 = _iowr::<NvmePtCmd>(b'n', 0);

/// `struct nvme_pt_status`: what the passthrough copies out to `pt_status`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NvmePtStatus {
    /// `ps_dv_unit`.
    pub ps_dv_unit: i32,
    /// `ps_nsid`.
    pub ps_nsid: i32,
    /// `ps_flags`.
    pub ps_flags: i32,
    /// `ps_csts`.
    pub ps_csts: u32,
    /// `ps_cc`.
    pub ps_cc: u32,
}

impl NvmePtStatus {
    /// The structure's 20 bytes, in memory order.
    pub fn to_bytes(&self) -> [u8; size_of::<NvmePtStatus>()] {
        let mut b = [0u8; size_of::<NvmePtStatus>()];
        b[0..4].copy_from_slice(&self.ps_dv_unit.to_ne_bytes());
        b[4..8].copy_from_slice(&self.ps_nsid.to_ne_bytes());
        b[8..12].copy_from_slice(&self.ps_flags.to_ne_bytes());
        b[12..16].copy_from_slice(&self.ps_csts.to_ne_bytes());
        b[16..20].copy_from_slice(&self.ps_cc.to_ne_bytes());
        b
    }
}

/// `struct nvme_pt_cmd`: one admin command from user space.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct NvmePtCmd {
    /// `pt_bio`: commands may arrive via /dev/bio.
    pub pt_bio: Bio,

    // The sqe fields that the caller may specify.
    /// `pt_opcode`.
    pub pt_opcode: u8,
    /// The hole after `pt_opcode`.
    pub _pad0: [u8; 3],
    /// `pt_nsid`.
    pub pt_nsid: u32,
    /// `pt_cdw10`.
    pub pt_cdw10: u32,
    /// `pt_cdw11`.
    pub pt_cdw11: u32,
    /// `pt_cdw12`.
    pub pt_cdw12: u32,
    /// `pt_cdw13`.
    pub pt_cdw13: u32,
    /// `pt_cdw14`.
    pub pt_cdw14: u32,
    /// `pt_cdw15`.
    pub pt_cdw15: u32,

    /// `pt_status`: user space address of a `struct nvme_pt_status`.
    pub pt_status: usize,
    /// `pt_statuslen`.
    pub pt_statuslen: u32,
    /// The hole after `pt_statuslen`.
    pub _pad1: u32,

    /// `pt_databuf`: user space address.
    pub pt_databuf: usize,
    /// `pt_databuflen`: length of buffer.
    pub pt_databuflen: u32,
    /// The hole after `pt_databuflen`.
    pub _pad2: u32,
}

#[cfg(target_pointer_width = "64")]
const _: () = {
    assert!(size_of::<Bio>() == 696);
    assert!(offset_of!(NvmePtCmd, pt_opcode) == 696);
    assert!(offset_of!(NvmePtCmd, pt_nsid) == 700);
    assert!(offset_of!(NvmePtCmd, pt_cdw15) == 724);
    assert!(offset_of!(NvmePtCmd, pt_status) == 728);
    assert!(offset_of!(NvmePtCmd, pt_statuslen) == 736);
    assert!(offset_of!(NvmePtCmd, pt_databuf) == 744);
    assert!(offset_of!(NvmePtCmd, pt_databuflen) == 752);
    assert!(size_of::<NvmePtCmd>() == 760);
    assert!(size_of::<NvmePtStatus>() == 20);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passthrough_ioctl_number() {
        // _IOWR('n', 0, 760 bytes): IOC_INOUT | 760 << 16 | 'n' << 8 | 0.
        assert_eq!(
            NVME_PASSTHROUGH_CMD,
            0xc000_0000 | (760 << 16) | (0x6e << 8)
        );
    }

    #[test]
    fn status_bytes_are_in_member_order() {
        let s = NvmePtStatus {
            ps_dv_unit: 1,
            ps_nsid: 2,
            ps_flags: 3,
            ps_csts: 4,
            ps_cc: 5,
        };
        let b = s.to_bytes();
        assert_eq!(b[0], 1);
        assert_eq!(b[4], 2);
        assert_eq!(b[16], 5);
    }
}
/* </TESTS> */
