/* $OpenBSD: hypervisor.h,v 1.5 2025/02/11 22:27:09 kettenis Exp $ */
/* <LICENSES> */
/*-
 * Copyright (c) 2013, 2014 Andrew Turner
 * All rights reserved.
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
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 * $FreeBSD: head/sys/arm64/include/hypervisor.h 281494 2015-04-13 14:43:10Z andrew $
 */
/* </LICENSES> */

//! arm64 `<machine/hypervisor.h>`: the EL2 registers' bit definitions.
//!
//! Upstream: sys/arch/arm64/include/hypervisor.h @ 3ce1f3f79392
//!
//! Status: `wip`. M14 (track A3) ports what `locore.S`'s `drop_to_el1` writes when boot(8)
//! enters the kernel at EL2: `CPTR_RES1` and the `HCR_EL2` bits `HCR_TGE`, `HCR_RW`,
//! `HCR_E2H`, `HCR_APK` and `HCR_API`. The rest (the trap, translation and debug
//! registers of a hypervisor) comes with vmm(4).

/// `CPTR_RES1`: the bits of `CPTR_EL2` that read as one.
pub const CPTR_RES1: u64 = 0x0000_32ff;

/// `HCR_TGE`: trap general exceptions to EL2 (host mode with `HCR_E2H`).
pub const HCR_TGE: u64 = 0x0000_0000_0800_0000;
/// `HCR_RW`: EL1 is AArch64.
pub const HCR_RW: u64 = 0x0000_0000_8000_0000;
/// `HCR_E2H`: EL2 host (VHE).
pub const HCR_E2H: u64 = 0x0000_0004_0000_0000;
/// `HCR_APK`: do not trap the pointer authentication key registers.
pub const HCR_APK: u64 = 0x0000_0100_0000_0000;
/// `HCR_API`: do not trap the pointer authentication instructions.
pub const HCR_API: u64 = 0x0000_0200_0000_0000;

#[cfg(test)]
mod tests {
    use super::*;

    /// The C header's values, read from `$OPENBSD_SRC` (`just test-ref`).
    #[test]
    #[ignore]
    fn matches_reference() {
        let want = [
            ("CPTR_RES1", CPTR_RES1 as i64),
            ("HCR_TGE", HCR_TGE as i64),
            ("HCR_RW", HCR_RW as i64),
            ("HCR_E2H", HCR_E2H as i64),
            ("HCR_APK", HCR_APK as i64),
            ("HCR_API", HCR_API as i64),
        ];
        let defs = crate::reftest::defines("sys/arch/arm64/include/hypervisor.h");
        for (name, value) in want {
            assert_eq!(crate::reftest::int(&defs, name), Some(value), "{name}");
        }
    }
}
