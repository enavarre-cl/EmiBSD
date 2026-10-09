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

/*-
 * Copyright (c) 1998 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Charles M. Hannum.
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

/*-
 * Copyright (c) 1990 The Regents of the University of California.
 * All rights reserved.
 *
 * This code is derived from software contributed to Berkeley by
 * William Jolitz and Don Ahn.
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
 *	@(#)pccons.c	5.11 (Berkeley) 5/21/91
 */
/* </LICENSES> */

/* <CODE> */
//! `pckbd(4)`: the PC (PS/2) keyboard on `pckbc(4)`'s keyboard slot, feeding `wskbd(4)`:
//! `dev/pckbc/pckbd.c`. Code to work keyboard for PC-style console.
//!
//! Upstream: sys/dev/pckbc/pckbd.c @ 3ce1f3f79392
//!
//! The probe resets the keyboard (and on a PC, when that fails, asks its id); the attach
//! asks the controller to translate to XT scan codes (set #1), falling back to set #2 or #3
//! on the keyboard itself, disables the keyboard until `wskbd` enables it and attaches a
//! `wskbd` child with the layouts of `wskbdmap_mfii.rs`. Every byte the keyboard sends
//! reaches [`pckbd_input`], which turns set #2 codes into set #1 if the controller does not,
//! decodes the `E0`/`E1` prefixes and the releases into key events, and hands them to
//! `wskbd_input` (or, in raw mode, the bytes to `wskbd_rawinput`).
//!
//! The bell is a hook: a beeper (`pcppi(4)`) registers itself with [`pckbd_hookup_bell`].
//! The console keyboard path ([`pckbd_cnattach`] and the console operations) is ported but
//! unused: `pckbc_cnattach` is never called (`pckbc.rs`), the console is the serial port.
//!
//! ## Deviations
//! - `WSDISPLAY_COMPAT_RAWKBD` is on in the GENERIC of both architectures, so its code is
//!   compiled in unconditionally (as in `hidkbd.rs`): `rawkbd`, `sc_rawcnt`, `sc_rawbuf`,
//!   the raw path of `pckbd_input` and `WSKBDIO_SETMODE`.
//! - The `#if defined(__i386__) || defined(__amd64__)` parts of the probe (the Chromebook
//!   `KBC_GETID` retry, the legacy-free PC without a PS/2 port) are chosen by the machine's
//!   `MACHINE_PC` constant. The `#ifdef __sparc64__` parts (set #3 keyboards on pckbc at
//!   ebus: `pckbd_xtbl3` and its use, the fixed table of `pckbd_set_xtscancode`) are not
//!   compiled, as on amd64 and arm64. `PCKBD_LAYOUT` is not defined: the layout is
//!   `KB_US | KB_DEFAULT`. The `DEBUG` messages are not compiled, as in GENERIC.
//! - `pckbd_internal`'s `t_kbctag` is a `PckbcTag`, the static `PCKBC_CONSDATA` until
//!   `pckbd_init` sets it; every field is a `Cell` (touched at `spltty()` under the kernel
//!   lock). The internal state of a non-console keyboard is written into its `malloc`ed
//!   block (`M_WAITOK`), never freed, as in the C.
//! - `pckbd_set_xtscancode` returns the C's 0 or 1 (which `pckbd_enable` returns as its
//!   errno, as the C); the access and console operations take the cookie as `*mut c_void`
//!   (`wskbdvar.rs`); `pckbd_decode` returns `bool`.
//! - The `wskbd` attach arguments' `audiocookie`, which the C leaves unset, is null.
//! - `pckbd_bell_fn` and `pckbd_bell_fn_arg` are one `StaticCell` (as `hidkbd.rs`'s bell).
//! - `pckbd_xtbl2_ext` is kept as the C has it, 127 entries: its `/* 0x00 */` row has 15,
//!   so every later entry sits one below the code its row comment names (`E0 11`, right
//!   Alt, finds 0). A slip of the C (docs/EXTERNAL_BUGS.md); only a controller that does not
//!   translate to set #1 reaches the table, and QEMU's does.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::{self, NonNull};

use libkern::StaticCell;

use crate::dev::ic::pckbc::{
    PCKBC_CONSDATA, pckbc_enqueue_cmd, pckbc_flush, pckbc_poll_cmd, pckbc_poll_data,
    pckbc_set_inputhandler, pckbc_set_poll, pckbc_slot_enable, pckbc_xt_translation,
};
use crate::dev::ic::pckbcvar::{
    PCKBC_KBD_SLOT, PCKBCCF_SLOT, PCKBCCF_SLOT_DEFAULT, PCKBCF_FORCE_KEYBOARD_PRESENT,
    PckbcAttachArgs, PckbcSlotT, PckbcTag,
};
use crate::dev::pckbc::pckbdreg::{
    KBC_DISABLE, KBC_ENABLE, KBC_GETID, KBC_MODEIND, KBC_RESET, KBC_SETTABLE, KBR_BREAK,
    KBR_EXTENDED0, KBR_EXTENDED1, KBR_RSTDONE,
};
use crate::dev::pckbc::wskbdmap_mfii::PCKBD_KEYDESCTAB;
use crate::dev::wscons::wsconsio::{
    WSCONS_EVENT_KEY_DOWN, WSCONS_EVENT_KEY_UP, WSKBD_LED_CAPS, WSKBD_LED_NUM, WSKBD_LED_SCROLL,
    WSKBD_RAW, WSKBD_TYPE_PC_XT, WSKBDIO_COMPLEXBELL, WSKBDIO_GETLEDS, WSKBDIO_GTYPE,
    WSKBDIO_SETLEDS, WSKBDIO_SETMODE, WskbdBellData,
};
use crate::dev::wscons::wskbd::{wskbd_cnattach, wskbd_input, wskbd_rawinput, wskbddevprint};
use crate::dev::wscons::wskbdraw::*;
use crate::dev::wscons::wskbdvar::{WskbdAccessops, WskbdConsops, WskbddevAttachArgs};
use crate::dev::wscons::wsksymdef::{KB_DEFAULT, KB_US};
use crate::dev::wscons::wsksymvar::WskbdMapdata;
use crate::kern::kern_malloc::malloc;
use crate::kern::subr_autoconf::{config_activate_children, config_found};
use crate::kern::subr_prf::{panic, printf};
use crate::machine::cpu::MACHINE_PC;
use crate::sys::device::{CfMatch, Cfattach, Cfdriver, DV_DULL, DVACT_RESUME, Device, Softc};
use crate::sys::errno::Errno;
use crate::sys::ioctl::{ioctl_arg, ioctl_ret};
use crate::sys::malloc::{M_DEVBUF, M_WAITOK};
use crate::sys::proc::Proc;

