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
 * Copyright (c) 1996, 1997 Christopher G. Demetriou.  All rights reserved.
 *
 * Keysym translator:
 * Contributed to The NetBSD Foundation by Juergen Hannken-Illjes.
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
 *	@(#)kbd.c	8.2 (Berkeley) 10/30/93
 */
/* </LICENSES> */

/* <CODE> */
//! `wskbd(4)`: the wscons keyboard driver (`wskbd* at ukbd? mux 1`, `/dev/wskbd*`).
//! Translates incoming key codes to keysyms or to `wscons_event`s and passes them up to the
//! appropriate reader.
//!
//! Upstream: sys/dev/wscons/wskbd.c @ 3ce1f3f79392
//!
//! A keyboard driver (`ukbd(4)` through `hidkbd`) attaches a `wskbd` child and calls
//! [`wskbd_input`] with each key press and release. While nobody has the keyboard open as
//! `/dev/wskbdN` (`sc_translating`), [`wskbd_translate`] turns the key code into keysyms
//! through the layout's keymap (`wskbdutil.rs`, the driver's layouts, `ukbdmap.rs` for USB),
//! with the modifiers, Caps and Num Lock, the compose key and dead accents, the control and
//! meta mappings, and the commands (`KS_Cmd_Screen*`, scrollback, ...), and hands the
//! keysyms to the display the keyboard is the input of (`wsdisplay_kbdinput`), repeating a
//! held key (`wskbd_repeat`). A keyboard joins the mux its `mux` locator names (`mux 1`, the
//! one `wsdisplay0` attaches to), which connects it to the mux's display; opened directly or
//! through an opened mux, it delivers raw [`WsconsEvent`]s to the reader's queue instead
//! ([`wskbd_deliver_event`]). The console keyboard ([`wskbd_cnattach`]) feeds the
//! console's `cngetc` by polling.
//!
//! ## Deviations
//! - The kernel configuration is GENERIC's: `NWSDISPLAY > 0`, `NWSMUX > 0`, `NAUDIO > 0`,
//!   `DDB`, `SUSPEND`, `HAVE_BURNER_SUPPORT` and `HAVE_SCROLLBACK_SUPPORT`, option
//!   `WSDISPLAY_COMPAT_RAWKBD` as the cargo feature `wsdisplay_compat_rawkbd`; the `#if
//!   NWSMUX == 0` path of `wskbd_attach` (wskbd0 bound to wsdisplay0 without a mux) is not
//!   compiled, as in C.
//! - `KS_Cmd_Sleep` (`SUSPEND`) reports the visible stub `unported!("request_sleep")`:
//!   `subr_suspend.c` is not ported. The key is still consumed.
//! - `KS_Cmd_KbdReset`'s `kbd_reset` (`#if defined(__i386__) || defined(__amd64__)`) is the
//!   machine's `machdep.kbdreset` through `machine::cpu::kbd_reset` (`None` on arm64, where
//!   the C compiles the case out: the key then does nothing, but is consumed, as any
//!   command the switch knows).
//! - `struct wskbd_internal`'s `t_keymap` (a copy of the driver's `wskbd_mapdata`) is the
//!   two `Cell`s `t_keydesc` and `t_layout`; [`WskbdInternal::keymap`] makes the
//!   `WskbdMapdata` `wskbdutil.rs` takes. The console keyboard's internal state
//!   (`wskbd_console_data`) is a `static` of `Cell`s, changed under the kernel lock at
//!   `spltty`, as in C; a non-console keyboard's is `malloc`ed at attach and, as in C, never
//!   freed.
//! - `struct wskbd_softc` is `#[repr(C)]` with its `Wsevsrc` first and all-zero valid (the
//!   `config_make_softc` contract): the members are `Cell`s, `sc_map` a raw pointer beside
//!   `sc_maplen` (the `malloc`ed keymap, `docs/C_TO_RUST.md`), `id` a `NonNull`.
//!   `sc_kbd_backlight_cmd` and `sc_brightness_steps`, which the C reads with atomic
//!   operations, are atomics.
//! - `wskbd_get_backlight`, `wskbd_set_backlight` (hooks a backlight driver installs; none
//!   does yet), `wskbd_default_bell_data` and `wskbd_default_keyrepeat_data` are
//!   `StaticCell`s changed under the kernel lock; `wskbd_console_initted` and
//!   `wskbd_console_device` are atomics; `wskbd_cngetc`'s static `num` and `pos` too.
//! - The access operations' `int` errno (`enable`) becomes `Result`; `ioctl` is
//!   `wskbdvar.rs`'s `Ok(true)`/`Ok(false)` (the C's 0/-1). [`wskbd_displayioctl_sc`] and
//!   [`wskbd_displayioctl`] return `Ok(false)` for the C's -1 too, and take the kernel copy
//!   of the ioctl argument as bytes and an `Option<&Proc>` (a mux passes NULL; `suser` of
//!   no thread is `EPERM`).
//! - `internal_command` returns a `bool` (the C's 0/1); `wskbd_translate` the number of
//!   symbols in `t_symbols`.
//! - `WSKBD_DEBUG`'s `DPRINTF`s and `DEBUG`'s key code range message are not carried; the
//!   `DIAGNOSTIC` checks of `wskbdread` and `wskbd_deliver_event` are behind the
//!   `diagnostic` feature.
//! - The device entry points answer `ENXIO` for a minor without a keyboard where the C
//!   indexes `wskbd_cd.cd_devs` unchecked (only `wskbdopen` checks there).

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicPtr, AtomicU32, Ordering};

use libkern::StaticCell;

use crate::dev::audio::{AUDIO_KBDCONTROL_ENABLE, wskbd_set_mixervolume_dev};
use crate::dev::wscons::wscons_callbacks::{
    WSDISPLAY_RESETCLOSE, WSDISPLAY_RESETEMUL, wsdisplay_kbdholdscreen, wsdisplay_kbdinput,
    wsdisplay_param, wsdisplay_rawkbdinput, wsdisplay_reset, wsdisplay_set_cons_kbd,
    wsdisplay_set_console_kbd, wsdisplay_switch, wsdisplay_unset_cons_kbd,
};
use crate::dev::wscons::wscons_features::{HAVE_BURNER_SUPPORT, HAVE_SCROLLBACK_SUPPORT};
use crate::dev::wscons::wsconsio::{
    WSCONS_EVENT_ALL_KEYS_UP, WSCONS_EVENT_KEY_DOWN, WSCONS_EVENT_KEY_UP, WSDISPLAY_BURN_KBD,
    WSDISPLAYIO_GETPARAM, WSDISPLAYIO_PARAM_BACKLIGHT, WSDISPLAYIO_PARAM_CONTRAST,
    WSDISPLAYIO_SETPARAM, WSKBD_BELL_DOALL, WSKBD_BELL_DOPERIOD, WSKBD_BELL_DOPITCH,
    WSKBD_BELL_DOVOLUME, WSKBD_KEYREPEAT_DOALL, WSKBD_KEYREPEAT_DODEL1, WSKBD_KEYREPEAT_DODELN,
    WSKBD_LED_CAPS, WSKBD_LED_COMPOSE, WSKBD_LED_NUM, WSKBD_LED_SCROLL, WSKBD_RAW, WSKBDIO_BELL,
    WSKBDIO_COMPLEXBELL, WSKBDIO_GETBACKLIGHT, WSKBDIO_GETBELL, WSKBDIO_GETDEFAULTBELL,
    WSKBDIO_GETDEFAULTKEYREPEAT, WSKBDIO_GETENCODING, WSKBDIO_GETENCODINGS, WSKBDIO_GETKEYREPEAT,
    WSKBDIO_GETMAP, WSKBDIO_MAXMAPLEN, WSKBDIO_SETBACKLIGHT, WSKBDIO_SETBELL,
    WSKBDIO_SETDEFAULTBELL, WSKBDIO_SETDEFAULTKEYREPEAT, WSKBDIO_SETENCODING, WSKBDIO_SETKEYREPEAT,
    WSKBDIO_SETMAP, WSKBDIO_SETMODE, WSMUX_KBD, WsconsEvent, WsdisplayParam, WskbdBacklight,
    WskbdBellData, WskbdEncodingData, WskbdKeyrepeatData, WskbdMapData,
};
use crate::dev::wscons::wsdisplay::{
    wsdisplay_brightness_cycle, wsdisplay_brightness_step, wsdisplay_burn, wsscrollback,
};
use crate::dev::wscons::wsdisplayvar::{
    WSDISPLAY_SCROLL_BACKWARD, WSDISPLAY_SCROLL_FORWARD, WSDISPLAY_SCROLL_RESET,
};
use crate::dev::wscons::wsevent::{wsevent_fini, wsevent_init, wsevent_kqfilter, wsevent_read};
use crate::dev::wscons::wseventvar::{WSEVENT_QSIZE, Wseventvar, wsevent_wakeup};
use crate::dev::wscons::wskbdutil::{
    ksym_upcase, wskbd_compose_value, wskbd_get_mapentry, wskbd_init_keymap, wskbd_load_keymap,
};
use crate::dev::wscons::wskbdvar::{
    WSKBDDEVCF_CONSOLE_UNK, WskbdAccessops, WskbdConsops, WskbddevAttachArgs, wskbddevcf_console,
    wskbddevcf_mux,
};
use crate::dev::wscons::wsksymdef::*;
use crate::dev::wscons::wsksymvar::{
    KB_HANDLEDBYWSKBD, KbdT, KeysymT, WsconsKeydesc, WsconsKeymap, WskbdMapdata,
};
use crate::dev::wscons::wsmux::{
    wsmux_attach_sc, wsmux_detach_sc, wsmux_get_layout, wsmux_getmux, wsmux_set_layout,
};
use crate::dev::wscons::wsmuxvar::{Wsevsrc, WsmuxSoftc, Wssrcops};
use crate::kern::init_main::INITPROCESS;
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_malloc::{free, malloc, mallocarray};
use crate::kern::kern_prot::suser;
use crate::kern::kern_sig::{prsignal, sigio_getown, sigio_setown};
use crate::kern::kern_synch::{tsleep_nsec, wakeup};
use crate::kern::kern_task::{SYSTQ, task_add, task_set};
use crate::kern::kern_tc::nanotime;
use crate::kern::kern_timeout::{timeout_add_msec, timeout_del, timeout_set};
use crate::kern::subr_prf::{DB_CONSOLE, Str, log, panic, printf};
use crate::kern::vfs_subr::vdevgone;
use crate::machine::conf::{cdevsw, nchrdev};
use crate::machine::copy::{copyin, copyout};
use crate::machine::db_machdep::db_enter;
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
use crate::sys::malloc::{M_DEVBUF, M_TEMP, M_WAITOK, M_ZERO};
use crate::sys::param::PZERO;
use crate::sys::proc::Proc;
use crate::sys::signal::SIGUSR1;
use crate::sys::syslog::LOG_WARNING;
use crate::sys::task::Task;
use crate::sys::time::sec_to_nsec;
use crate::sys::timeout::Timeout;
use crate::sys::ttycom::{TIOCGPGRP, TIOCSPGRP};
use crate::sys::types::{Dev, minor};
use crate::sys::uio::Uio;
use crate::sys::vnode::VCHR;
use crate::unported;

/// `NWSKBD`: `wskbd* at ...` is configured (`needs-flag`).
pub const NWSKBD: i32 = 1;

/// `WSKFL_METAESC`: Meta sends ESC and the key (`KB_METAESC`).
const WSKFL_METAESC: i32 = 1;

/// `MAXKEYSYMSPERKEY`: ESC <key> at max.
const MAXKEYSYMSPERKEY: usize = 2;

/// `enum wskbd_kbd_backlight_cmds`: `KBD_BACKLIGHT_NONE`.
const KBD_BACKLIGHT_NONE: u32 = 0;
/// `KBD_BACKLIGHT_UP`.
const KBD_BACKLIGHT_UP: u32 = 1;
/// `KBD_BACKLIGHT_DOWN`.
const KBD_BACKLIGHT_DOWN: u32 = 2;
/// `KBD_BACKLIGHT_TOGGLE`.
const KBD_BACKLIGHT_TOGGLE: u32 = 3;

/// `MOD_SHIFT_L`.
pub const MOD_SHIFT_L: i32 = 1 << 0;
/// `MOD_SHIFT_R`.
pub const MOD_SHIFT_R: i32 = 1 << 1;
/// `MOD_SHIFTLOCK`.
pub const MOD_SHIFTLOCK: i32 = 1 << 2;
/// `MOD_CAPSLOCK`.
pub const MOD_CAPSLOCK: i32 = 1 << 3;
/// `MOD_CONTROL_L`.
pub const MOD_CONTROL_L: i32 = 1 << 4;
/// `MOD_CONTROL_R`.
pub const MOD_CONTROL_R: i32 = 1 << 5;
/// `MOD_META_L`.
pub const MOD_META_L: i32 = 1 << 6;
/// `MOD_META_R`.
pub const MOD_META_R: i32 = 1 << 7;
/// `MOD_MODESHIFT`.
pub const MOD_MODESHIFT: i32 = 1 << 8;
/// `MOD_NUMLOCK`.
pub const MOD_NUMLOCK: i32 = 1 << 9;
/// `MOD_COMPOSE`.
pub const MOD_COMPOSE: i32 = 1 << 10;
/// `MOD_HOLDSCREEN`.
pub const MOD_HOLDSCREEN: i32 = 1 << 11;
/// `MOD_COMMAND`.
pub const MOD_COMMAND: i32 = 1 << 12;
/// `MOD_COMMAND1`.
pub const MOD_COMMAND1: i32 = 1 << 13;
/// `MOD_COMMAND2`.
pub const MOD_COMMAND2: i32 = 1 << 14;
/// `MOD_MODELOCK`.
pub const MOD_MODELOCK: i32 = 1 << 15;

