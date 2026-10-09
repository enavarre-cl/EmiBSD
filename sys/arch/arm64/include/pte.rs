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
 * Copyright (c) 2014 Dale Rahn <drahn@dalerahn.com>
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
//! arm64 `<machine/pte.h>`: the translation table descriptors.
//!
//! Upstream: sys/arch/arm64/include/pte.h @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M3 ports the descriptor types and attribute bits; nothing of the
//! header is left out, but see the deviation on the memory attribute indices.
//!
//! ## Deviations
//! - `PTE_ATTR_*` index `MAIR_EL1`, which the C's `locore.S` programs as 0 = Device-nGnRnE,
//!   1 = Device-nGnRE, 2 = non-cacheable, 3 = write-back, 4 = write-through. The kernel runs on
//!   the bootloader's `MAIR_EL1` until it owns its page tables: there index 0 is write-back
//!   (the kernel and the direct map use it) and index 1 Device-nGnRE, so the kernel keeps
//!   those two, fills 2 (Device-nGnRnE, `initarm`), 3 (non-cacheable) and 4 (write-through)
//!   in `pmap_bootstrap`, and the indices below follow that layout. `PTE_MEMATTR_*` (the
//!   stage-2 encodings for vmm) are the C's.

use crate::arch::arm64::include::param::PAGE_MASK;

/// `Lx_TYPE_MASK`: mask of type bits.
#[allow(non_upper_case_globals)] // the C name
pub const Lx_TYPE_MASK: u64 = 0x0000_0003;
/// `Lx_TYPE_S`: a block (section) descriptor.
#[allow(non_upper_case_globals)] // the C name
pub const Lx_TYPE_S: u64 = 0x0000_0001;
/// `Lx_TYPE_PT`: a table descriptor.
#[allow(non_upper_case_globals)] // the C name
pub const Lx_TYPE_PT: u64 = 0x0000_0003;

/// `Lx_PT_NS`: table descriptor: non-secure.
#[allow(non_upper_case_globals)] // the C name
pub const Lx_PT_NS: u64 = 1 << 63;
/// `Lx_PT_AP00`: table descriptor: no access restriction.
#[allow(non_upper_case_globals)] // the C name
pub const Lx_PT_AP00: u64 = 0 << 61;
/// `Lx_PT_AP01`.
#[allow(non_upper_case_globals)] // the C name
pub const Lx_PT_AP01: u64 = 1 << 61;
/// `Lx_PT_AP10`.
#[allow(non_upper_case_globals)] // the C name
pub const Lx_PT_AP10: u64 = 2 << 61;
/// `Lx_PT_AP11`.
#[allow(non_upper_case_globals)] // the C name
pub const Lx_PT_AP11: u64 = 3 << 61;
/// `Lx_PT_XN`: table descriptor: execute-never below.
#[allow(non_upper_case_globals)] // the C name
pub const Lx_PT_XN: u64 = 1 << 60;
/// `Lx_PT_PXN`: table descriptor: privileged execute-never below.
#[allow(non_upper_case_globals)] // the C name
pub const Lx_PT_PXN: u64 = 1 << 59;
/// `Lx_TABLE_ALIGN`: alignment of a translation table.
#[allow(non_upper_case_globals)] // the C name
pub const Lx_TABLE_ALIGN: usize = 4096;

// Block and Page attributes

/// `ATTR_MASK_H`: the upper attribute bits.
pub const ATTR_MASK_H: u64 = 0xfff0_0000_0000_0000;
/// `ATTR_MASK_L`: the lower attribute bits.
pub const ATTR_MASK_L: u64 = 0x0000_0000_0000_0fff;
/// `ATTR_MASK`.
pub const ATTR_MASK: u64 = ATTR_MASK_H | ATTR_MASK_L;

