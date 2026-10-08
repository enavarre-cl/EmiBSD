/*	$OpenBSD: ipmi_fdt.c,v 1.3 2024/10/09 00:38:26 jsg Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 2020 Mark Kettenis <kettenis@openbsd.org>
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
//! ipmi(4) on the device tree: `dev/fdt/ipmi_fdt.c`. An `ipmi-kcs` node is a KCS interface
//! (IPMI 2.0) in memory space at its first `reg`, with optional `reg-size` (the register
//! width) and `reg-spacing` properties; the rest is `ipmi_attach_common`.
//!
//! Upstream: sys/dev/fdt/ipmi_fdt.c @ 3ce1f3f79392
//!
//! arm64's GENERIC has `ipmi* at fdt?`; QEMU's `virt` has no `ipmi-kcs` node, so it attaches
//! nowhere in the smokes.
//!
//! ## Deviations
//! - The attach arguments are the machine's `struct fdt_attach_args`
//!   (`machine::fdt::FdtAttachArgs`).

use core::ffi::c_void;
use core::ptr;

use crate::dev::ipmi::{ipmi_activate, ipmi_attach_common};
use crate::dev::ipmivar::{IPMI_IF_KCS, IpmiAttachArgs, IpmiSoftc};
use crate::dev::ofw::fdt::{OF_getpropint, OF_is_compatible};
use crate::kprintf;
use crate::machine::fdt::FdtAttachArgs;
use crate::sys::device::{CfMatch, Cfattach, Device};

/// `ipmi_fdt_ca`.
pub static IPMI_FDT_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<IpmiSoftc>(),
    ca_match: Some(ipmi_fdt_match),
    ca_attach: ipmi_fdt_attach,
    ca_detach: None,
    ca_activate: Some(ipmi_activate),
};

/// `ipmi_fdt_match(parent, match, aux)`: an `ipmi-kcs` node.
pub fn ipmi_fdt_match(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: the device-tree buses hand their children `struct fdt_attach_args`.
    let faa = unsafe { &*aux.cast::<FdtAttachArgs<'_>>() };

    i32::from(OF_is_compatible(faa.fa_node, b"ipmi-kcs"))
}

/// `ipmi_fdt_attach(parent, self, aux)`: a KCS interface at the node's first register.
pub fn ipmi_fdt_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: `ipmi_fdt_ca` makes `IpmiSoftc`s, never detached.
    let sc: &'static IpmiSoftc = unsafe { &*ptr::from_ref(self_.softc::<IpmiSoftc>()) };
    // SAFETY: as in `ipmi_fdt_match`.
    let faa = unsafe { &*aux.cast::<FdtAttachArgs<'_>>() };

    let Some(reg) = faa.fa_reg.first() else {
        kprintf!(": no registers\n");
        return;
    };

    let ia = IpmiAttachArgs {
        iaa_memt: Some(faa.fa_iot),
        iaa_if_type: IPMI_IF_KCS,
        iaa_if_rev: 0x20,
        iaa_if_irq: -1,
        iaa_if_irqlvl: 0,
        iaa_if_iosize: OF_getpropint(faa.fa_node, b"reg-size", 1) as i32,
        iaa_if_iospacing: OF_getpropint(faa.fa_node, b"reg-spacing", 1) as i32,
        iaa_if_iobase: reg.addr as usize,
        iaa_if_iotype: b'm',
        ..IpmiAttachArgs::zeroed()
    };

    ipmi_attach_common(sc, &ia);
}
/* </CODE> */
