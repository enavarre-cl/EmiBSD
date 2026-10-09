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
//! hidms: the bus-independent HID mouse, tablet and touch panel code `ums(4)` and
//! `uwacom(4)` wrap: it reads a report descriptor into the locations of the axes and buttons
//! (`hidms_setup`), offers a `wsmouse(4)` child (`hidms_attach`) and turns the reports the
//! bus driver hands it into `wsmouse` input (`hidms_input`), relative or absolute.
//!
//! Upstream: sys/dev/hid/hidms.c @ 3ce1f3f79392
//!
//! `hidms_setup` finds X, Y, the wheel (WHEEL, TWHEEL, Z, in that order), the W axis (a
//! second Z or AC Pan), up to 31 buttons, the digitiser switches (tip, eraser, barrel), the
//! quirk-driven layouts of the broken Microsoft mice and the bounds of absolute axes;
//! `HIDMS_WACOM_SETUP` makes it parse a Wacom tablet's stylus and pad collections instead.
//! `hidms_input` reports absolute X and Y with `wsmouse_set` and the rest with
//! `WSMOUSE_INPUT`.
//!
//! ## Deviations
//! - `HIDMS_DEBUG` is not in GENERIC: the `DPRINTF`s and the descriptor dump of
//!   `hidms_attach` are absent. The `notyet` Wacom battery collection is not carried.
//! - The arrays of locations `hidms_wacom_setup` fills (`loc_pad_btn`, `loc_stylus_btn`)
//!   start zeroed, where the C leaves them uninitialised. The C copies the pad buttons with
//!   the index of the combined button list into `loc_pad_btn`; that is kept as it is.
//! - `hidms_setup` and `hidms_wacom_setup` return `Result<(), Errno>` (`ENXIO` for the C's
//!   `ENXIO`); `hidms_enable` returns the errno as an `i32` like the C. `hidms_ioctl`
//!   returns `Ok(false)` where the C returns -1, and takes the argument as bytes
//!   (`ioctl_arg`, `ioctl_ret`).
//! - `hidms_input` gets the report as a slice (see `uhidev.rs`); the leading byte of a
//!   `HIDMS_LEADINGBYTE` report is skipped by slicing, and an empty report with that quirk is
//!   dropped (the C reads a byte past it).
//! - The arithmetic of the axes wraps where the C overflows `int`.
//! - The `wsmouse` access operations are `fn` pointers over the bus driver's device as the
//!   untyped cookie (`wsmousevar.rs`).

use core::ffi::c_void;
use core::ptr::{self, NonNull};

use crate::dev::hid::hid::{
    HCOLL_APPLICATION, HCOLL_PHYSICAL, HIO_CONST, HIO_RELATIVE, HUC_AC_PAN, HUD_BARREL_SWITCH,
    HUD_DIGITIZER, HUD_ERASER, HUD_IN_RANGE, HUD_QUALITY, HUD_SECONDARY_BARREL_SWITCH, HUD_STYLUS,
    HUD_TABLET_FKEYS, HUD_TIP_PRESSURE, HUD_TIP_SWITCH, HUD_WACOM_DISTANCE,
    HUD_WACOM_PAD_BUTTONS00, HUD_WACOM_X, HUD_WACOM_Y, HUG_TWHEEL, HUG_WHEEL, HUG_X, HUG_Y, HUG_Z,
    HUP_BUTTON, HUP_CONSUMER, HUP_DIGITIZERS, HUP_GENERIC_DESKTOP, HUP_MICROSOFT, HUP_WACOM,
    HidData, HidItem, HidLocation, hid_end_parse, hid_endcollection, hid_get_collection_data,
    hid_get_data, hid_get_item, hid_get_usage, hid_get_usage_page, hid_input, hid_locate,
    hid_start_parse, hid_usage2,
};
use crate::dev::hid::hidmsvar::{
    HIDMS_ABSX, HIDMS_ABSY, HIDMS_BARREL, HIDMS_ERASER, HIDMS_LEADINGBYTE, HIDMS_MS_BAD_CLASS,
    HIDMS_REVW, HIDMS_REVZ, HIDMS_SEC_BARREL, HIDMS_SPUR_BUT_UP, HIDMS_TIP, HIDMS_VENDOR_BUTTONS,
    HIDMS_W, HIDMS_WACOM_SETUP, HIDMS_Z, Hidms, MAX_BUTTONS,
};
use crate::dev::wscons::wsconsio::{
    WSMOUSE_TYPE_TPANEL, WSMOUSEIO_GCALIBCOORDS, WSMOUSEIO_GTYPE, WSMOUSEIO_SCALIBCOORDS,
    WsmouseCalibcoords,
};
use crate::dev::wscons::wsmouse::{wsmouse_set, wsmousedevprint};
use crate::dev::wscons::wsmousevar::{
    WSMOUSE_ABS_X, WSMOUSE_ABS_Y, WsmouseAccessops, WsmousedevAttachArgs,
};
use crate::kern::subr_autoconf::{config_detach, config_found};
use crate::kprintf;
use crate::machine::intr::{spltty, splx};
use crate::sys::device::Device;
use crate::sys::errno::Errno;
use crate::sys::ioctl::{ioctl_arg, ioctl_ret};
use crate::sys::proc::Proc;

/// `MOUSE_FLAGS_MASK`.
const MOUSE_FLAGS_MASK: u32 = HIO_CONST | HIO_RELATIVE;