/// `struct pckbd_internal`: the keyboard's decoding state, apart from the device for the
/// console keyboard.
pub struct PckbdInternal {
    /// `t_isconsole`.
    pub t_isconsole: Cell<i32>,
    /// `t_kbctag`.
    pub t_kbctag: Cell<PckbcTag>,
    /// `t_kbcslot`.
    pub t_kbcslot: Cell<PckbcSlotT>,
    /// `t_translating`: nonzero if hardware performs translation.
    pub t_translating: Cell<i32>,
    /// `t_table`: scan code set in use.
    pub t_table: Cell<i32>,
    /// `t_lastchar`.
    pub t_lastchar: Cell<i32>,
    /// `t_extended`.
    pub t_extended: Cell<i32>,
    /// `t_extended1`.
    pub t_extended1: Cell<i32>,
    /// `t_releasing`.
    pub t_releasing: Cell<i32>,
    /// `t_sc`: back pointer.
    pub t_sc: Cell<Option<NonNull<PckbdSoftc>>>,
}

impl PckbdInternal {
    /// A keyboard nobody has initialised (`pckbd_init` sets it up).
    pub const fn new() -> Self {
        Self {
            t_isconsole: Cell::new(0),
            t_kbctag: Cell::new(&PCKBC_CONSDATA),
            t_kbcslot: Cell::new(0),
            t_translating: Cell::new(0),
            t_table: Cell::new(0),
            t_lastchar: Cell::new(0),
            t_extended: Cell::new(0),
            t_extended1: Cell::new(0),
            t_releasing: Cell::new(0),
            t_sc: Cell::new(None),
        }
    }
}

impl Default for PckbdInternal {
    fn default() -> Self {
        Self::new()
    }
}

// SAFETY: touched at `spltty()` under the kernel lock (the controller's interrupt handler is
// not `MPSAFE`), so no two CPUs reach the cells at once.
unsafe impl Sync for PckbdInternal {}

/// `struct pckbd_softc`.
#[repr(C)]
pub struct PckbdSoftc {
    /// `sc_dev`.
    pub sc_dev: Device,
    /// `id`.
    pub id: Cell<Option<&'static PckbdInternal>>,
    /// `sc_enabled`.
    pub sc_enabled: Cell<i32>,
    /// `sc_ledstate`.
    pub sc_ledstate: Cell<i32>,
    /// `sc_wskbddev`.
    pub sc_wskbddev: Cell<Option<NonNull<Device>>>,
    /// `rawkbd` (`WSDISPLAY_COMPAT_RAWKBD`).
    pub rawkbd: Cell<i32>,
    /// `sc_rawcnt`.
    pub sc_rawcnt: Cell<u32>,
    /// `sc_rawbuf`.
    pub sc_rawbuf: Cell<[u8; 3]>,
}

// SAFETY: `#[repr(C)]` with the device first; all-zero is `None`s, zeros and an empty buffer.
unsafe impl Softc for PckbdSoftc {}

/// The bell a beeper hooked up: `pckbd_bell_fn` and `pckbd_bell_fn_arg`.
#[derive(Clone, Copy)]
struct BellHook {
    /// `pckbd_bell_fn`.
    func: PckbdBellFn,
    /// `pckbd_bell_fn_arg`, as the address it is.
    arg: usize,
}

/// The type of `pckbd_bell_fn`: argument, pitch, period, volume, poll.
pub type PckbdBellFn = fn(arg: *mut c_void, pitch: u32, period: u32, volume: u32, poll: i32);

/// `pckbd_ca`.
pub static PCKBD_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<PckbdSoftc>(),
    ca_match: Some(pckbdprobe),
    ca_attach: pckbdattach,
    ca_detach: None,
    ca_activate: Some(pckbdactivate),
};

/// `pckbd_accessops`.
pub static PCKBD_ACCESSOPS: WskbdAccessops = WskbdAccessops {
    enable: pckbd_enable,
    set_leds: pckbd_set_leds,
    ioctl: pckbd_ioctl,
};

/// `pckbd_consops`.
pub static PCKBD_CONSOPS: WskbdConsops = WskbdConsops {
    getc: pckbd_cngetc,
    pollc: pckbd_cnpollc,
    bell: pckbd_cnbell,
    debugger: None,
};

/// `pckbd_keymapdata`: the layouts of `wskbdmap_mfii.c`, US by default (`PCKBD_LAYOUT` is
/// not defined).
pub static PCKBD_KEYMAPDATA: WskbdMapdata =
    WskbdMapdata::new(&PCKBD_KEYDESCTAB, KB_US | KB_DEFAULT);

/// Hackish support for a bell on the PC Keyboard; when a suitable beeper is found, it
/// attaches itself into the pckbd driver here. Written by `pckbd_hookup_bell`
/// (autoconfiguration) under the kernel lock.
static PCKBD_BELL: StaticCell<Option<BellHook>> = StaticCell::new(None);

