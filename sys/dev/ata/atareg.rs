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
 * Copyright (c) 1998, 2001 Manuel Bouyer.
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
 */
/* </LICENSES> */

/* <CODE> */
//! The ATA/ATAPI drive parameter block: `struct ataparams`, the 512 bytes an IDENTIFY DEVICE
//! (or IDENTIFY PACKET DEVICE) command returns, with the bits of its words.
//!
//! Upstream: sys/dev/ata/atareg.h @ 3ce1f3f79392
//!
//! The bit names say who uses them: `WDC_*` are common to ATA and ATAPI, `ATA_*` are ATA
//! only and `ATAPI_*` ATAPI only.
//!
//! ## Deviations
//! - `struct ataparams` is a `#[repr(C)]` [`Ataparams`] of the same 512-byte layout (checked
//!   at compile time). The byte pairs the C orders by `BYTE_ORDER` are ordered by
//!   `target_endian` the same way; both kernel targets are little-endian.
//! - [`Ataparams::from_bytes`] and [`Ataparams::as_bytes`] stand for the C's `bcopy` of the
//!   IDENTIFY buffer into the structure and its `bcmp` against another copy.
//! - `ATAPI_CFG_TYPE(x)` is the `const fn` [`ATAPI_CFG_TYPE`].

use core::mem::size_of;

/*
 * Drive parameter structure for ATA/ATAPI.
 * Bit fields: WDC_* : common to ATA/ATAPI
 *             ATA_* : ATA only
 *             ATAPI_* : ATAPI only.
 */

// atap_config, word 0: general configuration.

/// `WDC_CFG_ATAPI_MASK`.
pub const WDC_CFG_ATAPI_MASK: u16 = 0xc000;
/// `WDC_CFG_ATAPI`.
pub const WDC_CFG_ATAPI: u16 = 0x8000;
/// `ATA_CFG_REMOVABLE`.
pub const ATA_CFG_REMOVABLE: u16 = 0x0080;
/// `ATA_CFG_FIXED`.
pub const ATA_CFG_FIXED: u16 = 0x0040;
/// `ATAPI_CFG_TYPE_MASK`.
pub const ATAPI_CFG_TYPE_MASK: u16 = 0x1f00;
/// `ATAPI_CFG_TYPE_DIRECT`.
pub const ATAPI_CFG_TYPE_DIRECT: u16 = 0x00;
/// `ATAPI_CFG_TYPE_SEQUENTIAL`.
pub const ATAPI_CFG_TYPE_SEQUENTIAL: u16 = 0x01;
/// `ATAPI_CFG_TYPE_CDROM`.
pub const ATAPI_CFG_TYPE_CDROM: u16 = 0x05;
/// `ATAPI_CFG_TYPE_OPTICAL`.
pub const ATAPI_CFG_TYPE_OPTICAL: u16 = 0x07;
/// `ATAPI_CFG_TYPE_NODEVICE`.
pub const ATAPI_CFG_TYPE_NODEVICE: u16 = 0x1F;
/// `ATAPI_CFG_REMOV`.
pub const ATAPI_CFG_REMOV: u16 = 0x0080;
/// `ATAPI_CFG_DRQ_MASK`.
pub const ATAPI_CFG_DRQ_MASK: u16 = 0x0060;
/// `ATAPI_CFG_STD_DRQ`.
pub const ATAPI_CFG_STD_DRQ: u16 = 0x0000;
/// `ATAPI_CFG_IRQ_DRQ`.
pub const ATAPI_CFG_IRQ_DRQ: u16 = 0x0020;
/// `ATAPI_CFG_ACCEL_DRQ`.
pub const ATAPI_CFG_ACCEL_DRQ: u16 = 0x0040;
/// `ATAPI_CFG_CMD_MASK`.
pub const ATAPI_CFG_CMD_MASK: u16 = 0x0003;
/// `ATAPI_CFG_CMD_12`.
pub const ATAPI_CFG_CMD_12: u16 = 0x0000;
/// `ATAPI_CFG_CMD_16`.
pub const ATAPI_CFG_CMD_16: u16 = 0x0001;

// atap_capabilities1, word 49: capability flags.

