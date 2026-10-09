/* <LICENSES> */
/*
 * Copyright (c) 2005 David Gwynne <dlg@openbsd.org>
 * Copyright (c) 2006 Constantine A. Murenin <cnst+openbsd@bugmail.mojo.ru>
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
//! `kern_sensors.c`: the registry of hardware sensors behind `hw.sensors`
//! (`sensordev_install`, `sensor_attach`, `sensordev_get`, `sensor_find`) and the periodic
//! sensor refresh tasks (`sensor_task_register`), which run on the `sensors` task queue.
//!
//! Upstream: sys/kern/kern_sensors.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `hotplug_device_attach`/`hotplug_device_detach` (`NHOTPLUG > 0`; `dev/hotplug.c` is not
//!   ported) are reported, as `subr_autoconf.rs` does, when the machine is not `cold`.
//! - `sensordev_get` and `sensor_find` return the device or sensor (`Result<&Ksensordev, _>`)
//!   instead of filling a `struct ksensordev **`.
//! - `sensor_task_register` returns `Option<NonNull<SensorTask>>` (the C's `NULL` on an
//!   allocation failure); `sensor_task_unregister` takes it back and is `unsafe`, because
//!   the task frees itself afterwards (`sensor_task_work`), as `taskq_destroy` does.
//! - `sensors_taskq` is an `AtomicPtr` holding a `&'static Taskq`; `sensordev_count`,
//!   `sensors_quiesced` and `sensors_running` are atomics (the C uses `atomic_inc_int` on
//!   the last one).

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicI32, AtomicPtr, AtomicU32, Ordering};

use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_rwlock::{rw_enter_write, rw_exit_write, rw_init};
use crate::kern::kern_synch::{tsleep_nsec, wakeup};
use crate::kern::kern_task::{task_add, task_set, taskq_create};
use crate::kern::kern_timeout::{timeout_add_sec, timeout_set};
use crate::machine::intr::{IPL_HIGH, splhigh, splx};
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_DEVBUF, M_NOWAIT};
use crate::sys::param::PZERO;
use crate::sys::queue::SlistHead;
use crate::sys::rwlock::Rwlock;
use crate::sys::sensors::{
    Ksensor, KsensorList, Ksensordev, KsensordevList, SENSOR_MAX_TYPES, SensorType,
};
use crate::sys::systm::{COLD, INFSLP};
use crate::sys::task::{SYSTQ, Task, Taskq};
use crate::sys::timeout::Timeout;
use crate::unported;

/// `struct sensor_task`: a driver's refresh function, run every `period` seconds on
/// `sensors_taskq`.
pub struct SensorTask {
    /// `func`: the driver's refresh function.
    func: fn(*mut c_void),
    /// `arg`: its argument.
    arg: *mut c_void,
    /// `period`: seconds between runs; 0 marks a task that is dying. Protected by: `lock`.
    period: Cell<u32>,
    /// `timeout`: schedules the next run.
    timeout: Timeout,
    /// `task`: the run on `sensors_taskq`.
    task: Task,
    /// `lock`: serialises a run with `sensor_task_unregister`.
    lock: Rwlock,
}

// SAFETY: `period` changes under `lock`; `func` and `arg` are written once before the task is
// shared; the timeout and the task are their subsystems' (`timeout_mutex`, the queue's mutex).
unsafe impl Sync for SensorTask {}

/// `SLIST_HEAD(, ksensordev) sensordev_list`, made `Sync`.
pub struct SensordevListHead(SlistHead<KsensordevList>);

// SAFETY: the list changes at `splhigh` (`sensordev_install`, `sensordev_deinstall`) and is
// read under the kernel lock, which the writers hold too (autoconfiguration, the
// drivers' tasks).
unsafe impl Sync for SensordevListHead {}

/// `sensors_taskq`: the queue the refresh tasks run on (`systq` if it cannot be made).
/// Holds a `&'static Taskq` or NULL.
static SENSORS_TASKQ: AtomicPtr<Taskq> = AtomicPtr::new(ptr::null_mut());

/// `sensordev_count`.
static SENSORDEV_COUNT: AtomicI32 = AtomicI32::new(0);

/// `sensordev_list`: the installed sensor devices, sorted by `num`.
pub static SENSORDEV_LIST: SensordevListHead = SensordevListHead(SlistHead::new());

/// `sensors_quiesced`.
static SENSORS_QUIESCED: AtomicI32 = AtomicI32::new(0);
/// `sensors_running`: refresh functions running now.
static SENSORS_RUNNING: AtomicU32 = AtomicU32::new(0);

/// `sensordev_install`: gives `sensdev` the lowest free number and links it into
/// `sensordev_list`, which stays sorted.
pub fn sensordev_install(sensdev: &'static Ksensordev) {
    let list = &SENSORDEV_LIST.0;

    let s = splhigh();
    if SENSORDEV_COUNT.load(Ordering::Relaxed) == 0 {
        sensdev.num.set(0);
        // SAFETY: a device being installed is in no list; `'static`; at `splhigh`.
        unsafe { list.insert_head(sensdev) };
    } else {
        // the first device followed by a gap in the numbers, or the last one
        let mut v = list.first();
        while let Some(cur) = v {
            match SlistHead::<KsensordevList>::next(cur) {
                Some(nv) if nv.num.get() - cur.num.get() <= 1 => v = Some(nv),
                _ => break,
            }
        }
        if let Some(v) = v {
            sensdev.num.set(v.num.get() + 1);
            // SAFETY: `v` is linked; `sensdev` is in no list and `'static`; at `splhigh`.
            unsafe { SlistHead::<KsensordevList>::insert_after(v, sensdev) };
        }
    }
    SENSORDEV_COUNT.fetch_add(1, Ordering::Relaxed);
    splx(s);

    // NHOTPLUG > 0
    if !COLD.load(Ordering::Relaxed) {
        let _ = unported!("hotplug_device_attach (dev/hotplug.c)");
    }
}

/// `sensor_attach`: links `sens` into `sensdev`'s sensors after the others of its type,
/// giving it the lowest free number of that type (`numt`).
pub fn sensor_attach(sensdev: &Ksensordev, sens: &'static Ksensor) {
    let sh = &sensdev.sensors_list;

    let s = splhigh();
    if sensdev.sensors_count.get() == 0 {
        sensdev.maxnumt.set([0; SENSOR_MAX_TYPES]);
        sens.numt.set(0);
        // SAFETY: a sensor being attached is in no list; `'static`; at `splhigh`.
        unsafe { sh.insert_head(sens) };
    } else {
        let mut v = sh.first();
        while let Some(cur) = v {
            let Some(nv) = SlistHead::<KsensorList>::next(cur) else {
                break;
            };
            if cur.r#type.get() == sens.r#type.get()
                && (cur.r#type.get() != nv.r#type.get() || nv.numt.get() - cur.numt.get() > 1)
            {
                break;
            }
            v = Some(nv);
        }
        if let Some(v) = v {
            // sensors of the same type go after each other
            if v.r#type.get() == sens.r#type.get() {
                sens.numt.set(v.numt.get() + 1);
            } else {
                sens.numt.set(0);
            }
            // SAFETY: `v` is linked; `sens` is in no list and `'static`; at `splhigh`.
            unsafe { SlistHead::<KsensorList>::insert_after(v, sens) };
        }
    }
    // we only increment maxnumt[] if the sensor was added to the last position of sensors
    // of this type
    let mut maxnumt = sensdev.maxnumt.get();
    let t = sens.r#type.get() as usize;
    if maxnumt[t] == sens.numt.get() {
        maxnumt[t] += 1;
        sensdev.maxnumt.set(maxnumt);
    }
    sensdev.sensors_count.set(sensdev.sensors_count.get() + 1);
    splx(s);
}

/// `sensordev_deinstall`: unlinks `sensdev` from `sensordev_list`.
pub fn sensordev_deinstall(sensdev: &Ksensordev) {
    let s = splhigh();
    SENSORDEV_COUNT.fetch_sub(1, Ordering::Relaxed);
    // SAFETY: an installed device is in `sensordev_list` (`sensordev_install`'s contract with
    // its caller); at `splhigh`.
    unsafe { SENSORDEV_LIST.0.remove(sensdev) };
    splx(s);

    // NHOTPLUG > 0
    if !COLD.load(Ordering::Relaxed) {
        let _ = unported!("hotplug_device_detach (dev/hotplug.c)");
    }
}

/// `sensor_detach`: unlinks `sens` from `sensdev`'s sensors.
pub fn sensor_detach(sensdev: &Ksensordev, sens: &Ksensor) {
    let sh = &sensdev.sensors_list;

    let s = splhigh();
    sensdev.sensors_count.set(sensdev.sensors_count.get() - 1);
    // SAFETY: an attached sensor is in its device's list (`sensor_attach`); at `splhigh`.
    unsafe { sh.remove(sens) };
    // we only decrement maxnumt[] if this is the tail sensor of this type
    let mut maxnumt = sensdev.maxnumt.get();
    let t = sens.r#type.get() as usize;
    if sens.numt.get() == maxnumt[t] - 1 {
        maxnumt[t] -= 1;
        sensdev.maxnumt.set(maxnumt);
    }
    splx(s);
}

/// `sensordev_get`: the installed device numbered `num`; `ENXIO` when the number is a gap,
/// `ENOENT` past the last device.
pub fn sensordev_get(num: i32) -> Result<&'static Ksensordev, Errno> {
    for sd in SENSORDEV_LIST.0.iter() {
        if sd.num.get() == num {
            return Ok(sd);
        }
        if sd.num.get() > num {
            return Err(Errno::ENXIO);
        }
    }
    Err(Errno::ENOENT)
}

/// `sensor_find`: sensor `numt` of type `type` on device `dev`.
pub fn sensor_find(dev: i32, r#type: SensorType, numt: i32) -> Result<&'static Ksensor, Errno> {
    let sensdev = sensordev_get(dev)?;

    sensdev
        .sensors_list
        .iter()
        .find(|s| s.r#type.get() == r#type && s.numt.get() == numt)
        .ok_or(Errno::ENOENT)
}

/// `sensor_task_register`: runs `func(arg)` now and then every `period` seconds on the
/// `sensors` task queue. `None` when the task cannot be allocated.
pub fn sensor_task_register(
    arg: *mut c_void,
    func: fn(*mut c_void),
    period: u32,
) -> Option<NonNull<SensorTask>> {
    #[cfg(feature = "diagnostic")]
    if period == 0 {
        crate::kern::subr_prf::panic(format_args!("sensor_task_register: period is 0"));
    }

    if SENSORS_TASKQ.load(Ordering::Relaxed).is_null() {
        let tq = taskq_create(b"sensors", 1, IPL_HIGH, 0).unwrap_or(SYSTQ);
        SENSORS_TASKQ.store(ptr::from_ref(tq).cast_mut(), Ordering::Relaxed);
    }

    let st = malloc(size_of::<SensorTask>(), M_DEVBUF, M_NOWAIT)?.cast::<SensorTask>();
    // SAFETY: a fresh allocation of `size_of::<SensorTask>()` bytes (malloc's chunks are
    // aligned to their power-of-two size), written once before any use.
    unsafe {
        st.as_ptr().write(SensorTask {
            func,
            arg,
            period: Cell::new(period),
            timeout: Timeout::zeroed(),
            task: Task::zeroed(),
            lock: Rwlock::new("sensor"),
        });
    }
    // SAFETY: initialised above; it lives until `sensor_task_work` frees it, after
    // `sensor_task_unregister`.
    let stref: &'static SensorTask = unsafe { st.as_ref() };
    timeout_set(&stref.timeout, sensor_task_tick, st.as_ptr().cast());
    task_set(&stref.task, sensor_task_work, st.as_ptr().cast());
    rw_init(&stref.lock, "sensor");

    sensor_task_tick(st.as_ptr().cast());

    Some(st)
}

/// `sensor_task_unregister`: marks the task as dying; its next run frees it.
///
/// We can't reliably `timeout_del` or `task_del` because there's a window between when they
/// come off the lists and the timeout or task code actually runs the respective handlers for
/// them. Mark the sensor task as dying by setting period to 0 and let `sensor_task_work` mop
/// up.
///
/// # Safety
///
/// `st` came from [`sensor_task_register`] and is unregistered once; the caller does not use
/// it afterwards (it is freed by the task itself).
pub unsafe fn sensor_task_unregister(st: NonNull<SensorTask>) {
    // SAFETY: the caller's contract: `st` is live until its next run, which this call makes
    // the last.
    let st = unsafe { st.as_ref() };
    rw_enter_write(&st.lock);
    st.period.set(0);
    rw_exit_write(&st.lock);
}

/// `sensor_task_tick`: the timeout queues the task.
fn sensor_task_tick(arg: *mut c_void) {
    // SAFETY: `arg` is the `SensorTask` `sensor_task_register` set this timeout up with; it is
    // freed only by its own task run, after which no timeout is armed.
    let st: &'static SensorTask = unsafe { &*arg.cast::<SensorTask>() };
    let tq = SENSORS_TASKQ.load(Ordering::Relaxed);
    // SAFETY: `sensor_task_register` stored a `&'static Taskq` before arming any timeout.
    let tq: &'static Taskq = unsafe { tq.as_ref() }.unwrap_or(SYSTQ);
    let _ = task_add(tq, &st.task);
}

/// `sensor_quiesce`: stops the refresh functions and waits for the running ones.
pub fn sensor_quiesce() {
    SENSORS_QUIESCED.store(1, Ordering::Relaxed);
    while SENSORS_RUNNING.load(Ordering::Relaxed) > 0 {
        let _ = tsleep_nsec(
            ptr::from_ref(&SENSORS_RUNNING),
            PZERO,
            "sensorpause",
            INFSLP,
        );
    }
}

/// `sensor_restart`.
pub fn sensor_restart() {
    SENSORS_QUIESCED.store(0, Ordering::Relaxed);
}

/// `sensor_task_work`: one run: calls the driver's function unless the task is dying or the
/// sensors are quiesced, then rearms the timeout, or frees a dying task.
fn sensor_task_work(xst: *mut c_void) {
    let st = xst.cast::<SensorTask>();
    // SAFETY: `xst` is the live `SensorTask` this task belongs to (`sensor_task_register`);
    // only this function frees it, below, after its last use.
    let stref: &SensorTask = unsafe { &*st };

    SENSORS_RUNNING.fetch_add(1, Ordering::Relaxed);
    rw_enter_write(&stref.lock);
    let period = stref.period.get();
    if period > 0 && SENSORS_QUIESCED.load(Ordering::Relaxed) == 0 {
        (stref.func)(stref.arg);
    }
    rw_exit_write(&stref.lock);
    if SENSORS_RUNNING.fetch_sub(1, Ordering::Relaxed) == 1
        && SENSORS_QUIESCED.load(Ordering::Relaxed) != 0
    {
        wakeup(ptr::from_ref(&SENSORS_RUNNING));
    }

    if period == 0 {
        if let Some(p) = NonNull::new(st) {
            free(p.cast::<u8>(), M_DEVBUF, size_of::<SensorTask>());
        }
    } else {
        let _ = timeout_add_sec(&stref.timeout, i32::try_from(period).unwrap_or(i32::MAX));
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;
    use crate::sys::sensors::{SENSOR_FANRPM, SENSOR_TEMP};

    extern crate std;
    use std::boxed::Box;

    fn leak<T>(v: T) -> &'static T {
        Box::leak(Box::new(v))
    }

    fn sensor(t: SensorType) -> &'static Ksensor {
        let s = leak(Ksensor::new());
        s.r#type.set(t);
        s
    }

    #[test]
    fn sensor_attach_numbers_per_type() {
        let dev = leak(Ksensordev::new());
        let t0 = sensor(SENSOR_TEMP);
        let t1 = sensor(SENSOR_TEMP);
        let f0 = sensor(SENSOR_FANRPM);
        let t2 = sensor(SENSOR_TEMP);
        sensor_attach(dev, t0);
        sensor_attach(dev, t1);
        sensor_attach(dev, f0);
        sensor_attach(dev, t2);
        assert_eq!(
            (t0.numt.get(), t1.numt.get(), f0.numt.get(), t2.numt.get()),
            (0, 1, 0, 2)
        );
        assert_eq!(dev.maxnumt.get()[SENSOR_TEMP as usize], 3);
        assert_eq!(dev.maxnumt.get()[SENSOR_FANRPM as usize], 1);
        assert_eq!(dev.sensors_count.get(), 4);

        // a hole in the middle keeps maxnumt and is filled again
        sensor_detach(dev, t1);
        assert_eq!(dev.maxnumt.get()[SENSOR_TEMP as usize], 3);
        let t1b = sensor(SENSOR_TEMP);
        sensor_attach(dev, t1b);
        assert_eq!(t1b.numt.get(), 1);
        assert_eq!(dev.maxnumt.get()[SENSOR_TEMP as usize], 3);

        // the tail one lowers maxnumt
        sensor_detach(dev, t2);
        assert_eq!(dev.maxnumt.get()[SENSOR_TEMP as usize], 2);
        assert_eq!(dev.sensors_count.get(), 3);
    }

    #[test]
    fn sensordev_numbers_get_find() {
        let a = leak(Ksensordev::new());
        let b = leak(Ksensordev::new());
        let c = leak(Ksensordev::new());
        sensordev_install(a);
        sensordev_install(b);
        sensordev_install(c);
        assert_eq!((a.num.get(), b.num.get(), c.num.get()), (0, 1, 2));

        let s = sensor(SENSOR_FANRPM);
        sensor_attach(c, s);
        assert!(ptr::eq(sensor_find(2, SENSOR_FANRPM, 0).unwrap(), s));
        assert_eq!(sensor_find(2, SENSOR_TEMP, 0).err(), Some(Errno::ENOENT));

        sensordev_deinstall(b);
        assert_eq!(sensordev_get(1).err(), Some(Errno::ENXIO));
        assert_eq!(sensordev_get(7).err(), Some(Errno::ENOENT));
        assert!(ptr::eq(sensordev_get(2).unwrap(), c));

        // the gap is reused
        let d = leak(Ksensordev::new());
        sensordev_install(d);
        assert_eq!(d.num.get(), 1);

        sensordev_deinstall(a);
        sensordev_deinstall(c);
        sensordev_deinstall(d);
        assert_eq!(sensordev_get(0).err(), Some(Errno::ENOENT));
    }
}
/* </TESTS> */
