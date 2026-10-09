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
 * Copyright (c) 2018 Martin Pieuchot
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
//! The `DLT_USBPCAP` capture headers `usb_tap` prepends: `<dev/usb/usbpcap.h>`.
//!
//! Upstream: sys/dev/usb/usbpcap.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - The `__attribute__((packed))` structures are `#[repr(C, packed)]` with the C's integer
//!   members, read and written by value only (a reference to a member of a packed structure
//!   is undefined behaviour); their sizes are pinned by `const` asserts. `usb_tap` assembles
//!   the C's union of `struct usbpcap_ctl_hdr` and `struct usbpcap_iso_hdr_full` as bytes at
//!   these structures' offsets.

/// `struct usbpcap_pkt_hdr`: common `DLT_USBPCAP` header.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default)]
pub struct UsbpcapPktHdr {
    /// `uph_hlen`: header length.
    pub uph_hlen: u16,
    /// `uph_id`: request ID.
    pub uph_id: u64,
    /// `uph_status`: USB status code.
    pub uph_status: u32,
    /// `uph_function`: stack function ID.
    pub uph_function: u16,
    /// `uph_info`: info flags.
    pub uph_info: u8,
    /// `uph_bus`: bus number.
    pub uph_bus: u16,
    /// `uph_devaddr`: device address.
    pub uph_devaddr: u16,
    /// `uph_epaddr`: copy of `bEndpointAddress`.
    pub uph_epaddr: u8,
    /// `uph_xfertype`: transfer type.
    pub uph_xfertype: u8,
    /// `uph_dlen`: data length.
    pub uph_dlen: u32,
}

/// `USBPCAP_INFO_DIRECTION_IN`: from Device to Host.
pub const USBPCAP_INFO_DIRECTION_IN: u8 = 1 << 0;

/// `USBPCAP_TRANSFER_ISOCHRONOUS`.
pub const USBPCAP_TRANSFER_ISOCHRONOUS: u8 = 0;
/// `USBPCAP_TRANSFER_INTERRUPT`.
pub const USBPCAP_TRANSFER_INTERRUPT: u8 = 1;
/// `USBPCAP_TRANSFER_CONTROL`.
pub const USBPCAP_TRANSFER_CONTROL: u8 = 2;
/// `USBPCAP_TRANSFER_BULK`.
pub const USBPCAP_TRANSFER_BULK: u8 = 3;

/// `struct usbpcap_ctl_hdr`: header used when dumping control transfers.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default)]
pub struct UsbpcapCtlHdr {
    /// `uch_hdr`.
    pub uch_hdr: UsbpcapPktHdr,
    /// `uch_stage`.
    pub uch_stage: u8,
}

/// `USBPCAP_CONTROL_STAGE_SETUP`.
pub const USBPCAP_CONTROL_STAGE_SETUP: u8 = 0;
/// `USBPCAP_CONTROL_STAGE_DATA`.
pub const USBPCAP_CONTROL_STAGE_DATA: u8 = 1;
/// `USBPCAP_CONTROL_STAGE_STATUS`.
pub const USBPCAP_CONTROL_STAGE_STATUS: u8 = 2;

/// `struct usbpcap_iso_pkt`.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default)]
pub struct UsbpcapIsoPkt {
    /// `uip_offset`.
    pub uip_offset: u32,
    /// `uip_length`.
    pub uip_length: u32,
    /// `uip_status`.
    pub uip_status: u32,
}

/// `struct usbpcap_iso_hdr`: header used when dumping isochronous transfers.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default)]
pub struct UsbpcapIsoHdr {
    /// `uih_hdr`.
    pub uih_hdr: UsbpcapPktHdr,
    /// `uih_startframe`.
    pub uih_startframe: u32,
    /// `uih_nframes`: number of frame.
    pub uih_nframes: u32,
    /// `uih_errors`: error count.
    pub uih_errors: u32,
    /// `uih_frames[1]`.
    pub uih_frames: [UsbpcapIsoPkt; 1],
}

/// `_USBPCAP_MAX_ISOFRAMES`: OpenBSD specific, maximum number of frames per transfer used
/// across all USB drivers. This allows us to setup the header on the stack.
pub const _USBPCAP_MAX_ISOFRAMES: usize = 40;

/// `struct usbpcap_iso_hdr_full`.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct UsbpcapIsoHdrFull {
    /// `uih_hdr`.
    pub uih_hdr: UsbpcapPktHdr,
    /// `uih_startframe`.
    pub uih_startframe: u32,
    /// `uih_nframes`.
    pub uih_nframes: u32,
    /// `uih_errors`.
    pub uih_errors: u32,
    /// `uih_frames`.
    pub uih_frames: [UsbpcapIsoPkt; _USBPCAP_MAX_ISOFRAMES],
}

const _: () = {
    assert!(size_of::<UsbpcapPktHdr>() == 27);
    assert!(size_of::<UsbpcapCtlHdr>() == 28);
    assert!(size_of::<UsbpcapIsoPkt>() == 12);
    assert!(size_of::<UsbpcapIsoHdr>() == 27 + 12 + 12);
    assert!(size_of::<UsbpcapIsoHdrFull>() == 27 + 12 + 12 * _USBPCAP_MAX_ISOFRAMES);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn constants_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/usb/usbpcap.h");
        let ours = crate::reftest::assert_defines!(defs;
            USBPCAP_INFO_DIRECTION_IN, USBPCAP_TRANSFER_ISOCHRONOUS, USBPCAP_TRANSFER_INTERRUPT,
            USBPCAP_TRANSFER_CONTROL, USBPCAP_TRANSFER_BULK, USBPCAP_CONTROL_STAGE_SETUP,
            USBPCAP_CONTROL_STAGE_DATA, USBPCAP_CONTROL_STAGE_STATUS, _USBPCAP_MAX_ISOFRAMES,
        );
        crate::reftest::assert_complete(&defs, "USBPCAP_", &ours);
    }
}
/* </TESTS> */
