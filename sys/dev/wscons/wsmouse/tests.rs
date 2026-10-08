//! Host tests of the mouse: a fake driver reports through the interface `ums(4)` will use
//! (M15), and the events that reach the queue are checked: buttons, motion and scrolling,
//! the filters (scale with remainder, inversion, swapped axes, reversed scrolling),
//! absolute positions and touches, the queue overflow and `RESYNC`, multitouch pointer
//! control, slot tracking (`wsmouse_mtframe` over `wsmouse_matching`), the parameter ioctls,
//! and a mouse attached by autoconf that joins, leaves and rejoins its mux.

use core::sync::atomic::{AtomicI32, Ordering};
use std::boxed::Box;
use std::vec::Vec;

use super::*;
use crate::dev::wscons::wsconsio::{
    WSCONS_EVENT_HSCROLL, WSCONS_EVENT_MOUSE_ABSOLUTE_X, WSCONS_EVENT_MOUSE_ABSOLUTE_Y,
    WSCONS_EVENT_MOUSE_DELTA_W, WSCONS_EVENT_MOUSE_DELTA_X, WSCONS_EVENT_MOUSE_DELTA_Y,
    WSCONS_EVENT_MOUSE_DELTA_Z, WSCONS_EVENT_MOUSE_DOWN, WSCONS_EVENT_MOUSE_UP,
    WSCONS_EVENT_TOUCH_CONTACTS, WSCONS_EVENT_TOUCH_PRESSURE, WSCONS_EVENT_VSCROLL,
    WSMOUSE_TYPE_USB, WSMOUSEIO_GTYPE, WSMUXIO_ADD_DEVICE, WSMUXIO_LIST_DEVICES,
    WSMUXIO_REMOVE_DEVICE, WsmuxDevice, WsmuxDeviceList,
};
use crate::dev::wscons::wsmousevar::{WSMOUSEHW_MOUSE, WSMOUSEHW_TOUCHPAD, wsmouse_is_mt_code};
use crate::dev::wscons::wsmux::wsmux_do_ioctl;
use crate::kern::init_main::PROC0;
use crate::kern::subr_autoconf::{config_found, config_init, config_rootfound};
use crate::kern::subr_pool::tests::setup_real_memory;
use crate::machine::Machine;
use crate::sys::device::{Cfdata, DV_DULL, FSTATE_NOTFOUND, FSTATE_STAR};
use crate::sys::types::makedev;
use crate::{WSMOUSE_INPUT, WSMOUSE_TOUCH};

/// How many times the fake driver was enabled minus disabled.
static ENABLED: AtomicI32 = AtomicI32::new(0);

fn fake_enable(_v: *mut c_void) -> i32 {
    ENABLED.fetch_add(1, Ordering::Relaxed);
    0
}

fn fake_disable(_v: *mut c_void) {
    ENABLED.fetch_sub(1, Ordering::Relaxed);
}

/// The driver answers `WSMOUSEIO_GTYPE` (a USB mouse) and nothing else.
fn fake_ioctl(
    _v: *mut c_void,
    cmd: u64,
    data: &mut [u8],
    _flag: i32,
    _p: Option<&Proc>,
) -> Result<bool, Errno> {
    if cmd == WSMOUSEIO_GTYPE {
        ioctl_ret(data, &WSMOUSE_TYPE_USB);
        return Ok(true);
    }
    Ok(false)
}

static FAKE_ACCESSOPS: WsmouseAccessops = WsmouseAccessops {
    enable: fake_enable,
    ioctl: fake_ioctl,
    disable: fake_disable,
};

/// A mouse over the fake driver, opened directly: its own queue receives the events.
pub(crate) fn mouse() -> &'static WsmouseSoftc {
    // SAFETY: all-zero is a valid `WsmouseSoftc` (its `Softc` contract); leaked for good.
    let sc: &'static WsmouseSoftc = Box::leak(Box::new(unsafe { core::mem::zeroed() }));
    sc.sc_accessops.set(Some(&FAKE_ACCESSOPS));
    sc.sc_base.me_ops.set(Some(&WSMOUSE_SRCOPS));
    {
        let mut input = sc.input();
        input.evar = Some(NonNull::from(&sc.sc_base.me_evp));
        input.dv = Some(NonNull::from(&sc.sc_base.me_dv));
    }
    wsevent_init(&sc.sc_base.me_evar).unwrap();
    wsmousedoopen(sc, &sc.sc_base.me_evar).unwrap();
    sc
}

