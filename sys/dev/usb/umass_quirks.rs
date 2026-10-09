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
 * Copyright (c) 2001 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by MAEKAWA Masahide (gehenna@NetBSD.org).
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

/*
 * Copyright (c) 2001 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by MAEKAWA Masahide (gehenna@NetBSD.org).
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
//! umass(4) quirks: `<dev/usb/umass_quirks.h>` (`struct umass_quirk`) and
//! `dev/usb/umass_quirks.c` (the table of devices that need a wire or command protocol, a
//! match level, an init or a fixup of their own, and `umass_lookup`).
//!
//! Upstream: sys/dev/usb/umass_quirks.h @ 3ce1f3f79392, sys/dev/usb/umass_quirks.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - The header and the file share this module.
//! - `uq_busquirks` keeps the C's `u_int32_t`; the SCSI quirks it holds (`ADEV_NOSENSE`,
//!   `SDEV_NOSYNCCACHE`) are `u16` here and widened in the table.
//! - `umass_fixup_sony` does nothing for an interface without a descriptor, where the C
//!   dereferences NULL (the attach has checked it already).
//! - `UMASS_DEBUG` is not in GENERIC: the `DPRINTF` of `umass_init_insystem` is absent.

use crate::dev::usb::umassvar::{
    UMASS_CPROTO_ATAPI, UMASS_CPROTO_ISD_ATA, UMASS_CPROTO_SCSI, UMASS_CPROTO_UFI,
    UMASS_CPROTO_UNSPEC, UMASS_QUIRK_WRONG_CSWSIG, UMASS_QUIRK_WRONG_CSWTAG, UMASS_WPROTO_BBB,
    UMASS_WPROTO_CBI, UMASS_WPROTO_CBI_I, UMASS_WPROTO_UNSPEC, UmassSoftc,
};
use crate::dev::usb::usb::{UT_READ_VENDOR_DEVICE, UsbDeviceRequest, ugetw, usetw};
use crate::dev::usb::usbdevs::*;
use crate::dev::usb::usbdi::{
    UMATCH_DEVCLASS_DEVSUBCLASS_DEVPROTO, UMATCH_NONE, UMATCH_VENDOR_PRODUCT,
    UMATCH_VENDOR_PRODUCT_REV, USBD_NORMAL_COMPLETION, UsbDevno, UsbdStatus, usb_lookup,
    usbd_do_request, usbd_get_device_descriptor, usbd_get_interface_descriptor, usbd_set_interface,
};
use crate::scsi::scsiconf::{ADEV_NOSENSE, SDEV_NOSYNCCACHE};

/// `umass_init_quirk`: a device's own initialisation, before the pipes are opened.
pub type UmassInitQuirk = fn(sc: &'static UmassSoftc) -> UsbdStatus;
/// `umass_fixup_quirk`: changes the wire or command protocol the table gives.
pub type UmassFixupQuirk = fn(sc: &'static UmassSoftc);

/// `struct umass_quirk`.
#[derive(Clone, Copy)]
pub struct UmassQuirk {
    /// `uq_dev`.
    pub uq_dev: UsbDevno,

    /// `uq_wire`: `UMASS_WPROTO_*`.
    pub uq_wire: u8,
    /// `uq_cmd`: `UMASS_CPROTO_*`.
    pub uq_cmd: u8,
    /// `uq_flags`: `UMASS_QUIRK_*`.
    pub uq_flags: u32,
    /// `uq_busquirks`: the SCSI link's quirks.
    pub uq_busquirks: u32,
    /// `uq_match`: the match level.
    pub uq_match: i32,

    /// `uq_init`.
    pub uq_init: Option<UmassInitQuirk>,
    /// `uq_fixup`.
    pub uq_fixup: Option<UmassFixupQuirk>,
}

impl AsRef<UsbDevno> for UmassQuirk {
    fn as_ref(&self) -> &UsbDevno {
        &self.uq_dev
    }
}

/// `umass_quirks[]`.
pub static UMASS_QUIRKS: [UmassQuirk; 54] = [
    UmassQuirk {
        uq_dev: UsbDevno {
            ud_vendor: USB_VENDOR_ATI,
            ud_product: USB_PRODUCT_ATI2_205,
        },
        uq_wire: UMASS_WPROTO_BBB,
        uq_cmd: UMASS_CPROTO_ISD_ATA,
        uq_flags: 0,
        uq_busquirks: 0,
        uq_match: UMATCH_VENDOR_PRODUCT,
        uq_init: None,
        uq_fixup: None,
    },
    UmassQuirk {
        uq_dev: UsbDevno {
            ud_vendor: USB_VENDOR_DMI,
            ud_product: USB_PRODUCT_DMI_SA2_0,
        },
        uq_wire: UMASS_WPROTO_UNSPEC,
        uq_cmd: UMASS_CPROTO_UNSPEC,
        uq_flags: 0,
        uq_busquirks: 0,
        uq_match: UMATCH_VENDOR_PRODUCT,
        uq_init: None,
        uq_fixup: None,
    },
    UmassQuirk {
        uq_dev: UsbDevno {
            ud_vendor: USB_VENDOR_DOMAIN,
            ud_product: USB_PRODUCT_DOMAIN_ROCKCHIP,
        },
        uq_wire: UMASS_WPROTO_UNSPEC,
        uq_cmd: UMASS_CPROTO_UNSPEC,
        uq_flags: UMASS_QUIRK_WRONG_CSWTAG,
        uq_busquirks: 0,
        uq_match: UMATCH_VENDOR_PRODUCT,
        uq_init: None,
        uq_fixup: None,
    },
    UmassQuirk {
        uq_dev: UsbDevno {
            ud_vendor: USB_VENDOR_EASYDISK,
            ud_product: USB_PRODUCT_EASYDISK_EASYDISK,
        },
        uq_wire: UMASS_WPROTO_UNSPEC,
        uq_cmd: UMASS_CPROTO_UNSPEC,
        uq_flags: 0,
        uq_busquirks: 0,
        uq_match: UMATCH_VENDOR_PRODUCT,
        uq_init: None,
        uq_fixup: None,
    },
    UmassQuirk {
        uq_dev: UsbDevno {
            ud_vendor: USB_VENDOR_FUJIPHOTO,
            ud_product: USB_PRODUCT_FUJIPHOTO_MASS0100,
        },
        uq_wire: UMASS_WPROTO_UNSPEC,
        uq_cmd: UMASS_CPROTO_UNSPEC,
        uq_flags: 0,
        uq_busquirks: ADEV_NOSENSE as u32,
        uq_match: UMATCH_DEVCLASS_DEVSUBCLASS_DEVPROTO,
        uq_init: None,
        uq_fixup: None,
    },
    UmassQuirk {
        uq_dev: UsbDevno {
            ud_vendor: USB_VENDOR_GENESYS,
            ud_product: USB_PRODUCT_GENESYS_GL641USB,
        },
        uq_wire: UMASS_WPROTO_UNSPEC,
        uq_cmd: UMASS_CPROTO_UNSPEC,
        uq_flags: 0,
        uq_busquirks: 0,
        uq_match: UMATCH_DEVCLASS_DEVSUBCLASS_DEVPROTO,
        uq_init: None,
        uq_fixup: None,
    },
    UmassQuirk {
        uq_dev: UsbDevno {
            ud_vendor: USB_VENDOR_HP,
            ud_product: USB_PRODUCT_HP_CDWRITERPLUS,
        },
        uq_wire: UMASS_WPROTO_CBI,
        uq_cmd: UMASS_CPROTO_ATAPI,
        uq_flags: 0,
        uq_busquirks: ADEV_NOSENSE as u32,
        uq_match: UMATCH_VENDOR_PRODUCT,
        uq_init: None,
        uq_fixup: None,
    },
    UmassQuirk {
        uq_dev: UsbDevno {
            ud_vendor: USB_VENDOR_IMATION,
            ud_product: USB_PRODUCT_IMATION_FLASHGO,
        },
        uq_wire: UMASS_WPROTO_UNSPEC,
        uq_cmd: UMASS_CPROTO_UNSPEC,
        uq_flags: 0,
        uq_busquirks: 0,
        uq_match: UMATCH_DEVCLASS_DEVSUBCLASS_DEVPROTO,
        uq_init: None,
        uq_fixup: None,
    },
    UmassQuirk {
        uq_dev: UsbDevno {
            ud_vendor: USB_VENDOR_INSYSTEM,
            ud_product: USB_PRODUCT_INSYSTEM_ADAPTERV2,
        },
        uq_wire: UMASS_WPROTO_BBB,
        uq_cmd: UMASS_CPROTO_ISD_ATA,
        uq_flags: 0,
        uq_busquirks: 0,
        uq_match: UMATCH_VENDOR_PRODUCT,
        uq_init: None,
        uq_fixup: None,
    },
    UmassQuirk {
        uq_dev: UsbDevno {
            ud_vendor: USB_VENDOR_INSYSTEM,
            ud_product: USB_PRODUCT_INSYSTEM_ATAPI,
        },
        uq_wire: UMASS_WPROTO_BBB,
        uq_cmd: UMASS_CPROTO_ISD_ATA,
        uq_flags: 0,
        uq_busquirks: 0,
        uq_match: UMATCH_VENDOR_PRODUCT,
        uq_init: None,
        uq_fixup: None,
    },
    UmassQuirk {
        uq_dev: UsbDevno {
            ud_vendor: USB_VENDOR_INSYSTEM,
            ud_product: USB_PRODUCT_INSYSTEM_DRIVEV2_5,
        },
        uq_wire: UMASS_WPROTO_BBB,
        uq_cmd: UMASS_CPROTO_ISD_ATA,
        uq_flags: 0,
        uq_busquirks: 0,
        uq_match: UMATCH_VENDOR_PRODUCT,
        uq_init: None,
        uq_fixup: None,
    },
    UmassQuirk {
        uq_dev: UsbDevno {
            ud_vendor: USB_VENDOR_INSYSTEM,
            ud_product: USB_PRODUCT_INSYSTEM_IDEUSB2,
        },
        uq_wire: UMASS_WPROTO_UNSPEC,
        uq_cmd: UMASS_CPROTO_UNSPEC,
        uq_flags: 0,
        uq_busquirks: 0,
        uq_match: UMATCH_VENDOR_PRODUCT,
        uq_init: None,
        uq_fixup: None,
    },
    UmassQuirk {
        uq_dev: UsbDevno {
            ud_vendor: USB_VENDOR_INSYSTEM,
            ud_product: USB_PRODUCT_INSYSTEM_USBCABLE,
        },
        uq_wire: UMASS_WPROTO_CBI,
        uq_cmd: UMASS_CPROTO_ATAPI,
        uq_flags: 0,
        uq_busquirks: 0,
        uq_match: UMATCH_VENDOR_PRODUCT,
        uq_init: Some(umass_init_insystem),
        uq_fixup: None,
    },
    UmassQuirk {
        uq_dev: UsbDevno {
            ud_vendor: USB_VENDOR_IODATA2,
            ud_product: USB_PRODUCT_IODATA2_USB2SC,
        },
        uq_wire: UMASS_WPROTO_UNSPEC,
        uq_cmd: UMASS_CPROTO_UNSPEC,
        uq_flags: 0,
        uq_busquirks: 0,
        uq_match: UMATCH_VENDOR_PRODUCT,
        uq_init: None,
        uq_fixup: None,
    },
    UmassQuirk {
        uq_dev: UsbDevno {
            ud_vendor: USB_VENDOR_IOMEGA,
            ud_product: USB_PRODUCT_IOMEGA_ZIP100,
        },
        uq_wire: UMASS_WPROTO_UNSPEC,
        uq_cmd: UMASS_CPROTO_UNSPEC,
        uq_flags: 0,
        uq_busquirks: 0,
        uq_match: UMATCH_DEVCLASS_DEVSUBCLASS_DEVPROTO,
        uq_init: None,
        uq_fixup: None,
    },
    UmassQuirk {
        uq_dev: UsbDevno {
            ud_vendor: USB_VENDOR_IOMEGA,
            ud_product: USB_PRODUCT_IOMEGA_ZIP250,
        },
        uq_wire: UMASS_WPROTO_UNSPEC,
        uq_cmd: UMASS_CPROTO_UNSPEC,
        uq_flags: 0,
        uq_busquirks: 0,
        uq_match: UMATCH_DEVCLASS_DEVSUBCLASS_DEVPROTO,
        uq_init: None,
        uq_fixup: None,
    },
    UmassQuirk {
        uq_dev: UsbDevno {
            ud_vendor: USB_VENDOR_IOMEGA,
            ud_product: USB_PRODUCT_IOMEGA_ZIP250_2,
        },
        uq_wire: UMASS_WPROTO_UNSPEC,
        uq_cmd: UMASS_CPROTO_UNSPEC,
        uq_flags: 0,
        uq_busquirks: 0,
        uq_match: UMATCH_DEVCLASS_DEVSUBCLASS_DEVPROTO,
        uq_init: None,
        uq_fixup: None,
    },
    UmassQuirk {
        uq_dev: UsbDevno {
            ud_vendor: USB_VENDOR_IRIVER,
            ud_product: USB_PRODUCT_IRIVER_IFP_1XX,
        },
        uq_wire: UMASS_WPROTO_UNSPEC,
        uq_cmd: UMASS_CPROTO_UNSPEC,
        uq_flags: 0,
        uq_busquirks: 0,
        uq_match: UMATCH_VENDOR_PRODUCT,
        uq_init: None,
        uq_fixup: None,
    },
    UmassQuirk {
        uq_dev: UsbDevno {
            ud_vendor: USB_VENDOR_IRIVER,
            ud_product: USB_PRODUCT_IRIVER_IFP_3XX,
        },
        uq_wire: UMASS_WPROTO_UNSPEC,
        uq_cmd: UMASS_CPROTO_UNSPEC,
        uq_flags: 0,
        uq_busquirks: 0,
        uq_match: UMATCH_VENDOR_PRODUCT,
        uq_init: None,
        uq_fixup: None,
    },
    UmassQuirk {
        uq_dev: UsbDevno {
            ud_vendor: USB_VENDOR_MELCO,
            ud_product: USB_PRODUCT_MELCO_DUBPXXG,
        },
        uq_wire: UMASS_WPROTO_UNSPEC,
        uq_cmd: UMASS_CPROTO_UNSPEC,
        uq_flags: 0,
        uq_busquirks: 0,
        uq_match: UMATCH_DEVCLASS_DEVSUBCLASS_DEVPROTO,
        uq_init: None,
        uq_fixup: None,
    },
    UmassQuirk {
        uq_dev: UsbDevno {
            ud_vendor: USB_VENDOR_MICROTECH,
            ud_product: USB_PRODUCT_MICROTECH_DPCM,
        },
        uq_wire: UMASS_WPROTO_CBI,
        uq_cmd: UMASS_CPROTO_ATAPI,
        uq_flags: 0,
        uq_busquirks: 0,
        uq_match: UMATCH_VENDOR_PRODUCT,
        uq_init: None,
        uq_fixup: None,
    },
    UmassQuirk {
        uq_dev: UsbDevno {
            ud_vendor: USB_VENDOR_MINOLTA,
            ud_product: USB_PRODUCT_MINOLTA_S304,
        },
        uq_wire: UMASS_WPROTO_UNSPEC,
        uq_cmd: UMASS_CPROTO_UNSPEC,
        uq_flags: 0,
        uq_busquirks: 0,
        uq_match: UMATCH_DEVCLASS_DEVSUBCLASS_DEVPROTO,
        uq_init: None,
        uq_fixup: None,
    },
    UmassQuirk {
        uq_dev: UsbDevno {
            ud_vendor: USB_VENDOR_MINOLTA,
            ud_product: USB_PRODUCT_MINOLTA_X,
        },
        uq_wire: UMASS_WPROTO_UNSPEC,
        uq_cmd: UMASS_CPROTO_UNSPEC,
        uq_flags: 0,
        uq_busquirks: 0,
        uq_match: UMATCH_DEVCLASS_DEVSUBCLASS_DEVPROTO,
        uq_init: None,
        uq_fixup: None,
    },
    UmassQuirk {
        uq_dev: UsbDevno {
            ud_vendor: USB_VENDOR_MINOLTA,
            ud_product: USB_PRODUCT_MINOLTA_DIMAGEA1,
        },
        uq_wire: UMASS_WPROTO_UNSPEC,
        uq_cmd: UMASS_CPROTO_UNSPEC,
        uq_flags: 0,
        uq_busquirks: 0,
        uq_match: UMATCH_DEVCLASS_DEVSUBCLASS_DEVPROTO,
        uq_init: None,
        uq_fixup: None,
    },
    UmassQuirk {
        uq_dev: UsbDevno {
            ud_vendor: USB_VENDOR_MSYSTEMS,
            ud_product: USB_PRODUCT_MSYSTEMS_DISKONKEY,
        },
        uq_wire: UMASS_WPROTO_UNSPEC,
        uq_cmd: UMASS_CPROTO_UNSPEC,
        uq_flags: 0,
        uq_busquirks: 0,
        uq_match: UMATCH_DEVCLASS_DEVSUBCLASS_DEVPROTO,
        uq_init: None,
        uq_fixup: None,
    },
    UmassQuirk {
        uq_dev: UsbDevno {
            ud_vendor: USB_VENDOR_MSYSTEMS,
            ud_product: USB_PRODUCT_MSYSTEMS_DISKONKEY2,
        },
        uq_wire: UMASS_WPROTO_UNSPEC,
        uq_cmd: UMASS_CPROTO_ATAPI,
        uq_flags: 0,
        uq_busquirks: 0,
        uq_match: UMATCH_DEVCLASS_DEVSUBCLASS_DEVPROTO,
        uq_init: None,
        uq_fixup: None,
    },
    UmassQuirk {
        uq_dev: UsbDevno {
            ud_vendor: USB_VENDOR_NEODIO,
            ud_product: USB_PRODUCT_NEODIO_ND3050,
        },
        uq_wire: UMASS_WPROTO_UNSPEC,
        uq_cmd: UMASS_CPROTO_UNSPEC,
        uq_flags: 0,
        uq_busquirks: 0,
        uq_match: UMATCH_VENDOR_PRODUCT,
        uq_init: None,
        uq_fixup: None,
    },
    UmassQuirk {
        uq_dev: UsbDevno {
            ud_vendor: USB_VENDOR_NEODIO,
            ud_product: USB_PRODUCT_NEODIO_ND5010,
        },
        uq_wire: UMASS_WPROTO_UNSPEC,
        uq_cmd: UMASS_CPROTO_UNSPEC,
        uq_flags: 0,
        uq_busquirks: 0,
        uq_match: UMATCH_VENDOR_PRODUCT,
        uq_init: None,
        uq_fixup: None,
    },
    UmassQuirk {
        uq_dev: UsbDevno {
            ud_vendor: USB_VENDOR_OLYMPUS,
            ud_product: USB_PRODUCT_OLYMPUS_C1,
        },
        uq_wire: UMASS_WPROTO_UNSPEC,
        uq_cmd: UMASS_CPROTO_UNSPEC,
        uq_flags: UMASS_QUIRK_WRONG_CSWSIG,
        uq_busquirks: 0,
        uq_match: UMATCH_DEVCLASS_DEVSUBCLASS_DEVPROTO,
        uq_init: None,
        uq_fixup: None,
    },
    UmassQuirk {
        uq_dev: UsbDevno {
            ud_vendor: USB_VENDOR_OLYMPUS,
            ud_product: USB_PRODUCT_OLYMPUS_C700,
        },
        uq_wire: UMASS_WPROTO_UNSPEC,
        uq_cmd: UMASS_CPROTO_UNSPEC,
        uq_flags: 0,
        uq_busquirks: SDEV_NOSYNCCACHE as u32,
        uq_match: UMATCH_DEVCLASS_DEVSUBCLASS_DEVPROTO,
        uq_init: None,
        uq_fixup: None,
    },
    UmassQuirk {
        uq_dev: UsbDevno {
            ud_vendor: USB_VENDOR_ONSPEC,
            ud_product: USB_PRODUCT_ONSPEC_MD1II,
        },
        uq_wire: UMASS_WPROTO_UNSPEC,
        uq_cmd: UMASS_CPROTO_UNSPEC,
        uq_flags: 0,
        uq_busquirks: 0,
        uq_match: UMATCH_DEVCLASS_DEVSUBCLASS_DEVPROTO,
        uq_init: None,
        uq_fixup: None,
    },
    UmassQuirk {
        uq_dev: UsbDevno {
            ud_vendor: USB_VENDOR_ONSPEC,
            ud_product: USB_PRODUCT_ONSPEC_MD2,
        },
        uq_wire: UMASS_WPROTO_UNSPEC,
        uq_cmd: UMASS_CPROTO_UNSPEC,
        uq_flags: 0,
        uq_busquirks: 0,
        uq_match: UMATCH_DEVCLASS_DEVSUBCLASS_DEVPROTO,
        uq_init: None,
        uq_fixup: None,
    },
    UmassQuirk {
        uq_dev: UsbDevno {
            ud_vendor: USB_VENDOR_OTI,
            ud_product: USB_PRODUCT_OTI_SOLID,
        },
        uq_wire: UMASS_WPROTO_UNSPEC,
        uq_cmd: UMASS_CPROTO_UNSPEC,
        uq_flags: 0,
        uq_busquirks: 0,
        uq_match: UMATCH_VENDOR_PRODUCT,
        uq_init: None,
        uq_fixup: None,
    },
    UmassQuirk {
        uq_dev: UsbDevno {
            ud_vendor: USB_VENDOR_PEN,
            ud_product: USB_PRODUCT_PEN_MOBILEDRIVE,
        },
        uq_wire: UMASS_WPROTO_UNSPEC,
        uq_cmd: UMASS_CPROTO_UNSPEC,
        uq_flags: 0,
        uq_busquirks: 0,
        uq_match: UMATCH_DEVCLASS_DEVSUBCLASS_DEVPROTO,
        uq_init: None,
        uq_fixup: None,
    },
    UmassQuirk {
        uq_dev: UsbDevno {
            ud_vendor: USB_VENDOR_PEN,
            ud_product: USB_PRODUCT_PEN_USBDISK,
        },
        uq_wire: UMASS_WPROTO_UNSPEC,
        uq_cmd: UMASS_CPROTO_UNSPEC,
        uq_flags: 0,
        uq_busquirks: 0,
        uq_match: UMATCH_DEVCLASS_DEVSUBCLASS_DEVPROTO,
        uq_init: None,
        uq_fixup: None,
    },
    UmassQuirk {
        uq_dev: UsbDevno {
            ud_vendor: USB_VENDOR_PEN,
            ud_product: USB_PRODUCT_PEN_USBREADER,
        },
        uq_wire: UMASS_WPROTO_UNSPEC,
        uq_cmd: UMASS_CPROTO_UNSPEC,
        uq_flags: 0,
        uq_busquirks: 0,
        uq_match: UMATCH_DEVCLASS_DEVSUBCLASS_DEVPROTO,
        uq_init: None,
        uq_fixup: None,
    },
    UmassQuirk {
        uq_dev: UsbDevno {
            ud_vendor: USB_VENDOR_PILOTECH,
            ud_product: USB_PRODUCT_PILOTECH_CRW600,
        },
        uq_wire: UMASS_WPROTO_UNSPEC,
        uq_cmd: UMASS_CPROTO_UNSPEC,
        uq_flags: 0,
        uq_busquirks: 0,
        uq_match: UMATCH_VENDOR_PRODUCT,
        uq_init: None,
        uq_fixup: None,
    },
    UmassQuirk {
        uq_dev: UsbDevno {
            ud_vendor: USB_VENDOR_PQI,
            ud_product: USB_PRODUCT_PQI_TRAVELFLASH,
        },
        uq_wire: UMASS_WPROTO_UNSPEC,
        uq_cmd: UMASS_CPROTO_UNSPEC,
        uq_flags: 0,
        uq_busquirks: 0,
        uq_match: UMATCH_VENDOR_PRODUCT,
        uq_init: None,
        uq_fixup: None,
    },
    UmassQuirk {
        uq_dev: UsbDevno {
            ud_vendor: USB_VENDOR_SCANLOGIC,
            ud_product: USB_PRODUCT_SCANLOGIC_SL11R,
        },
        uq_wire: UMASS_WPROTO_UNSPEC,
        uq_cmd: UMASS_CPROTO_UNSPEC,
        uq_flags: UMASS_QUIRK_WRONG_CSWTAG,
        uq_busquirks: 0,
        uq_match: UMATCH_VENDOR_PRODUCT,
        uq_init: None,
        uq_fixup: None,
    },
    UmassQuirk {
        uq_dev: UsbDevno {
            ud_vendor: USB_VENDOR_SHUTTLE,
            ud_product: USB_PRODUCT_SHUTTLE_EUSB,
        },
        uq_wire: UMASS_WPROTO_CBI_I,
        uq_cmd: UMASS_CPROTO_ATAPI,
        uq_flags: 0,
        uq_busquirks: ADEV_NOSENSE as u32,
        uq_match: UMATCH_VENDOR_PRODUCT,
        uq_init: Some(umass_init_shuttle),
        uq_fixup: None,
    },
    UmassQuirk {
        uq_dev: UsbDevno {
            ud_vendor: USB_VENDOR_SHUTTLE,
            ud_product: USB_PRODUCT_SHUTTLE_ZIOMMC,
        },
        uq_wire: UMASS_WPROTO_CBI_I,
        uq_cmd: UMASS_CPROTO_ATAPI,
        uq_flags: 0,
        uq_busquirks: 0,
        uq_match: UMATCH_VENDOR_PRODUCT,
        uq_init: None,
        uq_fixup: None,
    },
    UmassQuirk {
        uq_dev: UsbDevno {
            ud_vendor: USB_VENDOR_SIIG,
            ud_product: USB_PRODUCT_SIIG_MULTICARDREADER,
        },
        uq_wire: UMASS_WPROTO_UNSPEC,
        uq_cmd: UMASS_CPROTO_UNSPEC,
        uq_flags: 0,
        uq_busquirks: 0,
        uq_match: UMATCH_DEVCLASS_DEVSUBCLASS_DEVPROTO,
        uq_init: None,
        uq_fixup: None,
    },
    UmassQuirk {
        uq_dev: UsbDevno {
            ud_vendor: USB_VENDOR_SONY,
            ud_product: USB_PRODUCT_SONY_DRIVEV2,
        },
        uq_wire: UMASS_WPROTO_BBB,
        uq_cmd: UMASS_CPROTO_ISD_ATA,
        uq_flags: 0,
        uq_busquirks: 0,
        uq_match: UMATCH_VENDOR_PRODUCT,
        uq_init: None,
        uq_fixup: None,
    },
    UmassQuirk {
        uq_dev: UsbDevno {
            ud_vendor: USB_VENDOR_SONY,
            ud_product: USB_PRODUCT_SONY_DSC,
        },
        uq_wire: UMASS_WPROTO_UNSPEC,
        uq_cmd: UMASS_CPROTO_UNSPEC,
        uq_flags: 0,
        uq_busquirks: 0,
        uq_match: UMATCH_DEVCLASS_DEVSUBCLASS_DEVPROTO,
        uq_init: None,
        uq_fixup: Some(umass_fixup_sony),
    },
    UmassQuirk {
        uq_dev: UsbDevno {
            ud_vendor: USB_VENDOR_SONY,
            ud_product: USB_PRODUCT_SONY_MSC,
        },
        uq_wire: UMASS_WPROTO_CBI,
        uq_cmd: UMASS_CPROTO_UFI,
        uq_flags: 0,
        uq_busquirks: 0,
        uq_match: UMATCH_DEVCLASS_DEVSUBCLASS_DEVPROTO,
        uq_init: None,
        uq_fixup: None,
    },
    UmassQuirk {
        uq_dev: UsbDevno {
            ud_vendor: USB_VENDOR_TEAC,
            ud_product: USB_PRODUCT_TEAC_FD05PUB,
        },
        uq_wire: UMASS_WPROTO_UNSPEC,
        uq_cmd: UMASS_CPROTO_UNSPEC,
        uq_flags: 0,
        uq_busquirks: 0,
        uq_match: UMATCH_DEVCLASS_DEVSUBCLASS_DEVPROTO,
        uq_init: None,
        uq_fixup: None,
    },
    UmassQuirk {
        uq_dev: UsbDevno {
            ud_vendor: USB_VENDOR_TREK,
            ud_product: USB_PRODUCT_TREK_THUMBDRIVE_8MB,
        },
        uq_wire: UMASS_WPROTO_UNSPEC,
        uq_cmd: UMASS_CPROTO_UNSPEC,
        uq_flags: 0,
        uq_busquirks: 0,
        uq_match: UMATCH_DEVCLASS_DEVSUBCLASS_DEVPROTO,
        uq_init: None,
        uq_fixup: None,
    },
    UmassQuirk {
        uq_dev: UsbDevno {
            ud_vendor: USB_VENDOR_TRUMPION,
            ud_product: USB_PRODUCT_TRUMPION_XXX1100,
        },
        uq_wire: UMASS_WPROTO_CBI,
        uq_cmd: UMASS_CPROTO_ATAPI,
        uq_flags: 0,
        uq_busquirks: 0,
        uq_match: UMATCH_VENDOR_PRODUCT,
        uq_init: None,
        uq_fixup: None,
    },
    UmassQuirk {
        uq_dev: UsbDevno {
            ud_vendor: USB_VENDOR_YANO,
            ud_product: USB_PRODUCT_YANO_U640MO,
        },
        uq_wire: UMASS_WPROTO_CBI_I,
        uq_cmd: UMASS_CPROTO_ATAPI,
        uq_flags: 0,
        uq_busquirks: 0,
        uq_match: UMATCH_VENDOR_PRODUCT,
        uq_init: None,
        uq_fixup: None,
    },
    UmassQuirk {
        uq_dev: UsbDevno {
            ud_vendor: USB_VENDOR_YEDATA,
            ud_product: USB_PRODUCT_YEDATA_FLASHBUSTERU,
        },
        uq_wire: UMASS_WPROTO_UNSPEC,
        uq_cmd: UMASS_CPROTO_UFI,
        uq_flags: 0,
        uq_busquirks: 0,
        uq_match: UMATCH_VENDOR_PRODUCT_REV,
        uq_init: None,
        uq_fixup: Some(umass_fixup_yedata),
    },
    UmassQuirk {
        uq_dev: UsbDevno {
            ud_vendor: USB_VENDOR_SIGMATEL,
            ud_product: USB_PRODUCT_SIGMATEL_DNSSF7X,
        },
        uq_wire: UMASS_WPROTO_UNSPEC,
        uq_cmd: UMASS_CPROTO_UNSPEC,
        uq_flags: 0,
        uq_busquirks: SDEV_NOSYNCCACHE as u32,
        uq_match: UMATCH_VENDOR_PRODUCT,
        uq_init: None,
        uq_fixup: None,
    },
    UmassQuirk {
        uq_dev: UsbDevno {
            ud_vendor: USB_VENDOR_CREATIVE,
            ud_product: USB_PRODUCT_CREATIVE_NOMAD,
        },
        uq_wire: UMASS_WPROTO_UNSPEC,
        uq_cmd: UMASS_CPROTO_UNSPEC,
        uq_flags: 0,
        uq_busquirks: SDEV_NOSYNCCACHE as u32,
        uq_match: UMATCH_VENDOR_PRODUCT,
        uq_init: None,
        uq_fixup: None,
    },
    UmassQuirk {
        uq_dev: UsbDevno {
            ud_vendor: USB_VENDOR_SUPERTOP,
            ud_product: USB_PRODUCT_SUPERTOP_IDEBRIDGE,
        },
        uq_wire: UMASS_WPROTO_UNSPEC,
        uq_cmd: UMASS_CPROTO_UNSPEC,
        uq_flags: 0,
        uq_busquirks: ADEV_NOSENSE as u32,
        uq_match: UMATCH_VENDOR_PRODUCT,
        uq_init: None,
        uq_fixup: None,
    },
    UmassQuirk {
        uq_dev: UsbDevno {
            ud_vendor: USB_VENDOR_ERICSSON,
            ud_product: USB_PRODUCT_ERICSSON_F5521GW,
        },
        uq_wire: UMASS_WPROTO_UNSPEC,
        uq_cmd: UMASS_CPROTO_UNSPEC,
        uq_flags: 0,
        uq_busquirks: 0,
        uq_match: UMATCH_NONE,
        uq_init: None,
        uq_fixup: None,
    },
];

/// `umass_lookup`: the quirk entry of a vendor/product pair.
pub fn umass_lookup(vendor: u16, product: u16) -> Option<&'static UmassQuirk> {
    usb_lookup(&UMASS_QUIRKS, vendor, product)
}

/// `umass_init_insystem`: switches to alternate setting 1.
pub fn umass_init_insystem(sc: &'static UmassSoftc) -> UsbdStatus {
    let err = usbd_set_interface(sc.iface(), 1);
    if err.is_err() {
        return err;
    }

    USBD_NORMAL_COMPLETION
}

/// `umass_init_shuttle`: the vendor request the Linux driver sends.
pub fn umass_init_shuttle(sc: &'static UmassSoftc) -> UsbdStatus {
    let mut status = [0u8; 2];

    // The Linux driver does this
    let mut req = UsbDeviceRequest {
        bmRequestType: UT_READ_VENDOR_DEVICE,
        bRequest: 1,
        ..UsbDeviceRequest::default()
    };
    usetw(&mut req.wValue, 0);
    usetw(&mut req.wIndex, sc.sc_ifaceno.get() as u16);
    usetw(&mut req.wLength, status.len() as u16);

    usbd_do_request(sc.udev(), &req, &mut status)
}

/// `umass_fixup_sony`: picks UFI or SCSI for the vendor subclass of the Sony cameras.
pub fn umass_fixup_sony(sc: &'static UmassSoftc) {
    let Some(id) = usbd_get_interface_descriptor(sc.iface()) else {
        return;
    };
    if id.bInterfaceSubClass == 0xff {
        let dd = usbd_get_device_descriptor(sc.udev());
        // Many Sony DSC cameras share the same product ID, so the revision number is used
        // to distinguish between them.
        match ugetw(dd.bcdDevice) {
            // Sony DSC-T10, rev 6.11; DSC-W50, rev 6.00; DSC-P41, rev 5.00
            0x611 | 0x600 | 0x500 => sc.sc_cmd.set(UMASS_CPROTO_UFI),
            _ => sc.sc_cmd.set(UMASS_CPROTO_SCSI),
        }
    }
}

/// `umass_fixup_yedata`: revisions < 1.28 do not handle the interrupt endpoint very well.
pub fn umass_fixup_yedata(sc: &'static UmassSoftc) {
    let dd = usbd_get_device_descriptor(sc.udev());

    if ugetw(dd.bcdDevice) < 0x128 {
        sc.sc_wire.set(UMASS_WPROTO_CBI);
    } else {
        sc.sc_wire.set(UMASS_WPROTO_CBI_I);
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_has_the_c_entries() {
        assert_eq!(UMASS_QUIRKS.len(), 54);
    }

    #[test]
    fn lookup_finds_the_quirks() {
        let q = umass_lookup(USB_VENDOR_SONY, USB_PRODUCT_SONY_MSC).expect("sony msc");
        assert_eq!((q.uq_wire, q.uq_cmd), (UMASS_WPROTO_CBI, UMASS_CPROTO_UFI));
        assert_eq!(q.uq_match, UMATCH_DEVCLASS_DEVSUBCLASS_DEVPROTO);

        let q = umass_lookup(USB_VENDOR_SHUTTLE, USB_PRODUCT_SHUTTLE_EUSB).expect("eusb");
        assert_eq!(q.uq_busquirks, u32::from(ADEV_NOSENSE));
        assert!(q.uq_init.is_some() && q.uq_fixup.is_none());

        let q = umass_lookup(USB_VENDOR_OLYMPUS, USB_PRODUCT_OLYMPUS_C1).expect("c1");
        assert_eq!(q.uq_flags, UMASS_QUIRK_WRONG_CSWSIG);

        let q = umass_lookup(USB_VENDOR_ERICSSON, USB_PRODUCT_ERICSSON_F5521GW).expect("f5521");
        assert_eq!(q.uq_match, UMATCH_NONE);

        let q = umass_lookup(USB_VENDOR_YEDATA, USB_PRODUCT_YEDATA_FLASHBUSTERU).expect("ye");
        assert_eq!(q.uq_match, UMATCH_VENDOR_PRODUCT_REV);
        assert!(q.uq_fixup.is_some());
    }

    #[test]
    fn lookup_misses_other_devices() {
        // QEMU's usb-storage stick (vendor 0x46f4, product 0x0001) has no quirk.
        assert!(umass_lookup(0x46f4, 0x0001).is_none());
        // The vendor alone does not match.
        assert!(umass_lookup(USB_VENDOR_SONY, 0xffff).is_none());
    }
}
/* </TESTS> */
