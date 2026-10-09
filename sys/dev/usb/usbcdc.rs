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
//! The USB Communications Device Class's descriptors, requests and notifications:
//! `<dev/usb/usbcdc.h>`.
//!
//! Upstream: sys/dev/usb/usbcdc.h @ 3ce1f3f79392
//!
//! The class-specific descriptors (`UDESC_CS_INTERFACE` subtypes `UDESCSUB_CDC_*`) that
//! `cdce(4)` walks, the class requests (`UCDC_*`), the notifications a control interface's
//! interrupt endpoint sends, and the SERIAL STATE bits.
//!
//! ## Deviations
//! - The `__packed` structures are `#[repr(C)]` of `u8`s and byte arrays ([`UWord`],
//!   [`UDWord`]): alignment 1 as in C, read from descriptor bytes with [`UsbWire`].
//!   `bSlaveInterface[1]` is one entry, as in C; the others follow it in the descriptor.

use crate::dev::usb::usb::{UByte, UDWord, UWord, UsbWire};

/// `UDESCSUB_CDC_HEADER`.
pub const UDESCSUB_CDC_HEADER: u8 = 0;
/// `UDESCSUB_CDC_CM`: Call Management.
pub const UDESCSUB_CDC_CM: u8 = 1;
/// `UDESCSUB_CDC_ACM`: Abstract Control Model.
pub const UDESCSUB_CDC_ACM: u8 = 2;
/// `UDESCSUB_CDC_DLM`: Direct Line Management.
pub const UDESCSUB_CDC_DLM: u8 = 3;
/// `UDESCSUB_CDC_TRF`: Telephone Ringer.
pub const UDESCSUB_CDC_TRF: u8 = 4;
/// `UDESCSUB_CDC_TCLSR`: Telephone Call ...
pub const UDESCSUB_CDC_TCLSR: u8 = 5;
/// `UDESCSUB_CDC_UNION`.
pub const UDESCSUB_CDC_UNION: u8 = 6;
/// `UDESCSUB_CDC_CS`: Country Selection.
pub const UDESCSUB_CDC_CS: u8 = 7;
/// `UDESCSUB_CDC_TOM`: Telephone Operational Modes.
pub const UDESCSUB_CDC_TOM: u8 = 8;
/// `UDESCSUB_CDC_USBT`: USB Terminal.
pub const UDESCSUB_CDC_USBT: u8 = 9;
/// `UDESCSUB_CDC_NCT`: Network Channel Terminal.
pub const UDESCSUB_CDC_NCT: u8 = 10;
/// `UDESCSUB_CDC_PUF`: Protocol Unit.
pub const UDESCSUB_CDC_PUF: u8 = 11;
/// `UDESCSUB_CDC_EUF`: Extension Unit.
pub const UDESCSUB_CDC_EUF: u8 = 12;
/// `UDESCSUB_CDC_MCMF`: Multi-Channel Management.
pub const UDESCSUB_CDC_MCMF: u8 = 13;
/// `UDESCSUB_CDC_CCMF`: CAPI Control Management.
pub const UDESCSUB_CDC_CCMF: u8 = 14;
/// `UDESCSUB_CDC_ENF`: Ethernet Networking.
pub const UDESCSUB_CDC_ENF: u8 = 15;
/// `UDESCSUB_CDC_ANF`: ATM Networking.
pub const UDESCSUB_CDC_ANF: u8 = 16;

/// `struct usb_cdc_header_descriptor`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(non_snake_case)] // the USB specification's member names, as in C
pub struct UsbCdcHeaderDescriptor {
    /// `bLength`.
    pub bLength: UByte,
    /// `bDescriptorType`.
    pub bDescriptorType: UByte,
    /// `bDescriptorSubtype`.
    pub bDescriptorSubtype: UByte,
    /// `bcdCDC`.
    pub bcdCDC: UWord,
}

/// `struct usb_cdc_cm_descriptor`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(non_snake_case)] // the USB specification's member names, as in C
pub struct UsbCdcCmDescriptor {
    /// `bLength`.
    pub bLength: UByte,
    /// `bDescriptorType`.
    pub bDescriptorType: UByte,
    /// `bDescriptorSubtype`.
    pub bDescriptorSubtype: UByte,
    /// `bmCapabilities`: `USB_CDC_CM_*`.
    pub bmCapabilities: UByte,
    /// `bDataInterface`.
    pub bDataInterface: UByte,
}

