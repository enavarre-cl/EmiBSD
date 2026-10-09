/* <LICENSES> */
/*
 * Copyright (c) 1991, 1993
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
 *	@(#)pmap.h	8.1 (Berkeley) 6/11/93
 *
 *
 * Copyright (c) 1987, 1990 Carnegie-Mellon University.
 * All rights reserved.
 *
 * Author: Avadis Tevanian, Jr.
 *
 * Permission to use, copy, modify and distribute this software and
 * its documentation is hereby granted, provided that both the copyright
 * notice and this permission notice appear in all copies of the
 * software, derivative works or modified versions, and any portions
 * thereof, and that both notices appear in supporting documentation.
 *
 * CARNEGIE MELLON ALLOWS FREE USE OF THIS SOFTWARE IN ITS "AS IS"
 * CONDITION.  CARNEGIE MELLON DISCLAIMS ANY LIABILITY OF ANY KIND
 * FOR ANY DAMAGES WHATSOEVER RESULTING FROM THE USE OF THIS SOFTWARE.
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
/* </LICENSES> */

/* <CODE> */
//! Machine address mapping definitions, the machine-independent section: `<uvm/uvm_pmap.h>`.
//! The machine-dependent section is the `machine::Pmap` contract (`sys/machine/pmap.rs`).
//!
//! Upstream: sys/uvm/uvm_pmap.h @ 3ce1f3f79392
//!
//! Status: `wip`. The prototypes it declares are the methods of `machine::Pmap`, added as the
//! milestones need them.
//!
//! ## Deviations
//! - The `pmap_statistics` counters are [`PmapCounter`]s, relaxed atomic words: a user
//!   pmap's are changed under its lock, but the kernel pmap's are changed by every CPU without
//!   one (the C's plain `long`s race there; a racing `Cell` would be undefined behaviour in
//!   Rust). Loads and stores stay separate, so the counts are as approximate as the C's.

use core::sync::atomic::{AtomicI64, Ordering};

use crate::uvm::uvm_extern::Voff;

/// A `long` of `struct pmap_statistics` (see the module's deviations).
pub struct PmapCounter(AtomicI64);

impl PmapCounter {
    /// A counter at `v`.
    pub const fn new(v: i64) -> Self {
        Self(AtomicI64::new(v))
    }

    /// Reads the counter.
    pub fn get(&self) -> i64 {
        self.0.load(Ordering::Relaxed)
    }

    /// Writes the counter.
    pub fn set(&self, v: i64) {
        self.0.store(v, Ordering::Relaxed);
    }
}

/// `struct pmap_statistics`.
pub struct PmapStatistics {
    /// Number of pages mapped (total).
    pub resident_count: PmapCounter,
    /// Number of pages wired.
    pub wired_count: PmapCounter,
}

impl PmapStatistics {
    /// Zeroed statistics.
    pub const fn new() -> Self {
        Self {
            resident_count: PmapCounter::new(0),
            wired_count: PmapCounter::new(0),
        }
    }
}

impl Default for PmapStatistics {
    fn default() -> Self {
        Self::new()
    }
}

/// Wired mapping.
pub const PMAP_WIRED: i32 = 0x0000_0010;
/// Can fail if resource shortage.
pub const PMAP_CANFAIL: i32 = 0x0000_0020;
/// Machine dependant.
pub const PMAP_MD0: i32 = 0x0000_0040;
/// Machine dependant.
pub const PMAP_MD1: i32 = 0x0000_0080;
/// Machine dependant.
pub const PMAP_MD2: i32 = 0x0000_0100;
/// Machine dependant.
pub const PMAP_MD3: i32 = 0x0000_0200;

/// `PMAP_PREFER_ALIGN()`: the alignment the pmap prefers for a mapping; 0 where `PMAP_PREFER`
/// is not defined (amd64, arm64).
#[inline]
pub const fn pmap_prefer_align() -> usize {
    0
}

/// `PMAP_PREFER_OFFSET(off)`: the offset inside that alignment; 0 where `PMAP_PREFER` is not
/// defined (amd64, arm64).
#[inline]
pub const fn pmap_prefer_offset(_off: Voff) -> usize {
    0
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statistics_start_at_zero_and_keep_what_is_stored() {
        let st = PmapStatistics::new();
        assert_eq!((st.resident_count.get(), st.wired_count.get()), (0, 0));
        st.resident_count.set(st.resident_count.get() + 3);
        st.wired_count.set(-1);
        assert_eq!((st.resident_count.get(), st.wired_count.get()), (3, -1));
    }
}
/* </TESTS> */
