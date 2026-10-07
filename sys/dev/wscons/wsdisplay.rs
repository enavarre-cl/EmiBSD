/* $OpenBSD: wsdisplay.c,v 1.156 2026/04/17 06:18:19 deraadt Exp $ */
/* $NetBSD: wsdisplay.c,v 1.82 2005/02/27 00:27:52 perry Exp $ */
/* $OpenBSD: wsmoused.h,v 1.10 2014/10/27 13:55:05 mpi Exp $ */
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
 * Copyright (c) 2001 Jean-Baptiste Marchand, Julien Montagne and Jerome Verdon
 *
 * All rights reserved.
 *
 * This code is for mouse console support under the wscons console driver.
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
 *	This product includes software developed by
 *	Hellmuth Michaelis, Brian Dunford-Shore, Joerg Wunsch, Scott Turner
 *	and Charles Hannum.
 * 4. The name authors may not be used to endorse or promote products
 *    derived from this software without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHORS ``AS IS'' AND ANY EXPRESS OR
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES
 * OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED.
 * IN NO EVENT SHALL THE AUTHORS BE LIABLE FOR ANY DIRECT, INDIRECT,
 * INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT
 * NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
 * DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
 * THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
 * (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF
 * THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

//! `wsdisplay(4)`: the display half of wscons, `dev/wscons/wsdisplay.c`, with the mouse
//! console helpers `wsdisplay.c` defines and `<dev/wscons/wsmoused.h>` declares.
//!
//! Upstream: sys/dev/wscons/wsdisplay.c @ 3ce1f3f79392
//! Upstream: sys/dev/wscons/wsmoused.h @ 3ce1f3f79392
//!
//! A display driver (`efifb(4)`, `simplefb`, ...) attaches a `wsdisplay` child with its
//! screen types and access operations ([`WsemuldisplaydevAttachArgs`]). wsdisplay makes
//! virtual screens on it (`WSDISPLAY_DEFAULTSCREENS`, 6 in GENERIC), each a terminal
//! emulation (`wsemul_vt100` by default) drawing through the screen type's emulops, and each
//! a tty: `/dev/ttyC0` .. `/dev/ttyCb` are character major 12, minor `unit << 8 | screen`,
//! and minor 255 (`/dev/ttyCcfg`) is the control device. As the console
//! ([`wsdisplay_cnattach`], from the display driver's console attach) the first screen gets
//! the kernel's messages through [`wsdisplay_cnputc`]. The keyboard reaches the focused
//! screen through `wsdisplay_kbdinput` (wskbd), and screens are switched with
//! `WSDISPLAYIO_SETSCREEN` or the USL `VT_*` ioctls (`wsdisplay_compat_usl.rs`); the
//! `wsmoused(8)` selection code (`WSDISPLAYIO_WSMOUSED`) copies and pastes text with the
//! mouse.
//!
//! ## Deviations
//! - The kernel configuration's choices are fixed as in amd64's and arm64's GENERIC:
//!   `wsmux` is configured (`NWSMUX` 2) and so is `wskbd`, `DDB`, and the options
//!   `WSDISPLAY_COMPAT_USL` and `WSDISPLAY_COMPAT_RAWKBD` (cargo features
//!   `wsdisplay_compat_usl`, `wsdisplay_compat_rawkbd`, default on); the `#if NWSMUX == 0`
//!   code (the non-mux keyboard binding in `wsdisplay_attach`) is not compiled, as in C.
//!   [`wsdisplay_set_kbd`], which C compiles only without wsmux, is kept for the
//!   `wscons_callbacks.h` interface. The `HAVE_*` features are `wscons_features.rs`'s.
//! - Not ported yet (M13 keyboard step): `wsmux.c` and `wsevent.c`. `wsmux_getmux`,
//!   `wsmux_create`, `wsmux_set_display` and `wsmux_attach_sc` are reported with
//!   `unported!`, so `sc_input` stays `None`: the event source type ([`Wsevsrc`]) is
//!   uninhabited until then, and every `wsevsrc_*` call is the empty `match` of a `None`.
//!   `wsdisplay_attach` still prints the ` mux N` its locator names, as the C does before it
//!   looks the mux up. wskbd's `wskbd_set_console_display` and `wskbd_pickfree` are
//!   `wskbd.rs`'s stubs.
//! - `struct wsdisplay_softc`, `struct wsscreen` and `struct wsscreen_internal` keep the C's
//!   members in `Cell`s (the softc is zeroed by `config_make_softc`; screens are `malloc`ed
//!   and written once); the C's pointers between them are `NonNull`s and raw pointers, live
//!   from `wsscreen_attach` to `wsscreen_detach`, which only runs after the screen left
//!   `sc_scr[]` (`wsdisplay_delscreen`).
//! - The emulation and display operation tables are `wsemulvar.rs`'s and `wsdisplayvar.rs`'s
//!   (`Option<unsafe fn>` over the C's `void *` cookies); a NULL `alloc_screen`,
//!   `free_screen`, `show_screen` or `ioctl`, which the C calls unchecked, fails with
//!   `ENODEV` or does nothing.
//! - Errors are `Result<_, Errno>`. Where the C returns -1 for "not mine" (`ioctl` paths,
//!   `wsdisplay_internal_ioctl`) the Rust returns `Ok(false)` (`docs/C_TO_RUST.md`); the
//!   screen switch's internal steps take the C's `int error` (0 or an errno, as the
//!   `show_screen` and syncops callbacks pass it). `wsdisplay_param` reports a driver's -1
//!   as `ENOTTY`; `wsmoused`'s -1 (an event that is neither motion, button nor control) is
//!   `ERESTART`, the C's -1 seen by the ioctl's caller, and `ctrl_event`'s `return (1)` is
//!   `EPERM` (errno 1), as in C.
//! - `caddr_t data` is the kernel copy of the ioctl argument as bytes (`ioctl_arg` /
//!   `ioctl_ret`); the C's NULL `data` and `struct proc *` are an empty slice and `None`.
//! - `wsdisplay_cons`'s `cn_getc` and `cn_bell` are hooks the keyboard sets
//!   (`wsdisplay_set_cons_kbd`): `Consdev`'s function pointers are fixed, so the console
//!   entry points [`wsdisplay_cngetc`] and [`wsdisplay_cnbell`] call the getc and bell the
//!   keyboard installed (`wsdisplay_getc_dummy` and none at first).
//! - `allocate_copybuffer` frees the previous buffer whenever there is one and zeroes the
//!   new one: the C keeps a same-sized buffer allocated twice (a leak) and a paste could read
//!   uninitialised bytes.
//! - `wsdisplay_defaultscreens` and `wsdisplay_clearonclose`, globals a kernel can be
//!   patched in, are atomics behind the functions of the same names.
//! - The `vdevgone` of `wsdisplay_closescreen` and the `cdevsw[]` lookups compare
//!   `d_open` against [`wsdisplayopen`] as the C does.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicPtr, Ordering};

use libkern::StaticCell;

use crate::conf::param::HZ;
use crate::ddb::db_output::db_resize;
use crate::dev::cons::{CN_LOWPRI, Consdev, cn_tab, set_cn_tab};
use crate::dev::wscons::wscons_callbacks::{
    WSDISPLAY_RESETCLOSE, WSDISPLAY_RESETEMUL, WsdisplayResetops, Wsevsrc, wskbd_pickfree,
    wskbd_set_console_display,
};
use crate::dev::wscons::wscons_features::{
    HAVE_BURNER_SUPPORT, HAVE_SCROLLBACK_SUPPORT, HAVE_WSMOUSED_SUPPORT,
};
use crate::dev::wscons::wsconsio::{
    WSCONS_EVENT_MOUSE_DELTA_X, WSCONS_EVENT_MOUSE_DELTA_Y, WSCONS_EVENT_MOUSE_DELTA_Z,
    WSCONS_EVENT_MOUSE_DOWN, WSCONS_EVENT_WSMOUSED_OFF, WSCONS_EVENT_WSMOUSED_ON,
    WSDISPLAY_BURN_KBD, WSDISPLAY_BURN_MOUSE, WSDISPLAY_BURN_OUTPUT, WSDISPLAY_BURN_VBLANK,
    WSDISPLAY_DELSCR_FORCE, WSDISPLAY_DELSCR_QUIET, WSDISPLAY_MAXFONTSZ, WSDISPLAYIO_ADDSCREEN,
    WSDISPLAYIO_DELFONT, WSDISPLAYIO_DELSCREEN, WSDISPLAYIO_GBURNER, WSDISPLAYIO_GETEMULTYPE,
    WSDISPLAYIO_GETPARAM, WSDISPLAYIO_GETSCREEN, WSDISPLAYIO_GETSCREENTYPE, WSDISPLAYIO_GMODE,
    WSDISPLAYIO_GTYPE, WSDISPLAYIO_GVIDEO, WSDISPLAYIO_LDFONT, WSDISPLAYIO_LSFONT,
    WSDISPLAYIO_MODE_DUMBFB, WSDISPLAYIO_MODE_EMUL, WSDISPLAYIO_MODE_MAPPED,
    WSDISPLAYIO_PARAM_BACKLIGHT, WSDISPLAYIO_PARAM_BRIGHTNESS, WSDISPLAYIO_PARAM_CONTRAST,
    WSDISPLAYIO_SBURNER, WSDISPLAYIO_SETPARAM, WSDISPLAYIO_SETSCREEN, WSDISPLAYIO_SMODE,
    WSDISPLAYIO_SVIDEO, WSDISPLAYIO_USEFONT, WSDISPLAYIO_VIDEO_OFF, WSDISPLAYIO_VIDEO_ON,
    WSDISPLAYIO_WSMOUSED, WSEMUL_NAME_SIZE, WSKBD_RAW, WSKBD_TRANSLATED, WSKBDIO_BELL,
    WSKBDIO_GETMODE, WSKBDIO_SETMODE, WSMUX_KBD, WSMUXIO_ADD_DEVICE, WSMUXIO_INJECTEVENT,
    WSMUXIO_LIST_DEVICES, WSMUXIO_REMOVE_DEVICE, WSSCREEN_NAME_SIZE, WsconsEvent,
    WsdisplayAddscreendata, WsdisplayBurner, WsdisplayDelscreendata, WsdisplayEmultype,
    WsdisplayFont, WsdisplayParam, WsdisplayScreentype, WsmuxDevice, is_button_event,
    is_ctrl_event, is_motion_event,
};
use crate::dev::wscons::wsdisplay_compat_usl::{wsdisplay_usl_ioctl1, wsdisplay_usl_ioctl2};
use crate::dev::wscons::wsdisplayvar::{
    ShowScreenCb, WSATTR_REVERSE, WSATTR_UNDERLINE, WSATTR_WSCOLORS, WSDISPLAY_DEFBURNIN_MSEC,
    WSDISPLAY_DEFBURNOUT_MSEC, WSDISPLAY_MAXSCREEN, WSDISPLAY_NULLSCREEN,
    WSDISPLAY_SCROLL_BACKWARD, WSDISPLAY_SCROLL_FORWARD, WSDISPLAY_SCROLL_RESET,
    WSEMULDISPLAYDEVCF_CONSOLE_UNK, WSEMULDISPLAYDEVCF_PRIMARY_UNK, WSSCREEN_REVERSE,
    WSSCREEN_WSCOLORS, WsconsSyncops, WsdisplayAccessops, WsdisplayCharcell, WsdisplayEmulops,
    WsemuldisplaydevAttachArgs, WsscreenDescr, WsscreenList, ws_get_param, ws_set_param,
    wsemuldisplaydevcf_console, wsemuldisplaydevcf_mux, wsemuldisplaydevcf_primary,
};
use crate::dev::wscons::wsemulconf::{wsemul_getname, wsemul_pick};
use crate::dev::wscons::wsemulvar::{
    WSEMUL_CLEARCURSOR, WSEMUL_CLEARSCREEN, WSEMUL_RESET, WSEMUL_SYNCFONT, WSEMUL_TRANSLATE_SIZE,
    WsemulOps, WsemulResetops,
};
use crate::dev::wscons::wsksymvar::{KbdT, KeysymT};
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_prot::suser;
use crate::kern::kern_synch::{tsleep_nsec, wakeup};
use crate::kern::kern_task::{
    Taskq, task_add, task_set, taskq_create, taskq_del_barrier, taskq_destroy,
};
use crate::kern::kern_timeout::{timeout_add, timeout_add_msec, timeout_del, timeout_set};
use crate::kern::subr_prf::{Str, panic, printf};
use crate::kern::tty::{ttioctl, ttsetwater, ttwakeupwr, ttychars, ttyclose, ttyfree, ttymalloc};
use crate::kern::tty_conf::linesw;
use crate::kern::tty_subr::{ndflush, ndqb};
use crate::kern::vfs_subr::vdevgone;
use crate::machine::conf::{cdevsw, nchrdev};
use crate::machine::copy::copyin;
use crate::machine::intr::{IPL_TTY, spltty, splx};
use crate::sys::conf::DevTypeOpen;
use crate::sys::device::{
    CfMatch, Cfattach, Cfdriver, DETACH_FORCE, DV_TTY, DVACT_POWERDOWN, Device, Softc, UNCONF,
};
use crate::sys::errno::Errno;
use crate::sys::fcntl::FWRITE;
use crate::sys::ioctl::{ioctl_arg, ioctl_ret};
use crate::sys::malloc::{M_DEVBUF, M_NOWAIT, M_WAITOK, M_ZERO};
use crate::sys::param::{NODEV, PCATCH};
use crate::sys::proc::Proc;
use crate::sys::systm::INFSLP;
use crate::sys::task::Task;
use crate::sys::termios::Termios;
use crate::sys::timeout::Timeout;
use crate::sys::tty::{
    TS_BUSY, TS_CARR_ON, TS_FLUSH, TS_ISOPEN, TS_TIMEOUT, TS_TTSTOP, TS_XCLUDE, Tty,
};
use crate::sys::ttydefaults::{
    TTYDEF_CFLAG, TTYDEF_IFLAG, TTYDEF_LFLAG, TTYDEF_OFLAG, TTYDEF_SPEED,
};
use crate::sys::types::{Dev, Paddr, major, makedev, minor};
use crate::sys::vnode::VCHR;
use crate::unported;

/// `SCR_OPEN`: is it open?
const SCR_OPEN: i32 = 1;
/// `SCR_WAITACTIVE`: someone waiting on activation.
const SCR_WAITACTIVE: i32 = 2;
/// `SCR_GRAPHICS`: graphics mode, no text (emulation) output.
const SCR_GRAPHICS: i32 = 4;
/// `SCR_DUMBFB`: in use as dumb fb (iff `SCR_GRAPHICS`).
const SCR_DUMBFB: i32 = 8;

/// `MOUSE_VISIBLE`: flag, the mouse cursor is visible.
const MOUSE_VISIBLE: u32 = 0x01;
/// `SEL_EXISTS`: flag, a selection exists.
const SEL_EXISTS: u32 = 0x02;
/// `SEL_IN_PROGRESS`: flag, a selection is in progress.
const SEL_IN_PROGRESS: u32 = 0x04;
/// `SEL_EXT_AFTER`: flag, selection is extended after.
const SEL_EXT_AFTER: u32 = 0x08;
/// `BLANK_TO_EOL`: flag, there are only blanks characters to eol.
const BLANK_TO_EOL: u32 = 0x10;
/// `SEL_BY_CHAR`: flag, select character by character.
const SEL_BY_CHAR: u32 = 0x20;
/// `SEL_BY_WORD`: flag, select word by word.
const SEL_BY_WORD: u32 = 0x40;
/// `SEL_BY_LINE`: flag, select line by line.
const SEL_BY_LINE: u32 = 0x80;

/// `SC_SWITCHPENDING`.
const SC_SWITCHPENDING: i32 = 0x01;
/// `SC_PASTE_AVAIL`.
const SC_PASTE_AVAIL: i32 = 0x02;

/// `WSDISPLAY_DEFAULTSCREENS`: the initial number of text consoles, GENERIC's
/// `option WSDISPLAY_DEFAULTSCREENS=6` (the C's default without the option is 1).
pub const WSDISPLAY_DEFAULTSCREENS: i32 = 6;

/// `NO_BORDER` (`wsmoused.h`).
const NO_BORDER: i32 = 0;
/// `BORDER` (`wsmoused.h`).
const BORDER: i32 = 1;

/// `MOUSE_COPY_BUTTON` (`wsmoused.h`).
const MOUSE_COPY_BUTTON: i32 = 0;
/// `MOUSE_PASTE_BUTTON` (`wsmoused.h`).
const MOUSE_PASTE_BUTTON: i32 = 1;
/// `MOUSE_EXTEND_BUTTON` (`wsmoused.h`).
const MOUSE_EXTEND_BUTTON: i32 = 2;

/// The console keyboard's `pollc` (`wsdisplay_set_cons_kbd`).
type KbdPollcFn = fn(Dev, i32);

/// The console keyboard's `bell` (`wsdisplay_set_cons_kbd`): pitch, period, volume.
type KbdBellFn = fn(Dev, u32, u32, u32);

/// `struct wsscreen_internal`: a screen's emulops and emulation, with their cookies.
///
/// The console's is the static [`WSDISPLAY_CONSOLE_CONF`]; the others are `malloc`ed by
/// `wsscreen_attach`. Every member is written before the screen is reachable and only read
/// afterwards (the console's while cold, by `wsdisplay_cnattach`).
pub struct WsscreenInternal {
    /// `emulops`.
    emulops: Cell<*const WsdisplayEmulops>,
    /// `emulcookie`.
    emulcookie: Cell<*mut c_void>,
    /// `scrdata`.
    scrdata: Cell<*const WsscreenDescr>,
    /// `wsemul`.
    wsemul: Cell<Option<&'static WsemulOps>>,
    /// `wsemulcookie`.
    wsemulcookie: Cell<*mut c_void>,
}

impl WsscreenInternal {
    /// All NULL.
    const fn new() -> Self {
        Self {
            emulops: Cell::new(ptr::null()),
            emulcookie: Cell::new(ptr::null_mut()),
            scrdata: Cell::new(ptr::null()),
            wsemul: Cell::new(None),
            wsemulcookie: Cell::new(ptr::null_mut()),
        }
    }

    /// `scrdata`: the screen type.
    fn scrdata(&self) -> &'static WsscreenDescr {
        // SAFETY: set to a display driver's screen type (a static or softc member that lives
        // for good) before the screen is reachable; never NULL afterwards.
        unsafe { &*self.scrdata.get() }
    }

    /// `N_COLS(dconf)`.
    fn n_cols(&self) -> u32 {
        self.scrdata().ncols as u32
    }

    /// `N_ROWS(dconf)`.
    fn n_rows(&self) -> u32 {
        self.scrdata().nrows as u32
    }

    /// `MAXCOL(dconf)`.
    fn maxcol(&self) -> u32 {
        self.n_cols().wrapping_sub(1)
    }

    /// `MAXROW(dconf)`.
    fn maxrow(&self) -> u32 {
        self.n_rows().wrapping_sub(1)
    }

    /// The display's emulops (the screen type's `textops`).
    fn emulops(&self) -> WsdisplayEmulops {
        let ops = self.emulops.get();
        if ops.is_null() {
            return WsdisplayEmulops::EMPTY;
        }
        // SAFETY: the screen type's `textops`, which lives as long as the driver.
        unsafe { *ops }
    }

    /// `(*wsemul->output)(wsemulcookie, data, count, kernel)`.
    fn output(&self, data: &[u8], kernel: bool) -> u32 {
        match self.wsemul.get() {
            // SAFETY: `wsemulcookie` is what this emulation's `attach`/`cnattach` returned;
            // the callers serialise output on a screen (its tty, or the console).
            Some(em) => unsafe { (em.output)(self.wsemulcookie.get(), data, kernel) },
            None => data.len() as u32,
        }
    }

    /// `(*wsemul->reset)(wsemulcookie, op)`.
    fn reset(&self, op: WsemulResetops) {
        if let Some(em) = self.wsemul.get() {
            // SAFETY: as in `output`.
            unsafe { (em.reset)(self.wsemulcookie.get(), op) };
        }
    }

    /// `PUTCHAR(dconf, pos, uc, attr)`.
    fn putchar(&self, pos: u32, uc: u32, attr: u32) -> Result<(), Errno> {
        let Some(putchar) = self.emulops().putchar else {
            return Err(Errno::ENODEV);
        };
        let n = self.n_cols();
        // SAFETY: `emulcookie` is the screen cookie the driver paired with these emulops.
        unsafe {
            putchar(
                self.emulcookie.get(),
                (pos / n) as i32,
                (pos % n) as i32,
                uc,
                attr,
            )
        }
    }
}

