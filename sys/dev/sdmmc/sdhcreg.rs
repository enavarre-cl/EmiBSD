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
 * Copyright (c) 2006 Uwe Stuehler <uwe@openbsd.org>
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
/* </LICENSES> */

/* <CODE> */
//! `<dev/sdmmc/sdhcreg.h>`: the SD Host Controller Standard register set, its PCI interface
//! classes and the ADMA2 descriptors.
//!
//! Upstream: sys/dev/sdmmc/sdhcreg.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - Register offsets are `BusSize`; a register's bits take the register's width (`u8` for
//!   the 8-bit registers `HREAD1` reads, `u16` for the 16-bit ones, `u32` for the 32-bit
//!   ones); shift counts are `u32`.
//! - The function-like macros are functions with the macros' names in lower case
//!   (`SDHC_SPEC_VERSION(hcv)` is [`sdhc_spec_version`]).
//! - The `%b` format strings (`SDHC_*_BITS`) are byte strings; they serve the debug output
//!   only (`SDHC_DEBUG`, not configured).
//! - The ADMA2 descriptors are `#[repr(C, packed)]`, as the C's `__packed` structures.

#![allow(clippy::identity_op)] // the constants' `(value << 0)`, kept as the C writes them

use crate::dev::pci::pcireg::{PCI_MAPREG_END, PCI_MAPREG_START};
use crate::machine::bus::BusSize;

/* PCI base address registers */

/// `SDHC_PCI_BAR_START`.
pub const SDHC_PCI_BAR_START: i32 = PCI_MAPREG_START;
/// `SDHC_PCI_BAR_END`.
pub const SDHC_PCI_BAR_END: i32 = PCI_MAPREG_END;

/* PCI interface classes */

/// `SDHC_PCI_INTERFACE_NO_DMA`.
pub const SDHC_PCI_INTERFACE_NO_DMA: u32 = 0x00;
/// `SDHC_PCI_INTERFACE_DMA`.
pub const SDHC_PCI_INTERFACE_DMA: u32 = 0x01;
/// `SDHC_PCI_INTERFACE_VENDOR`.
pub const SDHC_PCI_INTERFACE_VENDOR: u32 = 0x02;

/* Host standard register set */

