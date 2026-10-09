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
 * Copyright (c) 1998, 2000 The NetBSD Foundation, Inc.
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
//! ucom(4): the tty layer for USB serial ports. A USB serial driver (`uftdi(4)`, ...)
//! attaches a `ucom` child with its endpoints and a table of methods; `ucom` makes the tty
//! (`/dev/ttyU*`, `/dev/cuaU*`), opens the bulk pipes at the first open and moves the bytes
//! between the tty's queues and the transfers.
//!
//! Upstream: sys/dev/usb/ucom.c @ 3ce1f3f79392
//!
//! This code is very heavily based on the 16550 driver, `com.c`. The minor number is the unit
//! (`UCOMUNIT`, 7 bits) and the call-out bit (`UCOMCUA`, `0x80`). The first open of a unit
//! opens the bulk-in and bulk-out pipes (or takes the HID device's), allocates the transfers,
//! calls the driver's `ucom_open` and starts the read transfer; `ucomreadcb` feeds the bytes
//! to the line discipline's `l_rint` and starts the next read, `ucomstart` writes the
//! contiguous run of the output queue as one transfer and `ucomwritecb` flushes what was
//! written from the queue and calls `l_start`. `sc_lock` serialises opens and closes; `sc_refcnt`
//! lets `ucom_detach` wait for the entry points that are running.
//!
//! ## Deviations
//! - `UCOM_DEBUG` is not in GENERIC: the `DPRINTF`s are absent. `ucom_hwiflow` is the C's
//!   (`#if 0`) body-less function and `ucomstop` the C's (`#if 0`) stub returning 0.
//! - The tty is reached through `sc_tty`, a `*const Tty` set by `ucom_attach` and cleared by
//!   `ucom_detach`; entry points that can run after the detach (`ucomwritecb`, `ucomreadcb`,
//!   `ucomstart`) return when it is null where the C dereferences it.
//! - `ucomparam` works on a copy of the requested termios (`SOFTCAR` forces `CLOCAL` and
//!   clears `HUPCL` in the copy); the C changes the caller's.
//! - `ucom_do_open`'s `fail_3` frees the input xfer even for a HID-based port (whose xfer
//!   is the HID device's, `sc_bulkin_no == -1`); here it is only freed when `ucom` allocated it.
//! - `sysctl_ucominit` builds `hw.ucomnames` on each call into a fixed buffer
//!   ([`UcomNames`]); the C caches the string and rebuilds it when `ucom_change` is set
//!   (so `ucom_change` and `sysctl_ucomlock` are absent). A name that does not fit is left
//!   out, as the C does.
//! - `ucomread` and `ucomwrite` answer `EIO` for a unit that is not attached; the C
//!   dereferences `cd_devs[unit]`.

use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::slice;

use crate::dev::usb::ucomvar::{
    UCOM_SET_BREAK, UCOM_SET_DTR, UCOM_SET_RTS, UCOM_UNK_PORTNO, UMCR_DTR, UMCR_RTS, UMSR_CTS,
    UMSR_DCD, UMSR_DSR, UMSR_RI, UMSR_TERI, UcomAttachArgs, UcomMethods,
};
use crate::dev::usb::uhidev::UhidevSoftc;
use crate::dev::usb::usb::USBD_SHORT_XFER_OK;
use crate::dev::usb::usb_subr::usbd_get_location;
use crate::dev::usb::usbdi::{
    USBD_CANCELLED, USBD_EXCLUSIVE_USE, USBD_IN_PROGRESS, USBD_INVAL, USBD_IOERROR, USBD_NO_COPY,
    USBD_NO_TIMEOUT, USBD_NORMAL_COMPLETION, UsbdStatus, splusb, usbd_alloc_buffer,
    usbd_alloc_xfer, usbd_clear_endpoint_stall_async, usbd_close_pipe, usbd_free_buffer,
    usbd_free_xfer, usbd_get_xfer_status, usbd_is_dying, usbd_open_pipe, usbd_setup_xfer,
    usbd_transfer,
};
use crate::dev::usb::usbdi_util::{usb_detach_wait, usb_detach_wakeup};
use crate::dev::usb::usbdivar::{UsbdDevice, UsbdInterface, UsbdPipe, UsbdXfer};
use crate::kern::kern_prot::suser;
use crate::kern::kern_rwlock::{rw_enter_write, rw_exit_write, rw_init};
use crate::kern::kern_synch::tsleep_nsec;
use crate::kern::subr_prf::{Str, panic, printf, snprintf};
use crate::kern::tty::{
    TTCLOS, TTOPEN, ttioctl, ttsetwater, ttwakeup, ttwakeupwr, ttychars, ttyclose, ttyflush,
    ttyfree, ttymalloc, ttysleep, ttytstamp,
};
use crate::kern::tty_conf::linesw;
use crate::kern::tty_subr::{ndflush, ndqb};
use crate::kern::vfs_subr::vdevgone;
use crate::machine::conf::{cdevsw, nchrdev};
use crate::machine::cpu::curproc;
use crate::machine::intr::{spltty, splx};
use crate::sys::conf::DevTypeOpen;
use crate::sys::device::{CfMatch, Cfattach, Cfdriver, DV_TTY, DVF_ACTIVE, Device, Softc, UNCONF};
use crate::sys::errno::Errno;
use crate::sys::fcntl::{FNONBLOCK, FREAD, FWRITE, O_NONBLOCK};
use crate::sys::ioctl::{ioctl_arg, ioctl_ret};
use crate::sys::param::PCATCH;
use crate::sys::proc::Proc;
use crate::sys::rwlock::Rwlock;
use crate::sys::termios::{CLOCAL, CRTSCTS, HUPCL, MDMBUF, Termios};
use crate::sys::time::sec_to_nsec;
use crate::sys::tty::{
    TS_BUSY, TS_CARR_ON, TS_FLUSH, TS_ISOPEN, TS_TIMEOUT, TS_TTSTOP, TS_WOPEN, TS_XCLUDE, TTIPRI,
    Tty,
};
use crate::sys::ttycom::{
    TIOCCBRK, TIOCCDTR, TIOCFLAG_CLOCAL, TIOCFLAG_CRTSCTS, TIOCFLAG_MDMBUF, TIOCFLAG_SOFTCAR,
    TIOCGFLAGS, TIOCM_CD, TIOCM_CTS, TIOCM_DSR, TIOCM_DTR, TIOCM_RI, TIOCM_RTS, TIOCMBIC, TIOCMBIS,
    TIOCMGET, TIOCMSET, TIOCSBRK, TIOCSDTR, TIOCSFLAGS,
};
use crate::sys::ttydefaults::{
    TTYDEF_CFLAG, TTYDEF_IFLAG, TTYDEF_LFLAG, TTYDEF_OFLAG, TTYDEF_SPEED,
};
use crate::sys::types::Dev;
use crate::sys::uio::Uio;
use crate::sys::vnode::VCHR;

/// `NUCOM`: `ucom* at uftdi?` is configured (`needs-flag`; GENERIC has it).
pub const NUCOM: i32 = 1;

/// `UCOMUNIT_MASK`.
pub const UCOMUNIT_MASK: u32 = 0x7f;
/// `UCOMCUA_MASK`.
pub const UCOMCUA_MASK: u32 = 0x80;