// SAFETY: see the type: written once while unreachable (or cold), read afterwards under the
// kernel lock.
unsafe impl Sync for WsscreenInternal {}

/// `struct wsscreen`: a virtual screen.
pub struct Wsscreen {
    /// `scr_dconf`.
    scr_dconf: Cell<*const WsscreenInternal>,
    /// `scr_emulbell_task`.
    scr_emulbell_task: Task,
    /// `scr_tty`.
    scr_tty: Cell<Option<&'static Tty>>,
    /// `scr_hold_screen`: hold tty output.
    scr_hold_screen: Cell<i32>,
    /// `scr_flags`: `SCR_*`.
    scr_flags: Cell<i32>,
    /// `scr_syncops` (`WSDISPLAY_COMPAT_USL`).
    scr_syncops: Cell<Option<&'static WsconsSyncops>>,
    /// `scr_synccookie` (`WSDISPLAY_COMPAT_USL`).
    scr_synccookie: Cell<*mut c_void>,
    /// `scr_rawkbd` (`WSDISPLAY_COMPAT_RAWKBD`).
    scr_rawkbd: Cell<i32>,
    /// `sc`.
    sc: Cell<*const WsdisplaySoftc>,
    /// `mouse`: mouse cursor position.
    mouse: Cell<u32>,
    /// `cursor`: selection cursor position (if different from mouse cursor pos).
    cursor: Cell<u32>,
    /// `cpy_start`: position of the copy start mark.
    cpy_start: Cell<u32>,
    /// `cpy_end`: position of the copy end mark.
    cpy_end: Cell<u32>,
    /// `orig_start`: position of the original sel. start.
    orig_start: Cell<u32>,
    /// `orig_end`: position of the original sel. end.
    orig_end: Cell<u32>,
    /// `mouse_flags`: flags, status of the mouse.
    mouse_flags: Cell<u32>,
}

impl Wsscreen {
    /// A zeroed screen (`malloc(..., M_ZERO)`).
    const fn new() -> Self {
        Self {
            scr_dconf: Cell::new(ptr::null()),
            scr_emulbell_task: Task::zeroed(),
            scr_tty: Cell::new(None),
            scr_hold_screen: Cell::new(0),
            scr_flags: Cell::new(0),
            scr_syncops: Cell::new(None),
            scr_synccookie: Cell::new(ptr::null_mut()),
            scr_rawkbd: Cell::new(0),
            sc: Cell::new(ptr::null()),
            mouse: Cell::new(0),
            cursor: Cell::new(0),
            cpy_start: Cell::new(0),
            cpy_end: Cell::new(0),
            orig_start: Cell::new(0),
            orig_end: Cell::new(0),
            mouse_flags: Cell::new(0),
        }
    }

    /// `scr->scr_dconf`.
    fn dconf(&self) -> &'static WsscreenInternal {
        // SAFETY: set by `wsscreen_attach` before the screen is reachable: the console's
        // static or a `malloc`ed one freed only with the screen.
        unsafe { &*self.scr_dconf.get() }
    }

    /// `scr->sc`.
    fn sc(&self) -> &'static WsdisplaySoftc {
        // SAFETY: set by `wsscreen_attach`; the softc outlives its screens.
        unsafe { &*self.sc.get() }
    }

    /// `WSSCREEN_HAS_TTY(scr)`: the screen's tty.
    fn tty(&self) -> Option<&'static Tty> {
        self.scr_tty.get()
    }

    /// `WS_NCOLS(scr)`.
    fn ws_ncols(&self) -> u32 {
        self.dconf().n_cols()
    }

    /// `WS_NROWS(scr)`.
    fn ws_nrows(&self) -> u32 {
        self.dconf().n_rows()
    }

    /// `ISSET(scr->mouse_flags, f)`.
    fn mflag(&self, f: u32) -> bool {
        self.mouse_flags.get() & f != 0
    }

    /// `SET(scr->mouse_flags, f)`.
    fn mset(&self, f: u32) {
        self.mouse_flags.set(self.mouse_flags.get() | f);
    }

    /// `CLR(scr->mouse_flags, f)`.
    fn mclr(&self, f: u32) {
        self.mouse_flags.set(self.mouse_flags.get() & !f);
    }

    /// `GETCHAR(scr, pos, &cell)`: the cell, `None` where the C's macro is non-zero.
    fn getchar(&self, pos: u32) -> Option<WsdisplayCharcell> {
        let sc = self.sc();
        let getchar = sc.ops().getchar?;
        let n = self.ws_ncols();
        // SAFETY: the access cookie the driver paired with its accessops.
        unsafe { getchar(sc.sc_accesscookie.get(), (pos / n) as i32, (pos % n) as i32) }
    }
}

/// `struct wsdisplay_softc`.
#[repr(C)]
pub struct WsdisplaySoftc {
    /// `sc_dv`.
    pub sc_dv: Device,
    /// `sc_accessops`.
    sc_accessops: Cell<Option<&'static WsdisplayAccessops>>,
    /// `sc_accesscookie`.
    sc_accesscookie: Cell<*mut c_void>,
    /// `sc_scrdata`.
    sc_scrdata: Cell<*const WsscreenList>,
    /// `sc_scr[WSDISPLAY_MAXSCREEN]`.
    sc_scr: [Cell<Option<NonNull<Wsscreen>>>; WSDISPLAY_MAXSCREEN as usize],
    /// `sc_focusidx`: available only if `sc_focus` isn't null.
    sc_focusidx: Cell<i32>,
    /// `sc_focus`.
    sc_focus: Cell<Option<NonNull<Wsscreen>>>,
    /// `sc_taskq`.
    sc_taskq: Cell<Option<&'static Taskq>>,
    /// `sc_burner` (`HAVE_BURNER_SUPPORT`).
    sc_burner: Timeout,
    /// `sc_burnoutintvl`: delay before blanking (ms).
    sc_burnoutintvl: Cell<i32>,
    /// `sc_burninintvl`: delay before unblanking (ms).
    sc_burninintvl: Cell<i32>,
    /// `sc_burnout`: current `sc_burner` delay (ms).
    sc_burnout: Cell<i32>,
    /// `sc_burnman`: nonzero if screen blanked.
    sc_burnman: Cell<i32>,
    /// `sc_burnflags`.
    sc_burnflags: Cell<u32>,
    /// `sc_isconsole`.
    sc_isconsole: Cell<i32>,
    /// `sc_flags`: `SC_*`.
    sc_flags: Cell<i32>,
    /// `sc_screenwanted`: valid with `SC_SWITCHPENDING`.
    sc_screenwanted: Cell<i32>,
    /// `sc_oldscreen`: valid with `SC_SWITCHPENDING`.
    sc_oldscreen: Cell<i32>,
    /// `sc_resumescreen`: if set, can't switch until resume.
    sc_resumescreen: Cell<i32>,
    /// `sc_input` (`NWSKBD > 0`): the keyboard event source, `None` until wsmux.
    sc_input: Cell<Option<&'static Wsevsrc>>,
    /// `sc_rawkbd` (`WSDISPLAY_COMPAT_RAWKBD`).
    sc_rawkbd: Cell<i32>,
    /// `sc_copybuffer` (`HAVE_WSMOUSED_SUPPORT`).
    sc_copybuffer: Cell<Option<NonNull<u8>>>,
    /// `sc_copybuffer_size`.
    sc_copybuffer_size: Cell<u32>,
}

// SAFETY: `#[repr(C)]` with the device first; every member is valid as all-zero bits (null
// pointers, `None`, 0, an unarmed timeout).
unsafe impl Softc for WsdisplaySoftc {}

impl WsdisplaySoftc {
    /// The display's access operations.
    fn ops(&self) -> &'static WsdisplayAccessops {
        self.sc_accessops.get().unwrap_or(&NO_ACCESSOPS)
    }

    /// `sc->sc_scr[idx]`, `None` for an index out of range.
    fn scr(&self, idx: i32) -> Option<&'static Wsscreen> {
        let p = self.sc_scr.get(usize::try_from(idx).ok()?)?.get()?;
        // SAFETY: a screen in `sc_scr[]` lives until `wsdisplay_delscreen` has taken it out
        // and detached it.
        Some(unsafe { p.as_ref() })
    }

    /// `sc->sc_focus`.
    fn focus(&self) -> Option<&'static Wsscreen> {
        // SAFETY: the focus is one of `sc_scr[]` (or NULL), cleared before a screen goes.
        self.sc_focus.get().map(|p| unsafe { p.as_ref() })
    }

    /// `ISSET(sc->sc_flags, f)`.
    fn flag(&self, f: i32) -> bool {
        self.sc_flags.get() & f != 0
    }

    /// `SET(sc->sc_flags, f)`.
    fn set_flag(&self, f: i32) {
        self.sc_flags.set(self.sc_flags.get() | f);
    }

    /// `CLR(sc->sc_flags, f)`.
    fn clr_flag(&self, f: i32) {
        self.sc_flags.set(self.sc_flags.get() & !f);
    }

    /// `(*sc->sc_accessops->show_screen)(sc->sc_accesscookie, cookie, waitok, cb, cbarg)`.
    fn show_screen(
        &self,
        cookie: *mut c_void,
        waitok: i32,
        cb: Option<ShowScreenCb>,
        cbarg: *mut c_void,
    ) -> Result<(), Errno> {
        let Some(f) = self.ops().show_screen else {
            return Err(Errno::ENODEV);
        };
        // SAFETY: the access cookie paired with these accessops; `cookie` came from their
        // `alloc_screen` (or is the console's screen).
        unsafe { f(self.sc_accesscookie.get(), cookie, waitok, cb, cbarg) }
    }

    /// `(*sc->sc_accessops->ioctl)(sc->sc_accesscookie, cmd, data, flag, p)`.
    fn driver_ioctl(
        &self,
        cmd: u64,
        data: &mut [u8],
        flag: i32,
        p: Option<&Proc>,
    ) -> Result<bool, Errno> {
        let Some(f) = self.ops().ioctl else {
            return Ok(false);
        };
        // SAFETY: the access cookie paired with these accessops.
        unsafe { f(self.sc_accesscookie.get(), cmd, data, flag, p) }
    }

    /// The display's screen types.
    fn scrdata(&self) -> &'static WsscreenList {
        // SAFETY: the attach arguments' list, a static or softc member of the driver that
        // lives for good; set before anything reads it.
        unsafe { &*self.sc_scrdata.get() }
    }

    /// A pointer to this softc, the `void *` the timeouts, tasks and callbacks get.
    fn as_arg(&self) -> *mut c_void {
        ptr::from_ref(self).cast_mut().cast()
    }
}

/// An empty accessops table, for a softc not attached yet.
static NO_ACCESSOPS: WsdisplayAccessops = WsdisplayAccessops::EMPTY;

/// `wsdisplay_cd`.
pub static WSDISPLAY_CD: Cfdriver = Cfdriver::new(b"wsdisplay", DV_TTY, 0);

/// `wsdisplay_ca`.
pub static WSDISPLAY_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<WsdisplaySoftc>(),
    ca_match: Some(wsdisplay_match),
    ca_attach: wsdisplay_attach,
    ca_detach: Some(wsdisplay_detach),
    ca_activate: Some(wsdisplay_activate),
};

/// `wsdisplay_console_initted`.
static WSDISPLAY_CONSOLE_INITTED: AtomicBool = AtomicBool::new(false);
/// `wsdisplay_console_device`.
static WSDISPLAY_CONSOLE_DEVICE: AtomicPtr<WsdisplaySoftc> = AtomicPtr::new(ptr::null_mut());
/// `wsdisplay_console_conf`.
static WSDISPLAY_CONSOLE_CONF: WsscreenInternal = WsscreenInternal::new();

/// `wsdisplay_cons_pollmode`.
static WSDISPLAY_CONS_POLLMODE: AtomicI32 = AtomicI32::new(0);
/// `wsdisplay_cons_kbd_pollc`.
static WSDISPLAY_CONS_KBD_POLLC: StaticCell<Option<KbdPollcFn>> = StaticCell::new(None);
/// `wsdisplay_cons.cn_getc`: the console keyboard's polled getc, `wsdisplay_getc_dummy`
/// until a keyboard sets it.
static WSDISPLAY_CONS_GETC: StaticCell<fn(Dev) -> i32> = StaticCell::new(wsdisplay_getc_dummy);
/// `wsdisplay_cons.cn_bell`: the console keyboard's bell, if any.
static WSDISPLAY_CONS_BELL: StaticCell<Option<KbdBellFn>> = StaticCell::new(None);

/// `wsdisplay_cons`.
static WSDISPLAY_CONS: Consdev = Consdev {
    cn_probe: None,
    cn_init: None,
    cn_getc: wsdisplay_cngetc,
    cn_putc: wsdisplay_cnputc,
    cn_pollc: wsdisplay_cnpollc,
    cn_bell: Some(wsdisplay_cnbell),
    cn_dev: Cell::new(NODEV),
    cn_pri: Cell::new(CN_LOWPRI),
};

/// `wsdisplay_defaultscreens`.
static DEFAULTSCREENS: AtomicI32 = AtomicI32::new(WSDISPLAY_DEFAULTSCREENS);
/// `wsdisplay_clearonclose`.
static CLEARONCLOSE: AtomicI32 = AtomicI32::new(0);

/// `wsdisplay_defaultscreens`: the screens a display gets at attach when its driver does not
/// say (`WSDISPLAY_DEFAULTSCREENS`).
pub fn wsdisplay_defaultscreens() -> i32 {
    DEFAULTSCREENS.load(Ordering::Relaxed)
}

/// `wsdisplay_clearonclose`: whether the last close of a screen clears it.
pub fn wsdisplay_clearonclose() -> i32 {
    CLEARONCLOSE.load(Ordering::Relaxed)
}

/// `IS_ALPHANUM(c)` (`wsmoused.h`).
const fn is_alphanum(c: u32) -> bool {
    c != b' ' as u32
}

/// `IS_SPACE(c)` (`wsmoused.h`).
const fn is_space(c: u32) -> bool {
    c == b' ' as u32
}

/// `WSDISPLAYUNIT(dev)`.
const fn wsdisplayunit(dev: Dev) -> i32 {
    (minor(dev) >> 8) as i32
}

/// `WSDISPLAYSCREEN(dev)`.
const fn wsdisplayscreen(dev: Dev) -> i32 {
    (minor(dev) & 0xff) as i32
}

/// `ISWSDISPLAYCTL(dev)`.
const fn iswsdisplayctl(dev: Dev) -> bool {
    wsdisplayscreen(dev) == 255
}

/// `WSDISPLAYMINOR(unit, screen)`.
const fn wsdisplayminor(unit: i32, screen: i32) -> u32 {
    ((unit << 8) | screen) as u32
}

/// The bytes of a NUL-terminated name in a fixed array, up to the NUL.
fn cstr(b: &[u8]) -> &[u8] {
    let len = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    &b[..len]
}

/// `strlcpy(dst, src, size)` into a fixed array: as much of `src` as fits with the NUL.
fn strlcpy_into(dst: &mut [u8], src: &[u8]) {
    let n = src.len().min(dst.len().saturating_sub(1));
    dst[..n].copy_from_slice(&src[..n]);
    if let Some(rest) = dst.get_mut(n..) {
        rest.fill(0);
    }
}

/// `wsdisplay_cd.cd_devs[unit]`: an attached display.
fn wsdisplay_sc(unit: i32) -> Option<&'static WsdisplaySoftc> {
    let dev = WSDISPLAY_CD.cd_dev(unit)?;
    // SAFETY: `wsdisplay_cd`'s devices are made by `config_make_softc` for `wsdisplay_ca`,
    // whose `ca_devsize` is a `WsdisplaySoftc`, and live until detached.
    Some(unsafe { &*dev.as_ptr().cast::<WsdisplaySoftc>() })
}

/// `(struct wsdisplay_softc *)dev`.
fn sc_of(dev: &Device) -> &'static WsdisplaySoftc {
    // SAFETY: `dev` is a wsdisplay device (made for `wsdisplay_ca`), which lives for good
    // once attached.
    unsafe { &*ptr::from_ref(dev.softc::<WsdisplaySoftc>()) }
}

/// `wsdisplay_console_device`.
fn console_device() -> Option<&'static WsdisplaySoftc> {
    // SAFETY: NULL or the console display's softc, which is never detached.
    unsafe { WSDISPLAY_CONSOLE_DEVICE.load(Ordering::Acquire).as_ref() }
}

/// The major number of wsdisplay (`cdevsw[maj].d_open == wsdisplayopen`), `nchrdev` if none.
fn wsdisplay_major() -> u32 {
    let n = nchrdev();
    (0..n)
        .find(|&maj| ptr::fn_addr_eq(cdevsw(maj).d_open, wsdisplayopen as DevTypeOpen))
        .unwrap_or(n)
}

/// `wsscreen_attach`: a new screen of display `sc`: the console's (`console`, whose
/// emulation `wsdisplay_cnattach` made), or one of type `type_` over the driver's screen
/// `cookie` with the emulation named `emul`. `None` when memory or the emulation fails.
#[allow(clippy::too_many_arguments)] // the C's eight parameters
fn wsscreen_attach(
    sc: &'static WsdisplaySoftc,
    console: bool,
    emul: Option<&[u8]>,
    type_: *const WsscreenDescr,
    cookie: *mut c_void,
    ccol: i32,
    crow: i32,
    defattr: u32,
) -> Option<NonNull<Wsscreen>> {
    let scrp = malloc(size_of::<Wsscreen>(), M_DEVBUF, M_ZERO | M_NOWAIT)?.cast::<Wsscreen>();
    // SAFETY: a fresh allocation of one `Wsscreen` (malloc's chunks are aligned to their
    // power-of-two size), written before any use.
    unsafe { ptr::write(scrp.as_ptr(), Wsscreen::new()) };
    // SAFETY: just written; ours until it is published in `sc_scr[]`.
    let scr = unsafe { scrp.as_ref() };
    let cbcookie: *mut c_void = scrp.as_ptr().cast();

    let free_scr = || free(scrp.cast(), M_DEVBUF, size_of::<Wsscreen>());

    let dconf: *const WsscreenInternal = if console {
        let dconf = &WSDISPLAY_CONSOLE_CONF;
        // Tell the emulation about the callback argument. The other stuff is already there.
        if let Some(em) = dconf.wsemul.get() {
            // SAFETY: the console attach of the emulation `wsdisplay_cnattach` set up: only
            // the callback cookie is read.
            let _ = unsafe { (em.attach)(true, None, ptr::null_mut(), 0, 0, cbcookie, 0) };
        }
        dconf
    } else {
        // not console
        let Some(dmem) = malloc(size_of::<WsscreenInternal>(), M_DEVBUF, M_NOWAIT) else {
            free_scr();
            return None;
        };
        let dp = dmem.cast::<WsscreenInternal>();
        // SAFETY: a fresh allocation of one `WsscreenInternal`, written before any use.
        unsafe { ptr::write(dp.as_ptr(), WsscreenInternal::new()) };
        // SAFETY: just written; ours.
        let dconf = unsafe { dp.as_ref() };
        let fail = || {
            free(dmem, M_DEVBUF, size_of::<WsscreenInternal>());
            free_scr();
        };
        // SAFETY: `type_` is one of the display's screen types, which live for good.
        let ty = unsafe { &*type_ };
        dconf.emulops.set(ty.textops);
        dconf.emulcookie.set(cookie);
        let wsemul = if ty.textops.is_null() {
            None
        } else {
            wsemul_pick(emul)
        };
        let Some(wsemul) = wsemul else {
            fail();
            return None;
        };
        dconf.wsemul.set(Some(wsemul));
        // SAFETY: `cookie` is the screen the driver's `alloc_screen` made for `ty`, whose
        // `textops` the emulation draws with; `cbcookie` is this screen.
        let c = unsafe { (wsemul.attach)(false, Some(ty), cookie, ccol, crow, cbcookie, defattr) };
        if c.is_null() {
            fail();
            return None;
        }
        dconf.wsemulcookie.set(c);
        dconf.scrdata.set(type_);
        dp.as_ptr()
    };

    task_set(&scr.scr_emulbell_task, wsdisplay_emulbell_task, cbcookie);
    scr.scr_dconf.set(dconf);
    scr.scr_tty.set(Some(ttymalloc(0)));
    scr.sc.set(sc);
    Some(scrp)
}

