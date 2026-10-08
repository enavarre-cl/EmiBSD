/* $OpenBSD: vga_pcivar.h,v 1.20 2015/10/29 07:47:03 kettenis Exp $ */
/* $NetBSD: vga_pcivar.h,v 1.1 1998/03/22 15:16:19 drochner Exp $ */
/* <LICENSES> */
/*
 * Copyright (c) 1995, 1996 Carnegie-Mellon University.
 * All rights reserved.
 *
 * Author: Chris G. Demetriou
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

//! `<dev/pci/vga_pcivar.h>`: the softc of `vga(4)` at `pci`, the test for a VGA-class
//! function, and the BAR bookkeeping the frame buffer drivers built on it use.
//!
//! Upstream: sys/dev/pci/vga_pcivar.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - `struct vga_pci_softc`'s `pa` (a copy of the attach arguments) is not here: nothing in
//!   `vga_pci.c` reads it (only i386's `vesafb` and the drm drivers do), and a
//!   `pci_attach_args` is not valid all-zero, which a softc must be.
//! - The `NACPI > 0` register save areas are byte arrays of the sizes of the `vgareg.h`
//!   and `mc6845reg.h` structures (the C fills them through a `char *`); `X86EMU`'s
//!   `sc_posth` is an opaque pointer (`struct vga_post` is `vga_post.c`'s, not ported).
//!   Both are always present: amd64 GENERIC has `acpi` and `option X86EMU`.
//! - The `VESAFB` members (i386 only) are not here.
//! - `vga_aperture_needed` (`vga_pci_common.c`) is compiled only with `RAMDISK_HOOKS`,
//!   which no EmiBSD kernel configures; its prototype is not here.

use core::cell::Cell;
use core::ffi::c_void;

use crate::dev::ic::mc6845reg::RegMc6845;
use crate::dev::ic::vgareg::{RegVgaattr, RegVgagdc, RegVgats};
use crate::dev::ic::vgavar::VgaConfig;
use crate::dev::pci::pcireg::{
    PCI_CLASS_DISPLAY, PCI_CLASS_PREHISTORIC, PCI_SUBCLASS_DISPLAY_VGA,
    PCI_SUBCLASS_PREHISTORIC_VGA, pci_class, pci_subclass,
};
use crate::dev::pci::pcivar::Pcireg;
use crate::machine::bus::{BusAddr, BusSize, BusSpaceHandle, BusSpaceTag};
use crate::sys::device::{Device, Softc};

/// `VGA_PCI_MAX_BARS`.
pub const VGA_PCI_MAX_BARS: usize = 6;

/// `struct vga_pci_bar`.
#[derive(Clone, Copy)]
pub struct VgaPciBar {
    /// `addr`.
    pub addr: i32,
    /// `mapped`.
    pub mapped: u32,
    /// `maptype`.
    pub maptype: Pcireg,
    /// `base`.
    pub base: BusAddr,
    /// `size`.
    pub size: BusSize,
    /// `maxsize`.
    pub maxsize: BusSize,
    /// `bst`.
    pub bst: BusSpaceTag,
    /// `bsh`.
    pub bsh: BusSpaceHandle,
    /// `flags`.
    pub flags: i32,
    /// `vaddr`.
    pub vaddr: *mut c_void,
}

/// `struct vga_pci_softc`.
#[repr(C)]
pub struct VgaPciSoftc {
    /// `sc_dev`.
    pub sc_dev: Device,
    /// `sc_vc`.
    pub sc_vc: Cell<*const VgaConfig>,
    /// `sc_type`.
    pub sc_type: Cell<i32>,

    // NACPI > 0
    /// `sc_save_ts`: `struct reg_vgats`.
    pub sc_save_ts: Cell<[u8; size_of::<RegVgats>()]>,
    /// `sc_save_crtc`: `struct reg_mc6845`.
    pub sc_save_crtc: Cell<[u8; size_of::<RegMc6845>()]>,
    /// `sc_save_atc`: `struct reg_vgaattr`.
    pub sc_save_atc: Cell<[u8; size_of::<RegVgaattr>()]>,
    /// `sc_save_gdc`: `struct reg_vgagdc`.
    pub sc_save_gdc: Cell<[u8; size_of::<RegVgagdc>()]>,

    // X86EMU
    /// `sc_posth`: `struct vga_post *`.
    pub sc_posth: Cell<*mut c_void>,
}

impl VgaPciSoftc {
    /// `sc->sc_vc`.
    pub fn sc_vc(&self) -> Option<&VgaConfig> {
        // SAFETY: `vga_pci_attach` sets it to the configuration `vga_common_attach` made,
        // which lives for good.
        unsafe { self.sc_vc.get().as_ref() }
    }
}

// SAFETY: `#[repr(C)]` with the device first; every other member is a `Cell` of a pointer, an
// integer or a byte array, all valid all-zero.
unsafe impl Softc for VgaPciSoftc {}

/// `DEVICE_IS_VGA_PCI(class)`: a display controller of the VGA subclass, or a
/// pre-class-code VGA.
pub const fn device_is_vga_pci(class: Pcireg) -> bool {
    (pci_class(class) == PCI_CLASS_DISPLAY && pci_subclass(class) == PCI_SUBCLASS_DISPLAY_VGA)
        || (pci_class(class) == PCI_CLASS_PREHISTORIC
            && pci_subclass(class) == PCI_SUBCLASS_PREHISTORIC_VGA)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vga_class_codes() {
        // QEMU's std VGA: class 0x03, subclass 0x00 (class register 0x0300_0002).
        assert!(device_is_vga_pci(0x0300_0002));
        assert!(device_is_vga_pci(0x0001_0000));
        assert!(!device_is_vga_pci(0x0380_0000)); // other display
        assert!(!device_is_vga_pci(0x0200_0000)); // network
    }
}
