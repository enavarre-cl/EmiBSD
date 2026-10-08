/*	$OpenBSD: siopreg.h,v 1.12 2010/07/23 07:47:13 jsg Exp $ */
/*	$NetBSD: siopreg.h,v 1.16 2005/02/27 00:27:02 perry Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 2000 Manuel Bouyer.
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
//! `<dev/ic/siopreg.h>`: the registers of the Symbios/NCR 53c7xx/8xx PCI-SCSI I/O processors
//! (siop(4)) and their bits, the clock tables that turn a synchronous period into the
//! `SCNTL3` clock factor, and the layout of the Symbios NVRAM.
//!
//! Upstream: sys/dev/ic/siopreg.h @ 3ce1f3f79392
//!
//! "Devices definitions for Symbios/NCR M53c8xx PCI-SCSI I/O Processors. Docs available from
//! http://www.symbios.com/". Register offsets are `usize` (`bus_size_t`, what `bus_space(9)`
//! takes); the bits of the 8-bit registers are `u8`; shift counts are `u32`.
//!
//! ## Deviations
//! - `scf_period[]` and `dt_scf_period[]`, defined under `SIOP_NEEDS_PERIOD_TABLES` for
//!   `siop_common.c` alone, are the statics [`SCF_PERIOD`] and [`DT_SCF_PERIOD`].
//! - `#define STEST4_` (an empty, unused define) has no value to port and is left out.
//! - The `#if 0` block (`tekram_sync_table[]`, `struct nvram_tekram` and its `NVRAM_TEK_*`
//!   flags) is dead code in the C and is not ported.
//! - `struct nvram_symbios` and its nested structures are `#[repr(C, packed)]` with the C's
//!   368 bytes (asserted); no driver here reads the NVRAM.
//! - `AIPCNTL0_PARITYERRs` and `SIOP_NVRAM_TEK_*c*_ADDRESS` keep their mixed-case C names.

/// `SIOP_SCNTL0`: SCSI control 0, R/W.
pub const SIOP_SCNTL0: usize = 0x00;
/// `SCNTL0_ARB_MASK`.
pub const SCNTL0_ARB_MASK: u8 = 0xc0;
/// `SCNTL0_SARB`.
pub const SCNTL0_SARB: u8 = 0x00;
/// `SCNTL0_FARB`.
pub const SCNTL0_FARB: u8 = 0xc0;
/// `SCNTL0_START`.
pub const SCNTL0_START: u8 = 0x20;
/// `SCNTL0_WATM`.
pub const SCNTL0_WATM: u8 = 0x10;
/// `SCNTL0_EPC`.
pub const SCNTL0_EPC: u8 = 0x08;
/// `SCNTL0_AAP`.
pub const SCNTL0_AAP: u8 = 0x02;
/// `SCNTL0_TRG`.
pub const SCNTL0_TRG: u8 = 0x01;
/// `SIOP_SCNTL1`: SCSI control 1, R/W.
pub const SIOP_SCNTL1: usize = 0x01;
/// `SCNTL1_EXC`.
pub const SCNTL1_EXC: u8 = 0x80;
/// `SCNTL1_ADB`.
pub const SCNTL1_ADB: u8 = 0x40;
/// `SCNTL1_DHP`.
pub const SCNTL1_DHP: u8 = 0x20;
/// `SCNTL1_CON`.
pub const SCNTL1_CON: u8 = 0x10;
/// `SCNTL1_RST`.
pub const SCNTL1_RST: u8 = 0x08;
/// `SCNTL1_AESP`.
pub const SCNTL1_AESP: u8 = 0x04;
/// `SCNTL1_IARB`.
pub const SCNTL1_IARB: u8 = 0x02;
/// `SCNTL1_SST`.
pub const SCNTL1_SST: u8 = 0x01;
/// `SIOP_SCNTL2`: SCSI control 2, R/W.
pub const SIOP_SCNTL2: usize = 0x02;
/// `SCNTL2_SDU`.
pub const SCNTL2_SDU: u8 = 0x80;
/// `SCNTL2_CHM`: 875 only.
pub const SCNTL2_CHM: u8 = 0x40;
/// `SCNTL2_SLPMD`: 875 only.
pub const SCNTL2_SLPMD: u8 = 0x20;
/// `SCNTL2_SLPHBEN`: 875 only.
pub const SCNTL2_SLPHBEN: u8 = 0x10;
/// `SCNTL2_WSS`: 875 only.
pub const SCNTL2_WSS: u8 = 0x08;
/// `SCNTL2_VUE0`: 875 only.
pub const SCNTL2_VUE0: u8 = 0x04;
/// `SCNTL2_VUE1`: 875 only.
pub const SCNTL2_VUE1: u8 = 0x02;
/// `SCNTL2_WSR`: 875 only.
pub const SCNTL2_WSR: u8 = 0x01;
/// `SIOP_SCNTL3`: SCSI control 3, R/W.
pub const SIOP_SCNTL3: usize = 0x03;
/// `SCNTL3_ULTRA`: 875 only.
pub const SCNTL3_ULTRA: u8 = 0x80;
/// `SCNTL3_SCF_SHIFT`.
pub const SCNTL3_SCF_SHIFT: u32 = 4;
/// `SCNTL3_SCF_MASK`.
pub const SCNTL3_SCF_MASK: u8 = 0x70;
/// `SCNTL3_EWS`: 875 only.
pub const SCNTL3_EWS: u8 = 0x08;
/// `SCNTL3_CCF_SHIFT`.
pub const SCNTL3_CCF_SHIFT: u32 = 0;
/// `SCNTL3_CCF_MASK`.
pub const SCNTL3_CCF_MASK: u8 = 0x07;
/// `SIOP_SCID`: SCSI chip ID R/W.
pub const SIOP_SCID: usize = 0x04;
/// `SCID_RRE`.
pub const SCID_RRE: u8 = 0x40;
/// `SCID_SRE`.
pub const SCID_SRE: u8 = 0x20;
/// `SCID_ENCID_SHIFT`.
pub const SCID_ENCID_SHIFT: u32 = 0;
/// `SCID_ENCID_MASK`.
pub const SCID_ENCID_MASK: u8 = 0x07;
/// `SIOP_SXFER`: SCSI transfer, R/W.
pub const SIOP_SXFER: usize = 0x05;
/// `SXFER_TP_SHIFT`.
pub const SXFER_TP_SHIFT: u32 = 5;
/// `SXFER_TP_MASK`.
pub const SXFER_TP_MASK: u8 = 0xe0;
/// `SXFER_MO_SHIFT`.
pub const SXFER_MO_SHIFT: u32 = 0;
/// `SXFER_MO_MASK`.
pub const SXFER_MO_MASK: u8 = 0x3f;
/// `SIOP_SDID`: SCSI destination ID, R/W.
pub const SIOP_SDID: usize = 0x06;
/// `SDID_ENCID_SHIFT`.
pub const SDID_ENCID_SHIFT: u32 = 0;
/// `SDID_ENCID_MASK`.
pub const SDID_ENCID_MASK: u8 = 0x07;
/// `SIOP_GPREG`: General purpose, R/W.
pub const SIOP_GPREG: usize = 0x07;
/// `GPREG_GPIO4`: 875 only.
pub const GPREG_GPIO4: u8 = 0x10;
/// `GPREG_GPIO3`: 875 only.
pub const GPREG_GPIO3: u8 = 0x08;
/// `GPREG_GPIO2`: 875 only.
pub const GPREG_GPIO2: u8 = 0x04;
/// `GPREG_GPIO1`.
pub const GPREG_GPIO1: u8 = 0x02;
/// `GPREG_GPIO0`.
pub const GPREG_GPIO0: u8 = 0x01;
/// `SIOP_SFBR`: SCSI first byte received, R/W.
pub const SIOP_SFBR: usize = 0x08;
/// `SIOP_SOCL`: SCSI output control latch, RW.
pub const SIOP_SOCL: usize = 0x09;
/// `SIOP_SSID`: SCSI selector ID, RO.
pub const SIOP_SSID: usize = 0x0A;
/// `SSID_VAL`.
pub const SSID_VAL: u8 = 0x80;
/// `SSID_ENCID_SHIFT`.
pub const SSID_ENCID_SHIFT: u32 = 0;
/// `SSID_ENCID_MASK`.
pub const SSID_ENCID_MASK: u8 = 0x0f;
/// `SIOP_SBCL`: SCSI control line, RO.
pub const SIOP_SBCL: usize = 0x0B;
/// `SIOP_DSTAT`: DMA status, RO.
pub const SIOP_DSTAT: usize = 0x0C;
/// `DSTAT_DFE`.
pub const DSTAT_DFE: u8 = 0x80;
/// `DSTAT_MDPE`.
pub const DSTAT_MDPE: u8 = 0x40;
/// `DSTAT_BF`.
pub const DSTAT_BF: u8 = 0x20;
/// `DSTAT_ABRT`.
pub const DSTAT_ABRT: u8 = 0x10;
/// `DSTAT_SSI`.
pub const DSTAT_SSI: u8 = 0x08;
/// `DSTAT_SIR`.
pub const DSTAT_SIR: u8 = 0x04;
/// `DSTAT_IID`.
pub const DSTAT_IID: u8 = 0x01;
/// `SIOP_SSTAT0`: STSI status 0, RO.
pub const SIOP_SSTAT0: usize = 0x0D;
/// `SSTAT0_ILF`.
pub const SSTAT0_ILF: u8 = 0x80;
/// `SSTAT0_ORF`.
pub const SSTAT0_ORF: u8 = 0x40;
/// `SSTAT0_OLF`.
pub const SSTAT0_OLF: u8 = 0x20;
/// `SSTAT0_AIP`.
pub const SSTAT0_AIP: u8 = 0x10;
/// `SSTAT0_LOA`.
pub const SSTAT0_LOA: u8 = 0x08;
/// `SSTAT0_WOA`.
pub const SSTAT0_WOA: u8 = 0x04;
/// `SSTAT0_RST`.
pub const SSTAT0_RST: u8 = 0x02;
/// `SSTAT0_SDP`.
pub const SSTAT0_SDP: u8 = 0x01;
/// `SIOP_SSTAT1`: STSI status 1, RO.
pub const SIOP_SSTAT1: usize = 0x0E;
/// `SSTAT1_FFO_SHIFT`.
pub const SSTAT1_FFO_SHIFT: u32 = 4;
/// `SSTAT1_FFO_MASK`.
pub const SSTAT1_FFO_MASK: u8 = 0x80;
/// `SSTAT1_SDPL`.
pub const SSTAT1_SDPL: u8 = 0x08;
/// `SSTAT1_MSG`.
pub const SSTAT1_MSG: u8 = 0x04;
/// `SSTAT1_CD`.
pub const SSTAT1_CD: u8 = 0x02;
/// `SSTAT1_IO`.
pub const SSTAT1_IO: u8 = 0x01;
/// `SSTAT1_PHASE_MASK`.
pub const SSTAT1_PHASE_MASK: u8 = SSTAT1_IO | SSTAT1_CD | SSTAT1_MSG;
/// `SSTAT1_PHASE_DATAOUT`.
pub const SSTAT1_PHASE_DATAOUT: u8 = 0;
/// `SSTAT1_PHASE_DATAIN`.
pub const SSTAT1_PHASE_DATAIN: u8 = SSTAT1_IO;
/// `SSTAT1_PHASE_CMD`.
pub const SSTAT1_PHASE_CMD: u8 = SSTAT1_CD;
/// `SSTAT1_PHASE_STATUS`.
pub const SSTAT1_PHASE_STATUS: u8 = SSTAT1_CD | SSTAT1_IO;
/// `SSTAT1_PHASE_MSGOUT`.
pub const SSTAT1_PHASE_MSGOUT: u8 = SSTAT1_MSG | SSTAT1_CD;
/// `SSTAT1_PHASE_MSGIN`.
pub const SSTAT1_PHASE_MSGIN: u8 = SSTAT1_MSG | SSTAT1_CD | SSTAT1_IO;
/// `SIOP_SSTAT2`: STSI status 2, RO.
pub const SIOP_SSTAT2: usize = 0x0F;
/// `SSTAT2_ILF1`: 875 only.
pub const SSTAT2_ILF1: u8 = 0x80;
/// `SSTAT2_ORF1`: 875 only.
pub const SSTAT2_ORF1: u8 = 0x40;
/// `SSTAT2_OLF1`: 875 only.
pub const SSTAT2_OLF1: u8 = 0x20;
/// `SSTAT2_FF4`: 875 only.
pub const SSTAT2_FF4: u8 = 0x10;
/// `SSTAT2_SPL1`: 875 only.
pub const SSTAT2_SPL1: u8 = 0x08;
/// `SSTAT2_DF`: 875 only.
pub const SSTAT2_DF: u8 = 0x04;
/// `SSTAT2_LDSC`.
pub const SSTAT2_LDSC: u8 = 0x02;
/// `SSTAT2_SDP1`: 875 only.
pub const SSTAT2_SDP1: u8 = 0x01;
/// `SIOP_DSA`: data struct addr, R/W.
pub const SIOP_DSA: usize = 0x10;
/// `SIOP_ISTAT`: IRQ status, R/W.
pub const SIOP_ISTAT: usize = 0x14;
/// `ISTAT_ABRT`.
pub const ISTAT_ABRT: u8 = 0x80;
/// `ISTAT_SRST`.
pub const ISTAT_SRST: u8 = 0x40;
/// `ISTAT_SIGP`.
pub const ISTAT_SIGP: u8 = 0x20;
/// `ISTAT_SEM`.
pub const ISTAT_SEM: u8 = 0x10;
/// `ISTAT_CON`.
pub const ISTAT_CON: u8 = 0x08;
/// `ISTAT_INTF`.
pub const ISTAT_INTF: u8 = 0x04;
/// `ISTAT_SIP`.
pub const ISTAT_SIP: u8 = 0x02;
/// `ISTAT_DIP`.
pub const ISTAT_DIP: u8 = 0x01;
/// `SIOP_CTEST0`: Chip test 0, R/W.
pub const SIOP_CTEST0: usize = 0x18;
/// `CTEST0_EHP`: 720/770.
pub const CTEST0_EHP: u8 = 0x04;
/// `SIOP_CTEST1`: Chip test 1, R/W.
pub const SIOP_CTEST1: usize = 0x19;
/// `SIOP_CTEST2`: Chip test 2, R/W.
pub const SIOP_CTEST2: usize = 0x1A;
/// `CTEST2_SRTCH`: 875 only.
pub const CTEST2_SRTCH: u8 = 0x04;
/// `SIOP_CTEST3`: Chip test 3, R/W.
pub const SIOP_CTEST3: usize = 0x1B;
/// `CTEST3_FLF`.
pub const CTEST3_FLF: u8 = 0x08;
/// `CTEST3_CLF`.
pub const CTEST3_CLF: u8 = 0x04;
/// `CTEST3_FM`.
pub const CTEST3_FM: u8 = 0x02;
/// `CTEST3_WRIE`.
pub const CTEST3_WRIE: u8 = 0x01;
/// `SIOP_TEMP`: Temp register (used by CALL/RET), R/W.
pub const SIOP_TEMP: usize = 0x1C;
/// `SIOP_DFIFO`: DMA FIFO.
pub const SIOP_DFIFO: usize = 0x20;
/// `SIOP_CTEST4`: Chip test 4, R/W.
pub const SIOP_CTEST4: usize = 0x21;
/// `CTEST4_MUX`: 720/770.
pub const CTEST4_MUX: u8 = 0x80;
/// `CTEST4_BDIS`.
pub const CTEST4_BDIS: u8 = 0x80;
/// `CTEST_ZMOD`.
pub const CTEST_ZMOD: u8 = 0x40;
/// `CTEST_ZSD`.
pub const CTEST_ZSD: u8 = 0x20;
/// `CTEST_SRTM`.
pub const CTEST_SRTM: u8 = 0x10;
/// `CTEST_MPEE`.
pub const CTEST_MPEE: u8 = 0x08;
/// `SIOP_CTEST5`: Chip test 5, R/W.
pub const SIOP_CTEST5: usize = 0x22;
/// `CTEST5_ADCK`.
pub const CTEST5_ADCK: u8 = 0x80;
/// `CTEST5_BBCK`.
pub const CTEST5_BBCK: u8 = 0x40;
/// `CTEST5_DFS`.
pub const CTEST5_DFS: u8 = 0x20;
/// `CTEST5_MASR`.
pub const CTEST5_MASR: u8 = 0x10;
/// `CTEST5_DDIR`.
pub const CTEST5_DDIR: u8 = 0x08;
/// `CTEST5_BOMASK`.
pub const CTEST5_BOMASK: u8 = 0x03;
/// `SIOP_CTEST6`: Chip test 6, R/W.
pub const SIOP_CTEST6: usize = 0x23;
/// `SIOP_DBC`: DMA byte counter, R/W.
pub const SIOP_DBC: usize = 0x24;
/// `SIOP_DCMD`: DMA command, R/W.
pub const SIOP_DCMD: usize = 0x27;
/// `SIOP_DNAD`: DMA next addr, R/W.
pub const SIOP_DNAD: usize = 0x28;
/// `SIOP_DSP`: DMA scripts pointer, R/W.
pub const SIOP_DSP: usize = 0x2C;
/// `SIOP_DSPS`: DMA scripts pointer save, R/W.
pub const SIOP_DSPS: usize = 0x30;
/// `SIOP_SCRATCHA`: scratch register A. R/W.
pub const SIOP_SCRATCHA: usize = 0x34;
/// `SIOP_DMODE`: DMA mode, R/W.
pub const SIOP_DMODE: usize = 0x38;
/// `DMODE_BL_SHIFT`.
pub const DMODE_BL_SHIFT: u32 = 6;
/// `DMODE_BL_MASK`.
pub const DMODE_BL_MASK: u8 = 0xC0;
/// `DMODE_SIOM`.
pub const DMODE_SIOM: u8 = 0x20;
/// `DMODE_DIOM`.
pub const DMODE_DIOM: u8 = 0x10;
/// `DMODE_ERL`.
pub const DMODE_ERL: u8 = 0x08;
/// `DMODE_ERMP`.
pub const DMODE_ERMP: u8 = 0x04;
/// `DMODE_BOF`.
pub const DMODE_BOF: u8 = 0x02;
/// `DMODE_MAN`.
pub const DMODE_MAN: u8 = 0x01;
/// `SIOP_DIEN`: DMA interrupt enable, R/W.
pub const SIOP_DIEN: usize = 0x39;
/// `DIEN_MDPE`.
pub const DIEN_MDPE: u8 = 0x40;
/// `DIEN_BF`.
pub const DIEN_BF: u8 = 0x20;
/// `DIEN_AVRT`.
pub const DIEN_AVRT: u8 = 0x10;
/// `DIEN_SSI`.
pub const DIEN_SSI: u8 = 0x08;
/// `DIEN_SIR`.
pub const DIEN_SIR: u8 = 0x04;
/// `DIEN_IID`.
pub const DIEN_IID: u8 = 0x01;
/// `SIOP_SBR`: scratch byte register, R/W.
pub const SIOP_SBR: usize = 0x3A;
/// `SIOP_DCNTL`: DMA control, R/W.
pub const SIOP_DCNTL: usize = 0x3B;
/// `DCNTL_CLSE`.
pub const DCNTL_CLSE: u8 = 0x80;
/// `DCNTL_PFF`.
pub const DCNTL_PFF: u8 = 0x40;
/// `DCNTL_EA`: 720/770.
pub const DCNTL_EA: u8 = 0x20;
/// `DCNTL_PFEN`: 8xx.
pub const DCNTL_PFEN: u8 = 0x20;
/// `DCNTL_SSM`.
pub const DCNTL_SSM: u8 = 0x10;
/// `DCNTL_IRQM`.
pub const DCNTL_IRQM: u8 = 0x08;
/// `DCNTL_STD`.
pub const DCNTL_STD: u8 = 0x04;
/// `DCNTL_IRQD`.
pub const DCNTL_IRQD: u8 = 0x02;
/// `DCNTL_COM`.
pub const DCNTL_COM: u8 = 0x01;
/// `SIOP_ADDER`: adder output sum, RO.
pub const SIOP_ADDER: usize = 0x3C;
/// `SIOP_SIEN0`: SCSI interrupt enable 0, R/W.
pub const SIOP_SIEN0: usize = 0x40;
/// `SIEN0_MA`.
pub const SIEN0_MA: u8 = 0x80;
/// `SIEN0_CMP`.
pub const SIEN0_CMP: u8 = 0x40;
/// `SIEN0_SEL`.
pub const SIEN0_SEL: u8 = 0x20;
/// `SIEN0_RSL`.
pub const SIEN0_RSL: u8 = 0x10;
/// `SIEN0_SGE`.
pub const SIEN0_SGE: u8 = 0x08;
/// `SIEN0_UDC`.
pub const SIEN0_UDC: u8 = 0x04;
/// `SIEN0_SRT`.
pub const SIEN0_SRT: u8 = 0x02;
/// `SIEN0_PAR`.
pub const SIEN0_PAR: u8 = 0x01;
/// `SIOP_SIEN1`: SCSI interrupt enable 1, R/W.
pub const SIOP_SIEN1: usize = 0x41;
/// `SIEN1_SBMC`: 895 only.
pub const SIEN1_SBMC: u8 = 0x10;
/// `SIEN1_STO`.
pub const SIEN1_STO: u8 = 0x04;
/// `SIEN1_GEN`.
pub const SIEN1_GEN: u8 = 0x02;
/// `SIEN1_HTH`.
pub const SIEN1_HTH: u8 = 0x01;
/// `SIOP_SIST0`: SCSI interrupt status 0, RO.
pub const SIOP_SIST0: usize = 0x42;
/// `SIST0_MA`.
pub const SIST0_MA: u8 = 0x80;
/// `SIST0_CMP`.
pub const SIST0_CMP: u8 = 0x40;
/// `SIST0_SEL`.
pub const SIST0_SEL: u8 = 0x20;
/// `SIST0_RSL`.
pub const SIST0_RSL: u8 = 0x10;
/// `SIST0_SGE`.
pub const SIST0_SGE: u8 = 0x08;
/// `SIST0_UDC`.
pub const SIST0_UDC: u8 = 0x04;
/// `SIST0_RST`.
pub const SIST0_RST: u8 = 0x02;
/// `SIST0_PAR`.
pub const SIST0_PAR: u8 = 0x01;
/// `SIOP_SIST1`: SCSI interrupt status 1, RO.
pub const SIOP_SIST1: usize = 0x43;
/// `SIST1_SBMC`: 895 only.
pub const SIST1_SBMC: u8 = 0x10;
/// `SIST1_STO`.
pub const SIST1_STO: u8 = 0x04;
/// `SIST1_GEN`.
pub const SIST1_GEN: u8 = 0x02;
/// `SIST1_HTH`.
pub const SIST1_HTH: u8 = 0x01;
/// `SIOP_SLPAR`: scsi longitudinal parity, R/W.
pub const SIOP_SLPAR: usize = 0x44;
/// `SIOP_SWIDE`: scsi wide residue, RW, 875 only.
pub const SIOP_SWIDE: usize = 0x45;
/// `SIOP_MACNTL`: memory access control, R/W.
pub const SIOP_MACNTL: usize = 0x46;
/// `SIOP_GPCNTL`: General Purpose Pin control, R/W.
pub const SIOP_GPCNTL: usize = 0x47;
/// `GPCNTL_ME`: 875 only.
pub const GPCNTL_ME: u8 = 0x80;
/// `GPCNTL_FE`: 875 only.
pub const GPCNTL_FE: u8 = 0x40;
/// `GPCNTL_IN4`: 875 only.
pub const GPCNTL_IN4: u8 = 0x10;
/// `GPCNTL_IN3`: 875 only.
pub const GPCNTL_IN3: u8 = 0x08;
/// `GPCNTL_IN2`: 875 only.
pub const GPCNTL_IN2: u8 = 0x04;
/// `GPCNTL_IN1`.
pub const GPCNTL_IN1: u8 = 0x02;
/// `GPCNTL_IN0`.
pub const GPCNTL_IN0: u8 = 0x01;
/// `SIOP_STIME0`: SCSI timer 0, R/W.
pub const SIOP_STIME0: usize = 0x48;
/// `STIME0_HTH_SHIFT`.
pub const STIME0_HTH_SHIFT: u32 = 4;
/// `STIME0_HTH_MASK`.
pub const STIME0_HTH_MASK: u8 = 0xf0;
/// `STIME0_SEL_SHIFT`.
pub const STIME0_SEL_SHIFT: u32 = 0;
/// `STIME0_SEL_MASK`.
pub const STIME0_SEL_MASK: u8 = 0x0f;
/// `SIOP_STIME1`: SCSI timer 1, R/W.
pub const SIOP_STIME1: usize = 0x49;
/// `STIME1_HTHBA`: 875 only.
pub const STIME1_HTHBA: u8 = 0x40;
/// `STIME1_GENSF`: 875 only.
pub const STIME1_GENSF: u8 = 0x20;
/// `STIME1_HTHSF`: 875 only.
pub const STIME1_HTHSF: u8 = 0x10;
/// `STIME1_GEN_SHIFT`.
pub const STIME1_GEN_SHIFT: u32 = 0;
/// `STIME1_GEN_MASK`.
pub const STIME1_GEN_MASK: u8 = 0x0f;
/// `SIOP_RESPID0`: response ID, R/W.
pub const SIOP_RESPID0: usize = 0x4A;
/// `SIOP_RESPID1`: response ID, R/W, 875-only.
pub const SIOP_RESPID1: usize = 0x4B;
/// `SIOP_STEST0`: SCSI test 0, RO.
pub const SIOP_STEST0: usize = 0x4C;
/// `SIOP_STEST1`: SCSI test 1, RO, RW on 875.
pub const SIOP_STEST1: usize = 0x4D;
/// `STEST1_DOGE`: 1010 only.
pub const STEST1_DOGE: u8 = 0x20;
/// `STEST1_DIGE`: 1010 only.
pub const STEST1_DIGE: u8 = 0x10;
/// `STEST1_DBLEN`: 875-only.
pub const STEST1_DBLEN: u8 = 0x08;
/// `STEST1_DBLSEL`: 875-only.
pub const STEST1_DBLSEL: u8 = 0x04;
/// `SIOP_STEST2`: SCSI test 2, RO, R/W on 875.
pub const SIOP_STEST2: usize = 0x4E;
/// `STEST2_DIF`: 875 only.
pub const STEST2_DIF: u8 = 0x20;
/// `STEST2_EXT`.
pub const STEST2_EXT: u8 = 0x02;
/// `SIOP_STEST3`: SCSI test 3, RO, RW on 875.
pub const SIOP_STEST3: usize = 0x4F;
/// `STEST3_TE`.
pub const STEST3_TE: u8 = 0x80;
/// `STEST3_HSC`.
pub const STEST3_HSC: u8 = 0x20;
/// `SIOP_STEST4`: SCSI test 4, 895 only.
pub const SIOP_STEST4: usize = 0x52;
/// `STEST4_MODE_MASK`.
pub const STEST4_MODE_MASK: u8 = 0xc0;
/// `STEST4_MODE_DIF`.
pub const STEST4_MODE_DIF: u8 = 0x40;
/// `STEST4_MODE_SE`.
pub const STEST4_MODE_SE: u8 = 0x80;
/// `STEST4_MODE_LVD`.
pub const STEST4_MODE_LVD: u8 = 0xc0;
/// `STEST4_LOCK`.
pub const STEST4_LOCK: u8 = 0x20;
/// `SIOP_SIDL`: SCSI input data latch, RO.
pub const SIOP_SIDL: usize = 0x50;
/// `SIOP_SODL`: SCSI output data latch, R/W.
pub const SIOP_SODL: usize = 0x54;
/// `SIOP_SBDL`: SCSI bus data lines, RO.
pub const SIOP_SBDL: usize = 0x58;
/// `SIOP_SCRATCHB`: Scratch register B, R/W.
pub const SIOP_SCRATCHB: usize = 0x5C;
/// `SIOP_SCRATCHC`: Scratch register C, R/W, 875 only.
pub const SIOP_SCRATCHC: usize = 0x60;
/// `SIOP_SCRATCHD`: Scratch register D, R/W, 875-only.
pub const SIOP_SCRATCHD: usize = 0x64;
/// `SIOP_SCRATCHE`: Scratch register E, R/W, 875-only.
pub const SIOP_SCRATCHE: usize = 0x68;
/// `SIOP_SCRATCHF`: Scratch register F, R/W, 875-only.
pub const SIOP_SCRATCHF: usize = 0x6c;
/// `SIOP_SCRATCHG`: Scratch register G, R/W, 875-only.
pub const SIOP_SCRATCHG: usize = 0x70;
/// `SIOP_SCRATCHH`: Scratch register H, R/W, 875-only.
pub const SIOP_SCRATCHH: usize = 0x74;
/// `SIOP_SCRATCHI`: Scratch register I, R/W, 875-only.
pub const SIOP_SCRATCHI: usize = 0x78;
/// `SIOP_SCRATCHJ`: Scratch register J, R/W, 875-only.
pub const SIOP_SCRATCHJ: usize = 0x7c;
/// `SIOP_SCNTL4`: SCSI control 4, R/W, 1010-only.
pub const SIOP_SCNTL4: usize = 0xBC;
/// `SCNTL4_XCLKS_ST`.
pub const SCNTL4_XCLKS_ST: u8 = 0x01;
/// `SCNTL4_XCLKS_DT`.
pub const SCNTL4_XCLKS_DT: u8 = 0x02;
/// `SCNTL4_XCLKH_ST`.
pub const SCNTL4_XCLKH_ST: u8 = 0x04;
/// `SCNTL4_XCLKH_DT`.
pub const SCNTL4_XCLKH_DT: u8 = 0x08;
/// `SCNTL4_AIPEN`.
pub const SCNTL4_AIPEN: u8 = 0x40;
/// `SCNTL4_U3EN`.
pub const SCNTL4_U3EN: u8 = 0x80;
/// `SIOP_DFBC`: DMA fifo byte count, RO.
pub const SIOP_DFBC: usize = 0xf0;
/// `SIOP_AIPCNTL0`: AIP Control 0, 1010-only.
pub const SIOP_AIPCNTL0: usize = 0xbe;
/// `AIPCNTL0_ERRLIVE`: AIP error status, live.
pub const AIPCNTL0_ERRLIVE: u8 = 0x04;
/// `AIPCNTL0_ERR`: AIP error status, latched.
pub const AIPCNTL0_ERR: u8 = 0x02;
/// `AIPCNTL0_PARITYERRs`: Parity error.
#[allow(non_upper_case_globals)] // the C name
pub const AIPCNTL0_PARITYERRs: u8 = 0x01;
/// `SIOP_AIPCNTL1`: AIP Control 1, 1010-only.
pub const SIOP_AIPCNTL1: usize = 0xbf;
/// `AIPCNTL1_DIS`: disable AIP generation, 1010-66 only.
pub const AIPCNTL1_DIS: u8 = 0x08;
/// `AIPCNTL1_RSETERR`: reset AIP error 1010-66 only.
pub const AIPCNTL1_RSETERR: u8 = 0x04;
/// `AIPCNTL1_FB`: force bad AIP value 1010-66 only.
pub const AIPCNTL1_FB: u8 = 0x02;
/// `AIPCNTL1_RSET`: reset AIP sequence value 1010-66 only.
pub const AIPCNTL1_RSET: u8 = 0x01;
/// `SIOP_NVRAM_SYM_SIZE`.
pub const SIOP_NVRAM_SYM_SIZE: usize = 368;
/// `SIOP_NVRAM_SYM_ADDRESS`.
pub const SIOP_NVRAM_SYM_ADDRESS: usize = 0x100;
/// `NVRAM_SYM_F_SCAM_ENABLE`.
pub const NVRAM_SYM_F_SCAM_ENABLE: u16 = 0x0001;
/// `NVRAM_SYM_F_PARITY_ENABLE`.
pub const NVRAM_SYM_F_PARITY_ENABLE: u16 = 0x0002;
/// `NVRAM_SYM_F_VERBOSE_MESSAGES`.
pub const NVRAM_SYM_F_VERBOSE_MESSAGES: u16 = 0x0004;
/// `NVRAM_SYM_F_CHS_MAPPING`.
pub const NVRAM_SYM_F_CHS_MAPPING: u16 = 0x0008;
/// `NVRAM_SYM_F1_SCAN_HI_LO`.
pub const NVRAM_SYM_F1_SCAN_HI_LO: u16 = 0x0001;
/// `NVRAM_SYM_TERM_CANT_PROGRAM`.
pub const NVRAM_SYM_TERM_CANT_PROGRAM: u16 = 0;
/// `NVRAM_SYM_TERM_ENABLED`.
pub const NVRAM_SYM_TERM_ENABLED: u16 = 1;
/// `NVRAM_SYM_TERM_DISABLED`.
pub const NVRAM_SYM_TERM_DISABLED: u16 = 2;
/// `NVRAM_SYM_RMVBL_NO_SUPPORT`.
pub const NVRAM_SYM_RMVBL_NO_SUPPORT: u16 = 0;
/// `NVRAM_SYM_RMVBL_BOOT_DEVICE`.
pub const NVRAM_SYM_RMVBL_BOOT_DEVICE: u16 = 1;
/// `NVRAM_SYM_RMVBL_MEDIA_INSTALLED`.
pub const NVRAM_SYM_RMVBL_MEDIA_INSTALLED: u16 = 2;
/// `NVRAM_SYM_HOST_F_SCAN_AT_BOOT`.
pub const NVRAM_SYM_HOST_F_SCAN_AT_BOOT: u16 = 0x0001;
/// `NVRAM_SYM_TARG_F_DISCONNECT_EN`.
pub const NVRAM_SYM_TARG_F_DISCONNECT_EN: u8 = 0x0001;
/// `NVRAM_SYM_TARG_F_SCAN_AT_BOOT`.
pub const NVRAM_SYM_TARG_F_SCAN_AT_BOOT: u8 = 0x0002;
/// `NVRAM_SYM_TARG_F_SCAN_LUNS`.
pub const NVRAM_SYM_TARG_F_SCAN_LUNS: u8 = 0x0004;
/// `NVRAM_SYM_TARG_F_TQ_EN`.
pub const NVRAM_SYM_TARG_F_TQ_EN: u8 = 0x0008;
/// `NVRAM_SYM_SCAM_DEFAULT_METHOD`.
pub const NVRAM_SYM_SCAM_DEFAULT_METHOD: u16 = 0;
/// `NVRAM_SYM_SCAM_DONT_ASSIGN`.
pub const NVRAM_SYM_SCAM_DONT_ASSIGN: u16 = 1;
/// `NVRAM_SYM_SCAM_SET_SPECIFIC_ID`.
pub const NVRAM_SYM_SCAM_SET_SPECIFIC_ID: u16 = 2;
/// `NVRAM_SYM_SCAM_USE_ORDER_GIVEN`.
pub const NVRAM_SYM_SCAM_USE_ORDER_GIVEN: u16 = 3;
/// `NVRAM_SYM_SCAM_UNKNOWN`.
pub const NVRAM_SYM_SCAM_UNKNOWN: u16 = 0;
/// `NVRAM_SYM_SCAM_DEVICE_NOT_FOUND`.
pub const NVRAM_SYM_SCAM_DEVICE_NOT_FOUND: u16 = 1;
/// `NVRAM_SYM_SCAM_ID_NOT_SET`.
pub const NVRAM_SYM_SCAM_ID_NOT_SET: u16 = 2;
/// `NVRAM_SYM_SCAM_ID_VALID`.
pub const NVRAM_SYM_SCAM_ID_VALID: u16 = 3;
/// `SIOP_NVRAM_TEK_SIZE`.
pub const SIOP_NVRAM_TEK_SIZE: usize = 64;
/// `SIOP_NVRAM_TEK_93c46_ADDRESS`.
#[allow(non_upper_case_globals)] // the C name
pub const SIOP_NVRAM_TEK_93c46_ADDRESS: usize = 0;
/// `SIOP_NVRAM_TEK_24c16_ADDRESS`.
#[allow(non_upper_case_globals)] // the C name
pub const SIOP_NVRAM_TEK_24c16_ADDRESS: usize = 0x40;

/// `struct scf_period`: periods for various SCF values, assume transfer period of 4.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScfPeriod {
    /// `clock`: clock period (ns * 10).
    pub clock: i32,
    /// `period`: scsi period, as set in the SDTR message.
    pub period: i32,
    /// `scf`: scf value to use.
    pub scf: i32,
}

/// `struct nvram_symbios_host`: one entry of the boot order.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default)]
pub struct NvramSymbiosHost {
    /// `type`: 4 - 8xx.
    pub type_: u16,
    /// `device_id`: PCI device ID.
    pub device_id: u16,
    /// `vendor_id`: PCI vendor ID.
    pub vendor_id: u16,
    /// `bus_nr`: PCI bus number.
    pub bus_nr: u8,
    /// `device_fn`: PCI device/func # << 3.
    pub device_fn: u8,
    /// `word8`.
    pub word8: u16,
    /// `flags`: `NVRAM_SYM_HOST_F_*`.
    pub flags: u16,
    /// `io_port`: PCI I/O address.
    pub io_port: u16,
}

/// `struct nvram_symbios_target`: one target's settings.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default)]
pub struct NvramSymbiosTarget {
    /// `flags`: `NVRAM_SYM_TARG_F_*`.
    pub flags: u8,
    /// `rsvd`.
    pub rsvd: u8,
    /// `bus_width`.
    pub bus_width: u8,
    /// `sync_offset`: 8, 16, etc.
    pub sync_offset: u8,
    /// `sync_period`: 4 * factor.
    pub sync_period: u16,
    /// `timeout`.
    pub timeout: u16,
}

/// `struct nvram_symbios_scam`: one entry of the SCAM table.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default)]
pub struct NvramSymbiosScam {
    /// `id`.
    pub id: u16,
    /// `method`: `NVRAM_SYM_SCAM_*_METHOD` and friends.
    pub method: u16,
    /// `status`: `NVRAM_SYM_SCAM_UNKNOWN` and friends.
    pub status: u16,
    /// `target_id`.
    pub target_id: u8,
    /// `rsvd`.
    pub rsvd: u8,
}

/// `struct nvram_symbios`: the non-volatile configuration settings stored in the EEPROM, in
/// the Symbios Logic format.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct NvramSymbios {
    /* Header (6 bytes) */
    /// `type`: 0x0000.
    pub type_: u16,
    /// `byte_count`: excluding header/trailer.
    pub byte_count: u16,
    /// `checksum`.
    pub checksum: u16,

    /* Adapter configuration (20 bytes) */
    /// `v_major`.
    pub v_major: u8,
    /// `v_minor`.
    pub v_minor: u8,
    /// `boot_crc`.
    pub boot_crc: u32,
    /// `flags`: `NVRAM_SYM_F_*`.
    pub flags: u16,
    /// `flags1`: `NVRAM_SYM_F1_*`.
    pub flags1: u16,
    /// `term_state`: `NVRAM_SYM_TERM_*`.
    pub term_state: u16,
    /// `rmvbl_flags`: `NVRAM_SYM_RMVBL_*`.
    pub rmvbl_flags: u16,
    /// `host_id`.
    pub host_id: u8,
    /// `num_hba`.
    pub num_hba: u8,
    /// `num_devices`.
    pub num_devices: u8,
    /// `max_scam_devices`.
    pub max_scam_devices: u8,
    /// `num_valid_scam_devices`.
    pub num_valid_scam_devices: u8,
    /// `rsvd`.
    pub rsvd: u8,

    /// `host`: boot order (14 bytes x 4).
    pub host: [NvramSymbiosHost; 4],

    /// `target`: targets (8 bytes x 16).
    pub target: [NvramSymbiosTarget; 16],

    /// `scam`: SCAM table (8 bytes x 4).
    pub scam: [NvramSymbiosScam; 4],

    /// `spare_devices`.
    pub spare_devices: [u8; 15 * 8],
    /// `trailer`: 0xfe 0xfe 0x00 0x00 0x00 0x00.
    pub trailer: [u8; 6],
}

