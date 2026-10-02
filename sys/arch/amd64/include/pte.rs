/*	$OpenBSD: pte.h,v 1.18 2024/07/09 19:11:06 bluhm Exp $	*/
/*	$NetBSD: pte.h,v 1.1 2003/04/26 18:39:47 fvdl Exp $	*/

/*
 * Copyright (c) 2001 Wasabi Systems, Inc.
 * All rights reserved.
 *
 * Written by Frank van der Linden for Wasabi Systems, Inc.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. All advertising materials mentioning features or use of this software
 *    must display the following acknowledgement:
 *      This product includes software developed for the NetBSD Project by
 *      Wasabi Systems, Inc.
 * 4. The name of Wasabi Systems, Inc. may not be used to endorse
 *    or promote products derived from this software without specific prior
 *    written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY WASABI SYSTEMS, INC. ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
 * TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
 * PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL WASABI SYSTEMS, INC
 * BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
 * CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
 * SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
 * INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
 * CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
 * ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
 * POSSIBILITY OF SUCH DAMAGE.
 */

//! amd64 `<machine/pte.h>`: the MMU's page-table entries.
//!
//! Upstream: sys/arch/amd64/include/pte.h @ 3ce1f3f79392
//!
//! The (first generation) amd64 MMU is a 4-level MMU which maps 2^48 bytes of virtual memory.
//! The pagesize we use is 4K (4096 \[0x1000\] bytes), although 2M pages are also supported.
//!
//! Status: `wip`. Milestone M3 ports the entry types, the level geometry and the PDE/PTE
//! bits; `x86_round_pdr`, the EPT bits and `PGK_VALUE` come with the page tables and vmm.

/// `pd_entry_t`: a page directory entry (levels 2 to 4).
pub type PdEntry = u64;
/// `pt_entry_t`: a page table entry (level 1).
pub type PtEntry = u64;

// now we define various for playing with virtual addresses

/// `L1_SHIFT`.
pub const L1_SHIFT: u32 = 12;
/// `L2_SHIFT`.
pub const L2_SHIFT: u32 = 21;
/// `L3_SHIFT`.
pub const L3_SHIFT: u32 = 30;
/// `L4_SHIFT`.
pub const L4_SHIFT: u32 = 39;
/// `NBPD_L1`: # bytes mapped by L1 ent (4K).
pub const NBPD_L1: usize = 1 << L1_SHIFT;
/// `NBPD_L2`: # bytes mapped by L2 ent (2MB).
pub const NBPD_L2: usize = 1 << L2_SHIFT;
/// `NBPD_L3`: # bytes mapped by L3 ent (1G).
pub const NBPD_L3: usize = 1 << L3_SHIFT;
/// `NBPD_L4`: # bytes mapped by L4 ent (512G).
pub const NBPD_L4: usize = 1 << L4_SHIFT;

/// `L4_MASK`: the level-4 index bits of a virtual address.
pub const L4_MASK: usize = 0x0000_ff80_0000_0000;
/// `L3_MASK`.
pub const L3_MASK: usize = 0x0000_007f_c000_0000;
/// `L2_MASK`.
pub const L2_MASK: usize = 0x0000_0000_3fe0_0000;
/// `L1_MASK`.
pub const L1_MASK: usize = 0x0000_0000_001f_f000;

/// `L4_FRAME`: the address bits a level-4 entry covers.
pub const L4_FRAME: usize = L4_MASK;
/// `L3_FRAME`.
pub const L3_FRAME: usize = L4_FRAME | L3_MASK;
/// `L2_FRAME`.
pub const L2_FRAME: usize = L3_FRAME | L2_MASK;
/// `L1_FRAME`.
pub const L1_FRAME: usize = L2_FRAME | L1_MASK;

/// `PAGE_MASK_L2`: the offset bits inside a 2M page.
pub const PAGE_MASK_L2: usize = NBPD_L2 - 1;

// PDE/PTE bits. These are no different from their i386 counterparts.

