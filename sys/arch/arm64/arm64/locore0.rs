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
//! The kernel's entry from boot(8) (efiboot): `arch/arm64/arm64/locore0.S`, pulled in from
//! the `.S` file next to this module (the file keeps OpenBSD's licence block and layout;
//! `{NAME}` placeholders are what `assym.h` and the headers provide in C).
//!
//! Upstream: sys/arch/arm64/arm64/locore0.S @ 3ce1f3f79392
//!
//! efiboot loads the kernel into a 64 MB block at a 2 MB boundary (its `LOADADDR` puts
//! physical address `p_paddr` at `efi_loadaddr + p_paddr`; `conf/kernel.ld` links the image
//! with physical addresses from 0) and jumps to `_start` with the MMU on and an identity
//! map, `x0` the end of the loaded symbols, `x1` 0 and `x2` the flattened device tree (its
//! `exec.c`). `_start` drops to EL1 (`locore.S`'s `drop_to_el1`), turns the MMU off, finds
//! where it runs (`get_virt_delta`), stores `esym`, builds the bootstrap page tables, turns
//! the MMU on with them (`start_mmu`), jumps to the kernel's virtual addresses, sets up the
//! boot stack (`initstack`), zeroes the BSS, builds the `arm64_bootparams` on the stack and
//! calls the boot glue. Limine does not come here: its entry is `_start` in
//! `sys/stand/mod.rs` (the entry point request; the ELF entry is this file's `_start`).
//!
//! ## Deviations
//! - The bootstrap tables give the kernel what it expects from Limine (docs/ARCHITECTURE.md,
//!   "The kernel's boot(8) entry (arm64)"): `TTBR1_EL1` is walked in four levels
//!   (`TCR_EL1.T1SZ` 16, where the C's 39-bit kernel has three), so `create_pagetables`
//!   adds a level-0 table for it, and maps beside the kernel's 64 MB block a direct map at
//!   [`DIRECT_BASE`] by 1 GB blocks (`build_l1_block_pagetable`, not in the C): here the
//!   gigabytes of the kernel and of the FDT; `machdep.rs`'s `getbootinfo` adds those of
//!   the rest of RAM. `arm64/pmap.rs` adopts these tables as it adopts Limine's. The kernel
//!   is linked at `0xffffffff80000000` (not `KERNBASE`), so its block is found from that
//!   address; all 32 2 MB entries of the block are mapped (the C maps 31).
//! - The kernel's 2 MB blocks are global (`x14`, a new argument of
//!   `build_l2_block_pagetable`): the C's are `ATTR_nG` because `pmap_bootstrap` replaces
//!   them, while here they stay. The identity map in `TTBR0_EL1` keeps `ATTR_nG`.
//! - `_start` lives in `.text.locore0`, which `conf/kernel.ld` puts first in the image: the
//!   C's `_start` is first because `locore0.o` is linked first. efiboot cleans the data cache
//!   from the entry point to the end of the symbols, so the whole image must follow it.
//! - No proc0 trapframe is carved off the boot stack: proc0's u-area is `machdep.rs`'s
//!   static `PROC0_UAREA` on both entries. `initstack` is 64 KiB (`USPACE` in C), as
//!   Limine's stack.
//! - `kern_l1pt` is passed as the level-0 table's physical address (the C converts it with
//!   the delta of the wrong sign; nothing reads it here).
//! - The C ends with `bl initarm; bl main`; here `bl bootarg_main` (`sys/stand/bootarg.rs`),
//!   which builds the `BootInfo` from the parameters (`machdep.rs`'s `getbootinfo`, the
//!   `/chosen` half of `initarm`) and runs the Limine entry's tail (`initarm`, then `main`).
//! - No `.codepatch`/`.codepatchend` sections: there is no codepatch.
//! - `_start` masks the interrupts first (`msr daifset`, not in the C): the firmware may
//!   leave them unmasked after `ExitBootServices`, and the kernel expects them masked until
//!   it unmasks them itself, as Limine hands it over.

use core::arch::global_asm;

use crate::arch::arm64::include::armreg::SCTLR_M;
use crate::arch::arm64::include::param::{PAGE_SHIFT, PAGE_SIZE};
use crate::arch::arm64::include::pte::{
    ATTR_AF, ATTR_PXN, ATTR_UXN, ATTR_nG, L0_SHIFT, L0_TABLE, L1_BLOCK, L1_SHIFT, L1_TABLE,
    L2_BLOCK, L2_SHIFT, Ln_ADDR_MASK, PTE_ATTR_WB, SH_INNER, attr_sh,
};

/// Where `locore0.S` puts the direct map of physical memory (Limine's higher-half direct map
/// offset for four-level paging, so both entries hand the kernel the same layout): level-0
/// slot 0 of `TTBR1_EL1`, below `VM_MIN_KERNEL_ADDRESS` and the kernel image.
pub const DIRECT_BASE: usize = 0xffff_0000_0000_0000;

global_asm!(
    include_str!("locore0.S"),
    SCTLR_M = const SCTLR_M,
    DIRECT_BASE = const DIRECT_BASE,
    NORMAL_MEM = const PTE_ATTR_WB,
    PAGE_SIZE = const PAGE_SIZE,
    PAGE_SHIFT = const PAGE_SHIFT,
    ATTR_NG = const ATTR_nG,
    ATTR_AF_SH_INNER = const ATTR_AF | attr_sh(SH_INNER),
    ATTR_UXN = const ATTR_UXN,
    ATTR_XN = const ATTR_UXN | ATTR_PXN,
    L0_SHIFT = const L0_SHIFT,
    L1_SHIFT = const L1_SHIFT,
    L2_SHIFT = const L2_SHIFT,
    LN_ADDR_MASK = const Ln_ADDR_MASK,
    L0_TABLE = const L0_TABLE,
    L1_TABLE = const L1_TABLE,
    L1_BLOCK = const L1_BLOCK,
    L2_BLOCK = const L2_BLOCK,
);
/* </CODE> */
