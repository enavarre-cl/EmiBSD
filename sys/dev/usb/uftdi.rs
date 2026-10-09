/* <LICENSES> */
/*
 * Copyright (c) 2000 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Lennart Augustsson (lennart@augustsson.net).
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
//! uftdi(4): FTDI FT8U100AX serial adapters (and the FT232, FT2232 families that speak the
//! same protocol), a `ucom(4)` driver.
//!
//! Upstream: sys/dev/usb/uftdi.c @ 3ce1f3f79392
//!
//! This driver will not support multiple serial ports: the ucom layer needs to be extended
//! first. `uftdi_match` takes a device of the table `uftdi_devs[]` in configuration 1 (not
//! JTAG interface 0 of the OpenRD). `uftdi_attach` reads the bulk endpoints (and, from the
//! `bcdDevice`, the chip: SIO, 8U232AM, 2232H or 232R, which decide the baud rate divisors
//! and whether the bulk-out packets carry a length tag), and attaches a `ucom` child with the
//! methods below. The bulk-in packets begin with two status bytes (the modem and line status),
//! which `uftdi_read` strips and reports to `ucom_status_change`; `uftdi_open` resets the port,
//! sets 9600 baud, 2 stop bits, 8 data bits and no parity (`uftdi_param`) and RTS/CTS flow
//! control; `uftdi_param` programs the baud rate, the data format and the flow control from a
//! termios.
//!
//! ## Deviations
//! - `UFTDI_DEBUG` is not in GENERIC: the `DPRINTF`s are absent.
//! - `uftdi_8u232am_getrate` and `uftdi_2232h_getrate` answer `Option<i32>` (`None` for the
//!   C's -1; the rate is the return value, not an out parameter). The 2232H one answers `None`
//!   for a speed of 0 or less, where the C divides by zero, and when the divisor is 0.
//! - The `ucom_methods` are Rust functions of the types `ucomvar.rs` declares: `uftdi_read`
//!   strips the two status bytes by advancing a slice (the C's `*ptr += 2; *count -= 2` wraps
//!   `count` for a shorter packet), `uftdi_write` fills `to` and answers the packet's length,
//!   `uftdi_get_status` gets both bytes (never NULL).
//! - `uftdi_devs[]` has the 628 entries of the C table, in its order.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::{self, NonNull};

use crate::dev::usb::ucom::{ucom_softc_of, ucom_status_change, ucomprint, ucomsubmatch};
use crate::dev::usb::ucomvar::{
    UCOM_SET_BREAK, UCOM_SET_DTR, UCOM_SET_RTS, UcomAttachArgs, UcomMethods,
};
use crate::dev::usb::uftdireg::{
    FTDI_8U232AM_FREQ, FTDI_8U232AM_MAX_DIV, FTDI_8U232AM_MIN_DIV, FTDI_2232H_FREQ, FTDI_GET_LSR,
    FTDI_GET_MSR, FTDI_LSR_MASK, FTDI_OUT_TAG, FTDI_PIT_SIOA, FTDI_SIO_DISABLE_FLOW_CTRL,
    FTDI_SIO_MODEM_CTRL, FTDI_SIO_RESET, FTDI_SIO_RESET_SIO, FTDI_SIO_RTS_CTS_HS,
    FTDI_SIO_SET_BAUD_RATE, FTDI_SIO_SET_BREAK, FTDI_SIO_SET_DATA, FTDI_SIO_SET_DATA_BITS,
    FTDI_SIO_SET_DATA_PARITY_EVEN, FTDI_SIO_SET_DATA_PARITY_NONE, FTDI_SIO_SET_DATA_PARITY_ODD,
    FTDI_SIO_SET_DATA_STOP_BITS_1, FTDI_SIO_SET_DATA_STOP_BITS_2, FTDI_SIO_SET_DTR_HIGH,
    FTDI_SIO_SET_DTR_LOW, FTDI_SIO_SET_FLOW_CTRL, FTDI_SIO_SET_RTS_HIGH, FTDI_SIO_SET_RTS_LOW,
    FTDI_SIO_XON_XOFF_HS, UftdiType, ftdi_sio_b300, ftdi_sio_b600, ftdi_sio_b1200, ftdi_sio_b2400,
    ftdi_sio_b4800, ftdi_sio_b9600, ftdi_sio_b19200, ftdi_sio_b38400, ftdi_sio_b57600,
    ftdi_sio_b115200,
};
use crate::dev::usb::usb::{
    UE_BULK, UE_DIR_IN, UE_DIR_OUT, UT_WRITE_VENDOR_DEVICE, UsbDeviceRequest, ue_get_dir,
    ue_get_xfertype, ugetw, usetw, usetw2,
};
use crate::dev::usb::usbdevs::*;
use crate::dev::usb::usbdi::{
    UMATCH_NONE, UMATCH_VENDOR_PRODUCT_CONF_IFACE, UsbAttachArg, UsbDevno, usb_lookup,
    usbd_deactivate, usbd_do_request, usbd_get_interface_descriptor,
    usbd_interface2endpoint_descriptor, usbd_is_dying,
};
use crate::dev::usb::usbdivar::{UsbdDevice, UsbdInterface};
use crate::kern::subr_autoconf::{config_detach, config_found_sm};
use crate::kern::subr_prf::printf;
use crate::sys::device::{CfMatch, Cfattach, Cfdriver, DV_DULL, Device, Softc};
use crate::sys::errno::Errno;
use crate::sys::termios::{
    CRTSCTS, CS5, CS6, CS7, CS8, CSIZE, CSTOPB, IXOFF, IXON, PARENB, PARODD, Termios, VSTART, VSTOP,
};

/// `UFTDIIBUFSIZE`: the maximum number of bytes transferred per frame. The output buffer
/// size cannot be increased due to the size encoding.
pub const UFTDIIBUFSIZE: u32 = 64;
/// `UFTDIOBUFSIZE`.
pub const UFTDIOBUFSIZE: u32 = 64;

/// `struct uftdi_softc`. All-zero is a valid value.
#[repr(C)]
pub struct UftdiSoftc {
    /// `sc_dev`: base device.
    pub sc_dev: Device,
    /// `sc_udev`: device.
    pub sc_udev: Cell<Option<&'static UsbdDevice>>,
    /// `sc_iface`: interface.
    pub sc_iface: Cell<Option<&'static UsbdInterface>>,

    /// `sc_type`.
    pub sc_type: Cell<UftdiType>,
    /// `sc_hdrlen`.
    pub sc_hdrlen: Cell<u32>,

    /// `sc_msr`.
    pub sc_msr: Cell<u8>,
    /// `sc_lsr`.
    pub sc_lsr: Cell<u8>,

    /// `sc_subdev`: the ucom child.
    pub sc_subdev: Cell<Option<NonNull<Device>>>,

    /// `last_lcr`.
    pub last_lcr: Cell<u32>,
}

// SAFETY: `#[repr(C)]` with the device first; every other member is a `Cell` of an integer,
// an `Option` of a reference or a pointer, or `UftdiType`, whose first variant is 0: all
// valid as zero bits.
unsafe impl Softc for UftdiSoftc {}

/// `uftdi_methods`.
pub static UFTDI_METHODS: UcomMethods = UcomMethods {
    ucom_get_status: Some(uftdi_get_status),
    ucom_set: Some(uftdi_set),
    ucom_param: Some(uftdi_param),
    ucom_ioctl: None,
    ucom_open: Some(uftdi_open),
    ucom_close: None,
    ucom_read: Some(uftdi_read),
    ucom_write: Some(uftdi_write),
};

/// `uftdi_cd`.
pub static UFTDI_CD: Cfdriver = Cfdriver::new(b"uftdi", DV_DULL, 0);

/// `uftdi_ca`.
pub static UFTDI_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<UftdiSoftc>(),
    ca_match: Some(uftdi_match),
    ca_attach: uftdi_attach,
    ca_detach: Some(uftdi_detach),
    ca_activate: None,
};

/// `uftdi_devs[]`.
static UFTDI_DEVS: [UsbDevno; 628] = [
    UsbDevno {
        ud_vendor: USB_VENDOR_ALTI2,
        ud_product: USB_PRODUCT_ALTI2_NEPTUNE3,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_ANALOGDEVICES,
        ud_product: USB_PRODUCT_ANALOGDEVICES_GNICE,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_ANALOGDEVICES,
        ud_product: USB_PRODUCT_ANALOGDEVICES_GNICEPLUS,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_ATMEL,
        ud_product: USB_PRODUCT_ATMEL_STK541,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_BAYER,
        ud_product: USB_PRODUCT_BAYER_CONTOUR,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_BBELECTR,
        ud_product: USB_PRODUCT_BBELECTR_232USB9M,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_BBELECTR,
        ud_product: USB_PRODUCT_BBELECTR_485USB9F2W,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_BBELECTR,
        ud_product: USB_PRODUCT_BBELECTR_485USB9F4W,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_BBELECTR,
        ud_product: USB_PRODUCT_BBELECTR_485USBTB_2W,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_BBELECTR,
        ud_product: USB_PRODUCT_BBELECTR_485USBTB_4W,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_BBELECTR,
        ud_product: USB_PRODUCT_BBELECTR_TTL3USB9M,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_BBELECTR,
        ud_product: USB_PRODUCT_BBELECTR_TTL5USB9M,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_BBELECTR,
        ud_product: USB_PRODUCT_BBELECTR_USO9ML2,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_BBELECTR,
        ud_product: USB_PRODUCT_BBELECTR_USO9ML2DR,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_BBELECTR,
        ud_product: USB_PRODUCT_BBELECTR_USO9ML2DR2,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_BBELECTR,
        ud_product: USB_PRODUCT_BBELECTR_USOPTL4,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_BBELECTR,
        ud_product: USB_PRODUCT_BBELECTR_USOPTL4DR,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_BBELECTR,
        ud_product: USB_PRODUCT_BBELECTR_USOPTL4DR2,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_BBELECTR,
        ud_product: USB_PRODUCT_BBELECTR_USOTL4,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_BBELECTR,
        ud_product: USB_PRODUCT_BBELECTR_USPTL4,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_BBELECTR,
        ud_product: USB_PRODUCT_BBELECTR_USTL4,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_BBELECTR,
        ud_product: USB_PRODUCT_BBELECTR_ZZ_PROG1,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_BRAINBOXES,
        ud_product: USB_PRODUCT_BRAINBOXES_US101,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_BRAINBOXES,
        ud_product: USB_PRODUCT_BRAINBOXES_US159,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_BRAINBOXES,
        ud_product: USB_PRODUCT_BRAINBOXES_US235,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_BRAINBOXES,
        ud_product: USB_PRODUCT_BRAINBOXES_US257,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_BRAINBOXES,
        ud_product: USB_PRODUCT_BRAINBOXES_US279_12,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_BRAINBOXES,
        ud_product: USB_PRODUCT_BRAINBOXES_US279_34,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_BRAINBOXES,
        ud_product: USB_PRODUCT_BRAINBOXES_US279_56,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_BRAINBOXES,
        ud_product: USB_PRODUCT_BRAINBOXES_US279_78,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_BRAINBOXES,
        ud_product: USB_PRODUCT_BRAINBOXES_US313,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_BRAINBOXES,
        ud_product: USB_PRODUCT_BRAINBOXES_US320,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_BRAINBOXES,
        ud_product: USB_PRODUCT_BRAINBOXES_US324,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_BRAINBOXES,
        ud_product: USB_PRODUCT_BRAINBOXES_US346_12,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_BRAINBOXES,
        ud_product: USB_PRODUCT_BRAINBOXES_US346_34,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_BRAINBOXES,
        ud_product: USB_PRODUCT_BRAINBOXES_US701_12,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_BRAINBOXES,
        ud_product: USB_PRODUCT_BRAINBOXES_US701_34,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_BRAINBOXES,
        ud_product: USB_PRODUCT_BRAINBOXES_US842_12,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_BRAINBOXES,
        ud_product: USB_PRODUCT_BRAINBOXES_US842_34,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_BRAINBOXES,
        ud_product: USB_PRODUCT_BRAINBOXES_US842_56,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_BRAINBOXES,
        ud_product: USB_PRODUCT_BRAINBOXES_US842_78,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_DRESDENELEC,
        ud_product: USB_PRODUCT_DRESDENELEC_STB,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_DRESDENELEC,
        ud_product: USB_PRODUCT_DRESDENELEC_WHT,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_ELEKTOR,
        ud_product: USB_PRODUCT_ELEKTOR_FT323R,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_EVOLUTION,
        ud_product: USB_PRODUCT_EVOLUTION_ER1,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_EVOLUTION,
        ud_product: USB_PRODUCT_EVOLUTION_RCM4_1,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_EVOLUTION,
        ud_product: USB_PRODUCT_EVOLUTION_RCM4_2,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FALCOM,
        ud_product: USB_PRODUCT_FALCOM_SAMBA,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FALCOM,
        ud_product: USB_PRODUCT_FALCOM_TWIST,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_ACCESSO,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_ACG_HFDUAL,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_ACTROBOTS,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_ACTZWAVE,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_AMC232,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_ARTEMIS,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_ASK_RDR4X7_1,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_ASK_RDR4X7_2,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_ASK_RDR4X7_3,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_ASK_RDR4X7_4,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_ASK_RDR4X7_5,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_ASK_RDR4X7_6,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_ASK_RDR4X7_7,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_ASK_RDR4X7_8,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_ATK16,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_ATK16C,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_ATK16HR,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_ATK16HRC,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_ATK16IC,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_BCS_SE923,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_CANDAPTER,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_CANUSB,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_CCS_ICDU20,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_CCS_ICDU40,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_CCS_ICDU64,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_CCS_LOAD_N_GO,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_CCS_MACHX,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_CCS_PRIME8,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_CHAMELEON,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_COASTAL_TNCX,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_DGQG,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_DMX4ALL,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_DUSB,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_ECLO_1WIRE,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_ECO_PRO,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_EISCOU,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_ELV_ALC8500,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_ELV_CLI7000,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_ELV_CSI8,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_ELV_EM1000DL,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_ELV_EM1010PC,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_ELV_FEM,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_ELV_FHZ1000PC,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_ELV_FHZ1300PC,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_ELV_FM3RX,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_ELV_FS20SIG,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_ELV_HS485,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_ELV_KL100,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_ELV_MSM1,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_ELV_PCD200,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_ELV_PCK100,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_ELV_PPS7330,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_ELV_RFP500,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_ELV_T1100,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_ELV_TFD128,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_ELV_TFM100,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_ELV_TWS550,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_ELV_UAD7,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_ELV_UAD8,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_ELV_UDF77,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_ELV_UIO88,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_ELV_ULA200,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_ELV_UM100,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_ELV_UMS100,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_ELV_UO100,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_ELV_UR100,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_ELV_USI2,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_ELV_USR,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_ELV_UTP8,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_ELV_WS300PC,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_ELV_WS444PC,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_ELV_WS500,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_ELV_WS550,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_ELV_WS777,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_ELV_WS888,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_FISCO,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_FT232RL,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_FT232_1,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_FT232_3,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_FT232_4,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_FT232_5,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_FT232_6,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_FT4232H,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_FTX,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_GAMMASCOUT,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_GUDE_1,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_GUDE_2,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_GUDE_3,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_GUDE_4,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_GUDE_5,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_GUDE_6,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_GUDE_7,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_GUDE_8,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_GUDE_9,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_GUDE_A,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_GUDE_B,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_HO820,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_HO870,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_IBS_1,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_IBS_APP70,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_IBS_PCMCIA,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_IBS_PEDO,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_IBS_PICPRO,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_IBS_PK1,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_IBS_RS232MON,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_IBS_US485,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_IPLUS,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_IPLUS2,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_JTAGCABLEII,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_LCD_CFA_547,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_LCD_CFA_631,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_LCD_CFA_632,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_LCD_CFA_633,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_LCD_CFA_634,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_LCD_CFA_635,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_LCD_CFA_640,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_LCD_CFA_642,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_LCD_LK202_24,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_LCD_LK204_24,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_LCD_MTXO,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_LCD_MX200,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_LINX_1,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_LINX_2,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_LINX_3,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_LINX_MASTER2,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_LM3S_DEVEL,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_LM3S_EVAL,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_LOCOBUFFER,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_MATRIX_2,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_MATRIX_3,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_MHAM_DB9,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_MHAM_IC,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_MHAM_KW,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_MHAM_RS232,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_MHAM_Y6,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_MHAM_Y8,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_MHAM_Y9,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_MHAM_YS,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_MJS_SIRIUS_PC_1,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_MJS_SIRIUS_PC_2,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_NXTCAM,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_OCEANIC,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_OOCDLINK,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_OPENDCC,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_OPENDCC_GATEWAY,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_OPENDCC_SNIFFER,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_OPENDCC_THROTTLE,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_OPENPORT_13M,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_OPENPORT_13S,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_OPENPORT_13U,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_OPENRD,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_PCDJ_DAC2,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_PIEGROUP_IR,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_PROTEGO_1,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_PROTEGO_3,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_PROTEGO_4,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_PROTEGO_R200,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_PYRAMID,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_R2000KU_RNG,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_RELAIS,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_REU_TINY,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_SCS_0,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_SCS_1,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_SCS_2,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_SCS_3,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_SCS_4,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_SCS_5,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_SCS_6,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_SCS_7,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_SEMC_DSS20,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_SERIAL_2232C,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_SERIAL_2232L,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_SERIAL_232BM,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_SERIAL_8U100AX,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_SERIAL_8U232AM,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_SERIAL_8U232AM4,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_SPROG_II,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_SUUNTO,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_TERATRONIK_D2XX,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_TERATRONIK_VCP,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_THORLABS,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_TIRA1,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_TNCX,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_TURTELIZER_JTAG,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_UNICOM,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_UOPTBR,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_USBSERIAL,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_USBUIRT,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_USBX_707,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_VNHC,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_WESTREX_777,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_WESTREX_8900F,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_XSENS_1,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_XSENS_2,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_XSENS_3,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_XSENS_4,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_XSENS_5,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_XSENS_6,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_XSENS_7,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_XSENS_8,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_FTDI,
        ud_product: USB_PRODUCT_FTDI_YEI_SC31,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_GNOTOMETRICS,
        ud_product: USB_PRODUCT_GNOTOMETRICS_AURICAL,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_ICOM,
        ud_product: USB_PRODUCT_ICOM_ID1,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_ICOM,
        ud_product: USB_PRODUCT_ICOM_RP2000VR,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_ICOM,
        ud_product: USB_PRODUCT_ICOM_RP2000VT,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_ICOM,
        ud_product: USB_PRODUCT_ICOM_RP2C1,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_ICOM,
        ud_product: USB_PRODUCT_ICOM_RP2C2,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_ICOM,
        ud_product: USB_PRODUCT_ICOM_RP2D,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_ICOM,
        ud_product: USB_PRODUCT_ICOM_RP2VR,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_ICOM,
        ud_product: USB_PRODUCT_ICOM_RP2VT,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_ICOM,
        ud_product: USB_PRODUCT_ICOM_RP4000VR,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_ICOM,
        ud_product: USB_PRODUCT_ICOM_RP4000VT,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_IDTECH,
        ud_product: USB_PRODUCT_IDTECH_SERIAL,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_INTERBIO,
        ud_product: USB_PRODUCT_INTERBIO_IOBOARD,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_INTERBIO,
        ud_product: USB_PRODUCT_INTERBIO_MINIIOBOARD,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_INTERBIO,
        ud_product: USB_PRODUCT_INTERBIO_MINIIOBOARD2,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_INTREPIDCS,
        ud_product: USB_PRODUCT_INTREPIDCS_NEOVI,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_INTREPIDCS,
        ud_product: USB_PRODUCT_INTREPIDCS_VALUECAN,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_IODATA,
        ud_product: USB_PRODUCT_IODATA_FT232R,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_JETI,
        ud_product: USB_PRODUCT_JETI_SPC1201,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_KOBIL,
        ud_product: USB_PRODUCT_KOBIL_B1,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_KOBIL,
        ud_product: USB_PRODUCT_KOBIL_KAAN,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_LARSENBRUSGAARD,
        ud_product: USB_PRODUCT_LARSENBRUSGAARD_ALTITRACK,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MARVELL,
        ud_product: USB_PRODUCT_MARVELL_SHEEVAPLUG,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0100,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0101,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0102,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0103,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0104,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0105,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0106,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0107,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0108,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0109,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_010A,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_010B,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_010C,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_010D,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_010E,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_010F,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0110,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0111,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0112,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0113,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0114,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0115,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0116,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0117,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0118,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0119,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_011A,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_011B,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_011C,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_011D,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_011E,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_011F,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0120,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0121,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0122,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0123,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0124,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0125,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0126,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0127,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0128,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0129,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_012A,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_012B,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_012C,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_012D,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_012E,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_012F,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0130,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0131,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0132,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0133,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0134,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0135,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0136,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0137,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0138,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0139,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_013A,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_013B,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_013C,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_013D,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_013E,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_013F,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0140,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0141,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0142,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0143,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0144,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0145,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0146,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0147,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0148,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0149,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_014A,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_014B,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_014C,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_014D,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_014E,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_014F,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0150,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0151,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0152,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0153,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0154,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0155,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0156,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0157,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0158,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0159,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_015A,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_015B,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_015C,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_015D,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_015E,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_015F,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0160,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0161,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0162,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0163,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0164,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0165,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0166,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0167,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0168,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0169,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_016A,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_016B,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_016C,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_016D,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_016E,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_016F,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0170,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0171,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0172,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0173,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0174,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0175,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0176,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0177,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0178,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0179,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_017A,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_017B,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_017C,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_017D,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_017E,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_017F,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0180,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0181,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0182,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0183,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0184,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0185,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0186,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0187,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0188,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0189,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_018A,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_018B,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_018C,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_018D,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_018E,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_018F,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0190,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0191,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0192,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0193,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0194,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0195,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0196,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0197,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0198,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_0199,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_019A,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_019B,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_019C,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_019D,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_019E,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_019F,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01A0,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01A1,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01A2,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01A3,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01A4,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01A5,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01A6,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01A7,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01A8,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01A9,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01AA,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01AB,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01AC,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01AD,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01AE,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01AF,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01B0,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01B1,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01B2,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01B3,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01B4,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01B5,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01B6,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01B7,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01B8,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01B9,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01BA,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01BB,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01BC,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01BD,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01BE,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01BF,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01C0,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01C1,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01C2,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01C3,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01C4,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01C5,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01C6,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01C7,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01C8,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01C9,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01CA,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01CB,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01CC,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01CD,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01CE,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01CF,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01D0,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01D1,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01D2,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01D3,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01D4,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01D5,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01D6,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01D7,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01D8,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01D9,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01DA,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01DB,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01DC,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01DD,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01DE,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01DF,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01E0,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01E1,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01E2,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01E3,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01E4,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01E5,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01E6,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01E7,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01E8,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01E9,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01EA,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01EB,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01EC,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01ED,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01EE,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01EF,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01F0,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01F1,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01F2,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01F3,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01F4,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01F5,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01F6,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01F7,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01F8,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01F9,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01FA,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01FB,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01FC,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01FD,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01FE,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MATRIXORB,
        ud_product: USB_PRODUCT_MATRIXORB_LCD_01FF,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MECANIQUE,
        ud_product: USB_PRODUCT_MECANIQUE_TELLSTICK,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MELCO,
        ud_product: USB_PRODUCT_MELCO_PCOPRS1,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MOBILITY,
        ud_product: USB_PRODUCT_MOBILITY_ED200H,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_OCT,
        ud_product: USB_PRODUCT_OCT_US2308,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_PAPOUCH,
        ud_product: USB_PRODUCT_PAPOUCH_AD4USB,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_PAPOUCH,
        ud_product: USB_PRODUCT_PAPOUCH_AP485_1,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_PAPOUCH,
        ud_product: USB_PRODUCT_PAPOUCH_AP485_2,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_PAPOUCH,
        ud_product: USB_PRODUCT_PAPOUCH_DRAK5,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_PAPOUCH,
        ud_product: USB_PRODUCT_PAPOUCH_DRAK6,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_PAPOUCH,
        ud_product: USB_PRODUCT_PAPOUCH_GOLIATH_MSR,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_PAPOUCH,
        ud_product: USB_PRODUCT_PAPOUCH_GOLIATH_MUX,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_PAPOUCH,
        ud_product: USB_PRODUCT_PAPOUCH_IRAMP,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_PAPOUCH,
        ud_product: USB_PRODUCT_PAPOUCH_LEC,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_PAPOUCH,
        ud_product: USB_PRODUCT_PAPOUCH_MUC,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_PAPOUCH,
        ud_product: USB_PRODUCT_PAPOUCH_QUIDO101,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_PAPOUCH,
        ud_product: USB_PRODUCT_PAPOUCH_QUIDO216,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_PAPOUCH,
        ud_product: USB_PRODUCT_PAPOUCH_QUIDO22,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_PAPOUCH,
        ud_product: USB_PRODUCT_PAPOUCH_QUIDO303,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_PAPOUCH,
        ud_product: USB_PRODUCT_PAPOUCH_QUIDO332,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_PAPOUCH,
        ud_product: USB_PRODUCT_PAPOUCH_QUIDO44,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_PAPOUCH,
        ud_product: USB_PRODUCT_PAPOUCH_QUIDO603,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_PAPOUCH,
        ud_product: USB_PRODUCT_PAPOUCH_QUIDO88,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_PAPOUCH,
        ud_product: USB_PRODUCT_PAPOUCH_SB232,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_PAPOUCH,
        ud_product: USB_PRODUCT_PAPOUCH_SB422_1,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_PAPOUCH,
        ud_product: USB_PRODUCT_PAPOUCH_SB422_2,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_PAPOUCH,
        ud_product: USB_PRODUCT_PAPOUCH_SB485C,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_PAPOUCH,
        ud_product: USB_PRODUCT_PAPOUCH_SB485S,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_PAPOUCH,
        ud_product: USB_PRODUCT_PAPOUCH_SB485_1,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_PAPOUCH,
        ud_product: USB_PRODUCT_PAPOUCH_SB485_2,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_PAPOUCH,
        ud_product: USB_PRODUCT_PAPOUCH_SERIAL,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_PAPOUCH,
        ud_product: USB_PRODUCT_PAPOUCH_SIMUKEY,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_PAPOUCH,
        ud_product: USB_PRODUCT_PAPOUCH_STAVOVY,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_PAPOUCH,
        ud_product: USB_PRODUCT_PAPOUCH_TMU,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_POSIFLEX,
        ud_product: USB_PRODUCT_POSIFLEX_PP7000_1,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_POSIFLEX,
        ud_product: USB_PRODUCT_POSIFLEX_PP7000_2,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_RATOC,
        ud_product: USB_PRODUCT_RATOC_REXUSB60F,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_RTSYSTEMS,
        ud_product: USB_PRODUCT_RTSYSTEMS_CT57B,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_SACOM,
        ud_product: USB_PRODUCT_SACOM_USB485BL,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_SEALEVEL,
        ud_product: USB_PRODUCT_SEALEVEL_2101,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_SEALEVEL,
        ud_product: USB_PRODUCT_SEALEVEL_2102,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_SEALEVEL,
        ud_product: USB_PRODUCT_SEALEVEL_2103,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_SEALEVEL,
        ud_product: USB_PRODUCT_SEALEVEL_2104,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_SEALEVEL,
        ud_product: USB_PRODUCT_SEALEVEL_2106,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_SEALEVEL,
        ud_product: USB_PRODUCT_SEALEVEL_2201_1,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_SEALEVEL,
        ud_product: USB_PRODUCT_SEALEVEL_2201_2,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_SEALEVEL,
        ud_product: USB_PRODUCT_SEALEVEL_2202_1,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_SEALEVEL,
        ud_product: USB_PRODUCT_SEALEVEL_2202_2,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_SEALEVEL,
        ud_product: USB_PRODUCT_SEALEVEL_2203_1,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_SEALEVEL,
        ud_product: USB_PRODUCT_SEALEVEL_2203_2,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_SEALEVEL,
        ud_product: USB_PRODUCT_SEALEVEL_2401_1,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_SEALEVEL,
        ud_product: USB_PRODUCT_SEALEVEL_2401_2,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_SEALEVEL,
        ud_product: USB_PRODUCT_SEALEVEL_2401_3,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_SEALEVEL,
        ud_product: USB_PRODUCT_SEALEVEL_2401_4,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_SEALEVEL,
        ud_product: USB_PRODUCT_SEALEVEL_2402_1,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_SEALEVEL,
        ud_product: USB_PRODUCT_SEALEVEL_2402_2,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_SEALEVEL,
        ud_product: USB_PRODUCT_SEALEVEL_2402_3,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_SEALEVEL,
        ud_product: USB_PRODUCT_SEALEVEL_2402_4,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_SEALEVEL,
        ud_product: USB_PRODUCT_SEALEVEL_2403_1,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_SEALEVEL,
        ud_product: USB_PRODUCT_SEALEVEL_2403_2,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_SEALEVEL,
        ud_product: USB_PRODUCT_SEALEVEL_2403_3,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_SEALEVEL,
        ud_product: USB_PRODUCT_SEALEVEL_2403_4,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_SEALEVEL,
        ud_product: USB_PRODUCT_SEALEVEL_2801_1,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_SEALEVEL,
        ud_product: USB_PRODUCT_SEALEVEL_2801_2,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_SEALEVEL,
        ud_product: USB_PRODUCT_SEALEVEL_2801_3,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_SEALEVEL,
        ud_product: USB_PRODUCT_SEALEVEL_2801_4,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_SEALEVEL,
        ud_product: USB_PRODUCT_SEALEVEL_2801_5,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_SEALEVEL,
        ud_product: USB_PRODUCT_SEALEVEL_2801_6,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_SEALEVEL,
        ud_product: USB_PRODUCT_SEALEVEL_2801_7,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_SEALEVEL,
        ud_product: USB_PRODUCT_SEALEVEL_2801_8,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_SEALEVEL,
        ud_product: USB_PRODUCT_SEALEVEL_2802_1,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_SEALEVEL,
        ud_product: USB_PRODUCT_SEALEVEL_2802_2,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_SEALEVEL,
        ud_product: USB_PRODUCT_SEALEVEL_2802_3,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_SEALEVEL,
        ud_product: USB_PRODUCT_SEALEVEL_2802_4,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_SEALEVEL,
        ud_product: USB_PRODUCT_SEALEVEL_2802_5,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_SEALEVEL,
        ud_product: USB_PRODUCT_SEALEVEL_2802_6,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_SEALEVEL,
        ud_product: USB_PRODUCT_SEALEVEL_2802_7,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_SEALEVEL,
        ud_product: USB_PRODUCT_SEALEVEL_2802_8,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_SEALEVEL,
        ud_product: USB_PRODUCT_SEALEVEL_2803_1,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_SEALEVEL,
        ud_product: USB_PRODUCT_SEALEVEL_2803_2,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_SEALEVEL,
        ud_product: USB_PRODUCT_SEALEVEL_2803_3,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_SEALEVEL,
        ud_product: USB_PRODUCT_SEALEVEL_2803_4,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_SEALEVEL,
        ud_product: USB_PRODUCT_SEALEVEL_2803_5,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_SEALEVEL,
        ud_product: USB_PRODUCT_SEALEVEL_2803_6,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_SEALEVEL,
        ud_product: USB_PRODUCT_SEALEVEL_2803_7,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_SEALEVEL,
        ud_product: USB_PRODUCT_SEALEVEL_2803_8,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_STOLLMANN,
        ud_product: USB_PRODUCT_STOLLMANN_ISDN_TA_USBA,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_TESTO,
        ud_product: USB_PRODUCT_TESTO_174,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_TESTO,
        ud_product: USB_PRODUCT_TESTO_175,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_TESTO,
        ud_product: USB_PRODUCT_TESTO_330,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_TESTO,
        ud_product: USB_PRODUCT_TESTO_435,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_TESTO,
        ud_product: USB_PRODUCT_TESTO_556,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_TESTO,
        ud_product: USB_PRODUCT_TESTO_580,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_TESTO,
        ud_product: USB_PRODUCT_TESTO_845,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_TESTO,
        ud_product: USB_PRODUCT_TESTO_SERIAL_1,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_TESTO,
        ud_product: USB_PRODUCT_TESTO_SERIAL_2,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_TESTO,
        ud_product: USB_PRODUCT_TESTO_SERVICE,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_THURLBY,
        ud_product: USB_PRODUCT_THURLBY_QL355P,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_TML,
        ud_product: USB_PRODUCT_TML_SERIAL,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_UNKNOWN5,
        ud_product: USB_PRODUCT_UNKNOWN5_NF_RIC,
    },
];

/// `(struct uftdi_softc *)self`.
fn uftdi_softc(self_: &Device) -> &'static UftdiSoftc {
    // SAFETY: only called with devices `uftdi_ca` made (uftdi's own entry points), whose
    // softc is a `UftdiSoftc`; attached devices live until `config_detach` frees them after
    // `uftdi_detach`.
    unsafe { &*ptr::from_ref(self_.softc::<UftdiSoftc>()) }
}

/// `(struct uftdi_softc *)vsc`: the softc `ucom` hands back (`ucom_attach_args.arg`).
fn uftdi_arg(vsc: *mut c_void) -> &'static UftdiSoftc {
    if vsc.is_null() {
        crate::kern::subr_prf::panic(format_args!("uftdi: ucom without its softc"));
    }
    // SAFETY: `uftdi_attach` passes its softc as `uca.arg`; it outlives the ucom child, which
    // `uftdi_detach` detaches first.
    unsafe { &*vsc.cast::<UftdiSoftc>().cast_const() }
}

/// `sc->sc_udev`. Set by `uftdi_attach` before the ucom child exists.
fn uftdi_udev(sc: &UftdiSoftc) -> &'static UsbdDevice {
    match sc.sc_udev.get() {
        Some(d) => d,
        None => crate::kern::subr_prf::panic(format_args!("{}: no USB device", sc.sc_dev.xname())),
    }
}

/// `uftdi_match`.
pub fn uftdi_match(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: `usbd_probe_and_attach` hands its `usb_attach_arg`, valid during the match.
    let uaa = unsafe { &*aux.cast::<UsbAttachArg>() };

    if uaa.iface.is_none() || uaa.configno != 1 {
        return UMATCH_NONE;
    }

    if usb_lookup(&UFTDI_DEVS, uaa.vendor as u16, uaa.product as u16).is_none() {
        return UMATCH_NONE;
    }

    // JTAG on USB interface 0
    if uaa.vendor == i32::from(USB_VENDOR_FTDI)
        && uaa.product == i32::from(USB_PRODUCT_FTDI_OPENRD)
        && uaa.ifaceno == 0
    {
        return UMATCH_NONE;
    }

    UMATCH_VENDOR_PRODUCT_CONF_IFACE
}

/// `uftdi_attach`.
pub fn uftdi_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    let sc = uftdi_softc(self_);
    // SAFETY: as in `uftdi_match`, valid during the attach.
    let uaa = unsafe { &*aux.cast::<UsbAttachArg>() };
    let devname = sc.sc_dev.xname();

    sc.sc_udev.set(uaa.device);
    sc.sc_iface.set(uaa.iface);
    let Some(iface) = uaa.iface else {
        return;
    };

    if uaa.release < 0x0200 {
        sc.sc_type.set(UftdiType::UFTDI_TYPE_SIO);
        sc.sc_hdrlen.set(1);
    } else if uaa.release == 0x0600 {
        sc.sc_type.set(UftdiType::UFTDI_TYPE_232R);
        sc.sc_hdrlen.set(0);
    } else if uaa.release == 0x0700 || uaa.release == 0x0800 {
        sc.sc_type.set(UftdiType::UFTDI_TYPE_2232H);
        sc.sc_hdrlen.set(0);
    } else {
        sc.sc_type.set(UftdiType::UFTDI_TYPE_8U232AM);
        sc.sc_hdrlen.set(0);
    }

    let mut bulkin = -1;
    let mut bulkout = -1;
    let mut ibufsize = 0;
    let mut obufsize = 0;
    let Some(id) = usbd_get_interface_descriptor(iface) else {
        usbd_deactivate(uftdi_udev(sc));
        return;
    };
    'bad: {
        for i in 0..id.bNumEndpoints {
            let Some(ed) = usbd_interface2endpoint_descriptor(iface, i) else {
                printf(format_args!(
                    "{}: could not read endpoint descriptor\n",
                    devname
                ));
                break 'bad;
            };

            let addr = i32::from(ed.bEndpointAddress);
            let dir = ue_get_dir(ed.bEndpointAddress);
            let attr = ue_get_xfertype(ed.bmAttributes);
            let mps = u32::from(ugetw(ed.wMaxPacketSize));
            if dir == UE_DIR_IN && attr == UE_BULK {
                bulkin = addr;
                ibufsize = if mps > 0 { mps } else { UFTDIIBUFSIZE };
            } else if dir == UE_DIR_OUT && attr == UE_BULK {
                bulkout = addr;
                obufsize = if mps > 0 { mps } else { UFTDIOBUFSIZE };
                obufsize -= sc.sc_hdrlen.get();
            } else {
                printf(format_args!("{}: unexpected endpoint\n", devname));
                break 'bad;
            }
        }
        if bulkin == -1 {
            printf(format_args!("{}: Could not find data bulk in\n", devname));
            break 'bad;
        }
        if bulkout == -1 {
            printf(format_args!("{}: Could not find data bulk out\n", devname));
            break 'bad;
        }

        let portno = FTDI_PIT_SIOA + i32::from(id.bInterfaceNumber);
        let mut uca = UcomAttachArgs {
            portno,
            // bulkin, bulkout set above
            bulkin,
            bulkout,
            uhidev: None,
            ibufsize,
            ibufsizepad: ibufsize,
            obufsize,
            opkthdrlen: sc.sc_hdrlen.get(),
            info: None,
            device: uaa.device,
            iface: uaa.iface,
            methods: &UFTDI_METHODS,
            arg: ptr::from_ref(sc).cast_mut().cast(),
        };

        sc.sc_subdev.set(config_found_sm(
            self_,
            ptr::from_mut(&mut uca).cast(),
            Some(ucomprint),
            Some(ucomsubmatch),
        ));

        return;
    }

    // bad:
    usbd_deactivate(uftdi_udev(sc));
}

/// `uftdi_detach`.
pub fn uftdi_detach(self_: &Device, flags: i32) -> Result<(), Errno> {
    let sc = uftdi_softc(self_);

    if let Some(subdev) = sc.sc_subdev.take() {
        // SAFETY: the ucom child `uftdi_attach` made, detached once and not used after.
        let _ = unsafe { config_detach(subdev, flags) };
    }

    Ok(())
}

/// A vendor request to the device with the given `wValue` and `wIndex`, no data.
fn uftdi_request(sc: &UftdiSoftc, request: u8, value: u16, index: u16) -> bool {
    let mut req = UsbDeviceRequest {
        bmRequestType: UT_WRITE_VENDOR_DEVICE,
        bRequest: request,
        ..UsbDeviceRequest::default()
    };
    usetw(&mut req.wValue, value);
    usetw(&mut req.wIndex, index);
    usetw(&mut req.wLength, 0);
    usbd_do_request(uftdi_udev(sc), &req, &mut []).is_err()
}

/// `uftdi_open`.
pub fn uftdi_open(vsc: *mut c_void, portno: i32) -> Result<(), Errno> {
    let sc = uftdi_arg(vsc);

    if usbd_is_dying(uftdi_udev(sc)) {
        return Err(Errno::EIO);
    }

    // Perform a full reset on the device
    if uftdi_request(sc, FTDI_SIO_RESET, FTDI_SIO_RESET_SIO, portno as u16) {
        return Err(Errno::EIO);
    }

    // Set 9600 baud, 2 stop bits, no parity, 8 bits
    let mut t = Termios::zeroed();
    t.c_ospeed = 9600;
    t.c_cflag = CSTOPB | CS8;
    let _ = uftdi_param(vsc, portno, &t);

    // Turn on RTS/CTS flow control
    let mut req = UsbDeviceRequest {
        bmRequestType: UT_WRITE_VENDOR_DEVICE,
        bRequest: FTDI_SIO_SET_FLOW_CTRL,
        ..UsbDeviceRequest::default()
    };
    usetw(&mut req.wValue, 0);
    usetw2(&mut req.wIndex, FTDI_SIO_RTS_CTS_HS, portno as u8);
    usetw(&mut req.wLength, 0);
    if usbd_do_request(uftdi_udev(sc), &req, &mut []).is_err() {
        return Err(Errno::EIO);
    }

    Ok(())
}

/// `uftdi_read`: the packet starts with the modem and line status; reports a change to ucom
/// and moves `data` past them.
pub fn uftdi_read(vsc: *mut c_void, _portno: i32, data: &mut &[u8]) {
    let sc = uftdi_arg(vsc);

    let msr = FTDI_GET_MSR(data);
    let lsr = FTDI_GET_LSR(data);

    if sc.sc_msr.get() != msr || (sc.sc_lsr.get() & FTDI_LSR_MASK) != (lsr & FTDI_LSR_MASK) {
        sc.sc_msr.set(msr);
        sc.sc_lsr.set(lsr);
        if let Some(subdev) = sc.sc_subdev.get() {
            // SAFETY: the ucom child `uftdi_attach` made, alive until `uftdi_detach`.
            ucom_status_change(ucom_softc_of(unsafe { subdev.as_ref() }));
        }
    }

    // Pick up status and adjust data part.
    *data = data.get(2..).unwrap_or(&[]);
}

/// `uftdi_write`: makes the packet for the `from` bytes in `to` (with the length tag of the
/// SIO chip) and answers its length.
pub fn uftdi_write(vsc: *mut c_void, portno: i32, to: &mut [u8], from: &[u8]) -> usize {
    let sc = uftdi_arg(vsc);
    let hdrlen = sc.sc_hdrlen.get() as usize;
    let count = from.len();

    // Make length tag and copy data
    if hdrlen > 0 {
        to[0] = FTDI_OUT_TAG(count as u32, portno);
    }

    to[hdrlen..hdrlen + count].copy_from_slice(from);
    count + hdrlen
}

/// `uftdi_set`.
pub fn uftdi_set(vsc: *mut c_void, portno: i32, reg: i32, onoff: i32) {
    let sc = uftdi_arg(vsc);

    let ctl = match reg {
        UCOM_SET_DTR => {
            if onoff != 0 {
                FTDI_SIO_SET_DTR_HIGH
            } else {
                FTDI_SIO_SET_DTR_LOW
            }
        }
        UCOM_SET_RTS => {
            if onoff != 0 {
                FTDI_SIO_SET_RTS_HIGH
            } else {
                FTDI_SIO_SET_RTS_LOW
            }
        }
        UCOM_SET_BREAK => {
            uftdi_break(vsc, portno, onoff);
            return;
        }
        _ => return,
    };
    let _ = uftdi_request(sc, FTDI_SIO_MODEM_CTRL, ctl, portno as u16);
}

/// `uftdi_param`.
pub fn uftdi_param(vsc: *mut c_void, portno: i32, t: &Termios) -> Result<(), Errno> {
    let sc = uftdi_arg(vsc);

    if usbd_is_dying(uftdi_udev(sc)) {
        return Err(Errno::EIO);
    }

    let rate = match sc.sc_type.get() {
        UftdiType::UFTDI_TYPE_SIO => match t.c_ospeed {
            300 => ftdi_sio_b300,
            600 => ftdi_sio_b600,
            1200 => ftdi_sio_b1200,
            2400 => ftdi_sio_b2400,
            4800 => ftdi_sio_b4800,
            9600 => ftdi_sio_b9600,
            19200 => ftdi_sio_b19200,
            38400 => ftdi_sio_b38400,
            57600 => ftdi_sio_b57600,
            115200 => ftdi_sio_b115200,
            _ => return Err(Errno::EINVAL),
        },
        UftdiType::UFTDI_TYPE_232R | UftdiType::UFTDI_TYPE_8U232AM => {
            uftdi_8u232am_getrate(sc, t.c_ospeed).ok_or(Errno::EINVAL)?
        }
        UftdiType::UFTDI_TYPE_2232H => uftdi_2232h_getrate(t.c_ospeed).ok_or(Errno::EINVAL)?,
    };
    if uftdi_request(
        sc,
        FTDI_SIO_SET_BAUD_RATE,
        rate as u16,
        (((rate >> 8) & 0xFF00) | portno) as u16,
    ) {
        return Err(Errno::EIO);
    }

    let mut data = if t.c_cflag & CSTOPB != 0 {
        FTDI_SIO_SET_DATA_STOP_BITS_2
    } else {
        FTDI_SIO_SET_DATA_STOP_BITS_1
    };
    if t.c_cflag & PARENB != 0 {
        if t.c_cflag & PARODD != 0 {
            data |= FTDI_SIO_SET_DATA_PARITY_ODD;
        } else {
            data |= FTDI_SIO_SET_DATA_PARITY_EVEN;
        }
    } else {
        data |= FTDI_SIO_SET_DATA_PARITY_NONE;
    }
    match t.c_cflag & CSIZE {
        CS5 => data |= FTDI_SIO_SET_DATA_BITS(5),
        CS6 => data |= FTDI_SIO_SET_DATA_BITS(6),
        CS7 => data |= FTDI_SIO_SET_DATA_BITS(7),
        CS8 => data |= FTDI_SIO_SET_DATA_BITS(8),
        _ => {}
    }
    sc.last_lcr.set(u32::from(data));

    if uftdi_request(sc, FTDI_SIO_SET_DATA, data, portno as u16) {
        return Err(Errno::EIO);
    }

    let (flow, value) = if t.c_cflag & CRTSCTS != 0 {
        (FTDI_SIO_RTS_CTS_HS, 0)
    } else if t.c_iflag & (IXON | IXOFF) != 0 {
        let mut w = [0u8; 2];
        usetw2(&mut w, t.c_cc[VSTOP as usize], t.c_cc[VSTART as usize]);
        (FTDI_SIO_XON_XOFF_HS, u16::from_le_bytes(w))
    } else {
        (FTDI_SIO_DISABLE_FLOW_CTRL, 0)
    };
    let mut index = [0u8; 2];
    usetw2(&mut index, flow, portno as u8);
    if uftdi_request(sc, FTDI_SIO_SET_FLOW_CTRL, value, u16::from_le_bytes(index)) {
        return Err(Errno::EIO);
    }

    Ok(())
}

/// `uftdi_get_status`.
pub fn uftdi_get_status(vsc: *mut c_void, _portno: i32, lsr: &mut u8, msr: &mut u8) {
    let sc = uftdi_arg(vsc);

    *msr = sc.sc_msr.get();
    *lsr = sc.sc_lsr.get();
}

/// `uftdi_break`.
pub fn uftdi_break(vsc: *mut c_void, portno: i32, onoff: i32) {
    let sc = uftdi_arg(vsc);

    let data = if onoff != 0 {
        sc.last_lcr.get() as u16 | FTDI_SIO_SET_BREAK
    } else {
        sc.last_lcr.get() as u16
    };

    let _ = uftdi_request(sc, FTDI_SIO_SET_DATA, data, portno as u16);
}

/// `uftdi_8u232am_getrate`: the divisor for `speed` as the 8U232AM and 232R chips take it, or
/// `None` where the speed is more than 3% off.
pub fn uftdi_8u232am_getrate(sc: &UftdiSoftc, speed: i32) -> Option<i32> {
    uftdi_8u232am_divisor(sc.sc_type.get(), speed)
}

/// `uftdi_8u232am_getrate`'s arithmetic for the chip `type_`.
fn uftdi_8u232am_divisor(type_: UftdiType, speed: i32) -> Option<i32> {
    // Table of the nearest even powers-of-2 for values 0..15.
    const ROUNDOFF: [u32; 16] = [0, 2, 2, 4, 4, 4, 8, 8, 8, 8, 8, 8, 16, 16, 16, 16];

    if speed <= 0 {
        return None;
    }
    let speed = speed as u32;

    // Special cases for 2M and 3M.
    if (3_000_000 * 100 / 103..=3_000_000 * 100 / 97).contains(&speed) {
        return Some(0);
    }
    if (2_000_000 * 100 / 103..=2_000_000 * 100 / 97).contains(&speed) {
        return Some(1);
    }

    let mut d = (FTDI_8U232AM_FREQ << 4) / speed;
    d = (d & !15) + ROUNDOFF[(d & 15) as usize];

    d = d.clamp(FTDI_8U232AM_MIN_DIV, FTDI_8U232AM_MAX_DIV);

    // Calculate the frequency needed for d to exactly divide down to our target speed, and
    // check that the actual frequency is within 3% of this.
    let freq = speed.wrapping_mul(d);
    if i64::from(freq) < i64::from(FTDI_8U232AM_FREQ << 4) * 100 / 103
        || i64::from(freq) > i64::from(FTDI_8U232AM_FREQ << 4) * 100 / 97
    {
        return None;
    }

    // Pack the divisor into the resultant value.  The lower 14-bits hold the integral part,
    // while the upper 2 bits encode the fractional component: either 0, 0.5, 0.25, or 0.125.
    let mut result = d >> 4;
    if type_ == UftdiType::UFTDI_TYPE_8U232AM {
        if d & 8 != 0 {
            result |= 0x4000;
        } else if d & 4 != 0 {
            result |= 0x8000;
        }
    }
    if d & 2 != 0 {
        result |= 0xc000;
    }

    Some(result as i32)
}

/// `uftdi_2232h_getrate`: the divisor for `speed` as the 2232H chips take it, or `None` where
/// the speed is more than 3% off.
pub fn uftdi_2232h_getrate(speed: i32) -> Option<i32> {
    const SUB: [i32; 8] = [0, 3, 2, 4, 1, 5, 6, 7];

    if speed <= 0 {
        return None;
    }
    let n = ((FTDI_2232H_FREQ << 3) / speed as u32) as i32;
    let s = n & 7;
    let mut result = (n >> 3) | (SUB[s as usize] << 14);

    // Special cases
    if result == 1 {
        result = 0;
    } else if result == 0x4001 {
        result = 1;
    }

    // Check if resulting baud rate is within 3%.
    if result != 0 {
        let div = ((result & 0x0000_3FFF) << 3) | s;
        if div == 0 {
            return None;
        }
        let resultspeed = ((FTDI_2232H_FREQ << 3) / div as u32) as i32;
        let accuracy = (i64::from(speed - resultspeed).abs() * 100) / i64::from(speed);
        if accuracy > 3 {
            return None;
        }
    }

    Some(result | 0x0002_0000) // Set this bit to turn off a divide by 2.5
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eight_u232am_divisors() {
        let r = |t, s| uftdi_8u232am_divisor(t, s);
        // 9600 baud: 48 MHz / 9600 = 5000 sixteenths: divisor 312 and a half (0x4000)
        assert_eq!(r(UftdiType::UFTDI_TYPE_8U232AM, 9600), Some(0x4138));
        // 115200 baud: 416 sixteenths: divisor 26, no fraction
        assert_eq!(r(UftdiType::UFTDI_TYPE_232R, 115_200), Some(26));
        // the 2M and 3M special cases
        assert_eq!(r(UftdiType::UFTDI_TYPE_8U232AM, 3_000_000), Some(0));
        assert_eq!(r(UftdiType::UFTDI_TYPE_8U232AM, 2_000_000), Some(1));
        // nonsense
        assert_eq!(r(UftdiType::UFTDI_TYPE_8U232AM, 0), None);
        assert_eq!(r(UftdiType::UFTDI_TYPE_8U232AM, -5), None);
    }

    #[test]
    fn two_two_three_two_h_divisors() {
        // 12 MHz * 8 / 9600 = 10000: divisor 1250, no fraction
        assert_eq!(uftdi_2232h_getrate(9600), Some(1250 | 0x0002_0000));
        assert_eq!(uftdi_2232h_getrate(0), None);
        assert_eq!(uftdi_2232h_getrate(-1), None);
    }

    #[test]
    fn table_has_the_c_entries() {
        assert_eq!(UFTDI_DEVS.len(), 628);
        assert_eq!(UFTDI_DEVS[0].ud_vendor, USB_VENDOR_ALTI2);
        assert!(
            UFTDI_DEVS
                .iter()
                .any(|d| d.ud_vendor == USB_VENDOR_FTDI && d.ud_product == USB_PRODUCT_FTDI_FT232RL)
        );
    }
}
/* </TESTS> */