/// `MOD_ANYSHIFT`.
pub const MOD_ANYSHIFT: i32 = MOD_SHIFT_L | MOD_SHIFT_R | MOD_SHIFTLOCK;
/// `MOD_ANYCONTROL`.
pub const MOD_ANYCONTROL: i32 = MOD_CONTROL_L | MOD_CONTROL_R;
/// `MOD_ANYMETA`.
pub const MOD_ANYMETA: i32 = MOD_META_L | MOD_META_R;
/// `MOD_ANYLED`.
pub const MOD_ANYLED: i32 =
    MOD_SHIFTLOCK | MOD_CAPSLOCK | MOD_NUMLOCK | MOD_COMPOSE | MOD_HOLDSCREEN;

/// `WSKBD_DEFAULT_BELL_PITCH`: 400Hz.
pub const WSKBD_DEFAULT_BELL_PITCH: u32 = 400;
/// `WSKBD_DEFAULT_BELL_PERIOD`: 100ms.
pub const WSKBD_DEFAULT_BELL_PERIOD: u32 = 100;
/// `WSKBD_DEFAULT_BELL_VOLUME`: 50% volume.
pub const WSKBD_DEFAULT_BELL_VOLUME: u32 = 50;

/// `WSKBD_DEFAULT_KEYREPEAT_DEL1`: 400ms to start repeating.
pub const WSKBD_DEFAULT_KEYREPEAT_DEL1: u32 = 400;
/// `WSKBD_DEFAULT_KEYREPEAT_DELN`: 100ms to between repeats.
pub const WSKBD_DEFAULT_KEYREPEAT_DELN: u32 = 100;

/// `struct wskbd_internal`: the keyboard's translation state, shared by the console
/// keyboard before and after it attaches.
pub struct WskbdInternal {
    /// `t_consops`.
    t_consops: Cell<Option<&'static WskbdConsops>>,
    /// `t_consaccesscookie`.
    t_consaccesscookie: Cell<*mut c_void>,
    /// `t_modifiers`: `MOD_*`.
    t_modifiers: Cell<i32>,
    /// `t_composelen`: remaining entries in `t_composebuf`.
    t_composelen: Cell<i32>,
    /// `t_composebuf`.
    t_composebuf: Cell<[KeysymT; 2]>,
    /// `t_flags`: `WSKFL_*`.
    t_flags: Cell<i32>,
    /// `t_symbols`: what the last key press translated to.
    t_symbols: Cell<[KeysymT; MAXKEYSYMSPERKEY]>,
    /// `t_sc`: back pointer.
    t_sc: Cell<Option<NonNull<WskbdSoftc>>>,
    /// `t_keymap.keydesc`: translation map table.
    t_keydesc: Cell<&'static [WsconsKeydesc]>,
    /// `t_keymap.layout`: current layout.
    t_layout: Cell<KbdT>,
}

// SAFETY: `wskbd_console_data` is the one shared instance; it is changed by the console
// keyboard's attach and the keyboard paths, under the kernel lock at `spltty`, as in C.
unsafe impl Sync for WskbdInternal {}

impl WskbdInternal {
    /// An empty state (the C's zeroed `wskbd_console_data` or `M_ZERO` allocation).
    pub const fn new() -> Self {
        Self {
            t_consops: Cell::new(None),
            t_consaccesscookie: Cell::new(ptr::null_mut()),
            t_modifiers: Cell::new(0),
            t_composelen: Cell::new(0),
            t_composebuf: Cell::new([0; 2]),
            t_flags: Cell::new(0),
            t_symbols: Cell::new([0; MAXKEYSYMSPERKEY]),
            t_sc: Cell::new(None),
            t_keydesc: Cell::new(&[]),
            t_layout: Cell::new(KB_NONE),
        }
    }

    /// `t_keymap` as the `struct wskbd_mapdata` `wskbdutil.c` takes.
    pub fn keymap(&self) -> WskbdMapdata {
        WskbdMapdata::new(self.t_keydesc.get(), self.t_layout.get())
    }

    /// `t_sc`.
    fn sc(&self) -> Option<&'static WskbdSoftc> {
        // SAFETY: `t_sc` is the keyboard `wskbd_attach` gave this state to; a keyboard's
        // state is reached only through the keyboard (or, for the console's, through the
        // console entry points, which `wskbd_cndetach` disconnects when it detaches).
        self.t_sc.get().map(|p| unsafe { &*p.as_ptr() })
    }

    /// `MOD_ONESET(id, mask)`.
    fn mod_oneset(&self, mask: i32) -> bool {
        self.t_modifiers.get() & mask != 0
    }

    /// `MOD_ALLSET(id, mask)`.
    fn mod_allset(&self, mask: i32) -> bool {
        self.t_modifiers.get() & mask == mask
    }
}

impl Default for WskbdInternal {
    fn default() -> Self {
        Self::new()
    }
}

/// `struct wskbd_softc`.
#[repr(C)]
pub struct WskbdSoftc {
    /// `sc_base`.
    pub sc_base: Wsevsrc,
    /// `id`: the translation state.
    id: Cell<Option<NonNull<WskbdInternal>>>,
    /// `sc_accessops`.
    sc_accessops: Cell<Option<&'static WskbdAccessops>>,
    /// `sc_accesscookie`.
    sc_accesscookie: Cell<*mut c_void>,
    /// `sc_ledstate`.
    sc_ledstate: Cell<i32>,
    /// `sc_isconsole`.
    sc_isconsole: Cell<i32>,
    /// `sc_bell_data`.
    sc_bell_data: Cell<WskbdBellData>,
    /// `sc_keyrepeat_data`.
    sc_keyrepeat_data: Cell<WskbdKeyrepeatData>,
    /// `sc_repeating`: we've called timeout() (the number of symbols to repeat).
    sc_repeating: Cell<i32>,
    /// `sc_repkey`.
    sc_repkey: Cell<i32>,
    /// `sc_repeat_ch`.
    sc_repeat_ch: Timeout,
    /// `sc_repeat_type`.
    sc_repeat_type: Cell<u32>,
    /// `sc_repeat_value`.
    sc_repeat_value: Cell<i32>,
    /// `sc_translating`: xlate to chars for emulation.
    sc_translating: Cell<i32>,
    /// `sc_maplen`: number of entries in `sc_map`.
    sc_maplen: Cell<i32>,
    /// `sc_map`: current translation map (`malloc`ed, `sc_maplen` entries).
    sc_map: Cell<*mut WsconsKeymap>,
    /// `sc_refcnt`.
    sc_refcnt: Cell<i32>,
    /// `sc_dying`: device is being detached.
    sc_dying: Cell<u8>,
    /// `sc_audiocookie` (`NAUDIO > 0`).
    sc_audiocookie: Cell<*mut c_void>,
    /// `sc_kbd_backlight_task`.
    sc_kbd_backlight_task: Task,
    /// `sc_kbd_backlight_cmd`: `KBD_BACKLIGHT_*`.
    sc_kbd_backlight_cmd: AtomicU32,
    /// `sc_brightness_task` (`NWSDISPLAY > 0`).
    sc_brightness_task: Task,
    /// `sc_brightness_steps`.
    sc_brightness_steps: AtomicI32,
}

// SAFETY: `#[repr(C)]`, its `Wsevsrc` (whose first member is the device) first, and every
// member valid all-zero (`Cell`s of integers, `None`s and null pointers, idle timeout and
// tasks).
unsafe impl Softc for WskbdSoftc {}

impl WskbdSoftc {
    /// `sc->id`, which every attached keyboard has.
    fn id(&self) -> &'static WskbdInternal {
        match self.id.get() {
            // SAFETY: `wskbd_attach` points `id` at `wskbd_console_data` or at a never-freed
            // allocation.
            Some(id) => unsafe { &*id.as_ptr() },
            None => panic(format_args!("{}: no wskbd_internal", self.xname())),
        }
    }

    /// `sc_accessops`, which every attached keyboard has.
    fn accessops(&self) -> &'static WskbdAccessops {
        match self.sc_accessops.get() {
            Some(ops) => ops,
            None => panic(format_args!("{}: no accessops", self.xname())),
        }
    }

    /// `sc_displaydv`.
    fn displaydv(&self) -> Option<&Device> {
        self.sc_base.dispdv()
    }

    /// `sc_base.me_dv.dv_xname`.
    fn xname(&self) -> &str {
        self.sc_base.me_dv.xname()
    }

    /// The softc as a timeout or task argument.
    fn as_arg(&self) -> *mut c_void {
        ptr::from_ref(self).cast_mut().cast()
    }

    /// Cancel the key repeat (`sc_repeating = 0; timeout_del(&sc_repeat_ch)`).
    fn stop_repeat(&self) {
        if self.sc_repeating.get() != 0 {
            self.sc_repeating.set(0);
            timeout_del(&self.sc_repeat_ch);
        }
    }
}

/// `wskbd_cd`.
pub static WSKBD_CD: Cfdriver = Cfdriver::new(b"wskbd", DV_TTY, 0);

/// `wskbd_ca`.
pub static WSKBD_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<WskbdSoftc>(),
    ca_match: Some(wskbd_match),
    ca_attach: wskbd_attach,
    ca_detach: Some(wskbd_detach),
    ca_activate: Some(wskbd_activate),
};

/// `wskbd_default_bell_data`; changed by `WSKBDIO_SETDEFAULTBELL` under the kernel lock.
static WSKBD_DEFAULT_BELL_DATA: StaticCell<WskbdBellData> = StaticCell::new(WskbdBellData {
    which: WSKBD_BELL_DOALL,
    pitch: WSKBD_DEFAULT_BELL_PITCH,
    period: WSKBD_DEFAULT_BELL_PERIOD,
    volume: WSKBD_DEFAULT_BELL_VOLUME,
});

/// `wskbd_default_keyrepeat_data`; changed by `WSKBDIO_SETDEFAULTKEYREPEAT` under the
/// kernel lock.
static WSKBD_DEFAULT_KEYREPEAT_DATA: StaticCell<WskbdKeyrepeatData> =
    StaticCell::new(WskbdKeyrepeatData {
        which: WSKBD_KEYREPEAT_DOALL,
        del1: WSKBD_DEFAULT_KEYREPEAT_DEL1,
        delN: WSKBD_DEFAULT_KEYREPEAT_DELN,
    });

/// `wskbd_srcops`: a keyboard as an event source of a mux.
pub static WSKBD_SRCOPS: Wssrcops = Wssrcops {
    type_: WSMUX_KBD,
    dopen: wskbd_mux_open,
    dclose: wskbd_mux_close,
    dioctl: wskbd_do_ioctl,
    ddispioctl: Some(wskbd_displayioctl),
    dsetdisplay: Some(wskbd_set_display),
};

/// `wskbd_console_initted`.
static WSKBD_CONSOLE_INITTED: AtomicBool = AtomicBool::new(false);
/// `wskbd_console_device`: NULL until the console keyboard attaches.
static WSKBD_CONSOLE_DEVICE: AtomicPtr<WskbdSoftc> = AtomicPtr::new(ptr::null_mut());
/// `wskbd_console_data`.
static WSKBD_CONSOLE_DATA: WskbdInternal = WskbdInternal::new();

/// The type of `wskbd_get_backlight` and `wskbd_set_backlight`.
pub type WskbdBacklightFn = fn(&mut WskbdBacklight) -> Result<(), Errno>;

/// `wskbd_get_backlight`: installed by a driver that controls the keyboard's backlight.
pub static WSKBD_GET_BACKLIGHT: StaticCell<Option<WskbdBacklightFn>> = StaticCell::new(None);
/// `wskbd_set_backlight`.
pub static WSKBD_SET_BACKLIGHT: StaticCell<Option<WskbdBacklightFn>> = StaticCell::new(None);

/// `wskbd_cngetc`'s `static int num`.
static CNGETC_NUM: AtomicI32 = AtomicI32::new(0);
/// `wskbd_cngetc`'s `static int pos`.
static CNGETC_POS: AtomicI32 = AtomicI32::new(0);

/// `wskbd_console_device`.
fn console_device() -> Option<&'static WskbdSoftc> {
    // SAFETY: NULL or the attached console keyboard, cleared by `wskbd_cndetach` before it
    // goes.
    unsafe { WSKBD_CONSOLE_DEVICE.load(Ordering::Acquire).as_ref() }
}

/// `(struct wskbd_softc *)dev`.
fn sc_of(dev: &Device) -> &'static WskbdSoftc {
    // SAFETY: `dev` is a wskbd device (made for `wskbd_ca`), which lives until detached;
    // the callers are its own entry points and its parent while it is attached.
    unsafe { &*ptr::from_ref(dev.softc::<WskbdSoftc>()) }
}

/// `wskbd_cd.cd_devs[unit]`.
fn wskbd_sc(unit: i32) -> Option<&'static WskbdSoftc> {
    let dev = WSKBD_CD.cd_dev(unit)?;
    // SAFETY: `wskbd_cd`'s devices are made by `config_make_softc` for `wskbd_ca`.
    Some(unsafe { &*dev.as_ptr().cast::<WskbdSoftc>() })
}

