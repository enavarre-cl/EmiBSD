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
 * Copyright (c) 2003 Markus Friedl.  All rights reserved.
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
/* </LICENSES> */

/* <CODE> */
//! The watchdog framework: `kern/kern_watchdog.c`. One hardware watchdog driver registers
//! its control callback (`wdog_register`); `sysctl kern.watchdog.period` arms it (the
//! callback returns the period it really set) and, while `kern.watchdog.auto` is on, a
//! timeout re-arms it every half period so that the machine resets only when the kernel stops
//! running timeouts. `wdog_shutdown` disarms it when its driver powers down.
//!
//! Upstream: sys/kern/kern_watchdog.c @ 3ce1f3f79392
//!
//! The state is touched under the kernel lock: `wdog_register` at autoconfiguration,
//! `sysctl_wdog` inside `sysctl_vslock`, `wdog_tickle` from softclock, `wdog_shutdown` from
//! `config_suspend_all`.
//!
//! ## Deviations
//! - `int (*wdog_ctl_cb)(void *, int)` is a `StaticCell<Option<WdogCtlFn>>` (the C's
//!   `NULL` is `None`) and `wdog_ctl_cb_arg` an `AtomicPtr`; `wdog_period` and `wdog_auto`
//!   are `AtomicI32`s, because `sysctl_int_bounded` takes one.
//! - `name` is the slice after `KERN_WATCHDOG`, as `kern_sysctl_dirs_locked` hands it; an
//!   empty one is `EINVAL` (the C reads `name[0]` unchecked).
//! - The timeout's period in milliseconds is computed in 64 bits, so a period above
//!   `INT_MAX / 1000` seconds does not overflow as the C's `int` product would.

use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::{AtomicI32, AtomicPtr, Ordering};

use crate::kern::kern_sysctl::sysctl_int_bounded;
use crate::kern::kern_timeout::{timeout_add_msec, timeout_del, timeout_set};
use crate::sys::errno::Errno;
use crate::sys::sysctl::{KERN_WATCHDOG_AUTO, KERN_WATCHDOG_PERIOD};
use crate::sys::timeout::Timeout;
use libkern::StaticCell;

/// `int (*)(void *, int)`: a watchdog driver's control callback. It sets the period (in
/// seconds, 0 disarms) and returns the period it really set.
pub type WdogCtlFn = fn(*mut c_void, i32) -> i32;

/// `wdog_ctl_cb`: the registered driver's callback. Under the kernel lock (module doc).
static WDOG_CTL_CB: StaticCell<Option<WdogCtlFn>> = StaticCell::new(None);
/// `wdog_ctl_cb_arg`: its argument.
static WDOG_CTL_CB_ARG: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
/// `wdog_period`: the period in seconds the driver accepted; 0 when disarmed.
pub static WDOG_PERIOD: AtomicI32 = AtomicI32::new(0);
/// `wdog_auto`: re-arm the watchdog from a timeout.
pub static WDOG_AUTO: AtomicI32 = AtomicI32::new(1);
/// `wdog_timeout`.
static WDOG_TIMEOUT: Timeout = Timeout::zeroed();

/// The registered callback.
fn wdog_ctl_cb() -> Option<WdogCtlFn> {
    // SAFETY: written only by `wdog_register` and `wdog_shutdown`, under the kernel lock
    // like every reader (see the module doc); the copy ends before either can run.
    unsafe { WDOG_CTL_CB.read() }
}

/// Calls the registered callback with `period`; `None` when none is registered.
fn wdog_ctl(period: i32) -> Option<i32> {
    let cb = wdog_ctl_cb()?;
    Some(cb(WDOG_CTL_CB_ARG.load(Ordering::Relaxed), period))
}

/// Half of `period` seconds, in milliseconds (`period * 1000 / 2`).
fn wdog_half_period_msec(period: i32) -> u64 {
    (i64::from(period) * 1000 / 2).max(0) as u64
}

/// `wdog_register(cb, cb_arg)`: makes `cb` the watchdog. The first driver to register keeps
/// it.
pub fn wdog_register(cb: WdogCtlFn, cb_arg: *mut c_void) {
    if wdog_ctl_cb().is_some() {
        return;
    }

    // SAFETY: under the kernel lock (module doc); no reference into the cell is live.
    unsafe { WDOG_CTL_CB.write(Some(cb)) };
    WDOG_CTL_CB_ARG.store(cb_arg, Ordering::Relaxed);
    timeout_set(&WDOG_TIMEOUT, wdog_tickle, ptr::null_mut());
}

/// `wdog_tickle(arg)`: re-arms the watchdog for the current period and comes back after half
/// of it.
pub fn wdog_tickle(_arg: *mut c_void) {
    if wdog_ctl(WDOG_PERIOD.load(Ordering::Relaxed)).is_none() {
        return;
    }
    timeout_add_msec(
        &WDOG_TIMEOUT,
        wdog_half_period_msec(WDOG_PERIOD.load(Ordering::Relaxed)),
    );
}

/// `wdog_shutdown(arg)`: disarms and unregisters the watchdog if `arg` is the argument it was
/// registered with (a driver passes its device).
pub fn wdog_shutdown(arg: *mut c_void) {
    if wdog_ctl_cb().is_none() || WDOG_CTL_CB_ARG.load(Ordering::Relaxed) != arg {
        return;
    }
    timeout_del(&WDOG_TIMEOUT);
    let _ = wdog_ctl(0);
    // SAFETY: under the kernel lock (module doc); no reference into the cell is live.
    unsafe { WDOG_CTL_CB.write(None) };
    WDOG_PERIOD.store(0, Ordering::Relaxed);
    WDOG_AUTO.store(1, Ordering::Relaxed);
}

