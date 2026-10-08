/* $OpenBSD: wsmousevar.h,v 1.15 2017/06/18 13:21:48 bru Exp $ */
/* $NetBSD: wsmousevar.h,v 1.4 2000/01/08 02:57:24 takemura Exp $ */
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

//! `<dev/wscons/wsmousevar.h>`: the interface between mouse drivers and `wsmouse(4)`.
//!
//! Upstream: sys/dev/wscons/wsmousevar.h @ 3ce1f3f79392
//!
//! A mouse driver (`ums(4)`, `pms(4)`, a touchpad driver; M15) attaches a `wsmouse` child
//! with a [`WsmousedevAttachArgs`]: its [`WsmouseAccessops`] and the cookie they are called
//! with. It reports input through the functions of `wsmouse.rs`: button states
//! (`wsmouse_buttons`), relative motion (`wsmouse_motion`), absolute coordinates
//! (`wsmouse_position`), single-touch pressure and contacts (`wsmouse_touch`), slot-based
//! (`wsmouse_mtstate`) or point-based (`wsmouse_mtframe`) multitouch, or single values
//! (`wsmouse_set`), and ends each report with `wsmouse_input_sync`, which turns the changes
//! into `wscons_event`s. A driver describes its hardware in the [`Wsmousehw`] of
//! `wsmouse_get_hw` and calls `wsmouse_configure` once.
//!
//! ## Deviations
//! - The access operations are `fn` pointers over the untyped cookie (`wskbdvar.rs`);
//!   `ioctl` takes the kernel copy of the argument as bytes and an `Option<&Proc>` (a mux
//!   passes NULL), and returns `Ok(true)` where the C returns 0, `Ok(false)` where it returns
//!   -1 (not the driver's ioctl) and `Err` for an errno.
//! - The `wsmousedevcf_mux` locator macro is a function of the `cfdata`; a missing locator
//!   reads as the default of `define wsmousedev {[mux = 0]}` in `sys/conf/files`.
//! - `WSMOUSE_INPUT` and `WSMOUSE_TOUCH` are `macro_rules!` of the same names, exported at
//!   the crate root as `unported!` is (their lowercase names would be `wsmouse_touch`'s); `WSMOUSE_IS_MT_CODE` is the `const fn`
//!   [`wsmouse_is_mt_code`].
//! - `enum wsmouseval` and `enum wsmousehw_type` are `i32` constants of the C's names
//!   ([`Wsmouseval`], [`WsmousehwType`]), as `int`s they are in C.
//! - The prototypes belong to `wsmouse.c`; they are in `wsmouse.rs`.

use core::ffi::c_void;

use crate::sys::device::Cfdata;
use crate::sys::errno::Errno;
use crate::sys::proc::Proc;

/// The type of `wsmouse_accessops`' `ioctl`: `Ok(true)` handled, `Ok(false)` not the
/// driver's.
pub type WsmouseIoctlFn = fn(
    v: *mut c_void,
    cmd: u64,
    data: &mut [u8],
    flag: i32,
    p: Option<&Proc>,
) -> Result<bool, Errno>;

/// `struct wsmouse_accessops`: mouse access functions (must be provided by all mice).
///
/// There is a "void *" cookie provided by the mouse driver associated with these functions,
/// which is passed to them when they are invoked.
#[derive(Clone, Copy)]
pub struct WsmouseAccessops {
    /// `enable`: an errno, 0 for success.
    pub enable: fn(v: *mut c_void) -> i32,
    /// `ioctl`.
    pub ioctl: WsmouseIoctlFn,
    /// `disable`.
    pub disable: fn(v: *mut c_void),
}

/// `struct wsmousedev_attach_args`: attachment information provided by wsmousedev devices
/// when attaching wsmouse units.
pub struct WsmousedevAttachArgs {
    /// `accessops`: access ops.
    pub accessops: &'static WsmouseAccessops,
    /// `accesscookie`: access cookie.
    pub accesscookie: *mut c_void,
}

/// `wsmousedevcf_mux`: the mux the mouse attaches to (`mux = 0` by default).
pub fn wsmousedevcf_mux(cf: &Cfdata) -> i64 {
    cf.cf_loc
        .get(crate::dev::wscons::wsmuxvar::WSMOUSEDEVCF_MUX)
        .copied()
        .unwrap_or(0)
}

/// `WSMOUSE_INPUT(sc_wsmousedev, btns, dx, dy, dz, dw)`: process standard mouse input.
#[macro_export]
macro_rules! WSMOUSE_INPUT {
    ($dev:expr, $btns:expr, $dx:expr, $dy:expr, $dz:expr, $dw:expr) => {{
        let dev = $dev;
        $crate::dev::wscons::wsmouse::wsmouse_buttons(dev, $btns);
        $crate::dev::wscons::wsmouse::wsmouse_motion(dev, $dx, $dy, $dz, $dw);
        $crate::dev::wscons::wsmouse::wsmouse_input_sync(dev);
    }};
}

/// `WSMOUSE_TOUCH(sc_wsmousedev, btns, x, y, pressure, contacts)`: process standard
/// touchpad input.
#[macro_export]
macro_rules! WSMOUSE_TOUCH {
    ($dev:expr, $btns:expr, $x:expr, $y:expr, $pressure:expr, $contacts:expr) => {{
        let dev = $dev;
        $crate::dev::wscons::wsmouse::wsmouse_buttons(dev, $btns);
        $crate::dev::wscons::wsmouse::wsmouse_position(dev, $x, $y);
        $crate::dev::wscons::wsmouse::wsmouse_touch(dev, $pressure, $contacts);
        $crate::dev::wscons::wsmouse::wsmouse_input_sync(dev);
    }};
}