pub(crate) fn dev(sc: &WsmouseSoftc) -> &Device {
    &sc.sc_base.me_dv
}

/// The (type, value) of every queued event, emptying the queue.
pub(crate) fn drain(sc: &WsmouseSoftc) -> Vec<(u32, i32)> {
    let ev = sc.sc_base.evp().unwrap();
    let mut out = Vec::new();
    while ev.ws_get.get() != ev.ws_put.get() {
        // SAFETY: an open queue and a published slot nobody writes.
        let e = unsafe { ev.q_read(ev.ws_get.get()) };
        out.push((e.type_, e.value));
        ev.ws_get.set((ev.ws_get.get() + 1) % WSEVENT_QSIZE);
    }
    out
}

const DOWN: u32 = WSCONS_EVENT_MOUSE_DOWN;
const UP: u32 = WSCONS_EVENT_MOUSE_UP;
const DX: u32 = WSCONS_EVENT_MOUSE_DELTA_X;
const DY: u32 = WSCONS_EVENT_MOUSE_DELTA_Y;
const DZ: u32 = WSCONS_EVENT_MOUSE_DELTA_Z;
const DW: u32 = WSCONS_EVENT_MOUSE_DELTA_W;
const AX: u32 = WSCONS_EVENT_MOUSE_ABSOLUTE_X;
const AY: u32 = WSCONS_EVENT_MOUSE_ABSOLUTE_Y;
const PRESSURE: u32 = WSCONS_EVENT_TOUCH_PRESSURE;
const CONTACTS: u32 = WSCONS_EVENT_TOUCH_CONTACTS;
const SYNC: u32 = WSCONS_EVENT_SYNC;

fn set(sc: &WsmouseSoftc, pairs: &[(i32, i32)]) -> Result<(), Errno> {
    let params: Vec<WsmouseParam> = pairs
        .iter()
        .map(|&(key, value)| WsmouseParam { key, value })
        .collect();
    wsmouse_set_params(dev(sc), &params)
}

#[test]
fn buttons_motion_and_wheels_become_one_frame() {
    let _g = setup_real_memory();
    let sc = mouse();
    assert!(ENABLED.load(Ordering::Relaxed) > 0);

    WSMOUSE_INPUT!(dev(sc), 0b101, 3, -2, 1, -1);
    assert_eq!(
        drain(sc),
        [
            (DOWN, 0),
            (DOWN, 2),
            (DX, 3),
            (DY, -2),
            (DZ, 1),
            (DW, -1),
            (SYNC, 0)
        ]
    );
    // One time stamp for the whole frame.
    let ev = sc.sc_base.evp().unwrap();
    // SAFETY: slots 0 and 6 were written by the frame above, and nobody writes them now.
    let (a, b) = unsafe { (ev.q_read(0), ev.q_read(6)) };
    assert_eq!(a.time, b.time);

    WSMOUSE_INPUT!(dev(sc), 0b100, 0, 0, 0, 0);
    assert_eq!(drain(sc), [(UP, 0), (SYNC, 0)]);
    // Nothing changed: no events, not even a SYNC.
    WSMOUSE_INPUT!(dev(sc), 0b100, 0, 0, 0, 0);
    assert!(drain(sc).is_empty());

    // A change that is undone before the sync is no change.
    wsmouse_buttons(dev(sc), 0b110);
    wsmouse_buttons(dev(sc), 0b100);
    wsmouse_input_sync(dev(sc));
    assert!(drain(sc).is_empty());
}

