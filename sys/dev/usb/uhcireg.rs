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
 * Copyright (c) 1998 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Lennart Augustsson (lennart@augustsson.net) at
 * Carlstedt Research & Technology.
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
 * THIS SOFTWARE IS PROVIDED BY THE NETBSD FOUNDATION, INC. AND CONTRIBUTORS
 * ``AS IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
 * TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
 * PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL THE FOUNDATION OR CONTRIBUTORS
 * BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
 * CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
 * SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
 * INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
 * CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
 * ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
 * POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! The UHCI registers and the structures the controller shares with the driver in memory:
//! `<dev/usb/uhcireg.h>`.
//!
//! Upstream: sys/dev/usb/uhcireg.h @ 3ce1f3f79392
//!
//! A UHCI controller has a small block of 16-bit registers in I/O space (PCI BAR 4,
//! `PCI_CBIO`): command, status, interrupt enable, frame number, the frame list's base, the
//! start-of-frame modifier and the two root port status registers. Everything else is in DMA
//! memory, in the controller's little-endian layout: the frame list of
//! `UHCI_FRAMELIST_COUNT` links, the transfer descriptors (`struct uhci_td`) and the queue
//! heads (`struct uhci_qh`). A link is a 32-bit bus address with flags in its low bits
//! (`UHCI_PTR_*`). Once a TD is reachable from the frame list the controller owns its
//! `td_status`, and it owns a QH's `qh_elink`: the CPU does not write them then.
//!
//! ## Deviations
//! - Register offsets are `usize` (`bus_size_t`); the bits of the 16-bit registers are
//!   `u16`, `UHCI_SOF_MASK` `u8`; links and descriptor fields `u32`; the PCI configuration
//!   register numbers `i32` (`pci_conf_read`'s `int reg`) and their values `u32` (`pcireg_t`).
//! - Function-like macros (`URWMASK(x)`, `UHCI_TD_GET_ACTLEN(s)`, `UHCI_TD_IN(len, endp,
//!   dev, dt)`, ...) are `const fn`s with the macro's name in lower case
//!   (`docs/C_TO_RUST.md`). `UHCI_TD_SET_MAXLEN(l)` takes the length as `u32` and wraps as the
//!   C's `(uint32_t)(l) - 1` does for a zero length (the null packet's `0x7ff`).
//! - The descriptors are `#[repr(C)]` structures of `Cell<u32>` with the C's sizes pinned by
//!   compile-time checks. The driver keeps its software state next to them in the same DMA
//!   chunk (`uhcivar.rs`), so it reaches them as `&'static`, and every access to a member is
//!   one volatile load or store through [`uhci_get`] and [`uhci_set`], which also do the
//!   `letoh32`/`htole32` conversions (`docs/C_TO_RUST.md`, a structure a chip and the driver
//!   share in DMA memory).

use core::cell::Cell;
use core::ptr;

use crate::machine::bus::BusSize;

/*** PCI config registers ***/

/// `PCI_USBREV`: USB protocol revision.
pub const PCI_USBREV: i32 = 0x60;
/// `PCI_USBREV_MASK`.
pub const PCI_USBREV_MASK: u32 = 0xff;
/// `PCI_USBREV_PRE_1_0`.
pub const PCI_USBREV_PRE_1_0: u32 = 0x00;
/// `PCI_USBREV_1_0`.
pub const PCI_USBREV_1_0: u32 = 0x10;
/// `PCI_USBREV_1_1`.
pub const PCI_USBREV_1_1: u32 = 0x11;

/// `PCI_LEGSUP`: Legacy Support register.
pub const PCI_LEGSUP: i32 = 0xc0;
/// `PCI_LEGSUP_USBPIRQDEN`: USB PIRQ D Enable.
pub const PCI_LEGSUP_USBPIRQDEN: u32 = 0x2000;

/// `PCI_CBIO`: configuration base IO.
pub const PCI_CBIO: i32 = 0x20;

/// `PCI_INTERFACE_UHCI`.
pub const PCI_INTERFACE_UHCI: u32 = 0x00;

/*** UHCI registers ***/