/// `scf_period[]`: the SCF value of each synchronous period, per clock.
pub static SCF_PERIOD: [ScfPeriod; 13] = [
    scf(250, 25, 1), /* 10.0 MHz */
    scf(250, 37, 2), /* 6.67 MHz */
    scf(250, 50, 3), /* 5.00 MHz */
    scf(250, 75, 4), /* 3.33 MHz */
    scf(125, 12, 1), /* 20.0 MHz */
    scf(125, 18, 2), /* 13.3 MHz */
    scf(125, 25, 3), /* 10.0 MHz */
    scf(125, 37, 4), /* 6.67 MHz */
    scf(125, 50, 5), /* 5.0 MHz */
    scf(62, 10, 1),  /* 40.0 MHz */
    scf(62, 12, 3),  /* 20.0 MHz */
    scf(62, 18, 4),  /* 13.3 MHz */
    scf(62, 25, 5),  /* 10.0 MHz */
];

/// `dt_scf_period[]`: the same for DT (double transition) clocking.
pub static DT_SCF_PERIOD: [ScfPeriod; 5] = [
    scf(62, 9, 1),  /* 80.0 MHz */
    scf(62, 10, 3), /* 40.0 MHz */
    scf(62, 12, 5), /* 20.0 MHz */
    scf(62, 18, 6), /* 13.3 MHz */
    scf(62, 25, 7), /* 10.0 MHz */
];

