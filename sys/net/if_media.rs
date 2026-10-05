/*	$OpenBSD: if_media.h,v 1.47 2026/03/19 16:50:32 chris Exp $	*/
/*	$NetBSD: if_media.h,v 1.22 2000/02/17 21:53:16 sommerfeld Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (c) 1998, 2000 The NetBSD Foundation, Inc.
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

/*
 * Copyright (c) 1997
 *	Jonathan Stone and Jason R. Thorpe.  All rights reserved.
 *
 * This software is derived from information provided by Matt Thomas.
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
 *	This product includes software developed by Jonathan Stone
 *	and Jason R. Thorpe for the NetBSD Project.
 * 4. The names of the authors may not be used to endorse or promote products
 *    derived from this software without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHORS ``AS IS'' AND ANY EXPRESS OR
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES
 * OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED.
 * IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY DIRECT, INDIRECT,
 * INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING,
 * BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES;
 * LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED
 * AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY,
 * OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 */
/* </LICENSES> */

//! `<net/if_media.h>`: network interface media selection.
//!
//! Upstream: sys/net/if_media.h @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M7b needs the words `vio(4)`'s media status reports (Ethernet,
//! autoselect, full duplex, the valid and active status bits). `struct ifmedia`, the media
//! word tables and the `ifmedia_*` functions of `net/if_media.c` come with `ifconfig(8)`'s
//! media commands; until then a driver's `ifmedia_init`/`ifmedia_add`/`ifmedia_set`/
//! `ifmedia_ioctl` calls report themselves with `unported!`.
//!
//! ## Deviations
//! - Partial: only the media words ported drivers name.

/// `IFM_ETHER`: the Ethernet media type.
pub const IFM_ETHER: u64 = 0x0000_0000_0000_0100;
/// `IFM_AUTO`: autoselect best media.
pub const IFM_AUTO: u64 = 0;
/// `IFM_FDX`: force full duplex.
pub const IFM_FDX: u64 = 0x0000_0100_0000_0000;
/// `IFM_AVALID`: active bit valid.
pub const IFM_AVALID: u64 = 0x0000_0000_0000_0001;
/// `IFM_ACTIVE`: interface attached to working net.
pub const IFM_ACTIVE: u64 = 0x0000_0000_0000_0002;

// em(4) (M13): the Ethernet subtypes and options it reports and accepts, and the masks.

/// `IFM_10_T`: 10BaseT - RJ45.
pub const IFM_10_T: u64 = 3;
/// `IFM_100_TX`: 100BaseTX - RJ45.
pub const IFM_100_TX: u64 = 6;
/// `IFM_1000_SX`: 1000BaseSX - multi-mode fiber.
pub const IFM_1000_SX: u64 = 11;
/// `IFM_1000_LX`: 1000baseLX - single-mode fiber.
pub const IFM_1000_LX: u64 = 14;
/// `IFM_1000_T`: 1000baseT - 4 pair cat 5.
pub const IFM_1000_T: u64 = 16;
/// `IFM_ETH_MASTER`: master mode (1000baseT).
pub const IFM_ETH_MASTER: u64 = 0x0000_0000_0001_0000;
/// `IFM_ETH_RXPAUSE`: receive PAUSE frames.
pub const IFM_ETH_RXPAUSE: u64 = 0x0000_0000_0002_0000;
/// `IFM_ETH_TXPAUSE`: transmit PAUSE frames.
pub const IFM_ETH_TXPAUSE: u64 = 0x0000_0000_0004_0000;
/// `IFM_NONE`: deselect all media.
pub const IFM_NONE: u64 = 2;
/// `IFM_HDX`: force half duplex.
pub const IFM_HDX: u64 = 0x0000_0200_0000_0000;
/// `IFM_FLOW`: enable hardware flow control.
pub const IFM_FLOW: u64 = 0x0000_0400_0000_0000;
/// `IFM_NMASK`: network type.
pub const IFM_NMASK: u64 = 0x0000_0000_0000_ff00;
/// `IFM_TMASK`: media sub-type.
pub const IFM_TMASK: u64 = 0x0000_0000_0000_00ff;
/// `IFM_IMASK`: instance.
pub const IFM_IMASK: u64 = 0xff00_0000_0000_0000;
/// `IFM_ISHIFT`: instance shift.
pub const IFM_ISHIFT: u32 = 56;
/// `IFM_GMASK`: global options.
pub const IFM_GMASK: u64 = 0x00ff_ff00_0000_0000;

/// `IFM_TYPE(x)`: the network type of a media word.
pub const fn ifm_type(x: u64) -> u64 {
    x & IFM_NMASK
}

/// `IFM_SUBTYPE(x)`: the media sub-type of a media word.
pub const fn ifm_subtype(x: u64) -> u64 {
    x & IFM_TMASK
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/net/if_media.h");
        crate::reftest::assert_defines!(defs; IFM_ETHER, IFM_AUTO, IFM_FDX, IFM_AVALID, IFM_ACTIVE,
            IFM_10_T, IFM_100_TX, IFM_1000_SX, IFM_1000_LX, IFM_1000_T, IFM_ETH_MASTER,
            IFM_ETH_RXPAUSE, IFM_ETH_TXPAUSE, IFM_NONE, IFM_HDX, IFM_FLOW, IFM_NMASK, IFM_TMASK,
            IFM_IMASK, IFM_ISHIFT, IFM_GMASK);
    }

    #[test]
    fn type_and_subtype_split_a_media_word() {
        let w = IFM_ETHER | IFM_1000_T | IFM_FDX;
        assert_eq!(ifm_type(w), IFM_ETHER);
        assert_eq!(ifm_subtype(w), IFM_1000_T);
        assert_eq!(w & IFM_GMASK, IFM_FDX);
    }
}