/// `SDHC_DMA_ADDR`.
pub const SDHC_DMA_ADDR: BusSize = 0x00;
/// `SDHC_BLOCK_SIZE`.
pub const SDHC_BLOCK_SIZE: BusSize = 0x04;
/// `SDHC_BLOCK_COUNT`.
pub const SDHC_BLOCK_COUNT: BusSize = 0x06;
/// `SDHC_BLOCK_COUNT_MAX`.
pub const SDHC_BLOCK_COUNT_MAX: u16 = 512;
/// `SDHC_ARGUMENT`.
pub const SDHC_ARGUMENT: BusSize = 0x08;
/// `SDHC_TRANSFER_MODE`.
pub const SDHC_TRANSFER_MODE: BusSize = 0x0c;
/// `SDHC_MULTI_BLOCK_MODE`.
pub const SDHC_MULTI_BLOCK_MODE: u16 = 1 << 5;
/// `SDHC_READ_MODE`.
pub const SDHC_READ_MODE: u16 = 1 << 4;
/// `SDHC_AUTO_CMD12_ENABLE`.
pub const SDHC_AUTO_CMD12_ENABLE: u16 = 1 << 2;
/// `SDHC_BLOCK_COUNT_ENABLE`.
pub const SDHC_BLOCK_COUNT_ENABLE: u16 = 1 << 1;
/// `SDHC_DMA_ENABLE`.
pub const SDHC_DMA_ENABLE: u16 = 1 << 0;
/// `SDHC_COMMAND`.
pub const SDHC_COMMAND: BusSize = 0x0e;
/* 14-15 reserved */
/// `SDHC_COMMAND_INDEX_SHIFT`.
pub const SDHC_COMMAND_INDEX_SHIFT: u32 = 8;
/// `SDHC_COMMAND_INDEX_MASK`.
pub const SDHC_COMMAND_INDEX_MASK: u16 = 0x3f;
/// `SDHC_COMMAND_TYPE_ABORT`.
pub const SDHC_COMMAND_TYPE_ABORT: u16 = 3 << 6;
/// `SDHC_COMMAND_TYPE_RESUME`.
pub const SDHC_COMMAND_TYPE_RESUME: u16 = 2 << 6;
/// `SDHC_COMMAND_TYPE_SUSPEND`.
pub const SDHC_COMMAND_TYPE_SUSPEND: u16 = 1 << 6;
/// `SDHC_COMMAND_TYPE_NORMAL`.
pub const SDHC_COMMAND_TYPE_NORMAL: u16 = 0 << 6;
/// `SDHC_DATA_PRESENT_SELECT`.
pub const SDHC_DATA_PRESENT_SELECT: u16 = 1 << 5;
/// `SDHC_INDEX_CHECK_ENABLE`.
pub const SDHC_INDEX_CHECK_ENABLE: u16 = 1 << 4;
/// `SDHC_CRC_CHECK_ENABLE`.
pub const SDHC_CRC_CHECK_ENABLE: u16 = 1 << 3;
/* 2 reserved */
/// `SDHC_RESP_LEN_48_CHK_BUSY`.
pub const SDHC_RESP_LEN_48_CHK_BUSY: u16 = 3 << 0;
/// `SDHC_RESP_LEN_48`.
pub const SDHC_RESP_LEN_48: u16 = 2 << 0;
/// `SDHC_RESP_LEN_136`.
pub const SDHC_RESP_LEN_136: u16 = 1 << 0;
/// `SDHC_NO_RESPONSE`.
pub const SDHC_NO_RESPONSE: u16 = 0 << 0;
/// `SDHC_RESPONSE`: 0x10 - 0x1f.
pub const SDHC_RESPONSE: BusSize = 0x10;
/// `SDHC_DATA`.
pub const SDHC_DATA: BusSize = 0x20;
/// `SDHC_PRESENT_STATE`.
pub const SDHC_PRESENT_STATE: BusSize = 0x24;
/* 25-31 reserved */
/// `SDHC_CMD_LINE_SIGNAL_LEVEL`.
pub const SDHC_CMD_LINE_SIGNAL_LEVEL: u32 = 1 << 24;
/// `SDHC_DAT3_LINE_LEVEL`.
pub const SDHC_DAT3_LINE_LEVEL: u32 = 1 << 23;
/// `SDHC_DAT2_LINE_LEVEL`.
pub const SDHC_DAT2_LINE_LEVEL: u32 = 1 << 22;
/// `SDHC_DAT1_LINE_LEVEL`.
pub const SDHC_DAT1_LINE_LEVEL: u32 = 1 << 21;
/// `SDHC_DAT0_LINE_LEVEL`.
pub const SDHC_DAT0_LINE_LEVEL: u32 = 1 << 20;
/// `SDHC_WRITE_PROTECT_SWITCH`.
pub const SDHC_WRITE_PROTECT_SWITCH: u32 = 1 << 19;
/// `SDHC_CARD_DETECT_PIN_LEVEL`.
pub const SDHC_CARD_DETECT_PIN_LEVEL: u32 = 1 << 18;
/// `SDHC_CARD_STATE_STABLE`.
pub const SDHC_CARD_STATE_STABLE: u32 = 1 << 17;
/// `SDHC_CARD_INSERTED`.
pub const SDHC_CARD_INSERTED: u32 = 1 << 16;
/* 12-15 reserved */
/// `SDHC_BUFFER_READ_ENABLE`.
pub const SDHC_BUFFER_READ_ENABLE: u32 = 1 << 11;
/// `SDHC_BUFFER_WRITE_ENABLE`.
pub const SDHC_BUFFER_WRITE_ENABLE: u32 = 1 << 10;
/// `SDHC_READ_TRANSFER_ACTIVE`.
pub const SDHC_READ_TRANSFER_ACTIVE: u32 = 1 << 9;
/// `SDHC_WRITE_TRANSFER_ACTIVE`.
pub const SDHC_WRITE_TRANSFER_ACTIVE: u32 = 1 << 8;
/* 3-7 reserved */
/// `SDHC_DAT_ACTIVE`.
pub const SDHC_DAT_ACTIVE: u32 = 1 << 2;
/// `SDHC_CMD_INHIBIT_DAT`.
pub const SDHC_CMD_INHIBIT_DAT: u32 = 1 << 1;
/// `SDHC_CMD_INHIBIT_CMD`.
pub const SDHC_CMD_INHIBIT_CMD: u32 = 1 << 0;
/// `SDHC_CMD_INHIBIT_MASK`.
pub const SDHC_CMD_INHIBIT_MASK: u32 = 0x0003;
/// `SDHC_HOST_CTL`.
pub const SDHC_HOST_CTL: BusSize = 0x28;
/// `SDHC_8BIT_MODE`.
pub const SDHC_8BIT_MODE: u8 = 1 << 5;
/// `SDHC_DMA_SELECT`.
pub const SDHC_DMA_SELECT: u8 = 3 << 3;
/// `SDHC_DMA_SELECT_SDMA`.
pub const SDHC_DMA_SELECT_SDMA: u8 = 0 << 3;
/// `SDHC_DMA_SELECT_ADMA32`.
pub const SDHC_DMA_SELECT_ADMA32: u8 = 2 << 3;
/// `SDHC_DMA_SELECT_ADMA64`.
pub const SDHC_DMA_SELECT_ADMA64: u8 = 3 << 3;
/// `SDHC_HIGH_SPEED`.
pub const SDHC_HIGH_SPEED: u8 = 1 << 2;
/// `SDHC_4BIT_MODE`.
pub const SDHC_4BIT_MODE: u8 = 1 << 1;
/// `SDHC_LED_ON`.
pub const SDHC_LED_ON: u8 = 1 << 0;
/// `SDHC_POWER_CTL`.
pub const SDHC_POWER_CTL: BusSize = 0x29;
/// `SDHC_VOLTAGE_SHIFT`.
pub const SDHC_VOLTAGE_SHIFT: u32 = 1;
/// `SDHC_VOLTAGE_MASK`.
pub const SDHC_VOLTAGE_MASK: u8 = 0x07;
/// `SDHC_VOLTAGE_3_3V`.
pub const SDHC_VOLTAGE_3_3V: u8 = 0x07;
/// `SDHC_VOLTAGE_3_0V`.
pub const SDHC_VOLTAGE_3_0V: u8 = 0x06;
/// `SDHC_VOLTAGE_1_8V`.
pub const SDHC_VOLTAGE_1_8V: u8 = 0x05;
/// `SDHC_BUS_POWER`.
pub const SDHC_BUS_POWER: u8 = 1 << 0;
/// `SDHC_BLOCK_GAP_CTL`.
pub const SDHC_BLOCK_GAP_CTL: BusSize = 0x2a;
/// `SDHC_WAKEUP_CTL`.
pub const SDHC_WAKEUP_CTL: BusSize = 0x2b;
/// `SDHC_CLOCK_CTL`.
pub const SDHC_CLOCK_CTL: BusSize = 0x2c;
/// `SDHC_SDCLK_DIV_SHIFT`.
pub const SDHC_SDCLK_DIV_SHIFT: u32 = 8;
/// `SDHC_SDCLK_DIV_MASK`.
pub const SDHC_SDCLK_DIV_MASK: i32 = 0xff;
/// `SDHC_SDCLK_DIV_RSHIFT_V3`.
pub const SDHC_SDCLK_DIV_RSHIFT_V3: u32 = 2;
/// `SDHC_SDCLK_DIV_MASK_V3`.
pub const SDHC_SDCLK_DIV_MASK_V3: i32 = 0x300;
/// `SDHC_SDCLK_ENABLE`.
pub const SDHC_SDCLK_ENABLE: u16 = 1 << 2;
/// `SDHC_INTCLK_STABLE`.
pub const SDHC_INTCLK_STABLE: u16 = 1 << 1;
/// `SDHC_INTCLK_ENABLE`.
pub const SDHC_INTCLK_ENABLE: u16 = 1 << 0;
/// `SDHC_TIMEOUT_CTL`.
pub const SDHC_TIMEOUT_CTL: BusSize = 0x2e;
/// `SDHC_TIMEOUT_MAX`.
pub const SDHC_TIMEOUT_MAX: u8 = 0x0e;
/// `SDHC_SOFTWARE_RESET`.
pub const SDHC_SOFTWARE_RESET: BusSize = 0x2f;
/// `SDHC_RESET_MASK`.
pub const SDHC_RESET_MASK: u8 = 0x5;
/// `SDHC_RESET_DAT`.
pub const SDHC_RESET_DAT: u8 = 1 << 2;
/// `SDHC_RESET_CMD`.
pub const SDHC_RESET_CMD: u8 = 1 << 1;
/// `SDHC_RESET_ALL`.
pub const SDHC_RESET_ALL: u8 = 1 << 0;
/// `SDHC_NINTR_STATUS`.
pub const SDHC_NINTR_STATUS: BusSize = 0x30;
/// `SDHC_ERROR_INTERRUPT`.
pub const SDHC_ERROR_INTERRUPT: u16 = 1 << 15;
/// `SDHC_RETUNING_EVENT`.
pub const SDHC_RETUNING_EVENT: u16 = 1 << 12;
/// `SDHC_CARD_INTERRUPT`.
pub const SDHC_CARD_INTERRUPT: u16 = 1 << 8;
/// `SDHC_CARD_REMOVAL`.
pub const SDHC_CARD_REMOVAL: u16 = 1 << 7;
/// `SDHC_CARD_INSERTION`.
pub const SDHC_CARD_INSERTION: u16 = 1 << 6;
/// `SDHC_BUFFER_READ_READY`.
pub const SDHC_BUFFER_READ_READY: u16 = 1 << 5;
/// `SDHC_BUFFER_WRITE_READY`.
pub const SDHC_BUFFER_WRITE_READY: u16 = 1 << 4;
/// `SDHC_DMA_INTERRUPT`.
pub const SDHC_DMA_INTERRUPT: u16 = 1 << 3;
/// `SDHC_BLOCK_GAP_EVENT`.
pub const SDHC_BLOCK_GAP_EVENT: u16 = 1 << 2;
/// `SDHC_TRANSFER_COMPLETE`.
pub const SDHC_TRANSFER_COMPLETE: u16 = 1 << 1;
/// `SDHC_COMMAND_COMPLETE`.
pub const SDHC_COMMAND_COMPLETE: u16 = 1 << 0;
/// `SDHC_NINTR_STATUS_MASK`.
pub const SDHC_NINTR_STATUS_MASK: u16 = 0x91ff;
/// `SDHC_EINTR_STATUS`.
pub const SDHC_EINTR_STATUS: BusSize = 0x32;
/// `SDHC_ADMA_ERROR`.
pub const SDHC_ADMA_ERROR: u16 = 1 << 9;
/// `SDHC_AUTO_CMD12_ERROR`.
pub const SDHC_AUTO_CMD12_ERROR: u16 = 1 << 8;
/// `SDHC_CURRENT_LIMIT_ERROR`.
pub const SDHC_CURRENT_LIMIT_ERROR: u16 = 1 << 7;
/// `SDHC_DATA_END_BIT_ERROR`.
pub const SDHC_DATA_END_BIT_ERROR: u16 = 1 << 6;
/// `SDHC_DATA_CRC_ERROR`.
pub const SDHC_DATA_CRC_ERROR: u16 = 1 << 5;
/// `SDHC_DATA_TIMEOUT_ERROR`.
pub const SDHC_DATA_TIMEOUT_ERROR: u16 = 1 << 4;
/// `SDHC_CMD_INDEX_ERROR`.
pub const SDHC_CMD_INDEX_ERROR: u16 = 1 << 3;
/// `SDHC_CMD_END_BIT_ERROR`.
pub const SDHC_CMD_END_BIT_ERROR: u16 = 1 << 2;
/// `SDHC_CMD_CRC_ERROR`.
pub const SDHC_CMD_CRC_ERROR: u16 = 1 << 1;
/// `SDHC_CMD_TIMEOUT_ERROR`.
pub const SDHC_CMD_TIMEOUT_ERROR: u16 = 1 << 0;
/// `SDHC_EINTR_STATUS_MASK`: excluding vendor signals.
pub const SDHC_EINTR_STATUS_MASK: u16 = 0x03ff;
/// `SDHC_NINTR_STATUS_EN`.
pub const SDHC_NINTR_STATUS_EN: BusSize = 0x34;
/// `SDHC_EINTR_STATUS_EN`.
pub const SDHC_EINTR_STATUS_EN: BusSize = 0x36;
/// `SDHC_NINTR_SIGNAL_EN`.
pub const SDHC_NINTR_SIGNAL_EN: BusSize = 0x38;
/// `SDHC_NINTR_SIGNAL_MASK`.
pub const SDHC_NINTR_SIGNAL_MASK: u16 = 0x01ff;
/// `SDHC_EINTR_SIGNAL_EN`.
pub const SDHC_EINTR_SIGNAL_EN: BusSize = 0x3a;
/// `SDHC_EINTR_SIGNAL_MASK`: excluding vendor signals.
pub const SDHC_EINTR_SIGNAL_MASK: u16 = 0x03ff;
/// `SDHC_CMD12_ERROR_STATUS`.
pub const SDHC_CMD12_ERROR_STATUS: BusSize = 0x3c;
/// `SDHC_HOST_CTL2`.
pub const SDHC_HOST_CTL2: BusSize = 0x3e;
/// `SDHC_SAMPLING_CLOCK_SEL`.
pub const SDHC_SAMPLING_CLOCK_SEL: u16 = 1 << 7;
/// `SDHC_EXECUTE_TUNING`.
pub const SDHC_EXECUTE_TUNING: u16 = 1 << 6;
/// `SDHC_1_8V_SIGNAL_EN`.
pub const SDHC_1_8V_SIGNAL_EN: u16 = 1 << 3;
/// `SDHC_UHS_MODE_SELECT_SHIFT`.
pub const SDHC_UHS_MODE_SELECT_SHIFT: u32 = 0;
/// `SDHC_UHS_MODE_SELECT_MASK`.
pub const SDHC_UHS_MODE_SELECT_MASK: u16 = 0x7;
/// `SDHC_UHS_MODE_SELECT_SDR12`.
pub const SDHC_UHS_MODE_SELECT_SDR12: u16 = 0;
/// `SDHC_UHS_MODE_SELECT_SDR25`.
pub const SDHC_UHS_MODE_SELECT_SDR25: u16 = 1;
/// `SDHC_UHS_MODE_SELECT_SDR50`.
pub const SDHC_UHS_MODE_SELECT_SDR50: u16 = 2;
/// `SDHC_UHS_MODE_SELECT_SDR104`.
pub const SDHC_UHS_MODE_SELECT_SDR104: u16 = 3;
/// `SDHC_UHS_MODE_SELECT_DDR50`.
pub const SDHC_UHS_MODE_SELECT_DDR50: u16 = 4;
/// `SDHC_CAPABILITIES`.
pub const SDHC_CAPABILITIES: BusSize = 0x40;
/// `SDHC_64BIT_DMA_SUPP`.
pub const SDHC_64BIT_DMA_SUPP: u32 = 1 << 28;
/// `SDHC_VOLTAGE_SUPP_1_8V`.
pub const SDHC_VOLTAGE_SUPP_1_8V: u32 = 1 << 26;
/// `SDHC_VOLTAGE_SUPP_3_0V`.
pub const SDHC_VOLTAGE_SUPP_3_0V: u32 = 1 << 25;
/// `SDHC_VOLTAGE_SUPP_3_3V`.
pub const SDHC_VOLTAGE_SUPP_3_3V: u32 = 1 << 24;
/// `SDHC_SDMA_SUPP`.
pub const SDHC_SDMA_SUPP: u32 = 1 << 22;
/// `SDHC_HIGH_SPEED_SUPP`.
pub const SDHC_HIGH_SPEED_SUPP: u32 = 1 << 21;
/// `SDHC_ADMA2_SUPP`.
pub const SDHC_ADMA2_SUPP: u32 = 1 << 19;
/// `SDHC_8BIT_MODE_SUPP`.
pub const SDHC_8BIT_MODE_SUPP: u32 = 1 << 18;
/// `SDHC_MAX_BLK_LEN_512`.
pub const SDHC_MAX_BLK_LEN_512: u32 = 0;
/// `SDHC_MAX_BLK_LEN_1024`.
pub const SDHC_MAX_BLK_LEN_1024: u32 = 1;
/// `SDHC_MAX_BLK_LEN_2048`.
pub const SDHC_MAX_BLK_LEN_2048: u32 = 2;
/// `SDHC_MAX_BLK_LEN_SHIFT`.
pub const SDHC_MAX_BLK_LEN_SHIFT: u32 = 16;
/// `SDHC_MAX_BLK_LEN_MASK`.
pub const SDHC_MAX_BLK_LEN_MASK: u32 = 0x3;
/// `SDHC_BASE_FREQ_SHIFT`.
pub const SDHC_BASE_FREQ_SHIFT: u32 = 8;
/// `SDHC_BASE_FREQ_MASK`.
pub const SDHC_BASE_FREQ_MASK: u32 = 0x3f;
/// `SDHC_BASE_FREQ_MASK_V3`.
pub const SDHC_BASE_FREQ_MASK_V3: u32 = 0xff;
/// `SDHC_TIMEOUT_FREQ_UNIT`: 0=KHz, 1=MHz.
pub const SDHC_TIMEOUT_FREQ_UNIT: u32 = 1 << 7;
/// `SDHC_TIMEOUT_FREQ_SHIFT`.
pub const SDHC_TIMEOUT_FREQ_SHIFT: u32 = 0;
/// `SDHC_TIMEOUT_FREQ_MASK`.
pub const SDHC_TIMEOUT_FREQ_MASK: u32 = 0x1f;
/// `SDHC_CAPABILITIES2`.
pub const SDHC_CAPABILITIES2: BusSize = 0x44;
/// `SDHC_SDR50_SUPP`.
pub const SDHC_SDR50_SUPP: u32 = 1 << 0;
/// `SDHC_SDR104_SUPP`.
pub const SDHC_SDR104_SUPP: u32 = 1 << 1;
/// `SDHC_DDR50_SUPP`.
pub const SDHC_DDR50_SUPP: u32 = 1 << 2;
/// `SDHC_DRIVER_TYPE_A`.
pub const SDHC_DRIVER_TYPE_A: u32 = 1 << 4;
/// `SDHC_DRIVER_TYPE_C`.
pub const SDHC_DRIVER_TYPE_C: u32 = 1 << 5;
/// `SDHC_DRIVER_TYPE_D`.
pub const SDHC_DRIVER_TYPE_D: u32 = 1 << 6;
/// `SDHC_TIMER_COUNT_SHIFT`.
pub const SDHC_TIMER_COUNT_SHIFT: u32 = 8;
/// `SDHC_TIMER_COUNT_MASK`.
pub const SDHC_TIMER_COUNT_MASK: u32 = 0xf;
/// `SDHC_TUNING_SDR50`.
pub const SDHC_TUNING_SDR50: u32 = 1 << 13;
/// `SDHC_RETUNING_MODES_SHIFT`.
pub const SDHC_RETUNING_MODES_SHIFT: u32 = 14;
/// `SDHC_RETUNING_MODES_MASK`.
pub const SDHC_RETUNING_MODES_MASK: u32 = 0x3;
/// `SDHC_RETUNING_MODE_1`.
pub const SDHC_RETUNING_MODE_1: u32 = 0 << SDHC_RETUNING_MODES_SHIFT;
/// `SDHC_RETUNING_MODE_2`.
pub const SDHC_RETUNING_MODE_2: u32 = 1 << SDHC_RETUNING_MODES_SHIFT;
/// `SDHC_RETUNING_MODE_3`.
pub const SDHC_RETUNING_MODE_3: u32 = 2 << SDHC_RETUNING_MODES_SHIFT;
/// `SDHC_CLOCK_MULTIPLIER_SHIFT`.
pub const SDHC_CLOCK_MULTIPLIER_SHIFT: u32 = 16;
/// `SDHC_CLOCK_MULTIPLIER_MASK`.
pub const SDHC_CLOCK_MULTIPLIER_MASK: u32 = 0xff;
/// `SDHC_ADMA_ERROR_STATUS`.
pub const SDHC_ADMA_ERROR_STATUS: BusSize = 0x54;
/// `SDHC_ADMA_LENGTH_MISMATCH`.
pub const SDHC_ADMA_LENGTH_MISMATCH: u8 = 1 << 2;
/// `SDHC_ADMA_ERROR_STATE`.
pub const SDHC_ADMA_ERROR_STATE: u8 = 3 << 0;
/// `SDHC_ADMA_SYSTEM_ADDR`.
pub const SDHC_ADMA_SYSTEM_ADDR: BusSize = 0x58;
/// `SDHC_MAX_CAPABILITIES`.
pub const SDHC_MAX_CAPABILITIES: BusSize = 0x48;
/// `SDHC_SLOT_INTR_STATUS`.
pub const SDHC_SLOT_INTR_STATUS: BusSize = 0xfc;
/// `SDHC_HOST_CTL_VERSION`.
pub const SDHC_HOST_CTL_VERSION: BusSize = 0xfe;
/// `SDHC_SPEC_VERS_SHIFT`.
pub const SDHC_SPEC_VERS_SHIFT: u32 = 0;
/// `SDHC_SPEC_VERS_MASK`.
pub const SDHC_SPEC_VERS_MASK: u16 = 0xff;
/// `SDHC_SPEC_VERS_4_10`.
pub const SDHC_SPEC_VERS_4_10: u16 = 0x04;
/// `SDHC_SPEC_VERS_4_20`.
pub const SDHC_SPEC_VERS_4_20: u16 = 0x05;
/// `SDHC_VENDOR_VERS_SHIFT`.
pub const SDHC_VENDOR_VERS_SHIFT: u32 = 8;
/// `SDHC_VENDOR_VERS_MASK`.
pub const SDHC_VENDOR_VERS_MASK: u16 = 0xff;
/// `SDHC_SPEC_V1`.
pub const SDHC_SPEC_V1: u16 = 0;
/// `SDHC_SPEC_V2`.
pub const SDHC_SPEC_V2: u16 = 1;
/// `SDHC_SPEC_V3`.
pub const SDHC_SPEC_V3: u16 = 2;