#[test]
fn filters_scale_invert_swap_and_reverse() {
    let _g = setup_real_memory();
    let sc = mouse();

    // Half speed: the remainder carries the half pixel over.
    set(sc, &[(WSMOUSECFG_DX_SCALE, 2048)]).unwrap();
    WSMOUSE_INPUT!(dev(sc), 0, 3, 0, 0, 0);
    WSMOUSE_INPUT!(dev(sc), 0, 3, 0, 0, 0);
    WSMOUSE_INPUT!(dev(sc), 0, -3, 0, 0, 0);
    WSMOUSE_INPUT!(dev(sc), 0, -3, 0, 0, 0);
    assert_eq!(
        drain(sc),
        [
            (DX, 1),
            (SYNC, 0),
            (DX, 2),
            (SYNC, 0),
            (DX, -1),
            (SYNC, 0),
            (DX, -2),
            (SYNC, 0)
        ]
    );
    // 1 * 0.5 with an empty remainder is no event at all.
    WSMOUSE_INPUT!(dev(sc), 0, 1, 0, 0, 0);
    assert!(drain(sc).is_empty());

    set(
        sc,
        &[
            (WSMOUSECFG_DX_SCALE, 0),
            (WSMOUSECFG_X_INV, 1),
            (WSMOUSECFG_SWAPXY, 1),
            (WSMOUSECFG_REVERSE_SCROLLING, 1),
        ],
    )
    .unwrap();
    WSMOUSE_INPUT!(dev(sc), 0, 5, 7, 2, 3);
    assert_eq!(
        drain(sc),
        [(DY, -5), (DX, 7), (DZ, -2), (DW, -3), (SYNC, 0)]
    );

    // A touchpad's wheels are scroll events.
    sc.input().hw.hw_type = WSMOUSEHW_TOUCHPAD;
    set(sc, &[(WSMOUSECFG_REVERSE_SCROLLING, 0)]).unwrap();
    WSMOUSE_INPUT!(dev(sc), 0, 0, 0, 2, 3);
    assert_eq!(
        drain(sc),
        [
            (WSCONS_EVENT_VSCROLL, 2),
            (WSCONS_EVENT_HSCROLL, 3),
            (SYNC, 0)
        ]
    );
}

#[test]
fn absolute_positions_and_touches() {
    let _g = setup_real_memory();
    let sc = mouse();

    WSMOUSE_TOUCH!(dev(sc), 0, 100, 200, 50, 0);
    assert_eq!(
        drain(sc),
        [
            (AX, 100),
            (AY, 200),
            (PRESSURE, 50),
            (CONTACTS, 1),
            (SYNC, 0)
        ]
    );
    // Only Y moved; inverted Y is (inv - y).
    set(sc, &[(WSMOUSECFG_Y_INV, 1000)]).unwrap();
    WSMOUSE_TOUCH!(dev(sc), 0, 100, 250, 50, 0);
    assert_eq!(drain(sc), [(AY, 750), (SYNC, 0)]);

    // The release: pressure 0 drops the (arbitrary) coordinates the driver reports.
    WSMOUSE_TOUCH!(dev(sc), 0, 0, 0, 0, 0);
    assert_eq!(drain(sc), [(PRESSURE, 0), (CONTACTS, 0), (SYNC, 0)]);
    let input = sc.input();
    assert_eq!((input.motion.pos.x, input.motion.pos.y), (100, 250));
}

#[test]
fn pressure_limits_and_hysteresis() {
    let _g = setup_real_memory();
    let sc = mouse();
    let mut input = sc.input();

    // Below the threshold a touch is no touch; a negative pressure is "at the limit".
    input.filter.pressure_lo = 20;
    input.filter.pressure_hi = 30;
    input.touch.min_pressure = 30;
    input.touch(25, 1);
    assert_eq!((input.touch.pressure, input.touch.contacts), (0, 0));
    input.touch(-1, 0);
    assert_eq!((input.touch.pressure, input.touch.contacts), (30, 1));
    // Once touching, the low limit applies until the touch ends.
    wsmouse_touch_update(&mut input);
    assert_eq!(input.touch.min_pressure, 20);
    input.touch.sync = 0;
    input.touch.prev_contacts = 1; // as the sync leaves it
    input.touch(10, 1);
    wsmouse_touch_update(&mut input);
    assert_eq!(input.touch.min_pressure, 30);

    input.filter.h.hysteresis = 4;
    input.filter.v.hysteresis = 4;
    let mut pos = Position {
        acc_dx: 3,
        acc_dy: -3,
        ..Position::default()
    };
    assert!(wsmouse_hysteresis(&input, &pos));
    pos.acc_dy = -4;
    assert!(!wsmouse_hysteresis(&input, &pos));

    // Deltas accumulate while they keep their direction.
    let mut sync = 0;
    set_x(&mut pos, 2, &mut sync, SYNC_X);
    set_x(&mut pos, 2, &mut sync, SYNC_X);
    assert_eq!((pos.x, pos.dx, pos.acc_dx, sync), (2, 2, 5, SYNC_X));
    // A second report before the sync replaces the first one's delta.
    set_x(&mut pos, 4, &mut sync, SYNC_X);
    assert_eq!((pos.x, pos.dx, pos.acc_dx), (4, 4, 7));
    sync = 0;
    set_x(&mut pos, 1, &mut sync, SYNC_X);
    assert_eq!((pos.dx, pos.acc_dx), (-3, -3));
}

