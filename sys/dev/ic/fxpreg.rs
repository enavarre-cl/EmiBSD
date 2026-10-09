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
 * Copyright (c) 1995, David Greenman
 * All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice unmodified, this list of conditions, and the following
 *    disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 *	$FreeBSD: if_fxpreg.h,v 1.13 1998/06/08 09:47:46 bde Exp $
 */
/* </LICENSES> */

/* <CODE> */
//! `<dev/ic/fxpreg.h>`: the registers, command blocks and receive frame area of the Intel
//! 8255x (EtherExpress PRO/100) Fast Ethernet controllers.
//!
//! Upstream: sys/dev/ic/fxpreg.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - The command blocks are `#[repr(C)]` structs of plain integers. The C marks every member
//!   `volatile`; here the structs sit in `bus_dma(9)` memory that `fxp.rs` reads and writes
//!   whole or by member through `read_volatile` and `write_volatile`, so the type carries no
//!   marker. `macaddr[6]` and `mc_addr[MAXMCADDR][6]` are byte arrays.
//! - `struct fxp_cb_nop`, whose first member is two pointers of padding, is not used by the
//!   driver and is left out.
//! - The bit-field remark in the C is a comment on the old definition of `FXP_CSR_SCB_RUSCUS`
//!   and is not ported.

/// `FXP_VENDORID_INTEL`.
pub const FXP_VENDORID_INTEL: u32 = 0x8086;
/// `FXP_DEVICEID_i82557`.
pub const FXP_DEVICEID_I82557: u32 = 0x1229;

/// `FXP_PCI_MMBA`.
pub const FXP_PCI_MMBA: i32 = 0x10;
/// `FXP_PCI_IOBA`.
pub const FXP_PCI_IOBA: i32 = 0x14;

/// `FXP_CSR_SCB_STATUS`: scb_status (2 byte).
pub const FXP_CSR_SCB_STATUS: usize = 0;
/// `FXP_CSR_SCB_COMMAND`: scb_command (2 byte).
pub const FXP_CSR_SCB_COMMAND: usize = 2;
/// `FXP_CSR_SCB_GENERAL`: scb_general (4 bytes).
pub const FXP_CSR_SCB_GENERAL: usize = 4;
/// `FXP_CSR_PORT`: port (4 bytes).
pub const FXP_CSR_PORT: usize = 8;
/// `FXP_CSR_FLASHCONTROL`: flash control (2 bytes).
pub const FXP_CSR_FLASHCONTROL: usize = 12;
/// `FXP_CSR_EEPROMCONTROL`: eeprom control (2 bytes).
pub const FXP_CSR_EEPROMCONTROL: usize = 14;
/// `FXP_CSR_MDICONTROL`: mdi control (4 bytes).
pub const FXP_CSR_MDICONTROL: usize = 16;

/// `FXP_PORT_SOFTWARE_RESET`.
pub const FXP_PORT_SOFTWARE_RESET: u32 = 0;
/// `FXP_PORT_SELFTEST`.
pub const FXP_PORT_SELFTEST: u32 = 1;
/// `FXP_PORT_SELECTIVE_RESET`.
pub const FXP_PORT_SELECTIVE_RESET: u32 = 2;
/// `FXP_PORT_DUMP`.
pub const FXP_PORT_DUMP: u32 = 3;

/// `FXP_SCB_RUS_IDLE`.
pub const FXP_SCB_RUS_IDLE: u16 = 0x0000;
/// `FXP_SCB_RUS_SUSPENDED`.
pub const FXP_SCB_RUS_SUSPENDED: u16 = 0x0001;
/// `FXP_SCB_RUS_NORESOURCES`.
pub const FXP_SCB_RUS_NORESOURCES: u16 = 0x0002;
/// `FXP_SCB_RUS_READY`.
pub const FXP_SCB_RUS_READY: u16 = 0x0004;
/// `FXP_SCB_RUS_SUSP_NORBDS`.
pub const FXP_SCB_RUS_SUSP_NORBDS: u16 = 0x0009;
/// `FXP_SCB_RUS_NORES_NORBDS`.
pub const FXP_SCB_RUS_NORES_NORBDS: u16 = 0x000a;
/// `FXP_SCB_RUS_READY_NORBDS`.
pub const FXP_SCB_RUS_READY_NORBDS: u16 = 0x000c;

