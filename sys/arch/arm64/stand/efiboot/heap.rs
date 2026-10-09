/* <LICENSES> */
/*
 * Copyright (c) 2015 YASUOKA Masahiko <yasuoka@yasuoka.net>
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
//! `heap.h`: libsa's allocator gets efiboot's heap: it starts at the pages `efi_heap_init`
//! allocated.
//!
//! Upstream: sys/arch/arm64/stand/efiboot/heap.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - `heap_init()`, which `alloc()` calls on every call in C (`NEEDS_HEAP_INIT`) and which
//!   sets `top` the first time, is called once by `efi_heap_init`, right after the pages are
//!   allocated, and hands libsa the start and the end of the heap (`alloc.rs`'s
//!   `heap_init`).
//! - arm64's efiboot defines no `HEAP_LIMIT`, so the C's `alloc()` carves past the heap's
//!   pages unchecked; libsa's allocator always checks a limit, which is the end of the
//!   pages here (`heap + heapsiz`): running out panics ("heap full") instead of writing over
//!   memory the firmware may own.

use core::sync::atomic::Ordering;

use crate::efiboot::{HEAP, HEAPSIZ};

/// `heap_init()`.
///
/// # Safety
///
/// `HEAP` holds the address of `HEAPSIZ` bytes of pages the program allocated for the heap,
/// which nothing else uses; call it once, before the first allocation.
pub unsafe fn heap_init() {
    let heap = HEAP.load(Ordering::Relaxed) as usize;
    // SAFETY: the caller's contract, which is `sa_alloc::heap_init`'s.
    unsafe { libsa::sa_alloc::heap_init(heap, heap + HEAPSIZ as usize) };
}
/* </CODE> */
