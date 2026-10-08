//! Host tests of the touchpad processing over synthetic touch sequences: the defaults of
//! `wstpad_configure`, one- and two-finger taps (the click comes from the tap timeout),
//! a long touch that is no tap, tap-and-drag, edge scrolling on a single-touch pad,
//! two-finger scrolling on an MT pad, the parameters, and the geometry helpers.

use std::sync::MutexGuard;
use std::vec::Vec;

use super::*;
use crate::WSMOUSE_TOUCH;
use crate::dev::wscons::wsconsio::{
    WSCONS_EVENT_MOUSE_DELTA_X, WSCONS_EVENT_MOUSE_DELTA_Y, WSCONS_EVENT_MOUSE_DOWN,
    WSCONS_EVENT_MOUSE_UP, WSCONS_EVENT_SYNC, WSCONS_EVENT_VSCROLL, WSMOUSE_TYPE_TOUCHPAD,
    WSMOUSECFG_DECELERATION, WSMOUSECFG_X_HYSTERESIS, WsmouseParam,
};
use crate::dev::wscons::wsmouse::tests::{dev, drain, mouse};
use crate::dev::wscons::wsmouse::{
    WsmouseSoftc, wsmouse_configure, wsmouse_get_hw, wsmouse_get_params, wsmouse_input_cleanup,
    wsmouse_input_sync, wsmouse_mtstate, wsmouse_set_params,
};
use crate::dev::wscons::wsmousevar::WSMOUSEHW_TOUCHPAD;
use crate::kern::subr_pool::tests::setup_real_memory;

const DOWN: u32 = WSCONS_EVENT_MOUSE_DOWN;
const UP: u32 = WSCONS_EVENT_MOUSE_UP;
const SYNC: u32 = WSCONS_EVENT_SYNC;

/// Real memory for malloc(9), then the timeout wheel (the tap timeout is added to it), in
/// the order `kern_event`'s tests take them.
fn setup() -> (MutexGuard<'static, ()>, MutexGuard<'static, ()>) {
    let mem = setup_real_memory();
    let wheel = crate::kern::kern_timeout::tests::LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    crate::kern::kern_timeout::timeout_startup();
    (mem, wheel)
}

/// A 3000 x 2000 touchpad over the fake driver, opened, configured and in compat mode;
/// `mt_slots` 0 is a single-touch pad. Taps are on (one, two and three fingers: left,
/// right and middle button).
fn touchpad(mt_slots: i32, contacts_max: i32) -> &'static WsmouseSoftc {
    let sc = mouse();
    let d = dev(sc);
    {
        let mut hw = wsmouse_get_hw(d);
        hw.type_ = WSMOUSE_TYPE_TOUCHPAD as i32;
        hw.hw_type = WSMOUSEHW_TOUCHPAD;
        hw.x_max = 3000;
        hw.y_max = 2000;
        hw.mt_slots = mt_slots;
        hw.contacts_max = contacts_max;
    }
    wsmouse_configure(d, None).unwrap();
    let taps = [
        WsmouseParam {
            key: WSMOUSECFG_TAP_ONE_BTNMAP,
            value: 1,
        },
        WsmouseParam {
            key: WSMOUSECFG_TAP_TWO_BTNMAP,
            value: 3,
        },
        WsmouseParam {
            key: WSMOUSECFG_TAP_THREE_BTNMAP,
            value: 2,
        },
    ];
    wsmouse_set_params(d, &taps).unwrap();
    sc
}

/// Runs `f` on the input and its touchpad state.
fn with_tp<R>(sc: &WsmouseSoftc, f: impl FnOnce(&mut WsmouseInput, &mut Wstpad) -> R) -> R {
    let mut guard = sc.input();
    let input: &mut WsmouseInput = &mut guard;
    // SAFETY: the guard gives this function the input exclusively, and `f` gets the only
    // reference to the touchpad state.
    let tp = unsafe { tp_of(input) }.unwrap();
    f(input, tp)
}