/// `HIDMS_BUT(i)`: the wsmouse button of HID button `i` (the middle and right ones swap).
const fn hidms_but(i: u32) -> u32 {
    if i == 1 || i == 2 { 3 - i } else { i }
}

/// `NOTMOUSE(f)`: the flags of an input that is not relative data.
const fn notmouse(f: u32) -> bool {
    (f & MOUSE_FLAGS_MASK) != HIO_RELATIVE
}

/// `hid_locate` into one of the softc's locator cells.
fn locate(
    desc: &[u8],
    usage: u32,
    id: u8,
    cell: &core::cell::Cell<HidLocation>,
    flags: &mut u32,
) -> bool {
    let mut loc = cell.get();
    let r = hid_locate(desc, usage, id, hid_input, Some(&mut loc), Some(flags));
    cell.set(loc);
    r
}

/// `hidms_stylus_hid_parse`: the usages of a Wacom stylus collection: its switches become
/// buttons (`loc_stylus_btn`), its axes X, Y, Z (pressure) and W (distance).
pub fn hidms_stylus_hid_parse(
    ms: &Hidms,
    d: &mut HidData<'_>,
    loc_stylus_btn: &mut [HidLocation; MAX_BUTTONS],
) {
    let mut h = HidItem::default();

    while hid_get_item(d, &mut h) {
        if h.kind == hid_endcollection {
            break;
        }
        if h.kind != hid_input || (h.flags & HIO_CONST) != 0 {
            continue;
        }
        // All the possible stylus reported usages go here
        let mut add_button = |flag: u32| {
            let n = ms.sc_num_stylus_buttons.get();
            if n as usize >= MAX_BUTTONS {
                return;
            }
            loc_stylus_btn[n as usize] = h.loc;
            ms.sc_num_stylus_buttons.set(n + 1);
            if flag != 0 {
                ms.flags_or(flag);
            }
        };
        let page = HUP_WACOM | HUP_DIGITIZERS;
        match h.usage {
            // Buttons
            u if u == hid_usage2(page, HUD_TIP_SWITCH) => add_button(HIDMS_TIP),
            u if u == hid_usage2(page, HUD_BARREL_SWITCH) => add_button(HIDMS_BARREL),
            u if u == hid_usage2(page, HUD_SECONDARY_BARREL_SWITCH) => add_button(HIDMS_SEC_BARREL),
            u if u == hid_usage2(page, HUD_IN_RANGE) => add_button(0),
            u if u == hid_usage2(page, HUD_QUALITY) => add_button(0),
            // Axes
            u if u == hid_usage2(page, HUD_WACOM_X) => {
                ms.sc_loc_x.set(h.loc);
                ms.update_tsscale(|t| {
                    t.minx = h.logical_minimum;
                    t.maxx = h.logical_maximum;
                });
                ms.flags_or(HIDMS_ABSX);
            }
            u if u == hid_usage2(page, HUD_WACOM_Y) => {
                ms.sc_loc_y.set(h.loc);
                ms.update_tsscale(|t| {
                    t.miny = h.logical_minimum;
                    t.maxy = h.logical_maximum;
                });
                ms.flags_or(HIDMS_ABSY);
            }
            u if u == hid_usage2(page, HUD_TIP_PRESSURE) => {
                ms.sc_loc_z.set(h.loc);
                ms.update_tsscale(|t| {
                    t.minz = h.logical_minimum;
                    t.maxz = h.logical_maximum;
                });
                ms.flags_or(HIDMS_Z);
            }
            u if u == hid_usage2(page, HUD_WACOM_DISTANCE) => {
                ms.sc_loc_w.set(h.loc);
                ms.update_tsscale(|t| {
                    t.minw = h.logical_minimum;
                    t.maxw = h.logical_maximum;
                });
                ms.flags_or(HIDMS_W);
            }
            _ => {}
        }
    }
}

/// `hidms_pad_buttons_hid_parse`: the pad buttons of a Wacom tablet's function keys
/// collection (constant inputs whose usages count up from `HUD_WACOM_PAD_BUTTONS00`).
pub fn hidms_pad_buttons_hid_parse(
    ms: &Hidms,
    d: &mut HidData<'_>,
    loc_pad_btn: &mut [HidLocation; MAX_BUTTONS],
) {
    let mut h = HidItem::default();

    while hid_get_item(d, &mut h) {
        if h.kind == hid_endcollection {
            break;
        }
        if h.kind == hid_input
            && (h.flags & HIO_CONST) != 0
            && h.usage
                == hid_usage2(
                    HUP_WACOM | HUP_DIGITIZERS,
                    HUD_WACOM_PAD_BUTTONS00 | ms.sc_num_pad_buttons.get() as u32,
                )
        {
            let n = ms.sc_num_pad_buttons.get();
            if n as usize >= MAX_BUTTONS {
                break;
            }
            loc_pad_btn[n as usize] = h.loc;
            ms.sc_num_pad_buttons.set(n + 1);
        }
    }
}

