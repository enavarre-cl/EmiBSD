/*	$OpenBSD: usb.h,v 1.63 2024/05/23 03:21:09 jsg Exp $ */
/*	$NetBSD: usb.h,v 1.69 2002/09/22 23:20:50 augustss Exp $	*/
/*	$FreeBSD: src/sys/dev/usb/usb.h,v 1.14 1999/11/17 22:33:46 n_hibma Exp $	*/
/*	$OpenBSD: usb.c,v 1.134 2024/12/22 22:36:23 kirill Exp $	*/
/*	$NetBSD: usb.c,v 1.77 2003/01/01 00:10:26 thorpej Exp $	*/
/* <LICENSES> */
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

/*
 * Copyright (c) 1998, 2002 The NetBSD Foundation, Inc.
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

//! The USB stack's wire definitions and its bus driver: `<dev/usb/usb.h>` (requests,
//! descriptors, class codes, the `usb(4)` ioctls) and `dev/usb/usb.c` (`usb* at xhci?`, the
//! exploration and task threads, the `/dev/usb*` control device, `usb_tap`).
//!
//! Upstream: sys/dev/usb/usb.h @ 3ce1f3f79392, sys/dev/usb/usb.c @ 3ce1f3f79392
//!
//! The descriptors and requests are `__packed` structures of bytes and little-endian byte
//! pairs (`uWord`, `uDWord`); here they are `#[repr(C)]` structures of `u8` and `[u8; N]`
//! marked [`UsbWire`] (alignment 1, any bytes a valid value), read and written with
//! [`ugetw`]/[`usetw`] as the C's `UGETW`/`USETW`. A descriptor inside a buffer is reached with
//! [`usb_wire_at`], the bounds-checked form of the C's cast.
//!
//! A host controller embeds a [`UsbdBus`](crate::dev::usb::usbdivar::UsbdBus) first in its
//! softc and attaches `usb(4)` with `config_found(self, &sc_bus, usbctlprint)`; `usb_attach`
//! makes the root hub device (`usbd_new_device`), which `uhub(4)` attaches to, and schedules
//! the first exploration. Exploration and the drivers' `usb_task`s run in the `usbtask`
//! thread, transfer aborts in `usbatsk`.
//!
//! ## Deviations
//! - The header and `usb.c` share this module (as `zutil.h`/`zutil.c` share theirs).
//! - The function-like macros are lowercase `const fn`s (`docs/C_TO_RUST.md`): `ugetw`,
//!   `usetw`, `usetw2`, `ugetdw`, `usetdw`, `ue_get_dir`, `ue_set_dir`, `ue_get_addr`,
//!   `ue_get_xfertype`, `ue_get_iso_type`, `ue_get_trans`, `ue_get_size`,
//!   `uce_bulk_max_streams`, `uce_isoc_mult`, `uce_isoc_ssp_iso`, `uhd_not_remov`,
//!   `ups_port_ls_get`, `ups_port_ls_set`. `USETW`/`USETDW` write the bytes one by one as
//!   the strict-alignment form of the C does; the result is the same on both machines.
//! - The ioctl argument structures carry their implicit padding as explicit `_pad` members
//!   (so they can be [`AbiPod`]); their sizes and offsets are pinned by `const` asserts to
//!   the C's (LP64). User addresses (`ucr_data`, `ufd_data`, `udf_data`) are `usize`.
//! - `struct usb_device_request`'s and the descriptors' `typedef`s (`usb_device_request_t`,
//!   ...) are the structures themselves.
//! - `usb.c`: `USB_DEBUG` is not in GENERIC, so `usbdebug`, `usb_noexplore`, the
//!   `USB_SETDEBUG` ioctl and the `DPRINTF`s are absent, as in a kernel built without it. The
//!   `#include "ohci.h"`/`"uhci.h"`/`"ehci.h"` counts only feed those debug paths.
//! - `usb_softc` is reached from `usbd_bus.usbctl` (a [`Device`]) through
//!   [`Device::softc`], the C's `(struct usb_softc *)dev->bus->usbctl`; only `usb(4)`
//!   attaches below a host controller (`usbctlprint`).
//! - The `usb_task` queues are protected by `splusb()` and the kernel lock, as in C (every
//!   USB path but a host controller's hard interrupt runs under the kernel lock: xhci's
//!   interrupt is `IPL_MPSAFE` and only schedules the soft interrupt, which is not; the task
//!   threads are kernel threads).
//! - `usbioctl`'s `USB_DEVICE_GET_CDESC`/`USB_DEVICE_GET_FDESC` lend a task on the caller's
//!   stack to the task thread and wait for it (`usb_wait_task`), as the C does
//!   (`docs/C_TO_RUST.md`, an object on the caller's stack kept while the caller sleeps).

use core::cell::Cell;
use core::ffi::c_void;
use core::mem::{offset_of, size_of};
use core::ptr::{self, NonNull};
use core::slice;
use core::sync::atomic::{AtomicI32, AtomicPtr, Ordering};

use crate::dev::usb::usb_mem::kernaddr;
use crate::dev::usb::usb_subr::{
    usb_delay_ms, usbd_detach, usbd_fill_deviceinfo, usbd_get_cdesc, usbd_new_device,
};
use crate::dev::usb::usbdi::{
    IPL_SOFTUSB, USB_TASK_STATE_NONE, USB_TASK_STATE_ONQ, USB_TASK_STATE_RUN, USB_TASK_TYPE_ABORT,
    USB_TASK_TYPE_EXPLORE, USB_TASK_TYPE_GENERIC, USBD_DEFAULT_TIMEOUT, UsbTask, UsbTaskList,
    splusb, usb_init_task, usbd_do_request_flags, usbd_get_device_descriptor, usbd_is_dying,
};
use crate::dev::usb::usbdivar::{
    URQ_REQUEST, USB_BUS_CONFIG_PENDING, USB_BUS_DISCONNECTING, USBREV_1_0, USBREV_1_1, USBREV_2_0,
    USBREV_3_0, USBREV_STR, USBTAP_DIR_IN, USBTAP_DIR_OUT, UsbdBus, UsbdDevice, UsbdPort, UsbdXfer,
    usbd_xfer_isread,
};
use crate::dev::usb::usbpcap::{
    _USBPCAP_MAX_ISOFRAMES, USBPCAP_CONTROL_STAGE_DATA, USBPCAP_CONTROL_STAGE_SETUP,
    USBPCAP_CONTROL_STAGE_STATUS, USBPCAP_INFO_DIRECTION_IN, USBPCAP_TRANSFER_BULK,
    USBPCAP_TRANSFER_CONTROL, USBPCAP_TRANSFER_INTERRUPT, USBPCAP_TRANSFER_ISOCHRONOUS,
    UsbpcapCtlHdr, UsbpcapIsoHdr, UsbpcapIsoHdrFull, UsbpcapIsoPkt, UsbpcapPktHdr,
};
use crate::kern::kern_kthread::{kthread_create, kthread_create_deferred, kthread_exit};
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_rwlock::rw_init;
use crate::kern::kern_softintr::{softintr_disestablish, softintr_establish, softintr_schedule};
use crate::kern::kern_subr::uiomove;
use crate::kern::kern_synch::{tsleep_nsec, wakeup};
use crate::kern::kern_tc::getmicrouptime;
use crate::kern::subr_autoconf::{
    config_activate_children, config_pending_decr, config_pending_incr,
};
use crate::kern::subr_prf::{Str, panic, printf};
use crate::machine::copy::AbiPod;
use crate::machine::intr::splx;
use crate::net::bpf::{
    BPF_DIRECTION_IN, BPF_DIRECTION_OUT, DLT_USBPCAP, NBPFILTER, bpf_tap_hdr, bpfsattach,
    bpfsdetach,
};
use crate::sys::device::{
    CfMatch, Cfattach, Cfdriver, DV_DULL, DVACT_QUIESCE, DVACT_RESUME, DVACT_WAKEUP, Device, Softc,
    UNCONF,
};
use crate::sys::endian::{htole16, htole32};
use crate::sys::errno::Errno;
use crate::sys::fcntl::FWRITE;
use crate::sys::ioccom::{_ior, _iow, _iowr};
use crate::sys::ioctl::{ioctl_arg, ioctl_ret};
use crate::sys::malloc::{M_NOWAIT, M_TEMP};
use crate::sys::param::PWAIT;
use crate::sys::proc::Proc;
use crate::sys::queue::TailqHead;
use crate::sys::rwlock::Rwlock;
use crate::sys::systm::{COLD, INFSLP};
use crate::sys::time::{Timeval, timersub};
use crate::sys::types::{Dev, minor};
use crate::sys::uio::{Iovec, Uio, UioRw, UioSeg};

/*
 * USB_STACK_VERSION and the bus limits.
 */

/// `USB_STACK_VERSION`.
pub const USB_STACK_VERSION: i32 = 2;

/// `USB_MAX_DEVICES`: addresses per bus (`usbd_bus.devices[]`).
pub const USB_MAX_DEVICES: usize = 128;
/// `USB_START_ADDR`: the address of a device before `SET_ADDRESS`.
pub const USB_START_ADDR: u8 = 0;

/// `USB_CONTROL_ENDPOINT`.
pub const USB_CONTROL_ENDPOINT: u8 = 0;
/// `USB_MAX_ENDPOINTS`.
pub const USB_MAX_ENDPOINTS: usize = 16;

/// `USB_FRAMES_PER_SECOND`.
pub const USB_FRAMES_PER_SECOND: u32 = 1000;

/// `uByte`.
pub type UByte = u8;
/// `uWord`: an unaligned little-endian 16-bit value.
pub type UWord = [u8; 2];
/// `uDWord`: an unaligned little-endian 32-bit value.
pub type UDWord = [u8; 4];

/// `USETW2(w, h, l)`: stores the high byte `h` and the low byte `l`.
pub const fn usetw2(w: &mut UWord, h: u8, l: u8) {
    w[0] = l;
    w[1] = h;
}

/// `UGETW(w)`: the 16-bit value of `w`.
pub const fn ugetw(w: UWord) -> u16 {
    u16::from_le_bytes(w)
}

/// `USETW(w, v)`: stores the 16-bit value `v` (the C truncates an `int`; callers convert).
pub const fn usetw(w: &mut UWord, v: u16) {
    *w = v.to_le_bytes();
}

/// `UGETDW(w)`: the 32-bit value of `w`.
pub const fn ugetdw(w: UDWord) -> u32 {
    u32::from_le_bytes(w)
}

/// `USETDW(w, v)`: stores the 32-bit value `v`.
pub const fn usetdw(w: &mut UDWord, v: u32) {
    *w = v.to_le_bytes();
}

/// A USB wire structure: a request or a descriptor, `__packed` in C.
///
/// # Safety
///
/// Implement only for `#[repr(C)]` types made of `u8`s and arrays and structures of them:
/// the alignment is 1, there is no padding, and every bit pattern is a valid value.
pub unsafe trait UsbWire: Copy + 'static {
    /// A value of all-zero bytes (`memset(&x, 0, sizeof(x))`).
    fn zeroed() -> Self {
        // SAFETY: every bit pattern, all zeros included, is a valid value (the trait's
        // contract).
        unsafe { core::mem::zeroed() }
    }

    /// The structure's bytes (`(u_char *)&x`).
    fn as_bytes(&self) -> &[u8] {
        // SAFETY: no padding, so all `size_of::<Self>()` bytes are initialised; the slice
        // borrows `self`.
        unsafe { slice::from_raw_parts(core::ptr::from_ref(self).cast::<u8>(), size_of::<Self>()) }
    }

    /// The structure's bytes, writable.
    fn as_bytes_mut(&mut self) -> &mut [u8] {
        // SAFETY: as for `as_bytes`, and any bytes written make a valid value.
        unsafe {
            slice::from_raw_parts_mut(core::ptr::from_mut(self).cast::<u8>(), size_of::<Self>())
        }
    }

    /// A copy of the structure at the front of `bytes`; missing trailing bytes read as 0.
    fn read_from(bytes: &[u8]) -> Self {
        let mut v = Self::zeroed();
        let n = bytes.len().min(size_of::<Self>());
        v.as_bytes_mut()[..n].copy_from_slice(&bytes[..n]);
        v
    }
}

/// `(T *)(bytes + off)`: the wire structure at `off` in `bytes`, `None` when it does not fit.
pub fn usb_wire_at<T: UsbWire>(bytes: &[u8], off: usize) -> Option<&T> {
    let end = off.checked_add(size_of::<T>())?;
    let b = bytes.get(off..end)?;
    // SAFETY: `T: UsbWire` has alignment 1 and accepts any bytes; the slice covers the whole
    // `T` and the reference borrows it.
    Some(unsafe { &*b.as_ptr().cast::<T>() })
}

/// `struct usb_device_request` (`usb_device_request_t`): a control transfer's SETUP packet.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(non_snake_case)] // the USB specification's member names, as in C
pub struct UsbDeviceRequest {
    /// `bmRequestType`: `UT_*`.
    pub bmRequestType: UByte,
    /// `bRequest`: `UR_*`.
    pub bRequest: UByte,
    /// `wValue`.
    pub wValue: UWord,
    /// `wIndex`.
    pub wIndex: UWord,
    /// `wLength`: the length of the data stage.
    pub wLength: UWord,
}

/// `UT_WRITE`.
pub const UT_WRITE: u8 = 0x00;
/// `UT_READ`.
pub const UT_READ: u8 = 0x80;
/// `UT_STANDARD`.
pub const UT_STANDARD: u8 = 0x00;
/// `UT_CLASS`.
pub const UT_CLASS: u8 = 0x20;
/// `UT_VENDOR`.
pub const UT_VENDOR: u8 = 0x40;
/// `UT_DEVICE`.
pub const UT_DEVICE: u8 = 0x00;
/// `UT_INTERFACE`.
pub const UT_INTERFACE: u8 = 0x01;
/// `UT_ENDPOINT`.
pub const UT_ENDPOINT: u8 = 0x02;
/// `UT_OTHER`.
pub const UT_OTHER: u8 = 0x03;

/// `UT_READ_DEVICE`.
pub const UT_READ_DEVICE: u8 = UT_READ | UT_STANDARD | UT_DEVICE;
/// `UT_READ_INTERFACE`.
pub const UT_READ_INTERFACE: u8 = UT_READ | UT_STANDARD | UT_INTERFACE;
/// `UT_READ_ENDPOINT`.
pub const UT_READ_ENDPOINT: u8 = UT_READ | UT_STANDARD | UT_ENDPOINT;
/// `UT_WRITE_DEVICE`.
pub const UT_WRITE_DEVICE: u8 = UT_WRITE | UT_STANDARD | UT_DEVICE;
/// `UT_WRITE_INTERFACE`.
pub const UT_WRITE_INTERFACE: u8 = UT_WRITE | UT_STANDARD | UT_INTERFACE;
/// `UT_WRITE_ENDPOINT`.
pub const UT_WRITE_ENDPOINT: u8 = UT_WRITE | UT_STANDARD | UT_ENDPOINT;
/// `UT_READ_CLASS_DEVICE`.
pub const UT_READ_CLASS_DEVICE: u8 = UT_READ | UT_CLASS | UT_DEVICE;
/// `UT_READ_CLASS_INTERFACE`.
pub const UT_READ_CLASS_INTERFACE: u8 = UT_READ | UT_CLASS | UT_INTERFACE;
/// `UT_READ_CLASS_OTHER`.
pub const UT_READ_CLASS_OTHER: u8 = UT_READ | UT_CLASS | UT_OTHER;
/// `UT_READ_CLASS_ENDPOINT`.
pub const UT_READ_CLASS_ENDPOINT: u8 = UT_READ | UT_CLASS | UT_ENDPOINT;
/// `UT_WRITE_CLASS_DEVICE`.
pub const UT_WRITE_CLASS_DEVICE: u8 = UT_WRITE | UT_CLASS | UT_DEVICE;
/// `UT_WRITE_CLASS_INTERFACE`.
pub const UT_WRITE_CLASS_INTERFACE: u8 = UT_WRITE | UT_CLASS | UT_INTERFACE;
/// `UT_WRITE_CLASS_OTHER`.
pub const UT_WRITE_CLASS_OTHER: u8 = UT_WRITE | UT_CLASS | UT_OTHER;
/// `UT_WRITE_CLASS_ENDPOINT`.
pub const UT_WRITE_CLASS_ENDPOINT: u8 = UT_WRITE | UT_CLASS | UT_ENDPOINT;
/// `UT_READ_VENDOR_DEVICE`.
pub const UT_READ_VENDOR_DEVICE: u8 = UT_READ | UT_VENDOR | UT_DEVICE;
/// `UT_READ_VENDOR_INTERFACE`.
pub const UT_READ_VENDOR_INTERFACE: u8 = UT_READ | UT_VENDOR | UT_INTERFACE;
/// `UT_READ_VENDOR_OTHER`.
pub const UT_READ_VENDOR_OTHER: u8 = UT_READ | UT_VENDOR | UT_OTHER;
/// `UT_READ_VENDOR_ENDPOINT`.
pub const UT_READ_VENDOR_ENDPOINT: u8 = UT_READ | UT_VENDOR | UT_ENDPOINT;
/// `UT_WRITE_VENDOR_DEVICE`.
pub const UT_WRITE_VENDOR_DEVICE: u8 = UT_WRITE | UT_VENDOR | UT_DEVICE;
/// `UT_WRITE_VENDOR_INTERFACE`.
pub const UT_WRITE_VENDOR_INTERFACE: u8 = UT_WRITE | UT_VENDOR | UT_INTERFACE;
/// `UT_WRITE_VENDOR_OTHER`.
pub const UT_WRITE_VENDOR_OTHER: u8 = UT_WRITE | UT_VENDOR | UT_OTHER;
/// `UT_WRITE_VENDOR_ENDPOINT`.
pub const UT_WRITE_VENDOR_ENDPOINT: u8 = UT_WRITE | UT_VENDOR | UT_ENDPOINT;

