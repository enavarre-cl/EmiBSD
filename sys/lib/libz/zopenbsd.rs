/* <LICENSES> */
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

use alloc::alloc::{Layout, alloc};
use alloc::boxed::Box;
use alloc::vec::Vec;

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
pub fn zcalloc_box<T>(value: T) -> Option<Box<T>> {
    let layout = Layout::new::<T>();
    if layout.size() == 0 {
        // A zero-sized value needs no memory; Box::new does not allocate for it.
        return Some(Box::new(value));
    }
    // SAFETY: `layout` has a non-zero size (checked above), as `alloc` requires.
    let p = unsafe { alloc(layout) }.cast::<T>();
    if p.is_null() {
        return None;
    }
    // SAFETY: `p` is non-null, was just allocated by the global allocator with
    // `Layout::new::<T>()` (so it is valid for writes of a `T` and aligned), and nothing else
    // refers to it. Writing `value` initialises it, which is what `Box::from_raw` requires of
    // memory allocated that way; the Box then owns and frees it with the same layout.
    unsafe {
        p.write(value);
        Some(Box::from_raw(p))
    }
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
        let b = zcalloc_box([7u64; 64]).unwrap();
        assert!(b.iter().all(|&x| x == 7));
        assert_eq!(*zcalloc_box(()).unwrap(), ());
        zcfree(b);
    }

    #[test]
    fn zcalloc_fails_instead_of_aborting() {
        assert!(zcalloc::<u64>(usize::MAX / 4).is_none());
    }
}
/* </TESTS> */
