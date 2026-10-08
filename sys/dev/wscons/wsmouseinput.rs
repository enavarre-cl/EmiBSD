/* $OpenBSD: wsmouseinput.h,v 1.15 2021/03/21 16:20:49 bru Exp $ */
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

//! `<dev/wscons/wsmouseinput.h>`: wsmouse input processing, the private header of
//! `wsmouse.c` and `wstpad.c`.
//!
//! Upstream: sys/dev/wscons/wsmouseinput.h @ 3ce1f3f79392
//!
//! A [`WsmouseInput`] is the input state of one mouse: the button states (`btn`, and the
//! touchpad's soft buttons `sbtn`), the motion (deltas and the absolute position), the
//! single-touch state, the multitouch slots, the filters that the parameters of
//! `WSMOUSEIO_SETPARAMS` set, the hardware description and the touchpad state of
//! `wstpad.rs`. A driver's reports change it, and each `wsmouse_input_sync` converts what
//! changed (the `sync` masks) into events, through an [`EvqAccess`] to the event queue.
//!
//! ## Deviations
//! - `struct mt_state`'s `slots` and `matrix` are one `malloc`ed block, as in C
//!   (`wsmouse_mt_init`): the pointers are `Option<NonNull<_>>` and [`MtState::slots`],
//!   [`MtState::slots_mut`] and [`MtState::tracking`] make the slices.
//! - `struct wsmouseinput`'s `evar` points at the softc's `me_evp` cell (the C's `struct
//!   wseventvar **`); [`WsmouseInput::evar`] reads it. A new member, `dv`, is the mouse's
//!   device: `DEVNAME(input)` computes it from the softc layout in C, which a reference to
//!   the input alone cannot reach in Rust ([`devname`]).
//! - The anonymous `filter` structure is [`InputFilter`]; `tp` is an
//!   `Option<NonNull<Wstpad>>` (`wstpad.rs` allocates it).
//! - `struct evq_access`'s `evar` is a reference (never NULL there) and `put` a `u32` ring
//!   index.
//! - The function-like macros are functions: `FOREACHBIT` is the iterator [`foreachbit`]
//!   over a snapshot of the bits (every C loop body changes only bits it has passed, or
//!   none), `DELTA_X_EV` .. `ABS_Y_EV` take the input, `MATRIX_SIZE` is in bytes as in C,
//!   `IS_TOUCHPAD` and `LOGTIME` are `is_touchpad` and `logtime`.
//! - The prototypes belong to `wsmouse.c` and `wstpad.c`; they are in `wsmouse.rs` and
//!   `wstpad.rs`.

use core::cell::Cell;
use core::ptr::NonNull;
use core::slice;

use crate::dev::wscons::wsconsio::{
    WSCONS_EVENT_HSCROLL, WSCONS_EVENT_MOUSE_ABSOLUTE_X, WSCONS_EVENT_MOUSE_ABSOLUTE_Y,
    WSCONS_EVENT_MOUSE_DELTA_W, WSCONS_EVENT_MOUSE_DELTA_X, WSCONS_EVENT_MOUSE_DELTA_Y,
    WSCONS_EVENT_MOUSE_DELTA_Z, WSCONS_EVENT_MOUSE_DOWN, WSCONS_EVENT_MOUSE_UP, WSCONS_EVENT_SYNC,
    WSCONS_EVENT_TOUCH_CONTACTS, WSCONS_EVENT_TOUCH_PRESSURE, WSCONS_EVENT_VSCROLL,
};
use crate::dev::wscons::wseventvar::Wseventvar;
use crate::dev::wscons::wsmousevar::{WSMOUSEHW_CLICKPAD, WSMOUSEHW_TOUCHPAD, Wsmousehw};
use crate::dev::wscons::wstpad::Wstpad;
use crate::sys::device::Device;
use crate::sys::time::Timespec;

/// `struct position`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Position {
    /// `x`.
    pub x: i32,
    /// `y`.
    pub y: i32,
    /// `dx`: unfiltered coordinate deltas.
    pub dx: i32,
    /// `dy`.
    pub dy: i32,
    /// `acc_dx`: delta sums used for filtering.
    pub acc_dx: i32,
    /// `acc_dy`.
    pub acc_dy: i32,
}