/// `sysctl_wdog(name, namelen, oldp, oldlenp, newp, newlen)`: `kern.watchdog.period` and
/// `kern.watchdog.auto`. `EOPNOTSUPP` while no watchdog is registered.
pub fn sysctl_wdog(
    name: &[i32],
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    newlen: usize,
) -> Result<(), Errno> {
    if wdog_ctl_cb().is_none() {
        return Err(Errno::EOPNOTSUPP);
    }

    match name.first().copied() {
        Some(KERN_WATCHDOG_PERIOD) => {
            let period = AtomicI32::new(WDOG_PERIOD.load(Ordering::Relaxed));
            sysctl_int_bounded(oldp, oldlenp, newp, newlen, &period, 0, i32::MAX)?;
            if newp != 0 {
                timeout_del(&WDOG_TIMEOUT);
                if let Some(set) = wdog_ctl(period.into_inner()) {
                    WDOG_PERIOD.store(set, Ordering::Relaxed);
                }
            }
        }
        Some(KERN_WATCHDOG_AUTO) => {
            sysctl_int_bounded(oldp, oldlenp, newp, newlen, &WDOG_AUTO, 0, 1)?;
        }
        _ => return Err(Errno::EINVAL),
    }

    let period = WDOG_PERIOD.load(Ordering::Relaxed);
    if WDOG_AUTO.load(Ordering::Relaxed) != 0 && period > 0 {
        let _ = wdog_ctl(period);
        timeout_add_msec(&WDOG_TIMEOUT, wdog_half_period_msec(period));
    } else {
        timeout_del(&WDOG_TIMEOUT);
    }

    Ok(())
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;
    use crate::sys::timeout::timeout_pending;
    use std::sync::Mutex;
    use std::vec::Vec;

    /// The periods the fake driver was asked for, in order.
    static CALLS: Mutex<Vec<(usize, i32)>> = Mutex::new(Vec::new());

    /// A driver that rounds periods below 10 s up to 10, as ipmi(4) does.
    fn fake_cb(arg: *mut c_void, period: i32) -> i32 {
        CALLS.lock().unwrap().push((arg as usize, period));
        if period > 0 && period < 10 {
            10
        } else {
            period
        }
    }

    fn calls() -> Vec<(usize, i32)> {
        core::mem::take(&mut *CALLS.lock().unwrap())
    }

    fn ra<T>(b: &T) -> usize {
        b as *const T as usize
    }

    fn ua<T>(b: &mut T) -> usize {
        b as *mut T as usize
    }

    /// One test, because the framework is a set of globals.
    #[test]
    fn register_arm_tickle_and_shutdown() {
        let _t = crate::kern::kern_timeout::tests::LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::kern::kern_timeout::timeout_startup();
        let mut len = 4usize;
        let mut out = 0i32;

        // Nothing registered.
        assert_eq!(
            sysctl_wdog(&[KERN_WATCHDOG_PERIOD], ua(&mut out), &mut len, 0, 0),
            Err(Errno::EOPNOTSUPP)
        );

        let me = 0x1000usize as *mut c_void;
        wdog_register(fake_cb, me);
        // A second driver does not replace the first.
        wdog_register(fake_cb, 0x2000usize as *mut c_void);

        assert_eq!(
            sysctl_wdog(&[KERN_WATCHDOG_PERIOD], ua(&mut out), &mut len, 0, 0),
            Ok(())
        );
        assert_eq!(out, 0);
        assert!(calls().is_empty());
        assert!(!timeout_pending(&WDOG_TIMEOUT));

        // Arm with 3 s: the driver rounds it to 10, then auto re-arms with that.
        let three = 3i32;
        assert_eq!(
            sysctl_wdog(&[KERN_WATCHDOG_PERIOD], 0, &mut len, ra(&three), 4),
            Ok(())
        );
        assert_eq!(WDOG_PERIOD.load(Ordering::Relaxed), 10);
        assert_eq!(calls(), [(0x1000, 3), (0x1000, 10)]);
        assert!(timeout_pending(&WDOG_TIMEOUT));

        // The timeout's function re-arms with the current period.
        wdog_tickle(ptr::null_mut());
        assert_eq!(calls(), [(0x1000, 10)]);

        // Bad names and bounds.
        assert_eq!(sysctl_wdog(&[99], 0, &mut len, 0, 0), Err(Errno::EINVAL));
        let two = 2i32;
        assert_eq!(
            sysctl_wdog(&[KERN_WATCHDOG_AUTO], 0, &mut len, ra(&two), 4),
            Err(Errno::EINVAL)
        );

        // auto off: the timeout stops, the period stays.
        let zero = 0i32;
        assert_eq!(
            sysctl_wdog(&[KERN_WATCHDOG_AUTO], 0, &mut len, ra(&zero), 4),
            Ok(())
        );
        assert!(!timeout_pending(&WDOG_TIMEOUT));
        assert!(calls().is_empty());

        // Another argument does not shut it down; the right one does.
        wdog_shutdown(0x2000usize as *mut c_void);
        assert!(wdog_ctl_cb().is_some());
        wdog_shutdown(me);
        assert_eq!(calls(), [(0x1000, 0)]);
        assert!(wdog_ctl_cb().is_none());
        assert_eq!(WDOG_PERIOD.load(Ordering::Relaxed), 0);
        assert_eq!(WDOG_AUTO.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn half_period_in_msec() {
        assert_eq!(wdog_half_period_msec(0), 0);
        assert_eq!(wdog_half_period_msec(30), 15_000);
        assert_eq!(wdog_half_period_msec(i32::MAX), 1_073_741_823_500);
    }
}
/* </TESTS> */