/// `SDHC_SDCLK_DIV_MAX`.
pub const SDHC_SDCLK_DIV_MAX: i32 = 256;
/// `SDHC_SDCLK_DIV_MAX_V3`.
pub const SDHC_SDCLK_DIV_MAX_V3: i32 = 2046;

/// `SDHC_PRESENT_STATE_BITS`.
pub const SDHC_PRESENT_STATE_BITS: &[u8] =
    b"\x10\x19CL\x18D3L\x17D2L\x16D1L\x15D0L\x14WPS\x13CD\x12CSS\x11CI\x0cBRE\x0bBWE\x0aRTA\x09WTA\x03DLA\x02CID\x01CIC";
/// `SDHC_NINTR_STATUS_BITS`.
pub const SDHC_NINTR_STATUS_BITS: &[u8] =
    b"\x10\x10ERROR\x09CARD\x08REMOVAL\x07INSERTION\x06READ\x05WRITE\x04DMA\x03GAP\x02XFER\x01CMD";
/// `SDHC_EINTR_STATUS_BITS`.
pub const SDHC_EINTR_STATUS_BITS: &[u8] =
    b"\x10\x09ACMD12\x08CL\x07DEB\x06DCRC\x05DT\x04CI\x03CEB\x02CCRC\x01CT";
