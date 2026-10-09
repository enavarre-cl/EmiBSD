/* <LICENSES> */
/*
 * Copyright (c) 1998
 *	Matthias Drochner.  All rights reserved.
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
 *
 */
/* </LICENSES> */

/* <CODE> */
//! `wsdisplay(4)`'s USL compatibility (`option WSDISPLAY_COMPAT_USL`): the System V
//! virtual terminal ioctls (`VT_*`) and the PC console's keyboard and mode ioctls (`KD*`),
//! for X servers and other programs written for them. A process that takes a screen with
//! `VT_SETMODE`/`VT_PROCESS` is signalled before the screen is switched away from it and
//! back to it, and answers with `VT_RELDISP`; the screen switch waits for the answer (at
//! most `WSCOMPAT_USL_SYNCTIMEOUT` seconds) through the `wscons_syncops` hooks of
//! `wsdisplay.c`.
//!
//! Upstream: sys/dev/wscons/wsdisplay_compat_usl.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `struct usl_syncdata` is `malloc`ed as in C and reached through the syncops' `void *`
//!   cookie; its members that change are `Cell`s. `s_process` is compared, never
//!   dereferenced, except to signal it while `prfind` still finds it.
//! - The ioctls' `int` results are `Result<bool, Errno>`, `Ok(false)` for the C's -1 ("not
//!   mine"); `usl_sync_check` returns a `bool`.
//! - `*(long *)data`, the argument of the `_IO` commands (`VT_ACTIVATE`, `KDSETMODE`, ...),
//!   is the 8-byte value `sys_ioctl` stores in the argument buffer for `IOC_VOID`.
//! - `DPRINTF` (`WSDISPLAY_DEBUG`) is not configured; its messages are comments.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::{self, NonNull};

use crate::dev::wscons::wsconsio::{
    WSDISPLAYIO_MODE_EMUL, WSDISPLAYIO_MODE_MAPPED, WSDISPLAYIO_SMODE, WSKBD_BELL_DOPERIOD,
    WSKBD_BELL_DOPITCH, WSKBD_LED_CAPS, WSKBD_LED_NUM, WSKBD_LED_SCROLL, WSKBD_RAW,
    WSKBD_TRANSLATED, WSKBDIO_COMPLEXBELL, WSKBDIO_GETLEDS, WSKBDIO_GETMODE, WSKBDIO_SETLEDS,
    WSKBDIO_SETMODE, WskbdBellData,
};
use crate::dev::wscons::wsdisplay::{
    WsdisplaySoftc, Wsscreen, wsdisplay_getactivescreen, wsdisplay_internal_ioctl,
    wsdisplay_maxscreenidx, wsdisplay_screenstate, wsdisplay_switch, wsscreen_attach_sync,
    wsscreen_detach_sync, wsscreen_lookup_sync, wsscreen_switchwait,
};
use crate::dev::wscons::wsdisplay_usl_io::{
    K_RAW, K_XLATE, KD_GRAPHICS, KD_TEXT, KDDISABIO, KDENABIO, KDGETLED, KDGKBMODE, KDMKTONE,
    KDSETLED, KDSETMODE, KDSETRAD, KDSKBMODE, LED_CAP, LED_NUM, LED_SCR, VT_ACKACQ, VT_ACTIVATE,
    VT_AUTO, VT_FALSE, VT_GETACTIVE, VT_GETMODE, VT_GETSTATE, VT_OPENQRY, VT_PROCESS, VT_RELDISP,
    VT_SETMODE, VT_TRUE, VT_WAITACTIVE, VtMode, VtStat,
};
use crate::dev::wscons::wsdisplayvar::{ShowScreenCb, WsconsSyncops};
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_proc::prfind;
use crate::kern::kern_sig::prsignal;
use crate::kern::kern_timeout::{timeout_add_sec, timeout_del, timeout_set};
use crate::sys::errno::Errno;
use crate::sys::fcntl::FWRITE;
use crate::sys::ioctl::{ioctl_arg, ioctl_ret};
use crate::sys::malloc::{M_DEVBUF, M_NOWAIT};
use crate::sys::proc::{Proc, Process};
use crate::sys::signal::NSIG;
use crate::sys::timeout::Timeout;
use crate::sys::types::Pid;

/// `SF_DETACHPENDING`.
const SF_DETACHPENDING: i32 = 1;
/// `SF_ATTACHPENDING`.
const SF_ATTACHPENDING: i32 = 2;