/* Requests */
/// `UR_GET_STATUS`.
pub const UR_GET_STATUS: u8 = 0x00;
/// `UR_CLEAR_FEATURE`.
pub const UR_CLEAR_FEATURE: u8 = 0x01;
/// `UR_SET_FEATURE`.
pub const UR_SET_FEATURE: u8 = 0x03;
/// `UR_SET_ADDRESS`.
pub const UR_SET_ADDRESS: u8 = 0x05;
/// `UR_GET_DESCRIPTOR`.
pub const UR_GET_DESCRIPTOR: u8 = 0x06;
/// `UDESC_DEVICE`.
pub const UDESC_DEVICE: u8 = 0x01;
/// `UDESC_CONFIG`.
pub const UDESC_CONFIG: u8 = 0x02;
/// `UDESC_STRING`.
pub const UDESC_STRING: u8 = 0x03;
/// `UDESC_INTERFACE`.
pub const UDESC_INTERFACE: u8 = 0x04;
/// `UDESC_ENDPOINT`.
pub const UDESC_ENDPOINT: u8 = 0x05;
/// `UDESC_DEVICE_QUALIFIER`.
pub const UDESC_DEVICE_QUALIFIER: u8 = 0x06;
/// `UDESC_OTHER_SPEED_CONFIGURATION`.
pub const UDESC_OTHER_SPEED_CONFIGURATION: u8 = 0x07;
/// `UDESC_INTERFACE_POWER`.
pub const UDESC_INTERFACE_POWER: u8 = 0x08;
/// `UDESC_OTG`.
pub const UDESC_OTG: u8 = 0x09;
/// `UDESC_DEBUG`.
pub const UDESC_DEBUG: u8 = 0x0A;
/// `UDESC_IFACE_ASSOC`: interface association.
pub const UDESC_IFACE_ASSOC: u8 = 0x0B;
/// `UDESC_BOS`: binary object store.
pub const UDESC_BOS: u8 = 0x0F;
/// `UDESC_DEVICE_CAPABILITY`.
pub const UDESC_DEVICE_CAPABILITY: u8 = 0x10;
/// `UDESC_CS_DEVICE`: class specific.
pub const UDESC_CS_DEVICE: u8 = 0x21;
/// `UDESC_CS_CONFIG`.
pub const UDESC_CS_CONFIG: u8 = 0x22;
/// `UDESC_CS_STRING`.
pub const UDESC_CS_STRING: u8 = 0x23;
/// `UDESC_CS_INTERFACE`.
pub const UDESC_CS_INTERFACE: u8 = 0x24;
/// `UDESC_CS_ENDPOINT`.
pub const UDESC_CS_ENDPOINT: u8 = 0x25;
/// `UDESC_HUB`.
pub const UDESC_HUB: u8 = 0x29;
/// `UDESC_SS_HUB`: super speed.
pub const UDESC_SS_HUB: u8 = 0x2A;
/// `UDESC_ENDPOINT_SS_COMP`: super speed.
pub const UDESC_ENDPOINT_SS_COMP: u8 = 0x30;
/// `UR_SET_DESCRIPTOR`.
pub const UR_SET_DESCRIPTOR: u8 = 0x07;
/// `UR_GET_CONFIG`.
pub const UR_GET_CONFIG: u8 = 0x08;
/// `UR_SET_CONFIG`.
pub const UR_SET_CONFIG: u8 = 0x09;
/// `UR_GET_INTERFACE`.
pub const UR_GET_INTERFACE: u8 = 0x0a;
/// `UR_SET_INTERFACE`.
pub const UR_SET_INTERFACE: u8 = 0x0b;
/// `UR_SYNCH_FRAME`.
pub const UR_SYNCH_FRAME: u8 = 0x0c;

/* Feature numbers */
/// `UF_ENDPOINT_HALT`.
pub const UF_ENDPOINT_HALT: u16 = 0;
/// `UF_DEVICE_REMOTE_WAKEUP`.
pub const UF_DEVICE_REMOTE_WAKEUP: u16 = 1;
/// `UF_TEST_MODE`.
pub const UF_TEST_MODE: u16 = 2;

/// `USB_MAX_IPACKET`: maximum size of the initial packet.
pub const USB_MAX_IPACKET: usize = 8;

/// `USB_2_MAX_CTRL_PACKET`.
pub const USB_2_MAX_CTRL_PACKET: u16 = 64;
/// `USB_2_MAX_BULK_PACKET`.
pub const USB_2_MAX_BULK_PACKET: u16 = 512;

/// `struct usb_descriptor` (`usb_descriptor_t`): the head every descriptor shares.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(non_snake_case)] // the USB specification's member names, as in C
pub struct UsbDescriptor {
    /// `bLength`.
    pub bLength: UByte,
    /// `bDescriptorType`.
    pub bDescriptorType: UByte,
    /// `bDescriptorSubtype`.
    pub bDescriptorSubtype: UByte,
}

/// `struct usb_device_descriptor` (`usb_device_descriptor_t`).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(non_snake_case)] // the USB specification's member names, as in C
pub struct UsbDeviceDescriptor {
    /// `bLength`.
    pub bLength: UByte,
    /// `bDescriptorType`.
    pub bDescriptorType: UByte,
    /// `bcdUSB`.
    pub bcdUSB: UWord,
    /// `bDeviceClass`.
    pub bDeviceClass: UByte,
    /// `bDeviceSubClass`.
    pub bDeviceSubClass: UByte,
    /// `bDeviceProtocol`.
    pub bDeviceProtocol: UByte,
    /// `bMaxPacketSize`.
    pub bMaxPacketSize: UByte,
    // The fields below are not part of the initial descriptor.
    /// `idVendor`.
    pub idVendor: UWord,
    /// `idProduct`.
    pub idProduct: UWord,
    /// `bcdDevice`.
    pub bcdDevice: UWord,
    /// `iManufacturer`.
    pub iManufacturer: UByte,
    /// `iProduct`.
    pub iProduct: UByte,
    /// `iSerialNumber`.
    pub iSerialNumber: UByte,
    /// `bNumConfigurations`.
    pub bNumConfigurations: UByte,
}
/// `USB_DEVICE_DESCRIPTOR_SIZE`.
pub const USB_DEVICE_DESCRIPTOR_SIZE: usize = 18;

/// `struct usb_config_descriptor` (`usb_config_descriptor_t`): the head of a configuration's
/// descriptors, `wTotalLength` bytes in all.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(non_snake_case)] // the USB specification's member names, as in C
pub struct UsbConfigDescriptor {
    /// `bLength`.
    pub bLength: UByte,
    /// `bDescriptorType`.
    pub bDescriptorType: UByte,
    /// `wTotalLength`.
    pub wTotalLength: UWord,
    /// `bNumInterfaces`.
    pub bNumInterfaces: UByte,
    /// `bConfigurationValue`.
    pub bConfigurationValue: UByte,
    /// `iConfiguration`.
    pub iConfiguration: UByte,
    /// `bmAttributes`: `UC_*`.
    pub bmAttributes: UByte,
    /// `bMaxPower`: max current in 2 mA units.
    pub bMaxPower: UByte,
}
/// `UC_BUS_POWERED`.
pub const UC_BUS_POWERED: u8 = 0x80;
/// `UC_SELF_POWERED`.
pub const UC_SELF_POWERED: u8 = 0x40;
/// `UC_REMOTE_WAKEUP`.
pub const UC_REMOTE_WAKEUP: u8 = 0x20;
/// `UC_POWER_FACTOR`: `bMaxPower` units, in mA.
pub const UC_POWER_FACTOR: i32 = 2;
/// `USB_CONFIG_DESCRIPTOR_SIZE`.
pub const USB_CONFIG_DESCRIPTOR_SIZE: usize = 9;

/// `struct usb_interface_descriptor` (`usb_interface_descriptor_t`).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(non_snake_case)] // the USB specification's member names, as in C
pub struct UsbInterfaceDescriptor {
    /// `bLength`.
    pub bLength: UByte,
    /// `bDescriptorType`.
    pub bDescriptorType: UByte,
    /// `bInterfaceNumber`.
    pub bInterfaceNumber: UByte,
    /// `bAlternateSetting`.
    pub bAlternateSetting: UByte,
    /// `bNumEndpoints`.
    pub bNumEndpoints: UByte,
    /// `bInterfaceClass`.
    pub bInterfaceClass: UByte,
    /// `bInterfaceSubClass`.
    pub bInterfaceSubClass: UByte,
    /// `bInterfaceProtocol`.
    pub bInterfaceProtocol: UByte,
    /// `iInterface`.
    pub iInterface: UByte,
}
/// `USB_INTERFACE_DESCRIPTOR_SIZE`.
pub const USB_INTERFACE_DESCRIPTOR_SIZE: usize = 9;

/// `struct usb_interface_assoc_descriptor` (`usb_interface_assoc_descriptor_t`).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(non_snake_case)] // the USB specification's member names, as in C
pub struct UsbInterfaceAssocDescriptor {
    /// `bLength`.
    pub bLength: UByte,
    /// `bDescriptorType`.
    pub bDescriptorType: UByte,
    /// `bFirstInterface`.
    pub bFirstInterface: UByte,
    /// `bInterfaceCount`.
    pub bInterfaceCount: UByte,
    /// `bFunctionClass`.
    pub bFunctionClass: UByte,
    /// `bFunctionSubClass`.
    pub bFunctionSubClass: UByte,
    /// `bFunctionProtocol`.
    pub bFunctionProtocol: UByte,
    /// `iFunction`.
    pub iFunction: UByte,
}
/// `USB_INTERFACE_ASSOC_DESCRIPTOR_SIZE`.
pub const USB_INTERFACE_ASSOC_DESCRIPTOR_SIZE: usize = 8;

/// `struct usb_endpoint_descriptor` (`usb_endpoint_descriptor_t`).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(non_snake_case)] // the USB specification's member names, as in C
pub struct UsbEndpointDescriptor {
    /// `bLength`.
    pub bLength: UByte,
    /// `bDescriptorType`.
    pub bDescriptorType: UByte,
    /// `bEndpointAddress`: `UE_DIR_*` and the address.
    pub bEndpointAddress: UByte,
    /// `bmAttributes`: `UE_XFERTYPE` and `UE_ISO_TYPE`.
    pub bmAttributes: UByte,
    /// `wMaxPacketSize`.
    pub wMaxPacketSize: UWord,
    /// `bInterval`.
    pub bInterval: UByte,
}

/// `UE_GET_DIR(a)`.
pub const fn ue_get_dir(a: u8) -> u8 {
    a & 0x80
}
/// `UE_SET_DIR(a, d)`.
pub const fn ue_set_dir(a: u8, d: u8) -> u8 {
    a | ((d & 1) << 7)
}
/// `UE_DIR_IN`.
pub const UE_DIR_IN: u8 = 0x80;
/// `UE_DIR_OUT`.
pub const UE_DIR_OUT: u8 = 0x00;
/// `UE_ADDR`.
pub const UE_ADDR: u8 = 0x0f;
/// `UE_GET_ADDR(a)`.
pub const fn ue_get_addr(a: u8) -> u8 {
    a & UE_ADDR
}
/// `UE_XFERTYPE`.
pub const UE_XFERTYPE: u8 = 0x03;
/// `UE_CONTROL`.
pub const UE_CONTROL: u8 = 0x00;
/// `UE_ISOCHRONOUS`.
pub const UE_ISOCHRONOUS: u8 = 0x01;
/// `UE_BULK`.
pub const UE_BULK: u8 = 0x02;
/// `UE_INTERRUPT`.
pub const UE_INTERRUPT: u8 = 0x03;
/// `UE_GET_XFERTYPE(a)`.
pub const fn ue_get_xfertype(a: u8) -> u8 {
    a & UE_XFERTYPE
}
/// `UE_ISO_TYPE`.
pub const UE_ISO_TYPE: u8 = 0x0c;
/// `UE_ISO_ASYNC`.
pub const UE_ISO_ASYNC: u8 = 0x04;
/// `UE_ISO_ADAPT`.
pub const UE_ISO_ADAPT: u8 = 0x08;
/// `UE_ISO_SYNC`.
pub const UE_ISO_SYNC: u8 = 0x0c;
/// `UE_GET_ISO_TYPE(a)`.
pub const fn ue_get_iso_type(a: u8) -> u8 {
    a & UE_ISO_TYPE
}
/// `UE_GET_TRANS(a)`: the additional transactions per microframe of `wMaxPacketSize`.
pub const fn ue_get_trans(a: u16) -> u16 {
    (a >> 11) & 0x3
}
/// `UE_GET_SIZE(a)`: the packet size of `wMaxPacketSize`.
pub const fn ue_get_size(a: u16) -> u16 {
    a & 0x7ff
}
/// `USB_ENDPOINT_DESCRIPTOR_SIZE`.
pub const USB_ENDPOINT_DESCRIPTOR_SIZE: usize = 7;

/// `struct usb_endpoint_ss_comp_descriptor` (`usb_endpoint_ss_comp_descriptor_t`).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(non_snake_case)] // the USB specification's member names, as in C
pub struct UsbEndpointSsCompDescriptor {
    /// `bLength`.
    pub bLength: UByte,
    /// `bDescriptorType`.
    pub bDescriptorType: UByte,
    /// `bMaxBurst`.
    pub bMaxBurst: UByte,
    /// `bmAttributes`.
    pub bmAttributes: UByte,
    /// `wBytesPerInterval`.
    pub wBytesPerInterval: UWord,
}
/// `UCE_BULK_MAX_STREAMS(a)`.
pub const fn uce_bulk_max_streams(a: u8) -> u8 {
    a
}
/// `UCE_ISOC_MULT(a)`.
pub const fn uce_isoc_mult(a: u8) -> u8 {
    a & 0x0c
}
/// `UCE_ISOC_SSP_ISO(a)`.
pub const fn uce_isoc_ssp_iso(a: u8) -> u8 {
    a & 0x80
}
/// `USB_ENDPOINT_SS_COMP_DESCRIPTOR_SIZE`.
pub const USB_ENDPOINT_SS_COMP_DESCRIPTOR_SIZE: usize = 6;

