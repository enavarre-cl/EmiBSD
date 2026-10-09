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
//! uhid(4): the generic USB HID driver, a child of `uhidev(4)`: `/dev/uhid*` gives a program
//! the input reports of a report ID (`read(2)`, queued in a clist), lets it send output
//! reports (`write(2)`) and the report ioctls of `uhidev_ioctl`.
//!
//! Upstream: sys/dev/usb/uhid.c @ 3ce1f3f79392, sys/dev/usb/uhid.h @ 3ce1f3f79392
//!
//! `uhid_match` takes any report ID that no better driver claimed
//! (`UMATCH_IFACECLASS_GENERIC`). `uhid_intr` queues each input report (without its report
//! ID); `uhid_do_open` opens the interrupt pipe through `uhidev_open` and `uhid_do_read`
//! drains the queue, sleeping when it is empty.
//!
//! ## Deviations
//! - `UHID_DEBUG` is not in GENERIC: the `DPRINTF`s are absent. `fido(4)` and `ujoy(4)`
//!   (GENERIC's `fido* at uhidev?` and `ujoy* at uhidev?`, which share `uhid_lookup`,
//!   `uhid_do_open` and the `uhid*` entry points) are not ported: `NFIDO` and `NUJOY` are 0,
//!   so `uhid_lookup` knows only `uhid_cd`.
//! - The device switch entries are Rust functions of the types `Cdevsw` takes (`Dev`,
//!   `Uio`, `&mut [u8]` for the ioctl argument, `Result<(), Errno>`); `uhid_do_ioctl` maps
//!   the C's `-1` of `uhidev_ioctl` (`Ok(false)`) to `ENOTTY` as the C does.
//! - `uhid_intr` gets the report as a slice (see `uhidev.rs`); `sc_obuf` is a raw pointer to
//!   the `M_USBDEV` block allocated at open, as in C.
//! - A device whose `sc_udev` is not set yet counts as dying.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::{self, NonNull};

use crate::dev::hid::hid::{hid_feature, hid_input, hid_output, hid_report_size};
use crate::dev::usb::uhidev::uhidev_get_report_desc;
use crate::dev::usb::uhidev::{
    UHIDEV_OPEN, Uhidev, UhidevAttachArg, uhidev_close, uhidev_ioctl, uhidev_open,
    uhidev_set_report,
};
use crate::dev::usb::usb::{USB_GET_DEVICEINFO, UsbDeviceInfo};
use crate::dev::usb::usb_subr::usbd_fill_deviceinfo;
use crate::dev::usb::usbdi::{UMATCH_IFACECLASS_GENERIC, UMATCH_NONE, splusb, usbd_is_dying};
use crate::dev::usb::usbdi_util::{usb_detach_wait, usb_detach_wakeup};
use crate::dev::usb::usbhid::UHID_OUTPUT_REPORT;
use crate::kern::kern_event::{
    klist_insert_locked, klist_invalidate, klist_remove_locked, seltrue_kqfilter,
};
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_subr::uiomove;
use crate::kern::kern_synch::{tsleep_nsec, wakeup};
use crate::kern::subr_prf::printf;
use crate::kern::sys_generic::selwakeup;
use crate::kern::tty_subr::{b_to_q, clalloc, clfree, q_to_b};
use crate::kern::vfs_subr::vdevgone;
use crate::machine::conf::{cdevsw, nchrdev};
use crate::machine::intr::splx;
use crate::sys::conf::DevTypeOpen;
use crate::sys::device::{CfMatch, Cfattach, Cfdriver, DV_DULL, Device, Softc};
use crate::sys::errno::Errno;
use crate::sys::event::{EVFILT_READ, EVFILT_WRITE, FILTEROP_ISFD, Filterops, Knote};
use crate::sys::filio::FIOASYNC;
use crate::sys::ioctl::{ioctl_arg, ioctl_ret};
use crate::sys::malloc::{M_USBDEV, M_WAITOK};
use crate::sys::param::{PCATCH, PZERO};
use crate::sys::proc::Proc;
use crate::sys::selinfo::Selinfo;
use crate::sys::systm::INFSLP;
use crate::sys::tty::Clist;
use crate::sys::types::{Dev, major, minor};
use crate::sys::uio::Uio;
use crate::sys::vnode::{IO_NDELAY, VCHR};