/// The tap timeout firing.
fn tap_timeout(sc: &WsmouseSoftc) {
    wstpad_tap_timeout(ptr::from_ref(dev(sc)).cast_mut().cast());
}

/// Frees the touchpad state, which also deletes a pending tap timeout.
fn finish(sc: &WsmouseSoftc) {
    wsmouse_input_cleanup(&mut sc.input());
    assert!(sc.input().tp.is_none());
}

/// Moves the start of every touch one second back: it is no tap any more.
fn age_touches(sc: &WsmouseSoftc) {
    with_tp(sc, |_, tp| {
        for t in &mut tp.tpad_touches {
            t.orig.time.tv_sec -= 1;
        }
    });
}

#[test]
fn configure_derives_the_defaults_from_the_surface() {
    let _g = setup();
    let sc = touchpad(0, 1);
    with_tp(sc, |input, tp| {
        // diag = isqrt(3000^2 + 2000^2) = 3605, h_unit = 3605 / 280 = 12
        assert_eq!(input.filter.h.scale, (920 << 12) / 3605);
        assert_eq!(input.filter.h.hysteresis, 12);
        assert_eq!(input.filter.dclr, 10);
        assert_eq!(input.filter.h.mag_scale, (8 << 12) / 10);
        assert_ne!(input.flags & TPAD_COMPAT_MODE, 0);
        assert_eq!(
            tp.features, WSTPAD_EDGESCROLL,
            "contacts_max 1: edge scrolling"
        );
        assert_eq!(
            tp.handlers,
            (1 << EDGESCROLL_HDLR) | (1 << TAP_HDLR),
            "taps on"
        );
        assert_eq!((tp.edge.left, tp.edge.right), (150, 2850));
        assert_eq!((tp.edge.bottom, tp.edge.top), (i32::MIN, i32::MAX));
        assert_eq!(
            (tp.edge.center, tp.edge.center_left, tp.edge.center_right),
            (1500, 1313, 1687)
        );
        assert_eq!((tp.scroll.vdist, tp.tap.maxdist), (48, 48));
        assert_eq!(tp.tap.clicktime, TAP_CLICKTIME_DEFAULT);
        assert_eq!(tp.tap.btnmap, [LEFTBTN, RIGHTBTN, MIDDLEBTN]);
    });

    // The parameters, through wsmouse's.
    let mut p = [
        WsmouseParam {
            key: WSMOUSECFG_EDGESCROLL,
            value: 0,
        },
        WsmouseParam {
            key: WSMOUSECFG_TWOFINGERSCROLL,
            value: 9,
        },
        WsmouseParam {
            key: WSMOUSECFG_LEFT_EDGE,
            value: 0,
        },
        WsmouseParam {
            key: WSMOUSECFG_TAP_MAXTIME,
            value: 0,
        },
        WsmouseParam {
            key: WSMOUSECFG_TAP_TWO_BTNMAP,
            value: 0,
        },
        WsmouseParam {
            key: WSMOUSECFG_MTBTN_MAXDIST,
            value: 0,
        },
        WsmouseParam {
            key: WSMOUSECFG_X_HYSTERESIS,
            value: 0,
        },
    ];
    wsmouse_get_params(dev(sc), &mut p).unwrap();
    assert_eq!(
        p.map(|p| p.value),
        [1, 0, V_EDGE_RATIO_DEFAULT, 180, 3, -1, 12]
    );
    with_tp(sc, |input, _| {
        assert_eq!(
            wstpad_get_param(input, WSMOUSECFG_DECELERATION),
            Err(Errno::ENOTSUP)
        );
        assert_eq!(wstpad_set_param(input, 1000, 1), Err(Errno::ENOTSUP));
        wstpad_set_param(input, WSMOUSECFG_TAP_MAXTIME, 5000).unwrap();
        assert_eq!(wstpad_get_param(input, WSMOUSECFG_TAP_MAXTIME), Ok(999));
        wstpad_set_param(input, WSMOUSECFG_TAP_ONE_BTNMAP, 33).unwrap();
        assert_eq!(wstpad_get_param(input, WSMOUSECFG_TAP_ONE_BTNMAP), Ok(0));
    });
    finish(sc);
}

