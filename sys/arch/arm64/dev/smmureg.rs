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
 * Copyright (c) 2021 Patrick Wildt <patrick@blueri.se>
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
//! The ARM System MMU's registers and in-memory formats: `arch/arm64/dev/smmureg.h`.
//!
//! Upstream: sys/arch/arm64/dev/smmureg.h @ 3ce1f3f79392
//!
//! SMMUv2 (MMU-500) is programmed through its global register spaces and per-context-bank
//! pages; SMMUv3 through one register page, a command queue, an event queue and in-memory
//! stream table entries (STE) and context descriptors (CD), whose fields are the `SMMU_V3_CD_*`
//! and `SMMU_V3_STE_*` bits. Register offsets are `usize` (`bus_size_t`), 32-bit register
//! fields `u32`, the 64-bit ones and the queue/table words `u64`.
//!
//! ## Deviations
//! - Function-like macros are lowercase `const fn`s (`SMMU_SMR(x)` is [`smmu_smr`]).
//! - `SMMU_V3_Q_IDX(q, p)` and `SMMU_V3_Q_WRP(q, p)` take the queue's `sq_size_log2` instead
//!   of the queue.
//! - `SMMU_IDR2_EXNUMSMRG` is an object-like macro in the C that uses an `x` it does not
//!   take (nothing expands it); here it is [`smmu_idr2_exnumsmrg`] of the register.
//! - Identity operations of the C (`(x) >> 0`, `0x000 + (x) * 4`, `(x) << 0` in the
//!   functions) are left out of the function bodies; the constants keep the C's `(0x2 << 0)`
//!   field encodings as written.
#![allow(clippy::identity_op)] // the constants' `(value << 0)`, kept as the C writes them

// SMMU v2

// Global Register Space 0
/// `SMMU_SCR0`.
pub const SMMU_SCR0: usize = 0x000;
/// `SMMU_SCR0_CLIENTPD`.
pub const SMMU_SCR0_CLIENTPD: u32 = 1 << 0;
/// `SMMU_SCR0_GFRE`.
pub const SMMU_SCR0_GFRE: u32 = 1 << 1;
/// `SMMU_SCR0_GFIE`.
pub const SMMU_SCR0_GFIE: u32 = 1 << 2;
/// `SMMU_SCR0_EXIDENABLE`.
pub const SMMU_SCR0_EXIDENABLE: u32 = 1 << 3;
/// `SMMU_SCR0_GCFGFRE`.
pub const SMMU_SCR0_GCFGFRE: u32 = 1 << 4;
/// `SMMU_SCR0_GCFGFIE`.
pub const SMMU_SCR0_GCFGFIE: u32 = 1 << 5;
/// `SMMU_SCR0_USFCFG`.
pub const SMMU_SCR0_USFCFG: u32 = 1 << 10;
/// `SMMU_SCR0_VMIDPNE`.
pub const SMMU_SCR0_VMIDPNE: u32 = 1 << 11;
/// `SMMU_SCR0_PTM`.
pub const SMMU_SCR0_PTM: u32 = 1 << 12;
/// `SMMU_SCR0_FB`.
pub const SMMU_SCR0_FB: u32 = 1 << 13;
/// `SMMU_SCR0_BSU_MASK`.
pub const SMMU_SCR0_BSU_MASK: u32 = 0x3 << 14;
/// `SMMU_SCR0_VMID16EN`.
pub const SMMU_SCR0_VMID16EN: u32 = 1 << 31;
/// `SMMU_SCR1`.
pub const SMMU_SCR1: usize = 0x004;
/// `SMMU_SCR2`.
pub const SMMU_SCR2: usize = 0x008;
/// `SMMU_SACR`.
pub const SMMU_SACR: usize = 0x010;
/// `SMMU_SACR_MMU500_SMTNMB_TLBEN`.
pub const SMMU_SACR_MMU500_SMTNMB_TLBEN: u32 = 1 << 8;
/// `SMMU_SACR_MMU500_S2CRB_TLBEN`.
pub const SMMU_SACR_MMU500_S2CRB_TLBEN: u32 = 1 << 10;
/// `SMMU_SACR_MMU500_CACHE_LOCK`.
pub const SMMU_SACR_MMU500_CACHE_LOCK: u32 = 1 << 26;
/// `SMMU_IDR0`.
pub const SMMU_IDR0: usize = 0x020;
/// `SMMU_IDR0_NUMSMRG(x)`.
pub const fn smmu_idr0_numsmrg(x: u32) -> u32 {
    x & 0xff
}
/// `SMMU_IDR0_EXIDS`.
pub const SMMU_IDR0_EXIDS: u32 = 1 << 8;
/// `SMMU_IDR0_NUMSIDB(x)`.
pub const fn smmu_idr0_numsidb(x: u32) -> u32 {
    (x >> 9) & 0xf
}
/// `SMMU_IDR0_BTM`.
pub const SMMU_IDR0_BTM: u32 = 1 << 13;
/// `SMMU_IDR0_CCTM`.
pub const SMMU_IDR0_CCTM: u32 = 1 << 14;
/// `SMMU_IDR0_EXSMRGS`.
pub const SMMU_IDR0_EXSMRGS: u32 = 1 << 15;
/// `SMMU_IDR0_NUMIRPT(x)`.
pub const fn smmu_idr0_numirpt(x: u32) -> u32 {
    (x >> 16) & 0xff
}
/// `SMMU_IDR0_PTFS(x)`.
pub const fn smmu_idr0_ptfs(x: u32) -> u32 {
    (x >> 24) & 0x3
}
/// `SMMU_IDR0_PTFS_AARCH32_SHORT_AND_LONG`.
pub const SMMU_IDR0_PTFS_AARCH32_SHORT_AND_LONG: u32 = 0x0;
/// `SMMU_IDR0_PTFS_AARCH32_ONLY_LONG`.
pub const SMMU_IDR0_PTFS_AARCH32_ONLY_LONG: u32 = 0x1;
/// `SMMU_IDR0_PTFS_AARCH32_NO`.
pub const SMMU_IDR0_PTFS_AARCH32_NO: u32 = 0x2;
/// `SMMU_IDR0_PTFS_AARCH32_RES`.
pub const SMMU_IDR0_PTFS_AARCH32_RES: u32 = 0x3;
/// `SMMU_IDR0_ATOSNS`.
pub const SMMU_IDR0_ATOSNS: u32 = 1 << 26;
/// `SMMU_IDR0_SMS`.
pub const SMMU_IDR0_SMS: u32 = 1 << 27;
/// `SMMU_IDR0_NTS`.
pub const SMMU_IDR0_NTS: u32 = 1 << 28;
/// `SMMU_IDR0_S2TS`.
pub const SMMU_IDR0_S2TS: u32 = 1 << 29;
/// `SMMU_IDR0_S1TS`.
pub const SMMU_IDR0_S1TS: u32 = 1 << 30;
/// `SMMU_IDR0_SES`.
pub const SMMU_IDR0_SES: u32 = 1 << 31;
/// `SMMU_IDR1`.
pub const SMMU_IDR1: usize = 0x024;
/// `SMMU_IDR1_NUMCB(x)`.
pub const fn smmu_idr1_numcb(x: u32) -> u32 {
    x & 0xff
}
/// `SMMU_IDR1_NUMSSDNDXB(x)`.
pub const fn smmu_idr1_numssdndxb(x: u32) -> u32 {
    (x >> 8) & 0xf
}
/// `SMMU_IDR1_SSDTP(x)`.
pub const fn smmu_idr1_ssdtp(x: u32) -> u32 {
    (x >> 12) & 0x3
}
/// `SMMU_IDR1_SSDTP_UNK`.
pub const SMMU_IDR1_SSDTP_UNK: u32 = 0x0;
/// `SMMU_IDR1_SSDTP_IDX_NUMSSDNDXB`.
pub const SMMU_IDR1_SSDTP_IDX_NUMSSDNDXB: u32 = 0x1;
/// `SMMU_IDR1_SSDTP_RES`.
pub const SMMU_IDR1_SSDTP_RES: u32 = 0x2;
/// `SMMU_IDR1_SSDTP_IDX_16BIT`.
pub const SMMU_IDR1_SSDTP_IDX_16BIT: u32 = 0x3;
/// `SMMU_IDR1_SMCD`.
pub const SMMU_IDR1_SMCD: u32 = 1 << 15;
/// `SMMU_IDR1_NUMS2CB(x)`.
pub const fn smmu_idr1_nums2cb(x: u32) -> u32 {
    (x >> 16) & 0xff
}
/// `SMMU_IDR1_HAFDBS(x)`.
pub const fn smmu_idr1_hafdbs(x: u32) -> u32 {
    (x >> 24) & 0x3
}
/// `SMMU_IDR1_HAFDBS_NO`.
pub const SMMU_IDR1_HAFDBS_NO: u32 = 0x0;
/// `SMMU_IDR1_HAFDBS_AF`.
pub const SMMU_IDR1_HAFDBS_AF: u32 = 0x1;
/// `SMMU_IDR1_HAFDBS_RES`.
pub const SMMU_IDR1_HAFDBS_RES: u32 = 0x2;
/// `SMMU_IDR1_HAFDBS_AFDB`.
pub const SMMU_IDR1_HAFDBS_AFDB: u32 = 0x3;
/// `SMMU_IDR1_NUMPAGENDXB(x)`.
pub const fn smmu_idr1_numpagendxb(x: u32) -> u32 {
    (x >> 28) & 0x7
}
/// `SMMU_IDR1_PAGESIZE_4K`.
pub const SMMU_IDR1_PAGESIZE_4K: u32 = 0 << 31;
/// `SMMU_IDR1_PAGESIZE_64K`.
pub const SMMU_IDR1_PAGESIZE_64K: u32 = 1 << 31;
/// `SMMU_IDR2`.
pub const SMMU_IDR2: usize = 0x028;
/// `SMMU_IDR2_IAS(x)`.
pub const fn smmu_idr2_ias(x: u32) -> u32 {
    x & 0xf
}
/// `SMMU_IDR2_IAS_32BIT`.
pub const SMMU_IDR2_IAS_32BIT: u32 = 0x0;
/// `SMMU_IDR2_IAS_36BIT`.
pub const SMMU_IDR2_IAS_36BIT: u32 = 0x1;
/// `SMMU_IDR2_IAS_40BIT`.
pub const SMMU_IDR2_IAS_40BIT: u32 = 0x2;
/// `SMMU_IDR2_IAS_42BIT`.
pub const SMMU_IDR2_IAS_42BIT: u32 = 0x3;
/// `SMMU_IDR2_IAS_44BIT`.
pub const SMMU_IDR2_IAS_44BIT: u32 = 0x4;
/// `SMMU_IDR2_IAS_48BIT`.
pub const SMMU_IDR2_IAS_48BIT: u32 = 0x5;
/// `SMMU_IDR2_OAS(x)`.
pub const fn smmu_idr2_oas(x: u32) -> u32 {
    (x >> 4) & 0xf
}
/// `SMMU_IDR2_OAS_32BIT`.
pub const SMMU_IDR2_OAS_32BIT: u32 = 0x0;
/// `SMMU_IDR2_OAS_36BIT`.
pub const SMMU_IDR2_OAS_36BIT: u32 = 0x1;
/// `SMMU_IDR2_OAS_40BIT`.
pub const SMMU_IDR2_OAS_40BIT: u32 = 0x2;
/// `SMMU_IDR2_OAS_42BIT`.
pub const SMMU_IDR2_OAS_42BIT: u32 = 0x3;
/// `SMMU_IDR2_OAS_44BIT`.
pub const SMMU_IDR2_OAS_44BIT: u32 = 0x4;
/// `SMMU_IDR2_OAS_48BIT`.
pub const SMMU_IDR2_OAS_48BIT: u32 = 0x5;
/// `SMMU_IDR2_UBS(x)`.
pub const fn smmu_idr2_ubs(x: u32) -> u32 {
    (x >> 8) & 0xf
}
/// `SMMU_IDR2_UBS_32BIT`.
pub const SMMU_IDR2_UBS_32BIT: u32 = 0x0;
/// `SMMU_IDR2_UBS_36BIT`.
pub const SMMU_IDR2_UBS_36BIT: u32 = 0x1;
/// `SMMU_IDR2_UBS_40BIT`.
pub const SMMU_IDR2_UBS_40BIT: u32 = 0x2;
/// `SMMU_IDR2_UBS_42BIT`.
pub const SMMU_IDR2_UBS_42BIT: u32 = 0x3;
/// `SMMU_IDR2_UBS_44BIT`.
pub const SMMU_IDR2_UBS_44BIT: u32 = 0x4;
/// `SMMU_IDR2_UBS_49BIT`.
pub const SMMU_IDR2_UBS_49BIT: u32 = 0x5;
/// `SMMU_IDR2_UBS_64BIT`.
pub const SMMU_IDR2_UBS_64BIT: u32 = 0xf;
/// `SMMU_IDR2_PTFSV8_4KB`.
pub const SMMU_IDR2_PTFSV8_4KB: u32 = 1 << 12;
/// `SMMU_IDR2_PTFSV8_16KB`.
pub const SMMU_IDR2_PTFSV8_16KB: u32 = 1 << 13;
/// `SMMU_IDR2_PTFSV8_64KB`.
pub const SMMU_IDR2_PTFSV8_64KB: u32 = 1 << 14;
/// `SMMU_IDR2_VMID16S`.
pub const SMMU_IDR2_VMID16S: u32 = 1 << 15;
/// `SMMU_IDR2_EXNUMSMRG`: an object-like macro in the C that uses an `x` it does not take
/// (unused there); a function of the register here.
pub const fn smmu_idr2_exnumsmrg(x: u32) -> u32 {
    (x >> 16) & 0x7ff
}
/// `SMMU_IDR2_E2HS`.
pub const SMMU_IDR2_E2HS: u32 = 1 << 27;
/// `SMMU_IDR2_HADS`.
pub const SMMU_IDR2_HADS: u32 = 1 << 28;
/// `SMMU_IDR2_COMPINDEXS`.
pub const SMMU_IDR2_COMPINDEXS: u32 = 1 << 29;
/// `SMMU_IDR2_DIPANS`.
pub const SMMU_IDR2_DIPANS: u32 = 1 << 30;
/// `SMMU_IDR3`.
pub const SMMU_IDR3: usize = 0x02c;
/// `SMMU_IDR4`.
pub const SMMU_IDR4: usize = 0x030;
/// `SMMU_IDR5`.
pub const SMMU_IDR5: usize = 0x034;
/// `SMMU_IDR6`.
pub const SMMU_IDR6: usize = 0x038;
/// `SMMU_IDR7`.
pub const SMMU_IDR7: usize = 0x03c;
/// `SMMU_IDR7_MINOR(x)`.
pub const fn smmu_idr7_minor(x: u32) -> u32 {
    x & 0xf
}
/// `SMMU_IDR7_MAJOR(x)`.
pub const fn smmu_idr7_major(x: u32) -> u32 {
    (x >> 4) & 0xf
}
/// `SMMU_SGFSR`.
pub const SMMU_SGFSR: usize = 0x048;
/// `SMMU_SGFSYNR0`.
pub const SMMU_SGFSYNR0: usize = 0x050;
/// `SMMU_SGFSYNR1`.
pub const SMMU_SGFSYNR1: usize = 0x054;
/// `SMMU_SGFSYNR2`.
pub const SMMU_SGFSYNR2: usize = 0x058;
/// `SMMU_TLBIVMID`.
pub const SMMU_TLBIVMID: usize = 0x064;
/// `SMMU_TLBIALLNSNH`.
pub const SMMU_TLBIALLNSNH: usize = 0x068;
/// `SMMU_TLBIALLH`.
pub const SMMU_TLBIALLH: usize = 0x06c;
/// `SMMU_STLBGSYNC`.
pub const SMMU_STLBGSYNC: usize = 0x070;
/// `SMMU_STLBGSTATUS`.
pub const SMMU_STLBGSTATUS: usize = 0x074;
/// `SMMU_STLBGSTATUS_GSACTIVE`.
pub const SMMU_STLBGSTATUS_GSACTIVE: u32 = 1 << 0;
/// `SMMU_SMR(x)`.
pub const fn smmu_smr(x: usize) -> usize {
    0x800 + x * 0x4
}
/// `SMMU_SMR_ID_SHIFT`.
pub const SMMU_SMR_ID_SHIFT: u32 = 0;
/// `SMMU_SMR_ID_MASK`.
pub const SMMU_SMR_ID_MASK: u32 = 0x7fff;
/// `SMMU_SMR_MASK_SHIFT`.
pub const SMMU_SMR_MASK_SHIFT: u32 = 16;
/// `SMMU_SMR_MASK_MASK`.
pub const SMMU_SMR_MASK_MASK: u32 = 0x7fff;
/// `SMMU_SMR_VALID`.
pub const SMMU_SMR_VALID: u32 = 1 << 31;
/// `SMMU_S2CR(x)`.
pub const fn smmu_s2cr(x: usize) -> usize {
    0xc00 + x * 0x4
}
/// `SMMU_S2CR_EXIDVALID`.
pub const SMMU_S2CR_EXIDVALID: u32 = 1 << 10;
/// `SMMU_S2CR_TYPE_TRANS`.
pub const SMMU_S2CR_TYPE_TRANS: u32 = 0 << 16;
/// `SMMU_S2CR_TYPE_BYPASS`.
pub const SMMU_S2CR_TYPE_BYPASS: u32 = 1 << 16;
/// `SMMU_S2CR_TYPE_FAULT`.
pub const SMMU_S2CR_TYPE_FAULT: u32 = 2 << 16;
/// `SMMU_S2CR_TYPE_MASK`.
pub const SMMU_S2CR_TYPE_MASK: u32 = 0x3 << 16;

