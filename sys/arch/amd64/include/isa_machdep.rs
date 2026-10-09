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

/*-
 * Copyright (c) 1996, 1997, 1998 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Jason R. Thorpe of the Numerical Aerospace Simulation Facility,
 * NASA Ames Research Center.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 *
 * THIS SOFTWARE IS PROVIDED BY THE NETBSD FOUNDATION, INC. AND CONTRIBUTORS
 * ``AS IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
 * TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
 * PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL THE FOUNDATION OR CONTRIBUTORS
 * BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
 * CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
 * SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
 * INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
 * CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
 * ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
 * POSSIBILITY OF SUCH DAMAGE.
 */

/*-
 * Copyright (c) 1990 The Regents of the University of California.
 * All rights reserved.
 *
 * This code is derived from software contributed to Berkeley by
 * William Jolitz.
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
 *	@(#)isa.h	5.7 (Berkeley) 5/9/91
 */
/* </LICENSES> */

/* <CODE> */
//! amd64 `<machine/isa_machdep.h>`: the ISA DMA bounce-buffer cookie and its limit.
//!
//! Upstream: sys/arch/amd64/include/isa_machdep.h @ 3ce1f3f79392
//!
//! Status: `wip`. M16a ports what the ISA DMA tag (`isa_bus_dma_tag`, `_isa_bus_dma*` in
//! `arch/amd64/isa/isa_machdep.rs`) needs: `ISA_DMA_BOUNCE_THRESHOLD`, `struct
//! isa_dma_cookie` and its `ID_*` flags. `isa_chipset_tag_t` and the prototypes of the
//! interrupt functions are the `.c` port's and the machine contract's
//! (`machine::isa_machdep`), as M4 left them.
//!
//! ## Deviations
//! - `atdevbase`, `ISA_PHYSADDR` and `ISA_HOLE_VADDR` are not ported: the ISA hole is the
//!   direct map's (`bus_space.rs`, `locore0.S`), and no ported driver converts hole
//!   addresses.
//! - `struct isa_dma_cookie` ends with the zero-length `id_bouncesegs[0]`: here the header
//!   is followed in the same `malloc(9)` allocation by the map's `_dm_segcnt` segments,
//!   which [`IsaDmaCookie::id_bouncesegs`] reaches (a Rust struct cannot end in an unsized
//!   array). The members the load, unload and sync functions change are `Cell`s: the map's
//!   functions reach the cookie through the shared map.

use core::cell::Cell;
use core::mem::size_of;
use core::slice;

use crate::arch::amd64::include::bus::BusDmaSegment;
use crate::machine::bus::BusSize;

/// `ISA_DMA_BOUNCE_THRESHOLD`: ISA can only DMA to 0-16M.
pub const ISA_DMA_BOUNCE_THRESHOLD: usize = 0x00ff_ffff;

/// `ID_MIGHT_NEED_BOUNCE`: map could need bounce buffers.
pub const ID_MIGHT_NEED_BOUNCE: i32 = 0x01;
/// `ID_HAS_BOUNCE`: map currently has bounce buffers.
pub const ID_HAS_BOUNCE: i32 = 0x02;
/// `ID_IS_BOUNCING`: map is bouncing current xfer.
pub const ID_IS_BOUNCING: i32 = 0x04;

/// `struct isa_dma_cookie`: used by ISA dma. A pointer to one of these is stashed in the DMA
/// map.
#[repr(C)]
pub struct IsaDmaCookie {
    /// `id_flags`: flags; see `ID_*`.
    pub id_flags: Cell<i32>,

    /// `id_origbuf`: pointer to orig buffer if bouncing.
    pub id_origbuf: Cell<*mut u8>,
    /// `id_origbuflen`: ...and size.
    pub id_origbuflen: Cell<BusSize>,

    /// `id_bouncebuf`: pointer to the bounce buffer.
    pub id_bouncebuf: Cell<*mut u8>,
    /// `id_bouncebuflen`: ...and size.
    pub id_bouncebuflen: Cell<BusSize>,
    /// `id_nbouncesegs`: number of valid bounce segs.
    pub id_nbouncesegs: Cell<i32>,
    // id_bouncesegs[0]: array of bounce buffer physical memory segments, after the header.
}

impl IsaDmaCookie {
    /// `cookie->id_bouncesegs`: the `n` segments after the header.
    ///
    /// # Safety
    ///
    /// The cookie was allocated with room for `n` segments after it (`_isa_bus_dmamap_create`
    /// with `ID_MIGHT_NEED_BOUNCE`), zeroed, and no other reference to them is live: the
    /// map's functions run for the one driver that owns the map.
    #[allow(clippy::mut_from_ref)] // the C's trailing array, written through the map
    pub unsafe fn id_bouncesegs(&self, n: usize) -> &mut [BusDmaSegment] {
        // SAFETY: the caller's guarantee: `n` zeroed (valid) segments follow the header, in
        // the cookie's own allocation, aligned (the header's size is a multiple of 8).
        unsafe {
            let p = core::ptr::from_ref(self)
                .cast::<u8>()
                .add(size_of::<IsaDmaCookie>())
                .cast_mut()
                .cast::<BusDmaSegment>();
            slice::from_raw_parts_mut(p, n)
        }
    }
}

const _: () = assert!(size_of::<IsaDmaCookie>().is_multiple_of(align_of::<BusDmaSegment>()));
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/arch/amd64/include/isa_machdep.h");
        let int = |n| crate::reftest::int(&defs, n);
        assert_eq!(
            int("ISA_DMA_BOUNCE_THRESHOLD"),
            Some(ISA_DMA_BOUNCE_THRESHOLD as i64)
        );
        assert_eq!(
            int("ID_MIGHT_NEED_BOUNCE"),
            Some(ID_MIGHT_NEED_BOUNCE.into())
        );
        assert_eq!(int("ID_HAS_BOUNCE"), Some(ID_HAS_BOUNCE.into()));
        assert_eq!(int("ID_IS_BOUNCING"), Some(ID_IS_BOUNCING.into()));
    }
}
/* </TESTS> */
