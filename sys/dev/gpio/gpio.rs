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
 * Copyright (c) 2008 Marc Balmer <mbalmer@openbsd.org>
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
//! The General Purpose Input/Output framework, gpio(4): `dev/gpio/gpio.c`.
//!
//! Upstream: sys/dev/gpio/gpio.c @ 3ce1f3f79392
//!
//! A GPIO controller attaches `gpio*` with a [`GpiobusAttachArgs`]; the bus attaches the
//! drivers its kernel configuration puts on its pins (`gpio_search`) and those `GPIOATTACH`
//! names, and `/dev/gpioN` (`cdevsw[88]`) reads, writes, toggles, configures and names pins.
//! The arm64 GENERIC lines that attach it (`gpio* at bcmgpio?`, `gpio* at sxipio?`) are for
//! controllers that are not ported, and QEMU's PL061 (`plgpio`) attaches no bus, so nothing
//! attaches `gpio*` yet; the file is here with its device switch entry and host tests.
//!
//! ## Deviations
//! - `struct gpio_softc` is [`GpioSoftc`] of `Cell`s, as autoconfiguration allocates it
//!   zeroed; `sc_gc` and `sc_pins` are the controller's `'static` tag and array, kept as
//!   pointers ([`GpioSoftc::gc`], [`GpioSoftc::pins`]).
//! - `gpio_pin_map` returns `Err(EINVAL)` where the C returns 1; a mask with more pins than
//!   the child's map has slots fails too (the C writes past it).
//! - `GPIOATTACH` hands `config_found_sm` [`gpioattach_print`], which prints the requested
//!   driver's name, where the C passes `gpiobus_print`, which would read a
//!   `struct gpio_attach_args` as a `struct gpiobus_attach_args` and print garbage.
//! - `gpio_dev` and `gpio_name` elements are `Box`es (`malloc`/`free` with `M_DEVBUF`).
//! - Pin names are compared and copied up to their NUL within `GPIOPINMAXNAME`, as
//!   `strcmp`/`strlcpy` do on terminated names; an unterminated user name is taken whole.

use alloc::boxed::Box;
use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::sync::atomic::Ordering;

use crate::dev::gpio::gpiovar::{
    GpioAttachArgs, GpioChipsetTag, GpioDev, GpioDevList, GpioName, GpioNameList, GpioPin,
    GpioPinmap, GpiobusAttachArgs,
};
use crate::kern::kern_sysctl::SECURELEVEL;
use crate::kern::subr_autoconf::{
    config_attach, config_detach, config_found_sm, config_search, device_lookup, device_unref,
};
use crate::kern::subr_prf::{Str, printf};
use crate::kern::vfs_subr::vdevgone;
use crate::machine::conf::{cdevsw, nchrdev};
use crate::sys::conf::DevTypeOpen;
use crate::sys::device::{CfMatch, Cfattach, Cfdriver, DV_DULL, Device, Softc, UNCONF};
use crate::sys::errno::Errno;
use crate::sys::fcntl::FWRITE;
use crate::sys::gpio::{
    GPIO_PIN_HIGH, GPIO_PIN_LOW, GPIO_PIN_SET, GPIOATTACH, GPIODETACH, GPIOINFO, GPIOPINMAXNAME,
    GPIOPINREAD, GPIOPINSET, GPIOPINTOGGLE, GPIOPINUNSET, GPIOPINWRITE, GpioAttach, GpioInfo,
    GpioPinOp, GpioPinSet,
};
use crate::sys::ioctl::{ioctl_arg, ioctl_ret};
use crate::sys::proc::Proc;
use crate::sys::queue::ListHead;
use crate::sys::types::{Dev, minor};
use crate::sys::vnode::VCHR;

/// `NGPIO`: the count `config(8)` writes into `gpio.h` for `gpio*` (nonzero in both
/// GENERICs; only whether it is zero matters, to `cdev_gpio_init`).
pub const NGPIO: i32 = 1;

/// `struct gpio_softc`.
#[repr(C)]
pub struct GpioSoftc {
    /// `sc_dev`.
    pub sc_dev: Device,
    /// `sc_gc`: the GPIO controller.
    sc_gc: Cell<*const GpioChipsetTag>,
    /// `sc_pins`: the pins array.
    sc_pins: Cell<*const GpioPin>,
    /// `sc_npins`: number of pins.
    sc_npins: Cell<i32>,
    /// `sc_opened`.
    sc_opened: Cell<i32>,
    /// `sc_devs`: devices.
    sc_devs: ListHead<GpioDevList>,
    /// `sc_names`: named pins.
    sc_names: ListHead<GpioNameList>,
}