/// `WDC_CAP_IORDY`.
pub const WDC_CAP_IORDY: u16 = 0x0800;
/// `WDC_CAP_IORDY_DSBL`.
pub const WDC_CAP_IORDY_DSBL: u16 = 0x0400;
/// `WDC_CAP_LBA`.
pub const WDC_CAP_LBA: u16 = 0x0200;
/// `WDC_CAP_DMA`.
pub const WDC_CAP_DMA: u16 = 0x0100;
/// `ATA_CAP_STBY`.
pub const ATA_CAP_STBY: u16 = 0x2000;
/// `ATAPI_CAP_INTERL_DMA`.
pub const ATAPI_CAP_INTERL_DMA: u16 = 0x8000;
/// `ATAPI_CAP_CMD_QUEUE`.
pub const ATAPI_CAP_CMD_QUEUE: u16 = 0x4000;
/// `ATAPI_CAP_OVERLP`.
pub const ATAPI_CAP_OVERLP: u16 = 0x2000;
/// `ATAPI_CAP_ATA_RST`.
pub const ATAPI_CAP_ATA_RST: u16 = 0x1000;

// atap_extensions, word 53: extensions supported.

/// `WDC_EXT_UDMA_MODES`.
pub const WDC_EXT_UDMA_MODES: u16 = 0x0004;
/// `WDC_EXT_MODES`.
pub const WDC_EXT_MODES: u16 = 0x0002;
/// `WDC_EXT_GEOM`.
pub const WDC_EXT_GEOM: u16 = 0x0001;

// atap_curmulti, word 59: current multi-sector setting.

/// `WDC_MULTI_VALID`.
pub const WDC_MULTI_VALID: u16 = 0x0100;
/// `WDC_MULTI_MASK`.
pub const WDC_MULTI_MASK: u16 = 0x00ff;

// atap_queuedepth, word 75.

/// `WDC_QUEUE_DEPTH_MASK`.
pub const WDC_QUEUE_DEPTH_MASK: u16 = 0x1f;

// atap_sata_caps, word 76: SATA capabilities.

/// `SATA_SIGNAL_GEN1`: SATA Gen-1 signaling speed.
pub const SATA_SIGNAL_GEN1: u16 = 0x0002;
/// `SATA_SIGNAL_GEN2`: SATA Gen-2 signaling speed.
pub const SATA_SIGNAL_GEN2: u16 = 0x0004;
/// `SATA_NATIVE_CMDQ`: native command queuing.
pub const SATA_NATIVE_CMDQ: u16 = 0x0100;
/// `SATA_HOST_PWR_MGMT`: power management (host).
pub const SATA_HOST_PWR_MGMT: u16 = 0x0200;

// atap_sata_features_supp, word 78: SATA features supported.

/// `SATA_NONZERO_OFFSETS`: non-zero buffer offsets.
pub const SATA_NONZERO_OFFSETS: u16 = 0x0002;
/// `SATA_DMA_SETUP_AUTO`: DMA setup auto-activate.
pub const SATA_DMA_SETUP_AUTO: u16 = 0x0004;
/// `SATA_DRIVE_PWR_MGMT`: power management (device).
pub const SATA_DRIVE_PWR_MGMT: u16 = 0x0008;

// atap_ata_major, word 80: major version number.

/// `WDC_VER_ATA1`.
pub const WDC_VER_ATA1: u16 = 0x0002;
/// `WDC_VER_ATA2`.
pub const WDC_VER_ATA2: u16 = 0x0004;
/// `WDC_VER_ATA3`.
pub const WDC_VER_ATA3: u16 = 0x0008;
/// `WDC_VER_ATA4`.
pub const WDC_VER_ATA4: u16 = 0x0010;
/// `WDC_VER_ATA5`.
pub const WDC_VER_ATA5: u16 = 0x0020;
/// `WDC_VER_ATA6`.
pub const WDC_VER_ATA6: u16 = 0x0040;
/// `WDC_VER_ATA7`.
pub const WDC_VER_ATA7: u16 = 0x0080;
/// `WDC_VER_ATA8`.
pub const WDC_VER_ATA8: u16 = 0x0100;
/// `WDC_VER_ATA9`.
pub const WDC_VER_ATA9: u16 = 0x0200;
/// `WDC_VER_ATA10`.
pub const WDC_VER_ATA10: u16 = 0x0400;
/// `WDC_VER_ATA11`.
pub const WDC_VER_ATA11: u16 = 0x0800;
/// `WDC_VER_ATA12`.
pub const WDC_VER_ATA12: u16 = 0x1000;
/// `WDC_VER_ATA13`.
pub const WDC_VER_ATA13: u16 = 0x2000;
/// `WDC_VER_ATA14`.
pub const WDC_VER_ATA14: u16 = 0x4000;

// atap_cmd_set1, word 82 (and atap_cmd1_en, word 85): command set supported.

