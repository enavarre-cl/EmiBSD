/* $OpenBSD: umass_scsi.h,v 1.6 2016/08/03 13:44:49 krw Exp $ */
/*	$NetBSD: umass_scsipi.h,v 1.1 2001/12/24 13:25:53 augustss Exp $	*/
/*	$OpenBSD: umass_scsi.c,v 1.65 2024/05/23 03:21:09 jsg Exp $ */
/*	$NetBSD: umass_scsipi.c,v 1.9 2003/02/16 23:14:08 augustss Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 2001 The NetBSD Foundation, Inc.
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
 * Copyright (c) 2001 The NetBSD Foundation, Inc.
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
//! umass(4)'s SCSI adapter: `<dev/usb/umass_scsi.h>` and `dev/usb/umass_scsi.c`.
//!
//! Upstream: sys/dev/usb/umass_scsi.h @ 3ce1f3f79392, sys/dev/usb/umass_scsi.c @ 3ce1f3f79392
//!
//! `umass_scsi_attach` attaches a `scsibus` with one opening (an iopool of one I/O, the
//! softc itself) and two targets, the adapter at 0 and the device at 1 with `maxlun + 1`
//! LUNs. `umass_scsi_cmd` hands each command to the wire protocol (`umass_bbb_transfer`,
//! `umass_cbi_transfer`); `umass_scsi_cb` ends it, fetching the sense data with a REQUEST
//! SENSE of its own when the command failed or its status is unknown. Commands with
//! `SCSI_POLL` (the probe's) run with the bus polling and synchronous xfers, and have
//! completed when the wire protocol returns.
//!
//! ## Deviations
//! - The header and the file share this module.
//! - `struct umass_scsi_softc` is allocated with `malloc` and initialised in place (the C's
//!   `M_ZERO` alone), and freed with `free`.
//! - `UMASS_DEBUG` is not in GENERIC: the `DPRINTF`s, the timing (`microtime`) and the
//!   target check of `umass_scsi_cmd` under it are absent.
//! - `umass_scsi_attach` and `umass_scsi_detach` return `Result`; `umass_scsi_probe` is a
//!   `dev_probe` returning `Ok(())` for the C's 0.
//! - The command reaches the wire protocol as a copy of its first `cmdlen` bytes (at most
//!   the 16 of `struct scsi_generic`), which the wire protocol copies at once, as the C's
//!   `&xs->cmd` is.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::{self, NonNull};

use crate::dev::usb::umassvar::{
    DIR_IN, DIR_NONE, DIR_OUT, STATUS_CMD_FAILED, STATUS_CMD_OK, STATUS_CMD_UNKNOWN,
    STATUS_WIRE_FAILED, UMASS_CPROTO_ATAPI, UMASS_CPROTO_RBC, UMASS_CPROTO_SCSI, UMASS_CPROTO_UFI,
    UMASS_MAX_TRANSFER_SIZE, UmassSoftc,
};
use crate::dev::usb::usb::UsbDeviceInfo;
use crate::dev::usb::usb_subr::usbd_fill_deviceinfo;
use crate::dev::usb::usbdi::{
    USBD_INVAL, USBD_NORMAL_COMPLETION, USBD_SYNCHRONOUS, USBD_TIMEOUT, splusb, usbd_is_dying,
    usbd_set_polling,
};
use crate::dev::usb::usbdi_util::usb_detach_wakeup;
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::subr_autoconf::{config_detach, config_found};
use crate::kern::subr_prf::{panic, printf};
use crate::machine::intr::splx;
use crate::scsi::scsi_all::{
    REQUEST_SENSE, SCSI_CMD_LUN_SHIFT, ScsiSense, ScsiSenseData, ScsiWire,
};
use crate::scsi::scsi_base::{scsi_done, scsi_iopool_init};
use crate::scsi::scsiconf::{
    ADEV_NOSENSE, DEVID_F_PRINT, DEVID_SERIAL, SCSI_DATA_IN, SCSI_DATA_OUT, SCSI_POLL, SDEV_ATAPI,
    SDEV_UFI, SDEV_UMASS, ScsiAdapter, ScsiIo, ScsiIopool, ScsiLink, ScsiXfer, ScsibusAttachArgs,
    XS_DRIVER_STUFFUP, XS_NOERROR, XS_RESET, XS_SENSE, XS_SHORTSENSE, XS_TIMEOUT, devid_alloc,
    scsiprint,
};
use crate::sys::device::Device;
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_USBDEV, M_WAITOK, M_ZERO};

/// `struct umass_scsi_softc`.
pub struct UmassScsiSoftc {
    /// `sc_child`: the scsibus.
    pub sc_child: Cell<Option<NonNull<Device>>>,
    /// `sc_iopool`: one opening.
    pub sc_iopool: ScsiIopool,
    /// `sc_open`: the opening is taken.
    pub sc_open: Cell<i32>,

    /// `sc_sense_cmd`: the REQUEST SENSE of the autosense.
    pub sc_sense_cmd: Cell<ScsiSense>,
}

impl UmassScsiSoftc {
    /// A zeroed softc (`malloc(..., M_ZERO)`).
    pub const fn new() -> Self {
        Self {
            sc_child: Cell::new(None),
            sc_iopool: ScsiIopool::new(),
            sc_open: Cell::new(0),
            sc_sense_cmd: Cell::new(ScsiSense {
                opcode: 0,
                byte2: 0,
                unused: [0; 2],
                length: 0,
                control: 0,
            }),
        }
    }
}

impl Default for UmassScsiSoftc {
    fn default() -> Self {
        Self::new()
    }
}

/// `UMASS_SCSIID_HOST`.
pub const UMASS_SCSIID_HOST: u16 = 0x00;
/// `UMASS_SCSIID_DEVICE`.
pub const UMASS_SCSIID_DEVICE: u16 = 0x01;

/// `umass_scsi_switch`.
pub static UMASS_SCSI_SWITCH: ScsiAdapter = ScsiAdapter {
    scsi_cmd: umass_scsi_cmd,
    dev_minphys: None,
    dev_probe: Some(umass_scsi_probe),
    dev_free: None,
    ioctl: None,
};

/// `sc->bus`, set by `umass_scsi_attach` before the bus can issue commands.
fn umass_scsi_bus(sc: &UmassSoftc) -> &'static UmassScsiSoftc {
    match sc.bus.get() {
        // SAFETY: `umass_scsi_attach` allocated and initialised it; it is freed only by
        // `umass_scsi_detach`, after the scsibus (the only user) is detached.
        Some(p) => unsafe { &*p.as_ptr().cast_const() },
        None => panic(format_args!("{}: no scsi bus", sc.sc_dev.xname())),
    }
}

/// `link->bus->sb_adapter_softc`: the umass softc of a link.
fn umass_link_softc(link: &ScsiLink) -> &'static UmassSoftc {
    let p = link.bus().sb_adapter_softc.get();
    if p.is_null() {
        panic(format_args!("umass: bus without an adapter softc"));
    }
    // SAFETY: `umass_scsi_attach` attaches its scsibus with its `UmassSoftc` as
    // `saa_adapter_softc`, and only that bus reaches `umass_scsi_switch`; the softc
    // outlives the bus (`umass_detach` detaches it first).
    unsafe { &*p.cast::<UmassSoftc>().cast_const() }
}

/// `umass_scsi_attach`: attaches the SCSI bus of a umass.
pub fn umass_scsi_attach(sc: &'static UmassSoftc) -> Result<(), Errno> {
    let mut flags: u16 = 0;

    let Some(mem) = malloc(size_of::<UmassScsiSoftc>(), M_USBDEV, M_WAITOK | M_ZERO) else {
        return Err(Errno::ENOMEM);
    };
    let p = mem.cast::<UmassScsiSoftc>();
    // SAFETY: a fresh allocation of the softc's size, aligned for it (malloc's alignment
    // covers every kernel type), ours until `umass_scsi_detach` frees it.
    unsafe { p.as_ptr().write(UmassScsiSoftc::new()) };
    // SAFETY: just initialised; it lives until `umass_scsi_detach`.
    let scbus: &'static UmassScsiSoftc = unsafe { &*p.as_ptr().cast_const() };

    sc.bus.set(Some(p));

    match sc.sc_cmd.get() {
        UMASS_CPROTO_RBC | UMASS_CPROTO_SCSI => {}
        UMASS_CPROTO_UFI => flags |= SDEV_UFI | SDEV_ATAPI,
        UMASS_CPROTO_ATAPI => flags |= SDEV_ATAPI,
        _ => {}
    }

    // SAFETY: umass_io_get and umass_io_put take this umass_scsi_softc as their cookie; it
    // lives as long as the bus.
    unsafe {
        scsi_iopool_init(
            &scbus.sc_iopool,
            p.as_ptr().cast(),
            umass_io_get,
            umass_io_put,
        )
    };

    let mut saa = ScsibusAttachArgs::new();
    saa.saa_adapter_buswidth = 2;
    saa.saa_adapter = Some(&UMASS_SCSI_SWITCH);
    saa.saa_adapter_softc = ptr::from_ref(sc).cast_mut().cast();
    saa.saa_adapter_target = UMASS_SCSIID_HOST;
    saa.saa_luns = sc.maxlun.get().wrapping_add(1);
    saa.saa_openings = 1;
    saa.saa_quirks = sc.sc_busquirks.get() as u16;
    saa.saa_pool = Some(&scbus.sc_iopool);
    saa.saa_flags = SDEV_UMASS | flags;
    saa.saa_wwpn = 0;
    saa.saa_wwnn = 0;

    sc.sc_refcnt.set(sc.sc_refcnt.get() + 1);
    scbus.sc_child.set(config_found(
        &sc.sc_dev,
        ptr::from_mut(&mut saa).cast(),
        Some(scsiprint),
    ));
    sc.sc_refcnt.set(sc.sc_refcnt.get() - 1);
    if sc.sc_refcnt.get() < 0 {
        usb_detach_wakeup(&sc.sc_dev);
    }

    Ok(())
}

/// `umass_scsi_detach`: detaches the SCSI bus and frees the bus data.
pub fn umass_scsi_detach(sc: &UmassSoftc, flags: i32) -> Result<(), Errno> {
    let mut rv = Ok(());

    if let Some(p) = sc.bus.get() {
        // SAFETY: allocated and initialised by `umass_scsi_attach`, freed only below.
        let scbus = unsafe { p.as_ref() };
        if let Some(child) = scbus.sc_child.get() {
            // SAFETY: the scsibus `config_found` attached under this umass; nothing here
            // uses it after the detach.
            rv = unsafe { config_detach(child, flags) };
        }
        free(p.cast(), M_USBDEV, size_of::<UmassScsiSoftc>());
        sc.bus.set(None);
    }

    rv
}

/// `umass_scsi_probe`: makes a device id from the USB IDs and the serial number.
pub fn umass_scsi_probe(link: &'static ScsiLink) -> Result<(), Errno> {
    let sc = umass_link_softc(link);

    // dont fake devids when more than one scsi device can attach.
    if sc.maxlun.get() > 0 {
        return Ok(());
    }

    // SAFETY: `UsbDeviceInfo` is made of integers and arrays of them: all-zero is a value.
    let mut udi: UsbDeviceInfo = unsafe { core::mem::zeroed() };
    usbd_fill_deviceinfo(sc.udev(), &mut udi);

    // Create a fake devid using the vendor and product ids and the last 12 characters of
    // serial number, as recommended by Section 4.1.1 of the USB Mass Storage Class - Bulk
    // Only Transport spec.
    let len = udi
        .udi_serial
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(udi.udi_serial.len());
    if len >= 12 {
        let buf = umass_scsi_devid(
            udi.udi_vendorNo,
            udi.udi_productNo,
            &udi.udi_serial[len - 12..len],
        );
        link.id.set(devid_alloc(
            DEVID_SERIAL,
            DEVID_F_PRINT,
            (buf.len() - 1) as u8,
            &buf,
        ));
    }

    Ok(())
}

/// `snprintf(buf, sizeof(buf), "%04x%04x%s", vendor, product, serial)` of
/// `umass_scsi_probe`: 20 characters and the NUL.
pub fn umass_scsi_devid(vendor: u16, product: u16, serial: &[u8]) -> [u8; 21] {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut buf = [0u8; 21];
    for (i, shift) in [12u32, 8, 4, 0].into_iter().enumerate() {
        buf[i] = HEX[usize::from((vendor >> shift) & 0xf)];
        buf[4 + i] = HEX[usize::from((product >> shift) & 0xf)];
    }
    let n = serial.len().min(12);
    buf[8..8 + n].copy_from_slice(&serial[..n]);
    buf
}

/// `umass_scsi_cmd`: the adapter's `scsi_cmd`.
pub fn umass_scsi_cmd(xs: &'static ScsiXfer) {
    let sc_link = xs.link();
    let sc = umass_link_softc(sc_link);

    let done = 'done: {
        if usbd_is_dying(sc.udev()) {
            xs.error.set(XS_DRIVER_STUFFUP);
            break 'done true;
        }

        let cmd = xs.cmd.get();
        let cmdlen = (xs.cmdlen.get().max(0) as usize).min(size_of_val(&cmd));
        let datalen = xs.datalen();

        let mut dir = DIR_NONE;
        if datalen != 0 {
            match xs.flags.get() & (SCSI_DATA_IN | SCSI_DATA_OUT) {
                SCSI_DATA_IN => dir = DIR_IN,
                SCSI_DATA_OUT => dir = DIR_OUT,
                _ => {}
            }
        }

        if datalen > UMASS_MAX_TRANSFER_SIZE as i32 {
            printf(format_args!("umass_cmd: large datalen, {}\n", datalen));
            xs.error.set(XS_DRIVER_STUFFUP);
            break 'done true;
        }

        let lun = i32::from(sc_link.lun.get());
        let timeout = xs.timeout.get() as u32;
        let priv_ = ptr::from_ref(xs).cast_mut().cast::<c_void>();
        if xs.flags.get() & SCSI_POLL != 0 {
            usbd_set_polling(sc.udev(), true);
            sc.sc_xfer_flags.set(USBD_SYNCHRONOUS);
            sc.polled_xfer_status.set(USBD_INVAL);
            // SAFETY: `xs.data` is valid for `datalen` bytes (at most
            // `UMASS_MAX_TRANSFER_SIZE`, checked above) until `scsi_done`, which
            // `umass_scsi_cb` calls; `priv_` is the transfer it expects.
            unsafe {
                (sc.methods().wire_xfer)(
                    sc,
                    lun,
                    &cmd.as_bytes()[..cmdlen],
                    xs.data(),
                    datalen,
                    dir,
                    timeout,
                    umass_scsi_cb,
                    priv_,
                )
            };
            sc.sc_xfer_flags.set(0);
            usbd_set_polling(sc.udev(), false);
            // scsi_done() has already been called.
        } else {
            // SAFETY: as above.
            unsafe {
                (sc.methods().wire_xfer)(
                    sc,
                    lun,
                    &cmd.as_bytes()[..cmdlen],
                    xs.data(),
                    datalen,
                    dir,
                    timeout,
                    umass_scsi_cb,
                    priv_,
                )
            };
            // scsi_done() has already been called.
        }
        false
    };

    // Return if command finishes early.
    if done {
        scsi_done(xs);
    }
}

/// `(struct scsi_xfer *)priv`: the transfer of a umass callback.
fn umass_priv_xs(priv_: *mut c_void) -> &'static ScsiXfer {
    if priv_.is_null() {
        panic(format_args!("umass: callback without its scsi_xfer"));
    }
    // SAFETY: `umass_scsi_cmd` and `umass_scsi_cb` pass the transfer they are running,
    // which stays allocated until `scsi_done`.
    unsafe { &*priv_.cast::<ScsiXfer>().cast_const() }
}

/// The polled transfer's `xs->error`: the end of `umass_scsi_cb` and
/// `umass_scsi_sense_cb`.
fn umass_scsi_polled_error(sc: &UmassSoftc, xs: &ScsiXfer) {
    if xs.flags.get() & SCSI_POLL != 0 && xs.error.get() == XS_NOERROR {
        match sc.polled_xfer_status.get() {
            USBD_NORMAL_COMPLETION => xs.error.set(XS_NOERROR),
            USBD_TIMEOUT => xs.error.set(XS_TIMEOUT),
            _ => xs.error.set(XS_DRIVER_STUFFUP),
        }
    }
}

/// `umass_scsi_cb`: the end of a command.
pub fn umass_scsi_cb(sc: &'static UmassSoftc, priv_: *mut c_void, residue: i32, status: i32) {
    let scbus = umass_scsi_bus(sc);
    let xs = umass_priv_xs(priv_);
    let link = xs.link();

    // The C's `int` to `size_t`.
    xs.resid.set(residue as isize as usize);

    let mut status = status;
    if status == STATUS_CMD_UNKNOWN {
        // we can't issue REQUEST SENSE
        if xs.link().quirks.get() & ADEV_NOSENSE != 0 {
            // If no residue and no other USB error, command succeeded.
            if residue == 0 {
                xs.error.set(XS_NOERROR);
            // Some devices return a short INQUIRY response, omitting response data from
            // the "vendor specific data" on...
            } else if xs.cmd.get().opcode == crate::scsi::scsi_all::INQUIRY
                && residue < xs.datalen()
            {
                xs.error.set(XS_NOERROR);
            } else {
                xs.error.set(XS_DRIVER_STUFFUP);
            }
        } else {
            // FALLTHROUGH
            status = STATUS_CMD_FAILED;
        }
    }

    match status {
        STATUS_CMD_OK => xs.error.set(XS_NOERROR),
        STATUS_CMD_UNKNOWN => {} // ADEV_NOSENSE, decided above
        STATUS_CMD_FAILED => {
            // fetch sense data
            sc.sc_sense.set(1);
            let mut sense_cmd = ScsiSense::zeroed();
            sense_cmd.opcode = REQUEST_SENSE;
            sense_cmd.byte2 = (link.lun.get() as u8) << SCSI_CMD_LUN_SHIFT;
            sense_cmd.length = size_of::<ScsiSenseData>() as u8;
            scbus.sc_sense_cmd.set(sense_cmd);

            let poll = xs.flags.get() & SCSI_POLL != 0;
            if poll {
                usbd_set_polling(sc.udev(), true);
                sc.sc_xfer_flags.set(USBD_SYNCHRONOUS);
                sc.polled_xfer_status.set(USBD_INVAL);
            }
            // scsi_done() has already been called.
            // SAFETY: `xs.sense` belongs to the transfer, which stays allocated until
            // `scsi_done` (in `umass_scsi_sense_cb`); nothing else touches it meanwhile.
            unsafe {
                (sc.methods().wire_xfer)(
                    sc,
                    i32::from(link.lun.get()),
                    sense_cmd.as_bytes(),
                    xs.sense.as_ptr().cast(),
                    size_of::<ScsiSenseData>() as i32,
                    DIR_IN,
                    xs.timeout.get() as u32,
                    umass_scsi_sense_cb,
                    ptr::from_ref(xs).cast_mut().cast(),
                )
            };
            if poll {
                sc.sc_xfer_flags.set(0);
                usbd_set_polling(sc.udev(), false);
            }
            return;
        }
        STATUS_WIRE_FAILED => xs.error.set(XS_RESET),
        _ => panic(format_args!(
            "{}: Unknown status {} in umass_scsi_cb",
            sc.sc_dev.xname(),
            status
        )),
    }

    umass_scsi_polled_error(sc, xs);

    scsi_done(xs);
}

/// `umass_scsi_sense_cb`: finalise a completed autosense operation.
pub fn umass_scsi_sense_cb(sc: &'static UmassSoftc, priv_: *mut c_void, residue: i32, status: i32) {
    let xs = umass_priv_xs(priv_);

    sc.sc_sense.set(0);
    match status {
        STATUS_CMD_OK | STATUS_CMD_UNKNOWN => {
            // getting sense data succeeded
            if residue == 0 || residue == 14 {
                // XXX
                xs.error.set(XS_SENSE);
            } else {
                xs.error.set(XS_SHORTSENSE);
            }
        }
        _ => xs.error.set(XS_DRIVER_STUFFUP),
    }

    umass_scsi_polled_error(sc, xs);

    scsi_done(xs);
}

/// `umass_io_get`: the one opening, when it is free.
///
/// # Safety
///
/// `cookie` is a live [`UmassScsiSoftc`] (the one `umass_scsi_attach` gave
/// `scsi_iopool_init`).
pub unsafe fn umass_io_get(cookie: *mut c_void) -> Option<ScsiIo> {
    // SAFETY: the caller's guarantee.
    let scbus = unsafe { &*cookie.cast::<UmassScsiSoftc>().cast_const() };
    let mut io = None;

    let s = splusb();
    if scbus.sc_open.get() == 0 {
        scbus.sc_open.set(1);
        io = NonNull::new(cookie); // just has to be non-NULL
    }
    splx(s);

    io
}

/// `umass_io_put`: gives the opening back.
///
/// # Safety
///
/// As for [`umass_io_get`].
pub unsafe fn umass_io_put(cookie: *mut c_void, _io: ScsiIo) {
    // SAFETY: the caller's guarantee.
    let scbus = unsafe { &*cookie.cast::<UmassScsiSoftc>().cast_const() };

    let s = splusb();
    scbus.sc_open.set(0);
    splx(s);
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn devid_is_ids_then_the_serial_tail() {
        let buf = umass_scsi_devid(0x46f4, 0x0001, b"123456789abc");
        assert_eq!(&buf[..20], b"46f40001123456789abc");
        assert_eq!(buf[20], 0);
    }

    #[test]
    fn the_opening_is_taken_once() {
        let scbus = std::boxed::Box::leak(std::boxed::Box::new(UmassScsiSoftc::new()));
        let cookie = ptr::from_mut(scbus).cast::<c_void>();
        // SAFETY: a live umass_scsi_softc.
        let io = unsafe { umass_io_get(cookie) }.expect("free opening");
        // SAFETY: as above.
        assert!(unsafe { umass_io_get(cookie) }.is_none());
        // SAFETY: as above; `io` came from it.
        unsafe { umass_io_put(cookie, io) };
        // SAFETY: as above.
        assert!(unsafe { umass_io_get(cookie) }.is_some());
    }
}
/* </TESTS> */
