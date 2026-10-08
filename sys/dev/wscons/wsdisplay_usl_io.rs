/* $OpenBSD: wsdisplay_usl_io.h,v 1.4 2016/04/24 17:30:31 matthieu Exp $ */
/* $NetBSD: wsdisplay_usl_io.h,v 1.1 1998/06/11 22:00:04 drochner Exp $ */

/* <CODE> */
//! `<dev/wscons/wsdisplay_usl_io.h>`: the USL (System V) virtual terminal and keyboard
//! ioctls `wsdisplay_compat_usl.c` emulates (`VT_*`, `KD*`), as X servers and other programs
//! written for the PC console use them.
//!
//! Upstream: sys/dev/wscons/wsdisplay_usl_io.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - The header has no licence text; its `$OpenBSD$` and `$NetBSD$` lines are kept.
//! - The `_IO*` numbers are `sys/sys/ioccom.rs`'s `const fn`s; `vtmode_t` is
//!   [`VtMode`]; `unchar`/`ushort` are `u8`/`u16`.

use crate::machine::copy::AbiPod;
use crate::sys::ioccom::{_io, _ior, _iow, _iowr};

/// `struct vt_mode`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VtMode {
    /// `mode`: `VT_AUTO` or `VT_PROCESS`.
    pub mode: i8,
    /// `waitv`: not implemented yet SOS.
    pub waitv: i8,
    /// `relsig`.
    pub relsig: i16,
    /// `acqsig`.
    pub acqsig: i16,
    /// `frsig`: not implemented yet SOS.
    pub frsig: i16,
}

// SAFETY: two `char`s and three `short`s, 8 bytes without padding; any bytes are valid.
unsafe impl AbiPod for VtMode {}

/// `vtmode_t`.
pub type VtmodeT = VtMode;

/// `VT_OPENQRY`.
pub const VT_OPENQRY: u64 = _ior::<i32>(b'v', 1);
/// `VT_SETMODE`.
pub const VT_SETMODE: u64 = _iow::<VtmodeT>(b'v', 2);
/// `VT_GETMODE`.
pub const VT_GETMODE: u64 = _ior::<VtmodeT>(b'v', 3);

/// `VT_AUTO`: switching controlled by drvr.
pub const VT_AUTO: i8 = 0;
/// `VT_PROCESS`: switching controlled by prog.
pub const VT_PROCESS: i8 = 1;

/// `VT_RELDISP`.
pub const VT_RELDISP: u64 = _io(b'v', 4);
/// `VT_FALSE`: release of VT refused.
pub const VT_FALSE: i32 = 0;
/// `VT_TRUE`: VT released.
pub const VT_TRUE: i32 = 1;
/// `VT_ACKACQ`: acknowledging VT acquisition.
pub const VT_ACKACQ: i32 = 2;

/// `VT_ACTIVATE`.
pub const VT_ACTIVATE: u64 = _io(b'v', 5);
/// `VT_WAITACTIVE`.
pub const VT_WAITACTIVE: u64 = _io(b'v', 6);
/// `VT_GETACTIVE`.
pub const VT_GETACTIVE: u64 = _ior::<i32>(b'v', 7);

/// `struct vt_stat`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VtStat {
    /// `v_active`: active vt.
    pub v_active: u16,
    /// `v_signal`: signal to send.
    pub v_signal: u16,
    /// `v_state`: vt bitmask.
    pub v_state: u16,
}

// SAFETY: three `unsigned short`s, 6 bytes without padding; any bytes are valid.
unsafe impl AbiPod for VtStat {}

/// `VT_GETSTATE`.
pub const VT_GETSTATE: u64 = _ior::<VtStat>(b'v', 100);

/// `struct kbentry`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Kbentry {
    /// `kb_table`: which table to use.
    pub kb_table: u8,
    /// `kb_index`: which entry in table.
    pub kb_index: u8,
    /// `kb_value`: value to get/set in table.
    pub kb_value: u16,
}