/// `UHCI_CMD`.
pub const UHCI_CMD: BusSize = 0x00;
/// `UHCI_CMD_RS`.
pub const UHCI_CMD_RS: u16 = 0x0001;
/// `UHCI_CMD_HCRESET`.
pub const UHCI_CMD_HCRESET: u16 = 0x0002;
/// `UHCI_CMD_GRESET`.
pub const UHCI_CMD_GRESET: u16 = 0x0004;
/// `UHCI_CMD_EGSM`.
pub const UHCI_CMD_EGSM: u16 = 0x0008;
/// `UHCI_CMD_FGR`.
pub const UHCI_CMD_FGR: u16 = 0x0010;
/// `UHCI_CMD_SWDBG`.
pub const UHCI_CMD_SWDBG: u16 = 0x0020;
/// `UHCI_CMD_CF`.
pub const UHCI_CMD_CF: u16 = 0x0040;
/// `UHCI_CMD_MAXP`.
pub const UHCI_CMD_MAXP: u16 = 0x0080;

/// `UHCI_STS`.
pub const UHCI_STS: BusSize = 0x02;
/// `UHCI_STS_USBINT`.
pub const UHCI_STS_USBINT: u16 = 0x0001;
/// `UHCI_STS_USBEI`.
pub const UHCI_STS_USBEI: u16 = 0x0002;
/// `UHCI_STS_RD`.
pub const UHCI_STS_RD: u16 = 0x0004;
/// `UHCI_STS_HSE`.
pub const UHCI_STS_HSE: u16 = 0x0008;
/// `UHCI_STS_HCPE`.
pub const UHCI_STS_HCPE: u16 = 0x0010;
/// `UHCI_STS_HCH`.
pub const UHCI_STS_HCH: u16 = 0x0020;
/// `UHCI_STS_ALLINTRS`.
pub const UHCI_STS_ALLINTRS: u16 = 0x003f;

/// `UHCI_INTR`.
pub const UHCI_INTR: BusSize = 0x04;
/// `UHCI_INTR_TOCRCIE`.
pub const UHCI_INTR_TOCRCIE: u16 = 0x0001;
/// `UHCI_INTR_RIE`.
pub const UHCI_INTR_RIE: u16 = 0x0002;
/// `UHCI_INTR_IOCE`.
pub const UHCI_INTR_IOCE: u16 = 0x0004;
/// `UHCI_INTR_SPIE`.
pub const UHCI_INTR_SPIE: u16 = 0x0008;

/// `UHCI_FRNUM`.
pub const UHCI_FRNUM: BusSize = 0x06;
/// `UHCI_FRNUM_MASK`.
pub const UHCI_FRNUM_MASK: u16 = 0x03ff;

/// `UHCI_FLBASEADDR`.
pub const UHCI_FLBASEADDR: BusSize = 0x08;

/// `UHCI_SOF`.
pub const UHCI_SOF: BusSize = 0x0c;
/// `UHCI_SOF_MASK`.
pub const UHCI_SOF_MASK: u8 = 0x7f;

/// `UHCI_PORTSC1`.
pub const UHCI_PORTSC1: BusSize = 0x010;
/// `UHCI_PORTSC2`.
pub const UHCI_PORTSC2: BusSize = 0x012;
/// `UHCI_PORTSC_CCS`.
pub const UHCI_PORTSC_CCS: u16 = 0x0001;
/// `UHCI_PORTSC_CSC`.
pub const UHCI_PORTSC_CSC: u16 = 0x0002;
/// `UHCI_PORTSC_PE`.
pub const UHCI_PORTSC_PE: u16 = 0x0004;
/// `UHCI_PORTSC_POEDC`.
pub const UHCI_PORTSC_POEDC: u16 = 0x0008;
/// `UHCI_PORTSC_LS`.
pub const UHCI_PORTSC_LS: u16 = 0x0030;
/// `UHCI_PORTSC_LS_SHIFT`.
pub const UHCI_PORTSC_LS_SHIFT: u32 = 4;
/// `UHCI_PORTSC_RD`.
pub const UHCI_PORTSC_RD: u16 = 0x0040;
/// `UHCI_PORTSC_LSDA`.
pub const UHCI_PORTSC_LSDA: u16 = 0x0100;
/// `UHCI_PORTSC_PR`.
pub const UHCI_PORTSC_PR: u16 = 0x0200;
/// `UHCI_PORTSC_OCI`.
pub const UHCI_PORTSC_OCI: u16 = 0x0400;
/// `UHCI_PORTSC_OCIC`.
pub const UHCI_PORTSC_OCIC: u16 = 0x0800;
/// `UHCI_PORTSC_SUSP`.
pub const UHCI_PORTSC_SUSP: u16 = 0x1000;