/// The `int` errno an access operation returns, as a `Result`.
fn errno_result(e: i32) -> Result<(), Errno> {
    match e {
        0 => Ok(()),
        e => Err(Errno::from_raw(e).unwrap_or(Errno::EIO)),
    }
}

/// `wskbd_update_layout`.
pub fn wskbd_update_layout(id: &WskbdInternal, enc: KbdT) {
    if enc & KB_METAESC != 0 {
        id.t_flags.set(id.t_flags.get() | WSKFL_METAESC);
    } else {
        id.t_flags.set(id.t_flags.get() & !WSKFL_METAESC);
    }

    id.t_layout.set(enc);
}

/// `wskbddevprint`: print function (for parent devices).
pub fn wskbddevprint(aux: *mut c_void, pnp: Option<&[u8]>) -> i32 {
    let _ = aux;
    if let Some(pnp) = pnp {
        printf(format_args!("wskbd at {}", Str(pnp)));
    }

    UNCONF
}

/// `wskbd_match`.
pub fn wskbd_match(_parent: Option<&Device>, match_: &CfMatch, aux: *mut c_void) -> i32 {
    let cf = match_.cfdata();
    // SAFETY: a wskbddev parent hands its child a `wskbddev_attach_args`.
    let ap = unsafe { &*aux.cast::<WskbddevAttachArgs>() };

    if wskbddevcf_console(cf) != WSKBDDEVCF_CONSOLE_UNK {
        // If console-ness of device specified, either match exactly (at high priority), or
        // fail.
        return if wskbddevcf_console(cf) != 0 && ap.console != 0 {
            10
        } else {
            0
        };
    }

    // If console-ness unspecified, it wins.
    1
}

/// `wskbd_attach`.
pub fn wskbd_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    let sc = sc_of(self_);
    // SAFETY: as in `wskbd_match`.
    let ap = unsafe { &*aux.cast::<WskbddevAttachArgs>() };

    sc.sc_isconsole.set(ap.console);

    sc.sc_base.me_ops.set(Some(&WSKBD_SRCOPS));
    let mux = wskbddevcf_mux(self_.cfdata()) as i32;
    let wsmux_sc = if mux >= 0 { wsmux_getmux(mux) } else { None };

    let id: &'static WskbdInternal = if ap.console != 0 {
        &WSKBD_CONSOLE_DATA
    } else {
        let Some(mem) = malloc(size_of::<WskbdInternal>(), M_DEVBUF, M_WAITOK | M_ZERO) else {
            panic(format_args!("wskbd_attach: no memory"));
        };
        let idp = mem.as_ptr().cast::<WskbdInternal>();
        // SAFETY: a fresh block of `size_of::<WskbdInternal>()` bytes, aligned by `malloc`;
        // the keyboard keeps it for good (the C never frees it either).
        unsafe { idp.write(WskbdInternal::new()) };
        // SAFETY: initialised above.
        let id = unsafe { &*idp };
        id.t_keydesc.set(ap.keymap.keydesc);
        id.t_layout.set(ap.keymap.layout());
        id
    };
    sc.id.set(Some(NonNull::from(id)));

    task_set(
        &sc.sc_kbd_backlight_task,
        wskbd_kbd_backlight_task,
        sc.as_arg(),
    );
    timeout_set(&sc.sc_repeat_ch, wskbd_repeat, sc.as_arg());
    task_set(&sc.sc_brightness_task, wskbd_brightness_task, sc.as_arg());

    sc.sc_audiocookie.set(ap.audiocookie);

    id.t_sc.set(Some(NonNull::from(sc)));

    sc.sc_accessops.set(Some(ap.accessops));
    sc.sc_accesscookie.set(ap.accesscookie);
    sc.sc_repeating.set(0);
    sc.sc_translating.set(1);
    sc.sc_ledstate.set(-1); // force update

    // If this layout is the default choice of the driver (i.e. the driver doesn't know
    // better), pick the existing layout of the current mux, if any.
    let mut layout = id.t_layout.get();
    if layout & KB_DEFAULT != 0
        && let Some(m) = wsmux_sc
        && wsmux_get_layout(m) != KB_NONE
    {
        layout = wsmux_get_layout(m);
    }
    loop {
        if let Ok((map, maplen)) = wskbd_load_keymap(&id.keymap(), layout) {
            wskbd_set_keymap(sc, map.as_ptr(), maplen as i32);
            break;
        }
        if layout == id.t_layout.get() {
            panic(format_args!("cannot load keymap"));
        }
        match wsmux_sc {
            Some(m) if wsmux_get_layout(m) != KB_NONE => {
                printf(format_args!(
                    "\n{}: cannot load keymap, falling back to default\n{}",
                    sc.xname(),
                    sc.xname()
                ));
                layout = wsmux_get_layout(m);
            }
            _ => panic(format_args!("cannot load keymap")),
        }
    }
    wskbd_update_layout(id, layout);

    // set default bell and key repeat data
    // SAFETY: the defaults change under the kernel lock (`WSKBDIO_SETDEFAULT*`), which
    // autoconfiguration holds.
    unsafe {
        sc.sc_bell_data.set(WSKBD_DEFAULT_BELL_DATA.read());
        sc.sc_keyrepeat_data
            .set(WSKBD_DEFAULT_KEYREPEAT_DATA.read());
    }

    if ap.console != 0 {
        crate::kassert!(WSKBD_CONSOLE_INITTED.load(Ordering::Relaxed));
        crate::kassert!(console_device().is_none());

        WSKBD_CONSOLE_DEVICE.store(ptr::from_ref(sc).cast_mut(), Ordering::Release);

        printf(format_args!(": console keyboard"));

        wsdisplay_set_console_kbd(Some(&sc.sc_base)); // sets sc_displaydv
        if let Some(d) = sc.displaydv() {
            printf(format_args!(", using {}", d.xname()));
        }
    }

    // Ignore mux for console; it always goes to the console mux.
    if let Some(m) = wsmux_sc
        && ap.console == 0
    {
        printf(format_args!(" mux {}\n", mux));
        if let Err(error) = wsmux_attach_sc(Some(m), &sc.sc_base) {
            printf(format_args!(
                "{}: attach error={}\n",
                sc.xname(),
                error as i32
            ));
        }

        // Try and set this encoding as the mux default if it hasn't any yet, and if this is
        // not a driver default layout (i.e. parent driver pretends to know better). Note
        // that wsmux_set_layout() rejects layouts with KB_DEFAULT set.
        if wsmux_get_layout(m) == KB_NONE {
            wsmux_set_layout(m, layout);
        }
    } else {
        printf(format_args!("\n"));
    }
}

/// `wskbd_cnattach`: attach the console keyboard with its console operations, cookie and
/// layouts.
pub fn wskbd_cnattach(
    consops: &'static WskbdConsops,
    conscookie: *mut c_void,
    mapdata: &'static WskbdMapdata,
) {
    crate::kassert!(!WSKBD_CONSOLE_INITTED.load(Ordering::Relaxed));

    WSKBD_CONSOLE_DATA.t_keydesc.set(mapdata.keydesc);
    WSKBD_CONSOLE_DATA.t_layout.set(mapdata.layout());
    wskbd_update_layout(&WSKBD_CONSOLE_DATA, mapdata.layout());

    WSKBD_CONSOLE_DATA.t_consops.set(Some(consops));
    WSKBD_CONSOLE_DATA.t_consaccesscookie.set(conscookie);

    wsdisplay_set_cons_kbd(wskbd_cngetc, wskbd_cnpollc, Some(wskbd_cnbell));

    WSKBD_CONSOLE_INITTED.store(true, Ordering::Release);
}

/// `wskbd_cndetach`: detach the console keyboard.
pub fn wskbd_cndetach() {
    crate::kassert!(WSKBD_CONSOLE_INITTED.load(Ordering::Relaxed));

    WSKBD_CONSOLE_DATA.t_keydesc.set(&[]);
    WSKBD_CONSOLE_DATA.t_layout.set(KB_NONE);

    WSKBD_CONSOLE_DATA.t_consops.set(None);
    WSKBD_CONSOLE_DATA.t_consaccesscookie.set(ptr::null_mut());

    wsdisplay_unset_cons_kbd();

    WSKBD_CONSOLE_DEVICE.store(ptr::null_mut(), Ordering::Release);
    WSKBD_CONSOLE_INITTED.store(false, Ordering::Release);
}

/// `wskbd_repeat`: the key repeat timeout: deliver the held key again.
fn wskbd_repeat(v: *mut c_void) {
    // SAFETY: `wskbd_attach` set the timeout's argument to the softc, and `wskbd_detach`
    // deletes the timeout first.
    let sc = unsafe { &*v.cast::<WskbdSoftc>() };
    let s = spltty();

    if sc.sc_repeating.get() == 0 {
        // race condition: a "key up" event came in when wskbd_repeat() was already called
        // but not yet spltty()'d
        splx(s);
        return;
    }
    if sc.sc_translating.get() != 0 {
        // deliver keys
        if let Some(d) = sc.displaydv() {
            let id = sc.id();
            let syms = id.t_symbols.get();
            let n = (sc.sc_repeating.get() as usize).min(MAXKEYSYMSPERKEY);
            wsdisplay_kbdinput(d, id.t_layout.get(), &syms[..n]);
        }
    } else {
        // queue event
        wskbd_deliver_event(sc, sc.sc_repeat_type.get(), sc.sc_repeat_value.get());
    }
    let deln = sc.sc_keyrepeat_data.get().delN;
    if deln != 0 {
        timeout_add_msec(&sc.sc_repeat_ch, u64::from(deln));
    }
    splx(s);
}

/// `wskbd_activate`.
pub fn wskbd_activate(self_: &Device, act: i32) -> Result<(), Errno> {
    let sc = sc_of(self_);

    if act == DVACT_DEACTIVATE {
        sc.sc_dying.set(1);
    }
    Ok(())
}

/// `wskbd_detach`: detach a keyboard. To keep track of users of the softc we keep a
/// reference count that's incremented while inside, e.g., read. If the keyboard is active
/// and the reference count is > 0 (0 is the normal state) we post an event and then wait for
/// the process that had the reference to wake us up again. Then we blow away the vnode and
/// return (which will deallocate the softc).
pub fn wskbd_detach(self_: &Device, _flags: i32) -> Result<(), Errno> {
    let sc = sc_of(self_);

    // Tell parent mux we're leaving.
    if sc.sc_base.me_parent.get().is_some() {
        wsmux_detach_sc(&sc.sc_base);
    }

    sc.stop_repeat();

    if sc.sc_isconsole.get() != 0 {
        crate::kassert!(console_device().is_some_and(|c| ptr::eq(c, sc)));
        wskbd_cndetach();
    }

    if let Some(evar) = sc.sc_base.evp() {
        sc.sc_refcnt.set(sc.sc_refcnt.get() - 1);
        if sc.sc_refcnt.get() >= 0 {
            // Wake everyone by generating a dummy event.
            mtx_enter(&evar.ws_mtx);
            let mut put = evar.ws_put.get() + 1;
            if put >= WSEVENT_QSIZE {
                put = 0;
            }
            evar.ws_put.set(put);
            mtx_leave(&evar.ws_mtx);
            wsevent_wakeup(evar);
            // Wait for processes to go away.
            if tsleep_nsec(ptr::from_ref(sc), PZERO, "wskdet", sec_to_nsec(60)).is_err() {
                printf(format_args!("wskbd_detach: {} didn't detach\n", sc.xname()));
            }
        }
    }

    if let Some(map) = NonNull::new(sc.sc_map.get()) {
        free(
            map.cast(),
            M_DEVBUF,
            sc.sc_maplen.get() as usize * size_of::<WsconsKeymap>(),
        );
        sc.sc_map.set(ptr::null_mut());
    }

    // locate the major number
    let n = nchrdev();
    let maj = (0..n)
        .find(|&maj| ptr::fn_addr_eq(cdevsw(maj).d_open, wskbdopen as DevTypeOpen))
        .unwrap_or(n);

    // Nuke the vnodes for any open instances.
    let mn = self_.dv_unit.get() as u32;
    vdevgone(maj, mn, mn, VCHR);

    Ok(())
}

/// `wskbd_input`: a key event from the keyboard driver (`WSCONS_EVENT_KEY_UP`,
/// `WSCONS_EVENT_KEY_DOWN` or `WSCONS_EVENT_ALL_KEYS_UP`, and the key code).
pub fn wskbd_input(dev: &Device, type_: u32, value: i32) {
    let sc = sc_of(dev);

    sc.stop_repeat();

    // If /dev/wskbdN is not connected in event mode translate and send upstream.
    if sc.sc_translating.get() != 0 {
        if HAVE_BURNER_SUPPORT
            && type_ == WSCONS_EVENT_KEY_DOWN
            && let Some(d) = sc.displaydv()
        {
            wsdisplay_burn(ptr::from_ref(d).cast_mut().cast(), WSDISPLAY_BURN_KBD);
        }
        let id = sc.id();
        let num = wskbd_translate(id, type_, value);
        if num > 0 {
            if let Some(d) = sc.displaydv() {
                let syms = id.t_symbols.get();
                // XXX - Shift_R+PGUP(release) emits PrtSc
                if HAVE_SCROLLBACK_SUPPORT && syms[0] != KS_Print_Screen {
                    wsscrollback(ptr::from_ref(d).cast_mut().cast(), WSDISPLAY_SCROLL_RESET);
                }
                wsdisplay_kbdinput(d, id.t_layout.get(), &syms[..num as usize]);
            }

            let del1 = sc.sc_keyrepeat_data.get().del1;
            if del1 != 0 {
                sc.sc_repeating.set(num);
                timeout_add_msec(&sc.sc_repeat_ch, u64::from(del1));
            }
        }
        return;
    }

    wskbd_deliver_event(sc, type_, value);

    // Repeat key presses if enabled.
    let del1 = sc.sc_keyrepeat_data.get().del1;
    if type_ == WSCONS_EVENT_KEY_DOWN && del1 != 0 {
        sc.sc_repeat_type.set(type_);
        sc.sc_repeat_value.set(value);
        sc.sc_repeating.set(1);
        timeout_add_msec(&sc.sc_repeat_ch, u64::from(del1));
    }
}