/// `WSCOMPAT_USL_SYNCTIMEOUT`: seconds.
const WSCOMPAT_USL_SYNCTIMEOUT: i32 = 5;

/// `PCVT_SYSBEEPF`: the PC speaker's timer frequency, for `KDMKTONE`'s pitch.
const PCVT_SYSBEEPF: i32 = 1_193_182;

/// `struct usl_syncdata`: the process that owns a screen through `VT_PROCESS`.
struct UslSyncdata {
    /// `s_scr`.
    s_scr: NonNull<Wsscreen>,
    /// `s_process`.
    s_process: *const Process,
    /// `s_pid`.
    s_pid: Pid,
    /// `s_flags`: `SF_*`.
    s_flags: Cell<i32>,
    /// `s_acqsig`.
    s_acqsig: i32,
    /// `s_relsig`.
    s_relsig: i32,
    /// `s_frsig`: unused.
    s_frsig: i32,
    /// `s_callback`.
    s_callback: Cell<Option<ShowScreenCb>>,
    /// `s_cbarg`.
    s_cbarg: Cell<*mut c_void>,
    /// `s_attach_ch`.
    s_attach_ch: Timeout,
    /// `s_detach_ch`.
    s_detach_ch: Timeout,
}

/// `usl_syncops`.
static USL_SYNCOPS: WsconsSyncops = WsconsSyncops {
    detach: usl_detachproc,
    attach: usl_attachproc,
    check: usl_sync_check_op,
    destroy: usl_sync_destroy_op,
};

/// `wscompat_usl_synctimeout`.
static WSCOMPAT_USL_SYNCTIMEOUT_SECS: i32 = WSCOMPAT_USL_SYNCTIMEOUT;

/// `usl_sync_init`: make `pr` the owner of screen `scr`, with its acquire, release and
/// (unused) free signals.
fn usl_sync_init(
    scr: &Wsscreen,
    pr: &Process,
    acqsig: i32,
    relsig: i32,
    frsig: i32,
) -> Result<NonNull<UslSyncdata>, Errno> {
    if acqsig <= 0 || acqsig >= NSIG || relsig <= 0 || relsig >= NSIG || frsig <= 0 || frsig >= NSIG
    {
        return Err(Errno::EINVAL);
    }
    let sdp = malloc(size_of::<UslSyncdata>(), M_DEVBUF, M_NOWAIT)
        .ok_or(Errno::ENOMEM)?
        .cast::<UslSyncdata>();
    // SAFETY: a fresh allocation of one `UslSyncdata` (malloc's chunks are aligned to their
    // power-of-two size), written before any use.
    unsafe {
        ptr::write(
            sdp.as_ptr(),
            UslSyncdata {
                s_scr: NonNull::from(scr),
                s_process: pr,
                s_pid: pr.ps_pid.get(),
                s_flags: Cell::new(0),
                s_acqsig: acqsig,
                s_relsig: relsig,
                s_frsig: frsig,
                s_callback: Cell::new(None),
                s_cbarg: Cell::new(ptr::null_mut()),
                s_attach_ch: Timeout::zeroed(),
                s_detach_ch: Timeout::zeroed(),
            },
        )
    };
    // SAFETY: just written, ours until registered.
    let sd = unsafe { sdp.as_ref() };
    timeout_set(&sd.s_attach_ch, usl_attachtimeout, sdp.as_ptr().cast());
    timeout_set(&sd.s_detach_ch, usl_detachtimeout, sdp.as_ptr().cast());
    if let Err(e) = wsscreen_attach_sync(scr, &USL_SYNCOPS, sdp.as_ptr().cast()) {
        free(sdp.cast(), M_DEVBUF, size_of::<UslSyncdata>());
        return Err(e);
    }
    Ok(sdp)
}

/// `usl_sync_done`: the owner is gone: fail or finish a pending switch, release the screen
/// and free the data.
///
/// # Safety
///
/// `sdp` is a live `UslSyncdata` registered with its screen; nothing uses it afterwards.
unsafe fn usl_sync_done(sdp: NonNull<UslSyncdata>) {
    // SAFETY: the caller's contract.
    let sd = unsafe { sdp.as_ref() };
    if sd.s_flags.get() & SF_DETACHPENDING != 0 {
        timeout_del(&sd.s_detach_ch);
        if let Some(cb) = sd.s_callback.get() {
            cb(sd.s_cbarg.get(), 0, 0);
        }
    }
    if sd.s_flags.get() & SF_ATTACHPENDING != 0 {
        timeout_del(&sd.s_attach_ch);
        if let Some(cb) = sd.s_callback.get() {
            cb(sd.s_cbarg.get(), Errno::ENXIO as i32, 0);
        }
    }
    // SAFETY: the screen outlives its sync data (`wsdisplay_delscreen` refuses a screen
    // with syncops; its last close destroys them first).
    let _ = wsscreen_detach_sync(unsafe { sd.s_scr.as_ref() });
    free(sdp.cast(), M_DEVBUF, size_of::<UslSyncdata>());
}

