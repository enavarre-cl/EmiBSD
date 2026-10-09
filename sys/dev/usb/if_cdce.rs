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
 * Copyright (c) 1997, 1998, 1999, 2000-2003 Bill Paul <wpaul@windriver.com>
 * Copyright (c) 2003 Craig Boston
 * Copyright (c) 2004 Daniel Hartmeier
 * All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. All advertising materials mentioning features or use of this software
 *    must display the following acknowledgement:
 *	This product includes software developed by Bill Paul.
 * 4. Neither the name of the author nor the names of any co-contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY Bill Paul AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL Bill Paul, THE VOICES IN HIS HEAD OR
 * THE CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL,
 * EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO,
 * PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS;
 * OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
 * WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR
 * OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF
 * ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! cdce(4): USB Communication Device Class, Ethernet Networking Control Model (and Mobile
//! Direct Line Model) Ethernet adapters.
//!
//! Upstream: sys/dev/usb/if_cdce.c @ 3ce1f3f79392
//!
//! USB Communication Device Class (Ethernet Networking Control Model):
//! <https://www.usb.org/sites/default/files/CDC1.2_WMC1.1_012011.zip>
//!
//! `cdce_match` takes a device of the `cdce_devs[]` table, or an interface of the CDC class
//! with the Ethernet networking or mobile direct line subclass. `cdce_attach` reads the class
//! descriptors (the union descriptor names the data interface, the Ethernet descriptor the
//! string holding the MAC address), finds the interrupt endpoint of the control interface
//! and, walking the data interface's alternate settings, the first one with a bulk-in and a
//! bulk-out endpoint, then attaches an Ethernet interface. `cdce_init` opens the pipes and
//! starts the one receive transfer; `cdce_start` copies a frame into the one transmit
//! transfer (with a CRC appended for devices that want it) and `cdce_txeof` starts the next.
//!
//! ## Deviations
//! - `CDCE_DEBUG` is not in GENERIC: the `DPRINTF`s are absent. So is the decoding of the
//!   interrupt pipe's notifications in `cdce_intr`, whose every arm only prints under
//!   `CDCE_DEBUG`: the function keeps its status handling (a stalled pipe is cleared).
//! - `bpfilter` is configured (`NBPFILTER > 0`): `cdce_start` taps the frame with
//!   `bpf_mtap` as the C does.
//! - The class descriptors are read as bytes (`UsbWire`) from the configuration descriptor
//!   at the offset the iterator's descriptor sits; the C casts the pointer.
//! - `cdce_encap` stops a frame longer than the transfer buffer (`CDCE_BUFSZ`, the CRC
//!   included) at the buffer's end; the C overruns it (the interface's MTU keeps the frame
//!   within 1514 bytes, so this never happens).
//! - `cdce_rx_list_init` and `cdce_tx_list_init` answer `Result<(), Errno>` (`Err(ENOBUFS)`
//!   where the C returns `ENOBUFS`). `cdce_ioctl` is an `unsafe fn` of the `IfIoctlFn` type.

use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::slice;

use crate::dev::usb::if_cdcereg::{
    CDCE_BUFSZ, CDCE_CRC32, CDCE_SWAPUNION, CdceChain, CdceSoftc, CdceType,
};
use crate::dev::usb::usb::{
    UDESC_CS_INTERFACE, UE_BULK, UE_DIR_IN, UE_DIR_OUT, UE_INTERRUPT, UICLASS_CDC,
    UISUBCLASS_ETHERNET_NETWORKING_CONTROL_MODEL, UISUBCLASS_MOBILE_DIRECT_LINE_MODEL,
    USBD_SHORT_XFER_OK, UsbDescriptor, UsbStringDescriptor, UsbWire, ue_get_dir, ue_get_xfertype,
    ugetw,
};
use crate::dev::usb::usb_subr::{usbd_errstr, usbd_get_string_desc};
use crate::dev::usb::usbcdc::{
    UDESCSUB_CDC_ENF, UDESCSUB_CDC_UNION, UsbCdcEthernetDescriptor, UsbCdcNotification,
    UsbCdcUnionDescriptor,
};
use crate::dev::usb::usbdevs::{
    USB_PRODUCT_ACERLABS_M5632, USB_PRODUCT_AMBIT_NTL_250, USB_PRODUCT_COMPAQ_IPAQLINUX,
    USB_PRODUCT_GMATE_YP3X00, USB_PRODUCT_MOTOROLA2_USBLAN, USB_PRODUCT_MOTOROLA2_USBLAN2,
    USB_PRODUCT_PROLIFIC_PL2501, USB_PRODUCT_SHARP_A300, USB_PRODUCT_SHARP_C700,
    USB_PRODUCT_SHARP_C750, USB_PRODUCT_SHARP_SL5500, USB_PRODUCT_SHARP_SL5600,
    USB_VENDOR_ACERLABS, USB_VENDOR_AMBIT, USB_VENDOR_COMPAQ, USB_VENDOR_GMATE,
    USB_VENDOR_MOTOROLA2, USB_VENDOR_PROLIFIC, USB_VENDOR_SHARP,
};
use crate::dev::usb::usbdi::{
    UMATCH_IFACECLASS_GENERIC, UMATCH_NONE, UMATCH_VENDOR_PRODUCT, USBD_CANCELLED,
    USBD_DEFAULT_INTERVAL, USBD_EXCLUSIVE_USE, USBD_FORCE_SHORT_XFER, USBD_IN_PROGRESS,
    USBD_NO_COPY, USBD_NO_TIMEOUT, USBD_NORMAL_COMPLETION, USBD_NOT_STARTED, USBD_STALLED,
    UsbAttachArg, UsbDevno, UsbdStatus, splusb, usb_lookup, usbd_alloc_buffer, usbd_alloc_xfer,
    usbd_claim_iface, usbd_clear_endpoint_stall_async, usbd_close_pipe, usbd_deactivate,
    usbd_desc_iter_init, usbd_desc_iter_next, usbd_free_xfer, usbd_get_interface_descriptor,
    usbd_get_no_alts, usbd_get_xfer_status, usbd_iface_claimed, usbd_interface2endpoint_descriptor,
    usbd_is_dying, usbd_open_pipe, usbd_open_pipe_intr, usbd_set_interface, usbd_setup_xfer,
    usbd_transfer,
};
use crate::dev::usb::usbdivar::{UsbdDevice, UsbdPipe, UsbdXfer};
use crate::kern::subr_prf::{Str, panic, printf};
use crate::kern::uipc_mbuf::{m_adj, m_clget, m_copydata, m_freem, ml_enqueue};
use crate::machine::cpu::delay;
use crate::machine::intr::{splnet, splx};
use crate::net::bpf::{BPF_DIRECTION_OUT, bpf_mtap};
use crate::net::if_::{
    IFF_BROADCAST, IFF_MULTICAST, IFF_RUNNING, IFF_SIMPLEX, IFF_UP, IFNAMSIZ, if_attach, if_detach,
    if_input,
};
use crate::net::if_ethersubr::{
    ether_crc32_le, ether_fakeaddr, ether_ifattach, ether_ifdetach, ether_ioctl, ether_sprintf,
};
use crate::net::if_var::Ifnet;
use crate::net::ifq::{ifq_clr_oactive, ifq_dequeue, ifq_empty, ifq_is_oactive, ifq_set_oactive};
use crate::netinet::if_ether::{ETHER_ADDR_LEN, ETHER_ALIGN, ETHER_HDR_LEN};
use crate::sys::device::{CfMatch, Cfattach, Cfdriver, DV_IFNET, Device};
use crate::sys::errno::Errno;
use crate::sys::mbuf::{M_DONTWAIT, MCLBYTES, Mbuf, MbufList, mtod};
use crate::sys::sockio::{SIOCSIFADDR, SIOCSIFFLAGS};

/// `cdce_devs[]`: the devices that need a flag or that do not say they are CDC.
static CDCE_DEVS: [CdceType; 12] = [
    CdceType {
        cdce_dev: UsbDevno {
            ud_vendor: USB_VENDOR_ACERLABS,
            ud_product: USB_PRODUCT_ACERLABS_M5632,
        },
        cdce_flags: 0,
    },
    CdceType {
        cdce_dev: UsbDevno {
            ud_vendor: USB_VENDOR_PROLIFIC,
            ud_product: USB_PRODUCT_PROLIFIC_PL2501,
        },
        cdce_flags: 0,
    },
    CdceType {
        cdce_dev: UsbDevno {
            ud_vendor: USB_VENDOR_SHARP,
            ud_product: USB_PRODUCT_SHARP_SL5500,
        },
        cdce_flags: CDCE_CRC32,
    },
    CdceType {
        cdce_dev: UsbDevno {
            ud_vendor: USB_VENDOR_SHARP,
            ud_product: USB_PRODUCT_SHARP_A300,
        },
        cdce_flags: CDCE_CRC32,
    },
    CdceType {
        cdce_dev: UsbDevno {
            ud_vendor: USB_VENDOR_SHARP,
            ud_product: USB_PRODUCT_SHARP_SL5600,
        },
        cdce_flags: CDCE_CRC32,
    },
    CdceType {
        cdce_dev: UsbDevno {
            ud_vendor: USB_VENDOR_SHARP,
            ud_product: USB_PRODUCT_SHARP_C700,
        },
        cdce_flags: CDCE_CRC32,
    },
    CdceType {
        cdce_dev: UsbDevno {
            ud_vendor: USB_VENDOR_SHARP,
            ud_product: USB_PRODUCT_SHARP_C750,
        },
        cdce_flags: CDCE_CRC32,
    },
    CdceType {
        cdce_dev: UsbDevno {
            ud_vendor: USB_VENDOR_MOTOROLA2,
            ud_product: USB_PRODUCT_MOTOROLA2_USBLAN,
        },
        cdce_flags: CDCE_CRC32,
    },
    CdceType {
        cdce_dev: UsbDevno {
            ud_vendor: USB_VENDOR_MOTOROLA2,
            ud_product: USB_PRODUCT_MOTOROLA2_USBLAN2,
        },
        cdce_flags: CDCE_CRC32,
    },
    CdceType {
        cdce_dev: UsbDevno {
            ud_vendor: USB_VENDOR_GMATE,
            ud_product: USB_PRODUCT_GMATE_YP3X00,
        },
        cdce_flags: 0,
    },
    CdceType {
        cdce_dev: UsbDevno {
            ud_vendor: USB_VENDOR_COMPAQ,
            ud_product: USB_PRODUCT_COMPAQ_IPAQLINUX,
        },
        cdce_flags: 0,
    },
    CdceType {
        cdce_dev: UsbDevno {
            ud_vendor: USB_VENDOR_AMBIT,
            ud_product: USB_PRODUCT_AMBIT_NTL_250,
        },
        cdce_flags: CDCE_SWAPUNION,
    },
];

/// `cdce_cd`.
pub static CDCE_CD: Cfdriver = Cfdriver::new(b"cdce", DV_IFNET, 0);

/// `cdce_ca`.
pub static CDCE_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<CdceSoftc>(),
    ca_match: Some(cdce_match),
    ca_attach: cdce_attach,
    ca_detach: Some(cdce_detach),
    ca_activate: None,
};