/// `wskbd_deliver_event`: keyboard is generating events. Turn this keystroke into an event
/// and put it in the queue. If the queue is full, the keystroke is lost (sorry!).
pub fn wskbd_deliver_event(sc: &WskbdSoftc, type_: u32, value: i32) {
    let Some(evar) = sc.sc_base.evp() else {
        return;
    };

    if cfg!(feature = "diagnostic") && evar.ws_q.get().is_null() {
        printf(format_args!("wskbd_input: evar->q=NULL\n"));
        return;
    }

    mtx_enter(&evar.ws_mtx);
    let at = evar.ws_put.get();
    let put = (at + 1) % WSEVENT_QSIZE;
    if put == evar.ws_get.get() {
        mtx_leave(&evar.ws_mtx);
        log(
            LOG_WARNING,
            format_args!("{}: event queue overflow\n", sc.xname()),
        );
        return;
    }
    let ev = WsconsEvent {
        type_,
        value,
        time: nanotime(),
    };
    // SAFETY: the queue is open (`me_evp`), `at < WSEVENT_QSIZE`, `ws_mtx` held.
    unsafe { evar.q_write(at, ev) };
    evar.ws_put.set(put);
    mtx_leave(&evar.ws_mtx);
    wsevent_wakeup(evar);
}

/// `wskbd_rawinput` (`WSDISPLAY_COMPAT_RAWKBD`): raw XT scancodes, for the display.
pub fn wskbd_rawinput(dev: &Device, buf: &[u8]) {
    let sc = sc_of(dev);

    if let Some(d) = sc.displaydv() {
        wsdisplay_rawkbdinput(d, buf);
    }
}

/// `wskbd_enable`: turn the keyboard on or off, unless it is a display's input (then it
/// stays on).
pub fn wskbd_enable(sc: &WskbdSoftc, on: i32) -> Result<(), Errno> {
    if sc.displaydv().is_some() {
        return Ok(());
    }

    // Always cancel auto repeat when fiddling with the kbd.
    sc.stop_repeat();

    errno_result((sc.accessops().enable)(sc.sc_accesscookie.get(), on))
}

/// `wskbd_mux_open`.
pub fn wskbd_mux_open(me: &Wsevsrc, evp: &Wseventvar) -> Result<(), Errno> {
    // SAFETY: `wskbd_srcops` is only the methods of keyboards.
    let sc = unsafe { &*ptr::from_ref(me).cast::<WskbdSoftc>() };

    if sc.sc_dying.get() != 0 {
        return Err(Errno::EIO);
    }

    wskbd_do_open(sc, evp)
}

/// `wskbdopen`.
pub fn wskbdopen(dev: Dev, flags: i32, _mode: i32, _p: &Proc) -> Result<(), Errno> {
    let unit = minor(dev) as i32;
    // make sure it was attached
    let sc = wskbd_sc(unit).ok_or(Errno::ENXIO)?;

    if sc.sc_dying.get() != 0 {
        return Err(Errno::EIO);
    }

    if flags & (FREAD | FWRITE) == FWRITE {
        // Not opening for read, only ioctl is available.
        return Ok(());
    }

    if sc.sc_base.me_parent.get().is_some() {
        // Grab the keyboard out of the greedy hands of the mux.
        wsmux_detach_sc(&sc.sc_base);
    }

    if sc.sc_base.me_evp.get().is_some() {
        return Err(Errno::EBUSY);
    }

    let evar = &sc.sc_base.me_evar;
    if wsevent_init(evar).is_err() {
        return Err(Errno::EBUSY);
    }

    let error = wskbd_do_open(sc, evar);
    if error.is_err() {
        wsevent_fini(evar);
    }
    error
}

/// `wskbd_do_open`.
pub fn wskbd_do_open(sc: &WskbdSoftc, evp: &Wseventvar) -> Result<(), Errno> {
    // The device could already be attached to a mux.
    if sc.sc_base.me_evp.get().is_some() {
        return Err(Errno::EBUSY);
    }

    sc.sc_base.me_evp.set(Some(NonNull::from(evp)));
    sc.sc_translating.set(0);

    let error = wskbd_enable(sc, 1);
    if error.is_err() {
        sc.sc_base.me_evp.set(None);
    }
    error
}