/// `URWMASK(x)`: the read/write bits of a port status register (writing the others back
/// would clear their change bits).
pub const fn urwmask(x: u16) -> u16 {
    x & (UHCI_PORTSC_SUSP | UHCI_PORTSC_PR | UHCI_PORTSC_RD | UHCI_PORTSC_PE)
}

/// `UHCI_FRAMELIST_COUNT`.
pub const UHCI_FRAMELIST_COUNT: usize = 1024;
/// `UHCI_FRAMELIST_ALIGN`.
pub const UHCI_FRAMELIST_ALIGN: usize = 4096;

/// `UHCI_TD_ALIGN`.
pub const UHCI_TD_ALIGN: usize = 16;
/// `UHCI_QH_ALIGN`.
pub const UHCI_QH_ALIGN: usize = 16;

/// `uhci_physaddr_t`.
pub type UhciPhysaddrT = u32;
/// `UHCI_PTR_T`: terminate.
pub const UHCI_PTR_T: u32 = 0x00000001;
/// `UHCI_PTR_TD`.
pub const UHCI_PTR_TD: u32 = 0x00000000;
/// `UHCI_PTR_QH`.
pub const UHCI_PTR_QH: u32 = 0x00000002;
/// `UHCI_PTR_VF`: depth first.
pub const UHCI_PTR_VF: u32 = 0x00000004;

/// `UHCI_QH_REMOVE_DELAY`: wait this long (in microseconds) after a QH has been removed.
/// This gives that HC a chance to stop looking at it before it's recycled.
pub const UHCI_QH_REMOVE_DELAY: u32 = 5;

/// `struct uhci_td`: a transfer descriptor.
///
/// The Queue Heads and Transfer Descriptors are accessed by both the CPU and the USB
/// controller which run concurrently. This means that they have to be accessed with great
/// care. As long as the data structures are not linked into the controller's frame list they
/// cannot be accessed by it and anything goes. As soon as a TD is accessible by the
/// controller it "owns" the `td_status` field; it will not be written by the CPU. Similarly
/// the controller "owns" the `qh_elink` field.
#[repr(C)]
pub struct UhciTd {
    /// `td_link`.
    pub td_link: Cell<UhciPhysaddrT>,
    /// `td_status`.
    pub td_status: Cell<u32>,
    /// `td_token`.
    pub td_token: Cell<u32>,
    /// `td_buffer`.
    pub td_buffer: Cell<u32>,
}

/// `UHCI_TD_GET_ACTLEN(s)`.
pub const fn uhci_td_get_actlen(s: u32) -> u32 {
    s.wrapping_add(1) & 0x3ff
}

/// `UHCI_TD_ZERO_ACTLEN(t)`.
pub const fn uhci_td_zero_actlen(t: u32) -> u32 {
    t | 0x3ff
}

/// `UHCI_TD_BITSTUFF`.
pub const UHCI_TD_BITSTUFF: u32 = 0x00020000;
/// `UHCI_TD_CRCTO`.
pub const UHCI_TD_CRCTO: u32 = 0x00040000;
/// `UHCI_TD_NAK`.
pub const UHCI_TD_NAK: u32 = 0x00080000;
/// `UHCI_TD_BABBLE`.
pub const UHCI_TD_BABBLE: u32 = 0x00100000;
/// `UHCI_TD_DBUFFER`.
pub const UHCI_TD_DBUFFER: u32 = 0x00200000;
/// `UHCI_TD_STALLED`.
pub const UHCI_TD_STALLED: u32 = 0x00400000;
/// `UHCI_TD_ACTIVE`.
pub const UHCI_TD_ACTIVE: u32 = 0x00800000;
/// `UHCI_TD_IOC`.
pub const UHCI_TD_IOC: u32 = 0x01000000;
/// `UHCI_TD_IOS`.
pub const UHCI_TD_IOS: u32 = 0x02000000;
/// `UHCI_TD_LS`.
pub const UHCI_TD_LS: u32 = 0x04000000;

