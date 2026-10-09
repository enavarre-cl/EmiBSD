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
 * Copyright (c) 2016 Mark Kettenis
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
 * Copyright (c) 2016, 2019 Mark Kettenis
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
//! The device tree's GPIO controllers: `dev/ofw/ofw_gpio.h` (the flags, `struct
//! gpio_controller`) and `dev/ofw/ofw_gpio.c` (the registry the `gpios` properties are
//! resolved through).
//!
//! Upstream: sys/dev/ofw/ofw_gpio.h @ 3ce1f3f79392
//! Upstream: sys/dev/ofw/ofw_gpio.c @ 3ce1f3f79392
//!
//! A controller driver (`plgpio`) fills a [`GpioController`] in its softc and registers it;
//! a consumer (`gpiokeys`) passes a `gpios` property's cells, whose first cell is the
//! controller's phandle, and the rest reach the controller's methods.
//!
//! ## Deviations
//! - `struct gpio_controller`'s members are `Cell`s, as the controller fills the structure
//!   inside its zeroed softc after autoconfiguration allocated it; its methods are
//!   `Option<fn>` (NULL is `None`).
//! - The `uint32_t *cells` the C passes are slices of a `gpios` property: the controller
//!   gets the cells after the phandle, `gpio_controller_next_pin` returns the rest of the
//!   slice after this pin (`None` for an unknown phandle, the C's NULL), and a property too
//!   short for a controller's cells stops the walk instead of reading past it.
//! - `gpio_controllers` is a `static` behind a `Sync` wrapper ([`GpioControllers`]):
//!   controllers register while autoconfiguring and the list is only read after, as the
//!   C's unlocked list.
//! - The GPIO hogs' `gpios` are read into a `Vec` (`malloc`/`free` with `M_TEMP`).

use alloc::vec;
use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::NonNull;

use crate::dev::ofw::fdt::{OF_child, OF_getpropint, OF_getpropintarray, OF_getproplen, OF_peer};
use crate::machine::cpu::CpuInfo;
use crate::machine::fdt::fdt_intr_disestablish;
use crate::machine::intr::IntrFn;
use crate::queue_adapter;
use crate::sys::queue::{ListEntry, ListHead};

/// `GPIO_ACTIVE_HIGH`.
#[allow(clippy::identity_op)] // the C's `(0 << 0)`, beside its bit's other value
pub const GPIO_ACTIVE_HIGH: u32 = 0 << 0;
/// `GPIO_ACTIVE_LOW`.
pub const GPIO_ACTIVE_LOW: u32 = 1 << 0;
/// `GPIO_PUSH_PULL`.
pub const GPIO_PUSH_PULL: u32 = 0 << 1;
/// `GPIO_SINGLE_ENDED`.
pub const GPIO_SINGLE_ENDED: u32 = 1 << 1;
/// `GPIO_LINE_OPEN_SOURCE`.
pub const GPIO_LINE_OPEN_SOURCE: u32 = 0 << 2;
/// `GPIO_LINE_OPEN_DRAIN`.
pub const GPIO_LINE_OPEN_DRAIN: u32 = 1 << 2;

/// `GPIO_OPEN_DRAIN`.
pub const GPIO_OPEN_DRAIN: u32 = GPIO_SINGLE_ENDED | GPIO_LINE_OPEN_DRAIN;
/// `GPIO_OPEN_SOURCE`.
pub const GPIO_OPEN_SOURCE: u32 = GPIO_SINGLE_ENDED | GPIO_LINE_OPEN_SOURCE;

