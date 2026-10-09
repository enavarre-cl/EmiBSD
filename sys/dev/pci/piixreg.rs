/* <LICENSES> */
/*
 * Copyright (c) 2005 Alexander Yurchenko <grange@openbsd.org>
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
//! `<dev/pci/piixreg.h>`: Intel PCI-to-ISA / IDE Xcelerator (PIIX) register definitions: the
//! power management function's SMBus controller (`piixpm(4)`), and the AMD SB800 and FCH
//! registers the same driver reaches it through.
//!
//! Upstream: sys/dev/pci/piixreg.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - Types follow their use: PCI configuration offsets are `i32` and their contents `u32` (as in
//!   `pcireg.rs`), I/O offsets and sizes are `usize` (`bus_size_t`), the SMBus I/O registers'
//!   bits and the SB800/FCH index values are `u8` (the `SMB0EN` bits `u16`, as the register is
//!   read 16 bits wide).
//! - `PIIX_SMB_HS_BITS` is a byte string for the `%b` adaptor (`Bitmask`): the C's octal
//!   escapes as hex ones.
//! - `PIIX_SMB_TXSLVA_ADDR(x)` is the lower-case `const fn` `piix_smb_txslva_addr`.

// PCI configuration registers
/// `PIIX_SMB_BASE`: SMBus base address.
pub const PIIX_SMB_BASE: i32 = 0x90;
/// `PIIX_SMB_BASE_MASK`.
pub const PIIX_SMB_BASE_MASK: u32 = 0xfffe;
/// `PIIX_SMB_HOSTC`: SMBus host configuration.
pub const PIIX_SMB_HOSTC: i32 = 0xd0;
/// `PIIX_SMB_HOSTC_HSTEN`: enable host controller.
pub const PIIX_SMB_HOSTC_HSTEN: u32 = 1 << 16;
/// `PIIX_SMB_HOSTC_SMI`: SMI.
pub const PIIX_SMB_HOSTC_SMI: u32 = 0;
/// `PIIX_SMB_HOSTC_IRQ`: IRQ.
pub const PIIX_SMB_HOSTC_IRQ: u32 = 4 << 17;
/// `PIIX_SMB_HOSTC_INTMASK`.
pub const PIIX_SMB_HOSTC_INTMASK: u32 = 7 << 17;
// SMBus I/O registers
/// `PIIX_SMB_HS`: host status.
pub const PIIX_SMB_HS: usize = 0x00;
/// `PIIX_SMB_HS_BUSY`: running a command.
pub const PIIX_SMB_HS_BUSY: u8 = 1;
/// `PIIX_SMB_HS_INTR`: command completed.
pub const PIIX_SMB_HS_INTR: u8 = 1 << 1;
/// `PIIX_SMB_HS_DEVERR`: command error.
pub const PIIX_SMB_HS_DEVERR: u8 = 1 << 2;
/// `PIIX_SMB_HS_BUSERR`: transaction collision.
pub const PIIX_SMB_HS_BUSERR: u8 = 1 << 3;
/// `PIIX_SMB_HS_FAILED`: failed bus transaction.
pub const PIIX_SMB_HS_FAILED: u8 = 1 << 4;
/// `PIIX_SMB_HC`: host control.
pub const PIIX_SMB_HC: usize = 0x02;
/// `PIIX_SMB_HC_INTREN`: enable interrupts.
pub const PIIX_SMB_HC_INTREN: u8 = 1;
/// `PIIX_SMB_HC_KILL`: kill current transaction.
pub const PIIX_SMB_HC_KILL: u8 = 1 << 1;
/// `PIIX_SMB_HC_CMD_QUICK`: QUICK command.
pub const PIIX_SMB_HC_CMD_QUICK: u8 = 0;
/// `PIIX_SMB_HC_CMD_BYTE`: BYTE command.
pub const PIIX_SMB_HC_CMD_BYTE: u8 = 1 << 2;
/// `PIIX_SMB_HC_CMD_BDATA`: BYTE DATA command.
pub const PIIX_SMB_HC_CMD_BDATA: u8 = 2 << 2;
/// `PIIX_SMB_HC_CMD_WDATA`: WORD DATA command.
pub const PIIX_SMB_HC_CMD_WDATA: u8 = 3 << 2;
/// `PIIX_SMB_HC_CMD_BLOCK`: BLOCK command.
pub const PIIX_SMB_HC_CMD_BLOCK: u8 = 5 << 2;
/// `PIIX_SMB_HC_START`: start transaction.
pub const PIIX_SMB_HC_START: u8 = 1 << 6;
/// `PIIX_SMB_HCMD`: host command.
pub const PIIX_SMB_HCMD: usize = 0x03;
/// `PIIX_SMB_TXSLVA`: transmit slave address.
pub const PIIX_SMB_TXSLVA: usize = 0x04;
/// `PIIX_SMB_TXSLVA_READ`: read direction.
pub const PIIX_SMB_TXSLVA_READ: u8 = 1;
/// `PIIX_SMB_HD0`: host data 0.
pub const PIIX_SMB_HD0: usize = 0x05;
/// `PIIX_SMB_HD1`: host data 1.
pub const PIIX_SMB_HD1: usize = 0x06;
/// `PIIX_SMB_HBDB`: host block data byte.
pub const PIIX_SMB_HBDB: usize = 0x07;
/// `PIIX_SMB_SC`: slave control.
pub const PIIX_SMB_SC: usize = 0x08;
/// `PIIX_SMB_SC_ALERTEN`: enable SMBALERT#.
pub const PIIX_SMB_SC_ALERTEN: u8 = 1 << 3;
/// `PIIX_SMB_SIZE`: SMBus I/O space size.
pub const PIIX_SMB_SIZE: usize = 0x10;
// AMD SB800 configuration registers
/// `SB800_PMREG_BASE`.
pub const SB800_PMREG_BASE: usize = 0xcd6;
/// `SB800_PMREG_SIZE`: index/data pair.
pub const SB800_PMREG_SIZE: usize = 2;
/// `SB800_PMREG_SMB0EN`: 16-bit register.
pub const SB800_PMREG_SMB0EN: u8 = 0x2c;
/// `SB800_PMREG_SMB0SEL`: bus selection.
pub const SB800_PMREG_SMB0SEL: u8 = 0x2e;
/// `SB800_PMREG_SMB0SELEN`: bus selection enable.
pub const SB800_PMREG_SMB0SELEN: u8 = 0x2f;
/// `SB800_SMB0EN_EN`.
pub const SB800_SMB0EN_EN: u16 = 0x0001;
/// `SB800_SMB0EN_BASE_MASK`.
pub const SB800_SMB0EN_BASE_MASK: u16 = 0xffe0;
/// `SB800_SMB0SELEN_EN`.
pub const SB800_SMB0SELEN_EN: u8 = 0x01;
/// `SB800_SMB_HOSTC`: I2C bus configuration.
pub const SB800_SMB_HOSTC: usize = 0x10;
/// `SB800_SMB_HOSTC_INTMASK`: 0: SMI 1: IRQ.
pub const SB800_SMB_HOSTC_INTMASK: u8 = 0x1;
/// `SB800_SMB_SIZE`: SMBus I/O space size.
pub const SB800_SMB_SIZE: usize = 0x14;
/// `AMDFCH41_PM_DECODE_EN`: 16-bit register.
pub const AMDFCH41_PM_DECODE_EN: u8 = 0x00;
/// `AMDFCH41_PM_PORT_INDEX`.
pub const AMDFCH41_PM_PORT_INDEX: u8 = 0x02;
/// `AMDFCH41_SMBUS_EN`.
pub const AMDFCH41_SMBUS_EN: u8 = 0x10;

/// `PIIX_SMB_HS_BITS`: the names of the host status bits, for `%b`.
pub const PIIX_SMB_HS_BITS: &[u8] = b"\x10\x01BUSY\x02INTR\x03DEVERR\x04BUSERR\x05FAILED";

/// `PIIX_SMB_TXSLVA_ADDR(x)`: the 7-bit address in the transmit slave address register.
pub const fn piix_smb_txslva_addr(x: u16) -> u8 {
    ((x & 0x7f) << 1) as u8
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interrupt_selection_masks() {
        // IRQ and SMI are the two settings of the 3-bit field at bit 17.
        assert_eq!(
            PIIX_SMB_HOSTC_IRQ & PIIX_SMB_HOSTC_INTMASK,
            PIIX_SMB_HOSTC_IRQ
        );
        assert_eq!(PIIX_SMB_HOSTC_SMI & PIIX_SMB_HOSTC_INTMASK, 0);
        assert_eq!(piix_smb_txslva_addr(0x50), 0xa0);
        assert_eq!(PIIX_SMB_BASE_MASK & 0x1_0001, 0);
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/pci/piixreg.h");
        for (name, value) in [
            ("PIIX_SMB_BASE", PIIX_SMB_BASE as i64),
            ("PIIX_SMB_BASE_MASK", PIIX_SMB_BASE_MASK as i64),
            ("PIIX_SMB_HOSTC", PIIX_SMB_HOSTC as i64),
            ("PIIX_SMB_HOSTC_HSTEN", PIIX_SMB_HOSTC_HSTEN as i64),
            ("PIIX_SMB_HOSTC_SMI", PIIX_SMB_HOSTC_SMI as i64),
            ("PIIX_SMB_HOSTC_IRQ", PIIX_SMB_HOSTC_IRQ as i64),
            ("PIIX_SMB_HOSTC_INTMASK", PIIX_SMB_HOSTC_INTMASK as i64),
            ("PIIX_SMB_HS", PIIX_SMB_HS as i64),
            ("PIIX_SMB_HS_BUSY", PIIX_SMB_HS_BUSY as i64),
            ("PIIX_SMB_HS_INTR", PIIX_SMB_HS_INTR as i64),
            ("PIIX_SMB_HS_DEVERR", PIIX_SMB_HS_DEVERR as i64),
            ("PIIX_SMB_HS_BUSERR", PIIX_SMB_HS_BUSERR as i64),
            ("PIIX_SMB_HS_FAILED", PIIX_SMB_HS_FAILED as i64),
            ("PIIX_SMB_HC", PIIX_SMB_HC as i64),
            ("PIIX_SMB_HC_INTREN", PIIX_SMB_HC_INTREN as i64),
            ("PIIX_SMB_HC_KILL", PIIX_SMB_HC_KILL as i64),
            ("PIIX_SMB_HC_CMD_QUICK", PIIX_SMB_HC_CMD_QUICK as i64),
            ("PIIX_SMB_HC_CMD_BYTE", PIIX_SMB_HC_CMD_BYTE as i64),
            ("PIIX_SMB_HC_CMD_BDATA", PIIX_SMB_HC_CMD_BDATA as i64),
            ("PIIX_SMB_HC_CMD_WDATA", PIIX_SMB_HC_CMD_WDATA as i64),
            ("PIIX_SMB_HC_CMD_BLOCK", PIIX_SMB_HC_CMD_BLOCK as i64),
            ("PIIX_SMB_HC_START", PIIX_SMB_HC_START as i64),
            ("PIIX_SMB_HCMD", PIIX_SMB_HCMD as i64),
            ("PIIX_SMB_TXSLVA", PIIX_SMB_TXSLVA as i64),
            ("PIIX_SMB_TXSLVA_READ", PIIX_SMB_TXSLVA_READ as i64),
            ("PIIX_SMB_HD0", PIIX_SMB_HD0 as i64),
            ("PIIX_SMB_HD1", PIIX_SMB_HD1 as i64),
            ("PIIX_SMB_HBDB", PIIX_SMB_HBDB as i64),
            ("PIIX_SMB_SC", PIIX_SMB_SC as i64),
            ("PIIX_SMB_SC_ALERTEN", PIIX_SMB_SC_ALERTEN as i64),
            ("PIIX_SMB_SIZE", PIIX_SMB_SIZE as i64),
            ("SB800_PMREG_BASE", SB800_PMREG_BASE as i64),
            ("SB800_PMREG_SIZE", SB800_PMREG_SIZE as i64),
            ("SB800_PMREG_SMB0EN", SB800_PMREG_SMB0EN as i64),
            ("SB800_PMREG_SMB0SEL", SB800_PMREG_SMB0SEL as i64),
            ("SB800_PMREG_SMB0SELEN", SB800_PMREG_SMB0SELEN as i64),
            ("SB800_SMB0EN_EN", SB800_SMB0EN_EN as i64),
            ("SB800_SMB0EN_BASE_MASK", SB800_SMB0EN_BASE_MASK as i64),
            ("SB800_SMB0SELEN_EN", SB800_SMB0SELEN_EN as i64),
            ("SB800_SMB_HOSTC", SB800_SMB_HOSTC as i64),
            ("SB800_SMB_HOSTC_INTMASK", SB800_SMB_HOSTC_INTMASK as i64),
            ("SB800_SMB_SIZE", SB800_SMB_SIZE as i64),
            ("AMDFCH41_PM_DECODE_EN", AMDFCH41_PM_DECODE_EN as i64),
            ("AMDFCH41_PM_PORT_INDEX", AMDFCH41_PM_PORT_INDEX as i64),
            ("AMDFCH41_SMBUS_EN", AMDFCH41_SMBUS_EN as i64),
        ] {
            assert_eq!(crate::reftest::int(&defs, name), Some(value), "{name}");
        }
    }
}
/* </TESTS> */