/// `wsscreen_detach`: free a screen, its tty and its emulation.
///
/// # Safety
///
/// `scrp` came from [`wsscreen_attach`], is no longer in its display's `sc_scr[]` nor its
/// focus, and nothing uses it afterwards.
unsafe fn wsscreen_detach(scrp: NonNull<Wsscreen>) {
    // SAFETY: the caller's contract: a live screen.
    let scr = unsafe { scrp.as_ref() };
    let (mut ccol, mut crow) = (0u32, 0u32); // XXX

    if let Some(tp) = scr.tty() {
        timeout_del(&tp.t_rstrt_to);
        // SAFETY: the screen's own tty from `ttymalloc`; its restart timeout is off and the
        // screen, its only user, goes away.
        unsafe { ttyfree(NonNull::from(tp)) };
    }
    let dconf = scr.dconf();
    if let Some(em) = dconf.wsemul.get() {
        // SAFETY: the emulation state this emulation's `attach` made for the screen.
        unsafe { (em.detach)(dconf.wsemulcookie.get(), &mut ccol, &mut crow) };
    }
    if let Some(tq) = scr.sc().sc_taskq.get() {
        taskq_del_barrier(tq, &scr.scr_emulbell_task);
    }
    if !ptr::eq(dconf, &WSDISPLAY_CONSOLE_CONF) {
        free(
            NonNull::from(dconf).cast(),
            M_DEVBUF,
            size_of::<WsscreenInternal>(),
        );
    }
    free(scrp.cast(), M_DEVBUF, size_of::<Wsscreen>());
}

/// `wsdisplay_screentype_pick`: the screen type named `name` (the first one for none or an
/// empty name), compared over `WSSCREEN_NAME_SIZE` bytes as `strncmp` does.
pub fn wsdisplay_screentype_pick(
    scrdata: &WsscreenList,
    name: Option<&[u8]>,
) -> Option<*const WsscreenDescr> {
    crate::kassert!(scrdata.nscreens > 0);

    // SAFETY: a display's list points at its screen types, which live for good.
    let screens = unsafe { scrdata.screens() };

    let name = name.map(cstr).unwrap_or(&[]);
    if name.is_empty() {
        return screens.first().copied();
    }

    let want = &name[..name.len().min(WSSCREEN_NAME_SIZE)];
    screens.iter().copied().find(|&scr| {
        // SAFETY: as above.
        let have = unsafe { &*scr }.name();
        have == want
    })
}

/// `wsdisplay_addscreen_print`: print info about attached screen.
fn wsdisplay_addscreen_print(sc: &WsdisplaySoftc, idx: i32, count: i32) {
    printf(format_args!("{}: screen {}", sc.sc_dv.xname(), idx));
    if count > 1 {
        printf(format_args!("-{}", idx + (count - 1)));
    }
    let Some(scr) = sc.scr(idx) else {
        printf(format_args!("\n"));
        return;
    };
    let dconf = scr.dconf();
    printf(format_args!(
        " added ({}, {} emulation)\n",
        Str(dconf.scrdata().name()),
        Str(dconf.wsemul.get().map_or(&b""[..], |e| e.name()))
    ));
}

/// `wsdisplay_addscreen`: a new screen at `idx`, of the type and emulation named (the
/// defaults for `None`).
fn wsdisplay_addscreen(
    sc: &'static WsdisplaySoftc,
    idx: i32,
    screentype: Option<&[u8]>,
    emul: Option<&[u8]>,
) -> Result<(), Errno> {
    if !(0..WSDISPLAY_MAXSCREEN).contains(&idx) {
        return Err(Errno::EINVAL);
    }
    if sc.scr(idx).is_some() {
        return Err(Errno::EBUSY);
    }

    let scrdesc = wsdisplay_screentype_pick(sc.scrdata(), screentype).ok_or(Errno::ENXIO)?;
    let ops = sc.ops();
    let Some(alloc_screen) = ops.alloc_screen else {
        return Err(Errno::ENODEV);
    };
    let mut cookie = ptr::null_mut();
    let (mut ccol, mut crow, mut defattr) = (0, 0, 0);
    // SAFETY: the access cookie paired with these accessops, and one of their screen types.
    unsafe {
        alloc_screen(
            sc.sc_accesscookie.get(),
            scrdesc,
            &mut cookie,
            &mut ccol,
            &mut crow,
            &mut defattr,
        )
    }?;

    let Some(scrp) = wsscreen_attach(sc, false, emul, scrdesc, cookie, ccol, crow, defattr) else {
        if let Some(free_screen) = ops.free_screen {
            // SAFETY: the screen `alloc_screen` just made, unused.
            unsafe { free_screen(sc.sc_accesscookie.get(), cookie) };
        }
        return Err(Errno::ENXIO);
    };

    sc.sc_scr[idx as usize].set(Some(scrp));

    // if no screen has focus yet, activate the first we get
    let s = spltty();
    if sc.sc_focus.get().is_none() {
        // SAFETY: just attached, in `sc_scr[]`.
        let scr = unsafe { scrp.as_ref() };
        let _ = sc.show_screen(scr.dconf().emulcookie.get(), 0, None, ptr::null_mut());
        sc.sc_focusidx.set(idx);
        sc.sc_focus.set(Some(scrp));
    }
    splx(s);

    if HAVE_WSMOUSED_SUPPORT {
        allocate_copybuffer(sc); // enlarge the copy buffer if necessary
    }
    Ok(())
}

/// `wsdisplay_getscreen`: the type and emulation of screen `sd.idx` (the focused one for a
/// negative index).
fn wsdisplay_getscreen(sc: &WsdisplaySoftc, sd: &mut WsdisplayAddscreendata) -> Result<(), Errno> {
    if sd.idx < 0 && sc.sc_focus.get().is_some() {
        sd.idx = sc.sc_focusidx.get();
    }

    if sd.idx < 0 || sd.idx >= WSDISPLAY_MAXSCREEN {
        return Err(Errno::EINVAL);
    }

    let scr = sc.scr(sd.idx).ok_or(Errno::ENXIO)?;

    let dconf = scr.dconf();
    strlcpy_into(&mut sd.screentype, dconf.scrdata().name());
    strlcpy_into(
        &mut sd.emul[..WSEMUL_NAME_SIZE],
        dconf.wsemul.get().map_or(&b""[..], |e| e.name()),
    );

    Ok(())
}

/// `wsdisplay_closescreen`: hang the screen's tty up and revoke its vnodes.
fn wsdisplay_closescreen(sc: &WsdisplaySoftc, scr: &Wsscreen) {
    // hangup
    if let Some(tp) = scr.tty() {
        let _ = (linesw(tp).l_modem)(tp, 0);
    }

    // locate the major number
    let maj = wsdisplay_major();
    // locate the screen index
    let idx = (0..WSDISPLAY_MAXSCREEN)
        .find(|&i| sc.scr(i).is_some_and(|s| ptr::eq(s, scr)))
        .unwrap_or(WSDISPLAY_MAXSCREEN);
    #[cfg(feature = "diagnostic")]
    if idx == WSDISPLAY_MAXSCREEN {
        panic(format_args!("wsdisplay_forceclose: bad screen"));
    }

    // nuke the vnodes
    let mn = wsdisplayminor(sc.sc_dv.dv_unit.get(), idx);
    vdevgone(maj, mn, mn, VCHR);
}

/// `wsdisplay_delscreen`: delete screen `idx` (refused for the console's, a USL-managed one,
/// or an open one without `WSDISPLAY_DELSCR_FORCE`).
fn wsdisplay_delscreen(sc: &WsdisplaySoftc, idx: i32, flags: i32) -> Result<(), Errno> {
    if !(0..WSDISPLAY_MAXSCREEN).contains(&idx) {
        return Err(Errno::EINVAL);
    }
    let scrp = sc.sc_scr[idx as usize].get().ok_or(Errno::ENXIO)?;
    // SAFETY: in `sc_scr[]`, so live until the `wsscreen_detach` below.
    let scr = unsafe { scrp.as_ref() };

    if ptr::eq(scr.dconf(), &WSDISPLAY_CONSOLE_CONF)
        || (cfg!(feature = "wsdisplay_compat_usl") && scr.scr_syncops.get().is_some())
        || (scr.scr_flags.get() & SCR_OPEN != 0 && flags & WSDISPLAY_DELSCR_FORCE == 0)
    {
        return Err(Errno::EBUSY);
    }

    wsdisplay_closescreen(sc, scr);

    // delete pointers, so neither device entries nor keyboard input can reference it
    // anymore
    let s = spltty();
    if sc.sc_focus.get() == Some(scrp) {
        sc.sc_focus.set(None);
        if cfg!(feature = "wsdisplay_compat_rawkbd") {
            let _ = wsdisplay_update_rawkbd(sc, None);
        }
    }
    sc.sc_scr[idx as usize].set(None);
    splx(s);

    // Wake up processes waiting for the screen to be activated. Sleepers must check whether
    // the screen still exists.
    if scr.scr_flags.get() & SCR_WAITACTIVE != 0 {
        wakeup(scrp.as_ptr());
    }

    // save a reference to the graphics screen
    let cookie = scr.dconf().emulcookie.get();

    // SAFETY: out of `sc_scr[]` and the focus; nothing else holds it.
    unsafe { wsscreen_detach(scrp) };

    if let Some(free_screen) = sc.ops().free_screen {
        // SAFETY: the screen `alloc_screen` made for the deleted screen, now unused.
        unsafe { free_screen(sc.sc_accesscookie.get(), cookie) };
    }

    if flags & WSDISPLAY_DELSCR_QUIET == 0 {
        printf(format_args!(
            "{}: screen {} deleted\n",
            sc.sc_dv.xname(),
            idx
        ));
    }
    Ok(())
}

/// `wsdisplay_match`: autoconfiguration match.
pub fn wsdisplay_match(_parent: Option<&Device>, match_: &CfMatch, aux: *mut c_void) -> i32 {
    let cf = match_.cfdata();
    // SAFETY: a wsemuldisplaydev parent hands its child a `wsemuldisplaydev_attach_args`.
    let ap = unsafe { &*aux.cast::<WsemuldisplaydevAttachArgs>() };

    if wsemuldisplaydevcf_console(cf) != WSEMULDISPLAYDEVCF_CONSOLE_UNK {
        // If console-ness of device specified, either match exactly (at high priority), or
        // fail.
        return if wsemuldisplaydevcf_console(cf) != 0 && ap.console != 0 {
            10
        } else {
            0
        };
    }

    if wsemuldisplaydevcf_primary(cf) != WSEMULDISPLAYDEVCF_PRIMARY_UNK {
        // If primary-ness of device specified, either match exactly (at high priority), or
        // fail.
        return if wsemuldisplaydevcf_primary(cf) != 0 && ap.primary != 0 {
            10
        } else {
            0
        };
    }

    // If console-ness and primary-ness unspecified, it wins.
    1
}

/// `wsdisplay_activate`: on power-down, back to the console's screen.
pub fn wsdisplay_activate(_self: &Device, act: i32) -> Result<(), Errno> {
    if act == DVACT_POWERDOWN {
        wsdisplay_switchtoconsole();
    }
    Ok(())
}

/// `wsdisplay_detach`: detach a display.
pub fn wsdisplay_detach(self_: &Device, flags: i32) -> Result<(), Errno> {
    let sc = sc_of(self_);

    // We don't support detaching the console display yet.
    if sc.sc_isconsole.get() != 0 {
        return Err(Errno::EBUSY);
    }

    // Delete all screens managed by this display
    for i in 0..WSDISPLAY_MAXSCREEN {
        if sc.scr(i).is_some() {
            let force = if flags & DETACH_FORCE != 0 {
                WSDISPLAY_DELSCR_FORCE
            } else {
                0
            };
            wsdisplay_delscreen(sc, i, WSDISPLAY_DELSCR_QUIET | force)?;
        }
    }

    if HAVE_BURNER_SUPPORT {
        timeout_del(&sc.sc_burner);
    }

    // NWSKBD > 0, NWSMUX > 0: if we are the display of the mux we are attached to,
    // disconnect all input devices from us (wsmux_set_display). XXX If we created a
    // standalone mux (dmux), we should destroy it there, but there is currently no support
    // for this in wsmux.
    if let Some(inp) = sc.sc_input.get() {
        match *inp {}
    }

    if let Some(tq) = sc.sc_taskq.get() {
        sc.sc_taskq.set(None);
        // SAFETY: the display's own queue from `taskq_create`; its screens (and their bell
        // tasks) are gone.
        unsafe { taskq_destroy(NonNull::from(tq)) };
    }

    Ok(())
}

/// `wsemuldisplaydevprint`: print function (for parent devices).
pub fn wsemuldisplaydevprint(_aux: *mut c_void, pnp: Option<&[u8]>) -> i32 {
    if let Some(pnp) = pnp {
        printf(format_args!("wsdisplay at {}", Str(pnp)));
    }
    // don't bother printing " console %d"; it's ugly (#if 0 in C)

    UNCONF
}

/// `wsemuldisplaydevsubmatch`: submatch function (for parent devices): only allow wsdisplay
/// to attach.
pub fn wsemuldisplaydevsubmatch(
    parent: Option<&Device>,
    match_: &CfMatch,
    aux: *mut c_void,
) -> i32 {
    let cf = match_.cfdata();

    if ptr::eq(cf.cf_driver, &WSDISPLAY_CD) {
        return cf
            .cf_attach
            .ca_match
            .map_or(0, |ca_match| ca_match(parent, match_, aux));
    }

    0
}

/// `wsdisplay_attach`: the screens (the console's first), the burner, and the console's
/// device number.
pub fn wsdisplay_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    let sc = sc_of(self_);
    // SAFETY: a wsemuldisplaydev parent hands its child a `wsemuldisplaydev_attach_args`.
    let ap = unsafe { &*aux.cast::<WsemuldisplaydevAttachArgs>() };
    let mut defaultscreens = ap.defaultscreens as i32;
    let mut start = 0;

    // NWSKBD > 0, NWSMUX > 0: the keyboard mux this display reads, the one its `mux`
    // locator names, or a "dmux" of its own.
    let kbdmux = wsemuldisplaydevcf_mux(sc.sc_dv.cfdata());
    if kbdmux >= 0 {
        printf(format_args!(" mux {kbdmux}"));
    }

    sc.sc_isconsole.set(ap.console);
    sc.sc_resumescreen.set(WSDISPLAY_NULLSCREEN);

    // SAFETY: `dv_xname` is written once by `config_attach` before attach and the softc
    // lives for good, so the name's bytes do too.
    let xname: &'static [u8] = unsafe { &*ptr::from_ref(sc.sc_dv.xname().as_bytes()) };
    sc.sc_taskq.set(taskq_create(xname, 1, IPL_TTY, 0));

    if ap.console != 0 {
        crate::kassert!(WSDISPLAY_CONSOLE_INITTED.load(Ordering::Relaxed));
        crate::kassert!(console_device().is_none());

        let Some(scrp) = wsscreen_attach(sc, true, None, ptr::null(), ptr::null_mut(), 0, 0, 0)
        else {
            return;
        };
        sc.sc_scr[0].set(Some(scrp));
        WSDISPLAY_CONSOLE_DEVICE.store(ptr::from_ref(sc).cast_mut(), Ordering::Release);

        printf(format_args!(
            ": console ({}, {} emulation)",
            Str(WSDISPLAY_CONSOLE_CONF.scrdata().name()),
            Str(WSDISPLAY_CONSOLE_CONF
                .wsemul
                .get()
                .map_or(&b""[..], |e| e.name()))
        ));

        // NWSKBD > 0
        if let Some(kme) = wskbd_set_console_display(&sc.sc_dv, sc.sc_input.get()) {
            match *kme {}
        }

        sc.sc_focusidx.set(0);
        sc.sc_focus.set(Some(scrp));
        start = 1;
    }
    printf(format_args!("\n"));

    // NWSKBD > 0 && NWSMUX > 0: wsmux_getmux(kbdmux) or wsmux_create("dmux", unit) for
    // `sc_input`, then, if this mux did not have a display device yet, volunteer for the
    // job (wsmux_set_display).
    let _ = unported!("wsmux_getmux, wsmux_create, wsmux_set_display (wsdisplay_attach)");

    sc.sc_accessops.set(Some(ap.accessops));
    sc.sc_accesscookie.set(ap.accesscookie);
    sc.sc_scrdata.set(ap.scrdata);

    // Set up a number of virtual screens if wanted. The WSDISPLAYIO_ADDSCREEN ioctl is more
    // flexible, so this code is for special cases like installation kernels, as well as sane
    // multihead defaults.
    if defaultscreens == 0 {
        defaultscreens = wsdisplay_defaultscreens();
    }
    let mut i = start;
    while i < defaultscreens {
        if wsdisplay_addscreen(sc, i, None, None).is_err() {
            break;
        }
        i += 1;
    }

    if i > start {
        wsdisplay_addscreen_print(sc, start, i - start);
    }

    if HAVE_BURNER_SUPPORT {
        sc.sc_burnoutintvl.set(WSDISPLAY_DEFBURNOUT_MSEC);
        sc.sc_burninintvl.set(WSDISPLAY_DEFBURNIN_MSEC);
        sc.sc_burnflags
            .set(WSDISPLAY_BURN_OUTPUT | WSDISPLAY_BURN_KBD | WSDISPLAY_BURN_MOUSE);
        timeout_set(&sc.sc_burner, wsdisplay_burner, sc.as_arg());
        sc.sc_burnout.set(sc.sc_burnoutintvl.get());
        wsdisplay_burn(sc.as_arg(), sc.sc_burnflags.get());
    }

    if ap.console != 0 && cn_tab().is_some_and(|cn| ptr::eq(cn, &WSDISPLAY_CONS)) {
        // locate the major number
        let maj = wsdisplay_major();
        WSDISPLAY_CONS
            .cn_dev
            .set(makedev(maj, wsdisplayminor(self_.dv_unit.get(), 0)));
    }
}

/// `wsdisplay_cnattach`: make screen type `type_`, drawn through the driver's screen
/// `cookie`, the console, with the cursor at (`ccol`, `crow`) and default attribute
/// `defattr`. The first call also makes wsdisplay the console device (`cn_tab`).
///
/// # Safety
///
/// `cookie` is the screen cookie the driver pairs with `type_.textops` (its `rasops_info`
/// or active screen), valid for good.
pub unsafe fn wsdisplay_cnattach(
    type_: &'static WsscreenDescr,
    cookie: *mut c_void,
    ccol: i32,
    crow: i32,
    defattr: u32,
) {
    crate::kassert!(type_.nrows > 0);
    crate::kassert!(type_.ncols > 0);
    crate::kassert!(crow < type_.nrows);
    crate::kassert!(ccol < type_.ncols);

    let conf = &WSDISPLAY_CONSOLE_CONF;
    conf.emulops.set(type_.textops);
    conf.emulcookie.set(cookie);
    conf.scrdata.set(type_);
    let emulops = conf.emulops();

    // If the emulops structure is crippled, force a dumb emulation (WSEMUL_DUMB).
    let wsemul = if cfg!(feature = "wsemul_dumb")
        && (emulops.cursor.is_none()
            || emulops.copycols.is_none()
            || emulops.copyrows.is_none()
            || emulops.erasecols.is_none()
            || emulops.eraserows.is_none())
    {
        wsemul_pick(Some(b"dumb"))
    } else {
        wsemul_pick(Some(b""))
    };
    // The C dereferences the emulation unchecked; without one there is no console here.
    let Some(wsemul) = wsemul else {
        return;
    };
    conf.wsemul.set(Some(wsemul));
    // SAFETY: the caller's contract: `cookie` goes with `type_`'s emulops.
    let c = unsafe { (wsemul.cnattach)(type_, cookie, ccol, crow, defattr) };
    conf.wsemulcookie.set(c);

    if !WSDISPLAY_CONSOLE_INITTED.load(Ordering::Relaxed) {
        set_cn_tab(&WSDISPLAY_CONS);
    }

    WSDISPLAY_CONSOLE_INITTED.store(true, Ordering::Release);

    // DDB
    db_resize(type_.ncols, type_.nrows);
}

/// The display and screen of a minor that names one, as the tty entry points look them up.
fn dev_screen(dev: Dev) -> Result<(&'static WsdisplaySoftc, &'static Wsscreen), Errno> {
    let sc = wsdisplay_sc(wsdisplayunit(dev)).ok_or(Errno::ENXIO)?;
    let scr = sc.scr(wsdisplayscreen(dev)).ok_or(Errno::ENXIO)?;
    Ok((sc, scr))
}