#[test]
fn a_full_queue_drops_the_frame_and_resyncs() {
    let _g = setup_real_memory();
    let sc = mouse();
    let ev = sc.sc_base.evp().unwrap();

    // Room for one event: the frame needs four.
    ev.ws_get.set(2);
    ev.ws_put.set(0);
    WSMOUSE_INPUT!(dev(sc), 1, 4, 0, 0, 0);
    wsmouse_position(dev(sc), 10, 10);
    wsmouse_input_sync(dev(sc));
    assert_eq!(ev.ws_put.get(), 0, "nothing published");
    assert_ne!(sc.input().flags & RESYNC, 0);

    // With room again the button and the position come back; the stale delta does not.
    ev.ws_get.set(0);
    wsmouse_input_sync(dev(sc));
    assert_eq!(drain(sc), [(DOWN, 0), (AX, 10), (AY, 10), (SYNC, 0)]);
    assert_eq!(sc.input().flags & RESYNC, 0);
}

#[test]
fn the_pointer_follows_the_moving_touch() {
    let _g = setup_real_memory();
    let sc = mouse();
    let d = dev(sc);
    wsmouse_mt_init(d, 3, false).unwrap();
    assert!(
        wsmouse_mt_init(d, 3, false).is_ok(),
        "same layout: nothing to do"
    );

    wsmouse_mtstate(d, 0, 100, 100, 50);
    wsmouse_input_sync(d);
    assert_eq!(
        drain(sc),
        [
            (AX, 100),
            (AY, 100),
            (PRESSURE, 50),
            (CONTACTS, 1),
            (SYNC, 0)
        ]
    );
    assert_eq!(sc.input().mt.ptr, 0b01);

    // A second touch does not take the pointer.
    wsmouse_mtstate(d, 1, 500, 500, 60);
    wsmouse_input_sync(d);
    assert_eq!(drain(sc), [(CONTACTS, 2), (SYNC, 0)]);

    // Moving only the second touch: after a whole cycle without the first one moving,
    // the pointer jumps to it, without a jump of the deltas.
    for (i, x) in [510, 520, 530].into_iter().enumerate() {
        wsmouse_mtstate(d, 1, x, x, 60);
        wsmouse_input_sync(d);
        let ptr = sc.input().mt.ptr;
        assert_eq!(ptr, if i < 2 { 0b01 } else { 0b10 }, "frame {i}");
    }
    let evs = drain(sc);
    assert_eq!(
        evs[evs.len() - 4..],
        [(AX, 530), (AY, 530), (PRESSURE, 60), (SYNC, 0)]
    );
    assert_eq!(sc.input().motion.pos.dx, 0);

    // Lifting the first touch leaves the second in control.
    wsmouse_mtstate(d, 0, 0, 0, 0);
    wsmouse_input_sync(d);
    assert_eq!(drain(sc), [(CONTACTS, 1), (SYNC, 0)]);
    let input = sc.input();
    assert_eq!(
        (input.mt.touches, input.mt.ptr, input.mt.num_touches),
        (0b10, 0b10, 1)
    );
}

#[test]
fn set_routes_single_values() {
    let _g = setup_real_memory();
    let sc = mouse();
    let d = dev(sc);
    wsmouse_mt_init(d, 2, false).unwrap();
    assert!(wsmouse_is_mt_code(WSMOUSE_MT_PRESSURE));

    wsmouse_set(d, WSMOUSE_ABS_X, 10, 0);
    wsmouse_set(d, WSMOUSE_REL_X, 5, 0);
    wsmouse_set(d, WSMOUSE_ABS_Y, 7, 0);
    wsmouse_set(d, WSMOUSE_MT_PRESSURE, 40, 1);
    wsmouse_set(d, WSMOUSE_MT_ABS_X, 300, 1);
    wsmouse_set(d, WSMOUSE_MT_REL_Y, 20, 1);
    wsmouse_set(d, WSMOUSE_MT_ABS_X, 1, 9); // no such slot
    wsmouse_set(d, WSMOUSE_TOUCH_WIDTH, 3, 0);
    let input = sc.input();
    assert_eq!((input.motion.pos.x, input.motion.pos.y), (15, 7));
    let s = input.mt.slots()[1];
    assert_eq!((s.pos.x, s.pos.y, s.pressure), (300, 20, 40));
    assert_eq!(input.mt.touches, 0b10);
    assert_eq!(input.touch.width, 3);
}