/// `UCOMUNIT(x)`.
pub const fn ucomunit(x: Dev) -> u32 {
    (x as u32) & UCOMUNIT_MASK
}

/// `UCOMCUA(x)`.
pub const fn ucomcua(x: Dev) -> u32 {
    (x as u32) & UCOMCUA_MASK
}

/// `ROUTEROOTPORT(_x)`.
pub const fn routerootport(x: u32) -> u32 {
    x & 0xff
}

/// `ROUTESTRING(_x)`.
pub const fn routestring(x: u32) -> u32 {
    (x >> 8) & 0xfffff
}

/// `struct ucom_softc`. All-zero is a valid value (an unattached softc).
#[repr(C)]
pub struct UcomSoftc {
    /// `sc_dev`: base device.
    pub sc_dev: Device,

    /// `sc_uparent`: USB device.
    pub sc_uparent: core::cell::Cell<Option<&'static UsbdDevice>>,
    /// `sc_uhidev`: hid device (if deeper).
    pub sc_uhidev: core::cell::Cell<Option<&'static UhidevSoftc>>,

    /// `sc_iface`: data interface.
    pub sc_iface: core::cell::Cell<Option<&'static UsbdInterface>>,

    /// `sc_bulkin_no`: bulk in endpoint address.
    pub sc_bulkin_no: core::cell::Cell<i32>,
    /// `sc_bulkin_pipe`: bulk in pipe.
    pub sc_bulkin_pipe: core::cell::Cell<Option<&'static UsbdPipe>>,
    /// `sc_ixfer`: read request.
    pub sc_ixfer: core::cell::Cell<Option<&'static UsbdXfer>>,
    /// `sc_ibuf`: read buffer.
    pub sc_ibuf: core::cell::Cell<*mut u8>,
    /// `sc_ibufsize`: read buffer size.
    pub sc_ibufsize: core::cell::Cell<u32>,
    /// `sc_ibufsizepad`: read buffer size padded.
    pub sc_ibufsizepad: core::cell::Cell<u32>,

    /// `sc_bulkout_no`: bulk out endpoint address.
    pub sc_bulkout_no: core::cell::Cell<i32>,
    /// `sc_bulkout_pipe`: bulk out pipe.
    pub sc_bulkout_pipe: core::cell::Cell<Option<&'static UsbdPipe>>,
    /// `sc_oxfer`: write request.
    pub sc_oxfer: core::cell::Cell<Option<&'static UsbdXfer>>,
    /// `sc_obuf`: write buffer.
    pub sc_obuf: core::cell::Cell<*mut u8>,
    /// `sc_obufsize`: write buffer size.
    pub sc_obufsize: core::cell::Cell<u32>,
    /// `sc_opkthdrlen`: header length of output packet.
    pub sc_opkthdrlen: core::cell::Cell<u32>,

    /// `sc_ipipe`: hid interrupt input pipe.
    pub sc_ipipe: core::cell::Cell<Option<&'static UsbdPipe>>,
    /// `sc_opipe`: hid interrupt pipe.
    pub sc_opipe: core::cell::Cell<Option<&'static UsbdPipe>>,

    /// `sc_methods`.
    pub sc_methods: core::cell::Cell<Option<&'static UcomMethods>>,
    /// `sc_parent`.
    pub sc_parent: core::cell::Cell<*mut c_void>,
    /// `sc_portno`.
    pub sc_portno: core::cell::Cell<i32>,

    /// `sc_tty`: our tty.
    pub sc_tty: core::cell::Cell<*const Tty>,
    /// `sc_lsr`.
    pub sc_lsr: core::cell::Cell<u8>,
    /// `sc_msr`.
    pub sc_msr: core::cell::Cell<u8>,
    /// `sc_mcr`.
    pub sc_mcr: core::cell::Cell<u8>,
    /// `sc_tx_stopped`.
    pub sc_tx_stopped: core::cell::Cell<u8>,
    /// `sc_swflags`.
    pub sc_swflags: core::cell::Cell<i32>,

    /// `sc_cua`.
    pub sc_cua: core::cell::Cell<u8>,
    /// `sc_error`.
    pub sc_error: core::cell::Cell<i32>,

    /// `sc_lock`: lock during open.
    pub sc_lock: Rwlock,
    /// `sc_open`.
    pub sc_open: core::cell::Cell<i32>,
    /// `sc_refcnt`.
    pub sc_refcnt: core::cell::Cell<i32>,
}

// SAFETY: `#[repr(C)]` with the device first; the rwlock is all-zero valid (a free lock,
// named by `rw_init` at attach), and every other member is a `Cell` of an integer, a pointer,
// or an `Option` of a reference: all valid as zero bits.
unsafe impl Softc for UcomSoftc {}

/// `ucom_cd`.
pub static UCOM_CD: Cfdriver = Cfdriver::new(b"ucom", DV_TTY, 0);

/// `ucom_ca`.
pub static UCOM_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<UcomSoftc>(),
    ca_match: Some(ucom_match),
    ca_attach: ucom_attach,
    ca_detach: Some(ucom_detach),
    ca_activate: None,
};

/// `(struct ucom_softc *)self`.
pub fn ucom_softc_of(self_: &Device) -> &'static UcomSoftc {
    // SAFETY: only called with devices `ucom_ca` made (ucom's own entry points and the
    // driver that attached the child), whose softc is a `UcomSoftc`; attached devices live
    // until `config_detach` frees them after `ucom_detach`.
    unsafe { &*ptr::from_ref(self_.softc::<UcomSoftc>()) }
}

/// `ucom_cd.cd_devs[unit]`: the attached ucom of a unit.
fn ucom_unit(unit: u32) -> Option<&'static UcomSoftc> {
    let unit = unit as i32;
    if unit >= UCOM_CD.cd_ndevs.get() {
        return None;
    }
    let dv = UCOM_CD.cd_dev(unit)?;
    // SAFETY: an attached ucom, in `cd_devs` until `config_detach`.
    Some(ucom_softc_of(unsafe { dv.as_ref() }))
}

impl UcomSoftc {
    /// `sc->sc_tty`: `None` before the attach made it and after the detach freed it.
    fn tty(&self) -> Option<&'static Tty> {
        // SAFETY: `sc_tty` is null or `ttymalloc`'s tty, freed only by `ucom_detach`.
        unsafe { self.sc_tty.get().as_ref() }
    }

    /// `sc->sc_uparent`. Set by `ucom_attach` before any other entry point can run.
    fn uparent(&self) -> &'static UsbdDevice {
        match self.sc_uparent.get() {
            Some(d) => d,
            None => panic(format_args!("{}: no USB device", self.sc_dev.xname())),
        }
    }

    /// `usbd_is_dying(sc->sc_uparent)`.
    fn dying(&self) -> bool {
        usbd_is_dying(self.uparent())
    }

    /// The methods `ucom_attach` was given.
    fn methods(&self) -> &'static UcomMethods {
        match self.sc_methods.get() {
            Some(m) => m,
            None => panic(format_args!("{}: no methods", self.sc_dev.xname())),
        }
    }
}