/// `wsdisplayopen`: open a screen's tty (the control device needs nothing).
pub fn wsdisplayopen(dev: Dev, _flag: i32, _mode: i32, p: &Proc) -> Result<(), Errno> {
    let unit = wsdisplayunit(dev);
    // make sure it was attached
    let sc = if unit >= WSDISPLAY_CD.cd_ndevs.get() {
        None
    } else {
        wsdisplay_sc(unit)
    };
    let Some(sc) = sc else {
        return Err(Errno::ENXIO);
    };

    if iswsdisplayctl(dev) {
        return Ok(());
    }

    if wsdisplayscreen(dev) >= WSDISPLAY_MAXSCREEN {
        return Err(Errno::ENXIO);
    }
    let scr = sc.scr(wsdisplayscreen(dev)).ok_or(Errno::ENXIO)?;

    if let Some(tp) = scr.tty() {
        tp.t_oproc.set(Some(wsdisplaystart));
        tp.t_param.set(Some(wsdisplayparam));
        tp.t_dev.set(dev);
        let newopen = !tp.t_state_isset(TS_ISOPEN);
        if newopen {
            ttychars(tp);
            tp.set_t_iflag(TTYDEF_IFLAG);
            tp.set_t_oflag(TTYDEF_OFLAG);
            tp.set_t_cflag(TTYDEF_CFLAG);
            tp.set_t_lflag(TTYDEF_LFLAG);
            tp.set_t_ispeed(TTYDEF_SPEED as i32);
            tp.set_t_ospeed(TTYDEF_SPEED as i32);
            let _ = wsdisplayparam(tp, &tp.t_termios.get());
            ttsetwater(tp);
        } else if tp.t_state_isset(TS_XCLUDE) && suser(p).is_err() {
            return Err(Errno::EBUSY);
        }
        tp.t_state_set(TS_CARR_ON);

        (linesw(tp).l_open)(dev, tp, p)?;

        if newopen {
            // set window sizes as appropriate, and reset the emulation
            let mut ws = tp.t_winsize.get();
            let scrdata = scr.dconf().scrdata();
            ws.ws_row = scrdata.nrows as u16;
            ws.ws_col = scrdata.ncols as u16;
            tp.t_winsize.set(ws);
        }
    }

    scr.scr_flags.set(scr.scr_flags.get() | SCR_OPEN);
    Ok(())
}

/// `wsdisplayclose`: the last close of a screen's tty: hold released, the USL owner gone,
/// back to text mode, the emulation reset (and the screen cleared with
/// `wsdisplay_clearonclose`), the keyboard back to translated mode, the selection erased.
pub fn wsdisplayclose(dev: Dev, flag: i32, _mode: i32, p: Option<&Proc>) -> Result<(), Errno> {
    if iswsdisplayctl(dev) {
        return Ok(());
    }

    let (sc, scr) = dev_screen(dev)?;

    if let Some(tp) = scr.tty() {
        if scr.scr_hold_screen.get() != 0 {
            // XXX RESET KEYBOARD LEDS, etc.
            let s = spltty(); // avoid conflict with keyboard
            wsdisplay_kbdholdscr(scr, 0);
            splx(s);
        }
        let _ = (linesw(tp).l_close)(tp, flag, p);
        let _ = ttyclose(tp);
    }

    if cfg!(feature = "wsdisplay_compat_usl")
        && let Some(ops) = scr.scr_syncops.get()
    {
        // SAFETY: the cookie the compatibility code registered with these syncops.
        unsafe { (ops.destroy)(scr.scr_synccookie.get()) };
    }

    scr.scr_flags.set(scr.scr_flags.get() & !SCR_GRAPHICS);
    scr.dconf().reset(WSEMUL_RESET);
    if wsdisplay_clearonclose() != 0 {
        scr.dconf().reset(WSEMUL_CLEARSCREEN);
    }

    if cfg!(feature = "wsdisplay_compat_rawkbd") && scr.scr_rawkbd.get() != 0 {
        let mut kbmode = WSKBD_TRANSLATED.to_ne_bytes();
        let _ = wsdisplay_internal_ioctl(sc, scr, WSKBDIO_SETMODE, &mut kbmode, FWRITE, p);
    }

    scr.scr_flags.set(scr.scr_flags.get() & !SCR_OPEN);

    if HAVE_WSMOUSED_SUPPORT {
        // remove the selection at logout
        if let Some(buf) = sc.sc_copybuffer.get() {
            // SAFETY: the copy buffer, `sc_copybuffer_size` bytes from malloc.
            unsafe { ptr::write_bytes(buf.as_ptr(), 0, sc.sc_copybuffer_size.get() as usize) };
        }
        sc.clr_flag(SC_PASTE_AVAIL);
    }

    Ok(())
}

/// `wsdisplayread`.
pub fn wsdisplayread(dev: Dev, uio: &mut crate::sys::uio::Uio<'_>, flag: i32) -> Result<(), Errno> {
    if iswsdisplayctl(dev) {
        return Ok(());
    }

    let (_sc, scr) = dev_screen(dev)?;

    let tp = scr.tty().ok_or(Errno::ENODEV)?;
    (linesw(tp).l_read)(tp, uio, flag)
}

/// `wsdisplaywrite`.
pub fn wsdisplaywrite(
    dev: Dev,
    uio: &mut crate::sys::uio::Uio<'_>,
    flag: i32,
) -> Result<(), Errno> {
    if iswsdisplayctl(dev) {
        return Ok(());
    }

    let (_sc, scr) = dev_screen(dev)?;

    let tp = scr.tty().ok_or(Errno::ENODEV)?;
    (linesw(tp).l_write)(tp, uio, flag)
}

/// `wsdisplaytty`: a screen's tty; panics for the control device, which has none.
pub fn wsdisplaytty(dev: Dev) -> Option<&'static Tty> {
    let sc = wsdisplay_sc(wsdisplayunit(dev))?;

    if iswsdisplayctl(dev) {
        panic(format_args!("wsdisplaytty() on ctl device"));
    }

    sc.scr(wsdisplayscreen(dev))?.tty()
}

/// `wsdisplayioctl`: the USL ioctls, then the control device's or the screen's (line
/// discipline, tty, USL, wsdisplay, driver).
pub fn wsdisplayioctl(
    dev: Dev,
    cmd: u64,
    data: &mut [u8],
    flag: i32,
    p: &Proc,
) -> Result<(), Errno> {
    let unit = wsdisplayunit(dev);
    let sc = wsdisplay_sc(unit).ok_or(Errno::ENXIO)?;
    let mut dev = dev;

    if cfg!(feature = "wsdisplay_compat_usl") && wsdisplay_usl_ioctl1(sc, cmd, data, flag, p)? {
        return Ok(());
    }

    if iswsdisplayctl(dev) {
        match cmd {
            WSDISPLAYIO_GTYPE | WSDISPLAYIO_GETSCREENTYPE => {
                // pass to the first screen
                dev = makedev(major(dev), wsdisplayminor(unit, 0));
            }
            _ => return wsdisplay_cfg_ioctl(sc, cmd, data, flag, Some(p)),
        }
    }

    if wsdisplayscreen(dev) >= WSDISPLAY_MAXSCREEN {
        return Err(Errno::ENODEV);
    }

    let scr = sc.scr(wsdisplayscreen(dev)).ok_or(Errno::ENXIO)?;

    if let Some(tp) = scr.tty() {
        // do the line discipline ioctls first
        if (linesw(tp).l_ioctl)(tp, cmd, data, flag, p)? {
            return Ok(());
        }

        // then the tty ioctls
        if ttioctl(tp, cmd, data, flag, p)? {
            return Ok(());
        }
    }

    if cfg!(feature = "wsdisplay_compat_usl") && wsdisplay_usl_ioctl2(sc, scr, cmd, data, flag, p)?
    {
        return Ok(());
    }

    match wsdisplay_internal_ioctl(sc, scr, cmd, data, flag, Some(p)) {
        Ok(true) => Ok(()),
        Ok(false) => Err(Errno::ENOTTY),
        Err(e) => Err(e),
    }
}

/// `wsdisplay_param`: the `WSDISPLAYIO_GETPARAM`/`SETPARAM` of display `dev`, as the
/// driver answers it (`ENOTTY` where the driver's ioctl says it is not its command).
pub fn wsdisplay_param(dev: &Device, cmd: u64, dp: &mut WsdisplayParam) -> Result<(), Errno> {
    let sc = sc_of(dev);
    let mut data = [0u8; size_of::<WsdisplayParam>()];
    ioctl_ret(&mut data, dp);
    let r = wsdisplay_driver_ioctl(sc, cmd, &mut data, 0, None);
    *dp = ioctl_arg(&data);
    match r {
        Ok(true) => Ok(()),
        Ok(false) => Err(Errno::ENOTTY),
        Err(e) => Err(e),
    }
}

/// `wsdisplay_internal_ioctl`: the ioctls of a screen (`Ok(false)` where the C returns -1:
/// not the display's either).
pub fn wsdisplay_internal_ioctl(
    sc: &WsdisplaySoftc,
    scr: &Wsscreen,
    cmd: u64,
    data: &mut [u8],
    flag: i32,
    p: Option<&Proc>,
) -> Result<bool, Errno> {
    // NWSKBD > 0
    if cfg!(feature = "wsdisplay_compat_rawkbd") {
        match cmd {
            WSKBDIO_SETMODE => {
                if flag & FWRITE == 0 {
                    return Err(Errno::EACCES);
                }
                scr.scr_rawkbd
                    .set(i32::from(ioctl_arg::<i32>(data) == WSKBD_RAW));
                return wsdisplay_update_rawkbd(sc, Some(scr)).map(|()| true);
            }
            WSKBDIO_GETMODE => {
                let mode = if scr.scr_rawkbd.get() != 0 {
                    WSKBD_RAW
                } else {
                    WSKBD_TRANSLATED
                };
                ioctl_ret(data, &mode);
                return Ok(true);
            }
            _ => {}
        }
    }
    if let Some(inp) = sc.sc_input.get() {
        // wsevsrc_display_ioctl(inp, cmd, data, flag, p)
        match *inp {}
    }

    let burner_cmd = HAVE_BURNER_SUPPORT && matches!(cmd, WSDISPLAYIO_SVIDEO | WSDISPLAYIO_SBURNER);
    if (matches!(
        cmd,
        WSDISPLAYIO_SMODE | WSDISPLAYIO_USEFONT | WSDISPLAYIO_SETSCREEN
    ) || burner_cmd)
        && flag & FWRITE == 0
    {
        return Err(Errno::EACCES);
    }

    match cmd {
        WSDISPLAYIO_GMODE => {
            let f = scr.scr_flags.get();
            let mode = if f & SCR_GRAPHICS != 0 {
                if f & SCR_DUMBFB != 0 {
                    WSDISPLAYIO_MODE_DUMBFB
                } else {
                    WSDISPLAYIO_MODE_MAPPED
                }
            } else {
                WSDISPLAYIO_MODE_EMUL
            };
            ioctl_ret(data, &mode);
            return Ok(true);
        }

        WSDISPLAYIO_SMODE => {
            let d = ioctl_arg::<u32>(data);
            if d != WSDISPLAYIO_MODE_EMUL
                && d != WSDISPLAYIO_MODE_MAPPED
                && d != WSDISPLAYIO_MODE_DUMBFB
            {
                return Err(Errno::EINVAL);
            }

            scr.scr_flags.set(scr.scr_flags.get() & !SCR_GRAPHICS);
            if d == WSDISPLAYIO_MODE_MAPPED || d == WSDISPLAYIO_MODE_DUMBFB {
                let dumb = if d == WSDISPLAYIO_MODE_DUMBFB {
                    SCR_DUMBFB
                } else {
                    0
                };
                scr.scr_flags.set(scr.scr_flags.get() | SCR_GRAPHICS | dumb);

                // clear cursor
                scr.dconf().reset(WSEMUL_CLEARCURSOR);
            }

            if HAVE_BURNER_SUPPORT {
                wsdisplay_burner_setup(sc, scr);
            }

            let _ = sc.driver_ioctl(cmd, data, flag, p);

            return Ok(true);
        }

        WSDISPLAYIO_USEFONT => {
            let Some(load_font) = sc.ops().load_font else {
                return Err(Errno::EINVAL);
            };
            let mut d: WsdisplayFont = ioctl_arg(data);
            d.data = ptr::null_mut();
            // SAFETY: the access cookie and the screen's cookie, paired with these
            // accessops; a font with no data names one to use.
            let r = unsafe {
                load_font(
                    sc.sc_accesscookie.get(),
                    scr.dconf().emulcookie.get(),
                    &mut d,
                )
            };
            ioctl_ret(data, &d);
            if r.is_ok() {
                scr.dconf().reset(WSEMUL_SYNCFONT);
            }
            return r.map(|()| true);
        }

        WSDISPLAYIO_GVIDEO if HAVE_BURNER_SUPPORT => {
            ioctl_ret(data, &u32::from(sc.sc_burnman.get() == 0));
            // break: on to the driver's ioctl
        }

        WSDISPLAYIO_SVIDEO if HAVE_BURNER_SUPPORT => {
            let v = ioctl_arg::<u32>(data);
            if v != WSDISPLAYIO_VIDEO_OFF && v != WSDISPLAYIO_VIDEO_ON {
                return Err(Errno::EINVAL);
            }
            let Some(burn_screen) = sc.ops().burn_screen else {
                return Err(Errno::EOPNOTSUPP);
            };
            // SAFETY: the access cookie paired with these accessops.
            unsafe { burn_screen(sc.sc_accesscookie.get(), v, sc.sc_burnflags.get()) };
            sc.sc_burnman.set(i32::from(v == WSDISPLAYIO_VIDEO_OFF));
            // break: on to the driver's ioctl
        }

        WSDISPLAYIO_GBURNER if HAVE_BURNER_SUPPORT => {
            let d = WsdisplayBurner {
                on: sc.sc_burninintvl.get() as u32,
                off: sc.sc_burnoutintvl.get() as u32,
                flags: sc.sc_burnflags.get(),
            };
            ioctl_ret(data, &d);
            return Ok(true);
        }

        WSDISPLAYIO_SBURNER if HAVE_BURNER_SUPPORT => {
            let d: WsdisplayBurner = ioctl_arg(data);

            if d.flags
                & !(WSDISPLAY_BURN_VBLANK
                    | WSDISPLAY_BURN_KBD
                    | WSDISPLAY_BURN_MOUSE
                    | WSDISPLAY_BURN_OUTPUT)
                != 0
            {
                return Err(Errno::EINVAL);
            }

            sc.sc_burnflags.set(d.flags);
            // disable timeout if necessary
            if (d.off == 0
                || sc.sc_burnflags.get()
                    & (WSDISPLAY_BURN_OUTPUT | WSDISPLAY_BURN_KBD | WSDISPLAY_BURN_MOUSE)
                    == 0)
                && sc.sc_burnout.get() != 0
            {
                timeout_del(&sc.sc_burner);
            }

            let active = sc.focus().unwrap_or(scr);

            if d.on != 0 {
                sc.sc_burninintvl.set(d.on as i32);
                if sc.sc_burnman.get() != 0 {
                    sc.sc_burnout.set(sc.sc_burninintvl.get());
                    // reinit timeout if changed
                    if active.scr_flags.get() & SCR_GRAPHICS == 0 {
                        wsdisplay_burn(sc.as_arg(), sc.sc_burnflags.get());
                    }
                }
            }
            sc.sc_burnoutintvl.set(d.off as i32);
            if sc.sc_burnman.get() == 0 {
                sc.sc_burnout.set(sc.sc_burnoutintvl.get());
                // reinit timeout if changed
                if active.scr_flags.get() & SCR_GRAPHICS == 0 {
                    wsdisplay_burn(sc.as_arg(), sc.sc_burnflags.get());
                }
            }
            return Ok(true);
        }

        WSDISPLAYIO_GETSCREEN => {
            let mut sd: WsdisplayAddscreendata = ioctl_arg(data);
            let r = wsdisplay_getscreen(sc, &mut sd);
            ioctl_ret(data, &sd);
            return r.map(|()| true);
        }

        WSDISPLAYIO_SETSCREEN => {
            return wsdisplay_switch(&sc.sc_dv, ioctl_arg::<i32>(data), 1).map(|()| true);
        }

        WSDISPLAYIO_GETSCREENTYPE => {
            let mut d: WsdisplayScreentype = ioctl_arg(data);
            let list = sc.scrdata();
            if d.idx < 0 || d.idx >= list.nscreens {
                return Err(Errno::EINVAL);
            }

            d.nidx = list.nscreens;
            // SAFETY: the display's screen types, which live for good.
            let ty = unsafe { &*list.screens()[d.idx as usize] };
            strlcpy_into(&mut d.name, ty.name());
            d.ncols = ty.ncols;
            d.nrows = ty.nrows;
            d.fontwidth = ty.fontwidth;
            d.fontheight = ty.fontheight;
            ioctl_ret(data, &d);
            return Ok(true);
        }

        WSDISPLAYIO_GETEMULTYPE => {
            let mut d: WsdisplayEmultype = ioctl_arg(data);
            let Some(name) = wsemul_getname(d.idx) else {
                return Err(Errno::EINVAL);
            };
            strlcpy_into(&mut d.name[..WSEMUL_NAME_SIZE], name);
            ioctl_ret(data, &d);
            return Ok(true);
        }

        _ => {}
    }

    // check ioctls for display
    wsdisplay_driver_ioctl(sc, cmd, data, flag, p)
}

/// `wsdisplay_driver_ioctl`: the display driver's ioctl, hiding parameters with empty ranges
/// from userland.
fn wsdisplay_driver_ioctl(
    sc: &WsdisplaySoftc,
    cmd: u64,
    data: &mut [u8],
    flag: i32,
    p: Option<&Proc>,
) -> Result<bool, Errno> {
    let r = sc.driver_ioctl(cmd, data, flag, p);
    // Do not report parameters with empty ranges to userland.
    if r == Ok(true) && cmd == WSDISPLAYIO_GETPARAM {
        let dp: WsdisplayParam = ioctl_arg(data);
        match dp.param {
            WSDISPLAYIO_PARAM_BACKLIGHT
            | WSDISPLAYIO_PARAM_BRIGHTNESS
            | WSDISPLAYIO_PARAM_CONTRAST
                if dp.min == dp.max =>
            {
                return Err(Errno::ENOTTY);
            }
            _ => {}
        }
    }

    r
}