// SAFETY: `#[repr(C)]` with the `struct device` first; every other member is a `Cell` of an
// integer or a pointer, or a list head of one, all valid all-zero.
unsafe impl Softc for GpioSoftc {}

impl GpioSoftc {
    /// `sc->sc_gc`, once attached.
    fn gc(&self) -> Option<&'static GpioChipsetTag> {
        // SAFETY: `gpio_attach` stores the controller's `'static` tag, or nothing.
        unsafe { self.sc_gc.get().as_ref() }
    }

    /// `sc->sc_pins[0 .. sc->sc_npins]`.
    fn pins(&self) -> &'static [GpioPin] {
        let p = self.sc_pins.get();
        if p.is_null() {
            return &[];
        }
        // SAFETY: `gpio_attach` stores a `'static` slice's pointer and its length.
        unsafe { core::slice::from_raw_parts(p, self.sc_npins.get().max(0) as usize) }
    }

    /// `&sc->sc_pins[pin]`, if `pin` is in range.
    fn pin(&self, pin: i32) -> Option<&'static GpioPin> {
        usize::try_from(pin).ok().and_then(|i| self.pins().get(i))
    }

    /// The controller's `gpiobus_pin_read`.
    fn pin_read(&self, pin: i32) -> i32 {
        self.gc().map_or(0, |gc| gc.gpiobus_pin_read(pin))
    }

    /// The controller's `gpiobus_pin_write`.
    fn pin_write(&self, pin: i32, value: i32) {
        if let Some(gc) = self.gc() {
            gc.gpiobus_pin_write(pin, value);
        }
    }

    /// The controller's `gpiobus_pin_ctl`.
    fn pin_ctl(&self, pin: i32, flags: i32) {
        if let Some(gc) = self.gc() {
            gc.gpiobus_pin_ctl(pin, flags);
        }
    }
}

/// `gpio_ca`.
pub static GPIO_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<GpioSoftc>(),
    ca_match: Some(gpio_match),
    ca_attach: gpio_attach,
    ca_detach: Some(gpio_detach),
    ca_activate: None,
};

/// `gpio_cd`.
pub static GPIO_CD: Cfdriver = Cfdriver::new(b"gpio", DV_DULL, 0);

/// A NUL-terminated name in a fixed buffer, without the NUL (`strcmp`'s view of it).
fn cname(b: &[u8]) -> &[u8] {
    let n = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    &b[..n]
}

/// `gpio_match`: the bus whose name is the driver's.
pub fn gpio_match(_parent: Option<&Device>, match_: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: a GPIO controller hands its children a `struct gpiobus_attach_args`.
    let gba = unsafe { &*aux.cast::<GpiobusAttachArgs>() };

    i32::from(gba.gba_name == match_.cfdata().cf_driver.cd_name)
}

/// `gpio_submatch`: the driver `GPIOATTACH` named, if its own match agrees.
pub fn gpio_submatch(parent: Option<&Device>, match_: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: `GPIOATTACH` hands `config_found_sm` a `struct gpio_attach_args`.
    let ga = unsafe { &*aux.cast::<GpioAttachArgs<'_>>() };
    let cf = match_.cfdata();

    if ga.ga_dvname != cf.cf_driver.cd_name {
        return 0;
    }

    cf.cf_attach.ca_match.map_or(0, |m| m(parent, match_, aux))
}

/// `gpio_attach`: take the controller's pins and attach what the configuration puts on them.
pub fn gpio_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: `gpio_ca` makes `GpioSoftc`s.
    let sc = unsafe { self_.softc::<GpioSoftc>() };
    // SAFETY: as in `gpio_match`.
    let gba = unsafe { &*aux.cast::<GpiobusAttachArgs>() };

    sc.sc_gc.set(ptr::from_ref(gba.gba_gc));
    sc.sc_pins.set(gba.gba_pins.as_ptr());
    sc.sc_npins
        .set(gba.gba_npins.min(gba.gba_pins.len() as i32));

    printf(format_args!(": {} pins\n", sc.sc_npins.get()));

    /*
     * Attach all devices that can be connected to the GPIO pins
     * described in the kernel configuration file.
     */
    config_search(
        Some(gpio_search),
        self_,
        ptr::from_ref(sc).cast_mut().cast::<c_void>(),
    );
}

