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

/*-
 * Copyright (c) 2003 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Jason R. Thorpe of Wasabi Systems, Inc.
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
 * THIS SOFTWARE IS PROVIDED BY THE NETBSD FOUNDATION, INC. AND CONTRIBUTORS
 * ``AS IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
 * TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
 * PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL THE FOUNDATION OR CONTRIBUTORS
 * BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
 * CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
 * SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
 * INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
 * CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
 * ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
 * POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! Serial ATA register definitions: the SStatus, SError and SControl registers (SCR0-2) of a
//! SATA interface, which pciide's SATA chips in IDE mode read through their own windows.
//!
//! Upstream: sys/dev/ata/satareg.h @ 3ce1f3f79392
//!
//! Reference: Serial ATA: High Speed Serialized AT Attachment, Revision 1.0, 29-August-2001,
//! Serial ATA Working Group.
//!
//! ## Deviations
//! - The mixed-case C names (`SStatus_DET_DEV`, ...) are kept, with
//!   `allow(non_upper_case_globals)`; `SControl_PMP(x)` is a `const fn`.
//! - The values are `u32`, the width of the registers they describe.

#![allow(non_upper_case_globals)] // the C names, mixed case

/*
 * SStatus (SCR0) --
 *	Serial ATA interface status register
 */

// The DET value indicates the interface device detection and PHY state.

/// `SStatus_DET_NODEV`: no device connected.
pub const SStatus_DET_NODEV: u32 = 0x0;
/// `SStatus_DET_DEV_NE`: device, but PHY comm not established.
pub const SStatus_DET_DEV_NE: u32 = 0x1;
/// `SStatus_DET_DEV`: device, PHY comm established.
pub const SStatus_DET_DEV: u32 = 0x3;
/// `SStatus_DET_OFFLINE`: PHY in offline mode.
pub const SStatus_DET_OFFLINE: u32 = 0x4;
/// `SStatus_DET_mask`.
pub const SStatus_DET_mask: u32 = 0xf;
/// `SStatus_DET_shift`.
pub const SStatus_DET_shift: u32 = 0;

// The SPD value indicates the negotiated interface communication speed established.

/// `SStatus_SPD_NONE`: no negotiated speed.
pub const SStatus_SPD_NONE: u32 = 0x0 << 4;
/// `SStatus_SPD_G1`: Generation 1 (1.5Gb/s).
pub const SStatus_SPD_G1: u32 = 0x1 << 4;
/// `SStatus_SPD_G2`: Generation 2 (3.0Gb/s).
pub const SStatus_SPD_G2: u32 = 0x2 << 4;
/// `SStatus_SPD_mask`.
pub const SStatus_SPD_mask: u32 = 0xf << 4;
/// `SStatus_SPD_shift`.
pub const SStatus_SPD_shift: u32 = 4;

// The IPM value indicates the current interface power management state.

/// `SStatus_IPM_NODEV`: no device connected.
pub const SStatus_IPM_NODEV: u32 = 0x0 << 8;
/// `SStatus_IPM_ACTIVE`: ACTIVE state.
pub const SStatus_IPM_ACTIVE: u32 = 0x1 << 8;
/// `SStatus_IPM_PARTIAL`: PARTIAL pm state.
pub const SStatus_IPM_PARTIAL: u32 = 0x2 << 8;
/// `SStatus_IPM_SLUMBER`: SLUMBER pm state.
pub const SStatus_IPM_SLUMBER: u32 = 0x6 << 8;
/// `SStatus_IPM_mask`.
pub const SStatus_IPM_mask: u32 = 0xf << 8;
/// `SStatus_IPM_shift`.
pub const SStatus_IPM_shift: u32 = 8;

/*
 * SError (SCR1) --
 *	Serial ATA interface error register
 */

/// `SError_ERR_I`: recovered data integrity error.
pub const SError_ERR_I: u32 = 1;
/// `SError_ERR_M`: recovered communications error.
pub const SError_ERR_M: u32 = 1 << 1;
/// `SError_ERR_T`: non-recovered transient data integrity error.
pub const SError_ERR_T: u32 = 1 << 8;
/// `SError_ERR_C`: non-recovered persistent communication or data integrity error.
pub const SError_ERR_C: u32 = 1 << 9;
/// `SError_ERR_P`: protocol error.
pub const SError_ERR_P: u32 = 1 << 10;
/// `SError_ERR_E`: internal error.
pub const SError_ERR_E: u32 = 1 << 11;
/// `SError_DIAG_N`: PhyRdy change.
pub const SError_DIAG_N: u32 = 1 << 16;
/// `SError_DIAG_I`: PHY internal error.
pub const SError_DIAG_I: u32 = 1 << 17;
/// `SError_DIAG_W`: Comm Wake.
pub const SError_DIAG_W: u32 = 1 << 18;
/// `SError_DIAG_B`: 10b to 8b decode error.
pub const SError_DIAG_B: u32 = 1 << 19;
/// `SError_DIAG_D`: disparity error.
pub const SError_DIAG_D: u32 = 1 << 20;
/// `SError_DIAG_C`: CRC error.
pub const SError_DIAG_C: u32 = 1 << 21;
/// `SError_DIAG_H`: handshake error.
pub const SError_DIAG_H: u32 = 1 << 22;
/// `SError_DIAG_S`: link sequence error.
pub const SError_DIAG_S: u32 = 1 << 23;
/// `SError_DIAG_T`: transport state transition error.
pub const SError_DIAG_T: u32 = 1 << 24;
/// `SError_DIAG_F`: unrecognized FIS type.
pub const SError_DIAG_F: u32 = 1 << 25;
/// `SError_DIAG_X`: device exchanged.
pub const SError_DIAG_X: u32 = 1 << 26;

