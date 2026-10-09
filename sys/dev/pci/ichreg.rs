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
 * Copyright (c) 2004, 2005 Alexander Yurchenko <grange@openbsd.org>
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
//! `<dev/pci/ichreg.h>`: Intel I/O Controller Hub (ICH) register definitions: the LPC interface
//! bridge's power management registers, the SMBus controller's (`ichiic(4)`) and the watchdog
//! timer's.
//!
//! Upstream: sys/dev/pci/ichreg.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - Types follow their use: PCI configuration offsets are `i32` and their contents `u32` (as in
//!   `pcireg.rs`), I/O and memory offsets are `usize` (`bus_size_t`), the bits of the SMBus I/O
//!   registers are `u8` and those of the watchdog's 32-bit memory registers `u32`.
//! - `ICH_SMB_HS_BITS` is a byte string for the `%b` adaptor (`Bitmask`): the C's octal escapes
//!   as hex ones.
//! - The function-like macros are lower-case `const fn`s: `ich_smb_txslva_addr`,
//!   `ich_smb_sd_msg0`, `ich_smb_sd_msg1` and `ich_smb_ndaddr_addr`.

// PCI configuration registers
/// `ICH_PMBASE`: ACPI base address.
pub const ICH_PMBASE: i32 = 0x40;
/// `ICH_ACPI_CNTL`: ACPI control.
pub const ICH_ACPI_CNTL: i32 = 0x44;
/// `ICH_ACPI_CNTL_ACPI_EN`: ACPI enable.
pub const ICH_ACPI_CNTL_ACPI_EN: u32 = 1 << 4;
/// `ICH_GEN_PMCON1`: general PM configuration.
pub const ICH_GEN_PMCON1: i32 = 0xa0;
// ICHx-M only
/// `ICH_GEN_PMCON1_SS_EN`: enable SpeedStep.
pub const ICH_GEN_PMCON1_SS_EN: u32 = 0x08;
// Power management I/O registers
/// `ICH_PM_TMR`: PM timer.
pub const ICH_PM_TMR: usize = 0x08;
// ICHx-M only
/// `ICH_PM_CNTL`: power management control.
pub const ICH_PM_CNTL: usize = 0x20;
/// `ICH_PM_ARB_DIS`: disable arbiter.
pub const ICH_PM_ARB_DIS: u8 = 0x01;
/// `ICH_PM_SS_CNTL`: SpeedStep control.
pub const ICH_PM_SS_CNTL: usize = 0x50;
/// `ICH_PM_SS_STATE_LOW`: low power state.
pub const ICH_PM_SS_STATE_LOW: u8 = 0x01;
/// `ICH_PMSIZE`: ACPI I/O space size.
pub const ICH_PMSIZE: usize = 128;
// PCI configuration registers
/// `ICH_SMB_BASE`: SMBus base address.
pub const ICH_SMB_BASE: i32 = 0x20;
/// `ICH_SMB_HOSTC`: host configuration.
pub const ICH_SMB_HOSTC: i32 = 0x40;
/// `ICH_SMB_HOSTC_HSTEN`: enable host controller.
pub const ICH_SMB_HOSTC_HSTEN: u32 = 1;
/// `ICH_SMB_HOSTC_SMIEN`: generate SMI.
pub const ICH_SMB_HOSTC_SMIEN: u32 = 1 << 1;
/// `ICH_SMB_HOSTC_I2CEN`: enable I2C commands.
pub const ICH_SMB_HOSTC_I2CEN: u32 = 1 << 2;
// SMBus I/O registers
/// `ICH_SMB_HS`: host status.
pub const ICH_SMB_HS: usize = 0x00;
/// `ICH_SMB_HS_BUSY`: running a command.
pub const ICH_SMB_HS_BUSY: u8 = 1;
/// `ICH_SMB_HS_INTR`: command completed.
pub const ICH_SMB_HS_INTR: u8 = 1 << 1;
/// `ICH_SMB_HS_DEVERR`: command error.
pub const ICH_SMB_HS_DEVERR: u8 = 1 << 2;
/// `ICH_SMB_HS_BUSERR`: transaction collision.
pub const ICH_SMB_HS_BUSERR: u8 = 1 << 3;
/// `ICH_SMB_HS_FAILED`: failed bus transaction.
pub const ICH_SMB_HS_FAILED: u8 = 1 << 4;
/// `ICH_SMB_HS_SMBAL`: SMBALERT# asserted.
pub const ICH_SMB_HS_SMBAL: u8 = 1 << 5;
/// `ICH_SMB_HS_INUSE`: bus semaphore.
pub const ICH_SMB_HS_INUSE: u8 = 1 << 6;
/// `ICH_SMB_HS_BDONE`: byte received/transmitted.
pub const ICH_SMB_HS_BDONE: u8 = 1 << 7;
/// `ICH_SMB_HC`: host control.
pub const ICH_SMB_HC: usize = 0x02;
/// `ICH_SMB_HC_INTREN`: enable interrupts.
pub const ICH_SMB_HC_INTREN: u8 = 1;
/// `ICH_SMB_HC_KILL`: kill current transaction.
pub const ICH_SMB_HC_KILL: u8 = 1 << 1;
/// `ICH_SMB_HC_CMD_QUICK`: QUICK command.
pub const ICH_SMB_HC_CMD_QUICK: u8 = 0;
/// `ICH_SMB_HC_CMD_BYTE`: BYTE command.
pub const ICH_SMB_HC_CMD_BYTE: u8 = 1 << 2;
/// `ICH_SMB_HC_CMD_BDATA`: BYTE DATA command.
pub const ICH_SMB_HC_CMD_BDATA: u8 = 2 << 2;
/// `ICH_SMB_HC_CMD_WDATA`: WORD DATA command.
pub const ICH_SMB_HC_CMD_WDATA: u8 = 3 << 2;
/// `ICH_SMB_HC_CMD_PCALL`: PROCESS CALL command.
pub const ICH_SMB_HC_CMD_PCALL: u8 = 4 << 2;
/// `ICH_SMB_HC_CMD_BLOCK`: BLOCK command.
pub const ICH_SMB_HC_CMD_BLOCK: u8 = 5 << 2;
/// `ICH_SMB_HC_CMD_I2CREAD`: I2C READ command.
pub const ICH_SMB_HC_CMD_I2CREAD: u8 = 6 << 2;
/// `ICH_SMB_HC_CMD_BLOCKP`: BLOCK PROCESS command.
pub const ICH_SMB_HC_CMD_BLOCKP: u8 = 7 << 2;
/// `ICH_SMB_HC_LASTB`: last byte in block.
pub const ICH_SMB_HC_LASTB: u8 = 1 << 5;
/// `ICH_SMB_HC_START`: start transaction.
pub const ICH_SMB_HC_START: u8 = 1 << 6;
/// `ICH_SMB_HC_PECEN`: enable PEC.
pub const ICH_SMB_HC_PECEN: u8 = 1 << 7;
/// `ICH_SMB_HCMD`: host command.
pub const ICH_SMB_HCMD: usize = 0x03;
/// `ICH_SMB_TXSLVA`: transmit slave address.
pub const ICH_SMB_TXSLVA: usize = 0x04;
/// `ICH_SMB_TXSLVA_READ`: read direction.
pub const ICH_SMB_TXSLVA_READ: u8 = 1;
/// `ICH_SMB_HD0`: host data 0.
pub const ICH_SMB_HD0: usize = 0x05;
/// `ICH_SMB_HD1`: host data 1.
pub const ICH_SMB_HD1: usize = 0x06;
/// `ICH_SMB_HBDB`: host block data byte.
pub const ICH_SMB_HBDB: usize = 0x07;
/// `ICH_SMB_PEC`: PEC data.
pub const ICH_SMB_PEC: usize = 0x08;
/// `ICH_SMB_RXSLVA`: receive slave address.
pub const ICH_SMB_RXSLVA: usize = 0x09;
/// `ICH_SMB_SD`: receive slave data.
pub const ICH_SMB_SD: usize = 0x0a;
/// `ICH_SMB_AS`: auxiliary status.
pub const ICH_SMB_AS: usize = 0x0c;
/// `ICH_SMB_AS_CRCE`: CRC error.
pub const ICH_SMB_AS_CRCE: u8 = 1;
/// `ICH_SMB_AS_TCO`: advanced TCO mode.
pub const ICH_SMB_AS_TCO: u8 = 1 << 1;
/// `ICH_SMB_AC`: auxiliary control.
pub const ICH_SMB_AC: usize = 0x0d;
/// `ICH_SMB_AC_AAC`: automatically append CRC.
pub const ICH_SMB_AC_AAC: u8 = 1;
/// `ICH_SMB_AC_E32B`: enable 32-byte buffer.
pub const ICH_SMB_AC_E32B: u8 = 1 << 1;
/// `ICH_SMB_SMLPC`: SMLink pin control.
pub const ICH_SMB_SMLPC: usize = 0x0e;
/// `ICH_SMB_SMLPC_LINK0`: SMLINK0 pin state.
pub const ICH_SMB_SMLPC_LINK0: u8 = 1;
/// `ICH_SMB_SMLPC_LINK1`: SMLINK1 pin state.
pub const ICH_SMB_SMLPC_LINK1: u8 = 1 << 1;
/// `ICH_SMB_SMLPC_CLKC`: SMLINK0 pin is untouched.
pub const ICH_SMB_SMLPC_CLKC: u8 = 1 << 2;
/// `ICH_SMB_SMBPC`: SMBus pin control.
pub const ICH_SMB_SMBPC: usize = 0x0f;
/// `ICH_SMB_SMBPC_CLK`: SMBCLK pin state.
pub const ICH_SMB_SMBPC_CLK: u8 = 1;
/// `ICH_SMB_SMBPC_DATA`: SMBDATA pin state.
pub const ICH_SMB_SMBPC_DATA: u8 = 1 << 1;
/// `ICH_SMB_SMBPC_CLKC`: SMBCLK pin is untouched.
pub const ICH_SMB_SMBPC_CLKC: u8 = 1 << 2;
/// `ICH_SMB_SS`: slave status.
pub const ICH_SMB_SS: usize = 0x10;
/// `ICH_SMB_SS_HN`: Host Notify command.
pub const ICH_SMB_SS_HN: u8 = 1;
/// `ICH_SMB_SCMD`: slave command.
pub const ICH_SMB_SCMD: usize = 0x11;
/// `ICH_SMB_SCMD_INTREN`: enable interrupts on HN.
pub const ICH_SMB_SCMD_INTREN: u8 = 1;
/// `ICH_SMB_SCMD_WKEN`: wake on HN.
pub const ICH_SMB_SCMD_WKEN: u8 = 1 << 1;
/// `ICH_SMB_SCMD_SMBALDS`: disable SMBALERT# intr.
pub const ICH_SMB_SCMD_SMBALDS: u8 = 1 << 2;
/// `ICH_SMB_NDADDR`: notify device address.
pub const ICH_SMB_NDADDR: usize = 0x14;
/// `ICH_SMB_NDLOW`: notify data low byte.
pub const ICH_SMB_NDLOW: usize = 0x16;
/// `ICH_SMB_NDHIGH`: notify data high byte.
pub const ICH_SMB_NDHIGH: usize = 0x17;
// PCI configuration registers
/// `ICH_WDT_BASE`: memory space base address.
pub const ICH_WDT_BASE: i32 = 0x10;
/// `ICH_WDT_CONF`: configuration register.
pub const ICH_WDT_CONF: i32 = 0x60;
/// `ICH_WDT_CONF_MASK`: 16-bit register.
pub const ICH_WDT_CONF_MASK: u32 = 0xffff;
/// `ICH_WDT_CONF_INT_MASK`: interrupt type.
pub const ICH_WDT_CONF_INT_MASK: u32 = 0x3;
/// `ICH_WDT_CONF_INT_IRQ`: IRQ (APIC 1, INT 10).
pub const ICH_WDT_CONF_INT_IRQ: u32 = 0x0;
/// `ICH_WDT_CONF_INT_SMI`: SMI.
pub const ICH_WDT_CONF_INT_SMI: u32 = 0x2;
/// `ICH_WDT_CONF_INT_DIS`: disabled.
pub const ICH_WDT_CONF_INT_DIS: u32 = 0x3;
/// `ICH_WDT_CONF_PRE`: 2^5 clock divisor.
pub const ICH_WDT_CONF_PRE: u32 = 1 << 2;
/// `ICH_WDT_CONF_OUTDIS`: WDT_TOUT# output disabled.
pub const ICH_WDT_CONF_OUTDIS: u32 = 1 << 5;
/// `ICH_WDT_LOCK`: lock register.
pub const ICH_WDT_LOCK: i32 = 0x68;
/// `ICH_WDT_LOCK_LOCKED`: register locked.
pub const ICH_WDT_LOCK_LOCKED: u32 = 1;
/// `ICH_WDT_LOCK_ENABLED`: WDT enabled.
pub const ICH_WDT_LOCK_ENABLED: u32 = 1 << 1;
/// `ICH_WDT_LOCK_FREERUN`: free running mode.
pub const ICH_WDT_LOCK_FREERUN: u32 = 1 << 2;
// Memory mapped registers
/// `ICH_WDT_PRE1`: preload value 1.
pub const ICH_WDT_PRE1: usize = 0x00;
/// `ICH_WDT_PRE2`: preload value 2.
pub const ICH_WDT_PRE2: usize = 0x04;
/// `ICH_WDT_GIS`: general interrupt status.
pub const ICH_WDT_GIS: usize = 0x08;
/// `ICH_WDT_GIS_ACTIVE`: interrupt active.
pub const ICH_WDT_GIS_ACTIVE: u32 = 1;
/// `ICH_WDT_RELOAD`: reload register.
pub const ICH_WDT_RELOAD: usize = 0x0c;
/// `ICH_WDT_RELOAD_RLD`: safe reload.
pub const ICH_WDT_RELOAD_RLD: u32 = 1 << 8;
/// `ICH_WDT_RELOAD_TIMEOUT`: timeout occurred.
pub const ICH_WDT_RELOAD_TIMEOUT: u32 = 1 << 9;

