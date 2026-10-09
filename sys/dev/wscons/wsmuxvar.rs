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
//! `<dev/wscons/wsmuxvar.h>`: a wscons event source (`wskbd`, `wsmouse` or `wsmux`), the
//! methods a mux calls on its sources, and the mux's own softc.
//!
//! Upstream: sys/dev/wscons/wsmuxvar.h @ 3ce1f3f79392
//!
//! Every event source starts with a [`Wsevsrc`]: its device, its [`Wssrcops`], the event
//! queue it uses when opened directly (`me_evar`), the queue it delivers to while open
//! (`me_evp`: its own, or its root mux's), the display it is the input of and the mux it is
//! a child of. A [`WsmuxSoftc`] is a source too, with the list of its children; the
//! functions are `wsmux.rs`'s.
//!
//! ## Deviations
//! - The kernel configuration is GENERIC's: `NWSDISPLAY > 0` and `NWSMUX > 0`, so
//!   `me_dispdv`, `me_parent` and `me_next` are always members, and
//!   `WSDISPLAY_COMPAT_RAWKBD`'s `sc_rawkbd` is a member whatever the feature says (the code
//!   that reads it tests `cfg!(feature = "wsdisplay_compat_rawkbd")`).
//! - The structures are `#[repr(C)]` with the device first, as `config_make_softc`'s softcs
//!   must be ([`Softc`]): a `struct wskbd_softc` starts with its `Wsevsrc`, a `struct
//!   wsmux_softc` too, and the C's casts between `struct device *`, `struct wsevsrc *` and
//!   the softcs stay pointer casts ([`Wsevsrc::of_device`], [`WsmuxSoftc::of_evsrc`]). The
//!   pointers between them are `Cell<Option<NonNull<_>>>`, read through accessors.
//! - The `wssrcops` methods are `fn` pointers; `dioctl` returns `Result<(), Errno>`,
//!   `ddispioctl` `Result<bool, Errno>` with `Ok(false)` for the C's -1 ("not mine",
//!   `docs/C_TO_RUST.md`), and both take the kernel copy of the ioctl argument as bytes and
//!   an `Option<&Proc>` (the mux passes NULL). `ddispioctl` and `dsetdisplay`, which the C
//!   tests for NULL, are `Option`s.
//! - The `wsevsrc_*` macros are functions of the same names.
//! - The prototypes at the end of the header belong to `wsmux.c` (`wsmux.rs`), `wskbd.c`
//!   (`wskbd_add_mux`, `wskbd.rs`) and `wsmouse.c` (`wsmouse_add_mux`, `wsmouse.rs`).

use core::cell::Cell;
use core::ptr::{self, NonNull};

use crate::dev::wscons::wseventvar::Wseventvar;
use crate::queue_adapter;
use crate::sys::device::{Device, Softc};
use crate::sys::errno::Errno;
use crate::sys::proc::Proc;
use crate::sys::queue::{TailqEntry, TailqHead};
use crate::sys::rwlock::Rwlock;

/// `WSMOUSEDEVCF_MUX`: the index of `wsmousedev`'s `mux` locator.
pub const WSMOUSEDEVCF_MUX: usize = 0;

/// `struct wsevsrc`: a ws event source, i.e., wskbd, wsmouse, or wsmux.
///
/// All-zero is valid (no methods, no queue, no links), as `M_ZERO` softcs need.
#[repr(C)]
pub struct Wsevsrc {
    /// `me_dv`.
    pub me_dv: Device,
    /// `me_ops`: method pointers.
    pub me_ops: Cell<Option<&'static Wssrcops>>,
    /// `me_evar`: wseventvar opened directly.
    pub me_evar: Wseventvar,
    /// `me_evp`: our wseventvar when open.
    pub me_evp: Cell<Option<NonNull<Wseventvar>>>,
    /// `me_dispdv`: our display if part of one (`NWSDISPLAY > 0`; the softcs' `sc_displaydv`).
    pub me_dispdv: Cell<Option<NonNull<Device>>>,
    /// `me_parent`: parent mux device (`NWSMUX > 0`).
    pub me_parent: Cell<Option<NonNull<WsmuxSoftc>>>,
    /// `me_next`: sibling pointers.
    pub me_next: TailqEntry<Wsevsrc>,
}