/*
 * SControl (SCR2) --
 *	Serial ATA interface control register
 */

// The DET field controls the host adapter device detection and interface initialization.

/// `SControl_DET_NONE`: no device detection or initialization action requested.
pub const SControl_DET_NONE: u32 = 0x0;
/// `SControl_DET_INIT`: initialize interface communication (equiv of a hard reset).
pub const SControl_DET_INIT: u32 = 0x1;
/// `SControl_DET_DISABLE`: disable interface and take PHY offline.
pub const SControl_DET_DISABLE: u32 = 0x4;

// The SPD field represents the highest allowed communication speed the interface is allowed
// to negotiate when communication is established.

/// `SControl_SPD_ANY`: no restrictions.
pub const SControl_SPD_ANY: u32 = 0x0 << 4;
/// `SControl_SPD_G1`: Generation 1 (1.5Gb/s).
pub const SControl_SPD_G1: u32 = 0x1 << 4;
/// `SControl_SPD_G2`: Generation 2 (3.0Gb/s).
pub const SControl_SPD_G2: u32 = 0x2 << 4;

// The IPM field represents the enabled interface power management states that can be
// invoked via the Serial ATA interface power management capabilities.

/// `SControl_IPM_ANY`: no restrictions.
pub const SControl_IPM_ANY: u32 = 0x0 << 8;
/// `SControl_IPM_NOPARTIAL`: PARTIAL disabled.
pub const SControl_IPM_NOPARTIAL: u32 = 0x1 << 8;
/// `SControl_IPM_NOSLUMBER`: SLUMBER disabled.
pub const SControl_IPM_NOSLUMBER: u32 = 0x2 << 8;
/// `SControl_IPM_NONE`: no power management.
pub const SControl_IPM_NONE: u32 = 0x3 << 8;

// The SPM field selects a power management state. A non-zero value written to this field
// causes initiation of the selected power management state.

/// `SControl_SPM_PARTIAL`: transition to PARTIAL.
pub const SControl_SPM_PARTIAL: u32 = 0x1 << 12;
/// `SControl_SPM_SLUMBER`: transition to SLUMBER.
pub const SControl_SPM_SLUMBER: u32 = 0x2 << 12;
/// `SControl_SPM_ComWake`: transition from PM.
pub const SControl_SPM_ComWake: u32 = 0x4 << 12;

/// `SControl_PMP(x)`: the PMP field, which identifies the selected Port Multiplier Port for
/// accessing the SActive register.
#[allow(non_snake_case)] // the C macro's name
pub const fn SControl_PMP(x: u32) -> u32 {
    x << 16
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_multiplier_field() {
        assert_eq!(SControl_PMP(0xf), 0x000f_0000);
        assert_eq!(SStatus_DET_DEV & SStatus_DET_mask, 3);
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/ata/satareg.h");
        let names = crate::reftest::assert_defines!(defs;
            SStatus_DET_NODEV, SStatus_DET_DEV_NE, SStatus_DET_DEV, SStatus_DET_OFFLINE,
            SStatus_DET_mask, SStatus_DET_shift, SStatus_SPD_NONE, SStatus_SPD_G1,
            SStatus_SPD_G2, SStatus_SPD_mask, SStatus_SPD_shift, SStatus_IPM_NODEV,
            SStatus_IPM_ACTIVE, SStatus_IPM_PARTIAL, SStatus_IPM_SLUMBER, SStatus_IPM_mask,
            SStatus_IPM_shift, SError_ERR_I, SError_ERR_M, SError_ERR_T, SError_ERR_C,
            SError_ERR_P, SError_ERR_E, SError_DIAG_N, SError_DIAG_I, SError_DIAG_W,
            SError_DIAG_B, SError_DIAG_D, SError_DIAG_C, SError_DIAG_H, SError_DIAG_S,
            SError_DIAG_T, SError_DIAG_F, SError_DIAG_X, SControl_DET_NONE, SControl_DET_INIT,
            SControl_DET_DISABLE, SControl_SPD_ANY, SControl_SPD_G1, SControl_SPD_G2,
            SControl_IPM_ANY, SControl_IPM_NOPARTIAL, SControl_IPM_NOSLUMBER, SControl_IPM_NONE,
            SControl_SPM_PARTIAL, SControl_SPM_SLUMBER, SControl_SPM_ComWake,
        );
        for prefix in ["SStatus_", "SError_", "SControl_"] {
            crate::reftest::assert_complete(&defs, prefix, &names);
        }
    }
}
/* </TESTS> */
