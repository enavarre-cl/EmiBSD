/* $OpenBSD: wsmouse.c,v 1.76 2025/07/18 17:34:29 mvs Exp $ */
/* $NetBSD: wsmouse.c,v 1.35 2005/02/27 00:27:52 perry Exp $ */
/* <LICENSES> */
/*
 * Copyright (c) 1996, 1997 Christopher G. Demetriou.  All rights reserved.
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
 *      This product includes software developed by Christopher G. Demetriou
 *	for the NetBSD Project.
 * 4. The name of the author may not be used to endorse or promote products
 *    derived from this software without specific prior written permission
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR ``AS IS'' AND ANY EXPRESS OR
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES
 * OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED.
 * IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY DIRECT, INDIRECT,
 * INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT
 * NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
 * DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
 * THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
 * (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF
 * THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */

/*
 * Copyright (c) 1992, 1993
 *	The Regents of the University of California.  All rights reserved.
 *
 * This software was developed by the Computer Systems Engineering group
 * at Lawrence Berkeley Laboratory under DARPA contract BG 91-66 and
 * contributed to Berkeley.
 *
 * All advertising materials mentioning features or use of this software
 * must display the following acknowledgement:
 *	This product includes software developed by the University of
 *	California, Lawrence Berkeley Laboratory.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. Neither the name of the University nor the names of its contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE REGENTS AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE REGENTS OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 *	@(#)ms.c	8.1 (Berkeley) 6/11/93
 */