/// `FXP_SCB_CUS_IDLE`.
pub const FXP_SCB_CUS_IDLE: u16 = 0x0000;
/// `FXP_SCB_CUS_SUSPENDED`.
pub const FXP_SCB_CUS_SUSPENDED: u16 = 0x0040;
/// `FXP_SCB_CUS_ACTIVE`.
pub const FXP_SCB_CUS_ACTIVE: u16 = 0x0080;
/// `FXP_SCB_CUS_MASK`.
pub const FXP_SCB_CUS_MASK: u16 = 0x00c0;

/// `FXP_SCB_STATACK_SWI`.
pub const FXP_SCB_STATACK_SWI: u16 = 0x0400;
/// `FXP_SCB_STATACK_MDI`.
pub const FXP_SCB_STATACK_MDI: u16 = 0x0800;
/// `FXP_SCB_STATACK_RNR`.
pub const FXP_SCB_STATACK_RNR: u16 = 0x1000;
/// `FXP_SCB_STATACK_CNA`.
pub const FXP_SCB_STATACK_CNA: u16 = 0x2000;
/// `FXP_SCB_STATACK_FR`.
pub const FXP_SCB_STATACK_FR: u16 = 0x4000;
/// `FXP_SCB_STATACK_CXTNO`.
pub const FXP_SCB_STATACK_CXTNO: u16 = 0x8000;
/// `FXP_SCB_STATACK_MASK`.
pub const FXP_SCB_STATACK_MASK: u16 = 0xfc00;

/// `FXP_SCB_COMMAND_CU_NOP`.
pub const FXP_SCB_COMMAND_CU_NOP: u16 = 0x0000;
/// `FXP_SCB_COMMAND_CU_START`.
pub const FXP_SCB_COMMAND_CU_START: u16 = 0x0010;
/// `FXP_SCB_COMMAND_CU_RESUME`.
pub const FXP_SCB_COMMAND_CU_RESUME: u16 = 0x0020;
/// `FXP_SCB_COMMAND_CU_DUMP_ADR`.
pub const FXP_SCB_COMMAND_CU_DUMP_ADR: u16 = 0x0040;
/// `FXP_SCB_COMMAND_CU_DUMP`.
pub const FXP_SCB_COMMAND_CU_DUMP: u16 = 0x0050;
/// `FXP_SCB_COMMAND_CU_BASE`.
pub const FXP_SCB_COMMAND_CU_BASE: u16 = 0x0060;
/// `FXP_SCB_COMMAND_CU_DUMPRESET`.
pub const FXP_SCB_COMMAND_CU_DUMPRESET: u16 = 0x0070;

/// `FXP_SCB_COMMAND_RU_NOP`.
pub const FXP_SCB_COMMAND_RU_NOP: u16 = 0x0000;
/// `FXP_SCB_COMMAND_RU_START`.
pub const FXP_SCB_COMMAND_RU_START: u16 = 0x0001;
/// `FXP_SCB_COMMAND_RU_RESUME`.
pub const FXP_SCB_COMMAND_RU_RESUME: u16 = 0x0002;
/// `FXP_SCB_COMMAND_RU_ABORT`.
pub const FXP_SCB_COMMAND_RU_ABORT: u16 = 0x0004;
/// `FXP_SCB_COMMAND_RU_LOADHDS`.
pub const FXP_SCB_COMMAND_RU_LOADHDS: u16 = 0x0005;
/// `FXP_SCB_COMMAND_RU_BASE`.
pub const FXP_SCB_COMMAND_RU_BASE: u16 = 0x0006;
/// `FXP_SCB_COMMAND_RU_RBDRESUME`.
pub const FXP_SCB_COMMAND_RU_RBDRESUME: u16 = 0x0007;

