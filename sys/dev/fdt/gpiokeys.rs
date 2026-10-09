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
 * Copyright (c) 2021 Klemens Nanni <kn@openbsd.org>
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
//! Keys and switches wired to GPIO pins, gpiokeys(4): `dev/fdt/gpiokeys.c`.
//!
//! Upstream: sys/dev/fdt/gpiokeys.c @ 3ce1f3f79392
//!
//! A `gpio-keys` (or `gpio-keys-polled`) node lists keys by their Linux input codes: the
//! lid switch becomes an indicator sensor that follows `machdep.lidaction`, the power key
//! calls `powerbutton_event` on release. A key whose controller gives it no interrupt is
//! polled once a second by the sensor task, through `gpiokeys_update_key`, which only acts
//! on the lid switch. QEMU's `virt` has a `gpio-keys` node with the power key on its PL061,
//! whose driver (`plgpio`) has no interrupts: the key is polled and QEMU's
//! `system_powerdown` does nothing, exactly as on OpenBSD 8.0 on the same machine (`cargo
//! xtask diff-openbsd powerbtn`, M16f).
//!
//! ## Deviations
//! - `option SUSPEND` (arm64 GENERIC) waits for `kern/subr_suspend.c`: `cpu_suspended`
//!   reads as 0, and `device_register_wakeup` and `request_sleep(SLEEP_SUSPEND)` are
//!   visible stubs (`unported!`). Neither is reached on QEMU (no interrupt, no lid).
//! - The keys are `Box`es leaked for the device's life (`malloc` with `M_DEVBUF`, never
//!   freed, as the device never detaches); `key_pin` is the `gpios` property's cells; the
//!   label is read into a `Vec` (`M_TEMP`).
//! - `lid_action` comes through `machine::fdt::lid_action` (`extern int lid_action`).

use alloc::boxed::Box;
use alloc::vec;
use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::{self, NonNull};

use crate::dev::fdt::simplefb::SIMPLEFB_BURN_HOOK;
use crate::dev::ofw::fdt::{
    OF_child, OF_getprop, OF_getpropint, OF_getpropintarray, OF_getproplen, OF_is_compatible,
    OF_peer,
};
use crate::dev::ofw::ofw_gpio::{
    GPIO_CONFIG_INPUT, gpio_controller_config_pin, gpio_controller_get_pin,
    gpio_controller_intr_establish,
};
use crate::dev::ofw::ofw_pinctrl::pinctrl_byname;
use crate::kern::kern_sensors::{sensor_attach, sensor_task_register, sensordev_install};
use crate::kern::kern_xxx::powerbutton_event;
use crate::kern::subr_prf::{Str, printf};
use crate::machine::fdt::{FdtAttachArgs, lid_action};
use crate::machine::intr::{IPL_BIO, IPL_WAKEUP};
use crate::queue_adapter;
use crate::sys::device::{CfMatch, Cfattach, Cfdriver, DV_DULL, Device, Softc};
use crate::sys::queue::{SlistEntry, SlistHead};
use crate::sys::sensors::{Ksensor, Ksensordev, SENSOR_INDICATOR};
use crate::unported;

/*
 * Defines from Linux, see:
 *	Documentation/input/event-codes.rst
 *	include/dt-bindings/input/linux-event-codes.h
 */
/// `GPIOKEYS_EV_KEY` (`enum gpiokeys_event_type`).
pub const GPIOKEYS_EV_KEY: u32 = 1;
/// `GPIOKEYS_EV_SW`.
pub const GPIOKEYS_EV_SW: u32 = 5;

/// `GPIOKEYS_SW_LID` (`enum gpiokeys_switch_event`): set = lid closed.
pub const GPIOKEYS_SW_LID: u32 = 0;

/// `GPIOKEYS_KEY_POWER` (`enum gpiokeys_key_event`).
pub const GPIOKEYS_KEY_POWER: u32 = 116;

/// `struct gpiokeys_key`.
pub struct GpiokeysKey {
    /// `key_pin`: the `gpios` property's cells.
    pub key_pin: &'static [u32],
    /// `key_input_type`.
    pub key_input_type: u32,
    /// `key_code`.
    pub key_code: u32,
    /// `key_state`.
    pub key_state: Cell<i32>,
    /// `key_sensor`.
    pub key_sensor: Ksensor,
    /// `key_next`.
    pub key_next: SlistEntry<GpiokeysKey>,
    /// `key_func`.
    pub key_func: Cell<Option<fn(*mut c_void)>>,
    /// `key_ih`.
    pub key_ih: Cell<Option<NonNull<c_void>>>,
    /// `key_wakeup`.
    pub key_wakeup: Cell<i32>,
}

queue_adapter!(
    /// `SLIST_HEAD(, gpiokeys_key)` through `key_next`.
    pub GpiokeysKeyList: GpiokeysKey, key_next => SlistEntry<GpiokeysKey>
);