/// `UHID_ASLP`: waiting for device data (`sc_state`).
pub const UHID_ASLP: u8 = 0x01;

/// `UHID_CHUNK`: chunk size for read.
pub const UHID_CHUNK: usize = 128;
/// `UHID_BSIZE`: buffer size.
pub const UHID_BSIZE: i32 = 1020;

/// `struct uhid_softc`.
#[repr(C)]
pub struct UhidSoftc {
    /// `sc_hdev`: the `uhidev` child head.
    pub sc_hdev: Uhidev,

    /// `sc_obuf`: the output report buffer (`M_USBDEV`, `sc_osize` bytes), open only.
    pub sc_obuf: Cell<*mut u8>,

    /// `sc_q`: the input reports.
    pub sc_q: Clist,
    /// `sc_rsel`.
    pub sc_rsel: Selinfo,
    /// `sc_state`: driver state, `UHID_ASLP`.
    pub sc_state: Cell<u8>,

    /// `sc_refcnt`.
    pub sc_refcnt: Cell<i32>,
}

// SAFETY: `#[repr(C)]`, the `Uhidev` (whose first member is the `Device`) first; the
// `Clist` (null ring) and `Selinfo` (no knotes) are valid zeroed and the rest are integer
// and raw pointer `Cell`s.
unsafe impl Softc for UhidSoftc {}

/// `uhid_cd`.
pub static UHID_CD: Cfdriver = Cfdriver::new(b"uhid", DV_DULL, 0);

/// `uhid_ca`.
pub static UHID_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<UhidSoftc>(),
    ca_match: Some(uhid_match),
    ca_attach: uhid_attach,
    ca_detach: Some(uhid_detach),
    ca_activate: None,
};

/// `NUHID`: `uhid* at uhidev?` is configured (`needs-count`; GENERIC has it).
pub const NUHID: i32 = 1;

/// `(struct uhid_softc *)self`.
fn uhid_softc(self_: &Device) -> &'static UhidSoftc {
    // SAFETY: only called with devices `uhid_ca` made (uhid's own entry points); an
    // attached device lives until `config_detach` frees it after `uhid_detach`.
    unsafe { &*ptr::from_ref(self_.softc::<UhidSoftc>()) }
}

/// `UHIDUNIT(dev)`.
pub const fn uhidunit(dev: Dev) -> u32 {
    minor(dev)
}

/// `usbd_is_dying(sc->sc_hdev.sc_udev)`.
fn uhid_dying(sc: &UhidSoftc) -> bool {
    match sc.sc_hdev.sc_udev.get() {
        Some(udev) => usbd_is_dying(udev),
        None => true,
    }
}

/// `uhid_lookup`: the softc of the device number `dev`.
pub fn uhid_lookup(dev: Dev) -> Option<&'static UhidSoftc> {
    let cdev = cdevsw(major(dev));
    // fidoopen and ujoyopen (NFIDO, NUJOY) are not ported.
    if !core::ptr::fn_addr_eq(cdev.d_open, uhidopen as DevTypeOpen) {
        return None;
    }
    let unit = uhidunit(dev) as i32;
    if unit < UHID_CD.cd_ndevs.get() {
        let dv = UHID_CD.cd_dev(unit)?;
        // SAFETY: an attached uhid, in `cd_devs` until `config_detach`.
        return Some(uhid_softc(unsafe { dv.as_ref() }));
    }
    None
}

