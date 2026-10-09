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
//! The machine half of `<dev/pci/pciidevar.h>`: how a PCI IDE channel wired to
//! compatibility mode gets its legacy interrupt (each arch's `pciide_machdep.c`).
//!
//! amd64 establishes ISA IRQ 14 or 15 through its ISA interrupt glue
//! (`arch/amd64/pci/pciide_machdep.c`). arm64 has no `pciide_machdep.c` (its GENERIC has no
//! pciide) and no ISA bus: no compatibility interrupt can be established, as on a machine
//! without one.

use core::ffi::c_void;
use core::ptr::NonNull;

use crate::dev::pci::pcivar::PciAttachArgs;
use crate::machine::Machine;
use crate::machine::pci_machdep::PciChipsetTag;
use crate::sys::device::Device;

/// The PCI IDE compatibility interrupt of the machine.
pub trait PciideMachdep {
    /// `pciide_machdep_compat_intr_establish(dev, pa, chan, func, arg)`: attach the compat
    /// interrupt handler of channel `chan`, returning its handle or `None` if failed.
    fn pciide_machdep_compat_intr_establish(
        dev: &'static Device,
        pa: &PciAttachArgs,
        chan: i32,
        func: fn(*mut c_void) -> i32,
        arg: *mut c_void,
    ) -> Option<NonNull<c_void>>;

    /// `pciide_machdep_compat_intr_disestablish(pc, cookie)`.
    ///
    /// # Safety
    ///
    /// `cookie` came from `pciide_machdep_compat_intr_establish` and is not used afterwards.
    unsafe fn pciide_machdep_compat_intr_disestablish(pc: PciChipsetTag, cookie: NonNull<c_void>);
}

/// `pciide_machdep_compat_intr_establish` on the selected machine.
pub fn pciide_machdep_compat_intr_establish(
    dev: &'static Device,
    pa: &PciAttachArgs,
    chan: i32,
    func: fn(*mut c_void) -> i32,
    arg: *mut c_void,
) -> Option<NonNull<c_void>> {
    Machine::pciide_machdep_compat_intr_establish(dev, pa, chan, func, arg)
}

/// `pciide_machdep_compat_intr_disestablish` on the selected machine.
///
/// # Safety
///
/// As for [`PciideMachdep::pciide_machdep_compat_intr_disestablish`].
pub unsafe fn pciide_machdep_compat_intr_disestablish(pc: PciChipsetTag, cookie: NonNull<c_void>) {
    // SAFETY: forwarded.
    unsafe { Machine::pciide_machdep_compat_intr_disestablish(pc, cookie) }
}
/* </CODE> */