/// `pckbd_consdata`: the console keyboard's state.
pub static PCKBD_CONSDATA: PckbdInternal = PckbdInternal::new();

/// `pckbd_cd`.
pub static PCKBD_CD: Cfdriver = Cfdriver::new(b"pckbd", DV_DULL, 0);

/// The softc behind an access operation's cookie.
fn pckbd_cookie(v: *mut c_void) -> &'static PckbdSoftc {
    // SAFETY: the cookie is the softc `pckbdattach` handed to `wskbd` and to `pckbc` as
    // their cookie and input argument; it is never detached.
    unsafe { &*v.cast::<PckbdSoftc>() }
}

/// `sc->sc_wskbddev`.
fn pckbd_wskbddev(sc: &PckbdSoftc) -> Option<&Device> {
    // SAFETY: what `config_found` returned in `pckbdattach`; never detached.
    sc.sc_wskbddev.get().map(|d| unsafe { d.as_ref() })
}

/// `pckbdactivate`: on resume, enable an enabled keyboard again (some are not after a
/// reset).
pub fn pckbdactivate(self_: &Device, act: i32) -> Result<(), Errno> {
    // SAFETY: `pckbd_ca` makes `PckbdSoftc`s.
    let sc = unsafe { self_.softc::<PckbdSoftc>() };

    if act == DVACT_RESUME
        && sc.sc_enabled.get() != 0
        && let Some(id) = sc.id.get()
    {
        // Some keyboards are not enabled after a reset, so make sure it is enabled now.
        let _ = pckbc_poll_cmd(
            id.t_kbctag.get(),
            id.t_kbcslot.get(),
            &[KBC_ENABLE],
            0,
            None,
            false,
        );
        // XXX - also invoke pckbd_set_xtscancode() too?
    }

    config_activate_children(self_, act)
}

/// `pckbd_set_xtscancode`: have the controller translate to XT codes, or else find a scan
/// code set the keyboard and this driver can both do; 1 when none is found.
pub fn pckbd_set_xtscancode(
    kbctag: PckbcTag,
    kbcslot: PckbcSlotT,
    id: Option<&PckbdInternal>,
) -> i32 {
    let mut table = 0;

    if !pckbc_xt_translation(kbctag, &mut table) {
        // #ifdef __sparc64__: a controller stuck to its table; not here.

        // Since the keyboard controller can not translate scan codes to the XT set (#1), we
        // would like to request this exact set. However it is likely that the controller
        // does not support it either.
        //
        // So try scan code set #2 as well, which this driver knows how to translate.
        table = 2;
        if let Some(id) = id {
            id.t_translating.set(0);
        }
    } else {
        table = 3;
        if let Some(id) = id {
            id.t_translating.set(1);
            if id.t_table.get() == 0 {
                // Don't bother explicitly setting into set 2, it's the default.
                id.t_table.set(2);
                return 0;
            }
        }
    }

    // keep falling back until we hit a table that looks usable.
    while table >= 1 {
        let cmd = [KBC_SETTABLE, table as u8];
        if pckbc_poll_cmd(kbctag, kbcslot, &cmd, 0, None, false).is_err() && table > 1 {
            let _ = pckbc_poll_cmd(kbctag, kbcslot, &[KBC_RESET], 1, None, true);
            pckbc_flush(kbctag, kbcslot);

            table -= 1;
            continue;
        }

        // the 8042 took the table set request, however, not all that report they can work
        // with table 3 actually work, so ask what table it reports it's in.
        if table == 3 {
            let mut resp = [0u8; 1];

            let cmd = [KBC_SETTABLE, 0];
            if pckbc_poll_cmd(kbctag, kbcslot, &cmd, 1, Some(&mut resp), false).is_err() {
                // query failed, step down to table 2 to be safe.
                table -= 1;
                continue;
            } else if resp[0] == 3 {
                break;
            }
        } else {
            break;
        }
        table -= 1;
    }

    if table == 0 {
        return 1;
    }

    if let Some(id) = id {
        id.t_table.set(table);
    }

    0
}

/// `pckbd_is_console`: `tag`/`slot` is the console keyboard.
fn pckbd_is_console(tag: PckbcTag, slot: PckbcSlotT) -> bool {
    PCKBD_CONSDATA.t_isconsole.get() != 0
        && ptr::eq(tag, PCKBD_CONSDATA.t_kbctag.get())
        && slot == PCKBD_CONSDATA.t_kbcslot.get()
}

/// `pckbdprobe`: these are both bad jokes. Reset the keyboard (on a PC, ask its id if that
/// fails); 2 when it answers.
pub fn pckbdprobe(parent: Option<&Device>, match_: &CfMatch, aux: *mut c_void) -> i32 {
    let cf = match_.cfdata();
    // SAFETY: `pckbc_attach_slot` hands its children a `PckbcAttachArgs`.
    let pa = unsafe { &*aux.cast::<PckbcAttachArgs>() };
    let mut resp = [0u8; 2];

    // XXX There are rumours that a keyboard can be connected to the aux port as well. For
    // me, this didn't work. For further experiments, allow it if explicitly wired in the
    // config file.
    let loc = cf
        .cf_loc
        .get(PCKBCCF_SLOT)
        .copied()
        .unwrap_or(PCKBCCF_SLOT_DEFAULT);
    if pa.pa_slot != PCKBC_KBD_SLOT && loc == PCKBCCF_SLOT_DEFAULT {
        return 0;
    }

    // Flush any garbage.
    pckbc_flush(pa.pa_tag, pa.pa_slot);

    // Reset the keyboard.
    let mut res = pckbc_poll_cmd(
        pa.pa_tag,
        pa.pa_slot,
        &[KBC_RESET],
        1,
        Some(&mut resp),
        true,
    );
    if res.is_ok() && resp[0] != KBR_RSTDONE {
        res = Err(Errno::EINVAL);
    }
    if MACHINE_PC && res.is_err() {
        // The 8042 emulation on Chromebooks fails the reset command but otherwise appears to
        // work correctly. Try a "get ID" command to give it a second chance.
        res = pckbc_poll_cmd(
            pa.pa_tag,
            pa.pa_slot,
            &[KBC_GETID],
            2,
            Some(&mut resp),
            false,
        );
        if res.is_ok() && (resp[0] != 0xab || resp[1] != 0x83) {
            res = Err(Errno::EINVAL);
        }
    }
    if let Err(e) = res {
        // There is probably no keyboard connected. Let the probe succeed if the keyboard is
        // used as console input - it can be connected later.
        //
        // However, on legacy-free PCs, there might really be no PS/2 connector at all; in
        // that case, do not even try to attach; ukbd will take over as console.
        if MACHINE_PC && e == Errno::ENXIO {
            // check cf_flags from parent
            let flags = parent.map_or(0, |p| p.cfdata().cf_flags);
            if flags & PCKBCF_FORCE_KEYBOARD_PRESENT == 0 {
                return 0;
            }
        }
        return i32::from(pckbd_is_console(pa.pa_tag, pa.pa_slot));
    }

    // Some keyboards seem to leave a second ack byte after the reset. This is kind of
    // stupid, but we account for them anyway by just flushing the buffer.
    pckbc_flush(pa.pa_tag, pa.pa_slot);

    2
}