/// `hidms_wacom_setup`: the setup of a Wacom tablet: its stylus and tablet keys collections.
pub fn hidms_wacom_setup(_self: &Device, ms: &Hidms, desc: &[u8]) -> Result<(), Errno> {
    let mut loc_pad_btn = [HidLocation::default(); MAX_BUTTONS];
    let mut loc_stylus_btn = [HidLocation::default(); MAX_BUTTONS];

    ms.sc_flags.set(0);

    // Set x,y,z and w to zero by default
    ms.update_loc_x(|l| l.size = 0);
    ms.update_loc_y(|l| l.size = 0);
    ms.update_loc_z(|l| l.size = 0);
    let mut w = ms.sc_loc_w.get();
    w.size = 0;
    ms.sc_loc_w.set(w);

    if let Some(hd) = hid_get_collection_data(
        desc,
        hid_usage2(HUP_WACOM | HUP_DIGITIZERS, HUD_DIGITIZER) as i32,
        HCOLL_APPLICATION,
    ) {
        hid_end_parse(hd);
        if let Some(mut hd) = hid_get_collection_data(
            desc,
            hid_usage2(HUP_WACOM | HUP_DIGITIZERS, HUD_STYLUS) as i32,
            HCOLL_PHYSICAL,
        ) {
            hidms_stylus_hid_parse(ms, &mut hd, &mut loc_stylus_btn);
            hid_end_parse(hd);
        }
        if let Some(mut hd) = hid_get_collection_data(
            desc,
            hid_usage2(HUP_WACOM | HUP_DIGITIZERS, HUD_TABLET_FKEYS) as i32,
            HCOLL_PHYSICAL,
        ) {
            hidms_pad_buttons_hid_parse(ms, &mut hd, &mut loc_pad_btn);
            hid_end_parse(hd);
        }
        // Ignore the device config, it's not really needed;
        // Ignore the usage 0x10AC which is the debug collection, and
        // ignore firmware collection and other collections for now.
    }

    // Map the pad and stylus buttons to mouse buttons
    let nstylus = ms.sc_num_stylus_buttons.get();
    let mut i = 0;
    while i < nstylus {
        ms.set_loc_btn(i as usize, loc_stylus_btn[i as usize]);
        i += 1;
    }
    if ms.sc_num_pad_buttons.get() + nstylus >= MAX_BUTTONS as i32 {
        ms.sc_num_pad_buttons.set(MAX_BUTTONS as i32 - nstylus);
    }
    while i < ms.sc_num_pad_buttons.get() + nstylus {
        ms.set_loc_btn(i as usize, loc_pad_btn[i as usize]);
        i += 1;
    }
    ms.sc_num_buttons.set(i);
    Ok(())
}

