/* <LICENSES> */
/*
 * Copyright (c) 2018 Mark Kettenis <kettenis@openbsd.org>
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
//! The ARM PrimeCell PL061 GPIO controller on the device tree, plgpio(4): `dev/fdt/plgpio.c`.
//!
//! Upstream: sys/dev/fdt/plgpio.c @ 3ce1f3f79392
//!
//! QEMU's `virt` has one (`pl061@9030000`), whose pin 3 is the `gpio-keys` power key.
//! The controller registers with `ofw_gpio` for configuring, reading and driving its eight
//! pins; as in the C it offers no interrupts (no `gc_intr_establish`), so `gpiokeys` polls
//! its keys. OpenBSD 8.0 on the same QEMU machine attaches `plgpio0` and `gpiokeys0` and
//! does nothing on QEMU's `system_powerdown` (`cargo xtask diff-openbsd powerbtn`, M16f).
//!
//! ## Deviations
//! - The attach arguments are the machine's `struct fdt_attach_args`
//!   (`machine::fdt::FdtAttachArgs`).
//! - `sc_iot`/`sc_ioh` are `Cell<Option<..>>`, `None` until the registers are mapped; the
//!   pin methods do nothing (read 0) on a controller whose registers did not map, which
//!   never registers in the C either.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr;

use crate::dev::ofw::fdt::OF_is_compatible;
use crate::dev::ofw::ofw_gpio::{
    GPIO_ACTIVE_LOW, GPIO_CONFIG_OUTPUT, GpioController, gpio_controller_register,
};
use crate::kern::subr_prf::printf;
use crate::machine::bus::{
    BusSpaceHandle, BusSpaceTag, bus_space_map, bus_space_read_1, bus_space_write_1,
};
use crate::machine::fdt::FdtAttachArgs;
use crate::sys::device::{CfMatch, Cfattach, Cfdriver, DV_DULL, Device, Softc};

/* Registers. */
/// `GPIODATA(pin)`: the data register whose address bits mask only `pin`.
const fn gpiodata(pin: u32) -> usize {
    (1usize << pin) << 2
}
/// `GPIODIR`: the direction register (1 is output).
const GPIODIR: usize = 0x400;

/// `struct plgpio_softc`.
#[repr(C)]
pub struct PlgpioSoftc {
    /// `sc_dev`.
    pub sc_dev: Device,
    /// `sc_iot`.
    pub sc_iot: Cell<Option<BusSpaceTag>>,
    /// `sc_ioh`.
    pub sc_ioh: Cell<Option<BusSpaceHandle>>,

    /// `sc_gc`.
    pub sc_gc: GpioController,
}

// SAFETY: `#[repr(C)]` with the `struct device` first; the rest are `Cell`s of `Option`s,
// integers, pointers and `Option<fn>`s and a list entry of null pointers, all valid
// all-zero.
unsafe impl Softc for PlgpioSoftc {}

impl PlgpioSoftc {
    /// `HREAD1(sc, reg)`.
    fn hread1(&self, reg: usize) -> u8 {
        match (self.sc_iot.get(), self.sc_ioh.get()) {
            (Some(iot), Some(ioh)) => bus_space_read_1(iot, ioh, reg),
            _ => 0,
        }
    }

    /// `HWRITE1(sc, reg, val)`.
    fn hwrite1(&self, reg: usize, val: u8) {
        if let (Some(iot), Some(ioh)) = (self.sc_iot.get(), self.sc_ioh.get()) {
            bus_space_write_1(iot, ioh, reg, val);
        }
    }

    /// `HSET1(sc, reg, bits)`.
    fn hset1(&self, reg: usize, bits: u8) {
        self.hwrite1(reg, self.hread1(reg) | bits);
    }

    /// `HCLR1(sc, reg, bits)`.
    fn hclr1(&self, reg: usize, bits: u8) {
        self.hwrite1(reg, self.hread1(reg) & !bits);
    }
}

/// `plgpio_ca`.
pub static PLGPIO_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<PlgpioSoftc>(),
    ca_match: Some(plgpio_match),
    ca_attach: plgpio_attach,
    ca_detach: None,
    ca_activate: None,
};

/// `plgpio_cd`.
pub static PLGPIO_CD: Cfdriver = Cfdriver::new(b"plgpio", DV_DULL, 0);