/// `cdce_lookup(v, p)`.
fn cdce_lookup(vendor: i32, product: i32) -> Option<&'static CdceType> {
    usb_lookup(&CDCE_DEVS, vendor as u16, product as u16)
}

/// `(struct cdce_softc *)self`.
fn cdce_softc(self_: &Device) -> &'static CdceSoftc {
    // SAFETY: only called with devices `cdce_ca` made (cdce's own entry points), whose softc
    // is a `CdceSoftc`; attached devices live until `config_detach` frees them after
    // `cdce_detach`.
    unsafe { &*ptr::from_ref(self_.softc::<CdceSoftc>()) }
}

/// `(struct cdce_softc *)ifp->if_softc`.
fn cdce_ifp_softc(ifp: &Ifnet) -> &'static CdceSoftc {
    let p = ifp.if_softc.get().cast::<CdceSoftc>().cast_const();
    if p.is_null() {
        panic(format_args!("cdce: interface without its softc"));
    }
    // SAFETY: `cdce_attach` sets `if_softc` to its softc and installs cdce's functions only
    // on its own interface; softcs outlive their interfaces.
    unsafe { &*p }
}

/// `sc->cdce_udev`. Set by `cdce_attach` before any other entry point can run.
fn cdce_udev(sc: &CdceSoftc) -> &'static UsbdDevice {
    match sc.cdce_udev.get() {
        Some(d) => d,
        None => panic(format_args!("{}: no USB device", sc.cdce_dev.xname())),
    }
}