/// `GPIO_CONFIG_INPUT`.
pub const GPIO_CONFIG_INPUT: i32 = 0x0000;
/// `GPIO_CONFIG_OUTPUT`.
pub const GPIO_CONFIG_OUTPUT: i32 = 0x0001;
/// `GPIO_CONFIG_PULL_UP`.
pub const GPIO_CONFIG_PULL_UP: i32 = 0x0010;
/// `GPIO_CONFIG_PULL_DOWN`.
pub const GPIO_CONFIG_PULL_DOWN: i32 = 0x0020;
/// `GPIO_CONFIG_MD0`.
pub const GPIO_CONFIG_MD0: i32 = 0x1000;
/// `GPIO_CONFIG_MD1`.
pub const GPIO_CONFIG_MD1: i32 = 0x2000;
/// `GPIO_CONFIG_MD2`.
pub const GPIO_CONFIG_MD2: i32 = 0x4000;
/// `GPIO_CONFIG_MD3`.
pub const GPIO_CONFIG_MD3: i32 = 0x8000;

/// `gc_config_pin`: configure the pin the cells name.
pub type GcConfigPin = fn(*mut c_void, &[u32], i32);
/// `gc_get_pin`: read the pin the cells name.
pub type GcGetPin = fn(*mut c_void, &[u32]) -> i32;
/// `gc_set_pin`: drive the pin the cells name.
pub type GcSetPin = fn(*mut c_void, &[u32], i32);
/// `gc_intr_establish`: an interrupt on the pin the cells name.
pub type GcIntrEstablish = fn(
    *mut c_void,
    &[u32],
    i32,
    Option<&CpuInfo>,
    IntrFn,
    *mut c_void,
    &'static str,
) -> Option<NonNull<c_void>>;

/// `struct gpio_controller`.
#[derive(Default)]
pub struct GpioController {
    /// `gc_node`.
    pub gc_node: Cell<i32>,
    /// `gc_cookie`.
    pub gc_cookie: Cell<*mut c_void>,
    /// `gc_config_pin`.
    pub gc_config_pin: Cell<Option<GcConfigPin>>,
    /// `gc_get_pin`.
    pub gc_get_pin: Cell<Option<GcGetPin>>,
    /// `gc_set_pin`.
    pub gc_set_pin: Cell<Option<GcSetPin>>,
    /// `gc_intr_establish`.
    pub gc_intr_establish: Cell<Option<GcIntrEstablish>>,

    /// `gc_list`.
    pub gc_list: ListEntry<GpioController>,
    /// `gc_phandle`.
    pub gc_phandle: Cell<u32>,
    /// `gc_cells`.
    pub gc_cells: Cell<u32>,
}

queue_adapter!(
    /// `LIST_HEAD(, gpio_controller)` through `gc_list`.
    pub GpioControllerList: GpioController, gc_list => ListEntry<GpioController>
);

/// `gpio_controllers`, behind a wrapper that can be a `static`.
pub struct GpioControllers(ListHead<GpioControllerList>);

// SAFETY: controllers register while autoconfiguring (kernel lock) and the list is only read
// afterwards, as the C's unlocked list.
unsafe impl Sync for GpioControllers {}

/// `gpio_controllers`.
pub static GPIO_CONTROLLERS: GpioControllers = GpioControllers(ListHead::new());

/// `gpio_controller_register`: make the controller findable by its phandle and process its
/// GPIO hogs.
pub fn gpio_controller_register(gc: &'static GpioController) {
    let node = gc.gc_node.get();
    gc.gc_cells.set(OF_getpropint(node, b"#gpio-cells", 2));
    gc.gc_phandle.set(OF_getpropint(node, b"phandle", 0));
    if gc.gc_phandle.get() == 0 {
        return;
    }

    // SAFETY: `gc` lives forever and is not on the list yet.
    unsafe { GPIO_CONTROLLERS.0.insert_head(gc) };

    /* Process GPIO hogs. */
    let mut child = OF_child(node);
    while child != 0 {
        gpio_hog(gc, child);
        child = OF_peer(child);
    }
}