#[test]
fn tracking_ids_and_frames_keep_their_slots() {
    let _g = setup_real_memory();
    let sc = mouse();
    let d = dev(sc);

    assert_eq!(wsmouse_id_to_slot(d, 7), -1, "no slots yet");
    wsmouse_mt_init(d, 2, false).unwrap();
    assert_eq!(wsmouse_id_to_slot(d, 7), 0);
    assert_eq!(wsmouse_id_to_slot(d, 9), 1);
    assert_eq!(wsmouse_id_to_slot(d, 11), -1, "both slots taken this frame");
    wsmouse_mtstate(d, 0, 1, 1, 50);
    wsmouse_input_sync(d);
    assert_eq!(wsmouse_id_to_slot(d, 7), 0, "a known touch");

    // Point-based input with tracking.
    wsmouse_mt_init(d, 3, true).unwrap();
    let mut pts = [
        Mtpoint {
            x: 10,
            y: 10,
            pressure: 50,
            slot: -1,
        },
        Mtpoint {
            x: 100,
            y: 100,
            pressure: 50,
            slot: -1,
        },
    ];
    wsmouse_mtframe(d, &mut pts);
    assert_eq!((pts[0].slot, pts[1].slot), (0, 1));
    wsmouse_input_sync(d);

    // The same touches in the other order, slightly moved.
    let mut pts = [
        Mtpoint {
            x: 102,
            y: 101,
            pressure: 50,
            slot: -1,
        },
        Mtpoint {
            x: 11,
            y: 12,
            pressure: 50,
            slot: -1,
        },
    ];
    wsmouse_mtframe(d, &mut pts);
    assert_eq!((pts[0].slot, pts[1].slot), (1, 0));
    wsmouse_input_sync(d);

    // One touch left: the nearest slot keeps it, the other is released.
    let mut pts = [Mtpoint {
        x: 12,
        y: 12,
        pressure: 50,
        slot: -1,
    }];
    wsmouse_mtframe(d, &mut pts);
    assert_eq!(pts[0].slot, 0);
    wsmouse_input_sync(d);
    assert_eq!(sc.input().mt.touches, 0b001);

    // Beyond the tracking distance a point is a new touch.
    set(sc, &[(WSMOUSECFG_TRKMAXDIST, 5)]).unwrap();
    let mut pts = [Mtpoint {
        x: 200,
        y: 200,
        pressure: 50,
        slot: -1,
    }];
    wsmouse_mtframe(d, &mut pts);
    assert_eq!(pts[0].slot, 1);
    wsmouse_input_sync(d);
    assert_eq!(sc.input().mt.touches, 0b010);

    wsmouse_input_cleanup(&mut sc.input());
    assert!(sc.input().mt.slots.is_none());
}

/// A tiny deterministic generator for the matching test.
fn lcg(seed: &mut u32) -> i32 {
    *seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
    ((*seed >> 16) % 1000) as i32
}

/// The minimum of the sum over every assignment of the n columns to distinct rows.
fn brute_force(matrix: &[i32], m: usize, n: usize) -> i32 {
    fn go(matrix: &[i32], m: usize, n: usize, col: usize, used: &mut [bool]) -> i32 {
        if col == n {
            return 0;
        }
        let mut best = i32::MAX;
        for row in 0..m {
            if !used[row] {
                used[row] = true;
                let rest = go(matrix, m, n, col + 1, used);
                used[row] = false;
                best = best.min(matrix[row * n + col] + rest);
            }
        }
        best
    }
    go(matrix, m, n, 0, &mut [false; 8])
}

#[test]
fn matching_finds_the_minimum_weight_assignment() {
    let mut seed = 1;
    for (m, n) in [(1, 1), (2, 2), (3, 2), (4, 4), (5, 3), (6, 6), (4, 0)] {
        for _ in 0..30 {
            let matrix: Vec<i32> = (0..m * n).map(|_| lcg(&mut seed)).collect();
            let mut buffer = std::vec![0; 3 * m + 3 * n];
            wsmouse_matching(&matrix, m, n, &mut buffer);
            let r2c = &buffer[..m];
            let mut seen = std::vec![false; n];
            let mut sum = 0;
            for (row, &col) in r2c.iter().enumerate() {
                if col >= 0 {
                    assert!(!seen[col as usize], "column {col} twice");
                    seen[col as usize] = true;
                    sum += matrix[row * n + col as usize];
                }
            }
            assert!(seen.iter().all(|&s| s), "every column assigned");
            assert_eq!(sum, brute_force(&matrix, m, n), "{m}x{n} {matrix:?}");
        }
    }
}