/// `usl_sync_check`: whether the owner still lives; if not, [`usl_sync_done`].
///
/// # Safety
///
/// As for [`usl_sync_done`], except that the caller may use `sdp` again when this returns
/// `true`.
unsafe fn usl_sync_check(sdp: NonNull<UslSyncdata>) -> bool {
    // SAFETY: the caller's contract.
    let sd = unsafe { sdp.as_ref() };
    if prfind(sd.s_pid).is_some_and(|pr| ptr::eq(pr, sd.s_process)) {
        return true;
    }
    // usl_sync_check: process %d died
    // SAFETY: the caller's contract; `sd` is not used again.
    unsafe { usl_sync_done(sdp) };
    false
}

/// `_usl_sync_check`: [`usl_sync_check`] as the syncops' `check`.
///
/// # Safety
///
/// `cookie` is the `UslSyncdata` registered with these syncops.
unsafe fn usl_sync_check_op(cookie: *mut c_void) -> i32 {
    let Some(sdp) = NonNull::new(cookie.cast::<UslSyncdata>()) else {
        return 0;
    };
    // SAFETY: the caller's contract.
    i32::from(unsafe { usl_sync_check(sdp) })
}

/// `_usl_sync_destroy`: [`usl_sync_done`] as the syncops' `destroy`.
///
/// # Safety
///
/// As for [`usl_sync_check_op`].
unsafe fn usl_sync_destroy_op(cookie: *mut c_void) {
    if let Some(sdp) = NonNull::new(cookie.cast::<UslSyncdata>()) {
        // SAFETY: the caller's contract.
        unsafe { usl_sync_done(sdp) };
    }
}

/// `usl_sync_get`: the owner data of screen `scr`, if a process owns it.
fn usl_sync_get(scr: &Wsscreen) -> Option<NonNull<UslSyncdata>> {
    let cookie = wsscreen_lookup_sync(scr, &USL_SYNCOPS).ok()?;
    NonNull::new(cookie.cast::<UslSyncdata>())
}

/// `usl_detachproc`: ask the owner to release the screen (its release signal);
/// `EAGAIN`: the switch completes through `callback` when it answers or times out.
///
/// # Safety
///
/// As for [`usl_sync_check_op`].
unsafe fn usl_detachproc(
    cookie: *mut c_void,
    _waitok: i32,
    callback: Option<ShowScreenCb>,
    cbarg: *mut c_void,
) -> i32 {
    let Some(sdp) = NonNull::new(cookie.cast::<UslSyncdata>()) else {
        return 0;
    };

    // SAFETY: the caller's contract.
    if !unsafe { usl_sync_check(sdp) } {
        return 0;
    }
    // SAFETY: still registered (the check passed).
    let sd = unsafe { sdp.as_ref() };

    // we really need a callback
    if callback.is_none() {
        return Errno::EINVAL as i32;
    }

    // Normally, this is called from the controlling process. It is supposed to reply with a
    // VT_RELDISP ioctl(), so it is not useful to tsleep() here.
    sd.s_callback.set(callback);
    sd.s_cbarg.set(cbarg);
    sd.s_flags.set(sd.s_flags.get() | SF_DETACHPENDING);
    // SAFETY: `prfind` just found the owner, so it lives.
    prsignal(unsafe { &*sd.s_process }, sd.s_relsig);
    timeout_add_sec(&sd.s_detach_ch, WSCOMPAT_USL_SYNCTIMEOUT_SECS);

    Errno::EAGAIN as i32
}

/// `usl_detachack`: the owner's `VT_RELDISP` answer to a release request.
fn usl_detachack(sd: &UslSyncdata, ack: bool) -> Result<(), Errno> {
    if sd.s_flags.get() & SF_DETACHPENDING == 0 {
        // usl_detachack: not detaching
        return Err(Errno::EINVAL);
    }

    timeout_del(&sd.s_detach_ch);
    sd.s_flags.set(sd.s_flags.get() & !SF_DETACHPENDING);

    if let Some(cb) = sd.s_callback.get() {
        cb(sd.s_cbarg.get(), if ack { 0 } else { Errno::EIO as i32 }, 1);
    }

    Ok(())
}

