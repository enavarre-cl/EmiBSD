/* <LICENSES> */
/*
 * Copyright (c) 2015, 2016 Ulf Brosziewski
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
//! Touchpad input processing: the compat mode of `wsmouse(4)` for touchpads and clickpads
//! (`WSMOUSEHW_TOUCHPAD`, `WSMOUSEHW_CLICKPAD`).
//!
//! Upstream: sys/dev/wscons/wstpad.c @ 3ce1f3f79392
//!
//! In compat mode `wsmouse_input_sync` hands each frame to [`wstpad_compat_convert`]:
//! [`wstpad_filter`] turns the absolute coordinates into filtered, decelerated and smoothed
//! deltas, and, for a configured touchpad, [`wstpad_process_input`] tracks the touches
//! (`TOUCH_BEGIN`, `TOUCH_UPDATE`, `TOUCH_END`; their direction, stability, edge areas and
//! thumbs) and runs the handlers the features enable: soft buttons at the bottom or top
//! edge (or by the number of contacts, `MTBUTTONS`), tapping ([`wstpad_tap`]: one-, two-
//! and three-finger taps, tap-and-drag and locked drags, finished by the timeout
//! [`wstpad_tap_timeout`]), two-finger and edge scrolling, and the freeze after a click.
//! Their results are "commands" that [`wstpad_cmds`] turns into the button, delta and
//! scroll state `wsmouse_input_sync` reports. [`wstpad_configure`] derives the defaults
//! from the hardware description, `WSMOUSEIO_SETPARAMS` changes them
//! ([`wstpad_set_param`]).
//!
//! ## Deviations
//! - `struct wstpad` is `malloc`ed as in C, but holds its touches: `tpad_touches` is an
//!   array of `WSMOUSE_MT_SLOTS_MAX` [`TpadTouch`]es, of which the first `max(num_slots, 1)`
//!   are used (the C `malloc`s exactly those), and `t`, the C's pointer into it, is an
//!   index. A touch's `pos`, a pointer to the motion position or to an MT slot's, is a
//!   [`TouchPos`]; [`pos`] reads it.
//! - The functions that reach `input->tp` take the input and the touchpad state as two
//!   references (the state is a separate allocation the input owns), and the ones that take
//!   a `struct tpad_touch *` of the array take its index or a copy. `wstpad_scroll_coords`
//!   returns the deltas as an `Option`, `wstpad_tap_touch` the touch's index.
//! - The tap timeout's argument is the mouse's device: [`wstpad_tap_timeout`] takes the
//!   input through the device's guard (`wsmouse.rs`) where the C gets the input pointer
//!   itself.
//! - `enum tpad_handlers` and `enum tpad_cmd` are bit numbers (`u32` constants), `enum
//!   tap_state` and `enum touchstates` Rust enums with the C's names; the boolean `int`s
//!   are `bool`s where only C's truth is used (`wstpad_is_stable`, `wstpad_is_tap`, ...).
//! - `wstpad_init`'s and `wstpad_configure`'s -1 are `Err(EINVAL)`, the `int` errors of
//!   `wstpad_set_param` and `wstpad_get_param` `Err`s; `wstpad_get_param` returns the
//!   value.

use core::ffi::c_void;
use core::ptr::{self, NonNull};

use crate::dev::wscons::wsconsio::{
    WSMOUSE_TYPE_SYNAP_SBTN, WSMOUSECFG_BOTTOM_EDGE, WSMOUSECFG_CENTERWIDTH, WSMOUSECFG_DISABLE,
    WSMOUSECFG_EDGESCROLL, WSMOUSECFG_F2PRESSURE, WSMOUSECFG_F2WIDTH, WSMOUSECFG_HORIZSCROLL,
    WSMOUSECFG_HORIZSCROLLDIST, WSMOUSECFG_LEFT_EDGE, WSMOUSECFG_MTBTN_MAXDIST,
    WSMOUSECFG_MTBUTTONS, WSMOUSECFG_RIGHT_EDGE, WSMOUSECFG_SOFTBUTTONS, WSMOUSECFG_SOFTMBTN,
    WSMOUSECFG_SWAPSIDES, WSMOUSECFG_TAP_CLICKTIME, WSMOUSECFG_TAP_LOCKTIME,
    WSMOUSECFG_TAP_MAXTIME, WSMOUSECFG_TAP_ONE_BTNMAP, WSMOUSECFG_TAP_THREE_BTNMAP,
    WSMOUSECFG_TAP_TWO_BTNMAP, WSMOUSECFG_TOP_EDGE, WSMOUSECFG_TOPBUTTONS,
    WSMOUSECFG_TWOFINGERSCROLL, WSMOUSECFG_VERTSCROLLDIST,
};
use crate::dev::wscons::wseventvar::wsevent_wakeup;
use crate::dev::wscons::wsmouse::{
    wsmouse_evq_put, wsmouse_hysteresis, wsmouse_input_of, wsmouse_log_events,
};
use crate::dev::wscons::wsmouseinput::*;
use crate::dev::wscons::wsmousevar::{WSMOUSE_MT_SLOTS_MAX, WSMOUSEHW_CLICKPAD};
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_tc::getnanotime;
use crate::kern::kern_timeout::{timeout_add_msec, timeout_del, timeout_set};
use crate::kern::subr_prf::printf;
use crate::machine::intr::{spltty, splx};
use crate::sys::device::Device;
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_DEVBUF, M_WAITOK, M_ZERO};
use crate::sys::time::{Timespec, timespecadd, timespecsub};
use crate::sys::timeout::Timeout;

/// `BTNMASK(n)`: the mask of button `n` (counted from 1), 0 outside 1 .. 32.
pub const fn btnmask(n: i32) -> u32 {
    if n > 0 && n <= 32 { 1 << (n - 1) } else { 0 }
}

/// `LEFTBTN`.
pub const LEFTBTN: u32 = btnmask(1);
/// `MIDDLEBTN`.
pub const MIDDLEBTN: u32 = btnmask(2);
/// `RIGHTBTN`.
pub const RIGHTBTN: u32 = btnmask(3);

/// `PRIMARYBTN`.
pub const PRIMARYBTN: u32 = LEFTBTN;

// Ratios to the height or width of the touchpad surface, in [*.12] fixed-point format:
/// `V_EDGE_RATIO_DEFAULT`.
pub const V_EDGE_RATIO_DEFAULT: i32 = 205;
/// `B_EDGE_RATIO_DEFAULT`.
pub const B_EDGE_RATIO_DEFAULT: i32 = 410;
/// `T_EDGE_RATIO_DEFAULT`.
pub const T_EDGE_RATIO_DEFAULT: i32 = 512;
/// `CENTER_RATIO_DEFAULT`.
pub const CENTER_RATIO_DEFAULT: i32 = 512;

/// `TAP_MAXTIME_DEFAULT`.
pub const TAP_MAXTIME_DEFAULT: i32 = 180;
/// `TAP_CLICKTIME_DEFAULT`.
pub const TAP_CLICKTIME_DEFAULT: i32 = 180;
/// `TAP_LOCKTIME_DEFAULT`.
pub const TAP_LOCKTIME_DEFAULT: i32 = 0;
/// `TAP_BTNMAP_SIZE`.
pub const TAP_BTNMAP_SIZE: usize = 3;

/// `CLICKDELAY_MS`.
pub const CLICKDELAY_MS: u64 = 20;
/// `FREEZE_MS`.
pub const FREEZE_MS: i64 = 100;
/// `MATCHINTERVAL_MS`.
pub const MATCHINTERVAL_MS: i64 = 45;
/// `STOPINTERVAL_MS`.
pub const STOPINTERVAL_MS: i64 = 55;

/// `MAG_LOW`.
pub const MAG_LOW: i32 = 10 << 12;
/// `MAG_MEDIUM`.
pub const MAG_MEDIUM: i32 = 18 << 12;

// `enum tpad_handlers`: bit numbers of `wstpad.handlers`.
/// `SOFTBUTTON_HDLR`.
pub const SOFTBUTTON_HDLR: i32 = 0;
/// `TOPBUTTON_HDLR`.
pub const TOPBUTTON_HDLR: i32 = 1;
/// `TAP_HDLR`.
pub const TAP_HDLR: i32 = 2;
/// `F2SCROLL_HDLR`.
pub const F2SCROLL_HDLR: i32 = 3;
/// `EDGESCROLL_HDLR`.
pub const EDGESCROLL_HDLR: i32 = 4;
/// `CLICK_HDLR`.
pub const CLICK_HDLR: i32 = 5;

// `enum tpad_cmd`: bit numbers of the commands of a frame.
/// `CLEAR_MOTION_DELTAS`.
pub const CLEAR_MOTION_DELTAS: i32 = 0;
/// `SOFTBUTTON_DOWN`.
pub const SOFTBUTTON_DOWN: i32 = 1;
/// `SOFTBUTTON_UP`.
pub const SOFTBUTTON_UP: i32 = 2;
/// `TAPBUTTON_SYNC`.
pub const TAPBUTTON_SYNC: i32 = 3;
/// `TAPBUTTON_DOWN`.
pub const TAPBUTTON_DOWN: i32 = 4;
/// `TAPBUTTON_UP`.
pub const TAPBUTTON_UP: i32 = 5;
/// `VSCROLL`.
pub const VSCROLL: i32 = 6;
/// `HSCROLL`.
pub const HSCROLL: i32 = 7;

// tpad_touch.flags:
/// `L_EDGE`.
pub const L_EDGE: u32 = 1 << 0;
/// `R_EDGE`.
pub const R_EDGE: u32 = 1 << 1;
/// `T_EDGE`.
pub const T_EDGE: u32 = 1 << 2;
/// `B_EDGE`.
pub const B_EDGE: u32 = 1 << 3;
/// `THUMB`.
pub const THUMB: u32 = 1 << 4;

/// `EDGES`.
pub const EDGES: u32 = L_EDGE | R_EDGE | T_EDGE | B_EDGE;

// wstpad.features
/// `WSTPAD_SOFTBUTTONS`.
pub const WSTPAD_SOFTBUTTONS: u32 = 1 << 0;
/// `WSTPAD_SOFTMBTN`.
pub const WSTPAD_SOFTMBTN: u32 = 1 << 1;
/// `WSTPAD_TOPBUTTONS`.
pub const WSTPAD_TOPBUTTONS: u32 = 1 << 2;
/// `WSTPAD_TWOFINGERSCROLL`.
pub const WSTPAD_TWOFINGERSCROLL: u32 = 1 << 3;
/// `WSTPAD_EDGESCROLL`.
pub const WSTPAD_EDGESCROLL: u32 = 1 << 4;
/// `WSTPAD_HORIZSCROLL`.
pub const WSTPAD_HORIZSCROLL: u32 = 1 << 5;
/// `WSTPAD_SWAPSIDES`.
pub const WSTPAD_SWAPSIDES: u32 = 1 << 6;
/// `WSTPAD_DISABLE`.
pub const WSTPAD_DISABLE: u32 = 1 << 7;
/// `WSTPAD_MTBUTTONS`.
pub const WSTPAD_MTBUTTONS: u32 = 1 << 8;

/// `WSTPAD_MT`.
pub const WSTPAD_MT: u32 = 1 << 31;

/// `enum tap_state`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(non_camel_case_types)] // OpenBSD names, verbatim
pub enum TapState {
    /// `TAP_DETECT`.
    TAP_DETECT,
    /// `TAP_IGNORE`.
    TAP_IGNORE,
    /// `TAP_LIFTED`.
    TAP_LIFTED,
    /// `TAP_LOCKED`.
    TAP_LOCKED,
    /// `TAP_LOCKED_DRAG`.
    TAP_LOCKED_DRAG,
}

pub use TapState::*;

/// `enum touchstates`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(non_camel_case_types)] // OpenBSD names, verbatim
pub enum Touchstates {
    /// `TOUCH_NONE`.
    TOUCH_NONE,
    /// `TOUCH_BEGIN`.
    TOUCH_BEGIN,
    /// `TOUCH_UPDATE`.
    TOUCH_UPDATE,
    /// `TOUCH_END`.
    TOUCH_END,
}

pub use Touchstates::*;

/// The position a touch follows (the C's `struct position *pos`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TouchPos {
    /// `&input->motion.pos`: the touch of a single-touch device.
    Motion,
    /// `&input->mt.slots[i].pos`.
    Slot(usize),
}

/// The `orig` member of `struct tpad_touch`: where and when the touch began.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TouchOrig {
    /// `x`.
    pub x: i32,
    /// `y`.
    pub y: i32,
    /// `time`.
    pub time: Timespec,
}

/// `struct tpad_touch`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TpadTouch {
    /// `flags`: `L_EDGE`, ..., `THUMB`.
    pub flags: u32,
    /// `state`.
    pub state: Touchstates,
    /// `x`: normalized coordinates.
    pub x: i32,
    /// `y`.
    pub y: i32,
    /// `dir`: the direction sector 0 - 11, -1 for none.
    pub dir: i32,
    /// `start`.
    pub start: Timespec,
    /// `match`.
    pub match_: Timespec,
    /// `pos`.
    pub pos: TouchPos,
    /// `orig`.
    pub orig: TouchOrig,
}

impl TpadTouch {
    /// A touch that never began (the C's zeroed one, its `pos` the motion position).
    pub const fn new() -> Self {
        Self {
            flags: 0,
            state: TOUCH_NONE,
            x: 0,
            y: 0,
            dir: 0,
            start: Timespec::new(0, 0),
            match_: Timespec::new(0, 0),
            pos: TouchPos::Motion,
            orig: TouchOrig {
                x: 0,
                y: 0,
                time: Timespec::new(0, 0),
            },
        }
    }
}

/// The `edge` member of `struct wstpad`: edge coordinates.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TpadEdge {
    /// `left`.
    pub left: i32,
    /// `right`.
    pub right: i32,
    /// `top`.
    pub top: i32,
    /// `bottom`.
    pub bottom: i32,
    /// `center`.
    pub center: i32,
    /// `center_left`.
    pub center_left: i32,
    /// `center_right`.
    pub center_right: i32,
    /// `low`.
    pub low: i32,
}

/// The `params` member of `struct wstpad`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TpadParams {
    /// `left_edge`: ratios to the surface width or height.
    pub left_edge: i32,
    /// `right_edge`.
    pub right_edge: i32,
    /// `top_edge`.
    pub top_edge: i32,
    /// `bottom_edge`.
    pub bottom_edge: i32,
    /// `center_width`.
    pub center_width: i32,
    /// `f2pressure`: two-finger contacts.
    pub f2pressure: i32,
    /// `f2width`.
    pub f2width: i32,
    /// `mtbtn_maxdist`: MTBUTTONS: distance limit for two-finger clicks.
    pub mtbtn_maxdist: i32,
}

/// The `tap` member of `struct wstpad`: handler state and configuration.
pub struct TpadTap {
    /// `state`.
    pub state: TapState,
    /// `contacts`.
    pub contacts: i32,
    /// `valid`.
    pub valid: bool,
    /// `pending`.
    pub pending: u32,
    /// `button`.
    pub button: u32,
    /// `masked`.
    pub masked: i32,
    /// `maxdist`.
    pub maxdist: i32,
    /// `to`.
    pub to: Timeout,
    /// `maxtime`: parameters:
    pub maxtime: Timespec,
    /// `clicktime`.
    pub clicktime: i32,
    /// `locktime`.
    pub locktime: i32,
    /// `btnmap`.
    pub btnmap: [u32; TAP_BTNMAP_SIZE],
}

/// The `scroll` member of `struct wstpad`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TpadScroll {
    /// `dz`.
    pub dz: i32,
    /// `dw`.
    pub dw: i32,
    /// `hdist`.
    pub hdist: i32,
    /// `vdist`.
    pub vdist: i32,
    /// `mag`.
    pub mag: i32,
}

/// `struct wstpad`: the touchpad state of an input (`input->tp`).
pub struct Wstpad {
    /// `features`: `WSTPAD_*`.
    pub features: u32,
    /// `handlers`: `1 << *_HDLR`.
    pub handlers: u32,
    /// `t`: the index in `tpad_touches` of the pointer-controlling touch. If there is more
    /// than one touch, t selects the pointer-controlling touch.
    pub t: usize,
    /// `tpad_touches`: the first `max(num_slots, 1)` are used.
    pub tpad_touches: [TpadTouch; WSMOUSE_MT_SLOTS_MAX as usize],
    /// `mtcycle`.
    pub mtcycle: u32,
    /// `ignore`.
    pub ignore: u32,
    /// `contacts`.
    pub contacts: i32,
    /// `prev_contacts`.
    pub prev_contacts: i32,
    /// `btns`.
    pub btns: u32,
    /// `btns_sync`.
    pub btns_sync: u32,
    /// `ratio`.
    pub ratio: i32,
    /// `time`.
    pub time: Timespec,
    /// `freeze`.
    pub freeze: u32,
    /// `freeze_ts`.
    pub freeze_ts: Timespec,
    /// `edge`: edge coordinates.
    pub edge: TpadEdge,
    /// `params`.
    pub params: TpadParams,
    /// `softbutton`: handler state and configuration:
    pub softbutton: u32,
    /// `sbtnswap`.
    pub sbtnswap: u32,
    /// `tap`.
    pub tap: TpadTap,
    /// `scroll`.
    pub scroll: TpadScroll,
}

impl Wstpad {
    /// The state `wstpad_init` starts from (the C's `M_ZERO`).
    pub const fn new() -> Self {
        Self {
            features: 0,
            handlers: 0,
            t: 0,
            tpad_touches: [TpadTouch::new(); WSMOUSE_MT_SLOTS_MAX as usize],
            mtcycle: 0,
            ignore: 0,
            contacts: 0,
            prev_contacts: 0,
            btns: 0,
            btns_sync: 0,
            ratio: 0,
            time: Timespec::new(0, 0),
            freeze: 0,
            freeze_ts: Timespec::new(0, 0),
            edge: TpadEdge {
                left: 0,
                right: 0,
                top: 0,
                bottom: 0,
                center: 0,
                center_left: 0,
                center_right: 0,
                low: 0,
            },
            params: TpadParams {
                left_edge: 0,
                right_edge: 0,
                top_edge: 0,
                bottom_edge: 0,
                center_width: 0,
                f2pressure: 0,
                f2width: 0,
                mtbtn_maxdist: 0,
            },
            softbutton: 0,
            sbtnswap: 0,
            tap: TpadTap {
                state: TAP_DETECT,
                contacts: 0,
                valid: false,
                pending: 0,
                button: 0,
                masked: 0,
                maxdist: 0,
                to: Timeout::zeroed(),
                maxtime: Timespec::new(0, 0),
                clicktime: 0,
                locktime: 0,
                btnmap: [0; TAP_BTNMAP_SIZE],
            },
            scroll: TpadScroll {
                dz: 0,
                dw: 0,
                hdist: 0,
                vdist: 0,
                mag: 0,
            },
        }
    }

    /// `tp->t`.
    fn t(&self) -> &TpadTouch {
        &self.tpad_touches[self.t]
    }
}

impl Default for TpadTouch {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for Wstpad {
    fn default() -> Self {
        Self::new()
    }
}

/// `PRIMARYBTN_CLICKED(tp)`.
fn primarybtn_clicked(tp: &Wstpad) -> bool {
    tp.btns_sync & PRIMARYBTN & tp.btns != 0
}

/// `PRIMARYBTN_RELEASED(tp)`.
fn primarybtn_released(tp: &Wstpad) -> bool {
    tp.btns_sync & PRIMARYBTN & !tp.btns != 0
}

/// `IS_MT(tp)`.
fn is_mt(tp: &Wstpad) -> bool {
    tp.features & WSTPAD_MT != 0
}

/// `DISABLE(tp)`.
fn disable(tp: &Wstpad) -> bool {
    tp.features & WSTPAD_DISABLE != 0
}

/// `CENTERED(t)`: a touch is "centered" if it does not start and remain at the top edge or
/// one of the vertical edges. Two-finger scrolling and tapping require that at least one
/// touch is centered.
fn centered(t: &TpadTouch) -> bool {
    t.flags & (L_EDGE | R_EDGE | T_EDGE) == 0
}

/// `match_interval`.
const MATCH_INTERVAL: Timespec = Timespec::new(0, MATCHINTERVAL_MS * 1_000_000);

/// `stop_interval`.
const STOP_INTERVAL: Timespec = Timespec::new(0, STOPINTERVAL_MS * 1_000_000);

/// `input->tp`.
///
/// # Safety
///
/// No other reference to the touchpad state is live: the caller has the input exclusively
/// (a `&mut`, under its guard) and does not call this again while the result is used.
unsafe fn tp_of<'a>(input: &WsmouseInput) -> Option<&'a mut Wstpad> {
    // SAFETY: `wstpad_init` allocated and initialised the state, which the input owns
    // until `wstpad_cleanup`; the caller's contract makes this the only reference.
    input.tp.map(|p| unsafe { &mut *p.as_ptr() })
}

/// `t->pos`: the position a touch follows.
pub fn pos<'a>(input: &'a WsmouseInput, t: &TpadTouch) -> &'a Position {
    match t.pos {
        TouchPos::Motion => &input.motion.pos,
        TouchPos::Slot(i) => &input.mt.slots()[i].pos,
    }
}

/// `normalize_abs`: coordinates in the wstpad struct are "normalized" device coordinates,
/// the orientation is left-to-right and upward.
fn normalize_abs(filter: &AxisFilter, val: i32) -> i32 {
    if filter.inv != 0 {
        filter.inv - val
    } else {
        val
    }
}

/// `normalize_rel`.
fn normalize_rel(filter: &AxisFilter, val: i32) -> i32 {
    if filter.inv != 0 { -val } else { val }
}

// Directions of motion are represented by numbers in the range 0 - 11, corresponding to
// clockwise counted circle sectors:
//
//              11 | 0
//           10    |    1
//          9      |      2
//          -------+-------
//          8      |      3
//            7    |    4
//               6 | 5
//
// Tangent constants in [*.12] fixed-point format:
/// `TAN_DEG_60`.
pub const TAN_DEG_60: i32 = 7094;
/// `TAN_DEG_30`.
pub const TAN_DEG_30: i32 = 2365;

/// `NORTH(d)`.
fn north(d: i32) -> bool {
    d == 0 || d == 11
}

/// `SOUTH(d)`.
fn south(d: i32) -> bool {
    d == 5 || d == 6
}

/// `EAST(d)`.
fn east(d: i32) -> bool {
    d == 2 || d == 3
}

/// `WEST(d)`.
fn west(d: i32) -> bool {
    d == 8 || d == 9
}

/// `direction`: the sector of the delta pair, -1 for none.
pub fn direction(dx: i32, dy: i32, ratio: i32) -> i32 {
    let mut dir = -1;

    if dx != 0 || dy != 0 {
        let rdy = dy.wrapping_abs().wrapping_mul(ratio);
        dir = if dx.wrapping_abs().wrapping_mul(TAN_DEG_60) < rdy {
            0
        } else if dx.wrapping_abs().wrapping_mul(TAN_DEG_30) < rdy {
            1
        } else {
            2
        };
        if (dx < 0) != (dy < 0) {
            dir = 5 - dir;
        }
        if dx < 0 {
            dir += 6;
        }
    }
    dir
}

/// `dircmp`.
pub fn dircmp(dir1: i32, dir2: i32) -> i32 {
    let diff = (dir1 - dir2).abs();
    if diff <= 6 { diff } else { 12 - diff }
}

/// `wstpad_set_direction`: update direction and timespec attributes for a touch. They are
/// used to determine whether it is moving - or resting - stably.
///
/// The callers pass touches from the current frame and the touches that are no longer
/// present in the update cycle to this function. Even though this ensures that pairs of
/// zero deltas do not result from stale coordinates, zero deltas do not reset the state
/// immediately. A short time span - the "stop interval" - must pass before the state is
/// cleared, which is necessary because some touchpads report intermediate stops when a
/// touch is moving very slowly.
pub fn wstpad_set_direction(tp: &mut Wstpad, slot: usize, dx: i32, dy: i32) {
    let (time, ratio) = (tp.time, tp.ratio);
    let t = &mut tp.tpad_touches[slot];

    if t.state != TOUCH_UPDATE {
        t.dir = -1;
        t.start = time;
        return;
    }

    let dir = direction(dx, dy, ratio);
    if dir >= 0 {
        if t.dir < 0 || dircmp(dir, t.dir) > 1 {
            t.start = time;
        }
        t.dir = dir;
        t.match_ = time;
    } else if t.dir >= 0 {
        let ts = timespecsub(&time, &t.match_);
        if ts >= STOP_INTERVAL {
            t.dir = -1;
            t.start = t.match_;
        }
    }
}

/// `magnitude`: make a rough, but quick estimation of the speed of a touch. Its distance to
/// the previous position is scaled by factors derived from the average update rate and the
/// deceleration parameter (`filter.dclr`). The unit of the result is: (`filter.dclr` / 100)
/// device units per millisecond.
///
/// Magnitudes are returned in [*.12] fixed-point format. For purposes of filtering, they
/// are divided into medium and high speeds (> `MAG_MEDIUM`), low speeds, and very low
/// speeds (< `MAG_LOW`).
///
/// The scale factors are not affected if deceleration is turned off.
pub fn magnitude(input: &WsmouseInput, dx: i32, dy: i32) -> i32 {
    let h = dx.wrapping_abs().wrapping_mul(input.filter.h.mag_scale);
    let v = dy.wrapping_abs().wrapping_mul(input.filter.v.mag_scale);
    // Return an "alpha-max-plus-beta-min" approximation:
    if h >= v { h + 3 * v / 8 } else { v + 3 * h / 8 }
}

/// `wstpad_is_stable`: treat a touch as stable if it is moving at a medium or high speed,
/// if it is moving continuously, or if it has stopped for a certain time span.
pub fn wstpad_is_stable(input: &WsmouseInput, tp: &Wstpad, t: &TpadTouch) -> bool {
    let ts = if t.dir >= 0 {
        let p = pos(input, t);
        if magnitude(input, p.dx, p.dy) > MAG_MEDIUM {
            return true;
        }
        timespecsub(&t.match_, &t.start)
    } else {
        timespecsub(&tp.time, &t.start)
    };

    ts >= MATCH_INTERVAL
}

/// `edge_flags`: if a touch starts in an edge area, pointer movement will be suppressed as
/// long as it stays in that area.
fn edge_flags(tp: &Wstpad, x: i32, y: i32) -> u32 {
    let mut flags = 0;

    if x < tp.edge.left {
        flags |= L_EDGE;
    } else if x >= tp.edge.right {
        flags |= R_EDGE;
    }
    if y < tp.edge.bottom {
        flags |= B_EDGE;
    } else if y >= tp.edge.top {
        flags |= T_EDGE;
    }

    flags
}

/// `get_2nd_touch`: the index of a touch that is neither the pointer-controlling one nor
/// ignored.
fn get_2nd_touch(input: &WsmouseInput, tp: &Wstpad) -> Option<usize> {
    if is_mt(tp) {
        let slot = ffs(input.mt.touches & !(input.mt.ptr | tp.ignore));
        if slot != 0 {
            return Some(slot as usize - 1);
        }
    }
    None
}

/// `set_freeze_ts`: suppress pointer motion for a short period of time.
fn set_freeze_ts(tp: &mut Wstpad, sec: i64, ms: i64) {
    tp.freeze_ts = Timespec::new(sec, ms * 1_000_000);
    tp.freeze_ts = timespecadd(&tp.time, &tp.freeze_ts);
}

/// `wstpad_scroll_coords`: the normalized deltas if two-finger- or edge-scrolling would be
/// valid (the C's TRUE).
pub fn wstpad_scroll_coords(input: &WsmouseInput, tp: &mut Wstpad) -> Option<(i32, i32)> {
    if tp.contacts != tp.prev_contacts || tp.btns != 0 || tp.btns_sync != 0 {
        tp.scroll.dz = 0;
        tp.scroll.dw = 0;
        return None;
    }
    if input.motion.sync & SYNC_POSITION == 0 {
        return None;
    }
    // Try to exclude accidental scroll events by checking whether the pointer-controlling
    // touch is stable. The check, which may cause a short delay, is only applied
    // initially, a touch that stops and resumes scrolling is not affected.
    if tp.scroll.dz != 0 || tp.scroll.dw != 0 || wstpad_is_stable(input, tp, tp.t()) {
        let dx = normalize_rel(&input.filter.h, input.motion.pos.dx);
        let dy = normalize_rel(&input.filter.v, input.motion.pos.dy);
        return if dx != 0 || dy != 0 {
            Some((dx, dy))
        } else {
            None
        };
    }

    None
}

/// `wstpad_scroll`.
pub fn wstpad_scroll(tp: &mut Wstpad, dx: i32, dy: i32, mag: i32, cmds: &mut u32) {
    let mut n = 1;

    // The function applies strong deceleration, but only to input with very low speeds. A
    // higher threshold might make applications without support for precision scrolling
    // appear unresponsive.
    let mag = MAG_MEDIUM.min((mag + 3 * tp.scroll.mag) / 4);
    tp.scroll.mag = mag;
    if mag < MAG_LOW {
        n = (MAG_LOW - mag) / 4096 + 1;
    }

    if dy != 0 && tp.scroll.vdist != 0 {
        if tp.scroll.dw != 0 {
            // Before switching the axis, wstpad_scroll_coords() should check again whether
            // the movement is stable.
            tp.scroll.dw = 0;
            return;
        }
        let mut dz = (-dy).wrapping_mul(4096) / (tp.scroll.vdist * n);
        if tp.scroll.dz != 0 {
            if (dy < 0) != (tp.scroll.dz > 0) {
                tp.scroll.dz = -tp.scroll.dz;
            }
            dz = (dz + 3 * tp.scroll.dz) / 4;
        }
        if dz != 0 {
            tp.scroll.dz = dz;
            *cmds |= 1 << VSCROLL;
        }
    } else if dx != 0 && tp.scroll.hdist != 0 {
        if tp.scroll.dz != 0 {
            tp.scroll.dz = 0;
            return;
        }
        let mut dw = dx.wrapping_mul(4096) / (tp.scroll.hdist * n);
        if tp.scroll.dw != 0 {
            if (dx > 0) != (tp.scroll.dw > 0) {
                tp.scroll.dw = -tp.scroll.dw;
            }
            dw = (dw + 3 * tp.scroll.dw) / 4;
        }
        if dw != 0 {
            tp.scroll.dw = dw;
            *cmds |= 1 << HSCROLL;
        }
    }
}

/// `wstpad_f2scroll`: two-finger scrolling.
pub fn wstpad_f2scroll(input: &WsmouseInput, tp: &mut Wstpad, cmds: &mut u32) {
    if tp.ignore == 0 {
        if tp.contacts != 2 {
            return;
        }
    } else if tp.contacts != 3 || tp.ignore == input.mt.ptr {
        return;
    }

    let Some((mut dx, mut dy)) = wstpad_scroll_coords(input, tp) else {
        return;
    };

    let dir = tp.t().dir;
    if !(north(dir) || south(dir)) {
        dy = 0;
    }
    if !(east(dir) || west(dir)) {
        dx = 0;
    }

    if dx != 0 || dy != 0 {
        let mut is_centered = centered(tp.t());
        if is_mt(tp) {
            let Some(t2) = get_2nd_touch(input, tp) else {
                return;
            };
            let t2 = &tp.tpad_touches[t2];
            let dir = t2.dir;
            if (dy > 0 && !north(dir)) || (dy < 0 && !south(dir)) {
                return;
            }
            if (dx > 0 && !east(dir)) || (dx < 0 && !west(dir)) {
                return;
            }
            if !wstpad_is_stable(input, tp, t2) && !(tp.scroll.dz != 0 || tp.scroll.dw != 0) {
                return;
            }
            is_centered |= centered(t2);
        }
        if is_centered {
            wstpad_scroll(tp, dx, dy, magnitude(input, dx, dy), cmds);
            set_freeze_ts(tp, 0, FREEZE_MS);
        }
    }
}

/// `wstpad_edgescroll`: scrolling at the right (or left) and bottom edges.
pub fn wstpad_edgescroll(input: &WsmouseInput, tp: &mut Wstpad, cmds: &mut u32) {
    let Some((mut dx, mut dy)) = wstpad_scroll_coords(input, tp) else {
        return;
    };
    if tp.contacts != 1 {
        return;
    }
    let t = tp.t();

    let v_edge = if tp.features & WSTPAD_SWAPSIDES != 0 {
        L_EDGE
    } else {
        R_EDGE
    };
    let b_edge = if tp.features & WSTPAD_HORIZSCROLL != 0 {
        B_EDGE
    } else {
        0
    };

    if t.flags & v_edge == 0 {
        dy = 0;
    }
    if t.flags & b_edge == 0 {
        dx = 0;
    }

    if dx != 0 || dy != 0 {
        wstpad_scroll(tp, dx, dy, magnitude(input, dx, dy), cmds);
    }
}

/// `sbtn`: the soft button of a position in the bottom area.
fn sbtn(tp: &Wstpad, x: i32, y: i32) -> u32 {
    if y >= tp.edge.bottom {
        return 0;
    }
    if tp.features & WSTPAD_SOFTMBTN != 0 && x >= tp.edge.center_left && x < tp.edge.center_right {
        return MIDDLEBTN;
    }
    (if x < tp.edge.center {
        LEFTBTN
    } else {
        RIGHTBTN
    }) ^ tp.sbtnswap
}

/// `top_sbtn`: the soft button of a position in the top area.
fn top_sbtn(tp: &Wstpad, x: i32, y: i32) -> u32 {
    if y < tp.edge.top {
        return 0;
    }
    if x < tp.edge.center_left {
        return LEFTBTN ^ tp.sbtnswap;
    }
    if x > tp.edge.center_right {
        RIGHTBTN ^ tp.sbtnswap
    } else {
        MIDDLEBTN
    }
}

/// `wstpad_get_sbtn`.
pub fn wstpad_get_sbtn(input: &WsmouseInput, tp: &Wstpad, top: bool) -> u32 {
    let t = tp.t();
    let mut btn = 0;

    if tp.contacts != 0 {
        btn = if top {
            top_sbtn(tp, t.x, t.y)
        } else {
            sbtn(tp, t.x, t.y)
        };
        // If there is no middle-button area, but contacts in both halves of the edge
        // zone, generate a middle-button event:
        if btn != 0 && is_mt(tp) && tp.contacts == 2 && !top && tp.features & WSTPAD_SOFTMBTN == 0 {
            if let Some(t2) = get_2nd_touch(input, tp) {
                let t2 = &tp.tpad_touches[t2];
                btn |= sbtn(tp, t2.x, t2.y);
            }
            if btn == (LEFTBTN | RIGHTBTN) {
                btn = MIDDLEBTN;
            }
        }
    }
    if btn != PRIMARYBTN { btn } else { 0 }
}

/// `wstpad_mtbtn_contacts`.
pub fn wstpad_mtbtn_contacts(input: &WsmouseInput, tp: &Wstpad) -> i32 {
    if tp.ignore != 0 {
        return tp.contacts - 1;
    }

    if tp.contacts == 2
        && let Some(t2) = get_2nd_touch(input, tp)
    {
        let (t, t1) = (&tp.tpad_touches[t2], tp.t());
        let dx = (t.x - t1.x).wrapping_abs() << 12;
        let dy = (t.y - t1.y).wrapping_abs().wrapping_mul(tp.ratio);
        let dist = if dx >= dy {
            dx + 3 * dy / 8
        } else {
            dy + 3 * dx / 8
        };
        let mut limit = tp.params.mtbtn_maxdist << 12;
        if input.mt.ptr_mask != 0 {
            limit = limit * 2 / 3;
        }
        if dist > limit {
            return 1;
        }
    }
    tp.contacts
}

/// `wstpad_get_mtbtn`.
pub fn wstpad_get_mtbtn(input: &WsmouseInput, tp: &Wstpad) -> u32 {
    match wstpad_mtbtn_contacts(input, tp) {
        2 => RIGHTBTN,
        3 => MIDDLEBTN,
        _ => 0,
    }
}

/// `wstpad_softbuttons`.
pub fn wstpad_softbuttons(input: &WsmouseInput, tp: &mut Wstpad, cmds: &mut u32, hdlr: i32) {
    let top = hdlr == TOPBUTTON_HDLR;

    if tp.softbutton != 0 && primarybtn_released(tp) {
        *cmds |= 1 << SOFTBUTTON_UP;
        return;
    }

    if tp.softbutton == 0 && primarybtn_clicked(tp) {
        tp.softbutton = if tp.features & WSTPAD_MTBUTTONS != 0 {
            wstpad_get_mtbtn(input, tp)
        } else {
            wstpad_get_sbtn(input, tp, top)
        };
        if tp.softbutton != 0 {
            *cmds |= 1 << SOFTBUTTON_DOWN;
        }
    }
}

/// `wstpad_is_tap`: check whether the duration of t is within the tap limit.
pub fn wstpad_is_tap(tp: &Wstpad, t: &TpadTouch) -> bool {
    let ts = timespecsub(&tp.time, &t.orig.time);
    ts < tp.tap.maxtime
}

/// `wstpad_tap_filter`: at least one MT touch must remain close to its origin and end in
/// the main area. The same conditions apply to one-finger taps on single-touch devices.
pub fn wstpad_tap_filter(tp: &mut Wstpad, t: &TpadTouch) {
    let mut dist = 0;

    if is_mt(tp) || tp.tap.contacts == 1 {
        let dx = (t.x - t.orig.x).wrapping_abs() << 12;
        let dy = (t.y - t.orig.y).wrapping_abs().wrapping_mul(tp.ratio);
        dist = if dx >= dy {
            dx + 3 * dy / 8
        } else {
            dy + 3 * dx / 8
        };
    }
    tp.tap.valid = centered(t) && dist <= (tp.tap.maxdist << 12);
}

/// `wstpad_tap_touch`: the index of the oldest touch in the `TOUCH_END` state, or `None`.
pub fn wstpad_tap_touch(input: &WsmouseInput, tp: &mut Wstpad) -> Option<usize> {
    let mut t: Option<usize> = None;

    if is_mt(tp) {
        let lifted = input.mt.sync[MTS_TOUCH] & !input.mt.touches;
        for slot in foreachbit(lifted) {
            let s = tp.tpad_touches[slot as usize];
            if tp.tap.state == TAP_DETECT && !tp.tap.valid {
                wstpad_tap_filter(tp, &s);
            }
            if t.is_none_or(|t| tp.tpad_touches[t].orig.time > s.orig.time) {
                t = Some(slot as usize);
            }
        }
    } else if tp.t().state == TOUCH_END {
        t = Some(tp.t);
        if tp.tap.state == TAP_DETECT && !tp.tap.valid {
            let touch = *tp.t();
            wstpad_tap_filter(tp, &touch);
        }
    }

    t
}

/// `wstpad_tap_button`: determine the "tap button", keep track of whether a touch is
/// masked.
pub fn wstpad_tap_button(tp: &mut Wstpad) -> u32 {
    let n = tp.tap.contacts - tp.contacts - 1;

    tp.tap.masked = tp.contacts;

    if n >= 0 && (n as usize) < TAP_BTNMAP_SIZE {
        tp.tap.btnmap[n as usize]
    } else {
        0
    }
}

/// `tap_unmask`: in the hold/drag state, do not mask touches if no masking was involved in
/// the preceding tap gesture.
fn tap_unmask(tp: &Wstpad) -> bool {
    (tp.tap.button != 0 || tp.tap.pending != 0) && tp.tap.masked == 0
}

/// `wstpad_tap`: the tap handler.
///
/// In the default configuration, this handler maps one-, two-, and three-finger taps to
/// left-button, right-button, and middle-button events, respectively. Setting the LOCKTIME
/// parameter enables "locked drags", which are finished by a timeout or a tap-to-end
/// gesture.
pub fn wstpad_tap(input: &mut WsmouseInput, tp: &mut Wstpad, cmds: &mut u32) {
    let mut err = false;

    // Synchronize the button states, if necessary.
    if input.btn.sync != 0 {
        *cmds |= 1 << TAPBUTTON_SYNC;
    }

    // It is possible to produce a click within the tap timeout. Wait for a new touch
    // before generating new button events.
    if primarybtn_released(tp) {
        tp.tap.contacts = 0;
    }

    // Reset the detection state whenever a new touch starts.
    if tp.contacts > tp.prev_contacts
        || (is_mt(tp) && input.mt.touches & input.mt.sync[MTS_TOUCH] != 0)
    {
        tp.tap.contacts = tp.contacts;
        tp.tap.valid = false;
    }

    // The filtered number of active touches excludes a masked touch if its duration
    // exceeds the tap limit.
    let mut contacts = tp.contacts;
    let slot = ffs(input.mt.ptr_mask) - 1;
    if slot >= 0 && !wstpad_is_tap(tp, &tp.tpad_touches[slot as usize]) && !tap_unmask(tp) {
        contacts -= 1;
    }

    match tp.tap.state {
        TAP_DETECT => {
            // Find the oldest touch in the TOUCH_END state.
            if let Some(t) = wstpad_tap_touch(input, tp) {
                let is_tap = wstpad_is_tap(tp, &tp.tpad_touches[t]);
                if is_tap && contacts == 0 {
                    if tp.tap.button != 0 {
                        *cmds |= 1 << TAPBUTTON_UP;
                    }
                    tp.tap.pending = if tp.tap.valid {
                        wstpad_tap_button(tp)
                    } else {
                        0
                    };
                    if tp.tap.pending != 0 {
                        tp.tap.state = TAP_LIFTED;
                        err = !timeout_add_msec(&tp.tap.to, CLICKDELAY_MS);
                    }
                } else if !is_tap && tp.tap.locktime == 0 {
                    if contacts == 0 && tp.tap.button != 0 {
                        *cmds |= 1 << TAPBUTTON_UP;
                    } else if contacts != 0 {
                        tp.tap.state = TAP_IGNORE;
                    }
                } else if !is_tap && tp.tap.button != 0 {
                    if contacts == 0 {
                        tp.tap.state = TAP_LOCKED;
                        err = !timeout_add_msec(&tp.tap.to, tp.tap.locktime as u64);
                    } else {
                        tp.tap.state = TAP_LOCKED_DRAG;
                    }
                }
            }
        }
        TAP_IGNORE => {
            if contacts == 0 {
                tp.tap.state = TAP_DETECT;
                if tp.tap.button != 0 {
                    *cmds |= 1 << TAPBUTTON_UP;
                }
            }
        }
        TAP_LIFTED => {
            if contacts != 0 {
                timeout_del(&tp.tap.to);
                tp.tap.state = TAP_DETECT;
                if tp.tap.pending != 0 {
                    *cmds |= 1 << TAPBUTTON_DOWN;
                }
            }
        }
        TAP_LOCKED => {
            if contacts != 0 {
                timeout_del(&tp.tap.to);
                tp.tap.state = TAP_LOCKED_DRAG;
            }
        }
        TAP_LOCKED_DRAG => {
            if contacts == 0 {
                let t = wstpad_tap_touch(input, tp);
                if t.is_some_and(|t| wstpad_is_tap(tp, &tp.tpad_touches[t])) {
                    // "tap-to-end"
                    *cmds |= 1 << TAPBUTTON_UP;
                    tp.tap.state = TAP_DETECT;
                } else {
                    tp.tap.state = TAP_LOCKED;
                    err = !timeout_add_msec(&tp.tap.to, tp.tap.locktime as u64);
                }
            }
        }
    }

    if err {
        // Did timeout_add fail?
        input.sbtn.buttons &= !tp.tap.button;
        input.sbtn.sync |= tp.tap.button;
        tp.tap.pending = 0;
        tp.tap.button = 0;
        tp.tap.state = TAP_DETECT;
    }
}

/// `wstpad_tap_sync`.
pub fn wstpad_tap_sync(input: &WsmouseInput, tp: &Wstpad) -> bool {
    (tp.tap.button & (input.btn.buttons | tp.softbutton)) == 0
        || (tp.tap.button == PRIMARYBTN && tp.softbutton != 0)
}

/// `wstpad_tap_timeout`: the end of a tap's click, or of a locked drag. `p` is the
/// mouse's device.
pub fn wstpad_tap_timeout(p: *mut c_void) {
    let Some(dv) = NonNull::new(p.cast::<Device>()) else {
        return;
    };
    // SAFETY: `wstpad_init` set the timeout's argument to the mouse's device, which lives
    // until its detach, whose `wstpad_cleanup` deletes the timeout first.
    let dv = unsafe { dv.as_ref() };
    let s = spltty();
    let mut guard = wsmouse_input_of(dv);
    let input: &mut WsmouseInput = &mut guard;
    // SAFETY: the guard gives this function the input exclusively; nothing else takes
    // `input->tp` while `tp` is used.
    let tp = unsafe { tp_of(input) };
    if let (Some(evar), Some(tp)) = (input.evar(), tp) {
        // SAFETY: the queue stays allocated while it is the mouse's `me_evp`, which the
        // close that would free it clears under the kernel lock, not inside this callback.
        let evar = unsafe { &*ptr::from_ref(evar) };
        let mut ev = 0;
        let mut btn = 0;
        if tp.tap.pending != 0 {
            tp.tap.button = tp.tap.pending;
            tp.tap.pending = 0;
            input.sbtn.buttons |= tp.tap.button;
            timeout_add_msec(&tp.tap.to, tp.tap.clicktime as u64);
            if wstpad_tap_sync(input, tp) {
                ev = BTN_DOWN_EV;
                btn = ffs(tp.tap.button) - 1;
            }
        } else {
            if wstpad_tap_sync(input, tp) {
                ev = BTN_UP_EV;
                btn = ffs(tp.tap.button) - 1;
            }
            if tp.tap.button != tp.softbutton {
                input.sbtn.buttons &= !tp.tap.button;
            }
            tp.tap.button = 0;
            tp.tap.state = TAP_DETECT;
        }
        if ev != 0 {
            mtx_enter(&evar.ws_mtx);
            let put = evar.ws_put.get();
            mtx_leave(&evar.ws_mtx);
            let mut evq = EvqAccess {
                evar,
                ts: getnanotime(),
                put,
                result: EVQ_RESULT_NONE,
            };
            wsmouse_evq_put(&mut evq, ev, btn);
            wsmouse_evq_put(&mut evq, SYNC_EV, 0);
            if evq.result == EVQ_RESULT_SUCCESS {
                if input.flags & LOG_EVENTS != 0 {
                    wsmouse_log_events(input, &evq);
                }
                mtx_enter(&evar.ws_mtx);
                evar.ws_put.set(evq.put);
                mtx_leave(&evar.ws_mtx);
                wsevent_wakeup(evar);
            } else {
                input.sbtn.sync |= tp.tap.button;
            }
        }
    }
    drop(guard);
    splx(s);
}

/// `wstpad_click`: suppress accidental pointer movements after a click on a clickpad.
pub fn wstpad_click(tp: &mut Wstpad) {
    if tp.contacts == 1 && (primarybtn_clicked(tp) || primarybtn_released(tp)) {
        set_freeze_ts(tp, 0, FREEZE_MS);
    }
}

/// `wstpad_cmds`: translate the "command" bits into the sync-state of wsmouse.
pub fn wstpad_cmds(input: &mut WsmouseInput, tp: &mut Wstpad, cmds: u32) {
    for n in foreachbit(cmds) {
        match n {
            CLEAR_MOTION_DELTAS => {
                input.motion.dx = 0;
                input.motion.dy = 0;
                if input.motion.dz == 0 && input.motion.dw == 0 {
                    input.motion.sync &= !SYNC_DELTAS;
                }
            }
            SOFTBUTTON_DOWN => {
                input.btn.sync &= !PRIMARYBTN;
                input.sbtn.buttons |= tp.softbutton;
                if tp.softbutton != tp.tap.button {
                    input.sbtn.sync |= tp.softbutton;
                }
            }
            SOFTBUTTON_UP => {
                input.btn.sync &= !PRIMARYBTN;
                if tp.softbutton != tp.tap.button {
                    input.sbtn.buttons &= !tp.softbutton;
                    input.sbtn.sync |= tp.softbutton;
                }
                tp.softbutton = 0;
            }
            TAPBUTTON_SYNC => {
                if tp.tap.button != 0 {
                    input.btn.sync &= !tp.tap.button;
                }
            }
            TAPBUTTON_DOWN => {
                tp.tap.button = tp.tap.pending;
                tp.tap.pending = 0;
                input.sbtn.buttons |= tp.tap.button;
                if wstpad_tap_sync(input, tp) {
                    input.sbtn.sync |= tp.tap.button;
                }
            }
            TAPBUTTON_UP => {
                if tp.tap.button != tp.softbutton {
                    input.sbtn.buttons &= !tp.tap.button;
                }
                if wstpad_tap_sync(input, tp) {
                    input.sbtn.sync |= tp.tap.button;
                }
                tp.tap.button = 0;
            }
            HSCROLL => {
                input.motion.dw = tp.scroll.dw;
                input.motion.sync |= SYNC_DELTAS;
            }
            VSCROLL => {
                input.motion.dz = tp.scroll.dz;
                input.motion.sync |= SYNC_DELTAS;
            }
            _ => {
                printf(format_args!("[wstpad] invalid cmd {}\n", n));
            }
        }
    }
}

/// `clear_touchstates`: set the state of touches that have ended. `TOUCH_END` is a
/// transitional state and will be changed to `TOUCH_NONE` before `process_input()`
/// returns.
fn clear_touchstates(input: &WsmouseInput, tp: &mut Wstpad, state: Touchstates) {
    let touches = input.mt.sync[MTS_TOUCH] & !input.mt.touches;
    for slot in foreachbit(touches) {
        tp.tpad_touches[slot as usize].state = state;
    }
}

/// `wstpad_mt_inputs`: the touches of an MT frame.
pub fn wstpad_mt_inputs(input: &WsmouseInput, tp: &mut Wstpad) {
    // TOUCH_BEGIN
    let touches = input.mt.touches & input.mt.sync[MTS_TOUCH];
    for slot in foreachbit(touches) {
        let slot = slot as usize;
        let p = *pos(input, &tp.tpad_touches[slot]);
        let x = normalize_abs(&input.filter.h, p.x);
        let y = normalize_abs(&input.filter.v, p.y);
        let flags = edge_flags(tp, x, y);
        let time = tp.time;
        let t = &mut tp.tpad_touches[slot];
        t.state = TOUCH_BEGIN;
        t.x = x;
        t.y = y;
        t.orig.x = t.x;
        t.orig.y = t.y;
        t.orig.time = time;
        t.flags = flags;
        wstpad_set_direction(tp, slot, 0, 0);
    }

    // TOUCH_UPDATE
    let touches = input.mt.touches & input.mt.frame;
    let inactive = if touches & tp.mtcycle != 0 {
        // Slot data may be synchronized separately, in any order, or not at all if there
        // is no delta. Identify the touches without deltas.
        let inactive = input.mt.touches & !tp.mtcycle;
        tp.mtcycle = touches;
        inactive
    } else {
        tp.mtcycle |= touches;
        0
    };
    let touches = input.mt.touches & !input.mt.sync[MTS_TOUCH];
    for slot in foreachbit(touches) {
        let bit = 1u32 << slot;
        let slot = slot as usize;
        tp.tpad_touches[slot].state = TOUCH_UPDATE;
        if bit & input.mt.frame != 0 {
            let p = *pos(input, &tp.tpad_touches[slot]);
            let (tx, ty) = (tp.tpad_touches[slot].x, tp.tpad_touches[slot].y);
            let mut dx = normalize_abs(&input.filter.h, p.x) - tx;
            let mut dy = normalize_abs(&input.filter.v, p.y) - ty;
            let (x, y) = (tx + dx, ty + dy);
            let flags = edge_flags(tp, x, y);
            let t = &mut tp.tpad_touches[slot];
            t.x = x;
            t.y = y;
            t.flags &= !EDGES | flags;
            if wsmouse_hysteresis(input, &p) {
                dx = 0;
                dy = 0;
            }
            wstpad_set_direction(tp, slot, dx, dy);
        } else if bit & inactive != 0 {
            wstpad_set_direction(tp, slot, 0, 0);
        }
    }

    clear_touchstates(input, tp, TOUCH_END);
}

/// `wstpad_mt_masks`: identify "thumb" contacts in the bottom area.
///
/// The identification has three stages:
/// 1. If exactly one of two or more touches is in the bottom area, it is masked, which
///    means it does not receive pointer control as long as there are alternatives. Once
///    set, the mask will only be cleared when the touch is released. Tap detection ignores
///    a masked touch if it does not participate in a tap gesture.
/// 2. If the pointer-controlling touch is moving stably while a masked touch in the bottom
///    area is resting, or only moving minimally, the pointer mask is copied to
///    `tp->ignore`. In this stage, the masked touch does not block pointer movement, and it
///    is ignored by `wstpad_f2scroll()`. Decisions are made more or less immediately, there
///    may be errors in edge cases. If a fast or long upward movement is detected,
///    `tp->ignore` is cleared. There is no other transition from stage 2 to scrolling, or
///    vice versa, for a pair of touches.
/// 3. If `tp->ignore` is set and the touch is resting, it is marked as thumb, and it will
///    be ignored until it ends.
pub fn wstpad_mt_masks(input: &mut WsmouseInput, tp: &mut Wstpad) {
    tp.ignore &= input.mt.touches;

    if tp.contacts < 2 {
        return;
    }

    if tp.ignore != 0 {
        let slot = (ffs(tp.ignore) - 1) as usize;
        let t = tp.tpad_touches[slot];
        if t.flags & THUMB != 0 {
            return;
        }
        if t.dir < 0 && wstpad_is_stable(input, tp, &t) {
            tp.tpad_touches[slot].flags |= THUMB;
            return;
        }
        // The edge.low area is a bit larger than the bottom area.
        let p = pos(input, &t);
        if t.y >= tp.edge.low || (north(t.dir) && magnitude(input, p.dx, p.dy) >= MAG_MEDIUM) {
            tp.ignore = 0;
        }
        return;
    }

    if input.mt.ptr_mask == 0 {
        let mut mask = !0u32;
        for slot in foreachbit(input.mt.touches) {
            if tp.tpad_touches[slot as usize].flags & B_EDGE != 0 {
                mask &= 1 << slot;
                input.mt.ptr_mask = mask;
            }
        }
    }

    if (input.mt.ptr_mask & !input.mt.ptr) != 0
        && !(tp.scroll.dz != 0 || tp.scroll.dw != 0)
        && tp.t().dir >= 0
        && wstpad_is_stable(input, tp, tp.t())
    {
        let slot = (ffs(input.mt.ptr_mask) - 1) as usize;
        let t = tp.tpad_touches[slot];

        if t.y >= tp.edge.low {
            return;
        }

        if !wstpad_is_stable(input, tp, &t) {
            return;
        }

        // Default hysteresis limits are low. Make a strict check.
        let p = *pos(input, tp.t());
        if p.acc_dx.abs() < 3 * input.filter.h.hysteresis
            && p.acc_dy.abs() < 3 * input.filter.v.hysteresis
        {
            return;
        }

        if t.dir >= 0 {
            // Treat t as thumb if it is slow while tp->t is fast.
            let tpos = pos(input, &t);
            if magnitude(input, tpos.dx, tpos.dy) > MAG_LOW
                || magnitude(input, p.dx, p.dy) < MAG_MEDIUM
            {
                return;
            }
        }

        tp.ignore = input.mt.ptr_mask;
    }
}

/// `wstpad_touch_inputs`: the touches of a frame.
pub fn wstpad_touch_inputs(input: &mut WsmouseInput, tp: &mut Wstpad) {
    tp.btns = input.btn.buttons;
    tp.btns_sync = input.btn.sync;

    tp.prev_contacts = tp.contacts;
    tp.contacts = input.touch.contacts;

    if tp.contacts == 1
        && ((tp.params.f2width != 0 && input.touch.width >= tp.params.f2width)
            || (tp.params.f2pressure != 0 && input.touch.pressure >= tp.params.f2pressure))
    {
        tp.contacts = 2;
    }

    if is_mt(tp) {
        wstpad_mt_inputs(input, tp);
        if input.mt.ptr != 0 {
            tp.t = (ffs(input.mt.ptr) - 1) as usize;
        }
        wstpad_mt_masks(input, tp);
    } else {
        let slot = tp.t;
        let state = if tp.contacts != 0 {
            if tp.prev_contacts != 0 {
                TOUCH_UPDATE
            } else {
                TOUCH_BEGIN
            }
        } else if tp.prev_contacts != 0 {
            TOUCH_END
        } else {
            TOUCH_NONE
        };
        tp.tpad_touches[slot].state = state;

        let (mut dx, mut dy) = (0, 0);
        let p = *pos(input, &tp.tpad_touches[slot]);
        let x = normalize_abs(&input.filter.h, p.x);
        let y = normalize_abs(&input.filter.v, p.y);
        let flags = edge_flags(tp, x, y);
        let time = tp.time;
        let t = &mut tp.tpad_touches[slot];
        if state == TOUCH_BEGIN {
            t.x = x;
            t.orig.x = x;
            t.y = y;
            t.orig.y = y;
            t.orig.time = time;
            t.flags = flags;
        } else if input.motion.sync & SYNC_POSITION != 0 {
            if !wsmouse_hysteresis(input, &p) {
                dx = x - t.x;
                dy = y - t.y;
            }
            t.x = x;
            t.y = y;
            t.flags &= !EDGES | flags;
        }
        wstpad_set_direction(tp, slot, dx, dy);
    }
}

/// `t2_ignore`: if there are two touches, do not block pointer movement if they perform a
/// click-and-drag action, or if the second touch is resting in the bottom area.
fn t2_ignore(input: &WsmouseInput, tp: &Wstpad) -> bool {
    tp.contacts == 2 && ((tp.btns & PRIMARYBTN) != 0 || (tp.ignore & !input.mt.ptr) != 0)
}

/// `wstpad_process_input`: the touchpad handlers of a frame.
pub fn wstpad_process_input(input: &mut WsmouseInput, tp: &mut Wstpad, evq: &EvqAccess<'_>) {
    tp.time = evq.ts;
    wstpad_touch_inputs(input, tp);

    let mut cmds = 0;
    let mut handlers = tp.handlers;
    if disable(tp) {
        handlers &= (1 << TOPBUTTON_HDLR) | (1 << SOFTBUTTON_HDLR);
    }

    for hdlr in foreachbit(handlers) {
        match hdlr {
            SOFTBUTTON_HDLR | TOPBUTTON_HDLR => wstpad_softbuttons(input, tp, &mut cmds, hdlr),
            TAP_HDLR => wstpad_tap(input, tp, &mut cmds),
            F2SCROLL_HDLR => wstpad_f2scroll(input, tp, &mut cmds),
            EDGESCROLL_HDLR => wstpad_edgescroll(input, tp, &mut cmds),
            CLICK_HDLR => wstpad_click(tp),
            _ => {}
        }
    }

    // Check whether pointer movement should be blocked.
    if (input.motion.dx != 0 || input.motion.dy != 0)
        && (disable(tp)
            || (tp.t().flags & tp.freeze) != 0
            || tp.time < tp.freeze_ts
            || (tp.contacts > 1 && !t2_ignore(input, tp)))
    {
        cmds |= 1 << CLEAR_MOTION_DELTAS;
    }

    wstpad_cmds(input, tp, cmds);

    if is_mt(tp) {
        clear_touchstates(input, tp, TOUCH_NONE);
    }
}

/// `wstpad_track_interval`: try to determine the average interval between two updates.
///
/// Various conditions are checked in order to ensure that only valid samples enter into
/// the calculation. Above all, it is restricted to motion events occurring when there is
/// only one contact. MT devices may need more than one packet to transmit their state if
/// there are multiple touches, and the update frequency may be higher in this case.
pub fn wstpad_track_interval(input: &mut WsmouseInput, time: &Timespec) {
    const LIMIT: Timespec = Timespec::new(0, 30 * 1_000_000);

    if input.motion.sync == 0 || (input.touch.sync & SYNC_CONTACTS) != 0 || input.touch.contacts > 1
    {
        input.intv.track = 0;
        return;
    }
    if input.intv.track != 0 {
        let ts = timespecsub(time, &input.intv.ts);
        if ts < LIMIT {
            // The unit of the sum is 4096 nanoseconds.
            input.intv.sum += ts.tv_nsec >> 12;
            input.intv.samples += 1;
            let samples = input.intv.samples;
            // Make the first calculation quickly and later a more reliable one:
            if samples == 8 {
                input.intv.avg = input.intv.sum << 9;
                wstpad_init_deceleration(input);
            } else if samples == 128 {
                input.intv.avg = input.intv.sum << 5;
                wstpad_init_deceleration(input);
                input.intv.samples = 0;
                input.intv.sum = 0;
                input.flags &= !TRACK_INTERVAL;
            }
        }
    }
    input.intv.ts = *time;
    input.intv.track = 1;
}

/// `wstpad_decelerate`: scale small deltas down; true if it did.
///
/// The default acceleration options of X don't work convincingly with touchpads (the
/// synaptics driver installs its own "acceleration profile" and callback function). As a
/// preliminary workaround, this filter applies a simple deceleration scheme to small
/// deltas, based on the "magnitude" of the delta pair. A magnitude of 8 corresponds,
/// roughly, to a speed of (filter.dclr / 12.5) device units per millisecond. If its
/// magnitude is smaller than 7 a delta will be downscaled by the factor 2/8, deltas with
/// magnitudes from 7 to 11 by factors ranging from 3/8 to 7/8.
pub fn wstpad_decelerate(input: &mut WsmouseInput, dx: &mut i32, dy: &mut i32) -> bool {
    let mag = magnitude(input, *dx, *dy);

    // Don't change deceleration levels abruptly.
    let mag = (mag + 7 * input.filter.mag) / 8;
    // Don't use arbitrarily high values.
    input.filter.mag = mag.min(24 << 12);

    let n = ((mag >> 12) - 4).max(2);
    if n < 8 {
        // Scale by (n / 8).
        let h = *dx * n + input.filter.h.dclr_rmdr;
        let v = *dy * n + input.filter.v.dclr_rmdr;
        input.filter.h.dclr_rmdr = if h >= 0 { h & 7 } else { -(-h & 7) };
        input.filter.v.dclr_rmdr = if v >= 0 { v & 7 } else { -(-v & 7) };
        *dx = h / 8;
        *dy = v / 8;
        return true;
    }
    false
}

/// `wstpad_filter`: transform and filter the coordinate inputs into deltas.
pub fn wstpad_filter(input: &mut WsmouseInput) {
    let pos = input.motion.pos;
    let mut strength = (input.filter.mode & 7) as i32;
    let (h, v) = (input.filter.h, input.filter.v);

    let (mut dx, mut dy) = if input.motion.sync & SYNC_POSITION == 0
        || (h.dmax != 0 && pos.dx.abs() > h.dmax)
        || (v.dmax != 0 && pos.dy.abs() > v.dmax)
    {
        (0, 0)
    } else {
        (pos.dx, pos.dy)
    };

    if wsmouse_hysteresis(input, &pos) {
        dx = 0;
        dy = 0;
    }

    if input.filter.dclr != 0 && wstpad_decelerate(input, &mut dx, &mut dy) {
        // Strong smoothing may hamper the precision at low speeds.
        strength = strength.min(2);
    }

    if strength != 0 {
        let reset = (input.touch.sync & SYNC_CONTACTS) != 0 || input.mt.ptr != input.mt.prev_ptr;
        let h = &mut input.filter.h;
        let v = &mut input.filter.v;
        if reset {
            h.avg = 0;
            v.avg = 0;
        }
        // Use a weighted decaying average for smoothing.
        dx = dx * (8 - strength) + h.avg * strength + h.avg_rmdr;
        dy = dy * (8 - strength) + v.avg * strength + v.avg_rmdr;
        h.avg_rmdr = if dx >= 0 { dx & 7 } else { -(-dx & 7) };
        v.avg_rmdr = if dy >= 0 { dy & 7 } else { -(-dy & 7) };
        dx /= 8;
        h.avg = dx;
        dy /= 8;
        v.avg = dy;
    }

    input.motion.dx = dx;
    input.motion.dy = dy;
}

/// `wstpad_compat_convert`: compatibility-mode conversions. `wstpad_filter` transforms and
/// filters the coordinate inputs, extended functionality is provided by
/// `wstpad_process_input`.
pub fn wstpad_compat_convert(input: &mut WsmouseInput, evq: &mut EvqAccess<'_>) {
    if input.flags & TRACK_INTERVAL != 0 {
        wstpad_track_interval(input, &evq.ts);
    }

    wstpad_filter(input);

    if (input.motion.dx != 0 || input.motion.dy != 0) && input.motion.sync & SYNC_DELTAS == 0 {
        input.motion.dz = 0;
        input.motion.dw = 0;
        input.motion.sync |= SYNC_DELTAS;
    }

    // SAFETY: the caller (`wsmouse_input_sync`, under the input's guard) has the input
    // exclusively and does not reach `input->tp` while this runs.
    if let Some(tp) = unsafe { tp_of(input) } {
        wstpad_process_input(input, tp, evq);
    }

    input.motion.sync &= !SYNC_POSITION;
    input.touch.sync = 0;
}

/// `wstpad_init`: allocate the touchpad state. `Err(EINVAL)` for the C's -1.
pub fn wstpad_init(input: &mut WsmouseInput) -> Result<(), Errno> {
    if input.tp.is_some() {
        return Ok(());
    }

    let mem = malloc(size_of::<Wstpad>(), M_DEVBUF, M_WAITOK | M_ZERO).ok_or(Errno::EINVAL)?;
    let tp = mem.cast::<Wstpad>();
    // SAFETY: a fresh block of `size_of::<Wstpad>()` bytes, aligned by `malloc`, which
    // the input owns until `wstpad_cleanup`.
    unsafe { tp.as_ptr().write(Wstpad::new()) };
    input.tp = Some(tp);
    // SAFETY: initialised above; this function has the input exclusively.
    let tp = unsafe { &mut *tp.as_ptr() };

    tp.t = 0;
    if input.mt.num_slots != 0 {
        tp.features |= WSTPAD_MT;
        for i in 0..input.mt.num_slots as usize {
            tp.tpad_touches[i].pos = TouchPos::Slot(i);
        }
    } else {
        tp.tpad_touches[0].pos = TouchPos::Motion;
    }

    let arg = input
        .dv
        .map_or(ptr::null_mut(), |d| d.as_ptr().cast::<c_void>());
    timeout_set(&tp.tap.to, wstpad_tap_timeout, arg);

    tp.ratio = input.filter.ratio;

    Ok(())
}

/// `isqrt`: integer square root (Halleck's method).
///
/// An adaption of code from John B. Halleck (from
/// <http://www.cc.utah.edu/~nahaj/factoring/code.html>). This version is used and published
/// under the OpenBSD license terms with his permission.
///
/// Cf. also Martin Guy's "Square root by abacus" method.
pub fn isqrt(mut n: u32) -> u32 {
    let mut root = 0u32;
    let mut sqbit = 1u32 << (u32::BITS - 2);
    while sqbit != 0 {
        if n >= (sqbit | root) {
            n -= sqbit | root;
            root = (root >> 1) | sqbit;
        } else {
            root >>= 1;
        }
        sqbit >>= 2;
    }
    root
}

/// `wstpad_init_deceleration`: the deceleration coefficients of `filter.dclr` and the
/// average update interval.
pub fn wstpad_init_deceleration(input: &mut WsmouseInput) {
    let dclr = input.filter.dclr;
    if dclr == 0 {
        return;
    }

    let dclr = dclr.max(4);

    // For a standard update rate of about 80Hz, (dclr) units will be mapped to a magnitude
    // of 8. If the average rate is significantly higher or lower, adjust the coefficient
    // accordingly:
    let n = if input.intv.avg == 0 {
        8
    } else {
        ((8 * 13_000_000 / input.intv.avg) as i32).clamp(4, 32)
    };
    input.filter.h.mag_scale = (n << 12) / dclr;
    input.filter.v.mag_scale = (if input.filter.ratio != 0 {
        n * input.filter.ratio
    } else {
        n << 12
    }) / dclr;
    input.filter.h.dclr_rmdr = 0;
    input.filter.v.dclr_rmdr = 0;
    input.flags |= TRACK_INTERVAL;
}

/// `wstpad_configure`: the touchpad defaults from the hardware description, then the
/// edges and handlers from the parameters. `Err(EINVAL)` for the C's -1.
pub fn wstpad_configure(input: &mut WsmouseInput) -> Result<(), Errno> {
    let width = (input.hw.x_max - input.hw.x_min).abs();
    let height = (input.hw.y_max - input.hw.y_min).abs();
    if width == 0 || height == 0 {
        return Err(Errno::EINVAL); // We can't do anything.
    }

    if input.tp.is_none() && wstpad_init(input).is_err() {
        return Err(Errno::EINVAL);
    }
    // SAFETY: this function has the input exclusively and does not reach `input->tp`
    // again while `tp` is used.
    let Some(tp) = (unsafe { tp_of(input) }) else {
        return Err(Errno::EINVAL);
    };

    let diag_of = || {
        isqrt(
            width
                .wrapping_mul(width)
                .wrapping_add(height.wrapping_mul(height)) as u32,
        ) as i32
    };

    if input.flags & CONFIGURED == 0 {
        // The filter parameters are derived from the length of the diagonal in device
        // units, with some magic constants which are partly adapted from libinput or
        // synaptics code, or are based on tests and guess work. The absolute resolution
        // values might not be reliable, but if they are present the settings are adapted to
        // the ratio.
        let (mut h_res, mut v_res) = (input.hw.h_res, input.hw.v_res);
        if h_res == 0 || v_res == 0 {
            h_res = 1;
            v_res = 1;
        }
        let diag = diag_of();
        input.filter.h.scale = (920.min(diag) << 12) / diag;
        input.filter.v.scale = input.filter.h.scale * h_res / v_res;
        let h_unit = (diag / 280).max(3);
        let v_unit = ((h_unit * v_res + h_res / 2) / h_res).max(3);
        input.filter.h.hysteresis = h_unit;
        input.filter.v.hysteresis = v_unit;
        input.filter.mode = FILTER_MODE_DEFAULT;
        input.filter.dclr = h_unit - h_unit / 5;
        wstpad_init_deceleration(input);

        tp.features &= WSTPAD_MT | WSTPAD_DISABLE;

        if input.hw.contacts_max != 1 {
            tp.features |= WSTPAD_TWOFINGERSCROLL;
        } else {
            tp.features |= WSTPAD_EDGESCROLL;
        }

        if input.hw.hw_type == WSMOUSEHW_CLICKPAD {
            if input.hw.type_ == WSMOUSE_TYPE_SYNAP_SBTN as i32 {
                tp.features |= WSTPAD_TOPBUTTONS;
            } else {
                tp.features |= WSTPAD_SOFTBUTTONS;
                tp.features |= WSTPAD_SOFTMBTN;
            }
        }

        tp.params.left_edge = V_EDGE_RATIO_DEFAULT;
        tp.params.right_edge = V_EDGE_RATIO_DEFAULT;
        tp.params.bottom_edge = if tp.features & WSTPAD_SOFTBUTTONS != 0 {
            B_EDGE_RATIO_DEFAULT
        } else {
            0
        };
        tp.params.top_edge = if tp.features & WSTPAD_TOPBUTTONS != 0 {
            T_EDGE_RATIO_DEFAULT
        } else {
            0
        };
        tp.params.center_width = CENTER_RATIO_DEFAULT;

        tp.tap.maxtime.tv_nsec = i64::from(TAP_MAXTIME_DEFAULT) * 1_000_000;
        tp.tap.clicktime = TAP_CLICKTIME_DEFAULT;
        tp.tap.locktime = TAP_LOCKTIME_DEFAULT;

        tp.scroll.hdist = 4 * h_unit;
        tp.scroll.vdist = 4 * v_unit;
        tp.tap.maxdist = 4 * h_unit;

        if is_mt(tp)
            && h_res > 1
            && v_res > 1
            && input.hw.hw_type == WSMOUSEHW_CLICKPAD
            && (width + h_res / 2) / h_res > 100
            && (height + v_res / 2) / v_res > 60
        {
            tp.params.mtbtn_maxdist = h_res * 35;
        } else {
            tp.params.mtbtn_maxdist = -1; // not available
        }
    }

    // A touch with a flag set in this mask does not move the pointer.
    tp.freeze = EDGES;

    let hw = input.hw;
    let mut offset = width * tp.params.left_edge / 4096;
    tp.edge.left = if offset != 0 {
        hw.x_min + offset
    } else {
        i32::MIN
    };
    offset = width * tp.params.right_edge / 4096;
    tp.edge.right = if offset != 0 {
        hw.x_max - offset
    } else {
        i32::MAX
    };
    offset = height * tp.params.bottom_edge / 4096;
    tp.edge.bottom = if offset != 0 {
        hw.y_min + offset
    } else {
        i32::MIN
    };
    tp.edge.low = tp.edge.bottom.wrapping_add(offset / 2);
    offset = height * tp.params.top_edge / 4096;
    tp.edge.top = if offset != 0 {
        hw.y_max - offset
    } else {
        i32::MAX
    };

    offset = width * tp.params.center_width.abs() / 8192;
    tp.edge.center = hw.x_min + width / 2;
    tp.edge.center_left = tp.edge.center - offset;
    tp.edge.center_right = tp.edge.center + offset;

    // Make the MTBUTTONS configuration consistent. A non-negative 'maxdist' value makes the
    // feature visible in wsconsctl. 0-values are replaced by a default (one fourth of the
    // length of the touchpad diagonal).
    if tp.params.mtbtn_maxdist < 0 {
        tp.features &= !WSTPAD_MTBUTTONS;
    } else if tp.params.mtbtn_maxdist == 0 {
        tp.params.mtbtn_maxdist = diag_of() / 4;
    }

    tp.handlers = 0;

    if tp.features & (WSTPAD_SOFTBUTTONS | WSTPAD_MTBUTTONS) != 0 {
        tp.handlers |= 1 << SOFTBUTTON_HDLR;
    }
    if tp.features & WSTPAD_TOPBUTTONS != 0 {
        tp.handlers |= 1 << TOPBUTTON_HDLR;
    }
    if tp.features & WSTPAD_TWOFINGERSCROLL != 0 {
        tp.handlers |= 1 << F2SCROLL_HDLR;
    } else if tp.features & WSTPAD_EDGESCROLL != 0 {
        tp.handlers |= 1 << EDGESCROLL_HDLR;
    }

    if tp.tap.btnmap.iter().any(|&b| b != 0) {
        tp.tap.clicktime = tp.tap.clicktime.clamp(80, 350);
        if tp.tap.locktime != 0 {
            tp.tap.locktime = tp.tap.locktime.clamp(150, 5000);
        }
        tp.handlers |= 1 << TAP_HDLR;
    }

    if hw.hw_type == WSMOUSEHW_CLICKPAD {
        tp.handlers |= 1 << CLICK_HDLR;
    }

    tp.sbtnswap = if tp.features & WSTPAD_SWAPSIDES != 0 {
        LEFTBTN | RIGHTBTN
    } else {
        0
    };

    Ok(())
}

/// `wstpad_reset`: end a tap gesture and release the soft buttons.
pub fn wstpad_reset(input: &mut WsmouseInput) {
    // SAFETY: this function has the input exclusively and does not reach `input->tp`
    // again while `tp` is used.
    if let Some(tp) = unsafe { tp_of(input) } {
        timeout_del(&tp.tap.to);
        tp.tap.state = TAP_DETECT;
    }

    if input.sbtn.buttons != 0 {
        input.sbtn.sync = input.sbtn.buttons;
        input.sbtn.buttons = 0;
    }
}

/// `wstpad_cleanup`: free the touchpad state.
pub fn wstpad_cleanup(input: &mut WsmouseInput) {
    let Some(tp) = input.tp.take() else {
        return;
    };

    // SAFETY: `wstpad_init`'s initialised state; `take` removed the input's pointer, so
    // this is the last reference.
    timeout_del(&unsafe { tp.as_ref() }.tap.to);
    free(tp.cast(), M_DEVBUF, size_of::<Wstpad>());
}

/// The `WSTPAD_*` feature of a `WSMOUSECFG_SOFTBUTTONS .. WSMOUSECFG_MTBUTTONS` key.
fn feature_flag(key: i32) -> Option<u32> {
    Some(match key {
        WSMOUSECFG_SOFTBUTTONS => WSTPAD_SOFTBUTTONS,
        WSMOUSECFG_SOFTMBTN => WSTPAD_SOFTMBTN,
        WSMOUSECFG_TOPBUTTONS => WSTPAD_TOPBUTTONS,
        WSMOUSECFG_TWOFINGERSCROLL => WSTPAD_TWOFINGERSCROLL,
        WSMOUSECFG_EDGESCROLL => WSTPAD_EDGESCROLL,
        WSMOUSECFG_HORIZSCROLL => WSTPAD_HORIZSCROLL,
        WSMOUSECFG_SWAPSIDES => WSTPAD_SWAPSIDES,
        WSMOUSECFG_DISABLE => WSTPAD_DISABLE,
        WSMOUSECFG_MTBUTTONS => WSTPAD_MTBUTTONS,
        _ => return None,
    })
}

/// `wstpad_set_param`: `EINVAL` without a touchpad, `ENOTSUP` for a key that is not a
/// touchpad parameter.
pub fn wstpad_set_param(input: &mut WsmouseInput, key: i32, val: i32) -> Result<(), Errno> {
    // SAFETY: this function has the input exclusively and does not reach `input->tp`
    // again while `tp` is used.
    let Some(tp) = (unsafe { tp_of(input) }) else {
        return Err(Errno::EINVAL);
    };

    if let Some(flag) = feature_flag(key) {
        if val != 0 {
            tp.features |= flag;
        } else {
            tp.features &= !flag;
        }
        return Ok(());
    }
    match key {
        WSMOUSECFG_LEFT_EDGE => tp.params.left_edge = val,
        WSMOUSECFG_RIGHT_EDGE => tp.params.right_edge = val,
        WSMOUSECFG_TOP_EDGE => tp.params.top_edge = val,
        WSMOUSECFG_BOTTOM_EDGE => tp.params.bottom_edge = val,
        WSMOUSECFG_CENTERWIDTH => tp.params.center_width = val,
        WSMOUSECFG_HORIZSCROLLDIST => tp.scroll.hdist = val,
        WSMOUSECFG_VERTSCROLLDIST => tp.scroll.vdist = val,
        WSMOUSECFG_F2WIDTH => tp.params.f2width = val,
        WSMOUSECFG_F2PRESSURE => tp.params.f2pressure = val,
        WSMOUSECFG_TAP_MAXTIME => tp.tap.maxtime.tv_nsec = i64::from(val.min(999)) * 1_000_000,
        WSMOUSECFG_TAP_CLICKTIME => tp.tap.clicktime = val,
        WSMOUSECFG_TAP_LOCKTIME => tp.tap.locktime = val,
        WSMOUSECFG_TAP_ONE_BTNMAP => tp.tap.btnmap[0] = btnmask(val),
        WSMOUSECFG_TAP_TWO_BTNMAP => tp.tap.btnmap[1] = btnmask(val),
        WSMOUSECFG_TAP_THREE_BTNMAP => tp.tap.btnmap[2] = btnmask(val),
        WSMOUSECFG_MTBTN_MAXDIST => {
            if is_mt(tp) {
                tp.params.mtbtn_maxdist = val;
            }
        }
        _ => return Err(Errno::ENOTSUP),
    }

    Ok(())
}

/// `wstpad_get_param`: the value of a touchpad parameter; `EINVAL` without a touchpad,
/// `ENOTSUP` for a key that is not a touchpad parameter.
pub fn wstpad_get_param(input: &mut WsmouseInput, key: i32) -> Result<i32, Errno> {
    // SAFETY: this function has the input exclusively and does not reach `input->tp`
    // again while `tp` is used.
    let Some(tp) = (unsafe { tp_of(input) }) else {
        return Err(Errno::EINVAL);
    };

    if let Some(flag) = feature_flag(key) {
        return Ok(i32::from(tp.features & flag != 0));
    }
    Ok(match key {
        WSMOUSECFG_LEFT_EDGE => tp.params.left_edge,
        WSMOUSECFG_RIGHT_EDGE => tp.params.right_edge,
        WSMOUSECFG_TOP_EDGE => tp.params.top_edge,
        WSMOUSECFG_BOTTOM_EDGE => tp.params.bottom_edge,
        WSMOUSECFG_CENTERWIDTH => tp.params.center_width,
        WSMOUSECFG_HORIZSCROLLDIST => tp.scroll.hdist,
        WSMOUSECFG_VERTSCROLLDIST => tp.scroll.vdist,
        WSMOUSECFG_F2WIDTH => tp.params.f2width,
        WSMOUSECFG_F2PRESSURE => tp.params.f2pressure,
        WSMOUSECFG_TAP_MAXTIME => (tp.tap.maxtime.tv_nsec / 1_000_000) as i32,
        WSMOUSECFG_TAP_CLICKTIME => tp.tap.clicktime,
        WSMOUSECFG_TAP_LOCKTIME => tp.tap.locktime,
        WSMOUSECFG_TAP_ONE_BTNMAP => ffs(tp.tap.btnmap[0]),
        WSMOUSECFG_TAP_TWO_BTNMAP => ffs(tp.tap.btnmap[1]),
        WSMOUSECFG_TAP_THREE_BTNMAP => ffs(tp.tap.btnmap[2]),
        WSMOUSECFG_MTBTN_MAXDIST => tp.params.mtbtn_maxdist,
        _ => return Err(Errno::ENOTSUP),
    })
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests of the touchpad processing over synthetic touch sequences: the defaults of
    // `wstpad_configure`, one- and two-finger taps (the click comes from the tap timeout),
    // a long touch that is no tap, tap-and-drag, edge scrolling on a single-touch pad,
    // two-finger scrolling on an MT pad, the parameters, and the geometry helpers.

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
}
/* </TESTS> */