/// `FXP_SCB_INTRCNTL_REQUEST_SWI`.
pub const FXP_SCB_INTRCNTL_REQUEST_SWI: u16 = 0x0200;

/// `FXP_CMD_TMO`.
pub const FXP_CMD_TMO: i32 = 10000;

/// `struct fxp_cb_ias`: individual address setup command block.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct FxpCbIas {
    /// `cb_status`.
    pub cb_status: u16,
    /// `cb_command`.
    pub cb_command: u16,
    /// `link_addr`.
    pub link_addr: u32,
    /// `macaddr`.
    pub macaddr: [u8; 6],
}

/// `struct fxp_cb_config`: configure command block.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct FxpCbConfig {
    /// `cb_status`.
    pub cb_status: u16,
    /// `cb_command`.
    pub cb_command: u16,
    /// `link_addr`.
    pub link_addr: u32,
    /// `byte_count`.
    pub byte_count: u8,
    /// `fifo_limit`.
    pub fifo_limit: u8,
    /// `adaptive_ifs`.
    pub adaptive_ifs: u8,
    /// `ctrl0`.
    pub ctrl0: u8,
    /// `rx_dma_bytecount`.
    pub rx_dma_bytecount: u8,
    /// `tx_dma_bytecount`.
    pub tx_dma_bytecount: u8,
    /// `ctrl1`.
    pub ctrl1: u8,
    /// `ctrl2`.
    pub ctrl2: u8,
    /// `mediatype`.
    pub mediatype: u8,
    /// `void2`.
    pub void2: u8,
    /// `ctrl3`.
    pub ctrl3: u8,
    /// `linear_priority`.
    pub linear_priority: u8,
    /// `interfrm_spacing`.
    pub interfrm_spacing: u8,
    /// `void3`.
    pub void3: u8,
    /// `void4`.
    pub void4: u8,
    /// `promiscuous`.
    pub promiscuous: u8,
    /// `void5`.
    pub void5: u8,
    /// `void6`.
    pub void6: u8,
    /// `stripping`.
    pub stripping: u8,
    /// `fdx_pin`.
    pub fdx_pin: u8,
    /// `multi_ia`.
    pub multi_ia: u8,
    /// `mc_all`.
    pub mc_all: u8,
}

/// `MAXMCADDR`.
pub const MAXMCADDR: usize = 80;

/// `struct fxp_cb_mcs`: multicast setup command block.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FxpCbMcs {
    /// `cb_status`.
    pub cb_status: u16,
    /// `cb_command`.
    pub cb_command: u16,
    /// `link_addr`.
    pub link_addr: u32,
    /// `mc_cnt`.
    pub mc_cnt: u16,
    /// `mc_addr`.
    pub mc_addr: [[u8; 6]; MAXMCADDR],
}

/// `SZ_TXCB`: TX control block head size = 4 32 bit words.
pub const SZ_TXCB: usize = 16;
/// `SZ_TBD`: fragment ptr/size block size.
pub const SZ_TBD: usize = 8;
/// `FXP_NTXSEG`: the number of DMA segments in a TxCB, chosen to make the total struct size
/// an even power of two (no TxCB may be split across a page boundary).
pub const FXP_NTXSEG: usize = (256 - SZ_TXCB) / SZ_TBD;

/// `struct fxp_tbd`: transmit buffer descriptor.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct FxpTbd {
    /// `tb_addr`.
    pub tb_addr: u32,
    /// `tb_size`.
    pub tb_size: u32,
}