/// `hidms_setup`: read the report descriptor `desc` for report ID `id` into the locations of
/// `ms`; `quirks` are the `HIDMS_*` flags the bus driver derives from the device's quirks.
pub fn hidms_setup(
    self_: &Device,
    ms: &Hidms,
    quirks: u32,
    id: i32,
    desc: &[u8],
) -> Result<(), Errno> {
    let id = id as u8;
    let mut flags: u32 = 0;

    ms.sc_device.set(Some(NonNull::from(self_)));
    ms.sc_rawmode.set(1);

    ms.sc_flags.set(quirks);

    // We are setting up a Wacom tablet, not a regular mouse
    if quirks & HIDMS_WACOM_SETUP != 0 {
        return hidms_wacom_setup(self_, ms, desc);
    }

    locate(
        desc,
        hid_usage2(HUP_GENERIC_DESKTOP, HUG_X),
        id,
        &ms.sc_loc_x,
        &mut flags,
    );

    match flags & MOUSE_FLAGS_MASK {
        0 => ms.flags_or(HIDMS_ABSX),
        HIO_RELATIVE => {}
        _ => {
            kprintf!(
                "\n{}: X report {:#06x} not supported\n",
                self_.xname(),
                flags
            );
            return Err(Errno::ENXIO);
        }
    }

    locate(
        desc,
        hid_usage2(HUP_GENERIC_DESKTOP, HUG_Y),
        id,
        &ms.sc_loc_y,
        &mut flags,
    );

    match flags & MOUSE_FLAGS_MASK {
        0 => ms.flags_or(HIDMS_ABSY),
        HIO_RELATIVE => {}
        _ => {
            kprintf!(
                "\n{}: Y report {:#06x} not supported\n",
                self_.xname(),
                flags
            );
            return Err(Errno::ENXIO);
        }
    }

    // Try to guess the Z activator: check WHEEL, TWHEEL, and Z, in that order.
    let wheel = locate(
        desc,
        hid_usage2(HUP_GENERIC_DESKTOP, HUG_WHEEL),
        id,
        &ms.sc_loc_z,
        &mut flags,
    );
    let twheel = if !wheel {
        locate(
            desc,
            hid_usage2(HUP_GENERIC_DESKTOP, HUG_TWHEEL),
            id,
            &ms.sc_loc_z,
            &mut flags,
        )
    } else {
        false
    };

    if wheel || twheel {
        if notmouse(flags) {
            // Bad Z coord, ignore it
            ms.update_loc_z(|l| l.size = 0);
        } else {
            ms.flags_or(HIDMS_Z);
            // Wheels need the Z axis reversed.
            ms.sc_flags.set(ms.sc_flags.get() ^ HIDMS_REVZ);
        }
        // We might have both a wheel and Z direction; in this case, report the Z direction
        // on the W axis.
        //
        // Otherwise, check for a W direction as an AC Pan input used on some newer mice.
        if locate(
            desc,
            hid_usage2(HUP_GENERIC_DESKTOP, HUG_Z),
            id,
            &ms.sc_loc_w,
            &mut flags,
        ) {
            if notmouse(flags) {
                // Bad Z coord, ignore it
                let mut w = ms.sc_loc_w.get();
                w.size = 0;
                ms.sc_loc_w.set(w);
            } else {
                ms.flags_or(HIDMS_W);
            }
        } else if locate(
            desc,
            hid_usage2(HUP_CONSUMER, HUC_AC_PAN),
            id,
            &ms.sc_loc_w,
            &mut flags,
        ) {
            ms.flags_or(HIDMS_W);
        }
    } else if locate(
        desc,
        hid_usage2(HUP_GENERIC_DESKTOP, HUG_Z),
        id,
        &ms.sc_loc_z,
        &mut flags,
    ) {
        if notmouse(flags) {
            // Bad Z coord, ignore it
            ms.update_loc_z(|l| l.size = 0);
        } else {
            ms.flags_or(HIDMS_Z);
        }
    }

    // The Microsoft Wireless Intellimouse 2.0 reports its wheel using 0x0048 (I've called it
    // HUG_TWHEEL) and seems to expect us to know that the byte after the wheel is the tilt
    // axis. There are no other HID axis descriptors other than X, Y and TWHEEL, so we
    // report TWHEEL on the W axis.
    if twheel {
        let mut w = ms.sc_loc_z.get();
        w.pos += 8;
        ms.sc_loc_w.set(w);
        ms.flags_or(HIDMS_W | HIDMS_LEADINGBYTE);
        // Wheels need their axis reversed.
        ms.sc_flags.set(ms.sc_flags.get() ^ HIDMS_REVW);
    }

    // figure out the number of buttons
    let mut btn = ms.sc_loc_btn.get();
    let mut i = 1;
    while i <= MAX_BUTTONS {
        if !hid_locate(
            desc,
            hid_usage2(HUP_BUTTON, i as u32),
            id,
            hid_input,
            Some(&mut btn[i - 1]),
            None,
        ) {
            break;
        }
        i += 1;
    }
    let mut num_buttons = i - 1;

    // The Kensington Slimblade reports some of its buttons as binary inputs in the first
    // vendor usage page (0xff00). Add such inputs as buttons if the device has this quirk.
    if ms.sc_flags.get() & HIDMS_VENDOR_BUTTONS != 0 {
        let mut i = 1;
        while num_buttons < MAX_BUTTONS {
            if !hid_locate(
                desc,
                hid_usage2(HUP_MICROSOFT, i),
                id,
                hid_input,
                Some(&mut btn[num_buttons]),
                None,
            ) {
                break;
            }
            num_buttons += 1;
            i += 1;
        }
    }

    for (usage, flag) in [
        (HUD_TIP_SWITCH, HIDMS_TIP),
        (HUD_ERASER, HIDMS_ERASER),
        (HUD_BARREL_SWITCH, HIDMS_BARREL),
    ] {
        if num_buttons < MAX_BUTTONS
            && hid_locate(
                desc,
                hid_usage2(HUP_DIGITIZERS, usage),
                id,
                hid_input,
                Some(&mut btn[num_buttons]),
                None,
            )
        {
            ms.flags_or(flag);
            num_buttons += 1;
        }
    }
    ms.sc_loc_btn.set(btn);
    ms.sc_num_buttons.set(num_buttons as i32);

    // The Microsoft Wireless Notebook Optical Mouse seems to be in worse shape than the
    // Wireless Intellimouse 2.0, as its X, Y, wheel, and all of its other button positions
    // are all off. It also reports that it has two additional buttons and a tilt wheel.
    if ms.sc_flags.get() & HIDMS_MS_BAD_CLASS != 0 {
        // HIDMS_LEADINGBYTE cleared on purpose
        ms.sc_flags.set(HIDMS_Z | HIDMS_SPUR_BUT_UP);
        ms.sc_num_buttons.set(3);
        // XXX change sc_hdev isize to 5?
        // 1st byte of descriptor report contains garbage
        ms.update_loc_x(|l| l.pos = 16);
        ms.update_loc_y(|l| l.pos = 24);
        ms.update_loc_z(|l| l.pos = 32);
        ms.update_loc_btn(0, |l| l.pos = 8);
        ms.update_loc_btn(1, |l| l.pos = 9);
        ms.update_loc_btn(2, |l| l.pos = 10);
    }
    // Parse descriptors to get touch panel bounds
    let mut h = HidItem::default();
    let mut d = hid_start_parse(desc, hid_input);
    while hid_get_item(&mut d, &mut h) {
        if h.kind != hid_input || hid_get_usage_page(h.usage) != HUP_GENERIC_DESKTOP {
            continue;
        }
        match hid_get_usage(h.usage) {
            HUG_X if ms.sc_flags.get() & HIDMS_ABSX != 0 => {
                ms.update_tsscale(|t| {
                    t.minx = h.logical_minimum;
                    t.maxx = h.logical_maximum;
                });
            }
            HUG_Y if ms.sc_flags.get() & HIDMS_ABSY != 0 => {
                ms.update_tsscale(|t| {
                    t.miny = h.logical_minimum;
                    t.maxy = h.logical_maximum;
                });
            }
            _ => {}
        }
    }
    hid_end_parse(d);
    Ok(())
}