/// `UHCI_TD_GET_ERRCNT(s)`.
pub const fn uhci_td_get_errcnt(s: u32) -> u32 {
    (s >> 27) & 3
}

/// `UHCI_TD_SET_ERRCNT(n)`.
pub const fn uhci_td_set_errcnt(n: u32) -> u32 {
    n << 27
}

/// `UHCI_TD_SPD`.
pub const UHCI_TD_SPD: u32 = 0x20000000;

/// `UHCI_TD_PID_IN`.
pub const UHCI_TD_PID_IN: u32 = 0x00000069;
/// `UHCI_TD_PID_OUT`.
pub const UHCI_TD_PID_OUT: u32 = 0x000000e1;
/// `UHCI_TD_PID_SETUP`.
pub const UHCI_TD_PID_SETUP: u32 = 0x0000002d;

/// `UHCI_TD_GET_PID(s)`.
pub const fn uhci_td_get_pid(s: u32) -> u32 {
    s & 0xff
}

/// `UHCI_TD_SET_DEVADDR(a)`.
pub const fn uhci_td_set_devaddr(a: u32) -> u32 {
    a << 8
}

/// `UHCI_TD_GET_DEVADDR(s)`.
pub const fn uhci_td_get_devaddr(s: u32) -> u32 {
    (s >> 8) & 0x7f
}

/// `UHCI_TD_SET_ENDPT(e)`.
pub const fn uhci_td_set_endpt(e: u32) -> u32 {
    (e & 0xf) << 15
}

/// `UHCI_TD_GET_ENDPT(s)`.
pub const fn uhci_td_get_endpt(s: u32) -> u32 {
    (s >> 15) & 0xf
}

/// `UHCI_TD_SET_DT(t)`.
pub const fn uhci_td_set_dt(t: u32) -> u32 {
    t << 19
}

/// `UHCI_TD_GET_DT(s)`.
pub const fn uhci_td_get_dt(s: u32) -> u32 {
    (s >> 19) & 1
}

/// `UHCI_TD_SET_MAXLEN(l)`: the length minus one; a zero length (the null packet) is
/// `0x7ff` once masked by the controller.
pub const fn uhci_td_set_maxlen(l: u32) -> u32 {
    l.wrapping_sub(1) << 21
}

/// `UHCI_TD_GET_MAXLEN(s)`.
pub const fn uhci_td_get_maxlen(s: u32) -> u32 {
    ((s >> 21) + 1) & 0x7ff
}

/// `UHCI_TD_MAXLEN_MASK`.
pub const UHCI_TD_MAXLEN_MASK: u32 = 0xffe00000;

/// `UHCI_TD_ERROR`.
pub const UHCI_TD_ERROR: u32 =
    UHCI_TD_BITSTUFF | UHCI_TD_CRCTO | UHCI_TD_BABBLE | UHCI_TD_DBUFFER | UHCI_TD_STALLED;

/// `UHCI_TD_SETUP(len, endp, dev)`: a SETUP token.
pub const fn uhci_td_setup(len: u32, endp: u32, dev: u32) -> u32 {
    uhci_td_set_maxlen(len) | uhci_td_set_endpt(endp) | uhci_td_set_devaddr(dev) | UHCI_TD_PID_SETUP
}

/// `UHCI_TD_OUT(len, endp, dev, dt)`: an OUT token.
pub const fn uhci_td_out(len: u32, endp: u32, dev: u32, dt: u32) -> u32 {
    uhci_td_set_maxlen(len)
        | uhci_td_set_endpt(endp)
        | uhci_td_set_devaddr(dev)
        | UHCI_TD_PID_OUT
        | uhci_td_set_dt(dt)
}