/// `struct fxp_cb_tx`: transmit command block.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FxpCbTx {
    /// `cb_status`.
    pub cb_status: u16,
    /// `cb_command`.
    pub cb_command: u16,
    /// `link_addr`.
    pub link_addr: u32,
    /// `tbd_array_addr`.
    pub tbd_array_addr: u32,
    /// `byte_count`.
    pub byte_count: u16,
    /// `tx_threshold`.
    pub tx_threshold: u8,
    /// `tbd_number`.
    pub tbd_number: u8,
    /// `tbd`: not actually part of the TxCB.
    pub tbd: [FxpTbd; FXP_NTXSEG],
}

/// `FXP_CB_STATUS_OK`.
pub const FXP_CB_STATUS_OK: u16 = 0x2000;
/// `FXP_CB_STATUS_C`.
pub const FXP_CB_STATUS_C: u16 = 0x8000;
/// `FXP_CB_COMMAND_NOP`.
pub const FXP_CB_COMMAND_NOP: u16 = 0x0;
/// `FXP_CB_COMMAND_IAS`.
pub const FXP_CB_COMMAND_IAS: u16 = 0x1;
/// `FXP_CB_COMMAND_CONFIG`.
pub const FXP_CB_COMMAND_CONFIG: u16 = 0x2;
/// `FXP_CB_COMMAND_MCAS`.
pub const FXP_CB_COMMAND_MCAS: u16 = 0x3;
/// `FXP_CB_COMMAND_XMIT`.
pub const FXP_CB_COMMAND_XMIT: u16 = 0x4;
/// `FXP_CB_COMMAND_UCODE`.
pub const FXP_CB_COMMAND_UCODE: u16 = 0x5;
/// `FXP_CB_COMMAND_DUMP`.
pub const FXP_CB_COMMAND_DUMP: u16 = 0x6;
/// `FXP_CB_COMMAND_DIAG`.
pub const FXP_CB_COMMAND_DIAG: u16 = 0x7;
/// `FXP_CB_COMMAND_SF`: simple/flexible mode.
pub const FXP_CB_COMMAND_SF: u16 = 0x0008;
/// `FXP_CB_COMMAND_I`: generate interrupt on completion.
pub const FXP_CB_COMMAND_I: u16 = 0x2000;
/// `FXP_CB_COMMAND_S`: suspend on completion.
pub const FXP_CB_COMMAND_S: u16 = 0x4000;
/// `FXP_CB_COMMAND_EL`: end of list.
pub const FXP_CB_COMMAND_EL: u16 = 0x8000;

/// `struct fxp_rfa`: receive frame area. The driver reads and writes it in place inside the
/// receive cluster, at a 2-byte aligned address, through the offsets below.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct FxpRfa {
    /// `rfa_status`.
    pub rfa_status: u16,
    /// `rfa_control`.
    pub rfa_control: u16,
    /// `link_addr`.
    pub link_addr: u32,
    /// `rbd_addr`.
    pub rbd_addr: u32,
    /// `actual_size`.
    pub actual_size: u16,
    /// `size`.
    pub size: u16,
}

/// `offsetof(struct fxp_rfa, rfa_status)`.
pub const FXP_RFA_OFF_RFA_STATUS: usize = 0;
/// `offsetof(struct fxp_rfa, rfa_control)`.
pub const FXP_RFA_OFF_RFA_CONTROL: usize = 2;
/// `offsetof(struct fxp_rfa, link_addr)`.
pub const FXP_RFA_OFF_LINK_ADDR: usize = 4;
/// `offsetof(struct fxp_rfa, rbd_addr)`.
pub const FXP_RFA_OFF_RBD_ADDR: usize = 8;
/// `offsetof(struct fxp_rfa, actual_size)`.
pub const FXP_RFA_OFF_ACTUAL_SIZE: usize = 12;
/// `offsetof(struct fxp_rfa, size)`.
pub const FXP_RFA_OFF_SIZE: usize = 14;