/// `ATTR_SW_MANAGED`: software: a managed page.
pub const ATTR_SW_MANAGED: u64 = 1 << 56;
/// `ATTR_SW_WIRED`: software: a wired mapping.
pub const ATTR_SW_WIRED: u64 = 1 << 55;
/// `ATTR_UXN`: unprivileged execute-never.
pub const ATTR_UXN: u64 = 1 << 54;
/// `ATTR_PXN`: privileged execute-never.
pub const ATTR_PXN: u64 = 1 << 53;
/// `ATTR_GP`: guarded page (BTI).
pub const ATTR_GP: u64 = 1 << 50;
/// `ATTR_nG`: not global.
#[allow(non_upper_case_globals)] // the C name
pub const ATTR_nG: u64 = 1 << 11;
/// `ATTR_AF`: access flag.
pub const ATTR_AF: u64 = 1 << 10;

/// `ATTR_SH(x)`: shareability.
pub const fn attr_sh(x: u64) -> u64 {
    x << 8
}

/// `ATTR_AP_RW_BIT`: the read-only bit of the access permissions.
pub const ATTR_AP_RW_BIT: u64 = 1 << 7;

/// `ATTR_AP(x)`: access permissions.
pub const fn attr_ap(x: u64) -> u64 {
    x << 6
}

/// `ATTR_AP_MASK`.
pub const ATTR_AP_MASK: u64 = attr_ap(3);
/// `ATTR_NS`: non-secure.
pub const ATTR_NS: u64 = 1 << 5;

/// `ATTR_IDX(x)`: memory attribute index into `MAIR_EL1`.
pub const fn attr_idx(x: u64) -> u64 {
    x << 2
}

/// `ATTR_IDX_MASK`.
pub const ATTR_IDX_MASK: u64 = 7 << 2;

/// `PTE_ATTR_DEV_NGNRNE`: `MAIR_EL1` index of Device-nGnRnE (see the module's deviations).
pub const PTE_ATTR_DEV_NGNRNE: u64 = 2;
/// `PTE_ATTR_DEV_NGNRE`: index of Device-nGnRE (the bootloader's).
pub const PTE_ATTR_DEV_NGNRE: u64 = 1;
/// `PTE_ATTR_CI`: index of non-cacheable memory.
pub const PTE_ATTR_CI: u64 = 3;
/// `PTE_ATTR_WB`: index of write-back memory (the bootloader's).
pub const PTE_ATTR_WB: u64 = 0;
/// `PTE_ATTR_WT`: index of write-through memory.
pub const PTE_ATTR_WT: u64 = 4;

/// `MAIR_EL1` attribute encoding: Device-nGnRnE.
pub const MAIR_DEV_NGNRNE: u64 = 0x00;
/// `MAIR_EL1` attribute encoding: Device-nGnRE.
pub const MAIR_DEV_NGNRE: u64 = 0x04;
/// `MAIR_EL1` attribute encoding: normal, non-cacheable (`locore.S`'s 0x44).
pub const MAIR_CI: u64 = 0x44;
/// `MAIR_EL1` attribute encoding: normal, write-back (`locore.S`'s 0xff).
pub const MAIR_WB: u64 = 0xff;
/// `MAIR_EL1` attribute encoding: normal, write-through (`locore.S`'s 0x88).
pub const MAIR_WT: u64 = 0x88;

/// `MAIR_ATTR(attr, idx)` (`locore.S`): `attr` in the field of index `idx`.
pub const fn mair_attr(attr: u64, idx: u64) -> u64 {
    attr << (8 * idx)
}

/// `PTE_MEMATTR_DEV_NGNRNE`: stage-2 memory attribute.
pub const PTE_MEMATTR_DEV_NGNRNE: u64 = 0x0;
/// `PTE_MEMATTR_DEV_NGNRE`.
pub const PTE_MEMATTR_DEV_NGNRE: u64 = 0x1;
/// `PTE_MEMATTR_CI`.
pub const PTE_MEMATTR_CI: u64 = 0x5;
/// `PTE_MEMATTR_WB`.
pub const PTE_MEMATTR_WB: u64 = 0xf;
/// `PTE_MEMATTR_WT`.
pub const PTE_MEMATTR_WT: u64 = 0xa;

/// `SH_INNER`: inner shareable.
pub const SH_INNER: u64 = 3;
/// `SH_OUTER`: outer shareable.
pub const SH_OUTER: u64 = 2;
/// `SH_NONE`: non-shareable.
pub const SH_NONE: u64 = 0;

