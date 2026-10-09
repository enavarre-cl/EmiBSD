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
/* </LICENSES> */

/* <CODE> */
//! The USB stack: OpenBSD `sys/dev/usb/`.
//!
//! The machine-independent core (M12): the wire definitions and the bus driver (`usb`), the
//! driver interface (`usbdi`, `usbdi_util`), the shared structures (`usbdivar`), device
//! enumeration (`usb_subr`), DMA memory (`usb_mem`), quirks, IDs (`usbdevs`), the HID class
//! definitions (`usbhid`) and the capture headers (`usbpcap`). Host controller and device
//! drivers (`xhci`, `ehci`, `ohci`, `uhub`, `umass`, `uhidev`, ...) attach on top of it; `ukbdmap` holds the
//! keyboard layouts of `ukbd`.

pub mod ehci;
pub mod ehcireg;
pub mod ehcivar;
pub mod if_cdce;
pub mod if_cdcereg;
pub mod ohci;
pub mod ohcireg;
pub mod ohcivar;
pub mod uaudio;
pub mod ucom;
pub mod ucomvar;
pub mod uftdi;
pub mod uftdireg;
pub mod ugen;
pub mod uhci;
pub mod uhcireg;
pub mod uhcivar;
pub mod uhid;
pub mod uhid_rdesc;
pub mod uhidev;
pub mod uhub;
pub mod ukbd;
#[rustfmt::skip] // generated from the C, licence block verbatim (a trailing blank included)
pub mod ukbdmap;
pub mod umass;
pub mod umass_quirks;
pub mod umass_scsi;
pub mod umassvar;
pub mod ums;
#[allow(clippy::module_inception)] // OpenBSD's layout: sys/dev/usb/usb.c
pub mod usb;
pub mod usb_mem;
pub mod usb_quirks;
pub mod usb_subr;
pub mod usbcdc;
pub mod usbdevs;
pub mod usbdi;
pub mod usbdi_util;
pub mod usbdivar;
pub mod usbhid;
pub mod usbpcap;
pub mod uwacom;
pub mod xhci;
pub mod xhcireg;
pub mod xhcivar;
/* </CODE> */