/// `usl_detachtimeout`: the owner did not answer the release request in time.
fn usl_detachtimeout(arg: *mut c_void) {
    let Some(sdp) = NonNull::new(arg.cast::<UslSyncdata>()) else {
        return;
    };
    // SAFETY: the timeout is deleted before the data is freed (`usl_sync_done`).
    let sd = unsafe { sdp.as_ref() };

    // usl_detachtimeout

    if sd.s_flags.get() & SF_DETACHPENDING == 0 {
        // usl_detachtimeout: not detaching
        return;
    }

    sd.s_flags.set(sd.s_flags.get() & !SF_DETACHPENDING);

    if let Some(cb) = sd.s_callback.get() {
        cb(sd.s_cbarg.get(), Errno::EIO as i32, 0);
    }

    // SAFETY: still registered; not used again here.
    let _ = unsafe { usl_sync_check(sdp) };
}

/// `usl_attachproc`: tell the owner it gets the screen back (its acquire signal);
/// `EAGAIN`: the switch completes through `callback` when it acknowledges or times out.
///
/// # Safety
///
/// As for [`usl_sync_check_op`].
unsafe fn usl_attachproc(
    cookie: *mut c_void,
    _waitok: i32,
    callback: Option<ShowScreenCb>,
    cbarg: *mut c_void,
) -> i32 {
    let Some(sdp) = NonNull::new(cookie.cast::<UslSyncdata>()) else {
        return 0;
    };

    // SAFETY: the caller's contract.
    if !unsafe { usl_sync_check(sdp) } {
        return 0;
    }
    // SAFETY: still registered (the check passed).
    let sd = unsafe { sdp.as_ref() };

    // we really need a callback
    if callback.is_none() {
        return Errno::EINVAL as i32;
    }

    sd.s_callback.set(callback);
    sd.s_cbarg.set(cbarg);
    sd.s_flags.set(sd.s_flags.get() | SF_ATTACHPENDING);
    // SAFETY: `prfind` just found the owner, so it lives.
    prsignal(unsafe { &*sd.s_process }, sd.s_acqsig);
    timeout_add_sec(&sd.s_attach_ch, WSCOMPAT_USL_SYNCTIMEOUT_SECS);

    Errno::EAGAIN as i32
}

/// `usl_attachack`: the owner's `VT_ACKACQ`.
fn usl_attachack(sd: &UslSyncdata, ack: bool) -> Result<(), Errno> {
    if sd.s_flags.get() & SF_ATTACHPENDING == 0 {
        // usl_attachack: not attaching
        return Err(Errno::EINVAL);
    }

    timeout_del(&sd.s_attach_ch);
    sd.s_flags.set(sd.s_flags.get() & !SF_ATTACHPENDING);

    if let Some(cb) = sd.s_callback.get() {
        cb(sd.s_cbarg.get(), if ack { 0 } else { Errno::EIO as i32 }, 1);
    }

    Ok(())
}

/// `usl_attachtimeout`: the owner did not acknowledge in time.
fn usl_attachtimeout(arg: *mut c_void) {
    let Some(sdp) = NonNull::new(arg.cast::<UslSyncdata>()) else {
        return;
    };
    // SAFETY: as in `usl_detachtimeout`.
    let sd = unsafe { sdp.as_ref() };

    // usl_attachtimeout

    if sd.s_flags.get() & SF_ATTACHPENDING == 0 {
        // usl_attachtimeout: not attaching
        return;
    }

    sd.s_flags.set(sd.s_flags.get() & !SF_ATTACHPENDING);

    if let Some(cb) = sd.s_callback.get() {
        cb(sd.s_cbarg.get(), Errno::EIO as i32, 0);
    }

    // SAFETY: still registered; not used again here.
    let _ = unsafe { usl_sync_check(sdp) };
}

/// `*(long *)data`: the argument of an `_IO` command.
fn long_arg(data: &[u8]) -> i64 {
    let mut b = [0u8; 8];
    let n = data.len().min(8);
    b[..n].copy_from_slice(&data[..n]);
    i64::from_ne_bytes(b)
}