/// `(struct cdce_chain *)priv`: the chain an xfer's callback gets back.
fn cdce_priv_chain(priv_: *mut c_void) -> &'static CdceChain {
    if priv_.is_null() {
        panic(format_args!("cdce: xfer without its chain"));
    }
    // SAFETY: cdce sets up its xfers with one of its chains as `priv` (`cdce_encap`,
    // `cdce_init`, `cdce_rxeof`); the chains are inside the softc, which outlives them.
    unsafe { &*priv_.cast::<CdceChain>().cast_const() }
}

/// The chain's softc (`c->cdce_sc`).
fn cdce_chain_softc(c: &CdceChain) -> &'static CdceSoftc {
    let p = c.cdce_sc.get();
    if p.is_null() {
        panic(format_args!("cdce: chain without its softc"));
    }
    // SAFETY: the list init sets `cdce_sc` to the softc the chain is a part of.
    unsafe { &*p }
}

/// `(void *)c`: a chain as an xfer's `priv`.
fn cdce_chain_priv(c: &CdceChain) -> *mut c_void {
    ptr::from_ref(c).cast_mut().cast()
}

/// The bytes of the descriptor `desc`, which `usbd_desc_iter_next` returned for the
/// configuration descriptor `cdesc`.
fn cdce_desc_bytes<'a>(cdesc: &'a [u8], desc: &UsbDescriptor) -> &'a [u8] {
    let off = (ptr::from_ref(desc) as usize).wrapping_sub(cdesc.as_ptr() as usize);
    let end = off
        .saturating_add(usize::from(desc.bLength))
        .min(cdesc.len());
    cdesc.get(off..end).unwrap_or(&[])
}

/// The MAC address string of the Ethernet descriptor (twelve hexadecimal digits as UTF-16
/// characters) into `enaddr`, which the C ORs into a zeroed `ac_enaddr`.
fn cdce_eaddr_from_string(eaddr_str: &UsbStringDescriptor, enaddr: &mut [u8; ETHER_ADDR_LEN]) {
    for i in 0..ETHER_ADDR_LEN * 2 {
        let mut c = i32::from(ugetw(eaddr_str.bString[i]));

        if (i32::from(b'0')..=i32::from(b'9')).contains(&c) {
            c -= i32::from(b'0');
        } else if (i32::from(b'A')..=i32::from(b'F')).contains(&c) {
            c -= i32::from(b'A') - 10;
        } else if (i32::from(b'a')..=i32::from(b'f')).contains(&c) {
            c -= i32::from(b'a') - 10;
        }
        c &= 0xf;
        if i % 2 == 0 {
            c <<= 4;
        }
        enaddr[i / 2] |= c as u8;
    }
}

/// `cdce_match`.
pub fn cdce_match(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: `usbd_probe_and_attach` hands its `usb_attach_arg`, valid during the match.
    let uaa = unsafe { &*aux.cast::<UsbAttachArg>() };

    let Some(iface) = uaa.iface else {
        return UMATCH_NONE;
    };

    let Some(id) = usbd_get_interface_descriptor(iface) else {
        return UMATCH_NONE;
    };

    if cdce_lookup(uaa.vendor, uaa.product).is_some() {
        return UMATCH_VENDOR_PRODUCT;
    }

    if id.bInterfaceClass == UICLASS_CDC
        && (id.bInterfaceSubClass == UISUBCLASS_ETHERNET_NETWORKING_CONTROL_MODEL
            || id.bInterfaceSubClass == UISUBCLASS_MOBILE_DIRECT_LINE_MODEL)
    {
        return UMATCH_IFACECLASS_GENERIC;
    }

    UMATCH_NONE
}

