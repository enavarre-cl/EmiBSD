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
 * Copyright (c) 1998 The NetBSD Foundation, Inc.
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
/* </LICENSES> */

/* <CODE> */
//! Realtek 8019 and 8029 NE2000-compatible media support (`dev/ic/rtl80x9.c`,
//! `dev/ic/rtl80x9var.h`).
//!
//! Upstream: sys/dev/ic/rtl80x9.c @ 3ce1f3f79392
//! Upstream: sys/dev/ic/rtl80x9var.h @ 3ce1f3f79392
//!
//! The chips keep their media configuration in page 3 of the DP8390 registers (`CONFIG0`
//! to `CONFIG3`). `rtl80x9_media_init` reads which port and duplex the board is configured
//! for and makes it the default media, `rtl80x9_init_card` writes the selected media back
//! on every `dp8390_init`, and `rtl80x9_mediastatus` reads it. The prototypes of
//! `rtl80x9var.h` are these functions.
//!
//! ## Deviations
//! - `rtl80x9_mediachange` returns `Result<(), Errno>`, as the other media change callbacks.
//! - The C leaves `defmedia` unset when `CONFIG2`'s port select is a value the match does
//!   not name; the four values the two bits can take are all named here.

use core::ptr;

use crate::dev::ic::dp8390::{dp8390_mediachange, dp8390_mediastatus, dp8390_reset};
use crate::dev::ic::dp8390reg::{ED_CR_PAGE_0, ED_CR_PAGE_3, ED_CR_STA, ED_CR_STP, ED_P0_CR};
use crate::dev::ic::dp8390var::Dp8390Softc;
use crate::dev::ic::rtl80x9reg::{
    NERTL_RTL3_CONFIG0, NERTL_RTL3_CONFIG2, NERTL_RTL3_CONFIG3, NERTL_RTL3_EECR, RTL3_CONFIG0_BNC,
    RTL3_CONFIG2_PL0, RTL3_CONFIG2_PL1, RTL3_CONFIG3_FUDUP, RTL3_EECR_EEM0, RTL3_EECR_EEM1,
};
use crate::machine::bus::{bus_space_read_1, bus_space_write_1};
use crate::net::if_::{IFF_RUNNING, Ifmediareq};
use crate::net::if_media::{
    IFM_10_2, IFM_10_T, IFM_AUTO, IFM_ETHER, IFM_FDX, ifm_subtype, ifmedia_add, ifmedia_init,
    ifmedia_set,
};
use crate::sys::errno::Errno;

/// `rtl80x9_mediachange`.
pub fn rtl80x9_mediachange(dsc: &'static Dp8390Softc) -> Result<(), Errno> {
    // Current media is already set up. Just reset the interface to let the new value take
    // hold. The new media will be set up in ne_pci_rtl8029_init_card() called via
    // dp8390_init().
    dp8390_reset(dsc);
    Ok(())
}

/// `cr_proto | STA or STP` by whether the interface runs, as `rtl80x9_mediastatus` and
/// `rtl80x9_init_card` start.
fn running_cr_proto(sc: &'static Dp8390Softc) -> u8 {
    sc.cr_proto.get()
        | if sc.ifp().if_flags.get() & IFF_RUNNING != 0 {
            ED_CR_STA
        } else {
            ED_CR_STP
        }
}

/// `rtl80x9_mediastatus`.
pub fn rtl80x9_mediastatus(sc: &'static Dp8390Softc, ifmr: &mut Ifmediareq) {
    let cr_proto = running_cr_proto(sc);

    // Sigh, can detect which media is being used, but can't detect if we have link or not.

    // Set NIC to page 3 registers.
    sc.nic_put(ED_P0_CR, cr_proto | ED_CR_PAGE_3);

    if sc.nic_get(NERTL_RTL3_CONFIG0) & RTL3_CONFIG0_BNC != 0 {
        ifmr.ifm_active = IFM_ETHER | IFM_10_2;
    } else {
        ifmr.ifm_active = IFM_ETHER | IFM_10_T;
        if sc.nic_get(NERTL_RTL3_CONFIG3) & RTL3_CONFIG3_FUDUP != 0 {
            ifmr.ifm_active |= IFM_FDX;
        }
    }

    // Set NIC to page 0 registers.
    sc.nic_put(ED_P0_CR, cr_proto | ED_CR_PAGE_0);
}

