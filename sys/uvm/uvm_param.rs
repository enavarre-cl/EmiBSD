/*	$OpenBSD: uvm_param.h,v 1.18 2015/02/07 08:21:24 miod Exp $	*/
/*	$NetBSD: uvm_param.h,v 1.5 2001/03/09 01:02:12 chs Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1991, 1993
 *	The Regents of the University of California.  All rights reserved.
 *
 * This code is derived from software contributed to Berkeley by
 * The Mach Operating System project at Carnegie-Mellon University.
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
 *	@(#)vm_param.h	8.2 (Berkeley) 1/9/95
 *
 *
 * Copyright (c) 1987, 1990 Carnegie-Mellon University.
 * All rights reserved.
 *
 * Authors: Avadis Tevanian, Jr., Michael Wayne Young
 *
 * Permission to use, copy, modify and distribute this software and
 * its documentation is hereby granted, provided that both the copyright
 * notice and this permission notice appear in all copies of the
 * software, derivative works or modified versions, and any portions
 * thereof, and that both notices appear in supporting documentation.
 *
 * CARNEGIE MELLON ALLOWS FREE USE OF THIS SOFTWARE IN ITS "AS IS"
 * CONDITION.  CARNEGIE MELLON DISCLAIMS ANY LIABILITY OF ANY KIND
 * FOR ANY DAMAGES WHATSOEVER RESULTING FROM THE USE OF THIS SOFTWARE.
 *
 * Carnegie Mellon requests users of this software to return to
 *
 *  Software Distribution Coordinator  or  Software.Distribution@CS.CMU.EDU
 *  School of Computer Science
 *  Carnegie Mellon University
 *  Pittsburgh PA 15213-3890
 *
 * any improvements or extensions that they make and grant Carnegie the
 * rights to redistribute these changes.
 */
/* </LICENSES> */

//! Machine independent virtual memory parameters: `<uvm/uvm_param.h>`.
//!
//! Upstream: sys/uvm/uvm_param.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - `boolean_t`, `TRUE` and `FALSE` are `bool`.
//! - `atop`, `ptoa`, `round_page` and `trunc_page` are `const fn`s on `usize` (page numbers and
//!   byte counts), with typed conveniences on the address newtypes: [`Paddr::atop`],
//!   [`Paddr::ptoa`], [`Vaddr::round_page`] and friends.

use crate::machine::{Machine, VmParam};
use crate::sys::param::{PAGE_MASK, PAGE_SHIFT};
use crate::sys::types::{Paddr, Vaddr, Vsize};

/// Default page size.
pub const DEFAULT_PAGE_SIZE: usize = 4096;

/// `VM_KERNEL_SPACE_SIZE`: the size of the kernel's virtual address space.
pub const VM_KERNEL_SPACE_SIZE: usize =
    <Machine as VmParam>::VM_MAX_KERNEL_ADDRESS - <Machine as VmParam>::VM_MIN_KERNEL_ADDRESS;

/// `atop(x)`: bytes to pages.
pub const fn atop(x: usize) -> usize {
    x >> PAGE_SHIFT
}

/// `ptoa(x)`: pages to bytes.
pub const fn ptoa(x: usize) -> usize {
    x << PAGE_SHIFT
}

/// `round_page(x)`: rounds up to a page boundary.
pub const fn round_page(x: usize) -> usize {
    (x + PAGE_MASK) & !PAGE_MASK
}

/// `trunc_page(x)`: rounds down to a page boundary.
pub const fn trunc_page(x: usize) -> usize {
    x & !PAGE_MASK
}

impl Paddr {
    /// `atop(pa)`: the page frame number of this address.
    pub const fn atop(self) -> usize {
        atop(self.0)
    }

    /// `ptoa(pgno)`: the address of page frame `pgno`.
    pub const fn ptoa(pgno: usize) -> Paddr {
        Paddr(ptoa(pgno))
    }

    /// `round_page`.
    pub const fn round_page(self) -> Paddr {
        Paddr(round_page(self.0))
    }

    /// `trunc_page`.
    pub const fn trunc_page(self) -> Paddr {
        Paddr(trunc_page(self.0))
    }
}

impl Vaddr {
    /// `round_page`.
    pub const fn round_page(self) -> Vaddr {
        Vaddr(round_page(self.0))
    }

    /// `trunc_page`.
    pub const fn trunc_page(self) -> Vaddr {
        Vaddr(trunc_page(self.0))
    }
}

impl Vsize {
    /// `round_page`.
    pub const fn round_page(self) -> Vsize {
        Vsize(round_page(self.0))
    }

    /// `atop`.
    pub const fn atop(self) -> usize {
        atop(self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_arithmetic() {
        assert_eq!(atop(0x3000), 3);
        assert_eq!(ptoa(3), 0x3000);
        assert_eq!(round_page(0x1001), 0x2000);
        assert_eq!(round_page(0x1000), 0x1000);
        assert_eq!(trunc_page(0x1fff), 0x1000);
        assert_eq!(Paddr::new(0x5123).atop(), 5);
        assert_eq!(Paddr::ptoa(5), Paddr::new(0x5000));
        assert_eq!(Vaddr::new(0x1001).round_page(), Vaddr::new(0x2000));
        assert_eq!(Vsize::new(0x1001).round_page().atop(), 2);
        assert!(VM_KERNEL_SPACE_SIZE > 0);
    }
}