/// `cdce_attach`.
pub fn cdce_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    let sc = cdce_softc(self_);
    // SAFETY: as in `cdce_match`, valid during the attach.
    let uaa = unsafe { &*aux.cast::<UsbAttachArg>() };
    let ifp = sc.get_ifp();
    let dev = uaa.device();
    let mut data_ifcno: i32 = -1;

    sc.cdce_udev.set(uaa.device);
    sc.cdce_ctl_iface.set(uaa.iface);
    let Some(ctl_iface) = uaa.iface else {
        return;
    };
    let Some(id) = usbd_get_interface_descriptor(ctl_iface) else {
        return;
    };
    let ctl_ifcno = i32::from(id.bInterfaceNumber);

    if let Some(t) = cdce_lookup(uaa.vendor, uaa.product) {
        sc.cdce_flags.set(t.cdce_flags);
    }

    // Get the data interface no. and capabilities
    let mut ethd: Option<UsbCdcEthernetDescriptor> = None;
    let cdesc = dev.cdesc.get().unwrap_or(&[]);
    let mut iter = usbd_desc_iter_init(dev);
    while let Some(desc) = usbd_desc_iter_next(&mut iter) {
        if desc.bDescriptorType != UDESC_CS_INTERFACE {
            continue;
        }
        match desc.bDescriptorSubtype {
            UDESCSUB_CDC_UNION => {
                let ud = UsbCdcUnionDescriptor::read_from(cdce_desc_bytes(cdesc, desc));
                if sc.cdce_flags.get() & CDCE_SWAPUNION == 0
                    && i32::from(ud.bMasterInterface) == ctl_ifcno
                {
                    data_ifcno = i32::from(ud.bSlaveInterface[0]);
                }
                if sc.cdce_flags.get() & CDCE_SWAPUNION != 0
                    && i32::from(ud.bSlaveInterface[0]) == ctl_ifcno
                {
                    data_ifcno = i32::from(ud.bMasterInterface);
                }
            }
            UDESCSUB_CDC_ENF => {
                if ethd.is_some() {
                    printf(format_args!("{}: ", sc.cdce_dev.xname()));
                    printf(format_args!("extra ethernet descriptor\n"));
                    return;
                }
                ethd = Some(UsbCdcEthernetDescriptor::read_from(cdce_desc_bytes(
                    cdesc, desc,
                )));
            }
            _ => {}
        }
    }

    if data_ifcno == -1 {
        sc.cdce_data_iface.set(sc.cdce_ctl_iface.get());
    } else {
        for (i, iface) in uaa.ifaces().iter().enumerate() {
            if usbd_iface_claimed(dev, i) {
                continue;
            }
            let id = usbd_get_interface_descriptor(iface);
            if id.is_some_and(|id| i32::from(id.bInterfaceNumber) == data_ifcno) {
                sc.cdce_data_iface.set(Some(iface));
                usbd_claim_iface(dev, i);
            }
        }
    }

    let Some(data_iface) = sc.cdce_data_iface.get() else {
        printf(format_args!("{}: no data interface\n", sc.cdce_dev.xname()));
        return;
    };

    sc.cdce_intr_no.set(-1);
    let mut i = 0;
    while i < id.bNumEndpoints && sc.cdce_intr_no.get() == -1 {
        let Some(ed) = usbd_interface2endpoint_descriptor(ctl_iface, i) else {
            printf(format_args!(
                "{}: no descriptor for interrupt endpoint {}\n",
                sc.cdce_dev.xname(),
                i
            ));
            return;
        };
        if ue_get_dir(ed.bEndpointAddress) == UE_DIR_IN
            && ue_get_xfertype(ed.bmAttributes) == UE_INTERRUPT
        {
            sc.cdce_intr_no.set(i32::from(ed.bEndpointAddress));
            sc.cdce_intr_size
                .set(size_of::<UsbCdcNotification>() as i32);
        }
        i += 1;
    }

    let Some(id) = usbd_get_interface_descriptor(data_iface) else {
        return;
    };
    let numalts = usbd_get_no_alts(cdesc, i32::from(id.bInterfaceNumber));

    let mut found = false;
    for j in 0..numalts {
        if usbd_set_interface(data_iface, j).is_err() {
            printf(format_args!(
                "{}: interface alternate setting {} failed\n",
                sc.cdce_dev.xname(),
                j
            ));
            return;
        }
        // Find endpoints.
        let Some(id) = usbd_get_interface_descriptor(data_iface) else {
            return;
        };
        sc.cdce_bulkin_no.set(-1);
        sc.cdce_bulkout_no.set(-1);
        for i in 0..id.bNumEndpoints {
            let Some(ed) = usbd_interface2endpoint_descriptor(data_iface, i) else {
                printf(format_args!(
                    "{}: no descriptor for bulk endpoint {}\n",
                    sc.cdce_dev.xname(),
                    i
                ));
                return;
            };
            if ue_get_dir(ed.bEndpointAddress) == UE_DIR_IN
                && ue_get_xfertype(ed.bmAttributes) == UE_BULK
            {
                sc.cdce_bulkin_no.set(i32::from(ed.bEndpointAddress));
            } else if ue_get_dir(ed.bEndpointAddress) == UE_DIR_OUT
                && ue_get_xfertype(ed.bmAttributes) == UE_BULK
            {
                sc.cdce_bulkout_no.set(i32::from(ed.bEndpointAddress));
            }
        }
        if sc.cdce_bulkin_no.get() != -1 && sc.cdce_bulkout_no.get() != -1 {
            found = true;
            break;
        }
    }

    if !found {
        if sc.cdce_bulkin_no.get() == -1 {
            printf(format_args!(
                "{}: could not find data bulk in\n",
                sc.cdce_dev.xname()
            ));
            return;
        }
        if sc.cdce_bulkout_no.get() == -1 {
            printf(format_args!(
                "{}: could not find data bulk out\n",
                sc.cdce_dev.xname()
            ));
            return;
        }
    }

    // found:
    let s = splnet();

    let mut eaddr_str = UsbStringDescriptor::zeroed();
    let mut len = 0;
    let have_eaddr = ethd.is_some_and(|ethd| {
        usbd_get_string_desc(
            dev,
            i32::from(ethd.iMacAddress),
            0,
            &mut eaddr_str,
            &mut len,
        ) == USBD_NORMAL_COMPLETION
    });
    if have_eaddr {
        let mut enaddr = sc.cdce_arpcom.ac_enaddr.get();
        cdce_eaddr_from_string(&eaddr_str, &mut enaddr);
        sc.cdce_arpcom.ac_enaddr.set(enaddr);
    } else {
        ether_fakeaddr(ifp);
    }

    printf(format_args!(
        "{}: address {}\n",
        sc.cdce_dev.xname(),
        Str(&ether_sprintf(&sc.cdce_arpcom.ac_enaddr.get()))
    ));

    ifp.if_softc.set(ptr::from_ref(sc).cast_mut().cast());
    ifp.if_flags
        .set(IFF_BROADCAST | IFF_SIMPLEX | IFF_MULTICAST);
    ifp.if_ioctl.set(Some(cdce_ioctl));
    ifp.if_start.set(Some(cdce_start));
    ifp.if_watchdog.set(Some(cdce_watchdog));
    let mut xname = [0u8; IFNAMSIZ];
    let name = sc.cdce_dev.xname().as_bytes();
    let n = name.len().min(IFNAMSIZ - 1);
    xname[..n].copy_from_slice(&name[..n]);
    ifp.if_xname.set(xname);

    if_attach(ifp);
    ether_ifattach(&sc.cdce_arpcom);

    sc.cdce_attached.set(1);
    splx(s);
}