/*
 * Copyright (c) 2015, 2016 Ulf Brosziewski
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
//! `wsmouse(4)`: the wscons mouse driver (`wsmouse* at ums? mux 0`, `/dev/wsmouse*`). Turns
//! the input a mouse or touchpad driver reports into `wscons_event`s for the reader of the
//! mouse or of the mux it is in.
//!
//! Upstream: sys/dev/wscons/wsmouse.c @ 3ce1f3f79392
//!
//! A mouse driver attaches a `wsmouse` child ([`wsmouse_attach`]), which joins the mux its
//! `mux` locator names (`mux 0`, `/dev/wsmouse`). The driver reports its input with
//! [`wsmouse_buttons`], [`wsmouse_motion`], [`wsmouse_position`], [`wsmouse_touch`],
//! [`wsmouse_mtstate`], [`wsmouse_mtframe`] or [`wsmouse_set`], and ends each report with
//! [`wsmouse_input_sync`]: multitouch state becomes single-touch and pointer state (the
//! pointer-controlling slot of [`wsmouse_ptr_ctrl`]), a touchpad in compat mode goes through
//! `wstpad.rs`, and what changed becomes button, motion (scaled, inverted, swapped),
//! scroll and touch events, closed by a `WSCONS_EVENT_SYNC`, in the queue of whoever has
//! the mouse open (directly or through its mux). A full queue drops the frame and sets
//! `RESYNC`, so the next one reports the absolute position again. `WSMOUSEIO_GETPARAMS` and
//! `WSMOUSEIO_SETPARAMS` read and change the filters ([`wsmouse_get_params`],
//! [`wsmouse_set_params`]).
//!
//! In M13 no driver attaches a wsmouse (`ums(4)`, `pms(4)` and the others are M16); the
//! driver interface is proven by host tests over a fake mouse.
//!
//! ## Deviations
//! - The kernel configuration is GENERIC's: `NWSMUX > 0` (the mux paths are compiled);
//!   `WSMUX_DEBUG`'s `DPRINTF`s are not carried; `wsmouseread`'s `DIAGNOSTIC` check is
//!   behind the `diagnostic` feature.
//! - `struct wsmouse_softc` is `#[repr(C)]` with its `Wsevsrc` first and all-zero valid
//!   (`config_make_softc`). `sc_input`, which the driver's reports and the parameter ioctls
//!   change in place, is an `UnsafeCell` handed out by a guard ([`WsmouseSoftc::input`])
//!   that panics on a second live borrow (`docs/C_TO_RUST.md`); the C protects it with
//!   `spltty` and the kernel lock. [`wsmouse_get_hw`] returns such a guard too.
//! - The functions a driver calls take its `&Device`, as in C, and work on the input
//!   through the guard; their bodies are methods of `WsmouseInput` named without the
//!   `wsmouse_` prefix (`WsmouseInput::touch`), so that one function calling another
//!   (`wsmouse_set` calls `wsmouse_position`, `wsmouse_mt_convert` calls `wsmouse_touch`)
//!   does not take the guard twice. `wsmouse_mt_convert` takes the input.
//! - `wsmouse_mtframe` takes the points as a slice (`size` is its length).
//! - `wsmouse_mt_init`'s and `wsmouse_configure`'s -1 are `Err(EINVAL)`; `wsmouse_set_mode`
//!   returns `false` for the C's -1 (an unknown mode), which a driver's ioctl turns into its
//!   own -1 (`Ok(false)`, `ENOTTY`) as in C. The parameter functions return `Result`.
//! - `wsmouse_evq_put` writes the event with `ws_mtx` held (the C takes the mutex only to
//!   read `ws_get`): the slot is beyond `ws_put`, so no reader sees it either way, and
//!   `Wseventvar::q_write` asks for the mutex.
//! - The entry points answer `ENXIO` for a minor without a mouse where the C indexes
//!   `wsmouse_cd.cd_devs` unchecked (only `wsmouseopen` checks there).

use core::cell::{Cell, UnsafeCell};
use core::ffi::c_void;
use core::ops::{Deref, DerefMut};
use core::ptr::{self, NonNull};

use crate::dev::rnd::enqueue_randomness;
use crate::dev::wscons::wsconsio::{
    WSCONS_EVENT_SYNC, WSCONS_EVENT_TOUCH_RESET, WSCONS_EVENT_TOUCH_WIDTH, WSMOUSE_COMPAT,
    WSMOUSE_NATIVE, WSMOUSECFG_DECELERATION, WSMOUSECFG_DX_MAX, WSMOUSECFG_DX_SCALE,
    WSMOUSECFG_DY_MAX, WSMOUSECFG_DY_SCALE, WSMOUSECFG_LOG_EVENTS, WSMOUSECFG_LOG_INPUT,
    WSMOUSECFG_MAX, WSMOUSECFG_PRESSURE_HI, WSMOUSECFG_PRESSURE_LO, WSMOUSECFG_REVERSE_SCROLLING,
    WSMOUSECFG_SMOOTHING, WSMOUSECFG_STRONG_HYSTERESIS, WSMOUSECFG_SWAPXY, WSMOUSECFG_TRKMAXDIST,
    WSMOUSECFG_X_HYSTERESIS, WSMOUSECFG_X_INV, WSMOUSECFG_Y_HYSTERESIS, WSMOUSECFG_Y_INV,
    WSMOUSEIO_GETPARAMS, WSMOUSEIO_SETPARAMS, WSMUX_MOUSE, WsconsEvent, WsmouseParam,
    WsmouseParameters,
};
use crate::dev::wscons::wsevent::{wsevent_fini, wsevent_init, wsevent_kqfilter, wsevent_read};
use crate::dev::wscons::wseventvar::{WSEVENT_QSIZE, Wseventvar, wsevent_wakeup};
use crate::dev::wscons::wsmouseinput::*;
use crate::dev::wscons::wsmousevar::{
    Mtpoint, WSMOUSE_ABS_X, WSMOUSE_ABS_Y, WSMOUSE_CONTACTS, WSMOUSE_MT_ABS_X, WSMOUSE_MT_ABS_Y,
    WSMOUSE_MT_PRESSURE, WSMOUSE_MT_REL_X, WSMOUSE_MT_REL_Y, WSMOUSE_MT_SLOTS_MAX,
    WSMOUSE_PRESSURE, WSMOUSE_REL_X, WSMOUSE_REL_Y, WSMOUSE_TOUCH_WIDTH, WSMOUSEHW_LR_DOWN,
    WSMOUSEHW_MT_TRACKING, WsmouseAccessops, WsmousedevAttachArgs, Wsmousehw, Wsmouseval,
    wsmouse_is_mt_code, wsmousedevcf_mux,
};
use crate::dev::wscons::wsmux::{wsmux_attach_sc, wsmux_detach_sc, wsmux_getmux};
use crate::dev::wscons::wsmuxvar::{Wsevsrc, WsmuxSoftc, Wssrcops};
use crate::dev::wscons::wstpad::{
    wstpad_cleanup, wstpad_compat_convert, wstpad_configure, wstpad_get_param,
    wstpad_init_deceleration, wstpad_reset, wstpad_set_param,
};
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_sig::{sigio_getown, sigio_setown};
use crate::kern::kern_synch::{tsleep_nsec, wakeup};
use crate::kern::kern_tc::getnanotime;
use crate::kern::subr_prf::{panic, printf};
use crate::kern::vfs_subr::vdevgone;
use crate::machine::conf::{cdevsw, nchrdev};
use crate::machine::copy::{copyin, copyout};
use crate::machine::intr::{spltty, splx};
use crate::sys::conf::DevTypeOpen;
use crate::sys::device::{
    CfMatch, Cfattach, Cfdriver, DV_TTY, DVACT_DEACTIVATE, Device, Softc, UNCONF,
};
use crate::sys::errno::Errno;
use crate::sys::event::Knote;
use crate::sys::fcntl::{FREAD, FWRITE};
use crate::sys::filio::{FIOASYNC, FIOGETOWN, FIOSETOWN};
use crate::sys::ioctl::{ioctl_arg, ioctl_ret};
use crate::sys::malloc::{M_DEVBUF, M_WAITOK, M_ZERO};
use crate::sys::param::PZERO;
use crate::sys::proc::Proc;
use crate::sys::time::{Timespec, sec_to_nsec};
use crate::sys::ttycom::{TIOCGPGRP, TIOCSPGRP};
use crate::sys::types::{Dev, minor};
use crate::sys::uio::Uio;
use crate::sys::vnode::VCHR;

/// `NWSMOUSE`: `wsmouse* at ...` is configured (`needs-flag`; GENERIC has it, though no
/// parent attaches one before M16).
pub const NWSMOUSE: i32 = 1;

/// `struct wsmouse_softc`.
#[repr(C)]
pub struct WsmouseSoftc {
    /// `sc_base`.
    pub sc_base: Wsevsrc,
    /// `sc_accessops`.
    sc_accessops: Cell<Option<&'static WsmouseAccessops>>,
    /// `sc_accesscookie`.
    sc_accesscookie: Cell<*mut c_void>,
    /// `sc_input`. Protected by: `spltty` and the kernel lock (the driver's reports and the
    /// ioctls); reached only through [`WsmouseSoftc::input`].
    sc_input: UnsafeCell<WsmouseInput>,
    /// Whether an [`InputGuard`] of `sc_input` is alive (not in C).
    input_busy: Cell<bool>,
    /// `sc_refcnt`.
    sc_refcnt: Cell<i32>,
    /// `sc_dying`: device is being detached.
    sc_dying: Cell<u8>,
}

// SAFETY: `#[repr(C)]`, its `Wsevsrc` (whose first member is the device) first, and every
// member valid all-zero (`None`s, null pointers, integers, an input without slots).
unsafe impl Softc for WsmouseSoftc {}

/// The `&mut struct wsmouseinput` of a mouse ([`WsmouseSoftc::input`]).
pub struct InputGuard<'a> {
    sc: &'a WsmouseSoftc,
    input: &'a mut WsmouseInput,
}

impl WsmouseSoftc {
    /// `&sc->sc_input`: the caller is the driver's input path or an ioctl, at `spltty`
    /// under the kernel lock, and holds no other guard of this mouse.
    pub fn input(&self) -> InputGuard<'_> {
        if self.input_busy.replace(true) {
            panic(format_args!(
                "{}: wsmouse input borrowed twice",
                self.sc_base.me_dv.xname()
            ));
        }
        // SAFETY: every `&mut` to the input comes from here; the C's `spltty` and kernel
        // lock keep other CPUs and the driver's interrupt out, and `input_busy` refuses a
        // second one while this guard lives.
        let input = unsafe { &mut *self.sc_input.get() };
        InputGuard { sc: self, input }
    }

    /// `sc_accessops`, which every attached mouse has.
    fn accessops(&self) -> &'static WsmouseAccessops {
        match self.sc_accessops.get() {
            Some(ops) => ops,
            None => panic(format_args!("{}: no accessops", self.xname())),
        }
    }

    /// `sc_base.me_dv.dv_xname`.
    fn xname(&self) -> &str {
        self.sc_base.me_dv.xname()
    }

    /// `(*sc->sc_accessops->enable)(sc->sc_accesscookie)`.
    fn enable(&self) -> Result<(), Errno> {
        match (self.accessops().enable)(self.sc_accesscookie.get()) {
            0 => Ok(()),
            e => Err(Errno::from_raw(e).unwrap_or(Errno::EIO)),
        }
    }

    /// `(*sc->sc_accessops->disable)(sc->sc_accesscookie)`.
    fn disable(&self) {
        (self.accessops().disable)(self.sc_accesscookie.get());
    }
}

impl Deref for InputGuard<'_> {
    type Target = WsmouseInput;

    fn deref(&self) -> &WsmouseInput {
        self.input
    }
}

impl DerefMut for InputGuard<'_> {
    fn deref_mut(&mut self) -> &mut WsmouseInput {
        self.input
    }
}

impl Drop for InputGuard<'_> {
    fn drop(&mut self) {
        self.sc.input_busy.set(false);
    }
}

/// The `struct wsmousehw *` of [`wsmouse_get_hw`]: the hardware description inside the
/// input, while the guard lives.
pub struct HwGuard<'a>(InputGuard<'a>);

impl Deref for HwGuard<'_> {
    type Target = Wsmousehw;

    fn deref(&self) -> &Wsmousehw {
        &self.0.hw
    }
}

impl DerefMut for HwGuard<'_> {
    fn deref_mut(&mut self) -> &mut Wsmousehw {
        &mut self.0.hw
    }
}

/// `wsmouse_cd`.
pub static WSMOUSE_CD: Cfdriver = Cfdriver::new(b"wsmouse", DV_TTY, 0);

/// `wsmouse_ca`.
pub static WSMOUSE_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<WsmouseSoftc>(),
    ca_match: Some(wsmouse_match),
    ca_attach: wsmouse_attach,
    ca_detach: Some(wsmouse_detach),
    ca_activate: Some(wsmouse_activate),
};

/// `wsmouse_srcops`: a mouse as an event source of a mux.
pub static WSMOUSE_SRCOPS: Wssrcops = Wssrcops {
    type_: WSMUX_MOUSE,
    dopen: wsmouse_mux_open,
    dclose: wsmouse_mux_close,
    dioctl: wsmousedoioctl,
    ddispioctl: None,
    dsetdisplay: None,
};

/// `(struct wsmouse_softc *)dev`.
fn sc_of(dev: &Device) -> &WsmouseSoftc {
    // SAFETY: `dev` is a wsmouse device (made for `wsmouse_ca`): the callers are its own
    // entry points and its parent driver while it is attached.
    unsafe { dev.softc::<WsmouseSoftc>() }
}

/// `&((struct wsmouse_softc *)dev)->sc_input`, for `wstpad.c`'s tap timeout, whose
/// argument is the mouse's device.
pub(crate) fn wsmouse_input_of(dev: &Device) -> InputGuard<'_> {
    sc_of(dev).input()
}

/// `wsmouse_cd.cd_devs[unit]`.
fn wsmouse_sc(unit: i32) -> Option<&'static WsmouseSoftc> {
    let dev = WSMOUSE_CD.cd_dev(unit)?;
    // SAFETY: `wsmouse_cd`'s devices are made by `config_make_softc` for `wsmouse_ca`, and
    // live until detached (`vdevgone` first closes every open instance).
    Some(unsafe { &*dev.as_ptr().cast::<WsmouseSoftc>() })
}

/// `wsmousedevprint`: print function (for parent devices).
pub fn wsmousedevprint(_aux: *mut c_void, pnp: Option<&[u8]>) -> i32 {
    if let Some(pnp) = pnp {
        printf(format_args!(
            "wsmouse at {}",
            crate::kern::subr_prf::Str(pnp)
        ));
    }
    UNCONF
}

/// `wsmouse_match`.
pub fn wsmouse_match(_parent: Option<&Device>, _match: &CfMatch, _aux: *mut c_void) -> i32 {
    1
}

/// `wsmouse_attach`.
pub fn wsmouse_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    let sc = sc_of(self_);
    // SAFETY: a wsmousedev parent hands its child a `wsmousedev_attach_args`.
    let ap = unsafe { &*aux.cast::<WsmousedevAttachArgs>() };

    sc.sc_accessops.set(Some(ap.accessops));
    sc.sc_accesscookie.set(ap.accesscookie);

    {
        let mut input = sc.input();
        input.evar = Some(NonNull::from(&sc.sc_base.me_evp));
        input.dv = Some(NonNull::from(self_));
    }

    sc.sc_base.me_ops.set(Some(&WSMOUSE_SRCOPS));
    let mux = wsmousedevcf_mux(self_.cfdata()) as i32;
    if mux >= 0 {
        match wsmux_attach_sc(wsmux_getmux(mux), &sc.sc_base) {
            Err(error) => {
                printf(format_args!(" attach error={}", error as i32));
            }
            Ok(()) => {
                printf(format_args!(" mux {}", mux));
            }
        }
    }

    printf(format_args!("\n"));
}

/// `wsmouse_activate`.
pub fn wsmouse_activate(self_: &Device, act: i32) -> Result<(), Errno> {
    let sc = sc_of(self_);

    if act == DVACT_DEACTIVATE {
        sc.sc_dying.set(1);
    }
    Ok(())
}

/// `wsmouse_detach`: detach a mouse. To keep track of users of the softc we keep a
/// reference count that's incremented while inside, e.g., read. If the mouse is active and
/// the reference count is > 0 (0 is the normal state) we post an event and then wait for
/// the process that had the reference to wake us up again. Then we blow away the vnode and
/// return (which will deallocate the softc).
pub fn wsmouse_detach(self_: &Device, _flags: i32) -> Result<(), Errno> {
    let sc = sc_of(self_);

    // Tell parent mux we're leaving.
    if sc.sc_base.me_parent.get().is_some() {
        wsmux_detach_sc(&sc.sc_base);
    }

    // If we're open ...
    if let Some(evar) = sc.sc_base.evp() {
        sc.sc_refcnt.set(sc.sc_refcnt.get() - 1);
        if sc.sc_refcnt.get() >= 0 {
            mtx_enter(&evar.ws_mtx);
            // Wake everyone by generating a dummy event.
            let mut put = evar.ws_put.get() + 1;
            if put >= WSEVENT_QSIZE {
                put = 0;
            }
            evar.ws_put.set(put);
            mtx_leave(&evar.ws_mtx);
            wsevent_wakeup(evar);
            // Wait for processes to go away.
            if tsleep_nsec(ptr::from_ref(sc), PZERO, "wsmdet", sec_to_nsec(60)).is_err() {
                printf(format_args!(
                    "wsmouse_detach: {} didn't detach\n",
                    sc.xname()
                ));
            }
        }
    }

    // locate the major number
    let n = nchrdev();
    let maj = (0..n)
        .find(|&maj| ptr::fn_addr_eq(cdevsw(maj).d_open, wsmouseopen as DevTypeOpen))
        .unwrap_or(n);

    // Nuke the vnodes for any open instances (calls close).
    let mn = self_.dv_unit.get() as u32;
    vdevgone(maj, mn, mn, VCHR);

    wsmouse_input_cleanup(&mut sc.input());

    Ok(())
}

/// `wsmouseopen`.
pub fn wsmouseopen(dev: Dev, flags: i32, _mode: i32, _p: &Proc) -> Result<(), Errno> {
    let unit = minor(dev) as i32;
    // make sure it was attached
    let sc = wsmouse_sc(unit).ok_or(Errno::ENXIO)?;

    if sc.sc_dying.get() != 0 {
        return Err(Errno::EIO);
    }

    if flags & (FREAD | FWRITE) == FWRITE {
        // always allow open for write so ioctl() is possible.
        return Ok(());
    }

    if sc.sc_base.me_parent.get().is_some() {
        // Grab the mouse out of the greedy hands of the mux.
        wsmux_detach_sc(&sc.sc_base);
    }

    if sc.sc_base.me_evp.get().is_some() {
        return Err(Errno::EBUSY);
    }

    let evar = &sc.sc_base.me_evar;
    if wsevent_init(evar).is_err() {
        return Err(Errno::EBUSY);
    }

    let error = wsmousedoopen(sc, evar);
    if error.is_err() {
        wsevent_fini(evar);
    }
    error
}

/// `wsmouseclose`.
pub fn wsmouseclose(dev: Dev, flags: i32, _mode: i32, _p: Option<&Proc>) -> Result<(), Errno> {
    let sc = wsmouse_sc(minor(dev) as i32).ok_or(Errno::ENXIO)?;
    let evar = sc.sc_base.me_evp.get();

    if flags & (FREAD | FWRITE) == FWRITE {
        // Not open for read
        return Ok(());
    }

    sc.sc_base.me_evp.set(None);
    sc.disable();
    if let Some(evar) = evar {
        // SAFETY: the mouse's own `me_evar` (`wsmouseopen` opened it), a member of the
        // softc.
        wsevent_fini(unsafe { evar.as_ref() });
    }

    if sc.sc_base.me_parent.get().is_none() {
        let mux = wsmousedevcf_mux(sc.sc_base.me_dv.cfdata()) as i32;
        if mux >= 0
            && let Err(error) = wsmux_attach_sc(wsmux_getmux(mux), &sc.sc_base)
        {
            printf(format_args!(
                "{}: can't attach mux (error={})\n",
                sc.xname(),
                error as i32
            ));
        }
    }

    Ok(())
}

/// `wsmousedoopen`.
pub fn wsmousedoopen(sc: &WsmouseSoftc, evp: &Wseventvar) -> Result<(), Errno> {
    // The device could already be attached to a mux.
    if sc.sc_base.me_evp.get().is_some() {
        return Err(Errno::EBUSY);
    }
    sc.sc_base.me_evp.set(Some(NonNull::from(evp)));

    wsmouse_input_reset(&mut sc.input());

    // enable the device, and punt if that's not possible
    let error = sc.enable();
    if error.is_err() {
        sc.sc_base.me_evp.set(None);
    }
    error
}

/// `wsmouseread`.
pub fn wsmouseread(dev: Dev, uio: &mut Uio<'_>, flags: i32) -> Result<(), Errno> {
    let sc = wsmouse_sc(minor(dev) as i32).ok_or(Errno::ENXIO)?;

    if sc.sc_dying.get() != 0 {
        return Err(Errno::EIO);
    }

    let Some(evar) = sc.sc_base.evp() else {
        if cfg!(feature = "diagnostic") {
            printf(format_args!("wsmouseread: evp == NULL\n"));
        }
        return Err(Errno::EINVAL);
    };

    sc.sc_refcnt.set(sc.sc_refcnt.get() + 1);
    let mut error = wsevent_read(evar, uio, flags);
    sc.sc_refcnt.set(sc.sc_refcnt.get() - 1);
    if sc.sc_refcnt.get() < 0 {
        wakeup(ptr::from_ref(sc));
        error = Err(Errno::EIO);
    }
    error
}

/// `wsmouseioctl`.
pub fn wsmouseioctl(dev: Dev, cmd: u64, data: &mut [u8], flag: i32, p: &Proc) -> Result<(), Errno> {
    let sc = wsmouse_sc(minor(dev) as i32).ok_or(Errno::ENXIO)?;
    wsmousedoioctl(&sc.sc_base.me_dv, cmd, data, flag, Some(p))
}

/// `wsmousedoioctl`: a wrapper around the ioctl() workhorse to make reference counting
/// easy.
pub fn wsmousedoioctl(
    dv: &Device,
    cmd: u64,
    data: &mut [u8],
    flag: i32,
    p: Option<&Proc>,
) -> Result<(), Errno> {
    let sc = sc_of(dv);

    sc.sc_refcnt.set(sc.sc_refcnt.get() + 1);
    let error = wsmouse_do_ioctl(sc, cmd, data, flag, p);
    sc.sc_refcnt.set(sc.sc_refcnt.get() - 1);
    if sc.sc_refcnt.get() < 0 {
        wakeup(ptr::from_ref(sc));
    }
    error
}

/// `wsmouse_param_ioctl`: `WSMOUSEIO_GETPARAMS` and `WSMOUSEIO_SETPARAMS` with the user
/// array `params` of `nparams` pairs.
pub fn wsmouse_param_ioctl(
    sc: &WsmouseSoftc,
    cmd: u64,
    params: usize,
    nparams: u32,
) -> Result<(), Errno> {
    if params == 0 || nparams > WSMOUSECFG_MAX {
        return Err(Errno::EINVAL);
    }

    let size = nparams as usize * size_of::<WsmouseParam>();
    let buf = malloc(size, M_DEVBUF, M_WAITOK).ok_or(Errno::ENOMEM)?;
    // SAFETY: a fresh block of `size` bytes, aligned for any type by `malloc`, ours until
    // the `free` below.
    let bytes = unsafe { core::slice::from_raw_parts_mut(buf.as_ptr(), size) };

    if let Err(error) = copyin(params, bytes) {
        free(buf, M_DEVBUF, size);
        return Err(error);
    }

    // SAFETY: the same block, now `nparams` copied-in `struct wsmouse_param`s, which any
    // bytes are (`AbiPod`); the byte view above is not used while this one lives.
    let pairs = unsafe {
        core::slice::from_raw_parts_mut(buf.as_ptr().cast::<WsmouseParam>(), nparams as usize)
    };

    let s = spltty();
    let error = if cmd == WSMOUSEIO_SETPARAMS {
        sc.input().set_params(pairs).map_err(|_| Errno::EINVAL)
    } else {
        let got = sc.input().get_params(pairs).map_err(|_| Errno::EINVAL);
        got.and_then(|()| {
            // SAFETY: the same block of `size` initialised bytes; `pairs` is not used
            // again.
            let out = unsafe { core::slice::from_raw_parts(buf.as_ptr(), size) };
            copyout(out, params)
        })
    };
    splx(s);
    free(buf, M_DEVBUF, size);
    error
}

/// `wsmouse_do_ioctl`.
pub fn wsmouse_do_ioctl(
    sc: &WsmouseSoftc,
    cmd: u64,
    data: &mut [u8],
    flag: i32,
    p: Option<&Proc>,
) -> Result<(), Errno> {
    if sc.sc_dying.get() != 0 {
        return Err(Errno::EIO);
    }

    // Try the generic ioctls that the wsmouse interface supports.

    if matches!(cmd, FIOASYNC | FIOSETOWN | TIOCSPGRP) && flag & FWRITE == 0 {
        return Err(Errno::EACCES);
    }

    match cmd {
        FIOASYNC => {
            let evar = sc.sc_base.evp().ok_or(Errno::EINVAL)?;
            mtx_enter(&evar.ws_mtx);
            evar.ws_async.set(i32::from(ioctl_arg::<i32>(data) != 0));
            mtx_leave(&evar.ws_mtx);
            return Ok(());
        }
        FIOGETOWN | TIOCGPGRP => {
            let evar = sc.sc_base.evp().ok_or(Errno::EINVAL)?;
            let mut own = 0;
            sigio_getown(&evar.ws_sigio, cmd, &mut own);
            ioctl_ret(data, &own);
            return Ok(());
        }
        FIOSETOWN | TIOCSPGRP => {
            let evar = sc.sc_base.evp().ok_or(Errno::EINVAL)?;
            return sigio_setown(&evar.ws_sigio, cmd, &ioctl_arg::<i32>(data));
        }
        WSMOUSEIO_GETPARAMS | WSMOUSEIO_SETPARAMS => {
            let a: WsmouseParameters = ioctl_arg(data);
            return wsmouse_param_ioctl(sc, cmd, a.params, a.nparams);
        }
        _ => {}
    }

    // Try the mouse driver for WSMOUSEIO ioctls. It returns -1 if it didn't recognize the
    // request.
    match (sc.accessops().ioctl)(sc.sc_accesscookie.get(), cmd, data, flag, p) {
        Ok(true) => Ok(()),
        Ok(false) => Err(Errno::ENOTTY),
        Err(error) => Err(error),
    }
}

/// `wsmousekqfilter`.
pub fn wsmousekqfilter(dev: Dev, kn: &Knote) -> Result<(), Errno> {
    let sc = wsmouse_sc(minor(dev) as i32).ok_or(Errno::ENXIO)?;

    let evar = sc.sc_base.evp().ok_or(Errno::ENXIO)?;
    wsevent_kqfilter(evar, kn)
}

/// `(struct wsmouse_softc *)me`.
fn sc_of_evsrc(me: &Wsevsrc) -> &WsmouseSoftc {
    // SAFETY: `wsmouse_srcops` is only the methods of mice, whose softc starts with the
    // `Wsevsrc` (`#[repr(C)]`).
    unsafe { &*ptr::from_ref(me).cast::<WsmouseSoftc>() }
}

/// `wsmouse_mux_open`.
pub fn wsmouse_mux_open(me: &Wsevsrc, evp: &Wseventvar) -> Result<(), Errno> {
    wsmousedoopen(sc_of_evsrc(me), evp)
}

/// `wsmouse_mux_close`.
pub fn wsmouse_mux_close(me: &Wsevsrc) -> Result<(), Errno> {
    let sc = sc_of_evsrc(me);

    sc.disable();
    sc.sc_base.me_evp.set(None);

    Ok(())
}

/// `wsmouse_add_mux`: `WSMUXIO_ADD_DEVICE` of `wsmouse<unit>` to the mux `muxsc`.
pub fn wsmouse_add_mux(unit: i32, muxsc: &WsmuxSoftc) -> Result<(), Errno> {
    let sc = wsmouse_sc(unit).ok_or(Errno::ENXIO)?;

    if sc.sc_base.me_parent.get().is_some() || sc.sc_base.me_evp.get().is_some() {
        return Err(Errno::EBUSY);
    }

    wsmux_attach_sc(Some(muxsc), &sc.sc_base)
}

/// `wsmouse_buttons`: report button state.
pub fn wsmouse_buttons(sc: &Device, buttons: u32) {
    sc_of(sc).input().buttons(buttons);
}

/// `wsmouse_motion`: report motion deltas (dx, dy, dz, dw).
pub fn wsmouse_motion(sc: &Device, dx: i32, dy: i32, dz: i32, dw: i32) {
    sc_of(sc).input().motion(dx, dy, dz, dw);
}

/// `set_x`.
fn set_x(pos: &mut Position, x: i32, sync: &mut u32, mask: u32) {
    if *sync & mask != 0 {
        if x == pos.x {
            return;
        }
        pos.x -= pos.dx;
        pos.acc_dx -= pos.dx;
    }
    pos.dx = x.wrapping_sub(pos.x);
    if pos.dx != 0 {
        pos.x = x;
        if (pos.dx > 0) == (pos.acc_dx > 0) {
            pos.acc_dx = pos.acc_dx.wrapping_add(pos.dx);
        } else {
            pos.acc_dx = pos.dx;
        }
        *sync |= mask;
    }
}

/// `set_y`.
fn set_y(pos: &mut Position, y: i32, sync: &mut u32, mask: u32) {
    if *sync & mask != 0 {
        if y == pos.y {
            return;
        }
        pos.y -= pos.dy;
        pos.acc_dy -= pos.dy;
    }
    pos.dy = y.wrapping_sub(pos.y);
    if pos.dy != 0 {
        pos.y = y;
        if (pos.dy > 0) == (pos.acc_dy > 0) {
            pos.acc_dy = pos.acc_dy.wrapping_add(pos.dy);
        } else {
            pos.acc_dy = pos.dy;
        }
        *sync |= mask;
    }
}

/// `cleardeltas`.
fn cleardeltas(pos: &mut Position) {
    pos.dx = 0;
    pos.acc_dx = 0;
    pos.dy = 0;
    pos.acc_dy = 0;
}

/// `wsmouse_position`: report absolute coordinates (x, y).
pub fn wsmouse_position(sc: &Device, x: i32, y: i32) {
    sc_of(sc).input().position(x, y);
}

/// `normalized_pressure`.
fn normalized_pressure(input: &WsmouseInput, pressure: i32) -> i32 {
    let limit = input.touch.min_pressure.max(1);

    if pressure >= limit {
        pressure
    } else if pressure < 0 {
        limit
    } else {
        0
    }
}

/// `wsmouse_touch`: report (single-)touch input (pressure, contacts).
pub fn wsmouse_touch(sc: &Device, pressure: i32, contacts: i32) {
    sc_of(sc).input().touch(pressure, contacts);
}

/// `wsmouse_mtstate`: report slot-based multitouch input (slot, x, y, pressure).
pub fn wsmouse_mtstate(sc: &Device, slot: i32, x: i32, y: i32, pressure: i32) {
    sc_of(sc).input().mtstate(slot, x, y, pressure);
}

/// `wsmouse_set`: report a single value (type, value, aux).
pub fn wsmouse_set(sc: &Device, type_: Wsmouseval, value: i32, aux: i32) {
    sc_of(sc).input().set(type_, value, aux);
}

impl WsmouseInput {
    /// The body of `wsmouse_buttons`.
    pub fn buttons(&mut self, buttons: u32) {
        let btn = &mut self.btn;

        if btn.sync != 0 {
            // Restore the old state.
            btn.buttons ^= btn.sync;
        }

        btn.sync = btn.buttons ^ buttons;
        btn.buttons = buttons;
    }

    /// The body of `wsmouse_motion`.
    pub fn motion(&mut self, dx: i32, dy: i32, dz: i32, dw: i32) {
        let motion = &mut self.motion;

        motion.dx = dx;
        motion.dy = dy;
        motion.dz = dz;
        motion.dw = dw;
        if dx != 0 || dy != 0 || dz != 0 || dw != 0 {
            motion.sync |= SYNC_DELTAS;
        }
    }

    /// The body of `wsmouse_position`.
    pub fn position(&mut self, x: i32, y: i32) {
        let motion = &mut self.motion;

        set_x(&mut motion.pos, x, &mut motion.sync, SYNC_X);
        set_y(&mut motion.pos, y, &mut motion.sync, SYNC_Y);
    }

    /// The body of `wsmouse_touch`.
    pub fn touch(&mut self, pressure: i32, contacts: i32) {
        let pressure = normalized_pressure(self, pressure);
        let contacts = if pressure != 0 { contacts.max(1) } else { 0 };
        let touch = &mut self.touch;

        if pressure == 0 || pressure != touch.pressure {
            // pressure == 0: Drivers may report possibly arbitrary coordinates in this
            // case; touch_update will correct them.
            touch.pressure = pressure;
            touch.sync |= SYNC_PRESSURE;
        }
        if contacts != touch.contacts {
            touch.contacts = contacts;
            touch.sync |= SYNC_CONTACTS;
        }
    }

    /// The body of `wsmouse_mtstate`.
    pub fn mtstate(&mut self, slot: i32, x: i32, y: i32, pressure: i32) {
        if slot < 0 || slot >= self.mt.num_slots {
            return;
        }
        let pressure = normalized_pressure(self, pressure);
        let mt = &mut self.mt;

        let bit = 1u32 << slot;
        mt.frame |= bit;

        let mut mts = mt.slots()[slot as usize];

        let [sync_touch, sync_x, sync_y, sync_pressure] = &mut mt.sync;
        set_x(&mut mts.pos, x, sync_x, bit);
        set_y(&mut mts.pos, y, sync_y, bit);

        // Is this a new touch?
        if (mt.touches & bit) == (*sync_touch & bit) {
            cleardeltas(&mut mts.pos);
        }

        if pressure != mts.pressure {
            mts.pressure = pressure;
            *sync_pressure |= bit;

            if pressure != 0 {
                if mt.touches & bit == 0 {
                    mt.num_touches += 1;
                    mt.touches |= bit;
                    *sync_touch |= bit;

                    *sync_x |= bit;
                    *sync_y |= bit;
                }
            } else if mt.touches & bit != 0 {
                mt.num_touches -= 1;
                mt.touches ^= bit;
                *sync_touch |= bit;
                mt.ptr_mask &= mt.touches;
            }
        }
        mt.slots_mut()[slot as usize] = mts;
    }

    /// The body of `wsmouse_set`.
    pub fn set(&mut self, type_: Wsmouseval, mut value: i32, aux: i32) {
        let mut mts = MtSlot::default();
        if wsmouse_is_mt_code(type_) {
            if aux < 0 || aux >= self.mt.num_slots {
                return;
            }
            mts = self.mt.slots()[aux as usize];
        }

        match type_ {
            WSMOUSE_REL_X | WSMOUSE_ABS_X => {
                if type_ == WSMOUSE_REL_X {
                    value += self.motion.pos.x;
                }
                self.position(value, self.motion.pos.y);
            }
            WSMOUSE_REL_Y | WSMOUSE_ABS_Y => {
                if type_ == WSMOUSE_REL_Y {
                    value += self.motion.pos.y;
                }
                self.position(self.motion.pos.x, value);
            }
            WSMOUSE_PRESSURE => self.touch(value, self.touch.contacts),
            WSMOUSE_CONTACTS => {
                // Contact counts can be overridden by wsmouse_touch.
                if value != self.touch.contacts {
                    self.touch.contacts = value;
                    self.touch.sync |= SYNC_CONTACTS;
                }
            }
            WSMOUSE_TOUCH_WIDTH => {
                if value != self.touch.width {
                    self.touch.width = value;
                    self.touch.sync |= SYNC_TOUCH_WIDTH;
                }
            }
            WSMOUSE_MT_REL_X | WSMOUSE_MT_ABS_X => {
                if type_ == WSMOUSE_MT_REL_X {
                    value += mts.pos.x;
                }
                self.mtstate(aux, value, mts.pos.y, mts.pressure);
            }
            WSMOUSE_MT_REL_Y | WSMOUSE_MT_ABS_Y => {
                if type_ == WSMOUSE_MT_REL_Y {
                    value += mts.pos.y;
                }
                self.mtstate(aux, mts.pos.x, value, mts.pressure);
            }
            WSMOUSE_MT_PRESSURE => self.mtstate(aux, mts.pos.x, mts.pos.y, value),
            _ => {}
        }
    }
}

/// `wsmouse_touch_update`: make touch and motion state consistent.
pub fn wsmouse_touch_update(input: &mut WsmouseInput) {
    let motion = &mut input.motion;
    let touch = &mut input.touch;

    if touch.pressure == 0 {
        // There may be zero coordinates, or coordinates of touches with pressure values
        // below min_pressure.
        if motion.sync & SYNC_POSITION != 0 {
            // Restore valid coordinates.
            motion.pos.x -= motion.pos.dx;
            motion.pos.y -= motion.pos.dy;
            motion.sync &= !SYNC_POSITION;
        }

        if touch.prev_contacts == 0 {
            touch.sync &= !SYNC_PRESSURE;
        }
    }

    if touch.sync & SYNC_CONTACTS != 0 {
        // Suppress pointer movement.
        cleardeltas(&mut motion.pos);
    }

    if touch.sync & SYNC_PRESSURE != 0 && touch.min_pressure != 0 {
        if touch.pressure >= input.filter.pressure_hi {
            touch.min_pressure = input.filter.pressure_lo;
        } else if touch.pressure < input.filter.pressure_lo {
            touch.min_pressure = input.filter.pressure_hi;
        }
    }
}

/// `wsmouse_mt_update`: normalize multitouch state.
pub fn wsmouse_mt_update(input: &mut WsmouseInput) {
    // The same as above: There may be arbitrary coordinates if (pressure == 0). Clear the
    // sync flags for touches that have been released.
    if input.mt.frame & !input.mt.touches != 0 {
        for i in MTS_X..MTS_SIZE {
            input.mt.sync[i] &= input.mt.touches;
        }
    }
}

/// `wsmouse_hysteresis`: return TRUE if a coordinate update may be noise.
pub fn wsmouse_hysteresis(input: &WsmouseInput, pos: &Position) -> bool {
    pos.acc_dx.abs() < input.filter.h.hysteresis && pos.acc_dy.abs() < input.filter.v.hysteresis
}

/// `wsmouse_ptr_ctrl`: select the pointer-controlling MT slot.
///
/// Pointer-control is assigned to slots with non-zero motion deltas if at least one such
/// slot exists. This function doesn't impose any restrictions on the way drivers use
/// `wsmouse_mtstate()`, it covers partial, unordered, and "delta-filtered" input.
///
/// The "cycle" is the set of slots with X/Y updates in previous sync operations; it will be
/// cleared and rebuilt whenever a slot that is being updated is already a member. If a cycle
/// ends that doesn't contain the pointer-controlling slot, a new slot will be selected.
pub fn wsmouse_ptr_ctrl(input: &mut WsmouseInput) {
    input.mt.prev_ptr = input.mt.ptr;

    if input.mt.num_touches <= 1 {
        input.mt.ptr = input.mt.touches;
        input.mt.ptr_cycle = input.mt.ptr;
        return;
    }

    let mt = &input.mt;
    let mut updates = (mt.sync[MTS_X] | mt.sync[MTS_Y]) & !mt.sync[MTS_TOUCH];
    for slot in foreachbit(updates) {
        // Touches that just produce noise are no problem if the frequency of zero deltas
        // is high enough, but there might be no guarantee for that.
        if wsmouse_hysteresis(input, &mt.slots()[slot as usize].pos) {
            updates ^= 1 << slot;
        }
    }

    let mt = &mut input.mt;
    // If there is no pointer-controlling slot, or if it should be masked, select a new one.
    let mut select = (mt.ptr & mt.touches & !mt.ptr_mask) == 0;

    // Remove slots without coordinate deltas from the cycle.
    mt.ptr_cycle &= !(mt.frame ^ updates);

    if mt.ptr_cycle & updates != 0 {
        select |= (mt.ptr_cycle & mt.ptr) == 0;
        mt.ptr_cycle = updates;
    } else {
        mt.ptr_cycle |= updates;
    }
    if select {
        let slot = if mt.ptr_cycle & !mt.ptr_mask != 0 {
            ffs(mt.ptr_cycle & !mt.ptr_mask) - 1
        } else if mt.touches & !mt.ptr_mask != 0 {
            ffs(mt.touches & !mt.ptr_mask) - 1
        } else {
            ffs(mt.touches) - 1
        };
        mt.ptr = 1u32.wrapping_shl(slot as u32);
    }
}

/// `wsmouse_mt_convert`: derive touch and motion state from MT state.
pub fn wsmouse_mt_convert(input: &mut WsmouseInput) {
    wsmouse_ptr_ctrl(input);

    let pressure = if input.mt.ptr != 0 {
        let slot = (ffs(input.mt.ptr) - 1) as usize;
        let (ptr, prev_ptr) = (input.mt.ptr, input.mt.prev_ptr);
        let mts = &mut input.mt.slots_mut()[slot];
        if mts.pos.x != input.motion.pos.x {
            input.motion.sync |= SYNC_X;
        }
        if mts.pos.y != input.motion.pos.y {
            input.motion.sync |= SYNC_Y;
        }
        if ptr != prev_ptr {
            // Suppress pointer movement.
            mts.pos.dx = 0;
            mts.pos.dy = 0;
        }
        input.motion.pos = mts.pos;

        mts.pressure
    } else {
        0
    };

    let num_touches = input.mt.num_touches;
    input.touch(pressure, num_touches);
}

/// `wsmouse_evq_put`: put an event behind `evq->put`, or record the overflow.
pub fn wsmouse_evq_put(evq: &mut EvqAccess<'_>, ev_type: u32, ev_value: i32) {
    let evar = evq.evar;
    mtx_enter(&evar.ws_mtx);
    let space = evar.ws_get.get() as i32 - evq.put as i32;

    if space != 1 && space != 1 - WSEVENT_QSIZE as i32 {
        let ev = WsconsEvent {
            type_: ev_type,
            value: ev_value,
            time: evq.ts,
        };
        // SAFETY: the queue is open (`me_evp`), `put < WSEVENT_QSIZE`, `ws_mtx` held; the
        // slot is beyond `ws_put`, which no reader passes.
        unsafe { evar.q_write(evq.put, ev) };
        evq.put = (evq.put + 1) % WSEVENT_QSIZE;
        evq.result |= EVQ_RESULT_SUCCESS;
    } else {
        evq.result = EVQ_RESULT_OVERFLOW;
    }
    mtx_leave(&evar.ws_mtx);
}

/// `wsmouse_btn_sync`.
pub fn wsmouse_btn_sync(btn: &BtnState, evq: &mut EvqAccess<'_>) {
    for button in foreachbit(btn.sync) {
        let bit = 1u32 << button;
        let ev_type = if btn.buttons & bit != 0 {
            BTN_DOWN_EV
        } else {
            BTN_UP_EV
        };
        wsmouse_evq_put(evq, ev_type, button);
    }
}

/// `scale`: scale with a [*.12] fixed-point factor and a remainder.
fn scale(val: i32, factor: i32, rmdr: &mut i32) -> i32 {
    let val = val.wrapping_mul(factor).wrapping_add(*rmdr);
    if val >= 0 {
        *rmdr = val & 0xfff;
        val >> 12
    } else {
        *rmdr = -(val.wrapping_neg() & 0xfff);
        -(val.wrapping_neg() >> 12)
    }
}

/// `wsmouse_motion_sync`.
pub fn wsmouse_motion_sync(input: &mut WsmouseInput, evq: &mut EvqAccess<'_>) {
    let (delta_x, delta_y) = (delta_x_ev(input), delta_y_ev(input));
    let (abs_x, abs_y) = (abs_x_ev(input), abs_y_ev(input));
    let touchpad = is_touchpad(input);
    let flags = input.flags;
    let motion = &input.motion;
    let h = &mut input.filter.h;
    let v = &mut input.filter.v;

    if motion.sync & SYNC_DELTAS != 0 {
        let mut dx = if h.inv != 0 { -motion.dx } else { motion.dx };
        let mut dy = if v.inv != 0 { -motion.dy } else { motion.dy };
        if h.scale != 0 {
            dx = scale(dx, h.scale, &mut h.rmdr);
        }
        if v.scale != 0 {
            dy = scale(dy, v.scale, &mut v.rmdr);
        }
        if dx != 0 {
            wsmouse_evq_put(evq, delta_x, dx);
        }
        if dy != 0 {
            wsmouse_evq_put(evq, delta_y, dy);
        }
        if motion.dz != 0 {
            let dz = if flags & REVERSE_SCROLLING != 0 {
                -motion.dz
            } else {
                motion.dz
            };
            if touchpad {
                wsmouse_evq_put(evq, VSCROLL_EV, dz);
            } else {
                wsmouse_evq_put(evq, DELTA_Z_EV, dz);
            }
        }
        if motion.dw != 0 {
            let dw = if flags & REVERSE_SCROLLING != 0 {
                -motion.dw
            } else {
                motion.dw
            };
            if touchpad {
                wsmouse_evq_put(evq, HSCROLL_EV, dw);
            } else {
                wsmouse_evq_put(evq, DELTA_W_EV, dw);
            }
        }
    }
    if motion.sync & SYNC_POSITION != 0 {
        if motion.sync & SYNC_X != 0 {
            let x = if h.inv != 0 {
                h.inv - motion.pos.x
            } else {
                motion.pos.x
            };
            wsmouse_evq_put(evq, abs_x, x);
        }
        if motion.sync & SYNC_Y != 0 {
            let y = if v.inv != 0 {
                v.inv - motion.pos.y
            } else {
                motion.pos.y
            };
            wsmouse_evq_put(evq, abs_y, y);
        }
        if motion.pos.dx == 0 && motion.pos.dy == 0 && flags & TPAD_NATIVE_MODE != 0 {
            // Suppress pointer motion.
            wsmouse_evq_put(evq, WSCONS_EVENT_TOUCH_RESET, 0);
        }
    }
}

/// `wsmouse_touch_sync`.
pub fn wsmouse_touch_sync(input: &WsmouseInput, evq: &mut EvqAccess<'_>) {
    let touch = &input.touch;

    if touch.sync & SYNC_PRESSURE != 0 {
        wsmouse_evq_put(evq, ABS_Z_EV, touch.pressure);
    }
    if touch.sync & SYNC_CONTACTS != 0 {
        wsmouse_evq_put(evq, ABS_W_EV, touch.contacts);
    }
    if touch.sync & SYNC_TOUCH_WIDTH != 0 && input.flags & TPAD_NATIVE_MODE != 0 {
        wsmouse_evq_put(evq, WSCONS_EVENT_TOUCH_WIDTH, touch.width);
    }
}

/// `wsmouse_log_input`: `WSMOUSECFG_LOG_INPUT`'s line of the report.
pub fn wsmouse_log_input(input: &WsmouseInput, ts: &Timespec) {
    let motion = &input.motion;

    let t_sync = input.touch.sync & SYNC_CONTACTS != 0;
    let mt_sync =
        input.mt.frame != 0 && (input.mt.sync[MTS_TOUCH] != 0 || input.mt.ptr != input.mt.prev_ptr);

    if motion.sync != 0 || mt_sync || t_sync || input.btn.sync != 0 {
        printf(format_args!("[{}-in][{:04}]", devname(input), logtime(ts)));
    } else {
        return;
    }

    if motion.sync & SYNC_POSITION != 0 {
        printf(format_args!(" abs:{},{}", motion.pos.x, motion.pos.y));
    }
    if motion.sync & SYNC_DELTAS != 0 {
        printf(format_args!(
            " rel:{},{},{},{}",
            motion.dx, motion.dy, motion.dz, motion.dw
        ));
    }
    if mt_sync {
        printf(format_args!(
            " mt:0x{:02x}:{}",
            input.mt.touches,
            ffs(input.mt.ptr) - 1
        ));
    } else if t_sync {
        printf(format_args!(" t:{}", input.touch.contacts));
    }
    if input.btn.sync != 0 {
        printf(format_args!(" btn:0x{:02x}", input.btn.buttons));
    }
    printf(format_args!("\n"));
}

/// `wsmouse_log_events`: `WSMOUSECFG_LOG_EVENTS`'s line of the events not yet published.
pub fn wsmouse_log_events(input: &WsmouseInput, evq: &EvqAccess<'_>) {
    mtx_enter(&evq.evar.ws_mtx);
    let mut n = evq.evar.ws_put.get();
    mtx_leave(&evq.evar.ws_mtx);

    if n != evq.put {
        printf(format_args!(
            "[{}-ev][{:04}]",
            devname(input),
            logtime(&evq.ts)
        ));
        while n != evq.put {
            // SAFETY: the queue is open and `n` is between `ws_put` and `evq.put`: slots
            // this sync wrote and nobody reads yet.
            let ev = unsafe { evq.evar.q_read(n) };
            n = (n + 1) % WSEVENT_QSIZE;
            printf(format_args!(" {}:{}", ev.type_, ev.value));
        }
        printf(format_args!("\n"));
    }
}

/// `clear_sync_flags`.
fn clear_sync_flags(input: &mut WsmouseInput) {
    input.btn.sync = 0;
    input.sbtn.sync = 0;
    input.motion.sync = 0;
    input.touch.sync = 0;
    input.touch.prev_contacts = input.touch.contacts;
    if input.mt.frame != 0 {
        input.mt.frame = 0;
        input.mt.sync = [0; MTS_SIZE];
    }
}

/// `wsmouse_input_sync`: synchronize (generate wscons events).
pub fn wsmouse_input_sync(sc: &Device) {
    sc_of(sc).input().input_sync();
}

impl WsmouseInput {
    /// The body of `wsmouse_input_sync`.
    pub fn input_sync(&mut self) {
        let Some(evar) = self.evar() else {
            return;
        };
        // SAFETY: the queue outlives this sync: it is freed only by the close that clears
        // `me_evp`, which runs under the kernel lock, not inside a report. The reference is
        // detached from `self` so the input can change while events are put.
        let evar: &Wseventvar = unsafe { &*ptr::from_ref(evar) };
        let input = self;
        mtx_enter(&evar.ws_mtx);
        let put = evar.ws_put.get();
        mtx_leave(&evar.ws_mtx);
        let mut evq = EvqAccess {
            evar,
            ts: getnanotime(),
            put,
            result: EVQ_RESULT_NONE,
        };

        enqueue_randomness(
            input.btn.buttons
                ^ input.motion.dx as u32
                ^ input.motion.dy as u32
                ^ input.motion.pos.x as u32
                ^ input.motion.pos.y as u32
                ^ input.motion.dz as u32
                ^ input.motion.dw as u32,
        );

        if input.mt.frame != 0 {
            wsmouse_mt_update(input);
            wsmouse_mt_convert(input);
        }
        if input.touch.sync != 0 {
            wsmouse_touch_update(input);
        }

        if input.flags & LOG_INPUT != 0 {
            wsmouse_log_input(input, &evq.ts);
        }

        if input.flags & TPAD_COMPAT_MODE != 0 {
            wstpad_compat_convert(input, &mut evq);
        }

        if input.flags & RESYNC != 0 {
            input.flags &= !RESYNC;
            input.motion.sync &= SYNC_POSITION;
        }

        if input.btn.sync != 0 {
            wsmouse_btn_sync(&input.btn, &mut evq);
        }
        if input.sbtn.sync != 0 {
            wsmouse_btn_sync(&input.sbtn, &mut evq);
        }
        if input.motion.sync != 0 {
            wsmouse_motion_sync(input, &mut evq);
        }
        if input.touch.sync != 0 {
            wsmouse_touch_sync(input, &mut evq);
        }
        // No MT events are generated yet.

        if evq.result == EVQ_RESULT_SUCCESS {
            wsmouse_evq_put(&mut evq, WSCONS_EVENT_SYNC, 0);
            if evq.result == EVQ_RESULT_SUCCESS {
                if input.flags & LOG_EVENTS != 0 {
                    wsmouse_log_events(input, &evq);
                }
                mtx_enter(&evar.ws_mtx);
                evar.ws_put.set(evq.put);
                mtx_leave(&evar.ws_mtx);
                wsevent_wakeup(evar);
            }
        }

        if evq.result != EVQ_RESULT_OVERFLOW {
            clear_sync_flags(input);
        } else {
            input.flags |= RESYNC;
        }
    }
}

/// `wsmouse_id_to_slot`: assign or look up a slot number for a tracking ID (id); -1 when
/// there is none.
pub fn wsmouse_id_to_slot(sc: &Device, id: i32) -> i32 {
    sc_of(sc).input().id_to_slot(id)
}

impl WsmouseInput {
    /// The body of `wsmouse_id_to_slot`.
    pub fn id_to_slot(&mut self, id: i32) -> i32 {
        let mt = &mut self.mt;

        if mt.num_slots == 0 {
            return -1;
        }

        for slot in foreachbit(mt.touches) {
            if mt.slots()[slot as usize].id == id {
                return slot;
            }
        }
        let slot = ffs(!(mt.touches | mt.frame)) - 1;
        if slot >= 0 && slot < mt.num_slots {
            mt.frame |= 1 << slot;
            mt.slots_mut()[slot as usize].id = id;
            slot
        } else {
            -1
        }
    }
}

/// `wsmouse_matching`: find a minimum-weight matching for an m-by-n matrix.
///
/// m must be greater than or equal to n. The size of the buffer must be at least 3m + 3n.
///
/// On return, the first m elements of the buffer contain the row-to-column mappings, i.e.,
/// buffer[i] is the column index for row i, or -1 if there is no assignment for that row
/// (which may happen if n < m).
///
/// Wrong results because of overflows will not occur with input values in the range of 0
/// to INT_MAX / 2 inclusive.
///
/// The function applies the Dinic-Kronrod algorithm. It is not modern or popular, but it
/// seems to be a good choice for small matrices at least. The original form of the
/// algorithm is modified as follows: There is no initial search for row minima, the initial
/// assignments are in a "virtual" column with the index -1 and zero values. This permits
/// inputs with n < m, and it simplifies the reassignments.
pub fn wsmouse_matching(matrix: &[i32], m: usize, n: usize, buffer: &mut [i32]) {
    let (r2c, rest) = buffer.split_at_mut(m); // row-to-column assignments
    let (red, rest) = rest.split_at_mut(m); // reduced values of the assignments
    let (mc, rest) = rest.split_at_mut(m); // row-wise minimal elements of cs
    let (cs, rest) = rest.split_at_mut(n); // the column set
    let (c2r, rest) = rest.split_at_mut(n); // column-to-row assignments in cs
    let cd = &mut rest[..n]; // column deltas (reduction)

    r2c.fill(-1);
    red.fill(0);
    for col in 0..n {
        let mut delta = i32::MAX;
        let mut row = 0;
        for i in 0..m {
            let d = matrix[i * n + col].wrapping_sub(red[i]);
            if d < delta || (d == delta && r2c[i] < 0) {
                delta = d;
                row = i;
            }
        }
        cd[col] = delta;
        if r2c[row] < 0 {
            r2c[row] = col as i32;
            continue;
        }
        mc.fill(col as i32);
        let mut k = 0;
        loop {
            let j = r2c[row];
            if j < 0 {
                break;
            }
            let j = j as usize;
            cs[k] = j as i32;
            k += 1;
            c2r[j] = row as i32;
            mc[row] -= n as i32;
            delta = i32::MAX;
            for i in 0..m {
                let p = &matrix[i * n..(i + 1) * n];
                if mc[i] >= 0 {
                    let mut d = p[mc[i] as usize].wrapping_sub(cd[mc[i] as usize]);
                    let e = p[j].wrapping_sub(cd[j]);
                    if e < d {
                        d = e;
                        mc[i] = j as i32;
                    }
                    d = d.wrapping_sub(red[i]);
                    if d < delta || (d == delta && r2c[i] < 0) {
                        delta = d;
                        row = i;
                    }
                }
            }
            cd[col] = cd[col].wrapping_add(delta);
            for &c in &cs[..k] {
                cd[c as usize] = cd[c as usize].wrapping_add(delta);
                red[c2r[c as usize] as usize] = red[c2r[c as usize] as usize].wrapping_sub(delta);
            }
        }
        let mut j = mc[row];
        loop {
            r2c[row] = j;
            if j == col as i32 {
                break;
            }
            row = c2r[j as usize] as usize;
            j = mc[row] + n as i32;
        }
    }
}

/// `wsmouse_mtframe`: report multitouch input (the points of `pt`).
///
/// Assign slot numbers to the points in the pt array, and update all slots by calling
/// wsmouse_mtstate internally. The slot numbers are passed to the caller in the pt->slot
/// fields.
///
/// The slot assignment pairs the points with points of the previous frame in such a way
/// that the sum of the squared distances is minimal. Using squares instead of simple
/// distances favours assignments with more uniform distances, and it is faster.
pub fn wsmouse_mtframe(sc: &Device, pt: &mut [Mtpoint]) {
    sc_of(sc).input().mtframe(pt);
}

impl WsmouseInput {
    /// The body of `wsmouse_mtframe`.
    pub fn mtframe(&mut self, pt: &mut [Mtpoint]) {
        let maxdist = self.filter.tracking_maxdist;
        let mt = &mut self.mt;
        if mt.num_slots == 0 || mt.matrix.is_none() {
            return;
        }

        let size = pt.len().min(mt.num_slots as usize);
        let pt = &mut pt[..size];
        let touches = mt.touches;
        let num_touches = mt.num_touches as usize;
        let Some((slots, buf)) = mt.tracking() else {
            return;
        };
        let dist2 = |p: &Mtpoint, s: &MtSlot| {
            let dx = p.x.wrapping_sub(s.pos.x);
            let dy = p.y.wrapping_sub(s.pos.y);
            dx.wrapping_mul(dx).wrapping_add(dy.wrapping_mul(dy))
        };
        let mut k = 0;
        let (m, n) = if num_touches >= size {
            for slot in foreachbit(touches) {
                for p in pt.iter() {
                    buf[k] = dist2(p, &slots[slot as usize]);
                    k += 1;
                }
            }
            (num_touches, size)
        } else {
            for p in pt.iter() {
                for slot in foreachbit(touches) {
                    buf[k] = dist2(p, &slots[slot as usize]);
                    k += 1;
                }
            }
            (size, num_touches)
        };
        let (matrix, rest) = buf.split_at_mut(k);
        wsmouse_matching(matrix, m, n, rest);

        let (r2c, rest) = rest.split_at_mut(m);
        let c2r = &mut rest[..n.max(m)];
        let maxdist = if maxdist != 0 {
            maxdist.wrapping_mul(maxdist)
        } else {
            i32::MAX
        };
        for i in 0..m {
            let j = r2c[i];
            if j >= 0 {
                if matrix[i * n + j as usize] <= maxdist {
                    c2r[j as usize] = i as i32;
                } else {
                    c2r[j as usize] = -1;
                    r2c[i] = -1;
                }
            }
        }

        // The assignments go out of the buffer: the slots change below.
        let mut points_of = [0i32; WSMOUSE_MT_SLOTS_MAX as usize];
        let mut slots_of = [0i32; WSMOUSE_MT_SLOTS_MAX as usize];
        let (p_new, p_old) = if n == size {
            (&c2r[..size], &r2c[..num_touches])
        } else {
            (&r2c[..size], &c2r[..num_touches])
        };
        points_of[..size].copy_from_slice(p_new);
        slots_of[..num_touches].copy_from_slice(p_old);

        for i in 0..size {
            if points_of[i] < 0 {
                let slot = ffs(!(self.mt.touches | self.mt.frame)) - 1;
                if slot < 0 || slot >= self.mt.num_slots {
                    break;
                }
                self.mtstate(slot, pt[i].x, pt[i].y, pt[i].pressure);
                pt[i].slot = slot;
            }
        }

        for (k, slot) in foreachbit(touches).enumerate() {
            let i = slots_of[k];
            if i >= 0 {
                let p = &mut pt[i as usize];
                self.mtstate(slot, p.x, p.y, p.pressure);
                p.slot = slot;
            } else {
                self.mtstate(slot, 0, 0, 0);
            }
        }
    }
}

/// `free_mt_slots`.
fn free_mt_slots(input: &mut WsmouseInput) {
    let n = input.mt.num_slots;
    if n != 0 {
        let mut size = n as usize * size_of::<MtSlot>();
        if input.flags & MT_TRACKING != 0 {
            size += matrix_size(n as usize);
        }
        input.mt.num_slots = 0;
        if let Some(slots) = input.mt.slots.take() {
            free(slots.cast(), M_DEVBUF, size);
        }
        input.mt.matrix = None;
    }
}

/// `wsmouse_mt_init`: initialize MT structures (num_slots, tracking): allocate the MT slots
/// and, if necessary, the buffers for MT tracking. `Err(EINVAL)` for the C's -1 (no slots).
pub fn wsmouse_mt_init(sc: &Device, num_slots: i32, tracking: bool) -> Result<(), Errno> {
    sc_of(sc).input().mt_init(num_slots, tracking)
}

impl WsmouseInput {
    /// The body of `wsmouse_mt_init`.
    pub fn mt_init(&mut self, num_slots: i32, tracking: bool) -> Result<(), Errno> {
        if num_slots == self.mt.num_slots && tracking == (self.flags & MT_TRACKING != 0) {
            return Ok(());
        }

        free_mt_slots(self);

        if tracking {
            self.flags |= MT_TRACKING;
        } else {
            self.flags &= !MT_TRACKING;
        }
        let n = num_slots.clamp(0, WSMOUSE_MT_SLOTS_MAX);
        if n != 0 {
            let mut size = n as usize * size_of::<MtSlot>();
            if self.flags & MT_TRACKING != 0 {
                size += matrix_size(n as usize);
            }
            if let Some(mem) = malloc(size, M_DEVBUF, M_WAITOK | M_ZERO) {
                let slots = mem.cast::<MtSlot>();
                self.mt.slots = Some(slots);
                if self.flags & MT_TRACKING != 0 {
                    // SAFETY: the matrix starts right behind the `n` slots, inside the
                    // allocation of `size` bytes.
                    self.mt.matrix = Some(unsafe { slots.add(n as usize) }.cast());
                }
                self.mt.num_slots = n;
                return Ok(());
            }
        }
        Err(Errno::EINVAL)
    }

    /// The body of `wsmouse_get_params`.
    pub fn get_params(&mut self, params: &mut [WsmouseParam]) -> Result<(), Errno> {
        for param in params.iter_mut() {
            let key = param.key;
            param.value = match key {
                WSMOUSECFG_DX_SCALE => self.filter.h.scale,
                WSMOUSECFG_DY_SCALE => self.filter.v.scale,
                WSMOUSECFG_PRESSURE_LO => self.filter.pressure_lo,
                WSMOUSECFG_PRESSURE_HI => self.filter.pressure_hi,
                WSMOUSECFG_TRKMAXDIST => self.filter.tracking_maxdist,
                WSMOUSECFG_SWAPXY => self.filter.swapxy,
                WSMOUSECFG_X_INV => self.filter.h.inv,
                WSMOUSECFG_Y_INV => self.filter.v.inv,
                WSMOUSECFG_REVERSE_SCROLLING => i32::from(self.flags & REVERSE_SCROLLING != 0),
                WSMOUSECFG_DX_MAX => self.filter.h.dmax,
                WSMOUSECFG_DY_MAX => self.filter.v.dmax,
                WSMOUSECFG_X_HYSTERESIS => self.filter.h.hysteresis,
                WSMOUSECFG_Y_HYSTERESIS => self.filter.v.hysteresis,
                WSMOUSECFG_DECELERATION => self.filter.dclr,
                WSMOUSECFG_STRONG_HYSTERESIS => 0, // The feature has been removed.
                WSMOUSECFG_SMOOTHING => (self.filter.mode & SMOOTHING_MASK) as i32,
                WSMOUSECFG_LOG_INPUT => i32::from(self.flags & LOG_INPUT != 0),
                WSMOUSECFG_LOG_EVENTS => i32::from(self.flags & LOG_EVENTS != 0),
                _ => wstpad_get_param(self, key)?,
            };
        }

        Ok(())
    }

    /// The body of `wsmouse_set_params`.
    pub fn set_params(&mut self, params: &[WsmouseParam]) -> Result<(), Errno> {
        let mut needreset = false;

        for param in params {
            let (key, val) = (param.key, param.value);
            match key {
                WSMOUSECFG_PRESSURE_LO => {
                    self.filter.pressure_lo = val;
                    if val > self.filter.pressure_hi {
                        self.filter.pressure_hi = val;
                    }
                    self.touch.min_pressure = self.filter.pressure_hi;
                }
                WSMOUSECFG_PRESSURE_HI => {
                    self.filter.pressure_hi = val;
                    if val < self.filter.pressure_lo {
                        self.filter.pressure_lo = val;
                    }
                    self.touch.min_pressure = val;
                }
                WSMOUSECFG_X_HYSTERESIS => self.filter.h.hysteresis = val,
                WSMOUSECFG_Y_HYSTERESIS => self.filter.v.hysteresis = val,
                WSMOUSECFG_DECELERATION => {
                    self.filter.dclr = val;
                    wstpad_init_deceleration(self);
                }
                WSMOUSECFG_DX_SCALE => self.filter.h.scale = val,
                WSMOUSECFG_DY_SCALE => self.filter.v.scale = val,
                WSMOUSECFG_TRKMAXDIST => self.filter.tracking_maxdist = val,
                WSMOUSECFG_SWAPXY => self.filter.swapxy = val,
                WSMOUSECFG_X_INV => self.filter.h.inv = val,
                WSMOUSECFG_Y_INV => self.filter.v.inv = val,
                WSMOUSECFG_REVERSE_SCROLLING => set_flag(&mut self.flags, REVERSE_SCROLLING, val),
                WSMOUSECFG_DX_MAX => self.filter.h.dmax = val,
                WSMOUSECFG_DY_MAX => self.filter.v.dmax = val,
                WSMOUSECFG_SMOOTHING => {
                    self.filter.mode &= !SMOOTHING_MASK;
                    self.filter.mode |= val as u32 & SMOOTHING_MASK;
                }
                WSMOUSECFG_LOG_INPUT => set_flag(&mut self.flags, LOG_INPUT, val),
                WSMOUSECFG_LOG_EVENTS => set_flag(&mut self.flags, LOG_EVENTS, val),
                _ => {
                    needreset = true;
                    wstpad_set_param(self, key, val)?;
                }
            }
        }

        // Reset soft-states if touchpad parameters changed
        if needreset {
            wstpad_reset(self);
            return wstpad_configure(self);
        }

        Ok(())
    }

    /// The body of `wsmouse_set_mode`.
    pub fn set_mode(&mut self, mode: i32) -> bool {
        if mode == WSMOUSE_COMPAT {
            self.flags &= !TPAD_NATIVE_MODE;
            self.flags |= TPAD_COMPAT_MODE;
            true
        } else if mode == WSMOUSE_NATIVE {
            self.flags &= !TPAD_COMPAT_MODE;
            self.flags |= TPAD_NATIVE_MODE;
            true
        } else {
            false
        }
    }

    /// The body of `wsmouse_configure`.
    pub fn configure(&mut self, params: Option<&[WsmouseParam]>) -> Result<(), Errno> {
        if self.flags & CONFIGURED == 0 {
            if self.hw.x_max != 0 && self.hw.y_max != 0 && self.hw.flags & WSMOUSEHW_LR_DOWN != 0 {
                self.filter.v.inv = self.hw.y_max + self.hw.y_min;
            }
            self.filter.ratio = 1 << 12;
            if self.hw.h_res > 0 && self.hw.v_res > 0 {
                self.filter.ratio *= self.hw.h_res;
                self.filter.ratio /= self.hw.v_res;
            }
            if self
                .mt_init(self.hw.mt_slots, self.hw.flags & WSMOUSEHW_MT_TRACKING != 0)
                .is_err()
            {
                printf(format_args!(
                    "wsmouse_configure: MT initialization failed.\n"
                ));
                return Err(Errno::EINVAL);
            }
            if is_touchpad(self) && wstpad_configure(self).is_err() {
                printf(format_args!("wstpad_configure: Initialization failed.\n"));
                return Err(Errno::EINVAL);
            }
            self.flags |= CONFIGURED;
            if let Some(params) = params {
                self.set_params(params)?;
            }
        }
        if is_touchpad(self) {
            self.set_mode(WSMOUSE_COMPAT);
        }

        Ok(())
    }
}

/// `flags |= flag` for a non-zero `val`, `flags &= ~flag` for 0.
fn set_flag(flags: &mut u32, flag: u32, val: i32) {
    if val != 0 {
        *flags |= flag;
    } else {
        *flags &= !flag;
    }
}

/// `wsmouse_get_params`: read parameter values (`EINVAL` or `wstpad_get_param`'s error for
/// an unknown key).
pub fn wsmouse_get_params(sc: &Device, params: &mut [WsmouseParam]) -> Result<(), Errno> {
    sc_of(sc).input().get_params(params)
}

/// `wsmouse_set_params`: set parameter values.
pub fn wsmouse_set_params(sc: &Device, params: &[WsmouseParam]) -> Result<(), Errno> {
    sc_of(sc).input().set_params(params)
}

/// `wsmouse_set_mode`: switch between compatibility mode and native mode; `false` for an
/// unknown mode (the C's -1).
pub fn wsmouse_set_mode(sc: &Device, mode: i32) -> bool {
    sc_of(sc).input().set_mode(mode)
}

/// `wsmouse_get_hw`: the hardware description a driver fills before `wsmouse_configure`.
pub fn wsmouse_get_hw(sc: &Device) -> HwGuard<'_> {
    HwGuard(sc_of(sc).input())
}

/// `wsmouse_configure`: create a default configuration based on the hardware infos in the
/// 'hw' fields. The 'params' argument is optional, hardware drivers can use it to modify
/// the generic defaults. Up to now this function is only useful for touchpads.
pub fn wsmouse_configure(sc: &Device, params: Option<&[WsmouseParam]>) -> Result<(), Errno> {
    sc_of(sc).input().configure(params)
}

/// `wsmouse_input_reset`: clear the input state (the open of the mouse).
pub fn wsmouse_input_reset(input: &mut WsmouseInput) {
    input.btn = BtnState::default();
    input.motion = MotionState::default();
    input.touch = TouchState::default();
    input.touch.min_pressure = input.filter.pressure_hi;
    let num_slots = input.mt.num_slots;
    if num_slots != 0 {
        let (slots, matrix) = (input.mt.slots, input.mt.matrix);
        input.mt = MtState::default();
        input.mt.num_slots = num_slots;
        input.mt.slots = slots;
        input.mt.matrix = matrix;
        input.mt.slots_mut().fill(MtSlot::default());
    }
    if input.tp.is_some() {
        wstpad_reset(input);
    }
}

/// `wsmouse_input_cleanup`: free the input's allocations (the detach of the mouse).
pub fn wsmouse_input_cleanup(input: &mut WsmouseInput) {
    if input.tp.is_some() {
        wstpad_cleanup(input);
    }

    free_mt_slots(input);
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
pub(crate) mod tests {
    // Host tests of the mouse: a fake driver reports through the interface `ums(4)` will use
    // (M16), and the events that reach the queue are checked: buttons, motion and scrolling,
    // the filters (scale with remainder, inversion, swapped axes, reversed scrolling),
    // absolute positions and touches, the queue overflow and `RESYNC`, multitouch pointer
    // control, slot tracking (`wsmouse_mtframe` over `wsmouse_matching`), the parameter ioctls,
    // and a mouse attached by autoconf that joins, leaves and rejoins its mux.

    use core::sync::atomic::{AtomicI32, Ordering};
    use std::boxed::Box;
    use std::vec::Vec;

    use super::*;
    use crate::dev::wscons::wsconsio::{
        WSCONS_EVENT_HSCROLL, WSCONS_EVENT_MOUSE_ABSOLUTE_X, WSCONS_EVENT_MOUSE_ABSOLUTE_Y,
        WSCONS_EVENT_MOUSE_DELTA_W, WSCONS_EVENT_MOUSE_DELTA_X, WSCONS_EVENT_MOUSE_DELTA_Y,
        WSCONS_EVENT_MOUSE_DELTA_Z, WSCONS_EVENT_MOUSE_DOWN, WSCONS_EVENT_MOUSE_UP,
        WSCONS_EVENT_TOUCH_CONTACTS, WSCONS_EVENT_TOUCH_PRESSURE, WSCONS_EVENT_VSCROLL,
        WSMOUSE_TYPE_USB, WSMOUSEIO_GTYPE, WSMUXIO_ADD_DEVICE, WSMUXIO_LIST_DEVICES,
        WSMUXIO_REMOVE_DEVICE, WsmuxDevice, WsmuxDeviceList,
    };
    use crate::dev::wscons::wsmousevar::{WSMOUSEHW_MOUSE, WSMOUSEHW_TOUCHPAD, wsmouse_is_mt_code};
    use crate::dev::wscons::wsmux::wsmux_do_ioctl;
    use crate::kern::init_main::PROC0;
    use crate::kern::subr_autoconf::{config_found, config_init, config_rootfound};
    use crate::kern::subr_pool::tests::setup_real_memory;
    use crate::machine::Machine;
    use crate::sys::device::{Cfdata, DV_DULL, FSTATE_NOTFOUND, FSTATE_STAR};
    use crate::sys::types::makedev;
    use crate::{WSMOUSE_INPUT, WSMOUSE_TOUCH};

    /// How many times the fake driver was enabled minus disabled.
    static ENABLED: AtomicI32 = AtomicI32::new(0);

    fn fake_enable(_v: *mut c_void) -> i32 {
        ENABLED.fetch_add(1, Ordering::Relaxed);
        0
    }

    fn fake_disable(_v: *mut c_void) {
        ENABLED.fetch_sub(1, Ordering::Relaxed);
    }

    /// The driver answers `WSMOUSEIO_GTYPE` (a USB mouse) and nothing else.
    fn fake_ioctl(
        _v: *mut c_void,
        cmd: u64,
        data: &mut [u8],
        _flag: i32,
        _p: Option<&Proc>,
    ) -> Result<bool, Errno> {
        if cmd == WSMOUSEIO_GTYPE {
            ioctl_ret(data, &WSMOUSE_TYPE_USB);
            return Ok(true);
        }
        Ok(false)
    }

    static FAKE_ACCESSOPS: WsmouseAccessops = WsmouseAccessops {
        enable: fake_enable,
        ioctl: fake_ioctl,
        disable: fake_disable,
    };

    /// A mouse over the fake driver, opened directly: its own queue receives the events.
    pub(crate) fn mouse() -> &'static WsmouseSoftc {
        // SAFETY: all-zero is a valid `WsmouseSoftc` (its `Softc` contract); leaked for good.
        let sc: &'static WsmouseSoftc = Box::leak(Box::new(unsafe { core::mem::zeroed() }));
        sc.sc_accessops.set(Some(&FAKE_ACCESSOPS));
        sc.sc_base.me_ops.set(Some(&WSMOUSE_SRCOPS));
        {
            let mut input = sc.input();
            input.evar = Some(NonNull::from(&sc.sc_base.me_evp));
            input.dv = Some(NonNull::from(&sc.sc_base.me_dv));
        }
        wsevent_init(&sc.sc_base.me_evar).unwrap();
        wsmousedoopen(sc, &sc.sc_base.me_evar).unwrap();
        sc
    }

    pub(crate) fn dev(sc: &WsmouseSoftc) -> &Device {
        &sc.sc_base.me_dv
    }

    /// The (type, value) of every queued event, emptying the queue.
    pub(crate) fn drain(sc: &WsmouseSoftc) -> Vec<(u32, i32)> {
        let ev = sc.sc_base.evp().unwrap();
        let mut out = Vec::new();
        while ev.ws_get.get() != ev.ws_put.get() {
            // SAFETY: an open queue and a published slot nobody writes.
            let e = unsafe { ev.q_read(ev.ws_get.get()) };
            out.push((e.type_, e.value));
            ev.ws_get.set((ev.ws_get.get() + 1) % WSEVENT_QSIZE);
        }
        out
    }

    const DOWN: u32 = WSCONS_EVENT_MOUSE_DOWN;
    const UP: u32 = WSCONS_EVENT_MOUSE_UP;
    const DX: u32 = WSCONS_EVENT_MOUSE_DELTA_X;
    const DY: u32 = WSCONS_EVENT_MOUSE_DELTA_Y;
    const DZ: u32 = WSCONS_EVENT_MOUSE_DELTA_Z;
    const DW: u32 = WSCONS_EVENT_MOUSE_DELTA_W;
    const AX: u32 = WSCONS_EVENT_MOUSE_ABSOLUTE_X;
    const AY: u32 = WSCONS_EVENT_MOUSE_ABSOLUTE_Y;
    const PRESSURE: u32 = WSCONS_EVENT_TOUCH_PRESSURE;
    const CONTACTS: u32 = WSCONS_EVENT_TOUCH_CONTACTS;
    const SYNC: u32 = WSCONS_EVENT_SYNC;

    fn set(sc: &WsmouseSoftc, pairs: &[(i32, i32)]) -> Result<(), Errno> {
        let params: Vec<WsmouseParam> = pairs
            .iter()
            .map(|&(key, value)| WsmouseParam { key, value })
            .collect();
        wsmouse_set_params(dev(sc), &params)
    }

    #[test]
    fn buttons_motion_and_wheels_become_one_frame() {
        let _g = setup_real_memory();
        let sc = mouse();
        assert!(ENABLED.load(Ordering::Relaxed) > 0);

        WSMOUSE_INPUT!(dev(sc), 0b101, 3, -2, 1, -1);
        assert_eq!(
            drain(sc),
            [
                (DOWN, 0),
                (DOWN, 2),
                (DX, 3),
                (DY, -2),
                (DZ, 1),
                (DW, -1),
                (SYNC, 0)
            ]
        );
        // One time stamp for the whole frame.
        let ev = sc.sc_base.evp().unwrap();
        // SAFETY: slots 0 and 6 were written by the frame above, and nobody writes them now.
        let (a, b) = unsafe { (ev.q_read(0), ev.q_read(6)) };
        assert_eq!(a.time, b.time);

        WSMOUSE_INPUT!(dev(sc), 0b100, 0, 0, 0, 0);
        assert_eq!(drain(sc), [(UP, 0), (SYNC, 0)]);
        // Nothing changed: no events, not even a SYNC.
        WSMOUSE_INPUT!(dev(sc), 0b100, 0, 0, 0, 0);
        assert!(drain(sc).is_empty());

        // A change that is undone before the sync is no change.
        wsmouse_buttons(dev(sc), 0b110);
        wsmouse_buttons(dev(sc), 0b100);
        wsmouse_input_sync(dev(sc));
        assert!(drain(sc).is_empty());
    }

    #[test]
    fn filters_scale_invert_swap_and_reverse() {
        let _g = setup_real_memory();
        let sc = mouse();

        // Half speed: the remainder carries the half pixel over.
        set(sc, &[(WSMOUSECFG_DX_SCALE, 2048)]).unwrap();
        WSMOUSE_INPUT!(dev(sc), 0, 3, 0, 0, 0);
        WSMOUSE_INPUT!(dev(sc), 0, 3, 0, 0, 0);
        WSMOUSE_INPUT!(dev(sc), 0, -3, 0, 0, 0);
        WSMOUSE_INPUT!(dev(sc), 0, -3, 0, 0, 0);
        assert_eq!(
            drain(sc),
            [
                (DX, 1),
                (SYNC, 0),
                (DX, 2),
                (SYNC, 0),
                (DX, -1),
                (SYNC, 0),
                (DX, -2),
                (SYNC, 0)
            ]
        );
        // 1 * 0.5 with an empty remainder is no event at all.
        WSMOUSE_INPUT!(dev(sc), 0, 1, 0, 0, 0);
        assert!(drain(sc).is_empty());

        set(
            sc,
            &[
                (WSMOUSECFG_DX_SCALE, 0),
                (WSMOUSECFG_X_INV, 1),
                (WSMOUSECFG_SWAPXY, 1),
                (WSMOUSECFG_REVERSE_SCROLLING, 1),
            ],
        )
        .unwrap();
        WSMOUSE_INPUT!(dev(sc), 0, 5, 7, 2, 3);
        assert_eq!(
            drain(sc),
            [(DY, -5), (DX, 7), (DZ, -2), (DW, -3), (SYNC, 0)]
        );

        // A touchpad's wheels are scroll events.
        sc.input().hw.hw_type = WSMOUSEHW_TOUCHPAD;
        set(sc, &[(WSMOUSECFG_REVERSE_SCROLLING, 0)]).unwrap();
        WSMOUSE_INPUT!(dev(sc), 0, 0, 0, 2, 3);
        assert_eq!(
            drain(sc),
            [
                (WSCONS_EVENT_VSCROLL, 2),
                (WSCONS_EVENT_HSCROLL, 3),
                (SYNC, 0)
            ]
        );
    }

    #[test]
    fn absolute_positions_and_touches() {
        let _g = setup_real_memory();
        let sc = mouse();

        WSMOUSE_TOUCH!(dev(sc), 0, 100, 200, 50, 0);
        assert_eq!(
            drain(sc),
            [
                (AX, 100),
                (AY, 200),
                (PRESSURE, 50),
                (CONTACTS, 1),
                (SYNC, 0)
            ]
        );
        // Only Y moved; inverted Y is (inv - y).
        set(sc, &[(WSMOUSECFG_Y_INV, 1000)]).unwrap();
        WSMOUSE_TOUCH!(dev(sc), 0, 100, 250, 50, 0);
        assert_eq!(drain(sc), [(AY, 750), (SYNC, 0)]);

        // The release: pressure 0 drops the (arbitrary) coordinates the driver reports.
        WSMOUSE_TOUCH!(dev(sc), 0, 0, 0, 0, 0);
        assert_eq!(drain(sc), [(PRESSURE, 0), (CONTACTS, 0), (SYNC, 0)]);
        let input = sc.input();
        assert_eq!((input.motion.pos.x, input.motion.pos.y), (100, 250));
    }

    #[test]
    fn pressure_limits_and_hysteresis() {
        let _g = setup_real_memory();
        let sc = mouse();
        let mut input = sc.input();

        // Below the threshold a touch is no touch; a negative pressure is "at the limit".
        input.filter.pressure_lo = 20;
        input.filter.pressure_hi = 30;
        input.touch.min_pressure = 30;
        input.touch(25, 1);
        assert_eq!((input.touch.pressure, input.touch.contacts), (0, 0));
        input.touch(-1, 0);
        assert_eq!((input.touch.pressure, input.touch.contacts), (30, 1));
        // Once touching, the low limit applies until the touch ends.
        wsmouse_touch_update(&mut input);
        assert_eq!(input.touch.min_pressure, 20);
        input.touch.sync = 0;
        input.touch.prev_contacts = 1; // as the sync leaves it
        input.touch(10, 1);
        wsmouse_touch_update(&mut input);
        assert_eq!(input.touch.min_pressure, 30);

        input.filter.h.hysteresis = 4;
        input.filter.v.hysteresis = 4;
        let mut pos = Position {
            acc_dx: 3,
            acc_dy: -3,
            ..Position::default()
        };
        assert!(wsmouse_hysteresis(&input, &pos));
        pos.acc_dy = -4;
        assert!(!wsmouse_hysteresis(&input, &pos));

        // Deltas accumulate while they keep their direction.
        let mut sync = 0;
        set_x(&mut pos, 2, &mut sync, SYNC_X);
        set_x(&mut pos, 2, &mut sync, SYNC_X);
        assert_eq!((pos.x, pos.dx, pos.acc_dx, sync), (2, 2, 5, SYNC_X));
        // A second report before the sync replaces the first one's delta.
        set_x(&mut pos, 4, &mut sync, SYNC_X);
        assert_eq!((pos.x, pos.dx, pos.acc_dx), (4, 4, 7));
        sync = 0;
        set_x(&mut pos, 1, &mut sync, SYNC_X);
        assert_eq!((pos.dx, pos.acc_dx), (-3, -3));
    }

    #[test]
    fn a_full_queue_drops_the_frame_and_resyncs() {
        let _g = setup_real_memory();
        let sc = mouse();
        let ev = sc.sc_base.evp().unwrap();

        // Room for one event: the frame needs four.
        ev.ws_get.set(2);
        ev.ws_put.set(0);
        WSMOUSE_INPUT!(dev(sc), 1, 4, 0, 0, 0);
        wsmouse_position(dev(sc), 10, 10);
        wsmouse_input_sync(dev(sc));
        assert_eq!(ev.ws_put.get(), 0, "nothing published");
        assert_ne!(sc.input().flags & RESYNC, 0);

        // With room again the button and the position come back; the stale delta does not.
        ev.ws_get.set(0);
        wsmouse_input_sync(dev(sc));
        assert_eq!(drain(sc), [(DOWN, 0), (AX, 10), (AY, 10), (SYNC, 0)]);
        assert_eq!(sc.input().flags & RESYNC, 0);
    }

    #[test]
    fn the_pointer_follows_the_moving_touch() {
        let _g = setup_real_memory();
        let sc = mouse();
        let d = dev(sc);
        wsmouse_mt_init(d, 3, false).unwrap();
        assert!(
            wsmouse_mt_init(d, 3, false).is_ok(),
            "same layout: nothing to do"
        );

        wsmouse_mtstate(d, 0, 100, 100, 50);
        wsmouse_input_sync(d);
        assert_eq!(
            drain(sc),
            [
                (AX, 100),
                (AY, 100),
                (PRESSURE, 50),
                (CONTACTS, 1),
                (SYNC, 0)
            ]
        );
        assert_eq!(sc.input().mt.ptr, 0b01);

        // A second touch does not take the pointer.
        wsmouse_mtstate(d, 1, 500, 500, 60);
        wsmouse_input_sync(d);
        assert_eq!(drain(sc), [(CONTACTS, 2), (SYNC, 0)]);

        // Moving only the second touch: after a whole cycle without the first one moving,
        // the pointer jumps to it, without a jump of the deltas.
        for (i, x) in [510, 520, 530].into_iter().enumerate() {
            wsmouse_mtstate(d, 1, x, x, 60);
            wsmouse_input_sync(d);
            let ptr = sc.input().mt.ptr;
            assert_eq!(ptr, if i < 2 { 0b01 } else { 0b10 }, "frame {i}");
        }
        let evs = drain(sc);
        assert_eq!(
            evs[evs.len() - 4..],
            [(AX, 530), (AY, 530), (PRESSURE, 60), (SYNC, 0)]
        );
        assert_eq!(sc.input().motion.pos.dx, 0);

        // Lifting the first touch leaves the second in control.
        wsmouse_mtstate(d, 0, 0, 0, 0);
        wsmouse_input_sync(d);
        assert_eq!(drain(sc), [(CONTACTS, 1), (SYNC, 0)]);
        let input = sc.input();
        assert_eq!(
            (input.mt.touches, input.mt.ptr, input.mt.num_touches),
            (0b10, 0b10, 1)
        );
    }

    #[test]
    fn set_routes_single_values() {
        let _g = setup_real_memory();
        let sc = mouse();
        let d = dev(sc);
        wsmouse_mt_init(d, 2, false).unwrap();
        assert!(wsmouse_is_mt_code(WSMOUSE_MT_PRESSURE));

        wsmouse_set(d, WSMOUSE_ABS_X, 10, 0);
        wsmouse_set(d, WSMOUSE_REL_X, 5, 0);
        wsmouse_set(d, WSMOUSE_ABS_Y, 7, 0);
        wsmouse_set(d, WSMOUSE_MT_PRESSURE, 40, 1);
        wsmouse_set(d, WSMOUSE_MT_ABS_X, 300, 1);
        wsmouse_set(d, WSMOUSE_MT_REL_Y, 20, 1);
        wsmouse_set(d, WSMOUSE_MT_ABS_X, 1, 9); // no such slot
        wsmouse_set(d, WSMOUSE_TOUCH_WIDTH, 3, 0);
        let input = sc.input();
        assert_eq!((input.motion.pos.x, input.motion.pos.y), (15, 7));
        let s = input.mt.slots()[1];
        assert_eq!((s.pos.x, s.pos.y, s.pressure), (300, 20, 40));
        assert_eq!(input.mt.touches, 0b10);
        assert_eq!(input.touch.width, 3);
    }

    #[test]
    fn tracking_ids_and_frames_keep_their_slots() {
        let _g = setup_real_memory();
        let sc = mouse();
        let d = dev(sc);

        assert_eq!(wsmouse_id_to_slot(d, 7), -1, "no slots yet");
        wsmouse_mt_init(d, 2, false).unwrap();
        assert_eq!(wsmouse_id_to_slot(d, 7), 0);
        assert_eq!(wsmouse_id_to_slot(d, 9), 1);
        assert_eq!(wsmouse_id_to_slot(d, 11), -1, "both slots taken this frame");
        wsmouse_mtstate(d, 0, 1, 1, 50);
        wsmouse_input_sync(d);
        assert_eq!(wsmouse_id_to_slot(d, 7), 0, "a known touch");

        // Point-based input with tracking.
        wsmouse_mt_init(d, 3, true).unwrap();
        let mut pts = [
            Mtpoint {
                x: 10,
                y: 10,
                pressure: 50,
                slot: -1,
            },
            Mtpoint {
                x: 100,
                y: 100,
                pressure: 50,
                slot: -1,
            },
        ];
        wsmouse_mtframe(d, &mut pts);
        assert_eq!((pts[0].slot, pts[1].slot), (0, 1));
        wsmouse_input_sync(d);

        // The same touches in the other order, slightly moved.
        let mut pts = [
            Mtpoint {
                x: 102,
                y: 101,
                pressure: 50,
                slot: -1,
            },
            Mtpoint {
                x: 11,
                y: 12,
                pressure: 50,
                slot: -1,
            },
        ];
        wsmouse_mtframe(d, &mut pts);
        assert_eq!((pts[0].slot, pts[1].slot), (1, 0));
        wsmouse_input_sync(d);

        // One touch left: the nearest slot keeps it, the other is released.
        let mut pts = [Mtpoint {
            x: 12,
            y: 12,
            pressure: 50,
            slot: -1,
        }];
        wsmouse_mtframe(d, &mut pts);
        assert_eq!(pts[0].slot, 0);
        wsmouse_input_sync(d);
        assert_eq!(sc.input().mt.touches, 0b001);

        // Beyond the tracking distance a point is a new touch.
        set(sc, &[(WSMOUSECFG_TRKMAXDIST, 5)]).unwrap();
        let mut pts = [Mtpoint {
            x: 200,
            y: 200,
            pressure: 50,
            slot: -1,
        }];
        wsmouse_mtframe(d, &mut pts);
        assert_eq!(pts[0].slot, 1);
        wsmouse_input_sync(d);
        assert_eq!(sc.input().mt.touches, 0b010);

        wsmouse_input_cleanup(&mut sc.input());
        assert!(sc.input().mt.slots.is_none());
    }

    /// A tiny deterministic generator for the matching test.
    fn lcg(seed: &mut u32) -> i32 {
        *seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
        ((*seed >> 16) % 1000) as i32
    }

    /// The minimum of the sum over every assignment of the n columns to distinct rows.
    fn brute_force(matrix: &[i32], m: usize, n: usize) -> i32 {
        fn go(matrix: &[i32], m: usize, n: usize, col: usize, used: &mut [bool]) -> i32 {
            if col == n {
                return 0;
            }
            let mut best = i32::MAX;
            for row in 0..m {
                if !used[row] {
                    used[row] = true;
                    let rest = go(matrix, m, n, col + 1, used);
                    used[row] = false;
                    best = best.min(matrix[row * n + col] + rest);
                }
            }
            best
        }
        go(matrix, m, n, 0, &mut [false; 8])
    }

    #[test]
    fn matching_finds_the_minimum_weight_assignment() {
        let mut seed = 1;
        for (m, n) in [(1, 1), (2, 2), (3, 2), (4, 4), (5, 3), (6, 6), (4, 0)] {
            for _ in 0..30 {
                let matrix: Vec<i32> = (0..m * n).map(|_| lcg(&mut seed)).collect();
                let mut buffer = std::vec![0; 3 * m + 3 * n];
                wsmouse_matching(&matrix, m, n, &mut buffer);
                let r2c = &buffer[..m];
                let mut seen = std::vec![false; n];
                let mut sum = 0;
                for (row, &col) in r2c.iter().enumerate() {
                    if col >= 0 {
                        assert!(!seen[col as usize], "column {col} twice");
                        seen[col as usize] = true;
                        sum += matrix[row * n + col as usize];
                    }
                }
                assert!(seen.iter().all(|&s| s), "every column assigned");
                assert_eq!(sum, brute_force(&matrix, m, n), "{m}x{n} {matrix:?}");
            }
        }
    }

    #[test]
    fn parameter_ioctls_copy_in_and_out() {
        let _g = setup_real_memory();
        let sc = mouse();

        let mut pairs = [
            WsmouseParam {
                key: WSMOUSECFG_DX_SCALE,
                value: 8192,
            },
            WsmouseParam {
                key: WSMOUSECFG_SMOOTHING,
                value: 13,
            },
            WsmouseParam {
                key: WSMOUSECFG_LOG_EVENTS,
                value: 1,
            },
            WsmouseParam {
                key: WSMOUSECFG_PRESSURE_LO,
                value: 40,
            },
        ];
        let mut data = [0u8; size_of::<WsmouseParameters>()];
        let mut args = WsmouseParameters {
            params: pairs.as_mut_ptr() as usize,
            nparams: pairs.len() as u32,
            _pad0: [0; 4],
        };
        ioctl_ret(&mut data, &args);
        wsmouse_do_ioctl(sc, WSMOUSEIO_SETPARAMS, &mut data, FWRITE, None).unwrap();
        {
            let input = sc.input();
            assert_eq!(input.filter.h.scale, 8192);
            assert_eq!(input.filter.mode, 13 & SMOOTHING_MASK);
            assert_ne!(input.flags & LOG_EVENTS, 0);
            assert_eq!(
                (input.filter.pressure_lo, input.filter.pressure_hi),
                (40, 40)
            );
        }

        let mut out = [
            WsmouseParam {
                key: WSMOUSECFG_SMOOTHING,
                value: 0,
            },
            WsmouseParam {
                key: WSMOUSECFG_STRONG_HYSTERESIS,
                value: 9,
            },
            WsmouseParam {
                key: WSMOUSECFG_PRESSURE_HI,
                value: 0,
            },
            WsmouseParam {
                key: WSMOUSECFG_LOG_EVENTS,
                value: 0,
            },
        ];
        args.params = out.as_mut_ptr() as usize;
        ioctl_ret(&mut data, &args);
        wsmouse_do_ioctl(sc, WSMOUSEIO_GETPARAMS, &mut data, FREAD, None).unwrap();
        assert_eq!(
            out.map(|p| p.value),
            [(13 & SMOOTHING_MASK) as i32, 0, 40, 1]
        );

        // An unknown key, too many pairs, no array.
        let mut bad = [WsmouseParam {
            key: 1000,
            value: 0,
        }];
        args.params = bad.as_mut_ptr() as usize;
        args.nparams = 1;
        ioctl_ret(&mut data, &args);
        assert_eq!(
            wsmouse_do_ioctl(sc, WSMOUSEIO_GETPARAMS, &mut data, FREAD, None),
            Err(Errno::EINVAL)
        );
        args.nparams = WSMOUSECFG_MAX + 1;
        ioctl_ret(&mut data, &args);
        assert_eq!(
            wsmouse_do_ioctl(sc, WSMOUSEIO_GETPARAMS, &mut data, FREAD, None),
            Err(Errno::EINVAL)
        );
        args.params = 0;
        args.nparams = 1;
        ioctl_ret(&mut data, &args);
        assert_eq!(
            wsmouse_do_ioctl(sc, WSMOUSEIO_SETPARAMS, &mut data, FWRITE, None),
            Err(Errno::EINVAL)
        );

        // The driver's ioctls, and the rest.
        let mut t = [0u8; 4];
        wsmouse_do_ioctl(sc, WSMOUSEIO_GTYPE, &mut t, FREAD, None).unwrap();
        assert_eq!(ioctl_arg::<u32>(&t), WSMOUSE_TYPE_USB);
        assert_eq!(
            wsmouse_do_ioctl(sc, WSMOUSEIO_GTYPE + 1, &mut t, FREAD, None),
            Err(Errno::ENOTTY)
        );
        assert_eq!(
            wsmouse_do_ioctl(sc, FIOASYNC, &mut t, FREAD, None),
            Err(Errno::EACCES)
        );
        sc.sc_dying.set(1);
        assert_eq!(
            wsmouse_do_ioctl(sc, WSMOUSEIO_GTYPE, &mut t, FREAD, None),
            Err(Errno::EIO)
        );
    }

    #[test]
    fn modes_and_configuration() {
        let _g = setup_real_memory();
        let sc = mouse();
        let d = dev(sc);

        assert!(wsmouse_set_mode(d, WSMOUSE_NATIVE));
        assert_eq!(
            sc.input().flags & (TPAD_NATIVE_MODE | TPAD_COMPAT_MODE),
            TPAD_NATIVE_MODE
        );
        assert!(wsmouse_set_mode(d, WSMOUSE_COMPAT));
        assert_eq!(
            sc.input().flags & (TPAD_NATIVE_MODE | TPAD_COMPAT_MODE),
            TPAD_COMPAT_MODE
        );
        assert!(!wsmouse_set_mode(d, 7));
        sc.input().flags = 0;

        {
            let mut hw = wsmouse_get_hw(d);
            hw.type_ = WSMOUSE_TYPE_USB as i32;
            hw.hw_type = WSMOUSEHW_MOUSE;
            hw.x_max = 1000;
            hw.y_min = 10;
            hw.y_max = 600;
            hw.h_res = 30;
            hw.v_res = 20;
            hw.flags = WSMOUSEHW_LR_DOWN;
        }
        let params = [WsmouseParam {
            key: WSMOUSECFG_DY_SCALE,
            value: 4096,
        }];
        wsmouse_configure(d, Some(&params)).unwrap();
        {
            let input = sc.input();
            assert_eq!(input.filter.v.inv, 610);
            assert_eq!(input.filter.ratio, (1 << 12) * 30 / 20);
            assert_eq!(input.filter.v.scale, 4096);
            assert_ne!(input.flags & CONFIGURED, 0);
            assert_eq!(
                input.flags & TPAD_COMPAT_MODE,
                0,
                "a mouse has no compat mode"
            );
        }
        // Configured once: a second call changes nothing.
        sc.input().filter.v.inv = 0;
        wsmouse_configure(d, None).unwrap();
        assert_eq!(sc.input().filter.v.inv, 0);
    }

    /// The guard marks the input busy while it lives (a second `input()` would panic, which
    /// on the host ends the test process: `boot(RB_HALT)`).
    #[test]
    fn the_input_guard_marks_the_input_busy() {
        let _g = setup_real_memory();
        let sc = mouse();
        let a = sc.input();
        assert!(sc.input_busy.get());
        drop(a);
        assert!(!sc.input_busy.get());
        let hw = wsmouse_get_hw(dev(sc));
        assert!(sc.input_busy.get());
        drop(hw);
        assert!(!sc.input_busy.get());
    }

    /// The root of the test `ioconf`: a fake mouse driver that attaches one wsmouse.
    fn tms_match(_parent: Option<&Device>, _m: &CfMatch, _aux: *mut c_void) -> i32 {
        1
    }

    fn tms_attach(_parent: Option<&Device>, self_: &Device, _aux: *mut c_void) {
        let mut a = WsmousedevAttachArgs {
            accessops: &FAKE_ACCESSOPS,
            accesscookie: ptr::null_mut(),
        };
        let child = config_found(self_, ptr::from_mut(&mut a).cast(), Some(wsmousedevprint));
        assert!(child.is_some());
    }

    static TMS_CA: Cfattach = Cfattach {
        ca_devsize: size_of::<Device>(),
        ca_match: Some(tms_match),
        ca_attach: tms_attach,
        ca_detach: None,
        ca_activate: None,
    };
    static TMS_CD: Cfdriver = Cfdriver::new(b"tms", DV_DULL, 0);

    /// `tms0 at root`, `wsmouse* at tms? mux 20`.
    static IOCONF: [Cfdata; 2] = [
        Cfdata::new(&TMS_CA, &TMS_CD, 0, FSTATE_NOTFOUND, &[], 0, &[], 0, 0),
        Cfdata::new(
            &WSMOUSE_CA,
            &WSMOUSE_CD,
            0,
            FSTATE_STAR,
            &[20],
            0,
            &[0],
            0,
            0,
        ),
    ];

    /// A mux's device list.
    fn list(mux: &WsmuxSoftc) -> Vec<(i32, i32)> {
        let mut data = [0u8; size_of::<WsmuxDeviceList>()];
        wsmux_do_ioctl(
            &mux.sc_base.me_dv,
            WSMUXIO_LIST_DEVICES,
            &mut data,
            FREAD,
            None,
        )
        .unwrap();
        let l: WsmuxDeviceList = ioctl_arg(&data);
        l.devices[..l.ndevices as usize]
            .iter()
            .map(|d| (d.type_, d.idx))
            .collect()
    }

    fn mux_ioctl(mux: &WsmuxSoftc, cmd: u64, idx: i32) -> Result<(), Errno> {
        let mut data = [0u8; size_of::<WsmuxDevice>()];
        ioctl_ret(
            &mut data,
            &WsmuxDevice {
                type_: WSMUX_MOUSE,
                idx,
            },
        );
        wsmux_do_ioctl(&mux.sc_base.me_dv, cmd, &mut data, FWRITE, None)
    }

    #[test]
    fn an_attached_mouse_feeds_its_mux() {
        let _g = setup_real_memory();
        // SAFETY: `setup_real_memory`'s lock serialises the tests that install an ioconf.
        unsafe { Machine::set_ioconf(&IOCONF, &[0]) };
        config_init();
        assert!(config_rootfound(b"tms", ptr::null_mut()).is_some());
        let unit = (0..WSMOUSE_CD.cd_ndevs.get())
            .rev()
            .find(|&u| WSMOUSE_CD.cd_dev(u).is_some())
            .unwrap();
        let sc = wsmouse_sc(unit).unwrap();
        let mux = wsmux_getmux(20).unwrap();
        assert_eq!(list(mux), [(WSMUX_MOUSE, unit)]);
        assert!(sc.sc_base.parent().is_some_and(|p| ptr::eq(p, mux)));

        // An open mux receives the mouse's events.
        let evar = &mux.sc_base.me_evar;
        wsevent_init(evar).unwrap();
        crate::dev::wscons::wsmux::wsmux_do_open(mux, evar).unwrap();
        WSMOUSE_INPUT!(dev(sc), 1, 0, 2, 0, 0);
        let mut got = Vec::new();
        while evar.ws_get.get() != evar.ws_put.get() {
            // SAFETY: published slots of the open queue.
            let e = unsafe { evar.q_read(evar.ws_get.get()) };
            got.push((e.type_, e.value));
            evar.ws_get.set(evar.ws_get.get() + 1);
        }
        assert_eq!(got, [(DOWN, 0), (DY, 2), (SYNC, 0)]);
        crate::dev::wscons::wsmux::wsmux_do_close(mux);
        mux.sc_base.me_evp.set(None);
        wsevent_fini(evar);

        // Out of the mux, back in through WSMUXIO_ADD_DEVICE (wsmouse_add_mux).
        mux_ioctl(mux, WSMUXIO_REMOVE_DEVICE, unit).unwrap();
        assert!(list(mux).is_empty());
        mux_ioctl(mux, WSMUXIO_ADD_DEVICE, unit).unwrap();
        assert_eq!(list(mux), [(WSMUX_MOUSE, unit)]);
        assert_eq!(mux_ioctl(mux, WSMUXIO_ADD_DEVICE, unit), Err(Errno::EBUSY));
        assert_eq!(
            mux_ioctl(mux, WSMUXIO_ADD_DEVICE, unit + 1),
            Err(Errno::ENXIO)
        );

        // Opening /dev/wsmouseN takes the mouse out of its mux; closing puts it back.
        let d = makedev(68, unit as u32);
        let before = ENABLED.load(Ordering::Relaxed);
        wsmouseopen(d, FREAD, 0, &PROC0).unwrap();
        assert!(list(mux).is_empty());
        assert_eq!(wsmouseopen(d, FREAD, 0, &PROC0), Err(Errno::EBUSY));
        assert_eq!(ENABLED.load(Ordering::Relaxed), before + 1);
        let mut t = [0u8; 4];
        wsmouseioctl(d, WSMOUSEIO_GTYPE, &mut t, FREAD, &PROC0).unwrap();
        wsmouseclose(d, FREAD, 0, None).unwrap();
        assert_eq!(ENABLED.load(Ordering::Relaxed), before);
        assert_eq!(list(mux), [(WSMUX_MOUSE, unit)]);
        assert_eq!(
            wsmouseopen(makedev(68, 99), FREAD, 0, &PROC0),
            Err(Errno::ENXIO)
        );
    }
}
/* </TESTS> */