#[test]
fn a_one_finger_tap_is_a_left_click() {
    let _g = setup();
    let sc = touchpad(0, 1);
    let d = dev(sc);

    WSMOUSE_TOUCH!(d, 0, 1500, 1000, 50, 0);
    WSMOUSE_TOUCH!(d, 0, 1500, 1000, 0, 0);
    assert!(drain(sc).is_empty(), "a touch alone reports nothing");
    with_tp(sc, |_, tp| {
        assert_eq!(tp.tap.state, TAP_LIFTED);
        assert_eq!(tp.tap.pending, LEFTBTN);
    });

    tap_timeout(sc);
    assert_eq!(drain(sc), [(DOWN, 0), (SYNC, 0)]);
    tap_timeout(sc);
    assert_eq!(drain(sc), [(UP, 0), (SYNC, 0)]);
    with_tp(sc, |input, tp| {
        assert_eq!((tp.tap.state, tp.tap.button), (TAP_DETECT, 0));
        assert_eq!(input.sbtn.buttons, 0);
    });
    finish(sc);
}

#[test]
fn a_two_finger_tap_is_a_right_click() {
    let _g = setup();
    let sc = touchpad(0, 1);
    let d = dev(sc);

    WSMOUSE_TOUCH!(d, 0, 1500, 1000, 50, 2);
    WSMOUSE_TOUCH!(d, 0, 1500, 1000, 0, 0);
    tap_timeout(sc);
    tap_timeout(sc);
    assert_eq!(drain(sc), [(DOWN, 2), (SYNC, 0), (UP, 2), (SYNC, 0)]);
    finish(sc);
}

#[test]
fn a_long_touch_is_no_tap() {
    let _g = setup();
    let sc = touchpad(0, 1);
    let d = dev(sc);

    WSMOUSE_TOUCH!(d, 0, 1500, 1000, 50, 0);
    age_touches(sc);
    WSMOUSE_TOUCH!(d, 0, 1500, 1000, 0, 0);
    assert!(drain(sc).is_empty());
    with_tp(sc, |_, tp| {
        assert_eq!(
            (tp.tap.state, tp.tap.pending, tp.tap.button),
            (TAP_DETECT, 0, 0)
        );
    });
    finish(sc);
}

#[test]
fn tap_and_drag_holds_the_button() {
    let _g = setup();
    let sc = touchpad(0, 1);
    let d = dev(sc);

    // A tap, and a new touch before the click delay: the button goes down at once.
    WSMOUSE_TOUCH!(d, 0, 1500, 1000, 50, 0);
    WSMOUSE_TOUCH!(d, 0, 1500, 1000, 0, 0);
    WSMOUSE_TOUCH!(d, 0, 1500, 1000, 50, 0);
    assert_eq!(drain(sc), [(DOWN, 0), (SYNC, 0)]);
    with_tp(sc, |_, tp| {
        assert_eq!((tp.tap.state, tp.tap.button), (TAP_DETECT, LEFTBTN));
    });

    // The drag moves the pointer.
    WSMOUSE_TOUCH!(d, 0, 1600, 1000, 50, 0);
    let evs = drain(sc);
    assert!(
        evs.iter()
            .any(|&(t, v)| t == WSCONS_EVENT_MOUSE_DELTA_X && v > 0),
        "{evs:?}"
    );
    assert!(!evs.iter().any(|&(t, _)| t == DOWN || t == UP));

    // Lifted after a long drag (no tap): the button goes up.
    age_touches(sc);
    WSMOUSE_TOUCH!(d, 0, 1600, 1000, 0, 0);
    assert_eq!(drain(sc), [(UP, 0), (SYNC, 0)]);
    finish(sc);
}