// Level 0 table, 512GiB per entry

/// `L0_SHIFT`.
pub const L0_SHIFT: u32 = 39;
/// `L0_INVAL`: an invalid address.
pub const L0_INVAL: u64 = 0x0;
/// `L0_BLOCK`: a block.
pub const L0_BLOCK: u64 = 0x1;
/// `L0_TABLE`: a next-level table.
pub const L0_TABLE: u64 = 0x3;

// Level 1 table, 1GiB per entry

/// `L1_SHIFT`.
pub const L1_SHIFT: u32 = 30;
/// `L1_SIZE`.
pub const L1_SIZE: usize = 1 << L1_SHIFT;
/// `L1_OFFSET`.
pub const L1_OFFSET: usize = L1_SIZE - 1;
/// `L1_INVAL`.
pub const L1_INVAL: u64 = L0_INVAL;
/// `L1_BLOCK`.
pub const L1_BLOCK: u64 = L0_BLOCK;
/// `L1_TABLE`.
pub const L1_TABLE: u64 = L0_TABLE;

// Level 2 table, 2MiB per entry

/// `L2_SHIFT`.
pub const L2_SHIFT: u32 = 21;
/// `L2_SIZE`.
pub const L2_SIZE: usize = 1 << L2_SHIFT;
/// `L2_OFFSET`.
pub const L2_OFFSET: usize = L2_SIZE - 1;
/// `L2_INVAL`.
pub const L2_INVAL: u64 = L0_INVAL;
/// `L2_BLOCK`.
pub const L2_BLOCK: u64 = L0_BLOCK;
/// `L2_TABLE`.
pub const L2_TABLE: u64 = L0_TABLE;

// Level 3 table, 4KiB per entry

/// `L3_P`: a page.
pub const L3_P: u64 = 0x3;

/// `Ln_ENTRIES`: entries per table.
#[allow(non_upper_case_globals)] // the C name
pub const Ln_ENTRIES: usize = 1 << 9;
/// `Ln_ADDR_MASK`.
#[allow(non_upper_case_globals)] // the C name
pub const Ln_ADDR_MASK: usize = Ln_ENTRIES - 1;
/// `Ln_TABLE_MASK`.
#[allow(non_upper_case_globals)] // the C name
pub const Ln_TABLE_MASK: usize = (1 << 12) - 1;

/// `PTE_RPGN`: the output address bits of a descriptor.
pub const PTE_RPGN: u64 = ((1 << 48) - 1) & !(PAGE_MASK as u64);
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bit_helpers() {
        assert_eq!(attr_sh(SH_INNER), 3 << 8);
        assert_eq!(attr_ap(3), ATTR_AP_MASK);
        assert_eq!(attr_idx(PTE_ATTR_WT), 4 << 2);
        assert_eq!(mair_attr(MAIR_CI, 3), 0x44 << 24);
        assert_eq!(PTE_RPGN, 0x0000_ffff_ffff_f000);
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/arch/arm64/include/pte.h");
        let ours: &[(&str, i64)] = &[
            ("Lx_TYPE_MASK", Lx_TYPE_MASK as i64),
            ("Lx_TYPE_S", Lx_TYPE_S as i64),
            ("Lx_TYPE_PT", Lx_TYPE_PT as i64),
            ("Lx_TABLE_ALIGN", Lx_TABLE_ALIGN as i64),
            ("PTE_MEMATTR_CI", PTE_MEMATTR_CI as i64),
            ("PTE_MEMATTR_WB", PTE_MEMATTR_WB as i64),
            ("PTE_MEMATTR_WT", PTE_MEMATTR_WT as i64),
            ("SH_INNER", SH_INNER as i64),
            ("SH_OUTER", SH_OUTER as i64),
            ("L0_SHIFT", i64::from(L0_SHIFT)),
            ("L1_SHIFT", i64::from(L1_SHIFT)),
            ("L2_SHIFT", i64::from(L2_SHIFT)),
            ("L3_P", L3_P as i64),
        ];
        for (name, value) in ours {
            assert_eq!(crate::reftest::int(&defs, name), Some(*value), "{name}");
        }
    }
}
/* </TESTS> */
