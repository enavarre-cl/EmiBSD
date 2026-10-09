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
//! The kernel's 32-bit entry from boot(8) (efiboot): `arch/amd64/amd64/locore0.S`, pulled in
//! from the `.S` file next to this module (the file keeps OpenBSD's licence blocks and layout;
//! `{NAME}` placeholders are what `assym.h` and the headers provide in C).
//!
//! Upstream: sys/arch/amd64/amd64/locore0.S @ 3ce1f3f79392
//!
//! boot(8) loads the kernel at its physical addresses (from `0x1000000`, `AT()` in
//! `conf/kernel.ld`) and calls `start` in 32-bit protected mode, paging off, with its
//! arguments on the stack. `start` saves them (`boothowto`, `bootdev`, `esym`,
//! `biosbasemem`, `bootapiver`) and copies the `bootarg` list into `bootinfo[]`, probes the
//! CPU, builds the bootstrap page tables after the kernel and its symbols (the kernel image
//! at `KERNBASE`, an identity map of it, the first 4 GB of the direct map, the recursive
//! slot), enables PAE, long mode, NX and paging, jumps to the higher half and drops the
//! identity map. Limine does not come here: its entry is `_start` (`sys/stand/mod.rs`).
//!
//! ## Deviations
//! - AT&T syntax through `global_asm!` without the C preprocessor: `RELOC(x)` is written
//!   `x - {KERNBASE}`, the `fillkpt` macros are `.macro`s, `#if` is `.if`; the values of
//!   `assym.h` and the headers are `const` operands, the variables `sym` operands.
//! - The Meltdown probe is not here: `cpu_meltdown` is not ported and `pg_g_kern` stays 0
//!   (the pmap has no `tlbflushg` for global kernel pages). Neither is the AMD SEV probe
//!   (`AMDSEV` is not configured): `pg_crypt` stays 0 and the frame masks keep their values.
//!   The `#VC` handler `locore_vc_trap32` is its `!AMDSEV` half, the termination request,
//!   and there is no GHCB remap.
//! - No `pmap_direct_rand` (no `.openbsd.randomdata`): the direct map starts at
//!   `PDIR_SLOT_DIRECT` itself.
//! - The bootstrap tables hold no proc0 stack (`UPAGES` pages in C): proc0's u-area is
//!   `machdep.rs`'s static `PROC0_UAREA` on both entries, and the boot stack is
//!   `locore0_bootstk` in `.bss`, 64 KiB like Limine's, where `main` runs as under Limine.
//!   `proc0paddr` and its `pcb_cr3` are not set here (`x86_64_proc0_tss_ldt_init` sets the
//!   latter from `%cr3`).
//! - `atdevbase` is not relocated: it is the direct map's ISA hole (`bus_space.rs`).
//! - The C ends with `call init_x86_64; call main`; here `call bootarg_main`
//!   (`sys/stand/bootarg.rs`), which builds the [`BootInfo`] (`machdep.rs`'s `getbootinfo`)
//!   and runs the same tail as the Limine entry (`init_x86_64`, then `main`). `first_avail`
//!   is passed as in C: the bootstrap tables' end less their five spare pages.
//! - No `.codepatch` sections: there is no codepatch.
//!
//! [`BootInfo`]: crate::machine::BootInfo

use core::arch::global_asm;