/// `struct usb_string_descriptor` (`usb_string_descriptor_t`).
///
/// Note: The length of the USB string descriptor is stored in a one byte value and can
/// therefore be no longer than 255 bytes. Two bytes are used for the length itself and the
/// descriptor type, a theoretical maximum of 253 bytes is left for the actual string data.
/// Since the strings are encoded as 2-byte unicode characters, only 252 bytes or 126 two-byte
/// characters can be used. `USB_MAX_STRING_LEN` is defined as 127, leaving space for the
/// terminal '\0' character in C strings.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(non_snake_case)] // the USB specification's member names, as in C
pub struct UsbStringDescriptor {
    /// `bLength`.
    pub bLength: UByte,
    /// `bDescriptorType`.
    pub bDescriptorType: UByte,
    /// `bString`: UTF-16LE characters.
    pub bString: [UWord; 126],
}
/// `USB_MAX_STRING_LEN`.
pub const USB_MAX_STRING_LEN: usize = 127;
/// `USB_LANGUAGE_TABLE`: # of the string language id table.
pub const USB_LANGUAGE_TABLE: i32 = 0;

/* Hub specific request */
/// `UR_GET_BUS_STATE`.
pub const UR_GET_BUS_STATE: u8 = 0x02;
/// `UR_CLEAR_TT_BUFFER`.
pub const UR_CLEAR_TT_BUFFER: u8 = 0x08;
/// `UR_RESET_TT`.
pub const UR_RESET_TT: u8 = 0x09;
/// `UR_GET_TT_STATE`.
pub const UR_GET_TT_STATE: u8 = 0x0a;
/// `UR_STOP_TT`.
pub const UR_STOP_TT: u8 = 0x0b;
/// `UR_SET_DEPTH`.
pub const UR_SET_DEPTH: u8 = 0x0c;

/* Hub features */
/// `UHF_C_HUB_LOCAL_POWER`.
pub const UHF_C_HUB_LOCAL_POWER: i32 = 0;
/// `UHF_C_HUB_OVER_CURRENT`.
pub const UHF_C_HUB_OVER_CURRENT: i32 = 1;

/* Port feature */
/// `UHF_PORT_CONNECTION`.
pub const UHF_PORT_CONNECTION: i32 = 0;
/// `UHF_PORT_ENABLE`.
pub const UHF_PORT_ENABLE: i32 = 1;
/// `UHF_PORT_SUSPEND`.
pub const UHF_PORT_SUSPEND: i32 = 2;
/// `UHF_PORT_OVER_CURRENT`.
pub const UHF_PORT_OVER_CURRENT: i32 = 3;
/// `UHF_PORT_RESET`.
pub const UHF_PORT_RESET: i32 = 4;
/// `UHF_PORT_POWER`.
pub const UHF_PORT_POWER: i32 = 8;
/// `UHF_PORT_LOW_SPEED`.
pub const UHF_PORT_LOW_SPEED: i32 = 9;
/// `UHF_C_PORT_CONNECTION`.
pub const UHF_C_PORT_CONNECTION: i32 = 16;
/// `UHF_C_PORT_ENABLE`.
pub const UHF_C_PORT_ENABLE: i32 = 17;
/// `UHF_C_PORT_SUSPEND`.
pub const UHF_C_PORT_SUSPEND: i32 = 18;
/// `UHF_C_PORT_OVER_CURRENT`.
pub const UHF_C_PORT_OVER_CURRENT: i32 = 19;
/// `UHF_C_PORT_RESET`.
pub const UHF_C_PORT_RESET: i32 = 20;
/// `UHF_PORT_TEST`.
pub const UHF_PORT_TEST: i32 = 21;
/// `UHF_PORT_INDICATOR`.
pub const UHF_PORT_INDICATOR: i32 = 22;
/// `UHF_C_PORT_L1`.
pub const UHF_C_PORT_L1: i32 = 23;
/// `UHF_PORT_DISOWN_TO_1_1`.
pub const UHF_PORT_DISOWN_TO_1_1: i32 = 30;

/* Super-Speed Port feature */
/// `UHF_PORT_U1_TIMEOUT`.
pub const UHF_PORT_U1_TIMEOUT: i32 = 23;
/// `UHF_PORT_U2_TIMEOUT`.
pub const UHF_PORT_U2_TIMEOUT: i32 = 24;
/// `UHF_C_PORT_LINK_STATE`.
pub const UHF_C_PORT_LINK_STATE: i32 = 25;
/// `UHF_C_PORT_CONFIG_ERROR`.
pub const UHF_C_PORT_CONFIG_ERROR: i32 = 26;
/// `UHF_PORT_REMOTE_WAKE_MASK`.
pub const UHF_PORT_REMOTE_WAKE_MASK: i32 = 27;
/// `UHF_BH_PORT_RESET`.
pub const UHF_BH_PORT_RESET: i32 = 28;
/// `UHF_C_BH_PORT_RESET`.
pub const UHF_C_BH_PORT_RESET: i32 = 29;
/// `UHF_FORCE_LINKPM_ACCEPT`.
pub const UHF_FORCE_LINKPM_ACCEPT: i32 = 30;

/// `struct usb_hub_descriptor` (`usb_hub_descriptor_t`).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(non_snake_case)] // the USB specification's member names, as in C
pub struct UsbHubDescriptor {
    /// `bDescLength`.
    pub bDescLength: UByte,
    /// `bDescriptorType`.
    pub bDescriptorType: UByte,
    /// `bNbrPorts`.
    pub bNbrPorts: UByte,
    /// `wHubCharacteristics`: `UHD_*`.
    pub wHubCharacteristics: UWord,
    /// `bPwrOn2PwrGood`: delay in 2 ms units.
    pub bPwrOn2PwrGood: UByte,
    /// `bHubContrCurrent`.
    pub bHubContrCurrent: UByte,
    /// `DeviceRemovable`: max 255 ports.
    pub DeviceRemovable: [UByte; 32],
}
/// `UHD_PWR`.
pub const UHD_PWR: u16 = 0x0003;
/// `UHD_PWR_GANGED`.
pub const UHD_PWR_GANGED: u16 = 0x0000;
/// `UHD_PWR_INDIVIDUAL`.
pub const UHD_PWR_INDIVIDUAL: u16 = 0x0001;
/// `UHD_PWR_NO_SWITCH`.
pub const UHD_PWR_NO_SWITCH: u16 = 0x0002;
/// `UHD_COMPOUND`.
pub const UHD_COMPOUND: u16 = 0x0004;
/// `UHD_OC`.
pub const UHD_OC: u16 = 0x0018;
/// `UHD_OC_GLOBAL`.
pub const UHD_OC_GLOBAL: u16 = 0x0000;
/// `UHD_OC_INDIVIDUAL`.
pub const UHD_OC_INDIVIDUAL: u16 = 0x0008;
/// `UHD_OC_NONE`.
pub const UHD_OC_NONE: u16 = 0x0010;
/// `UHD_TT_THINK`.
pub const UHD_TT_THINK: u16 = 0x0060;
/// `UHD_TT_THINK_8`.
pub const UHD_TT_THINK_8: u16 = 0x0000;
/// `UHD_TT_THINK_16`.
pub const UHD_TT_THINK_16: u16 = 0x0020;
/// `UHD_TT_THINK_24`.
pub const UHD_TT_THINK_24: u16 = 0x0040;
/// `UHD_TT_THINK_32`.
pub const UHD_TT_THINK_32: u16 = 0x0060;
/// `UHD_PORT_IND`.
pub const UHD_PORT_IND: u16 = 0x0080;
/// `UHD_PWRON_FACTOR`: `bPwrOn2PwrGood` units, in ms.
pub const UHD_PWRON_FACTOR: i32 = 2;
/// `UHD_NOT_REMOV(desc, i)`: whether the device on port `i` is not removable.
pub const fn uhd_not_remov(desc: &UsbHubDescriptor, i: usize) -> u8 {
    (desc.DeviceRemovable[i / 8] >> (i % 8)) & 1
}
/// `USB_HUB_DESCRIPTOR_SIZE`.
pub const USB_HUB_DESCRIPTOR_SIZE: usize = 8;

/// `struct usb_hub_ss_descriptor` (`usb_hub_ss_descriptor_t`).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(non_snake_case)] // the USB specification's member names, as in C
pub struct UsbHubSsDescriptor {
    /// `bDescLength`.
    pub bDescLength: UByte,
    /// `bDescriptorType`.
    pub bDescriptorType: UByte,
    /// `bNbrPorts`.
    pub bNbrPorts: UByte,
    /// `wHubCharacteristics`.
    pub wHubCharacteristics: UWord,
    /// `bPwrOn2PwrGood`: delay in 2 ms units.
    pub bPwrOn2PwrGood: UByte,
    /// `bHubContrCurrent`.
    pub bHubContrCurrent: UByte,
    /// `bHubHdrDecLat`.
    pub bHubHdrDecLat: UByte,
    /// `wHubDelay`.
    pub wHubDelay: UWord,
    /// `DeviceRemovable`: max 255 ports.
    pub DeviceRemovable: [UByte; 32],
}
/// `USB_HUB_SS_DESCRIPTOR_SIZE`.
pub const USB_HUB_SS_DESCRIPTOR_SIZE: usize = 11;

/// `struct usb_device_qualifier` (`usb_device_qualifier_t`).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(non_snake_case)] // the USB specification's member names, as in C
pub struct UsbDeviceQualifier {
    /// `bLength`.
    pub bLength: UByte,
    /// `bDescriptorType`.
    pub bDescriptorType: UByte,
    /// `bcdUSB`.
    pub bcdUSB: UWord,
    /// `bDeviceClass`.
    pub bDeviceClass: UByte,
    /// `bDeviceSubClass`.
    pub bDeviceSubClass: UByte,
    /// `bDeviceProtocol`.
    pub bDeviceProtocol: UByte,
    /// `bMaxPacketSize0`.
    pub bMaxPacketSize0: UByte,
    /// `bNumConfigurations`.
    pub bNumConfigurations: UByte,
    /// `bReserved`.
    pub bReserved: UByte,
}
/// `USB_DEVICE_QUALIFIER_SIZE`.
pub const USB_DEVICE_QUALIFIER_SIZE: usize = 10;

/// `struct usb_otg_descriptor` (`usb_otg_descriptor_t`).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(non_snake_case)] // the USB specification's member names, as in C
pub struct UsbOtgDescriptor {
    /// `bLength`.
    pub bLength: UByte,
    /// `bDescriptorType`.
    pub bDescriptorType: UByte,
    /// `bmAttributes`: `UOTG_SRP`, `UOTG_HNP`.
    pub bmAttributes: UByte,
}
/// `UOTG_SRP`.
pub const UOTG_SRP: u8 = 0x01;
/// `UOTG_HNP`.
pub const UOTG_HNP: u8 = 0x02;

/* OTG feature selectors */
/// `UOTG_B_HNP_ENABLE`.
pub const UOTG_B_HNP_ENABLE: u16 = 3;
/// `UOTG_A_HNP_SUPPORT`.
pub const UOTG_A_HNP_SUPPORT: u16 = 4;
/// `UOTG_A_ALT_HNP_SUPPORT`.
pub const UOTG_A_ALT_HNP_SUPPORT: u16 = 5;

/// `struct usb_status` (`usb_status_t`).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(non_snake_case)] // the USB specification's member names, as in C
pub struct UsbStatus {
    /// `wStatus`: `UDS_*` or `UES_*`.
    pub wStatus: UWord,
}
/* Device status flags */
/// `UDS_SELF_POWERED`.
pub const UDS_SELF_POWERED: u16 = 0x0001;
/// `UDS_REMOTE_WAKEUP`.
pub const UDS_REMOTE_WAKEUP: u16 = 0x0002;
/* Endpoint status flags */
/// `UES_HALT`.
pub const UES_HALT: u16 = 0x0001;

/// `struct usb_hub_status` (`usb_hub_status_t`).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(non_snake_case)] // the USB specification's member names, as in C
pub struct UsbHubStatus {
    /// `wHubStatus`: `UHS_*`.
    pub wHubStatus: UWord,
    /// `wHubChange`.
    pub wHubChange: UWord,
}
/// `UHS_LOCAL_POWER`.
pub const UHS_LOCAL_POWER: u16 = 0x0001;
/// `UHS_OVER_CURRENT`.
pub const UHS_OVER_CURRENT: u16 = 0x0002;

/// `struct usb_port_status` (`usb_port_status_t`).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(non_snake_case)] // the USB specification's member names, as in C
pub struct UsbPortStatus {
    /// `wPortStatus`: `UPS_*`.
    pub wPortStatus: UWord,
    /// `wPortChange`: `UPS_C_*`.
    pub wPortChange: UWord,
}
/// `UPS_CURRENT_CONNECT_STATUS`.
pub const UPS_CURRENT_CONNECT_STATUS: u16 = 0x0001;
/// `UPS_PORT_ENABLED`.
pub const UPS_PORT_ENABLED: u16 = 0x0002;
/// `UPS_SUSPEND`.
pub const UPS_SUSPEND: u16 = 0x0004;
/// `UPS_OVERCURRENT_INDICATOR`.
pub const UPS_OVERCURRENT_INDICATOR: u16 = 0x0008;
/// `UPS_RESET`.
pub const UPS_RESET: u16 = 0x0010;
/// `UPS_PORT_L1`: USB 2.0 only.
pub const UPS_PORT_L1: u16 = 0x0020;

/* Super-Speed port link state values. */
/// `UPS_PORT_LS_U0`.
pub const UPS_PORT_LS_U0: u16 = 0x00;
/// `UPS_PORT_LS_U1`.
pub const UPS_PORT_LS_U1: u16 = 0x01;
/// `UPS_PORT_LS_U2`.
pub const UPS_PORT_LS_U2: u16 = 0x02;
/// `UPS_PORT_LS_U3`.
pub const UPS_PORT_LS_U3: u16 = 0x03;
/// `UPS_PORT_LS_SS_DISABLED`.
pub const UPS_PORT_LS_SS_DISABLED: u16 = 0x04;
/// `UPS_PORT_LS_RX_DETECT`.
pub const UPS_PORT_LS_RX_DETECT: u16 = 0x05;
/// `UPS_PORT_LS_SS_INACTIVE`.
pub const UPS_PORT_LS_SS_INACTIVE: u16 = 0x06;
/// `UPS_PORT_LS_POLLING`.
pub const UPS_PORT_LS_POLLING: u16 = 0x07;
/// `UPS_PORT_LS_RECOVERY`.
pub const UPS_PORT_LS_RECOVERY: u16 = 0x08;
/// `UPS_PORT_LS_HOT_RESET`.
pub const UPS_PORT_LS_HOT_RESET: u16 = 0x09;
/// `UPS_PORT_LS_COMP_MOD`.
pub const UPS_PORT_LS_COMP_MOD: u16 = 0x0a;
/// `UPS_PORT_LS_LOOPBACK`.
pub const UPS_PORT_LS_LOOPBACK: u16 = 0x0b;
/// `UPS_PORT_LS_RESUME`.
pub const UPS_PORT_LS_RESUME: u16 = 0x0f;
/// `UPS_PORT_LS_GET(x)`.
pub const fn ups_port_ls_get(x: u16) -> u16 {
    (x >> 5) & 0xf
}
/// `UPS_PORT_LS_SET(x)`.
pub const fn ups_port_ls_set(x: u16) -> u16 {
    (x & 0xf) << 5
}

/// `UPS_PORT_POWER`.
pub const UPS_PORT_POWER: u16 = 0x0100;
/// `UPS_PORT_POWER_SS`: USB 3.0 only.
pub const UPS_PORT_POWER_SS: u16 = 0x0200;
/// `UPS_FULL_SPEED`.
pub const UPS_FULL_SPEED: u16 = 0x0000;
/// `UPS_LOW_SPEED`.
pub const UPS_LOW_SPEED: u16 = 0x0200;
/// `UPS_HIGH_SPEED`.
pub const UPS_HIGH_SPEED: u16 = 0x0400;
/// `UPS_PORT_TEST`.
pub const UPS_PORT_TEST: u16 = 0x0800;
/// `UPS_PORT_INDICATOR`.
pub const UPS_PORT_INDICATOR: u16 = 0x1000;