/// `USB_CDC_CM_DOES_CM`.
pub const USB_CDC_CM_DOES_CM: u8 = 0x01;
/// `USB_CDC_CM_OVER_DATA`.
pub const USB_CDC_CM_OVER_DATA: u8 = 0x02;

/// `struct usb_cdc_acm_descriptor`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(non_snake_case)] // the USB specification's member names, as in C
pub struct UsbCdcAcmDescriptor {
    /// `bLength`.
    pub bLength: UByte,
    /// `bDescriptorType`.
    pub bDescriptorType: UByte,
    /// `bDescriptorSubtype`.
    pub bDescriptorSubtype: UByte,
    /// `bmCapabilities`: `USB_CDC_ACM_HAS_*`.
    pub bmCapabilities: UByte,
}

/// `USB_CDC_ACM_HAS_FEATURE`.
pub const USB_CDC_ACM_HAS_FEATURE: u8 = 0x01;
/// `USB_CDC_ACM_HAS_LINE`.
pub const USB_CDC_ACM_HAS_LINE: u8 = 0x02;
/// `USB_CDC_ACM_HAS_BREAK`.
pub const USB_CDC_ACM_HAS_BREAK: u8 = 0x04;
/// `USB_CDC_ACM_HAS_NETWORK_CONN`.
pub const USB_CDC_ACM_HAS_NETWORK_CONN: u8 = 0x08;

/// `struct usb_cdc_union_descriptor`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(non_snake_case)] // the USB specification's member names, as in C
pub struct UsbCdcUnionDescriptor {
    /// `bLength`.
    pub bLength: UByte,
    /// `bDescriptorType`.
    pub bDescriptorType: UByte,
    /// `bDescriptorSubtype`.
    pub bDescriptorSubtype: UByte,
    /// `bMasterInterface`.
    pub bMasterInterface: UByte,
    /// `bSlaveInterface[1]`: the first of the slave interfaces.
    pub bSlaveInterface: [UByte; 1],
}

/// `struct usb_cdc_ethernet_descriptor`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(non_snake_case)] // the USB specification's member names, as in C
pub struct UsbCdcEthernetDescriptor {
    /// `bLength`.
    pub bLength: UByte,
    /// `bDescriptorType`.
    pub bDescriptorType: UByte,
    /// `bDescriptorSubtype`.
    pub bDescriptorSubtype: UByte,
    /// `iMacAddress`: the index of the string holding the MAC address.
    pub iMacAddress: UByte,
    /// `bmEthernetStatistics`.
    pub bmEthernetStatistics: UDWord,
    /// `wMaxSegmentSize`.
    pub wMaxSegmentSize: UWord,
    /// `wNumberMCFilters`.
    pub wNumberMCFilters: UWord,
    /// `bNumberPowerFilters`.
    pub bNumberPowerFilters: UByte,
}

/// `UCDC_SEND_ENCAPSULATED_COMMAND`.
pub const UCDC_SEND_ENCAPSULATED_COMMAND: u8 = 0x00;
/// `UCDC_GET_ENCAPSULATED_RESPONSE`.
pub const UCDC_GET_ENCAPSULATED_RESPONSE: u8 = 0x01;
/// `UCDC_SET_COMM_FEATURE`.
pub const UCDC_SET_COMM_FEATURE: u8 = 0x02;
/// `UCDC_GET_COMM_FEATURE`.
pub const UCDC_GET_COMM_FEATURE: u8 = 0x03;
/// `UCDC_ABSTRACT_STATE`.
pub const UCDC_ABSTRACT_STATE: u8 = 0x01;
/// `UCDC_COUNTRY_SETTING`.
pub const UCDC_COUNTRY_SETTING: u8 = 0x02;
/// `UCDC_CLEAR_COMM_FEATURE`.
pub const UCDC_CLEAR_COMM_FEATURE: u8 = 0x04;
/// `UCDC_SET_LINE_CODING`.
pub const UCDC_SET_LINE_CODING: u8 = 0x20;
/// `UCDC_GET_LINE_CODING`.
pub const UCDC_GET_LINE_CODING: u8 = 0x21;
/// `UCDC_SET_CONTROL_LINE_STATE`.
pub const UCDC_SET_CONTROL_LINE_STATE: u8 = 0x22;
/// `UCDC_LINE_DTR`.
pub const UCDC_LINE_DTR: u16 = 0x0001;
/// `UCDC_LINE_RTS`.
pub const UCDC_LINE_RTS: u16 = 0x0002;
/// `UCDC_SEND_BREAK`.
pub const UCDC_SEND_BREAK: u8 = 0x23;
/// `UCDC_BREAK_ON`.
pub const UCDC_BREAK_ON: u16 = 0xffff;
/// `UCDC_BREAK_OFF`.
pub const UCDC_BREAK_OFF: u16 = 0x0000;