#[test]
fn parameter_ioctls_copy_in_and_out() {
    let _g = setup_real_memory();
    let sc = mouse();

    let mut pairs = [
        WsmouseParam {
            key: WSMOUSECFG_DX_SCALE,
            value: 8192,
        },
        WsmouseParam {
            key: WSMOUSECFG_SMOOTHING,
            value: 13,
        },
        WsmouseParam {
            key: WSMOUSECFG_LOG_EVENTS,
            value: 1,
        },
        WsmouseParam {
            key: WSMOUSECFG_PRESSURE_LO,
            value: 40,
        },
    ];
    let mut data = [0u8; size_of::<WsmouseParameters>()];
    let mut args = WsmouseParameters {
        params: pairs.as_mut_ptr() as usize,
        nparams: pairs.len() as u32,
        _pad0: [0; 4],
    };
    ioctl_ret(&mut data, &args);
    wsmouse_do_ioctl(sc, WSMOUSEIO_SETPARAMS, &mut data, FWRITE, None).unwrap();
    {
        let input = sc.input();
        assert_eq!(input.filter.h.scale, 8192);
        assert_eq!(input.filter.mode, 13 & SMOOTHING_MASK);
        assert_ne!(input.flags & LOG_EVENTS, 0);
        assert_eq!(
            (input.filter.pressure_lo, input.filter.pressure_hi),
            (40, 40)
        );
    }

    let mut out = [
        WsmouseParam {
            key: WSMOUSECFG_SMOOTHING,
            value: 0,
        },
        WsmouseParam {
            key: WSMOUSECFG_STRONG_HYSTERESIS,
            value: 9,
        },
        WsmouseParam {
            key: WSMOUSECFG_PRESSURE_HI,
            value: 0,
        },
        WsmouseParam {
            key: WSMOUSECFG_LOG_EVENTS,
            value: 0,
        },
    ];
    args.params = out.as_mut_ptr() as usize;
    ioctl_ret(&mut data, &args);
    wsmouse_do_ioctl(sc, WSMOUSEIO_GETPARAMS, &mut data, FREAD, None).unwrap();
    assert_eq!(
        out.map(|p| p.value),
        [(13 & SMOOTHING_MASK) as i32, 0, 40, 1]
    );

    // An unknown key, too many pairs, no array.
    let mut bad = [WsmouseParam {
        key: 1000,
        value: 0,
    }];
    args.params = bad.as_mut_ptr() as usize;
    args.nparams = 1;
    ioctl_ret(&mut data, &args);
    assert_eq!(
        wsmouse_do_ioctl(sc, WSMOUSEIO_GETPARAMS, &mut data, FREAD, None),
        Err(Errno::EINVAL)
    );
    args.nparams = WSMOUSECFG_MAX + 1;
    ioctl_ret(&mut data, &args);
    assert_eq!(
        wsmouse_do_ioctl(sc, WSMOUSEIO_GETPARAMS, &mut data, FREAD, None),
        Err(Errno::EINVAL)
    );
    args.params = 0;
    args.nparams = 1;
    ioctl_ret(&mut data, &args);
    assert_eq!(
        wsmouse_do_ioctl(sc, WSMOUSEIO_SETPARAMS, &mut data, FWRITE, None),
        Err(Errno::EINVAL)
    );

    // The driver's ioctls, and the rest.
    let mut t = [0u8; 4];
    wsmouse_do_ioctl(sc, WSMOUSEIO_GTYPE, &mut t, FREAD, None).unwrap();
    assert_eq!(ioctl_arg::<u32>(&t), WSMOUSE_TYPE_USB);
    assert_eq!(
        wsmouse_do_ioctl(sc, WSMOUSEIO_GTYPE + 1, &mut t, FREAD, None),
        Err(Errno::ENOTTY)
    );
    assert_eq!(
        wsmouse_do_ioctl(sc, FIOASYNC, &mut t, FREAD, None),
        Err(Errno::EACCES)
    );
    sc.sc_dying.set(1);
    assert_eq!(
        wsmouse_do_ioctl(sc, WSMOUSEIO_GTYPE, &mut t, FREAD, None),
        Err(Errno::EIO)
    );
}

