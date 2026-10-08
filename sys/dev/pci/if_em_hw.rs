/* $OpenBSD: if_em_hw.h,v 1.103 2026/08/14 07:21:33 jsg Exp $ */
/* $OpenBSD: if_em_hw.c,v 1.129 2026/08/14 07:21:33 jsg Exp $ */
/* $FreeBSD: if_em_hw.h,v 1.15 2005/05/26 23:32:02 tackerman Exp $ */
/* <LICENSES> */
/*******************************************************************************

  Copyright (c) 2001-2005, Intel Corporation
  All rights reserved.

  Redistribution and use in source and binary forms, with or without
  modification, are permitted provided that the following conditions are met:

   1. Redistributions of source code must retain the above copyright notice,
      this list of conditions and the following disclaimer.

   2. Redistributions in binary form must reproduce the above copyright
      notice, this list of conditions and the following disclaimer in the
      documentation and/or other materials provided with the distribution.

   3. Neither the name of the Intel Corporation nor the names of its
      contributors may be used to endorse or promote products derived from
      this software without specific prior written permission.

  THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
  AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
  IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
  ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT OWNER OR CONTRIBUTORS BE
  LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
  CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
  SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
  INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
  CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
  ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
  POSSIBILITY OF SUCH DAMAGE.

*******************************************************************************/
/* </LICENSES> */

/* <CODE> */
//! em(4)'s shared code: Intel's MAC, PHY and NVM layer for the PRO/1000 family
//! (`<dev/pci/if_em_hw.h>` and `if_em_hw.c`).
//!
//! Upstream: sys/dev/pci/if_em_hw.h @ 3ce1f3f79392, sys/dev/pci/if_em_hw.c @ 3ce1f3f79392
//!
//! The header's register map, bit definitions, descriptor layouts, enumerations and
//! `struct em_hw` come first; then the functions of `if_em_hw.c`, which identify the MAC
//! from the PCI device id, reset and initialise it, read the MAC address and the other NVM
//! words, drive the PHY over MDIO (and the kumeran, I2C and paged BM/HV interfaces), set
//! the link up, and keep the receive address, multicast and VLAN filter tables. if_em.c (the
//! driver proper) owns a `struct em_hw` per device and calls in here.
//!
//! The functions keep the C's names and its status protocol: a C `int32_t` status is a
//! `Result<_, i32>` whose `Err` holds the C's non-zero value unchanged (`-E1000_ERR_PHY`,
//! `E1000_BLK_PHY_RESET`, ...), so `?` is the C's `if (ret_val) return ret_val;`. Output
//! parameters stay `&mut` parameters: callers of the shared code often ignore the status
//! and read the variable, which keeps its previous value when the read failed.
//!
//! ## Deviations
//! - `struct em_hw` keeps the C's members; `back` is a `NonNull<EmOsdep>` (the C's `void *`
//!   is only ever that), `gcu` an `Option<NonNull<GcuSoftc>>`, and one member is added:
//!   `pci_ops`, the table of the functions if_em.c defines for the shared code
//!   (`em_pci_set_mwi`, `em_pci_clear_mwi`, `em_read_pci_cfg`, `em_write_pci_cfg`,
//!   `em_read_pcie_cap_reg`), which the C links by name. The functions of the same names
//!   here call through it.
//! - `em_init_hw` and `em_initialize_hardware_bits` take `struct em_softc *` in the C and
//!   read only `sc->hw` and `sc->num_queues`: they take the `EmHw` and the queue count.
//! - Register names are full constants (`e1000_read_reg(hw, E1000_CTRL)`), see
//!   `if_em_osdep.rs`. Function-like macros are lowercase const fns (`phy_reg`,
//!   `bm_phy_reg_num`, `e1000_rdbal`, ...); `E1000_RDBAL0`/`RDBAH0`/`RDLEN0` (aliases of
//!   macros) are `e1000_rdbal0`/... .
//! - The enumerations are Rust enums with the C's member names, imported into the module
//!   (`hw.mac_type >= em_82543` reads as in C); `em_cable_length`, `em_gg_cable_length` and
//!   `em_igp_cable_length`, whose members are compared with and used as numbers, are `u16`
//!   type aliases with the members as constants.
//! - The bit field unions of the ICH8 flash registers (`ich8_hws_flash_status`, `_ctrl`,
//!   `_regacc`) and `struct sfp_e1000_flags` are a register word with one getter and setter
//!   per field; `ich8_hws_flash_regacc`'s `regval` is 32 bits wide (the C's is a `uint16_t`
//!   under a 32-bit field structure; nothing uses it).
//! - The descriptor unions are `repr(C)` unions of plain-integer structures with a safe
//!   getter per member.
//! - `DBG` is not defined: the `DEBUGFUNC`/`DEBUGOUT*` calls (empty in the C) are left out.
//! - `E1000_READ_REG_IO` expands to `em_read_reg_io()`, which no file defines and nothing
//!   uses; it has no counterpart.
//! - The EEPROM functions take slices for the C's `uint16_t *data` and refuse one shorter
//!   than `words` (`-E1000_ERR_EEPROM`); a shadow RAM shorter than
//!   `E1000_SHADOW_RAM_WORDS` reads as unmodified past its end and refuses writes there.
//! - `em_mc_addr_list_update` takes a slice and stops at its end; `em_hash_mc_addr` and
//!   `em_rar_set` take `&[u8; 6]`.
//! - `em_host_if_read_cookie` returns nothing (the C's always succeeds); the cookie goes
//!   through its bytes (`EmHostMngDhcpCookie::from_bytes`/`to_bytes`).
//! - `em_write_eeprom_spi` skips the page-boundary test when `page_size` is 0, where the C
//!   divides by it.
//! - Variables the C leaves uninitialised when a read fails start at 0; NULL checks on `hw`
//!   are gone (a reference is never NULL).
//! - Helpers that only regroup the C's own code: `em_clear_shadow_ram`, `em_shadow_word`,
//!   `em_read_clear` (`em_clear_hw_cntrs`), `em_lplu_restore_smart_speed` and
//!   `em_lplu_disable_smart_speed` (the D3/D0 LPLU functions), `em_ich8_read_hsfsts`,
//!   `em_ich8_write_hsfsts` and `em_ich8_retry_after_error` (the flash cycles).
//! - `em_valid_nvm_bank_detect_ich8lan`'s bare `-1` is `Err(-1)`.

use alloc::boxed::Box;
use core::ptr::NonNull;

use crate::dev::mii::rgephyreg::{
    RGEPHY_CR, RGEPHY_CR_ASSERT_CRS, RGEPHY_CR_MDI_MASK, RGEPHY_LC, RGEPHY_LC_DUPLEX,
    RGEPHY_LC_LINK, RGEPHY_LC_P2, RGEPHY_LC_PULSE_42MS, RGEPHY_LC_RX, RGEPHY_PS, RGEPHY_PS_PAGE_2,
    RGEPHY_SR,
};
use crate::dev::pci::gcu_var::GcuSoftc;
use crate::dev::pci::if_em_osdep::*;
use crate::dev::pci::if_em_soc::{RTL8211_E_PHY_ID, gcu_miibus_readreg, gcu_miibus_writereg};
use crate::kern::subr_prf::printf;
use crate::machine::cpu::delay;

/// `E1000_HOST_IF_MAX_SIZE`.
pub const E1000_HOST_IF_MAX_SIZE: u32 = 2048;
/// `E1000_SUCCESS`.
pub const E1000_SUCCESS: u32 = 0;
/// `E1000_ERR_EEPROM`.
pub const E1000_ERR_EEPROM: i32 = 1;
/// `E1000_ERR_PHY`.
pub const E1000_ERR_PHY: i32 = 2;
/// `E1000_ERR_CONFIG`.
pub const E1000_ERR_CONFIG: i32 = 3;
/// `E1000_ERR_PARAM`.
pub const E1000_ERR_PARAM: i32 = 4;
/// `E1000_ERR_MAC_TYPE`.
pub const E1000_ERR_MAC_TYPE: i32 = 5;
/// `E1000_ERR_PHY_TYPE`.
pub const E1000_ERR_PHY_TYPE: i32 = 6;
/// `E1000_ERR_RESET`.
pub const E1000_ERR_RESET: i32 = 9;
/// `E1000_ERR_MASTER_REQUESTS_PENDING`.
pub const E1000_ERR_MASTER_REQUESTS_PENDING: i32 = 10;
/// `E1000_ERR_HOST_INTERFACE_COMMAND`.
pub const E1000_ERR_HOST_INTERFACE_COMMAND: i32 = 11;
/// `E1000_BLK_PHY_RESET`.
pub const E1000_BLK_PHY_RESET: i32 = 12;
/// `E1000_ERR_SWFW_SYNC`.
pub const E1000_ERR_SWFW_SYNC: i32 = 13;
/// `E1000_NOT_IMPLEMENTED`.
pub const E1000_NOT_IMPLEMENTED: i32 = 14;
/// `E1000_DEFER_INIT`.
pub const E1000_DEFER_INIT: i32 = 15;
/// `E1000_BYTE_SWAP_WORD(_value)`: swaps the two bytes of a word.
pub const fn e1000_byte_swap_word(value: u16) -> u16 {
    ((value & 0x00ff) << 8) | ((value & 0xff00) >> 8)
}
/// `E1000_MNG_DHCP_TX_PAYLOAD_CMD`.
pub const E1000_MNG_DHCP_TX_PAYLOAD_CMD: u32 = 64;
/// `E1000_HI_MAX_MNG_DATA_LENGTH`: Host Interface data length.
pub const E1000_HI_MAX_MNG_DATA_LENGTH: usize = 0x6F8;
/// `E1000_MNG_DHCP_COMMAND_TIMEOUT`: Time in ms to process MNG command.
pub const E1000_MNG_DHCP_COMMAND_TIMEOUT: u32 = 10;
/// `E1000_MNG_DHCP_COOKIE_OFFSET`: Cookie offset.
pub const E1000_MNG_DHCP_COOKIE_OFFSET: u32 = 0x6F0;
/// `E1000_MNG_DHCP_COOKIE_LENGTH`: Cookie length.
pub const E1000_MNG_DHCP_COOKIE_LENGTH: u32 = 0x10;
/// `E1000_MNG_IAMT_MODE`.
pub const E1000_MNG_IAMT_MODE: u32 = 0x3;
/// `E1000_MNG_ICH_IAMT_MODE`.
pub const E1000_MNG_ICH_IAMT_MODE: u32 = 0x2;
/// `E1000_IAMT_SIGNATURE`: Intel(R) Active Management Technology signature.
pub const E1000_IAMT_SIGNATURE: u32 = 0x544D4149;
/// `E1000_MNG_DHCP_COOKIE_STATUS_PARSING_SUPPORT`: DHCP parsing enabled.
pub const E1000_MNG_DHCP_COOKIE_STATUS_PARSING_SUPPORT: u32 = 0x1;
/// `E1000_MNG_DHCP_COOKIE_STATUS_VLAN_SUPPORT`: DHCP parsing enabled.
pub const E1000_MNG_DHCP_COOKIE_STATUS_VLAN_SUPPORT: u32 = 0x2;
/// `E1000_VFTA_ENTRY_SHIFT`.
pub const E1000_VFTA_ENTRY_SHIFT: u32 = 0x5;
/// `E1000_VFTA_ENTRY_MASK`.
pub const E1000_VFTA_ENTRY_MASK: u32 = 0x7F;
/// `E1000_VFTA_ENTRY_BIT_SHIFT_MASK`.
pub const E1000_VFTA_ENTRY_BIT_SHIFT_MASK: u32 = 0x1F;
// `E1000_READ_REG_IO(a, reg)` expands to `em_read_reg_io()`, which no file defines (the
// macro is never used); it has no Rust counterpart.
/// `E1000_WRITE_REG_IO(a, reg, val)`: writes register `reg` (the full `E1000_*` offset)
/// through the I/O BAR's indirect access window.
pub fn e1000_write_reg_io(hw: &EmHw, reg: u32, val: u32) {
    em_write_reg_io(hw, reg, val);
}
/// `E1000_DEV_ID_82542`.
pub const E1000_DEV_ID_82542: u16 = 0x1000;
/// `E1000_DEV_ID_82543GC_FIBER`.
pub const E1000_DEV_ID_82543GC_FIBER: u16 = 0x1001;
/// `E1000_DEV_ID_82543GC_COPPER`.
pub const E1000_DEV_ID_82543GC_COPPER: u16 = 0x1004;
/// `E1000_DEV_ID_82544EI_COPPER`.
pub const E1000_DEV_ID_82544EI_COPPER: u16 = 0x1008;
/// `E1000_DEV_ID_82544EI_FIBER`.
pub const E1000_DEV_ID_82544EI_FIBER: u16 = 0x1009;
/// `E1000_DEV_ID_82544GC_COPPER`.
pub const E1000_DEV_ID_82544GC_COPPER: u16 = 0x100C;
/// `E1000_DEV_ID_82544GC_LOM`.
pub const E1000_DEV_ID_82544GC_LOM: u16 = 0x100D;
/// `E1000_DEV_ID_82540EM`.
pub const E1000_DEV_ID_82540EM: u16 = 0x100E;
/// `E1000_DEV_ID_82540EM_LOM`.
pub const E1000_DEV_ID_82540EM_LOM: u16 = 0x1015;
/// `E1000_DEV_ID_82540EP_LOM`.
pub const E1000_DEV_ID_82540EP_LOM: u16 = 0x1016;
/// `E1000_DEV_ID_82540EP`.
pub const E1000_DEV_ID_82540EP: u16 = 0x1017;
/// `E1000_DEV_ID_82540EP_LP`.
pub const E1000_DEV_ID_82540EP_LP: u16 = 0x101E;
/// `E1000_DEV_ID_82545EM_COPPER`.
pub const E1000_DEV_ID_82545EM_COPPER: u16 = 0x100F;
/// `E1000_DEV_ID_82545EM_FIBER`.
pub const E1000_DEV_ID_82545EM_FIBER: u16 = 0x1011;
/// `E1000_DEV_ID_82545GM_COPPER`.
pub const E1000_DEV_ID_82545GM_COPPER: u16 = 0x1026;
/// `E1000_DEV_ID_82545GM_FIBER`.
pub const E1000_DEV_ID_82545GM_FIBER: u16 = 0x1027;
/// `E1000_DEV_ID_82545GM_SERDES`.
pub const E1000_DEV_ID_82545GM_SERDES: u16 = 0x1028;
/// `E1000_DEV_ID_82546EB_COPPER`.
pub const E1000_DEV_ID_82546EB_COPPER: u16 = 0x1010;
/// `E1000_DEV_ID_82546EB_FIBER`.
pub const E1000_DEV_ID_82546EB_FIBER: u16 = 0x1012;
/// `E1000_DEV_ID_82546EB_QUAD_COPPER`.
pub const E1000_DEV_ID_82546EB_QUAD_COPPER: u16 = 0x101D;
/// `E1000_DEV_ID_82541EI`.
pub const E1000_DEV_ID_82541EI: u16 = 0x1013;
/// `E1000_DEV_ID_82541EI_MOBILE`.
pub const E1000_DEV_ID_82541EI_MOBILE: u16 = 0x1018;
/// `E1000_DEV_ID_82541ER_LOM`.
pub const E1000_DEV_ID_82541ER_LOM: u16 = 0x1014;
/// `E1000_DEV_ID_82541ER`.
pub const E1000_DEV_ID_82541ER: u16 = 0x1078;
/// `E1000_DEV_ID_82547GI`.
pub const E1000_DEV_ID_82547GI: u16 = 0x1075;
/// `E1000_DEV_ID_82541GI`.
pub const E1000_DEV_ID_82541GI: u16 = 0x1076;
/// `E1000_DEV_ID_82541GI_MOBILE`.
pub const E1000_DEV_ID_82541GI_MOBILE: u16 = 0x1077;
/// `E1000_DEV_ID_82541GI_LF`.
pub const E1000_DEV_ID_82541GI_LF: u16 = 0x107C;
/// `E1000_DEV_ID_82546GB_COPPER`.
pub const E1000_DEV_ID_82546GB_COPPER: u16 = 0x1079;
/// `E1000_DEV_ID_82546GB_FIBER`.
pub const E1000_DEV_ID_82546GB_FIBER: u16 = 0x107A;
/// `E1000_DEV_ID_82546GB_SERDES`.
pub const E1000_DEV_ID_82546GB_SERDES: u16 = 0x107B;
/// `E1000_DEV_ID_82546GB_PCIE`.
pub const E1000_DEV_ID_82546GB_PCIE: u16 = 0x108A;
/// `E1000_DEV_ID_82546GB_QUAD_COPPER`.
pub const E1000_DEV_ID_82546GB_QUAD_COPPER: u16 = 0x1099;
/// `E1000_DEV_ID_82547EI`.
pub const E1000_DEV_ID_82547EI: u16 = 0x1019;
/// `E1000_DEV_ID_82547EI_MOBILE`.
pub const E1000_DEV_ID_82547EI_MOBILE: u16 = 0x101A;
/// `E1000_DEV_ID_82571EB_COPPER`.
pub const E1000_DEV_ID_82571EB_COPPER: u16 = 0x105E;
/// `E1000_DEV_ID_82571EB_FIBER`.
pub const E1000_DEV_ID_82571EB_FIBER: u16 = 0x105F;
/// `E1000_DEV_ID_82571EB_SERDES`.
pub const E1000_DEV_ID_82571EB_SERDES: u16 = 0x1060;
/// `E1000_DEV_ID_82571EB_SERDES_DUAL`.
pub const E1000_DEV_ID_82571EB_SERDES_DUAL: u16 = 0x10D9;
/// `E1000_DEV_ID_82571EB_SERDES_QUAD`.
pub const E1000_DEV_ID_82571EB_SERDES_QUAD: u16 = 0x10DA;
/// `E1000_DEV_ID_82571EB_QUAD_COPPER`.
pub const E1000_DEV_ID_82571EB_QUAD_COPPER: u16 = 0x10A4;
/// `E1000_DEV_ID_82571EB_QUAD_FIBER`.
pub const E1000_DEV_ID_82571EB_QUAD_FIBER: u16 = 0x10A5;
/// `E1000_DEV_ID_82571EB_QUAD_COPPER_LP`.
pub const E1000_DEV_ID_82571EB_QUAD_COPPER_LP: u16 = 0x10BC;
/// `E1000_DEV_ID_82571PT_QUAD_COPPER`.
pub const E1000_DEV_ID_82571PT_QUAD_COPPER: u16 = 0x10D5;
/// `E1000_DEV_ID_82572EI_COPPER`.
pub const E1000_DEV_ID_82572EI_COPPER: u16 = 0x107D;
/// `E1000_DEV_ID_82572EI_FIBER`.
pub const E1000_DEV_ID_82572EI_FIBER: u16 = 0x107E;
/// `E1000_DEV_ID_82572EI_SERDES`.
pub const E1000_DEV_ID_82572EI_SERDES: u16 = 0x107F;
/// `E1000_DEV_ID_82572EI`.
pub const E1000_DEV_ID_82572EI: u16 = 0x10B9;
/// `E1000_DEV_ID_82573E`.
pub const E1000_DEV_ID_82573E: u16 = 0x108B;
/// `E1000_DEV_ID_82573E_IAMT`.
pub const E1000_DEV_ID_82573E_IAMT: u16 = 0x108C;
/// `E1000_DEV_ID_82573L`.
pub const E1000_DEV_ID_82573L: u16 = 0x109A;
/// `E1000_DEV_ID_82574L`.
pub const E1000_DEV_ID_82574L: u16 = 0x10D3;
/// `E1000_DEV_ID_82574LA`.
pub const E1000_DEV_ID_82574LA: u16 = 0x10F6;
/// `E1000_DEV_ID_82546GB_2`.
pub const E1000_DEV_ID_82546GB_2: u16 = 0x109B;
/// `E1000_DEV_ID_82571EB_AT`.
pub const E1000_DEV_ID_82571EB_AT: u16 = 0x10A0;
/// `E1000_DEV_ID_82571EB_AF`.
pub const E1000_DEV_ID_82571EB_AF: u16 = 0x10A1;
/// `E1000_DEV_ID_82573L_PL_1`.
pub const E1000_DEV_ID_82573L_PL_1: u16 = 0x10B0;
/// `E1000_DEV_ID_82573V_PM`.
pub const E1000_DEV_ID_82573V_PM: u16 = 0x10B2;
/// `E1000_DEV_ID_82573E_PM`.
pub const E1000_DEV_ID_82573E_PM: u16 = 0x10B3;
/// `E1000_DEV_ID_82573L_PL_2`.
pub const E1000_DEV_ID_82573L_PL_2: u16 = 0x10B4;
/// `E1000_DEV_ID_82546GB_QUAD_COPPER_KSP3`.
pub const E1000_DEV_ID_82546GB_QUAD_COPPER_KSP3: u16 = 0x10B5;
/// `E1000_DEV_ID_80003ES2LAN_COPPER_DPT`.
pub const E1000_DEV_ID_80003ES2LAN_COPPER_DPT: u16 = 0x1096;
/// `E1000_DEV_ID_80003ES2LAN_SERDES_DPT`.
pub const E1000_DEV_ID_80003ES2LAN_SERDES_DPT: u16 = 0x1098;
/// `E1000_DEV_ID_80003ES2LAN_COPPER_SPT`.
pub const E1000_DEV_ID_80003ES2LAN_COPPER_SPT: u16 = 0x10BA;
/// `E1000_DEV_ID_80003ES2LAN_SERDES_SPT`.
pub const E1000_DEV_ID_80003ES2LAN_SERDES_SPT: u16 = 0x10BB;
/// `E1000_DEV_ID_ICH8_82567V_3`.
pub const E1000_DEV_ID_ICH8_82567V_3: u16 = 0x1501;
/// `E1000_DEV_ID_ICH8_IGP_M_AMT`.
pub const E1000_DEV_ID_ICH8_IGP_M_AMT: u16 = 0x1049;
/// `E1000_DEV_ID_ICH8_IGP_AMT`.
pub const E1000_DEV_ID_ICH8_IGP_AMT: u16 = 0x104A;
/// `E1000_DEV_ID_ICH8_IGP_C`.
pub const E1000_DEV_ID_ICH8_IGP_C: u16 = 0x104B;
/// `E1000_DEV_ID_ICH8_IFE`.
pub const E1000_DEV_ID_ICH8_IFE: u16 = 0x104C;
/// `E1000_DEV_ID_ICH8_IFE_GT`.
pub const E1000_DEV_ID_ICH8_IFE_GT: u16 = 0x10C4;
/// `E1000_DEV_ID_ICH8_IFE_G`.
pub const E1000_DEV_ID_ICH8_IFE_G: u16 = 0x10C5;
/// `E1000_DEV_ID_ICH8_IGP_M`.
pub const E1000_DEV_ID_ICH8_IGP_M: u16 = 0x104D;
/// `E1000_DEV_ID_ICH9_IGP_M`.
pub const E1000_DEV_ID_ICH9_IGP_M: u16 = 0x10BF;
/// `E1000_DEV_ID_ICH9_IGP_M_AMT`.
pub const E1000_DEV_ID_ICH9_IGP_M_AMT: u16 = 0x10F5;
/// `E1000_DEV_ID_ICH9_IGP_M_V`.
pub const E1000_DEV_ID_ICH9_IGP_M_V: u16 = 0x10CB;
/// `E1000_DEV_ID_ICH9_IGP_AMT`.
pub const E1000_DEV_ID_ICH9_IGP_AMT: u16 = 0x10BD;
/// `E1000_DEV_ID_ICH9_BM`.
pub const E1000_DEV_ID_ICH9_BM: u16 = 0x10E5;
/// `E1000_DEV_ID_ICH9_IGP_C`.
pub const E1000_DEV_ID_ICH9_IGP_C: u16 = 0x294C;
/// `E1000_DEV_ID_ICH9_IFE`.
pub const E1000_DEV_ID_ICH9_IFE: u16 = 0x10C0;
/// `E1000_DEV_ID_ICH9_IFE_GT`.
pub const E1000_DEV_ID_ICH9_IFE_GT: u16 = 0x10C3;
/// `E1000_DEV_ID_ICH9_IFE_G`.
pub const E1000_DEV_ID_ICH9_IFE_G: u16 = 0x10C2;
/// `E1000_DEV_ID_ICH10_R_BM_LM`.
pub const E1000_DEV_ID_ICH10_R_BM_LM: u16 = 0x10CC;
/// `E1000_DEV_ID_ICH10_R_BM_LF`.
pub const E1000_DEV_ID_ICH10_R_BM_LF: u16 = 0x10CD;
/// `E1000_DEV_ID_ICH10_R_BM_V`.
pub const E1000_DEV_ID_ICH10_R_BM_V: u16 = 0x10CE;
/// `E1000_DEV_ID_ICH10_D_BM_LM`.
pub const E1000_DEV_ID_ICH10_D_BM_LM: u16 = 0x10DE;
/// `E1000_DEV_ID_ICH10_D_BM_LF`.
pub const E1000_DEV_ID_ICH10_D_BM_LF: u16 = 0x10DF;
/// `E1000_DEV_ID_ICH10_D_BM_V`.
pub const E1000_DEV_ID_ICH10_D_BM_V: u16 = 0x1525;
/// `E1000_DEV_ID_PCH_M_HV_LM`.
pub const E1000_DEV_ID_PCH_M_HV_LM: u16 = 0x10EA;
/// `E1000_DEV_ID_PCH_M_HV_LC`.
pub const E1000_DEV_ID_PCH_M_HV_LC: u16 = 0x10EB;
/// `E1000_DEV_ID_PCH_D_HV_DM`.
pub const E1000_DEV_ID_PCH_D_HV_DM: u16 = 0x10EF;
/// `E1000_DEV_ID_PCH_D_HV_DC`.
pub const E1000_DEV_ID_PCH_D_HV_DC: u16 = 0x10F0;
/// `E1000_DEV_ID_PCH2_LV_LM`.
pub const E1000_DEV_ID_PCH2_LV_LM: u16 = 0x1502;
/// `E1000_DEV_ID_PCH2_LV_V`.
pub const E1000_DEV_ID_PCH2_LV_V: u16 = 0x1503;
/// `E1000_DEV_ID_PCH_LPT_I217_LM`.
pub const E1000_DEV_ID_PCH_LPT_I217_LM: u16 = 0x153A;
/// `E1000_DEV_ID_PCH_LPT_I217_V`.
pub const E1000_DEV_ID_PCH_LPT_I217_V: u16 = 0x153B;
/// `E1000_DEV_ID_PCH_LPTLP_I218_LM`.
pub const E1000_DEV_ID_PCH_LPTLP_I218_LM: u16 = 0x155A;
/// `E1000_DEV_ID_PCH_LPTLP_I218_V`.
pub const E1000_DEV_ID_PCH_LPTLP_I218_V: u16 = 0x1559;
/// `E1000_DEV_ID_PCH_I218_LM2`.
pub const E1000_DEV_ID_PCH_I218_LM2: u16 = 0x15A0;
/// `E1000_DEV_ID_PCH_I218_V2`.
pub const E1000_DEV_ID_PCH_I218_V2: u16 = 0x15A1;
/// `E1000_DEV_ID_PCH_I218_LM3`.
pub const E1000_DEV_ID_PCH_I218_LM3: u16 = 0x15A2;
/// `E1000_DEV_ID_PCH_I218_V3`.
pub const E1000_DEV_ID_PCH_I218_V3: u16 = 0x15A3;
/// `E1000_DEV_ID_PCH_SPT_I219_LM`.
pub const E1000_DEV_ID_PCH_SPT_I219_LM: u16 = 0x156F;
/// `E1000_DEV_ID_PCH_SPT_I219_V`.
pub const E1000_DEV_ID_PCH_SPT_I219_V: u16 = 0x1570;
/// `E1000_DEV_ID_PCH_SPT_I219_LM2`.
pub const E1000_DEV_ID_PCH_SPT_I219_LM2: u16 = 0x15B7;
/// `E1000_DEV_ID_PCH_SPT_I219_V2`.
pub const E1000_DEV_ID_PCH_SPT_I219_V2: u16 = 0x15B8;
/// `E1000_DEV_ID_PCH_LBG_I219_LM3`.
pub const E1000_DEV_ID_PCH_LBG_I219_LM3: u16 = 0x15B9;
/// `E1000_DEV_ID_PCH_SPT_I219_LM4`.
pub const E1000_DEV_ID_PCH_SPT_I219_LM4: u16 = 0x15D7;
/// `E1000_DEV_ID_PCH_SPT_I219_V4`.
pub const E1000_DEV_ID_PCH_SPT_I219_V4: u16 = 0x15D8;
/// `E1000_DEV_ID_PCH_SPT_I219_LM5`.
pub const E1000_DEV_ID_PCH_SPT_I219_LM5: u16 = 0x15E3;
/// `E1000_DEV_ID_PCH_SPT_I219_V5`.
pub const E1000_DEV_ID_PCH_SPT_I219_V5: u16 = 0x15D6;
/// `E1000_DEV_ID_PCH_CNP_I219_LM6`.
pub const E1000_DEV_ID_PCH_CNP_I219_LM6: u16 = 0x15BD;
/// `E1000_DEV_ID_PCH_CNP_I219_V6`.
pub const E1000_DEV_ID_PCH_CNP_I219_V6: u16 = 0x15BE;
/// `E1000_DEV_ID_PCH_CNP_I219_LM7`.
pub const E1000_DEV_ID_PCH_CNP_I219_LM7: u16 = 0x15BB;
/// `E1000_DEV_ID_PCH_CNP_I219_V7`.
pub const E1000_DEV_ID_PCH_CNP_I219_V7: u16 = 0x15BC;
/// `E1000_DEV_ID_PCH_ICP_I219_LM8`.
pub const E1000_DEV_ID_PCH_ICP_I219_LM8: u16 = 0x15DF;
/// `E1000_DEV_ID_PCH_ICP_I219_V8`.
pub const E1000_DEV_ID_PCH_ICP_I219_V8: u16 = 0x15E0;
/// `E1000_DEV_ID_PCH_ICP_I219_LM9`.
pub const E1000_DEV_ID_PCH_ICP_I219_LM9: u16 = 0x15E1;
/// `E1000_DEV_ID_PCH_ICP_I219_V9`.
pub const E1000_DEV_ID_PCH_ICP_I219_V9: u16 = 0x15E2;
/// `E1000_DEV_ID_PCH_CMP_I219_LM10`.
pub const E1000_DEV_ID_PCH_CMP_I219_LM10: u16 = 0x0D4E;
/// `E1000_DEV_ID_PCH_CMP_I219_V10`.
pub const E1000_DEV_ID_PCH_CMP_I219_V10: u16 = 0x0D4F;
/// `E1000_DEV_ID_PCH_CMP_I219_LM11`.
pub const E1000_DEV_ID_PCH_CMP_I219_LM11: u16 = 0x0D4C;
/// `E1000_DEV_ID_PCH_CMP_I219_V11`.
pub const E1000_DEV_ID_PCH_CMP_I219_V11: u16 = 0x0D4D;
/// `E1000_DEV_ID_PCH_CMP_I219_LM12`.
pub const E1000_DEV_ID_PCH_CMP_I219_LM12: u16 = 0x0D53;
/// `E1000_DEV_ID_PCH_CMP_I219_V12`.
pub const E1000_DEV_ID_PCH_CMP_I219_V12: u16 = 0x0D55;
/// `E1000_DEV_ID_PCH_TGP_I219_LM13`.
pub const E1000_DEV_ID_PCH_TGP_I219_LM13: u16 = 0x15FB;
/// `E1000_DEV_ID_PCH_TGP_I219_V13`.
pub const E1000_DEV_ID_PCH_TGP_I219_V13: u16 = 0x15FC;
/// `E1000_DEV_ID_PCH_TGP_I219_LM14`.
pub const E1000_DEV_ID_PCH_TGP_I219_LM14: u16 = 0x15F9;
/// `E1000_DEV_ID_PCH_TGP_I219_V14`.
pub const E1000_DEV_ID_PCH_TGP_I219_V14: u16 = 0x15FA;
/// `E1000_DEV_ID_PCH_TGP_I219_LM15`.
pub const E1000_DEV_ID_PCH_TGP_I219_LM15: u16 = 0x15F4;
/// `E1000_DEV_ID_PCH_TGP_I219_V15`.
pub const E1000_DEV_ID_PCH_TGP_I219_V15: u16 = 0x15F5;
/// `E1000_DEV_ID_PCH_ADP_I219_LM16`.
pub const E1000_DEV_ID_PCH_ADP_I219_LM16: u16 = 0x1A1E;
/// `E1000_DEV_ID_PCH_ADP_I219_V16`.
pub const E1000_DEV_ID_PCH_ADP_I219_V16: u16 = 0x1A1F;
/// `E1000_DEV_ID_PCH_ADP_I219_LM17`.
pub const E1000_DEV_ID_PCH_ADP_I219_LM17: u16 = 0x1A1C;
/// `E1000_DEV_ID_PCH_ADP_I219_V17`.
pub const E1000_DEV_ID_PCH_ADP_I219_V17: u16 = 0x1A1D;
/// `E1000_DEV_ID_PCH_MTP_I219_LM18`.
pub const E1000_DEV_ID_PCH_MTP_I219_LM18: u16 = 0x550A;
/// `E1000_DEV_ID_PCH_MTP_I219_V18`.
pub const E1000_DEV_ID_PCH_MTP_I219_V18: u16 = 0x550B;
/// `E1000_DEV_ID_PCH_MTP_I219_LM19`.
pub const E1000_DEV_ID_PCH_MTP_I219_LM19: u16 = 0x550C;
/// `E1000_DEV_ID_PCH_MTP_I219_V19`.
pub const E1000_DEV_ID_PCH_MTP_I219_V19: u16 = 0x550D;
/// `E1000_DEV_ID_PCH_LNP_I219_LM20`.
pub const E1000_DEV_ID_PCH_LNP_I219_LM20: u16 = 0x550E;
/// `E1000_DEV_ID_PCH_LNP_I219_V20`.
pub const E1000_DEV_ID_PCH_LNP_I219_V20: u16 = 0x550F;
/// `E1000_DEV_ID_PCH_LNP_I219_LM21`.
pub const E1000_DEV_ID_PCH_LNP_I219_LM21: u16 = 0x5510;
/// `E1000_DEV_ID_PCH_LNP_I219_V21`.
pub const E1000_DEV_ID_PCH_LNP_I219_V21: u16 = 0x5511;
/// `E1000_DEV_ID_PCH_RPL_I219_LM22`.
pub const E1000_DEV_ID_PCH_RPL_I219_LM22: u16 = 0x0DC7;
/// `E1000_DEV_ID_PCH_RPL_I219_V22`.
pub const E1000_DEV_ID_PCH_RPL_I219_V22: u16 = 0x0DC8;
/// `E1000_DEV_ID_PCH_RPL_I219_LM23`.
pub const E1000_DEV_ID_PCH_RPL_I219_LM23: u16 = 0x0DC5;
/// `E1000_DEV_ID_PCH_RPL_I219_V23`.
pub const E1000_DEV_ID_PCH_RPL_I219_V23: u16 = 0x0DC6;
/// `E1000_DEV_ID_PCH_ARL_I219_LM24`.
pub const E1000_DEV_ID_PCH_ARL_I219_LM24: u16 = 0x57A0;
/// `E1000_DEV_ID_PCH_ARL_I219_V24`.
pub const E1000_DEV_ID_PCH_ARL_I219_V24: u16 = 0x57A1;
/// `E1000_DEV_ID_PCH_PTP_I219_LM25`.
pub const E1000_DEV_ID_PCH_PTP_I219_LM25: u16 = 0x57B3;
/// `E1000_DEV_ID_PCH_PTP_I219_V25`.
pub const E1000_DEV_ID_PCH_PTP_I219_V25: u16 = 0x57B4;
/// `E1000_DEV_ID_PCH_WCL_I219_LM27`.
pub const E1000_DEV_ID_PCH_WCL_I219_LM27: u16 = 0x57B7;
/// `E1000_DEV_ID_PCH_WCL_I219_V27`.
pub const E1000_DEV_ID_PCH_WCL_I219_V27: u16 = 0x57B8;
/// `E1000_DEV_ID_82575EB_PT`.
pub const E1000_DEV_ID_82575EB_PT: u16 = 0x10A7;
/// `E1000_DEV_ID_82575EB_PF`.
pub const E1000_DEV_ID_82575EB_PF: u16 = 0x10A9;
/// `E1000_DEV_ID_82575GB_QP`.
pub const E1000_DEV_ID_82575GB_QP: u16 = 0x10D6;
/// `E1000_DEV_ID_82575GB_QP_PM`.
pub const E1000_DEV_ID_82575GB_QP_PM: u16 = 0x10E2;
/// `E1000_DEV_ID_82576`.
pub const E1000_DEV_ID_82576: u16 = 0x10C9;
/// `E1000_DEV_ID_82576_FIBER`.
pub const E1000_DEV_ID_82576_FIBER: u16 = 0x10E6;
/// `E1000_DEV_ID_82576_SERDES`.
pub const E1000_DEV_ID_82576_SERDES: u16 = 0x10E7;
/// `E1000_DEV_ID_82576_QUAD_COPPER`.
pub const E1000_DEV_ID_82576_QUAD_COPPER: u16 = 0x10E8;
/// `E1000_DEV_ID_82576_NS`.
pub const E1000_DEV_ID_82576_NS: u16 = 0x150A;
/// `E1000_DEV_ID_82583V`.
pub const E1000_DEV_ID_82583V: u16 = 0x150C;
/// `E1000_DEV_ID_82576_NS_SERDES`.
pub const E1000_DEV_ID_82576_NS_SERDES: u16 = 0x1518;
/// `E1000_DEV_ID_82576_SERDES_QUAD`.
pub const E1000_DEV_ID_82576_SERDES_QUAD: u16 = 0x150D;
/// `E1000_DEV_ID_82580_COPPER`.
pub const E1000_DEV_ID_82580_COPPER: u16 = 0x150E;
/// `E1000_DEV_ID_82580_FIBER`.
pub const E1000_DEV_ID_82580_FIBER: u16 = 0x150F;
/// `E1000_DEV_ID_82580_SERDES`.
pub const E1000_DEV_ID_82580_SERDES: u16 = 0x1510;
/// `E1000_DEV_ID_82580_SGMII`.
pub const E1000_DEV_ID_82580_SGMII: u16 = 0x1511;
/// `E1000_DEV_ID_82580_COPPER_DUAL`.
pub const E1000_DEV_ID_82580_COPPER_DUAL: u16 = 0x1516;
/// `E1000_DEV_ID_82580_QUAD_FIBER`.
pub const E1000_DEV_ID_82580_QUAD_FIBER: u16 = 0x1527;
/// `E1000_DEV_ID_DH89XXCC_SGMII`.
pub const E1000_DEV_ID_DH89XXCC_SGMII: u16 = 0x0438;
/// `E1000_DEV_ID_DH89XXCC_SERDES`.
pub const E1000_DEV_ID_DH89XXCC_SERDES: u16 = 0x043A;
/// `E1000_DEV_ID_DH89XXCC_BACKPLANE`.
pub const E1000_DEV_ID_DH89XXCC_BACKPLANE: u16 = 0x043C;
/// `E1000_DEV_ID_DH89XXCC_SFP`.
pub const E1000_DEV_ID_DH89XXCC_SFP: u16 = 0x0440;
/// `E1000_DEV_ID_I350_COPPER`.
pub const E1000_DEV_ID_I350_COPPER: u16 = 0x1521;
/// `E1000_DEV_ID_I350_FIBER`.
pub const E1000_DEV_ID_I350_FIBER: u16 = 0x1522;
/// `E1000_DEV_ID_I350_SERDES`.
pub const E1000_DEV_ID_I350_SERDES: u16 = 0x1523;
/// `E1000_DEV_ID_I350_SGMII`.
pub const E1000_DEV_ID_I350_SGMII: u16 = 0x1524;
/// `E1000_DEV_ID_82576_QUAD_CU_ET2`.
pub const E1000_DEV_ID_82576_QUAD_CU_ET2: u16 = 0x1526;
/// `E1000_DEV_ID_I210_COPPER`.
pub const E1000_DEV_ID_I210_COPPER: u16 = 0x1533;
/// `E1000_DEV_ID_I210_COPPER_OEM1`.
pub const E1000_DEV_ID_I210_COPPER_OEM1: u16 = 0x1534;
/// `E1000_DEV_ID_I210_COPPER_IT`.
pub const E1000_DEV_ID_I210_COPPER_IT: u16 = 0x1535;
/// `E1000_DEV_ID_I210_FIBER`.
pub const E1000_DEV_ID_I210_FIBER: u16 = 0x1536;
/// `E1000_DEV_ID_I210_SERDES`.
pub const E1000_DEV_ID_I210_SERDES: u16 = 0x1537;
/// `E1000_DEV_ID_I210_SGMII`.
pub const E1000_DEV_ID_I210_SGMII: u16 = 0x1538;
/// `E1000_DEV_ID_I210_COPPER_FLASHLESS`.
pub const E1000_DEV_ID_I210_COPPER_FLASHLESS: u16 = 0x157B;
/// `E1000_DEV_ID_I210_SERDES_FLASHLESS`.
pub const E1000_DEV_ID_I210_SERDES_FLASHLESS: u16 = 0x157C;
/// `E1000_DEV_ID_I211_COPPER`.
pub const E1000_DEV_ID_I211_COPPER: u16 = 0x1539;
/// `E1000_DEV_ID_I350_DA4`.
pub const E1000_DEV_ID_I350_DA4: u16 = 0x1546;
/// `E1000_DEV_ID_I354_BACKPLANE_1GBPS`.
pub const E1000_DEV_ID_I354_BACKPLANE_1GBPS: u16 = 0x1F40;
/// `E1000_DEV_ID_I354_SGMII`.
pub const E1000_DEV_ID_I354_SGMII: u16 = 0x1F41;
/// `E1000_DEV_ID_I354_BACKPLANE_2_5GBPS`.
pub const E1000_DEV_ID_I354_BACKPLANE_2_5GBPS: u16 = 0x1F45;
/// `E1000_DEV_ID_EP80579_LAN_1`.
pub const E1000_DEV_ID_EP80579_LAN_1: u16 = 0x5040;
/// `E1000_DEV_ID_EP80579_LAN_2`.
pub const E1000_DEV_ID_EP80579_LAN_2: u16 = 0x5044;
/// `E1000_DEV_ID_EP80579_LAN_3`.
pub const E1000_DEV_ID_EP80579_LAN_3: u16 = 0x5048;
/// `E1000_DEV_ID_EP80579_LAN_4`.
pub const E1000_DEV_ID_EP80579_LAN_4: u16 = 0x5041;
/// `E1000_DEV_ID_EP80579_LAN_5`.
pub const E1000_DEV_ID_EP80579_LAN_5: u16 = 0x5045;
/// `E1000_DEV_ID_EP80579_LAN_6`.
pub const E1000_DEV_ID_EP80579_LAN_6: u16 = 0x5049;
/// `NODE_ADDRESS_SIZE`.
pub const NODE_ADDRESS_SIZE: usize = 6;
/// `ETH_LENGTH_OF_ADDRESS`.
pub const ETH_LENGTH_OF_ADDRESS: usize = 6;
/// `MAC_DECODE_SIZE`.
pub const MAC_DECODE_SIZE: u32 = 128 * 1024;
/// `E1000_82542_2_0_REV_ID`.
pub const E1000_82542_2_0_REV_ID: u8 = 2;
/// `E1000_82542_2_1_REV_ID`.
pub const E1000_82542_2_1_REV_ID: u8 = 3;
/// `E1000_REVISION_0`.
pub const E1000_REVISION_0: u32 = 0;
/// `E1000_REVISION_1`.
pub const E1000_REVISION_1: u32 = 1;
/// `E1000_REVISION_2`.
pub const E1000_REVISION_2: u32 = 2;
/// `E1000_REVISION_3`.
pub const E1000_REVISION_3: u32 = 3;
/// `SPEED_10`.
pub const SPEED_10: u16 = 10;
/// `SPEED_100`.
pub const SPEED_100: u16 = 100;
/// `SPEED_1000`.
pub const SPEED_1000: u16 = 1000;
/// `HALF_DUPLEX`.
pub const HALF_DUPLEX: u16 = 1;
/// `FULL_DUPLEX`.
pub const FULL_DUPLEX: u16 = 2;
/// `ENET_HEADER_SIZE`.
pub const ENET_HEADER_SIZE: u32 = 14;
/// `MAXIMUM_ETHERNET_FRAME_SIZE`: With FCS.
pub const MAXIMUM_ETHERNET_FRAME_SIZE: u32 = 1518;
/// `MINIMUM_ETHERNET_FRAME_SIZE`: With FCS.
pub const MINIMUM_ETHERNET_FRAME_SIZE: u32 = 64;
/// `ETHERNET_FCS_SIZE`.
pub const ETHERNET_FCS_SIZE: u32 = 4;
/// `MAXIMUM_ETHERNET_PACKET_SIZE`.
pub const MAXIMUM_ETHERNET_PACKET_SIZE: u32 = MAXIMUM_ETHERNET_FRAME_SIZE - ETHERNET_FCS_SIZE;
/// `MINIMUM_ETHERNET_PACKET_SIZE`.
pub const MINIMUM_ETHERNET_PACKET_SIZE: u32 = MINIMUM_ETHERNET_FRAME_SIZE - ETHERNET_FCS_SIZE;
/// `CRC_LENGTH`.
pub const CRC_LENGTH: u32 = ETHERNET_FCS_SIZE;
/// `MAX_JUMBO_FRAME_SIZE`.
pub const MAX_JUMBO_FRAME_SIZE: u32 = 0x3F00;
/// `VLAN_TAG_SIZE`: 802.3ac tag (not DMAed).
pub const VLAN_TAG_SIZE: u32 = 4;
/// `ETHERNET_IEEE_VLAN_TYPE`: 802.3ac packet.
pub const ETHERNET_IEEE_VLAN_TYPE: u32 = 0x8100;
/// `ETHERNET_IP_TYPE`: IP packets.
pub const ETHERNET_IP_TYPE: u32 = 0x0800;
/// `ETHERNET_ARP_TYPE`: Address Resolution Protocol (ARP).
pub const ETHERNET_ARP_TYPE: u32 = 0x0806;
/// `IP_PROTOCOL_TCP`.
pub const IP_PROTOCOL_TCP: u32 = 6;
/// `IP_PROTOCOL_UDP`.
pub const IP_PROTOCOL_UDP: u32 = 0x11;
/// `POLL_IMS_ENABLE_MASK`.
pub const POLL_IMS_ENABLE_MASK: u32 = E1000_IMS_RXDMT0 | E1000_IMS_RXSEQ;
/// `IMS_ENABLE_MASK`.
pub const IMS_ENABLE_MASK: u32 = E1000_IMS_RXT0
    | E1000_IMS_TXDW
    | E1000_IMS_RXDMT0
    | E1000_IMS_RXSEQ
    | E1000_IMS_RXO
    | E1000_IMS_LSC;
/// `IMS_ICH8LAN_ENABLE_MASK`.
pub const IMS_ICH8LAN_ENABLE_MASK: u32 = E1000_IMS_DSW | E1000_IMS_PHYINT | E1000_IMS_EPRST;
/// `E1000_RAR_ENTRIES`.
pub const E1000_RAR_ENTRIES: u32 = 15;
/// `E1000_RAR_ENTRIES_ICH8LAN`.
pub const E1000_RAR_ENTRIES_ICH8LAN: u32 = 7;
/// `E1000_RAR_ENTRIES_82575`.
pub const E1000_RAR_ENTRIES_82575: u32 = 16;
/// `E1000_RAR_ENTRIES_82576`.
pub const E1000_RAR_ENTRIES_82576: u32 = 24;
/// `E1000_RAR_ENTRIES_82580`.
pub const E1000_RAR_ENTRIES_82580: u32 = 24;
/// `E1000_RAR_ENTRIES_I350`.
pub const E1000_RAR_ENTRIES_I350: u32 = 32;
/// `MIN_NUMBER_OF_DESCRIPTORS`.
pub const MIN_NUMBER_OF_DESCRIPTORS: u32 = 8;
/// `MAX_NUMBER_OF_DESCRIPTORS`.
pub const MAX_NUMBER_OF_DESCRIPTORS: u32 = 0xFFF8;
/// `MAX_PS_BUFFERS`.
pub const MAX_PS_BUFFERS: usize = 4;
/// `E1000_RXD_STAT_DD`: Descriptor Done.
pub const E1000_RXD_STAT_DD: u32 = 0x01;
/// `E1000_RXD_STAT_EOP`: End of Packet.
pub const E1000_RXD_STAT_EOP: u32 = 0x02;
/// `E1000_RXD_STAT_IXSM`: Ignore checksum.
pub const E1000_RXD_STAT_IXSM: u32 = 0x04;
/// `E1000_RXD_STAT_VP`: IEEE VLAN Packet.
pub const E1000_RXD_STAT_VP: u32 = 0x08;
/// `E1000_RXD_STAT_UDPCS`: UDP xsum calculated.
pub const E1000_RXD_STAT_UDPCS: u32 = 0x10;
/// `E1000_RXD_STAT_TCPCS`: TCP xsum calculated.
pub const E1000_RXD_STAT_TCPCS: u32 = 0x20;
/// `E1000_RXD_STAT_IPCS`: IP xsum calculated.
pub const E1000_RXD_STAT_IPCS: u32 = 0x40;
/// `E1000_RXD_STAT_PIF`: passed in-exact filter.
pub const E1000_RXD_STAT_PIF: u32 = 0x80;
/// `E1000_RXD_STAT_IPIDV`: IP identification valid.
pub const E1000_RXD_STAT_IPIDV: u32 = 0x200;
/// `E1000_RXD_STAT_UDPV`: Valid UDP checksum.
pub const E1000_RXD_STAT_UDPV: u32 = 0x400;
/// `E1000_RXD_STAT_ACK`: ACK Packet indication.
pub const E1000_RXD_STAT_ACK: u32 = 0x8000;
/// `E1000_RXD_STAT_STRIPCRC`: CRC has been stripped.
pub const E1000_RXD_STAT_STRIPCRC: u32 = 0x1000;
/// `E1000_RXD_ERR_CE`: CRC Error.
pub const E1000_RXD_ERR_CE: u32 = 0x01;
/// `E1000_RXD_ERR_SE`: Symbol Error.
pub const E1000_RXD_ERR_SE: u32 = 0x02;
/// `E1000_RXD_ERR_SEQ`: Sequence Error.
pub const E1000_RXD_ERR_SEQ: u32 = 0x04;
/// `E1000_RXD_ERR_CXE`: Carrier Extension Error.
pub const E1000_RXD_ERR_CXE: u32 = 0x10;
/// `E1000_RXD_ERR_TCPE`: TCP/UDP Checksum Error.
pub const E1000_RXD_ERR_TCPE: u32 = 0x20;
/// `E1000_RXD_ERR_IPE`: IP Checksum Error.
pub const E1000_RXD_ERR_IPE: u32 = 0x40;
/// `E1000_RXD_ERR_RXE`: Rx Data Error.
pub const E1000_RXD_ERR_RXE: u32 = 0x80;
/// `E1000_RXD_SPC_VLAN_MASK`: VLAN ID is in lower 12 bits.
pub const E1000_RXD_SPC_VLAN_MASK: u32 = 0x0FFF;
/// `E1000_RXD_SPC_PRI_MASK`: Priority is in upper 3 bits.
pub const E1000_RXD_SPC_PRI_MASK: u32 = 0xE000;
/// `E1000_RXD_SPC_PRI_SHIFT`.
pub const E1000_RXD_SPC_PRI_SHIFT: u32 = 13;
/// `E1000_RXD_SPC_CFI_MASK`: CFI is bit 12.
pub const E1000_RXD_SPC_CFI_MASK: u32 = 0x1000;
/// `E1000_RXD_SPC_CFI_SHIFT`.
pub const E1000_RXD_SPC_CFI_SHIFT: u32 = 12;
/// `E1000_RXDEXT_STATERR_CE`.
pub const E1000_RXDEXT_STATERR_CE: u32 = 0x01000000;
/// `E1000_RXDEXT_STATERR_SE`.
pub const E1000_RXDEXT_STATERR_SE: u32 = 0x02000000;
/// `E1000_RXDEXT_STATERR_SEQ`.
pub const E1000_RXDEXT_STATERR_SEQ: u32 = 0x04000000;
/// `E1000_RXDEXT_STATERR_CXE`.
pub const E1000_RXDEXT_STATERR_CXE: u32 = 0x10000000;
/// `E1000_RXDEXT_STATERR_TCPE`.
pub const E1000_RXDEXT_STATERR_TCPE: u32 = 0x20000000;
/// `E1000_RXDEXT_STATERR_IPE`.
pub const E1000_RXDEXT_STATERR_IPE: u32 = 0x40000000;
/// `E1000_RXDEXT_STATERR_RXE`.
pub const E1000_RXDEXT_STATERR_RXE: u32 = 0x80000000;
/// `E1000_RXDPS_HDRSTAT_HDRSP`.
pub const E1000_RXDPS_HDRSTAT_HDRSP: u32 = 0x00008000;
/// `E1000_RXDPS_HDRSTAT_HDRLEN_MASK`.
pub const E1000_RXDPS_HDRSTAT_HDRLEN_MASK: u32 = 0x000003FF;
/// `E1000_RXD_ERR_FRAME_ERR_MASK`.
pub const E1000_RXD_ERR_FRAME_ERR_MASK: u32 =
    E1000_RXD_ERR_CE | E1000_RXD_ERR_SE | E1000_RXD_ERR_SEQ | E1000_RXD_ERR_CXE | E1000_RXD_ERR_RXE;
/// `E1000_RXDEXT_ERR_FRAME_ERR_MASK`.
pub const E1000_RXDEXT_ERR_FRAME_ERR_MASK: u32 = E1000_RXDEXT_STATERR_CE
    | E1000_RXDEXT_STATERR_SE
    | E1000_RXDEXT_STATERR_SEQ
    | E1000_RXDEXT_STATERR_CXE
    | E1000_RXDEXT_STATERR_RXE;
/// `E1000_TXD_DTYP_D`: Data Descriptor.
pub const E1000_TXD_DTYP_D: u32 = 0x00100000;
/// `E1000_TXD_DTYP_C`: Context Descriptor.
pub const E1000_TXD_DTYP_C: u32 = 0x00000000;
/// `E1000_TXD_POPTS_IXSM`: Insert IP checksum.
pub const E1000_TXD_POPTS_IXSM: u32 = 0x01;
/// `E1000_TXD_POPTS_TXSM`: Insert TCP/UDP checksum.
pub const E1000_TXD_POPTS_TXSM: u32 = 0x02;
/// `E1000_TXD_CMD_EOP`: End of Packet.
pub const E1000_TXD_CMD_EOP: u32 = 0x01000000;
/// `E1000_TXD_CMD_IFCS`: Insert FCS (Ethernet CRC).
pub const E1000_TXD_CMD_IFCS: u32 = 0x02000000;
/// `E1000_TXD_CMD_IC`: Insert Checksum.
pub const E1000_TXD_CMD_IC: u32 = 0x04000000;
/// `E1000_TXD_CMD_RS`: Report Status.
pub const E1000_TXD_CMD_RS: u32 = 0x08000000;
/// `E1000_TXD_CMD_RPS`: Report Packet Sent.
pub const E1000_TXD_CMD_RPS: u32 = 0x10000000;
/// `E1000_TXD_CMD_DEXT`: Descriptor extension (0 = legacy).
pub const E1000_TXD_CMD_DEXT: u32 = 0x20000000;
/// `E1000_TXD_CMD_VLE`: Add VLAN tag.
pub const E1000_TXD_CMD_VLE: u32 = 0x40000000;
/// `E1000_TXD_CMD_IDE`: Enable Tidv register.
pub const E1000_TXD_CMD_IDE: u32 = 0x80000000;
/// `E1000_TXD_STAT_DD`: Descriptor Done.
pub const E1000_TXD_STAT_DD: u32 = 0x00000001;
/// `E1000_TXD_STAT_EC`: Excess Collisions.
pub const E1000_TXD_STAT_EC: u32 = 0x00000002;
/// `E1000_TXD_STAT_LC`: Late Collisions.
pub const E1000_TXD_STAT_LC: u32 = 0x00000004;
/// `E1000_TXD_STAT_TU`: Transmit underrun.
pub const E1000_TXD_STAT_TU: u32 = 0x00000008;
/// `E1000_TXD_CMD_TCP`: TCP packet.
pub const E1000_TXD_CMD_TCP: u32 = 0x01000000;
/// `E1000_TXD_CMD_IP`: IP packet.
pub const E1000_TXD_CMD_IP: u32 = 0x02000000;
/// `E1000_TXD_CMD_TSE`: TCP Seg enable.
pub const E1000_TXD_CMD_TSE: u32 = 0x04000000;
/// `E1000_TXD_STAT_TC`: Tx Underrun.
pub const E1000_TXD_STAT_TC: u32 = 0x00000004;
/// `E1000_NUM_UNICAST`: Unicast filter entries.
pub const E1000_NUM_UNICAST: u32 = 16;
/// `E1000_MC_TBL_SIZE`: Multicast Filter Table (4096 bits).
pub const E1000_MC_TBL_SIZE: u32 = 128;
/// `E1000_VLAN_FILTER_TBL_SIZE`: VLAN Filter Table (4096 bits).
pub const E1000_VLAN_FILTER_TBL_SIZE: u32 = 128;
/// `E1000_NUM_UNICAST_ICH8LAN`.
pub const E1000_NUM_UNICAST_ICH8LAN: u32 = 7;
/// `E1000_MC_TBL_SIZE_ICH8LAN`.
pub const E1000_MC_TBL_SIZE_ICH8LAN: u32 = 32;
/// `E1000_NUM_MTA_REGISTERS`.
pub const E1000_NUM_MTA_REGISTERS: u32 = 128;
/// `E1000_NUM_MTA_REGISTERS_ICH8LAN`.
pub const E1000_NUM_MTA_REGISTERS_ICH8LAN: u32 = 32;
/// `E1000_WAKEUP_IP_ADDRESS_COUNT_MAX`.
pub const E1000_WAKEUP_IP_ADDRESS_COUNT_MAX: u32 = 4;
/// `E1000_IP4AT_SIZE`.
pub const E1000_IP4AT_SIZE: u32 = E1000_WAKEUP_IP_ADDRESS_COUNT_MAX;
/// `E1000_IP4AT_SIZE_ICH8LAN`.
pub const E1000_IP4AT_SIZE_ICH8LAN: u32 = 3;
/// `E1000_IP6AT_SIZE`.
pub const E1000_IP6AT_SIZE: u32 = 1;
/// `E1000_FLEXIBLE_FILTER_COUNT_MAX`.
pub const E1000_FLEXIBLE_FILTER_COUNT_MAX: u32 = 4;
/// `E1000_FLEXIBLE_FILTER_SIZE_MAX`.
pub const E1000_FLEXIBLE_FILTER_SIZE_MAX: u32 = 128;
/// `E1000_FFLT_SIZE`.
pub const E1000_FFLT_SIZE: u32 = E1000_FLEXIBLE_FILTER_COUNT_MAX;
/// `E1000_FFMT_SIZE`.
pub const E1000_FFMT_SIZE: u32 = E1000_FLEXIBLE_FILTER_SIZE_MAX;
/// `E1000_FFVT_SIZE`.
pub const E1000_FFVT_SIZE: u32 = E1000_FLEXIBLE_FILTER_SIZE_MAX;
/// `E1000_DISABLE_SERDES_LOOPBACK`.
pub const E1000_DISABLE_SERDES_LOOPBACK: u32 = 0x0400;
/// `E1000_CTRL`: Device Control - RW.
pub const E1000_CTRL: u32 = 0x00000;
/// `E1000_CTRL_DUP`: Device Control Duplicate (Shadow) - RW.
pub const E1000_CTRL_DUP: u32 = 0x00004;
/// `E1000_STATUS`: Device Status - RO.
pub const E1000_STATUS: u32 = 0x00008;
/// `E1000_EECD`: EEPROM/Flash Control - RW.
pub const E1000_EECD: u32 = 0x00010;
/// `E1000_EERD`: EEPROM Read - RW.
pub const E1000_EERD: u32 = 0x00014;
/// `E1000_CTRL_EXT`: Extended Device Control - RW.
pub const E1000_CTRL_EXT: u32 = 0x00018;
/// `E1000_FLA`: Flash Access - RW.
pub const E1000_FLA: u32 = 0x0001C;
/// `E1000_MDIC`: MDI Control - RW.
pub const E1000_MDIC: u32 = 0x00020;
/// `E1000_MDICNFG`: MDI Config - RW.
pub const E1000_MDICNFG: u32 = 0x00E04;
/// `E1000_SCTL`: SerDes Control - RW.
pub const E1000_SCTL: u32 = 0x00024;
/// `E1000_FEXTNVM`: Future Extended NVM register.
pub const E1000_FEXTNVM: u32 = 0x00028;
/// `E1000_FEXTNVM3`: Future Extended NVM 3 - RW.
pub const E1000_FEXTNVM3: u32 = 0x0003C;
/// `E1000_FEXTNVM4`: Future Extended NVM 4 - RW.
pub const E1000_FEXTNVM4: u32 = 0x00024;
/// `E1000_FEXTNVM6`: Future Extended NVM 6 - RW.
pub const E1000_FEXTNVM6: u32 = 0x00010;
/// `E1000_FEXTNVM12`: Future Extended NVM 12 - RW.
pub const E1000_FEXTNVM12: u32 = 0x5BC0;
/// `E1000_FCAL`: Flow Control Address Low - RW.
pub const E1000_FCAL: u32 = 0x00028;
/// `E1000_FCAH`: Flow Control Address High -RW.
pub const E1000_FCAH: u32 = 0x0002C;
/// `E1000_FCT`: Flow Control Type - RW.
pub const E1000_FCT: u32 = 0x00030;
/// `E1000_CONNSW`: Copper/Fiber switch control - RW.
pub const E1000_CONNSW: u32 = 0x00034;
/// `E1000_VET`: VLAN Ether Type - RW.
pub const E1000_VET: u32 = 0x00038;
/// `E1000_ICR`: Interrupt Cause Read - R/clr.
pub const E1000_ICR: u32 = 0x000C0;
/// `E1000_ITR`: Interrupt Throttling Rate - RW.
pub const E1000_ITR: u32 = 0x000C4;
/// `E1000_ICS`: Interrupt Cause Set - WO.
pub const E1000_ICS: u32 = 0x000C8;
/// `E1000_IMS`: Interrupt Mask Set - RW.
pub const E1000_IMS: u32 = 0x000D0;
/// `E1000_IMC`: Interrupt Mask Clear - WO.
pub const E1000_IMC: u32 = 0x000D8;
/// `E1000_IAM`: Interrupt Acknowledge Auto Mask.
pub const E1000_IAM: u32 = 0x000E0;
/// `E1000_RCTL`: RX Control - RW.
pub const E1000_RCTL: u32 = 0x00100;
/// `E1000_GPIE`: General Purpose Interrupt Enable - RW.
pub const E1000_GPIE: u32 = 0x01514;
/// `E1000_EICS`: Ext. Interrupt Cause Set - W0.
pub const E1000_EICS: u32 = 0x01520;
/// `E1000_EIMS`: Ext. Interrupt Mask Set/Read - RW.
pub const E1000_EIMS: u32 = 0x01524;
/// `E1000_EIMC`: Ext. Interrupt Mask Clear - WO.
pub const E1000_EIMC: u32 = 0x01528;
/// `E1000_EIAC`: Ext. Interrupt Auto Clear - RW.
pub const E1000_EIAC: u32 = 0x0152C;
/// `E1000_EIAM`: Ext. Interrupt Ack Auto Clear Mask - RW.
pub const E1000_EIAM: u32 = 0x01530;
/// `E1000_EICR`: Ext. Interrupt Cause Read - R/clr.
pub const E1000_EICR: u32 = 0x01580;
/// `E1000_EITR(_n)`: extended interrupt throttle rate of vector `n`.
pub const fn e1000_eitr(n: u32) -> u32 {
    0x01680 + (0x4 * n)
}
/// `E1000_IVAR0`: Interrupt Vector Allocation (array) - RW.
pub const E1000_IVAR0: u32 = 0x01700;
/// `E1000_IVAR_MISC`: IVAR for "other" causes - RW.
pub const E1000_IVAR_MISC: u32 = 0x01740;
/// `E1000_RDTR1`: RX Delay Timer (1) - RW.
pub const E1000_RDTR1: u32 = 0x02820;
/// `E1000_RDBAL1`: RX Descriptor Base Address Low (1) - RW.
pub const E1000_RDBAL1: u32 = 0x02900;
/// `E1000_RDBAH1`: RX Descriptor Base Address High (1) - RW.
pub const E1000_RDBAH1: u32 = 0x02904;
/// `E1000_RDLEN1`: RX Descriptor Length (1) - RW.
pub const E1000_RDLEN1: u32 = 0x02908;
/// `E1000_FCTTV`: Flow Control Transmit Timer Value - RW.
pub const E1000_FCTTV: u32 = 0x00170;
/// `E1000_TXCW`: TX Configuration Word - RW.
pub const E1000_TXCW: u32 = 0x00178;
/// `E1000_RXCW`: RX Configuration Word - RO.
pub const E1000_RXCW: u32 = 0x00180;
/// `E1000_TCTL`: TX Control - RW.
pub const E1000_TCTL: u32 = 0x00400;
/// `E1000_TCTL_EXT`: Extended TX Control - RW.
pub const E1000_TCTL_EXT: u32 = 0x00404;
/// `E1000_TIPG`: TX Inter-packet gap -RW.
pub const E1000_TIPG: u32 = 0x00410;
/// `E1000_TBT`: TX Burst Timer - RW.
pub const E1000_TBT: u32 = 0x00448;
/// `E1000_AIT`: Adaptive Interframe Spacing Throttle - RW.
pub const E1000_AIT: u32 = 0x00458;
/// `E1000_LEDCTL`: LED Control - RW.
pub const E1000_LEDCTL: u32 = 0x00E00;
/// `E1000_EXTCNF_CTRL`: Extended Configuration Control.
pub const E1000_EXTCNF_CTRL: u32 = 0x00F00;
/// `E1000_EXTCNF_SIZE`: Extended Configuration Size.
pub const E1000_EXTCNF_SIZE: u32 = 0x00F08;
/// `E1000_PHY_CTRL`: PHY Control Register in CSR.
pub const E1000_PHY_CTRL: u32 = 0x00F10;
/// `FEXTNVM_SW_CONFIG`.
pub const FEXTNVM_SW_CONFIG: u32 = 1;
/// `FEXTNVM_SW_CONFIG_ICH8M`: Bit redefined for ICH8M :/.
pub const FEXTNVM_SW_CONFIG_ICH8M: u32 = 1 << 27;
/// `E1000_PBA`: Packet Buffer Allocation - RW.
pub const E1000_PBA: u32 = 0x01000;
/// `E1000_PBS`: Packet Buffer Size.
pub const E1000_PBS: u32 = 0x01008;
/// `E1000_IOSFPC`: TX corrupted data.
pub const E1000_IOSFPC: u32 = 0x00F28;
/// `E1000_EEMNGCTL`: MNG EEprom Control.
pub const E1000_EEMNGCTL: u32 = 0x01010;
/// `E1000_FLASH_UPDATES`.
pub const E1000_FLASH_UPDATES: u32 = 1000;
/// `E1000_EEARBC`: EEPROM Auto Read Bus Control.
pub const E1000_EEARBC: u32 = 0x01024;
/// `E1000_FLASHT`: FLASH Timer Register.
pub const E1000_FLASHT: u32 = 0x01028;
/// `E1000_EEWR`: EEPROM Write Register - RW.
pub const E1000_EEWR: u32 = 0x0102C;
/// `E1000_FLSWCTL`: FLASH control register.
pub const E1000_FLSWCTL: u32 = 0x01030;
/// `E1000_FLSWDATA`: FLASH data register.
pub const E1000_FLSWDATA: u32 = 0x01034;
/// `E1000_FLSWCNT`: FLASH Access Counter.
pub const E1000_FLSWCNT: u32 = 0x01038;
/// `E1000_FLOP`: FLASH Opcode Register.
pub const E1000_FLOP: u32 = 0x0103C;
/// `E1000_I2CCMD`: SFPI2C Command Register - RW.
pub const E1000_I2CCMD: u32 = 0x01028;
/// `E1000_ERT`: Early Rx Threshold - RW.
pub const E1000_ERT: u32 = 0x02008;
/// `E1000_FCRTL`: Flow Control Receive Threshold Low - RW.
pub const E1000_FCRTL: u32 = 0x02160;
/// `E1000_FCRTH`: Flow Control Receive Threshold High - RW.
pub const E1000_FCRTH: u32 = 0x02168;
/// `E1000_PSRCTL`: Packet Split Receive Control - RW.
pub const E1000_PSRCTL: u32 = 0x02170;
/// `E1000_RDBAL(_n)`: RX descriptor base address low - RW.
pub const fn e1000_rdbal(n: u32) -> u32 {
    if n < 4 {
        0x02800 + (n * 0x100)
    } else {
        0x0C000 + (n * 0x40)
    }
}
/// `E1000_RDBAH(_n)`: RX descriptor base address high - RW.
pub const fn e1000_rdbah(n: u32) -> u32 {
    if n < 4 {
        0x02804 + (n * 0x100)
    } else {
        0x0C004 + (n * 0x40)
    }
}
/// `E1000_RDLEN(_n)`: RX descriptor length - RW.
pub const fn e1000_rdlen(n: u32) -> u32 {
    if n < 4 {
        0x02808 + (n * 0x100)
    } else {
        0x0C008 + (n * 0x40)
    }
}
/// `E1000_SRRCTL(_n)`: split and replication receive control - RW.
pub const fn e1000_srrctl(n: u32) -> u32 {
    if n < 4 {
        0x0280C + (n * 0x100)
    } else {
        0x0C00C + (n * 0x40)
    }
}
/// `E1000_RDH(_n)`: RX descriptor head - RW.
pub const fn e1000_rdh(n: u32) -> u32 {
    if n < 4 {
        0x02810 + (n * 0x100)
    } else {
        0x0C010 + (n * 0x40)
    }
}
/// `E1000_RDT(_n)`: RX descriptor tail - RW.
pub const fn e1000_rdt(n: u32) -> u32 {
    if n < 4 {
        0x02818 + (n * 0x100)
    } else {
        0x0C018 + (n * 0x40)
    }
}
/// `E1000_RDTR`: RX Delay Timer - RW.
pub const E1000_RDTR: u32 = 0x02820;
/// `E1000_RDBAL0`: an alias of `E1000_RDBAL`, RX desc base address low (0) - RW.
pub const fn e1000_rdbal0(n: u32) -> u32 {
    e1000_rdbal(n)
}
/// `E1000_RDBAH0`: an alias of `E1000_RDBAH`, RX desc base address high (0) - RW.
pub const fn e1000_rdbah0(n: u32) -> u32 {
    e1000_rdbah(n)
}
/// `E1000_RDLEN0`: an alias of `E1000_RDLEN`, RX desc length (0) - RW.
pub const fn e1000_rdlen0(n: u32) -> u32 {
    e1000_rdlen(n)
}
/// `E1000_RDTR0`: RX Delay Timer (0) - RW.
pub const E1000_RDTR0: u32 = E1000_RDTR;
/// `E1000_RXDCTL(_n)`: receive descriptor control.
pub const fn e1000_rxdctl(n: u32) -> u32 {
    if n < 4 {
        0x02828 + (n * 0x100)
    } else {
        0x0C028 + (n * 0x40)
    }
}
/// `E1000_RADV`: RX Interrupt Absolute Delay Timer - RW.
pub const E1000_RADV: u32 = 0x0282C;
/// `E1000_RSRPD`: RX Small Packet Detect - RW.
pub const E1000_RSRPD: u32 = 0x02C00;
/// `E1000_RAID`: Receive Ack Interrupt Delay - RW.
pub const E1000_RAID: u32 = 0x02C08;
/// `E1000_TXDMAC`: TX DMA Control - RW.
pub const E1000_TXDMAC: u32 = 0x03000;
/// `E1000_KABGTXD`: AFE Band Gap Transmit Ref Data.
pub const E1000_KABGTXD: u32 = 0x03004;
/// `E1000_TDFH`: TX Data FIFO Head - RW.
pub const E1000_TDFH: u32 = 0x03410;
/// `E1000_TDFT`: TX Data FIFO Tail - RW.
pub const E1000_TDFT: u32 = 0x03418;
/// `E1000_TDFHS`: TX Data FIFO Head Saved - RW.
pub const E1000_TDFHS: u32 = 0x03420;
/// `E1000_TDFTS`: TX Data FIFO Tail Saved - RW.
pub const E1000_TDFTS: u32 = 0x03428;
/// `E1000_TDFPC`: TX Data FIFO Packet Count - RW.
pub const E1000_TDFPC: u32 = 0x03430;
/// `E1000_TDBAL(_n)`: TX descriptor base address low - RW.
pub const fn e1000_tdbal(n: u32) -> u32 {
    if n < 4 {
        0x03800 + (n * 0x100)
    } else {
        0x0E000 + (n * 0x40)
    }
}
/// `E1000_TDBAH(_n)`: TX descriptor base address high - RW.
pub const fn e1000_tdbah(n: u32) -> u32 {
    if n < 4 {
        0x03804 + (n * 0x100)
    } else {
        0x0E004 + (n * 0x40)
    }
}
/// `E1000_TDLEN(_n)`: TX descriptor length - RW.
pub const fn e1000_tdlen(n: u32) -> u32 {
    if n < 4 {
        0x03808 + (n * 0x100)
    } else {
        0x0E008 + (n * 0x40)
    }
}
/// `E1000_TDH(_n)`: TX descriptor head - RW.
pub const fn e1000_tdh(n: u32) -> u32 {
    if n < 4 {
        0x03810 + (n * 0x100)
    } else {
        0x0E010 + (n * 0x40)
    }
}
/// `E1000_TDT(_n)`: TX descriptor tail - RW.
pub const fn e1000_tdt(n: u32) -> u32 {
    if n < 4 {
        0x03818 + (n * 0x100)
    } else {
        0x0E018 + (n * 0x40)
    }
}
/// `E1000_TIDV`: TX Interrupt Delay Value - RW.
pub const E1000_TIDV: u32 = 0x03820;
/// `E1000_TXDCTL(_n)`: transmit descriptor control.
pub const fn e1000_txdctl(n: u32) -> u32 {
    if n < 4 {
        0x03828 + (n * 0x100)
    } else {
        0x0E028 + (n * 0x40)
    }
}
/// `E1000_TADV`: TX Interrupt Absolute Delay Val - RW.
pub const E1000_TADV: u32 = 0x0382C;
/// `E1000_TSPMT`: TCP Segmentation PAD & Min Threshold - RW.
pub const E1000_TSPMT: u32 = 0x03830;
/// `E1000_TARC0`: TX Arbitration Count (0).
pub const E1000_TARC0: u32 = 0x03840;
/// `E1000_TDBAL1`: TX Desc Base Address Low (1) - RW.
pub const E1000_TDBAL1: u32 = 0x03900;
/// `E1000_TDBAH1`: TX Desc Base Address High (1) - RW.
pub const E1000_TDBAH1: u32 = 0x03904;
/// `E1000_TDLEN1`: TX Desc Length (1) - RW.
pub const E1000_TDLEN1: u32 = 0x03908;
/// `E1000_TDH1`: TX Desc Head (1) - RW.
pub const E1000_TDH1: u32 = 0x03910;
/// `E1000_TDT1`: TX Desc Tail (1) - RW.
pub const E1000_TDT1: u32 = 0x03918;
/// `E1000_TARC1`: TX Arbitration Count (1).
pub const E1000_TARC1: u32 = 0x03940;
/// `E1000_CRCERRS`: CRC Error Count - R/clr.
pub const E1000_CRCERRS: u32 = 0x04000;
/// `E1000_ALGNERRC`: Alignment Error Count - R/clr.
pub const E1000_ALGNERRC: u32 = 0x04004;
/// `E1000_SYMERRS`: Symbol Error Count - R/clr.
pub const E1000_SYMERRS: u32 = 0x04008;
/// `E1000_RXERRC`: Receive Error Count - R/clr.
pub const E1000_RXERRC: u32 = 0x0400C;
/// `E1000_MPC`: Missed Packet Count - R/clr.
pub const E1000_MPC: u32 = 0x04010;
/// `E1000_SCC`: Single Collision Count - R/clr.
pub const E1000_SCC: u32 = 0x04014;
/// `E1000_ECOL`: Excessive Collision Count - R/clr.
pub const E1000_ECOL: u32 = 0x04018;
/// `E1000_MCC`: Multiple Collision Count - R/clr.
pub const E1000_MCC: u32 = 0x0401C;
/// `E1000_LATECOL`: Late Collision Count - R/clr.
pub const E1000_LATECOL: u32 = 0x04020;
/// `E1000_COLC`: Collision Count - R/clr.
pub const E1000_COLC: u32 = 0x04028;
/// `E1000_DC`: Defer Count - R/clr.
pub const E1000_DC: u32 = 0x04030;
/// `E1000_TNCRS`: TX-No CRS - R/clr.
pub const E1000_TNCRS: u32 = 0x04034;
/// `E1000_SEC`: Sequence Error Count - R/clr.
pub const E1000_SEC: u32 = 0x04038;
/// `E1000_CEXTERR`: Carrier Extension Error Count - R/clr.
pub const E1000_CEXTERR: u32 = 0x0403C;
/// `E1000_RLEC`: Receive Length Error Count - R/clr.
pub const E1000_RLEC: u32 = 0x04040;
/// `E1000_XONRXC`: XON RX Count - R/clr.
pub const E1000_XONRXC: u32 = 0x04048;
/// `E1000_XONTXC`: XON TX Count - R/clr.
pub const E1000_XONTXC: u32 = 0x0404C;
/// `E1000_XOFFRXC`: XOFF RX Count - R/clr.
pub const E1000_XOFFRXC: u32 = 0x04050;
/// `E1000_XOFFTXC`: XOFF TX Count - R/clr.
pub const E1000_XOFFTXC: u32 = 0x04054;
/// `E1000_FCRUC`: Flow Control RX Unsupported Count- R/clr.
pub const E1000_FCRUC: u32 = 0x04058;
/// `E1000_PRC64`: Packets RX (64 bytes) - R/clr.
pub const E1000_PRC64: u32 = 0x0405C;
/// `E1000_PRC127`: Packets RX (65-127 bytes) - R/clr.
pub const E1000_PRC127: u32 = 0x04060;
/// `E1000_PRC255`: Packets RX (128-255 bytes) - R/clr.
pub const E1000_PRC255: u32 = 0x04064;
/// `E1000_PRC511`: Packets RX (255-511 bytes) - R/clr.
pub const E1000_PRC511: u32 = 0x04068;
/// `E1000_PRC1023`: Packets RX (512-1023 bytes) - R/clr.
pub const E1000_PRC1023: u32 = 0x0406C;
/// `E1000_PRC1522`: Packets RX (1024-1522 bytes) - R/clr.
pub const E1000_PRC1522: u32 = 0x04070;
/// `E1000_GPRC`: Good Packets RX Count - R/clr.
pub const E1000_GPRC: u32 = 0x04074;
/// `E1000_BPRC`: Broadcast Packets RX Count - R/clr.
pub const E1000_BPRC: u32 = 0x04078;
/// `E1000_MPRC`: Multicast Packets RX Count - R/clr.
pub const E1000_MPRC: u32 = 0x0407C;
/// `E1000_GPTC`: Good Packets TX Count - R/clr.
pub const E1000_GPTC: u32 = 0x04080;
/// `E1000_GORCL`: Good Octets RX Count Low - R/clr.
pub const E1000_GORCL: u32 = 0x04088;
/// `E1000_GORCH`: Good Octets RX Count High - R/clr.
pub const E1000_GORCH: u32 = 0x0408C;
/// `E1000_GOTCL`: Good Octets TX Count Low - R/clr.
pub const E1000_GOTCL: u32 = 0x04090;
/// `E1000_GOTCH`: Good Octets TX Count High - R/clr.
pub const E1000_GOTCH: u32 = 0x04094;
/// `E1000_RNBC`: RX No Buffers Count - R/clr.
pub const E1000_RNBC: u32 = 0x040A0;
/// `E1000_RUC`: RX Undersize Count - R/clr.
pub const E1000_RUC: u32 = 0x040A4;
/// `E1000_RFC`: RX Fragment Count - R/clr.
pub const E1000_RFC: u32 = 0x040A8;
/// `E1000_ROC`: RX Oversize Count - R/clr.
pub const E1000_ROC: u32 = 0x040AC;
/// `E1000_RJC`: RX Jabber Count - R/clr.
pub const E1000_RJC: u32 = 0x040B0;
/// `E1000_MGTPRC`: Management Packets RX Count - R/clr.
pub const E1000_MGTPRC: u32 = 0x040B4;
/// `E1000_MGTPDC`: Management Packets Dropped Count - R/clr.
pub const E1000_MGTPDC: u32 = 0x040B8;
/// `E1000_MGTPTC`: Management Packets TX Count - R/clr.
pub const E1000_MGTPTC: u32 = 0x040BC;
/// `E1000_TORL`: Total Octets RX Low - R/clr.
pub const E1000_TORL: u32 = 0x040C0;
/// `E1000_TORH`: Total Octets RX High - R/clr.
pub const E1000_TORH: u32 = 0x040C4;
/// `E1000_TOTL`: Total Octets TX Low - R/clr.
pub const E1000_TOTL: u32 = 0x040C8;
/// `E1000_TOTH`: Total Octets TX High - R/clr.
pub const E1000_TOTH: u32 = 0x040CC;
/// `E1000_TPR`: Total Packets RX - R/clr.
pub const E1000_TPR: u32 = 0x040D0;
/// `E1000_TPT`: Total Packets TX - R/clr.
pub const E1000_TPT: u32 = 0x040D4;
/// `E1000_PTC64`: Packets TX (64 bytes) - R/clr.
pub const E1000_PTC64: u32 = 0x040D8;
/// `E1000_PTC127`: Packets TX (65-127 bytes) - R/clr.
pub const E1000_PTC127: u32 = 0x040DC;
/// `E1000_PTC255`: Packets TX (128-255 bytes) - R/clr.
pub const E1000_PTC255: u32 = 0x040E0;
/// `E1000_PTC511`: Packets TX (256-511 bytes) - R/clr.
pub const E1000_PTC511: u32 = 0x040E4;
/// `E1000_PTC1023`: Packets TX (512-1023 bytes) - R/clr.
pub const E1000_PTC1023: u32 = 0x040E8;
/// `E1000_PTC1522`: Packets TX (1024-1522 Bytes) - R/clr.
pub const E1000_PTC1522: u32 = 0x040EC;
/// `E1000_MPTC`: Multicast Packets TX Count - R/clr.
pub const E1000_MPTC: u32 = 0x040F0;
/// `E1000_BPTC`: Broadcast Packets TX Count - R/clr.
pub const E1000_BPTC: u32 = 0x040F4;
/// `E1000_TSCTC`: TCP Segmentation Context TX - R/clr.
pub const E1000_TSCTC: u32 = 0x040F8;
/// `E1000_TSCTFC`: TCP Segmentation Context TX Fail - R/clr.
pub const E1000_TSCTFC: u32 = 0x040FC;
/// `E1000_IAC`: Interrupt Assertion Count.
pub const E1000_IAC: u32 = 0x04100;
/// `E1000_RPTHC`: CONFLICT Rx Packets to Host Count.
pub const E1000_RPTHC: u32 = 0x04104;
/// `E1000_ICRXPTC`: Interrupt Cause Rx Packet Timer Expire Count.
pub const E1000_ICRXPTC: u32 = 0x04104;
/// `E1000_ICRXATC`: Interrupt Cause Rx Absolute Timer Expire Count.
pub const E1000_ICRXATC: u32 = 0x04108;
/// `E1000_ICTXPTC`: Interrupt Cause Tx Packet Timer Expire Count.
pub const E1000_ICTXPTC: u32 = 0x0410C;
/// `E1000_ICTXATC`: Interrupt Cause Tx Absolute Timer Expire Count.
pub const E1000_ICTXATC: u32 = 0x04110;
/// `E1000_ICTXQEC`: Interrupt Cause Tx Queue Empty Count.
pub const E1000_ICTXQEC: u32 = 0x04118;
/// `E1000_ICTXQMTC`: Interrupt Cause Tx Queue Minimum Threshold Count.
pub const E1000_ICTXQMTC: u32 = 0x0411C;
/// `E1000_ICRXDMTC`: Interrupt Cause Rx Descriptor Minimum Threshold Count.
pub const E1000_ICRXDMTC: u32 = 0x04120;
/// `E1000_ICRXOC`: Interrupt Cause Receiver Overrun Count.
pub const E1000_ICRXOC: u32 = 0x04124;
/// `E1000_SDPC`: Switch Drop Packet Count.
pub const E1000_SDPC: u32 = 0x041A4;
/// `E1000_PCS_CFG0`: PCS Configuration 0 - RW.
pub const E1000_PCS_CFG0: u32 = 0x04200;
/// `E1000_PCS_LCTL`: PCS Link Control - RW.
pub const E1000_PCS_LCTL: u32 = 0x04208;
/// `E1000_PCS_LSTAT`: PCS Link Status - RO.
pub const E1000_PCS_LSTAT: u32 = 0x0420C;
/// `E1000_RXCSUM`: RX Checksum Control - RW.
pub const E1000_RXCSUM: u32 = 0x05000;
/// `E1000_RFCTL`: Receive Filter Control.
pub const E1000_RFCTL: u32 = 0x05008;
/// `E1000_MTA`: Multicast Table Array - RW Array.
pub const E1000_MTA: u32 = 0x05200;
/// `E1000_RA`: Receive Address - RW Array.
pub const E1000_RA: u32 = 0x05400;
/// `E1000_VFTA`: VLAN Filter Table Array - RW Array.
pub const E1000_VFTA: u32 = 0x05600;
/// `E1000_WUC`: Wakeup Control - RW.
pub const E1000_WUC: u32 = 0x05800;
/// `E1000_WUFC`: Wakeup Filter Control - RW.
pub const E1000_WUFC: u32 = 0x05808;
/// `E1000_WUS`: Wakeup Status - RO.
pub const E1000_WUS: u32 = 0x05810;
/// `E1000_MANC`: Management Control - RW.
pub const E1000_MANC: u32 = 0x05820;
/// `E1000_IPAV`: IP Address Valid - RW.
pub const E1000_IPAV: u32 = 0x05838;
/// `E1000_IP4AT`: IPv4 Address Table - RW Array.
pub const E1000_IP4AT: u32 = 0x05840;
/// `E1000_IP6AT`: IPv6 Address Table - RW Array.
pub const E1000_IP6AT: u32 = 0x05880;
/// `E1000_WUPL`: Wakeup Packet Length - RW.
pub const E1000_WUPL: u32 = 0x05900;
/// `E1000_WUPM`: Wakeup Packet Memory - RO A.
pub const E1000_WUPM: u32 = 0x05A00;
/// `E1000_FFLT`: Flexible Filter Length Table - RW Array.
pub const E1000_FFLT: u32 = 0x05F00;
/// `E1000_FCRTV_PCH`: PCH Flow Control Refresh Timer Value.
pub const E1000_FCRTV_PCH: u32 = 0x05F40;
/// `E1000_CRC_OFFSET`: CRC Offset Register.
pub const E1000_CRC_OFFSET: u32 = 0x05F50;
/// `E1000_HOST_IF`: Host Interface.
pub const E1000_HOST_IF: u32 = 0x08800;
/// `E1000_FFMT`: Flexible Filter Mask Table - RW Array.
pub const E1000_FFMT: u32 = 0x09000;
/// `E1000_FFVT`: Flexible Filter Value Table - RW Array.
pub const E1000_FFVT: u32 = 0x09800;
/// `E1000_KUMCTRLSTA`: MAC-PHY interface - RW.
pub const E1000_KUMCTRLSTA: u32 = 0x00034;
/// `E1000_MDPHYA`: PHY address - RW.
pub const E1000_MDPHYA: u32 = 0x0003C;
/// `E1000_MANC2H`: Management Control To Host - RW.
pub const E1000_MANC2H: u32 = 0x05860;
/// `E1000_SW_FW_SYNC`: Software-Firmware Synchronization - RW.
pub const E1000_SW_FW_SYNC: u32 = 0x05B5C;
/// `E1000_GCR`: PCI-Ex Control.
pub const E1000_GCR: u32 = 0x05B00;
/// `E1000_GSCL_1`: PCI-Ex Statistic Control #1.
pub const E1000_GSCL_1: u32 = 0x05B10;
/// `E1000_GSCL_2`: PCI-Ex Statistic Control #2.
pub const E1000_GSCL_2: u32 = 0x05B14;
/// `E1000_GSCL_3`: PCI-Ex Statistic Control #3.
pub const E1000_GSCL_3: u32 = 0x05B18;
/// `E1000_GSCL_4`: PCI-Ex Statistic Control #4.
pub const E1000_GSCL_4: u32 = 0x05B1C;
/// `E1000_FACTPS`: Function Active and Power State to MNG.
pub const E1000_FACTPS: u32 = 0x05B30;
/// `E1000_SWSM`: SW Semaphore.
pub const E1000_SWSM: u32 = 0x05B50;
/// `E1000_H2ME`: Host to ME.
pub const E1000_H2ME: u32 = E1000_SWSM;
/// `E1000_FWSM`: FW Semaphore.
pub const E1000_FWSM: u32 = 0x05B54;
/// `E1000_FFLT_DBG`: Debug Register.
pub const E1000_FFLT_DBG: u32 = 0x05F04;
/// `E1000_HICR`: Host Interface Control.
pub const E1000_HICR: u32 = 0x08F00;
/// `E1000_CPUVEC`: CPU Vector Register - RW.
pub const E1000_CPUVEC: u32 = 0x02C10;
/// `E1000_MRQC`: Multiple Receive Control - RW.
pub const E1000_MRQC: u32 = 0x05818;
/// `E1000_RETA(_i)`: redirection table - RW array.
pub const fn e1000_reta(i: u32) -> u32 {
    0x05C00 + (i * 4)
}
/// `E1000_RSSRK(_i)`: RSS random key - RW array.
pub const fn e1000_rssrk(i: u32) -> u32 {
    0x05C80 + (i * 4)
}
/// `E1000_RSSIM`: RSS Interrupt Mask.
pub const E1000_RSSIM: u32 = 0x05864;
/// `E1000_RSSIR`: RSS Interrupt Request.
pub const E1000_RSSIR: u32 = 0x05868;
/// `E1000_B2OSPC`.
pub const E1000_B2OSPC: u32 = 0x8FE0;
/// `E1000_B2OGPRC`.
pub const E1000_B2OGPRC: u32 = 0x4158;
/// `E1000_O2BGPTC`.
pub const E1000_O2BGPTC: u32 = 0x8FE4;
/// `E1000_O2BSPC`.
pub const E1000_O2BSPC: u32 = 0x415C;
/// `E1000_PQGPRC(_i)`: per queue good packets received count.
pub const fn e1000_pqgprc(i: u32) -> u32 {
    0x010010 + (i * 0x100)
}
/// `E1000_PQGPTC(_i)`: per queue good packets transmitted count.
pub const fn e1000_pqgptc(i: u32) -> u32 {
    0x010014 + (i * 0x100)
}
/// `E1000_PHPM`.
pub const E1000_PHPM: u32 = 0x0E14;
/// `E1000_PHPM_SPD_EN`.
pub const E1000_PHPM_SPD_EN: u32 = 1 << 0;
/// `E1000_PHPM_D0LPLU`.
pub const E1000_PHPM_D0LPLU: u32 = 1 << 1;
/// `E1000_PHPM_LPLU`.
pub const E1000_PHPM_LPLU: u32 = 1 << 2;
/// `E1000_PHPM_DIS_1000_ND0`.
pub const E1000_PHPM_DIS_1000_ND0: u32 = 1 << 3;
/// `E1000_PHPM_LINK_ED`.
pub const E1000_PHPM_LINK_ED: u32 = 1 << 4;
/// `E1000_PHPM_GOLINK_DISC`.
pub const E1000_PHPM_GOLINK_DISC: u32 = 1 << 5;
/// `E1000_PHPM_DIS_1000`.
pub const E1000_PHPM_DIS_1000: u32 = 1 << 6;
/// `E1000_PHPM_SPD_B2B_EN`.
pub const E1000_PHPM_SPD_B2B_EN: u32 = 1 << 7;
/// `E1000_PHPM_RST_COMPL`.
pub const E1000_PHPM_RST_COMPL: u32 = 1 << 8;
/// `E1000_PHPM_DIS_100_ND0`.
pub const E1000_PHPM_DIS_100_ND0: u32 = 1 << 9;
/// `E1000_IPCNFG`: Internal PHY Configuration.
pub const E1000_IPCNFG: u32 = 0x0E38;
/// `E1000_LTRC`: Latency Tolerance Reporting Control.
pub const E1000_LTRC: u32 = 0x01A0;
/// `E1000_EEER`: Energy Efficient Ethernet "EEE".
pub const E1000_EEER: u32 = 0x0E30;
/// `E1000_EEE_SU`: EEE Setup.
pub const E1000_EEE_SU: u32 = 0x0E34;
/// `E1000_TLPIC`: EEE Tx LPI Count - TLPIC.
pub const E1000_TLPIC: u32 = 0x4148;
/// `E1000_RLPIC`: EEE Rx LPI Count - RLPIC.
pub const E1000_RLPIC: u32 = 0x414C;
/// `E1000_FEXTNVM3_PHY_CFG_COUNTER_MASK`.
pub const E1000_FEXTNVM3_PHY_CFG_COUNTER_MASK: u32 = 0x0C000000;
/// `E1000_FEXTNVM3_PHY_CFG_COUNTER_50MSEC`.
pub const E1000_FEXTNVM3_PHY_CFG_COUNTER_50MSEC: u32 = 0x08000000;
/// `E1000_FEXTNVM4_BEACON_DURATION_MASK`.
pub const E1000_FEXTNVM4_BEACON_DURATION_MASK: u32 = 0x7;
/// `E1000_FEXTNVM4_BEACON_DURATION_8USEC`.
pub const E1000_FEXTNVM4_BEACON_DURATION_8USEC: u32 = 0x7;
/// `E1000_FEXTNVM4_BEACON_DURATION_16USEC`.
pub const E1000_FEXTNVM4_BEACON_DURATION_16USEC: u32 = 0x3;
/// `E1000_FEXTNVM6_REQ_PLL_CLK`.
pub const E1000_FEXTNVM6_REQ_PLL_CLK: u32 = 0x00000100;
/// `E1000_FEXTNVM6_ENABLE_K1_ENTRY_CONDITION`.
pub const E1000_FEXTNVM6_ENABLE_K1_ENTRY_CONDITION: u32 = 0x00000200;
/// `E1000_FEXTNVM12_PHYPD_CTRL_MASK`.
pub const E1000_FEXTNVM12_PHYPD_CTRL_MASK: u32 = 0x00C00000;
/// `E1000_FEXTNVM12_PHYPD_CTRL_P1`.
pub const E1000_FEXTNVM12_PHYPD_CTRL_P1: u32 = 0x00800000;
/// `E1000_EEPROM_SWDPIN0`: SWDPIN 0 EEPROM Value.
pub const E1000_EEPROM_SWDPIN0: u32 = 0x0001;
/// `E1000_EEPROM_LED_LOGIC`: Led Logic Word.
pub const E1000_EEPROM_LED_LOGIC: u32 = 0x0020;
/// `E1000_EEPROM_RW_REG_DATA`: Offset to data in EEPROM read/write registers.
pub const E1000_EEPROM_RW_REG_DATA: u32 = 16;
/// `E1000_EEPROM_RW_REG_DONE`: Offset to READ/WRITE done bit.
pub const E1000_EEPROM_RW_REG_DONE: u32 = 2;
/// `E1000_EEPROM_RW_REG_START`: First bit for telling part to start operation.
pub const E1000_EEPROM_RW_REG_START: u32 = 1;
/// `E1000_EEPROM_RW_ADDR_SHIFT`: Shift to the address bits.
pub const E1000_EEPROM_RW_ADDR_SHIFT: u32 = 2;
/// `E1000_EEPROM_POLL_WRITE`: Flag for polling for write complete.
pub const E1000_EEPROM_POLL_WRITE: u32 = 1;
/// `E1000_EEPROM_POLL_READ`: Flag for polling for read complete.
pub const E1000_EEPROM_POLL_READ: u32 = 0;
/// `E1000_CTRL_FD`: Full duplex.0=half; 1=full.
pub const E1000_CTRL_FD: u32 = 0x00000001;
/// `E1000_CTRL_BEM`: Endian Mode.0=little,1=big.
pub const E1000_CTRL_BEM: u32 = 0x00000002;
/// `E1000_CTRL_PRIOR`: Priority on PCI. 0=rx,1=fair.
pub const E1000_CTRL_PRIOR: u32 = 0x00000004;
/// `E1000_CTRL_GIO_MASTER_DISABLE`: Blocks new Master requests.
pub const E1000_CTRL_GIO_MASTER_DISABLE: u32 = 0x00000004;
/// `E1000_CTRL_LRST`: Link reset. 0=normal,1=reset.
pub const E1000_CTRL_LRST: u32 = 0x00000008;
/// `E1000_CTRL_TME`: Test mode. 0=normal,1=test.
pub const E1000_CTRL_TME: u32 = 0x00000010;
/// `E1000_CTRL_SLE`: Serial Link on 0=dis,1=en.
pub const E1000_CTRL_SLE: u32 = 0x00000020;
/// `E1000_CTRL_ASDE`: Auto-speed detect enable.
pub const E1000_CTRL_ASDE: u32 = 0x00000020;
/// `E1000_CTRL_SLU`: Set link up (Force Link).
pub const E1000_CTRL_SLU: u32 = 0x00000040;
/// `E1000_CTRL_ILOS`: Invert Loss-Of Signal.
pub const E1000_CTRL_ILOS: u32 = 0x00000080;
/// `E1000_CTRL_SPD_SEL`: Speed Select Mask.
pub const E1000_CTRL_SPD_SEL: u32 = 0x00000300;
/// `E1000_CTRL_SPD_10`: Force 10Mb.
pub const E1000_CTRL_SPD_10: u32 = 0x00000000;
/// `E1000_CTRL_SPD_100`: Force 100Mb.
pub const E1000_CTRL_SPD_100: u32 = 0x00000100;
/// `E1000_CTRL_SPD_1000`: Force 1Gb.
pub const E1000_CTRL_SPD_1000: u32 = 0x00000200;
/// `E1000_CTRL_BEM32`: Big Endian 32 mode.
pub const E1000_CTRL_BEM32: u32 = 0x00000400;
/// `E1000_CTRL_FRCSPD`: Force Speed.
pub const E1000_CTRL_FRCSPD: u32 = 0x00000800;
/// `E1000_CTRL_FRCDPX`: Force Duplex.
pub const E1000_CTRL_FRCDPX: u32 = 0x00001000;
/// `E1000_CTRL_D_UD_EN`: Dock/Undock enable.
pub const E1000_CTRL_D_UD_EN: u32 = 0x00002000;
/// `E1000_CTRL_D_UD_POLARITY`: Defined polarity of Dock/Undock indication in SDP[0].
pub const E1000_CTRL_D_UD_POLARITY: u32 = 0x00004000;
/// `E1000_CTRL_FORCE_PHY_RESET`: Reset both PHY ports, through PHYRST_N pin.
pub const E1000_CTRL_FORCE_PHY_RESET: u32 = 0x00008000;
/// `E1000_CTRL_LANPHYPC_OVERRIDE`: SW control of LANPHYPC.
pub const E1000_CTRL_LANPHYPC_OVERRIDE: u32 = 0x00010000;
/// `E1000_CTRL_LANPHYPC_VALUE`: SW value of LANPHYPC.
pub const E1000_CTRL_LANPHYPC_VALUE: u32 = 0x00020000;
/// `E1000_CTRL_EXT_DPG_EN`: Dynamic Power Gating Enable.
pub const E1000_CTRL_EXT_DPG_EN: u32 = 0x00000008;
/// `E1000_CTRL_EXT_FORCE_SMBUS`: Force SMBus mode.
pub const E1000_CTRL_EXT_FORCE_SMBUS: u32 = 0x00000800;
/// `E1000_CTRL_EXT_PHYPDEN`.
pub const E1000_CTRL_EXT_PHYPDEN: u32 = 0x00100000;
/// `E1000_I2CCMD_REG_ADDR_SHIFT`.
pub const E1000_I2CCMD_REG_ADDR_SHIFT: u32 = 16;
/// `E1000_I2CCMD_PHY_ADDR_SHIFT`.
pub const E1000_I2CCMD_PHY_ADDR_SHIFT: u32 = 24;
/// `E1000_I2CCMD_OPCODE_READ`.
pub const E1000_I2CCMD_OPCODE_READ: u32 = 0x08000000;
/// `E1000_I2CCMD_OPCODE_WRITE`.
pub const E1000_I2CCMD_OPCODE_WRITE: u32 = 0x00000000;
/// `E1000_I2CCMD_READY`.
pub const E1000_I2CCMD_READY: u32 = 0x20000000;
/// `E1000_I2CCMD_ERROR`.
pub const E1000_I2CCMD_ERROR: u32 = 0x80000000;
/// `E1000_I2CCMD_SFP_DATA_ADDR(a)`: a byte of the SFP module database.
pub const fn e1000_i2ccmd_sfp_data_addr(a: u16) -> u16 {
    a
}
/// `E1000_I2CCMD_SFP_DIAG_ADDR(a)`: a byte of the SFP diagnostics parameters.
pub const fn e1000_i2ccmd_sfp_diag_addr(a: u16) -> u16 {
    0x0100 + a
}
/// `E1000_MAX_SGMII_PHY_REG_ADDR`.
pub const E1000_MAX_SGMII_PHY_REG_ADDR: u32 = 255;
/// `E1000_I2CCMD_PHY_TIMEOUT`.
pub const E1000_I2CCMD_PHY_TIMEOUT: u32 = 200;
/// `E1000_CTRL_SWDPIN0`: SWDPIN 0 value.
pub const E1000_CTRL_SWDPIN0: u32 = 0x00040000;
/// `E1000_CTRL_SWDPIN1`: SWDPIN 1 value.
pub const E1000_CTRL_SWDPIN1: u32 = 0x00080000;
/// `E1000_CTRL_SWDPIN2`: SWDPIN 2 value.
pub const E1000_CTRL_SWDPIN2: u32 = 0x00100000;
/// `E1000_CTRL_SWDPIN3`: SWDPIN 3 value.
pub const E1000_CTRL_SWDPIN3: u32 = 0x00200000;
/// `E1000_CTRL_SWDPIO0`: SWDPIN 0 Input or output.
pub const E1000_CTRL_SWDPIO0: u32 = 0x00400000;
/// `E1000_CTRL_SWDPIO1`: SWDPIN 1 input or output.
pub const E1000_CTRL_SWDPIO1: u32 = 0x00800000;
/// `E1000_CTRL_SWDPIO2`: SWDPIN 2 input or output.
pub const E1000_CTRL_SWDPIO2: u32 = 0x01000000;
/// `E1000_CTRL_SWDPIO3`: SWDPIN 3 input or output.
pub const E1000_CTRL_SWDPIO3: u32 = 0x02000000;
/// `E1000_CTRL_RST`: Global reset.
pub const E1000_CTRL_RST: u32 = 0x04000000;
/// `E1000_CTRL_RFCE`: Receive Flow Control enable.
pub const E1000_CTRL_RFCE: u32 = 0x08000000;
/// `E1000_CTRL_TFCE`: Transmit flow control enable.
pub const E1000_CTRL_TFCE: u32 = 0x10000000;
/// `E1000_CTRL_RTE`: Routing tag enable.
pub const E1000_CTRL_RTE: u32 = 0x20000000;
/// `E1000_CTRL_DEV_RST`: Device Reset.
pub const E1000_CTRL_DEV_RST: u32 = 0x20000000;
/// `E1000_CTRL_VME`: IEEE VLAN mode enable.
pub const E1000_CTRL_VME: u32 = 0x40000000;
/// `E1000_CTRL_PHY_RST`: PHY Reset.
pub const E1000_CTRL_PHY_RST: u32 = 0x80000000;
/// `E1000_CTRL_SW2FW_INT`: Initiate an interrupt to manageability engine.
pub const E1000_CTRL_SW2FW_INT: u32 = 0x02000000;
/// `E1000_CTRL_I2C_ENA`: I2C enable.
pub const E1000_CTRL_I2C_ENA: u32 = 0x02000000;
/// `E1000_CONNSW_ENRGSRC`.
pub const E1000_CONNSW_ENRGSRC: u32 = 0x4;
/// `E1000_PCS_CFG_PCS_EN`.
pub const E1000_PCS_CFG_PCS_EN: u32 = 8;
/// `E1000_PCS_LCTL_FSV_1000`.
pub const E1000_PCS_LCTL_FSV_1000: u32 = 4;
/// `E1000_PCS_LCTL_FDV_FULL`.
pub const E1000_PCS_LCTL_FDV_FULL: u32 = 8;
/// `E1000_PCS_LCTL_FSD`.
pub const E1000_PCS_LCTL_FSD: u32 = 0x10;
/// `E1000_PCS_LCTL_FORCE_FCTRL`.
pub const E1000_PCS_LCTL_FORCE_FCTRL: u32 = 0x80;
/// `E1000_PCS_LSTS_LINK_OK`.
pub const E1000_PCS_LSTS_LINK_OK: u32 = 0x01;
/// `E1000_PCS_LSTS_SPEED_100`.
pub const E1000_PCS_LSTS_SPEED_100: u32 = 0x02;
/// `E1000_PCS_LSTS_SPEED_1000`.
pub const E1000_PCS_LSTS_SPEED_1000: u32 = 0x04;
/// `E1000_PCS_LSTS_DUPLEX_FULL`.
pub const E1000_PCS_LSTS_DUPLEX_FULL: u32 = 0x08;
/// `E1000_PCS_LSTS_SYNK_OK`.
pub const E1000_PCS_LSTS_SYNK_OK: u32 = 0x10;
/// `E1000_STATUS_FD`: Full duplex.0=half,1=full.
pub const E1000_STATUS_FD: u32 = 0x00000001;
/// `E1000_STATUS_LU`: Link up.0=no,1=link.
pub const E1000_STATUS_LU: u32 = 0x00000002;
/// `E1000_STATUS_FUNC_MASK`: PCI Function Mask.
pub const E1000_STATUS_FUNC_MASK: u32 = 0x0000000C;
/// `E1000_STATUS_FUNC_SHIFT`.
pub const E1000_STATUS_FUNC_SHIFT: u32 = 2;
/// `E1000_STATUS_FUNC_0`: Function 0.
pub const E1000_STATUS_FUNC_0: u32 = 0x00000000;
/// `E1000_STATUS_FUNC_1`: Function 1.
pub const E1000_STATUS_FUNC_1: u32 = 0x00000004;
/// `E1000_STATUS_TXOFF`: transmission paused.
pub const E1000_STATUS_TXOFF: u32 = 0x00000010;
/// `E1000_STATUS_TBIMODE`: TBI mode.
pub const E1000_STATUS_TBIMODE: u32 = 0x00000020;
/// `E1000_STATUS_SPEED_MASK`.
pub const E1000_STATUS_SPEED_MASK: u32 = 0x000000C0;
/// `E1000_STATUS_SPEED_10`: Speed 10Mb/s.
pub const E1000_STATUS_SPEED_10: u32 = 0x00000000;
/// `E1000_STATUS_SPEED_100`: Speed 100Mb/s.
pub const E1000_STATUS_SPEED_100: u32 = 0x00000040;
/// `E1000_STATUS_SPEED_1000`: Speed 1000Mb/s.
pub const E1000_STATUS_SPEED_1000: u32 = 0x00000080;
/// `E1000_STATUS_LAN_INIT_DONE`: Lan Init Completion by EEPROM/Flash.
pub const E1000_STATUS_LAN_INIT_DONE: u32 = 0x00000200;
/// `E1000_STATUS_ASDV`: Auto speed detect value.
pub const E1000_STATUS_ASDV: u32 = 0x00000300;
/// `E1000_STATUS_DOCK_CI`: Change in Dock/Undock state. Clear on write '0'.
pub const E1000_STATUS_DOCK_CI: u32 = 0x00000800;
/// `E1000_STATUS_GIO_MASTER_ENABLE`: Status of Master requests.
pub const E1000_STATUS_GIO_MASTER_ENABLE: u32 = 0x00080000;
/// `E1000_STATUS_MTXCKOK`: MTX clock running OK.
pub const E1000_STATUS_MTXCKOK: u32 = 0x00000400;
/// `E1000_STATUS_PCI66`: In 66MHz slot.
pub const E1000_STATUS_PCI66: u32 = 0x00000800;
/// `E1000_STATUS_BUS64`: In 64 bit slot.
pub const E1000_STATUS_BUS64: u32 = 0x00001000;
/// `E1000_STATUS_PCIX_MODE`: PCI-X mode.
pub const E1000_STATUS_PCIX_MODE: u32 = 0x00002000;
/// `E1000_STATUS_PCIX_SPEED`: PCI-X bus speed.
pub const E1000_STATUS_PCIX_SPEED: u32 = 0x0000C000;
/// `E1000_STATUS_BMC_SKU_0`: BMC USB redirect disabled.
pub const E1000_STATUS_BMC_SKU_0: u32 = 0x00100000;
/// `E1000_STATUS_DEV_RST_SET`.
pub const E1000_STATUS_DEV_RST_SET: u32 = 0x00100000;
/// `E1000_STATUS_BMC_SKU_1`: BMC SRAM disabled.
pub const E1000_STATUS_BMC_SKU_1: u32 = 0x00200000;
/// `E1000_STATUS_BMC_SKU_2`: BMC SDRAM disabled.
pub const E1000_STATUS_BMC_SKU_2: u32 = 0x00400000;
/// `E1000_STATUS_BMC_CRYPTO`: BMC crypto disabled.
pub const E1000_STATUS_BMC_CRYPTO: u32 = 0x00800000;
/// `E1000_STATUS_BMC_LITE`: BMC external code execution disabled.
pub const E1000_STATUS_BMC_LITE: u32 = 0x01000000;
/// `E1000_STATUS_RGMII_ENABLE`: RGMII disabled.
pub const E1000_STATUS_RGMII_ENABLE: u32 = 0x02000000;
/// `E1000_STATUS_FUSE_8`.
pub const E1000_STATUS_FUSE_8: u32 = 0x04000000;
/// `E1000_STATUS_FUSE_9`.
pub const E1000_STATUS_FUSE_9: u32 = 0x08000000;
/// `E1000_STATUS_SERDES0_DIS`: SERDES disabled on port 0.
pub const E1000_STATUS_SERDES0_DIS: u32 = 0x10000000;
/// `E1000_STATUS_SERDES1_DIS`: SERDES disabled on port 1.
pub const E1000_STATUS_SERDES1_DIS: u32 = 0x20000000;
/// `E1000_STATUS_PCIX_SPEED_66`: PCI-X bus speed 50-66 MHz.
pub const E1000_STATUS_PCIX_SPEED_66: u32 = 0x00000000;
/// `E1000_STATUS_PCIX_SPEED_100`: PCI-X bus speed 66-100 MHz.
pub const E1000_STATUS_PCIX_SPEED_100: u32 = 0x00004000;
/// `E1000_STATUS_PCIX_SPEED_133`: PCI-X bus speed 100-133 MHz.
pub const E1000_STATUS_PCIX_SPEED_133: u32 = 0x00008000;
/// `E1000_EECD_SK`: EEPROM Clock.
pub const E1000_EECD_SK: u32 = 0x00000001;
/// `E1000_EECD_CS`: EEPROM Chip Select.
pub const E1000_EECD_CS: u32 = 0x00000002;
/// `E1000_EECD_DI`: EEPROM Data In.
pub const E1000_EECD_DI: u32 = 0x00000004;
/// `E1000_EECD_DO`: EEPROM Data Out.
pub const E1000_EECD_DO: u32 = 0x00000008;
/// `E1000_EECD_FWE_MASK`.
pub const E1000_EECD_FWE_MASK: u32 = 0x00000030;
/// `E1000_EECD_FWE_DIS`: Disable FLASH writes.
pub const E1000_EECD_FWE_DIS: u32 = 0x00000010;
/// `E1000_EECD_FWE_EN`: Enable FLASH writes.
pub const E1000_EECD_FWE_EN: u32 = 0x00000020;
/// `E1000_EECD_FWE_SHIFT`.
pub const E1000_EECD_FWE_SHIFT: u32 = 4;
/// `E1000_EECD_REQ`: EEPROM Access Request.
pub const E1000_EECD_REQ: u32 = 0x00000040;
/// `E1000_EECD_GNT`: EEPROM Access Grant.
pub const E1000_EECD_GNT: u32 = 0x00000080;
/// `E1000_EECD_PRES`: EEPROM Present.
pub const E1000_EECD_PRES: u32 = 0x00000100;
/// `E1000_EECD_SIZE`: EEPROM Size (0=64 word 1=256 word).
pub const E1000_EECD_SIZE: u32 = 0x00000200;
/// `E1000_EECD_ADDR_BITS`: EEPROM Addressing bits based on type (0-small, 1-large).
pub const E1000_EECD_ADDR_BITS: u32 = 0x00000400;
/// `E1000_EECD_TYPE`: EEPROM Type (1-SPI, 0-Microwire).
pub const E1000_EECD_TYPE: u32 = 0x00002000;
/// `E1000_EEPROM_GRANT_ATTEMPTS`: EEPROM # attempts to gain grant.
pub const E1000_EEPROM_GRANT_ATTEMPTS: u32 = 1000;
/// `E1000_EECD_AUTO_RD`: EEPROM Auto Read done.
pub const E1000_EECD_AUTO_RD: u32 = 0x00000200;
/// `E1000_EECD_SIZE_EX_MASK`: EEprom Size.
pub const E1000_EECD_SIZE_EX_MASK: u32 = 0x00007800;
/// `E1000_EECD_SIZE_EX_SHIFT`.
pub const E1000_EECD_SIZE_EX_SHIFT: u32 = 11;
/// `E1000_EECD_NVADDS`: NVM Address Size.
pub const E1000_EECD_NVADDS: u32 = 0x00018000;
/// `E1000_EECD_SELSHAD`: Select Shadow RAM.
pub const E1000_EECD_SELSHAD: u32 = 0x00020000;
/// `E1000_EECD_INITSRAM`: Initialize Shadow RAM.
pub const E1000_EECD_INITSRAM: u32 = 0x00040000;
/// `E1000_EECD_FLUPD`: Update FLASH.
pub const E1000_EECD_FLUPD: u32 = 0x00080000;
/// `E1000_EECD_AUPDEN`: Enable Autonomous FLASH update.
pub const E1000_EECD_AUPDEN: u32 = 0x00100000;
/// `E1000_EECD_SHADV`: Shadow RAM Data Valid.
pub const E1000_EECD_SHADV: u32 = 0x00200000;
/// `E1000_EECD_SEC1VAL`: Sector One Valid.
pub const E1000_EECD_SEC1VAL: u32 = 0x00400000;
/// `E1000_EECD_SEC1VAL_VALID_MASK`.
pub const E1000_EECD_SEC1VAL_VALID_MASK: u32 = E1000_EECD_AUTO_RD | E1000_EECD_PRES;
/// `E1000_EECD_SECVAL_SHIFT`.
pub const E1000_EECD_SECVAL_SHIFT: u32 = 22;
/// `E1000_STM_OPCODE`.
pub const E1000_STM_OPCODE: u32 = 0xDB00;
/// `E1000_HICR_FW_RESET`.
pub const E1000_HICR_FW_RESET: u32 = 0xC0;
/// `E1000_SHADOW_RAM_WORDS`.
pub const E1000_SHADOW_RAM_WORDS: u32 = 2048;
/// `E1000_ICH_NVM_SIG_WORD`.
pub const E1000_ICH_NVM_SIG_WORD: u32 = 0x13;
/// `E1000_ICH_NVM_SIG_MASK`.
pub const E1000_ICH_NVM_SIG_MASK: u32 = 0xC000;
/// `E1000_ICH_NVM_VALID_SIG_MASK`.
pub const E1000_ICH_NVM_VALID_SIG_MASK: u32 = 0xC0;
/// `E1000_ICH_NVM_SIG_VALUE`.
pub const E1000_ICH_NVM_SIG_VALUE: u32 = 0x80;
/// `E1000_EERD_START`: Start Read.
pub const E1000_EERD_START: u32 = 0x00000001;
/// `E1000_EERD_DONE`: Read Done.
pub const E1000_EERD_DONE: u32 = 0x00000010;
/// `E1000_EERD_ADDR_SHIFT`.
pub const E1000_EERD_ADDR_SHIFT: u32 = 8;
/// `E1000_EERD_ADDR_MASK`: Read Address.
pub const E1000_EERD_ADDR_MASK: u32 = 0x0000FF00;
/// `E1000_EERD_DATA_SHIFT`.
pub const E1000_EERD_DATA_SHIFT: u32 = 16;
/// `E1000_EERD_DATA_MASK`: Read Data.
pub const E1000_EERD_DATA_MASK: u32 = 0xFFFF0000;
/// `EEPROM_STATUS_RDY_SPI`.
pub const EEPROM_STATUS_RDY_SPI: u8 = 0x01;
/// `EEPROM_STATUS_WEN_SPI`.
pub const EEPROM_STATUS_WEN_SPI: u16 = 0x02;
/// `EEPROM_STATUS_BP0_SPI`.
pub const EEPROM_STATUS_BP0_SPI: u16 = 0x04;
/// `EEPROM_STATUS_BP1_SPI`.
pub const EEPROM_STATUS_BP1_SPI: u16 = 0x08;
/// `EEPROM_STATUS_WPEN_SPI`.
pub const EEPROM_STATUS_WPEN_SPI: u16 = 0x80;
/// `E1000_CTRL_EXT_GPI0_EN`: Maps SDP4 to GPI0.
pub const E1000_CTRL_EXT_GPI0_EN: u32 = 0x00000001;
/// `E1000_CTRL_EXT_GPI1_EN`: Maps SDP5 to GPI1.
pub const E1000_CTRL_EXT_GPI1_EN: u32 = 0x00000002;
/// `E1000_CTRL_EXT_PHYINT_EN`.
pub const E1000_CTRL_EXT_PHYINT_EN: u32 = E1000_CTRL_EXT_GPI1_EN;
/// `E1000_CTRL_EXT_GPI2_EN`: Maps SDP6 to GPI2.
pub const E1000_CTRL_EXT_GPI2_EN: u32 = 0x00000004;
/// `E1000_CTRL_EXT_LPCD`: LCD Power Cycle Done.
pub const E1000_CTRL_EXT_LPCD: u32 = 0x00000004;
/// `E1000_CTRL_EXT_GPI3_EN`: Maps SDP7 to GPI3.
pub const E1000_CTRL_EXT_GPI3_EN: u32 = 0x00000008;
/// `E1000_CTRL_EXT_SDP4_DATA`: Value of SW Definable Pin 4.
pub const E1000_CTRL_EXT_SDP4_DATA: u32 = 0x00000010;
/// `E1000_CTRL_EXT_SDP5_DATA`: Value of SW Definable Pin 5.
pub const E1000_CTRL_EXT_SDP5_DATA: u32 = 0x00000020;
/// `E1000_CTRL_EXT_PHY_INT`.
pub const E1000_CTRL_EXT_PHY_INT: u32 = E1000_CTRL_EXT_SDP5_DATA;
/// `E1000_CTRL_EXT_SDP6_DATA`: Value of SW Definable Pin 6.
pub const E1000_CTRL_EXT_SDP6_DATA: u32 = 0x00000040;
/// `E1000_CTRL_EXT_SDP7_DATA`: Value of SW Definable Pin 7.
pub const E1000_CTRL_EXT_SDP7_DATA: u32 = 0x00000080;
/// `E1000_CTRL_EXT_SDP3_DATA`: Value of SW Definable Pin 3.
pub const E1000_CTRL_EXT_SDP3_DATA: u32 = 0x00000080;
/// `E1000_CTRL_EXT_SDP4_DIR`: Direction of SDP4 0=in 1=out.
pub const E1000_CTRL_EXT_SDP4_DIR: u32 = 0x00000100;
/// `E1000_CTRL_EXT_SDP5_DIR`: Direction of SDP5 0=in 1=out.
pub const E1000_CTRL_EXT_SDP5_DIR: u32 = 0x00000200;
/// `E1000_CTRL_EXT_SDP6_DIR`: Direction of SDP6 0=in 1=out.
pub const E1000_CTRL_EXT_SDP6_DIR: u32 = 0x00000400;
/// `E1000_CTRL_EXT_SDP7_DIR`: Direction of SDP7 0=in 1=out.
pub const E1000_CTRL_EXT_SDP7_DIR: u32 = 0x00000800;
/// `E1000_CTRL_EXT_ASDCHK`: Initiate an ASD sequence.
pub const E1000_CTRL_EXT_ASDCHK: u32 = 0x00001000;
/// `E1000_CTRL_EXT_EE_RST`: Reinitialize from EEPROM.
pub const E1000_CTRL_EXT_EE_RST: u32 = 0x00002000;
/// `E1000_CTRL_EXT_IPS`: Invert Power State.
pub const E1000_CTRL_EXT_IPS: u32 = 0x00004000;
/// `E1000_CTRL_EXT_SPD_BYPS`: Speed Select Bypass.
pub const E1000_CTRL_EXT_SPD_BYPS: u32 = 0x00008000;
/// `E1000_CTRL_EXT_RO_DIS`: Relaxed Ordering disable.
pub const E1000_CTRL_EXT_RO_DIS: u32 = 0x00020000;
/// `E1000_CTRL_EXT_LINK_MODE_MASK`.
pub const E1000_CTRL_EXT_LINK_MODE_MASK: u32 = 0x00C00000;
/// `E1000_CTRL_EXT_LINK_MODE_GMII`.
pub const E1000_CTRL_EXT_LINK_MODE_GMII: u32 = 0x00000000;
/// `E1000_CTRL_EXT_LINK_MODE_TBI`.
pub const E1000_CTRL_EXT_LINK_MODE_TBI: u32 = 0x00C00000;
/// `E1000_CTRL_EXT_LINK_MODE_KMRN`.
pub const E1000_CTRL_EXT_LINK_MODE_KMRN: u32 = 0x00000000;
/// `E1000_CTRL_EXT_LINK_MODE_PCIE_SERDES`.
pub const E1000_CTRL_EXT_LINK_MODE_PCIE_SERDES: u32 = 0x00C00000;
/// `E1000_CTRL_EXT_LINK_MODE_1000BASE_KX`.
pub const E1000_CTRL_EXT_LINK_MODE_1000BASE_KX: u32 = 0x00400000;
/// `E1000_CTRL_EXT_LINK_MODE_SGMII`.
pub const E1000_CTRL_EXT_LINK_MODE_SGMII: u32 = 0x00800000;
/// `E1000_CTRL_EXT_WR_WMARK_MASK`.
pub const E1000_CTRL_EXT_WR_WMARK_MASK: u32 = 0x03000000;
/// `E1000_CTRL_EXT_WR_WMARK_256`.
pub const E1000_CTRL_EXT_WR_WMARK_256: u32 = 0x00000000;
/// `E1000_CTRL_EXT_WR_WMARK_320`.
pub const E1000_CTRL_EXT_WR_WMARK_320: u32 = 0x01000000;
/// `E1000_CTRL_EXT_WR_WMARK_384`.
pub const E1000_CTRL_EXT_WR_WMARK_384: u32 = 0x02000000;
/// `E1000_CTRL_EXT_WR_WMARK_448`.
pub const E1000_CTRL_EXT_WR_WMARK_448: u32 = 0x03000000;
/// `E1000_CTRL_EXT_EXT_VLAN`.
pub const E1000_CTRL_EXT_EXT_VLAN: u32 = 0x04000000;
/// `E1000_CTRL_EXT_DRV_LOAD`: Driver loaded bit for FW.
pub const E1000_CTRL_EXT_DRV_LOAD: u32 = 0x10000000;
/// `E1000_CTRL_EXT_IAME`: Interrupt acknowledge Auto-mask.
pub const E1000_CTRL_EXT_IAME: u32 = 0x08000000;
/// `E1000_CTRL_EXT_INT_TIMER_CLR`: Clear Interrupt timers after IMS clear.
pub const E1000_CTRL_EXT_INT_TIMER_CLR: u32 = 0x20000000;
/// `E1000_CRTL_EXT_PB_PAREN`: packet buffer parity error detection enabled.
pub const E1000_CRTL_EXT_PB_PAREN: u32 = 0x01000000;
/// `E1000_CTRL_EXT_DF_PAREN`: descriptor FIFO parity error detection enable.
pub const E1000_CTRL_EXT_DF_PAREN: u32 = 0x02000000;
/// `E1000_CTRL_EXT_GHOST_PAREN`.
pub const E1000_CTRL_EXT_GHOST_PAREN: u32 = 0x40000000;
/// `E1000_MDIC_DATA_MASK`.
pub const E1000_MDIC_DATA_MASK: u32 = 0x0000FFFF;
/// `E1000_MDIC_REG_MASK`.
pub const E1000_MDIC_REG_MASK: u32 = 0x001F0000;
/// `E1000_MDIC_REG_SHIFT`.
pub const E1000_MDIC_REG_SHIFT: u32 = 16;
/// `E1000_MDIC_PHY_MASK`.
pub const E1000_MDIC_PHY_MASK: u32 = 0x03E00000;
/// `E1000_MDIC_PHY_SHIFT`.
pub const E1000_MDIC_PHY_SHIFT: u32 = 21;
/// `E1000_MDIC_OP_WRITE`.
pub const E1000_MDIC_OP_WRITE: u32 = 0x04000000;
/// `E1000_MDIC_OP_READ`.
pub const E1000_MDIC_OP_READ: u32 = 0x08000000;
/// `E1000_MDIC_READY`.
pub const E1000_MDIC_READY: u32 = 0x10000000;
/// `E1000_MDIC_INT_EN`.
pub const E1000_MDIC_INT_EN: u32 = 0x20000000;
/// `E1000_MDIC_ERROR`.
pub const E1000_MDIC_ERROR: u32 = 0x40000000;
/// `E1000_MDIC_DEST`.
pub const E1000_MDIC_DEST: u32 = 0x80000000;
/// `E1000_KUMCTRLSTA_MASK`.
pub const E1000_KUMCTRLSTA_MASK: u32 = 0x0000FFFF;
/// `E1000_KUMCTRLSTA_OFFSET`.
pub const E1000_KUMCTRLSTA_OFFSET: u32 = 0x001F0000;
/// `E1000_KUMCTRLSTA_OFFSET_SHIFT`.
pub const E1000_KUMCTRLSTA_OFFSET_SHIFT: u32 = 16;
/// `E1000_KUMCTRLSTA_REN`.
pub const E1000_KUMCTRLSTA_REN: u32 = 0x00200000;
/// `E1000_KUMCTRLSTA_OFFSET_FIFO_CTRL`.
pub const E1000_KUMCTRLSTA_OFFSET_FIFO_CTRL: u32 = 0x00000000;
/// `E1000_KUMCTRLSTA_OFFSET_CTRL`.
pub const E1000_KUMCTRLSTA_OFFSET_CTRL: u32 = 0x00000001;
/// `E1000_KUMCTRLSTA_OFFSET_INB_CTRL`.
pub const E1000_KUMCTRLSTA_OFFSET_INB_CTRL: u32 = 0x00000002;
/// `E1000_KUMCTRLSTA_OFFSET_DIAG`.
pub const E1000_KUMCTRLSTA_OFFSET_DIAG: u32 = 0x00000003;
/// `E1000_KUMCTRLSTA_OFFSET_TIMEOUTS`.
pub const E1000_KUMCTRLSTA_OFFSET_TIMEOUTS: u32 = 0x00000004;
/// `E1000_KUMCTRLSTA_OFFSET_INB_PARAM`.
pub const E1000_KUMCTRLSTA_OFFSET_INB_PARAM: u32 = 0x00000009;
/// `E1000_KUMCTRLSTA_OFFSET_HD_CTRL`.
pub const E1000_KUMCTRLSTA_OFFSET_HD_CTRL: u32 = 0x00000010;
/// `E1000_KUMCTRLSTA_OFFSET_M2P_SERDES`.
pub const E1000_KUMCTRLSTA_OFFSET_M2P_SERDES: u32 = 0x0000001E;
/// `E1000_KUMCTRLSTA_OFFSET_M2P_MODES`.
pub const E1000_KUMCTRLSTA_OFFSET_M2P_MODES: u32 = 0x0000001F;
/// `E1000_KUMCTRLSTA_FIFO_CTRL_RX_BYPASS`.
pub const E1000_KUMCTRLSTA_FIFO_CTRL_RX_BYPASS: u16 = 0x00000008;
/// `E1000_KUMCTRLSTA_FIFO_CTRL_TX_BYPASS`.
pub const E1000_KUMCTRLSTA_FIFO_CTRL_TX_BYPASS: u16 = 0x00000800;
/// `E1000_KUMCTRLSTA_INB_CTRL_LINK_STATUS_TX_TIMEOUT_DEFAULT`.
pub const E1000_KUMCTRLSTA_INB_CTRL_LINK_STATUS_TX_TIMEOUT_DEFAULT: u16 = 0x00000500;
/// `E1000_KUMCTRLSTA_INB_CTRL_DIS_PADDING`.
pub const E1000_KUMCTRLSTA_INB_CTRL_DIS_PADDING: u16 = 0x00000010;
/// `E1000_KUMCTRLSTA_HD_CTRL_10_100_DEFAULT`.
pub const E1000_KUMCTRLSTA_HD_CTRL_10_100_DEFAULT: u16 = 0x00000004;
/// `E1000_KUMCTRLSTA_HD_CTRL_1000_DEFAULT`.
pub const E1000_KUMCTRLSTA_HD_CTRL_1000_DEFAULT: u16 = 0x00000000;
/// `E1000_KUMCTRLSTA_OFFSET_K0S_CTRL`.
pub const E1000_KUMCTRLSTA_OFFSET_K0S_CTRL: u32 = 0x0000001E;
/// `E1000_KUMCTRLSTA_DIAG_FELPBK`.
pub const E1000_KUMCTRLSTA_DIAG_FELPBK: u32 = 0x2000;
/// `E1000_KUMCTRLSTA_DIAG_NELPBK`.
pub const E1000_KUMCTRLSTA_DIAG_NELPBK: u32 = 0x1000;
/// `E1000_KUMCTRLSTA_K0S_100_EN`.
pub const E1000_KUMCTRLSTA_K0S_100_EN: u32 = 0x2000;
/// `E1000_KUMCTRLSTA_K0S_GBE_EN`.
pub const E1000_KUMCTRLSTA_K0S_GBE_EN: u32 = 0x1000;
/// `E1000_KUMCTRLSTA_K0S_ENTRY_LATENCY_MASK`.
pub const E1000_KUMCTRLSTA_K0S_ENTRY_LATENCY_MASK: u32 = 0x0003;
/// `E1000_KABGTXD_BGSQLBIAS`.
pub const E1000_KABGTXD_BGSQLBIAS: u32 = 0x00050000;
/// `E1000_PHY_CTRL_SPD_EN`.
pub const E1000_PHY_CTRL_SPD_EN: u32 = 0x00000001;
/// `E1000_PHY_CTRL_D0A_LPLU`.
pub const E1000_PHY_CTRL_D0A_LPLU: u32 = 0x00000002;
/// `E1000_PHY_CTRL_NOND0A_LPLU`.
pub const E1000_PHY_CTRL_NOND0A_LPLU: u32 = 0x00000004;
/// `E1000_PHY_CTRL_NOND0A_GBE_DISABLE`.
pub const E1000_PHY_CTRL_NOND0A_GBE_DISABLE: u32 = 0x00000008;
/// `E1000_PHY_CTRL_GBE_DISABLE`.
pub const E1000_PHY_CTRL_GBE_DISABLE: u32 = 0x00000040;
/// `E1000_PHY_CTRL_B2B_EN`.
pub const E1000_PHY_CTRL_B2B_EN: u32 = 0x00000080;
/// `E1000_PHY_CTRL_LOOPBACK`.
pub const E1000_PHY_CTRL_LOOPBACK: u16 = 0x00004000;
/// `E1000_LEDCTL_LED0_MODE_MASK`.
pub const E1000_LEDCTL_LED0_MODE_MASK: u32 = 0x0000000F;
/// `E1000_LEDCTL_LED0_MODE_SHIFT`.
pub const E1000_LEDCTL_LED0_MODE_SHIFT: u32 = 0;
/// `E1000_LEDCTL_LED0_BLINK_RATE`.
pub const E1000_LEDCTL_LED0_BLINK_RATE: u32 = 0x0000020;
/// `E1000_LEDCTL_LED0_IVRT`.
pub const E1000_LEDCTL_LED0_IVRT: u32 = 0x00000040;
/// `E1000_LEDCTL_LED0_BLINK`.
pub const E1000_LEDCTL_LED0_BLINK: u32 = 0x00000080;
/// `E1000_LEDCTL_LED1_MODE_MASK`.
pub const E1000_LEDCTL_LED1_MODE_MASK: u32 = 0x00000F00;
/// `E1000_LEDCTL_LED1_MODE_SHIFT`.
pub const E1000_LEDCTL_LED1_MODE_SHIFT: u32 = 8;
/// `E1000_LEDCTL_LED1_BLINK_RATE`.
pub const E1000_LEDCTL_LED1_BLINK_RATE: u32 = 0x0002000;
/// `E1000_LEDCTL_LED1_IVRT`.
pub const E1000_LEDCTL_LED1_IVRT: u32 = 0x00004000;
/// `E1000_LEDCTL_LED1_BLINK`.
pub const E1000_LEDCTL_LED1_BLINK: u32 = 0x00008000;
/// `E1000_LEDCTL_LED2_MODE_MASK`.
pub const E1000_LEDCTL_LED2_MODE_MASK: u32 = 0x000F0000;
/// `E1000_LEDCTL_LED2_MODE_SHIFT`.
pub const E1000_LEDCTL_LED2_MODE_SHIFT: u32 = 16;
/// `E1000_LEDCTL_LED2_BLINK_RATE`.
pub const E1000_LEDCTL_LED2_BLINK_RATE: u32 = 0x00200000;
/// `E1000_LEDCTL_LED2_IVRT`.
pub const E1000_LEDCTL_LED2_IVRT: u32 = 0x00400000;
/// `E1000_LEDCTL_LED2_BLINK`.
pub const E1000_LEDCTL_LED2_BLINK: u32 = 0x00800000;
/// `E1000_LEDCTL_LED3_MODE_MASK`.
pub const E1000_LEDCTL_LED3_MODE_MASK: u32 = 0x0F000000;
/// `E1000_LEDCTL_LED3_MODE_SHIFT`.
pub const E1000_LEDCTL_LED3_MODE_SHIFT: u32 = 24;
/// `E1000_LEDCTL_LED3_BLINK_RATE`.
pub const E1000_LEDCTL_LED3_BLINK_RATE: u32 = 0x20000000;
/// `E1000_LEDCTL_LED3_IVRT`.
pub const E1000_LEDCTL_LED3_IVRT: u32 = 0x40000000;
/// `E1000_LEDCTL_LED3_BLINK`.
pub const E1000_LEDCTL_LED3_BLINK: u32 = 0x80000000;
/// `E1000_LEDCTL_MODE_LINK_10_1000`.
pub const E1000_LEDCTL_MODE_LINK_10_1000: u32 = 0x0;
/// `E1000_LEDCTL_MODE_LINK_100_1000`.
pub const E1000_LEDCTL_MODE_LINK_100_1000: u32 = 0x1;
/// `E1000_LEDCTL_MODE_LINK_UP`.
pub const E1000_LEDCTL_MODE_LINK_UP: u32 = 0x2;
/// `E1000_LEDCTL_MODE_ACTIVITY`.
pub const E1000_LEDCTL_MODE_ACTIVITY: u32 = 0x3;
/// `E1000_LEDCTL_MODE_LINK_ACTIVITY`.
pub const E1000_LEDCTL_MODE_LINK_ACTIVITY: u32 = 0x4;
/// `E1000_LEDCTL_MODE_LINK_10`.
pub const E1000_LEDCTL_MODE_LINK_10: u32 = 0x5;
/// `E1000_LEDCTL_MODE_LINK_100`.
pub const E1000_LEDCTL_MODE_LINK_100: u32 = 0x6;
/// `E1000_LEDCTL_MODE_LINK_1000`.
pub const E1000_LEDCTL_MODE_LINK_1000: u32 = 0x7;
/// `E1000_LEDCTL_MODE_PCIX_MODE`.
pub const E1000_LEDCTL_MODE_PCIX_MODE: u32 = 0x8;
/// `E1000_LEDCTL_MODE_FULL_DUPLEX`.
pub const E1000_LEDCTL_MODE_FULL_DUPLEX: u32 = 0x9;
/// `E1000_LEDCTL_MODE_COLLISION`.
pub const E1000_LEDCTL_MODE_COLLISION: u32 = 0xA;
/// `E1000_LEDCTL_MODE_BUS_SPEED`.
pub const E1000_LEDCTL_MODE_BUS_SPEED: u32 = 0xB;
/// `E1000_LEDCTL_MODE_BUS_SIZE`.
pub const E1000_LEDCTL_MODE_BUS_SIZE: u32 = 0xC;
/// `E1000_LEDCTL_MODE_PAUSED`.
pub const E1000_LEDCTL_MODE_PAUSED: u32 = 0xD;
/// `E1000_LEDCTL_MODE_LED_ON`.
pub const E1000_LEDCTL_MODE_LED_ON: u32 = 0xE;
/// `E1000_LEDCTL_MODE_LED_OFF`.
pub const E1000_LEDCTL_MODE_LED_OFF: u32 = 0xF;
/// `E1000_RAH_AV`: Receive descriptor valid.
pub const E1000_RAH_AV: u32 = 0x80000000;
/// `E1000_ICR_TXDW`: Transmit desc written back.
pub const E1000_ICR_TXDW: u32 = 0x00000001;
/// `E1000_ICR_TXQE`: Transmit Queue empty.
pub const E1000_ICR_TXQE: u32 = 0x00000002;
/// `E1000_ICR_LSC`: Link Status Change.
pub const E1000_ICR_LSC: u32 = 0x00000004;
/// `E1000_ICR_RXSEQ`: rx sequence error.
pub const E1000_ICR_RXSEQ: u32 = 0x00000008;
/// `E1000_ICR_RXDMT0`: rx desc min. threshold (0).
pub const E1000_ICR_RXDMT0: u32 = 0x00000010;
/// `E1000_ICR_RXO`: rx overrun.
pub const E1000_ICR_RXO: u32 = 0x00000040;
/// `E1000_ICR_RXT0`: rx timer intr (ring 0).
pub const E1000_ICR_RXT0: u32 = 0x00000080;
/// `E1000_ICR_MDAC`: MDIO access complete.
pub const E1000_ICR_MDAC: u32 = 0x00000200;
/// `E1000_ICR_RXCFG`: RX /c/ ordered set.
pub const E1000_ICR_RXCFG: u32 = 0x00000400;
/// `E1000_ICR_GPI_EN0`: GP Int 0.
pub const E1000_ICR_GPI_EN0: u32 = 0x00000800;
/// `E1000_ICR_GPI_EN1`: GP Int 1.
pub const E1000_ICR_GPI_EN1: u32 = 0x00001000;
/// `E1000_ICR_GPI_EN2`: GP Int 2.
pub const E1000_ICR_GPI_EN2: u32 = 0x00002000;
/// `E1000_ICR_GPI_EN3`: GP Int 3.
pub const E1000_ICR_GPI_EN3: u32 = 0x00004000;
/// `E1000_ICR_TXD_LOW`.
pub const E1000_ICR_TXD_LOW: u32 = 0x00008000;
/// `E1000_ICR_SRPD`.
pub const E1000_ICR_SRPD: u32 = 0x00010000;
/// `E1000_ICR_ACK`: Receive Ack frame.
pub const E1000_ICR_ACK: u32 = 0x00020000;
/// `E1000_ICR_MNG`: Manageability event.
pub const E1000_ICR_MNG: u32 = 0x00040000;
/// `E1000_ICR_DOCK`: Dock/Undock.
pub const E1000_ICR_DOCK: u32 = 0x00080000;
/// `E1000_ICR_INT_ASSERTED`: If this bit asserted, the driver should claim the interrupt.
pub const E1000_ICR_INT_ASSERTED: u32 = 0x80000000;
/// `E1000_ICR_RXD_FIFO_PAR0`: queue 0 Rx descriptor FIFO parity error.
pub const E1000_ICR_RXD_FIFO_PAR0: u32 = 0x00100000;
/// `E1000_ICR_TXD_FIFO_PAR0`: queue 0 Tx descriptor FIFO parity error.
pub const E1000_ICR_TXD_FIFO_PAR0: u32 = 0x00200000;
/// `E1000_ICR_HOST_ARB_PAR`: host arb read buffer parity error.
pub const E1000_ICR_HOST_ARB_PAR: u32 = 0x00400000;
/// `E1000_ICR_PB_PAR`: packet buffer parity error.
pub const E1000_ICR_PB_PAR: u32 = 0x00800000;
/// `E1000_ICR_RXD_FIFO_PAR1`: queue 1 Rx descriptor FIFO parity error.
pub const E1000_ICR_RXD_FIFO_PAR1: u32 = 0x01000000;
/// `E1000_ICR_TXD_FIFO_PAR1`: queue 1 Tx descriptor FIFO parity error.
pub const E1000_ICR_TXD_FIFO_PAR1: u32 = 0x02000000;
/// `E1000_ICR_ALL_PARITY`: all parity error bits.
pub const E1000_ICR_ALL_PARITY: u32 = 0x03F00000;
/// `E1000_ICR_DSW`: FW changed the status of DISSW bit in the FWSM.
pub const E1000_ICR_DSW: u32 = 0x00000020;
/// `E1000_ICR_PHYINT`: LAN connected device generates an interrupt.
pub const E1000_ICR_PHYINT: u32 = 0x00001000;
/// `E1000_ICR_EPRST`: ME hardware reset occurs.
pub const E1000_ICR_EPRST: u32 = 0x00100000;
/// `E1000_ICR_DRSTA`: Device Reset Asserted.
pub const E1000_ICR_DRSTA: u32 = 0x40000000;
/// `E1000_ICS_TXDW`: Transmit desc written back.
pub const E1000_ICS_TXDW: u32 = E1000_ICR_TXDW;
/// `E1000_ICS_TXQE`: Transmit Queue empty.
pub const E1000_ICS_TXQE: u32 = E1000_ICR_TXQE;
/// `E1000_ICS_LSC`: Link Status Change.
pub const E1000_ICS_LSC: u32 = E1000_ICR_LSC;
/// `E1000_ICS_RXSEQ`: rx sequence error.
pub const E1000_ICS_RXSEQ: u32 = E1000_ICR_RXSEQ;
/// `E1000_ICS_RXDMT0`: rx desc min. threshold.
pub const E1000_ICS_RXDMT0: u32 = E1000_ICR_RXDMT0;
/// `E1000_ICS_RXO`: rx overrun.
pub const E1000_ICS_RXO: u32 = E1000_ICR_RXO;
/// `E1000_ICS_RXT0`: rx timer intr.
pub const E1000_ICS_RXT0: u32 = E1000_ICR_RXT0;
/// `E1000_ICS_MDAC`: MDIO access complete.
pub const E1000_ICS_MDAC: u32 = E1000_ICR_MDAC;
/// `E1000_ICS_RXCFG`: RX /c/ ordered set.
pub const E1000_ICS_RXCFG: u32 = E1000_ICR_RXCFG;
/// `E1000_ICS_GPI_EN0`: GP Int 0.
pub const E1000_ICS_GPI_EN0: u32 = E1000_ICR_GPI_EN0;
/// `E1000_ICS_GPI_EN1`: GP Int 1.
pub const E1000_ICS_GPI_EN1: u32 = E1000_ICR_GPI_EN1;
/// `E1000_ICS_GPI_EN2`: GP Int 2.
pub const E1000_ICS_GPI_EN2: u32 = E1000_ICR_GPI_EN2;
/// `E1000_ICS_GPI_EN3`: GP Int 3.
pub const E1000_ICS_GPI_EN3: u32 = E1000_ICR_GPI_EN3;
/// `E1000_ICS_TXD_LOW`.
pub const E1000_ICS_TXD_LOW: u32 = E1000_ICR_TXD_LOW;
/// `E1000_ICS_SRPD`.
pub const E1000_ICS_SRPD: u32 = E1000_ICR_SRPD;
/// `E1000_ICS_ACK`: Receive Ack frame.
pub const E1000_ICS_ACK: u32 = E1000_ICR_ACK;
/// `E1000_ICS_MNG`: Manageability event.
pub const E1000_ICS_MNG: u32 = E1000_ICR_MNG;
/// `E1000_ICS_DOCK`: Dock/Undock.
pub const E1000_ICS_DOCK: u32 = E1000_ICR_DOCK;
/// `E1000_ICS_RXD_FIFO_PAR0`: queue 0 Rx descriptor FIFO parity error.
pub const E1000_ICS_RXD_FIFO_PAR0: u32 = E1000_ICR_RXD_FIFO_PAR0;
/// `E1000_ICS_TXD_FIFO_PAR0`: queue 0 Tx descriptor FIFO parity error.
pub const E1000_ICS_TXD_FIFO_PAR0: u32 = E1000_ICR_TXD_FIFO_PAR0;
/// `E1000_ICS_HOST_ARB_PAR`: host arb read buffer parity error.
pub const E1000_ICS_HOST_ARB_PAR: u32 = E1000_ICR_HOST_ARB_PAR;
/// `E1000_ICS_PB_PAR`: packet buffer parity error.
pub const E1000_ICS_PB_PAR: u32 = E1000_ICR_PB_PAR;
/// `E1000_ICS_RXD_FIFO_PAR1`: queue 1 Rx descriptor FIFO parity error.
pub const E1000_ICS_RXD_FIFO_PAR1: u32 = E1000_ICR_RXD_FIFO_PAR1;
/// `E1000_ICS_TXD_FIFO_PAR1`: queue 1 Tx descriptor FIFO parity error.
pub const E1000_ICS_TXD_FIFO_PAR1: u32 = E1000_ICR_TXD_FIFO_PAR1;
/// `E1000_ICS_DSW`.
pub const E1000_ICS_DSW: u32 = E1000_ICR_DSW;
/// `E1000_ICS_PHYINT`.
pub const E1000_ICS_PHYINT: u32 = E1000_ICR_PHYINT;
/// `E1000_ICS_EPRST`.
pub const E1000_ICS_EPRST: u32 = E1000_ICR_EPRST;
/// `E1000_ICS_DRSTA`.
pub const E1000_ICS_DRSTA: u32 = E1000_ICR_DRSTA;
/// `E1000_IMS_TXDW`: Transmit desc written back.
pub const E1000_IMS_TXDW: u32 = E1000_ICR_TXDW;
/// `E1000_IMS_TXQE`: Transmit Queue empty.
pub const E1000_IMS_TXQE: u32 = E1000_ICR_TXQE;
/// `E1000_IMS_LSC`: Link Status Change.
pub const E1000_IMS_LSC: u32 = E1000_ICR_LSC;
/// `E1000_IMS_RXSEQ`: rx sequence error.
pub const E1000_IMS_RXSEQ: u32 = E1000_ICR_RXSEQ;
/// `E1000_IMS_RXDMT0`: rx desc min. threshold.
pub const E1000_IMS_RXDMT0: u32 = E1000_ICR_RXDMT0;
/// `E1000_IMS_RXO`: rx overrun.
pub const E1000_IMS_RXO: u32 = E1000_ICR_RXO;
/// `E1000_IMS_RXT0`: rx timer intr.
pub const E1000_IMS_RXT0: u32 = E1000_ICR_RXT0;
/// `E1000_IMS_MDAC`: MDIO access complete.
pub const E1000_IMS_MDAC: u32 = E1000_ICR_MDAC;
/// `E1000_IMS_RXCFG`: RX /c/ ordered set.
pub const E1000_IMS_RXCFG: u32 = E1000_ICR_RXCFG;
/// `E1000_IMS_GPI_EN0`: GP Int 0.
pub const E1000_IMS_GPI_EN0: u32 = E1000_ICR_GPI_EN0;
/// `E1000_IMS_GPI_EN1`: GP Int 1.
pub const E1000_IMS_GPI_EN1: u32 = E1000_ICR_GPI_EN1;
/// `E1000_IMS_GPI_EN2`: GP Int 2.
pub const E1000_IMS_GPI_EN2: u32 = E1000_ICR_GPI_EN2;
/// `E1000_IMS_GPI_EN3`: GP Int 3.
pub const E1000_IMS_GPI_EN3: u32 = E1000_ICR_GPI_EN3;
/// `E1000_IMS_TXD_LOW`.
pub const E1000_IMS_TXD_LOW: u32 = E1000_ICR_TXD_LOW;
/// `E1000_IMS_SRPD`.
pub const E1000_IMS_SRPD: u32 = E1000_ICR_SRPD;
/// `E1000_IMS_ACK`: Receive Ack frame.
pub const E1000_IMS_ACK: u32 = E1000_ICR_ACK;
/// `E1000_IMS_MNG`: Manageability event.
pub const E1000_IMS_MNG: u32 = E1000_ICR_MNG;
/// `E1000_IMS_DOCK`: Dock/Undock.
pub const E1000_IMS_DOCK: u32 = E1000_ICR_DOCK;
/// `E1000_IMS_RXD_FIFO_PAR0`: queue 0 Rx descriptor FIFO parity error.
pub const E1000_IMS_RXD_FIFO_PAR0: u32 = E1000_ICR_RXD_FIFO_PAR0;
/// `E1000_IMS_TXD_FIFO_PAR0`: queue 0 Tx descriptor FIFO parity error.
pub const E1000_IMS_TXD_FIFO_PAR0: u32 = E1000_ICR_TXD_FIFO_PAR0;
/// `E1000_IMS_HOST_ARB_PAR`: host arb read buffer parity error.
pub const E1000_IMS_HOST_ARB_PAR: u32 = E1000_ICR_HOST_ARB_PAR;
/// `E1000_IMS_PB_PAR`: packet buffer parity error.
pub const E1000_IMS_PB_PAR: u32 = E1000_ICR_PB_PAR;
/// `E1000_IMS_RXD_FIFO_PAR1`: queue 1 Rx descriptor FIFO parity error.
pub const E1000_IMS_RXD_FIFO_PAR1: u32 = E1000_ICR_RXD_FIFO_PAR1;
/// `E1000_IMS_TXD_FIFO_PAR1`: queue 1 Tx descriptor FIFO parity error.
pub const E1000_IMS_TXD_FIFO_PAR1: u32 = E1000_ICR_TXD_FIFO_PAR1;
/// `E1000_IMS_DSW`.
pub const E1000_IMS_DSW: u32 = E1000_ICR_DSW;
/// `E1000_IMS_PHYINT`.
pub const E1000_IMS_PHYINT: u32 = E1000_ICR_PHYINT;
/// `E1000_IMS_EPRST`.
pub const E1000_IMS_EPRST: u32 = E1000_ICR_EPRST;
/// `E1000_IMS_DRSTA`.
pub const E1000_IMS_DRSTA: u32 = E1000_ICR_DRSTA;
/// `E1000_IMC_TXDW`: Transmit desc written back.
pub const E1000_IMC_TXDW: u32 = E1000_ICR_TXDW;
/// `E1000_IMC_TXQE`: Transmit Queue empty.
pub const E1000_IMC_TXQE: u32 = E1000_ICR_TXQE;
/// `E1000_IMC_LSC`: Link Status Change.
pub const E1000_IMC_LSC: u32 = E1000_ICR_LSC;
/// `E1000_IMC_RXSEQ`: rx sequence error.
pub const E1000_IMC_RXSEQ: u32 = E1000_ICR_RXSEQ;
/// `E1000_IMC_RXDMT0`: rx desc min. threshold.
pub const E1000_IMC_RXDMT0: u32 = E1000_ICR_RXDMT0;
/// `E1000_IMC_RXO`: rx overrun.
pub const E1000_IMC_RXO: u32 = E1000_ICR_RXO;
/// `E1000_IMC_RXT0`: rx timer intr.
pub const E1000_IMC_RXT0: u32 = E1000_ICR_RXT0;
/// `E1000_IMC_MDAC`: MDIO access complete.
pub const E1000_IMC_MDAC: u32 = E1000_ICR_MDAC;
/// `E1000_IMC_RXCFG`: RX /c/ ordered set.
pub const E1000_IMC_RXCFG: u32 = E1000_ICR_RXCFG;
/// `E1000_IMC_GPI_EN0`: GP Int 0.
pub const E1000_IMC_GPI_EN0: u32 = E1000_ICR_GPI_EN0;
/// `E1000_IMC_GPI_EN1`: GP Int 1.
pub const E1000_IMC_GPI_EN1: u32 = E1000_ICR_GPI_EN1;
/// `E1000_IMC_GPI_EN2`: GP Int 2.
pub const E1000_IMC_GPI_EN2: u32 = E1000_ICR_GPI_EN2;
/// `E1000_IMC_GPI_EN3`: GP Int 3.
pub const E1000_IMC_GPI_EN3: u32 = E1000_ICR_GPI_EN3;
/// `E1000_IMC_TXD_LOW`.
pub const E1000_IMC_TXD_LOW: u32 = E1000_ICR_TXD_LOW;
/// `E1000_IMC_SRPD`.
pub const E1000_IMC_SRPD: u32 = E1000_ICR_SRPD;
/// `E1000_IMC_ACK`: Receive Ack frame.
pub const E1000_IMC_ACK: u32 = E1000_ICR_ACK;
/// `E1000_IMC_MNG`: Manageability event.
pub const E1000_IMC_MNG: u32 = E1000_ICR_MNG;
/// `E1000_IMC_DOCK`: Dock/Undock.
pub const E1000_IMC_DOCK: u32 = E1000_ICR_DOCK;
/// `E1000_IMC_RXD_FIFO_PAR0`: queue 0 Rx descriptor FIFO parity error.
pub const E1000_IMC_RXD_FIFO_PAR0: u32 = E1000_ICR_RXD_FIFO_PAR0;
/// `E1000_IMC_TXD_FIFO_PAR0`: queue 0 Tx descriptor FIFO parity error.
pub const E1000_IMC_TXD_FIFO_PAR0: u32 = E1000_ICR_TXD_FIFO_PAR0;
/// `E1000_IMC_HOST_ARB_PAR`: host arb read buffer parity error.
pub const E1000_IMC_HOST_ARB_PAR: u32 = E1000_ICR_HOST_ARB_PAR;
/// `E1000_IMC_PB_PAR`: packet buffer parity error.
pub const E1000_IMC_PB_PAR: u32 = E1000_ICR_PB_PAR;
/// `E1000_IMC_RXD_FIFO_PAR1`: queue 1 Rx descriptor FIFO parity error.
pub const E1000_IMC_RXD_FIFO_PAR1: u32 = E1000_ICR_RXD_FIFO_PAR1;
/// `E1000_IMC_TXD_FIFO_PAR1`: queue 1 Tx descriptor FIFO parity error.
pub const E1000_IMC_TXD_FIFO_PAR1: u32 = E1000_ICR_TXD_FIFO_PAR1;
/// `E1000_IMC_DSW`.
pub const E1000_IMC_DSW: u32 = E1000_ICR_DSW;
/// `E1000_IMC_PHYINT`.
pub const E1000_IMC_PHYINT: u32 = E1000_ICR_PHYINT;
/// `E1000_IMC_EPRST`.
pub const E1000_IMC_EPRST: u32 = E1000_ICR_EPRST;
/// `E1000_IMC_DRSTA`.
pub const E1000_IMC_DRSTA: u32 = E1000_ICR_DRSTA;
/// `E1000_RCTL_RST`: Software reset.
pub const E1000_RCTL_RST: u32 = 0x00000001;
/// `E1000_RCTL_EN`: enable.
pub const E1000_RCTL_EN: u32 = 0x00000002;
/// `E1000_RCTL_SBP`: store bad packet.
pub const E1000_RCTL_SBP: u32 = 0x00000004;
/// `E1000_RCTL_UPE`: unicast promiscuous enable.
pub const E1000_RCTL_UPE: u32 = 0x00000008;
/// `E1000_RCTL_MPE`: multicast promiscuous enab.
pub const E1000_RCTL_MPE: u32 = 0x00000010;
/// `E1000_RCTL_LPE`: long packet enable.
pub const E1000_RCTL_LPE: u32 = 0x00000020;
/// `E1000_RCTL_LBM_NO`: no loopback mode.
pub const E1000_RCTL_LBM_NO: u32 = 0x00000000;
/// `E1000_RCTL_LBM_MAC`: MAC loopback mode.
pub const E1000_RCTL_LBM_MAC: u32 = 0x00000040;
/// `E1000_RCTL_LBM_SLP`: serial link loopback mode.
pub const E1000_RCTL_LBM_SLP: u32 = 0x00000080;
/// `E1000_RCTL_LBM_TCVR`: tcvr loopback mode.
pub const E1000_RCTL_LBM_TCVR: u32 = 0x000000C0;
/// `E1000_RCTL_DTYP_MASK`: Descriptor type mask.
pub const E1000_RCTL_DTYP_MASK: u32 = 0x00000C00;
/// `E1000_RCTL_DTYP_PS`: Packet Split descriptor.
pub const E1000_RCTL_DTYP_PS: u32 = 0x00000400;
/// `E1000_RCTL_RDMTS_HALF`: rx desc min threshold size.
pub const E1000_RCTL_RDMTS_HALF: u32 = 0x00000000;
/// `E1000_RCTL_RDMTS_QUAT`: rx desc min threshold size.
pub const E1000_RCTL_RDMTS_QUAT: u32 = 0x00000100;
/// `E1000_RCTL_RDMTS_EIGTH`: rx desc min threshold size.
pub const E1000_RCTL_RDMTS_EIGTH: u32 = 0x00000200;
/// `E1000_RCTL_RDMTS_HEX`.
pub const E1000_RCTL_RDMTS_HEX: u32 = 0x00010000;
/// `E1000_RCTL_MO_SHIFT`: multicast offset shift.
pub const E1000_RCTL_MO_SHIFT: u32 = 12;
/// `E1000_RCTL_MO_0`: multicast offset 11:0.
pub const E1000_RCTL_MO_0: u32 = 0x00000000;
/// `E1000_RCTL_MO_1`: multicast offset 12:1.
pub const E1000_RCTL_MO_1: u32 = 0x00001000;
/// `E1000_RCTL_MO_2`: multicast offset 13:2.
pub const E1000_RCTL_MO_2: u32 = 0x00002000;
/// `E1000_RCTL_MO_3`: multicast offset 15:4.
pub const E1000_RCTL_MO_3: u32 = 0x00003000;
/// `E1000_RCTL_MDR`: multicast desc ring 0.
pub const E1000_RCTL_MDR: u32 = 0x00004000;
/// `E1000_RCTL_BAM`: broadcast enable.
pub const E1000_RCTL_BAM: u32 = 0x00008000;
/// `E1000_RCTL_SZ_2048`: rx buffer size 2048.
pub const E1000_RCTL_SZ_2048: u32 = 0x00000000;
/// `E1000_RCTL_SZ_1024`: rx buffer size 1024.
pub const E1000_RCTL_SZ_1024: u32 = 0x00010000;
/// `E1000_RCTL_SZ_512`: rx buffer size 512.
pub const E1000_RCTL_SZ_512: u32 = 0x00020000;
/// `E1000_RCTL_SZ_256`: rx buffer size 256.
pub const E1000_RCTL_SZ_256: u32 = 0x00030000;
/// `E1000_RCTL_SZ_16384`: rx buffer size 16384.
pub const E1000_RCTL_SZ_16384: u32 = 0x00010000;
/// `E1000_RCTL_SZ_8192`: rx buffer size 8192.
pub const E1000_RCTL_SZ_8192: u32 = 0x00020000;
/// `E1000_RCTL_SZ_4096`: rx buffer size 4096.
pub const E1000_RCTL_SZ_4096: u32 = 0x00030000;
/// `E1000_RCTL_VFE`: vlan filter enable.
pub const E1000_RCTL_VFE: u32 = 0x00040000;
/// `E1000_RCTL_CFIEN`: canonical form enable.
pub const E1000_RCTL_CFIEN: u32 = 0x00080000;
/// `E1000_RCTL_CFI`: canonical form indicator.
pub const E1000_RCTL_CFI: u32 = 0x00100000;
/// `E1000_RCTL_DPF`: discard pause frames.
pub const E1000_RCTL_DPF: u32 = 0x00400000;
/// `E1000_RCTL_PMCF`: pass MAC control frames.
pub const E1000_RCTL_PMCF: u32 = 0x00800000;
/// `E1000_RCTL_BSEX`: Buffer size extension.
pub const E1000_RCTL_BSEX: u32 = 0x02000000;
/// `E1000_RCTL_SECRC`: Strip Ethernet CRC.
pub const E1000_RCTL_SECRC: u32 = 0x04000000;
/// `E1000_RCTL_FLXBUF_MASK`: Flexible buffer size.
pub const E1000_RCTL_FLXBUF_MASK: u32 = 0x78000000;
/// `E1000_RCTL_FLXBUF_SHIFT`: Flexible buffer shift.
pub const E1000_RCTL_FLXBUF_SHIFT: u32 = 27;
/// `E1000_PSRCTL_BSIZE0_MASK`.
pub const E1000_PSRCTL_BSIZE0_MASK: u32 = 0x0000007F;
/// `E1000_PSRCTL_BSIZE1_MASK`.
pub const E1000_PSRCTL_BSIZE1_MASK: u32 = 0x00003F00;
/// `E1000_PSRCTL_BSIZE2_MASK`.
pub const E1000_PSRCTL_BSIZE2_MASK: u32 = 0x003F0000;
/// `E1000_PSRCTL_BSIZE3_MASK`.
pub const E1000_PSRCTL_BSIZE3_MASK: u32 = 0x3F000000;
/// `E1000_PSRCTL_BSIZE0_SHIFT`: Shift _right_ 7.
pub const E1000_PSRCTL_BSIZE0_SHIFT: u32 = 7;
/// `E1000_PSRCTL_BSIZE1_SHIFT`: Shift _right_ 2.
pub const E1000_PSRCTL_BSIZE1_SHIFT: u32 = 2;
/// `E1000_PSRCTL_BSIZE2_SHIFT`: Shift _left_ 6.
pub const E1000_PSRCTL_BSIZE2_SHIFT: u32 = 6;
/// `E1000_PSRCTL_BSIZE3_SHIFT`: Shift _left_ 14.
pub const E1000_PSRCTL_BSIZE3_SHIFT: u32 = 14;
/// `E1000_SWFW_EEP_SM`.
pub const E1000_SWFW_EEP_SM: u16 = 0x0001;
/// `E1000_SWFW_PHY0_SM`.
pub const E1000_SWFW_PHY0_SM: u16 = 0x0002;
/// `E1000_SWFW_PHY1_SM`.
pub const E1000_SWFW_PHY1_SM: u16 = 0x0004;
/// `E1000_SWFW_MAC_CSR_SM`.
pub const E1000_SWFW_MAC_CSR_SM: u16 = 0x0008;
/// `E1000_SWFW_PHY2_SM`.
pub const E1000_SWFW_PHY2_SM: u16 = 0x0020;
/// `E1000_SWFW_PHY3_SM`.
pub const E1000_SWFW_PHY3_SM: u16 = 0x0040;
/// `E1000_RDT_DELAY`: Delay timer (1=1024us).
pub const E1000_RDT_DELAY: u32 = 0x0000ffff;
/// `E1000_RDT_FPDB`: Flush descriptor block.
pub const E1000_RDT_FPDB: u32 = 0x80000000;
/// `E1000_RDLEN_LEN`: descriptor length.
pub const E1000_RDLEN_LEN: u32 = 0x0007ff80;
/// `E1000_RDH_RDH`: receive descriptor head.
pub const E1000_RDH_RDH: u32 = 0x0000ffff;
/// `E1000_RDT_RDT`: receive descriptor tail.
pub const E1000_RDT_RDT: u32 = 0x0000ffff;
/// `E1000_FCRTH_RTH`: Mask Bits[15:3] for RTH.
pub const E1000_FCRTH_RTH: u32 = 0x0000FFF8;
/// `E1000_FCRTH_XFCE`: External Flow Control Enable.
pub const E1000_FCRTH_XFCE: u32 = 0x80000000;
/// `E1000_FCRTL_RTL`: Mask Bits[15:3] for RTL.
pub const E1000_FCRTL_RTL: u32 = 0x0000FFF8;
/// `E1000_FCRTL_XONE`: Enable XON frame transmission.
pub const E1000_FCRTL_XONE: u32 = 0x80000000;
/// `E1000_FC_NONE`.
pub const E1000_FC_NONE: u32 = 0;
/// `E1000_FC_RX_PAUSE`.
pub const E1000_FC_RX_PAUSE: u32 = 1;
/// `E1000_FC_TX_PAUSE`.
pub const E1000_FC_TX_PAUSE: u32 = 2;
/// `E1000_FC_FULL`.
pub const E1000_FC_FULL: u32 = 3;
/// `E1000_FC_DEFAULT`.
pub const E1000_FC_DEFAULT: u32 = 0xFF;
/// `E1000_RFCTL_ISCSI_DIS`.
pub const E1000_RFCTL_ISCSI_DIS: u32 = 0x00000001;
/// `E1000_RFCTL_ISCSI_DWC_MASK`.
pub const E1000_RFCTL_ISCSI_DWC_MASK: u32 = 0x0000003E;
/// `E1000_RFCTL_ISCSI_DWC_SHIFT`.
pub const E1000_RFCTL_ISCSI_DWC_SHIFT: u32 = 1;
/// `E1000_RFCTL_NFSW_DIS`.
pub const E1000_RFCTL_NFSW_DIS: u32 = 0x00000040;
/// `E1000_RFCTL_NFSR_DIS`.
pub const E1000_RFCTL_NFSR_DIS: u32 = 0x00000080;
/// `E1000_RFCTL_NFS_VER_MASK`.
pub const E1000_RFCTL_NFS_VER_MASK: u32 = 0x00000300;
/// `E1000_RFCTL_NFS_VER_SHIFT`.
pub const E1000_RFCTL_NFS_VER_SHIFT: u32 = 8;
/// `E1000_RFCTL_IPV6_DIS`.
pub const E1000_RFCTL_IPV6_DIS: u32 = 0x00000400;
/// `E1000_RFCTL_IPV6_XSUM_DIS`.
pub const E1000_RFCTL_IPV6_XSUM_DIS: u32 = 0x00000800;
/// `E1000_RFCTL_ACK_DIS`.
pub const E1000_RFCTL_ACK_DIS: u32 = 0x00001000;
/// `E1000_RFCTL_ACKD_DIS`.
pub const E1000_RFCTL_ACKD_DIS: u32 = 0x00002000;
/// `E1000_RFCTL_IPFRSP_DIS`.
pub const E1000_RFCTL_IPFRSP_DIS: u32 = 0x00004000;
/// `E1000_RFCTL_EXTEN`.
pub const E1000_RFCTL_EXTEN: u32 = 0x00008000;
/// `E1000_RFCTL_IPV6_EX_DIS`.
pub const E1000_RFCTL_IPV6_EX_DIS: u32 = 0x00010000;
/// `E1000_RFCTL_NEW_IPV6_EXT_DIS`.
pub const E1000_RFCTL_NEW_IPV6_EXT_DIS: u32 = 0x00020000;
/// `E1000_RXDCTL_PTHRESH`: RXDCTL Prefetch Threshold.
pub const E1000_RXDCTL_PTHRESH: u32 = 0x0000003F;
/// `E1000_RXDCTL_HTHRESH`: RXDCTL Host Threshold.
pub const E1000_RXDCTL_HTHRESH: u32 = 0x00003F00;
/// `E1000_RXDCTL_WTHRESH`: RXDCTL Writeback Threshold.
pub const E1000_RXDCTL_WTHRESH: u32 = 0x003F0000;
/// `E1000_RXDCTL_THRESH_UNIT_DESC`.
pub const E1000_RXDCTL_THRESH_UNIT_DESC: u32 = 0x1000000;
/// `E1000_RXDCTL_QUEUE_ENABLE`.
pub const E1000_RXDCTL_QUEUE_ENABLE: u32 = 0x2000000;
/// `E1000_EITR_ITR_INT_MASK`.
pub const E1000_EITR_ITR_INT_MASK: u32 = 0x0000FFFF;
/// `E1000_EITR_CNT_IGNR`: Don't reset counters on write.
pub const E1000_EITR_CNT_IGNR: u32 = 0x80000000;
/// `E1000_EITR_INTERVAL`.
pub const E1000_EITR_INTERVAL: u32 = 0x00007FFC;
/// `E1000_TXDCTL_PTHRESH`: TXDCTL Prefetch Threshold.
pub const E1000_TXDCTL_PTHRESH: u32 = 0x000000FF;
/// `E1000_TXDCTL_HTHRESH`: TXDCTL Host Threshold.
pub const E1000_TXDCTL_HTHRESH: u32 = 0x0000FF00;
/// `E1000_TXDCTL_WTHRESH`: TXDCTL Writeback Threshold.
pub const E1000_TXDCTL_WTHRESH: u32 = 0x00FF0000;
/// `E1000_TXDCTL_GRAN`: TXDCTL Granularity.
pub const E1000_TXDCTL_GRAN: u32 = 0x01000000;
/// `E1000_TXDCTL_LWTHRESH`: TXDCTL Low Threshold.
pub const E1000_TXDCTL_LWTHRESH: u32 = 0xFE000000;
/// `E1000_TXDCTL_FULL_TX_DESC_WB`: GRAN=1, WTHRESH=1.
pub const E1000_TXDCTL_FULL_TX_DESC_WB: u32 = 0x01010000;
/// `E1000_TXDCTL_COUNT_DESC`: Enable the counting of desc. still to be processed.
pub const E1000_TXDCTL_COUNT_DESC: u32 = 0x00400000;
/// `E1000_TXDCTL_QUEUE_ENABLE`.
pub const E1000_TXDCTL_QUEUE_ENABLE: u32 = 0x02000000;
/// `E1000_TXCW_FD`: TXCW full duplex.
pub const E1000_TXCW_FD: u32 = 0x00000020;
/// `E1000_TXCW_HD`: TXCW half duplex.
pub const E1000_TXCW_HD: u32 = 0x00000040;
/// `E1000_TXCW_PAUSE`: TXCW sym pause request.
pub const E1000_TXCW_PAUSE: u32 = 0x00000080;
/// `E1000_TXCW_ASM_DIR`: TXCW astm pause direction.
pub const E1000_TXCW_ASM_DIR: u32 = 0x00000100;
/// `E1000_TXCW_PAUSE_MASK`: TXCW pause request mask.
pub const E1000_TXCW_PAUSE_MASK: u32 = 0x00000180;
/// `E1000_TXCW_RF`: TXCW remote fault.
pub const E1000_TXCW_RF: u32 = 0x00003000;
/// `E1000_TXCW_NP`: TXCW next page.
pub const E1000_TXCW_NP: u32 = 0x00008000;
/// `E1000_TXCW_CW`: TxConfigWord mask.
pub const E1000_TXCW_CW: u32 = 0x0000ffff;
/// `E1000_TXCW_TXC`: Transmit Config control.
pub const E1000_TXCW_TXC: u32 = 0x40000000;
/// `E1000_TXCW_ANE`: Auto-neg enable.
pub const E1000_TXCW_ANE: u32 = 0x80000000;
/// `E1000_RXCW_CW`: RxConfigWord mask.
pub const E1000_RXCW_CW: u32 = 0x0000ffff;
/// `E1000_RXCW_NC`: Receive config no carrier.
pub const E1000_RXCW_NC: u32 = 0x04000000;
/// `E1000_RXCW_IV`: Receive config invalid.
pub const E1000_RXCW_IV: u32 = 0x08000000;
/// `E1000_RXCW_CC`: Receive config change.
pub const E1000_RXCW_CC: u32 = 0x10000000;
/// `E1000_RXCW_C`: Receive config.
pub const E1000_RXCW_C: u32 = 0x20000000;
/// `E1000_RXCW_SYNCH`: Receive config synch.
pub const E1000_RXCW_SYNCH: u32 = 0x40000000;
/// `E1000_RXCW_ANC`: Auto-neg complete.
pub const E1000_RXCW_ANC: u32 = 0x80000000;
/// `E1000_TCTL_RST`: software reset.
pub const E1000_TCTL_RST: u32 = 0x00000001;
/// `E1000_TCTL_EN`: enable tx.
pub const E1000_TCTL_EN: u32 = 0x00000002;
/// `E1000_TCTL_BCE`: busy check enable.
pub const E1000_TCTL_BCE: u32 = 0x00000004;
/// `E1000_TCTL_PSP`: pad short packets.
pub const E1000_TCTL_PSP: u32 = 0x00000008;
/// `E1000_TCTL_CT`: collision threshold.
pub const E1000_TCTL_CT: u32 = 0x00000ff0;
/// `E1000_TCTL_COLD`: collision distance.
pub const E1000_TCTL_COLD: u32 = 0x003ff000;
/// `E1000_TCTL_SWXOFF`: SW Xoff transmission.
pub const E1000_TCTL_SWXOFF: u32 = 0x00400000;
/// `E1000_TCTL_PBE`: Packet Burst Enable.
pub const E1000_TCTL_PBE: u32 = 0x00800000;
/// `E1000_TCTL_RTLC`: Re-transmit on late collision.
pub const E1000_TCTL_RTLC: u32 = 0x01000000;
/// `E1000_TCTL_NRTU`: No Re-transmit on underrun.
pub const E1000_TCTL_NRTU: u32 = 0x02000000;
/// `E1000_TCTL_MULR`: Multiple request support.
pub const E1000_TCTL_MULR: u32 = 0x10000000;
/// `E1000_TCTL_EXT_BST_MASK`: Backoff Slot Time.
pub const E1000_TCTL_EXT_BST_MASK: u32 = 0x000003FF;
/// `E1000_TCTL_EXT_GCEX_MASK`: Gigabit Carry Extend Padding.
pub const E1000_TCTL_EXT_GCEX_MASK: u32 = 0x000FFC00;
/// `DEFAULT_80003ES2LAN_TCTL_EXT_GCEX`.
pub const DEFAULT_80003ES2LAN_TCTL_EXT_GCEX: u32 = 0x00010000;
/// `E1000_RXCSUM_PCSS_MASK`: Packet Checksum Start.
pub const E1000_RXCSUM_PCSS_MASK: u32 = 0x000000FF;
/// `E1000_RXCSUM_IPOFL`: IPv4 checksum offload.
pub const E1000_RXCSUM_IPOFL: u32 = 0x00000100;
/// `E1000_RXCSUM_TUOFL`: TCP / UDP checksum offload.
pub const E1000_RXCSUM_TUOFL: u32 = 0x00000200;
/// `E1000_RXCSUM_IPV6OFL`: IPv6 checksum offload.
pub const E1000_RXCSUM_IPV6OFL: u32 = 0x00000400;
/// `E1000_RXCSUM_IPPCSE`: IP payload checksum enable.
pub const E1000_RXCSUM_IPPCSE: u32 = 0x00001000;
/// `E1000_RXCSUM_PCSD`: packet checksum disabled.
pub const E1000_RXCSUM_PCSD: u32 = 0x00002000;
/// `E1000_ADVTXD_DTYP_CTXT`: Advanced Context Descriptor.
pub const E1000_ADVTXD_DTYP_CTXT: u32 = 0x00200000;
/// `E1000_ADVTXD_DTYP_DATA`: Advanced Data Descriptor.
pub const E1000_ADVTXD_DTYP_DATA: u32 = 0x00300000;
/// `E1000_ADVTXD_DCMD_IFCS`: Insert FCS (Ethernet CRC).
pub const E1000_ADVTXD_DCMD_IFCS: u32 = 0x02000000;
/// `E1000_ADVTXD_DCMD_DEXT`: Descriptor extension (1=Adv).
pub const E1000_ADVTXD_DCMD_DEXT: u32 = 0x20000000;
/// `E1000_ADVTXD_DCMD_VLE`: VLAN pkt enable.
pub const E1000_ADVTXD_DCMD_VLE: u32 = 0x40000000;
/// `E1000_ADVTXD_DCMD_TSE`: TCP Seg enable.
pub const E1000_ADVTXD_DCMD_TSE: u32 = 0x80000000;
/// `E1000_ADVTXD_PAYLEN_SHIFT`: Adv desc PAYLEN shift.
pub const E1000_ADVTXD_PAYLEN_SHIFT: u32 = 14;
/// `E1000_ADVTXD_MACLEN_SHIFT`: Adv ctxt desc mac len shift.
pub const E1000_ADVTXD_MACLEN_SHIFT: u32 = 9;
/// `E1000_ADVTXD_VLAN_SHIFT`: Adv ctxt vlan tag shift.
pub const E1000_ADVTXD_VLAN_SHIFT: u32 = 16;
/// `E1000_ADVTXD_TUCMD_IPV4`: IP Packet Type: 1=IPv4.
pub const E1000_ADVTXD_TUCMD_IPV4: u32 = 0x00000400;
/// `E1000_ADVTXD_TUCMD_IPV6`: IP Packet Type: 0=IPv6.
pub const E1000_ADVTXD_TUCMD_IPV6: u32 = 0x00000000;
/// `E1000_ADVTXD_TUCMD_L4T_UDP`: L4 Packet TYPE of UDP.
pub const E1000_ADVTXD_TUCMD_L4T_UDP: u32 = 0x00000000;
/// `E1000_ADVTXD_TUCMD_L4T_TCP`: L4 Packet TYPE of TCP.
pub const E1000_ADVTXD_TUCMD_L4T_TCP: u32 = 0x00000800;
/// `E1000_ADVTXD_L4LEN_SHIFT`: Adv ctxt L4LEN shift.
pub const E1000_ADVTXD_L4LEN_SHIFT: u32 = 8;
/// `E1000_ADVTXD_MSS_SHIFT`: Adv ctxt MSS shift.
pub const E1000_ADVTXD_MSS_SHIFT: u32 = 16;
/// `E1000_MRQC_ENABLE_MASK`.
pub const E1000_MRQC_ENABLE_MASK: u32 = 0x00000003;
/// `E1000_MRQC_ENABLE_RSS_2Q`.
pub const E1000_MRQC_ENABLE_RSS_2Q: u32 = 0x00000001;
/// `E1000_MRQC_ENABLE_RSS_INT`.
pub const E1000_MRQC_ENABLE_RSS_INT: u32 = 0x00000004;
/// `E1000_MRQC_RSS_FIELD_MASK`.
pub const E1000_MRQC_RSS_FIELD_MASK: u32 = 0xFFFF0000;
/// `E1000_MRQC_RSS_FIELD_IPV4_TCP`.
pub const E1000_MRQC_RSS_FIELD_IPV4_TCP: u32 = 0x00010000;
/// `E1000_MRQC_RSS_FIELD_IPV4`.
pub const E1000_MRQC_RSS_FIELD_IPV4: u32 = 0x00020000;
/// `E1000_MRQC_RSS_FIELD_IPV6_TCP_EX`.
pub const E1000_MRQC_RSS_FIELD_IPV6_TCP_EX: u32 = 0x00040000;
/// `E1000_MRQC_RSS_FIELD_IPV6_EX`.
pub const E1000_MRQC_RSS_FIELD_IPV6_EX: u32 = 0x00080000;
/// `E1000_MRQC_RSS_FIELD_IPV6`.
pub const E1000_MRQC_RSS_FIELD_IPV6: u32 = 0x00100000;
/// `E1000_MRQC_RSS_FIELD_IPV6_TCP`.
pub const E1000_MRQC_RSS_FIELD_IPV6_TCP: u32 = 0x00200000;
/// `E1000_WUC_APME`: APM Enable.
pub const E1000_WUC_APME: u32 = 0x00000001;
/// `E1000_WUC_PME_EN`: PME Enable.
pub const E1000_WUC_PME_EN: u32 = 0x00000002;
/// `E1000_WUC_PME_STATUS`: PME Status.
pub const E1000_WUC_PME_STATUS: u32 = 0x00000004;
/// `E1000_WUC_APMPME`: Assert PME on APM Wakeup.
pub const E1000_WUC_APMPME: u32 = 0x00000008;
/// `E1000_WUC_SPM`: Enable SPM.
pub const E1000_WUC_SPM: u32 = 0x80000000;
/// `E1000_FHFT(_n)`: flexible host filter table.
pub const fn e1000_fhft(n: u32) -> u32 {
    0x09000 + (n * 0x100)
}
/// `E1000_FHFT_EXT(_n)`: extended flexible host filter table.
pub const fn e1000_fhft_ext(n: u32) -> u32 {
    0x09A00 + (n * 0x100)
}
/// `E1000_WUFC_LNKC`: Link Status Change Wakeup Enable.
pub const E1000_WUFC_LNKC: u32 = 0x00000001;
/// `E1000_WUFC_MAG`: Magic Packet Wakeup Enable.
pub const E1000_WUFC_MAG: u32 = 0x00000002;
/// `E1000_WUFC_EX`: Directed Exact Wakeup Enable.
pub const E1000_WUFC_EX: u32 = 0x00000004;
/// `E1000_WUFC_MC`: Directed Multicast Wakeup Enable.
pub const E1000_WUFC_MC: u32 = 0x00000008;
/// `E1000_WUFC_BC`: Broadcast Wakeup Enable.
pub const E1000_WUFC_BC: u32 = 0x00000010;
/// `E1000_WUFC_ARP`: ARP Request Packet Wakeup Enable.
pub const E1000_WUFC_ARP: u32 = 0x00000020;
/// `E1000_WUFC_IPV4`: Directed IPv4 Packet Wakeup Enable.
pub const E1000_WUFC_IPV4: u32 = 0x00000040;
/// `E1000_WUFC_IPV6`: Directed IPv6 Packet Wakeup Enable.
pub const E1000_WUFC_IPV6: u32 = 0x00000080;
/// `E1000_WUFC_IGNORE_TCO`: Ignore WakeOn TCO packets.
pub const E1000_WUFC_IGNORE_TCO: u32 = 0x00008000;
/// `E1000_WUFC_FLX0`: Flexible Filter 0 Enable.
pub const E1000_WUFC_FLX0: u32 = 0x00010000;
/// `E1000_WUFC_FLX1`: Flexible Filter 1 Enable.
pub const E1000_WUFC_FLX1: u32 = 0x00020000;
/// `E1000_WUFC_FLX2`: Flexible Filter 2 Enable.
pub const E1000_WUFC_FLX2: u32 = 0x00040000;
/// `E1000_WUFC_FLX3`: Flexible Filter 3 Enable.
pub const E1000_WUFC_FLX3: u32 = 0x00080000;
/// `E1000_WUFC_ALL_FILTERS`: Mask for all wakeup filters.
pub const E1000_WUFC_ALL_FILTERS: u32 = 0x000F00FF;
/// `E1000_WUFC_FLX_OFFSET`: Offset to the Flexible Filters bits.
pub const E1000_WUFC_FLX_OFFSET: u32 = 16;
/// `E1000_WUFC_FLX_FILTERS`: Mask for the 4 flexible filters.
pub const E1000_WUFC_FLX_FILTERS: u32 = 0x000F0000;
/// `E1000_WUS_LNKC`: Link Status Changed.
pub const E1000_WUS_LNKC: u32 = 0x00000001;
/// `E1000_WUS_MAG`: Magic Packet Received.
pub const E1000_WUS_MAG: u32 = 0x00000002;
/// `E1000_WUS_EX`: Directed Exact Received.
pub const E1000_WUS_EX: u32 = 0x00000004;
/// `E1000_WUS_MC`: Directed Multicast Received.
pub const E1000_WUS_MC: u32 = 0x00000008;
/// `E1000_WUS_BC`: Broadcast Received.
pub const E1000_WUS_BC: u32 = 0x00000010;
/// `E1000_WUS_ARP`: ARP Request Packet Received.
pub const E1000_WUS_ARP: u32 = 0x00000020;
/// `E1000_WUS_IPV4`: Directed IPv4 Packet Wakeup Received.
pub const E1000_WUS_IPV4: u32 = 0x00000040;
/// `E1000_WUS_IPV6`: Directed IPv6 Packet Wakeup Received.
pub const E1000_WUS_IPV6: u32 = 0x00000080;
/// `E1000_WUS_FLX0`: Flexible Filter 0 Match.
pub const E1000_WUS_FLX0: u32 = 0x00010000;
/// `E1000_WUS_FLX1`: Flexible Filter 1 Match.
pub const E1000_WUS_FLX1: u32 = 0x00020000;
/// `E1000_WUS_FLX2`: Flexible Filter 2 Match.
pub const E1000_WUS_FLX2: u32 = 0x00040000;
/// `E1000_WUS_FLX3`: Flexible Filter 3 Match.
pub const E1000_WUS_FLX3: u32 = 0x00080000;
/// `E1000_WUS_FLX_FILTERS`: Mask for the 4 flexible filters.
pub const E1000_WUS_FLX_FILTERS: u32 = 0x000F0000;
/// `E1000_TARC0_CB_MULTIQ_2_REQ`.
pub const E1000_TARC0_CB_MULTIQ_2_REQ: u32 = 1 << 29;
/// `E1000_TARC0_CB_MULTIQ_3_REQ`.
pub const E1000_TARC0_CB_MULTIQ_3_REQ: u32 = 1 << 28 | 1 << 29;
/// `E1000_MANC_SMBUS_EN`: SMBus Enabled - RO.
pub const E1000_MANC_SMBUS_EN: u32 = 0x00000001;
/// `E1000_MANC_ASF_EN`: ASF Enabled - RO.
pub const E1000_MANC_ASF_EN: u32 = 0x00000002;
/// `E1000_MANC_R_ON_FORCE`: Reset on Force TCO - RO.
pub const E1000_MANC_R_ON_FORCE: u32 = 0x00000004;
/// `E1000_MANC_RMCP_EN`: Enable RCMP 026Fh Filtering.
pub const E1000_MANC_RMCP_EN: u32 = 0x00000100;
/// `E1000_MANC_0298_EN`: Enable RCMP 0298h Filtering.
pub const E1000_MANC_0298_EN: u32 = 0x00000200;
/// `E1000_MANC_IPV4_EN`: Enable IPv4.
pub const E1000_MANC_IPV4_EN: u32 = 0x00000400;
/// `E1000_MANC_IPV6_EN`: Enable IPv6.
pub const E1000_MANC_IPV6_EN: u32 = 0x00000800;
/// `E1000_MANC_SNAP_EN`: Accept LLC/SNAP.
pub const E1000_MANC_SNAP_EN: u32 = 0x00001000;
/// `E1000_MANC_ARP_EN`: Enable ARP Request Filtering.
pub const E1000_MANC_ARP_EN: u32 = 0x00002000;
/// `E1000_MANC_NEIGHBOR_EN`: Enable Neighbor Discovery Filtering.
pub const E1000_MANC_NEIGHBOR_EN: u32 = 0x00004000;
/// `E1000_MANC_ARP_RES_EN`: Enable ARP response Filtering.
pub const E1000_MANC_ARP_RES_EN: u32 = 0x00008000;
/// `E1000_MANC_TCO_RESET`: TCO Reset Occurred.
pub const E1000_MANC_TCO_RESET: u32 = 0x00010000;
/// `E1000_MANC_RCV_TCO_EN`: Receive TCO Packets Enabled.
pub const E1000_MANC_RCV_TCO_EN: u32 = 0x00020000;
/// `E1000_MANC_REPORT_STATUS`: Status Reporting Enabled.
pub const E1000_MANC_REPORT_STATUS: u32 = 0x00040000;
/// `E1000_MANC_RCV_ALL`: Receive All Enabled.
pub const E1000_MANC_RCV_ALL: u32 = 0x00080000;
/// `E1000_MANC_BLK_PHY_RST_ON_IDE`: Block phy resets.
pub const E1000_MANC_BLK_PHY_RST_ON_IDE: u32 = 0x00040000;
/// `E1000_MANC_EN_MAC_ADDR_FILTER`: Enable MAC address filtering.
pub const E1000_MANC_EN_MAC_ADDR_FILTER: u32 = 0x00100000;
/// `E1000_MANC_EN_MNG2HOST`: Enable MNG packets to host memory.
pub const E1000_MANC_EN_MNG2HOST: u32 = 0x00200000;
/// `E1000_MANC_EN_IP_ADDR_FILTER`: Enable IP address filtering.
pub const E1000_MANC_EN_IP_ADDR_FILTER: u32 = 0x00400000;
/// `E1000_MANC_EN_XSUM_FILTER`: Enable checksum filtering.
pub const E1000_MANC_EN_XSUM_FILTER: u32 = 0x00800000;
/// `E1000_MANC_BR_EN`: Enable broadcast filtering.
pub const E1000_MANC_BR_EN: u32 = 0x01000000;
/// `E1000_MANC_SMB_REQ`: SMBus Request.
pub const E1000_MANC_SMB_REQ: u32 = 0x01000000;
/// `E1000_MANC_SMB_GNT`: SMBus Grant.
pub const E1000_MANC_SMB_GNT: u32 = 0x02000000;
/// `E1000_MANC_SMB_CLK_IN`: SMBus Clock In.
pub const E1000_MANC_SMB_CLK_IN: u32 = 0x04000000;
/// `E1000_MANC_SMB_DATA_IN`: SMBus Data In.
pub const E1000_MANC_SMB_DATA_IN: u32 = 0x08000000;
/// `E1000_MANC_SMB_DATA_OUT`: SMBus Data Out.
pub const E1000_MANC_SMB_DATA_OUT: u32 = 0x10000000;
/// `E1000_MANC_SMB_CLK_OUT`: SMBus Clock Out.
pub const E1000_MANC_SMB_CLK_OUT: u32 = 0x20000000;
/// `E1000_MANC_SMB_DATA_OUT_SHIFT`: SMBus Data Out Shift.
pub const E1000_MANC_SMB_DATA_OUT_SHIFT: u32 = 28;
/// `E1000_MANC_SMB_CLK_OUT_SHIFT`: SMBus Clock Out Shift.
pub const E1000_MANC_SMB_CLK_OUT_SHIFT: u32 = 29;
/// `E1000_SWSM_SMBI`: Driver Semaphore bit.
pub const E1000_SWSM_SMBI: u32 = 0x00000001;
/// `E1000_SWSM_SWESMBI`: FW Semaphore bit.
pub const E1000_SWSM_SWESMBI: u32 = 0x00000002;
/// `E1000_SWSM_WMNG`: Wake MNG Clock.
pub const E1000_SWSM_WMNG: u32 = 0x00000004;
/// `E1000_SWSM_DRV_LOAD`: Driver Loaded Bit.
pub const E1000_SWSM_DRV_LOAD: u32 = 0x00000008;
/// `E1000_H2ME_ULP`: ULP Indication Bit.
pub const E1000_H2ME_ULP: u32 = 0x00000800;
/// `E1000_H2ME_ENFORCE_SETTINGS`: Enforce Settings.
pub const E1000_H2ME_ENFORCE_SETTINGS: u32 = 0x00001000;
/// `E1000_FWSM_MODE_MASK`: FW mode.
pub const E1000_FWSM_MODE_MASK: u32 = 0x0000000E;
/// `E1000_FWSM_MODE_SHIFT`.
pub const E1000_FWSM_MODE_SHIFT: u32 = 1;
/// `E1000_FWSM_ULP_CFG_DONE`: Low power cfg done.
pub const E1000_FWSM_ULP_CFG_DONE: u32 = 0x00000400;
/// `E1000_FWSM_FW_VALID`: FW established a valid mode.
pub const E1000_FWSM_FW_VALID: u32 = 0x00008000;
/// `E1000_FWSM_RSPCIPHY`: Reset PHY on PCI reset.
pub const E1000_FWSM_RSPCIPHY: u32 = 0x00000040;
/// `E1000_FWSM_DISSW`: FW disable SW Write Access.
pub const E1000_FWSM_DISSW: u32 = 0x10000000;
/// `E1000_FWSM_SKUSEL_MASK`: LAN SKU select.
pub const E1000_FWSM_SKUSEL_MASK: u32 = 0x60000000;
/// `E1000_FWSM_SKUEL_SHIFT`.
pub const E1000_FWSM_SKUEL_SHIFT: u32 = 29;
/// `E1000_FWSM_SKUSEL_EMB`: Embedded SKU.
pub const E1000_FWSM_SKUSEL_EMB: u32 = 0x0;
/// `E1000_FWSM_SKUSEL_CONS`: Consumer SKU.
pub const E1000_FWSM_SKUSEL_CONS: u32 = 0x1;
/// `E1000_FWSM_SKUSEL_PERF_100`: Perf & Corp 10/100 SKU.
pub const E1000_FWSM_SKUSEL_PERF_100: u32 = 0x2;
/// `E1000_FWSM_SKUSEL_PERF_GBE`: Perf & Copr GbE SKU.
pub const E1000_FWSM_SKUSEL_PERF_GBE: u32 = 0x3;
/// `E1000_FFLT_DBG_INVC`: Invalid /C/ code handling.
pub const E1000_FFLT_DBG_INVC: u32 = 0x00100000;
/// `E1000_HICR_EN`: Enable Bit - RO.
pub const E1000_HICR_EN: u32 = 0x00000001;
/// `E1000_HICR_C`: Driver sets this bit when done to put command in RAM.
pub const E1000_HICR_C: u32 = 0x00000002;
/// `E1000_HICR_SV`: Status Validity.
pub const E1000_HICR_SV: u32 = 0x00000004;
/// `E1000_HICR_FWR`: FW reset. Set by the Host.
pub const E1000_HICR_FWR: u32 = 0x00000080;
/// `E1000_HI_MAX_DATA_LENGTH`: Host Interface data length.
pub const E1000_HI_MAX_DATA_LENGTH: usize = 252;
/// `E1000_HI_MAX_BLOCK_BYTE_LENGTH`: Number of bytes in range.
pub const E1000_HI_MAX_BLOCK_BYTE_LENGTH: u32 = 1792;
/// `E1000_HI_MAX_BLOCK_DWORD_LENGTH`: Number of dwords in range.
pub const E1000_HI_MAX_BLOCK_DWORD_LENGTH: u32 = 448;
/// `E1000_HI_COMMAND_TIMEOUT`: Time in ms to process HI command.
pub const E1000_HI_COMMAND_TIMEOUT: u32 = 500;
/// `E1000_HSMC0R_CLKIN`: SMB Clock in.
pub const E1000_HSMC0R_CLKIN: u32 = 0x00000001;
/// `E1000_HSMC0R_DATAIN`: SMB Data in.
pub const E1000_HSMC0R_DATAIN: u32 = 0x00000002;
/// `E1000_HSMC0R_DATAOUT`: SMB Data out.
pub const E1000_HSMC0R_DATAOUT: u32 = 0x00000004;
/// `E1000_HSMC0R_CLKOUT`: SMB Clock out.
pub const E1000_HSMC0R_CLKOUT: u32 = 0x00000008;
/// `E1000_HSMC1R_CLKIN`.
pub const E1000_HSMC1R_CLKIN: u32 = E1000_HSMC0R_CLKIN;
/// `E1000_HSMC1R_DATAIN`.
pub const E1000_HSMC1R_DATAIN: u32 = E1000_HSMC0R_DATAIN;
/// `E1000_HSMC1R_DATAOUT`.
pub const E1000_HSMC1R_DATAOUT: u32 = E1000_HSMC0R_DATAOUT;
/// `E1000_HSMC1R_CLKOUT`.
pub const E1000_HSMC1R_CLKOUT: u32 = E1000_HSMC0R_CLKOUT;
/// `E1000_FWSTS_FWS_MASK`: FW Status.
pub const E1000_FWSTS_FWS_MASK: u32 = 0x000000FF;
/// `E1000_WUPL_LENGTH_MASK`: Only the lower 12 bits are valid.
pub const E1000_WUPL_LENGTH_MASK: u32 = 0x0FFF;
/// `E1000_MDALIGN`.
pub const E1000_MDALIGN: u32 = 4096;
/// `E1000_MDICNFG_EXT_MDIO`: MDI ext/int destination.
pub const E1000_MDICNFG_EXT_MDIO: u32 = 0x80000000;
/// `E1000_MDICNFG_COM_MDIO`: MDI shared w/ lan 0.
pub const E1000_MDICNFG_COM_MDIO: u32 = 0x40000000;
/// `E1000_MDICNFG_PHY_MASK`.
pub const E1000_MDICNFG_PHY_MASK: u32 = 0x03E00000;
/// `E1000_MDICNFG_PHY_SHIFT`.
pub const E1000_MDICNFG_PHY_SHIFT: u32 = 21;
/// `E1000_IPCNFG_EEE_1G_AN`: IPCNFG EEE Ena 1G AN.
pub const E1000_IPCNFG_EEE_1G_AN: u32 = 0x00000008;
/// `E1000_IPCNFG_EEE_100M_AN`: IPCNFG EEE Ena 100M AN.
pub const E1000_IPCNFG_EEE_100M_AN: u32 = 0x00000004;
/// `E1000_EEER_TX_LPI_EN`: EEER Tx LPI Enable.
pub const E1000_EEER_TX_LPI_EN: u32 = 0x00010000;
/// `E1000_EEER_RX_LPI_EN`: EEER Rx LPI Enable.
pub const E1000_EEER_RX_LPI_EN: u32 = 0x00020000;
/// `E1000_EEER_LPI_FC`: EEER Ena on Flow Cntrl.
pub const E1000_EEER_LPI_FC: u32 = 0x00040000;
/// `E1000_EEER_EEE_NEG`: EEE capability nego.
pub const E1000_EEER_EEE_NEG: u32 = 0x20000000;
/// `E1000_EEER_RX_LPI_STATUS`: Rx in LPI state.
pub const E1000_EEER_RX_LPI_STATUS: u32 = 0x40000000;
/// `E1000_EEER_TX_LPI_STATUS`: Tx in LPI state.
pub const E1000_EEER_TX_LPI_STATUS: u32 = 0x80000000;
/// `E1000_GCR_RXD_NO_SNOOP`.
pub const E1000_GCR_RXD_NO_SNOOP: u32 = 0x00000001;
/// `E1000_GCR_RXDSCW_NO_SNOOP`.
pub const E1000_GCR_RXDSCW_NO_SNOOP: u32 = 0x00000002;
/// `E1000_GCR_RXDSCR_NO_SNOOP`.
pub const E1000_GCR_RXDSCR_NO_SNOOP: u32 = 0x00000004;
/// `E1000_GCR_TXD_NO_SNOOP`.
pub const E1000_GCR_TXD_NO_SNOOP: u32 = 0x00000008;
/// `E1000_GCR_TXDSCW_NO_SNOOP`.
pub const E1000_GCR_TXDSCW_NO_SNOOP: u32 = 0x00000010;
/// `E1000_GCR_TXDSCR_NO_SNOOP`.
pub const E1000_GCR_TXDSCR_NO_SNOOP: u32 = 0x00000020;
/// `E1000_GCR_CMPL_TMOUT_MASK`.
pub const E1000_GCR_CMPL_TMOUT_MASK: u32 = 0x0000F000;
/// `E1000_GCR_CMPL_TMOUT_10ms`.
#[allow(non_upper_case_globals)] // OpenBSD name, verbatim
pub const E1000_GCR_CMPL_TMOUT_10ms: u32 = 0x00001000;
/// `E1000_GCR_CMPL_TMOUT_RESEND`.
pub const E1000_GCR_CMPL_TMOUT_RESEND: u32 = 0x00010000;
/// `E1000_GCR_CAP_VER2`.
pub const E1000_GCR_CAP_VER2: u32 = 0x00040000;
/// `PCI_EX_NO_SNOOP_ALL`.
pub const PCI_EX_NO_SNOOP_ALL: u32 = E1000_GCR_RXD_NO_SNOOP
    | E1000_GCR_RXDSCW_NO_SNOOP
    | E1000_GCR_RXDSCR_NO_SNOOP
    | E1000_GCR_TXD_NO_SNOOP
    | E1000_GCR_TXDSCW_NO_SNOOP
    | E1000_GCR_TXDSCR_NO_SNOOP;
/// `PCI_EX_82566_SNOOP_ALL`.
pub const PCI_EX_82566_SNOOP_ALL: u32 = PCI_EX_NO_SNOOP_ALL;
/// `E1000_GCR_L1_ACT_WITHOUT_L0S_RX`.
pub const E1000_GCR_L1_ACT_WITHOUT_L0S_RX: u32 = 0x08000000;
/// `E1000_FACTPS_FUNC0_POWER_STATE_MASK`.
pub const E1000_FACTPS_FUNC0_POWER_STATE_MASK: u32 = 0x00000003;
/// `E1000_FACTPS_LAN0_VALID`.
pub const E1000_FACTPS_LAN0_VALID: u32 = 0x00000004;
/// `E1000_FACTPS_FUNC0_AUX_EN`.
pub const E1000_FACTPS_FUNC0_AUX_EN: u32 = 0x00000008;
/// `E1000_FACTPS_FUNC1_POWER_STATE_MASK`.
pub const E1000_FACTPS_FUNC1_POWER_STATE_MASK: u32 = 0x000000C0;
/// `E1000_FACTPS_FUNC1_POWER_STATE_SHIFT`.
pub const E1000_FACTPS_FUNC1_POWER_STATE_SHIFT: u32 = 6;
/// `E1000_FACTPS_LAN1_VALID`.
pub const E1000_FACTPS_LAN1_VALID: u32 = 0x00000100;
/// `E1000_FACTPS_FUNC1_AUX_EN`.
pub const E1000_FACTPS_FUNC1_AUX_EN: u32 = 0x00000200;
/// `E1000_FACTPS_FUNC2_POWER_STATE_MASK`.
pub const E1000_FACTPS_FUNC2_POWER_STATE_MASK: u32 = 0x00003000;
/// `E1000_FACTPS_FUNC2_POWER_STATE_SHIFT`.
pub const E1000_FACTPS_FUNC2_POWER_STATE_SHIFT: u32 = 12;
/// `E1000_FACTPS_IDE_ENABLE`.
pub const E1000_FACTPS_IDE_ENABLE: u32 = 0x00004000;
/// `E1000_FACTPS_FUNC2_AUX_EN`.
pub const E1000_FACTPS_FUNC2_AUX_EN: u32 = 0x00008000;
/// `E1000_FACTPS_FUNC3_POWER_STATE_MASK`.
pub const E1000_FACTPS_FUNC3_POWER_STATE_MASK: u32 = 0x000C0000;
/// `E1000_FACTPS_FUNC3_POWER_STATE_SHIFT`.
pub const E1000_FACTPS_FUNC3_POWER_STATE_SHIFT: u32 = 18;
/// `E1000_FACTPS_SP_ENABLE`.
pub const E1000_FACTPS_SP_ENABLE: u32 = 0x00100000;
/// `E1000_FACTPS_FUNC3_AUX_EN`.
pub const E1000_FACTPS_FUNC3_AUX_EN: u32 = 0x00200000;
/// `E1000_FACTPS_FUNC4_POWER_STATE_MASK`.
pub const E1000_FACTPS_FUNC4_POWER_STATE_MASK: u32 = 0x03000000;
/// `E1000_FACTPS_FUNC4_POWER_STATE_SHIFT`.
pub const E1000_FACTPS_FUNC4_POWER_STATE_SHIFT: u32 = 24;
/// `E1000_FACTPS_IPMI_ENABLE`.
pub const E1000_FACTPS_IPMI_ENABLE: u32 = 0x04000000;
/// `E1000_FACTPS_FUNC4_AUX_EN`.
pub const E1000_FACTPS_FUNC4_AUX_EN: u32 = 0x08000000;
/// `E1000_FACTPS_MNGCG`.
pub const E1000_FACTPS_MNGCG: u32 = 0x20000000;
/// `E1000_FACTPS_LAN_FUNC_SEL`.
pub const E1000_FACTPS_LAN_FUNC_SEL: u32 = 0x40000000;
/// `E1000_FACTPS_PM_STATE_CHANGED`.
pub const E1000_FACTPS_PM_STATE_CHANGED: u32 = 0x80000000;
/// `E1000_IVAR_VALID`.
pub const E1000_IVAR_VALID: u32 = 0x80;
/// `E1000_GPIE_NSICR`.
pub const E1000_GPIE_NSICR: u32 = 0x00000001;
/// `E1000_GPIE_MSIX_MODE`.
pub const E1000_GPIE_MSIX_MODE: u32 = 0x00000010;
/// `E1000_GPIE_EIAME`.
pub const E1000_GPIE_EIAME: u32 = 0x40000000;
/// `E1000_GPIE_PBA`.
pub const E1000_GPIE_PBA: u32 = 0x80000000;
/// `E1000_MRQC_ENABLE_RSS_4Q`.
pub const E1000_MRQC_ENABLE_RSS_4Q: u32 = 0x00000002;
/// `E1000_MRQC_ENABLE_VMDQ`.
pub const E1000_MRQC_ENABLE_VMDQ: u32 = 0x00000003;
/// `E1000_MRQC_ENABLE_VMDQ_RSS_2Q`.
pub const E1000_MRQC_ENABLE_VMDQ_RSS_2Q: u32 = 0x00000005;
/// `E1000_MRQC_RSS_FIELD_IPV4_UDP`.
pub const E1000_MRQC_RSS_FIELD_IPV4_UDP: u32 = 0x00400000;
/// `E1000_MRQC_RSS_FIELD_IPV6_UDP`.
pub const E1000_MRQC_RSS_FIELD_IPV6_UDP: u32 = 0x00800000;
/// `E1000_MRQC_RSS_FIELD_IPV6_UDP_EX`.
pub const E1000_MRQC_RSS_FIELD_IPV6_UDP_EX: u32 = 0x01000000;
/// `E1000_MRQC_ENABLE_RSS_8Q`.
pub const E1000_MRQC_ENABLE_RSS_8Q: u32 = 0x00000002;
/// `E1000_SRRCTL_BSIZEPKT_SHIFT`: Shift _right_.
pub const E1000_SRRCTL_BSIZEPKT_SHIFT: u32 = 10;
/// `E1000_SRRCTL_BSIZEHDRSIZE_MASK`.
pub const E1000_SRRCTL_BSIZEHDRSIZE_MASK: u32 = 0x00000F00;
/// `E1000_SRRCTL_BSIZEHDRSIZE_SHIFT`: Shift _left_.
pub const E1000_SRRCTL_BSIZEHDRSIZE_SHIFT: u32 = 2;
/// `E1000_SRRCTL_DESCTYPE_LEGACY`.
pub const E1000_SRRCTL_DESCTYPE_LEGACY: u32 = 0x00000000;
/// `E1000_SRRCTL_DESCTYPE_ADV_ONEBUF`.
pub const E1000_SRRCTL_DESCTYPE_ADV_ONEBUF: u32 = 0x02000000;
/// `E1000_SRRCTL_DESCTYPE_HDR_SPLIT`.
pub const E1000_SRRCTL_DESCTYPE_HDR_SPLIT: u32 = 0x04000000;
/// `E1000_SRRCTL_DESCTYPE_HDR_SPLIT_ALWAYS`.
pub const E1000_SRRCTL_DESCTYPE_HDR_SPLIT_ALWAYS: u32 = 0x0A000000;
/// `E1000_SRRCTL_DESCTYPE_HDR_REPLICATION`.
pub const E1000_SRRCTL_DESCTYPE_HDR_REPLICATION: u32 = 0x06000000;
/// `E1000_SRRCTL_DESCTYPE_HDR_REPLICATION_LARGE_PKT`.
pub const E1000_SRRCTL_DESCTYPE_HDR_REPLICATION_LARGE_PKT: u32 = 0x08000000;
/// `E1000_SRRCTL_DESCTYPE_MASK`.
pub const E1000_SRRCTL_DESCTYPE_MASK: u32 = 0x0E000000;
/// `E1000_SRRCTL_TIMESTAMP`.
pub const E1000_SRRCTL_TIMESTAMP: u32 = 0x40000000;
/// `E1000_SRRCTL_DROP_EN`.
pub const E1000_SRRCTL_DROP_EN: u32 = 0x80000000;
/// `E1000_WUFC_FLX(_n)`: wake up on flexible filter `n`.
pub const fn e1000_wufc_flx(n: u32) -> u32 {
    1 << (16 + n)
}
/// `E1000_WUFC_FLEX_HQ`.
pub const E1000_WUFC_FLEX_HQ: u32 = 1 << 14;
/// `PCI_EX_LINK_STATUS`.
pub const PCI_EX_LINK_STATUS: u32 = 0x12;
/// `PCI_EX_LINK_WIDTH_MASK`.
pub const PCI_EX_LINK_WIDTH_MASK: u16 = 0x3F0;
/// `PCI_EX_LINK_WIDTH_SHIFT`.
pub const PCI_EX_LINK_WIDTH_SHIFT: u16 = 4;
/// `PCI_EX_DEVICE_CONTROL2`.
pub const PCI_EX_DEVICE_CONTROL2: u32 = 0x28;
/// `PCI_EX_DEVICE_CONTROL2_16ms`.
#[allow(non_upper_case_globals)] // OpenBSD name, verbatim
pub const PCI_EX_DEVICE_CONTROL2_16ms: u32 = 0x0005;
/// `EEPROM_READ_OPCODE_MICROWIRE`: EEPROM read opcode.
pub const EEPROM_READ_OPCODE_MICROWIRE: u16 = 0x6;
/// `EEPROM_WRITE_OPCODE_MICROWIRE`: EEPROM write opcode.
pub const EEPROM_WRITE_OPCODE_MICROWIRE: u16 = 0x5;
/// `EEPROM_ERASE_OPCODE_MICROWIRE`: EEPROM erase opcode.
pub const EEPROM_ERASE_OPCODE_MICROWIRE: u16 = 0x7;
/// `EEPROM_EWEN_OPCODE_MICROWIRE`: EEPROM erase/write enable.
pub const EEPROM_EWEN_OPCODE_MICROWIRE: u16 = 0x13;
/// `EEPROM_EWDS_OPCODE_MICROWIRE`: EEPROM erase/write disable.
pub const EEPROM_EWDS_OPCODE_MICROWIRE: u16 = 0x10;
/// `EEPROM_MAX_RETRY_SPI`: Max wait of 5ms, for RDY signal.
pub const EEPROM_MAX_RETRY_SPI: u16 = 5000;
/// `EEPROM_READ_OPCODE_SPI`: EEPROM read opcode.
pub const EEPROM_READ_OPCODE_SPI: u16 = 0x03;
/// `EEPROM_WRITE_OPCODE_SPI`: EEPROM write opcode.
pub const EEPROM_WRITE_OPCODE_SPI: u16 = 0x02;
/// `EEPROM_A8_OPCODE_SPI`: opcode bit-3 = address bit-8.
pub const EEPROM_A8_OPCODE_SPI: u16 = 0x08;
/// `EEPROM_WREN_OPCODE_SPI`: EEPROM set Write Enable latch.
pub const EEPROM_WREN_OPCODE_SPI: u16 = 0x06;
/// `EEPROM_WRDI_OPCODE_SPI`: EEPROM reset Write Enable latch.
pub const EEPROM_WRDI_OPCODE_SPI: u16 = 0x04;
/// `EEPROM_RDSR_OPCODE_SPI`: EEPROM read Status register.
pub const EEPROM_RDSR_OPCODE_SPI: u16 = 0x05;
/// `EEPROM_WRSR_OPCODE_SPI`: EEPROM write Status register.
pub const EEPROM_WRSR_OPCODE_SPI: u16 = 0x01;
/// `EEPROM_ERASE4K_OPCODE_SPI`: EEPROM ERASE 4KB.
pub const EEPROM_ERASE4K_OPCODE_SPI: u16 = 0x20;
/// `EEPROM_ERASE64K_OPCODE_SPI`: EEPROM ERASE 64KB.
pub const EEPROM_ERASE64K_OPCODE_SPI: u16 = 0xD8;
/// `EEPROM_ERASE256_OPCODE_SPI`: EEPROM ERASE 256B.
pub const EEPROM_ERASE256_OPCODE_SPI: u16 = 0xDB;
/// `EEPROM_WORD_SIZE_SHIFT`.
pub const EEPROM_WORD_SIZE_SHIFT: u32 = 6;
/// `EEPROM_WORD_SIZE_SHIFT_MAX`.
pub const EEPROM_WORD_SIZE_SHIFT_MAX: u32 = 14;
/// `EEPROM_SIZE_SHIFT`.
pub const EEPROM_SIZE_SHIFT: u16 = 10;
/// `EEPROM_SIZE_MASK`.
pub const EEPROM_SIZE_MASK: u16 = 0x1C00;
/// `EEPROM_MAC_ADDR_WORD0`.
pub const EEPROM_MAC_ADDR_WORD0: u16 = 0x0000;
/// `EEPROM_MAC_ADDR_WORD1`.
pub const EEPROM_MAC_ADDR_WORD1: u16 = 0x0001;
/// `EEPROM_MAC_ADDR_WORD2`.
pub const EEPROM_MAC_ADDR_WORD2: u16 = 0x0002;
/// `EEPROM_COMPAT`.
pub const EEPROM_COMPAT: u16 = 0x0003;
/// `EEPROM_ID_LED_SETTINGS`.
pub const EEPROM_ID_LED_SETTINGS: u16 = 0x0004;
/// `EEPROM_VERSION`.
pub const EEPROM_VERSION: u16 = 0x0005;
/// `EEPROM_SERDES_AMPLITUDE`: For SERDES output amplitude adjustment.
pub const EEPROM_SERDES_AMPLITUDE: u16 = 0x0006;
/// `EEPROM_PHY_CLASS_WORD`.
pub const EEPROM_PHY_CLASS_WORD: u16 = 0x0007;
/// `EEPROM_INIT_CONTROL1_REG`.
pub const EEPROM_INIT_CONTROL1_REG: u16 = 0x000A;
/// `EEPROM_INIT_CONTROL2_REG`.
pub const EEPROM_INIT_CONTROL2_REG: u16 = 0x000F;
/// `EEPROM_SWDEF_PINS_CTRL_PORT_1`.
pub const EEPROM_SWDEF_PINS_CTRL_PORT_1: u16 = 0x0010;
/// `EEPROM_INIT_CONTROL4_REG`.
pub const EEPROM_INIT_CONTROL4_REG: u16 = 0x0013;
/// `EEPROM_INIT_CONTROL3_PORT_B`.
pub const EEPROM_INIT_CONTROL3_PORT_B: u16 = 0x0014;
/// `EEPROM_INIT_3GIO_3`.
pub const EEPROM_INIT_3GIO_3: u16 = 0x001A;
/// `EEPROM_LED_1_CFG`.
pub const EEPROM_LED_1_CFG: u16 = 0x001C;
/// `EEPROM_LED_0_2_CFG`.
pub const EEPROM_LED_0_2_CFG: u16 = 0x001F;
/// `EEPROM_SWDEF_PINS_CTRL_PORT_0`.
pub const EEPROM_SWDEF_PINS_CTRL_PORT_0: u16 = 0x0020;
/// `EEPROM_INIT_CONTROL3_PORT_A`.
pub const EEPROM_INIT_CONTROL3_PORT_A: u16 = 0x0024;
/// `EEPROM_CFG`.
pub const EEPROM_CFG: u16 = 0x0012;
/// `EEPROM_FLASH_VERSION`.
pub const EEPROM_FLASH_VERSION: u16 = 0x0032;
/// `EEPROM_CHECKSUM_REG`.
pub const EEPROM_CHECKSUM_REG: u16 = 0x003F;
/// `EEPROM_COMPAT_VALID_CSUM`.
pub const EEPROM_COMPAT_VALID_CSUM: u16 = 0x0001;
/// `EEPROM_FUTURE_INIT_WORD1`.
pub const EEPROM_FUTURE_INIT_WORD1: u16 = 0x0019;
/// `EEPROM_FUTURE_INIT_WORD1_VALID_CSUM`.
pub const EEPROM_FUTURE_INIT_WORD1_VALID_CSUM: u16 = 0x0040;
/// `E1000_NVM_CFG_DONE_PORT_0`: MNG config cycle done.
pub const E1000_NVM_CFG_DONE_PORT_0: u32 = 0x040000;
/// `E1000_NVM_CFG_DONE_PORT_1`: ...for second port.
pub const E1000_NVM_CFG_DONE_PORT_1: u32 = 0x080000;
/// `E1000_NVM_CFG_DONE_PORT_2`: ...for third port.
pub const E1000_NVM_CFG_DONE_PORT_2: u32 = 0x100000;
/// `E1000_NVM_CFG_DONE_PORT_3`: ...for fourth port.
pub const E1000_NVM_CFG_DONE_PORT_3: u32 = 0x200000;
/// `NVM_82580_LAN_FUNC_OFFSET(a)`: the word offset of LAN function `a`'s NVM section.
pub const fn nvm_82580_lan_func_offset(a: u8) -> u16 {
    if a != 0 { 0x40 + (0x40 * a as u16) } else { 0 }
}
/// `NVM_WORD24_COM_MDIO`: MDIO interface shared.
pub const NVM_WORD24_COM_MDIO: u16 = 0x0008;
/// `NVM_WORD24_EXT_MDIO`: MDIO accesses routed external.
pub const NVM_WORD24_EXT_MDIO: u16 = 0x0004;
/// `ID_LED_RESERVED_0000`.
pub const ID_LED_RESERVED_0000: u16 = 0x0000;
/// `ID_LED_RESERVED_FFFF`.
pub const ID_LED_RESERVED_FFFF: u16 = 0xFFFF;
/// `ID_LED_RESERVED_82573`.
pub const ID_LED_RESERVED_82573: u16 = 0xF746;
/// `ID_LED_DEFAULT_82573`.
pub const ID_LED_DEFAULT_82573: u16 = 0x1811;
/// `ID_LED_DEFAULT`.
pub const ID_LED_DEFAULT: u16 = (ID_LED_OFF1_ON2 << 12)
    | (ID_LED_OFF1_OFF2 << 8)
    | (ID_LED_DEF1_DEF2 << 4)
    | (ID_LED_DEF1_DEF2);
/// `ID_LED_DEFAULT_ICH8LAN`.
pub const ID_LED_DEFAULT_ICH8LAN: u16 = (ID_LED_DEF1_DEF2 << 12)
    | (ID_LED_DEF1_OFF2 << 8)
    | (ID_LED_DEF1_ON2 << 4)
    | (ID_LED_DEF1_DEF2);
/// `ID_LED_DEF1_DEF2`.
pub const ID_LED_DEF1_DEF2: u16 = 0x1;
/// `ID_LED_DEF1_ON2`.
pub const ID_LED_DEF1_ON2: u16 = 0x2;
/// `ID_LED_DEF1_OFF2`.
pub const ID_LED_DEF1_OFF2: u16 = 0x3;
/// `ID_LED_ON1_DEF2`.
pub const ID_LED_ON1_DEF2: u16 = 0x4;
/// `ID_LED_ON1_ON2`.
pub const ID_LED_ON1_ON2: u16 = 0x5;
/// `ID_LED_ON1_OFF2`.
pub const ID_LED_ON1_OFF2: u16 = 0x6;
/// `ID_LED_OFF1_DEF2`.
pub const ID_LED_OFF1_DEF2: u16 = 0x7;
/// `ID_LED_OFF1_ON2`.
pub const ID_LED_OFF1_ON2: u16 = 0x8;
/// `ID_LED_OFF1_OFF2`.
pub const ID_LED_OFF1_OFF2: u16 = 0x9;
/// `IGP_ACTIVITY_LED_MASK`.
pub const IGP_ACTIVITY_LED_MASK: u32 = 0xFFFFF0FF;
/// `IGP_ACTIVITY_LED_ENABLE`.
pub const IGP_ACTIVITY_LED_ENABLE: u32 = 0x0300;
/// `IGP_LED3_MODE`.
pub const IGP_LED3_MODE: u32 = 0x07000000;
/// `EEPROM_SERDES_AMPLITUDE_MASK`.
pub const EEPROM_SERDES_AMPLITUDE_MASK: u16 = 0x000F;
/// `EEPROM_PHY_CLASS_A`.
pub const EEPROM_PHY_CLASS_A: u16 = 0x8000;
/// `EEPROM_WORD0A_ILOS`.
pub const EEPROM_WORD0A_ILOS: u16 = 0x0010;
/// `EEPROM_WORD0A_SWDPIO`.
pub const EEPROM_WORD0A_SWDPIO: u16 = 0x01E0;
/// `EEPROM_WORD0A_LRST`.
pub const EEPROM_WORD0A_LRST: u16 = 0x0200;
/// `EEPROM_WORD0A_FD`.
pub const EEPROM_WORD0A_FD: u16 = 0x0400;
/// `EEPROM_WORD0A_66MHZ`.
pub const EEPROM_WORD0A_66MHZ: u16 = 0x0800;
/// `EEPROM_WORD0F_PAUSE_MASK`.
pub const EEPROM_WORD0F_PAUSE_MASK: u16 = 0x3000;
/// `EEPROM_WORD0F_PAUSE`.
pub const EEPROM_WORD0F_PAUSE: u16 = 0x1000;
/// `EEPROM_WORD0F_ASM_DIR`.
pub const EEPROM_WORD0F_ASM_DIR: u16 = 0x2000;
/// `EEPROM_WORD0F_ANE`.
pub const EEPROM_WORD0F_ANE: u16 = 0x0800;
/// `EEPROM_WORD0F_SWPDIO_EXT`.
pub const EEPROM_WORD0F_SWPDIO_EXT: u16 = 0x00F0;
/// `EEPROM_WORD0F_LPLU`.
pub const EEPROM_WORD0F_LPLU: u16 = 0x0001;
/// `EEPROM_WORD1020_GIGA_DISABLE`.
pub const EEPROM_WORD1020_GIGA_DISABLE: u16 = 0x0010;
/// `EEPROM_WORD1020_GIGA_DISABLE_NON_D0A`.
pub const EEPROM_WORD1020_GIGA_DISABLE_NON_D0A: u16 = 0x0008;
/// `EEPROM_WORD1A_ASPM_MASK`.
pub const EEPROM_WORD1A_ASPM_MASK: u16 = 0x000C;
/// `EEPROM_SUM`.
pub const EEPROM_SUM: u16 = 0xBABA;
/// `EEPROM_NODE_ADDRESS_BYTE_0`.
pub const EEPROM_NODE_ADDRESS_BYTE_0: u16 = 0;
/// `EEPROM_PBA_BYTE_1`.
pub const EEPROM_PBA_BYTE_1: u16 = 8;
/// `EEPROM_RESERVED_WORD`.
pub const EEPROM_RESERVED_WORD: u16 = 0xFFFF;
/// `PBA_SIZE`.
pub const PBA_SIZE: u32 = 4;
/// `E1000_COLLISION_THRESHOLD`.
pub const E1000_COLLISION_THRESHOLD: u32 = 15;
/// `E1000_CT_SHIFT`.
pub const E1000_CT_SHIFT: u32 = 4;
/// `E1000_COLLISION_DISTANCE`.
pub const E1000_COLLISION_DISTANCE: u32 = 63;
/// `E1000_COLLISION_DISTANCE_82542`.
pub const E1000_COLLISION_DISTANCE_82542: u32 = 64;
/// `E1000_FDX_COLLISION_DISTANCE`.
pub const E1000_FDX_COLLISION_DISTANCE: u32 = E1000_COLLISION_DISTANCE;
/// `E1000_HDX_COLLISION_DISTANCE`.
pub const E1000_HDX_COLLISION_DISTANCE: u32 = E1000_COLLISION_DISTANCE;
/// `E1000_COLD_SHIFT`.
pub const E1000_COLD_SHIFT: u32 = 12;
/// `REQ_TX_DESCRIPTOR_MULTIPLE`.
pub const REQ_TX_DESCRIPTOR_MULTIPLE: u32 = 8;
/// `REQ_RX_DESCRIPTOR_MULTIPLE`.
pub const REQ_RX_DESCRIPTOR_MULTIPLE: u32 = 8;
/// `DEFAULT_82542_TIPG_IPGT`.
pub const DEFAULT_82542_TIPG_IPGT: u32 = 10;
/// `DEFAULT_82543_TIPG_IPGT_FIBER`.
pub const DEFAULT_82543_TIPG_IPGT_FIBER: u32 = 9;
/// `DEFAULT_82543_TIPG_IPGT_COPPER`.
pub const DEFAULT_82543_TIPG_IPGT_COPPER: u32 = 8;
/// `E1000_TIPG_IPGT_MASK`.
pub const E1000_TIPG_IPGT_MASK: u32 = 0x000003FF;
/// `E1000_TIPG_IPGR1_MASK`.
pub const E1000_TIPG_IPGR1_MASK: u32 = 0x000FFC00;
/// `E1000_TIPG_IPGR2_MASK`.
pub const E1000_TIPG_IPGR2_MASK: u32 = 0x3FF00000;
/// `DEFAULT_82542_TIPG_IPGR1`.
pub const DEFAULT_82542_TIPG_IPGR1: u32 = 2;
/// `DEFAULT_82543_TIPG_IPGR1`.
pub const DEFAULT_82543_TIPG_IPGR1: u32 = 8;
/// `E1000_TIPG_IPGR1_SHIFT`.
pub const E1000_TIPG_IPGR1_SHIFT: u32 = 10;
/// `DEFAULT_82542_TIPG_IPGR2`.
pub const DEFAULT_82542_TIPG_IPGR2: u32 = 10;
/// `DEFAULT_82543_TIPG_IPGR2`.
pub const DEFAULT_82543_TIPG_IPGR2: u32 = 6;
/// `DEFAULT_80003ES2LAN_TIPG_IPGR2`.
pub const DEFAULT_80003ES2LAN_TIPG_IPGR2: u32 = 7;
/// `E1000_TIPG_IPGR2_SHIFT`.
pub const E1000_TIPG_IPGR2_SHIFT: u32 = 20;
/// `DEFAULT_80003ES2LAN_TIPG_IPGT_10_100`.
pub const DEFAULT_80003ES2LAN_TIPG_IPGT_10_100: u32 = 0x00000009;
/// `DEFAULT_80003ES2LAN_TIPG_IPGT_1000`.
pub const DEFAULT_80003ES2LAN_TIPG_IPGT_1000: u32 = 0x00000008;
/// `E1000_TXDMAC_DPP`.
pub const E1000_TXDMAC_DPP: u32 = 0x00000001;
/// `TX_THRESHOLD_START`.
pub const TX_THRESHOLD_START: u32 = 8;
/// `TX_THRESHOLD_INCREMENT`.
pub const TX_THRESHOLD_INCREMENT: u32 = 10;
/// `TX_THRESHOLD_DECREMENT`.
pub const TX_THRESHOLD_DECREMENT: u32 = 1;
/// `TX_THRESHOLD_STOP`.
pub const TX_THRESHOLD_STOP: u32 = 190;
/// `TX_THRESHOLD_DISABLE`.
pub const TX_THRESHOLD_DISABLE: u32 = 0;
/// `TX_THRESHOLD_TIMER_MS`.
pub const TX_THRESHOLD_TIMER_MS: u32 = 10000;
/// `MIN_NUM_XMITS`.
pub const MIN_NUM_XMITS: u32 = 1000;
/// `IFS_MAX`.
pub const IFS_MAX: u32 = 80;
/// `IFS_STEP`.
pub const IFS_STEP: u32 = 10;
/// `IFS_MIN`.
pub const IFS_MIN: u32 = 40;
/// `IFS_RATIO`.
pub const IFS_RATIO: u32 = 4;
/// `E1000_EXTCNF_CTRL_PCIE_WRITE_ENABLE`.
pub const E1000_EXTCNF_CTRL_PCIE_WRITE_ENABLE: u32 = 0x00000001;
/// `E1000_EXTCNF_CTRL_PHY_WRITE_ENABLE`.
pub const E1000_EXTCNF_CTRL_PHY_WRITE_ENABLE: u32 = 0x00000002;
/// `E1000_EXTCNF_CTRL_D_UD_ENABLE`.
pub const E1000_EXTCNF_CTRL_D_UD_ENABLE: u32 = 0x00000004;
/// `E1000_EXTCNF_CTRL_D_UD_LATENCY`.
pub const E1000_EXTCNF_CTRL_D_UD_LATENCY: u32 = 0x00000008;
/// `E1000_EXTCNF_CTRL_D_UD_OWNER`.
pub const E1000_EXTCNF_CTRL_D_UD_OWNER: u32 = 0x00000010;
/// `E1000_EXTCNF_CTRL_MDIO_SW_OWNERSHIP`.
pub const E1000_EXTCNF_CTRL_MDIO_SW_OWNERSHIP: u32 = 0x00000020;
/// `E1000_EXTCNF_CTRL_MDIO_HW_OWNERSHIP`.
pub const E1000_EXTCNF_CTRL_MDIO_HW_OWNERSHIP: u32 = 0x00000040;
/// `E1000_EXTCNF_CTRL_EXT_CNF_POINTER`.
pub const E1000_EXTCNF_CTRL_EXT_CNF_POINTER: u32 = 0x0FFF0000;
/// `E1000_EXTCNF_SIZE_EXT_PHY_LENGTH`.
pub const E1000_EXTCNF_SIZE_EXT_PHY_LENGTH: u32 = 0x000000FF;
/// `E1000_EXTCNF_SIZE_EXT_DOCK_LENGTH`.
pub const E1000_EXTCNF_SIZE_EXT_DOCK_LENGTH: u32 = 0x0000FF00;
/// `E1000_EXTCNF_SIZE_EXT_PCIE_LENGTH`.
pub const E1000_EXTCNF_SIZE_EXT_PCIE_LENGTH: u32 = 0x00FF0000;
/// `E1000_EXTCNF_CTRL_LCD_WRITE_ENABLE`.
pub const E1000_EXTCNF_CTRL_LCD_WRITE_ENABLE: u32 = 0x00000001;
/// `E1000_EXTCNF_CTRL_SWFLAG`.
pub const E1000_EXTCNF_CTRL_SWFLAG: u32 = 0x00000020;
/// `E1000_EXTCNF_CTRL_GATE_PHY_CFG`.
pub const E1000_EXTCNF_CTRL_GATE_PHY_CFG: u32 = 0x00000080;
/// `E1000_PBA_8K`: 8KB, default Rx allocation.
pub const E1000_PBA_8K: u32 = 0x0008;
/// `E1000_PBA_10K`.
pub const E1000_PBA_10K: u32 = 0x000A;
/// `E1000_PBA_12K`: 12KB, default Rx allocation.
pub const E1000_PBA_12K: u32 = 0x000C;
/// `E1000_PBA_14K`: 14KB.
pub const E1000_PBA_14K: u32 = 0x000E;
/// `E1000_PBA_16K`: 16KB, default TX allocation.
pub const E1000_PBA_16K: u32 = 0x0010;
/// `E1000_PBA_20K`.
pub const E1000_PBA_20K: u32 = 0x0014;
/// `E1000_PBA_22K`.
pub const E1000_PBA_22K: u32 = 0x0016;
/// `E1000_PBA_24K`.
pub const E1000_PBA_24K: u32 = 0x0018;
/// `E1000_PBA_26K`.
pub const E1000_PBA_26K: u32 = 0x001A;
/// `E1000_PBA_30K`.
pub const E1000_PBA_30K: u32 = 0x001E;
/// `E1000_PBA_32K`.
pub const E1000_PBA_32K: u32 = 0x0020;
/// `E1000_PBA_34K`.
pub const E1000_PBA_34K: u32 = 0x0022;
/// `E1000_PBA_38K`.
pub const E1000_PBA_38K: u32 = 0x0026;
/// `E1000_PBA_40K`.
pub const E1000_PBA_40K: u32 = 0x0028;
/// `E1000_PBA_48K`: 48KB, default RX allocation.
pub const E1000_PBA_48K: u32 = 0x0030;
/// `E1000_PBS_16K`.
pub const E1000_PBS_16K: u32 = E1000_PBA_16K;
/// `FLOW_CONTROL_ADDRESS_LOW`.
pub const FLOW_CONTROL_ADDRESS_LOW: u32 = 0x00C28001;
/// `FLOW_CONTROL_ADDRESS_HIGH`.
pub const FLOW_CONTROL_ADDRESS_HIGH: u32 = 0x00000100;
/// `FLOW_CONTROL_TYPE`.
pub const FLOW_CONTROL_TYPE: u32 = 0x8808;
/// `FC_DEFAULT_HI_THRESH`: 32KB.
pub const FC_DEFAULT_HI_THRESH: u32 = 0x8000;
/// `FC_DEFAULT_LO_THRESH`: 16KB.
pub const FC_DEFAULT_LO_THRESH: u32 = 0x4000;
/// `FC_DEFAULT_TX_TIMER`: ~130 us.
pub const FC_DEFAULT_TX_TIMER: u32 = 0x100;
/// `PCIX_COMMAND_REGISTER`.
pub const PCIX_COMMAND_REGISTER: u32 = 0xE6;
/// `PCIX_STATUS_REGISTER_LO`.
pub const PCIX_STATUS_REGISTER_LO: u32 = 0xE8;
/// `PCIX_STATUS_REGISTER_HI`.
pub const PCIX_STATUS_REGISTER_HI: u32 = 0xEA;
/// `PCIX_COMMAND_MMRBC_MASK`.
pub const PCIX_COMMAND_MMRBC_MASK: u16 = 0x000C;
/// `PCIX_COMMAND_MMRBC_SHIFT`.
pub const PCIX_COMMAND_MMRBC_SHIFT: u16 = 0x2;
/// `PCIX_STATUS_HI_MMRBC_MASK`.
pub const PCIX_STATUS_HI_MMRBC_MASK: u16 = 0x0060;
/// `PCIX_STATUS_HI_MMRBC_SHIFT`.
pub const PCIX_STATUS_HI_MMRBC_SHIFT: u16 = 0x5;
/// `PCIX_STATUS_HI_MMRBC_4K`.
pub const PCIX_STATUS_HI_MMRBC_4K: u16 = 0x3;
/// `PCIX_STATUS_HI_MMRBC_2K`.
pub const PCIX_STATUS_HI_MMRBC_2K: u16 = 0x2;
/// `PAUSE_SHIFT`.
pub const PAUSE_SHIFT: u32 = 5;
/// `SWDPIO_SHIFT`.
pub const SWDPIO_SHIFT: u32 = 17;
/// `SWDPIO__EXT_SHIFT`.
pub const SWDPIO__EXT_SHIFT: u32 = 4;
/// `ILOS_SHIFT`.
pub const ILOS_SHIFT: u32 = 3;
/// `RECEIVE_BUFFER_ALIGN_SIZE`.
pub const RECEIVE_BUFFER_ALIGN_SIZE: u32 = 256;
/// `LINK_UP_TIMEOUT`.
pub const LINK_UP_TIMEOUT: u32 = 500;
/// `MASTER_DISABLE_TIMEOUT`.
pub const MASTER_DISABLE_TIMEOUT: u32 = 800;
/// `AUTO_READ_DONE_TIMEOUT`.
pub const AUTO_READ_DONE_TIMEOUT: u32 = 10;
/// `PHY_CFG_TIMEOUT`.
pub const PHY_CFG_TIMEOUT: u32 = 100;
/// `SW_FLAG_TIMEOUT`.
pub const SW_FLAG_TIMEOUT: u32 = 1000;
/// `E1000_TX_BUFFER_SIZE`.
pub const E1000_TX_BUFFER_SIZE: u32 = 1514;
/// `CARRIER_EXTENSION`.
pub const CARRIER_EXTENSION: u32 = 0x0F;
/// `TBI_ACCEPT(sc, status, errors, length, last_byte)`: whether a frame the MAC flagged as a
/// carrier extension error is acceptable in TBI compatibility mode.
pub fn tbi_accept(hw: &EmHw, status: u8, errors: u8, length: u32, last_byte: u8) -> bool {
    tbi_accept_with(
        hw.tbi_compatibility_on,
        hw.min_frame_size,
        hw.max_frame_size,
        status,
        errors,
        length,
        last_byte,
    )
}
/// `TBI_ACCEPT` over the three members of `struct em_hw` it reads, passed by value: if_em(4)
/// evaluates it in its receive interrupt from copies it keeps (`sc->hw` belongs to the
/// kernel lock there).
pub fn tbi_accept_with(
    tbi_compatibility_on: bool,
    min_frame_size: u32,
    max_frame_size: u32,
    status: u8,
    errors: u8,
    length: u32,
    last_byte: u8,
) -> bool {
    tbi_compatibility_on
        && ((u32::from(errors) & E1000_RXD_ERR_FRAME_ERR_MASK) == E1000_RXD_ERR_CE)
        && (u32::from(last_byte) == CARRIER_EXTENSION)
        && (if u32::from(status) & E1000_RXD_STAT_VP != 0 {
            length > min_frame_size.wrapping_sub(VLAN_TAG_SIZE)
                && length <= max_frame_size.wrapping_add(1)
        } else {
            length > min_frame_size && length <= max_frame_size.wrapping_add(VLAN_TAG_SIZE + 1)
        })
}
/// `E1000_CTRL_PHY_RESET_DIR`.
pub const E1000_CTRL_PHY_RESET_DIR: u32 = E1000_CTRL_SWDPIO0;
/// `E1000_CTRL_PHY_RESET`.
pub const E1000_CTRL_PHY_RESET: u32 = E1000_CTRL_SWDPIN0;
/// `E1000_CTRL_MDIO_DIR`.
pub const E1000_CTRL_MDIO_DIR: u32 = E1000_CTRL_SWDPIO2;
/// `E1000_CTRL_MDIO`.
pub const E1000_CTRL_MDIO: u32 = E1000_CTRL_SWDPIN2;
/// `E1000_CTRL_MDC_DIR`.
pub const E1000_CTRL_MDC_DIR: u32 = E1000_CTRL_SWDPIO3;
/// `E1000_CTRL_MDC`.
pub const E1000_CTRL_MDC: u32 = E1000_CTRL_SWDPIN3;
/// `E1000_CTRL_PHY_RESET_DIR4`.
pub const E1000_CTRL_PHY_RESET_DIR4: u32 = E1000_CTRL_EXT_SDP4_DIR;
/// `E1000_CTRL_PHY_RESET4`.
pub const E1000_CTRL_PHY_RESET4: u32 = E1000_CTRL_EXT_SDP4_DATA;
/// `PHY_CTRL`: Control Register.
pub const PHY_CTRL: u32 = 0x00;
/// `PHY_STATUS`: Status Register.
pub const PHY_STATUS: u32 = 0x01;
/// `PHY_ID1`: Phy Id Reg (word 1).
pub const PHY_ID1: u32 = 0x02;
/// `PHY_ID2`: Phy Id Reg (word 2).
pub const PHY_ID2: u32 = 0x03;
/// `PHY_AUTONEG_ADV`: Autoneg Advertisement.
pub const PHY_AUTONEG_ADV: u32 = 0x04;
/// `PHY_LP_ABILITY`: Link Partner Ability (Base Page).
pub const PHY_LP_ABILITY: u32 = 0x05;
/// `PHY_AUTONEG_EXP`: Autoneg Expansion Reg.
pub const PHY_AUTONEG_EXP: u32 = 0x06;
/// `PHY_NEXT_PAGE_TX`: Next Page TX.
pub const PHY_NEXT_PAGE_TX: u32 = 0x07;
/// `PHY_LP_NEXT_PAGE`: Link Partner Next Page.
pub const PHY_LP_NEXT_PAGE: u32 = 0x08;
/// `PHY_1000T_CTRL`: 1000Base-T Control Reg.
pub const PHY_1000T_CTRL: u32 = 0x09;
/// `PHY_1000T_STATUS`: 1000Base-T Status Reg.
pub const PHY_1000T_STATUS: u32 = 0x0A;
/// `PHY_EXT_STATUS`: Extended Status Reg.
pub const PHY_EXT_STATUS: u32 = 0x0F;
/// `MAX_PHY_REG_ADDRESS`: 5 bit address bus (0-0x1F).
pub const MAX_PHY_REG_ADDRESS: u32 = 0x1F;
/// `MAX_PHY_MULTI_PAGE_REG`: Registers equal on all pages.
pub const MAX_PHY_MULTI_PAGE_REG: u32 = 0xF;
/// `M88E1000_PHY_SPEC_CTRL`: PHY Specific Control Register.
pub const M88E1000_PHY_SPEC_CTRL: u32 = 0x10;
/// `M88E1000_PHY_SPEC_STATUS`: PHY Specific Status Register.
pub const M88E1000_PHY_SPEC_STATUS: u32 = 0x11;
/// `M88E1000_INT_ENABLE`: Interrupt Enable Register.
pub const M88E1000_INT_ENABLE: u32 = 0x12;
/// `M88E1000_INT_STATUS`: Interrupt Status Register.
pub const M88E1000_INT_STATUS: u32 = 0x13;
/// `M88E1000_EXT_PHY_SPEC_CTRL`: Extended PHY Specific Control.
pub const M88E1000_EXT_PHY_SPEC_CTRL: u32 = 0x14;
/// `M88E1000_RX_ERR_CNTR`: Receive Error Counter.
pub const M88E1000_RX_ERR_CNTR: u32 = 0x15;
/// `M88E1000_PHY_EXT_CTRL`: PHY extend control register.
pub const M88E1000_PHY_EXT_CTRL: u32 = 0x1A;
/// `M88E1000_PHY_PAGE_SELECT`: Reg 29 for page number setting.
pub const M88E1000_PHY_PAGE_SELECT: u32 = 0x1D;
/// `M88E1000_PHY_GEN_CONTROL`: Its meaning depends on reg 29.
pub const M88E1000_PHY_GEN_CONTROL: u32 = 0x1E;
/// `M88E1000_PHY_VCO_REG_BIT8`: Bits 8 & 11 are adjusted for.
pub const M88E1000_PHY_VCO_REG_BIT8: u16 = 0x100;
/// `M88E1000_PHY_VCO_REG_BIT11`: improved BER performance.
pub const M88E1000_PHY_VCO_REG_BIT11: u16 = 0x800;
/// `M88E1543_PAGE_ADDR`: Page Offset Register.
pub const M88E1543_PAGE_ADDR: u32 = 0x16;
/// `M88E1543_EEE_CTRL_1`.
pub const M88E1543_EEE_CTRL_1: u32 = 0x0;
/// `M88E1543_EEE_CTRL_1_MS`: EEE Master/Slave.
pub const M88E1543_EEE_CTRL_1_MS: u32 = 0x0001;
/// `M88E1512_CFG_REG_1`.
pub const M88E1512_CFG_REG_1: u32 = 0x0010;
/// `M88E1512_CFG_REG_2`.
pub const M88E1512_CFG_REG_2: u32 = 0x0011;
/// `M88E1512_CFG_REG_3`.
pub const M88E1512_CFG_REG_3: u32 = 0x0007;
/// `M88E1512_MODE`.
pub const M88E1512_MODE: u32 = 0x0014;
/// `BME1000_PSCR_ENABLE_DOWNSHIFT`: 1 = enable downshift.
pub const BME1000_PSCR_ENABLE_DOWNSHIFT: u16 = 0x0800;
/// `BM_PHY_PAGE_SELECT`: Page Select for BM.
pub const BM_PHY_PAGE_SELECT: u32 = 22;
/// `BM_REG_BIAS1`.
pub const BM_REG_BIAS1: u32 = 29;
/// `BM_REG_BIAS2`.
pub const BM_REG_BIAS2: u32 = 30;
/// `BM_PORT_CTRL_PAGE`.
pub const BM_PORT_CTRL_PAGE: u32 = 769;
/// `IGP01E1000_IEEE_REGS_PAGE`.
pub const IGP01E1000_IEEE_REGS_PAGE: u32 = 0x0000;
/// `IGP01E1000_IEEE_RESTART_AUTONEG`.
pub const IGP01E1000_IEEE_RESTART_AUTONEG: u16 = 0x3300;
/// `IGP01E1000_IEEE_FORCE_GIGA`.
pub const IGP01E1000_IEEE_FORCE_GIGA: u16 = 0x0140;
/// `IGP01E1000_PHY_PORT_CONFIG`: PHY Specific Port Config Register.
pub const IGP01E1000_PHY_PORT_CONFIG: u32 = 0x10;
/// `IGP01E1000_PHY_PORT_STATUS`: PHY Specific Status Register.
pub const IGP01E1000_PHY_PORT_STATUS: u32 = 0x11;
/// `IGP01E1000_PHY_PORT_CTRL`: PHY Specific Control Register.
pub const IGP01E1000_PHY_PORT_CTRL: u32 = 0x12;
/// `IGP01E1000_PHY_LINK_HEALTH`: PHY Link Health Register.
pub const IGP01E1000_PHY_LINK_HEALTH: u32 = 0x13;
/// `IGP01E1000_GMII_FIFO`: GMII FIFO Register.
pub const IGP01E1000_GMII_FIFO: u32 = 0x14;
/// `IGP01E1000_PHY_CHANNEL_QUALITY`: PHY Channel Quality Register.
pub const IGP01E1000_PHY_CHANNEL_QUALITY: u32 = 0x15;
/// `IGP02E1000_PHY_POWER_MGMT`.
pub const IGP02E1000_PHY_POWER_MGMT: u32 = 0x19;
/// `IGP01E1000_PHY_PAGE_SELECT`: PHY Page Select Core Register.
pub const IGP01E1000_PHY_PAGE_SELECT: u32 = 0x1F;
/// `IGP01E1000_PHY_AGC_A`.
pub const IGP01E1000_PHY_AGC_A: u32 = 0x1172;
/// `IGP01E1000_PHY_AGC_B`.
pub const IGP01E1000_PHY_AGC_B: u32 = 0x1272;
/// `IGP01E1000_PHY_AGC_C`.
pub const IGP01E1000_PHY_AGC_C: u32 = 0x1472;
/// `IGP01E1000_PHY_AGC_D`.
pub const IGP01E1000_PHY_AGC_D: u32 = 0x1872;
/// `IGP02E1000_PHY_AGC_A`.
pub const IGP02E1000_PHY_AGC_A: u32 = 0x11B1;
/// `IGP02E1000_PHY_AGC_B`.
pub const IGP02E1000_PHY_AGC_B: u32 = 0x12B1;
/// `IGP02E1000_PHY_AGC_C`.
pub const IGP02E1000_PHY_AGC_C: u32 = 0x14B1;
/// `IGP02E1000_PHY_AGC_D`.
pub const IGP02E1000_PHY_AGC_D: u32 = 0x18B1;
/// `IGP01E1000_PHY_DSP_RESET`.
pub const IGP01E1000_PHY_DSP_RESET: u32 = 0x1F33;
/// `IGP01E1000_PHY_DSP_SET`.
pub const IGP01E1000_PHY_DSP_SET: u32 = 0x1F71;
/// `IGP01E1000_PHY_DSP_FFE`.
pub const IGP01E1000_PHY_DSP_FFE: u32 = 0x1F35;
/// `IGP01E1000_PHY_CHANNEL_NUM`.
pub const IGP01E1000_PHY_CHANNEL_NUM: u32 = 4;
/// `IGP02E1000_PHY_CHANNEL_NUM`.
pub const IGP02E1000_PHY_CHANNEL_NUM: u32 = 4;
/// `IGP01E1000_PHY_AGC_PARAM_A`.
pub const IGP01E1000_PHY_AGC_PARAM_A: u32 = 0x1171;
/// `IGP01E1000_PHY_AGC_PARAM_B`.
pub const IGP01E1000_PHY_AGC_PARAM_B: u32 = 0x1271;
/// `IGP01E1000_PHY_AGC_PARAM_C`.
pub const IGP01E1000_PHY_AGC_PARAM_C: u32 = 0x1471;
/// `IGP01E1000_PHY_AGC_PARAM_D`.
pub const IGP01E1000_PHY_AGC_PARAM_D: u32 = 0x1871;
/// `IGP01E1000_PHY_EDAC_MU_INDEX`.
pub const IGP01E1000_PHY_EDAC_MU_INDEX: u16 = 0xC000;
/// `IGP01E1000_PHY_EDAC_SIGN_EXT_9_BITS`.
pub const IGP01E1000_PHY_EDAC_SIGN_EXT_9_BITS: u16 = 0x8000;
/// `IGP01E1000_PHY_ANALOG_TX_STATE`.
pub const IGP01E1000_PHY_ANALOG_TX_STATE: u32 = 0x2890;
/// `IGP01E1000_PHY_ANALOG_CLASS_A`.
pub const IGP01E1000_PHY_ANALOG_CLASS_A: u32 = 0x2000;
/// `IGP01E1000_PHY_FORCE_ANALOG_ENABLE`.
pub const IGP01E1000_PHY_FORCE_ANALOG_ENABLE: u32 = 0x0004;
/// `IGP01E1000_PHY_DSP_FFE_CM_CP`.
pub const IGP01E1000_PHY_DSP_FFE_CM_CP: u16 = 0x0069;
/// `IGP01E1000_PHY_DSP_FFE_DEFAULT`.
pub const IGP01E1000_PHY_DSP_FFE_DEFAULT: u16 = 0x002A;
/// `IGP01E1000_PHY_PCS_INIT_REG`.
pub const IGP01E1000_PHY_PCS_INIT_REG: u32 = 0x00B4;
/// `IGP01E1000_PHY_PCS_CTRL_REG`.
pub const IGP01E1000_PHY_PCS_CTRL_REG: u32 = 0x00B5;
/// `IGP01E1000_ANALOG_REGS_PAGE`.
pub const IGP01E1000_ANALOG_REGS_PAGE: u32 = 0x20C0;
/// `I82580_ADDR_REG`.
pub const I82580_ADDR_REG: u32 = 16;
/// `I82580_CFG_REG`.
pub const I82580_CFG_REG: u32 = 22;
/// `I82580_CFG_ASSERT_CRS_ON_TX`.
pub const I82580_CFG_ASSERT_CRS_ON_TX: u16 = 1 << 15;
/// `I82580_CFG_ENABLE_DOWNSHIFT`: auto downshift 100/10.
pub const I82580_CFG_ENABLE_DOWNSHIFT: u16 = 3 << 10;
/// `I82580_CTRL_REG`.
pub const I82580_CTRL_REG: u32 = 23;
/// `I82580_CTRL_DOWNSHIFT_MASK`.
pub const I82580_CTRL_DOWNSHIFT_MASK: u32 = 7 << 10;
/// `GG82563_PAGE_SHIFT`.
pub const GG82563_PAGE_SHIFT: u32 = 5;
/// `GG82563_REG(page, reg)`: a GG82563 register address, page and register.
pub const fn gg82563_reg(page: u32, reg: u32) -> u32 {
    (page << GG82563_PAGE_SHIFT) | (reg & MAX_PHY_REG_ADDRESS)
}
/// `GG82563_MIN_ALT_REG`.
pub const GG82563_MIN_ALT_REG: u32 = 30;
/// `GG82563_PHY_SPEC_CTRL`: PHY Specific Control.
pub const GG82563_PHY_SPEC_CTRL: u32 = gg82563_reg(0, 16);
/// `GG82563_PHY_SPEC_STATUS`: PHY Specific Status.
pub const GG82563_PHY_SPEC_STATUS: u32 = gg82563_reg(0, 17);
/// `GG82563_PHY_INT_ENABLE`: Interrupt Enable.
pub const GG82563_PHY_INT_ENABLE: u32 = gg82563_reg(0, 18);
/// `GG82563_PHY_SPEC_STATUS_2`: PHY Specific Status 2.
pub const GG82563_PHY_SPEC_STATUS_2: u32 = gg82563_reg(0, 19);
/// `GG82563_PHY_RX_ERR_CNTR`: Receive Error Counter.
pub const GG82563_PHY_RX_ERR_CNTR: u32 = gg82563_reg(0, 21);
/// `GG82563_PHY_PAGE_SELECT`: Page Select.
pub const GG82563_PHY_PAGE_SELECT: u32 = gg82563_reg(0, 22);
/// `GG82563_PHY_SPEC_CTRL_2`: PHY Specific Control 2.
pub const GG82563_PHY_SPEC_CTRL_2: u32 = gg82563_reg(0, 26);
/// `GG82563_PHY_PAGE_SELECT_ALT`: Alternate Page Select.
pub const GG82563_PHY_PAGE_SELECT_ALT: u32 = gg82563_reg(0, 29);
/// `GG82563_PHY_TEST_CLK_CTRL`: Test Clock Control (use reg. 29 to select).
pub const GG82563_PHY_TEST_CLK_CTRL: u32 = gg82563_reg(0, 30);
/// `GG82563_PHY_MAC_SPEC_CTRL`: MAC Specific Control Register.
pub const GG82563_PHY_MAC_SPEC_CTRL: u32 = gg82563_reg(2, 21);
/// `GG82563_PHY_MAC_SPEC_CTRL_2`: MAC Specific Control 2.
pub const GG82563_PHY_MAC_SPEC_CTRL_2: u32 = gg82563_reg(2, 26);
/// `GG82563_PHY_DSP_DISTANCE`: DSP Distance.
pub const GG82563_PHY_DSP_DISTANCE: u32 = gg82563_reg(5, 26);
/// `GG82563_PHY_KMRN_MODE_CTRL`: Kumeran Mode Control.
pub const GG82563_PHY_KMRN_MODE_CTRL: u32 = gg82563_reg(193, 16);
/// `GG82563_PHY_PORT_RESET`: Port Reset.
pub const GG82563_PHY_PORT_RESET: u32 = gg82563_reg(193, 17);
/// `GG82563_PHY_REVISION_ID`: Revision ID.
pub const GG82563_PHY_REVISION_ID: u32 = gg82563_reg(193, 18);
/// `GG82563_PHY_DEVICE_ID`: Device ID.
pub const GG82563_PHY_DEVICE_ID: u32 = gg82563_reg(193, 19);
/// `GG82563_PHY_PWR_MGMT_CTRL`: Power Management Control.
pub const GG82563_PHY_PWR_MGMT_CTRL: u32 = gg82563_reg(193, 20);
/// `GG82563_PHY_RATE_ADAPT_CTRL`: Rate Adaptation Control.
pub const GG82563_PHY_RATE_ADAPT_CTRL: u32 = gg82563_reg(193, 25);
/// `GG82563_PHY_KMRN_FIFO_CTRL_STAT`: FIFO's Control/Status.
pub const GG82563_PHY_KMRN_FIFO_CTRL_STAT: u32 = gg82563_reg(194, 16);
/// `GG82563_PHY_KMRN_CTRL`: Control.
pub const GG82563_PHY_KMRN_CTRL: u32 = gg82563_reg(194, 17);
/// `GG82563_PHY_INBAND_CTRL`: Inband Control.
pub const GG82563_PHY_INBAND_CTRL: u32 = gg82563_reg(194, 18);
/// `GG82563_PHY_KMRN_DIAGNOSTIC`: Diagnostic.
pub const GG82563_PHY_KMRN_DIAGNOSTIC: u32 = gg82563_reg(194, 19);
/// `GG82563_PHY_ACK_TIMEOUTS`: Acknowledge Timeouts.
pub const GG82563_PHY_ACK_TIMEOUTS: u32 = gg82563_reg(194, 20);
/// `GG82563_PHY_ADV_ABILITY`: Advertised Ability.
pub const GG82563_PHY_ADV_ABILITY: u32 = gg82563_reg(194, 21);
/// `GG82563_PHY_LINK_PARTNER_ADV_ABILITY`: Link Partner Advertised Ability.
pub const GG82563_PHY_LINK_PARTNER_ADV_ABILITY: u32 = gg82563_reg(194, 23);
/// `GG82563_PHY_ADV_NEXT_PAGE`: Advertised Next Page.
pub const GG82563_PHY_ADV_NEXT_PAGE: u32 = gg82563_reg(194, 24);
/// `GG82563_PHY_LINK_PARTNER_ADV_NEXT_PAGE`: Link Partner Advertised Next page.
pub const GG82563_PHY_LINK_PARTNER_ADV_NEXT_PAGE: u32 = gg82563_reg(194, 25);
/// `GG82563_PHY_KMRN_MISC`: Misc.
pub const GG82563_PHY_KMRN_MISC: u32 = gg82563_reg(194, 26);
/// `I82577_PHY_ADDR_REG`.
pub const I82577_PHY_ADDR_REG: u32 = 16;
/// `I82577_PHY_CFG_REG`.
pub const I82577_PHY_CFG_REG: u32 = 22;
/// `I82577_PHY_CTRL_REG`.
pub const I82577_PHY_CTRL_REG: u32 = 23;
/// `I82577_PHY_CFG_ENABLE_CRS_ON_TX`.
pub const I82577_PHY_CFG_ENABLE_CRS_ON_TX: u16 = 1 << 15;
/// `I82577_PHY_CFG_ENABLE_DOWNSHIFT`.
pub const I82577_PHY_CFG_ENABLE_DOWNSHIFT: u16 = (1 << 10) + (1 << 11);
/// `I82578_PHY_ADDR_REG`.
pub const I82578_PHY_ADDR_REG: u32 = 29;
/// `I82578_EPSCR_DOWNSHIFT_ENABLE`.
pub const I82578_EPSCR_DOWNSHIFT_ENABLE: u16 = 0x0020;
/// `I82578_EPSCR_DOWNSHIFT_COUNTER_MASK`.
pub const I82578_EPSCR_DOWNSHIFT_COUNTER_MASK: u16 = 0x001C;
/// `MII_CR_SPEED_SELECT_MSB`: bits 6,13: 10=1000, 01=100, 00=10.
pub const MII_CR_SPEED_SELECT_MSB: u32 = 0x0040;
/// `MII_CR_COLL_TEST_ENABLE`: Collision test enable.
pub const MII_CR_COLL_TEST_ENABLE: u32 = 0x0080;
/// `MII_CR_FULL_DUPLEX`: FDX =1, half duplex =0.
pub const MII_CR_FULL_DUPLEX: u16 = 0x0100;
/// `MII_CR_RESTART_AUTO_NEG`: Restart auto negotiation.
pub const MII_CR_RESTART_AUTO_NEG: u16 = 0x0200;
/// `MII_CR_ISOLATE`: Isolate PHY from MII.
pub const MII_CR_ISOLATE: u32 = 0x0400;
/// `MII_CR_POWER_DOWN`: Power down.
pub const MII_CR_POWER_DOWN: u32 = 0x0800;
/// `MII_CR_AUTO_NEG_EN`: Auto Neg Enable.
pub const MII_CR_AUTO_NEG_EN: u16 = 0x1000;
/// `MII_CR_SPEED_SELECT_LSB`: bits 6,13: 10=1000, 01=100, 00=10.
pub const MII_CR_SPEED_SELECT_LSB: u32 = 0x2000;
/// `MII_CR_LOOPBACK`: 0 = normal, 1 = loopback.
pub const MII_CR_LOOPBACK: u32 = 0x4000;
/// `MII_CR_RESET`: 0 = normal, 1 = PHY reset.
pub const MII_CR_RESET: u16 = 0x8000;
/// `MII_SR_EXTENDED_CAPS`: Extended register capabilities.
pub const MII_SR_EXTENDED_CAPS: u32 = 0x0001;
/// `MII_SR_JABBER_DETECT`: Jabber Detected.
pub const MII_SR_JABBER_DETECT: u32 = 0x0002;
/// `MII_SR_LINK_STATUS`: Link Status 1 = link.
pub const MII_SR_LINK_STATUS: u16 = 0x0004;
/// `MII_SR_AUTONEG_CAPS`: Auto Neg Capable.
pub const MII_SR_AUTONEG_CAPS: u32 = 0x0008;
/// `MII_SR_REMOTE_FAULT`: Remote Fault Detect.
pub const MII_SR_REMOTE_FAULT: u32 = 0x0010;
/// `MII_SR_AUTONEG_COMPLETE`: Auto Neg Complete.
pub const MII_SR_AUTONEG_COMPLETE: u16 = 0x0020;
/// `MII_SR_PREAMBLE_SUPPRESS`: Preamble may be suppressed.
pub const MII_SR_PREAMBLE_SUPPRESS: u32 = 0x0040;
/// `MII_SR_EXTENDED_STATUS`: Ext. status info in Reg 0x0F.
pub const MII_SR_EXTENDED_STATUS: u32 = 0x0100;
/// `MII_SR_100T2_HD_CAPS`: 100T2 Half Duplex Capable.
pub const MII_SR_100T2_HD_CAPS: u32 = 0x0200;
/// `MII_SR_100T2_FD_CAPS`: 100T2 Full Duplex Capable.
pub const MII_SR_100T2_FD_CAPS: u32 = 0x0400;
/// `MII_SR_10T_HD_CAPS`: 10T Half Duplex Capable.
pub const MII_SR_10T_HD_CAPS: u32 = 0x0800;
/// `MII_SR_10T_FD_CAPS`: 10T Full Duplex Capable.
pub const MII_SR_10T_FD_CAPS: u32 = 0x1000;
/// `MII_SR_100X_HD_CAPS`: 100X Half Duplex Capable.
pub const MII_SR_100X_HD_CAPS: u32 = 0x2000;
/// `MII_SR_100X_FD_CAPS`: 100X Full Duplex Capable.
pub const MII_SR_100X_FD_CAPS: u32 = 0x4000;
/// `MII_SR_100T4_CAPS`: 100T4 Capable.
pub const MII_SR_100T4_CAPS: u32 = 0x8000;
/// `NWAY_AR_SELECTOR_FIELD`: indicates IEEE 802.3 CSMA/CD.
pub const NWAY_AR_SELECTOR_FIELD: u32 = 0x0001;
/// `NWAY_AR_10T_HD_CAPS`: 10T Half Duplex Capable.
pub const NWAY_AR_10T_HD_CAPS: u16 = 0x0020;
/// `NWAY_AR_10T_FD_CAPS`: 10T Full Duplex Capable.
pub const NWAY_AR_10T_FD_CAPS: u16 = 0x0040;
/// `NWAY_AR_100TX_HD_CAPS`: 100TX Half Duplex Capable.
pub const NWAY_AR_100TX_HD_CAPS: u16 = 0x0080;
/// `NWAY_AR_100TX_FD_CAPS`: 100TX Full Duplex Capable.
pub const NWAY_AR_100TX_FD_CAPS: u16 = 0x0100;
/// `NWAY_AR_100T4_CAPS`: 100T4 Capable.
pub const NWAY_AR_100T4_CAPS: u32 = 0x0200;
/// `NWAY_AR_PAUSE`: Pause operation desired.
pub const NWAY_AR_PAUSE: u16 = 0x0400;
/// `NWAY_AR_ASM_DIR`: Asymmetric Pause Direction bit.
pub const NWAY_AR_ASM_DIR: u16 = 0x0800;
/// `NWAY_AR_REMOTE_FAULT`: Remote Fault detected.
pub const NWAY_AR_REMOTE_FAULT: u32 = 0x2000;
/// `NWAY_AR_NEXT_PAGE`: Next Page ability supported.
pub const NWAY_AR_NEXT_PAGE: u32 = 0x8000;
/// `NWAY_LPAR_SELECTOR_FIELD`: LP protocol selector field.
pub const NWAY_LPAR_SELECTOR_FIELD: u32 = 0x0000;
/// `NWAY_LPAR_10T_HD_CAPS`: LP is 10T Half Duplex Capable.
pub const NWAY_LPAR_10T_HD_CAPS: u32 = 0x0020;
/// `NWAY_LPAR_10T_FD_CAPS`: LP is 10T Full Duplex Capable.
pub const NWAY_LPAR_10T_FD_CAPS: u16 = 0x0040;
/// `NWAY_LPAR_100TX_HD_CAPS`: LP is 100TX Half Duplex Capable.
pub const NWAY_LPAR_100TX_HD_CAPS: u32 = 0x0080;
/// `NWAY_LPAR_100TX_FD_CAPS`: LP is 100TX Full Duplex Capable.
pub const NWAY_LPAR_100TX_FD_CAPS: u16 = 0x0100;
/// `NWAY_LPAR_100T4_CAPS`: LP is 100T4 Capable.
pub const NWAY_LPAR_100T4_CAPS: u32 = 0x0200;
/// `NWAY_LPAR_PAUSE`: LP Pause operation desired.
pub const NWAY_LPAR_PAUSE: u16 = 0x0400;
/// `NWAY_LPAR_ASM_DIR`: LP Asymmetric Pause Direction bit.
pub const NWAY_LPAR_ASM_DIR: u16 = 0x0800;
/// `NWAY_LPAR_REMOTE_FAULT`: LP has detected Remote Fault.
pub const NWAY_LPAR_REMOTE_FAULT: u32 = 0x2000;
/// `NWAY_LPAR_ACKNOWLEDGE`: LP has rx'd link code word.
pub const NWAY_LPAR_ACKNOWLEDGE: u32 = 0x4000;
/// `NWAY_LPAR_NEXT_PAGE`: Next Page ability supported.
pub const NWAY_LPAR_NEXT_PAGE: u32 = 0x8000;
/// `NWAY_ER_LP_NWAY_CAPS`: LP has Auto Neg Capability.
pub const NWAY_ER_LP_NWAY_CAPS: u16 = 0x0001;
/// `NWAY_ER_PAGE_RXD`: LP is 10T Half Duplex Capable.
pub const NWAY_ER_PAGE_RXD: u32 = 0x0002;
/// `NWAY_ER_NEXT_PAGE_CAPS`: LP is 10T Full Duplex Capable.
pub const NWAY_ER_NEXT_PAGE_CAPS: u32 = 0x0004;
/// `NWAY_ER_LP_NEXT_PAGE_CAPS`: LP is 100TX Half Duplex Capable.
pub const NWAY_ER_LP_NEXT_PAGE_CAPS: u32 = 0x0008;
/// `NWAY_ER_PAR_DETECT_FAULT`: LP is 100TX Full Duplex Capable.
pub const NWAY_ER_PAR_DETECT_FAULT: u32 = 0x0010;
/// `NPTX_MSG_CODE_FIELD`: NP msg code or unformatted data.
pub const NPTX_MSG_CODE_FIELD: u32 = 0x0001;
/// `NPTX_TOGGLE`: Toggles between exchanges of different NP.
pub const NPTX_TOGGLE: u32 = 0x0800;
/// `NPTX_ACKNOWLDGE2`: 1 = will comply with msg 0 = cannot comply with msg.
pub const NPTX_ACKNOWLDGE2: u32 = 0x1000;
/// `NPTX_MSG_PAGE`: formatted(1)/unformatted(0) pg.
pub const NPTX_MSG_PAGE: u32 = 0x2000;
/// `NPTX_NEXT_PAGE`: 1 = addition NP will follow 0 = sending last NP.
pub const NPTX_NEXT_PAGE: u32 = 0x8000;
/// `LP_RNPR_MSG_CODE_FIELD`: NP msg code or unformatted data.
pub const LP_RNPR_MSG_CODE_FIELD: u32 = 0x0001;
/// `LP_RNPR_TOGGLE`: Toggles between exchanges of different NP.
pub const LP_RNPR_TOGGLE: u32 = 0x0800;
/// `LP_RNPR_ACKNOWLDGE2`: 1 = will comply with msg 0 = cannot comply with msg.
pub const LP_RNPR_ACKNOWLDGE2: u32 = 0x1000;
/// `LP_RNPR_MSG_PAGE`: formatted(1)/unformatted(0) pg.
pub const LP_RNPR_MSG_PAGE: u32 = 0x2000;
/// `LP_RNPR_ACKNOWLDGE`: 1 = ACK / 0 = NO ACK.
pub const LP_RNPR_ACKNOWLDGE: u32 = 0x4000;
/// `LP_RNPR_NEXT_PAGE`: 1 = addition NP will follow 0 = sending last NP.
pub const LP_RNPR_NEXT_PAGE: u32 = 0x8000;
/// `CR_1000T_ASYM_PAUSE`: Advertise asymmetric pause bit.
pub const CR_1000T_ASYM_PAUSE: u32 = 0x0080;
/// `CR_1000T_HD_CAPS`: Advertise 1000T HD capability.
pub const CR_1000T_HD_CAPS: u32 = 0x0100;
/// `CR_1000T_FD_CAPS`: Advertise 1000T FD capability.
pub const CR_1000T_FD_CAPS: u16 = 0x0200;
/// `CR_1000T_REPEATER_DTE`: 1=Repeater/switch device port.
pub const CR_1000T_REPEATER_DTE: u32 = 0x0400;
/// `CR_1000T_MS_VALUE`: 1=Configure PHY as Master.
pub const CR_1000T_MS_VALUE: u16 = 0x0800;
/// `CR_1000T_MS_ENABLE`: 1=Master/Slave manual config value.
pub const CR_1000T_MS_ENABLE: u16 = 0x1000;
/// `CR_1000T_TEST_MODE_NORMAL`: Normal Operation.
pub const CR_1000T_TEST_MODE_NORMAL: u32 = 0x0000;
/// `CR_1000T_TEST_MODE_1`: Transmit Waveform test.
pub const CR_1000T_TEST_MODE_1: u32 = 0x2000;
/// `CR_1000T_TEST_MODE_2`: Master Transmit Jitter test.
pub const CR_1000T_TEST_MODE_2: u32 = 0x4000;
/// `CR_1000T_TEST_MODE_3`: Slave Transmit Jitter test.
pub const CR_1000T_TEST_MODE_3: u32 = 0x6000;
/// `CR_1000T_TEST_MODE_4`: Transmitter Distortion test.
pub const CR_1000T_TEST_MODE_4: u32 = 0x8000;
/// `SR_1000T_IDLE_ERROR_CNT`: Num idle errors since last read.
pub const SR_1000T_IDLE_ERROR_CNT: u16 = 0x00FF;
/// `SR_1000T_ASYM_PAUSE_DIR`: LP asymmetric pause direction bit.
pub const SR_1000T_ASYM_PAUSE_DIR: u32 = 0x0100;
/// `SR_1000T_LP_HD_CAPS`: LP is 1000T HD capable.
pub const SR_1000T_LP_HD_CAPS: u32 = 0x0400;
/// `SR_1000T_LP_FD_CAPS`: LP is 1000T FD capable.
pub const SR_1000T_LP_FD_CAPS: u32 = 0x0800;
/// `SR_1000T_REMOTE_RX_STATUS`: Remote receiver OK.
pub const SR_1000T_REMOTE_RX_STATUS: u32 = 0x1000;
/// `SR_1000T_LOCAL_RX_STATUS`: Local receiver OK.
pub const SR_1000T_LOCAL_RX_STATUS: u32 = 0x2000;
/// `SR_1000T_MS_CONFIG_RES`: 1=Local TX is Master, 0=Slave.
pub const SR_1000T_MS_CONFIG_RES: u32 = 0x4000;
/// `SR_1000T_MS_CONFIG_FAULT`: Master/Slave config fault.
pub const SR_1000T_MS_CONFIG_FAULT: u32 = 0x8000;
/// `SR_1000T_REMOTE_RX_STATUS_SHIFT`.
pub const SR_1000T_REMOTE_RX_STATUS_SHIFT: u32 = 12;
/// `SR_1000T_LOCAL_RX_STATUS_SHIFT`.
pub const SR_1000T_LOCAL_RX_STATUS_SHIFT: u32 = 13;
/// `SR_1000T_PHY_EXCESSIVE_IDLE_ERR_COUNT`.
pub const SR_1000T_PHY_EXCESSIVE_IDLE_ERR_COUNT: u32 = 5;
/// `FFE_IDLE_ERR_COUNT_TIMEOUT_20`.
pub const FFE_IDLE_ERR_COUNT_TIMEOUT_20: u32 = 20;
/// `FFE_IDLE_ERR_COUNT_TIMEOUT_100`.
pub const FFE_IDLE_ERR_COUNT_TIMEOUT_100: u32 = 100;
/// `IEEE_ESR_1000T_HD_CAPS`: 1000T HD capable.
pub const IEEE_ESR_1000T_HD_CAPS: u32 = 0x1000;
/// `IEEE_ESR_1000T_FD_CAPS`: 1000T FD capable.
pub const IEEE_ESR_1000T_FD_CAPS: u32 = 0x2000;
/// `IEEE_ESR_1000X_HD_CAPS`: 1000X HD capable.
pub const IEEE_ESR_1000X_HD_CAPS: u32 = 0x4000;
/// `IEEE_ESR_1000X_FD_CAPS`: 1000X FD capable.
pub const IEEE_ESR_1000X_FD_CAPS: u32 = 0x8000;
/// `PHY_TX_POLARITY_MASK`: register 10h bit 8 (polarity bit).
pub const PHY_TX_POLARITY_MASK: u32 = 0x0100;
/// `PHY_TX_NORMAL_POLARITY`: register 10h bit 8 (normal polarity).
pub const PHY_TX_NORMAL_POLARITY: u32 = 0;
/// `AUTO_POLARITY_DISABLE`: register 11h bit 4.
pub const AUTO_POLARITY_DISABLE: u32 = 0x0010;
/// `M88E1000_PSCR_JABBER_DISABLE`: 1=Jabber Function disabled.
pub const M88E1000_PSCR_JABBER_DISABLE: u32 = 0x0001;
/// `M88E1000_PSCR_POLARITY_REVERSAL`: 1=Polarity Reversal enabled.
pub const M88E1000_PSCR_POLARITY_REVERSAL: u16 = 0x0002;
/// `M88E1000_PSCR_SQE_TEST`: 1=SQE Test enabled.
pub const M88E1000_PSCR_SQE_TEST: u32 = 0x0004;
/// `M88E1000_PSCR_CLK125_DISABLE`: 1=CLK125 low, 0=CLK125 toggling.
pub const M88E1000_PSCR_CLK125_DISABLE: u32 = 0x0010;
/// `M88E1000_PSCR_MDI_MANUAL_MODE`: MDI Crossover Mode bits 6:5.
pub const M88E1000_PSCR_MDI_MANUAL_MODE: u16 = 0x0000;
/// `M88E1000_PSCR_MDIX_MANUAL_MODE`: Manual MDIX configuration.
pub const M88E1000_PSCR_MDIX_MANUAL_MODE: u16 = 0x0020;
/// `M88E1000_PSCR_AUTO_X_1000T`: 1000BASE-T: Auto crossover, 100BASE-TX/10BASE-T: MDI Mode.
pub const M88E1000_PSCR_AUTO_X_1000T: u16 = 0x0040;
/// `M88E1000_PSCR_AUTO_X_MODE`: Auto crossover enabled all speeds.
pub const M88E1000_PSCR_AUTO_X_MODE: u16 = 0x0060;
/// `M88E1000_PSCR_10BT_EXT_DIST_ENABLE`.
pub const M88E1000_PSCR_10BT_EXT_DIST_ENABLE: u32 = 0x0080;
/// `M88E1000_PSCR_MII_5BIT_ENABLE`.
pub const M88E1000_PSCR_MII_5BIT_ENABLE: u32 = 0x0100;
/// `M88E1000_PSCR_SCRAMBLER_DISABLE`: 1=Scrambler disable.
pub const M88E1000_PSCR_SCRAMBLER_DISABLE: u32 = 0x0200;
/// `M88E1000_PSCR_FORCE_LINK_GOOD`: 1=Force link good.
pub const M88E1000_PSCR_FORCE_LINK_GOOD: u32 = 0x0400;
/// `M88E1000_PSCR_ASSERT_CRS_ON_TX`: 1=Assert CRS on Transmit.
pub const M88E1000_PSCR_ASSERT_CRS_ON_TX: u16 = 0x0800;
/// `M88E1000_PSCR_POLARITY_REVERSAL_SHIFT`.
pub const M88E1000_PSCR_POLARITY_REVERSAL_SHIFT: u32 = 1;
/// `M88E1000_PSCR_AUTO_X_MODE_SHIFT`.
pub const M88E1000_PSCR_AUTO_X_MODE_SHIFT: u32 = 5;
/// `M88E1000_PSCR_10BT_EXT_DIST_ENABLE_SHIFT`.
pub const M88E1000_PSCR_10BT_EXT_DIST_ENABLE_SHIFT: u32 = 7;
/// `M88E1000_PSSR_JABBER`: 1=Jabber.
pub const M88E1000_PSSR_JABBER: u32 = 0x0001;
/// `M88E1000_PSSR_REV_POLARITY`: 1=Polarity reversed.
pub const M88E1000_PSSR_REV_POLARITY: u32 = 0x0002;
/// `M88E1000_PSSR_DOWNSHIFT`: 1=Downshifted.
pub const M88E1000_PSSR_DOWNSHIFT: u16 = 0x0020;
/// `M88E1000_PSSR_MDIX`: 1=MDIX; 0=MDI.
pub const M88E1000_PSSR_MDIX: u32 = 0x0040;
/// `M88E1000_PSSR_CABLE_LENGTH`: 0=<50M;1=50-80M;2=80-110M; 3=110-140M;4=>140M.
pub const M88E1000_PSSR_CABLE_LENGTH: u16 = 0x0380;
/// `M88E1000_PSSR_LINK`: 1=Link up, 0=Link down.
pub const M88E1000_PSSR_LINK: u32 = 0x0400;
/// `M88E1000_PSSR_SPD_DPLX_RESOLVED`: 1=Speed & Duplex resolved.
pub const M88E1000_PSSR_SPD_DPLX_RESOLVED: u32 = 0x0800;
/// `M88E1000_PSSR_PAGE_RCVD`: 1=Page received.
pub const M88E1000_PSSR_PAGE_RCVD: u32 = 0x1000;
/// `M88E1000_PSSR_DPLX`: 1=Duplex 0=Half Duplex.
pub const M88E1000_PSSR_DPLX: u16 = 0x2000;
/// `M88E1000_PSSR_SPEED`: Speed, bits 14:15.
pub const M88E1000_PSSR_SPEED: u16 = 0xC000;
/// `M88E1000_PSSR_10MBS`: 00=10Mbs.
pub const M88E1000_PSSR_10MBS: u32 = 0x0000;
/// `M88E1000_PSSR_100MBS`: 01=100Mbs.
pub const M88E1000_PSSR_100MBS: u16 = 0x4000;
/// `M88E1000_PSSR_1000MBS`: 10=1000Mbs.
pub const M88E1000_PSSR_1000MBS: u16 = 0x8000;
/// `M88E1000_PSSR_REV_POLARITY_SHIFT`.
pub const M88E1000_PSSR_REV_POLARITY_SHIFT: u32 = 1;
/// `M88E1000_PSSR_DOWNSHIFT_SHIFT`.
pub const M88E1000_PSSR_DOWNSHIFT_SHIFT: u16 = 5;
/// `M88E1000_PSSR_MDIX_SHIFT`.
pub const M88E1000_PSSR_MDIX_SHIFT: u32 = 6;
/// `M88E1000_PSSR_CABLE_LENGTH_SHIFT`.
pub const M88E1000_PSSR_CABLE_LENGTH_SHIFT: u16 = 7;
/// `M88E1000_EPSCR_FIBER_LOOPBACK`: 1=Fiber loopback.
pub const M88E1000_EPSCR_FIBER_LOOPBACK: u32 = 0x4000;
/// `M88E1000_EPSCR_DOWN_NO_IDLE`: 1=Lost lock detect enabled. Will assert lost lock and bring link down if idle not seen within 1ms in 1000BASE-T.
pub const M88E1000_EPSCR_DOWN_NO_IDLE: u32 = 0x8000;
/// `M88E1000_EPSCR_MASTER_DOWNSHIFT_MASK`.
pub const M88E1000_EPSCR_MASTER_DOWNSHIFT_MASK: u16 = 0x0C00;
/// `M88E1000_EPSCR_MASTER_DOWNSHIFT_1X`.
pub const M88E1000_EPSCR_MASTER_DOWNSHIFT_1X: u16 = 0x0000;
/// `M88E1000_EPSCR_MASTER_DOWNSHIFT_2X`.
pub const M88E1000_EPSCR_MASTER_DOWNSHIFT_2X: u32 = 0x0400;
/// `M88E1000_EPSCR_MASTER_DOWNSHIFT_3X`.
pub const M88E1000_EPSCR_MASTER_DOWNSHIFT_3X: u32 = 0x0800;
/// `M88E1000_EPSCR_MASTER_DOWNSHIFT_4X`.
pub const M88E1000_EPSCR_MASTER_DOWNSHIFT_4X: u32 = 0x0C00;
/// `M88E1000_EPSCR_SLAVE_DOWNSHIFT_MASK`.
pub const M88E1000_EPSCR_SLAVE_DOWNSHIFT_MASK: u16 = 0x0300;
/// `M88E1000_EPSCR_SLAVE_DOWNSHIFT_DIS`.
pub const M88E1000_EPSCR_SLAVE_DOWNSHIFT_DIS: u32 = 0x0000;
/// `M88E1000_EPSCR_SLAVE_DOWNSHIFT_1X`.
pub const M88E1000_EPSCR_SLAVE_DOWNSHIFT_1X: u16 = 0x0100;
/// `M88E1000_EPSCR_SLAVE_DOWNSHIFT_2X`.
pub const M88E1000_EPSCR_SLAVE_DOWNSHIFT_2X: u32 = 0x0200;
/// `M88E1000_EPSCR_SLAVE_DOWNSHIFT_3X`.
pub const M88E1000_EPSCR_SLAVE_DOWNSHIFT_3X: u32 = 0x0300;
/// `M88E1000_EPSCR_TX_CLK_2_5`: 2.5 MHz TX_CLK.
pub const M88E1000_EPSCR_TX_CLK_2_5: u32 = 0x0060;
/// `M88E1000_EPSCR_TX_CLK_25`: 25 MHz TX_CLK.
pub const M88E1000_EPSCR_TX_CLK_25: u16 = 0x0070;
/// `M88E1000_EPSCR_TX_CLK_0`: NO TX_CLK.
pub const M88E1000_EPSCR_TX_CLK_0: u32 = 0x0000;
/// `M88EC018_EPSCR_DOWNSHIFT_COUNTER_MASK`.
pub const M88EC018_EPSCR_DOWNSHIFT_COUNTER_MASK: u16 = 0x0E00;
/// `M88EC018_EPSCR_DOWNSHIFT_COUNTER_1X`.
pub const M88EC018_EPSCR_DOWNSHIFT_COUNTER_1X: u32 = 0x0000;
/// `M88EC018_EPSCR_DOWNSHIFT_COUNTER_2X`.
pub const M88EC018_EPSCR_DOWNSHIFT_COUNTER_2X: u32 = 0x0200;
/// `M88EC018_EPSCR_DOWNSHIFT_COUNTER_3X`.
pub const M88EC018_EPSCR_DOWNSHIFT_COUNTER_3X: u32 = 0x0400;
/// `M88EC018_EPSCR_DOWNSHIFT_COUNTER_4X`.
pub const M88EC018_EPSCR_DOWNSHIFT_COUNTER_4X: u32 = 0x0600;
/// `M88EC018_EPSCR_DOWNSHIFT_COUNTER_5X`.
pub const M88EC018_EPSCR_DOWNSHIFT_COUNTER_5X: u16 = 0x0800;
/// `M88EC018_EPSCR_DOWNSHIFT_COUNTER_6X`.
pub const M88EC018_EPSCR_DOWNSHIFT_COUNTER_6X: u32 = 0x0A00;
/// `M88EC018_EPSCR_DOWNSHIFT_COUNTER_7X`.
pub const M88EC018_EPSCR_DOWNSHIFT_COUNTER_7X: u32 = 0x0C00;
/// `M88EC018_EPSCR_DOWNSHIFT_COUNTER_8X`.
pub const M88EC018_EPSCR_DOWNSHIFT_COUNTER_8X: u32 = 0x0E00;
/// `M88E1000_EPSCR_TX_TIME_CTRL`: Add Delay.
pub const M88E1000_EPSCR_TX_TIME_CTRL: u16 = 0x0002;
/// `M88E1000_EPSCR_RX_TIME_CTRL`: Add Delay.
pub const M88E1000_EPSCR_RX_TIME_CTRL: u16 = 0x0080;
/// `IGP01E1000_PSCFR_AUTO_MDIX_PAR_DETECT`.
pub const IGP01E1000_PSCFR_AUTO_MDIX_PAR_DETECT: u32 = 0x0010;
/// `IGP01E1000_PSCFR_PRE_EN`.
pub const IGP01E1000_PSCFR_PRE_EN: u32 = 0x0020;
/// `IGP01E1000_PSCFR_SMART_SPEED`.
pub const IGP01E1000_PSCFR_SMART_SPEED: u16 = 0x0080;
/// `IGP01E1000_PSCFR_DISABLE_TPLOOPBACK`.
pub const IGP01E1000_PSCFR_DISABLE_TPLOOPBACK: u32 = 0x0100;
/// `IGP01E1000_PSCFR_DISABLE_JABBER`.
pub const IGP01E1000_PSCFR_DISABLE_JABBER: u32 = 0x0400;
/// `IGP01E1000_PSCFR_DISABLE_TRANSMIT`.
pub const IGP01E1000_PSCFR_DISABLE_TRANSMIT: u32 = 0x2000;
/// `IGP01E1000_PSSR_AUTONEG_FAILED`: RO LH SC.
pub const IGP01E1000_PSSR_AUTONEG_FAILED: u32 = 0x0001;
/// `IGP01E1000_PSSR_POLARITY_REVERSED`.
pub const IGP01E1000_PSSR_POLARITY_REVERSED: u32 = 0x0002;
/// `IGP01E1000_PSSR_CABLE_LENGTH`.
pub const IGP01E1000_PSSR_CABLE_LENGTH: u32 = 0x007C;
/// `IGP01E1000_PSSR_FULL_DUPLEX`.
pub const IGP01E1000_PSSR_FULL_DUPLEX: u32 = 0x0200;
/// `IGP01E1000_PSSR_LINK_UP`.
pub const IGP01E1000_PSSR_LINK_UP: u32 = 0x0400;
/// `IGP01E1000_PSSR_MDIX`.
pub const IGP01E1000_PSSR_MDIX: u32 = 0x0800;
/// `IGP01E1000_PSSR_SPEED_MASK`: speed bits mask.
pub const IGP01E1000_PSSR_SPEED_MASK: u32 = 0xC000;
/// `IGP01E1000_PSSR_SPEED_10MBPS`.
pub const IGP01E1000_PSSR_SPEED_10MBPS: u32 = 0x4000;
/// `IGP01E1000_PSSR_SPEED_100MBPS`.
pub const IGP01E1000_PSSR_SPEED_100MBPS: u32 = 0x8000;
/// `IGP01E1000_PSSR_SPEED_1000MBPS`.
pub const IGP01E1000_PSSR_SPEED_1000MBPS: u32 = 0xC000;
/// `IGP01E1000_PSSR_CABLE_LENGTH_SHIFT`: shift right 2.
pub const IGP01E1000_PSSR_CABLE_LENGTH_SHIFT: u32 = 0x0002;
/// `IGP01E1000_PSSR_MDIX_SHIFT`: shift right 11.
pub const IGP01E1000_PSSR_MDIX_SHIFT: u32 = 0x000B;
/// `IGP01E1000_PSCR_TP_LOOPBACK`.
pub const IGP01E1000_PSCR_TP_LOOPBACK: u32 = 0x0010;
/// `IGP01E1000_PSCR_CORRECT_NC_SCMBLR`.
pub const IGP01E1000_PSCR_CORRECT_NC_SCMBLR: u32 = 0x0200;
/// `IGP01E1000_PSCR_TEN_CRS_SELECT`.
pub const IGP01E1000_PSCR_TEN_CRS_SELECT: u32 = 0x0400;
/// `IGP01E1000_PSCR_FLIP_CHIP`.
pub const IGP01E1000_PSCR_FLIP_CHIP: u32 = 0x0800;
/// `IGP01E1000_PSCR_AUTO_MDIX`.
pub const IGP01E1000_PSCR_AUTO_MDIX: u16 = 0x1000;
/// `IGP01E1000_PSCR_FORCE_MDI_MDIX`: 0-MDI, 1-MDIX.
pub const IGP01E1000_PSCR_FORCE_MDI_MDIX: u16 = 0x2000;
/// `IGP01E1000_PLHR_SS_DOWNGRADE`.
pub const IGP01E1000_PLHR_SS_DOWNGRADE: u16 = 0x8000;
/// `IGP01E1000_PLHR_GIG_SCRAMBLER_ERROR`.
pub const IGP01E1000_PLHR_GIG_SCRAMBLER_ERROR: u32 = 0x4000;
/// `IGP01E1000_PLHR_MASTER_FAULT`.
pub const IGP01E1000_PLHR_MASTER_FAULT: u32 = 0x2000;
/// `IGP01E1000_PLHR_MASTER_RESOLUTION`.
pub const IGP01E1000_PLHR_MASTER_RESOLUTION: u32 = 0x1000;
/// `IGP01E1000_PLHR_GIG_REM_RCVR_NOK`: LH.
pub const IGP01E1000_PLHR_GIG_REM_RCVR_NOK: u32 = 0x0800;
/// `IGP01E1000_PLHR_IDLE_ERROR_CNT_OFLOW`: LH.
pub const IGP01E1000_PLHR_IDLE_ERROR_CNT_OFLOW: u32 = 0x0400;
/// `IGP01E1000_PLHR_DATA_ERR_1`: LH.
pub const IGP01E1000_PLHR_DATA_ERR_1: u32 = 0x0200;
/// `IGP01E1000_PLHR_DATA_ERR_0`.
pub const IGP01E1000_PLHR_DATA_ERR_0: u32 = 0x0100;
/// `IGP01E1000_PLHR_AUTONEG_FAULT`.
pub const IGP01E1000_PLHR_AUTONEG_FAULT: u32 = 0x0040;
/// `IGP01E1000_PLHR_AUTONEG_ACTIVE`.
pub const IGP01E1000_PLHR_AUTONEG_ACTIVE: u32 = 0x0010;
/// `IGP01E1000_PLHR_VALID_CHANNEL_D`.
pub const IGP01E1000_PLHR_VALID_CHANNEL_D: u32 = 0x0008;
/// `IGP01E1000_PLHR_VALID_CHANNEL_C`.
pub const IGP01E1000_PLHR_VALID_CHANNEL_C: u32 = 0x0004;
/// `IGP01E1000_PLHR_VALID_CHANNEL_B`.
pub const IGP01E1000_PLHR_VALID_CHANNEL_B: u32 = 0x0002;
/// `IGP01E1000_PLHR_VALID_CHANNEL_A`.
pub const IGP01E1000_PLHR_VALID_CHANNEL_A: u32 = 0x0001;
/// `IGP01E1000_MSE_CHANNEL_D`.
pub const IGP01E1000_MSE_CHANNEL_D: u32 = 0x000F;
/// `IGP01E1000_MSE_CHANNEL_C`.
pub const IGP01E1000_MSE_CHANNEL_C: u32 = 0x00F0;
/// `IGP01E1000_MSE_CHANNEL_B`.
pub const IGP01E1000_MSE_CHANNEL_B: u32 = 0x0F00;
/// `IGP01E1000_MSE_CHANNEL_A`.
pub const IGP01E1000_MSE_CHANNEL_A: u32 = 0xF000;
/// `IGP02E1000_PM_SPD`: Smart Power Down.
pub const IGP02E1000_PM_SPD: u32 = 0x0001;
/// `IGP02E1000_PM_D3_LPLU`: Enable LPLU in non-D0a modes.
pub const IGP02E1000_PM_D3_LPLU: u16 = 0x0004;
/// `IGP02E1000_PM_D0_LPLU`: Enable LPLU in D0a mode.
pub const IGP02E1000_PM_D0_LPLU: u16 = 0x0002;
/// `DSP_RESET_ENABLE`.
pub const DSP_RESET_ENABLE: u32 = 0x0;
/// `DSP_RESET_DISABLE`.
pub const DSP_RESET_DISABLE: u32 = 0x2;
/// `E1000_MAX_DSP_RESETS`.
pub const E1000_MAX_DSP_RESETS: u32 = 10;
/// `IGP01E1000_AGC_LENGTH_SHIFT`: Coarse - 13:11, Fine - 10:7.
pub const IGP01E1000_AGC_LENGTH_SHIFT: u32 = 7;
/// `IGP02E1000_AGC_LENGTH_SHIFT`: Coarse - 15:13, Fine - 12:9.
pub const IGP02E1000_AGC_LENGTH_SHIFT: u32 = 9;
/// `IGP02E1000_AGC_LENGTH_MASK`.
pub const IGP02E1000_AGC_LENGTH_MASK: u16 = 0x7F;
/// `IGP01E1000_AGC_LENGTH_TABLE_SIZE`.
pub const IGP01E1000_AGC_LENGTH_TABLE_SIZE: usize = 128;
/// `IGP02E1000_AGC_LENGTH_TABLE_SIZE`.
pub const IGP02E1000_AGC_LENGTH_TABLE_SIZE: usize = 113;
/// `IGP01E1000_AGC_RANGE`.
pub const IGP01E1000_AGC_RANGE: u16 = 10;
/// `IGP02E1000_AGC_RANGE`.
pub const IGP02E1000_AGC_RANGE: u16 = 15;
/// `IGP01E1000_PHY_POLARITY_MASK`.
pub const IGP01E1000_PHY_POLARITY_MASK: u32 = 0x0078;
/// `IGP01E1000_GMII_FLEX_SPD`: Enable flexible speed on Link-Up.
pub const IGP01E1000_GMII_FLEX_SPD: u16 = 0x10;
/// `IGP01E1000_GMII_SPD`: Enable SPD.
pub const IGP01E1000_GMII_SPD: u32 = 0x20;
/// `IGP01E1000_ANALOG_SPARE_FUSE_STATUS`.
pub const IGP01E1000_ANALOG_SPARE_FUSE_STATUS: u32 = 0x20D1;
/// `IGP01E1000_ANALOG_FUSE_STATUS`.
pub const IGP01E1000_ANALOG_FUSE_STATUS: u32 = 0x20D0;
/// `IGP01E1000_ANALOG_FUSE_CONTROL`.
pub const IGP01E1000_ANALOG_FUSE_CONTROL: u32 = 0x20DC;
/// `IGP01E1000_ANALOG_FUSE_BYPASS`.
pub const IGP01E1000_ANALOG_FUSE_BYPASS: u32 = 0x20DE;
/// `IGP01E1000_ANALOG_FUSE_POLY_MASK`.
pub const IGP01E1000_ANALOG_FUSE_POLY_MASK: u16 = 0xF000;
/// `IGP01E1000_ANALOG_FUSE_FINE_MASK`.
pub const IGP01E1000_ANALOG_FUSE_FINE_MASK: u16 = 0x0F80;
/// `IGP01E1000_ANALOG_FUSE_COARSE_MASK`.
pub const IGP01E1000_ANALOG_FUSE_COARSE_MASK: u16 = 0x0070;
/// `IGP01E1000_ANALOG_SPARE_FUSE_ENABLED`.
pub const IGP01E1000_ANALOG_SPARE_FUSE_ENABLED: u16 = 0x0100;
/// `IGP01E1000_ANALOG_FUSE_ENABLE_SW_CONTROL`.
pub const IGP01E1000_ANALOG_FUSE_ENABLE_SW_CONTROL: u16 = 0x0002;
/// `IGP01E1000_ANALOG_FUSE_COARSE_THRESH`.
pub const IGP01E1000_ANALOG_FUSE_COARSE_THRESH: u16 = 0x0040;
/// `IGP01E1000_ANALOG_FUSE_COARSE_10`.
pub const IGP01E1000_ANALOG_FUSE_COARSE_10: u16 = 0x0010;
/// `IGP01E1000_ANALOG_FUSE_FINE_1`.
pub const IGP01E1000_ANALOG_FUSE_FINE_1: u16 = 0x0080;
/// `IGP01E1000_ANALOG_FUSE_FINE_10`.
pub const IGP01E1000_ANALOG_FUSE_FINE_10: u16 = 0x0500;
/// `GG82563_PSCR_DISABLE_JABBER`: 1=Disable Jabber.
pub const GG82563_PSCR_DISABLE_JABBER: u32 = 0x0001;
/// `GG82563_PSCR_POLARITY_REVERSAL_DISABLE`: 1=Polarity Reversal Disabled.
pub const GG82563_PSCR_POLARITY_REVERSAL_DISABLE: u16 = 0x0002;
/// `GG82563_PSCR_POWER_DOWN`: 1=Power Down.
pub const GG82563_PSCR_POWER_DOWN: u32 = 0x0004;
/// `GG82563_PSCR_COPPER_TRANSMITER_DISABLE`: 1=Transmitter Disabled.
pub const GG82563_PSCR_COPPER_TRANSMITER_DISABLE: u32 = 0x0008;
/// `GG82563_PSCR_CROSSOVER_MODE_MASK`.
pub const GG82563_PSCR_CROSSOVER_MODE_MASK: u16 = 0x0060;
/// `GG82563_PSCR_CROSSOVER_MODE_MDI`: 00=Manual MDI configuration.
pub const GG82563_PSCR_CROSSOVER_MODE_MDI: u16 = 0x0000;
/// `GG82563_PSCR_CROSSOVER_MODE_MDIX`: 01=Manual MDIX configuration.
pub const GG82563_PSCR_CROSSOVER_MODE_MDIX: u16 = 0x0020;
/// `GG82563_PSCR_CROSSOVER_MODE_AUTO`: 11=Automatic crossover.
pub const GG82563_PSCR_CROSSOVER_MODE_AUTO: u16 = 0x0060;
/// `GG82563_PSCR_ENALBE_EXTENDED_DISTANCE`: 1=Enable Extended Distance.
pub const GG82563_PSCR_ENALBE_EXTENDED_DISTANCE: u32 = 0x0080;
/// `GG82563_PSCR_ENERGY_DETECT_MASK`.
pub const GG82563_PSCR_ENERGY_DETECT_MASK: u32 = 0x0300;
/// `GG82563_PSCR_ENERGY_DETECT_OFF`: 00,01=Off.
pub const GG82563_PSCR_ENERGY_DETECT_OFF: u32 = 0x0000;
/// `GG82563_PSCR_ENERGY_DETECT_RX`: 10=Sense on Rx only (Energy Detect).
pub const GG82563_PSCR_ENERGY_DETECT_RX: u32 = 0x0200;
/// `GG82563_PSCR_ENERGY_DETECT_RX_TM`: 11=Sense and Tx NLP.
pub const GG82563_PSCR_ENERGY_DETECT_RX_TM: u32 = 0x0300;
/// `GG82563_PSCR_FORCE_LINK_GOOD`: 1=Force Link Good.
pub const GG82563_PSCR_FORCE_LINK_GOOD: u32 = 0x0400;
/// `GG82563_PSCR_DOWNSHIFT_ENABLE`: 1=Enable Downshift.
pub const GG82563_PSCR_DOWNSHIFT_ENABLE: u32 = 0x0800;
/// `GG82563_PSCR_DOWNSHIFT_COUNTER_MASK`.
pub const GG82563_PSCR_DOWNSHIFT_COUNTER_MASK: u32 = 0x7000;
/// `GG82563_PSCR_DOWNSHIFT_COUNTER_SHIFT`.
pub const GG82563_PSCR_DOWNSHIFT_COUNTER_SHIFT: u32 = 12;
/// `GG82563_PSSR_JABBER`: 1=Jabber.
pub const GG82563_PSSR_JABBER: u32 = 0x0001;
/// `GG82563_PSSR_POLARITY`: 1=Polarity Reversed.
pub const GG82563_PSSR_POLARITY: u32 = 0x0002;
/// `GG82563_PSSR_LINK`: 1=Link is Up.
pub const GG82563_PSSR_LINK: u32 = 0x0008;
/// `GG82563_PSSR_ENERGY_DETECT`: 1=Sleep, 0=Active.
pub const GG82563_PSSR_ENERGY_DETECT: u32 = 0x0010;
/// `GG82563_PSSR_DOWNSHIFT`: 1=Downshift.
pub const GG82563_PSSR_DOWNSHIFT: u32 = 0x0020;
/// `GG82563_PSSR_CROSSOVER_STATUS`: 1=MDIX, 0=MDI.
pub const GG82563_PSSR_CROSSOVER_STATUS: u32 = 0x0040;
/// `GG82563_PSSR_RX_PAUSE_ENABLED`: 1=Receive Pause Enabled.
pub const GG82563_PSSR_RX_PAUSE_ENABLED: u32 = 0x0100;
/// `GG82563_PSSR_TX_PAUSE_ENABLED`: 1=Transmit Pause Enabled.
pub const GG82563_PSSR_TX_PAUSE_ENABLED: u32 = 0x0200;
/// `GG82563_PSSR_LINK_UP`: 1=Link Up.
pub const GG82563_PSSR_LINK_UP: u32 = 0x0400;
/// `GG82563_PSSR_SPEED_DUPLEX_RESOLVED`: 1=Resolved.
pub const GG82563_PSSR_SPEED_DUPLEX_RESOLVED: u32 = 0x0800;
/// `GG82563_PSSR_PAGE_RECEIVED`: 1=Page Received.
pub const GG82563_PSSR_PAGE_RECEIVED: u32 = 0x1000;
/// `GG82563_PSSR_DUPLEX`: 1-Full-Duplex.
pub const GG82563_PSSR_DUPLEX: u32 = 0x2000;
/// `GG82563_PSSR_SPEED_MASK`.
pub const GG82563_PSSR_SPEED_MASK: u32 = 0xC000;
/// `GG82563_PSSR_SPEED_10MBPS`: 00=10Mbps.
pub const GG82563_PSSR_SPEED_10MBPS: u32 = 0x0000;
/// `GG82563_PSSR_SPEED_100MBPS`: 01=100Mbps.
pub const GG82563_PSSR_SPEED_100MBPS: u32 = 0x4000;
/// `GG82563_PSSR_SPEED_1000MBPS`: 10=1000Mbps.
pub const GG82563_PSSR_SPEED_1000MBPS: u32 = 0x8000;
/// `GG82563_PSSR2_JABBER`: 1=Jabber.
pub const GG82563_PSSR2_JABBER: u32 = 0x0001;
/// `GG82563_PSSR2_POLARITY_CHANGED`: 1=Polarity Changed.
pub const GG82563_PSSR2_POLARITY_CHANGED: u32 = 0x0002;
/// `GG82563_PSSR2_ENERGY_DETECT_CHANGED`: 1=Energy Detect Changed.
pub const GG82563_PSSR2_ENERGY_DETECT_CHANGED: u32 = 0x0010;
/// `GG82563_PSSR2_DOWNSHIFT_INTERRUPT`: 1=Downshift Detected.
pub const GG82563_PSSR2_DOWNSHIFT_INTERRUPT: u32 = 0x0020;
/// `GG82563_PSSR2_MDI_CROSSOVER_CHANGE`: 1=Crossover Changed.
pub const GG82563_PSSR2_MDI_CROSSOVER_CHANGE: u32 = 0x0040;
/// `GG82563_PSSR2_FALSE_CARRIER`: 1=False Carrier.
pub const GG82563_PSSR2_FALSE_CARRIER: u32 = 0x0100;
/// `GG82563_PSSR2_SYMBOL_ERROR`: 1=Symbol Error.
pub const GG82563_PSSR2_SYMBOL_ERROR: u32 = 0x0200;
/// `GG82563_PSSR2_LINK_STATUS_CHANGED`: 1=Link Status Changed.
pub const GG82563_PSSR2_LINK_STATUS_CHANGED: u32 = 0x0400;
/// `GG82563_PSSR2_AUTO_NEG_COMPLETED`: 1=Auto-Neg Completed.
pub const GG82563_PSSR2_AUTO_NEG_COMPLETED: u32 = 0x0800;
/// `GG82563_PSSR2_PAGE_RECEIVED`: 1=Page Received.
pub const GG82563_PSSR2_PAGE_RECEIVED: u32 = 0x1000;
/// `GG82563_PSSR2_DUPLEX_CHANGED`: 1=Duplex Changed.
pub const GG82563_PSSR2_DUPLEX_CHANGED: u32 = 0x2000;
/// `GG82563_PSSR2_SPEED_CHANGED`: 1=Speed Changed.
pub const GG82563_PSSR2_SPEED_CHANGED: u32 = 0x4000;
/// `GG82563_PSSR2_AUTO_NEG_ERROR`: 1=Auto-Neg Error.
pub const GG82563_PSSR2_AUTO_NEG_ERROR: u32 = 0x8000;
/// `GG82563_PSCR2_10BT_POLARITY_FORCE`: 1=Force Negative Polarity.
pub const GG82563_PSCR2_10BT_POLARITY_FORCE: u32 = 0x0002;
/// `GG82563_PSCR2_1000MB_TEST_SELECT_MASK`.
pub const GG82563_PSCR2_1000MB_TEST_SELECT_MASK: u32 = 0x000C;
/// `GG82563_PSCR2_1000MB_TEST_SELECT_NORMAL`: 00,01=Normal Operation.
pub const GG82563_PSCR2_1000MB_TEST_SELECT_NORMAL: u32 = 0x0000;
/// `GG82563_PSCR2_1000MB_TEST_SELECT_112NS`: 10=Select 112ns Sequence.
pub const GG82563_PSCR2_1000MB_TEST_SELECT_112NS: u32 = 0x0008;
/// `GG82563_PSCR2_1000MB_TEST_SELECT_16NS`: 11=Select 16ns Sequence.
pub const GG82563_PSCR2_1000MB_TEST_SELECT_16NS: u32 = 0x000C;
/// `GG82563_PSCR2_REVERSE_AUTO_NEG`: 1=Reverse Auto-Negotiation.
pub const GG82563_PSCR2_REVERSE_AUTO_NEG: u16 = 0x2000;
/// `GG82563_PSCR2_1000BT_DISABLE`: 1=Disable 1000BASE-T.
pub const GG82563_PSCR2_1000BT_DISABLE: u32 = 0x4000;
/// `GG82563_PSCR2_TRANSMITER_TYPE_MASK`.
pub const GG82563_PSCR2_TRANSMITER_TYPE_MASK: u32 = 0x8000;
/// `GG82563_PSCR2_TRANSMITTER_TYPE_CLASS_B`: 0=Class B.
pub const GG82563_PSCR2_TRANSMITTER_TYPE_CLASS_B: u32 = 0x0000;
/// `GG82563_PSCR2_TRANSMITTER_TYPE_CLASS_A`: 1=Class A.
pub const GG82563_PSCR2_TRANSMITTER_TYPE_CLASS_A: u32 = 0x8000;
/// `GG82563_MSCR_TX_CLK_MASK`.
pub const GG82563_MSCR_TX_CLK_MASK: u16 = 0x0007;
/// `GG82563_MSCR_TX_CLK_10MBPS_2_5MHZ`.
pub const GG82563_MSCR_TX_CLK_10MBPS_2_5MHZ: u16 = 0x0004;
/// `GG82563_MSCR_TX_CLK_100MBPS_25MHZ`.
pub const GG82563_MSCR_TX_CLK_100MBPS_25MHZ: u16 = 0x0005;
/// `GG82563_MSCR_TX_CLK_1000MBPS_2_5MHZ`.
pub const GG82563_MSCR_TX_CLK_1000MBPS_2_5MHZ: u32 = 0x0006;
/// `GG82563_MSCR_TX_CLK_1000MBPS_25MHZ`.
pub const GG82563_MSCR_TX_CLK_1000MBPS_25MHZ: u16 = 0x0007;
/// `GG82563_MSCR_ASSERT_CRS_ON_TX`: 1=Assert.
pub const GG82563_MSCR_ASSERT_CRS_ON_TX: u16 = 0x0010;
/// `GG82563_DSPD_CABLE_LENGTH`: 0 = <50M; 1 = 50-80M; 2 = 80-110M; 3 = 110-140M; 4 = >140M.
pub const GG82563_DSPD_CABLE_LENGTH: u16 = 0x0007;
/// `GG82563_KMCR_PHY_LEDS_EN`: 1=PHY LEDs, 0=Kumeran Inband LEDs.
pub const GG82563_KMCR_PHY_LEDS_EN: u32 = 0x0020;
/// `GG82563_KMCR_FORCE_LINK_UP`: 1=Force Link Up.
pub const GG82563_KMCR_FORCE_LINK_UP: u32 = 0x0040;
/// `GG82563_KMCR_SUPPRESS_SGMII_EPD_EXT`.
pub const GG82563_KMCR_SUPPRESS_SGMII_EPD_EXT: u32 = 0x0080;
/// `GG82563_KMCR_MDIO_BUS_SPEED_SELECT_MASK`.
pub const GG82563_KMCR_MDIO_BUS_SPEED_SELECT_MASK: u32 = 0x0400;
/// `GG82563_KMCR_MDIO_BUS_SPEED_SELECT`: 1=6.25MHz, 0=0.8MHz.
pub const GG82563_KMCR_MDIO_BUS_SPEED_SELECT: u32 = 0x0400;
/// `GG82563_KMCR_PASS_FALSE_CARRIER`.
pub const GG82563_KMCR_PASS_FALSE_CARRIER: u16 = 0x0800;
/// `GG82563_PMCR_ENABLE_ELECTRICAL_IDLE`: 1=Enable SERDES Electrical Idle.
pub const GG82563_PMCR_ENABLE_ELECTRICAL_IDLE: u16 = 0x0001;
/// `GG82563_PMCR_DISABLE_PORT`: 1=Disable Port.
pub const GG82563_PMCR_DISABLE_PORT: u32 = 0x0002;
/// `GG82563_PMCR_DISABLE_SERDES`: 1=Disable SERDES.
pub const GG82563_PMCR_DISABLE_SERDES: u32 = 0x0004;
/// `GG82563_PMCR_REVERSE_AUTO_NEG`: 1=Enable Reverse Auto-Negotiation.
pub const GG82563_PMCR_REVERSE_AUTO_NEG: u32 = 0x0008;
/// `GG82563_PMCR_DISABLE_1000_NON_D0`: 1=Disable 1000Mbps Auto-Neg in non D0.
pub const GG82563_PMCR_DISABLE_1000_NON_D0: u32 = 0x0010;
/// `GG82563_PMCR_DISABLE_1000`: 1=Disable 1000Mbps Auto-Neg Always.
pub const GG82563_PMCR_DISABLE_1000: u32 = 0x0020;
/// `GG82563_PMCR_REVERSE_AUTO_NEG_D0A`: 1=Enable D0a Reverse Auto-Negotiation.
pub const GG82563_PMCR_REVERSE_AUTO_NEG_D0A: u32 = 0x0040;
/// `GG82563_PMCR_FORCE_POWER_STATE`: 1=Force Power State.
pub const GG82563_PMCR_FORCE_POWER_STATE: u32 = 0x0080;
/// `GG82563_PMCR_PROGRAMMED_POWER_STATE_MASK`.
pub const GG82563_PMCR_PROGRAMMED_POWER_STATE_MASK: u32 = 0x0300;
/// `GG82563_PMCR_PROGRAMMED_POWER_STATE_DR`: 00=Dr.
pub const GG82563_PMCR_PROGRAMMED_POWER_STATE_DR: u32 = 0x0000;
/// `GG82563_PMCR_PROGRAMMED_POWER_STATE_D0U`: 01=D0u.
pub const GG82563_PMCR_PROGRAMMED_POWER_STATE_D0U: u32 = 0x0100;
/// `GG82563_PMCR_PROGRAMMED_POWER_STATE_D0A`: 10=D0a.
pub const GG82563_PMCR_PROGRAMMED_POWER_STATE_D0A: u32 = 0x0200;
/// `GG82563_PMCR_PROGRAMMED_POWER_STATE_D3`: 11=D3.
pub const GG82563_PMCR_PROGRAMMED_POWER_STATE_D3: u32 = 0x0300;
/// `GG82563_ICR_DIS_PADDING`: Disable Padding Use.
pub const GG82563_ICR_DIS_PADDING: u16 = 0x0010;
/// `M88_VENDOR`.
pub const M88_VENDOR: u32 = 0x0141;
/// `M88E1000_E_PHY_ID`.
pub const M88E1000_E_PHY_ID: u32 = 0x01410C50;
/// `M88E1000_I_PHY_ID`.
pub const M88E1000_I_PHY_ID: u32 = 0x01410C30;
/// `M88E1011_I_PHY_ID`.
pub const M88E1011_I_PHY_ID: u32 = 0x01410C20;
/// `IGP01E1000_I_PHY_ID`.
pub const IGP01E1000_I_PHY_ID: u32 = 0x02A80380;
/// `M88E1000_12_PHY_ID`.
pub const M88E1000_12_PHY_ID: u32 = M88E1000_E_PHY_ID;
/// `M88E1000_14_PHY_ID`.
pub const M88E1000_14_PHY_ID: u32 = M88E1000_E_PHY_ID;
/// `M88E1011_I_REV_4`.
pub const M88E1011_I_REV_4: u32 = 0x04;
/// `M88E1111_I_PHY_ID`.
pub const M88E1111_I_PHY_ID: u32 = 0x01410CC0;
/// `M88E1112_E_PHY_ID`.
pub const M88E1112_E_PHY_ID: u32 = 0x01410C90;
/// `I347AT4_E_PHY_ID`.
pub const I347AT4_E_PHY_ID: u32 = 0x01410DC0;
/// `L1LXT971A_PHY_ID`.
pub const L1LXT971A_PHY_ID: u32 = 0x001378E0;
/// `GG82563_E_PHY_ID`.
pub const GG82563_E_PHY_ID: u32 = 0x01410CA0;
/// `BME1000_E_PHY_ID`.
pub const BME1000_E_PHY_ID: u32 = 0x01410CB0;
/// `BME1000_E_PHY_ID_R2`.
pub const BME1000_E_PHY_ID_R2: u32 = 0x01410CB1;
/// `M88E1543_E_PHY_ID`.
pub const M88E1543_E_PHY_ID: u32 = 0x01410EA0;
/// `I82577_E_PHY_ID`.
pub const I82577_E_PHY_ID: u32 = 0x01540050;
/// `I82578_E_PHY_ID`.
pub const I82578_E_PHY_ID: u32 = 0x004DD040;
/// `I82579_E_PHY_ID`.
pub const I82579_E_PHY_ID: u32 = 0x01540090;
/// `I217_E_PHY_ID`.
pub const I217_E_PHY_ID: u32 = 0x015400A0;
/// `I82580_I_PHY_ID`.
pub const I82580_I_PHY_ID: u32 = 0x015403A0;
/// `I350_I_PHY_ID`.
pub const I350_I_PHY_ID: u32 = 0x015403B0;
/// `I210_I_PHY_ID`.
pub const I210_I_PHY_ID: u32 = 0x01410C00;
/// `IGP04E1000_E_PHY_ID`.
pub const IGP04E1000_E_PHY_ID: u32 = 0x02A80391;
/// `M88E1141_E_PHY_ID`.
pub const M88E1141_E_PHY_ID: u32 = 0x01410CD0;
/// `M88E1512_E_PHY_ID`.
pub const M88E1512_E_PHY_ID: u32 = 0x01410DD0;
/// `PHY_PAGE_SHIFT`.
pub const PHY_PAGE_SHIFT: u32 = 5;
/// `PHY_REG(page, reg)`: a paged PHY register address.
pub const fn phy_reg(page: u32, reg: u32) -> u32 {
    (page << PHY_PAGE_SHIFT) | (reg & MAX_PHY_REG_ADDRESS)
}
/// `IGP3_PHY_PORT_CTRL`: Port General Configuration.
pub const IGP3_PHY_PORT_CTRL: u32 = phy_reg(769, 17);
/// `IGP3_PHY_RATE_ADAPT_CTRL`: Rate Adapter Control Register.
pub const IGP3_PHY_RATE_ADAPT_CTRL: u32 = phy_reg(769, 25);
/// `IGP3_KMRN_FIFO_CTRL_STATS`: KMRN FIFO's control/status register.
pub const IGP3_KMRN_FIFO_CTRL_STATS: u32 = phy_reg(770, 16);
/// `IGP3_KMRN_POWER_MNG_CTRL`: KMRN Power Management Control Register.
pub const IGP3_KMRN_POWER_MNG_CTRL: u32 = phy_reg(770, 17);
/// `IGP3_KMRN_INBAND_CTRL`: KMRN Inband Control Register.
pub const IGP3_KMRN_INBAND_CTRL: u32 = phy_reg(770, 18);
/// `IGP3_KMRN_DIAG`: KMRN Diagnostic register.
pub const IGP3_KMRN_DIAG: u32 = phy_reg(770, 19);
/// `IGP3_KMRN_DIAG_PCS_LOCK_LOSS`: RX PCS is not synced.
pub const IGP3_KMRN_DIAG_PCS_LOCK_LOSS: u16 = 0x0002;
/// `IGP3_KMRN_ACK_TIMEOUT`: KMRN Acknowledge Timeouts register.
pub const IGP3_KMRN_ACK_TIMEOUT: u32 = phy_reg(770, 20);
/// `IGP3_VR_CTRL`: Voltage regulator control register.
pub const IGP3_VR_CTRL: u32 = phy_reg(776, 18);
/// `IGP3_VR_CTRL_MODE_SHUT`: Enter powerdown, shutdown VRs.
pub const IGP3_VR_CTRL_MODE_SHUT: u32 = 0x0200;
/// `IGP3_VR_CTRL_MODE_MASK`: Shutdown VR Mask.
pub const IGP3_VR_CTRL_MODE_MASK: u32 = 0x0300;
/// `IGP3_CAPABILITY`: IGP3 Capability Register.
pub const IGP3_CAPABILITY: u32 = phy_reg(776, 19);
/// `IGP3_CAP_INITIATE_TEAM`: Able to initiate a team.
pub const IGP3_CAP_INITIATE_TEAM: u32 = 0x0001;
/// `IGP3_CAP_WFM`: Support WoL and PXE.
pub const IGP3_CAP_WFM: u32 = 0x0002;
/// `IGP3_CAP_ASF`: Support ASF.
pub const IGP3_CAP_ASF: u32 = 0x0004;
/// `IGP3_CAP_LPLU`: Support Low Power Link Up.
pub const IGP3_CAP_LPLU: u32 = 0x0008;
/// `IGP3_CAP_DC_AUTO_SPEED`: Support AC/DC Auto Link Speed.
pub const IGP3_CAP_DC_AUTO_SPEED: u32 = 0x0010;
/// `IGP3_CAP_SPD`: Support Smart Power Down.
pub const IGP3_CAP_SPD: u32 = 0x0020;
/// `IGP3_CAP_MULT_QUEUE`: Support 2 tx & 2 rx queues.
pub const IGP3_CAP_MULT_QUEUE: u32 = 0x0040;
/// `IGP3_CAP_RSS`: Support RSS.
pub const IGP3_CAP_RSS: u32 = 0x0080;
/// `IGP3_CAP_8021PQ`: Support 802.1Q & 802.1p.
pub const IGP3_CAP_8021PQ: u32 = 0x0100;
/// `IGP3_CAP_AMT_CB`: Support active manageability and circuit breaker.
pub const IGP3_CAP_AMT_CB: u32 = 0x0200;
/// `IGP3_PPC_JORDAN_EN`.
pub const IGP3_PPC_JORDAN_EN: u32 = 0x0001;
/// `IGP3_PPC_JORDAN_GIGA_SPEED`.
pub const IGP3_PPC_JORDAN_GIGA_SPEED: u32 = 0x0002;
/// `IGP3_KMRN_PMC_EE_IDLE_LINK_DIS`.
pub const IGP3_KMRN_PMC_EE_IDLE_LINK_DIS: u32 = 0x0001;
/// `IGP3_KMRN_PMC_K0S_ENTRY_LATENCY_MASK`.
pub const IGP3_KMRN_PMC_K0S_ENTRY_LATENCY_MASK: u32 = 0x001E;
/// `IGP3_KMRN_PMC_K0S_MODE1_EN_GIGA`.
pub const IGP3_KMRN_PMC_K0S_MODE1_EN_GIGA: u32 = 0x0020;
/// `IGP3_KMRN_PMC_K0S_MODE1_EN_100`.
pub const IGP3_KMRN_PMC_K0S_MODE1_EN_100: u32 = 0x0040;
/// `IGP3E1000_PHY_MISC_CTRL`: Misc. Ctrl register.
pub const IGP3E1000_PHY_MISC_CTRL: u32 = 0x1B;
/// `IGP3_PHY_MISC_DUPLEX_MANUAL_SET`: Duplex Manual Set.
pub const IGP3_PHY_MISC_DUPLEX_MANUAL_SET: u32 = 0x1000;
/// `IGP3_KMRN_EXT_CTRL`.
pub const IGP3_KMRN_EXT_CTRL: u32 = phy_reg(770, 18);
/// `IGP3_KMRN_EC_DIS_INBAND`.
pub const IGP3_KMRN_EC_DIS_INBAND: u32 = 0x0080;
/// `IGP03E1000_E_PHY_ID`.
pub const IGP03E1000_E_PHY_ID: u32 = 0x02A80390;
/// `IFE_E_PHY_ID`: 10/100 PHY.
pub const IFE_E_PHY_ID: u32 = 0x02A80330;
/// `IFE_PLUS_E_PHY_ID`.
pub const IFE_PLUS_E_PHY_ID: u32 = 0x02A80320;
/// `IFE_C_E_PHY_ID`.
pub const IFE_C_E_PHY_ID: u32 = 0x02A80310;
/// `IFE_PHY_EXTENDED_STATUS_CONTROL`: 100BaseTx Extended Status, Control and Address.
pub const IFE_PHY_EXTENDED_STATUS_CONTROL: u32 = 0x10;
/// `IFE_PHY_SPECIAL_CONTROL`: 100BaseTx PHY special control register.
pub const IFE_PHY_SPECIAL_CONTROL: u32 = 0x11;
/// `IFE_PHY_RCV_FALSE_CARRIER`: 100BaseTx Receive False Carrier Counter.
pub const IFE_PHY_RCV_FALSE_CARRIER: u32 = 0x13;
/// `IFE_PHY_RCV_DISCONNECT`: 100BaseTx Receive Disconnect Counter.
pub const IFE_PHY_RCV_DISCONNECT: u32 = 0x14;
/// `IFE_PHY_RCV_ERROT_FRAME`: 100BaseTx Receive Error Frame Counter.
pub const IFE_PHY_RCV_ERROT_FRAME: u32 = 0x15;
/// `IFE_PHY_RCV_SYMBOL_ERR`: Receive Symbol Error Counter.
pub const IFE_PHY_RCV_SYMBOL_ERR: u32 = 0x16;
/// `IFE_PHY_PREM_EOF_ERR`: 100BaseTx Receive Premature End Of Frame Error Counter.
pub const IFE_PHY_PREM_EOF_ERR: u32 = 0x17;
/// `IFE_PHY_RCV_EOF_ERR`: 10BaseT Receive End Of Frame Error Counter.
pub const IFE_PHY_RCV_EOF_ERR: u32 = 0x18;
/// `IFE_PHY_TX_JABBER_DETECT`: 10BaseT Transmit Jabber Detect Counter.
pub const IFE_PHY_TX_JABBER_DETECT: u32 = 0x19;
/// `IFE_PHY_EQUALIZER`: PHY Equalizer Control and Status.
pub const IFE_PHY_EQUALIZER: u32 = 0x1A;
/// `IFE_PHY_SPECIAL_CONTROL_LED`: PHY special control and LED configuration.
pub const IFE_PHY_SPECIAL_CONTROL_LED: u32 = 0x1B;
/// `IFE_PHY_MDIX_CONTROL`: MDI/MDI-X Control register.
pub const IFE_PHY_MDIX_CONTROL: u32 = 0x1C;
/// `IFE_PHY_HWI_CONTROL`: Hardware Integrity Control (HWI).
pub const IFE_PHY_HWI_CONTROL: u32 = 0x1D;
/// `IFE_PESC_REDUCED_POWER_DOWN_DISABLE`: Default 1 = Disable auto reduced power down.
pub const IFE_PESC_REDUCED_POWER_DOWN_DISABLE: u32 = 0x2000;
/// `IFE_PESC_100BTX_POWER_DOWN`: Indicates the power state of 100BASE-TX.
pub const IFE_PESC_100BTX_POWER_DOWN: u32 = 0x0400;
/// `IFE_PESC_10BTX_POWER_DOWN`: Indicates the power state of 10BASE-T.
pub const IFE_PESC_10BTX_POWER_DOWN: u32 = 0x0200;
/// `IFE_PESC_POLARITY_REVERSED`: Indicates 10BASE-T polarity.
pub const IFE_PESC_POLARITY_REVERSED: u32 = 0x0100;
/// `IFE_PESC_PHY_ADDR_MASK`: Bit 6:2 for sampled PHY address.
pub const IFE_PESC_PHY_ADDR_MASK: u32 = 0x007C;
/// `IFE_PESC_SPEED`: Auto-negotiation speed result 1=100Mbs, 0=10Mbs.
pub const IFE_PESC_SPEED: u32 = 0x0002;
/// `IFE_PESC_DUPLEX`: Auto-negotiation duplex result 1=Full, 0=Half.
pub const IFE_PESC_DUPLEX: u32 = 0x0001;
/// `IFE_PESC_POLARITY_REVERSED_SHIFT`.
pub const IFE_PESC_POLARITY_REVERSED_SHIFT: u32 = 8;
/// `IFE_PSC_DISABLE_DYNAMIC_POWER_DOWN`: 1 = Dynamic Power Down disabled.
pub const IFE_PSC_DISABLE_DYNAMIC_POWER_DOWN: u32 = 0x0100;
/// `IFE_PSC_FORCE_POLARITY`: 1=Reversed Polarity, 0=Normal.
pub const IFE_PSC_FORCE_POLARITY: u32 = 0x0020;
/// `IFE_PSC_AUTO_POLARITY_DISABLE`: 1=Auto Polarity Disabled, 0=Enabled.
pub const IFE_PSC_AUTO_POLARITY_DISABLE: u32 = 0x0010;
/// `IFE_PSC_JABBER_FUNC_DISABLE`: 1=Jabber Disabled, 0=Normal Jabber Operation.
pub const IFE_PSC_JABBER_FUNC_DISABLE: u32 = 0x0001;
/// `IFE_PSC_FORCE_POLARITY_SHIFT`.
pub const IFE_PSC_FORCE_POLARITY_SHIFT: u32 = 5;
/// `IFE_PSC_AUTO_POLARITY_DISABLE_SHIFT`.
pub const IFE_PSC_AUTO_POLARITY_DISABLE_SHIFT: u32 = 4;
/// `IFE_PMC_AUTO_MDIX`: 1=enable MDI/MDI-X feature, default 0=disabled.
pub const IFE_PMC_AUTO_MDIX: u16 = 0x0080;
/// `IFE_PMC_FORCE_MDIX`: 1=force MDIX-X, 0=force MDI.
pub const IFE_PMC_FORCE_MDIX: u16 = 0x0040;
/// `IFE_PMC_MDIX_STATUS`: 1=MDI-X, 0=MDI.
pub const IFE_PMC_MDIX_STATUS: u32 = 0x0020;
/// `IFE_PMC_AUTO_MDIX_COMPLETE`: Resolution algorithm is completed.
pub const IFE_PMC_AUTO_MDIX_COMPLETE: u32 = 0x0010;
/// `IFE_PMC_MDIX_MODE_SHIFT`.
pub const IFE_PMC_MDIX_MODE_SHIFT: u32 = 6;
/// `IFE_PHC_MDIX_RESET_ALL_MASK`: Disable auto MDI-X.
pub const IFE_PHC_MDIX_RESET_ALL_MASK: u32 = 0x0000;
/// `IFE_PHC_HWI_ENABLE`: Enable the HWI feature.
pub const IFE_PHC_HWI_ENABLE: u32 = 0x8000;
/// `IFE_PHC_ABILITY_CHECK`: 1= Test Passed, 0=failed.
pub const IFE_PHC_ABILITY_CHECK: u32 = 0x4000;
/// `IFE_PHC_TEST_EXEC`: PHY launch test pulses on the wire.
pub const IFE_PHC_TEST_EXEC: u32 = 0x2000;
/// `IFE_PHC_HIGHZ`: 1 = Open Circuit.
pub const IFE_PHC_HIGHZ: u32 = 0x0200;
/// `IFE_PHC_LOWZ`: 1 = Short Circuit.
pub const IFE_PHC_LOWZ: u32 = 0x0400;
/// `IFE_PHC_LOW_HIGH_Z_MASK`: Mask for indication type of problem on the line.
pub const IFE_PHC_LOW_HIGH_Z_MASK: u32 = 0x0600;
/// `IFE_PHC_DISTANCE_MASK`: Mask for distance to the cable problem, in 80cm granularity.
pub const IFE_PHC_DISTANCE_MASK: u32 = 0x01FF;
/// `IFE_PHC_RESET_ALL_MASK`: Disable HWI.
pub const IFE_PHC_RESET_ALL_MASK: u32 = 0x0000;
/// `IFE_PSCL_PROBE_MODE`: LED Probe mode.
pub const IFE_PSCL_PROBE_MODE: u32 = 0x0020;
/// `IFE_PSCL_PROBE_LEDS_OFF`: Force LEDs 0 and 2 off.
pub const IFE_PSCL_PROBE_LEDS_OFF: u32 = 0x0006;
/// `IFE_PSCL_PROBE_LEDS_ON`: Force LEDs 0 and 2 on.
pub const IFE_PSCL_PROBE_LEDS_ON: u32 = 0x0007;
/// `ICH_FLASH_COMMAND_TIMEOUT`: 5000 uSecs - adjusted.
pub const ICH_FLASH_COMMAND_TIMEOUT: u32 = 5000;
/// `ICH_FLASH_ERASE_TIMEOUT`: Up to 3 seconds - worst case.
pub const ICH_FLASH_ERASE_TIMEOUT: u32 = 3000000;
/// `ICH_FLASH_CYCLE_REPEAT_COUNT`: 10 cycles.
pub const ICH_FLASH_CYCLE_REPEAT_COUNT: u32 = 10;
/// `ICH_FLASH_SEG_SIZE_256`.
pub const ICH_FLASH_SEG_SIZE_256: u32 = 256;
/// `ICH_FLASH_SEG_SIZE_4K`.
pub const ICH_FLASH_SEG_SIZE_4K: u32 = 4096;
/// `ICH_FLASH_SEG_SIZE_8K`.
pub const ICH_FLASH_SEG_SIZE_8K: u32 = 8192;
/// `ICH_FLASH_SEG_SIZE_64K`.
pub const ICH_FLASH_SEG_SIZE_64K: u32 = 65536;
/// `ICH_CYCLE_READ`.
pub const ICH_CYCLE_READ: u32 = 0x0;
/// `ICH_CYCLE_RESERVED`.
pub const ICH_CYCLE_RESERVED: u32 = 0x1;
/// `ICH_CYCLE_WRITE`.
pub const ICH_CYCLE_WRITE: u32 = 0x2;
/// `ICH_CYCLE_ERASE`.
pub const ICH_CYCLE_ERASE: u32 = 0x3;
/// `ICH_FLASH_GFPREG`.
pub const ICH_FLASH_GFPREG: u32 = 0x0000;
/// `ICH_FLASH_HSFSTS`.
pub const ICH_FLASH_HSFSTS: u32 = 0x0004;
/// `ICH_FLASH_HSFCTL`.
pub const ICH_FLASH_HSFCTL: u32 = 0x0006;
/// `ICH_FLASH_FADDR`.
pub const ICH_FLASH_FADDR: u32 = 0x0008;
/// `ICH_FLASH_FDATA0`.
pub const ICH_FLASH_FDATA0: u32 = 0x0010;
/// `ICH_FLASH_FRACC`.
pub const ICH_FLASH_FRACC: u32 = 0x0050;
/// `ICH_FLASH_FREG0`.
pub const ICH_FLASH_FREG0: u32 = 0x0054;
/// `ICH_FLASH_FREG1`.
pub const ICH_FLASH_FREG1: u32 = 0x0058;
/// `ICH_FLASH_FREG2`.
pub const ICH_FLASH_FREG2: u32 = 0x005C;
/// `ICH_FLASH_FREG3`.
pub const ICH_FLASH_FREG3: u32 = 0x0060;
/// `ICH_FLASH_FPR0`.
pub const ICH_FLASH_FPR0: u32 = 0x0074;
/// `ICH_FLASH_FPR1`.
pub const ICH_FLASH_FPR1: u32 = 0x0078;
/// `ICH_FLASH_SSFSTS`.
pub const ICH_FLASH_SSFSTS: u32 = 0x0090;
/// `ICH_FLASH_SSFCTL`.
pub const ICH_FLASH_SSFCTL: u32 = 0x0092;
/// `ICH_FLASH_PREOP`.
pub const ICH_FLASH_PREOP: u32 = 0x0094;
/// `ICH_FLASH_OPTYPE`.
pub const ICH_FLASH_OPTYPE: u32 = 0x0096;
/// `ICH_FLASH_OPMENU`.
pub const ICH_FLASH_OPMENU: u32 = 0x0098;
/// `ICH_FLASH_REG_MAPSIZE`.
pub const ICH_FLASH_REG_MAPSIZE: u32 = 0x00A0;
/// `ICH_FLASH_SECTOR_SIZE`.
pub const ICH_FLASH_SECTOR_SIZE: u32 = 4096;
/// `ICH_GFPREG_BASE_MASK`.
pub const ICH_GFPREG_BASE_MASK: u32 = 0x1FFF;
/// `ICH_FLASH_LINEAR_ADDR_MASK`.
pub const ICH_FLASH_LINEAR_ADDR_MASK: u32 = 0x00FFFFFF;
/// `ICH_FLASH_SECT_ADDR_SHIFT`.
pub const ICH_FLASH_SECT_ADDR_SHIFT: u32 = 12;
/// `PHY_PREAMBLE`.
pub const PHY_PREAMBLE: u32 = 0xFFFFFFFF;
/// `PHY_SOF`.
pub const PHY_SOF: u32 = 0x01;
/// `PHY_OP_READ`.
pub const PHY_OP_READ: u32 = 0x02;
/// `PHY_OP_WRITE`.
pub const PHY_OP_WRITE: u32 = 0x01;
/// `PHY_TURNAROUND`.
pub const PHY_TURNAROUND: u32 = 0x02;
/// `PHY_PREAMBLE_SIZE`.
pub const PHY_PREAMBLE_SIZE: u32 = 32;
/// `MII_CR_SPEED_1000`.
pub const MII_CR_SPEED_1000: u16 = 0x0040;
/// `MII_CR_SPEED_100`.
pub const MII_CR_SPEED_100: u16 = 0x2000;
/// `MII_CR_SPEED_10`.
pub const MII_CR_SPEED_10: u16 = 0x0000;
/// `E1000_PHY_ADDRESS`.
pub const E1000_PHY_ADDRESS: u32 = 0x01;
/// `PHY_AUTO_NEG_TIME`: 4.5 Seconds.
pub const PHY_AUTO_NEG_TIME: u32 = 45;
/// `PHY_FORCE_TIME`: 2.0 Seconds.
pub const PHY_FORCE_TIME: u32 = 20;
/// `PHY_REVISION_MASK`.
pub const PHY_REVISION_MASK: u32 = 0xFFFFFFF0;
/// `DEVICE_SPEED_MASK`: Device Ctrl Reg Speed Mask.
pub const DEVICE_SPEED_MASK: u32 = 0x00000300;
/// `REG4_SPEED_MASK`.
pub const REG4_SPEED_MASK: u16 = 0x01E0;
/// `REG9_SPEED_MASK`.
pub const REG9_SPEED_MASK: u16 = 0x0300;
/// `ADVERTISE_10_HALF`.
pub const ADVERTISE_10_HALF: u16 = 0x0001;
/// `ADVERTISE_10_FULL`.
pub const ADVERTISE_10_FULL: u16 = 0x0002;
/// `ADVERTISE_100_HALF`.
pub const ADVERTISE_100_HALF: u16 = 0x0004;
/// `ADVERTISE_100_FULL`.
pub const ADVERTISE_100_FULL: u16 = 0x0008;
/// `ADVERTISE_1000_HALF`.
pub const ADVERTISE_1000_HALF: u32 = 0x0010;
/// `ADVERTISE_1000_FULL`.
pub const ADVERTISE_1000_FULL: u16 = 0x0020;
/// `AUTONEG_ADVERTISE_SPEED_DEFAULT`: Everything but 1000-Half.
pub const AUTONEG_ADVERTISE_SPEED_DEFAULT: u16 = 0x002F;
/// `AUTONEG_ADVERTISE_10_100_ALL`: All 10/100 speeds.
pub const AUTONEG_ADVERTISE_10_100_ALL: u16 = 0x000F;
/// `AUTONEG_ADVERTISE_10_ALL`: 10Mbps Full & Half speeds.
pub const AUTONEG_ADVERTISE_10_ALL: u16 = 0x0003;
/// `EEPROM_MGMT_CONTROL_ICP_xxxx(device_num)`: ICP word offset of the management control.
pub const fn eeprom_mgmt_control_icp_xxxx(device_num: u32) -> u16 {
    ((device_num + 1) << 4) as u16
}
/// `EEPROM_INIT_CONTROL3_ICP_xxxx(device_num)`: ICP word offset of init control 3.
pub const fn eeprom_init_control3_icp_xxxx(device_num: u32) -> u16 {
    (((device_num + 1) << 4) + 1) as u16
}
/// `EEPROM_IA_START_ICP_xxxx(device_num)`: ICP word offset of the Ethernet address.
pub const fn eeprom_ia_start_icp_xxxx(device_num: u32) -> u16 {
    (((device_num + 1) << 4) + 2) as u16
}
/// `EEPROM_IPV4_START_ICP_xxxx(device_num)`: ICP word offset of the IPv4 address.
pub const fn eeprom_ipv4_start_icp_xxxx(device_num: u32) -> u16 {
    (((device_num + 1) << 4) + 5) as u16
}
/// `EEPROM_IPV6_START_ICP_xxxx(device_num)`: ICP word offset of the IPv6 address.
pub const fn eeprom_ipv6_start_icp_xxxx(device_num: u32) -> u16 {
    (((device_num + 1) << 4) + 7) as u16
}
/// `EEPROM_CHECKSUM_REG_ICP_xxxx`.
#[allow(non_upper_case_globals)] // OpenBSD name, verbatim
pub const EEPROM_CHECKSUM_REG_ICP_xxxx: u16 = EEPROM_CHECKSUM_REG;
/// `PCI_CAP_ID_ST`.
pub const PCI_CAP_ID_ST: u32 = 0x09;
/// `PCI_ST_SMIA_OFFSET`.
pub const PCI_ST_SMIA_OFFSET: u32 = 0x04;
/// `E1000_IMC1`: Interrupt Mask Clear 1 - RW.
pub const E1000_IMC1: u32 = 0x008D8;
/// `E1000_IMC2`: Interrupt Mask Clear 2 - RW.
pub const E1000_IMC2: u32 = 0x008F8;
/// `E1000_82542_IMC1`.
pub const E1000_82542_IMC1: u32 = E1000_IMC1;
/// `E1000_82542_IMC2`.
pub const E1000_82542_IMC2: u32 = E1000_IMC2;
/// `E1000_NVM_K1_CONFIG`: NVM K1 Config Word.
pub const E1000_NVM_K1_CONFIG: u16 = 0x1B;
/// `E1000_NVM_K1_ENABLE`: NVM Enable K1 bit.
pub const E1000_NVM_K1_ENABLE: u16 = 0x1;
/// `E1000_KMRNCTRLSTA_OFFSET`.
pub const E1000_KMRNCTRLSTA_OFFSET: u32 = 0x001F0000;
/// `E1000_KMRNCTRLSTA_OFFSET_SHIFT`.
pub const E1000_KMRNCTRLSTA_OFFSET_SHIFT: u32 = 16;
/// `E1000_KMRNCTRLSTA_REN`.
pub const E1000_KMRNCTRLSTA_REN: u32 = 0x00200000;
/// `E1000_KMRNCTRLSTA_DIAG_OFFSET`: Diagnostic.
pub const E1000_KMRNCTRLSTA_DIAG_OFFSET: u32 = 0x3;
/// `E1000_KMRNCTRLSTA_TIMEOUTS`: Timeouts.
pub const E1000_KMRNCTRLSTA_TIMEOUTS: u32 = 0x4;
/// `E1000_KMRNCTRLSTA_INBAND_PARAM`: InBand Parameters.
pub const E1000_KMRNCTRLSTA_INBAND_PARAM: u32 = 0x9;
/// `E1000_KMRNCTRLSTA_DIAG_NELPBK`: Loopback mode.
pub const E1000_KMRNCTRLSTA_DIAG_NELPBK: u32 = 0x1000;
/// `E1000_KMRNCTRLSTA_K1_CONFIG`.
pub const E1000_KMRNCTRLSTA_K1_CONFIG: u32 = 0x7;
/// `E1000_KMRNCTRLSTA_K1_ENABLE`.
pub const E1000_KMRNCTRLSTA_K1_ENABLE: u16 = 0x0002;
// `E1000_EXTCNF_CTRL_MDIO_SW_OWNERSHIP` is defined again here, with the same value (0x00000020).
// `E1000_EXTCNF_CTRL_LCD_WRITE_ENABLE` is defined again here, with the same value (0x00000001).
/// `E1000_EXTCNF_CTRL_OEM_WRITE_ENABLE`.
pub const E1000_EXTCNF_CTRL_OEM_WRITE_ENABLE: u32 = 0x00000008;
// `E1000_EXTCNF_CTRL_SWFLAG` is defined again here, with the same value (0x00000020).
/// `E1000_EXTCNF_SIZE_EXT_PCIE_LENGTH_MASK`.
pub const E1000_EXTCNF_SIZE_EXT_PCIE_LENGTH_MASK: u32 = 0x00FF0000;
/// `E1000_EXTCNF_SIZE_EXT_PCIE_LENGTH_SHIFT`.
pub const E1000_EXTCNF_SIZE_EXT_PCIE_LENGTH_SHIFT: u32 = 16;
/// `E1000_EXTCNF_CTRL_EXT_CNF_POINTER_MASK`.
pub const E1000_EXTCNF_CTRL_EXT_CNF_POINTER_MASK: u32 = 0x0FFF0000;
/// `E1000_EXTCNF_CTRL_EXT_CNF_POINTER_SHIFT`.
pub const E1000_EXTCNF_CTRL_EXT_CNF_POINTER_SHIFT: u32 = 16;
/// `CV_SMB_CTRL`.
pub const CV_SMB_CTRL: u32 = phy_reg(769, 23);
/// `CV_SMB_CTRL_FORCE_SMBUS`.
pub const CV_SMB_CTRL_FORCE_SMBUS: u16 = 0x0001;
/// `I218_ULP_CONFIG1`.
pub const I218_ULP_CONFIG1: u32 = phy_reg(779, 16);
/// `I218_ULP_CONFIG1_START`: Start auto ULP config.
pub const I218_ULP_CONFIG1_START: u16 = 0x0001;
/// `I218_ULP_CONFIG1_IND`: Pwr up from ULP indication.
pub const I218_ULP_CONFIG1_IND: u16 = 0x0004;
/// `I218_ULP_CONFIG1_STICKY_ULP`: Set sticky ULP mode.
pub const I218_ULP_CONFIG1_STICKY_ULP: u16 = 0x0010;
/// `I218_ULP_CONFIG1_INBAND_EXIT`: Inband on ULP exit.
pub const I218_ULP_CONFIG1_INBAND_EXIT: u16 = 0x0020;
/// `I218_ULP_CONFIG1_WOL_HOST`: WoL Host on ULP exit.
pub const I218_ULP_CONFIG1_WOL_HOST: u16 = 0x0040;
/// `I218_ULP_CONFIG1_RESET_TO_SMBUS`: Reset to SMBus mode.
pub const I218_ULP_CONFIG1_RESET_TO_SMBUS: u16 = 0x0100;
/// `I218_ULP_CONFIG1_EN_ULP_LANPHYPC`.
pub const I218_ULP_CONFIG1_EN_ULP_LANPHYPC: u16 = 0x0400;
/// `I218_ULP_CONFIG1_DIS_CLR_STICKY_ON_PERST`.
pub const I218_ULP_CONFIG1_DIS_CLR_STICKY_ON_PERST: u16 = 0x0800;
/// `I218_ULP_CONFIG1_DISABLE_SMB_PERST`: Disable on PERST#.
pub const I218_ULP_CONFIG1_DISABLE_SMB_PERST: u16 = 0x1000;
/// `HV_INTC_FC_PAGE_START`.
pub const HV_INTC_FC_PAGE_START: u32 = 768;
/// `HV_SCC_UPPER`: Single Collision Count.
pub const HV_SCC_UPPER: u32 = phy_reg(778, 16);
/// `HV_SCC_LOWER`.
pub const HV_SCC_LOWER: u32 = phy_reg(778, 17);
/// `HV_ECOL_UPPER`: Excessive Collision Count.
pub const HV_ECOL_UPPER: u32 = phy_reg(778, 18);
/// `HV_ECOL_LOWER`.
pub const HV_ECOL_LOWER: u32 = phy_reg(778, 19);
/// `HV_MCC_UPPER`: Multiple Collision Count.
pub const HV_MCC_UPPER: u32 = phy_reg(778, 20);
/// `HV_MCC_LOWER`.
pub const HV_MCC_LOWER: u32 = phy_reg(778, 21);
/// `HV_LATECOL_UPPER`: Late Collision Count.
pub const HV_LATECOL_UPPER: u32 = phy_reg(778, 23);
/// `HV_LATECOL_LOWER`.
pub const HV_LATECOL_LOWER: u32 = phy_reg(778, 24);
/// `HV_COLC_UPPER`: Collision Count.
pub const HV_COLC_UPPER: u32 = phy_reg(778, 25);
/// `HV_COLC_LOWER`.
pub const HV_COLC_LOWER: u32 = phy_reg(778, 26);
/// `HV_DC_UPPER`: Defer Count.
pub const HV_DC_UPPER: u32 = phy_reg(778, 27);
/// `HV_DC_LOWER`.
pub const HV_DC_LOWER: u32 = phy_reg(778, 28);
/// `HV_TNCRS_UPPER`: Transmit with no CRS.
pub const HV_TNCRS_UPPER: u32 = phy_reg(778, 29);
/// `HV_TNCRS_LOWER`.
pub const HV_TNCRS_LOWER: u32 = phy_reg(778, 30);
/// `HV_OEM_BITS`.
pub const HV_OEM_BITS: u32 = phy_reg(768, 25);
/// `HV_OEM_BITS_LPLU`: Low Power Link Up.
pub const HV_OEM_BITS_LPLU: u16 = 0x0004;
/// `HV_OEM_BITS_GBE_DIS`: Gigabit Disable.
pub const HV_OEM_BITS_GBE_DIS: u16 = 0x0040;
/// `HV_OEM_BITS_RESTART_AN`: Restart Auto-negotiation.
pub const HV_OEM_BITS_RESTART_AN: u16 = 0x0400;
/// `HV_MUX_DATA_CTRL`.
pub const HV_MUX_DATA_CTRL: u32 = phy_reg(776, 16);
/// `HV_MUX_DATA_CTRL_GEN_TO_MAC`.
pub const HV_MUX_DATA_CTRL_GEN_TO_MAC: u16 = 0x0400;
/// `HV_MUX_DATA_CTRL_FORCE_SPEED`.
pub const HV_MUX_DATA_CTRL_FORCE_SPEED: u16 = 0x0004;
/// `HV_KMRN_MODE_CTRL`.
pub const HV_KMRN_MODE_CTRL: u32 = phy_reg(769, 16);
/// `HV_KMRN_MDIO_SLOW`.
pub const HV_KMRN_MDIO_SLOW: u16 = 0x0400;
/// `HV_PM_CTRL`.
pub const HV_PM_CTRL: u32 = phy_reg(770, 17);
/// `HV_PM_CTRL_K1_CLK_REQ`.
pub const HV_PM_CTRL_K1_CLK_REQ: u32 = 0x200;
/// `HV_PM_CTRL_K1_ENABLE`.
pub const HV_PM_CTRL_K1_ENABLE: u16 = 0x4000;
/// `I2_DFT_CTRL`.
pub const I2_DFT_CTRL: u32 = phy_reg(769, 20);
/// `I2_SMBUS_CTRL`.
pub const I2_SMBUS_CTRL: u32 = phy_reg(769, 23);
/// `I2_MODE_CTRL`.
pub const I2_MODE_CTRL: u32 = HV_KMRN_MODE_CTRL;
/// `I2_PCIE_POWER_CTRL`.
pub const I2_PCIE_POWER_CTRL: u32 = IGP3_KMRN_POWER_MNG_CTRL;
/// `E1000_FEXTNVM7`.
pub const E1000_FEXTNVM7: u32 = 0xe4;
/// `E1000_FEXTNVM7_SIDE_CLK_UNGATE`.
pub const E1000_FEXTNVM7_SIDE_CLK_UNGATE: u32 = 0x04;
/// `E1000_FEXTNVM7_DISABLE_SMB_PERST`.
pub const E1000_FEXTNVM7_DISABLE_SMB_PERST: u32 = 0x00000020;
/// `E1000_FEXTNVM9`.
pub const E1000_FEXTNVM9: u32 = 0x5bb4;
/// `E1000_FEXTNVM9_IOSFSB_CLKGATE_DIS`.
pub const E1000_FEXTNVM9_IOSFSB_CLKGATE_DIS: u32 = 0x0800;
/// `E1000_FEXTNVM9_IOSFSB_CLKREQ_DIS`.
pub const E1000_FEXTNVM9_IOSFSB_CLKREQ_DIS: u32 = 0x1000;
/// `E1000_FEXTNVM11`.
pub const E1000_FEXTNVM11: u32 = 0x05bbc;
/// `E1000_FEXTNVM11_DISABLE_MULR_FIX`.
pub const E1000_FEXTNVM11_DISABLE_MULR_FIX: u32 = 0x00002000;
// `BM_PORT_CTRL_PAGE` is defined again here, with the same value (769).
/// `BM_PCIE_PAGE`.
pub const BM_PCIE_PAGE: u32 = 770;
/// `BM_WUC_PAGE`.
pub const BM_WUC_PAGE: u32 = 800;
/// `BM_WUC_ADDRESS_OPCODE`.
pub const BM_WUC_ADDRESS_OPCODE: u32 = 0x11;
/// `BM_WUC_DATA_OPCODE`.
pub const BM_WUC_DATA_OPCODE: u32 = 0x12;
/// `BM_WUC_ENABLE_PAGE`.
pub const BM_WUC_ENABLE_PAGE: u32 = BM_PORT_CTRL_PAGE;
/// `BM_WUC_ENABLE_REG`.
pub const BM_WUC_ENABLE_REG: u32 = 17;
/// `BM_WUC_ENABLE_BIT`.
pub const BM_WUC_ENABLE_BIT: u16 = 1 << 2;
/// `BM_WUC_HOST_WU_BIT`.
pub const BM_WUC_HOST_WU_BIT: u16 = 1 << 4;
/// `BM_CS_STATUS`.
pub const BM_CS_STATUS: u32 = 17;
/// `BM_CS_STATUS_ENERGY_DETECT`: Energy Detect Status.
pub const BM_CS_STATUS_ENERGY_DETECT: u32 = 0x0010;
/// `BM_CS_STATUS_LINK_UP`.
pub const BM_CS_STATUS_LINK_UP: u16 = 0x0400;
/// `BM_CS_STATUS_RESOLVED`.
pub const BM_CS_STATUS_RESOLVED: u16 = 0x0800;
/// `BM_CS_STATUS_SPEED_MASK`.
pub const BM_CS_STATUS_SPEED_MASK: u16 = 0xC000;
/// `BM_CS_STATUS_SPEED_1000`.
pub const BM_CS_STATUS_SPEED_1000: u16 = 0x8000;
/// `HV_M_STATUS`.
pub const HV_M_STATUS: u32 = 26;
/// `HV_M_STATUS_AUTONEG_COMPLETE`.
pub const HV_M_STATUS_AUTONEG_COMPLETE: u16 = 0x1000;
/// `HV_M_STATUS_SPEED_MASK`.
pub const HV_M_STATUS_SPEED_MASK: u16 = 0x0300;
/// `HV_M_STATUS_SPEED_1000`.
pub const HV_M_STATUS_SPEED_1000: u16 = 0x0200;
/// `HV_M_STATUS_LINK_UP`.
pub const HV_M_STATUS_LINK_UP: u16 = 0x0040;
/// `I217_INBAND_CTRL`.
pub const I217_INBAND_CTRL: u32 = phy_reg(770, 18);
/// `I217_INBAND_CTRL_LINK_STAT_TX_TIMEOUT_MASK`.
pub const I217_INBAND_CTRL_LINK_STAT_TX_TIMEOUT_MASK: u16 = 0x3F00;
/// `I217_INBAND_CTRL_LINK_STAT_TX_TIMEOUT_SHIFT`.
pub const I217_INBAND_CTRL_LINK_STAT_TX_TIMEOUT_SHIFT: u32 = 8;
/// `E1000_PHY_TIMEOUTS_REG`.
pub const E1000_PHY_TIMEOUTS_REG: u32 = phy_reg(770, 21);
/// `E1000_PHY_TIMEOUTS_K1_EXIT_TO_MASK`.
pub const E1000_PHY_TIMEOUTS_K1_EXIT_TO_MASK: u16 = 0x0FC0;
/// `I82579_LPI_CTRL`.
pub const I82579_LPI_CTRL: u32 = phy_reg(772, 20);
/// `I82579_LPI_CTRL_ENABLE_MASK`.
pub const I82579_LPI_CTRL_ENABLE_MASK: u16 = 0x6000;
/// `I82579_LPI_CTRL_FORCE_PLL_LOCK_COUNT`.
pub const I82579_LPI_CTRL_FORCE_PLL_LOCK_COUNT: u32 = 0x80;
/// `I82579_EMI_ADDR`.
pub const I82579_EMI_ADDR: u32 = 0x10;
/// `I82579_EMI_DATA`.
pub const I82579_EMI_DATA: u32 = 0x11;
/// `I82579_LPI_UPDATE_TIMER`: in 40ns units + 40 ns base value.
pub const I82579_LPI_UPDATE_TIMER: u32 = 0x4805;
/// `I82579_MSE_THRESHOLD`: Mean Square Error Threshold.
pub const I82579_MSE_THRESHOLD: u16 = 0x084F;
/// `I82579_MSE_LINK_DOWN`: MSE count before dropping link.
pub const I82579_MSE_LINK_DOWN: u16 = 0x2411;
/// `E1000_INVM_DATA_REG(reg)`: i210 iNVM data register `reg`.
pub const fn e1000_invm_data_reg(reg: u32) -> u32 {
    0x12120 + 4 * reg
}
/// `INVM_SIZE`: Number of INVM Data Registers.
pub const INVM_SIZE: u32 = 64;
/// `NVM_INIT_CTRL_2_DEFAULT_I211`.
pub const NVM_INIT_CTRL_2_DEFAULT_I211: u16 = 0x7243;
/// `NVM_INIT_CTRL_4_DEFAULT_I211`.
pub const NVM_INIT_CTRL_4_DEFAULT_I211: u16 = 0x00C1;
/// `NVM_LED_1_CFG_DEFAULT_I211`.
pub const NVM_LED_1_CFG_DEFAULT_I211: u16 = 0x0184;
/// `NVM_LED_0_2_CFG_DEFAULT_I211`.
pub const NVM_LED_0_2_CFG_DEFAULT_I211: u16 = 0x200C;
/// `NVM_RESERVED_WORD`.
pub const NVM_RESERVED_WORD: u16 = 0xFFFF;
/// `INVM_DWORD_TO_RECORD_TYPE(dword)`.
pub const fn invm_dword_to_record_type(dword: u32) -> u32 {
    dword & 0x7
}
/// `INVM_DWORD_TO_WORD_ADDRESS(dword)`.
pub const fn invm_dword_to_word_address(dword: u32) -> u32 {
    (dword & 0x0000FE00) >> 9
}
/// `INVM_DWORD_TO_WORD_DATA(dword)`.
pub const fn invm_dword_to_word_data(dword: u32) -> u32 {
    (dword & 0xFFFF0000) >> 16
}
/// `INVM_UNINITIALIZED_STRUCTURE`.
pub const INVM_UNINITIALIZED_STRUCTURE: u32 = 0x0;
/// `INVM_WORD_AUTOLOAD_STRUCTURE`.
pub const INVM_WORD_AUTOLOAD_STRUCTURE: u32 = 0x1;
/// `INVM_CSR_AUTOLOAD_STRUCTURE`.
pub const INVM_CSR_AUTOLOAD_STRUCTURE: u32 = 0x2;
/// `INVM_PHY_REGISTER_AUTOLOAD_STRUCTURE`.
pub const INVM_PHY_REGISTER_AUTOLOAD_STRUCTURE: u32 = 0x3;
/// `INVM_RSA_KEY_SHA256_STRUCTURE`.
pub const INVM_RSA_KEY_SHA256_STRUCTURE: u32 = 0x4;
/// `INVM_INVALIDATED_STRUCTURE`.
pub const INVM_INVALIDATED_STRUCTURE: u32 = 0x5;
/// `INVM_RSA_KEY_SHA256_DATA_SIZE_IN_DWORDS`.
pub const INVM_RSA_KEY_SHA256_DATA_SIZE_IN_DWORDS: u32 = 8;
/// `INVM_CSR_AUTOLOAD_DATA_SIZE_IN_DWORDS`.
pub const INVM_CSR_AUTOLOAD_DATA_SIZE_IN_DWORDS: u32 = 1;
/// `PHY_UPPER_SHIFT`.
pub const PHY_UPPER_SHIFT: u32 = 21;
/// `BM_PHY_REG(page, reg)`: a BM PHY register address, page and register.
pub const fn bm_phy_reg(page: u32, reg: u32) -> u32 {
    (reg & MAX_PHY_REG_ADDRESS)
        | ((page & 0xFFFF) << PHY_PAGE_SHIFT)
        | ((reg & !MAX_PHY_REG_ADDRESS) << (PHY_UPPER_SHIFT - PHY_PAGE_SHIFT))
}
/// `BM_PHY_REG_PAGE(offset)`: the page of a BM PHY register address.
pub const fn bm_phy_reg_page(offset: u32) -> u16 {
    ((offset >> PHY_PAGE_SHIFT) & 0xFFFF) as u16
}
/// `BM_PHY_REG_NUM(offset)`: the register number of a BM PHY register address.
pub const fn bm_phy_reg_num(offset: u32) -> u16 {
    ((offset & MAX_PHY_REG_ADDRESS)
        | ((offset >> (PHY_UPPER_SHIFT - PHY_PAGE_SHIFT)) & !MAX_PHY_REG_ADDRESS)) as u16
}
/// `E1000_SFF_IDENTIFIER_OFFSET`.
pub const E1000_SFF_IDENTIFIER_OFFSET: u16 = 0x00;
/// `E1000_SFF_IDENTIFIER_SFF`.
pub const E1000_SFF_IDENTIFIER_SFF: u8 = 0x02;
/// `E1000_SFF_IDENTIFIER_SFP`.
pub const E1000_SFF_IDENTIFIER_SFP: u8 = 0x03;
/// `E1000_SFF_ETH_FLAGS_OFFSET`.
pub const E1000_SFF_ETH_FLAGS_OFFSET: u16 = 0x06;
/// `E1000_SFF_VENDOR_OUI_TYCO`.
pub const E1000_SFF_VENDOR_OUI_TYCO: u32 = 0x00407600;
/// `E1000_SFF_VENDOR_OUI_FTL`.
pub const E1000_SFF_VENDOR_OUI_FTL: u32 = 0x00906500;
/// `E1000_SFF_VENDOR_OUI_AVAGO`.
pub const E1000_SFF_VENDOR_OUI_AVAGO: u32 = 0x00176A00;
/// `E1000_SFF_VENDOR_OUI_INTEL`.
pub const E1000_SFF_VENDOR_OUI_INTEL: u32 = 0x001B2100;

/// `em_mac_type`: the media access controllers, in the order the shared code compares them
/// (`hw->mac_type >= em_82543`).
#[repr(u32)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
#[allow(non_camel_case_types)] // OpenBSD names, verbatim, for grep-ability
pub enum EmMacType {
    /// `em_undefined`.
    #[default]
    em_undefined = 0,
    /// `em_82542_rev2_0`.
    em_82542_rev2_0,
    /// `em_82542_rev2_1`.
    em_82542_rev2_1,
    /// `em_82543`.
    em_82543,
    /// `em_82544`.
    em_82544,
    /// `em_82540`.
    em_82540,
    /// `em_82545`.
    em_82545,
    /// `em_82545_rev_3`.
    em_82545_rev_3,
    /// `em_icp_xxxx`.
    em_icp_xxxx,
    /// `em_82546`.
    em_82546,
    /// `em_82546_rev_3`.
    em_82546_rev_3,
    /// `em_82541`.
    em_82541,
    /// `em_82541_rev_2`.
    em_82541_rev_2,
    /// `em_82547`.
    em_82547,
    /// `em_82547_rev_2`.
    em_82547_rev_2,
    /// `em_82571`.
    em_82571,
    /// `em_82572`.
    em_82572,
    /// `em_82573`.
    em_82573,
    /// `em_82574`.
    em_82574,
    /// `em_82575`.
    em_82575,
    /// `em_82576`.
    em_82576,
    /// `em_82580`.
    em_82580,
    /// `em_i350`.
    em_i350,
    /// `em_i210`.
    em_i210,
    /// `em_80003es2lan`.
    em_80003es2lan,
    /// `em_ich8lan`.
    em_ich8lan,
    /// `em_ich9lan`.
    em_ich9lan,
    /// `em_ich10lan`.
    em_ich10lan,
    /// `em_pchlan`.
    em_pchlan,
    /// `em_pch2lan`.
    em_pch2lan,
    /// `em_pch_lpt`.
    em_pch_lpt,
    /// `em_pch_spt`.
    em_pch_spt,
    /// `em_pch_cnp`.
    em_pch_cnp,
    /// `em_pch_tgp`.
    em_pch_tgp,
    /// `em_pch_adp`.
    em_pch_adp,
    /// `em_pch_mtp`.
    em_pch_mtp,
    /// `em_pch_ptp`.
    em_pch_ptp,
    /// `em_num_macs`.
    em_num_macs,
}

pub use EmMacType::*;

/// `IS_ICH8(t)`: the ICH8 and later integrated MACs.
pub const fn is_ich8(t: EmMacType) -> bool {
    t as u32 >= em_ich8lan as u32
}

/// `em_eeprom_type`.
#[repr(u32)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(non_camel_case_types)] // OpenBSD names, verbatim, for grep-ability
pub enum EmEepromType {
    /// `em_eeprom_uninitialized`.
    #[default]
    em_eeprom_uninitialized = 0,
    /// `em_eeprom_spi`.
    em_eeprom_spi,
    /// `em_eeprom_microwire`.
    em_eeprom_microwire,
    /// `em_eeprom_flash`.
    em_eeprom_flash,
    /// `em_eeprom_ich8`.
    em_eeprom_ich8,
    /// `em_eeprom_invm`.
    em_eeprom_invm,
    /// `em_eeprom_none`: no NVM support.
    em_eeprom_none,
    /// `em_num_eeprom_types`.
    em_num_eeprom_types,
}

pub use EmEepromType::*;

/// `em_media_type`: media types.
#[repr(u32)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(non_camel_case_types)] // OpenBSD names, verbatim, for grep-ability
pub enum EmMediaType {
    /// `em_media_type_copper`.
    #[default]
    em_media_type_copper = 0,
    /// `em_media_type_fiber`.
    em_media_type_fiber = 1,
    /// `em_media_type_internal_serdes`.
    em_media_type_internal_serdes = 2,
    /// `em_media_type_oem`.
    em_media_type_oem = 3,
    /// `em_num_media_types`.
    em_num_media_types,
}

pub use EmMediaType::*;

/// `em_speed_duplex_type`: the forced speed and duplex (`hw->forced_speed_duplex`, a
/// `uint8_t` in `struct em_hw`).
#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(non_camel_case_types)] // OpenBSD names, verbatim, for grep-ability
pub enum EmSpeedDuplexType {
    /// `em_10_half`.
    #[default]
    em_10_half = 0,
    /// `em_10_full`.
    em_10_full = 1,
    /// `em_100_half`.
    em_100_half = 2,
    /// `em_100_full`.
    em_100_full = 3,
}

pub use EmSpeedDuplexType::*;

/// `struct em_shadow_ram`: one word of the ICH8 NVM shadow RAM.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EmShadowRam {
    /// `eeprom_word`.
    pub eeprom_word: u16,
    /// `modified`.
    pub modified: bool,
}

/// `em_bus_type`: PCI bus types.
#[repr(u32)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(non_camel_case_types)] // OpenBSD names, verbatim, for grep-ability
pub enum EmBusType {
    /// `em_bus_type_unknown`.
    #[default]
    em_bus_type_unknown = 0,
    /// `em_bus_type_pci`.
    em_bus_type_pci,
    /// `em_bus_type_pcix`.
    em_bus_type_pcix,
    /// `em_bus_type_pci_express`.
    em_bus_type_pci_express,
    /// `em_bus_type_cpp`.
    em_bus_type_cpp,
    /// `em_bus_type_reserved`.
    em_bus_type_reserved,
}

pub use EmBusType::*;

/// `em_bus_speed`: PCI bus speeds.
#[repr(u32)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(non_camel_case_types)] // OpenBSD names, verbatim, for grep-ability
pub enum EmBusSpeed {
    /// `em_bus_speed_unknown`.
    #[default]
    em_bus_speed_unknown = 0,
    /// `em_bus_speed_33`.
    em_bus_speed_33,
    /// `em_bus_speed_66`.
    em_bus_speed_66,
    /// `em_bus_speed_100`.
    em_bus_speed_100,
    /// `em_bus_speed_120`.
    em_bus_speed_120,
    /// `em_bus_speed_133`.
    em_bus_speed_133,
    /// `em_bus_speed_2500`.
    em_bus_speed_2500,
    /// `em_bus_speed_reserved`.
    em_bus_speed_reserved,
}

pub use EmBusSpeed::*;

/// `em_bus_width`: PCI bus widths.
#[repr(u32)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(non_camel_case_types)] // OpenBSD names, verbatim, for grep-ability
pub enum EmBusWidth {
    /// `em_bus_width_unknown`.
    #[default]
    em_bus_width_unknown = 0,
    /// `em_bus_width_pciex_1`: these PCIe values should literally match the possible return
    /// values from config space.
    em_bus_width_pciex_1 = 1,
    /// `em_bus_width_pciex_2`.
    em_bus_width_pciex_2 = 2,
    /// `em_bus_width_pciex_4`.
    em_bus_width_pciex_4 = 4,
    /// `em_bus_width_32`.
    em_bus_width_32,
    /// `em_bus_width_64`.
    em_bus_width_64,
    /// `em_bus_width_reserved`.
    em_bus_width_reserved,
}

impl EmBusWidth {
    /// The width of a PCIe link status's width field, which the C stores into `bus_width`
    /// as is; a width the enumeration has no member for is `em_bus_width_reserved`.
    pub fn from_pcie_link_width(w: u16) -> Self {
        match w {
            0 => em_bus_width_unknown,
            1 => em_bus_width_pciex_1,
            2 => em_bus_width_pciex_2,
            4 => em_bus_width_pciex_4,
            _ => em_bus_width_reserved,
        }
    }
}

pub use EmBusWidth::*;

/// `em_cable_length`: the M88 PHY's cable length field, compared with values read from the
/// PHY (an integer type with its values as constants).
pub type EmCableLength = u16;
/// `em_cable_length_50`.
#[allow(non_upper_case_globals)] // OpenBSD name, verbatim
pub const em_cable_length_50: EmCableLength = 0;
/// `em_cable_length_50_80`.
#[allow(non_upper_case_globals)] // OpenBSD name, verbatim
pub const em_cable_length_50_80: EmCableLength = 1;
/// `em_cable_length_80_110`.
#[allow(non_upper_case_globals)] // OpenBSD name, verbatim
pub const em_cable_length_80_110: EmCableLength = 2;
/// `em_cable_length_110_140`.
#[allow(non_upper_case_globals)] // OpenBSD name, verbatim
pub const em_cable_length_110_140: EmCableLength = 3;
/// `em_cable_length_140`.
#[allow(non_upper_case_globals)] // OpenBSD name, verbatim
pub const em_cable_length_140: EmCableLength = 4;
/// `em_cable_length_undefined`.
#[allow(non_upper_case_globals)] // OpenBSD name, verbatim
pub const em_cable_length_undefined: EmCableLength = 0xFF;

/// `em_gg_cable_length`: the GG82563 PHY's cable length field (an integer type with its
/// values as constants).
pub type EmGgCableLength = u16;
/// `em_gg_cable_length_60`.
#[allow(non_upper_case_globals)] // OpenBSD name, verbatim
pub const em_gg_cable_length_60: EmGgCableLength = 0;
/// `em_gg_cable_length_60_115`.
#[allow(non_upper_case_globals)] // OpenBSD name, verbatim
pub const em_gg_cable_length_60_115: EmGgCableLength = 1;
/// `em_gg_cable_length_115_150`.
#[allow(non_upper_case_globals)] // OpenBSD name, verbatim
pub const em_gg_cable_length_115_150: EmGgCableLength = 2;
/// `em_gg_cable_length_150`.
#[allow(non_upper_case_globals)] // OpenBSD name, verbatim
pub const em_gg_cable_length_150: EmGgCableLength = 4;

/// `em_igp_cable_length`: cable lengths in metres, used as numbers (an integer type with its
/// values as constants).
pub type EmIgpCableLength = u16;
/// `em_igp_cable_length_10`.
#[allow(non_upper_case_globals)] // OpenBSD name, verbatim
pub const em_igp_cable_length_10: EmIgpCableLength = 10;
/// `em_igp_cable_length_20`.
#[allow(non_upper_case_globals)] // OpenBSD name, verbatim
pub const em_igp_cable_length_20: EmIgpCableLength = 20;
/// `em_igp_cable_length_30`.
#[allow(non_upper_case_globals)] // OpenBSD name, verbatim
pub const em_igp_cable_length_30: EmIgpCableLength = 30;
/// `em_igp_cable_length_40`.
#[allow(non_upper_case_globals)] // OpenBSD name, verbatim
pub const em_igp_cable_length_40: EmIgpCableLength = 40;
/// `em_igp_cable_length_50`.
#[allow(non_upper_case_globals)] // OpenBSD name, verbatim
pub const em_igp_cable_length_50: EmIgpCableLength = 50;
/// `em_igp_cable_length_60`.
#[allow(non_upper_case_globals)] // OpenBSD name, verbatim
pub const em_igp_cable_length_60: EmIgpCableLength = 60;
/// `em_igp_cable_length_70`.
#[allow(non_upper_case_globals)] // OpenBSD name, verbatim
pub const em_igp_cable_length_70: EmIgpCableLength = 70;
/// `em_igp_cable_length_80`.
#[allow(non_upper_case_globals)] // OpenBSD name, verbatim
pub const em_igp_cable_length_80: EmIgpCableLength = 80;
/// `em_igp_cable_length_90`.
#[allow(non_upper_case_globals)] // OpenBSD name, verbatim
pub const em_igp_cable_length_90: EmIgpCableLength = 90;
/// `em_igp_cable_length_100`.
#[allow(non_upper_case_globals)] // OpenBSD name, verbatim
pub const em_igp_cable_length_100: EmIgpCableLength = 100;
/// `em_igp_cable_length_110`.
#[allow(non_upper_case_globals)] // OpenBSD name, verbatim
pub const em_igp_cable_length_110: EmIgpCableLength = 110;
/// `em_igp_cable_length_115`.
#[allow(non_upper_case_globals)] // OpenBSD name, verbatim
pub const em_igp_cable_length_115: EmIgpCableLength = 115;
/// `em_igp_cable_length_120`.
#[allow(non_upper_case_globals)] // OpenBSD name, verbatim
pub const em_igp_cable_length_120: EmIgpCableLength = 120;
/// `em_igp_cable_length_130`.
#[allow(non_upper_case_globals)] // OpenBSD name, verbatim
pub const em_igp_cable_length_130: EmIgpCableLength = 130;
/// `em_igp_cable_length_140`.
#[allow(non_upper_case_globals)] // OpenBSD name, verbatim
pub const em_igp_cable_length_140: EmIgpCableLength = 140;
/// `em_igp_cable_length_150`.
#[allow(non_upper_case_globals)] // OpenBSD name, verbatim
pub const em_igp_cable_length_150: EmIgpCableLength = 150;
/// `em_igp_cable_length_160`.
#[allow(non_upper_case_globals)] // OpenBSD name, verbatim
pub const em_igp_cable_length_160: EmIgpCableLength = 160;
/// `em_igp_cable_length_170`.
#[allow(non_upper_case_globals)] // OpenBSD name, verbatim
pub const em_igp_cable_length_170: EmIgpCableLength = 170;
/// `em_igp_cable_length_180`.
#[allow(non_upper_case_globals)] // OpenBSD name, verbatim
pub const em_igp_cable_length_180: EmIgpCableLength = 180;

/// `em_10bt_ext_dist_enable`.
#[repr(u32)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(non_camel_case_types)] // OpenBSD names, verbatim, for grep-ability
pub enum Em10btExtDistEnable {
    /// `em_10bt_ext_dist_enable_normal`.
    #[default]
    em_10bt_ext_dist_enable_normal = 0,
    /// `em_10bt_ext_dist_enable_lower`.
    em_10bt_ext_dist_enable_lower,
    /// `em_10bt_ext_dist_enable_undefined`.
    em_10bt_ext_dist_enable_undefined = 0xFF,
}

pub use Em10btExtDistEnable::*;

/// `em_rev_polarity`.
#[repr(u32)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(non_camel_case_types)] // OpenBSD names, verbatim, for grep-ability
pub enum EmRevPolarity {
    /// `em_rev_polarity_normal`.
    #[default]
    em_rev_polarity_normal = 0,
    /// `em_rev_polarity_reversed`.
    em_rev_polarity_reversed,
    /// `em_rev_polarity_undefined`.
    em_rev_polarity_undefined = 0xFF,
}

pub use EmRevPolarity::*;

/// `em_downshift`.
#[repr(u32)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(non_camel_case_types)] // OpenBSD names, verbatim, for grep-ability
pub enum EmDownshift {
    /// `em_downshift_normal`.
    #[default]
    em_downshift_normal = 0,
    /// `em_downshift_activated`.
    em_downshift_activated,
    /// `em_downshift_undefined`.
    em_downshift_undefined = 0xFF,
}

pub use EmDownshift::*;

/// `em_smart_speed`.
#[repr(u32)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(non_camel_case_types)] // OpenBSD names, verbatim, for grep-ability
pub enum EmSmartSpeed {
    /// `em_smart_speed_default`.
    #[default]
    em_smart_speed_default = 0,
    /// `em_smart_speed_on`.
    em_smart_speed_on,
    /// `em_smart_speed_off`.
    em_smart_speed_off,
}

pub use EmSmartSpeed::*;

/// `em_polarity_reversal`.
#[repr(u32)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(non_camel_case_types)] // OpenBSD names, verbatim, for grep-ability
pub enum EmPolarityReversal {
    /// `em_polarity_reversal_enabled`.
    #[default]
    em_polarity_reversal_enabled = 0,
    /// `em_polarity_reversal_disabled`.
    em_polarity_reversal_disabled,
    /// `em_polarity_reversal_undefined`.
    em_polarity_reversal_undefined = 0xFF,
}

pub use EmPolarityReversal::*;

/// `em_auto_x_mode`.
#[repr(u32)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(non_camel_case_types)] // OpenBSD names, verbatim, for grep-ability
pub enum EmAutoXMode {
    /// `em_auto_x_mode_manual_mdi`.
    #[default]
    em_auto_x_mode_manual_mdi = 0,
    /// `em_auto_x_mode_manual_mdix`.
    em_auto_x_mode_manual_mdix,
    /// `em_auto_x_mode_auto1`.
    em_auto_x_mode_auto1,
    /// `em_auto_x_mode_auto2`.
    em_auto_x_mode_auto2,
    /// `em_auto_x_mode_undefined`.
    em_auto_x_mode_undefined = 0xFF,
}

pub use EmAutoXMode::*;

/// `em_1000t_rx_status`.
#[repr(u32)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(non_camel_case_types)] // OpenBSD names, verbatim, for grep-ability
pub enum Em1000tRxStatus {
    /// `em_1000t_rx_status_not_ok`.
    #[default]
    em_1000t_rx_status_not_ok = 0,
    /// `em_1000t_rx_status_ok`.
    em_1000t_rx_status_ok,
    /// `em_1000t_rx_status_undefined`.
    em_1000t_rx_status_undefined = 0xFF,
}

pub use Em1000tRxStatus::*;

/// `em_phy_type`.
#[repr(u32)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(non_camel_case_types)] // OpenBSD names, verbatim, for grep-ability
pub enum EmPhyType {
    /// `em_phy_m88` (also what a zeroed `struct em_hw` holds, as in C).
    #[default]
    em_phy_m88 = 0,
    /// `em_phy_igp`.
    em_phy_igp,
    /// `em_phy_igp_2`.
    em_phy_igp_2,
    /// `em_phy_gg82563`.
    em_phy_gg82563,
    /// `em_phy_igp_3`.
    em_phy_igp_3,
    /// `em_phy_ife`.
    em_phy_ife,
    /// `em_phy_bm`: phy used in i82574L, ICH10 and some ICH9.
    em_phy_bm,
    /// `em_phy_oem`.
    em_phy_oem,
    /// `em_phy_82577`.
    em_phy_82577,
    /// `em_phy_82578`.
    em_phy_82578,
    /// `em_phy_82579`.
    em_phy_82579,
    /// `em_phy_i217`.
    em_phy_i217,
    /// `em_phy_82580`.
    em_phy_82580,
    /// `em_phy_rtl8211`.
    em_phy_rtl8211,
    /// `em_phy_undefined`.
    em_phy_undefined = 0xFF,
}

pub use EmPhyType::*;

/// `em_ms_type`: master/slave setting of a 1000BASE-T PHY.
#[repr(u32)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(non_camel_case_types)] // OpenBSD names, verbatim, for grep-ability
pub enum EmMsType {
    /// `em_ms_hw_default`.
    #[default]
    em_ms_hw_default = 0,
    /// `em_ms_force_master`.
    em_ms_force_master,
    /// `em_ms_force_slave`.
    em_ms_force_slave,
    /// `em_ms_auto`.
    em_ms_auto,
}

pub use EmMsType::*;

/// `em_ffe_config`.
#[repr(u32)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(non_camel_case_types)] // OpenBSD names, verbatim, for grep-ability
pub enum EmFfeConfig {
    /// `em_ffe_config_enabled`.
    #[default]
    em_ffe_config_enabled = 0,
    /// `em_ffe_config_active`.
    em_ffe_config_active,
    /// `em_ffe_config_blocked`.
    em_ffe_config_blocked,
}

pub use EmFfeConfig::*;

/// `em_dsp_config`.
#[repr(u32)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(non_camel_case_types)] // OpenBSD names, verbatim, for grep-ability
pub enum EmDspConfig {
    /// `em_dsp_config_disabled`.
    #[default]
    em_dsp_config_disabled = 0,
    /// `em_dsp_config_enabled`.
    em_dsp_config_enabled,
    /// `em_dsp_config_activated`.
    em_dsp_config_activated,
    /// `em_dsp_config_undefined`.
    em_dsp_config_undefined = 0xFF,
}

pub use EmDspConfig::*;

/// `struct em_phy_info`: PHY status info.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EmPhyInfo {
    /// `cable_length`.
    pub cable_length: EmCableLength,
    /// `extended_10bt_distance`.
    pub extended_10bt_distance: Em10btExtDistEnable,
    /// `cable_polarity`.
    pub cable_polarity: EmRevPolarity,
    /// `downshift`.
    pub downshift: EmDownshift,
    /// `polarity_correction`.
    pub polarity_correction: EmPolarityReversal,
    /// `mdix_mode`.
    pub mdix_mode: EmAutoXMode,
    /// `local_rx`.
    pub local_rx: Em1000tRxStatus,
    /// `remote_rx`.
    pub remote_rx: Em1000tRxStatus,
}

/// `struct em_phy_stats`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EmPhyStats {
    /// `idle_errors`.
    pub idle_errors: u32,
    /// `receive_errors`.
    pub receive_errors: u32,
}

/// `struct em_eeprom_info`: the NVM's type and geometry.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EmEepromInfo {
    /// `type`.
    pub r#type: EmEepromType,
    /// `word_size`.
    pub word_size: u16,
    /// `opcode_bits`.
    pub opcode_bits: u16,
    /// `address_bits`.
    pub address_bits: u16,
    /// `delay_usec`.
    pub delay_usec: u16,
    /// `page_size`.
    pub page_size: u16,
    /// `use_eerd`.
    pub use_eerd: bool,
    /// `use_eewr`.
    pub use_eewr: bool,
}

/// `em_align_type`.
#[repr(u32)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(non_camel_case_types)] // OpenBSD names, verbatim, for grep-ability
pub enum EmAlignType {
    /// `em_byte_align`.
    #[default]
    em_byte_align = 0,
    /// `em_word_align`.
    em_word_align = 1,
    /// `em_dword_align`.
    em_dword_align = 2,
}

pub use EmAlignType::*;

/// `struct em_host_mng_command_header`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EmHostMngCommandHeader {
    /// `command_id`.
    pub command_id: u8,
    /// `checksum`.
    pub checksum: u8,
    /// `reserved1`.
    pub reserved1: u16,
    /// `reserved2`.
    pub reserved2: u16,
    /// `command_length`.
    pub command_length: u16,
}

/// `struct em_host_mng_command_info`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EmHostMngCommandInfo {
    /// `command_header`: command head/command result head has 4 bytes.
    pub command_header: EmHostMngCommandHeader,
    /// `command_data`: command data can length 0..0x658.
    pub command_data: [u8; E1000_HI_MAX_MNG_DATA_LENGTH],
}

/// `struct em_host_mng_dhcp_cookie`: the DHCP cookie the firmware keeps in the host interface
/// memory (`E1000_MNG_DHCP_COOKIE_LENGTH` bytes, little-endian).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EmHostMngDhcpCookie {
    /// `signature`.
    pub signature: u32,
    /// `status`.
    pub status: u8,
    /// `reserved0`.
    pub reserved0: u8,
    /// `vlan_id`.
    pub vlan_id: u16,
    /// `reserved1`.
    pub reserved1: u32,
    /// `reserved2`.
    pub reserved2: u16,
    /// `reserved3`.
    pub reserved3: u8,
    /// `checksum`.
    pub checksum: u8,
}

impl EmHostMngDhcpCookie {
    /// The cookie's bytes, as the C's `(uint8_t *)&hw->mng_cookie` sees them.
    pub fn to_bytes(&self) -> [u8; 16] {
        let mut b = [0u8; 16];
        b[0..4].copy_from_slice(&self.signature.to_le_bytes());
        b[4] = self.status;
        b[5] = self.reserved0;
        b[6..8].copy_from_slice(&self.vlan_id.to_le_bytes());
        b[8..12].copy_from_slice(&self.reserved1.to_le_bytes());
        b[12..14].copy_from_slice(&self.reserved2.to_le_bytes());
        b[14] = self.reserved3;
        b[15] = self.checksum;
        b
    }

    /// The cookie whose bytes are `b`, as the C's stores through `(uint8_t *)&hw->mng_cookie`
    /// leave it.
    pub fn from_bytes(b: &[u8; 16]) -> Self {
        Self {
            signature: u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
            status: b[4],
            reserved0: b[5],
            vlan_id: u16::from_le_bytes([b[6], b[7]]),
            reserved1: u32::from_le_bytes([b[8], b[9], b[10], b[11]]),
            reserved2: u16::from_le_bytes([b[12], b[13]]),
            reserved3: b[14],
            checksum: b[15],
        }
    }
}

/// `struct em_rx_desc`: receive descriptor (legacy).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EmRxDesc {
    /// `buffer_addr`: address of the descriptor's data buffer.
    pub buffer_addr: u64,
    /// `length`: length of data DMAed into data buffer.
    pub length: u16,
    /// `csum`: packet checksum.
    pub csum: u16,
    /// `status`: descriptor status.
    pub status: u8,
    /// `errors`: descriptor errors.
    pub errors: u8,
    /// `special`.
    pub special: u16,
}

/// The `read` member of [`EmRxDescExtended`].
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EmRxDescExtendedRead {
    /// `buffer_addr`.
    pub buffer_addr: u64,
    /// `reserved`.
    pub reserved: u64,
}

/// The `csum_ip` member of [`EmRxDescHiDword`].
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EmRxDescCsumIp {
    /// `ip_id`: IP id.
    pub ip_id: u16,
    /// `csum`: packet checksum.
    pub csum: u16,
}

/// The `hi_dword` union of the write-back descriptors: the RSS hash or the IP id and checksum.
#[repr(C)]
#[derive(Clone, Copy)]
pub union EmRxDescHiDword {
    /// `rss`: RSS hash.
    pub rss: u32,
    /// `csum_ip`.
    pub csum_ip: EmRxDescCsumIp,
}

impl EmRxDescHiDword {
    /// `hi_dword.rss`.
    pub fn rss(&self) -> u32 {
        // SAFETY: both members are plain integers covering the same four bytes; every bit
        // pattern is a valid value of either.
        unsafe { self.rss }
    }

    /// `hi_dword.csum_ip`.
    pub fn csum_ip(&self) -> EmRxDescCsumIp {
        // SAFETY: as in `rss`: plain integers, every bit pattern valid.
        unsafe { self.csum_ip }
    }
}

/// The `wb.lower` member of the extended and packet-split descriptors.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct EmRxDescWbLower {
    /// `mrq`: multiple Rx queues.
    pub mrq: u32,
    /// `hi_dword`.
    pub hi_dword: EmRxDescHiDword,
}

/// The `wb.upper` member of [`EmRxDescExtended`].
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EmRxDescExtendedWbUpper {
    /// `status_error`: ext status/error.
    pub status_error: u32,
    /// `length`.
    pub length: u16,
    /// `vlan`: VLAN tag.
    pub vlan: u16,
}

/// The `wb` (writeback) member of [`EmRxDescExtended`].
#[repr(C)]
#[derive(Clone, Copy)]
pub struct EmRxDescExtendedWb {
    /// `lower`.
    pub lower: EmRxDescWbLower,
    /// `upper`.
    pub upper: EmRxDescExtendedWbUpper,
}

/// `union em_rx_desc_extended`: receive descriptor, extended.
#[repr(C)]
#[derive(Clone, Copy)]
pub union EmRxDescExtended {
    /// `read`.
    pub read: EmRxDescExtendedRead,
    /// `wb`: writeback.
    pub wb: EmRxDescExtendedWb,
}

impl EmRxDescExtended {
    /// The `read` view.
    pub fn read(&self) -> EmRxDescExtendedRead {
        // SAFETY: both members are `repr(C)` structures of plain integers over the same 16
        // bytes; every bit pattern is a valid value of either.
        unsafe { self.read }
    }

    /// The `wb` view.
    pub fn wb(&self) -> EmRxDescExtendedWb {
        // SAFETY: as in `read`.
        unsafe { self.wb }
    }
}

/// The `read` member of [`EmRxDescPacketSplit`]: one buffer for protocol header(s), three
/// data buffers.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EmRxDescPacketSplitRead {
    /// `buffer_addr`.
    pub buffer_addr: [u64; MAX_PS_BUFFERS],
}

/// The `wb.middle` member of [`EmRxDescPacketSplit`].
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EmRxDescPacketSplitWbMiddle {
    /// `status_error`: ext status/error.
    pub status_error: u32,
    /// `length0`: length of buffer 0.
    pub length0: u16,
    /// `vlan`: VLAN tag.
    pub vlan: u16,
}

/// The `wb.upper` member of [`EmRxDescPacketSplit`].
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EmRxDescPacketSplitWbUpper {
    /// `header_status`.
    pub header_status: u16,
    /// `length`: length of buffers 1-3.
    pub length: [u16; 3],
}

/// The `wb` (writeback) member of [`EmRxDescPacketSplit`].
#[repr(C)]
#[derive(Clone, Copy)]
pub struct EmRxDescPacketSplitWb {
    /// `lower`.
    pub lower: EmRxDescWbLower,
    /// `middle`.
    pub middle: EmRxDescPacketSplitWbMiddle,
    /// `upper`.
    pub upper: EmRxDescPacketSplitWbUpper,
    /// `reserved`.
    pub reserved: u64,
}

/// `union em_rx_desc_packet_split`: receive descriptor, packet split.
#[repr(C)]
#[derive(Clone, Copy)]
pub union EmRxDescPacketSplit {
    /// `read`.
    pub read: EmRxDescPacketSplitRead,
    /// `wb`: writeback.
    pub wb: EmRxDescPacketSplitWb,
}

impl EmRxDescPacketSplit {
    /// The `read` view.
    pub fn read(&self) -> EmRxDescPacketSplitRead {
        // SAFETY: both members are `repr(C)` structures of plain integers over the same 32
        // bytes; every bit pattern is a valid value of either.
        unsafe { self.read }
    }

    /// The `wb` view.
    pub fn wb(&self) -> EmRxDescPacketSplitWb {
        // SAFETY: as in `read`.
        unsafe { self.wb }
    }
}

/// The `lower.flags` member of [`EmTxDesc`].
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EmTxDescLowerFlags {
    /// `length`: data buffer length.
    pub length: u16,
    /// `cso`: checksum offset.
    pub cso: u8,
    /// `cmd`: descriptor control.
    pub cmd: u8,
}

/// The `lower` union of [`EmTxDesc`].
#[repr(C)]
#[derive(Clone, Copy)]
pub union EmTxDescLower {
    /// `data`.
    pub data: u32,
    /// `flags`.
    pub flags: EmTxDescLowerFlags,
}

impl EmTxDescLower {
    /// `lower.data`.
    pub fn data(&self) -> u32 {
        // SAFETY: both members are plain integers over the same four bytes; every bit pattern
        // is a valid value of either.
        unsafe { self.data }
    }

    /// `lower.flags`.
    pub fn flags(&self) -> EmTxDescLowerFlags {
        // SAFETY: as in `data`.
        unsafe { self.flags }
    }
}

/// The `upper.fields` member of [`EmTxDesc`].
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EmTxDescUpperFields {
    /// `status`: descriptor status.
    pub status: u8,
    /// `css`: checksum start.
    pub css: u8,
    /// `special`.
    pub special: u16,
}

/// The `upper` union of [`EmTxDesc`].
#[repr(C)]
#[derive(Clone, Copy)]
pub union EmTxDescUpper {
    /// `data`.
    pub data: u32,
    /// `fields`.
    pub fields: EmTxDescUpperFields,
}

impl EmTxDescUpper {
    /// `upper.data`.
    pub fn data(&self) -> u32 {
        // SAFETY: both members are plain integers over the same four bytes; every bit pattern
        // is a valid value of either.
        unsafe { self.data }
    }

    /// `upper.fields`.
    pub fn fields(&self) -> EmTxDescUpperFields {
        // SAFETY: as in `data`.
        unsafe { self.fields }
    }
}

/// `struct em_tx_desc`: transmit descriptor.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct EmTxDesc {
    /// `buffer_addr`: address of the descriptor's data buffer.
    pub buffer_addr: u64,
    /// `lower`.
    pub lower: EmTxDescLower,
    /// `upper`.
    pub upper: EmTxDescUpper,
}

/// The `lower_setup.ip_fields` member of [`EmContextDesc`].
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EmContextDescIpFields {
    /// `ipcss`: IP checksum start.
    pub ipcss: u8,
    /// `ipcso`: IP checksum offset.
    pub ipcso: u8,
    /// `ipcse`: IP checksum end.
    pub ipcse: u16,
}

/// The `lower_setup` union of [`EmContextDesc`].
#[repr(C)]
#[derive(Clone, Copy)]
pub union EmContextDescLowerSetup {
    /// `ip_config`.
    pub ip_config: u32,
    /// `ip_fields`.
    pub ip_fields: EmContextDescIpFields,
}

impl EmContextDescLowerSetup {
    /// `lower_setup.ip_config`.
    pub fn ip_config(&self) -> u32 {
        // SAFETY: both members are plain integers over the same four bytes; every bit pattern
        // is a valid value of either.
        unsafe { self.ip_config }
    }

    /// `lower_setup.ip_fields`.
    pub fn ip_fields(&self) -> EmContextDescIpFields {
        // SAFETY: as in `ip_config`.
        unsafe { self.ip_fields }
    }
}

/// The `upper_setup.tcp_fields` member of [`EmContextDesc`].
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EmContextDescTcpFields {
    /// `tucss`: TCP checksum start.
    pub tucss: u8,
    /// `tucso`: TCP checksum offset.
    pub tucso: u8,
    /// `tucse`: TCP checksum end.
    pub tucse: u16,
}

/// The `upper_setup` union of [`EmContextDesc`].
#[repr(C)]
#[derive(Clone, Copy)]
pub union EmContextDescUpperSetup {
    /// `tcp_config`.
    pub tcp_config: u32,
    /// `tcp_fields`.
    pub tcp_fields: EmContextDescTcpFields,
}

impl EmContextDescUpperSetup {
    /// `upper_setup.tcp_config`.
    pub fn tcp_config(&self) -> u32 {
        // SAFETY: both members are plain integers over the same four bytes; every bit pattern
        // is a valid value of either.
        unsafe { self.tcp_config }
    }

    /// `upper_setup.tcp_fields`.
    pub fn tcp_fields(&self) -> EmContextDescTcpFields {
        // SAFETY: as in `tcp_config`.
        unsafe { self.tcp_fields }
    }
}

/// The `tcp_seg_setup.fields` member of [`EmContextDesc`].
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EmContextDescSegFields {
    /// `status`: descriptor status.
    pub status: u8,
    /// `hdr_len`: header length.
    pub hdr_len: u8,
    /// `mss`: maximum segment size.
    pub mss: u16,
}

/// The `tcp_seg_setup` union of [`EmContextDesc`].
#[repr(C)]
#[derive(Clone, Copy)]
pub union EmContextDescTcpSegSetup {
    /// `data`.
    pub data: u32,
    /// `fields`.
    pub fields: EmContextDescSegFields,
}

impl EmContextDescTcpSegSetup {
    /// `tcp_seg_setup.data`.
    pub fn data(&self) -> u32 {
        // SAFETY: both members are plain integers over the same four bytes; every bit pattern
        // is a valid value of either.
        unsafe { self.data }
    }

    /// `tcp_seg_setup.fields`.
    pub fn fields(&self) -> EmContextDescSegFields {
        // SAFETY: as in `data`.
        unsafe { self.fields }
    }
}

/// `struct em_context_desc`: offload context descriptor.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct EmContextDesc {
    /// `lower_setup`.
    pub lower_setup: EmContextDescLowerSetup,
    /// `upper_setup`.
    pub upper_setup: EmContextDescUpperSetup,
    /// `cmd_and_length`.
    pub cmd_and_length: u32,
    /// `tcp_seg_setup`.
    pub tcp_seg_setup: EmContextDescTcpSegSetup,
}

/// The `lower.flags` member of [`EmDataDesc`].
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EmDataDescLowerFlags {
    /// `length`: data buffer length.
    pub length: u16,
    /// `typ_len_ext`.
    pub typ_len_ext: u8,
    /// `cmd`.
    pub cmd: u8,
}

/// The `lower` union of [`EmDataDesc`].
#[repr(C)]
#[derive(Clone, Copy)]
pub union EmDataDescLower {
    /// `data`.
    pub data: u32,
    /// `flags`.
    pub flags: EmDataDescLowerFlags,
}

impl EmDataDescLower {
    /// `lower.data`.
    pub fn data(&self) -> u32 {
        // SAFETY: both members are plain integers over the same four bytes; every bit pattern
        // is a valid value of either.
        unsafe { self.data }
    }

    /// `lower.flags`.
    pub fn flags(&self) -> EmDataDescLowerFlags {
        // SAFETY: as in `data`.
        unsafe { self.flags }
    }
}

/// The `upper.fields` member of [`EmDataDesc`].
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EmDataDescUpperFields {
    /// `status`: descriptor status.
    pub status: u8,
    /// `popts`: packet options.
    pub popts: u8,
    /// `special`.
    pub special: u16,
}

/// The `upper` union of [`EmDataDesc`].
#[repr(C)]
#[derive(Clone, Copy)]
pub union EmDataDescUpper {
    /// `data`.
    pub data: u32,
    /// `fields`.
    pub fields: EmDataDescUpperFields,
}

impl EmDataDescUpper {
    /// `upper.data`.
    pub fn data(&self) -> u32 {
        // SAFETY: both members are plain integers over the same four bytes; every bit pattern
        // is a valid value of either.
        unsafe { self.data }
    }

    /// `upper.fields`.
    pub fn fields(&self) -> EmDataDescUpperFields {
        // SAFETY: as in `data`.
        unsafe { self.fields }
    }
}

/// `struct em_data_desc`: offload data descriptor.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct EmDataDesc {
    /// `buffer_addr`: address of the descriptor's buffer address.
    pub buffer_addr: u64,
    /// `lower`.
    pub lower: EmDataDescLower,
    /// `upper`.
    pub upper: EmDataDescUpper,
}

/// `struct em_rar`: receive address register (a register layout; the C's `volatile` members
/// are reached through `bus_space(9)`).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EmRar {
    /// `low`: receive address low.
    pub low: u32,
    /// `high`: receive address high.
    pub high: u32,
}

/// `struct em_ipv4_at_entry`: IPv4 address table entry (a register layout).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EmIpv4AtEntry {
    /// `ipv4_addr`: IP address (RW).
    pub ipv4_addr: u32,
    /// `reserved`.
    pub reserved: u32,
}

/// `struct em_ipv6_at_entry`: IPv6 address table entry (a register layout).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EmIpv6AtEntry {
    /// `ipv6_addr`.
    pub ipv6_addr: [u8; 16],
}

/// `struct em_fflt_entry`: flexible filter length table entry (a register layout).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EmFfltEntry {
    /// `length`: flexible filter length (RW).
    pub length: u32,
    /// `reserved`.
    pub reserved: u32,
}

/// `struct em_ffmt_entry`: flexible filter mask table entry (a register layout).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EmFfmtEntry {
    /// `mask`: flexible filter mask (RW).
    pub mask: u32,
    /// `reserved`.
    pub reserved: u32,
}

/// `struct em_ffvt_entry`: flexible filter value table entry (a register layout).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EmFfvtEntry {
    /// `value`: flexible filter value (RW).
    pub value: u32,
    /// `reserved`.
    pub reserved: u32,
}

/// `struct em_hw_stats`: statistics counters collected by the MAC.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EmHwStats {
    /// `crcerrs`.
    pub crcerrs: u64,
    /// `algnerrc`.
    pub algnerrc: u64,
    /// `symerrs`.
    pub symerrs: u64,
    /// `rxerrc`.
    pub rxerrc: u64,
    /// `mpc`.
    pub mpc: u64,
    /// `scc`.
    pub scc: u64,
    /// `ecol`.
    pub ecol: u64,
    /// `mcc`.
    pub mcc: u64,
    /// `latecol`.
    pub latecol: u64,
    /// `colc`.
    pub colc: u64,
    /// `dc`.
    pub dc: u64,
    /// `tncrs`.
    pub tncrs: u64,
    /// `sec`.
    pub sec: u64,
    /// `cexterr`.
    pub cexterr: u64,
    /// `rlec`.
    pub rlec: u64,
    /// `xonrxc`.
    pub xonrxc: u64,
    /// `xontxc`.
    pub xontxc: u64,
    /// `xoffrxc`.
    pub xoffrxc: u64,
    /// `xofftxc`.
    pub xofftxc: u64,
    /// `fcruc`.
    pub fcruc: u64,
    /// `prc64`.
    pub prc64: u64,
    /// `prc127`.
    pub prc127: u64,
    /// `prc255`.
    pub prc255: u64,
    /// `prc511`.
    pub prc511: u64,
    /// `prc1023`.
    pub prc1023: u64,
    /// `prc1522`.
    pub prc1522: u64,
    /// `gprc`.
    pub gprc: u64,
    /// `bprc`.
    pub bprc: u64,
    /// `mprc`.
    pub mprc: u64,
    /// `gptc`.
    pub gptc: u64,
    /// `gorcl`.
    pub gorcl: u64,
    /// `gorch`.
    pub gorch: u64,
    /// `gotcl`.
    pub gotcl: u64,
    /// `gotch`.
    pub gotch: u64,
    /// `rnbc`.
    pub rnbc: u64,
    /// `ruc`.
    pub ruc: u64,
    /// `rfc`.
    pub rfc: u64,
    /// `roc`.
    pub roc: u64,
    /// `rjc`.
    pub rjc: u64,
    /// `mgprc`.
    pub mgprc: u64,
    /// `mgpdc`.
    pub mgpdc: u64,
    /// `mgptc`.
    pub mgptc: u64,
    /// `torl`.
    pub torl: u64,
    /// `torh`.
    pub torh: u64,
    /// `totl`.
    pub totl: u64,
    /// `toth`.
    pub toth: u64,
    /// `tpr`.
    pub tpr: u64,
    /// `tpt`.
    pub tpt: u64,
    /// `ptc64`.
    pub ptc64: u64,
    /// `ptc127`.
    pub ptc127: u64,
    /// `ptc255`.
    pub ptc255: u64,
    /// `ptc511`.
    pub ptc511: u64,
    /// `ptc1023`.
    pub ptc1023: u64,
    /// `ptc1522`.
    pub ptc1522: u64,
    /// `mptc`.
    pub mptc: u64,
    /// `bptc`.
    pub bptc: u64,
    /// `tsctc`.
    pub tsctc: u64,
    /// `tsctfc`.
    pub tsctfc: u64,
    /// `iac`.
    pub iac: u64,
    /// `icrxptc`.
    pub icrxptc: u64,
    /// `icrxatc`.
    pub icrxatc: u64,
    /// `ictxptc`.
    pub ictxptc: u64,
    /// `ictxatc`.
    pub ictxatc: u64,
    /// `ictxqec`.
    pub ictxqec: u64,
    /// `ictxqmtc`.
    pub ictxqmtc: u64,
    /// `icrxdmtc`.
    pub icrxdmtc: u64,
    /// `icrxoc`.
    pub icrxoc: u64,
    /// `sdpc`.
    pub sdpc: u64,
    /// `mngpdc`.
    pub mngpdc: u64,
    /// `mngptc`.
    pub mngptc: u64,
    /// `mngprc`.
    pub mngprc: u64,
    /// `b2ospc`.
    pub b2ospc: u64,
    /// `o2bgptc`.
    pub o2bgptc: u64,
    /// `b2ogprc`.
    pub b2ogprc: u64,
    /// `o2bspc`.
    pub o2bspc: u64,
    /// `rpthc`.
    pub rpthc: u64,
}

/// `struct em_hw`: the variables the shared code (`if_em_hw.c`) reads and writes.
///
/// The C's shape is kept member for member; if_em(4) embeds it in its softc as `sc->hw` and
/// hands `&sc->hw` to every function here. Two members are typed rather than `void *`:
/// `back` points at the softc's [`EmOsdep`] (the register handles), and `gcu` at the EP80579
/// GCU softc. One member is added: `pci_ops`, the if_em.c functions the shared code calls
/// back into (see [`EmPciOps`]).
pub struct EmHw {
    /// `hw_addr`: kept for the shape; OpenBSD's driver never sets or reads it (the registers
    /// are reached through `back`).
    pub hw_addr: Option<NonNull<u8>>,
    /// `flash_address`: kept for the shape, unused as `hw_addr`.
    pub flash_address: Option<NonNull<u8>>,
    /// `mac_type`.
    pub mac_type: EmMacType,
    /// `phy_type`.
    pub phy_type: EmPhyType,
    /// `phy_init_script`.
    pub phy_init_script: u32,
    /// `media_type`.
    pub media_type: EmMediaType,
    /// `back`: the softc's `struct em_osdep` (the C's `void *`, always cast to it).
    pub back: NonNull<EmOsdep>,
    /// `eeprom_shadow_ram`: the ICH8 shadow RAM, `NULL` (`None`) unless the driver allocates
    /// it (OpenBSD's never does).
    pub eeprom_shadow_ram: Option<Box<[EmShadowRam]>>,
    /// `flash_bank_size`.
    pub flash_bank_size: u32,
    /// `flash_base_addr`.
    pub flash_base_addr: u32,
    /// `fc`: the flow control mode (`E1000_FC_*`).
    pub fc: u32,
    /// `bus_speed`.
    pub bus_speed: EmBusSpeed,
    /// `bus_width`.
    pub bus_width: EmBusWidth,
    /// `bus_type`.
    pub bus_type: EmBusType,
    /// `eeprom`.
    pub eeprom: EmEepromInfo,
    /// `master_slave`.
    pub master_slave: EmMsType,
    /// `original_master_slave`.
    pub original_master_slave: EmMsType,
    /// `ffe_config_state`.
    pub ffe_config_state: EmFfeConfig,
    /// `asf_firmware_present`.
    pub asf_firmware_present: u32,
    /// `eeprom_semaphore_present`.
    pub eeprom_semaphore_present: u32,
    /// `swfw_sync_present`.
    pub swfw_sync_present: u32,
    /// `swfwhw_semaphore_present`.
    pub swfwhw_semaphore_present: u32,
    /// `io_base`.
    pub io_base: u64,
    /// `phy_id`.
    pub phy_id: u32,
    /// `phy_revision`.
    pub phy_revision: u32,
    /// `phy_addr`.
    pub phy_addr: u32,
    /// `original_fc`.
    pub original_fc: u32,
    /// `txcw`.
    pub txcw: u32,
    /// `autoneg_failed`.
    pub autoneg_failed: u32,
    /// `max_frame_size`.
    pub max_frame_size: u32,
    /// `min_frame_size`.
    pub min_frame_size: u32,
    /// `mc_filter_type`.
    pub mc_filter_type: u32,
    /// `num_mc_addrs`.
    pub num_mc_addrs: u32,
    /// `collision_delta`.
    pub collision_delta: u32,
    /// `tx_packet_delta`.
    pub tx_packet_delta: u32,
    /// `ledctl_default`.
    pub ledctl_default: u32,
    /// `ledctl_mode1`.
    pub ledctl_mode1: u32,
    /// `ledctl_mode2`.
    pub ledctl_mode2: u32,
    /// `tx_pkt_filtering`.
    pub tx_pkt_filtering: bool,
    /// `mng_cookie`.
    pub mng_cookie: EmHostMngDhcpCookie,
    /// `phy_spd_default`.
    pub phy_spd_default: u16,
    /// `autoneg_advertised`.
    pub autoneg_advertised: u16,
    /// `pci_cmd_word`.
    pub pci_cmd_word: u16,
    /// `fc_high_water`.
    pub fc_high_water: u16,
    /// `fc_low_water`.
    pub fc_low_water: u16,
    /// `fc_pause_time`.
    pub fc_pause_time: u16,
    /// `current_ifs_val`.
    pub current_ifs_val: u16,
    /// `ifs_min_val`.
    pub ifs_min_val: u16,
    /// `ifs_max_val`.
    pub ifs_max_val: u16,
    /// `ifs_step_size`.
    pub ifs_step_size: u16,
    /// `ifs_ratio`.
    pub ifs_ratio: u16,
    /// `device_id`.
    pub device_id: u16,
    /// `vendor_id`.
    pub vendor_id: u16,
    /// `subsystem_id`.
    pub subsystem_id: u16,
    /// `subsystem_vendor_id`.
    pub subsystem_vendor_id: u16,
    /// `revision_id`.
    pub revision_id: u8,
    /// `autoneg`.
    pub autoneg: u8,
    /// `mdix`.
    pub mdix: u8,
    /// `forced_speed_duplex`.
    pub forced_speed_duplex: EmSpeedDuplexType,
    /// `wait_autoneg_complete`.
    pub wait_autoneg_complete: u8,
    /// `dma_fairness`.
    pub dma_fairness: u8,
    /// `mac_addr`.
    pub mac_addr: [u8; NODE_ADDRESS_SIZE],
    /// `perm_mac_addr`.
    pub perm_mac_addr: [u8; NODE_ADDRESS_SIZE],
    /// `disable_polarity_correction`.
    pub disable_polarity_correction: bool,
    /// `speed_downgraded`.
    pub speed_downgraded: bool,
    /// `smart_speed`.
    pub smart_speed: EmSmartSpeed,
    /// `dsp_config_state`.
    pub dsp_config_state: EmDspConfig,
    /// `get_link_status`.
    pub get_link_status: bool,
    /// `serdes_link_down`.
    pub serdes_link_down: bool,
    /// `tbi_compatibility_en`.
    pub tbi_compatibility_en: bool,
    /// `tbi_compatibility_on`.
    pub tbi_compatibility_on: bool,
    /// `laa_is_present`.
    pub laa_is_present: bool,
    /// `phy_reset_disable`.
    pub phy_reset_disable: bool,
    /// `initialize_hw_bits_disable`.
    pub initialize_hw_bits_disable: bool,
    /// `fc_send_xon`.
    pub fc_send_xon: bool,
    /// `fc_strict_ieee`.
    pub fc_strict_ieee: bool,
    /// `report_tx_early`.
    pub report_tx_early: bool,
    /// `adaptive_ifs`.
    pub adaptive_ifs: bool,
    /// `ifs_params_forced`.
    pub ifs_params_forced: bool,
    /// `in_ifs_mode`.
    pub in_ifs_mode: bool,
    /// `mng_reg_access_disabled`.
    pub mng_reg_access_disabled: bool,
    /// `leave_av_bit_off`.
    pub leave_av_bit_off: bool,
    /// `kmrn_lock_loss_workaround_disabled`.
    pub kmrn_lock_loss_workaround_disabled: bool,
    /// `icp_xxxx_is_link_up`.
    pub icp_xxxx_is_link_up: bool,
    /// `icp_xxxx_port_num`.
    pub icp_xxxx_port_num: u32,
    /// `gcu`: the EP80579 GCU that owns the MDIO bus of an `em_icp_xxxx` MAC.
    pub gcu: Option<NonNull<GcuSoftc>>,
    /// `bus_func`.
    pub bus_func: u8,
    /// `swfw`.
    pub swfw: u16,
    /// `eee_enable`.
    pub eee_enable: bool,
    /// `sw_flag`.
    pub sw_flag: i32,
    /// `sgmii_active`.
    pub sgmii_active: bool,
    /// Not in the C: the callbacks into if_em.c (`em_pci_set_mwi`, `em_read_pci_cfg`, ...),
    /// which the C links by name.
    pub pci_ops: &'static EmPciOps,
}

impl EmHw {
    /// A `struct em_hw` as the C's zeroed softc holds it, with `back` and the if_em.c
    /// callbacks set: every integer 0, every flag false, every enumeration its 0 member.
    ///
    /// # Safety
    ///
    /// `back` must point at an [`EmOsdep`] that stays valid, and whose members are not
    /// written, for as long as any function of this module runs on the returned value (in
    /// if_em(4) both live in the same softc and the osdep is filled before the first call).
    pub unsafe fn new(back: NonNull<EmOsdep>, pci_ops: &'static EmPciOps) -> Self {
        Self {
            hw_addr: None,
            flash_address: None,
            mac_type: em_undefined,
            phy_type: em_phy_m88,
            phy_init_script: 0,
            media_type: em_media_type_copper,
            back,
            eeprom_shadow_ram: None,
            flash_bank_size: 0,
            flash_base_addr: 0,
            fc: 0,
            bus_speed: em_bus_speed_unknown,
            bus_width: em_bus_width_unknown,
            bus_type: em_bus_type_unknown,
            eeprom: EmEepromInfo::default(),
            master_slave: em_ms_hw_default,
            original_master_slave: em_ms_hw_default,
            ffe_config_state: em_ffe_config_enabled,
            asf_firmware_present: 0,
            eeprom_semaphore_present: 0,
            swfw_sync_present: 0,
            swfwhw_semaphore_present: 0,
            io_base: 0,
            phy_id: 0,
            phy_revision: 0,
            phy_addr: 0,
            original_fc: 0,
            txcw: 0,
            autoneg_failed: 0,
            max_frame_size: 0,
            min_frame_size: 0,
            mc_filter_type: 0,
            num_mc_addrs: 0,
            collision_delta: 0,
            tx_packet_delta: 0,
            ledctl_default: 0,
            ledctl_mode1: 0,
            ledctl_mode2: 0,
            tx_pkt_filtering: false,
            mng_cookie: EmHostMngDhcpCookie::default(),
            phy_spd_default: 0,
            autoneg_advertised: 0,
            pci_cmd_word: 0,
            fc_high_water: 0,
            fc_low_water: 0,
            fc_pause_time: 0,
            current_ifs_val: 0,
            ifs_min_val: 0,
            ifs_max_val: 0,
            ifs_step_size: 0,
            ifs_ratio: 0,
            device_id: 0,
            vendor_id: 0,
            subsystem_id: 0,
            subsystem_vendor_id: 0,
            revision_id: 0,
            autoneg: 0,
            mdix: 0,
            forced_speed_duplex: em_10_half,
            wait_autoneg_complete: 0,
            dma_fairness: 0,
            mac_addr: [0; NODE_ADDRESS_SIZE],
            perm_mac_addr: [0; NODE_ADDRESS_SIZE],
            disable_polarity_correction: false,
            speed_downgraded: false,
            smart_speed: em_smart_speed_default,
            dsp_config_state: em_dsp_config_disabled,
            get_link_status: false,
            serdes_link_down: false,
            tbi_compatibility_en: false,
            tbi_compatibility_on: false,
            laa_is_present: false,
            phy_reset_disable: false,
            initialize_hw_bits_disable: false,
            fc_send_xon: false,
            fc_strict_ieee: false,
            report_tx_early: false,
            adaptive_ifs: false,
            ifs_params_forced: false,
            in_ifs_mode: false,
            mng_reg_access_disabled: false,
            leave_av_bit_off: false,
            kmrn_lock_loss_workaround_disabled: false,
            icp_xxxx_is_link_up: false,
            icp_xxxx_port_num: 0,
            gcu: None,
            bus_func: 0,
            swfw: 0,
            eee_enable: false,
            sw_flag: 0,
            sgmii_active: false,
            pci_ops,
        }
    }

    /// The softc's `struct em_osdep`, `(struct em_osdep *)hw->back` in the C.
    pub fn osdep(&self) -> &EmOsdep {
        // SAFETY: `EmHw::new`'s contract: `back` points at a live `EmOsdep` that nothing
        // writes while the shared code runs, so a shared reference for this borrow is sound.
        unsafe { self.back.as_ref() }
    }
}

/// The functions if_em.c defines for the shared code (prototypes in `if_em_hw.h`). The C
/// links them by name; here if_em(4) passes a `static` table of them to [`EmHw::new`], and
/// the free functions of the same names in this module call through it.
pub struct EmPciOps {
    /// `em_pci_set_mwi`: sets Memory Write and Invalidate in the PCI command register.
    pub em_pci_set_mwi: fn(&EmHw),
    /// `em_pci_clear_mwi`: clears it.
    pub em_pci_clear_mwi: fn(&EmHw),
    /// `em_read_pci_cfg`: reads the 16-bit configuration word at `reg`.
    pub em_read_pci_cfg: fn(&EmHw, u32, &mut u16),
    /// `em_write_pci_cfg`: writes the 16-bit configuration word at `reg`.
    pub em_write_pci_cfg: fn(&EmHw, u32, &u16),
    /// `em_read_pcie_cap_reg`: reads a PCIe capability register; the C's status as `Err`.
    pub em_read_pcie_cap_reg: fn(&EmHw, u32, &mut u16) -> Result<(), i32>,
}

/// The `u` union of [`E1000AdvTxContextDesc`].
#[repr(C)]
#[derive(Clone, Copy)]
pub union E1000AdvTxContextDescU {
    /// `launch_time`.
    pub launch_time: u32,
    /// `seqnum_seed`.
    pub seqnum_seed: u32,
}

impl E1000AdvTxContextDescU {
    /// `u.seqnum_seed` (the same word as `u.launch_time`).
    pub fn seqnum_seed(&self) -> u32 {
        // SAFETY: both members are the same `u32`; every bit pattern is valid.
        unsafe { self.seqnum_seed }
    }
}

/// `struct e1000_adv_tx_context_desc`: the advanced (82575 and later) context descriptor.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct E1000AdvTxContextDesc {
    /// `vlan_macip_lens`.
    pub vlan_macip_lens: u32,
    /// `u`.
    pub u: E1000AdvTxContextDescU,
    /// `type_tucmd_mlhl`.
    pub type_tucmd_mlhl: u32,
    /// `mss_l4len_idx`.
    pub mss_l4len_idx: u32,
}

/// `em_mng_mode`.
#[repr(u32)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(non_camel_case_types)] // OpenBSD names, verbatim, for grep-ability
pub enum EmMngMode {
    /// `em_mng_mode_none`.
    #[default]
    em_mng_mode_none = 0,
    /// `em_mng_mode_asf`.
    em_mng_mode_asf,
    /// `em_mng_mode_pt`.
    em_mng_mode_pt,
    /// `em_mng_mode_ipmi`.
    em_mng_mode_ipmi,
    /// `em_mng_mode_host_interface_only`.
    em_mng_mode_host_interface_only,
}

pub use EmMngMode::*;

/// `struct em_host_command_header`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EmHostCommandHeader {
    /// `command_id`.
    pub command_id: u8,
    /// `command_length`.
    pub command_length: u8,
    /// `command_options`: I/F bits for command, status for return.
    pub command_options: u8,
    /// `checksum`.
    pub checksum: u8,
}

/// `struct em_host_command_info`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EmHostCommandInfo {
    /// `command_header`: command head/command result head has 4 bytes.
    pub command_header: EmHostCommandHeader,
    /// `command_data`: command data can length 0..252.
    pub command_data: [u8; E1000_HI_MAX_DATA_LENGTH],
}

/// `union ich8_hws_flash_status`: the ICH8 GbE flash hardware sequencing status register
/// (offset 04h HSFSTS), `regval` with one accessor pair per bit field of `hsf_status`
/// (`struct ich8_hsfsts`, allocated from bit 0 up as both ABIs do).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ich8HwsFlashStatus {
    /// `regval`.
    pub regval: u16,
}

/// Bit field accessors over a register word: `get_<f>`/`set_<f>` for a field of `w` bits at
/// bit `s`.
macro_rules! em_bitfields {
    ($t:ty, $ty:ty; $($get:ident, $set:ident, $s:expr, $w:expr;)*) => {
        impl $t {
            $(
                #[doc = concat!("The `", stringify!($get), "` bit field.")]
                pub fn $get(&self) -> $ty {
                    (self.regval >> $s) & ((1 << $w) - 1)
                }

                #[doc = concat!("Sets the `", stringify!($get), "` bit field.")]
                pub fn $set(&mut self, v: $ty) {
                    let mask: $ty = ((1 << $w) - 1) << $s;
                    self.regval = (self.regval & !mask) | ((v << $s) & mask);
                }
            )*
        }
    };
}

em_bitfields!(Ich8HwsFlashStatus, u16;
    flcdone, set_flcdone, 0, 1;
    flcerr, set_flcerr, 1, 1;
    dael, set_dael, 2, 1;
    berasesz, set_berasesz, 3, 2;
    flcinprog, set_flcinprog, 5, 1;
    reserved1, set_reserved1, 6, 2;
    reserved2, set_reserved2, 8, 6;
    fldesvalid, set_fldesvalid, 14, 1;
    flockdn, set_flockdn, 15, 1;
);

/// `union ich8_hws_flash_ctrl`: the ICH8 GbE flash hardware sequencing control register
/// (offset 06h FLCTL), `regval` with accessors for `hsf_ctrl` (`struct ich8_hsflctl`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ich8HwsFlashCtrl {
    /// `regval`.
    pub regval: u16,
}

em_bitfields!(Ich8HwsFlashCtrl, u16;
    flcgo, set_flcgo, 0, 1;
    flcycle, set_flcycle, 1, 2;
    reserved, set_reserved, 3, 5;
    fldbcount, set_fldbcount, 8, 2;
    flockdn, set_flockdn, 10, 6;
);

/// `union ich8_hws_flash_regacc`: ICH8 flash region access permissions, `regval` with
/// accessors for `hsf_flregacc` (`struct ich8_flracc`). The C's `regval` is a `uint16_t`
/// over a 32-bit bit field structure; it is kept 32 bits wide here so every field is
/// reachable (nothing in the shared code uses the type).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ich8HwsFlashRegacc {
    /// `regval`.
    pub regval: u32,
}

em_bitfields!(Ich8HwsFlashRegacc, u32;
    grra, set_grra, 0, 8;
    grwa, set_grwa, 8, 8;
    gmrag, set_gmrag, 16, 8;
    gmwag, set_gmwag, 24, 8;
);

/// `struct sfp_e1000_flags`: flags for SFP modules compatible with Ethernet up to 1Gb (one
/// byte read from the module; bit fields allocated from bit 0 up).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SfpE1000Flags {
    /// The byte.
    pub regval: u8,
}

em_bitfields!(SfpE1000Flags, u8;
    e1000_base_sx, set_e1000_base_sx, 0, 1;
    e1000_base_lx, set_e1000_base_lx, 1, 1;
    e1000_base_cx, set_e1000_base_cx, 2, 1;
    e1000_base_t, set_e1000_base_t, 3, 1;
    e100_base_lx, set_e100_base_lx, 4, 1;
    e100_base_fx, set_e100_base_fx, 5, 1;
    e10_base_bx10, set_e10_base_bx10, 6, 1;
    e10_base_px, set_e10_base_px, 7, 1;
);

/// `em_igp_cable_length_table`: IGP cable length (metres) by AGC value.
static EM_IGP_CABLE_LENGTH_TABLE: [u16; IGP01E1000_AGC_LENGTH_TABLE_SIZE] = [
    5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 10, 10, 10, 10, 10, 10, 10, 20, 20, 20, 20,
    20, 25, 25, 25, 25, 25, 25, 25, 30, 30, 30, 30, 40, 40, 40, 40, 40, 40, 40, 40, 40, 50, 50, 50,
    50, 50, 50, 50, 60, 60, 60, 60, 60, 60, 60, 60, 60, 70, 70, 70, 70, 70, 70, 80, 80, 80, 80, 80,
    80, 90, 90, 90, 90, 90, 90, 90, 90, 90, 100, 100, 100, 100, 100, 100, 100, 100, 100, 100, 100,
    100, 100, 100, 110, 110, 110, 110, 110, 110, 110, 110, 110, 110, 110, 110, 110, 110, 110, 110,
    110, 110, 120, 120, 120, 120, 120, 120, 120, 120, 120, 120,
];

/// `em_igp_2_cable_length_table`: IGP2/IGP3 cable length (metres) by AGC index.
static EM_IGP_2_CABLE_LENGTH_TABLE: [u16; IGP02E1000_AGC_LENGTH_TABLE_SIZE] = [
    0, 0, 0, 0, 0, 0, 0, 0, 3, 5, 8, 11, 13, 16, 18, 21, 0, 0, 0, 3, 6, 10, 13, 16, 19, 23, 26, 29,
    32, 35, 38, 41, 6, 10, 14, 18, 22, 26, 30, 33, 37, 41, 44, 48, 51, 54, 58, 61, 21, 26, 31, 35,
    40, 44, 49, 53, 57, 61, 65, 68, 72, 75, 79, 82, 40, 45, 51, 56, 61, 66, 70, 75, 79, 83, 87, 91,
    94, 98, 101, 104, 60, 66, 72, 77, 82, 87, 92, 96, 100, 104, 108, 111, 114, 117, 119, 121, 83,
    89, 95, 100, 105, 109, 113, 116, 119, 122, 124, 104, 109, 114, 118, 121, 124,
];

/// `em_pci_set_mwi` (if_em.c): sets Memory Write and Invalidate, through `hw.pci_ops`.
pub fn em_pci_set_mwi(hw: &EmHw) {
    (hw.pci_ops.em_pci_set_mwi)(hw)
}

/// `em_pci_clear_mwi` (if_em.c): clears Memory Write and Invalidate, through `hw.pci_ops`.
pub fn em_pci_clear_mwi(hw: &EmHw) {
    (hw.pci_ops.em_pci_clear_mwi)(hw)
}

/// `em_read_pci_cfg` (if_em.c): reads a 16-bit configuration word, through `hw.pci_ops`.
pub fn em_read_pci_cfg(hw: &EmHw, reg: u32, value: &mut u16) {
    (hw.pci_ops.em_read_pci_cfg)(hw, reg, value)
}

/// `em_write_pci_cfg` (if_em.c): writes a 16-bit configuration word, through `hw.pci_ops`.
pub fn em_write_pci_cfg(hw: &EmHw, reg: u32, value: &u16) {
    (hw.pci_ops.em_write_pci_cfg)(hw, reg, value)
}

/// `em_read_pcie_cap_reg` (if_em.c): reads a PCIe capability register, through
/// `hw.pci_ops`.
pub fn em_read_pcie_cap_reg(hw: &EmHw, reg: u32, value: &mut u16) -> Result<(), i32> {
    (hw.pci_ops.em_read_pcie_cap_reg)(hw, reg, value)
}

/// Sets the phy type member in the hw struct.
fn em_set_phy_type(hw: &mut EmHw) -> Result<(), i32> {
    if hw.mac_type == em_undefined {
        return Err(-E1000_ERR_PHY_TYPE);
    }

    hw.phy_type = match hw.phy_id {
        M88E1000_E_PHY_ID | M88E1000_I_PHY_ID | M88E1011_I_PHY_ID | M88E1111_I_PHY_ID
        | M88E1112_E_PHY_ID | M88E1543_E_PHY_ID | M88E1512_E_PHY_ID | I210_I_PHY_ID
        | I347AT4_E_PHY_ID => em_phy_m88,
        IGP01E1000_I_PHY_ID
            if matches!(
                hw.mac_type,
                em_82541 | em_82541_rev_2 | em_82547 | em_82547_rev_2
            ) =>
        {
            em_phy_igp
        }
        IGP01E1000_I_PHY_ID | IGP03E1000_E_PHY_ID | IGP04E1000_E_PHY_ID => em_phy_igp_3,
        IFE_E_PHY_ID | IFE_PLUS_E_PHY_ID | IFE_C_E_PHY_ID => em_phy_ife,
        M88E1141_E_PHY_ID => em_phy_oem,
        I82577_E_PHY_ID => em_phy_82577,
        I82578_E_PHY_ID => em_phy_82578,
        I82579_E_PHY_ID => em_phy_82579,
        I217_E_PHY_ID => em_phy_i217,
        I82580_I_PHY_ID | I350_I_PHY_ID => em_phy_82580,
        RTL8211_E_PHY_ID => em_phy_rtl8211,
        BME1000_E_PHY_ID if hw.phy_revision == 1 => em_phy_bm,
        BME1000_E_PHY_ID | GG82563_E_PHY_ID if hw.mac_type == em_80003es2lan => em_phy_gg82563,
        _ => {
            // Should never have loaded on this device.
            hw.phy_type = em_phy_undefined;
            return Err(-E1000_ERR_PHY_TYPE);
        }
    };

    Ok(())
}

/// IGP phy init script: initializes the GbE PHY.
fn em_phy_init_script(hw: &mut EmHw) {
    if hw.phy_init_script != 0 {
        msec_delay(20);
        // Save off the current value of register 0x2F5B to be restored at the end of this
        // routine.
        let mut phy_saved_data: u16 = 0;
        let _ = em_read_phy_reg(hw, 0x2F5B, &mut phy_saved_data);

        // Disable the PHY transmitter.
        let _ = em_write_phy_reg(hw, 0x2F5B, 0x0003);
        msec_delay(20);
        let _ = em_write_phy_reg(hw, 0x0000, 0x0140);
        msec_delay(5);

        match hw.mac_type {
            em_82541 | em_82547 => {
                let _ = em_write_phy_reg(hw, 0x1F95, 0x0001);
                let _ = em_write_phy_reg(hw, 0x1F71, 0xBD21);
                let _ = em_write_phy_reg(hw, 0x1F79, 0x0018);
                let _ = em_write_phy_reg(hw, 0x1F30, 0x1600);
                let _ = em_write_phy_reg(hw, 0x1F31, 0x0014);
                let _ = em_write_phy_reg(hw, 0x1F32, 0x161C);
                let _ = em_write_phy_reg(hw, 0x1F94, 0x0003);
                let _ = em_write_phy_reg(hw, 0x1F96, 0x003F);
                let _ = em_write_phy_reg(hw, 0x2010, 0x0008);
            }
            em_82541_rev_2 | em_82547_rev_2 => {
                let _ = em_write_phy_reg(hw, 0x1F73, 0x0099);
            }
            _ => {}
        }

        let _ = em_write_phy_reg(hw, 0x0000, 0x3300);
        msec_delay(20);

        // Now enable the transmitter.
        let _ = em_write_phy_reg(hw, 0x2F5B, phy_saved_data);

        if hw.mac_type == em_82547 {
            let mut fused: u16 = 0;
            // Move to analog registers page.
            let _ = em_read_phy_reg(hw, IGP01E1000_ANALOG_SPARE_FUSE_STATUS, &mut fused);

            if fused & IGP01E1000_ANALOG_SPARE_FUSE_ENABLED == 0 {
                let _ = em_read_phy_reg(hw, IGP01E1000_ANALOG_FUSE_STATUS, &mut fused);

                let mut fine = fused & IGP01E1000_ANALOG_FUSE_FINE_MASK;
                let mut coarse = fused & IGP01E1000_ANALOG_FUSE_COARSE_MASK;

                if coarse > IGP01E1000_ANALOG_FUSE_COARSE_THRESH {
                    coarse = coarse.wrapping_sub(IGP01E1000_ANALOG_FUSE_COARSE_10);
                    fine = fine.wrapping_sub(IGP01E1000_ANALOG_FUSE_FINE_1);
                } else if coarse == IGP01E1000_ANALOG_FUSE_COARSE_THRESH {
                    fine = fine.wrapping_sub(IGP01E1000_ANALOG_FUSE_FINE_10);
                }

                fused = (fused & IGP01E1000_ANALOG_FUSE_POLY_MASK)
                    | (fine & IGP01E1000_ANALOG_FUSE_FINE_MASK)
                    | (coarse & IGP01E1000_ANALOG_FUSE_COARSE_MASK);

                let _ = em_write_phy_reg(hw, IGP01E1000_ANALOG_FUSE_CONTROL, fused);

                let _ = em_write_phy_reg(
                    hw,
                    IGP01E1000_ANALOG_FUSE_BYPASS,
                    IGP01E1000_ANALOG_FUSE_ENABLE_SW_CONTROL,
                );
            }
        }
    }
}

/// `em_set_mac_type`: sets the mac type member in the hw struct from the PCI device id (and
/// the revision, for the 82542), and the semaphore and firmware flags of that MAC.
pub fn em_set_mac_type(hw: &mut EmHw) -> Result<(), i32> {
    match hw.device_id {
        E1000_DEV_ID_82542 => match hw.revision_id {
            E1000_82542_2_0_REV_ID => hw.mac_type = em_82542_rev2_0,
            E1000_82542_2_1_REV_ID => hw.mac_type = em_82542_rev2_1,
            // Invalid 82542 revision ID.
            _ => return Err(-E1000_ERR_MAC_TYPE),
        },
        E1000_DEV_ID_82543GC_FIBER | E1000_DEV_ID_82543GC_COPPER => hw.mac_type = em_82543,
        E1000_DEV_ID_82544EI_COPPER
        | E1000_DEV_ID_82544EI_FIBER
        | E1000_DEV_ID_82544GC_COPPER
        | E1000_DEV_ID_82544GC_LOM => hw.mac_type = em_82544,
        E1000_DEV_ID_82540EM
        | E1000_DEV_ID_82540EM_LOM
        | E1000_DEV_ID_82540EP
        | E1000_DEV_ID_82540EP_LOM
        | E1000_DEV_ID_82540EP_LP => hw.mac_type = em_82540,
        E1000_DEV_ID_82545EM_COPPER | E1000_DEV_ID_82545EM_FIBER => hw.mac_type = em_82545,
        E1000_DEV_ID_82545GM_COPPER | E1000_DEV_ID_82545GM_FIBER | E1000_DEV_ID_82545GM_SERDES => {
            hw.mac_type = em_82545_rev_3
        }
        E1000_DEV_ID_82546EB_COPPER
        | E1000_DEV_ID_82546EB_FIBER
        | E1000_DEV_ID_82546EB_QUAD_COPPER => hw.mac_type = em_82546,
        E1000_DEV_ID_82546GB_COPPER
        | E1000_DEV_ID_82546GB_FIBER
        | E1000_DEV_ID_82546GB_SERDES
        | E1000_DEV_ID_82546GB_PCIE
        | E1000_DEV_ID_82546GB_QUAD_COPPER
        | E1000_DEV_ID_82546GB_QUAD_COPPER_KSP3
        | E1000_DEV_ID_82546GB_2 => hw.mac_type = em_82546_rev_3,
        E1000_DEV_ID_82541EI | E1000_DEV_ID_82541EI_MOBILE | E1000_DEV_ID_82541ER_LOM => {
            hw.mac_type = em_82541
        }
        E1000_DEV_ID_82541ER
        | E1000_DEV_ID_82541GI
        | E1000_DEV_ID_82541GI_LF
        | E1000_DEV_ID_82541GI_MOBILE => hw.mac_type = em_82541_rev_2,
        E1000_DEV_ID_82547EI | E1000_DEV_ID_82547EI_MOBILE => hw.mac_type = em_82547,
        E1000_DEV_ID_82547GI => hw.mac_type = em_82547_rev_2,
        E1000_DEV_ID_82571EB_AF
        | E1000_DEV_ID_82571EB_AT
        | E1000_DEV_ID_82571EB_COPPER
        | E1000_DEV_ID_82571EB_FIBER
        | E1000_DEV_ID_82571EB_SERDES
        | E1000_DEV_ID_82571EB_QUAD_COPPER
        | E1000_DEV_ID_82571EB_QUAD_FIBER
        | E1000_DEV_ID_82571EB_QUAD_COPPER_LP
        | E1000_DEV_ID_82571EB_SERDES_DUAL
        | E1000_DEV_ID_82571EB_SERDES_QUAD
        | E1000_DEV_ID_82571PT_QUAD_COPPER => hw.mac_type = em_82571,
        E1000_DEV_ID_82572EI_COPPER
        | E1000_DEV_ID_82572EI_FIBER
        | E1000_DEV_ID_82572EI_SERDES
        | E1000_DEV_ID_82572EI => hw.mac_type = em_82572,
        E1000_DEV_ID_82573E
        | E1000_DEV_ID_82573E_IAMT
        | E1000_DEV_ID_82573E_PM
        | E1000_DEV_ID_82573L
        | E1000_DEV_ID_82573L_PL_1
        | E1000_DEV_ID_82573L_PL_2
        | E1000_DEV_ID_82573V_PM => hw.mac_type = em_82573,
        E1000_DEV_ID_82574L | E1000_DEV_ID_82574LA | E1000_DEV_ID_82583V => hw.mac_type = em_82574,
        E1000_DEV_ID_82575EB_PT
        | E1000_DEV_ID_82575EB_PF
        | E1000_DEV_ID_82575GB_QP
        | E1000_DEV_ID_82575GB_QP_PM => {
            hw.mac_type = em_82575;
            hw.initialize_hw_bits_disable = true;
        }
        E1000_DEV_ID_82576
        | E1000_DEV_ID_82576_FIBER
        | E1000_DEV_ID_82576_SERDES
        | E1000_DEV_ID_82576_QUAD_COPPER
        | E1000_DEV_ID_82576_QUAD_CU_ET2
        | E1000_DEV_ID_82576_NS
        | E1000_DEV_ID_82576_NS_SERDES
        | E1000_DEV_ID_82576_SERDES_QUAD => {
            hw.mac_type = em_82576;
            hw.initialize_hw_bits_disable = true;
        }
        E1000_DEV_ID_82580_COPPER
        | E1000_DEV_ID_82580_FIBER
        | E1000_DEV_ID_82580_QUAD_FIBER
        | E1000_DEV_ID_82580_SERDES
        | E1000_DEV_ID_82580_SGMII
        | E1000_DEV_ID_82580_COPPER_DUAL
        | E1000_DEV_ID_DH89XXCC_SGMII
        | E1000_DEV_ID_DH89XXCC_SERDES
        | E1000_DEV_ID_DH89XXCC_BACKPLANE
        | E1000_DEV_ID_DH89XXCC_SFP => {
            hw.mac_type = em_82580;
            hw.initialize_hw_bits_disable = true;
        }
        E1000_DEV_ID_I210_COPPER
        | E1000_DEV_ID_I210_COPPER_OEM1
        | E1000_DEV_ID_I210_COPPER_IT
        | E1000_DEV_ID_I210_FIBER
        | E1000_DEV_ID_I210_SERDES
        | E1000_DEV_ID_I210_SGMII
        | E1000_DEV_ID_I210_COPPER_FLASHLESS
        | E1000_DEV_ID_I210_SERDES_FLASHLESS
        | E1000_DEV_ID_I211_COPPER => {
            hw.mac_type = em_i210;
            hw.initialize_hw_bits_disable = true;
            hw.eee_enable = true;
        }
        E1000_DEV_ID_I350_COPPER
        | E1000_DEV_ID_I350_FIBER
        | E1000_DEV_ID_I350_SERDES
        | E1000_DEV_ID_I350_SGMII
        | E1000_DEV_ID_I350_DA4
        | E1000_DEV_ID_I354_BACKPLANE_1GBPS
        | E1000_DEV_ID_I354_SGMII
        | E1000_DEV_ID_I354_BACKPLANE_2_5GBPS => {
            hw.mac_type = em_i350;
            hw.initialize_hw_bits_disable = true;
            hw.eee_enable = true;
        }
        E1000_DEV_ID_80003ES2LAN_COPPER_SPT
        | E1000_DEV_ID_80003ES2LAN_SERDES_SPT
        | E1000_DEV_ID_80003ES2LAN_COPPER_DPT
        | E1000_DEV_ID_80003ES2LAN_SERDES_DPT => hw.mac_type = em_80003es2lan,
        E1000_DEV_ID_ICH8_IFE
        | E1000_DEV_ID_ICH8_IFE_G
        | E1000_DEV_ID_ICH8_IFE_GT
        | E1000_DEV_ID_ICH8_IGP_AMT
        | E1000_DEV_ID_ICH8_IGP_C
        | E1000_DEV_ID_ICH8_IGP_M
        | E1000_DEV_ID_ICH8_IGP_M_AMT
        | E1000_DEV_ID_ICH8_82567V_3 => hw.mac_type = em_ich8lan,
        E1000_DEV_ID_ICH9_BM
        | E1000_DEV_ID_ICH9_IFE
        | E1000_DEV_ID_ICH9_IFE_G
        | E1000_DEV_ID_ICH9_IFE_GT
        | E1000_DEV_ID_ICH9_IGP_AMT
        | E1000_DEV_ID_ICH9_IGP_C
        | E1000_DEV_ID_ICH9_IGP_M
        | E1000_DEV_ID_ICH9_IGP_M_AMT
        | E1000_DEV_ID_ICH9_IGP_M_V
        | E1000_DEV_ID_ICH10_R_BM_LF
        | E1000_DEV_ID_ICH10_R_BM_LM
        | E1000_DEV_ID_ICH10_R_BM_V => hw.mac_type = em_ich9lan,
        E1000_DEV_ID_ICH10_D_BM_LF | E1000_DEV_ID_ICH10_D_BM_LM | E1000_DEV_ID_ICH10_D_BM_V => {
            hw.mac_type = em_ich10lan
        }
        E1000_DEV_ID_PCH_M_HV_LC
        | E1000_DEV_ID_PCH_M_HV_LM
        | E1000_DEV_ID_PCH_D_HV_DC
        | E1000_DEV_ID_PCH_D_HV_DM => {
            hw.mac_type = em_pchlan;
            hw.eee_enable = true;
        }
        E1000_DEV_ID_PCH2_LV_LM | E1000_DEV_ID_PCH2_LV_V => hw.mac_type = em_pch2lan,
        E1000_DEV_ID_PCH_LPT_I217_LM
        | E1000_DEV_ID_PCH_LPT_I217_V
        | E1000_DEV_ID_PCH_LPTLP_I218_LM
        | E1000_DEV_ID_PCH_LPTLP_I218_V
        | E1000_DEV_ID_PCH_I218_LM2
        | E1000_DEV_ID_PCH_I218_V2
        | E1000_DEV_ID_PCH_I218_LM3
        | E1000_DEV_ID_PCH_I218_V3 => hw.mac_type = em_pch_lpt,
        E1000_DEV_ID_PCH_SPT_I219_LM
        | E1000_DEV_ID_PCH_SPT_I219_V
        | E1000_DEV_ID_PCH_SPT_I219_LM2
        | E1000_DEV_ID_PCH_SPT_I219_V2
        | E1000_DEV_ID_PCH_LBG_I219_LM3
        | E1000_DEV_ID_PCH_SPT_I219_LM4
        | E1000_DEV_ID_PCH_SPT_I219_V4
        | E1000_DEV_ID_PCH_SPT_I219_LM5
        | E1000_DEV_ID_PCH_SPT_I219_V5
        | E1000_DEV_ID_PCH_CMP_I219_LM12
        | E1000_DEV_ID_PCH_CMP_I219_V12 => hw.mac_type = em_pch_spt,
        E1000_DEV_ID_PCH_CNP_I219_LM6
        | E1000_DEV_ID_PCH_CNP_I219_V6
        | E1000_DEV_ID_PCH_CNP_I219_LM7
        | E1000_DEV_ID_PCH_CNP_I219_V7
        | E1000_DEV_ID_PCH_ICP_I219_LM8
        | E1000_DEV_ID_PCH_ICP_I219_V8
        | E1000_DEV_ID_PCH_ICP_I219_LM9
        | E1000_DEV_ID_PCH_ICP_I219_V9
        | E1000_DEV_ID_PCH_CMP_I219_LM10
        | E1000_DEV_ID_PCH_CMP_I219_V10
        | E1000_DEV_ID_PCH_CMP_I219_LM11
        | E1000_DEV_ID_PCH_CMP_I219_V11 => hw.mac_type = em_pch_cnp,
        E1000_DEV_ID_PCH_TGP_I219_LM13
        | E1000_DEV_ID_PCH_TGP_I219_V13
        | E1000_DEV_ID_PCH_TGP_I219_LM14
        | E1000_DEV_ID_PCH_TGP_I219_V14
        | E1000_DEV_ID_PCH_TGP_I219_LM15
        | E1000_DEV_ID_PCH_TGP_I219_V15 => hw.mac_type = em_pch_tgp,
        E1000_DEV_ID_PCH_ADP_I219_LM16
        | E1000_DEV_ID_PCH_ADP_I219_V16
        | E1000_DEV_ID_PCH_ADP_I219_LM17
        | E1000_DEV_ID_PCH_ADP_I219_V17
        | E1000_DEV_ID_PCH_RPL_I219_LM22
        | E1000_DEV_ID_PCH_RPL_I219_V22
        | E1000_DEV_ID_PCH_RPL_I219_LM23
        | E1000_DEV_ID_PCH_RPL_I219_V23 => hw.mac_type = em_pch_adp,
        E1000_DEV_ID_PCH_MTP_I219_LM18
        | E1000_DEV_ID_PCH_MTP_I219_V18
        | E1000_DEV_ID_PCH_MTP_I219_LM19
        | E1000_DEV_ID_PCH_MTP_I219_V19
        | E1000_DEV_ID_PCH_LNP_I219_LM20
        | E1000_DEV_ID_PCH_LNP_I219_V20
        | E1000_DEV_ID_PCH_LNP_I219_LM21
        | E1000_DEV_ID_PCH_LNP_I219_V21
        | E1000_DEV_ID_PCH_ARL_I219_LM24
        | E1000_DEV_ID_PCH_ARL_I219_V24 => hw.mac_type = em_pch_mtp,
        E1000_DEV_ID_PCH_PTP_I219_LM25
        | E1000_DEV_ID_PCH_PTP_I219_V25
        | E1000_DEV_ID_PCH_WCL_I219_LM27
        | E1000_DEV_ID_PCH_WCL_I219_V27 => hw.mac_type = em_pch_ptp,
        E1000_DEV_ID_EP80579_LAN_1 => {
            hw.mac_type = em_icp_xxxx;
            hw.icp_xxxx_port_num = 0;
        }
        E1000_DEV_ID_EP80579_LAN_2 | E1000_DEV_ID_EP80579_LAN_4 => {
            hw.mac_type = em_icp_xxxx;
            hw.icp_xxxx_port_num = 1;
        }
        E1000_DEV_ID_EP80579_LAN_3 | E1000_DEV_ID_EP80579_LAN_5 => {
            hw.mac_type = em_icp_xxxx;
            hw.icp_xxxx_port_num = 2;
        }
        E1000_DEV_ID_EP80579_LAN_6 => {
            hw.mac_type = em_icp_xxxx;
            hw.icp_xxxx_port_num = 3;
        }
        // Should never have loaded on this device.
        _ => return Err(-E1000_ERR_MAC_TYPE),
    }

    if matches!(
        hw.mac_type,
        em_ich8lan
            | em_ich9lan
            | em_ich10lan
            | em_pchlan
            | em_pch2lan
            | em_pch_lpt
            | em_pch_spt
            | em_pch_cnp
            | em_pch_tgp
            | em_pch_adp
            | em_pch_mtp
            | em_pch_ptp
    ) {
        hw.swfwhw_semaphore_present = 1;
        hw.asf_firmware_present = 1;
    }
    // The C's switch falls through from each group into the next.
    let swfw = matches!(
        hw.mac_type,
        em_80003es2lan | em_82575 | em_82576 | em_82580 | em_i210 | em_i350
    );
    let eeprom_sem = swfw || matches!(hw.mac_type, em_82571 | em_82572 | em_82573 | em_82574);
    let asf = eeprom_sem
        || matches!(
            hw.mac_type,
            em_82541 | em_82547 | em_82541_rev_2 | em_82547_rev_2
        );
    if swfw {
        hw.swfw_sync_present = 1;
    }
    if eeprom_sem {
        hw.eeprom_semaphore_present = 1;
    }
    if asf {
        hw.asf_firmware_present = 1;
    }

    Ok(())
}

/// `em_set_sfp_media_type_82575`: derives the media type from the compatibility flags of the
/// SFP module's ID EEPROM.
fn em_set_sfp_media_type_82575(hw: &mut EmHw) -> Result<(), i32> {
    let mut eth_flags = SfpE1000Flags::default();
    let mut transceiver_type: u8 = 0;
    let mut timeout = 3;

    // Turn I2C interface ON and power on sfp cage.
    let mut ctrl_ext = e1000_read_reg(hw, E1000_CTRL_EXT);
    ctrl_ext &= !E1000_CTRL_EXT_SDP3_DATA;
    e1000_write_reg(hw, E1000_CTRL_EXT, ctrl_ext | E1000_CTRL_I2C_ENA);

    e1000_write_flush(hw);

    let ret_val = 'out: {
        // Read SFP module data.
        let mut ret_val = Err(E1000_ERR_CONFIG);
        while timeout != 0 {
            ret_val = em_read_sfp_data_byte(
                hw,
                e1000_i2ccmd_sfp_data_addr(E1000_SFF_IDENTIFIER_OFFSET),
                &mut transceiver_type,
            );
            if ret_val.is_ok() {
                break;
            }
            msec_delay(100);
            timeout -= 1;
        }
        if ret_val.is_err() {
            break 'out ret_val;
        }

        let r = em_read_sfp_data_byte(
            hw,
            e1000_i2ccmd_sfp_data_addr(E1000_SFF_ETH_FLAGS_OFFSET),
            &mut eth_flags.regval,
        );
        if r.is_err() {
            break 'out r;
        }

        // Check if there is some SFP module plugged and powered.
        if transceiver_type == E1000_SFF_IDENTIFIER_SFP
            || transceiver_type == E1000_SFF_IDENTIFIER_SFF
        {
            if eth_flags.e1000_base_lx() != 0 || eth_flags.e1000_base_sx() != 0 {
                hw.media_type = em_media_type_internal_serdes;
            } else if eth_flags.e100_base_fx() != 0 || eth_flags.e100_base_lx() != 0 {
                hw.media_type = em_media_type_internal_serdes;
                hw.sgmii_active = true;
            } else if eth_flags.e1000_base_t() != 0 {
                hw.media_type = em_media_type_copper;
                hw.sgmii_active = true;
            } else {
                // PHY module has not been recognized.
                break 'out Err(E1000_ERR_CONFIG);
            }
        } else {
            break 'out Err(E1000_ERR_CONFIG);
        }
        Ok(())
    };
    // Restore I2C interface setting.
    e1000_write_reg(hw, E1000_CTRL_EXT, ctrl_ext);
    ret_val
}

/// `em_set_media_type`: sets the media type and TBI compatibility.
pub fn em_set_media_type(hw: &mut EmHw) {
    if hw.mac_type != em_82543 {
        // tbi_compatibility is only valid on 82543.
        hw.tbi_compatibility_en = false;
    }

    if matches!(
        hw.mac_type,
        em_82575 | em_82580 | em_82576 | em_i210 | em_i350
    ) {
        hw.media_type = em_media_type_copper;
        hw.sgmii_active = false;

        let mut ctrl_ext = e1000_read_reg(hw, E1000_CTRL_EXT);
        let mode = ctrl_ext & E1000_CTRL_EXT_LINK_MODE_MASK;
        // The SGMII case falls through to the SerDes one without an external MDIO.
        let mut serdes = false;
        if mode == E1000_CTRL_EXT_LINK_MODE_1000BASE_KX {
            hw.media_type = em_media_type_internal_serdes;
            ctrl_ext |= E1000_CTRL_I2C_ENA;
        } else if mode == E1000_CTRL_EXT_LINK_MODE_SGMII {
            let mdic = em_read_reg(hw, E1000_MDICNFG);
            ctrl_ext |= E1000_CTRL_I2C_ENA;
            if mdic & E1000_MDICNFG_EXT_MDIO != 0 {
                hw.media_type = em_media_type_copper;
                hw.sgmii_active = true;
            } else {
                serdes = true;
            }
        } else if mode == E1000_CTRL_EXT_LINK_MODE_PCIE_SERDES {
            serdes = true;
        } else {
            ctrl_ext &= !E1000_CTRL_I2C_ENA;
        }
        if serdes {
            ctrl_ext |= E1000_CTRL_I2C_ENA;
            if em_set_sfp_media_type_82575(hw).is_err() {
                hw.media_type = em_media_type_internal_serdes;
                if (ctrl_ext & E1000_CTRL_EXT_LINK_MODE_MASK) == E1000_CTRL_EXT_LINK_MODE_SGMII {
                    hw.media_type = em_media_type_copper;
                    hw.sgmii_active = true;
                }
            }

            ctrl_ext &= !E1000_CTRL_EXT_LINK_MODE_MASK;
            if hw.sgmii_active {
                ctrl_ext |= E1000_CTRL_EXT_LINK_MODE_SGMII;
            } else {
                ctrl_ext |= E1000_CTRL_EXT_LINK_MODE_PCIE_SERDES;
            }
        }
        e1000_write_reg(hw, E1000_CTRL_EXT, ctrl_ext);
        return;
    }

    match hw.device_id {
        E1000_DEV_ID_82545GM_SERDES
        | E1000_DEV_ID_82546GB_SERDES
        | E1000_DEV_ID_82571EB_SERDES
        | E1000_DEV_ID_82571EB_SERDES_DUAL
        | E1000_DEV_ID_82571EB_SERDES_QUAD
        | E1000_DEV_ID_82572EI_SERDES
        | E1000_DEV_ID_80003ES2LAN_SERDES_DPT => hw.media_type = em_media_type_internal_serdes,
        E1000_DEV_ID_EP80579_LAN_1
        | E1000_DEV_ID_EP80579_LAN_2
        | E1000_DEV_ID_EP80579_LAN_3
        | E1000_DEV_ID_EP80579_LAN_4
        | E1000_DEV_ID_EP80579_LAN_5
        | E1000_DEV_ID_EP80579_LAN_6 => hw.media_type = em_media_type_copper,
        _ => match hw.mac_type {
            em_82542_rev2_0 | em_82542_rev2_1 => hw.media_type = em_media_type_fiber,
            em_ich8lan | em_ich9lan | em_ich10lan | em_pchlan | em_pch2lan | em_pch_lpt
            | em_pch_spt | em_pch_cnp | em_pch_tgp | em_pch_adp | em_pch_mtp | em_pch_ptp
            | em_82573 | em_82574 => {
                // The STATUS_TBIMODE bit is reserved or reused for this device.
                hw.media_type = em_media_type_copper;
            }
            _ => {
                let status = e1000_read_reg(hw, E1000_STATUS);
                if status & E1000_STATUS_TBIMODE != 0 {
                    hw.media_type = em_media_type_fiber;
                    // tbi_compatibility not valid on fiber.
                    hw.tbi_compatibility_en = false;
                } else {
                    hw.media_type = em_media_type_copper;
                }
            }
        },
    }
}

/// `em_reset_hw`: resets the transmit and receive units; masks and clears all interrupts.
pub fn em_reset_hw(hw: &mut EmHw) -> Result<(), i32> {
    // For 82542 (rev 2.0), disable MWI before issuing a device reset.
    if hw.mac_type == em_82542_rev2_0 {
        em_pci_clear_mwi(hw);
    }
    if hw.bus_type == em_bus_type_pci_express {
        // Prevent the PCI-E bus from sticking if there is no TLP connection on the last TLP
        // read/write transaction when MAC is reset. A failure ("PCI-E Master disable
        // polling has failed") is only a debug message.
        let _ = em_disable_pciex_master(hw);
    }

    // Set the completion timeout for 82575 chips (a failure is only a debug message).
    if matches!(
        hw.mac_type,
        em_82575 | em_82580 | em_82576 | em_i210 | em_i350
    ) {
        let _ = em_set_pciex_completion_timeout(hw);
    }

    // Clear interrupt mask to stop board from generating interrupts.
    e1000_write_reg(hw, E1000_IMC, 0xffffffff);
    // Disable the Transmit and Receive units. Then delay to allow any pending transactions
    // to complete before we hit the MAC with the global reset.
    e1000_write_reg(hw, E1000_RCTL, 0);
    e1000_write_reg(hw, E1000_TCTL, E1000_TCTL_PSP);
    e1000_write_flush(hw);
    // The tbi_compatibility_on Flag must be cleared when Rctl is cleared.
    hw.tbi_compatibility_on = false;
    // Delay to allow any outstanding PCI transactions to complete before resetting the
    // device.
    msec_delay(10);

    let mut ctrl = e1000_read_reg(hw, E1000_CTRL);

    // Must reset the PHY before resetting the MAC.
    if hw.mac_type == em_82541 || hw.mac_type == em_82547 {
        e1000_write_reg(hw, E1000_CTRL, ctrl | E1000_CTRL_PHY_RST);
        msec_delay(5);
    }
    // Must acquire the MDIO ownership before MAC reset. Ownership defaults to firmware
    // after a reset.
    if hw.mac_type == em_82573 || hw.mac_type == em_82574 {
        let mut timeout: u32 = 10;

        let mut extcnf_ctrl = e1000_read_reg(hw, E1000_EXTCNF_CTRL);
        extcnf_ctrl |= E1000_EXTCNF_CTRL_MDIO_SW_OWNERSHIP;

        loop {
            e1000_write_reg(hw, E1000_EXTCNF_CTRL, extcnf_ctrl);
            extcnf_ctrl = e1000_read_reg(hw, E1000_EXTCNF_CTRL);

            if extcnf_ctrl & E1000_EXTCNF_CTRL_MDIO_SW_OWNERSHIP != 0 {
                break;
            } else {
                extcnf_ctrl |= E1000_EXTCNF_CTRL_MDIO_SW_OWNERSHIP;
            }

            msec_delay(2);
            timeout -= 1;
            if timeout == 0 {
                break;
            }
        }
    }
    // Workaround for ICH8 bit corruption issue in FIFO memory.
    if hw.mac_type == em_ich8lan {
        // Set Tx and Rx buffer allocation to 8k apiece.
        e1000_write_reg(hw, E1000_PBA, E1000_PBA_8K);
        // Set Packet Buffer Size to 16k.
        e1000_write_reg(hw, E1000_PBS, E1000_PBS_16K);
    }
    // Issue a global reset to the MAC. This will reset the chip's transmit, receive, DMA,
    // and link units. It will not effect the current PCI configuration. The global reset
    // bit is self-clearing, and should clear within a microsecond.
    match hw.mac_type {
        em_82544 | em_82540 | em_82545 | em_82546 | em_82541 | em_82541_rev_2 => {
            // These controllers can't ack the 64-bit write when issuing the reset, so use
            // IO-mapping as a workaround to issue the reset.
            e1000_write_reg_io(hw, E1000_CTRL, ctrl | E1000_CTRL_RST);
        }
        em_82545_rev_3 | em_82546_rev_3 => {
            // Reset is performed on a shadow of the control register.
            e1000_write_reg(hw, E1000_CTRL_DUP, ctrl | E1000_CTRL_RST);
        }
        em_ich8lan | em_ich9lan | em_ich10lan | em_pchlan | em_pch2lan | em_pch_lpt
        | em_pch_spt | em_pch_cnp | em_pch_tgp | em_pch_adp | em_pch_mtp | em_pch_ptp => {
            if !hw.phy_reset_disable && em_check_phy_reset_block(hw).is_ok() {
                // PHY HW reset requires MAC CORE reset at the same time to make sure the
                // interface between MAC and the external PHY is reset.
                ctrl |= E1000_CTRL_PHY_RST;
                // Gate automatic PHY configuration by hardware on non-managed 82579.
                if hw.mac_type == em_pch2lan
                    && e1000_read_reg(hw, E1000_FWSM) & E1000_FWSM_FW_VALID == 0
                {
                    em_gate_hw_phy_config_ich8lan(hw, true);
                }
            }
            let _ = em_get_software_flag(hw);
            e1000_write_reg(hw, E1000_CTRL, ctrl | E1000_CTRL_RST);
            // HW reset releases software_flag.
            hw.sw_flag = 0;
            msec_delay(20);

            // Ungate automatic PHY configuration on non-managed 82579.
            if hw.mac_type == em_pch2lan
                && !hw.phy_reset_disable
                && e1000_read_reg(hw, E1000_FWSM) & E1000_FWSM_FW_VALID == 0
            {
                msec_delay(10);
                em_gate_hw_phy_config_ich8lan(hw, false);
            }
        }
        _ => e1000_write_reg(hw, E1000_CTRL, ctrl | E1000_CTRL_RST),
    }

    if em_check_phy_reset_block(hw).is_ok() {
        if hw.mac_type == em_pchlan {
            em_hv_phy_workarounds_ich8lan(hw)?;
        } else if hw.mac_type == em_pch2lan {
            em_lv_phy_workarounds_ich8lan(hw)?;
        }
    }

    // After MAC reset, force reload of EEPROM to restore power-on settings to device.
    // Later controllers reload the EEPROM automatically, so just wait for reload to
    // complete.
    match hw.mac_type {
        em_82542_rev2_0 | em_82542_rev2_1 | em_82543 | em_82544 => {
            // Wait for reset to complete.
            usec_delay(10);
            let mut ctrl_ext = e1000_read_reg(hw, E1000_CTRL_EXT);
            ctrl_ext |= E1000_CTRL_EXT_EE_RST;
            e1000_write_reg(hw, E1000_CTRL_EXT, ctrl_ext);
            e1000_write_flush(hw);
            // Wait for EEPROM reload.
            msec_delay(2);
        }
        em_82541 | em_82541_rev_2 | em_82547 | em_82547_rev_2 => {
            // Wait for EEPROM reload.
            msec_delay(20);
        }
        em_82573 | em_82574 => {
            if !em_is_onboard_nvm_eeprom(hw) {
                usec_delay(10);
                let mut ctrl_ext = e1000_read_reg(hw, E1000_CTRL_EXT);
                ctrl_ext |= E1000_CTRL_EXT_EE_RST;
                e1000_write_reg(hw, E1000_CTRL_EXT, ctrl_ext);
                e1000_write_flush(hw);
            }

            // Auto read done will delay 5ms or poll based on mac type.
            em_get_auto_rd_done(hw)?;
        }
        _ => {
            // Wait for EEPROM reload (it happens automatically).
            msec_delay(5);
        }
    }

    // Disable HW ARPs on ASF enabled adapters.
    if hw.mac_type >= em_82540 && hw.mac_type <= em_82547_rev_2 && hw.mac_type != em_icp_xxxx {
        let mut manc = e1000_read_reg(hw, E1000_MANC);
        manc &= !E1000_MANC_ARP_EN;
        e1000_write_reg(hw, E1000_MANC, manc);
    }
    if hw.mac_type == em_82541 || hw.mac_type == em_82547 {
        em_phy_init_script(hw);

        // Configure activity LED after PHY reset.
        let mut led_ctrl = e1000_read_reg(hw, E1000_LEDCTL);
        led_ctrl &= IGP_ACTIVITY_LED_MASK;
        led_ctrl |= IGP_ACTIVITY_LED_ENABLE | IGP_LED3_MODE;
        e1000_write_reg(hw, E1000_LEDCTL, led_ctrl);
    }

    // For PCH, this write will make sure that any noise will be detected as a CRC error
    // and be dropped rather than show up as a bad packet to the DMA engine.
    if hw.mac_type == em_pchlan {
        e1000_write_reg(hw, E1000_CRC_OFFSET, 0x65656565);
    }

    // Clear interrupt mask to stop board from generating interrupts.
    e1000_write_reg(hw, E1000_IMC, 0xffffffff);

    // Clear any pending interrupt events.
    let _icr = e1000_read_reg(hw, E1000_ICR);

    // If MWI was previously enabled, reenable it.
    if hw.mac_type == em_82542_rev2_0 && u32::from(hw.pci_cmd_word) & CMD_MEM_WRT_INVALIDATE != 0 {
        em_pci_set_mwi(hw);
    }
    if is_ich8(hw.mac_type) {
        let mut reg = e1000_read_reg(hw, E1000_KABGTXD);
        reg |= E1000_KABGTXD_BGSQLBIAS;
        e1000_write_reg(hw, E1000_KABGTXD, reg);

        if hw.mac_type >= em_pch_ptp {
            reg = e1000_read_reg(hw, E1000_CTRL_EXT);
            reg &= !E1000_CTRL_EXT_DPG_EN;
            e1000_write_reg(hw, E1000_CTRL_EXT, reg);
        }
    }

    if hw.mac_type == em_82580 || hw.mac_type == em_i350 {
        let mut nvm_data = [0u16; 1];

        // Clear global device reset status bit.
        em_write_reg(hw, E1000_STATUS, E1000_STATUS_DEV_RST_SET);

        let _ = em_read_eeprom(
            hw,
            EEPROM_INIT_CONTROL3_PORT_A + nvm_82580_lan_func_offset(hw.bus_func),
            1,
            &mut nvm_data,
        );

        let mut mdicnfg = em_read_reg(hw, E1000_MDICNFG);
        if nvm_data[0] & NVM_WORD24_EXT_MDIO != 0 {
            mdicnfg |= E1000_MDICNFG_EXT_MDIO;
        }
        if nvm_data[0] & NVM_WORD24_COM_MDIO != 0 {
            mdicnfg |= E1000_MDICNFG_COM_MDIO;
        }
        em_write_reg(hw, E1000_MDICNFG, mdicnfg);
    }

    if hw.mac_type == em_i210 || hw.mac_type == em_i350 {
        let _ = em_set_eee_i350(hw);
    }

    Ok(())
}

/// Initializes a number of hardware-dependent bits. The C takes the softc and walks its
/// queues (`FOREACH_QUEUE`); if_em(4) numbers them from 0 (`que->me`), so the queue count is
/// enough here.
fn em_initialize_hardware_bits(hw: &mut EmHw, num_queues: i32) {
    if hw.mac_type >= em_82571 && !hw.initialize_hw_bits_disable {
        // Settings common to all silicon.
        let mut reg_tarc0 = e1000_read_reg(hw, E1000_TARC0);
        reg_tarc0 &= !0x78000000; // Clear bits 30, 29, 28, and 27.
        for me in 0..num_queues.max(0) as u32 {
            let mut reg_txdctl = e1000_read_reg(hw, e1000_txdctl(me));
            reg_txdctl |= E1000_TXDCTL_COUNT_DESC; // Set bit 22.
            e1000_write_reg(hw, e1000_txdctl(me), reg_txdctl);
        }

        // Old code always initialized queue 1, even when unused, keep behaviour.
        if num_queues == 1 {
            let mut reg_txdctl = e1000_read_reg(hw, e1000_txdctl(1));
            reg_txdctl |= E1000_TXDCTL_COUNT_DESC;
            e1000_write_reg(hw, e1000_txdctl(1), reg_txdctl);
        }

        match hw.mac_type {
            em_82571 | em_82572 => {
                let mut reg_tarc1 = e1000_read_reg(hw, E1000_TARC1);
                let reg_tctl = e1000_read_reg(hw, E1000_TCTL);

                // Set the phy Tx compatible mode bits.
                reg_tarc1 &= !0x60000000; // Clear bits 30 and 29.

                reg_tarc0 |= 0x07800000; // Set TARC0 bits 23-26.
                reg_tarc1 |= 0x07000000; // Set TARC1 bits 24-26.

                if reg_tctl & E1000_TCTL_MULR != 0 {
                    reg_tarc1 &= !0x10000000; // Clear bit 28 if MULR is 1b.
                } else {
                    reg_tarc1 |= 0x10000000; // Set bit 28 if MULR is 0b.
                }

                e1000_write_reg(hw, E1000_TARC1, reg_tarc1);
            }
            em_82573 | em_82574 => {
                let mut reg_ctrl_ext = e1000_read_reg(hw, E1000_CTRL_EXT);
                let mut reg_ctrl = e1000_read_reg(hw, E1000_CTRL);

                reg_ctrl_ext &= !0x00800000; // Clear bit 23.
                reg_ctrl_ext |= 0x00400000; // Set bit 22.
                reg_ctrl &= !0x20000000; // Clear bit 29.

                e1000_write_reg(hw, E1000_CTRL_EXT, reg_ctrl_ext);
                e1000_write_reg(hw, E1000_CTRL, reg_ctrl);
            }
            em_80003es2lan => {
                if hw.media_type == em_media_type_fiber
                    || hw.media_type == em_media_type_internal_serdes
                {
                    reg_tarc0 &= !0x00100000; // Clear bit 20.
                }
                let reg_tctl = e1000_read_reg(hw, E1000_TCTL);
                let mut reg_tarc1 = e1000_read_reg(hw, E1000_TARC1);
                if reg_tctl & E1000_TCTL_MULR != 0 {
                    reg_tarc1 &= !0x10000000; // Clear bit 28 if MULR is 1b.
                } else {
                    reg_tarc1 |= 0x10000000; // Set bit 28 if MULR is 0b.
                }

                e1000_write_reg(hw, E1000_TARC1, reg_tarc1);
            }
            em_ich8lan | em_ich9lan | em_ich10lan | em_pchlan | em_pch2lan | em_pch_lpt
            | em_pch_spt | em_pch_cnp | em_pch_tgp | em_pch_adp | em_pch_mtp | em_pch_ptp => {
                if hw.mac_type == em_ich8lan {
                    reg_tarc0 |= 0x30000000; // Set TARC0 bits 29 and 28.
                }

                let mut reg_ctrl_ext = e1000_read_reg(hw, E1000_CTRL_EXT);
                reg_ctrl_ext |= 0x00400000; // Set bit 22.
                // Enable PHY low-power state when MAC is at D3 w/o WoL.
                if hw.mac_type >= em_pchlan {
                    reg_ctrl_ext |= E1000_CTRL_EXT_PHYPDEN;
                }
                e1000_write_reg(hw, E1000_CTRL_EXT, reg_ctrl_ext);

                reg_tarc0 |= 0x0d800000; // Set TARC0 bits 23, 24, 26, 27.

                let mut reg_tarc1 = e1000_read_reg(hw, E1000_TARC1);
                let reg_tctl = e1000_read_reg(hw, E1000_TCTL);

                if reg_tctl & E1000_TCTL_MULR != 0 {
                    reg_tarc1 &= !0x10000000; // Clear bit 28 if MULR is 1b.
                } else {
                    reg_tarc1 |= 0x10000000; // Set bit 28 if MULR is 0b.
                }

                reg_tarc1 |= 0x45000000; // Set bit 24, 26 and 30.

                e1000_write_reg(hw, E1000_TARC1, reg_tarc1);
            }
            _ => {}
        }

        e1000_write_reg(hw, E1000_TARC0, reg_tarc0);
    }
}

/// `em_toggle_lanphypc_pch_lpt`: toggles the LANPHYPC pin value, which fully power-cycles
/// the PHY and is used to reset the PHY to a quiescent state when necessary.
fn em_toggle_lanphypc_pch_lpt(hw: &EmHw) {
    // Set Phy Config Counter to 50msec.
    let mut mac_reg = e1000_read_reg(hw, E1000_FEXTNVM3);
    mac_reg &= !E1000_FEXTNVM3_PHY_CFG_COUNTER_MASK;
    mac_reg |= E1000_FEXTNVM3_PHY_CFG_COUNTER_50MSEC;
    e1000_write_reg(hw, E1000_FEXTNVM3, mac_reg);

    // Toggle LANPHYPC Value bit.
    mac_reg = e1000_read_reg(hw, E1000_CTRL);
    mac_reg |= E1000_CTRL_LANPHYPC_OVERRIDE;
    mac_reg &= !E1000_CTRL_LANPHYPC_VALUE;
    e1000_write_reg(hw, E1000_CTRL, mac_reg);
    e1000_write_flush(hw);
    msec_delay(1);
    mac_reg &= !E1000_CTRL_LANPHYPC_OVERRIDE;
    e1000_write_reg(hw, E1000_CTRL, mac_reg);
    e1000_write_flush(hw);

    if hw.mac_type < em_pch_lpt {
        msec_delay(50);
    } else {
        let mut count: u16 = 20;

        loop {
            msec_delay(5);
            // The C's `count--` in the condition: tested, then decremented.
            if e1000_read_reg(hw, E1000_CTRL_EXT) & E1000_CTRL_EXT_LPCD != 0 || count == 0 {
                break;
            }
            count -= 1;
        }

        msec_delay(30);
    }
}

/// `em_disable_ulp_lpt_lp`: unconfigures Ultra Low Power mode for LynxPoint-LP.
///
/// Un-configures ULP mode when link is up, the system is transitioned from Sx or the driver
/// is unloaded. On a Manageability Engine (ME) enabled system, polls for an indication from
/// ME that ULP has been un-configured; otherwise un-configures the ULP mode by software.
/// During nominal operation it is called when link is acquired (`force` false); when
/// unloading the driver or during Sx->S0 transitions, with `force` true.
fn em_disable_ulp_lpt_lp(hw: &mut EmHw, force: bool) -> Result<(), i32> {
    let mut phy_reg: u16 = 0;
    let mut i = 0;

    if hw.mac_type < em_pch_lpt
        || hw.device_id == E1000_DEV_ID_PCH_LPT_I217_LM
        || hw.device_id == E1000_DEV_ID_PCH_LPT_I217_V
        || hw.device_id == E1000_DEV_ID_PCH_I218_LM2
        || hw.device_id == E1000_DEV_ID_PCH_I218_V2
    {
        return Ok(());
    }

    if e1000_read_reg(hw, E1000_FWSM) & E1000_FWSM_FW_VALID != 0 {
        if force {
            // Request ME un-configure ULP mode in the PHY.
            let mut mac_reg = e1000_read_reg(hw, E1000_H2ME);
            mac_reg &= !E1000_H2ME_ULP;
            mac_reg |= E1000_H2ME_ENFORCE_SETTINGS;
            e1000_write_reg(hw, E1000_H2ME, mac_reg);
        }

        // Poll up to 300msec for ME to clear ULP_CFG_DONE.
        while e1000_read_reg(hw, E1000_FWSM) & E1000_FWSM_ULP_CFG_DONE != 0 {
            let at = i;
            i += 1;
            if at == 30 {
                return Err(-E1000_ERR_PHY);
            }

            msec_delay(10);
        }

        if force {
            let mut mac_reg = e1000_read_reg(hw, E1000_H2ME);
            mac_reg &= !E1000_H2ME_ENFORCE_SETTINGS;
            e1000_write_reg(hw, E1000_H2ME, mac_reg);
        } else {
            // Clear H2ME.ULP after ME ULP configuration.
            let mut mac_reg = e1000_read_reg(hw, E1000_H2ME);
            mac_reg &= !E1000_H2ME_ULP;
            e1000_write_reg(hw, E1000_H2ME, mac_reg);
        }

        return Ok(());
    }

    em_get_software_flag(hw)?;

    if force {
        // Toggle LANPHYPC Value bit.
        em_toggle_lanphypc_pch_lpt(hw);
    }

    let ret_val: Result<(), i32> = 'release: {
        // Unforce SMBus mode in PHY.
        if em_read_phy_reg(hw, CV_SMB_CTRL, &mut phy_reg).is_err() {
            // The MAC might be in PCIe mode, so temporarily force to SMBus mode in order to
            // access the PHY.
            let mut mac_reg = e1000_read_reg(hw, E1000_CTRL_EXT);
            mac_reg |= E1000_CTRL_EXT_FORCE_SMBUS;
            e1000_write_reg(hw, E1000_CTRL_EXT, mac_reg);

            msec_delay(50);

            let r = em_read_phy_reg(hw, CV_SMB_CTRL, &mut phy_reg);
            if r.is_err() {
                break 'release r;
            }
        }
        phy_reg &= !CV_SMB_CTRL_FORCE_SMBUS;
        let _ = em_write_phy_reg(hw, CV_SMB_CTRL, phy_reg);

        // Unforce SMBus mode in MAC.
        let mut mac_reg = e1000_read_reg(hw, E1000_CTRL_EXT);
        mac_reg &= !E1000_CTRL_EXT_FORCE_SMBUS;
        e1000_write_reg(hw, E1000_CTRL_EXT, mac_reg);

        // When ULP mode was previously entered, K1 was disabled by the hardware. Re-Enable
        // K1 in the PHY when exiting ULP.
        let r = em_read_phy_reg(hw, HV_PM_CTRL, &mut phy_reg);
        if r.is_err() {
            break 'release r;
        }
        phy_reg |= HV_PM_CTRL_K1_ENABLE;
        let _ = em_write_phy_reg(hw, HV_PM_CTRL, phy_reg);

        // Clear ULP enabled configuration.
        let r = em_read_phy_reg(hw, I218_ULP_CONFIG1, &mut phy_reg);
        if r.is_err() {
            break 'release r;
        }
        phy_reg &= !(I218_ULP_CONFIG1_IND
            | I218_ULP_CONFIG1_STICKY_ULP
            | I218_ULP_CONFIG1_RESET_TO_SMBUS
            | I218_ULP_CONFIG1_WOL_HOST
            | I218_ULP_CONFIG1_INBAND_EXIT
            | I218_ULP_CONFIG1_EN_ULP_LANPHYPC
            | I218_ULP_CONFIG1_DIS_CLR_STICKY_ON_PERST
            | I218_ULP_CONFIG1_DISABLE_SMB_PERST);
        let _ = em_write_phy_reg(hw, I218_ULP_CONFIG1, phy_reg);

        // Commit ULP changes by starting auto ULP configuration.
        phy_reg |= I218_ULP_CONFIG1_START;
        let _ = em_write_phy_reg(hw, I218_ULP_CONFIG1, phy_reg);

        // Clear Disable SMBus Release on PERST# in MAC.
        let mut mac_reg = e1000_read_reg(hw, E1000_FEXTNVM7);
        mac_reg &= !E1000_FEXTNVM7_DISABLE_SMB_PERST;
        e1000_write_reg(hw, E1000_FEXTNVM7, mac_reg);
        Ok(())
    };

    em_release_software_flag(hw);
    if force {
        let _ = em_phy_reset(hw);
        msec_delay(50);
    }
    ret_val
}

/// `em_reconfigure_k1_exit_timeout`: reconfigures the K1 exit timeout as a workaround to
/// the PHY synchronization issue on MTL, LNL, PTL and WCL.
pub fn em_reconfigure_k1_exit_timeout(hw: &mut EmHw) -> Result<(), i32> {
    let mut phy_timeout: u16 = 0;

    if hw.mac_type < em_pch_mtp {
        return Ok(());
    }

    let mut fextnvm12 = e1000_read_reg(hw, E1000_FEXTNVM12);
    fextnvm12 &= !E1000_FEXTNVM12_PHYPD_CTRL_MASK;
    fextnvm12 |= E1000_FEXTNVM12_PHYPD_CTRL_P1;
    e1000_write_reg(hw, E1000_FEXTNVM12, fextnvm12);

    delay(1000);

    em_read_phy_reg(hw, E1000_PHY_TIMEOUTS_REG, &mut phy_timeout)?;
    phy_timeout &= !E1000_PHY_TIMEOUTS_K1_EXIT_TO_MASK;
    phy_timeout |= 0xF00;

    em_write_phy_reg(hw, E1000_PHY_TIMEOUTS_REG, phy_timeout)
}

/// `em_init_hw`: performs basic configuration of the adapter.
///
/// Assumes that the controller has previously been reset and is in a post-reset
/// uninitialized state. Initializes the receive address registers, multicast table, and
/// VLAN filter table. Calls routines to setup link configuration and flow control settings.
/// Clears all on-chip counters. Leaves the transmit and receive units disabled and
/// uninitialized. The C takes the softc for `sc->hw` and its queues; if_em(4) numbers its
/// queues from 0, so `num_queues` (`sc->num_queues`) stands for them. The result is
/// `em_setup_link`'s, as in the C, which goes on after a link setup failure.
pub fn em_init_hw(hw: &mut EmHw, num_queues: i32) -> Result<(), i32> {
    // Force full DMA clock frequency for ICH8.
    if hw.mac_type == em_ich8lan {
        let mut reg_data = e1000_read_reg(hw, E1000_STATUS);
        reg_data &= !0x80000000;
        e1000_write_reg(hw, E1000_STATUS, reg_data);
    }

    if hw.mac_type >= em_pchlan {
        // The MAC-PHY interconnect may still be in SMBus mode after Sx->S0. Toggle the
        // LANPHYPC Value bit to force the interconnect to PCIe mode, but only if there is no
        // firmware present otherwise firmware will have done it.
        let fwsm = e1000_read_reg(hw, E1000_FWSM);
        if fwsm & E1000_FWSM_FW_VALID == 0 {
            let mut ctrl = e1000_read_reg(hw, E1000_CTRL);
            ctrl |= E1000_CTRL_LANPHYPC_OVERRIDE;
            ctrl &= !E1000_CTRL_LANPHYPC_VALUE;
            e1000_write_reg(hw, E1000_CTRL, ctrl);
            usec_delay(10);
            ctrl &= !E1000_CTRL_LANPHYPC_OVERRIDE;
            e1000_write_reg(hw, E1000_CTRL, ctrl);
            msec_delay(50);
        }

        // Gate automatic PHY configuration on non-managed 82579.
        if hw.mac_type == em_pch2lan {
            em_gate_hw_phy_config_ich8lan(hw, true);
        }

        let _ = em_disable_ulp_lpt_lp(hw, true);
        // Reset the PHY before any access to it. Doing so, ensures that the PHY is in a
        // known good state before we read/write PHY registers. The generic reset is
        // sufficient here, because we haven't determined the PHY type yet.
        let _ = em_phy_reset(hw);

        // Ungate automatic PHY configuration on non-managed 82579.
        if hw.mac_type == em_pch2lan && fwsm & E1000_FWSM_FW_VALID == 0 {
            em_gate_hw_phy_config_ich8lan(hw, false);
        }

        // Set MDIO slow mode before any other MDIO access.
        em_set_mdio_slow_mode_hv(hw)?;
    }

    // Initialize Identification LED.
    em_id_led_init(hw)?;
    // Set the media type and TBI compatibility.
    em_set_media_type(hw);

    // Magic delay that improves problems with i219LM on HP Elitebook.
    msec_delay(1);
    // Must be called after em_set_media_type because media_type is used.
    em_initialize_hardware_bits(hw, num_queues);

    // Disabling VLAN filtering.
    // VET hardcoded to standard value and VFTA removed in ICH8/ICH9 LAN.
    if !is_ich8(hw.mac_type) {
        if hw.mac_type < em_82545_rev_3 {
            e1000_write_reg(hw, E1000_VET, 0);
        }
        if hw.mac_type == em_i350 {
            em_clear_vfta_i350(hw);
        } else {
            em_clear_vfta(hw);
        }
    }
    // For 82542 (rev 2.0), disable MWI and put the receiver into reset.
    if hw.mac_type == em_82542_rev2_0 {
        em_pci_clear_mwi(hw);
        e1000_write_reg(hw, E1000_RCTL, E1000_RCTL_RST);
        e1000_write_flush(hw);
        msec_delay(5);
    }
    // Setup the receive address. This involves initializing all of the Receive Address
    // Registers (RARs 0 - 15).
    em_init_rx_addrs(hw);

    // For 82542 (rev 2.0), take the receiver out of reset and enable MWI.
    if hw.mac_type == em_82542_rev2_0 {
        e1000_write_reg(hw, E1000_RCTL, 0);
        e1000_write_flush(hw);
        msec_delay(1);
        if u32::from(hw.pci_cmd_word) & CMD_MEM_WRT_INVALIDATE != 0 {
            em_pci_set_mwi(hw);
        }
    }
    // Zero out the Multicast HASH table.
    let mut mta_size = E1000_MC_TBL_SIZE;
    if is_ich8(hw.mac_type) {
        mta_size = E1000_MC_TBL_SIZE_ICH8LAN;
    }
    for i in 0..mta_size {
        e1000_write_reg_array(hw, E1000_MTA, i, 0);
        // Use write flush to prevent Memory Write Block (MWB) from occurring when accessing
        // our register space.
        e1000_write_flush(hw);
    }
    // Set the PCI priority bit correctly in the CTRL register. This determines if the
    // adapter gives priority to receives, or if it gives equal priority to transmits and
    // receives. Valid only on 82542 and 82543 silicon.
    if hw.dma_fairness != 0 && hw.mac_type <= em_82543 {
        let ctrl = e1000_read_reg(hw, E1000_CTRL);
        e1000_write_reg(hw, E1000_CTRL, ctrl | E1000_CTRL_PRIOR);
    }
    match hw.mac_type {
        em_82545_rev_3 | em_82546_rev_3 => {}
        _ => {
            // Workaround for PCI-X problem when BIOS sets MMRBC incorrectly.
            if hw.bus_type == em_bus_type_pcix {
                let mut pcix_cmd_word: u16 = 0;
                let mut pcix_stat_hi_word: u16 = 0;
                em_read_pci_cfg(hw, PCIX_COMMAND_REGISTER, &mut pcix_cmd_word);
                em_read_pci_cfg(hw, PCIX_STATUS_REGISTER_HI, &mut pcix_stat_hi_word);
                let cmd_mmrbc =
                    (pcix_cmd_word & PCIX_COMMAND_MMRBC_MASK) >> PCIX_COMMAND_MMRBC_SHIFT;
                let mut stat_mmrbc =
                    (pcix_stat_hi_word & PCIX_STATUS_HI_MMRBC_MASK) >> PCIX_STATUS_HI_MMRBC_SHIFT;

                if stat_mmrbc == PCIX_STATUS_HI_MMRBC_4K {
                    stat_mmrbc = PCIX_STATUS_HI_MMRBC_2K;
                }
                if cmd_mmrbc > stat_mmrbc {
                    pcix_cmd_word &= !PCIX_COMMAND_MMRBC_MASK;
                    pcix_cmd_word |= stat_mmrbc << PCIX_COMMAND_MMRBC_SHIFT;
                    em_write_pci_cfg(hw, PCIX_COMMAND_REGISTER, &pcix_cmd_word);
                }
            }
        }
    }

    // More time needed for PHY to initialize.
    if is_ich8(hw.mac_type) {
        msec_delay(15);
    }

    // The 82578 Rx buffer will stall if wakeup is enabled in host and the ME. Reading the
    // BM_WUC register will clear the host wakeup bit. Reset the phy after disabling host
    // wakeup to reset the Rx buffer.
    if hw.phy_type == em_phy_82578 {
        let mut wuc: u16 = 0;
        let _ = em_read_phy_reg(hw, phy_reg(BM_WUC_PAGE, 1), &mut wuc);
        em_phy_reset(hw)?;
    }

    // Call a subroutine to configure the link and setup flow control.
    let ret_val = em_setup_link(hw);

    // Set the transmit descriptor write-back policy.
    if hw.mac_type > em_82544 {
        for me in 0..num_queues.max(0) as u32 {
            let mut ctrl = e1000_read_reg(hw, e1000_txdctl(me));
            ctrl = (ctrl & !E1000_TXDCTL_WTHRESH) | E1000_TXDCTL_FULL_TX_DESC_WB;
            e1000_write_reg(hw, e1000_txdctl(me), ctrl);
        }
    }
    if hw.mac_type == em_82573 || hw.mac_type == em_82574 {
        em_enable_tx_pkt_filtering(hw);
    }
    // The 80003ES2LAN case falls through into the queue 1 write-back of the others.
    if hw.mac_type == em_80003es2lan {
        // Enable retransmit on late collisions.
        let mut reg_data = e1000_read_reg(hw, E1000_TCTL);
        reg_data |= E1000_TCTL_RTLC;
        e1000_write_reg(hw, E1000_TCTL, reg_data);

        // Configure Gigabit Carry Extend Padding.
        reg_data = e1000_read_reg(hw, E1000_TCTL_EXT);
        reg_data &= !E1000_TCTL_EXT_GCEX_MASK;
        reg_data |= DEFAULT_80003ES2LAN_TCTL_EXT_GCEX;
        e1000_write_reg(hw, E1000_TCTL_EXT, reg_data);

        // Configure Transmit Inter-Packet Gap.
        reg_data = e1000_read_reg(hw, E1000_TIPG);
        reg_data &= !E1000_TIPG_IPGT_MASK;
        reg_data |= DEFAULT_80003ES2LAN_TIPG_IPGT_1000;
        e1000_write_reg(hw, E1000_TIPG, reg_data);

        reg_data = e1000_read_reg_array(hw, E1000_FFLT, 0x0001);
        reg_data &= !0x00100000;
        e1000_write_reg_array(hw, E1000_FFLT, 0x0001, reg_data);
    }
    if matches!(
        hw.mac_type,
        em_80003es2lan
            | em_82571
            | em_82572
            | em_82575
            | em_82576
            | em_82580
            | em_i210
            | em_i350
            | em_ich8lan
            | em_ich9lan
            | em_ich10lan
            | em_pchlan
            | em_pch2lan
            | em_pch_lpt
            | em_pch_spt
            | em_pch_cnp
            | em_pch_tgp
            | em_pch_adp
            | em_pch_mtp
            | em_pch_ptp
    ) {
        // Old code always initialized queue 1, even when unused, keep behaviour.
        if num_queues == 1 {
            let mut ctrl = e1000_read_reg(hw, e1000_txdctl(1));
            ctrl = (ctrl & !E1000_TXDCTL_WTHRESH) | E1000_TXDCTL_FULL_TX_DESC_WB;
            e1000_write_reg(hw, e1000_txdctl(1), ctrl);
        }
    }

    if hw.mac_type == em_82573 || hw.mac_type == em_82574 {
        let mut gcr = e1000_read_reg(hw, E1000_GCR);
        gcr |= E1000_GCR_L1_ACT_WITHOUT_L0S_RX;
        e1000_write_reg(hw, E1000_GCR, gcr);
    }
    // Clear all of the statistics registers (clear on read). It is important that we do
    // this after we have tried to establish link because the symbol error count will
    // increment wildly if there is no link.
    em_clear_hw_cntrs(hw);
    // ICH8 No-snoop bits are opposite polarity. Set to snoop by default after reset.
    if is_ich8(hw.mac_type) {
        let snoop = if hw.mac_type == em_ich8lan {
            PCI_EX_82566_SNOOP_ALL
        } else {
            !PCI_EX_NO_SNOOP_ALL
        };

        let _ = em_set_pci_ex_no_snoop(hw, snoop);
    }

    // Ungate DMA clock to avoid packet loss.
    if hw.mac_type >= em_pch_tgp {
        let mut fflt_dbg = e1000_read_reg(hw, E1000_FFLT_DBG);
        fflt_dbg |= 1 << 12;
        e1000_write_reg(hw, E1000_FFLT_DBG, fflt_dbg);
    }

    if hw.device_id == E1000_DEV_ID_82546GB_QUAD_COPPER
        || hw.device_id == E1000_DEV_ID_82546GB_QUAD_COPPER_KSP3
    {
        let mut ctrl_ext = e1000_read_reg(hw, E1000_CTRL_EXT);
        // Relaxed ordering must be disabled to avoid a parity error crash in a PCI slot.
        ctrl_ext |= E1000_CTRL_EXT_RO_DIS;
        e1000_write_reg(hw, E1000_CTRL_EXT, ctrl_ext);
    }
    ret_val
}

/// Adjusts the SERDES output amplitude based on the EEPROM setting.
fn em_adjust_serdes_amplitude(hw: &mut EmHw) -> Result<(), i32> {
    if hw.media_type != em_media_type_internal_serdes || hw.mac_type >= em_82575 {
        return Ok(());
    }

    match hw.mac_type {
        em_82545_rev_3 | em_82546_rev_3 => {}
        _ => return Ok(()),
    }

    let mut eeprom_data = [0u16; 1];
    em_read_eeprom(hw, EEPROM_SERDES_AMPLITUDE, 1, &mut eeprom_data)?;
    if eeprom_data[0] != EEPROM_RESERVED_WORD {
        // Adjust SERDES output amplitude only.
        let d = eeprom_data[0] & EEPROM_SERDES_AMPLITUDE_MASK;
        em_write_phy_reg(hw, M88E1000_PHY_EXT_CTRL, d)?;
    }
    Ok(())
}

/// `em_setup_link`: configures flow control and link settings.
///
/// Determines which flow control settings to use. Calls the appropriate media-specific link
/// configuration function. Configures the flow control settings. Assuming the adapter has a
/// valid link partner, a valid link should be established. Assumes the hardware has
/// previously been reset and the transmitter and receiver are not enabled.
pub fn em_setup_link(hw: &mut EmHw) -> Result<(), i32> {
    let mut eeprom_data = [0u16; 1];

    let eeprom_control2_reg_offset = if hw.mac_type != em_icp_xxxx {
        EEPROM_INIT_CONTROL2_REG
    } else {
        eeprom_init_control3_icp_xxxx(hw.icp_xxxx_port_num)
    };
    // In the case of the phy reset being blocked, we already have a link. We do not have to
    // set it up again.
    if em_check_phy_reset_block(hw).is_err() {
        return Ok(());
    }
    // Read and store word 0x0F of the EEPROM. This word contains bits that determine the
    // hardware's default PAUSE (flow control) mode, a bit that determines whether the HW
    // defaults to enabling or disabling auto-negotiation, and the direction of the SW
    // defined pins. If there is no SW over-ride of the flow control setting, then the
    // variable hw->fc will be initialized based on a value in the EEPROM.
    if hw.fc == E1000_FC_DEFAULT {
        match hw.mac_type {
            em_ich8lan | em_ich9lan | em_ich10lan | em_pchlan | em_pch2lan | em_pch_lpt
            | em_pch_spt | em_pch_cnp | em_pch_tgp | em_pch_adp | em_pch_mtp | em_pch_ptp
            | em_82573 | em_82574 => hw.fc = E1000_FC_FULL,
            _ => {
                if em_read_eeprom(hw, eeprom_control2_reg_offset, 1, &mut eeprom_data).is_err() {
                    return Err(-E1000_ERR_EEPROM);
                }
                if eeprom_data[0] & EEPROM_WORD0F_PAUSE_MASK == 0 {
                    hw.fc = E1000_FC_NONE;
                } else if eeprom_data[0] & EEPROM_WORD0F_PAUSE_MASK == EEPROM_WORD0F_ASM_DIR {
                    hw.fc = E1000_FC_TX_PAUSE;
                } else {
                    hw.fc = E1000_FC_FULL;
                }
            }
        }
    }
    // We want to save off the original Flow Control configuration just in case we get
    // disconnected and then reconnected into a different hub or switch with different Flow
    // Control capabilities.
    if hw.mac_type == em_82542_rev2_0 {
        hw.fc &= !E1000_FC_TX_PAUSE;
    }

    if hw.mac_type < em_82543 && hw.report_tx_early {
        hw.fc &= !E1000_FC_RX_PAUSE;
    }

    hw.original_fc = hw.fc;

    // Take the 4 bits from EEPROM word 0x0F that determine the initial polarity value for
    // the SW controlled pins, and setup the Extended Device Control reg with that info.
    // This is needed because one of the SW controlled pins is used for signal detection. So
    // this should be done before em_setup_pcs_link() or em_phy_setup() is called.
    if hw.mac_type == em_82543 {
        if em_read_eeprom(hw, EEPROM_INIT_CONTROL2_REG, 1, &mut eeprom_data).is_err() {
            return Err(-E1000_ERR_EEPROM);
        }
        let ctrl_ext = u32::from(eeprom_data[0] & EEPROM_WORD0F_SWPDIO_EXT) << SWDPIO__EXT_SHIFT;
        e1000_write_reg(hw, E1000_CTRL_EXT, ctrl_ext);
    }
    // Make sure we have a valid PHY.
    if let Err(e) = em_detect_gig_phy(hw) {
        if hw.mac_type == em_icp_xxxx {
            return Err(E1000_DEFER_INIT);
        } else {
            return Err(e);
        }
    }

    // Call the necessary subroutine to configure the link.
    let ret_val = match hw.media_type {
        em_media_type_copper | em_media_type_oem => em_setup_copper_link(hw),
        _ => em_setup_fiber_serdes_link(hw),
    };
    // Initialize the flow control address, type, and PAUSE timer registers to their
    // default values. This is done even if flow control is disabled, because it does not
    // hurt anything to initialize these registers.

    // FCAL/H and FCT are hardcoded to standard values in em_ich8lan / em_ich9lan /
    // em_ich10lan.
    if !is_ich8(hw.mac_type) {
        e1000_write_reg(hw, E1000_FCT, FLOW_CONTROL_TYPE);
        e1000_write_reg(hw, E1000_FCAH, FLOW_CONTROL_ADDRESS_HIGH);
        e1000_write_reg(hw, E1000_FCAL, FLOW_CONTROL_ADDRESS_LOW);
    }
    e1000_write_reg(hw, E1000_FCTTV, u32::from(hw.fc_pause_time));

    if matches!(
        hw.phy_type,
        em_phy_82577 | em_phy_82578 | em_phy_82579 | em_phy_i217
    ) {
        e1000_write_reg(hw, E1000_FCRTV_PCH, 0x1000);
        let t = hw.fc_pause_time;
        let _ = em_write_phy_reg(hw, phy_reg(BM_PORT_CTRL_PAGE, 27), t);
    }

    // Set the flow control receive threshold registers. Normally, these registers will be
    // set to a default threshold that may be adjusted later by the driver's runtime code.
    // However, if the ability to transmit pause frames in not enabled, then these registers
    // will be set to 0.
    if hw.fc & E1000_FC_TX_PAUSE == 0 {
        e1000_write_reg(hw, E1000_FCRTL, 0);
        e1000_write_reg(hw, E1000_FCRTH, 0);
    } else {
        // We need to set up the Receive Threshold high and low water marks as well as
        // (optionally) enabling the transmission of XON frames.
        if hw.fc_send_xon {
            e1000_write_reg(
                hw,
                E1000_FCRTL,
                u32::from(hw.fc_low_water) | E1000_FCRTL_XONE,
            );
            e1000_write_reg(hw, E1000_FCRTH, u32::from(hw.fc_high_water));
        } else {
            e1000_write_reg(hw, E1000_FCRTL, u32::from(hw.fc_low_water));
            e1000_write_reg(hw, E1000_FCRTH, u32::from(hw.fc_high_water));
        }
    }
    ret_val
}

/// `em_power_up_serdes_link_82575`: enables the PCS and powers up the laser of an 82575 or
/// later SerDes/SGMII port.
pub fn em_power_up_serdes_link_82575(hw: &EmHw) {
    if hw.media_type != em_media_type_internal_serdes && !hw.sgmii_active {
        return;
    }

    // Enable PCS to turn on link.
    let mut reg = e1000_read_reg(hw, E1000_PCS_CFG0);
    reg |= E1000_PCS_CFG_PCS_EN;
    e1000_write_reg(hw, E1000_PCS_CFG0, reg);

    // Power up the laser.
    reg = e1000_read_reg(hw, E1000_CTRL_EXT);
    reg &= !E1000_CTRL_EXT_SDP3_DATA;
    e1000_write_reg(hw, E1000_CTRL_EXT, reg);

    // Flush the write to verify completion.
    e1000_write_flush(hw);
    delay(5);
}

/// Sets up link for a fiber based or serdes based adapter.
///
/// Manipulates Physical Coding Sublayer functions in order to configure link. Assumes the
/// hardware has been previously reset and the transmitter and receiver are not enabled.
fn em_setup_fiber_serdes_link(hw: &mut EmHw) -> Result<(), i32> {
    let mut signal: u32 = 0;

    if hw.media_type != em_media_type_internal_serdes && !hw.sgmii_active {
        return Err(-E1000_ERR_CONFIG);
    }

    // On 82571 and 82572 Fiber connections, SerDes loopback mode persists until explicitly
    // turned off or a power cycle is performed. A read to the register does not indicate
    // its status. Therefore, we ensure loopback mode is disabled during initialization.
    if hw.mac_type == em_82571 || hw.mac_type == em_82572 || hw.mac_type >= em_82575 {
        e1000_write_reg(hw, E1000_SCTL, E1000_DISABLE_SERDES_LOOPBACK);
    }

    if hw.mac_type >= em_82575 {
        em_power_up_serdes_link_82575(hw);
    }

    // On adapters with a MAC newer than 82544, SWDP 1 will be set when the optics detect a
    // signal. On older adapters, it will be cleared when there is a signal. This applies to
    // fiber media only. If we're on serdes media, adjust the output amplitude to value set
    // in the EEPROM.
    let mut ctrl = e1000_read_reg(hw, E1000_CTRL);
    if hw.media_type == em_media_type_fiber {
        signal = if hw.mac_type > em_82544 {
            E1000_CTRL_SWDPIN1
        } else {
            0
        };
    }

    em_adjust_serdes_amplitude(hw)?;

    // Take the link out of reset.
    ctrl &= !E1000_CTRL_LRST;

    if hw.mac_type >= em_82575 {
        // Set both sw defined pins on 82575/82576.
        ctrl |= E1000_CTRL_SWDPIN0 | E1000_CTRL_SWDPIN1;

        let ctrl_ext = e1000_read_reg(hw, E1000_CTRL_EXT);
        let mode = ctrl_ext & E1000_CTRL_EXT_LINK_MODE_MASK;
        if mode == E1000_CTRL_EXT_LINK_MODE_1000BASE_KX
            || mode == E1000_CTRL_EXT_LINK_MODE_PCIE_SERDES
        {
            // The backplane is always connected.
            let mut reg = e1000_read_reg(hw, E1000_PCS_LCTL);
            reg |= E1000_PCS_LCTL_FORCE_FCTRL;
            reg |= E1000_PCS_LCTL_FSV_1000 | E1000_PCS_LCTL_FDV_FULL;
            reg |= E1000_PCS_LCTL_FSD; // Force Speed.
            e1000_write_reg(hw, E1000_PCS_LCTL, reg);
            let _ = em_force_mac_fc(hw);
            hw.autoneg_failed = 0;
            return Ok(());
        } else {
            // Set switch control to serdes energy detect.
            let mut reg = e1000_read_reg(hw, E1000_CONNSW);
            reg |= E1000_CONNSW_ENRGSRC;
            e1000_write_reg(hw, E1000_CONNSW, reg);
        }
    }

    // Adjust VCO speed to improve BER performance.
    em_set_vco_speed(hw)?;

    em_config_collision_dist(hw);
    // Check for a software override of the flow control settings, and setup the device
    // accordingly. If auto-negotiation is enabled, then software will have to set the
    // "PAUSE" bits to the correct value in the Transmit Config Word Register (TXCW) and
    // re-start auto-negotiation. However, if auto-negotiation is disabled, then software
    // will have to manually configure the two flow control enable bits in the CTRL
    // register.
    //
    // The possible values of the "fc" parameter are: 0: Flow control is completely
    // disabled 1: Rx flow control is enabled (we can receive pause frames, but not send
    // pause frames). 2: Tx flow control is enabled (we can send pause frames but we do not
    // support receiving pause frames). 3: Both Rx and TX flow control (symmetric) are
    // enabled.
    let txcw = match hw.fc {
        // Flow control is completely disabled by a software over-ride.
        E1000_FC_NONE => E1000_TXCW_ANE | E1000_TXCW_FD,
        // RX Flow control is enabled and TX Flow control is disabled by a software
        // over-ride. Since there really isn't a way to advertise that we are capable of RX
        // Pause ONLY, we will advertise that we support both symmetric and asymmetric RX
        // PAUSE. Later, we will disable the adapter's ability to send PAUSE frames.
        E1000_FC_RX_PAUSE => E1000_TXCW_ANE | E1000_TXCW_FD | E1000_TXCW_PAUSE_MASK,
        // TX Flow control is enabled, and RX Flow control is disabled, by a software
        // over-ride.
        E1000_FC_TX_PAUSE => E1000_TXCW_ANE | E1000_TXCW_FD | E1000_TXCW_ASM_DIR,
        // Flow control (both RX and TX) is enabled by a software over-ride.
        E1000_FC_FULL => E1000_TXCW_ANE | E1000_TXCW_FD | E1000_TXCW_PAUSE_MASK,
        // Flow control param set incorrectly.
        _ => return Err(-E1000_ERR_CONFIG),
    };
    // Since auto-negotiation is enabled, take the link out of reset (the link will be in
    // reset, because we previously reset the chip). This will restart auto-negotiation. If
    // auto-negotiation is successful then the link-up status bit will be set and the flow
    // control enable bits (RFCE and TFCE) will be set according to their negotiated value.
    e1000_write_reg(hw, E1000_TXCW, txcw);
    e1000_write_reg(hw, E1000_CTRL, ctrl);
    e1000_write_flush(hw);

    hw.txcw = txcw;
    msec_delay(1);
    // If we have a signal (the cable is plugged in) then poll for a "Link-Up" indication in
    // the Device Status Register. Time-out if a link isn't seen in 500 milliseconds seconds
    // (Auto-negotiation should complete in less than 500 milliseconds even if the other end
    // is doing it in SW). For internal serdes, we just assume a signal is present, then
    // poll.
    if hw.media_type == em_media_type_internal_serdes
        || (e1000_read_reg(hw, E1000_CTRL) & E1000_CTRL_SWDPIN1) == signal
    {
        let mut i = 0;
        while i < LINK_UP_TIMEOUT / 10 {
            msec_delay(10);
            let status = e1000_read_reg(hw, E1000_STATUS);
            if status & E1000_STATUS_LU != 0 {
                break;
            }
            i += 1;
        }
        if i == LINK_UP_TIMEOUT / 10 {
            // Never got a valid link from auto-neg.
            hw.autoneg_failed = 1;
            // AutoNeg failed to achieve a link, so we'll call em_check_for_link. This
            // routine will force the link up if we detect a signal. This will allow us to
            // communicate with non-autonegotiating link partners.
            em_check_for_link(hw)?;
            hw.autoneg_failed = 0;
        } else {
            hw.autoneg_failed = 0;
        }
    }
    Ok(())
}

/// Makes sure we have a valid PHY and changes the PHY mode before link setup.
fn em_copper_link_preconfig(hw: &mut EmHw) -> Result<(), i32> {
    let mut phy_data: u16 = 0;

    let mut ctrl = e1000_read_reg(hw, E1000_CTRL);
    // With 82543, we need to force speed and duplex on the MAC equal to what the PHY speed
    // and duplex configuration is. In addition, we need to perform a hardware reset on the
    // PHY to take it out of reset.
    if hw.mac_type > em_82543 {
        ctrl |= E1000_CTRL_SLU;
        ctrl &= !(E1000_CTRL_FRCSPD | E1000_CTRL_FRCDPX);
        e1000_write_reg(hw, E1000_CTRL, ctrl);
    } else {
        ctrl |= E1000_CTRL_FRCSPD | E1000_CTRL_FRCDPX | E1000_CTRL_SLU;
        e1000_write_reg(hw, E1000_CTRL, ctrl);
        em_phy_hw_reset(hw)?;
    }

    // Set PHY to class A mode (if necessary).
    em_set_phy_mode(hw)?;

    if hw.mac_type == em_82545_rev_3 || hw.mac_type == em_82546_rev_3 {
        let _ = em_read_phy_reg(hw, M88E1000_PHY_SPEC_CTRL, &mut phy_data);
        phy_data |= 0x00000008;
        let _ = em_write_phy_reg(hw, M88E1000_PHY_SPEC_CTRL, phy_data);
    }
    if hw.mac_type <= em_82543
        || hw.mac_type == em_82541
        || hw.mac_type == em_82547
        || hw.mac_type == em_82541_rev_2
        || hw.mac_type == em_82547_rev_2
    {
        hw.phy_reset_disable = false;
    }
    if matches!(
        hw.mac_type,
        em_82575 | em_82580 | em_82576 | em_i210 | em_i350
    ) && hw.sgmii_active
    {
        // Allow time for SFP cage time to power up phy.
        msec_delay(300);

        // SFP documentation requires the following to configure the SFP module to work on
        // SGMII. No further documentation is given.
        let _ = em_write_phy_reg(hw, 0x1B, 0x8084);
        let _ = em_phy_hw_reset(hw);
    }

    Ok(())
}

/// Copper link setup for the em_phy_igp series.
fn em_copper_link_igp_setup(hw: &mut EmHw) -> Result<(), i32> {
    let mut phy_data: u16 = 0;

    if hw.phy_reset_disable {
        return Ok(());
    }

    em_phy_reset(hw)?;
    // Wait 15ms for MAC to configure PHY from eeprom settings.
    msec_delay(15);
    if hw.mac_type != em_ich8lan && hw.mac_type != em_ich9lan && hw.mac_type != em_ich10lan {
        // Configure activity LED after PHY reset.
        let mut led_ctrl = e1000_read_reg(hw, E1000_LEDCTL);
        led_ctrl &= IGP_ACTIVITY_LED_MASK;
        led_ctrl |= IGP_ACTIVITY_LED_ENABLE | IGP_LED3_MODE;
        e1000_write_reg(hw, E1000_LEDCTL, led_ctrl);
    }
    // The NVM settings will configure LPLU in D3 for IGP2 and IGP3 PHYs.
    if hw.phy_type == em_phy_igp {
        // Disable lplu d3 during driver init.
        em_set_d3_lplu_state(hw, false)?;
    }
    // Disable lplu d0 during driver init.
    if hw.mac_type >= em_pchlan {
        em_set_lplu_state_pchlan(hw, false)?;
    } else {
        em_set_d0_lplu_state(hw, false)?;
    }
    // Configure mdi-mdix settings.
    em_read_phy_reg(hw, IGP01E1000_PHY_PORT_CTRL, &mut phy_data)?;

    if hw.mac_type == em_82541 || hw.mac_type == em_82547 {
        hw.dsp_config_state = em_dsp_config_disabled;
        // Force MDI for earlier revs of the IGP PHY.
        phy_data &= !(IGP01E1000_PSCR_AUTO_MDIX | IGP01E1000_PSCR_FORCE_MDI_MDIX);
        hw.mdix = 1;
    } else {
        hw.dsp_config_state = em_dsp_config_enabled;
        phy_data &= !IGP01E1000_PSCR_AUTO_MDIX;

        match hw.mdix {
            1 => phy_data &= !IGP01E1000_PSCR_FORCE_MDI_MDIX,
            2 => phy_data |= IGP01E1000_PSCR_FORCE_MDI_MDIX,
            _ => phy_data |= IGP01E1000_PSCR_AUTO_MDIX,
        }
    }
    em_write_phy_reg(hw, IGP01E1000_PHY_PORT_CTRL, phy_data)?;

    // Set auto-master slave resolution settings.
    if hw.autoneg != 0 {
        let phy_ms_setting = hw.master_slave;
        if hw.ffe_config_state == em_ffe_config_active {
            hw.ffe_config_state = em_ffe_config_enabled;
        }

        if hw.dsp_config_state == em_dsp_config_activated {
            hw.dsp_config_state = em_dsp_config_enabled;
        }
        // When autonegotiation advertisement is only 1000Mbps then we should disable
        // SmartSpeed and enable Auto MasterSlave resolution as hardware default.
        if hw.autoneg_advertised == ADVERTISE_1000_FULL {
            // Disable SmartSpeed.
            em_read_phy_reg(hw, IGP01E1000_PHY_PORT_CONFIG, &mut phy_data)?;

            phy_data &= !IGP01E1000_PSCFR_SMART_SPEED;
            em_write_phy_reg(hw, IGP01E1000_PHY_PORT_CONFIG, phy_data)?;
            // Set auto Master/Slave resolution process.
            em_read_phy_reg(hw, PHY_1000T_CTRL, &mut phy_data)?;

            phy_data &= !CR_1000T_MS_ENABLE;
            em_write_phy_reg(hw, PHY_1000T_CTRL, phy_data)?;
        }
        em_read_phy_reg(hw, PHY_1000T_CTRL, &mut phy_data)?;

        // Load defaults for future use.
        hw.original_master_slave = if phy_data & CR_1000T_MS_ENABLE != 0 {
            if phy_data & CR_1000T_MS_VALUE != 0 {
                em_ms_force_master
            } else {
                em_ms_force_slave
            }
        } else {
            em_ms_auto
        };

        match phy_ms_setting {
            em_ms_force_master => phy_data |= CR_1000T_MS_ENABLE | CR_1000T_MS_VALUE,
            em_ms_force_slave => {
                phy_data |= CR_1000T_MS_ENABLE;
                phy_data &= !CR_1000T_MS_VALUE;
            }
            em_ms_auto => phy_data &= !CR_1000T_MS_ENABLE,
            _ => {}
        }
        em_write_phy_reg(hw, PHY_1000T_CTRL, phy_data)?;
    }
    Ok(())
}

/// Copper link setup for the em_phy_gg82563 series.
fn em_copper_link_ggp_setup(hw: &mut EmHw) -> Result<(), i32> {
    let mut phy_data: u16 = 0;

    if !hw.phy_reset_disable {
        // Enable CRS on TX for half-duplex operation.
        em_read_phy_reg(hw, GG82563_PHY_MAC_SPEC_CTRL, &mut phy_data)?;

        phy_data |= GG82563_MSCR_ASSERT_CRS_ON_TX;
        // Use 25MHz for both link down and 1000BASE-T for Tx clock.
        phy_data |= GG82563_MSCR_TX_CLK_1000MBPS_25MHZ;

        em_write_phy_reg(hw, GG82563_PHY_MAC_SPEC_CTRL, phy_data)?;
        // Options: MDI/MDI-X = 0 (default) 0 - Auto for all speeds 1 - MDI mode 2 - MDI-X
        // mode 3 - Auto for 1000Base-T only (MDI-X for 10/100Base-T modes).
        em_read_phy_reg(hw, GG82563_PHY_SPEC_CTRL, &mut phy_data)?;

        phy_data &= !GG82563_PSCR_CROSSOVER_MODE_MASK;

        match hw.mdix {
            1 => phy_data |= GG82563_PSCR_CROSSOVER_MODE_MDI,
            2 => phy_data |= GG82563_PSCR_CROSSOVER_MODE_MDIX,
            _ => phy_data |= GG82563_PSCR_CROSSOVER_MODE_AUTO,
        }
        // Options: disable_polarity_correction = 0 (default) Automatic Correction for
        // Reversed Cable Polarity 0 - Disabled 1 - Enabled.
        phy_data &= !GG82563_PSCR_POLARITY_REVERSAL_DISABLE;
        if hw.disable_polarity_correction {
            phy_data |= GG82563_PSCR_POLARITY_REVERSAL_DISABLE;
        }
        em_write_phy_reg(hw, GG82563_PHY_SPEC_CTRL, phy_data)?;

        // SW Reset the PHY so all changes take effect.
        em_phy_reset(hw)?;
    }
    if hw.mac_type == em_80003es2lan {
        // Bypass RX and TX FIFO's.
        em_write_kmrn_reg(
            hw,
            E1000_KUMCTRLSTA_OFFSET_FIFO_CTRL,
            E1000_KUMCTRLSTA_FIFO_CTRL_RX_BYPASS | E1000_KUMCTRLSTA_FIFO_CTRL_TX_BYPASS,
        )?;

        em_read_phy_reg(hw, GG82563_PHY_SPEC_CTRL_2, &mut phy_data)?;

        phy_data &= !GG82563_PSCR2_REVERSE_AUTO_NEG;
        em_write_phy_reg(hw, GG82563_PHY_SPEC_CTRL_2, phy_data)?;

        let mut reg_data = e1000_read_reg(hw, E1000_CTRL_EXT);
        reg_data &= !E1000_CTRL_EXT_LINK_MODE_MASK;
        e1000_write_reg(hw, E1000_CTRL_EXT, reg_data);

        em_read_phy_reg(hw, GG82563_PHY_PWR_MGMT_CTRL, &mut phy_data)?;
        // Do not init these registers when the HW is in IAMT mode, since the firmware will
        // have already initialized them. We only initialize them if the HW is not in IAMT
        // mode.
        if !em_check_mng_mode(hw) {
            // Enable Electrical Idle on the PHY.
            phy_data |= GG82563_PMCR_ENABLE_ELECTRICAL_IDLE;
            em_write_phy_reg(hw, GG82563_PHY_PWR_MGMT_CTRL, phy_data)?;

            em_read_phy_reg(hw, GG82563_PHY_KMRN_MODE_CTRL, &mut phy_data)?;

            phy_data &= !GG82563_KMCR_PASS_FALSE_CARRIER;
            em_write_phy_reg(hw, GG82563_PHY_KMRN_MODE_CTRL, phy_data)?;
        }
        // Workaround: Disable padding in Kumeran interface in the MAC and in the PHY to
        // avoid CRC errors.
        em_read_phy_reg(hw, GG82563_PHY_INBAND_CTRL, &mut phy_data)?;
        phy_data |= GG82563_ICR_DIS_PADDING;
        em_write_phy_reg(hw, GG82563_PHY_INBAND_CTRL, phy_data)?;
    }
    Ok(())
}

/// Copper link setup for the em_phy_m88 series.
fn em_copper_link_mgp_setup(hw: &mut EmHw) -> Result<(), i32> {
    let mut phy_data: u16 = 0;

    if hw.phy_reset_disable {
        return Ok(());
    }

    // Disable lplu d0 during driver init (the C overwrites the status unread).
    if hw.mac_type >= em_pchlan {
        let _ = em_set_lplu_state_pchlan(hw, false);
    }

    // Enable CRS on TX. This must be set for half-duplex operation.
    em_read_phy_reg(hw, M88E1000_PHY_SPEC_CTRL, &mut phy_data)?;

    if hw.phy_id == M88E1141_E_PHY_ID {
        phy_data |= 0x00000008;
        em_write_phy_reg(hw, M88E1000_PHY_SPEC_CTRL, phy_data)?;

        em_read_phy_reg(hw, M88E1000_PHY_SPEC_CTRL, &mut phy_data)?;

        phy_data &= !M88E1000_PSCR_ASSERT_CRS_ON_TX;
    } else if hw.phy_type != em_phy_bm {
        // For BM PHY this bit is downshift enable.
        phy_data |= M88E1000_PSCR_ASSERT_CRS_ON_TX;
    }
    // Options: MDI/MDI-X = 0 (default) 0 - Auto for all speeds 1 - MDI mode 2 - MDI-X mode
    // 3 - Auto for 1000Base-T only (MDI-X for 10/100Base-T modes).
    phy_data &= !M88E1000_PSCR_AUTO_X_MODE;

    match hw.mdix {
        1 => phy_data |= M88E1000_PSCR_MDI_MANUAL_MODE,
        2 => phy_data |= M88E1000_PSCR_MDIX_MANUAL_MODE,
        3 => phy_data |= M88E1000_PSCR_AUTO_X_1000T,
        _ => phy_data |= M88E1000_PSCR_AUTO_X_MODE,
    }
    // Options: disable_polarity_correction = 0 (default) Automatic Correction for Reversed
    // Cable Polarity 0 - Disabled 1 - Enabled.
    phy_data &= !M88E1000_PSCR_POLARITY_REVERSAL;
    if hw.disable_polarity_correction {
        phy_data |= M88E1000_PSCR_POLARITY_REVERSAL;
    }

    // Enable downshift on BM (disabled by default).
    if hw.phy_type == em_phy_bm {
        phy_data |= BME1000_PSCR_ENABLE_DOWNSHIFT;
    }

    em_write_phy_reg(hw, M88E1000_PHY_SPEC_CTRL, phy_data)?;

    if (hw.phy_type == em_phy_m88
        && hw.phy_revision < M88E1011_I_REV_4
        && hw.phy_id != BME1000_E_PHY_ID)
        || hw.phy_type == em_phy_oem
    {
        // Force TX_CLK in the Extended PHY Specific Control Register to 25MHz clock.
        em_read_phy_reg(hw, M88E1000_EXT_PHY_SPEC_CTRL, &mut phy_data)?;

        if hw.phy_type == em_phy_oem {
            phy_data |= M88E1000_EPSCR_TX_TIME_CTRL;
            phy_data |= M88E1000_EPSCR_RX_TIME_CTRL;
        }
        phy_data |= M88E1000_EPSCR_TX_CLK_25;

        if hw.phy_revision == E1000_REVISION_2 && hw.phy_id == M88E1111_I_PHY_ID {
            // Vidalia Phy, set the downshift counter to 5x.
            phy_data &= !M88EC018_EPSCR_DOWNSHIFT_COUNTER_MASK;
            phy_data |= M88EC018_EPSCR_DOWNSHIFT_COUNTER_5X;
            em_write_phy_reg(hw, M88E1000_EXT_PHY_SPEC_CTRL, phy_data)?;
        } else {
            // Configure Master and Slave downshift values.
            phy_data &=
                !(M88E1000_EPSCR_MASTER_DOWNSHIFT_MASK | M88E1000_EPSCR_SLAVE_DOWNSHIFT_MASK);
            phy_data |= M88E1000_EPSCR_MASTER_DOWNSHIFT_1X | M88E1000_EPSCR_SLAVE_DOWNSHIFT_1X;
            em_write_phy_reg(hw, M88E1000_EXT_PHY_SPEC_CTRL, phy_data)?;
        }
    }
    if hw.phy_type == em_phy_bm && hw.phy_revision == 1 {
        // Set PHY page 0, register 29 to 0x0003. The next two writes are supposed to lower
        // BER for gig connection.
        em_write_phy_reg(hw, BM_REG_BIAS1, 0x0003)?;

        // Set PHY page 0, register 30 to 0x0000.
        em_write_phy_reg(hw, BM_REG_BIAS2, 0x0000)?;
    }
    if hw.phy_type == em_phy_82578 {
        em_read_phy_reg(hw, M88E1000_EXT_PHY_SPEC_CTRL, &mut phy_data)?;

        // 82578 PHY - set the downshift count to 1x.
        phy_data |= I82578_EPSCR_DOWNSHIFT_ENABLE;
        phy_data &= !I82578_EPSCR_DOWNSHIFT_COUNTER_MASK;
        em_write_phy_reg(hw, M88E1000_EXT_PHY_SPEC_CTRL, phy_data)?;
    }
    // SW Reset the PHY so all changes take effect.
    em_phy_reset(hw)?;
    Ok(())
}

/// Copper link setup for the em_phy_82577 series.
fn em_copper_link_82577_setup(hw: &mut EmHw) -> Result<(), i32> {
    let mut phy_data: u16 = 0;

    if hw.phy_reset_disable {
        return Ok(());
    }

    // Enable CRS on TX for half-duplex operation.
    em_read_phy_reg(hw, I82577_PHY_CFG_REG, &mut phy_data)?;

    phy_data |= I82577_PHY_CFG_ENABLE_CRS_ON_TX | I82577_PHY_CFG_ENABLE_DOWNSHIFT;

    em_write_phy_reg(hw, I82577_PHY_CFG_REG, phy_data)?;

    // Wait 15ms for MAC to configure PHY from eeprom settings.
    msec_delay(15);
    let led_ctl = hw.ledctl_mode1;

    // Disable lplu d0 during driver init.
    em_set_lplu_state_pchlan(hw, false)?;

    e1000_write_reg(hw, E1000_LEDCTL, led_ctl);

    Ok(())
}

/// Copper link setup for the 82580 (and i350) internal PHY.
fn em_copper_link_82580_setup(hw: &mut EmHw) -> Result<(), i32> {
    let mut phy_data: u16 = 0;

    if hw.phy_reset_disable {
        return Ok(());
    }

    em_phy_reset(hw)?;

    // Enable CRS on TX. This must be set for half-duplex operation.
    em_read_phy_reg(hw, I82580_CFG_REG, &mut phy_data)?;

    phy_data |= I82580_CFG_ASSERT_CRS_ON_TX | I82580_CFG_ENABLE_DOWNSHIFT;

    em_write_phy_reg(hw, I82580_CFG_REG, phy_data)
}

/// Copper link setup for the RTL8211 PHY of the EP80579 boards.
fn em_copper_link_rtl8211_setup(hw: &mut EmHw) -> Result<(), i32> {
    // SW Reset the PHY so all changes take effect.
    let _ = em_phy_hw_reset(hw);

    // Enable CRS on TX. This must be set for half-duplex operation.
    let mut phy_data: u16 = 0;

    if let Err(e) = em_read_phy_reg_ex(hw, RGEPHY_CR, &mut phy_data) {
        printf(format_args!("Unable to read RGEPHY_CR register\n"));
        return Err(e);
    }
    phy_data |= RGEPHY_CR_ASSERT_CRS;

    if let Err(e) = em_write_phy_reg_ex(hw, RGEPHY_CR, phy_data) {
        printf(format_args!("Unable to write RGEPHY_CR register\n"));
        return Err(e);
    }

    phy_data = 0; // LED Control Register 0x18.
    if let Err(e) = em_read_phy_reg_ex(hw, RGEPHY_LC, &mut phy_data) {
        printf(format_args!("Unable to read RGEPHY_LC register\n"));
        return Err(e);
    }

    phy_data &= 0x80FF; // bit-15=0 disable, clear bit 8-10.
    if let Err(e) = em_write_phy_reg_ex(hw, RGEPHY_LC, phy_data) {
        printf(format_args!("Unable to write RGEPHY_LC register\n"));
        return Err(e);
    }
    // LED Control and Definition Register 0x11, PHY spec status reg.
    phy_data = 0;
    if let Err(e) = em_read_phy_reg_ex(hw, RGEPHY_SR, &mut phy_data) {
        printf(format_args!("Unable to read RGEPHY_SR register\n"));
        return Err(e);
    }

    phy_data |= 0x0010; // LED active Low.
    if let Err(e) = em_write_phy_reg_ex(hw, RGEPHY_SR, phy_data) {
        printf(format_args!("Unable to write RGEPHY_SR register\n"));
        return Err(e);
    }

    phy_data = 0;
    if let Err(e) = em_read_phy_reg_ex(hw, RGEPHY_SR, &mut phy_data) {
        printf(format_args!("Unable to read RGEPHY_SR register\n"));
        return Err(e);
    }

    // Switch to Page2.
    phy_data = RGEPHY_PS_PAGE_2;
    if let Err(e) = em_write_phy_reg_ex(hw, RGEPHY_PS, phy_data) {
        printf(format_args!("Unable to write PHY RGEPHY_PS register\n"));
        return Err(e);
    }

    phy_data = 0x0000;
    if let Err(e) = em_write_phy_reg_ex(hw, RGEPHY_LC_P2, phy_data) {
        printf(format_args!("Unable to write RGEPHY_LC_P2 register\n"));
        return Err(e);
    }
    usec_delay(5);

    // LED Configuration Control Reg for setting for 0x1A Register.
    phy_data = 0;
    if let Err(e) = em_read_phy_reg_ex(hw, RGEPHY_LC_P2, &mut phy_data) {
        printf(format_args!("Unable to read RGEPHY_LC_P2 register\n"));
        return Err(e);
    }

    phy_data &= 0xF000;
    phy_data |= 0x0F24;
    if let Err(e) = em_write_phy_reg_ex(hw, RGEPHY_LC_P2, phy_data) {
        printf(format_args!("Unable to write RGEPHY_LC_P2 register\n"));
        return Err(e);
    }
    phy_data = 0;
    if let Err(e) = em_read_phy_reg_ex(hw, RGEPHY_LC_P2, &mut phy_data) {
        printf(format_args!("Unable to read RGEPHY_LC_P2 register\n"));
        return Err(e);
    }

    // After setting Page2, go back to Page 0.
    phy_data = 0;
    if let Err(e) = em_write_phy_reg_ex(hw, RGEPHY_PS, phy_data) {
        printf(format_args!("Unable to write PHY RGEPHY_PS register\n"));
        return Err(e);
    }

    // Pulse stretching= 42-84ms, blink rate=84mm.
    phy_data = 0x140 | RGEPHY_LC_PULSE_42MS | RGEPHY_LC_LINK | RGEPHY_LC_DUPLEX | RGEPHY_LC_RX;

    if let Err(e) = em_write_phy_reg_ex(hw, RGEPHY_LC, phy_data) {
        printf(format_args!("Unable to write RGEPHY_LC register\n"));
        return Err(e);
    }
    Ok(())
}

/// `em_copper_link_autoneg`: sets up auto-negotiation and flow control advertisements, and
/// then performs auto-negotiation.
pub fn em_copper_link_autoneg(hw: &mut EmHw) -> Result<(), i32> {
    let mut phy_data: u16 = 0;
    // Perform some bounds checking on the hw->autoneg_advertised parameter. If this
    // variable is zero, then set it to the default.
    hw.autoneg_advertised &= AUTONEG_ADVERTISE_SPEED_DEFAULT;
    // If autoneg_advertised is zero, we assume it was not defaulted by the calling code so
    // we set to advertise full capability.
    if hw.autoneg_advertised == 0 {
        hw.autoneg_advertised = AUTONEG_ADVERTISE_SPEED_DEFAULT;
    }

    // IFE phy only supports 10/100.
    if hw.phy_type == em_phy_ife {
        hw.autoneg_advertised &= AUTONEG_ADVERTISE_10_100_ALL;
    }

    em_phy_setup_autoneg(hw)?;
    // Restart auto-negotiation by setting the Auto Neg Enable bit and the Auto Neg Restart
    // bit in the PHY control register.
    em_read_phy_reg(hw, PHY_CTRL, &mut phy_data)?;

    phy_data |= MII_CR_AUTO_NEG_EN | MII_CR_RESTART_AUTO_NEG;
    em_write_phy_reg(hw, PHY_CTRL, phy_data)?;
    // Does the user want to wait for Auto-Neg to complete here, or check at a later time
    // (for example, callback routine).
    if hw.wait_autoneg_complete != 0 {
        em_wait_autoneg(hw)?;
    }
    hw.get_link_status = true;

    Ok(())
}

/// `em_copper_link_postconfig`: configures the MAC and the PHY after link is up.
///
/// 1. Sets up the MAC to the current PHY speed/duplex if we are on 82543. If we are on
///    newer silicon, we only need to configure collision distance in the Transmit Control
///    Register.
/// 2. Sets up flow control on the MAC to that established with the link partner.
/// 3. Configures the DSP to improve Gigabit link quality for some PHY revisions.
pub fn em_copper_link_postconfig(hw: &mut EmHw) -> Result<(), i32> {
    if hw.mac_type >= em_82544 && hw.mac_type != em_icp_xxxx {
        em_config_collision_dist(hw);
    } else {
        em_config_mac_to_phy(hw)?;
    }
    em_config_fc_after_link_up(hw)?;
    // Config DSP to improve Giga link quality.
    if hw.phy_type == em_phy_igp {
        em_config_dsp_after_link_change(hw, true)?;
    }
    Ok(())
}

/// Detects which PHY is present and sets up the speed and duplex.
fn em_setup_copper_link(hw: &mut EmHw) -> Result<(), i32> {
    let mut phy_data: u16 = 0;
    let mut reg_data: u16 = 0;

    if matches!(
        hw.mac_type,
        em_80003es2lan
            | em_ich8lan
            | em_ich9lan
            | em_ich10lan
            | em_pchlan
            | em_pch2lan
            | em_pch_lpt
            | em_pch_spt
            | em_pch_cnp
            | em_pch_tgp
            | em_pch_adp
            | em_pch_mtp
            | em_pch_ptp
    ) {
        // Set the mac to wait the maximum time between each iteration and increase the max
        // iterations when polling the phy; this fixes erroneous timeouts at 10Mbps.
        em_write_kmrn_reg(hw, gg82563_reg(0x34, 4), 0xFFFF)?;
        em_read_kmrn_reg(hw, gg82563_reg(0x34, 9), &mut reg_data)?;
        reg_data |= 0x3F;
        em_write_kmrn_reg(hw, gg82563_reg(0x34, 9), reg_data)?;
    }

    // Check if it is a valid PHY and set PHY mode if necessary.
    em_copper_link_preconfig(hw)?;

    if hw.mac_type == em_80003es2lan {
        // Kumeran registers are written-only.
        reg_data = E1000_KUMCTRLSTA_INB_CTRL_LINK_STATUS_TX_TIMEOUT_DEFAULT;
        reg_data |= E1000_KUMCTRLSTA_INB_CTRL_DIS_PADDING;
        em_write_kmrn_reg(hw, E1000_KUMCTRLSTA_OFFSET_INB_CTRL, reg_data)?;
    }

    if hw.phy_type == em_phy_igp || hw.phy_type == em_phy_igp_3 || hw.phy_type == em_phy_igp_2 {
        em_copper_link_igp_setup(hw)?;
    } else if hw.phy_type == em_phy_m88
        || hw.phy_type == em_phy_bm
        || hw.phy_type == em_phy_oem
        || hw.phy_type == em_phy_82578
    {
        em_copper_link_mgp_setup(hw)?;
    } else if hw.phy_type == em_phy_gg82563 {
        em_copper_link_ggp_setup(hw)?;
    } else if hw.phy_type == em_phy_82577
        || hw.phy_type == em_phy_82579
        || hw.phy_type == em_phy_i217
    {
        em_copper_link_82577_setup(hw)?;
    } else if hw.phy_type == em_phy_82580 {
        em_copper_link_82580_setup(hw)?;
    } else if hw.phy_type == em_phy_rtl8211 {
        em_copper_link_rtl8211_setup(hw)?;
    }
    if hw.autoneg != 0 {
        // Setup autoneg and flow control advertisement and perform autonegotiation.
        em_copper_link_autoneg(hw)?;
    } else {
        // PHY will be set to 10H, 10F, 100H,or 100F depending on value from
        // forced_speed_duplex.
        em_phy_force_speed_duplex(hw)?;
    }
    // Check link status. Wait up to 100 microseconds for link to become valid.
    for _ in 0..10 {
        em_read_phy_reg(hw, PHY_STATUS, &mut phy_data)?;
        em_read_phy_reg(hw, PHY_STATUS, &mut phy_data)?;

        hw.icp_xxxx_is_link_up = (phy_data & MII_SR_LINK_STATUS) != 0;

        if phy_data & MII_SR_LINK_STATUS != 0 {
            // Config the MAC and PHY after link is up.
            em_copper_link_postconfig(hw)?;

            return Ok(());
        }
        usec_delay(10);
    }

    // Unable to establish link.
    Ok(())
}

/// Configures the MAC-to-PHY interface for 10/100Mbps.
fn em_configure_kmrn_for_10_100(hw: &mut EmHw, duplex: u16) -> Result<(), i32> {
    let mut reg_data = E1000_KUMCTRLSTA_HD_CTRL_10_100_DEFAULT;
    em_write_kmrn_reg(hw, E1000_KUMCTRLSTA_OFFSET_HD_CTRL, reg_data)?;

    // Configure Transmit Inter-Packet Gap.
    let mut tipg = e1000_read_reg(hw, E1000_TIPG);
    tipg &= !E1000_TIPG_IPGT_MASK;
    tipg |= DEFAULT_80003ES2LAN_TIPG_IPGT_10_100;
    e1000_write_reg(hw, E1000_TIPG, tipg);

    em_read_phy_reg(hw, GG82563_PHY_KMRN_MODE_CTRL, &mut reg_data)?;

    if duplex == HALF_DUPLEX {
        reg_data |= GG82563_KMCR_PASS_FALSE_CARRIER;
    } else {
        reg_data &= !GG82563_KMCR_PASS_FALSE_CARRIER;
    }

    em_write_phy_reg(hw, GG82563_PHY_KMRN_MODE_CTRL, reg_data)
}

/// Configures the MAC-to-PHY interface for 1000Mbps.
fn em_configure_kmrn_for_1000(hw: &mut EmHw) -> Result<(), i32> {
    let mut reg_data = E1000_KUMCTRLSTA_HD_CTRL_1000_DEFAULT;
    em_write_kmrn_reg(hw, E1000_KUMCTRLSTA_OFFSET_HD_CTRL, reg_data)?;

    // Configure Transmit Inter-Packet Gap.
    let mut tipg = e1000_read_reg(hw, E1000_TIPG);
    tipg &= !E1000_TIPG_IPGT_MASK;
    tipg |= DEFAULT_80003ES2LAN_TIPG_IPGT_1000;
    e1000_write_reg(hw, E1000_TIPG, tipg);

    em_read_phy_reg(hw, GG82563_PHY_KMRN_MODE_CTRL, &mut reg_data)?;

    reg_data &= !GG82563_KMCR_PASS_FALSE_CARRIER;
    em_write_phy_reg(hw, GG82563_PHY_KMRN_MODE_CTRL, reg_data)
}

/// `em_phy_setup_autoneg`: configures the PHY autoneg and flow control advertisement
/// settings.
pub fn em_phy_setup_autoneg(hw: &mut EmHw) -> Result<(), i32> {
    let mut mii_autoneg_adv_reg: u16 = 0;
    let mut mii_1000t_ctrl_reg: u16 = 0;

    // Read the MII Auto-Neg Advertisement Register (Address 4).
    em_read_phy_reg(hw, PHY_AUTONEG_ADV, &mut mii_autoneg_adv_reg)?;

    if hw.phy_type != em_phy_ife {
        // Read the MII 1000Base-T Control Register (Address 9).
        em_read_phy_reg(hw, PHY_1000T_CTRL, &mut mii_1000t_ctrl_reg)?;
    } else {
        mii_1000t_ctrl_reg = 0;
    }
    // Need to parse both autoneg_advertised and fc and set up the appropriate PHY
    // registers. First we will parse for autoneg_advertised software override. Since we
    // can advertise a plethora of combinations, we need to check each bit individually.

    // First we clear all the 10/100 mb speed bits in the Auto-Neg Advertisement Register
    // (Address 4) and the 1000 mb speed bits in the 1000Base-T Control Register (Address
    // 9).
    mii_autoneg_adv_reg &= !REG4_SPEED_MASK;
    mii_1000t_ctrl_reg &= !REG9_SPEED_MASK;

    // Do we want to advertise 10 Mb Half Duplex?
    if hw.autoneg_advertised & ADVERTISE_10_HALF != 0 {
        mii_autoneg_adv_reg |= NWAY_AR_10T_HD_CAPS;
    }
    // Do we want to advertise 10 Mb Full Duplex?
    if hw.autoneg_advertised & ADVERTISE_10_FULL != 0 {
        mii_autoneg_adv_reg |= NWAY_AR_10T_FD_CAPS;
    }
    // Do we want to advertise 100 Mb Half Duplex?
    if hw.autoneg_advertised & ADVERTISE_100_HALF != 0 {
        mii_autoneg_adv_reg |= NWAY_AR_100TX_HD_CAPS;
    }
    // Do we want to advertise 100 Mb Full Duplex?
    if hw.autoneg_advertised & ADVERTISE_100_FULL != 0 {
        mii_autoneg_adv_reg |= NWAY_AR_100TX_FD_CAPS;
    }
    // We do not allow the Phy to advertise 1000 Mb Half Duplex (ADVERTISE_1000_HALF is
    // only a debug message in the C).

    // Do we want to advertise 1000 Mb Full Duplex?
    if hw.autoneg_advertised & ADVERTISE_1000_FULL != 0 {
        mii_1000t_ctrl_reg |= CR_1000T_FD_CAPS;
    }
    // Check for a software override of the flow control settings, and setup the PHY
    // advertisement registers accordingly. If auto-negotiation is enabled, then software
    // will have to set the "PAUSE" bits to the correct value in the Auto-Negotiation
    // Advertisement Register (PHY_AUTONEG_ADV) and re-start auto-negotiation.
    //
    // The possible values of the "fc" parameter are: 0: Flow control is completely
    // disabled 1: Rx flow control is enabled (we can receive pause frames but not send
    // pause frames). 2: Tx flow control is enabled (we can send pause frames but we do not
    // support receiving pause frames). 3: Both Rx and TX flow control (symmetric) are
    // enabled. other: No software override. The flow control configuration in the EEPROM
    // is used.
    match hw.fc {
        E1000_FC_NONE => {
            // Flow control (RX & TX) is completely disabled by a software over-ride.
            mii_autoneg_adv_reg &= !(NWAY_AR_ASM_DIR | NWAY_AR_PAUSE);
        }
        E1000_FC_RX_PAUSE => {
            // RX Flow control is enabled, and TX Flow control is disabled, by a software
            // over-ride. Since there really isn't a way to advertise that we are capable
            // of RX Pause ONLY, we will advertise that we support both symmetric and
            // asymmetric RX PAUSE. Later (in em_config_fc_after_link_up) we will disable
            // the hw's ability to send PAUSE frames.
            mii_autoneg_adv_reg |= NWAY_AR_ASM_DIR | NWAY_AR_PAUSE;
        }
        E1000_FC_TX_PAUSE => {
            // TX Flow control is enabled, and RX Flow control is disabled, by a software
            // over-ride.
            mii_autoneg_adv_reg |= NWAY_AR_ASM_DIR;
            mii_autoneg_adv_reg &= !NWAY_AR_PAUSE;
        }
        E1000_FC_FULL => {
            // Flow control (both RX and TX) is enabled by a software over-ride.
            mii_autoneg_adv_reg |= NWAY_AR_ASM_DIR | NWAY_AR_PAUSE;
        }
        // Flow control param set incorrectly.
        _ => return Err(-E1000_ERR_CONFIG),
    }

    em_write_phy_reg(hw, PHY_AUTONEG_ADV, mii_autoneg_adv_reg)?;

    if hw.phy_type != em_phy_ife {
        em_write_phy_reg(hw, PHY_1000T_CTRL, mii_1000t_ctrl_reg)?;
    }
    Ok(())
}

/// Forces the PHY speed and duplex settings to `hw.forced_speed_duplex`.
fn em_phy_force_speed_duplex(hw: &mut EmHw) -> Result<(), i32> {
    let mut mii_ctrl_reg: u16 = 0;
    let mut mii_status_reg: u16;
    let mut phy_data: u16 = 0;

    // Turn off Flow control if we are forcing speed and duplex.
    hw.fc = E1000_FC_NONE;

    // Read the Device Control Register.
    let mut ctrl = e1000_read_reg(hw, E1000_CTRL);

    // Set the bits to Force Speed and Duplex in the Device Ctrl Reg.
    ctrl |= E1000_CTRL_FRCSPD | E1000_CTRL_FRCDPX;
    ctrl &= !DEVICE_SPEED_MASK;

    // Clear the Auto Speed Detect Enable bit.
    ctrl &= !E1000_CTRL_ASDE;

    // Read the MII Control Register.
    em_read_phy_reg(hw, PHY_CTRL, &mut mii_ctrl_reg)?;

    // We need to disable autoneg in order to force link and duplex.
    mii_ctrl_reg &= !MII_CR_AUTO_NEG_EN;

    // Are we forcing Full or Half Duplex?
    if hw.forced_speed_duplex == em_100_full || hw.forced_speed_duplex == em_10_full {
        // We want to force full duplex so we SET the full duplex bits in the Device and MII
        // Control Registers.
        ctrl |= E1000_CTRL_FD;
        mii_ctrl_reg |= MII_CR_FULL_DUPLEX;
    } else {
        // We want to force half duplex so we CLEAR the full duplex bits in the Device and
        // MII Control Registers.
        ctrl &= !E1000_CTRL_FD;
        mii_ctrl_reg &= !MII_CR_FULL_DUPLEX;
    }

    // Are we forcing 100Mbps???
    if hw.forced_speed_duplex == em_100_full || hw.forced_speed_duplex == em_100_half {
        // Set the 100Mb bit and turn off the 1000Mb and 10Mb bits.
        ctrl |= E1000_CTRL_SPD_100;
        mii_ctrl_reg |= MII_CR_SPEED_100;
        mii_ctrl_reg &= !(MII_CR_SPEED_1000 | MII_CR_SPEED_10);
    } else {
        // Set the 10Mb bit and turn off the 1000Mb and 100Mb bits.
        ctrl &= !(E1000_CTRL_SPD_1000 | E1000_CTRL_SPD_100);
        mii_ctrl_reg |= MII_CR_SPEED_10;
        mii_ctrl_reg &= !(MII_CR_SPEED_1000 | MII_CR_SPEED_100);
    }

    em_config_collision_dist(hw);

    // Write the configured values back to the Device Control Reg.
    e1000_write_reg(hw, E1000_CTRL, ctrl);

    if hw.phy_type == em_phy_m88
        || hw.phy_type == em_phy_gg82563
        || hw.phy_type == em_phy_bm
        || hw.phy_type == em_phy_oem
        || hw.phy_type == em_phy_82578
    {
        em_read_phy_reg(hw, M88E1000_PHY_SPEC_CTRL, &mut phy_data)?;
        // Clear Auto-Crossover to force MDI manually. M88E1000 requires MDI forced whenever
        // speed are duplex are forced.
        phy_data &= !M88E1000_PSCR_AUTO_X_MODE;
        em_write_phy_reg(hw, M88E1000_PHY_SPEC_CTRL, phy_data)?;

        // Need to reset the PHY or these changes will be ignored.
        mii_ctrl_reg |= MII_CR_RESET;
    } else if hw.phy_type == em_phy_rtl8211 {
        if let Err(e) = em_read_phy_reg_ex(hw, RGEPHY_CR, &mut phy_data) {
            printf(format_args!("Unable to read RGEPHY_CR register\n"));
            return Err(e);
        }

        // Clear Auto-Crossover to force MDI manually. RTL8211 requires MDI forced whenever
        // speed are duplex are forced.
        phy_data |= RGEPHY_CR_MDI_MASK; // enable MDIX
        if let Err(e) = em_write_phy_reg_ex(hw, RGEPHY_CR, phy_data) {
            printf(format_args!("Unable to write RGEPHY_CR register\n"));
            return Err(e);
        }
        mii_ctrl_reg |= MII_CR_RESET;
    } else if hw.phy_type == em_phy_ife {
        // Disable MDI-X support for 10/100.
        em_read_phy_reg(hw, IFE_PHY_MDIX_CONTROL, &mut phy_data)?;

        phy_data &= !IFE_PMC_AUTO_MDIX;
        phy_data &= !IFE_PMC_FORCE_MDIX;

        em_write_phy_reg(hw, IFE_PHY_MDIX_CONTROL, phy_data)?;
    } else {
        // Clear Auto-Crossover to force MDI manually. IGP requires MDI forced whenever speed
        // or duplex are forced.
        em_read_phy_reg(hw, IGP01E1000_PHY_PORT_CTRL, &mut phy_data)?;

        phy_data &= !IGP01E1000_PSCR_AUTO_MDIX;
        phy_data &= !IGP01E1000_PSCR_FORCE_MDI_MDIX;

        em_write_phy_reg(hw, IGP01E1000_PHY_PORT_CTRL, phy_data)?;
    }

    // Write back the modified PHY MII control register.
    em_write_phy_reg(hw, PHY_CTRL, mii_ctrl_reg)?;

    usec_delay(1);
    // The wait_autoneg_complete flag may be a little misleading here. Since we are forcing
    // speed and duplex, Auto-Neg is not enabled. But we do want to delay for a period while
    // forcing only so we don't generate false No Link messages. So we will wait here only
    // if the user has set wait_autoneg_complete to 1, which is the default.
    if hw.wait_autoneg_complete != 0 {
        // We will wait for autoneg to complete.
        mii_status_reg = 0;
        // We will wait for autoneg to complete or 4.5 seconds to expire.
        let mut i = PHY_FORCE_TIME;
        while i > 0 {
            // Read the MII Status Register and wait for Auto-Neg Complete bit to be set.
            em_read_phy_reg(hw, PHY_STATUS, &mut mii_status_reg)?;
            em_read_phy_reg(hw, PHY_STATUS, &mut mii_status_reg)?;

            if mii_status_reg & MII_SR_LINK_STATUS != 0 {
                break;
            }
            msec_delay(100);
            i -= 1;
        }
        if i == 0
            && (hw.phy_type == em_phy_m88
                || hw.phy_type == em_phy_gg82563
                || hw.phy_type == em_phy_bm)
        {
            // We didn't get link. Reset the DSP and wait again for link.
            em_phy_reset_dsp(hw)?;
        }
        // This loop will early-out if the link condition has been met.
        let mut i = PHY_FORCE_TIME;
        while i > 0 {
            if mii_status_reg & MII_SR_LINK_STATUS != 0 {
                break;
            }
            msec_delay(100);
            // Read the MII Status Register and wait for Auto-Neg Complete bit to be set.
            em_read_phy_reg(hw, PHY_STATUS, &mut mii_status_reg)?;
            em_read_phy_reg(hw, PHY_STATUS, &mut mii_status_reg)?;
            i -= 1;
        }
    }
    if hw.phy_type == em_phy_m88 || hw.phy_type == em_phy_bm || hw.phy_type == em_phy_oem {
        // Because we reset the PHY above, we need to re-force TX_CLK in the Extended PHY
        // Specific Control Register to 25MHz clock. This value defaults back to a 2.5MHz
        // clock when the PHY is reset.
        em_read_phy_reg(hw, M88E1000_EXT_PHY_SPEC_CTRL, &mut phy_data)?;

        phy_data |= M88E1000_EPSCR_TX_CLK_25;
        em_write_phy_reg(hw, M88E1000_EXT_PHY_SPEC_CTRL, phy_data)?;
        // In addition, because of the s/w reset above, we need to enable CRS on TX. This
        // must be set for both full and half duplex operation.
        em_read_phy_reg(hw, M88E1000_PHY_SPEC_CTRL, &mut phy_data)?;

        if hw.phy_id == M88E1141_E_PHY_ID {
            phy_data &= !M88E1000_PSCR_ASSERT_CRS_ON_TX;
        } else {
            phy_data |= M88E1000_PSCR_ASSERT_CRS_ON_TX;
        }

        em_write_phy_reg(hw, M88E1000_PHY_SPEC_CTRL, phy_data)?;

        if (hw.mac_type == em_82544 || hw.mac_type == em_82543)
            && hw.autoneg == 0
            && (hw.forced_speed_duplex == em_10_full || hw.forced_speed_duplex == em_10_half)
        {
            em_polarity_reversal_workaround(hw)?;
        }
    } else if hw.phy_type == em_phy_rtl8211 {
        // In addition, because of the s/w reset above, we need to enable CRX on TX. This
        // must be set for both full and half duplex operation.
        if let Err(e) = em_read_phy_reg_ex(hw, RGEPHY_CR, &mut phy_data) {
            printf(format_args!("Unable to read RGEPHY_CR register\n"));
            return Err(e);
        }

        phy_data &= !RGEPHY_CR_ASSERT_CRS;
        if let Err(e) = em_write_phy_reg_ex(hw, RGEPHY_CR, phy_data) {
            printf(format_args!("Unable to write RGEPHY_CR register\n"));
            return Err(e);
        }
    } else if hw.phy_type == em_phy_gg82563 {
        // The TX_CLK of the Extended PHY Specific Control Register defaults to 2.5MHz on a
        // reset. We need to re-force it back to 25MHz, if we're not in a forced 10/duplex
        // configuration.
        em_read_phy_reg(hw, GG82563_PHY_MAC_SPEC_CTRL, &mut phy_data)?;

        phy_data &= !GG82563_MSCR_TX_CLK_MASK;
        if hw.forced_speed_duplex == em_10_full || hw.forced_speed_duplex == em_10_half {
            phy_data |= GG82563_MSCR_TX_CLK_10MBPS_2_5MHZ;
        } else {
            phy_data |= GG82563_MSCR_TX_CLK_100MBPS_25MHZ;
        }

        // Also due to the reset, we need to enable CRS on Tx.
        phy_data |= GG82563_MSCR_ASSERT_CRS_ON_TX;

        em_write_phy_reg(hw, GG82563_PHY_MAC_SPEC_CTRL, phy_data)?;
    }
    Ok(())
}

/// `em_config_collision_dist`: sets the collision distance in the Transmit Control register.
///
/// Link should have been established previously. Reads the speed and duplex information
/// from the Device Status register.
pub fn em_config_collision_dist(hw: &EmHw) {
    let coll_dist = if hw.mac_type < em_82543 {
        E1000_COLLISION_DISTANCE_82542
    } else {
        E1000_COLLISION_DISTANCE
    };

    let mut tctl = e1000_read_reg(hw, E1000_TCTL);

    tctl &= !E1000_TCTL_COLD;
    tctl |= coll_dist << E1000_COLD_SHIFT;

    e1000_write_reg(hw, E1000_TCTL, tctl);
    e1000_write_flush(hw);
}

/// Sets the MAC speed and duplex settings to reflect those in the PHY.
fn em_config_mac_to_phy(hw: &mut EmHw) -> Result<(), i32> {
    let mut phy_data: u16 = 0;
    // 82544 or newer MAC, Auto Speed Detection takes care of MAC speed/duplex
    // configuration.
    if hw.mac_type >= em_82544 && hw.mac_type != em_icp_xxxx {
        return Ok(());
    }
    // Read the Device Control Register and set the bits to Force Speed and Duplex.
    let mut ctrl = e1000_read_reg(hw, E1000_CTRL);
    ctrl |= E1000_CTRL_FRCSPD | E1000_CTRL_FRCDPX;
    ctrl &= !(E1000_CTRL_SPD_SEL | E1000_CTRL_ILOS);
    // Set up duplex in the Device Control and Transmit Control registers depending on
    // negotiated values.
    em_read_phy_reg(hw, M88E1000_PHY_SPEC_STATUS, &mut phy_data)?;

    if phy_data & M88E1000_PSSR_DPLX != 0 {
        ctrl |= E1000_CTRL_FD;
    } else {
        ctrl &= !E1000_CTRL_FD;
    }

    em_config_collision_dist(hw);
    // Set up speed in the Device Control register depending on negotiated values.
    if (phy_data & M88E1000_PSSR_SPEED) == M88E1000_PSSR_1000MBS {
        ctrl |= E1000_CTRL_SPD_1000;
    } else if (phy_data & M88E1000_PSSR_SPEED) == M88E1000_PSSR_100MBS {
        ctrl |= E1000_CTRL_SPD_100;
    }

    // Write the configured values back to the Device Control Reg.
    e1000_write_reg(hw, E1000_CTRL, ctrl);
    Ok(())
}

/// `em_force_mac_fc`: forces the MAC's flow control settings.
///
/// Sets the TFCE and RFCE bits in the device control register to reflect the adapter
/// settings. TFCE and RFCE need to be explicitly set by software when a Copper PHY is used
/// because autonegotiation is managed by the PHY rather than the MAC. Software must also
/// configure these bits when link is forced on a fiber connection.
pub fn em_force_mac_fc(hw: &EmHw) -> Result<(), i32> {
    // Get the current configuration of the Device Control Register.
    let mut ctrl = e1000_read_reg(hw, E1000_CTRL);
    // Because we didn't get link via the internal auto-negotiation mechanism (we either
    // forced link or we got link via PHY auto-neg), we have to manually enable/disable
    // transmit an receive flow control.
    //
    // The possible values of the "fc" parameter are: 0: Flow control is completely
    // disabled 1: Rx flow control is enabled (we can receive pause frames but not send
    // pause frames). 2: Tx flow control is enabled (we can send pause frames but we do not
    // receive pause frames). 3: Both Rx and TX flow control (symmetric) is enabled. other:
    // No other values should be possible at this point.
    match hw.fc {
        E1000_FC_NONE => ctrl &= !(E1000_CTRL_TFCE | E1000_CTRL_RFCE),
        E1000_FC_RX_PAUSE => {
            ctrl &= !E1000_CTRL_TFCE;
            ctrl |= E1000_CTRL_RFCE;
        }
        E1000_FC_TX_PAUSE => {
            ctrl &= !E1000_CTRL_RFCE;
            ctrl |= E1000_CTRL_TFCE;
        }
        E1000_FC_FULL => ctrl |= E1000_CTRL_TFCE | E1000_CTRL_RFCE,
        // Flow control param set incorrectly.
        _ => return Err(-E1000_ERR_CONFIG),
    }

    // Disable TX Flow Control for 82542 (rev 2.0).
    if hw.mac_type == em_82542_rev2_0 {
        ctrl &= !E1000_CTRL_TFCE;
    }

    e1000_write_reg(hw, E1000_CTRL, ctrl);
    Ok(())
}

/// Configures the flow control settings after link is established.
///
/// Should be called immediately after a valid link has been established. Forces MAC flow
/// control settings if link was forced. When in MII/GMII mode and autonegotiation is
/// enabled, the MAC flow control settings will be set based on the flow control negotiated
/// by the PHY. In TBI mode, the TFCE and RFCE bits will be automatically set to the
/// negotiated flow control mode.
fn em_config_fc_after_link_up(hw: &mut EmHw) -> Result<(), i32> {
    let mut mii_status_reg: u16 = 0;
    let mut mii_nway_adv_reg: u16 = 0;
    let mut mii_nway_lp_ability_reg: u16 = 0;
    let mut speed: u16 = 0;
    let mut duplex: u16 = 0;
    // Check for the case where we have fiber media and auto-neg failed so we had to force
    // link. In this case, we need to force the configuration of the MAC to match the "fc"
    // parameter.
    if (hw.media_type == em_media_type_fiber && hw.autoneg_failed != 0)
        || (hw.media_type == em_media_type_internal_serdes && hw.autoneg_failed != 0)
        || (hw.media_type == em_media_type_copper && hw.autoneg == 0)
        || (hw.media_type == em_media_type_oem && hw.autoneg == 0)
    {
        em_force_mac_fc(hw)?;
    }
    // Check for the case where we have copper media and auto-neg is enabled. In this case,
    // we need to check and see if Auto-Neg has completed, and if so, how the PHY and link
    // partner has flow control configured.
    if (hw.media_type == em_media_type_copper || hw.media_type == em_media_type_oem)
        && hw.autoneg != 0
    {
        // Read the MII Status Register and check to see if AutoNeg has completed. We read
        // this twice because this reg has some "sticky" (latched) bits.
        em_read_phy_reg(hw, PHY_STATUS, &mut mii_status_reg)?;
        em_read_phy_reg(hw, PHY_STATUS, &mut mii_status_reg)?;

        if mii_status_reg & MII_SR_AUTONEG_COMPLETE != 0 {
            // The AutoNeg process has completed, so we now need to read both the Auto
            // Negotiation Advertisement Register (Address 4) and the Auto_Negotiation Base
            // Page Ability Register (Address 5) to determine how flow control was
            // negotiated.
            em_read_phy_reg(hw, PHY_AUTONEG_ADV, &mut mii_nway_adv_reg)?;
            em_read_phy_reg(hw, PHY_LP_ABILITY, &mut mii_nway_lp_ability_reg)?;
            // Two bits in the Auto Negotiation Advertisement Register (Address 4) and two
            // bits in the Auto Negotiation Base Page Ability Register (Address 5) determine
            // flow control for both the PHY and the link partner. The following table,
            // taken out of the IEEE 802.3ab/D6.0 dated March 25, 1999, describes these
            // PAUSE resolution bits and how flow control is determined based upon these
            // settings. NOTE: DC = Don't Care
            //   LOCAL DEVICE   |   LINK PARTNER  |
            //  PAUSE | ASM_DIR | PAUSE | ASM_DIR | NIC Resolution
            // -------|---------|-------|---------|---------------
            //    0   |    0    |  DC   |   DC    | em_fc_none
            //    0   |    1    |   0   |   DC    | em_fc_none
            //    0   |    1    |   1   |    0    | em_fc_none
            //    0   |    1    |   1   |    1    | em_fc_tx_pause
            //    1   |    0    |   0   |   DC    | em_fc_none
            //    1   |   DC    |   1   |   DC    | em_fc_full
            //    1   |    1    |   0   |    0    | em_fc_none
            //    1   |    1    |   0   |    1    | em_fc_rx_pause
            if mii_nway_adv_reg & NWAY_AR_PAUSE != 0
                && mii_nway_lp_ability_reg & NWAY_LPAR_PAUSE != 0
            {
                // Are both PAUSE bits set to 1? If so, this implies Symmetric Flow Control
                // is enabled at both ends. The ASM_DIR bits are irrelevant per the spec.
                //
                // Now we need to check if the user selected RX ONLY of pause frames. In
                // this case, we had to advertise FULL flow control because we could not
                // advertise RX ONLY. Hence, we must now check to see if we need to turn
                // OFF the TRANSMISSION of PAUSE frames.
                if hw.original_fc == E1000_FC_FULL {
                    hw.fc = E1000_FC_FULL;
                } else {
                    hw.fc = E1000_FC_RX_PAUSE;
                }
            } else if mii_nway_adv_reg & NWAY_AR_PAUSE == 0
                && mii_nway_adv_reg & NWAY_AR_ASM_DIR != 0
                && mii_nway_lp_ability_reg & NWAY_LPAR_PAUSE != 0
                && mii_nway_lp_ability_reg & NWAY_LPAR_ASM_DIR != 0
            {
                // For receiving PAUSE frames ONLY (0 1 1 1: em_fc_tx_pause).
                hw.fc = E1000_FC_TX_PAUSE;
            } else if mii_nway_adv_reg & NWAY_AR_PAUSE != 0
                && mii_nway_adv_reg & NWAY_AR_ASM_DIR != 0
                && mii_nway_lp_ability_reg & NWAY_LPAR_PAUSE == 0
                && mii_nway_lp_ability_reg & NWAY_LPAR_ASM_DIR != 0
            {
                // For transmitting PAUSE frames ONLY (1 1 0 1: em_fc_rx_pause).
                hw.fc = E1000_FC_RX_PAUSE;
            } else if hw.original_fc == E1000_FC_NONE
                || hw.original_fc == E1000_FC_TX_PAUSE
                || hw.fc_strict_ieee
            {
                // Per the IEEE spec, at this point flow control should be disabled.
                // However, we want to consider that we could be connected to a legacy
                // switch that doesn't advertise desired flow control, but can be forced on
                // the link partner. So if we advertised no flow control, that is what we
                // will resolve to. If we advertised some kind of receive capability (Rx
                // Pause Only or Full Flow Control) and the link partner advertised none, we
                // will configure ourselves to enable Rx Flow Control only. We can do this
                // safely for two reasons: If the link partner really didn't want flow
                // control enabled, and we enable Rx, no harm done since we won't be
                // receiving any PAUSE frames anyway. If the intent on the link partner was
                // to have flow control enabled, then by us enabling RX only, we can at
                // least receive pause frames and process them. This is a good idea because
                // in most cases, since we are predominantly a server NIC, more times than
                // not we will be asked to delay transmission of packets than asking our
                // link partner to pause transmission of frames.
                hw.fc = E1000_FC_NONE;
            } else {
                hw.fc = E1000_FC_RX_PAUSE;
            }
            // Now we need to do one last check... If we auto-negotiated to HALF DUPLEX,
            // flow control should not be enabled per IEEE 802.3 spec.
            em_get_speed_and_duplex(hw, &mut speed, &mut duplex)?;
            if duplex == HALF_DUPLEX {
                hw.fc = E1000_FC_NONE;
            }
            // Now we call a subroutine to actually force the MAC controller to use the
            // correct flow control settings.
            em_force_mac_fc(hw)?;
        }
        // Otherwise the copper PHY's auto-negotiation has not completed.
    }
    Ok(())
}

/// `em_check_for_link`: checks to see if the link status of the hardware has changed.
///
/// Called by any function that needs to check the link status of the adapter.
pub fn em_check_for_link(hw: &mut EmHw) -> Result<(), i32> {
    let mut rxcw: u32 = 0;
    let mut signal: u32 = 0;
    let mut phy_data: u16 = 0;
    let mut speed: u16 = 0;
    let mut duplex: u16 = 0;

    if hw.mac_type >= em_82575 && hw.media_type != em_media_type_copper {
        let ret_val = em_get_pcs_speed_and_duplex_82575(hw, &mut speed, &mut duplex);
        hw.get_link_status = hw.serdes_link_down;

        return ret_val;
    }

    let mut ctrl = e1000_read_reg(hw, E1000_CTRL);
    let status = e1000_read_reg(hw, E1000_STATUS);
    // On adapters with a MAC newer than 82544, SW Definable pin 1 will be set when the
    // optics detect a signal. On older adapters, it will be cleared when there is a signal.
    // This applies to fiber media only.
    if hw.media_type == em_media_type_fiber || hw.media_type == em_media_type_internal_serdes {
        rxcw = e1000_read_reg(hw, E1000_RXCW);

        if hw.media_type == em_media_type_fiber {
            signal = if hw.mac_type > em_82544 {
                E1000_CTRL_SWDPIN1
            } else {
                0
            };
            if status & E1000_STATUS_LU != 0 {
                hw.get_link_status = false;
            }
        }
    }
    // If we have a copper PHY then we only want to go out to the PHY registers to see if
    // Auto-Neg has completed and/or if our link status has changed. The get_link_status
    // flag will be set if we receive a Link Status Change interrupt or we have Rx Sequence
    // Errors.
    if (hw.media_type == em_media_type_copper || hw.media_type == em_media_type_oem)
        && hw.get_link_status
    {
        // First we want to see if the MII Status Register reports link. If so, then we want
        // to get the current speed/duplex of the PHY. Read the register twice since the
        // link bit is sticky.
        em_read_phy_reg(hw, PHY_STATUS, &mut phy_data)?;
        em_read_phy_reg(hw, PHY_STATUS, &mut phy_data)?;

        hw.icp_xxxx_is_link_up = (phy_data & MII_SR_LINK_STATUS) != 0;

        if hw.mac_type == em_pchlan {
            let up = hw.icp_xxxx_is_link_up;
            em_k1_gig_workaround_hv(hw, up)?;
        }

        if phy_data & MII_SR_LINK_STATUS != 0 {
            hw.get_link_status = false;

            if hw.phy_type == em_phy_82578 {
                em_link_stall_workaround_hv(hw)?;
            }

            if hw.mac_type == em_pch2lan {
                em_k1_workaround_lv(hw)?;
            }
            // Work-around I218 hang issue.
            if hw.device_id == E1000_DEV_ID_PCH_LPTLP_I218_LM
                || hw.device_id == E1000_DEV_ID_PCH_LPTLP_I218_V
                || hw.device_id == E1000_DEV_ID_PCH_I218_LM3
                || hw.device_id == E1000_DEV_ID_PCH_I218_V3
            {
                let up = hw.icp_xxxx_is_link_up;
                em_k1_workaround_lpt_lp(hw, up)?;
            }

            // Check if there was DownShift, must be checked immediately after link-up.
            let _ = em_check_downshift(hw);

            // Enable/Disable EEE after link up.
            if hw.mac_type >= em_pch2lan {
                em_set_eee_pchlan(hw)?;
            }

            // If we are on 82544 or 82543 silicon and speed/duplex are forced to 10H or
            // 10F, then we will implement the polarity reversal workaround. We disable
            // interrupts first, and upon returning, place the devices interrupt state to
            // its previous value except for the link status change interrupt which will
            // happen due to the execution of this workaround.
            if (hw.mac_type == em_82544 || hw.mac_type == em_82543)
                && hw.autoneg == 0
                && (hw.forced_speed_duplex == em_10_full || hw.forced_speed_duplex == em_10_half)
            {
                e1000_write_reg(hw, E1000_IMC, 0xffffffff);
                // The C overwrites this status unread.
                let _ = em_polarity_reversal_workaround(hw);
                let icr = e1000_read_reg(hw, E1000_ICR);
                e1000_write_reg(hw, E1000_ICS, icr & !E1000_ICS_LSC);
                e1000_write_reg(hw, E1000_IMS, IMS_ENABLE_MASK);
            }
        } else {
            // No link detected.
            let _ = em_config_dsp_after_link_change(hw, false);
            return Ok(());
        }
        // If we are forcing speed/duplex, then we simply return since we have already
        // determined whether we have link or not.
        if hw.autoneg == 0 {
            return Err(-E1000_ERR_CONFIG);
        }

        // Optimize the dsp settings for the igp phy.
        let _ = em_config_dsp_after_link_change(hw, true);
        // We have a M88E1000 PHY and Auto-Neg is enabled. If we have Si on board that is
        // 82544 or newer, Auto Speed Detection takes care of MAC speed/duplex
        // configuration. So we only need to configure Collision Distance in the MAC.
        // Otherwise, we need to force speed/duplex on the MAC to the current PHY
        // speed/duplex settings.
        if hw.mac_type >= em_82544 && hw.mac_type != em_icp_xxxx {
            em_config_collision_dist(hw);
        } else {
            em_config_mac_to_phy(hw)?;
        }
        // Configure Flow Control now that Auto-Neg has completed. First, we need to restore
        // the desired flow control settings because we may have had to re-autoneg with a
        // different link partner.
        em_config_fc_after_link_up(hw)?;
        // At this point we know that we are on copper and we have auto-negotiated link.
        // These are conditions for checking the link partner capability register. We use
        // the link speed to determine if TBI compatibility needs to be turned on or off. If
        // the link is not at gigabit speed, then TBI compatibility is not needed. If we are
        // at gigabit speed, we turn on TBI compatibility.
        if hw.tbi_compatibility_en {
            let mut speed: u16 = 0;
            let mut duplex: u16 = 0;
            em_get_speed_and_duplex(hw, &mut speed, &mut duplex)?;
            if speed != SPEED_1000 {
                // If link speed is not set to gigabit speed, we do not need to enable TBI
                // compatibility.
                if hw.tbi_compatibility_on {
                    // If we previously were in the mode, turn it off.
                    let mut rctl = e1000_read_reg(hw, E1000_RCTL);
                    rctl &= !E1000_RCTL_SBP;
                    e1000_write_reg(hw, E1000_RCTL, rctl);
                    hw.tbi_compatibility_on = false;
                }
            } else {
                // If TBI compatibility is was previously off, turn it on. For compatibility
                // with a TBI link partner, we will store bad packets. Some frames have an
                // additional byte on the end and will look like CRC errors to the hardware.
                if !hw.tbi_compatibility_on {
                    hw.tbi_compatibility_on = true;
                    let mut rctl = e1000_read_reg(hw, E1000_RCTL);
                    rctl |= E1000_RCTL_SBP;
                    e1000_write_reg(hw, E1000_RCTL, rctl);
                }
            }
        }
    } else if ((hw.media_type == em_media_type_fiber && (ctrl & E1000_CTRL_SWDPIN1) == signal)
        || hw.media_type == em_media_type_internal_serdes)
        && status & E1000_STATUS_LU == 0
        && rxcw & E1000_RXCW_C == 0
    {
        // If we don't have link (auto-negotiation failed or link partner cannot
        // auto-negotiate), the cable is plugged in (we have signal), and our link partner is
        // not trying to auto-negotiate with us (we are receiving idles or data), we need to
        // force link up. We also need to give auto-negotiation time to complete, in case the
        // cable was just plugged in. The autoneg_failed flag does this.
        if hw.autoneg_failed == 0 {
            hw.autoneg_failed = 1;
            return Ok(());
        }
        // NOT RXing /C/, disable AutoNeg and force link.

        // Disable auto-negotiation in the TXCW register.
        e1000_write_reg(hw, E1000_TXCW, hw.txcw & !E1000_TXCW_ANE);

        // Force link-up and also force full-duplex.
        ctrl = e1000_read_reg(hw, E1000_CTRL);
        ctrl |= E1000_CTRL_SLU | E1000_CTRL_FD;
        e1000_write_reg(hw, E1000_CTRL, ctrl);

        // Configure Flow Control after forcing link up.
        em_config_fc_after_link_up(hw)?;
    } else if (hw.media_type == em_media_type_fiber
        || hw.media_type == em_media_type_internal_serdes)
        && ctrl & E1000_CTRL_SLU != 0
        && rxcw & E1000_RXCW_C != 0
    {
        // If we are forcing link and we are receiving /C/ ordered sets, re-enable
        // auto-negotiation in the TXCW register and disable forced link in the Device
        // Control register in an attempt to auto-negotiate with our link partner.
        e1000_write_reg(hw, E1000_TXCW, hw.txcw);
        e1000_write_reg(hw, E1000_CTRL, ctrl & !E1000_CTRL_SLU);

        hw.serdes_link_down = false;
    } else if hw.media_type == em_media_type_internal_serdes
        && E1000_TXCW_ANE & e1000_read_reg(hw, E1000_TXCW) == 0
    {
        // If we force link for non-auto-negotiation switch, check link status based on MAC
        // synchronization for internal serdes media type.

        // SYNCH bit and IV bit are sticky.
        usec_delay(10);
        if E1000_RXCW_SYNCH & e1000_read_reg(hw, E1000_RXCW) != 0 {
            if rxcw & E1000_RXCW_IV == 0 {
                hw.serdes_link_down = false;
            }
        } else {
            hw.serdes_link_down = true;
        }
    }
    if hw.media_type == em_media_type_internal_serdes
        && E1000_TXCW_ANE & e1000_read_reg(hw, E1000_TXCW) != 0
    {
        hw.serdes_link_down = E1000_STATUS_LU & e1000_read_reg(hw, E1000_STATUS) == 0;
    }
    Ok(())
}

/// `em_get_pcs_speed_and_duplex_82575`: the speed and duplex of an 82575 or later port that
/// is not copper, from the PCS link status (the device status register is not accurate in
/// that mode); sets `serdes_link_down`.
pub fn em_get_pcs_speed_and_duplex_82575(
    hw: &mut EmHw,
    speed: &mut u16,
    duplex: &mut u16,
) -> Result<(), i32> {
    hw.serdes_link_down = true;
    *speed = 0;
    *duplex = 0;

    // Read the PCS Status register for link state. For non-copper mode, the status register
    // is not accurate. The PCS status register is used instead.
    let pcs = e1000_read_reg(hw, E1000_PCS_LSTAT);

    // The link up bit determines when link is up on autoneg. The sync ok gets set once both
    // sides sync up and agree upon link. Stable link can be determined by checking for both
    // link up and link sync ok.
    if pcs & E1000_PCS_LSTS_LINK_OK != 0 && pcs & E1000_PCS_LSTS_SYNK_OK != 0 {
        hw.serdes_link_down = false;

        // Detect and store PCS speed.
        if pcs & E1000_PCS_LSTS_SPEED_1000 != 0 {
            *speed = SPEED_1000;
        } else if pcs & E1000_PCS_LSTS_SPEED_100 != 0 {
            *speed = SPEED_100;
        } else {
            *speed = SPEED_10;
        }

        // Detect and store PCS duplex.
        if pcs & E1000_PCS_LSTS_DUPLEX_FULL != 0 {
            *duplex = FULL_DUPLEX;
        } else {
            *duplex = HALF_DUPLEX;
        }
    }

    Ok(())
}

/// `em_get_speed_and_duplex`: detects the current speed and duplex settings of the
/// hardware.
pub fn em_get_speed_and_duplex(
    hw: &mut EmHw,
    speed: &mut u16,
    duplex: &mut u16,
) -> Result<(), i32> {
    let mut phy_data: u16 = 0;

    if hw.mac_type >= em_82575 && hw.media_type != em_media_type_copper {
        return em_get_pcs_speed_and_duplex_82575(hw, speed, duplex);
    }

    if hw.mac_type >= em_82543 {
        let status = e1000_read_reg(hw, E1000_STATUS);
        if status & E1000_STATUS_SPEED_1000 != 0 {
            *speed = SPEED_1000;
        } else if status & E1000_STATUS_SPEED_100 != 0 {
            *speed = SPEED_100;
        } else {
            *speed = SPEED_10;
        }

        if status & E1000_STATUS_FD != 0 {
            *duplex = FULL_DUPLEX;
        } else {
            *duplex = HALF_DUPLEX;
        }
    } else {
        *speed = SPEED_1000;
        *duplex = FULL_DUPLEX;
    }
    // IGP01 PHY may advertise full duplex operation after speed downgrade even if it is
    // operating at half duplex. Here we set the duplex settings to match the duplex in the
    // link partner's capabilities.
    if hw.phy_type == em_phy_igp && hw.speed_downgraded {
        em_read_phy_reg(hw, PHY_AUTONEG_EXP, &mut phy_data)?;

        if phy_data & NWAY_ER_LP_NWAY_CAPS == 0 {
            *duplex = HALF_DUPLEX;
        } else {
            em_read_phy_reg(hw, PHY_LP_ABILITY, &mut phy_data)?;
            if (*speed == SPEED_100 && phy_data & NWAY_LPAR_100TX_FD_CAPS == 0)
                || (*speed == SPEED_10 && phy_data & NWAY_LPAR_10T_FD_CAPS == 0)
            {
                *duplex = HALF_DUPLEX;
            }
        }
    }
    if hw.mac_type == em_80003es2lan && hw.media_type == em_media_type_copper {
        if *speed == SPEED_1000 {
            em_configure_kmrn_for_1000(hw)?;
        } else {
            em_configure_kmrn_for_10_100(hw, *duplex)?;
        }
    }
    if hw.mac_type == em_ich8lan && hw.phy_type == em_phy_igp_3 && *speed == SPEED_1000 {
        em_kumeran_lock_loss_workaround(hw)?;
    }
    Ok(())
}

/// Blocks until autoneg completes or times out (~4.5 seconds).
fn em_wait_autoneg(hw: &mut EmHw) -> Result<(), i32> {
    let mut phy_data: u16 = 0;

    // We will wait for autoneg to complete or 4.5 seconds to expire.
    for _ in 0..PHY_AUTO_NEG_TIME {
        // Read the MII Status Register and wait for Auto-Neg Complete bit to be set.
        em_read_phy_reg(hw, PHY_STATUS, &mut phy_data)?;
        em_read_phy_reg(hw, PHY_STATUS, &mut phy_data)?;
        if phy_data & MII_SR_AUTONEG_COMPLETE != 0 {
            return Ok(());
        }
        msec_delay(100);
    }
    Ok(())
}

/// Raises the Management Data Clock (by setting the MDC bit), and then delays 10
/// microseconds. `ctrl` is the device control register's current value.
fn em_raise_mdi_clk(hw: &EmHw, ctrl: &mut u32) {
    e1000_write_reg(hw, E1000_CTRL, *ctrl | E1000_CTRL_MDC);
    e1000_write_flush(hw);
    usec_delay(10);
}

/// Lowers the Management Data Clock (by clearing the MDC bit), and then delays 10
/// microseconds. `ctrl` is the device control register's current value.
fn em_lower_mdi_clk(hw: &EmHw, ctrl: &mut u32) {
    e1000_write_reg(hw, E1000_CTRL, *ctrl & !E1000_CTRL_MDC);
    e1000_write_flush(hw);
    usec_delay(10);
}

/// Shifts `count` data bits out to the PHY, MSB first.
fn em_shift_out_mdi_bits(hw: &EmHw, data: u32, count: u16) {
    // We need to shift "count" number of bits out to the PHY. So, the value in the "data"
    // parameter will be shifted out to the PHY one bit at a time. In order to do this,
    // "data" must be broken down into bits.
    let mut mask: u32 = 0x01;
    mask <<= count.wrapping_sub(1);

    let mut ctrl = e1000_read_reg(hw, E1000_CTRL);

    // Set MDIO_DIR and MDC_DIR direction bits to be used as output pins.
    ctrl |= E1000_CTRL_MDIO_DIR | E1000_CTRL_MDC_DIR;

    while mask != 0 {
        // A "1" is shifted out to the PHY by setting the MDIO bit to "1" and then raising
        // and lowering the Management Data Clock. A "0" is shifted out to the PHY by setting
        // the MDIO bit to "0" and then raising and lowering the clock.
        if data & mask != 0 {
            ctrl |= E1000_CTRL_MDIO;
        } else {
            ctrl &= !E1000_CTRL_MDIO;
        }

        e1000_write_reg(hw, E1000_CTRL, ctrl);
        e1000_write_flush(hw);

        usec_delay(10);

        em_raise_mdi_clk(hw, &mut ctrl);
        em_lower_mdi_clk(hw, &mut ctrl);

        mask >>= 1;
    }
}

/// Shifts 16 data bits in from the PHY, MSB first.
fn em_shift_in_mdi_bits(hw: &EmHw) -> u16 {
    let mut data: u16 = 0;
    // In order to read a register from the PHY, we need to shift in a total of 18 bits from
    // the PHY. The first two bit (turnaround) times are used to avoid contention on the
    // MDIO pin when a read operation is performed. These two bits are ignored by us and
    // thrown away. Bits are "shifted in" by raising the input to the Management Data Clock
    // (setting the MDC bit), and then reading the value of the MDIO bit.
    let mut ctrl = e1000_read_reg(hw, E1000_CTRL);
    // Clear MDIO_DIR (SWDPIO1) to indicate this bit is to be used as input.
    ctrl &= !E1000_CTRL_MDIO_DIR;
    ctrl &= !E1000_CTRL_MDIO;

    e1000_write_reg(hw, E1000_CTRL, ctrl);
    e1000_write_flush(hw);
    // Raise and Lower the clock before reading in the data. This accounts for the
    // turnaround bits. The first clock occurred when we clocked out the last bit of the
    // Register Address.
    em_raise_mdi_clk(hw, &mut ctrl);
    em_lower_mdi_clk(hw, &mut ctrl);

    for _ in 0..16 {
        data <<= 1;
        em_raise_mdi_clk(hw, &mut ctrl);
        ctrl = e1000_read_reg(hw, E1000_CTRL);
        // Check to see if we shifted in a "1".
        if ctrl & E1000_CTRL_MDIO != 0 {
            data |= 1;
        }
        em_lower_mdi_clk(hw, &mut ctrl);
    }

    em_raise_mdi_clk(hw, &mut ctrl);
    em_lower_mdi_clk(hw, &mut ctrl);

    data
}

/// Acquires the software/firmware semaphore of the resources in `mask` (`SWFW_*_SM`): the
/// ICH software flag, the EEPROM semaphore, or the `SW_FW_SYNC` register by MAC.
fn em_swfw_sync_acquire(hw: &mut EmHw, mask: u16) -> Result<(), i32> {
    let mut swfw_sync: u32 = 0;
    let swmask = u32::from(mask);
    let fwmask = u32::from(mask) << 16;
    let mut timeout: i32 = 200;

    if hw.swfwhw_semaphore_present != 0 {
        return em_get_software_flag(hw);
    }

    if hw.swfw_sync_present == 0 {
        return em_get_hw_eeprom_semaphore(hw);
    }

    while timeout != 0 {
        if em_get_hw_eeprom_semaphore(hw).is_err() {
            return Err(-E1000_ERR_SWFW_SYNC);
        }

        swfw_sync = e1000_read_reg(hw, E1000_SW_FW_SYNC);
        if swfw_sync & (fwmask | swmask) == 0 {
            break;
        }
        // Firmware currently using resource (fwmask) or other software thread currently
        // using resource (swmask).
        em_put_hw_eeprom_semaphore(hw);
        msec_delay_irq(5);
        timeout -= 1;
    }

    if timeout == 0 {
        // Driver can't access resource, SW_FW_SYNC timeout.
        return Err(-E1000_ERR_SWFW_SYNC);
    }
    swfw_sync |= swmask;
    e1000_write_reg(hw, E1000_SW_FW_SYNC, swfw_sync);

    em_put_hw_eeprom_semaphore(hw);
    Ok(())
}

/// Releases the software/firmware semaphore of the resources in `mask`.
fn em_swfw_sync_release(hw: &mut EmHw, mask: u16) {
    let swmask = u32::from(mask);

    if hw.swfwhw_semaphore_present != 0 {
        em_release_software_flag(hw);
        return;
    }
    if hw.swfw_sync_present == 0 {
        em_put_hw_eeprom_semaphore(hw);
        return;
    }
    // if (em_get_hw_eeprom_semaphore(hw)) return -E1000_ERR_SWFW_SYNC;
    while em_get_hw_eeprom_semaphore(hw).is_err() {}

    let mut swfw_sync = e1000_read_reg(hw, E1000_SW_FW_SYNC);
    swfw_sync &= !swmask;
    e1000_write_reg(hw, E1000_SW_FW_SYNC, swfw_sync);

    em_put_hw_eeprom_semaphore(hw);
}

/// `em_access_phy_wakeup_reg_bm`: reads or writes a BM PHY wakeup register:
/// 1. set page 769, register 17, bit 2 = 1;
/// 2. set page to 800 for host (801 if we were manageability);
/// 3. write the address using the address opcode (0x11);
/// 4. read or write the data using the data opcode (0x12);
/// 5. restore 769_17.2 to its original value.
pub fn em_access_phy_wakeup_reg_bm(
    hw: &mut EmHw,
    reg_addr: u32,
    phy_data: &mut u16,
    read: bool,
) -> Result<(), i32> {
    let reg = bm_phy_reg_num(reg_addr);
    let mut phy_reg: u16 = 0;

    // All operations in this function are phy address 1.
    hw.phy_addr = 1;

    // Set page 769.
    let _ = em_write_phy_reg_ex(
        hw,
        IGP01E1000_PHY_PAGE_SELECT,
        (BM_WUC_ENABLE_PAGE << PHY_PAGE_SHIFT) as u16,
    );

    em_read_phy_reg_ex(hw, BM_WUC_ENABLE_REG, &mut phy_reg)?;

    // First clear bit 4 to avoid a power state change.
    phy_reg &= !BM_WUC_HOST_WU_BIT;
    em_write_phy_reg_ex(hw, BM_WUC_ENABLE_REG, phy_reg)?;

    // Write bit 2 = 1, and clear bit 4 to 769_17.
    em_write_phy_reg_ex(hw, BM_WUC_ENABLE_REG, phy_reg | BM_WUC_ENABLE_BIT)?;

    // Select page 800 (the C overwrites this status unread).
    let _ = em_write_phy_reg_ex(
        hw,
        IGP01E1000_PHY_PAGE_SELECT,
        (BM_WUC_PAGE << PHY_PAGE_SHIFT) as u16,
    );

    // Write the page 800 offset value using opcode 0x11.
    em_write_phy_reg_ex(hw, BM_WUC_ADDRESS_OPCODE, reg)?;

    if read {
        // Read the page 800 value using opcode 0x12.
        em_read_phy_reg_ex(hw, BM_WUC_DATA_OPCODE, phy_data)?;
    } else {
        // Write the page 800 value using opcode 0x12.
        em_write_phy_reg_ex(hw, BM_WUC_DATA_OPCODE, *phy_data)?;
    }

    // Restore 769_17.2 to its original value. Set page 769.
    let _ = em_write_phy_reg_ex(
        hw,
        IGP01E1000_PHY_PAGE_SELECT,
        (BM_WUC_ENABLE_PAGE << PHY_PAGE_SHIFT) as u16,
    );

    // Clear 769_17.2.
    em_write_phy_reg_ex(hw, BM_WUC_ENABLE_REG, phy_reg)
}

/// `em_access_phy_debug_regs_hv`: reads or writes HV PHY vendor specific high registers.
pub fn em_access_phy_debug_regs_hv(
    hw: &mut EmHw,
    reg_addr: u32,
    phy_data: &mut u16,
    read: bool,
) -> Result<(), i32> {
    // This takes care of the difference with desktop vs mobile phy.
    let addr_reg = if hw.phy_type == em_phy_82578 {
        I82578_PHY_ADDR_REG
    } else {
        I82577_PHY_ADDR_REG
    };
    let data_reg = addr_reg + 1;

    // All operations in this function are phy address 2.
    hw.phy_addr = 2;

    // Masking with 0x3F to remove the page from offset.
    if let Err(e) = em_write_phy_reg_ex(hw, addr_reg, (reg_addr as u16) & 0x3F) {
        printf(format_args!(
            "Could not write PHY the HV address register\n"
        ));
        return Err(e);
    }

    // Read or write the data value next.
    let ret_val = if read {
        em_read_phy_reg_ex(hw, data_reg, phy_data)
    } else {
        em_write_phy_reg_ex(hw, data_reg, *phy_data)
    };

    if ret_val.is_err() {
        printf(format_args!(
            "Could not read data value from HV data register\n"
        ));
    }
    ret_val
}

/// `em_access_phy_reg_hv`: reads or writes the value of a PHY register; if the value is on
/// a specific non zero page, sets the page first.
pub fn em_access_phy_reg_hv(
    hw: &mut EmHw,
    reg_addr: u32,
    phy_data: &mut u16,
    read: bool,
) -> Result<(), i32> {
    let mut page = bm_phy_reg_page(reg_addr);
    let reg = bm_phy_reg_num(reg_addr);

    let swfw = E1000_SWFW_PHY0_SM;

    if em_swfw_sync_acquire(hw, swfw).is_err() {
        return Err(-E1000_ERR_SWFW_SYNC);
    }

    let ret_val = 'release: {
        if u32::from(page) == BM_WUC_PAGE {
            break 'release em_access_phy_wakeup_reg_bm(hw, reg_addr, phy_data, read);
        }

        if u32::from(page) >= HV_INTC_FC_PAGE_START {
            hw.phy_addr = 1;
        } else {
            hw.phy_addr = 2;
        }

        if u32::from(page) == HV_INTC_FC_PAGE_START {
            page = 0;
        }

        // Workaround MDIO accesses being disabled after entering IEEE Power Down (whenever
        // bit 11 of the PHY Control register is set).
        if !read
            && hw.phy_type == em_phy_82578
            && hw.phy_revision >= 1
            && hw.phy_addr == 2
            && (MAX_PHY_REG_ADDRESS & u32::from(reg)) == 0
            && *phy_data & (1 << 11) != 0
        {
            let mut data2: u16 = 0x7EFF;

            // The C returns here without releasing the semaphore.
            em_access_phy_debug_regs_hv(hw, (1 << 6) | 0x3, &mut data2, false)?;
        }

        if reg_addr > MAX_PHY_MULTI_PAGE_REG {
            // The C returns here without releasing the semaphore.
            em_write_phy_reg_ex(
                hw,
                IGP01E1000_PHY_PAGE_SELECT,
                (u32::from(page) << PHY_PAGE_SHIFT) as u16,
            )?;
        }
        if read {
            em_read_phy_reg_ex(hw, MAX_PHY_REG_ADDRESS & u32::from(reg), phy_data)
        } else {
            em_write_phy_reg_ex(hw, MAX_PHY_REG_ADDRESS & u32::from(reg), *phy_data)
        }
    };
    em_swfw_sync_release(hw, swfw);
    ret_val
}

/// `em_read_phy_reg`: reads the value of a PHY register; if the value is on a specific non
/// zero page, sets the page first.
pub fn em_read_phy_reg(hw: &mut EmHw, reg_addr: u32, phy_data: &mut u16) -> Result<(), i32> {
    if hw.mac_type >= em_pchlan {
        return em_access_phy_reg_hv(hw, reg_addr, phy_data, true);
    }

    let swfw =
        if (hw.mac_type == em_80003es2lan || hw.mac_type == em_82575 || hw.mac_type == em_82576)
            && e1000_read_reg(hw, E1000_STATUS) & E1000_STATUS_FUNC_1 != 0
        {
            E1000_SWFW_PHY1_SM
        } else {
            E1000_SWFW_PHY0_SM
        };
    if em_swfw_sync_acquire(hw, swfw).is_err() {
        return Err(-E1000_ERR_SWFW_SYNC);
    }

    if (hw.phy_type == em_phy_igp || hw.phy_type == em_phy_igp_3 || hw.phy_type == em_phy_igp_2)
        && reg_addr > MAX_PHY_MULTI_PAGE_REG
    {
        if let Err(e) = em_write_phy_reg_ex(hw, IGP01E1000_PHY_PAGE_SELECT, reg_addr as u16) {
            em_swfw_sync_release(hw, swfw);
            return Err(e);
        }
    } else if hw.phy_type == em_phy_gg82563 {
        if (reg_addr & MAX_PHY_REG_ADDRESS) > MAX_PHY_MULTI_PAGE_REG
            || hw.mac_type == em_80003es2lan
        {
            let page = (reg_addr as u16) >> GG82563_PAGE_SHIFT;
            // Select Configuration Page.
            let ret_val = if (reg_addr & MAX_PHY_REG_ADDRESS) < GG82563_MIN_ALT_REG {
                em_write_phy_reg_ex(hw, GG82563_PHY_PAGE_SELECT, page)
            } else {
                // Use Alternative Page Select register to access registers 30 and 31.
                em_write_phy_reg_ex(hw, GG82563_PHY_PAGE_SELECT_ALT, page)
            };

            if let Err(e) = ret_val {
                em_swfw_sync_release(hw, swfw);
                return Err(e);
            }
        }
    } else if hw.phy_type == em_phy_bm && hw.phy_revision == 1 && reg_addr > MAX_PHY_MULTI_PAGE_REG
    {
        // The C returns here without releasing the semaphore.
        em_write_phy_reg_ex(hw, BM_PHY_PAGE_SELECT, (reg_addr as u16) >> PHY_PAGE_SHIFT)?;
    }
    let ret_val = em_read_phy_reg_ex(hw, MAX_PHY_REG_ADDRESS & reg_addr, phy_data);

    em_swfw_sync_release(hw, swfw);
    ret_val
}

/// Reads a PHY register without paging or semaphores: over I2C for an SGMII PHY, through
/// the GCU on the EP80579, through `MDIC` on the 82544 and later, or by bit-banging the
/// MDIO pins on older MACs.
fn em_read_phy_reg_ex(hw: &mut EmHw, reg_addr: u32, phy_data: &mut u16) -> Result<(), i32> {
    // SGMII active is only set on some specific chips.
    if hw.sgmii_active && !em_sgmii_uses_mdio_82575(hw) {
        if reg_addr > E1000_MAX_SGMII_PHY_REG_ADDR {
            // PHY Address is out of range.
            return Err(-E1000_ERR_PARAM);
        }
        return em_read_phy_reg_i2c(hw, reg_addr, phy_data);
    }
    if reg_addr > MAX_PHY_REG_ADDRESS {
        // PHY Address is out of range.
        return Err(-E1000_ERR_PARAM);
    }
    if hw.mac_type == em_icp_xxxx {
        *phy_data = gcu_miibus_readreg(hw, hw.icp_xxxx_port_num as i32, reg_addr as i32) as u16;
        return Ok(());
    }
    if hw.mac_type > em_82543 {
        // Set up Op-code, Phy Address, and register address in the MDI Control register.
        // The MAC will take care of interfacing with the PHY to retrieve the desired data.
        let mut mdic = (reg_addr << E1000_MDIC_REG_SHIFT)
            | (hw.phy_addr << E1000_MDIC_PHY_SHIFT)
            | E1000_MDIC_OP_READ;

        e1000_write_reg(hw, E1000_MDIC, mdic);

        // Poll the ready bit to see if the MDI read completed. Increasing the time out as
        // testing showed failures with the lower time out (from FreeBSD driver).
        for _ in 0..1960 {
            usec_delay(50);
            mdic = e1000_read_reg(hw, E1000_MDIC);
            if mdic & E1000_MDIC_READY != 0 {
                break;
            }
        }
        if mdic & E1000_MDIC_READY == 0 {
            // MDI Read did not complete.
            return Err(-E1000_ERR_PHY);
        }
        if mdic & E1000_MDIC_ERROR != 0 {
            // MDI Error.
            return Err(-E1000_ERR_PHY);
        }
        *phy_data = mdic as u16;

        if hw.mac_type >= em_pch2lan {
            usec_delay(100);
        }
    } else {
        // We must first send a preamble through the MDIO pin to signal the beginning of an
        // MII instruction. This is done by sending 32 consecutive "1" bits.
        em_shift_out_mdi_bits(hw, PHY_PREAMBLE, PHY_PREAMBLE_SIZE as u16);
        // Now combine the next few fields that are required for a read operation. We use
        // this method instead of calling the em_shift_out_mdi_bits routine five different
        // times. The format of a MII read instruction consists of a shift out of 14 bits
        // and is defined as follows: <Preamble><SOF><Op Code><Phy Addr><Reg Addr> followed
        // by a shift in of 18 bits. This first two bits shifted in are TurnAround bits used
        // to avoid contention on the MDIO pin when a READ operation is performed. These two
        // bits are thrown away followed by a shift in of 16 bits which contains the desired
        // data.
        let mdic = reg_addr | (hw.phy_addr << 5) | (PHY_OP_READ << 10) | (PHY_SOF << 12);

        em_shift_out_mdi_bits(hw, mdic, 14);
        // Now that we've shifted out the read command to the MII, we need to "shift in" the
        // 16-bit value (18 total bits) of the requested PHY register address.
        *phy_data = em_shift_in_mdi_bits(hw);
    }
    Ok(())
}

/// `em_write_phy_reg`: writes a value to a PHY register; if the register is on a specific
/// non zero page, sets the page first.
pub fn em_write_phy_reg(hw: &mut EmHw, reg_addr: u32, phy_data: u16) -> Result<(), i32> {
    if hw.mac_type >= em_pchlan {
        let mut d = phy_data;
        return em_access_phy_reg_hv(hw, reg_addr, &mut d, false);
    }

    let swfw = hw.swfw;
    if em_swfw_sync_acquire(hw, swfw).is_err() {
        return Err(-E1000_ERR_SWFW_SYNC);
    }

    if (hw.phy_type == em_phy_igp || hw.phy_type == em_phy_igp_3 || hw.phy_type == em_phy_igp_2)
        && reg_addr > MAX_PHY_MULTI_PAGE_REG
    {
        if let Err(e) = em_write_phy_reg_ex(hw, IGP01E1000_PHY_PAGE_SELECT, reg_addr as u16) {
            let swfw = hw.swfw;
            em_swfw_sync_release(hw, swfw);
            return Err(e);
        }
    } else if hw.phy_type == em_phy_gg82563 {
        if (reg_addr & MAX_PHY_REG_ADDRESS) > MAX_PHY_MULTI_PAGE_REG
            || hw.mac_type == em_80003es2lan
        {
            let page = (reg_addr as u16) >> GG82563_PAGE_SHIFT;
            // Select Configuration Page.
            let ret_val = if (reg_addr & MAX_PHY_REG_ADDRESS) < GG82563_MIN_ALT_REG {
                em_write_phy_reg_ex(hw, GG82563_PHY_PAGE_SELECT, page)
            } else {
                // Use Alternative Page Select register to access registers 30 and 31.
                em_write_phy_reg_ex(hw, GG82563_PHY_PAGE_SELECT_ALT, page)
            };

            if let Err(e) = ret_val {
                let swfw = hw.swfw;
                em_swfw_sync_release(hw, swfw);
                return Err(e);
            }
        }
    } else if hw.phy_type == em_phy_bm && hw.phy_revision == 1 && reg_addr > MAX_PHY_MULTI_PAGE_REG
    {
        // The C returns here without releasing the semaphore.
        em_write_phy_reg_ex(hw, BM_PHY_PAGE_SELECT, (reg_addr as u16) >> PHY_PAGE_SHIFT)?;
    }
    let ret_val = em_write_phy_reg_ex(hw, MAX_PHY_REG_ADDRESS & reg_addr, phy_data);

    let swfw = hw.swfw;
    em_swfw_sync_release(hw, swfw);
    ret_val
}

/// Writes a PHY register without paging or semaphores (the counterpart of
/// `em_read_phy_reg_ex`).
fn em_write_phy_reg_ex(hw: &mut EmHw, reg_addr: u32, phy_data: u16) -> Result<(), i32> {
    // SGMII active is only set on some specific chips.
    if hw.sgmii_active && !em_sgmii_uses_mdio_82575(hw) {
        if reg_addr > E1000_MAX_SGMII_PHY_REG_ADDR {
            // PHY Address is out of range.
            return Err(-E1000_ERR_PARAM);
        }
        return em_write_phy_reg_i2c(hw, reg_addr, phy_data);
    }
    if reg_addr > MAX_PHY_REG_ADDRESS {
        // PHY Address is out of range.
        return Err(-E1000_ERR_PARAM);
    }
    if hw.mac_type == em_icp_xxxx {
        gcu_miibus_writereg(
            hw,
            hw.icp_xxxx_port_num as i32,
            reg_addr as i32,
            i32::from(phy_data),
        );
        return Ok(());
    }
    if hw.mac_type > em_82543 {
        // Set up Op-code, Phy Address, register address, and data intended for the PHY
        // register in the MDI Control register. The MAC will take care of interfacing with
        // the PHY to send the desired data.
        let mut mdic = u32::from(phy_data)
            | (reg_addr << E1000_MDIC_REG_SHIFT)
            | (hw.phy_addr << E1000_MDIC_PHY_SHIFT)
            | E1000_MDIC_OP_WRITE;

        e1000_write_reg(hw, E1000_MDIC, mdic);

        // Poll the ready bit to see if the MDI read completed.
        for _ in 0..641 {
            usec_delay(5);
            mdic = e1000_read_reg(hw, E1000_MDIC);
            if mdic & E1000_MDIC_READY != 0 {
                break;
            }
        }
        if mdic & E1000_MDIC_READY == 0 {
            // MDI Write did not complete.
            return Err(-E1000_ERR_PHY);
        }

        if hw.mac_type >= em_pch2lan {
            usec_delay(100);
        }
    } else {
        // We'll need to use the SW defined pins to shift the write command out to the PHY.
        // We first send a preamble to the PHY to signal the beginning of the MII
        // instruction. This is done by sending 32 consecutive "1" bits.
        em_shift_out_mdi_bits(hw, PHY_PREAMBLE, PHY_PREAMBLE_SIZE as u16);
        // Now combine the remaining required fields that will indicate a write operation.
        // We use this method instead of calling the em_shift_out_mdi_bits routine for each
        // field in the command. The format of a MII write instruction is as follows:
        // <Preamble><SOF><Op Code><Phy Addr><Reg Addr><Turnaround><Data>.
        let mut mdic = PHY_TURNAROUND
            | (reg_addr << 2)
            | (hw.phy_addr << 7)
            | (PHY_OP_WRITE << 12)
            | (PHY_SOF << 14);
        mdic <<= 16;
        mdic |= u32::from(phy_data);

        em_shift_out_mdi_bits(hw, mdic, 32);
    }

    Ok(())
}

/// Reads a kumeran (MAC-PHY interconnect) register.
fn em_read_kmrn_reg(hw: &mut EmHw, reg_addr: u32, data: &mut u16) -> Result<(), i32> {
    let swfw = hw.swfw;
    if em_swfw_sync_acquire(hw, swfw).is_err() {
        return Err(-E1000_ERR_SWFW_SYNC);
    }

    // Write register address.
    let mut reg_val = ((reg_addr << E1000_KUMCTRLSTA_OFFSET_SHIFT) & E1000_KUMCTRLSTA_OFFSET)
        | E1000_KUMCTRLSTA_REN;

    e1000_write_reg(hw, E1000_KUMCTRLSTA, reg_val);
    usec_delay(2);

    // Read the data returned.
    reg_val = e1000_read_reg(hw, E1000_KUMCTRLSTA);
    *data = reg_val as u16;

    let swfw = hw.swfw;
    em_swfw_sync_release(hw, swfw);
    Ok(())
}

/// Writes a kumeran (MAC-PHY interconnect) register.
fn em_write_kmrn_reg(hw: &mut EmHw, reg_addr: u32, data: u16) -> Result<(), i32> {
    let swfw = hw.swfw;
    if em_swfw_sync_acquire(hw, swfw).is_err() {
        return Err(-E1000_ERR_SWFW_SYNC);
    }

    let reg_val =
        ((reg_addr << E1000_KUMCTRLSTA_OFFSET_SHIFT) & E1000_KUMCTRLSTA_OFFSET) | u32::from(data);

    e1000_write_reg(hw, E1000_KUMCTRLSTA, reg_val);
    usec_delay(2);

    let swfw = hw.swfw;
    em_swfw_sync_release(hw, swfw);
    Ok(())
}

/// `em_sgmii_uses_mdio_82575`: whether the I2C pins are used as an external MDIO interface
/// rather than for I2C (the two options are mutually exclusive).
pub fn em_sgmii_uses_mdio_82575(hw: &EmHw) -> bool {
    match hw.mac_type {
        em_82575 | em_82576 => {
            let reg = e1000_read_reg(hw, E1000_MDIC);
            reg & E1000_MDIC_DEST != 0
        }
        em_82580 | em_i350 | em_i210 => {
            let reg = e1000_read_reg(hw, E1000_MDICNFG);
            reg & E1000_MDICNFG_EXT_MDIO != 0
        }
        _ => false,
    }
}

/// `em_read_phy_reg_i2c`: reads the PHY register at `offset` over the I2C interface.
pub fn em_read_phy_reg_i2c(hw: &EmHw, offset: u32, data: &mut u16) -> Result<(), i32> {
    // Set up Op-code, Phy Address, and register address in the I2CCMD register. The MAC
    // will take care of interfacing with the PHY to retrieve the desired data.
    let mut i2ccmd = (offset << E1000_I2CCMD_REG_ADDR_SHIFT)
        | (hw.phy_addr << E1000_I2CCMD_PHY_ADDR_SHIFT)
        | E1000_I2CCMD_OPCODE_READ;

    e1000_write_reg(hw, E1000_I2CCMD, i2ccmd);

    // Poll the ready bit to see if the I2C read completed.
    for _ in 0..E1000_I2CCMD_PHY_TIMEOUT {
        usec_delay(50);
        i2ccmd = e1000_read_reg(hw, E1000_I2CCMD);
        if i2ccmd & E1000_I2CCMD_READY != 0 {
            break;
        }
    }
    if i2ccmd & E1000_I2CCMD_READY == 0 {
        // I2CCMD Read did not complete.
        return Err(-E1000_ERR_PHY);
    }
    if i2ccmd & E1000_I2CCMD_ERROR != 0 {
        // I2CCMD Error bit set.
        return Err(-E1000_ERR_PHY);
    }

    // Need to byte-swap the 16-bit value.
    *data = (((i2ccmd >> 8) & 0x00FF) | ((i2ccmd << 8) & 0xFF00)) as u16;

    Ok(())
}

/// `em_write_phy_reg_i2c`: writes `data` to the PHY register at `offset` over the I2C
/// interface.
pub fn em_write_phy_reg_i2c(hw: &EmHw, offset: u32, data: u16) -> Result<(), i32> {
    // Prevent overwriting SFP I2C EEPROM which is at A0 address.
    if hw.phy_addr == 0 || hw.phy_addr > 7 {
        // PHY I2C Address is out of range.
        return Err(-E1000_ERR_CONFIG);
    }

    // Swap the data bytes for the I2C interface.
    let phy_data_swapped = ((data >> 8) & 0x00FF) | ((data << 8) & 0xFF00);

    // Set up Op-code, Phy Address, and register address in the I2CCMD register. The MAC
    // will take care of interfacing with the PHY to retrieve the desired data.
    let mut i2ccmd = (offset << E1000_I2CCMD_REG_ADDR_SHIFT)
        | (hw.phy_addr << E1000_I2CCMD_PHY_ADDR_SHIFT)
        | E1000_I2CCMD_OPCODE_WRITE
        | u32::from(phy_data_swapped);

    e1000_write_reg(hw, E1000_I2CCMD, i2ccmd);

    // Poll the ready bit to see if the I2C read completed.
    for _ in 0..E1000_I2CCMD_PHY_TIMEOUT {
        usec_delay(50);
        i2ccmd = e1000_read_reg(hw, E1000_I2CCMD);
        if i2ccmd & E1000_I2CCMD_READY != 0 {
            break;
        }
    }
    if i2ccmd & E1000_I2CCMD_READY == 0 {
        // I2CCMD Write did not complete.
        return Err(-E1000_ERR_PHY);
    }
    if i2ccmd & E1000_I2CCMD_ERROR != 0 {
        // I2CCMD Error bit set.
        return Err(-E1000_ERR_PHY);
    }

    Ok(())
}

/// `em_read_sfp_data_byte`: reads one byte of the SFP module's EEPROM or diagnostic area;
/// `offset` is `E1000_I2CCMD_SFP_DATA_ADDR(<byte offset>)` for the module database or
/// `E1000_I2CCMD_SFP_DIAG_ADDR(<byte offset>)` for the diagnostics parameters.
pub fn em_read_sfp_data_byte(hw: &EmHw, offset: u16, data: &mut u8) -> Result<(), i32> {
    let mut data_local: u32 = 0;

    if offset > e1000_i2ccmd_sfp_diag_addr(255) {
        // I2CCMD command address exceeds upper limit.
        return Err(-E1000_ERR_PHY);
    }

    // Set up Op-code, EEPROM Address,in the I2CCMD register. The MAC will take care of
    // interfacing with the EEPROM to retrieve the desired data.
    let i2ccmd = (u32::from(offset) << E1000_I2CCMD_REG_ADDR_SHIFT) | E1000_I2CCMD_OPCODE_READ;

    e1000_write_reg(hw, E1000_I2CCMD, i2ccmd);

    // Poll the ready bit to see if the I2C read completed.
    for _ in 0..E1000_I2CCMD_PHY_TIMEOUT {
        usec_delay(50);
        data_local = e1000_read_reg(hw, E1000_I2CCMD);
        if data_local & E1000_I2CCMD_READY != 0 {
            break;
        }
    }
    if data_local & E1000_I2CCMD_READY == 0 {
        // I2CCMD Read did not complete.
        return Err(-E1000_ERR_PHY);
    }
    if data_local & E1000_I2CCMD_ERROR != 0 {
        // I2CCMD Error bit set.
        return Err(-E1000_ERR_PHY);
    }
    *data = data_local as u8;

    Ok(())
}

/// `em_phy_hw_reset`: returns the PHY to the power-on reset state.
pub fn em_phy_hw_reset(hw: &mut EmHw) -> Result<(), i32> {
    // In the case of the phy reset being blocked, it's not an error, we simply return
    // success without performing the reset.
    if em_check_phy_reset_block(hw).is_err() {
        return Ok(());
    }

    if hw.mac_type > em_82543 && hw.mac_type != em_icp_xxxx {
        let swfw = hw.swfw;
        if em_swfw_sync_acquire(hw, swfw).is_err() {
            // Unable to acquire swfw sync.
            return Err(-E1000_ERR_SWFW_SYNC);
        }
        // Read the device control register and assert the E1000_CTRL_PHY_RST bit. Then,
        // take it out of reset. For pre-em_82571 hardware, we delay for 10ms between the
        // assert and deassert. For em_82571 hardware and later, we instead delay for 50us
        // between and 10ms after the deassertion.
        let ctrl = e1000_read_reg(hw, E1000_CTRL);
        e1000_write_reg(hw, E1000_CTRL, ctrl | E1000_CTRL_PHY_RST);
        e1000_write_flush(hw);

        if hw.mac_type < em_82571 {
            msec_delay(10);
        } else {
            usec_delay(100);
        }

        e1000_write_reg(hw, E1000_CTRL, ctrl);
        e1000_write_flush(hw);

        if hw.mac_type >= em_82571 {
            msec_delay_irq(10);
        }
        let swfw = hw.swfw;
        em_swfw_sync_release(hw, swfw);
        // The M88E1141_E_PHY_ID might need reset here, but nothing proves it.
    } else {
        // Read the Extended Device Control Register, assert the PHY_RESET_DIR bit to put
        // the PHY into reset. Then, take it out of reset.
        let mut ctrl_ext = e1000_read_reg(hw, E1000_CTRL_EXT);
        ctrl_ext |= E1000_CTRL_EXT_SDP4_DIR;
        ctrl_ext &= !E1000_CTRL_EXT_SDP4_DATA;
        e1000_write_reg(hw, E1000_CTRL_EXT, ctrl_ext);
        e1000_write_flush(hw);
        msec_delay(10);
        ctrl_ext |= E1000_CTRL_EXT_SDP4_DATA;
        e1000_write_reg(hw, E1000_CTRL_EXT, ctrl_ext);
        e1000_write_flush(hw);
    }
    usec_delay(150);

    if hw.mac_type == em_82541 || hw.mac_type == em_82547 {
        // Configure activity LED after PHY reset.
        let mut led_ctrl = e1000_read_reg(hw, E1000_LEDCTL);
        led_ctrl &= IGP_ACTIVITY_LED_MASK;
        led_ctrl |= IGP_ACTIVITY_LED_ENABLE | IGP_LED3_MODE;
        e1000_write_reg(hw, E1000_LEDCTL, led_ctrl);
    }
    // Wait for FW to finish PHY configuration.
    em_get_phy_cfg_done(hw)?;
    em_release_software_semaphore(hw);

    if hw.mac_type == em_ich8lan && hw.phy_type == em_phy_igp_3 {
        return em_init_lcd_from_nvm(hw);
    }

    Ok(())
}

/// `em_oem_bits_config_pchlan`: SW-based LCD configuration. SW will configure Gbe Disable
/// and LPLU based on the NVM. The four bits are collectively called OEM bits. The OEM Write
/// Enable bit and SW Config bit in NVM determines whether HW should configure LPLU and Gbe
/// Disable.
pub fn em_oem_bits_config_pchlan(hw: &mut EmHw, d0_state: bool) -> Result<(), i32> {
    let mut oem_reg: u16 = 0;
    let swfw = E1000_SWFW_PHY0_SM;

    if hw.mac_type < em_pchlan {
        return Ok(());
    }

    em_swfw_sync_acquire(hw, swfw)?;

    let ret_val = 'out: {
        if hw.mac_type == em_pchlan {
            let mac_reg = e1000_read_reg(hw, E1000_EXTCNF_CTRL);
            if mac_reg & E1000_EXTCNF_CTRL_OEM_WRITE_ENABLE != 0 {
                break 'out Ok(());
            }
        }

        let mac_reg = e1000_read_reg(hw, E1000_FEXTNVM);
        if mac_reg & FEXTNVM_SW_CONFIG_ICH8M == 0 {
            break 'out Ok(());
        }

        let mac_reg = e1000_read_reg(hw, E1000_PHY_CTRL);

        let r = em_read_phy_reg(hw, HV_OEM_BITS, &mut oem_reg);
        if r.is_err() {
            break 'out r;
        }

        oem_reg &= !(HV_OEM_BITS_GBE_DIS | HV_OEM_BITS_LPLU);

        if d0_state {
            if mac_reg & E1000_PHY_CTRL_GBE_DISABLE != 0 {
                oem_reg |= HV_OEM_BITS_GBE_DIS;
            }

            if mac_reg & E1000_PHY_CTRL_D0A_LPLU != 0 {
                oem_reg |= HV_OEM_BITS_LPLU;
            }
            // Restart auto-neg to activate the bits.
            if em_check_phy_reset_block(hw).is_ok() {
                oem_reg |= HV_OEM_BITS_RESTART_AN;
            }
        } else {
            if mac_reg & (E1000_PHY_CTRL_GBE_DISABLE | E1000_PHY_CTRL_NOND0A_GBE_DISABLE) != 0 {
                oem_reg |= HV_OEM_BITS_GBE_DIS;
            }

            if mac_reg & (E1000_PHY_CTRL_D0A_LPLU | E1000_PHY_CTRL_NOND0A_LPLU) != 0 {
                oem_reg |= HV_OEM_BITS_LPLU;
            }
        }

        em_write_phy_reg(hw, HV_OEM_BITS, oem_reg)
    };

    em_swfw_sync_release(hw, swfw);

    ret_val
}

/// `em_phy_reset`: resets the PHY (sets bit 15 of the MII Control register, or a hardware
/// reset for the IGP and IFE PHYs), then applies the per-MAC PHY workarounds.
pub fn em_phy_reset(hw: &mut EmHw) -> Result<(), i32> {
    let mut phy_data: u16 = 0;
    // In the case of the phy reset being blocked, it's not an error, we simply return
    // success without performing the reset.
    if em_check_phy_reset_block(hw).is_err() {
        return Ok(());
    }

    match hw.phy_type {
        em_phy_igp | em_phy_igp_2 | em_phy_igp_3 | em_phy_ife => {
            em_phy_hw_reset(hw)?;
        }
        _ => {
            em_read_phy_reg(hw, PHY_CTRL, &mut phy_data)?;

            phy_data |= MII_CR_RESET;
            em_write_phy_reg(hw, PHY_CTRL, phy_data)?;

            usec_delay(1);
        }
    }

    // Allow time for h/w to get to a quiescent state after reset.
    msec_delay(10);

    if hw.phy_type == em_phy_igp || hw.phy_type == em_phy_igp_2 {
        em_phy_init_script(hw);
    }

    if hw.mac_type == em_pchlan {
        em_hv_phy_workarounds_ich8lan(hw)?;
    } else if hw.mac_type == em_pch2lan {
        em_lv_phy_workarounds_ich8lan(hw)?;
    } else if hw.mac_type >= em_pch_mtp {
        em_reconfigure_k1_exit_timeout(hw)?;
    }

    if hw.mac_type >= em_pchlan {
        em_oem_bits_config_pchlan(hw, true)?;
    }

    // Ungate automatic PHY configuration on non-managed 82579.
    if hw.mac_type == em_pch2lan && e1000_read_reg(hw, E1000_FWSM) & E1000_FWSM_FW_VALID == 0 {
        msec_delay(10);
        em_gate_hw_phy_config_ich8lan(hw, false);
    }

    if hw.phy_id == M88E1512_E_PHY_ID {
        em_initialize_M88E1512_phy(hw)?;
    }

    Ok(())
}

/// Work-around for 82566 Kumeran PCS lock loss. On link status change (i.e. PCI reset,
/// speed change) and link is up and speed is gigabit:
/// 0. if workaround is optionally disabled do nothing;
/// 1. wait 1ms for Kumeran link to come up;
/// 2. check Kumeran Diagnostic register PCS lock loss bit;
/// 3. if not set the link is locked (all is good), otherwise...
/// 4. reset the PHY;
/// 5. repeat up to 10 times.
///
/// Only called for IGP3 copper when speed is 1gb.
fn em_kumeran_lock_loss_workaround(hw: &mut EmHw) -> Result<(), i32> {
    let mut phy_data: u16 = 0;
    if hw.kmrn_lock_loss_workaround_disabled {
        return Ok(());
    }
    // Make sure link is up before proceeding. If not just return. Attempting this while
    // link is negotiating fouled up link stability.
    let _ = em_read_phy_reg(hw, PHY_STATUS, &mut phy_data);
    let _ = em_read_phy_reg(hw, PHY_STATUS, &mut phy_data);

    if phy_data & MII_SR_LINK_STATUS != 0 {
        for _ in 0..10 {
            // Read once to clear.
            em_read_phy_reg(hw, IGP3_KMRN_DIAG, &mut phy_data)?;
            // And again to get new status.
            em_read_phy_reg(hw, IGP3_KMRN_DIAG, &mut phy_data)?;

            // Check for PCS lock.
            if phy_data & IGP3_KMRN_DIAG_PCS_LOCK_LOSS == 0 {
                return Ok(());
            }

            // Issue PHY reset.
            let _ = em_phy_hw_reset(hw);
            msec_delay_irq(5);
        }
        // Disable GigE link negotiation.
        let reg = e1000_read_reg(hw, E1000_PHY_CTRL);
        e1000_write_reg(
            hw,
            E1000_PHY_CTRL,
            reg | E1000_PHY_CTRL_GBE_DISABLE | E1000_PHY_CTRL_NOND0A_GBE_DISABLE,
        );

        // Unable to acquire PCS lock (the C's positive status).
        return Err(E1000_ERR_PHY);
    }
    Ok(())
}

/// Reads the PHY id at `hw.phy_addr` and matches it against the PHYs expected on this MAC.
fn em_match_gig_phy(hw: &mut EmHw) -> Result<(), i32> {
    let mut phy_id_high: u16 = 0;
    let mut phy_id_low: u16 = 0;
    let mut r#match = false;

    em_read_phy_reg(hw, PHY_ID1, &mut phy_id_high)?;

    hw.phy_id = u32::from(phy_id_high) << 16;
    usec_delay(20);
    em_read_phy_reg(hw, PHY_ID2, &mut phy_id_low)?;

    hw.phy_id |= u32::from(phy_id_low) & PHY_REVISION_MASK;
    hw.phy_revision = u32::from(phy_id_low) & !PHY_REVISION_MASK;

    let id = hw.phy_id;
    match hw.mac_type {
        em_82543 => r#match = id == M88E1000_E_PHY_ID,
        em_82544 => r#match = id == M88E1000_I_PHY_ID,
        em_82540 | em_82545 | em_82545_rev_3 | em_82546 | em_82546_rev_3 => {
            r#match = id == M88E1011_I_PHY_ID
        }
        em_82541 | em_82541_rev_2 | em_82547 | em_82547_rev_2 => {
            r#match = id == IGP01E1000_I_PHY_ID
        }
        em_82573 => r#match = id == M88E1111_I_PHY_ID,
        em_82574 => r#match = id == BME1000_E_PHY_ID,
        em_82575 | em_82576 => {
            r#match =
                id == M88E1000_E_PHY_ID || id == IGP01E1000_I_PHY_ID || id == IGP03E1000_E_PHY_ID
        }
        em_82580 | em_i210 | em_i350 => {
            if id == I82580_I_PHY_ID
                || id == I210_I_PHY_ID
                || id == I347AT4_E_PHY_ID
                || id == I350_I_PHY_ID
                || id == M88E1111_I_PHY_ID
                || id == M88E1112_E_PHY_ID
                || id == M88E1543_E_PHY_ID
                || id == M88E1512_E_PHY_ID
            {
                let mut mdic = em_read_reg(hw, E1000_MDICNFG);
                if mdic & E1000_MDICNFG_EXT_MDIO != 0 {
                    mdic &= E1000_MDICNFG_PHY_MASK;
                    hw.phy_addr = mdic >> E1000_MDICNFG_PHY_SHIFT;
                }
                r#match = true;
            }
        }
        em_80003es2lan => r#match = id == GG82563_E_PHY_ID,
        em_ich8lan | em_ich9lan | em_ich10lan | em_pchlan | em_pch2lan => {
            r#match = id == IGP03E1000_E_PHY_ID
                || id == IFE_E_PHY_ID
                || id == IFE_PLUS_E_PHY_ID
                || id == IFE_C_E_PHY_ID
                || id == BME1000_E_PHY_ID
                || id == I82577_E_PHY_ID
                || id == I82578_E_PHY_ID
                || id == I82579_E_PHY_ID
        }
        em_pch_lpt | em_pch_spt | em_pch_cnp | em_pch_tgp | em_pch_adp | em_pch_mtp
        | em_pch_ptp => r#match = id == I217_E_PHY_ID,
        em_icp_xxxx => r#match = id == M88E1141_E_PHY_ID || id == RTL8211_E_PHY_ID,
        // Invalid MAC type.
        _ => return Err(-E1000_ERR_CONFIG),
    }
    let phy_init_status = em_set_phy_type(hw);

    if r#match && phy_init_status.is_ok() {
        return Ok(());
    }
    // Invalid PHY ID.
    Err(-E1000_ERR_PHY)
}

/// Probes the expected PHY address for known PHY IDs.
fn em_detect_gig_phy(hw: &mut EmHw) -> Result<(), i32> {
    if hw.phy_id != 0 {
        return Ok(());
    }

    // Default phy address, most phys reside here, but not all (ICH10).
    if hw.mac_type != em_icp_xxxx {
        hw.phy_addr = 1;
    } else {
        hw.phy_addr = 0; // There is a phy at phy_addr 0 on EP80579.
    }

    // The 82571 firmware may still be configuring the PHY. In this case, we cannot access
    // the PHY until the configuration is done. So we explicitly set the PHY values.
    if hw.mac_type == em_82571 || hw.mac_type == em_82572 {
        hw.phy_id = IGP01E1000_I_PHY_ID;
        hw.phy_type = em_phy_igp_2;
        return Ok(());
    }

    // Some of the fiber cards dont have a phy, so we must exit cleanly here.
    if hw.media_type == em_media_type_fiber
        && matches!(
            hw.mac_type,
            em_82542_rev2_0 | em_82542_rev2_1 | em_82543 | em_82573 | em_82574 | em_80003es2lan
        )
    {
        hw.phy_type = em_phy_undefined;
        return Ok(());
    }

    if (hw.media_type == em_media_type_internal_serdes || hw.media_type == em_media_type_fiber)
        && hw.mac_type >= em_82575
    {
        hw.phy_type = em_phy_undefined;
        return Ok(());
    }

    // Up to 82543 (incl), we need reset the phy, or it might not get detected.
    if hw.mac_type <= em_82543 {
        em_phy_hw_reset(hw)?;
    }
    // ESB-2 PHY reads require em_phy_gg82563 to be set because of a work- around that
    // forces PHY page 0 to be set or the reads fail. The rest of the code in this routine
    // uses em_read_phy_reg to read the PHY ID. So for ESB-2 we need to have this set so our
    // reads won't fail. If the attached PHY is not a em_phy_gg82563, the routines below
    // will figure this out as well.
    if hw.mac_type == em_80003es2lan {
        hw.phy_type = em_phy_gg82563;
    }

    // Power on SGMII phy if it is disabled.
    if hw.mac_type == em_82580 || hw.mac_type == em_i210 || hw.mac_type == em_i350 {
        let ctrl_ext = em_read_reg(hw, E1000_CTRL_EXT);
        em_write_reg(hw, E1000_CTRL_EXT, ctrl_ext & !E1000_CTRL_EXT_SDP3_DATA);
        e1000_write_flush(hw);
        msec_delay(300);
    }

    // Read the PHY ID Registers to identify which PHY is onboard.
    for i in 1..8 {
        // hw->phy_addr may be modified down in the call stack, we can't use it as loop
        // variable.
        hw.phy_addr = i;
        if em_match_gig_phy(hw).is_ok() {
            return Ok(());
        }
    }
    Err(-E1000_ERR_PHY)
}

/// Resets the PHY's DSP.
fn em_phy_reset_dsp(hw: &mut EmHw) -> Result<(), i32> {
    if hw.phy_type != em_phy_gg82563 {
        em_write_phy_reg(hw, 29, 0x001d)?;
    }
    em_write_phy_reg(hw, 30, 0x00c1)?;
    em_write_phy_reg(hw, 30, 0x0000)?;
    Ok(())
}

/// Marks every word of the ICH8 shadow RAM unmodified and erased, when the driver
/// allocated one (`init_eeprom_params`'s "zero the shadow RAM structure").
fn em_clear_shadow_ram(hw: &mut EmHw) {
    if let Some(ram) = hw.eeprom_shadow_ram.as_deref_mut() {
        for w in ram.iter_mut().take(E1000_SHADOW_RAM_WORDS as usize) {
            w.modified = false;
            w.eeprom_word = 0xFFFF;
        }
    }
}

/// `em_init_eeprom_params`: sets up the eeprom variables in the hw struct. Must be called
/// after mac_type is configured. Additionally, if this is ICH8, the flash controller GbE
/// registers must be mapped, or this will crash.
pub fn em_init_eeprom_params(hw: &mut EmHw) -> Result<(), i32> {
    let mut eecd = e1000_read_reg(hw, E1000_EECD);
    let mut eeprom_size: u16;

    match hw.mac_type {
        em_82542_rev2_0 | em_82542_rev2_1 | em_82543 | em_82544 => {
            hw.eeprom.r#type = em_eeprom_microwire;
            hw.eeprom.word_size = 64;
            hw.eeprom.opcode_bits = 3;
            hw.eeprom.address_bits = 6;
            hw.eeprom.delay_usec = 50;
            hw.eeprom.use_eerd = false;
            hw.eeprom.use_eewr = false;
        }
        em_82540 | em_82545 | em_82545_rev_3 | em_icp_xxxx | em_82546 | em_82546_rev_3 => {
            hw.eeprom.r#type = em_eeprom_microwire;
            hw.eeprom.opcode_bits = 3;
            hw.eeprom.delay_usec = 50;
            if eecd & E1000_EECD_SIZE != 0 {
                hw.eeprom.word_size = 256;
                hw.eeprom.address_bits = 8;
            } else {
                hw.eeprom.word_size = 64;
                hw.eeprom.address_bits = 6;
            }
            hw.eeprom.use_eerd = false;
            hw.eeprom.use_eewr = false;
        }
        em_82541 | em_82541_rev_2 | em_82547 | em_82547_rev_2 => {
            if eecd & E1000_EECD_TYPE != 0 {
                hw.eeprom.r#type = em_eeprom_spi;
                hw.eeprom.opcode_bits = 8;
                hw.eeprom.delay_usec = 1;
                if eecd & E1000_EECD_ADDR_BITS != 0 {
                    hw.eeprom.page_size = 32;
                    hw.eeprom.address_bits = 16;
                } else {
                    hw.eeprom.page_size = 8;
                    hw.eeprom.address_bits = 8;
                }
            } else {
                hw.eeprom.r#type = em_eeprom_microwire;
                hw.eeprom.opcode_bits = 3;
                hw.eeprom.delay_usec = 50;
                if eecd & E1000_EECD_ADDR_BITS != 0 {
                    hw.eeprom.word_size = 256;
                    hw.eeprom.address_bits = 8;
                } else {
                    hw.eeprom.word_size = 64;
                    hw.eeprom.address_bits = 6;
                }
            }
            hw.eeprom.use_eerd = false;
            hw.eeprom.use_eewr = false;
        }
        em_82571 | em_82572 => {
            hw.eeprom.r#type = em_eeprom_spi;
            hw.eeprom.opcode_bits = 8;
            hw.eeprom.delay_usec = 1;
            if eecd & E1000_EECD_ADDR_BITS != 0 {
                hw.eeprom.page_size = 32;
                hw.eeprom.address_bits = 16;
            } else {
                hw.eeprom.page_size = 8;
                hw.eeprom.address_bits = 8;
            }
            hw.eeprom.use_eerd = false;
            hw.eeprom.use_eewr = false;
        }
        em_82573 | em_82574 | em_82575 | em_82576 | em_82580 | em_i210 | em_i350 => {
            hw.eeprom.r#type = em_eeprom_spi;
            hw.eeprom.opcode_bits = 8;
            hw.eeprom.delay_usec = 1;
            if eecd & E1000_EECD_ADDR_BITS != 0 {
                hw.eeprom.page_size = 32;
                hw.eeprom.address_bits = 16;
            } else {
                hw.eeprom.page_size = 8;
                hw.eeprom.address_bits = 8;
            }
            hw.eeprom.use_eerd = true;
            hw.eeprom.use_eewr = true;
            if !em_is_onboard_nvm_eeprom(hw) {
                hw.eeprom.r#type = em_eeprom_flash;
                hw.eeprom.word_size = 2048;
                // Ensure that the Autonomous FLASH update bit is cleared due to Flash
                // update issue on parts which use a FLASH for NVM.
                eecd &= !E1000_EECD_AUPDEN;
                e1000_write_reg(hw, E1000_EECD, eecd);
            }
            if !em_get_flash_presence_i210(hw) {
                hw.eeprom.r#type = em_eeprom_invm;
                hw.eeprom.word_size = INVM_SIZE as u16;
                hw.eeprom.use_eerd = false;
                hw.eeprom.use_eewr = false;
            }
        }
        em_80003es2lan => {
            hw.eeprom.r#type = em_eeprom_spi;
            hw.eeprom.opcode_bits = 8;
            hw.eeprom.delay_usec = 1;
            if eecd & E1000_EECD_ADDR_BITS != 0 {
                hw.eeprom.page_size = 32;
                hw.eeprom.address_bits = 16;
            } else {
                hw.eeprom.page_size = 8;
                hw.eeprom.address_bits = 8;
            }
            hw.eeprom.use_eerd = true;
            hw.eeprom.use_eewr = false;
        }
        em_ich8lan | em_ich9lan | em_ich10lan | em_pchlan | em_pch2lan | em_pch_lpt => {
            let flash_size = e1000_read_ich_flash_reg(hw, ICH_FLASH_GFPREG);
            hw.eeprom.r#type = em_eeprom_ich8;
            hw.eeprom.use_eerd = false;
            hw.eeprom.use_eewr = false;
            hw.eeprom.word_size = E1000_SHADOW_RAM_WORDS as u16;
            // Zero the shadow RAM structure. But don't load it from NVM so as to save time
            // for driver init.
            em_clear_shadow_ram(hw);
            hw.flash_base_addr = (flash_size & ICH_GFPREG_BASE_MASK) * ICH_FLASH_SECTOR_SIZE;

            hw.flash_bank_size = ((flash_size >> 16) & ICH_GFPREG_BASE_MASK) + 1;
            hw.flash_bank_size = hw
                .flash_bank_size
                .wrapping_sub(flash_size & ICH_GFPREG_BASE_MASK);

            hw.flash_bank_size = hw.flash_bank_size.wrapping_mul(ICH_FLASH_SECTOR_SIZE);

            hw.flash_bank_size /= 2 * core::mem::size_of::<u16>() as u32;
        }
        em_pch_spt | em_pch_cnp | em_pch_tgp | em_pch_adp | em_pch_mtp | em_pch_ptp => {
            let mut flash_size = em_read_reg(hw, 0xc /* STRAP */);

            hw.eeprom.r#type = em_eeprom_ich8;
            hw.eeprom.use_eerd = false;
            hw.eeprom.use_eewr = false;
            hw.eeprom.word_size = E1000_SHADOW_RAM_WORDS as u16;
            // Zero the shadow RAM structure. But don't load it from NVM so as to save time
            // for driver init.
            em_clear_shadow_ram(hw);
            hw.flash_base_addr = 0;
            flash_size = ((flash_size >> 1) & 0x1f) + 1;
            flash_size *= 4096;
            hw.flash_bank_size = flash_size / 4;
        }
        _ => {}
    }

    if hw.eeprom.r#type == em_eeprom_spi {
        // eeprom_size will be an enum [0..8] that maps to eeprom sizes 128B to 32KB
        // (incremented by powers of 2).
        if hw.mac_type <= em_82547_rev_2 {
            // Set to default value for initial eeprom read.
            hw.eeprom.word_size = 64;
            let mut d = [0u16; 1];
            em_read_eeprom(hw, EEPROM_CFG, 1, &mut d)?;
            eeprom_size = (d[0] & EEPROM_SIZE_MASK) >> EEPROM_SIZE_SHIFT;
            // 256B eeprom size was not supported in earlier hardware, so we bump
            // eeprom_size up one to ensure that "1" (which maps to 256B) is never the
            // result used in the shifting logic below.
            if eeprom_size != 0 {
                eeprom_size += 1;
            }
        } else {
            eeprom_size = ((eecd & E1000_EECD_SIZE_EX_MASK) >> E1000_EECD_SIZE_EX_SHIFT) as u16;
        }

        // EEPROM access above 16k is unsupported.
        if u32::from(eeprom_size) + EEPROM_WORD_SIZE_SHIFT > EEPROM_WORD_SIZE_SHIFT_MAX {
            hw.eeprom.word_size = 1 << EEPROM_WORD_SIZE_SHIFT_MAX;
        } else {
            hw.eeprom.word_size = 1 << (u32::from(eeprom_size) + EEPROM_WORD_SIZE_SHIFT);
        }
    }
    Ok(())
}

/// Raises the EEPROM's clock input (by setting the SK bit), and then waits `delay_usec`
/// microseconds. `eecd` is EECD's current value.
fn em_raise_ee_clk(hw: &EmHw, eecd: &mut u32) {
    *eecd |= E1000_EECD_SK;
    e1000_write_reg(hw, E1000_EECD, *eecd);
    e1000_write_flush(hw);
    usec_delay(u32::from(hw.eeprom.delay_usec));
}

/// Lowers the EEPROM's clock input (by clearing the SK bit), and then waits `delay_usec`
/// microseconds. `eecd` is EECD's current value.
fn em_lower_ee_clk(hw: &EmHw, eecd: &mut u32) {
    *eecd &= !E1000_EECD_SK;
    e1000_write_reg(hw, E1000_EECD, *eecd);
    e1000_write_flush(hw);
    usec_delay(u32::from(hw.eeprom.delay_usec));
}

/// Shifts `count` data bits out to the EEPROM, MSB first.
fn em_shift_out_ee_bits(hw: &EmHw, data: u16, count: u16) {
    // We need to shift "count" bits out to the EEPROM. So, value in the "data" parameter
    // will be shifted out to the EEPROM one bit at a time. In order to do this, "data" must
    // be broken down into bits.
    let mut mask: u32 = 0x01 << count.wrapping_sub(1);
    let mut eecd = e1000_read_reg(hw, E1000_EECD);
    if hw.eeprom.r#type == em_eeprom_microwire {
        eecd &= !E1000_EECD_DO;
    } else if hw.eeprom.r#type == em_eeprom_spi {
        eecd |= E1000_EECD_DO;
    }
    loop {
        // A "1" is shifted out to the EEPROM by setting bit "DI" to a "1", and then raising
        // and then lowering the clock (the SK bit controls the clock input to the EEPROM).
        // A "0" is shifted out to the EEPROM by setting "DI" to "0" and then raising and
        // then lowering the clock.
        eecd &= !E1000_EECD_DI;

        if u32::from(data) & mask != 0 {
            eecd |= E1000_EECD_DI;
        }

        e1000_write_reg(hw, E1000_EECD, eecd);
        e1000_write_flush(hw);

        usec_delay(u32::from(hw.eeprom.delay_usec));

        em_raise_ee_clk(hw, &mut eecd);
        em_lower_ee_clk(hw, &mut eecd);

        mask >>= 1;
        if mask == 0 {
            break;
        }
    }

    // We leave the "DI" bit set to "0" when we leave this routine.
    eecd &= !E1000_EECD_DI;
    e1000_write_reg(hw, E1000_EECD, eecd);
}

/// Shifts `count` data bits in from the EEPROM, MSB first.
fn em_shift_in_ee_bits(hw: &EmHw, count: u16) -> u16 {
    // In order to read a register from the EEPROM, we need to shift 'count' bits in from
    // the EEPROM. Bits are "shifted in" by raising the clock input to the EEPROM (setting
    // the SK bit), and then reading the value of the "DO" bit. During this "shifting in"
    // process the "DI" bit should always be clear.
    let mut eecd = e1000_read_reg(hw, E1000_EECD);

    eecd &= !(E1000_EECD_DO | E1000_EECD_DI);
    let mut data: u16 = 0;

    for _ in 0..count {
        data <<= 1;
        em_raise_ee_clk(hw, &mut eecd);

        eecd = e1000_read_reg(hw, E1000_EECD);

        eecd &= !E1000_EECD_DI;
        if eecd & E1000_EECD_DO != 0 {
            data |= 1;
        }

        em_lower_ee_clk(hw, &mut eecd);
    }

    data
}

/// Prepares the EEPROM for access: lowers the EEPROM clock, clears the input pin, sets the
/// chip select pin. Called before issuing a command to the EEPROM.
fn em_acquire_eeprom(hw: &mut EmHw) -> Result<(), i32> {
    let mut i = 0;

    if em_swfw_sync_acquire(hw, E1000_SWFW_EEP_SM).is_err() {
        return Err(-E1000_ERR_SWFW_SYNC);
    }
    let mut eecd = e1000_read_reg(hw, E1000_EECD);

    if hw.mac_type != em_82573 && hw.mac_type != em_82574 {
        // Request EEPROM Access.
        if hw.mac_type > em_82544 {
            eecd |= E1000_EECD_REQ;
            e1000_write_reg(hw, E1000_EECD, eecd);
            eecd = e1000_read_reg(hw, E1000_EECD);
            while eecd & E1000_EECD_GNT == 0 && i < E1000_EEPROM_GRANT_ATTEMPTS {
                i += 1;
                usec_delay(5);
                eecd = e1000_read_reg(hw, E1000_EECD);
            }
            if eecd & E1000_EECD_GNT == 0 {
                eecd &= !E1000_EECD_REQ;
                e1000_write_reg(hw, E1000_EECD, eecd);
                // Could not acquire EEPROM grant.
                em_swfw_sync_release(hw, E1000_SWFW_EEP_SM);
                return Err(-E1000_ERR_EEPROM);
            }
        }
    }

    // Setup EEPROM for Read/Write.
    if hw.eeprom.r#type == em_eeprom_microwire {
        // Clear SK and DI.
        eecd &= !(E1000_EECD_DI | E1000_EECD_SK);
        e1000_write_reg(hw, E1000_EECD, eecd);

        // Set CS.
        eecd |= E1000_EECD_CS;
        e1000_write_reg(hw, E1000_EECD, eecd);
    } else if hw.eeprom.r#type == em_eeprom_spi {
        // Clear SK and CS.
        eecd &= !(E1000_EECD_CS | E1000_EECD_SK);
        e1000_write_reg(hw, E1000_EECD, eecd);
        usec_delay(1);
    }
    Ok(())
}

/// Returns the EEPROM to a "standby" state.
fn em_standby_eeprom(hw: &EmHw) {
    let delay_usec = u32::from(hw.eeprom.delay_usec);
    let mut eecd = e1000_read_reg(hw, E1000_EECD);

    if hw.eeprom.r#type == em_eeprom_microwire {
        eecd &= !(E1000_EECD_CS | E1000_EECD_SK);
        e1000_write_reg(hw, E1000_EECD, eecd);
        e1000_write_flush(hw);
        usec_delay(delay_usec);

        // Clock high.
        eecd |= E1000_EECD_SK;
        e1000_write_reg(hw, E1000_EECD, eecd);
        e1000_write_flush(hw);
        usec_delay(delay_usec);

        // Select EEPROM.
        eecd |= E1000_EECD_CS;
        e1000_write_reg(hw, E1000_EECD, eecd);
        e1000_write_flush(hw);
        usec_delay(delay_usec);

        // Clock low.
        eecd &= !E1000_EECD_SK;
        e1000_write_reg(hw, E1000_EECD, eecd);
        e1000_write_flush(hw);
        usec_delay(delay_usec);
    } else if hw.eeprom.r#type == em_eeprom_spi {
        // Toggle CS to flush commands.
        eecd |= E1000_EECD_CS;
        e1000_write_reg(hw, E1000_EECD, eecd);
        e1000_write_flush(hw);
        usec_delay(delay_usec);
        eecd &= !E1000_EECD_CS;
        e1000_write_reg(hw, E1000_EECD, eecd);
        e1000_write_flush(hw);
        usec_delay(delay_usec);
    }
}

/// Terminates a command by inverting the EEPROM's chip select pin.
fn em_release_eeprom(hw: &mut EmHw) {
    let delay_usec = u32::from(hw.eeprom.delay_usec);
    let mut eecd = e1000_read_reg(hw, E1000_EECD);

    if hw.eeprom.r#type == em_eeprom_spi {
        eecd |= E1000_EECD_CS; // Pull CS high.
        eecd &= !E1000_EECD_SK; // Lower SCK.

        e1000_write_reg(hw, E1000_EECD, eecd);

        usec_delay(delay_usec);
    } else if hw.eeprom.r#type == em_eeprom_microwire {
        // Cleanup eeprom.

        // CS on Microwire is active-high.
        eecd &= !(E1000_EECD_CS | E1000_EECD_DI);

        e1000_write_reg(hw, E1000_EECD, eecd);

        // Rising edge of clock.
        eecd |= E1000_EECD_SK;
        e1000_write_reg(hw, E1000_EECD, eecd);
        e1000_write_flush(hw);
        usec_delay(delay_usec);

        // Falling edge of clock.
        eecd &= !E1000_EECD_SK;
        e1000_write_reg(hw, E1000_EECD, eecd);
        e1000_write_flush(hw);
        usec_delay(delay_usec);
    }
    // Stop requesting EEPROM access.
    if hw.mac_type > em_82544 {
        eecd &= !E1000_EECD_REQ;
        e1000_write_reg(hw, E1000_EECD, eecd);
    }
    em_swfw_sync_release(hw, E1000_SWFW_EEP_SM);
}

/// Waits for an SPI EEPROM to finish its command: reads the "Status Register" repeatedly
/// until the LSB is cleared, for at most `EEPROM_MAX_RETRY_SPI` microseconds.
fn em_spi_eeprom_ready(hw: &EmHw) -> Result<(), i32> {
    // Read "Status Register" repeatedly until the LSB is cleared. The EEPROM will signal
    // that the command has been completed by clearing bit 0 of the internal status
    // register. If it's not cleared within 5 milliseconds, then error out.
    let mut retry_count: u16 = 0;
    loop {
        em_shift_out_ee_bits(hw, EEPROM_RDSR_OPCODE_SPI, hw.eeprom.opcode_bits);
        let spi_stat_reg = em_shift_in_ee_bits(hw, 8) as u8;
        if spi_stat_reg & EEPROM_STATUS_RDY_SPI == 0 {
            break;
        }

        usec_delay(5);
        retry_count += 5;

        em_standby_eeprom(hw);
        if retry_count >= EEPROM_MAX_RETRY_SPI {
            break;
        }
    }
    // ATMEL SPI write time could vary from 0-20mSec on 3.3V devices (and only 0-5mSec on 5V
    // devices).
    if retry_count >= EEPROM_MAX_RETRY_SPI {
        // SPI EEPROM Status error.
        return Err(-E1000_ERR_EEPROM);
    }
    Ok(())
}

/// `em_read_eeprom`: reads `words` 16-bit words from the EEPROM at word `offset` into
/// `data`. A `data` shorter than `words` is refused (`-E1000_ERR_EEPROM`) where the C would
/// write past the caller's buffer.
pub fn em_read_eeprom(hw: &mut EmHw, offset: u16, words: u16, data: &mut [u16]) -> Result<(), i32> {
    // If eeprom is not yet detected, do so now.
    if hw.eeprom.word_size == 0 {
        let _ = em_init_eeprom_params(hw);
    }
    // A check for invalid values: offset too large, too many words, and not enough words.
    if offset >= hw.eeprom.word_size
        || words > hw.eeprom.word_size - offset
        || words == 0
        || data.len() < usize::from(words)
    {
        // "words" parameter out of bounds.
        return Err(-E1000_ERR_EEPROM);
    }
    // EEPROM's that don't use EERD to read require us to bit-bang the SPI directly. In this
    // case, we need to acquire the EEPROM so that FW or other port software does not
    // interrupt.
    if em_is_onboard_nvm_eeprom(hw) && em_get_flash_presence_i210(hw) && !hw.eeprom.use_eerd {
        // Prepare the EEPROM for bit-bang reading.
        if em_acquire_eeprom(hw).is_err() {
            return Err(-E1000_ERR_EEPROM);
        }
    }
    // Eerd register EEPROM access requires no eeprom acquire/release.
    if hw.eeprom.use_eerd {
        return em_read_eeprom_eerd(hw, offset, words, data);
    }

    // ICH EEPROM access is done via the ICH flash controller.
    if hw.eeprom.r#type == em_eeprom_ich8 {
        return em_read_eeprom_ich8(hw, offset, words, data);
    }

    // Some i210/i211 have a special OTP chip.
    if hw.eeprom.r#type == em_eeprom_invm {
        return em_read_invm_i210(hw, offset, words, data);
    }

    // Set up the SPI or Microwire EEPROM for bit-bang reading. We have acquired the EEPROM
    // at this point, so any returns should release it.
    if hw.eeprom.r#type == em_eeprom_spi {
        let mut read_opcode: u8 = EEPROM_READ_OPCODE_SPI as u8;
        if em_spi_eeprom_ready(hw).is_err() {
            em_release_eeprom(hw);
            return Err(-E1000_ERR_EEPROM);
        }
        em_standby_eeprom(hw);
        // Some SPI eeproms use the 8th address bit embedded in the opcode.
        if hw.eeprom.address_bits == 8 && offset >= 128 {
            read_opcode |= EEPROM_A8_OPCODE_SPI as u8;
        }

        // Send the READ command (opcode + addr).
        em_shift_out_ee_bits(hw, u16::from(read_opcode), hw.eeprom.opcode_bits);
        em_shift_out_ee_bits(hw, offset.wrapping_mul(2), hw.eeprom.address_bits);
        // Read the data. The address of the eeprom internally increments with each byte
        // (spi) being read, saving on the overhead of eeprom setup and tear-down. The
        // address counter will roll over if reading beyond the size of the eeprom, thus
        // allowing the entire memory to be read starting from any offset.
        for d in data.iter_mut().take(usize::from(words)) {
            let word_in = em_shift_in_ee_bits(hw, 16);
            *d = word_in.rotate_left(8);
        }
    } else if hw.eeprom.r#type == em_eeprom_microwire {
        for (i, d) in data.iter_mut().take(usize::from(words)).enumerate() {
            // Send the READ command (opcode + addr).
            em_shift_out_ee_bits(hw, EEPROM_READ_OPCODE_MICROWIRE, hw.eeprom.opcode_bits);
            em_shift_out_ee_bits(hw, offset.wrapping_add(i as u16), hw.eeprom.address_bits);
            // Read the data. For microwire, each word requires the overhead of eeprom setup
            // and tear-down.
            *d = em_shift_in_ee_bits(hw, 16);
            em_standby_eeprom(hw);
        }
    }
    // End this read operation.
    em_release_eeprom(hw);

    Ok(())
}

/// Reads 16-bit words from the EEPROM using the EERD register.
fn em_read_eeprom_eerd(hw: &EmHw, offset: u16, words: u16, data: &mut [u16]) -> Result<(), i32> {
    for (i, d) in data.iter_mut().take(usize::from(words)).enumerate() {
        let eerd = ((u32::from(offset) + i as u32) << E1000_EEPROM_RW_ADDR_SHIFT)
            + E1000_EEPROM_RW_REG_START;

        e1000_write_reg(hw, E1000_EERD, eerd);
        em_poll_eerd_eewr_done(hw, E1000_EEPROM_POLL_READ)?;
        *d = (e1000_read_reg(hw, E1000_EERD) >> E1000_EEPROM_RW_REG_DATA) as u16;
    }

    Ok(())
}

/// Writes 16-bit words to the EEPROM using the EEWR register.
fn em_write_eeprom_eewr(hw: &mut EmHw, offset: u16, words: u16, data: &[u16]) -> Result<(), i32> {
    if em_swfw_sync_acquire(hw, E1000_SWFW_EEP_SM).is_err() {
        return Err(-E1000_ERR_SWFW_SYNC);
    }

    let mut error = Ok(());
    for (i, d) in data.iter().take(usize::from(words)).enumerate() {
        let register_value = (u32::from(*d) << E1000_EEPROM_RW_REG_DATA)
            | ((u32::from(offset) + i as u32) << E1000_EEPROM_RW_ADDR_SHIFT)
            | E1000_EEPROM_RW_REG_START;

        error = em_poll_eerd_eewr_done(hw, E1000_EEPROM_POLL_WRITE);
        if error.is_err() {
            break;
        }
        e1000_write_reg(hw, E1000_EEWR, register_value);

        error = em_poll_eerd_eewr_done(hw, E1000_EEPROM_POLL_WRITE);

        if error.is_err() {
            break;
        }
    }

    em_swfw_sync_release(hw, E1000_SWFW_EEP_SM);
    error
}

/// Polls the status bit (bit 1) of the EERD (`eerd` is `E1000_EEPROM_POLL_READ`) or EEWR
/// register to determine when the access is done; the C's positive `E1000_ERR_EEPROM` on a
/// timeout.
fn em_poll_eerd_eewr_done(hw: &EmHw, eerd: u32) -> Result<(), i32> {
    let attempts: u32 = 100000;
    for _ in 0..attempts {
        let reg = if eerd == E1000_EEPROM_POLL_READ {
            e1000_read_reg(hw, E1000_EERD)
        } else {
            e1000_read_reg(hw, E1000_EEWR)
        };

        if reg & E1000_EEPROM_RW_REG_DONE != 0 {
            return Ok(());
        }
        usec_delay(5);
    }

    Err(E1000_ERR_EEPROM)
}

/// Determines if the onboard NVM is FLASH (false) or EEPROM (true).
fn em_is_onboard_nvm_eeprom(hw: &EmHw) -> bool {
    if is_ich8(hw.mac_type) {
        return false;
    }

    if hw.mac_type == em_82573 || hw.mac_type == em_82574 {
        let mut eecd = e1000_read_reg(hw, E1000_EECD);

        // Isolate bits 15 & 16.
        eecd = (eecd >> 15) & 0x03;

        // If both bits are set, device is Flash type.
        if eecd == 0x03 {
            return false;
        }
    }
    true
}

/// `em_get_flash_presence_i210`: whether a flash device is detected (always true but on
/// the i210, whose `EECD.FLUPD` tells).
pub fn em_get_flash_presence_i210(hw: &EmHw) -> bool {
    if hw.mac_type != em_i210 {
        return true;
    }

    let eecd = e1000_read_reg(hw, E1000_EECD);

    eecd & E1000_EECD_FLUPD != 0
}

/// `em_validate_eeprom_checksum`: verifies that the EEPROM has a valid checksum.
///
/// Reads the first 64 16 bit words of the EEPROM and sums the values read. If the sum of
/// the 64 16 bit words is 0xBABA, the EEPROM's checksum is valid.
pub fn em_validate_eeprom_checksum(hw: &mut EmHw) -> Result<(), i32> {
    let mut checksum: u16 = 0;
    let mut eeprom_data = [0u16; 1];

    let checksum_reg = if hw.mac_type != em_icp_xxxx {
        EEPROM_CHECKSUM_REG
    } else {
        EEPROM_CHECKSUM_REG_ICP_xxxx
    };

    if (hw.mac_type == em_82573 || hw.mac_type == em_82574) && !em_is_onboard_nvm_eeprom(hw) {
        // Check bit 4 of word 10h. If it is 0, firmware is done updating 10h-12h. Checksum
        // may need to be fixed.
        let _ = em_read_eeprom(hw, 0x10, 1, &mut eeprom_data);
        if eeprom_data[0] & 0x10 == 0 {
            // Read 0x23 and check bit 15. This bit is a 1 when the checksum has already been
            // fixed. If the checksum is still wrong and this bit is a 1, we need to return
            // bad checksum. Otherwise, we need to set this bit to a 1 and update the
            // checksum.
            let _ = em_read_eeprom(hw, 0x23, 1, &mut eeprom_data);
            if eeprom_data[0] & 0x8000 == 0 {
                eeprom_data[0] |= 0x8000;
                let _ = em_write_eeprom(hw, 0x23, 1, &eeprom_data);
                let _ = em_update_eeprom_checksum(hw);
            }
        }
    }
    if is_ich8(hw.mac_type) {
        // Drivers must allocate the shadow ram structure for the EEPROM checksum to be
        // updated. Otherwise, this bit as well as the checksum must both be set correctly
        // for this validation to pass.
        let (word, valid_csum_mask) = match hw.mac_type {
            em_pch_lpt | em_pch_spt | em_pch_cnp | em_pch_tgp | em_pch_adp | em_pch_mtp
            | em_pch_ptp => (EEPROM_COMPAT, EEPROM_COMPAT_VALID_CSUM),
            _ => (
                EEPROM_FUTURE_INIT_WORD1,
                EEPROM_FUTURE_INIT_WORD1_VALID_CSUM,
            ),
        };
        let _ = em_read_eeprom(hw, word, 1, &mut eeprom_data);
        if eeprom_data[0] & valid_csum_mask == 0 {
            eeprom_data[0] |= valid_csum_mask;
            let _ = em_write_eeprom(hw, word, 1, &eeprom_data);
            let _ = em_update_eeprom_checksum(hw);
        }
    }
    for i in 0..=checksum_reg {
        if em_read_eeprom(hw, i, 1, &mut eeprom_data).is_err() {
            // EEPROM Read Error.
            return Err(-E1000_ERR_EEPROM);
        }
        checksum = checksum.wrapping_add(eeprom_data[0]);
    }

    if checksum == EEPROM_SUM {
        Ok(())
    } else {
        // EEPROM Checksum Invalid.
        Err(-E1000_ERR_EEPROM)
    }
}

/// `em_update_eeprom_checksum`: calculates the EEPROM checksum and writes it to the EEPROM.
///
/// Sums the first 63 16 bit words of the EEPROM. Subtracts the sum from 0xBABA. Writes the
/// difference to word offset 63 of the EEPROM.
pub fn em_update_eeprom_checksum(hw: &mut EmHw) -> Result<(), i32> {
    let mut checksum: u16 = 0;
    let mut eeprom_data = [0u16; 1];

    for i in 0..EEPROM_CHECKSUM_REG {
        if em_read_eeprom(hw, i, 1, &mut eeprom_data).is_err() {
            // EEPROM Read Error.
            return Err(-E1000_ERR_EEPROM);
        }
        checksum = checksum.wrapping_add(eeprom_data[0]);
    }
    checksum = EEPROM_SUM.wrapping_sub(checksum);
    if em_write_eeprom(hw, EEPROM_CHECKSUM_REG, 1, &[checksum]).is_err() {
        // EEPROM Write Error.
        return Err(-E1000_ERR_EEPROM);
    } else if hw.eeprom.r#type == em_eeprom_flash {
        let _ = em_commit_shadow_ram(hw);
    } else if hw.eeprom.r#type == em_eeprom_ich8 {
        let _ = em_commit_shadow_ram(hw);
        // Reload the EEPROM, or else modifications will not appear until after next adapter
        // reset.
        let mut ctrl_ext = e1000_read_reg(hw, E1000_CTRL_EXT);
        ctrl_ext |= E1000_CTRL_EXT_EE_RST;
        e1000_write_reg(hw, E1000_CTRL_EXT, ctrl_ext);
        msec_delay(10);
    }
    Ok(())
}

/// `em_write_eeprom`: the parent function for writing `words` words of `data` at word
/// `offset` of the different EEPROM types. If `em_update_eeprom_checksum` is not called
/// after this function, the EEPROM will most likely contain an invalid checksum. A `data`
/// shorter than `words` is refused (`-E1000_ERR_EEPROM`).
pub fn em_write_eeprom(hw: &mut EmHw, offset: u16, words: u16, data: &[u16]) -> Result<(), i32> {
    // If eeprom is not yet detected, do so now.
    if hw.eeprom.word_size == 0 {
        let _ = em_init_eeprom_params(hw);
    }
    // A check for invalid values: offset too large, too many words, and not enough words.
    if offset >= hw.eeprom.word_size
        || words > hw.eeprom.word_size - offset
        || words == 0
        || data.len() < usize::from(words)
    {
        // "words" parameter out of bounds.
        return Err(-E1000_ERR_EEPROM);
    }
    // 82573/4 writes only through eewr.
    if hw.eeprom.use_eewr {
        return em_write_eeprom_eewr(hw, offset, words, data);
    }

    if hw.eeprom.r#type == em_eeprom_ich8 {
        return em_write_eeprom_ich8(hw, offset, words, data);
    }

    // Prepare the EEPROM for writing.
    if em_acquire_eeprom(hw).is_err() {
        return Err(-E1000_ERR_EEPROM);
    }

    let status = if hw.eeprom.r#type == em_eeprom_microwire {
        em_write_eeprom_microwire(hw, offset, words, data)
    } else {
        let s = em_write_eeprom_spi(hw, offset, words, data);
        msec_delay(10);
        s
    };

    // Done with writing.
    em_release_eeprom(hw);

    status
}

/// Writes 16-bit words to a given offset in an SPI EEPROM.
fn em_write_eeprom_spi(hw: &EmHw, offset: u16, words: u16, data: &[u16]) -> Result<(), i32> {
    let mut widx: u16 = 0;

    while widx < words {
        let mut write_opcode: u8 = EEPROM_WRITE_OPCODE_SPI as u8;
        if em_spi_eeprom_ready(hw).is_err() {
            return Err(-E1000_ERR_EEPROM);
        }

        em_standby_eeprom(hw);

        // Send the WRITE ENABLE command (8 bit opcode).
        em_shift_out_ee_bits(hw, EEPROM_WREN_OPCODE_SPI, hw.eeprom.opcode_bits);

        em_standby_eeprom(hw);
        // Some SPI eeproms use the 8th address bit embedded in the opcode.
        if hw.eeprom.address_bits == 8 && offset >= 128 {
            write_opcode |= EEPROM_A8_OPCODE_SPI as u8;
        }

        // Send the Write command (8-bit opcode + addr).
        em_shift_out_ee_bits(hw, u16::from(write_opcode), hw.eeprom.opcode_bits);

        em_shift_out_ee_bits(
            hw,
            offset.wrapping_add(widx).wrapping_mul(2),
            hw.eeprom.address_bits,
        );

        // Send the data. Loop to allow for up to whole page write (32 bytes) of eeprom.
        while widx < words {
            let word_out = data[usize::from(widx)].rotate_left(8);
            em_shift_out_ee_bits(hw, word_out, 16);
            widx += 1;
            // Some larger eeprom sizes are capable of a 32-byte PAGE WRITE operation, while
            // the smaller eeproms are capable of an 8-byte PAGE WRITE operation. Break the
            // inner loop to pass new address.
            if hw.eeprom.page_size != 0
                && (u32::from(offset) + u32::from(widx)) * 2 % u32::from(hw.eeprom.page_size) == 0
            {
                em_standby_eeprom(hw);
                break;
            }
        }
    }

    Ok(())
}

/// Writes 16-bit words to a given offset in a Microwire EEPROM.
fn em_write_eeprom_microwire(hw: &EmHw, offset: u16, words: u16, data: &[u16]) -> Result<(), i32> {
    let mut words_written: u16 = 0;
    // Send the write enable command to the EEPROM (3-bit opcode plus 6/8-bit dummy address
    // beginning with 11). It's less work to include the 11 of the dummy address as part of
    // the opcode than it is to shift it over the correct number of bits for the address.
    // This puts the EEPROM into write/erase mode.
    em_shift_out_ee_bits(
        hw,
        EEPROM_EWEN_OPCODE_MICROWIRE,
        hw.eeprom.opcode_bits.wrapping_add(2),
    );

    em_shift_out_ee_bits(hw, 0, hw.eeprom.address_bits.wrapping_sub(2));

    // Prepare the EEPROM.
    em_standby_eeprom(hw);

    while words_written < words {
        // Send the Write command (3-bit opcode + addr).
        em_shift_out_ee_bits(hw, EEPROM_WRITE_OPCODE_MICROWIRE, hw.eeprom.opcode_bits);

        em_shift_out_ee_bits(
            hw,
            offset.wrapping_add(words_written),
            hw.eeprom.address_bits,
        );

        // Send the data.
        em_shift_out_ee_bits(hw, data[usize::from(words_written)], 16);
        // Toggle the CS line. This in effect tells the EEPROM to execute the previous
        // command.
        em_standby_eeprom(hw);
        // Read DO repeatedly until it is high (equal to '1'). The EEPROM will signal that
        // the command has been completed by raising the DO signal. If DO does not go high in
        // 10 milliseconds, then error out.
        let mut i = 0;
        while i < 200 {
            let eecd = e1000_read_reg(hw, E1000_EECD);
            if eecd & E1000_EECD_DO != 0 {
                break;
            }
            usec_delay(50);
            i += 1;
        }
        if i == 200 {
            // EEPROM Write did not complete.
            return Err(-E1000_ERR_EEPROM);
        }
        // Recover from write.
        em_standby_eeprom(hw);

        words_written += 1;
    }
    // Send the write disable command to the EEPROM (3-bit opcode plus 6/8-bit dummy address
    // beginning with 10). It's less work to include the 10 of the dummy address as part of
    // the opcode than it is to shift it over the correct number of bits for the address.
    // This takes the EEPROM out of write/erase mode.
    em_shift_out_ee_bits(
        hw,
        EEPROM_EWDS_OPCODE_MICROWIRE,
        hw.eeprom.opcode_bits.wrapping_add(2),
    );

    em_shift_out_ee_bits(hw, 0, hw.eeprom.address_bits.wrapping_sub(2));

    Ok(())
}

/// Word `i` of the shadow RAM; past the end of a shorter allocation (where the C would
/// read past it) the word reads as unmodified.
fn em_shadow_word(hw: &EmHw, i: u32) -> EmShadowRam {
    hw.eeprom_shadow_ram
        .as_deref()
        .and_then(|r| r.get(i as usize).copied())
        .unwrap_or(EmShadowRam {
            eeprom_word: 0xFFFF,
            modified: false,
        })
}

/// Flushes the cached eeprom to NVM. This is done by saving the modified values in the
/// eeprom cache and the non modified values in the currently active bank to the new bank.
fn em_commit_shadow_ram(hw: &mut EmHw) -> Result<(), i32> {
    let attempts: u32 = 100000;
    let mut error: Result<(), i32> = Ok(());
    let mut low_byte: u8 = 0;
    let mut high_byte: u8 = 0;

    if hw.mac_type == em_82573 || hw.mac_type == em_82574 {
        // The flop register will be used to determine if flash type is STM.
        let flop = e1000_read_reg(hw, E1000_FLOP);
        let mut eecd: u32 = 0;
        let mut i = 0;
        while i < attempts {
            eecd = e1000_read_reg(hw, E1000_EECD);
            if eecd & E1000_EECD_FLUPD == 0 {
                break;
            }
            usec_delay(5);
            i += 1;
        }

        if i == attempts {
            return Err(-E1000_ERR_EEPROM);
        }
        // If STM opcode located in bits 15:8 of flop, reset firmware.
        if (flop & 0xFF00) == E1000_STM_OPCODE {
            e1000_write_reg(hw, E1000_HICR, E1000_HICR_FW_RESET);
        }
        // Perform the flash update.
        e1000_write_reg(hw, E1000_EECD, eecd | E1000_EECD_FLUPD);

        i = 0;
        while i < attempts {
            eecd = e1000_read_reg(hw, E1000_EECD);
            if eecd & E1000_EECD_FLUPD == 0 {
                break;
            }
            usec_delay(5);
            i += 1;
        }

        if i == attempts {
            return Err(-E1000_ERR_EEPROM);
        }
    }
    if (hw.mac_type == em_ich8lan || hw.mac_type == em_ich9lan) && hw.eeprom_shadow_ram.is_some() {
        // We're writing to the opposite bank so if we're on bank 1, write to bank 0 etc. We
        // also need to erase the segment that is going to be written.
        let old_bank_offset;
        let new_bank_offset;
        if e1000_read_reg(hw, E1000_EECD) & E1000_EECD_SEC1VAL == 0 {
            new_bank_offset = hw.flash_bank_size * 2;
            old_bank_offset = 0;
            let _ = em_erase_ich8_4k_segment(hw, 1);
        } else {
            old_bank_offset = hw.flash_bank_size * 2;
            new_bank_offset = 0;
            let _ = em_erase_ich8_4k_segment(hw, 0);
        }

        let mut sector_write_failed = false;
        // Loop for every byte in the shadow RAM, which is in units of words.
        for i in 0..E1000_SHADOW_RAM_WORDS {
            // Determine whether to write the value stored in the other NVM bank or a
            // modified value stored in the shadow RAM.
            let shadow = em_shadow_word(hw, i);
            if shadow.modified {
                low_byte = shadow.eeprom_word as u8;
                usec_delay(100);
                error = em_verify_write_ich8_byte(hw, (i << 1) + new_bank_offset, low_byte);

                if error.is_err() {
                    sector_write_failed = true;
                } else {
                    high_byte = (shadow.eeprom_word >> 8) as u8;
                    usec_delay(100);
                }
            } else {
                let _ = em_read_ich8_byte(hw, (i << 1) + old_bank_offset, &mut low_byte);
                usec_delay(100);
                error = em_verify_write_ich8_byte(hw, (i << 1) + new_bank_offset, low_byte);

                if error.is_err() {
                    sector_write_failed = true;
                } else {
                    let _ = em_read_ich8_byte(hw, (i << 1) + old_bank_offset + 1, &mut high_byte);
                    usec_delay(100);
                }
            }
            // If the write of the low byte was successful, go ahread and write the high byte
            // while checking to make sure that if it is the signature byte, then it is
            // handled properly.
            if !sector_write_failed {
                // If the word is 0x13, then make sure the signature bits (15:14) are 11b
                // until the commit has completed. This will allow us to write 10b which
                // indicates the signature is valid. We want to do this after the write has
                // completed so that we don't mark the segment valid while the write is
                // still in progress.
                if i == E1000_ICH_NVM_SIG_WORD {
                    high_byte |= E1000_ICH_NVM_VALID_SIG_MASK as u8;
                }

                error = em_verify_write_ich8_byte(hw, (i << 1) + new_bank_offset + 1, high_byte);
                if error.is_err() {
                    sector_write_failed = true;
                }
            } else {
                // If the write failed then break from the loop and return an error.
                break;
            }
        }
        // Don't bother writing the segment valid bits if sector programming failed.
        if !sector_write_failed {
            // Finally validate the new segment by setting bit 15:14 to 10b in word 0x13 ,
            // this can be done without an erase as well since these bits are 11 to start
            // with and we need to change bit 14 to 0b.
            let _ = em_read_ich8_byte(
                hw,
                E1000_ICH_NVM_SIG_WORD * 2 + 1 + new_bank_offset,
                &mut high_byte,
            );
            high_byte &= 0xBF;
            error = em_verify_write_ich8_byte(
                hw,
                E1000_ICH_NVM_SIG_WORD * 2 + 1 + new_bank_offset,
                high_byte,
            );
            // And invalidate the previously valid segment by setting its signature word
            // (0x13) high_byte to 0b. This can be done without an erase because flash erase
            // sets all bits to 1's. We can write 1's to 0's without an erase.
            if error.is_ok() {
                error = em_verify_write_ich8_byte(
                    hw,
                    E1000_ICH_NVM_SIG_WORD * 2 + 1 + old_bank_offset,
                    0,
                );
            }
            // Clear the now not used entry in the cache.
            em_clear_shadow_ram(hw);
        }
    }
    error
}

/// `em_read_part_num`: reads the adapter's part number from the EEPROM.
pub fn em_read_part_num(hw: &mut EmHw, part_num: &mut u32) -> Result<(), i32> {
    let mut offset = EEPROM_PBA_BYTE_1;
    let mut eeprom_data = [0u16; 1];

    // Get word 0 from EEPROM.
    if em_read_eeprom(hw, offset, 1, &mut eeprom_data).is_err() {
        // EEPROM Read Error.
        return Err(-E1000_ERR_EEPROM);
    }
    // Save word 0 in upper half of part_num.
    *part_num = u32::from(eeprom_data[0]) << 16;

    // Get word 1 from EEPROM.
    offset += 1;
    if em_read_eeprom(hw, offset, 1, &mut eeprom_data).is_err() {
        // EEPROM Read Error.
        return Err(-E1000_ERR_EEPROM);
    }
    // Save word 1 in lower half of part_num.
    *part_num |= u32::from(eeprom_data[0]);

    Ok(())
}

/// `em_read_mac_addr`: reads the adapter's MAC address from the EEPROM into
/// `perm_mac_addr` and `mac_addr`, and inverts the LSB for the second function of dual
/// function devices.
pub fn em_read_mac_addr(hw: &mut EmHw) -> Result<(), i32> {
    let mut eeprom_data = [0u16; 1];
    let mut ia_base_addr: u16 = 0;

    if hw.mac_type == em_icp_xxxx {
        ia_base_addr = eeprom_ia_start_icp_xxxx(hw.icp_xxxx_port_num);
    } else if hw.mac_type == em_82580 || hw.mac_type == em_i350 {
        ia_base_addr = nvm_82580_lan_func_offset(hw.bus_func);
    }
    for i in (0..NODE_ADDRESS_SIZE).step_by(2) {
        let offset = (i >> 1) as u16;
        if em_read_eeprom(hw, offset.wrapping_add(ia_base_addr), 1, &mut eeprom_data).is_err() {
            // EEPROM Read Error.
            return Err(-E1000_ERR_EEPROM);
        }
        hw.perm_mac_addr[i] = (eeprom_data[0] & 0x00FF) as u8;
        hw.perm_mac_addr[i + 1] = (eeprom_data[0] >> 8) as u8;
    }

    if matches!(
        hw.mac_type,
        em_82546 | em_82546_rev_3 | em_82571 | em_82575 | em_82576 | em_80003es2lan
    ) && e1000_read_reg(hw, E1000_STATUS) & E1000_STATUS_FUNC_1 != 0
    {
        hw.perm_mac_addr[5] ^= 0x01;
    }

    hw.mac_addr = hw.perm_mac_addr;
    Ok(())
}

/// Explicitly disables jumbo frames and resets some PHY registers back to hw-defaults. This
/// is necessary in case the ethernet cable was inserted AFTER the firmware initialized the
/// PHY. Otherwise it is left in a state where it is possible to transmit but not receive
/// packets. Observed on I217-LM and fixed in FreeBSD's sys/dev/e1000/e1000_ich8lan.c.
fn em_phy_no_cable_workaround(hw: &mut EmHw) -> Result<(), i32> {
    let mut data: u16 = 0;
    let mut phy_reg_val: u16 = 0;

    // Disable Rx path while enabling workaround.
    let _ = em_read_phy_reg(hw, I2_DFT_CTRL, &mut phy_reg_val);
    em_write_phy_reg(hw, I2_DFT_CTRL, phy_reg_val | (1 << 14))?;

    // Write MAC register values back to h/w defaults.
    let mut mac_reg = e1000_read_reg(hw, E1000_FFLT_DBG);
    mac_reg &= !(0xF << 14);
    e1000_write_reg(hw, E1000_FFLT_DBG, mac_reg);

    mac_reg = e1000_read_reg(hw, E1000_RCTL);
    mac_reg &= !E1000_RCTL_SECRC;
    e1000_write_reg(hw, E1000_RCTL, mac_reg);

    let ret_val: Result<(), i32> = 'out: {
        let r = em_read_kmrn_reg(hw, E1000_KUMCTRLSTA_OFFSET_CTRL, &mut data);
        if r.is_err() {
            break 'out r;
        }
        let r = em_write_kmrn_reg(hw, E1000_KUMCTRLSTA_OFFSET_CTRL, data & !(1 << 0));
        if r.is_err() {
            break 'out r;
        }

        let r = em_read_kmrn_reg(hw, E1000_KUMCTRLSTA_OFFSET_HD_CTRL, &mut data);
        if r.is_err() {
            break 'out r;
        }

        data &= !(0xF << 8);
        data |= 0xB << 8;
        let r = em_write_kmrn_reg(hw, E1000_KUMCTRLSTA_OFFSET_HD_CTRL, data);
        if r.is_err() {
            break 'out r;
        }

        // Write PHY register values back to h/w defaults.
        let _ = em_read_phy_reg(hw, I2_SMBUS_CTRL, &mut data);
        data &= !(0x7F << 5);
        let r = em_write_phy_reg(hw, I2_SMBUS_CTRL, data);
        if r.is_err() {
            break 'out r;
        }

        let _ = em_read_phy_reg(hw, I2_MODE_CTRL, &mut data);
        data |= 1 << 13;
        let r = em_write_phy_reg(hw, I2_MODE_CTRL, data);
        if r.is_err() {
            break 'out r;
        }

        // 776.20 and 776.23 are not documented in i217-ethernet-controller-datasheet.pdf...
        let _ = em_read_phy_reg(hw, phy_reg(776, 20), &mut data);
        data &= !(0x3FF << 2);
        data |= 0x8 << 2;
        let r = em_write_phy_reg(hw, phy_reg(776, 20), data);
        if r.is_err() {
            break 'out r;
        }

        let r = em_write_phy_reg(hw, phy_reg(776, 23), 0x7E00);
        if r.is_err() {
            break 'out r;
        }

        let _ = em_read_phy_reg(hw, I2_PCIE_POWER_CTRL, &mut data);
        em_write_phy_reg(hw, I2_PCIE_POWER_CTRL, data & !(1 << 10))
    };

    // Re-enable Rx path after enabling workaround.
    let dft_ret_val = em_write_phy_reg(hw, I2_DFT_CTRL, phy_reg_val & !(1 << 14));
    if ret_val.is_err() {
        ret_val
    } else {
        dft_ret_val
    }
}

/// Initializes the receive address filters.
///
/// Places the MAC address in receive address register 0 and clears the rest of the receive
/// address registers. Clears the multicast table. Assumes the receiver is in reset when the
/// routine is called.
fn em_init_rx_addrs(hw: &mut EmHw) {
    if hw.mac_type >= em_pch2lan && em_phy_no_cable_workaround(hw).is_err() {
        printf(format_args!(
            " ...failed to apply em_phy_no_cable_workaround.\n"
        ));
    }

    // Setup the receive address.
    let mac_addr = hw.mac_addr;
    em_rar_set(hw, &mac_addr, 0);

    let mut rar_num = E1000_RAR_ENTRIES;
    // Reserve a spot for the Locally Administered Address to work around an 82571 issue in
    // which a reset on one port will reload the MAC on the other port.
    if hw.mac_type == em_82571 && hw.laa_is_present {
        rar_num -= 1;
    }
    if is_ich8(hw.mac_type) {
        rar_num = E1000_RAR_ENTRIES_ICH8LAN;
    }
    if hw.mac_type == em_ich8lan {
        rar_num -= 1;
    }
    if hw.mac_type == em_82580 {
        rar_num = E1000_RAR_ENTRIES_82580;
    }
    if hw.mac_type == em_i210 {
        rar_num = E1000_RAR_ENTRIES_82575;
    }
    if hw.mac_type == em_i350 {
        rar_num = E1000_RAR_ENTRIES_I350;
    }

    // Zero out the other 15 receive addresses.
    for i in 1..rar_num {
        e1000_write_reg_array(hw, E1000_RA, i << 1, 0);
        e1000_write_flush(hw);
        e1000_write_reg_array(hw, E1000_RA, (i << 1) + 1, 0);
        e1000_write_flush(hw);
    }
}

/// `em_mc_addr_list_update`: updates the MAC's list of multicast addresses.
///
/// `mc_addr_list` holds `mc_addr_count` addresses, each followed by `pad` bytes. The given
/// list replaces any existing list and hashes the addresses into the multicast table. A
/// list shorter than `mc_addr_count` entries ends the update at its end (the C reads past
/// it).
pub fn em_mc_addr_list_update(hw: &mut EmHw, mc_addr_list: &[u8], mc_addr_count: u32, pad: u32) {
    // Set the new number of MC addresses that we are being requested to use.
    hw.num_mc_addrs = mc_addr_count;

    // Clear the MTA.
    let mut num_mta_entry = E1000_NUM_MTA_REGISTERS;
    if is_ich8(hw.mac_type) {
        num_mta_entry = E1000_NUM_MTA_REGISTERS_ICH8LAN;
    }

    for i in 0..num_mta_entry {
        e1000_write_reg_array(hw, E1000_MTA, i, 0);
        e1000_write_flush(hw);
    }

    // Add the new addresses.
    let stride = ETH_LENGTH_OF_ADDRESS + pad as usize;
    for i in 0..mc_addr_count as usize {
        let start = i * stride;
        let Some(addr) = mc_addr_list
            .get(start..start + ETH_LENGTH_OF_ADDRESS)
            .and_then(|a| <&[u8; ETH_LENGTH_OF_ADDRESS]>::try_from(a).ok())
        else {
            break;
        };

        let hash_value = em_hash_mc_addr(hw, addr);

        em_mta_set(hw, hash_value);
    }
}

/// `em_hash_mc_addr`: hashes an address to determine its location in the multicast table.
pub fn em_hash_mc_addr(hw: &EmHw, mc_addr: &[u8; ETH_LENGTH_OF_ADDRESS]) -> u32 {
    let b4 = u32::from(mc_addr[4]);
    let b5 = u32::from(mc_addr[5]);
    let ich8 = is_ich8(hw.mac_type);
    // The portion of the address that is used for the hash table is determined by the
    // mc_filter_type setting.
    //   [0] [1] [2] [3] [4] [5]
    //    01  AA  00  12  34  56
    //   LSB                 MSB
    let mut hash_value = match hw.mc_filter_type {
        // [47:38] i.e. 0x158 for above example address (ICH8),
        // [47:36] i.e. 0x563 for above example address.
        0 if ich8 => (b4 >> 6) | (b5 << 2),
        0 => (b4 >> 4) | (b5 << 4),
        // [46:37] i.e. 0x2B1, [46:35] i.e. 0xAC6.
        1 if ich8 => (b4 >> 5) | (b5 << 3),
        1 => (b4 >> 3) | (b5 << 5),
        // [45:36] i.e. 0x163, [45:34] i.e. 0x5D8.
        2 if ich8 => (b4 >> 4) | (b5 << 4),
        2 => (b4 >> 2) | (b5 << 6),
        // [43:34] i.e. 0x18D, [43:32] i.e. 0x634.
        3 if ich8 => (b4 >> 2) | (b5 << 6),
        3 => b4 | (b5 << 8),
        _ => 0,
    };

    hash_value &= 0xFFF;
    if ich8 {
        hash_value &= 0x3FF;
    }

    hash_value
}

/// `em_mta_set`: sets the bit in the multicast table corresponding to the hash value.
pub fn em_mta_set(hw: &EmHw, hash_value: u32) {
    // The MTA is a register array of 128 32-bit registers. It is treated like an array of
    // 4096 bits. We want to set bit BitArray[hash_value]. So we figure out what register the
    // bit is in, read it, OR in the new bit, then write back the new value. The register is
    // determined by the upper 7 bits of the hash value and the bit within that register are
    // determined by the lower 5 bits of the value.
    let mut hash_reg = (hash_value >> 5) & 0x7F;
    if is_ich8(hw.mac_type) {
        hash_reg &= 0x1F;
    }

    let hash_bit = hash_value & 0x1F;

    let mut mta = e1000_read_reg_array(hw, E1000_MTA, hash_reg);

    mta |= 1 << hash_bit;
    // If we are on an 82544 and we are trying to write an odd offset in the MTA, save off
    // the previous entry before writing and restore the old value after writing.
    if hw.mac_type == em_82544 && (hash_reg & 0x1) == 1 {
        let temp = e1000_read_reg_array(hw, E1000_MTA, hash_reg - 1);
        e1000_write_reg_array(hw, E1000_MTA, hash_reg, mta);
        e1000_write_flush(hw);
        e1000_write_reg_array(hw, E1000_MTA, hash_reg - 1, temp);
        e1000_write_flush(hw);
    } else {
        e1000_write_reg_array(hw, E1000_MTA, hash_reg, mta);
        e1000_write_flush(hw);
    }
}

/// `em_rar_set`: puts an ethernet address into receive address register `index`.
pub fn em_rar_set(hw: &EmHw, addr: &[u8; ETH_LENGTH_OF_ADDRESS], index: u32) {
    // HW expects these in little endian so we reverse the byte order from network order
    // (big endian) to little endian.
    let rar_low = u32::from_le_bytes([addr[0], addr[1], addr[2], addr[3]]);
    let mut rar_high = u32::from(addr[4]) | (u32::from(addr[5]) << 8);
    // Disable Rx and flush all Rx frames before enabling RSS to avoid Rx unit hang.
    //
    // Description: If there are any Rx frames queued up or otherwise present in the HW
    // before RSS is enabled, and then we enable RSS, the HW Rx unit will hang. To work
    // around this issue, we have to disable receives and flush out all Rx frames before we
    // enable RSS. To do so, we modify we redirect all Rx traffic to manageability and then
    // reset the HW. This flushes away Rx frames, and (since the redirections to
    // manageability persists across resets) keeps new ones from coming in while we work.
    // Then, we clear the Address Valid AV bit for all MAC addresses and undo the
    // re-direction to manageability. Now, frames are coming in again, but the MAC won't
    // accept them, so far so good. We now proceed to initialize RSS (if necessary) and
    // configure the Rx unit. Last, we re-enable the AV bits and continue on our merry way.
    let leave_off =
        matches!(hw.mac_type, em_82571 | em_82572 | em_80003es2lan) && hw.leave_av_bit_off;
    if !leave_off {
        // Indicate to hardware the Address is Valid.
        rar_high |= E1000_RAH_AV;
    }

    e1000_write_reg_array(hw, E1000_RA, index << 1, rar_low);
    e1000_write_flush(hw);
    e1000_write_reg_array(hw, E1000_RA, (index << 1) + 1, rar_high);
    e1000_write_flush(hw);
}

/// Clears the VLAN filter table (but the manageability unit's VLAN on the 82573/82574).
fn em_clear_vfta(hw: &EmHw) {
    let mut vfta_offset: u32 = 0;
    let mut vfta_bit_in_reg: u32 = 0;
    if is_ich8(hw.mac_type) {
        return;
    }

    if (hw.mac_type == em_82573 || hw.mac_type == em_82574) && hw.mng_cookie.vlan_id != 0 {
        // The VFTA is a 4096b bit-field, each identifying a single VLAN ID. The following
        // operations determine which 32b entry (i.e. offset) into the array we want to set
        // the VLAN ID (i.e. bit) of the manageability unit.
        let vlan_id = u32::from(hw.mng_cookie.vlan_id);
        vfta_offset = (vlan_id >> E1000_VFTA_ENTRY_SHIFT) & E1000_VFTA_ENTRY_MASK;

        vfta_bit_in_reg = 1 << (vlan_id & E1000_VFTA_ENTRY_BIT_SHIFT_MASK);
    }
    for offset in 0..E1000_VLAN_FILTER_TBL_SIZE {
        // If the offset we want to clear is the same offset of the manageability VLAN ID,
        // then clear all bits except that of the manageability unit.
        let vfta_value = if offset == vfta_offset {
            vfta_bit_in_reg
        } else {
            0
        };
        e1000_write_reg_array(hw, E1000_VFTA, offset, vfta_value);
        e1000_write_flush(hw);
    }
}

/// `em_clear_vfta_i350`: clears the VLAN filter table of an i350. Due to hw errata, if the
/// host tries to configure the VFTA register while performing queries from the BMC or DMA,
/// then the VFTA in some cases won't be written.
pub fn em_clear_vfta_i350(hw: &EmHw) {
    for offset in 0..E1000_VLAN_FILTER_TBL_SIZE {
        for _ in 0..10 {
            e1000_write_reg_array(hw, E1000_VFTA, offset, 0);
        }
        e1000_write_flush(hw);
    }
}

/// Initializes the identification LED modes from the EEPROM.
fn em_id_led_init(hw: &mut EmHw) -> Result<(), i32> {
    let ledctl_mask: u32 = 0x000000FF;
    let ledctl_on: u32 = E1000_LEDCTL_MODE_LED_ON;
    let ledctl_off: u32 = E1000_LEDCTL_MODE_LED_OFF;
    let mut eeprom_data = [0u16; 1];
    let led_mask: u16 = 0x0F;

    if hw.mac_type < em_82540 || hw.mac_type == em_icp_xxxx {
        // Nothing to do.
        return Ok(());
    }
    let ledctl = e1000_read_reg(hw, E1000_LEDCTL);
    hw.ledctl_default = ledctl;
    hw.ledctl_mode1 = hw.ledctl_default;
    hw.ledctl_mode2 = hw.ledctl_default;

    if em_read_eeprom(hw, EEPROM_ID_LED_SETTINGS, 1, &mut eeprom_data).is_err() {
        // EEPROM Read Error.
        return Err(-E1000_ERR_EEPROM);
    }
    let mut eeprom_data = eeprom_data[0];
    if hw.mac_type == em_82573 && eeprom_data == ID_LED_RESERVED_82573 {
        eeprom_data = ID_LED_DEFAULT_82573;
    } else if eeprom_data == ID_LED_RESERVED_0000 || eeprom_data == ID_LED_RESERVED_FFFF {
        if hw.mac_type == em_ich8lan || hw.mac_type == em_ich9lan || hw.mac_type == em_ich10lan {
            eeprom_data = ID_LED_DEFAULT_ICH8LAN;
        } else {
            eeprom_data = ID_LED_DEFAULT;
        }
    }
    for i in 0..4u32 {
        let temp = (eeprom_data >> (i << 2)) & led_mask;
        match temp {
            ID_LED_ON1_DEF2 | ID_LED_ON1_ON2 | ID_LED_ON1_OFF2 => {
                hw.ledctl_mode1 &= !(ledctl_mask << (i << 3));
                hw.ledctl_mode1 |= ledctl_on << (i << 3);
            }
            ID_LED_OFF1_DEF2 | ID_LED_OFF1_ON2 | ID_LED_OFF1_OFF2 => {
                hw.ledctl_mode1 &= !(ledctl_mask << (i << 3));
                hw.ledctl_mode1 |= ledctl_off << (i << 3);
            }
            // Do nothing.
            _ => {}
        }
        match temp {
            ID_LED_DEF1_ON2 | ID_LED_ON1_ON2 | ID_LED_OFF1_ON2 => {
                hw.ledctl_mode2 &= !(ledctl_mask << (i << 3));
                hw.ledctl_mode2 |= ledctl_on << (i << 3);
            }
            ID_LED_DEF1_OFF2 | ID_LED_ON1_OFF2 | ID_LED_OFF1_OFF2 => {
                hw.ledctl_mode2 &= !(ledctl_mask << (i << 3));
                hw.ledctl_mode2 |= ledctl_off << (i << 3);
            }
            // Do nothing.
            _ => {}
        }
    }
    Ok(())
}

/// Reads each register of `regs` once (they clear on read).
fn em_read_clear(hw: &EmHw, regs: &[u32]) {
    for &r in regs {
        let _temp = e1000_read_reg(hw, r);
    }
}

/// `em_clear_hw_cntrs`: clears all hardware statistics counters (they clear on read).
pub fn em_clear_hw_cntrs(hw: &mut EmHw) {
    em_read_clear(
        hw,
        &[
            E1000_CRCERRS,
            E1000_SYMERRS,
            E1000_MPC,
            E1000_SCC,
            E1000_ECOL,
            E1000_MCC,
            E1000_LATECOL,
            E1000_COLC,
            E1000_DC,
            E1000_SEC,
            E1000_RLEC,
            E1000_XONRXC,
            E1000_XONTXC,
            E1000_XOFFRXC,
            E1000_XOFFTXC,
            E1000_FCRUC,
        ],
    );

    if !is_ich8(hw.mac_type) {
        em_read_clear(
            hw,
            &[
                E1000_PRC64,
                E1000_PRC127,
                E1000_PRC255,
                E1000_PRC511,
                E1000_PRC1023,
                E1000_PRC1522,
            ],
        );
    }
    em_read_clear(
        hw,
        &[
            E1000_GPRC,
            E1000_BPRC,
            E1000_MPRC,
            E1000_GPTC,
            E1000_GORCL,
            E1000_GORCH,
            E1000_GOTCL,
            E1000_GOTCH,
            E1000_RNBC,
            E1000_RUC,
            E1000_RFC,
            E1000_ROC,
            E1000_RJC,
            E1000_TORL,
            E1000_TORH,
            E1000_TOTL,
            E1000_TOTH,
            E1000_TPR,
            E1000_TPT,
        ],
    );

    if !is_ich8(hw.mac_type) {
        em_read_clear(
            hw,
            &[
                E1000_PTC64,
                E1000_PTC127,
                E1000_PTC255,
                E1000_PTC511,
                E1000_PTC1023,
                E1000_PTC1522,
            ],
        );
    }
    em_read_clear(hw, &[E1000_MPTC, E1000_BPTC]);

    if hw.mac_type < em_82543 {
        return;
    }

    em_read_clear(
        hw,
        &[
            E1000_ALGNERRC,
            E1000_RXERRC,
            E1000_TNCRS,
            E1000_CEXTERR,
            E1000_TSCTC,
            E1000_TSCTFC,
        ],
    );

    if hw.mac_type <= em_82544 || hw.mac_type == em_icp_xxxx {
        return;
    }

    em_read_clear(hw, &[E1000_MGTPRC, E1000_MGTPDC, E1000_MGTPTC]);

    if hw.mac_type <= em_82547_rev_2 {
        return;
    }

    em_read_clear(hw, &[E1000_IAC, E1000_ICRXOC]);

    if matches!(
        hw.phy_type,
        em_phy_82577 | em_phy_82578 | em_phy_82579 | em_phy_i217
    ) {
        let mut phy_data: u16 = 0;

        for reg in [
            HV_SCC_UPPER,
            HV_SCC_LOWER,
            HV_ECOL_UPPER,
            HV_ECOL_LOWER,
            HV_MCC_UPPER,
            HV_MCC_LOWER,
            HV_LATECOL_UPPER,
            HV_LATECOL_LOWER,
            HV_COLC_UPPER,
            HV_COLC_LOWER,
            HV_DC_UPPER,
            HV_DC_LOWER,
            HV_TNCRS_UPPER,
            HV_TNCRS_LOWER,
        ] {
            let _ = em_read_phy_reg(hw, reg, &mut phy_data);
        }
    }

    if hw.mac_type >= em_ich8lan {
        return;
    }

    em_read_clear(
        hw,
        &[
            E1000_ICRXPTC,
            E1000_ICRXATC,
            E1000_ICTXPTC,
            E1000_ICTXATC,
            E1000_ICTXQEC,
            E1000_ICTXQMTC,
            E1000_ICRXDMTC,
        ],
    );
}

/// `em_get_bus_info`: gets the current PCI bus type, speed, and width of the hardware.
pub fn em_get_bus_info(hw: &mut EmHw) {
    match hw.mac_type {
        em_82542_rev2_0 | em_82542_rev2_1 => {
            hw.bus_type = em_bus_type_unknown;
            hw.bus_speed = em_bus_speed_unknown;
            hw.bus_width = em_bus_width_unknown;
        }
        em_icp_xxxx => {
            hw.bus_type = em_bus_type_cpp;
            hw.bus_speed = em_bus_speed_unknown;
            hw.bus_width = em_bus_width_unknown;
        }
        em_82571 | em_82572 | em_82573 | em_82574 | em_82575 | em_82576 | em_82580
        | em_80003es2lan | em_i210 | em_i350 => {
            let mut pci_ex_link_status: u16 = 0;
            hw.bus_type = em_bus_type_pci_express;
            hw.bus_speed = em_bus_speed_2500;
            if em_read_pcie_cap_reg(hw, PCI_EX_LINK_STATUS, &mut pci_ex_link_status).is_err() {
                hw.bus_width = em_bus_width_unknown;
            } else {
                hw.bus_width = EmBusWidth::from_pcie_link_width(
                    (pci_ex_link_status & PCI_EX_LINK_WIDTH_MASK) >> PCI_EX_LINK_WIDTH_SHIFT,
                );
            }
        }
        em_ich8lan | em_ich9lan | em_ich10lan | em_pchlan | em_pch2lan | em_pch_lpt
        | em_pch_spt | em_pch_cnp | em_pch_tgp | em_pch_adp | em_pch_mtp | em_pch_ptp => {
            hw.bus_type = em_bus_type_pci_express;
            hw.bus_speed = em_bus_speed_2500;
            hw.bus_width = em_bus_width_pciex_1;
        }
        _ => {
            let status = e1000_read_reg(hw, E1000_STATUS);
            hw.bus_type = if status & E1000_STATUS_PCIX_MODE != 0 {
                em_bus_type_pcix
            } else {
                em_bus_type_pci
            };

            if hw.device_id == E1000_DEV_ID_82546EB_QUAD_COPPER {
                hw.bus_speed = if hw.bus_type == em_bus_type_pci {
                    em_bus_speed_66
                } else {
                    em_bus_speed_120
                };
            } else if hw.bus_type == em_bus_type_pci {
                hw.bus_speed = if status & E1000_STATUS_PCI66 != 0 {
                    em_bus_speed_66
                } else {
                    em_bus_speed_33
                };
            } else {
                hw.bus_speed = match status & E1000_STATUS_PCIX_SPEED {
                    E1000_STATUS_PCIX_SPEED_66 => em_bus_speed_66,
                    E1000_STATUS_PCIX_SPEED_100 => em_bus_speed_100,
                    E1000_STATUS_PCIX_SPEED_133 => em_bus_speed_133,
                    _ => em_bus_speed_reserved,
                };
            }
            hw.bus_width = if status & E1000_STATUS_BUS64 != 0 {
                em_bus_width_64
            } else {
                em_bus_width_32
            };
        }
    }
}

/// Writes a value to one of the device's registers using port I/O (as opposed to memory
/// mapped I/O). Only 82544 and newer devices support port I/O.
fn em_write_reg_io(hw: &EmHw, offset: u32, value: u32) {
    let io_addr = hw.io_base;
    let io_data = hw.io_base + 4;
    em_io_write(hw, io_addr, offset);
    em_io_write(hw, io_data, value);
}

/// Estimates the cable length, as a range (`min_length`, `max_length` in metres).
///
/// For M88 PHYs the one value the register gives is turned into a range; for IGP PHYs the
/// range is computed from the AGC registers.
#[allow(non_upper_case_globals)] // the C's enum members (em_cable_length_50, ...) as patterns
fn em_get_cable_length(
    hw: &mut EmHw,
    min_length: &mut u16,
    max_length: &mut u16,
) -> Result<(), i32> {
    let mut agc_value: u16 = 0;
    let mut phy_data: u16 = 0;

    *min_length = 0;
    *max_length = 0;

    // Use old method for Phy older than IGP.
    if hw.phy_type == em_phy_m88 || hw.phy_type == em_phy_oem || hw.phy_type == em_phy_82578 {
        em_read_phy_reg(hw, M88E1000_PHY_SPEC_STATUS, &mut phy_data)?;
        let cable_length =
            (phy_data & M88E1000_PSSR_CABLE_LENGTH) >> M88E1000_PSSR_CABLE_LENGTH_SHIFT;

        // Convert the enum value to ranged values.
        (*min_length, *max_length) = match cable_length {
            em_cable_length_50 => (0, em_igp_cable_length_50),
            em_cable_length_50_80 => (em_igp_cable_length_50, em_igp_cable_length_80),
            em_cable_length_80_110 => (em_igp_cable_length_80, em_igp_cable_length_110),
            em_cable_length_110_140 => (em_igp_cable_length_110, em_igp_cable_length_140),
            em_cable_length_140 => (em_igp_cable_length_140, em_igp_cable_length_170),
            _ => return Err(-E1000_ERR_PHY),
        };
    } else if hw.phy_type == em_phy_rtl8211 {
        // No cable length info on RTL8211, fake.
        *min_length = 0;
        *max_length = em_igp_cable_length_50;
    } else if hw.phy_type == em_phy_gg82563 {
        em_read_phy_reg(hw, GG82563_PHY_DSP_DISTANCE, &mut phy_data)?;
        let cable_length = phy_data & GG82563_DSPD_CABLE_LENGTH;

        (*min_length, *max_length) = match cable_length {
            em_gg_cable_length_60 => (0, em_igp_cable_length_60),
            em_gg_cable_length_60_115 => (em_igp_cable_length_60, em_igp_cable_length_115),
            em_gg_cable_length_115_150 => (em_igp_cable_length_115, em_igp_cable_length_150),
            em_gg_cable_length_150 => (em_igp_cable_length_150, em_igp_cable_length_180),
            _ => return Err(-E1000_ERR_PHY),
        };
    } else if hw.phy_type == em_phy_igp {
        // For IGP PHY.
        let mut min_agc_value = IGP01E1000_AGC_LENGTH_TABLE_SIZE as u16;
        let agc_reg_array: [u32; IGP01E1000_PHY_CHANNEL_NUM as usize] = [
            IGP01E1000_PHY_AGC_A,
            IGP01E1000_PHY_AGC_B,
            IGP01E1000_PHY_AGC_C,
            IGP01E1000_PHY_AGC_D,
        ];

        // Read the AGC registers for all channels.
        for reg in agc_reg_array {
            em_read_phy_reg(hw, reg, &mut phy_data)?;

            let cur_agc_value = phy_data >> IGP01E1000_AGC_LENGTH_SHIFT;

            // Value bound check.
            if usize::from(cur_agc_value) >= IGP01E1000_AGC_LENGTH_TABLE_SIZE - 1
                || cur_agc_value == 0
            {
                return Err(-E1000_ERR_PHY);
            }

            agc_value += cur_agc_value;

            // Update minimal AGC value.
            if min_agc_value > cur_agc_value {
                min_agc_value = cur_agc_value;
            }
        }

        // Remove the minimal AGC result for length < 50m.
        if u32::from(agc_value) < IGP01E1000_PHY_CHANNEL_NUM * u32::from(em_igp_cable_length_50) {
            agc_value -= min_agc_value;

            // Get the average length of the remaining 3 channels.
            agc_value /= (IGP01E1000_PHY_CHANNEL_NUM - 1) as u16;
        } else {
            // Get the average length of all the 4 channels.
            agc_value /= IGP01E1000_PHY_CHANNEL_NUM as u16;
        }

        // Set the range of the calculated length.
        let len = EM_IGP_CABLE_LENGTH_TABLE[usize::from(agc_value)];
        *min_length = len.saturating_sub(IGP01E1000_AGC_RANGE);
        *max_length = len + IGP01E1000_AGC_RANGE;
    } else if hw.phy_type == em_phy_igp_2 || hw.phy_type == em_phy_igp_3 {
        let mut max_agc_index: usize = 0;
        let mut min_agc_index: usize = IGP02E1000_AGC_LENGTH_TABLE_SIZE - 1;
        let agc_reg_array: [u32; IGP02E1000_PHY_CHANNEL_NUM as usize] = [
            IGP02E1000_PHY_AGC_A,
            IGP02E1000_PHY_AGC_B,
            IGP02E1000_PHY_AGC_C,
            IGP02E1000_PHY_AGC_D,
        ];
        let table = &EM_IGP_2_CABLE_LENGTH_TABLE;
        // Read the AGC registers for all channels.
        for reg in agc_reg_array {
            em_read_phy_reg(hw, reg, &mut phy_data)?;
            // Getting bits 15:9, which represent the combination of course and fine gain
            // values. The result is a number that can be put into the lookup table to obtain
            // the approximate cable length.
            let cur_agc_index =
                usize::from((phy_data >> IGP02E1000_AGC_LENGTH_SHIFT) & IGP02E1000_AGC_LENGTH_MASK);

            // Array index bound check.
            if cur_agc_index >= IGP02E1000_AGC_LENGTH_TABLE_SIZE || cur_agc_index == 0 {
                return Err(-E1000_ERR_PHY);
            }

            // Remove min & max AGC values from calculation.
            if table[min_agc_index] > table[cur_agc_index] {
                min_agc_index = cur_agc_index;
            }
            if table[max_agc_index] < table[cur_agc_index] {
                max_agc_index = cur_agc_index;
            }

            agc_value += table[cur_agc_index];
        }

        agc_value = agc_value.wrapping_sub(table[min_agc_index] + table[max_agc_index]);
        agc_value /= (IGP02E1000_PHY_CHANNEL_NUM - 2) as u16;
        // Calculate cable length with the error range of +/- 10 meters.
        *min_length = agc_value.saturating_sub(IGP02E1000_AGC_RANGE);
        *max_length = agc_value.wrapping_add(IGP02E1000_AGC_RANGE);
    }
    Ok(())
}

/// Checks if Downshift occurred and records it in `hw.speed_downgraded`.
///
/// For phy's older then IGP, this function reads the Downshift bit in the Phy Specific
/// Status register. For IGP phy's, it reads the Downgrade bit in the Link Health register.
/// In IGP this bit is latched high, so the driver must read it immediately after link is
/// established.
fn em_check_downshift(hw: &mut EmHw) -> Result<(), i32> {
    let mut phy_data: u16 = 0;

    if hw.phy_type == em_phy_igp || hw.phy_type == em_phy_igp_3 || hw.phy_type == em_phy_igp_2 {
        em_read_phy_reg(hw, IGP01E1000_PHY_LINK_HEALTH, &mut phy_data)?;

        hw.speed_downgraded = phy_data & IGP01E1000_PLHR_SS_DOWNGRADE != 0;
    } else if hw.phy_type == em_phy_m88
        || hw.phy_type == em_phy_gg82563
        || hw.phy_type == em_phy_oem
        || hw.phy_type == em_phy_82578
    {
        em_read_phy_reg(hw, M88E1000_PHY_SPEC_STATUS, &mut phy_data)?;

        hw.speed_downgraded =
            (phy_data & M88E1000_PSSR_DOWNSHIFT) >> M88E1000_PSSR_DOWNSHIFT_SHIFT != 0;
    } else if hw.phy_type == em_phy_ife {
        // em_phy_ife supports 10/100 speed only.
        hw.speed_downgraded = false;
    }
    Ok(())
}

/// Configures the DSP when a gigabit link is achieved to improve link quality (the
/// 82541_rev_2 and 82547_rev_2 have the capability), and restores it when the link goes
/// down.
fn em_config_dsp_after_link_change(hw: &mut EmHw, link_up: bool) -> Result<(), i32> {
    let mut phy_data: u16 = 0;
    let mut phy_saved_data: u16 = 0;
    let mut speed: u16 = 0;
    let mut duplex: u16 = 0;
    let dsp_reg_array: [u32; IGP01E1000_PHY_CHANNEL_NUM as usize] = [
        IGP01E1000_PHY_AGC_PARAM_A,
        IGP01E1000_PHY_AGC_PARAM_B,
        IGP01E1000_PHY_AGC_PARAM_C,
        IGP01E1000_PHY_AGC_PARAM_D,
    ];
    let mut min_length: u16 = 0;
    let mut max_length: u16 = 0;

    if hw.phy_type != em_phy_igp {
        return Ok(());
    }

    if link_up {
        em_get_speed_and_duplex(hw, &mut speed, &mut duplex)?;
        if speed == SPEED_1000 {
            em_get_cable_length(hw, &mut min_length, &mut max_length)?;

            if hw.dsp_config_state == em_dsp_config_enabled && min_length >= em_igp_cable_length_50
            {
                for reg in dsp_reg_array {
                    em_read_phy_reg(hw, reg, &mut phy_data)?;

                    phy_data &= !IGP01E1000_PHY_EDAC_MU_INDEX;

                    em_write_phy_reg(hw, reg, phy_data)?;
                }
                hw.dsp_config_state = em_dsp_config_activated;
            }
            if hw.ffe_config_state == em_ffe_config_enabled && min_length < em_igp_cable_length_50 {
                let mut ffe_idle_err_timeout = FFE_IDLE_ERR_COUNT_TIMEOUT_20;
                let mut idle_errs: u32 = 0;
                // Clear previous idle error counts.
                em_read_phy_reg(hw, PHY_1000T_STATUS, &mut phy_data)?;

                let mut i = 0;
                while i < ffe_idle_err_timeout {
                    usec_delay(1000);
                    em_read_phy_reg(hw, PHY_1000T_STATUS, &mut phy_data)?;

                    idle_errs += u32::from(phy_data & SR_1000T_IDLE_ERROR_CNT);
                    if idle_errs > SR_1000T_PHY_EXCESSIVE_IDLE_ERR_COUNT {
                        hw.ffe_config_state = em_ffe_config_active;

                        em_write_phy_reg(hw, IGP01E1000_PHY_DSP_FFE, IGP01E1000_PHY_DSP_FFE_CM_CP)?;
                        break;
                    }
                    if idle_errs != 0 {
                        ffe_idle_err_timeout = FFE_IDLE_ERR_COUNT_TIMEOUT_100;
                    }
                    i += 1;
                }
            }
        }
    } else {
        if hw.dsp_config_state == em_dsp_config_activated {
            // Save off the current value of register 0x2F5B to be restored at the end of the
            // routines.
            em_read_phy_reg(hw, 0x2F5B, &mut phy_saved_data)?;

            // Disable the PHY transmitter.
            em_write_phy_reg(hw, 0x2F5B, 0x0003)?;

            msec_delay_irq(20);

            em_write_phy_reg(hw, 0x0000, IGP01E1000_IEEE_FORCE_GIGA)?;
            for reg in dsp_reg_array {
                em_read_phy_reg(hw, reg, &mut phy_data)?;

                phy_data &= !IGP01E1000_PHY_EDAC_MU_INDEX;
                phy_data |= IGP01E1000_PHY_EDAC_SIGN_EXT_9_BITS;

                em_write_phy_reg(hw, reg, phy_data)?;
            }

            em_write_phy_reg(hw, 0x0000, IGP01E1000_IEEE_RESTART_AUTONEG)?;

            msec_delay_irq(20);

            // Now enable the transmitter.
            em_write_phy_reg(hw, 0x2F5B, phy_saved_data)?;

            hw.dsp_config_state = em_dsp_config_enabled;
        }
        if hw.ffe_config_state == em_ffe_config_active {
            // Save off the current value of register 0x2F5B to be restored at the end of the
            // routines.
            em_read_phy_reg(hw, 0x2F5B, &mut phy_saved_data)?;

            // Disable the PHY transmitter.
            em_write_phy_reg(hw, 0x2F5B, 0x0003)?;

            msec_delay_irq(20);

            em_write_phy_reg(hw, 0x0000, IGP01E1000_IEEE_FORCE_GIGA)?;
            em_write_phy_reg(hw, IGP01E1000_PHY_DSP_FFE, IGP01E1000_PHY_DSP_FFE_DEFAULT)?;

            em_write_phy_reg(hw, 0x0000, IGP01E1000_IEEE_RESTART_AUTONEG)?;

            msec_delay_irq(20);

            // Now enable the transmitter.
            em_write_phy_reg(hw, 0x2F5B, phy_saved_data)?;

            hw.ffe_config_state = em_ffe_config_enabled;
        }
    }
    Ok(())
}

/// Sets the PHY to class A mode. Assumes the following operations will follow to enable
/// the new class mode: a PHY soft reset, then restarting auto-negotiation or forcing link.
fn em_set_phy_mode(hw: &mut EmHw) -> Result<(), i32> {
    let mut eeprom_data = [0u16; 1];

    if hw.mac_type == em_82545_rev_3 && hw.media_type == em_media_type_copper {
        em_read_eeprom(hw, EEPROM_PHY_CLASS_WORD, 1, &mut eeprom_data)?;
        if eeprom_data[0] != EEPROM_RESERVED_WORD && eeprom_data[0] & EEPROM_PHY_CLASS_A != 0 {
            em_write_phy_reg(hw, M88E1000_PHY_PAGE_SELECT, 0x000B)?;
            em_write_phy_reg(hw, M88E1000_PHY_GEN_CONTROL, 0x8104)?;

            hw.phy_reset_disable = false;
        }
    }
    Ok(())
}

/// Turns SmartSpeed on or off in the IGP port configuration as `hw.smart_speed` asks
/// (nothing for `em_smart_speed_default`); the part the D3 and D0 LPLU functions share when
/// they disable LPLU.
fn em_lplu_restore_smart_speed(hw: &mut EmHw) -> Result<(), i32> {
    let mut phy_data: u16 = 0;
    // LPLU and SmartSpeed are mutually exclusive. LPLU is used during Dx states where the
    // power conservation is most important. During driver activity we should enable
    // SmartSpeed, so performance is maintained.
    if hw.smart_speed == em_smart_speed_on {
        em_read_phy_reg(hw, IGP01E1000_PHY_PORT_CONFIG, &mut phy_data)?;

        phy_data |= IGP01E1000_PSCFR_SMART_SPEED;
        em_write_phy_reg(hw, IGP01E1000_PHY_PORT_CONFIG, phy_data)?;
    } else if hw.smart_speed == em_smart_speed_off {
        em_read_phy_reg(hw, IGP01E1000_PHY_PORT_CONFIG, &mut phy_data)?;

        phy_data &= !IGP01E1000_PSCFR_SMART_SPEED;
        em_write_phy_reg(hw, IGP01E1000_PHY_PORT_CONFIG, phy_data)?;
    }
    Ok(())
}

/// When LPLU is enabled we should disable SmartSpeed (shared by the D3 and D0 LPLU
/// functions).
fn em_lplu_disable_smart_speed(hw: &mut EmHw) -> Result<(), i32> {
    let mut phy_data: u16 = 0;
    em_read_phy_reg(hw, IGP01E1000_PHY_PORT_CONFIG, &mut phy_data)?;

    phy_data &= !IGP01E1000_PSCFR_SMART_SPEED;
    em_write_phy_reg(hw, IGP01E1000_PHY_PORT_CONFIG, phy_data)
}

/// Sets the LPLU (D3) state according to `active`. When activating LPLU this function also
/// disables smart speed and vice versa. LPLU will not be activated unless the device
/// autonegotiation advertisement meets standards of either 10 or 10/100 or 10/100/1000 at
/// all duplexes.
fn em_set_d3_lplu_state(hw: &mut EmHw, active: bool) -> Result<(), i32> {
    let mut phy_ctrl: u32 = 0;
    let mut phy_data: u16 = 0;

    if hw.phy_type != em_phy_igp && hw.phy_type != em_phy_igp_2 && hw.phy_type != em_phy_igp_3 {
        return Ok(());
    }
    let rev2 = hw.mac_type == em_82541_rev_2 || hw.mac_type == em_82547_rev_2;
    // During driver activity LPLU should not be used or it will attain link from the lowest
    // speeds starting from 10Mbps. The capability is used for Dx transitions and states.
    if rev2 {
        em_read_phy_reg(hw, IGP01E1000_GMII_FIFO, &mut phy_data)?;
    } else if is_ich8(hw.mac_type) {
        // MAC writes into PHY register based on the state transition and start
        // auto-negotiation. SW driver can overwrite the settings in CSR PHY power control
        // E1000_PHY_CTRL register.
        phy_ctrl = e1000_read_reg(hw, E1000_PHY_CTRL);
    } else {
        em_read_phy_reg(hw, IGP02E1000_PHY_POWER_MGMT, &mut phy_data)?;
    }

    if !active {
        if rev2 {
            phy_data &= !IGP01E1000_GMII_FLEX_SPD;
            em_write_phy_reg(hw, IGP01E1000_GMII_FIFO, phy_data)?;
        } else if is_ich8(hw.mac_type) {
            phy_ctrl &= !E1000_PHY_CTRL_NOND0A_LPLU;
            e1000_write_reg(hw, E1000_PHY_CTRL, phy_ctrl);
        } else {
            phy_data &= !IGP02E1000_PM_D3_LPLU;
            em_write_phy_reg(hw, IGP02E1000_PHY_POWER_MGMT, phy_data)?;
        }
        em_lplu_restore_smart_speed(hw)?;
    } else if hw.autoneg_advertised == AUTONEG_ADVERTISE_SPEED_DEFAULT
        || hw.autoneg_advertised == AUTONEG_ADVERTISE_10_ALL
        || hw.autoneg_advertised == AUTONEG_ADVERTISE_10_100_ALL
    {
        if rev2 {
            phy_data |= IGP01E1000_GMII_FLEX_SPD;
            em_write_phy_reg(hw, IGP01E1000_GMII_FIFO, phy_data)?;
        } else if is_ich8(hw.mac_type) {
            phy_ctrl |= E1000_PHY_CTRL_NOND0A_LPLU;
            e1000_write_reg(hw, E1000_PHY_CTRL, phy_ctrl);
        } else {
            phy_data |= IGP02E1000_PM_D3_LPLU;
            em_write_phy_reg(hw, IGP02E1000_PHY_POWER_MGMT, phy_data)?;
        }

        // When LPLU is enabled we should disable SmartSpeed.
        em_lplu_disable_smart_speed(hw)?;
    }
    Ok(())
}

/// Sets the LPLU D0 state according to `active`. When activating LPLU this function also
/// disables smart speed and vice versa.
fn em_set_d0_lplu_state(hw: &mut EmHw, active: bool) -> Result<(), i32> {
    let mut phy_ctrl: u32 = 0;
    let mut phy_data: u16 = 0;

    if hw.mac_type <= em_82547_rev_2 {
        return Ok(());
    }

    if is_ich8(hw.mac_type) {
        phy_ctrl = e1000_read_reg(hw, E1000_PHY_CTRL);
    } else {
        em_read_phy_reg(hw, IGP02E1000_PHY_POWER_MGMT, &mut phy_data)?;
    }

    if !active {
        if is_ich8(hw.mac_type) {
            phy_ctrl &= !E1000_PHY_CTRL_D0A_LPLU;
            e1000_write_reg(hw, E1000_PHY_CTRL, phy_ctrl);
        } else {
            phy_data &= !IGP02E1000_PM_D0_LPLU;
            em_write_phy_reg(hw, IGP02E1000_PHY_POWER_MGMT, phy_data)?;
        }
        em_lplu_restore_smart_speed(hw)?;
    } else {
        if is_ich8(hw.mac_type) {
            phy_ctrl |= E1000_PHY_CTRL_D0A_LPLU;
            e1000_write_reg(hw, E1000_PHY_CTRL, phy_ctrl);
        } else {
            phy_data |= IGP02E1000_PM_D0_LPLU;
            em_write_phy_reg(hw, IGP02E1000_PHY_POWER_MGMT, phy_data)?;
        }

        // When LPLU is enabled we should disable SmartSpeed.
        em_lplu_disable_smart_speed(hw)?;
    }
    Ok(())
}

/// `em_set_lplu_state_pchlan`: sets the Low Power Link Up state according to `active`. For
/// PCH, if OEM write bit are disabled in the NVM, writing the LPLU bits in the MAC will not
/// set the phy speed. This function will manually set the LPLU bit and restart auto-neg as
/// hw would do. D3 and D0 LPLU will call the same function since it configures the same
/// bit.
pub fn em_set_lplu_state_pchlan(hw: &mut EmHw, active: bool) -> Result<(), i32> {
    let mut oem_reg: u16 = 0;

    em_read_phy_reg(hw, HV_OEM_BITS, &mut oem_reg)?;

    if active {
        oem_reg |= HV_OEM_BITS_LPLU;
    } else {
        oem_reg &= !HV_OEM_BITS_LPLU;
    }

    oem_reg |= HV_OEM_BITS_RESTART_AN;
    em_write_phy_reg(hw, HV_OEM_BITS, oem_reg)
}

/// Changes the VCO speed register to improve the Bit Error Rate performance of SERDES.
fn em_set_vco_speed(hw: &mut EmHw) -> Result<(), i32> {
    let mut default_page: u16 = 0;
    let mut phy_data: u16 = 0;

    match hw.mac_type {
        em_82545_rev_3 | em_82546_rev_3 => {}
        _ => return Ok(()),
    }

    // Set PHY register 30, page 5, bit 8 to 0.
    em_read_phy_reg(hw, M88E1000_PHY_PAGE_SELECT, &mut default_page)?;

    em_write_phy_reg(hw, M88E1000_PHY_PAGE_SELECT, 0x0005)?;

    em_read_phy_reg(hw, M88E1000_PHY_GEN_CONTROL, &mut phy_data)?;

    phy_data &= !M88E1000_PHY_VCO_REG_BIT8;
    em_write_phy_reg(hw, M88E1000_PHY_GEN_CONTROL, phy_data)?;

    // Set PHY register 30, page 4, bit 11 to 1.
    em_write_phy_reg(hw, M88E1000_PHY_PAGE_SELECT, 0x0004)?;

    em_read_phy_reg(hw, M88E1000_PHY_GEN_CONTROL, &mut phy_data)?;

    phy_data |= M88E1000_PHY_VCO_REG_BIT11;
    em_write_phy_reg(hw, M88E1000_PHY_GEN_CONTROL, phy_data)?;

    em_write_phy_reg(hw, M88E1000_PHY_PAGE_SELECT, default_page)?;

    Ok(())
}

/// Reads the DHCP cookie from the ARC ram (the host interface memory) into `buffer`.
fn em_host_if_read_cookie(hw: &EmHw, buffer: &mut [u8; E1000_MNG_DHCP_COOKIE_LENGTH as usize]) {
    let offset = E1000_MNG_DHCP_COOKIE_OFFSET >> 2;
    let length = E1000_MNG_DHCP_COOKIE_LENGTH >> 2;

    for i in 0..length {
        let d = e1000_read_reg_array(hw, E1000_HOST_IF, offset + i);
        let at = (i * 4) as usize;
        buffer[at..at + 4].copy_from_slice(&d.to_le_bytes());
    }
}

/// Checks whether the HOST IF is enabled for command operation and also whether the
/// previous command is completed. It busy waits in case of previous command is not
/// completed.
fn em_mng_enable_host_if(hw: &EmHw) -> Result<(), i32> {
    // Check that the host interface is enabled.
    let mut hicr = e1000_read_reg(hw, E1000_HICR);
    if hicr & E1000_HICR_EN == 0 {
        // E1000_HOST_EN bit disabled.
        return Err(-E1000_ERR_HOST_INTERFACE_COMMAND);
    }
    // Check the previous command is completed.
    let mut i = 0;
    while i < E1000_MNG_DHCP_COMMAND_TIMEOUT {
        hicr = e1000_read_reg(hw, E1000_HICR);
        if hicr & E1000_HICR_C == 0 {
            break;
        }
        msec_delay_irq(1);
        i += 1;
    }

    if i == E1000_MNG_DHCP_COMMAND_TIMEOUT {
        // Previous command timeout failed.
        return Err(-E1000_ERR_HOST_INTERFACE_COMMAND);
    }
    Ok(())
}

/// `em_check_mng_mode`: whether the firmware's mode is IAMT.
pub fn em_check_mng_mode(hw: &EmHw) -> bool {
    let fwsm = e1000_read_reg(hw, E1000_FWSM);

    if is_ich8(hw.mac_type) {
        (fwsm & E1000_FWSM_MODE_MASK) == (E1000_MNG_ICH_IAMT_MODE << E1000_FWSM_MODE_SHIFT)
    } else {
        (fwsm & E1000_FWSM_MODE_MASK) == (E1000_MNG_IAMT_MODE << E1000_FWSM_MODE_SHIFT)
    }
}

/// Calculates the checksum of `buffer` (the C's NULL buffer gives 0; a slice is never
/// NULL).
fn em_calculate_mng_checksum(buffer: &[u8]) -> u8 {
    let mut sum: u8 = 0;

    for &b in buffer {
        sum = sum.wrapping_add(b);
    }

    0u8.wrapping_sub(sum)
}

/// `em_enable_tx_pkt_filtering`: whether tx packet filtering needs to be enabled (called in
/// init as well as watchdog timer functions); records it in `hw.tx_pkt_filtering`.
pub fn em_enable_tx_pkt_filtering(hw: &mut EmHw) -> bool {
    let mut tx_filter = false;
    if em_check_mng_mode(hw) && em_mng_enable_host_if(hw).is_ok() {
        let mut buffer = [0u8; E1000_MNG_DHCP_COOKIE_LENGTH as usize];
        em_host_if_read_cookie(hw, &mut buffer);
        hw.mng_cookie = EmHostMngDhcpCookie::from_bytes(&buffer);
        let checksum = hw.mng_cookie.checksum;
        hw.mng_cookie.checksum = 0;
        let bytes = hw.mng_cookie.to_bytes();
        if hw.mng_cookie.signature == E1000_IAMT_SIGNATURE
            && checksum == em_calculate_mng_checksum(&bytes)
        {
            if u32::from(hw.mng_cookie.status) & E1000_MNG_DHCP_COOKIE_STATUS_PARSING_SUPPORT != 0 {
                tx_filter = true;
            }
        } else {
            tx_filter = true;
        }
    }
    hw.tx_pkt_filtering = tx_filter;
    tx_filter
}

/// Polarity reversal workaround for forced 10F/10H links.
fn em_polarity_reversal_workaround(hw: &mut EmHw) -> Result<(), i32> {
    let mut mii_status_reg: u16 = 0;

    // Disable the transmitter on the PHY.
    em_write_phy_reg(hw, M88E1000_PHY_PAGE_SELECT, 0x0019)?;
    em_write_phy_reg(hw, M88E1000_PHY_GEN_CONTROL, 0xFFFF)?;

    em_write_phy_reg(hw, M88E1000_PHY_PAGE_SELECT, 0x0000)?;

    // This loop will early-out if the NO link condition has been met.
    for _ in 0..PHY_FORCE_TIME {
        // Read the MII Status Register and wait for Link Status bit to be clear.
        em_read_phy_reg(hw, PHY_STATUS, &mut mii_status_reg)?;
        em_read_phy_reg(hw, PHY_STATUS, &mut mii_status_reg)?;

        if (mii_status_reg & !MII_SR_LINK_STATUS) == 0 {
            break;
        }
        msec_delay_irq(100);
    }

    // Recommended delay time after link has been lost.
    msec_delay_irq(1000);

    // Now we will re-enable the transmitter on the PHY.
    em_write_phy_reg(hw, M88E1000_PHY_PAGE_SELECT, 0x0019)?;
    msec_delay_irq(50);
    em_write_phy_reg(hw, M88E1000_PHY_GEN_CONTROL, 0xFFF0)?;
    msec_delay_irq(50);
    em_write_phy_reg(hw, M88E1000_PHY_GEN_CONTROL, 0xFF00)?;
    msec_delay_irq(50);
    em_write_phy_reg(hw, M88E1000_PHY_GEN_CONTROL, 0x0000)?;

    em_write_phy_reg(hw, M88E1000_PHY_PAGE_SELECT, 0x0000)?;

    // This loop will early-out if the link condition has been met.
    for _ in 0..PHY_FORCE_TIME {
        // Read the MII Status Register and wait for Link Status bit to be set.
        em_read_phy_reg(hw, PHY_STATUS, &mut mii_status_reg)?;
        em_read_phy_reg(hw, PHY_STATUS, &mut mii_status_reg)?;

        if mii_status_reg & MII_SR_LINK_STATUS != 0 {
            break;
        }
        msec_delay_irq(100);
    }
    Ok(())
}

/// Disables PCI-Express master access.
fn em_set_pci_express_master_disable(hw: &EmHw) {
    if hw.bus_type != em_bus_type_pci_express {
        return;
    }

    let mut ctrl = e1000_read_reg(hw, E1000_CTRL);
    ctrl |= E1000_CTRL_GIO_MASTER_DISABLE;
    e1000_write_reg(hw, E1000_CTRL, ctrl);
}

/// `em_disable_pciex_master`: disables PCI-Express master access and verifies there are no
/// pending requests (`-E1000_ERR_MASTER_REQUESTS_PENDING` if the master disable bit hasn't
/// caused the master requests to be disabled).
pub fn em_disable_pciex_master(hw: &EmHw) -> Result<(), i32> {
    let mut timeout = MASTER_DISABLE_TIMEOUT; // 80ms

    if hw.bus_type != em_bus_type_pci_express {
        return Ok(());
    }

    em_set_pci_express_master_disable(hw);

    while timeout != 0 {
        if e1000_read_reg(hw, E1000_STATUS) & E1000_STATUS_GIO_MASTER_ENABLE == 0 {
            break;
        } else {
            usec_delay(100);
        }
        timeout -= 1;
    }

    if timeout == 0 {
        // Master requests are pending.
        return Err(-E1000_ERR_MASTER_REQUESTS_PENDING);
    }
    Ok(())
}

/// Checks for the EEPROM Auto Read bit done (`-E1000_ERR_RESET` if it never comes).
fn em_get_auto_rd_done(hw: &EmHw) -> Result<(), i32> {
    let mut timeout = AUTO_READ_DONE_TIMEOUT;

    match hw.mac_type {
        em_82571 | em_82572 | em_82573 | em_82574 | em_82575 | em_82576 | em_82580
        | em_80003es2lan | em_i210 | em_i350 | em_ich8lan | em_ich9lan | em_ich10lan
        | em_pchlan | em_pch2lan | em_pch_lpt | em_pch_spt | em_pch_cnp | em_pch_tgp
        | em_pch_adp | em_pch_mtp | em_pch_ptp => {
            while timeout != 0 {
                if e1000_read_reg(hw, E1000_EECD) & E1000_EECD_AUTO_RD != 0 {
                    break;
                } else {
                    msec_delay(1);
                }
                timeout -= 1;
            }

            if timeout == 0 {
                // Auto read by HW from EEPROM has not completed.
                return Err(-E1000_ERR_RESET);
            }
        }
        _ => msec_delay(5),
    }
    // PHY configuration from NVM just starts after EECD_AUTO_RD sets to high. Need to wait
    // for PHY configuration completion before accessing NVM and PHY.
    if hw.mac_type == em_82573 || hw.mac_type == em_82574 {
        msec_delay(25);
    }

    Ok(())
}

/// Checks if the PHY configuration is done (a timeout is only a debug message).
fn em_get_phy_cfg_done(hw: &EmHw) -> Result<(), i32> {
    let mut timeout = PHY_CFG_TIMEOUT;
    let mut cfg_mask = E1000_NVM_CFG_DONE_PORT_0;

    match hw.mac_type {
        em_80003es2lan | em_82575 | em_82576 | em_82580 | em_i350 | em_82571 | em_82572 => {
            // The 82571/82572 case is the C's fallthrough target, with port 0's mask.
            if !matches!(hw.mac_type, em_82571 | em_82572) {
                match hw.bus_func {
                    1 => cfg_mask = E1000_NVM_CFG_DONE_PORT_1,
                    2 => cfg_mask = E1000_NVM_CFG_DONE_PORT_2,
                    3 => cfg_mask = E1000_NVM_CFG_DONE_PORT_3,
                    _ => {}
                }
            }
            while timeout != 0 {
                if e1000_read_reg(hw, E1000_EEMNGCTL) & cfg_mask != 0 {
                    break;
                } else {
                    msec_delay(1);
                }
                timeout -= 1;
            }
            // A timeout ("MNG configuration cycle has not completed") is not an error.
        }
        _ => msec_delay_irq(10),
    }

    Ok(())
}

/// Takes the hardware EEPROM semaphore: the SMBI (on the 80003ES2LAN) and SWESMBI semaphore
/// bits, used when resetting the adapter or accessing the EEPROM (`-E1000_ERR_EEPROM` if
/// they cannot be had).
fn em_get_hw_eeprom_semaphore(hw: &mut EmHw) -> Result<(), i32> {
    if hw.eeprom_semaphore_present == 0 {
        return Ok(());
    }

    if hw.mac_type == em_80003es2lan {
        // Get the SW semaphore.
        if em_get_software_semaphore(hw).is_err() {
            return Err(-E1000_ERR_EEPROM);
        }
    }
    // Get the FW semaphore.
    let mut timeout = i32::from(hw.eeprom.word_size) + 1;
    while timeout != 0 {
        let mut swsm = e1000_read_reg(hw, E1000_SWSM);
        swsm |= E1000_SWSM_SWESMBI;
        e1000_write_reg(hw, E1000_SWSM, swsm);
        // If we managed to set the bit we got the semaphore.
        swsm = e1000_read_reg(hw, E1000_SWSM);
        if swsm & E1000_SWSM_SWESMBI != 0 {
            break;
        }

        usec_delay(50);
        timeout -= 1;
    }

    if timeout == 0 {
        // Release semaphores.
        em_put_hw_eeprom_semaphore(hw);
        // Driver can't access the Eeprom - SWESMBI bit is set.
        return Err(-E1000_ERR_EEPROM);
    }
    Ok(())
}

/// Clears the HW semaphore bits.
fn em_put_hw_eeprom_semaphore(hw: &EmHw) {
    if hw.eeprom_semaphore_present == 0 {
        return;
    }

    let mut swsm = e1000_read_reg(hw, E1000_SWSM);
    if hw.mac_type == em_80003es2lan {
        // Release both semaphores.
        swsm &= !(E1000_SWSM_SMBI | E1000_SWSM_SWESMBI);
    } else {
        swsm &= !E1000_SWSM_SWESMBI;
    }
    e1000_write_reg(hw, E1000_SWSM, swsm);
}

/// Obtains the software semaphore bit (SMBI) before resetting the PHY (`-E1000_ERR_RESET`
/// if it cannot be had).
fn em_get_software_semaphore(hw: &EmHw) -> Result<(), i32> {
    let mut timeout = i32::from(hw.eeprom.word_size) + 1;

    if hw.mac_type != em_80003es2lan {
        return Ok(());
    }

    while timeout != 0 {
        let swsm = e1000_read_reg(hw, E1000_SWSM);
        // If SMBI bit cleared, it is now set and we hold the semaphore.
        if swsm & E1000_SWSM_SMBI == 0 {
            break;
        }
        msec_delay_irq(1);
        timeout -= 1;
    }

    if timeout == 0 {
        // Driver can't access device - SMBI bit is set.
        return Err(-E1000_ERR_RESET);
    }
    Ok(())
}

/// Releases the semaphore bit (SMBI).
fn em_release_software_semaphore(hw: &EmHw) {
    if hw.mac_type != em_80003es2lan {
        return;
    }

    let mut swsm = e1000_read_reg(hw, E1000_SWSM);
    // Release the SW semaphores.
    swsm &= !E1000_SWSM_SMBI;
    e1000_write_reg(hw, E1000_SWSM, swsm);
}

/// `em_check_phy_reset_block`: checks if PHY reset is blocked due to SOL/IDER session, for
/// example. `Err(E1000_BLK_PHY_RESET)` isn't necessarily an error; it's up to the caller to
/// figure out how to deal with it.
pub fn em_check_phy_reset_block(hw: &EmHw) -> Result<(), i32> {
    let mut manc: u32 = 0;

    if is_ich8(hw.mac_type) {
        let mut i = 0;
        let mut blocked;
        loop {
            let fwsm = e1000_read_reg(hw, E1000_FWSM);
            if fwsm & E1000_FWSM_RSPCIPHY == 0 {
                blocked = true;
                msec_delay(10);
            } else {
                blocked = false;
            }
            // The C's `while (blocked && (i++ < 30))`.
            if !blocked {
                break;
            }
            let at = i;
            i += 1;
            if at >= 30 {
                break;
            }
        }
        return if blocked {
            Err(E1000_BLK_PHY_RESET)
        } else {
            Ok(())
        };
    }
    if hw.mac_type > em_82547_rev_2 {
        manc = e1000_read_reg(hw, E1000_MANC);
    }
    if manc & E1000_MANC_BLK_PHY_RST_ON_IDE != 0 {
        Err(E1000_BLK_PHY_RESET)
    } else {
        Ok(())
    }
}

/// Configures PCI-Ex no-snoop; `no_snoop` is the bitmap of no-snoop events.
fn em_set_pci_ex_no_snoop(hw: &mut EmHw, no_snoop: u32) -> Result<(), i32> {
    if hw.bus_type == em_bus_type_unknown {
        em_get_bus_info(hw);
    }

    if hw.bus_type != em_bus_type_pci_express {
        return Ok(());
    }

    if no_snoop != 0 {
        let mut gcr_reg = e1000_read_reg(hw, E1000_GCR);
        gcr_reg &= !PCI_EX_NO_SNOOP_ALL;
        gcr_reg |= no_snoop;
        e1000_write_reg(hw, E1000_GCR, gcr_reg);
    }
    if is_ich8(hw.mac_type) {
        let mut ctrl_ext = e1000_read_reg(hw, E1000_CTRL_EXT);
        ctrl_ext |= E1000_CTRL_EXT_RO_DIS;
        e1000_write_reg(hw, E1000_CTRL_EXT, ctrl_ext);
    }
    Ok(())
}

/// Gets the software semaphore FLAG bit (SWFLAG), which synchronizes the access to all
/// shared resources between SW, FW and HW; nested acquisitions are counted in `sw_flag`.
fn em_get_software_flag(hw: &mut EmHw) -> Result<(), i32> {
    let mut timeout = PHY_CFG_TIMEOUT;

    if is_ich8(hw.mac_type) {
        if hw.sw_flag != 0 {
            hw.sw_flag += 1;
            return Ok(());
        }
        let mut extcnf_ctrl: u32 = 0;
        while timeout != 0 {
            extcnf_ctrl = e1000_read_reg(hw, E1000_EXTCNF_CTRL);
            if extcnf_ctrl & E1000_EXTCNF_CTRL_SWFLAG == 0 {
                break;
            }
            msec_delay_irq(1);
            timeout -= 1;
        }
        if timeout == 0 {
            printf(format_args!(
                "{}: SW has already locked the resource?\n",
                "em_get_software_flag"
            ));
            return Err(-E1000_ERR_CONFIG);
        }
        timeout = SW_FLAG_TIMEOUT;
        extcnf_ctrl |= E1000_EXTCNF_CTRL_SWFLAG;
        e1000_write_reg(hw, E1000_EXTCNF_CTRL, extcnf_ctrl);

        while timeout != 0 {
            extcnf_ctrl = e1000_read_reg(hw, E1000_EXTCNF_CTRL);
            if extcnf_ctrl & E1000_EXTCNF_CTRL_SWFLAG != 0 {
                break;
            }
            msec_delay_irq(1);
            timeout -= 1;
        }

        if timeout == 0 {
            printf(format_args!(
                "Failed to acquire the semaphore, FW or HW has it: FWSM=0x{:08x} \
                 EXTCNF_CTRL=0x{:08x})\n",
                e1000_read_reg(hw, E1000_FWSM),
                extcnf_ctrl
            ));
            extcnf_ctrl &= !E1000_EXTCNF_CTRL_SWFLAG;
            e1000_write_reg(hw, E1000_EXTCNF_CTRL, extcnf_ctrl);
            return Err(-E1000_ERR_CONFIG);
        }
    }
    hw.sw_flag += 1;
    Ok(())
}

/// Releases the software semaphore FLAG bit (SWFLAG) when the last nested holder lets go.
fn em_release_software_flag(hw: &mut EmHw) {
    if is_ich8(hw.mac_type) {
        crate::kassert!(hw.sw_flag > 0);
        hw.sw_flag -= 1;
        if hw.sw_flag > 0 {
            return;
        }
        let mut extcnf_ctrl = e1000_read_reg(hw, E1000_EXTCNF_CTRL);
        extcnf_ctrl &= !E1000_EXTCNF_CTRL_SWFLAG;
        e1000_write_reg(hw, E1000_EXTCNF_CTRL, extcnf_ctrl);
    }
}

/// `em_valid_nvm_bank_detect_ich8lan`: finds out the valid NVM bank (0 or 1) from the
/// signature byte read through the flash access registers (word 0x13 bits 15:14 = 10b
/// indicate a valid signature for that bank); the C's bare `-1` when none is valid.
pub fn em_valid_nvm_bank_detect_ich8lan(hw: &mut EmHw, bank: &mut u32) -> Result<(), i32> {
    let mut bank1_offset = hw.flash_bank_size * core::mem::size_of::<u16>() as u32;
    let mut act_offset = E1000_ICH_NVM_SIG_WORD * 2 + 1;
    let mut nvm_dword: u32 = 0;
    let mut sig_byte: u8 = 0;
    let sig_mask = E1000_ICH_NVM_VALID_SIG_MASK as u8;
    let sig_value = E1000_ICH_NVM_SIG_VALUE as u8;

    match hw.mac_type {
        em_pch_spt | em_pch_cnp | em_pch_tgp | em_pch_adp | em_pch_mtp | em_pch_ptp => {
            bank1_offset = hw.flash_bank_size * 2;
            act_offset = E1000_ICH_NVM_SIG_WORD * 2;

            // Set bank to 0 in case flash read fails.
            *bank = 0;

            // Check bank 0.
            em_read_ich8_dword(hw, act_offset, &mut nvm_dword)?;
            sig_byte = ((nvm_dword & 0xFF00) >> 8) as u8;
            if sig_byte & sig_mask == sig_value {
                *bank = 0;
                return Ok(());
            }

            // Check bank 1.
            em_read_ich8_dword(hw, act_offset + bank1_offset, &mut nvm_dword)?;
            sig_byte = ((nvm_dword & 0xFF00) >> 8) as u8;
            if sig_byte & sig_mask == sig_value {
                *bank = 1;
                return Ok(());
            }

            // ERROR: No valid NVM bank present.
            return Err(-1);
        }
        em_ich8lan | em_ich9lan => {
            let eecd = e1000_read_reg(hw, E1000_EECD);
            if (eecd & E1000_EECD_SEC1VAL_VALID_MASK) == E1000_EECD_SEC1VAL_VALID_MASK {
                if eecd & E1000_EECD_SEC1VAL != 0 {
                    *bank = 1;
                } else {
                    *bank = 0;
                }

                return Ok(());
            }
            // Unable to determine valid NVM bank via EEC - reading flash signature (falls
            // through).
        }
        _ => {}
    }
    // Set bank to 0 in case flash read fails.
    *bank = 0;

    // Check bank 0.
    em_read_ich8_byte(hw, act_offset, &mut sig_byte)?;
    if sig_byte & sig_mask == sig_value {
        *bank = 0;
        return Ok(());
    }

    // Check bank 1.
    em_read_ich8_byte(hw, act_offset + bank1_offset, &mut sig_byte)?;
    if sig_byte & sig_mask == sig_value {
        *bank = 1;
        return Ok(());
    }

    // ERROR: No valid NVM bank present.
    Err(-1)
}

/// Reads 16-bit words from the NVM of an SPT or later PCH, which the flash controller
/// reads 32 bits at a time; words modified in the shadow RAM come from there.
fn em_read_eeprom_spt(hw: &mut EmHw, offset: u16, words: u16, data: &mut [u16]) -> Result<(), i32> {
    let mut flash_bank: u32 = 0;
    let mut dword: u32 = 0;

    // We need to know which is the valid flash bank. In the event that we didn't allocate
    // eeprom_shadow_ram, we may not be managing flash_bank. So it cannot be trusted and
    // needs to be updated with each read.
    if hw.mac_type < em_pch_spt {
        return Err(-E1000_ERR_EEPROM);
    }

    em_get_software_flag(hw)?;

    if em_valid_nvm_bank_detect_ich8lan(hw, &mut flash_bank).is_err() {
        // Could not detect valid bank, assuming bank 0.
        flash_bank = 0;
    }

    // Adjust offset appropriately if we're on bank 1 - adjust for word size.
    let bank_offset = flash_bank * (hw.flash_bank_size * 2);

    let mut error = Ok(());
    let offset = u32::from(offset);
    let words = u32::from(words);
    let mut i: u32 = 0;
    while i < words {
        let at = offset + i;
        let odd = !at.is_multiple_of(2);
        let add = if odd { 1 } else { 2 };
        if odd {
            let w = em_shadow_word(hw, at);
            if hw.eeprom_shadow_ram.is_some() && w.modified {
                data[i as usize] = w.eeprom_word;
                i += add;
                continue;
            }
        } else {
            let w0 = em_shadow_word(hw, at);
            let w1 = em_shadow_word(hw, at + 1);
            if hw.eeprom_shadow_ram.is_some() && w0.modified && w1.modified {
                data[i as usize] = w0.eeprom_word;
                data[i as usize + 1] = w1.eeprom_word;
                i += add;
                continue;
            }
        }
        let act_offset = if odd {
            bank_offset + (at - 1) * 2
        } else {
            bank_offset + at * 2
        };
        error = em_read_ich8_dword(hw, act_offset, &mut dword);
        if error.is_err() {
            break;
        }
        let w0 = em_shadow_word(hw, at);
        if hw.eeprom_shadow_ram.is_some() && w0.modified {
            data[i as usize] = w0.eeprom_word;
        } else if add == 1 {
            data[i as usize] = (dword >> 16) as u16;
        } else {
            data[i as usize] = (dword & 0xFFFF) as u16;
        }
        if add == 1 || words - i == 1 {
            i += add;
            continue;
        }
        let w1 = em_shadow_word(hw, at + 1);
        if hw.eeprom_shadow_ram.is_some() && w1.modified {
            data[i as usize + 1] = w1.eeprom_word;
        } else {
            data[i as usize + 1] = (dword >> 16) as u16;
        }
        i += add;
    }

    em_release_software_flag(hw);

    error
}

/// Reads 16-bit words from the EEPROM using the ICH8's flash access registers; words
/// modified in the shadow RAM come from there.
fn em_read_eeprom_ich8(
    hw: &mut EmHw,
    offset: u16,
    words: u16,
    data: &mut [u16],
) -> Result<(), i32> {
    let mut flash_bank: u32 = 0;
    let mut word: u16 = 0;
    // We need to know which is the valid flash bank. In the event that we didn't allocate
    // eeprom_shadow_ram, we may not be managing flash_bank. So it cannot be trusted and
    // needs to be updated with each read.
    if hw.mac_type >= em_pch_spt {
        return em_read_eeprom_spt(hw, offset, words, data);
    }

    em_get_software_flag(hw)?;

    if em_valid_nvm_bank_detect_ich8lan(hw, &mut flash_bank).is_err() {
        // Could not detect valid bank, assuming bank 0.
        flash_bank = 0;
    }

    // Adjust offset appropriately if we're on bank 1 - adjust for word size.
    let bank_offset = flash_bank * (hw.flash_bank_size * 2);

    let mut error = Ok(());
    for i in 0..u32::from(words) {
        let at = u32::from(offset) + i;
        let w = em_shadow_word(hw, at);
        if hw.eeprom_shadow_ram.is_some() && w.modified {
            data[i as usize] = w.eeprom_word;
        } else {
            // The NVM part needs a byte offset, hence * 2.
            let act_offset = bank_offset + at * 2;
            error = em_read_ich8_word(hw, act_offset, &mut word);
            if error.is_err() {
                break;
            }
            data[i as usize] = word;
        }
    }

    em_release_software_flag(hw);

    error
}

/// Writes 16-bit words to the EEPROM using the ICH8's flash access register. Actually,
/// writes are written to the shadow ram cache in the hw structure (`eeprom_shadow_ram`);
/// `em_commit_shadow_ram` flushes it to the NVM, which occurs when the NVM checksum is
/// updated. A word past the end of the allocation is refused as one past
/// `E1000_SHADOW_RAM_WORDS` is.
fn em_write_eeprom_ich8(hw: &mut EmHw, offset: u16, words: u16, data: &[u16]) -> Result<(), i32> {
    em_get_software_flag(hw)?;
    // A driver can write to the NVM only if it has eeprom_shadow_ram allocated. Subsequent
    // reads to the modified words are read from this cached structure as well. Writes will
    // only go into this cached structure unless it's followed by a call to
    // em_update_eeprom_checksum() where it will commit the changes and clear the "modified"
    // field.
    let error = if let Some(ram) = hw.eeprom_shadow_ram.as_deref_mut() {
        let mut error = Ok(());
        for (i, d) in data.iter().take(usize::from(words)).enumerate() {
            let at = usize::from(offset) + i;
            match ram.get_mut(at) {
                Some(w) if at < E1000_SHADOW_RAM_WORDS as usize => {
                    w.modified = true;
                    w.eeprom_word = *d;
                }
                _ => {
                    error = Err(-E1000_ERR_EEPROM);
                    break;
                }
            }
        }
        error
    } else {
        // Drivers have the option to not allocate eeprom_shadow_ram as long as they don't
        // perform any NVM writes. An attempt in doing so will result in this error.
        Err(-E1000_ERR_EEPROM)
    };

    em_release_software_flag(hw);

    error
}

/// Reads the ICH flash status register (HSFSTS), 32 bits wide from SPT on.
fn em_ich8_read_hsfsts(hw: &EmHw) -> Ich8HwsFlashStatus {
    let regval = if hw.mac_type >= em_pch_spt {
        (e1000_read_ich_flash_reg32(hw, ICH_FLASH_HSFSTS) & 0xFFFF) as u16
    } else {
        e1000_read_ich_flash_reg16(hw, ICH_FLASH_HSFSTS)
    };
    Ich8HwsFlashStatus { regval }
}

/// Writes the ICH flash status register (HSFSTS), 32 bits wide from SPT on.
fn em_ich8_write_hsfsts(hw: &EmHw, hsfsts: Ich8HwsFlashStatus) {
    if hw.mac_type >= em_pch_spt {
        e1000_write_ich_flash_reg32(hw, ICH_FLASH_HSFSTS, u32::from(hsfsts.regval));
    } else {
        e1000_write_ich_flash_reg16(hw, ICH_FLASH_HSFSTS, hsfsts.regval);
    }
}

/// Does the initial flash setup so that a new read/write/erase cycle can be started (the
/// C's positive `E1000_ERR_EEPROM` when it cannot).
fn em_ich8_cycle_init(hw: &EmHw) -> Result<(), i32> {
    let mut error = Err(E1000_ERR_EEPROM);

    let mut hsfsts = em_ich8_read_hsfsts(hw);

    // May be check the Flash Des Valid bit in Hw status.
    if hsfsts.fldesvalid() == 0 {
        // Flash descriptor invalid. SW Sequencing must be used.
        return error;
    }
    // Clear FCERR in Hw status by writing 1. Clear DAEL in Hw status by writing a 1.
    hsfsts.set_flcerr(1);
    hsfsts.set_dael(1);
    em_ich8_write_hsfsts(hw, hsfsts);
    // Either we should have a hardware SPI cycle in progress bit to check against, in order
    // to start a new cycle or FDONE bit should be changed in the hardware so that it is 1
    // after hardware reset, which can then be used as an indication whether a cycle is in
    // progress or has been completed .. we should also have some software semaphore
    // mechanism to guard FDONE or the cycle in progress bit so that two threads access to
    // those bits can be sequentiallized or a way so that 2 threads dont start the cycle at
    // the same time.
    if hsfsts.flcinprog() == 0 {
        // There is no cycle running at present, so we can start a cycle. Begin by setting
        // Flash Cycle Done.
        hsfsts.set_flcdone(1);
        em_ich8_write_hsfsts(hw, hsfsts);
        error = Ok(());
    } else {
        // Otherwise poll for sometime so the current cycle has a chance to end before
        // giving up.
        for _ in 0..ICH_FLASH_COMMAND_TIMEOUT {
            hsfsts = em_ich8_read_hsfsts(hw);
            if hsfsts.flcinprog() == 0 {
                error = Ok(());
                break;
            }
            usec_delay(1);
        }
        if error.is_ok() {
            // Successful in waiting for previous cycle to timeout, now set the Flash Cycle
            // Done.
            hsfsts.set_flcdone(1);
            em_ich8_write_hsfsts(hw, hsfsts);
        }
        // Otherwise the flash controller is busy, cannot get access.
    }
    error
}

/// Starts a flash cycle and waits up to `timeout` microseconds for its completion.
fn em_ich8_flash_cycle(hw: &EmHw, timeout: u32) -> Result<(), i32> {
    // Start a cycle by writing 1 in Flash Cycle Go in Hw Flash Control.
    let mut hsflctl = Ich8HwsFlashCtrl {
        regval: if hw.mac_type >= em_pch_spt {
            (e1000_read_ich_flash_reg32(hw, ICH_FLASH_HSFSTS) >> 16) as u16
        } else {
            e1000_read_ich_flash_reg16(hw, ICH_FLASH_HSFCTL)
        },
    };
    hsflctl.set_flcgo(1);

    if hw.mac_type >= em_pch_spt {
        e1000_write_ich_flash_reg32(hw, ICH_FLASH_HSFSTS, u32::from(hsflctl.regval) << 16);
    } else {
        e1000_write_ich_flash_reg16(hw, ICH_FLASH_HSFCTL, hsflctl.regval);
    }

    // Wait till FDONE bit is set to 1.
    let mut i: u32 = 0;
    let mut hsfsts;
    loop {
        hsfsts = em_ich8_read_hsfsts(hw);
        if hsfsts.flcdone() == 1 {
            break;
        }
        usec_delay(1);
        i += 1;
        if i >= timeout {
            break;
        }
    }
    if hsfsts.flcdone() == 1 && hsfsts.flcerr() == 0 {
        Ok(())
    } else {
        Err(E1000_ERR_EEPROM)
    }
}

/// After a failed flash cycle: whether the whole sequence may be tried again (FCERR set),
/// read from the 16-bit status register as the C does on every MAC.
fn em_ich8_retry_after_error(hw: &EmHw) -> bool {
    let hsfsts = Ich8HwsFlashStatus {
        regval: e1000_read_ich_flash_reg16(hw, ICH_FLASH_HSFSTS),
    };
    // If we've gotten here, then things are probably completely hosed, but if the error
    // condition is detected, it won't hurt to give it another try...
    // ICH_FLASH_CYCLE_REPEAT_COUNT times. A cycle that did not complete is a timeout
    // error: give up.
    hsfsts.flcerr() == 1 || hsfsts.flcdone() != 0
}

/// Reads a byte (`size` 1) or word (`size` 2) at `index` of the NVM using the ICH8 flash
/// access registers.
fn em_read_ich8_data(hw: &EmHw, index: u32, size: u32, data: &mut u16) -> Result<(), i32> {
    let mut error = Err(-E1000_ERR_EEPROM);
    let mut count = 0;

    if !(1..=2).contains(&size) || index > ICH_FLASH_LINEAR_ADDR_MASK {
        return error;
    }

    let flash_linear_address = (ICH_FLASH_LINEAR_ADDR_MASK & index) + hw.flash_base_addr;

    loop {
        usec_delay(1);
        // Steps.
        error = em_ich8_cycle_init(hw);
        if error.is_err() {
            break;
        }

        let mut hsflctl = Ich8HwsFlashCtrl {
            regval: e1000_read_ich_flash_reg16(hw, ICH_FLASH_HSFCTL),
        };
        // 0b/1b corresponds to 1 or 2 byte size, respectively.
        hsflctl.set_fldbcount((size - 1) as u16);
        hsflctl.set_flcycle(ICH_CYCLE_READ as u16);
        e1000_write_ich_flash_reg16(hw, ICH_FLASH_HSFCTL, hsflctl.regval);
        // Write the last 24 bits of index into Flash Linear address field in Flash Address.
        e1000_write_ich_flash_reg32(hw, ICH_FLASH_FADDR, flash_linear_address);

        error = em_ich8_flash_cycle(hw, ICH_FLASH_COMMAND_TIMEOUT);
        // Check if FCERR is set to 1, if set to 1, clear it and try the whole sequence a few
        // more times, else read in (shift in) the Flash Data0, the order is least
        // significant byte first msb to lsb.
        if error.is_ok() {
            let flash_data = e1000_read_ich_flash_reg(hw, ICH_FLASH_FDATA0);
            if size == 1 {
                *data = (flash_data & 0x000000FF) as u16;
            } else {
                *data = (flash_data & 0x0000FFFF) as u16;
            }
            break;
        } else if !em_ich8_retry_after_error(hw) {
            break;
        }
        let c = count;
        count += 1;
        if c >= ICH_FLASH_CYCLE_REPEAT_COUNT {
            break;
        }
    }

    error
}

/// Reads a dword at `offset` of the NVM of an SPT or later PCH (32-bit flash accesses).
fn em_read_ich8_data32(hw: &EmHw, offset: u32, data: &mut u32) -> Result<(), i32> {
    let mut error = Err(-E1000_ERR_EEPROM);
    let mut count = 0;

    if hw.mac_type < em_pch_spt {
        return error;
    }
    if offset > ICH_FLASH_LINEAR_ADDR_MASK {
        return error;
    }
    let flash_linear_address = (ICH_FLASH_LINEAR_ADDR_MASK & offset) + hw.flash_base_addr;

    loop {
        usec_delay(1);
        // Steps.
        error = em_ich8_cycle_init(hw);
        if error.is_err() {
            break;
        }

        // 32 bit accesses in SPT.
        let mut hsflctl = Ich8HwsFlashCtrl {
            regval: (e1000_read_ich_flash_reg32(hw, ICH_FLASH_HSFSTS) >> 16) as u16,
        };

        hsflctl.set_fldbcount((core::mem::size_of::<u32>() - 1) as u16);
        hsflctl.set_flcycle(ICH_CYCLE_READ as u16);

        e1000_write_ich_flash_reg32(hw, ICH_FLASH_HSFSTS, u32::from(hsflctl.regval) << 16);
        // Write the last 24 bits of offset into Flash Linear address field in Flash
        // Address.
        e1000_write_ich_flash_reg32(hw, ICH_FLASH_FADDR, flash_linear_address);

        error = em_ich8_flash_cycle(hw, ICH_FLASH_COMMAND_TIMEOUT);
        // Check if FCERR is set to 1, if set to 1, clear it and try the whole sequence a few
        // more times, else read in (shift in) the Flash Data0, the order is least
        // significant byte first msb to lsb.
        if error.is_ok() {
            *data = e1000_read_ich_flash_reg32(hw, ICH_FLASH_FDATA0);
            break;
        } else if !em_ich8_retry_after_error(hw) {
            break;
        }
        let c = count;
        count += 1;
        if c >= ICH_FLASH_CYCLE_REPEAT_COUNT {
            break;
        }
    }

    error
}

/// Writes one (`size` 1) or two bytes at `index` of the NVM using the ICH8 flash access
/// registers.
fn em_write_ich8_data(hw: &EmHw, index: u32, size: u32, data: u16) -> Result<(), i32> {
    let mut error = Err(-E1000_ERR_EEPROM);
    let mut count = 0;

    if hw.mac_type >= em_pch_spt {
        return Err(-E1000_ERR_EEPROM);
    }
    if !(1..=2).contains(&size)
        || u32::from(data) > size * 0xff
        || index > ICH_FLASH_LINEAR_ADDR_MASK
    {
        return error;
    }

    let flash_linear_address = (ICH_FLASH_LINEAR_ADDR_MASK & index) + hw.flash_base_addr;

    loop {
        usec_delay(1);
        // Steps.
        error = em_ich8_cycle_init(hw);
        if error.is_err() {
            break;
        }

        let mut hsflctl = Ich8HwsFlashCtrl {
            regval: e1000_read_ich_flash_reg16(hw, ICH_FLASH_HSFCTL),
        };
        // 0b/1b corresponds to 1 or 2 byte size, respectively.
        hsflctl.set_fldbcount((size - 1) as u16);
        hsflctl.set_flcycle(ICH_CYCLE_WRITE as u16);
        e1000_write_ich_flash_reg16(hw, ICH_FLASH_HSFCTL, hsflctl.regval);
        // Write the last 24 bits of index into Flash Linear address field in Flash Address.
        e1000_write_ich_flash_reg32(hw, ICH_FLASH_FADDR, flash_linear_address);

        let flash_data = if size == 1 {
            u32::from(data) & 0x00FF
        } else {
            u32::from(data)
        };

        e1000_write_ich_flash_reg32(hw, ICH_FLASH_FDATA0, flash_data);
        // Check if FCERR is set to 1 , if set to 1, clear it and try the whole sequence a
        // few more times else done.
        error = em_ich8_flash_cycle(hw, ICH_FLASH_COMMAND_TIMEOUT);
        if error.is_ok() || !em_ich8_retry_after_error(hw) {
            break;
        }
        let c = count;
        count += 1;
        if c >= ICH_FLASH_CYCLE_REPEAT_COUNT {
            break;
        }
    }

    error
}

/// Reads a single byte at `index` of the NVM using the ICH8 flash access registers.
fn em_read_ich8_byte(hw: &EmHw, index: u32, data: &mut u8) -> Result<(), i32> {
    let mut word: u16 = 0;

    if hw.mac_type >= em_pch_spt {
        return Err(-E1000_ERR_EEPROM);
    }
    em_read_ich8_data(hw, index, 1, &mut word)?;
    *data = word as u8;
    Ok(())
}

/// Writes a single byte at `index` of the NVM using the ICH8 flash access registers;
/// performs verification by reading back the value and then going through a retry
/// algorithm before giving up (the C's positive `E1000_ERR_EEPROM` after 100 retries).
fn em_verify_write_ich8_byte(hw: &EmHw, index: u32, byte: u8) -> Result<(), i32> {
    let mut program_retries = 0;

    let mut error = em_write_ich8_byte(hw, index, byte);

    if error.is_err() {
        while program_retries < 100 {
            error = em_write_ich8_byte(hw, index, byte);
            usec_delay(100);
            if error.is_ok() {
                break;
            }
            program_retries += 1;
        }
    }
    if program_retries == 100 {
        error = Err(E1000_ERR_EEPROM);
    }

    error
}

/// Writes a single byte at `index` of the NVM using the ICH8 flash access registers.
fn em_write_ich8_byte(hw: &EmHw, index: u32, data: u8) -> Result<(), i32> {
    em_write_ich8_data(hw, index, 1, u16::from(data))
}

/// Reads a dword starting at byte `index` of the NVM using the ICH8 flash access registers.
fn em_read_ich8_dword(hw: &EmHw, index: u32, data: &mut u32) -> Result<(), i32> {
    em_read_ich8_data32(hw, index, data)
}

/// Reads a word starting at byte `index` of the NVM using the ICH8 flash access registers.
fn em_read_ich8_word(hw: &EmHw, index: u32, data: &mut u16) -> Result<(), i32> {
    em_read_ich8_data(hw, index, 2, data)
}

/// `em_erase_ich8_4k_segment`: erases `bank` (0 or 1) of the ICH NVM. Each bank may be a
/// 4, 8 or 64k block: the amount of NVM used in each bank is a *minimum* of 4 KBytes, so
/// this may actually erase as much as 8 or 64 KBytes. The C's positive `E1000_ERR_EEPROM`
/// on failure.
pub fn em_erase_ich8_4k_segment(hw: &EmHw, bank: u32) -> Result<(), i32> {
    let mut count = 0;
    let mut error = Err(E1000_ERR_EEPROM);
    let iteration: u32;
    let mut sub_sector_size: u32 = 0;
    let bank_size: u32;
    let mut error_flag = false;
    let hsfsts = Ich8HwsFlashStatus {
        regval: e1000_read_ich_flash_reg16(hw, ICH_FLASH_HSFSTS),
    };
    // Determine HW Sector size: Read BERASE bits of Hw flash Status register.
    //   00: The Hw sector is 256 bytes, hence we need to erase 16 consecutive sectors. The
    //       start index for the nth Hw sector can be calculated as bank * 4096 + n * 256
    //   01: The Hw sector is 4K bytes, hence we need to erase 1 sector. The start index for
    //       the nth Hw sector can be calculated as bank * 4096
    //   10: The HW sector is 8K bytes
    //   11: The Hw sector size is 64K bytes
    match hsfsts.berasesz() {
        0x0 => {
            // Hw sector size 256.
            sub_sector_size = ICH_FLASH_SEG_SIZE_256;
            bank_size = ICH_FLASH_SECTOR_SIZE;
            iteration = ICH_FLASH_SECTOR_SIZE / ICH_FLASH_SEG_SIZE_256;
        }
        0x1 => {
            bank_size = ICH_FLASH_SEG_SIZE_4K;
            iteration = 1;
        }
        0x2 => {
            if hw.mac_type == em_ich9lan {
                let gfpreg = e1000_read_ich_flash_reg(hw, ICH_FLASH_GFPREG);
                // sector_X_addr is a "sector"-aligned address (4096 bytes). Add 1 to
                // sector_end_addr since this sector is included in the overall size.
                let sector_base_addr = gfpreg & ICH_GFPREG_BASE_MASK;
                let sector_end_addr = ((gfpreg >> 16) & ICH_GFPREG_BASE_MASK) + 1;

                // Find total size of the NVM, then cut in half since the total size
                // represents two separate NVM banks.
                let mut bs =
                    sector_end_addr.wrapping_sub(sector_base_addr) << ICH_FLASH_SECT_ADDR_SHIFT;
                bs /= 2;
                // Word align.
                bs = (bs / 2) * 2;
                bank_size = bs;

                sub_sector_size = ICH_FLASH_SEG_SIZE_8K;
                iteration = bank_size / ICH_FLASH_SEG_SIZE_8K;
            } else {
                return error;
            }
        }
        0x3 => {
            bank_size = ICH_FLASH_SEG_SIZE_64K;
            iteration = 1;
        }
        _ => return error,
    }

    for j in 0..iteration {
        loop {
            count += 1;
            // Steps.
            error = em_ich8_cycle_init(hw);
            if error.is_err() {
                error_flag = true;
                break;
            }
            // Write a value 11 (block Erase) in Flash Cycle field in Hw flash Control.
            let mut hsflctl = Ich8HwsFlashCtrl {
                regval: e1000_read_ich_flash_reg16(hw, ICH_FLASH_HSFCTL),
            };
            hsflctl.set_flcycle(ICH_CYCLE_ERASE as u16);
            e1000_write_ich_flash_reg16(hw, ICH_FLASH_HSFCTL, hsflctl.regval);
            // Write the last 24 bits of an index within the block into Flash Linear address
            // field in Flash Address. This probably needs to be calculated here based off
            // the on-chip erase sector size and the software bank size (4, 8 or 64 KBytes).
            let mut flash_linear_address = bank
                .wrapping_mul(bank_size)
                .wrapping_add(j.wrapping_mul(sub_sector_size));
            flash_linear_address = flash_linear_address.wrapping_add(hw.flash_base_addr);
            flash_linear_address &= ICH_FLASH_LINEAR_ADDR_MASK;

            e1000_write_ich_flash_reg32(hw, ICH_FLASH_FADDR, flash_linear_address);

            error = em_ich8_flash_cycle(hw, ICH_FLASH_ERASE_TIMEOUT);
            // Check if FCERR is set to 1. If 1, clear it and try the whole sequence a few
            // more times else Done.
            if error.is_ok() {
                break;
            } else {
                let hsfsts = Ich8HwsFlashStatus {
                    regval: e1000_read_ich_flash_reg16(hw, ICH_FLASH_HSFSTS),
                };
                if hsfsts.flcerr() != 1 && hsfsts.flcdone() == 0 {
                    error_flag = true;
                    break;
                }
                // FCERR: repeat for some time before giving up.
            }
            if !(count < ICH_FLASH_CYCLE_REPEAT_COUNT && !error_flag) {
                break;
            }
        }
        if error_flag {
            break;
        }
    }
    if !error_flag {
        error = Ok(());
    }
    error
}

/// Reads a 16-bit word from the i210/i211 OTP (iNVM); `words` is not used, as in the C. A
/// word the OTP does not hold reads as its documented default.
fn em_read_invm_i210(hw: &EmHw, offset: u16, _words: u16, data: &mut [u16]) -> Result<(), i32> {
    let Some(data) = data.first_mut() else {
        return Err(-E1000_ERR_EEPROM);
    };

    let default = match offset {
        // Generate random MAC address if there's none.
        EEPROM_MAC_ADDR_WORD0 | EEPROM_MAC_ADDR_WORD1 | EEPROM_MAC_ADDR_WORD2 => 0xFFFF,
        EEPROM_INIT_CONTROL2_REG => NVM_INIT_CTRL_2_DEFAULT_I211,
        EEPROM_INIT_CONTROL4_REG => NVM_INIT_CTRL_4_DEFAULT_I211,
        EEPROM_LED_1_CFG => NVM_LED_1_CFG_DEFAULT_I211,
        EEPROM_LED_0_2_CFG => NVM_LED_0_2_CFG_DEFAULT_I211,
        EEPROM_ID_LED_SETTINGS => ID_LED_RESERVED_FFFF,
        _ => {
            // NVM word is not mapped.
            *data = NVM_RESERVED_WORD;
            return Ok(());
        }
    };
    if em_read_invm_word_i210(hw, offset, data).is_err() {
        *data = default;
    }

    Ok(())
}

/// Reads the 16-bit word at `address` from the OTP (`-E1000_NOT_IMPLEMENTED` when the word
/// is not stored in OTP).
fn em_read_invm_word_i210(hw: &EmHw, address: u16, data: &mut u16) -> Result<(), i32> {
    let mut i: u32 = 0;
    while i < INVM_SIZE {
        let invm_dword = em_read_reg(hw, e1000_invm_data_reg(i));
        // Get record type.
        let record_type = invm_dword_to_record_type(invm_dword);
        if record_type == INVM_UNINITIALIZED_STRUCTURE {
            break;
        }
        if record_type == INVM_CSR_AUTOLOAD_STRUCTURE {
            i += INVM_CSR_AUTOLOAD_DATA_SIZE_IN_DWORDS;
        }
        if record_type == INVM_RSA_KEY_SHA256_STRUCTURE {
            i += INVM_RSA_KEY_SHA256_DATA_SIZE_IN_DWORDS;
        }
        if record_type == INVM_WORD_AUTOLOAD_STRUCTURE {
            let word_address = invm_dword_to_word_address(invm_dword) as u8;
            if u16::from(word_address) == address {
                *data = invm_dword_to_word_data(invm_dword) as u16;
                return Ok(());
            }
        }
        i += 1;
    }

    Err(-E1000_NOT_IMPLEMENTED)
}

/// Writes the PHY registers of the NVM's extended configuration region (`cnf_size` dword
/// pairs of data and address at dword `cnf_base_addr`).
fn em_init_lcd_from_nvm_config_region(
    hw: &mut EmHw,
    cnf_base_addr: u32,
    cnf_size: u32,
) -> Result<(), i32> {
    let mut ret_val = Ok(());
    let mut reg_data = [0u16; 1];
    let mut reg_addr = [0u16; 1];
    // cnf_base_addr is in DWORD.
    let word_addr = (cnf_base_addr << 1) as u16;

    // cnf_size is returned in size of dwords.
    let mut i: u16 = 0;
    while u32::from(i) < cnf_size {
        em_read_eeprom(
            hw,
            word_addr.wrapping_add(i.wrapping_mul(2)),
            1,
            &mut reg_data,
        )?;

        em_read_eeprom(
            hw,
            word_addr.wrapping_add(i.wrapping_mul(2)).wrapping_add(1),
            1,
            &mut reg_addr,
        )?;

        em_get_software_flag(hw)?;

        ret_val = em_write_phy_reg_ex(hw, u32::from(reg_addr[0]), reg_data[0]);

        em_release_software_flag(hw);
        i = i.wrapping_add(1);
    }

    ret_val
}

/// Initializes the PHY from the NVM on ICH8 platforms. This is needed due to an issue where
/// the NVM configuration is not properly autoloaded after power transitions. Therefore,
/// after each PHY reset, we will load the configuration data out of the NVM manually.
fn em_init_lcd_from_nvm(hw: &mut EmHw) -> Result<(), i32> {
    if hw.phy_type != em_phy_igp_3 {
        return Ok(());
    }

    // Check if SW needs configure the PHY.
    let sw_cfg_mask = if hw.device_id == E1000_DEV_ID_ICH8_IGP_M_AMT
        || hw.device_id == E1000_DEV_ID_ICH8_IGP_M
        || hw.mac_type >= em_pchlan
    {
        FEXTNVM_SW_CONFIG_ICH8M
    } else {
        FEXTNVM_SW_CONFIG
    };

    let mut reg_data = e1000_read_reg(hw, E1000_FEXTNVM);
    if reg_data & sw_cfg_mask == 0 {
        return Ok(());
    }

    // Wait for basic configuration completes before proceeding.
    let mut loop_count = 0;
    loop {
        reg_data = e1000_read_reg(hw, E1000_STATUS) & E1000_STATUS_LAN_INIT_DONE;
        usec_delay(100);
        loop_count += 1;
        if reg_data != 0 || loop_count >= 50 {
            break;
        }
    }

    // Clear the Init Done bit for the next init event.
    reg_data = e1000_read_reg(hw, E1000_STATUS);
    reg_data &= !E1000_STATUS_LAN_INIT_DONE;
    e1000_write_reg(hw, E1000_STATUS, reg_data);
    // Make sure HW does not configure LCD from PHY extended configuration before SW
    // configuration.
    reg_data = e1000_read_reg(hw, E1000_EXTCNF_CTRL);
    if reg_data & E1000_EXTCNF_CTRL_LCD_WRITE_ENABLE == 0x0000 {
        reg_data = e1000_read_reg(hw, E1000_EXTCNF_SIZE);
        let mut cnf_size = reg_data & E1000_EXTCNF_SIZE_EXT_PCIE_LENGTH;
        cnf_size >>= 16;
        if cnf_size != 0 {
            reg_data = e1000_read_reg(hw, E1000_EXTCNF_CTRL);
            let mut cnf_base_addr = reg_data & E1000_EXTCNF_CTRL_EXT_CNF_POINTER;
            // cnf_base_addr is in DWORD.
            cnf_base_addr >>= 16;

            // Configure LCD from extended configuration region.
            em_init_lcd_from_nvm_config_region(hw, cnf_base_addr, cnf_size)?;
        }
    }
    Ok(())
}

/// `em_set_pciex_completion_timeout`: sets the PCIe completion timeout.
///
/// The defaults for 82575 and 82576 should be in the range of 50us to 50ms, however the
/// hardware default for these parts is 500us to 1ms which is less than the 10ms recommended
/// by the pci-e spec. To address this we need to increase the value to either 10ms to
/// 200ms for capability version 1 config, or 16ms to 55ms for version 2 (not done: the C's
/// version 2 code is `#if 0`, waiting for `em_*_pcie_cap_reg()`).
pub fn em_set_pciex_completion_timeout(hw: &EmHw) -> Result<(), i32> {
    let mut gcr = e1000_read_reg(hw, E1000_GCR);

    // Only take action if timeout value is not set by system BIOS. If capabilities version
    // is type 1 we can write the timeout of 10ms to 200ms through the GCR register.
    if gcr & E1000_GCR_CMPL_TMOUT_MASK == 0 && gcr & E1000_GCR_CAP_VER2 == 0 {
        gcr |= E1000_GCR_CMPL_TMOUT_10ms;
    }

    // Disable completion timeout resend.
    gcr &= !E1000_GCR_CMPL_TMOUT_RESEND;

    e1000_write_reg(hw, E1000_GCR, gcr);
    Ok(())
}

/// Sets the slow MDIO access mode.
fn em_set_mdio_slow_mode_hv(hw: &mut EmHw) -> Result<(), i32> {
    let mut data: u16 = 0;

    em_read_phy_reg(hw, HV_KMRN_MODE_CTRL, &mut data)?;

    data |= HV_KMRN_MDIO_SLOW;

    em_write_phy_reg(hw, HV_KMRN_MODE_CTRL, data)
}

/// `em_hv_phy_workarounds_ich8lan`: a series of PHY workarounds to be done after every PHY
/// reset (PCH).
pub fn em_hv_phy_workarounds_ich8lan(hw: &mut EmHw) -> Result<(), i32> {
    let mut phy_data: u16 = 0;

    if hw.mac_type != em_pchlan {
        return Ok(());
    }

    let swfw = E1000_SWFW_PHY0_SM;

    // Set MDIO slow mode before any other MDIO access.
    if hw.phy_type == em_phy_82577 || hw.phy_type == em_phy_82578 {
        em_set_mdio_slow_mode_hv(hw)?;
    }

    // Hanksville M Phy init for IEEE.
    if hw.revision_id == 2
        && hw.phy_type == em_phy_82577
        && (hw.phy_revision == 2 || hw.phy_revision == 3)
    {
        for (reg, val) in [
            (0x10, 0x8823),
            (0x11, 0x0018),
            (0x10, 0x8824),
            (0x11, 0x0016),
            (0x10, 0x8825),
            (0x11, 0x001A),
            (0x10, 0x888C),
            (0x11, 0x0007),
            (0x10, 0x888D),
            (0x11, 0x0007),
            (0x10, 0x888E),
            (0x11, 0x0007),
            (0x10, 0x8827),
            (0x11, 0x0001),
            (0x10, 0x8835),
            (0x11, 0x0001),
            (0x10, 0x8834),
            (0x11, 0x0001),
            (0x10, 0x8833),
            (0x11, 0x0002),
        ] {
            let _ = em_write_phy_reg(hw, reg, val);
        }
    }

    if (hw.phy_type == em_phy_82577 && (hw.phy_revision == 1 || hw.phy_revision == 2))
        || (hw.phy_type == em_phy_82578 && hw.phy_revision == 1)
    {
        // Disable generation of early preamble.
        em_write_phy_reg(hw, phy_reg(769, 25), 0x4431)?;

        // Preamble tuning for SSC.
        em_write_phy_reg(hw, phy_reg(770, 16), 0xA204)?;
    }

    if hw.phy_type == em_phy_82578 && hw.phy_revision < 2 {
        // Return registers to default by doing a soft reset then writing 0x3140 to the
        // control register (the C overwrites both statuses unread).
        let _ = em_phy_reset(hw);
        let _ = em_write_phy_reg(hw, PHY_CTRL, 0x3140);
    }

    if hw.revision_id == 2
        && hw.phy_type == em_phy_82577
        && (hw.phy_revision == 2 || hw.phy_revision == 3)
    {
        // Workaround for OEM (GbE) not operating after reset - restart AN (twice).
        em_write_phy_reg(hw, phy_reg(0, 25), 0x0400)?;
        em_write_phy_reg(hw, phy_reg(0, 25), 0x0400)?;
    }

    // Select page 0.
    em_swfw_sync_acquire(hw, swfw)?;

    hw.phy_addr = 1;
    let ret_val = em_write_phy_reg(hw, IGP01E1000_PHY_PAGE_SELECT, 0);
    em_swfw_sync_release(hw, swfw);
    ret_val?;

    // Workaround for link disconnects on a busy hub in half duplex.
    em_read_phy_reg(hw, phy_reg(BM_PORT_CTRL_PAGE, 17), &mut phy_data)?;
    em_write_phy_reg(hw, phy_reg(BM_PORT_CTRL_PAGE, 17), phy_data & 0x00FF)
}

/// `em_link_stall_workaround_hv`: works around a Si bug where the link partner can get a
/// link up indication before the PHY does. If small packets are sent by the link partner
/// they can be placed in the packet buffer without being properly accounted for by the PHY
/// and will stall preventing further packets from being received. The workaround is to
/// clear the packet buffer after the PHY detects link up.
pub fn em_link_stall_workaround_hv(hw: &mut EmHw) -> Result<(), i32> {
    let mut phy_data: u16 = 0;

    if hw.phy_type != em_phy_82578 {
        return Ok(());
    }

    // Do not apply workaround if in PHY loopback bit 14 set.
    let _ = em_read_phy_reg(hw, PHY_CTRL, &mut phy_data);
    if phy_data & E1000_PHY_CTRL_LOOPBACK != 0 {
        return Ok(());
    }

    // Check if link is up and at 1Gbps.
    em_read_phy_reg(hw, BM_CS_STATUS, &mut phy_data)?;

    phy_data &= BM_CS_STATUS_LINK_UP | BM_CS_STATUS_RESOLVED | BM_CS_STATUS_SPEED_MASK;

    if phy_data != (BM_CS_STATUS_LINK_UP | BM_CS_STATUS_RESOLVED | BM_CS_STATUS_SPEED_1000) {
        return Ok(());
    }

    msec_delay(200);

    // Flush the packets in the fifo buffer.
    em_write_phy_reg(
        hw,
        HV_MUX_DATA_CTRL,
        HV_MUX_DATA_CTRL_GEN_TO_MAC | HV_MUX_DATA_CTRL_FORCE_SPEED,
    )?;

    em_write_phy_reg(hw, HV_MUX_DATA_CTRL, HV_MUX_DATA_CTRL_GEN_TO_MAC)
}

/// `em_k1_gig_workaround_hv`: K1 Si workaround. If K1 is enabled for 1Gbps, the MAC might
/// stall when transitioning from a lower speed. This workaround disables K1 whenever link
/// is at 1Gig. If link is down, the function will restore the default K1 setting located
/// in the NVM.
pub fn em_k1_gig_workaround_hv(hw: &mut EmHw, link: bool) -> Result<(), i32> {
    let mut phy_data: u16 = 0;

    if hw.mac_type != em_pchlan {
        return Ok(());
    }

    let mut nvm = [0u16; 1];
    em_read_eeprom_ich8(hw, E1000_NVM_K1_CONFIG, 1, &mut nvm)?;

    let mut k1_enable = nvm[0] & E1000_NVM_K1_ENABLE != 0;

    // Disable K1 when link is 1Gbps, otherwise use the NVM setting.
    if link {
        if hw.phy_type == em_phy_82578 {
            em_read_phy_reg(hw, BM_CS_STATUS, &mut phy_data)?;

            phy_data &= BM_CS_STATUS_LINK_UP | BM_CS_STATUS_RESOLVED | BM_CS_STATUS_SPEED_MASK;

            if phy_data == (BM_CS_STATUS_LINK_UP | BM_CS_STATUS_RESOLVED | BM_CS_STATUS_SPEED_1000)
            {
                k1_enable = false;
            }
        }

        if hw.phy_type == em_phy_82577 {
            em_read_phy_reg(hw, HV_M_STATUS, &mut phy_data)?;

            phy_data &= HV_M_STATUS_LINK_UP | HV_M_STATUS_AUTONEG_COMPLETE | HV_M_STATUS_SPEED_MASK;

            if phy_data
                == (HV_M_STATUS_LINK_UP | HV_M_STATUS_AUTONEG_COMPLETE | HV_M_STATUS_SPEED_1000)
            {
                k1_enable = false;
            }
        }

        // Link stall fix for link up.
        em_write_phy_reg(hw, phy_reg(770, 19), 0x0100)?;
    } else {
        // Link stall fix for link down.
        em_write_phy_reg(hw, phy_reg(770, 19), 0x4100)?;
    }

    em_configure_k1_ich8lan(hw, k1_enable)
}

/// `em_k1_workaround_lv`: sets the K1 beacon duration for 82579 parts.
pub fn em_k1_workaround_lv(hw: &mut EmHw) -> Result<(), i32> {
    let mut phy_data: u16 = 0;

    em_read_phy_reg(hw, BM_CS_STATUS, &mut phy_data)?;

    if (phy_data & (HV_M_STATUS_LINK_UP | HV_M_STATUS_AUTONEG_COMPLETE))
        == (HV_M_STATUS_LINK_UP | HV_M_STATUS_AUTONEG_COMPLETE)
    {
        let mut mac_reg = e1000_read_reg(hw, E1000_FEXTNVM4);
        mac_reg &= !E1000_FEXTNVM4_BEACON_DURATION_MASK;

        if phy_data & HV_M_STATUS_SPEED_1000 != 0 {
            mac_reg |= E1000_FEXTNVM4_BEACON_DURATION_8USEC;
        } else {
            mac_reg |= E1000_FEXTNVM4_BEACON_DURATION_16USEC;
        }

        e1000_write_reg(hw, E1000_FEXTNVM4, mac_reg);
    }

    Ok(())
}

/// `em_k1_workaround_lpt_lp`: K1 workaround on Lynxpoint-LP. When K1 is enabled for 1Gbps,
/// the MAC can miss 2 DMA completion indications preventing further DMA write requests.
/// Workaround the issue by disabling the de-assertion of the clock request when in 1Gbps
/// mode. Also, set appropriate Tx re-transmission timeouts for 10 and 100Half link speeds
/// in order to avoid Tx hangs.
pub fn em_k1_workaround_lpt_lp(hw: &mut EmHw, link: bool) -> Result<(), i32> {
    let mut fextnvm6 = e1000_read_reg(hw, E1000_FEXTNVM6);
    let status = e1000_read_reg(hw, E1000_STATUS);
    let mut reg: u16 = 0;

    if link && status & E1000_STATUS_SPEED_1000 != 0 {
        em_read_kmrn_reg(hw, E1000_KMRNCTRLSTA_K1_CONFIG, &mut reg)?;

        em_write_kmrn_reg(
            hw,
            E1000_KMRNCTRLSTA_K1_CONFIG,
            reg & !E1000_KMRNCTRLSTA_K1_ENABLE,
        )?;

        usec_delay(10);

        e1000_write_reg(hw, E1000_FEXTNVM6, fextnvm6 | E1000_FEXTNVM6_REQ_PLL_CLK);

        em_write_kmrn_reg(hw, E1000_KMRNCTRLSTA_K1_CONFIG, reg)
    } else {
        // Clear FEXTNVM6 bit 8 on link down or 10/100.
        fextnvm6 &= !E1000_FEXTNVM6_REQ_PLL_CLK;

        if link && !(status & E1000_STATUS_SPEED_100 != 0 && status & E1000_STATUS_FD != 0) {
            em_read_phy_reg(hw, I217_INBAND_CTRL, &mut reg)?;

            // Clear link status transmit timeout.
            reg &= !I217_INBAND_CTRL_LINK_STAT_TX_TIMEOUT_MASK;

            if status & E1000_STATUS_SPEED_100 != 0 {
                // Set inband Tx timeout to 5x10us for 100Half.
                reg |= 5 << I217_INBAND_CTRL_LINK_STAT_TX_TIMEOUT_SHIFT;

                // Do not extend the K1 entry latency for 100Half.
                fextnvm6 &= !E1000_FEXTNVM6_ENABLE_K1_ENTRY_CONDITION;
            } else {
                // Set inband Tx timeout to 50x10us for 10Full/Half.
                reg |= 50 << I217_INBAND_CTRL_LINK_STAT_TX_TIMEOUT_SHIFT;

                // Extend the K1 entry latency for 10 Mbps.
                fextnvm6 |= E1000_FEXTNVM6_ENABLE_K1_ENTRY_CONDITION;
            }

            em_write_phy_reg(hw, I217_INBAND_CTRL, reg)?;
        }

        e1000_write_reg(hw, E1000_FEXTNVM6, fextnvm6);
        Ok(())
    }
}

/// `em_gate_hw_phy_config_ich8lan`: gates (`gate` true) or ungates the automatic PHY
/// configuration via hardware, to perform the configuration via software instead.
pub fn em_gate_hw_phy_config_ich8lan(hw: &EmHw, gate: bool) {
    if hw.mac_type != em_pch2lan {
        return;
    }

    let mut extcnf_ctrl = e1000_read_reg(hw, E1000_EXTCNF_CTRL);

    if gate {
        extcnf_ctrl |= E1000_EXTCNF_CTRL_GATE_PHY_CFG;
    } else {
        extcnf_ctrl &= !E1000_EXTCNF_CTRL_GATE_PHY_CFG;
    }

    e1000_write_reg(hw, E1000_EXTCNF_CTRL, extcnf_ctrl);
}

/// `em_configure_k1_ich8lan`: configures the K1 power state. Assumes the semaphore is
/// already acquired.
pub fn em_configure_k1_ich8lan(hw: &mut EmHw, k1_enable: bool) -> Result<(), i32> {
    let mut kmrn_reg: u16 = 0;

    em_read_kmrn_reg(hw, E1000_KMRNCTRLSTA_K1_CONFIG, &mut kmrn_reg)?;

    if k1_enable {
        kmrn_reg |= E1000_KMRNCTRLSTA_K1_ENABLE;
    } else {
        kmrn_reg &= !E1000_KMRNCTRLSTA_K1_ENABLE;
    }

    em_write_kmrn_reg(hw, E1000_KMRNCTRLSTA_K1_CONFIG, kmrn_reg)?;

    usec_delay(20);
    let ctrl_ext = e1000_read_reg(hw, E1000_CTRL_EXT);
    let ctrl_reg = e1000_read_reg(hw, E1000_CTRL);

    let mut reg = ctrl_reg & !(E1000_CTRL_SPD_1000 | E1000_CTRL_SPD_100);
    reg |= E1000_CTRL_FRCSPD;
    e1000_write_reg(hw, E1000_CTRL, reg);

    e1000_write_reg(hw, E1000_CTRL_EXT, ctrl_ext | E1000_CTRL_EXT_SPD_BYPS);
    usec_delay(20);
    e1000_write_reg(hw, E1000_CTRL, ctrl_reg);
    e1000_write_reg(hw, E1000_CTRL_EXT, ctrl_ext);
    usec_delay(20);

    Ok(())
}

/// `em_lv_phy_workarounds_ich8lan`: a series of PHY workarounds to be done after every PHY
/// reset (PCH2).
pub fn em_lv_phy_workarounds_ich8lan(hw: &mut EmHw) -> Result<(), i32> {
    if hw.mac_type != em_pch2lan {
        return Ok(());
    }

    // Set MDIO slow mode before any other MDIO access (the C overwrites the status unread).
    let _ = em_set_mdio_slow_mode_hv(hw);

    let swfw = E1000_SWFW_PHY0_SM;
    em_swfw_sync_acquire(hw, swfw)?;
    let ret_val = 'release: {
        let r = em_write_phy_reg(hw, I82579_EMI_ADDR, I82579_MSE_THRESHOLD);
        if r.is_err() {
            break 'release r;
        }
        // Set MSE higher to enable link to stay up when noise is high.
        let r = em_write_phy_reg(hw, I82579_EMI_DATA, 0x0034);
        if r.is_err() {
            break 'release r;
        }
        let r = em_write_phy_reg(hw, I82579_EMI_ADDR, I82579_MSE_LINK_DOWN);
        if r.is_err() {
            break 'release r;
        }
        // Drop link after 5 times MSE threshold was reached.
        em_write_phy_reg(hw, I82579_EMI_DATA, 0x0005)
    };
    em_swfw_sync_release(hw, swfw);

    ret_val
}

/// `em_set_eee_i350`: enables or disables Energy Efficient Ethernet on an i350/i210 copper
/// port as `hw.eee_enable` says.
pub fn em_set_eee_i350(hw: &EmHw) -> Result<(), i32> {
    if hw.mac_type < em_i350 || hw.media_type != em_media_type_copper {
        return Ok(());
    }
    let mut ipcnfg = em_read_reg(hw, E1000_IPCNFG);
    let mut eeer = em_read_reg(hw, E1000_EEER);

    if hw.eee_enable {
        ipcnfg |= E1000_IPCNFG_EEE_1G_AN | E1000_IPCNFG_EEE_100M_AN;
        eeer |= E1000_EEER_TX_LPI_EN | E1000_EEER_RX_LPI_EN | E1000_EEER_LPI_FC;
    } else {
        ipcnfg &= !(E1000_IPCNFG_EEE_1G_AN | E1000_IPCNFG_EEE_100M_AN);
        eeer &= !(E1000_EEER_TX_LPI_EN | E1000_EEER_RX_LPI_EN | E1000_EEER_LPI_FC);
    }
    em_write_reg(hw, E1000_IPCNFG, ipcnfg);
    em_write_reg(hw, E1000_EEER, eeer);
    let _ = em_read_reg(hw, E1000_IPCNFG);
    let _ = em_read_reg(hw, E1000_EEER);
    Ok(())
}

/// `em_set_eee_pchlan`: enables or disables EEE support as `hw.eee_enable` says. The bits
/// in the LPI Control register will remain set only if/when link is up.
pub fn em_set_eee_pchlan(hw: &mut EmHw) -> Result<(), i32> {
    let mut phy_reg: u16 = 0;

    if hw.phy_type != em_phy_82579 && hw.phy_type != em_phy_i217 {
        return Ok(());
    }

    em_read_phy_reg(hw, I82579_LPI_CTRL, &mut phy_reg)?;

    if hw.eee_enable {
        phy_reg &= !I82579_LPI_CTRL_ENABLE_MASK;
    } else {
        phy_reg |= I82579_LPI_CTRL_ENABLE_MASK;
    }

    em_write_phy_reg(hw, I82579_LPI_CTRL, phy_reg)
}

/// `em_initialize_M88E1512_phy`: initializes the Marvell 1512 to work correctly with
/// Avoton.
#[allow(non_snake_case)] // OpenBSD name, verbatim
pub fn em_initialize_M88E1512_phy(hw: &mut EmHw) -> Result<(), i32> {
    // Check if this is correct PHY.
    if hw.phy_id != M88E1512_E_PHY_ID {
        return Ok(());
    }

    for (reg, val) in [
        // Switch to PHY page 0xFF.
        (M88E1543_PAGE_ADDR, 0x00FF),
        (M88E1512_CFG_REG_2, 0x214B),
        (M88E1512_CFG_REG_1, 0x2144),
        (M88E1512_CFG_REG_2, 0x0C28),
        (M88E1512_CFG_REG_1, 0x2146),
        (M88E1512_CFG_REG_2, 0xB233),
        (M88E1512_CFG_REG_1, 0x214D),
        (M88E1512_CFG_REG_2, 0xCC0C),
        (M88E1512_CFG_REG_1, 0x2159),
        // Switch to PHY page 0xFB.
        (M88E1543_PAGE_ADDR, 0x00FB),
        (M88E1512_CFG_REG_3, 0x000D),
        // Switch to PHY page 0x12.
        (M88E1543_PAGE_ADDR, 0x12),
        // Change mode to SGMII-to-Copper.
        (M88E1512_MODE, 0x8001),
        // Return the PHY to page 0.
        (M88E1543_PAGE_ADDR, 0),
    ] {
        em_write_phy_reg(hw, reg, val)?;
    }

    // Error committing the PHY changes.
    em_phy_hw_reset(hw)?;

    msec_delay(1000);
    Ok(())
}

/// `em_translate_82542_register`: some of the 82542 registers are located at different
/// offsets than they are in newer adapters. Despite the difference in location, the
/// registers function in the same manner.
pub const fn em_translate_82542_register(reg: u32) -> u32 {
    match reg {
        E1000_RA => 0x00040,
        E1000_RDTR => 0x00108,
        r if r == e1000_rdbal(0) => 0x00110,
        r if r == e1000_rdbah(0) => 0x00114,
        r if r == e1000_rdlen(0) => 0x00118,
        r if r == e1000_rdh(0) => 0x00120,
        r if r == e1000_rdt(0) => 0x00128,
        r if r == e1000_rdbal(1) => 0x00138,
        r if r == e1000_rdbah(1) => 0x0013C,
        r if r == e1000_rdlen(1) => 0x00140,
        r if r == e1000_rdh(1) => 0x00148,
        r if r == e1000_rdt(1) => 0x00150,
        E1000_FCRTH => 0x00160,
        E1000_FCRTL => 0x00168,
        E1000_MTA => 0x00200,
        r if r == e1000_tdbal(0) => 0x00420,
        r if r == e1000_tdbah(0) => 0x00424,
        r if r == e1000_tdlen(0) => 0x00428,
        r if r == e1000_tdh(0) => 0x00430,
        r if r == e1000_tdt(0) => 0x00438,
        E1000_TIDV => 0x00440,
        E1000_VFTA => 0x00600,
        E1000_TDFH => 0x08010,
        E1000_TDFT => 0x08018,
        _ => reg,
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use core::ptr::NonNull;

    use super::*;
    use crate::dev::pci::pci_map::tests::attach_args;
    use crate::machine::bus::{BusSpaceTag, bus_space_map};

    fn no_mwi(_: &EmHw) {}

    fn no_cfg_read(_: &EmHw, _: u32, v: &mut u16) {
        *v = 0;
    }

    fn no_cfg_write(_: &EmHw, _: u32, _: &u16) {}

    fn no_cap(_: &EmHw, _: u32, _: &mut u16) -> Result<(), i32> {
        Err(-E1000_NOT_IMPLEMENTED)
    }

    /// What if_em.c will pass: the C's `em_read_pcie_cap_reg` is "not implemented".
    static TEST_PCI_OPS: EmPciOps = EmPciOps {
        em_pci_set_mwi: no_mwi,
        em_pci_clear_mwi: no_mwi,
        em_read_pci_cfg: no_cfg_read,
        em_write_pci_cfg: no_cfg_write,
        em_read_pcie_cap_reg: no_cap,
    };

    /// A softc's osdep over the host's fake bus space (every register reads 0).
    fn osdep() -> Box<EmOsdep> {
        let t = BusSpaceTag::default();
        // SAFETY: the host bus space maps nothing; its handles only carry the address.
        let h = unsafe { bus_space_map(t, 0, 0x20000, 0) }.expect("host map");
        Box::new(EmOsdep {
            mem_bus_space_tag: t,
            mem_bus_space_handle: h,
            io_bus_space_tag: t,
            io_bus_space_handle: h,
            flash_bus_space_tag: t,
            flash_bus_space_handle: h,
            dev: None,
            em_pa: attach_args(0, 3, 0),
            em_memsize: 0x20000,
            em_membase: 0,
            em_iosize: 0,
            em_iobase: 0,
            em_flashsize: 0,
            em_flashbase: 0,
            em_flashoffset: 0,
        })
    }

    /// An `EmHw` for `device_id`/`revision_id`, its osdep kept alive beside it.
    fn hw_for(device_id: u16, revision_id: u8) -> (Box<EmOsdep>, EmHw) {
        let mut o = osdep();
        let back = NonNull::from(&mut *o);
        // SAFETY: the osdep is boxed, lives as long as the returned pair, and is never written.
        let mut hw = unsafe { EmHw::new(back, &TEST_PCI_OPS) };
        hw.device_id = device_id;
        hw.revision_id = revision_id;
        (o, hw)
    }

    #[test]
    fn mac_type_from_device_id() {
        for (id, rev, mac) in [
            (E1000_DEV_ID_82540EM, 0, em_82540),
            (E1000_DEV_ID_82574L, 0, em_82574),
            (E1000_DEV_ID_82583V, 0, em_82574),
            (E1000_DEV_ID_82576, 0, em_82576),
            (E1000_DEV_ID_82542, E1000_82542_2_0_REV_ID, em_82542_rev2_0),
            (E1000_DEV_ID_82542, E1000_82542_2_1_REV_ID, em_82542_rev2_1),
            (E1000_DEV_ID_I210_COPPER, 0, em_i210),
            (E1000_DEV_ID_PCH_SPT_I219_LM, 0, em_pch_spt),
            (E1000_DEV_ID_EP80579_LAN_5, 0, em_icp_xxxx),
        ] {
            let (_o, mut hw) = hw_for(id, rev);
            assert_eq!(em_set_mac_type(&mut hw), Ok(()), "{id:#x}");
            assert_eq!(hw.mac_type, mac, "{id:#x}");
        }
        let (_o, mut hw) = hw_for(E1000_DEV_ID_82542, 7);
        assert_eq!(em_set_mac_type(&mut hw), Err(-E1000_ERR_MAC_TYPE));
        let (_o, mut hw) = hw_for(0xffff, 0);
        assert_eq!(em_set_mac_type(&mut hw), Err(-E1000_ERR_MAC_TYPE));
    }

    #[test]
    fn mac_type_semaphore_flags() {
        // The C's fall-through: 82576 has all three, 82574 the EEPROM semaphore and ASF.
        let (_o, mut hw) = hw_for(E1000_DEV_ID_82576, 0);
        em_set_mac_type(&mut hw).expect("82576");
        assert_eq!(
            (
                hw.swfw_sync_present,
                hw.eeprom_semaphore_present,
                hw.asf_firmware_present
            ),
            (1, 1, 1)
        );
        assert!(hw.initialize_hw_bits_disable);
        let (_o, mut hw) = hw_for(E1000_DEV_ID_82574L, 0);
        em_set_mac_type(&mut hw).expect("82574");
        assert_eq!(
            (
                hw.swfw_sync_present,
                hw.eeprom_semaphore_present,
                hw.asf_firmware_present
            ),
            (0, 1, 1)
        );
        let (_o, mut hw) = hw_for(E1000_DEV_ID_82540EM, 0);
        em_set_mac_type(&mut hw).expect("82540");
        assert_eq!(
            (
                hw.swfw_sync_present,
                hw.eeprom_semaphore_present,
                hw.asf_firmware_present
            ),
            (0, 0, 0)
        );
        let (_o, mut hw) = hw_for(E1000_DEV_ID_ICH9_BM, 0);
        em_set_mac_type(&mut hw).expect("ich9");
        assert_eq!(
            (hw.swfwhw_semaphore_present, hw.asf_firmware_present),
            (1, 1)
        );
        assert!(is_ich8(hw.mac_type));
    }

    #[test]
    fn multicast_hash() {
        // The C's comment example: 01 AA 00 12 34 56.
        let addr = [0x01, 0xAA, 0x00, 0x12, 0x34, 0x56];
        let (_o, mut hw) = hw_for(E1000_DEV_ID_82540EM, 0);
        hw.mac_type = em_82540;
        // Type 2 is 0x58D: the C comment's 0x5D8 is a typo of its own code, `(0x34 >> 2) | (0x56 << 6)`.
        for (t, v) in [(0, 0x563), (1, 0xAC6), (2, 0x58D), (3, 0x634)] {
            hw.mc_filter_type = t;
            assert_eq!(em_hash_mc_addr(&hw, &addr), v, "type {t}");
        }
        hw.mac_type = em_ich8lan;
        for (t, v) in [(0, 0x158), (1, 0x2B1), (2, 0x163), (3, 0x18D)] {
            hw.mc_filter_type = t;
            assert_eq!(em_hash_mc_addr(&hw, &addr), v, "ich8 type {t}");
        }
    }

    #[test]
    fn translate_82542() {
        assert_eq!(em_translate_82542_register(e1000_rdbal(0)), 0x00110);
        assert_eq!(em_translate_82542_register(e1000_rdt(1)), 0x00150);
        assert_eq!(em_translate_82542_register(e1000_tdt(0)), 0x00438);
        assert_eq!(em_translate_82542_register(E1000_MTA), 0x00200);
        assert_eq!(em_translate_82542_register(E1000_CTRL), E1000_CTRL);
        assert_eq!(em_translate_82542_register(e1000_rdbal(4)), e1000_rdbal(4));
    }

    #[test]
    fn register_macros() {
        assert_eq!(e1000_rdbal(0), 0x02800);
        assert_eq!(e1000_rdbal(4), 0x0C000 + 4 * 0x40);
        assert_eq!(e1000_txdctl(1), 0x03928);
        assert_eq!(e1000_byte_swap_word(0x1234), 0x3412);
        assert_eq!(nvm_82580_lan_func_offset(0), 0);
        assert_eq!(nvm_82580_lan_func_offset(2), 0xC0);
        assert_eq!(eeprom_ia_start_icp_xxxx(1), 0x22);
        // BM PHY addresses: page and register come back out.
        let r = bm_phy_reg(BM_WUC_PAGE, 17);
        assert_eq!(u32::from(bm_phy_reg_page(r)), BM_WUC_PAGE);
        assert_eq!(bm_phy_reg_num(r), 17);
        assert_eq!(phy_reg(769, 17), (769 << PHY_PAGE_SHIFT) | 17);
        assert_eq!(gg82563_reg(194, 18), GG82563_PHY_INBAND_CTRL);
    }

    #[test]
    fn mng_checksum_and_cookie() {
        let bytes = [1u8, 2, 3, 250, 7];
        let c = em_calculate_mng_checksum(&bytes);
        let sum = bytes.iter().fold(c, |a, b| a.wrapping_add(*b));
        assert_eq!(sum, 0);
        let mut b = [0u8; 16];
        for (i, x) in b.iter_mut().enumerate() {
            *x = i as u8 * 3 + 1;
        }
        let cookie = EmHostMngDhcpCookie::from_bytes(&b);
        assert_eq!(cookie.signature, u32::from_le_bytes([1, 4, 7, 10]));
        assert_eq!(cookie.checksum, 46);
        assert_eq!(cookie.to_bytes(), b);
    }

    #[test]
    fn flash_register_fields() {
        let mut s = Ich8HwsFlashStatus { regval: 0x4000 };
        assert_eq!(s.fldesvalid(), 1);
        s.set_flcerr(1);
        s.set_dael(1);
        assert_eq!(s.regval, 0x4006);
        s.set_berasesz(3);
        assert_eq!(s.berasesz(), 3);
        let mut c = Ich8HwsFlashCtrl::default();
        c.set_fldbcount(1);
        c.set_flcycle(ICH_CYCLE_ERASE as u16);
        c.set_flcgo(1);
        assert_eq!(c.regval, 0x0100 | (3 << 1) | 1);
        let f = SfpE1000Flags { regval: 0x08 };
        assert_eq!((f.e1000_base_t(), f.e1000_base_sx()), (1, 0));
    }

    #[test]
    fn bus_width_from_pcie() {
        assert_eq!(EmBusWidth::from_pcie_link_width(1), em_bus_width_pciex_1);
        assert_eq!(EmBusWidth::from_pcie_link_width(4), em_bus_width_pciex_4);
        assert_eq!(EmBusWidth::from_pcie_link_width(8), em_bus_width_reserved);
    }

    /// Every simple `#define` of `if_em_hw.h`, with its Rust value.
    fn header_defines() -> std::vec::Vec<(&'static str, i64)> {
        std::vec![
            ("E1000_HOST_IF_MAX_SIZE", E1000_HOST_IF_MAX_SIZE as i64),
            ("E1000_SUCCESS", E1000_SUCCESS as i64),
            ("E1000_ERR_EEPROM", E1000_ERR_EEPROM as i64),
            ("E1000_ERR_PHY", E1000_ERR_PHY as i64),
            ("E1000_ERR_CONFIG", E1000_ERR_CONFIG as i64),
            ("E1000_ERR_PARAM", E1000_ERR_PARAM as i64),
            ("E1000_ERR_MAC_TYPE", E1000_ERR_MAC_TYPE as i64),
            ("E1000_ERR_PHY_TYPE", E1000_ERR_PHY_TYPE as i64),
            ("E1000_ERR_RESET", E1000_ERR_RESET as i64),
            (
                "E1000_ERR_MASTER_REQUESTS_PENDING",
                E1000_ERR_MASTER_REQUESTS_PENDING as i64
            ),
            (
                "E1000_ERR_HOST_INTERFACE_COMMAND",
                E1000_ERR_HOST_INTERFACE_COMMAND as i64
            ),
            ("E1000_BLK_PHY_RESET", E1000_BLK_PHY_RESET as i64),
            ("E1000_ERR_SWFW_SYNC", E1000_ERR_SWFW_SYNC as i64),
            ("E1000_NOT_IMPLEMENTED", E1000_NOT_IMPLEMENTED as i64),
            ("E1000_DEFER_INIT", E1000_DEFER_INIT as i64),
            (
                "E1000_MNG_DHCP_TX_PAYLOAD_CMD",
                E1000_MNG_DHCP_TX_PAYLOAD_CMD as i64
            ),
            (
                "E1000_HI_MAX_MNG_DATA_LENGTH",
                E1000_HI_MAX_MNG_DATA_LENGTH as i64
            ),
            (
                "E1000_MNG_DHCP_COMMAND_TIMEOUT",
                E1000_MNG_DHCP_COMMAND_TIMEOUT as i64
            ),
            (
                "E1000_MNG_DHCP_COOKIE_OFFSET",
                E1000_MNG_DHCP_COOKIE_OFFSET as i64
            ),
            (
                "E1000_MNG_DHCP_COOKIE_LENGTH",
                E1000_MNG_DHCP_COOKIE_LENGTH as i64
            ),
            ("E1000_MNG_IAMT_MODE", E1000_MNG_IAMT_MODE as i64),
            ("E1000_MNG_ICH_IAMT_MODE", E1000_MNG_ICH_IAMT_MODE as i64),
            ("E1000_IAMT_SIGNATURE", E1000_IAMT_SIGNATURE as i64),
            (
                "E1000_MNG_DHCP_COOKIE_STATUS_PARSING_SUPPORT",
                E1000_MNG_DHCP_COOKIE_STATUS_PARSING_SUPPORT as i64
            ),
            (
                "E1000_MNG_DHCP_COOKIE_STATUS_VLAN_SUPPORT",
                E1000_MNG_DHCP_COOKIE_STATUS_VLAN_SUPPORT as i64
            ),
            ("E1000_VFTA_ENTRY_SHIFT", E1000_VFTA_ENTRY_SHIFT as i64),
            ("E1000_VFTA_ENTRY_MASK", E1000_VFTA_ENTRY_MASK as i64),
            (
                "E1000_VFTA_ENTRY_BIT_SHIFT_MASK",
                E1000_VFTA_ENTRY_BIT_SHIFT_MASK as i64
            ),
            ("E1000_DEV_ID_82542", E1000_DEV_ID_82542 as i64),
            (
                "E1000_DEV_ID_82543GC_FIBER",
                E1000_DEV_ID_82543GC_FIBER as i64
            ),
            (
                "E1000_DEV_ID_82543GC_COPPER",
                E1000_DEV_ID_82543GC_COPPER as i64
            ),
            (
                "E1000_DEV_ID_82544EI_COPPER",
                E1000_DEV_ID_82544EI_COPPER as i64
            ),
            (
                "E1000_DEV_ID_82544EI_FIBER",
                E1000_DEV_ID_82544EI_FIBER as i64
            ),
            (
                "E1000_DEV_ID_82544GC_COPPER",
                E1000_DEV_ID_82544GC_COPPER as i64
            ),
            ("E1000_DEV_ID_82544GC_LOM", E1000_DEV_ID_82544GC_LOM as i64),
            ("E1000_DEV_ID_82540EM", E1000_DEV_ID_82540EM as i64),
            ("E1000_DEV_ID_82540EM_LOM", E1000_DEV_ID_82540EM_LOM as i64),
            ("E1000_DEV_ID_82540EP_LOM", E1000_DEV_ID_82540EP_LOM as i64),
            ("E1000_DEV_ID_82540EP", E1000_DEV_ID_82540EP as i64),
            ("E1000_DEV_ID_82540EP_LP", E1000_DEV_ID_82540EP_LP as i64),
            (
                "E1000_DEV_ID_82545EM_COPPER",
                E1000_DEV_ID_82545EM_COPPER as i64
            ),
            (
                "E1000_DEV_ID_82545EM_FIBER",
                E1000_DEV_ID_82545EM_FIBER as i64
            ),
            (
                "E1000_DEV_ID_82545GM_COPPER",
                E1000_DEV_ID_82545GM_COPPER as i64
            ),
            (
                "E1000_DEV_ID_82545GM_FIBER",
                E1000_DEV_ID_82545GM_FIBER as i64
            ),
            (
                "E1000_DEV_ID_82545GM_SERDES",
                E1000_DEV_ID_82545GM_SERDES as i64
            ),
            (
                "E1000_DEV_ID_82546EB_COPPER",
                E1000_DEV_ID_82546EB_COPPER as i64
            ),
            (
                "E1000_DEV_ID_82546EB_FIBER",
                E1000_DEV_ID_82546EB_FIBER as i64
            ),
            (
                "E1000_DEV_ID_82546EB_QUAD_COPPER",
                E1000_DEV_ID_82546EB_QUAD_COPPER as i64
            ),
            ("E1000_DEV_ID_82541EI", E1000_DEV_ID_82541EI as i64),
            (
                "E1000_DEV_ID_82541EI_MOBILE",
                E1000_DEV_ID_82541EI_MOBILE as i64
            ),
            ("E1000_DEV_ID_82541ER_LOM", E1000_DEV_ID_82541ER_LOM as i64),
            ("E1000_DEV_ID_82541ER", E1000_DEV_ID_82541ER as i64),
            ("E1000_DEV_ID_82547GI", E1000_DEV_ID_82547GI as i64),
            ("E1000_DEV_ID_82541GI", E1000_DEV_ID_82541GI as i64),
            (
                "E1000_DEV_ID_82541GI_MOBILE",
                E1000_DEV_ID_82541GI_MOBILE as i64
            ),
            ("E1000_DEV_ID_82541GI_LF", E1000_DEV_ID_82541GI_LF as i64),
            (
                "E1000_DEV_ID_82546GB_COPPER",
                E1000_DEV_ID_82546GB_COPPER as i64
            ),
            (
                "E1000_DEV_ID_82546GB_FIBER",
                E1000_DEV_ID_82546GB_FIBER as i64
            ),
            (
                "E1000_DEV_ID_82546GB_SERDES",
                E1000_DEV_ID_82546GB_SERDES as i64
            ),
            (
                "E1000_DEV_ID_82546GB_PCIE",
                E1000_DEV_ID_82546GB_PCIE as i64
            ),
            (
                "E1000_DEV_ID_82546GB_QUAD_COPPER",
                E1000_DEV_ID_82546GB_QUAD_COPPER as i64
            ),
            ("E1000_DEV_ID_82547EI", E1000_DEV_ID_82547EI as i64),
            (
                "E1000_DEV_ID_82547EI_MOBILE",
                E1000_DEV_ID_82547EI_MOBILE as i64
            ),
            (
                "E1000_DEV_ID_82571EB_COPPER",
                E1000_DEV_ID_82571EB_COPPER as i64
            ),
            (
                "E1000_DEV_ID_82571EB_FIBER",
                E1000_DEV_ID_82571EB_FIBER as i64
            ),
            (
                "E1000_DEV_ID_82571EB_SERDES",
                E1000_DEV_ID_82571EB_SERDES as i64
            ),
            (
                "E1000_DEV_ID_82571EB_SERDES_DUAL",
                E1000_DEV_ID_82571EB_SERDES_DUAL as i64
            ),
            (
                "E1000_DEV_ID_82571EB_SERDES_QUAD",
                E1000_DEV_ID_82571EB_SERDES_QUAD as i64
            ),
            (
                "E1000_DEV_ID_82571EB_QUAD_COPPER",
                E1000_DEV_ID_82571EB_QUAD_COPPER as i64
            ),
            (
                "E1000_DEV_ID_82571EB_QUAD_FIBER",
                E1000_DEV_ID_82571EB_QUAD_FIBER as i64
            ),
            (
                "E1000_DEV_ID_82571EB_QUAD_COPPER_LP",
                E1000_DEV_ID_82571EB_QUAD_COPPER_LP as i64
            ),
            (
                "E1000_DEV_ID_82571PT_QUAD_COPPER",
                E1000_DEV_ID_82571PT_QUAD_COPPER as i64
            ),
            (
                "E1000_DEV_ID_82572EI_COPPER",
                E1000_DEV_ID_82572EI_COPPER as i64
            ),
            (
                "E1000_DEV_ID_82572EI_FIBER",
                E1000_DEV_ID_82572EI_FIBER as i64
            ),
            (
                "E1000_DEV_ID_82572EI_SERDES",
                E1000_DEV_ID_82572EI_SERDES as i64
            ),
            ("E1000_DEV_ID_82572EI", E1000_DEV_ID_82572EI as i64),
            ("E1000_DEV_ID_82573E", E1000_DEV_ID_82573E as i64),
            ("E1000_DEV_ID_82573E_IAMT", E1000_DEV_ID_82573E_IAMT as i64),
            ("E1000_DEV_ID_82573L", E1000_DEV_ID_82573L as i64),
            ("E1000_DEV_ID_82574L", E1000_DEV_ID_82574L as i64),
            ("E1000_DEV_ID_82574LA", E1000_DEV_ID_82574LA as i64),
            ("E1000_DEV_ID_82546GB_2", E1000_DEV_ID_82546GB_2 as i64),
            ("E1000_DEV_ID_82571EB_AT", E1000_DEV_ID_82571EB_AT as i64),
            ("E1000_DEV_ID_82571EB_AF", E1000_DEV_ID_82571EB_AF as i64),
            ("E1000_DEV_ID_82573L_PL_1", E1000_DEV_ID_82573L_PL_1 as i64),
            ("E1000_DEV_ID_82573V_PM", E1000_DEV_ID_82573V_PM as i64),
            ("E1000_DEV_ID_82573E_PM", E1000_DEV_ID_82573E_PM as i64),
            ("E1000_DEV_ID_82573L_PL_2", E1000_DEV_ID_82573L_PL_2 as i64),
            (
                "E1000_DEV_ID_82546GB_QUAD_COPPER_KSP3",
                E1000_DEV_ID_82546GB_QUAD_COPPER_KSP3 as i64
            ),
            (
                "E1000_DEV_ID_80003ES2LAN_COPPER_DPT",
                E1000_DEV_ID_80003ES2LAN_COPPER_DPT as i64
            ),
            (
                "E1000_DEV_ID_80003ES2LAN_SERDES_DPT",
                E1000_DEV_ID_80003ES2LAN_SERDES_DPT as i64
            ),
            (
                "E1000_DEV_ID_80003ES2LAN_COPPER_SPT",
                E1000_DEV_ID_80003ES2LAN_COPPER_SPT as i64
            ),
            (
                "E1000_DEV_ID_80003ES2LAN_SERDES_SPT",
                E1000_DEV_ID_80003ES2LAN_SERDES_SPT as i64
            ),
            (
                "E1000_DEV_ID_ICH8_82567V_3",
                E1000_DEV_ID_ICH8_82567V_3 as i64
            ),
            (
                "E1000_DEV_ID_ICH8_IGP_M_AMT",
                E1000_DEV_ID_ICH8_IGP_M_AMT as i64
            ),
            (
                "E1000_DEV_ID_ICH8_IGP_AMT",
                E1000_DEV_ID_ICH8_IGP_AMT as i64
            ),
            ("E1000_DEV_ID_ICH8_IGP_C", E1000_DEV_ID_ICH8_IGP_C as i64),
            ("E1000_DEV_ID_ICH8_IFE", E1000_DEV_ID_ICH8_IFE as i64),
            ("E1000_DEV_ID_ICH8_IFE_GT", E1000_DEV_ID_ICH8_IFE_GT as i64),
            ("E1000_DEV_ID_ICH8_IFE_G", E1000_DEV_ID_ICH8_IFE_G as i64),
            ("E1000_DEV_ID_ICH8_IGP_M", E1000_DEV_ID_ICH8_IGP_M as i64),
            ("E1000_DEV_ID_ICH9_IGP_M", E1000_DEV_ID_ICH9_IGP_M as i64),
            (
                "E1000_DEV_ID_ICH9_IGP_M_AMT",
                E1000_DEV_ID_ICH9_IGP_M_AMT as i64
            ),
            (
                "E1000_DEV_ID_ICH9_IGP_M_V",
                E1000_DEV_ID_ICH9_IGP_M_V as i64
            ),
            (
                "E1000_DEV_ID_ICH9_IGP_AMT",
                E1000_DEV_ID_ICH9_IGP_AMT as i64
            ),
            ("E1000_DEV_ID_ICH9_BM", E1000_DEV_ID_ICH9_BM as i64),
            ("E1000_DEV_ID_ICH9_IGP_C", E1000_DEV_ID_ICH9_IGP_C as i64),
            ("E1000_DEV_ID_ICH9_IFE", E1000_DEV_ID_ICH9_IFE as i64),
            ("E1000_DEV_ID_ICH9_IFE_GT", E1000_DEV_ID_ICH9_IFE_GT as i64),
            ("E1000_DEV_ID_ICH9_IFE_G", E1000_DEV_ID_ICH9_IFE_G as i64),
            (
                "E1000_DEV_ID_ICH10_R_BM_LM",
                E1000_DEV_ID_ICH10_R_BM_LM as i64
            ),
            (
                "E1000_DEV_ID_ICH10_R_BM_LF",
                E1000_DEV_ID_ICH10_R_BM_LF as i64
            ),
            (
                "E1000_DEV_ID_ICH10_R_BM_V",
                E1000_DEV_ID_ICH10_R_BM_V as i64
            ),
            (
                "E1000_DEV_ID_ICH10_D_BM_LM",
                E1000_DEV_ID_ICH10_D_BM_LM as i64
            ),
            (
                "E1000_DEV_ID_ICH10_D_BM_LF",
                E1000_DEV_ID_ICH10_D_BM_LF as i64
            ),
            (
                "E1000_DEV_ID_ICH10_D_BM_V",
                E1000_DEV_ID_ICH10_D_BM_V as i64
            ),
            ("E1000_DEV_ID_PCH_M_HV_LM", E1000_DEV_ID_PCH_M_HV_LM as i64),
            ("E1000_DEV_ID_PCH_M_HV_LC", E1000_DEV_ID_PCH_M_HV_LC as i64),
            ("E1000_DEV_ID_PCH_D_HV_DM", E1000_DEV_ID_PCH_D_HV_DM as i64),
            ("E1000_DEV_ID_PCH_D_HV_DC", E1000_DEV_ID_PCH_D_HV_DC as i64),
            ("E1000_DEV_ID_PCH2_LV_LM", E1000_DEV_ID_PCH2_LV_LM as i64),
            ("E1000_DEV_ID_PCH2_LV_V", E1000_DEV_ID_PCH2_LV_V as i64),
            (
                "E1000_DEV_ID_PCH_LPT_I217_LM",
                E1000_DEV_ID_PCH_LPT_I217_LM as i64
            ),
            (
                "E1000_DEV_ID_PCH_LPT_I217_V",
                E1000_DEV_ID_PCH_LPT_I217_V as i64
            ),
            (
                "E1000_DEV_ID_PCH_LPTLP_I218_LM",
                E1000_DEV_ID_PCH_LPTLP_I218_LM as i64
            ),
            (
                "E1000_DEV_ID_PCH_LPTLP_I218_V",
                E1000_DEV_ID_PCH_LPTLP_I218_V as i64
            ),
            (
                "E1000_DEV_ID_PCH_I218_LM2",
                E1000_DEV_ID_PCH_I218_LM2 as i64
            ),
            ("E1000_DEV_ID_PCH_I218_V2", E1000_DEV_ID_PCH_I218_V2 as i64),
            (
                "E1000_DEV_ID_PCH_I218_LM3",
                E1000_DEV_ID_PCH_I218_LM3 as i64
            ),
            ("E1000_DEV_ID_PCH_I218_V3", E1000_DEV_ID_PCH_I218_V3 as i64),
            (
                "E1000_DEV_ID_PCH_SPT_I219_LM",
                E1000_DEV_ID_PCH_SPT_I219_LM as i64
            ),
            (
                "E1000_DEV_ID_PCH_SPT_I219_V",
                E1000_DEV_ID_PCH_SPT_I219_V as i64
            ),
            (
                "E1000_DEV_ID_PCH_SPT_I219_LM2",
                E1000_DEV_ID_PCH_SPT_I219_LM2 as i64
            ),
            (
                "E1000_DEV_ID_PCH_SPT_I219_V2",
                E1000_DEV_ID_PCH_SPT_I219_V2 as i64
            ),
            (
                "E1000_DEV_ID_PCH_LBG_I219_LM3",
                E1000_DEV_ID_PCH_LBG_I219_LM3 as i64
            ),
            (
                "E1000_DEV_ID_PCH_SPT_I219_LM4",
                E1000_DEV_ID_PCH_SPT_I219_LM4 as i64
            ),
            (
                "E1000_DEV_ID_PCH_SPT_I219_V4",
                E1000_DEV_ID_PCH_SPT_I219_V4 as i64
            ),
            (
                "E1000_DEV_ID_PCH_SPT_I219_LM5",
                E1000_DEV_ID_PCH_SPT_I219_LM5 as i64
            ),
            (
                "E1000_DEV_ID_PCH_SPT_I219_V5",
                E1000_DEV_ID_PCH_SPT_I219_V5 as i64
            ),
            (
                "E1000_DEV_ID_PCH_CNP_I219_LM6",
                E1000_DEV_ID_PCH_CNP_I219_LM6 as i64
            ),
            (
                "E1000_DEV_ID_PCH_CNP_I219_V6",
                E1000_DEV_ID_PCH_CNP_I219_V6 as i64
            ),
            (
                "E1000_DEV_ID_PCH_CNP_I219_LM7",
                E1000_DEV_ID_PCH_CNP_I219_LM7 as i64
            ),
            (
                "E1000_DEV_ID_PCH_CNP_I219_V7",
                E1000_DEV_ID_PCH_CNP_I219_V7 as i64
            ),
            (
                "E1000_DEV_ID_PCH_ICP_I219_LM8",
                E1000_DEV_ID_PCH_ICP_I219_LM8 as i64
            ),
            (
                "E1000_DEV_ID_PCH_ICP_I219_V8",
                E1000_DEV_ID_PCH_ICP_I219_V8 as i64
            ),
            (
                "E1000_DEV_ID_PCH_ICP_I219_LM9",
                E1000_DEV_ID_PCH_ICP_I219_LM9 as i64
            ),
            (
                "E1000_DEV_ID_PCH_ICP_I219_V9",
                E1000_DEV_ID_PCH_ICP_I219_V9 as i64
            ),
            (
                "E1000_DEV_ID_PCH_CMP_I219_LM10",
                E1000_DEV_ID_PCH_CMP_I219_LM10 as i64
            ),
            (
                "E1000_DEV_ID_PCH_CMP_I219_V10",
                E1000_DEV_ID_PCH_CMP_I219_V10 as i64
            ),
            (
                "E1000_DEV_ID_PCH_CMP_I219_LM11",
                E1000_DEV_ID_PCH_CMP_I219_LM11 as i64
            ),
            (
                "E1000_DEV_ID_PCH_CMP_I219_V11",
                E1000_DEV_ID_PCH_CMP_I219_V11 as i64
            ),
            (
                "E1000_DEV_ID_PCH_CMP_I219_LM12",
                E1000_DEV_ID_PCH_CMP_I219_LM12 as i64
            ),
            (
                "E1000_DEV_ID_PCH_CMP_I219_V12",
                E1000_DEV_ID_PCH_CMP_I219_V12 as i64
            ),
            (
                "E1000_DEV_ID_PCH_TGP_I219_LM13",
                E1000_DEV_ID_PCH_TGP_I219_LM13 as i64
            ),
            (
                "E1000_DEV_ID_PCH_TGP_I219_V13",
                E1000_DEV_ID_PCH_TGP_I219_V13 as i64
            ),
            (
                "E1000_DEV_ID_PCH_TGP_I219_LM14",
                E1000_DEV_ID_PCH_TGP_I219_LM14 as i64
            ),
            (
                "E1000_DEV_ID_PCH_TGP_I219_V14",
                E1000_DEV_ID_PCH_TGP_I219_V14 as i64
            ),
            (
                "E1000_DEV_ID_PCH_TGP_I219_LM15",
                E1000_DEV_ID_PCH_TGP_I219_LM15 as i64
            ),
            (
                "E1000_DEV_ID_PCH_TGP_I219_V15",
                E1000_DEV_ID_PCH_TGP_I219_V15 as i64
            ),
            (
                "E1000_DEV_ID_PCH_ADP_I219_LM16",
                E1000_DEV_ID_PCH_ADP_I219_LM16 as i64
            ),
            (
                "E1000_DEV_ID_PCH_ADP_I219_V16",
                E1000_DEV_ID_PCH_ADP_I219_V16 as i64
            ),
            (
                "E1000_DEV_ID_PCH_ADP_I219_LM17",
                E1000_DEV_ID_PCH_ADP_I219_LM17 as i64
            ),
            (
                "E1000_DEV_ID_PCH_ADP_I219_V17",
                E1000_DEV_ID_PCH_ADP_I219_V17 as i64
            ),
            (
                "E1000_DEV_ID_PCH_MTP_I219_LM18",
                E1000_DEV_ID_PCH_MTP_I219_LM18 as i64
            ),
            (
                "E1000_DEV_ID_PCH_MTP_I219_V18",
                E1000_DEV_ID_PCH_MTP_I219_V18 as i64
            ),
            (
                "E1000_DEV_ID_PCH_MTP_I219_LM19",
                E1000_DEV_ID_PCH_MTP_I219_LM19 as i64
            ),
            (
                "E1000_DEV_ID_PCH_MTP_I219_V19",
                E1000_DEV_ID_PCH_MTP_I219_V19 as i64
            ),
            (
                "E1000_DEV_ID_PCH_LNP_I219_LM20",
                E1000_DEV_ID_PCH_LNP_I219_LM20 as i64
            ),
            (
                "E1000_DEV_ID_PCH_LNP_I219_V20",
                E1000_DEV_ID_PCH_LNP_I219_V20 as i64
            ),
            (
                "E1000_DEV_ID_PCH_LNP_I219_LM21",
                E1000_DEV_ID_PCH_LNP_I219_LM21 as i64
            ),
            (
                "E1000_DEV_ID_PCH_LNP_I219_V21",
                E1000_DEV_ID_PCH_LNP_I219_V21 as i64
            ),
            (
                "E1000_DEV_ID_PCH_RPL_I219_LM22",
                E1000_DEV_ID_PCH_RPL_I219_LM22 as i64
            ),
            (
                "E1000_DEV_ID_PCH_RPL_I219_V22",
                E1000_DEV_ID_PCH_RPL_I219_V22 as i64
            ),
            (
                "E1000_DEV_ID_PCH_RPL_I219_LM23",
                E1000_DEV_ID_PCH_RPL_I219_LM23 as i64
            ),
            (
                "E1000_DEV_ID_PCH_RPL_I219_V23",
                E1000_DEV_ID_PCH_RPL_I219_V23 as i64
            ),
            (
                "E1000_DEV_ID_PCH_ARL_I219_LM24",
                E1000_DEV_ID_PCH_ARL_I219_LM24 as i64
            ),
            (
                "E1000_DEV_ID_PCH_ARL_I219_V24",
                E1000_DEV_ID_PCH_ARL_I219_V24 as i64
            ),
            (
                "E1000_DEV_ID_PCH_PTP_I219_LM25",
                E1000_DEV_ID_PCH_PTP_I219_LM25 as i64
            ),
            (
                "E1000_DEV_ID_PCH_PTP_I219_V25",
                E1000_DEV_ID_PCH_PTP_I219_V25 as i64
            ),
            (
                "E1000_DEV_ID_PCH_WCL_I219_LM27",
                E1000_DEV_ID_PCH_WCL_I219_LM27 as i64
            ),
            (
                "E1000_DEV_ID_PCH_WCL_I219_V27",
                E1000_DEV_ID_PCH_WCL_I219_V27 as i64
            ),
            ("E1000_DEV_ID_82575EB_PT", E1000_DEV_ID_82575EB_PT as i64),
            ("E1000_DEV_ID_82575EB_PF", E1000_DEV_ID_82575EB_PF as i64),
            ("E1000_DEV_ID_82575GB_QP", E1000_DEV_ID_82575GB_QP as i64),
            (
                "E1000_DEV_ID_82575GB_QP_PM",
                E1000_DEV_ID_82575GB_QP_PM as i64
            ),
            ("E1000_DEV_ID_82576", E1000_DEV_ID_82576 as i64),
            ("E1000_DEV_ID_82576_FIBER", E1000_DEV_ID_82576_FIBER as i64),
            (
                "E1000_DEV_ID_82576_SERDES",
                E1000_DEV_ID_82576_SERDES as i64
            ),
            (
                "E1000_DEV_ID_82576_QUAD_COPPER",
                E1000_DEV_ID_82576_QUAD_COPPER as i64
            ),
            ("E1000_DEV_ID_82576_NS", E1000_DEV_ID_82576_NS as i64),
            ("E1000_DEV_ID_82583V", E1000_DEV_ID_82583V as i64),
            (
                "E1000_DEV_ID_82576_NS_SERDES",
                E1000_DEV_ID_82576_NS_SERDES as i64
            ),
            (
                "E1000_DEV_ID_82576_SERDES_QUAD",
                E1000_DEV_ID_82576_SERDES_QUAD as i64
            ),
            (
                "E1000_DEV_ID_82580_COPPER",
                E1000_DEV_ID_82580_COPPER as i64
            ),
            ("E1000_DEV_ID_82580_FIBER", E1000_DEV_ID_82580_FIBER as i64),
            (
                "E1000_DEV_ID_82580_SERDES",
                E1000_DEV_ID_82580_SERDES as i64
            ),
            ("E1000_DEV_ID_82580_SGMII", E1000_DEV_ID_82580_SGMII as i64),
            (
                "E1000_DEV_ID_82580_COPPER_DUAL",
                E1000_DEV_ID_82580_COPPER_DUAL as i64
            ),
            (
                "E1000_DEV_ID_82580_QUAD_FIBER",
                E1000_DEV_ID_82580_QUAD_FIBER as i64
            ),
            (
                "E1000_DEV_ID_DH89XXCC_SGMII",
                E1000_DEV_ID_DH89XXCC_SGMII as i64
            ),
            (
                "E1000_DEV_ID_DH89XXCC_SERDES",
                E1000_DEV_ID_DH89XXCC_SERDES as i64
            ),
            (
                "E1000_DEV_ID_DH89XXCC_BACKPLANE",
                E1000_DEV_ID_DH89XXCC_BACKPLANE as i64
            ),
            (
                "E1000_DEV_ID_DH89XXCC_SFP",
                E1000_DEV_ID_DH89XXCC_SFP as i64
            ),
            ("E1000_DEV_ID_I350_COPPER", E1000_DEV_ID_I350_COPPER as i64),
            ("E1000_DEV_ID_I350_FIBER", E1000_DEV_ID_I350_FIBER as i64),
            ("E1000_DEV_ID_I350_SERDES", E1000_DEV_ID_I350_SERDES as i64),
            ("E1000_DEV_ID_I350_SGMII", E1000_DEV_ID_I350_SGMII as i64),
            (
                "E1000_DEV_ID_82576_QUAD_CU_ET2",
                E1000_DEV_ID_82576_QUAD_CU_ET2 as i64
            ),
            ("E1000_DEV_ID_I210_COPPER", E1000_DEV_ID_I210_COPPER as i64),
            (
                "E1000_DEV_ID_I210_COPPER_OEM1",
                E1000_DEV_ID_I210_COPPER_OEM1 as i64
            ),
            (
                "E1000_DEV_ID_I210_COPPER_IT",
                E1000_DEV_ID_I210_COPPER_IT as i64
            ),
            ("E1000_DEV_ID_I210_FIBER", E1000_DEV_ID_I210_FIBER as i64),
            ("E1000_DEV_ID_I210_SERDES", E1000_DEV_ID_I210_SERDES as i64),
            ("E1000_DEV_ID_I210_SGMII", E1000_DEV_ID_I210_SGMII as i64),
            (
                "E1000_DEV_ID_I210_COPPER_FLASHLESS",
                E1000_DEV_ID_I210_COPPER_FLASHLESS as i64
            ),
            (
                "E1000_DEV_ID_I210_SERDES_FLASHLESS",
                E1000_DEV_ID_I210_SERDES_FLASHLESS as i64
            ),
            ("E1000_DEV_ID_I211_COPPER", E1000_DEV_ID_I211_COPPER as i64),
            ("E1000_DEV_ID_I350_DA4", E1000_DEV_ID_I350_DA4 as i64),
            (
                "E1000_DEV_ID_I354_BACKPLANE_1GBPS",
                E1000_DEV_ID_I354_BACKPLANE_1GBPS as i64
            ),
            ("E1000_DEV_ID_I354_SGMII", E1000_DEV_ID_I354_SGMII as i64),
            (
                "E1000_DEV_ID_I354_BACKPLANE_2_5GBPS",
                E1000_DEV_ID_I354_BACKPLANE_2_5GBPS as i64
            ),
            (
                "E1000_DEV_ID_EP80579_LAN_1",
                E1000_DEV_ID_EP80579_LAN_1 as i64
            ),
            (
                "E1000_DEV_ID_EP80579_LAN_2",
                E1000_DEV_ID_EP80579_LAN_2 as i64
            ),
            (
                "E1000_DEV_ID_EP80579_LAN_3",
                E1000_DEV_ID_EP80579_LAN_3 as i64
            ),
            (
                "E1000_DEV_ID_EP80579_LAN_4",
                E1000_DEV_ID_EP80579_LAN_4 as i64
            ),
            (
                "E1000_DEV_ID_EP80579_LAN_5",
                E1000_DEV_ID_EP80579_LAN_5 as i64
            ),
            (
                "E1000_DEV_ID_EP80579_LAN_6",
                E1000_DEV_ID_EP80579_LAN_6 as i64
            ),
            ("NODE_ADDRESS_SIZE", NODE_ADDRESS_SIZE as i64),
            ("ETH_LENGTH_OF_ADDRESS", ETH_LENGTH_OF_ADDRESS as i64),
            ("MAC_DECODE_SIZE", MAC_DECODE_SIZE as i64),
            ("E1000_82542_2_0_REV_ID", E1000_82542_2_0_REV_ID as i64),
            ("E1000_82542_2_1_REV_ID", E1000_82542_2_1_REV_ID as i64),
            ("E1000_REVISION_0", E1000_REVISION_0 as i64),
            ("E1000_REVISION_1", E1000_REVISION_1 as i64),
            ("E1000_REVISION_2", E1000_REVISION_2 as i64),
            ("E1000_REVISION_3", E1000_REVISION_3 as i64),
            ("SPEED_10", SPEED_10 as i64),
            ("SPEED_100", SPEED_100 as i64),
            ("SPEED_1000", SPEED_1000 as i64),
            ("HALF_DUPLEX", HALF_DUPLEX as i64),
            ("FULL_DUPLEX", FULL_DUPLEX as i64),
            ("ENET_HEADER_SIZE", ENET_HEADER_SIZE as i64),
            (
                "MAXIMUM_ETHERNET_FRAME_SIZE",
                MAXIMUM_ETHERNET_FRAME_SIZE as i64
            ),
            (
                "MINIMUM_ETHERNET_FRAME_SIZE",
                MINIMUM_ETHERNET_FRAME_SIZE as i64
            ),
            ("ETHERNET_FCS_SIZE", ETHERNET_FCS_SIZE as i64),
            (
                "MAXIMUM_ETHERNET_PACKET_SIZE",
                MAXIMUM_ETHERNET_PACKET_SIZE as i64
            ),
            (
                "MINIMUM_ETHERNET_PACKET_SIZE",
                MINIMUM_ETHERNET_PACKET_SIZE as i64
            ),
            ("CRC_LENGTH", CRC_LENGTH as i64),
            ("MAX_JUMBO_FRAME_SIZE", MAX_JUMBO_FRAME_SIZE as i64),
            ("VLAN_TAG_SIZE", VLAN_TAG_SIZE as i64),
            ("ETHERNET_IEEE_VLAN_TYPE", ETHERNET_IEEE_VLAN_TYPE as i64),
            ("ETHERNET_IP_TYPE", ETHERNET_IP_TYPE as i64),
            ("ETHERNET_ARP_TYPE", ETHERNET_ARP_TYPE as i64),
            ("IP_PROTOCOL_TCP", IP_PROTOCOL_TCP as i64),
            ("IP_PROTOCOL_UDP", IP_PROTOCOL_UDP as i64),
            ("POLL_IMS_ENABLE_MASK", POLL_IMS_ENABLE_MASK as i64),
            ("IMS_ENABLE_MASK", IMS_ENABLE_MASK as i64),
            ("IMS_ICH8LAN_ENABLE_MASK", IMS_ICH8LAN_ENABLE_MASK as i64),
            ("E1000_RAR_ENTRIES", E1000_RAR_ENTRIES as i64),
            (
                "E1000_RAR_ENTRIES_ICH8LAN",
                E1000_RAR_ENTRIES_ICH8LAN as i64
            ),
            ("E1000_RAR_ENTRIES_82575", E1000_RAR_ENTRIES_82575 as i64),
            ("E1000_RAR_ENTRIES_82576", E1000_RAR_ENTRIES_82576 as i64),
            ("E1000_RAR_ENTRIES_82580", E1000_RAR_ENTRIES_82580 as i64),
            ("E1000_RAR_ENTRIES_I350", E1000_RAR_ENTRIES_I350 as i64),
            (
                "MIN_NUMBER_OF_DESCRIPTORS",
                MIN_NUMBER_OF_DESCRIPTORS as i64
            ),
            (
                "MAX_NUMBER_OF_DESCRIPTORS",
                MAX_NUMBER_OF_DESCRIPTORS as i64
            ),
            ("MAX_PS_BUFFERS", MAX_PS_BUFFERS as i64),
            ("E1000_RXD_STAT_DD", E1000_RXD_STAT_DD as i64),
            ("E1000_RXD_STAT_EOP", E1000_RXD_STAT_EOP as i64),
            ("E1000_RXD_STAT_IXSM", E1000_RXD_STAT_IXSM as i64),
            ("E1000_RXD_STAT_VP", E1000_RXD_STAT_VP as i64),
            ("E1000_RXD_STAT_UDPCS", E1000_RXD_STAT_UDPCS as i64),
            ("E1000_RXD_STAT_TCPCS", E1000_RXD_STAT_TCPCS as i64),
            ("E1000_RXD_STAT_IPCS", E1000_RXD_STAT_IPCS as i64),
            ("E1000_RXD_STAT_PIF", E1000_RXD_STAT_PIF as i64),
            ("E1000_RXD_STAT_IPIDV", E1000_RXD_STAT_IPIDV as i64),
            ("E1000_RXD_STAT_UDPV", E1000_RXD_STAT_UDPV as i64),
            ("E1000_RXD_STAT_ACK", E1000_RXD_STAT_ACK as i64),
            ("E1000_RXD_STAT_STRIPCRC", E1000_RXD_STAT_STRIPCRC as i64),
            ("E1000_RXD_ERR_CE", E1000_RXD_ERR_CE as i64),
            ("E1000_RXD_ERR_SE", E1000_RXD_ERR_SE as i64),
            ("E1000_RXD_ERR_SEQ", E1000_RXD_ERR_SEQ as i64),
            ("E1000_RXD_ERR_CXE", E1000_RXD_ERR_CXE as i64),
            ("E1000_RXD_ERR_TCPE", E1000_RXD_ERR_TCPE as i64),
            ("E1000_RXD_ERR_IPE", E1000_RXD_ERR_IPE as i64),
            ("E1000_RXD_ERR_RXE", E1000_RXD_ERR_RXE as i64),
            ("E1000_RXD_SPC_VLAN_MASK", E1000_RXD_SPC_VLAN_MASK as i64),
            ("E1000_RXD_SPC_PRI_MASK", E1000_RXD_SPC_PRI_MASK as i64),
            ("E1000_RXD_SPC_PRI_SHIFT", E1000_RXD_SPC_PRI_SHIFT as i64),
            ("E1000_RXD_SPC_CFI_MASK", E1000_RXD_SPC_CFI_MASK as i64),
            ("E1000_RXD_SPC_CFI_SHIFT", E1000_RXD_SPC_CFI_SHIFT as i64),
            ("E1000_RXDEXT_STATERR_CE", E1000_RXDEXT_STATERR_CE as i64),
            ("E1000_RXDEXT_STATERR_SE", E1000_RXDEXT_STATERR_SE as i64),
            ("E1000_RXDEXT_STATERR_SEQ", E1000_RXDEXT_STATERR_SEQ as i64),
            ("E1000_RXDEXT_STATERR_CXE", E1000_RXDEXT_STATERR_CXE as i64),
            (
                "E1000_RXDEXT_STATERR_TCPE",
                E1000_RXDEXT_STATERR_TCPE as i64
            ),
            ("E1000_RXDEXT_STATERR_IPE", E1000_RXDEXT_STATERR_IPE as i64),
            ("E1000_RXDEXT_STATERR_RXE", E1000_RXDEXT_STATERR_RXE as i64),
            (
                "E1000_RXDPS_HDRSTAT_HDRSP",
                E1000_RXDPS_HDRSTAT_HDRSP as i64
            ),
            (
                "E1000_RXDPS_HDRSTAT_HDRLEN_MASK",
                E1000_RXDPS_HDRSTAT_HDRLEN_MASK as i64
            ),
            (
                "E1000_RXD_ERR_FRAME_ERR_MASK",
                E1000_RXD_ERR_FRAME_ERR_MASK as i64
            ),
            (
                "E1000_RXDEXT_ERR_FRAME_ERR_MASK",
                E1000_RXDEXT_ERR_FRAME_ERR_MASK as i64
            ),
            ("E1000_TXD_DTYP_D", E1000_TXD_DTYP_D as i64),
            ("E1000_TXD_DTYP_C", E1000_TXD_DTYP_C as i64),
            ("E1000_TXD_POPTS_IXSM", E1000_TXD_POPTS_IXSM as i64),
            ("E1000_TXD_POPTS_TXSM", E1000_TXD_POPTS_TXSM as i64),
            ("E1000_TXD_CMD_EOP", E1000_TXD_CMD_EOP as i64),
            ("E1000_TXD_CMD_IFCS", E1000_TXD_CMD_IFCS as i64),
            ("E1000_TXD_CMD_IC", E1000_TXD_CMD_IC as i64),
            ("E1000_TXD_CMD_RS", E1000_TXD_CMD_RS as i64),
            ("E1000_TXD_CMD_RPS", E1000_TXD_CMD_RPS as i64),
            ("E1000_TXD_CMD_DEXT", E1000_TXD_CMD_DEXT as i64),
            ("E1000_TXD_CMD_VLE", E1000_TXD_CMD_VLE as i64),
            ("E1000_TXD_CMD_IDE", E1000_TXD_CMD_IDE as i64),
            ("E1000_TXD_STAT_DD", E1000_TXD_STAT_DD as i64),
            ("E1000_TXD_STAT_EC", E1000_TXD_STAT_EC as i64),
            ("E1000_TXD_STAT_LC", E1000_TXD_STAT_LC as i64),
            ("E1000_TXD_STAT_TU", E1000_TXD_STAT_TU as i64),
            ("E1000_TXD_CMD_TCP", E1000_TXD_CMD_TCP as i64),
            ("E1000_TXD_CMD_IP", E1000_TXD_CMD_IP as i64),
            ("E1000_TXD_CMD_TSE", E1000_TXD_CMD_TSE as i64),
            ("E1000_TXD_STAT_TC", E1000_TXD_STAT_TC as i64),
            ("E1000_NUM_UNICAST", E1000_NUM_UNICAST as i64),
            ("E1000_MC_TBL_SIZE", E1000_MC_TBL_SIZE as i64),
            (
                "E1000_VLAN_FILTER_TBL_SIZE",
                E1000_VLAN_FILTER_TBL_SIZE as i64
            ),
            (
                "E1000_NUM_UNICAST_ICH8LAN",
                E1000_NUM_UNICAST_ICH8LAN as i64
            ),
            (
                "E1000_MC_TBL_SIZE_ICH8LAN",
                E1000_MC_TBL_SIZE_ICH8LAN as i64
            ),
            ("E1000_NUM_MTA_REGISTERS", E1000_NUM_MTA_REGISTERS as i64),
            (
                "E1000_NUM_MTA_REGISTERS_ICH8LAN",
                E1000_NUM_MTA_REGISTERS_ICH8LAN as i64
            ),
            (
                "E1000_WAKEUP_IP_ADDRESS_COUNT_MAX",
                E1000_WAKEUP_IP_ADDRESS_COUNT_MAX as i64
            ),
            ("E1000_IP4AT_SIZE", E1000_IP4AT_SIZE as i64),
            ("E1000_IP4AT_SIZE_ICH8LAN", E1000_IP4AT_SIZE_ICH8LAN as i64),
            ("E1000_IP6AT_SIZE", E1000_IP6AT_SIZE as i64),
            (
                "E1000_FLEXIBLE_FILTER_COUNT_MAX",
                E1000_FLEXIBLE_FILTER_COUNT_MAX as i64
            ),
            (
                "E1000_FLEXIBLE_FILTER_SIZE_MAX",
                E1000_FLEXIBLE_FILTER_SIZE_MAX as i64
            ),
            ("E1000_FFLT_SIZE", E1000_FFLT_SIZE as i64),
            ("E1000_FFMT_SIZE", E1000_FFMT_SIZE as i64),
            ("E1000_FFVT_SIZE", E1000_FFVT_SIZE as i64),
            (
                "E1000_DISABLE_SERDES_LOOPBACK",
                E1000_DISABLE_SERDES_LOOPBACK as i64
            ),
            ("E1000_CTRL", E1000_CTRL as i64),
            ("E1000_CTRL_DUP", E1000_CTRL_DUP as i64),
            ("E1000_STATUS", E1000_STATUS as i64),
            ("E1000_EECD", E1000_EECD as i64),
            ("E1000_EERD", E1000_EERD as i64),
            ("E1000_CTRL_EXT", E1000_CTRL_EXT as i64),
            ("E1000_FLA", E1000_FLA as i64),
            ("E1000_MDIC", E1000_MDIC as i64),
            ("E1000_MDICNFG", E1000_MDICNFG as i64),
            ("E1000_SCTL", E1000_SCTL as i64),
            ("E1000_FEXTNVM", E1000_FEXTNVM as i64),
            ("E1000_FEXTNVM3", E1000_FEXTNVM3 as i64),
            ("E1000_FEXTNVM4", E1000_FEXTNVM4 as i64),
            ("E1000_FEXTNVM6", E1000_FEXTNVM6 as i64),
            ("E1000_FEXTNVM12", E1000_FEXTNVM12 as i64),
            ("E1000_FCAL", E1000_FCAL as i64),
            ("E1000_FCAH", E1000_FCAH as i64),
            ("E1000_FCT", E1000_FCT as i64),
            ("E1000_CONNSW", E1000_CONNSW as i64),
            ("E1000_VET", E1000_VET as i64),
            ("E1000_ICR", E1000_ICR as i64),
            ("E1000_ITR", E1000_ITR as i64),
            ("E1000_ICS", E1000_ICS as i64),
            ("E1000_IMS", E1000_IMS as i64),
            ("E1000_IMC", E1000_IMC as i64),
            ("E1000_IAM", E1000_IAM as i64),
            ("E1000_RCTL", E1000_RCTL as i64),
            ("E1000_GPIE", E1000_GPIE as i64),
            ("E1000_EICS", E1000_EICS as i64),
            ("E1000_EIMS", E1000_EIMS as i64),
            ("E1000_EIMC", E1000_EIMC as i64),
            ("E1000_EIAC", E1000_EIAC as i64),
            ("E1000_EIAM", E1000_EIAM as i64),
            ("E1000_EICR", E1000_EICR as i64),
            ("E1000_IVAR0", E1000_IVAR0 as i64),
            ("E1000_IVAR_MISC", E1000_IVAR_MISC as i64),
            ("E1000_RDTR1", E1000_RDTR1 as i64),
            ("E1000_RDBAL1", E1000_RDBAL1 as i64),
            ("E1000_RDBAH1", E1000_RDBAH1 as i64),
            ("E1000_RDLEN1", E1000_RDLEN1 as i64),
            ("E1000_FCTTV", E1000_FCTTV as i64),
            ("E1000_TXCW", E1000_TXCW as i64),
            ("E1000_RXCW", E1000_RXCW as i64),
            ("E1000_TCTL", E1000_TCTL as i64),
            ("E1000_TCTL_EXT", E1000_TCTL_EXT as i64),
            ("E1000_TIPG", E1000_TIPG as i64),
            ("E1000_TBT", E1000_TBT as i64),
            ("E1000_AIT", E1000_AIT as i64),
            ("E1000_LEDCTL", E1000_LEDCTL as i64),
            ("E1000_EXTCNF_CTRL", E1000_EXTCNF_CTRL as i64),
            ("E1000_EXTCNF_SIZE", E1000_EXTCNF_SIZE as i64),
            ("E1000_PHY_CTRL", E1000_PHY_CTRL as i64),
            ("FEXTNVM_SW_CONFIG", FEXTNVM_SW_CONFIG as i64),
            ("FEXTNVM_SW_CONFIG_ICH8M", FEXTNVM_SW_CONFIG_ICH8M as i64),
            ("E1000_PBA", E1000_PBA as i64),
            ("E1000_PBS", E1000_PBS as i64),
            ("E1000_IOSFPC", E1000_IOSFPC as i64),
            ("E1000_EEMNGCTL", E1000_EEMNGCTL as i64),
            ("E1000_FLASH_UPDATES", E1000_FLASH_UPDATES as i64),
            ("E1000_EEARBC", E1000_EEARBC as i64),
            ("E1000_FLASHT", E1000_FLASHT as i64),
            ("E1000_EEWR", E1000_EEWR as i64),
            ("E1000_FLSWCTL", E1000_FLSWCTL as i64),
            ("E1000_FLSWDATA", E1000_FLSWDATA as i64),
            ("E1000_FLSWCNT", E1000_FLSWCNT as i64),
            ("E1000_FLOP", E1000_FLOP as i64),
            ("E1000_I2CCMD", E1000_I2CCMD as i64),
            ("E1000_ERT", E1000_ERT as i64),
            ("E1000_FCRTL", E1000_FCRTL as i64),
            ("E1000_FCRTH", E1000_FCRTH as i64),
            ("E1000_PSRCTL", E1000_PSRCTL as i64),
            ("E1000_RDTR", E1000_RDTR as i64),
            ("E1000_RDTR0", E1000_RDTR0 as i64),
            ("E1000_RADV", E1000_RADV as i64),
            ("E1000_RSRPD", E1000_RSRPD as i64),
            ("E1000_RAID", E1000_RAID as i64),
            ("E1000_TXDMAC", E1000_TXDMAC as i64),
            ("E1000_KABGTXD", E1000_KABGTXD as i64),
            ("E1000_TDFH", E1000_TDFH as i64),
            ("E1000_TDFT", E1000_TDFT as i64),
            ("E1000_TDFHS", E1000_TDFHS as i64),
            ("E1000_TDFTS", E1000_TDFTS as i64),
            ("E1000_TDFPC", E1000_TDFPC as i64),
            ("E1000_TIDV", E1000_TIDV as i64),
            ("E1000_TADV", E1000_TADV as i64),
            ("E1000_TSPMT", E1000_TSPMT as i64),
            ("E1000_TARC0", E1000_TARC0 as i64),
            ("E1000_TDBAL1", E1000_TDBAL1 as i64),
            ("E1000_TDBAH1", E1000_TDBAH1 as i64),
            ("E1000_TDLEN1", E1000_TDLEN1 as i64),
            ("E1000_TDH1", E1000_TDH1 as i64),
            ("E1000_TDT1", E1000_TDT1 as i64),
            ("E1000_TARC1", E1000_TARC1 as i64),
            ("E1000_CRCERRS", E1000_CRCERRS as i64),
            ("E1000_ALGNERRC", E1000_ALGNERRC as i64),
            ("E1000_SYMERRS", E1000_SYMERRS as i64),
            ("E1000_RXERRC", E1000_RXERRC as i64),
            ("E1000_MPC", E1000_MPC as i64),
            ("E1000_SCC", E1000_SCC as i64),
            ("E1000_ECOL", E1000_ECOL as i64),
            ("E1000_MCC", E1000_MCC as i64),
            ("E1000_LATECOL", E1000_LATECOL as i64),
            ("E1000_COLC", E1000_COLC as i64),
            ("E1000_DC", E1000_DC as i64),
            ("E1000_TNCRS", E1000_TNCRS as i64),
            ("E1000_SEC", E1000_SEC as i64),
            ("E1000_CEXTERR", E1000_CEXTERR as i64),
            ("E1000_RLEC", E1000_RLEC as i64),
            ("E1000_XONRXC", E1000_XONRXC as i64),
            ("E1000_XONTXC", E1000_XONTXC as i64),
            ("E1000_XOFFRXC", E1000_XOFFRXC as i64),
            ("E1000_XOFFTXC", E1000_XOFFTXC as i64),
            ("E1000_FCRUC", E1000_FCRUC as i64),
            ("E1000_PRC64", E1000_PRC64 as i64),
            ("E1000_PRC127", E1000_PRC127 as i64),
            ("E1000_PRC255", E1000_PRC255 as i64),
            ("E1000_PRC511", E1000_PRC511 as i64),
            ("E1000_PRC1023", E1000_PRC1023 as i64),
            ("E1000_PRC1522", E1000_PRC1522 as i64),
            ("E1000_GPRC", E1000_GPRC as i64),
            ("E1000_BPRC", E1000_BPRC as i64),
            ("E1000_MPRC", E1000_MPRC as i64),
            ("E1000_GPTC", E1000_GPTC as i64),
            ("E1000_GORCL", E1000_GORCL as i64),
            ("E1000_GORCH", E1000_GORCH as i64),
            ("E1000_GOTCL", E1000_GOTCL as i64),
            ("E1000_GOTCH", E1000_GOTCH as i64),
            ("E1000_RNBC", E1000_RNBC as i64),
            ("E1000_RUC", E1000_RUC as i64),
            ("E1000_RFC", E1000_RFC as i64),
            ("E1000_ROC", E1000_ROC as i64),
            ("E1000_RJC", E1000_RJC as i64),
            ("E1000_MGTPRC", E1000_MGTPRC as i64),
            ("E1000_MGTPDC", E1000_MGTPDC as i64),
            ("E1000_MGTPTC", E1000_MGTPTC as i64),
            ("E1000_TORL", E1000_TORL as i64),
            ("E1000_TORH", E1000_TORH as i64),
            ("E1000_TOTL", E1000_TOTL as i64),
            ("E1000_TOTH", E1000_TOTH as i64),
            ("E1000_TPR", E1000_TPR as i64),
            ("E1000_TPT", E1000_TPT as i64),
            ("E1000_PTC64", E1000_PTC64 as i64),
            ("E1000_PTC127", E1000_PTC127 as i64),
            ("E1000_PTC255", E1000_PTC255 as i64),
            ("E1000_PTC511", E1000_PTC511 as i64),
            ("E1000_PTC1023", E1000_PTC1023 as i64),
            ("E1000_PTC1522", E1000_PTC1522 as i64),
            ("E1000_MPTC", E1000_MPTC as i64),
            ("E1000_BPTC", E1000_BPTC as i64),
            ("E1000_TSCTC", E1000_TSCTC as i64),
            ("E1000_TSCTFC", E1000_TSCTFC as i64),
            ("E1000_IAC", E1000_IAC as i64),
            ("E1000_RPTHC", E1000_RPTHC as i64),
            ("E1000_ICRXPTC", E1000_ICRXPTC as i64),
            ("E1000_ICRXATC", E1000_ICRXATC as i64),
            ("E1000_ICTXPTC", E1000_ICTXPTC as i64),
            ("E1000_ICTXATC", E1000_ICTXATC as i64),
            ("E1000_ICTXQEC", E1000_ICTXQEC as i64),
            ("E1000_ICTXQMTC", E1000_ICTXQMTC as i64),
            ("E1000_ICRXDMTC", E1000_ICRXDMTC as i64),
            ("E1000_ICRXOC", E1000_ICRXOC as i64),
            ("E1000_SDPC", E1000_SDPC as i64),
            ("E1000_PCS_CFG0", E1000_PCS_CFG0 as i64),
            ("E1000_PCS_LCTL", E1000_PCS_LCTL as i64),
            ("E1000_PCS_LSTAT", E1000_PCS_LSTAT as i64),
            ("E1000_RXCSUM", E1000_RXCSUM as i64),
            ("E1000_RFCTL", E1000_RFCTL as i64),
            ("E1000_MTA", E1000_MTA as i64),
            ("E1000_RA", E1000_RA as i64),
            ("E1000_VFTA", E1000_VFTA as i64),
            ("E1000_WUC", E1000_WUC as i64),
            ("E1000_WUFC", E1000_WUFC as i64),
            ("E1000_WUS", E1000_WUS as i64),
            ("E1000_MANC", E1000_MANC as i64),
            ("E1000_IPAV", E1000_IPAV as i64),
            ("E1000_IP4AT", E1000_IP4AT as i64),
            ("E1000_IP6AT", E1000_IP6AT as i64),
            ("E1000_WUPL", E1000_WUPL as i64),
            ("E1000_WUPM", E1000_WUPM as i64),
            ("E1000_FFLT", E1000_FFLT as i64),
            ("E1000_FCRTV_PCH", E1000_FCRTV_PCH as i64),
            ("E1000_CRC_OFFSET", E1000_CRC_OFFSET as i64),
            ("E1000_HOST_IF", E1000_HOST_IF as i64),
            ("E1000_FFMT", E1000_FFMT as i64),
            ("E1000_FFVT", E1000_FFVT as i64),
            ("E1000_KUMCTRLSTA", E1000_KUMCTRLSTA as i64),
            ("E1000_MDPHYA", E1000_MDPHYA as i64),
            ("E1000_MANC2H", E1000_MANC2H as i64),
            ("E1000_SW_FW_SYNC", E1000_SW_FW_SYNC as i64),
            ("E1000_GCR", E1000_GCR as i64),
            ("E1000_GSCL_1", E1000_GSCL_1 as i64),
            ("E1000_GSCL_2", E1000_GSCL_2 as i64),
            ("E1000_GSCL_3", E1000_GSCL_3 as i64),
            ("E1000_GSCL_4", E1000_GSCL_4 as i64),
            ("E1000_FACTPS", E1000_FACTPS as i64),
            ("E1000_SWSM", E1000_SWSM as i64),
            ("E1000_H2ME", E1000_H2ME as i64),
            ("E1000_FWSM", E1000_FWSM as i64),
            ("E1000_FFLT_DBG", E1000_FFLT_DBG as i64),
            ("E1000_HICR", E1000_HICR as i64),
            ("E1000_CPUVEC", E1000_CPUVEC as i64),
            ("E1000_MRQC", E1000_MRQC as i64),
            ("E1000_RSSIM", E1000_RSSIM as i64),
            ("E1000_RSSIR", E1000_RSSIR as i64),
            ("E1000_B2OSPC", E1000_B2OSPC as i64),
            ("E1000_B2OGPRC", E1000_B2OGPRC as i64),
            ("E1000_O2BGPTC", E1000_O2BGPTC as i64),
            ("E1000_O2BSPC", E1000_O2BSPC as i64),
            ("E1000_PHPM", E1000_PHPM as i64),
            ("E1000_PHPM_SPD_EN", E1000_PHPM_SPD_EN as i64),
            ("E1000_PHPM_D0LPLU", E1000_PHPM_D0LPLU as i64),
            ("E1000_PHPM_LPLU", E1000_PHPM_LPLU as i64),
            ("E1000_PHPM_DIS_1000_ND0", E1000_PHPM_DIS_1000_ND0 as i64),
            ("E1000_PHPM_LINK_ED", E1000_PHPM_LINK_ED as i64),
            ("E1000_PHPM_GOLINK_DISC", E1000_PHPM_GOLINK_DISC as i64),
            ("E1000_PHPM_DIS_1000", E1000_PHPM_DIS_1000 as i64),
            ("E1000_PHPM_SPD_B2B_EN", E1000_PHPM_SPD_B2B_EN as i64),
            ("E1000_PHPM_RST_COMPL", E1000_PHPM_RST_COMPL as i64),
            ("E1000_PHPM_DIS_100_ND0", E1000_PHPM_DIS_100_ND0 as i64),
            ("E1000_IPCNFG", E1000_IPCNFG as i64),
            ("E1000_LTRC", E1000_LTRC as i64),
            ("E1000_EEER", E1000_EEER as i64),
            ("E1000_EEE_SU", E1000_EEE_SU as i64),
            ("E1000_TLPIC", E1000_TLPIC as i64),
            ("E1000_RLPIC", E1000_RLPIC as i64),
            (
                "E1000_FEXTNVM3_PHY_CFG_COUNTER_MASK",
                E1000_FEXTNVM3_PHY_CFG_COUNTER_MASK as i64
            ),
            (
                "E1000_FEXTNVM3_PHY_CFG_COUNTER_50MSEC",
                E1000_FEXTNVM3_PHY_CFG_COUNTER_50MSEC as i64
            ),
            (
                "E1000_FEXTNVM4_BEACON_DURATION_MASK",
                E1000_FEXTNVM4_BEACON_DURATION_MASK as i64
            ),
            (
                "E1000_FEXTNVM4_BEACON_DURATION_8USEC",
                E1000_FEXTNVM4_BEACON_DURATION_8USEC as i64
            ),
            (
                "E1000_FEXTNVM4_BEACON_DURATION_16USEC",
                E1000_FEXTNVM4_BEACON_DURATION_16USEC as i64
            ),
            (
                "E1000_FEXTNVM6_REQ_PLL_CLK",
                E1000_FEXTNVM6_REQ_PLL_CLK as i64
            ),
            (
                "E1000_FEXTNVM6_ENABLE_K1_ENTRY_CONDITION",
                E1000_FEXTNVM6_ENABLE_K1_ENTRY_CONDITION as i64
            ),
            (
                "E1000_FEXTNVM12_PHYPD_CTRL_MASK",
                E1000_FEXTNVM12_PHYPD_CTRL_MASK as i64
            ),
            (
                "E1000_FEXTNVM12_PHYPD_CTRL_P1",
                E1000_FEXTNVM12_PHYPD_CTRL_P1 as i64
            ),
            ("E1000_EEPROM_SWDPIN0", E1000_EEPROM_SWDPIN0 as i64),
            ("E1000_EEPROM_LED_LOGIC", E1000_EEPROM_LED_LOGIC as i64),
            ("E1000_EEPROM_RW_REG_DATA", E1000_EEPROM_RW_REG_DATA as i64),
            ("E1000_EEPROM_RW_REG_DONE", E1000_EEPROM_RW_REG_DONE as i64),
            (
                "E1000_EEPROM_RW_REG_START",
                E1000_EEPROM_RW_REG_START as i64
            ),
            (
                "E1000_EEPROM_RW_ADDR_SHIFT",
                E1000_EEPROM_RW_ADDR_SHIFT as i64
            ),
            ("E1000_EEPROM_POLL_WRITE", E1000_EEPROM_POLL_WRITE as i64),
            ("E1000_EEPROM_POLL_READ", E1000_EEPROM_POLL_READ as i64),
            ("E1000_CTRL_FD", E1000_CTRL_FD as i64),
            ("E1000_CTRL_BEM", E1000_CTRL_BEM as i64),
            ("E1000_CTRL_PRIOR", E1000_CTRL_PRIOR as i64),
            (
                "E1000_CTRL_GIO_MASTER_DISABLE",
                E1000_CTRL_GIO_MASTER_DISABLE as i64
            ),
            ("E1000_CTRL_LRST", E1000_CTRL_LRST as i64),
            ("E1000_CTRL_TME", E1000_CTRL_TME as i64),
            ("E1000_CTRL_SLE", E1000_CTRL_SLE as i64),
            ("E1000_CTRL_ASDE", E1000_CTRL_ASDE as i64),
            ("E1000_CTRL_SLU", E1000_CTRL_SLU as i64),
            ("E1000_CTRL_ILOS", E1000_CTRL_ILOS as i64),
            ("E1000_CTRL_SPD_SEL", E1000_CTRL_SPD_SEL as i64),
            ("E1000_CTRL_SPD_10", E1000_CTRL_SPD_10 as i64),
            ("E1000_CTRL_SPD_100", E1000_CTRL_SPD_100 as i64),
            ("E1000_CTRL_SPD_1000", E1000_CTRL_SPD_1000 as i64),
            ("E1000_CTRL_BEM32", E1000_CTRL_BEM32 as i64),
            ("E1000_CTRL_FRCSPD", E1000_CTRL_FRCSPD as i64),
            ("E1000_CTRL_FRCDPX", E1000_CTRL_FRCDPX as i64),
            ("E1000_CTRL_D_UD_EN", E1000_CTRL_D_UD_EN as i64),
            ("E1000_CTRL_D_UD_POLARITY", E1000_CTRL_D_UD_POLARITY as i64),
            (
                "E1000_CTRL_FORCE_PHY_RESET",
                E1000_CTRL_FORCE_PHY_RESET as i64
            ),
            (
                "E1000_CTRL_LANPHYPC_OVERRIDE",
                E1000_CTRL_LANPHYPC_OVERRIDE as i64
            ),
            (
                "E1000_CTRL_LANPHYPC_VALUE",
                E1000_CTRL_LANPHYPC_VALUE as i64
            ),
            ("E1000_CTRL_EXT_DPG_EN", E1000_CTRL_EXT_DPG_EN as i64),
            (
                "E1000_CTRL_EXT_FORCE_SMBUS",
                E1000_CTRL_EXT_FORCE_SMBUS as i64
            ),
            ("E1000_CTRL_EXT_PHYPDEN", E1000_CTRL_EXT_PHYPDEN as i64),
            (
                "E1000_I2CCMD_REG_ADDR_SHIFT",
                E1000_I2CCMD_REG_ADDR_SHIFT as i64
            ),
            (
                "E1000_I2CCMD_PHY_ADDR_SHIFT",
                E1000_I2CCMD_PHY_ADDR_SHIFT as i64
            ),
            ("E1000_I2CCMD_OPCODE_READ", E1000_I2CCMD_OPCODE_READ as i64),
            (
                "E1000_I2CCMD_OPCODE_WRITE",
                E1000_I2CCMD_OPCODE_WRITE as i64
            ),
            ("E1000_I2CCMD_READY", E1000_I2CCMD_READY as i64),
            ("E1000_I2CCMD_ERROR", E1000_I2CCMD_ERROR as i64),
            (
                "E1000_MAX_SGMII_PHY_REG_ADDR",
                E1000_MAX_SGMII_PHY_REG_ADDR as i64
            ),
            ("E1000_I2CCMD_PHY_TIMEOUT", E1000_I2CCMD_PHY_TIMEOUT as i64),
            ("E1000_CTRL_SWDPIN0", E1000_CTRL_SWDPIN0 as i64),
            ("E1000_CTRL_SWDPIN1", E1000_CTRL_SWDPIN1 as i64),
            ("E1000_CTRL_SWDPIN2", E1000_CTRL_SWDPIN2 as i64),
            ("E1000_CTRL_SWDPIN3", E1000_CTRL_SWDPIN3 as i64),
            ("E1000_CTRL_SWDPIO0", E1000_CTRL_SWDPIO0 as i64),
            ("E1000_CTRL_SWDPIO1", E1000_CTRL_SWDPIO1 as i64),
            ("E1000_CTRL_SWDPIO2", E1000_CTRL_SWDPIO2 as i64),
            ("E1000_CTRL_SWDPIO3", E1000_CTRL_SWDPIO3 as i64),
            ("E1000_CTRL_RST", E1000_CTRL_RST as i64),
            ("E1000_CTRL_RFCE", E1000_CTRL_RFCE as i64),
            ("E1000_CTRL_TFCE", E1000_CTRL_TFCE as i64),
            ("E1000_CTRL_RTE", E1000_CTRL_RTE as i64),
            ("E1000_CTRL_DEV_RST", E1000_CTRL_DEV_RST as i64),
            ("E1000_CTRL_VME", E1000_CTRL_VME as i64),
            ("E1000_CTRL_PHY_RST", E1000_CTRL_PHY_RST as i64),
            ("E1000_CTRL_SW2FW_INT", E1000_CTRL_SW2FW_INT as i64),
            ("E1000_CTRL_I2C_ENA", E1000_CTRL_I2C_ENA as i64),
            ("E1000_CONNSW_ENRGSRC", E1000_CONNSW_ENRGSRC as i64),
            ("E1000_PCS_CFG_PCS_EN", E1000_PCS_CFG_PCS_EN as i64),
            ("E1000_PCS_LCTL_FSV_1000", E1000_PCS_LCTL_FSV_1000 as i64),
            ("E1000_PCS_LCTL_FDV_FULL", E1000_PCS_LCTL_FDV_FULL as i64),
            ("E1000_PCS_LCTL_FSD", E1000_PCS_LCTL_FSD as i64),
            (
                "E1000_PCS_LCTL_FORCE_FCTRL",
                E1000_PCS_LCTL_FORCE_FCTRL as i64
            ),
            ("E1000_PCS_LSTS_LINK_OK", E1000_PCS_LSTS_LINK_OK as i64),
            ("E1000_PCS_LSTS_SPEED_100", E1000_PCS_LSTS_SPEED_100 as i64),
            (
                "E1000_PCS_LSTS_SPEED_1000",
                E1000_PCS_LSTS_SPEED_1000 as i64
            ),
            (
                "E1000_PCS_LSTS_DUPLEX_FULL",
                E1000_PCS_LSTS_DUPLEX_FULL as i64
            ),
            ("E1000_PCS_LSTS_SYNK_OK", E1000_PCS_LSTS_SYNK_OK as i64),
            ("E1000_STATUS_FD", E1000_STATUS_FD as i64),
            ("E1000_STATUS_LU", E1000_STATUS_LU as i64),
            ("E1000_STATUS_FUNC_MASK", E1000_STATUS_FUNC_MASK as i64),
            ("E1000_STATUS_FUNC_SHIFT", E1000_STATUS_FUNC_SHIFT as i64),
            ("E1000_STATUS_FUNC_0", E1000_STATUS_FUNC_0 as i64),
            ("E1000_STATUS_FUNC_1", E1000_STATUS_FUNC_1 as i64),
            ("E1000_STATUS_TXOFF", E1000_STATUS_TXOFF as i64),
            ("E1000_STATUS_TBIMODE", E1000_STATUS_TBIMODE as i64),
            ("E1000_STATUS_SPEED_MASK", E1000_STATUS_SPEED_MASK as i64),
            ("E1000_STATUS_SPEED_10", E1000_STATUS_SPEED_10 as i64),
            ("E1000_STATUS_SPEED_100", E1000_STATUS_SPEED_100 as i64),
            ("E1000_STATUS_SPEED_1000", E1000_STATUS_SPEED_1000 as i64),
            (
                "E1000_STATUS_LAN_INIT_DONE",
                E1000_STATUS_LAN_INIT_DONE as i64
            ),
            ("E1000_STATUS_ASDV", E1000_STATUS_ASDV as i64),
            ("E1000_STATUS_DOCK_CI", E1000_STATUS_DOCK_CI as i64),
            (
                "E1000_STATUS_GIO_MASTER_ENABLE",
                E1000_STATUS_GIO_MASTER_ENABLE as i64
            ),
            ("E1000_STATUS_MTXCKOK", E1000_STATUS_MTXCKOK as i64),
            ("E1000_STATUS_PCI66", E1000_STATUS_PCI66 as i64),
            ("E1000_STATUS_BUS64", E1000_STATUS_BUS64 as i64),
            ("E1000_STATUS_PCIX_MODE", E1000_STATUS_PCIX_MODE as i64),
            ("E1000_STATUS_PCIX_SPEED", E1000_STATUS_PCIX_SPEED as i64),
            ("E1000_STATUS_BMC_SKU_0", E1000_STATUS_BMC_SKU_0 as i64),
            ("E1000_STATUS_DEV_RST_SET", E1000_STATUS_DEV_RST_SET as i64),
            ("E1000_STATUS_BMC_SKU_1", E1000_STATUS_BMC_SKU_1 as i64),
            ("E1000_STATUS_BMC_SKU_2", E1000_STATUS_BMC_SKU_2 as i64),
            ("E1000_STATUS_BMC_CRYPTO", E1000_STATUS_BMC_CRYPTO as i64),
            ("E1000_STATUS_BMC_LITE", E1000_STATUS_BMC_LITE as i64),
            (
                "E1000_STATUS_RGMII_ENABLE",
                E1000_STATUS_RGMII_ENABLE as i64
            ),
            ("E1000_STATUS_FUSE_8", E1000_STATUS_FUSE_8 as i64),
            ("E1000_STATUS_FUSE_9", E1000_STATUS_FUSE_9 as i64),
            ("E1000_STATUS_SERDES0_DIS", E1000_STATUS_SERDES0_DIS as i64),
            ("E1000_STATUS_SERDES1_DIS", E1000_STATUS_SERDES1_DIS as i64),
            (
                "E1000_STATUS_PCIX_SPEED_66",
                E1000_STATUS_PCIX_SPEED_66 as i64
            ),
            (
                "E1000_STATUS_PCIX_SPEED_100",
                E1000_STATUS_PCIX_SPEED_100 as i64
            ),
            (
                "E1000_STATUS_PCIX_SPEED_133",
                E1000_STATUS_PCIX_SPEED_133 as i64
            ),
            ("E1000_EECD_SK", E1000_EECD_SK as i64),
            ("E1000_EECD_CS", E1000_EECD_CS as i64),
            ("E1000_EECD_DI", E1000_EECD_DI as i64),
            ("E1000_EECD_DO", E1000_EECD_DO as i64),
            ("E1000_EECD_FWE_MASK", E1000_EECD_FWE_MASK as i64),
            ("E1000_EECD_FWE_DIS", E1000_EECD_FWE_DIS as i64),
            ("E1000_EECD_FWE_EN", E1000_EECD_FWE_EN as i64),
            ("E1000_EECD_FWE_SHIFT", E1000_EECD_FWE_SHIFT as i64),
            ("E1000_EECD_REQ", E1000_EECD_REQ as i64),
            ("E1000_EECD_GNT", E1000_EECD_GNT as i64),
            ("E1000_EECD_PRES", E1000_EECD_PRES as i64),
            ("E1000_EECD_SIZE", E1000_EECD_SIZE as i64),
            ("E1000_EECD_ADDR_BITS", E1000_EECD_ADDR_BITS as i64),
            ("E1000_EECD_TYPE", E1000_EECD_TYPE as i64),
            (
                "E1000_EEPROM_GRANT_ATTEMPTS",
                E1000_EEPROM_GRANT_ATTEMPTS as i64
            ),
            ("E1000_EECD_AUTO_RD", E1000_EECD_AUTO_RD as i64),
            ("E1000_EECD_SIZE_EX_MASK", E1000_EECD_SIZE_EX_MASK as i64),
            ("E1000_EECD_SIZE_EX_SHIFT", E1000_EECD_SIZE_EX_SHIFT as i64),
            ("E1000_EECD_NVADDS", E1000_EECD_NVADDS as i64),
            ("E1000_EECD_SELSHAD", E1000_EECD_SELSHAD as i64),
            ("E1000_EECD_INITSRAM", E1000_EECD_INITSRAM as i64),
            ("E1000_EECD_FLUPD", E1000_EECD_FLUPD as i64),
            ("E1000_EECD_AUPDEN", E1000_EECD_AUPDEN as i64),
            ("E1000_EECD_SHADV", E1000_EECD_SHADV as i64),
            ("E1000_EECD_SEC1VAL", E1000_EECD_SEC1VAL as i64),
            (
                "E1000_EECD_SEC1VAL_VALID_MASK",
                E1000_EECD_SEC1VAL_VALID_MASK as i64
            ),
            ("E1000_EECD_SECVAL_SHIFT", E1000_EECD_SECVAL_SHIFT as i64),
            ("E1000_STM_OPCODE", E1000_STM_OPCODE as i64),
            ("E1000_HICR_FW_RESET", E1000_HICR_FW_RESET as i64),
            ("E1000_SHADOW_RAM_WORDS", E1000_SHADOW_RAM_WORDS as i64),
            ("E1000_ICH_NVM_SIG_WORD", E1000_ICH_NVM_SIG_WORD as i64),
            ("E1000_ICH_NVM_SIG_MASK", E1000_ICH_NVM_SIG_MASK as i64),
            (
                "E1000_ICH_NVM_VALID_SIG_MASK",
                E1000_ICH_NVM_VALID_SIG_MASK as i64
            ),
            ("E1000_ICH_NVM_SIG_VALUE", E1000_ICH_NVM_SIG_VALUE as i64),
            ("E1000_EERD_START", E1000_EERD_START as i64),
            ("E1000_EERD_DONE", E1000_EERD_DONE as i64),
            ("E1000_EERD_ADDR_SHIFT", E1000_EERD_ADDR_SHIFT as i64),
            ("E1000_EERD_ADDR_MASK", E1000_EERD_ADDR_MASK as i64),
            ("E1000_EERD_DATA_SHIFT", E1000_EERD_DATA_SHIFT as i64),
            ("E1000_EERD_DATA_MASK", E1000_EERD_DATA_MASK as i64),
            ("EEPROM_STATUS_RDY_SPI", EEPROM_STATUS_RDY_SPI as i64),
            ("EEPROM_STATUS_WEN_SPI", EEPROM_STATUS_WEN_SPI as i64),
            ("EEPROM_STATUS_BP0_SPI", EEPROM_STATUS_BP0_SPI as i64),
            ("EEPROM_STATUS_BP1_SPI", EEPROM_STATUS_BP1_SPI as i64),
            ("EEPROM_STATUS_WPEN_SPI", EEPROM_STATUS_WPEN_SPI as i64),
            ("E1000_CTRL_EXT_GPI0_EN", E1000_CTRL_EXT_GPI0_EN as i64),
            ("E1000_CTRL_EXT_GPI1_EN", E1000_CTRL_EXT_GPI1_EN as i64),
            ("E1000_CTRL_EXT_PHYINT_EN", E1000_CTRL_EXT_PHYINT_EN as i64),
            ("E1000_CTRL_EXT_GPI2_EN", E1000_CTRL_EXT_GPI2_EN as i64),
            ("E1000_CTRL_EXT_LPCD", E1000_CTRL_EXT_LPCD as i64),
            ("E1000_CTRL_EXT_GPI3_EN", E1000_CTRL_EXT_GPI3_EN as i64),
            ("E1000_CTRL_EXT_SDP4_DATA", E1000_CTRL_EXT_SDP4_DATA as i64),
            ("E1000_CTRL_EXT_SDP5_DATA", E1000_CTRL_EXT_SDP5_DATA as i64),
            ("E1000_CTRL_EXT_PHY_INT", E1000_CTRL_EXT_PHY_INT as i64),
            ("E1000_CTRL_EXT_SDP6_DATA", E1000_CTRL_EXT_SDP6_DATA as i64),
            ("E1000_CTRL_EXT_SDP7_DATA", E1000_CTRL_EXT_SDP7_DATA as i64),
            ("E1000_CTRL_EXT_SDP3_DATA", E1000_CTRL_EXT_SDP3_DATA as i64),
            ("E1000_CTRL_EXT_SDP4_DIR", E1000_CTRL_EXT_SDP4_DIR as i64),
            ("E1000_CTRL_EXT_SDP5_DIR", E1000_CTRL_EXT_SDP5_DIR as i64),
            ("E1000_CTRL_EXT_SDP6_DIR", E1000_CTRL_EXT_SDP6_DIR as i64),
            ("E1000_CTRL_EXT_SDP7_DIR", E1000_CTRL_EXT_SDP7_DIR as i64),
            ("E1000_CTRL_EXT_ASDCHK", E1000_CTRL_EXT_ASDCHK as i64),
            ("E1000_CTRL_EXT_EE_RST", E1000_CTRL_EXT_EE_RST as i64),
            ("E1000_CTRL_EXT_IPS", E1000_CTRL_EXT_IPS as i64),
            ("E1000_CTRL_EXT_SPD_BYPS", E1000_CTRL_EXT_SPD_BYPS as i64),
            ("E1000_CTRL_EXT_RO_DIS", E1000_CTRL_EXT_RO_DIS as i64),
            (
                "E1000_CTRL_EXT_LINK_MODE_MASK",
                E1000_CTRL_EXT_LINK_MODE_MASK as i64
            ),
            (
                "E1000_CTRL_EXT_LINK_MODE_GMII",
                E1000_CTRL_EXT_LINK_MODE_GMII as i64
            ),
            (
                "E1000_CTRL_EXT_LINK_MODE_TBI",
                E1000_CTRL_EXT_LINK_MODE_TBI as i64
            ),
            (
                "E1000_CTRL_EXT_LINK_MODE_KMRN",
                E1000_CTRL_EXT_LINK_MODE_KMRN as i64
            ),
            (
                "E1000_CTRL_EXT_LINK_MODE_PCIE_SERDES",
                E1000_CTRL_EXT_LINK_MODE_PCIE_SERDES as i64
            ),
            (
                "E1000_CTRL_EXT_LINK_MODE_1000BASE_KX",
                E1000_CTRL_EXT_LINK_MODE_1000BASE_KX as i64
            ),
            (
                "E1000_CTRL_EXT_LINK_MODE_SGMII",
                E1000_CTRL_EXT_LINK_MODE_SGMII as i64
            ),
            (
                "E1000_CTRL_EXT_WR_WMARK_MASK",
                E1000_CTRL_EXT_WR_WMARK_MASK as i64
            ),
            (
                "E1000_CTRL_EXT_WR_WMARK_256",
                E1000_CTRL_EXT_WR_WMARK_256 as i64
            ),
            (
                "E1000_CTRL_EXT_WR_WMARK_320",
                E1000_CTRL_EXT_WR_WMARK_320 as i64
            ),
            (
                "E1000_CTRL_EXT_WR_WMARK_384",
                E1000_CTRL_EXT_WR_WMARK_384 as i64
            ),
            (
                "E1000_CTRL_EXT_WR_WMARK_448",
                E1000_CTRL_EXT_WR_WMARK_448 as i64
            ),
            ("E1000_CTRL_EXT_EXT_VLAN", E1000_CTRL_EXT_EXT_VLAN as i64),
            ("E1000_CTRL_EXT_DRV_LOAD", E1000_CTRL_EXT_DRV_LOAD as i64),
            ("E1000_CTRL_EXT_IAME", E1000_CTRL_EXT_IAME as i64),
            (
                "E1000_CTRL_EXT_INT_TIMER_CLR",
                E1000_CTRL_EXT_INT_TIMER_CLR as i64
            ),
            ("E1000_CRTL_EXT_PB_PAREN", E1000_CRTL_EXT_PB_PAREN as i64),
            ("E1000_CTRL_EXT_DF_PAREN", E1000_CTRL_EXT_DF_PAREN as i64),
            (
                "E1000_CTRL_EXT_GHOST_PAREN",
                E1000_CTRL_EXT_GHOST_PAREN as i64
            ),
            ("E1000_MDIC_DATA_MASK", E1000_MDIC_DATA_MASK as i64),
            ("E1000_MDIC_REG_MASK", E1000_MDIC_REG_MASK as i64),
            ("E1000_MDIC_REG_SHIFT", E1000_MDIC_REG_SHIFT as i64),
            ("E1000_MDIC_PHY_MASK", E1000_MDIC_PHY_MASK as i64),
            ("E1000_MDIC_PHY_SHIFT", E1000_MDIC_PHY_SHIFT as i64),
            ("E1000_MDIC_OP_WRITE", E1000_MDIC_OP_WRITE as i64),
            ("E1000_MDIC_OP_READ", E1000_MDIC_OP_READ as i64),
            ("E1000_MDIC_READY", E1000_MDIC_READY as i64),
            ("E1000_MDIC_INT_EN", E1000_MDIC_INT_EN as i64),
            ("E1000_MDIC_ERROR", E1000_MDIC_ERROR as i64),
            ("E1000_MDIC_DEST", E1000_MDIC_DEST as i64),
            ("E1000_KUMCTRLSTA_MASK", E1000_KUMCTRLSTA_MASK as i64),
            ("E1000_KUMCTRLSTA_OFFSET", E1000_KUMCTRLSTA_OFFSET as i64),
            (
                "E1000_KUMCTRLSTA_OFFSET_SHIFT",
                E1000_KUMCTRLSTA_OFFSET_SHIFT as i64
            ),
            ("E1000_KUMCTRLSTA_REN", E1000_KUMCTRLSTA_REN as i64),
            (
                "E1000_KUMCTRLSTA_OFFSET_FIFO_CTRL",
                E1000_KUMCTRLSTA_OFFSET_FIFO_CTRL as i64
            ),
            (
                "E1000_KUMCTRLSTA_OFFSET_CTRL",
                E1000_KUMCTRLSTA_OFFSET_CTRL as i64
            ),
            (
                "E1000_KUMCTRLSTA_OFFSET_INB_CTRL",
                E1000_KUMCTRLSTA_OFFSET_INB_CTRL as i64
            ),
            (
                "E1000_KUMCTRLSTA_OFFSET_DIAG",
                E1000_KUMCTRLSTA_OFFSET_DIAG as i64
            ),
            (
                "E1000_KUMCTRLSTA_OFFSET_TIMEOUTS",
                E1000_KUMCTRLSTA_OFFSET_TIMEOUTS as i64
            ),
            (
                "E1000_KUMCTRLSTA_OFFSET_INB_PARAM",
                E1000_KUMCTRLSTA_OFFSET_INB_PARAM as i64
            ),
            (
                "E1000_KUMCTRLSTA_OFFSET_HD_CTRL",
                E1000_KUMCTRLSTA_OFFSET_HD_CTRL as i64
            ),
            (
                "E1000_KUMCTRLSTA_OFFSET_M2P_SERDES",
                E1000_KUMCTRLSTA_OFFSET_M2P_SERDES as i64
            ),
            (
                "E1000_KUMCTRLSTA_OFFSET_M2P_MODES",
                E1000_KUMCTRLSTA_OFFSET_M2P_MODES as i64
            ),
            (
                "E1000_KUMCTRLSTA_FIFO_CTRL_RX_BYPASS",
                E1000_KUMCTRLSTA_FIFO_CTRL_RX_BYPASS as i64
            ),
            (
                "E1000_KUMCTRLSTA_FIFO_CTRL_TX_BYPASS",
                E1000_KUMCTRLSTA_FIFO_CTRL_TX_BYPASS as i64
            ),
            (
                "E1000_KUMCTRLSTA_INB_CTRL_LINK_STATUS_TX_TIMEOUT_DEFAULT",
                E1000_KUMCTRLSTA_INB_CTRL_LINK_STATUS_TX_TIMEOUT_DEFAULT as i64
            ),
            (
                "E1000_KUMCTRLSTA_INB_CTRL_DIS_PADDING",
                E1000_KUMCTRLSTA_INB_CTRL_DIS_PADDING as i64
            ),
            (
                "E1000_KUMCTRLSTA_HD_CTRL_10_100_DEFAULT",
                E1000_KUMCTRLSTA_HD_CTRL_10_100_DEFAULT as i64
            ),
            (
                "E1000_KUMCTRLSTA_HD_CTRL_1000_DEFAULT",
                E1000_KUMCTRLSTA_HD_CTRL_1000_DEFAULT as i64
            ),
            (
                "E1000_KUMCTRLSTA_OFFSET_K0S_CTRL",
                E1000_KUMCTRLSTA_OFFSET_K0S_CTRL as i64
            ),
            (
                "E1000_KUMCTRLSTA_DIAG_FELPBK",
                E1000_KUMCTRLSTA_DIAG_FELPBK as i64
            ),
            (
                "E1000_KUMCTRLSTA_DIAG_NELPBK",
                E1000_KUMCTRLSTA_DIAG_NELPBK as i64
            ),
            (
                "E1000_KUMCTRLSTA_K0S_100_EN",
                E1000_KUMCTRLSTA_K0S_100_EN as i64
            ),
            (
                "E1000_KUMCTRLSTA_K0S_GBE_EN",
                E1000_KUMCTRLSTA_K0S_GBE_EN as i64
            ),
            (
                "E1000_KUMCTRLSTA_K0S_ENTRY_LATENCY_MASK",
                E1000_KUMCTRLSTA_K0S_ENTRY_LATENCY_MASK as i64
            ),
            ("E1000_KABGTXD_BGSQLBIAS", E1000_KABGTXD_BGSQLBIAS as i64),
            ("E1000_PHY_CTRL_SPD_EN", E1000_PHY_CTRL_SPD_EN as i64),
            ("E1000_PHY_CTRL_D0A_LPLU", E1000_PHY_CTRL_D0A_LPLU as i64),
            (
                "E1000_PHY_CTRL_NOND0A_LPLU",
                E1000_PHY_CTRL_NOND0A_LPLU as i64
            ),
            (
                "E1000_PHY_CTRL_NOND0A_GBE_DISABLE",
                E1000_PHY_CTRL_NOND0A_GBE_DISABLE as i64
            ),
            (
                "E1000_PHY_CTRL_GBE_DISABLE",
                E1000_PHY_CTRL_GBE_DISABLE as i64
            ),
            ("E1000_PHY_CTRL_B2B_EN", E1000_PHY_CTRL_B2B_EN as i64),
            ("E1000_PHY_CTRL_LOOPBACK", E1000_PHY_CTRL_LOOPBACK as i64),
            (
                "E1000_LEDCTL_LED0_MODE_MASK",
                E1000_LEDCTL_LED0_MODE_MASK as i64
            ),
            (
                "E1000_LEDCTL_LED0_MODE_SHIFT",
                E1000_LEDCTL_LED0_MODE_SHIFT as i64
            ),
            (
                "E1000_LEDCTL_LED0_BLINK_RATE",
                E1000_LEDCTL_LED0_BLINK_RATE as i64
            ),
            ("E1000_LEDCTL_LED0_IVRT", E1000_LEDCTL_LED0_IVRT as i64),
            ("E1000_LEDCTL_LED0_BLINK", E1000_LEDCTL_LED0_BLINK as i64),
            (
                "E1000_LEDCTL_LED1_MODE_MASK",
                E1000_LEDCTL_LED1_MODE_MASK as i64
            ),
            (
                "E1000_LEDCTL_LED1_MODE_SHIFT",
                E1000_LEDCTL_LED1_MODE_SHIFT as i64
            ),
            (
                "E1000_LEDCTL_LED1_BLINK_RATE",
                E1000_LEDCTL_LED1_BLINK_RATE as i64
            ),
            ("E1000_LEDCTL_LED1_IVRT", E1000_LEDCTL_LED1_IVRT as i64),
            ("E1000_LEDCTL_LED1_BLINK", E1000_LEDCTL_LED1_BLINK as i64),
            (
                "E1000_LEDCTL_LED2_MODE_MASK",
                E1000_LEDCTL_LED2_MODE_MASK as i64
            ),
            (
                "E1000_LEDCTL_LED2_MODE_SHIFT",
                E1000_LEDCTL_LED2_MODE_SHIFT as i64
            ),
            (
                "E1000_LEDCTL_LED2_BLINK_RATE",
                E1000_LEDCTL_LED2_BLINK_RATE as i64
            ),
            ("E1000_LEDCTL_LED2_IVRT", E1000_LEDCTL_LED2_IVRT as i64),
            ("E1000_LEDCTL_LED2_BLINK", E1000_LEDCTL_LED2_BLINK as i64),
            (
                "E1000_LEDCTL_LED3_MODE_MASK",
                E1000_LEDCTL_LED3_MODE_MASK as i64
            ),
            (
                "E1000_LEDCTL_LED3_MODE_SHIFT",
                E1000_LEDCTL_LED3_MODE_SHIFT as i64
            ),
            (
                "E1000_LEDCTL_LED3_BLINK_RATE",
                E1000_LEDCTL_LED3_BLINK_RATE as i64
            ),
            ("E1000_LEDCTL_LED3_IVRT", E1000_LEDCTL_LED3_IVRT as i64),
            ("E1000_LEDCTL_LED3_BLINK", E1000_LEDCTL_LED3_BLINK as i64),
            (
                "E1000_LEDCTL_MODE_LINK_10_1000",
                E1000_LEDCTL_MODE_LINK_10_1000 as i64
            ),
            (
                "E1000_LEDCTL_MODE_LINK_100_1000",
                E1000_LEDCTL_MODE_LINK_100_1000 as i64
            ),
            (
                "E1000_LEDCTL_MODE_LINK_UP",
                E1000_LEDCTL_MODE_LINK_UP as i64
            ),
            (
                "E1000_LEDCTL_MODE_ACTIVITY",
                E1000_LEDCTL_MODE_ACTIVITY as i64
            ),
            (
                "E1000_LEDCTL_MODE_LINK_ACTIVITY",
                E1000_LEDCTL_MODE_LINK_ACTIVITY as i64
            ),
            (
                "E1000_LEDCTL_MODE_LINK_10",
                E1000_LEDCTL_MODE_LINK_10 as i64
            ),
            (
                "E1000_LEDCTL_MODE_LINK_100",
                E1000_LEDCTL_MODE_LINK_100 as i64
            ),
            (
                "E1000_LEDCTL_MODE_LINK_1000",
                E1000_LEDCTL_MODE_LINK_1000 as i64
            ),
            (
                "E1000_LEDCTL_MODE_PCIX_MODE",
                E1000_LEDCTL_MODE_PCIX_MODE as i64
            ),
            (
                "E1000_LEDCTL_MODE_FULL_DUPLEX",
                E1000_LEDCTL_MODE_FULL_DUPLEX as i64
            ),
            (
                "E1000_LEDCTL_MODE_COLLISION",
                E1000_LEDCTL_MODE_COLLISION as i64
            ),
            (
                "E1000_LEDCTL_MODE_BUS_SPEED",
                E1000_LEDCTL_MODE_BUS_SPEED as i64
            ),
            (
                "E1000_LEDCTL_MODE_BUS_SIZE",
                E1000_LEDCTL_MODE_BUS_SIZE as i64
            ),
            ("E1000_LEDCTL_MODE_PAUSED", E1000_LEDCTL_MODE_PAUSED as i64),
            ("E1000_LEDCTL_MODE_LED_ON", E1000_LEDCTL_MODE_LED_ON as i64),
            (
                "E1000_LEDCTL_MODE_LED_OFF",
                E1000_LEDCTL_MODE_LED_OFF as i64
            ),
            ("E1000_RAH_AV", E1000_RAH_AV as i64),
            ("E1000_ICR_TXDW", E1000_ICR_TXDW as i64),
            ("E1000_ICR_TXQE", E1000_ICR_TXQE as i64),
            ("E1000_ICR_LSC", E1000_ICR_LSC as i64),
            ("E1000_ICR_RXSEQ", E1000_ICR_RXSEQ as i64),
            ("E1000_ICR_RXDMT0", E1000_ICR_RXDMT0 as i64),
            ("E1000_ICR_RXO", E1000_ICR_RXO as i64),
            ("E1000_ICR_RXT0", E1000_ICR_RXT0 as i64),
            ("E1000_ICR_MDAC", E1000_ICR_MDAC as i64),
            ("E1000_ICR_RXCFG", E1000_ICR_RXCFG as i64),
            ("E1000_ICR_GPI_EN0", E1000_ICR_GPI_EN0 as i64),
            ("E1000_ICR_GPI_EN1", E1000_ICR_GPI_EN1 as i64),
            ("E1000_ICR_GPI_EN2", E1000_ICR_GPI_EN2 as i64),
            ("E1000_ICR_GPI_EN3", E1000_ICR_GPI_EN3 as i64),
            ("E1000_ICR_TXD_LOW", E1000_ICR_TXD_LOW as i64),
            ("E1000_ICR_SRPD", E1000_ICR_SRPD as i64),
            ("E1000_ICR_ACK", E1000_ICR_ACK as i64),
            ("E1000_ICR_MNG", E1000_ICR_MNG as i64),
            ("E1000_ICR_DOCK", E1000_ICR_DOCK as i64),
            ("E1000_ICR_INT_ASSERTED", E1000_ICR_INT_ASSERTED as i64),
            ("E1000_ICR_RXD_FIFO_PAR0", E1000_ICR_RXD_FIFO_PAR0 as i64),
            ("E1000_ICR_TXD_FIFO_PAR0", E1000_ICR_TXD_FIFO_PAR0 as i64),
            ("E1000_ICR_HOST_ARB_PAR", E1000_ICR_HOST_ARB_PAR as i64),
            ("E1000_ICR_PB_PAR", E1000_ICR_PB_PAR as i64),
            ("E1000_ICR_RXD_FIFO_PAR1", E1000_ICR_RXD_FIFO_PAR1 as i64),
            ("E1000_ICR_TXD_FIFO_PAR1", E1000_ICR_TXD_FIFO_PAR1 as i64),
            ("E1000_ICR_ALL_PARITY", E1000_ICR_ALL_PARITY as i64),
            ("E1000_ICR_DSW", E1000_ICR_DSW as i64),
            ("E1000_ICR_PHYINT", E1000_ICR_PHYINT as i64),
            ("E1000_ICR_EPRST", E1000_ICR_EPRST as i64),
            ("E1000_ICR_DRSTA", E1000_ICR_DRSTA as i64),
            ("E1000_ICS_TXDW", E1000_ICS_TXDW as i64),
            ("E1000_ICS_TXQE", E1000_ICS_TXQE as i64),
            ("E1000_ICS_LSC", E1000_ICS_LSC as i64),
            ("E1000_ICS_RXSEQ", E1000_ICS_RXSEQ as i64),
            ("E1000_ICS_RXDMT0", E1000_ICS_RXDMT0 as i64),
            ("E1000_ICS_RXO", E1000_ICS_RXO as i64),
            ("E1000_ICS_RXT0", E1000_ICS_RXT0 as i64),
            ("E1000_ICS_MDAC", E1000_ICS_MDAC as i64),
            ("E1000_ICS_RXCFG", E1000_ICS_RXCFG as i64),
            ("E1000_ICS_GPI_EN0", E1000_ICS_GPI_EN0 as i64),
            ("E1000_ICS_GPI_EN1", E1000_ICS_GPI_EN1 as i64),
            ("E1000_ICS_GPI_EN2", E1000_ICS_GPI_EN2 as i64),
            ("E1000_ICS_GPI_EN3", E1000_ICS_GPI_EN3 as i64),
            ("E1000_ICS_TXD_LOW", E1000_ICS_TXD_LOW as i64),
            ("E1000_ICS_SRPD", E1000_ICS_SRPD as i64),
            ("E1000_ICS_ACK", E1000_ICS_ACK as i64),
            ("E1000_ICS_MNG", E1000_ICS_MNG as i64),
            ("E1000_ICS_DOCK", E1000_ICS_DOCK as i64),
            ("E1000_ICS_RXD_FIFO_PAR0", E1000_ICS_RXD_FIFO_PAR0 as i64),
            ("E1000_ICS_TXD_FIFO_PAR0", E1000_ICS_TXD_FIFO_PAR0 as i64),
            ("E1000_ICS_HOST_ARB_PAR", E1000_ICS_HOST_ARB_PAR as i64),
            ("E1000_ICS_PB_PAR", E1000_ICS_PB_PAR as i64),
            ("E1000_ICS_RXD_FIFO_PAR1", E1000_ICS_RXD_FIFO_PAR1 as i64),
            ("E1000_ICS_TXD_FIFO_PAR1", E1000_ICS_TXD_FIFO_PAR1 as i64),
            ("E1000_ICS_DSW", E1000_ICS_DSW as i64),
            ("E1000_ICS_PHYINT", E1000_ICS_PHYINT as i64),
            ("E1000_ICS_EPRST", E1000_ICS_EPRST as i64),
            ("E1000_ICS_DRSTA", E1000_ICS_DRSTA as i64),
            ("E1000_IMS_TXDW", E1000_IMS_TXDW as i64),
            ("E1000_IMS_TXQE", E1000_IMS_TXQE as i64),
            ("E1000_IMS_LSC", E1000_IMS_LSC as i64),
            ("E1000_IMS_RXSEQ", E1000_IMS_RXSEQ as i64),
            ("E1000_IMS_RXDMT0", E1000_IMS_RXDMT0 as i64),
            ("E1000_IMS_RXO", E1000_IMS_RXO as i64),
            ("E1000_IMS_RXT0", E1000_IMS_RXT0 as i64),
            ("E1000_IMS_MDAC", E1000_IMS_MDAC as i64),
            ("E1000_IMS_RXCFG", E1000_IMS_RXCFG as i64),
            ("E1000_IMS_GPI_EN0", E1000_IMS_GPI_EN0 as i64),
            ("E1000_IMS_GPI_EN1", E1000_IMS_GPI_EN1 as i64),
            ("E1000_IMS_GPI_EN2", E1000_IMS_GPI_EN2 as i64),
            ("E1000_IMS_GPI_EN3", E1000_IMS_GPI_EN3 as i64),
            ("E1000_IMS_TXD_LOW", E1000_IMS_TXD_LOW as i64),
            ("E1000_IMS_SRPD", E1000_IMS_SRPD as i64),
            ("E1000_IMS_ACK", E1000_IMS_ACK as i64),
            ("E1000_IMS_MNG", E1000_IMS_MNG as i64),
            ("E1000_IMS_DOCK", E1000_IMS_DOCK as i64),
            ("E1000_IMS_RXD_FIFO_PAR0", E1000_IMS_RXD_FIFO_PAR0 as i64),
            ("E1000_IMS_TXD_FIFO_PAR0", E1000_IMS_TXD_FIFO_PAR0 as i64),
            ("E1000_IMS_HOST_ARB_PAR", E1000_IMS_HOST_ARB_PAR as i64),
            ("E1000_IMS_PB_PAR", E1000_IMS_PB_PAR as i64),
            ("E1000_IMS_RXD_FIFO_PAR1", E1000_IMS_RXD_FIFO_PAR1 as i64),
            ("E1000_IMS_TXD_FIFO_PAR1", E1000_IMS_TXD_FIFO_PAR1 as i64),
            ("E1000_IMS_DSW", E1000_IMS_DSW as i64),
            ("E1000_IMS_PHYINT", E1000_IMS_PHYINT as i64),
            ("E1000_IMS_EPRST", E1000_IMS_EPRST as i64),
            ("E1000_IMS_DRSTA", E1000_IMS_DRSTA as i64),
            ("E1000_IMC_TXDW", E1000_IMC_TXDW as i64),
            ("E1000_IMC_TXQE", E1000_IMC_TXQE as i64),
            ("E1000_IMC_LSC", E1000_IMC_LSC as i64),
            ("E1000_IMC_RXSEQ", E1000_IMC_RXSEQ as i64),
            ("E1000_IMC_RXDMT0", E1000_IMC_RXDMT0 as i64),
            ("E1000_IMC_RXO", E1000_IMC_RXO as i64),
            ("E1000_IMC_RXT0", E1000_IMC_RXT0 as i64),
            ("E1000_IMC_MDAC", E1000_IMC_MDAC as i64),
            ("E1000_IMC_RXCFG", E1000_IMC_RXCFG as i64),
            ("E1000_IMC_GPI_EN0", E1000_IMC_GPI_EN0 as i64),
            ("E1000_IMC_GPI_EN1", E1000_IMC_GPI_EN1 as i64),
            ("E1000_IMC_GPI_EN2", E1000_IMC_GPI_EN2 as i64),
            ("E1000_IMC_GPI_EN3", E1000_IMC_GPI_EN3 as i64),
            ("E1000_IMC_TXD_LOW", E1000_IMC_TXD_LOW as i64),
            ("E1000_IMC_SRPD", E1000_IMC_SRPD as i64),
            ("E1000_IMC_ACK", E1000_IMC_ACK as i64),
            ("E1000_IMC_MNG", E1000_IMC_MNG as i64),
            ("E1000_IMC_DOCK", E1000_IMC_DOCK as i64),
            ("E1000_IMC_RXD_FIFO_PAR0", E1000_IMC_RXD_FIFO_PAR0 as i64),
            ("E1000_IMC_TXD_FIFO_PAR0", E1000_IMC_TXD_FIFO_PAR0 as i64),
            ("E1000_IMC_HOST_ARB_PAR", E1000_IMC_HOST_ARB_PAR as i64),
            ("E1000_IMC_PB_PAR", E1000_IMC_PB_PAR as i64),
            ("E1000_IMC_RXD_FIFO_PAR1", E1000_IMC_RXD_FIFO_PAR1 as i64),
            ("E1000_IMC_TXD_FIFO_PAR1", E1000_IMC_TXD_FIFO_PAR1 as i64),
            ("E1000_IMC_DSW", E1000_IMC_DSW as i64),
            ("E1000_IMC_PHYINT", E1000_IMC_PHYINT as i64),
            ("E1000_IMC_EPRST", E1000_IMC_EPRST as i64),
            ("E1000_IMC_DRSTA", E1000_IMC_DRSTA as i64),
            ("E1000_RCTL_RST", E1000_RCTL_RST as i64),
            ("E1000_RCTL_EN", E1000_RCTL_EN as i64),
            ("E1000_RCTL_SBP", E1000_RCTL_SBP as i64),
            ("E1000_RCTL_UPE", E1000_RCTL_UPE as i64),
            ("E1000_RCTL_MPE", E1000_RCTL_MPE as i64),
            ("E1000_RCTL_LPE", E1000_RCTL_LPE as i64),
            ("E1000_RCTL_LBM_NO", E1000_RCTL_LBM_NO as i64),
            ("E1000_RCTL_LBM_MAC", E1000_RCTL_LBM_MAC as i64),
            ("E1000_RCTL_LBM_SLP", E1000_RCTL_LBM_SLP as i64),
            ("E1000_RCTL_LBM_TCVR", E1000_RCTL_LBM_TCVR as i64),
            ("E1000_RCTL_DTYP_MASK", E1000_RCTL_DTYP_MASK as i64),
            ("E1000_RCTL_DTYP_PS", E1000_RCTL_DTYP_PS as i64),
            ("E1000_RCTL_RDMTS_HALF", E1000_RCTL_RDMTS_HALF as i64),
            ("E1000_RCTL_RDMTS_QUAT", E1000_RCTL_RDMTS_QUAT as i64),
            ("E1000_RCTL_RDMTS_EIGTH", E1000_RCTL_RDMTS_EIGTH as i64),
            ("E1000_RCTL_RDMTS_HEX", E1000_RCTL_RDMTS_HEX as i64),
            ("E1000_RCTL_MO_SHIFT", E1000_RCTL_MO_SHIFT as i64),
            ("E1000_RCTL_MO_0", E1000_RCTL_MO_0 as i64),
            ("E1000_RCTL_MO_1", E1000_RCTL_MO_1 as i64),
            ("E1000_RCTL_MO_2", E1000_RCTL_MO_2 as i64),
            ("E1000_RCTL_MO_3", E1000_RCTL_MO_3 as i64),
            ("E1000_RCTL_MDR", E1000_RCTL_MDR as i64),
            ("E1000_RCTL_BAM", E1000_RCTL_BAM as i64),
            ("E1000_RCTL_SZ_2048", E1000_RCTL_SZ_2048 as i64),
            ("E1000_RCTL_SZ_1024", E1000_RCTL_SZ_1024 as i64),
            ("E1000_RCTL_SZ_512", E1000_RCTL_SZ_512 as i64),
            ("E1000_RCTL_SZ_256", E1000_RCTL_SZ_256 as i64),
            ("E1000_RCTL_SZ_16384", E1000_RCTL_SZ_16384 as i64),
            ("E1000_RCTL_SZ_8192", E1000_RCTL_SZ_8192 as i64),
            ("E1000_RCTL_SZ_4096", E1000_RCTL_SZ_4096 as i64),
            ("E1000_RCTL_VFE", E1000_RCTL_VFE as i64),
            ("E1000_RCTL_CFIEN", E1000_RCTL_CFIEN as i64),
            ("E1000_RCTL_CFI", E1000_RCTL_CFI as i64),
            ("E1000_RCTL_DPF", E1000_RCTL_DPF as i64),
            ("E1000_RCTL_PMCF", E1000_RCTL_PMCF as i64),
            ("E1000_RCTL_BSEX", E1000_RCTL_BSEX as i64),
            ("E1000_RCTL_SECRC", E1000_RCTL_SECRC as i64),
            ("E1000_RCTL_FLXBUF_MASK", E1000_RCTL_FLXBUF_MASK as i64),
            ("E1000_RCTL_FLXBUF_SHIFT", E1000_RCTL_FLXBUF_SHIFT as i64),
            ("E1000_PSRCTL_BSIZE0_MASK", E1000_PSRCTL_BSIZE0_MASK as i64),
            ("E1000_PSRCTL_BSIZE1_MASK", E1000_PSRCTL_BSIZE1_MASK as i64),
            ("E1000_PSRCTL_BSIZE2_MASK", E1000_PSRCTL_BSIZE2_MASK as i64),
            ("E1000_PSRCTL_BSIZE3_MASK", E1000_PSRCTL_BSIZE3_MASK as i64),
            (
                "E1000_PSRCTL_BSIZE0_SHIFT",
                E1000_PSRCTL_BSIZE0_SHIFT as i64
            ),
            (
                "E1000_PSRCTL_BSIZE1_SHIFT",
                E1000_PSRCTL_BSIZE1_SHIFT as i64
            ),
            (
                "E1000_PSRCTL_BSIZE2_SHIFT",
                E1000_PSRCTL_BSIZE2_SHIFT as i64
            ),
            (
                "E1000_PSRCTL_BSIZE3_SHIFT",
                E1000_PSRCTL_BSIZE3_SHIFT as i64
            ),
            ("E1000_SWFW_EEP_SM", E1000_SWFW_EEP_SM as i64),
            ("E1000_SWFW_PHY0_SM", E1000_SWFW_PHY0_SM as i64),
            ("E1000_SWFW_PHY1_SM", E1000_SWFW_PHY1_SM as i64),
            ("E1000_SWFW_MAC_CSR_SM", E1000_SWFW_MAC_CSR_SM as i64),
            ("E1000_SWFW_PHY2_SM", E1000_SWFW_PHY2_SM as i64),
            ("E1000_SWFW_PHY3_SM", E1000_SWFW_PHY3_SM as i64),
            ("E1000_RDT_DELAY", E1000_RDT_DELAY as i64),
            ("E1000_RDT_FPDB", E1000_RDT_FPDB as i64),
            ("E1000_RDLEN_LEN", E1000_RDLEN_LEN as i64),
            ("E1000_RDH_RDH", E1000_RDH_RDH as i64),
            ("E1000_RDT_RDT", E1000_RDT_RDT as i64),
            ("E1000_FCRTH_RTH", E1000_FCRTH_RTH as i64),
            ("E1000_FCRTH_XFCE", E1000_FCRTH_XFCE as i64),
            ("E1000_FCRTL_RTL", E1000_FCRTL_RTL as i64),
            ("E1000_FCRTL_XONE", E1000_FCRTL_XONE as i64),
            ("E1000_FC_NONE", E1000_FC_NONE as i64),
            ("E1000_FC_RX_PAUSE", E1000_FC_RX_PAUSE as i64),
            ("E1000_FC_TX_PAUSE", E1000_FC_TX_PAUSE as i64),
            ("E1000_FC_FULL", E1000_FC_FULL as i64),
            ("E1000_FC_DEFAULT", E1000_FC_DEFAULT as i64),
            ("E1000_RFCTL_ISCSI_DIS", E1000_RFCTL_ISCSI_DIS as i64),
            (
                "E1000_RFCTL_ISCSI_DWC_MASK",
                E1000_RFCTL_ISCSI_DWC_MASK as i64
            ),
            (
                "E1000_RFCTL_ISCSI_DWC_SHIFT",
                E1000_RFCTL_ISCSI_DWC_SHIFT as i64
            ),
            ("E1000_RFCTL_NFSW_DIS", E1000_RFCTL_NFSW_DIS as i64),
            ("E1000_RFCTL_NFSR_DIS", E1000_RFCTL_NFSR_DIS as i64),
            ("E1000_RFCTL_NFS_VER_MASK", E1000_RFCTL_NFS_VER_MASK as i64),
            (
                "E1000_RFCTL_NFS_VER_SHIFT",
                E1000_RFCTL_NFS_VER_SHIFT as i64
            ),
            ("E1000_RFCTL_IPV6_DIS", E1000_RFCTL_IPV6_DIS as i64),
            (
                "E1000_RFCTL_IPV6_XSUM_DIS",
                E1000_RFCTL_IPV6_XSUM_DIS as i64
            ),
            ("E1000_RFCTL_ACK_DIS", E1000_RFCTL_ACK_DIS as i64),
            ("E1000_RFCTL_ACKD_DIS", E1000_RFCTL_ACKD_DIS as i64),
            ("E1000_RFCTL_IPFRSP_DIS", E1000_RFCTL_IPFRSP_DIS as i64),
            ("E1000_RFCTL_EXTEN", E1000_RFCTL_EXTEN as i64),
            ("E1000_RFCTL_IPV6_EX_DIS", E1000_RFCTL_IPV6_EX_DIS as i64),
            (
                "E1000_RFCTL_NEW_IPV6_EXT_DIS",
                E1000_RFCTL_NEW_IPV6_EXT_DIS as i64
            ),
            ("E1000_RXDCTL_PTHRESH", E1000_RXDCTL_PTHRESH as i64),
            ("E1000_RXDCTL_HTHRESH", E1000_RXDCTL_HTHRESH as i64),
            ("E1000_RXDCTL_WTHRESH", E1000_RXDCTL_WTHRESH as i64),
            (
                "E1000_RXDCTL_THRESH_UNIT_DESC",
                E1000_RXDCTL_THRESH_UNIT_DESC as i64
            ),
            (
                "E1000_RXDCTL_QUEUE_ENABLE",
                E1000_RXDCTL_QUEUE_ENABLE as i64
            ),
            ("E1000_EITR_ITR_INT_MASK", E1000_EITR_ITR_INT_MASK as i64),
            ("E1000_EITR_CNT_IGNR", E1000_EITR_CNT_IGNR as i64),
            ("E1000_EITR_INTERVAL", E1000_EITR_INTERVAL as i64),
            ("E1000_TXDCTL_PTHRESH", E1000_TXDCTL_PTHRESH as i64),
            ("E1000_TXDCTL_HTHRESH", E1000_TXDCTL_HTHRESH as i64),
            ("E1000_TXDCTL_WTHRESH", E1000_TXDCTL_WTHRESH as i64),
            ("E1000_TXDCTL_GRAN", E1000_TXDCTL_GRAN as i64),
            ("E1000_TXDCTL_LWTHRESH", E1000_TXDCTL_LWTHRESH as i64),
            (
                "E1000_TXDCTL_FULL_TX_DESC_WB",
                E1000_TXDCTL_FULL_TX_DESC_WB as i64
            ),
            ("E1000_TXDCTL_COUNT_DESC", E1000_TXDCTL_COUNT_DESC as i64),
            (
                "E1000_TXDCTL_QUEUE_ENABLE",
                E1000_TXDCTL_QUEUE_ENABLE as i64
            ),
            ("E1000_TXCW_FD", E1000_TXCW_FD as i64),
            ("E1000_TXCW_HD", E1000_TXCW_HD as i64),
            ("E1000_TXCW_PAUSE", E1000_TXCW_PAUSE as i64),
            ("E1000_TXCW_ASM_DIR", E1000_TXCW_ASM_DIR as i64),
            ("E1000_TXCW_PAUSE_MASK", E1000_TXCW_PAUSE_MASK as i64),
            ("E1000_TXCW_RF", E1000_TXCW_RF as i64),
            ("E1000_TXCW_NP", E1000_TXCW_NP as i64),
            ("E1000_TXCW_CW", E1000_TXCW_CW as i64),
            ("E1000_TXCW_TXC", E1000_TXCW_TXC as i64),
            ("E1000_TXCW_ANE", E1000_TXCW_ANE as i64),
            ("E1000_RXCW_CW", E1000_RXCW_CW as i64),
            ("E1000_RXCW_NC", E1000_RXCW_NC as i64),
            ("E1000_RXCW_IV", E1000_RXCW_IV as i64),
            ("E1000_RXCW_CC", E1000_RXCW_CC as i64),
            ("E1000_RXCW_C", E1000_RXCW_C as i64),
            ("E1000_RXCW_SYNCH", E1000_RXCW_SYNCH as i64),
            ("E1000_RXCW_ANC", E1000_RXCW_ANC as i64),
            ("E1000_TCTL_RST", E1000_TCTL_RST as i64),
            ("E1000_TCTL_EN", E1000_TCTL_EN as i64),
            ("E1000_TCTL_BCE", E1000_TCTL_BCE as i64),
            ("E1000_TCTL_PSP", E1000_TCTL_PSP as i64),
            ("E1000_TCTL_CT", E1000_TCTL_CT as i64),
            ("E1000_TCTL_COLD", E1000_TCTL_COLD as i64),
            ("E1000_TCTL_SWXOFF", E1000_TCTL_SWXOFF as i64),
            ("E1000_TCTL_PBE", E1000_TCTL_PBE as i64),
            ("E1000_TCTL_RTLC", E1000_TCTL_RTLC as i64),
            ("E1000_TCTL_NRTU", E1000_TCTL_NRTU as i64),
            ("E1000_TCTL_MULR", E1000_TCTL_MULR as i64),
            ("E1000_TCTL_EXT_BST_MASK", E1000_TCTL_EXT_BST_MASK as i64),
            ("E1000_TCTL_EXT_GCEX_MASK", E1000_TCTL_EXT_GCEX_MASK as i64),
            (
                "DEFAULT_80003ES2LAN_TCTL_EXT_GCEX",
                DEFAULT_80003ES2LAN_TCTL_EXT_GCEX as i64
            ),
            ("E1000_RXCSUM_PCSS_MASK", E1000_RXCSUM_PCSS_MASK as i64),
            ("E1000_RXCSUM_IPOFL", E1000_RXCSUM_IPOFL as i64),
            ("E1000_RXCSUM_TUOFL", E1000_RXCSUM_TUOFL as i64),
            ("E1000_RXCSUM_IPV6OFL", E1000_RXCSUM_IPV6OFL as i64),
            ("E1000_RXCSUM_IPPCSE", E1000_RXCSUM_IPPCSE as i64),
            ("E1000_RXCSUM_PCSD", E1000_RXCSUM_PCSD as i64),
            ("E1000_ADVTXD_DTYP_CTXT", E1000_ADVTXD_DTYP_CTXT as i64),
            ("E1000_ADVTXD_DTYP_DATA", E1000_ADVTXD_DTYP_DATA as i64),
            ("E1000_ADVTXD_DCMD_IFCS", E1000_ADVTXD_DCMD_IFCS as i64),
            ("E1000_ADVTXD_DCMD_DEXT", E1000_ADVTXD_DCMD_DEXT as i64),
            ("E1000_ADVTXD_DCMD_VLE", E1000_ADVTXD_DCMD_VLE as i64),
            ("E1000_ADVTXD_DCMD_TSE", E1000_ADVTXD_DCMD_TSE as i64),
            (
                "E1000_ADVTXD_PAYLEN_SHIFT",
                E1000_ADVTXD_PAYLEN_SHIFT as i64
            ),
            (
                "E1000_ADVTXD_MACLEN_SHIFT",
                E1000_ADVTXD_MACLEN_SHIFT as i64
            ),
            ("E1000_ADVTXD_VLAN_SHIFT", E1000_ADVTXD_VLAN_SHIFT as i64),
            ("E1000_ADVTXD_TUCMD_IPV4", E1000_ADVTXD_TUCMD_IPV4 as i64),
            ("E1000_ADVTXD_TUCMD_IPV6", E1000_ADVTXD_TUCMD_IPV6 as i64),
            (
                "E1000_ADVTXD_TUCMD_L4T_UDP",
                E1000_ADVTXD_TUCMD_L4T_UDP as i64
            ),
            (
                "E1000_ADVTXD_TUCMD_L4T_TCP",
                E1000_ADVTXD_TUCMD_L4T_TCP as i64
            ),
            ("E1000_ADVTXD_L4LEN_SHIFT", E1000_ADVTXD_L4LEN_SHIFT as i64),
            ("E1000_ADVTXD_MSS_SHIFT", E1000_ADVTXD_MSS_SHIFT as i64),
            ("E1000_MRQC_ENABLE_MASK", E1000_MRQC_ENABLE_MASK as i64),
            ("E1000_MRQC_ENABLE_RSS_2Q", E1000_MRQC_ENABLE_RSS_2Q as i64),
            (
                "E1000_MRQC_ENABLE_RSS_INT",
                E1000_MRQC_ENABLE_RSS_INT as i64
            ),
            (
                "E1000_MRQC_RSS_FIELD_MASK",
                E1000_MRQC_RSS_FIELD_MASK as i64
            ),
            (
                "E1000_MRQC_RSS_FIELD_IPV4_TCP",
                E1000_MRQC_RSS_FIELD_IPV4_TCP as i64
            ),
            (
                "E1000_MRQC_RSS_FIELD_IPV4",
                E1000_MRQC_RSS_FIELD_IPV4 as i64
            ),
            (
                "E1000_MRQC_RSS_FIELD_IPV6_TCP_EX",
                E1000_MRQC_RSS_FIELD_IPV6_TCP_EX as i64
            ),
            (
                "E1000_MRQC_RSS_FIELD_IPV6_EX",
                E1000_MRQC_RSS_FIELD_IPV6_EX as i64
            ),
            (
                "E1000_MRQC_RSS_FIELD_IPV6",
                E1000_MRQC_RSS_FIELD_IPV6 as i64
            ),
            (
                "E1000_MRQC_RSS_FIELD_IPV6_TCP",
                E1000_MRQC_RSS_FIELD_IPV6_TCP as i64
            ),
            ("E1000_WUC_APME", E1000_WUC_APME as i64),
            ("E1000_WUC_PME_EN", E1000_WUC_PME_EN as i64),
            ("E1000_WUC_PME_STATUS", E1000_WUC_PME_STATUS as i64),
            ("E1000_WUC_APMPME", E1000_WUC_APMPME as i64),
            ("E1000_WUC_SPM", E1000_WUC_SPM as i64),
            ("E1000_WUFC_LNKC", E1000_WUFC_LNKC as i64),
            ("E1000_WUFC_MAG", E1000_WUFC_MAG as i64),
            ("E1000_WUFC_EX", E1000_WUFC_EX as i64),
            ("E1000_WUFC_MC", E1000_WUFC_MC as i64),
            ("E1000_WUFC_BC", E1000_WUFC_BC as i64),
            ("E1000_WUFC_ARP", E1000_WUFC_ARP as i64),
            ("E1000_WUFC_IPV4", E1000_WUFC_IPV4 as i64),
            ("E1000_WUFC_IPV6", E1000_WUFC_IPV6 as i64),
            ("E1000_WUFC_IGNORE_TCO", E1000_WUFC_IGNORE_TCO as i64),
            ("E1000_WUFC_FLX0", E1000_WUFC_FLX0 as i64),
            ("E1000_WUFC_FLX1", E1000_WUFC_FLX1 as i64),
            ("E1000_WUFC_FLX2", E1000_WUFC_FLX2 as i64),
            ("E1000_WUFC_FLX3", E1000_WUFC_FLX3 as i64),
            ("E1000_WUFC_ALL_FILTERS", E1000_WUFC_ALL_FILTERS as i64),
            ("E1000_WUFC_FLX_OFFSET", E1000_WUFC_FLX_OFFSET as i64),
            ("E1000_WUFC_FLX_FILTERS", E1000_WUFC_FLX_FILTERS as i64),
            ("E1000_WUS_LNKC", E1000_WUS_LNKC as i64),
            ("E1000_WUS_MAG", E1000_WUS_MAG as i64),
            ("E1000_WUS_EX", E1000_WUS_EX as i64),
            ("E1000_WUS_MC", E1000_WUS_MC as i64),
            ("E1000_WUS_BC", E1000_WUS_BC as i64),
            ("E1000_WUS_ARP", E1000_WUS_ARP as i64),
            ("E1000_WUS_IPV4", E1000_WUS_IPV4 as i64),
            ("E1000_WUS_IPV6", E1000_WUS_IPV6 as i64),
            ("E1000_WUS_FLX0", E1000_WUS_FLX0 as i64),
            ("E1000_WUS_FLX1", E1000_WUS_FLX1 as i64),
            ("E1000_WUS_FLX2", E1000_WUS_FLX2 as i64),
            ("E1000_WUS_FLX3", E1000_WUS_FLX3 as i64),
            ("E1000_WUS_FLX_FILTERS", E1000_WUS_FLX_FILTERS as i64),
            (
                "E1000_TARC0_CB_MULTIQ_2_REQ",
                E1000_TARC0_CB_MULTIQ_2_REQ as i64
            ),
            (
                "E1000_TARC0_CB_MULTIQ_3_REQ",
                E1000_TARC0_CB_MULTIQ_3_REQ as i64
            ),
            ("E1000_MANC_SMBUS_EN", E1000_MANC_SMBUS_EN as i64),
            ("E1000_MANC_ASF_EN", E1000_MANC_ASF_EN as i64),
            ("E1000_MANC_R_ON_FORCE", E1000_MANC_R_ON_FORCE as i64),
            ("E1000_MANC_RMCP_EN", E1000_MANC_RMCP_EN as i64),
            ("E1000_MANC_0298_EN", E1000_MANC_0298_EN as i64),
            ("E1000_MANC_IPV4_EN", E1000_MANC_IPV4_EN as i64),
            ("E1000_MANC_IPV6_EN", E1000_MANC_IPV6_EN as i64),
            ("E1000_MANC_SNAP_EN", E1000_MANC_SNAP_EN as i64),
            ("E1000_MANC_ARP_EN", E1000_MANC_ARP_EN as i64),
            ("E1000_MANC_NEIGHBOR_EN", E1000_MANC_NEIGHBOR_EN as i64),
            ("E1000_MANC_ARP_RES_EN", E1000_MANC_ARP_RES_EN as i64),
            ("E1000_MANC_TCO_RESET", E1000_MANC_TCO_RESET as i64),
            ("E1000_MANC_RCV_TCO_EN", E1000_MANC_RCV_TCO_EN as i64),
            ("E1000_MANC_REPORT_STATUS", E1000_MANC_REPORT_STATUS as i64),
            ("E1000_MANC_RCV_ALL", E1000_MANC_RCV_ALL as i64),
            (
                "E1000_MANC_BLK_PHY_RST_ON_IDE",
                E1000_MANC_BLK_PHY_RST_ON_IDE as i64
            ),
            (
                "E1000_MANC_EN_MAC_ADDR_FILTER",
                E1000_MANC_EN_MAC_ADDR_FILTER as i64
            ),
            ("E1000_MANC_EN_MNG2HOST", E1000_MANC_EN_MNG2HOST as i64),
            (
                "E1000_MANC_EN_IP_ADDR_FILTER",
                E1000_MANC_EN_IP_ADDR_FILTER as i64
            ),
            (
                "E1000_MANC_EN_XSUM_FILTER",
                E1000_MANC_EN_XSUM_FILTER as i64
            ),
            ("E1000_MANC_BR_EN", E1000_MANC_BR_EN as i64),
            ("E1000_MANC_SMB_REQ", E1000_MANC_SMB_REQ as i64),
            ("E1000_MANC_SMB_GNT", E1000_MANC_SMB_GNT as i64),
            ("E1000_MANC_SMB_CLK_IN", E1000_MANC_SMB_CLK_IN as i64),
            ("E1000_MANC_SMB_DATA_IN", E1000_MANC_SMB_DATA_IN as i64),
            ("E1000_MANC_SMB_DATA_OUT", E1000_MANC_SMB_DATA_OUT as i64),
            ("E1000_MANC_SMB_CLK_OUT", E1000_MANC_SMB_CLK_OUT as i64),
            (
                "E1000_MANC_SMB_DATA_OUT_SHIFT",
                E1000_MANC_SMB_DATA_OUT_SHIFT as i64
            ),
            (
                "E1000_MANC_SMB_CLK_OUT_SHIFT",
                E1000_MANC_SMB_CLK_OUT_SHIFT as i64
            ),
            ("E1000_SWSM_SMBI", E1000_SWSM_SMBI as i64),
            ("E1000_SWSM_SWESMBI", E1000_SWSM_SWESMBI as i64),
            ("E1000_SWSM_WMNG", E1000_SWSM_WMNG as i64),
            ("E1000_SWSM_DRV_LOAD", E1000_SWSM_DRV_LOAD as i64),
            ("E1000_H2ME_ULP", E1000_H2ME_ULP as i64),
            (
                "E1000_H2ME_ENFORCE_SETTINGS",
                E1000_H2ME_ENFORCE_SETTINGS as i64
            ),
            ("E1000_FWSM_MODE_MASK", E1000_FWSM_MODE_MASK as i64),
            ("E1000_FWSM_MODE_SHIFT", E1000_FWSM_MODE_SHIFT as i64),
            ("E1000_FWSM_ULP_CFG_DONE", E1000_FWSM_ULP_CFG_DONE as i64),
            ("E1000_FWSM_FW_VALID", E1000_FWSM_FW_VALID as i64),
            ("E1000_FWSM_RSPCIPHY", E1000_FWSM_RSPCIPHY as i64),
            ("E1000_FWSM_DISSW", E1000_FWSM_DISSW as i64),
            ("E1000_FWSM_SKUSEL_MASK", E1000_FWSM_SKUSEL_MASK as i64),
            ("E1000_FWSM_SKUEL_SHIFT", E1000_FWSM_SKUEL_SHIFT as i64),
            ("E1000_FWSM_SKUSEL_EMB", E1000_FWSM_SKUSEL_EMB as i64),
            ("E1000_FWSM_SKUSEL_CONS", E1000_FWSM_SKUSEL_CONS as i64),
            (
                "E1000_FWSM_SKUSEL_PERF_100",
                E1000_FWSM_SKUSEL_PERF_100 as i64
            ),
            (
                "E1000_FWSM_SKUSEL_PERF_GBE",
                E1000_FWSM_SKUSEL_PERF_GBE as i64
            ),
            ("E1000_FFLT_DBG_INVC", E1000_FFLT_DBG_INVC as i64),
            ("E1000_HICR_EN", E1000_HICR_EN as i64),
            ("E1000_HICR_C", E1000_HICR_C as i64),
            ("E1000_HICR_SV", E1000_HICR_SV as i64),
            ("E1000_HICR_FWR", E1000_HICR_FWR as i64),
            ("E1000_HI_MAX_DATA_LENGTH", E1000_HI_MAX_DATA_LENGTH as i64),
            (
                "E1000_HI_MAX_BLOCK_BYTE_LENGTH",
                E1000_HI_MAX_BLOCK_BYTE_LENGTH as i64
            ),
            (
                "E1000_HI_MAX_BLOCK_DWORD_LENGTH",
                E1000_HI_MAX_BLOCK_DWORD_LENGTH as i64
            ),
            ("E1000_HI_COMMAND_TIMEOUT", E1000_HI_COMMAND_TIMEOUT as i64),
            ("E1000_HSMC0R_CLKIN", E1000_HSMC0R_CLKIN as i64),
            ("E1000_HSMC0R_DATAIN", E1000_HSMC0R_DATAIN as i64),
            ("E1000_HSMC0R_DATAOUT", E1000_HSMC0R_DATAOUT as i64),
            ("E1000_HSMC0R_CLKOUT", E1000_HSMC0R_CLKOUT as i64),
            ("E1000_HSMC1R_CLKIN", E1000_HSMC1R_CLKIN as i64),
            ("E1000_HSMC1R_DATAIN", E1000_HSMC1R_DATAIN as i64),
            ("E1000_HSMC1R_DATAOUT", E1000_HSMC1R_DATAOUT as i64),
            ("E1000_HSMC1R_CLKOUT", E1000_HSMC1R_CLKOUT as i64),
            ("E1000_FWSTS_FWS_MASK", E1000_FWSTS_FWS_MASK as i64),
            ("E1000_WUPL_LENGTH_MASK", E1000_WUPL_LENGTH_MASK as i64),
            ("E1000_MDALIGN", E1000_MDALIGN as i64),
            ("E1000_MDICNFG_EXT_MDIO", E1000_MDICNFG_EXT_MDIO as i64),
            ("E1000_MDICNFG_COM_MDIO", E1000_MDICNFG_COM_MDIO as i64),
            ("E1000_MDICNFG_PHY_MASK", E1000_MDICNFG_PHY_MASK as i64),
            ("E1000_MDICNFG_PHY_SHIFT", E1000_MDICNFG_PHY_SHIFT as i64),
            ("E1000_IPCNFG_EEE_1G_AN", E1000_IPCNFG_EEE_1G_AN as i64),
            ("E1000_IPCNFG_EEE_100M_AN", E1000_IPCNFG_EEE_100M_AN as i64),
            ("E1000_EEER_TX_LPI_EN", E1000_EEER_TX_LPI_EN as i64),
            ("E1000_EEER_RX_LPI_EN", E1000_EEER_RX_LPI_EN as i64),
            ("E1000_EEER_LPI_FC", E1000_EEER_LPI_FC as i64),
            ("E1000_EEER_EEE_NEG", E1000_EEER_EEE_NEG as i64),
            ("E1000_EEER_RX_LPI_STATUS", E1000_EEER_RX_LPI_STATUS as i64),
            ("E1000_EEER_TX_LPI_STATUS", E1000_EEER_TX_LPI_STATUS as i64),
            ("E1000_GCR_RXD_NO_SNOOP", E1000_GCR_RXD_NO_SNOOP as i64),
            (
                "E1000_GCR_RXDSCW_NO_SNOOP",
                E1000_GCR_RXDSCW_NO_SNOOP as i64
            ),
            (
                "E1000_GCR_RXDSCR_NO_SNOOP",
                E1000_GCR_RXDSCR_NO_SNOOP as i64
            ),
            ("E1000_GCR_TXD_NO_SNOOP", E1000_GCR_TXD_NO_SNOOP as i64),
            (
                "E1000_GCR_TXDSCW_NO_SNOOP",
                E1000_GCR_TXDSCW_NO_SNOOP as i64
            ),
            (
                "E1000_GCR_TXDSCR_NO_SNOOP",
                E1000_GCR_TXDSCR_NO_SNOOP as i64
            ),
            (
                "E1000_GCR_CMPL_TMOUT_MASK",
                E1000_GCR_CMPL_TMOUT_MASK as i64
            ),
            (
                "E1000_GCR_CMPL_TMOUT_10ms",
                E1000_GCR_CMPL_TMOUT_10ms as i64
            ),
            (
                "E1000_GCR_CMPL_TMOUT_RESEND",
                E1000_GCR_CMPL_TMOUT_RESEND as i64
            ),
            ("E1000_GCR_CAP_VER2", E1000_GCR_CAP_VER2 as i64),
            ("PCI_EX_NO_SNOOP_ALL", PCI_EX_NO_SNOOP_ALL as i64),
            ("PCI_EX_82566_SNOOP_ALL", PCI_EX_82566_SNOOP_ALL as i64),
            (
                "E1000_GCR_L1_ACT_WITHOUT_L0S_RX",
                E1000_GCR_L1_ACT_WITHOUT_L0S_RX as i64
            ),
            (
                "E1000_FACTPS_FUNC0_POWER_STATE_MASK",
                E1000_FACTPS_FUNC0_POWER_STATE_MASK as i64
            ),
            ("E1000_FACTPS_LAN0_VALID", E1000_FACTPS_LAN0_VALID as i64),
            (
                "E1000_FACTPS_FUNC0_AUX_EN",
                E1000_FACTPS_FUNC0_AUX_EN as i64
            ),
            (
                "E1000_FACTPS_FUNC1_POWER_STATE_MASK",
                E1000_FACTPS_FUNC1_POWER_STATE_MASK as i64
            ),
            (
                "E1000_FACTPS_FUNC1_POWER_STATE_SHIFT",
                E1000_FACTPS_FUNC1_POWER_STATE_SHIFT as i64
            ),
            ("E1000_FACTPS_LAN1_VALID", E1000_FACTPS_LAN1_VALID as i64),
            (
                "E1000_FACTPS_FUNC1_AUX_EN",
                E1000_FACTPS_FUNC1_AUX_EN as i64
            ),
            (
                "E1000_FACTPS_FUNC2_POWER_STATE_MASK",
                E1000_FACTPS_FUNC2_POWER_STATE_MASK as i64
            ),
            (
                "E1000_FACTPS_FUNC2_POWER_STATE_SHIFT",
                E1000_FACTPS_FUNC2_POWER_STATE_SHIFT as i64
            ),
            ("E1000_FACTPS_IDE_ENABLE", E1000_FACTPS_IDE_ENABLE as i64),
            (
                "E1000_FACTPS_FUNC2_AUX_EN",
                E1000_FACTPS_FUNC2_AUX_EN as i64
            ),
            (
                "E1000_FACTPS_FUNC3_POWER_STATE_MASK",
                E1000_FACTPS_FUNC3_POWER_STATE_MASK as i64
            ),
            (
                "E1000_FACTPS_FUNC3_POWER_STATE_SHIFT",
                E1000_FACTPS_FUNC3_POWER_STATE_SHIFT as i64
            ),
            ("E1000_FACTPS_SP_ENABLE", E1000_FACTPS_SP_ENABLE as i64),
            (
                "E1000_FACTPS_FUNC3_AUX_EN",
                E1000_FACTPS_FUNC3_AUX_EN as i64
            ),
            (
                "E1000_FACTPS_FUNC4_POWER_STATE_MASK",
                E1000_FACTPS_FUNC4_POWER_STATE_MASK as i64
            ),
            (
                "E1000_FACTPS_FUNC4_POWER_STATE_SHIFT",
                E1000_FACTPS_FUNC4_POWER_STATE_SHIFT as i64
            ),
            ("E1000_FACTPS_IPMI_ENABLE", E1000_FACTPS_IPMI_ENABLE as i64),
            (
                "E1000_FACTPS_FUNC4_AUX_EN",
                E1000_FACTPS_FUNC4_AUX_EN as i64
            ),
            ("E1000_FACTPS_MNGCG", E1000_FACTPS_MNGCG as i64),
            (
                "E1000_FACTPS_LAN_FUNC_SEL",
                E1000_FACTPS_LAN_FUNC_SEL as i64
            ),
            (
                "E1000_FACTPS_PM_STATE_CHANGED",
                E1000_FACTPS_PM_STATE_CHANGED as i64
            ),
            ("E1000_IVAR_VALID", E1000_IVAR_VALID as i64),
            ("E1000_GPIE_NSICR", E1000_GPIE_NSICR as i64),
            ("E1000_GPIE_MSIX_MODE", E1000_GPIE_MSIX_MODE as i64),
            ("E1000_GPIE_EIAME", E1000_GPIE_EIAME as i64),
            ("E1000_GPIE_PBA", E1000_GPIE_PBA as i64),
            ("E1000_MRQC_ENABLE_RSS_4Q", E1000_MRQC_ENABLE_RSS_4Q as i64),
            ("E1000_MRQC_ENABLE_VMDQ", E1000_MRQC_ENABLE_VMDQ as i64),
            (
                "E1000_MRQC_ENABLE_VMDQ_RSS_2Q",
                E1000_MRQC_ENABLE_VMDQ_RSS_2Q as i64
            ),
            (
                "E1000_MRQC_RSS_FIELD_IPV4_UDP",
                E1000_MRQC_RSS_FIELD_IPV4_UDP as i64
            ),
            (
                "E1000_MRQC_RSS_FIELD_IPV6_UDP",
                E1000_MRQC_RSS_FIELD_IPV6_UDP as i64
            ),
            (
                "E1000_MRQC_RSS_FIELD_IPV6_UDP_EX",
                E1000_MRQC_RSS_FIELD_IPV6_UDP_EX as i64
            ),
            ("E1000_MRQC_ENABLE_RSS_8Q", E1000_MRQC_ENABLE_RSS_8Q as i64),
            (
                "E1000_SRRCTL_BSIZEPKT_SHIFT",
                E1000_SRRCTL_BSIZEPKT_SHIFT as i64
            ),
            (
                "E1000_SRRCTL_BSIZEHDRSIZE_MASK",
                E1000_SRRCTL_BSIZEHDRSIZE_MASK as i64
            ),
            (
                "E1000_SRRCTL_BSIZEHDRSIZE_SHIFT",
                E1000_SRRCTL_BSIZEHDRSIZE_SHIFT as i64
            ),
            (
                "E1000_SRRCTL_DESCTYPE_LEGACY",
                E1000_SRRCTL_DESCTYPE_LEGACY as i64
            ),
            (
                "E1000_SRRCTL_DESCTYPE_ADV_ONEBUF",
                E1000_SRRCTL_DESCTYPE_ADV_ONEBUF as i64
            ),
            (
                "E1000_SRRCTL_DESCTYPE_HDR_SPLIT",
                E1000_SRRCTL_DESCTYPE_HDR_SPLIT as i64
            ),
            (
                "E1000_SRRCTL_DESCTYPE_HDR_SPLIT_ALWAYS",
                E1000_SRRCTL_DESCTYPE_HDR_SPLIT_ALWAYS as i64
            ),
            (
                "E1000_SRRCTL_DESCTYPE_HDR_REPLICATION",
                E1000_SRRCTL_DESCTYPE_HDR_REPLICATION as i64
            ),
            (
                "E1000_SRRCTL_DESCTYPE_HDR_REPLICATION_LARGE_PKT",
                E1000_SRRCTL_DESCTYPE_HDR_REPLICATION_LARGE_PKT as i64
            ),
            (
                "E1000_SRRCTL_DESCTYPE_MASK",
                E1000_SRRCTL_DESCTYPE_MASK as i64
            ),
            ("E1000_SRRCTL_TIMESTAMP", E1000_SRRCTL_TIMESTAMP as i64),
            ("E1000_SRRCTL_DROP_EN", E1000_SRRCTL_DROP_EN as i64),
            ("E1000_WUFC_FLEX_HQ", E1000_WUFC_FLEX_HQ as i64),
            ("PCI_EX_LINK_STATUS", PCI_EX_LINK_STATUS as i64),
            ("PCI_EX_LINK_WIDTH_MASK", PCI_EX_LINK_WIDTH_MASK as i64),
            ("PCI_EX_LINK_WIDTH_SHIFT", PCI_EX_LINK_WIDTH_SHIFT as i64),
            ("PCI_EX_DEVICE_CONTROL2", PCI_EX_DEVICE_CONTROL2 as i64),
            (
                "PCI_EX_DEVICE_CONTROL2_16ms",
                PCI_EX_DEVICE_CONTROL2_16ms as i64
            ),
            (
                "EEPROM_READ_OPCODE_MICROWIRE",
                EEPROM_READ_OPCODE_MICROWIRE as i64
            ),
            (
                "EEPROM_WRITE_OPCODE_MICROWIRE",
                EEPROM_WRITE_OPCODE_MICROWIRE as i64
            ),
            (
                "EEPROM_ERASE_OPCODE_MICROWIRE",
                EEPROM_ERASE_OPCODE_MICROWIRE as i64
            ),
            (
                "EEPROM_EWEN_OPCODE_MICROWIRE",
                EEPROM_EWEN_OPCODE_MICROWIRE as i64
            ),
            (
                "EEPROM_EWDS_OPCODE_MICROWIRE",
                EEPROM_EWDS_OPCODE_MICROWIRE as i64
            ),
            ("EEPROM_MAX_RETRY_SPI", EEPROM_MAX_RETRY_SPI as i64),
            ("EEPROM_READ_OPCODE_SPI", EEPROM_READ_OPCODE_SPI as i64),
            ("EEPROM_WRITE_OPCODE_SPI", EEPROM_WRITE_OPCODE_SPI as i64),
            ("EEPROM_A8_OPCODE_SPI", EEPROM_A8_OPCODE_SPI as i64),
            ("EEPROM_WREN_OPCODE_SPI", EEPROM_WREN_OPCODE_SPI as i64),
            ("EEPROM_WRDI_OPCODE_SPI", EEPROM_WRDI_OPCODE_SPI as i64),
            ("EEPROM_RDSR_OPCODE_SPI", EEPROM_RDSR_OPCODE_SPI as i64),
            ("EEPROM_WRSR_OPCODE_SPI", EEPROM_WRSR_OPCODE_SPI as i64),
            (
                "EEPROM_ERASE4K_OPCODE_SPI",
                EEPROM_ERASE4K_OPCODE_SPI as i64
            ),
            (
                "EEPROM_ERASE64K_OPCODE_SPI",
                EEPROM_ERASE64K_OPCODE_SPI as i64
            ),
            (
                "EEPROM_ERASE256_OPCODE_SPI",
                EEPROM_ERASE256_OPCODE_SPI as i64
            ),
            ("EEPROM_WORD_SIZE_SHIFT", EEPROM_WORD_SIZE_SHIFT as i64),
            (
                "EEPROM_WORD_SIZE_SHIFT_MAX",
                EEPROM_WORD_SIZE_SHIFT_MAX as i64
            ),
            ("EEPROM_SIZE_SHIFT", EEPROM_SIZE_SHIFT as i64),
            ("EEPROM_SIZE_MASK", EEPROM_SIZE_MASK as i64),
            ("EEPROM_MAC_ADDR_WORD0", EEPROM_MAC_ADDR_WORD0 as i64),
            ("EEPROM_MAC_ADDR_WORD1", EEPROM_MAC_ADDR_WORD1 as i64),
            ("EEPROM_MAC_ADDR_WORD2", EEPROM_MAC_ADDR_WORD2 as i64),
            ("EEPROM_COMPAT", EEPROM_COMPAT as i64),
            ("EEPROM_ID_LED_SETTINGS", EEPROM_ID_LED_SETTINGS as i64),
            ("EEPROM_VERSION", EEPROM_VERSION as i64),
            ("EEPROM_SERDES_AMPLITUDE", EEPROM_SERDES_AMPLITUDE as i64),
            ("EEPROM_PHY_CLASS_WORD", EEPROM_PHY_CLASS_WORD as i64),
            ("EEPROM_INIT_CONTROL1_REG", EEPROM_INIT_CONTROL1_REG as i64),
            ("EEPROM_INIT_CONTROL2_REG", EEPROM_INIT_CONTROL2_REG as i64),
            (
                "EEPROM_SWDEF_PINS_CTRL_PORT_1",
                EEPROM_SWDEF_PINS_CTRL_PORT_1 as i64
            ),
            ("EEPROM_INIT_CONTROL4_REG", EEPROM_INIT_CONTROL4_REG as i64),
            (
                "EEPROM_INIT_CONTROL3_PORT_B",
                EEPROM_INIT_CONTROL3_PORT_B as i64
            ),
            ("EEPROM_INIT_3GIO_3", EEPROM_INIT_3GIO_3 as i64),
            ("EEPROM_LED_1_CFG", EEPROM_LED_1_CFG as i64),
            ("EEPROM_LED_0_2_CFG", EEPROM_LED_0_2_CFG as i64),
            (
                "EEPROM_SWDEF_PINS_CTRL_PORT_0",
                EEPROM_SWDEF_PINS_CTRL_PORT_0 as i64
            ),
            (
                "EEPROM_INIT_CONTROL3_PORT_A",
                EEPROM_INIT_CONTROL3_PORT_A as i64
            ),
            ("EEPROM_CFG", EEPROM_CFG as i64),
            ("EEPROM_FLASH_VERSION", EEPROM_FLASH_VERSION as i64),
            ("EEPROM_CHECKSUM_REG", EEPROM_CHECKSUM_REG as i64),
            ("EEPROM_COMPAT_VALID_CSUM", EEPROM_COMPAT_VALID_CSUM as i64),
            ("EEPROM_FUTURE_INIT_WORD1", EEPROM_FUTURE_INIT_WORD1 as i64),
            (
                "EEPROM_FUTURE_INIT_WORD1_VALID_CSUM",
                EEPROM_FUTURE_INIT_WORD1_VALID_CSUM as i64
            ),
            (
                "E1000_NVM_CFG_DONE_PORT_0",
                E1000_NVM_CFG_DONE_PORT_0 as i64
            ),
            (
                "E1000_NVM_CFG_DONE_PORT_1",
                E1000_NVM_CFG_DONE_PORT_1 as i64
            ),
            (
                "E1000_NVM_CFG_DONE_PORT_2",
                E1000_NVM_CFG_DONE_PORT_2 as i64
            ),
            (
                "E1000_NVM_CFG_DONE_PORT_3",
                E1000_NVM_CFG_DONE_PORT_3 as i64
            ),
            ("NVM_WORD24_COM_MDIO", NVM_WORD24_COM_MDIO as i64),
            ("NVM_WORD24_EXT_MDIO", NVM_WORD24_EXT_MDIO as i64),
            ("ID_LED_RESERVED_0000", ID_LED_RESERVED_0000 as i64),
            ("ID_LED_RESERVED_FFFF", ID_LED_RESERVED_FFFF as i64),
            ("ID_LED_RESERVED_82573", ID_LED_RESERVED_82573 as i64),
            ("ID_LED_DEFAULT_82573", ID_LED_DEFAULT_82573 as i64),
            ("ID_LED_DEFAULT", ID_LED_DEFAULT as i64),
            ("ID_LED_DEFAULT_ICH8LAN", ID_LED_DEFAULT_ICH8LAN as i64),
            ("ID_LED_DEF1_DEF2", ID_LED_DEF1_DEF2 as i64),
            ("ID_LED_DEF1_ON2", ID_LED_DEF1_ON2 as i64),
            ("ID_LED_DEF1_OFF2", ID_LED_DEF1_OFF2 as i64),
            ("ID_LED_ON1_DEF2", ID_LED_ON1_DEF2 as i64),
            ("ID_LED_ON1_ON2", ID_LED_ON1_ON2 as i64),
            ("ID_LED_ON1_OFF2", ID_LED_ON1_OFF2 as i64),
            ("ID_LED_OFF1_DEF2", ID_LED_OFF1_DEF2 as i64),
            ("ID_LED_OFF1_ON2", ID_LED_OFF1_ON2 as i64),
            ("ID_LED_OFF1_OFF2", ID_LED_OFF1_OFF2 as i64),
            ("IGP_ACTIVITY_LED_MASK", IGP_ACTIVITY_LED_MASK as i64),
            ("IGP_ACTIVITY_LED_ENABLE", IGP_ACTIVITY_LED_ENABLE as i64),
            ("IGP_LED3_MODE", IGP_LED3_MODE as i64),
            (
                "EEPROM_SERDES_AMPLITUDE_MASK",
                EEPROM_SERDES_AMPLITUDE_MASK as i64
            ),
            ("EEPROM_PHY_CLASS_A", EEPROM_PHY_CLASS_A as i64),
            ("EEPROM_WORD0A_ILOS", EEPROM_WORD0A_ILOS as i64),
            ("EEPROM_WORD0A_SWDPIO", EEPROM_WORD0A_SWDPIO as i64),
            ("EEPROM_WORD0A_LRST", EEPROM_WORD0A_LRST as i64),
            ("EEPROM_WORD0A_FD", EEPROM_WORD0A_FD as i64),
            ("EEPROM_WORD0A_66MHZ", EEPROM_WORD0A_66MHZ as i64),
            ("EEPROM_WORD0F_PAUSE_MASK", EEPROM_WORD0F_PAUSE_MASK as i64),
            ("EEPROM_WORD0F_PAUSE", EEPROM_WORD0F_PAUSE as i64),
            ("EEPROM_WORD0F_ASM_DIR", EEPROM_WORD0F_ASM_DIR as i64),
            ("EEPROM_WORD0F_ANE", EEPROM_WORD0F_ANE as i64),
            ("EEPROM_WORD0F_SWPDIO_EXT", EEPROM_WORD0F_SWPDIO_EXT as i64),
            ("EEPROM_WORD0F_LPLU", EEPROM_WORD0F_LPLU as i64),
            (
                "EEPROM_WORD1020_GIGA_DISABLE",
                EEPROM_WORD1020_GIGA_DISABLE as i64
            ),
            (
                "EEPROM_WORD1020_GIGA_DISABLE_NON_D0A",
                EEPROM_WORD1020_GIGA_DISABLE_NON_D0A as i64
            ),
            ("EEPROM_WORD1A_ASPM_MASK", EEPROM_WORD1A_ASPM_MASK as i64),
            ("EEPROM_SUM", EEPROM_SUM as i64),
            (
                "EEPROM_NODE_ADDRESS_BYTE_0",
                EEPROM_NODE_ADDRESS_BYTE_0 as i64
            ),
            ("EEPROM_PBA_BYTE_1", EEPROM_PBA_BYTE_1 as i64),
            ("EEPROM_RESERVED_WORD", EEPROM_RESERVED_WORD as i64),
            ("PBA_SIZE", PBA_SIZE as i64),
            (
                "E1000_COLLISION_THRESHOLD",
                E1000_COLLISION_THRESHOLD as i64
            ),
            ("E1000_CT_SHIFT", E1000_CT_SHIFT as i64),
            ("E1000_COLLISION_DISTANCE", E1000_COLLISION_DISTANCE as i64),
            (
                "E1000_COLLISION_DISTANCE_82542",
                E1000_COLLISION_DISTANCE_82542 as i64
            ),
            (
                "E1000_FDX_COLLISION_DISTANCE",
                E1000_FDX_COLLISION_DISTANCE as i64
            ),
            (
                "E1000_HDX_COLLISION_DISTANCE",
                E1000_HDX_COLLISION_DISTANCE as i64
            ),
            ("E1000_COLD_SHIFT", E1000_COLD_SHIFT as i64),
            (
                "REQ_TX_DESCRIPTOR_MULTIPLE",
                REQ_TX_DESCRIPTOR_MULTIPLE as i64
            ),
            (
                "REQ_RX_DESCRIPTOR_MULTIPLE",
                REQ_RX_DESCRIPTOR_MULTIPLE as i64
            ),
            ("DEFAULT_82542_TIPG_IPGT", DEFAULT_82542_TIPG_IPGT as i64),
            (
                "DEFAULT_82543_TIPG_IPGT_FIBER",
                DEFAULT_82543_TIPG_IPGT_FIBER as i64
            ),
            (
                "DEFAULT_82543_TIPG_IPGT_COPPER",
                DEFAULT_82543_TIPG_IPGT_COPPER as i64
            ),
            ("E1000_TIPG_IPGT_MASK", E1000_TIPG_IPGT_MASK as i64),
            ("E1000_TIPG_IPGR1_MASK", E1000_TIPG_IPGR1_MASK as i64),
            ("E1000_TIPG_IPGR2_MASK", E1000_TIPG_IPGR2_MASK as i64),
            ("DEFAULT_82542_TIPG_IPGR1", DEFAULT_82542_TIPG_IPGR1 as i64),
            ("DEFAULT_82543_TIPG_IPGR1", DEFAULT_82543_TIPG_IPGR1 as i64),
            ("E1000_TIPG_IPGR1_SHIFT", E1000_TIPG_IPGR1_SHIFT as i64),
            ("DEFAULT_82542_TIPG_IPGR2", DEFAULT_82542_TIPG_IPGR2 as i64),
            ("DEFAULT_82543_TIPG_IPGR2", DEFAULT_82543_TIPG_IPGR2 as i64),
            (
                "DEFAULT_80003ES2LAN_TIPG_IPGR2",
                DEFAULT_80003ES2LAN_TIPG_IPGR2 as i64
            ),
            ("E1000_TIPG_IPGR2_SHIFT", E1000_TIPG_IPGR2_SHIFT as i64),
            (
                "DEFAULT_80003ES2LAN_TIPG_IPGT_10_100",
                DEFAULT_80003ES2LAN_TIPG_IPGT_10_100 as i64
            ),
            (
                "DEFAULT_80003ES2LAN_TIPG_IPGT_1000",
                DEFAULT_80003ES2LAN_TIPG_IPGT_1000 as i64
            ),
            ("E1000_TXDMAC_DPP", E1000_TXDMAC_DPP as i64),
            ("TX_THRESHOLD_START", TX_THRESHOLD_START as i64),
            ("TX_THRESHOLD_INCREMENT", TX_THRESHOLD_INCREMENT as i64),
            ("TX_THRESHOLD_DECREMENT", TX_THRESHOLD_DECREMENT as i64),
            ("TX_THRESHOLD_STOP", TX_THRESHOLD_STOP as i64),
            ("TX_THRESHOLD_DISABLE", TX_THRESHOLD_DISABLE as i64),
            ("TX_THRESHOLD_TIMER_MS", TX_THRESHOLD_TIMER_MS as i64),
            ("MIN_NUM_XMITS", MIN_NUM_XMITS as i64),
            ("IFS_MAX", IFS_MAX as i64),
            ("IFS_STEP", IFS_STEP as i64),
            ("IFS_MIN", IFS_MIN as i64),
            ("IFS_RATIO", IFS_RATIO as i64),
            (
                "E1000_EXTCNF_CTRL_PCIE_WRITE_ENABLE",
                E1000_EXTCNF_CTRL_PCIE_WRITE_ENABLE as i64
            ),
            (
                "E1000_EXTCNF_CTRL_PHY_WRITE_ENABLE",
                E1000_EXTCNF_CTRL_PHY_WRITE_ENABLE as i64
            ),
            (
                "E1000_EXTCNF_CTRL_D_UD_ENABLE",
                E1000_EXTCNF_CTRL_D_UD_ENABLE as i64
            ),
            (
                "E1000_EXTCNF_CTRL_D_UD_LATENCY",
                E1000_EXTCNF_CTRL_D_UD_LATENCY as i64
            ),
            (
                "E1000_EXTCNF_CTRL_D_UD_OWNER",
                E1000_EXTCNF_CTRL_D_UD_OWNER as i64
            ),
            (
                "E1000_EXTCNF_CTRL_MDIO_SW_OWNERSHIP",
                E1000_EXTCNF_CTRL_MDIO_SW_OWNERSHIP as i64
            ),
            (
                "E1000_EXTCNF_CTRL_MDIO_HW_OWNERSHIP",
                E1000_EXTCNF_CTRL_MDIO_HW_OWNERSHIP as i64
            ),
            (
                "E1000_EXTCNF_CTRL_EXT_CNF_POINTER",
                E1000_EXTCNF_CTRL_EXT_CNF_POINTER as i64
            ),
            (
                "E1000_EXTCNF_SIZE_EXT_PHY_LENGTH",
                E1000_EXTCNF_SIZE_EXT_PHY_LENGTH as i64
            ),
            (
                "E1000_EXTCNF_SIZE_EXT_DOCK_LENGTH",
                E1000_EXTCNF_SIZE_EXT_DOCK_LENGTH as i64
            ),
            (
                "E1000_EXTCNF_SIZE_EXT_PCIE_LENGTH",
                E1000_EXTCNF_SIZE_EXT_PCIE_LENGTH as i64
            ),
            (
                "E1000_EXTCNF_CTRL_LCD_WRITE_ENABLE",
                E1000_EXTCNF_CTRL_LCD_WRITE_ENABLE as i64
            ),
            ("E1000_EXTCNF_CTRL_SWFLAG", E1000_EXTCNF_CTRL_SWFLAG as i64),
            (
                "E1000_EXTCNF_CTRL_GATE_PHY_CFG",
                E1000_EXTCNF_CTRL_GATE_PHY_CFG as i64
            ),
            ("E1000_PBA_8K", E1000_PBA_8K as i64),
            ("E1000_PBA_10K", E1000_PBA_10K as i64),
            ("E1000_PBA_12K", E1000_PBA_12K as i64),
            ("E1000_PBA_14K", E1000_PBA_14K as i64),
            ("E1000_PBA_16K", E1000_PBA_16K as i64),
            ("E1000_PBA_20K", E1000_PBA_20K as i64),
            ("E1000_PBA_22K", E1000_PBA_22K as i64),
            ("E1000_PBA_24K", E1000_PBA_24K as i64),
            ("E1000_PBA_26K", E1000_PBA_26K as i64),
            ("E1000_PBA_30K", E1000_PBA_30K as i64),
            ("E1000_PBA_32K", E1000_PBA_32K as i64),
            ("E1000_PBA_34K", E1000_PBA_34K as i64),
            ("E1000_PBA_38K", E1000_PBA_38K as i64),
            ("E1000_PBA_40K", E1000_PBA_40K as i64),
            ("E1000_PBA_48K", E1000_PBA_48K as i64),
            ("E1000_PBS_16K", E1000_PBS_16K as i64),
            ("FLOW_CONTROL_ADDRESS_LOW", FLOW_CONTROL_ADDRESS_LOW as i64),
            (
                "FLOW_CONTROL_ADDRESS_HIGH",
                FLOW_CONTROL_ADDRESS_HIGH as i64
            ),
            ("FLOW_CONTROL_TYPE", FLOW_CONTROL_TYPE as i64),
            ("FC_DEFAULT_HI_THRESH", FC_DEFAULT_HI_THRESH as i64),
            ("FC_DEFAULT_LO_THRESH", FC_DEFAULT_LO_THRESH as i64),
            ("FC_DEFAULT_TX_TIMER", FC_DEFAULT_TX_TIMER as i64),
            ("PCIX_COMMAND_REGISTER", PCIX_COMMAND_REGISTER as i64),
            ("PCIX_STATUS_REGISTER_LO", PCIX_STATUS_REGISTER_LO as i64),
            ("PCIX_STATUS_REGISTER_HI", PCIX_STATUS_REGISTER_HI as i64),
            ("PCIX_COMMAND_MMRBC_MASK", PCIX_COMMAND_MMRBC_MASK as i64),
            ("PCIX_COMMAND_MMRBC_SHIFT", PCIX_COMMAND_MMRBC_SHIFT as i64),
            (
                "PCIX_STATUS_HI_MMRBC_MASK",
                PCIX_STATUS_HI_MMRBC_MASK as i64
            ),
            (
                "PCIX_STATUS_HI_MMRBC_SHIFT",
                PCIX_STATUS_HI_MMRBC_SHIFT as i64
            ),
            ("PCIX_STATUS_HI_MMRBC_4K", PCIX_STATUS_HI_MMRBC_4K as i64),
            ("PCIX_STATUS_HI_MMRBC_2K", PCIX_STATUS_HI_MMRBC_2K as i64),
            ("PAUSE_SHIFT", PAUSE_SHIFT as i64),
            ("SWDPIO_SHIFT", SWDPIO_SHIFT as i64),
            ("SWDPIO__EXT_SHIFT", SWDPIO__EXT_SHIFT as i64),
            ("ILOS_SHIFT", ILOS_SHIFT as i64),
            (
                "RECEIVE_BUFFER_ALIGN_SIZE",
                RECEIVE_BUFFER_ALIGN_SIZE as i64
            ),
            ("LINK_UP_TIMEOUT", LINK_UP_TIMEOUT as i64),
            ("MASTER_DISABLE_TIMEOUT", MASTER_DISABLE_TIMEOUT as i64),
            ("AUTO_READ_DONE_TIMEOUT", AUTO_READ_DONE_TIMEOUT as i64),
            ("PHY_CFG_TIMEOUT", PHY_CFG_TIMEOUT as i64),
            ("SW_FLAG_TIMEOUT", SW_FLAG_TIMEOUT as i64),
            ("E1000_TX_BUFFER_SIZE", E1000_TX_BUFFER_SIZE as i64),
            ("CARRIER_EXTENSION", CARRIER_EXTENSION as i64),
            ("E1000_CTRL_PHY_RESET_DIR", E1000_CTRL_PHY_RESET_DIR as i64),
            ("E1000_CTRL_PHY_RESET", E1000_CTRL_PHY_RESET as i64),
            ("E1000_CTRL_MDIO_DIR", E1000_CTRL_MDIO_DIR as i64),
            ("E1000_CTRL_MDIO", E1000_CTRL_MDIO as i64),
            ("E1000_CTRL_MDC_DIR", E1000_CTRL_MDC_DIR as i64),
            ("E1000_CTRL_MDC", E1000_CTRL_MDC as i64),
            (
                "E1000_CTRL_PHY_RESET_DIR4",
                E1000_CTRL_PHY_RESET_DIR4 as i64
            ),
            ("E1000_CTRL_PHY_RESET4", E1000_CTRL_PHY_RESET4 as i64),
            ("PHY_CTRL", PHY_CTRL as i64),
            ("PHY_STATUS", PHY_STATUS as i64),
            ("PHY_ID1", PHY_ID1 as i64),
            ("PHY_ID2", PHY_ID2 as i64),
            ("PHY_AUTONEG_ADV", PHY_AUTONEG_ADV as i64),
            ("PHY_LP_ABILITY", PHY_LP_ABILITY as i64),
            ("PHY_AUTONEG_EXP", PHY_AUTONEG_EXP as i64),
            ("PHY_NEXT_PAGE_TX", PHY_NEXT_PAGE_TX as i64),
            ("PHY_LP_NEXT_PAGE", PHY_LP_NEXT_PAGE as i64),
            ("PHY_1000T_CTRL", PHY_1000T_CTRL as i64),
            ("PHY_1000T_STATUS", PHY_1000T_STATUS as i64),
            ("PHY_EXT_STATUS", PHY_EXT_STATUS as i64),
            ("MAX_PHY_REG_ADDRESS", MAX_PHY_REG_ADDRESS as i64),
            ("MAX_PHY_MULTI_PAGE_REG", MAX_PHY_MULTI_PAGE_REG as i64),
            ("M88E1000_PHY_SPEC_CTRL", M88E1000_PHY_SPEC_CTRL as i64),
            ("M88E1000_PHY_SPEC_STATUS", M88E1000_PHY_SPEC_STATUS as i64),
            ("M88E1000_INT_ENABLE", M88E1000_INT_ENABLE as i64),
            ("M88E1000_INT_STATUS", M88E1000_INT_STATUS as i64),
            (
                "M88E1000_EXT_PHY_SPEC_CTRL",
                M88E1000_EXT_PHY_SPEC_CTRL as i64
            ),
            ("M88E1000_RX_ERR_CNTR", M88E1000_RX_ERR_CNTR as i64),
            ("M88E1000_PHY_EXT_CTRL", M88E1000_PHY_EXT_CTRL as i64),
            ("M88E1000_PHY_PAGE_SELECT", M88E1000_PHY_PAGE_SELECT as i64),
            ("M88E1000_PHY_GEN_CONTROL", M88E1000_PHY_GEN_CONTROL as i64),
            (
                "M88E1000_PHY_VCO_REG_BIT8",
                M88E1000_PHY_VCO_REG_BIT8 as i64
            ),
            (
                "M88E1000_PHY_VCO_REG_BIT11",
                M88E1000_PHY_VCO_REG_BIT11 as i64
            ),
            ("M88E1543_PAGE_ADDR", M88E1543_PAGE_ADDR as i64),
            ("M88E1543_EEE_CTRL_1", M88E1543_EEE_CTRL_1 as i64),
            ("M88E1543_EEE_CTRL_1_MS", M88E1543_EEE_CTRL_1_MS as i64),
            ("M88E1512_CFG_REG_1", M88E1512_CFG_REG_1 as i64),
            ("M88E1512_CFG_REG_2", M88E1512_CFG_REG_2 as i64),
            ("M88E1512_CFG_REG_3", M88E1512_CFG_REG_3 as i64),
            ("M88E1512_MODE", M88E1512_MODE as i64),
            (
                "BME1000_PSCR_ENABLE_DOWNSHIFT",
                BME1000_PSCR_ENABLE_DOWNSHIFT as i64
            ),
            ("BM_PHY_PAGE_SELECT", BM_PHY_PAGE_SELECT as i64),
            ("BM_REG_BIAS1", BM_REG_BIAS1 as i64),
            ("BM_REG_BIAS2", BM_REG_BIAS2 as i64),
            ("BM_PORT_CTRL_PAGE", BM_PORT_CTRL_PAGE as i64),
            (
                "IGP01E1000_IEEE_REGS_PAGE",
                IGP01E1000_IEEE_REGS_PAGE as i64
            ),
            (
                "IGP01E1000_IEEE_RESTART_AUTONEG",
                IGP01E1000_IEEE_RESTART_AUTONEG as i64
            ),
            (
                "IGP01E1000_IEEE_FORCE_GIGA",
                IGP01E1000_IEEE_FORCE_GIGA as i64
            ),
            (
                "IGP01E1000_PHY_PORT_CONFIG",
                IGP01E1000_PHY_PORT_CONFIG as i64
            ),
            (
                "IGP01E1000_PHY_PORT_STATUS",
                IGP01E1000_PHY_PORT_STATUS as i64
            ),
            ("IGP01E1000_PHY_PORT_CTRL", IGP01E1000_PHY_PORT_CTRL as i64),
            (
                "IGP01E1000_PHY_LINK_HEALTH",
                IGP01E1000_PHY_LINK_HEALTH as i64
            ),
            ("IGP01E1000_GMII_FIFO", IGP01E1000_GMII_FIFO as i64),
            (
                "IGP01E1000_PHY_CHANNEL_QUALITY",
                IGP01E1000_PHY_CHANNEL_QUALITY as i64
            ),
            (
                "IGP02E1000_PHY_POWER_MGMT",
                IGP02E1000_PHY_POWER_MGMT as i64
            ),
            (
                "IGP01E1000_PHY_PAGE_SELECT",
                IGP01E1000_PHY_PAGE_SELECT as i64
            ),
            ("IGP01E1000_PHY_AGC_A", IGP01E1000_PHY_AGC_A as i64),
            ("IGP01E1000_PHY_AGC_B", IGP01E1000_PHY_AGC_B as i64),
            ("IGP01E1000_PHY_AGC_C", IGP01E1000_PHY_AGC_C as i64),
            ("IGP01E1000_PHY_AGC_D", IGP01E1000_PHY_AGC_D as i64),
            ("IGP02E1000_PHY_AGC_A", IGP02E1000_PHY_AGC_A as i64),
            ("IGP02E1000_PHY_AGC_B", IGP02E1000_PHY_AGC_B as i64),
            ("IGP02E1000_PHY_AGC_C", IGP02E1000_PHY_AGC_C as i64),
            ("IGP02E1000_PHY_AGC_D", IGP02E1000_PHY_AGC_D as i64),
            ("IGP01E1000_PHY_DSP_RESET", IGP01E1000_PHY_DSP_RESET as i64),
            ("IGP01E1000_PHY_DSP_SET", IGP01E1000_PHY_DSP_SET as i64),
            ("IGP01E1000_PHY_DSP_FFE", IGP01E1000_PHY_DSP_FFE as i64),
            (
                "IGP01E1000_PHY_CHANNEL_NUM",
                IGP01E1000_PHY_CHANNEL_NUM as i64
            ),
            (
                "IGP02E1000_PHY_CHANNEL_NUM",
                IGP02E1000_PHY_CHANNEL_NUM as i64
            ),
            (
                "IGP01E1000_PHY_AGC_PARAM_A",
                IGP01E1000_PHY_AGC_PARAM_A as i64
            ),
            (
                "IGP01E1000_PHY_AGC_PARAM_B",
                IGP01E1000_PHY_AGC_PARAM_B as i64
            ),
            (
                "IGP01E1000_PHY_AGC_PARAM_C",
                IGP01E1000_PHY_AGC_PARAM_C as i64
            ),
            (
                "IGP01E1000_PHY_AGC_PARAM_D",
                IGP01E1000_PHY_AGC_PARAM_D as i64
            ),
            (
                "IGP01E1000_PHY_EDAC_MU_INDEX",
                IGP01E1000_PHY_EDAC_MU_INDEX as i64
            ),
            (
                "IGP01E1000_PHY_EDAC_SIGN_EXT_9_BITS",
                IGP01E1000_PHY_EDAC_SIGN_EXT_9_BITS as i64
            ),
            (
                "IGP01E1000_PHY_ANALOG_TX_STATE",
                IGP01E1000_PHY_ANALOG_TX_STATE as i64
            ),
            (
                "IGP01E1000_PHY_ANALOG_CLASS_A",
                IGP01E1000_PHY_ANALOG_CLASS_A as i64
            ),
            (
                "IGP01E1000_PHY_FORCE_ANALOG_ENABLE",
                IGP01E1000_PHY_FORCE_ANALOG_ENABLE as i64
            ),
            (
                "IGP01E1000_PHY_DSP_FFE_CM_CP",
                IGP01E1000_PHY_DSP_FFE_CM_CP as i64
            ),
            (
                "IGP01E1000_PHY_DSP_FFE_DEFAULT",
                IGP01E1000_PHY_DSP_FFE_DEFAULT as i64
            ),
            (
                "IGP01E1000_PHY_PCS_INIT_REG",
                IGP01E1000_PHY_PCS_INIT_REG as i64
            ),
            (
                "IGP01E1000_PHY_PCS_CTRL_REG",
                IGP01E1000_PHY_PCS_CTRL_REG as i64
            ),
            (
                "IGP01E1000_ANALOG_REGS_PAGE",
                IGP01E1000_ANALOG_REGS_PAGE as i64
            ),
            ("I82580_ADDR_REG", I82580_ADDR_REG as i64),
            ("I82580_CFG_REG", I82580_CFG_REG as i64),
            (
                "I82580_CFG_ASSERT_CRS_ON_TX",
                I82580_CFG_ASSERT_CRS_ON_TX as i64
            ),
            (
                "I82580_CFG_ENABLE_DOWNSHIFT",
                I82580_CFG_ENABLE_DOWNSHIFT as i64
            ),
            ("I82580_CTRL_REG", I82580_CTRL_REG as i64),
            (
                "I82580_CTRL_DOWNSHIFT_MASK",
                I82580_CTRL_DOWNSHIFT_MASK as i64
            ),
            ("GG82563_PAGE_SHIFT", GG82563_PAGE_SHIFT as i64),
            ("GG82563_MIN_ALT_REG", GG82563_MIN_ALT_REG as i64),
            ("GG82563_PHY_SPEC_CTRL", GG82563_PHY_SPEC_CTRL as i64),
            ("GG82563_PHY_SPEC_STATUS", GG82563_PHY_SPEC_STATUS as i64),
            ("GG82563_PHY_INT_ENABLE", GG82563_PHY_INT_ENABLE as i64),
            (
                "GG82563_PHY_SPEC_STATUS_2",
                GG82563_PHY_SPEC_STATUS_2 as i64
            ),
            ("GG82563_PHY_RX_ERR_CNTR", GG82563_PHY_RX_ERR_CNTR as i64),
            ("GG82563_PHY_PAGE_SELECT", GG82563_PHY_PAGE_SELECT as i64),
            ("GG82563_PHY_SPEC_CTRL_2", GG82563_PHY_SPEC_CTRL_2 as i64),
            (
                "GG82563_PHY_PAGE_SELECT_ALT",
                GG82563_PHY_PAGE_SELECT_ALT as i64
            ),
            (
                "GG82563_PHY_TEST_CLK_CTRL",
                GG82563_PHY_TEST_CLK_CTRL as i64
            ),
            (
                "GG82563_PHY_MAC_SPEC_CTRL",
                GG82563_PHY_MAC_SPEC_CTRL as i64
            ),
            (
                "GG82563_PHY_MAC_SPEC_CTRL_2",
                GG82563_PHY_MAC_SPEC_CTRL_2 as i64
            ),
            ("GG82563_PHY_DSP_DISTANCE", GG82563_PHY_DSP_DISTANCE as i64),
            (
                "GG82563_PHY_KMRN_MODE_CTRL",
                GG82563_PHY_KMRN_MODE_CTRL as i64
            ),
            ("GG82563_PHY_PORT_RESET", GG82563_PHY_PORT_RESET as i64),
            ("GG82563_PHY_REVISION_ID", GG82563_PHY_REVISION_ID as i64),
            ("GG82563_PHY_DEVICE_ID", GG82563_PHY_DEVICE_ID as i64),
            (
                "GG82563_PHY_PWR_MGMT_CTRL",
                GG82563_PHY_PWR_MGMT_CTRL as i64
            ),
            (
                "GG82563_PHY_RATE_ADAPT_CTRL",
                GG82563_PHY_RATE_ADAPT_CTRL as i64
            ),
            (
                "GG82563_PHY_KMRN_FIFO_CTRL_STAT",
                GG82563_PHY_KMRN_FIFO_CTRL_STAT as i64
            ),
            ("GG82563_PHY_KMRN_CTRL", GG82563_PHY_KMRN_CTRL as i64),
            ("GG82563_PHY_INBAND_CTRL", GG82563_PHY_INBAND_CTRL as i64),
            (
                "GG82563_PHY_KMRN_DIAGNOSTIC",
                GG82563_PHY_KMRN_DIAGNOSTIC as i64
            ),
            ("GG82563_PHY_ACK_TIMEOUTS", GG82563_PHY_ACK_TIMEOUTS as i64),
            ("GG82563_PHY_ADV_ABILITY", GG82563_PHY_ADV_ABILITY as i64),
            (
                "GG82563_PHY_LINK_PARTNER_ADV_ABILITY",
                GG82563_PHY_LINK_PARTNER_ADV_ABILITY as i64
            ),
            (
                "GG82563_PHY_ADV_NEXT_PAGE",
                GG82563_PHY_ADV_NEXT_PAGE as i64
            ),
            (
                "GG82563_PHY_LINK_PARTNER_ADV_NEXT_PAGE",
                GG82563_PHY_LINK_PARTNER_ADV_NEXT_PAGE as i64
            ),
            ("GG82563_PHY_KMRN_MISC", GG82563_PHY_KMRN_MISC as i64),
            ("I82577_PHY_ADDR_REG", I82577_PHY_ADDR_REG as i64),
            ("I82577_PHY_CFG_REG", I82577_PHY_CFG_REG as i64),
            ("I82577_PHY_CTRL_REG", I82577_PHY_CTRL_REG as i64),
            (
                "I82577_PHY_CFG_ENABLE_CRS_ON_TX",
                I82577_PHY_CFG_ENABLE_CRS_ON_TX as i64
            ),
            (
                "I82577_PHY_CFG_ENABLE_DOWNSHIFT",
                I82577_PHY_CFG_ENABLE_DOWNSHIFT as i64
            ),
            ("I82578_PHY_ADDR_REG", I82578_PHY_ADDR_REG as i64),
            (
                "I82578_EPSCR_DOWNSHIFT_ENABLE",
                I82578_EPSCR_DOWNSHIFT_ENABLE as i64
            ),
            (
                "I82578_EPSCR_DOWNSHIFT_COUNTER_MASK",
                I82578_EPSCR_DOWNSHIFT_COUNTER_MASK as i64
            ),
            ("MII_CR_SPEED_SELECT_MSB", MII_CR_SPEED_SELECT_MSB as i64),
            ("MII_CR_COLL_TEST_ENABLE", MII_CR_COLL_TEST_ENABLE as i64),
            ("MII_CR_FULL_DUPLEX", MII_CR_FULL_DUPLEX as i64),
            ("MII_CR_RESTART_AUTO_NEG", MII_CR_RESTART_AUTO_NEG as i64),
            ("MII_CR_ISOLATE", MII_CR_ISOLATE as i64),
            ("MII_CR_POWER_DOWN", MII_CR_POWER_DOWN as i64),
            ("MII_CR_AUTO_NEG_EN", MII_CR_AUTO_NEG_EN as i64),
            ("MII_CR_SPEED_SELECT_LSB", MII_CR_SPEED_SELECT_LSB as i64),
            ("MII_CR_LOOPBACK", MII_CR_LOOPBACK as i64),
            ("MII_CR_RESET", MII_CR_RESET as i64),
            ("MII_SR_EXTENDED_CAPS", MII_SR_EXTENDED_CAPS as i64),
            ("MII_SR_JABBER_DETECT", MII_SR_JABBER_DETECT as i64),
            ("MII_SR_LINK_STATUS", MII_SR_LINK_STATUS as i64),
            ("MII_SR_AUTONEG_CAPS", MII_SR_AUTONEG_CAPS as i64),
            ("MII_SR_REMOTE_FAULT", MII_SR_REMOTE_FAULT as i64),
            ("MII_SR_AUTONEG_COMPLETE", MII_SR_AUTONEG_COMPLETE as i64),
            ("MII_SR_PREAMBLE_SUPPRESS", MII_SR_PREAMBLE_SUPPRESS as i64),
            ("MII_SR_EXTENDED_STATUS", MII_SR_EXTENDED_STATUS as i64),
            ("MII_SR_100T2_HD_CAPS", MII_SR_100T2_HD_CAPS as i64),
            ("MII_SR_100T2_FD_CAPS", MII_SR_100T2_FD_CAPS as i64),
            ("MII_SR_10T_HD_CAPS", MII_SR_10T_HD_CAPS as i64),
            ("MII_SR_10T_FD_CAPS", MII_SR_10T_FD_CAPS as i64),
            ("MII_SR_100X_HD_CAPS", MII_SR_100X_HD_CAPS as i64),
            ("MII_SR_100X_FD_CAPS", MII_SR_100X_FD_CAPS as i64),
            ("MII_SR_100T4_CAPS", MII_SR_100T4_CAPS as i64),
            ("NWAY_AR_SELECTOR_FIELD", NWAY_AR_SELECTOR_FIELD as i64),
            ("NWAY_AR_10T_HD_CAPS", NWAY_AR_10T_HD_CAPS as i64),
            ("NWAY_AR_10T_FD_CAPS", NWAY_AR_10T_FD_CAPS as i64),
            ("NWAY_AR_100TX_HD_CAPS", NWAY_AR_100TX_HD_CAPS as i64),
            ("NWAY_AR_100TX_FD_CAPS", NWAY_AR_100TX_FD_CAPS as i64),
            ("NWAY_AR_100T4_CAPS", NWAY_AR_100T4_CAPS as i64),
            ("NWAY_AR_PAUSE", NWAY_AR_PAUSE as i64),
            ("NWAY_AR_ASM_DIR", NWAY_AR_ASM_DIR as i64),
            ("NWAY_AR_REMOTE_FAULT", NWAY_AR_REMOTE_FAULT as i64),
            ("NWAY_AR_NEXT_PAGE", NWAY_AR_NEXT_PAGE as i64),
            ("NWAY_LPAR_SELECTOR_FIELD", NWAY_LPAR_SELECTOR_FIELD as i64),
            ("NWAY_LPAR_10T_HD_CAPS", NWAY_LPAR_10T_HD_CAPS as i64),
            ("NWAY_LPAR_10T_FD_CAPS", NWAY_LPAR_10T_FD_CAPS as i64),
            ("NWAY_LPAR_100TX_HD_CAPS", NWAY_LPAR_100TX_HD_CAPS as i64),
            ("NWAY_LPAR_100TX_FD_CAPS", NWAY_LPAR_100TX_FD_CAPS as i64),
            ("NWAY_LPAR_100T4_CAPS", NWAY_LPAR_100T4_CAPS as i64),
            ("NWAY_LPAR_PAUSE", NWAY_LPAR_PAUSE as i64),
            ("NWAY_LPAR_ASM_DIR", NWAY_LPAR_ASM_DIR as i64),
            ("NWAY_LPAR_REMOTE_FAULT", NWAY_LPAR_REMOTE_FAULT as i64),
            ("NWAY_LPAR_ACKNOWLEDGE", NWAY_LPAR_ACKNOWLEDGE as i64),
            ("NWAY_LPAR_NEXT_PAGE", NWAY_LPAR_NEXT_PAGE as i64),
            ("NWAY_ER_LP_NWAY_CAPS", NWAY_ER_LP_NWAY_CAPS as i64),
            ("NWAY_ER_PAGE_RXD", NWAY_ER_PAGE_RXD as i64),
            ("NWAY_ER_NEXT_PAGE_CAPS", NWAY_ER_NEXT_PAGE_CAPS as i64),
            (
                "NWAY_ER_LP_NEXT_PAGE_CAPS",
                NWAY_ER_LP_NEXT_PAGE_CAPS as i64
            ),
            ("NWAY_ER_PAR_DETECT_FAULT", NWAY_ER_PAR_DETECT_FAULT as i64),
            ("NPTX_MSG_CODE_FIELD", NPTX_MSG_CODE_FIELD as i64),
            ("NPTX_TOGGLE", NPTX_TOGGLE as i64),
            ("NPTX_ACKNOWLDGE2", NPTX_ACKNOWLDGE2 as i64),
            ("NPTX_MSG_PAGE", NPTX_MSG_PAGE as i64),
            ("NPTX_NEXT_PAGE", NPTX_NEXT_PAGE as i64),
            ("LP_RNPR_MSG_CODE_FIELD", LP_RNPR_MSG_CODE_FIELD as i64),
            ("LP_RNPR_TOGGLE", LP_RNPR_TOGGLE as i64),
            ("LP_RNPR_ACKNOWLDGE2", LP_RNPR_ACKNOWLDGE2 as i64),
            ("LP_RNPR_MSG_PAGE", LP_RNPR_MSG_PAGE as i64),
            ("LP_RNPR_ACKNOWLDGE", LP_RNPR_ACKNOWLDGE as i64),
            ("LP_RNPR_NEXT_PAGE", LP_RNPR_NEXT_PAGE as i64),
            ("CR_1000T_ASYM_PAUSE", CR_1000T_ASYM_PAUSE as i64),
            ("CR_1000T_HD_CAPS", CR_1000T_HD_CAPS as i64),
            ("CR_1000T_FD_CAPS", CR_1000T_FD_CAPS as i64),
            ("CR_1000T_REPEATER_DTE", CR_1000T_REPEATER_DTE as i64),
            ("CR_1000T_MS_VALUE", CR_1000T_MS_VALUE as i64),
            ("CR_1000T_MS_ENABLE", CR_1000T_MS_ENABLE as i64),
            (
                "CR_1000T_TEST_MODE_NORMAL",
                CR_1000T_TEST_MODE_NORMAL as i64
            ),
            ("CR_1000T_TEST_MODE_1", CR_1000T_TEST_MODE_1 as i64),
            ("CR_1000T_TEST_MODE_2", CR_1000T_TEST_MODE_2 as i64),
            ("CR_1000T_TEST_MODE_3", CR_1000T_TEST_MODE_3 as i64),
            ("CR_1000T_TEST_MODE_4", CR_1000T_TEST_MODE_4 as i64),
            ("SR_1000T_IDLE_ERROR_CNT", SR_1000T_IDLE_ERROR_CNT as i64),
            ("SR_1000T_ASYM_PAUSE_DIR", SR_1000T_ASYM_PAUSE_DIR as i64),
            ("SR_1000T_LP_HD_CAPS", SR_1000T_LP_HD_CAPS as i64),
            ("SR_1000T_LP_FD_CAPS", SR_1000T_LP_FD_CAPS as i64),
            (
                "SR_1000T_REMOTE_RX_STATUS",
                SR_1000T_REMOTE_RX_STATUS as i64
            ),
            ("SR_1000T_LOCAL_RX_STATUS", SR_1000T_LOCAL_RX_STATUS as i64),
            ("SR_1000T_MS_CONFIG_RES", SR_1000T_MS_CONFIG_RES as i64),
            ("SR_1000T_MS_CONFIG_FAULT", SR_1000T_MS_CONFIG_FAULT as i64),
            (
                "SR_1000T_REMOTE_RX_STATUS_SHIFT",
                SR_1000T_REMOTE_RX_STATUS_SHIFT as i64
            ),
            (
                "SR_1000T_LOCAL_RX_STATUS_SHIFT",
                SR_1000T_LOCAL_RX_STATUS_SHIFT as i64
            ),
            (
                "SR_1000T_PHY_EXCESSIVE_IDLE_ERR_COUNT",
                SR_1000T_PHY_EXCESSIVE_IDLE_ERR_COUNT as i64
            ),
            (
                "FFE_IDLE_ERR_COUNT_TIMEOUT_20",
                FFE_IDLE_ERR_COUNT_TIMEOUT_20 as i64
            ),
            (
                "FFE_IDLE_ERR_COUNT_TIMEOUT_100",
                FFE_IDLE_ERR_COUNT_TIMEOUT_100 as i64
            ),
            ("IEEE_ESR_1000T_HD_CAPS", IEEE_ESR_1000T_HD_CAPS as i64),
            ("IEEE_ESR_1000T_FD_CAPS", IEEE_ESR_1000T_FD_CAPS as i64),
            ("IEEE_ESR_1000X_HD_CAPS", IEEE_ESR_1000X_HD_CAPS as i64),
            ("IEEE_ESR_1000X_FD_CAPS", IEEE_ESR_1000X_FD_CAPS as i64),
            ("PHY_TX_POLARITY_MASK", PHY_TX_POLARITY_MASK as i64),
            ("PHY_TX_NORMAL_POLARITY", PHY_TX_NORMAL_POLARITY as i64),
            ("AUTO_POLARITY_DISABLE", AUTO_POLARITY_DISABLE as i64),
            (
                "M88E1000_PSCR_JABBER_DISABLE",
                M88E1000_PSCR_JABBER_DISABLE as i64
            ),
            (
                "M88E1000_PSCR_POLARITY_REVERSAL",
                M88E1000_PSCR_POLARITY_REVERSAL as i64
            ),
            ("M88E1000_PSCR_SQE_TEST", M88E1000_PSCR_SQE_TEST as i64),
            (
                "M88E1000_PSCR_CLK125_DISABLE",
                M88E1000_PSCR_CLK125_DISABLE as i64
            ),
            (
                "M88E1000_PSCR_MDI_MANUAL_MODE",
                M88E1000_PSCR_MDI_MANUAL_MODE as i64
            ),
            (
                "M88E1000_PSCR_MDIX_MANUAL_MODE",
                M88E1000_PSCR_MDIX_MANUAL_MODE as i64
            ),
            (
                "M88E1000_PSCR_AUTO_X_1000T",
                M88E1000_PSCR_AUTO_X_1000T as i64
            ),
            (
                "M88E1000_PSCR_AUTO_X_MODE",
                M88E1000_PSCR_AUTO_X_MODE as i64
            ),
            (
                "M88E1000_PSCR_10BT_EXT_DIST_ENABLE",
                M88E1000_PSCR_10BT_EXT_DIST_ENABLE as i64
            ),
            (
                "M88E1000_PSCR_MII_5BIT_ENABLE",
                M88E1000_PSCR_MII_5BIT_ENABLE as i64
            ),
            (
                "M88E1000_PSCR_SCRAMBLER_DISABLE",
                M88E1000_PSCR_SCRAMBLER_DISABLE as i64
            ),
            (
                "M88E1000_PSCR_FORCE_LINK_GOOD",
                M88E1000_PSCR_FORCE_LINK_GOOD as i64
            ),
            (
                "M88E1000_PSCR_ASSERT_CRS_ON_TX",
                M88E1000_PSCR_ASSERT_CRS_ON_TX as i64
            ),
            (
                "M88E1000_PSCR_POLARITY_REVERSAL_SHIFT",
                M88E1000_PSCR_POLARITY_REVERSAL_SHIFT as i64
            ),
            (
                "M88E1000_PSCR_AUTO_X_MODE_SHIFT",
                M88E1000_PSCR_AUTO_X_MODE_SHIFT as i64
            ),
            (
                "M88E1000_PSCR_10BT_EXT_DIST_ENABLE_SHIFT",
                M88E1000_PSCR_10BT_EXT_DIST_ENABLE_SHIFT as i64
            ),
            ("M88E1000_PSSR_JABBER", M88E1000_PSSR_JABBER as i64),
            (
                "M88E1000_PSSR_REV_POLARITY",
                M88E1000_PSSR_REV_POLARITY as i64
            ),
            ("M88E1000_PSSR_DOWNSHIFT", M88E1000_PSSR_DOWNSHIFT as i64),
            ("M88E1000_PSSR_MDIX", M88E1000_PSSR_MDIX as i64),
            (
                "M88E1000_PSSR_CABLE_LENGTH",
                M88E1000_PSSR_CABLE_LENGTH as i64
            ),
            ("M88E1000_PSSR_LINK", M88E1000_PSSR_LINK as i64),
            (
                "M88E1000_PSSR_SPD_DPLX_RESOLVED",
                M88E1000_PSSR_SPD_DPLX_RESOLVED as i64
            ),
            ("M88E1000_PSSR_PAGE_RCVD", M88E1000_PSSR_PAGE_RCVD as i64),
            ("M88E1000_PSSR_DPLX", M88E1000_PSSR_DPLX as i64),
            ("M88E1000_PSSR_SPEED", M88E1000_PSSR_SPEED as i64),
            ("M88E1000_PSSR_10MBS", M88E1000_PSSR_10MBS as i64),
            ("M88E1000_PSSR_100MBS", M88E1000_PSSR_100MBS as i64),
            ("M88E1000_PSSR_1000MBS", M88E1000_PSSR_1000MBS as i64),
            (
                "M88E1000_PSSR_REV_POLARITY_SHIFT",
                M88E1000_PSSR_REV_POLARITY_SHIFT as i64
            ),
            (
                "M88E1000_PSSR_DOWNSHIFT_SHIFT",
                M88E1000_PSSR_DOWNSHIFT_SHIFT as i64
            ),
            ("M88E1000_PSSR_MDIX_SHIFT", M88E1000_PSSR_MDIX_SHIFT as i64),
            (
                "M88E1000_PSSR_CABLE_LENGTH_SHIFT",
                M88E1000_PSSR_CABLE_LENGTH_SHIFT as i64
            ),
            (
                "M88E1000_EPSCR_FIBER_LOOPBACK",
                M88E1000_EPSCR_FIBER_LOOPBACK as i64
            ),
            (
                "M88E1000_EPSCR_DOWN_NO_IDLE",
                M88E1000_EPSCR_DOWN_NO_IDLE as i64
            ),
            (
                "M88E1000_EPSCR_MASTER_DOWNSHIFT_MASK",
                M88E1000_EPSCR_MASTER_DOWNSHIFT_MASK as i64
            ),
            (
                "M88E1000_EPSCR_MASTER_DOWNSHIFT_1X",
                M88E1000_EPSCR_MASTER_DOWNSHIFT_1X as i64
            ),
            (
                "M88E1000_EPSCR_MASTER_DOWNSHIFT_2X",
                M88E1000_EPSCR_MASTER_DOWNSHIFT_2X as i64
            ),
            (
                "M88E1000_EPSCR_MASTER_DOWNSHIFT_3X",
                M88E1000_EPSCR_MASTER_DOWNSHIFT_3X as i64
            ),
            (
                "M88E1000_EPSCR_MASTER_DOWNSHIFT_4X",
                M88E1000_EPSCR_MASTER_DOWNSHIFT_4X as i64
            ),
            (
                "M88E1000_EPSCR_SLAVE_DOWNSHIFT_MASK",
                M88E1000_EPSCR_SLAVE_DOWNSHIFT_MASK as i64
            ),
            (
                "M88E1000_EPSCR_SLAVE_DOWNSHIFT_DIS",
                M88E1000_EPSCR_SLAVE_DOWNSHIFT_DIS as i64
            ),
            (
                "M88E1000_EPSCR_SLAVE_DOWNSHIFT_1X",
                M88E1000_EPSCR_SLAVE_DOWNSHIFT_1X as i64
            ),
            (
                "M88E1000_EPSCR_SLAVE_DOWNSHIFT_2X",
                M88E1000_EPSCR_SLAVE_DOWNSHIFT_2X as i64
            ),
            (
                "M88E1000_EPSCR_SLAVE_DOWNSHIFT_3X",
                M88E1000_EPSCR_SLAVE_DOWNSHIFT_3X as i64
            ),
            (
                "M88E1000_EPSCR_TX_CLK_2_5",
                M88E1000_EPSCR_TX_CLK_2_5 as i64
            ),
            ("M88E1000_EPSCR_TX_CLK_25", M88E1000_EPSCR_TX_CLK_25 as i64),
            ("M88E1000_EPSCR_TX_CLK_0", M88E1000_EPSCR_TX_CLK_0 as i64),
            (
                "M88EC018_EPSCR_DOWNSHIFT_COUNTER_MASK",
                M88EC018_EPSCR_DOWNSHIFT_COUNTER_MASK as i64
            ),
            (
                "M88EC018_EPSCR_DOWNSHIFT_COUNTER_1X",
                M88EC018_EPSCR_DOWNSHIFT_COUNTER_1X as i64
            ),
            (
                "M88EC018_EPSCR_DOWNSHIFT_COUNTER_2X",
                M88EC018_EPSCR_DOWNSHIFT_COUNTER_2X as i64
            ),
            (
                "M88EC018_EPSCR_DOWNSHIFT_COUNTER_3X",
                M88EC018_EPSCR_DOWNSHIFT_COUNTER_3X as i64
            ),
            (
                "M88EC018_EPSCR_DOWNSHIFT_COUNTER_4X",
                M88EC018_EPSCR_DOWNSHIFT_COUNTER_4X as i64
            ),
            (
                "M88EC018_EPSCR_DOWNSHIFT_COUNTER_5X",
                M88EC018_EPSCR_DOWNSHIFT_COUNTER_5X as i64
            ),
            (
                "M88EC018_EPSCR_DOWNSHIFT_COUNTER_6X",
                M88EC018_EPSCR_DOWNSHIFT_COUNTER_6X as i64
            ),
            (
                "M88EC018_EPSCR_DOWNSHIFT_COUNTER_7X",
                M88EC018_EPSCR_DOWNSHIFT_COUNTER_7X as i64
            ),
            (
                "M88EC018_EPSCR_DOWNSHIFT_COUNTER_8X",
                M88EC018_EPSCR_DOWNSHIFT_COUNTER_8X as i64
            ),
            (
                "M88E1000_EPSCR_TX_TIME_CTRL",
                M88E1000_EPSCR_TX_TIME_CTRL as i64
            ),
            (
                "M88E1000_EPSCR_RX_TIME_CTRL",
                M88E1000_EPSCR_RX_TIME_CTRL as i64
            ),
            (
                "IGP01E1000_PSCFR_AUTO_MDIX_PAR_DETECT",
                IGP01E1000_PSCFR_AUTO_MDIX_PAR_DETECT as i64
            ),
            ("IGP01E1000_PSCFR_PRE_EN", IGP01E1000_PSCFR_PRE_EN as i64),
            (
                "IGP01E1000_PSCFR_SMART_SPEED",
                IGP01E1000_PSCFR_SMART_SPEED as i64
            ),
            (
                "IGP01E1000_PSCFR_DISABLE_TPLOOPBACK",
                IGP01E1000_PSCFR_DISABLE_TPLOOPBACK as i64
            ),
            (
                "IGP01E1000_PSCFR_DISABLE_JABBER",
                IGP01E1000_PSCFR_DISABLE_JABBER as i64
            ),
            (
                "IGP01E1000_PSCFR_DISABLE_TRANSMIT",
                IGP01E1000_PSCFR_DISABLE_TRANSMIT as i64
            ),
            (
                "IGP01E1000_PSSR_AUTONEG_FAILED",
                IGP01E1000_PSSR_AUTONEG_FAILED as i64
            ),
            (
                "IGP01E1000_PSSR_POLARITY_REVERSED",
                IGP01E1000_PSSR_POLARITY_REVERSED as i64
            ),
            (
                "IGP01E1000_PSSR_CABLE_LENGTH",
                IGP01E1000_PSSR_CABLE_LENGTH as i64
            ),
            (
                "IGP01E1000_PSSR_FULL_DUPLEX",
                IGP01E1000_PSSR_FULL_DUPLEX as i64
            ),
            ("IGP01E1000_PSSR_LINK_UP", IGP01E1000_PSSR_LINK_UP as i64),
            ("IGP01E1000_PSSR_MDIX", IGP01E1000_PSSR_MDIX as i64),
            (
                "IGP01E1000_PSSR_SPEED_MASK",
                IGP01E1000_PSSR_SPEED_MASK as i64
            ),
            (
                "IGP01E1000_PSSR_SPEED_10MBPS",
                IGP01E1000_PSSR_SPEED_10MBPS as i64
            ),
            (
                "IGP01E1000_PSSR_SPEED_100MBPS",
                IGP01E1000_PSSR_SPEED_100MBPS as i64
            ),
            (
                "IGP01E1000_PSSR_SPEED_1000MBPS",
                IGP01E1000_PSSR_SPEED_1000MBPS as i64
            ),
            (
                "IGP01E1000_PSSR_CABLE_LENGTH_SHIFT",
                IGP01E1000_PSSR_CABLE_LENGTH_SHIFT as i64
            ),
            (
                "IGP01E1000_PSSR_MDIX_SHIFT",
                IGP01E1000_PSSR_MDIX_SHIFT as i64
            ),
            (
                "IGP01E1000_PSCR_TP_LOOPBACK",
                IGP01E1000_PSCR_TP_LOOPBACK as i64
            ),
            (
                "IGP01E1000_PSCR_CORRECT_NC_SCMBLR",
                IGP01E1000_PSCR_CORRECT_NC_SCMBLR as i64
            ),
            (
                "IGP01E1000_PSCR_TEN_CRS_SELECT",
                IGP01E1000_PSCR_TEN_CRS_SELECT as i64
            ),
            (
                "IGP01E1000_PSCR_FLIP_CHIP",
                IGP01E1000_PSCR_FLIP_CHIP as i64
            ),
            (
                "IGP01E1000_PSCR_AUTO_MDIX",
                IGP01E1000_PSCR_AUTO_MDIX as i64
            ),
            (
                "IGP01E1000_PSCR_FORCE_MDI_MDIX",
                IGP01E1000_PSCR_FORCE_MDI_MDIX as i64
            ),
            (
                "IGP01E1000_PLHR_SS_DOWNGRADE",
                IGP01E1000_PLHR_SS_DOWNGRADE as i64
            ),
            (
                "IGP01E1000_PLHR_GIG_SCRAMBLER_ERROR",
                IGP01E1000_PLHR_GIG_SCRAMBLER_ERROR as i64
            ),
            (
                "IGP01E1000_PLHR_MASTER_FAULT",
                IGP01E1000_PLHR_MASTER_FAULT as i64
            ),
            (
                "IGP01E1000_PLHR_MASTER_RESOLUTION",
                IGP01E1000_PLHR_MASTER_RESOLUTION as i64
            ),
            (
                "IGP01E1000_PLHR_GIG_REM_RCVR_NOK",
                IGP01E1000_PLHR_GIG_REM_RCVR_NOK as i64
            ),
            (
                "IGP01E1000_PLHR_IDLE_ERROR_CNT_OFLOW",
                IGP01E1000_PLHR_IDLE_ERROR_CNT_OFLOW as i64
            ),
            (
                "IGP01E1000_PLHR_DATA_ERR_1",
                IGP01E1000_PLHR_DATA_ERR_1 as i64
            ),
            (
                "IGP01E1000_PLHR_DATA_ERR_0",
                IGP01E1000_PLHR_DATA_ERR_0 as i64
            ),
            (
                "IGP01E1000_PLHR_AUTONEG_FAULT",
                IGP01E1000_PLHR_AUTONEG_FAULT as i64
            ),
            (
                "IGP01E1000_PLHR_AUTONEG_ACTIVE",
                IGP01E1000_PLHR_AUTONEG_ACTIVE as i64
            ),
            (
                "IGP01E1000_PLHR_VALID_CHANNEL_D",
                IGP01E1000_PLHR_VALID_CHANNEL_D as i64
            ),
            (
                "IGP01E1000_PLHR_VALID_CHANNEL_C",
                IGP01E1000_PLHR_VALID_CHANNEL_C as i64
            ),
            (
                "IGP01E1000_PLHR_VALID_CHANNEL_B",
                IGP01E1000_PLHR_VALID_CHANNEL_B as i64
            ),
            (
                "IGP01E1000_PLHR_VALID_CHANNEL_A",
                IGP01E1000_PLHR_VALID_CHANNEL_A as i64
            ),
            ("IGP01E1000_MSE_CHANNEL_D", IGP01E1000_MSE_CHANNEL_D as i64),
            ("IGP01E1000_MSE_CHANNEL_C", IGP01E1000_MSE_CHANNEL_C as i64),
            ("IGP01E1000_MSE_CHANNEL_B", IGP01E1000_MSE_CHANNEL_B as i64),
            ("IGP01E1000_MSE_CHANNEL_A", IGP01E1000_MSE_CHANNEL_A as i64),
            ("IGP02E1000_PM_SPD", IGP02E1000_PM_SPD as i64),
            ("IGP02E1000_PM_D3_LPLU", IGP02E1000_PM_D3_LPLU as i64),
            ("IGP02E1000_PM_D0_LPLU", IGP02E1000_PM_D0_LPLU as i64),
            ("DSP_RESET_ENABLE", DSP_RESET_ENABLE as i64),
            ("DSP_RESET_DISABLE", DSP_RESET_DISABLE as i64),
            ("E1000_MAX_DSP_RESETS", E1000_MAX_DSP_RESETS as i64),
            (
                "IGP01E1000_AGC_LENGTH_SHIFT",
                IGP01E1000_AGC_LENGTH_SHIFT as i64
            ),
            (
                "IGP02E1000_AGC_LENGTH_SHIFT",
                IGP02E1000_AGC_LENGTH_SHIFT as i64
            ),
            (
                "IGP02E1000_AGC_LENGTH_MASK",
                IGP02E1000_AGC_LENGTH_MASK as i64
            ),
            (
                "IGP01E1000_AGC_LENGTH_TABLE_SIZE",
                IGP01E1000_AGC_LENGTH_TABLE_SIZE as i64
            ),
            (
                "IGP02E1000_AGC_LENGTH_TABLE_SIZE",
                IGP02E1000_AGC_LENGTH_TABLE_SIZE as i64
            ),
            ("IGP01E1000_AGC_RANGE", IGP01E1000_AGC_RANGE as i64),
            ("IGP02E1000_AGC_RANGE", IGP02E1000_AGC_RANGE as i64),
            (
                "IGP01E1000_PHY_POLARITY_MASK",
                IGP01E1000_PHY_POLARITY_MASK as i64
            ),
            ("IGP01E1000_GMII_FLEX_SPD", IGP01E1000_GMII_FLEX_SPD as i64),
            ("IGP01E1000_GMII_SPD", IGP01E1000_GMII_SPD as i64),
            (
                "IGP01E1000_ANALOG_SPARE_FUSE_STATUS",
                IGP01E1000_ANALOG_SPARE_FUSE_STATUS as i64
            ),
            (
                "IGP01E1000_ANALOG_FUSE_STATUS",
                IGP01E1000_ANALOG_FUSE_STATUS as i64
            ),
            (
                "IGP01E1000_ANALOG_FUSE_CONTROL",
                IGP01E1000_ANALOG_FUSE_CONTROL as i64
            ),
            (
                "IGP01E1000_ANALOG_FUSE_BYPASS",
                IGP01E1000_ANALOG_FUSE_BYPASS as i64
            ),
            (
                "IGP01E1000_ANALOG_FUSE_POLY_MASK",
                IGP01E1000_ANALOG_FUSE_POLY_MASK as i64
            ),
            (
                "IGP01E1000_ANALOG_FUSE_FINE_MASK",
                IGP01E1000_ANALOG_FUSE_FINE_MASK as i64
            ),
            (
                "IGP01E1000_ANALOG_FUSE_COARSE_MASK",
                IGP01E1000_ANALOG_FUSE_COARSE_MASK as i64
            ),
            (
                "IGP01E1000_ANALOG_SPARE_FUSE_ENABLED",
                IGP01E1000_ANALOG_SPARE_FUSE_ENABLED as i64
            ),
            (
                "IGP01E1000_ANALOG_FUSE_ENABLE_SW_CONTROL",
                IGP01E1000_ANALOG_FUSE_ENABLE_SW_CONTROL as i64
            ),
            (
                "IGP01E1000_ANALOG_FUSE_COARSE_THRESH",
                IGP01E1000_ANALOG_FUSE_COARSE_THRESH as i64
            ),
            (
                "IGP01E1000_ANALOG_FUSE_COARSE_10",
                IGP01E1000_ANALOG_FUSE_COARSE_10 as i64
            ),
            (
                "IGP01E1000_ANALOG_FUSE_FINE_1",
                IGP01E1000_ANALOG_FUSE_FINE_1 as i64
            ),
            (
                "IGP01E1000_ANALOG_FUSE_FINE_10",
                IGP01E1000_ANALOG_FUSE_FINE_10 as i64
            ),
            (
                "GG82563_PSCR_DISABLE_JABBER",
                GG82563_PSCR_DISABLE_JABBER as i64
            ),
            (
                "GG82563_PSCR_POLARITY_REVERSAL_DISABLE",
                GG82563_PSCR_POLARITY_REVERSAL_DISABLE as i64
            ),
            ("GG82563_PSCR_POWER_DOWN", GG82563_PSCR_POWER_DOWN as i64),
            (
                "GG82563_PSCR_COPPER_TRANSMITER_DISABLE",
                GG82563_PSCR_COPPER_TRANSMITER_DISABLE as i64
            ),
            (
                "GG82563_PSCR_CROSSOVER_MODE_MASK",
                GG82563_PSCR_CROSSOVER_MODE_MASK as i64
            ),
            (
                "GG82563_PSCR_CROSSOVER_MODE_MDI",
                GG82563_PSCR_CROSSOVER_MODE_MDI as i64
            ),
            (
                "GG82563_PSCR_CROSSOVER_MODE_MDIX",
                GG82563_PSCR_CROSSOVER_MODE_MDIX as i64
            ),
            (
                "GG82563_PSCR_CROSSOVER_MODE_AUTO",
                GG82563_PSCR_CROSSOVER_MODE_AUTO as i64
            ),
            (
                "GG82563_PSCR_ENALBE_EXTENDED_DISTANCE",
                GG82563_PSCR_ENALBE_EXTENDED_DISTANCE as i64
            ),
            (
                "GG82563_PSCR_ENERGY_DETECT_MASK",
                GG82563_PSCR_ENERGY_DETECT_MASK as i64
            ),
            (
                "GG82563_PSCR_ENERGY_DETECT_OFF",
                GG82563_PSCR_ENERGY_DETECT_OFF as i64
            ),
            (
                "GG82563_PSCR_ENERGY_DETECT_RX",
                GG82563_PSCR_ENERGY_DETECT_RX as i64
            ),
            (
                "GG82563_PSCR_ENERGY_DETECT_RX_TM",
                GG82563_PSCR_ENERGY_DETECT_RX_TM as i64
            ),
            (
                "GG82563_PSCR_FORCE_LINK_GOOD",
                GG82563_PSCR_FORCE_LINK_GOOD as i64
            ),
            (
                "GG82563_PSCR_DOWNSHIFT_ENABLE",
                GG82563_PSCR_DOWNSHIFT_ENABLE as i64
            ),
            (
                "GG82563_PSCR_DOWNSHIFT_COUNTER_MASK",
                GG82563_PSCR_DOWNSHIFT_COUNTER_MASK as i64
            ),
            (
                "GG82563_PSCR_DOWNSHIFT_COUNTER_SHIFT",
                GG82563_PSCR_DOWNSHIFT_COUNTER_SHIFT as i64
            ),
            ("GG82563_PSSR_JABBER", GG82563_PSSR_JABBER as i64),
            ("GG82563_PSSR_POLARITY", GG82563_PSSR_POLARITY as i64),
            ("GG82563_PSSR_LINK", GG82563_PSSR_LINK as i64),
            (
                "GG82563_PSSR_ENERGY_DETECT",
                GG82563_PSSR_ENERGY_DETECT as i64
            ),
            ("GG82563_PSSR_DOWNSHIFT", GG82563_PSSR_DOWNSHIFT as i64),
            (
                "GG82563_PSSR_CROSSOVER_STATUS",
                GG82563_PSSR_CROSSOVER_STATUS as i64
            ),
            (
                "GG82563_PSSR_RX_PAUSE_ENABLED",
                GG82563_PSSR_RX_PAUSE_ENABLED as i64
            ),
            (
                "GG82563_PSSR_TX_PAUSE_ENABLED",
                GG82563_PSSR_TX_PAUSE_ENABLED as i64
            ),
            ("GG82563_PSSR_LINK_UP", GG82563_PSSR_LINK_UP as i64),
            (
                "GG82563_PSSR_SPEED_DUPLEX_RESOLVED",
                GG82563_PSSR_SPEED_DUPLEX_RESOLVED as i64
            ),
            (
                "GG82563_PSSR_PAGE_RECEIVED",
                GG82563_PSSR_PAGE_RECEIVED as i64
            ),
            ("GG82563_PSSR_DUPLEX", GG82563_PSSR_DUPLEX as i64),
            ("GG82563_PSSR_SPEED_MASK", GG82563_PSSR_SPEED_MASK as i64),
            (
                "GG82563_PSSR_SPEED_10MBPS",
                GG82563_PSSR_SPEED_10MBPS as i64
            ),
            (
                "GG82563_PSSR_SPEED_100MBPS",
                GG82563_PSSR_SPEED_100MBPS as i64
            ),
            (
                "GG82563_PSSR_SPEED_1000MBPS",
                GG82563_PSSR_SPEED_1000MBPS as i64
            ),
            ("GG82563_PSSR2_JABBER", GG82563_PSSR2_JABBER as i64),
            (
                "GG82563_PSSR2_POLARITY_CHANGED",
                GG82563_PSSR2_POLARITY_CHANGED as i64
            ),
            (
                "GG82563_PSSR2_ENERGY_DETECT_CHANGED",
                GG82563_PSSR2_ENERGY_DETECT_CHANGED as i64
            ),
            (
                "GG82563_PSSR2_DOWNSHIFT_INTERRUPT",
                GG82563_PSSR2_DOWNSHIFT_INTERRUPT as i64
            ),
            (
                "GG82563_PSSR2_MDI_CROSSOVER_CHANGE",
                GG82563_PSSR2_MDI_CROSSOVER_CHANGE as i64
            ),
            (
                "GG82563_PSSR2_FALSE_CARRIER",
                GG82563_PSSR2_FALSE_CARRIER as i64
            ),
            (
                "GG82563_PSSR2_SYMBOL_ERROR",
                GG82563_PSSR2_SYMBOL_ERROR as i64
            ),
            (
                "GG82563_PSSR2_LINK_STATUS_CHANGED",
                GG82563_PSSR2_LINK_STATUS_CHANGED as i64
            ),
            (
                "GG82563_PSSR2_AUTO_NEG_COMPLETED",
                GG82563_PSSR2_AUTO_NEG_COMPLETED as i64
            ),
            (
                "GG82563_PSSR2_PAGE_RECEIVED",
                GG82563_PSSR2_PAGE_RECEIVED as i64
            ),
            (
                "GG82563_PSSR2_DUPLEX_CHANGED",
                GG82563_PSSR2_DUPLEX_CHANGED as i64
            ),
            (
                "GG82563_PSSR2_SPEED_CHANGED",
                GG82563_PSSR2_SPEED_CHANGED as i64
            ),
            (
                "GG82563_PSSR2_AUTO_NEG_ERROR",
                GG82563_PSSR2_AUTO_NEG_ERROR as i64
            ),
            (
                "GG82563_PSCR2_10BT_POLARITY_FORCE",
                GG82563_PSCR2_10BT_POLARITY_FORCE as i64
            ),
            (
                "GG82563_PSCR2_1000MB_TEST_SELECT_MASK",
                GG82563_PSCR2_1000MB_TEST_SELECT_MASK as i64
            ),
            (
                "GG82563_PSCR2_1000MB_TEST_SELECT_NORMAL",
                GG82563_PSCR2_1000MB_TEST_SELECT_NORMAL as i64
            ),
            (
                "GG82563_PSCR2_1000MB_TEST_SELECT_112NS",
                GG82563_PSCR2_1000MB_TEST_SELECT_112NS as i64
            ),
            (
                "GG82563_PSCR2_1000MB_TEST_SELECT_16NS",
                GG82563_PSCR2_1000MB_TEST_SELECT_16NS as i64
            ),
            (
                "GG82563_PSCR2_REVERSE_AUTO_NEG",
                GG82563_PSCR2_REVERSE_AUTO_NEG as i64
            ),
            (
                "GG82563_PSCR2_1000BT_DISABLE",
                GG82563_PSCR2_1000BT_DISABLE as i64
            ),
            (
                "GG82563_PSCR2_TRANSMITER_TYPE_MASK",
                GG82563_PSCR2_TRANSMITER_TYPE_MASK as i64
            ),
            (
                "GG82563_PSCR2_TRANSMITTER_TYPE_CLASS_B",
                GG82563_PSCR2_TRANSMITTER_TYPE_CLASS_B as i64
            ),
            (
                "GG82563_PSCR2_TRANSMITTER_TYPE_CLASS_A",
                GG82563_PSCR2_TRANSMITTER_TYPE_CLASS_A as i64
            ),
            ("GG82563_MSCR_TX_CLK_MASK", GG82563_MSCR_TX_CLK_MASK as i64),
            (
                "GG82563_MSCR_TX_CLK_10MBPS_2_5MHZ",
                GG82563_MSCR_TX_CLK_10MBPS_2_5MHZ as i64
            ),
            (
                "GG82563_MSCR_TX_CLK_100MBPS_25MHZ",
                GG82563_MSCR_TX_CLK_100MBPS_25MHZ as i64
            ),
            (
                "GG82563_MSCR_TX_CLK_1000MBPS_2_5MHZ",
                GG82563_MSCR_TX_CLK_1000MBPS_2_5MHZ as i64
            ),
            (
                "GG82563_MSCR_TX_CLK_1000MBPS_25MHZ",
                GG82563_MSCR_TX_CLK_1000MBPS_25MHZ as i64
            ),
            (
                "GG82563_MSCR_ASSERT_CRS_ON_TX",
                GG82563_MSCR_ASSERT_CRS_ON_TX as i64
            ),
            (
                "GG82563_DSPD_CABLE_LENGTH",
                GG82563_DSPD_CABLE_LENGTH as i64
            ),
            ("GG82563_KMCR_PHY_LEDS_EN", GG82563_KMCR_PHY_LEDS_EN as i64),
            (
                "GG82563_KMCR_FORCE_LINK_UP",
                GG82563_KMCR_FORCE_LINK_UP as i64
            ),
            (
                "GG82563_KMCR_SUPPRESS_SGMII_EPD_EXT",
                GG82563_KMCR_SUPPRESS_SGMII_EPD_EXT as i64
            ),
            (
                "GG82563_KMCR_MDIO_BUS_SPEED_SELECT_MASK",
                GG82563_KMCR_MDIO_BUS_SPEED_SELECT_MASK as i64
            ),
            (
                "GG82563_KMCR_MDIO_BUS_SPEED_SELECT",
                GG82563_KMCR_MDIO_BUS_SPEED_SELECT as i64
            ),
            (
                "GG82563_KMCR_PASS_FALSE_CARRIER",
                GG82563_KMCR_PASS_FALSE_CARRIER as i64
            ),
            (
                "GG82563_PMCR_ENABLE_ELECTRICAL_IDLE",
                GG82563_PMCR_ENABLE_ELECTRICAL_IDLE as i64
            ),
            (
                "GG82563_PMCR_DISABLE_PORT",
                GG82563_PMCR_DISABLE_PORT as i64
            ),
            (
                "GG82563_PMCR_DISABLE_SERDES",
                GG82563_PMCR_DISABLE_SERDES as i64
            ),
            (
                "GG82563_PMCR_REVERSE_AUTO_NEG",
                GG82563_PMCR_REVERSE_AUTO_NEG as i64
            ),
            (
                "GG82563_PMCR_DISABLE_1000_NON_D0",
                GG82563_PMCR_DISABLE_1000_NON_D0 as i64
            ),
            (
                "GG82563_PMCR_DISABLE_1000",
                GG82563_PMCR_DISABLE_1000 as i64
            ),
            (
                "GG82563_PMCR_REVERSE_AUTO_NEG_D0A",
                GG82563_PMCR_REVERSE_AUTO_NEG_D0A as i64
            ),
            (
                "GG82563_PMCR_FORCE_POWER_STATE",
                GG82563_PMCR_FORCE_POWER_STATE as i64
            ),
            (
                "GG82563_PMCR_PROGRAMMED_POWER_STATE_MASK",
                GG82563_PMCR_PROGRAMMED_POWER_STATE_MASK as i64
            ),
            (
                "GG82563_PMCR_PROGRAMMED_POWER_STATE_DR",
                GG82563_PMCR_PROGRAMMED_POWER_STATE_DR as i64
            ),
            (
                "GG82563_PMCR_PROGRAMMED_POWER_STATE_D0U",
                GG82563_PMCR_PROGRAMMED_POWER_STATE_D0U as i64
            ),
            (
                "GG82563_PMCR_PROGRAMMED_POWER_STATE_D0A",
                GG82563_PMCR_PROGRAMMED_POWER_STATE_D0A as i64
            ),
            (
                "GG82563_PMCR_PROGRAMMED_POWER_STATE_D3",
                GG82563_PMCR_PROGRAMMED_POWER_STATE_D3 as i64
            ),
            ("GG82563_ICR_DIS_PADDING", GG82563_ICR_DIS_PADDING as i64),
            ("M88_VENDOR", M88_VENDOR as i64),
            ("M88E1000_E_PHY_ID", M88E1000_E_PHY_ID as i64),
            ("M88E1000_I_PHY_ID", M88E1000_I_PHY_ID as i64),
            ("M88E1011_I_PHY_ID", M88E1011_I_PHY_ID as i64),
            ("IGP01E1000_I_PHY_ID", IGP01E1000_I_PHY_ID as i64),
            ("M88E1000_12_PHY_ID", M88E1000_12_PHY_ID as i64),
            ("M88E1000_14_PHY_ID", M88E1000_14_PHY_ID as i64),
            ("M88E1011_I_REV_4", M88E1011_I_REV_4 as i64),
            ("M88E1111_I_PHY_ID", M88E1111_I_PHY_ID as i64),
            ("M88E1112_E_PHY_ID", M88E1112_E_PHY_ID as i64),
            ("I347AT4_E_PHY_ID", I347AT4_E_PHY_ID as i64),
            ("L1LXT971A_PHY_ID", L1LXT971A_PHY_ID as i64),
            ("GG82563_E_PHY_ID", GG82563_E_PHY_ID as i64),
            ("BME1000_E_PHY_ID", BME1000_E_PHY_ID as i64),
            ("BME1000_E_PHY_ID_R2", BME1000_E_PHY_ID_R2 as i64),
            ("M88E1543_E_PHY_ID", M88E1543_E_PHY_ID as i64),
            ("I82577_E_PHY_ID", I82577_E_PHY_ID as i64),
            ("I82578_E_PHY_ID", I82578_E_PHY_ID as i64),
            ("I82579_E_PHY_ID", I82579_E_PHY_ID as i64),
            ("I217_E_PHY_ID", I217_E_PHY_ID as i64),
            ("I82580_I_PHY_ID", I82580_I_PHY_ID as i64),
            ("I350_I_PHY_ID", I350_I_PHY_ID as i64),
            ("I210_I_PHY_ID", I210_I_PHY_ID as i64),
            ("IGP04E1000_E_PHY_ID", IGP04E1000_E_PHY_ID as i64),
            ("M88E1141_E_PHY_ID", M88E1141_E_PHY_ID as i64),
            ("M88E1512_E_PHY_ID", M88E1512_E_PHY_ID as i64),
            ("PHY_PAGE_SHIFT", PHY_PAGE_SHIFT as i64),
            ("IGP3_PHY_PORT_CTRL", IGP3_PHY_PORT_CTRL as i64),
            ("IGP3_PHY_RATE_ADAPT_CTRL", IGP3_PHY_RATE_ADAPT_CTRL as i64),
            (
                "IGP3_KMRN_FIFO_CTRL_STATS",
                IGP3_KMRN_FIFO_CTRL_STATS as i64
            ),
            ("IGP3_KMRN_POWER_MNG_CTRL", IGP3_KMRN_POWER_MNG_CTRL as i64),
            ("IGP3_KMRN_INBAND_CTRL", IGP3_KMRN_INBAND_CTRL as i64),
            ("IGP3_KMRN_DIAG", IGP3_KMRN_DIAG as i64),
            (
                "IGP3_KMRN_DIAG_PCS_LOCK_LOSS",
                IGP3_KMRN_DIAG_PCS_LOCK_LOSS as i64
            ),
            ("IGP3_KMRN_ACK_TIMEOUT", IGP3_KMRN_ACK_TIMEOUT as i64),
            ("IGP3_VR_CTRL", IGP3_VR_CTRL as i64),
            ("IGP3_VR_CTRL_MODE_SHUT", IGP3_VR_CTRL_MODE_SHUT as i64),
            ("IGP3_VR_CTRL_MODE_MASK", IGP3_VR_CTRL_MODE_MASK as i64),
            ("IGP3_CAPABILITY", IGP3_CAPABILITY as i64),
            ("IGP3_CAP_INITIATE_TEAM", IGP3_CAP_INITIATE_TEAM as i64),
            ("IGP3_CAP_WFM", IGP3_CAP_WFM as i64),
            ("IGP3_CAP_ASF", IGP3_CAP_ASF as i64),
            ("IGP3_CAP_LPLU", IGP3_CAP_LPLU as i64),
            ("IGP3_CAP_DC_AUTO_SPEED", IGP3_CAP_DC_AUTO_SPEED as i64),
            ("IGP3_CAP_SPD", IGP3_CAP_SPD as i64),
            ("IGP3_CAP_MULT_QUEUE", IGP3_CAP_MULT_QUEUE as i64),
            ("IGP3_CAP_RSS", IGP3_CAP_RSS as i64),
            ("IGP3_CAP_8021PQ", IGP3_CAP_8021PQ as i64),
            ("IGP3_CAP_AMT_CB", IGP3_CAP_AMT_CB as i64),
            ("IGP3_PPC_JORDAN_EN", IGP3_PPC_JORDAN_EN as i64),
            (
                "IGP3_PPC_JORDAN_GIGA_SPEED",
                IGP3_PPC_JORDAN_GIGA_SPEED as i64
            ),
            (
                "IGP3_KMRN_PMC_EE_IDLE_LINK_DIS",
                IGP3_KMRN_PMC_EE_IDLE_LINK_DIS as i64
            ),
            (
                "IGP3_KMRN_PMC_K0S_ENTRY_LATENCY_MASK",
                IGP3_KMRN_PMC_K0S_ENTRY_LATENCY_MASK as i64
            ),
            (
                "IGP3_KMRN_PMC_K0S_MODE1_EN_GIGA",
                IGP3_KMRN_PMC_K0S_MODE1_EN_GIGA as i64
            ),
            (
                "IGP3_KMRN_PMC_K0S_MODE1_EN_100",
                IGP3_KMRN_PMC_K0S_MODE1_EN_100 as i64
            ),
            ("IGP3E1000_PHY_MISC_CTRL", IGP3E1000_PHY_MISC_CTRL as i64),
            (
                "IGP3_PHY_MISC_DUPLEX_MANUAL_SET",
                IGP3_PHY_MISC_DUPLEX_MANUAL_SET as i64
            ),
            ("IGP3_KMRN_EXT_CTRL", IGP3_KMRN_EXT_CTRL as i64),
            ("IGP3_KMRN_EC_DIS_INBAND", IGP3_KMRN_EC_DIS_INBAND as i64),
            ("IGP03E1000_E_PHY_ID", IGP03E1000_E_PHY_ID as i64),
            ("IFE_E_PHY_ID", IFE_E_PHY_ID as i64),
            ("IFE_PLUS_E_PHY_ID", IFE_PLUS_E_PHY_ID as i64),
            ("IFE_C_E_PHY_ID", IFE_C_E_PHY_ID as i64),
            (
                "IFE_PHY_EXTENDED_STATUS_CONTROL",
                IFE_PHY_EXTENDED_STATUS_CONTROL as i64
            ),
            ("IFE_PHY_SPECIAL_CONTROL", IFE_PHY_SPECIAL_CONTROL as i64),
            (
                "IFE_PHY_RCV_FALSE_CARRIER",
                IFE_PHY_RCV_FALSE_CARRIER as i64
            ),
            ("IFE_PHY_RCV_DISCONNECT", IFE_PHY_RCV_DISCONNECT as i64),
            ("IFE_PHY_RCV_ERROT_FRAME", IFE_PHY_RCV_ERROT_FRAME as i64),
            ("IFE_PHY_RCV_SYMBOL_ERR", IFE_PHY_RCV_SYMBOL_ERR as i64),
            ("IFE_PHY_PREM_EOF_ERR", IFE_PHY_PREM_EOF_ERR as i64),
            ("IFE_PHY_RCV_EOF_ERR", IFE_PHY_RCV_EOF_ERR as i64),
            ("IFE_PHY_TX_JABBER_DETECT", IFE_PHY_TX_JABBER_DETECT as i64),
            ("IFE_PHY_EQUALIZER", IFE_PHY_EQUALIZER as i64),
            (
                "IFE_PHY_SPECIAL_CONTROL_LED",
                IFE_PHY_SPECIAL_CONTROL_LED as i64
            ),
            ("IFE_PHY_MDIX_CONTROL", IFE_PHY_MDIX_CONTROL as i64),
            ("IFE_PHY_HWI_CONTROL", IFE_PHY_HWI_CONTROL as i64),
            (
                "IFE_PESC_REDUCED_POWER_DOWN_DISABLE",
                IFE_PESC_REDUCED_POWER_DOWN_DISABLE as i64
            ),
            (
                "IFE_PESC_100BTX_POWER_DOWN",
                IFE_PESC_100BTX_POWER_DOWN as i64
            ),
            (
                "IFE_PESC_10BTX_POWER_DOWN",
                IFE_PESC_10BTX_POWER_DOWN as i64
            ),
            (
                "IFE_PESC_POLARITY_REVERSED",
                IFE_PESC_POLARITY_REVERSED as i64
            ),
            ("IFE_PESC_PHY_ADDR_MASK", IFE_PESC_PHY_ADDR_MASK as i64),
            ("IFE_PESC_SPEED", IFE_PESC_SPEED as i64),
            ("IFE_PESC_DUPLEX", IFE_PESC_DUPLEX as i64),
            (
                "IFE_PESC_POLARITY_REVERSED_SHIFT",
                IFE_PESC_POLARITY_REVERSED_SHIFT as i64
            ),
            (
                "IFE_PSC_DISABLE_DYNAMIC_POWER_DOWN",
                IFE_PSC_DISABLE_DYNAMIC_POWER_DOWN as i64
            ),
            ("IFE_PSC_FORCE_POLARITY", IFE_PSC_FORCE_POLARITY as i64),
            (
                "IFE_PSC_AUTO_POLARITY_DISABLE",
                IFE_PSC_AUTO_POLARITY_DISABLE as i64
            ),
            (
                "IFE_PSC_JABBER_FUNC_DISABLE",
                IFE_PSC_JABBER_FUNC_DISABLE as i64
            ),
            (
                "IFE_PSC_FORCE_POLARITY_SHIFT",
                IFE_PSC_FORCE_POLARITY_SHIFT as i64
            ),
            (
                "IFE_PSC_AUTO_POLARITY_DISABLE_SHIFT",
                IFE_PSC_AUTO_POLARITY_DISABLE_SHIFT as i64
            ),
            ("IFE_PMC_AUTO_MDIX", IFE_PMC_AUTO_MDIX as i64),
            ("IFE_PMC_FORCE_MDIX", IFE_PMC_FORCE_MDIX as i64),
            ("IFE_PMC_MDIX_STATUS", IFE_PMC_MDIX_STATUS as i64),
            (
                "IFE_PMC_AUTO_MDIX_COMPLETE",
                IFE_PMC_AUTO_MDIX_COMPLETE as i64
            ),
            ("IFE_PMC_MDIX_MODE_SHIFT", IFE_PMC_MDIX_MODE_SHIFT as i64),
            (
                "IFE_PHC_MDIX_RESET_ALL_MASK",
                IFE_PHC_MDIX_RESET_ALL_MASK as i64
            ),
            ("IFE_PHC_HWI_ENABLE", IFE_PHC_HWI_ENABLE as i64),
            ("IFE_PHC_ABILITY_CHECK", IFE_PHC_ABILITY_CHECK as i64),
            ("IFE_PHC_TEST_EXEC", IFE_PHC_TEST_EXEC as i64),
            ("IFE_PHC_HIGHZ", IFE_PHC_HIGHZ as i64),
            ("IFE_PHC_LOWZ", IFE_PHC_LOWZ as i64),
            ("IFE_PHC_LOW_HIGH_Z_MASK", IFE_PHC_LOW_HIGH_Z_MASK as i64),
            ("IFE_PHC_DISTANCE_MASK", IFE_PHC_DISTANCE_MASK as i64),
            ("IFE_PHC_RESET_ALL_MASK", IFE_PHC_RESET_ALL_MASK as i64),
            ("IFE_PSCL_PROBE_MODE", IFE_PSCL_PROBE_MODE as i64),
            ("IFE_PSCL_PROBE_LEDS_OFF", IFE_PSCL_PROBE_LEDS_OFF as i64),
            ("IFE_PSCL_PROBE_LEDS_ON", IFE_PSCL_PROBE_LEDS_ON as i64),
            (
                "ICH_FLASH_COMMAND_TIMEOUT",
                ICH_FLASH_COMMAND_TIMEOUT as i64
            ),
            ("ICH_FLASH_ERASE_TIMEOUT", ICH_FLASH_ERASE_TIMEOUT as i64),
            (
                "ICH_FLASH_CYCLE_REPEAT_COUNT",
                ICH_FLASH_CYCLE_REPEAT_COUNT as i64
            ),
            ("ICH_FLASH_SEG_SIZE_256", ICH_FLASH_SEG_SIZE_256 as i64),
            ("ICH_FLASH_SEG_SIZE_4K", ICH_FLASH_SEG_SIZE_4K as i64),
            ("ICH_FLASH_SEG_SIZE_8K", ICH_FLASH_SEG_SIZE_8K as i64),
            ("ICH_FLASH_SEG_SIZE_64K", ICH_FLASH_SEG_SIZE_64K as i64),
            ("ICH_CYCLE_READ", ICH_CYCLE_READ as i64),
            ("ICH_CYCLE_RESERVED", ICH_CYCLE_RESERVED as i64),
            ("ICH_CYCLE_WRITE", ICH_CYCLE_WRITE as i64),
            ("ICH_CYCLE_ERASE", ICH_CYCLE_ERASE as i64),
            ("ICH_FLASH_GFPREG", ICH_FLASH_GFPREG as i64),
            ("ICH_FLASH_HSFSTS", ICH_FLASH_HSFSTS as i64),
            ("ICH_FLASH_HSFCTL", ICH_FLASH_HSFCTL as i64),
            ("ICH_FLASH_FADDR", ICH_FLASH_FADDR as i64),
            ("ICH_FLASH_FDATA0", ICH_FLASH_FDATA0 as i64),
            ("ICH_FLASH_FRACC", ICH_FLASH_FRACC as i64),
            ("ICH_FLASH_FREG0", ICH_FLASH_FREG0 as i64),
            ("ICH_FLASH_FREG1", ICH_FLASH_FREG1 as i64),
            ("ICH_FLASH_FREG2", ICH_FLASH_FREG2 as i64),
            ("ICH_FLASH_FREG3", ICH_FLASH_FREG3 as i64),
            ("ICH_FLASH_FPR0", ICH_FLASH_FPR0 as i64),
            ("ICH_FLASH_FPR1", ICH_FLASH_FPR1 as i64),
            ("ICH_FLASH_SSFSTS", ICH_FLASH_SSFSTS as i64),
            ("ICH_FLASH_SSFCTL", ICH_FLASH_SSFCTL as i64),
            ("ICH_FLASH_PREOP", ICH_FLASH_PREOP as i64),
            ("ICH_FLASH_OPTYPE", ICH_FLASH_OPTYPE as i64),
            ("ICH_FLASH_OPMENU", ICH_FLASH_OPMENU as i64),
            ("ICH_FLASH_REG_MAPSIZE", ICH_FLASH_REG_MAPSIZE as i64),
            ("ICH_FLASH_SECTOR_SIZE", ICH_FLASH_SECTOR_SIZE as i64),
            ("ICH_GFPREG_BASE_MASK", ICH_GFPREG_BASE_MASK as i64),
            (
                "ICH_FLASH_LINEAR_ADDR_MASK",
                ICH_FLASH_LINEAR_ADDR_MASK as i64
            ),
            (
                "ICH_FLASH_SECT_ADDR_SHIFT",
                ICH_FLASH_SECT_ADDR_SHIFT as i64
            ),
            ("PHY_PREAMBLE", PHY_PREAMBLE as i64),
            ("PHY_SOF", PHY_SOF as i64),
            ("PHY_OP_READ", PHY_OP_READ as i64),
            ("PHY_OP_WRITE", PHY_OP_WRITE as i64),
            ("PHY_TURNAROUND", PHY_TURNAROUND as i64),
            ("PHY_PREAMBLE_SIZE", PHY_PREAMBLE_SIZE as i64),
            ("MII_CR_SPEED_1000", MII_CR_SPEED_1000 as i64),
            ("MII_CR_SPEED_100", MII_CR_SPEED_100 as i64),
            ("MII_CR_SPEED_10", MII_CR_SPEED_10 as i64),
            ("E1000_PHY_ADDRESS", E1000_PHY_ADDRESS as i64),
            ("PHY_AUTO_NEG_TIME", PHY_AUTO_NEG_TIME as i64),
            ("PHY_FORCE_TIME", PHY_FORCE_TIME as i64),
            ("PHY_REVISION_MASK", PHY_REVISION_MASK as i64),
            ("DEVICE_SPEED_MASK", DEVICE_SPEED_MASK as i64),
            ("REG4_SPEED_MASK", REG4_SPEED_MASK as i64),
            ("REG9_SPEED_MASK", REG9_SPEED_MASK as i64),
            ("ADVERTISE_10_HALF", ADVERTISE_10_HALF as i64),
            ("ADVERTISE_10_FULL", ADVERTISE_10_FULL as i64),
            ("ADVERTISE_100_HALF", ADVERTISE_100_HALF as i64),
            ("ADVERTISE_100_FULL", ADVERTISE_100_FULL as i64),
            ("ADVERTISE_1000_HALF", ADVERTISE_1000_HALF as i64),
            ("ADVERTISE_1000_FULL", ADVERTISE_1000_FULL as i64),
            (
                "AUTONEG_ADVERTISE_SPEED_DEFAULT",
                AUTONEG_ADVERTISE_SPEED_DEFAULT as i64
            ),
            (
                "AUTONEG_ADVERTISE_10_100_ALL",
                AUTONEG_ADVERTISE_10_100_ALL as i64
            ),
            ("AUTONEG_ADVERTISE_10_ALL", AUTONEG_ADVERTISE_10_ALL as i64),
            (
                "EEPROM_CHECKSUM_REG_ICP_xxxx",
                EEPROM_CHECKSUM_REG_ICP_xxxx as i64
            ),
            ("PCI_CAP_ID_ST", PCI_CAP_ID_ST as i64),
            ("PCI_ST_SMIA_OFFSET", PCI_ST_SMIA_OFFSET as i64),
            ("E1000_IMC1", E1000_IMC1 as i64),
            ("E1000_IMC2", E1000_IMC2 as i64),
            ("E1000_82542_IMC1", E1000_82542_IMC1 as i64),
            ("E1000_82542_IMC2", E1000_82542_IMC2 as i64),
            ("E1000_NVM_K1_CONFIG", E1000_NVM_K1_CONFIG as i64),
            ("E1000_NVM_K1_ENABLE", E1000_NVM_K1_ENABLE as i64),
            ("E1000_KMRNCTRLSTA_OFFSET", E1000_KMRNCTRLSTA_OFFSET as i64),
            (
                "E1000_KMRNCTRLSTA_OFFSET_SHIFT",
                E1000_KMRNCTRLSTA_OFFSET_SHIFT as i64
            ),
            ("E1000_KMRNCTRLSTA_REN", E1000_KMRNCTRLSTA_REN as i64),
            (
                "E1000_KMRNCTRLSTA_DIAG_OFFSET",
                E1000_KMRNCTRLSTA_DIAG_OFFSET as i64
            ),
            (
                "E1000_KMRNCTRLSTA_TIMEOUTS",
                E1000_KMRNCTRLSTA_TIMEOUTS as i64
            ),
            (
                "E1000_KMRNCTRLSTA_INBAND_PARAM",
                E1000_KMRNCTRLSTA_INBAND_PARAM as i64
            ),
            (
                "E1000_KMRNCTRLSTA_DIAG_NELPBK",
                E1000_KMRNCTRLSTA_DIAG_NELPBK as i64
            ),
            (
                "E1000_KMRNCTRLSTA_K1_CONFIG",
                E1000_KMRNCTRLSTA_K1_CONFIG as i64
            ),
            (
                "E1000_KMRNCTRLSTA_K1_ENABLE",
                E1000_KMRNCTRLSTA_K1_ENABLE as i64
            ),
            (
                "E1000_EXTCNF_CTRL_OEM_WRITE_ENABLE",
                E1000_EXTCNF_CTRL_OEM_WRITE_ENABLE as i64
            ),
            (
                "E1000_EXTCNF_SIZE_EXT_PCIE_LENGTH_MASK",
                E1000_EXTCNF_SIZE_EXT_PCIE_LENGTH_MASK as i64
            ),
            (
                "E1000_EXTCNF_SIZE_EXT_PCIE_LENGTH_SHIFT",
                E1000_EXTCNF_SIZE_EXT_PCIE_LENGTH_SHIFT as i64
            ),
            (
                "E1000_EXTCNF_CTRL_EXT_CNF_POINTER_MASK",
                E1000_EXTCNF_CTRL_EXT_CNF_POINTER_MASK as i64
            ),
            (
                "E1000_EXTCNF_CTRL_EXT_CNF_POINTER_SHIFT",
                E1000_EXTCNF_CTRL_EXT_CNF_POINTER_SHIFT as i64
            ),
            ("CV_SMB_CTRL", CV_SMB_CTRL as i64),
            ("CV_SMB_CTRL_FORCE_SMBUS", CV_SMB_CTRL_FORCE_SMBUS as i64),
            ("I218_ULP_CONFIG1", I218_ULP_CONFIG1 as i64),
            ("I218_ULP_CONFIG1_START", I218_ULP_CONFIG1_START as i64),
            ("I218_ULP_CONFIG1_IND", I218_ULP_CONFIG1_IND as i64),
            (
                "I218_ULP_CONFIG1_STICKY_ULP",
                I218_ULP_CONFIG1_STICKY_ULP as i64
            ),
            (
                "I218_ULP_CONFIG1_INBAND_EXIT",
                I218_ULP_CONFIG1_INBAND_EXIT as i64
            ),
            (
                "I218_ULP_CONFIG1_WOL_HOST",
                I218_ULP_CONFIG1_WOL_HOST as i64
            ),
            (
                "I218_ULP_CONFIG1_RESET_TO_SMBUS",
                I218_ULP_CONFIG1_RESET_TO_SMBUS as i64
            ),
            (
                "I218_ULP_CONFIG1_EN_ULP_LANPHYPC",
                I218_ULP_CONFIG1_EN_ULP_LANPHYPC as i64
            ),
            (
                "I218_ULP_CONFIG1_DIS_CLR_STICKY_ON_PERST",
                I218_ULP_CONFIG1_DIS_CLR_STICKY_ON_PERST as i64
            ),
            (
                "I218_ULP_CONFIG1_DISABLE_SMB_PERST",
                I218_ULP_CONFIG1_DISABLE_SMB_PERST as i64
            ),
            ("HV_INTC_FC_PAGE_START", HV_INTC_FC_PAGE_START as i64),
            ("HV_SCC_UPPER", HV_SCC_UPPER as i64),
            ("HV_SCC_LOWER", HV_SCC_LOWER as i64),
            ("HV_ECOL_UPPER", HV_ECOL_UPPER as i64),
            ("HV_ECOL_LOWER", HV_ECOL_LOWER as i64),
            ("HV_MCC_UPPER", HV_MCC_UPPER as i64),
            ("HV_MCC_LOWER", HV_MCC_LOWER as i64),
            ("HV_LATECOL_UPPER", HV_LATECOL_UPPER as i64),
            ("HV_LATECOL_LOWER", HV_LATECOL_LOWER as i64),
            ("HV_COLC_UPPER", HV_COLC_UPPER as i64),
            ("HV_COLC_LOWER", HV_COLC_LOWER as i64),
            ("HV_DC_UPPER", HV_DC_UPPER as i64),
            ("HV_DC_LOWER", HV_DC_LOWER as i64),
            ("HV_TNCRS_UPPER", HV_TNCRS_UPPER as i64),
            ("HV_TNCRS_LOWER", HV_TNCRS_LOWER as i64),
            ("HV_OEM_BITS", HV_OEM_BITS as i64),
            ("HV_OEM_BITS_LPLU", HV_OEM_BITS_LPLU as i64),
            ("HV_OEM_BITS_GBE_DIS", HV_OEM_BITS_GBE_DIS as i64),
            ("HV_OEM_BITS_RESTART_AN", HV_OEM_BITS_RESTART_AN as i64),
            ("HV_MUX_DATA_CTRL", HV_MUX_DATA_CTRL as i64),
            (
                "HV_MUX_DATA_CTRL_GEN_TO_MAC",
                HV_MUX_DATA_CTRL_GEN_TO_MAC as i64
            ),
            (
                "HV_MUX_DATA_CTRL_FORCE_SPEED",
                HV_MUX_DATA_CTRL_FORCE_SPEED as i64
            ),
            ("HV_KMRN_MODE_CTRL", HV_KMRN_MODE_CTRL as i64),
            ("HV_KMRN_MDIO_SLOW", HV_KMRN_MDIO_SLOW as i64),
            ("HV_PM_CTRL", HV_PM_CTRL as i64),
            ("HV_PM_CTRL_K1_CLK_REQ", HV_PM_CTRL_K1_CLK_REQ as i64),
            ("HV_PM_CTRL_K1_ENABLE", HV_PM_CTRL_K1_ENABLE as i64),
            ("I2_DFT_CTRL", I2_DFT_CTRL as i64),
            ("I2_SMBUS_CTRL", I2_SMBUS_CTRL as i64),
            ("I2_MODE_CTRL", I2_MODE_CTRL as i64),
            ("I2_PCIE_POWER_CTRL", I2_PCIE_POWER_CTRL as i64),
            ("E1000_FEXTNVM7", E1000_FEXTNVM7 as i64),
            (
                "E1000_FEXTNVM7_SIDE_CLK_UNGATE",
                E1000_FEXTNVM7_SIDE_CLK_UNGATE as i64
            ),
            (
                "E1000_FEXTNVM7_DISABLE_SMB_PERST",
                E1000_FEXTNVM7_DISABLE_SMB_PERST as i64
            ),
            ("E1000_FEXTNVM9", E1000_FEXTNVM9 as i64),
            (
                "E1000_FEXTNVM9_IOSFSB_CLKGATE_DIS",
                E1000_FEXTNVM9_IOSFSB_CLKGATE_DIS as i64
            ),
            (
                "E1000_FEXTNVM9_IOSFSB_CLKREQ_DIS",
                E1000_FEXTNVM9_IOSFSB_CLKREQ_DIS as i64
            ),
            ("E1000_FEXTNVM11", E1000_FEXTNVM11 as i64),
            (
                "E1000_FEXTNVM11_DISABLE_MULR_FIX",
                E1000_FEXTNVM11_DISABLE_MULR_FIX as i64
            ),
            ("BM_PCIE_PAGE", BM_PCIE_PAGE as i64),
            ("BM_WUC_PAGE", BM_WUC_PAGE as i64),
            ("BM_WUC_ADDRESS_OPCODE", BM_WUC_ADDRESS_OPCODE as i64),
            ("BM_WUC_DATA_OPCODE", BM_WUC_DATA_OPCODE as i64),
            ("BM_WUC_ENABLE_PAGE", BM_WUC_ENABLE_PAGE as i64),
            ("BM_WUC_ENABLE_REG", BM_WUC_ENABLE_REG as i64),
            ("BM_WUC_ENABLE_BIT", BM_WUC_ENABLE_BIT as i64),
            ("BM_WUC_HOST_WU_BIT", BM_WUC_HOST_WU_BIT as i64),
            ("BM_CS_STATUS", BM_CS_STATUS as i64),
            (
                "BM_CS_STATUS_ENERGY_DETECT",
                BM_CS_STATUS_ENERGY_DETECT as i64
            ),
            ("BM_CS_STATUS_LINK_UP", BM_CS_STATUS_LINK_UP as i64),
            ("BM_CS_STATUS_RESOLVED", BM_CS_STATUS_RESOLVED as i64),
            ("BM_CS_STATUS_SPEED_MASK", BM_CS_STATUS_SPEED_MASK as i64),
            ("BM_CS_STATUS_SPEED_1000", BM_CS_STATUS_SPEED_1000 as i64),
            ("HV_M_STATUS", HV_M_STATUS as i64),
            (
                "HV_M_STATUS_AUTONEG_COMPLETE",
                HV_M_STATUS_AUTONEG_COMPLETE as i64
            ),
            ("HV_M_STATUS_SPEED_MASK", HV_M_STATUS_SPEED_MASK as i64),
            ("HV_M_STATUS_SPEED_1000", HV_M_STATUS_SPEED_1000 as i64),
            ("HV_M_STATUS_LINK_UP", HV_M_STATUS_LINK_UP as i64),
            ("I217_INBAND_CTRL", I217_INBAND_CTRL as i64),
            (
                "I217_INBAND_CTRL_LINK_STAT_TX_TIMEOUT_MASK",
                I217_INBAND_CTRL_LINK_STAT_TX_TIMEOUT_MASK as i64
            ),
            (
                "I217_INBAND_CTRL_LINK_STAT_TX_TIMEOUT_SHIFT",
                I217_INBAND_CTRL_LINK_STAT_TX_TIMEOUT_SHIFT as i64
            ),
            ("E1000_PHY_TIMEOUTS_REG", E1000_PHY_TIMEOUTS_REG as i64),
            (
                "E1000_PHY_TIMEOUTS_K1_EXIT_TO_MASK",
                E1000_PHY_TIMEOUTS_K1_EXIT_TO_MASK as i64
            ),
            ("I82579_LPI_CTRL", I82579_LPI_CTRL as i64),
            (
                "I82579_LPI_CTRL_ENABLE_MASK",
                I82579_LPI_CTRL_ENABLE_MASK as i64
            ),
            (
                "I82579_LPI_CTRL_FORCE_PLL_LOCK_COUNT",
                I82579_LPI_CTRL_FORCE_PLL_LOCK_COUNT as i64
            ),
            ("I82579_EMI_ADDR", I82579_EMI_ADDR as i64),
            ("I82579_EMI_DATA", I82579_EMI_DATA as i64),
            ("I82579_LPI_UPDATE_TIMER", I82579_LPI_UPDATE_TIMER as i64),
            ("I82579_MSE_THRESHOLD", I82579_MSE_THRESHOLD as i64),
            ("I82579_MSE_LINK_DOWN", I82579_MSE_LINK_DOWN as i64),
            ("INVM_SIZE", INVM_SIZE as i64),
            (
                "NVM_INIT_CTRL_2_DEFAULT_I211",
                NVM_INIT_CTRL_2_DEFAULT_I211 as i64
            ),
            (
                "NVM_INIT_CTRL_4_DEFAULT_I211",
                NVM_INIT_CTRL_4_DEFAULT_I211 as i64
            ),
            (
                "NVM_LED_1_CFG_DEFAULT_I211",
                NVM_LED_1_CFG_DEFAULT_I211 as i64
            ),
            (
                "NVM_LED_0_2_CFG_DEFAULT_I211",
                NVM_LED_0_2_CFG_DEFAULT_I211 as i64
            ),
            ("NVM_RESERVED_WORD", NVM_RESERVED_WORD as i64),
            (
                "INVM_UNINITIALIZED_STRUCTURE",
                INVM_UNINITIALIZED_STRUCTURE as i64
            ),
            (
                "INVM_WORD_AUTOLOAD_STRUCTURE",
                INVM_WORD_AUTOLOAD_STRUCTURE as i64
            ),
            (
                "INVM_CSR_AUTOLOAD_STRUCTURE",
                INVM_CSR_AUTOLOAD_STRUCTURE as i64
            ),
            (
                "INVM_PHY_REGISTER_AUTOLOAD_STRUCTURE",
                INVM_PHY_REGISTER_AUTOLOAD_STRUCTURE as i64
            ),
            (
                "INVM_RSA_KEY_SHA256_STRUCTURE",
                INVM_RSA_KEY_SHA256_STRUCTURE as i64
            ),
            (
                "INVM_INVALIDATED_STRUCTURE",
                INVM_INVALIDATED_STRUCTURE as i64
            ),
            (
                "INVM_RSA_KEY_SHA256_DATA_SIZE_IN_DWORDS",
                INVM_RSA_KEY_SHA256_DATA_SIZE_IN_DWORDS as i64
            ),
            (
                "INVM_CSR_AUTOLOAD_DATA_SIZE_IN_DWORDS",
                INVM_CSR_AUTOLOAD_DATA_SIZE_IN_DWORDS as i64
            ),
            ("PHY_UPPER_SHIFT", PHY_UPPER_SHIFT as i64),
            (
                "E1000_SFF_IDENTIFIER_OFFSET",
                E1000_SFF_IDENTIFIER_OFFSET as i64
            ),
            ("E1000_SFF_IDENTIFIER_SFF", E1000_SFF_IDENTIFIER_SFF as i64),
            ("E1000_SFF_IDENTIFIER_SFP", E1000_SFF_IDENTIFIER_SFP as i64),
            (
                "E1000_SFF_ETH_FLAGS_OFFSET",
                E1000_SFF_ETH_FLAGS_OFFSET as i64
            ),
            (
                "E1000_SFF_VENDOR_OUI_TYCO",
                E1000_SFF_VENDOR_OUI_TYCO as i64
            ),
            ("E1000_SFF_VENDOR_OUI_FTL", E1000_SFF_VENDOR_OUI_FTL as i64),
            (
                "E1000_SFF_VENDOR_OUI_AVAGO",
                E1000_SFF_VENDOR_OUI_AVAGO as i64
            ),
            (
                "E1000_SFF_VENDOR_OUI_INTEL",
                E1000_SFF_VENDOR_OUI_INTEL as i64
            ),
        ]
    }

    /// Every define of the header against the C (`just test-ref`): the value where the reader
    /// can evaluate it (literals, other defines, `|`, `<<`, `+`, `-`, `*`), and that nothing in
    /// the header is left out.
    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn defines_match_reference() {
        let defs = crate::reftest::defines("sys/dev/pci/if_em_hw.h");
        let ours = header_defines();
        let mut checked = 0;
        for (name, value) in &ours {
            let text = defs
                .get(*name)
                .unwrap_or_else(|| panic!("{name} is not in the header"));
            match crate::reftest::int(&defs, name) {
                Some(v) => {
                    assert_eq!(v, *value, "{name}");
                    checked += 1;
                }
                // Multi-line defines, casts and the PHY_REG()/GG82563_REG() forms are beyond
                // the reader; the host tests check the macros themselves.
                None => assert!(
                    text.contains('(')
                        || text.ends_with('\\')
                        || text.is_empty()
                        || defs.contains_key(text.as_str()),
                    "{name} = {text} was not evaluated"
                ),
            }
        }
        assert!(checked > 2000, "only {checked} defines checked");
        for name in defs.keys() {
            let known = ours.iter().any(|(n, _)| n == name)
                || matches!(
                    name.as_str(),
                    "_EM_HW_H_" | "E1000_RDBAL0" | "E1000_RDBAH0" | "E1000_RDLEN0"
                );
            assert!(known, "{name} is not ported");
        }
    }

    /// The two IGP cable length tables against the C's initialisers.
    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn cable_length_tables_match_reference() {
        let path = crate::reftest::openbsd_src().join("sys/dev/pci/if_em_hw.c");
        let text =
            std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let table = |name: &str| -> std::vec::Vec<u16> {
            let at = text
                .find(&std::format!("{name}["))
                .unwrap_or_else(|| panic!("{name}"));
            let open = at + text[at..].find('{').expect("{");
            let close = open + text[open..].find('}').expect("}");
            text[open + 1..close]
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| s.parse().expect("number"))
                .collect()
        };
        assert_eq!(
            table("em_igp_cable_length_table"),
            EM_IGP_CABLE_LENGTH_TABLE.to_vec()
        );
        assert_eq!(
            table("em_igp_2_cable_length_table"),
            EM_IGP_2_CABLE_LENGTH_TABLE.to_vec()
        );
    }
}
/* </TESTS> */