// Global Register Space 1
/// `SMMU_CBAR(x)`.
pub const fn smmu_cbar(x: usize) -> usize {
    x * 0x4
}
/// `SMMU_CBAR_VMID_SHIFT`.
pub const SMMU_CBAR_VMID_SHIFT: u32 = 0;
/// `SMMU_CBAR_BPSHCFG_RES`.
pub const SMMU_CBAR_BPSHCFG_RES: u32 = 0x0 << 8;
/// `SMMU_CBAR_BPSHCFG_OSH`.
pub const SMMU_CBAR_BPSHCFG_OSH: u32 = 0x1 << 8;
/// `SMMU_CBAR_BPSHCFG_ISH`.
pub const SMMU_CBAR_BPSHCFG_ISH: u32 = 0x2 << 8;
/// `SMMU_CBAR_BPSHCFG_NSH`.
pub const SMMU_CBAR_BPSHCFG_NSH: u32 = 0x3 << 8;
/// `SMMU_CBAR_MEMATTR_WB`.
pub const SMMU_CBAR_MEMATTR_WB: u32 = 0xf << 12;
/// `SMMU_CBAR_TYPE_S2_TRANS`.
pub const SMMU_CBAR_TYPE_S2_TRANS: u32 = 0x0 << 16;
/// `SMMU_CBAR_TYPE_S1_TRANS_S2_BYPASS`.
pub const SMMU_CBAR_TYPE_S1_TRANS_S2_BYPASS: u32 = 0x1 << 16;
/// `SMMU_CBAR_TYPE_S1_TRANS_S2_FAULT`.
pub const SMMU_CBAR_TYPE_S1_TRANS_S2_FAULT: u32 = 0x2 << 16;
/// `SMMU_CBAR_TYPE_S1_TRANS_S2_TRANS`.
pub const SMMU_CBAR_TYPE_S1_TRANS_S2_TRANS: u32 = 0x3 << 16;
/// `SMMU_CBAR_TYPE_MASK`.
pub const SMMU_CBAR_TYPE_MASK: u32 = 0x3 << 16;
/// `SMMU_CBAR_IRPTNDX_SHIFT`.
pub const SMMU_CBAR_IRPTNDX_SHIFT: u32 = 24;
/// `SMMU_CBFRSYNRA(x)`.
pub const fn smmu_cbfrsynra(x: usize) -> usize {
    0x400 + x * 0x4
}
/// `SMMU_CBA2R(x)`.
pub const fn smmu_cba2r(x: usize) -> usize {
    0x800 + x * 0x4
}
/// `SMMU_CBA2R_VA64`.
pub const SMMU_CBA2R_VA64: u32 = 1 << 0;
/// `SMMU_CBA2R_MONC`.
pub const SMMU_CBA2R_MONC: u32 = 1 << 1;
/// `SMMU_CBA2R_VMID16_SHIFT`.
pub const SMMU_CBA2R_VMID16_SHIFT: u32 = 16;