/// `struct usb_cdc_abstract_state`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(non_snake_case)] // the USB specification's member names, as in C
pub struct UsbCdcAbstractState {
    /// `wState`: `UCDC_IDLE_SETTING`, `UCDC_DATA_MULTIPLEXED`.
    pub wState: UWord,
}

/// `UCDC_IDLE_SETTING`.
pub const UCDC_IDLE_SETTING: u16 = 0x0001;
/// `UCDC_DATA_MULTIPLEXED`.
pub const UCDC_DATA_MULTIPLEXED: u16 = 0x0002;
/// `UCDC_ABSTRACT_STATE_LENGTH`.
pub const UCDC_ABSTRACT_STATE_LENGTH: usize = 2;

/// `struct usb_cdc_line_state`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(non_snake_case)] // the USB specification's member names, as in C
pub struct UsbCdcLineState {
    /// `dwDTERate`.
    pub dwDTERate: UDWord,
    /// `bCharFormat`: `UCDC_STOP_BIT_*`.
    pub bCharFormat: UByte,
    /// `bParityType`: `UCDC_PARITY_*`.
    pub bParityType: UByte,
    /// `bDataBits`.
    pub bDataBits: UByte,
}

/// `UCDC_STOP_BIT_1`.
pub const UCDC_STOP_BIT_1: u8 = 0;
/// `UCDC_STOP_BIT_1_5`.
pub const UCDC_STOP_BIT_1_5: u8 = 1;
/// `UCDC_STOP_BIT_2`.
pub const UCDC_STOP_BIT_2: u8 = 2;
/// `UCDC_PARITY_NONE`.
pub const UCDC_PARITY_NONE: u8 = 0;
/// `UCDC_PARITY_ODD`.
pub const UCDC_PARITY_ODD: u8 = 1;
/// `UCDC_PARITY_EVEN`.
pub const UCDC_PARITY_EVEN: u8 = 2;
/// `UCDC_PARITY_MARK`.
pub const UCDC_PARITY_MARK: u8 = 3;
/// `UCDC_PARITY_SPACE`.
pub const UCDC_PARITY_SPACE: u8 = 4;
/// `UCDC_LINE_STATE_LENGTH`.
pub const UCDC_LINE_STATE_LENGTH: usize = 7;

/// `struct usb_cdc_notification`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(non_snake_case)] // the USB specification's member names, as in C
pub struct UsbCdcNotification {
    /// `bmRequestType`: `UCDC_NOTIFICATION`.
    pub bmRequestType: UByte,
    /// `bNotification`: `UCDC_N_*`.
    pub bNotification: UByte,
    /// `wValue`.
    pub wValue: UWord,
    /// `wIndex`.
    pub wIndex: UWord,
    /// `wLength`.
    pub wLength: UWord,
    /// `data`.
    pub data: [UByte; 16],
}

/// `UCDC_NOTIFICATION`.
pub const UCDC_NOTIFICATION: u8 = 0xa1;
/// `UCDC_N_NETWORK_CONNECTION`.
pub const UCDC_N_NETWORK_CONNECTION: u8 = 0x00;
/// `UCDC_N_RESPONSE_AVAILABLE`.
pub const UCDC_N_RESPONSE_AVAILABLE: u8 = 0x01;
/// `UCDC_N_AUX_JACK_HOOK_STATE`.
pub const UCDC_N_AUX_JACK_HOOK_STATE: u8 = 0x08;
/// `UCDC_N_RING_DETECT`.
pub const UCDC_N_RING_DETECT: u8 = 0x09;
/// `UCDC_N_SERIAL_STATE`.
pub const UCDC_N_SERIAL_STATE: u8 = 0x20;
/// `UCDC_N_CALL_STATE_CHANGED`.
pub const UCDC_N_CALL_STATE_CHANGED: u8 = 0x28;
/// `UCDC_N_LINE_STATE_CHANGED`.
pub const UCDC_N_LINE_STATE_CHANGED: u8 = 0x29;
/// `UCDC_N_CONNECTION_SPEED_CHANGE`.
pub const UCDC_N_CONNECTION_SPEED_CHANGE: u8 = 0x2a;
/// `UCDC_NOTIFICATION_LENGTH`.
pub const UCDC_NOTIFICATION_LENGTH: usize = 8;