/// `(void *)sc`: the softc as an xfer's `priv`.
fn ucom_priv(sc: &'static UcomSoftc) -> *mut c_void {
    ptr::from_ref(sc).cast_mut().cast()
}

/// `(struct ucom_softc *)priv`: the softc an xfer's callback gets back.
fn ucom_priv_softc(priv_: *mut c_void) -> &'static UcomSoftc {
    if priv_.is_null() {
        panic(format_args!("ucom: xfer without its softc"));
    }
    // SAFETY: ucom sets up its xfers with its own softc as `priv` (`ucomstart`,
    // `ucomstartread`); the softc outlives them.
    unsafe { &*priv_.cast::<UcomSoftc>().cast_const() }
}

/// `ucom_lock`.
pub fn ucom_lock(sc: &UcomSoftc) {
    rw_enter_write(&sc.sc_lock);
}

/// `ucom_unlock`.
pub fn ucom_unlock(sc: &UcomSoftc) {
    rw_exit_write(&sc.sc_lock);
}

/// `ucom_match`.
pub fn ucom_match(_parent: Option<&Device>, _match: &CfMatch, _aux: *mut c_void) -> i32 {
    1
}

/// `ucom_attach`.
pub fn ucom_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    let sc = ucom_softc_of(self_);
    // SAFETY: `ucomsubmatch`'s caller (`config_found_sm` in a driver such as `uftdi_attach`)
    // passes its `ucom_attach_args`, valid during the attach.
    let uca = unsafe { &*aux.cast::<UcomAttachArgs>() };

    if let Some(info) = uca.info {
        printf(format_args!(", {}", info));
    }

    sc.sc_uparent.set(uca.device);
    sc.sc_iface.set(uca.iface);
    sc.sc_bulkout_no.set(uca.bulkout);
    sc.sc_bulkin_no.set(uca.bulkin);
    sc.sc_uhidev.set(uca.uhidev);
    sc.sc_ibufsize.set(uca.ibufsize);
    sc.sc_ibufsizepad.set(uca.ibufsizepad);
    sc.sc_obufsize.set(uca.obufsize);
    sc.sc_opkthdrlen.set(uca.opkthdrlen);
    sc.sc_methods.set(Some(uca.methods));
    sc.sc_parent.set(uca.arg);
    sc.sc_portno.set(uca.portno);

    if let Some(iface) = uca.iface
        && let Some((bus, route, ifaceno)) = usbd_get_location(uca.device, iface)
    {
        printf(format_args!(
            ": usb{}.{}.{:05x}.{}",
            bus,
            routerootport(route),
            routestring(route),
            ifaceno
        ));
    }
    printf(format_args!("\n"));

    let tp = ttymalloc(1_000_000);
    tp.t_oproc.set(Some(ucomstart));
    tp.t_param.set(Some(ucomparam));
    sc.sc_tty.set(ptr::from_ref(tp));
    sc.sc_cua.set(0);

    rw_init(&sc.sc_lock, "ucomlk");
}

/// `ucom_major`: the major number of ucom (`cdevsw[maj].d_open == ucomopen`), `nchrdev` if
/// none.
fn ucom_major() -> u32 {
    let n = nchrdev();
    (0..n)
        .find(|&maj| ptr::fn_addr_eq(cdevsw(maj).d_open, ucomopen as DevTypeOpen))
        .unwrap_or(n)
}

/// `ucom_detach`.
pub fn ucom_detach(self_: &Device, _flags: i32) -> Result<(), Errno> {
    let sc = ucom_softc_of(self_);
    let tp = sc.tty();

    if let Some(p) = sc.sc_bulkin_pipe.take() {
        // SAFETY: the pipe was opened by `ucom_do_open` and nothing uses it once it is off
        // the softc.
        unsafe { usbd_close_pipe(NonNull::from(p)) };
    }
    if let Some(p) = sc.sc_bulkout_pipe.take() {
        // SAFETY: as above.
        unsafe { usbd_close_pipe(NonNull::from(p)) };
    }
    ucom_free_xfers(sc);

    let s = splusb();
    let refcnt = sc.sc_refcnt.get() - 1;
    sc.sc_refcnt.set(refcnt);
    if refcnt >= 0 {
        // Wake up anyone waiting
        if let Some(tp) = tp {
            tp.t_state_clr(TS_CARR_ON);
            tp.set_t_cflag(tp.t_cflag() & !(CLOCAL | MDMBUF));
            ttyflush(tp, FREAD | FWRITE);
        }
        usb_detach_wait(&sc.sc_dev);
    }
    splx(s);

    // locate the major number
    let maj = ucom_major();

    // Nuke the vnodes for any open instances.
    let mn = self_.dv_unit.get() as u32;
    vdevgone(maj, mn, mn, VCHR);
    vdevgone(maj, mn | UCOMCUA_MASK, mn | UCOMCUA_MASK, VCHR);

    // Detach and free the tty.
    if let Some(tp) = tp {
        let _ = (linesw(tp).l_close)(tp, FNONBLOCK, curproc());
        let s = spltty();
        tp.t_state_clr(TS_BUSY | TS_FLUSH);
        let _ = ttyclose(tp);
        splx(s);
        sc.sc_tty.set(ptr::null());
        // SAFETY: the vnodes are gone, the pipes closed and the port is detached, so nothing
        // uses the tty any more.
        unsafe { ttyfree(NonNull::from(tp)) };
    }

    Ok(())
}

/// The xfers `ucom_detach` and `ucom_cleanup` free: the input one (with its buffer) and the
/// output one, which belong to the HID device when the port is HID-based.
fn ucom_free_xfers(sc: &UcomSoftc) {
    if let Some(x) = sc.sc_ixfer.take()
        && sc.sc_bulkin_no.get() != -1
    {
        usbd_free_buffer(x);
        sc.sc_ibuf.set(ptr::null_mut());
        // SAFETY: allocated by `ucom_do_open` (a bulk port); its pipe is closed or aborted,
        // so the xfer is not queued, and nothing uses it once it is off the softc.
        unsafe { usbd_free_xfer(NonNull::from(x)) };
    }
    if let Some(x) = sc.sc_oxfer.take() {
        usbd_free_buffer(x);
        sc.sc_obuf.set(ptr::null_mut());
        if sc.sc_bulkin_no.get() != -1 {
            // SAFETY: as above, for the output xfer.
            unsafe { usbd_free_xfer(NonNull::from(x)) };
        }
    }
}

/// `ucom_shutdown`.
pub fn ucom_shutdown(sc: &'static UcomSoftc) {
    let Some(tp) = sc.tty() else {
        return;
    };

    // Hang up if necessary.  Wait a bit, so the other side has time to notice even if we
    // immediately open the port again.
    if tp.t_cflag() & HUPCL != 0 {
        ucom_dtr(sc, 0);
        ucom_rts(sc, 0);
        let _ = tsleep_nsec(ptr::from_ref(sc), TTIPRI, TTCLOS, sec_to_nsec(1));
    }
}