/// `rtl80x9_init_card`.
pub fn rtl80x9_init_card(sc: &'static Dp8390Softc) {
    let cur = sc.sc_media().ifm_cur().map_or(0, |e| e.ifm_media);
    let cr_proto = running_cr_proto(sc);

    // Set NIC to page 3 registers.
    sc.nic_put(ED_P0_CR, cr_proto | ED_CR_PAGE_3);

    // write enable config1-3.
    sc.nic_put(NERTL_RTL3_EECR, RTL3_EECR_EEM1 | RTL3_EECR_EEM0);

    // First, set basic media type.
    let mut reg = sc.nic_get(NERTL_RTL3_CONFIG2);
    reg &= !(RTL3_CONFIG2_PL1 | RTL3_CONFIG2_PL0);
    match ifm_subtype(cur) {
        IFM_AUTO => {
            // Nothing to do; both bits clear == auto-detect.
        }
        IFM_10_T => {
            // According to docs, this should be:
            // reg |= RTL3_CONFIG2_PL0;
            // but this doesn't work, so make it the same as AUTO.
        }
        IFM_10_2 => {
            reg |= RTL3_CONFIG2_PL1 | RTL3_CONFIG2_PL0;
        }
        _ => {}
    }
    sc.nic_put(NERTL_RTL3_CONFIG2, reg);

    // Now, set duplex mode.
    let mut reg = sc.nic_get(NERTL_RTL3_CONFIG3);
    if cur & IFM_FDX != 0 {
        reg |= RTL3_CONFIG3_FUDUP;
    } else {
        reg &= !RTL3_CONFIG3_FUDUP;
    }
    sc.nic_put(NERTL_RTL3_CONFIG3, reg);

    // write disable config1-3
    sc.nic_put(NERTL_RTL3_EECR, 0);

    // Set NIC to page 0 registers.
    sc.nic_put(ED_P0_CR, cr_proto | ED_CR_PAGE_0);
}

/// `rtl80x9_media[]`.
static RTL80X9_MEDIA: [u64; 4] = [
    IFM_ETHER | IFM_AUTO,
    IFM_ETHER | IFM_10_T,
    IFM_ETHER | IFM_10_T | IFM_FDX,
    IFM_ETHER | IFM_10_2,
];

/// The media `CONFIG2`'s port select and `CONFIG3`'s duplex bit configure the board for.
pub fn rtl80x9_defmedia(conf2: u8, conf3: u8) -> u64 {
    match conf2 & (RTL3_CONFIG2_PL1 | RTL3_CONFIG2_PL0) {
        0 => IFM_ETHER | IFM_AUTO,

        // RTL3_CONFIG2_PL1 alone: XXX rtl docs sys 10base5, but chip cant do
        x if x & RTL3_CONFIG2_PL1 != 0 => IFM_ETHER | IFM_10_2,

        // RTL3_CONFIG2_PL0
        _ => {
            if conf3 & RTL3_CONFIG3_FUDUP != 0 {
                IFM_ETHER | IFM_10_T | IFM_FDX
            } else {
                IFM_ETHER | IFM_10_T
            }
        }
    }
}

/// `rtl80x9_media_init`.
pub fn rtl80x9_media_init(sc: &'static Dp8390Softc) {
    let (t, h) = sc.regs();

    // Set NIC to page 3 registers.
    bus_space_write_1(t, h, ED_P0_CR, ED_CR_PAGE_3);

    let conf2 = bus_space_read_1(t, h, NERTL_RTL3_CONFIG2);
    let conf3 = bus_space_read_1(t, h, NERTL_RTL3_CONFIG3);

    let defmedia = rtl80x9_defmedia(conf2, conf3);

    // Set NIC to page 0 registers.
    bus_space_write_1(t, h, ED_P0_CR, ED_CR_PAGE_0);

    ifmedia_init(sc.sc_media(), 0, dp8390_mediachange, dp8390_mediastatus);
    for &m in RTL80X9_MEDIA.iter() {
        ifmedia_add(sc.sc_media(), m, 0, ptr::null_mut());
    }
    ifmedia_set(sc.sc_media(), defmedia);
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defmedia_follows_config2_and_config3() {
        assert_eq!(rtl80x9_defmedia(0, 0), IFM_ETHER | IFM_AUTO);
        assert_eq!(
            rtl80x9_defmedia(0x3f, RTL3_CONFIG3_FUDUP),
            IFM_ETHER | IFM_AUTO
        );
        assert_eq!(
            rtl80x9_defmedia(RTL3_CONFIG2_PL1 | RTL3_CONFIG2_PL0, 0),
            IFM_ETHER | IFM_10_2
        );
        assert_eq!(rtl80x9_defmedia(RTL3_CONFIG2_PL1, 0), IFM_ETHER | IFM_10_2);
        assert_eq!(rtl80x9_defmedia(RTL3_CONFIG2_PL0, 0), IFM_ETHER | IFM_10_T);
        assert_eq!(
            rtl80x9_defmedia(RTL3_CONFIG2_PL0, RTL3_CONFIG3_FUDUP),
            IFM_ETHER | IFM_10_T | IFM_FDX
        );
    }
}
/* </TESTS> */