/// `PG_V`: valid.
pub const PG_V: u64 = 0x0000_0000_0000_0001;
/// `PG_RO`: read-only.
pub const PG_RO: u64 = 0x0000_0000_0000_0000;
/// `PG_RW`: read-write.
pub const PG_RW: u64 = 0x0000_0000_0000_0002;
/// `PG_u`: user accessible.
#[allow(non_upper_case_globals)] // the C name; PG_U is a different bit
pub const PG_u: u64 = 0x0000_0000_0000_0004;
/// `PG_PROT`: the protection bits.
pub const PG_PROT: u64 = 0x0000_0000_0000_0006;
/// `PG_WT`: write through.
pub const PG_WT: u64 = 0x0000_0000_0000_0008;
/// `PG_N`: non-cacheable.
pub const PG_N: u64 = 0x0000_0000_0000_0010;
/// `PG_U`: used.
pub const PG_U: u64 = 0x0000_0000_0000_0020;
/// `PG_M`: modified.
pub const PG_M: u64 = 0x0000_0000_0000_0040;
/// `PG_PAT`: PAT bit (on pte).
pub const PG_PAT: u64 = 0x0000_0000_0000_0080;
/// `PG_PS`: 2MB page size (on pde).
pub const PG_PS: u64 = 0x0000_0000_0000_0080;
/// `PG_G`: not flushed.
pub const PG_G: u64 = 0x0000_0000_0000_0100;
/// `PG_AVAIL1`: free for the OS.
pub const PG_AVAIL1: u64 = 0x0000_0000_0000_0200;
/// `PG_AVAIL2`.
pub const PG_AVAIL2: u64 = 0x0000_0000_0000_0400;
/// `PG_AVAIL3`.
pub const PG_AVAIL3: u64 = 0x0000_0000_0000_0800;
/// `PG_PATLG`: PAT on large pages.
pub const PG_PATLG: u64 = 0x0000_0000_0000_1000;
/// `PG_PKMASK`: Protection Key Mask.
pub const PG_PKMASK: u64 = 0x7800_0000_0000_0000;
/// `PG_XO`: key1 used for execute-only.
pub const PG_XO: u64 = 0x0800_0000_0000_0000;
/// `PG_NX`: non-executable.
pub const PG_NX: u64 = 0x8000_0000_0000_0000;
/// `PG_FRAME`: the page frame bits.
pub const PG_FRAME: u64 = 0x000f_ffff_ffff_f000;

/// `PG_LGFRAME`: large (2M) page frame mask.
pub const PG_LGFRAME: u64 = 0x000f_ffff_ffe0_0000;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/arch/amd64/include/pte.h");
        let ours: &[(&str, i64)] = &[
            ("L1_SHIFT", i64::from(L1_SHIFT)),
            ("L2_SHIFT", i64::from(L2_SHIFT)),
            ("L3_SHIFT", i64::from(L3_SHIFT)),
            ("L4_SHIFT", i64::from(L4_SHIFT)),
            ("L4_MASK", L4_MASK as i64),
            ("L3_MASK", L3_MASK as i64),
            ("L2_MASK", L2_MASK as i64),
            ("L1_MASK", L1_MASK as i64),
            ("PG_V", PG_V as i64),
            ("PG_RW", PG_RW as i64),
            ("PG_u", PG_u as i64),
            ("PG_PROT", PG_PROT as i64),
            ("PG_WT", PG_WT as i64),
            ("PG_N", PG_N as i64),
            ("PG_U", PG_U as i64),
            ("PG_M", PG_M as i64),
            ("PG_PAT", PG_PAT as i64),
            ("PG_PS", PG_PS as i64),
            ("PG_G", PG_G as i64),
            ("PG_AVAIL1", PG_AVAIL1 as i64),
            ("PG_AVAIL2", PG_AVAIL2 as i64),
            ("PG_AVAIL3", PG_AVAIL3 as i64),
            ("PG_PATLG", PG_PATLG as i64),
            ("PG_FRAME", PG_FRAME as i64),
            ("PG_LGFRAME", PG_LGFRAME as i64),
        ];
        for (name, value) in ours {
            assert_eq!(crate::reftest::int(&defs, name), Some(*value), "{name}");
        }
    }
}