/// One child of `gpio_controller_register`'s loop: a GPIO hog's pins set up as it says.
fn gpio_hog(gc: &GpioController, child: i32) {
    if OF_getproplen(child, b"gpio-hog") != 0 {
        return;
    }

    let len = OF_getproplen(child, b"gpios");
    if len <= 0 {
        return;
    }

    /*
     * These need to be processed in the order prescribed
     * by the device tree binding.  First match wins.
     */
    let (config, active) = if OF_getproplen(child, b"input") == 0 {
        (GPIO_CONFIG_INPUT, 0)
    } else if OF_getproplen(child, b"output-low") == 0 {
        (GPIO_CONFIG_OUTPUT, 0)
    } else if OF_getproplen(child, b"output-high") == 0 {
        (GPIO_CONFIG_OUTPUT, 1)
    } else {
        return;
    };

    let mut gpios = vec![0u32; len as usize / size_of::<u32>()];
    OF_getpropintarray(child, b"gpios", &mut gpios);

    let step = (gc.gc_cells.get() as usize).max(1);
    let mut at = 0;
    while at < gpios.len() {
        let gpio = &gpios[at..];
        if let Some(f) = gc.gc_config_pin.get() {
            f(gc.gc_cookie.get(), gpio, config);
        }
        if config & GPIO_CONFIG_OUTPUT != 0
            && let Some(f) = gc.gc_set_pin.get()
        {
            f(gc.gc_cookie.get(), gpio, active);
        }
        at += step;
    }
}

/// The controller whose phandle is `phandle`.
fn gpio_controller_byphandle(phandle: u32) -> Option<&'static GpioController> {
    GPIO_CONTROLLERS
        .0
        .iter()
        .find(|gc| gc.gc_phandle.get() == phandle)
}

/// The controller `cells[0]` names and the cells after it.
fn gpio_controller_of(cells: &[u32]) -> Option<(&'static GpioController, &[u32])> {
    let (&phandle, rest) = cells.split_first()?;
    Some((gpio_controller_byphandle(phandle)?, rest))
}

/// `gpio_controller_config_pin`.
pub fn gpio_controller_config_pin(cells: &[u32], config: i32) {
    if let Some((gc, rest)) = gpio_controller_of(cells)
        && let Some(f) = gc.gc_config_pin.get()
    {
        f(gc.gc_cookie.get(), rest, config);
    }
}

/// `gpio_controller_get_pin`.
pub fn gpio_controller_get_pin(cells: &[u32]) -> i32 {
    let mut val = 0;

    if let Some((gc, rest)) = gpio_controller_of(cells)
        && let Some(f) = gc.gc_get_pin.get()
    {
        val = f(gc.gc_cookie.get(), rest);
    }

    val
}

/// `gpio_controller_set_pin`.
pub fn gpio_controller_set_pin(cells: &[u32], val: i32) {
    if let Some((gc, rest)) = gpio_controller_of(cells)
        && let Some(f) = gc.gc_set_pin.get()
    {
        f(gc.gc_cookie.get(), rest, val);
    }
}

/// `gpio_controller_next_pin`: the cells after this pin's, `None` for an unknown controller.
pub fn gpio_controller_next_pin(cells: &[u32]) -> Option<&[u32]> {
    let (gc, _) = gpio_controller_of(cells)?;
    Some(
        cells
            .get(gc.gc_cells.get() as usize + 1..)
            .unwrap_or_default(),
    )
}

/// `gpio_controller_intr_establish`: an interrupt on the pin, if its controller has them.
pub fn gpio_controller_intr_establish(
    cells: &[u32],
    ipl: i32,
    ci: Option<&CpuInfo>,
    func: IntrFn,
    arg: *mut c_void,
    name: &'static str,
) -> Option<NonNull<c_void>> {
    let (gc, rest) = gpio_controller_of(cells)?;
    let f = gc.gc_intr_establish.get()?;
    f(gc.gc_cookie.get(), rest, ipl, ci, func, arg, name)
}

