/* $OpenBSD: refcnt.h,v 1.10 2024/05/13 01:15:53 jsg Exp $ */
/* <LICENSES> */
/*
 * Copyright (c) 2015 David Gwynne <dlg@openbsd.org>
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
//! `<sys/refcnt.h>`: reference counts. The functions live in `kern/kern_synch.rs`.
//!
//! Upstream: sys/sys/refcnt.h @ 3ce1f3f79392
//!
//! Status: `ported` (M5).

use core::cell::Cell;
use core::sync::atomic::AtomicU32;

/// `struct refcnt`.
///
/// Locks used to protect struct members in this file:
/// - I: immutable after creation
/// - a: atomic operations
pub struct Refcnt {
    /// \[a\] `r_refs`: reference counter.
    pub r_refs: AtomicU32,
    /// \[I\] `r_traceidx`: index for dt(4) tracing.
    pub r_traceidx: Cell<i32>,
}

// SAFETY: the count is atomic; the trace index is written once at init.
unsafe impl Sync for Refcnt {}

impl Refcnt {
    /// `REFCNT_INITIALIZER()`: one reference, no tracing.
    pub const fn new() -> Self {
        Self {
            r_refs: AtomicU32::new(1),
            r_traceidx: Cell::new(0),
        }
    }
}

impl Default for Refcnt {
    fn default() -> Self {
        Self::new()
    }
}

/* sorted alphabetically, keep in sync with dev/dt/dt_prov_static.c */

/// `DT_REFCNT_IDX_ETHMULTI`.
pub const DT_REFCNT_IDX_ETHMULTI: i32 = 1;
/// `DT_REFCNT_IDX_IFADDR`.
pub const DT_REFCNT_IDX_IFADDR: i32 = 2;
/// `DT_REFCNT_IDX_IFMADDR`.
pub const DT_REFCNT_IDX_IFMADDR: i32 = 3;
/// `DT_REFCNT_IDX_INPCB`.
pub const DT_REFCNT_IDX_INPCB: i32 = 4;
/// `DT_REFCNT_IDX_RTENTRY`.
pub const DT_REFCNT_IDX_RTENTRY: i32 = 5;
/// `DT_REFCNT_IDX_SOCKET`.
pub const DT_REFCNT_IDX_SOCKET: i32 = 6;
/// `DT_REFCNT_IDX_SYNCACHE`.
pub const DT_REFCNT_IDX_SYNCACHE: i32 = 7;
/// `DT_REFCNT_IDX_TDB`.
pub const DT_REFCNT_IDX_TDB: i32 = 8;
/* </CODE> */