#[test]
fn modes_and_configuration() {
    let _g = setup_real_memory();
    let sc = mouse();
    let d = dev(sc);

    assert!(wsmouse_set_mode(d, WSMOUSE_NATIVE));
    assert_eq!(
        sc.input().flags & (TPAD_NATIVE_MODE | TPAD_COMPAT_MODE),
        TPAD_NATIVE_MODE
    );
    assert!(wsmouse_set_mode(d, WSMOUSE_COMPAT));
    assert_eq!(
        sc.input().flags & (TPAD_NATIVE_MODE | TPAD_COMPAT_MODE),
        TPAD_COMPAT_MODE
    );
    assert!(!wsmouse_set_mode(d, 7));
    sc.input().flags = 0;

    {
        let mut hw = wsmouse_get_hw(d);
        hw.type_ = WSMOUSE_TYPE_USB as i32;
        hw.hw_type = WSMOUSEHW_MOUSE;
        hw.x_max = 1000;
        hw.y_min = 10;
        hw.y_max = 600;
        hw.h_res = 30;
        hw.v_res = 20;
        hw.flags = WSMOUSEHW_LR_DOWN;
    }
    let params = [WsmouseParam {
        key: WSMOUSECFG_DY_SCALE,
        value: 4096,
    }];
    wsmouse_configure(d, Some(&params)).unwrap();
    {
        let input = sc.input();
        assert_eq!(input.filter.v.inv, 610);
        assert_eq!(input.filter.ratio, (1 << 12) * 30 / 20);
        assert_eq!(input.filter.v.scale, 4096);
        assert_ne!(input.flags & CONFIGURED, 0);
        assert_eq!(
            input.flags & TPAD_COMPAT_MODE,
            0,
            "a mouse has no compat mode"
        );
    }
    // Configured once: a second call changes nothing.
    sc.input().filter.v.inv = 0;
    wsmouse_configure(d, None).unwrap();
    assert_eq!(sc.input().filter.v.inv, 0);
}

/// The guard marks the input busy while it lives (a second `input()` would panic, which
/// on the host ends the test process: `boot(RB_HALT)`).
#[test]
fn the_input_guard_marks_the_input_busy() {
    let _g = setup_real_memory();
    let sc = mouse();
    let a = sc.input();
    assert!(sc.input_busy.get());
    drop(a);
    assert!(!sc.input_busy.get());
    let hw = wsmouse_get_hw(dev(sc));
    assert!(sc.input_busy.get());
    drop(hw);
    assert!(!sc.input_busy.get());
}

/// The root of the test `ioconf`: a fake mouse driver that attaches one wsmouse.
fn tms_match(_parent: Option<&Device>, _m: &CfMatch, _aux: *mut c_void) -> i32 {
    1
}

fn tms_attach(_parent: Option<&Device>, self_: &Device, _aux: *mut c_void) {
    let mut a = WsmousedevAttachArgs {
        accessops: &FAKE_ACCESSOPS,
        accesscookie: ptr::null_mut(),
    };
    let child = config_found(self_, ptr::from_mut(&mut a).cast(), Some(wsmousedevprint));
    assert!(child.is_some());
}

static TMS_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<Device>(),
    ca_match: Some(tms_match),
    ca_attach: tms_attach,
    ca_detach: None,
    ca_activate: None,
};
static TMS_CD: Cfdriver = Cfdriver::new(b"tms", DV_DULL, 0);

/// `tms0 at root`, `wsmouse* at tms? mux 20`.
static IOCONF: [Cfdata; 2] = [
    Cfdata::new(&TMS_CA, &TMS_CD, 0, FSTATE_NOTFOUND, &[], 0, &[], 0, 0),
    Cfdata::new(
        &WSMOUSE_CA,
        &WSMOUSE_CD,
        0,
        FSTATE_STAR,
        &[20],
        0,
        &[0],
        0,
        0,
    ),
];