/// `pckbdattach`: set the keyboard up (disabled until `wskbd` enables it, unless it is the
/// console's) and attach its `wskbd`.
pub fn pckbdattach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: `pckbd_ca` makes `PckbdSoftc`s; `config_make_softc`'s allocation lives as long
    // as the device, which is never freed while attached.
    let sc: &'static PckbdSoftc = unsafe { &*ptr::from_ref(self_.softc::<PckbdSoftc>()) };
    // SAFETY: as in `pckbdprobe`.
    let pa = unsafe { &*aux.cast::<PckbcAttachArgs>() };

    let isconsole = pckbd_is_console(pa.pa_tag, pa.pa_slot);

    let id: &'static PckbdInternal = if isconsole {
        let id = &PCKBD_CONSDATA;
        if id.t_table.get() == 0 {
            pckbd_set_xtscancode(pa.pa_tag, pa.pa_slot, Some(id));
        }

        // Some keyboards are not enabled after a reset, so make sure it is enabled now.
        let _ = pckbc_poll_cmd(
            id.t_kbctag.get(),
            id.t_kbcslot.get(),
            &[KBC_ENABLE],
            0,
            None,
            false,
        );
        sc.sc_enabled.set(1);
        id
    } else {
        let Some(mem) = malloc(size_of::<PckbdInternal>(), M_DEVBUF, M_WAITOK) else {
            panic(format_args!("pckbdattach: malloc"));
        };
        let p: NonNull<PckbdInternal> = mem.cast();
        // SAFETY: a fresh block of `size_of::<PckbdInternal>()` bytes, aligned by `malloc`,
        // that nothing else references; it is never freed.
        let id: &'static PckbdInternal = unsafe {
            p.as_ptr().write(PckbdInternal::new());
            &*p.as_ptr()
        };
        pckbd_init(id, pa.pa_tag, pa.pa_slot, 0);
        pckbd_set_xtscancode(pa.pa_tag, pa.pa_slot, Some(id));

        // no interrupts until enabled
        let _ = pckbc_poll_cmd(
            id.t_kbctag.get(),
            id.t_kbcslot.get(),
            &[KBC_DISABLE],
            0,
            None,
            false,
        );
        sc.sc_enabled.set(0);
        id
    };
    sc.id.set(Some(id));

    id.t_sc.set(Some(NonNull::from(sc)));

    // SAFETY: the softc is the device, alive while attached (see above).
    let name: &'static str = unsafe { &*ptr::from_ref(sc.sc_dev.xname()) };
    pckbc_set_inputhandler(
        id.t_kbctag.get(),
        id.t_kbcslot.get(),
        Some(pckbd_input),
        ptr::from_ref(sc).cast_mut().cast(),
        Some(name),
    );

    let mut a = WskbddevAttachArgs {
        console: i32::from(isconsole),
        keymap: &PCKBD_KEYMAPDATA,
        accessops: &PCKBD_ACCESSOPS,
        accesscookie: ptr::from_ref(sc).cast_mut().cast(),
        audiocookie: ptr::null_mut(),
    };

    printf(format_args!("\n"));

    // Attach the wskbd, saving a handle to it.
    sc.sc_wskbddev.set(config_found(
        &sc.sc_dev,
        ptr::from_mut(&mut a).cast(),
        Some(wskbddevprint),
    ));
}

/// `pckbd_enable`: the `wskbd` access operation: enable the keyboard (its port, scanning and
/// scan code set) or disable it; returns the errno.
pub fn pckbd_enable(v: *mut c_void, on: i32) -> i32 {
    let sc = pckbd_cookie(v);
    let Some(id) = sc.id.get() else {
        return Errno::ENXIO as i32;
    };
    let (tag, slot) = (id.t_kbctag.get(), id.t_kbcslot.get());

    if on != 0 {
        if sc.sc_enabled.get() != 0 {
            return Errno::EBUSY as i32;
        }

        pckbc_slot_enable(tag, slot, true);

        if let Err(res) = pckbc_poll_cmd(tag, slot, &[KBC_ENABLE], 0, None, false) {
            printf(format_args!("pckbd_enable: command error\n"));
            return res as i32;
        }

        let res = pckbd_set_xtscancode(tag, slot, Some(id));
        if res != 0 {
            return res;
        }

        sc.sc_enabled.set(1);
    } else {
        if id.t_isconsole.get() != 0 {
            return Errno::EBUSY as i32;
        }

        if let Err(res) = pckbc_enqueue_cmd(tag, slot, &[KBC_DISABLE], 0, true, None) {
            printf(format_args!("pckbd_disable: command error\n"));
            return res as i32;
        }

        pckbc_slot_enable(tag, slot, false);

        sc.sc_enabled.set(0);
    }

    0
}