/// `struct btn_state`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BtnState {
    /// `buttons`.
    pub buttons: u32,
    /// `sync`: the buttons that changed.
    pub sync: u32,
}

/// `struct motion_state`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MotionState {
    /// `dx`: mouse input, or filtered deltas.
    pub dx: i32,
    /// `dy`.
    pub dy: i32,
    /// `dz`.
    pub dz: i32,
    /// `dw`.
    pub dw: i32,
    /// `pos`.
    pub pos: Position,
    /// `sync`: `SYNC_*`.
    pub sync: u32,
}

/// `SYNC_DELTAS`.
pub const SYNC_DELTAS: u32 = 1 << 0;
/// `SYNC_X`.
pub const SYNC_X: u32 = 1 << 1;
/// `SYNC_Y`.
pub const SYNC_Y: u32 = 1 << 2;
/// `SYNC_POSITION`.
pub const SYNC_POSITION: u32 = SYNC_X | SYNC_Y;

/// `struct touch_state`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TouchState {
    /// `pressure`.
    pub pressure: i32,
    /// `contacts`.
    pub contacts: i32,
    /// `width`.
    pub width: i32,
    /// `sync`: `SYNC_PRESSURE`, `SYNC_CONTACTS`, `SYNC_TOUCH_WIDTH`.
    pub sync: u32,
    /// `min_pressure`.
    pub min_pressure: i32,
    /// `prev_contacts`.
    pub prev_contacts: i32,
}

/// `SYNC_PRESSURE`.
pub const SYNC_PRESSURE: u32 = 1 << 0;
/// `SYNC_CONTACTS`.
pub const SYNC_CONTACTS: u32 = 1 << 1;
/// `SYNC_TOUCH_WIDTH`.
pub const SYNC_TOUCH_WIDTH: u32 = 1 << 2;

/// `struct mt_slot`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MtSlot {
    /// `pos`.
    pub pos: Position,
    /// `pressure`.
    pub pressure: i32,
    /// `id`: tracking ID.
    pub id: i32,
}

/// `MTS_TOUCH`.
pub const MTS_TOUCH: usize = 0;
/// `MTS_X`.
pub const MTS_X: usize = 1;
/// `MTS_Y`.
pub const MTS_Y: usize = 2;
/// `MTS_PRESSURE`.
pub const MTS_PRESSURE: usize = 3;

/// `MTS_SIZE`.
pub const MTS_SIZE: usize = 4;

/// `struct mt_state`.
///
/// All-zero is valid (no slots).
#[derive(Debug, Default)]
pub struct MtState {
    /// `touches`: the set of slots with active touches.
    pub touches: u32,
    /// `frame`: the set of slots with unsynchronized state.
    pub frame: u32,
    /// `num_slots`.
    pub num_slots: i32,
    /// `slots`: `num_slots` slots, `malloc`ed by `wsmouse_mt_init` (with the tracking
    /// matrix behind them).
    pub slots: Option<NonNull<MtSlot>>,
    /// `sync`: the sets of changes per slot axis.
    pub sync: [u32; MTS_SIZE],
    /// `num_touches`.
    pub num_touches: i32,
    /// `ptr`: pointer control.
    pub ptr: u32,
    /// `ptr_cycle`.
    pub ptr_cycle: u32,
    /// `prev_ptr`.
    pub prev_ptr: u32,
    /// `ptr_mask`.
    pub ptr_mask: u32,
    /// `matrix`: a buffer for the MT tracking function, `MATRIX_SIZE(num_slots)` bytes right
    /// behind the slots; `None` without tracking.
    pub matrix: Option<NonNull<i32>>,
}

impl MtState {
    /// `mt->slots[0 .. num_slots]`.
    pub fn slots(&self) -> &[MtSlot] {
        match self.slots {
            // SAFETY: `wsmouse_mt_init` allocated `num_slots` zeroed slots there, owned by
            // this state until `free_mt_slots` clears the pointer.
            Some(p) => unsafe { slice::from_raw_parts(p.as_ptr(), self.num_slots as usize) },
            None => &[],
        }
    }

    /// `mt->slots[0 .. num_slots]`, to change.
    pub fn slots_mut(&mut self) -> &mut [MtSlot] {
        match self.slots {
            // SAFETY: as in `slots`; the `&mut self` makes this the only view.
            Some(p) => unsafe { slice::from_raw_parts_mut(p.as_ptr(), self.num_slots as usize) },
            None => &mut [],
        }
    }