/// `FXP_RFA_STATUS_RCOL`: receive collision.
pub const FXP_RFA_STATUS_RCOL: u16 = 0x0001;
/// `FXP_RFA_STATUS_IAMATCH`: 0 = matches station address.
pub const FXP_RFA_STATUS_IAMATCH: u16 = 0x0002;
/// `FXP_RFA_STATUS_S4`: receive error from PHY.
pub const FXP_RFA_STATUS_S4: u16 = 0x0010;
/// `FXP_RFA_STATUS_TL`: type/length.
pub const FXP_RFA_STATUS_TL: u16 = 0x0020;
/// `FXP_RFA_STATUS_FTS`: frame too short.
pub const FXP_RFA_STATUS_FTS: u16 = 0x0080;
/// `FXP_RFA_STATUS_OVERRUN`: DMA overrun.
pub const FXP_RFA_STATUS_OVERRUN: u16 = 0x0100;
/// `FXP_RFA_STATUS_RNR`: RU not ready.
pub const FXP_RFA_STATUS_RNR: u16 = 0x0200;
/// `FXP_RFA_STATUS_ALIGN`: alignment error.
pub const FXP_RFA_STATUS_ALIGN: u16 = 0x0400;
/// `FXP_RFA_STATUS_CRC`: CRC error.
pub const FXP_RFA_STATUS_CRC: u16 = 0x0800;
/// `FXP_RFA_STATUS_OK`: packet received okay.
pub const FXP_RFA_STATUS_OK: u16 = 0x2000;
/// `FXP_RFA_STATUS_C`: packet reception complete.
pub const FXP_RFA_STATUS_C: u16 = 0x8000;
/// `FXP_RFA_CONTROL_SF`: simple/flexible memory mode.
pub const FXP_RFA_CONTROL_SF: u16 = 0x08;
/// `FXP_RFA_CONTROL_H`: header RFD.
pub const FXP_RFA_CONTROL_H: u16 = 0x10;
/// `FXP_RFA_CONTROL_S`: suspend after reception.
pub const FXP_RFA_CONTROL_S: u16 = 0x4000;
/// `FXP_RFA_CONTROL_EL`: end of list.
pub const FXP_RFA_CONTROL_EL: u16 = 0x8000;

/// `struct fxp_stats`: statistics dump area.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct FxpStats {
    /// `tx_good`.
    pub tx_good: u32,
    /// `tx_maxcols`.
    pub tx_maxcols: u32,
    /// `tx_latecols`.
    pub tx_latecols: u32,
    /// `tx_underruns`.
    pub tx_underruns: u32,
    /// `tx_lostcrs`.
    pub tx_lostcrs: u32,
    /// `tx_deffered`.
    pub tx_deffered: u32,
    /// `tx_single_collisions`.
    pub tx_single_collisions: u32,
    /// `tx_multiple_collisions`.
    pub tx_multiple_collisions: u32,
    /// `tx_total_collisions`.
    pub tx_total_collisions: u32,
    /// `rx_good`.
    pub rx_good: u32,
    /// `rx_crc_errors`.
    pub rx_crc_errors: u32,
    /// `rx_alignment_errors`.
    pub rx_alignment_errors: u32,
    /// `rx_rnr_errors`.
    pub rx_rnr_errors: u32,
    /// `rx_overrun_errors`.
    pub rx_overrun_errors: u32,
    /// `rx_cdt_errors`.
    pub rx_cdt_errors: u32,
    /// `rx_shortframes`.
    pub rx_shortframes: u32,
    /// `completion_status`.
    pub completion_status: u32,
}

/// `FXP_STATS_DUMP_COMPLETE`.
pub const FXP_STATS_DUMP_COMPLETE: u32 = 0xa005;
/// `FXP_STATS_DR_COMPLETE`.
pub const FXP_STATS_DR_COMPLETE: u32 = 0xa007;

/// `FXP_EEPROM_EESK`: shift clock.
pub const FXP_EEPROM_EESK: u16 = 0x01;
/// `FXP_EEPROM_EECS`: chip select.
pub const FXP_EEPROM_EECS: u16 = 0x02;
/// `FXP_EEPROM_EEDI`: data in.
pub const FXP_EEPROM_EEDI: u16 = 0x04;
/// `FXP_EEPROM_EEDO`: data out.
pub const FXP_EEPROM_EEDO: u16 = 0x08;

