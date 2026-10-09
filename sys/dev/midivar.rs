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
 * Copyright (c) 2003, 2004 Alexandre Ratchov
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
//! `<dev/midivar.h>`: midi(4)'s softc and its ring buffers.
//!
//! Upstream: sys/dev/midivar.h @ 3ce1f3f79392
//!
//! Each direction has a simple ring buffer of [`MIDIBUF_SIZE`] bytes; the `MIDIBUF_*`
//! macros are [`MidiBuffer`]'s methods. Everything in the buffers and the softc's
//! `isbusy` is protected by `AUDIO_LOCK` (`audio_lock`), which the interrupt handlers of
//! the hardware drivers hold when they call midi(4) back.
//!
//! ## Deviations
//! - `MIDIBUF_INIT`, `MIDIBUF_WRITE`, `MIDIBUF_READ`, `MIDIBUF_REMOVE` and the size
//!   queries are methods of [`MidiBuffer`]; the members the C mutates through a shared
//!   softc are `Cell`s, and the data bytes an `UnsafeCell` (uiomove(9) copies into and out
//!   of them).
//! - `hw_if` is an `Option` of the driver's `&'static MidiHwIf`.

use core::cell::{Cell, UnsafeCell};
use core::ffi::c_void;

use crate::dev::midi_if::MidiHwIf;
use crate::sys::device::{Device, Softc};
use crate::sys::event::Klist;
use crate::sys::timeout::Timeout;

/// `MIDI_RATE`: midi uart baud rate in bytes/second.
pub const MIDI_RATE: i32 = 3125;

/// `MIDIBUF_SIZE`: the size of each ring buffer.
pub const MIDIBUF_SIZE: usize = 1 << 10;
/// `MIDIBUF_MASK`.
pub const MIDIBUF_MASK: usize = MIDIBUF_SIZE - 1;

/// `struct midi_buffer`: a simple ring buffer. Protected by `AUDIO_LOCK`.
#[repr(C)]
pub struct MidiBuffer {
    /// `klist`: to record & wakeup poll(2).
    pub klist: Klist,
    /// `blocking`: read/write blocking.
    pub blocking: Cell<i32>,
    /// `data`.
    pub data: UnsafeCell<[u8; MIDIBUF_SIZE]>,
    /// `start`.
    pub start: Cell<usize>,
    /// `used`.
    pub used: Cell<usize>,
}

impl MidiBuffer {
    /// `MIDIBUF_START(buf)`.
    pub fn start(&self) -> usize {
        self.start.get()
    }

    /// `MIDIBUF_END(buf)`.
    pub fn end(&self) -> usize {
        (self.start.get() + self.used.get()) & MIDIBUF_MASK
    }

    /// `MIDIBUF_USED(buf)`.
    pub fn used(&self) -> usize {
        self.used.get()
    }

    /// `MIDIBUF_AVAIL(buf)`.
    pub fn avail(&self) -> usize {
        MIDIBUF_SIZE - self.used.get()
    }

    /// `MIDIBUF_ISFULL(buf)`.
    pub fn is_full(&self) -> bool {
        self.used.get() >= MIDIBUF_SIZE
    }

    /// `MIDIBUF_ISEMPTY(buf)`.
    pub fn is_empty(&self) -> bool {
        self.used.get() == 0
    }

    /// `MIDIBUF_WRITE(buf, byte)`: appends a byte; the buffer is not full.
    pub fn write(&self, byte: u8) {
        let end = self.end();
        // SAFETY: AUDIO_LOCK is held (the buffer's lock), so no other path touches the
        // bytes; `end` is masked into the array.
        unsafe { (*self.data.get())[end] = byte };
        self.used.set(self.used.get() + 1);
    }

    /// `MIDIBUF_READ(buf, byte)`: takes the first byte; the buffer is not empty.
    pub fn read(&self) -> u8 {
        let start = self.start.get();
        // SAFETY: as in `write`; `start` is always masked into the array.
        let byte = unsafe { (*self.data.get())[start] };
        self.start.set((start + 1) & MIDIBUF_MASK);
        self.used.set(self.used.get() - 1);
        byte
    }

