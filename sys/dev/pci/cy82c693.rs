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

/*-
 * Copyright (c) 2000 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Jason R. Thorpe.
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
/* </LICENSES> */

/* <CODE> */
//! Common routines to read/write control registers on the Cypress 82c693 hyperCache(tm)
//! Stand-Alone PCI Peripheral Controller with USB (`pciide.c`'s cy693 path).
//!
//! Upstream: sys/dev/pci/cy82c693.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `cyhc_handle` and `cyhc_initialized` are one `StaticCell<Option<Cy82c693Handle>>`:
//!   `Some` is "initialized". Like the C, it is written once, at autoconf time, by the
//!   first `cy82c693_init`, and only read afterwards.
//! - `cy82c693_init`'s `panic("cy82c693_init")` when called again with another tag is not
//!   made: the machine's bus space tags have no equality (`BusSpace::Tag` is only `Copy`).
//!   Its one caller, `cy693_chip_map`, always passes its PCI bus's I/O tag.

use libkern::StaticCell;

use crate::dev::pci::cy82c693reg::CYHC_CONFIG_ADDR;
use crate::dev::pci::cy82c693var::Cy82c693Handle;
use crate::kern::subr_prf::{panic, printf};
use crate::machine::bus::{BusSpaceTag, bus_space_map, bus_space_read_1, bus_space_write_1};

/// `cyhc_handle` and `cyhc_initialized`: `Some` once `cy82c693_init` mapped the ports.
static CYHC_HANDLE: StaticCell<Option<Cy82c693Handle>> = StaticCell::new(None);

/// `cy82c693_init`: maps the chipset configuration ports, once.
pub fn cy82c693_init(iot: BusSpaceTag) -> Option<&'static Cy82c693Handle> {
    // SAFETY: written only below, at autoconf time (a single thread under the kernel
    // lock), before any reader has a reference.
    if let Some(h) = unsafe { CYHC_HANDLE.get() } {
        return Some(h);
    }

    // SAFETY: the chipset configuration address/data ports belong to the CY82C693, which
    // the caller found on the PCI bus.
    let ioh = match unsafe { bus_space_map(iot, CYHC_CONFIG_ADDR, 2, 0) } {
        Ok(ioh) => ioh,
        Err(error) => {
            printf(format_args!(
                "cy82c693_init: bus_space_map failed ({})",
                error as i32
            ));
            return None;
        }
    };

    // SAFETY: the one write, at autoconf time; no reference to the cell is live.
    unsafe {
        CYHC_HANDLE.write(Some(Cy82c693Handle {
            cyhc_iot: iot,
            cyhc_ioh: ioh,
        }))
    };

    // SAFETY: as above; never written again.
    unsafe { CYHC_HANDLE.get() }.as_ref()
}

/// `cyhc_initialized`.
fn cyhc_initialized() -> bool {
    // SAFETY: written once, by `cy82c693_init`, before any handle exists to pass here.
    unsafe { CYHC_HANDLE.get() }.is_some()
}

/// `cy82c693_read`.
pub fn cy82c693_read(cyhc: &Cy82c693Handle, reg: u8) -> u8 {
    if !cyhc_initialized() {
        panic(format_args!("cy82c693_read"));
    }

    bus_space_write_1(cyhc.cyhc_iot, cyhc.cyhc_ioh, 0, reg);
    bus_space_read_1(cyhc.cyhc_iot, cyhc.cyhc_ioh, 1)
}

/// `cy82c693_write`.
pub fn cy82c693_write(cyhc: &Cy82c693Handle, reg: u8, val: u8) {
    if !cyhc_initialized() {
        panic(format_args!("cy82c693_write"));
    }

    bus_space_write_1(cyhc.cyhc_iot, cyhc.cyhc_ioh, 0, reg);
    bus_space_write_1(cyhc.cyhc_iot, cyhc.cyhc_ioh, 1, val);
}
/* </CODE> */