/// `WDC_CMD1_NOP`.
pub const WDC_CMD1_NOP: u16 = 0x4000;
/// `WDC_CMD1_RB`.
pub const WDC_CMD1_RB: u16 = 0x2000;
/// `WDC_CMD1_WB`.
pub const WDC_CMD1_WB: u16 = 0x1000;
/// `WDC_CMD1_HPA`.
pub const WDC_CMD1_HPA: u16 = 0x0400;
/// `WDC_CMD1_DVRST`.
pub const WDC_CMD1_DVRST: u16 = 0x0200;
/// `WDC_CMD1_SRV`.
pub const WDC_CMD1_SRV: u16 = 0x0100;
/// `WDC_CMD1_RLSE`.
pub const WDC_CMD1_RLSE: u16 = 0x0080;
/// `WDC_CMD1_AHEAD`.
pub const WDC_CMD1_AHEAD: u16 = 0x0040;
/// `WDC_CMD1_CACHE`.
pub const WDC_CMD1_CACHE: u16 = 0x0020;
/// `WDC_CMD1_PKT`.
pub const WDC_CMD1_PKT: u16 = 0x0010;
/// `WDC_CMD1_PM`.
pub const WDC_CMD1_PM: u16 = 0x0008;
/// `WDC_CMD1_REMOV`.
pub const WDC_CMD1_REMOV: u16 = 0x0004;
/// `WDC_CMD1_SEC`.
pub const WDC_CMD1_SEC: u16 = 0x0002;
/// `WDC_CMD1_SMART`.
pub const WDC_CMD1_SMART: u16 = 0x0001;

// atap_cmd_set2, word 83 (and atap_cmd2_en, word 86): command set supported.

/// `ATAPI_CMD2_FCE`: Flush Cache Ext supported.
pub const ATAPI_CMD2_FCE: u16 = 0x2000;
/// `ATAPI_CMD2_FC`: Flush Cache supported.
pub const ATAPI_CMD2_FC: u16 = 0x1000;
/// `ATAPI_CMD2_DCO`: Device Configuration Overlay supported.
pub const ATAPI_CMD2_DCO: u16 = 0x0800;
/// `ATAPI_CMD2_48AD`: 48bit address supported.
pub const ATAPI_CMD2_48AD: u16 = 0x0400;
/// `ATAPI_CMD2_AAM`: Automatic Acoustic Management supported.
pub const ATAPI_CMD2_AAM: u16 = 0x0200;
/// `ATAPI_CMD2_SM`: Set Max security extension supported.
pub const ATAPI_CMD2_SM: u16 = 0x0100;
/// `ATAPI_CMD2_SF`: Set Features subcommand required.
pub const ATAPI_CMD2_SF: u16 = 0x0040;
/// `ATAPI_CMD2_PUIS`: Power up in standby supported.
pub const ATAPI_CMD2_PUIS: u16 = 0x0020;
/// `WDC_CMD2_RMSN`.
pub const WDC_CMD2_RMSN: u16 = 0x0010;
/// `ATA_CMD2_APM`.
pub const ATA_CMD2_APM: u16 = 0x0008;
/// `ATA_CMD2_CFA`.
pub const ATA_CMD2_CFA: u16 = 0x0004;
/// `ATA_CMD2_RWQ`.
pub const ATA_CMD2_RWQ: u16 = 0x0002;
/// `WDC_CMD2_DM`: Download Microcode supported.
pub const WDC_CMD2_DM: u16 = 0x0001;

// atap_cmd_ext, word 84: command/features supported, extension.

/// `ATAPI_CMDE_IIUF`: IDLE IMMEDIATE with UNLOAD FEATURE.
pub const ATAPI_CMDE_IIUF: u16 = 0x2000;
/// `ATAPI_CMDE_MSER`: Media serial number supported.
pub const ATAPI_CMDE_MSER: u16 = 0x0004;
/// `ATAPI_CMDE_TEST`: SMART self-test supported.
pub const ATAPI_CMDE_TEST: u16 = 0x0002;
/// `ATAPI_CMDE_SLOG`: SMART error logging supported.
pub const ATAPI_CMDE_SLOG: u16 = 0x0001;

// atap_hwreset_res, word 93: hardware reset value.

