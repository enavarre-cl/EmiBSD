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
 * National Semiconductor DS8390 NIC register definitions.
 *
 * Copyright (C) 1993, David Greenman.  This software may be used, modified,
 * copied, distributed, and sold, in both source and binary form provided that
 * the above copyright and these terms are retained.  Under no circumstances is
 * the author responsible for the proper functioning of this software, nor does
 * the author assume any responsibility for damages incurred with its use.
 */
/* </LICENSES> */

/* <CODE> */
//! National Semiconductor DS8390 NIC register definitions (`dev/ic/dp8390reg.h`).
//!
//! Upstream: sys/dev/ic/dp8390reg.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - Register offsets are `usize` (they index `sc_reg_map`), register bits `u8`.
//! - `struct dp8390_ring` is [`Dp8390Ring`]; its `count` is read little-endian by
//!   `ne2000_read_hdr`, which is the C's `BYTE_ORDER` handling.

/// `ED_P0_CR`: Command Register.
pub const ED_P0_CR: usize = 0x00;
/// `ED_P0_CLDA0`: Current Local DMA Addr low (read).
pub const ED_P0_CLDA0: usize = 0x01;
/// `ED_P0_PSTART`: Page Start register (write).
pub const ED_P0_PSTART: usize = 0x01;
/// `ED_P0_CLDA1`: Current Local DMA Addr high (read).
pub const ED_P0_CLDA1: usize = 0x02;
/// `ED_P0_PSTOP`: Page Stop register (write).
pub const ED_P0_PSTOP: usize = 0x02;
/// `ED_P0_BNRY`: Boundary Pointer.
pub const ED_P0_BNRY: usize = 0x03;
/// `ED_P0_TSR`: Transmit Status Register (read).
pub const ED_P0_TSR: usize = 0x04;
/// `ED_P0_TPSR`: Transmit Page Start (write).
pub const ED_P0_TPSR: usize = 0x04;
/// `ED_P0_NCR`: Number of Collisions Reg (read).
pub const ED_P0_NCR: usize = 0x05;
/// `ED_P0_TBCR0`: Transmit Byte count, low (write).
pub const ED_P0_TBCR0: usize = 0x05;
/// `ED_P0_FIFO`: FIFO register (read).
pub const ED_P0_FIFO: usize = 0x06;
/// `ED_P0_TBCR1`: Transmit Byte count, high (write).
pub const ED_P0_TBCR1: usize = 0x06;
/// `ED_P0_ISR`: Interrupt Status Register.
pub const ED_P0_ISR: usize = 0x07;
/// `ED_P0_CRDA0`: Current Remote DMA Addr low (read).
pub const ED_P0_CRDA0: usize = 0x08;
/// `ED_P0_RSAR0`: Remote Start Address low (write).
pub const ED_P0_RSAR0: usize = 0x08;
/// `ED_P0_CRDA1`: Current Remote DMA Addr high (read).
pub const ED_P0_CRDA1: usize = 0x09;
/// `ED_P0_RSAR1`: Remote Start Address high (write).
pub const ED_P0_RSAR1: usize = 0x09;
/// `ED_P0_RBCR0`: Remote Byte Count low (write).
pub const ED_P0_RBCR0: usize = 0x0a;
/// `ED_P0_RBCR1`: Remote Byte Count high (write).
pub const ED_P0_RBCR1: usize = 0x0b;
/// `ED_P0_RSR`: Receive Status (read).
pub const ED_P0_RSR: usize = 0x0c;
/// `ED_P0_RCR`: Receive Configuration Reg (write).
pub const ED_P0_RCR: usize = 0x0c;
/// `ED_P0_CNTR0`: frame alignment error counter (read).
pub const ED_P0_CNTR0: usize = 0x0d;
/// `ED_P0_TCR`: Transmit Configuration Reg (write).
pub const ED_P0_TCR: usize = 0x0d;
/// `ED_P0_CNTR1`: CRC error counter (read).
pub const ED_P0_CNTR1: usize = 0x0e;
/// `ED_P0_DCR`: Data Configuration Reg (write).
pub const ED_P0_DCR: usize = 0x0e;
/// `ED_P0_CNTR2`: missed packet counter (read).
pub const ED_P0_CNTR2: usize = 0x0f;
/// `ED_P0_IMR`: Interrupt Mask Register (write).
pub const ED_P0_IMR: usize = 0x0f;
/// `ED_P1_CR`: Command Register.
pub const ED_P1_CR: usize = 0x00;
/// `ED_P1_PAR0`: Physical Address Register 0.
pub const ED_P1_PAR0: usize = 0x01;
/// `ED_P1_PAR1`: Physical Address Register 1.
pub const ED_P1_PAR1: usize = 0x02;
/// `ED_P1_PAR2`: Physical Address Register 2.
pub const ED_P1_PAR2: usize = 0x03;
/// `ED_P1_PAR3`: Physical Address Register 3.
pub const ED_P1_PAR3: usize = 0x04;
/// `ED_P1_PAR4`: Physical Address Register 4.
pub const ED_P1_PAR4: usize = 0x05;
/// `ED_P1_PAR5`: Physical Address Register 5.
pub const ED_P1_PAR5: usize = 0x06;
/// `ED_P1_CURR`: Current RX ring-buffer page.
pub const ED_P1_CURR: usize = 0x07;
/// `ED_P1_MAR0`: Multicast Address Register 0.
pub const ED_P1_MAR0: usize = 0x08;
/// `ED_P1_MAR1`: Multicast Address Register 1.
pub const ED_P1_MAR1: usize = 0x09;
/// `ED_P1_MAR2`: Multicast Address Register 2.
pub const ED_P1_MAR2: usize = 0x0a;
/// `ED_P1_MAR3`: Multicast Address Register 3.
pub const ED_P1_MAR3: usize = 0x0b;
/// `ED_P1_MAR4`: Multicast Address Register 4.
pub const ED_P1_MAR4: usize = 0x0c;
/// `ED_P1_MAR5`: Multicast Address Register 5.
pub const ED_P1_MAR5: usize = 0x0d;
/// `ED_P1_MAR6`: Multicast Address Register 6.
pub const ED_P1_MAR6: usize = 0x0e;
/// `ED_P1_MAR7`: Multicast Address Register 7.
pub const ED_P1_MAR7: usize = 0x0f;
/// `ED_P2_CR`: Command Register.
pub const ED_P2_CR: usize = 0x00;
/// `ED_P2_PSTART`: Page Start (read).
pub const ED_P2_PSTART: usize = 0x01;
/// `ED_P2_CLDA0`: Current Local DMA Addr 0 (write).
pub const ED_P2_CLDA0: usize = 0x01;
/// `ED_P2_PSTOP`: Page Stop (read).
pub const ED_P2_PSTOP: usize = 0x02;
/// `ED_P2_CLDA1`: Current Local DMA Addr 1 (write).
pub const ED_P2_CLDA1: usize = 0x02;
/// `ED_P2_RNPP`: Remote Next Packet Pointer.
pub const ED_P2_RNPP: usize = 0x03;
/// `ED_P2_TPSR`: Transmit Page Start (read).
pub const ED_P2_TPSR: usize = 0x04;
/// `ED_P2_LNPP`: Local Next Packet Pointer.
pub const ED_P2_LNPP: usize = 0x05;
/// `ED_P2_ACU`: Address Counter Upper.
pub const ED_P2_ACU: usize = 0x06;
/// `ED_P2_ACL`: Address Counter Lower.
pub const ED_P2_ACL: usize = 0x07;
/// `ED_P2_RCR`: Receive Configuration Register (read).
pub const ED_P2_RCR: usize = 0x0c;
/// `ED_P2_TCR`: Transmit Configuration Register (read).
pub const ED_P2_TCR: usize = 0x0d;
/// `ED_P2_DCR`: Data Configuration Register (read).
pub const ED_P2_DCR: usize = 0x0e;
/// `ED_P2_IMR`: Interrupt Mask Register (read).
pub const ED_P2_IMR: usize = 0x0f;
/// `ED_CR_STP`.
pub const ED_CR_STP: u8 = 0x01;
/// `ED_CR_STA`.
pub const ED_CR_STA: u8 = 0x02;
/// `ED_CR_TXP`.
pub const ED_CR_TXP: u8 = 0x04;
/// `ED_CR_RD0`.
pub const ED_CR_RD0: u8 = 0x08;
/// `ED_CR_RD1`.
pub const ED_CR_RD1: u8 = 0x10;
/// `ED_CR_RD2`.
pub const ED_CR_RD2: u8 = 0x20;
/// `ED_CR_PS0`.
pub const ED_CR_PS0: u8 = 0x40;
/// `ED_CR_PS1`.
pub const ED_CR_PS1: u8 = 0x80;
/// `ED_CR_PAGE_0`: (for consistency).
pub const ED_CR_PAGE_0: u8 = 0x00;
/// `ED_CR_PAGE_1`.
pub const ED_CR_PAGE_1: u8 = ED_CR_PS0;
/// `ED_CR_PAGE_2`.
pub const ED_CR_PAGE_2: u8 = ED_CR_PS1;
/// `ED_CR_PAGE_3`.
pub const ED_CR_PAGE_3: u8 = ED_CR_PS1 | ED_CR_PS0;
/// `ED_ISR_PRX`.
pub const ED_ISR_PRX: u8 = 0x01;
/// `ED_ISR_PTX`.
pub const ED_ISR_PTX: u8 = 0x02;
/// `ED_ISR_RXE`.
pub const ED_ISR_RXE: u8 = 0x04;
/// `ED_ISR_TXE`.
pub const ED_ISR_TXE: u8 = 0x08;
/// `ED_ISR_OVW`.
pub const ED_ISR_OVW: u8 = 0x10;
/// `ED_ISR_CNT`.
pub const ED_ISR_CNT: u8 = 0x20;
/// `ED_ISR_RDC`.
pub const ED_ISR_RDC: u8 = 0x40;
/// `ED_ISR_RST`.
pub const ED_ISR_RST: u8 = 0x80;
/// `ED_IMR_PRXE`.
pub const ED_IMR_PRXE: u8 = 0x01;
/// `ED_IMR_PTXE`.
pub const ED_IMR_PTXE: u8 = 0x02;
/// `ED_IMR_RXEE`.
pub const ED_IMR_RXEE: u8 = 0x04;
/// `ED_IMR_TXEE`.
pub const ED_IMR_TXEE: u8 = 0x08;
/// `ED_IMR_OVWE`.
pub const ED_IMR_OVWE: u8 = 0x10;
/// `ED_IMR_CNTE`.
pub const ED_IMR_CNTE: u8 = 0x20;
/// `ED_IMR_RDCE`.
pub const ED_IMR_RDCE: u8 = 0x40;
/// `ED_DCR_WTS`.
pub const ED_DCR_WTS: u8 = 0x01;
/// `ED_DCR_BOS`.
pub const ED_DCR_BOS: u8 = 0x02;
/// `ED_DCR_LAS`.
pub const ED_DCR_LAS: u8 = 0x04;
/// `ED_DCR_LS`.
pub const ED_DCR_LS: u8 = 0x08;
/// `ED_DCR_AR`.
pub const ED_DCR_AR: u8 = 0x10;
/// `ED_DCR_FT0`.
pub const ED_DCR_FT0: u8 = 0x20;
/// `ED_DCR_FT1`.
pub const ED_DCR_FT1: u8 = 0x40;
/// `ED_TCR_CRC`.
pub const ED_TCR_CRC: u8 = 0x01;
/// `ED_TCR_LB0`.
pub const ED_TCR_LB0: u8 = 0x02;
/// `ED_TCR_LB1`.
pub const ED_TCR_LB1: u8 = 0x04;
/// `ED_TCR_ATD`.
pub const ED_TCR_ATD: u8 = 0x08;
/// `ED_TCR_OFST`.
pub const ED_TCR_OFST: u8 = 0x10;
/// `ED_TSR_PTX`.
pub const ED_TSR_PTX: u8 = 0x01;
/// `ED_TSR_COL`.
pub const ED_TSR_COL: u8 = 0x04;
/// `ED_TSR_ABT`.
pub const ED_TSR_ABT: u8 = 0x08;
/// `ED_TSR_CRS`.
pub const ED_TSR_CRS: u8 = 0x10;
/// `ED_TSR_FU`.
pub const ED_TSR_FU: u8 = 0x20;
/// `ED_TSR_CDH`.
pub const ED_TSR_CDH: u8 = 0x40;
/// `ED_TSR_OWC`.
pub const ED_TSR_OWC: u8 = 0x80;
/// `ED_RCR_SEP`.
pub const ED_RCR_SEP: u8 = 0x01;
/// `ED_RCR_AR`.
pub const ED_RCR_AR: u8 = 0x02;
/// `ED_RCR_AB`.
pub const ED_RCR_AB: u8 = 0x04;
/// `ED_RCR_AM`.
pub const ED_RCR_AM: u8 = 0x08;
/// `ED_RCR_PRO`.
pub const ED_RCR_PRO: u8 = 0x10;
/// `ED_RCR_MON`.
pub const ED_RCR_MON: u8 = 0x20;
/// `ED_RCR_INTT`.
pub const ED_RCR_INTT: u8 = 0x40;
/// `ED_RSR_PRX`.
pub const ED_RSR_PRX: u8 = 0x01;
/// `ED_RSR_CRC`.
pub const ED_RSR_CRC: u8 = 0x02;
/// `ED_RSR_FAE`.
pub const ED_RSR_FAE: u8 = 0x04;
/// `ED_RSR_FO`.
pub const ED_RSR_FO: u8 = 0x08;
/// `ED_RSR_MPA`.
pub const ED_RSR_MPA: u8 = 0x10;
/// `ED_RSR_PHY`.
pub const ED_RSR_PHY: u8 = 0x20;
/// `ED_RSR_DIS`.
pub const ED_RSR_DIS: u8 = 0x40;
/// `ED_RSR_DFR`.
pub const ED_RSR_DFR: u8 = 0x80;
/// `ED_RING_RSR`: receiver status.
pub const ED_RING_RSR: usize = 0;
/// `ED_RING_NEXT_PACKET`: pointer to next packet.
pub const ED_RING_NEXT_PACKET: usize = 1;
/// `ED_RING_COUNT`: bytes in packet (length + 4).
pub const ED_RING_COUNT: usize = 2;
/// `ED_RING_HDRSZ`: Header size.
pub const ED_RING_HDRSZ: usize = 4;
/// `ED_PAGE_SIZE`: Size of RAM pages in bytes.
pub const ED_PAGE_SIZE: usize = 256;
/// `ED_PAGE_MASK`.
pub const ED_PAGE_MASK: usize = 255;
/// `ED_PAGE_SHIFT`.
pub const ED_PAGE_SHIFT: usize = 8;
/// `ED_TXBUF_SIZE`: Size of TX buffer in pages.
pub const ED_TXBUF_SIZE: usize = 6;

/// `struct dp8390_ring`: the header the NIC puts before each received packet.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[repr(C)]
pub struct Dp8390Ring {
    /// `rsr`: receiver status.
    pub rsr: u8,
    /// `next_packet`: pointer to next packet.
    pub next_packet: u8,
    /// `count`: bytes in packet (length + 4).
    pub count: u16,
}
/* </CODE> */