/// `UPS_C_CONNECT_STATUS`.
pub const UPS_C_CONNECT_STATUS: u16 = 0x0001;
/// `UPS_C_PORT_ENABLED`.
pub const UPS_C_PORT_ENABLED: u16 = 0x0002;
/// `UPS_C_SUSPEND`.
pub const UPS_C_SUSPEND: u16 = 0x0004;
/// `UPS_C_OVERCURRENT_INDICATOR`.
pub const UPS_C_OVERCURRENT_INDICATOR: u16 = 0x0008;
/// `UPS_C_PORT_RESET`.
pub const UPS_C_PORT_RESET: u16 = 0x0010;
/// `UPS_C_PORT_L1`: USB 2.0 only.
pub const UPS_C_PORT_L1: u16 = 0x0020;
/// `UPS_C_BH_PORT_RESET`: USB 3.0 only.
pub const UPS_C_BH_PORT_RESET: u16 = 0x0020;
/// `UPS_C_PORT_LINK_STATE`.
pub const UPS_C_PORT_LINK_STATE: u16 = 0x0040;
/// `UPS_C_PORT_CONFIG_ERROR`.
pub const UPS_C_PORT_CONFIG_ERROR: u16 = 0x0080;

/* Device class codes */
/// `UDCLASS_IN_INTERFACE`.
pub const UDCLASS_IN_INTERFACE: u8 = 0x00;
/// `UDCLASS_COMM`.
pub const UDCLASS_COMM: u8 = 0x02;
/// `UDCLASS_HUB`.
pub const UDCLASS_HUB: u8 = 0x09;
/// `UDSUBCLASS_HUB`.
pub const UDSUBCLASS_HUB: u8 = 0x00;
/// `UDPROTO_FSHUB`.
pub const UDPROTO_FSHUB: u8 = 0x00;
/// `UDPROTO_HSHUBSTT`.
pub const UDPROTO_HSHUBSTT: u8 = 0x01;
/// `UDPROTO_HSHUBMTT`.
pub const UDPROTO_HSHUBMTT: u8 = 0x02;
/// `UDPROTO_SSHUB`.
pub const UDPROTO_SSHUB: u8 = 0x03;
/// `UDCLASS_DIAGNOSTIC`.
pub const UDCLASS_DIAGNOSTIC: u8 = 0xdc;
/// `UDCLASS_WIRELESS`.
pub const UDCLASS_WIRELESS: u8 = 0xe0;
/// `UDCLASS_VIDEO`.
pub const UDCLASS_VIDEO: u8 = 0xef;
/// `UDSUBCLASS_RF`.
pub const UDSUBCLASS_RF: u8 = 0x01;
/// `UDPROTO_BLUETOOTH`.
pub const UDPROTO_BLUETOOTH: u8 = 0x01;
/// `UDCLASS_VENDOR`.
pub const UDCLASS_VENDOR: u8 = 0xff;

/* Interface class codes */
/// `UICLASS_UNSPEC`.
pub const UICLASS_UNSPEC: u8 = 0x00;

/// `UICLASS_AUDIO`.
pub const UICLASS_AUDIO: u8 = 0x01;
/// `UISUBCLASS_AUDIOCONTROL`.
pub const UISUBCLASS_AUDIOCONTROL: u8 = 1;
/// `UISUBCLASS_AUDIOSTREAM`.
pub const UISUBCLASS_AUDIOSTREAM: u8 = 2;
/// `UISUBCLASS_MIDISTREAM`.
pub const UISUBCLASS_MIDISTREAM: u8 = 3;

/// `UICLASS_CDC`: communication.
pub const UICLASS_CDC: u8 = 0x02;
/// `UISUBCLASS_DIRECT_LINE_CONTROL_MODEL`.
pub const UISUBCLASS_DIRECT_LINE_CONTROL_MODEL: u8 = 1;
/// `UISUBCLASS_ABSTRACT_CONTROL_MODEL`.
pub const UISUBCLASS_ABSTRACT_CONTROL_MODEL: u8 = 2;
/// `UISUBCLASS_TELEPHONE_CONTROL_MODEL`.
pub const UISUBCLASS_TELEPHONE_CONTROL_MODEL: u8 = 3;
/// `UISUBCLASS_MULTICHANNEL_CONTROL_MODEL`.
pub const UISUBCLASS_MULTICHANNEL_CONTROL_MODEL: u8 = 4;
/// `UISUBCLASS_CAPI_CONTROLMODEL`.
pub const UISUBCLASS_CAPI_CONTROLMODEL: u8 = 5;
/// `UISUBCLASS_ETHERNET_NETWORKING_CONTROL_MODEL`.
pub const UISUBCLASS_ETHERNET_NETWORKING_CONTROL_MODEL: u8 = 6;
/// `UISUBCLASS_ATM_NETWORKING_CONTROL_MODEL`.
pub const UISUBCLASS_ATM_NETWORKING_CONTROL_MODEL: u8 = 7;
/// `UISUBCLASS_MOBILE_DIRECT_LINE_MODEL`.
pub const UISUBCLASS_MOBILE_DIRECT_LINE_MODEL: u8 = 10;
/// `UISUBCLASS_NETWORK_CONTROL_MODEL`.
pub const UISUBCLASS_NETWORK_CONTROL_MODEL: u8 = 13;
/// `UISUBCLASS_MOBILE_BROADBAND_INTERFACE_MODEL`.
pub const UISUBCLASS_MOBILE_BROADBAND_INTERFACE_MODEL: u8 = 14;
/// `UIPROTO_CDC_NOCLASS`.
pub const UIPROTO_CDC_NOCLASS: u8 = 0;
/// `UIPROTO_CDC_AT`.
pub const UIPROTO_CDC_AT: u8 = 1;

/// `UICLASS_HID`.
pub const UICLASS_HID: u8 = 0x03;
/// `UISUBCLASS_BOOT`.
pub const UISUBCLASS_BOOT: u8 = 1;
/// `UIPROTO_BOOT_KEYBOARD`.
pub const UIPROTO_BOOT_KEYBOARD: u8 = 1;
/// `UIPROTO_BOOT_MOUSE`.
pub const UIPROTO_BOOT_MOUSE: u8 = 2;

/// `UICLASS_PHYSICAL`.
pub const UICLASS_PHYSICAL: u8 = 0x05;

/// `UICLASS_IMAGE`.
pub const UICLASS_IMAGE: u8 = 0x06;

/// `UICLASS_PRINTER`.
pub const UICLASS_PRINTER: u8 = 0x07;
/// `UISUBCLASS_PRINTER`.
pub const UISUBCLASS_PRINTER: u8 = 1;
/// `UIPROTO_PRINTER_UNI`.
pub const UIPROTO_PRINTER_UNI: u8 = 1;
/// `UIPROTO_PRINTER_BI`.
pub const UIPROTO_PRINTER_BI: u8 = 2;
/// `UIPROTO_PRINTER_1284`.
pub const UIPROTO_PRINTER_1284: u8 = 3;

/// `UICLASS_MASS`.
pub const UICLASS_MASS: u8 = 0x08;
/// `UISUBCLASS_RBC`.
pub const UISUBCLASS_RBC: u8 = 1;
/// `UISUBCLASS_SFF8020I`.
pub const UISUBCLASS_SFF8020I: u8 = 2;
/// `UISUBCLASS_QIC157`.
pub const UISUBCLASS_QIC157: u8 = 3;
/// `UISUBCLASS_UFI`.
pub const UISUBCLASS_UFI: u8 = 4;
/// `UISUBCLASS_SFF8070I`.
pub const UISUBCLASS_SFF8070I: u8 = 5;
/// `UISUBCLASS_SCSI`.
pub const UISUBCLASS_SCSI: u8 = 6;
/// `UIPROTO_MASS_CBI_I`.
pub const UIPROTO_MASS_CBI_I: u8 = 0;
/// `UIPROTO_MASS_CBI`.
pub const UIPROTO_MASS_CBI: u8 = 1;
/// `UIPROTO_MASS_BBB_OLD`: not in the spec anymore.
pub const UIPROTO_MASS_BBB_OLD: u8 = 2;
/// `UIPROTO_MASS_BBB`: 'P' for the Iomega Zip drive.
pub const UIPROTO_MASS_BBB: u8 = 80;

/// `UICLASS_HUB`.
pub const UICLASS_HUB: u8 = 0x09;
/// `UISUBCLASS_HUB`.
pub const UISUBCLASS_HUB: u8 = 0;
/// `UIPROTO_FSHUB`.
pub const UIPROTO_FSHUB: u8 = 0;
/// `UIPROTO_HSHUBSTT`: yes, same as previous.
pub const UIPROTO_HSHUBSTT: u8 = 0;
/// `UIPROTO_HSHUBMTT`.
pub const UIPROTO_HSHUBMTT: u8 = 1;

/// `UICLASS_CDC_DATA`.
pub const UICLASS_CDC_DATA: u8 = 0x0a;
/// `UISUBCLASS_DATA`.
pub const UISUBCLASS_DATA: u8 = 0;
/// `UIPROTO_DATA_MBIM`: MBIM.
pub const UIPROTO_DATA_MBIM: u8 = 0x02;
/// `UIPROTO_DATA_ISDNBRI`: physical iface.
pub const UIPROTO_DATA_ISDNBRI: u8 = 0x30;
/// `UIPROTO_DATA_HDLC`: HDLC.
pub const UIPROTO_DATA_HDLC: u8 = 0x31;
/// `UIPROTO_DATA_TRANSPARENT`: transparent.
pub const UIPROTO_DATA_TRANSPARENT: u8 = 0x32;
/// `UIPROTO_DATA_Q921M`: management for Q921.
pub const UIPROTO_DATA_Q921M: u8 = 0x50;
/// `UIPROTO_DATA_Q921`: data for Q921.
pub const UIPROTO_DATA_Q921: u8 = 0x51;
/// `UIPROTO_DATA_Q921TM`: TEI multiplexer for Q921.
pub const UIPROTO_DATA_Q921TM: u8 = 0x52;
/// `UIPROTO_DATA_V42BIS`: data compression.
pub const UIPROTO_DATA_V42BIS: u8 = 0x90;
/// `UIPROTO_DATA_Q931`: Euro-ISDN.
pub const UIPROTO_DATA_Q931: u8 = 0x91;
/// `UIPROTO_DATA_V120`: V.24 rate adaption.
pub const UIPROTO_DATA_V120: u8 = 0x92;
/// `UIPROTO_DATA_CAPI`: CAPI 2.0 commands.
pub const UIPROTO_DATA_CAPI: u8 = 0x93;
/// `UIPROTO_DATA_HOST_BASED`: host based driver.
pub const UIPROTO_DATA_HOST_BASED: u8 = 0xfd;
/// `UIPROTO_DATA_PUF`: see Prot. Unit Func. Desc.
pub const UIPROTO_DATA_PUF: u8 = 0xfe;
/// `UIPROTO_DATA_VENDOR`: vendor specific.
pub const UIPROTO_DATA_VENDOR: u8 = 0xff;

/// `UICLASS_SMARTCARD`.
pub const UICLASS_SMARTCARD: u8 = 0x0b;

/// `UICLASS_SECURITY`.
pub const UICLASS_SECURITY: u8 = 0x0d;

/// `UICLASS_VIDEO`.
pub const UICLASS_VIDEO: u8 = 0x0e;
/// `UISUBCLASS_VIDEOCONTROL`.
pub const UISUBCLASS_VIDEOCONTROL: u8 = 1;
/// `UISUBCLASS_VIDEOSTREAM`.
pub const UISUBCLASS_VIDEOSTREAM: u8 = 2;
/// `UISUBCLASS_VIDEO_IF_COLLECTION`.
pub const UISUBCLASS_VIDEO_IF_COLLECTION: u8 = 3;

/// `UICLASS_DIAGNOSTIC`.
pub const UICLASS_DIAGNOSTIC: u8 = 0xdc;

/// `UICLASS_WIRELESS`.
pub const UICLASS_WIRELESS: u8 = 0xe0;
/// `UISUBCLASS_RF`.
pub const UISUBCLASS_RF: u8 = 0x01;
/// `UIPROTO_BLUETOOTH`.
pub const UIPROTO_BLUETOOTH: u8 = 0x01;
/// `UIPROTO_RNDIS`.
pub const UIPROTO_RNDIS: u8 = 0x03;

/// `UICLASS_MISC`.
pub const UICLASS_MISC: u8 = 0xef;
/// `UISUBCLASS_SYNC`.
pub const UISUBCLASS_SYNC: u8 = 0x01;
/// `UIPROTO_ACTIVESYNC`.
pub const UIPROTO_ACTIVESYNC: u8 = 0x01;

/// `UICLASS_APPL_SPEC`.
pub const UICLASS_APPL_SPEC: u8 = 0xfe;
/// `UISUBCLASS_FIRMWARE_DOWNLOAD`.
pub const UISUBCLASS_FIRMWARE_DOWNLOAD: u8 = 1;
/// `UISUBCLASS_IRDA`.
pub const UISUBCLASS_IRDA: u8 = 2;
/// `UIPROTO_IRDA`.
pub const UIPROTO_IRDA: u8 = 0;

/// `UICLASS_VENDOR`.
pub const UICLASS_VENDOR: u8 = 0xff;

/// `USB_HUB_MAX_DEPTH`.
pub const USB_HUB_MAX_DEPTH: u8 = 5;

// Minimum time a device needs to be powered down to go through a power cycle. XXX Are these
// time in the spec?
/// `USB_POWER_DOWN_TIME`, in ms.
pub const USB_POWER_DOWN_TIME: u32 = 200;
/// `USB_PORT_POWER_DOWN_TIME`, in ms.
pub const USB_PORT_POWER_DOWN_TIME: u32 = 100;

// The C keeps the values from the spec under `#if 0` and uses these, which allow for marginal
// (i.e. non-conforming) devices.
/// `USB_PORT_RESET_DELAY`, in ms.
pub const USB_PORT_RESET_DELAY: u32 = 50;
/// `USB_PORT_ROOT_RESET_DELAY`, in ms.
pub const USB_PORT_ROOT_RESET_DELAY: u32 = 100;
/// `USB_PORT_RESET_RECOVERY`, in ms.
pub const USB_PORT_RESET_RECOVERY: u32 = 250;
/// `USB_PORT_POWERUP_DELAY`, in ms.
pub const USB_PORT_POWERUP_DELAY: u32 = 300;
/// `USB_SET_ADDRESS_SETTLE`, in ms.
pub const USB_SET_ADDRESS_SETTLE: u32 = 10;
/// `USB_RESUME_DELAY`, in ms.
pub const USB_RESUME_DELAY: u32 = 50 * 5;
/// `USB_RESUME_WAIT`, in ms.
pub const USB_RESUME_WAIT: u32 = 50;
/// `USB_RESUME_RECOVERY`, in ms.
pub const USB_RESUME_RECOVERY: u32 = 50;
/// `USB_EXTRA_POWER_UP_TIME`, in ms.
pub const USB_EXTRA_POWER_UP_TIME: u32 = 20;

/// `USB_MIN_POWER`, in mA.
pub const USB_MIN_POWER: u16 = 100;
/// `USB_MAX_POWER`, in mA.
pub const USB_MAX_POWER: u16 = 500;

/// `USB_BUS_RESET_DELAY`, in ms (XXX?).
pub const USB_BUS_RESET_DELAY: u32 = 100;

/// `USB_UNCONFIG_NO`.
pub const USB_UNCONFIG_NO: i32 = 0;
/// `USB_UNCONFIG_INDEX`.
pub const USB_UNCONFIG_INDEX: i32 = -1;

/*** ioctl() related stuff ***/

/// `struct usb_ctl_request`: `USB_REQUEST`, `USB_DO_REQUEST`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct UsbCtlRequest {
    /// `ucr_addr`.
    pub ucr_addr: i32,
    /// `ucr_request`.
    pub ucr_request: UsbDeviceRequest,
    /// The C's padding before `ucr_data`.
    pub _pad: [u8; 4],
    /// `ucr_data`: a user address.
    pub ucr_data: usize,
    /// `ucr_flags`: `USBD_SHORT_XFER_OK`.
    pub ucr_flags: i32,
    /// `ucr_actlen`: actual length transferred.
    pub ucr_actlen: i32,
}
/// `USBD_SHORT_XFER_OK`: allow short reads.
pub const USBD_SHORT_XFER_OK: u16 = 0x04;