/// `pckbd_xtbl2`: scan code set #2 to XT (set #1) codes, by set #2 code.
#[rustfmt::skip]
pub static PCKBD_XTBL2: [u8; 133] = [
    // 0x00
    0,
    RAWKEY_f9,
    0,
    RAWKEY_f5,
    RAWKEY_f3,
    RAWKEY_f1,
    RAWKEY_f2,
    RAWKEY_f12,
    RAWKEY_f6, // F6 according to documentation
    RAWKEY_f10,
    RAWKEY_f8,
    RAWKEY_f6, // F6 according to experimentation
    RAWKEY_f4,
    RAWKEY_Tab,
    RAWKEY_grave,
    0,
    // 0x10
    0,
    RAWKEY_Alt_L,
    RAWKEY_Shift_L,
    0,
    RAWKEY_Control_L,
    RAWKEY_q,
    RAWKEY_1,
    0,
    0,
    0,
    RAWKEY_z,
    RAWKEY_s,
    RAWKEY_a,
    RAWKEY_w,
    RAWKEY_2,
    RAWKEY_Meta_L,
    // 0x20
    0,
    RAWKEY_c,
    RAWKEY_x,
    RAWKEY_d,
    RAWKEY_e,
    RAWKEY_4,
    RAWKEY_3,
    0,
    RAWKEY_Meta_R,
    RAWKEY_space,
    RAWKEY_v,
    RAWKEY_f,
    RAWKEY_t,
    RAWKEY_r,
    RAWKEY_5,
    0,
    // 0x30
    0,
    RAWKEY_n,
    RAWKEY_b,
    RAWKEY_h,
    RAWKEY_g,
    RAWKEY_y,
    RAWKEY_6,
    0,
    0,
    0,
    RAWKEY_m,
    RAWKEY_j,
    RAWKEY_u,
    RAWKEY_7,
    RAWKEY_8,
    0,
    // 0x40
    0,
    RAWKEY_comma,
    RAWKEY_k,
    RAWKEY_i,
    RAWKEY_o,
    RAWKEY_0,
    RAWKEY_9,
    0,
    0,
    RAWKEY_period,
    RAWKEY_slash,
    RAWKEY_l,
    RAWKEY_semicolon,
    RAWKEY_p,
    RAWKEY_minus,
    0,
    // 0x50
    0,
    0,
    RAWKEY_apostrophe,
    0,
    RAWKEY_bracketleft,
    RAWKEY_equal,
    0,
    0,
    RAWKEY_Caps_Lock,
    RAWKEY_Shift_R,
    RAWKEY_Return,
    RAWKEY_bracketright,
    0,
    RAWKEY_backslash,
    0,
    0,
    // 0x60
    0,
    0,
    0,
    0,
    0,
    0,
    RAWKEY_BackSpace,
    0,
    0,
    RAWKEY_KP_End,
    0,
    RAWKEY_KP_Left,
    RAWKEY_KP_Home,
    0,
    0,
    0,
    // 0x70
    RAWKEY_KP_Insert,
    RAWKEY_KP_Delete,
    RAWKEY_KP_Down,
    RAWKEY_KP_Begin,
    RAWKEY_KP_Right,
    RAWKEY_KP_Up,
    RAWKEY_Escape,
    RAWKEY_Num_Lock,
    RAWKEY_f11,
    RAWKEY_KP_Add,
    RAWKEY_KP_Next,
    RAWKEY_KP_Subtract,
    RAWKEY_KP_Multiply,
    RAWKEY_KP_Prior,
    RAWKEY_Hold_Screen,
    0,
    // 0x80
    0,
    0,
    0,
    RAWKEY_f7,
    0, // Alt-Print Screen
];

/// `pckbd_xtbl2_ext`: the extended (`E0`-prefixed) set #2 codes; they already have the
/// upper bit set.
#[rustfmt::skip]
pub static PCKBD_XTBL2_EXT: [u8; 127] = [
    // 0x00
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    // 0x10
    0,
    RAWKEY_Alt_R,
    0, // E0 12, to be ignored
    0,
    RAWKEY_Control_R,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    // 0x20
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    // 0x30
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    // 0x40
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    RAWKEY_KP_Divide,
    0,
    0,
    0,
    0,
    0,
    // 0x50
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    RAWKEY_KP_Enter,
    0,
    0,
    0,
    0,
    0,
    // 0x60
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    RAWKEY_End,
    0,
    RAWKEY_Left,
    RAWKEY_Home,
    0,
    0,
    0,
    // 0x70
    RAWKEY_Insert,
    RAWKEY_Delete,
    RAWKEY_Down,
    0,
    RAWKEY_Right,
    RAWKEY_Up,
    0,
    0,
    0,
    0,
    RAWKEY_Next,
    0,
    RAWKEY_Print_Screen,
    RAWKEY_Prior,
    0xc6, // Ctrl-Break
    0,
];
// #ifdef __sparc64__: `pckbd_xtbl3`, the scan code set #3 translation table (pckbc at
// ebus on sparc64); not compiled here.