/// `ATA_HWRES_CBLID`: CBLID above Vih.
pub const ATA_HWRES_CBLID: u16 = 0x2000;
/// `ATA_HWRES_D1_PDIAG`: Device 1 PDIAG detect OK.
pub const ATA_HWRES_D1_PDIAG: u16 = 0x0800;
/// `ATA_HWRES_D1_CSEL`: Device 1 used CSEL for address.
pub const ATA_HWRES_D1_CSEL: u16 = 0x0400;
/// `ATA_HWRES_D1_JUMP`: Device 1 jumpered to address.
pub const ATA_HWRES_D1_JUMP: u16 = 0x0200;
/// `ATA_HWRES_D0_SEL`: Device 0 responds when Dev 1 selected.
pub const ATA_HWRES_D0_SEL: u16 = 0x0040;
/// `ATA_HWRES_D0_DASP`: Device 0 DASP detect OK.
pub const ATA_HWRES_D0_DASP: u16 = 0x0020;
/// `ATA_HWRES_D0_PDIAG`: Device 0 PDIAG detect OK.
pub const ATA_HWRES_D0_PDIAG: u16 = 0x0010;
/// `ATA_HWRES_D0_DIAG`: Device 0 diag OK.
pub const ATA_HWRES_D0_DIAG: u16 = 0x0008;
/// `ATA_HWRES_D0_CSEL`: Device 0 used CSEL for address.
pub const ATA_HWRES_D0_CSEL: u16 = 0x0004;
/// `ATA_HWRES_D0_JUMP`: Device 0 jumpered to address.
pub const ATA_HWRES_D0_JUMP: u16 = 0x0002;

// atap_rmsn_supp, word 127: removable media status notification.

/// `WDC_RMSN_SUPP_MASK`.
pub const WDC_RMSN_SUPP_MASK: u16 = 0x0003;
/// `WDC_RMSN_SUPP`.
pub const WDC_RMSN_SUPP: u16 = 0x0001;

// atap_sec_st, word 128: security status.

/// `WDC_SEC_LEV_MAX`.
pub const WDC_SEC_LEV_MAX: u16 = 0x0100;
/// `WDC_SEC_ESE_SUPP`.
pub const WDC_SEC_ESE_SUPP: u16 = 0x0020;
/// `WDC_SEC_EXP`.
pub const WDC_SEC_EXP: u16 = 0x0010;
/// `WDC_SEC_FROZEN`.
pub const WDC_SEC_FROZEN: u16 = 0x0008;
/// `WDC_SEC_LOCKED`.
pub const WDC_SEC_LOCKED: u16 = 0x0004;
/// `WDC_SEC_EN`.
pub const WDC_SEC_EN: u16 = 0x0002;
/// `WDC_SEC_SUPP`.
pub const WDC_SEC_SUPP: u16 = 0x0001;

// atap_cfa_power, word 160: CFA power mode.

/// `ATAPI_CFA_MAX_MASK`.
pub const ATAPI_CFA_MAX_MASK: u16 = 0x0FFF;
/// `ATAPI_CFA_MODE1_DIS`: CFA Mode 1 Disabled.
pub const ATAPI_CFA_MODE1_DIS: u16 = 0x1000;
/// `ATAPI_CFA_MODE1_REQ`: CFA Mode 1 Required.
pub const ATAPI_CFA_MODE1_REQ: u16 = 0x2000;
/// `ATAPI_CFA_WORD160`: Word 160 supported.
pub const ATAPI_CFA_WORD160: u16 = 0x8000;

/// `sizeof(struct ataparams)`: one 512-byte IDENTIFY block.
pub const ATAPARAMS_LEN: usize = 512;