/// `wsdisplay_cfg_ioctl`: the control device's ioctls (`/dev/ttyCcfg`).
pub fn wsdisplay_cfg_ioctl(
    sc: &'static WsdisplaySoftc,
    cmd: u64,
    data: &mut [u8],
    flag: i32,
    p: Option<&Proc>,
) -> Result<(), Errno> {
    match cmd {
        WSDISPLAYIO_WSMOUSED if HAVE_WSMOUSED_SUPPORT => wsmoused(sc, data, flag, p),
        WSDISPLAYIO_ADDSCREEN => {
            let d: WsdisplayAddscreendata = ioctl_arg(data);
            wsdisplay_addscreen(sc, d.idx, Some(&d.screentype), Some(&d.emul))?;
            wsdisplay_addscreen_print(sc, d.idx, 0);
            Ok(())
        }
        WSDISPLAYIO_DELSCREEN => {
            let d: WsdisplayDelscreendata = ioctl_arg(data);
            wsdisplay_delscreen(sc, d.idx, d.flags)
        }
        WSDISPLAYIO_GETSCREEN => {
            let mut sd: WsdisplayAddscreendata = ioctl_arg(data);
            let r = wsdisplay_getscreen(sc, &mut sd);
            ioctl_ret(data, &sd);
            r
        }
        WSDISPLAYIO_SETSCREEN => wsdisplay_switch(&sc.sc_dv, ioctl_arg::<i32>(data), 1),
        WSDISPLAYIO_LDFONT => {
            let Some(load_font) = sc.ops().load_font else {
                return Err(Errno::EINVAL);
            };
            let mut d: WsdisplayFont = ioctl_arg(data);
            if d.fontheight > 64 || d.stride > 8 {
                // 64x64 pixels
                return Err(Errno::EINVAL);
            }
            if d.numchars > 65536 {
                // unicode plane
                return Err(Errno::EINVAL);
            }
            // Some mapchar emulops, such as rasops_mapchar, require the availability of the
            // question mark character to use for missing glyphs, so make sure it exists.
            if d.firstchar > i32::from(b'?')
                || d.firstchar.wrapping_add(d.numchars) <= i32::from(b'?')
            {
                return Err(Errno::EINVAL);
            }
            let fontsz = (d.fontheight as usize)
                .wrapping_mul(d.stride as usize)
                .wrapping_mul(d.numchars.max(0) as usize);
            if fontsz > WSDISPLAY_MAXFONTSZ as usize {
                return Err(Errno::EINVAL);
            }

            let Some(buf) = malloc(fontsz, M_DEVBUF, M_WAITOK) else {
                return Err(Errno::ENOMEM);
            };
            // SAFETY: a fresh allocation of `fontsz` bytes, ours.
            let kbuf = unsafe { core::slice::from_raw_parts_mut(buf.as_ptr(), fontsz) };
            if let Err(e) = copyin(d.data as usize, kbuf) {
                free(buf, M_DEVBUF, fontsz);
                return Err(e);
            }
            d.data = buf.as_ptr().cast();
            // SAFETY: the access cookie paired with these accessops; the font's glyphs are
            // the kernel buffer above, which the driver keeps on success.
            let r = unsafe { load_font(sc.sc_accesscookie.get(), ptr::null_mut(), &mut d) };
            if r.is_err() {
                free(buf, M_DEVBUF, fontsz);
            }
            ioctl_ret(data, &d);
            r
        }

        WSDISPLAYIO_LSFONT => {
            let Some(list_font) = sc.ops().list_font else {
                return Err(Errno::EINVAL);
            };
            let mut d: WsdisplayFont = ioctl_arg(data);
            // SAFETY: the access cookie paired with these accessops.
            let r = unsafe { list_font(sc.sc_accesscookie.get(), &mut d) };
            ioctl_ret(data, &d);
            r
        }

        WSDISPLAYIO_DELFONT => Err(Errno::EINVAL),

        // NWSKBD > 0
        WSMUXIO_ADD_DEVICE | WSMUXIO_INJECTEVENT | WSMUXIO_REMOVE_DEVICE | WSMUXIO_LIST_DEVICES => {
            if cmd == WSMUXIO_ADD_DEVICE {
                let mut d: WsmuxDevice = ioctl_arg(data);
                if d.idx == -1 && d.type_ == WSMUX_KBD {
                    d.idx = wskbd_pickfree();
                }
                ioctl_ret(data, &d);
            }
            let Some(inp) = sc.sc_input.get() else {
                return Err(Errno::ENXIO);
            };
            // wsevsrc_ioctl(inp, cmd, data, flag, p)
            match *inp {}
        }

        _ => Err(Errno::EINVAL),
    }
}

/// `wsdisplaymmap`: the display's `mmap`, for a screen in graphics mode.
pub fn wsdisplaymmap(dev: Dev, offset: i64, prot: i32) -> Option<Paddr> {
    let sc = wsdisplay_sc(wsdisplayunit(dev))?;

    if iswsdisplayctl(dev) {
        return None;
    }

    let scr = sc.scr(wsdisplayscreen(dev))?;

    if scr.scr_flags.get() & SCR_GRAPHICS == 0 {
        return None;
    }

    // pass mmap to display
    let mmap = sc.ops().mmap?;
    // SAFETY: the access cookie paired with these accessops.
    unsafe { mmap(sc.sc_accesscookie.get(), offset, prot) }
}

/// `wsdisplaykqfilter`: a screen's tty's.
pub fn wsdisplaykqfilter(dev: Dev, kn: &crate::sys::event::Knote) -> Result<(), Errno> {
    if iswsdisplayctl(dev) {
        return Err(Errno::ENXIO);
    }

    let (_sc, scr) = dev_screen(dev)?;

    if scr.tty().is_none() {
        return Err(Errno::ENXIO);
    }

    crate::kern::tty::ttkqfilter(dev, kn)
}

/// The contiguous bytes at the head of a tty's output queue (`ndqb(&tp->t_outq, 0)` of them
/// at `tp->t_outq.c_cf`).
fn outq_chunk(tp: &Tty) -> &[u8] {
    let q = &tp.t_outq;
    let n = ndqb(q, 0);
    if n <= 0 || q.c_cs.get().is_null() {
        return &[];
    }
    // SAFETY: `ndqb` counted `n` characters from `c_cf` up to the ring's end or `c_cl`,
    // inside the `c_cn` bytes at `c_cs`; while `TS_BUSY` is set only this driver takes them
    // off the queue, and writers append past them.
    unsafe { core::slice::from_raw_parts(q.c_cs.get().add(q.c_cf.get()), n as usize) }
}

/// `wsdisplaystart`: drain a screen's output queue into its emulation.
pub fn wsdisplaystart(tp: &Tty) {
    let unit = wsdisplayunit(tp.t_dev.get());
    let sc = if unit >= WSDISPLAY_CD.cd_ndevs.get() {
        None
    } else {
        wsdisplay_sc(unit)
    };
    let Some(sc) = sc else {
        return;
    };

    let s = spltty();
    'low: {
        if tp.t_state_isset(TS_TIMEOUT | TS_BUSY | TS_TTSTOP) {
            splx(s);
            return;
        }
        if tp.t_outq.c_cc.get() == 0 {
            break 'low;
        }

        let Some(scr) = sc.scr(wsdisplayscreen(tp.t_dev.get())) else {
            splx(s);
            return;
        };
        if scr.scr_hold_screen.get() != 0 {
            tp.t_state_set(TS_TIMEOUT);
            splx(s);
            return;
        }
        tp.t_state_set(TS_BUSY);
        splx(s);

        // Drain output from ring buffer. The output will normally be in one contiguous
        // chunk, but when the ring wraps, it will be in two pieces.. one at the end of the
        // ring, the other at the start. For performance, rather than loop here, we output
        // one chunk, see if there's another one, and if so, output it too.

        let buf = outq_chunk(tp);
        let n = buf.len() as u32;
        let done = if scr.scr_flags.get() & SCR_GRAPHICS == 0 {
            if HAVE_BURNER_SUPPORT {
                wsdisplay_burn(sc.as_arg(), WSDISPLAY_BURN_OUTPUT);
            }
            if HAVE_WSMOUSED_SUPPORT && sc.focus().is_some_and(|f| ptr::eq(f, scr)) {
                mouse_remove(scr);
            }
            scr.dconf().output(buf, false)
        } else {
            n
        };
        ndflush(&tp.t_outq, done as i32);

        if done == n {
            let buf = outq_chunk(tp);
            if !buf.is_empty() {
                let n = buf.len() as u32;
                let done = if scr.scr_flags.get() & SCR_GRAPHICS == 0 {
                    scr.dconf().output(buf, false)
                } else {
                    n
                };
                ndflush(&tp.t_outq, done as i32);
            }
        }

        let s2 = spltty();
        tp.t_state_clr(TS_BUSY);
        // Come back if there's more to do
        if tp.t_outq.c_cc.get() != 0 {
            tp.t_state_set(TS_TIMEOUT);
            let hz = HZ.load(Ordering::Relaxed);
            timeout_add(&tp.t_rstrt_to, if hz > 128 { hz / 128 } else { 1 });
        }
        ttwakeupwr(tp);
        splx(s2);
        return;
    }
    // low:
    ttwakeupwr(tp);
    splx(s);
}

/// `wsdisplaystop`.
pub fn wsdisplaystop(tp: &Tty, _flag: i32) -> Result<(), Errno> {
    let s = spltty();
    if tp.t_state_isset(TS_BUSY) && !tp.t_state_isset(TS_TTSTOP) {
        tp.t_state_set(TS_FLUSH);
    }
    splx(s);

    Ok(())
}

/// `wsdisplayparam`: set line parameters.
pub fn wsdisplayparam(tp: &Tty, t: &Termios) -> Result<(), Errno> {
    tp.set_t_ispeed(t.c_ispeed);
    tp.set_t_ospeed(t.c_ospeed);
    tp.set_t_cflag(t.c_cflag);
    Ok(())
}

/// `wsdisplay_emulbell`: callback from the emulation code: ring the bell of screen `v`
/// (from a task: the keyboard's bell may sleep).
pub fn wsdisplay_emulbell(v: *mut c_void) {
    // SAFETY: NULL (the console, before its real attach) or the screen `wsscreen_attach`
    // gave the emulation as its callback cookie, which outlives the emulation.
    let Some(scr) = (unsafe { v.cast::<Wsscreen>().as_ref() }) else {
        return; // console, before real attach
    };

    if scr.scr_flags.get() & SCR_GRAPHICS != 0 {
        // can this happen?
        return;
    }

    if let Some(tq) = scr.sc().sc_taskq.get() {
        // SAFETY: the screen lives until `wsscreen_detach`, which waits for the task
        // (`taskq_del_barrier`) before freeing it.
        let task: &'static Task = unsafe { &*ptr::from_ref(&scr.scr_emulbell_task) };
        let _ = task_add(tq, task);
    }
}

/// `wsdisplay_emulbell_task`: the bell, through the keyboard's `WSKBDIO_BELL`.
fn wsdisplay_emulbell_task(v: *mut c_void) {
    // SAFETY: the screen `wsscreen_attach` set the task up with, alive while it is queued.
    let scr = unsafe { &*v.cast::<Wsscreen>() };

    let _ = wsdisplay_internal_ioctl(scr.sc(), scr, WSKBDIO_BELL, &mut [], FWRITE, None);
}

/// `wsdisplay_emulinput`: callback from the emulation code: `data` (a terminal's answer)
/// as input of screen `v`'s tty (`!WSEMUL_NO_VT100`).
pub fn wsdisplay_emulinput(v: *mut c_void, data: &[u8]) {
    // SAFETY: as in `wsdisplay_emulbell`.
    let Some(scr) = (unsafe { v.cast::<Wsscreen>().as_ref() }) else {
        return; // console, before real attach
    };

    if scr.scr_flags.get() & SCR_GRAPHICS != 0 {
        // XXX can't happen
        return;
    }
    let Some(tp) = scr.tty() else {
        return;
    };

    for &c in data {
        let _ = (linesw(tp).l_rint)(i32::from(c), tp);
    }
}

/// `wsdisplay_kbdinput`: calls from the keyboard interface: keysyms `ks` in layout
/// `layout`, translated by the focused screen's emulation, as its tty's input.
pub fn wsdisplay_kbdinput(dev: &Device, layout: KbdT, ks: &[KeysymT]) {
    let sc = sc_of(dev);

    let Some(scr) = sc.focus() else {
        return;
    };
    let Some(tp) = scr.tty() else {
        return;
    };

    let dconf = scr.dconf();
    let Some(em) = dconf.wsemul.get() else {
        return;
    };
    for &k in ks {
        let mut buf = [0u8; WSEMUL_TRANSLATE_SIZE];
        // SAFETY: the emulation state this emulation made for the screen.
        let dp = unsafe { (em.translate)(dconf.wsemulcookie.get(), layout, k, &mut buf) };
        for &c in dp {
            let _ = (linesw(tp).l_rint)(i32::from(c), tp);
        }
    }
}

/// `wsdisplay_rawkbdinput`: raw scancodes as the focused screen's tty's input
/// (`WSDISPLAY_COMPAT_RAWKBD`).
pub fn wsdisplay_rawkbdinput(dev: &Device, buf: &[u8]) {
    let sc = sc_of(dev);

    let Some(scr) = sc.focus() else {
        return;
    };
    let Some(tp) = scr.tty() else {
        return;
    };

    for &c in buf {
        let _ = (linesw(tp).l_rint)(i32::from(c), tp);
    }
}

/// `wsdisplay_update_rawkbd`: put the keyboard in the raw or translated mode the focused
/// screen wants (`WSDISPLAY_COMPAT_RAWKBD`).
fn wsdisplay_update_rawkbd(sc: &WsdisplaySoftc, scr: Option<&Wsscreen>) -> Result<(), Errno> {
    // NWSKBD > 0
    let s = spltty();

    let raw = scr.map_or(0, |s| s.scr_rawkbd.get());

    let focused = match (scr, sc.focus()) {
        (Some(a), Some(b)) => ptr::eq(a, b),
        (None, None) => true,
        _ => false,
    };
    if !focused || sc.sc_rawkbd.get() == raw {
        splx(s);
        return Ok(());
    }

    let _data = if raw != 0 {
        WSKBD_RAW
    } else {
        WSKBD_TRANSLATED
    };
    let Some(inp) = sc.sc_input.get() else {
        splx(s);
        return Err(Errno::ENXIO);
    };
    // wsevsrc_display_ioctl(inp, WSKBDIO_SETMODE, &data, FWRITE, 0); then sc_rawkbd = raw
    match *inp {}
}

/// `wsswitch_cb1`: `wsdisplay_switch1` as a completion callback.
fn wsswitch_cb1(arg: *mut c_void, error: i32, waitok: i32) {
    // SAFETY: the callers pass the display's softc as the argument.
    let sc = unsafe { &*arg.cast::<WsdisplaySoftc>() };
    let _ = wsdisplay_switch1(sc, error, waitok);
}

/// `wsswitch_cb2`: `wsdisplay_switch2` as a completion callback.
fn wsswitch_cb2(arg: *mut c_void, error: i32, waitok: i32) {
    // SAFETY: as in `wsswitch_cb1`.
    let sc = unsafe { &*arg.cast::<WsdisplaySoftc>() };
    let _ = wsdisplay_switch2(sc, error, waitok);
}

/// `wsswitch_cb3`: `wsdisplay_switch3` as a completion callback.
fn wsswitch_cb3(arg: *mut c_void, error: i32, waitok: i32) {
    // SAFETY: as in `wsswitch_cb1`.
    let sc = unsafe { &*arg.cast::<WsdisplaySoftc>() };
    let _ = wsdisplay_switch3(sc, error, waitok);
}

/// The C's `int error` of the switch steps as a result.
fn errno_of(error: i32) -> Result<(), Errno> {
    if error == 0 {
        Ok(())
    } else {
        Err(Errno::from_raw(error).unwrap_or(Errno::EIO))
    }
}

/// The C's `int error` of a result.
fn error_of(r: Result<(), Errno>) -> i32 {
    match r {
        Ok(()) => 0,
        Err(e) => e as i32,
    }
}

/// `wsdisplay_switch3`: the last step of a screen switch: the new screen's USL owner has
/// acknowledged, or there is none.
fn wsdisplay_switch3(sc: &WsdisplaySoftc, mut error: i32, waitok: i32) -> Result<(), Errno> {
    let no;
    let scr;

    if cfg!(feature = "wsdisplay_compat_usl") {
        if !sc.flag(SC_SWITCHPENDING) {
            printf(format_args!("wsdisplay_switch3: not switching\n"));
            return Err(Errno::EINVAL);
        }

        no = sc.sc_screenwanted.get();
        if !(0..WSDISPLAY_MAXSCREEN).contains(&no) {
            panic(format_args!("wsdisplay_switch3: invalid screen {no}"));
        }
        scr = sc.scr(no);
        if scr.is_none() {
            printf(format_args!("wsdisplay_switch3: screen {no} disappeared\n"));
            error = Errno::ENXIO as i32;
        }

        if error != 0 {
            // try to recover, avoid recursion

            if sc.sc_oldscreen.get() == WSDISPLAY_NULLSCREEN {
                printf(format_args!("wsdisplay_switch3: giving up\n"));
                sc.sc_focus.set(None);
                if cfg!(feature = "wsdisplay_compat_rawkbd") {
                    let _ = wsdisplay_update_rawkbd(sc, None);
                }
                sc.clr_flag(SC_SWITCHPENDING);
                return errno_of(error);
            }

            sc.sc_screenwanted.set(sc.sc_oldscreen.get());
            sc.sc_oldscreen.set(WSDISPLAY_NULLSCREEN);
            return wsdisplay_switch1(sc, 0, waitok);
        }
    } else {
        // If we do not have syncops support, we come straight from wsdisplay_switch2 which
        // has already validated our arguments and did not sleep.
        no = sc.sc_screenwanted.get();
        scr = sc.scr(no);
    }

    sc.clr_flag(SC_SWITCHPENDING);

    if let Some(scr) = scr {
        if HAVE_BURNER_SUPPORT && error == 0 {
            wsdisplay_burner_setup(sc, scr);
        }

        if error == 0 && scr.scr_flags.get() & SCR_WAITACTIVE != 0 {
            wakeup(ptr::from_ref(scr));
        }
    }
    errno_of(error)
}

/// `wsdisplay_switch2`: the display shows the new screen; give it the focus and, with a USL
/// owner, ask it to take the screen.
fn wsdisplay_switch2(sc: &WsdisplaySoftc, mut error: i32, waitok: i32) -> Result<(), Errno> {
    if !sc.flag(SC_SWITCHPENDING) {
        printf(format_args!("wsdisplay_switch2: not switching\n"));
        return Err(Errno::EINVAL);
    }

    let no = sc.sc_screenwanted.get();
    if !(0..WSDISPLAY_MAXSCREEN).contains(&no) {
        panic(format_args!("wsdisplay_switch2: invalid screen {no}"));
    }
    let scr = sc.scr(no);
    if scr.is_none() {
        printf(format_args!("wsdisplay_switch2: screen {no} disappeared\n"));
        error = Errno::ENXIO as i32;
    }

    let Some(scr) = scr.filter(|_| error == 0) else {
        // try to recover, avoid recursion

        if sc.sc_oldscreen.get() == WSDISPLAY_NULLSCREEN {
            printf(format_args!("wsdisplay_switch2: giving up\n"));
            sc.sc_focus.set(None);
            sc.clr_flag(SC_SWITCHPENDING);
            return errno_of(error);
        }

        sc.sc_screenwanted.set(sc.sc_oldscreen.get());
        sc.sc_oldscreen.set(WSDISPLAY_NULLSCREEN);
        return wsdisplay_switch1(sc, 0, waitok);
    };

    sc.sc_focusidx.set(no);
    sc.sc_focus.set(Some(NonNull::from(scr)));

    if cfg!(feature = "wsdisplay_compat_rawkbd") {
        let _ = wsdisplay_update_rawkbd(sc, Some(scr));
    }
    // keyboard map???

    if cfg!(feature = "wsdisplay_compat_usl")
        && let Some(ops) = scr.scr_syncops.get()
    {
        let cb: Option<ShowScreenCb> =
            if sc.sc_isconsole.get() != 0 && WSDISPLAY_CONS_POLLMODE.load(Ordering::Relaxed) != 0 {
                None
            } else {
                Some(wsswitch_cb3)
            };
        // SAFETY: the cookie the compatibility code registered with these syncops.
        error = unsafe { (ops.attach)(scr.scr_synccookie.get(), waitok, cb, sc.as_arg()) };
        if error == Errno::EAGAIN as i32 {
            // switch will be done asynchronously
            return Ok(());
        }
    }

    wsdisplay_switch3(sc, error, waitok)
}

/// `wsdisplay_switch1`: the old screen is released; ask the display to show the new one.
fn wsdisplay_switch1(sc: &WsdisplaySoftc, mut error: i32, waitok: i32) -> Result<(), Errno> {
    if !sc.flag(SC_SWITCHPENDING) {
        printf(format_args!("wsdisplay_switch1: not switching\n"));
        return Err(Errno::EINVAL);
    }

    let no = sc.sc_screenwanted.get();
    if no == WSDISPLAY_NULLSCREEN {
        sc.clr_flag(SC_SWITCHPENDING);
        if error == 0 {
            sc.sc_focus.set(None);
        }
        wakeup(ptr::from_ref(sc));
        return errno_of(error);
    }
    if !(0..WSDISPLAY_MAXSCREEN).contains(&no) {
        panic(format_args!("wsdisplay_switch1: invalid screen {no}"));
    }
    let scr = sc.scr(no);
    if scr.is_none() {
        printf(format_args!("wsdisplay_switch1: screen {no} disappeared\n"));
        error = Errno::ENXIO as i32;
    }

    let Some(scr) = scr.filter(|_| error == 0) else {
        sc.clr_flag(SC_SWITCHPENDING);
        return errno_of(error);
    };

    let cb: Option<ShowScreenCb> =
        if sc.sc_isconsole.get() != 0 && WSDISPLAY_CONS_POLLMODE.load(Ordering::Relaxed) != 0 {
            None
        } else {
            Some(wsswitch_cb2)
        };
    let r = sc.show_screen(scr.dconf().emulcookie.get(), waitok, cb, sc.as_arg());
    if r == Err(Errno::EAGAIN) {
        // switch will be done asynchronously
        return Ok(());
    }

    wsdisplay_switch2(sc, error_of(r), waitok)
}

