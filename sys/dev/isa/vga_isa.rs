/* $OpenBSD: vga_isa.c,v 1.11 2022/04/06 18:59:29 naddy Exp $ */
/* $NetBSD: vga_isa.c,v 1.3 1998/06/12 18:45:48 drochner Exp $ */
/* $OpenBSD: vga_isavar.h,v 1.4 2002/03/14 01:26:56 millert Exp $ */
/* $NetBSD: vga_isavar.h,v 1.1 1998/03/22 15:14:36 drochner Exp $ */
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
/*
 * Copyright (c) 1996 Carnegie-Mellon University.
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

//! `vga(4)` at `isa`: the VGA at its fixed ISA addresses (I/O 0x3b0-0x3df, memory
//! 0xa0000-0xbffff), driven by `vga.c`; and `<dev/isa/vga_isavar.h>`, whose one prototype
//! is [`vga_isa_cnattach`].
//!
//! Upstream: sys/dev/isa/vga_isa.c @ 3ce1f3f79392
//! Upstream: sys/dev/isa/vga_isavar.h @ 3ce1f3f79392
//!
//! On a PC whose VGA is a PCI function, OpenBSD finds it at `pci` first, and the ISA probe
//! fails because the VGA's I/O ports are then claimed in the `ioport_ex` extent. On QEMU's
//! q35 under UEFI (the `just smoke` machine) the probe fails earlier: the legacy text
//! memory does not answer, so neither `vga1 at pci0` nor `vga0 at isa0` attaches, as an
//! OpenBSD 8.0 snapshot shows on that machine.
//!
//! ## Deviations
//! - Without the `ioport_ex` extent (`subr_extent.c`, not ported; see
//!   `arch/amd64/amd64/bus_space.rs`) a map does not fail on ports another device holds:
//!   on a machine where a PCI VGA does attach (a BIOS boot, not how EmiBSD boots), this
//!   probe would succeed too and attach a second `vga`, where OpenBSD's fails.
//! - The `#if 0` softc member `sc_vc` is not here, as in C.

use core::ffi::c_void;

use crate::dev::ic::vga::{vga_cnattach, vga_common_attach, vga_common_probe, vga_is_console};
use crate::dev::isa::isavar::{DRQUNK, IOBASEUNK, IRQUNK, IsaAttachArgs, MADDRUNK};
use crate::dev::wscons::wsconsio::WSDISPLAY_TYPE_ISAVGA;
use crate::kern::subr_prf::printf;
use crate::machine::bus::BusSpaceTag;
use crate::sys::device::{CfMatch, Cfattach, Device, Softc};
use crate::sys::errno::Errno;

/// `struct vga_isa_softc`.
#[repr(C)]
pub struct VgaIsaSoftc {
    /// `sc_dev`.
    pub sc_dev: Device,
}

// SAFETY: `#[repr(C)]`, only the device, which is valid all-zero.
unsafe impl Softc for VgaIsaSoftc {}

/// `vga_isa_ca`.
pub static VGA_ISA_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<VgaIsaSoftc>(),
    ca_match: Some(vga_isa_match),
    ca_attach: vga_isa_attach,
    ca_detach: None,
    ca_activate: None,
};

/// `vga_isa_match`: the VGA at its fixed addresses, if the locators allow them and it is
/// the console or answers `vga_common_probe`; it claims the ranges and outbids a generic
/// `pcdisplay`.
pub fn vga_isa_match(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: isa attaches its children with an `isa_attach_args` it owns for the probe.
    let ia = unsafe { &mut *aux.cast::<IsaAttachArgs>() };

    // If values are hardwired to something that they can't be, punt.
    if (ia.ia_iobase() != IOBASEUNK && ia.ia_iobase() != 0x3b0)
        // ia->ia_iosize != 0 || XXX isa.c
        || (ia.ia_maddr() != MADDRUNK && ia.ia_maddr() != 0xa0000)
        || (ia.ia_msize() != 0 && ia.ia_msize() != 0x20000)
        || ia.ia_irq() != IRQUNK
        || ia.ia_drq() != DRQUNK
    {
        return 0;
    }

    if !vga_is_console(ia.ia_iot, WSDISPLAY_TYPE_ISAVGA as i32)
        && !vga_common_probe(ia.ia_iot, ia.ia_memt)
    {
        return 0;
    }

    ia.set_ia_iobase(0x3b0); // XXX mono 0x3b0 color 0x3c0
    ia.set_ia_iosize(0x30); // XXX 0x20
    ia.set_ia_maddr(0xa0000);
    ia.set_ia_msize(0x20000);
    2 // more than generic pcdisplay
}

/// `vga_isa_attach`.
pub fn vga_isa_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: isa attaches its children with an `isa_attach_args` it owns for the attach.
    let ia = unsafe { &*aux.cast::<IsaAttachArgs>() };

    printf(format_args!("\n"));

    let _ = vga_common_attach(self_, ia.ia_iot, ia.ia_memt, WSDISPLAY_TYPE_ISAVGA as i32);
}

/// `vga_isa_cnattach`: the ISA VGA as the console, after probing it.
pub fn vga_isa_cnattach(iot: BusSpaceTag, memt: BusSpaceTag) -> Result<(), Errno> {
    vga_cnattach(iot, memt, WSDISPLAY_TYPE_ISAVGA as i32, true)
}