/// `struct usb_alt_interface`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct UsbAltInterface {
    /// `uai_config_index`.
    pub uai_config_index: i32,
    /// `uai_interface_index`.
    pub uai_interface_index: i32,
    /// `uai_alt_no`.
    pub uai_alt_no: i32,
}

/// `USB_CURRENT_CONFIG_INDEX`.
pub const USB_CURRENT_CONFIG_INDEX: i32 = -1;
/// `USB_CURRENT_ALT_INDEX`.
pub const USB_CURRENT_ALT_INDEX: i32 = -1;

/// `struct usb_config_desc`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct UsbConfigDesc {
    /// `ucd_config_index`.
    pub ucd_config_index: i32,
    /// `ucd_desc`.
    pub ucd_desc: UsbConfigDescriptor,
    /// The C's tail padding.
    pub _pad: [u8; 3],
}

/// `struct usb_device_cdesc`: `USB_DEVICE_GET_CDESC`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct UsbDeviceCdesc {
    /// `udc_bus`.
    pub udc_bus: u8,
    /// `udc_addr`: device address.
    pub udc_addr: u8,
    /// The C's padding before `udc_config_index`.
    pub _pad0: [u8; 2],
    /// `udc_config_index`.
    pub udc_config_index: i32,
    /// `udc_desc`.
    pub udc_desc: UsbConfigDescriptor,
    /// The C's tail padding.
    pub _pad1: [u8; 3],
}

/// `struct usb_interface_desc`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct UsbInterfaceDesc {
    /// `uid_config_index`.
    pub uid_config_index: i32,
    /// `uid_interface_index`.
    pub uid_interface_index: i32,
    /// `uid_alt_index`.
    pub uid_alt_index: i32,
    /// `uid_desc`.
    pub uid_desc: UsbInterfaceDescriptor,
    /// The C's tail padding.
    pub _pad: [u8; 3],
}

/// `struct usb_endpoint_desc`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct UsbEndpointDesc {
    /// `ued_config_index`.
    pub ued_config_index: i32,
    /// `ued_interface_index`.
    pub ued_interface_index: i32,
    /// `ued_alt_index`.
    pub ued_alt_index: i32,
    /// `ued_endpoint_index`.
    pub ued_endpoint_index: i32,
    /// `ued_desc`.
    pub ued_desc: UsbEndpointDescriptor,
    /// The C's tail padding.
    pub _pad: [u8; 1],
}

/// `struct usb_full_desc`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct UsbFullDesc {
    /// `ufd_config_index`.
    pub ufd_config_index: i32,
    /// `ufd_size`.
    pub ufd_size: u32,
    /// `ufd_data`: a user address.
    pub ufd_data: usize,
}

/// `struct usb_device_fdesc`: `USB_DEVICE_GET_FDESC`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct UsbDeviceFdesc {
    /// `udf_bus`.
    pub udf_bus: u8,
    /// `udf_addr`: device address.
    pub udf_addr: u8,
    /// The C's padding before `udf_config_index`.
    pub _pad0: [u8; 2],
    /// `udf_config_index`.
    pub udf_config_index: i32,
    /// `udf_size`.
    pub udf_size: u32,
    /// The C's padding before `udf_data`.
    pub _pad1: [u8; 4],
    /// `udf_data`: a user address (the kernel's descriptor copy while the task fills it).
    pub udf_data: usize,
}

/// `struct usb_device_ddesc`: `USB_DEVICE_GET_DDESC`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct UsbDeviceDdesc {
    /// `udd_bus`.
    pub udd_bus: u8,
    /// `udd_addr`: device address.
    pub udd_addr: u8,
    /// `udd_desc`.
    pub udd_desc: UsbDeviceDescriptor,
}

/// `struct usb_string_desc`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct UsbStringDesc {
    /// `usd_string_index`.
    pub usd_string_index: i32,
    /// `usd_language_id`.
    pub usd_language_id: i32,
    /// `usd_desc`.
    pub usd_desc: UsbStringDescriptor,
    /// The C's tail padding.
    pub _pad: [u8; 2],
}

/// `struct usb_ctl_report_desc`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct UsbCtlReportDesc {
    /// `ucrd_size`.
    pub ucrd_size: i32,
    /// `ucrd_data`: filled data size will vary.
    pub ucrd_data: [u8; 1024],
}

/// `USB_MAX_DEVNAMES`.
pub const USB_MAX_DEVNAMES: usize = 4;
/// `USB_MAX_DEVNAMELEN`.
pub const USB_MAX_DEVNAMELEN: usize = 16;

/// `struct usb_device_info`: `USB_DEVICEINFO`, `USB_GET_DEVICEINFO`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
#[allow(non_snake_case)] // `udi_productNo` & co., as in C
pub struct UsbDeviceInfo {
    /// `udi_bus`.
    pub udi_bus: u8,
    /// `udi_addr`: device address.
    pub udi_addr: u8,
    /// `udi_product`.
    pub udi_product: [u8; USB_MAX_STRING_LEN],
    /// `udi_vendor`.
    pub udi_vendor: [u8; USB_MAX_STRING_LEN],
    /// `udi_release`.
    pub udi_release: [u8; 8],
    /// `udi_productNo`.
    pub udi_productNo: u16,
    /// `udi_vendorNo`.
    pub udi_vendorNo: u16,
    /// `udi_releaseNo`.
    pub udi_releaseNo: u16,
    /// `udi_class`.
    pub udi_class: u8,
    /// `udi_subclass`.
    pub udi_subclass: u8,
    /// `udi_protocol`.
    pub udi_protocol: u8,
    /// `udi_config`.
    pub udi_config: u8,
    /// `udi_speed`: `USB_SPEED_*`.
    pub udi_speed: u8,
    /// `udi_port`.
    pub udi_port: u8,
    /// `udi_power`: power consumption in mA, 0 if selfpowered.
    pub udi_power: i32,
    /// `udi_nports`.
    pub udi_nports: i32,
    /// `udi_devnames`.
    pub udi_devnames: [[u8; USB_MAX_DEVNAMELEN]; USB_MAX_DEVNAMES],
    /// `udi_ports`: hub only: ports status/change.
    pub udi_ports: [u32; 16],
    /// `udi_serial`.
    pub udi_serial: [u8; USB_MAX_STRING_LEN],
    /// The C's tail padding.
    pub _pad: [u8; 1],
}
/// `USB_SPEED_LOW`.
pub const USB_SPEED_LOW: u8 = 1;
/// `USB_SPEED_FULL`.
pub const USB_SPEED_FULL: u8 = 2;
/// `USB_SPEED_HIGH`.
pub const USB_SPEED_HIGH: u8 = 3;
/// `USB_SPEED_SUPER`.
pub const USB_SPEED_SUPER: u8 = 4;

/// `struct usb_ctl_report`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct UsbCtlReport {
    /// `ucr_report`.
    pub ucr_report: i32,
    /// `ucr_data`: filled data size will vary.
    pub ucr_data: [u8; 1024],
}

/// `struct usb_device_stats`: `USB_DEVICESTATS`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct UsbDeviceStats {
    /// `uds_requests`: indexed by transfer type `UE_*`.
    pub uds_requests: [u64; 4],
}

/* USB controller */
/// `USB_REQUEST`.
pub const USB_REQUEST: u64 = _iowr::<UsbCtlRequest>(b'U', 1);
/// `USB_SETDEBUG`.
pub const USB_SETDEBUG: u64 = _iow::<u32>(b'U', 2);
/// `USB_DEVICEINFO`.
pub const USB_DEVICEINFO: u64 = _iowr::<UsbDeviceInfo>(b'U', 4);
/// `USB_DEVICESTATS`.
pub const USB_DEVICESTATS: u64 = _ior::<UsbDeviceStats>(b'U', 5);
/// `USB_DEVICE_GET_CDESC`.
pub const USB_DEVICE_GET_CDESC: u64 = _iowr::<UsbDeviceCdesc>(b'U', 6);
/// `USB_DEVICE_GET_FDESC`.
pub const USB_DEVICE_GET_FDESC: u64 = _iowr::<UsbDeviceFdesc>(b'U', 7);
/// `USB_DEVICE_GET_DDESC`.
pub const USB_DEVICE_GET_DDESC: u64 = _iowr::<UsbDeviceDdesc>(b'U', 8);

/* Generic HID device */
/// `USB_GET_REPORT_DESC`.
pub const USB_GET_REPORT_DESC: u64 = _ior::<UsbCtlReportDesc>(b'U', 21);
/// `USB_GET_REPORT`.
pub const USB_GET_REPORT: u64 = _iowr::<UsbCtlReport>(b'U', 23);
/// `USB_SET_REPORT`.
pub const USB_SET_REPORT: u64 = _iow::<UsbCtlReport>(b'U', 24);
/// `USB_GET_REPORT_ID`.
pub const USB_GET_REPORT_ID: u64 = _ior::<i32>(b'U', 25);

/* Generic USB device */
/// `USB_GET_CONFIG`.
pub const USB_GET_CONFIG: u64 = _ior::<i32>(b'U', 100);
/// `USB_SET_CONFIG`.
pub const USB_SET_CONFIG: u64 = _iow::<i32>(b'U', 101);
/// `USB_GET_ALTINTERFACE`.
pub const USB_GET_ALTINTERFACE: u64 = _iowr::<UsbAltInterface>(b'U', 102);
/// `USB_SET_ALTINTERFACE`.
pub const USB_SET_ALTINTERFACE: u64 = _iowr::<UsbAltInterface>(b'U', 103);
/// `USB_GET_NO_ALT`.
pub const USB_GET_NO_ALT: u64 = _iowr::<UsbAltInterface>(b'U', 104);
/// `USB_GET_DEVICE_DESC`.
pub const USB_GET_DEVICE_DESC: u64 = _ior::<UsbDeviceDescriptor>(b'U', 105);
/// `USB_GET_CONFIG_DESC`.
pub const USB_GET_CONFIG_DESC: u64 = _iowr::<UsbConfigDesc>(b'U', 106);
/// `USB_GET_INTERFACE_DESC`.
pub const USB_GET_INTERFACE_DESC: u64 = _iowr::<UsbInterfaceDesc>(b'U', 107);
/// `USB_GET_ENDPOINT_DESC`.
pub const USB_GET_ENDPOINT_DESC: u64 = _iowr::<UsbEndpointDesc>(b'U', 108);
/// `USB_GET_FULL_DESC`.
pub const USB_GET_FULL_DESC: u64 = _iowr::<UsbFullDesc>(b'U', 109);
/// `USB_DO_REQUEST`.
pub const USB_DO_REQUEST: u64 = _iowr::<UsbCtlRequest>(b'U', 111);
/// `USB_GET_DEVICEINFO`.
pub const USB_GET_DEVICEINFO: u64 = _ior::<UsbDeviceInfo>(b'U', 112);
/// `USB_SET_SHORT_XFER`.
pub const USB_SET_SHORT_XFER: u64 = _iow::<i32>(b'U', 113);
/// `USB_SET_TIMEOUT`.
pub const USB_SET_TIMEOUT: u64 = _iow::<i32>(b'U', 114);

/// `NUSB`: the count `config(8)` writes into `usb.h` for `usb*` (nonzero in GENERIC; only
/// whether it is zero matters, to `cdev_usb_init`).
pub const NUSB: i32 = 1;

/// `struct usb_softc`: `usb(4)`, one per host controller.
#[repr(C)]
pub struct UsbSoftc {
    /// `sc_dev`: base device.
    pub sc_dev: Device,
    /// `sc_bus`: USB controller.
    pub sc_bus: Cell<Option<&'static UsbdBus>>,
    /// `sc_port`: dummy port for root hub.
    pub sc_port: UsbdPort,
    /// `sc_speed`.
    pub sc_speed: Cell<i32>,

    /// `sc_explore_task`.
    pub sc_explore_task: UsbTask,

    /// `sc_ptime`: when the bus attached (`usb_explore` waits for power to stabilize).
    pub sc_ptime: Cell<Timeval>,
}

impl UsbSoftc {
    /// `sc->sc_bus`. Panics before `usb_attach` set it.
    pub fn bus(&self) -> &'static UsbdBus {
        match self.sc_bus.get() {
            Some(b) => b,
            None => panic(format_args!("{}: no bus", self.sc_dev.xname())),
        }
    }
}

// SAFETY: `#[repr(C)]` with the device first; the other members are `Cell`s of options of
// references, integers and a `timeval`, a port and a task: all valid as zero bits.
unsafe impl Softc for UsbSoftc {}

/// The `usb(4)` softc behind `dev`, for as long as the device is attached.
fn usb_softc(dev: &Device) -> &'static UsbSoftc {
    // SAFETY: `dev` is a `usb` device (made by `config_make_softc` for `USB_CA`, whose
    // `ca_devsize` is a `UsbSoftc`); the softc stays allocated until `config_detach` frees
    // it, after `usb_detach` has stopped the explore task and the bus has no devices, so the
    // `'static` the tasks and the bus's port need is never outlived.
    unsafe { &*ptr::from_ref(dev.softc::<UsbSoftc>()) }
}

/// `usbpalock`: serialises `usbd_probe_and_attach`.
pub static USBPALOCK: Rwlock = Rwlock::new("usbpalock");

/// `TAILQ_HEAD(, usb_task)`, made `Sync`.
struct UsbTaskq(TailqHead<UsbTaskList>);
// SAFETY: changed at `splusb()` under the kernel lock (every USB path holds it).
unsafe impl Sync for UsbTaskq {}

/// `usb_abort_tasks`.
static USB_ABORT_TASKS: UsbTaskq = UsbTaskq(TailqHead::new());
/// `usb_explore_tasks`.
static USB_EXPLORE_TASKS: UsbTaskq = UsbTaskq(TailqHead::new());
/// `usb_generic_tasks`.
static USB_GENERIC_TASKS: UsbTaskq = UsbTaskq(TailqHead::new());

/// `usb_nbuses`.
static USB_NBUSES: AtomicI32 = AtomicI32::new(0);
/// `usb_run_tasks`: also the task thread's wait channel.
static USB_RUN_TASKS: AtomicI32 = AtomicI32::new(0);
/// `usb_run_abort_tasks`: also the abort thread's wait channel.
static USB_RUN_ABORT_TASKS: AtomicI32 = AtomicI32::new(0);
/// `explore_pending`.
pub static EXPLORE_PENDING: AtomicI32 = AtomicI32::new(0);

/// `usb_task_thread_proc`.
static USB_TASK_THREAD_PROC: AtomicPtr<Proc> = AtomicPtr::new(ptr::null_mut());
/// `usb_abort_task_thread_proc`.
static USB_ABORT_TASK_THREAD_PROC: AtomicPtr<Proc> = AtomicPtr::new(ptr::null_mut());

/// `usb_cd`.
pub static USB_CD: Cfdriver = Cfdriver::new(b"usb", DV_DULL, 0);

/// `usb_ca`.
pub static USB_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<UsbSoftc>(),
    ca_match: Some(usb_match),
    ca_attach: usb_attach,
    ca_detach: Some(usb_detach),
    ca_activate: Some(usb_activate),
};

/// `usb_match`: `usb(4)` attaches to every host controller.
pub fn usb_match(_parent: Option<&Device>, _match: &CfMatch, _aux: *mut c_void) -> i32 {
    1
}

