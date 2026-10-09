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
//! `<machine/atomic.h>` as a trait: the memory barriers machine-independent code names.
//!
//! The atomic operations themselves are `core::sync::atomic` (`docs/C_TO_RUST.md`); what the
//! header adds that `core` cannot spell are the barriers whose strength is the machine's
//! choice. `virtio(4)` needs barriers that order its accesses to the rings against the
//! device even on a uniprocessor kernel ("virtio needs MP membars even on SP kernels", as
//! the arm64 header says): amd64 makes the producer and consumer barriers compiler barriers
//! and the full one an `mfence`, arm64 uses `dmb st`, `dmb ld` and `dmb sy` (the full system,
//! which `core`'s fences, `dmb ish*`, do not cover).

use crate::machine::Machine;

/// The machine's barriers.
pub trait Atomic {
    /// `virtio_membar_producer()`: orders earlier stores before later stores, as the device
    /// sees them.
    fn virtio_membar_producer();
    /// `virtio_membar_consumer()`: orders earlier loads before later loads.
    fn virtio_membar_consumer();
    /// `virtio_membar_sync()`: orders every earlier access before every later one.
    fn virtio_membar_sync();
}

/// `virtio_membar_producer()` on the selected machine.
#[inline]
pub fn virtio_membar_producer() {
    Machine::virtio_membar_producer()
}

/// `virtio_membar_consumer()` on the selected machine.
#[inline]
pub fn virtio_membar_consumer() {
    Machine::virtio_membar_consumer()
}

/// `virtio_membar_sync()` on the selected machine.
#[inline]
pub fn virtio_membar_sync() {
    Machine::virtio_membar_sync()
}
/* </CODE> */
