/*	$OpenBSD: isa_machdep.c,v 1.31 2020/09/29 03:06:34 guenther Exp $	*/
/*	$NetBSD: isa_machdep.c,v 1.22 1997/06/12 23:57:32 thorpej Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (c) 1996, 1997 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Jason R. Thorpe of the Numerical Aerospace Simulation Facility,
 * NASA Ames Research Center.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 *
 * THIS SOFTWARE IS PROVIDED BY THE NETBSD FOUNDATION, INC. AND CONTRIBUTORS
 * ``AS IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
 * TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
 * PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL THE FOUNDATION OR CONTRIBUTORS
 * BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
 * CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
 * SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
 * INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
 * CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
 * ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
 * POSSIBILITY OF SUCH DAMAGE.
 */

/*-
 * Copyright (c) 1993, 1994, 1996, 1997
 *	Charles M. Hannum.  All rights reserved.
 * Copyright (c) 1991 The Regents of the University of California.
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
 *	@(#)isa.c	7.2 (Berkeley) 5/13/91
 */
/* </LICENSES> */

//! amd64 ISA machine-dependent code: `arch/amd64/isa/isa_machdep.c`.
//!
//! Upstream: sys/arch/amd64/isa/isa_machdep.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M4 ports the interrupt side: `isa_intr_alloc`, `isa_intr_check`,
//! `isa_intr_establish`, `isa_intr_disestablish` and `isa_attach_hook`. The ISA DMA bounce
//! buffers (`_isa_bus_dma*`, `isa_bus_dma_tag`) come with `bus_dma(9)` (M7); the IOAPIC
//! pin lookup (`mp_busses`) with the IOAPIC (M5).
//!
//! ## Deviations
//! - `isa_intr_alloc` returns the IRQ as `Option` where the C returns 0/1 with an out
//!   parameter.
//! - `isa_attach_hook` takes no devices (`struct device`, `isabus_attach_args`: M6) and keeps
//!   `isa_has_been_seen` here (`dev/isa/isa.c` defines it in C).

use core::ffi::c_void;
use core::ptr::NonNull;
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicPtr, Ordering};

use crate::arch::amd64::amd64::i8259::I8259_PIC;
use crate::arch::amd64::amd64::intr::{intr_disestablish, intr_establish};
use crate::arch::amd64::include::i8259::ICU_LEN;
use crate::arch::amd64::include::intr::{IntrFn, Intrhand};
use crate::arch::amd64::include::intrdefs::{IST_EDGE, IST_LEVEL, IST_NONE, IST_PULSE};
use crate::kern::subr_prf::panic;

/// `isa_chipset_tag_t`: unused on amd64.
pub type IsaChipsetTag = *const c_void;

/// `intrtype[ICU_LEN]`: the trigger type recorded per IRQ.
static INTRTYPE: [AtomicI32; ICU_LEN as usize] = [const { AtomicI32::new(0) }; ICU_LEN as usize];
/// `intrlevel[ICU_LEN]`.
static INTRLEVEL: [AtomicI32; ICU_LEN as usize] = [const { AtomicI32::new(0) }; ICU_LEN as usize];
/// `intrhand[ICU_LEN]`: the handler chains the allocator counts.
static INTRHAND: [AtomicPtr<Intrhand>; ICU_LEN as usize] =
    [const { AtomicPtr::new(core::ptr::null_mut()) }; ICU_LEN as usize];

/// `LEGAL_IRQ(x)`.
const fn legal_irq(x: i32) -> bool {
    x >= 0 && x < ICU_LEN && x != 2
}