/// `usb_attach`: `aux` is the host controller's `usbd_bus`.
pub fn usb_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    let sc = usb_softc(self_);

    if USB_NBUSES.load(Ordering::Relaxed) == 0 {
        rw_init(&USBPALOCK, "usbpalock");
        USB_ABORT_TASKS.0.init();
        USB_EXPLORE_TASKS.0.init();
        USB_GENERIC_TASKS.0.init();
        USB_RUN_TASKS.store(1, Ordering::Relaxed);
        USB_RUN_ABORT_TASKS.store(1, Ordering::Relaxed);
        kthread_create_deferred(usb_create_task_threads, ptr::null_mut());
    }
    USB_NBUSES.fetch_add(1, Ordering::Relaxed);

    // SAFETY: a host controller attaches `usb` with its own `usbd_bus` as `aux`
    // (`usbctlprint`'s contract), embedded in its softc, which outlives this child.
    let bus: &'static UsbdBus = unsafe { &*aux.cast::<UsbdBus>() };
    sc.sc_bus.set(Some(bus));
    bus.usbctl.set(Some(NonNull::from(self_)));
    sc.sc_port.power.set(USB_MAX_POWER);

    let usbrev = bus.usbrev.get();
    printf(format_args!(
        ": USB revision {}",
        USBREV_STR.get(usbrev as usize).copied().unwrap_or("?")
    ));
    match usbrev {
        USBREV_1_0 | USBREV_1_1 => sc.sc_speed.set(i32::from(USB_SPEED_FULL)),
        USBREV_2_0 => sc.sc_speed.set(i32::from(USB_SPEED_HIGH)),
        USBREV_3_0 => sc.sc_speed.set(i32::from(USB_SPEED_SUPER)),
        _ => {
            printf(format_args!(", not supported\n"));
            bus.dying.set(1);
            return;
        }
    }
    printf(format_args!("\n"));

    if NBPFILTER > 0 {
        let bpfif = bpfsattach(
            &bus.bpf,
            self_.xname().as_bytes(),
            DLT_USBPCAP,
            size_of::<UsbpcapPktHdr>() as u32,
        );
        bus.bpfif.set(Some(bpfif));
    }

    let cold = COLD.load(Ordering::Relaxed);

    // Make sure not to use tsleep() if we are cold booting.
    if cold {
        bus.use_polling.set(bus.use_polling.get() + 1);
    }

    // Don't let hub interrupts cause explore until ready.
    bus.flags.set(bus.flags.get() | USB_BUS_CONFIG_PENDING);

    // explore task
    usb_init_task(
        &sc.sc_explore_task,
        usb_explore,
        ptr::from_ref(sc).cast_mut().cast(),
        USB_TASK_TYPE_EXPLORE,
    );

    let soft = softintr_establish(
        IPL_SOFTUSB,
        bus.methods().soft_intr,
        ptr::from_ref(bus).cast_mut().cast(),
    );
    bus.soft.set(soft);
    if soft.is_none() {
        printf(format_args!("{}: can't register softintr\n", self_.xname()));
        bus.dying.set(1);
        return;
    }

    if usb_attach_roothub(sc) {
        // Turning this code off will delay attachment of USB devices until the USB task
        // thread is running, which means that the keyboard will not work until after cold
        // boot.
        if cold
            && self_.cfdata().cf_flags & 1 != 0
            && let Some(dev) = bus.root_hub.get()
            && let Some(hub) = dev.hub.get()
        {
            hub.explore(dev);
        }
    }

    if cold {
        bus.use_polling.set(bus.use_polling.get() - 1);
    }

    if bus.dying.get() == 0 {
        sc.sc_ptime.set(getmicrouptime());
        if bus.usbrev.get() == USBREV_2_0 {
            EXPLORE_PENDING.fetch_add(1, Ordering::Relaxed);
        }
        config_pending_incr();
        if let Some(rh) = bus.root_hub.get() {
            usb_needs_explore(rh, true);
        }
    }
}

/// `usb_attach_roothub`: makes the root hub's device; `true` for the C's 0 (success).
fn usb_attach_roothub(sc: &'static UsbSoftc) -> bool {
    let bus = sc.bus();

    if usbd_new_device(&sc.sc_dev, bus, 0, sc.sc_speed.get(), 0, &sc.sc_port).is_err() {
        printf(format_args!("{}: root hub problem\n", sc.sc_dev.xname()));
        bus.dying.set(1);
        return false;
    }

    let Some(dev) = sc.sc_port.device.get() else {
        printf(format_args!("{}: root hub problem\n", sc.sc_dev.xname()));
        bus.dying.set(1);
        return false;
    };
    if dev.hub.get().is_none() {
        printf(format_args!(
            "{}: root device is not a hub\n",
            sc.sc_dev.xname()
        ));
        bus.dying.set(1);
        return false;
    }
    bus.root_hub.set(Some(dev));

    true
}

/// `usb_detach_roothub`.
fn usb_detach_roothub(sc: &'static UsbSoftc) {
    let bus = sc.bus();
    let Some(rh) = bus.root_hub.get() else {
        return;
    };

    // To avoid races with the usb task thread, mark the root hub as disconnecting and
    // schedule an exploration task to detach it.
    bus.flags.set(bus.flags.get() | USB_BUS_DISCONNECTING);
    // Reset the dying flag in case it has been set by the interrupt handler when unplugging
    // an HC card otherwise the task won't be scheduled. This is safe since a dead HC should
    // not trigger new interrupt.
    bus.dying.set(0);
    usb_needs_explore(rh, false);

    usb_wait_task(rh, &sc.sc_explore_task);

    bus.root_hub.set(None);
}

/// `usb_create_task_threads`.
pub fn usb_create_task_threads(_arg: *mut c_void) {
    match kthread_create(usb_abort_task_thread, ptr::null_mut(), b"usbatsk") {
        Ok(p) => USB_ABORT_TASK_THREAD_PROC.store(ptr::from_ref(p).cast_mut(), Ordering::Relaxed),
        Err(_) => panic(format_args!("unable to create usb abort task thread")),
    }

    match kthread_create(usb_task_thread, ptr::null_mut(), b"usbtask") {
        Ok(p) => USB_TASK_THREAD_PROC.store(ptr::from_ref(p).cast_mut(), Ordering::Relaxed),
        Err(_) => panic(format_args!("unable to create usb task thread")),
    }
}

/// The queue of a task's type.
fn usb_task_queue(t: i8) -> Option<&'static UsbTaskq> {
    match t {
        USB_TASK_TYPE_ABORT => Some(&USB_ABORT_TASKS),
        USB_TASK_TYPE_EXPLORE => Some(&USB_EXPLORE_TASKS),
        USB_TASK_TYPE_GENERIC => Some(&USB_GENERIC_TASKS),
        _ => None,
    }
}

/// `usb_add_task`: add a task to be performed by the task thread. This function can be
/// called from any context and the task will be executed in a process context ASAP.
pub fn usb_add_task(dev: &'static UsbdDevice, task: &'static UsbTask) {
    // If the thread detaching ``dev'' is sleeping, waiting for all submitted transfers to
    // finish, we must be able to enqueue abort tasks. Otherwise timeouts can't give back
    // submitted transfers to the stack.
    if usbd_is_dying(dev) && task.r#type.get() != USB_TASK_TYPE_ABORT {
        return;
    }

    let s = splusb();
    if task.state.get() & USB_TASK_STATE_ONQ == 0 {
        if let Some(q) = usb_task_queue(task.r#type.get()) {
            // SAFETY: not ONQ, so on no queue; `'static`; at splusb() under the kernel lock.
            unsafe { q.0.insert_tail(task) };
        }
        task.state.set(task.state.get() | USB_TASK_STATE_ONQ);
        task.dev.set(Some(dev));
    }
    if task.r#type.get() == USB_TASK_TYPE_ABORT {
        wakeup(ptr::from_ref(&USB_RUN_ABORT_TASKS));
    } else {
        wakeup(ptr::from_ref(&USB_RUN_TASKS));
    }
    splx(s);
}

/// `usb_rem_task`: takes a queued task off its queue.
pub fn usb_rem_task(_dev: &UsbdDevice, task: &UsbTask) {
    if task.state.get() & USB_TASK_STATE_ONQ == 0 {
        return;
    }

    let s = splusb();

    if let Some(q) = usb_task_queue(task.r#type.get()) {
        // SAFETY: ONQ: on its type's queue; at splusb() under the kernel lock.
        unsafe { q.0.remove(task) };
    }
    task.state.set(task.state.get() & !USB_TASK_STATE_ONQ);
    if task.state.get() == USB_TASK_STATE_NONE {
        wakeup(ptr::from_ref(task));
    }

    splx(s);
}

/// `usb_wait_task`: sleeps until the task is neither queued nor running.
pub fn usb_wait_task(_dev: &UsbdDevice, task: &UsbTask) {
    if task.state.get() == USB_TASK_STATE_NONE {
        return;
    }

    let s = splusb();
    while task.state.get() != USB_TASK_STATE_NONE {
        let _ = tsleep_nsec(ptr::from_ref(task), PWAIT, "endtask", INFSLP);
    }
    splx(s);
}

/// `usb_rem_wait_task`.
pub fn usb_rem_wait_task(dev: &UsbdDevice, task: &UsbTask) {
    usb_rem_task(dev, task);
    usb_wait_task(dev, task);
}

/// Runs `task` the way both task threads do: the run bit set before the queued bit is
/// cleared (this avoids state == none between dequeue and execution, which could cause
/// `usb_wait_task()` to do the wrong thing), the function called at `spl0`-ish (the caller's
/// `splx`), the waiters woken when it is done.
fn usb_run_task(task: &UsbTask, s: &mut i32, skip_if_dying: bool) {
    task.state.set(task.state.get() | USB_TASK_STATE_RUN);
    task.state.set(task.state.get() & !USB_TASK_STATE_ONQ);
    // Don't actually execute the task if dying.
    let dying = skip_if_dying && task.dev.get().is_some_and(usbd_is_dying);
    if !dying {
        splx(*s);
        if let Some(f) = task.fun.get() {
            f(task.arg.get());
        }
        *s = splusb();
    }
    task.state.set(task.state.get() & !USB_TASK_STATE_RUN);
    if task.state.get() == USB_TASK_STATE_NONE {
        wakeup(ptr::from_ref(task));
    }
}

/// `usb_task_thread`: runs the explore tasks first, then the generic ones.
pub fn usb_task_thread(_arg: *mut c_void) {
    let mut s = splusb();
    while USB_RUN_TASKS.load(Ordering::Relaxed) != 0 {
        let task = if let Some(t) = USB_EXPLORE_TASKS.0.first() {
            // SAFETY: the head of the queue; at splusb() under the kernel lock.
            unsafe { USB_EXPLORE_TASKS.0.remove(t) };
            t
        } else if let Some(t) = USB_GENERIC_TASKS.0.first() {
            // SAFETY: as above.
            unsafe { USB_GENERIC_TASKS.0.remove(t) };
            t
        } else {
            let _ = tsleep_nsec(ptr::from_ref(&USB_RUN_TASKS), PWAIT, "usbtsk", INFSLP);
            continue;
        };
        usb_run_task(task, &mut s, true);
    }
    splx(s);

    kthread_exit(0);
}

/// `usb_abort_task_thread`: this thread is ONLY for the HCI drivers to be able to abort
/// xfers. Synchronous xfers sleep the task thread, so the aborts need to happen in a
/// different thread.
pub fn usb_abort_task_thread(_arg: *mut c_void) {
    let mut s = splusb();
    while USB_RUN_ABORT_TASKS.load(Ordering::Relaxed) != 0 {
        let Some(task) = USB_ABORT_TASKS.0.first() else {
            let _ = tsleep_nsec(
                ptr::from_ref(&USB_RUN_ABORT_TASKS),
                PWAIT,
                "usbatsk",
                INFSLP,
            );
            continue;
        };
        // SAFETY: the head of the queue; at splusb() under the kernel lock.
        unsafe { USB_ABORT_TASKS.0.remove(task) };
        usb_run_task(task, &mut s, false);
    }
    splx(s);

    kthread_exit(0);
}

/// `usbctlprint`: the `cfprint_t` host controllers pass when they attach `usb`.
pub fn usbctlprint(_aux: *mut c_void, pnp: Option<&[u8]>) -> i32 {
    // only "usb"es can attach to host controllers
    if let Some(pnp) = pnp {
        printf(format_args!("usb at {}", Str(pnp)));
    }

    UNCONF
}

/// The `usb(4)` softc of a minor number, `None` if no such unit is attached.
fn usb_unit(unit: u32) -> Option<&'static UsbSoftc> {
    let dv = USB_CD.cd_dev(unit as i32)?;
    // SAFETY: a `usb` unit in `usb_cd.cd_devs`, attached (not freed while listed).
    Some(usb_softc(unsafe { dv.as_ref() }))
}

/// `usbopen`.
pub fn usbopen(dev: Dev, _flag: i32, _mode: i32, _p: &Proc) -> Result<(), Errno> {
    let Some(sc) = usb_unit(minor(dev)) else {
        return Err(Errno::ENXIO);
    };

    if sc.bus().dying.get() != 0 {
        return Err(Errno::EIO);
    }

    Ok(())
}

/// `usbclose`.
pub fn usbclose(_dev: Dev, _flag: i32, _mode: i32, _p: Option<&Proc>) -> Result<(), Errno> {
    Ok(())
}

/// `usb_fill_udc_task`: `USB_DEVICE_GET_CDESC`'s work, in the task thread.
pub fn usb_fill_udc_task(arg: *mut c_void) {
    // SAFETY: `usbioctl` passes its `UsbDeviceCdesc` and waits for this task to finish.
    let udc = unsafe { &mut *arg.cast::<UsbDeviceCdesc>() };
    let addr = usize::from(udc.udc_addr);

    // check that the bus and device are still present
    let Some(sc) = usb_unit(u32::from(udc.udc_bus)) else {
        return;
    };
    let Some(dev) = sc.bus().device(addr) else {
        return;
    };

    let mut cdesc_len = 0u32;
    let Some(cdesc) = usbd_get_cdesc(dev, udc.udc_config_index, Some(&mut cdesc_len)) else {
        return;
    };
    // SAFETY: `usbd_get_cdesc` returns `cdesc_len` bytes, at least a configuration
    // descriptor's when it succeeds.
    let bytes = unsafe { core::slice::from_raw_parts(cdesc.as_ptr(), cdesc_len as usize) };
    udc.udc_desc = UsbConfigDescriptor::read_from(bytes);
    free(cdesc, M_TEMP, (cdesc_len as usize).max(1));
}

/// `usb_fill_udf_task`: `USB_DEVICE_GET_FDESC`'s work, in the task thread.
pub fn usb_fill_udf_task(arg: *mut c_void) {
    // SAFETY: `usbioctl` passes its `UsbDeviceFdesc` and waits for this task to finish.
    let udf = unsafe { &mut *arg.cast::<UsbDeviceFdesc>() };
    let addr = usize::from(udf.udf_addr);

    // check that the bus and device are still present
    let Some(sc) = usb_unit(u32::from(udf.udf_bus)) else {
        return;
    };
    let Some(dev) = sc.bus().device(addr) else {
        return;
    };

    let cdesc = usbd_get_cdesc(dev, udf.udf_config_index, Some(&mut udf.udf_size));
    udf.udf_data = cdesc.map_or(0, |p| p.as_ptr() as usize);
}

/// Runs `fun(arg)` in the task thread and waits for it (`usb_init_task` on a stack task,
/// `usb_add_task`, `usb_wait_task`).
fn usb_run_in_task_thread(root_hub: &'static UsbdDevice, fun: fn(*mut c_void), arg: *mut c_void) {
    let task = UsbTask::new();
    usb_init_task(&task, fun, arg, USB_TASK_TYPE_GENERIC);
    // SAFETY: the task lives on this stack frame until `usb_wait_task` below has seen it
    // neither queued nor running, after which the task thread no longer touches it
    // (`docs/C_TO_RUST.md`, an object on the caller's stack kept while the caller sleeps).
    let t: &'static UsbTask = unsafe { &*ptr::from_ref(&task) };
    usb_add_task(root_hub, t);
    usb_wait_task(root_hub, t);
    // A dying root hub refuses the task (usb_add_task): it never ran; nothing is queued.
}

