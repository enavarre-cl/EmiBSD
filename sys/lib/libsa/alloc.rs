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
 * Copyright (c) 1997 Christopher G. Demetriou.  All rights reserved.
 * Copyright (c) 1996
 *	Matthias Drochner.  All rights reserved.
 * Copyright (c) 1993
 *	The Regents of the University of California.  All rights reserved.
 *
 * This code is derived from software contributed to Berkeley by
 * The Mach Operating System project at Carnegie-Mellon University.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. Neither the name of the University nor the names of its contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE REGENTS AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE REGENTS OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 *	@(#)alloc.c	8.1 (Berkeley) 6/11/93
 *
 *
 * Copyright (c) 1989, 1990, 1991 Carnegie Mellon University
 * All Rights Reserved.
 *
 * Author: Alessandro Forin
 *
 * Permission to use, copy, modify and distribute this software and its
 * documentation is hereby granted, provided that both the copyright
 * notice and this permission notice appear in all copies of the
 * software, derivative works or modified versions, and any portions
 * thereof, and that both notices appear in supporting documentation.
 *
 * CARNEGIE MELLON ALLOWS FREE USE OF THIS SOFTWARE IN ITS "AS IS"
 * CONDITION.  CARNEGIE MELLON DISCLAIMS ANY LIABILITY OF ANY KIND FOR
 * ANY DAMAGES WHATSOEVER RESULTING FROM THE USE OF THIS SOFTWARE.
 *
 * Carnegie Mellon requests users of this software to return to
 *
 *  Software Distribution Coordinator  or  Software.Distribution@CS.CMU.EDU
 *  School of Computer Science
 *  Carnegie Mellon University
 *  Pittsburgh PA 15213-3890
 *
 * any improvements or extensions that they make and grant Carnegie the
 * rights to redistribute these changes.
 */

/*
 * Dynamic memory allocator.
 *
 * Compile options:
 *
 *	ALLOC_TRACE	enable tracing of allocations/deallocations
 *
 *	ALLOC_FIRST_FIT	use a first-fit allocation algorithm, rather than
 *			the default best-fit algorithm.
 *
 *	HEAP_LIMIT	heap limit address (defaults to "no limit").
 *
 *	HEAP_START	start address of heap (defaults to '&end').
 *
 *	NEEDS_HEAP_H	needs to #include "heap.h" to declare things
 *			needed by HEAP_LIMIT and/or HEAP_START.
 *
 *	NEEDS_HEAP_INIT	needs to invoke heap_init() to initialize
 *			heap boundaries.
 *
 *	DEBUG		enable debugging sanity checks.
 */
/* </LICENSES> */

/* <CODE> */
//! The standalone dynamic memory allocator: a best-fit free list over a heap that only grows.
//!
//! Upstream: sys/lib/libsa/alloc.c @ 3ce1f3f79392
//!
//! Each block has `ALIGN(sizeof(unsigned)) + ALIGN(size)` bytes: the first word holds the
//! size of the user-data part; on the free list the block is a `struct fl` (size, then the
//! next free block). `free()` puts a block back on the list without merging; `alloc()` takes
//! an exact fit at once, otherwise the best fit unless that is twice the size or more, and
//! otherwise carves a new block from the top of the heap, panicking past `HEAP_LIMIT`.
//! Compiled as efiboot compiles it: best fit (no `ALLOC_FIRST_FIT`), with `NEEDS_HEAP_H`,
//! `NEEDS_HEAP_INIT` and `HEAP_LIMIT`, no `ALLOC_TRACE`, no `DEBUG`.
//!
//! ## Deviations
//! - The free list and `top` are a [`Heap`] value; the program's one is [`HEAP`], set up by
//!   [`heap_init`], which the program calls with its heap's start and `HEAP_LIMIT` (the C's
//!   `heap.h` hook reads both from globals). Tests run their own `Heap` on a buffer.
//! - Rust's allocations (`Box`, `Vec`: the file systems' buffers, zlib's state) come here
//!   too, through [`SaAlloc`], the `GlobalAlloc` the program installs; requests aligned to
//!   more than `ALIGN` (8 bytes) are padded and the block's start is kept in front of them.

use core::alloc::{GlobalAlloc, Layout};
use core::ptr;

use libkern::staticcell::StaticCell;

/// `ALIGN(p)`: `_ALIGNBYTES` is `sizeof(long) - 1` on amd64 and arm64.
const fn align(n: usize) -> usize {
    (n + 7) & !7
}

/// `ALIGN(sizeof(unsigned))`: the header in front of the user data.
const HDR: usize = align(core::mem::size_of::<u32>());

/// `struct fl`: a block on the free list.
#[repr(C)]
struct Fl {
    size: u32,
    next: *mut Fl,
}

/// The allocator's state: `freelist` and `top` (and the `HEAP_LIMIT` it checks).
pub struct Heap {
    freelist: *mut Fl,
    top: usize,
    limit: usize,
}

// SAFETY: the heap's memory is owned by the one standalone program; it has no threads.
unsafe impl Send for Heap {}

impl Heap {
    /// A heap of the memory from `start` up to `limit` (exclusive), all unused.
    pub const fn new(start: usize, limit: usize) -> Self {
        Self {
            freelist: ptr::null_mut(),
            top: start,
            limit,
        }
    }

    /// `alloc(size)`: a block of at least `size` bytes, aligned to 8.
    ///
    /// # Safety
    ///
    /// The heap's memory, from its start to its limit, must be writable and used by nothing
    /// else, and every block on the free list must have come from this heap.
    pub unsafe fn alloc(&mut self, size: u32) -> *mut u8 {
        let mut size = size;
        let mut f: *mut *mut Fl = &raw mut self.freelist;
        let mut bestf: *mut *mut Fl = ptr::null_mut();
        let mut bestsize: u32 = 0xffff_ffff; // greater than any real size

        // SAFETY: the free list links blocks of this heap (the caller's contract), each a
        // valid `Fl` written by `free`.
        unsafe {
            // scan freelist
            while !(*f).is_null() {
                if (**f).size >= size {
                    if (**f).size == size {
                        // exact match
                        return self.take(f);
                    }
                    if (**f).size < bestsize {
                        // keep best fit
                        bestf = f;
                        bestsize = (**f).size;
                    }
                }
                f = &raw mut (**f).next;
            }
        }

        // no match in freelist if bestsize unchanged
        let failed = bestsize == 0xffff_ffff || u64::from(bestsize) >= u64::from(size) * 2;

        if failed {
            // allocate from heap, keep chunk len in first word
            let help = self.top;

            // make _sure_ the region can hold a struct fl.
            let min = align(core::mem::size_of::<*mut Fl>()) as u32;
            if size < min {
                size = min;
            }
            self.top += HDR + align(size as usize);
            if self.top > self.limit {
                crate::exit::panic(format_args!("heap full ({:#x}+{})", help, size));
            }
            // SAFETY: `help..top` is heap memory below the limit (checked above) that no
            // block uses yet; its first word is the size.
            unsafe { (help as *mut u32).write(align(size as usize) as u32) };
            return (help + HDR) as *mut u8;
        }

        // we take the best fit
        // SAFETY: `bestf` points at a link of the free list (found above).
        unsafe { self.take(bestf) }
    }

    /// Removes the block `*f` from the free list and returns its user data.
    ///
    /// # Safety
    ///
    /// `f` points at a link of this heap's free list whose block is not null.
    unsafe fn take(&mut self, f: *mut *mut Fl) -> *mut u8 {
        // SAFETY: the caller's contract: `*f` is a free block of this heap.
        unsafe {
            let help = *f;
            *f = (*help).next;
            help.cast::<u8>().add(HDR)
        }
    }

    /// `free(ptr, size)`: put a block back on the free list (`size` is not used: the block
    /// keeps its own).
    ///
    /// # Safety
    ///
    /// `p` is null or a block `alloc` of this heap returned and that has not been freed
    /// since.
    pub unsafe fn free(&mut self, p: *mut u8, _size: u32) {
        if p.is_null() {
            return;
        }
        // SAFETY: `p` came from `alloc`, so `HDR` bytes before it are its block's header, and
        // the block (at least `HDR + 8` bytes) can hold an `Fl`.
        unsafe {
            let f = p.sub(HDR).cast::<Fl>();
            // put into freelist
            (*f).next = self.freelist;
            self.freelist = f;
        }
    }
}

/// The program's heap (`freelist`, `top`), empty until [`heap_init`].
pub static HEAP: StaticCell<Heap> = StaticCell::new(Heap::new(0, 0));

/// `heap_init()` of the program's `heap.h`: the heap starts at `start`; `top` may not pass
/// `limit` (`HEAP_LIMIT`).
///
/// # Safety
///
/// The memory from `start` to `limit` must be the program's, unused, for as long as it
/// allocates; call it once, before the first allocation.
pub unsafe fn heap_init(start: usize, limit: usize) {
    // SAFETY: called once before any allocation (the caller's contract), so no other
    // reference to HEAP exists.
    unsafe { HEAP.write(Heap::new(start, limit)) };
}

/// `alloc(size)` on the program's heap.
pub fn alloc(size: u32) -> *mut u8 {
    // SAFETY: the standalone program is single-threaded and the allocator does not reenter
    // itself, so this is the only reference to HEAP; `heap_init` handed it its memory.
    unsafe { HEAP.get_mut().alloc(size) }
}

/// `free(ptr, size)` on the program's heap.
///
/// # Safety
///
/// `p` is null or a block [`alloc`] returned and that has not been freed since.
pub unsafe fn free(p: *mut u8, size: u32) {
    // SAFETY: as in `alloc`; the caller vouches for `p`.
    unsafe { HEAP.get_mut().free(p, size) }
}

/// The `GlobalAlloc` over [`alloc`]/[`free`] that a standalone program installs with
/// `#[global_allocator]`.
pub struct SaAlloc;

// SAFETY: blocks come from `alloc`, which never returns memory in use; alignments up to 8
// are the heap's own, larger ones are made by padding (the padded block's start is stored in
// the word before the pointer handed out, which `dealloc` reads back).
unsafe impl GlobalAlloc for SaAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let Ok(size) = u32::try_from(layout.size()) else {
            return ptr::null_mut();
        };
        if layout.align() <= 8 {
            return alloc(size);
        }
        let Some(padded) = size.checked_add(layout.align() as u32 + 8) else {
            return ptr::null_mut();
        };
        let base = alloc(padded);
        let user = (base as usize + 8).next_multiple_of(layout.align());
        // SAFETY: `user - 8 >= base`, and `user + size <= base + padded`: both words are in
        // the block just allocated.
        unsafe { (user as *mut usize).sub(1).write(base as usize) };
        user as *mut u8
    }

    unsafe fn dealloc(&self, p: *mut u8, layout: Layout) {
        let size = layout.size() as u32;
        if layout.align() <= 8 {
            // SAFETY: `p` came from `alloc` above with the same layout.
            unsafe { free(p, size) };
            return;
        }
        // SAFETY: `alloc` stored the block's start in the word before `p`.
        unsafe {
            let base = p.cast::<usize>().sub(1).read() as *mut u8;
            free(base, size);
        }
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;
    use std::vec;

    #[test]
    fn best_fit_and_reuse() {
        let mut mem = vec![0u64; 1024];
        let start = mem.as_mut_ptr() as usize;
        let mut h = Heap::new(start, start + 8 * 1024);
        // SAFETY: the heap is `mem`, which nothing else uses while `h` lives.
        unsafe {
            let a = h.alloc(100);
            let b = h.alloc(40);
            let c = h.alloc(3);
            assert_eq!(a as usize, start + 8);
            assert_eq!(b as usize, a as usize + 104 + 8);
            assert_eq!(c as usize, b as usize + 40 + 8);
            h.free(a, 100);
            h.free(b, 40);
            // best fit: 40 fits 30 (not twice the size), 104 would be too big a waste
            assert_eq!(h.alloc(30), b);
            // nothing fits 300: a new block from the top
            let d = h.alloc(300);
            assert_eq!(d as usize, c as usize + 8 + 8);
            // the exact size comes back at once
            assert_eq!(h.alloc(104), a);
        }
    }
}
/* </TESTS> */