/// `cdce_detach`.
pub fn cdce_detach(self_: &Device, _flags: i32) -> Result<(), Errno> {
    let sc = cdce_softc(self_);
    let ifp = sc.get_ifp();

    if sc.cdce_attached.get() == 0 {
        return Ok(());
    }

    let s = splusb();

    if ifp.if_flags.get() & IFF_RUNNING != 0 {
        cdce_stop(sc);
    }

    if !ifp.if_softc.get().is_null() {
        ether_ifdetach(ifp);
        if_detach(ifp);
    }

    sc.cdce_attached.set(0);
    splx(s);

    Ok(())
}

/// `cdce_start`.
pub fn cdce_start(ifp: &'static Ifnet) {
    let sc = cdce_ifp_softc(ifp);

    if usbd_is_dying(cdce_udev(sc)) || ifq_is_oactive(&ifp.if_snd) {
        return;
    }

    let Some(m_head) = ifq_dequeue(&ifp.if_snd) else {
        return;
    };

    if cdce_encap(sc, m_head, 0).is_err() {
        m_freem(m_head);
        ifq_set_oactive(&ifp.if_snd);
        return;
    }

    let bpf = ifp.if_bpf.get();
    if !bpf.is_null() {
        let _ = bpf_mtap(bpf, m_head, BPF_DIRECTION_OUT);
    }

    ifq_set_oactive(&ifp.if_snd);

    ifp.if_timer.set(6);
}

/// `cdce_encap`: copies the frame `m` into transmit chain `idx` and starts the transfer.
pub fn cdce_encap(sc: &'static CdceSoftc, m: &'static Mbuf, idx: usize) -> Result<(), Errno> {
    let c = &sc.cdce_cdata.cdce_tx_chain[idx];
    let (Some(xfer), Some(pipe)) = (c.cdce_xfer.get(), sc.cdce_bulkout_pipe.get()) else {
        return Err(Errno::EIO);
    };
    let crc32 = sc.cdce_flags.get() & CDCE_CRC32 != 0;
    let extra = if crc32 { 4 } else { 0 };
    let len = (m.m_pkthdr().len.get().max(0) as usize).min(CDCE_BUFSZ - extra);

    // SAFETY: `cdce_buf` is the transfer's DMA buffer of `CDCE_BUFSZ` bytes
    // (`cdce_tx_list_init`); `len + extra` is within it, and nothing else uses the buffer
    // while no transmit is in progress (`cdce_start` runs one at a time).
    let buf = unsafe { slice::from_raw_parts_mut(c.cdce_buf.get(), len + extra) };
    m_copydata(m, 0, &mut buf[..len]);
    if crc32 {
        // Some devices want a 32-bit CRC appended to every frame
        let crc = ether_crc32_le(&buf[..len]) ^ !0u32;
        buf[len..len + 4].copy_from_slice(&crc.to_ne_bytes());
    }
    c.cdce_mbuf.set(Some(m));

    // SAFETY: the buffer is the xfer's own DMA buffer (`USBD_NO_COPY`), `len + extra` bytes
    // of it, valid until the transfer completes.
    unsafe {
        usbd_setup_xfer(
            xfer,
            pipe,
            cdce_chain_priv(c),
            c.cdce_buf.get(),
            (len + extra) as u32,
            USBD_FORCE_SHORT_XFER | USBD_NO_COPY,
            10000,
            Some(cdce_txeof),
        );
    }
    let err = usbd_transfer(xfer);
    if err != USBD_IN_PROGRESS {
        c.cdce_mbuf.set(None);
        cdce_stop(sc);
        return Err(Errno::EIO);
    }

    sc.cdce_cdata
        .cdce_tx_cnt
        .set(sc.cdce_cdata.cdce_tx_cnt.get() + 1);

    Ok(())
}

/// `usbd_close_pipe` of a pipe taken off the softc, with the C's message on failure.
fn cdce_close_pipe(sc: &CdceSoftc, pipe: &'static UsbdPipe, what: &str) {
    // SAFETY: the pipe was opened by `cdce_init` and nothing uses it once it is off the
    // softc.
    let err = unsafe { usbd_close_pipe(NonNull::from(pipe)) };
    if err.is_err() {
        printf(format_args!(
            "{}: close {} pipe failed: {}\n",
            sc.cdce_dev.xname(),
            what,
            usbd_errstr(err)
        ));
    }
}

