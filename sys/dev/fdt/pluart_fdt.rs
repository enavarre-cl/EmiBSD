/*	$OpenBSD: pluart_fdt.c,v 1.8 2022/06/27 13:03:32 anton Exp $	*/
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

//! The PL011 on the device tree: `dev/fdt/pluart_fdt.c`.
//!
//! Upstream: sys/dev/fdt/pluart_fdt.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M4 ports `pluart_init_cons`; `pluart_fdt_match` and
//! `pluart_fdt_attach` (the `cfattach`, `clock_enable_all`, `pinctrl_byname`,
//! `pluart_attach_common`) come with autoconfiguration (M5).

use crate::dev::ic::pluart::pluartcnattach;
use crate::dev::ofw::fdt::{FdtReg, fdt_get_reg};
use crate::machine::fdt::{fdt_cons_bs_tag, fdt_find_cons};
use crate::sys::termios::B115200;
use crate::sys::ttydefaults::TTYDEF_CFLAG;

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
