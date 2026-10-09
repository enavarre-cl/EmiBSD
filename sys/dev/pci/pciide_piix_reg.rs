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
 * Copyright (c) 1998 Manuel Bouyer.
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
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR ``AS IS'' AND ANY EXPRESS OR
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES
 * OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED.
 * IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY DIRECT, INDIRECT,
 * INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT
 * NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
 * DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
 * THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
 * (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF
 * THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 *
 */
/* </LICENSES> */

/* <CODE> */
//! Intel PIIX/PIIX3/PIIX4, ICH and ICH5-7 SATA IDE controller registers.
//!
//! Upstream: sys/dev/pci/pciide_piix_reg.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - The function-like macros are `const fn`s named in lower case. The configuration-space
//!   offsets `pciide.c` reads and writes are `i32` (`pci_conf_read`'s `reg`), bus space
//!   offsets `BusSize`, shift counts `i32`, register values and unused offsets `u32`.
//! - The `static` timing tables are `pub const` arrays named in capitals, of `u8` where the
//!   C has `int8_t` or `u_int8_t` (every entry is written to a byte-wide register or shifted
//!   into a field, never sign-extended).
#![allow(non_upper_case_globals)] // the C's mixed-case names (`PDC2xx_STATE`, `ACER_0x4B`)

// Registers definitions for Intel's PIIX series PCI IDE controllers.
// See Intel's
// "82371FB PIIX and 82371SB PIIX3 PCI ISA IDE XCELERATOR"
// "82371AB PCI-TO-ISA / IDE XCELERATOR PIIX4" and
// "Intel 82801AA ICH and Intel 82801AB ICH0 I/O Controller Hub"
// available from http://developer.intel.com/

// Bus master interface base address register
/// `PIIX_BMIBA`.
pub const PIIX_BMIBA: u32 = 0x20;
/// `PIIX_BMIBA_ADDR`.
pub const fn piix_bmiba_addr(x: u32) -> u32 {
    x & 0x0000FFFF0
}
/// `PIIX_BMIBA_RTE`.
pub const fn piix_bmiba_rte(x: u32) -> u32 {
    x & 0x000000001
}
/// `PIIX_BMIBA_RTE_IO`: base addr maps to I/O space.
pub const PIIX_BMIBA_RTE_IO: u32 = 0x000000001;

// IDE timing register
// 0x40/0x41 is for primary, 0x42/0x43 for secondary channel
/// `PIIX_IDETIM`.
pub const PIIX_IDETIM: i32 = 0x40;
/// `PIIX_IDETIM_READ`.
pub const fn piix_idetim_read(x: u32, channel: i32) -> u32 {
    (x >> (16 * channel)) & 0x0000FFFF
}
/// `PIIX_IDETIM_SET`.
pub const fn piix_idetim_set(x: u32, bytes: u32, channel: i32) -> u32 {
    x | (bytes << (16 * channel))
}
/// `PIIX_IDETIM_CLEAR`.
pub const fn piix_idetim_clear(x: u32, bytes: u32, channel: i32) -> u32 {
    x & !(bytes << (16 * channel))
}

/// `PIIX_IDETIM_IDE`: PIIX decode IDE registers.
pub const PIIX_IDETIM_IDE: u32 = 0x8000;
/// `PIIX_IDETIM_SITRE`: slaves IDE timing registers enabled (PIIX3/4 only).
pub const PIIX_IDETIM_SITRE: u32 = 0x4000;
/// `PIIX_IDETIM_ISP_MASK`: IOrdy sample point.
pub const PIIX_IDETIM_ISP_MASK: u32 = 0x3000;
/// `PIIX_IDETIM_ISP_SHIFT`.
pub const PIIX_IDETIM_ISP_SHIFT: i32 = 12;
/// `PIIX_IDETIM_ISP_SET`.
pub const fn piix_idetim_isp_set(x: u32) -> u32 {
    x << PIIX_IDETIM_ISP_SHIFT
}
/// `PIIX_IDETIM_RTC_MASK`: recovery time.
pub const PIIX_IDETIM_RTC_MASK: u32 = 0x0300;
/// `PIIX_IDETIM_RTC_SHIFT`.
pub const PIIX_IDETIM_RTC_SHIFT: i32 = 8;
/// `PIIX_IDETIM_RTC_SET`.
pub const fn piix_idetim_rtc_set(x: u32) -> u32 {
    x << PIIX_IDETIM_RTC_SHIFT
}
/// `PIIX_IDETIM_DTE`: DMA timing only.
pub const fn piix_idetim_dte(d: i32) -> u32 {
    0x0008 << (4 * d)
}
/// `PIIX_IDETIM_PPE`: prefetch/posting.
pub const fn piix_idetim_ppe(d: i32) -> u32 {
    0x0004 << (4 * d)
}
/// `PIIX_IDETIM_IE`: IORDY enable.
pub const fn piix_idetim_ie(d: i32) -> u32 {
    0x0002 << (4 * d)
}
/// `PIIX_IDETIM_TIME`: Fast timing enable.
pub const fn piix_idetim_time(d: i32) -> u32 {
    0x0001 << (4 * d)
}
// Slave IDE timing register (PIIX3/4 only)
// This register must be enabled via the PIIX_IDETIM_SITRE bit
/// `PIIX_SIDETIM`.
pub const PIIX_SIDETIM: i32 = 0x44;
/// `PIIX_SIDETIM_ISP_MASK`.
pub const fn piix_sidetim_isp_mask(channel: i32) -> u32 {
    0x0c << (channel * 4)
}
/// `PIIX_SIDETIM_ISP_SHIFT`.
pub const PIIX_SIDETIM_ISP_SHIFT: i32 = 2;
/// `PIIX_SIDETIM_ISP_SET`.
pub const fn piix_sidetim_isp_set(x: u32, channel: i32) -> u32 {
    x << (PIIX_SIDETIM_ISP_SHIFT + (channel * 4))
}
/// `PIIX_SIDETIM_RTC_MASK`.
pub const fn piix_sidetim_rtc_mask(channel: i32) -> u32 {
    0x03 << (channel * 4)
}
/// `PIIX_SIDETIM_RTC_SHIFT`.
pub const PIIX_SIDETIM_RTC_SHIFT: i32 = 0;
/// `PIIX_SIDETIM_RTC_SET`.
pub const fn piix_sidetim_rtc_set(x: u32, channel: i32) -> u32 {
    x << (PIIX_SIDETIM_RTC_SHIFT + (channel * 4))
}

// Ultra DMA/33 register (PIIX4 only)
/// `PIIX_UDMAREG`.
pub const PIIX_UDMAREG: i32 = 0x48;
// Control register
/// `PIIX_UDMACTL_DRV_EN`.
pub const fn piix_udmactl_drv_en(channel: i32, drive: i32) -> u32 {
    0x01 << (channel * 2 + drive)
}
// Ultra DMA/33 timing register (PIIX4 only)
/// `PIIX_UDMATIM_SHIFT`.
pub const PIIX_UDMATIM_SHIFT: i32 = 16;
/// `PIIX_UDMATIM_SET`.
pub const fn piix_udmatim_set(x: u32, channel: i32, drive: i32) -> u32 {
    (x << ((channel * 8) + (drive * 4))) << PIIX_UDMATIM_SHIFT
}

// IDE config register (ICH/ICH0/ICH2 only)
/// `PIIX_CONFIG`.
pub const PIIX_CONFIG: i32 = 0x54;
/// `PIIX_CONFIG_PINGPONG`.
pub const PIIX_CONFIG_PINGPONG: u32 = 0x0400;
// The following are only for the 82801AA ICH and 82801BA ICH2
/// `PIIX_CONFIG_CR`.
pub const fn piix_config_cr(channel: i32, drive: i32) -> u32 {
    0x0010 << (channel * 2 + drive)
}
/// `PIIX_CONFIG_UDMA66`.
pub const fn piix_config_udma66(channel: i32, drive: i32) -> u32 {
    0x0001 << (channel * 2 + drive)
}
// The following are only for the 82801BA ICH2
/// `PIIX_CONFIG_UDMA100`.
pub const fn piix_config_udma100(channel: i32, drive: i32) -> u32 {
    0x1000 << (channel * 2 + drive)
}

// these tables define the different values to upload to the
// ISP and RTC registers for the various PIO and DMA mode
// (from the PIIX4 doc).
/// `piix_isp_pio`.
pub const PIIX_ISP_PIO: [u8; 5] = [0x00, 0x00, 0x01, 0x02, 0x02];
/// `piix_rtc_pio`.
pub const PIIX_RTC_PIO: [u8; 5] = [0x00, 0x00, 0x00, 0x01, 0x03];
/// `piix_isp_dma`.
pub const PIIX_ISP_DMA: [u8; 3] = [0x00, 0x02, 0x02];
/// `piix_rtc_dma`.
pub const PIIX_RTC_DMA: [u8; 3] = [0x00, 0x02, 0x03];
/// `piix4_sct_udma`.
pub const PIIX4_SCT_UDMA: [u8; 6] = [0x00, 0x01, 0x02, 0x01, 0x02, 0x01];

// ICH5/ICH5R SATA registers definitions
/// `ICH5_SATA_MAP`: Address Map Register.
pub const ICH5_SATA_MAP: i32 = 0x90;
/// `ICH5_SATA_MAP_MV_MASK`: Map Value mask.
pub const ICH5_SATA_MAP_MV_MASK: u32 = 0x07;
/// `ICH5_SATA_MAP_COMBINED`: Combined mode.
pub const ICH5_SATA_MAP_COMBINED: u32 = 0x04;

/// `ICH5_SATA_PI`: Program Interface register.
pub const ICH5_SATA_PI: i32 = 0x09;
/// `ICH5_SATA_PI_PRI_NATIVE`: Put Pri IDE channel in native mode.
pub const ICH5_SATA_PI_PRI_NATIVE: u32 = 0x01;
/// `ICH5_SATA_PI_SEC_NATIVE`: Put Sec IDE channel in native mode.
pub const ICH5_SATA_PI_SEC_NATIVE: u32 = 0x04;

/// `ICH_SATA_PCS`: Port Control and Status Register.
pub const ICH_SATA_PCS: i32 = 0x92;
/// `ICH_SATA_PCS_P0E`: Port 0 enabled.
pub const ICH_SATA_PCS_P0E: u32 = 0x01;
/// `ICH_SATA_PCS_P1E`: Port 1 enabled.
pub const ICH_SATA_PCS_P1E: u32 = 0x02;
/// `ICH_SATA_PCS_P0P`: Port 0 present.
pub const ICH_SATA_PCS_P0P: u32 = 0x10;
/// `ICH_SATA_PCS_P1P`: Port 1 present.
pub const ICH_SATA_PCS_P1P: u32 = 0x20;

// ICH6/ICH7 SATA registers definitions
/// `ICH6_SATA_MAP_CMB_MASK`: Combined mode bits.
pub const ICH6_SATA_MAP_CMB_MASK: u32 = 0x03;
/// `ICH6_SATA_MAP_CMB_PRI`: Combined mode, IDE Primary.
pub const ICH6_SATA_MAP_CMB_PRI: u32 = 0x01;
/// `ICH6_SATA_MAP_CMB_SEC`: Combined mode, IDE Secondary.
pub const ICH6_SATA_MAP_CMB_SEC: u32 = 0x02;
/// `ICH7_SATA_MAP_SMS_MASK`: SATA Mode Select.
pub const ICH7_SATA_MAP_SMS_MASK: u32 = 0xc0;
/// `ICH7_SATA_MAP_SMS_IDE`.
pub const ICH7_SATA_MAP_SMS_IDE: u32 = 0x00;
/// `ICH7_SATA_MAP_SMS_AHCI`.
pub const ICH7_SATA_MAP_SMS_AHCI: u32 = 0x40;
/// `ICH7_SATA_MAP_SMS_RAID`.
pub const ICH7_SATA_MAP_SMS_RAID: u32 = 0x80;
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn defines_match_the_header() {
        let defs = crate::reftest::defines("sys/dev/pci/pciide_piix_reg.h");

        crate::reftest::assert_defines!(defs; PIIX_BMIBA, PIIX_BMIBA_RTE_IO, PIIX_IDETIM, PIIX_IDETIM_IDE, PIIX_IDETIM_SITRE, PIIX_IDETIM_ISP_MASK, PIIX_IDETIM_ISP_SHIFT, PIIX_IDETIM_RTC_MASK, PIIX_IDETIM_RTC_SHIFT, PIIX_SIDETIM, PIIX_SIDETIM_ISP_SHIFT, PIIX_SIDETIM_RTC_SHIFT, PIIX_UDMAREG, PIIX_UDMATIM_SHIFT, PIIX_CONFIG, PIIX_CONFIG_PINGPONG, ICH5_SATA_MAP, ICH5_SATA_MAP_MV_MASK, ICH5_SATA_MAP_COMBINED, ICH5_SATA_PI, ICH5_SATA_PI_PRI_NATIVE, ICH5_SATA_PI_SEC_NATIVE, ICH_SATA_PCS, ICH_SATA_PCS_P0E, ICH_SATA_PCS_P1E, ICH_SATA_PCS_P0P, ICH_SATA_PCS_P1P, ICH6_SATA_MAP_CMB_MASK, ICH6_SATA_MAP_CMB_PRI, ICH6_SATA_MAP_CMB_SEC, ICH7_SATA_MAP_SMS_MASK, ICH7_SATA_MAP_SMS_IDE, ICH7_SATA_MAP_SMS_AHCI, ICH7_SATA_MAP_SMS_RAID);
    }
}
/* </TESTS> */
