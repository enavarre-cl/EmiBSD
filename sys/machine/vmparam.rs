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
//! `<machine/vmparam.h>` as a trait: the virtual address space layout and the physical segment
//! policy of each architecture.
//!
//! `uvm/uvm_param.rs` re-exports the values generic code wants, as `<uvm/uvm_param.h>` includes
//! `<machine/vmparam.h>` in C. The `VM_PSTRAT_*` constants live in `uvm/uvm_page.rs`.

/// The virtual memory layout of the selected architecture.
pub trait VmParam {
    /// `VM_MIN_ADDRESS`: the lowest user address.
    const VM_MIN_ADDRESS: usize;
    /// `VM_MAXUSER_ADDRESS`: the highest address user mappings may reach.
    const VM_MAXUSER_ADDRESS: usize;
    /// `VM_MAX_ADDRESS`: the end of the user address space.
    const VM_MAX_ADDRESS: usize;
    /// `VM_MIN_KERNEL_ADDRESS`: the start of the kernel address space.
    const VM_MIN_KERNEL_ADDRESS: usize;
    /// `VM_MAX_KERNEL_ADDRESS`: the end of the kernel's own virtual space.
    const VM_MAX_KERNEL_ADDRESS: usize;
    /// `VM_PHYSSEG_MAX`: how many physical memory segments `uvm_page_physload` accepts.
    const VM_PHYSSEG_MAX: usize;
    /// `VM_PHYSSEG_STRAT`: how `vm_physmem[]` is ordered (one of `VM_PSTRAT_*`).
    const VM_PHYSSEG_STRAT: i32;
    /// `VM_PHYSSEG_NOADD`: whether RAM can be added after `uvm_init`.
    const VM_PHYSSEG_NOADD: bool;
    /// `USRSTACK`: the top (end) of the user stack.
    const USRSTACK: usize;
    /// `MAXTSIZ`: max text size.
    const MAXTSIZ: usize;
    /// `DFLDSIZ`: initial data size limit.
    const DFLDSIZ: usize;
    /// `MAXDSIZ`: max data size.
    const MAXDSIZ: usize;
    /// `BRKSIZ`: heap gap size.
    const BRKSIZ: usize;
    /// `DFLSSIZ`: initial stack size limit.
    const DFLSSIZ: usize;
    /// `MAXSSIZ`: max stack size.
    const MAXSSIZ: usize;
    /// `STACKGAP_RANDOM`: the range of the random gap below the stack.
    const STACKGAP_RANDOM: usize;
    /// `VM_MIN_STACK_ADDRESS`: the lowest address the stack may be placed at.
    const VM_MIN_STACK_ADDRESS: usize;
}
/* </CODE> */
