/*	$NetBSD$	*/
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
 * Copyright (c) 2001 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Enami Tsugutomo.
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
//! ASIX AX88190 and AX88790 register definitions (`dev/ic/ax88190reg.h`).
//!
//! Upstream: sys/dev/ic/ax88190reg.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - `AX88190_MEMR` is a register offset (`usize`), its bits `u8`; the NIC memory offsets
//!   (`AX88190_NODEID_OFFSET`, `AX88190_BUFFER_START`) are `i32` as `ne2000_readmem`'s `src`;
//!   the I/O window constants are `u16`/`usize`.

/// `AX88190_MEMR`: MII/EEPROM/ Management Register.
pub const AX88190_MEMR: usize = 0x04;
/// `AX88190_MEMR_MDC`: MII Clock.
pub const AX88190_MEMR_MDC: u8 = 0x01;
/// `AX88190_MEMR_MDIR`: MII STA MDIO signal direction, assert -> input.
pub const AX88190_MEMR_MDIR: u8 = 0x02;
/// `AX88190_MEMR_MDI`: MII Data In.
pub const AX88190_MEMR_MDI: u8 = 0x04;
/// `AX88190_MEMR_MDO`: MII Data Out.
pub const AX88190_MEMR_MDO: u8 = 0x08;
/// `AX88190_MEMR_EECS`: EEPROM Chip Select.
pub const AX88190_MEMR_EECS: u8 = 0x10;
/// `AX88190_MEMR_EEI`: EEPROM Data In.
pub const AX88190_MEMR_EEI: u8 = 0x20;
/// `AX88190_MEMR_EEO`: EEPROM Data Out.
pub const AX88190_MEMR_EEO: u8 = 0x40;
/// `AX88190_MEMR_EECLK`: EEPROM Clock.
pub const AX88190_MEMR_EECLK: u8 = 0x80;
/// `AX88190_LAN_IOBASE`.
pub const AX88190_LAN_IOBASE: u16 = 0x3ca;
/// `AX88190_LAN_IOSIZE`.
pub const AX88190_LAN_IOSIZE: usize = 4;
/// `AX88790_CSR`.
pub const AX88790_CSR: u16 = 0x3c2;
/// `AX88790_CSR_SIZE`.
pub const AX88790_CSR_SIZE: usize = 2;
/// `AX88190_NODEID_OFFSET`.
pub const AX88190_NODEID_OFFSET: i32 = 0x400;
/// `AX88190_BUFFER_START`.
pub const AX88190_BUFFER_START: i32 = 0x800;
/* </CODE> */