// Context Bank Format
/// `SMMU_CB_SCTLR`.
pub const SMMU_CB_SCTLR: usize = 0x000;
/// `SMMU_CB_SCTLR_M`.
pub const SMMU_CB_SCTLR_M: u32 = 1 << 0;
/// `SMMU_CB_SCTLR_TRE`.
pub const SMMU_CB_SCTLR_TRE: u32 = 1 << 1;
/// `SMMU_CB_SCTLR_AFE`.
pub const SMMU_CB_SCTLR_AFE: u32 = 1 << 2;
/// `SMMU_CB_SCTLR_CFRE`.
pub const SMMU_CB_SCTLR_CFRE: u32 = 1 << 5;
/// `SMMU_CB_SCTLR_CFIE`.
pub const SMMU_CB_SCTLR_CFIE: u32 = 1 << 6;
/// `SMMU_CB_SCTLR_ASIDPNE`.
pub const SMMU_CB_SCTLR_ASIDPNE: u32 = 1 << 12;
/// `SMMU_CB_ACTLR`.
pub const SMMU_CB_ACTLR: usize = 0x004;
/// `SMMU_CB_ACTLR_CPRE`.
pub const SMMU_CB_ACTLR_CPRE: u32 = 1 << 1;
/// `SMMU_CB_TCR2`.
pub const SMMU_CB_TCR2: usize = 0x010;
/// `SMMU_CB_TCR2_PASIZE_32BIT`.
pub const SMMU_CB_TCR2_PASIZE_32BIT: u32 = 0x0 << 0;
/// `SMMU_CB_TCR2_PASIZE_36BIT`.
pub const SMMU_CB_TCR2_PASIZE_36BIT: u32 = 0x1 << 0;
/// `SMMU_CB_TCR2_PASIZE_40BIT`.
pub const SMMU_CB_TCR2_PASIZE_40BIT: u32 = 0x2 << 0;
/// `SMMU_CB_TCR2_PASIZE_42BIT`.
pub const SMMU_CB_TCR2_PASIZE_42BIT: u32 = 0x3 << 0;
/// `SMMU_CB_TCR2_PASIZE_44BIT`.
pub const SMMU_CB_TCR2_PASIZE_44BIT: u32 = 0x4 << 0;
/// `SMMU_CB_TCR2_PASIZE_48BIT`.
pub const SMMU_CB_TCR2_PASIZE_48BIT: u32 = 0x5 << 0;
/// `SMMU_CB_TCR2_PASIZE_MASK`.
pub const SMMU_CB_TCR2_PASIZE_MASK: u32 = 0x7 << 0;
/// `SMMU_CB_TCR2_AS`.
pub const SMMU_CB_TCR2_AS: u32 = 1 << 4;
/// `SMMU_CB_TCR2_SEP_UPSTREAM`.
pub const SMMU_CB_TCR2_SEP_UPSTREAM: u32 = 0x7 << 15;
/// `SMMU_CB_TTBR0`.
pub const SMMU_CB_TTBR0: usize = 0x020;
/// `SMMU_CB_TTBR1`.
pub const SMMU_CB_TTBR1: usize = 0x028;
/// `SMMU_CB_TTBR_ASID_SHIFT`.
pub const SMMU_CB_TTBR_ASID_SHIFT: u32 = 48;
/// `SMMU_CB_TCR`.
pub const SMMU_CB_TCR: usize = 0x030;
/// `SMMU_CB_TCR_T0SZ(x)`.
pub const fn smmu_cb_tcr_t0sz(x: u32) -> u32 {
    x
}
/// `SMMU_CB_TCR_EPD0`.
pub const SMMU_CB_TCR_EPD0: u32 = 1 << 7;
/// `SMMU_CB_TCR_IRGN0_NC`.
pub const SMMU_CB_TCR_IRGN0_NC: u32 = 0x0 << 8;
/// `SMMU_CB_TCR_IRGN0_WBWA`.
pub const SMMU_CB_TCR_IRGN0_WBWA: u32 = 0x1 << 8;
/// `SMMU_CB_TCR_IRGN0_WT`.
pub const SMMU_CB_TCR_IRGN0_WT: u32 = 0x2 << 8;
/// `SMMU_CB_TCR_IRGN0_WB`.
pub const SMMU_CB_TCR_IRGN0_WB: u32 = 0x3 << 8;
/// `SMMU_CB_TCR_ORGN0_NC`.
pub const SMMU_CB_TCR_ORGN0_NC: u32 = 0x0 << 10;
/// `SMMU_CB_TCR_ORGN0_WBWA`.
pub const SMMU_CB_TCR_ORGN0_WBWA: u32 = 0x1 << 10;
/// `SMMU_CB_TCR_ORGN0_WT`.
pub const SMMU_CB_TCR_ORGN0_WT: u32 = 0x2 << 10;
/// `SMMU_CB_TCR_ORGN0_WB`.
pub const SMMU_CB_TCR_ORGN0_WB: u32 = 0x3 << 10;
/// `SMMU_CB_TCR_SH0_NSH`.
pub const SMMU_CB_TCR_SH0_NSH: u32 = 0x0 << 12;
/// `SMMU_CB_TCR_SH0_OSH`.
pub const SMMU_CB_TCR_SH0_OSH: u32 = 0x2 << 12;
/// `SMMU_CB_TCR_SH0_ISH`.
pub const SMMU_CB_TCR_SH0_ISH: u32 = 0x3 << 12;
/// `SMMU_CB_TCR_TG0_4KB`.
pub const SMMU_CB_TCR_TG0_4KB: u32 = 0x0 << 14;
/// `SMMU_CB_TCR_TG0_64KB`.
pub const SMMU_CB_TCR_TG0_64KB: u32 = 0x1 << 14;
/// `SMMU_CB_TCR_TG0_16KB`.
pub const SMMU_CB_TCR_TG0_16KB: u32 = 0x2 << 14;
/// `SMMU_CB_TCR_TG0_MASK`.
pub const SMMU_CB_TCR_TG0_MASK: u32 = 0x3 << 14;
/// `SMMU_CB_TCR_T1SZ(x)`.
pub const fn smmu_cb_tcr_t1sz(x: u32) -> u32 {
    x << 16
}
/// `SMMU_CB_TCR_EPD1`.
pub const SMMU_CB_TCR_EPD1: u32 = 1 << 23;
/// `SMMU_CB_TCR_IRGN1_NC`.
pub const SMMU_CB_TCR_IRGN1_NC: u32 = 0x0 << 24;
/// `SMMU_CB_TCR_IRGN1_WBWA`.
pub const SMMU_CB_TCR_IRGN1_WBWA: u32 = 0x1 << 24;
/// `SMMU_CB_TCR_IRGN1_WT`.
pub const SMMU_CB_TCR_IRGN1_WT: u32 = 0x2 << 24;
/// `SMMU_CB_TCR_IRGN1_WB`.
pub const SMMU_CB_TCR_IRGN1_WB: u32 = 0x3 << 24;
/// `SMMU_CB_TCR_ORGN1_NC`.
pub const SMMU_CB_TCR_ORGN1_NC: u32 = 0x0 << 26;
/// `SMMU_CB_TCR_ORGN1_WBWA`.
pub const SMMU_CB_TCR_ORGN1_WBWA: u32 = 0x1 << 26;
/// `SMMU_CB_TCR_ORGN1_WT`.
pub const SMMU_CB_TCR_ORGN1_WT: u32 = 0x2 << 26;
/// `SMMU_CB_TCR_ORGN1_WB`.
pub const SMMU_CB_TCR_ORGN1_WB: u32 = 0x3 << 26;
/// `SMMU_CB_TCR_SH1_NSH`.
pub const SMMU_CB_TCR_SH1_NSH: u32 = 0x0 << 28;
/// `SMMU_CB_TCR_SH1_OSH`.
pub const SMMU_CB_TCR_SH1_OSH: u32 = 0x2 << 28;
/// `SMMU_CB_TCR_SH1_ISH`.
pub const SMMU_CB_TCR_SH1_ISH: u32 = 0x3 << 28;
/// `SMMU_CB_TCR_TG1_16KB`.
pub const SMMU_CB_TCR_TG1_16KB: u32 = 0x1 << 30;
/// `SMMU_CB_TCR_TG1_4KB`.
pub const SMMU_CB_TCR_TG1_4KB: u32 = 0x2 << 30;
/// `SMMU_CB_TCR_TG1_64KB`.
pub const SMMU_CB_TCR_TG1_64KB: u32 = 0x3 << 30;
/// `SMMU_CB_TCR_TG1_MASK`.
pub const SMMU_CB_TCR_TG1_MASK: u32 = 0x3 << 30;
/// `SMMU_CB_TCR_S2_SL0_4KB_L2`.
pub const SMMU_CB_TCR_S2_SL0_4KB_L2: u32 = 0x0 << 6;
/// `SMMU_CB_TCR_S2_SL0_4KB_L1`.
pub const SMMU_CB_TCR_S2_SL0_4KB_L1: u32 = 0x1 << 6;
/// `SMMU_CB_TCR_S2_SL0_4KB_L0`.
pub const SMMU_CB_TCR_S2_SL0_4KB_L0: u32 = 0x2 << 6;
/// `SMMU_CB_TCR_S2_SL0_16KB_L3`.
pub const SMMU_CB_TCR_S2_SL0_16KB_L3: u32 = 0x0 << 6;
/// `SMMU_CB_TCR_S2_SL0_16KB_L2`.
pub const SMMU_CB_TCR_S2_SL0_16KB_L2: u32 = 0x1 << 6;
/// `SMMU_CB_TCR_S2_SL0_16KB_L1`.
pub const SMMU_CB_TCR_S2_SL0_16KB_L1: u32 = 0x2 << 6;
/// `SMMU_CB_TCR_S2_SL0_64KB_L3`.
pub const SMMU_CB_TCR_S2_SL0_64KB_L3: u32 = 0x0 << 6;
/// `SMMU_CB_TCR_S2_SL0_64KB_L2`.
pub const SMMU_CB_TCR_S2_SL0_64KB_L2: u32 = 0x1 << 6;
/// `SMMU_CB_TCR_S2_SL0_64KB_L1`.
pub const SMMU_CB_TCR_S2_SL0_64KB_L1: u32 = 0x2 << 6;
/// `SMMU_CB_TCR_S2_SL0_MASK`.
pub const SMMU_CB_TCR_S2_SL0_MASK: u32 = 0x3 << 6;
/// `SMMU_CB_TCR_S2_PASIZE_32BIT`.
pub const SMMU_CB_TCR_S2_PASIZE_32BIT: u32 = 0x0 << 16;
/// `SMMU_CB_TCR_S2_PASIZE_36BIT`.
pub const SMMU_CB_TCR_S2_PASIZE_36BIT: u32 = 0x1 << 16;
/// `SMMU_CB_TCR_S2_PASIZE_40BIT`.
pub const SMMU_CB_TCR_S2_PASIZE_40BIT: u32 = 0x2 << 16;
/// `SMMU_CB_TCR_S2_PASIZE_42BIT`.
pub const SMMU_CB_TCR_S2_PASIZE_42BIT: u32 = 0x3 << 16;
/// `SMMU_CB_TCR_S2_PASIZE_44BIT`.
pub const SMMU_CB_TCR_S2_PASIZE_44BIT: u32 = 0x4 << 16;
/// `SMMU_CB_TCR_S2_PASIZE_48BIT`.
pub const SMMU_CB_TCR_S2_PASIZE_48BIT: u32 = 0x5 << 16;
/// `SMMU_CB_TCR_S2_PASIZE_MASK`.
pub const SMMU_CB_TCR_S2_PASIZE_MASK: u32 = 0x7 << 16;
/// `SMMU_CB_MAIR0`.
pub const SMMU_CB_MAIR0: usize = 0x038;
/// `SMMU_CB_MAIR1`.
pub const SMMU_CB_MAIR1: usize = 0x03c;
/// `SMMU_CB_MAIR_MAIR_ATTR(attr, idx)`.
pub const fn smmu_cb_mair_mair_attr(attr: u32, idx: u32) -> u32 {
    attr << (idx * 8)
}
/// `SMMU_CB_MAIR_DEVICE_nGnRnE`.
#[allow(non_upper_case_globals)] // the C name
pub const SMMU_CB_MAIR_DEVICE_nGnRnE: u32 = 0x00;
/// `SMMU_CB_MAIR_DEVICE_nGnRE`.
#[allow(non_upper_case_globals)] // the C name
pub const SMMU_CB_MAIR_DEVICE_nGnRE: u32 = 0x04;
/// `SMMU_CB_MAIR_DEVICE_NC`.
pub const SMMU_CB_MAIR_DEVICE_NC: u32 = 0x44;
/// `SMMU_CB_MAIR_DEVICE_WB`.
pub const SMMU_CB_MAIR_DEVICE_WB: u32 = 0xff;
/// `SMMU_CB_MAIR_DEVICE_WT`.
pub const SMMU_CB_MAIR_DEVICE_WT: u32 = 0x88;
/// `SMMU_CB_FSR`.
pub const SMMU_CB_FSR: usize = 0x058;
/// `SMMU_CB_FSR_TF`.
pub const SMMU_CB_FSR_TF: u32 = 1 << 1;
/// `SMMU_CB_FSR_AFF`.
pub const SMMU_CB_FSR_AFF: u32 = 1 << 2;
/// `SMMU_CB_FSR_PF`.
pub const SMMU_CB_FSR_PF: u32 = 1 << 3;
/// `SMMU_CB_FSR_EF`.
pub const SMMU_CB_FSR_EF: u32 = 1 << 4;
/// `SMMU_CB_FSR_TLBMCF`.
pub const SMMU_CB_FSR_TLBMCF: u32 = 1 << 5;
/// `SMMU_CB_FSR_TLBLKF`.
pub const SMMU_CB_FSR_TLBLKF: u32 = 1 << 6;
/// `SMMU_CB_FSR_ASF`.
pub const SMMU_CB_FSR_ASF: u32 = 1 << 7;
/// `SMMU_CB_FSR_UUT`.
pub const SMMU_CB_FSR_UUT: u32 = 1 << 8;
/// `SMMU_CB_FSR_SS`.
pub const SMMU_CB_FSR_SS: u32 = 1 << 30;
/// `SMMU_CB_FSR_MULTI`.
pub const SMMU_CB_FSR_MULTI: u32 = 1 << 31;
/// `SMMU_CB_FSR_MASK`: every fault bit.
pub const SMMU_CB_FSR_MASK: u32 = SMMU_CB_FSR_TF
    | SMMU_CB_FSR_AFF
    | SMMU_CB_FSR_PF
    | SMMU_CB_FSR_EF
    | SMMU_CB_FSR_TLBMCF
    | SMMU_CB_FSR_TLBLKF
    | SMMU_CB_FSR_ASF
    | SMMU_CB_FSR_UUT
    | SMMU_CB_FSR_SS
    | SMMU_CB_FSR_MULTI;