/// `FXP_EEPROM_OPC_ERASE`: serial EEPROM opcode, including the start bit.
pub const FXP_EEPROM_OPC_ERASE: i32 = 0x4;
/// `FXP_EEPROM_OPC_WRITE`.
pub const FXP_EEPROM_OPC_WRITE: i32 = 0x5;
/// `FXP_EEPROM_OPC_READ`.
pub const FXP_EEPROM_OPC_READ: i32 = 0x6;

/// `FXP_EEPROM_REG_MAC`.
pub const FXP_EEPROM_REG_MAC: i32 = 0x00;
/// `FXP_EEPROM_REG_COMPAT`.
pub const FXP_EEPROM_REG_COMPAT: i32 = 0x03;
/// `FXP_EEPROM_REG_COMPAT_MC10`.
pub const FXP_EEPROM_REG_COMPAT_MC10: u16 = 0x0001;
/// `FXP_EEPROM_REG_COMPAT_MC100`.
pub const FXP_EEPROM_REG_COMPAT_MC100: u16 = 0x0002;
/// `FXP_EEPROM_REG_COMPAT_SRV`.
pub const FXP_EEPROM_REG_COMPAT_SRV: u16 = 0x0400;
/// `FXP_EEPROM_REG_PHY`.
pub const FXP_EEPROM_REG_PHY: i32 = 0x06;
/// `FXP_EEPROM_REG_ID`.
pub const FXP_EEPROM_REG_ID: i32 = 0x0a;
/// `FXP_EEPROM_REG_ID_STB`.
pub const FXP_EEPROM_REG_ID_STB: u16 = 0x0002;

/// `FXP_MDI_WRITE`: Management Data Interface opcode.
pub const FXP_MDI_WRITE: u32 = 0x1;
/// `FXP_MDI_READ`.
pub const FXP_MDI_READ: u32 = 0x2;

/// `FXP_PHY_DEVICE_MASK`.
pub const FXP_PHY_DEVICE_MASK: u32 = 0x3f00;
/// `FXP_PHY_SERIAL_ONLY`.
pub const FXP_PHY_SERIAL_ONLY: u32 = 0x8000;
/// `FXP_PHY_NONE`.
pub const FXP_PHY_NONE: u32 = 0;
/// `FXP_PHY_82553A`.
pub const FXP_PHY_82553A: u32 = 1;
/// `FXP_PHY_82553C`.
pub const FXP_PHY_82553C: u32 = 2;
/// `FXP_PHY_82503`.
pub const FXP_PHY_82503: u32 = 3;
/// `FXP_PHY_DP83840`.
pub const FXP_PHY_DP83840: u32 = 4;
/// `FXP_PHY_80C240`.
pub const FXP_PHY_80C240: u32 = 5;
/// `FXP_PHY_80C24`.
pub const FXP_PHY_80C24: u32 = 6;
/// `FXP_PHY_82555`.
pub const FXP_PHY_82555: u32 = 7;
/// `FXP_PHY_DP83840A`.
pub const FXP_PHY_DP83840A: u32 = 10;
/// `FXP_PHY_82555B`.
pub const FXP_PHY_82555B: u32 = 11;

/// `FXP_PHY_BMCR`: PHY Basic Mode Control Register.
pub const FXP_PHY_BMCR: u32 = 0x0;
/// `FXP_PHY_BMCR_FULLDUPLEX`.
pub const FXP_PHY_BMCR_FULLDUPLEX: u32 = 0x0100;
/// `FXP_PHY_BMCR_AUTOEN`.
pub const FXP_PHY_BMCR_AUTOEN: u32 = 0x1000;
/// `FXP_PHY_BMCR_SPEED_100M`.
pub const FXP_PHY_BMCR_SPEED_100M: u32 = 0x2000;