/// `gpio_controller_intr_disestablish`.
///
/// # Safety
///
/// `ih` is a handle `gpio_controller_intr_establish` returned (an `fdt_intr_establish`
/// cookie) that is not used again.
pub unsafe fn gpio_controller_intr_disestablish(ih: NonNull<c_void>) {
    // SAFETY: the caller's contract.
    unsafe { fdt_intr_disestablish(ih) };
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use std::boxed::Box;
    use std::ptr;

    use super::*;

    /// What a fake controller saw: the last cells and value of each method.
    #[derive(Default)]
    struct Seen {
        config: Cell<(u32, i32)>,
        set: Cell<(u32, i32)>,
    }

    fn seen(cookie: *mut c_void) -> &'static Seen {
        // SAFETY: the tests' cookie is a leaked `Seen`.
        unsafe { &*cookie.cast::<Seen>() }
    }

    fn cfg(cookie: *mut c_void, cells: &[u32], config: i32) {
        seen(cookie).config.set((cells[0], config));
    }

    fn get(_cookie: *mut c_void, cells: &[u32]) -> i32 {
        (cells[0] == 3 && cells[1] == GPIO_ACTIVE_HIGH) as i32
    }

    fn set(cookie: *mut c_void, cells: &[u32], val: i32) {
        seen(cookie).set.set((cells[0], val));
    }

    #[test]
    fn dispatch_by_phandle() {
        let s: &'static Seen = Box::leak(Box::default());
        let gc: &'static GpioController = Box::leak(Box::default());
        gc.gc_cookie.set(ptr::from_ref(s).cast_mut().cast());
        gc.gc_config_pin.set(Some(cfg));
        gc.gc_get_pin.set(Some(get));
        gc.gc_set_pin.set(Some(set));
        gc.gc_phandle.set(0x8001);
        gc.gc_cells.set(2);
        // SAFETY: a leaked controller, on no list (as gpio_controller_register would put it).
        unsafe { GPIO_CONTROLLERS.0.insert_head(gc) };

        // A gpios property of two pins on the controller: <&ctl 3 0>, <&ctl 5 1>.
        let gpios = [0x8001, 3, GPIO_ACTIVE_HIGH, 0x8001, 5, GPIO_ACTIVE_LOW];
        gpio_controller_config_pin(&gpios, GPIO_CONFIG_INPUT);
        assert_eq!(s.config.get(), (3, GPIO_CONFIG_INPUT));
        assert_eq!(gpio_controller_get_pin(&gpios), 1);
        let next = gpio_controller_next_pin(&gpios).unwrap();
        assert_eq!(next, &[0x8001, 5, GPIO_ACTIVE_LOW]);
        gpio_controller_set_pin(next, 1);
        assert_eq!(s.set.get(), (5, 1));
        assert_eq!(gpio_controller_get_pin(next), 0);
        assert_eq!(gpio_controller_next_pin(next).unwrap(), &[] as &[u32]);

        // An unknown phandle: nothing happens, 0, NULL.
        assert_eq!(gpio_controller_get_pin(&[0x9999, 3, 0]), 0);
        assert!(gpio_controller_next_pin(&[0x9999, 3, 0]).is_none());
        fn handler(_: *mut c_void) -> i32 {
            1
        }
        // The controller has no interrupts: NULL.
        assert!(
            gpio_controller_intr_establish(&gpios, 0, None, handler, ptr::null_mut(), "t")
                .is_none()
        );
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/ofw/ofw_gpio.h");
        crate::reftest::assert_defines!(defs;
            GPIO_ACTIVE_HIGH, GPIO_ACTIVE_LOW, GPIO_PUSH_PULL, GPIO_SINGLE_ENDED,
            GPIO_LINE_OPEN_SOURCE, GPIO_LINE_OPEN_DRAIN, GPIO_CONFIG_INPUT, GPIO_CONFIG_OUTPUT,
            GPIO_CONFIG_PULL_UP, GPIO_CONFIG_PULL_DOWN, GPIO_CONFIG_MD0, GPIO_CONFIG_MD1,
            GPIO_CONFIG_MD2, GPIO_CONFIG_MD3,
        );
    }
}
/* </TESTS> */