#[test]
fn a_locked_drag_ends_by_timeout_or_by_a_tap() {
    let _g = setup();
    let sc = touchpad(0, 1);
    let d = dev(sc);
    let lock = [WsmouseParam {
        key: WSMOUSECFG_TAP_LOCKTIME,
        value: 20,
    }];
    wsmouse_set_params(d, &lock).unwrap();
    with_tp(sc, |_, tp| {
        assert_eq!(tp.tap.locktime, 150, "clamped to 150 .. 5000")
    });

    // Tap, touch again (the button goes down), lift after a long drag: locked.
    WSMOUSE_TOUCH!(d, 0, 1500, 1000, 50, 0);
    WSMOUSE_TOUCH!(d, 0, 1500, 1000, 0, 0);
    WSMOUSE_TOUCH!(d, 0, 1500, 1000, 50, 0);
    age_touches(sc);
    WSMOUSE_TOUCH!(d, 0, 1500, 1000, 0, 0);
    assert_eq!(drain(sc), [(DOWN, 0), (SYNC, 0)]);
    with_tp(sc, |_, tp| assert_eq!(tp.tap.state, TAP_LOCKED));

    // A touch resumes the drag; a quick tap ends it ("tap-to-end").
    WSMOUSE_TOUCH!(d, 0, 1500, 1000, 50, 0);
    with_tp(sc, |_, tp| assert_eq!(tp.tap.state, TAP_LOCKED_DRAG));
    WSMOUSE_TOUCH!(d, 0, 1500, 1000, 0, 0);
    assert_eq!(drain(sc), [(UP, 0), (SYNC, 0)]);
    with_tp(sc, |_, tp| {
        assert_eq!((tp.tap.state, tp.tap.button), (TAP_DETECT, 0))
    });

    // Locked again; this time the lock time runs out.
    WSMOUSE_TOUCH!(d, 0, 1500, 1000, 50, 0);
    WSMOUSE_TOUCH!(d, 0, 1500, 1000, 0, 0);
    WSMOUSE_TOUCH!(d, 0, 1500, 1000, 50, 0);
    age_touches(sc);
    WSMOUSE_TOUCH!(d, 0, 1500, 1000, 0, 0);
    assert_eq!(drain(sc), [(DOWN, 0), (SYNC, 0)]);
    tap_timeout(sc);
    assert_eq!(drain(sc), [(UP, 0), (SYNC, 0)]);
    with_tp(sc, |_, tp| assert_eq!(tp.tap.state, TAP_DETECT));
    finish(sc);
}

#[test]
fn moving_at_the_right_edge_scrolls() {
    let _g = setup();
    let sc = touchpad(0, 1);
    let d = dev(sc);

    WSMOUSE_TOUCH!(d, 0, 2900, 500, 50, 0);
    assert!(drain(sc).is_empty());
    let mut scrolls = Vec::new();
    for y in [540, 580, 620] {
        WSMOUSE_TOUCH!(d, 0, 2900, y, 50, 0);
        for (t, v) in drain(sc) {
            assert!(
                t != WSCONS_EVENT_MOUSE_DELTA_X && t != WSCONS_EVENT_MOUSE_DELTA_Y,
                "the edge freezes the pointer"
            );
            if t == WSCONS_EVENT_VSCROLL {
                scrolls.push(v);
            }
        }
    }
    // Upward on the pad is scrolling up: negative values, every frame.
    assert_eq!(scrolls.len(), 3, "{scrolls:?}");
    assert!(scrolls.iter().all(|&v| v < 0), "{scrolls:?}");
    // The first step: dz = -dy * 4096 / (vdist * n), with n = 3 at this speed.
    assert_eq!(scrolls[0], -40 * 4096 / (48 * 3));

    // The same move in the middle of the pad moves the pointer instead.
    WSMOUSE_TOUCH!(d, 0, 2900, 620, 0, 0);
    WSMOUSE_TOUCH!(d, 0, 1500, 500, 50, 0);
    drain(sc);
    WSMOUSE_TOUCH!(d, 0, 1500, 540, 50, 0);
    let evs = drain(sc);
    assert!(
        evs.iter().any(|&(t, _)| t == WSCONS_EVENT_MOUSE_DELTA_Y),
        "{evs:?}"
    );
    assert!(!evs.iter().any(|&(t, _)| t == WSCONS_EVENT_VSCROLL));
    finish(sc);
}