/// `struct ataparams`: drive parameter structure for ATA/ATAPI. Words 1-9 and 54-62 are
/// ATA only, 71-72 ATAPI only, 89-92 ATA only.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ataparams {
    /* drive info */
    /// `atap_config`, 0: general configuration.
    pub atap_config: u16,
    /// `atap_cylinders`, 1: # of non-removable cylinders.
    pub atap_cylinders: u16,
    /// `__reserved1`.
    pub __reserved1: u16,
    /// `atap_heads`, 3: # of heads.
    pub atap_heads: u16,
    /// `__retired1`, 4-5: # of unform. bytes/track.
    pub __retired1: [u16; 2],
    /// `atap_sectors`, 6: # of sectors.
    pub atap_sectors: u16,
    /// `__retired2`.
    pub __retired2: [u16; 3],

    /// `atap_serial`, 10-19: serial number.
    pub atap_serial: [u8; 20],
    /// `__retired3`.
    pub __retired3: [u16; 2],
    /// `__obsolete1`.
    pub __obsolete1: u16,
    /// `atap_revision`, 23-26: firmware revision.
    pub atap_revision: [u8; 8],
    /// `atap_model`, 27-46: model number.
    pub atap_model: [u8; 40],
    /// `atap_multi`, 47: maximum sectors per irq (ATA).
    pub atap_multi: u16,
    /// `__reserved2`.
    pub __reserved2: u16,
    /// `atap_capabilities1`, 49: capability flags.
    pub atap_capabilities1: u16,
    /// `atap_capabilities2`, 50: capability flags (ATA).
    pub atap_capabilities2: u16,
    #[cfg(target_endian = "little")]
    /// `__junk2`.
    pub __junk2: u8,
    /// `atap_oldpiotiming`, 51: old PIO timing mode.
    pub atap_oldpiotiming: u8,
    #[cfg(target_endian = "big")]
    /// `__junk2`.
    pub __junk2: u8,
    #[cfg(target_endian = "little")]
    /// `__junk3`.
    pub __junk3: u8,
    /// `atap_olddmatiming`, 52: old DMA timing mode (ATA).
    pub atap_olddmatiming: u8,
    #[cfg(target_endian = "big")]
    /// `__junk3`.
    pub __junk3: u8,
    /// `atap_extensions`, 53: extensions supported.
    pub atap_extensions: u16,
    /// `atap_curcylinders`, 54: current logical cylinders.
    pub atap_curcylinders: u16,
    /// `atap_curheads`, 55: current logical heads.
    pub atap_curheads: u16,
    /// `atap_cursectors`, 56: current logical sectors/tracks.
    pub atap_cursectors: u16,
    /// `atap_curcapacity`, 57-58: current capacity.
    pub atap_curcapacity: [u16; 2],
    /// `atap_curmulti`, 59: current multi-sector setting.
    pub atap_curmulti: u16,
    /// `atap_capacity`, 60-61: total capacity (LBA only).
    pub atap_capacity: [u16; 2],
    /// `__retired4`.
    pub __retired4: u16,
    #[cfg(target_endian = "big")]
    /// `atap_dmamode_act`: multiword DMA mode active.
    pub atap_dmamode_act: u8,
    /// `atap_dmamode_supp`, 63: multiword DMA mode supported.
    pub atap_dmamode_supp: u8,
    #[cfg(target_endian = "little")]
    /// `atap_dmamode_act`: multiword DMA mode active.
    pub atap_dmamode_act: u8,
    #[cfg(target_endian = "big")]
    /// `__junk4`.
    pub __junk4: u8,
    /// `atap_piomode_supp`, 64: PIO mode supported.
    pub atap_piomode_supp: u8,
    #[cfg(target_endian = "little")]
    /// `__junk4`.
    pub __junk4: u8,
    /// `atap_dmatiming_mimi`, 65: minimum DMA cycle time.
    pub atap_dmatiming_mimi: u16,
    /// `atap_dmatiming_recom`, 66: recommended DMA cycle time.
    pub atap_dmatiming_recom: u16,
    /// `atap_piotiming`, 67: mini PIO cycle time without FC.
    pub atap_piotiming: u16,
    /// `atap_piotiming_iordy`, 68: mini PIO cycle time with IORDY FC.
    pub atap_piotiming_iordy: u16,
    /// `__reserved3`.
    pub __reserved3: [u16; 2],
    /// `atap_pkt_br`, 71: time (ns) to bus release.
    pub atap_pkt_br: u16,
    /// `atap_pkt_bsyclr`, 72: time to clear BSY after service.
    pub atap_pkt_bsyclr: u16,
    /// `__reserved4`.
    pub __reserved4: [u16; 2],
    /// `atap_queuedepth`, 75.
    pub atap_queuedepth: u16,
    /// `atap_sata_caps`, 76: SATA capabilities.
    pub atap_sata_caps: u16,
    /// `atap_sata_reserved`, 77: reserved.
    pub atap_sata_reserved: u16,
    /// `atap_sata_features_supp`, 78: SATA features supported.
    pub atap_sata_features_supp: u16,
    /// `atap_sata_features_en`, 79: SATA features enabled.
    pub atap_sata_features_en: u16,
    /// `atap_ata_major`, 80: Major version number.
    pub atap_ata_major: u16,
    /// `atap_ata_minor`, 81: Minor version number.
    pub atap_ata_minor: u16,
    /// `atap_cmd_set1`, 82: command set supported.
    pub atap_cmd_set1: u16,
    /// `atap_cmd_set2`, 83: command set supported.
    pub atap_cmd_set2: u16,
    /// `atap_cmd_ext`, 84: command/features supp. ext.
    pub atap_cmd_ext: u16,
    /// `atap_cmd1_en`, 85: cmd/features enabled (bits as `atap_cmd_set1`).
    pub atap_cmd1_en: u16,
    /// `atap_cmd2_en`, 86: cmd/features enabled (bits as `atap_cmd_set2`).
    pub atap_cmd2_en: u16,
    /// `atap_cmd_def`, 87: cmd/features default (bits NOT as `atap_cmd_ext`).
    pub atap_cmd_def: u16,
    #[cfg(target_endian = "big")]
    /// `atap_udmamode_act`: Ultra-DMA mode active.
    pub atap_udmamode_act: u8,
    /// `atap_udmamode_supp`, 88: Ultra-DMA mode supported.
    pub atap_udmamode_supp: u8,
    #[cfg(target_endian = "little")]
    /// `atap_udmamode_act`: Ultra-DMA mode active.
    pub atap_udmamode_act: u8,
    /// `atap_seu_time`, 89: Sec. Erase Unit compl. time.
    pub atap_seu_time: u16,
    /// `atap_eseu_time`, 90: Enhanced SEU compl. time.
    pub atap_eseu_time: u16,
    /// `atap_apm_val`, 91: current APM value.
    pub atap_apm_val: u16,
    /// `atap_mpasswd_rev`, 92: Master Password revision.
    pub atap_mpasswd_rev: u16,
    /// `atap_hwreset_res`, 93: Hardware reset value.
    pub atap_hwreset_res: u16,
    #[cfg(target_endian = "big")]
    /// `atap_acoustic_def`: recommended level.
    pub atap_acoustic_def: u8,
    /// `atap_acoustic_val`, 94: Current acoustic level.
    pub atap_acoustic_val: u8,
    #[cfg(target_endian = "little")]
    /// `atap_acoustic_def`: recommended level.
    pub atap_acoustic_def: u8,
    /// `__reserved6`, 95-99: reserved.
    pub __reserved6: [u16; 5],
    /// `atap_max_lba`, 100-103: Max. user LBA add.
    pub atap_max_lba: [u16; 4],
    /// `__reserved7`, 104-126: reserved.
    pub __reserved7: [u16; 23],
    /// `atap_rmsn_supp`, 127: remov. media status notif.
    pub atap_rmsn_supp: u16,
    /// `atap_sec_st`, 128: security status.
    pub atap_sec_st: u16,
    /// `__reserved8`, 129-159: vendor specific.
    pub __reserved8: [u16; 31],
    /// `atap_cfa_power`, 160: CFA powermode.
    pub atap_cfa_power: u16,
    /// `__reserved9`, 161-175: reserved for CFA.
    pub __reserved9: [u16; 15],
    /// `atap_media_serial`, 176-205: media serial number.
    pub atap_media_serial: [u8; 60],
    /// `__reserved10`, 206-254: reserved.
    pub __reserved10: [u16; 49],
    #[cfg(target_endian = "big")]
    /// `atap_checksum`: Checksum.
    pub atap_checksum: u8,
    /// `atap_signature`, 255: Signature.
    pub atap_signature: u8,
    #[cfg(target_endian = "little")]
    /// `atap_checksum`: Checksum.
    pub atap_checksum: u8,
}