/// `hidms_attach`: print the mouse's capabilities and offer a `wsmouse` child with the bus
/// driver's access operations.
pub fn hidms_attach(ms: &Hidms, ops: &'static WsmouseAccessops) {
    let nb = ms.sc_num_buttons.get();
    let flags = ms.sc_flags.get();

    kprintf!(": {} button{}", nb, if nb == 1 { "" } else { "s" });
    match flags & (HIDMS_Z | HIDMS_W) {
        HIDMS_Z => {
            kprintf!(", Z dir");
        }
        HIDMS_W => {
            kprintf!(", W dir");
        }
        f if f == HIDMS_Z | HIDMS_W => {
            kprintf!(", Z and W dir");
        }
        _ => {}
    }

    if flags & HIDMS_TIP != 0 {
        kprintf!(", tip");
    }
    if flags & HIDMS_BARREL != 0 {
        kprintf!(", barrel");
    }
    if flags & HIDMS_ERASER != 0 {
        kprintf!(", eraser");
    }

    kprintf!("\n");

    let Some(dev) = ms.sc_device.get() else {
        return;
    };
    // SAFETY: `sc_device` is the bus driver's own device, set by `hidms_setup` (or the bus
    // driver); it is attached for as long as the mouse is.
    let dev = unsafe { dev.as_ref() };
    let mut a = WsmousedevAttachArgs {
        accessops: ops,
        accesscookie: ptr::from_ref(dev).cast_mut().cast(),
    };
    ms.sc_wsmousedev.set(config_found(
        dev,
        ptr::from_mut(&mut a).cast::<c_void>(),
        Some(wsmousedevprint),
    ));
}

/// `hidms_detach`: detach the `wsmouse` child.
pub fn hidms_detach(ms: &Hidms, flags: i32) -> Result<(), Errno> {
    let mut rv = Ok(());

    // No need to do reference counting of hidms, wsmouse has all the goo
    if let Some(wsm) = ms.sc_wsmousedev.get() {
        // SAFETY: `wsm` is the attached child `hidms_attach` stored; it is not used again
        // (`sc_wsmousedev` is cleared on success).
        rv = unsafe { config_detach(wsm, flags) };
        if rv.is_ok() {
            ms.sc_wsmousedev.set(None);
        }
    }

    rv
}

/// `hidms_input`: a report from the device (without its report ID): decode the axes and
/// buttons and hand them to `wsmouse`.
pub fn hidms_input(ms: &Hidms, data: &[u8]) {
    let mut buttons: u32 = 0;
    let mut data = data;
    let flags = ms.sc_flags.get();

    // The Microsoft Wireless Intellimouse 2.0 sends one extra leading byte of data compared
    // to most USB mice. This byte frequently switches from 0x01 (usual state) to 0x02. It
    // may be used to report non-standard events (such as battery life). However, at the
    // same time, it generates a left click event on the button byte, where there shouldn't
    // be any. We simply discard the packet in this case.
    //
    // This problem affects the MS Wireless Notebook Optical Mouse, too. However, the leading
    // byte for this mouse is normally 0x11, and the phantom mouse click occurs when it's
    // 0x14.
    if flags & HIDMS_LEADINGBYTE != 0 {
        let Some((&first, rest)) = data.split_first() else {
            return;
        };
        data = rest;
        if first == 0x02 {
            return;
        }
    } else if flags & HIDMS_SPUR_BUT_UP != 0 && matches!(data.first(), Some(0x14 | 0x15)) {
        return;
    }

    let mut dx = hid_get_data(data, &ms.sc_loc_x.get());
    let mut dy = hid_get_data(data, &ms.sc_loc_y.get()).wrapping_neg();
    let mut dz = hid_get_data(data, &ms.sc_loc_z.get());
    let mut dw = hid_get_data(data, &ms.sc_loc_w.get());

    if flags & HIDMS_ABSY != 0 {
        dy = dy.wrapping_neg();
    }
    if flags & HIDMS_REVZ != 0 {
        dz = dz.wrapping_neg();
    }
    if flags & HIDMS_REVW != 0 {
        dw = dw.wrapping_neg();
    }

    let ts = ms.sc_tsscale.get();
    let rawmode = ms.sc_rawmode.get() != 0;
    if ts.swapxy != 0 && !rawmode {
        core::mem::swap(&mut dx, &mut dy);
    }

    if !rawmode && ts.maxx.wrapping_sub(ts.minx) != 0 && ts.maxy.wrapping_sub(ts.miny) != 0 {
        // Scale down to the screen resolution.
        dx = dx.wrapping_sub(ts.minx).wrapping_mul(ts.resx) / ts.maxx.wrapping_sub(ts.minx);
        dy = dy.wrapping_sub(ts.miny).wrapping_mul(ts.resy) / ts.maxy.wrapping_sub(ts.miny);
    }

    for i in 0..ms.sc_num_buttons.get().max(0) as usize {
        if hid_get_data(data, &ms.loc_btn(i)) != 0 {
            buttons |= 1 << hidms_but(i as u32);
        }
    }

    if dx != 0 || dy != 0 || dz != 0 || dw != 0 || buttons != ms.sc_buttons.get() {
        let released = ms.sc_buttons.get() != 0 && buttons == 0;
        ms.sc_buttons.set(buttons);
        if let Some(wsm) = ms.sc_wsmousedev.get() {
            // SAFETY: the `wsmouse` child `hidms_attach` stored; `hidms_detach` clears
            // `sc_wsmousedev` only after detaching it, under the kernel lock.
            let wsm = unsafe { wsm.as_ref() };
            let s = spltty();
            // A polled touchscreen reports finger-up as a zeroed packet; emitting its (0,0)
            // as an absolute position would snap the pointer to the corner. On a button
            // release edge, hold the last position instead.
            if flags & HIDMS_ABSX != 0 {
                if !released {
                    wsmouse_set(wsm, WSMOUSE_ABS_X, dx, 0);
                }
                dx = 0;
            }
            if flags & HIDMS_ABSY != 0 {
                if !released {
                    wsmouse_set(wsm, WSMOUSE_ABS_Y, dy, 0);
                }
                dy = 0;
            }
            crate::WSMOUSE_INPUT!(wsm, buttons, dx, dy, dz, dw);
            splx(s);
        }
    }
}