// SAFETY: `#[repr(C)]` with the device first, and every member valid all-zero (`None`s,
// null links, a closed queue).
unsafe impl Softc for Wsevsrc {}

impl Wsevsrc {
    /// `(struct wsevsrc *)dv`: the event source a device is the head of.
    ///
    /// # Safety
    ///
    /// `dv` is the `me_dv` of a [`Wsevsrc`] (the device of a wskbd, or a mux's), which lives
    /// for as long as the reference is used.
    pub unsafe fn of_device(dv: &Device) -> &Wsevsrc {
        // SAFETY: the caller's contract; `me_dv` is the first member of a `#[repr(C)]`
        // structure.
        unsafe { &*ptr::from_ref(dv).cast::<Wsevsrc>() }
    }

    /// `me_ops`, which every attached source has.
    pub fn ops(&self) -> &'static Wssrcops {
        match self.me_ops.get() {
            Some(ops) => ops,
            None => crate::kern::subr_prf::panic(format_args!(
                "wsevsrc {}: no methods",
                self.me_dv.xname()
            )),
        }
    }

    /// `me_evp`: the queue this source delivers to, while open.
    pub fn evp(&self) -> Option<&Wseventvar> {
        // SAFETY: `me_evp` is set by the open paths to this source's own `me_evar` or to its
        // root mux's, which stays allocated until the close that clears `me_evp`.
        self.me_evp.get().map(|p| unsafe { p.as_ref() })
    }

    /// `me_dispdv` (`sc_displaydv`): the display this source is the input of.
    pub fn dispdv(&self) -> Option<&Device> {
        // SAFETY: a display device is never freed while a source points at it: wsdisplay's
        // detach disconnects its mux's sources first (`wsmux_set_display(NULL)`).
        self.me_dispdv.get().map(|p| unsafe { p.as_ref() })
    }

    /// `me_parent`: the mux this source is a child of.
    pub fn parent(&self) -> Option<&WsmuxSoftc> {
        // SAFETY: muxes are created by `wsmux_create` and never freed.
        self.me_parent.get().map(|p| unsafe { p.as_ref() })
    }
}

queue_adapter!(
    /// `TAILQ_ENTRY(wsevsrc) me_next`: a mux's children.
    pub WsevsrcNext: Wsevsrc, me_next => TailqEntry<Wsevsrc>
);

/// The type of `wssrcops`' `dioctl`.
pub type WssrcIoctlFn =
    fn(dv: &Device, cmd: u64, data: &mut [u8], flag: i32, p: Option<&Proc>) -> Result<(), Errno>;

/// The type of `wssrcops`' `ddispioctl`: `Ok(false)` where the C returns -1 (no component
/// accepts the ioctl).
pub type WssrcDispIoctlFn =
    fn(dv: &Device, cmd: u64, data: &mut [u8], flag: i32, p: Option<&Proc>) -> Result<bool, Errno>;

/// The type of `wssrcops`' `dsetdisplay`: connect the source to a display, or disconnect it
/// for `None`.
pub type WssrcSetDisplayFn = fn(dv: &Device, displaydv: Option<&Device>) -> Result<(), Errno>;

/// `struct wssrcops`: methods that can be performed on an events source. Usually called from
/// a wsmux.
pub struct Wssrcops {
    /// `type`: device type: `WSMUX_{MOUSE,KBD,MUX}`.
    pub type_: i32,
    /// `dopen`.
    pub dopen: fn(me: &Wsevsrc, evp: &Wseventvar) -> Result<(), Errno>,
    /// `dclose`.
    pub dclose: fn(me: &Wsevsrc) -> Result<(), Errno>,
    /// `dioctl`.
    pub dioctl: WssrcIoctlFn,
    /// `ddispioctl`.
    pub ddispioctl: Option<WssrcDispIoctlFn>,
    /// `dsetdisplay`.
    pub dsetdisplay: Option<WssrcSetDisplayFn>,
}