/// `SDHC_CAPABILITIES_BITS`.
pub const SDHC_CAPABILITIES_BITS: &[u8] =
    b"\x10\x1bVdd1.8V\x1aVdd3.0V\x19Vdd3.3V\x18SUSPEND\x17DMA\x16HIGHSPEED";

/// `SDHC_ADMA2_VALID`.
pub const SDHC_ADMA2_VALID: u16 = 1 << 0;
/// `SDHC_ADMA2_END`.
pub const SDHC_ADMA2_END: u16 = 1 << 1;
/// `SDHC_ADMA2_INT`.
pub const SDHC_ADMA2_INT: u16 = 1 << 2;
/// `SDHC_ADMA2_ACT`.
pub const SDHC_ADMA2_ACT: u16 = 3 << 4;
/// `SDHC_ADMA2_ACT_NOP`.
pub const SDHC_ADMA2_ACT_NOP: u16 = 0 << 4;
/// `SDHC_ADMA2_ACT_TRANS`.
pub const SDHC_ADMA2_ACT_TRANS: u16 = 2 << 4;
/// `SDHC_ADMA2_ACT_LINK`.
pub const SDHC_ADMA2_ACT_LINK: u16 = 3 << 4;

/// `struct sdhc_adma2_descriptor32`.
#[repr(C, packed)]
#[derive(Clone, Copy, Default)]
pub struct SdhcAdma2Descriptor32 {
    /// `attribute`.
    pub attribute: u16,
    /// `length`.
    pub length: u16,
    /// `address`.
    pub address: u32,
}