    /// The slots and the tracking buffer (`mt->matrix`, `MATRIX_SIZE(num_slots)` bytes as
    /// `int`s): `None` without tracking.
    pub fn tracking(&mut self) -> Option<(&[MtSlot], &mut [i32])> {
        let m = self.matrix?;
        let n = self.num_slots as usize;
        // SAFETY: `wsmouse_mt_init` put the matrix right behind the `n` slots in the same
        // allocation, `matrix_size(n)` bytes long; the two ranges do not overlap, and the
        // `&mut self` makes these the only views.
        unsafe {
            Some((
                slice::from_raw_parts(self.slots?.as_ptr(), n),
                slice::from_raw_parts_mut(m.as_ptr(), matrix_size(n) / size_of::<i32>()),
            ))
        }
    }
}

/// `struct axis_filter`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AxisFilter {
    /// `scale`: a scale factor in [*.12] fixed-point format.
    pub scale: i32,
    /// `rmdr`.
    pub rmdr: i32,
    /// `inv`: invert coordinates.
    pub inv: i32,
    /// `hysteresis`: hysteresis limit.
    pub hysteresis: i32,
    /// `avg`: weighted delta average.
    pub avg: i32,
    /// `avg_rmdr`.
    pub avg_rmdr: i32,
    /// `mag_scale`: a [*.12] coefficient for "magnitudes", used for deceleration.
    pub mag_scale: i32,
    /// `dclr_rmdr`.
    pub dclr_rmdr: i32,
    /// `dmax`: ignore deltas that are greater than this limit.
    pub dmax: i32,
}

/// `struct interval`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Interval {
    /// `avg`: average update interval in nanoseconds.
    pub avg: i64,
    /// `sum`.
    pub sum: i64,
    /// `samples`.
    pub samples: i32,
    /// `ts`.
    pub ts: Timespec,
    /// `track`.
    pub track: i32,
}

/// The `filter` member of `struct wsmouseinput`: parameters and state of various input
/// filters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InputFilter {
    /// `h`.
    pub h: AxisFilter,
    /// `v`.
    pub v: AxisFilter,
    /// `dclr`: deceleration threshold.
    pub dclr: i32,
    /// `mag`: weighted average of delta magnitudes.
    pub mag: i32,
    /// `mode`: hysteresis type, smoothing factor.
    pub mode: u32,
    /// `ratio`: X/Y ratio.
    pub ratio: i32,
    /// `swapxy`.
    pub swapxy: i32,
    /// `tracking_maxdist`.
    pub tracking_maxdist: i32,
    /// `pressure_lo`.
    pub pressure_lo: i32,
    /// `pressure_hi`.
    pub pressure_hi: i32,
}

/// `struct wsmouseinput`.
///
/// All-zero is valid (no slots, no touchpad state, no queue), as the member of an `M_ZERO`
/// softc.
#[derive(Debug, Default)]
pub struct WsmouseInput {
    /// `flags`: `TPAD_COMPAT_MODE`, ... .
    pub flags: u32,
    /// `btn`.
    pub btn: BtnState,
    /// `sbtn`: softbuttons.
    pub sbtn: BtnState,
    /// `motion`.
    pub motion: MotionState,
    /// `touch`.
    pub touch: TouchState,
    /// `mt`.
    pub mt: MtState,
    /// `filter`: parameters and state of various input filters.
    pub filter: InputFilter,
    /// `hw`.
    pub hw: Wsmousehw,
    /// `intv`.
    pub intv: Interval,
    /// `tp`: the touchpad state, `malloc`ed by `wstpad_configure`.
    pub tp: Option<NonNull<Wstpad>>,
    /// `evar`: the softc's `sc_base.me_evp`, the queue events go to while open.
    pub evar: Option<NonNull<Cell<Option<NonNull<Wseventvar>>>>>,
    /// The mouse's device, for `DEVNAME` (not in C, see the module docs).
    pub dv: Option<NonNull<Device>>,
}