/// `ucomopen`.
pub fn ucomopen(dev: Dev, flag: i32, mode: i32, p: &Proc) -> Result<(), Errno> {
    let unit = ucomunit(dev);

    let Some(sc) = ucom_unit(unit) else {
        return Err(Errno::ENXIO);
    };

    sc.sc_error.set(0);

    if sc.dying() {
        return Err(Errno::EIO);
    }

    if sc.sc_dev.dv_flags.get() & DVF_ACTIVE == 0 {
        return Err(Errno::ENXIO);
    }

    sc.sc_refcnt.set(sc.sc_refcnt.get() + 1);
    let error = ucom_do_open(sc, dev, flag, mode, p);
    sc.sc_refcnt.set(sc.sc_refcnt.get() - 1);
    if sc.sc_refcnt.get() < 0 {
        usb_detach_wakeup(&sc.sc_dev);
    }

    error
}

/// The labels `fail_4` to `fail_1` of `ucom_do_open`: undoes what the open made, from the
/// output buffer's allocation back to the bulk-in pipe (`from` is the label's number).
fn ucom_open_unwind(sc: &UcomSoftc, from: u8) {
    if from >= 4 {
        // fail_4:
        if let Some(x) = sc.sc_oxfer.take()
            && sc.sc_bulkin_no.get() != -1
        {
            // SAFETY: allocated by `ucom_do_open`, not queued, off the softc.
            unsafe { usbd_free_xfer(NonNull::from(x)) };
        }
    }
    if from >= 3 {
        // fail_3:
        if let Some(x) = sc.sc_ixfer.take()
            && sc.sc_bulkin_no.get() != -1
        {
            // SAFETY: as above, for the input xfer.
            unsafe { usbd_free_xfer(NonNull::from(x)) };
        }
    }
    if from >= 2 {
        // fail_2:
        if let Some(p) = sc.sc_bulkout_pipe.take() {
            // SAFETY: opened by `ucom_do_open`, nothing uses it once it is off the softc.
            unsafe { usbd_close_pipe(NonNull::from(p)) };
        }
    }
    if from >= 1 {
        // fail_1:
        if let Some(p) = sc.sc_bulkin_pipe.take() {
            // SAFETY: as above.
            unsafe { usbd_close_pipe(NonNull::from(p)) };
        }
    }
}

/// `ucom_do_open`.
pub fn ucom_do_open(
    sc: &'static UcomSoftc,
    dev: Dev,
    flag: i32,
    _mode: i32,
    p: &Proc,
) -> Result<(), Errno> {
    // open the pipes if this is the first open
    ucom_lock(sc);
    let s = splusb();
    if sc.sc_open.get() == 0 {
        let udev = sc.uparent();

        if sc.sc_bulkin_no.get() != -1 {
            let Some(iface) = sc.sc_iface.get() else {
                splx(s);
                ucom_unlock(sc);
                return Err(Errno::EIO);
            };

            // Open the bulk pipes
            match usbd_open_pipe(iface, sc.sc_bulkin_no.get() as u8, 0) {
                Ok(pipe) => sc.sc_bulkin_pipe.set(Some(pipe)),
                Err(_) => {
                    // fail_0:
                    splx(s);
                    ucom_unlock(sc);
                    return Err(Errno::EIO);
                }
            }
            match usbd_open_pipe(iface, sc.sc_bulkout_no.get() as u8, USBD_EXCLUSIVE_USE) {
                Ok(pipe) => sc.sc_bulkout_pipe.set(Some(pipe)),
                Err(_) => {
                    ucom_open_unwind(sc, 1);
                    splx(s);
                    ucom_unlock(sc);
                    return Err(Errno::EIO);
                }
            }

            // Allocate a request and an input buffer and start reading.
            let ixfer = usbd_alloc_xfer(udev);
            sc.sc_ixfer.set(ixfer);
            let Some(ixfer) = ixfer else {
                ucom_open_unwind(sc, 2);
                splx(s);
                ucom_unlock(sc);
                return Err(Errno::ENOMEM);
            };

            match usbd_alloc_buffer(ixfer, sc.sc_ibufsizepad.get()) {
                Some(b) => sc.sc_ibuf.set(b.as_ptr()),
                None => {
                    ucom_open_unwind(sc, 2);
                    splx(s);
                    ucom_unlock(sc);
                    return Err(Errno::ENOMEM);
                }
            }

            let oxfer = usbd_alloc_xfer(udev);
            sc.sc_oxfer.set(oxfer);
            if oxfer.is_none() {
                ucom_open_unwind(sc, 3);
                splx(s);
                ucom_unlock(sc);
                return Err(Errno::ENOMEM);
            }
        } else {
            // input/output pipes and xfers already allocated as is the input buffer.
            let Some(uhidev) = sc.sc_uhidev.get() else {
                splx(s);
                ucom_unlock(sc);
                return Err(Errno::EIO);
            };
            sc.sc_ipipe.set(uhidev.sc_ipipe.get());
            sc.sc_ixfer.set(uhidev.sc_ixfer.get());
            sc.sc_opipe.set(uhidev.sc_opipe.get());
            sc.sc_oxfer.set(uhidev.sc_oxfer.get());
        }

        let obuf = sc
            .sc_oxfer
            .get()
            .and_then(|x| usbd_alloc_buffer(x, sc.sc_obufsize.get() + sc.sc_opkthdrlen.get()));
        match obuf {
            Some(b) => sc.sc_obuf.set(b.as_ptr()),
            None => {
                ucom_open_unwind(sc, 4);
                splx(s);
                ucom_unlock(sc);
                return Err(Errno::ENOMEM);
            }
        }

        if let Some(open) = sc.methods().ucom_open
            && let Err(error) = open(sc.sc_parent.get(), sc.sc_portno.get())
        {
            ucom_cleanup(sc);
            splx(s);
            ucom_unlock(sc);
            return Err(error);
        }

        ucom_status_change(sc);

        let _ = ucomstartread(sc);
        sc.sc_open.set(1);
    }
    splx(s);
    let s = spltty();
    ucom_unlock(sc);
    let tp = sc.tty();
    splx(s);
    let Some(tp) = tp else {
        return Err(Errno::ENXIO);
    };

    tp.t_dev.set(dev);
    let s = if !tp.t_state_isset(TS_ISOPEN) {
        tp.t_state_set(TS_WOPEN);
        ttychars(tp);

        // Initialize the termios status to the defaults.  Add in the sticky bits from
        // TIOCSFLAGS.
        let mut t = Termios::zeroed();
        t.c_ispeed = 0;
        t.c_ospeed = TTYDEF_SPEED as i32;
        t.c_cflag = TTYDEF_CFLAG;
        if sc.sc_swflags.get() & TIOCFLAG_CLOCAL != 0 {
            t.c_cflag |= CLOCAL;
        }
        if sc.sc_swflags.get() & TIOCFLAG_CRTSCTS != 0 {
            t.c_cflag |= CRTSCTS;
        }
        if sc.sc_swflags.get() & TIOCFLAG_MDMBUF != 0 {
            t.c_cflag |= MDMBUF;
        }

        // Make sure ucomparam() will do something.
        tp.set_t_ospeed(0);
        let _ = ucomparam(tp, &t);
        tp.set_t_iflag(TTYDEF_IFLAG);
        tp.set_t_oflag(TTYDEF_OFLAG);
        tp.set_t_lflag(TTYDEF_LFLAG);

        let s = spltty();
        ttsetwater(tp);

        // Turn on DTR.  We must always do this, even if carrier is not present, because
        // otherwise we'd have to use TIOCSDTR immediately after setting CLOCAL, which
        // applications do not expect.  We always assert DTR while the device is open unless
        // explicitly requested to deassert it.
        ucom_dtr(sc, 1);
        // When not using CRTSCTS, RTS follows DTR.
        if t.c_cflag & CRTSCTS == 0 {
            ucom_rts(sc, 1);
        }

        // XXX CLR(sc->sc_rx_flags, RX_ANY_BLOCK);
        ucom_hwiflow(sc);

        if sc.sc_swflags.get() & TIOCFLAG_SOFTCAR != 0
            || ucomcua(dev) != 0
            || sc.sc_msr.get() & UMSR_DCD != 0
            || tp.t_cflag() & MDMBUF != 0
        {
            tp.t_state_set(TS_CARR_ON);
        } else {
            tp.t_state_clr(TS_CARR_ON);
        }
        s
    } else if tp.t_state_isset(TS_XCLUDE) && suser(p).is_err() {
        return Err(Errno::EBUSY);
    } else {
        spltty()
    };

    if ucomcua(dev) != 0 {
        if tp.t_state_isset(TS_ISOPEN) {
            // Someone is already dialed in
            splx(s);
            return Err(Errno::EBUSY);
        }
        sc.sc_cua.set(1);
    } else {
        // tty (not cua) device, wait for carrier
        if flag & O_NONBLOCK != 0 {
            if sc.sc_cua.get() != 0 {
                splx(s);
                return Err(Errno::EBUSY);
            }
        } else {
            while sc.sc_cua.get() != 0
                || (tp.t_cflag() & CLOCAL == 0 && !tp.t_state_isset(TS_CARR_ON))
            {
                tp.t_state_set(TS_WOPEN);
                let error = ttysleep(
                    tp,
                    ptr::from_ref(&tp.t_rawq).cast(),
                    TTIPRI | PCATCH,
                    TTOPEN,
                );

                if sc.dying() {
                    splx(s);
                    return Err(Errno::EIO);
                }

                // If TS_WOPEN has been reset, that means the cua device has been closed.
                // We don't want to fail in that case, so just go around again.
                if let Err(e) = error
                    && tp.t_state_isset(TS_WOPEN)
                {
                    tp.t_state_clr(TS_WOPEN);
                    splx(s);
                    return ucom_open_bad(sc, e);
                }
            }
        }
    }
    splx(s);

    match (linesw(tp).l_open)(dev, tp, p) {
        Ok(()) => Ok(()),
        Err(e) => ucom_open_bad(sc, e),
    }
}