#[test]
fn two_fingers_moving_together_scroll() {
    let _g = setup();
    let sc = touchpad(2, 0);
    let d = dev(sc);
    with_tp(sc, |_, tp| {
        assert_ne!(tp.features & WSTPAD_MT, 0);
        assert_ne!(tp.handlers & (1 << F2SCROLL_HDLR), 0);
        assert_eq!(tp.tpad_touches[1].pos, TouchPos::Slot(1));
    });

    wsmouse_mtstate(d, 0, 1000, 500, 50);
    wsmouse_mtstate(d, 1, 1400, 500, 50);
    wsmouse_input_sync(d);
    assert!(drain(sc).is_empty());

    let mut scrolls = Vec::new();
    for y in [540, 580] {
        wsmouse_mtstate(d, 0, 1000, y, 50);
        wsmouse_mtstate(d, 1, 1400, y, 50);
        wsmouse_input_sync(d);
        for (t, v) in drain(sc) {
            assert!(
                t != WSCONS_EVENT_MOUSE_DELTA_X && t != WSCONS_EVENT_MOUSE_DELTA_Y,
                "two touches do not move the pointer"
            );
            if t == WSCONS_EVENT_VSCROLL {
                scrolls.push(v);
            }
        }
    }
    assert_eq!(scrolls.len(), 2, "{scrolls:?}");
    assert!(scrolls.iter().all(|&v| v < 0));
    with_tp(sc, |_, tp| {
        assert_eq!((tp.contacts, tp.t), (2, 0));
        assert_eq!(tp.tpad_touches[1].dir, 0, "north");
    });

    // Fingers moving apart (one up, one down) do not scroll.
    wsmouse_mtstate(d, 0, 1000, 620, 50);
    wsmouse_mtstate(d, 1, 1400, 540, 50);
    wsmouse_input_sync(d);
    assert!(
        !drain(sc).iter().any(|&(t, _)| t == WSCONS_EVENT_VSCROLL),
        "opposite directions"
    );
    finish(sc);
}

#[test]
fn directions_magnitudes_and_square_roots() {
    // Clockwise sectors from north.
    let r = 1 << 12;
    assert_eq!(direction(0, 0, r), -1);
    assert_eq!(direction(0, 10, r), 0);
    assert_eq!(direction(10, 10, r), 1);
    assert_eq!(direction(10, 0, r), 2);
    assert_eq!(direction(10, -10, r), 4);
    assert_eq!(direction(0, -10, r), 5);
    assert_eq!(direction(-10, -10, r), 7);
    assert_eq!(
        direction(-10, 0, r),
        9,
        "dy 0 counts as positive: west, lower half"
    );
    assert_eq!(direction(-10, 10, r), 10);
    assert_eq!(direction(-1, 10, r), 11);
    assert_eq!((dircmp(0, 11), dircmp(1, 7), dircmp(2, 3)), (1, 6, 1));

    let mut input = WsmouseInput::default();
    input.filter.h.mag_scale = 4096;
    input.filter.v.mag_scale = 4096;
    assert_eq!(magnitude(&input, 8, -3), (8 << 12) + 3 * (3 << 12) / 8);

    for n in [0u32, 1, 2, 3, 4, 15, 16, 17, 13_000_000, u32::MAX] {
        let r = isqrt(n);
        assert!(u64::from(r) * u64::from(r) <= u64::from(n), "{n}");
        assert!(
            (u64::from(r) + 1) * (u64::from(r) + 1) > u64::from(n),
            "{n}"
        );
    }
    assert_eq!(isqrt(13_000_000), 3605);
    assert_eq!(
        (btnmask(0), btnmask(1), btnmask(32), btnmask(33)),
        (0, 1, 1 << 31, 0)
    );
}