/// `SMMU_CB_FAR`.
pub const SMMU_CB_FAR: usize = 0x060;
/// `SMMU_CB_FSYNR0`.
pub const SMMU_CB_FSYNR0: usize = 0x068;
/// `SMMU_CB_IPAFAR`.
pub const SMMU_CB_IPAFAR: usize = 0x070;
/// `SMMU_CB_TLBIVA`.
pub const SMMU_CB_TLBIVA: usize = 0x600;
/// `SMMU_CB_TLBIVAA`.
pub const SMMU_CB_TLBIVAA: usize = 0x608;
/// `SMMU_CB_TLBIASID`.
pub const SMMU_CB_TLBIASID: usize = 0x610;
/// `SMMU_CB_TLBIALL`.
pub const SMMU_CB_TLBIALL: usize = 0x618;
/// `SMMU_CB_TLBIVAL`.
pub const SMMU_CB_TLBIVAL: usize = 0x620;
/// `SMMU_CB_TLBIVAAL`.
pub const SMMU_CB_TLBIVAAL: usize = 0x628;
/// `SMMU_CB_TLBIIPAS2`.
pub const SMMU_CB_TLBIIPAS2: usize = 0x630;
/// `SMMU_CB_TLBIIPAS2L`.
pub const SMMU_CB_TLBIIPAS2L: usize = 0x638;
/// `SMMU_CB_TLBSYNC`.
pub const SMMU_CB_TLBSYNC: usize = 0x7f0;
/// `SMMU_CB_TLBSTATUS`.
pub const SMMU_CB_TLBSTATUS: usize = 0x7f4;
/// `SMMU_CB_TLBSTATUS_SACTIVE`.
pub const SMMU_CB_TLBSTATUS_SACTIVE: u32 = 1 << 0;