impl WsmouseInput {
    /// `*input->evar`: the queue the events go to, `None` while nobody has the mouse open.
    pub fn evar(&self) -> Option<&Wseventvar> {
        // SAFETY: `wsmouse_attach` points `evar` at the softc's `me_evp`, which lives as
        // long as the softc that holds this input; `me_evp` is a queue that stays allocated
        // until the close that clears it (`Wsevsrc::evp`).
        self.evar
            .and_then(|e| unsafe { e.as_ref() }.get())
            .map(|q| unsafe { q.as_ref() })
    }
}

/// `TPAD_COMPAT_MODE` (`wsmouseinput.flags`).
pub const TPAD_COMPAT_MODE: u32 = 1 << 0;
/// `TPAD_NATIVE_MODE`.
pub const TPAD_NATIVE_MODE: u32 = 1 << 1;
/// `MT_TRACKING`.
pub const MT_TRACKING: u32 = 1 << 2;
/// `REVERSE_SCROLLING`.
pub const REVERSE_SCROLLING: u32 = 1 << 3;
/// `RESYNC`.
pub const RESYNC: u32 = 1 << 16;
/// `TRACK_INTERVAL`.
pub const TRACK_INTERVAL: u32 = 1 << 17;
/// `CONFIGURED`.
pub const CONFIGURED: u32 = 1 << 18;
/// `LOG_INPUT`.
pub const LOG_INPUT: u32 = 1 << 19;
/// `LOG_EVENTS`.
pub const LOG_EVENTS: u32 = 1 << 20;

/// `SMOOTHING_MASK` (`filter.mode`, bit 0-2: smoothing factor, bit 3-n: unused).
pub const SMOOTHING_MASK: u32 = 7;
/// `FILTER_MODE_DEFAULT`.
pub const FILTER_MODE_DEFAULT: u32 = 0;

/// `struct evq_access`: the events of one `wsmouse_input_sync`, put behind `ws_put` and
/// published together.
pub struct EvqAccess<'a> {
    /// `evar`.
    pub evar: &'a Wseventvar,
    /// `ts`: the time stamp of every event.
    pub ts: Timespec,
    /// `put`: the next slot.
    pub put: u32,
    /// `result`: `EVQ_RESULT_*`.
    pub result: i32,
}

/// `EVQ_RESULT_OVERFLOW`.
pub const EVQ_RESULT_OVERFLOW: i32 = -1;
/// `EVQ_RESULT_NONE`.
pub const EVQ_RESULT_NONE: i32 = 0;
/// `EVQ_RESULT_SUCCESS`.
pub const EVQ_RESULT_SUCCESS: i32 = 1;

/// The bit positions of a mask, lowest first.
pub struct Bits(u32);

impl Iterator for Bits {
    type Item = i32;

    fn next(&mut self) -> Option<i32> {
        if self.0 == 0 {
            return None;
        }
        let i = self.0.trailing_zeros();
        self.0 &= self.0 - 1;
        Some(i as i32)
    }
}

/// `FOREACHBIT(v, i)`: the set bits of `v`, lowest first.
pub fn foreachbit(v: u32) -> Bits {
    Bits(v)
}

/// `ffs(v)`: the position of the lowest set bit, counting from 1; 0 for 0.
pub fn ffs(v: u32) -> i32 {
    if v == 0 {
        0
    } else {
        v.trailing_zeros() as i32 + 1
    }
}

/// `DELTA_X_EV(input)`.
pub fn delta_x_ev(input: &WsmouseInput) -> u32 {
    if input.filter.swapxy != 0 {
        WSCONS_EVENT_MOUSE_DELTA_Y
    } else {
        WSCONS_EVENT_MOUSE_DELTA_X
    }
}

/// `DELTA_Y_EV(input)`.
pub fn delta_y_ev(input: &WsmouseInput) -> u32 {
    if input.filter.swapxy != 0 {
        WSCONS_EVENT_MOUSE_DELTA_X
    } else {
        WSCONS_EVENT_MOUSE_DELTA_Y
    }
}

/// `ABS_X_EV(input)`.
pub fn abs_x_ev(input: &WsmouseInput) -> u32 {
    if input.filter.swapxy != 0 {
        WSCONS_EVENT_MOUSE_ABSOLUTE_Y
    } else {
        WSCONS_EVENT_MOUSE_ABSOLUTE_X
    }
}