/// `wskbdclose`.
pub fn wskbdclose(dev: Dev, flags: i32, _mode: i32, _p: Option<&Proc>) -> Result<(), Errno> {
    let sc = wskbd_sc(minor(dev) as i32).ok_or(Errno::ENXIO)?;
    let evar = sc.sc_base.me_evp.get();

    if flags & (FREAD | FWRITE) == FWRITE {
        // not open for read
        return Ok(());
    }

    sc.sc_base.me_evp.set(None);
    sc.sc_translating.set(1);
    let _ = wskbd_enable(sc, 0);
    if let Some(evar) = evar {
        // SAFETY: the keyboard's own `me_evar` (`wskbdopen` opened it), a member of the
        // softc.
        wsevent_fini(unsafe { evar.as_ref() });
    }

    if sc.sc_base.me_parent.get().is_none() {
        let mux = wskbddevcf_mux(sc.sc_base.me_dv.cfdata()) as i32;
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

/// `wskbd_mux_close`.
pub fn wskbd_mux_close(me: &Wsevsrc) -> Result<(), Errno> {
    // SAFETY: `wskbd_srcops` is only the methods of keyboards.
    let sc = unsafe { &*ptr::from_ref(me).cast::<WskbdSoftc>() };

    let _ = wskbd_enable(sc, 0);
    sc.sc_translating.set(1);
    sc.sc_base.me_evp.set(None);

    Ok(())
}

/// `wskbdread`.
pub fn wskbdread(dev: Dev, uio: &mut Uio<'_>, flags: i32) -> Result<(), Errno> {
    let sc = wskbd_sc(minor(dev) as i32).ok_or(Errno::ENXIO)?;

    if sc.sc_dying.get() != 0 {
        return Err(Errno::EIO);
    }

    if cfg!(feature = "diagnostic") && sc.sc_base.me_evp.get().is_none() {
        printf(format_args!("wskbdread: evp == NULL\n"));
        return Err(Errno::EINVAL);
    }

    sc.sc_refcnt.set(sc.sc_refcnt.get() + 1);
    let mut error = wsevent_read(&sc.sc_base.me_evar, uio, flags);
    sc.sc_refcnt.set(sc.sc_refcnt.get() - 1);
    if sc.sc_refcnt.get() < 0 {
        wakeup(ptr::from_ref(sc));
        error = Err(Errno::EIO);
    }
    error
}

/// `wskbdioctl`.
pub fn wskbdioctl(dev: Dev, cmd: u64, data: &mut [u8], flag: i32, p: &Proc) -> Result<(), Errno> {
    let sc = wskbd_sc(minor(dev) as i32).ok_or(Errno::ENXIO)?;

    sc.sc_refcnt.set(sc.sc_refcnt.get() + 1);
    let error = wskbd_do_ioctl_sc(sc, cmd, data, flag, Some(p), false);
    sc.sc_refcnt.set(sc.sc_refcnt.get() - 1);
    if sc.sc_refcnt.get() < 0 {
        wakeup(ptr::from_ref(sc));
    }
    error
}

/// `wskbd_do_ioctl`: a wrapper around the ioctl() workhorse to make reference counting easy.
pub fn wskbd_do_ioctl(
    dv: &Device,
    cmd: u64,
    data: &mut [u8],
    flag: i32,
    p: Option<&Proc>,
) -> Result<(), Errno> {
    let sc = sc_of(dv);

    sc.sc_refcnt.set(sc.sc_refcnt.get() + 1);
    let error = wskbd_do_ioctl_sc(sc, cmd, data, flag, p, true);
    sc.sc_refcnt.set(sc.sc_refcnt.get() - 1);
    if sc.sc_refcnt.get() < 0 {
        wakeup(ptr::from_ref(sc));
    }
    error
}

/// `wskbd_do_ioctl_sc`: the generic ioctls of the wskbd interface, then the `WSKBDIO`
/// ones; `ENOTTY` for an ioctl nobody recognises.
pub fn wskbd_do_ioctl_sc(
    sc: &WskbdSoftc,
    cmd: u64,
    data: &mut [u8],
    flag: i32,
    p: Option<&Proc>,
    evsrc: bool,
) -> Result<(), Errno> {
    // Try the generic ioctls that the wskbd interface supports.
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

        _ => {}
    }

    // Try the keyboard driver for WSKBDIO ioctls. It returns -1 if it didn't recognize the
    // request.
    match wskbd_displayioctl_sc(sc, cmd, data, flag, p, evsrc)? {
        true => Ok(()),
        false => Err(Errno::ENOTTY),
    }
}

/// `wskbd_displayioctl`: WSKBDIO ioctls, handled in both emulation mode and in ``raw''
/// mode. Some of these have no real effect in raw mode, however.
pub fn wskbd_displayioctl(
    dv: &Device,
    cmd: u64,
    data: &mut [u8],
    flag: i32,
    p: Option<&Proc>,
) -> Result<bool, Errno> {
    let sc = sc_of(dv);

    wskbd_displayioctl_sc(sc, cmd, data, flag, p, true)
}

/// `SETBELL(dstp, srcp, dfltp)`.
fn setbell(src: &WskbdBellData, dflt: &WskbdBellData) -> WskbdBellData {
    WskbdBellData {
        pitch: if src.which & WSKBD_BELL_DOPITCH != 0 {
            src.pitch
        } else {
            dflt.pitch
        },
        period: if src.which & WSKBD_BELL_DOPERIOD != 0 {
            src.period
        } else {
            dflt.period
        },
        volume: if src.which & WSKBD_BELL_DOVOLUME != 0 {
            src.volume
        } else {
            dflt.volume
        },
        which: WSKBD_BELL_DOALL,
    }
}

/// `SETKEYREPEAT(dstp, srcp, dfltp)`.
fn setkeyrepeat(src: &WskbdKeyrepeatData, dflt: &WskbdKeyrepeatData) -> WskbdKeyrepeatData {
    WskbdKeyrepeatData {
        del1: if src.which & WSKBD_KEYREPEAT_DODEL1 != 0 {
            src.del1
        } else {
            dflt.del1
        },
        delN: if src.which & WSKBD_KEYREPEAT_DODELN != 0 {
            src.delN
        } else {
            dflt.delN
        },
        which: WSKBD_KEYREPEAT_DOALL,
    }
}

/// `suser(p)`, with no thread (a mux's NULL) refused.
fn suser_opt(p: Option<&Proc>) -> Result<(), Errno> {
    match p {
        Some(p) => suser(p),
        None => Err(Errno::EPERM),
    }
}

/// `wskbd_displayioctl_sc`: the WSKBDIO ioctls, then the keyboard driver's; `Ok(false)`
/// (the C's -1) if nobody recognised the request.
pub fn wskbd_displayioctl_sc(
    sc: &WskbdSoftc,
    cmd: u64,
    data: &mut [u8],
    flag: i32,
    p: Option<&Proc>,
    evsrc: bool,
) -> Result<bool, Errno> {
    if matches!(
        cmd,
        WSKBDIO_BELL
            | WSKBDIO_COMPLEXBELL
            | WSKBDIO_SETBELL
            | WSKBDIO_SETKEYREPEAT
            | WSKBDIO_SETDEFAULTKEYREPEAT
            | WSKBDIO_SETMAP
            | WSKBDIO_SETENCODING
            | WSKBDIO_SETBACKLIGHT
    ) && flag & FWRITE == 0
    {
        return Err(Errno::EACCES);
    }

    let id = sc.id();
    let ioctl = sc.accessops().ioctl;
    let cookie = sc.sc_accesscookie.get();

    match cmd {
        WSKBDIO_BELL => {
            let mut buf = [0u8; size_of::<WskbdBellData>()];
            ioctl_ret(&mut buf, &sc.sc_bell_data.get());
            let r = ioctl(cookie, WSKBDIO_COMPLEXBELL, &mut buf, flag, p);
            sc.sc_bell_data.set(ioctl_arg(&buf));
            return r;
        }

        WSKBDIO_COMPLEXBELL => {
            let ubd: WskbdBellData = ioctl_arg(data);
            let ubd = setbell(&ubd, &sc.sc_bell_data.get());
            ioctl_ret(data, &ubd);
            return ioctl(cookie, WSKBDIO_COMPLEXBELL, data, flag, p);
        }

        WSKBDIO_SETBELL => {
            let ubd: WskbdBellData = ioctl_arg(data);
            sc.sc_bell_data.set(setbell(&ubd, &sc.sc_bell_data.get()));
            return Ok(true);
        }

        WSKBDIO_GETBELL => {
            let kbd = sc.sc_bell_data.get();
            ioctl_ret(data, &setbell(&kbd, &kbd));
            return Ok(true);
        }

        WSKBDIO_SETDEFAULTBELL => {
            suser_opt(p)?;
            let ubd: WskbdBellData = ioctl_arg(data);
            // SAFETY: the defaults change under the kernel lock, which ioctls hold.
            let kbd = unsafe { WSKBD_DEFAULT_BELL_DATA.get_mut() };
            *kbd = setbell(&ubd, kbd);
            return Ok(true);
        }

        WSKBDIO_GETDEFAULTBELL => {
            // SAFETY: as for `WSKBDIO_SETDEFAULTBELL`.
            let kbd = unsafe { WSKBD_DEFAULT_BELL_DATA.read() };
            ioctl_ret(data, &setbell(&kbd, &kbd));
            return Ok(true);
        }

        WSKBDIO_SETKEYREPEAT => {
            let ukd: WskbdKeyrepeatData = ioctl_arg(data);
            sc.sc_keyrepeat_data
                .set(setkeyrepeat(&ukd, &sc.sc_keyrepeat_data.get()));
            return Ok(true);
        }

        WSKBDIO_GETKEYREPEAT => {
            let kkd = sc.sc_keyrepeat_data.get();
            ioctl_ret(data, &setkeyrepeat(&kkd, &kkd));
            return Ok(true);
        }

        WSKBDIO_SETDEFAULTKEYREPEAT => {
            suser_opt(p)?;
            let ukd: WskbdKeyrepeatData = ioctl_arg(data);
            // SAFETY: as for `WSKBDIO_SETDEFAULTBELL`.
            let kkd = unsafe { WSKBD_DEFAULT_KEYREPEAT_DATA.get_mut() };
            *kkd = setkeyrepeat(&ukd, kkd);
            return Ok(true);
        }

        WSKBDIO_GETDEFAULTKEYREPEAT => {
            // SAFETY: as for `WSKBDIO_SETDEFAULTBELL`.
            let kkd = unsafe { WSKBD_DEFAULT_KEYREPEAT_DATA.read() };
            ioctl_ret(data, &setkeyrepeat(&kkd, &kkd));
            return Ok(true);
        }

        WSKBDIO_SETMAP => {
            let umd: WskbdMapData = ioctl_arg(data);
            if umd.maplen > WSKBDIO_MAXMAPLEN {
                return Err(Errno::EINVAL);
            }

            let maplen = umd.maplen as usize;
            let len = maplen * size_of::<WsconsKeymap>();
            let buf = mallocarray(maplen, size_of::<WsconsKeymap>(), M_TEMP, M_WAITOK)
                .ok_or(Errno::ENOMEM)?;
            // SAFETY: a fresh allocation of `len` bytes, ours until freed below.
            let kbuf = unsafe { core::slice::from_raw_parts_mut(buf.as_ptr(), len) };

            let error = copyin(umd.map, kbuf);
            if error.is_ok() {
                match wskbd_init_keymap(maplen) {
                    Ok(map) => {
                        // SAFETY: the new map has `maplen` entries, `len` bytes, and the
                        // buffer holds `len` bytes of them (any bytes are a keymap entry).
                        unsafe {
                            ptr::copy_nonoverlapping(buf.as_ptr(), map.as_ptr().cast::<u8>(), len)
                        };
                        wskbd_set_keymap(sc, map.as_ptr(), maplen as i32);
                        // drop the variant bits handled by the map
                        let enc = KB_USER | (kb_variant(id.t_layout.get()) & KB_HANDLEDBYWSKBD);
                        wskbd_update_layout(id, enc);
                    }
                    Err(e) => {
                        free(buf, M_TEMP, len);
                        return Err(e);
                    }
                }
            }
            free(buf, M_TEMP, len);
            return error.map(|()| true);
        }

        WSKBDIO_GETMAP => {
            let mut umd: WskbdMapData = ioctl_arg(data);
            if umd.maplen > sc.sc_maplen.get() as u32 {
                umd.maplen = sc.sc_maplen.get() as u32;
            }
            ioctl_ret(data, &umd);
            let len = umd.maplen as usize * size_of::<WsconsKeymap>();
            // SAFETY: `sc_map` has `sc_maplen` entries and `umd.maplen` is at most that.
            let kmap = unsafe { core::slice::from_raw_parts(sc.sc_map.get().cast::<u8>(), len) };
            return copyout(kmap, umd.map).map(|()| true);
        }

        WSKBDIO_GETENCODING => {
            // Do not advertise encoding to the parent mux.
            if evsrc && id.t_layout.get() & KB_NOENCODING != 0 {
                return Err(Errno::ENOTTY);
            }
            ioctl_ret(data, &(id.t_layout.get() & !KB_DEFAULT));
            return Ok(true);
        }

        WSKBDIO_SETENCODING => {
            let enc: KbdT = ioctl_arg(data);
            if kb_encoding(enc) == KB_USER {
                // user map must already be loaded
                if kb_encoding(id.t_layout.get()) != KB_USER {
                    return Err(Errno::EINVAL);
                }
                // map variants make no sense
                if kb_variant(enc) & !KB_HANDLEDBYWSKBD != 0 {
                    return Err(Errno::EINVAL);
                }
            } else if id.t_layout.get() & KB_NOENCODING != 0 {
                return Ok(true);
            } else {
                let (map, maplen) = wskbd_load_keymap(&id.keymap(), enc)?;
                wskbd_set_keymap(sc, map.as_ptr(), maplen as i32);
            }
            wskbd_update_layout(id, enc);
            // Update mux default layout
            if let Some(parent) = sc.sc_base.parent() {
                wsmux_set_layout(parent, enc);
            }
            return Ok(true);
        }

        WSKBDIO_GETENCODINGS => {
            let mut ued: WskbdEncodingData = ioctl_arg(data);
            let keydesc = id.t_keydesc.get();
            let count = keydesc.iter().take_while(|k| k.name != 0).count() as i32;
            if ued.nencodings > count {
                ued.nencodings = count;
            }
            ioctl_ret(data, &ued);
            let n = ued.nencodings.max(0) as usize;
            for (i, kd) in keydesc.iter().take(n).enumerate() {
                copyout(
                    &kd.name.to_ne_bytes(),
                    ued.encodings + i * size_of::<KbdT>(),
                )?;
            }
            return Ok(true);
        }

        WSKBDIO_GETBACKLIGHT => {
            // SAFETY: the hooks are installed at attach, under the kernel lock.
            if let Some(get) = unsafe { WSKBD_GET_BACKLIGHT.read() } {
                let mut bl: WskbdBacklight = ioctl_arg(data);
                let r = get(&mut bl);
                ioctl_ret(data, &bl);
                return r.map(|()| true);
            }
        }

        WSKBDIO_SETBACKLIGHT => {
            // SAFETY: as for `WSKBDIO_GETBACKLIGHT`.
            if let Some(set) = unsafe { WSKBD_SET_BACKLIGHT.read() } {
                let mut bl: WskbdBacklight = ioctl_arg(data);
                return set(&mut bl).map(|()| true);
            }
        }

        _ => {}
    }

    // Try the keyboard driver for WSKBDIO ioctls. It returns -1 if it didn't recognize the
    // request, and in turn we return -1 if we didn't recognize the request.
    let error = ioctl(cookie, cmd, data, flag, p);
    if cfg!(feature = "wsdisplay_compat_rawkbd")
        && error == Ok(true)
        && cmd == WSKBDIO_SETMODE
        && ioctl_arg::<i32>(data) == WSKBD_RAW
    {
        let s = spltty();
        id.t_modifiers.set(
            id.t_modifiers.get()
                & !(MOD_SHIFT_L
                    | MOD_SHIFT_R
                    | MOD_CONTROL_L
                    | MOD_CONTROL_R
                    | MOD_META_L
                    | MOD_META_R
                    | MOD_COMMAND
                    | MOD_COMMAND1
                    | MOD_COMMAND2),
        );
        sc.stop_repeat();
        splx(s);
    }
    error
}

/// `wskbdkqfilter`.
pub fn wskbdkqfilter(dev: Dev, kn: &Knote) -> Result<(), Errno> {
    let sc = wskbd_sc(minor(dev) as i32).ok_or(Errno::ENXIO)?;

    let evar = sc.sc_base.evp().ok_or(Errno::ENXIO)?;
    wsevent_kqfilter(evar, kn)
}

/// `wskbd_pickfree`: the index of a keyboard not yet bound to a display, -1 when none is.
pub fn wskbd_pickfree() -> i32 {
    for i in 0..WSKBD_CD.cd_ndevs.get() {
        let Some(sc) = wskbd_sc(i) else {
            continue;
        };
        if sc.displaydv().is_none() {
            return i;
        }
    }
    -1
}

/// `wskbd_set_console_display`: bind the console keyboard to the console display and join
/// it to the display's mux `me`; the keyboard's event source, `None` without a console
/// keyboard.
pub fn wskbd_set_console_display(
    displaydv: &Device,
    me: Option<&'static Wsevsrc>,
) -> Option<&'static Wsevsrc> {
    let sc = console_device()?;
    sc.sc_base.me_dispdv.set(Some(NonNull::from(displaydv)));
    // SAFETY: wsdisplay's `sc_input` is a mux (`wsmux_getmux` or `wsmux_create`).
    let mux = me.map(|m| unsafe { WsmuxSoftc::of_evsrc(m) });
    let _ = wsmux_attach_sc(mux, &sc.sc_base);
    Some(&sc.sc_base)
}

/// `wskbd_set_display`: bind keyboard `dv` to display `displaydv` (unbind for `None`).
pub fn wskbd_set_display(dv: &Device, displaydv: Option<&Device>) -> Result<(), Errno> {
    let sc = sc_of(dv);

    if sc.sc_isconsole.get() != 0 {
        return Err(Errno::EBUSY);
    }

    if displaydv.is_some() {
        if sc.displaydv().is_some() {
            return Err(Errno::EBUSY);
        }
    } else if sc.displaydv().is_none() {
        return Err(Errno::ENXIO);
    }

    let odisplaydv = sc.sc_base.me_dispdv.get();
    sc.sc_base.me_dispdv.set(None);
    let error = wskbd_enable(sc, i32::from(displaydv.is_some()));
    sc.sc_base.me_dispdv.set(displaydv.map(NonNull::from));
    if let Err(error) = error {
        sc.sc_base.me_dispdv.set(odisplaydv);
        return Err(error);
    }

    match (displaydv, odisplaydv) {
        (Some(d), _) => {
            printf(format_args!(
                "{}: connecting to {}\n",
                sc.xname(),
                d.xname()
            ));
        }
        (None, Some(o)) => {
            // SAFETY: the display we were connected to; displays are never freed while a
            // keyboard points at them.
            let o = unsafe { o.as_ref() };
            printf(format_args!(
                "{}: disconnecting from {}\n",
                sc.xname(),
                o.xname()
            ));
        }
        (None, None) => {}
    }

    Ok(())
}

/// `wskbd_add_mux`: add keyboard `unit` to mux `muxsc` (`WSMUXIO_ADD_DEVICE`).
pub fn wskbd_add_mux(unit: i32, muxsc: &WsmuxSoftc) -> Result<(), Errno> {
    let sc = wskbd_sc(unit).ok_or(Errno::ENXIO)?;

    if sc.sc_base.me_parent.get().is_some() || sc.sc_base.me_evp.get().is_some() {
        return Err(Errno::EBUSY);
    }

    wsmux_attach_sc(Some(muxsc), &sc.sc_base)
}

/// `wskbd_cngetc`: console interface: the next ASCII character typed, polling the console
/// keyboard; 0 without one, or while it is open in event mode.
pub fn wskbd_cngetc(_dev: Dev) -> i32 {
    if !WSKBD_CONSOLE_INITTED.load(Ordering::Acquire) {
        return 0;
    }

    if let Some(c) = console_device()
        && c.sc_translating.get() == 0
    {
        return 0;
    }

    loop {
        let num = CNGETC_NUM.fetch_sub(1, Ordering::Relaxed);
        if num > 0 {
            let pos = CNGETC_POS.fetch_add(1, Ordering::Relaxed);
            let ks = WSKBD_CONSOLE_DATA.t_symbols.get()[(pos as usize).min(MAXKEYSYMSPERKEY - 1)];
            if ks_group(u32::from(ks)) == KS_GROUP_Ascii {
                return ks_value(u32::from(ks)) as i32;
            }
        } else {
            let Some(consops) = WSKBD_CONSOLE_DATA.t_consops.get() else {
                return 0;
            };
            let mut type_ = 0;
            let mut data = 0;
            (consops.getc)(
                WSKBD_CONSOLE_DATA.t_consaccesscookie.get(),
                &mut type_,
                &mut data,
            );
            let n = wskbd_translate(&WSKBD_CONSOLE_DATA, type_, data);
            CNGETC_NUM.store(n, Ordering::Relaxed);
            CNGETC_POS.store(0, Ordering::Relaxed);
        }
    }
}

/// `wskbd_cnpollc`: switch the console keyboard's polling on or off.
pub fn wskbd_cnpollc(_dev: Dev, poll: i32) {
    if !WSKBD_CONSOLE_INITTED.load(Ordering::Acquire) {
        return;
    }

    if let Some(c) = console_device()
        && c.sc_translating.get() == 0
    {
        return;
    }

    if let Some(consops) = WSKBD_CONSOLE_DATA.t_consops.get() {
        (consops.pollc)(WSKBD_CONSOLE_DATA.t_consaccesscookie.get(), poll);
    }
}