/// `pckbd_scancode_translate`: translate scan codes from set 2 or 3 to set 1; 0 when the
/// byte was consumed (a release prefix).
pub fn pckbd_scancode_translate(id: &PckbdInternal, datain: i32) -> i32 {
    if id.t_translating.get() != 0 || id.t_table.get() == 1 {
        return datain;
    }

    if datain == i32::from(KBR_BREAK) {
        id.t_releasing.set(0x80); // next keycode is a release
        return 0; // consume scancode
    }

    let mut datain = datain;
    #[allow(clippy::single_match)] // the C's switch, whose `case 3` is sparc64's
    match id.t_table.get() {
        2 => {
            // Convert BREAK sequence (14 77 -> 1D 45)
            if id.t_extended1.get() == 2 && datain == 0x14 {
                return 0x1d | id.t_releasing.get();
            } else if id.t_extended1.get() == 1 && datain == 0x77 {
                return 0x45 | id.t_releasing.get();
            }

            if id.t_extended.get() != 0 {
                datain = PCKBD_XTBL2_EXT
                    .get(datain as usize)
                    .map_or(0, |&c| i32::from(c));
                // xtbl2_ext already has the upper bit set
                id.t_extended.set(0);
            } else {
                datain = PCKBD_XTBL2
                    .get(datain as usize)
                    .map_or(0, |&c| i32::from(c) & !0x80);
            }
        }
        // #ifdef __sparc64__: case 3, pckbd_xtbl3.
        _ => {}
    }

    if datain == 0 {
        // We don't know how to translate this scan code, but we can't silently eat it
        // either (because there might have been an extended byte transmitted already).
        // Hopefully this value will be harmless to the upper layers.
        return 0xff;
    }

    datain | id.t_releasing.get()
}

/// `pckbd_decode`: turn a set #1 byte into a key event (`type_`, `dataout`); `false` while
/// a sequence is incomplete or for typematic repeats.
fn pckbd_decode(id: &PckbdInternal, datain: i32, type_: &mut u32, dataout: &mut i32) -> bool {
    if datain == i32::from(KBR_EXTENDED0) {
        id.t_extended.set(0x80);
        return false;
    } else if datain == i32::from(KBR_EXTENDED1) {
        id.t_extended1.set(2);
        return false;
    }

    let releasing = datain & 0x80;
    let mut datain = datain & 0x7f;

    // process BREAK key sequence (EXT1 1D 45 / EXT1 9D C5): map to (unused) code 7F
    if id.t_extended1.get() == 2 && datain == 0x1d {
        id.t_extended1.set(1);
        return false;
    } else if id.t_extended1.get() == 1 && datain == 0x45 {
        id.t_extended1.set(0);
        datain = 0x7f;
    } else {
        id.t_extended1.set(0);
    }

    if id.t_translating.get() != 0 || id.t_table.get() == 1 {
        id.t_releasing.set(releasing);
    } else {
        // id->t_releasing computed in pckbd_scancode_translate()
    }

    // map extended keys to (unused) codes 128-254
    let key = datain | id.t_extended.get();
    id.t_extended.set(0);

    if id.t_releasing.get() != 0 {
        id.t_releasing.set(0);
        id.t_lastchar.set(0);
        *type_ = WSCONS_EVENT_KEY_UP;
    } else {
        // Always ignore typematic keys
        if key == id.t_lastchar.get() {
            return false;
        }
        id.t_lastchar.set(key);
        *type_ = WSCONS_EVENT_KEY_DOWN;
    }

    *dataout = key;
    true
}

/// `pckbd_init`: a fresh decoding state for the keyboard on `kbctag`/`kbcslot`.
pub fn pckbd_init(t: &PckbdInternal, kbctag: PckbcTag, kbcslot: PckbcSlotT, console: i32) {
    // bzero
    t.t_translating.set(0);
    t.t_table.set(0);
    t.t_lastchar.set(0);
    t.t_extended.set(0);
    t.t_extended1.set(0);
    t.t_releasing.set(0);
    t.t_sc.set(None);

    t.t_isconsole.set(console);
    t.t_kbctag.set(kbctag);
    t.t_kbcslot.set(kbcslot);
}

/// `pckbd_led_encode`: `WSKBD_LED_*` to the keyboard's LED bits.
fn pckbd_led_encode(led: i32) -> u8 {
    let mut res = 0;

    if led & WSKBD_LED_SCROLL != 0 {
        res |= 0x01;
    }
    if led & WSKBD_LED_NUM != 0 {
        res |= 0x02;
    }
    if led & WSKBD_LED_CAPS != 0 {
        res |= 0x04;
    }
    res
}

/// `pckbd_set_leds`: the `wskbd` access operation: light the LEDs (asynchronously).
pub fn pckbd_set_leds(v: *mut c_void, leds: i32) {
    let sc = pckbd_cookie(v);
    let cmd = [KBC_MODEIND, pckbd_led_encode(leds)];
    sc.sc_ledstate.set(leds);

    if let Some(id) = sc.id.get() {
        let _ = pckbc_enqueue_cmd(id.t_kbctag.get(), id.t_kbcslot.get(), &cmd, 0, false, None);
    }
}

/// `pckbd_input`: got a console receive interrupt - the console processor wants to give us
/// a character.
pub fn pckbd_input(vsc: *mut c_void, data: i32) {
    let sc = pckbd_cookie(vsc);
    let Some(id) = sc.id.get() else {
        return;
    };
    let mut type_ = 0;
    let mut key = 0;

    let data = pckbd_scancode_translate(id, data);
    if data == 0 {
        return;
    }

    let rc = pckbd_decode(id, data, &mut type_, &mut key);

    // WSDISPLAY_COMPAT_RAWKBD
    if sc.rawkbd.get() != 0 {
        let mut buf = sc.sc_rawbuf.get();
        let cnt = sc.sc_rawcnt.get() as usize;
        buf[cnt % buf.len()] = data as u8;
        let cnt = cnt + 1;
        sc.sc_rawbuf.set(buf);
        sc.sc_rawcnt.set(cnt as u32);

        if rc || cnt == buf.len() {
            if let Some(dev) = pckbd_wskbddev(sc) {
                wskbd_rawinput(dev, &buf[..cnt.min(buf.len())]);
            }
            sc.sc_rawcnt.set(0);
        }

        // Pass audio keys to wskbd_input anyway.
        if !rc || (key != 160 && key != 174 && key != 176) {
            return;
        }
    }
    if rc && let Some(dev) = pckbd_wskbddev(sc) {
        wskbd_input(dev, type_, key);
    }
}

