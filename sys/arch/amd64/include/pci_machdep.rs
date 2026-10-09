/* <LICENSES> */
/*
 * Copyright (c) 1996 Christopher G. Demetriou.  All rights reserved.
 * Copyright (c) 1994 Charles M. Hannum.  All rights reserved.
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
 *	This product includes software developed by Charles M. Hannum.
 * 4. The name of the author may not be used to endorse or promote products
 *    derived from this software without specific prior written permission.
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
//! amd64 `<machine/pci_machdep.h>`: machine-specific definitions for PCI autoconfiguration.
//!
//! Upstream: sys/arch/amd64/include/pci_machdep.h @ 3ce1f3f79392
//!
//! The types amd64 provides to machine-independent PCI code (`pci_chipset_tag_t`,
//! `pcitag_t`, `pci_intr_handle_t`) and the amd64-only constants. The functions it declares
//! live in `arch/amd64/pci/pci_machdep.rs` (and `pci_bus_dma_tag` there, as in C); MI code
//! reaches them through `machine::pci_machdep`.
//!
//! ## Deviations
//! - `pci_chipset_tag_t` keeps the C's `void *` as `Option<NonNull<c_void>>`; it is always NULL
//!   on amd64 (`pci_lookup_segment` returns NULL for the only segment it accepts).
//! - The `pci_intr_line(pc, ih)` macro is a function of the handle.
//! - The extent pointers (`pciio_ex`, `pcimem_ex`, `pcibus_ex`) and `pci_init_extents` wait
//!   for `sys/extent.h` (`pci_machdep.rs` reports the call).

use core::ffi::c_void;
use core::ptr::NonNull;

/// `pci_chipset_tag_t`: always NULL (`None`) on amd64.
pub type PciChipsetTag = Option<NonNull<c_void>>;

/// `pcitag_t`: configuration mechanism #1's address: enable bit, bus, device, function.
pub type Pcitag = u32;

/// `pci_intr_handle_t`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PciIntrHandle {
    /// `tag`: the function (MSI and MSI-X carry the vector in its low byte).
    pub tag: Pcitag,
    /// `line`: the interrupt line, or `APIC_INT_VIA_MSG`/`APIC_INT_VIA_MSGX`, or an I/O
    /// APIC encoding (`APIC_INT_VIA_APIC`); -1 when unmapped.
    pub line: i32,
    /// `pin`: the interrupt pin (`PCI_INTERRUPT_PIN_A`..D).
    pub pin: i32,
}

/// `pci_intr_line(pc, ih)`: the legacy line of a handle.
pub const fn pci_intr_line(ih: PciIntrHandle) -> i32 {
    ih.line & 0xff
}

/// `X86_PCI_INTERRUPT_LINE_NO_CONNECTION`: section 6.2.4, "Miscellaneous Functions", of
/// the PCI Specification says that 255 means "unknown" or "no connection" to the interrupt
/// controller on a PC.
pub const X86_PCI_INTERRUPT_LINE_NO_CONNECTION: i32 = 0xff;

/// `PCI_IO_START`: PCI address space is shared with ISA, so avoid legacy ISA I/O registers.
pub const PCI_IO_START: u64 = 0x400;
/// `PCI_IO_END`.
pub const PCI_IO_END: u64 = 0xffff;

/// `PCI_MEM_START`: avoid the DOS Compatibility Memory area.
pub const PCI_MEM_START: u64 = 0x10_0000;
/* </CODE> */