/// `wskbd_cnbell`: ring the console keyboard's bell.
pub fn wskbd_cnbell(_dev: Dev, pitch: u32, period: u32, volume: u32) {
    if !WSKBD_CONSOLE_INITTED.load(Ordering::Acquire) {
        return;
    }

    if let Some(consops) = WSKBD_CONSOLE_DATA.t_consops.get() {
        (consops.bell)(
            WSKBD_CONSOLE_DATA.t_consaccesscookie.get(),
            pitch,
            period,
            volume,
        );
    }
}

/// `update_leds`: the LEDs the lock and compose modifiers light.
pub fn update_leds(id: &WskbdInternal) {
    let mut new_state = 0;
    if id.t_modifiers.get() & (MOD_SHIFTLOCK | MOD_CAPSLOCK) != 0 {
        new_state |= WSKBD_LED_CAPS;
    }
    if id.t_modifiers.get() & MOD_NUMLOCK != 0 {
        new_state |= WSKBD_LED_NUM;
    }
    if id.t_modifiers.get() & MOD_COMPOSE != 0 {
        new_state |= WSKBD_LED_COMPOSE;
    }
    if id.t_modifiers.get() & MOD_HOLDSCREEN != 0 {
        new_state |= WSKBD_LED_SCROLL;
    }

    if let Some(sc) = id.sc()
        && new_state != sc.sc_ledstate.get()
    {
        (sc.accessops().set_leds)(sc.sc_accesscookie.get(), new_state);
        sc.sc_ledstate.set(new_state);
    }
}

/// `update_modifier`: a modifier key pressed or released (`toggle`: a lock, which a press
/// flips).
pub fn update_modifier(id: &WskbdInternal, type_: u32, toggle: i32, mask: i32) {
    if toggle != 0 {
        if type_ == WSCONS_EVENT_KEY_DOWN {
            id.t_modifiers.set(id.t_modifiers.get() ^ mask);
        }
    } else if type_ == WSCONS_EVENT_KEY_DOWN {
        id.t_modifiers.set(id.t_modifiers.get() | mask);
    } else {
        id.t_modifiers.set(id.t_modifiers.get() & !mask);
    }
    if mask & MOD_ANYLED != 0 {
        update_leds(id);
    }
}

/// `change_displayparam`: step a display parameter (backlight, contrast) up or down,
/// clamping or wrapping around at its limits.
pub fn change_displayparam(sc: &WskbdSoftc, param: i32, updown: i32, wraparound: i32) {
    let Some(d) = sc.displaydv() else {
        return;
    };
    let mut dp = WsdisplayParam {
        param,
        ..WsdisplayParam::default()
    };
    let res = wsdisplay_param(d, WSDISPLAYIO_GETPARAM, &mut dp);

    if res == Err(Errno::EINVAL) {
        return; // no such parameter
    }

    dp.curval += updown;
    if dp.max < dp.curval {
        dp.curval = if wraparound != 0 { dp.min } else { dp.max };
    } else if dp.curval < dp.min {
        dp.curval = if wraparound != 0 { dp.max } else { dp.min };
    }
    let _ = wsdisplay_param(d, WSDISPLAYIO_SETPARAM, &mut dp);
}

/// `internal_command`: the command keysym `ksym` of a key (`ksym2` its plain keysym, for
/// `KS_Cmd`): `true` when it was a command and is consumed.
#[allow(non_upper_case_globals)] // the keysym names (`KS_Cmd_Screen0`), verbatim, as patterns
pub fn internal_command(
    sc: &'static WskbdSoftc,
    type_: &mut u32,
    ksym: KeysymT,
    ksym2: KeysymT,
) -> bool {
    let id = sc.id();
    let mut ksym = ksym;

    match ksym {
        KS_Cmd => {
            update_modifier(id, *type_, 0, MOD_COMMAND);
            ksym = ksym2;
        }

        KS_Cmd1 => update_modifier(id, *type_, 0, MOD_COMMAND1),

        KS_Cmd2 => update_modifier(id, *type_, 0, MOD_COMMAND2),

        _ => {}
    }

    if *type_ != WSCONS_EVENT_KEY_DOWN {
        return false;
    }

    // SUSPEND
    if ksym == KS_Cmd_Sleep {
        // request_sleep(SLEEP_SUSPEND)
        let _ = unported!("request_sleep (KS_Cmd_Sleep; kern/subr_suspend.c)");
        return true;
    }

    if HAVE_SCROLLBACK_SUPPORT {
        match ksym {
            KS_Cmd_ScrollBack if id.mod_oneset(MOD_ANYSHIFT) => {
                if let Some(d) = sc.displaydv() {
                    wsscrollback(
                        ptr::from_ref(d).cast_mut().cast(),
                        WSDISPLAY_SCROLL_BACKWARD,
                    );
                }
                return true;
            }

            KS_Cmd_ScrollFwd if id.mod_oneset(MOD_ANYSHIFT) => {
                if let Some(d) = sc.displaydv() {
                    wsscrollback(ptr::from_ref(d).cast_mut().cast(), WSDISPLAY_SCROLL_FORWARD);
                }
                return true;
            }

            _ => {}
        }
    }

    let backlight = match ksym {
        KS_Cmd_KbdBacklightUp => KBD_BACKLIGHT_UP,
        KS_Cmd_KbdBacklightDown => KBD_BACKLIGHT_DOWN,
        KS_Cmd_KbdBacklightToggle => KBD_BACKLIGHT_TOGGLE,
        _ => KBD_BACKLIGHT_NONE,
    };
    if backlight != KBD_BACKLIGHT_NONE {
        sc.sc_kbd_backlight_cmd.store(backlight, Ordering::Relaxed);
        task_add(SYSTQ, &sc.sc_kbd_backlight_task);
        return true;
    }

    match ksym {
        KS_Cmd_BrightnessUp => {
            sc.sc_brightness_steps.fetch_add(1, Ordering::Relaxed);
            task_add(SYSTQ, &sc.sc_brightness_task);
            return true;
        }
        KS_Cmd_BrightnessDown => {
            sc.sc_brightness_steps.fetch_sub(1, Ordering::Relaxed);
            task_add(SYSTQ, &sc.sc_brightness_task);
            return true;
        }
        KS_Cmd_BrightnessRotate => {
            wsdisplay_brightness_cycle(sc.displaydv());
            return true;
        }
        _ => {}
    }

    if !id.mod_oneset(MOD_COMMAND) && !id.mod_allset(MOD_COMMAND1 | MOD_COMMAND2) {
        return false;
    }

    // DDB
    if ksym == KS_Cmd_Debugger {
        wskbd_debugger(sc);
        // discard this key (ddb discarded command modifiers)
        *type_ = WSCONS_EVENT_KEY_UP;
        return true;
    }

    let Some(d) = sc.displaydv() else {
        return false;
    };

    match ksym {
        KS_Cmd_Screen0 | KS_Cmd_Screen1 | KS_Cmd_Screen2 | KS_Cmd_Screen3 | KS_Cmd_Screen4
        | KS_Cmd_Screen5 | KS_Cmd_Screen6 | KS_Cmd_Screen7 | KS_Cmd_Screen8 | KS_Cmd_Screen9
        | KS_Cmd_Screen10 | KS_Cmd_Screen11 => {
            let _ = wsdisplay_switch(d, i32::from(ksym - KS_Cmd_Screen0), 0);
            true
        }
        KS_Cmd_ResetEmul => {
            wsdisplay_reset(d, WSDISPLAY_RESETEMUL);
            true
        }
        KS_Cmd_ResetClose => {
            wsdisplay_reset(d, WSDISPLAY_RESETCLOSE);
            true
        }
        // #if defined(__i386__) || defined(__amd64__)
        KS_Cmd_KbdReset => {
            if let Some(kbd_reset) = crate::machine::cpu::kbd_reset() {
                match kbd_reset.load(Ordering::Relaxed) {
                    // DDB
                    2 => {
                        wskbd_debugger(sc);
                        // discard this key (ddb discarded command modifiers)
                        *type_ = WSCONS_EVENT_KEY_UP;
                    }
                    1 => {
                        kbd_reset.store(0, Ordering::Relaxed);
                        // SAFETY: NULL until `start_init` forks init(8), which never exits.
                        if let Some(init) = unsafe { INITPROCESS.load(Ordering::Acquire).as_ref() }
                        {
                            prsignal(init, SIGUSR1);
                        }
                    }
                    _ => {}
                }
            }
            true
        }
        KS_Cmd_BacklightOn | KS_Cmd_BacklightOff | KS_Cmd_BacklightToggle => {
            change_displayparam(
                sc,
                WSDISPLAYIO_PARAM_BACKLIGHT,
                if ksym == KS_Cmd_BacklightOff { -1 } else { 1 },
                i32::from(ksym == KS_Cmd_BacklightToggle),
            );
            true
        }
        KS_Cmd_ContrastUp | KS_Cmd_ContrastDown | KS_Cmd_ContrastRotate => {
            change_displayparam(
                sc,
                WSDISPLAYIO_PARAM_CONTRAST,
                if ksym == KS_Cmd_ContrastDown { -1 } else { 1 },
                i32::from(ksym == KS_Cmd_ContrastRotate),
            );
            true
        }
        _ => false,
    }
}

/// `wskbd_translate`: a key event through the keymap and the modifiers: the number of
/// keysyms it produced in `id.t_symbols` (0 for a release, a modifier, a command, or a
/// compose sequence not yet complete).
#[allow(non_upper_case_globals)] // the keysym names (`KS_Shift_L`), verbatim, as patterns
pub fn wskbd_translate(id: &WskbdInternal, type_: u32, value: i32) -> i32 {
    let sc = id.sc();
    let mut type_ = type_;
    let mut iscommand = false;

    if type_ == WSCONS_EVENT_ALL_KEYS_UP {
        if let Some(sc) = sc {
            sc.stop_repeat();
        }
        id.t_modifiers.set(
            id.t_modifiers.get()
                & !(MOD_SHIFT_L
                    | MOD_SHIFT_R
                    | MOD_CONTROL_L
                    | MOD_CONTROL_R
                    | MOD_META_L
                    | MOD_META_R
                    | MOD_MODESHIFT
                    | MOD_MODELOCK
                    | MOD_COMMAND
                    | MOD_COMMAND1
                    | MOD_COMMAND2),
        );
        return 0;
    }

    let kp = match sc {
        Some(sc) => {
            if value < 0 || value >= sc.sc_maplen.get() {
                return 0;
            }
            // SAFETY: `sc_map` has `sc_maplen` entries and `value` is below that.
            unsafe { *sc.sc_map.get().add(value as usize) }
        }
        None => {
            let mut kpbuf = WsconsKeymap::default();
            wskbd_get_mapentry(&id.keymap(), value, &mut kpbuf);
            kpbuf
        }
    };

    // if this key has a command, process it first
    if let Some(sc) = sc
        && kp.command != KS_voidSymbol
    {
        iscommand = internal_command(sc, &mut type_, kp.command, kp.group1[0]);
    }

    // Now update modifiers
    match kp.group1[0] {
        KS_Shift_L => update_modifier(id, type_, 0, MOD_SHIFT_L),
        KS_Shift_R => update_modifier(id, type_, 0, MOD_SHIFT_R),
        KS_Shift_Lock => update_modifier(id, type_, 1, MOD_SHIFTLOCK),
        KS_Caps_Lock => update_modifier(id, type_, 1, MOD_CAPSLOCK),
        KS_Control_L => update_modifier(id, type_, 0, MOD_CONTROL_L),
        KS_Control_R => update_modifier(id, type_, 0, MOD_CONTROL_R),
        KS_Alt_L => update_modifier(id, type_, 0, MOD_META_L),
        KS_Alt_R => update_modifier(id, type_, 0, MOD_META_R),
        KS_Mode_switch => update_modifier(id, type_, 0, MOD_MODESHIFT),
        KS_Mode_Lock => update_modifier(id, type_, 1, MOD_MODELOCK),
        KS_Num_Lock => update_modifier(id, type_, 1, MOD_NUMLOCK),
        KS_Hold_Screen => {
            if let Some(sc) = sc {
                update_modifier(id, type_, 1, MOD_HOLDSCREEN);
                if let Some(d) = sc.displaydv() {
                    wsdisplay_kbdholdscreen(d, id.t_modifiers.get() & MOD_HOLDSCREEN);
                }
            }
        }
        _ => {
            if let Some(sc) = sc
                && sc.sc_repeating.get() != 0
                && ((type_ == WSCONS_EVENT_KEY_UP && value != sc.sc_repkey.get())
                    || (type_ == WSCONS_EVENT_KEY_DOWN && value == sc.sc_repkey.get()))
            {
                return 0;
            }
        }
    }

    if let Some(sc) = sc {
        sc.stop_repeat();
        sc.sc_repkey.set(value);
    }

    // If this is a key release or we are in command mode, we are done
    if type_ != WSCONS_EVENT_KEY_DOWN || iscommand {
        return 0;
    }

    // Get the keysym
    let group2 = id.t_modifiers.get() & (MOD_MODESHIFT | MOD_MODELOCK) != 0
        && !id.mod_oneset(MOD_ANYCONTROL);
    let group = if group2 { kp.group2 } else { kp.group1 };

    let gindex;
    let ksym;
    if id.t_modifiers.get() & MOD_NUMLOCK != 0 && ks_group(u32::from(group[1])) == KS_GROUP_Keypad {
        gindex = usize::from(!id.mod_oneset(MOD_ANYSHIFT));
        ksym = group[gindex];
    } else if id.t_modifiers.get() & (MOD_CAPSLOCK | MOD_ANYSHIFT) == MOD_CAPSLOCK {
        // CAPS alone should only affect letter keys
        gindex = 0;
        ksym = ksym_upcase(group[0]);
    } else {
        gindex = usize::from(id.mod_oneset(MOD_ANYSHIFT));
        ksym = group[gindex];
    }

    // Submit Audio keys for hotkey processing
    if ks_group(u32::from(ksym)) == KS_GROUP_Function {
        let dir = match ksym {
            KS_AudioMute => Some(0),
            KS_AudioLower => Some(-1),
            KS_AudioRaise => Some(1),
            _ => None,
        };
        if let Some(dir) = dir {
            if AUDIO_KBDCONTROL_ENABLE.load(Ordering::Relaxed) == 1 {
                let cookie = sc.map_or(ptr::null_mut(), |sc| sc.sc_audiocookie.get());
                let _ = wskbd_set_mixervolume_dev(cookie, dir, 1);
            }
            return 0;
        }
    }

    // Process compose sequence and dead accents
    let mut res = KS_voidSymbol;

    match ks_group(u32::from(ksym)) {
        KS_GROUP_Ascii | KS_GROUP_Keypad | KS_GROUP_Function => res = ksym,

        KS_GROUP_Mod => {
            if ksym == KS_Multi_key {
                update_modifier(id, 1, 0, MOD_COMPOSE);
                id.t_composelen.set(2);
            }
        }

        KS_GROUP_Dead => {
            if id.t_composelen.get() == 0 {
                update_modifier(id, 1, 0, MOD_COMPOSE);
                id.t_composelen.set(1);
                let mut buf = id.t_composebuf.get();
                buf[0] = ksym;
                id.t_composebuf.set(buf);
            } else {
                res = ksym;
            }
        }

        _ => {}
    }

    if res == KS_voidSymbol {
        return 0;
    }

    if id.t_composelen.get() > 0 {
        // If the compose key also serves as AltGr (i.e. set to both KS_Multi_key and
        // KS_Mode_switch), and would provide a valid, distinct combination as AltGr, leave
        // compose mode.
        if id.t_composelen.get() == 2 && group2 && kp.group1[gindex] != kp.group2[gindex] {
            id.t_composelen.set(0);
        }

        if id.t_composelen.get() != 0 {
            let mut buf = id.t_composebuf.get();
            buf[(2 - id.t_composelen.get()) as usize] = res;
            id.t_composebuf.set(buf);
            id.t_composelen.set(id.t_composelen.get() - 1);
            if id.t_composelen.get() == 0 {
                res = wskbd_compose_value(&buf);
                update_modifier(id, 0, 0, MOD_COMPOSE);
            } else {
                return 0;
            }
        }
    }

    // We are done, return the symbol
    if ks_group(u32::from(res)) == KS_GROUP_Ascii {
        if id.mod_oneset(MOD_ANYCONTROL) {
            if (KS_at..=KS_z).contains(&res) || res == KS_space {
                res &= 0x1f;
            } else if res == KS_2 {
                res = 0x00;
            } else if (KS_3..=KS_7).contains(&res) {
                res = KS_Escape + (res - KS_3);
            } else if res == KS_8 {
                res = KS_Delete;
            }
        }
        if id.mod_oneset(MOD_ANYMETA) {
            if id.t_flags.get() & WSKFL_METAESC != 0 {
                id.t_symbols.set([KS_Escape, res]);
                return 2;
            } else {
                res |= 0x80;
            }
        }
    }

    let mut syms = id.t_symbols.get();
    syms[0] = res;
    id.t_symbols.set(syms);
    1
}