/// `bad:` of `ucom_do_open`.
fn ucom_open_bad(sc: &'static UcomSoftc, error: Errno) -> Result<(), Errno> {
    ucom_lock(sc);
    ucom_cleanup(sc);
    ucom_unlock(sc);

    Err(error)
}

/// `ucomclose`.
pub fn ucomclose(dev: Dev, flag: i32, mode: i32, p: Option<&Proc>) -> Result<(), Errno> {
    let Some(sc) = ucom_unit(ucomunit(dev)) else {
        return Err(Errno::EIO);
    };

    if sc.dying() {
        return Err(Errno::EIO);
    }

    sc.sc_refcnt.set(sc.sc_refcnt.get() + 1);
    let error = ucom_do_close(sc, flag, mode, p);
    sc.sc_refcnt.set(sc.sc_refcnt.get() - 1);
    if sc.sc_refcnt.get() < 0 {
        usb_detach_wakeup(&sc.sc_dev);
    }

    error
}

/// `ucom_do_close`.
pub fn ucom_do_close(
    sc: &'static UcomSoftc,
    flag: i32,
    _mode: i32,
    p: Option<&Proc>,
) -> Result<(), Errno> {
    let Some(tp) = sc.tty() else {
        return Ok(());
    };

    if !tp.t_state_isset(TS_ISOPEN) {
        return Ok(());
    }

    ucom_lock(sc);

    let _ = (linesw(tp).l_close)(tp, flag, p);
    let s = spltty();
    tp.t_state_clr(TS_BUSY | TS_FLUSH);
    sc.sc_cua.set(0);
    let _ = ttyclose(tp);
    splx(s);
    ucom_cleanup(sc);

    if let Some(close) = sc.methods().ucom_close {
        close(sc.sc_parent.get(), sc.sc_portno.get());
    }

    ucom_unlock(sc);

    Ok(())
}

/// `ucomread`.
pub fn ucomread(dev: Dev, uio: &mut Uio<'_>, flag: i32) -> Result<(), Errno> {
    let Some(sc) = ucom_unit(ucomunit(dev)) else {
        return Err(Errno::EIO);
    };

    if sc.dying() {
        return Err(Errno::EIO);
    }

    // `sc_error` only ever holds `EIO` (`ucomreadcb`).
    if sc.sc_error.get() != 0 {
        return Err(Errno::EIO);
    }

    sc.sc_refcnt.set(sc.sc_refcnt.get() + 1);
    let error = match sc.tty() {
        Some(tp) => (linesw(tp).l_read)(tp, uio, flag),
        None => Err(Errno::EIO),
    };
    sc.sc_refcnt.set(sc.sc_refcnt.get() - 1);
    if sc.sc_refcnt.get() < 0 {
        usb_detach_wakeup(&sc.sc_dev);
    }
    error
}

/// `ucomwrite`.
pub fn ucomwrite(dev: Dev, uio: &mut Uio<'_>, flag: i32) -> Result<(), Errno> {
    let Some(sc) = ucom_unit(ucomunit(dev)) else {
        return Err(Errno::EIO);
    };

    if sc.dying() {
        return Err(Errno::EIO);
    }

    sc.sc_refcnt.set(sc.sc_refcnt.get() + 1);
    let error = match sc.tty() {
        Some(tp) => (linesw(tp).l_write)(tp, uio, flag),
        None => Err(Errno::EIO),
    };
    sc.sc_refcnt.set(sc.sc_refcnt.get() - 1);
    if sc.sc_refcnt.get() < 0 {
        usb_detach_wakeup(&sc.sc_dev);
    }
    error
}

/// `ucomtty`: the tty even if the device is dying, in order to properly close it in the detach
/// routine.
pub fn ucomtty(dev: Dev) -> Option<&'static Tty> {
    ucom_unit(ucomunit(dev))?.tty()
}

/// `ucomioctl`.
pub fn ucomioctl(dev: Dev, cmd: u64, data: &mut [u8], flag: i32, p: &Proc) -> Result<(), Errno> {
    let Some(sc) = ucom_unit(ucomunit(dev)) else {
        return Err(Errno::EIO);
    };

    if sc.dying() {
        return Err(Errno::EIO);
    }

    sc.sc_refcnt.set(sc.sc_refcnt.get() + 1);
    let error = ucom_do_ioctl(sc, cmd, data, flag, p);
    sc.sc_refcnt.set(sc.sc_refcnt.get() - 1);
    if sc.sc_refcnt.get() < 0 {
        usb_detach_wakeup(&sc.sc_dev);
    }
    error
}