/// A mux's device list.
fn list(mux: &WsmuxSoftc) -> Vec<(i32, i32)> {
    let mut data = [0u8; size_of::<WsmuxDeviceList>()];
    wsmux_do_ioctl(
        &mux.sc_base.me_dv,
        WSMUXIO_LIST_DEVICES,
        &mut data,
        FREAD,
        None,
    )
    .unwrap();
    let l: WsmuxDeviceList = ioctl_arg(&data);
    l.devices[..l.ndevices as usize]
        .iter()
        .map(|d| (d.type_, d.idx))
        .collect()
}

fn mux_ioctl(mux: &WsmuxSoftc, cmd: u64, idx: i32) -> Result<(), Errno> {
    let mut data = [0u8; size_of::<WsmuxDevice>()];
    ioctl_ret(
        &mut data,
        &WsmuxDevice {
            type_: WSMUX_MOUSE,
            idx,
        },
    );
    wsmux_do_ioctl(&mux.sc_base.me_dv, cmd, &mut data, FWRITE, None)
}

#[test]
fn an_attached_mouse_feeds_its_mux() {
    let _g = setup_real_memory();
    // SAFETY: `setup_real_memory`'s lock serialises the tests that install an ioconf.
    unsafe { Machine::set_ioconf(&IOCONF, &[0]) };
    config_init();
    assert!(config_rootfound(b"tms", ptr::null_mut()).is_some());
    let unit = (0..WSMOUSE_CD.cd_ndevs.get())
        .rev()
        .find(|&u| WSMOUSE_CD.cd_dev(u).is_some())
        .unwrap();
    let sc = wsmouse_sc(unit).unwrap();
    let mux = wsmux_getmux(20).unwrap();
    assert_eq!(list(mux), [(WSMUX_MOUSE, unit)]);
    assert!(sc.sc_base.parent().is_some_and(|p| ptr::eq(p, mux)));

    // An open mux receives the mouse's events.
    let evar = &mux.sc_base.me_evar;
    wsevent_init(evar).unwrap();
    crate::dev::wscons::wsmux::wsmux_do_open(mux, evar).unwrap();
    WSMOUSE_INPUT!(dev(sc), 1, 0, 2, 0, 0);
    let mut got = Vec::new();
    while evar.ws_get.get() != evar.ws_put.get() {
        // SAFETY: published slots of the open queue.
        let e = unsafe { evar.q_read(evar.ws_get.get()) };
        got.push((e.type_, e.value));
        evar.ws_get.set(evar.ws_get.get() + 1);
    }
    assert_eq!(got, [(DOWN, 0), (DY, 2), (SYNC, 0)]);
    crate::dev::wscons::wsmux::wsmux_do_close(mux);
    mux.sc_base.me_evp.set(None);
    wsevent_fini(evar);

    // Out of the mux, back in through WSMUXIO_ADD_DEVICE (wsmouse_add_mux).
    mux_ioctl(mux, WSMUXIO_REMOVE_DEVICE, unit).unwrap();
    assert!(list(mux).is_empty());
    mux_ioctl(mux, WSMUXIO_ADD_DEVICE, unit).unwrap();
    assert_eq!(list(mux), [(WSMUX_MOUSE, unit)]);
    assert_eq!(mux_ioctl(mux, WSMUXIO_ADD_DEVICE, unit), Err(Errno::EBUSY));
    assert_eq!(
        mux_ioctl(mux, WSMUXIO_ADD_DEVICE, unit + 1),
        Err(Errno::ENXIO)
    );

    // Opening /dev/wsmouseN takes the mouse out of its mux; closing puts it back.
    let d = makedev(68, unit as u32);
    let before = ENABLED.load(Ordering::Relaxed);
    wsmouseopen(d, FREAD, 0, &PROC0).unwrap();
    assert!(list(mux).is_empty());
    assert_eq!(wsmouseopen(d, FREAD, 0, &PROC0), Err(Errno::EBUSY));
    assert_eq!(ENABLED.load(Ordering::Relaxed), before + 1);
    let mut t = [0u8; 4];
    wsmouseioctl(d, WSMOUSEIO_GTYPE, &mut t, FREAD, &PROC0).unwrap();
    wsmouseclose(d, FREAD, 0, None).unwrap();
    assert_eq!(ENABLED.load(Ordering::Relaxed), before);
    assert_eq!(list(mux), [(WSMUX_MOUSE, unit)]);
    assert_eq!(
        wsmouseopen(makedev(68, 99), FREAD, 0, &PROC0),
        Err(Errno::ENXIO)
    );
}