/// `pckbd_ioctl`: the `wskbd` access operation for the ioctls: the keyboard type, the LEDs,
/// the bell and the raw mode. `Ok(false)` where the C returns `-1`.
pub fn pckbd_ioctl(
    v: *mut c_void,
    cmd: u64,
    data: &mut [u8],
    _flag: i32,
    _p: Option<&Proc>,
) -> Result<bool, Errno> {
    let sc = pckbd_cookie(v);

    match cmd {
        WSKBDIO_GTYPE => {
            ioctl_ret(data, &(WSKBD_TYPE_PC_XT as i32));
            Ok(true)
        }
        WSKBDIO_SETLEDS => {
            let leds: i32 = ioctl_arg(data);
            let cmd = [KBC_MODEIND, pckbd_led_encode(leds)];
            sc.sc_ledstate
                .set(leds & (WSKBD_LED_SCROLL | WSKBD_LED_NUM | WSKBD_LED_CAPS));
            let Some(id) = sc.id.get() else {
                return Err(Errno::ENXIO);
            };
            pckbc_enqueue_cmd(id.t_kbctag.get(), id.t_kbcslot.get(), &cmd, 0, true, None)?;
            Ok(true)
        }
        WSKBDIO_GETLEDS => {
            ioctl_ret(data, &sc.sc_ledstate.get());
            Ok(true)
        }
        WSKBDIO_COMPLEXBELL => {
            // Keyboard can't beep directly; we have an externally-provided global hook to do
            // this.
            let d: WskbdBellData = ioctl_arg(data);
            pckbd_bell(d.pitch, d.period, d.volume, 0);
            Ok(true)
        }
        // WSDISPLAY_COMPAT_RAWKBD
        WSKBDIO_SETMODE => {
            sc.rawkbd
                .set(i32::from(ioctl_arg::<i32>(data) == WSKBD_RAW));
            Ok(true)
        }
        _ => Ok(false),
    }
}

/// `pckbd_bell`: ring the bell through the hook a beeper registered, if any.
pub fn pckbd_bell(pitch: u32, period: u32, volume: u32, poll: i32) {
    // SAFETY: the hook is written once, by `pckbd_hookup_bell` during autoconfiguration under
    // the kernel lock, which every caller of the bell also holds.
    if let Some(hook) = unsafe { PCKBD_BELL.read() } {
        (hook.func)(hook.arg as *mut c_void, pitch, period, volume, poll);
    }
}

/// `pckbd_hookup_bell`: register the bell (the first beeper to do so keeps it).
pub fn pckbd_hookup_bell(func: PckbdBellFn, arg: *mut c_void) {
    // SAFETY: see `pckbd_bell`: autoconfiguration under the kernel lock, no reader running.
    let hook = unsafe { PCKBD_BELL.get_mut() };
    if hook.is_none() {
        *hook = Some(BellHook {
            func,
            arg: arg as usize,
        });
    }
}

/// `pckbd_cnattach`: the keyboard on the console controller is the console keyboard.
pub fn pckbd_cnattach(kbctag: PckbcTag) -> Result<(), Errno> {
    pckbd_init(&PCKBD_CONSDATA, kbctag, PCKBC_KBD_SLOT, 1);
    wskbd_cnattach(
        &PCKBD_CONSOPS,
        ptr::from_ref(&PCKBD_CONSDATA).cast_mut().cast(),
        &PCKBD_KEYMAPDATA,
    );
    Ok(())
}

/// The console keyboard's state behind a console operation's cookie.
fn pckbd_cons_cookie(v: *mut c_void) -> &'static PckbdInternal {
    // SAFETY: `pckbd_cnattach` hands `wskbd_cnattach` `pckbd_consdata` as the cookie.
    unsafe { &*v.cast::<PckbdInternal>() }
}

/// `pckbd_cngetc`: the console keyboard's next key event, by polling.
pub fn pckbd_cngetc(v: *mut c_void, type_: &mut u32, data: &mut i32) {
    let t = pckbd_cons_cookie(v);

    loop {
        let Some(val) = pckbc_poll_data(t.t_kbctag.get(), t.t_kbcslot.get()) else {
            continue;
        };

        let val = pckbd_scancode_translate(t, i32::from(val));
        if val == 0 {
            continue;
        }

        if pckbd_decode(t, val, type_, data) {
            return;
        }
    }
}

/// `pckbd_cnpollc`: switch the console keyboard's polling on or off.
pub fn pckbd_cnpollc(v: *mut c_void, on: i32) {
    let t = pckbd_cons_cookie(v);

    pckbc_set_poll(t.t_kbctag.get(), t.t_kbcslot.get(), on != 0);

    // If we enter ukc or ddb before having attached the console keyboard we need to probe
    // its scan code set.
    if t.t_table.get() == 0 {
        pckbc_flush(t.t_kbctag.get(), t.t_kbcslot.get());
        pckbd_set_xtscancode(t.t_kbctag.get(), t.t_kbcslot.get(), Some(t));

        // Just to be sure.
        let _ = pckbc_poll_cmd(
            t.t_kbctag.get(),
            PCKBC_KBD_SLOT,
            &[KBC_ENABLE],
            0,
            None,
            false,
        );
    }
}