/// `ucom_do_ioctl`.
pub fn ucom_do_ioctl(
    sc: &'static UcomSoftc,
    cmd: u64,
    data: &mut [u8],
    flag: i32,
    p: &Proc,
) -> Result<(), Errno> {
    let Some(tp) = sc.tty() else {
        return Err(Errno::EIO);
    };

    if (linesw(tp).l_ioctl)(tp, cmd, data, flag, p)? {
        return Ok(());
    }

    if ttioctl(tp, cmd, data, flag, p)? {
        return Ok(());
    }

    if let Some(ioctl) = sc.methods().ucom_ioctl {
        match ioctl(sc.sc_parent.get(), sc.sc_portno.get(), cmd, data, flag, p) {
            Err(Errno::ENOTTY) => {}
            error => return error,
        }
    }

    let mut error = Ok(());

    let s = spltty();

    match cmd {
        TIOCSBRK => ucom_break(sc, 1),
        TIOCCBRK => ucom_break(sc, 0),
        TIOCSDTR => ucom_dtr(sc, 1),
        TIOCCDTR => ucom_dtr(sc, 0),
        TIOCGFLAGS => ioctl_ret(data, &sc.sc_swflags.get()),
        TIOCSFLAGS => {
            error = suser(p);
            if error.is_ok() {
                sc.sc_swflags.set(ioctl_arg::<i32>(data));
            }
        }
        TIOCMSET | TIOCMBIS | TIOCMBIC => tiocm_to_ucom(sc, cmd, ioctl_arg::<i32>(data)),
        TIOCMGET => ioctl_ret(data, &ucom_to_tiocm(sc)),
        _ => error = Err(Errno::ENOTTY),
    }
    splx(s);

    error
}

/// `tiocm_to_ucom`.
pub fn tiocm_to_ucom(sc: &'static UcomSoftc, how: u64, ttybits: i32) {
    let mut combits = 0;
    if ttybits & TIOCM_DTR != 0 {
        combits |= UMCR_DTR;
    }
    if ttybits & TIOCM_RTS != 0 {
        combits |= UMCR_RTS;
    }

    match how {
        TIOCMBIC => sc.sc_mcr.set(sc.sc_mcr.get() & !combits),
        TIOCMBIS => sc.sc_mcr.set(sc.sc_mcr.get() | combits),
        TIOCMSET => {
            sc.sc_mcr.set(sc.sc_mcr.get() & !(UMCR_DTR | UMCR_RTS));
            sc.sc_mcr.set(sc.sc_mcr.get() | combits);
        }
        _ => {}
    }

    if how == TIOCMSET || combits & UMCR_DTR != 0 {
        ucom_dtr(sc, i32::from(sc.sc_mcr.get() & UMCR_DTR != 0));
    }
    if how == TIOCMSET || combits & UMCR_RTS != 0 {
        ucom_rts(sc, i32::from(sc.sc_mcr.get() & UMCR_RTS != 0));
    }
}

/// `ucom_to_tiocm`.
pub fn ucom_to_tiocm(sc: &UcomSoftc) -> i32 {
    let mut ttybits = 0;

    let combits = sc.sc_mcr.get();
    if combits & UMCR_DTR != 0 {
        ttybits |= TIOCM_DTR;
    }
    if combits & UMCR_RTS != 0 {
        ttybits |= TIOCM_RTS;
    }

    let combits = sc.sc_msr.get();
    if combits & UMSR_DCD != 0 {
        ttybits |= TIOCM_CD;
    }
    if combits & UMSR_CTS != 0 {
        ttybits |= TIOCM_CTS;
    }
    if combits & UMSR_DSR != 0 {
        ttybits |= TIOCM_DSR;
    }
    if combits & (UMSR_RI | UMSR_TERI) != 0 {
        ttybits |= TIOCM_RI;
    }

    // XXX if (sc->sc_ier != 0) SET(ttybits, TIOCM_LE);

    ttybits
}

/// `ucom_break`.
pub fn ucom_break(sc: &UcomSoftc, onoff: i32) {
    if let Some(set) = sc.methods().ucom_set {
        set(
            sc.sc_parent.get(),
            sc.sc_portno.get(),
            UCOM_SET_BREAK,
            onoff,
        );
    }
}

/// `ucom_dtr`.
pub fn ucom_dtr(sc: &UcomSoftc, onoff: i32) {
    if let Some(set) = sc.methods().ucom_set {
        set(sc.sc_parent.get(), sc.sc_portno.get(), UCOM_SET_DTR, onoff);
    }
}

/// `ucom_rts`.
pub fn ucom_rts(sc: &UcomSoftc, onoff: i32) {
    if let Some(set) = sc.methods().ucom_set {
        set(sc.sc_parent.get(), sc.sc_portno.get(), UCOM_SET_RTS, onoff);
    }
}

/// `ucom_status_change`.
pub fn ucom_status_change(sc: &UcomSoftc) {
    let Some(tp) = sc.tty() else {
        return;
    };

    match sc.methods().ucom_get_status {
        Some(get_status) => {
            let old_msr = sc.sc_msr.get();
            let mut lsr = sc.sc_lsr.get();
            let mut msr = old_msr;
            get_status(sc.sc_parent.get(), sc.sc_portno.get(), &mut lsr, &mut msr);
            sc.sc_lsr.set(lsr);
            sc.sc_msr.set(msr);

            ttytstamp(
                tp,
                i32::from(old_msr & UMSR_CTS),
                i32::from(msr & UMSR_CTS),
                i32::from(old_msr & UMSR_DCD),
                i32::from(msr & UMSR_DCD),
            );

            if (msr ^ old_msr) & UMSR_DCD != 0 {
                let _ = (linesw(tp).l_modem)(tp, i32::from(msr & UMSR_DCD != 0));
            }
        }
        None => {
            sc.sc_lsr.set(0);
            sc.sc_msr.set(0);
        }
    }
}

/// `ucomparam`.
pub fn ucomparam(tp: &Tty, t: &Termios) -> Result<(), Errno> {
    let Some(sc) = ucom_unit(ucomunit(tp.t_dev.get())) else {
        return Err(Errno::EIO);
    };
    if sc.dying() {
        return Err(Errno::EIO);
    }
    let mut t = *t;

    // Check requested parameters.
    if t.c_ispeed != 0 && t.c_ispeed != t.c_ospeed {
        return Err(Errno::EINVAL);
    }

    // For the console, always force CLOCAL and !HUPCL, so that the port is always active.
    if sc.sc_swflags.get() & TIOCFLAG_SOFTCAR != 0 {
        t.c_cflag |= CLOCAL;
        t.c_cflag &= !HUPCL;
    }

    // If there were no changes, don't do anything.  This avoids dropping input and improves
    // performance when all we did was frob things like VMIN and VTIME.
    if tp.t_ospeed() == t.c_ospeed && tp.t_cflag() == t.c_cflag {
        return Ok(());
    }

    // XXX lcr = ISSET(sc->sc_lcr, LCR_SBREAK) | cflag2lcr(t->c_cflag);

    // And copy to tty.
    tp.set_t_ispeed(0);
    tp.set_t_ospeed(t.c_ospeed);
    tp.set_t_cflag(t.c_cflag);

    // When not using CRTSCTS, RTS follows DTR. This assumes that the ucom_param() call will
    // enable these signals for real.
    if t.c_cflag & CRTSCTS == 0 {
        sc.sc_mcr.set(UMCR_DTR | UMCR_RTS);
    } else {
        sc.sc_mcr.set(UMCR_DTR);
    }

    if let Some(param) = sc.methods().ucom_param {
        param(sc.sc_parent.get(), sc.sc_portno.get(), &t)?;
    }

    // XXX worry about CHWFLOW

    // Update the tty layer's idea of the carrier bit, in case we changed CLOCAL or MDMBUF.
    // We don't hang up here; we only do that by explicit request.
    let _ = (linesw(tp).l_modem)(tp, 1 /* XXX carrier */);

    // XXX what if the hardware is not open
    // if (!ISSET(t->c_cflag, CHWFLOW)) { if (sc->sc_tx_stopped) { sc->sc_tx_stopped = 0;
    // ucomstart(tp); } }

    Ok(())
}

