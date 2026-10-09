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
 * Copyright (c) 2011 Theo de Raadt <deraadt@openbsd.org>
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
//! Space allocation and freeing routines for use by zlib routines in the kernel.
//!
//! Upstream: sys/lib/libz/zopenbsd.c @ 3ce1f3f79392
//! LZ: sys/lib/libz/zopenbsd.rs@f5985f1d055a
//!
//! The C's `zcalloc` is `mallocarray(items, size, M_DEVBUF, M_NOWAIT)`: it may fail, and zlib
//! turns the failure into `Z_MEM_ERROR`. The Rust functions keep that: they allocate through
//! the global allocator (malloc(9) in the kernel, `sys/kern/rust_alloc.rs`) and return `None`
//! instead of aborting when it has no memory. They also stand for zutil.h's `ZALLOC`, `ZFREE`
//! and `TRY_FREE`, which call `zcalloc`/`zcfree` through the stream.
//!
//! ## Deviations
//! - `zcalloc` returns a typed, zero-filled `Vec<T>` of `items` elements (the C returns
//!   uninitialised memory; zlib zeroes what it reads before writing it, so zeroing is only
//!   safer); [`zcalloc_box`] allocates one value, for the stream states. `M_DEVBUF` has no
//!   counterpart: the global allocator has one malloc type.
//! - `zcfree` drops what `zcalloc` returned; the size the C passes to free(9) is the `Vec`'s.
//!
//! ## Redesign
//! - LZ's `zcalloc_box` allocated with `alloc::alloc::alloc`, wrote the value through the raw
//!   pointer and rebuilt a `Box` with `Box::from_raw` (two `unsafe` blocks), because
//!   `Box::try_new` is not stable. It now reserves a one-element `Vec` with
//!   `try_reserve_exact` (the fallible step, `None` on failure as before), pushes the value
//!   and turns the vector into a `Box<[T; 1]>` in place (its capacity is exactly one, so the
//!   conversion never reallocates). [`ZBox`] wraps that box and dereferences to the one
//!   value, so the stream states keep the shape of a `Box<T>`. No `unsafe` is left.

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::ops::{Deref, DerefMut};

/// One value on the heap, allocated fallibly by [`zcalloc_box`]: what `ZALLOC` returns for a
/// stream state. It dereferences to the value and frees it when dropped.
pub struct ZBox<T>(Box<[T; 1]>);

impl<T> Deref for ZBox<T> {
    type Target = T;

    fn deref(&self) -> &T {
        let [value] = &*self.0;
        value
    }
}

impl<T> DerefMut for ZBox<T> {
    fn deref_mut(&mut self) -> &mut T {
        let [value] = &mut *self.0;
        value
    }
}

/// `zcalloc`: `items` elements of `T`, zero-filled (`T::default()`), or `None` when the
/// allocator has no memory (the C's `M_NOWAIT` returning `NULL`).
pub fn zcalloc<T: Copy + Default>(items: usize) -> Option<Vec<T>> {
    let mut v = Vec::new();
    v.try_reserve_exact(items).ok()?;
    v.resize(items, T::default());
    Some(v)
}

/// `ZALLOC` of one object: `value` moved to the heap, or `None` when the allocator has no
/// memory.
pub fn zcalloc_box<T>(value: T) -> Option<ZBox<T>> {
    let mut v = Vec::new();
    v.try_reserve_exact(1).ok()?;
    v.push(value);
    // The capacity is one (zero-sized values have no allocation at all), so the conversion
    // reuses the allocation; it cannot fail with exactly one element.
    Box::<[T; 1]>::try_from(v).ok().map(ZBox)
}

/// `zcfree`: free what [`zcalloc`] or [`zcalloc_box`] returned.
pub fn zcfree<T>(ptr: T) {
    drop(ptr);
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zcalloc_is_zero_filled() {
        let v = zcalloc::<u16>(1000).unwrap();
        assert_eq!(v.len(), 1000);
        assert!(v.iter().all(|&x| x == 0));
        zcfree(v);
    }

    #[test]
    fn zcalloc_box_holds_the_value() {
        let mut b = zcalloc_box([7u64; 64]).unwrap();
        assert!(b.iter().all(|&x| x == 7));
        b[3] = 9;
        assert_eq!(b[3], 9);
        assert_eq!(*zcalloc_box(()).unwrap(), ());
        zcfree(b);
    }

    #[test]
    fn zcalloc_box_owns_and_drops_its_value() {
        use std::rc::Rc;
        let shared = Rc::new(5u8);
        let b = zcalloc_box(Rc::clone(&shared)).unwrap();
        assert_eq!(Rc::strong_count(&shared), 2);
        assert_eq!(**b, 5);
        zcfree(b);
        assert_eq!(Rc::strong_count(&shared), 1);
    }

    #[test]
    fn zcalloc_fails_instead_of_aborting() {
        assert!(zcalloc::<u64>(usize::MAX / 4).is_none());
    }
}
/* </TESTS> */