/// `struct wsmux_softc` (`NWSMUX > 0`).
///
/// All-zero is valid but for `sc_lock`'s name, which `wsmux_create` sets (`rw_init_flags`).
#[repr(C)]
pub struct WsmuxSoftc {
    /// `sc_base`.
    pub sc_base: Wsevsrc,
    /// `sc_p`: open proc.
    pub sc_p: Cell<*const Proc>,
    /// `sc_cld`: list of children.
    pub sc_cld: TailqHead<WsevsrcNext>,
    /// `sc_lock`: lock for `sc_cld`.
    pub sc_lock: Rwlock,
    /// `sc_kbd_layout`: current layout of keyboard.
    pub sc_kbd_layout: Cell<u32>,
    /// `sc_rawkbd` (`WSDISPLAY_COMPAT_RAWKBD`): a hack to remember the kbd mode.
    pub sc_rawkbd: Cell<i32>,
}

// SAFETY: `#[repr(C)]`, its first member a `Wsevsrc` whose first member is the device, and
// every member valid all-zero (an unnamed, free rwlock; an empty list).
unsafe impl Softc for WsmuxSoftc {}

impl WsmuxSoftc {
    /// `(struct wsmux_softc *)me`: the mux whose `sc_base` `me` is.
    ///
    /// # Safety
    ///
    /// `me` is the `sc_base` of a [`WsmuxSoftc`] (its `me_ops` is `wsmux_srcops`).
    pub unsafe fn of_evsrc(me: &Wsevsrc) -> &WsmuxSoftc {
        // SAFETY: the caller's contract; `sc_base` is the first member of a `#[repr(C)]`
        // structure.
        unsafe { &*ptr::from_ref(me).cast::<WsmuxSoftc>() }
    }

    /// `sc_displaydv`: `sc_base.me_dispdv`.
    pub fn displaydv(&self) -> Option<&Device> {
        self.sc_base.dispdv()
    }
}

/// `wsevsrc_open(me, evp)`.
pub fn wsevsrc_open(me: &Wsevsrc, evp: &Wseventvar) -> Result<(), Errno> {
    (me.ops().dopen)(me, evp)
}

/// `wsevsrc_close(me)`.
pub fn wsevsrc_close(me: &Wsevsrc) -> Result<(), Errno> {
    (me.ops().dclose)(me)
}

/// `wsevsrc_ioctl(me, cmd, data, flag, p)`.
pub fn wsevsrc_ioctl(
    me: &Wsevsrc,
    cmd: u64,
    data: &mut [u8],
    flag: i32,
    p: Option<&Proc>,
) -> Result<(), Errno> {
    (me.ops().dioctl)(&me.me_dv, cmd, data, flag, p)
}

/// `wsevsrc_display_ioctl(me, cmd, data, flag, p)`: `Ok(false)` for the C's -1. A source
/// without `ddispioctl` (the C would call NULL) answers "not mine".
pub fn wsevsrc_display_ioctl(
    me: &Wsevsrc,
    cmd: u64,
    data: &mut [u8],
    flag: i32,
    p: Option<&Proc>,
) -> Result<bool, Errno> {
    match me.ops().ddispioctl {
        Some(f) => f(&me.me_dv, cmd, data, flag, p),
        None => Ok(false),
    }
}

/// `wsevsrc_set_display(me, arg)`. A source without `dsetdisplay` (the C would call NULL)
/// answers `ENODEV`.
pub fn wsevsrc_set_display(me: &Wsevsrc, arg: Option<&Device>) -> Result<(), Errno> {
    match me.ops().dsetdisplay {
        Some(f) => f(&me.me_dv, arg),
        None => Err(Errno::ENODEV),
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sources_start_with_their_device() {
        assert_eq!(core::mem::offset_of!(Wsevsrc, me_dv), 0);
        assert_eq!(core::mem::offset_of!(WsmuxSoftc, sc_base), 0);
        assert_eq!(WSMOUSEDEVCF_MUX, 0);
    }
}
/* </TESTS> */