/// `struct gpiokeys_softc`.
#[repr(C)]
pub struct GpiokeysSoftc {
    /// `sc_dev`.
    pub sc_dev: Device,
    /// `sc_sensordev`.
    pub sc_sensordev: Ksensordev,
    /// `sc_keys`.
    pub sc_keys: SlistHead<GpiokeysKeyList>,
}

// SAFETY: `#[repr(C)]` with the `struct device` first; the sensor device and the list head
// are `Cell`s of integers, arrays and null pointers, valid all-zero.
unsafe impl Softc for GpiokeysSoftc {}

/// `gpiokeys_ca`.
pub static GPIOKEYS_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<GpiokeysSoftc>(),
    ca_match: Some(gpiokeys_match),
    ca_attach: gpiokeys_attach,
    ca_detach: None,
    ca_activate: None,
};

/// `gpiokeys_cd`.
pub static GPIOKEYS_CD: Cfdriver = Cfdriver::new(b"gpiokeys", DV_DULL, 0);

/// `gpiokeys_match`: a `gpio-keys` or `gpio-keys-polled` node.
pub fn gpiokeys_match(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: the device-tree buses hand their children `struct fdt_attach_args`.
    let faa = unsafe { &*aux.cast::<FdtAttachArgs<'_>>() };

    i32::from(
        OF_is_compatible(faa.fa_node, b"gpio-keys")
            || OF_is_compatible(faa.fa_node, b"gpio-keys-polled"),
    )
}

/// The `strlcpy` of a sensor description.
fn desc(s: &[u8]) -> [u8; 32] {
    let mut d = [0u8; 32];
    let n = s.len().min(d.len() - 1);
    d[..n].copy_from_slice(&s[..n]);
    d
}

/// `gpiokeys_attach`: one key per child node with a `linux,code` and `gpios`.
pub fn gpiokeys_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: as in `gpiokeys_match`.
    let faa = unsafe { &*aux.cast::<FdtAttachArgs<'_>>() };
    // SAFETY: `gpiokeys_ca` makes `GpiokeysSoftc`s; `config_make_softc`'s allocation lives
    // as long as the device, which is never detached.
    let sc: &'static GpiokeysSoftc = unsafe { &*ptr::from_ref(self_.softc::<GpiokeysSoftc>()) };
    let mut have_labels = false;
    let mut have_sensors = false;

    sc.sc_keys.init();

    pinctrl_byname(faa.fa_node, b"default");

    let mut node = OF_child(faa.fa_node);
    while node != 0 {
        let this = node;
        node = OF_peer(node);

        let code = OF_getpropint(this, b"linux,code", u32::MAX);
        if code == u32::MAX {
            continue;
        }
        let gpios_len = OF_getproplen(this, b"gpios");
        if gpios_len <= 0 {
            continue;
        }
        let mut label = None;
        let len = OF_getproplen(this, b"label");
        if len > 0 {
            let mut buf = vec![0u8; len as usize];
            if OF_getprop(this, b"label", &mut buf) != len {
                continue;
            }
            label = Some(buf);
        }
        let mut pins = vec![0u32; gpios_len as usize / size_of::<u32>()];
        OF_getpropintarray(this, b"gpios", &mut pins);
        let key: &'static GpiokeysKey = Box::leak(Box::new(GpiokeysKey {
            key_pin: Box::leak(pins.into_boxed_slice()),
            key_input_type: OF_getpropint(this, b"linux,input-type", GPIOKEYS_EV_KEY),
            key_code: code,
            key_state: Cell::new(0),
            key_sensor: Ksensor::new(),
            key_next: SlistEntry::new(),
            key_func: Cell::new(None),
            key_ih: Cell::new(None),
            key_wakeup: Cell::new(0),
        }));
        gpio_controller_config_pin(key.key_pin, GPIO_CONFIG_INPUT);

        match (key.key_input_type, key.key_code) {
            (GPIOKEYS_EV_SW, GPIOKEYS_SW_LID) => {
                key.key_sensor.desc.set(desc(b"lid open"));
                key.key_sensor.r#type.set(SENSOR_INDICATOR);
                sensor_attach(&sc.sc_sensordev, &key.key_sensor);
                key.key_func.set(Some(gpiokeys_update_key));
                key.key_wakeup.set(1);
                have_sensors = true;
            }
            (GPIOKEYS_EV_KEY, GPIOKEYS_KEY_POWER) => {
                key.key_func.set(Some(gpiokeys_power_button));
            }
            _ => {}
        }

        if let Some(label) = label {
            let text = label.split(|&c| c == 0).next().unwrap_or_default();
            printf(format_args!(
                "{} \"{}\"",
                if have_labels { "," } else { ":" },
                Str(text)
            ));
            have_labels = true;
        }

        // SAFETY: a fresh key that lives forever.
        unsafe { sc.sc_keys.insert_head(key) };
    }

    for key in sc.sc_keys.iter() {
        if key.key_func.get().is_none() {
            continue;
        }

        let arg = ptr::from_ref(key).cast_mut().cast::<c_void>();
        if OF_is_compatible(faa.fa_node, b"gpio-keys") {
            let wakeup = if key.key_wakeup.get() != 0 {
                IPL_WAKEUP
            } else {
                0
            };
            key.key_ih.set(gpio_controller_intr_establish(
                key.key_pin,
                IPL_BIO | wakeup,
                None,
                gpiokeys_intr,
                arg,
                sc.sc_dev.xname(),
            ));
        }
        // SUSPEND: kern/subr_suspend.c is not ported.
        if key.key_wakeup.get() != 0 && key.key_ih.get().is_some() {
            let _ = unported!("device_register_wakeup");
        }
        if key.key_ih.get().is_none() {
            sensor_task_register(arg, gpiokeys_update_key, 1);
        } else {
            gpiokeys_update_key(arg);
        }
    }

    if have_sensors {
        let mut xname = [0u8; 16];
        let name = sc.sc_dev.xname().as_bytes();
        let n = name.len().min(xname.len() - 1);
        xname[..n].copy_from_slice(&name[..n]);
        sc.sc_sensordev.xname.set(xname);
        sensordev_install(&sc.sc_sensordev);
    }

    if sc.sc_keys.is_empty() {
        printf(format_args!(": no keys"));
    }
    printf(format_args!("\n"));
}