/// `cdce_stop`.
pub fn cdce_stop(sc: &'static CdceSoftc) {
    let ifp = sc.get_ifp();

    ifp.if_timer.set(0);
    ifp.if_flags.set(ifp.if_flags.get() & !IFF_RUNNING);
    ifq_clr_oactive(&ifp.if_snd);

    if let Some(p) = sc.cdce_bulkin_pipe.take() {
        cdce_close_pipe(sc, p, "rx");
    }

    if let Some(p) = sc.cdce_bulkout_pipe.take() {
        cdce_close_pipe(sc, p, "tx");
    }

    if let Some(p) = sc.cdce_intr_pipe.take() {
        cdce_close_pipe(sc, p, "interrupt");
    }

    for c in sc
        .cdce_cdata
        .cdce_rx_chain
        .iter()
        .chain(sc.cdce_cdata.cdce_tx_chain.iter())
    {
        if let Some(m) = c.cdce_mbuf.take() {
            m_freem(m);
        }
        if let Some(x) = c.cdce_xfer.take() {
            // SAFETY: allocated by the list init; the pipes are closed (their queues
            // aborted), so the xfer is not queued, and nothing uses it once it is off the
            // chain. Its DMA buffer goes with it.
            unsafe { usbd_free_xfer(NonNull::from(x)) };
            c.cdce_buf.set(ptr::null_mut());
        }
    }
}

/// `cdce_ioctl`.
///
/// # Safety
///
/// As for `IfIoctlFn` (`net/if_var.rs`): `data` is the kernel copy of the request the
/// command encodes.
pub unsafe fn cdce_ioctl(ifp: &'static Ifnet, command: u64, data: *mut u8) -> Result<(), Errno> {
    let sc = cdce_ifp_softc(ifp);
    let mut error = Ok(());

    if usbd_is_dying(cdce_udev(sc)) {
        return Err(Errno::ENXIO);
    }

    let s = splnet();

    match command {
        SIOCSIFADDR => {
            ifp.if_flags.set(ifp.if_flags.get() | IFF_UP);
            if ifp.if_flags.get() & IFF_RUNNING == 0 {
                cdce_init(sc);
            }
        }
        SIOCSIFFLAGS => {
            if ifp.if_flags.get() & IFF_UP != 0 {
                if ifp.if_flags.get() & IFF_RUNNING != 0 {
                    error = Err(Errno::ENETRESET);
                } else {
                    cdce_init(sc);
                }
            } else if ifp.if_flags.get() & IFF_RUNNING != 0 {
                cdce_stop(sc);
            }
        }
        _ => {
            // SAFETY: this function's contract, forwarded.
            error = unsafe { ether_ioctl(ifp, &sc.cdce_arpcom, command, data) };
        }
    }

    if error == Err(Errno::ENETRESET) {
        error = Ok(());
    }

    splx(s);
    error
}

/// `cdce_watchdog`.
pub fn cdce_watchdog(ifp: &'static Ifnet) {
    let sc = cdce_ifp_softc(ifp);

    if usbd_is_dying(cdce_udev(sc)) {
        return;
    }

    ifp.if_oerrors().set(ifp.if_oerrors().get() + 1);
    printf(format_args!("{}: watchdog timeout\n", sc.cdce_dev.xname()));
}

/// `cdce_init`.
pub fn cdce_init(sc: &'static CdceSoftc) {
    let ifp = sc.get_ifp();

    let s = splnet();

    if sc.cdce_intr_no.get() != -1 && sc.cdce_intr_pipe.get().is_none() {
        let Some(ctl_iface) = sc.cdce_ctl_iface.get() else {
            splx(s);
            return;
        };
        // SAFETY: `cdce_intr_buf` is inside the softc, which outlives the pipe: `cdce_stop`
        // (and so `cdce_detach`) closes the pipe before the softc is freed.
        let r = unsafe {
            usbd_open_pipe_intr(
                ctl_iface,
                sc.cdce_intr_no.get() as u8,
                USBD_SHORT_XFER_OK as u8,
                ptr::from_ref(sc).cast_mut().cast(),
                sc.cdce_intr_buf.as_ptr().cast(),
                sc.cdce_intr_size.get() as u32,
                cdce_intr,
                USBD_DEFAULT_INTERVAL,
            )
        };
        match r {
            Ok(p) => sc.cdce_intr_pipe.set(Some(p)),
            Err(err) => {
                printf(format_args!(
                    "{}: open interrupt pipe failed: {}\n",
                    sc.cdce_dev.xname(),
                    usbd_errstr(err)
                ));
                splx(s);
                return;
            }
        }
    }

    if cdce_tx_list_init(sc).is_err() {
        printf(format_args!(
            "{}: tx list init failed\n",
            sc.cdce_dev.xname()
        ));
        splx(s);
        return;
    }

    if cdce_rx_list_init(sc).is_err() {
        printf(format_args!(
            "{}: rx list init failed\n",
            sc.cdce_dev.xname()
        ));
        splx(s);
        return;
    }

    // Maybe set multicast / broadcast here???

    let Some(data_iface) = sc.cdce_data_iface.get() else {
        splx(s);
        return;
    };
    match usbd_open_pipe(
        data_iface,
        sc.cdce_bulkin_no.get() as u8,
        USBD_EXCLUSIVE_USE,
    ) {
        Ok(p) => sc.cdce_bulkin_pipe.set(Some(p)),
        Err(err) => {
            printf(format_args!(
                "{}: open rx pipe failed: {}\n",
                sc.cdce_dev.xname(),
                usbd_errstr(err)
            ));
            splx(s);
            return;
        }
    }

    match usbd_open_pipe(
        data_iface,
        sc.cdce_bulkout_no.get() as u8,
        USBD_EXCLUSIVE_USE,
    ) {
        Ok(p) => sc.cdce_bulkout_pipe.set(Some(p)),
        Err(err) => {
            printf(format_args!(
                "{}: open tx pipe failed: {}\n",
                sc.cdce_dev.xname(),
                usbd_errstr(err)
            ));
            splx(s);
            return;
        }
    }

    for c in &sc.cdce_cdata.cdce_rx_chain {
        cdce_rx_start(c);
    }

    ifp.if_flags.set(ifp.if_flags.get() | IFF_RUNNING);
    ifq_clr_oactive(&ifp.if_snd);

    splx(s);
}