use crate::arch::amd64::amd64::cpu::{
    CPU_EBXFEATURE, CPU_ECXFEATURE, CPU_FEATURE, CPU_ID, CPU_VENDOR, CPUID_LEVEL,
};
use crate::arch::amd64::amd64::machdep::{
    BIOSBASEMEM, BOOTAPIVER, BOOTDEV, BOOTINFO, BOOTINFO_SIZE, ESYM, SSYM,
};
use crate::arch::amd64::amd64::pmap::{PG_CRYPT, PG_G_KERN, PG_NX_BIT};
use crate::arch::amd64::include::param::{KERNBASE, KERNTEXTOFF, NBPG, PGOFSET, PGSHIFT};
use crate::arch::amd64::include::pmap::{
    L4_SLOT_KERNBASE, NDML2_ENTRIES, NDML3_ENTRIES, NKL2_KIMG_ENTRIES, NKL3_KIMG_ENTRIES,
    NKL4_KIMG_ENTRIES, NPDPG, PDIR_SLOT_DIRECT, PDIR_SLOT_PTE, pl2_pi, pl3_pi,
};
use crate::arch::amd64::include::psl::PSL_MBO;
use crate::arch::amd64::include::pte::{NBPD_L2, PG_KR, PG_KW, PG_PS, PG_V};
use crate::arch::amd64::include::segments::{GCODE_SEL, NIDT, SDT_SYS386IGT, SEL_KPL, gsel};
use crate::arch::amd64::include::specialreg::{
    CPUID_NXE, CR0_DEFAULT, CR4_DEFAULT, EFER_LMA, EFER_LME, EFER_NXE, EFER_SCE, MSR_EFER,
    MSR_SEV_GHCB,
};
use crate::arch::amd64::include::trap::T_VC;
use crate::dev::isa::isareg::{IOM_BEGIN, IOM_SIZE};
use crate::kern::init_main::BOOTHOWTO;

/// `L3_SLOT_KERNBASE` (`genassym.cf`): the kernel image's slot in its level-3 table.
const L3_SLOT_KERNBASE: usize = pl3_pi(KERNBASE);
/// `L2_SLOT_KERNBASE` (`genassym.cf`): the kernel image's slot in its level-2 table.
const L2_SLOT_KERNBASE: usize = pl2_pi(KERNBASE);

/// `TABLE_L2_ENTRIES`: the level-1 pages of the kernel image. The C doubles them when
/// `L2_SLOT_KERNBASE > 0`, which `KERNBASE` excludes (checked at the end of this file).
const TABLE_L2_ENTRIES: usize = NKL2_KIMG_ENTRIES + 1;
/// `TABLE_L3_ENTRIES`: the level-2 pages.
const TABLE_L3_ENTRIES: usize = if L3_SLOT_KERNBASE > 0 {
    2 * NKL3_KIMG_ENTRIES
} else {
    NKL3_KIMG_ENTRIES
};

/// `PROC0_PML4_OFF`: the PML4, first page of the bootstrap tables.
const PROC0_PML4_OFF: usize = 0;
/// `PROC0_PTP3_OFF`: the level-3 page (the C has proc0's `UPAGES` stack pages before it).
const PROC0_PTP3_OFF: usize = PROC0_PML4_OFF + NBPG;
/// `PROC0_PTP2_OFF`.
const PROC0_PTP2_OFF: usize = PROC0_PTP3_OFF + NKL4_KIMG_ENTRIES * NBPG;
/// `PROC0_PTP1_OFF`.
const PROC0_PTP1_OFF: usize = PROC0_PTP2_OFF + TABLE_L3_ENTRIES * NBPG;
/// `PROC0_DMP3_OFF`: the direct map's level-3 page.
const PROC0_DMP3_OFF: usize = PROC0_PTP1_OFF + TABLE_L2_ENTRIES * NBPG;
/// `PROC0_DMP2_OFF`: the direct map's level-2 pages (4 GB of 2 MB pages).
const PROC0_DMP2_OFF: usize = PROC0_DMP3_OFF + NDML3_ENTRIES * NBPG;
/// `TABLESIZE`: the bootstrap tables and the five pages after them (the C's early PTE pages
/// and SEV-ES GHCB; unused here).
pub const TABLESIZE: usize = (NKL4_KIMG_ENTRIES
    + TABLE_L3_ENTRIES
    + TABLE_L2_ENTRIES
    + 1
    + NDML3_ENTRIES
    + NDML2_ENTRIES
    + 5)
    * NBPG;

/// The boot stack `bootarg_main` runs on: 64 KiB, as Limine's.
const BOOTSTACK_SIZE: usize = 64 * 1024;

/// `MSR_PROTO_TERMINATION_REQ` (`<machine/ghcb.h>`): the GHCB MSR protocol's termination
/// request.
const MSR_PROTO_TERMINATION_REQ: u32 = 0x100;