/// `uhid_match`: any report ID the better drivers left.
pub fn uhid_match(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: `uhidev_attach` passes a `UhidevAttachArg` that lives across the
    // `config_found_sm` calling this.
    let uha = unsafe { &*aux.cast::<UhidevAttachArg<'_>>() };

    if uha.claim_multiple_reportid() {
        return UMATCH_NONE;
    }

    UMATCH_IFACECLASS_GENERIC
}

/// `uhid_attach`.
pub fn uhid_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    let sc = uhid_softc(self_);
    // SAFETY: as in `uhid_match`, valid during the attach.
    let uha = unsafe { &*aux.cast::<UhidevAttachArg<'_>>() };

    sc.sc_hdev.sc_intr.set(Some(uhid_intr));
    sc.sc_hdev.sc_parent.set(Some(uha.parent));
    sc.sc_hdev.sc_udev.set(Some(uha.uaa.device()));
    sc.sc_hdev.sc_report_id.set(uha.reportid);

    let desc = uhidev_get_report_desc(uha.parent);
    let repid = uha.reportid;
    sc.sc_hdev
        .sc_isize
        .set(hid_report_size(desc, hid_input, repid));
    sc.sc_hdev
        .sc_osize
        .set(hid_report_size(desc, hid_output, repid));
    sc.sc_hdev
        .sc_fsize
        .set(hid_report_size(desc, hid_feature, repid));

    printf(format_args!(
        ": input={}, output={}, feature={}\n",
        sc.sc_hdev.sc_isize.get(),
        sc.sc_hdev.sc_osize.get(),
        sc.sc_hdev.sc_fsize.get()
    ));
}

/// `uhid_detach`.
pub fn uhid_detach(self_: &Device, _flags: i32) -> Result<(), Errno> {
    let sc = uhid_softc(self_);

    if sc.sc_hdev.sc_state.get() & UHIDEV_OPEN != 0 {
        let s = splusb();
        sc.sc_refcnt.set(sc.sc_refcnt.get() - 1);
        if sc.sc_refcnt.get() >= 0 {
            // Wake everyone
            wakeup(ptr::from_ref(&sc.sc_q));
            // Wait for processes to go away.
            usb_detach_wait(&sc.sc_hdev.sc_dev);
        }
        splx(s);
    }

    // locate the major number
    let n = nchrdev();
    let maj = (0..n)
        .find(|&maj| core::ptr::fn_addr_eq(cdevsw(maj).d_open, uhidopen as DevTypeOpen))
        .unwrap_or(n);

    // Nuke the vnodes for any open instances (calls close).
    let mn = self_.dv_unit.get() as u32;
    vdevgone(maj, mn, mn, VCHR);

    let s = splusb();
    klist_invalidate(&sc.sc_rsel.si_note);
    splx(s);

    Ok(())
}

/// `uhid_intr`: an input report (without its report ID): queue it and wake the readers.
pub fn uhid_intr(addr: &Uhidev, data: &mut [u8]) {
    // SAFETY: `uhid_attach` set `sc_intr` to this function in the `sc_hdev` of a `UhidSoftc`,
    // which `uhidev_intr` hands back: the head of the softc, which has `UhidSoftc`'s layout.
    let sc = unsafe { &*ptr::from_ref(addr).cast::<UhidSoftc>() };

    let _ = b_to_q(data, &sc.sc_q);

    if sc.sc_state.get() & UHID_ASLP != 0 {
        sc.sc_state.set(sc.sc_state.get() & !UHID_ASLP);
        wakeup(ptr::from_ref(&sc.sc_q));
    }
    selwakeup(&sc.sc_rsel);
}

/// `uhidopen`.
pub fn uhidopen(dev: Dev, flag: i32, mode: i32, p: &Proc) -> Result<(), Errno> {
    uhid_do_open(dev, flag, mode, p)
}

/// `uhid_do_open`.
pub fn uhid_do_open(dev: Dev, _flag: i32, _mode: i32, _p: &Proc) -> Result<(), Errno> {
    let Some(sc) = uhid_lookup(dev) else {
        return Err(Errno::ENXIO);
    };

    if uhid_dying(sc) {
        return Err(Errno::ENXIO);
    }

    if sc.sc_hdev.sc_state.get() & UHIDEV_OPEN != 0 {
        return Err(Errno::EBUSY);
    }

    clalloc(&sc.sc_q, UHID_BSIZE, false);

    if let Err(error) = uhidev_open(&sc.sc_hdev) {
        clfree(&sc.sc_q);
        return Err(error);
    }

    let obuf = malloc(
        sc.sc_hdev.sc_osize.get().max(0) as usize,
        M_USBDEV,
        M_WAITOK,
    );
    sc.sc_obuf
        .set(obuf.map_or(ptr::null_mut(), NonNull::as_ptr));

    Ok(())
}

/// `uhidclose`.
pub fn uhidclose(dev: Dev, _flag: i32, _mode: i32, _p: Option<&Proc>) -> Result<(), Errno> {
    let Some(sc) = uhid_lookup(dev) else {
        return Err(Errno::ENXIO);
    };

    clfree(&sc.sc_q);
    if let Some(obuf) = NonNull::new(sc.sc_obuf.replace(ptr::null_mut())) {
        free(obuf, M_USBDEV, sc.sc_hdev.sc_osize.get().max(0) as usize);
    }
    uhidev_close(&sc.sc_hdev);

    Ok(())
}

/// `uhid_do_read`: sleep until a report is queued (`IO_NDELAY`: `EWOULDBLOCK`), then move
/// as many chunks as the caller has room for.
pub fn uhid_do_read(sc: &UhidSoftc, uio: &mut Uio<'_>, flag: i32) -> Result<(), Errno> {
    let mut error: Result<(), Errno> = Ok(());
    let mut buffer = [0u8; UHID_CHUNK];

    let s = splusb();
    while sc.sc_q.c_cc.get() == 0 {
        if flag & IO_NDELAY != 0 {
            splx(s);
            return Err(Errno::EWOULDBLOCK);
        }
        sc.sc_state.set(sc.sc_state.get() | UHID_ASLP);
        error = tsleep_nsec(ptr::from_ref(&sc.sc_q), PZERO | PCATCH, "uhidrea", INFSLP);
        if uhid_dying(sc) {
            error = Err(Errno::EIO);
        }
        if error.is_err() {
            sc.sc_state.set(sc.sc_state.get() & !UHID_ASLP);
            break;
        }
    }
    splx(s);

    // Transfer as many chunks as possible.
    while sc.sc_q.c_cc.get() > 0 && uio.uio_resid > 0 && error.is_ok() {
        let mut length = (sc.sc_q.c_cc.get() as usize).min(uio.uio_resid);
        if length > buffer.len() {
            length = buffer.len();
        }

        // Remove a small chunk from the input queue.
        let _ = q_to_b(&sc.sc_q, &mut buffer[..length]);

        // Copy the data to the user process.
        error = uiomove(&mut buffer[..length], uio);
        if error.is_err() {
            break;
        }
    }

    error
}

/// `uhidread`.
pub fn uhidread(dev: Dev, uio: &mut Uio<'_>, flag: i32) -> Result<(), Errno> {
    let Some(sc) = uhid_lookup(dev) else {
        return Err(Errno::ENXIO);
    };

    sc.sc_refcnt.set(sc.sc_refcnt.get() + 1);
    let error = uhid_do_read(sc, uio, flag);
    sc.sc_refcnt.set(sc.sc_refcnt.get() - 1);
    if sc.sc_refcnt.get() < 0 {
        usb_detach_wakeup(&sc.sc_hdev.sc_dev);
    }
    error
}

/// `uhid_do_write`: send `uio` as an output report, zero-padded to the report size.
pub fn uhid_do_write(sc: &UhidSoftc, uio: &mut Uio<'_>, _flag: i32) -> Result<(), Errno> {
    if uhid_dying(sc) {
        return Err(Errno::EIO);
    }

    let size = sc.sc_hdev.sc_osize.get().max(0) as usize;
    let Some(obuf) = NonNull::new(sc.sc_obuf.get()) else {
        return Err(Errno::EIO);
    };
    // SAFETY: `uhid_do_open` allocated `sc_osize` bytes at `sc_obuf` for as long as the
    // device is open (`uhidclose` clears the pointer), and the kernel lock serializes the
    // entry points.
    let obuf = unsafe { core::slice::from_raw_parts_mut(obuf.as_ptr(), size) };
    if uio.uio_resid > size {
        return Err(Errno::EMSGSIZE);
    } else if uio.uio_resid < size {
        // don't leak kernel memory to the USB device
        obuf[uio.uio_resid..].fill(0);
    }
    let resid = uio.uio_resid;
    uiomove(&mut obuf[..resid], uio)?;

    let parent = sc.sc_hdev.parent();
    match uhidev_set_report(
        parent,
        i32::from(UHID_OUTPUT_REPORT),
        i32::from(sc.sc_hdev.sc_report_id.get()),
        obuf,
    ) {
        Ok(n) if n == size => Ok(()),
        _ => Err(Errno::EIO),
    }
}

/// `uhidwrite`.
pub fn uhidwrite(dev: Dev, uio: &mut Uio<'_>, flag: i32) -> Result<(), Errno> {
    let Some(sc) = uhid_lookup(dev) else {
        return Err(Errno::ENXIO);
    };

    sc.sc_refcnt.set(sc.sc_refcnt.get() + 1);
    let error = uhid_do_write(sc, uio, flag);
    sc.sc_refcnt.set(sc.sc_refcnt.get() - 1);
    if sc.sc_refcnt.get() < 0 {
        usb_detach_wakeup(&sc.sc_hdev.sc_dev);
    }
    error
}

/// `uhid_do_ioctl`: `FIOASYNC` (handled above), the device info, then `uhidev`'s report
/// ioctls; `ENOTTY` for any other.
pub fn uhid_do_ioctl(
    sc: &UhidSoftc,
    cmd: u64,
    addr: &mut [u8],
    flag: i32,
    p: &Proc,
) -> Result<(), Errno> {
    if uhid_dying(sc) {
        return Err(Errno::EIO);
    }

    match cmd {
        FIOASYNC => {
            // All handled in the upper FS layer.
        }

        USB_GET_DEVICEINFO => {
            let Some(udev) = sc.sc_hdev.sc_udev.get() else {
                return Err(Errno::EIO);
            };
            let mut di: UsbDeviceInfo = ioctl_arg(addr);
            usbd_fill_deviceinfo(udev, &mut di);
            ioctl_ret(addr, &di);
        }

        // USB_GET_REPORT_DESC, USB_GET_REPORT, USB_SET_REPORT, USB_GET_REPORT_ID and the rest.
        _ => {
            return match uhidev_ioctl(&sc.sc_hdev, cmd, addr, flag, Some(p)) {
                Ok(true) => Ok(()),
                Ok(false) => Err(Errno::ENOTTY),
                Err(e) => Err(e),
            };
        }
    }
    Ok(())
}

/// `uhidioctl`.
pub fn uhidioctl(dev: Dev, cmd: u64, addr: &mut [u8], flag: i32, p: &Proc) -> Result<(), Errno> {
    let Some(sc) = uhid_lookup(dev) else {
        return Err(Errno::ENXIO);
    };

    sc.sc_refcnt.set(sc.sc_refcnt.get() + 1);
    let error = uhid_do_ioctl(sc, cmd, addr, flag, p);
    sc.sc_refcnt.set(sc.sc_refcnt.get() - 1);
    if sc.sc_refcnt.get() < 0 {
        usb_detach_wakeup(&sc.sc_hdev.sc_dev);
    }
    error
}

/// The softc a knote hangs on (`kn->kn_hook`).
fn kn_uhid(kn: &Knote) -> &UhidSoftc {
    // SAFETY: `uhidkqfilter` set `kn_hook` to the softc, and `uhid_detach` invalidates the
    // klist (detaching every knote) before the device goes.
    unsafe { &*kn.kn_hook.get().cast::<UhidSoftc>() }
}

/// `filt_uhidrdetach`.
pub fn filt_uhidrdetach(kn: &Knote) {
    let sc = kn_uhid(kn);

    let s = splusb();
    klist_remove_locked(&sc.sc_rsel.si_note, kn);
    splx(s);
}

/// `filt_uhidread`: readable when reports are queued; their size is the data.
pub fn filt_uhidread(kn: &Knote, _hint: i64) -> bool {
    let sc = kn_uhid(kn);

    kn.kn_data().set(i64::from(sc.sc_q.c_cc.get()));
    kn.kn_data().get() > 0
}

/// `uhidread_filtops`.
pub static UHIDREAD_FILTOPS: Filterops = Filterops {
    f_flags: FILTEROP_ISFD,
    f_attach: None,
    f_detach: Some(filt_uhidrdetach),
    f_event: Some(filt_uhidread),
    f_modify: None,
    f_process: None,
};

/// `uhidkqfilter`.
pub fn uhidkqfilter(dev: Dev, kn: &Knote) -> Result<(), Errno> {
    let Some(sc) = uhid_lookup(dev) else {
        return Err(Errno::ENXIO);
    };

    if uhid_dying(sc) {
        return Err(Errno::ENXIO);
    }

    let klist = match kn.kn_filter().get() {
        EVFILT_READ => {
            kn.kn_fop.set(Some(&UHIDREAD_FILTOPS));
            &sc.sc_rsel.si_note
        }
        EVFILT_WRITE => return seltrue_kqfilter(dev, kn),
        _ => return Err(Errno::EINVAL),
    };

    kn.kn_hook.set(ptr::from_ref(sc).cast_mut().cast());

    let s = splusb();
    klist_insert_locked(klist, kn);
    splx(s);

    Ok(())
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for `uhid`: the report queue (`uhid_intr`, `uhid_do_read` in chunks, the
    // non-blocking read), the knote filter and the lookup of a device number with no uhid.

    use std::boxed::Box;
    use std::vec::Vec;
    use std::{assert, assert_eq};

    use super::*;
    use crate::kern::subr_pool::tests::setup_real_memory;
    use crate::sys::uio::{Iovec, UioRw, UioSeg};

    fn softc() -> Box<UhidSoftc> {
        // SAFETY: a zeroed softc is a valid one (`unsafe impl Softc`): `config_make_softc`
        // allocates them zeroed.
        Box::new(unsafe { core::mem::zeroed::<UhidSoftc>() })
    }

    /// Reads up to `len` bytes of `sc`'s queue without blocking.
    fn read(sc: &UhidSoftc, len: usize) -> Result<Vec<u8>, Errno> {
        let mut buf = std::vec![0u8; len];
        let mut iov = [Iovec {
            iov_base: buf.as_mut_ptr().cast(),
            iov_len: len,
        }];
        let mut uio = Uio {
            uio_iov: &mut iov,
            uio_offset: 0,
            uio_resid: len,
            uio_segflg: UioSeg::UIO_SYSSPACE,
            uio_rw: UioRw::UIO_READ,
            uio_procp: None,
        };
        uhid_do_read(sc, &mut uio, IO_NDELAY)?;
        let done = len - uio.uio_resid;
        buf.truncate(done);
        Ok(buf)
    }

    #[test]
    fn reports_are_queued_and_read_back() {
        let _g = setup_real_memory();
        let sc = softc();
        clalloc(&sc.sc_q, UHID_BSIZE, false);

        assert_eq!(read(&sc, 8), Err(Errno::EWOULDBLOCK));

        uhid_intr(&sc.sc_hdev, &mut [1, 2, 3]);
        uhid_intr(&sc.sc_hdev, &mut [4, 5]);
        assert_eq!(sc.sc_q.c_cc.get(), 5);
        assert_eq!(read(&sc, 4).unwrap(), [1, 2, 3, 4]);
        assert_eq!(read(&sc, 16).unwrap(), [5]);
        assert_eq!(read(&sc, 16), Err(Errno::EWOULDBLOCK));

        clfree(&sc.sc_q);
    }

    #[test]
    fn a_read_moves_one_chunk_at_a_time() {
        let _g = setup_real_memory();
        let sc = softc();
        clalloc(&sc.sc_q, UHID_BSIZE, false);

        let data: Vec<u8> = (0..300u32).map(|i| i as u8).collect();
        for c in data.chunks(60) {
            uhid_intr(&sc.sc_hdev, &mut c.to_vec());
        }
        assert_eq!(sc.sc_q.c_cc.get(), 300);
        // The loop in `uhid_do_read` moves UHID_CHUNK bytes per `q_to_b`, as many times as the
        // caller has room for.
        assert_eq!(read(&sc, 1000).unwrap(), data);
        clfree(&sc.sc_q);
    }

    #[test]
    fn the_queue_drops_what_does_not_fit() {
        let _g = setup_real_memory();
        let sc = softc();
        clalloc(&sc.sc_q, UHID_BSIZE, false);

        let mut big = std::vec![7u8; UHID_BSIZE as usize + 100];
        uhid_intr(&sc.sc_hdev, &mut big);
        assert!(sc.sc_q.c_cc.get() <= UHID_BSIZE);
        clfree(&sc.sc_q);
    }

    #[test]
    fn the_read_filter_counts_the_queue() {
        let _g = setup_real_memory();
        let sc = softc();
        clalloc(&sc.sc_q, UHID_BSIZE, false);
        let kn = Knote::new();
        kn.kn_hook.set(ptr::from_ref(&*sc).cast_mut().cast());

        assert!(!filt_uhidread(&kn, 0));
        uhid_intr(&sc.sc_hdev, &mut [9, 9, 9]);
        assert!(filt_uhidread(&kn, 0));
        assert_eq!(kn.kn_data().get(), 3);
        clfree(&sc.sc_q);
    }

    #[test]
    fn the_unit_is_the_minor() {
        assert_eq!(uhidunit(crate::sys::types::makedev(62, 3)), 3);
        assert_eq!(UHID_CHUNK, 128);
        assert_eq!(UHID_BSIZE, 1020);
    }
}
/* </TESTS> */