/// `struct sdhc_adma2_descriptor64`.
#[repr(C, packed)]
#[derive(Clone, Copy, Default)]
pub struct SdhcAdma2Descriptor64 {
    /// `attribute`.
    pub attribute: u16,
    /// `length`.
    pub length: u16,
    /// `address_lo`.
    pub address_lo: u32,
    /// `address_hi`.
    pub address_hi: u32,
}

/* SDHC_CLOCK_CTL encoding */

/// `SDHC_SDCLK_DIV(div)`.
pub const fn sdhc_sdclk_div(div: i32) -> i32 {
    (div & SDHC_SDCLK_DIV_MASK) << SDHC_SDCLK_DIV_SHIFT
}

/// `SDHC_SDCLK_DIV_V3(div)`.
pub const fn sdhc_sdclk_div_v3(div: i32) -> i32 {
    sdhc_sdclk_div(div) | ((div & SDHC_SDCLK_DIV_MASK_V3) >> SDHC_SDCLK_DIV_RSHIFT_V3)
}

/* SDHC_CAPABILITIES decoding */

/// `SDHC_BASE_FREQ_KHZ(cap)`.
pub const fn sdhc_base_freq_khz(cap: u32) -> u32 {
    ((cap >> SDHC_BASE_FREQ_SHIFT) & SDHC_BASE_FREQ_MASK) * 1000
}

/// `SDHC_BASE_FREQ_KHZ_V3(cap)`.
pub const fn sdhc_base_freq_khz_v3(cap: u32) -> u32 {
    ((cap >> SDHC_BASE_FREQ_SHIFT) & SDHC_BASE_FREQ_MASK_V3) * 1000
}

/// `SDHC_TIMEOUT_FREQ(cap)`.
pub const fn sdhc_timeout_freq(cap: u32) -> u32 {
    (cap >> SDHC_TIMEOUT_FREQ_SHIFT) & SDHC_TIMEOUT_FREQ_MASK
}

/// `SDHC_TIMEOUT_FREQ_KHZ(cap)`.
pub const fn sdhc_timeout_freq_khz(cap: u32) -> u32 {
    if cap & SDHC_TIMEOUT_FREQ_UNIT != 0 {
        sdhc_timeout_freq(cap) * 1000
    } else {
        sdhc_timeout_freq(cap)
    }
}

/* SDHC_HOST_CTL_VERSION decoding */

/// `SDHC_SPEC_VERSION(hcv)`.
pub const fn sdhc_spec_version(hcv: u16) -> u16 {
    (hcv >> SDHC_SPEC_VERS_SHIFT) & SDHC_SPEC_VERS_MASK
}

/// `SDHC_VENDOR_VERSION(hcv)`.
pub const fn sdhc_vendor_version(hcv: u16) -> u16 {
    (hcv >> SDHC_VENDOR_VERS_SHIFT) & SDHC_VENDOR_VERS_MASK
}