/// `wsdisplay_switch`: switch display `dev` to screen `no` (`WSDISPLAY_NULLSCREEN` for
/// none), sleeping for a pending resume when `waitok`.
pub fn wsdisplay_switch(dev: &Device, no: i32, waitok: i32) -> Result<(), Errno> {
    let sc = sc_of(dev);

    if no != WSDISPLAY_NULLSCREEN {
        if !(0..WSDISPLAY_MAXSCREEN).contains(&no) {
            return Err(Errno::EINVAL);
        }
        if sc.scr(no).is_none() {
            return Err(Errno::ENXIO);
        }
    }

    let s = spltty();

    if sc.sc_resumescreen.get() != WSDISPLAY_NULLSCREEN && waitok == 0 {
        splx(s);
        return Err(Errno::EBUSY);
    }

    let mut res = Ok(());
    while sc.sc_resumescreen.get() != WSDISPLAY_NULLSCREEN && res.is_ok() {
        res = tsleep_nsec(sc.sc_resumescreen.as_ptr(), PCATCH, "wsrestore", INFSLP);
    }
    if res.is_err() {
        splx(s);
        return res;
    }

    if (sc.sc_focus.get().is_some() && no == sc.sc_focusidx.get())
        || (sc.sc_focus.get().is_none() && no == WSDISPLAY_NULLSCREEN)
    {
        splx(s);
        return Ok(());
    }

    if sc.flag(SC_SWITCHPENDING) {
        splx(s);
        return Err(Errno::EBUSY);
    }

    sc.set_flag(SC_SWITCHPENDING);
    sc.sc_screenwanted.set(no);

    splx(s);

    let Some(scr) = sc.focus() else {
        sc.sc_oldscreen.set(WSDISPLAY_NULLSCREEN);
        return wsdisplay_switch1(sc, 0, waitok);
    };
    sc.sc_oldscreen.set(sc.sc_focusidx.get());

    let mut res = 0;
    if cfg!(feature = "wsdisplay_compat_usl") {
        if let Some(ops) = scr.scr_syncops.get() {
            let cb: Option<ShowScreenCb> = if sc.sc_isconsole.get() != 0
                && WSDISPLAY_CONS_POLLMODE.load(Ordering::Relaxed) != 0
            {
                None
            } else {
                Some(wsswitch_cb1)
            };
            // SAFETY: the cookie the compatibility code registered with these syncops.
            res = unsafe { (ops.detach)(scr.scr_synccookie.get(), waitok, cb, sc.as_arg()) };
            if res == Errno::EAGAIN as i32 {
                // switch will be done asynchronously
                return Ok(());
            }
        } else if scr.scr_flags.get() & SCR_GRAPHICS != 0 {
            // no way to save state
            res = Errno::EBUSY as i32;
        }
    }

    if HAVE_WSMOUSED_SUPPORT {
        mouse_remove(scr);
    }

    wsdisplay_switch1(sc, res, waitok)
}

/// `wsdisplay_reset`: reset the focused screen's emulation, or close it as on last close.
pub fn wsdisplay_reset(dev: &Device, op: WsdisplayResetops) {
    let sc = sc_of(dev);

    let Some(scr) = sc.focus() else {
        return;
    };

    match op {
        WSDISPLAY_RESETEMUL => scr.dconf().reset(WSEMUL_RESET),
        WSDISPLAY_RESETCLOSE => wsdisplay_closescreen(sc, scr),
    }
}

/// `wsscreen_attach_sync`: interface for (external) VT switch / process synchronization
/// code: claim screen `scr` for `ops` (`WSDISPLAY_COMPAT_USL`).
pub fn wsscreen_attach_sync(
    scr: &Wsscreen,
    ops: &'static WsconsSyncops,
    cookie: *mut c_void,
) -> Result<(), Errno> {
    if let Some(cur) = scr.scr_syncops.get() {
        // The screen is already claimed. Check if the owner is still alive.
        // SAFETY: the cookie registered with these syncops.
        if unsafe { (cur.check)(scr.scr_synccookie.get()) } != 0 {
            return Err(Errno::EBUSY);
        }
    }
    scr.scr_syncops.set(Some(ops));
    scr.scr_synccookie.set(cookie);
    Ok(())
}

/// `wsscreen_detach_sync`.
pub fn wsscreen_detach_sync(scr: &Wsscreen) -> Result<(), Errno> {
    if scr.scr_syncops.get().is_none() {
        return Err(Errno::EINVAL);
    }
    scr.scr_syncops.set(None);
    Ok(())
}

/// `wsscreen_lookup_sync`: the cookie of the owner registered with `ops` (used as ID).
pub fn wsscreen_lookup_sync(
    scr: &Wsscreen,
    ops: &'static WsconsSyncops,
) -> Result<*mut c_void, Errno> {
    match scr.scr_syncops.get() {
        Some(cur) if ptr::eq(cur, ops) => Ok(scr.scr_synccookie.get()),
        _ => Err(Errno::EINVAL),
    }
}

/// `wsdisplay_maxscreenidx`: interface to virtual screen stuff.
pub fn wsdisplay_maxscreenidx(_sc: &WsdisplaySoftc) -> i32 {
    WSDISPLAY_MAXSCREEN - 1
}

/// `wsdisplay_screenstate`: `EBUSY` for an open screen, `Ok` for a closed one.
pub fn wsdisplay_screenstate(sc: &WsdisplaySoftc, idx: i32) -> Result<(), Errno> {
    if !(0..WSDISPLAY_MAXSCREEN).contains(&idx) {
        return Err(Errno::EINVAL);
    }
    let scr = sc.scr(idx).ok_or(Errno::ENXIO)?;
    if scr.scr_flags.get() & SCR_OPEN != 0 {
        Err(Errno::EBUSY)
    } else {
        Ok(())
    }
}

/// `wsdisplay_getactivescreen`.
pub fn wsdisplay_getactivescreen(sc: &WsdisplaySoftc) -> i32 {
    if sc.sc_focus.get().is_some() {
        sc.sc_focusidx.get()
    } else {
        WSDISPLAY_NULLSCREEN
    }
}

/// `wsscreen_switchwait`: sleep until screen `no` is the active one (until there is none
/// for `WSDISPLAY_NULLSCREEN`).
pub fn wsscreen_switchwait(sc: &WsdisplaySoftc, no: i32) -> Result<(), Errno> {
    if no == WSDISPLAY_NULLSCREEN {
        let s = spltty();
        let mut res = Ok(());
        while sc.sc_focus.get().is_some() && res.is_ok() {
            res = tsleep_nsec(ptr::from_ref(sc), PCATCH, "wswait", INFSLP);
        }
        splx(s);
        return res;
    }

    if !(0..WSDISPLAY_MAXSCREEN).contains(&no) {
        return Err(Errno::ENXIO);
    }
    let scrp = sc.sc_scr[no as usize].get().ok_or(Errno::ENXIO)?;

    let s = spltty();
    let mut res = Ok(());
    if sc.sc_focus.get() != Some(scrp) {
        // SAFETY: in `sc_scr[]`, live until `wsdisplay_delscreen`, which wakes us first.
        let scr = unsafe { scrp.as_ref() };
        scr.scr_flags.set(scr.scr_flags.get() | SCR_WAITACTIVE);
        res = tsleep_nsec(scrp.as_ptr(), PCATCH, "wswait2", INFSLP);
        if sc.sc_scr[no as usize].get() != Some(scrp) {
            res = Err(Errno::ENXIO); // disappeared in the meantime
        } else {
            scr.scr_flags.set(scr.scr_flags.get() & !SCR_WAITACTIVE);
        }
    }
    splx(s);
    res
}

/// `wsdisplay_kbdholdscr`: hold or release a screen's output.
fn wsdisplay_kbdholdscr(scr: &Wsscreen, hold: i32) {
    if hold != 0 {
        scr.scr_hold_screen.set(1);
    } else {
        scr.scr_hold_screen.set(0);
        if let Some(tp) = scr.tty() {
            timeout_add(&tp.t_rstrt_to, 0); // "immediate"
        }
    }
}

/// `wsdisplay_kbdholdscreen`: the keyboard's Hold Screen key, on the focused screen.
pub fn wsdisplay_kbdholdscreen(dev: &Device, hold: i32) {
    let sc = sc_of(dev);

    if let Some(scr) = sc.focus()
        && scr.tty().is_some()
    {
        wsdisplay_kbdholdscr(scr, hold);
    }
}

/// `wsdisplay_set_console_kbd`: the console keyboard's event source joins the console
/// display's mux (`NWSKBD > 0`).
pub fn wsdisplay_set_console_kbd(src: Option<&Wsevsrc>) {
    // The event source is uninhabited until wsmux.c: with one, the C sets
    // `src->me_dispdv` to the console display (after wsmux_attach_sc), or to NULL without
    // a console display.
    let Some(src) = src else {
        return;
    };
    match *src {}
}

/// `wsdisplay_set_kbd`: attach the keyboard event source `kbd` to display `disp` (C compiles
/// it only without wsmux).
pub fn wsdisplay_set_kbd(disp: &Device, kbd: Option<&'static Wsevsrc>) -> Result<(), Errno> {
    let sc = sc_of(disp);

    if sc.sc_input.get().is_some() {
        return Err(Errno::EBUSY);
    }

    sc.sc_input.set(kbd);

    Ok(())
}

/// `wsdisplay_cnputc`: console interface: a character of the kernel's output, on the
/// console screen unless it is in graphics mode.
pub fn wsdisplay_cnputc(_dev: Dev, i: i32) {
    let c = [i as u8];

    if !WSDISPLAY_CONSOLE_INITTED.load(Ordering::Acquire) {
        return;
    }

    if let Some(cd) = console_device()
        && let Some(scr0) = cd.scr(0)
        && scr0.scr_flags.get() & SCR_GRAPHICS != 0
    {
        return;
    }

    // HAVE_BURNER_SUPPORT: wsdisplay_burn(wsdisplay_console_device, WSDISPLAY_BURN_OUTPUT)
    // is commented out in C.
    let _ = WSDISPLAY_CONSOLE_CONF.output(&c, true);
}

/// `wsdisplay_cons.cn_getc`: the console keyboard's polled getc
/// ([`wsdisplay_set_cons_kbd`]).
pub fn wsdisplay_cngetc(dev: Dev) -> i32 {
    // SAFETY: written by the console keyboard's attach (cold) or under the kernel lock.
    let getc = unsafe { WSDISPLAY_CONS_GETC.read() };
    getc(dev)
}

/// `wsdisplay_cons.cn_bell`: the console keyboard's bell, if it set one.
pub fn wsdisplay_cnbell(dev: Dev, pitch: u32, period: u32, volume: u32) {
    // SAFETY: as in `wsdisplay_cngetc`.
    if let Some(bell) = unsafe { WSDISPLAY_CONS_BELL.read() } {
        bell(dev, pitch, period, volume);
    }
}

/// `wsdisplay_getc_dummy`: no console keyboard.
pub fn wsdisplay_getc_dummy(_dev: Dev) -> i32 {
    // panic?
    0
}

/// `wsdisplay_pollc`: polling on or off, for the frame buffer and keyboard drivers.
pub fn wsdisplay_pollc(dev: Dev, on: i32) {
    WSDISPLAY_CONS_POLLMODE.store(on, Ordering::Relaxed);

    // notify to fb drivers
    if let Some(cd) = console_device()
        && let Some(pollc) = cd.ops().pollc
    {
        // SAFETY: the access cookie paired with these accessops.
        unsafe { pollc(cd.sc_accesscookie.get(), on) };
    }

    // notify to kbd drivers
    // SAFETY: as in `wsdisplay_cngetc`.
    if let Some(kbd_pollc) = unsafe { WSDISPLAY_CONS_KBD_POLLC.read() } {
        kbd_pollc(dev, on);
    }
}

/// `wsdisplay_cons.cn_pollc`: [`wsdisplay_pollc`] with the console's `bool`.
fn wsdisplay_cnpollc(dev: Dev, on: bool) {
    wsdisplay_pollc(dev, i32::from(on));
}

/// `wsdisplay_set_cons_kbd`: the console keyboard's polled `getc`, `pollc` and `bell`.
pub fn wsdisplay_set_cons_kbd(
    get: fn(Dev) -> i32,
    poll: fn(Dev, i32),
    bell: Option<fn(Dev, u32, u32, u32)>,
) {
    // SAFETY: the console keyboard's attach, cold, or under the kernel lock; the console
    // entry points read these cells only through `read`.
    unsafe {
        WSDISPLAY_CONS_GETC.write(get);
        WSDISPLAY_CONS_BELL.write(bell);
        WSDISPLAY_CONS_KBD_POLLC.write(Some(poll));
    }
}

/// `wsdisplay_unset_cons_kbd`.
pub fn wsdisplay_unset_cons_kbd() {
    // SAFETY: as in `wsdisplay_set_cons_kbd`.
    unsafe {
        WSDISPLAY_CONS_GETC.write(wsdisplay_getc_dummy);
        WSDISPLAY_CONS_BELL.write(None);
        WSDISPLAY_CONS_KBD_POLLC.write(None);
    }
}

/// Whether wsdisplay is the console device (`cn_tab == &wsdisplay_cons`).
fn is_console_tab() -> bool {
    cn_tab().is_some_and(|cn| ptr::eq(cn, &WSDISPLAY_CONS))
}

/// `wsdisplay_switchtoconsole`: switch the console display to its first screen.
pub fn wsdisplay_switchtoconsole() {
    if let Some(sc) = console_device()
        && is_console_tab()
    {
        let Some(scr) = sc.scr(0) else {
            return;
        };
        let _ = sc.show_screen(scr.dconf().emulcookie.get(), 0, None, ptr::null_mut());
    }
}

/// `wsdisplay_enter_ddb`: switch the console display to its ddb screen, avoiding locking
/// where we can.
pub fn wsdisplay_enter_ddb() {
    if let Some(sc) = console_device()
        && is_console_tab()
    {
        let Some(scr) = sc.scr(0) else {
            return;
        };
        if let Some(enter_ddb) = sc.ops().enter_ddb {
            // SAFETY: the access cookie paired with these accessops, and the console
            // screen's cookie.
            unsafe { enter_ddb(sc.sc_accesscookie.get(), scr.dconf().emulcookie.get()) };
        } else {
            let _ = sc.show_screen(scr.dconf().emulcookie.get(), 0, None, ptr::null_mut());
        }
    }
}

/// `wsdisplay_suspend`: deal with the xserver doing driver in userland and thus screwing up
/// suspend and resume by switching away from it at suspend/resume time.
///
/// These functions must be called from the MD suspend callback, since we may need to sleep
/// if we have a user (probably an X server) on a vt. therefore this can't be a
/// `config_suspend()` hook.
pub fn wsdisplay_suspend() {
    for i in 0..WSDISPLAY_CD.cd_ndevs.get() {
        if let Some(sc) = wsdisplay_sc(i) {
            wsdisplay_suspend_device(&sc.sc_dv);
        }
    }
}

/// `wsdisplay_suspend_device`: out of graphics mode for the suspend, onto a text screen,
/// with other switches blocked until resume.
fn wsdisplay_suspend_device(dev: &Device) {
    let sc = sc_of(dev);

    let active = wsdisplay_getactivescreen(sc);
    if active == WSDISPLAY_NULLSCREEN {
        return;
    }

    let Some(scr) = sc.scr(active) else {
        return;
    };
    // We want to switch out of graphics mode for the suspend
    let idx = loop {
        let mut idx = WSDISPLAY_MAXSCREEN;
        if scr.scr_flags.get() & SCR_GRAPHICS != 0 {
            idx = (0..WSDISPLAY_MAXSCREEN)
                .find(|&i| {
                    sc.scr(i).is_some_and(|other| {
                        !ptr::eq(other, scr) && other.scr_flags.get() & SCR_GRAPHICS == 0
                    })
                })
                .unwrap_or(WSDISPLAY_MAXSCREEN);
        }

        // if we don't have anything to switch to, we can't do anything
        if idx == WSDISPLAY_MAXSCREEN {
            return;
        }

        // we do a lot of magic here because we need to know that the switch has completed
        // before we return
        match wsdisplay_switch(&sc.sc_dv, idx, 1) {
            Err(Errno::EBUSY) => continue, // XXX sleep on what's going on
            Err(_) => return,
            Ok(()) => break idx,
        }
    };

    let s = spltty();
    sc.sc_resumescreen.set(active); // block other vt switches until resume
    splx(s);
    // This will either return ENXIO (invalid (shouldn't happen) or wsdisplay disappeared
    // (problem solved)), or EINTR/ERESTART. Not much we can do about the latter since we
    // can't return to userland.
    let _ = wsscreen_switchwait(sc, idx);
}

/// `wsdisplay_resume`.
pub fn wsdisplay_resume() {
    for i in 0..WSDISPLAY_CD.cd_ndevs.get() {
        if let Some(sc) = wsdisplay_sc(i) {
            wsdisplay_resume_device(&sc.sc_dv);
        }
    }
}

/// `wsdisplay_resume_device`: back to the screen the suspend left.
fn wsdisplay_resume_device(dev: &Device) {
    let sc = sc_of(dev);

    if sc.sc_resumescreen.get() != WSDISPLAY_NULLSCREEN {
        let s = spltty();
        let idx = sc.sc_resumescreen.get();
        sc.sc_resumescreen.set(WSDISPLAY_NULLSCREEN);
        wakeup(sc.sc_resumescreen.as_ptr());
        splx(s);
        let _ = wsdisplay_switch(&sc.sc_dv, idx, 1);
    }
}

/// `wsscrollback`: scroll the focused screen back or forward a page, or back to the bottom
/// (`HAVE_SCROLLBACK_SUPPORT`; for wskbd).
pub fn wsscrollback(arg: *mut c_void, op: i32) {
    // SAFETY: wskbd passes the display's softc.
    let sc = unsafe { &*arg.cast::<WsdisplaySoftc>() };

    let Some(focus) = sc.focus() else {
        return;
    };

    let lines = if op == WSDISPLAY_SCROLL_RESET {
        0
    } else {
        let lines = focus.dconf().scrdata().nrows - 1;
        if op == WSDISPLAY_SCROLL_BACKWARD {
            -lines
        } else {
            lines
        }
    };

    if let Some(scrollback) = sc.ops().scrollback {
        // SAFETY: the access cookie paired with these accessops, and the focused screen's
        // cookie.
        unsafe {
            scrollback(
                sc.sc_accesscookie.get(),
                focus.dconf().emulcookie.get(),
                lines,
            )
        };
    }
}

/// `wsdisplay_burner_setup`: update screen burner behaviour after either a screen focus
/// change or a screen mode change. This is needed to allow X11 to manage screen blanking
/// without any interference from the kernel.
fn wsdisplay_burner_setup(sc: &WsdisplaySoftc, scr: &Wsscreen) {
    if scr.scr_flags.get() & SCR_GRAPHICS != 0 {
        // enable video _immediately_ if it needs to be...
        if sc.sc_burnman.get() != 0 {
            wsdisplay_burner(sc.as_arg());
        }
        // ...and disable the burner while X is running
        if sc.sc_burnout.get() != 0 {
            timeout_del(&sc.sc_burner);
            sc.sc_burnout.set(0);
        }
    } else {
        // reenable the burner after exiting from X
        if sc.sc_burnman.get() == 0 {
            sc.sc_burnout.set(sc.sc_burnoutintvl.get());
            wsdisplay_burn(sc.as_arg(), sc.sc_burnflags.get());
        }
    }
}

/// `wsdisplay_burn`: activity of the kinds in `flags`: restart the blanking delay (for
/// wskbd and wsmouse too).
pub fn wsdisplay_burn(v: *mut c_void, flags: u32) {
    // SAFETY: the callers pass a display's softc.
    let sc = unsafe { &*v.cast::<WsdisplaySoftc>() };

    if flags
        & sc.sc_burnflags.get()
        & (WSDISPLAY_BURN_OUTPUT | WSDISPLAY_BURN_KBD | WSDISPLAY_BURN_MOUSE)
        != 0
        && sc.ops().burn_screen.is_some()
    {
        if sc.sc_burnout.get() != 0 {
            timeout_add_msec(&sc.sc_burner, sc.sc_burnout.get() as u64);
        }
        if sc.sc_burnman.get() != 0 {
            sc.sc_burnout.set(0);
        }
    }
}

