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
 * Copyright (c) 1998 Christopher G. Demetriou.  All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. All advertising materials mentioning features or use of this software
 *    must display the following acknowledgement:
 *      This product includes software developed by Christopher G. Demetriou
 *	for the NetBSD Project.
 * 4. The name of the author may not be used to endorse or promote products
 *    derived from this software without specific prior written permission
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR ``AS IS'' AND ANY EXPRESS OR
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES
 * OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED.
 * IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY DIRECT, INDIRECT,
 * INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT
 * NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
 * DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
 * THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
 * (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF
 * THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! PCI IDE controller driver (amd64 machine-dependent portion): a compatibility-mode
//! channel's interrupt is ISA IRQ 14 (primary) or 15 (secondary), edge-triggered.
//!
//! Upstream: sys/arch/amd64/pci/pciide_machdep.c @ 3ce1f3f79392
//!
//! Author: Christopher G. Demetriou, March 2, 1998 (derived from NetBSD
//! sys/dev/pci/ppb.c, revision 1.16).
//!
//! See "PCI IDE Controller Specification, Revision 1.0 3/4/94" from the PCI SIG.
//!
//! ## Deviations
//! - The two functions are reached through `machine::pciide_machdep`
//!   (`PciideMachdep`), which `arch/amd64/mod.rs` implements with them.

use core::ffi::c_void;
use core::ptr::{self, NonNull};

use crate::arch::amd64::include::intrdefs::IST_EDGE;
use crate::arch::amd64::isa::isa_machdep::{isa_intr_disestablish, isa_intr_establish};
use crate::dev::pci::pciidereg::pciide_compat_irq;
use crate::dev::pci::pcivar::PciAttachArgs;
use crate::machine::intr::IPL_BIO;
use crate::machine::pci_machdep::PciChipsetTag;
use crate::sys::device::Device;

/// `pciide_machdep_compat_intr_establish`.
pub fn pciide_machdep_compat_intr_establish(
    dev: &'static Device,
    _pa: &PciAttachArgs,
    chan: i32,
    func: fn(*mut c_void) -> i32,
    arg: *mut c_void,
) -> Option<NonNull<c_void>> {
    let irq = pciide_compat_irq(chan);
    let cookie = isa_intr_establish(ptr::null(), irq, IST_EDGE, IPL_BIO, func, arg, dev.xname());

    cookie.map(|ih| ih.cast())
}

/// `pciide_machdep_compat_intr_disestablish`.
///
/// # Safety
///
/// `cookie` came from `pciide_machdep_compat_intr_establish` and is not used afterwards.
pub unsafe fn pciide_machdep_compat_intr_disestablish(_pc: PciChipsetTag, cookie: NonNull<c_void>) {
    // SAFETY: the caller's guarantee: the handle `isa_intr_establish` returned.
    unsafe { isa_intr_disestablish(ptr::null(), cookie.cast()) };
}
/* </CODE> */