// SMMU v3
/// `SMMU_V3_IDR0`.
pub const SMMU_V3_IDR0: usize = 0x00;
/// `SMMU_V3_IDR0_S2P`.
pub const SMMU_V3_IDR0_S2P: u32 = 1 << 0;
/// `SMMU_V3_IDR0_S1P`.
pub const SMMU_V3_IDR0_S1P: u32 = 1 << 1;
/// `SMMU_V3_TTF_AA32`.
pub const SMMU_V3_TTF_AA32: u32 = 1 << 2;
/// `SMMU_V3_TTF_AA64`.
pub const SMMU_V3_TTF_AA64: u32 = 1 << 3;
/// `SMMU_V3_IDR0_COHACC`.
pub const SMMU_V3_IDR0_COHACC: u32 = 1 << 4;
/// `SMMU_V3_IDR0_ASID16`.
pub const SMMU_V3_IDR0_ASID16: u32 = 1 << 12;
/// `SMMU_V3_IDR0_PRI`.
pub const SMMU_V3_IDR0_PRI: u32 = 1 << 16;
/// `SMMU_V3_IDR0_VMID16`.
pub const SMMU_V3_IDR0_VMID16: u32 = 1 << 18;
/// `SMMU_V3_IDR0_CD2L`.
pub const SMMU_V3_IDR0_CD2L: u32 = 1 << 19;
/// `SMMU_V3_IDR0_ST_LEVEL(x)`.
pub const fn smmu_v3_idr0_st_level(x: u32) -> u32 {
    (x >> 27) & 0x3
}
/// `SMMU_V3_IDR0_ST_LEVEL_1`.
pub const SMMU_V3_IDR0_ST_LEVEL_1: u32 = 0x0;
/// `SMMU_V3_IDR0_ST_LEVEL_2`.
pub const SMMU_V3_IDR0_ST_LEVEL_2: u32 = 0x1;
/// `SMMU_V3_IDR1`.
pub const SMMU_V3_IDR1: usize = 0x04;
/// `SMMU_V3_IDR1_SIDSIZE(x)`.
pub const fn smmu_v3_idr1_sidsize(x: u32) -> u32 {
    x & 0x3f
}
/// `SMMU_V3_IDR1_SSIDSIZE(x)`.
pub const fn smmu_v3_idr1_ssidsize(x: u32) -> u32 {
    (x >> 6) & 0x1f
}
/// `SMMU_V3_IDR1_PRIQS(x)`.
pub const fn smmu_v3_idr1_priqs(x: u32) -> u32 {
    (x >> 11) & 0x1f
}
/// `SMMU_V3_IDR1_EVENTQS(x)`.
pub const fn smmu_v3_idr1_eventqs(x: u32) -> u32 {
    (x >> 16) & 0x1f
}
/// `SMMU_V3_IDR1_CMDQS(x)`.
pub const fn smmu_v3_idr1_cmdqs(x: u32) -> u32 {
    (x >> 21) & 0x1f
}
/// `SMMU_V3_IDR2`.
pub const SMMU_V3_IDR2: usize = 0x08;
/// `SMMU_V3_IDR3`.
pub const SMMU_V3_IDR3: usize = 0x0c;
/// `SMMU_V3_IDR3_STT`.
pub const SMMU_V3_IDR3_STT: u32 = 1 << 9;
/// `SMMU_V3_IDR4`.
pub const SMMU_V3_IDR4: usize = 0x10;
/// `SMMU_V3_IDR5`.
pub const SMMU_V3_IDR5: usize = 0x14;
/// `SMMU_V3_IDR5_OAS(x)`.
pub const fn smmu_v3_idr5_oas(x: u32) -> u32 {
    x & 0x7
}
/// `SMMU_V3_IDR5_OAS_32BIT`.
pub const SMMU_V3_IDR5_OAS_32BIT: u32 = 0x0;
/// `SMMU_V3_IDR5_OAS_36BIT`.
pub const SMMU_V3_IDR5_OAS_36BIT: u32 = 0x1;
/// `SMMU_V3_IDR5_OAS_40BIT`.
pub const SMMU_V3_IDR5_OAS_40BIT: u32 = 0x2;
/// `SMMU_V3_IDR5_OAS_42BIT`.
pub const SMMU_V3_IDR5_OAS_42BIT: u32 = 0x3;
/// `SMMU_V3_IDR5_OAS_44BIT`.
pub const SMMU_V3_IDR5_OAS_44BIT: u32 = 0x4;
/// `SMMU_V3_IDR5_OAS_48BIT`.
pub const SMMU_V3_IDR5_OAS_48BIT: u32 = 0x5;
/// `SMMU_V3_IDR5_OAS_52BIT`.
pub const SMMU_V3_IDR5_OAS_52BIT: u32 = 0x6;
/// `SMMU_V3_IDR5_VAX`.
pub const SMMU_V3_IDR5_VAX: u32 = 1 << 10;
/// `SMMU_V3_IIDR`.
pub const SMMU_V3_IIDR: usize = 0x018;
/// `SMMU_V3_AIDR`.
pub const SMMU_V3_AIDR: usize = 0x01c;
/// `SMMU_V3_CR0`.
pub const SMMU_V3_CR0: usize = 0x020;
/// `SMMU_V3_CR0_SMMUEN`.
pub const SMMU_V3_CR0_SMMUEN: u32 = 1 << 0;
/// `SMMU_V3_CR0_PRIQEN`.
pub const SMMU_V3_CR0_PRIQEN: u32 = 1 << 1;
/// `SMMU_V3_CR0_EVENTQEN`.
pub const SMMU_V3_CR0_EVENTQEN: u32 = 1 << 2;
/// `SMMU_V3_CR0_CMDQEN`.
pub const SMMU_V3_CR0_CMDQEN: u32 = 1 << 3;
/// `SMMU_V3_CR0ACK`.
pub const SMMU_V3_CR0ACK: usize = 0x24;
/// `SMMU_V3_CR1`.
pub const SMMU_V3_CR1: usize = 0x28;
/// `SMMU_V3_CR1_QUEUE_IC(x)`.
pub const fn smmu_v3_cr1_queue_ic(x: u32) -> u32 {
    x
}
/// `SMMU_V3_CR1_QUEUE_OC(x)`.
pub const fn smmu_v3_cr1_queue_oc(x: u32) -> u32 {
    x << 2
}
/// `SMMU_V3_CR1_QUEUE_SH(x)`.
pub const fn smmu_v3_cr1_queue_sh(x: u32) -> u32 {
    x << 4
}
/// `SMMU_V3_CR1_TABLE_IC(x)`.
pub const fn smmu_v3_cr1_table_ic(x: u32) -> u32 {
    x << 6
}
/// `SMMU_V3_CR1_TABLE_OC(x)`.
pub const fn smmu_v3_cr1_table_oc(x: u32) -> u32 {
    x << 8
}
/// `SMMU_V3_CR1_TABLE_SH(x)`.
pub const fn smmu_v3_cr1_table_sh(x: u32) -> u32 {
    x << 10
}
/// `SMMU_V3_CR1_CACHE_NC`.
pub const SMMU_V3_CR1_CACHE_NC: u32 = 0x0;
/// `SMMU_V3_CR1_CACHE_WB`.
pub const SMMU_V3_CR1_CACHE_WB: u32 = 0x1;
/// `SMMU_V3_CR1_CACHE_WT`.
pub const SMMU_V3_CR1_CACHE_WT: u32 = 0x2;
/// `SMMU_V3_CR1_SHARE_NSH`.
pub const SMMU_V3_CR1_SHARE_NSH: u32 = 0x0;
/// `SMMU_V3_CR1_SHARE_OSH`.
pub const SMMU_V3_CR1_SHARE_OSH: u32 = 0x2;
/// `SMMU_V3_CR1_SHARE_ISH`.
pub const SMMU_V3_CR1_SHARE_ISH: u32 = 0x3;
/// `SMMU_V3_CR2`.
pub const SMMU_V3_CR2: usize = 0x2c;
/// `SMMU_V3_CR2_E2H`.
pub const SMMU_V3_CR2_E2H: u32 = 1 << 0;
/// `SMMU_V3_CR2_RECINVSID`.
pub const SMMU_V3_CR2_RECINVSID: u32 = 1 << 1;
/// `SMMU_V3_CR2_PTM`.
pub const SMMU_V3_CR2_PTM: u32 = 1 << 2;
/// `SMMU_V3_GBPA`.
pub const SMMU_V3_GBPA: usize = 0x44;
/// `SMMU_V3_GBPA_ABORT`.
pub const SMMU_V3_GBPA_ABORT: u32 = 1 << 20;
/// `SMMU_V3_GBPA_UPDATE`.
pub const SMMU_V3_GBPA_UPDATE: u32 = 1 << 31;
/// `SMMU_V3_IRQ_CTRL`.
pub const SMMU_V3_IRQ_CTRL: usize = 0x50;
/// `SMMU_V3_IRQ_CTRL_GERROR`.
pub const SMMU_V3_IRQ_CTRL_GERROR: u32 = 1 << 0;
/// `SMMU_V3_IRQ_CTRL_PRIQ`.
pub const SMMU_V3_IRQ_CTRL_PRIQ: u32 = 1 << 1;
/// `SMMU_V3_IRQ_CTRL_EVENTQ`.
pub const SMMU_V3_IRQ_CTRL_EVENTQ: u32 = 1 << 2;
/// `SMMU_V3_IRQ_CTRLACK`.
pub const SMMU_V3_IRQ_CTRLACK: usize = 0x54;
/// `SMMU_V3_GERROR`.
pub const SMMU_V3_GERROR: usize = 0x60;
/// `SMMU_V3_GERROR_MASK`.
pub const SMMU_V3_GERROR_MASK: u32 = 0x1fd;
/// `SMMU_V3_GERROR_CMDQ_ERR`.
pub const SMMU_V3_GERROR_CMDQ_ERR: u32 = 1 << 0;
/// `SMMU_V3_GERRORN`.
pub const SMMU_V3_GERRORN: usize = 0x64;
/// `SMMU_V3_GERROR_IRQ_CFG0`.
pub const SMMU_V3_GERROR_IRQ_CFG0: usize = 0x68;
/// `SMMU_V3_STRTAB_BASE`.
pub const SMMU_V3_STRTAB_BASE: usize = 0x80;
/// `SMMU_V3_STRTAB_BASE_RA`.
pub const SMMU_V3_STRTAB_BASE_RA: u64 = 1 << 62;
/// `SMMU_V3_STRTAB_BASE_CFG`.
pub const SMMU_V3_STRTAB_BASE_CFG: usize = 0x88;
/// `SMMU_V3_STRTAB_BASE_CFG_LOG2SIZE(x)`.
pub const fn smmu_v3_strtab_base_cfg_log2size(x: u64) -> u64 {
    x
}
/// `SMMU_V3_STRTAB_BASE_CFG_SPLIT(x)`.
pub const fn smmu_v3_strtab_base_cfg_split(x: u32) -> u32 {
    x << 6
}
/// `SMMU_V3_STRTAB_BASE_CFG_FMT_L1`.
pub const SMMU_V3_STRTAB_BASE_CFG_FMT_L1: u32 = 0 << 16;
/// `SMMU_V3_STRTAB_BASE_CFG_FMT_L2`.
pub const SMMU_V3_STRTAB_BASE_CFG_FMT_L2: u32 = 1 << 16;
/// `SMMU_V3_CMDQ_BASE`.
pub const SMMU_V3_CMDQ_BASE: usize = 0x90;
/// `SMMU_V3_CMDQ_BASE_RA`.
pub const SMMU_V3_CMDQ_BASE_RA: u64 = 1 << 62;
/// `SMMU_V3_CMDQ_BASE_LOG2SIZE(x)`.
pub const fn smmu_v3_cmdq_base_log2size(x: u64) -> u64 {
    x
}
/// `SMMU_V3_CMDQ_PROD`.
pub const SMMU_V3_CMDQ_PROD: usize = 0x98;
/// `SMMU_V3_CMDQ_CONS`.
pub const SMMU_V3_CMDQ_CONS: usize = 0x9c;
/// `SMMU_V3_CMDQ_CONS_ERR(x)`.
pub const fn smmu_v3_cmdq_cons_err(x: u32) -> u32 {
    (x >> 24) & 0x7f
}
/// `SMMU_V3_EVENTQ_BASE`.
pub const SMMU_V3_EVENTQ_BASE: usize = 0xa0;
/// `SMMU_V3_EVENTQ_BASE_WA`.
pub const SMMU_V3_EVENTQ_BASE_WA: u64 = 1 << 62;
/// `SMMU_V3_EVENTQ_BASE_LOG2SIZE(x)`.
pub const fn smmu_v3_eventq_base_log2size(x: u64) -> u64 {
    x
}
/// `SMMU_V3_EVENTQ_PROD`.
pub const SMMU_V3_EVENTQ_PROD: usize = 0x100a8;
/// `SMMU_V3_EVENTQ_CONS`.
pub const SMMU_V3_EVENTQ_CONS: usize = 0x100ac;
/// `SMMU_V3_EVENTQ_IRQ_CFG0`.
pub const SMMU_V3_EVENTQ_IRQ_CFG0: usize = 0xb0;
/// `SMMU_V3_PRIQ_BASE`.
pub const SMMU_V3_PRIQ_BASE: usize = 0xc0;
/// `SMMU_V3_PRIQ_BASE_WA`.
pub const SMMU_V3_PRIQ_BASE_WA: u64 = 1 << 62;
/// `SMMU_V3_PRIQ_BASE_LOG2SIZE(x)`.
pub const fn smmu_v3_priq_base_log2size(x: u64) -> u64 {
    x
}
/// `SMMU_V3_PRIQ_PROD`.
pub const SMMU_V3_PRIQ_PROD: usize = 0x100c8;
/// `SMMU_V3_PRIQ_CONS`.
pub const SMMU_V3_PRIQ_CONS: usize = 0x100cc;
/// `SMMU_V3_PRIQ_IRQ_CFG0`.
pub const SMMU_V3_PRIQ_IRQ_CFG0: usize = 0xd0;
/// `SMMU_V3_Q_OVF(p)`.
pub const fn smmu_v3_q_ovf(p: u32) -> u32 {
    p & (1 << 31)
}
/// `SMMU_V3_Q_IDX(q, p)`.
pub const fn smmu_v3_q_idx(size_log2: u32, p: u32) -> u32 {
    p & ((1 << size_log2) - 1)
}
/// `SMMU_V3_Q_WRP(q, p)`.
pub const fn smmu_v3_q_wrp(size_log2: u32, p: u32) -> u32 {
    p & (1 << size_log2)
}
/// `SMMU_V3_CMD_CFGI_STE`.
pub const SMMU_V3_CMD_CFGI_STE: u64 = 0x03;
/// `SMMU_V3_CMD_CFGI_STE_RANGE`.
pub const SMMU_V3_CMD_CFGI_STE_RANGE: u64 = 0x04;
/// `SMMU_V3_CMD_CFGI_CD`.
pub const SMMU_V3_CMD_CFGI_CD: u64 = 0x05;
/// `SMMU_V3_CMD_CFGI_0_SID(x)`.
pub const fn smmu_v3_cmd_cfgi_0_sid(x: u64) -> u64 {
    (x & 0xffff) << 32
}
/// `SMMU_V3_CMD_CFGI_1_LEAF`.
pub const SMMU_V3_CMD_CFGI_1_LEAF: u64 = 1 << 0;
/// `SMMU_V3_CMD_CFGI_1_RANGE(x)`.
pub const fn smmu_v3_cmd_cfgi_1_range(x: u64) -> u64 {
    x & 0x1f
}
/// `SMMU_V3_CMD_TLBI_NH_ALL`.
pub const SMMU_V3_CMD_TLBI_NH_ALL: u64 = 0x10;
/// `SMMU_V3_CMD_TLBI_NH_ASID`.
pub const SMMU_V3_CMD_TLBI_NH_ASID: u64 = 0x11;
/// `SMMU_V3_CMD_TLBI_NH_VA`.
pub const SMMU_V3_CMD_TLBI_NH_VA: u64 = 0x12;
/// `SMMU_V3_CMD_TLBI_EL2_ALL`.
pub const SMMU_V3_CMD_TLBI_EL2_ALL: u64 = 0x20;
/// `SMMU_V3_CMD_TLBI_EL2_ASID`.
pub const SMMU_V3_CMD_TLBI_EL2_ASID: u64 = 0x21;
/// `SMMU_V3_CMD_TLBI_EL2_VA`.
pub const SMMU_V3_CMD_TLBI_EL2_VA: u64 = 0x22;
/// `SMMU_V3_CMD_TLBI_NSNH_ALL`.
pub const SMMU_V3_CMD_TLBI_NSNH_ALL: u64 = 0x30;
/// `SMMU_V3_CMD_TLBI_0_NUM(x)`.
pub const fn smmu_v3_cmd_tlbi_0_num(x: u64) -> u64 {
    (x & 0x3f) << 12
}
/// `SMMU_V3_CMD_TLBI_0_SCALE(x)`.
pub const fn smmu_v3_cmd_tlbi_0_scale(x: u64) -> u64 {
    (x & 0x7f) << 20
}
/// `SMMU_V3_CMD_TLBI_0_VMID(x)`.
pub const fn smmu_v3_cmd_tlbi_0_vmid(x: u64) -> u64 {
    (x & 0xffff) << 32
}
/// `SMMU_V3_CMD_TLBI_0_ASID(x)`.
pub const fn smmu_v3_cmd_tlbi_0_asid(x: u64) -> u64 {
    (x & 0xffff) << 48
}
/// `SMMU_V3_CMD_TLBI_1_LEAF`.
pub const SMMU_V3_CMD_TLBI_1_LEAF: u64 = 1 << 0;
/// `SMMU_V3_CMD_TLBI_1_TTL(x)`.
pub const fn smmu_v3_cmd_tlbi_1_ttl(x: u64) -> u64 {
    (x & 0x3) << 8
}
/// `SMMU_V3_CMD_TLBI_1_TG(x)`.
pub const fn smmu_v3_cmd_tlbi_1_tg(x: u64) -> u64 {
    (x & 0x3) << 10
}
/// `SMMU_V3_CMD_SYNC`.
pub const SMMU_V3_CMD_SYNC: u64 = 0x46;
/// `SMMU_V3_CMD_SYNC_0_CS_NONE`.
pub const SMMU_V3_CMD_SYNC_0_CS_NONE: u64 = 0 << 12;
/// `SMMU_V3_CMD_SYNC_0_CS_IRQ`.
pub const SMMU_V3_CMD_SYNC_0_CS_IRQ: u64 = 1 << 12;
/// `SMMU_V3_CMD_SYNC_0_CS_SEV`.
pub const SMMU_V3_CMD_SYNC_0_CS_SEV: u64 = 2 << 12;
/// `SMMU_V3_CMD_SYNC_0_MSH_NSH`.
pub const SMMU_V3_CMD_SYNC_0_MSH_NSH: u64 = 0x0 << 22;
/// `SMMU_V3_CMD_SYNC_0_MSH_OSH`.
pub const SMMU_V3_CMD_SYNC_0_MSH_OSH: u64 = 0x2 << 22;
/// `SMMU_V3_CMD_SYNC_0_MSH_ISH`.
pub const SMMU_V3_CMD_SYNC_0_MSH_ISH: u64 = 0x3 << 22;
/// `SMMU_V3_CMD_SYNC_0_MSIATTR_OIWB`.
pub const SMMU_V3_CMD_SYNC_0_MSIATTR_OIWB: u64 = 0xf << 24;
/// `SMMU_V3_CD_0_TCR_T0SZ(x)`.
pub const fn smmu_v3_cd_0_tcr_t0sz(x: u64) -> u64 {
    x & 0x3f
}
/// `SMMU_V3_CD_0_TCR_TG0_4KB`.
pub const SMMU_V3_CD_0_TCR_TG0_4KB: u64 = 0x0 << 6;
/// `SMMU_V3_CD_0_TCR_TG0_64KB`.
pub const SMMU_V3_CD_0_TCR_TG0_64KB: u64 = 0x1 << 6;
/// `SMMU_V3_CD_0_TCR_TG0_16KB`.
pub const SMMU_V3_CD_0_TCR_TG0_16KB: u64 = 0x2 << 6;
/// `SMMU_V3_CD_0_TCR_IRGN0_NC`.
pub const SMMU_V3_CD_0_TCR_IRGN0_NC: u64 = 0x0 << 8;
/// `SMMU_V3_CD_0_TCR_IRGN0_WBWA`.
pub const SMMU_V3_CD_0_TCR_IRGN0_WBWA: u64 = 0x1 << 8;
/// `SMMU_V3_CD_0_TCR_IRGN0_WT`.
pub const SMMU_V3_CD_0_TCR_IRGN0_WT: u64 = 0x2 << 8;
/// `SMMU_V3_CD_0_TCR_IRGN0_WB`.
pub const SMMU_V3_CD_0_TCR_IRGN0_WB: u64 = 0x3 << 8;
/// `SMMU_V3_CD_0_TCR_ORGN0_NC`.
pub const SMMU_V3_CD_0_TCR_ORGN0_NC: u64 = 0x0 << 10;
/// `SMMU_V3_CD_0_TCR_ORGN0_WBWA`.
pub const SMMU_V3_CD_0_TCR_ORGN0_WBWA: u64 = 0x1 << 10;
/// `SMMU_V3_CD_0_TCR_ORGN0_WT`.
pub const SMMU_V3_CD_0_TCR_ORGN0_WT: u64 = 0x2 << 10;
/// `SMMU_V3_CD_0_TCR_ORGN0_WB`.
pub const SMMU_V3_CD_0_TCR_ORGN0_WB: u64 = 0x3 << 10;
/// `SMMU_V3_CD_0_TCR_SH0_NSH`.
pub const SMMU_V3_CD_0_TCR_SH0_NSH: u64 = 0x0 << 12;
/// `SMMU_V3_CD_0_TCR_SH0_OSH`.
pub const SMMU_V3_CD_0_TCR_SH0_OSH: u64 = 0x2 << 12;
/// `SMMU_V3_CD_0_TCR_SH0_ISH`.
pub const SMMU_V3_CD_0_TCR_SH0_ISH: u64 = 0x3 << 12;
/// `SMMU_V3_CD_0_TCR_EPD0`.
pub const SMMU_V3_CD_0_TCR_EPD0: u64 = 1 << 14;
/// `SMMU_V3_CD_0_ENDI`.
pub const SMMU_V3_CD_0_ENDI: u64 = 1 << 15;
/// `SMMU_V3_CD_0_TCR_EPD1`.
pub const SMMU_V3_CD_0_TCR_EPD1: u64 = 1 << 30;
/// `SMMU_V3_CD_0_V`.
pub const SMMU_V3_CD_0_V: u64 = 1 << 31;
/// `SMMU_V3_CD_0_TCR_IPS_32BIT`.
pub const SMMU_V3_CD_0_TCR_IPS_32BIT: u64 = 0x0 << 32;
/// `SMMU_V3_CD_0_TCR_IPS_36BIT`.
pub const SMMU_V3_CD_0_TCR_IPS_36BIT: u64 = 0x1 << 32;
/// `SMMU_V3_CD_0_TCR_IPS_40BIT`.
pub const SMMU_V3_CD_0_TCR_IPS_40BIT: u64 = 0x2 << 32;
/// `SMMU_V3_CD_0_TCR_IPS_42BIT`.
pub const SMMU_V3_CD_0_TCR_IPS_42BIT: u64 = 0x3 << 32;
/// `SMMU_V3_CD_0_TCR_IPS_44BIT`.
pub const SMMU_V3_CD_0_TCR_IPS_44BIT: u64 = 0x4 << 32;
/// `SMMU_V3_CD_0_TCR_IPS_48BIT`.
pub const SMMU_V3_CD_0_TCR_IPS_48BIT: u64 = 0x5 << 32;
/// `SMMU_V3_CD_0_TCR_IPS_52BIT`.
pub const SMMU_V3_CD_0_TCR_IPS_52BIT: u64 = 0x6 << 32;
/// `SMMU_V3_CD_0_TCR_TBI0`.
pub const SMMU_V3_CD_0_TCR_TBI0: u64 = 1 << 38;
/// `SMMU_V3_CD_0_AA64`.
pub const SMMU_V3_CD_0_AA64: u64 = 1 << 41;
/// `SMMU_V3_CD_0_TCR_HD`.
pub const SMMU_V3_CD_0_TCR_HD: u64 = 1 << 42;
/// `SMMU_V3_CD_0_TCR_HA`.
pub const SMMU_V3_CD_0_TCR_HA: u64 = 1 << 43;
/// `SMMU_V3_CD_0_S`.
pub const SMMU_V3_CD_0_S: u64 = 1 << 44;
/// `SMMU_V3_CD_0_R`.
pub const SMMU_V3_CD_0_R: u64 = 1 << 45;
/// `SMMU_V3_CD_0_A`.
pub const SMMU_V3_CD_0_A: u64 = 1 << 46;
/// `SMMU_V3_CD_0_ASET`.
pub const SMMU_V3_CD_0_ASET: u64 = 1 << 47;
/// `SMMU_V3_CD_0_ASID(x)`.
pub const fn smmu_v3_cd_0_asid(x: u64) -> u64 {
    (x & 0xffff) << 48
}
/// `SMMU_V3_CD_3_MAIR_ATTR(attr, idx)`.
pub const fn smmu_v3_cd_3_mair_attr(attr: u64, idx: u64) -> u64 {
    attr << (idx * 8)
}
/// `SMMU_V3_CD_3_MAIR_DEVICE_nGnRnE`.
#[allow(non_upper_case_globals)] // the C name
pub const SMMU_V3_CD_3_MAIR_DEVICE_nGnRnE: u64 = 0x00;
/// `SMMU_V3_CD_3_MAIR_DEVICE_nGnRE`.
#[allow(non_upper_case_globals)] // the C name
pub const SMMU_V3_CD_3_MAIR_DEVICE_nGnRE: u64 = 0x04;
/// `SMMU_V3_CD_3_MAIR_DEVICE_NC`.
pub const SMMU_V3_CD_3_MAIR_DEVICE_NC: u64 = 0x44;
/// `SMMU_V3_CD_3_MAIR_DEVICE_WB`.
pub const SMMU_V3_CD_3_MAIR_DEVICE_WB: u64 = 0xff;
/// `SMMU_V3_CD_3_MAIR_DEVICE_WT`.
pub const SMMU_V3_CD_3_MAIR_DEVICE_WT: u64 = 0x88;
/// `SMMU_V3_STE_0_V`.
pub const SMMU_V3_STE_0_V: u64 = 1 << 0;
/// `SMMU_V3_STE_0_CFG_ABORT`.
pub const SMMU_V3_STE_0_CFG_ABORT: u64 = 0 << 1;
/// `SMMU_V3_STE_0_CFG_BYPASS`.
pub const SMMU_V3_STE_0_CFG_BYPASS: u64 = 4 << 1;
/// `SMMU_V3_STE_0_CFG_S1_TRANS`.
pub const SMMU_V3_STE_0_CFG_S1_TRANS: u64 = 5 << 1;
/// `SMMU_V3_STE_0_CFG_S2_TRANS`.
pub const SMMU_V3_STE_0_CFG_S2_TRANS: u64 = 6 << 1;
/// `SMMU_V3_STE_0_CFG_NESTED`.
pub const SMMU_V3_STE_0_CFG_NESTED: u64 = 7 << 1;
/// `SMMU_V3_STE_0_S1FMT_LINEAR`.
pub const SMMU_V3_STE_0_S1FMT_LINEAR: u64 = 0 << 4;
/// `SMMU_V3_STE_0_S1FMT_64K_L2`.
pub const SMMU_V3_STE_0_S1FMT_64K_L2: u64 = 2 << 4;
/// `SMMU_V3_STE_1_S1DSS_TERMINATE`.
pub const SMMU_V3_STE_1_S1DSS_TERMINATE: u64 = 0 << 0;
/// `SMMU_V3_STE_1_S1DSS_BYPASS`.
pub const SMMU_V3_STE_1_S1DSS_BYPASS: u64 = 1 << 0;
/// `SMMU_V3_STE_1_S1DSS_SSID0`.
pub const SMMU_V3_STE_1_S1DSS_SSID0: u64 = 2 << 0;
/// `SMMU_V3_STE_1_S1CIR_NC`.
pub const SMMU_V3_STE_1_S1CIR_NC: u64 = 0 << 2;
/// `SMMU_V3_STE_1_S1CIR_WBRA`.
pub const SMMU_V3_STE_1_S1CIR_WBRA: u64 = 1 << 2;
/// `SMMU_V3_STE_1_S1CIR_WT`.
pub const SMMU_V3_STE_1_S1CIR_WT: u64 = 2 << 2;
/// `SMMU_V3_STE_1_S1CIR_WB`.
pub const SMMU_V3_STE_1_S1CIR_WB: u64 = 3 << 2;
/// `SMMU_V3_STE_1_S1COR_NC`.
pub const SMMU_V3_STE_1_S1COR_NC: u64 = 0 << 4;
/// `SMMU_V3_STE_1_S1COR_WBRA`.
pub const SMMU_V3_STE_1_S1COR_WBRA: u64 = 1 << 4;
/// `SMMU_V3_STE_1_S1COR_WT`.
pub const SMMU_V3_STE_1_S1COR_WT: u64 = 2 << 4;
/// `SMMU_V3_STE_1_S1COR_WB`.
pub const SMMU_V3_STE_1_S1COR_WB: u64 = 3 << 4;
/// `SMMU_V3_STE_1_S1CSH_NSH`.
pub const SMMU_V3_STE_1_S1CSH_NSH: u64 = 0 << 6;
/// `SMMU_V3_STE_1_S1CSH_OSH`.
pub const SMMU_V3_STE_1_S1CSH_OSH: u64 = 2 << 6;
/// `SMMU_V3_STE_1_S1CSH_ISH`.
pub const SMMU_V3_STE_1_S1CSH_ISH: u64 = 3 << 6;
/// `SMMU_V3_STE_1_S2FWB`.
pub const SMMU_V3_STE_1_S2FWB: u64 = 1 << 25;
/// `SMMU_V3_STE_1_S1STALLD`.
pub const SMMU_V3_STE_1_S1STALLD: u64 = 1 << 27;
/// `SMMU_V3_STE_1_EATS_ABT`.
pub const SMMU_V3_STE_1_EATS_ABT: u64 = 0 << 28;
/// `SMMU_V3_STE_1_EATS_TRANS`.
pub const SMMU_V3_STE_1_EATS_TRANS: u64 = 1 << 28;
/// `SMMU_V3_STE_1_EATS_S1CHK`.
pub const SMMU_V3_STE_1_EATS_S1CHK: u64 = 2 << 28;
/// `SMMU_V3_STE_1_STRW_NSEL1`.
pub const SMMU_V3_STE_1_STRW_NSEL1: u64 = 0 << 30;
/// `SMMU_V3_STE_1_STRW_EL2`.
pub const SMMU_V3_STE_1_STRW_EL2: u64 = 2 << 30;
/// `SMMU_V3_STE_1_SHCFG_INCOMING`.
pub const SMMU_V3_STE_1_SHCFG_INCOMING: u64 = 1 << 44;
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use std::assert_eq;

    use super::*;

    #[test]
    fn idr_fields() {
        // QEMU-like SMMUv3 IDR1: SIDSIZE 16, CMDQS 19, EVENTQS 19, PRIQS 0.
        let idr1 = 16 | (19 << 21) | (19 << 16);
        assert_eq!(smmu_v3_idr1_sidsize(idr1), 16);
        assert_eq!(smmu_v3_idr1_cmdqs(idr1), 19);
        assert_eq!(smmu_v3_idr1_eventqs(idr1), 19);
        assert_eq!(smmu_v3_idr1_priqs(idr1), 0);
        assert_eq!(smmu_v3_idr0_st_level(1 << 27), SMMU_V3_IDR0_ST_LEVEL_2);
        // SMMUv2 IDR1: 8 context banks, 2 stage-2 only, NUMPAGENDXB 3.
        let idr1 = 8 | (2 << 16) | (3 << 28);
        assert_eq!(smmu_idr1_numcb(idr1), 8);
        assert_eq!(smmu_idr1_nums2cb(idr1), 2);
        assert_eq!(smmu_idr1_numpagendxb(idr1), 3);
        assert_eq!(smmu_idr2_exnumsmrg(0x7ff << 16), 0x7ff);
    }

    #[test]
    fn register_arrays() {
        assert_eq!(smmu_smr(0), 0x800);
        assert_eq!(smmu_smr(127), 0x800 + 127 * 4);
        assert_eq!(smmu_s2cr(3), 0xc0c);
        assert_eq!(smmu_cbar(2), 0x8);
        assert_eq!(smmu_cbfrsynra(1), 0x404);
        assert_eq!(smmu_cba2r(1), 0x804);
    }

    #[test]
    fn queue_pointers() {
        // A 4-entry queue: index in bits 0-1, wrap bit 2, overflow bit 31.
        assert_eq!(smmu_v3_q_idx(2, 0b111), 0b11);
        assert_eq!(smmu_v3_q_wrp(2, 0b111), 0b100);
        assert_eq!(smmu_v3_q_ovf(0x8000_0001), 0x8000_0000);
    }

    #[test]
    fn command_and_descriptor_fields() {
        assert_eq!(
            SMMU_V3_CMD_CFGI_STE | smmu_v3_cmd_cfgi_0_sid(0x10008),
            0x0000_0008_0000_0003
        );
        assert_eq!(smmu_v3_cmd_cfgi_1_range(31), 31);
        assert_eq!(smmu_v3_cmd_tlbi_0_asid(0x1234), 0x1234 << 48);
        assert_eq!(smmu_v3_cd_0_tcr_t0sz(64 - 48), 16);
        assert_eq!(smmu_v3_cd_0_asid(0x1_0001), 1 << 48);
        assert_eq!(
            smmu_v3_cd_3_mair_attr(SMMU_V3_CD_3_MAIR_DEVICE_WT, 4),
            0x88 << 32
        );
        assert_eq!(
            smmu_cb_mair_mair_attr(SMMU_CB_MAIR_DEVICE_WB, 3),
            0xff << 24
        );
        assert_eq!(SMMU_V3_STE_0_CFG_S1_TRANS, 0xa);
        assert_eq!(smmu_v3_strtab_base_cfg_log2size(16), 16);
        assert_eq!(smmu_v3_strtab_base_cfg_split(8), 8 << 6);
    }

    /// Every object-like define of the header, against the C (`just test-ref`).
    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn defines_match_reference() {
        let defs = crate::reftest::defines("sys/arch/arm64/dev/smmureg.h");
        let mut ours = crate::reftest::assert_defines!(defs; SMMU_SCR0, SMMU_SCR0_CLIENTPD, SMMU_SCR0_GFRE, SMMU_SCR0_GFIE, SMMU_SCR0_EXIDENABLE, SMMU_SCR0_GCFGFRE, SMMU_SCR0_GCFGFIE, SMMU_SCR0_USFCFG, SMMU_SCR0_VMIDPNE, SMMU_SCR0_PTM, SMMU_SCR0_FB, SMMU_SCR0_BSU_MASK, SMMU_SCR0_VMID16EN, SMMU_SCR1, SMMU_SCR2, SMMU_SACR, SMMU_SACR_MMU500_SMTNMB_TLBEN, SMMU_SACR_MMU500_S2CRB_TLBEN, SMMU_SACR_MMU500_CACHE_LOCK, SMMU_IDR0, fn, SMMU_IDR0_EXIDS, fn, SMMU_IDR0_BTM, SMMU_IDR0_CCTM, SMMU_IDR0_EXSMRGS, fn, fn, SMMU_IDR0_PTFS_AARCH32_SHORT_AND_LONG, SMMU_IDR0_PTFS_AARCH32_ONLY_LONG, SMMU_IDR0_PTFS_AARCH32_NO, SMMU_IDR0_PTFS_AARCH32_RES, SMMU_IDR0_ATOSNS, SMMU_IDR0_SMS, SMMU_IDR0_NTS, SMMU_IDR0_S2TS, SMMU_IDR0_S1TS, SMMU_IDR0_SES, SMMU_IDR1, fn, fn, fn, SMMU_IDR1_SSDTP_UNK, SMMU_IDR1_SSDTP_IDX_NUMSSDNDXB, SMMU_IDR1_SSDTP_RES, SMMU_IDR1_SSDTP_IDX_16BIT, SMMU_IDR1_SMCD, fn, fn, SMMU_IDR1_HAFDBS_NO, SMMU_IDR1_HAFDBS_AF, SMMU_IDR1_HAFDBS_RES, SMMU_IDR1_HAFDBS_AFDB, fn, SMMU_IDR1_PAGESIZE_4K, SMMU_IDR1_PAGESIZE_64K, SMMU_IDR2, fn, SMMU_IDR2_IAS_32BIT, SMMU_IDR2_IAS_36BIT, SMMU_IDR2_IAS_40BIT, SMMU_IDR2_IAS_42BIT, SMMU_IDR2_IAS_44BIT, SMMU_IDR2_IAS_48BIT, fn, SMMU_IDR2_OAS_32BIT, SMMU_IDR2_OAS_36BIT, SMMU_IDR2_OAS_40BIT, SMMU_IDR2_OAS_42BIT, SMMU_IDR2_OAS_44BIT, SMMU_IDR2_OAS_48BIT, fn, SMMU_IDR2_UBS_32BIT, SMMU_IDR2_UBS_36BIT, SMMU_IDR2_UBS_40BIT, SMMU_IDR2_UBS_42BIT, SMMU_IDR2_UBS_44BIT, SMMU_IDR2_UBS_49BIT, SMMU_IDR2_UBS_64BIT, SMMU_IDR2_PTFSV8_4KB, SMMU_IDR2_PTFSV8_16KB, SMMU_IDR2_PTFSV8_64KB, SMMU_IDR2_VMID16S, fn, SMMU_IDR2_E2HS, SMMU_IDR2_HADS, SMMU_IDR2_COMPINDEXS, SMMU_IDR2_DIPANS, SMMU_IDR3, SMMU_IDR4, SMMU_IDR5, SMMU_IDR6, SMMU_IDR7, fn, fn, SMMU_SGFSR, SMMU_SGFSYNR0, SMMU_SGFSYNR1, SMMU_SGFSYNR2, SMMU_TLBIVMID, SMMU_TLBIALLNSNH, SMMU_TLBIALLH, SMMU_STLBGSYNC, SMMU_STLBGSTATUS, SMMU_STLBGSTATUS_GSACTIVE, fn, SMMU_SMR_ID_SHIFT, SMMU_SMR_ID_MASK, SMMU_SMR_MASK_SHIFT, SMMU_SMR_MASK_MASK, SMMU_SMR_VALID, fn, SMMU_S2CR_EXIDVALID, SMMU_S2CR_TYPE_TRANS, SMMU_S2CR_TYPE_BYPASS, SMMU_S2CR_TYPE_FAULT, SMMU_S2CR_TYPE_MASK, fn, SMMU_CBAR_VMID_SHIFT, SMMU_CBAR_BPSHCFG_RES, SMMU_CBAR_BPSHCFG_OSH, SMMU_CBAR_BPSHCFG_ISH, SMMU_CBAR_BPSHCFG_NSH, SMMU_CBAR_MEMATTR_WB, SMMU_CBAR_TYPE_S2_TRANS, SMMU_CBAR_TYPE_S1_TRANS_S2_BYPASS, SMMU_CBAR_TYPE_S1_TRANS_S2_FAULT, SMMU_CBAR_TYPE_S1_TRANS_S2_TRANS, SMMU_CBAR_TYPE_MASK, SMMU_CBAR_IRPTNDX_SHIFT, fn, fn, SMMU_CBA2R_VA64, SMMU_CBA2R_MONC, SMMU_CBA2R_VMID16_SHIFT, SMMU_CB_SCTLR, SMMU_CB_SCTLR_M, SMMU_CB_SCTLR_TRE, SMMU_CB_SCTLR_AFE, SMMU_CB_SCTLR_CFRE, SMMU_CB_SCTLR_CFIE, SMMU_CB_SCTLR_ASIDPNE, SMMU_CB_ACTLR, SMMU_CB_ACTLR_CPRE, SMMU_CB_TCR2, SMMU_CB_TCR2_PASIZE_32BIT, SMMU_CB_TCR2_PASIZE_36BIT, SMMU_CB_TCR2_PASIZE_40BIT, SMMU_CB_TCR2_PASIZE_42BIT, SMMU_CB_TCR2_PASIZE_44BIT, SMMU_CB_TCR2_PASIZE_48BIT, SMMU_CB_TCR2_PASIZE_MASK, SMMU_CB_TCR2_AS, SMMU_CB_TCR2_SEP_UPSTREAM, SMMU_CB_TTBR0, SMMU_CB_TTBR1, SMMU_CB_TTBR_ASID_SHIFT, SMMU_CB_TCR, fn, SMMU_CB_TCR_EPD0, SMMU_CB_TCR_IRGN0_NC, SMMU_CB_TCR_IRGN0_WBWA, SMMU_CB_TCR_IRGN0_WT, SMMU_CB_TCR_IRGN0_WB, SMMU_CB_TCR_ORGN0_NC, SMMU_CB_TCR_ORGN0_WBWA, SMMU_CB_TCR_ORGN0_WT, SMMU_CB_TCR_ORGN0_WB, SMMU_CB_TCR_SH0_NSH, SMMU_CB_TCR_SH0_OSH, SMMU_CB_TCR_SH0_ISH, SMMU_CB_TCR_TG0_4KB, SMMU_CB_TCR_TG0_64KB, SMMU_CB_TCR_TG0_16KB, SMMU_CB_TCR_TG0_MASK, fn, SMMU_CB_TCR_EPD1, SMMU_CB_TCR_IRGN1_NC, SMMU_CB_TCR_IRGN1_WBWA, SMMU_CB_TCR_IRGN1_WT, SMMU_CB_TCR_IRGN1_WB, SMMU_CB_TCR_ORGN1_NC, SMMU_CB_TCR_ORGN1_WBWA, SMMU_CB_TCR_ORGN1_WT, SMMU_CB_TCR_ORGN1_WB, SMMU_CB_TCR_SH1_NSH, SMMU_CB_TCR_SH1_OSH, SMMU_CB_TCR_SH1_ISH, SMMU_CB_TCR_TG1_16KB, SMMU_CB_TCR_TG1_4KB, SMMU_CB_TCR_TG1_64KB, SMMU_CB_TCR_TG1_MASK, SMMU_CB_TCR_S2_SL0_4KB_L2, SMMU_CB_TCR_S2_SL0_4KB_L1, SMMU_CB_TCR_S2_SL0_4KB_L0, SMMU_CB_TCR_S2_SL0_16KB_L3, SMMU_CB_TCR_S2_SL0_16KB_L2, SMMU_CB_TCR_S2_SL0_16KB_L1, SMMU_CB_TCR_S2_SL0_64KB_L3, SMMU_CB_TCR_S2_SL0_64KB_L2, SMMU_CB_TCR_S2_SL0_64KB_L1, SMMU_CB_TCR_S2_SL0_MASK, SMMU_CB_TCR_S2_PASIZE_32BIT, SMMU_CB_TCR_S2_PASIZE_36BIT, SMMU_CB_TCR_S2_PASIZE_40BIT, SMMU_CB_TCR_S2_PASIZE_42BIT, SMMU_CB_TCR_S2_PASIZE_44BIT, SMMU_CB_TCR_S2_PASIZE_48BIT, SMMU_CB_TCR_S2_PASIZE_MASK, SMMU_CB_MAIR0, SMMU_CB_MAIR1, fn, SMMU_CB_MAIR_DEVICE_nGnRnE, SMMU_CB_MAIR_DEVICE_nGnRE, SMMU_CB_MAIR_DEVICE_NC, SMMU_CB_MAIR_DEVICE_WB, SMMU_CB_MAIR_DEVICE_WT, SMMU_CB_FSR, SMMU_CB_FSR_TF, SMMU_CB_FSR_AFF, SMMU_CB_FSR_PF, SMMU_CB_FSR_EF, SMMU_CB_FSR_TLBMCF, SMMU_CB_FSR_TLBLKF, SMMU_CB_FSR_ASF, SMMU_CB_FSR_UUT, SMMU_CB_FSR_SS, SMMU_CB_FSR_MULTI, SMMU_CB_FAR, SMMU_CB_FSYNR0, SMMU_CB_IPAFAR, SMMU_CB_TLBIVA, SMMU_CB_TLBIVAA, SMMU_CB_TLBIASID, SMMU_CB_TLBIALL, SMMU_CB_TLBIVAL, SMMU_CB_TLBIVAAL, SMMU_CB_TLBIIPAS2, SMMU_CB_TLBIIPAS2L, SMMU_CB_TLBSYNC, SMMU_CB_TLBSTATUS, SMMU_CB_TLBSTATUS_SACTIVE, SMMU_V3_IDR0, SMMU_V3_IDR0_S2P, SMMU_V3_IDR0_S1P, SMMU_V3_TTF_AA32, SMMU_V3_TTF_AA64, SMMU_V3_IDR0_COHACC, SMMU_V3_IDR0_ASID16, SMMU_V3_IDR0_PRI, SMMU_V3_IDR0_VMID16, SMMU_V3_IDR0_CD2L, fn, SMMU_V3_IDR0_ST_LEVEL_1, SMMU_V3_IDR0_ST_LEVEL_2, SMMU_V3_IDR1, fn, fn, fn, fn, fn, SMMU_V3_IDR2, SMMU_V3_IDR3, SMMU_V3_IDR3_STT, SMMU_V3_IDR4, SMMU_V3_IDR5, fn, SMMU_V3_IDR5_OAS_32BIT, SMMU_V3_IDR5_OAS_36BIT, SMMU_V3_IDR5_OAS_40BIT, SMMU_V3_IDR5_OAS_42BIT, SMMU_V3_IDR5_OAS_44BIT, SMMU_V3_IDR5_OAS_48BIT, SMMU_V3_IDR5_OAS_52BIT, SMMU_V3_IDR5_VAX, SMMU_V3_IIDR, SMMU_V3_AIDR, SMMU_V3_CR0, SMMU_V3_CR0_SMMUEN, SMMU_V3_CR0_PRIQEN, SMMU_V3_CR0_EVENTQEN, SMMU_V3_CR0_CMDQEN, SMMU_V3_CR0ACK, SMMU_V3_CR1, fn, fn, fn, fn, fn, fn, SMMU_V3_CR1_CACHE_NC, SMMU_V3_CR1_CACHE_WB, SMMU_V3_CR1_CACHE_WT, SMMU_V3_CR1_SHARE_NSH, SMMU_V3_CR1_SHARE_OSH, SMMU_V3_CR1_SHARE_ISH, SMMU_V3_CR2, SMMU_V3_CR2_E2H, SMMU_V3_CR2_RECINVSID, SMMU_V3_CR2_PTM, SMMU_V3_GBPA, SMMU_V3_GBPA_ABORT, SMMU_V3_GBPA_UPDATE, SMMU_V3_IRQ_CTRL, SMMU_V3_IRQ_CTRL_GERROR, SMMU_V3_IRQ_CTRL_PRIQ, SMMU_V3_IRQ_CTRL_EVENTQ, SMMU_V3_IRQ_CTRLACK, SMMU_V3_GERROR, SMMU_V3_GERROR_MASK, SMMU_V3_GERROR_CMDQ_ERR, SMMU_V3_GERRORN, SMMU_V3_GERROR_IRQ_CFG0, SMMU_V3_STRTAB_BASE, SMMU_V3_STRTAB_BASE_RA, SMMU_V3_STRTAB_BASE_CFG, fn, fn, SMMU_V3_STRTAB_BASE_CFG_FMT_L1, SMMU_V3_STRTAB_BASE_CFG_FMT_L2, SMMU_V3_CMDQ_BASE, SMMU_V3_CMDQ_BASE_RA, fn, SMMU_V3_CMDQ_PROD, SMMU_V3_CMDQ_CONS, fn, SMMU_V3_EVENTQ_BASE, SMMU_V3_EVENTQ_BASE_WA, fn, SMMU_V3_EVENTQ_PROD, SMMU_V3_EVENTQ_CONS, SMMU_V3_EVENTQ_IRQ_CFG0, SMMU_V3_PRIQ_BASE, SMMU_V3_PRIQ_BASE_WA, fn, SMMU_V3_PRIQ_PROD, SMMU_V3_PRIQ_CONS, SMMU_V3_PRIQ_IRQ_CFG0, fn, fn, fn, SMMU_V3_CMD_CFGI_STE, SMMU_V3_CMD_CFGI_STE_RANGE, SMMU_V3_CMD_CFGI_CD, fn, SMMU_V3_CMD_CFGI_1_LEAF, fn, SMMU_V3_CMD_TLBI_NH_ALL, SMMU_V3_CMD_TLBI_NH_ASID, SMMU_V3_CMD_TLBI_NH_VA, SMMU_V3_CMD_TLBI_EL2_ALL, SMMU_V3_CMD_TLBI_EL2_ASID, SMMU_V3_CMD_TLBI_EL2_VA, SMMU_V3_CMD_TLBI_NSNH_ALL, fn, fn, fn, fn, SMMU_V3_CMD_TLBI_1_LEAF, fn, fn, SMMU_V3_CMD_SYNC, SMMU_V3_CMD_SYNC_0_CS_NONE, SMMU_V3_CMD_SYNC_0_CS_IRQ, SMMU_V3_CMD_SYNC_0_CS_SEV, SMMU_V3_CMD_SYNC_0_MSH_NSH, SMMU_V3_CMD_SYNC_0_MSH_OSH, SMMU_V3_CMD_SYNC_0_MSH_ISH, SMMU_V3_CMD_SYNC_0_MSIATTR_OIWB, fn, SMMU_V3_CD_0_TCR_TG0_4KB, SMMU_V3_CD_0_TCR_TG0_64KB, SMMU_V3_CD_0_TCR_TG0_16KB, SMMU_V3_CD_0_TCR_IRGN0_NC, SMMU_V3_CD_0_TCR_IRGN0_WBWA, SMMU_V3_CD_0_TCR_IRGN0_WT, SMMU_V3_CD_0_TCR_IRGN0_WB, SMMU_V3_CD_0_TCR_ORGN0_NC, SMMU_V3_CD_0_TCR_ORGN0_WBWA, SMMU_V3_CD_0_TCR_ORGN0_WT, SMMU_V3_CD_0_TCR_ORGN0_WB, SMMU_V3_CD_0_TCR_SH0_NSH, SMMU_V3_CD_0_TCR_SH0_OSH, SMMU_V3_CD_0_TCR_SH0_ISH, SMMU_V3_CD_0_TCR_EPD0, SMMU_V3_CD_0_ENDI, SMMU_V3_CD_0_TCR_EPD1, SMMU_V3_CD_0_V, SMMU_V3_CD_0_TCR_IPS_32BIT, SMMU_V3_CD_0_TCR_IPS_36BIT, SMMU_V3_CD_0_TCR_IPS_40BIT, SMMU_V3_CD_0_TCR_IPS_42BIT, SMMU_V3_CD_0_TCR_IPS_44BIT, SMMU_V3_CD_0_TCR_IPS_48BIT, SMMU_V3_CD_0_TCR_IPS_52BIT, SMMU_V3_CD_0_TCR_TBI0, SMMU_V3_CD_0_AA64, SMMU_V3_CD_0_TCR_HD, SMMU_V3_CD_0_TCR_HA, SMMU_V3_CD_0_S, SMMU_V3_CD_0_R, SMMU_V3_CD_0_A, SMMU_V3_CD_0_ASET, fn, fn, SMMU_V3_CD_3_MAIR_DEVICE_nGnRnE, SMMU_V3_CD_3_MAIR_DEVICE_nGnRE, SMMU_V3_CD_3_MAIR_DEVICE_NC, SMMU_V3_CD_3_MAIR_DEVICE_WB, SMMU_V3_CD_3_MAIR_DEVICE_WT, SMMU_V3_STE_0_V, SMMU_V3_STE_0_CFG_ABORT, SMMU_V3_STE_0_CFG_BYPASS, SMMU_V3_STE_0_CFG_S1_TRANS, SMMU_V3_STE_0_CFG_S2_TRANS, SMMU_V3_STE_0_CFG_NESTED, SMMU_V3_STE_0_S1FMT_LINEAR, SMMU_V3_STE_0_S1FMT_64K_L2, SMMU_V3_STE_1_S1DSS_TERMINATE, SMMU_V3_STE_1_S1DSS_BYPASS, SMMU_V3_STE_1_S1DSS_SSID0, SMMU_V3_STE_1_S1CIR_NC, SMMU_V3_STE_1_S1CIR_WBRA, SMMU_V3_STE_1_S1CIR_WT, SMMU_V3_STE_1_S1CIR_WB, SMMU_V3_STE_1_S1COR_NC, SMMU_V3_STE_1_S1COR_WBRA, SMMU_V3_STE_1_S1COR_WT, SMMU_V3_STE_1_S1COR_WB, SMMU_V3_STE_1_S1CSH_NSH, SMMU_V3_STE_1_S1CSH_OSH, SMMU_V3_STE_1_S1CSH_ISH, SMMU_V3_STE_1_S2FWB, SMMU_V3_STE_1_S1STALLD, SMMU_V3_STE_1_EATS_ABT, SMMU_V3_STE_1_EATS_TRANS, SMMU_V3_STE_1_EATS_S1CHK, SMMU_V3_STE_1_STRW_NSEL1, SMMU_V3_STE_1_STRW_EL2, SMMU_V3_STE_1_SHCFG_INCOMING);
        // A multi-line define the evaluator does not read, and the C's broken macro.
        ours.push("SMMU_CB_FSR_MASK");
        ours.push("SMMU_IDR2_EXNUMSMRG");
        crate::reftest::assert_complete(&defs, "SMMU_", &ours);
    }
}
/* </TESTS> */
