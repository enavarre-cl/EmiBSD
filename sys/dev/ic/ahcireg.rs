/* <LICENSES> */
/*
 * Copyright (c) 2006 David Gwynne <dlg@openbsd.org>
 * Copyright (c) 2010 Conformal Systems LLC <info@conformal.com>
 * Copyright (c) 2010 Jonathan Matthew <jonathan@d14n.org>
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
//! `<dev/ic/ahcireg.h>`: the AHCI (Serial ATA Advanced Host Controller Interface) registers,
//! global and per port, and the command list, received FIS and command table structures the
//! controller reads and writes in host memory.
//!
//! Upstream: sys/dev/ic/ahcireg.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - The function-like macros (`AHCI_REG_CAP_NP(_r)`, `AHCI_PORT_REGION(_p)`,
//!   `AHCI_PREG_CMD_CCS(_r)`, ...) are lower-case `const fn`s (`docs/C_TO_RUST.md`).
//! - Register offsets are `usize` (`bus_size_t`), register bits `u32`, the command header's
//!   `AHCI_CMD_LIST_FLAG_*` `u16` (the width of `flags`).
//! - The `%b` format strings (`AHCI_FMT_CAP`, `AHCI_PFMT_IS`, ...) are byte strings with the
//!   C's octal escapes written in hexadecimal.
//! - The structures are `__packed` with `__aligned(8)` or `__aligned(128)` in C; every member
//!   sits at its natural alignment, so they are `#[repr(C, align(N))]` (Rust does not combine
//!   `packed` with `align`) and their sizes and offsets are asserted at compile time.

/// `AHCI_REG_CAP`: HBA Capabilities.
pub const AHCI_REG_CAP: usize = 0x000;
/// `AHCI_REG_CAP_SXS`: External SATA.
pub const AHCI_REG_CAP_SXS: u32 = 1 << 5;
/// `AHCI_REG_CAP_EMS`: Enclosure Mgmt.
pub const AHCI_REG_CAP_EMS: u32 = 1 << 6;
/// `AHCI_REG_CAP_CCCS`: Cmd Coalescing.
pub const AHCI_REG_CAP_CCCS: u32 = 1 << 7;
/// `AHCI_REG_CAP_PSC`: Partial State Capable.
pub const AHCI_REG_CAP_PSC: u32 = 1 << 13;
/// `AHCI_REG_CAP_SSC`: Slumber State Capable.
pub const AHCI_REG_CAP_SSC: u32 = 1 << 14;
/// `AHCI_REG_CAP_PMD`: PIO Multiple DRQ Block.
pub const AHCI_REG_CAP_PMD: u32 = 1 << 15;
/// `AHCI_REG_CAP_FBSS`: FIS-Based Switching.
pub const AHCI_REG_CAP_FBSS: u32 = 1 << 16;
/// `AHCI_REG_CAP_SPM`: Port Multiplier.
pub const AHCI_REG_CAP_SPM: u32 = 1 << 17;
/// `AHCI_REG_CAP_SAM`: AHCI Only mode.
pub const AHCI_REG_CAP_SAM: u32 = 1 << 18;
/// `AHCI_REG_CAP_SNZO`: Non Zero DMA Offsets.
pub const AHCI_REG_CAP_SNZO: u32 = 1 << 19;
/// `AHCI_REG_CAP_ISS`: Interface Speed Support.
pub const AHCI_REG_CAP_ISS: u32 = 0xf << 20;
/// `AHCI_REG_CAP_ISS_G1`: Gen 1 (1.5 Gbps).
pub const AHCI_REG_CAP_ISS_G1: u32 = 0x1 << 20;
/// `AHCI_REG_CAP_ISS_G2`: Gen 2 (3 Gbps).
pub const AHCI_REG_CAP_ISS_G2: u32 = 0x2 << 20;
/// `AHCI_REG_CAP_ISS_G3`: Gen 3 (6 Gbps).
pub const AHCI_REG_CAP_ISS_G3: u32 = 0x3 << 20;
/// `AHCI_REG_CAP_SCLO`: Cmd List Override.
pub const AHCI_REG_CAP_SCLO: u32 = 1 << 24;
/// `AHCI_REG_CAP_SAL`: Activity LED.
pub const AHCI_REG_CAP_SAL: u32 = 1 << 25;
/// `AHCI_REG_CAP_SALP`: Aggressive Link Pwr Mgmt.
pub const AHCI_REG_CAP_SALP: u32 = 1 << 26;
/// `AHCI_REG_CAP_SSS`: Staggered Spinup.
pub const AHCI_REG_CAP_SSS: u32 = 1 << 27;
/// `AHCI_REG_CAP_SMPS`: Mech Presence Switch.
pub const AHCI_REG_CAP_SMPS: u32 = 1 << 28;
/// `AHCI_REG_CAP_SSNTF`: SNotification Register.
pub const AHCI_REG_CAP_SSNTF: u32 = 1 << 29;
/// `AHCI_REG_CAP_SNCQ`: Native Cmd Queuing.
pub const AHCI_REG_CAP_SNCQ: u32 = 1 << 30;
/// `AHCI_REG_CAP_S64A`: 64bit Addressing.
pub const AHCI_REG_CAP_S64A: u32 = 1 << 31;
/// `AHCI_FMT_CAP`: the `%b` format of the bits above.
pub const AHCI_FMT_CAP: &[u8] = b"\x10\x20S64A\x1fNCQ\x1eSSNTF\x1dSMPS\x1cSSS\x1bSALP\x1aSAL\x19SCLO\x14SNZO\x13SAM\x12SPM\x11FBSS\x10PMD\x0fSSC\x0ePSC\x08CCCS\x07EMS\x06SXS";
/// `AHCI_REG_GHC`: Global HBA Control.
pub const AHCI_REG_GHC: usize = 0x004;
/// `AHCI_REG_GHC_HR`: HBA Reset.
pub const AHCI_REG_GHC_HR: u32 = 1 << 0;
/// `AHCI_REG_GHC_IE`: Interrupt Enable.
pub const AHCI_REG_GHC_IE: u32 = 1 << 1;
/// `AHCI_REG_GHC_MRSM`: MSI Revert to Single Msg.
pub const AHCI_REG_GHC_MRSM: u32 = 1 << 2;
/// `AHCI_REG_GHC_AE`: AHCI Enable.
pub const AHCI_REG_GHC_AE: u32 = 1 << 31;
/// `AHCI_FMT_GHC`: the `%b` format of the bits above.
pub const AHCI_FMT_GHC: &[u8] = b"\x10\x20AE\x03MRSM\x02IE\x01HR";
/// `AHCI_REG_IS`: Interrupt Status.
pub const AHCI_REG_IS: usize = 0x008;
/// `AHCI_REG_PI`: Ports Implemented.
pub const AHCI_REG_PI: usize = 0x00c;
/// `AHCI_REG_VS`: AHCI Version.
pub const AHCI_REG_VS: usize = 0x010;
/// `AHCI_REG_VS_0_95`: 0.95.
pub const AHCI_REG_VS_0_95: u32 = 0x00000905;
/// `AHCI_REG_VS_1_0`: 1.0.
pub const AHCI_REG_VS_1_0: u32 = 0x00010000;
/// `AHCI_REG_VS_1_1`: 1.1.
pub const AHCI_REG_VS_1_1: u32 = 0x00010100;
/// `AHCI_REG_VS_1_2`: 1.2.
pub const AHCI_REG_VS_1_2: u32 = 0x00010200;
/// `AHCI_REG_VS_1_3`: 1.3.
pub const AHCI_REG_VS_1_3: u32 = 0x00010300;
/// `AHCI_REG_VS_1_3_1`: 1.3.1.
pub const AHCI_REG_VS_1_3_1: u32 = 0x00010301;
/// `AHCI_REG_CCC_CTL`: Coalescing Control.
pub const AHCI_REG_CCC_CTL: usize = 0x014;
/// `AHCI_REG_CCC_PORTS`: Coalescing Ports.
pub const AHCI_REG_CCC_PORTS: usize = 0x018;
/// `AHCI_REG_EM_LOC`: Enclosure Mgmt Location.
pub const AHCI_REG_EM_LOC: usize = 0x01c;
/// `AHCI_REG_EM_CTL`: Enclosure Mgmt Control.
pub const AHCI_REG_EM_CTL: usize = 0x020;
/// `AHCI_REG_CAP2`: HBA Capabilities Extended.
pub const AHCI_REG_CAP2: usize = 0x024;
/// `AHCI_REG_CAP2_DESO`: DevSlp from slumber only.
pub const AHCI_REG_CAP2_DESO: u32 = 1 << 5;
/// `AHCI_REG_CAP2_SADM`: Aggro DevSlp mgmt.
pub const AHCI_REG_CAP2_SADM: u32 = 1 << 4;
/// `AHCI_REG_CAP2_SDS`: Supports DevSlp.
pub const AHCI_REG_CAP2_SDS: u32 = 1 << 3;
/// `AHCI_REG_CAP2_APST`: Auto partial->slumber.
pub const AHCI_REG_CAP2_APST: u32 = 1 << 2;
/// `AHCI_REG_CAP2_NVMP`: NVMHCI present.
pub const AHCI_REG_CAP2_NVMP: u32 = 1 << 1;
/// `AHCI_REG_CAP2_BOH`: BIOS/OS handoff.
pub const AHCI_REG_CAP2_BOH: u32 = 1 << 0;
/// `AHCI_FMT_CAP2`: the `%b` format of the bits above.
pub const AHCI_FMT_CAP2: &[u8] = b"\x10\x06DESO\x05SADM\x04SDS\x03APST\x02NVMP\x01BOH";
/// `AHCI_PORT_SIZE`.
pub const AHCI_PORT_SIZE: usize = 0x80;
/// `AHCI_PREG_CLB`: Cmd List Base Addr.
pub const AHCI_PREG_CLB: usize = 0x00;
/// `AHCI_PREG_CLBU`: Cmd List Base Hi Addr.
pub const AHCI_PREG_CLBU: usize = 0x04;
/// `AHCI_PREG_FB`: FIS Base Addr.
pub const AHCI_PREG_FB: usize = 0x08;
/// `AHCI_PREG_FBU`: FIS Base Hi Addr.
pub const AHCI_PREG_FBU: usize = 0x0c;
/// `AHCI_PREG_IS`: Interrupt Status.
pub const AHCI_PREG_IS: usize = 0x10;
/// `AHCI_PREG_IS_DHRS`: Device to Host FIS.
pub const AHCI_PREG_IS_DHRS: u32 = 1 << 0;
/// `AHCI_PREG_IS_PSS`: PIO Setup FIS.
pub const AHCI_PREG_IS_PSS: u32 = 1 << 1;
/// `AHCI_PREG_IS_DSS`: DMA Setup FIS.
pub const AHCI_PREG_IS_DSS: u32 = 1 << 2;
/// `AHCI_PREG_IS_SDBS`: Set Device Bits FIS.
pub const AHCI_PREG_IS_SDBS: u32 = 1 << 3;
/// `AHCI_PREG_IS_UFS`: Unknown FIS.
pub const AHCI_PREG_IS_UFS: u32 = 1 << 4;
/// `AHCI_PREG_IS_DPS`: Descriptor Processed.
pub const AHCI_PREG_IS_DPS: u32 = 1 << 5;
/// `AHCI_PREG_IS_PCS`: Port Change.
pub const AHCI_PREG_IS_PCS: u32 = 1 << 6;
/// `AHCI_PREG_IS_DMPS`: Device Mechanical Presence.
pub const AHCI_PREG_IS_DMPS: u32 = 1 << 7;
/// `AHCI_PREG_IS_PRCS`: PhyRdy Change.
pub const AHCI_PREG_IS_PRCS: u32 = 1 << 22;
/// `AHCI_PREG_IS_IPMS`: Incorrect Port Multiplier.
pub const AHCI_PREG_IS_IPMS: u32 = 1 << 23;
/// `AHCI_PREG_IS_OFS`: Overflow.
pub const AHCI_PREG_IS_OFS: u32 = 1 << 24;
/// `AHCI_PREG_IS_INFS`: Interface Non-fatal Error.
pub const AHCI_PREG_IS_INFS: u32 = 1 << 26;
/// `AHCI_PREG_IS_IFS`: Interface Fatal Error.
pub const AHCI_PREG_IS_IFS: u32 = 1 << 27;
/// `AHCI_PREG_IS_HBDS`: Host Bus Data Error.
pub const AHCI_PREG_IS_HBDS: u32 = 1 << 28;
/// `AHCI_PREG_IS_HBFS`: Host Bus Fatal Error.
pub const AHCI_PREG_IS_HBFS: u32 = 1 << 29;
/// `AHCI_PREG_IS_TFES`: Task File Error.
pub const AHCI_PREG_IS_TFES: u32 = 1 << 30;
/// `AHCI_PREG_IS_CPDS`: Cold Presence Detect.
pub const AHCI_PREG_IS_CPDS: u32 = 1 << 31;
/// `AHCI_PFMT_IS`: the `%b` format of the bits above.
pub const AHCI_PFMT_IS: &[u8] = b"\x10\x20CPDS\x1fTFES\x1eHBFS\x1dHBDS\x1cIFS\x1bINFS\x19OFS\x18IPMS\x17PRCS\x08DMPS\x06DPS\x07PCS\x05UFS\x04SDBS\x03DSS\x02PSS\x01DHRS";
/// `AHCI_PREG_IE`: Interrupt Enable.
pub const AHCI_PREG_IE: usize = 0x14;
/// `AHCI_PREG_IE_DHRE`: Device to Host FIS.
pub const AHCI_PREG_IE_DHRE: u32 = 1 << 0;
/// `AHCI_PREG_IE_PSE`: PIO Setup FIS.
pub const AHCI_PREG_IE_PSE: u32 = 1 << 1;
/// `AHCI_PREG_IE_DSE`: DMA Setup FIS.
pub const AHCI_PREG_IE_DSE: u32 = 1 << 2;
/// `AHCI_PREG_IE_SDBE`: Set Device Bits FIS.
pub const AHCI_PREG_IE_SDBE: u32 = 1 << 3;
/// `AHCI_PREG_IE_UFE`: Unknown FIS.
pub const AHCI_PREG_IE_UFE: u32 = 1 << 4;
/// `AHCI_PREG_IE_DPE`: Descriptor Processed.
pub const AHCI_PREG_IE_DPE: u32 = 1 << 5;
/// `AHCI_PREG_IE_PCE`: Port Change.
pub const AHCI_PREG_IE_PCE: u32 = 1 << 6;
/// `AHCI_PREG_IE_DMPE`: Device Mechanical Presence.
pub const AHCI_PREG_IE_DMPE: u32 = 1 << 7;
/// `AHCI_PREG_IE_PRCE`: PhyRdy Change.
pub const AHCI_PREG_IE_PRCE: u32 = 1 << 22;
/// `AHCI_PREG_IE_IPME`: Incorrect Port Multiplier.
pub const AHCI_PREG_IE_IPME: u32 = 1 << 23;
/// `AHCI_PREG_IE_OFE`: Overflow.
pub const AHCI_PREG_IE_OFE: u32 = 1 << 24;
/// `AHCI_PREG_IE_INFE`: Interface Non-fatal Error.
pub const AHCI_PREG_IE_INFE: u32 = 1 << 26;
/// `AHCI_PREG_IE_IFE`: Interface Fatal Error.
pub const AHCI_PREG_IE_IFE: u32 = 1 << 27;
/// `AHCI_PREG_IE_HBDE`: Host Bus Data Error.
pub const AHCI_PREG_IE_HBDE: u32 = 1 << 28;
/// `AHCI_PREG_IE_HBFE`: Host Bus Fatal Error.
pub const AHCI_PREG_IE_HBFE: u32 = 1 << 29;
/// `AHCI_PREG_IE_TFEE`: Task File Error.
pub const AHCI_PREG_IE_TFEE: u32 = 1 << 30;
/// `AHCI_PREG_IE_CPDE`: Cold Presence Detect.
pub const AHCI_PREG_IE_CPDE: u32 = 1 << 31;
/// `AHCI_PFMT_IE`: the `%b` format of the bits above.
pub const AHCI_PFMT_IE: &[u8] = b"\x10\x20CPDE\x1fTFEE\x1eHBFE\x1dHBDE\x1cIFE\x1bINFE\x19OFE\x18IPME\x17PRCE\x08DMPE\x07PCE\x06DPE\x05UFE\x04SDBE\x03DSE\x02PSE\x01DHRE";
/// `AHCI_PREG_CMD`: Command and Status.
pub const AHCI_PREG_CMD: usize = 0x18;
/// `AHCI_PREG_CMD_ST`: Start.
pub const AHCI_PREG_CMD_ST: u32 = 1 << 0;
/// `AHCI_PREG_CMD_SUD`: Spin Up Device.
pub const AHCI_PREG_CMD_SUD: u32 = 1 << 1;
/// `AHCI_PREG_CMD_POD`: Power On Device.
pub const AHCI_PREG_CMD_POD: u32 = 1 << 2;
/// `AHCI_PREG_CMD_CLO`: Command List Override.
pub const AHCI_PREG_CMD_CLO: u32 = 1 << 3;
/// `AHCI_PREG_CMD_FRE`: FIS Receive Enable.
pub const AHCI_PREG_CMD_FRE: u32 = 1 << 4;
/// `AHCI_PREG_CMD_MPSS`: Mech Presence State.
pub const AHCI_PREG_CMD_MPSS: u32 = 1 << 13;
/// `AHCI_PREG_CMD_FR`: FIS Receive Running.
pub const AHCI_PREG_CMD_FR: u32 = 1 << 14;
/// `AHCI_PREG_CMD_CR`: Command List Running.
pub const AHCI_PREG_CMD_CR: u32 = 1 << 15;
/// `AHCI_PREG_CMD_CPS`: Cold Presence State.
pub const AHCI_PREG_CMD_CPS: u32 = 1 << 16;
/// `AHCI_PREG_CMD_PMA`: Port Multiplier Attached.
pub const AHCI_PREG_CMD_PMA: u32 = 1 << 17;
/// `AHCI_PREG_CMD_HPCP`: Hot Plug Capable.
pub const AHCI_PREG_CMD_HPCP: u32 = 1 << 18;
/// `AHCI_PREG_CMD_MPSP`: Mech Presence Switch.
pub const AHCI_PREG_CMD_MPSP: u32 = 1 << 19;
/// `AHCI_PREG_CMD_CPD`: Cold Presence Detection.
pub const AHCI_PREG_CMD_CPD: u32 = 1 << 20;
/// `AHCI_PREG_CMD_ESP`: External SATA Port.
pub const AHCI_PREG_CMD_ESP: u32 = 1 << 21;
/// `AHCI_PREG_CMD_ATAPI`: Device is ATAPI.
pub const AHCI_PREG_CMD_ATAPI: u32 = 1 << 24;
/// `AHCI_PREG_CMD_DLAE`: Drv LED on ATAPI Enable.
pub const AHCI_PREG_CMD_DLAE: u32 = 1 << 25;
/// `AHCI_PREG_CMD_ALPE`: Aggro Pwr Mgmt Enable.
pub const AHCI_PREG_CMD_ALPE: u32 = 1 << 26;
/// `AHCI_PREG_CMD_ASP`: Aggro Slumber/Partial.
pub const AHCI_PREG_CMD_ASP: u32 = 1 << 27;
/// `AHCI_PREG_CMD_ICC`: Interface Comm Ctrl.
pub const AHCI_PREG_CMD_ICC: u32 = 0xf0000000;
/// `AHCI_PREG_CMD_ICC_SLUMBER`.
pub const AHCI_PREG_CMD_ICC_SLUMBER: u32 = 0x60000000;
/// `AHCI_PREG_CMD_ICC_PARTIAL`.
pub const AHCI_PREG_CMD_ICC_PARTIAL: u32 = 0x20000000;
/// `AHCI_PREG_CMD_ICC_ACTIVE`.
pub const AHCI_PREG_CMD_ICC_ACTIVE: u32 = 0x10000000;
/// `AHCI_PREG_CMD_ICC_IDLE`.
pub const AHCI_PREG_CMD_ICC_IDLE: u32 = 0x00000000;
/// `AHCI_PFMT_CMD`: the `%b` format of the bits above.
pub const AHCI_PFMT_CMD: &[u8] = b"\x10\x1cASP\x1bALPE\x1aDLAE\x19ATAPI\x16ESP\x15CPD\x14MPSP\x13HPCP\x12PMA\x11CPS\x10CR\x0fFR\x0eMPSS\x05FRE\x04CLO\x03POD\x02SUD\x01ST";
/// `AHCI_PREG_TFD`: Task File Data.
pub const AHCI_PREG_TFD: usize = 0x20;
/// `AHCI_PREG_TFD_STS`.
pub const AHCI_PREG_TFD_STS: u32 = 0xff;
/// `AHCI_PREG_TFD_STS_ERR`.
pub const AHCI_PREG_TFD_STS_ERR: u32 = 1 << 0;
/// `AHCI_PREG_TFD_STS_DRQ`.
pub const AHCI_PREG_TFD_STS_DRQ: u32 = 1 << 3;
/// `AHCI_PREG_TFD_STS_BSY`.
pub const AHCI_PREG_TFD_STS_BSY: u32 = 1 << 7;
/// `AHCI_PREG_TFD_ERR`.
pub const AHCI_PREG_TFD_ERR: u32 = 0xff00;
/// `AHCI_PFMT_TFD_STS`: the `%b` format of the bits above.
pub const AHCI_PFMT_TFD_STS: &[u8] = b"\x10\x08BSY\x04DRQ\x01ERR";
/// `AHCI_PREG_SIG`: Signature.
pub const AHCI_PREG_SIG: usize = 0x24;
/// `AHCI_PREG_SSTS`: SATA Status.
pub const AHCI_PREG_SSTS: usize = 0x28;
/// `AHCI_PREG_SSTS_DET`: Device Detection.
pub const AHCI_PREG_SSTS_DET: u32 = 0xf;
/// `AHCI_PREG_SSTS_DET_NONE`.
pub const AHCI_PREG_SSTS_DET_NONE: u32 = 0x0;
/// `AHCI_PREG_SSTS_DET_DEV_NE`.
pub const AHCI_PREG_SSTS_DET_DEV_NE: u32 = 0x1;
/// `AHCI_PREG_SSTS_DET_DEV`.
pub const AHCI_PREG_SSTS_DET_DEV: u32 = 0x3;
/// `AHCI_PREG_SSTS_DET_PHYOFFLINE`.
pub const AHCI_PREG_SSTS_DET_PHYOFFLINE: u32 = 0x4;
/// `AHCI_PREG_SSTS_SPD`: Current Interface Speed.
pub const AHCI_PREG_SSTS_SPD: u32 = 0xf0;
/// `AHCI_PREG_SSTS_SPD_NONE`.
pub const AHCI_PREG_SSTS_SPD_NONE: u32 = 0x00;
/// `AHCI_PREG_SSTS_SPD_GEN1`.
pub const AHCI_PREG_SSTS_SPD_GEN1: u32 = 0x10;
/// `AHCI_PREG_SSTS_SPD_GEN2`.
pub const AHCI_PREG_SSTS_SPD_GEN2: u32 = 0x20;
/// `AHCI_PREG_SSTS_SPD_GEN3`.
pub const AHCI_PREG_SSTS_SPD_GEN3: u32 = 0x30;
/// `AHCI_PREG_SSTS_IPM`: Interface Power Management.
pub const AHCI_PREG_SSTS_IPM: u32 = 0xf00;
/// `AHCI_PREG_SSTS_IPM_NONE`.
pub const AHCI_PREG_SSTS_IPM_NONE: u32 = 0x000;
/// `AHCI_PREG_SSTS_IPM_ACTIVE`.
pub const AHCI_PREG_SSTS_IPM_ACTIVE: u32 = 0x100;
/// `AHCI_PREG_SSTS_IPM_PARTIAL`.
pub const AHCI_PREG_SSTS_IPM_PARTIAL: u32 = 0x200;
/// `AHCI_PREG_SSTS_IPM_SLUMBER`.
pub const AHCI_PREG_SSTS_IPM_SLUMBER: u32 = 0x600;
/// `AHCI_PREG_SCTL`: SATA Control.
pub const AHCI_PREG_SCTL: usize = 0x2c;
/// `AHCI_PREG_SCTL_DET`: Device Detection.
pub const AHCI_PREG_SCTL_DET: u32 = 0xf;
/// `AHCI_PREG_SCTL_DET_NONE`.
pub const AHCI_PREG_SCTL_DET_NONE: u32 = 0x0;
/// `AHCI_PREG_SCTL_DET_INIT`.
pub const AHCI_PREG_SCTL_DET_INIT: u32 = 0x1;
/// `AHCI_PREG_SCTL_DET_DISABLE`.
pub const AHCI_PREG_SCTL_DET_DISABLE: u32 = 0x4;
/// `AHCI_PREG_SCTL_SPD`: Speed Allowed.
pub const AHCI_PREG_SCTL_SPD: u32 = 0xf0;
/// `AHCI_PREG_SCTL_SPD_ANY`.
pub const AHCI_PREG_SCTL_SPD_ANY: u32 = 0x00;
/// `AHCI_PREG_SCTL_SPD_GEN1`.
pub const AHCI_PREG_SCTL_SPD_GEN1: u32 = 0x10;
/// `AHCI_PREG_SCTL_SPD_GEN2`.
pub const AHCI_PREG_SCTL_SPD_GEN2: u32 = 0x20;
/// `AHCI_PREG_SCTL_SPD_GEN3`.
pub const AHCI_PREG_SCTL_SPD_GEN3: u32 = 0x30;
/// `AHCI_PREG_SCTL_IPM`: Interface Power Management.
pub const AHCI_PREG_SCTL_IPM: u32 = 0xf00;
/// `AHCI_PREG_SCTL_IPM_NONE`.
pub const AHCI_PREG_SCTL_IPM_NONE: u32 = 0x000;
/// `AHCI_PREG_SCTL_IPM_NOPARTIAL`.
pub const AHCI_PREG_SCTL_IPM_NOPARTIAL: u32 = 0x100;
/// `AHCI_PREG_SCTL_IPM_NOSLUMBER`.
pub const AHCI_PREG_SCTL_IPM_NOSLUMBER: u32 = 0x200;
/// `AHCI_PREG_SCTL_IPM_DISABLED`.
pub const AHCI_PREG_SCTL_IPM_DISABLED: u32 = 0x300;
/// `AHCI_PREG_SERR`: SATA Error.
pub const AHCI_PREG_SERR: usize = 0x30;
/// `AHCI_PREG_SERR_ERR_I`: Recovered Data Integrity.
pub const AHCI_PREG_SERR_ERR_I: u32 = 1 << 0;
/// `AHCI_PREG_SERR_ERR_M`: Recovered Communications.
pub const AHCI_PREG_SERR_ERR_M: u32 = 1 << 1;
/// `AHCI_PREG_SERR_ERR_T`: Transient Data Integrity.
pub const AHCI_PREG_SERR_ERR_T: u32 = 1 << 8;
/// `AHCI_PREG_SERR_ERR_C`: Persistent Comm/Data.
pub const AHCI_PREG_SERR_ERR_C: u32 = 1 << 9;
/// `AHCI_PREG_SERR_ERR_P`: Protocol.
pub const AHCI_PREG_SERR_ERR_P: u32 = 1 << 10;
/// `AHCI_PREG_SERR_ERR_E`: Internal.
pub const AHCI_PREG_SERR_ERR_E: u32 = 1 << 11;
/// `AHCI_PFMT_SERR_ERR`: the `%b` format of the bits above.
pub const AHCI_PFMT_SERR_ERR: &[u8] = b"\x10\x0cE\x0bP\x0aC\x09T\x02M\x01I";
/// `AHCI_PREG_SERR_DIAG_N`: PhyRdy Change.
pub const AHCI_PREG_SERR_DIAG_N: u32 = 1 << 0;
/// `AHCI_PREG_SERR_DIAG_I`: Phy Internal Error.
pub const AHCI_PREG_SERR_DIAG_I: u32 = 1 << 1;
/// `AHCI_PREG_SERR_DIAG_W`: Comm Wake.
pub const AHCI_PREG_SERR_DIAG_W: u32 = 1 << 2;
/// `AHCI_PREG_SERR_DIAG_B`: 10B to 8B Decode Error.
pub const AHCI_PREG_SERR_DIAG_B: u32 = 1 << 3;
/// `AHCI_PREG_SERR_DIAG_D`: Disparity Error.
pub const AHCI_PREG_SERR_DIAG_D: u32 = 1 << 4;
/// `AHCI_PREG_SERR_DIAG_C`: CRC Error.
pub const AHCI_PREG_SERR_DIAG_C: u32 = 1 << 5;
/// `AHCI_PREG_SERR_DIAG_H`: Handshake Error.
pub const AHCI_PREG_SERR_DIAG_H: u32 = 1 << 6;
/// `AHCI_PREG_SERR_DIAG_S`: Link Sequence Error.
pub const AHCI_PREG_SERR_DIAG_S: u32 = 1 << 7;
/// `AHCI_PREG_SERR_DIAG_T`: Transport State Trans Err.
pub const AHCI_PREG_SERR_DIAG_T: u32 = 1 << 8;
/// `AHCI_PREG_SERR_DIAG_F`: Unknown FIS Type.
pub const AHCI_PREG_SERR_DIAG_F: u32 = 1 << 9;
/// `AHCI_PREG_SERR_DIAG_X`: Exchanged.
pub const AHCI_PREG_SERR_DIAG_X: u32 = 1 << 10;
/// `AHCI_PFMT_SERR_DIAG`: the `%b` format of the bits above.
pub const AHCI_PFMT_SERR_DIAG: &[u8] =
    b"\x10\x0bX\x0aF\x09T\x08S\x07H\x06C\x05D\x04B\x03W\x02I\x01N";
/// `AHCI_PREG_SACT`: SATA Active.
pub const AHCI_PREG_SACT: usize = 0x34;
/// `AHCI_PREG_CI`: Command Issue.
pub const AHCI_PREG_CI: usize = 0x38;
/// `AHCI_PREG_CI_ALL_SLOTS`.
pub const AHCI_PREG_CI_ALL_SLOTS: u32 = 0xffffffff;
/// `AHCI_PREG_SNTF`: SNotification.
pub const AHCI_PREG_SNTF: usize = 0x3c;
/// `AHCI_PREG_FBS`: FIS-based Switching Control.
pub const AHCI_PREG_FBS: usize = 0x40;
/// `AHCI_PREG_FBS_DWE`: Device With Error.
pub const AHCI_PREG_FBS_DWE: u32 = 0xf0000;
/// `AHCI_PREG_FBS_ADO`: Active Device Optimization.
pub const AHCI_PREG_FBS_ADO: u32 = 0xf000;
/// `AHCI_PREG_FBS_DEV`: Device To Issue.
pub const AHCI_PREG_FBS_DEV: u32 = 0xf00;
/// `AHCI_PREG_FBS_SDE`: Single Device Error.
pub const AHCI_PREG_FBS_SDE: u32 = 1 << 2;
/// `AHCI_PREG_FBS_DEC`: Device Error Clear.
pub const AHCI_PREG_FBS_DEC: u32 = 1 << 1;
/// `AHCI_PREG_FBS_EN`: Enable.
pub const AHCI_PREG_FBS_EN: u32 = 1 << 0;
/// `AHCI_CMD_LIST_FLAG_CFL`: Command FIS Length.
pub const AHCI_CMD_LIST_FLAG_CFL: u16 = 0x001f;
/// `AHCI_CMD_LIST_FLAG_A`: ATAPI.
pub const AHCI_CMD_LIST_FLAG_A: u16 = 1 << 5;
/// `AHCI_CMD_LIST_FLAG_W`: Write.
pub const AHCI_CMD_LIST_FLAG_W: u16 = 1 << 6;
/// `AHCI_CMD_LIST_FLAG_P`: Prefetchable.
pub const AHCI_CMD_LIST_FLAG_P: u16 = 1 << 7;
/// `AHCI_CMD_LIST_FLAG_R`: Reset.
pub const AHCI_CMD_LIST_FLAG_R: u16 = 1 << 8;
/// `AHCI_CMD_LIST_FLAG_B`: BIST.
pub const AHCI_CMD_LIST_FLAG_B: u16 = 1 << 9;
/// `AHCI_CMD_LIST_FLAG_C`: Clear Busy upon R_OK.
pub const AHCI_CMD_LIST_FLAG_C: u16 = 1 << 10;
/// `AHCI_CMD_LIST_FLAG_PMP`: Port Multiplier Port.
pub const AHCI_CMD_LIST_FLAG_PMP: u16 = 0xf000;
/// `AHCI_CMD_LIST_FLAG_PMP_SHIFT`.
pub const AHCI_CMD_LIST_FLAG_PMP_SHIFT: u16 = 12;

/// `AHCI_REG_CAP_NP(_r)`: Number of Ports.
pub const fn ahci_reg_cap_np(r: u32) -> u32 {
    (r & 0x1f) + 1
}

/// `AHCI_REG_CAP_NCS(_r)`: Number of Command Slots.
pub const fn ahci_reg_cap_ncs(r: u32) -> u32 {
    ((r & 0x1f00) >> 8) + 1
}

/// `AHCI_REG_CCC_CTL_INT(_r)`: the coalescing interrupt's slot.
pub const fn ahci_reg_ccc_ctl_int(r: u32) -> u32 {
    (r & 0xf8) >> 3
}

/// `AHCI_PORT_REGION(_p)`: the offset of port `p`'s registers.
pub const fn ahci_port_region(p: usize) -> usize {
    0x100 + (p * 0x80)
}

/// `AHCI_PREG_CMD_CCS(_r)`: Current Command Slot.
pub const fn ahci_preg_cmd_ccs(r: u32) -> u32 {
    (r >> 8) & 0x1f
}

/// `AHCI_PREG_SERR_ERR(_r)`: the error half of SERR.
pub const fn ahci_preg_serr_err(r: u32) -> u32 {
    r & 0xffff
}

/// `AHCI_PREG_SERR_DIAG(_r)`: the diagnostics half of SERR.
pub const fn ahci_preg_serr_diag(r: u32) -> u32 {
    (r >> 16) & 0xffff
}

/// `struct ahci_cmd_hdr`: one entry of a port's command list.
#[repr(C, align(8))]
#[derive(Clone, Copy, Debug, Default)]
pub struct AhciCmdHdr {
    /// `flags`: `AHCI_CMD_LIST_FLAG_*`, little-endian.
    pub flags: u16,
    /// `prdtl`: sgl len.
    pub prdtl: u16,
    /// `prdbc`: transferred byte count.
    pub prdbc: u32,
    /// `ctba`: the command table's address.
    pub ctba: u64,
    /// `reserved`.
    pub reserved: [u32; 4],
}

/// `struct ahci_rfis`: a port's received FIS area.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct AhciRfis {
    /// `dsfis`: DMA Setup FIS.
    pub dsfis: [u8; 28],
    /// `reserved1`.
    pub reserved1: [u8; 4],
    /// `psfis`: PIO Setup FIS.
    pub psfis: [u8; 24],
    /// `reserved2`.
    pub reserved2: [u8; 8],
    /// `rfis`: D2H Register FIS.
    pub rfis: [u8; 24],
    /// `reserved3`.
    pub reserved3: [u8; 4],
    /// `sdbfis`: Set Device Bits FIS.
    pub sdbfis: [u8; 4],
    /// `ufis`: Unknown FIS.
    pub ufis: [u8; 64],
    /// `reserved4`.
    pub reserved4: [u8; 96],
}

/// `struct ahci_prdt`: one entry of a command's physical region descriptor table.
#[repr(C, align(8))]
#[derive(Clone, Copy, Debug, Default)]
pub struct AhciPrdt {
    /// `dba`: data base address.
    pub dba: u64,
    /// `reserved`.
    pub reserved: u32,
    /// `flags`: the byte count less one, and `AHCI_PRDT_FLAG_INTR`.
    pub flags: u32,
}

/// `AHCI_PRDT_FLAG_INTR`: interrupt on completion.
pub const AHCI_PRDT_FLAG_INTR: u32 = 1 << 31;

/// `AHCI_MAX_PRDT`: this makes `ahci_cmd_table` 512 bytes, supporting 128-byte alignment.
pub const AHCI_MAX_PRDT: usize = 24;

/// `struct ahci_cmd_table`: a command's FIS, ATAPI command and PRDT.
#[repr(C, align(128))]
#[derive(Clone, Copy, Debug)]
pub struct AhciCmdTable {
    /// `cfis`: Command FIS.
    pub cfis: [u8; 64],
    /// `acmd`: ATAPI Command.
    pub acmd: [u8; 16],
    /// `reserved`.
    pub reserved: [u8; 48],
    /// `prdt`.
    pub prdt: [AhciPrdt; AHCI_MAX_PRDT],
}

/// `AHCI_MAX_PORTS`.
pub const AHCI_MAX_PORTS: usize = 32;

const _: () = {
    assert!(size_of::<AhciCmdHdr>() == 32);
    assert!(core::mem::offset_of!(AhciCmdHdr, prdbc) == 4);
    assert!(core::mem::offset_of!(AhciCmdHdr, ctba) == 8);
    assert!(size_of::<AhciRfis>() == 256);
    assert!(core::mem::offset_of!(AhciRfis, rfis) == 0x40);
    assert!(core::mem::offset_of!(AhciRfis, ufis) == 0x60);
    assert!(size_of::<AhciPrdt>() == 16);
    assert!(core::mem::offset_of!(AhciPrdt, flags) == 12);
    assert!(size_of::<AhciCmdTable>() == 512);
    assert!(core::mem::offset_of!(AhciCmdTable, acmd) == 0x40);
    assert!(core::mem::offset_of!(AhciCmdTable, prdt) == 0x80);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_fields() {
        // CAP of QEMU's ich9-ahci: 6 ports, 32 command slots, NCQ, SSS, 64-bit, ISS gen 1.
        let cap: u32 = 0xc030_ff05;
        assert_eq!(ahci_reg_cap_np(cap), 6);
        assert_eq!(ahci_reg_cap_ncs(cap), 32);
        assert_ne!(cap & AHCI_REG_CAP_SNCQ, 0);
        assert_ne!(cap & AHCI_REG_CAP_S64A, 0);
        assert_eq!(ahci_reg_ccc_ctl_int(0x0000_0028), 5);
        assert_eq!(ahci_port_region(0), 0x100);
        assert_eq!(ahci_port_region(5), 0x380);
        assert_eq!(ahci_preg_cmd_ccs(0x0000_1f00), 31);
        assert_eq!(ahci_preg_serr_err(0x0401_0002), 0x0002);
        assert_eq!(ahci_preg_serr_diag(0x0401_0002), 0x0401);
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/ic/ahcireg.h");
        let mut names = crate::reftest::assert_defines!(defs;
            AHCI_REG_CAP, AHCI_REG_CAP_SXS, AHCI_REG_CAP_EMS, AHCI_REG_CAP_CCCS, AHCI_REG_CAP_PSC,
            AHCI_REG_CAP_SSC, AHCI_REG_CAP_PMD, AHCI_REG_CAP_FBSS, AHCI_REG_CAP_SPM,
            AHCI_REG_CAP_SAM, AHCI_REG_CAP_SNZO, AHCI_REG_CAP_ISS, AHCI_REG_CAP_ISS_G1,
            AHCI_REG_CAP_ISS_G2, AHCI_REG_CAP_ISS_G3, AHCI_REG_CAP_SCLO, AHCI_REG_CAP_SAL,
            AHCI_REG_CAP_SALP, AHCI_REG_CAP_SSS, AHCI_REG_CAP_SMPS, AHCI_REG_CAP_SSNTF,
            AHCI_REG_CAP_SNCQ, AHCI_REG_CAP_S64A, AHCI_REG_GHC, AHCI_REG_GHC_HR, AHCI_REG_GHC_IE,
            AHCI_REG_GHC_MRSM, AHCI_REG_GHC_AE, AHCI_REG_IS, AHCI_REG_PI, AHCI_REG_VS,
            AHCI_REG_VS_0_95, AHCI_REG_VS_1_0, AHCI_REG_VS_1_1, AHCI_REG_VS_1_2, AHCI_REG_VS_1_3,
            AHCI_REG_VS_1_3_1, AHCI_REG_CCC_CTL, AHCI_REG_CCC_PORTS, AHCI_REG_EM_LOC,
            AHCI_REG_EM_CTL, AHCI_REG_CAP2, AHCI_REG_CAP2_DESO, AHCI_REG_CAP2_SADM,
            AHCI_REG_CAP2_SDS, AHCI_REG_CAP2_APST, AHCI_REG_CAP2_NVMP, AHCI_REG_CAP2_BOH,
            AHCI_PORT_SIZE, AHCI_PREG_CLB, AHCI_PREG_CLBU, AHCI_PREG_FB, AHCI_PREG_FBU,
            AHCI_PREG_IS, AHCI_PREG_IS_DHRS, AHCI_PREG_IS_PSS, AHCI_PREG_IS_DSS, AHCI_PREG_IS_SDBS,
            AHCI_PREG_IS_UFS, AHCI_PREG_IS_DPS, AHCI_PREG_IS_PCS, AHCI_PREG_IS_DMPS,
            AHCI_PREG_IS_PRCS, AHCI_PREG_IS_IPMS, AHCI_PREG_IS_OFS, AHCI_PREG_IS_INFS,
            AHCI_PREG_IS_IFS, AHCI_PREG_IS_HBDS, AHCI_PREG_IS_HBFS, AHCI_PREG_IS_TFES,
            AHCI_PREG_IS_CPDS, AHCI_PREG_IE, AHCI_PREG_IE_DHRE, AHCI_PREG_IE_PSE, AHCI_PREG_IE_DSE,
            AHCI_PREG_IE_SDBE, AHCI_PREG_IE_UFE, AHCI_PREG_IE_DPE, AHCI_PREG_IE_PCE,
            AHCI_PREG_IE_DMPE, AHCI_PREG_IE_PRCE, AHCI_PREG_IE_IPME, AHCI_PREG_IE_OFE,
            AHCI_PREG_IE_INFE, AHCI_PREG_IE_IFE, AHCI_PREG_IE_HBDE, AHCI_PREG_IE_HBFE,
            AHCI_PREG_IE_TFEE, AHCI_PREG_IE_CPDE, AHCI_PREG_CMD, AHCI_PREG_CMD_ST,
            AHCI_PREG_CMD_SUD, AHCI_PREG_CMD_POD, AHCI_PREG_CMD_CLO, AHCI_PREG_CMD_FRE,
            AHCI_PREG_CMD_MPSS, AHCI_PREG_CMD_FR, AHCI_PREG_CMD_CR, AHCI_PREG_CMD_CPS,
            AHCI_PREG_CMD_PMA, AHCI_PREG_CMD_HPCP, AHCI_PREG_CMD_MPSP, AHCI_PREG_CMD_CPD,
            AHCI_PREG_CMD_ESP, AHCI_PREG_CMD_ATAPI, AHCI_PREG_CMD_DLAE, AHCI_PREG_CMD_ALPE,
            AHCI_PREG_CMD_ASP, AHCI_PREG_CMD_ICC, AHCI_PREG_CMD_ICC_SLUMBER,
            AHCI_PREG_CMD_ICC_PARTIAL, AHCI_PREG_CMD_ICC_ACTIVE, AHCI_PREG_CMD_ICC_IDLE,
            AHCI_PREG_TFD, AHCI_PREG_TFD_STS, AHCI_PREG_TFD_STS_ERR, AHCI_PREG_TFD_STS_DRQ,
            AHCI_PREG_TFD_STS_BSY, AHCI_PREG_TFD_ERR, AHCI_PREG_SIG, AHCI_PREG_SSTS,
            AHCI_PREG_SSTS_DET, AHCI_PREG_SSTS_DET_NONE, AHCI_PREG_SSTS_DET_DEV_NE,
            AHCI_PREG_SSTS_DET_DEV, AHCI_PREG_SSTS_DET_PHYOFFLINE, AHCI_PREG_SSTS_SPD,
            AHCI_PREG_SSTS_SPD_NONE, AHCI_PREG_SSTS_SPD_GEN1, AHCI_PREG_SSTS_SPD_GEN2,
            AHCI_PREG_SSTS_SPD_GEN3, AHCI_PREG_SSTS_IPM, AHCI_PREG_SSTS_IPM_NONE,
            AHCI_PREG_SSTS_IPM_ACTIVE, AHCI_PREG_SSTS_IPM_PARTIAL, AHCI_PREG_SSTS_IPM_SLUMBER,
            AHCI_PREG_SCTL, AHCI_PREG_SCTL_DET, AHCI_PREG_SCTL_DET_NONE, AHCI_PREG_SCTL_DET_INIT,
            AHCI_PREG_SCTL_DET_DISABLE, AHCI_PREG_SCTL_SPD, AHCI_PREG_SCTL_SPD_ANY,
            AHCI_PREG_SCTL_SPD_GEN1, AHCI_PREG_SCTL_SPD_GEN2, AHCI_PREG_SCTL_SPD_GEN3,
            AHCI_PREG_SCTL_IPM, AHCI_PREG_SCTL_IPM_NONE, AHCI_PREG_SCTL_IPM_NOPARTIAL,
            AHCI_PREG_SCTL_IPM_NOSLUMBER, AHCI_PREG_SCTL_IPM_DISABLED, AHCI_PREG_SERR,
            AHCI_PREG_SERR_ERR_I, AHCI_PREG_SERR_ERR_M, AHCI_PREG_SERR_ERR_T, AHCI_PREG_SERR_ERR_C,
            AHCI_PREG_SERR_ERR_P, AHCI_PREG_SERR_ERR_E, AHCI_PREG_SERR_DIAG_N,
            AHCI_PREG_SERR_DIAG_I, AHCI_PREG_SERR_DIAG_W, AHCI_PREG_SERR_DIAG_B,
            AHCI_PREG_SERR_DIAG_D, AHCI_PREG_SERR_DIAG_C, AHCI_PREG_SERR_DIAG_H,
            AHCI_PREG_SERR_DIAG_S, AHCI_PREG_SERR_DIAG_T, AHCI_PREG_SERR_DIAG_F,
            AHCI_PREG_SERR_DIAG_X, AHCI_PREG_SACT, AHCI_PREG_CI, AHCI_PREG_CI_ALL_SLOTS,
            AHCI_PREG_SNTF, AHCI_PREG_FBS, AHCI_PREG_FBS_DWE, AHCI_PREG_FBS_ADO, AHCI_PREG_FBS_DEV,
            AHCI_PREG_FBS_SDE, AHCI_PREG_FBS_DEC, AHCI_PREG_FBS_EN, AHCI_CMD_LIST_FLAG_CFL,
            AHCI_CMD_LIST_FLAG_A, AHCI_CMD_LIST_FLAG_W, AHCI_CMD_LIST_FLAG_P, AHCI_CMD_LIST_FLAG_R,
            AHCI_CMD_LIST_FLAG_B, AHCI_CMD_LIST_FLAG_C, AHCI_CMD_LIST_FLAG_PMP,
            AHCI_CMD_LIST_FLAG_PMP_SHIFT, AHCI_PRDT_FLAG_INTR, AHCI_MAX_PRDT, AHCI_MAX_PORTS,
        );
        // The `%b` strings are not simple defines.
        names.extend([
            "AHCI_FMT_CAP",
            "AHCI_FMT_GHC",
            "AHCI_FMT_CAP2",
            "AHCI_PFMT_IS",
            "AHCI_PFMT_IE",
            "AHCI_PFMT_CMD",
            "AHCI_PFMT_TFD_STS",
            "AHCI_PFMT_SERR_ERR",
            "AHCI_PFMT_SERR_DIAG",
        ]);
        crate::reftest::assert_complete(&defs, "AHCI_", &names);
    }
}
/* </TESTS> */