/// `gpio_detach`: revoke the open instances of `/dev/gpioN`.
pub fn gpio_detach(self_: &Device, _flags: i32) -> Result<(), Errno> {
    /* Locate the major number */
    let maj = (0..nchrdev())
        .find(|&maj| ptr::fn_addr_eq(cdevsw(maj).d_open, gpioopen as DevTypeOpen))
        .unwrap_or(nchrdev());

    /* Nuke the vnodes for any open instances (calls close) */
    let mn = self_.dv_unit.get() as u32;
    vdevgone(maj, mn, mn, VCHR);

    Ok(())
}

/// `gpio_search`: attach a configured child if its driver matches its pins.
pub fn gpio_search(parent: Option<&Device>, match_: &CfMatch, aux: *mut c_void) -> i32 {
    let cf = match_.cfdata();
    let loc = |i: usize| cf.cf_loc.get(i).copied().unwrap_or(0);
    let mut ga = GpioAttachArgs {
        ga_gpio: aux,
        ga_offset: loc(0) as i32,
        ga_mask: loc(1) as u32,
        ga_dvname: b"",
        ga_flags: loc(2) as u32,
    };
    let gap = ptr::from_mut(&mut ga).cast::<c_void>();

    if cf.cf_attach.ca_match.map_or(0, |m| m(parent, match_, gap)) > 0 {
        config_attach(parent, CfMatch::Cfdata(cf), gap, Some(gpio_print));
    }

    0
}