/// `UHCI_TD_IN(len, endp, dev, dt)`: an IN token.
pub const fn uhci_td_in(len: u32, endp: u32, dev: u32, dt: u32) -> u32 {
    uhci_td_set_maxlen(len)
        | uhci_td_set_endpt(endp)
        | uhci_td_set_devaddr(dev)
        | UHCI_TD_PID_IN
        | uhci_td_set_dt(dt)
}

/// `struct uhci_qh`: a queue head.
#[repr(C)]
pub struct UhciQh {
    /// `qh_hlink`: the next QH (horizontal).
    pub qh_hlink: Cell<UhciPhysaddrT>,
    /// `qh_elink`: the first element (vertical), owned by the controller once linked.
    pub qh_elink: Cell<UhciPhysaddrT>,
}

/// `letoh32(member)`: one volatile load of a descriptor member in DMA memory, converted from
/// the controller's little-endian layout.
#[inline]
pub fn uhci_get(c: &Cell<u32>) -> u32 {
    // SAFETY: the cell's own four bytes, aligned (`#[repr(C)]` at a 16-byte aligned address);
    // the controller writes the memory, so the load is volatile; any bits are a `u32`.
    u32::from_le(unsafe { ptr::read_volatile(c.as_ptr()) })
}

/// `member = htole32(v)`: one volatile store of a descriptor member in DMA memory.
#[inline]
pub fn uhci_set(c: &Cell<u32>, v: u32) {
    // SAFETY: the cell's own four bytes, aligned; a `Cell` may be written through a shared
    // reference, and the controller reads the memory, so the store is volatile.
    unsafe { ptr::write_volatile(c.as_ptr(), v.to_le()) }
}

