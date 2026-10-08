/*	$OpenBSD: gpiovar.h,v 1.6 2011/10/03 20:24:51 matthieu Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 2004, 2006 Alexander Yurchenko <grange@openbsd.org>
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
//! `dev/gpio/gpiovar.h`: what a GPIO controller hands the gpio(4) bus, and what the bus
//! hands the drivers attached to its pins.
//!
//! Upstream: sys/dev/gpio/gpiovar.h @ 3ce1f3f79392
//!
//! The prototypes (`gpio_pin_map`, ..., `gpio_npins`, `gpiobus_print`) are `gpio.c`'s and
//! live in `dev/gpio/gpio.rs`.
//!
//! ## Deviations
//! - `gpio_chipset_tag_t` is `&'static GpioChipsetTag`: a controller's tag lives in its
//!   softc, which is never freed. The three methods are `Option<fn>`, so a controller softc
//!   that embeds the tag stays valid all-zero (`config_make_softc`); calling one the
//!   controller did not set reads 0 or does nothing, where the C would jump to NULL.
//! - `gpio_pin_t`'s members are `Cell`s: the controller owns the array and gpio(4) changes
//!   `pin_flags`, `pin_state` and `pin_mapped` through its shared pointer.
//! - `gba_name` and `ga_dvname` are byte strings without the NUL; `gba_pins` is a
//!   `&'static` slice beside the C's `gba_npins`.
//! - `struct gpio_pinmap`'s `pm_map` is the child's slice of slots (`int sc_map[N]` in the
//!   C drivers); `gpio_pin_map` refuses a mask with more pins than slots, where the C
//!   writes past the array.
//! - `struct gpio_dev` and `struct gpio_name` are list elements `gpio.c` allocates
//!   (`Box`), with their `LIST_ENTRY`s as `ListEntry`s; `gp_name` is a `Cell`, as
//!   `GPIOPINSET` renames a pin in place.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::NonNull;

use crate::queue_adapter;
use crate::sys::device::Device;
use crate::sys::gpio::GPIOPINMAXNAME;
use crate::sys::queue::ListEntry;

/// `struct gpio_chipset_tag`: the GPIO controller's description.
pub struct GpioChipsetTag {
    /// `gp_cookie`: the controller's softc.
    pub gp_cookie: *mut c_void,
    /// `gp_pin_read`.
    pub gp_pin_read: Option<fn(*mut c_void, i32) -> i32>,
    /// `gp_pin_write`.
    pub gp_pin_write: Option<fn(*mut c_void, i32, i32)>,
    /// `gp_pin_ctl`.
    pub gp_pin_ctl: Option<fn(*mut c_void, i32, i32)>,
}

impl GpioChipsetTag {
    /// `gpiobus_pin_read(gc, pin)`.
    pub fn gpiobus_pin_read(&self, pin: i32) -> i32 {
        self.gp_pin_read.map_or(0, |f| f(self.gp_cookie, pin))
    }

    /// `gpiobus_pin_write(gc, pin, value)`.
    pub fn gpiobus_pin_write(&self, pin: i32, value: i32) {
        if let Some(f) = self.gp_pin_write {
            f(self.gp_cookie, pin, value);
        }
    }

    /// `gpiobus_pin_ctl(gc, pin, flags)`.
    pub fn gpiobus_pin_ctl(&self, pin: i32, flags: i32) {
        if let Some(f) = self.gp_pin_ctl {
            f(self.gp_cookie, pin, flags);
        }
    }
}

/// `gpio_chipset_tag_t`.
pub type GpioChipsetTagT = &'static GpioChipsetTag;

/// `gpio_pin_t` (`struct gpio_pin`): a pin's description.
#[derive(Default)]
pub struct GpioPin {
    /// `pin_num`: number.
    pub pin_num: Cell<i32>,
    /// `pin_caps`: capabilities.
    pub pin_caps: Cell<i32>,
    /// `pin_flags`: current configuration.
    pub pin_flags: Cell<i32>,
    /// `pin_state`: current state.
    pub pin_state: Cell<i32>,
    /// `pin_mapped`: is mapped.
    pub pin_mapped: Cell<i32>,
}

/// `gpio_pin_t`.
pub type GpioPinT = GpioPin;

/// `struct gpiobus_attach_args`: what attaches the GPIO framework to a controller.
pub struct GpiobusAttachArgs {
    /// `gba_name`: the bus name.
    pub gba_name: &'static [u8],
    /// `gba_gc`: the underlying controller.
    pub gba_gc: GpioChipsetTagT,
    /// `gba_pins`: the pins array.
    pub gba_pins: &'static [GpioPin],
    /// `gba_npins`: total number of pins.
    pub gba_npins: i32,
}

/// `struct gpio_attach_args`: what attaches a device connected to GPIO pins.
pub struct GpioAttachArgs<'a> {
    /// `ga_gpio`: the gpio(4) softc.
    pub ga_gpio: *mut c_void,
    /// `ga_offset`.
    pub ga_offset: i32,
    /// `ga_mask`.
    pub ga_mask: u32,
    /// `ga_dvname`: the driver's name (no NUL).
    pub ga_dvname: &'a [u8],
    /// `ga_flags`.
    pub ga_flags: u32,
}

/// `struct gpio_pinmap`: a child's pin map.
pub struct GpioPinmap<'a> {
    /// `pm_map`: the pin map.
    pub pm_map: &'a mut [i32],
    /// `pm_size`: the map's size.
    pub pm_size: i32,
}

/// `struct gpio_dev`: a device attached by `GPIOATTACH`.
pub struct GpioDev {
    /// `sc_dev`: the gpio device.
    pub sc_dev: NonNull<Device>,
    /// `sc_next`.
    pub sc_next: ListEntry<GpioDev>,
}

queue_adapter!(
    /// `LIST_HEAD(, gpio_dev)` through `sc_next`.
    pub GpioDevList: GpioDev, sc_next => ListEntry<GpioDev>
);

/// `struct gpio_name`: a named pin.
pub struct GpioName {
    /// `gp_name`.
    pub gp_name: Cell<[u8; GPIOPINMAXNAME]>,
    /// `gp_pin`.
    pub gp_pin: Cell<i32>,
    /// `gp_next`.
    pub gp_next: ListEntry<GpioName>,
}

queue_adapter!(
    /// `LIST_HEAD(, gpio_name)` through `gp_next`.
    pub GpioNameList: GpioName, gp_next => ListEntry<GpioName>
);
/* </CODE> */
