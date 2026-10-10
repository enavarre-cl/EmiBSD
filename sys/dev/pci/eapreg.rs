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

/*
 * Copyright (c) 1998, 1999 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Lennart Augustsson <augustss@netbsd.org> and Charles M. Hannum.
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
//! `<dev/pci/eapreg.h>`: the ES1370/ES1371/ES1373 registers, the ES1371 sample rate
//! converter's, the ES1370's AK4531 codec's, and eap(4)'s mixer controls.
//!
//! Upstream: sys/dev/pci/eapreg.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - Register offsets are `usize` (bus_space(9) offsets); the 32-bit registers' bits
//!   `u32`, the UART's 8-bit status and control bits `u8`; the AK4531 register numbers and
//!   values, the sample rate converter's addresses and data and the mixer indices `i32`,
//!   as eap.c passes them as `int`s.
//! - The function-like macros are `const fn`s with lower-case names (`EAP_SET_CODEC` is
//!   [`eap_set_codec`], ...).
//! - `EAP_JYSTK_EN` is defined twice in the C, with the same value; once here.
//! - `SRC_MAGIC` keeps the C's `(1<15)`, a comparison worth 1 (unused, as in the C).

/// `EAP_ICSC`: interrupt / chip select control.
pub const EAP_ICSC: usize = 0x00;
/// `EAP_SERR_DISABLE`.
pub const EAP_SERR_DISABLE: u32 = 0x0000_0001;
/// `EAP_CDC_EN`.
pub const EAP_CDC_EN: u32 = 0x0000_0002;
/// `EAP_JYSTK_EN`.
pub const EAP_JYSTK_EN: u32 = 0x0000_0004;
/// `EAP_UART_EN`.
pub const EAP_UART_EN: u32 = 0x0000_0008;
/// `EAP_ADC_EN`.
pub const EAP_ADC_EN: u32 = 0x0000_0010;
/// `EAP_DAC2_EN`.
pub const EAP_DAC2_EN: u32 = 0x0000_0020;
/// `EAP_DAC1_EN`.
pub const EAP_DAC1_EN: u32 = 0x0000_0040;
/// `EAP_BREQ`.
pub const EAP_BREQ: u32 = 0x0000_0080;
/// `EAP_XTCL0`.
pub const EAP_XTCL0: u32 = 0x0000_0100;
/// `EAP_M_CB`.
pub const EAP_M_CB: u32 = 0x0000_0200;
/// `EAP_CCB_INTRM`.
pub const EAP_CCB_INTRM: u32 = 0x0000_0400;
/// `EAP_DAC_SYNC`.
pub const EAP_DAC_SYNC: u32 = 0x0000_0800;
/// `EAP_WTSRSEL`.
pub const EAP_WTSRSEL: u32 = 0x0000_3000;
/// `EAP_WTSRSEL_5`.
pub const EAP_WTSRSEL_5: u32 = 0x0000_0000;
/// `EAP_WTSRSEL_11`.
pub const EAP_WTSRSEL_11: u32 = 0x0000_1000;
/// `EAP_WTSRSEL_22`.
pub const EAP_WTSRSEL_22: u32 = 0x0000_2000;
/// `EAP_WTSRSEL_44`.
pub const EAP_WTSRSEL_44: u32 = 0x0000_3000;
/// `EAP_M_SBB`.
pub const EAP_M_SBB: u32 = 0x0000_4000;
/// `E1371_SYNC_RES`.
pub const E1371_SYNC_RES: u32 = 0x0000_4000;
/// `EAP_MSFMTSEL`.
pub const EAP_MSFMTSEL: u32 = 0x0000_8000;
/// `EAP_PCLKBITS`.
pub const EAP_PCLKBITS: u32 = 0x1fff_0000;
/// `EAP_XTCL1`.
pub const EAP_XTCL1: u32 = 0x4000_0000;
/// `EAP_ADC_STOP`.
pub const EAP_ADC_STOP: u32 = 0x8000_0000;
/// `EAP_ICSS`: interrupt / chip select status; on the 5880 control / status.
pub const EAP_ICSS: usize = 0x04;
/// `EAP_I_ADC`.
pub const EAP_I_ADC: u32 = 0x0000_0001;
/// `EAP_I_DAC2`.
pub const EAP_I_DAC2: u32 = 0x0000_0002;
/// `EAP_I_DAC1`.
pub const EAP_I_DAC1: u32 = 0x0000_0004;
/// `EAP_I_UART`.
pub const EAP_I_UART: u32 = 0x0000_0008;
/// `EAP_I_MCCB`.
pub const EAP_I_MCCB: u32 = 0x0000_0010;
/// `EAP_VC`.
pub const EAP_VC: u32 = 0x0000_0060;
/// `EAP_CWRIP`.
pub const EAP_CWRIP: u32 = 0x0000_0100;
/// `EAP_CBUSY`.
pub const EAP_CBUSY: u32 = 0x0000_0200;
/// `EAP_CSTAT`.
pub const EAP_CSTAT: u32 = 0x0000_0400;
/// `EAP_CT5880_AC97_RESET`.
pub const EAP_CT5880_AC97_RESET: u32 = 0x2000_0000;
/// `EAP_INTR`.
pub const EAP_INTR: u32 = 0x8000_0000;
/// `EAP_UART_DATA`.
pub const EAP_UART_DATA: usize = 0x08;
/// `EAP_UART_STATUS`.
pub const EAP_UART_STATUS: usize = 0x09;
/// `EAP_US_RXRDY`.
pub const EAP_US_RXRDY: u8 = 0x01;
/// `EAP_US_TXRDY`.
pub const EAP_US_TXRDY: u8 = 0x02;
/// `EAP_US_TXINT`.
pub const EAP_US_TXINT: u8 = 0x04;
/// `EAP_US_RXINT`.
pub const EAP_US_RXINT: u8 = 0x80;
/// `EAP_UART_CONTROL`.
pub const EAP_UART_CONTROL: usize = 0x09;
/// `EAP_UC_CNTRL`.
pub const EAP_UC_CNTRL: u8 = 0x03;
/// `EAP_UC_TXINTEN`.
pub const EAP_UC_TXINTEN: u8 = 0x20;
/// `EAP_UC_RXINTEN`.
pub const EAP_UC_RXINTEN: u8 = 0x80;
/// `EAP_MEMPAGE`.
pub const EAP_MEMPAGE: usize = 0x0c;
/// `EAP_CODEC`.
pub const EAP_CODEC: usize = 0x10;
/// `E1371_CODEC`.
pub const E1371_CODEC: usize = 0x14;
/// `E1371_CODEC_VALID`.
pub const E1371_CODEC_VALID: u32 = 0x8000_0000;
/// `E1371_CODEC_WIP`.
pub const E1371_CODEC_WIP: u32 = 0x4000_0000;
/// `E1371_CODEC_READ`.
pub const E1371_CODEC_READ: u32 = 0x0080_0000;
/// `E1371_SRC`.
pub const E1371_SRC: usize = 0x10;
/// `E1371_SRC_RAMWE`.
pub const E1371_SRC_RAMWE: u32 = 0x0100_0000;
/// `E1371_SRC_RBUSY`.
pub const E1371_SRC_RBUSY: u32 = 0x0080_0000;
/// `E1371_SRC_DISABLE`.
pub const E1371_SRC_DISABLE: u32 = 0x0040_0000;
/// `E1371_SRC_DISP1`.
pub const E1371_SRC_DISP1: u32 = 0x0020_0000;
/// `E1371_SRC_DISP2`.
pub const E1371_SRC_DISP2: u32 = 0x0010_0000;
/// `E1371_SRC_DISREC`.
pub const E1371_SRC_DISREC: u32 = 0x0008_0000;
/// `E1371_SRC_DATAMASK`.
pub const E1371_SRC_DATAMASK: u32 = 0x0000_ffff;
/// `E1371_SRC_CTLMASK`.
pub const E1371_SRC_CTLMASK: u32 =
    E1371_SRC_DISABLE | E1371_SRC_DISP1 | E1371_SRC_DISP2 | E1371_SRC_DISREC;
/// `E1371_SRC_STATE_MASK`.
pub const E1371_SRC_STATE_MASK: u32 = 0x0087_0000;
/// `E1371_SRC_STATE_OK`.
pub const E1371_SRC_STATE_OK: u32 = 0x0001_0000;
/// `E1371_LEGACY`.
pub const E1371_LEGACY: usize = 0x18;
/// `ESRC_ADC`.
pub const ESRC_ADC: i32 = 0x78;
/// `ESRC_DAC1`.
pub const ESRC_DAC1: i32 = 0x74;
/// `ESRC_DAC2`.
pub const ESRC_DAC2: i32 = 0x70;
/// `ESRC_ADC_VOLL`.
pub const ESRC_ADC_VOLL: i32 = 0x6c;
/// `ESRC_ADC_VOLR`.
pub const ESRC_ADC_VOLR: i32 = 0x6d;
/// `ESRC_DAC1_VOLL`.
pub const ESRC_DAC1_VOLL: i32 = 0x7c;
/// `ESRC_DAC1_VOLR`.
pub const ESRC_DAC1_VOLR: i32 = 0x7d;
/// `ESRC_DAC2_VOLL`.
pub const ESRC_DAC2_VOLL: i32 = 0x7e;
/// `ESRC_DAC2_VOLR`.
pub const ESRC_DAC2_VOLR: i32 = 0x7f;
/// `ESRC_TRUNC_N`.
pub const ESRC_TRUNC_N: i32 = 0x00;
/// `ESRC_IREGS`.
pub const ESRC_IREGS: i32 = 0x01;
/// `ESRC_ACF`.
pub const ESRC_ACF: i32 = 0x02;
/// `ESRC_VFF`.
pub const ESRC_VFF: i32 = 0x03;
/// `ESRC_SMF`.
pub const ESRC_SMF: i32 = 0x8000;
/// `SRC_MAGIC`.
pub const SRC_MAGIC: u32 = (1 < 15) as u32 | (1 << 13) | (1 << 11) | (1 << 9);
/// `EAP_SIC`.
pub const EAP_SIC: usize = 0x20;
/// `EAP_P1_S_MB`.
pub const EAP_P1_S_MB: u32 = 0x0000_0001;
/// `EAP_P1_S_EB`.
pub const EAP_P1_S_EB: u32 = 0x0000_0002;
/// `EAP_P2_S_MB`.
pub const EAP_P2_S_MB: u32 = 0x0000_0004;
/// `EAP_P2_S_EB`.
pub const EAP_P2_S_EB: u32 = 0x0000_0008;
/// `EAP_R1_S_MB`.
pub const EAP_R1_S_MB: u32 = 0x0000_0010;
/// `EAP_R1_S_EB`.
pub const EAP_R1_S_EB: u32 = 0x0000_0020;
/// `EAP_P2_DAC_SEN`.
pub const EAP_P2_DAC_SEN: u32 = 0x0000_0040;
/// `EAP_P1_SCT_RLD`.
pub const EAP_P1_SCT_RLD: u32 = 0x0000_0080;
/// `EAP_P1_INTR_EN`.
pub const EAP_P1_INTR_EN: u32 = 0x0000_0100;
/// `EAP_P2_INTR_EN`.
pub const EAP_P2_INTR_EN: u32 = 0x0000_0200;
/// `EAP_R1_INTR_EN`.
pub const EAP_R1_INTR_EN: u32 = 0x0000_0400;
/// `EAP_P1_PAUSE`.
pub const EAP_P1_PAUSE: u32 = 0x0000_0800;
/// `EAP_P2_PAUSE`.
pub const EAP_P2_PAUSE: u32 = 0x0000_1000;
/// `EAP_P1_LOOP_SEL`.
pub const EAP_P1_LOOP_SEL: u32 = 0x0000_2000;
/// `EAP_P2_LOOP_SEL`.
pub const EAP_P2_LOOP_SEL: u32 = 0x0000_4000;
/// `EAP_R1_LOOP_SEL`.
pub const EAP_R1_LOOP_SEL: u32 = 0x0000_8000;
/// `EAP_INC_BITS`.
pub const EAP_INC_BITS: u32 = 0x003f_0000;
/// `EAP_DAC1_CSR`.
pub const EAP_DAC1_CSR: usize = 0x24;
/// `EAP_DAC2_CSR`.
pub const EAP_DAC2_CSR: usize = 0x28;
/// `EAP_ADC_CSR`.
pub const EAP_ADC_CSR: usize = 0x2c;
/// `EAP_DAC_PAGE`.
pub const EAP_DAC_PAGE: u32 = 0xc;
/// `EAP_ADC_PAGE`.
pub const EAP_ADC_PAGE: u32 = 0xd;
/// `EAP_UART_PAGE1`.
pub const EAP_UART_PAGE1: u32 = 0xe;
/// `EAP_UART_PAGE2`.
pub const EAP_UART_PAGE2: u32 = 0xf;
/// `EAP_DAC1_ADDR`.
pub const EAP_DAC1_ADDR: usize = 0x30;
/// `EAP_DAC1_SIZE`.
pub const EAP_DAC1_SIZE: usize = 0x34;
/// `EAP_DAC2_ADDR`.
pub const EAP_DAC2_ADDR: usize = 0x38;
/// `EAP_DAC2_SIZE`.
pub const EAP_DAC2_SIZE: usize = 0x3c;
/// `EAP_ADC_ADDR`.
pub const EAP_ADC_ADDR: usize = 0x30;
/// `EAP_ADC_SIZE`.
pub const EAP_ADC_SIZE: usize = 0x34;
/// `EAP_READ_TIMEOUT`.
pub const EAP_READ_TIMEOUT: i32 = 5000;
/// `EAP_WRITE_TIMEOUT`.
pub const EAP_WRITE_TIMEOUT: i32 = 5000;
/// `EAP_XTAL_FREQ`: 22.5792 / 16 MHz.
pub const EAP_XTAL_FREQ: u32 = 1411200;
// AK4531 registers
/// `AK_MASTER_L`.
pub const AK_MASTER_L: i32 = 0x00;
/// `AK_MASTER_R`.
pub const AK_MASTER_R: i32 = 0x01;
/// `AK_VOICE_L`.
pub const AK_VOICE_L: i32 = 0x02;
/// `AK_VOICE_R`.
pub const AK_VOICE_R: i32 = 0x03;
/// `AK_FM_L`.
pub const AK_FM_L: i32 = 0x04;
/// `AK_FM_R`.
pub const AK_FM_R: i32 = 0x05;
/// `AK_CD_L`.
pub const AK_CD_L: i32 = 0x06;
/// `AK_CD_R`.
pub const AK_CD_R: i32 = 0x07;
/// `AK_LINE_L`.
pub const AK_LINE_L: i32 = 0x08;
/// `AK_LINE_R`.
pub const AK_LINE_R: i32 = 0x09;
/// `AK_AUX_L`.
pub const AK_AUX_L: i32 = 0x0a;
/// `AK_AUX_R`.
pub const AK_AUX_R: i32 = 0x0b;
/// `AK_MONO1`.
pub const AK_MONO1: i32 = 0x0c;
/// `AK_MONO2`.
pub const AK_MONO2: i32 = 0x0d;
/// `AK_MIC`.
pub const AK_MIC: i32 = 0x0e;
/// `AK_MONO`.
pub const AK_MONO: i32 = 0x0f;
/// `AK_OUT_MIXER1`.
pub const AK_OUT_MIXER1: i32 = 0x10;
/// `AK_M_FM_L`.
pub const AK_M_FM_L: i32 = 0x40;
/// `AK_M_FM_R`.
pub const AK_M_FM_R: i32 = 0x20;
/// `AK_M_LINE_L`.
pub const AK_M_LINE_L: i32 = 0x10;
/// `AK_M_LINE_R`.
pub const AK_M_LINE_R: i32 = 0x08;
/// `AK_M_CD_L`.
pub const AK_M_CD_L: i32 = 0x04;
/// `AK_M_CD_R`.
pub const AK_M_CD_R: i32 = 0x02;
/// `AK_M_MIC`.
pub const AK_M_MIC: i32 = 0x01;
/// `AK_OUT_MIXER2`.
pub const AK_OUT_MIXER2: i32 = 0x11;
/// `AK_M_AUX_L`.
pub const AK_M_AUX_L: i32 = 0x20;
/// `AK_M_AUX_R`.
pub const AK_M_AUX_R: i32 = 0x10;
/// `AK_M_VOICE_L`.
pub const AK_M_VOICE_L: i32 = 0x08;
/// `AK_M_VOICE_R`.
pub const AK_M_VOICE_R: i32 = 0x04;
/// `AK_M_MONO2`.
pub const AK_M_MONO2: i32 = 0x02;
/// `AK_M_MONO1`.
pub const AK_M_MONO1: i32 = 0x01;
/// `AK_IN_MIXER1_L`.
pub const AK_IN_MIXER1_L: i32 = 0x12;
/// `AK_IN_MIXER1_R`.
pub const AK_IN_MIXER1_R: i32 = 0x13;
/// `AK_IN_MIXER2_L`.
pub const AK_IN_MIXER2_L: i32 = 0x14;
/// `AK_IN_MIXER2_R`.
pub const AK_IN_MIXER2_R: i32 = 0x15;
/// `AK_M_TMIC`.
pub const AK_M_TMIC: i32 = 0x80;
/// `AK_M_TMONO1`.
pub const AK_M_TMONO1: i32 = 0x40;
/// `AK_M_TMONO2`.
pub const AK_M_TMONO2: i32 = 0x20;
/// `AK_M2_AUX_L`.
pub const AK_M2_AUX_L: i32 = 0x10;
/// `AK_M2_AUX_R`.
pub const AK_M2_AUX_R: i32 = 0x08;
/// `AK_M_VOICE`.
pub const AK_M_VOICE: i32 = 0x04;
/// `AK_M2_MONO2`.
pub const AK_M2_MONO2: i32 = 0x02;
/// `AK_M2_MONO1`.
pub const AK_M2_MONO1: i32 = 0x01;
/// `AK_RESET`.
pub const AK_RESET: i32 = 0x16;
/// `AK_PD`.
pub const AK_PD: i32 = 0x02;
/// `AK_NRST`.
pub const AK_NRST: i32 = 0x01;
/// `AK_CS`.
pub const AK_CS: i32 = 0x17;
/// `AK_ADSEL`.
pub const AK_ADSEL: i32 = 0x18;
/// `AK_MGAIN`.
pub const AK_MGAIN: i32 = 0x19;
/// `AK_NPORTS`.
pub const AK_NPORTS: i32 = 0x20;
/// `VOL_0DB`.
pub const VOL_0DB: i32 = 200;
/// `EAP_MASTER_VOL`: the mixer controls ("futzable parms").
pub const EAP_MASTER_VOL: i32 = 0;
/// `EAP_VOICE_VOL`.
pub const EAP_VOICE_VOL: i32 = 1;
/// `EAP_FM_VOL`.
pub const EAP_FM_VOL: i32 = 2;
/// `EAP_VIDEO_VOL`: ES1371.
pub const EAP_VIDEO_VOL: i32 = 2;
/// `EAP_CD_VOL`.
pub const EAP_CD_VOL: i32 = 3;
/// `EAP_LINE_VOL`.
pub const EAP_LINE_VOL: i32 = 4;
/// `EAP_AUX_VOL`.
pub const EAP_AUX_VOL: i32 = 5;
/// `EAP_MIC_VOL`.
pub const EAP_MIC_VOL: i32 = 6;
/// `EAP_RECORD_SOURCE`.
pub const EAP_RECORD_SOURCE: i32 = 7;
/// `EAP_INPUT_SOURCE`.
pub const EAP_INPUT_SOURCE: i32 = 8;
/// `EAP_MIC_PREAMP`.
pub const EAP_MIC_PREAMP: i32 = 9;
/// `EAP_OUTPUT_CLASS`.
pub const EAP_OUTPUT_CLASS: i32 = 10;
/// `EAP_RECORD_CLASS`.
pub const EAP_RECORD_CLASS: i32 = 11;
/// `EAP_INPUT_CLASS`.
pub const EAP_INPUT_CLASS: i32 = 12;
/// `EAP_EV1938_A`.
pub const EAP_EV1938_A: u32 = 0x00;
/// `EAP_ES1371_A`.
pub const EAP_ES1371_A: u32 = 0x02;
/// `EAP_CT5880_C`.
pub const EAP_CT5880_C: u32 = 0x02;
/// `EAP_CT5880_D`.
pub const EAP_CT5880_D: u32 = 0x03;
/// `EAP_ES1373_A`.
pub const EAP_ES1373_A: u32 = 0x04;
/// `EAP_ES1373_B`.
pub const EAP_ES1373_B: u32 = 0x06;
/// `EAP_CT5880_A`.
pub const EAP_CT5880_A: u32 = 0x07;
/// `EAP_ES1373_8`.
pub const EAP_ES1373_8: u32 = 0x08;
/// `EAP_ES1371_B`.
pub const EAP_ES1371_B: u32 = 0x09;

/// `EAP_SET_PCLKDIV(n)`.
pub const fn eap_set_pclkdiv(n: u32) -> u32 {
    (n & 0x1fff) << 16
}

/// `EAP_GET_PCLKDIV(n)`.
pub const fn eap_get_pclkdiv(n: u32) -> u32 {
    (n >> 16) & 0x1fff
}

/// `EAP_SET_CODEC(a,d)`: an AK4531 register write.
pub const fn eap_set_codec(a: i32, d: i32) -> u32 {
    ((a << 8) | d) as u32
}

/// `E1371_SET_CODEC(a,d)`: an AC97 codec access.
pub const fn e1371_set_codec(a: u8, d: u16) -> u32 {
    ((a as u32) << 16) | d as u32
}

/// `E1371_SRC_ADDR(a)`.
pub const fn e1371_src_addr(a: i32) -> u32 {
    (a as u32) << 25
}

/// `E1371_SRC_DATA(d)`.
pub const fn e1371_src_data(d: i32) -> u32 {
    d as u32
}

/// `ESRC_SET_TRUNC(n)`.
pub const fn esrc_set_trunc(n: i32) -> i32 {
    n << 9
}

/// `ESRC_SET_N(n)`.
pub const fn esrc_set_n(n: i32) -> i32 {
    n << 4
}

/// `ESRC_SET_VFI(n)`.
pub const fn esrc_set_vfi(n: i32) -> i32 {
    n << 10
}

/// `ESRC_SET_ACI(n)`.
pub const fn esrc_set_aci(n: i32) -> i32 {
    n
}

/// `ESRC_SET_ADC_VOL(n)`.
pub const fn esrc_set_adc_vol(n: i32) -> i32 {
    n << 8
}

/// `ESRC_SET_DAC_VOLI(n)`.
pub const fn esrc_set_dac_voli(n: i32) -> i32 {
    n << 12
}

/// `ESRC_SET_DAC_VOLF(n)`.
pub const fn esrc_set_dac_volf(n: i32) -> i32 {
    n
}

/// `EAP_SET_P2_ST_INC(i)`.
pub const fn eap_set_p2_st_inc(i: u32) -> u32 {
    i << 16
}

/// `EAP_SET_P2_END_INC(i)`.
pub const fn eap_set_p2_end_inc(i: u32) -> u32 {
    i << 19
}

/// `EAP_GET_CURRSAMP(r)`.
pub const fn eap_get_currsamp(r: u32) -> u32 {
    r >> 16
}

/// `EAP_SET_SIZE(c,s)`.
pub const fn eap_set_size(c: u32, s: u32) -> u32 {
    (c << 16) | s
}

/// `VOL_TO_ATT5(v)`: a 0..255 level to the AK4531's 5-bit attenuation.
pub const fn vol_to_att5(v: i32) -> i32 {
    0x1f - (v >> 3)
}

/// `VOL_TO_GAIN5(v)`.
pub const fn vol_to_gain5(v: i32) -> i32 {
    vol_to_att5(v)
}

/// `ATT5_TO_VOL(v)`.
pub const fn att5_to_vol(v: i32) -> i32 {
    (0x1f - v) << 3
}

/// `GAIN5_TO_VOL(v)`.
pub const fn gain5_to_vol(v: i32) -> i32 {
    att5_to_vol(v)
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macros() {
        assert_eq!(eap_set_pclkdiv(EAP_XTAL_FREQ / 48000 - 2), 27 << 16);
        assert_eq!(eap_get_pclkdiv(eap_set_pclkdiv(0x2fff)), 0xfff);
        assert_eq!(eap_set_codec(AK_RESET, AK_PD | AK_NRST), 0x1603);
        assert_eq!(e1371_set_codec(0x02, 0x8000), 0x0002_8000);
        assert_eq!(e1371_src_addr(0x7f), 0xfe00_0000);
        assert_eq!(esrc_set_n(16), 0x100);
        assert_eq!(esrc_set_vfi(16), 0x4000);
        assert_eq!(esrc_set_dac_voli(1), 0x1000);
        assert_eq!(eap_set_size(0, 0x3fff), 0x3fff);
        assert_eq!(eap_set_p2_end_inc(2), 0x0010_0000);
        // the mixer levels round to the AK4531's 32 steps: 200 reads back as 200.
        assert_eq!(vol_to_att5(VOL_0DB), 6);
        assert_eq!(att5_to_vol(vol_to_att5(VOL_0DB)), VOL_0DB);
        assert_eq!(gain5_to_vol(vol_to_gain5(255)), 248);
        assert_eq!(E1371_SRC_CTLMASK, 0x0078_0000);
        assert_eq!(SRC_MAGIC, 0x2a01);
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/pci/eapreg.h");
        let mut ours = crate::reftest::assert_defines!(defs;
            EAP_ICSC, EAP_SERR_DISABLE, EAP_CDC_EN, EAP_JYSTK_EN, EAP_UART_EN, EAP_ADC_EN,
            EAP_DAC2_EN, EAP_DAC1_EN, EAP_BREQ, EAP_XTCL0, EAP_M_CB, EAP_CCB_INTRM,
            EAP_DAC_SYNC, EAP_WTSRSEL, EAP_WTSRSEL_5, EAP_WTSRSEL_11, EAP_WTSRSEL_22,
            EAP_WTSRSEL_44, EAP_M_SBB, E1371_SYNC_RES, EAP_MSFMTSEL, EAP_PCLKBITS, EAP_XTCL1,
            EAP_ADC_STOP, EAP_ICSS, EAP_I_ADC, EAP_I_DAC2, EAP_I_DAC1, EAP_I_UART, EAP_I_MCCB,
            EAP_VC, EAP_CWRIP, EAP_CBUSY, EAP_CSTAT, EAP_CT5880_AC97_RESET, EAP_INTR,
            EAP_UART_DATA, EAP_UART_STATUS, EAP_US_RXRDY, EAP_US_TXRDY, EAP_US_TXINT,
            EAP_US_RXINT, EAP_UART_CONTROL, EAP_UC_CNTRL, EAP_UC_TXINTEN, EAP_UC_RXINTEN,
            EAP_MEMPAGE, EAP_CODEC, E1371_CODEC, E1371_CODEC_VALID, E1371_CODEC_WIP,
            E1371_CODEC_READ, E1371_SRC, E1371_SRC_RAMWE, E1371_SRC_RBUSY, E1371_SRC_DISABLE,
            E1371_SRC_DISP1, E1371_SRC_DISP2, E1371_SRC_DISREC, E1371_SRC_DATAMASK,
            E1371_SRC_STATE_MASK, E1371_SRC_STATE_OK, E1371_LEGACY,
            ESRC_ADC, ESRC_DAC1, ESRC_DAC2, ESRC_ADC_VOLL, ESRC_ADC_VOLR, ESRC_DAC1_VOLL,
            ESRC_DAC1_VOLR, ESRC_DAC2_VOLL, ESRC_DAC2_VOLR, ESRC_TRUNC_N, ESRC_IREGS, ESRC_ACF,
            ESRC_VFF, ESRC_SMF, EAP_SIC, EAP_P1_S_MB, EAP_P1_S_EB, EAP_P2_S_MB,
            EAP_P2_S_EB, EAP_R1_S_MB, EAP_R1_S_EB, EAP_P2_DAC_SEN, EAP_P1_SCT_RLD,
            EAP_P1_INTR_EN, EAP_P2_INTR_EN, EAP_R1_INTR_EN, EAP_P1_PAUSE, EAP_P2_PAUSE,
            EAP_P1_LOOP_SEL, EAP_P2_LOOP_SEL, EAP_R1_LOOP_SEL, EAP_INC_BITS, EAP_DAC1_CSR,
            EAP_DAC2_CSR, EAP_ADC_CSR, EAP_DAC_PAGE, EAP_ADC_PAGE, EAP_UART_PAGE1,
            EAP_UART_PAGE2, EAP_DAC1_ADDR, EAP_DAC1_SIZE, EAP_DAC2_ADDR, EAP_DAC2_SIZE,
            EAP_ADC_ADDR, EAP_ADC_SIZE, EAP_READ_TIMEOUT, EAP_WRITE_TIMEOUT, EAP_XTAL_FREQ,
            AK_MASTER_L, AK_MASTER_R, AK_VOICE_L, AK_VOICE_R, AK_FM_L, AK_FM_R, AK_CD_L,
            AK_CD_R, AK_LINE_L, AK_LINE_R, AK_AUX_L, AK_AUX_R, AK_MONO1, AK_MONO2, AK_MIC,
            AK_MONO, AK_OUT_MIXER1, AK_M_FM_L, AK_M_FM_R, AK_M_LINE_L, AK_M_LINE_R,
            AK_M_CD_L, AK_M_CD_R, AK_M_MIC, AK_OUT_MIXER2, AK_M_AUX_L, AK_M_AUX_R,
            AK_M_VOICE_L, AK_M_VOICE_R, AK_M_MONO2, AK_M_MONO1, AK_IN_MIXER1_L,
            AK_IN_MIXER1_R, AK_IN_MIXER2_L, AK_IN_MIXER2_R, AK_M_TMIC, AK_M_TMONO1,
            AK_M_TMONO2, AK_M2_AUX_L, AK_M2_AUX_R, AK_M_VOICE, AK_M2_MONO2, AK_M2_MONO1,
            AK_RESET, AK_PD, AK_NRST, AK_CS, AK_ADSEL, AK_MGAIN, AK_NPORTS, VOL_0DB,
            EAP_MASTER_VOL, EAP_VOICE_VOL, EAP_FM_VOL, EAP_VIDEO_VOL, EAP_CD_VOL,
            EAP_LINE_VOL, EAP_AUX_VOL, EAP_MIC_VOL, EAP_RECORD_SOURCE, EAP_INPUT_SOURCE,
            EAP_MIC_PREAMP, EAP_OUTPUT_CLASS, EAP_RECORD_CLASS, EAP_INPUT_CLASS,
            EAP_EV1938_A, EAP_ES1371_A, EAP_CT5880_C, EAP_CT5880_D, EAP_ES1373_A,
            EAP_ES1373_B, EAP_CT5880_A, EAP_ES1373_8, EAP_ES1371_B);
        // Its value spans two lines, which the parser does not join: `macros` checks it.
        ours.push("E1371_SRC_CTLMASK");
        for prefix in ["EAP_", "E1371_", "ESRC_", "AK_", "VOL_"] {
            crate::reftest::assert_complete(&defs, prefix, &ours);
        }
    }
}
/* </TESTS> */
