/*	$OpenBSD: gpio.h,v 1.8 2011/10/03 20:24:51 matthieu Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 2004 Alexander Yurchenko <grange@openbsd.org>
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
//! `<sys/gpio.h>`: the gpio(4) device's pin flags and `ioctl(2)` interface.
//!
//! Upstream: sys/sys/gpio.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - The `ioctl` structures are `#[repr(C)]` with the C's members and no padding, marked
//!   [`AbiPod`], so `gpio_ioctl` reads and writes them with `ioctl_arg`/`ioctl_ret`.

use crate::machine::copy::AbiPod;
use crate::sys::ioccom::{_ior, _iowr};

/* GPIO pin states */
/// `GPIO_PIN_LOW`: low level (logical 0).
pub const GPIO_PIN_LOW: i32 = 0x00;
/// `GPIO_PIN_HIGH`: high level (logical 1).
pub const GPIO_PIN_HIGH: i32 = 0x01;

/// `GPIOPINMAXNAME`: the longest pin name.
pub const GPIOPINMAXNAME: usize = 64;

/* GPIO pin configuration flags */
/// `GPIO_PIN_INPUT`: input direction.
pub const GPIO_PIN_INPUT: i32 = 0x0001;
/// `GPIO_PIN_OUTPUT`: output direction.
pub const GPIO_PIN_OUTPUT: i32 = 0x0002;
/// `GPIO_PIN_INOUT`: bi-directional.
pub const GPIO_PIN_INOUT: i32 = 0x0004;
/// `GPIO_PIN_OPENDRAIN`: open-drain output.
pub const GPIO_PIN_OPENDRAIN: i32 = 0x0008;
/// `GPIO_PIN_PUSHPULL`: push-pull output.
pub const GPIO_PIN_PUSHPULL: i32 = 0x0010;
/// `GPIO_PIN_TRISTATE`: output disabled.
pub const GPIO_PIN_TRISTATE: i32 = 0x0020;
/// `GPIO_PIN_PULLUP`: internal pull-up enabled.
pub const GPIO_PIN_PULLUP: i32 = 0x0040;
/// `GPIO_PIN_PULLDOWN`: internal pull-down enabled.
pub const GPIO_PIN_PULLDOWN: i32 = 0x0080;
/// `GPIO_PIN_INVIN`: invert input.
pub const GPIO_PIN_INVIN: i32 = 0x0100;
/// `GPIO_PIN_INVOUT`: invert output.
pub const GPIO_PIN_INVOUT: i32 = 0x0200;
/// `GPIO_PIN_USER`: user != 0 can access.
pub const GPIO_PIN_USER: i32 = 0x0400;
/// `GPIO_PIN_SET`: set for securelevel access.
pub const GPIO_PIN_SET: i32 = 0x8000;

/// `struct gpio_info`: the GPIO controller's description.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GpioInfo {
    /// `gpio_npins`: total number of pins available.
    pub gpio_npins: i32,
}

// SAFETY: `#[repr(C)]`, one `int`.
unsafe impl AbiPod for GpioInfo {}

/// `struct gpio_pin_op`: a pin operation (read, write, toggle).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GpioPinOp {
    /// `gp_name`: the pin's name.
    pub gp_name: [u8; GPIOPINMAXNAME],
    /// `gp_pin`: the pin's number.
    pub gp_pin: i32,
    /// `gp_value`: the value.
    pub gp_value: i32,
}

// SAFETY: `#[repr(C)]`: 64 bytes, then two `int`s at 64 and 68; no padding.
unsafe impl AbiPod for GpioPinOp {}

/// `struct gpio_pin_set`: a pin's configuration.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GpioPinSet {
    /// `gp_name`.
    pub gp_name: [u8; GPIOPINMAXNAME],
    /// `gp_pin`.
    pub gp_pin: i32,
    /// `gp_caps`.
    pub gp_caps: i32,
    /// `gp_flags`.
    pub gp_flags: i32,
    /// `gp_name2`: the new name.
    pub gp_name2: [u8; GPIOPINMAXNAME],
}