/// `usbioctl`: the `/dev/usb*` requests.
pub fn usbioctl(devt: Dev, cmd: u64, data: &mut [u8], flag: i32, p: &Proc) -> Result<(), Errno> {
    let unit = minor(devt);
    let Some(sc) = usb_unit(unit) else {
        return Err(Errno::ENXIO);
    };
    let bus = sc.bus();

    if bus.dying.get() != 0 {
        return Err(Errno::EIO);
    }

    match cmd {
        // USB_SETDEBUG: USB_DEBUG only.
        USB_REQUEST => {
            let mut ur: UsbCtlRequest = ioctl_arg(data);
            let len = usize::from(ugetw(ur.ucr_request.wLength));
            let addr = ur.ucr_addr;

            if flag & FWRITE == 0 {
                return Err(Errno::EBADF);
            }

            // Avoid requests that would damage the bus integrity.
            let rq = ur.ucr_request;
            if (rq.bmRequestType == UT_WRITE_DEVICE && rq.bRequest == UR_SET_ADDRESS)
                || (rq.bmRequestType == UT_WRITE_DEVICE && rq.bRequest == UR_SET_CONFIG)
                || (rq.bmRequestType == UT_WRITE_INTERFACE && rq.bRequest == UR_SET_INTERFACE)
            {
                return Err(Errno::EINVAL);
            }

            if len > 32767 {
                return Err(Errno::EINVAL);
            }
            if !(0..USB_MAX_DEVICES as i32).contains(&addr) {
                return Err(Errno::EINVAL);
            }
            let Some(dev) = bus.device(addr as usize) else {
                return Err(Errno::ENXIO);
            };
            let rw = if rq.bmRequestType & UT_READ != 0 {
                UioRw::UIO_READ
            } else {
                UioRw::UIO_WRITE
            };
            let mut iov = [Iovec {
                iov_base: ur.ucr_data as *mut c_void,
                iov_len: len,
            }];
            let mut uio = Uio {
                uio_iov: &mut iov,
                uio_offset: 0,
                uio_resid: len,
                uio_segflg: UioSeg::UIO_USERSPACE,
                uio_rw: rw,
                uio_procp: Some(p),
            };
            let mut ptr_mem: Option<NonNull<u8>> = None;
            let r = 'ret: {
                if len != 0 {
                    let Some(m) = malloc(len, M_TEMP, M_NOWAIT) else {
                        break 'ret Err(Errno::ENOMEM);
                    };
                    ptr_mem = Some(m);
                    if rw == UioRw::UIO_WRITE {
                        // SAFETY: a fresh allocation of `len` bytes, ours until the free.
                        let buf = unsafe { core::slice::from_raw_parts_mut(m.as_ptr(), len) };
                        if let Err(e) = uiomove(buf, &mut uio) {
                            break 'ret Err(e);
                        }
                    }
                }
                let buf: &mut [u8] = match ptr_mem {
                    // SAFETY: as above.
                    Some(m) => unsafe { core::slice::from_raw_parts_mut(m.as_ptr(), len) },
                    None => &mut [],
                };
                let err = usbd_do_request_flags(
                    dev,
                    &ur.ucr_request,
                    buf,
                    ur.ucr_flags as u16,
                    Some(&mut ur.ucr_actlen),
                    USBD_DEFAULT_TIMEOUT,
                );
                if err.is_err() {
                    break 'ret Err(Errno::EIO);
                }
                // Only if USBD_SHORT_XFER_OK is set.
                let mlen = len.min(ur.ucr_actlen.max(0) as usize);
                if mlen != 0
                    && rw == UioRw::UIO_READ
                    && let Err(e) = uiomove(&mut buf[..mlen], &mut uio)
                {
                    break 'ret Err(e);
                }
                Ok(())
            };
            // ret:
            if let Some(m) = ptr_mem {
                free(m, M_TEMP, len);
            }
            ioctl_ret(data, &ur);
            r
        }

        USB_DEVICEINFO => {
            let mut di: UsbDeviceInfo = ioctl_arg(data);
            let addr = usize::from(di.udi_addr);

            if !(1..USB_MAX_DEVICES).contains(&addr) {
                return Err(Errno::EINVAL);
            }

            let Some(dev) = bus.device(addr) else {
                return Err(Errno::ENXIO);
            };

            usbd_fill_deviceinfo(dev, &mut di);
            ioctl_ret(data, &di);
            Ok(())
        }

        USB_DEVICESTATS => {
            ioctl_ret(data, &bus.stats.get());
            Ok(())
        }

        USB_DEVICE_GET_DDESC => {
            let mut udd: UsbDeviceDdesc = ioctl_arg(data);
            let addr = usize::from(udd.udd_addr);

            if !(1..USB_MAX_DEVICES).contains(&addr) {
                return Err(Errno::EINVAL);
            }

            let Some(dev) = bus.device(addr) else {
                return Err(Errno::ENXIO);
            };

            udd.udd_bus = unit as u8;

            udd.udd_desc = usbd_get_device_descriptor(dev);
            ioctl_ret(data, &udd);
            Ok(())
        }

        USB_DEVICE_GET_CDESC => {
            let mut udc: UsbDeviceCdesc = ioctl_arg(data);
            let addr = usize::from(udc.udc_addr);

            if !(1..USB_MAX_DEVICES).contains(&addr) {
                return Err(Errno::EINVAL);
            }
            if bus.device(addr).is_none() {
                return Err(Errno::ENXIO);
            }
            let Some(rh) = bus.root_hub.get() else {
                return Err(Errno::EIO);
            };

            udc.udc_bus = unit as u8;

            udc.udc_desc.bLength = 0;
            usb_run_in_task_thread(rh, usb_fill_udc_task, ptr::from_mut(&mut udc).cast());
            if udc.udc_desc.bLength == 0 {
                return Err(Errno::EINVAL);
            }
            ioctl_ret(data, &udc);
            Ok(())
        }

        USB_DEVICE_GET_FDESC => {
            let mut udf: UsbDeviceFdesc = ioctl_arg(data);
            let addr = usize::from(udf.udf_addr);

            if !(1..USB_MAX_DEVICES).contains(&addr) {
                return Err(Errno::EINVAL);
            }
            if bus.device(addr).is_none() {
                return Err(Errno::ENXIO);
            }
            let Some(rh) = bus.root_hub.get() else {
                return Err(Errno::EIO);
            };

            udf.udf_bus = unit as u8;

            let save_udf = udf;
            udf.udf_data = 0;
            usb_run_in_task_thread(rh, usb_fill_udf_task, ptr::from_mut(&mut udf).cast());
            let cdesc_len = udf.udf_size as usize;
            let mut len = cdesc_len;
            let cdesc = NonNull::new(udf.udf_data as *mut u8);
            udf = save_udf;
            ioctl_ret(data, &udf);
            let Some(cdesc) = cdesc else {
                return Err(Errno::EINVAL);
            };
            if len > udf.udf_size as usize {
                len = udf.udf_size as usize;
            }
            let mut iov = [Iovec {
                iov_base: udf.udf_data as *mut c_void,
                iov_len: len,
            }];
            let mut uio = Uio {
                uio_iov: &mut iov,
                uio_offset: 0,
                uio_resid: len,
                uio_segflg: UioSeg::UIO_USERSPACE,
                uio_rw: UioRw::UIO_READ,
                uio_procp: Some(p),
            };
            // SAFETY: `usbd_get_cdesc` allocated `cdesc_len` >= `len` bytes; ours until freed.
            let buf = unsafe { core::slice::from_raw_parts_mut(cdesc.as_ptr(), len) };
            let error = uiomove(buf, &mut uio);
            free(cdesc, M_TEMP, cdesc_len.max(1));
            error
        }

        _ => Err(Errno::EINVAL),
    }
}

/// `usb_explore`: explore device tree from the root. We need mutual exclusion to this hub
/// while traversing the device tree, but this is guaranteed since this function is only
/// called from the task thread, with one exception: `usb_attach()` calls this function, but
/// there shouldn't be anything else trying to explore this hub at that time.
pub fn usb_explore(v: *mut c_void) {
    // SAFETY: the explore task's argument is its `usb` softc (`usb_attach`), alive while the
    // task can run (`usb_detach_roothub` waits for it).
    let sc: &'static UsbSoftc = unsafe { &*v.cast::<UsbSoftc>() };
    let bus = sc.bus();

    if bus.dying.get() != 0 {
        return;
    }

    if bus.flags.get() & USB_BUS_CONFIG_PENDING != 0 {
        // If this is a low/full speed hub and there is a high speed hub that hasn't explored
        // yet, reschedule this task, allowing the high speed explore task to run.
        if bus.usbrev.get() < USBREV_2_0 && EXPLORE_PENDING.load(Ordering::Relaxed) > 0 {
            if let Some(rh) = bus.root_hub.get() {
                usb_add_task(rh, &sc.sc_explore_task);
            }
            return;
        }

        // Wait for power to stabilize.
        let now = getmicrouptime();
        let waited = timersub(&now, &sc.sc_ptime.get());
        let waited_ms = waited.tv_sec * 1000 + waited.tv_usec as i64 / 1000;

        let pwrdly = bus
            .root_hub
            .get()
            .and_then(|rh| rh.hub.get())
            .map_or(0, |h| i64::from(h.powerdelay.get()))
            + i64::from(USB_EXTRA_POWER_UP_TIME);
        if pwrdly > waited_ms {
            usb_delay_ms(bus, (pwrdly - waited_ms) as u32);
        }
    }

    if bus.flags.get() & USB_BUS_DISCONNECTING != 0 {
        // Prevent new tasks from being scheduled.
        bus.dying.set(1);

        // Make all devices disconnect.
        if let Some(dev) = sc.sc_port.device.get() {
            let _ = usbd_detach(dev, &sc.sc_dev);
            sc.sc_port.device.set(None);
        }

        bus.flags.set(bus.flags.get() & !USB_BUS_DISCONNECTING);
    } else if let Some(rh) = bus.root_hub.get()
        && let Some(hub) = rh.hub.get()
    {
        hub.explore(rh);
    }

    if bus.flags.get() & USB_BUS_CONFIG_PENDING != 0 {
        if bus.usbrev.get() == USBREV_2_0 && EXPLORE_PENDING.load(Ordering::Relaxed) != 0 {
            EXPLORE_PENDING.fetch_sub(1, Ordering::Relaxed);
        }
        config_pending_decr();
        bus.flags.set(bus.flags.get() & !USB_BUS_CONFIG_PENDING);
    }
}

/// `usb_needs_explore`: schedules an exploration of `dev`'s bus.
pub fn usb_needs_explore(dev: &'static UsbdDevice, first_explore: bool) {
    let bus = dev.bus();
    let Some(usbctl) = bus.usbctl.get() else {
        return;
    };
    // SAFETY: `usbctl` is the bus's attached `usb(4)` device (only `usb` attaches below a
    // host controller), alive while the bus is.
    let usbctl = usb_softc(unsafe { usbctl.as_ref() });

    if !first_explore && bus.flags.get() & USB_BUS_CONFIG_PENDING != 0 {
        return;
    }

    usb_add_task(dev, &usbctl.sc_explore_task);
}

/// `usb_needs_reattach`.
pub fn usb_needs_reattach(dev: &'static UsbdDevice) {
    if let Some(p) = dev.powersrc.get() {
        p.reattach.set(1);
    }
    usb_needs_explore(dev, false);
}

/// `usb_schedsoftintr`: runs the bus's soft interrupt (at once when polling).
pub fn usb_schedsoftintr(bus: &UsbdBus) {
    // In case usb(4) is disabled
    let Some(soft) = bus.soft.get() else {
        return;
    };

    if bus.use_polling.get() != 0 {
        (bus.methods().soft_intr)(ptr::from_ref(bus).cast_mut().cast());
    } else {
        softintr_schedule(soft);
    }
}

/// `usb_activate`.
pub fn usb_activate(self_: &Device, act: i32) -> Result<(), Errno> {
    let sc = usb_softc(self_);
    let bus = sc.bus();

    match act {
        DVACT_QUIESCE => {
            if bus.root_hub.get().is_some() {
                usb_detach_roothub(sc);
            }
            Ok(())
        }
        DVACT_RESUME => {
            bus.dying.set(0);
            Ok(())
        }
        DVACT_WAKEUP => {
            bus.use_polling.set(bus.use_polling.get() + 1);
            if usb_attach_roothub(sc)
                && let Some(rh) = bus.root_hub.get()
            {
                usb_needs_explore(rh, false);
            }
            bus.use_polling.set(bus.use_polling.get() - 1);
            Ok(())
        }
        _ => config_activate_children(self_, act),
    }
}

/// `usb_detach`.
pub fn usb_detach(self_: &Device, _flags: i32) -> Result<(), Errno> {
    let sc = usb_softc(self_);
    let bus = sc.bus();

    if bus.root_hub.get().is_some() {
        usb_detach_roothub(sc);

        if USB_NBUSES.fetch_sub(1, Ordering::Relaxed) - 1 == 0 {
            USB_RUN_TASKS.store(0, Ordering::Relaxed);
            USB_RUN_ABORT_TASKS.store(0, Ordering::Relaxed);
            wakeup(ptr::from_ref(&USB_RUN_ABORT_TASKS));
            wakeup(ptr::from_ref(&USB_RUN_TASKS));
        }
    }

    if let Some(soft) = bus.soft.get() {
        // SAFETY: established by `usb_attach` and not used after this (the field is cleared).
        unsafe { softintr_disestablish(soft) };
        bus.soft.set(None);
    }

    if NBPFILTER > 0
        && let Some(bpfif) = bus.bpfif.get()
    {
        bpfsdetach(bpfif);
        bus.bpfif.set(None);
    }
    Ok(())
}

// Byte offsets of `usb_tap`'s header members (the C's union of `struct usbpcap_ctl_hdr` and
// `struct usbpcap_iso_hdr_full`).
/// `usb_tap`'s header: `hlen`.
const TAP_HLEN: usize = offset_of!(UsbpcapPktHdr, uph_hlen);
/// `usb_tap`'s header: `id`.
const TAP_ID: usize = offset_of!(UsbpcapPktHdr, uph_id);
/// `usb_tap`'s header: `status`.
const TAP_STATUS: usize = offset_of!(UsbpcapPktHdr, uph_status);
/// `usb_tap`'s header: `function`.
const TAP_FUNCTION: usize = offset_of!(UsbpcapPktHdr, uph_function);
/// `usb_tap`'s header: `info`.
const TAP_INFO: usize = offset_of!(UsbpcapPktHdr, uph_info);
/// `usb_tap`'s header: `bus`.
const TAP_BUS: usize = offset_of!(UsbpcapPktHdr, uph_bus);
/// `usb_tap`'s header: `devaddr`.
const TAP_DEVADDR: usize = offset_of!(UsbpcapPktHdr, uph_devaddr);
/// `usb_tap`'s header: `epaddr`.
const TAP_EPADDR: usize = offset_of!(UsbpcapPktHdr, uph_epaddr);
/// `usb_tap`'s header: `xfertype`.
const TAP_XFERTYPE: usize = offset_of!(UsbpcapPktHdr, uph_xfertype);
/// `usb_tap`'s header: `dlen`.
const TAP_DLEN: usize = offset_of!(UsbpcapPktHdr, uph_dlen);
/// `usb_tap`'s header: `stage`.
const TAP_STAGE: usize = offset_of!(UsbpcapCtlHdr, uch_stage);
/// `usb_tap`'s header: `startframe`.
const TAP_STARTFRAME: usize = offset_of!(UsbpcapIsoHdrFull, uih_startframe);
/// `usb_tap`'s header: `nframes`.
const TAP_NFRAMES: usize = offset_of!(UsbpcapIsoHdrFull, uih_nframes);
/// `usb_tap`'s header: `errors`.
const TAP_ERRORS: usize = offset_of!(UsbpcapIsoHdrFull, uih_errors);
/// `usb_tap`'s header: `frames`.
const TAP_FRAMES: usize = offset_of!(UsbpcapIsoHdrFull, uih_frames);
/// `usb_tap`'s header: `pkt`.
const TAP_PKT: usize = size_of::<UsbpcapIsoPkt>();
/// `usb_tap`'s header: `pkt_offset`.
const TAP_PKT_OFFSET: usize = offset_of!(UsbpcapIsoPkt, uip_offset);
/// `usb_tap`'s header: `pkt_length`.
const TAP_PKT_LENGTH: usize = offset_of!(UsbpcapIsoPkt, uip_length);
/// `usb_tap`'s header: `pkt_status`.
const TAP_PKT_STATUS: usize = offset_of!(UsbpcapIsoPkt, uip_status);
/// `usb_tap`'s header: `size`.
const TAP_SIZE: usize = size_of::<UsbpcapIsoHdrFull>();