/// `pckbd_cnbell`: the console bell.
pub fn pckbd_cnbell(_v: *mut c_void, pitch: u32, period: u32, volume: u32) {
    pckbd_bell(pitch, period, volume, 1);
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use std::vec::Vec;

    use super::*;

    /// A keyboard whose controller translates (as QEMU's i8042 does), or not, on `table`.
    fn kbd(translating: i32, table: i32) -> PckbdInternal {
        let t = PckbdInternal::new();
        pckbd_init(&t, &PCKBC_CONSDATA, PCKBC_KBD_SLOT, 0);
        t.t_translating.set(translating);
        t.t_table.set(table);
        t
    }

    /// The events `bytes` decode into, as `pckbd_input` makes them.
    fn events(t: &PckbdInternal, bytes: &[u8]) -> Vec<(u32, i32)> {
        let mut out = Vec::new();
        for &b in bytes {
            let d = pckbd_scancode_translate(t, i32::from(b));
            if d == 0 {
                continue;
            }
            let (mut ty, mut key) = (0, 0);
            if pckbd_decode(t, d, &mut ty, &mut key) {
                out.push((ty, key));
            }
        }
        out
    }

    const DOWN: u32 = WSCONS_EVENT_KEY_DOWN;
    const UP: u32 = WSCONS_EVENT_KEY_UP;

    #[test]
    fn translated_set1_keys_and_releases() {
        let t = kbd(1, 2);
        // 'h' (0x23), its release (0xa3), 'i' (0x17) and Return (0x1c) pressed and released.
        assert_eq!(
            events(&t, &[0x23, 0xa3, 0x17, 0x97, 0x1c, 0x9c]),
            [
                (DOWN, 0x23),
                (UP, 0x23),
                (DOWN, 0x17),
                (UP, 0x17),
                (DOWN, 0x1c),
                (UP, 0x1c)
            ]
        );
    }

    #[test]
    fn typematic_repeats_are_dropped_and_extended_keys_moved_up() {
        let t = kbd(1, 2);
        assert_eq!(
            events(&t, &[0x1e, 0x1e, 0x1e, 0x9e]),
            [(DOWN, 0x1e), (UP, 0x1e)]
        );
        // Right Control: E0 1D, E0 9D.
        assert_eq!(
            events(&t, &[0xe0, 0x1d, 0xe0, 0x9d]),
            [(DOWN, 0x9d), (UP, 0x9d)]
        );
        // Pause: E1 1D 45 E1 9D C5 is code 7F down, then up.
        assert_eq!(
            events(&t, &[0xe1, 0x1d, 0x45, 0xe1, 0x9d, 0xc5]),
            [(DOWN, 0x7f), (UP, 0x7f)]
        );
    }

    #[test]
    fn untranslated_set2_codes() {
        let t = kbd(0, 2);
        // 'a' is 1C in set 2, F0 1C its release; RAWKEY_a is 0x1e.
        assert_eq!(events(&t, &[0x1c, 0xf0, 0x1c]), [(DOWN, 0x1e), (UP, 0x1e)]);
        // An unknown code becomes 0xff (0x7f once decoded), not silence.
        assert_eq!(pckbd_scancode_translate(&t, 0x02), 0xff);
        assert_eq!(pckbd_scancode_translate(&t, 0x200), 0xff);
        // Set 1 keyboards are not translated.
        let t1 = kbd(0, 1);
        assert_eq!(pckbd_scancode_translate(&t1, 0x1c), 0x1c);
    }

    #[test]
    fn led_encoding() {
        assert_eq!(pckbd_led_encode(0), 0);
        assert_eq!(pckbd_led_encode(WSKBD_LED_SCROLL), 1);
        assert_eq!(pckbd_led_encode(WSKBD_LED_NUM), 2);
        assert_eq!(pckbd_led_encode(WSKBD_LED_CAPS), 4);
        assert_eq!(
            pckbd_led_encode(WSKBD_LED_SCROLL | WSKBD_LED_NUM | WSKBD_LED_CAPS),
            7
        );
    }

    #[test]
    fn the_tables_have_the_c_sizes() {
        assert_eq!(PCKBD_XTBL2.len(), 0x85);
        assert_eq!(PCKBD_XTBL2_EXT.len(), 127);
        assert_eq!(PCKBD_XTBL2[0x1c], RAWKEY_a);
        // The C's `/* 0x00 */` row of pckbd_xtbl2_ext has 15 entries, not 16, so every
        // later entry sits one below the index its row comment names (Ctrl-Break, in the
        // `/* 0x70 */` row's 15th place, is at 0x7d); kept as the C has it.
        assert_eq!(PCKBD_XTBL2_EXT[0x10], RAWKEY_Alt_R);
        assert_eq!(PCKBD_XTBL2_EXT[0x7d], 0xc6);
    }

    /// Both translation tables against `pckbd.c`, entry by entry, with the `RAWKEY_*`
    /// names evaluated from `wskbdraw.h`.
    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn tables_match_the_c() {
        let path = crate::reftest::openbsd_src().join("sys/dev/pckbc/pckbd.c");
        let text = std::fs::read_to_string(path).unwrap();
        let defs = crate::reftest::defines("sys/dev/wscons/wskbdraw.h");
        for (name, ours) in [
            ("pckbd_xtbl2[]", &PCKBD_XTBL2[..]),
            ("pckbd_xtbl2_ext[]", &PCKBD_XTBL2_EXT[..]),
        ] {
            let start = text.find(name).unwrap();
            let body = &text[start..];
            let body = &body[body.find('{').unwrap() + 1..body.find("};").unwrap()];
            let mut vals = Vec::new();
            for line in body.lines() {
                let line = match line.find("/*") {
                    Some(i) => &line[..i],
                    None => line,
                };
                for tok in line.split(',').map(str::trim).filter(|t| !t.is_empty()) {
                    let v = if let Some(h) = tok.strip_prefix("0x") {
                        i64::from_str_radix(h, 16).unwrap()
                    } else if tok == "0" {
                        0
                    } else {
                        crate::reftest::int(&defs, tok).unwrap_or_else(|| panic!("{tok}"))
                    };
                    vals.push(v);
                }
            }
            let ours: Vec<i64> = ours.iter().map(|&b| i64::from(b)).collect();
            assert_eq!(ours, vals, "{name}");
        }
    }
}
/* </TESTS> */