/// `ucom_hwiflow`: (un)block input via hw flowcontrol; the C's body is `#if 0`.
pub fn ucom_hwiflow(_sc: &UcomSoftc) {}

/// `ucomstart`.
pub fn ucomstart(tp: &Tty) {
    let Some(sc) = ucom_unit(ucomunit(tp.t_dev.get())) else {
        return;
    };
    if sc.dying() {
        return;
    }

    let s = spltty();
    'out: {
        if tp.t_state_isset(TS_BUSY | TS_TIMEOUT | TS_TTSTOP) {
            break 'out;
        }
        if sc.sc_tx_stopped.get() != 0 {
            break 'out;
        }

        ttwakeupwr(tp);
        if tp.t_outq.c_cc.get() == 0 {
            break 'out;
        }

        // Grab the first contiguous region of buffer space.
        let mut cnt = ndqb(&tp.t_outq, 0);
        if cnt <= 0 {
            break 'out;
        }
        let cs = tp.t_outq.c_cs.get();
        if cs.is_null() {
            break 'out;
        }

        tp.t_state_set(TS_BUSY);

        if cnt as u32 > sc.sc_obufsize.get() {
            cnt = sc.sc_obufsize.get() as i32;
        }
        let obuf = sc.sc_obuf.get();
        let Some(oxfer) = sc.sc_oxfer.get() else {
            // DIAGNOSTIC's "ucomstart: null oxfer"
            tp.t_state_clr(TS_BUSY);
            break 'out;
        };
        if obuf.is_null() {
            tp.t_state_clr(TS_BUSY);
            break 'out;
        }
        // SAFETY: `c_cs` is the `c_cn`-byte ring of the output queue and `ndqb` counted `cnt`
        // contiguous bytes from `c_cf` within it, which the output queue keeps (`TS_BUSY`
        // stops anyone else flushing) until `ucomwritecb`.
        let data = unsafe {
            slice::from_raw_parts(cs.add(tp.t_outq.c_cf.get()).cast_const(), cnt as usize)
        };
        // SAFETY: `sc_obuf` is the output xfer's DMA buffer of `sc_obufsize + sc_opkthdrlen`
        // bytes (`ucom_do_open`), idle while `TS_BUSY` is clear.
        let to = unsafe {
            slice::from_raw_parts_mut(
                obuf,
                (sc.sc_obufsize.get() + sc.sc_opkthdrlen.get()) as usize,
            )
        };
        let cnt = match sc.methods().ucom_write {
            Some(write) => write(sc.sc_parent.get(), sc.sc_portno.get(), to, data),
            None => {
                to[..data.len()].copy_from_slice(data);
                data.len()
            }
        };

        let Some(pipe) = sc.sc_bulkout_pipe.get().or_else(|| sc.sc_opipe.get()) else {
            tp.t_state_clr(TS_BUSY);
            break 'out;
        };
        // SAFETY: `obuf` is the xfer's own DMA buffer (`USBD_NO_COPY`), `cnt` bytes of it,
        // valid until the transfer completes.
        unsafe {
            usbd_setup_xfer(
                oxfer,
                pipe,
                ucom_priv(sc),
                obuf,
                cnt as u32,
                USBD_NO_COPY,
                USBD_NO_TIMEOUT,
                Some(ucomwritecb),
            );
        }
        // What can we do on error?
        let err = usbd_transfer(oxfer);
        #[cfg(feature = "diagnostic")]
        if err != USBD_IN_PROGRESS {
            printf(format_args!("ucomstart: err={}\n", usbd_errstr(err)));
        }
        let _ = err;
    }
    splx(s);
}

/// `ucomstop`: the C's body is `#if 0`.
pub fn ucomstop(_tp: &Tty, _flag: i32) -> Result<(), Errno> {
    Ok(())
}

/// `ucomwritecb`.
pub fn ucomwritecb(xfer: &'static UsbdXfer, p: *mut c_void, status: UsbdStatus) {
    let sc = ucom_priv_softc(p);
    let Some(tp) = sc.tty() else {
        return;
    };

    let error = status == USBD_CANCELLED || sc.dying() || {
        if let Some(pipe) = sc.sc_bulkin_pipe.get()
            && status != USBD_NORMAL_COMPLETION
        {
            let _ = usbd_clear_endpoint_stall_async(pipe);
            // XXX we should restart after some delay.
            true
        } else {
            false
        }
    };
    if error {
        let s = spltty();
        tp.t_state_clr(TS_BUSY);
        splx(s);
        return;
    }

    let mut cc = 0;
    usbd_get_xfer_status(xfer, None, None, Some(&mut cc), None);

    // convert from USB bytes to tty bytes
    let cc = cc.wrapping_sub(sc.sc_opkthdrlen.get());

    let s = spltty();
    tp.t_state_clr(TS_BUSY);
    if tp.t_state_isset(TS_FLUSH) {
        tp.t_state_clr(TS_FLUSH);
    } else {
        ndflush(&tp.t_outq, cc as i32);
    }
    let _ = (linesw(tp).l_start)(tp);
    splx(s);
}

/// `ucomstartread`.
pub fn ucomstartread(sc: &'static UcomSoftc) -> UsbdStatus {
    let Some(ixfer) = sc.sc_ixfer.get() else {
        // DIAGNOSTIC's "ucomstartread: null ixfer"
        return USBD_INVAL;
    };

    if let Some(pipe) = sc.sc_bulkin_pipe.get() {
        // SAFETY: `sc_ibuf` is the xfer's own DMA buffer of `sc_ibufsizepad` bytes, of which
        // `sc_ibufsize` are asked for, valid until the transfer completes (`USBD_NO_COPY`).
        unsafe {
            usbd_setup_xfer(
                ixfer,
                pipe,
                ucom_priv(sc),
                sc.sc_ibuf.get(),
                sc.sc_ibufsize.get(),
                USBD_SHORT_XFER_OK | USBD_NO_COPY,
                USBD_NO_TIMEOUT,
                Some(ucomreadcb),
            );
        }
        let err = usbd_transfer(ixfer);
        if err != USBD_IN_PROGRESS {
            return err;
        }
    }

    USBD_NORMAL_COMPLETION
}