global_asm!(
    include_str!("locore0.S"),
    KERNBASE = const KERNBASE,
    KERNBASE_LO = const KERNBASE & 0xffff_ffff,
    KERNBASE_HI = const KERNBASE >> 32,
    KERNTEXTOFF_OFF = const KERNTEXTOFF - KERNBASE,
    BOOTHOWTO = sym BOOTHOWTO,
    BOOTDEV = sym BOOTDEV,
    ESYM = sym ESYM,
    SSYM = sym SSYM,
    BIOSBASEMEM = sym BIOSBASEMEM,
    BOOTAPIVER = sym BOOTAPIVER,
    BOOTINFO_SIZE = sym BOOTINFO_SIZE,
    BOOTINFO = sym BOOTINFO,
    PSL_MBO = const PSL_MBO,
    T_VC = const T_VC,
    SDT_SYS386IGT = const SDT_SYS386IGT,
    NIDT = const NIDT,
    CPUID_LEVEL = sym CPUID_LEVEL,
    CPU_VENDOR = sym CPU_VENDOR,
    CPU_ID = sym CPU_ID,
    CPU_EBXFEATURE = sym CPU_EBXFEATURE,
    CPU_ECXFEATURE = sym CPU_ECXFEATURE,
    CPU_FEATURE = sym CPU_FEATURE,
    CPUID_NXE = const CPUID_NXE,
    PG_NX = sym PG_NX_BIT,
    PG_CRYPT = sym PG_CRYPT,
    PG_G_KERN = sym PG_G_KERN,
    NBPG = const NBPG,
    PGSHIFT = const PGSHIFT,
    PGOFSET = const PGOFSET,
    PG_V = const PG_V,
    PG_KR = const PG_KR,
    PG_KW = const PG_KW,
    PG_PS = const PG_PS,
    NBPD_L2 = const NBPD_L2,
    NPDPG = const NPDPG,
    TABLESIZE = const TABLESIZE,
    PROC0_PML4_OFF = const PROC0_PML4_OFF,
    PROC0_PTP3_OFF = const PROC0_PTP3_OFF,
    PROC0_PTP2_OFF = const PROC0_PTP2_OFF,
    PROC0_PTP1_OFF = const PROC0_PTP1_OFF,
    PROC0_DMP3_OFF = const PROC0_DMP3_OFF,
    PROC0_DMP2_OFF = const PROC0_DMP2_OFF,
    NKL2_KIMG_ENTRIES = const NKL2_KIMG_ENTRIES,
    NKL3_KIMG_ENTRIES = const NKL3_KIMG_ENTRIES,
    NKL4_KIMG_ENTRIES = const NKL4_KIMG_ENTRIES,
    NDML2_ENTRIES = const NDML2_ENTRIES,
    NDML3_ENTRIES = const NDML3_ENTRIES,
    L2_SLOT_KERNBASE = const L2_SLOT_KERNBASE,
    L3_SLOT_KERNBASE = const L3_SLOT_KERNBASE,
    L4_SLOT_KERNBASE = const L4_SLOT_KERNBASE,
    PDIR_SLOT_DIRECT = const PDIR_SLOT_DIRECT,
    PDIR_SLOT_PTE = const PDIR_SLOT_PTE,
    IOM_BEGIN = const IOM_BEGIN,
    IOM_SIZE = const IOM_SIZE,
    CR4_DEFAULT = const CR4_DEFAULT,
    MSR_EFER = const MSR_EFER,
    EFER_LME = const EFER_LME,
    EFER_SCE = const EFER_SCE,
    EFER_LMA = const EFER_LMA,
    EFER_NXE = const EFER_NXE,
    CR0_DEFAULT = const CR0_DEFAULT,
    GSEL_KCODE = const gsel(GCODE_SEL, SEL_KPL),
    MSR_PROTO_TERMINATION_REQ = const MSR_PROTO_TERMINATION_REQ,
    MSR_SEV_GHCB = const MSR_SEV_GHCB,
    BOOTSTACK_SIZE = const BOOTSTACK_SIZE,
    options(att_syntax)
);

const _: () = {
    // The kernel image's level-1 pages are filled from KERNTEXTOFF up, in one level-2 table.
    assert!(L2_SLOT_KERNBASE == 0 && L3_SLOT_KERNBASE == 510);
    // The bootstrap tables are mapped through the same level-1 pages as the image.
    assert!(TABLESIZE.is_multiple_of(NBPG));
};
/* </CODE> */
