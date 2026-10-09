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
/* </LICENSES> */

/* <CODE> */
//! `StaticCell<T>`: a `static` whose contents the kernel mutates in place.
//!
//! Not an OpenBSD file. C spells these `static struct foo bar;` and mutates them under a lock or
//! during single-threaded boot; Rust would spell that `static mut`, which this workspace forbids
//! (`.claude/rules/rust-kernel.md`). Every access is `unsafe` so that each site states which
//! lock or boot phase excludes other references. Atomics and the ported `Mutex<T>` are preferred
//! whenever the value fits them; this is for the rest: hardware handles, buffers, function tables.

use core::cell::UnsafeCell;

/// A `static` with interior mutability and no runtime checks.
pub struct StaticCell<T>(UnsafeCell<T>);

// SAFETY: a `static` is shared by definition; the `unsafe` accessors below make every site name
// the invariant (a lock, or the single boot CPU before interrupts exist) that excludes aliasing
// writers, which is what makes the sharing sound.
unsafe impl<T: Send> Sync for StaticCell<T> {}

impl<T> StaticCell<T> {
    /// A cell holding `value`.
    pub const fn new(value: T) -> Self {
        Self(UnsafeCell::new(value))
    }

    /// Raw pointer to the contents; never dereferenced here.
    pub const fn as_ptr(&self) -> *mut T {
        self.0.get()
    }

    /// Shared access.
    ///
    /// # Safety
    ///
    /// No `&mut T` to the contents may be live while the returned reference is.
    pub unsafe fn get(&self) -> &T {
        // SAFETY: the caller excludes mutable aliases for the lifetime of the reference.
        unsafe { &*self.0.get() }
    }

    /// Exclusive access.
    ///
    /// # Safety
    ///
    /// No other reference to the contents may be live while the returned one is.
    #[allow(clippy::mut_from_ref)] // the cell is the static; the caller owns the phase or the lock
    pub unsafe fn get_mut(&self) -> &mut T {
        // SAFETY: the caller excludes every other reference for the lifetime of this one.
        unsafe { &mut *self.0.get() }
    }

    /// Copies the contents out.
    ///
    /// # Safety
    ///
    /// As for [`get`](Self::get).
    pub unsafe fn read(&self) -> T
    where
        T: Copy,
    {
        // SAFETY: as for `get`; the copy ends before any writer can run.
        unsafe { *self.0.get() }
    }

    /// Replaces the contents.
    ///
    /// # Safety
    ///
    /// As for [`get_mut`](Self::get_mut).
    pub unsafe fn write(&self, value: T) {
        // SAFETY: as for `get_mut`; no reference outlives this store.
        unsafe { *self.0.get() = value }
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    static CELL: StaticCell<[u8; 4]> = StaticCell::new([0; 4]);

    #[test]
    fn read_write_round_trip() {
        // SAFETY: this test is the only user of CELL and runs single-threaded.
        unsafe {
            CELL.write([1, 2, 3, 4]);
            assert_eq!(CELL.read(), [1, 2, 3, 4]);
            CELL.get_mut()[0] = 9;
            assert_eq!(CELL.get()[0], 9);
        }
        assert_eq!(CELL.as_ptr() as usize % core::mem::align_of::<[u8; 4]>(), 0);
    }
}
/* </TESTS> */
