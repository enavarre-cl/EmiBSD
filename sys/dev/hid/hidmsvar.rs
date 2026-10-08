/*	$OpenBSD: hidmsvar.h,v 1.3 2023/08/12 20:47:06 miod Exp $ */
/*	$NetBSD: ums.c,v 1.60 2003/03/11 16:44:00 augustss Exp $	*/
/* <LICENSES> */
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
//! `<dev/hid/hidmsvar.h>`: the state of one HID mouse, tablet or touch panel, a member of its
//! bus driver's softc (`ums(4)`, `uwacom(4)`), and its quirk flags.
//!
//! Upstream: sys/dev/hid/hidmsvar.h @ 3ce1f3f79392
//!
//! The functions are `hidms.rs`'s (`hidms_attach`, `hidms_input`, ...): the header's
//! prototypes belong to the `.c` file.
//!
//! ## Deviations
//! - Every member of [`Hidms`] is a `Cell` (the members change under the kernel lock, from
//!   the interrupt that delivers reports, the `wsmouse` entry points and the attach), as
//!   [`Hidkbd`](crate::dev::hid::hidkbd::Hidkbd) does; all-zero is a valid value, so a bus
//!   driver embeds one in its softc. `sc_device` and `sc_wsmousedev` are
//!   `Cell<Option<NonNull<Device>>>`; the location arrays are `Cell<[HidLocation; N]>`.

use core::cell::Cell;
use core::ptr::NonNull;

use crate::dev::hid::hid::HidLocation;
use crate::sys::device::Device;

/// `MAX_BUTTONS`: must not exceed size of `sc_buttons`.
pub const MAX_BUTTONS: usize = 31;

/// `struct tsscale`: the touch panel calibration (`WSMOUSEIO_SCALIBCOORDS`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tsscale {
    /// `minx`.
    pub minx: i32,
    /// `maxx`.
    pub maxx: i32,
    /// `miny`.
    pub miny: i32,
    /// `maxy`.
    pub maxy: i32,
    /// `minz`.
    pub minz: i32,
    /// `maxz`.
    pub maxz: i32,
    /// `minw`.
    pub minw: i32,
    /// `maxw`.
    pub maxw: i32,
    /// `swapxy`.
    pub swapxy: i32,
    /// `resx`.
    pub resx: i32,
    /// `resy`.
    pub resy: i32,
}

/// `HIDMS_SPUR_BUT_UP`: spurious button up events.
pub const HIDMS_SPUR_BUT_UP: u32 = 0x0001;
/// `HIDMS_Z`: Z direction available.
pub const HIDMS_Z: u32 = 0x0002;
/// `HIDMS_REVZ`: Z-axis is reversed.
pub const HIDMS_REVZ: u32 = 0x0004;
/// `HIDMS_W`: W direction available.
pub const HIDMS_W: u32 = 0x0008;
/// `HIDMS_REVW`: W-axis is reversed.
pub const HIDMS_REVW: u32 = 0x0010;
/// `HIDMS_LEADINGBYTE`: Unknown leading byte.
pub const HIDMS_LEADINGBYTE: u32 = 0x0020;
/// `HIDMS_ABSX`: X-axis is absolute.
pub const HIDMS_ABSX: u32 = 0x0040;
/// `HIDMS_ABSY`: Y-axis is absolute.
pub const HIDMS_ABSY: u32 = 0x0080;
/// `HIDMS_TIP`: Tip switch on a digitiser pen.
pub const HIDMS_TIP: u32 = 0x0100;
/// `HIDMS_BARREL`: Barrel switch on a digitiser pen.
pub const HIDMS_BARREL: u32 = 0x0200;
/// `HIDMS_ERASER`: Eraser switch on a digitiser pen.
pub const HIDMS_ERASER: u32 = 0x0400;
/// `HIDMS_MS_BAD_CLASS`: Mouse doesn't identify properly.
pub const HIDMS_MS_BAD_CLASS: u32 = 0x0800;
/// `HIDMS_VENDOR_BUTTONS`: extra buttons in vendor page.
pub const HIDMS_VENDOR_BUTTONS: u32 = 0x1000;
/// `HIDMS_SEC_BARREL`: Secondary Barrel switch on a digitiser pen.
pub const HIDMS_SEC_BARREL: u32 = 0x2000;
/// `HIDMS_WACOM_SETUP`: Requires Wacom-style setup.
pub const HIDMS_WACOM_SETUP: u32 = 0x4000;

/// `struct hidms`: the state of one HID mouse. All-zero is a valid value.
pub struct Hidms {
    /// `sc_device`: the bus driver's device.
    pub sc_device: Cell<Option<NonNull<Device>>>,
    /// `sc_wsmousedev`: the `wsmouse` child, if one attached.
    pub sc_wsmousedev: Cell<Option<NonNull<Device>>>,

