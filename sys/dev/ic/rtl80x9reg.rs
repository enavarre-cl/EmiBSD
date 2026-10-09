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
//! Realtek 8019 and 8029 NE2000-compatible register definitions (`dev/ic/rtl80x9reg.h`).
//!
//! Upstream: sys/dev/ic/rtl80x9reg.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - Register numbers (`NERTL_*`) are `usize` (they index `sc_reg_map`), bits and the
//!   character constants (`RTL0_8019ID0`, ...) are `u8`. The C's bit-pattern comments that
//!   follow some registers are kept in the docs of the register.

/// `NERTL_RTL0_8019ID0`: 8019 ID Register 0.
pub const NERTL_RTL0_8019ID0: usize = 0x0a;
/// `RTL0_8019ID0`.
pub const RTL0_8019ID0: u8 = b'P';
/// `NERTL_RTL0_8019ID1`: 8019 ID Register 1.
pub const NERTL_RTL0_8019ID1: usize = 0x0b;
/// `RTL0_8019ID1`.
pub const RTL0_8019ID1: u8 = b'p';
/// `NERTL_RTL3_EECR`: EEPROM Command Register.
pub const NERTL_RTL3_EECR: usize = 0x01;
/// `RTL3_EECR_EEM1`: EEPROM Operating Mode.
pub const RTL3_EECR_EEM1: u8 = 0x80;
/// `RTL3_EECR_EEM0`.
pub const RTL3_EECR_EEM0: u8 = 0x40;
/// `RTL3_EECR_EECS`: EEPROM Chip Select.
pub const RTL3_EECR_EECS: u8 = 0x08;
/// `RTL3_EECR_EESK`: EEPROM Clock.
pub const RTL3_EECR_EESK: u8 = 0x04;
/// `RTL3_EECR_EEDI`: EEPROM Data In.
pub const RTL3_EECR_EEDI: u8 = 0x02;
/// `RTL3_EECR_EEDO`: EEPROM Data Out.
pub const RTL3_EECR_EEDO: u8 = 0x01;
/// `NERTL_RTL3_BPAGE`: BROM Page Register (8019).
pub const NERTL_RTL3_BPAGE: usize = 0x02;
/// `NERTL_RTL3_CONFIG0`: Configuration 0 (ro).
pub const NERTL_RTL3_CONFIG0: usize = 0x03;
/// `RTL3_CONFIG0_JP`: jumper mode (8019).
pub const RTL3_CONFIG0_JP: u8 = 0x08;
/// `RTL3_CONFIG0_BNC`: BNC is active.
pub const RTL3_CONFIG0_BNC: u8 = 0x04;
/// `NERTL_RTL3_CONFIG1`: Configuration 1 (8019).
pub const NERTL_RTL3_CONFIG1: usize = 0x04;
/// `RTL3_CONFIG1_IRQEN`: IRQ Enable.
pub const RTL3_CONFIG1_IRQEN: u8 = 0x80;
/// `RTL3_CONFIG1_IRQS2`: IRQ Select.
pub const RTL3_CONFIG1_IRQS2: u8 = 0x40;
/// `RTL3_CONFIG1_IRQS1`.
pub const RTL3_CONFIG1_IRQS1: u8 = 0x20;
/// `RTL3_CONFIG1_IRQS0`.
pub const RTL3_CONFIG1_IRQS0: u8 = 0x10;
/// `RTL_CONFIG1_IOS3`: I/O base Select.
pub const RTL_CONFIG1_IOS3: u8 = 0x08;
/// `RTL_CONFIG1_IOS2`.
pub const RTL_CONFIG1_IOS2: u8 = 0x04;
/// `RTL_CONFIG1_IOS1`.
pub const RTL_CONFIG1_IOS1: u8 = 0x02;
/// `RTL_CONFIG1_IOS0`.
pub const RTL_CONFIG1_IOS0: u8 = 0x01;
/// `NERTL_RTL3_CONFIG2`: Configuration 2.
pub const NERTL_RTL3_CONFIG2: usize = 0x05;
/// `RTL3_CONFIG2_PL1`: Network media type.
pub const RTL3_CONFIG2_PL1: u8 = 0x80;
/// `RTL3_CONFIG2_PL0`.
pub const RTL3_CONFIG2_PL0: u8 = 0x40;
/// `RTL3_CONFIG2_8029FCE`: Flow Control Enable.
pub const RTL3_CONFIG2_8029FCE: u8 = 0x20;
/// `RTL3_CONFIG2_8029PF`: Pause Flag.
pub const RTL3_CONFIG2_8029PF: u8 = 0x10;
/// `RTL3_CONFIG2_8029BS1`: Boot Rom Size.
pub const RTL3_CONFIG2_8029BS1: u8 = 0x02;
/// `RTL3_CONFIG2_8029BS0`.
pub const RTL3_CONFIG2_8029BS0: u8 = 0x01;
/// `RTL3_CONFIG2_8019BSELB`: BROM disable.
pub const RTL3_CONFIG2_8019BSELB: u8 = 0x20;
/// `RTL3_CONFIG2_8019BS4`: BROM size/base.
pub const RTL3_CONFIG2_8019BS4: u8 = 0x10;
/// `RTL3_CONFIG2_8019BS3`.
pub const RTL3_CONFIG2_8019BS3: u8 = 0x08;
/// `RTL3_CONFIG2_8019BS2`.
pub const RTL3_CONFIG2_8019BS2: u8 = 0x04;
/// `RTL3_CONFIG2_8019BS1`.
pub const RTL3_CONFIG2_8019BS1: u8 = 0x02;
/// `RTL3_CONFIG2_8019BS0`.
pub const RTL3_CONFIG2_8019BS0: u8 = 0x01;
/// `NERTL_RTL3_CONFIG3`: Configuration 3.
pub const NERTL_RTL3_CONFIG3: usize = 0x06;
/// `RTL3_CONFIG3_8019PNP`: PnP Mode.
pub const RTL3_CONFIG3_8019PNP: u8 = 0x80;
/// `RTL3_CONFIG3_FUDUP`: Full Duplex.
pub const RTL3_CONFIG3_FUDUP: u8 = 0x40;
/// `RTL3_CONFIG3_LEDS1`: LED1/2 pin configuration.
pub const RTL3_CONFIG3_LEDS1: u8 = 0x20;
/// `RTL3_CONFIG3_LEDS0`: LED0 pin configuration.
pub const RTL3_CONFIG3_LEDS0: u8 = 0x10;
/// `RTL3_CONFIG3_SLEEP`: Sleep mode.
pub const RTL3_CONFIG3_SLEEP: u8 = 0x04;
/// `RTL3_CONFIG3_PWRDN`: Power Down.
pub const RTL3_CONFIG3_PWRDN: u8 = 0x02;
/// `RTL3_CONFIG3_8019ACTIVEB`: inverse of bit 0 in PnP Act Reg.
pub const RTL3_CONFIG3_8019ACTIVEB: u8 = 0x01;
/// `NERTL_RTL3_CSNSAV`: CSN Save Register (8019).
pub const NERTL_RTL3_CSNSAV: usize = 0x08;
/// `NERTL_RTL3_HLTCLK`: Halt Clock.
pub const NERTL_RTL3_HLTCLK: usize = 0x09;
/// `RTL3_HLTCLK_RUNNING`: clock runs in power down.
pub const RTL3_HLTCLK_RUNNING: u8 = b'R';
/// `RTL3_HLTCLK_HALTED`: clock halted in power down.
pub const RTL3_HLTCLK_HALTED: u8 = b'H';
/// `NERTL_RTL3_INTR`: ISA bus states of INT7-0 (8019).
pub const NERTL_RTL3_INTR: usize = 0x0b;
/// `NERTL_RTL3_8029ID0`: ID register 0.
pub const NERTL_RTL3_8029ID0: usize = 0x0e;
/// `NERTL_RTL3_8029ID1`: ID register 1.
pub const NERTL_RTL3_8029ID1: usize = 0x0f;
/* </CODE> */