/// `wskbd_debugger`: enter ddb from the console keyboard, if `db_console` allows it.
pub fn wskbd_debugger(sc: &WskbdSoftc) {
    if sc.sc_isconsole.get() != 0 && DB_CONSOLE.load(Ordering::Relaxed) != 0 {
        let id = sc.id();
        match id.t_consops.get().and_then(|ops| ops.debugger) {
            Some(debugger) => debugger(id.t_consaccesscookie.get()),
            None => db_enter(),
        }
    }
}

/// `wskbd_set_keymap`: make `map` (`maplen` entries, `malloc`ed) the keyboard's map, freeing
/// the old one.
pub fn wskbd_set_keymap(sc: &WskbdSoftc, map: *mut WsconsKeymap, maplen: i32) {
    if let Some(old) = NonNull::new(sc.sc_map.get()) {
        free(
            old.cast(),
            M_DEVBUF,
            sc.sc_maplen.get() as usize * size_of::<WsconsKeymap>(),
        );
    }
    sc.sc_map.set(map);
    sc.sc_maplen.set(maplen);
}

/// `wskbd_kbd_backlight_task`: the keyboard backlight keys, through the backlight hooks.
fn wskbd_kbd_backlight_task(arg: *mut c_void) {
    // SAFETY: `wskbd_attach` set the task's argument to the softc.
    let sc = unsafe { &*arg.cast::<WskbdSoftc>() };

    // SAFETY: the hooks are installed at attach, under the kernel lock, which tasks of
    // `systq` hold.
    let (Some(get), Some(set)) = (unsafe { WSKBD_GET_BACKLIGHT.read() }, unsafe {
        WSKBD_SET_BACKLIGHT.read()
    }) else {
        return;
    };

    let cmd = sc.sc_kbd_backlight_cmd.swap(0, Ordering::Relaxed);
    if cmd != KBD_BACKLIGHT_UP && cmd != KBD_BACKLIGHT_DOWN && cmd != KBD_BACKLIGHT_TOGGLE {
        return;
    }

    let mut data = WskbdBacklight::default();
    let _ = get(&mut data);
    let (min, max, curval) = (data.min as i32, data.max as i32, data.curval as i32);
    let step = (max - min + 1) / 8;
    let val = if cmd == KBD_BACKLIGHT_UP {
        curval + step
    } else if cmd == KBD_BACKLIGHT_DOWN {
        curval - step
    } else if curval != 0 {
        0
    } else {
        (max - min + 1) / 2
    };
    data.curval = val.clamp(0, 0xff) as u32;
    let _ = set(&mut data);
}

