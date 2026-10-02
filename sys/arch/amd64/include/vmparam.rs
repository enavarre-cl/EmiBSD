/*	$OpenBSD: vmparam.h,v 1.26 2026/06/22 00:27:33 jsg Exp $	*/
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

//! amd64 `<machine/vmparam.h>`: the virtual address space layout.
//!
//! Upstream: sys/arch/amd64/include/vmparam.h @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M2 needs the kernel/user boundary for `ddb`'s `INKERNEL`, M3 the
//! physical segment policy; the user limits (`MAXTSIZ`, `DFLDSIZ`, `MAXDSIZ`, `BRKSIZ`,
//! `DFLSSIZ`, `MAXSSIZ`, `STACKGAP_RANDOM`, `USRSTACK`, `VM_MIN_STACK_ADDRESS`), `SHMMAXPGS`,
//! `USRIOSIZE`, `VM_PHYS_SIZE` and the `VM_FREELIST_*` arrive with M6.

use crate::arch::amd64::include::param::PAGE_SIZE;
use crate::uvm::uvm_page::VM_PSTRAT_BIGFIRST;

/// `VM_MIN_ADDRESS`: the lowest user address.
pub const VM_MIN_ADDRESS: usize = PAGE_SIZE;
/// `VM_MAXUSER_ADDRESS`: the highest address user mappings may reach.
pub const VM_MAXUSER_ADDRESS: usize = 0x0000_7f7f_ffff_c000;
/// `VM_MAX_ADDRESS`: the end of the user address space.
pub const VM_MAX_ADDRESS: usize = 0x0000_7fbf_dfef_f000;
/// `VM_MIN_KERNEL_ADDRESS`: the start of the kernel address space (the direct map).
pub const VM_MIN_KERNEL_ADDRESS: usize = 0xffff_8000_0000_0000;
/// `VM_MAX_KERNEL_ADDRESS`: the end of the kernel's own virtual space.
pub const VM_MAX_KERNEL_ADDRESS: usize = 0xffff_8080_0000_0000;
/// `VM_PHYSSEG_MAX`: how many physical memory segments `uvm_page_physload` accepts (actually
/// we could have this many segments).
pub const VM_PHYSSEG_MAX: usize = 16;
/// `VM_PHYSSEG_STRAT`: `vm_physmem[]` keeps the biggest segment first.
pub const VM_PHYSSEG_STRAT: i32 = VM_PSTRAT_BIGFIRST;
/// `VM_PHYSSEG_NOADD`: can't add RAM after `vm_mem_init`.
pub const VM_PHYSSEG_NOADD: bool = true;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/arch/amd64/include/vmparam.h");
        let ours: &[(&str, i64)] = &[
            ("VM_MAXUSER_ADDRESS", VM_MAXUSER_ADDRESS as i64),
            ("VM_MAX_ADDRESS", VM_MAX_ADDRESS as i64),
            ("VM_MIN_KERNEL_ADDRESS", VM_MIN_KERNEL_ADDRESS as i64),
            ("VM_MAX_KERNEL_ADDRESS", VM_MAX_KERNEL_ADDRESS as i64),
            ("VM_PHYSSEG_MAX", VM_PHYSSEG_MAX as i64),
        ];
        for (name, value) in ours {
            assert_eq!(crate::reftest::int(&defs, name), Some(*value), "{name}");
        }
    }
}
