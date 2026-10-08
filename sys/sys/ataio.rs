/*	$OpenBSD: ataio.h,v 1.5 2003/09/26 21:43:32 miod Exp $	*/
/*	$NetBSD: ataio.h,v 1.2 1998/11/23 22:58:23 kenh Exp $	*/
/* <LICENSES> */
/* </LICENSES> */

/* <CODE> */
//! `<sys/ataio.h>`: the ATA pass-through ioctls (`ATAIOCCOMMAND`, `ATAIOGETTRACE`), which
//! `scsi_ioctl.c` turns into SCSI ATA PASS-THROUGH(12) commands.
//!
//! Upstream: sys/sys/ataio.h @ 3ce1f3f79392
//!
//! The original file carries no licence text, only its `$OpenBSD$` and `$NetBSD$` lines
//! (kept above); every notice in the reference tree is accepted (the user's rule of
//! 2026-10-04).
//!
//! ## Deviations
//! - The structures are `#[repr(C)]` ABI types with LP64 sizes and offsets checked at
//!   compile time; the C compiler's holes are the named `_pad*` fields, so that every byte
//!   is a field and the structures can be read and written as plain bytes ([`AbiPod`]).
//! - `caddr_t databuf` and `void *buf` are `usize`: user addresses, only handed to
//!   `copyin`/`copyout` (`docs/C_TO_RUST.md`).
//! - `u_long` is `u64`, `u_short` `u16`, `u_char` `u8`; the `ATACMD_*` request flags are `u64`
//!   (they fill `flags`), its return statuses `u8` (they fill `retsts`).

use core::mem::{offset_of, size_of};

use crate::machine::copy::AbiPod;
use crate::sys::ioccom::_iowr;

/// `atareq_t` (`struct atareq`): a raw ATA command from user space (`ATAIOCCOMMAND`).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Atareq {
    /// `flags`: info about the request status and type (`ATACMD_*`).
    pub flags: u64,
    /// `command`: command code.
    pub command: u8,
    /// `features`: feature modifier bits for the command.
    pub features: u8,
    /// `sec_count`: sector count.
    pub sec_count: u8,
    /// `sec_num`: sector number.
    pub sec_num: u8,
    /// `head`: head number.
    pub head: u8,
    /// The C compiler's padding before `cylinder`.
    pub _pad0: u8,
    /// `cylinder`: cylinder/LBA address.
    pub cylinder: u16,
    /// `databuf`: user address of the I/O data buffer.
    pub databuf: usize,
    /// `datalen`: length of the data buffer.
    pub datalen: u64,
    /// `timeout`: command timeout.
    pub timeout: i32,
    /// `retsts`: return status for the command (`ATACMD_OK` ...).
    pub retsts: u8,
    /// `error`: error bits.
    pub error: u8,
    /// The C compiler's padding at the end.
    pub _pad1: [u8; 2],
}

// SAFETY: `#[repr(C)]`, integers only, the holes filled by `_pad0`/`_pad1` (checked below):
// every byte is initialised and every pattern is valid.
unsafe impl AbiPod for Atareq {}

// bit definitions for flags

/// `ATACMD_READ`: data comes in from the device.
pub const ATACMD_READ: u64 = 0x0000_0001;
/// `ATACMD_WRITE`: data goes out to the device.
pub const ATACMD_WRITE: u64 = 0x0000_0002;
/// `ATACMD_READREG`: read the registers back.
pub const ATACMD_READREG: u64 = 0x0000_0004;

// definitions for the return status (retsts)

/// `ATACMD_OK`.
pub const ATACMD_OK: u8 = 0x00;
/// `ATACMD_TIMEOUT`.
pub const ATACMD_TIMEOUT: u8 = 0x01;
/// `ATACMD_ERROR`.
pub const ATACMD_ERROR: u8 = 0x02;
/// `ATACMD_DF`: device fault.
pub const ATACMD_DF: u8 = 0x03;

/// `ATAIOCCOMMAND`: run a raw ATA command.
pub const ATAIOCCOMMAND: u64 = _iowr::<Atareq>(b'Q', 8);

/// `atagettrace_t` (`struct atagettrace`): a request for the ATA trace buffer.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Atagettrace {
    /// `buf_size`: length of the data buffer.
    pub buf_size: u32,
    /// The C compiler's padding before `buf`.
    pub _pad0: u32,
    /// `buf`: user address of the data buffer.
    pub buf: usize,
    /// `bytes_copied`: number of bytes copied to the buffer.
    pub bytes_copied: u32,
    /// `bytes_left`: number of bytes left.
    pub bytes_left: u32,
}

// SAFETY: `#[repr(C)]`, integers only, the hole filled by `_pad0` (checked below).
unsafe impl AbiPod for Atagettrace {}

/// `ATAIOGETTRACE`: read the ATA trace buffer.
pub const ATAIOGETTRACE: u64 = _iowr::<Atagettrace>(b'Q', 27);

const _: () = {
    assert!(size_of::<Atareq>() == 40);
    assert!(offset_of!(Atareq, cylinder) == 14);
    assert!(offset_of!(Atareq, databuf) == 16);
    assert!(offset_of!(Atareq, timeout) == 32);
    assert!(offset_of!(Atareq, error) == 37);
    assert!(size_of::<Atagettrace>() == 24);
    assert!(offset_of!(Atagettrace, buf) == 8);
    assert!(offset_of!(Atagettrace, bytes_left) == 20);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_have_openbsd_values() {
        // The values the C compiler computes on LP64 for _IOWR('Q', 8, atareq_t) and
        // _IOWR('Q', 27, struct atagettrace).
        assert_eq!(ATAIOCCOMMAND, 0xc028_5108);
        assert_eq!(ATAIOGETTRACE, 0xc018_511b);
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/sys/ataio.h");
        let ours = crate::reftest::assert_defines!(defs;
            ATACMD_READ, ATACMD_WRITE, ATACMD_READREG, ATACMD_OK, ATACMD_TIMEOUT,
            ATACMD_ERROR, ATACMD_DF,
        );
        crate::reftest::assert_complete(&defs, "ATACMD_", &ours);
    }
}
/* </TESTS> */