/// `wsdisplay_usl_ioctl1`: the USL ioctls of the display, before the screen's (`Ok(false)`:
/// not one of them).
pub fn wsdisplay_usl_ioctl1(
    sc: &WsdisplaySoftc,
    cmd: u64,
    data: &mut [u8],
    flag: i32,
    _p: &Proc,
) -> Result<bool, Errno> {
    match cmd {
        VT_OPENQRY => {
            let maxidx = wsdisplay_maxscreenidx(sc);
            for idx in 0..=maxidx {
                if wsdisplay_screenstate(sc, idx).is_ok() {
                    ioctl_ret(data, &(idx + 1));
                    return Ok(true);
                }
            }
            Err(Errno::ENXIO)
        }
        VT_GETACTIVE => {
            let idx = wsdisplay_getactivescreen(sc);
            ioctl_ret(data, &(idx + 1));
            Ok(true)
        }
        VT_ACTIVATE => {
            if flag & FWRITE == 0 {
                return Err(Errno::EACCES);
            }
            let idx = long_arg(data) - 1;
            if idx < 0 {
                return Err(Errno::EINVAL);
            }
            wsdisplay_switch(&sc.sc_dv, idx as i32, 1).map(|()| true)
        }
        VT_WAITACTIVE => {
            if flag & FWRITE == 0 {
                return Err(Errno::EACCES);
            }
            let idx = long_arg(data) - 1;
            if idx < 0 {
                return Err(Errno::EINVAL);
            }
            wsscreen_switchwait(sc, idx as i32).map(|()| true)
        }
        VT_GETSTATE => {
            let mut ss = VtStat::default();
            let idx = wsdisplay_getactivescreen(sc);
            ss.v_active = (idx + 1) as u16;
            ss.v_state = 0;
            let maxidx = wsdisplay_maxscreenidx(sc);
            for idx in 0..=maxidx {
                if wsdisplay_screenstate(sc, idx) == Err(Errno::EBUSY) {
                    ss.v_state |= (1u32 << (idx + 1)) as u16;
                }
            }
            ioctl_ret(data, &ss);
            Ok(true)
        }

        _ => Ok(false),
    }
}