/// `gpio_print`: the pins a child uses.
pub fn gpio_print(aux: *mut c_void, _pnp: Option<&[u8]>) -> i32 {
    // SAFETY: `gpio_search` attaches with a `struct gpio_attach_args`.
    let ga = unsafe { &*aux.cast::<GpioAttachArgs<'_>>() };

    printf(format_args!(" pins"));
    for i in 0..32 {
        if ga.ga_mask & (1 << i) != 0 {
            printf(format_args!(" {}", ga.ga_offset + i));
        }
    }

    UNCONF
}

/// `gpiobus_print`: a controller's bus that did not attach.
pub fn gpiobus_print(aux: *mut c_void, pnp: Option<&[u8]>) -> i32 {
    // SAFETY: the controllers attach `gpio*` with a `struct gpiobus_attach_args`.
    let gba = unsafe { &*aux.cast::<GpiobusAttachArgs>() };

    if let Some(pnp) = pnp {
        printf(format_args!("{} at {}", Str(gba.gba_name), Str(pnp)));
    }

    UNCONF
}

/// What `GPIOATTACH` prints when no driver took the device (see the deviations).
fn gpioattach_print(aux: *mut c_void, pnp: Option<&[u8]>) -> i32 {
    // SAFETY: `GPIOATTACH` hands `config_found_sm` a `struct gpio_attach_args`.
    let ga = unsafe { &*aux.cast::<GpioAttachArgs<'_>>() };

    if let Some(pnp) = pnp {
        printf(format_args!("{} at {}", Str(ga.ga_dvname), Str(pnp)));
    }

    UNCONF
}

/// `gpio_pin_map`: reserve the pins of `mask` from `offset` for a child.
pub fn gpio_pin_map(
    sc: &GpioSoftc,
    offset: i32,
    mask: u32,
    map: &mut GpioPinmap<'_>,
) -> Result<(), Errno> {
    if gpio_npins(mask) > sc.sc_npins.get() || gpio_npins(mask) as usize > map.pm_map.len() {
        return Err(Errno::EINVAL);
    }

    let mut npins = 0;
    for i in 0..32 {
        if mask & (1 << i) != 0 {
            let pin = offset + i;
            let Some(p) = sc.pin(pin) else {
                return Err(Errno::EINVAL);
            };
            if p.pin_mapped.get() != 0 {
                return Err(Errno::EINVAL);
            }
            p.pin_mapped.set(1);
            map.pm_map[npins] = pin;
            npins += 1;
        }
    }
    map.pm_size = npins as i32;

    Ok(())
}

/// `gpio_pin_unmap`: give a child's pins back.
pub fn gpio_pin_unmap(sc: &GpioSoftc, map: &GpioPinmap<'_>) {
    for &pin in map.pm_map.iter().take(map.pm_size.max(0) as usize) {
        if let Some(p) = sc.pin(pin) {
            p.pin_mapped.set(0);
        }
    }
}

/// The controller's pin for a child's map index.
fn mapped(map: &GpioPinmap<'_>, pin: i32) -> i32 {
    usize::try_from(pin)
        .ok()
        .and_then(|i| map.pm_map.get(i).copied())
        .unwrap_or(-1)
}

/// `gpio_pin_read`.
pub fn gpio_pin_read(sc: &GpioSoftc, map: &GpioPinmap<'_>, pin: i32) -> i32 {
    sc.pin_read(mapped(map, pin))
}

/// `gpio_pin_write`.
pub fn gpio_pin_write(sc: &GpioSoftc, map: &GpioPinmap<'_>, pin: i32, value: i32) {
    sc.pin_write(mapped(map, pin), value);
}

/// `gpio_pin_ctl`.
pub fn gpio_pin_ctl(sc: &GpioSoftc, map: &GpioPinmap<'_>, pin: i32, flags: i32) {
    sc.pin_ctl(mapped(map, pin), flags);
}

/// `gpio_pin_caps`.
pub fn gpio_pin_caps(sc: &GpioSoftc, map: &GpioPinmap<'_>, pin: i32) -> i32 {
    sc.pin(mapped(map, pin)).map_or(0, |p| p.pin_caps.get())
}

/// `gpio_npins`: how many bits of `mask` are set.
pub fn gpio_npins(mask: u32) -> i32 {
    mask.count_ones() as i32
}

/// A reference to an attached gpio(4) device, from `device_lookup`, given back with
/// `device_unref` when dropped.
struct GpioRef(NonNull<Device>);

impl GpioRef {
    /// `(struct gpio_softc *)device_lookup(&gpio_cd, unit)`.
    fn lookup(dev: Dev) -> Option<Self> {
        device_lookup(&GPIO_CD, minor(dev) as i32).map(Self)
    }

    /// The softc.
    fn sc(&self) -> &GpioSoftc {
        // SAFETY: `gpio_cd`'s devices are made by `gpio_ca`; the reference this holds keeps
        // the allocation alive.
        unsafe { self.0.as_ref().softc::<GpioSoftc>() }
    }
}

impl Drop for GpioRef {
    fn drop(&mut self) {
        // SAFETY: the reference `device_lookup` took.
        unsafe { device_unref(self.0) };
    }
}

/// `gpioopen`: one opener at a time.
pub fn gpioopen(dev: Dev, _flag: i32, _mode: i32, _p: &Proc) -> Result<(), Errno> {
    let r = GpioRef::lookup(dev).ok_or(Errno::ENXIO)?;
    let sc = r.sc();

    if sc.sc_opened.get() != 0 {
        return Err(Errno::EBUSY);
    }
    sc.sc_opened.set(1);

    Ok(())
}

/// `gpioclose`.
pub fn gpioclose(dev: Dev, _flag: i32, _mode: i32, _p: Option<&Proc>) -> Result<(), Errno> {
    let r = GpioRef::lookup(dev).ok_or(Errno::ENXIO)?;

    r.sc().sc_opened.set(0);

    Ok(())
}

/// `gpio_pinbyname`: the pin a name was given, or -1.
pub fn gpio_pinbyname(sc: &GpioSoftc, gp_name: &[u8]) -> i32 {
    let want = cname(gp_name);
    sc.sc_names
        .iter()
        .find(|nm| cname(&nm.gp_name.get()) == want)
        .map_or(-1, |nm| nm.gp_pin.get())
}

/// The pin an operation names: by `gp_name` if set, else `gp_pin`.
fn pin_of(sc: &GpioSoftc, gp_name: &[u8], gp_pin: i32) -> Result<i32, Errno> {
    if gp_name.first().is_some_and(|&c| c != 0) {
        match gpio_pinbyname(sc, gp_name) {
            -1 => Err(Errno::EINVAL),
            pin => Ok(pin),
        }
    } else {
        Ok(gp_pin)
    }
}

/// `strlcpy(dst, src, GPIOPINMAXNAME)` of a terminated name.
fn copy_name(src: &[u8]) -> [u8; GPIOPINMAXNAME] {
    let mut dst = [0u8; GPIOPINMAXNAME];
    let s = cname(src);
    let n = s.len().min(GPIOPINMAXNAME - 1);
    dst[..n].copy_from_slice(&s[..n]);
    dst
}

/// `gpio_ioctl`: the `GPIO*` commands on one gpio(4) device.
pub fn gpio_ioctl(sc: &GpioSoftc, cmd: u64, data: &mut [u8], flag: i32) -> Result<(), Errno> {
    let securelevel = SECURELEVEL.load(Ordering::Relaxed);

    match cmd {
        GPIOINFO => {
            let npins = if securelevel < 1 {
                sc.sc_npins.get()
            } else {
                sc.pins()
                    .iter()
                    .filter(|p| p.pin_flags.get() & GPIO_PIN_SET != 0)
                    .count() as i32
            };
            ioctl_ret(&mut *data, &GpioInfo { gpio_npins: npins });
        }
        GPIOPINREAD => {
            let mut op: GpioPinOp = ioctl_arg(data);
            let pin = pin_of(sc, &op.gp_name, op.gp_pin)?;
            let p = sc.pin(pin).ok_or(Errno::EINVAL)?;

            if p.pin_flags.get() & GPIO_PIN_SET == 0 && securelevel > 0 {
                return Err(Errno::EPERM);
            }

            /* return read value */
            op.gp_value = sc.pin_read(pin);
            ioctl_ret(&mut *data, &op);
        }
        GPIOPINWRITE | GPIOPINTOGGLE => {
            if flag & FWRITE == 0 {
                return Err(Errno::EBADF);
            }

            let mut op: GpioPinOp = ioctl_arg(data);
            let pin = pin_of(sc, &op.gp_name, op.gp_pin)?;
            let p = sc.pin(pin).ok_or(Errno::EINVAL)?;

            if p.pin_mapped.get() != 0 {
                return Err(Errno::EBUSY);
            }

            if p.pin_flags.get() & GPIO_PIN_SET == 0 && securelevel > 0 {
                return Err(Errno::EPERM);
            }

            let value = if cmd == GPIOPINWRITE {
                if op.gp_value != GPIO_PIN_LOW && op.gp_value != GPIO_PIN_HIGH {
                    return Err(Errno::EINVAL);
                }
                op.gp_value
            } else if p.pin_state.get() == GPIO_PIN_LOW {
                GPIO_PIN_HIGH
            } else {
                GPIO_PIN_LOW
            };

            sc.pin_write(pin, value);
            /* return old value */
            op.gp_value = p.pin_state.get();
            /* update current value */
            p.pin_state.set(value);
            ioctl_ret(&mut *data, &op);
        }
        GPIOATTACH => {
            if securelevel > 0 {
                return Err(Errno::EPERM);
            }

            let attach: GpioAttach = ioctl_arg(data);
            let mut ga = GpioAttachArgs {
                ga_gpio: ptr::from_ref(sc).cast_mut().cast::<c_void>(),
                ga_offset: attach.ga_offset,
                ga_mask: attach.ga_mask,
                ga_dvname: cname(&attach.ga_dvname),
                ga_flags: attach.ga_flags,
            };
            let dv = config_found_sm(
                &sc.sc_dev,
                ptr::from_mut(&mut ga).cast::<c_void>(),
                Some(gpioattach_print),
                Some(gpio_submatch),
            );
            if let Some(dv) = dv {
                let gdev: &'static GpioDev = Box::leak(Box::new(GpioDev {
                    sc_dev: dv,
                    sc_next: Default::default(),
                }));
                // SAFETY: a fresh element that lives until GPIODETACH unlinks and frees it.
                unsafe { sc.sc_devs.insert_head(gdev) };
            }
        }
        GPIODETACH => {
            if securelevel > 0 {
                return Err(Errno::EPERM);
            }

            let attach: GpioAttach = ioctl_arg(data);
            let want = cname(&attach.ga_dvname);
            let found = sc.sc_devs.iter().find(|gdev| {
                // SAFETY: an attached device stays allocated while it is on the list.
                cname(&unsafe { gdev.sc_dev.as_ref() }.dv_xname.get()) == want
            });
            if let Some(gdev) = found {
                // SAFETY: `gdev.sc_dev` is the attached device GPIOATTACH recorded.
                if unsafe { config_detach(gdev.sc_dev, 0) }.is_ok() {
                    // SAFETY: on `sc_devs`; the element was leaked from a `Box` by
                    // GPIOATTACH and nothing else refers to it once unlinked.
                    unsafe {
                        ListHead::<GpioDevList>::remove(gdev);
                        drop(Box::from_raw(ptr::from_ref(gdev).cast_mut()));
                    }
                }
            }
        }
        GPIOPINSET => {
            if securelevel > 0 {
                return Err(Errno::EPERM);
            }

            let mut set: GpioPinSet = ioctl_arg(data);
            let pin = pin_of(sc, &set.gp_name, set.gp_pin)?;
            let p = sc.pin(pin).ok_or(Errno::EINVAL)?;
            let flags = set.gp_flags;
            /* check that the controller supports all requested flags */
            if flags & p.pin_caps.get() != flags {
                return Err(Errno::ENODEV);
            }
            let flags = set.gp_flags | GPIO_PIN_SET;

            set.gp_caps = p.pin_caps.get();
            /* return old value */
            set.gp_flags = p.pin_flags.get();
            if flags > 0 {
                sc.pin_ctl(pin, flags);
                /* update current value */
                p.pin_flags.set(flags);
            }

            /* rename pin or new pin? */
            if set.gp_name2[0] != 0 {
                match sc.sc_names.iter().find(|nm| nm.gp_pin.get() == pin) {
                    Some(nm) => nm.gp_name.set(copy_name(&set.gp_name2)),
                    None => {
                        let nm: &'static GpioName = Box::leak(Box::new(GpioName {
                            gp_name: Cell::new(copy_name(&set.gp_name2)),
                            gp_pin: Cell::new(set.gp_pin),
                            gp_next: Default::default(),
                        }));
                        // SAFETY: a fresh element that lives until GPIOPINUNSET frees it.
                        unsafe { sc.sc_names.insert_head(nm) };
                    }
                }
            }
            ioctl_ret(&mut *data, &set);
        }
        GPIOPINUNSET => {
            if securelevel > 0 {
                return Err(Errno::EPERM);
            }

            let set: GpioPinSet = ioctl_arg(data);
            let pin = pin_of(sc, &set.gp_name, set.gp_pin)?;
            let p = sc.pin(pin).ok_or(Errno::EINVAL)?;
            if p.pin_mapped.get() != 0 {
                return Err(Errno::EBUSY);
            }
            if p.pin_flags.get() & GPIO_PIN_SET == 0 {
                return Err(Errno::EINVAL);
            }

            if let Some(nm) = sc.sc_names.iter().find(|nm| nm.gp_pin.get() == pin) {
                // SAFETY: on `sc_names`; leaked from a `Box` by GPIOPINSET, and nothing else
                // refers to it once unlinked.
                unsafe {
                    ListHead::<GpioNameList>::remove(nm);
                    drop(Box::from_raw(ptr::from_ref(nm).cast_mut()));
                }
            }
            p.pin_flags.set(p.pin_flags.get() & !GPIO_PIN_SET);
        }
        _ => return Err(Errno::ENOTTY),
    }

    Ok(())
}

