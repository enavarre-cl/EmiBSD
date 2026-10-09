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
 * Copyright (c) 2009 The DragonFly Project.  All rights reserved.
 *
 * This code is derived from software contributed to The DragonFly Project
 * by Matthew Dillon <dillon@backplane.com>
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 *
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in
 *    the documentation and/or other materials provided with the
 *    distribution.
 * 3. Neither the name of The DragonFly Project nor the names of its
 *    contributors may be used to endorse or promote products derived
 *    from this software without specific, prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
 * ``AS IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
 * LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS
 * FOR A PARTICULAR PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL THE
 * COPYRIGHT HOLDERS OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT,
 * INCIDENTAL, SPECIAL, EXEMPLARY OR CONSEQUENTIAL DAMAGES (INCLUDING,
 * BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES;
 * LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED
 * AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY,
 * OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT
 * OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! `<dev/ata/pmreg.h>`: the SATA port multiplier registers (the PSCR and GSCR numbers and
//! their bits), read and written by `ahci(4)` through READ/WRITE PORT MULTIPLIER commands.
//!
//! Upstream: sys/dev/ata/pmreg.h @ 3ce1f3f79392
//!
//! The header comes from DragonFly BSD and has no `$OpenBSD$` line; its copyright and licence
//! are kept as they are.
//!
//! ## Deviations
//! - The register numbers (`SATA_PMREG_*`), `SATA_PMP_MAX_PORTS` and `SATA_PMP_CONTROL_PORT`
//!   are `i32` (the C's `int` arguments and port numbers), the register bits `u32`.
//! - The `%b` format strings are byte strings with the C's octal escapes in hexadecimal.

/// `SATA_PMREG_SSTS`: use SATA_PM_SSTS_ bit defs.
pub const SATA_PMREG_SSTS: i32 = 0;
/// `SATA_PMREG_SERR`: use SATA_PM_SERR_ bit defs.
pub const SATA_PMREG_SERR: i32 = 1;
/// `SATA_PMREG_SCTL`: use SATA_PM_SCTL_ bit defs.
pub const SATA_PMREG_SCTL: i32 = 2;
/// `SATA_PMREG_SACT`: (not implemented on PM).
pub const SATA_PMREG_SACT: i32 = 3;
/// `SATA_PM_SSTS_DET`: Device Detection.
pub const SATA_PM_SSTS_DET: u32 = 0xf;
/// `SATA_PM_SSTS_DET_NONE`.
pub const SATA_PM_SSTS_DET_NONE: u32 = 0x0;
/// `SATA_PM_SSTS_DET_DEV_NE`.
pub const SATA_PM_SSTS_DET_DEV_NE: u32 = 0x1;
/// `SATA_PM_SSTS_DET_DEV`.
pub const SATA_PM_SSTS_DET_DEV: u32 = 0x3;
/// `SATA_PM_SSTS_DET_PHYOFFLINE`.
pub const SATA_PM_SSTS_DET_PHYOFFLINE: u32 = 0x4;
/// `SATA_PM_SSTS_SPD`: Current Interface Speed.
pub const SATA_PM_SSTS_SPD: u32 = 0xf0;
/// `SATA_PM_SSTS_SPD_NONE`.
pub const SATA_PM_SSTS_SPD_NONE: u32 = 0x00;
/// `SATA_PM_SSTS_SPD_GEN1`.
pub const SATA_PM_SSTS_SPD_GEN1: u32 = 0x10;
/// `SATA_PM_SSTS_SPD_GEN2`.
pub const SATA_PM_SSTS_SPD_GEN2: u32 = 0x20;
/// `SATA_PM_SSTS_IPM`: Interface Power Management.
pub const SATA_PM_SSTS_IPM: u32 = 0xf00;
/// `SATA_PM_SSTS_IPM_NONE`.
pub const SATA_PM_SSTS_IPM_NONE: u32 = 0x000;
/// `SATA_PM_SSTS_IPM_ACTIVE`.
pub const SATA_PM_SSTS_IPM_ACTIVE: u32 = 0x100;
/// `SATA_PM_SSTS_IPM_PARTIAL`.
pub const SATA_PM_SSTS_IPM_PARTIAL: u32 = 0x200;
/// `SATA_PM_SSTS_IPM_SLUMBER`.
pub const SATA_PM_SSTS_IPM_SLUMBER: u32 = 0x600;
/// `SATA_PM_SCTL_DET`: Device Detection.
pub const SATA_PM_SCTL_DET: u32 = 0xf;
/// `SATA_PM_SCTL_DET_NONE`.
pub const SATA_PM_SCTL_DET_NONE: u32 = 0x0;
/// `SATA_PM_SCTL_DET_INIT`.
pub const SATA_PM_SCTL_DET_INIT: u32 = 0x1;
/// `SATA_PM_SCTL_DET_DISABLE`.
pub const SATA_PM_SCTL_DET_DISABLE: u32 = 0x4;
/// `SATA_PM_SCTL_SPD`: Speed Allowed.
pub const SATA_PM_SCTL_SPD: u32 = 0xf0;
/// `SATA_PM_SCTL_SPD_ANY`.
pub const SATA_PM_SCTL_SPD_ANY: u32 = 0x00;
/// `SATA_PM_SCTL_SPD_GEN1`.
pub const SATA_PM_SCTL_SPD_GEN1: u32 = 0x10;
/// `SATA_PM_SCTL_SPD_GEN2`.
pub const SATA_PM_SCTL_SPD_GEN2: u32 = 0x20;
/// `SATA_PM_SCTL_IPM`: Interface Power Management.
pub const SATA_PM_SCTL_IPM: u32 = 0xf00;
/// `SATA_PM_SCTL_IPM_NONE`.
pub const SATA_PM_SCTL_IPM_NONE: u32 = 0x000;
/// `SATA_PM_SCTL_IPM_NOPARTIAL`.
pub const SATA_PM_SCTL_IPM_NOPARTIAL: u32 = 0x100;
/// `SATA_PM_SCTL_IPM_NOSLUMBER`.
pub const SATA_PM_SCTL_IPM_NOSLUMBER: u32 = 0x200;
/// `SATA_PM_SCTL_IPM_DISABLED`.
pub const SATA_PM_SCTL_IPM_DISABLED: u32 = 0x300;
/// `SATA_PM_SCTL_SPM`: Select Power Management.
pub const SATA_PM_SCTL_SPM: u32 = 0xf000;
/// `SATA_PM_SCTL_SPM_NONE`.
pub const SATA_PM_SCTL_SPM_NONE: u32 = 0x0000;
/// `SATA_PM_SCTL_SPM_NOPARTIAL`.
pub const SATA_PM_SCTL_SPM_NOPARTIAL: u32 = 0x1000;
/// `SATA_PM_SCTL_SPM_NOSLUMBER`.
pub const SATA_PM_SCTL_SPM_NOSLUMBER: u32 = 0x2000;
/// `SATA_PM_SCTL_SPM_DISABLED`.
pub const SATA_PM_SCTL_SPM_DISABLED: u32 = 0x3000;
/// `SATA_PM_SCTL_PMP`: Set PM port for xmit FISes.
pub const SATA_PM_SCTL_PMP: u32 = 0xf0000;
/// `SATA_PM_SCTL_PMP_SHIFT`.
pub const SATA_PM_SCTL_PMP_SHIFT: u32 = 16;
/// `SATA_PM_SERR_ERR_I`: Recovered Data Integrity.
pub const SATA_PM_SERR_ERR_I: u32 = 1 << 0;
/// `SATA_PM_SERR_ERR_M`: Recovered Communications.
pub const SATA_PM_SERR_ERR_M: u32 = 1 << 1;
/// `SATA_PM_SERR_ERR_T`: Transient Data Integrity.
pub const SATA_PM_SERR_ERR_T: u32 = 1 << 8;
/// `SATA_PM_SERR_ERR_C`: Persistent Comm/Data.
pub const SATA_PM_SERR_ERR_C: u32 = 1 << 9;
/// `SATA_PM_SERR_ERR_P`: Protocol.
pub const SATA_PM_SERR_ERR_P: u32 = 1 << 10;
/// `SATA_PM_SERR_ERR_E`: Internal.
pub const SATA_PM_SERR_ERR_E: u32 = 1 << 11;
/// `SATA_PM_SERR_DIAG_N`: PhyRdy Change.
pub const SATA_PM_SERR_DIAG_N: u32 = 1 << 16;
/// `SATA_PM_SERR_DIAG_I`: Phy Internal Error.
pub const SATA_PM_SERR_DIAG_I: u32 = 1 << 17;
/// `SATA_PM_SERR_DIAG_W`: Comm Wake.
pub const SATA_PM_SERR_DIAG_W: u32 = 1 << 18;
/// `SATA_PM_SERR_DIAG_B`: 10B to 8B Decode Error.
pub const SATA_PM_SERR_DIAG_B: u32 = 1 << 19;
/// `SATA_PM_SERR_DIAG_D`: Disparity Error.
pub const SATA_PM_SERR_DIAG_D: u32 = 1 << 20;
/// `SATA_PM_SERR_DIAG_C`: CRC Error.
pub const SATA_PM_SERR_DIAG_C: u32 = 1 << 21;
/// `SATA_PM_SERR_DIAG_H`: Handshake Error.
pub const SATA_PM_SERR_DIAG_H: u32 = 1 << 22;
/// `SATA_PM_SERR_DIAG_S`: Link Sequence Error.
pub const SATA_PM_SERR_DIAG_S: u32 = 1 << 23;
/// `SATA_PM_SERR_DIAG_T`: Transport State Trans Err.
pub const SATA_PM_SERR_DIAG_T: u32 = 1 << 24;
/// `SATA_PM_SERR_DIAG_F`: Unknown FIS Type.
pub const SATA_PM_SERR_DIAG_F: u32 = 1 << 25;
/// `SATA_PM_SERR_DIAG_X`: Exchanged.
pub const SATA_PM_SERR_DIAG_X: u32 = 1 << 26;
/// `SATA_PFMT_SERR`: the `%b` format of the bits above.
pub const SATA_PFMT_SERR: &[u8] = b"\x10\x1bDIAG.X\x1aDIAG.F\x19DIAG.T\x18DIAG.S\x17DIAG.H\x16DIAG.C\x15DIAG.D\x14DIAG.B\x13DIAG.W\x12DIAG.I\x11DIAG.N\x0cERR.E\x0bERR.P\x0aERR.C\x09ERR.T\x02ERR.M\x01ERR.I";
/// `SATA_PMREV_PM1_0`.
pub const SATA_PMREV_PM1_0: u32 = 0x00000002;
/// `SATA_PMREV_PM1_1`.
pub const SATA_PMREV_PM1_1: u32 = 0x00000004;
/// `SATA_PFMT_PM_REV`: the `%b` format of the bits above.
pub const SATA_PFMT_PM_REV: &[u8] = b"\x10\x03PM1.1\x02PM1.0";
/// `SATA_PMREG_FEA`.
pub const SATA_PMREG_FEA: i32 = 64;
/// `SATA_PMREG_FEAEN`: (features enabled).
pub const SATA_PMREG_FEAEN: i32 = 96;
/// `SATA_PMFEA_BIST`: BIST Support.
pub const SATA_PMFEA_BIST: u32 = 0x00000001;
/// `SATA_PMFEA_PMREQ`: Can issue PMREQp to host.
pub const SATA_PMFEA_PMREQ: u32 = 0x00000002;
/// `SATA_PMFEA_DYNSSC`: Dynamic SSC transmit enab.
pub const SATA_PMFEA_DYNSSC: u32 = 0x00000004;
/// `SATA_PMFEA_ASYNCNOTIFY`: Async notification.
pub const SATA_PMFEA_ASYNCNOTIFY: u32 = 0x00000008;
/// `SATA_PFMT_PM_FEA`: the `%b` format of the bits above.
pub const SATA_PFMT_PM_FEA: &[u8] = b"\x10\x04AsyncNotify\x03DynamicSSC\x02PMREQ\x01BIST";
/// `SATA_PMREG_EINFO`: error info 16 ports.
pub const SATA_PMREG_EINFO: i32 = 32;
/// `SATA_PMREG_EEENA`: error info enable 16 ports.
pub const SATA_PMREG_EEENA: i32 = 33;
/// `SATA_PMP_MAX_PORTS`.
pub const SATA_PMP_MAX_PORTS: i32 = 16;
/// `SATA_PMP_CONTROL_PORT`.
pub const SATA_PMP_CONTROL_PORT: i32 = 0x0f;
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/ata/pmreg.h");
        let mut names = crate::reftest::assert_defines!(defs;
            SATA_PMREG_SSTS, SATA_PMREG_SERR, SATA_PMREG_SCTL, SATA_PMREG_SACT,
            SATA_PM_SSTS_DET, SATA_PM_SSTS_DET_NONE, SATA_PM_SSTS_DET_DEV_NE,
            SATA_PM_SSTS_DET_DEV, SATA_PM_SSTS_DET_PHYOFFLINE, SATA_PM_SSTS_SPD,
            SATA_PM_SSTS_SPD_NONE, SATA_PM_SSTS_SPD_GEN1, SATA_PM_SSTS_SPD_GEN2,
            SATA_PM_SSTS_IPM, SATA_PM_SSTS_IPM_NONE, SATA_PM_SSTS_IPM_ACTIVE,
            SATA_PM_SSTS_IPM_PARTIAL, SATA_PM_SSTS_IPM_SLUMBER, SATA_PM_SCTL_DET,
            SATA_PM_SCTL_DET_NONE, SATA_PM_SCTL_DET_INIT, SATA_PM_SCTL_DET_DISABLE,
            SATA_PM_SCTL_SPD, SATA_PM_SCTL_SPD_ANY, SATA_PM_SCTL_SPD_GEN1,
            SATA_PM_SCTL_SPD_GEN2, SATA_PM_SCTL_IPM, SATA_PM_SCTL_IPM_NONE,
            SATA_PM_SCTL_IPM_NOPARTIAL, SATA_PM_SCTL_IPM_NOSLUMBER, SATA_PM_SCTL_IPM_DISABLED,
            SATA_PM_SCTL_SPM, SATA_PM_SCTL_SPM_NONE, SATA_PM_SCTL_SPM_NOPARTIAL,
            SATA_PM_SCTL_SPM_NOSLUMBER, SATA_PM_SCTL_SPM_DISABLED, SATA_PM_SCTL_PMP,
            SATA_PM_SCTL_PMP_SHIFT, SATA_PM_SERR_ERR_I, SATA_PM_SERR_ERR_M, SATA_PM_SERR_ERR_T,
            SATA_PM_SERR_ERR_C, SATA_PM_SERR_ERR_P, SATA_PM_SERR_ERR_E, SATA_PM_SERR_DIAG_N,
            SATA_PM_SERR_DIAG_I, SATA_PM_SERR_DIAG_W, SATA_PM_SERR_DIAG_B, SATA_PM_SERR_DIAG_D,
            SATA_PM_SERR_DIAG_C, SATA_PM_SERR_DIAG_H, SATA_PM_SERR_DIAG_S, SATA_PM_SERR_DIAG_T,
            SATA_PM_SERR_DIAG_F, SATA_PM_SERR_DIAG_X, SATA_PMREV_PM1_0, SATA_PMREV_PM1_1,
            SATA_PMREG_FEA, SATA_PMREG_FEAEN, SATA_PMFEA_BIST, SATA_PMFEA_PMREQ,
            SATA_PMFEA_DYNSSC, SATA_PMFEA_ASYNCNOTIFY, SATA_PMREG_EINFO, SATA_PMREG_EEENA,
            SATA_PMP_MAX_PORTS, SATA_PMP_CONTROL_PORT,
        );
        names.extend(["SATA_PFMT_SERR", "SATA_PFMT_PM_REV", "SATA_PFMT_PM_FEA"]);
        crate::reftest::assert_complete(&defs, "SATA_", &names);
        assert_eq!(
            SATA_PFMT_PM_FEA,
            b"\x10\x04AsyncNotify\x03DynamicSSC\x02PMREQ\x01BIST"
        );
    }
}
/* </TESTS> */