    /// `MIDIBUF_REMOVE(buf, count)`: drops the first `count` bytes.
    pub fn remove(&self, count: usize) {
        self.start.set((self.start.get() + count) & MIDIBUF_MASK);
        self.used.set(self.used.get() - count);
    }

    /// `MIDIBUF_INIT(buf)`.
    pub fn init(&self) {
        self.start.set(0);
        self.used.set(0);
    }

    /// `buf->data[at]`.
    pub fn byte(&self, at: usize) -> u8 {
        // SAFETY: as in `read`.
        unsafe { (*self.data.get())[at & MIDIBUF_MASK] }
    }

    /// The bytes `[at, at + count)` of `data`, for uiomove(9).
    ///
    /// # Safety
    ///
    /// The caller owns these bytes for the borrow under the ring protocol: the reader the
    /// used part it is about to remove, the writer the free part it is about to add, the
    /// lock dropped only around the copy, as in the C.
    #[allow(clippy::mut_from_ref)] // the ring's bytes, owned by the ring protocol
    pub unsafe fn bytes(&self, at: usize, count: usize) -> &mut [u8] {
        assert!(
            at + count <= MIDIBUF_SIZE,
            "midi_buffer: {at}+{count} past the buffer"
        );
        // SAFETY: inside the array (checked above); exclusive by the caller's contract.
        unsafe { &mut (&mut *self.data.get())[at..at + count] }
    }
}

/// `struct midi_softc`. Allocated zeroed by autoconf, so every member is valid as zero.
#[repr(C)]
pub struct MidiSoftc {
    /// `dev`.
    pub dev: Device,
    /// `hw_if`.
    pub hw_if: Cell<Option<&'static MidiHwIf>>,
    /// `hw_hdl`.
    pub hw_hdl: Cell<*mut c_void>,
    /// `isbusy`: concerns only the output.
    pub isbusy: Cell<i32>,
    /// `flags`: open flags.
    pub flags: Cell<i32>,
    /// `props`: midi hw proprieties.
    pub props: Cell<i32>,
    /// `timeo`.
    pub timeo: Timeout,
    /// `inbuf`.
    pub inbuf: MidiBuffer,
    /// `outbuf`.
    pub outbuf: MidiBuffer,
}

// SAFETY: `#[repr(C)]` with the device first; the other members are `Cell`s of integers,
// pointers and `Option`s of references, a byte array, the klists and the timeout, all
// valid as zero bits (as `AudioSoftc`'s are).
unsafe impl Softc for MidiSoftc {}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    fn buf() -> std::boxed::Box<MidiBuffer> {
        std::boxed::Box::new(MidiBuffer {
            klist: Klist::new(),
            blocking: Cell::new(0),
            data: UnsafeCell::new([0; MIDIBUF_SIZE]),
            start: Cell::new(0),
            used: Cell::new(0),
        })
    }

    #[test]
    fn ring_wraps() {
        let b = buf();
        b.start.set(MIDIBUF_SIZE - 1);
        assert!(b.is_empty());
        b.write(1);
        b.write(2);
        assert_eq!(b.end(), 1);
        assert_eq!(b.used(), 2);
        assert_eq!(b.avail(), MIDIBUF_SIZE - 2);
        assert_eq!(b.read(), 1);
        assert_eq!(b.start(), 0);
        assert_eq!(b.byte(b.start()), 2);
        b.remove(1);
        assert!(b.is_empty());
        b.init();
        assert_eq!((b.start(), b.used()), (0, 0));
        for i in 0..MIDIBUF_SIZE {
            b.write(i as u8);
        }
        assert!(b.is_full());
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/midivar.h");
        let ours = crate::reftest::assert_defines!(defs; MIDI_RATE);
        crate::reftest::assert_complete(&defs, "MIDI_RATE", &ours);
    }
}
/* </TESTS> */