/// `ICH_SMB_HS_BITS`: the names of the host status bits, for `%b`.
pub const ICH_SMB_HS_BITS: &[u8] =
    b"\x10\x01BUSY\x02INTR\x03DEVERR\x04BUSERR\x05FAILED\x06SMBAL\x07INUSE\x08BDONE";

/// `ICH_SMB_TXSLVA_ADDR(x)`: the 7-bit address in the transmit slave address register.
pub const fn ich_smb_txslva_addr(x: u16) -> u8 {
    ((x & 0x7f) << 1) as u8
}

/// `ICH_SMB_SD_MSG0(x)`: data message byte 0.
pub const fn ich_smb_sd_msg0(x: u16) -> u8 {
    (x & 0xff) as u8
}

/// `ICH_SMB_SD_MSG1(x)`: data message byte 1.
pub const fn ich_smb_sd_msg1(x: u16) -> u8 {
    (x >> 8) as u8
}

/// `ICH_SMB_NDADDR_ADDR(x)`: the 7-bit address in the notify device address register.
pub const fn ich_smb_ndaddr_addr(x: u8) -> u8 {
    x >> 1
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slave_address_register_encodes_the_address_and_the_direction() {
        // A 7-bit address in bits 7..1, the direction in bit 0; bit 7 of the address is dropped.
        assert_eq!(ich_smb_txslva_addr(0x50), 0xa0);
        assert_eq!(ich_smb_txslva_addr(0x50) | ICH_SMB_TXSLVA_READ, 0xa1);
        assert_eq!(ich_smb_txslva_addr(0xd0), 0xa0);
        assert_eq!(ich_smb_sd_msg0(0x1234), 0x34);
        assert_eq!(ich_smb_sd_msg1(0x1234), 0x12);
        assert_eq!(ich_smb_ndaddr_addr(0xa0), 0x50);
    }

    #[test]
    fn status_bits_are_named_for_printf_b() {
        use crate::kern::subr_prf::Bitmask;
        use std::string::ToString;
        let st = ICH_SMB_HS_BUSY | ICH_SMB_HS_FAILED | ICH_SMB_HS_BDONE;
        assert_eq!(
            Bitmask(u64::from(st), ICH_SMB_HS_BITS).to_string(),
            "91<BUSY,FAILED,BDONE>"
        );
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/pci/ichreg.h");
        for (name, value) in [
            ("ICH_PMBASE", ICH_PMBASE as i64),
            ("ICH_ACPI_CNTL", ICH_ACPI_CNTL as i64),
            ("ICH_ACPI_CNTL_ACPI_EN", ICH_ACPI_CNTL_ACPI_EN as i64),
            ("ICH_GEN_PMCON1", ICH_GEN_PMCON1 as i64),
            ("ICH_GEN_PMCON1_SS_EN", ICH_GEN_PMCON1_SS_EN as i64),
            ("ICH_PM_TMR", ICH_PM_TMR as i64),
            ("ICH_PM_CNTL", ICH_PM_CNTL as i64),
            ("ICH_PM_ARB_DIS", ICH_PM_ARB_DIS as i64),
            ("ICH_PM_SS_CNTL", ICH_PM_SS_CNTL as i64),
            ("ICH_PM_SS_STATE_LOW", ICH_PM_SS_STATE_LOW as i64),
            ("ICH_PMSIZE", ICH_PMSIZE as i64),
            ("ICH_SMB_BASE", ICH_SMB_BASE as i64),
            ("ICH_SMB_HOSTC", ICH_SMB_HOSTC as i64),
            ("ICH_SMB_HOSTC_HSTEN", ICH_SMB_HOSTC_HSTEN as i64),
            ("ICH_SMB_HOSTC_SMIEN", ICH_SMB_HOSTC_SMIEN as i64),
            ("ICH_SMB_HOSTC_I2CEN", ICH_SMB_HOSTC_I2CEN as i64),
            ("ICH_SMB_HS", ICH_SMB_HS as i64),
            ("ICH_SMB_HS_BUSY", ICH_SMB_HS_BUSY as i64),
            ("ICH_SMB_HS_INTR", ICH_SMB_HS_INTR as i64),
            ("ICH_SMB_HS_DEVERR", ICH_SMB_HS_DEVERR as i64),
            ("ICH_SMB_HS_BUSERR", ICH_SMB_HS_BUSERR as i64),
            ("ICH_SMB_HS_FAILED", ICH_SMB_HS_FAILED as i64),
            ("ICH_SMB_HS_SMBAL", ICH_SMB_HS_SMBAL as i64),
            ("ICH_SMB_HS_INUSE", ICH_SMB_HS_INUSE as i64),
            ("ICH_SMB_HS_BDONE", ICH_SMB_HS_BDONE as i64),
            ("ICH_SMB_HC", ICH_SMB_HC as i64),
            ("ICH_SMB_HC_INTREN", ICH_SMB_HC_INTREN as i64),
            ("ICH_SMB_HC_KILL", ICH_SMB_HC_KILL as i64),
            ("ICH_SMB_HC_CMD_QUICK", ICH_SMB_HC_CMD_QUICK as i64),
            ("ICH_SMB_HC_CMD_BYTE", ICH_SMB_HC_CMD_BYTE as i64),
            ("ICH_SMB_HC_CMD_BDATA", ICH_SMB_HC_CMD_BDATA as i64),
            ("ICH_SMB_HC_CMD_WDATA", ICH_SMB_HC_CMD_WDATA as i64),
            ("ICH_SMB_HC_CMD_PCALL", ICH_SMB_HC_CMD_PCALL as i64),
            ("ICH_SMB_HC_CMD_BLOCK", ICH_SMB_HC_CMD_BLOCK as i64),
            ("ICH_SMB_HC_CMD_I2CREAD", ICH_SMB_HC_CMD_I2CREAD as i64),
            ("ICH_SMB_HC_CMD_BLOCKP", ICH_SMB_HC_CMD_BLOCKP as i64),
            ("ICH_SMB_HC_LASTB", ICH_SMB_HC_LASTB as i64),
            ("ICH_SMB_HC_START", ICH_SMB_HC_START as i64),
            ("ICH_SMB_HC_PECEN", ICH_SMB_HC_PECEN as i64),
            ("ICH_SMB_HCMD", ICH_SMB_HCMD as i64),
            ("ICH_SMB_TXSLVA", ICH_SMB_TXSLVA as i64),
            ("ICH_SMB_TXSLVA_READ", ICH_SMB_TXSLVA_READ as i64),
            ("ICH_SMB_HD0", ICH_SMB_HD0 as i64),
            ("ICH_SMB_HD1", ICH_SMB_HD1 as i64),
            ("ICH_SMB_HBDB", ICH_SMB_HBDB as i64),
            ("ICH_SMB_PEC", ICH_SMB_PEC as i64),
            ("ICH_SMB_RXSLVA", ICH_SMB_RXSLVA as i64),
            ("ICH_SMB_SD", ICH_SMB_SD as i64),
            ("ICH_SMB_AS", ICH_SMB_AS as i64),
            ("ICH_SMB_AS_CRCE", ICH_SMB_AS_CRCE as i64),
            ("ICH_SMB_AS_TCO", ICH_SMB_AS_TCO as i64),
            ("ICH_SMB_AC", ICH_SMB_AC as i64),
            ("ICH_SMB_AC_AAC", ICH_SMB_AC_AAC as i64),
            ("ICH_SMB_AC_E32B", ICH_SMB_AC_E32B as i64),
            ("ICH_SMB_SMLPC", ICH_SMB_SMLPC as i64),
            ("ICH_SMB_SMLPC_LINK0", ICH_SMB_SMLPC_LINK0 as i64),
            ("ICH_SMB_SMLPC_LINK1", ICH_SMB_SMLPC_LINK1 as i64),
            ("ICH_SMB_SMLPC_CLKC", ICH_SMB_SMLPC_CLKC as i64),
            ("ICH_SMB_SMBPC", ICH_SMB_SMBPC as i64),
            ("ICH_SMB_SMBPC_CLK", ICH_SMB_SMBPC_CLK as i64),
            ("ICH_SMB_SMBPC_DATA", ICH_SMB_SMBPC_DATA as i64),
            ("ICH_SMB_SMBPC_CLKC", ICH_SMB_SMBPC_CLKC as i64),
            ("ICH_SMB_SS", ICH_SMB_SS as i64),
            ("ICH_SMB_SS_HN", ICH_SMB_SS_HN as i64),
            ("ICH_SMB_SCMD", ICH_SMB_SCMD as i64),
            ("ICH_SMB_SCMD_INTREN", ICH_SMB_SCMD_INTREN as i64),
            ("ICH_SMB_SCMD_WKEN", ICH_SMB_SCMD_WKEN as i64),
            ("ICH_SMB_SCMD_SMBALDS", ICH_SMB_SCMD_SMBALDS as i64),
            ("ICH_SMB_NDADDR", ICH_SMB_NDADDR as i64),
            ("ICH_SMB_NDLOW", ICH_SMB_NDLOW as i64),
            ("ICH_SMB_NDHIGH", ICH_SMB_NDHIGH as i64),
            ("ICH_WDT_BASE", ICH_WDT_BASE as i64),
            ("ICH_WDT_CONF", ICH_WDT_CONF as i64),
            ("ICH_WDT_CONF_MASK", ICH_WDT_CONF_MASK as i64),
            ("ICH_WDT_CONF_INT_MASK", ICH_WDT_CONF_INT_MASK as i64),
            ("ICH_WDT_CONF_INT_IRQ", ICH_WDT_CONF_INT_IRQ as i64),
            ("ICH_WDT_CONF_INT_SMI", ICH_WDT_CONF_INT_SMI as i64),
            ("ICH_WDT_CONF_INT_DIS", ICH_WDT_CONF_INT_DIS as i64),
            ("ICH_WDT_CONF_PRE", ICH_WDT_CONF_PRE as i64),
            ("ICH_WDT_CONF_OUTDIS", ICH_WDT_CONF_OUTDIS as i64),
            ("ICH_WDT_LOCK", ICH_WDT_LOCK as i64),
            ("ICH_WDT_LOCK_LOCKED", ICH_WDT_LOCK_LOCKED as i64),
            ("ICH_WDT_LOCK_ENABLED", ICH_WDT_LOCK_ENABLED as i64),
            ("ICH_WDT_LOCK_FREERUN", ICH_WDT_LOCK_FREERUN as i64),
            ("ICH_WDT_PRE1", ICH_WDT_PRE1 as i64),
            ("ICH_WDT_PRE2", ICH_WDT_PRE2 as i64),
            ("ICH_WDT_GIS", ICH_WDT_GIS as i64),
            ("ICH_WDT_GIS_ACTIVE", ICH_WDT_GIS_ACTIVE as i64),
            ("ICH_WDT_RELOAD", ICH_WDT_RELOAD as i64),
            ("ICH_WDT_RELOAD_RLD", ICH_WDT_RELOAD_RLD as i64),
            ("ICH_WDT_RELOAD_TIMEOUT", ICH_WDT_RELOAD_TIMEOUT as i64),
        ] {
            assert_eq!(crate::reftest::int(&defs, name), Some(value), "{name}");
        }
    }
}
/* </TESTS> */