/// The `usbd_setup_xfer` and `usbd_transfer` that start (or restart) a receive transfer.
fn cdce_rx_start(c: &'static CdceChain) {
    let sc = cdce_chain_softc(c);
    let (Some(xfer), Some(pipe)) = (c.cdce_xfer.get(), sc.cdce_bulkin_pipe.get()) else {
        return;
    };
    // SAFETY: `cdce_buf` is the xfer's own DMA buffer of `CDCE_BUFSZ` bytes
    // (`cdce_rx_list_init`), valid until the transfer completes (`USBD_NO_COPY`).
    unsafe {
        usbd_setup_xfer(
            xfer,
            pipe,
            cdce_chain_priv(c),
            c.cdce_buf.get(),
            CDCE_BUFSZ as u32,
            USBD_SHORT_XFER_OK | USBD_NO_COPY,
            USBD_NO_TIMEOUT,
            Some(cdce_rxeof),
        );
    }
    let _ = usbd_transfer(xfer);
}

/// `cdce_newbuf`: gives chain `c` a cluster mbuf to receive into (a new one, or `m` reset).
pub fn cdce_newbuf(sc: &CdceSoftc, c: &CdceChain, m: Option<&'static Mbuf>) -> Result<(), Errno> {
    let m_new = match m {
        None => {
            let Some(m_new) = m_clget(None, M_DONTWAIT, MCLBYTES as u32) else {
                printf(format_args!(
                    "{}: no memory for rx list -- packet dropped!\n",
                    sc.cdce_dev.xname()
                ));
                return Err(Errno::ENOBUFS);
            };
            m_new.m_len().set(MCLBYTES as u32);
            m_new.m_pkthdr().len.set(MCLBYTES as i32);
            m_new
        }
        Some(m) => {
            m.m_len().set(MCLBYTES as u32);
            m.m_pkthdr().len.set(MCLBYTES as i32);
            m.m_data().set(m.m_ext().ext_buf.get());
            m
        }
    };

    m_adj(m_new, ETHER_ALIGN as i32);
    c.cdce_mbuf.set(Some(m_new));
    Ok(())
}

/// `cdce_rx_list_init`.
pub fn cdce_rx_list_init(sc: &'static CdceSoftc) -> Result<(), Errno> {
    for (i, c) in sc.cdce_cdata.cdce_rx_chain.iter().enumerate() {
        c.cdce_sc.set(ptr::from_ref(sc));
        c.cdce_idx.set(i as i32);
        cdce_newbuf(sc, c, None)?;
        if c.cdce_xfer.get().is_none() {
            let xfer = usbd_alloc_xfer(cdce_udev(sc));
            c.cdce_xfer.set(xfer);
            let Some(xfer) = xfer else {
                return Err(Errno::ENOBUFS);
            };
            let Some(buf) = usbd_alloc_buffer(xfer, CDCE_BUFSZ as u32) else {
                return Err(Errno::ENOBUFS);
            };
            c.cdce_buf.set(buf.as_ptr());
        }
    }

    Ok(())
}

/// `cdce_tx_list_init`.
pub fn cdce_tx_list_init(sc: &'static CdceSoftc) -> Result<(), Errno> {
    for (i, c) in sc.cdce_cdata.cdce_tx_chain.iter().enumerate() {
        c.cdce_sc.set(ptr::from_ref(sc));
        c.cdce_idx.set(i as i32);
        c.cdce_mbuf.set(None);
        if c.cdce_xfer.get().is_none() {
            let xfer = usbd_alloc_xfer(cdce_udev(sc));
            c.cdce_xfer.set(xfer);
            let Some(xfer) = xfer else {
                return Err(Errno::ENOBUFS);
            };
            let Some(buf) = usbd_alloc_buffer(xfer, CDCE_BUFSZ as u32) else {
                return Err(Errno::ENOBUFS);
            };
            c.cdce_buf.set(buf.as_ptr());
        }
    }

    Ok(())
}

/// `cdce_rxeof`: a receive transfer completed; hands the frame to the stack and starts the
/// next transfer.
pub fn cdce_rxeof(xfer: &'static UsbdXfer, priv_: *mut c_void, status: UsbdStatus) {
    let c = cdce_priv_chain(priv_);
    let sc = cdce_chain_softc(c);
    let ifp = sc.get_ifp();

    if usbd_is_dying(cdce_udev(sc)) || ifp.if_flags.get() & IFF_RUNNING == 0 {
        return;
    }

    'done: {
        if status != USBD_NORMAL_COMPLETION {
            if status == USBD_NOT_STARTED || status == USBD_CANCELLED {
                return;
            }
            if sc.cdce_rxeof_errors.get() == 0 {
                printf(format_args!(
                    "{}: usb error on rx: {}\n",
                    sc.cdce_dev.xname(),
                    usbd_errstr(status)
                ));
            }
            if status == USBD_STALLED
                && let Some(pipe) = sc.cdce_bulkin_pipe.get()
            {
                let _ = usbd_clear_endpoint_stall_async(pipe);
            }
            delay((sc.cdce_rxeof_errors.get() * 10000) as u32);
            let errors = sc.cdce_rxeof_errors.get();
            sc.cdce_rxeof_errors.set(errors + 1);
            if errors > 10 {
                printf(format_args!(
                    "{}: too many errors, disabling\n",
                    sc.cdce_dev.xname()
                ));
                usbd_deactivate(cdce_udev(sc));
                return;
            }
            break 'done;
        }

        sc.cdce_rxeof_errors.set(0);

        let mut count = 0;
        usbd_get_xfer_status(xfer, None, None, Some(&mut count), None);
        let mut total_len = count as i32;
        if sc.cdce_flags.get() & CDCE_CRC32 != 0 {
            total_len -= 4; // Strip off added CRC
        }
        if total_len <= 1 {
            break 'done;
        }

        let Some(m) = c.cdce_mbuf.get() else {
            break 'done;
        };
        let n = (total_len as usize).min(CDCE_BUFSZ);
        // SAFETY: `cdce_buf` is the xfer's DMA buffer of `CDCE_BUFSZ` bytes, holding the
        // `total_len` bytes just received (`n` is within it); the mbuf's cluster is
        // `MCLBYTES` minus `ETHER_ALIGN` bytes, more than `CDCE_BUFSZ`; they do not overlap.
        unsafe { ptr::copy_nonoverlapping(c.cdce_buf.get().cast_const(), mtod::<u8>(m), n) };

        if (total_len as usize) < ETHER_HDR_LEN {
            ifp.if_ierrors().set(ifp.if_ierrors().get() + 1);
            break 'done;
        }

        m.m_pkthdr().len.set(total_len);
        m.m_len().set(total_len as u32);
        let ml = MbufList::new();
        ml_enqueue(&ml, m);

        if cdce_newbuf(sc, c, None).is_err() {
            ifp.if_ierrors().set(ifp.if_ierrors().get() + 1);
            break 'done;
        }

        let s = splnet();
        if_input(ifp, &ml);
        splx(s);
    }

    // done: Setup new transfer.
    cdce_rx_start(c);
}