/// `struct usb_cdc_connection_speed`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(non_snake_case)] // the USB specification's member names, as in C
pub struct UsbCdcConnectionSpeed {
    /// `dwUSBitRate`.
    pub dwUSBitRate: UDWord,
    /// `dwDSBitRate`.
    pub dwDSBitRate: UDWord,
}

/// `UCDC_CONNECTION_SPEED_LENGTH`.
pub const UCDC_CONNECTION_SPEED_LENGTH: usize = 8;

/* Bits set in the SERIAL STATE notification (first byte of data) */
/// `UCDC_N_SERIAL_OVERRUN`.
pub const UCDC_N_SERIAL_OVERRUN: u8 = 0x40;
/// `UCDC_N_SERIAL_PARITY`.
pub const UCDC_N_SERIAL_PARITY: u8 = 0x20;
/// `UCDC_N_SERIAL_FRAMING`.
pub const UCDC_N_SERIAL_FRAMING: u8 = 0x10;
/// `UCDC_N_SERIAL_RI`.
pub const UCDC_N_SERIAL_RI: u8 = 0x08;
/// `UCDC_N_SERIAL_BREAK`.
pub const UCDC_N_SERIAL_BREAK: u8 = 0x04;
/// `UCDC_N_SERIAL_DSR`.
pub const UCDC_N_SERIAL_DSR: u8 = 0x02;
/// `UCDC_N_SERIAL_DCD`.
pub const UCDC_N_SERIAL_DCD: u8 = 0x01;

// SAFETY: `#[repr(C)]` of `u8`s and byte arrays only (sizes asserted below).
unsafe impl UsbWire for UsbCdcHeaderDescriptor {}
// SAFETY: as above.
unsafe impl UsbWire for UsbCdcCmDescriptor {}
// SAFETY: as above.
unsafe impl UsbWire for UsbCdcAcmDescriptor {}
// SAFETY: as above.
unsafe impl UsbWire for UsbCdcUnionDescriptor {}
// SAFETY: as above.
unsafe impl UsbWire for UsbCdcEthernetDescriptor {}
// SAFETY: as above.
unsafe impl UsbWire for UsbCdcAbstractState {}
// SAFETY: as above.
unsafe impl UsbWire for UsbCdcLineState {}
// SAFETY: as above.
unsafe impl UsbWire for UsbCdcNotification {}
// SAFETY: as above.
unsafe impl UsbWire for UsbCdcConnectionSpeed {}