/// `gpioioctl`.
pub fn gpioioctl(dev: Dev, cmd: u64, data: &mut [u8], flag: i32, _p: &Proc) -> Result<(), Errno> {
    let r = GpioRef::lookup(dev).ok_or(Errno::ENXIO)?;

    gpio_ioctl(r.sc(), cmd, data, flag)
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::vec;
    use std::vec::Vec;

    use super::*;

    /// SECURELEVEL is global: the tests that change it take this lock.
    static LEVEL: Mutex<()> = Mutex::new(());

    /// A fake controller: eight pins, its levels in a cell, its last ctl flags per pin.
    struct Fake {
        levels: Cell<u32>,
        ctl: [Cell<i32>; 8],
    }

    fn fake_read(cookie: *mut c_void, pin: i32) -> i32 {
        // SAFETY: the tests' cookie is a `Fake`.
        let f = unsafe { &*cookie.cast::<Fake>() };
        ((f.levels.get() >> pin) & 1) as i32
    }

    fn fake_write(cookie: *mut c_void, pin: i32, value: i32) {
        // SAFETY: as in `fake_read`.
        let f = unsafe { &*cookie.cast::<Fake>() };
        let bit = 1 << pin;
        f.levels.set(if value != 0 {
            f.levels.get() | bit
        } else {
            f.levels.get() & !bit
        });
    }

    fn fake_ctl(cookie: *mut c_void, pin: i32, flags: i32) {
        // SAFETY: as in `fake_read`.
        let f = unsafe { &*cookie.cast::<Fake>() };
        f.ctl[pin as usize].set(flags);
    }

    /// A gpio softc attached (by hand) to a fresh fake controller.
    fn attached() -> (Box<GpioSoftc>, &'static Fake) {
        let fake: &'static Fake = Box::leak(Box::new(Fake {
            levels: Cell::new(0),
            ctl: Default::default(),
        }));
        let gc: &'static GpioChipsetTag = Box::leak(Box::new(GpioChipsetTag {
            gp_cookie: ptr::from_ref(fake).cast_mut().cast(),
            gp_pin_read: Some(fake_read),
            gp_pin_write: Some(fake_write),
            gp_pin_ctl: Some(fake_ctl),
        }));
        let pins: &'static [GpioPin] = Box::leak(
            (0..8)
                .map(|i| {
                    let p = GpioPin::default();
                    p.pin_num.set(i);
                    p.pin_caps.set(0x3);
                    p
                })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        );
        // SAFETY: all-zero is a valid `GpioSoftc` (`unsafe impl Softc`).
        let sc = unsafe { Box::<GpioSoftc>::new_zeroed().assume_init() };
        sc.sc_gc.set(gc);
        sc.sc_pins.set(pins.as_ptr());
        sc.sc_npins.set(8);
        (sc, fake)
    }

    fn name(s: &str) -> [u8; GPIOPINMAXNAME] {
        copy_name(s.as_bytes())
    }

    fn op_bytes(op: &GpioPinOp) -> Vec<u8> {
        let mut b = vec![0u8; size_of::<GpioPinOp>()];
        ioctl_ret(&mut b, op);
        b
    }

    fn set_bytes(set: &GpioPinSet) -> Vec<u8> {
        let mut b = vec![0u8; size_of::<GpioPinSet>()];
        ioctl_ret(&mut b, set);
        b
    }

    #[test]
    fn npins_counts_bits() {
        assert_eq!(gpio_npins(0), 0);
        assert_eq!(gpio_npins(0b1011), 3);
        assert_eq!(gpio_npins(u32::MAX), 32);
    }

    #[test]
    fn pin_map_and_unmap() {
        let (sc, fake) = attached();
        let mut slots = [0i32; 4];
        let mut map = GpioPinmap {
            pm_map: &mut slots,
            pm_size: 0,
        };
        gpio_pin_map(&sc, 2, 0b101, &mut map).unwrap();
        assert_eq!(map.pm_size, 2);
        assert_eq!(&map.pm_map[..2], &[2, 4]);
        assert_eq!(sc.pins()[2].pin_mapped.get(), 1);
        // Already mapped, out of range, more pins than slots.
        let mut other = [0i32; 4];
        let mut m2 = GpioPinmap {
            pm_map: &mut other,
            pm_size: 0,
        };
        assert_eq!(gpio_pin_map(&sc, 4, 1, &mut m2), Err(Errno::EINVAL));
        assert_eq!(gpio_pin_map(&sc, 7, 0b11, &mut m2), Err(Errno::EINVAL));
        let mut one = [0i32; 1];
        let mut m3 = GpioPinmap {
            pm_map: &mut one,
            pm_size: 0,
        };
        assert_eq!(gpio_pin_map(&sc, 0, 0b11, &mut m3), Err(Errno::EINVAL));
        // Through the map: write, read, ctl, caps.
        gpio_pin_write(&sc, &map, 1, 1);
        assert_eq!(fake.levels.get(), 1 << 4);
        assert_eq!(gpio_pin_read(&sc, &map, 1), 1);
        gpio_pin_ctl(&sc, &map, 0, 0x2);
        assert_eq!(fake.ctl[2].get(), 0x2);
        assert_eq!(gpio_pin_caps(&sc, &map, 0), 0x3);
        gpio_pin_unmap(&sc, &map);
        assert_eq!(sc.pins()[2].pin_mapped.get(), 0);
        assert_eq!(sc.pins()[4].pin_mapped.get(), 0);
    }

    #[test]
    fn ioctl_info_read_write_toggle() {
        let _g = LEVEL.lock().unwrap();
        SECURELEVEL.store(0, Ordering::Relaxed);
        let (sc, fake) = attached();

        let mut info = [0u8; 4];
        gpio_ioctl(&sc, GPIOINFO, &mut info, 0).unwrap();
        assert_eq!(ioctl_arg::<GpioInfo>(&info).gpio_npins, 8);

        let mut b = op_bytes(&GpioPinOp {
            gp_name: [0; 64],
            gp_pin: 3,
            gp_value: 1,
        });
        assert_eq!(gpio_ioctl(&sc, GPIOPINWRITE, &mut b, 0), Err(Errno::EBADF));
        gpio_ioctl(&sc, GPIOPINWRITE, &mut b, FWRITE).unwrap();
        assert_eq!(ioctl_arg::<GpioPinOp>(&b).gp_value, 0); // the old state
        assert_eq!(fake.levels.get(), 1 << 3);

        gpio_ioctl(&sc, GPIOPINTOGGLE, &mut b, FWRITE).unwrap();
        assert_eq!(ioctl_arg::<GpioPinOp>(&b).gp_value, 1);
        assert_eq!(fake.levels.get(), 0);

        let mut r = op_bytes(&GpioPinOp {
            gp_name: [0; 64],
            gp_pin: 3,
            gp_value: 0,
        });
        fake.levels.set(1 << 3);
        gpio_ioctl(&sc, GPIOPINREAD, &mut r, 0).unwrap();
        assert_eq!(ioctl_arg::<GpioPinOp>(&r).gp_value, 1);

        let mut bad = op_bytes(&GpioPinOp {
            gp_name: [0; 64],
            gp_pin: 8,
            gp_value: 0,
        });
        assert_eq!(
            gpio_ioctl(&sc, GPIOPINREAD, &mut bad, 0),
            Err(Errno::EINVAL)
        );
        let mut two = op_bytes(&GpioPinOp {
            gp_name: [0; 64],
            gp_pin: 1,
            gp_value: 2,
        });
        assert_eq!(
            gpio_ioctl(&sc, GPIOPINWRITE, &mut two, FWRITE),
            Err(Errno::EINVAL)
        );
        assert_eq!(gpio_ioctl(&sc, 0, &mut two, 0), Err(Errno::ENOTTY));
    }

    #[test]
    fn ioctl_set_names_and_securelevel() {
        let _g = LEVEL.lock().unwrap();
        SECURELEVEL.store(0, Ordering::Relaxed);
        let (sc, fake) = attached();

        // Unsupported flags.
        let mut s = set_bytes(&GpioPinSet {
            gp_name: [0; 64],
            gp_pin: 5,
            gp_caps: 0,
            gp_flags: 0x4,
            gp_name2: [0; 64],
        });
        assert_eq!(gpio_ioctl(&sc, GPIOPINSET, &mut s, 0), Err(Errno::ENODEV));

        // Configure pin 5 as output and name it "led".
        let mut s = set_bytes(&GpioPinSet {
            gp_name: [0; 64],
            gp_pin: 5,
            gp_caps: 0,
            gp_flags: 0x2,
            gp_name2: name("led"),
        });
        gpio_ioctl(&sc, GPIOPINSET, &mut s, 0).unwrap();
        let out = ioctl_arg::<GpioPinSet>(&s);
        assert_eq!(out.gp_caps, 0x3);
        assert_eq!(out.gp_flags, 0); // the old flags
        assert_eq!(fake.ctl[5].get(), 0x2 | GPIO_PIN_SET);
        assert_eq!(gpio_pinbyname(&sc, &name("led")), 5);

        // Rename it.
        let mut s = set_bytes(&GpioPinSet {
            gp_name: name("led"),
            gp_pin: 0,
            gp_caps: 0,
            gp_flags: 0x2,
            gp_name2: name("lamp"),
        });
        gpio_ioctl(&sc, GPIOPINSET, &mut s, 0).unwrap();
        assert_eq!(gpio_pinbyname(&sc, &name("led")), -1);
        assert_eq!(gpio_pinbyname(&sc, &name("lamp")), 5);

        // By name; at securelevel 1 only the set pins count and can be read.
        SECURELEVEL.store(1, Ordering::Relaxed);
        let mut info = [0u8; 4];
        gpio_ioctl(&sc, GPIOINFO, &mut info, 0).unwrap();
        assert_eq!(ioctl_arg::<GpioInfo>(&info).gpio_npins, 1);
        let mut r = op_bytes(&GpioPinOp {
            gp_name: name("lamp"),
            gp_pin: 0,
            gp_value: 0,
        });
        gpio_ioctl(&sc, GPIOPINREAD, &mut r, 0).unwrap();
        let mut r0 = op_bytes(&GpioPinOp {
            gp_name: [0; 64],
            gp_pin: 0,
            gp_value: 0,
        });
        assert_eq!(gpio_ioctl(&sc, GPIOPINREAD, &mut r0, 0), Err(Errno::EPERM));
        assert_eq!(gpio_ioctl(&sc, GPIOPINSET, &mut s, 0), Err(Errno::EPERM));
        SECURELEVEL.store(0, Ordering::Relaxed);

        // Unset drops the name and the flag.
        let mut u = set_bytes(&GpioPinSet {
            gp_name: name("lamp"),
            gp_pin: 0,
            gp_caps: 0,
            gp_flags: 0,
            gp_name2: [0; 64],
        });
        gpio_ioctl(&sc, GPIOPINUNSET, &mut u, 0).unwrap();
        assert_eq!(gpio_pinbyname(&sc, &name("lamp")), -1);
        assert_eq!(sc.pins()[5].pin_flags.get() & GPIO_PIN_SET, 0);
        let mut u5 = set_bytes(&GpioPinSet {
            gp_name: [0; 64],
            gp_pin: 5,
            gp_caps: 0,
            gp_flags: 0,
            gp_name2: [0; 64],
        });
        assert_eq!(
            gpio_ioctl(&sc, GPIOPINUNSET, &mut u5, 0),
            Err(Errno::EINVAL)
        );
    }
}
/* </TESTS> */