impl Ataparams {
    /// All zero (`bzero(prms, sizeof(struct ataparams))`).
    pub const fn zeroed() -> Self {
        Self::from_bytes(&[0; ATAPARAMS_LEN])
    }

    /// `bcopy(tb, prms, sizeof(struct ataparams))`: the structure the 512 bytes of `tb` are.
    pub const fn from_bytes(tb: &[u8; ATAPARAMS_LEN]) -> Self {
        // SAFETY: `Ataparams` is `#[repr(C)]` of `u8`/`u16` fields and arrays with no
        // padding (its size is checked to be 512), so every 512-byte pattern is a valid
        // value; the read is unaligned because `tb` is a byte array.
        unsafe { core::ptr::read_unaligned(tb.as_ptr().cast::<Self>()) }
    }

    /// The structure's 512 bytes (what the C's `bcmp` of two copies compares).
    pub const fn as_bytes(&self) -> &[u8; ATAPARAMS_LEN] {
        // SAFETY: `Ataparams` is 512 bytes of plain integers with no padding (checked
        // below), so every byte is initialised; a byte array has alignment 1.
        unsafe { &*core::ptr::from_ref(self).cast::<[u8; ATAPARAMS_LEN]>() }
    }
}

impl Default for Ataparams {
    fn default() -> Self {
        Self::zeroed()
    }
}