// SAFETY: `#[repr(C)]`: 64 bytes, three `int`s, 64 bytes: 140 bytes, 4-aligned, no padding.
unsafe impl AbiPod for GpioPinSet {}

/// `struct gpio_attach`: attach or detach a driver that uses GPIO pins.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GpioAttach {
    /// `ga_dvname`: the device's name.
    pub ga_dvname: [u8; 16],
    /// `ga_offset`: the pin number.
    pub ga_offset: i32,
    /// `ga_mask`: the binary mask.
    pub ga_mask: u32,
    /// `ga_flags`: flags.
    pub ga_flags: u32,
}

// SAFETY: `#[repr(C)]`: 16 bytes, then three 4-byte integers; no padding.
unsafe impl AbiPod for GpioAttach {}

/// `GPIOINFO`: `_IOR('G', 0, struct gpio_info)`.
pub const GPIOINFO: u64 = _ior::<GpioInfo>(b'G', 0);
/// `GPIOPINREAD`: `_IOWR('G', 1, struct gpio_pin_op)`.
pub const GPIOPINREAD: u64 = _iowr::<GpioPinOp>(b'G', 1);
/// `GPIOPINWRITE`: `_IOWR('G', 2, struct gpio_pin_op)`.
pub const GPIOPINWRITE: u64 = _iowr::<GpioPinOp>(b'G', 2);
/// `GPIOPINTOGGLE`: `_IOWR('G', 3, struct gpio_pin_op)`.
pub const GPIOPINTOGGLE: u64 = _iowr::<GpioPinOp>(b'G', 3);
/// `GPIOPINSET`: `_IOWR('G', 4, struct gpio_pin_set)`.
pub const GPIOPINSET: u64 = _iowr::<GpioPinSet>(b'G', 4);
/// `GPIOPINUNSET`: `_IOWR('G', 5, struct gpio_pin_set)`.
pub const GPIOPINUNSET: u64 = _iowr::<GpioPinSet>(b'G', 5);
/// `GPIOATTACH`: `_IOWR('G', 6, struct gpio_attach)`.
pub const GPIOATTACH: u64 = _iowr::<GpioAttach>(b'G', 6);
/// `GPIODETACH`: `_IOWR('G', 7, struct gpio_attach)`.
pub const GPIODETACH: u64 = _iowr::<GpioAttach>(b'G', 7);

const _: () = {
    assert!(size_of::<GpioInfo>() == 4);
    assert!(size_of::<GpioPinOp>() == 72);
    assert!(size_of::<GpioPinSet>() == 140);
    assert!(size_of::<GpioAttach>() == 28);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ioctl_numbers() {
        // _IOR('G', 0, 4 bytes), _IOWR('G', 1, 72 bytes), _IOWR('G', 4, 140 bytes),
        // _IOWR('G', 7, 28 bytes): the C macros' arithmetic.
        assert_eq!(GPIOINFO, 0x4004_4700);
        assert_eq!(GPIOPINREAD, 0xc048_4701);
        assert_eq!(GPIOPINSET, 0xc08c_4704);
        assert_eq!(GPIODETACH, 0xc01c_4707);
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/sys/gpio.h");
        crate::reftest::assert_defines!(defs;
            GPIO_PIN_LOW, GPIO_PIN_HIGH, GPIOPINMAXNAME, GPIO_PIN_INPUT, GPIO_PIN_OUTPUT,
            GPIO_PIN_INOUT, GPIO_PIN_OPENDRAIN, GPIO_PIN_PUSHPULL, GPIO_PIN_TRISTATE,
            GPIO_PIN_PULLUP, GPIO_PIN_PULLDOWN, GPIO_PIN_INVIN, GPIO_PIN_INVOUT, GPIO_PIN_USER,
            GPIO_PIN_SET,
        );
    }
}
/* </TESTS> */
