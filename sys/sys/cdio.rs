/*	$OpenBSD: cdio.h,v 1.17 2017/10/24 09:36:13 jsg Exp $	*/
/*	$NetBSD: cdio.h,v 1.11 1996/02/19 18:29:04 scottr Exp $	*/
/* <LICENSES> */
/* </LICENSES> */

//! `<sys/cdio.h>`: the CD-ROM ioctls, shared between kernel and process. Only
//! `CDIOREADMSADDR` is here so far: `iso_mountfs` (`cd9660_vfsops.c`) asks the device for
//! the start of the last session with it. The rest of the header (the audio, table of
//! contents and sub-channel requests) comes with `cd(4)`.
//!
//! Upstream: sys/sys/cdio.h @ 3ce1f3f79392
//!
//! The original file carries no licence text, only its `$OpenBSD$` and `$NetBSD$` lines
//! (kept above); every notice in the reference tree is accepted (the user's rule of
//! 2026-10-04).
//!
//! ## Deviations
//! - Partial: `union msf_lba`, `struct cd_toc_entry`, the sub-channel, table of contents,
//!   play and volume requests and their ioctls come with `cd(4)`, which is not ported.

use crate::sys::ioccom::_iowr;

/// `CDIOREADMSADDR`: read LBA start of a given session; 0=last, others not yet supported.
pub const CDIOREADMSADDR: u64 = _iowr::<i32>(b'c', 6);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cdioreadmsaddr_is_an_inout_int_request() {
        // _IOWR('c', 6, int): IOC_INOUT | sizeof(int) << 16 | 'c' << 8 | 6.
        assert_eq!(CDIOREADMSADDR, 0xc004_6306);
    }
}