// SAFETY: two bytes and an `unsigned short`, 4 bytes without padding; any bytes are valid.
unsafe impl AbiPod for Kbentry {}

/// `KDGETKBENT`.
pub const KDGETKBENT: u64 = _iowr::<Kbentry>(b'K', 4);

/// `KDGKBMODE`: get keyboard mode.
pub const KDGKBMODE: u64 = _ior::<i32>(b'K', 6);

/// `KDSKBMODE`: set keyboard mode (`_IO('K', 7 /*, int */)`).
pub const KDSKBMODE: u64 = _io(b'K', 7);
/// `K_RAW`: kbd switched to raw mode.
pub const K_RAW: i32 = 0;
/// `K_XLATE`: kbd switched to "normal" mode.
pub const K_XLATE: i32 = 1;

/// `KDMKTONE` (`_IO('K', 8 /*, int */)`).
pub const KDMKTONE: u64 = _io(b'K', 8);

/// `KDSETMODE` (`_IO('K', 10 /*, int */)`).
pub const KDSETMODE: u64 = _io(b'K', 10);
/// `KD_TEXT`: set text mode restore fonts.
pub const KD_TEXT: i32 = 0;
/// `KD_GRAPHICS`: set graphics mode.
pub const KD_GRAPHICS: i32 = 1;

/// `KDENABIO`: only allowed if euid == 0.
pub const KDENABIO: u64 = _io(b'K', 60);
/// `KDDISABIO`.
pub const KDDISABIO: u64 = _io(b'K', 61);

/// `KDGKBTYPE`.
pub const KDGKBTYPE: u64 = _ior::<u8>(b'K', 64);
/// `KB_84`.
pub const KB_84: i32 = 1;
/// `KB_101`.
pub const KB_101: i32 = 2;
/// `KB_OTHER`.
pub const KB_OTHER: i32 = 3;

/// `KDGETLED`.
pub const KDGETLED: u64 = _ior::<i32>(b'K', 65);
/// `KDSETLED` (`_IO('K', 66 /*, int */)`).
pub const KDSETLED: u64 = _io(b'K', 66);
/// `LED_CAP`.
pub const LED_CAP: i32 = 1;
/// `LED_NUM`.
pub const LED_NUM: i32 = 2;
/// `LED_SCR`.
pub const LED_SCR: i32 = 4;

/// `KDSETRAD` (`_IO('K', 67 /*, int */)`).
pub const KDSETRAD: u64 = _io(b'K', 67);

const _: () = {
    assert!(size_of::<VtMode>() == 8);
    assert!(size_of::<VtStat>() == 6);
    assert!(size_of::<Kbentry>() == 4);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ioctl_numbers() {
        // _IOR('v', 1, int), _IOW('v', 2, vtmode_t), _IO('v', 5), _IOR('v', 100, vt_stat)
        assert_eq!(VT_OPENQRY, 0x4004_7601);
        assert_eq!(VT_SETMODE, 0x8008_7602);
        assert_eq!(VT_ACTIVATE, 0x2000_7605);
        assert_eq!(VT_GETSTATE, 0x4006_7664);
        // _IOWR('K', 4, struct kbentry), _IOR('K', 64, char)
        assert_eq!(KDGETKBENT, 0xc004_4b04);
        assert_eq!(KDGKBTYPE, 0x4001_4b40);
    }

    /// Every plain constant against the C header.
    #[test]
    #[ignore = "needs OPENBSD_SRC"]
    fn constants_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/wscons/wsdisplay_usl_io.h");
        crate::reftest::assert_defines!(defs;
            VT_AUTO,
            VT_PROCESS,
            VT_FALSE,
            VT_TRUE,
            VT_ACKACQ,
            K_RAW,
            K_XLATE,
            KD_TEXT,
            KD_GRAPHICS,
            KB_84,
            KB_101,
            KB_OTHER,
            LED_CAP,
            LED_NUM,
            LED_SCR,
        );
    }
}
/* </TESTS> */
