/*	$OpenBSD: vmparam.h,v 1.9 2023/04/28 18:33:22 robert Exp $	*/
/*	$NetBSD: vmparam.h,v 1.1 2003/04/26 18:39:49 fvdl Exp $	*/
/* <LICENSES> */
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
 *	@(#)vmparam.h	5.9 (Berkeley) 5/12/91
 */
/* </LICENSES> */

/* <CODE> */
//! arm64 `<machine/vmparam.h>`: the virtual address space layout.
//!
//! Upstream: sys/arch/arm64/include/vmparam.h @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M3 needs the address-space bounds and the physical segment
//! policy; the user limits (`MAXTSIZ`, `DFLDSIZ`, `MAXDSIZ`, `BRKSIZ`, `DFLSSIZ`, `MAXSSIZ`,
//! `STACKGAP_RANDOM`, `USRSTACK`, `VM_MIN_STACK_ADDRESS`); M10a (physio) `USRIOSIZE` and
//! `VM_PHYS_SIZE`; `SHMMAXPGS` is not there yet.

use crate::arch::arm64::include::param::PAGE_SIZE;
use crate::uvm::uvm_page::VM_PSTRAT_BSEARCH;

/// `USRIOSIZE`: size of User Raw I/O map, in pages.
pub const USRIOSIZE: usize = 300;
/// `KERNEL_BASE`: where the kernel's virtual space starts.
pub const KERNEL_BASE: usize = 0xffff_ff80_0000_0000;
/// `VM_MIN_ADDRESS`: the lowest user address.
pub const VM_MIN_ADDRESS: usize = PAGE_SIZE;
/// `USER_SPACE_BITS`: user virtual addresses are this wide.
pub const USER_SPACE_BITS: u32 = 39;
/// `VM_MAXUSER_ADDRESS`: the highest address user mappings may reach.
pub const VM_MAXUSER_ADDRESS: usize = (1 << USER_SPACE_BITS) - 0x8000;
/// `VM_MAX_ADDRESS`: the end of the user address space.
pub const VM_MAX_ADDRESS: usize = VM_MAXUSER_ADDRESS;
/// `USRSTACK`: the top (end) of the user stack.
pub const USRSTACK: usize = VM_MAXUSER_ADDRESS;
/// `MAXTSIZ`: max text size.
pub const MAXTSIZ: usize = 256 * 1024 * 1024;
/// `DFLDSIZ`: initial data size limit.
pub const DFLDSIZ: usize = 512 * 1024 * 1024;
/// `MAXDSIZ`: max data size.
pub const MAXDSIZ: usize = 64 * 1024 * 1024 * 1024;
/// `BRKSIZ`: heap gap size.
pub const BRKSIZ: usize = 16 * 1024 * 1024 * 1024;
/// `DFLSSIZ`: initial stack size limit.
pub const DFLSSIZ: usize = 2 * 1024 * 1024;
/// `MAXSSIZ`: max stack size.
pub const MAXSSIZ: usize = 32 * 1024 * 1024;
/// `STACKGAP_RANDOM`.
pub const STACKGAP_RANDOM: usize = 256 * 1024;
/// `VM_MIN_STACK_ADDRESS`.
pub const VM_MIN_STACK_ADDRESS: usize = 3 << (USER_SPACE_BITS - 2);
/// `VM_MIN_KERNEL_ADDRESS`: the start of the kernel address space.
pub const VM_MIN_KERNEL_ADDRESS: usize = 0xffff_ff80_0000_0000;
/// `VM_MAX_KERNEL_ADDRESS`: the end of the kernel's own virtual space.
pub const VM_MAX_KERNEL_ADDRESS: usize = 0xffff_ff83_ffff_ffff;
/// `VM_PHYS_SIZE`: virtual size (bytes) of the physio submap (`phys_map`).
pub const VM_PHYS_SIZE: usize = USRIOSIZE * PAGE_SIZE;
/// `VM_PHYSSEG_MAX`: how many physical memory segments `uvm_page_physload` accepts.
pub const VM_PHYSSEG_MAX: usize = 32;
/// `VM_PHYSSEG_STRAT`: `vm_physmem[]` is kept sorted for a binary search.
pub const VM_PHYSSEG_STRAT: i32 = VM_PSTRAT_BSEARCH;
/// `VM_PHYSSEG_NOADD`: can't add RAM after `vm_mem_init`.
pub const VM_PHYSSEG_NOADD: bool = true;
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/arch/arm64/include/vmparam.h");
        let ours: &[(&str, i64)] = &[
            ("USER_SPACE_BITS", i64::from(USER_SPACE_BITS)),
            ("VM_PHYSSEG_MAX", VM_PHYSSEG_MAX as i64),
            ("USRIOSIZE", USRIOSIZE as i64),
        ];
        for (name, value) in ours {
            assert_eq!(crate::reftest::int(&defs, name), Some(*value), "{name}");
        }
    }
}
/* </TESTS> */
