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
 * by Charles M. Hannum.
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

/*-
 * Copyright (c) 1992, 1993
 *	The Regents of the University of California.  All rights reserved.
 *
 * This code is derived from software contributed to Berkeley by
 * Ralph Campbell and Rick Macklem.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. Neither the name of the University nor the names of its contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE REGENTS AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE REGENTS OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 *	@(#)if_lereg.h	8.1 (Berkeley) 6/10/93
 */
/* </LICENSES> */

/* <CODE> */
//! `<dev/ic/am79900reg.h>`: the 32-bit software model (ILACC, PCnet-PCI) of the AMD LANCE
//! family: the receive and transmit message descriptors, the initialization block and their
//! bits. pcn(4) uses it with software styles 2 and 3 (`LE_B20_SSTYLE_PCNETPCI2/3` in
//! `lancereg.rs`).
//!
//! Upstream: sys/dev/ic/am79900reg.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - The descriptors and the initialization block are `#[repr(C)]` structs of plain `u32`
//!   words (the `int32_t` members, `rmd3`, `tmd3` and `pad`, are `u32` too: nothing reads
//!   them), 16 bytes for a descriptor and 32 for the block, as in C. A const assertion checks
//!   the sizes. `init_ladrf` is the four `u16` of the logical address filter; the offsets in
//!   the C comments are the C's.
//! - Constants are `u32`. `LE_T3_BITS` keeps the C name; its string has the octal escapes
//!   written as `\xNN`. The `#if 0` `LE_T3_TDR_MASK` is not ported.

/// `struct lermd`: receive message descriptor.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Lermd {
    /// `rmd0`.
    pub rmd0: u32,
    /// `rmd1`.
    pub rmd1: u32,
    /// `rmd2`.
    pub rmd2: u32,
    /// `rmd3`.
    pub rmd3: u32,
}

/// `struct letmd`: transmit message descriptor.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Letmd {
    /// `tmd0`.
    pub tmd0: u32,
    /// `tmd1`.
    pub tmd1: u32,
    /// `tmd2`.
    pub tmd2: u32,
    /// `tmd3`.
    pub tmd3: u32,
}

/// `struct leinit`: initialization block.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Leinit {
    /// `init_mode`: +0x0000.
    pub init_mode: u32,
    /// `init_padr`: +0x0002.
    pub init_padr: [u32; 2],
    /// `init_ladrf`: +0x0008.
    pub init_ladrf: [u16; 4],
    /// `init_rdra`: +0x0010.
    pub init_rdra: u32,
    /// `init_tdra`: +0x0014.
    pub init_tdra: u32,
    /// `pad`: pad to 16 shorts.
    pub pad: u32,
}

const _: () = {
    assert!(size_of::<Lermd>() == 16);
    assert!(size_of::<Letmd>() == 16);
    assert!(size_of::<Leinit>() == 32);
};

/// `LE_R1_OWN`: LANCE owns the packet.
pub const LE_R1_OWN: u32 = 1 << 31;
/// `LE_R1_ERR`: error summary.
pub const LE_R1_ERR: u32 = 1 << 30;
/// `LE_R1_FRAM`: framing error.
pub const LE_R1_FRAM: u32 = 1 << 29;
/// `LE_R1_OFLO`: overflow error.
pub const LE_R1_OFLO: u32 = 1 << 28;
/// `LE_R1_CRC`: CRC error.
pub const LE_R1_CRC: u32 = 1 << 27;
/// `LE_R1_BUFF`: buffer error.
pub const LE_R1_BUFF: u32 = 1 << 26;
/// `LE_R1_STP`: start of packet.
pub const LE_R1_STP: u32 = 1 << 25;
/// `LE_R1_ENP`: end of packet.
pub const LE_R1_ENP: u32 = 1 << 24;
/// `LE_R1_ONES`: must be ones.
pub const LE_R1_ONES: u32 = 0xf << 12;
/// `LE_R1_BCNT_MASK`: byte count mask.
pub const LE_R1_BCNT_MASK: u32 = 0xfff;

/// `LE_R1_BITS`: the `%b` bit names of `rmd1`.
pub const LE_R1_BITS: &str = "\x10\x20OWN\x1fERR\x1eFRAM\x1dOFLO\x1cCRC\x1bBUFF\x1aSTP\x19ENP";

/// `LE_T1_OWN`: LANCE owns the packet.
pub const LE_T1_OWN: u32 = 1 << 31;
/// `LE_T1_ERR`: error summary.
pub const LE_T1_ERR: u32 = 1 << 30;
/// `LE_T1_ADD_FCS`: add FCS (PCnet-PCI).
pub const LE_T1_ADD_FCS: u32 = 1 << 29;
/// `LE_T1_NO_FCS`: no FCS (ILACC).
pub const LE_T1_NO_FCS: u32 = 1 << 29;
/// `LE_T1_MORE`: multiple collisions.
pub const LE_T1_MORE: u32 = 1 << 28;
/// `LE_T1_LTINT`: transmit interrupt (if LTINTEN).
pub const LE_T1_LTINT: u32 = 1 << 28;
/// `LE_T1_ONE`: single collision.
pub const LE_T1_ONE: u32 = 1 << 27;
/// `LE_T1_DEF`: deferred transmit.
pub const LE_T1_DEF: u32 = 1 << 26;
/// `LE_T1_STP`: start of packet.
pub const LE_T1_STP: u32 = 1 << 25;
/// `LE_T1_ENP`: end of packet.
pub const LE_T1_ENP: u32 = 1 << 24;
/// `LE_T1_ONES`: must be ones.
pub const LE_T1_ONES: u32 = 0xf << 12;
/// `LE_T1_BCNT_MASK`: byte count mask.
pub const LE_T1_BCNT_MASK: u32 = 0xfff;

/// `LE_T1_BITS`: the `%b` bit names of `tmd1`.
pub const LE_T1_BITS: &str = "\x10\x20OWN\x1fERR\x1eRES\x1dMORE\x1cONE\x1bDEF\x1aSTP\x19ENP";

/// `LE_T2_BUFF`: buffer error.
pub const LE_T2_BUFF: u32 = 1 << 31;
/// `LE_T2_UFLO`: underflow error.
pub const LE_T2_UFLO: u32 = 1 << 30;
/// `LE_T2_EXDEF`: excessive deferral.
pub const LE_T2_EXDEF: u32 = 1 << 29;
/// `LE_T2_LCOL`: late collision.
pub const LE_T2_LCOL: u32 = 1 << 28;
/// `LE_T2_LCAR`: loss of carrier.
pub const LE_T2_LCAR: u32 = 1 << 27;
/// `LE_T2_RTRY`: retry error.
pub const LE_T2_RTRY: u32 = 1 << 26;

/// `LE_T3_BITS`: the `%b` bit names of transmit message descriptor 3.
pub const LE_T3_BITS: &str = "\x0a\x20BUFF\x1fUFLO\x1dLCOL\x1cLCAR\x1bRTRY";
/* </CODE> */
