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
//! `<machine/param.h>` and the alignment rules of `<machine/_types.h>` as traits.
//!
//! Each architecture implements them in `arch/<arch>/include/{param,_types}.rs`; `sys::param`
//! re-exports the values so generic code never names an architecture.

/// Identity of the running architecture: `MACHINE` and `MACHINE_ARCH` of `<machine/param.h>`.
pub trait MachineInfo {
    /// The architecture name as OpenBSD spells it: `"amd64"` or `"arm64"`; `"host"` for the
    /// test double.
    const MACHINE: &'static str;
    /// The CPU architecture name: `"amd64"` or `"aarch64"`; `"host"` for the test double.
    const MACHINE_ARCH: &'static str;
}

/// Machine-dependent parameters: `<machine/param.h>` plus the alignment rules of
/// `<machine/_types.h>`.
///
/// Generic code reads these through `sys::param` (`PAGE_SIZE`, `ALIGNBYTES`, ...), exactly as C
/// reaches them through `<sys/param.h>`. Every value is a plain constant in the architecture's
/// `include/param.rs` or `include/_types.rs`; the trait only proves that each architecture
/// defines the whole set.
pub trait MachineParam {
    /// `PAGE_SHIFT`: log2 of the page size.
    const PAGE_SHIFT: usize;
    /// `PAGE_SIZE`: bytes per page.
    const PAGE_SIZE: usize;
    /// `PAGE_MASK`: byte offset mask within a page.
    const PAGE_MASK: usize;
    /// `KERNBASE`: start of kernel virtual address space.
    const KERNBASE: usize;
    /// `UPAGES`: pages of u-area (per-thread kernel stack and PCB).
    const UPAGES: usize;
    /// `USPACE`: total size of the u-area.
    const USPACE: usize;
    /// `USPACE_ALIGN`: u-area alignment, 0 for none.
    const USPACE_ALIGN: usize;
    /// `__HAVE_USPACE_GUARD`: the u-area carries a guard page.
    const HAVE_USPACE_GUARD: bool;
    /// `NMBCLUSTERS`: maximum number of mbuf clusters.
    const NMBCLUSTERS: usize;
    /// `MSGBUFSIZE`: default kernel message buffer size.
    const MSGBUFSIZE: usize;
    /// `__HAVE_ACPI`: the architecture boots with ACPI tables.
    const HAVE_ACPI: bool;
    /// `__HAVE_FDT`: the architecture boots with a flattened device tree.
    const HAVE_FDT: bool;
    /// `_ALIGNBYTES`: rounding mask that aligns an address for every data type.
    const ALIGNBYTES: usize;
    /// `_STACKALIGNBYTES`: rounding mask for the stack pointer.
    const STACKALIGNBYTES: usize;
    /// `_MAX_PAGE_SHIFT`: the largest page shift the architecture can use.
    const MAX_PAGE_SHIFT: usize;
    /// `__STRICT_ALIGNMENT` (`<machine/endian.h>`): the CPU does not fetch misaligned data
    /// (drivers then shift received frames so the IP header is aligned).
    const STRICT_ALIGNMENT: bool;
    /// `_ALIGNED_POINTER(p, t)`: whether a value of type `T` may be fetched from address `p`.
    /// This reflects possibility, not optimal alignment.
    fn aligned_pointer<T>(p: usize) -> bool;
}
/* </CODE> */