/// One `{clock, period, scf}` initialiser of the tables.
const fn scf(clock: i32, period: i32, scf: i32) -> ScfPeriod {
    ScfPeriod { clock, period, scf }
}

const _: () = {
    assert!(size_of::<NvramSymbiosHost>() == 14);
    assert!(size_of::<NvramSymbiosTarget>() == 8);
    assert!(size_of::<NvramSymbiosScam>() == 8);
    assert!(size_of::<NvramSymbios>() == SIOP_NVRAM_SYM_SIZE);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for `siopreg.rs`: the period tables and the NVRAM layout, and a reference
    // test of every define.

    use super::*;
    use crate::reftest;

    #[test]
    fn period_tables_cover_the_three_clocks() {
        for clock in [250, 125, 62] {
            assert!(SCF_PERIOD.iter().any(|p| p.clock == clock));
        }
        assert!(DT_SCF_PERIOD.iter().all(|p| p.clock == 62));
        // The 53c895A's clock (62) reaches 40 MHz (period 10) single-transition.
        assert_eq!(
            SCF_PERIOD[9],
            ScfPeriod {
                clock: 62,
                period: 10,
                scf: 1
            }
        );
        assert_eq!(core::mem::offset_of!(NvramSymbios, host), 26);
        assert_eq!(core::mem::offset_of!(NvramSymbios, target), 26 + 56);
        assert_eq!(core::mem::offset_of!(NvramSymbios, trailer), 362);
    }

    #[test]
    #[ignore = "reads the C reference (just test-ref)"]
    fn defines_match_the_reference() {
        let defs = reftest::defines("sys/dev/ic/siopreg.h");
        #[allow(clippy::unnecessary_cast)] // a column of mixed types
        let ours: &[(&str, i64)] = &[
            ("SIOP_SCNTL0", SIOP_SCNTL0 as i64),
            ("SCNTL0_ARB_MASK", SCNTL0_ARB_MASK as i64),
            ("SCNTL0_SARB", SCNTL0_SARB as i64),
            ("SCNTL0_FARB", SCNTL0_FARB as i64),
            ("SCNTL0_START", SCNTL0_START as i64),
            ("SCNTL0_WATM", SCNTL0_WATM as i64),
            ("SCNTL0_EPC", SCNTL0_EPC as i64),
            ("SCNTL0_AAP", SCNTL0_AAP as i64),
            ("SCNTL0_TRG", SCNTL0_TRG as i64),
            ("SIOP_SCNTL1", SIOP_SCNTL1 as i64),
            ("SCNTL1_EXC", SCNTL1_EXC as i64),
            ("SCNTL1_ADB", SCNTL1_ADB as i64),
            ("SCNTL1_DHP", SCNTL1_DHP as i64),
            ("SCNTL1_CON", SCNTL1_CON as i64),
            ("SCNTL1_RST", SCNTL1_RST as i64),
            ("SCNTL1_AESP", SCNTL1_AESP as i64),
            ("SCNTL1_IARB", SCNTL1_IARB as i64),
            ("SCNTL1_SST", SCNTL1_SST as i64),
            ("SIOP_SCNTL2", SIOP_SCNTL2 as i64),
            ("SCNTL2_SDU", SCNTL2_SDU as i64),
            ("SCNTL2_CHM", SCNTL2_CHM as i64),
            ("SCNTL2_SLPMD", SCNTL2_SLPMD as i64),
            ("SCNTL2_SLPHBEN", SCNTL2_SLPHBEN as i64),
            ("SCNTL2_WSS", SCNTL2_WSS as i64),
            ("SCNTL2_VUE0", SCNTL2_VUE0 as i64),
            ("SCNTL2_VUE1", SCNTL2_VUE1 as i64),
            ("SCNTL2_WSR", SCNTL2_WSR as i64),
            ("SIOP_SCNTL3", SIOP_SCNTL3 as i64),
            ("SCNTL3_ULTRA", SCNTL3_ULTRA as i64),
            ("SCNTL3_SCF_SHIFT", SCNTL3_SCF_SHIFT as i64),
            ("SCNTL3_SCF_MASK", SCNTL3_SCF_MASK as i64),
            ("SCNTL3_EWS", SCNTL3_EWS as i64),
            ("SCNTL3_CCF_SHIFT", SCNTL3_CCF_SHIFT as i64),
            ("SCNTL3_CCF_MASK", SCNTL3_CCF_MASK as i64),
            ("SIOP_SCID", SIOP_SCID as i64),
            ("SCID_RRE", SCID_RRE as i64),
            ("SCID_SRE", SCID_SRE as i64),
            ("SCID_ENCID_SHIFT", SCID_ENCID_SHIFT as i64),
            ("SCID_ENCID_MASK", SCID_ENCID_MASK as i64),
            ("SIOP_SXFER", SIOP_SXFER as i64),
            ("SXFER_TP_SHIFT", SXFER_TP_SHIFT as i64),
            ("SXFER_TP_MASK", SXFER_TP_MASK as i64),
            ("SXFER_MO_SHIFT", SXFER_MO_SHIFT as i64),
            ("SXFER_MO_MASK", SXFER_MO_MASK as i64),
            ("SIOP_SDID", SIOP_SDID as i64),
            ("SDID_ENCID_SHIFT", SDID_ENCID_SHIFT as i64),
            ("SDID_ENCID_MASK", SDID_ENCID_MASK as i64),
            ("SIOP_GPREG", SIOP_GPREG as i64),
            ("GPREG_GPIO4", GPREG_GPIO4 as i64),
            ("GPREG_GPIO3", GPREG_GPIO3 as i64),
            ("GPREG_GPIO2", GPREG_GPIO2 as i64),
            ("GPREG_GPIO1", GPREG_GPIO1 as i64),
            ("GPREG_GPIO0", GPREG_GPIO0 as i64),
            ("SIOP_SFBR", SIOP_SFBR as i64),
            ("SIOP_SOCL", SIOP_SOCL as i64),
            ("SIOP_SSID", SIOP_SSID as i64),
            ("SSID_VAL", SSID_VAL as i64),
            ("SSID_ENCID_SHIFT", SSID_ENCID_SHIFT as i64),
            ("SSID_ENCID_MASK", SSID_ENCID_MASK as i64),
            ("SIOP_SBCL", SIOP_SBCL as i64),
            ("SIOP_DSTAT", SIOP_DSTAT as i64),
            ("DSTAT_DFE", DSTAT_DFE as i64),
            ("DSTAT_MDPE", DSTAT_MDPE as i64),
            ("DSTAT_BF", DSTAT_BF as i64),
            ("DSTAT_ABRT", DSTAT_ABRT as i64),
            ("DSTAT_SSI", DSTAT_SSI as i64),
            ("DSTAT_SIR", DSTAT_SIR as i64),
            ("DSTAT_IID", DSTAT_IID as i64),
            ("SIOP_SSTAT0", SIOP_SSTAT0 as i64),
            ("SSTAT0_ILF", SSTAT0_ILF as i64),
            ("SSTAT0_ORF", SSTAT0_ORF as i64),
            ("SSTAT0_OLF", SSTAT0_OLF as i64),
            ("SSTAT0_AIP", SSTAT0_AIP as i64),
            ("SSTAT0_LOA", SSTAT0_LOA as i64),
            ("SSTAT0_WOA", SSTAT0_WOA as i64),
            ("SSTAT0_RST", SSTAT0_RST as i64),
            ("SSTAT0_SDP", SSTAT0_SDP as i64),
            ("SIOP_SSTAT1", SIOP_SSTAT1 as i64),
            ("SSTAT1_FFO_SHIFT", SSTAT1_FFO_SHIFT as i64),
            ("SSTAT1_FFO_MASK", SSTAT1_FFO_MASK as i64),
            ("SSTAT1_SDPL", SSTAT1_SDPL as i64),
            ("SSTAT1_MSG", SSTAT1_MSG as i64),
            ("SSTAT1_CD", SSTAT1_CD as i64),
            ("SSTAT1_IO", SSTAT1_IO as i64),
            ("SSTAT1_PHASE_MASK", SSTAT1_PHASE_MASK as i64),
            ("SSTAT1_PHASE_DATAOUT", SSTAT1_PHASE_DATAOUT as i64),
            ("SSTAT1_PHASE_DATAIN", SSTAT1_PHASE_DATAIN as i64),
            ("SSTAT1_PHASE_CMD", SSTAT1_PHASE_CMD as i64),
            ("SSTAT1_PHASE_STATUS", SSTAT1_PHASE_STATUS as i64),
            ("SSTAT1_PHASE_MSGOUT", SSTAT1_PHASE_MSGOUT as i64),
            ("SSTAT1_PHASE_MSGIN", SSTAT1_PHASE_MSGIN as i64),
            ("SIOP_SSTAT2", SIOP_SSTAT2 as i64),
            ("SSTAT2_ILF1", SSTAT2_ILF1 as i64),
            ("SSTAT2_ORF1", SSTAT2_ORF1 as i64),
            ("SSTAT2_OLF1", SSTAT2_OLF1 as i64),
            ("SSTAT2_FF4", SSTAT2_FF4 as i64),
            ("SSTAT2_SPL1", SSTAT2_SPL1 as i64),
            ("SSTAT2_DF", SSTAT2_DF as i64),
            ("SSTAT2_LDSC", SSTAT2_LDSC as i64),
            ("SSTAT2_SDP1", SSTAT2_SDP1 as i64),
            ("SIOP_DSA", SIOP_DSA as i64),
            ("SIOP_ISTAT", SIOP_ISTAT as i64),
            ("ISTAT_ABRT", ISTAT_ABRT as i64),
            ("ISTAT_SRST", ISTAT_SRST as i64),
            ("ISTAT_SIGP", ISTAT_SIGP as i64),
            ("ISTAT_SEM", ISTAT_SEM as i64),
            ("ISTAT_CON", ISTAT_CON as i64),
            ("ISTAT_INTF", ISTAT_INTF as i64),
            ("ISTAT_SIP", ISTAT_SIP as i64),
            ("ISTAT_DIP", ISTAT_DIP as i64),
            ("SIOP_CTEST0", SIOP_CTEST0 as i64),
            ("CTEST0_EHP", CTEST0_EHP as i64),
            ("SIOP_CTEST1", SIOP_CTEST1 as i64),
            ("SIOP_CTEST2", SIOP_CTEST2 as i64),
            ("CTEST2_SRTCH", CTEST2_SRTCH as i64),
            ("SIOP_CTEST3", SIOP_CTEST3 as i64),
            ("CTEST3_FLF", CTEST3_FLF as i64),
            ("CTEST3_CLF", CTEST3_CLF as i64),
            ("CTEST3_FM", CTEST3_FM as i64),
            ("CTEST3_WRIE", CTEST3_WRIE as i64),
            ("SIOP_TEMP", SIOP_TEMP as i64),
            ("SIOP_DFIFO", SIOP_DFIFO as i64),
            ("SIOP_CTEST4", SIOP_CTEST4 as i64),
            ("CTEST4_MUX", CTEST4_MUX as i64),
            ("CTEST4_BDIS", CTEST4_BDIS as i64),
            ("CTEST_ZMOD", CTEST_ZMOD as i64),
            ("CTEST_ZSD", CTEST_ZSD as i64),
            ("CTEST_SRTM", CTEST_SRTM as i64),
            ("CTEST_MPEE", CTEST_MPEE as i64),
            ("SIOP_CTEST5", SIOP_CTEST5 as i64),
            ("CTEST5_ADCK", CTEST5_ADCK as i64),
            ("CTEST5_BBCK", CTEST5_BBCK as i64),
            ("CTEST5_DFS", CTEST5_DFS as i64),
            ("CTEST5_MASR", CTEST5_MASR as i64),
            ("CTEST5_DDIR", CTEST5_DDIR as i64),
            ("CTEST5_BOMASK", CTEST5_BOMASK as i64),
            ("SIOP_CTEST6", SIOP_CTEST6 as i64),
            ("SIOP_DBC", SIOP_DBC as i64),
            ("SIOP_DCMD", SIOP_DCMD as i64),
            ("SIOP_DNAD", SIOP_DNAD as i64),
            ("SIOP_DSP", SIOP_DSP as i64),
            ("SIOP_DSPS", SIOP_DSPS as i64),
            ("SIOP_SCRATCHA", SIOP_SCRATCHA as i64),
            ("SIOP_DMODE", SIOP_DMODE as i64),
            ("DMODE_BL_SHIFT", DMODE_BL_SHIFT as i64),
            ("DMODE_BL_MASK", DMODE_BL_MASK as i64),
            ("DMODE_SIOM", DMODE_SIOM as i64),
            ("DMODE_DIOM", DMODE_DIOM as i64),
            ("DMODE_ERL", DMODE_ERL as i64),
            ("DMODE_ERMP", DMODE_ERMP as i64),
            ("DMODE_BOF", DMODE_BOF as i64),
            ("DMODE_MAN", DMODE_MAN as i64),
            ("SIOP_DIEN", SIOP_DIEN as i64),
            ("DIEN_MDPE", DIEN_MDPE as i64),
            ("DIEN_BF", DIEN_BF as i64),
            ("DIEN_AVRT", DIEN_AVRT as i64),
            ("DIEN_SSI", DIEN_SSI as i64),
            ("DIEN_SIR", DIEN_SIR as i64),
            ("DIEN_IID", DIEN_IID as i64),
            ("SIOP_SBR", SIOP_SBR as i64),
            ("SIOP_DCNTL", SIOP_DCNTL as i64),
            ("DCNTL_CLSE", DCNTL_CLSE as i64),
            ("DCNTL_PFF", DCNTL_PFF as i64),
            ("DCNTL_EA", DCNTL_EA as i64),
            ("DCNTL_PFEN", DCNTL_PFEN as i64),
            ("DCNTL_SSM", DCNTL_SSM as i64),
            ("DCNTL_IRQM", DCNTL_IRQM as i64),
            ("DCNTL_STD", DCNTL_STD as i64),
            ("DCNTL_IRQD", DCNTL_IRQD as i64),
            ("DCNTL_COM", DCNTL_COM as i64),
            ("SIOP_ADDER", SIOP_ADDER as i64),
            ("SIOP_SIEN0", SIOP_SIEN0 as i64),
            ("SIEN0_MA", SIEN0_MA as i64),
            ("SIEN0_CMP", SIEN0_CMP as i64),
            ("SIEN0_SEL", SIEN0_SEL as i64),
            ("SIEN0_RSL", SIEN0_RSL as i64),
            ("SIEN0_SGE", SIEN0_SGE as i64),
            ("SIEN0_UDC", SIEN0_UDC as i64),
            ("SIEN0_SRT", SIEN0_SRT as i64),
            ("SIEN0_PAR", SIEN0_PAR as i64),
            ("SIOP_SIEN1", SIOP_SIEN1 as i64),
            ("SIEN1_SBMC", SIEN1_SBMC as i64),
            ("SIEN1_STO", SIEN1_STO as i64),
            ("SIEN1_GEN", SIEN1_GEN as i64),
            ("SIEN1_HTH", SIEN1_HTH as i64),
            ("SIOP_SIST0", SIOP_SIST0 as i64),
            ("SIST0_MA", SIST0_MA as i64),
            ("SIST0_CMP", SIST0_CMP as i64),
            ("SIST0_SEL", SIST0_SEL as i64),
            ("SIST0_RSL", SIST0_RSL as i64),
            ("SIST0_SGE", SIST0_SGE as i64),
            ("SIST0_UDC", SIST0_UDC as i64),
            ("SIST0_RST", SIST0_RST as i64),
            ("SIST0_PAR", SIST0_PAR as i64),
            ("SIOP_SIST1", SIOP_SIST1 as i64),
            ("SIST1_SBMC", SIST1_SBMC as i64),
            ("SIST1_STO", SIST1_STO as i64),
            ("SIST1_GEN", SIST1_GEN as i64),
            ("SIST1_HTH", SIST1_HTH as i64),
            ("SIOP_SLPAR", SIOP_SLPAR as i64),
            ("SIOP_SWIDE", SIOP_SWIDE as i64),
            ("SIOP_MACNTL", SIOP_MACNTL as i64),
            ("SIOP_GPCNTL", SIOP_GPCNTL as i64),
            ("GPCNTL_ME", GPCNTL_ME as i64),
            ("GPCNTL_FE", GPCNTL_FE as i64),
            ("GPCNTL_IN4", GPCNTL_IN4 as i64),
            ("GPCNTL_IN3", GPCNTL_IN3 as i64),
            ("GPCNTL_IN2", GPCNTL_IN2 as i64),
            ("GPCNTL_IN1", GPCNTL_IN1 as i64),
            ("GPCNTL_IN0", GPCNTL_IN0 as i64),
            ("SIOP_STIME0", SIOP_STIME0 as i64),
            ("STIME0_HTH_SHIFT", STIME0_HTH_SHIFT as i64),
            ("STIME0_HTH_MASK", STIME0_HTH_MASK as i64),
            ("STIME0_SEL_SHIFT", STIME0_SEL_SHIFT as i64),
            ("STIME0_SEL_MASK", STIME0_SEL_MASK as i64),
            ("SIOP_STIME1", SIOP_STIME1 as i64),
            ("STIME1_HTHBA", STIME1_HTHBA as i64),
            ("STIME1_GENSF", STIME1_GENSF as i64),
            ("STIME1_HTHSF", STIME1_HTHSF as i64),
            ("STIME1_GEN_SHIFT", STIME1_GEN_SHIFT as i64),
            ("STIME1_GEN_MASK", STIME1_GEN_MASK as i64),
            ("SIOP_RESPID0", SIOP_RESPID0 as i64),
            ("SIOP_RESPID1", SIOP_RESPID1 as i64),
            ("SIOP_STEST0", SIOP_STEST0 as i64),
            ("SIOP_STEST1", SIOP_STEST1 as i64),
            ("STEST1_DOGE", STEST1_DOGE as i64),
            ("STEST1_DIGE", STEST1_DIGE as i64),
            ("STEST1_DBLEN", STEST1_DBLEN as i64),
            ("STEST1_DBLSEL", STEST1_DBLSEL as i64),
            ("SIOP_STEST2", SIOP_STEST2 as i64),
            ("STEST2_DIF", STEST2_DIF as i64),
            ("STEST2_EXT", STEST2_EXT as i64),
            ("SIOP_STEST3", SIOP_STEST3 as i64),
            ("STEST3_TE", STEST3_TE as i64),
            ("STEST3_HSC", STEST3_HSC as i64),
            ("SIOP_STEST4", SIOP_STEST4 as i64),
            ("STEST4_MODE_MASK", STEST4_MODE_MASK as i64),
            ("STEST4_MODE_DIF", STEST4_MODE_DIF as i64),
            ("STEST4_MODE_SE", STEST4_MODE_SE as i64),
            ("STEST4_MODE_LVD", STEST4_MODE_LVD as i64),
            ("STEST4_LOCK", STEST4_LOCK as i64),
            ("SIOP_SIDL", SIOP_SIDL as i64),
            ("SIOP_SODL", SIOP_SODL as i64),
            ("SIOP_SBDL", SIOP_SBDL as i64),
            ("SIOP_SCRATCHB", SIOP_SCRATCHB as i64),
            ("SIOP_SCRATCHC", SIOP_SCRATCHC as i64),
            ("SIOP_SCRATCHD", SIOP_SCRATCHD as i64),
            ("SIOP_SCRATCHE", SIOP_SCRATCHE as i64),
            ("SIOP_SCRATCHF", SIOP_SCRATCHF as i64),
            ("SIOP_SCRATCHG", SIOP_SCRATCHG as i64),
            ("SIOP_SCRATCHH", SIOP_SCRATCHH as i64),
            ("SIOP_SCRATCHI", SIOP_SCRATCHI as i64),
            ("SIOP_SCRATCHJ", SIOP_SCRATCHJ as i64),
            ("SIOP_SCNTL4", SIOP_SCNTL4 as i64),
            ("SCNTL4_XCLKS_ST", SCNTL4_XCLKS_ST as i64),
            ("SCNTL4_XCLKS_DT", SCNTL4_XCLKS_DT as i64),
            ("SCNTL4_XCLKH_ST", SCNTL4_XCLKH_ST as i64),
            ("SCNTL4_XCLKH_DT", SCNTL4_XCLKH_DT as i64),
            ("SCNTL4_AIPEN", SCNTL4_AIPEN as i64),
            ("SCNTL4_U3EN", SCNTL4_U3EN as i64),
            ("SIOP_DFBC", SIOP_DFBC as i64),
            ("SIOP_AIPCNTL0", SIOP_AIPCNTL0 as i64),
            ("AIPCNTL0_ERRLIVE", AIPCNTL0_ERRLIVE as i64),
            ("AIPCNTL0_ERR", AIPCNTL0_ERR as i64),
            ("AIPCNTL0_PARITYERRs", AIPCNTL0_PARITYERRs as i64),
            ("SIOP_AIPCNTL1", SIOP_AIPCNTL1 as i64),
            ("AIPCNTL1_DIS", AIPCNTL1_DIS as i64),
            ("AIPCNTL1_RSETERR", AIPCNTL1_RSETERR as i64),
            ("AIPCNTL1_FB", AIPCNTL1_FB as i64),
            ("AIPCNTL1_RSET", AIPCNTL1_RSET as i64),
            ("SIOP_NVRAM_SYM_SIZE", SIOP_NVRAM_SYM_SIZE as i64),
            ("SIOP_NVRAM_SYM_ADDRESS", SIOP_NVRAM_SYM_ADDRESS as i64),
            ("NVRAM_SYM_F_SCAM_ENABLE", NVRAM_SYM_F_SCAM_ENABLE as i64),
            (
                "NVRAM_SYM_F_PARITY_ENABLE",
                NVRAM_SYM_F_PARITY_ENABLE as i64,
            ),
            (
                "NVRAM_SYM_F_VERBOSE_MESSAGES",
                NVRAM_SYM_F_VERBOSE_MESSAGES as i64,
            ),
            ("NVRAM_SYM_F_CHS_MAPPING", NVRAM_SYM_F_CHS_MAPPING as i64),
            ("NVRAM_SYM_F1_SCAN_HI_LO", NVRAM_SYM_F1_SCAN_HI_LO as i64),
            (
                "NVRAM_SYM_TERM_CANT_PROGRAM",
                NVRAM_SYM_TERM_CANT_PROGRAM as i64,
            ),
            ("NVRAM_SYM_TERM_ENABLED", NVRAM_SYM_TERM_ENABLED as i64),
            ("NVRAM_SYM_TERM_DISABLED", NVRAM_SYM_TERM_DISABLED as i64),
            (
                "NVRAM_SYM_RMVBL_NO_SUPPORT",
                NVRAM_SYM_RMVBL_NO_SUPPORT as i64,
            ),
            (
                "NVRAM_SYM_RMVBL_BOOT_DEVICE",
                NVRAM_SYM_RMVBL_BOOT_DEVICE as i64,
            ),
            (
                "NVRAM_SYM_RMVBL_MEDIA_INSTALLED",
                NVRAM_SYM_RMVBL_MEDIA_INSTALLED as i64,
            ),
            (
                "NVRAM_SYM_HOST_F_SCAN_AT_BOOT",
                NVRAM_SYM_HOST_F_SCAN_AT_BOOT as i64,
            ),
            (
                "NVRAM_SYM_TARG_F_DISCONNECT_EN",
                NVRAM_SYM_TARG_F_DISCONNECT_EN as i64,
            ),
            (
                "NVRAM_SYM_TARG_F_SCAN_AT_BOOT",
                NVRAM_SYM_TARG_F_SCAN_AT_BOOT as i64,
            ),
            (
                "NVRAM_SYM_TARG_F_SCAN_LUNS",
                NVRAM_SYM_TARG_F_SCAN_LUNS as i64,
            ),
            ("NVRAM_SYM_TARG_F_TQ_EN", NVRAM_SYM_TARG_F_TQ_EN as i64),
            (
                "NVRAM_SYM_SCAM_DEFAULT_METHOD",
                NVRAM_SYM_SCAM_DEFAULT_METHOD as i64,
            ),
            (
                "NVRAM_SYM_SCAM_DONT_ASSIGN",
                NVRAM_SYM_SCAM_DONT_ASSIGN as i64,
            ),
            (
                "NVRAM_SYM_SCAM_SET_SPECIFIC_ID",
                NVRAM_SYM_SCAM_SET_SPECIFIC_ID as i64,
            ),
            (
                "NVRAM_SYM_SCAM_USE_ORDER_GIVEN",
                NVRAM_SYM_SCAM_USE_ORDER_GIVEN as i64,
            ),
            ("NVRAM_SYM_SCAM_UNKNOWN", NVRAM_SYM_SCAM_UNKNOWN as i64),
            (
                "NVRAM_SYM_SCAM_DEVICE_NOT_FOUND",
                NVRAM_SYM_SCAM_DEVICE_NOT_FOUND as i64,
            ),
            (
                "NVRAM_SYM_SCAM_ID_NOT_SET",
                NVRAM_SYM_SCAM_ID_NOT_SET as i64,
            ),
            ("NVRAM_SYM_SCAM_ID_VALID", NVRAM_SYM_SCAM_ID_VALID as i64),
            ("SIOP_NVRAM_TEK_SIZE", SIOP_NVRAM_TEK_SIZE as i64),
            (
                "SIOP_NVRAM_TEK_93c46_ADDRESS",
                SIOP_NVRAM_TEK_93c46_ADDRESS as i64,
            ),
            (
                "SIOP_NVRAM_TEK_24c16_ADDRESS",
                SIOP_NVRAM_TEK_24c16_ADDRESS as i64,
            ),
        ];
        for &(name, v) in ours {
            assert_eq!(reftest::int(&defs, name), Some(v), "{name}");
        }
    }
}
/* </TESTS> */