/// `hidms_enable`: `EBUSY` if it is enabled already, 0 otherwise.
pub fn hidms_enable(ms: &Hidms) -> i32 {
    if ms.sc_enabled.get() != 0 {
        return Errno::EBUSY as i32;
    }

    ms.sc_enabled.set(1);
    ms.sc_buttons.set(0);
    0
}

/// `hidms_ioctl`: the touch panel calibration and the type of a touch panel; `Ok(false)` is
/// the C's -1.
pub fn hidms_ioctl(
    ms: &Hidms,
    cmd: u64,
    data: &mut [u8],
    _flag: i32,
    _p: Option<&Proc>,
) -> Result<bool, Errno> {
    match cmd {
        WSMOUSEIO_SCALIBCOORDS => {
            let wsmc: WsmouseCalibcoords = ioctl_arg(data);
            if !(wsmc.minx >= -32768
                && wsmc.maxx >= -32768
                && wsmc.miny >= -32768
                && wsmc.maxy >= -32768
                && wsmc.resx >= 0
                && wsmc.resy >= 0
                && wsmc.minx < 32768
                && wsmc.maxx < 32768
                && wsmc.miny < 32768
                && wsmc.maxy < 32768
                && (wsmc.maxx - wsmc.minx) != 0
                && (wsmc.maxy - wsmc.miny) != 0
                && wsmc.resx < 32768
                && wsmc.resy < 32768
                && wsmc.swapxy >= 0
                && wsmc.swapxy <= 1
                && wsmc.samplelen >= 0
                && wsmc.samplelen <= 1)
            {
                return Err(Errno::EINVAL);
            }

            ms.update_tsscale(|t| {
                t.minx = wsmc.minx;
                t.maxx = wsmc.maxx;
                t.miny = wsmc.miny;
                t.maxy = wsmc.maxy;
                t.swapxy = wsmc.swapxy;
                t.resx = wsmc.resx;
                t.resy = wsmc.resy;
            });
            ms.sc_rawmode.set(wsmc.samplelen);
            Ok(true)
        }
        WSMOUSEIO_GCALIBCOORDS => {
            let mut wsmc: WsmouseCalibcoords = ioctl_arg(data);
            let t = ms.sc_tsscale.get();
            wsmc.minx = t.minx;
            wsmc.maxx = t.maxx;
            wsmc.miny = t.miny;
            wsmc.maxy = t.maxy;
            wsmc.swapxy = t.swapxy;
            wsmc.resx = t.resx;
            wsmc.resy = t.resy;
            wsmc.samplelen = ms.sc_rawmode.get();
            ioctl_ret(data, &wsmc);
            Ok(true)
        }
        WSMOUSEIO_GTYPE => {
            let flags = ms.sc_flags.get();
            if flags & HIDMS_ABSX != 0 && flags & HIDMS_ABSY != 0 {
                ioctl_ret(data, &WSMOUSE_TYPE_TPANEL);
                return Ok(true);
            }
            Ok(false)
        }
        _ => Ok(false),
    }
}