/// `isa_intr_alloc`: picks an IRQ from `mask` for a `type_` interrupt, the least shared one.
pub fn isa_intr_alloc(_ic: IsaChipsetTag, mask: i32, type_: i32) -> Option<i32> {
    if type_ == IST_NONE {
        panic(format_args!("intr_alloc: bogus type"));
    }

    let mut bestirq = -1;
    let mut count = -1;

    // some interrupts should never be dynamically allocated
    let mut mask = mask & 0xdef8;

    // XXX some interrupts will be used later (6 for fdc, 12 for pms). the right answer is to
    // do "breadth-first" searching of devices.
    mask &= 0xefbf;

    for i in 0..ICU_LEN {
        if !legal_irq(i) || (mask & (1 << i)) == 0 {
            continue;
        }
        let t = INTRTYPE[i as usize].load(Ordering::Relaxed);
        if t == IST_NONE {
            // if nothing's using the irq, just return it
            return Some(i);
        } else if t == IST_EDGE || t == IST_LEVEL {
            if type_ != t {
                continue;
            }
            // if the irq is shareable, count the number of other handlers, and if it's smaller
            // than the last irq like this, remember it
            //
            // XXX We should probably also consider the interrupt level and stick IPL_TTY with
            // other IPL_TTY, etc.
            let mut tmp = 0;
            let mut q = INTRHAND[i as usize].load(Ordering::Relaxed).cast_const();
            // SAFETY: the chain's handlers are established ones, alive until disestablished.
            while let Some(ih) = unsafe { q.as_ref() } {
                q = ih.ih_next.get();
                tmp += 1;
            }
            if bestirq == -1 || count > tmp {
                bestirq = i;
                count = tmp;
            }
        } else if t == IST_PULSE {
            // this just isn't shareable
            continue;
        }
    }

    if bestirq == -1 { None } else { Some(bestirq) }
}

/// `isa_intr_check`: just check to see if an IRQ is available/can be shared. 0 = interrupt
/// not available, 1 = interrupt shareable, 2 = interrupt all to ourself.
pub fn isa_intr_check(_ic: IsaChipsetTag, irq: i32, type_: i32) -> i32 {
    if !legal_irq(irq) || type_ == IST_NONE {
        return 0;
    }

    let t = INTRTYPE[irq as usize].load(Ordering::Relaxed);
    if t == IST_NONE {
        return 2;
    }
    if t == IST_LEVEL {
        if type_ != t {
            return 0;
        }
        return 1;
    }
    if (t == IST_EDGE || t == IST_PULSE) && type_ != IST_NONE {
        return 0;
    }
    1
}

/// `isa_intr_establish`: set up an interrupt handler to start being called. XXX PRONE TO
/// RACE CONDITIONS, UGLY, 'INTERESTING' INSERTION ALGORITHM.
pub fn isa_intr_establish(
    _ic: IsaChipsetTag,
    irq: i32,
    type_: i32,
    level: i32,
    ih_fun: IntrFn,
    ih_arg: *mut c_void,
    ih_what: &'static str,
) -> Option<NonNull<Intrhand>> {
    let pic = &I8259_PIC;
    let pin = irq;
    // NIOAPIC > 0: the mp_busses pin lookup, M5.

    let _ = &INTRLEVEL;
    intr_establish(irq, pic, pin, type_, level, None, ih_fun, ih_arg, ih_what)
}

/// `isa_intr_disestablish`: deregister an interrupt handler.
///
/// # Safety
///
/// `arg` must come from `isa_intr_establish` and not be used afterwards.
pub unsafe fn isa_intr_disestablish(_ic: IsaChipsetTag, arg: NonNull<Intrhand>) {
    // SAFETY: the caller's guarantee.
    unsafe { intr_disestablish(arg) };
}

/// `isa_has_been_seen` (`dev/isa/isa.c`).
static ISA_HAS_BEEN_SEEN: AtomicBool = AtomicBool::new(false);

/// `isa_attach_hook`: notify others that might need to know that the ISA bus has now been
/// attached.
pub fn isa_attach_hook() {
    if ISA_HAS_BEEN_SEEN.swap(true, Ordering::Relaxed) {
        panic(format_args!("isaattach: ISA bus already seen!"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legal_irqs() {
        assert!(legal_irq(4));
        assert!(!legal_irq(2));
        assert!(!legal_irq(16));
        assert!(!legal_irq(-1));
        assert_eq!(isa_intr_check(core::ptr::null(), 4, IST_EDGE), 2);
        assert_eq!(isa_intr_check(core::ptr::null(), 2, IST_EDGE), 0);
        assert_eq!(isa_intr_alloc(core::ptr::null(), 0x18, IST_EDGE), Some(3));
    }
}