/// `FXP_DP83840_PCR`: DP84830 PHY, PCS Configuration Register.
pub const FXP_DP83840_PCR: u32 = 0x17;
/// `FXP_DP83840_PCR_LED4_MODE`: 1 = LED4 always indicates full duplex.
pub const FXP_DP83840_PCR_LED4_MODE: u32 = 0x0002;
/// `FXP_DP83840_PCR_F_CONNECT`: 1 = force link disconnect function bypass.
pub const FXP_DP83840_PCR_F_CONNECT: u32 = 0x0020;
/// `FXP_DP83840_PCR_BIT8`.
pub const FXP_DP83840_PCR_BIT8: u32 = 0x0100;
/// `FXP_DP83840_PCR_BIT10`.
pub const FXP_DP83840_PCR_BIT10: u32 = 0x0400;

/// `MAXUCODESIZE`.
pub const MAXUCODESIZE: usize = 192;

/// `struct fxp_cb_ucode`: microcode download command block.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FxpCbUcode {
    /// `cb_status`.
    pub cb_status: u16,
    /// `cb_command`.
    pub cb_command: u16,
    /// `link_addr`.
    pub link_addr: u32,
    /// `ucode`.
    pub ucode: [u32; MAXUCODESIZE],
}

/// `FXP_REV_82557_A`: 82557 A.
pub const FXP_REV_82557_A: u32 = 0;
/// `FXP_REV_82557_B`: 82557 B.
pub const FXP_REV_82557_B: u32 = 1;
/// `FXP_REV_82557_C`: 82557 C.
pub const FXP_REV_82557_C: u32 = 2;
/// `FXP_REV_82558_A4`: 82558 A4 stepping.
pub const FXP_REV_82558_A4: u32 = 4;
/// `FXP_REV_82558_B0`: 82558 B0 stepping.
pub const FXP_REV_82558_B0: u32 = 5;
/// `FXP_REV_82559_A0`: 82559 A0 stepping.
pub const FXP_REV_82559_A0: u32 = 8;
/// `FXP_REV_82559S_A`: 82559S A stepping.
pub const FXP_REV_82559S_A: u32 = 9;
/// `FXP_REV_82550`.
pub const FXP_REV_82550: u32 = 12;
/// `FXP_REV_82550_C`: 82550 C stepping.
pub const FXP_REV_82550_C: u32 = 13;
/// `FXP_REV_82551_E`: 82551.
pub const FXP_REV_82551_E: u32 = 14;
/// `FXP_REV_82551_F`: 82551.
pub const FXP_REV_82551_F: u32 = 15;
/// `FXP_REV_82551_10`: 82551.
pub const FXP_REV_82551_10: u32 = 16;

