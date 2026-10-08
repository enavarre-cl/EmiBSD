/*	$OpenBSD: pluart_fdt.c,v 1.8 2022/06/27 13:03:32 anton Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 2014 Patrick Wildt <patrick@blueri.se>
 * Copyright (c) 2005 Dale Rahn <drahn@dalerahn.com>
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
//! The PL011 on the device tree: `dev/fdt/pluart_fdt.c`.
//!
//! Upstream: sys/dev/fdt/pluart_fdt.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `clock_enable_all`, `clock_get_frequency` (`ofw_clock.c`) and `pinctrl_byname`
//!   (`ofw_pinctrl.c`) are not ported: they are reported, and `sc_clkfreq` stays 0, so
//!   `pluart_param` keeps the baud rate the firmware programmed (as the C does for a UART
//!   without a known clock).
//! - The attach arguments are the machine's `struct fdt_attach_args`
//!   (`machine::fdt::FdtAttachArgs`).

use core::ffi::c_void;
use core::ptr;

use crate::dev::ic::pluart::{
    COM_HW_SBSA, PluartSoftc, pluart_attach_common, pluart_intr, pluartcnattach,
};
use crate::dev::ofw::fdt::{FdtReg, OF_getpropint, OF_is_compatible, fdt_get_reg};
use crate::kern::subr_prf::{panic, printf};
use crate::machine::bus::bus_space_map;
use crate::machine::fdt::{
    FdtAttachArgs, fdt_cons_bs_tag, fdt_find_cons, fdt_intr_establish, stdout_node,
};
use crate::machine::intr::IPL_TTY;
use crate::sys::device::{CfMatch, Cfattach, Device};
use crate::sys::termios::B115200;
use crate::sys::ttydefaults::TTYDEF_CFLAG;
use crate::unported;

/// `pluart_fdt_ca`.
pub static PLUART_FDT_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<PluartSoftc>(),
    ca_match: Some(pluart_fdt_match),
    ca_attach: pluart_fdt_attach,
    ca_detach: None,
    ca_activate: None,
};

/// `pluart_init_cons`: attaches the PL011 `/chosen` names as the console.
pub fn pluart_init_cons() {
    let node = fdt_find_cons(b"arm,pl011");
    if node.is_null() {
        return;
    }
    let mut reg = FdtReg::default();
    if fdt_get_reg(node, 0, &mut reg).is_err() {
        return;
    }
    // SAFETY: the device tree's PL011, which nothing else drives.
    let _ = unsafe {
        pluartcnattach(
            fdt_cons_bs_tag(),
            reg.addr as usize,
            B115200 as i32,
            TTYDEF_CFLAG,
        )
    };
}

/// `pluart_fdt_match`: a node compatible with `arm,pl011`.
pub fn pluart_fdt_match(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: the device-tree buses hand their children `struct fdt_attach_args`.
    let faa = unsafe { &*aux.cast::<FdtAttachArgs<'_>>() };

    i32::from(OF_is_compatible(faa.fa_node, b"arm,pl011"))
}

/// `pluart_fdt_attach`: map the UART, establish its interrupt and attach it.
pub fn pluart_fdt_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: as in `pluart_fdt_match`.
    let faa = unsafe { &*aux.cast::<FdtAttachArgs<'_>>() };
    // SAFETY: `pluart_fdt_ca` makes `PluartSoftc`s; `config_make_softc`'s allocation lives
    // as long as the device, which is never detached.
    let sc: &'static PluartSoftc = unsafe { &*ptr::from_ref(self_.softc::<PluartSoftc>()) };

    let Some(reg) = faa.fa_reg.first() else {
        printf(format_args!(": no registers\n"));
        return;
    };

    if OF_is_compatible(faa.fa_node, b"arm,sbsa-uart") {
        sc.sc_hwflags.set(sc.sc_hwflags.get() | COM_HW_SBSA);
    } else {
        // clock_enable_all(faa->fa_node); sc_clkfreq = clock_get_frequency(node, "uartclk").
        let _ = unported!("pluart_fdt_attach: clock_enable_all, clock_get_frequency (ofw_clock.c)");
        sc.sc_clkfreq.set(0);
    }

    let periphid = OF_getpropint(faa.fa_node, b"arm,primecell-periphid", 0);
    if periphid != 0 {
        sc.sc_hwrev.set(((periphid >> 20) & 0x0f) as u8);
    }

    // SAFETY: the softc is the device, alive while attached (see above).
    let name: &'static str = unsafe { &*ptr::from_ref(sc.sc_dev.xname()) };
    let irq = fdt_intr_establish(
        faa.fa_node,
        IPL_TTY,
        pluart_intr,
        ptr::from_ref(sc).cast_mut().cast::<c_void>(),
        name,
    );
    sc.sc_irq.set(irq.map_or(ptr::null_mut(), |ih| ih.as_ptr()));

    sc.sc_iot.set(Some(faa.fa_iot));
    // SAFETY: the node's register window, which only this driver drives.
    match unsafe { bus_space_map(faa.fa_iot, reg.addr as usize, reg.size as usize, 0) } {
        Ok(ioh) => sc.sc_ioh.set(Some(ioh)),
        Err(_) => panic(format_args!("pluartattach: bus_space_map failed!")),
    }

    // pinctrl_byname(faa->fa_node, "default"): ofw_pinctrl.c (not ported).
    let _ = unported!("pluart_fdt_attach: pinctrl_byname (ofw_pinctrl.c)");

    pluart_attach_common(sc, stdout_node() == faa.fa_node);
}
/* </CODE> */