/// The constants against `wstpad.c`.
#[test]
#[ignore = "needs OPENBSD_SRC"]
fn constants_match_the_c() {
    let defs = crate::reftest::defines("sys/dev/wscons/wstpad.c");
    let _ = crate::reftest::assert_defines!(defs;
        V_EDGE_RATIO_DEFAULT, B_EDGE_RATIO_DEFAULT, T_EDGE_RATIO_DEFAULT, CENTER_RATIO_DEFAULT,
        TAP_MAXTIME_DEFAULT, TAP_CLICKTIME_DEFAULT, TAP_LOCKTIME_DEFAULT, TAP_BTNMAP_SIZE,
        CLICKDELAY_MS, FREEZE_MS, MATCHINTERVAL_MS, STOPINTERVAL_MS, MAG_LOW, MAG_MEDIUM,
        L_EDGE, R_EDGE, T_EDGE, B_EDGE, THUMB, EDGES, WSTPAD_SOFTBUTTONS, WSTPAD_SOFTMBTN,
        WSTPAD_TOPBUTTONS, WSTPAD_TWOFINGERSCROLL, WSTPAD_EDGESCROLL, WSTPAD_HORIZSCROLL,
        WSTPAD_SWAPSIDES, WSTPAD_DISABLE, WSTPAD_MTBUTTONS, WSTPAD_MT, TAN_DEG_60, TAN_DEG_30,
    );
    // The enumerations, in the order of the file.
    let text =
        std::fs::read_to_string(crate::reftest::openbsd_src().join("sys/dev/wscons/wstpad.c"))
            .unwrap();
    let names = |name: &str| -> Vec<std::string::String> {
        let start = text.find(name).unwrap();
        let body = &text[start..start + text[start..].find("};").unwrap()];
        body.lines()
            .skip(1)
            .map(|l| l.trim().trim_end_matches(','))
            .filter(|l| !l.is_empty())
            .map(std::string::String::from)
            .collect()
    };
    assert_eq!(
        names("enum tpad_handlers {"),
        [
            "SOFTBUTTON_HDLR",
            "TOPBUTTON_HDLR",
            "TAP_HDLR",
            "F2SCROLL_HDLR",
            "EDGESCROLL_HDLR",
            "CLICK_HDLR"
        ]
    );
    assert_eq!((SOFTBUTTON_HDLR, CLICK_HDLR), (0, 5));
    assert_eq!(
        names("enum tpad_cmd {"),
        [
            "CLEAR_MOTION_DELTAS",
            "SOFTBUTTON_DOWN",
            "SOFTBUTTON_UP",
            "TAPBUTTON_SYNC",
            "TAPBUTTON_DOWN",
            "TAPBUTTON_UP",
            "VSCROLL",
            "HSCROLL",
        ]
    );
    assert_eq!((CLEAR_MOTION_DELTAS, HSCROLL), (0, 7));
    assert_eq!(
        names("enum tap_state {"),
        [
            "TAP_DETECT",
            "TAP_IGNORE",
            "TAP_LIFTED",
            "TAP_LOCKED",
            "TAP_LOCKED_DRAG"
        ]
    );
    assert_eq!(
        names("enum touchstates {"),
        ["TOUCH_NONE", "TOUCH_BEGIN", "TOUCH_UPDATE", "TOUCH_END"]
    );
}