/// `plgpio_match`: a node compatible with `arm,pl061`.
pub fn plgpio_match(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: the device-tree buses hand their children `struct fdt_attach_args`.
    let faa = unsafe { &*aux.cast::<FdtAttachArgs<'_>>() };

    i32::from(OF_is_compatible(faa.fa_node, b"arm,pl061"))
}

/// `plgpio_attach`: map the registers and register the controller.
pub fn plgpio_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: as in `plgpio_match`.
    let faa = unsafe { &*aux.cast::<FdtAttachArgs<'_>>() };
    // SAFETY: `plgpio_ca` makes `PlgpioSoftc`s; `config_make_softc`'s allocation lives as
    // long as the device, which is never detached.
    let sc: &'static PlgpioSoftc = unsafe { &*ptr::from_ref(self_.softc::<PlgpioSoftc>()) };

    let Some(reg) = faa.fa_reg.first() else {
        printf(format_args!(": no registers\n"));
        return;
    };

    sc.sc_iot.set(Some(faa.fa_iot));

    // SAFETY: the node's register window, which only this driver drives.
    match unsafe { bus_space_map(faa.fa_iot, reg.addr as usize, reg.size as usize, 0) } {
        Ok(ioh) => sc.sc_ioh.set(Some(ioh)),
        Err(_) => {
            printf(format_args!(": can't map registers\n"));
            return;
        }
    }

    sc.sc_gc.gc_node.set(faa.fa_node);
    sc.sc_gc
        .gc_cookie
        .set(ptr::from_ref(sc).cast_mut().cast::<c_void>());
    sc.sc_gc.gc_config_pin.set(Some(plgpio_config_pin));
    sc.sc_gc.gc_get_pin.set(Some(plgpio_get_pin));
    sc.sc_gc.gc_set_pin.set(Some(plgpio_set_pin));
    gpio_controller_register(&sc.sc_gc);

    printf(format_args!("\n"));
}

/// The softc a controller cookie names.
fn softc(cookie: *mut c_void) -> &'static PlgpioSoftc {
    // SAFETY: `plgpio_attach` sets the cookie to its softc, which lives forever.
    unsafe { &*cookie.cast::<PlgpioSoftc>() }
}

/// `plgpio_config_pin`: input or output.
pub fn plgpio_config_pin(cookie: *mut c_void, cells: &[u32], config: i32) {
    let sc = softc(cookie);
    let pin = cells.first().copied().unwrap_or(u32::MAX);

    if pin >= 8 {
        return;
    }

    if config & GPIO_CONFIG_OUTPUT != 0 {
        sc.hset1(GPIODIR, 1 << pin);
    } else {
        sc.hclr1(GPIODIR, 1 << pin);
    }
}

/// `plgpio_get_pin`: the pin's level, inverted for an active-low pin.
pub fn plgpio_get_pin(cookie: *mut c_void, cells: &[u32]) -> i32 {
    let sc = softc(cookie);
    let pin = cells.first().copied().unwrap_or(u32::MAX);
    let flags = cells.get(1).copied().unwrap_or(0);

    if pin >= 8 {
        return 0;
    }

    let reg = sc.hread1(gpiodata(pin));
    pin_value(reg, flags)
}

/// `plgpio_get_pin`'s logic: `!!reg`, inverted when `flags` says active low.
fn pin_value(reg: u8, flags: u32) -> i32 {
    let val = i32::from(reg != 0);
    if flags & GPIO_ACTIVE_LOW != 0 {
        i32::from(val == 0)
    } else {
        val
    }
}

/// `plgpio_set_pin`: drive the pin, inverted for an active-low pin.
pub fn plgpio_set_pin(cookie: *mut c_void, cells: &[u32], val: i32) {
    let sc = softc(cookie);
    let pin = cells.first().copied().unwrap_or(u32::MAX);
    let flags = cells.get(1).copied().unwrap_or(0);

    if pin >= 8 {
        return;
    }

    let val = if flags & GPIO_ACTIVE_LOW != 0 {
        i32::from(val == 0)
    } else {
        val
    };
    if val != 0 {
        sc.hwrite1(gpiodata(pin), 1 << pin);
    } else {
        sc.hwrite1(gpiodata(pin), 0);
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_register_masks_one_pin() {
        // GPIODATA's address bits [9:2] mask the pins a read or write touches.
        assert_eq!(gpiodata(0), 0x004);
        assert_eq!(gpiodata(3), 0x020);
        assert_eq!(gpiodata(7), 0x200);
        assert!(gpiodata(7) < GPIODIR);
    }

    #[test]
    fn active_low_inverts() {
        assert_eq!(pin_value(0x08, 0), 1);
        assert_eq!(pin_value(0, 0), 0);
        assert_eq!(pin_value(0x08, GPIO_ACTIVE_LOW), 0);
        assert_eq!(pin_value(0, GPIO_ACTIVE_LOW), 1);
    }
}
/* </TESTS> */