/// `ABS_Y_EV(input)`.
pub fn abs_y_ev(input: &WsmouseInput) -> u32 {
    if input.filter.swapxy != 0 {
        WSCONS_EVENT_MOUSE_ABSOLUTE_X
    } else {
        WSCONS_EVENT_MOUSE_ABSOLUTE_Y
    }
}

/// `DELTA_Z_EV`.
pub const DELTA_Z_EV: u32 = WSCONS_EVENT_MOUSE_DELTA_Z;
/// `DELTA_W_EV`.
pub const DELTA_W_EV: u32 = WSCONS_EVENT_MOUSE_DELTA_W;
/// `VSCROLL_EV`.
pub const VSCROLL_EV: u32 = WSCONS_EVENT_VSCROLL;
/// `HSCROLL_EV`.
pub const HSCROLL_EV: u32 = WSCONS_EVENT_HSCROLL;
/// `ABS_Z_EV`.
pub const ABS_Z_EV: u32 = WSCONS_EVENT_TOUCH_PRESSURE;
/// `ABS_W_EV`.
pub const ABS_W_EV: u32 = WSCONS_EVENT_TOUCH_CONTACTS;
/// `BTN_DOWN_EV`.
pub const BTN_DOWN_EV: u32 = WSCONS_EVENT_MOUSE_DOWN;
/// `BTN_UP_EV`.
pub const BTN_UP_EV: u32 = WSCONS_EVENT_MOUSE_UP;
/// `SYNC_EV`.
pub const SYNC_EV: u32 = WSCONS_EVENT_SYNC;

/// `MATRIX_SIZE(slots)`: matrix size + buffer size for `wsmouse_matching`, in bytes.
pub const fn matrix_size(slots: usize) -> usize {
    (slots + 6) * slots * size_of::<i32>()
}

/// `IS_TOUCHPAD(input)`.
pub fn is_touchpad(input: &WsmouseInput) -> bool {
    input.hw.hw_type == WSMOUSEHW_TOUCHPAD || input.hw.hw_type == WSMOUSEHW_CLICKPAD
}

/// `LOGTIME(tsp)`: extract a four-digit millisecond value from a timespec.
pub fn logtime(ts: &Timespec) -> i32 {
    ((ts.tv_sec % 10) * 1000 + ts.tv_nsec / 1_000_000) as i32
}

/// `DEVNAME(input)`: the mouse's device name.
pub fn devname(input: &WsmouseInput) -> &str {
    match input.dv {
        // SAFETY: `wsmouse_attach` sets `dv` to the device whose softc holds this input.
        Some(dv) => unsafe { dv.as_ref() }.xname(),
        None => "wsmouse?",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bits_ffs_and_matrix_size() {
        assert_eq!(
            foreachbit(0b1010_0101).collect::<std::vec::Vec<_>>(),
            [0, 2, 5, 7]
        );
        assert_eq!(foreachbit(1 << 31).collect::<std::vec::Vec<_>>(), [31]);
        assert_eq!(foreachbit(0).count(), 0);
        assert_eq!((ffs(0), ffs(1), ffs(0b1000)), (0, 1, 4));
        assert_eq!(matrix_size(10), 640);
        let ts = Timespec {
            tv_sec: 1234,
            tv_nsec: 567_890_123,
        };
        assert_eq!(logtime(&ts), 4567);
    }

    /// The constants against `<dev/wscons/wsmouseinput.h>`.
    #[test]
    #[ignore = "needs OPENBSD_SRC"]
    fn constants_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/wscons/wsmouseinput.h");
        let _ = crate::reftest::assert_defines!(defs;
            SYNC_DELTAS, SYNC_X, SYNC_Y, SYNC_POSITION, SYNC_PRESSURE, SYNC_CONTACTS,
            SYNC_TOUCH_WIDTH, MTS_TOUCH, MTS_X, MTS_Y, MTS_PRESSURE, MTS_SIZE,
            TPAD_COMPAT_MODE, TPAD_NATIVE_MODE, MT_TRACKING, REVERSE_SCROLLING, RESYNC,
            TRACK_INTERVAL, CONFIGURED, LOG_INPUT, LOG_EVENTS, SMOOTHING_MASK,
            FILTER_MODE_DEFAULT, EVQ_RESULT_OVERFLOW, EVQ_RESULT_NONE, EVQ_RESULT_SUCCESS,
        );
    }
}