/// `hidms_disable`.
pub fn hidms_disable(ms: &Hidms) {
    ms.sc_enabled.set(0);
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for `hidms`: the setup over the report descriptors of QEMU's `usb-mouse` and
    // `usb-tablet` and over a Wacom stylus collection, the decoding of reports (read back
    // from `sc_buttons`; there is no `wsmouse` child on the host), the quirk layouts and the
    // calibration ioctls.

    use std::assert_eq;
    use std::boxed::Box;

    use super::*;

    /// QEMU's `usb-mouse` report descriptor (`hw/usb/dev-hid.c`): three buttons, relative X,
    /// Y and wheel, one byte each.
    const QEMU_MOUSE: &[u8] = &[
        0x05, 0x01, 0x09, 0x02, 0xa1, 0x01, 0x09, 0x01, 0xa1, 0x00, 0x05, 0x09, 0x19, 0x01, 0x29,
        0x03, 0x15, 0x00, 0x25, 0x01, 0x95, 0x03, 0x75, 0x01, 0x81, 0x02, 0x95, 0x01, 0x75, 0x05,
        0x81, 0x01, 0x05, 0x01, 0x09, 0x30, 0x09, 0x31, 0x09, 0x38, 0x15, 0x81, 0x25, 0x7f, 0x75,
        0x08, 0x95, 0x03, 0x81, 0x06, 0xc0, 0xc0,
    ];

    /// QEMU's `usb-tablet` report descriptor: three buttons, absolute X and Y of 15 bits in
    /// 16-bit fields, a relative wheel.
    const QEMU_TABLET: &[u8] = &[
        0x05, 0x01, 0x09, 0x02, 0xa1, 0x01, 0x09, 0x01, 0xa1, 0x00, 0x05, 0x09, 0x19, 0x01, 0x29,
        0x03, 0x15, 0x00, 0x25, 0x01, 0x95, 0x03, 0x75, 0x01, 0x81, 0x02, 0x95, 0x01, 0x75, 0x05,
        0x81, 0x01, 0x05, 0x01, 0x09, 0x30, 0x09, 0x31, 0x15, 0x00, 0x26, 0xff, 0x7f, 0x35, 0x00,
        0x46, 0xff, 0x7f, 0x75, 0x10, 0x95, 0x02, 0x81, 0x02, 0x05, 0x01, 0x09, 0x38, 0x15, 0x81,
        0x25, 0x7f, 0x35, 0x00, 0x45, 0x00, 0x75, 0x08, 0x95, 0x01, 0x81, 0x06, 0xc0, 0xc0,
    ];

    /// A Wacom stylus collection: tip and barrel switches, 6 bits of padding, X and Y of
    /// 16 bits (usages 0x0130 and 0x0131 on the Wacom digitizer page).
    const WACOM_STYLUS: &[u8] = &[
        0x06, 0x0d, 0xff, 0x09, 0x01, 0xa1, 0x01, 0x09, 0x20, 0xa1, 0x00, 0x09, 0x42, 0x15, 0x00,
        0x25, 0x01, 0x75, 0x01, 0x95, 0x01, 0x81, 0x02, 0x09, 0x44, 0x81, 0x02, 0x75, 0x06, 0x81,
        0x03, 0x75, 0x10, 0x26, 0xff, 0x7f, 0x0a, 0x30, 0x01, 0x81, 0x02, 0x0a, 0x31, 0x01, 0x81,
        0x02, 0xc0, 0xc0,
    ];

    fn fake_device() -> &'static Device {
        // SAFETY: a `Device` is all-zero valid (`config_make_softc` allocates it zeroed); the box
        // is leaked, so the reference lives forever.
        Box::leak(Box::new(unsafe { core::mem::zeroed::<Device>() }))
    }

    fn setup(desc: &[u8], quirks: u32) -> Box<Hidms> {
        let ms = Box::new(Hidms::new());
        assert_eq!(hidms_setup(fake_device(), &ms, quirks, 0, desc), Ok(()));
        ms
    }

    #[test]
    fn button_numbering_swaps_the_middle_and_right_buttons() {
        assert_eq!(
            [0, 1, 2, 3, 4].map(hidms_but),
            [0, 2, 1, 3, 4],
            "HIDMS_BUT(1) = 2 and HIDMS_BUT(2) = 1"
        );
    }

    #[test]
    fn setup_qemu_usb_mouse() {
        let ms = setup(QEMU_MOUSE, 0);
        assert_eq!(ms.sc_num_buttons.get(), 3);
        assert_eq!(ms.sc_flags.get(), HIDMS_Z | HIDMS_REVZ);
        let loc = |pos| HidLocation {
            size: 8,
            count: 1,
            pos,
        };
        assert_eq!(ms.sc_loc_x.get(), loc(8));
        assert_eq!(ms.sc_loc_y.get(), loc(16));
        assert_eq!(ms.sc_loc_z.get(), loc(24));
        for i in 0..3 {
            assert_eq!(
                ms.loc_btn(i),
                HidLocation {
                    size: 1,
                    count: 1,
                    pos: i as u32
                }
            );
        }
    }

    #[test]
    fn setup_qemu_usb_tablet_is_absolute() {
        let ms = setup(QEMU_TABLET, 0);
        assert_eq!(ms.sc_num_buttons.get(), 3);
        assert_eq!(
            ms.sc_flags.get(),
            HIDMS_ABSX | HIDMS_ABSY | HIDMS_Z | HIDMS_REVZ
        );
        let t = ms.sc_tsscale.get();
        assert_eq!((t.minx, t.maxx, t.miny, t.maxy), (0, 0x7fff, 0, 0x7fff));
        assert_eq!(ms.sc_loc_x.get().size, 16);
        assert_eq!(ms.sc_loc_x.get().pos, 8);
        assert_eq!(ms.sc_loc_y.get().pos, 24);
        assert_eq!(ms.sc_loc_z.get().pos, 40);

        let mut gtype = 0u32.to_ne_bytes();
        assert_eq!(
            hidms_ioctl(&ms, WSMOUSEIO_GTYPE, &mut gtype, 0, None),
            Ok(true)
        );
        assert_eq!(u32::from_ne_bytes(gtype), WSMOUSE_TYPE_TPANEL);
    }

    #[test]
    fn a_relative_mouse_has_no_type_of_its_own() {
        let ms = setup(QEMU_MOUSE, 0);
        let mut gtype = [0u8; 4];
        assert_eq!(
            hidms_ioctl(&ms, WSMOUSEIO_GTYPE, &mut gtype, 0, None),
            Ok(false)
        );
        assert_eq!(hidms_ioctl(&ms, 0x1234, &mut gtype, 0, None), Ok(false));
    }

    #[test]
    fn input_tracks_the_buttons() {
        let ms = setup(QEMU_MOUSE, 0);
        hidms_input(&ms, &[0x01, 5, 3, 0]);
        assert_eq!(ms.sc_buttons.get(), 1 << 0);
        hidms_input(&ms, &[0x02, 0, 0, 0]);
        assert_eq!(
            ms.sc_buttons.get(),
            1 << 2,
            "button 2 is wsmouse's right button"
        );
        hidms_input(&ms, &[0x04, 0, 0, 0]);
        assert_eq!(
            ms.sc_buttons.get(),
            1 << 1,
            "button 3 is wsmouse's middle button"
        );
        hidms_input(&ms, &[0x00, 0, 0, 0]);
        assert_eq!(ms.sc_buttons.get(), 0);
    }

    #[test]
    fn enable_is_exclusive_and_clears_the_buttons() {
        let ms = setup(QEMU_MOUSE, 0);
        ms.sc_buttons.set(7);
        assert_eq!(hidms_enable(&ms), 0);
        assert_eq!(ms.sc_buttons.get(), 0);
        assert_eq!(hidms_enable(&ms), Errno::EBUSY as i32);
        hidms_disable(&ms);
        assert_eq!(hidms_enable(&ms), 0);
    }

    #[test]
    fn spurious_button_up_reports_are_dropped() {
        let ms = setup(QEMU_MOUSE, HIDMS_SPUR_BUT_UP);
        hidms_input(&ms, &[0x14, 0, 0, 0]);
        assert_eq!(
            ms.sc_buttons.get(),
            0,
            "0x14 is dropped, its button bit not seen"
        );
        hidms_input(&ms, &[0x05, 0, 0, 0]);
        assert_eq!(
            ms.sc_buttons.get(),
            0b11,
            "buttons 1 and 3 are wsmouse's left and middle"
        );
    }

    #[test]
    fn leading_byte_reports() {
        let ms = setup(QEMU_MOUSE, 0);
        ms.flags_or(HIDMS_LEADINGBYTE);
        hidms_input(&ms, &[0x02, 0x01, 0, 0, 0]);
        assert_eq!(ms.sc_buttons.get(), 0, "a leading 0x02 drops the report");
        hidms_input(&ms, &[0x01, 0x01, 0, 0, 0]);
        assert_eq!(ms.sc_buttons.get(), 1);
        hidms_input(&ms, &[]);
        assert_eq!(ms.sc_buttons.get(), 1, "an empty report is dropped");
    }

    #[test]
    fn microsoft_bad_class_layout() {
        let ms = setup(QEMU_MOUSE, HIDMS_MS_BAD_CLASS);
        assert_eq!(ms.sc_flags.get(), HIDMS_Z | HIDMS_SPUR_BUT_UP);
        assert_eq!(ms.sc_num_buttons.get(), 3);
        assert_eq!(ms.sc_loc_x.get().pos, 16);
        assert_eq!(ms.sc_loc_y.get().pos, 24);
        assert_eq!(ms.sc_loc_z.get().pos, 32);
        assert_eq!(ms.loc_btn(0).pos, 8);
        assert_eq!(ms.loc_btn(1).pos, 9);
        assert_eq!(ms.loc_btn(2).pos, 10);
    }

    #[test]
    fn wacom_setup_collects_the_stylus_switches_and_axes() {
        let ms = setup(WACOM_STYLUS, HIDMS_WACOM_SETUP);
        assert_eq!(
            ms.sc_flags.get(),
            HIDMS_TIP | HIDMS_BARREL | HIDMS_ABSX | HIDMS_ABSY
        );
        assert_eq!(ms.sc_num_stylus_buttons.get(), 2);
        assert_eq!(ms.sc_num_buttons.get(), 2);
        assert_eq!(ms.loc_btn(0).pos, 0);
        assert_eq!(ms.loc_btn(1).pos, 1);
        assert_eq!(ms.sc_loc_x.get().pos, 8);
        assert_eq!(ms.sc_loc_y.get().pos, 24);
        let t = ms.sc_tsscale.get();
        assert_eq!((t.maxx, t.maxy), (0x7fff, 0x7fff));
    }

    #[test]
    fn calibration_ioctls() {
        let ms = setup(QEMU_TABLET, 0);
        let mut c = WsmouseCalibcoords {
            minx: 0,
            miny: 0,
            maxx: 0x7fff,
            maxy: 0x7fff,
            swapxy: 0,
            resx: 1024,
            resy: 768,
            samplelen: 0,
            ..Default::default()
        };
        let mut buf = [0u8; size_of::<WsmouseCalibcoords>()];
        ioctl_ret(&mut buf, &c);
        assert_eq!(
            hidms_ioctl(&ms, WSMOUSEIO_SCALIBCOORDS, &mut buf, 0, None),
            Ok(true)
        );
        assert_eq!(ms.sc_rawmode.get(), 0);
        assert_eq!(ms.sc_tsscale.get().resx, 1024);

        let mut out = [0u8; size_of::<WsmouseCalibcoords>()];
        assert_eq!(
            hidms_ioctl(&ms, WSMOUSEIO_GCALIBCOORDS, &mut out, 0, None),
            Ok(true)
        );
        let got: WsmouseCalibcoords = ioctl_arg(&out);
        assert_eq!((got.maxx, got.resy, got.samplelen), (0x7fff, 768, 0));

        // the scaled report: the middle of the range lands in the middle of the screen
        // (read back through the buttons only: with no wsmouse the position is not kept)
        c.maxx = c.minx;
        ioctl_ret(&mut buf, &c);
        assert_eq!(
            hidms_ioctl(&ms, WSMOUSEIO_SCALIBCOORDS, &mut buf, 0, None),
            Err(Errno::EINVAL),
            "an empty range is refused"
        );
        c.maxx = 0x7fff;
        c.resx = -1;
        ioctl_ret(&mut buf, &c);
        assert_eq!(
            hidms_ioctl(&ms, WSMOUSEIO_SCALIBCOORDS, &mut buf, 0, None),
            Err(Errno::EINVAL)
        );
    }
}
/* </TESTS> */
