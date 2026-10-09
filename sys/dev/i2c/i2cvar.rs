/* <LICENSES> */
/*
 * Copyright (c) 2003 Wasabi Systems, Inc.
 * All rights reserved.
 *
 * Written by Steve C. Woodford and Jason R. Thorpe for Wasabi Systems, Inc.
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
 *      This product includes software developed for the NetBSD Project by
 *      Wasabi Systems, Inc.
 * 4. The name of Wasabi Systems, Inc. may not be used to endorse
 *    or promote products derived from this software without specific prior
 *    written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY WASABI SYSTEMS, INC. ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
 * TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
 * PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL WASABI SYSTEMS, INC
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
//! `<dev/i2c/i2cvar.h>`: the interface between the i2c framework and an i2c controller, and
//! the attach arguments of the bus and of its devices.
//!
//! Upstream: sys/dev/i2c/i2cvar.h @ 3ce1f3f79392
//!
//! A controller fills in an [`I2cController`] (the `i2c_controller` its `i2c_tag_t` points
//! to): an `ic_exec` function for the scripted API, or the low-level `ic_send_start`, ...,
//! `ic_write_byte` ones, with `ic_acquire_bus`/`ic_release_bus` serialising its users. It
//! attaches the bus with an [`I2cbusAttachArgs`] (`config_found(self, &iba, iicbus_print)`),
//! and the `iic` driver (`i2c.rs`) attaches its devices with an [`I2cAttachArgs`] each.
//!
//! ## Deviations
//! - `i2c_tag_t` (a pointer) is [`I2cTag`], a `&'static I2cController`: the controller lives
//!   in its driver's softc, which is never freed while the device exists. The members the
//!   controller sets in its attach are `Cell`s (the softc is made zero-filled).
//! - The function pointers keep their C order and meaning, with slices for the
//!   `(pointer, length)` pairs of `ic_exec` (`cmd`, `buf`) and `&mut u8` for `ic_read_byte`'s
//!   byte. They return the C `int` (0 for success, nonzero for failure), not an `Errno`: the
//!   controllers return a bare 1 and the clients only compare with 0.
//! - A hook a controller left unset is `None` where the C would call through a NULL pointer:
//!   [`iic_acquire_bus`] and [`iic_release_bus`] then do nothing (a bus without locking), the
//!   other hooks fail with `ENXIO`.
//! - `ia_name` is a byte slice instead of a `char *`: the name, or (when `ia_namelen` is not 0)
//!   the concatenation of NUL-terminated names of `ia_namelen` bytes.
//! - `iba_bus_scan` takes the bus attach arguments as a reference instead of the C's `void *`.
//! - The `_I2C_PRIVATE` macros (`iic_send_start`, ..., `iic_write_byte`) are
//!   functions; `iic_scan` and `iic_print` are declared by `i2c_scan.rs` and `i2c.rs`.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr;

use crate::dev::i2c::i2c_io::{I2cAddr, I2cOp};
use crate::sys::device::Device;
use crate::sys::errno::Errno;

/// `I2C_F_WRITE`: new transfer is a write.
pub const I2C_F_WRITE: i32 = 0x00;
/// `I2C_F_READ`: new transfer is a read.
pub const I2C_F_READ: i32 = 0x01;
/// `I2C_F_LAST`: last byte of read.
pub const I2C_F_LAST: i32 = 0x02;
/// `I2C_F_STOP`: send stop after byte.
pub const I2C_F_STOP: i32 = 0x04;
/// `I2C_F_POLL`: poll, don't sleep.
pub const I2C_F_POLL: i32 = 0x08;

/// `ic_acquire_bus`.
pub type IcAcquireBusFn = fn(cookie: *mut c_void, flags: i32) -> i32;
/// `ic_release_bus`.
pub type IcReleaseBusFn = fn(cookie: *mut c_void, flags: i32);
/// `ic_exec`: `(cookie, op, addr, cmd, cmdlen = cmd.len(), buf, buflen = buf.len(), flags)`.
pub type IcExecFn = fn(
    cookie: *mut c_void,
    op: I2cOp,
    addr: I2cAddr,
    cmd: &[u8],
    buf: &mut [u8],
    flags: i32,
) -> i32;
/// `ic_send_start` and `ic_send_stop`.
pub type IcSendFn = fn(cookie: *mut c_void, flags: i32) -> i32;
/// `ic_initiate_xfer`.
pub type IcInitiateXferFn = fn(cookie: *mut c_void, addr: I2cAddr, flags: i32) -> i32;
/// `ic_read_byte`.
pub type IcReadByteFn = fn(cookie: *mut c_void, byte: &mut u8, flags: i32) -> i32;
/// `ic_write_byte`.
pub type IcWriteByteFn = fn(cookie: *mut c_void, byte: u8, flags: i32) -> i32;
/// `ic_intr_establish`.
pub type IcIntrEstablishFn = fn(
    cookie: *mut c_void,
    ih: *mut c_void,
    level: i32,
    func: fn(*mut c_void) -> i32,
    arg: *mut c_void,
    name: &'static str,
) -> *mut c_void;
/// `ic_intr_disestablish`.
pub type IcIntrDisestablishFn = fn(cookie: *mut c_void, ih: *mut c_void);
/// `ic_intr_string`.
pub type IcIntrStringFn = fn(cookie: *mut c_void, ih: *mut c_void) -> &'static str;

/// `struct i2c_controller`: the interface between the i2c framework and the underlying i2c
/// controller.
///
/// Note that this structure is designed specifically to allow us to either use the
/// autoconfiguration framework or not. This allows a driver for a board with a private i2c
/// bus use generic i2c client drivers for chips that might be on that board.
pub struct I2cController {
    /// `ic_cookie`: controller private.
    pub ic_cookie: Cell<*mut c_void>,
    /// `ic_acquire_bus`: synchronization in the presence of multiple users of the i2c bus.
    /// When a device driver wishes to perform transfers on the i2c bus, the driver should
    /// acquire the bus. When the driver is finished, it should release the bus. This is
    /// provided by the back-end since a single controller may present e.g. i2c and smbus
    /// views of the same set of i2c wires.
    pub ic_acquire_bus: Cell<Option<IcAcquireBusFn>>,
    /// `ic_release_bus`.
    pub ic_release_bus: Cell<Option<IcReleaseBusFn>>,
    /// `ic_exec`: the preferred API for clients of the i2c interface is the scripted API.
    /// This handles i2c controllers that do not provide raw access to the i2c signals.
    pub ic_exec: Cell<Option<IcExecFn>>,
    /// `ic_send_start`.
    pub ic_send_start: Cell<Option<IcSendFn>>,
    /// `ic_send_stop`.
    pub ic_send_stop: Cell<Option<IcSendFn>>,
    /// `ic_initiate_xfer`.
    pub ic_initiate_xfer: Cell<Option<IcInitiateXferFn>>,
    /// `ic_read_byte`.
    pub ic_read_byte: Cell<Option<IcReadByteFn>>,
    /// `ic_write_byte`.
    pub ic_write_byte: Cell<Option<IcWriteByteFn>>,
    /// `ic_intr_establish`.
    pub ic_intr_establish: Cell<Option<IcIntrEstablishFn>>,
    /// `ic_intr_disestablish`.
    pub ic_intr_disestablish: Cell<Option<IcIntrDisestablishFn>>,
    /// `ic_intr_string`.
    pub ic_intr_string: Cell<Option<IcIntrStringFn>>,
}

impl I2cController {
    /// A controller with no hooks and a null cookie: what a zero-filled softc holds.
    pub const fn new() -> I2cController {
        I2cController {
            ic_cookie: Cell::new(ptr::null_mut()),
            ic_acquire_bus: Cell::new(None),
            ic_release_bus: Cell::new(None),
            ic_exec: Cell::new(None),
            ic_send_start: Cell::new(None),
            ic_send_stop: Cell::new(None),
            ic_initiate_xfer: Cell::new(None),
            ic_read_byte: Cell::new(None),
            ic_write_byte: Cell::new(None),
            ic_intr_establish: Cell::new(None),
            ic_intr_disestablish: Cell::new(None),
            ic_intr_string: Cell::new(None),
        }
    }
}

impl Default for I2cController {
    fn default() -> I2cController {
        I2cController::new()
    }
}

/// `i2c_tag_t`: the controller, as the framework and the clients hold it.
pub type I2cTag = &'static I2cController;

/// `struct i2cbus_attach_args`: used to attach the i2c framework to the controller.
#[derive(Clone, Copy)]
pub struct I2cbusAttachArgs {
    /// `iba_name`: bus name ("iic").
    pub iba_name: &'static str,
    /// `iba_tag`: the controller.
    pub iba_tag: I2cTag,
    /// `iba_bus_scan`: scans the bus for devices; `None` selects [`iic_scan`](super::i2c_scan::iic_scan).
    pub iba_bus_scan: Option<fn(&Device, &I2cbusAttachArgs, *mut c_void)>,
    /// `iba_bus_scan_arg`.
    pub iba_bus_scan_arg: *mut c_void,
}

impl I2cbusAttachArgs {
    /// `bzero(&iba); iba.iba_name = "iic"; iba.iba_tag = tag;`: what a controller attaches its
    /// bus with.
    pub const fn new(iba_name: &'static str, iba_tag: I2cTag) -> I2cbusAttachArgs {
        I2cbusAttachArgs {
            iba_name,
            iba_tag,
            iba_bus_scan: None,
            iba_bus_scan_arg: ptr::null_mut(),
        }
    }
}

/// `struct i2c_attach_args`: used to attach devices on the i2c bus.
#[derive(Clone, Copy)]
pub struct I2cAttachArgs<'a> {
    /// `ia_tag`: our controller.
    pub ia_tag: I2cTag,
    /// `ia_addr`: address of device.
    pub ia_addr: I2cAddr,
    /// `ia_size`: size (for EEPROMs).
    pub ia_size: i32,
    /// `ia_name`: chip name, or the concatenation of NUL-terminated names `ia_namelen` long.
    pub ia_name: &'a [u8],
    /// `ia_namelen`: length of the name concatenation; 0 when `ia_name` is one name.
    pub ia_namelen: usize,
    /// `ia_cookie`: pass extra info from bus to dev.
    pub ia_cookie: *mut c_void,
    /// `ia_intr`: interrupt info.
    pub ia_intr: *mut c_void,
    /// `ia_poll`: to force polling.
    pub ia_poll: i32,
}

impl<'a> I2cAttachArgs<'a> {
    /// `memset(&ia, 0, sizeof(ia))` and the four members every bus sets: the tag, the address,
    /// the size and a single name.
    pub const fn new(ia_tag: I2cTag, ia_addr: I2cAddr, ia_size: i32, ia_name: &'a [u8]) -> Self {
        I2cAttachArgs {
            ia_tag,
            ia_addr,
            ia_size,
            ia_name,
            ia_namelen: 0,
            ia_cookie: ptr::null_mut(),
            ia_intr: ptr::null_mut(),
            ia_poll: 0,
        }
    }
}

/// The error of a hook the controller did not provide.
const fn nohook() -> i32 {
    Errno::ENXIO as i32
}

/// `iic_acquire_bus(ic, flags)`: gains the exclusive use of the bus (no-op without a hook).
pub fn iic_acquire_bus(ic: I2cTag, flags: i32) -> i32 {
    match ic.ic_acquire_bus.get() {
        Some(f) => f(ic.ic_cookie.get(), flags),
        None => 0,
    }
}

/// `iic_release_bus(ic, flags)`.
pub fn iic_release_bus(ic: I2cTag, flags: i32) {
    if let Some(f) = ic.ic_release_bus.get() {
        f(ic.ic_cookie.get(), flags);
    }
}

/// `iic_send_start(ic, flags)` (`_I2C_PRIVATE`).
pub fn iic_send_start(ic: I2cTag, flags: i32) -> i32 {
    match ic.ic_send_start.get() {
        Some(f) => f(ic.ic_cookie.get(), flags),
        None => nohook(),
    }
}

/// `iic_send_stop(ic, flags)` (`_I2C_PRIVATE`).
pub fn iic_send_stop(ic: I2cTag, flags: i32) -> i32 {
    match ic.ic_send_stop.get() {
        Some(f) => f(ic.ic_cookie.get(), flags),
        None => nohook(),
    }
}

/// `iic_initiate_xfer(ic, addr, flags)` (`_I2C_PRIVATE`).
pub fn iic_initiate_xfer(ic: I2cTag, addr: I2cAddr, flags: i32) -> i32 {
    match ic.ic_initiate_xfer.get() {
        Some(f) => f(ic.ic_cookie.get(), addr, flags),
        None => nohook(),
    }
}

/// `iic_read_byte(ic, bytep, flags)` (`_I2C_PRIVATE`).
pub fn iic_read_byte(ic: I2cTag, byte: &mut u8, flags: i32) -> i32 {
    match ic.ic_read_byte.get() {
        Some(f) => f(ic.ic_cookie.get(), byte, flags),
        None => nohook(),
    }
}

/// `iic_write_byte(ic, byte, flags)` (`_I2C_PRIVATE`).
pub fn iic_write_byte(ic: I2cTag, byte: u8, flags: i32) -> i32 {
    match ic.ic_write_byte.get() {
        Some(f) => f(ic.ic_cookie.get(), byte, flags),
        None => nohook(),
    }
}

/// `iic_intr_establish(ic, ih, level, func, arg, name)`: `None` when the controller has no
/// interrupt hook.
pub fn iic_intr_establish(
    ic: I2cTag,
    ih: *mut c_void,
    level: i32,
    func: fn(*mut c_void) -> i32,
    arg: *mut c_void,
    name: &'static str,
) -> Option<*mut c_void> {
    let f = ic.ic_intr_establish.get()?;
    Some(f(ic.ic_cookie.get(), ih, level, func, arg, name))
}

/// `iic_intr_disestablish(ic, ih)`.
pub fn iic_intr_disestablish(ic: I2cTag, ih: *mut c_void) {
    if let Some(f) = ic.ic_intr_disestablish.get() {
        f(ic.ic_cookie.get(), ih);
    }
}

/// `iic_intr_string(ic, ih)`: `None` when the controller has no interrupt hook.
pub fn iic_intr_string(ic: I2cTag, ih: *mut c_void) -> Option<&'static str> {
    let f = ic.ic_intr_string.get()?;
    Some(f(ic.ic_cookie.get(), ih))
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;
    use crate::dev::i2c::i2c_io::I2C_OP_READ;

    fn leak(ic: I2cController) -> I2cTag {
        std::boxed::Box::leak(std::boxed::Box::new(ic))
    }

    #[test]
    fn flags_match_the_header_values() {
        assert_eq!(
            [I2C_F_WRITE, I2C_F_READ, I2C_F_LAST, I2C_F_STOP, I2C_F_POLL],
            [0, 1, 2, 4, 8]
        );
    }

    #[test]
    fn a_controller_without_hooks_locks_nothing_and_refuses_the_rest() {
        let ic = leak(I2cController::new());
        assert_eq!(iic_acquire_bus(ic, 0), 0);
        iic_release_bus(ic, 0);
        assert_eq!(iic_send_start(ic, 0), Errno::ENXIO as i32);
        assert_eq!(iic_send_stop(ic, 0), Errno::ENXIO as i32);
        assert_eq!(iic_initiate_xfer(ic, 0x50, 0), Errno::ENXIO as i32);
        assert_eq!(iic_write_byte(ic, 1, 0), Errno::ENXIO as i32);
        let mut b = 0;
        assert_eq!(iic_read_byte(ic, &mut b, 0), Errno::ENXIO as i32);
        assert!(iic_intr_string(ic, ptr::null_mut()).is_none());
        assert!(ic.ic_exec.get().is_none());
        let _ = I2C_OP_READ;
    }

    #[test]
    fn hooks_receive_the_cookie() {
        fn acquire(cookie: *mut c_void, flags: i32) -> i32 {
            // SAFETY: the test passes a `*mut i32` as the cookie.
            unsafe { *cookie.cast::<i32>() += flags };
            0
        }
        let mut n = 40;
        let ic = leak(I2cController::new());
        ic.ic_cookie.set(ptr::from_mut(&mut n).cast());
        ic.ic_acquire_bus.set(Some(acquire));
        assert_eq!(iic_acquire_bus(ic, 2), 0);
        assert_eq!(n, 42);
    }

    #[test]
    fn attach_args_start_zeroed() {
        let ic = leak(I2cController::new());
        let ia = I2cAttachArgs::new(ic, 0x50, 1, b"eeprom");
        assert_eq!(
            (ia.ia_addr, ia.ia_size, ia.ia_namelen, ia.ia_poll),
            (0x50, 1, 0, 0)
        );
        assert!(ia.ia_cookie.is_null() && ia.ia_intr.is_null());
        let iba = I2cbusAttachArgs::new("iic", ic);
        assert_eq!(iba.iba_name, "iic");
        assert!(iba.iba_bus_scan.is_none() && iba.iba_bus_scan_arg.is_null());
    }
}
/* </TESTS> */