const _: () = {
    assert!(size_of::<UsbCdcHeaderDescriptor>() == 5);
    assert!(size_of::<UsbCdcCmDescriptor>() == 5);
    assert!(size_of::<UsbCdcAcmDescriptor>() == 4);
    assert!(size_of::<UsbCdcUnionDescriptor>() == 5);
    assert!(size_of::<UsbCdcEthernetDescriptor>() == 13);
    assert!(size_of::<UsbCdcAbstractState>() == UCDC_ABSTRACT_STATE_LENGTH);
    assert!(size_of::<UsbCdcLineState>() == UCDC_LINE_STATE_LENGTH);
    assert!(size_of::<UsbCdcNotification>() == UCDC_NOTIFICATION_LENGTH + 16);
    assert!(size_of::<UsbCdcConnectionSpeed>() == UCDC_CONNECTION_SPEED_LENGTH);
    assert!(align_of::<UsbCdcNotification>() == 1);
    assert!(align_of::<UsbCdcEthernetDescriptor>() == 1);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;
    use crate::dev::usb::usb::ugetdw;

    #[test]
    fn notification_decodes_a_connection_speed_change() {
        let mut raw = [0u8; 24];
        raw[0] = UCDC_NOTIFICATION;
        raw[1] = UCDC_N_CONNECTION_SPEED_CHANGE;
        raw[8..12].copy_from_slice(&12_000_000u32.to_le_bytes());
        raw[12..16].copy_from_slice(&10_000_000u32.to_le_bytes());
        let n = UsbCdcNotification::read_from(&raw);
        assert_eq!(n.bmRequestType, UCDC_NOTIFICATION);
        let speed = UsbCdcConnectionSpeed::read_from(&n.data);
        assert_eq!(ugetdw(speed.dwUSBitRate), 12_000_000);
        assert_eq!(ugetdw(speed.dwDSBitRate), 10_000_000);
    }

    #[test]
    fn union_descriptor_reads_master_and_slave() {
        let raw = [5u8, 0x24, UDESCSUB_CDC_UNION, 0, 1];
        let u = UsbCdcUnionDescriptor::read_from(&raw);
        assert_eq!((u.bMasterInterface, u.bSlaveInterface[0]), (0, 1));
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn constants_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/usb/usbcdc.h");
        let ours = crate::reftest::assert_defines!(defs;
            UDESCSUB_CDC_HEADER, UDESCSUB_CDC_CM, UDESCSUB_CDC_ACM, UDESCSUB_CDC_DLM,
            UDESCSUB_CDC_TRF, UDESCSUB_CDC_TCLSR, UDESCSUB_CDC_UNION, UDESCSUB_CDC_CS,
            UDESCSUB_CDC_TOM, UDESCSUB_CDC_USBT, UDESCSUB_CDC_NCT, UDESCSUB_CDC_PUF,
            UDESCSUB_CDC_EUF, UDESCSUB_CDC_MCMF, UDESCSUB_CDC_CCMF, UDESCSUB_CDC_ENF,
            UDESCSUB_CDC_ANF, USB_CDC_CM_DOES_CM, USB_CDC_CM_OVER_DATA, USB_CDC_ACM_HAS_FEATURE,
            USB_CDC_ACM_HAS_LINE, USB_CDC_ACM_HAS_BREAK, USB_CDC_ACM_HAS_NETWORK_CONN,
            UCDC_SEND_ENCAPSULATED_COMMAND, UCDC_GET_ENCAPSULATED_RESPONSE,
            UCDC_SET_COMM_FEATURE, UCDC_GET_COMM_FEATURE, UCDC_ABSTRACT_STATE,
            UCDC_COUNTRY_SETTING, UCDC_CLEAR_COMM_FEATURE, UCDC_SET_LINE_CODING,
            UCDC_GET_LINE_CODING, UCDC_SET_CONTROL_LINE_STATE, UCDC_LINE_DTR, UCDC_LINE_RTS,
            UCDC_SEND_BREAK, UCDC_BREAK_ON, UCDC_BREAK_OFF, UCDC_IDLE_SETTING,
            UCDC_DATA_MULTIPLEXED, UCDC_ABSTRACT_STATE_LENGTH, UCDC_STOP_BIT_1,
            UCDC_STOP_BIT_1_5, UCDC_STOP_BIT_2, UCDC_PARITY_NONE, UCDC_PARITY_ODD,
            UCDC_PARITY_EVEN, UCDC_PARITY_MARK, UCDC_PARITY_SPACE, UCDC_LINE_STATE_LENGTH,
            UCDC_NOTIFICATION, UCDC_N_NETWORK_CONNECTION, UCDC_N_RESPONSE_AVAILABLE,
            UCDC_N_AUX_JACK_HOOK_STATE, UCDC_N_RING_DETECT, UCDC_N_SERIAL_STATE,
            UCDC_N_CALL_STATE_CHANGED, UCDC_N_LINE_STATE_CHANGED,
            UCDC_N_CONNECTION_SPEED_CHANGE, UCDC_NOTIFICATION_LENGTH,
            UCDC_CONNECTION_SPEED_LENGTH, UCDC_N_SERIAL_OVERRUN, UCDC_N_SERIAL_PARITY,
            UCDC_N_SERIAL_FRAMING, UCDC_N_SERIAL_RI, UCDC_N_SERIAL_BREAK, UCDC_N_SERIAL_DSR,
            UCDC_N_SERIAL_DCD,
        );
        crate::reftest::assert_complete(&defs, "UCDC_", &ours);
        crate::reftest::assert_complete(&defs, "UDESCSUB_CDC_", &ours);
    }
}
/* </TESTS> */