/// `wsdisplay_burner`: the `sc_burner` timeout: blank or unblank the screen.
fn wsdisplay_burner(v: *mut c_void) {
    // SAFETY: `wsdisplay_attach` set the timeout up with the softc.
    let sc = unsafe { &*v.cast::<WsdisplaySoftc>() };

    if let Some(burn_screen) = sc.ops().burn_screen {
        // SAFETY: the access cookie paired with these accessops.
        unsafe {
            burn_screen(
                sc.sc_accesscookie.get(),
                sc.sc_burnman.get() as u32,
                sc.sc_burnflags.get(),
            )
        };
        let s = spltty();
        if sc.sc_burnman.get() != 0 {
            sc.sc_burnout.set(sc.sc_burnoutintvl.get());
            timeout_add_msec(&sc.sc_burner, sc.sc_burnout.get() as u64);
        } else {
            sc.sc_burnout.set(sc.sc_burninintvl.get());
        }
        sc.sc_burnman.set(i32::from(sc.sc_burnman.get() == 0));
        splx(s);
    }
}

/// The `int` of a `ws_get_param`/`ws_set_param` hook as a result.
fn param_hook(r: i32) -> Result<(), Errno> {
    match r {
        0 => Ok(()),
        -1 => Err(Errno::ENOTTY),
        e => Err(Errno::from_raw(e).unwrap_or(Errno::EINVAL)),
    }
}

/// `wsdisplay_get_param`: the parameter from display `sc`, or from the first display (then
/// the firmware hook) that has it.
fn wsdisplay_get_param(sc: Option<&WsdisplaySoftc>, dp: &mut WsdisplayParam) -> Result<(), Errno> {
    if let Some(sc) = sc {
        return wsdisplay_param(&sc.sc_dv, WSDISPLAYIO_GETPARAM, dp);
    }

    let mut error = Err(Errno::ENXIO);
    for i in 0..WSDISPLAY_CD.cd_ndevs.get() {
        let Some(sc) = wsdisplay_sc(i) else {
            continue;
        };
        error = wsdisplay_param(&sc.sc_dv, WSDISPLAYIO_GETPARAM, dp);
        if error.is_ok() {
            break;
        }
    }

    if error.is_err()
        && let Some(f) = ws_get_param()
    {
        error = param_hook(f(dp));
    }

    error
}

/// `wsdisplay_set_param`: as [`wsdisplay_get_param`], to set it.
fn wsdisplay_set_param(sc: Option<&WsdisplaySoftc>, dp: &mut WsdisplayParam) -> Result<(), Errno> {
    if let Some(sc) = sc {
        return wsdisplay_param(&sc.sc_dv, WSDISPLAYIO_SETPARAM, dp);
    }

    let mut error = Err(Errno::ENXIO);
    for i in 0..WSDISPLAY_CD.cd_ndevs.get() {
        let Some(sc) = wsdisplay_sc(i) else {
            continue;
        };
        error = wsdisplay_param(&sc.sc_dv, WSDISPLAYIO_SETPARAM, dp);
        if error.is_ok() {
            break;
        }
    }

    if error.is_err()
        && let Some(f) = ws_set_param()
    {
        error = param_hook(f(dp));
    }

    error
}

/// The brightness step of `wsdisplay_brightness_step` from `dp`'s range and value: about
/// 5% up (`dir > 0`) or down (`dir < 0`), clamped to the range.
fn brightness_step(dp: &WsdisplayParam, dir: i32) -> i32 {
    // Use a step size of approximately 5%.
    let delta = 1.max((dp.max - dp.min) * 5 / 100);
    let mut new = dp.curval;

    if dir > 0 {
        if delta > dp.max - dp.curval {
            new = dp.max;
        } else {
            new += delta;
        }
    } else if dir < 0 {
        if delta > dp.curval - dp.min {
            new = dp.min;
        } else {
            new -= delta;
        }
    }
    new
}

/// `wsdisplay_brightness_step`: the brightness keys.
pub fn wsdisplay_brightness_step(dev: Option<&Device>, dir: i32) {
    let sc = dev.map(sc_of);
    let mut dp = WsdisplayParam {
        param: WSDISPLAYIO_PARAM_BRIGHTNESS,
        ..WsdisplayParam::default()
    };

    if wsdisplay_get_param(sc, &mut dp).is_err() {
        return;
    }

    let new = brightness_step(&dp, dir);

    if dp.curval == new {
        return;
    }

    dp.curval = new;
    let _ = wsdisplay_set_param(sc, &mut dp);
}

/// `wsdisplay_brightness_zero`.
pub fn wsdisplay_brightness_zero(dev: Option<&Device>) {
    let sc = dev.map(sc_of);
    let mut dp = WsdisplayParam {
        param: WSDISPLAYIO_PARAM_BRIGHTNESS,
        ..WsdisplayParam::default()
    };

    if wsdisplay_get_param(sc, &mut dp).is_err() {
        return;
    }

    dp.curval = dp.min;
    let _ = wsdisplay_set_param(sc, &mut dp);
}

/// `wsdisplay_brightness_cycle`.
pub fn wsdisplay_brightness_cycle(dev: Option<&Device>) {
    let sc = dev.map(sc_of);
    let mut dp = WsdisplayParam {
        param: WSDISPLAYIO_PARAM_BRIGHTNESS,
        ..WsdisplayParam::default()
    };

    if wsdisplay_get_param(sc, &mut dp).is_err() {
        return;
    }

    if dp.curval == dp.max {
        wsdisplay_brightness_zero(dev);
    } else {
        wsdisplay_brightness_step(dev, 1);
    }
}

/// `wsmoused`: `wsmoused(8)` support functions. Main function, called from
/// `wsdisplay_cfg_ioctl` (`HAVE_WSMOUSED_SUPPORT`).
fn wsmoused(
    sc: &WsdisplaySoftc,
    data: &mut [u8],
    _flag: i32,
    p: Option<&Proc>,
) -> Result<(), Errno> {
    let mouse_event: WsconsEvent = ioctl_arg(data);

    if is_motion_event(mouse_event.type_) {
        if let Some(focus) = sc.focus() {
            motion_event(focus, mouse_event.type_, mouse_event.value);
        }
        return Ok(());
    }
    if is_button_event(mouse_event.type_) {
        if let Some(focus) = sc.focus() {
            // XXX tv_sec contains the number of clicks
            if mouse_event.type_ == WSCONS_EVENT_MOUSE_DOWN {
                button_event(focus, mouse_event.value, mouse_event.time.tv_sec as i32);
            } else {
                button_event(focus, mouse_event.value, 0);
            }
        }
        return Ok(());
    }
    if is_ctrl_event(mouse_event.type_) {
        return ctrl_event(sc, mouse_event.type_, mouse_event.value, p);
    }
    // The C's -1, which the ioctl's caller sees as ERESTART.
    Err(Errno::ERESTART)
}

/// `motion_event`: mouse motion events.
fn motion_event(scr: &Wsscreen, type_: u32, value: i32) {
    match type_ {
        WSCONS_EVENT_MOUSE_DELTA_X => mouse_moverel(scr, value, 0),
        WSCONS_EVENT_MOUSE_DELTA_Y => mouse_moverel(scr, 0, value.wrapping_neg()),
        WSCONS_EVENT_MOUSE_DELTA_Z if HAVE_SCROLLBACK_SUPPORT => mouse_zaxis(scr, value),
        _ => {}
    }
}

/// `button_event`: button clicks events.
fn button_event(scr: &Wsscreen, button: i32, clicks: i32) {
    match button {
        MOUSE_COPY_BUTTON => match clicks % 4 {
            0 => {
                // button is up
                mouse_copy_end(scr);
                mouse_copy_selection(scr);
            }
            1 => {
                // single click
                mouse_copy_start(scr);
                mouse_copy_selection(scr);
            }
            2 => {
                // double click
                mouse_copy_word(scr);
                mouse_copy_selection(scr);
            }
            3 => {
                // triple click
                mouse_copy_line(scr);
                mouse_copy_selection(scr);
            }
            _ => {}
        },
        MOUSE_PASTE_BUTTON if clicks != 0 => mouse_paste(scr),
        MOUSE_EXTEND_BUTTON if clicks != 0 => mouse_copy_extend_after(scr),
        _ => {}
    }
}

/// `ctrl_event`: control events (`wsmoused(8)` starting and stopping).
fn ctrl_event(
    sc: &WsdisplaySoftc,
    type_: u32,
    _value: i32,
    _p: Option<&Proc>,
) -> Result<(), Errno> {
    match type_ {
        WSCONS_EVENT_WSMOUSED_OFF => {
            sc.clr_flag(SC_PASTE_AVAIL);
            Ok(())
        }
        WSCONS_EVENT_WSMOUSED_ON => {
            if sc.ops().getchar.is_none() {
                // no wsmoused(8) support in the display driver; the C's 1 is EPERM
                return Err(Errno::EPERM);
            }
            allocate_copybuffer(sc);
            sc.clr_flag(SC_PASTE_AVAIL);

            for i in 0..WSDISPLAY_DEFAULTSCREENS {
                if let Some(scr) = sc.scr(i) {
                    scr.mouse.set((scr.ws_ncols() * scr.ws_nrows()) / 2);
                    scr.cursor.set(scr.mouse.get());
                    scr.cpy_start.set(0);
                    scr.cpy_end.set(0);
                    scr.orig_start.set(0);
                    scr.orig_end.set(0);
                    scr.mouse_flags.set(0);
                }
            }
            Ok(())
        }
        _ => Ok(()), // can't happen, really
    }
}

/// `mouse_moverel`: move the mouse cursor by (`dx`, `dy`) cells, extending the selection
/// when one is in progress.
fn mouse_moverel(scr: &Wsscreen, dx: i32, dy: i32) {
    let dconf = scr.dconf();
    let old_mouse = scr.mouse.get();
    let n = dconf.n_cols();
    let mut mouse_col = (scr.mouse.get() % n) as i32;
    let mut mouse_row = (scr.mouse.get() / n) as i32;

    // update position
    if mouse_col + dx >= dconf.maxcol() as i32 {
        mouse_col = dconf.maxcol() as i32;
    } else if mouse_col + dx <= 0 {
        mouse_col = 0;
    } else {
        mouse_col += dx;
    }
    if mouse_row + dy >= dconf.maxrow() as i32 {
        mouse_row = dconf.maxrow() as i32;
    } else if mouse_row + dy <= 0 {
        mouse_row = 0;
    } else {
        mouse_row += dy;
    }
    scr.mouse.set((mouse_row as u32) * n + mouse_col as u32);

    // if we have moved
    if old_mouse != scr.mouse.get() {
        // XXX unblank screen if display.ms_act
        if scr.mflag(SEL_IN_PROGRESS) {
            // selection in progress
            mouse_copy_extend(scr);
        } else {
            inverse_char(scr, scr.mouse.get());
            if scr.mflag(MOUSE_VISIBLE) {
                inverse_char(scr, old_mouse);
            } else {
                scr.mset(MOUSE_VISIBLE);
            }
        }
    }
}

/// `inverse_char`: show the cell at `pos` colour-inverted (or in reverse video).
fn inverse_char(scr: &Wsscreen, pos: u32) {
    let dconf = scr.dconf();
    let mut cell = scr.getchar(pos).unwrap_or_default();
    let ops = dconf.emulops();

    let (mut fg, mut bg, ul) = match ops.unpack_attr {
        // SAFETY: the screen cookie the driver paired with these emulops.
        Some(unpack_attr) => unsafe { unpack_attr(dconf.emulcookie.get(), cell.attr) },
        None => (0, 0, 0),
    };

    // Display the mouse cursor as a color inverted cell whenever possible. If this is not
    // possible, ask for the video reverse attribute.
    let mut flags = 0;
    if dconf.scrdata().capabilities & WSSCREEN_WSCOLORS != 0 {
        flags |= WSATTR_WSCOLORS;
        core::mem::swap(&mut fg, &mut bg);
    } else if dconf.scrdata().capabilities & WSSCREEN_REVERSE != 0 {
        flags |= WSATTR_REVERSE;
    }
    let ulflag = if ul != 0 { WSATTR_UNDERLINE } else { 0 };
    let Some(pack_attr) = ops.pack_attr else {
        return;
    };
    // SAFETY: as above.
    if let Ok(attr) = unsafe { pack_attr(dconf.emulcookie.get(), fg, bg, flags | ulflag) } {
        cell.attr = attr;
        let _ = dconf.putchar(pos, cell.uc, cell.attr);
    }
}

/// `inverse_region`: [`inverse_char`] from `start` to `end`.
fn inverse_region(scr: &Wsscreen, start: u32, end: u32) {
    let dconf = scr.dconf();

    // sanity check, useful because 'end' can be (u_int)-1
    let abs_end = dconf.n_cols().wrapping_mul(dconf.n_rows());
    if end > abs_end {
        return;
    }
    let mut current_pos = start;
    while current_pos <= end {
        inverse_char(scr, current_pos);
        current_pos = current_pos.wrapping_add(1);
    }
}

/// `skip_spc_right`: the number of contiguous blank characters between the right margin if
/// `border == BORDER` or between the next non-blank character and the current mouse cursor
/// if `border == NO_BORDER`.
fn skip_spc_right(scr: &Wsscreen, border: i32) -> u32 {
    let dconf = scr.dconf();
    let mut current = scr.cpy_end.get();
    let mouse_col = scr.cpy_end.get() % dconf.n_cols();
    let limit = current.wrapping_add(dconf.n_cols() - mouse_col - 1);
    let mut res: u32 = 0;

    while scr
        .getchar(current)
        .is_some_and(|c| c.uc == u32::from(b' '))
        && current <= limit
    {
        current = current.wrapping_add(1);
        res = res.wrapping_add(1);
    }
    if border == BORDER {
        if current > limit {
            res.wrapping_sub(1)
        } else {
            0
        }
    } else if res != 0 {
        res - 1
    } else {
        res
    }
}

/// `skip_spc_left`: the number of contiguous blank characters between the first of the
/// contiguous blank characters and the current mouse cursor.
fn skip_spc_left(scr: &Wsscreen) -> u32 {
    let dconf = scr.dconf();
    let mut current = scr.cpy_start.get();
    let mouse_col = scr.mouse.get() % dconf.n_cols();
    let limit = current.wrapping_sub(mouse_col);
    let mut res: u32 = 0;

    while scr
        .getchar(current)
        .is_some_and(|c| c.uc == u32::from(b' '))
        && current >= limit
    {
        current = current.wrapping_sub(1);
        res = res.wrapping_add(1);
    }
    res.saturating_sub(1)
}

/// `charClass`: class of characters. Stolen from xterm sources of the Xfree project (see
/// cvs tag below); `$TOG: button.c /main/76 1997/07/30 16:56:19 kaleb $`.
#[rustfmt::skip]
static CHAR_CLASS: [u8; 256] = [
// NUL  SOH  STX  ETX  EOT  ENQ  ACK  BEL
    32,   1,   1,   1,   1,   1,   1,   1,
//  BS   HT   NL   VT   NP   CR   SO   SI
     1,  32,   1,   1,   1,   1,   1,   1,
// DLE  DC1  DC2  DC3  DC4  NAK  SYN  ETB
     1,   1,   1,   1,   1,   1,   1,   1,
// CAN   EM  SUB  ESC   FS   GS   RS   US
     1,   1,   1,   1,   1,   1,   1,   1,
//  SP    !    "    #    $    %    &    '
    32,  33,  34,  35,  36,  37,  38,  39,
//   (    )    *    +    ,    -    .    /
    40,  41,  42,  43,  44,  45,  46,  47,
//   0    1    2    3    4    5    6    7
    48,  48,  48,  48,  48,  48,  48,  48,
//   8    9    :    ;    <    =    >    ?
    48,  48,  58,  59,  60,  61,  62,  63,
//   @    A    B    C    D    E    F    G
    64,  48,  48,  48,  48,  48,  48,  48,
//   H    I    J    K    L    M    N    O
    48,  48,  48,  48,  48,  48,  48,  48,
//   P    Q    R    S    T    U    V    W
    48,  48,  48,  48,  48,  48,  48,  48,
//   X    Y    Z    [    \    ]    ^    _
    48,  48,  48,  91,  92,  93,  94,  48,
//   `    a    b    c    d    e    f    g
    96,  48,  48,  48,  48,  48,  48,  48,
//   h    i    j    k    l    m    n    o
    48,  48,  48,  48,  48,  48,  48,  48,
//   p    q    r    s    t    u    v    w
    48,  48,  48,  48,  48,  48,  48,  48,
//   x    y    z    {    |    }    ~  DEL
    48,  48,  48, 123, 124, 125, 126,   1,
// x80  x81  x82  x83  IND  NEL  SSA  ESA
     1,   1,   1,   1,   1,   1,   1,   1,
// HTS  HTJ  VTS  PLD  PLU   RI  SS2  SS3
     1,   1,   1,   1,   1,   1,   1,   1,
// DCS  PU1  PU2  STS  CCH   MW  SPA  EPA
     1,   1,   1,   1,   1,   1,   1,   1,
// x98  x99  x9A  CSI   ST  OSC   PM  APC
     1,   1,   1,   1,   1,   1,   1,   1,
//   -    i   c/    L   ox   Y-    |   So
   160, 161, 162, 163, 164, 165, 166, 167,
//  ..   c0   ip   <<    _        R0    -
   168, 169, 170, 171, 172, 173, 174, 175,
//   o   +-    2    3    '    u   q|    .
   176, 177, 178, 179, 180, 181, 182, 183,
//   ,    1    2   >>  1/4  1/2  3/4    ?
   184, 185, 186, 187, 188, 189, 190, 191,
//  A`   A'   A^   A~   A:   Ao   AE   C,
    48,  48,  48,  48,  48,  48,  48,  48,
//  E`   E'   E^   E:   I`   I'   I^   I:
    48,  48,  48,  48,  48,  48,  48,  48,
//  D-   N~   O`   O'   O^   O~   O:    X
    48,  48,  48,  48,  48,  48,  48, 216,
//  O/   U`   U'   U^   U:   Y'    P    B
    48,  48,  48,  48,  48,  48,  48,  48,
//  a`   a'   a^   a~   a:   ao   ae   c,
    48,  48,  48,  48,  48,  48,  48,  48,
//  e`   e'   e^   e:    i`  i'   i^   i:
    48,  48,  48,  48,  48,  48,  48,  48,
//   d   n~   o`   o'   o^   o~   o:   -:
    48,  48,  48,  48,  48,  48,  48,  248,
//  o/   u`   u'   u^   u:   y'    P   y:
    48,  48,  48,  48,  48,  48,  48,  48,
];

/// `charClass[c & 0xff]`.
fn char_class(uc: u32) -> u8 {
    CHAR_CLASS[(uc & 0xff) as usize]
}

/// `skip_char_right`: find the first blank beginning after the current cursor position.
fn skip_char_right(scr: &Wsscreen, offset: u32) -> u32 {
    let dconf = scr.dconf();
    let mut current = offset;
    let limit = current.wrapping_add(dconf.n_cols() - (scr.mouse.get() % dconf.n_cols()) - 1);
    let mut res: u32 = 0;

    let class = char_class(scr.getchar(current).unwrap_or_default().uc);
    while scr
        .getchar(current)
        .is_some_and(|c| char_class(c.uc) == class)
        && current <= limit
    {
        current = current.wrapping_add(1);
        res = res.wrapping_add(1);
    }
    res.saturating_sub(1)
}

/// `skip_char_left`: find the first non-blank character before the cursor position.
fn skip_char_left(scr: &Wsscreen, offset: u32) -> u32 {
    let dconf = scr.dconf();
    let mut current = offset;
    let limit = current.wrapping_sub(scr.mouse.get() % dconf.n_cols());
    let mut res: u32 = 0;

    let class = char_class(scr.getchar(current).unwrap_or_default().uc);
    while scr
        .getchar(current)
        .is_some_and(|c| char_class(c.uc) == class)
        && current >= limit
    {
        current = current.wrapping_sub(1);
        res = res.wrapping_add(1);
    }
    res.saturating_sub(1)
}

/// `class_cmp`: compare character classes: 1 when they differ (or a cell cannot be read).
fn class_cmp(scr: &Wsscreen, first: u32, second: u32) -> u32 {
    let Some(a) = scr.getchar(first) else {
        return 1;
    };
    let Some(b) = scr.getchar(second) else {
        return 1;
    };

    u32::from(char_class(a.uc) != char_class(b.uc))
}