/// `ucomreadcb`.
pub fn ucomreadcb(xfer: &'static UsbdXfer, p: *mut c_void, status: UsbdStatus) {
    let sc = ucom_priv_softc(p);
    let Some(tp) = sc.tty() else {
        return;
    };
    let rint = linesw(tp).l_rint;

    if status == USBD_CANCELLED || status == USBD_IOERROR || sc.dying() {
        // Send something to wake upper layer
        sc.sc_error.set(Errno::EIO as i32);
        let s = spltty();
        let _ = rint(i32::from(b'\n'), tp);
        ttwakeup(tp);
        splx(s);
        return;
    }

    if status != USBD_NORMAL_COMPLETION
        && let Some(pipe) = sc.sc_bulkin_pipe.get()
    {
        let _ = usbd_clear_endpoint_stall_async(pipe);
        // XXX we should restart after some delay.
        return;
    }

    let mut cp: *mut u8 = ptr::null_mut();
    let mut cc = 0;
    usbd_get_xfer_status(xfer, None, Some(&mut cp), Some(&mut cc), None);
    let mut data: &[u8] = if cp.is_null() {
        &[]
    } else {
        // SAFETY: the xfer's buffer is `sc_ibuf`, the DMA buffer of `sc_ibufsizepad` bytes
        // (`ucom_do_open`) holding the `cc` bytes just received (at most `sc_ibufsize`); it
        // is not touched until the next read is started below.
        unsafe { slice::from_raw_parts(cp.cast_const(), cc as usize) }
    };
    if let Some(read) = sc.methods().ucom_read {
        read(sc.sc_parent.get(), sc.sc_portno.get(), &mut data);
    }

    let s = spltty();
    // Give characters to tty layer.
    for (i, &c) in data.iter().enumerate() {
        if rint(i32::from(c), tp) == -1 {
            // XXX what should we do?
            printf(format_args!(
                "{}: lost {} chars\n",
                sc.sc_dev.xname(),
                data.len() - i - 1
            ));
            break;
        }
    }
    splx(s);

    let err = ucomstartread(sc);
    if err.is_err() {
        printf(format_args!("{}: read start failed\n", sc.sc_dev.xname()));
        // XXX what should we dow now?
    }
}

/// `ucom_cleanup`.
pub fn ucom_cleanup(sc: &'static UcomSoftc) {
    sc.sc_open.set(0);

    ucom_shutdown(sc);
    if let Some(p) = sc.sc_bulkin_pipe.take() {
        // SAFETY: opened by `ucom_do_open`, nothing uses it once it is off the softc.
        unsafe { usbd_close_pipe(NonNull::from(p)) };
    }
    if let Some(p) = sc.sc_bulkout_pipe.take() {
        // SAFETY: as above.
        unsafe { usbd_close_pipe(NonNull::from(p)) };
    }
    ucom_free_xfers(sc);
}

/// The size of [`UcomNames`]: room for sixteen `name:usb000.000.00000.000,` entries of the
/// C's 64-byte `name`.
const UCOMNAMES_LEN: usize = 16 * 64;

/// What `sysctl_ucominit` returns: the `hw.ucomnames` string.
pub struct UcomNames {
    /// The string, NUL-terminated.
    buf: [u8; UCOMNAMES_LEN],
}

impl UcomNames {
    /// The names, `unit:usbB.R.SSSSS.I` separated by commas, without the NUL.
    pub fn as_bytes(&self) -> &[u8] {
        let n = self
            .buf
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(self.buf.len());
        &self.buf[..n]
    }
}

/// `sysctl_ucominit`: the names of the ucom ports with their USB locations, for
/// `hw.ucomnames`.
pub fn sysctl_ucominit() -> UcomNames {
    let mut names = UcomNames {
        buf: [0; UCOMNAMES_LEN],
    };
    let mut len = 0;
    let ndevs = UCOM_CD.cd_ndevs.get().max(0) as u32;

    for unit in 0..ndevs {
        let Some(sc) = ucom_unit(unit) else {
            continue;
        };
        let Some(iface) = sc.sc_iface.get() else {
            continue;
        };
        let Some((bus, route, ifaceno)) = usbd_get_location(sc.sc_uparent.get(), iface) else {
            continue;
        };
        let mut name = [0u8; 64]; // dv_xname + ":usb000.000.00000.000,"
        let rslt = snprintf(
            &mut name,
            format_args!(
                "{}:usb{}.{}.{:05x}.{},",
                sc.sc_dev.xname(),
                bus,
                routerootport(route),
                routestring(route),
                ifaceno
            ),
        );
        if rslt < name.len() && len + rslt < UCOMNAMES_LEN {
            names.buf[len..len + rslt].copy_from_slice(&name[..rslt]);
            len += rslt;
        }
    }

    // Remove trailing ','.
    if len > 0 {
        names.buf[len - 1] = 0;
    }

    names
}

/// `ucomprint`.
pub fn ucomprint(aux: *mut c_void, pnp: Option<&[u8]>) -> i32 {
    // SAFETY: a driver's `config_found_sm` passes its `ucom_attach_args`.
    let uca = unsafe { &*aux.cast::<UcomAttachArgs>() };

    if let Some(pnp) = pnp {
        printf(format_args!("ucom at {}", Str(pnp)));
    }
    if uca.portno != UCOM_UNK_PORTNO {
        printf(format_args!(" portno {}", uca.portno));
    }
    UNCONF
}

/// `ucomsubmatch`: checks the `portno` locator before the driver's match.
pub fn ucomsubmatch(parent: Option<&Device>, match_: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: as in `ucomprint`.
    let uca = unsafe { &*aux.cast::<UcomAttachArgs>() };
    let cf = match_.cfdata();

    // `ucomcf_portno` is `cf_loc[UCOMBUSCF_PORTNO]`.
    let portno = cf
        .cf_loc
        .get(crate::dev::usb::ucomvar::UCOMBUSCF_PORTNO)
        .map_or(UCOM_UNK_PORTNO, |&l| l as i32);
    if uca.portno != UCOM_UNK_PORTNO && portno != UCOM_UNK_PORTNO && portno != uca.portno {
        return 0;
    }
    match cf.cf_attach.ca_match {
        Some(ca_match) => ca_match(parent, match_, aux),
        None => 0,
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minor_numbers_split_into_unit_and_call_out_bit() {
        assert_eq!(ucomunit(0), 0);
        assert_eq!(ucomcua(0), 0);
        assert_eq!(ucomunit(0x80 | 3), 3);
        assert_eq!(ucomcua(0x80 | 3), UCOMCUA_MASK);
        assert_eq!(ucomunit(0x7f), 0x7f);
    }

    #[test]
    fn route_string_fields() {
        let route = 0x0012_3405;
        assert_eq!(routerootport(route), 5);
        assert_eq!(routestring(route), 0x1234);
    }

    #[test]
    fn names_of_no_ucom_are_empty() {
        let names = sysctl_ucominit();
        assert_eq!(names.as_bytes(), b"");
    }
}
/* </TESTS> */