    /// `sc_enabled`.
    pub sc_enabled: Cell<i32>,
    /// `sc_flags`: device configuration (`HIDMS_*`).
    pub sc_flags: Cell<u32>,

    /// `sc_num_buttons`.
    pub sc_num_buttons: Cell<i32>,
    /// `sc_buttons`: mouse button status.
    pub sc_buttons: Cell<u32>,

    /// `sc_num_pad_buttons`: Wacom-specific field.
    pub sc_num_pad_buttons: Cell<i32>,
    /// `sc_num_stylus_buttons`: Wacom-specific field.
    pub sc_num_stylus_buttons: Cell<i32>,

    /// `sc_loc_x`: locator of X.
    pub sc_loc_x: Cell<HidLocation>,
    /// `sc_loc_y`: locator of Y.
    pub sc_loc_y: Cell<HidLocation>,
    /// `sc_loc_z`: locator of Z.
    pub sc_loc_z: Cell<HidLocation>,
    /// `sc_loc_w`: locator of W.
    pub sc_loc_w: Cell<HidLocation>,
    /// `sc_loc_btn`: locators of the buttons.
    pub sc_loc_btn: Cell<[HidLocation; MAX_BUTTONS]>,

    /// `sc_tsscale`.
    pub sc_tsscale: Cell<Tsscale>,
    /// `sc_rawmode`.
    pub sc_rawmode: Cell<i32>,
}

impl Hidms {
    /// A mouse in the all-zero state, as a zeroed softc has it.
    pub const fn new() -> Self {
        let loc = HidLocation {
            size: 0,
            count: 0,
            pos: 0,
        };
        Self {
            sc_device: Cell::new(None),
            sc_wsmousedev: Cell::new(None),
            sc_enabled: Cell::new(0),
            sc_flags: Cell::new(0),
            sc_num_buttons: Cell::new(0),
            sc_buttons: Cell::new(0),
            sc_num_pad_buttons: Cell::new(0),
            sc_num_stylus_buttons: Cell::new(0),
            sc_loc_x: Cell::new(loc),
            sc_loc_y: Cell::new(loc),
            sc_loc_z: Cell::new(loc),
            sc_loc_w: Cell::new(loc),
            sc_loc_btn: Cell::new([loc; MAX_BUTTONS]),
            sc_tsscale: Cell::new(Tsscale {
                minx: 0,
                maxx: 0,
                miny: 0,
                maxy: 0,
                minz: 0,
                maxz: 0,
                minw: 0,
                maxw: 0,
                swapxy: 0,
                resx: 0,
                resy: 0,
            }),
            sc_rawmode: Cell::new(0),
        }
    }

    /// `ms->sc_loc_btn[i]`.
    pub fn loc_btn(&self, i: usize) -> HidLocation {
        let a = self.sc_loc_btn.get();
        a.get(i).copied().unwrap_or_default()
    }

    /// `ms->sc_loc_btn[i] = loc`; nothing past `MAX_BUTTONS`.
    pub fn set_loc_btn(&self, i: usize, loc: HidLocation) {
        let mut a = self.sc_loc_btn.get();
        if let Some(e) = a.get_mut(i) {
            *e = loc;
            self.sc_loc_btn.set(a);
        }
    }

    /// Change `sc_loc_btn[i]` with `f`.
    pub fn update_loc_btn(&self, i: usize, f: impl FnOnce(&mut HidLocation)) {
        let mut l = self.loc_btn(i);
        f(&mut l);
        self.set_loc_btn(i, l);
    }

    /// Change `sc_loc_x` with `f`.
    pub fn update_loc_x(&self, f: impl FnOnce(&mut HidLocation)) {
        let mut l = self.sc_loc_x.get();
        f(&mut l);
        self.sc_loc_x.set(l);
    }

    /// Change `sc_loc_y` with `f`.
    pub fn update_loc_y(&self, f: impl FnOnce(&mut HidLocation)) {
        let mut l = self.sc_loc_y.get();
        f(&mut l);
        self.sc_loc_y.set(l);
    }

    /// Change `sc_loc_z` with `f`.
    pub fn update_loc_z(&self, f: impl FnOnce(&mut HidLocation)) {
        let mut l = self.sc_loc_z.get();
        f(&mut l);
        self.sc_loc_z.set(l);
    }

    /// Change `sc_tsscale` with `f`.
    pub fn update_tsscale(&self, f: impl FnOnce(&mut Tsscale)) {
        let mut t = self.sc_tsscale.get();
        f(&mut t);
        self.sc_tsscale.set(t);
    }

    /// `ms->sc_flags |= f`.
    pub fn flags_or(&self, f: u32) {
        self.sc_flags.set(self.sc_flags.get() | f);
    }
}

impl Default for Hidms {
    fn default() -> Self {
        Self::new()
    }
}
/* </CODE> */