/// `wsdisplay_usl_ioctl2`: the USL ioctls of a screen, some turned into wsdisplay and wskbd
/// ioctls (`Ok(false)`: not one of them).
pub fn wsdisplay_usl_ioctl2(
    sc: &WsdisplaySoftc,
    scr: &Wsscreen,
    cmd: u64,
    data: &mut [u8],
    flag: i32,
    p: &Proc,
) -> Result<bool, Errno> {
    let mut intarg: i32 = 0;
    let mut bd = WskbdBellData::default();
    let rawkbd = cfg!(feature = "wsdisplay_compat_rawkbd");

    // the commands converted to wsdisplay ioctls, with their argument
    let req = match cmd {
        VT_SETMODE => {
            if flag & FWRITE == 0 {
                return Err(Errno::EACCES);
            }
            let newmode: VtMode = ioctl_arg(data);
            if newmode.mode == VT_PROCESS {
                usl_sync_init(
                    scr,
                    p.process(),
                    i32::from(newmode.acqsig),
                    i32::from(newmode.relsig),
                    i32::from(newmode.frsig),
                )?;
            } else if let Some(sd) = usl_sync_get(scr) {
                // SAFETY: the screen's registered sync data, freed here and not used again.
                unsafe { usl_sync_done(sd) };
            }
            return Ok(true);
        }
        VT_GETMODE => {
            let mut cmode: VtMode = ioctl_arg(data);
            if let Some(sdp) = usl_sync_get(scr) {
                // SAFETY: the screen's registered sync data.
                let sd = unsafe { sdp.as_ref() };
                cmode.mode = VT_PROCESS;
                cmode.relsig = sd.s_relsig as i16;
                cmode.acqsig = sd.s_acqsig as i16;
                cmode.frsig = sd.s_frsig as i16;
            } else {
                cmode.mode = VT_AUTO;
            }
            ioctl_ret(data, &cmode);
            return Ok(true);
        }
        VT_RELDISP => {
            if flag & FWRITE == 0 {
                return Err(Errno::EACCES);
            }
            let d = long_arg(data) as i32;
            let Some(sdp) = usl_sync_get(scr) else {
                return Err(Errno::EINVAL);
            };
            // SAFETY: the screen's registered sync data.
            let sd = unsafe { sdp.as_ref() };
            return match d {
                VT_FALSE | VT_TRUE => usl_detachack(sd, d == VT_TRUE).map(|()| true),
                VT_ACKACQ => usl_attachack(sd, true).map(|()| true),
                _ => Err(Errno::EINVAL),
            };
        }

        KDENABIO | KDDISABIO => {
            if flag & FWRITE == 0 {
                return Err(Errno::EACCES);
            }
            // This is a lie, but non-x86 platforms are not supposed to issue these ioctls
            // anyway.
            return Ok(true);
        }

        KDSETRAD => {
            if flag & FWRITE == 0 {
                return Err(Errno::EACCES);
            }
            // XXX ignore for now
            return Ok(true);
        }

        // the following are converted to wsdisplay ioctls
        KDSETMODE => {
            if flag & FWRITE == 0 {
                return Err(Errno::EACCES);
            }
            intarg = match long_arg(data) as i32 {
                KD_GRAPHICS => WSDISPLAYIO_MODE_MAPPED as i32,
                KD_TEXT => WSDISPLAYIO_MODE_EMUL as i32,
                _ => return Err(Errno::EINVAL),
            };
            WSDISPLAYIO_SMODE
        }
        KDMKTONE => {
            if flag & FWRITE == 0 {
                return Err(Errno::EACCES);
            }
            let d = long_arg(data) as i32;
            if d != 0 {
                if d >> 16 != 0 {
                    bd.which = WSKBD_BELL_DOPERIOD;
                    bd.period = (d >> 16) as u32; // ms
                } else {
                    bd.which = 0;
                }
                if d & 0xffff != 0 {
                    bd.which |= WSKBD_BELL_DOPITCH;
                    bd.pitch = (PCVT_SYSBEEPF / (d & 0xffff)) as u32; // Hz
                }
            } else {
                bd.which = 0; // default
            }
            WSKBDIO_COMPLEXBELL
        }
        KDSETLED => {
            if flag & FWRITE == 0 {
                return Err(Errno::EACCES);
            }
            intarg = 0;
            let d = long_arg(data) as i32;
            if d & LED_CAP != 0 {
                intarg |= WSKBD_LED_CAPS;
            }
            if d & LED_NUM != 0 {
                intarg |= WSKBD_LED_NUM;
            }
            if d & LED_SCR != 0 {
                intarg |= WSKBD_LED_SCROLL;
            }
            WSKBDIO_SETLEDS
        }
        KDGETLED => WSKBDIO_GETLEDS,
        KDSKBMODE if rawkbd => {
            if flag & FWRITE == 0 {
                return Err(Errno::EACCES);
            }
            intarg = match long_arg(data) as i32 {
                K_RAW => WSKBD_RAW,
                K_XLATE => WSKBD_TRANSLATED,
                _ => return Err(Errno::EINVAL),
            };
            WSKBDIO_SETMODE
        }
        KDGKBMODE if rawkbd => WSKBDIO_GETMODE,

        _ => return Ok(false),
    };

    let res = if req == WSKBDIO_COMPLEXBELL {
        let mut arg = [0u8; size_of::<WskbdBellData>()];
        ioctl_ret(&mut arg, &bd);
        wsdisplay_internal_ioctl(sc, scr, req, &mut arg, flag, Some(p))
    } else {
        let mut arg = intarg.to_ne_bytes();
        let r = wsdisplay_internal_ioctl(sc, scr, req, &mut arg, flag, Some(p));
        intarg = i32::from_ne_bytes(arg);
        r
    };
    if res != Ok(true) {
        return res;
    }

    match cmd {
        KDGETLED => {
            let mut d = 0;
            if intarg & WSKBD_LED_CAPS != 0 {
                d |= LED_CAP;
            }
            if intarg & WSKBD_LED_NUM != 0 {
                d |= LED_NUM;
            }
            if intarg & WSKBD_LED_SCROLL != 0 {
                d |= LED_SCR;
            }
            ioctl_ret(data, &d);
        }
        KDGKBMODE if rawkbd => {
            let mode = if intarg == WSKBD_RAW { K_RAW } else { K_XLATE };
            ioctl_ret(data, &mode);
        }
        _ => {}
    }

    Ok(true)
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_argument_of_io_commands() {
        assert_eq!(long_arg(&3i64.to_ne_bytes()), 3);
        assert_eq!(long_arg(&(-1i64).to_ne_bytes()), -1);
        assert_eq!(long_arg(&[]), 0);
    }

    #[test]
    fn sync_timeout_is_the_c_default() {
        assert_eq!(WSCOMPAT_USL_SYNCTIMEOUT_SECS, 5);
    }
}
/* </TESTS> */
