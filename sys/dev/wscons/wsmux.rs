/*	$OpenBSD: wsmux.c,v 1.62 2025/07/18 17:34:29 mvs Exp $	*/
/*      $NetBSD: wsmux.c,v 1.37 2005/04/30 03:47:12 augustss Exp $      */
/* <LICENSES> */
/*
 * Copyright (c) 1998, 2005 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * Author: Lennart Augustsson <augustss@carlstedt.se>
 *         Carlstedt Research & Technology
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
//! wscons mux device: a collection of real mice and keyboards that acts as a merge point
//! for all the events from the different real devices.
//!
//! Upstream: sys/dev/wscons/wsmux.c @ 3ce1f3f79392
//!
//! The wsmux pseudo device is used to multiplex events from several wsmouse, wskbd, and/or
//! wsmux devices together. The devices connected together form a tree with muxes in the
//! interior and real devices (mouse and kbd) at the leaves. The special case of a tree with
//! one node (mux or other) is supported as well. Only the device at the root of the tree can
//! be opened (if a non-root device is opened the subtree rooted at that point is severed from
//! the containing tree). When the root is opened it allocates a wseventvar struct which all
//! the nodes in the tree will send their events too. An ioctl() performed on the root is
//! propagated to all the nodes. There are also ioctl() operations to add and remove nodes
//! from a tree.
//!
//! A mux is also the input of a display: `wsdisplay(4)` attaches to the mux its `mux`
//! locator names (`wsmux_getmux`) and volunteers as its display (`wsmux_set_display`); a
//! keyboard that joins that mux (`wskbd* at ukbd? mux 1`) is then connected to the display
//! and types into its screens. `/dev/wsmouse` and `/dev/wskbd` are muxes 0 and 1 (character
//! major 69).
//!
//! ## Deviations
//! - The kernel configuration is GENERIC's (`pseudo-device wsmux 2`, `NWSDISPLAY > 0`,
//!   `NWSKBD > 0`, option `WSDISPLAY_COMPAT_RAWKBD` as the cargo feature
//!   `wsdisplay_compat_rawkbd`) and `NWSMOUSE > 0` (`WSMUXIO_ADD_DEVICE` of a
//!   `WSMUX_MOUSE` is `wsmouse_add_mux`).
//! - `nwsmux` and `wsmuxdevs`, the table of muxes `wsmux_getmux` grows, are one
//!   `StaticCell` changed under the kernel lock (the C has no other lock either); a mux,
//!   once created, is never freed, so its softc is `&'static`.
//! - The entry points look the mux up and answer `ENXIO` for a minor without one, where
//!   the C indexes `wsmuxdevs[minor(dev)]` unchecked (open made it).
//! - `wsmux_attach_sc`'s check that the source has no parent yet is always on (the C
//!   compiles it under `DIAGNOSTIC`): linking a source that is in another mux's list would
//!   corrupt both lists. Its message, and the other `DIAGNOSTIC` checks of a child's parent,
//!   are behind the `diagnostic` feature.
//! - `wsmux_attach_sc` takes an `Option` (its C tests for a NULL mux).
//! - `WSMUX_DEBUG`'s `DPRINTF`s are not carried.

use core::ptr::{self, NonNull};

use libkern::StaticCell;

use crate::dev::wscons::wsconsio::{
    WSKBDIO_SETMODE, WSMUX_KBD, WSMUX_MAXDEV, WSMUX_MOUSE, WSMUX_MUX, WSMUXIO_ADD_DEVICE,
    WSMUXIO_INJECTEVENT, WSMUXIO_LIST_DEVICES, WSMUXIO_REMOVE_DEVICE, WsconsEvent, WsmuxDevice,
    WsmuxDeviceList,
};
use crate::dev::wscons::wsevent::{wsevent_fini, wsevent_init, wsevent_kqfilter, wsevent_read};
use crate::dev::wscons::wseventvar::{WSEVENT_QSIZE, Wseventvar, wsevent_wakeup};
use crate::dev::wscons::wskbd::wskbd_add_mux;
use crate::dev::wscons::wsksymdef::{KB_DEFAULT, KB_NOENCODING, KB_NONE};
use crate::dev::wscons::wsmouse::wsmouse_add_mux;
use crate::dev::wscons::wsmuxvar::{
    Wsevsrc, WsmuxSoftc, Wssrcops, wsevsrc_close, wsevsrc_display_ioctl, wsevsrc_ioctl,
    wsevsrc_open, wsevsrc_set_display,
};
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_malloc::{free, malloc, mallocarray};
use crate::kern::kern_rwlock::{
    rw_assert_anylock, rw_assert_wrlock, rw_enter_read, rw_enter_write, rw_exit_read,
    rw_exit_write, rw_init_flags,
};
use crate::kern::kern_sig::{sigio_getown, sigio_setown};
use crate::kern::kern_tc::nanotime;
use crate::kern::subr_prf::{Str, printf, snprintf};
use crate::sys::device::Device;
use crate::sys::errno::Errno;
use crate::sys::event::Knote;
use crate::sys::fcntl::{FREAD, FWRITE};
use crate::sys::filio::{FIOASYNC, FIOGETOWN, FIOSETOWN};
use crate::sys::ioctl::{ioctl_arg, ioctl_ret};
use crate::sys::malloc::{M_DEVBUF, M_NOWAIT, M_ZERO};
use crate::sys::proc::Proc;
use crate::sys::rwlock::{RWL_DUPOK, Rwlock};
use crate::sys::ttycom::{TIOCGPGRP, TIOCSPGRP};
use crate::sys::types::{Dev, minor};
use crate::sys::uio::Uio;

/// `NWSMUX`: `pseudo-device wsmux` is configured (`needs-flag`; GENERIC's count, 2, is the
/// `pdevinit[]` entry's).
pub const NWSMUX: i32 = 1;

/// `WSMUX_MAXDEPTH`: the most muxes stacked on each other.
const WSMUX_MAXDEPTH: i32 = 8;

/// `nwsmux` and `wsmuxdevs`: the muxes allocated so far, `wsmuxdevs[n]` for mux `n`.
struct Wsmuxdevs {
    /// `nwsmux`: the length of `devs`.
    nwsmux: i32,
    /// `wsmuxdevs`: `nwsmux` slots (`mallocarray`ed), NULL for a mux not created yet.
    devs: *mut Option<NonNull<WsmuxSoftc>>,
}

// SAFETY: the table and the muxes it points at are touched only under the kernel lock.
unsafe impl Send for Wsmuxdevs {}

/// `wsmux_srcops`: a mux as an event source of its parent mux.
pub static WSMUX_SRCOPS: Wssrcops = Wssrcops {
    type_: WSMUX_MUX,
    dopen: wsmux_mux_open,
    dclose: wsmux_mux_close,
    dioctl: wsmux_do_ioctl,
    ddispioctl: Some(wsmux_do_displayioctl),
    dsetdisplay: Some(wsmux_evsrc_set_display),
};

/// `wsmux_tree_lock`: lock used by `wsmux_add_mux()` to grant exclusive access to the tree
/// of stacked wsmux devices.
static WSMUX_TREE_LOCK: Rwlock = Rwlock::new("wsmuxtreelk");

/// Keep track of all muxes that have been allocated. Grown by `wsmux_getmux` under the
/// kernel lock (autoconfiguration, or the open of a mux's device).
static WSMUXDEVS: StaticCell<Wsmuxdevs> = StaticCell::new(Wsmuxdevs {
    nwsmux: 0,
    devs: ptr::null_mut(),
});

/// `wsmuxattach`: from upper level (the pseudo-device's attach: the muxes are made on
/// demand).
pub fn wsmuxattach(_n: i32) {}

/// `wsmuxdevs[n]`, `None` for a mux not created yet.
fn wsmux_lookup(n: i32) -> Option<&'static WsmuxSoftc> {
    // SAFETY: the table changes only in `wsmux_getmux`, under the kernel lock, which every
    // caller holds.
    let t = unsafe { WSMUXDEVS.get() };
    if n < 0 || n >= t.nwsmux {
        return None;
    }
    // SAFETY: `devs` has `nwsmux` initialised slots; a mux is never freed.
    unsafe { (*t.devs.add(n as usize)).map(|sc| sc.as_ref()) }
}

/// `wsmux_getmux`: return mux `n`, create if necessary.
pub fn wsmux_getmux(n: i32) -> Option<&'static WsmuxSoftc> {
    if n < 0 || n >= WSMUX_MAXDEV as i32 {
        return None;
    }

    // SAFETY: see `WSMUXDEVS`: the kernel lock, and no reference to the table outlives
    // this function's other calls.
    let t = unsafe { WSMUXDEVS.get_mut() };

    // Make sure there is room for mux n in the table
    if n >= t.nwsmux {
        let old = t.devs;
        let slot = size_of::<Option<NonNull<WsmuxSoftc>>>();
        let Some(new) = mallocarray(n as usize + 1, slot, M_DEVBUF, M_NOWAIT) else {
            printf(format_args!("wsmux_getmux: no memory for mux {}\n", n));
            return None;
        };
        let new = new.as_ptr().cast::<Option<NonNull<WsmuxSoftc>>>();
        for i in 0..=n as usize {
            let v = if (i as i32) < t.nwsmux {
                // SAFETY: the old table has `nwsmux` initialised slots.
                unsafe { *old.add(i) }
            } else {
                None
            };
            // SAFETY: the new table has `n + 1` slots.
            unsafe { new.add(i).write(v) };
        }
        if let Some(old) = NonNull::new(old) {
            free(old.cast(), M_DEVBUF, t.nwsmux as usize * slot);
        }
        t.devs = new;
        t.nwsmux = n + 1;
    }

    // SAFETY: `n < nwsmux`.
    let slot = unsafe { &mut *t.devs.add(n as usize) };
    if slot.is_none() {
        let sc = wsmux_create(b"wsmux", n);
        if sc.is_none() {
            printf(format_args!("wsmux: attach out of memory\n"));
        }
        *slot = sc.map(NonNull::from);
    }
    // SAFETY: a mux from `wsmux_create`, never freed.
    slot.map(|sc| unsafe { sc.as_ref() })
}

/// `wsmuxopen`: open() of the pseudo device from device table.
pub fn wsmuxopen(dev: Dev, flags: i32, _mode: i32, _p: &Proc) -> Result<(), Errno> {
    let unit = minor(dev) as i32;
    let sc = wsmux_getmux(unit).ok_or(Errno::ENXIO)?;

    if flags & (FREAD | FWRITE) == FWRITE {
        // Not opening for read, only ioctl is available.
        return Ok(());
    }

    if sc.sc_base.me_parent.get().is_some() {
        // Grab the mux out of the greedy hands of the parent mux.
        wsmux_detach_sc(&sc.sc_base);
    }

    if sc.sc_base.me_evp.get().is_some() {
        // Already open.
        return Err(Errno::EBUSY);
    }

    let evar = &sc.sc_base.me_evar;
    if wsevent_init(evar).is_err() {
        return Err(Errno::EBUSY);
    }
    if cfg!(feature = "wsdisplay_compat_rawkbd") {
        sc.sc_rawkbd.set(0);
    }

    let error = wsmux_do_open(sc, evar);
    if error.is_err() {
        wsevent_fini(evar);
    }
    error
}

/// `wsmux_mux_open`: open of a mux via the parent mux.
pub fn wsmux_mux_open(me: &Wsevsrc, evar: &Wseventvar) -> Result<(), Errno> {
    // SAFETY: `wsmux_srcops` is only the methods of muxes.
    let sc = unsafe { WsmuxSoftc::of_evsrc(me) };

    if cfg!(feature = "diagnostic") && sc.sc_base.me_parent.get().is_none() {
        printf(format_args!("wsmux_mux_open: no parent\n"));
        return Err(Errno::EINVAL);
    }

    wsmux_do_open(sc, evar)
}

/// The `DIAGNOSTIC` check of the loops over a mux's children: `me` names `sc` as its
/// parent. It always holds without `diagnostic`.
fn child_of(me: &Wsevsrc, sc: &WsmuxSoftc) -> bool {
    !cfg!(feature = "diagnostic") || me.me_parent.get() == Some(NonNull::from(sc))
}

/// `wsmux_do_open`: common part of opening a mux.
pub fn wsmux_do_open(sc: &WsmuxSoftc, evar: &Wseventvar) -> Result<(), Errno> {
    // The device could already be attached to a mux.
    if sc.sc_base.me_evp.get().is_some() {
        return Err(Errno::EBUSY);
    }
    // remember event variable, mark as open
    sc.sc_base.me_evp.set(Some(NonNull::from(evar)));

    // Open all children.
    rw_enter_read(&sc.sc_lock);
    for me in sc.sc_cld.iter() {
        if cfg!(feature = "diagnostic") {
            if me.me_evp.get().is_some() {
                printf(format_args!("wsmuxopen: dev already in use\n"));
                continue;
            }
            if !child_of(me, sc) {
                printf(format_args!("wsmux_do_open: bad child={:p}\n", me));
                continue;
            }
        }
        // ignore errors, failing children will not be marked open
        let _ = wsevsrc_open(me, evar);
    }
    rw_exit_read(&sc.sc_lock);

    Ok(())
}

/// `wsmuxclose`: close() of the pseudo device from device table.
pub fn wsmuxclose(dev: Dev, flags: i32, _mode: i32, _p: Option<&Proc>) -> Result<(), Errno> {
    let sc = wsmux_lookup(minor(dev) as i32).ok_or(Errno::ENXIO)?;
    let evar = sc.sc_base.me_evp.get();

    if flags & (FREAD | FWRITE) == FWRITE {
        // Not open for read
        return Ok(());
    }

    wsmux_do_close(sc);
    sc.sc_base.me_evp.set(None);
    if let Some(evar) = evar {
        // SAFETY: the mux's own `me_evar` (the queue `wsmuxopen` opened), a member of the
        // never-freed softc.
        wsevent_fini(unsafe { evar.as_ref() });
    }
    Ok(())
}

/// `wsmux_mux_close`: close of a mux via the parent mux.
pub fn wsmux_mux_close(me: &Wsevsrc) -> Result<(), Errno> {
    // SAFETY: `wsmux_srcops` is only the methods of muxes.
    let sc = unsafe { WsmuxSoftc::of_evsrc(me) };

    wsmux_do_close(sc);
    sc.sc_base.me_evp.set(None);

    Ok(())
}

/// `wsmux_do_close`: common part of closing a mux.
pub fn wsmux_do_close(sc: &WsmuxSoftc) {
    // Close all the children.
    rw_enter_read(&sc.sc_lock);
    for me in sc.sc_cld.iter() {
        if !child_of(me, sc) {
            printf(format_args!("wsmuxclose: bad child={:p}\n", me));
            continue;
        }
        let _ = wsevsrc_close(me);
    }
    rw_exit_read(&sc.sc_lock);
}

/// `wsmuxread`: read() of the pseudo device from device table.
pub fn wsmuxread(dev: Dev, uio: &mut Uio<'_>, flags: i32) -> Result<(), Errno> {
    let sc = wsmux_lookup(minor(dev) as i32).ok_or(Errno::ENXIO)?;

    let Some(evar) = sc.sc_base.evp() else {
        // XXX can we get here?
        if cfg!(feature = "diagnostic") {
            printf(format_args!("wsmuxread: not open\n"));
        }
        return Err(Errno::EINVAL);
    };

    wsevent_read(evar, uio, flags)
}

/// `wsmuxioctl`: ioctl of the pseudo device from device table.
pub fn wsmuxioctl(dev: Dev, cmd: u64, data: &mut [u8], flag: i32, p: &Proc) -> Result<(), Errno> {
    let sc = wsmux_lookup(minor(dev) as i32).ok_or(Errno::ENXIO)?;
    wsmux_do_ioctl(&sc.sc_base.me_dv, cmd, data, flag, Some(p))
}

/// `wsmux_do_ioctl`: ioctl of a mux via the parent mux, continuation of `wsmuxioctl()`.
pub fn wsmux_do_ioctl(
    dv: &Device,
    cmd: u64,
    data: &mut [u8],
    flag: i32,
    p: Option<&Proc>,
) -> Result<(), Errno> {
    // SAFETY: `dv` is a mux's device (its `wsmuxdevs` slot, or a child mux's `me_dv` through
    // `wsmux_srcops`).
    let sc = unsafe { WsmuxSoftc::of_evsrc(Wsevsrc::of_device(dv)) };

    let needs_write = matches!(
        cmd,
        WSMUXIO_INJECTEVENT | WSMUXIO_ADD_DEVICE | WSMUXIO_REMOVE_DEVICE
    ) || (cfg!(feature = "wsdisplay_compat_rawkbd") && cmd == WSKBDIO_SETMODE);
    if needs_write && flag & FWRITE == 0 {
        return Err(Errno::EACCES);
    }

    match cmd {
        WSMUXIO_INJECTEVENT => {
            // Inject an event, e.g., from moused.
            let Some(evar) = sc.sc_base.evp() else {
                // No event sink, so ignore it.
                return Ok(());
            };

            mtx_enter(&evar.ws_mtx);
            let get = evar.ws_get.get();
            let mut put = evar.ws_put.get();
            let at = put;
            put += 1;
            if put % WSEVENT_QSIZE == get {
                mtx_leave(&evar.ws_mtx);
                return Err(Errno::ENOSPC);
            }
            if put >= WSEVENT_QSIZE {
                put = 0;
            }
            let mut ev: WsconsEvent = ioctl_arg(data);
            ev.time = nanotime();
            // SAFETY: the queue is open (`me_evp`), `at < WSEVENT_QSIZE`, `ws_mtx` held.
            unsafe { evar.q_write(at, ev) };
            evar.ws_put.set(put);
            mtx_leave(&evar.ws_mtx);
            wsevent_wakeup(evar);
            return Ok(());
        }
        WSMUXIO_ADD_DEVICE => {
            let d: WsmuxDevice = ioctl_arg(data);
            if d.idx < 0 {
                return Err(Errno::ENXIO);
            }
            return match d.type_ {
                WSMUX_MOUSE => wsmouse_add_mux(d.idx, sc),
                WSMUX_KBD => wskbd_add_mux(d.idx, sc),
                WSMUX_MUX => wsmux_add_mux(d.idx, sc),
                _ => Err(Errno::EINVAL),
            };
        }
        WSMUXIO_REMOVE_DEVICE => {
            let d: WsmuxDevice = ioctl_arg(data);
            // Locate the device
            rw_enter_write(&sc.sc_lock);
            let found = sc
                .sc_cld
                .iter()
                .find(|me| me.ops().type_ == d.type_ && me.me_dv.dv_unit.get() == d.idx);
            if let Some(me) = found {
                wsmux_detach_sc_locked(sc, me);
                rw_exit_write(&sc.sc_lock);
                return Ok(());
            }
            rw_exit_write(&sc.sc_lock);
            return Err(Errno::EINVAL);
        }
        WSMUXIO_LIST_DEVICES => {
            let mut l: WsmuxDeviceList = ioctl_arg(data);
            let mut n = 0;
            rw_enter_read(&sc.sc_lock);
            for me in sc.sc_cld.iter() {
                if n >= WSMUX_MAXDEV {
                    break;
                }
                l.devices[n].type_ = me.ops().type_;
                l.devices[n].idx = me.me_dv.dv_unit.get();
                n += 1;
            }
            rw_exit_read(&sc.sc_lock);
            l.ndevices = n as i32;
            ioctl_ret(data, &l);
            return Ok(());
        }
        WSKBDIO_SETMODE if cfg!(feature = "wsdisplay_compat_rawkbd") => {
            sc.sc_rawkbd.set(ioctl_arg::<i32>(data));
        }
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
        _ => {}
    }

    if sc.sc_base.me_evp.get().is_none() && sc.displaydv().is_none() {
        return Err(Errno::EACCES);
    }

    // If children are attached: return 0 if any of the ioctl() succeeds, otherwise the last
    // error.
    let mut error = Err(Errno::ENOTTY);
    let mut ok = false;
    rw_enter_read(&sc.sc_lock);
    for me in sc.sc_cld.iter() {
        // XXX check evp?
        if !child_of(me, sc) {
            printf(format_args!("wsmux_do_ioctl: bad child {:p}\n", me));
            continue;
        }
        error = wsevsrc_ioctl(me, cmd, data, flag, p);
        if error.is_ok() {
            ok = true;
        }
    }
    rw_exit_read(&sc.sc_lock);
    if ok {
        error = Ok(());
    }

    error
}

/// `wsmuxkqfilter`.
pub fn wsmuxkqfilter(dev: Dev, kn: &Knote) -> Result<(), Errno> {
    let sc = wsmux_lookup(minor(dev) as i32).ok_or(Errno::ENXIO)?;

    let evar = sc.sc_base.evp().ok_or(Errno::ENXIO)?;
    wsevent_kqfilter(evar, kn)
}

/// `wsmux_add_mux`: add mux `unit` as a child to `muxsc`.
pub fn wsmux_add_mux(unit: i32, muxsc: &WsmuxSoftc) -> Result<(), Errno> {
    let sc = wsmux_getmux(unit).ok_or(Errno::ENXIO)?;

    rw_enter_write(&WSMUX_TREE_LOCK);

    let error = 'out: {
        if sc.sc_base.me_parent.get().is_some() || sc.sc_base.me_evp.get().is_some() {
            break 'out Err(Errno::EBUSY);
        }

        // The mux we are adding must not be an ancestor of itself.
        let mut depth = 0;
        let mut m = Some(muxsc);
        while let Some(mm) = m {
            if ptr::eq(mm, sc) {
                break 'out Err(Errno::EINVAL);
            }
            depth += 1;
            m = mm.sc_base.parent();
        }

        // Limit the number of stacked wsmux devices to avoid exhausting the kernel stack
        // during wsmux_do_open().
        if depth + wsmux_depth(sc) > WSMUX_MAXDEPTH {
            break 'out Err(Errno::EBUSY);
        }

        wsmux_attach_sc(Some(muxsc), &sc.sc_base)
    };
    rw_exit_write(&WSMUX_TREE_LOCK);
    error
}

/// `wsmux_create`: create a new mux softc, `<name><unit>`.
pub fn wsmux_create(name: &[u8], unit: i32) -> Option<&'static WsmuxSoftc> {
    let mem = malloc(size_of::<WsmuxSoftc>(), M_DEVBUF, M_NOWAIT | M_ZERO)?;
    // SAFETY: a fresh zeroed block of `size_of::<WsmuxSoftc>()` bytes, aligned by `malloc`;
    // all-zero is a valid `WsmuxSoftc` (its `Softc` contract), and a mux is never freed.
    let sc: &'static WsmuxSoftc = unsafe { &*mem.as_ptr().cast::<WsmuxSoftc>() };
    sc.sc_cld.init();
    rw_init_flags(&sc.sc_lock, "wsmuxlk", RWL_DUPOK);
    let mut xname = [0u8; 16];
    let _ = snprintf(&mut xname, format_args!("{}{}", Str(name), unit));
    sc.sc_base.me_dv.dv_xname.set(xname);
    sc.sc_base.me_dv.dv_unit.set(unit);
    sc.sc_base.me_ops.set(Some(&WSMUX_SRCOPS));
    sc.sc_kbd_layout.set(KB_NONE);
    Some(sc)
}

/// `wsmux_attach_sc`: attach `me` as a child to `sc`.
pub fn wsmux_attach_sc(sc: Option<&WsmuxSoftc>, me: &Wsevsrc) -> Result<(), Errno> {
    let sc = sc.ok_or(Errno::EINVAL)?;

    rw_enter_write(&sc.sc_lock);

    if me.me_parent.get().is_some() {
        rw_exit_write(&sc.sc_lock);
        if cfg!(feature = "diagnostic") {
            printf(format_args!("wsmux_attach_sc: busy\n"));
        }
        return Err(Errno::EBUSY);
    }
    me.me_parent.set(Some(NonNull::from(sc)));
    // SAFETY: `me` has no parent, so it is in no mux's list; event sources stay in place
    // until detached (`wskbd_detach` and `wsmux_detach_sc` unlink them first).
    unsafe { sc.sc_cld.insert_tail(me) };

    let mut error = Ok(());
    if let Some(displaydv) = sc.displaydv() {
        // This is a display mux, so attach the new device to it.
        if me.ops().dsetdisplay.is_some() {
            error = wsevsrc_set_display(me, Some(displaydv));
            // Ignore that the console already has a display.
            if error == Err(Errno::EBUSY) {
                error = Ok(());
            }
            if error.is_ok() && cfg!(feature = "wsdisplay_compat_rawkbd") {
                let mut raw = sc.sc_rawkbd.get().to_ne_bytes();
                let _ = wsevsrc_ioctl(me, WSKBDIO_SETMODE, &mut raw, FWRITE, None);
            }
        }
    }
    if let Some(evp) = sc.sc_base.evp() {
        // Mux is open, try to open the subdevice.
        error = wsevsrc_open(me, evp);
    } else {
        // Mux is closed, ensure that the subdevice is also closed.
        if me.me_evp.get().is_some() {
            error = Err(Errno::EBUSY);
        }
    }

    if error.is_err() {
        me.me_parent.set(None);
        // SAFETY: inserted above.
        unsafe { sc.sc_cld.remove(me) };
    }

    rw_exit_write(&sc.sc_lock);

    error
}

/// `wsmux_detach_sc`: remove `me` from the parent.
pub fn wsmux_detach_sc(me: &Wsevsrc) {
    let Some(sc) = me.parent() else {
        printf(format_args!(
            "wsmux_detach_sc: {} has no parent\n",
            me.me_dv.xname()
        ));
        return;
    };

    rw_enter_write(&sc.sc_lock);
    wsmux_detach_sc_locked(sc, me);
    rw_exit_write(&sc.sc_lock);
}

/// `wsmux_detach_sc_locked`: remove `me` from `sc`, whose lock is held for writing.
pub fn wsmux_detach_sc_locked(sc: &WsmuxSoftc, me: &Wsevsrc) {
    rw_assert_wrlock(&sc.sc_lock);

    if me.me_parent.get() != Some(NonNull::from(sc)) {
        // Device detached or attached to another mux while sleeping.
        return;
    }

    if sc.displaydv().is_some() {
        if me.ops().dsetdisplay.is_some() {
            // ignore error, there's nothing we can do
            let _ = wsevsrc_set_display(me, None);
        }
    } else if me.me_evp.get().is_some() {
        // mux device is open, so close multiplexee
        let _ = wsevsrc_close(me);
    }

    // SAFETY: `me` names `sc` as its parent, so it is in `sc`'s list.
    unsafe { sc.sc_cld.remove(me) };
    me.me_parent.set(None);
}

/// `wsmux_do_displayioctl`: display ioctl() of a mux via the parent mux. `Ok(true)` if any
/// of the ioctl() succeeds, otherwise the last error; `Ok(false)` (the C's -1) if no mux
/// component accepts the ioctl.
pub fn wsmux_do_displayioctl(
    dv: &Device,
    cmd: u64,
    data: &mut [u8],
    flag: i32,
    p: Option<&Proc>,
) -> Result<bool, Errno> {
    // SAFETY: as in `wsmux_do_ioctl`.
    let sc = unsafe { WsmuxSoftc::of_evsrc(Wsevsrc::of_device(dv)) };

    if cfg!(feature = "wsdisplay_compat_rawkbd") && cmd == WSKBDIO_SETMODE {
        sc.sc_rawkbd.set(ioctl_arg::<i32>(data));
    }

    // Return 0 if any of the ioctl() succeeds, otherwise the last error. Return -1 if no mux
    // component accepts the ioctl.
    let mut error = Ok(false);
    let mut ok = false;
    rw_enter_read(&sc.sc_lock);
    for me in sc.sc_cld.iter() {
        if !child_of(me, sc) {
            printf(format_args!("wsmux_displayioctl: bad child {:p}\n", me));
            continue;
        }
        if me.ops().ddispioctl.is_some() {
            error = wsevsrc_display_ioctl(me, cmd, data, flag, p);
            if error == Ok(true) {
                ok = true;
            }
        }
    }
    rw_exit_read(&sc.sc_lock);
    if ok {
        error = Ok(true);
    }

    error
}

/// `wsmux_evsrc_set_display`: set display of a mux via the parent mux.
pub fn wsmux_evsrc_set_display(dv: &Device, displaydv: Option<&Device>) -> Result<(), Errno> {
    // SAFETY: as in `wsmux_do_ioctl`.
    let sc = unsafe { WsmuxSoftc::of_evsrc(Wsevsrc::of_device(dv)) };

    if displaydv.is_some() {
        if sc.displaydv().is_some() {
            return Err(Errno::EBUSY);
        }
    } else if sc.displaydv().is_none() {
        return Err(Errno::ENXIO);
    }

    wsmux_set_display(sc, displaydv)
}

/// `wsmux_set_display`: make `displaydv` (or none) the display of `sc` and of its children.
pub fn wsmux_set_display(sc: &WsmuxSoftc, displaydv: Option<&Device>) -> Result<(), Errno> {
    rw_enter_read(&sc.sc_lock);

    sc.sc_base.me_dispdv.set(displaydv.map(NonNull::from));

    let mut ok = false;
    let mut error = Ok(());
    for me in sc.sc_cld.iter() {
        if !child_of(me, sc) {
            printf(format_args!(
                "wsmux_set_display: bad child parent {:p}\n",
                me
            ));
            continue;
        }
        if me.ops().dsetdisplay.is_some() {
            error = wsevsrc_set_display(me, displaydv);
            if error.is_ok() {
                ok = true;
                if cfg!(feature = "wsdisplay_compat_rawkbd") {
                    let mut raw = sc.sc_rawkbd.get().to_ne_bytes();
                    let _ = wsevsrc_ioctl(me, WSKBDIO_SETMODE, &mut raw, FWRITE, None);
                }
            }
        }
    }
    if ok {
        error = Ok(());
    }

    rw_exit_read(&sc.sc_lock);

    error
}

/// `wsmux_get_layout`.
pub fn wsmux_get_layout(sc: &WsmuxSoftc) -> u32 {
    sc.sc_kbd_layout.get()
}

/// `wsmux_set_layout`: a layout the driver does not merely default to becomes the mux's.
pub fn wsmux_set_layout(sc: &WsmuxSoftc, layout: u32) {
    if layout & (KB_DEFAULT | KB_NOENCODING) == 0 {
        sc.sc_kbd_layout.set(layout);
    }
}

/// `wsmux_depth`: returns the depth of the longest chain of nested wsmux devices starting
/// from `sc`.
pub fn wsmux_depth(sc: &WsmuxSoftc) -> i32 {
    let mut maxdepth = 0;

    rw_assert_anylock(&WSMUX_TREE_LOCK);

    rw_enter_read(&sc.sc_lock);
    for me in sc.sc_cld.iter() {
        if me.ops().type_ != WSMUX_MUX {
            continue;
        }

        // SAFETY: a `WSMUX_MUX` source is a mux.
        let depth = wsmux_depth(unsafe { WsmuxSoftc::of_evsrc(me) });
        if depth > maxdepth {
            maxdepth = depth;
        }
    }
    rw_exit_read(&sc.sc_lock);

    maxdepth + 1
}

/// Host tests: the mux table back to empty after `setup_real_memory`, as the old table and
/// its muxes are in the previous test's memory (growing it would `free` the old table there).
/// The muxes themselves are forgotten, never freed, as in the kernel.
#[cfg(test)]
pub(crate) fn wsmux_test_reset() {
    // SAFETY: called by `setup_real_memory` under its lock, which every test that reaches the
    // mux table holds, so no reference to the table is alive.
    let t = unsafe { WSMUXDEVS.get_mut() };
    t.nwsmux = 0;
    t.devs = ptr::null_mut();
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests of the mux: creating muxes, stacking them, the device list ioctls, opening a
    // tree, injecting an event, and handing a display down to the children.
    //
    // Muxes are global and never freed, so each test uses mux numbers of its own (10 and up;
    // 0 and 1 are the ones `/dev/wsmouse`, `/dev/wskbd` and wsdisplay use).

    use std::boxed::Box;
    use std::vec;

    use super::*;
    use crate::dev::wscons::wsksymdef::{KB_DE, KB_US};
    use crate::kern::subr_pool::tests::setup_real_memory;
    use crate::sys::uio::{Iovec, UioRw, UioSeg};

    /// The mux's own device.
    fn dv(sc: &WsmuxSoftc) -> &Device {
        &sc.sc_base.me_dv
    }

    /// `WSMUXIO_LIST_DEVICES` on `sc`: the (type, unit) pairs.
    fn list(sc: &WsmuxSoftc) -> std::vec::Vec<(i32, i32)> {
        let mut data = [0u8; size_of::<WsmuxDeviceList>()];
        wsmux_do_ioctl(dv(sc), WSMUXIO_LIST_DEVICES, &mut data, FREAD, None).unwrap();
        let l: WsmuxDeviceList = ioctl_arg(&data);
        l.devices[..l.ndevices as usize]
            .iter()
            .map(|d| (d.type_, d.idx))
            .collect()
    }

    /// A `WSMUXIO_ADD_DEVICE`/`WSMUXIO_REMOVE_DEVICE` on `sc`.
    fn dev_ioctl(sc: &WsmuxSoftc, cmd: u64, type_: i32, idx: i32, flag: i32) -> Result<(), Errno> {
        let mut data = [0u8; size_of::<WsmuxDevice>()];
        ioctl_ret(&mut data, &WsmuxDevice { type_, idx });
        wsmux_do_ioctl(dv(sc), cmd, &mut data, flag, None)
    }

    #[test]
    fn getmux_creates_each_mux_once() {
        let _g = setup_real_memory();
        assert!(wsmux_getmux(-1).is_none());
        assert!(wsmux_getmux(WSMUX_MAXDEV as i32).is_none());
        let a = wsmux_getmux(10).unwrap();
        assert!(ptr::eq(a, wsmux_getmux(10).unwrap()));
        assert_eq!(a.sc_base.me_dv.xname(), "wsmux10");
        assert_eq!(a.sc_base.me_dv.dv_unit.get(), 10);
        assert_eq!(a.sc_base.ops().type_, WSMUX_MUX);
        assert_eq!(wsmux_get_layout(a), KB_NONE);
        assert!(wsmux_lookup(10).is_some());

        // A driver's default layout or one without an encoding never becomes the mux's.
        wsmux_set_layout(a, KB_DE | KB_DEFAULT);
        assert_eq!(wsmux_get_layout(a), KB_NONE);
        wsmux_set_layout(a, KB_US | KB_NOENCODING);
        assert_eq!(wsmux_get_layout(a), KB_NONE);
        wsmux_set_layout(a, KB_DE);
        assert_eq!(wsmux_get_layout(a), KB_DE);
    }

    #[test]
    fn muxes_stack_without_loops() {
        let _g = setup_real_memory();
        let (m11, m12, m13) = (
            wsmux_getmux(11).unwrap(),
            wsmux_getmux(12).unwrap(),
            wsmux_getmux(13).unwrap(),
        );

        assert_eq!(
            dev_ioctl(m11, WSMUXIO_ADD_DEVICE, WSMUX_MUX, 12, FWRITE),
            Ok(())
        );
        assert!(m12.sc_base.parent().is_some_and(|p| ptr::eq(p, m11)));
        assert_eq!(list(m11), [(WSMUX_MUX, 12)]);
        rw_enter_read(&WSMUX_TREE_LOCK);
        assert_eq!(wsmux_depth(m11), 2);
        rw_exit_read(&WSMUX_TREE_LOCK);

        // 11 is above 12: adding it below 12 would make a loop.
        assert_eq!(wsmux_add_mux(11, m12), Err(Errno::EINVAL));
        // 12 has a parent already.
        assert_eq!(wsmux_add_mux(12, m13), Err(Errno::EBUSY));

        assert_eq!(
            dev_ioctl(m11, WSMUXIO_REMOVE_DEVICE, WSMUX_MUX, 12, FREAD),
            Err(Errno::EACCES)
        );
        assert_eq!(
            dev_ioctl(m11, WSMUXIO_REMOVE_DEVICE, WSMUX_MUX, 12, FWRITE),
            Ok(())
        );
        assert!(list(m11).is_empty());
        assert!(m12.sc_base.parent().is_none());
        assert_eq!(
            dev_ioctl(m11, WSMUXIO_REMOVE_DEVICE, WSMUX_MUX, 12, FWRITE),
            Err(Errno::EINVAL)
        );
    }

    #[test]
    fn add_device_by_type() {
        let _g = setup_real_memory();
        let m = wsmux_getmux(14).unwrap();
        let add = |type_, idx| dev_ioctl(m, WSMUXIO_ADD_DEVICE, type_, idx, FWRITE);
        assert_eq!(add(WSMUX_KBD, -1), Err(Errno::ENXIO));
        assert_eq!(add(WSMUX_KBD, 7), Err(Errno::ENXIO), "no such keyboard");
        assert_eq!(add(WSMUX_MOUSE, 31), Err(Errno::ENXIO), "no such mouse");
        assert_eq!(add(9, 0), Err(Errno::EINVAL));
        assert_eq!(
            dev_ioctl(m, WSMUXIO_ADD_DEVICE, WSMUX_MUX, 15, FREAD),
            Err(Errno::EACCES)
        );
        // Neither open nor a display's input: other ioctls are refused.
        let mut data = [0u8; 4];
        assert_eq!(
            wsmux_do_ioctl(dv(m), WSKBDIO_SETMODE + 1, &mut data, FWRITE, None),
            Err(Errno::EACCES)
        );
    }

    #[test]
    fn an_open_tree_shares_the_root_queue() {
        let _g = setup_real_memory();
        let (root, child) = (wsmux_getmux(16).unwrap(), wsmux_getmux(17).unwrap());
        wsmux_add_mux(17, root).unwrap();

        let evar = &root.sc_base.me_evar;
        wsevent_init(evar).unwrap();
        wsmux_do_open(root, evar).unwrap();
        assert!(child.sc_base.evp().is_some_and(|e| ptr::eq(e, evar)));
        assert_eq!(wsmux_do_open(root, evar), Err(Errno::EBUSY));

        // FIOASYNC, then an injected event reaches the root's queue with a fresh time stamp.
        let mut on = 1i32.to_ne_bytes();
        wsmux_do_ioctl(dv(root), FIOASYNC, &mut on, FREAD, None).unwrap();
        assert_eq!(evar.ws_async.get(), 1);
        let mut data = [0u8; size_of::<WsconsEvent>()];
        let e = WsconsEvent {
            type_: 2,
            value: 42,
            time: crate::sys::time::Timespec::default(),
        };
        ioctl_ret(&mut data, &e);
        assert_eq!(
            wsmux_do_ioctl(dv(root), WSMUXIO_INJECTEVENT, &mut data, FREAD, None),
            Err(Errno::EACCES)
        );
        evar.ws_async.set(0); // no SIGIO owner on the host
        wsmux_do_ioctl(dv(root), WSMUXIO_INJECTEVENT, &mut data, FWRITE, None).unwrap();
        assert_eq!(evar.ws_put.get(), 1);

        let mut buf = vec![0u8; size_of::<WsconsEvent>()];
        let mut iov = [Iovec {
            iov_base: buf.as_mut_ptr().cast(),
            iov_len: buf.len(),
        }];
        let mut uio = Uio {
            uio_iov: &mut iov,
            uio_offset: 0,
            uio_resid: size_of::<WsconsEvent>(),
            uio_segflg: UioSeg::UIO_SYSSPACE,
            uio_rw: UioRw::UIO_READ,
            uio_procp: None,
        };
        wsevent_read(evar, &mut uio, crate::sys::vnode::IO_NDELAY).unwrap();
        let got: WsconsEvent = ioctl_arg(&buf);
        assert_eq!((got.type_, got.value), (2, 42));

        wsmux_do_close(root);
        root.sc_base.me_evp.set(None);
        assert!(child.sc_base.evp().is_none());
        wsevent_fini(evar);
    }

    #[test]
    fn a_display_reaches_the_children() {
        let _g = setup_real_memory();
        let (root, child) = (wsmux_getmux(18).unwrap(), wsmux_getmux(19).unwrap());
        wsmux_add_mux(19, root).unwrap();
        // SAFETY: all-zero is a valid `Device` (its documented contract); leaked for good.
        let disp: &'static Device = Box::leak(Box::new(unsafe { core::mem::zeroed::<Device>() }));

        wsmux_set_display(root, Some(disp)).unwrap();
        assert!(root.displaydv().is_some_and(|d| ptr::eq(d, disp)));
        assert!(child.displaydv().is_some_and(|d| ptr::eq(d, disp)));
        assert_eq!(
            wsmux_evsrc_set_display(dv(root), Some(disp)),
            Err(Errno::EBUSY)
        );

        // A display's input answers ioctls (here none of its children takes this one).
        let mut data = [0u8; 4];
        assert_eq!(
            wsmux_do_displayioctl(dv(root), WSKBDIO_SETMODE + 1, &mut data, FWRITE, None),
            Ok(false)
        );

        wsmux_set_display(root, None).unwrap();
        assert!(root.displaydv().is_none());
        assert!(child.displaydv().is_none());
        assert_eq!(wsmux_evsrc_set_display(dv(root), None), Err(Errno::ENXIO));
    }
}
/* </TESTS> */