const _: () = {
    assert!(size_of::<UhciTd>() == 16);
    assert!(size_of::<UhciQh>() == 8);
    assert!(core::mem::offset_of!(UhciTd, td_buffer) == 12);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_rw_mask() {
        // The change bits (CSC, POEDC, OCIC) and the read-only ones never come back.
        let x = UHCI_PORTSC_CCS
            | UHCI_PORTSC_CSC
            | UHCI_PORTSC_PE
            | UHCI_PORTSC_POEDC
            | UHCI_PORTSC_LSDA
            | UHCI_PORTSC_OCIC
            | UHCI_PORTSC_SUSP;
        assert_eq!(urwmask(x), UHCI_PORTSC_PE | UHCI_PORTSC_SUSP);
        assert_eq!(urwmask(0xffff), 0x1244);
    }

    #[test]
    fn td_status_fields() {
        let st = uhci_td_zero_actlen(uhci_td_set_errcnt(3) | UHCI_TD_ACTIVE);
        assert_eq!(st, 0x1880_03ff);
        assert_eq!(uhci_td_get_errcnt(st), 3);
        // A fresh TD reports zero bytes: the field is the length minus one.
        assert_eq!(uhci_td_get_actlen(st), 0);
        assert_eq!(uhci_td_get_actlen(0x0000_003f), 64);
        assert_eq!(uhci_td_get_actlen(0x0000_0007), 8);
        assert_eq!(UHCI_TD_ERROR & UHCI_TD_NAK, 0);
        assert_eq!(UHCI_TD_ERROR & UHCI_TD_STALLED, UHCI_TD_STALLED);
    }

    #[test]
    fn td_tokens() {
        // SETUP to device 5, endpoint 0: 8 bytes.
        let t = uhci_td_setup(8, 0, 5);
        assert_eq!(uhci_td_get_pid(t), UHCI_TD_PID_SETUP);
        assert_eq!(uhci_td_get_devaddr(t), 5);
        assert_eq!(uhci_td_get_endpt(t), 0);
        assert_eq!(uhci_td_get_maxlen(t), 8);
        assert_eq!(t, 0x00e0_052d);
        // IN from endpoint 0x81 (the direction bit falls outside the 4-bit field), toggle 1.
        let t = uhci_td_in(64, 0x81, 2, 1);
        assert_eq!(uhci_td_get_pid(t), UHCI_TD_PID_IN);
        assert_eq!(uhci_td_get_endpt(t), 1);
        assert_eq!(uhci_td_get_dt(t), 1);
        assert_eq!(uhci_td_get_maxlen(t), 64);
        // The null data packet of a status stage: maxlen 0x7ff, read back as 0.
        let t = uhci_td_out(0, 0, 3, 1);
        assert_eq!(t & UHCI_TD_MAXLEN_MASK, UHCI_TD_MAXLEN_MASK);
        assert_eq!(uhci_td_get_maxlen(t), 0);
        assert_eq!(uhci_td_get_pid(t), UHCI_TD_PID_OUT);
    }

    #[test]
    fn little_endian_access() {
        let td = UhciTd {
            td_link: Cell::new(0),
            td_status: Cell::new(0),
            td_token: Cell::new(0),
            td_buffer: Cell::new(0),
        };
        uhci_set(&td.td_link, 0x1234_5602 | UHCI_PTR_QH);
        assert_eq!(td.td_link.get(), (0x1234_5602u32).to_le());
        assert_eq!(uhci_get(&td.td_link), 0x1234_5602);
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn constants_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/usb/uhcireg.h");
        let ours = crate::reftest::assert_defines!(defs;
            PCI_USBREV, PCI_USBREV_MASK, PCI_USBREV_PRE_1_0, PCI_USBREV_1_0, PCI_USBREV_1_1,
            PCI_LEGSUP, PCI_LEGSUP_USBPIRQDEN, PCI_CBIO, PCI_INTERFACE_UHCI,
            UHCI_CMD, UHCI_CMD_RS, UHCI_CMD_HCRESET, UHCI_CMD_GRESET, UHCI_CMD_EGSM,
            UHCI_CMD_FGR, UHCI_CMD_SWDBG, UHCI_CMD_CF, UHCI_CMD_MAXP,
            UHCI_STS, UHCI_STS_USBINT, UHCI_STS_USBEI, UHCI_STS_RD, UHCI_STS_HSE,
            UHCI_STS_HCPE, UHCI_STS_HCH, UHCI_STS_ALLINTRS,
            UHCI_INTR, UHCI_INTR_TOCRCIE, UHCI_INTR_RIE, UHCI_INTR_IOCE, UHCI_INTR_SPIE,
            UHCI_FRNUM, UHCI_FRNUM_MASK, UHCI_FLBASEADDR, UHCI_SOF, UHCI_SOF_MASK,
            UHCI_PORTSC1, UHCI_PORTSC2, UHCI_PORTSC_CCS, UHCI_PORTSC_CSC, UHCI_PORTSC_PE,
            UHCI_PORTSC_POEDC, UHCI_PORTSC_LS, UHCI_PORTSC_LS_SHIFT, UHCI_PORTSC_RD,
            UHCI_PORTSC_LSDA, UHCI_PORTSC_PR, UHCI_PORTSC_OCI, UHCI_PORTSC_OCIC,
            UHCI_PORTSC_SUSP,
            UHCI_FRAMELIST_COUNT, UHCI_FRAMELIST_ALIGN, UHCI_TD_ALIGN, UHCI_QH_ALIGN,
            UHCI_PTR_T, UHCI_PTR_TD, UHCI_PTR_QH, UHCI_PTR_VF, UHCI_QH_REMOVE_DELAY,
            UHCI_TD_BITSTUFF, UHCI_TD_CRCTO, UHCI_TD_NAK, UHCI_TD_BABBLE, UHCI_TD_DBUFFER,
            UHCI_TD_STALLED, UHCI_TD_ACTIVE, UHCI_TD_IOC, UHCI_TD_IOS, UHCI_TD_LS, UHCI_TD_SPD,
            UHCI_TD_PID_IN, UHCI_TD_PID_OUT, UHCI_TD_PID_SETUP, UHCI_TD_MAXLEN_MASK,
            UHCI_TD_ERROR,
        );
        crate::reftest::assert_complete(&defs, "UHCI_", &ours);
        crate::reftest::assert_complete(&defs, "PCI_", &ours);
    }
}
/* </TESTS> */