/// `ATAPI_CFG_TYPE(x)`: the ATAPI device type of `atap_config`.
#[allow(non_snake_case)] // the C macro's name
pub const fn ATAPI_CFG_TYPE(x: u16) -> u16 {
    (x & ATAPI_CFG_TYPE_MASK) >> 8
}

const _: () = {
    assert!(size_of::<Ataparams>() == ATAPARAMS_LEN);
    assert!(core::mem::offset_of!(Ataparams, atap_serial) == 10 * 2);
    assert!(core::mem::offset_of!(Ataparams, atap_model) == 27 * 2);
    assert!(core::mem::offset_of!(Ataparams, atap_multi) == 47 * 2);
    assert!(core::mem::offset_of!(Ataparams, atap_capabilities1) == 49 * 2);
    assert!(core::mem::offset_of!(Ataparams, atap_extensions) == 53 * 2);
    assert!(core::mem::offset_of!(Ataparams, atap_capacity) == 60 * 2);
    assert!(core::mem::offset_of!(Ataparams, atap_dmamode_supp) == 63 * 2);
    assert!(core::mem::offset_of!(Ataparams, atap_piomode_supp) == 64 * 2);
    assert!(core::mem::offset_of!(Ataparams, atap_ata_major) == 80 * 2);
    assert!(core::mem::offset_of!(Ataparams, atap_cmd_set1) == 82 * 2);
    assert!(core::mem::offset_of!(Ataparams, atap_cmd2_en) == 86 * 2);
    assert!(core::mem::offset_of!(Ataparams, atap_udmamode_supp) == 88 * 2);
    assert!(core::mem::offset_of!(Ataparams, atap_max_lba) == 100 * 2);
    assert!(core::mem::offset_of!(Ataparams, atap_sec_st) == 128 * 2);
    assert!(core::mem::offset_of!(Ataparams, atap_cfa_power) == 160 * 2);
    assert!(core::mem::offset_of!(Ataparams, atap_media_serial) == 176 * 2);
    assert!(core::mem::offset_of!(Ataparams, atap_signature) == 255 * 2);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identify_words_land_on_their_fields() {
        let mut tb = [0u8; ATAPARAMS_LEN];
        tb[0..2].copy_from_slice(&0x0040u16.to_le_bytes()); // config: fixed
        tb[2..4].copy_from_slice(&16383u16.to_le_bytes()); // cylinders
        tb[2 * 47] = 16; // multi, low byte
        tb[2 * 51 + 1] = 2; // old PIO timing
        tb[2 * 63] = 0x07; // multiword DMA supported
        tb[2 * 63 + 1] = 0x04; // multiword DMA active
        tb[2 * 64] = 0x03; // PIO 3 and 4
        tb[2 * 88] = 0x3f; // UDMA supported
        tb[2 * 88 + 1] = 0x20; // UDMA 5 active
        tb[2 * 100..2 * 100 + 2].copy_from_slice(&0x0000u16.to_le_bytes());
        tb[2 * 100 + 2..2 * 100 + 4].copy_from_slice(&0x0002u16.to_le_bytes());
        tb[510] = 0xa5;
        tb[511] = 0x5a;
        let p = Ataparams::from_bytes(&tb);
        assert_eq!(p.atap_config, ATA_CFG_FIXED);
        assert_eq!(p.atap_cylinders, 16383);
        assert_eq!(p.atap_multi & 0xff, 16);
        assert_eq!(p.atap_oldpiotiming, 2);
        assert_eq!(p.atap_dmamode_supp, 0x07);
        assert_eq!(p.atap_dmamode_act, 0x04);
        assert_eq!(p.atap_piomode_supp, 0x03);
        assert_eq!(p.atap_udmamode_supp, 0x3f);
        assert_eq!(p.atap_udmamode_act, 0x20);
        assert_eq!(p.atap_max_lba, [0, 2, 0, 0]);
        assert_eq!(p.atap_signature, 0xa5);
        assert_eq!(p.atap_checksum, 0x5a);
        assert_eq!(p.as_bytes(), &tb);
        assert_eq!(Ataparams::zeroed().as_bytes(), &[0; ATAPARAMS_LEN]);
    }

    #[test]
    fn atapi_device_type() {
        assert_eq!(ATAPI_CFG_TYPE(0x8580), ATAPI_CFG_TYPE_CDROM);
        assert_eq!(ATAPI_CFG_TYPE(0x9f00), ATAPI_CFG_TYPE_NODEVICE);
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/ata/atareg.h");
        let names = crate::reftest::assert_defines!(defs;
            WDC_CFG_ATAPI_MASK, WDC_CFG_ATAPI, ATA_CFG_REMOVABLE, ATA_CFG_FIXED,
            ATAPI_CFG_TYPE_MASK, ATAPI_CFG_TYPE_DIRECT, ATAPI_CFG_TYPE_SEQUENTIAL,
            ATAPI_CFG_TYPE_CDROM, ATAPI_CFG_TYPE_OPTICAL, ATAPI_CFG_TYPE_NODEVICE,
            ATAPI_CFG_REMOV, ATAPI_CFG_DRQ_MASK, ATAPI_CFG_STD_DRQ, ATAPI_CFG_IRQ_DRQ,
            ATAPI_CFG_ACCEL_DRQ, ATAPI_CFG_CMD_MASK, ATAPI_CFG_CMD_12, ATAPI_CFG_CMD_16,
            WDC_CAP_IORDY, WDC_CAP_IORDY_DSBL, WDC_CAP_LBA, WDC_CAP_DMA, ATA_CAP_STBY,
            ATAPI_CAP_INTERL_DMA, ATAPI_CAP_CMD_QUEUE, ATAPI_CAP_OVERLP, ATAPI_CAP_ATA_RST,
            WDC_EXT_UDMA_MODES, WDC_EXT_MODES, WDC_EXT_GEOM, WDC_MULTI_VALID, WDC_MULTI_MASK,
            WDC_QUEUE_DEPTH_MASK, SATA_SIGNAL_GEN1, SATA_SIGNAL_GEN2, SATA_NATIVE_CMDQ,
            SATA_HOST_PWR_MGMT, SATA_NONZERO_OFFSETS, SATA_DMA_SETUP_AUTO, SATA_DRIVE_PWR_MGMT,
            WDC_VER_ATA1, WDC_VER_ATA2, WDC_VER_ATA3, WDC_VER_ATA4, WDC_VER_ATA5, WDC_VER_ATA6,
            WDC_VER_ATA7, WDC_VER_ATA8, WDC_VER_ATA9, WDC_VER_ATA10, WDC_VER_ATA11,
            WDC_VER_ATA12, WDC_VER_ATA13, WDC_VER_ATA14, WDC_CMD1_NOP, WDC_CMD1_RB,
            WDC_CMD1_WB, WDC_CMD1_HPA, WDC_CMD1_DVRST, WDC_CMD1_SRV, WDC_CMD1_RLSE,
            WDC_CMD1_AHEAD, WDC_CMD1_CACHE, WDC_CMD1_PKT, WDC_CMD1_PM, WDC_CMD1_REMOV,
            WDC_CMD1_SEC, WDC_CMD1_SMART, ATAPI_CMD2_FCE, ATAPI_CMD2_FC, ATAPI_CMD2_DCO,
            ATAPI_CMD2_48AD, ATAPI_CMD2_AAM, ATAPI_CMD2_SM, ATAPI_CMD2_SF, ATAPI_CMD2_PUIS,
            WDC_CMD2_RMSN, ATA_CMD2_APM, ATA_CMD2_CFA, ATA_CMD2_RWQ, WDC_CMD2_DM,
            ATAPI_CMDE_IIUF, ATAPI_CMDE_MSER, ATAPI_CMDE_TEST, ATAPI_CMDE_SLOG,
            ATA_HWRES_CBLID, ATA_HWRES_D1_PDIAG, ATA_HWRES_D1_CSEL, ATA_HWRES_D1_JUMP,
            ATA_HWRES_D0_SEL, ATA_HWRES_D0_DASP, ATA_HWRES_D0_PDIAG, ATA_HWRES_D0_DIAG,
            ATA_HWRES_D0_CSEL, ATA_HWRES_D0_JUMP, WDC_RMSN_SUPP_MASK, WDC_RMSN_SUPP,
            WDC_SEC_LEV_MAX, WDC_SEC_ESE_SUPP, WDC_SEC_EXP, WDC_SEC_FROZEN, WDC_SEC_LOCKED,
            WDC_SEC_EN, WDC_SEC_SUPP, ATAPI_CFA_MAX_MASK, ATAPI_CFA_MODE1_DIS,
            ATAPI_CFA_MODE1_REQ, ATAPI_CFA_WORD160,
        );
        for prefix in ["WDC_", "ATA_", "ATAPI_", "SATA_"] {
            crate::reftest::assert_complete(&defs, prefix, &names);
        }
    }
}
/* </TESTS> */