/// The bytes of `usb_tap`'s header union, written member by member in machine order (the
/// C's stores; `htole*` where the C has them is the identity on both machines).
struct TapHdr([u8; TAP_SIZE]);

impl TapHdr {
    fn put(&mut self, off: usize, bytes: &[u8]) {
        self.0[off..off + bytes.len()].copy_from_slice(bytes);
    }
    fn get_u16(&self, off: usize) -> u16 {
        u16::from_ne_bytes([self.0[off], self.0[off + 1]])
    }
    fn get_u32(&self, off: usize) -> u32 {
        u32::from_ne_bytes([
            self.0[off],
            self.0[off + 1],
            self.0[off + 2],
            self.0[off + 3],
        ])
    }
}

/// `usb_tap`: hands the transfer to the bus's bpf listeners as `DLT_USBPCAP` records.
pub fn usb_tap(bus: &UsbdBus, xfer: &UsbdXfer, dir: u8) {
    if NBPFILTER == 0 {
        return;
    }
    let bpf = bus.bpf.get();
    if bpf.is_null() {
        return;
    }
    let Some(usbctl) = bus.usbctl.get() else {
        return;
    };
    // SAFETY: the bus's attached `usb(4)` device, alive while the bus is.
    let sc = usb_softc(unsafe { usbctl.as_ref() });
    let ed = xfer.pipe().endpoint().edesc();
    let mut h = TapHdr([0; TAP_SIZE]);

    match ue_get_xfertype(ed.bmAttributes) {
        UE_CONTROL => {
            // Control transfer headers include an extra byte
            h.put(
                TAP_HLEN,
                &htole16(size_of::<UsbpcapCtlHdr>() as u16).to_ne_bytes(),
            );
            h.put(TAP_XFERTYPE, &[USBPCAP_TRANSFER_CONTROL]);
        }
        UE_ISOCHRONOUS => {
            let mut nframes = xfer.nframes.get().max(0) as usize;
            // All our drivers use a fixed size (psize) for ISOCHRONOUS packets. Calculate
            // it to determine the correct offset below.
            let psize = (xfer.length.get() as usize)
                .checked_div(nframes)
                .unwrap_or(0);
            if nframes > _USBPCAP_MAX_ISOFRAMES {
                #[cfg(feature = "diagnostic")]
                printf(format_args!(
                    "usb_tap: too many frames: {} > {}\n",
                    nframes, _USBPCAP_MAX_ISOFRAMES
                ));
                // Without DIAGNOSTIC the C would overrun its header; clamp as DIAGNOSTIC does.
                nframes = _USBPCAP_MAX_ISOFRAMES;
            }
            // Isochronous transfer headers include space for one frame
            let flen = nframes.saturating_sub(1) * size_of::<UsbpcapIsoPkt>();
            h.put(
                TAP_HLEN,
                &htole16((size_of::<UsbpcapIsoHdr>() + flen) as u16).to_ne_bytes(),
            );
            h.put(TAP_XFERTYPE, &[USBPCAP_TRANSFER_ISOCHRONOUS]);
            h.put(TAP_STARTFRAME, &0u32.to_ne_bytes()); // not yet used
            h.put(TAP_NFRAMES, &(nframes as u32).to_ne_bytes());
            h.put(TAP_ERRORS, &0u32.to_ne_bytes()); // we don't have per-frame error
            let frlengths = xfer.frlengths.get();
            for i in 0..nframes {
                let at = TAP_FRAMES + i * TAP_PKT;
                // We can't use length, because IN frame may have shorter length of packet
                // whan expected.
                h.put(at + TAP_PKT_OFFSET, &((i * psize) as u32).to_ne_bytes());
                let l = if frlengths.is_null() {
                    0
                } else {
                    // SAFETY: `usbd_setup_isoc_xfer`'s contract: `nframes` lengths at
                    // `frlengths` while the transfer is in flight.
                    u32::from(unsafe { *frlengths.add(i) })
                };
                h.put(at + TAP_PKT_LENGTH, &l.to_ne_bytes());
                // See above, we don't have per-frame error
                h.put(at + TAP_PKT_STATUS, &0u32.to_ne_bytes());
            }
        }
        UE_BULK => {
            h.put(
                TAP_HLEN,
                &htole16(size_of::<UsbpcapPktHdr>() as u16).to_ne_bytes(),
            );
            h.put(TAP_XFERTYPE, &[USBPCAP_TRANSFER_BULK]);
        }
        UE_INTERRUPT => {
            h.put(
                TAP_HLEN,
                &htole16(size_of::<UsbpcapPktHdr>() as u16).to_ne_bytes(),
            );
            h.put(TAP_XFERTYPE, &[USBPCAP_TRANSFER_INTERRUPT]);
        }
        _ => return,
    }

    h.put(TAP_ID, &0u64.to_ne_bytes()); // not yet used
    h.put(TAP_STATUS, &htole32(xfer.status.get() as u32).to_ne_bytes());
    h.put(TAP_FUNCTION, &0u16.to_ne_bytes()); // not yet used
    // htole32(dv_unit) stored into the 16-bit member: its low half on these little-endian
    // machines.
    h.put(TAP_BUS, &(sc.sc_dev.dv_unit.get() as u16).to_ne_bytes());
    h.put(
        TAP_DEVADDR,
        &htole16(u16::from(xfer.device().address.get())).to_ne_bytes(),
    );
    h.put(TAP_EPADDR, &[ed.bEndpointAddress]);
    h.put(TAP_INFO, &[0]);

    let hlen = usize::from(h.get_u16(TAP_HLEN)).min(TAP_SIZE);
    let request = xfer.request.get();
    let is_request = xfer.rqflags.get() & URQ_REQUEST != 0;

    // Outgoing control requests start with a STAGE dump.
    if is_request && dir == USBTAP_DIR_OUT {
        h.put(TAP_STAGE, &[USBPCAP_CONTROL_STAGE_SETUP]);
        h.put(
            TAP_DLEN,
            &(size_of::<UsbDeviceRequest>() as u32).to_ne_bytes(),
        );
        let _ = bpf_tap_hdr(
            bpf,
            Some(&h.0[..hlen]),
            Some(request.as_bytes()),
            BPF_DIRECTION_OUT,
        );
    }

    let len = xfer.length.get() as usize;
    let bpfdir;
    let mut data: Option<&[u8]> = None;
    if dir == USBTAP_DIR_OUT {
        bpfdir = BPF_DIRECTION_OUT;
        if !usbd_xfer_isread(xfer) {
            if len != 0 {
                // SAFETY: the DMA buffer holds the `length` bytes `usbd_transfer` just
                // copied in (or the driver wrote); it lives until the transfer completes.
                data = Some(unsafe {
                    core::slice::from_raw_parts(kernaddr(&xfer.dmabuf, 0).cast_const(), len)
                });
            }
            h.put(TAP_DLEN, &(len as u32).to_ne_bytes());
            if is_request {
                h.put(TAP_STAGE, &[USBPCAP_CONTROL_STAGE_DATA]);
            }
        } else {
            h.put(TAP_DLEN, &0u32.to_ne_bytes());
            if is_request {
                h.put(TAP_STAGE, &[USBPCAP_CONTROL_STAGE_STATUS]);
            }
        }
    } else {
        // USBTAP_DIR_IN
        bpfdir = BPF_DIRECTION_IN;
        h.put(TAP_INFO, &[USBPCAP_INFO_DIRECTION_IN]);
        if usbd_xfer_isread(xfer) {
            h.put(TAP_DLEN, &xfer.actlen.get().to_ne_bytes());
            if is_request {
                h.put(TAP_STAGE, &[USBPCAP_CONTROL_STAGE_DATA]);
            }
        } else {
            h.put(TAP_DLEN, &0u32.to_ne_bytes());
            if is_request {
                h.put(TAP_STAGE, &[USBPCAP_CONTROL_STAGE_STATUS]);
            }
        }
    }

    // ISOCHRONOUS IN from device may have gaps, use full buffer
    if bpfdir == BPF_DIRECTION_IN
        && h.get_u32(TAP_DLEN) > 0
        && h.0[TAP_XFERTYPE] == USBPCAP_TRANSFER_ISOCHRONOUS
    {
        h.put(TAP_DLEN, &(len as u32).to_ne_bytes());
    }

    if bpfdir == BPF_DIRECTION_IN && usbd_xfer_isread(xfer) {
        let dlen = (h.get_u32(TAP_DLEN) as usize).min(len);
        if dlen != 0 {
            // SAFETY: the DMA buffer holds the `length` >= `dlen` bytes the device wrote.
            // `usb_transfer_complete` may already have given the piece back (as in C), but
            // DMA blocks are never unmapped, so the bytes are still mapped memory.
            data = Some(unsafe {
                core::slice::from_raw_parts(kernaddr(&xfer.dmabuf, 0).cast_const(), dlen)
            });
        }
    }

    // Dump bulk/intr/iso data, ctrl DATA or STATUS stage.
    let _ = bpf_tap_hdr(bpf, Some(&h.0[..hlen]), data, bpfdir);

    // Incoming control requests with DATA need a STATUS stage.
    if is_request && dir == USBTAP_DIR_IN && h.0[TAP_STAGE] == USBPCAP_CONTROL_STAGE_DATA {
        h.put(TAP_STAGE, &[USBPCAP_CONTROL_STAGE_STATUS]);
        h.put(TAP_DLEN, &0u32.to_ne_bytes());
        let _ = bpf_tap_hdr(bpf, Some(&h.0[..hlen]), None, BPF_DIRECTION_IN);
    }
}

/// Marks wire structures (see [`UsbWire`]).
macro_rules! usb_wire {
    ($($t:ty),* $(,)?) => {
        $(
            // SAFETY: `#[repr(C)]` of `u8`s and byte arrays only (the size assertions below
            // pin that there is no padding).
            unsafe impl UsbWire for $t {}
        )*
    };
}

usb_wire!(
    UsbDeviceRequest,
    UsbDescriptor,
    UsbDeviceDescriptor,
    UsbConfigDescriptor,
    UsbInterfaceDescriptor,
    UsbInterfaceAssocDescriptor,
    UsbEndpointDescriptor,
    UsbEndpointSsCompDescriptor,
    UsbStringDescriptor,
    UsbHubDescriptor,
    UsbHubSsDescriptor,
    UsbDeviceQualifier,
    UsbOtgDescriptor,
    UsbStatus,
    UsbHubStatus,
    UsbPortStatus,
);

/// Marks the ioctl argument structures [`AbiPod`].
macro_rules! usb_abi {
    ($($t:ty),* $(,)?) => {
        $(
            // SAFETY: `#[repr(C)]` of integers, byte arrays and wire structures, with the C's
            // padding as explicit members (the size assertions below): every byte is
            // initialised and any bit pattern is a value.
            unsafe impl AbiPod for $t {}
        )*
    };
}

usb_abi!(
    UsbCtlRequest,
    UsbAltInterface,
    UsbConfigDesc,
    UsbDeviceCdesc,
    UsbInterfaceDesc,
    UsbEndpointDesc,
    UsbFullDesc,
    UsbDeviceFdesc,
    UsbDeviceDdesc,
    UsbStringDesc,
    UsbCtlReportDesc,
    UsbDeviceInfo,
    UsbCtlReport,
    UsbDeviceStats,
    UsbDeviceDescriptor,
);

const _: () = {
    // The wire structures: the C's `__packed` sizes.
    assert!(size_of::<UsbDeviceRequest>() == 8);
    assert!(size_of::<UsbDescriptor>() == 3);
    assert!(size_of::<UsbDeviceDescriptor>() == USB_DEVICE_DESCRIPTOR_SIZE);
    assert!(size_of::<UsbConfigDescriptor>() == USB_CONFIG_DESCRIPTOR_SIZE);
    assert!(size_of::<UsbInterfaceDescriptor>() == USB_INTERFACE_DESCRIPTOR_SIZE);
    assert!(size_of::<UsbInterfaceAssocDescriptor>() == USB_INTERFACE_ASSOC_DESCRIPTOR_SIZE);
    assert!(size_of::<UsbEndpointDescriptor>() == USB_ENDPOINT_DESCRIPTOR_SIZE);
    assert!(size_of::<UsbEndpointSsCompDescriptor>() == USB_ENDPOINT_SS_COMP_DESCRIPTOR_SIZE);
    assert!(size_of::<UsbStringDescriptor>() == 254);
    assert!(size_of::<UsbHubDescriptor>() == 39);
    assert!(size_of::<UsbHubSsDescriptor>() == 42);
    assert!(size_of::<UsbDeviceQualifier>() == USB_DEVICE_QUALIFIER_SIZE);
    assert!(size_of::<UsbOtgDescriptor>() == 3);
    assert!(size_of::<UsbStatus>() == 2);
    assert!(size_of::<UsbHubStatus>() == 4);
    assert!(size_of::<UsbPortStatus>() == 4);
    assert!(align_of::<UsbStringDescriptor>() == 1);
    assert!(align_of::<UsbHubSsDescriptor>() == 1);

    // The ioctl structures: the C's LP64 layouts (clang on the C header).
    assert!(size_of::<UsbCtlRequest>() == 32);
    assert!(offset_of!(UsbCtlRequest, ucr_data) == 16);
    assert!(offset_of!(UsbCtlRequest, ucr_flags) == 24);
    assert!(offset_of!(UsbCtlRequest, ucr_actlen) == 28);
    assert!(size_of::<UsbAltInterface>() == 12);
    assert!(size_of::<UsbConfigDesc>() == 16);
    assert!(size_of::<UsbDeviceCdesc>() == 20);
    assert!(offset_of!(UsbDeviceCdesc, udc_config_index) == 4);
    assert!(offset_of!(UsbDeviceCdesc, udc_desc) == 8);
    assert!(size_of::<UsbInterfaceDesc>() == 24);
    assert!(offset_of!(UsbInterfaceDesc, uid_desc) == 12);
    assert!(size_of::<UsbEndpointDesc>() == 24);
    assert!(offset_of!(UsbEndpointDesc, ued_desc) == 16);
    assert!(size_of::<UsbFullDesc>() == 16);
    assert!(offset_of!(UsbFullDesc, ufd_size) == 4);
    assert!(offset_of!(UsbFullDesc, ufd_data) == 8);
    assert!(size_of::<UsbDeviceFdesc>() == 24);
    assert!(offset_of!(UsbDeviceFdesc, udf_config_index) == 4);
    assert!(offset_of!(UsbDeviceFdesc, udf_size) == 8);
    assert!(offset_of!(UsbDeviceFdesc, udf_data) == 16);
    assert!(size_of::<UsbDeviceDdesc>() == 20);
    assert!(offset_of!(UsbDeviceDdesc, udd_desc) == 2);
    assert!(size_of::<UsbStringDesc>() == 264);
    assert!(offset_of!(UsbStringDesc, usd_desc) == 8);
    assert!(size_of::<UsbCtlReportDesc>() == 1028);
    assert!(size_of::<UsbCtlReport>() == 1028);
    assert!(size_of::<UsbDeviceInfo>() == 540);
    assert!(offset_of!(UsbDeviceInfo, udi_productNo) == 264);
    assert!(offset_of!(UsbDeviceInfo, udi_power) == 276);
    assert!(offset_of!(UsbDeviceInfo, udi_nports) == 280);
    assert!(offset_of!(UsbDeviceInfo, udi_devnames) == 284);
    assert!(offset_of!(UsbDeviceInfo, udi_ports) == 348);
    assert!(offset_of!(UsbDeviceInfo, udi_serial) == 412);
    assert!(size_of::<UsbDeviceStats>() == 32);
};

#[cfg(test)]
mod tests;