const _: () = {
    assert!(size_of::<SdhcAdma2Descriptor32>() == 8);
    assert!(size_of::<SdhcAdma2Descriptor64>() == 12);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;
    use crate::reftest;

    #[test]
    fn clock_and_capability_decoding() {
        // A divisor of 64 on a v3 controller: bits 15:8 = 64, bits 7:6 = 0.
        assert_eq!(sdhc_sdclk_div_v3(64), 64 << 8);
        // A 10-bit divisor of 0x2ff: low byte in 15:8, the two high bits in 7:6.
        assert_eq!(sdhc_sdclk_div_v3(0x2ff), (0xff << 8) | 0x80);
        assert_eq!(sdhc_sdclk_div(0x80), 0x8000);
        // QEMU's v2 capabilities: base clock 52 MHz in bits 13:8 (0x34).
        assert_eq!(sdhc_base_freq_khz(0x34 << 8), 52000);
        assert_eq!(sdhc_timeout_freq_khz(0x80 | 0x34), 0x14 * 1000);
        assert_eq!(sdhc_timeout_freq_khz(0x14), 0x14);
        assert_eq!(sdhc_spec_version(0x1001), SDHC_SPEC_V2);
        assert_eq!(sdhc_vendor_version(0x1001), 0x10);
    }

    #[test]
    #[ignore = "reads the C reference (just test-ref)"]
    fn defines_match_the_reference() {
        let defs = reftest::defines("sys/dev/sdmmc/sdhcreg.h");
        #[allow(clippy::unnecessary_cast)] // a column of mixed types
        let ours: &[(&str, i64)] = &[
            (
                "SDHC_PCI_INTERFACE_NO_DMA",
                SDHC_PCI_INTERFACE_NO_DMA as i64,
            ),
            ("SDHC_PCI_INTERFACE_DMA", SDHC_PCI_INTERFACE_DMA as i64),
            (
                "SDHC_PCI_INTERFACE_VENDOR",
                SDHC_PCI_INTERFACE_VENDOR as i64,
            ),
            ("SDHC_DMA_ADDR", SDHC_DMA_ADDR as i64),
            ("SDHC_BLOCK_SIZE", SDHC_BLOCK_SIZE as i64),
            ("SDHC_BLOCK_COUNT", SDHC_BLOCK_COUNT as i64),
            ("SDHC_BLOCK_COUNT_MAX", SDHC_BLOCK_COUNT_MAX as i64),
            ("SDHC_ARGUMENT", SDHC_ARGUMENT as i64),
            ("SDHC_TRANSFER_MODE", SDHC_TRANSFER_MODE as i64),
            ("SDHC_MULTI_BLOCK_MODE", SDHC_MULTI_BLOCK_MODE as i64),
            ("SDHC_READ_MODE", SDHC_READ_MODE as i64),
            ("SDHC_AUTO_CMD12_ENABLE", SDHC_AUTO_CMD12_ENABLE as i64),
            ("SDHC_BLOCK_COUNT_ENABLE", SDHC_BLOCK_COUNT_ENABLE as i64),
            ("SDHC_DMA_ENABLE", SDHC_DMA_ENABLE as i64),
            ("SDHC_COMMAND", SDHC_COMMAND as i64),
            ("SDHC_COMMAND_INDEX_SHIFT", SDHC_COMMAND_INDEX_SHIFT as i64),
            ("SDHC_COMMAND_INDEX_MASK", SDHC_COMMAND_INDEX_MASK as i64),
            ("SDHC_COMMAND_TYPE_ABORT", SDHC_COMMAND_TYPE_ABORT as i64),
            ("SDHC_COMMAND_TYPE_RESUME", SDHC_COMMAND_TYPE_RESUME as i64),
            (
                "SDHC_COMMAND_TYPE_SUSPEND",
                SDHC_COMMAND_TYPE_SUSPEND as i64,
            ),
            ("SDHC_COMMAND_TYPE_NORMAL", SDHC_COMMAND_TYPE_NORMAL as i64),
            ("SDHC_DATA_PRESENT_SELECT", SDHC_DATA_PRESENT_SELECT as i64),
            ("SDHC_INDEX_CHECK_ENABLE", SDHC_INDEX_CHECK_ENABLE as i64),
            ("SDHC_CRC_CHECK_ENABLE", SDHC_CRC_CHECK_ENABLE as i64),
            (
                "SDHC_RESP_LEN_48_CHK_BUSY",
                SDHC_RESP_LEN_48_CHK_BUSY as i64,
            ),
            ("SDHC_RESP_LEN_48", SDHC_RESP_LEN_48 as i64),
            ("SDHC_RESP_LEN_136", SDHC_RESP_LEN_136 as i64),
            ("SDHC_NO_RESPONSE", SDHC_NO_RESPONSE as i64),
            ("SDHC_RESPONSE", SDHC_RESPONSE as i64),
            ("SDHC_DATA", SDHC_DATA as i64),
            ("SDHC_PRESENT_STATE", SDHC_PRESENT_STATE as i64),
            (
                "SDHC_CMD_LINE_SIGNAL_LEVEL",
                SDHC_CMD_LINE_SIGNAL_LEVEL as i64,
            ),
            ("SDHC_DAT3_LINE_LEVEL", SDHC_DAT3_LINE_LEVEL as i64),
            ("SDHC_DAT2_LINE_LEVEL", SDHC_DAT2_LINE_LEVEL as i64),
            ("SDHC_DAT1_LINE_LEVEL", SDHC_DAT1_LINE_LEVEL as i64),
            ("SDHC_DAT0_LINE_LEVEL", SDHC_DAT0_LINE_LEVEL as i64),
            (
                "SDHC_WRITE_PROTECT_SWITCH",
                SDHC_WRITE_PROTECT_SWITCH as i64,
            ),
            (
                "SDHC_CARD_DETECT_PIN_LEVEL",
                SDHC_CARD_DETECT_PIN_LEVEL as i64,
            ),
            ("SDHC_CARD_STATE_STABLE", SDHC_CARD_STATE_STABLE as i64),
            ("SDHC_CARD_INSERTED", SDHC_CARD_INSERTED as i64),
            ("SDHC_BUFFER_READ_ENABLE", SDHC_BUFFER_READ_ENABLE as i64),
            ("SDHC_BUFFER_WRITE_ENABLE", SDHC_BUFFER_WRITE_ENABLE as i64),
            (
                "SDHC_READ_TRANSFER_ACTIVE",
                SDHC_READ_TRANSFER_ACTIVE as i64,
            ),
            (
                "SDHC_WRITE_TRANSFER_ACTIVE",
                SDHC_WRITE_TRANSFER_ACTIVE as i64,
            ),
            ("SDHC_DAT_ACTIVE", SDHC_DAT_ACTIVE as i64),
            ("SDHC_CMD_INHIBIT_DAT", SDHC_CMD_INHIBIT_DAT as i64),
            ("SDHC_CMD_INHIBIT_CMD", SDHC_CMD_INHIBIT_CMD as i64),
            ("SDHC_CMD_INHIBIT_MASK", SDHC_CMD_INHIBIT_MASK as i64),
            ("SDHC_HOST_CTL", SDHC_HOST_CTL as i64),
            ("SDHC_8BIT_MODE", SDHC_8BIT_MODE as i64),
            ("SDHC_DMA_SELECT", SDHC_DMA_SELECT as i64),
            ("SDHC_DMA_SELECT_SDMA", SDHC_DMA_SELECT_SDMA as i64),
            ("SDHC_DMA_SELECT_ADMA32", SDHC_DMA_SELECT_ADMA32 as i64),
            ("SDHC_DMA_SELECT_ADMA64", SDHC_DMA_SELECT_ADMA64 as i64),
            ("SDHC_HIGH_SPEED", SDHC_HIGH_SPEED as i64),
            ("SDHC_4BIT_MODE", SDHC_4BIT_MODE as i64),
            ("SDHC_LED_ON", SDHC_LED_ON as i64),
            ("SDHC_POWER_CTL", SDHC_POWER_CTL as i64),
            ("SDHC_VOLTAGE_SHIFT", SDHC_VOLTAGE_SHIFT as i64),
            ("SDHC_VOLTAGE_MASK", SDHC_VOLTAGE_MASK as i64),
            ("SDHC_VOLTAGE_3_3V", SDHC_VOLTAGE_3_3V as i64),
            ("SDHC_VOLTAGE_3_0V", SDHC_VOLTAGE_3_0V as i64),
            ("SDHC_VOLTAGE_1_8V", SDHC_VOLTAGE_1_8V as i64),
            ("SDHC_BUS_POWER", SDHC_BUS_POWER as i64),
            ("SDHC_BLOCK_GAP_CTL", SDHC_BLOCK_GAP_CTL as i64),
            ("SDHC_WAKEUP_CTL", SDHC_WAKEUP_CTL as i64),
            ("SDHC_CLOCK_CTL", SDHC_CLOCK_CTL as i64),
            ("SDHC_SDCLK_DIV_SHIFT", SDHC_SDCLK_DIV_SHIFT as i64),
            ("SDHC_SDCLK_DIV_MASK", SDHC_SDCLK_DIV_MASK as i64),
            ("SDHC_SDCLK_DIV_RSHIFT_V3", SDHC_SDCLK_DIV_RSHIFT_V3 as i64),
            ("SDHC_SDCLK_DIV_MASK_V3", SDHC_SDCLK_DIV_MASK_V3 as i64),
            ("SDHC_SDCLK_ENABLE", SDHC_SDCLK_ENABLE as i64),
            ("SDHC_INTCLK_STABLE", SDHC_INTCLK_STABLE as i64),
            ("SDHC_INTCLK_ENABLE", SDHC_INTCLK_ENABLE as i64),
            ("SDHC_TIMEOUT_CTL", SDHC_TIMEOUT_CTL as i64),
            ("SDHC_TIMEOUT_MAX", SDHC_TIMEOUT_MAX as i64),
            ("SDHC_SOFTWARE_RESET", SDHC_SOFTWARE_RESET as i64),
            ("SDHC_RESET_MASK", SDHC_RESET_MASK as i64),
            ("SDHC_RESET_DAT", SDHC_RESET_DAT as i64),
            ("SDHC_RESET_CMD", SDHC_RESET_CMD as i64),
            ("SDHC_RESET_ALL", SDHC_RESET_ALL as i64),
            ("SDHC_NINTR_STATUS", SDHC_NINTR_STATUS as i64),
            ("SDHC_ERROR_INTERRUPT", SDHC_ERROR_INTERRUPT as i64),
            ("SDHC_RETUNING_EVENT", SDHC_RETUNING_EVENT as i64),
            ("SDHC_CARD_INTERRUPT", SDHC_CARD_INTERRUPT as i64),
            ("SDHC_CARD_REMOVAL", SDHC_CARD_REMOVAL as i64),
            ("SDHC_CARD_INSERTION", SDHC_CARD_INSERTION as i64),
            ("SDHC_BUFFER_READ_READY", SDHC_BUFFER_READ_READY as i64),
            ("SDHC_BUFFER_WRITE_READY", SDHC_BUFFER_WRITE_READY as i64),
            ("SDHC_DMA_INTERRUPT", SDHC_DMA_INTERRUPT as i64),
            ("SDHC_BLOCK_GAP_EVENT", SDHC_BLOCK_GAP_EVENT as i64),
            ("SDHC_TRANSFER_COMPLETE", SDHC_TRANSFER_COMPLETE as i64),
            ("SDHC_COMMAND_COMPLETE", SDHC_COMMAND_COMPLETE as i64),
            ("SDHC_NINTR_STATUS_MASK", SDHC_NINTR_STATUS_MASK as i64),
            ("SDHC_EINTR_STATUS", SDHC_EINTR_STATUS as i64),
            ("SDHC_ADMA_ERROR", SDHC_ADMA_ERROR as i64),
            ("SDHC_AUTO_CMD12_ERROR", SDHC_AUTO_CMD12_ERROR as i64),
            ("SDHC_CURRENT_LIMIT_ERROR", SDHC_CURRENT_LIMIT_ERROR as i64),
            ("SDHC_DATA_END_BIT_ERROR", SDHC_DATA_END_BIT_ERROR as i64),
            ("SDHC_DATA_CRC_ERROR", SDHC_DATA_CRC_ERROR as i64),
            ("SDHC_DATA_TIMEOUT_ERROR", SDHC_DATA_TIMEOUT_ERROR as i64),
            ("SDHC_CMD_INDEX_ERROR", SDHC_CMD_INDEX_ERROR as i64),
            ("SDHC_CMD_END_BIT_ERROR", SDHC_CMD_END_BIT_ERROR as i64),
            ("SDHC_CMD_CRC_ERROR", SDHC_CMD_CRC_ERROR as i64),
            ("SDHC_CMD_TIMEOUT_ERROR", SDHC_CMD_TIMEOUT_ERROR as i64),
            ("SDHC_EINTR_STATUS_MASK", SDHC_EINTR_STATUS_MASK as i64),
            ("SDHC_NINTR_STATUS_EN", SDHC_NINTR_STATUS_EN as i64),
            ("SDHC_EINTR_STATUS_EN", SDHC_EINTR_STATUS_EN as i64),
            ("SDHC_NINTR_SIGNAL_EN", SDHC_NINTR_SIGNAL_EN as i64),
            ("SDHC_NINTR_SIGNAL_MASK", SDHC_NINTR_SIGNAL_MASK as i64),
            ("SDHC_EINTR_SIGNAL_EN", SDHC_EINTR_SIGNAL_EN as i64),
            ("SDHC_EINTR_SIGNAL_MASK", SDHC_EINTR_SIGNAL_MASK as i64),
            ("SDHC_CMD12_ERROR_STATUS", SDHC_CMD12_ERROR_STATUS as i64),
            ("SDHC_HOST_CTL2", SDHC_HOST_CTL2 as i64),
            ("SDHC_SAMPLING_CLOCK_SEL", SDHC_SAMPLING_CLOCK_SEL as i64),
            ("SDHC_EXECUTE_TUNING", SDHC_EXECUTE_TUNING as i64),
            ("SDHC_1_8V_SIGNAL_EN", SDHC_1_8V_SIGNAL_EN as i64),
            (
                "SDHC_UHS_MODE_SELECT_SHIFT",
                SDHC_UHS_MODE_SELECT_SHIFT as i64,
            ),
            (
                "SDHC_UHS_MODE_SELECT_MASK",
                SDHC_UHS_MODE_SELECT_MASK as i64,
            ),
            (
                "SDHC_UHS_MODE_SELECT_SDR12",
                SDHC_UHS_MODE_SELECT_SDR12 as i64,
            ),
            (
                "SDHC_UHS_MODE_SELECT_SDR25",
                SDHC_UHS_MODE_SELECT_SDR25 as i64,
            ),
            (
                "SDHC_UHS_MODE_SELECT_SDR50",
                SDHC_UHS_MODE_SELECT_SDR50 as i64,
            ),
            (
                "SDHC_UHS_MODE_SELECT_SDR104",
                SDHC_UHS_MODE_SELECT_SDR104 as i64,
            ),
            (
                "SDHC_UHS_MODE_SELECT_DDR50",
                SDHC_UHS_MODE_SELECT_DDR50 as i64,
            ),
            ("SDHC_CAPABILITIES", SDHC_CAPABILITIES as i64),
            ("SDHC_64BIT_DMA_SUPP", SDHC_64BIT_DMA_SUPP as i64),
            ("SDHC_VOLTAGE_SUPP_1_8V", SDHC_VOLTAGE_SUPP_1_8V as i64),
            ("SDHC_VOLTAGE_SUPP_3_0V", SDHC_VOLTAGE_SUPP_3_0V as i64),
            ("SDHC_VOLTAGE_SUPP_3_3V", SDHC_VOLTAGE_SUPP_3_3V as i64),
            ("SDHC_SDMA_SUPP", SDHC_SDMA_SUPP as i64),
            ("SDHC_HIGH_SPEED_SUPP", SDHC_HIGH_SPEED_SUPP as i64),
            ("SDHC_ADMA2_SUPP", SDHC_ADMA2_SUPP as i64),
            ("SDHC_8BIT_MODE_SUPP", SDHC_8BIT_MODE_SUPP as i64),
            ("SDHC_MAX_BLK_LEN_512", SDHC_MAX_BLK_LEN_512 as i64),
            ("SDHC_MAX_BLK_LEN_1024", SDHC_MAX_BLK_LEN_1024 as i64),
            ("SDHC_MAX_BLK_LEN_2048", SDHC_MAX_BLK_LEN_2048 as i64),
            ("SDHC_MAX_BLK_LEN_SHIFT", SDHC_MAX_BLK_LEN_SHIFT as i64),
            ("SDHC_MAX_BLK_LEN_MASK", SDHC_MAX_BLK_LEN_MASK as i64),
            ("SDHC_BASE_FREQ_SHIFT", SDHC_BASE_FREQ_SHIFT as i64),
            ("SDHC_BASE_FREQ_MASK", SDHC_BASE_FREQ_MASK as i64),
            ("SDHC_BASE_FREQ_MASK_V3", SDHC_BASE_FREQ_MASK_V3 as i64),
            ("SDHC_TIMEOUT_FREQ_UNIT", SDHC_TIMEOUT_FREQ_UNIT as i64),
            ("SDHC_TIMEOUT_FREQ_SHIFT", SDHC_TIMEOUT_FREQ_SHIFT as i64),
            ("SDHC_TIMEOUT_FREQ_MASK", SDHC_TIMEOUT_FREQ_MASK as i64),
            ("SDHC_CAPABILITIES2", SDHC_CAPABILITIES2 as i64),
            ("SDHC_SDR50_SUPP", SDHC_SDR50_SUPP as i64),
            ("SDHC_SDR104_SUPP", SDHC_SDR104_SUPP as i64),
            ("SDHC_DDR50_SUPP", SDHC_DDR50_SUPP as i64),
            ("SDHC_DRIVER_TYPE_A", SDHC_DRIVER_TYPE_A as i64),
            ("SDHC_DRIVER_TYPE_C", SDHC_DRIVER_TYPE_C as i64),
            ("SDHC_DRIVER_TYPE_D", SDHC_DRIVER_TYPE_D as i64),
            ("SDHC_TIMER_COUNT_SHIFT", SDHC_TIMER_COUNT_SHIFT as i64),
            ("SDHC_TIMER_COUNT_MASK", SDHC_TIMER_COUNT_MASK as i64),
            ("SDHC_TUNING_SDR50", SDHC_TUNING_SDR50 as i64),
            (
                "SDHC_RETUNING_MODES_SHIFT",
                SDHC_RETUNING_MODES_SHIFT as i64,
            ),
            ("SDHC_RETUNING_MODES_MASK", SDHC_RETUNING_MODES_MASK as i64),
            ("SDHC_RETUNING_MODE_1", SDHC_RETUNING_MODE_1 as i64),
            ("SDHC_RETUNING_MODE_2", SDHC_RETUNING_MODE_2 as i64),
            ("SDHC_RETUNING_MODE_3", SDHC_RETUNING_MODE_3 as i64),
            (
                "SDHC_CLOCK_MULTIPLIER_SHIFT",
                SDHC_CLOCK_MULTIPLIER_SHIFT as i64,
            ),
            (
                "SDHC_CLOCK_MULTIPLIER_MASK",
                SDHC_CLOCK_MULTIPLIER_MASK as i64,
            ),
            ("SDHC_ADMA_ERROR_STATUS", SDHC_ADMA_ERROR_STATUS as i64),
            (
                "SDHC_ADMA_LENGTH_MISMATCH",
                SDHC_ADMA_LENGTH_MISMATCH as i64,
            ),
            ("SDHC_ADMA_ERROR_STATE", SDHC_ADMA_ERROR_STATE as i64),
            ("SDHC_ADMA_SYSTEM_ADDR", SDHC_ADMA_SYSTEM_ADDR as i64),
            ("SDHC_MAX_CAPABILITIES", SDHC_MAX_CAPABILITIES as i64),
            ("SDHC_SLOT_INTR_STATUS", SDHC_SLOT_INTR_STATUS as i64),
            ("SDHC_HOST_CTL_VERSION", SDHC_HOST_CTL_VERSION as i64),
            ("SDHC_SPEC_VERS_SHIFT", SDHC_SPEC_VERS_SHIFT as i64),
            ("SDHC_SPEC_VERS_MASK", SDHC_SPEC_VERS_MASK as i64),
            ("SDHC_SPEC_VERS_4_10", SDHC_SPEC_VERS_4_10 as i64),
            ("SDHC_SPEC_VERS_4_20", SDHC_SPEC_VERS_4_20 as i64),
            ("SDHC_VENDOR_VERS_SHIFT", SDHC_VENDOR_VERS_SHIFT as i64),
            ("SDHC_VENDOR_VERS_MASK", SDHC_VENDOR_VERS_MASK as i64),
            ("SDHC_SPEC_V1", SDHC_SPEC_V1 as i64),
            ("SDHC_SPEC_V2", SDHC_SPEC_V2 as i64),
            ("SDHC_SPEC_V3", SDHC_SPEC_V3 as i64),
            ("SDHC_SDCLK_DIV_MAX", SDHC_SDCLK_DIV_MAX as i64),
            ("SDHC_SDCLK_DIV_MAX_V3", SDHC_SDCLK_DIV_MAX_V3 as i64),
            ("SDHC_ADMA2_VALID", SDHC_ADMA2_VALID as i64),
            ("SDHC_ADMA2_END", SDHC_ADMA2_END as i64),
            ("SDHC_ADMA2_INT", SDHC_ADMA2_INT as i64),
            ("SDHC_ADMA2_ACT", SDHC_ADMA2_ACT as i64),
            ("SDHC_ADMA2_ACT_NOP", SDHC_ADMA2_ACT_NOP as i64),
            ("SDHC_ADMA2_ACT_TRANS", SDHC_ADMA2_ACT_TRANS as i64),
            ("SDHC_ADMA2_ACT_LINK", SDHC_ADMA2_ACT_LINK as i64),
        ];
        for &(name, v) in ours {
            assert_eq!(reftest::int(&defs, name), Some(v), "{name}");
        }
        // The %b strings, compared as the C writes them (octal escapes).
        let text = std::fs::read_to_string(reftest::openbsd_src().join("sys/dev/sdmmc/sdhcreg.h"))
            .unwrap_or_default();
        assert!(text.contains(r#""\20\31CL\30D3L\27D2L\26D1L\25D0L\24WPS\23CD\22CSS\21CI""#));
        assert_eq!(SDHC_PRESENT_STATE_BITS[0], 0o20);
        assert_eq!(SDHC_PRESENT_STATE_BITS[1], 0o31);
    }
}
/* </TESTS> */
