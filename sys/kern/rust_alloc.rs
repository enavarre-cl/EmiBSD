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
//! The Rust global allocator over `malloc(9)`/`free(9)`: a project helper, not OpenBSD code.
//!
//! With it, `alloc::vec::Vec`, `alloc::boxed::Box` and friends work in kernel code: every
//! allocation becomes a `malloc(size, M_TEMP, M_NOWAIT)` in `kern/kern_malloc.rs` (nothing can
//! sleep yet) and every release a `free`. The size handed to `malloc` is raised to the
//! layout's alignment, so the bucket's natural alignment (a power of two, chunks carved
//! end-first out of page-aligned blocks) satisfies it; an alignment above `PAGE_SIZE` cannot be
//! met that way and is refused (the allocation fails, which Rust turns into
//! `handle_alloc_error`). `free` gets the same adjusted size, so `DIAGNOSTIC`'s size checks
//! hold.

use core::alloc::{GlobalAlloc, Layout};
use core::ptr::{self, NonNull};

use crate::kern::kern_malloc::{free, malloc};
use crate::sys::malloc::{M_NOWAIT, M_TEMP, M_ZERO};
use crate::sys::param::PAGE_SIZE;

/// The kernel's `GlobalAlloc`.
pub struct KernelAllocator;

/// What is asked of `malloc` for a layout: the size, raised to the alignment and to one.
fn request_size(layout: Layout) -> usize {
    layout.size().max(layout.align()).max(1)
}

// SAFETY: `malloc` hands out blocks nobody else uses until `free`; the bucket chunks are
// aligned to their power-of-two size and the page blocks to the page, which covers every
// alignment up to `PAGE_SIZE`; larger alignments are refused rather than mis-served.
unsafe impl GlobalAlloc for KernelAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if layout.align() > PAGE_SIZE {
            return ptr::null_mut();
        }
        malloc(request_size(layout), M_TEMP, M_NOWAIT).map_or(ptr::null_mut(), NonNull::as_ptr)
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if layout.align() > PAGE_SIZE {
            return ptr::null_mut();
        }
        malloc(request_size(layout), M_TEMP, M_NOWAIT | M_ZERO)
            .map_or(ptr::null_mut(), NonNull::as_ptr)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if let Some(p) = NonNull::new(ptr) {
            free(p, M_TEMP, request_size(layout));
        }
    }
}

/// The kernel's allocator, on bare metal; the host double keeps `std`'s.
#[cfg(target_os = "none")]
#[global_allocator]
static GLOBAL: KernelAllocator = KernelAllocator;
/* </CODE> */