/// `cdce_txeof`: a transmit transfer completed; frees the frame and starts the next one.
pub fn cdce_txeof(xfer: &'static UsbdXfer, priv_: *mut c_void, status: UsbdStatus) {
    let c = cdce_priv_chain(priv_);
    let sc = cdce_chain_softc(c);
    let ifp = sc.get_ifp();

    if usbd_is_dying(cdce_udev(sc)) {
        return;
    }

    let s = splnet();

    ifp.if_timer.set(0);
    ifq_clr_oactive(&ifp.if_snd);

    if status != USBD_NORMAL_COMPLETION {
        if status == USBD_NOT_STARTED || status == USBD_CANCELLED {
            splx(s);
            return;
        }
        ifp.if_oerrors().set(ifp.if_oerrors().get() + 1);
        printf(format_args!(
            "{}: usb error on tx: {}\n",
            sc.cdce_dev.xname(),
            usbd_errstr(status)
        ));
        if status == USBD_STALLED
            && let Some(pipe) = sc.cdce_bulkout_pipe.get()
        {
            let _ = usbd_clear_endpoint_stall_async(pipe);
        }
        splx(s);
        return;
    }

    let mut err = USBD_NORMAL_COMPLETION;
    usbd_get_xfer_status(xfer, None, None, None, Some(&mut err));

    if let Some(m) = c.cdce_mbuf.take() {
        m_freem(m);
    }

    if err.is_err() {
        ifp.if_oerrors().set(ifp.if_oerrors().get() + 1);
    }

    if !ifq_empty(&ifp.if_snd) {
        cdce_start(ifp);
    }

    splx(s);
}

/// `cdce_intr`: the interrupt pipe's notifications.
pub fn cdce_intr(_xfer: &'static UsbdXfer, addr: *mut c_void, status: UsbdStatus) {
    if addr.is_null() {
        panic(format_args!("cdce: interrupt without its softc"));
    }
    // SAFETY: `cdce_init` opens the interrupt pipe with the softc as `priv`; the softc
    // outlives the pipe.
    let sc = unsafe { &*addr.cast::<CdceSoftc>().cast_const() };

    if status == USBD_CANCELLED {
        return;
    }

    // On success the notification (network connection, connection speed change) is only
    // reported under CDCE_DEBUG: see the module's deviations.
    if status != USBD_NORMAL_COMPLETION
        && status == USBD_STALLED
        && let Some(pipe) = sc.cdce_intr_pipe.get()
    {
        let _ = usbd_clear_endpoint_stall_async(pipe);
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;
    use crate::dev::usb::usb::{usb_wire_at, usetw};

    #[test]
    fn lookup_finds_the_flagged_devices() {
        let t = cdce_lookup(
            i32::from(USB_VENDOR_SHARP),
            i32::from(USB_PRODUCT_SHARP_SL5500),
        );
        assert_eq!(t.map(|t| t.cdce_flags), Some(CDCE_CRC32));
        let t = cdce_lookup(
            i32::from(USB_VENDOR_AMBIT),
            i32::from(USB_PRODUCT_AMBIT_NTL_250),
        );
        assert_eq!(t.map(|t| t.cdce_flags), Some(CDCE_SWAPUNION));
        assert!(cdce_lookup(0x1234, 0x5678).is_none());
    }

    #[test]
    fn eaddr_string_is_read_as_twelve_hex_digits() {
        let mut s = UsbStringDescriptor::zeroed();
        for (i, ch) in b"52540012aBcD".iter().enumerate() {
            usetw(&mut s.bString[i], u16::from(*ch));
        }
        let mut enaddr = [0u8; ETHER_ADDR_LEN];
        cdce_eaddr_from_string(&s, &mut enaddr);
        assert_eq!(enaddr, [0x52, 0x54, 0x00, 0x12, 0xab, 0xcd]);
    }

    #[test]
    fn desc_bytes_are_the_descriptors_own_length() {
        let cdesc = [9u8, 2, 0, 0, 0, 0, 0, 0, 0, 5, 0x24, 6, 0, 1, 3, 0x24, 2];
        let desc = usb_wire_at::<UsbDescriptor>(&cdesc, 9).unwrap();
        assert_eq!(desc.bLength, 5);
        let bytes = cdce_desc_bytes(&cdesc, desc);
        assert_eq!(bytes, &cdesc[9..14]);
        let u = UsbCdcUnionDescriptor::read_from(bytes);
        assert_eq!((u.bMasterInterface, u.bSlaveInterface[0]), (0, 1));
    }
}
/* </TESTS> */