/// `WSMOUSE_DEFAULT_PRESSURE`: drivers for touchpads that don't report pressure values can
/// pass it to `wsmouse_touch` or `wsmouse_mtstate`. Use a synaptics-compatible value.
///
/// A pressure value of 0 signals that a touch has been released (coordinates will be
/// ignored). Based on its pressure argument, `wsmouse_touch` will normalize the contact count
/// (drivers for touch devices that don't recognize multiple contacts can always pass 0 as
/// contact count to `wsmouse_touch`).
pub const WSMOUSE_DEFAULT_PRESSURE: i32 = 45;

/// `enum wsmouseval`: type codes for `wsmouse_set`. `REL_X/Y`, `MT_REL_X/Y`, and
/// `TOUCH_WIDTH` cannot be reported by other functions. Please note that `REL_X/Y` values
/// are deltas to be applied to the absolute coordinates and don't represent "pure" relative
/// motion.
pub type Wsmouseval = i32;
/// `WSMOUSE_REL_X`.
pub const WSMOUSE_REL_X: Wsmouseval = 0;
/// `WSMOUSE_ABS_X`.
pub const WSMOUSE_ABS_X: Wsmouseval = 1;
/// `WSMOUSE_REL_Y`.
pub const WSMOUSE_REL_Y: Wsmouseval = 2;
/// `WSMOUSE_ABS_Y`.
pub const WSMOUSE_ABS_Y: Wsmouseval = 3;
/// `WSMOUSE_PRESSURE`.
pub const WSMOUSE_PRESSURE: Wsmouseval = 4;
/// `WSMOUSE_CONTACTS`.
pub const WSMOUSE_CONTACTS: Wsmouseval = 5;
/// `WSMOUSE_TOUCH_WIDTH`.
pub const WSMOUSE_TOUCH_WIDTH: Wsmouseval = 6;
/// `WSMOUSE_MT_REL_X`.
pub const WSMOUSE_MT_REL_X: Wsmouseval = 7;
/// `WSMOUSE_MT_ABS_X`.
pub const WSMOUSE_MT_ABS_X: Wsmouseval = 8;
/// `WSMOUSE_MT_REL_Y`.
pub const WSMOUSE_MT_REL_Y: Wsmouseval = 9;
/// `WSMOUSE_MT_ABS_Y`.
pub const WSMOUSE_MT_ABS_Y: Wsmouseval = 10;
/// `WSMOUSE_MT_PRESSURE`.
pub const WSMOUSE_MT_PRESSURE: Wsmouseval = 11;

/// `WSMOUSE_IS_MT_CODE(code)`.
pub const fn wsmouse_is_mt_code(code: Wsmouseval) -> bool {
    code >= WSMOUSE_MT_REL_X && code <= WSMOUSE_MT_PRESSURE
}

/// `struct mtpoint`: a point of a multitouch frame (`wsmouse_mtframe`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mtpoint {
    /// `x`.
    pub x: i32,
    /// `y`.
    pub y: i32,
    /// `pressure`.
    pub pressure: i32,
    /// `slot`: an output field, set by `wsmouse_mtframe`.
    pub slot: i32,
}

/// `WSMOUSE_MT_SLOTS_MAX`.
pub const WSMOUSE_MT_SLOTS_MAX: i32 = 10;
/// `WSMOUSE_MT_INIT_TRACKING`.
pub const WSMOUSE_MT_INIT_TRACKING: i32 = 1;

/// `enum wsmousehw_type`.
pub type WsmousehwType = i32;
/// `WSMOUSEHW_RAW`.
pub const WSMOUSEHW_RAW: WsmousehwType = 0;
/// `WSMOUSEHW_MOUSE`.
pub const WSMOUSEHW_MOUSE: WsmousehwType = 1;
/// `WSMOUSEHW_TOUCHPAD`.
pub const WSMOUSEHW_TOUCHPAD: WsmousehwType = 2;
/// `WSMOUSEHW_CLICKPAD`.
pub const WSMOUSEHW_CLICKPAD: WsmousehwType = 3;
/// `WSMOUSEHW_TPANEL`.
pub const WSMOUSEHW_TPANEL: WsmousehwType = 4;

/// `WSMOUSEHW_LR_DOWN` (`wsmousehw.flags`): invert Y-coordinates.
pub const WSMOUSEHW_LR_DOWN: i32 = 1 << 0;
/// `WSMOUSEHW_MT_TRACKING`: allocate the buffers for `wsmouse_mtframe()`.
pub const WSMOUSEHW_MT_TRACKING: i32 = 1 << 1;

/// `struct wsmousehw`: the more or less minimal hardware description for the default
/// configuration.
///
/// Drivers that report coordinates with a downward orientation must set the flag
/// `WSMOUSEHW_LR_DOWN`. Drivers for MT hardware must provide the number of slots. If they use
/// `wsmouse_mtframe()`, `WSMOUSEHW_MT_TRACKING` must be set.
///
/// The resolution values are optional.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Wsmousehw {
    /// `type`: `WSMOUSE_TYPE_*`, cf. `wsconsio.h`.
    pub type_: i32,
    /// `hw_type`: `WSMOUSEHW_*`.
    pub hw_type: WsmousehwType,
    /// `x_min`.
    pub x_min: i32,
    /// `x_max`.
    pub x_max: i32,
    /// `y_min`.
    pub y_min: i32,
    /// `y_max`.
    pub y_max: i32,
    /// `h_res`.
    pub h_res: i32,
    /// `v_res`.
    pub v_res: i32,
    /// `flags`: `WSMOUSEHW_*`.
    pub flags: i32,
    /// `mt_slots`.
    pub mt_slots: i32,
    /// `contacts_max`: inclusive (not needed for MT touchpads).
    pub contacts_max: i32,
}

#[cfg(test)]
mod tests;