/// `mouse_copy_start`: beginning of a copy operation.
fn mouse_copy_start(scr: &Wsscreen) {
    // if no selection, then that's the first one
    scr.sc().set_flag(SC_PASTE_AVAIL);

    // remove the previous selection
    if scr.mflag(SEL_EXISTS) {
        remove_selection(scr);
    }

    // initial show of the cursor
    if !scr.mflag(MOUSE_VISIBLE) {
        inverse_char(scr, scr.mouse.get());
    }

    scr.cpy_start.set(scr.mouse.get());
    scr.cpy_end.set(scr.mouse.get());
    scr.orig_start.set(scr.cpy_start.get());
    scr.orig_end.set(scr.cpy_end.get());
    scr.cursor.set(scr.cpy_end.get().wrapping_add(1)); // init value

    // useful later, in mouse_copy_extend
    let right = skip_spc_right(scr, BORDER);
    if right != 0 {
        scr.mset(BLANK_TO_EOL);
    }

    scr.mset(SEL_IN_PROGRESS | SEL_EXISTS | SEL_BY_CHAR);
    scr.mclr(SEL_BY_WORD | SEL_BY_LINE);
    scr.mclr(MOUSE_VISIBLE); // cursor hidden in selection
}

/// `mouse_copy_word`: copy of the word under the cursor.
fn mouse_copy_word(scr: &Wsscreen) {
    if scr.mflag(SEL_EXISTS) {
        remove_selection(scr);
    }

    if scr.mflag(MOUSE_VISIBLE) {
        inverse_char(scr, scr.mouse.get());
    }

    scr.cpy_start.set(scr.mouse.get());
    scr.cpy_end.set(scr.mouse.get());

    let (right, left) = if scr
        .getchar(scr.mouse.get())
        .is_some_and(|c| is_alphanum(c.uc))
    {
        (
            skip_char_right(scr, scr.cpy_end.get()),
            skip_char_left(scr, scr.cpy_start.get()),
        )
    } else {
        (skip_spc_right(scr, NO_BORDER), skip_spc_left(scr))
    };

    scr.cpy_start.set(scr.cpy_start.get().wrapping_sub(left));
    scr.cpy_end.set(scr.cpy_end.get().wrapping_add(right));
    scr.orig_start.set(scr.cpy_start.get());
    scr.orig_end.set(scr.cpy_end.get());
    scr.cursor.set(scr.cpy_end.get().wrapping_add(1)); // init value, never happen
    inverse_region(scr, scr.cpy_start.get(), scr.cpy_end.get());

    scr.mset(SEL_IN_PROGRESS | SEL_EXISTS | SEL_BY_WORD);
    scr.mclr(SEL_BY_CHAR | SEL_BY_LINE);
    // mouse cursor hidden in the selection
    scr.mclr(BLANK_TO_EOL | MOUSE_VISIBLE);
}

/// `mouse_copy_line`: copy of the current line.
fn mouse_copy_line(scr: &Wsscreen) {
    let dconf = scr.dconf();
    let row = scr.mouse.get() / dconf.n_cols();

    if scr.mflag(SEL_EXISTS) {
        remove_selection(scr);
    }

    if scr.mflag(MOUSE_VISIBLE) {
        inverse_char(scr, scr.mouse.get());
    }

    scr.cpy_start.set(row * dconf.n_cols());
    scr.cpy_end.set(scr.cpy_start.get() + (dconf.n_cols() - 1));
    scr.orig_start.set(scr.cpy_start.get());
    scr.orig_end.set(scr.cpy_end.get());
    scr.cursor.set(scr.cpy_end.get().wrapping_add(1));
    inverse_region(scr, scr.cpy_start.get(), scr.cpy_end.get());

    scr.mset(SEL_IN_PROGRESS | SEL_EXISTS | SEL_BY_LINE);
    scr.mclr(SEL_BY_CHAR | SEL_BY_WORD);
    // mouse cursor hidden in the selection
    scr.mclr(BLANK_TO_EOL | MOUSE_VISIBLE);
}

/// `mouse_copy_end`: end of a copy operation.
fn mouse_copy_end(scr: &Wsscreen) {
    scr.mclr(SEL_IN_PROGRESS);
    if scr.mflag(SEL_BY_WORD) || scr.mflag(SEL_BY_LINE) {
        if scr.cursor.get() != scr.cpy_end.get().wrapping_add(1) {
            inverse_char(scr, scr.cursor.get());
        }
        scr.cursor.set(scr.cpy_end.get().wrapping_add(1));
    }
}

/// `mouse_copy_extend`: generic selection extend function.
fn mouse_copy_extend(scr: &Wsscreen) {
    if scr.mflag(SEL_BY_CHAR) {
        mouse_copy_extend_char(scr);
    }
    if scr.mflag(SEL_BY_WORD) {
        mouse_copy_extend_word(scr);
    }
    if scr.mflag(SEL_BY_LINE) {
        mouse_copy_extend_line(scr);
    }
}

/// `mouse_copy_extend_char`: extend a selected region, character by character.
fn mouse_copy_extend_char(scr: &Wsscreen) {
    if !scr.mflag(SEL_EXT_AFTER) {
        if scr.mflag(BLANK_TO_EOL) {
            // First extension of selection. We handle special cases of blank characters to
            // eol

            let right = skip_spc_right(scr, BORDER);
            if scr.mouse.get() > scr.orig_start.get() {
                // the selection goes to the lower part of the screen

                // remove the previous cursor, start of selection is now next line
                inverse_char(scr, scr.cpy_start.get());
                scr.cpy_start
                    .set(scr.cpy_start.get().wrapping_add(right.wrapping_add(1)));
                scr.cpy_end.set(scr.cpy_start.get());
                scr.orig_start.set(scr.cpy_start.get());
                // simulate the initial mark
                inverse_char(scr, scr.cpy_start.get());
            } else {
                // the selection goes to the upper part of the screen
                // remove the previous cursor, start of selection is now at the eol
                inverse_char(scr, scr.cpy_start.get());
                scr.orig_start
                    .set(scr.orig_start.get().wrapping_add(right.wrapping_add(1)));
                scr.cpy_start.set(scr.orig_start.get().wrapping_sub(1));
                scr.cpy_end.set(scr.orig_start.get().wrapping_sub(1));
                // simulate the initial mark
                inverse_char(scr, scr.cpy_start.get());
            }
            scr.mclr(BLANK_TO_EOL);
        }

        if scr.mouse.get() < scr.orig_start.get() && scr.cpy_end.get() >= scr.orig_start.get() {
            // we go to the upper part of the screen

            // reverse the old selection region
            remove_selection(scr);
            scr.cpy_end.set(scr.orig_start.get().wrapping_sub(1));
            scr.cpy_start.set(scr.orig_start.get());
        }
        if scr.cpy_start.get() < scr.orig_start.get() && scr.mouse.get() >= scr.orig_start.get() {
            // we go to the lower part of the screen

            // reverse the old selection region

            remove_selection(scr);
            scr.cpy_start.set(scr.orig_start.get());
            scr.cpy_end.set(scr.orig_start.get().wrapping_sub(1));
        }
        // restore flags cleared in remove_selection()
        scr.mset(SEL_IN_PROGRESS | SEL_EXISTS);
    }

    if scr.mouse.get() >= scr.orig_start.get() {
        // lower part of the screen
        if scr.mouse.get() > scr.cpy_end.get() {
            // extending selection
            inverse_region(scr, scr.cpy_end.get().wrapping_add(1), scr.mouse.get());
        } else {
            // reducing selection
            inverse_region(scr, scr.mouse.get().wrapping_add(1), scr.cpy_end.get());
        }
        scr.cpy_end.set(scr.mouse.get());
    } else {
        // upper part of the screen
        if scr.mouse.get() < scr.cpy_start.get() {
            // extending selection
            inverse_region(scr, scr.mouse.get(), scr.cpy_start.get().wrapping_sub(1));
        } else {
            // reducing selection
            inverse_region(scr, scr.cpy_start.get(), scr.mouse.get().wrapping_sub(1));
        }
        scr.cpy_start.set(scr.mouse.get());
    }
}

/// `mouse_copy_extend_word`: extend a selected region, word by word.
fn mouse_copy_extend_word(scr: &Wsscreen) {
    if !scr.mflag(SEL_EXT_AFTER) {
        // remove cursor in selection (black one)
        if scr.cursor.get() != scr.cpy_end.get().wrapping_add(1) {
            inverse_char(scr, scr.cursor.get());
        }

        // now, switch between lower and upper part of the screen
        if scr.mouse.get() < scr.orig_start.get() && scr.cpy_end.get() >= scr.orig_start.get() {
            // going to the upper part of the screen
            inverse_region(scr, scr.orig_end.get().wrapping_add(1), scr.cpy_end.get());
            scr.cpy_end.set(scr.orig_end.get());
        }

        if scr.mouse.get() > scr.orig_end.get() && scr.cpy_start.get() <= scr.orig_start.get() {
            // going to the lower part of the screen
            inverse_region(
                scr,
                scr.cpy_start.get(),
                scr.orig_start.get().wrapping_sub(1),
            );
            scr.cpy_start.set(scr.orig_start.get());
        }
    }

    let mouse = scr.mouse.get();
    if mouse >= scr.orig_start.get() {
        // lower part of the screen
        if mouse > scr.cpy_end.get() {
            // extending selection
            let old_cpy_end = scr.cpy_end.get();
            scr.cpy_end
                .set(mouse.wrapping_add(skip_char_right(scr, mouse)));
            inverse_region(scr, old_cpy_end.wrapping_add(1), scr.cpy_end.get());
        } else if class_cmp(scr, mouse, mouse.wrapping_add(1)) != 0 {
            // reducing selection (remove last word)
            let old_cpy_end = scr.cpy_end.get();
            scr.cpy_end.set(mouse);
            inverse_region(scr, scr.cpy_end.get().wrapping_add(1), old_cpy_end);
        } else {
            let old_cpy_end = scr.cpy_end.get();
            scr.cpy_end
                .set(mouse.wrapping_add(skip_char_right(scr, mouse)));
            if scr.cpy_end.get() != old_cpy_end {
                // reducing selection, from the end of next word
                inverse_region(scr, scr.cpy_end.get().wrapping_add(1), old_cpy_end);
            }
        }
    } else {
        // upper part of the screen
        if mouse < scr.cpy_start.get() {
            // extending selection
            let old_cpy_start = scr.cpy_start.get();
            scr.cpy_start
                .set(mouse.wrapping_sub(skip_char_left(scr, mouse)));
            inverse_region(scr, scr.cpy_start.get(), old_cpy_start.wrapping_sub(1));
        } else if class_cmp(scr, mouse.wrapping_sub(1), mouse) != 0 {
            // reducing selection (remove last word)
            let old_cpy_start = scr.cpy_start.get();
            scr.cpy_start.set(mouse);
            inverse_region(scr, old_cpy_start, scr.cpy_start.get().wrapping_sub(1));
        } else {
            let old_cpy_start = scr.cpy_start.get();
            scr.cpy_start
                .set(mouse.wrapping_sub(skip_char_left(scr, mouse)));
            if scr.cpy_start.get() != old_cpy_start {
                inverse_region(scr, old_cpy_start, scr.cpy_start.get().wrapping_sub(1));
            }
        }
    }

    if !scr.mflag(SEL_EXT_AFTER) {
        // display new cursor
        scr.cursor.set(scr.mouse.get());
        inverse_char(scr, scr.cursor.get());
    }
}

/// `mouse_copy_extend_line`: extend a selected region, line by line.
fn mouse_copy_extend_line(scr: &Wsscreen) {
    let dconf = scr.dconf();
    let n = dconf.n_cols();

    if !scr.mflag(SEL_EXT_AFTER) {
        // remove cursor in selection (black one)
        if scr.cursor.get() != scr.cpy_end.get().wrapping_add(1) {
            inverse_char(scr, scr.cursor.get());
        }

        // now, switch between lower and upper part of the screen
        if scr.mouse.get() < scr.orig_start.get() && scr.cpy_end.get() >= scr.orig_start.get() {
            // going to the upper part of the screen
            inverse_region(scr, scr.orig_end.get().wrapping_add(1), scr.cpy_end.get());
            scr.cpy_end.set(scr.orig_end.get());
        }

        if scr.mouse.get() > scr.orig_end.get() && scr.cpy_start.get() <= scr.orig_start.get() {
            // going to the lower part of the screen
            inverse_region(
                scr,
                scr.cpy_start.get(),
                scr.orig_start.get().wrapping_sub(1),
            );
            scr.cpy_start.set(scr.orig_start.get());
        }
    }

    if scr.mouse.get() >= scr.orig_start.get() {
        // lower part of the screen
        if scr.cursor.get() == scr.cpy_end.get().wrapping_add(1) {
            scr.cursor.set(scr.cpy_end.get());
        }
        let old_row = scr.cursor.get() / n;
        let new_row = scr.mouse.get() / n;
        let old_cpy_end = scr.cpy_end.get();
        scr.cpy_end.set(new_row * n + dconf.maxcol());
        if new_row > old_row {
            inverse_region(scr, old_cpy_end.wrapping_add(1), scr.cpy_end.get());
        } else if new_row < old_row {
            inverse_region(scr, scr.cpy_end.get().wrapping_add(1), old_cpy_end);
        }
    } else {
        // upper part of the screen
        let old_row = scr.cursor.get() / n;
        let new_row = scr.mouse.get() / n;
        let old_cpy_start = scr.cpy_start.get();
        scr.cpy_start.set(new_row * n);
        if new_row < old_row {
            inverse_region(scr, scr.cpy_start.get(), old_cpy_start.wrapping_sub(1));
        } else if new_row > old_row {
            inverse_region(scr, old_cpy_start, scr.cpy_start.get().wrapping_sub(1));
        }
    }

    if !scr.mflag(SEL_EXT_AFTER) {
        // display new cursor
        scr.cursor.set(scr.mouse.get());
        inverse_char(scr, scr.cursor.get());
    }
}

/// `mouse_copy_extend_after`: add an extension to a selected region, word by word.
fn mouse_copy_extend_after(scr: &Wsscreen) {
    if scr.mflag(SEL_EXISTS) {
        scr.mset(SEL_EXT_AFTER);
        mouse_hide(scr); // hide current cursor

        let mouse = scr.mouse.get();
        let start_dist = scr.cpy_start.get().abs_diff(mouse);
        let end_dist = mouse.abs_diff(scr.cpy_end.get());
        if start_dist < end_dist {
            // upper part of the screen
            scr.orig_start.set(mouse.wrapping_add(1));
            // only used in mouse_copy_extend_line()
            scr.cursor.set(scr.cpy_start.get());
        } else {
            // lower part of the screen
            scr.orig_start.set(mouse);
            // only used in mouse_copy_extend_line()
            scr.cursor.set(scr.cpy_end.get());
        }
        if scr.mflag(SEL_BY_CHAR) {
            mouse_copy_extend_char(scr);
        }
        if scr.mflag(SEL_BY_WORD) {
            mouse_copy_extend_word(scr);
        }
        if scr.mflag(SEL_BY_LINE) {
            mouse_copy_extend_line(scr);
        }
        mouse_copy_selection(scr);
    }
}

/// `mouse_hide`.
fn mouse_hide(scr: &Wsscreen) {
    if scr.mflag(MOUSE_VISIBLE) {
        inverse_char(scr, scr.mouse.get());
        scr.mclr(MOUSE_VISIBLE);
    }
}

/// `remove_selection`: remove a previously selected region.
fn remove_selection(scr: &Wsscreen) {
    if scr.mflag(SEL_EXT_AFTER) {
        // reset the flag indicating an extension of selection
        scr.mclr(SEL_EXT_AFTER);
    }
    inverse_region(scr, scr.cpy_start.get(), scr.cpy_end.get());
    scr.mclr(SEL_IN_PROGRESS | SEL_EXISTS);
}

/// `sc->sc_copybuffer[i] = c`, inside the buffer's `sc_copybuffer_size` bytes.
fn copybuffer_put(sc: &WsdisplaySoftc, i: u32, c: u8) {
    if let Some(p) = sc.sc_copybuffer.get()
        && i < sc.sc_copybuffer_size.get()
    {
        // SAFETY: `allocate_copybuffer`'s allocation of `sc_copybuffer_size` bytes, and `i`
        // is below that; the selection code touches it under the kernel lock, one call at a
        // time.
        unsafe { p.as_ptr().add(i as usize).write(c) };
    }
}

/// The copy buffer's string, up to its NUL (empty without a buffer).
fn copybuffer_str(sc: &WsdisplaySoftc) -> &[u8] {
    let Some(p) = sc.sc_copybuffer.get() else {
        return &[];
    };
    // SAFETY: as in `copybuffer_put`; nothing writes it while this borrow is read.
    let buf =
        unsafe { core::slice::from_raw_parts(p.as_ptr(), sc.sc_copybuffer_size.get() as usize) };
    cstr(buf)
}

/// `mouse_copy_selection`: put the current visual selection in the selection buffer.
fn mouse_copy_selection(scr: &Wsscreen) {
    let dconf = scr.dconf();
    let n = dconf.n_cols();
    let sc = scr.sc();
    let mut current: u32 = 0;
    let mut blank = current;
    let buf_end = (n + 1) * dconf.n_rows();

    let mut sel_cur = scr.cpy_start.get();
    let sel_end = scr.cpy_end.get();

    let put = |i: u32, c: u8| copybuffer_put(sc, i, c);
    while sel_cur <= sel_end && current < buf_end - 1 {
        let Some(cell) = scr.getchar(sel_cur) else {
            break;
        };
        put(current, cell.uc as u8);
        if !is_space(cell.uc) {
            blank = current + 1; // first blank after non-blank
        }
        current += 1;
        if sel_cur % n == dconf.maxcol() {
            // If we are on the last column of the screen, insert a carriage return.
            put(blank, b'\r');
            blank += 1;
            current = blank;
        }
        sel_cur = sel_cur.wrapping_add(1);
    }

    put(current, 0);
}

/// `mouse_paste`: paste the current selection.
fn mouse_paste(scr: &Wsscreen) {
    let sc = scr.sc();
    if sc.flag(SC_PASTE_AVAIL) {
        let Some(tp) = scr.tty() else {
            return;
        };

        for &c in copybuffer_str(sc) {
            let _ = (linesw(tp).l_rint)(i32::from(c), tp);
        }
    }
}

/// `mouse_zaxis`: handle the z axis. The z axis (roller or wheel) is mapped by default to
/// scrollback (`HAVE_SCROLLBACK_SUPPORT`).
fn mouse_zaxis(scr: &Wsscreen, z: i32) {
    let arg = scr.sc().as_arg();
    if z < 0 {
        wsscrollback(arg, WSDISPLAY_SCROLL_BACKWARD);
    } else {
        wsscrollback(arg, WSDISPLAY_SCROLL_FORWARD);
    }
}

/// `allocate_copybuffer`: allocate the copy buffer. The size is `(cols + 1) * rows` (+1 for
/// `'\n'` at the end of lines), where cols and rows are the maximum of column and rows of all
/// screens.
fn allocate_copybuffer(sc: &WsdisplaySoftc) {
    let mut size = sc.sc_copybuffer_size.get();

    let s = spltty();
    // SAFETY: the display's screen types, which live for good.
    for &current in unsafe { sc.scrdata().screens() } {
        // SAFETY: as above.
        let current = unsafe { &*current };
        let need = ((current.ncols + 1) * current.nrows) as u32;
        if need > size {
            size = need;
        }
    }
    if let Some(old) = sc.sc_copybuffer.get() {
        let oldsize = sc.sc_copybuffer_size.get() as usize;
        // SAFETY: the previous buffer of `oldsize` bytes, ours.
        unsafe { ptr::write_bytes(old.as_ptr(), 0, oldsize) };
        free(old, M_DEVBUF, oldsize);
        sc.sc_copybuffer.set(None);
    }
    let buf = if size == 0 {
        None
    } else {
        malloc(size as usize, M_DEVBUF, M_NOWAIT | M_ZERO)
    };
    if buf.is_none() && size != 0 {
        printf(format_args!(
            "{}: couldn't allocate copy buffer\n",
            sc.sc_dv.xname()
        ));
        size = 0;
    }
    sc.sc_copybuffer.set(buf);
    sc.sc_copybuffer_size.set(size);
    splx(s);
}

/// `mouse_remove`: remove selection and cursor on current screen.
fn mouse_remove(scr: &Wsscreen) {
    if scr.mflag(SEL_EXISTS) {
        remove_selection(scr);
    }

    mouse_hide(scr);
}

#[cfg(test)]
mod tests;