// The sizes the chip's DMA engine depends on.
const _: () = {
    assert!(size_of::<FxpCbTx>() == 256);
    assert!(size_of::<FxpCbConfig>() == 32);
    assert!(size_of::<FxpCbIas>() == 16);
    assert!(size_of::<FxpRfa>() == 16);
    assert!(size_of::<FxpStats>() == 68);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::offset_of;

    #[test]
    fn layouts_match_the_c_structs() {
        assert_eq!(FXP_NTXSEG, 30);
        assert_eq!(size_of::<FxpCbTx>(), 256);
        assert_eq!(size_of::<FxpCbMcs>(), 8 + 2 + 480 + 2);
        assert_eq!(size_of::<FxpCbUcode>(), 8 + 4 * MAXUCODESIZE);
        assert_eq!(offset_of!(FxpCbTx, tbd), SZ_TXCB);
        assert_eq!(offset_of!(FxpCbConfig, byte_count), 8);
        assert_eq!(offset_of!(FxpCbConfig, mc_all), 29);
        assert_eq!(offset_of!(FxpRfa, rbd_addr), FXP_RFA_OFF_RBD_ADDR);
        assert_eq!(offset_of!(FxpRfa, actual_size), FXP_RFA_OFF_ACTUAL_SIZE);
        assert_eq!(offset_of!(FxpRfa, size), FXP_RFA_OFF_SIZE);
        assert_eq!(offset_of!(FxpRfa, link_addr), FXP_RFA_OFF_LINK_ADDR);
        assert_eq!(offset_of!(FxpRfa, rfa_control), FXP_RFA_OFF_RFA_CONTROL);
    }

    #[test]
    fn statack_bits_make_the_mask() {
        assert_eq!(
            FXP_SCB_STATACK_SWI
                | FXP_SCB_STATACK_MDI
                | FXP_SCB_STATACK_RNR
                | FXP_SCB_STATACK_CNA
                | FXP_SCB_STATACK_FR
                | FXP_SCB_STATACK_CXTNO,
            FXP_SCB_STATACK_MASK
        );
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn constants_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/ic/fxpreg.h");
        for (name, value) in [
            ("FXP_CSR_SCB_STATUS", FXP_CSR_SCB_STATUS as i64),
            ("FXP_CSR_SCB_COMMAND", FXP_CSR_SCB_COMMAND as i64),
            ("FXP_CSR_SCB_GENERAL", FXP_CSR_SCB_GENERAL as i64),
            ("FXP_CSR_PORT", FXP_CSR_PORT as i64),
            ("FXP_CSR_EEPROMCONTROL", FXP_CSR_EEPROMCONTROL as i64),
            ("FXP_CSR_MDICONTROL", FXP_CSR_MDICONTROL as i64),
            ("FXP_SCB_CUS_MASK", i64::from(FXP_SCB_CUS_MASK)),
            ("FXP_SCB_STATACK_MASK", i64::from(FXP_SCB_STATACK_MASK)),
            (
                "FXP_SCB_COMMAND_CU_START",
                i64::from(FXP_SCB_COMMAND_CU_START),
            ),
            (
                "FXP_SCB_COMMAND_CU_DUMPRESET",
                i64::from(FXP_SCB_COMMAND_CU_DUMPRESET),
            ),
            (
                "FXP_SCB_COMMAND_RU_START",
                i64::from(FXP_SCB_COMMAND_RU_START),
            ),
            (
                "FXP_SCB_INTRCNTL_REQUEST_SWI",
                i64::from(FXP_SCB_INTRCNTL_REQUEST_SWI),
            ),
            ("FXP_CMD_TMO", i64::from(FXP_CMD_TMO)),
            ("FXP_CB_STATUS_C", i64::from(FXP_CB_STATUS_C)),
            ("FXP_CB_COMMAND_XMIT", i64::from(FXP_CB_COMMAND_XMIT)),
            ("FXP_CB_COMMAND_UCODE", i64::from(FXP_CB_COMMAND_UCODE)),
            ("FXP_CB_COMMAND_EL", i64::from(FXP_CB_COMMAND_EL)),
            ("FXP_RFA_STATUS_C", i64::from(FXP_RFA_STATUS_C)),
            ("FXP_RFA_STATUS_CRC", i64::from(FXP_RFA_STATUS_CRC)),
            ("FXP_RFA_CONTROL_EL", i64::from(FXP_RFA_CONTROL_EL)),
            ("FXP_EEPROM_OPC_READ", i64::from(FXP_EEPROM_OPC_READ)),
            ("FXP_EEPROM_REG_PHY", i64::from(FXP_EEPROM_REG_PHY)),
            ("FXP_REV_82558_A4", i64::from(FXP_REV_82558_A4)),
            ("FXP_REV_82559S_A", i64::from(FXP_REV_82559S_A)),
            ("FXP_REV_82551_10", i64::from(FXP_REV_82551_10)),
            ("MAXMCADDR", MAXMCADDR as i64),
            ("MAXUCODESIZE", MAXUCODESIZE as i64),
        ] {
            assert_eq!(crate::reftest::int(&defs, name), Some(value), "{name}");
        }
    }
}
/* </TESTS> */