/// `wskbd_brightness_task`: the display brightness keys.
fn wskbd_brightness_task(arg: *mut c_void) {
    // SAFETY: `wskbd_attach` set the task's argument to the softc.
    let sc = unsafe { &*arg.cast::<WskbdSoftc>() };
    let mut steps = sc.sc_brightness_steps.swap(0, Ordering::Relaxed);
    let mut dir = 1;

    if steps < 0 {
        steps = -steps;
        dir = -1;
    }
    while steps > 0 {
        steps -= 1;
        wsdisplay_brightness_step(None, dir);
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
pub(crate) mod tests {
    // Host tests of the keyboard: the keysym translation (modifiers, locks, control and meta,
    // the keypad, dead keys and compose) through the console path and through an attached
    // keyboard's map, the LEDs, event mode, the bell, repeat and encoding ioctls, and binding
    // to a display.

    use core::sync::atomic::AtomicI32;
    use std::boxed::Box;

    use super::*;
    use crate::dev::usb::ukbdmap::UKBD_KEYDESCTAB;
    use crate::kern::subr_pool::tests::setup_real_memory;
    use crate::sys::time::Timespec;

    /// The LEDs the fake driver was last told to light.
    static LEDS: AtomicI32 = AtomicI32::new(-1);
    /// How many times the fake driver was enabled minus disabled.
    static ENABLED: AtomicI32 = AtomicI32::new(0);

    fn fake_enable(_v: *mut c_void, on: i32) -> i32 {
        ENABLED.fetch_add(if on != 0 { 1 } else { -1 }, Ordering::Relaxed);
        0
    }

    fn fake_set_leds(_v: *mut c_void, leds: i32) {
        LEDS.store(leds, Ordering::Relaxed);
    }

    fn fake_ioctl(
        _v: *mut c_void,
        cmd: u64,
        _data: &mut [u8],
        _flag: i32,
        _p: Option<&Proc>,
    ) -> Result<bool, Errno> {
        Ok(cmd == WSKBDIO_COMPLEXBELL)
    }

    static FAKE_ACCESSOPS: WskbdAccessops = WskbdAccessops {
        enable: fake_enable,
        set_leds: fake_set_leds,
        ioctl: fake_ioctl,
    };

    /// A translation state over the USB layouts, as the console keyboard's before it attaches.
    fn state(layout: KbdT) -> WskbdInternal {
        let id = WskbdInternal::new();
        id.t_keydesc.set(&UKBD_KEYDESCTAB);
        wskbd_update_layout(&id, layout);
        id
    }

    /// An attached keyboard over the fake driver, with the map of `layout` and no key repeat.
    pub(crate) fn keyboard(layout: KbdT) -> &'static WskbdSoftc {
        // SAFETY: all-zero is a valid `WskbdSoftc` (its `Softc` contract); leaked for good.
        let sc: &'static WskbdSoftc = Box::leak(Box::new(unsafe { core::mem::zeroed() }));
        let id: &'static WskbdInternal = Box::leak(Box::new(state(layout)));
        sc.id.set(Some(NonNull::from(id)));
        id.t_sc.set(Some(NonNull::from(sc)));
        sc.sc_base.me_ops.set(Some(&WSKBD_SRCOPS));
        sc.sc_accessops.set(Some(&FAKE_ACCESSOPS));
        sc.sc_translating.set(1);
        sc.sc_ledstate.set(-1);
        // SAFETY: the defaults, read on the test's thread.
        sc.sc_bell_data
            .set(unsafe { WSKBD_DEFAULT_BELL_DATA.read() });
        let (map, len) = wskbd_load_keymap(&id.keymap(), layout).unwrap();
        wskbd_set_keymap(sc, map.as_ptr(), len as i32);
        sc
    }

    /// Presses and releases key `kc`: the keysyms the press produced.
    fn tap(id: &WskbdInternal, kc: i32) -> std::vec::Vec<KeysymT> {
        let n = wskbd_translate(id, WSCONS_EVENT_KEY_DOWN, kc);
        let syms = id.t_symbols.get()[..n as usize].to_vec();
        assert_eq!(wskbd_translate(id, WSCONS_EVENT_KEY_UP, kc), 0);
        syms
    }

    fn down(id: &WskbdInternal, kc: i32) {
        assert_eq!(
            wskbd_translate(id, WSCONS_EVENT_KEY_DOWN, kc),
            0,
            "a modifier"
        );
    }

    fn up(id: &WskbdInternal, kc: i32) {
        assert_eq!(wskbd_translate(id, WSCONS_EVENT_KEY_UP, kc), 0);
    }

    // USB key codes (usage page 7).
    const A: i32 = 4;
    const C: i32 = 6;
    const E: i32 = 8;
    const Y: i32 = 28;
    const ONE: i32 = 30;
    const TWO: i32 = 31;
    const THREE: i32 = 32;
    const RET: i32 = 40;
    const DEAD_ACUTE_DE: i32 = 46;
    const CAPS: i32 = 57;
    const NUMLOCK: i32 = 83;
    const KP1: i32 = 89;
    const CTRL_L: i32 = 224;
    const SHIFT_L: i32 = 225;
    const ALT_L: i32 = 226;
    const ALT_R: i32 = 230;

    #[test]
    fn plain_and_shifted_keys() {
        let id = state(KB_US | KB_DEFAULT);
        assert_eq!(tap(&id, A), [KS_a]);
        assert_eq!(tap(&id, ONE), [KS_1]);
        assert_eq!(tap(&id, RET), [KS_Return]);
        down(&id, SHIFT_L);
        assert_eq!(tap(&id, A), [KS_A]);
        assert_eq!(tap(&id, ONE), [KS_exclam]);
        up(&id, SHIFT_L);
        assert_eq!(tap(&id, A), [KS_a]);
        assert_eq!(tap(&id, 1000), [], "a code no layout has");
    }

    #[test]
    fn caps_lock_changes_letters_only() {
        let id = state(KB_US);
        assert_eq!(tap(&id, CAPS), []);
        assert_eq!(id.t_modifiers.get() & MOD_CAPSLOCK, MOD_CAPSLOCK);
        assert_eq!(tap(&id, A), [KS_A]);
        assert_eq!(tap(&id, ONE), [KS_1]);
        down(&id, SHIFT_L);
        assert_eq!(tap(&id, A), [KS_A], "shift and caps: the shifted symbol");
        assert_eq!(tap(&id, ONE), [KS_exclam]);
        up(&id, SHIFT_L);
        assert_eq!(tap(&id, CAPS), []);
        assert_eq!(tap(&id, A), [KS_a]);
    }

    #[test]
    fn control_and_meta() {
        let id = state(KB_US);
        down(&id, CTRL_L);
        assert_eq!(tap(&id, C), [0x03]);
        assert_eq!(tap(&id, TWO), [0x00]);
        assert_eq!(tap(&id, THREE), [KS_Escape]);
        up(&id, CTRL_L);

        down(&id, ALT_L);
        assert_eq!(tap(&id, A), [KS_a | 0x80]);
        up(&id, ALT_L);

        let id = state(KB_US | KB_METAESC);
        down(&id, ALT_L);
        assert_eq!(tap(&id, A), [KS_Escape, KS_a]);
        // All keys up drops the held modifiers.
        assert_eq!(wskbd_translate(&id, WSCONS_EVENT_ALL_KEYS_UP, 0), 0);
        assert_eq!(tap(&id, A), [KS_a]);
    }

    #[test]
    fn num_lock_picks_the_keypad_digit() {
        let id = state(KB_US);
        assert_eq!(tap(&id, KP1), [KS_KP_End]);
        assert_eq!(tap(&id, NUMLOCK), []);
        assert_eq!(tap(&id, KP1), [KS_KP_1]);
        down(&id, SHIFT_L);
        assert_eq!(tap(&id, KP1), [KS_KP_End]);
    }

    #[test]
    fn dead_accent_and_compose() {
        // German: the acute dead key, then e.
        let id = state(KB_DE);
        assert_eq!(tap(&id, Y), [KS_z]);
        assert_eq!(tap(&id, DEAD_ACUTE_DE), []);
        assert_eq!(id.t_composelen.get(), 1);
        // The C passes the type 1 (`WSCONS_EVENT_KEY_UP`) to `update_modifier` when it starts
        // a compose sequence, so `MOD_COMPOSE` (and its LED) stays off: kept as is.
        assert_eq!(id.t_modifiers.get() & MOD_COMPOSE, 0);
        assert_eq!(tap(&id, E), [KS_eacute]);
        assert_eq!(id.t_composelen.get(), 0);

        // US: shifted right Alt is Multi_key; a a composes to @.
        let id = state(KB_US);
        down(&id, SHIFT_L);
        assert_eq!(tap(&id, ALT_R), []);
        up(&id, SHIFT_L);
        assert_eq!(id.t_composelen.get(), 2);
        assert_eq!(tap(&id, A), []);
        assert_eq!(tap(&id, A), [KS_at]);
    }

    #[test]
    fn attached_keyboard_uses_its_map_and_lights_its_leds() {
        let _g = setup_real_memory();
        let sc = keyboard(KB_US | KB_DEFAULT);
        let id = sc.id();
        assert_eq!(sc.sc_maplen.get(), 237);
        assert_eq!(tap(id, A), [KS_a]);
        assert_eq!(
            wskbd_translate(id, WSCONS_EVENT_KEY_DOWN, 237),
            0,
            "out of the map"
        );
        assert_eq!(wskbd_translate(id, WSCONS_EVENT_KEY_DOWN, -1), 0);

        assert_eq!(tap(id, CAPS), []);
        assert_eq!(LEDS.load(Ordering::Relaxed), WSKBD_LED_CAPS);
        assert_eq!(tap(id, CAPS), []);
        assert_eq!(LEDS.load(Ordering::Relaxed), 0);
        assert_eq!(tap(id, NUMLOCK), []);
        assert_eq!(LEDS.load(Ordering::Relaxed), WSKBD_LED_NUM);
        assert_eq!(tap(id, NUMLOCK), []);

        // Ctrl_L carries KS_Cmd1, Alt_L KS_Cmd2: both held is command mode, where a key that
        // is no command is swallowed.
        down(id, CTRL_L);
        down(id, ALT_L);
        assert_eq!(
            id.t_modifiers.get() & (MOD_COMMAND1 | MOD_COMMAND2),
            MOD_COMMAND1 | MOD_COMMAND2
        );
        up(id, ALT_L);
        up(id, CTRL_L);
        assert_eq!(id.t_modifiers.get() & (MOD_COMMAND1 | MOD_COMMAND2), 0);
    }

    #[test]
    fn event_mode_queues_the_raw_key_codes() {
        let _g = setup_real_memory();
        let sc = keyboard(KB_US);
        let evar = &sc.sc_base.me_evar;
        wsevent_init(evar).unwrap();
        ENABLED.store(0, Ordering::Relaxed);
        wskbd_do_open(sc, evar).unwrap();
        assert_eq!(ENABLED.load(Ordering::Relaxed), 1);
        assert_eq!(sc.sc_translating.get(), 0);
        assert_eq!(wskbd_do_open(sc, evar), Err(Errno::EBUSY));

        wskbd_input(&sc.sc_base.me_dv, WSCONS_EVENT_KEY_DOWN, A);
        wskbd_input(&sc.sc_base.me_dv, WSCONS_EVENT_KEY_UP, A);
        assert_eq!(evar.ws_put.get(), 2);
        // SAFETY: two events were queued, nobody else writes the ring.
        let q = unsafe { evar.q_events(0, 2) };
        let ev = |i: usize| -> WsconsEvent { ioctl_arg(&q[i * 24..]) };
        assert_eq!((ev(0).type_, ev(0).value), (WSCONS_EVENT_KEY_DOWN, A));
        assert_eq!((ev(1).type_, ev(1).value), (WSCONS_EVENT_KEY_UP, A));
        assert_ne!(ev(0).time, Timespec::default());

        wskbd_mux_close(&sc.sc_base).unwrap();
        assert_eq!(ENABLED.load(Ordering::Relaxed), 0);
        assert_eq!(sc.sc_translating.get(), 1);
        wsevent_fini(evar);
    }

    #[test]
    fn bell_repeat_and_encoding_ioctls() {
        let _g = setup_real_memory();
        let sc = keyboard(KB_US | KB_DEFAULT);
        let ioctl =
            |cmd, data: &mut [u8], flag| wskbd_displayioctl_sc(sc, cmd, data, flag, None, false);

        let mut b = [0u8; size_of::<WskbdBellData>()];
        assert_eq!(ioctl(WSKBDIO_GETBELL, &mut b, FREAD), Ok(true));
        let bell: WskbdBellData = ioctl_arg(&b);
        assert_eq!(
            (bell.which, bell.pitch, bell.period, bell.volume),
            (WSKBD_BELL_DOALL, 400, 100, 50)
        );
        ioctl_ret(
            &mut b,
            &WskbdBellData {
                which: WSKBD_BELL_DOPITCH,
                pitch: 1000,
                period: 1,
                volume: 1,
            },
        );
        assert_eq!(ioctl(WSKBDIO_SETBELL, &mut b, FREAD), Err(Errno::EACCES));
        assert_eq!(ioctl(WSKBDIO_SETBELL, &mut b, FWRITE), Ok(true));
        assert_eq!(ioctl(WSKBDIO_GETBELL, &mut b, FREAD), Ok(true));
        let bell: WskbdBellData = ioctl_arg(&b);
        assert_eq!((bell.pitch, bell.period, bell.volume), (1000, 100, 50));
        assert_eq!(
            ioctl(WSKBDIO_BELL, &mut [], FWRITE),
            Ok(true),
            "the driver rings it"
        );
        assert_eq!(
            ioctl(WSKBDIO_SETDEFAULTBELL, &mut b, FWRITE),
            Err(Errno::EPERM)
        );

        let mut r = [0u8; size_of::<WskbdKeyrepeatData>()];
        assert_eq!(ioctl(WSKBDIO_GETKEYREPEAT, &mut r, FREAD), Ok(true));
        let rep: WskbdKeyrepeatData = ioctl_arg(&r);
        assert_eq!(
            (rep.which, rep.del1, rep.delN),
            (WSKBD_KEYREPEAT_DOALL, 0, 0)
        );
        assert_eq!(ioctl(WSKBDIO_GETDEFAULTKEYREPEAT, &mut r, FREAD), Ok(true));
        let rep: WskbdKeyrepeatData = ioctl_arg(&r);
        assert_eq!((rep.del1, rep.delN), (400, 100));

        let mut e = [0u8; 4];
        assert_eq!(ioctl(WSKBDIO_GETENCODING, &mut e, FREAD), Ok(true));
        assert_eq!(ioctl_arg::<KbdT>(&e), KB_US, "without KB_DEFAULT");
        ioctl_ret(&mut e, &KB_DE);
        assert_eq!(ioctl(WSKBDIO_SETENCODING, &mut e, FWRITE), Ok(true));
        assert_eq!(sc.id().t_layout.get(), KB_DE);
        assert_eq!(tap(sc.id(), Y), [KS_z], "the German map is loaded");
        ioctl_ret(&mut e, &(KB_HU | KB_APPLE));
        assert_eq!(
            ioctl(WSKBDIO_SETENCODING, &mut e, FWRITE),
            Err(Errno::EINVAL)
        );
        ioctl_ret(&mut e, &KB_USER);
        assert_eq!(
            ioctl(WSKBDIO_SETENCODING, &mut e, FWRITE),
            Err(Errno::EINVAL)
        );

        // Not wskbd's and not the driver's.
        assert_eq!(ioctl(0x1234, &mut e, FREAD), Ok(false));
        assert_eq!(
            wskbd_do_ioctl_sc(sc, 0x1234, &mut e, FREAD, None, false),
            Err(Errno::ENOTTY)
        );
        // FIOASYNC needs an open queue.
        assert_eq!(
            wskbd_do_ioctl_sc(sc, FIOASYNC, &mut e, FREAD, None, false),
            Err(Errno::EINVAL)
        );
    }

    #[test]
    fn display_binding() {
        let _g = setup_real_memory();
        let sc = keyboard(KB_US);
        // SAFETY: all-zero is a valid `Device`; leaked for good.
        let disp: &'static Device = Box::leak(Box::new(unsafe { core::mem::zeroed::<Device>() }));
        ENABLED.store(0, Ordering::Relaxed);

        assert_eq!(
            wskbd_set_display(&sc.sc_base.me_dv, None),
            Err(Errno::ENXIO)
        );
        assert_eq!(wskbd_set_display(&sc.sc_base.me_dv, Some(disp)), Ok(()));
        assert_eq!(ENABLED.load(Ordering::Relaxed), 1);
        assert!(sc.displaydv().is_some_and(|d| ptr::eq(d, disp)));
        assert_eq!(
            wskbd_set_display(&sc.sc_base.me_dv, Some(disp)),
            Err(Errno::EBUSY)
        );
        // A display's keyboard stays on whatever its users do.
        assert_eq!(wskbd_enable(sc, 0), Ok(()));
        assert_eq!(ENABLED.load(Ordering::Relaxed), 1);
        assert_eq!(wskbd_set_display(&sc.sc_base.me_dv, None), Ok(()));
        assert_eq!(ENABLED.load(Ordering::Relaxed), 0);
        assert!(sc.displaydv().is_none());

        sc.sc_isconsole.set(1);
        assert_eq!(
            wskbd_set_display(&sc.sc_base.me_dv, Some(disp)),
            Err(Errno::EBUSY)
        );
    }

    #[test]
    fn attach_locators_match_like_the_c() {
        static CF_ANY: crate::sys::device::Cfdata =
            crate::sys::device::Cfdata::new(&WSKBD_CA, &WSKBD_CD, 0, 0, &[-1, 1], 0, &[], 0, 0);
        static CF_CONS: crate::sys::device::Cfdata =
            crate::sys::device::Cfdata::new(&WSKBD_CA, &WSKBD_CD, 0, 0, &[1, 1], 0, &[], 0, 0);
        static NO_MAP: WskbdMapdata = WskbdMapdata::new(&[], 0);
        let mut a = WskbddevAttachArgs {
            console: 0,
            keymap: &NO_MAP,
            accessops: &FAKE_ACCESSOPS,
            accesscookie: ptr::null_mut(),
            audiocookie: ptr::null_mut(),
        };
        let aux = ptr::from_mut(&mut a).cast();
        assert_eq!(wskbd_match(None, &CfMatch::Cfdata(&CF_ANY), aux), 1);
        assert_eq!(wskbd_match(None, &CfMatch::Cfdata(&CF_CONS), aux), 0);
        a.console = 1;
        let aux = ptr::from_mut(&mut a).cast();
        assert_eq!(wskbd_match(None, &CfMatch::Cfdata(&CF_CONS), aux), 10);
        assert_eq!(wskbddevprint(ptr::null_mut(), Some(b"ukbd0")), UNCONF);
    }

    /// The defaults and `MOD_*` bits against `wskbd.c`.
    #[test]
    #[ignore = "needs OPENBSD_SRC"]
    fn constants_match_the_c() {
        let defs = crate::reftest::defines("sys/dev/wscons/wskbd.c");
        let mut ours = crate::reftest::assert_defines!(defs;
            MOD_SHIFT_L, MOD_SHIFT_R, MOD_SHIFTLOCK, MOD_CAPSLOCK, MOD_CONTROL_L, MOD_CONTROL_R,
            MOD_META_L, MOD_META_R, MOD_MODESHIFT, MOD_NUMLOCK, MOD_COMPOSE, MOD_HOLDSCREEN,
            MOD_COMMAND, MOD_COMMAND1, MOD_COMMAND2, MOD_MODELOCK, MOD_ANYSHIFT, MOD_ANYCONTROL,
            MOD_ANYMETA, WSKBD_DEFAULT_BELL_PITCH, WSKBD_DEFAULT_BELL_PERIOD,
            WSKBD_DEFAULT_BELL_VOLUME, WSKBD_DEFAULT_KEYREPEAT_DEL1, WSKBD_DEFAULT_KEYREPEAT_DELN,
            WSKFL_METAESC, MAXKEYSYMSPERKEY,
        );
        // `MOD_ANYLED` spans two lines (a `\` continuation), which the parser does not read.
        assert_eq!(MOD_ANYLED, 3596);
        ours.push("MOD_ANYLED");
        crate::reftest::assert_complete(&defs, "MOD_", &ours);
    }
}
/* </TESTS> */