/// The key a callback's argument names.
fn key_of(arg: *mut c_void) -> &'static GpiokeysKey {
    // SAFETY: `gpiokeys_attach` registers its leaked keys as the argument.
    unsafe { &*arg.cast::<GpiokeysKey>() }
}

/// `gpiokeys_update_key`: the lid switch's state (the sensor task's callback for a key with
/// no interrupt).
pub fn gpiokeys_update_key(arg: *mut c_void) {
    let key = key_of(arg);

    let val = gpio_controller_get_pin(key.key_pin);

    if key.key_input_type == GPIOKEYS_EV_SW && key.key_code == GPIOKEYS_SW_LID {
        // SUSPEND: kern/subr_suspend.c is not ported, so cpu_suspended is 0 and the
        // resume-on-open path never runs.

        /*
         * Match acpibtn(4), i.e. closed ThinkPad lid yields
         * hw.sensors.acpibtn1.indicator0=Off (lid open)
         */
        key.key_sensor.value.set(i64::from(val == 0));

        match lid_action() {
            0 => {
                // SAFETY: the hook is set while cold, if ever, and only read afterwards.
                if let Some(hook) = unsafe { SIMPLEFB_BURN_HOOK.read() } {
                    hook(u32::from(val == 0));
                }
            }
            1 => {
                // SUSPEND
                if val != 0 {
                    let _ = unported!("request_sleep");
                }
            }
            2 => { /* XXX: hibernate */ }
            _ => {}
        }
    }
}

/// `gpiokeys_power_button`: a release of the power key is a power-button event.
pub fn gpiokeys_power_button(arg: *mut c_void) {
    let key = key_of(arg);

    let state = gpio_controller_get_pin(key.key_pin);

    if state != key.key_state.get() {
        /* Ignore presses, handle releases. */
        if state == 0 {
            powerbutton_event();
        }
        key.key_state.set(state);
    }
}

/// `gpiokeys_intr`: a key's interrupt runs its function.
pub fn gpiokeys_intr(arg: *mut c_void) -> i32 {
    let key = key_of(arg);

    if let Some(f) = key.key_func.get() {
        f(arg);
    }
    1
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_event_codes() {
        // include/dt-bindings/input/linux-event-codes.h: EV_KEY, EV_SW, SW_LID, KEY_POWER.
        assert_eq!(
            (
                GPIOKEYS_EV_KEY,
                GPIOKEYS_EV_SW,
                GPIOKEYS_SW_LID,
                GPIOKEYS_KEY_POWER
            ),
            (1, 5, 0, 116)
        );
    }

    #[test]
    fn sensor_description_is_truncated_and_terminated() {
        assert_eq!(&desc(b"lid open")[..9], b"lid open\0");
        let long = [b'x'; 40];
        let d = desc(&long);
        assert_eq!(d[30], b'x');
        assert_eq!(d[31], 0);
    }

    #[test]
    fn power_key_without_controller_reads_low() {
        // No controller has this phandle: the pin reads 0, so the state stays 0 and no
        // power-button event is raised.
        let key: &'static GpiokeysKey = Box::leak(Box::new(GpiokeysKey {
            key_pin: Box::leak(Box::new([0xdead, 3, 0])),
            key_input_type: GPIOKEYS_EV_KEY,
            key_code: GPIOKEYS_KEY_POWER,
            key_state: Cell::new(0),
            key_sensor: Ksensor::new(),
            key_next: SlistEntry::new(),
            key_func: Cell::new(Some(gpiokeys_power_button)),
            key_ih: Cell::new(None),
            key_wakeup: Cell::new(0),
        }));
        let arg = ptr::from_ref(key).cast_mut().cast::<c_void>();
        assert_eq!(gpiokeys_intr(arg), 1);
        assert_eq!(key.key_state.get(), 0);
    }
}
/* </TESTS> */
