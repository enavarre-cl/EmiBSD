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
 * Copyright (c) 1998 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Frank van der Linden.
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
//! amd64 `<machine/i82489reg.h>`: the local APIC's registers.
//!
//! Upstream: sys/arch/amd64/include/i82489reg.h @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M4 ports the registers and bits `lapic.c`'s boot path uses
//! (ID, version, TPR, EOI, SVR, the LVT entries, the timer registers, the delivery modes and
//! `LAPIC_BASE`); M11a the interrupt command registers (`LAPIC_ICRLO`, `LAPIC_ICRHI`) the
//! IPIs send through. `ESR`, `LDR`/`DFR`, the ISR/TMR/IRR arrays and the rest of the x2APIC
//! MSR map come with their users.

/// `LAPIC_ID`: ID. RW.
pub const LAPIC_ID: i32 = 0x020;
/// `LAPIC_ID_MASK`.
pub const LAPIC_ID_MASK: u32 = 0x0f00_0000;
/// `LAPIC_ID_SHIFT`.
pub const LAPIC_ID_SHIFT: u32 = 24;

/// `LAPIC_VERS`: Version. R.
pub const LAPIC_VERS: i32 = 0x030;
/// `LAPIC_VERSION_MASK`.
pub const LAPIC_VERSION_MASK: u32 = 0x0000_00ff;
/// `LAPIC_VERSION_LVT_MASK`.
pub const LAPIC_VERSION_LVT_MASK: u32 = 0x00ff_0000;
/// `LAPIC_VERSION_LVT_SHIFT`.
pub const LAPIC_VERSION_LVT_SHIFT: u32 = 16;

/// `LAPIC_TPRI`: Task Prio. RW.
pub const LAPIC_TPRI: i32 = 0x080;
/// `LAPIC_TPRI_MASK`.
pub const LAPIC_TPRI_MASK: u32 = 0x0000_00ff;
/// `LAPIC_APRI`: Arbitration prio R.
pub const LAPIC_APRI: i32 = 0x090;
/// `LAPIC_PPRI`: Processor prio. R.
pub const LAPIC_PPRI: i32 = 0x0a0;
/// `LAPIC_EOI`: End Int. W.
pub const LAPIC_EOI: i32 = 0x0b0;
/// `LAPIC_RRR`: Remote read R.
pub const LAPIC_RRR: i32 = 0x0c0;

/// `LAPIC_SVR`: Spurious intvec RW.
pub const LAPIC_SVR: i32 = 0x0f0;
/// `LAPIC_SVR_VECTOR_MASK`.
pub const LAPIC_SVR_VECTOR_MASK: u32 = 0x0000_00ff;
/// `LAPIC_SVR_ENABLE`.
pub const LAPIC_SVR_ENABLE: u32 = 0x0000_0100;
/// `LAPIC_SVR_SWEN`.
pub const LAPIC_SVR_SWEN: u32 = 0x0000_0100;
/// `LAPIC_SVR_FOCUS`.
pub const LAPIC_SVR_FOCUS: u32 = 0x0000_0200;
/// `LAPIC_SVR_FDIS`.
pub const LAPIC_SVR_FDIS: u32 = 0x0000_0200;

/// `LAPIC_LVT_CMCI`: Corrected machine chk LVT.
pub const LAPIC_LVT_CMCI: i32 = 0x2f0;

/// `LAPIC_ICRLO`: interrupt command, low half (RW).
pub const LAPIC_ICRLO: i32 = 0x300;
/// `LAPIC_DLMODE_MASK`: the delivery mode of an ICR or LVT entry.
pub const LAPIC_DLMODE_MASK: u32 = 0x0000_0700;
/// `LAPIC_DLMODE_FIXED`.
pub const LAPIC_DLMODE_FIXED: u32 = 0x0000_0000;
/// `LAPIC_DLMODE_LOW`.
pub const LAPIC_DLMODE_LOW: u32 = 0x0000_0100;
/// `LAPIC_DLMODE_SMI`.
pub const LAPIC_DLMODE_SMI: u32 = 0x0000_0200;
/// `LAPIC_DLMODE_RR`.
pub const LAPIC_DLMODE_RR: u32 = 0x0000_0300;
/// `LAPIC_DLMODE_NMI`.
pub const LAPIC_DLMODE_NMI: u32 = 0x0000_0400;
/// `LAPIC_DLMODE_INIT`.
pub const LAPIC_DLMODE_INIT: u32 = 0x0000_0500;
/// `LAPIC_DLMODE_STARTUP`.
pub const LAPIC_DLMODE_STARTUP: u32 = 0x0000_0600;
/// `LAPIC_DLMODE_EXTINT`.
pub const LAPIC_DLMODE_EXTINT: u32 = 0x0000_0700;
/// `LAPIC_DSTMODE_LOG`.
pub const LAPIC_DSTMODE_LOG: u32 = 0x0000_0800;
/// `LAPIC_DLSTAT_BUSY`.
pub const LAPIC_DLSTAT_BUSY: u32 = 0x0000_1000;
/// `LAPIC_LVL_ASSERT`.
pub const LAPIC_LVL_ASSERT: u32 = 0x0000_4000;
/// `LAPIC_LVL_DEASSERT`.
pub const LAPIC_LVL_DEASSERT: u32 = 0x0000_0000;
/// `LAPIC_LVL_TRIG`.
pub const LAPIC_LVL_TRIG: u32 = 0x0000_8000;
/// `LAPIC_DEST_MASK`.
pub const LAPIC_DEST_MASK: u32 = 0x000c_0000;
/// `LAPIC_DEST_SELF`.
pub const LAPIC_DEST_SELF: u32 = 0x0004_0000;
/// `LAPIC_DEST_ALLINCL`.
pub const LAPIC_DEST_ALLINCL: u32 = 0x0008_0000;
/// `LAPIC_DEST_ALLEXCL`.
pub const LAPIC_DEST_ALLEXCL: u32 = 0x000c_0000;

/// `LAPIC_ICRHI`: interrupt command, high half (RW): the destination.
pub const LAPIC_ICRHI: i32 = 0x310;

/// `LAPIC_LVTT`: Loc.vec.(timer) RW.
pub const LAPIC_LVTT: i32 = 0x320;
/// `LAPIC_LVTT_VEC_MASK`.
pub const LAPIC_LVTT_VEC_MASK: u32 = 0x0000_00ff;
/// `LAPIC_LVTT_DS`.
pub const LAPIC_LVTT_DS: u32 = 0x0000_1000;
/// `LAPIC_LVTT_M`.
pub const LAPIC_LVTT_M: u32 = 0x0001_0000;
/// `LAPIC_LVTT_TM`.
pub const LAPIC_LVTT_TM: u32 = 0x0006_0000;
/// `LAPIC_LVTT_TM_ONESHOT`.
pub const LAPIC_LVTT_TM_ONESHOT: u32 = 0x0000_0000;
/// `LAPIC_LVTT_TM_PERIODIC`.
pub const LAPIC_LVTT_TM_PERIODIC: u32 = 0x0002_0000;
/// `LAPIC_LVTT_TM_TSCDL`.
pub const LAPIC_LVTT_TM_TSCDL: u32 = 0x0004_0000;

/// `LAPIC_LVT_THERM`: Thermal sensor LVT.
pub const LAPIC_LVT_THERM: i32 = 0x330;
/// `LAPIC_PCINT`: the performance counter LVT.
pub const LAPIC_PCINT: i32 = 0x340;
/// `LAPIC_LVINT0`: Loc.vec (LINT0) RW.
pub const LAPIC_LVINT0: i32 = 0x350;
/// `LAPIC_LVT_PERIODIC`.
pub const LAPIC_LVT_PERIODIC: u32 = 0x0002_0000;
/// `LAPIC_LVT_MASKED`.
pub const LAPIC_LVT_MASKED: u32 = 0x0001_0000;
/// `LAPIC_LVT_LEVTRIG`.
pub const LAPIC_LVT_LEVTRIG: u32 = 0x0000_8000;
/// `LAPIC_LVT_REMOTE_IRR`.
pub const LAPIC_LVT_REMOTE_IRR: u32 = 0x0000_4000;
/// `LAPIC_INP_POL`.
pub const LAPIC_INP_POL: u32 = 0x0000_2000;
/// `LAPIC_PEND_SEND`.
pub const LAPIC_PEND_SEND: u32 = 0x0000_1000;
/// `LAPIC_LVINT1`: Loc.vec (LINT1) RW.
pub const LAPIC_LVINT1: i32 = 0x360;
/// `LAPIC_LVERR`: Loc.vec (ERROR) RW.
pub const LAPIC_LVERR: i32 = 0x370;
/// `LAPIC_ICR_TIMER`: Initial count RW.
pub const LAPIC_ICR_TIMER: i32 = 0x380;
/// `LAPIC_CCR_TIMER`: Current count RO.
pub const LAPIC_CCR_TIMER: i32 = 0x390;
/// `LAPIC_DCR_TIMER`: Divisor config register.
pub const LAPIC_DCR_TIMER: i32 = 0x3e0;
/// `LAPIC_DCRT_DIV1`.
pub const LAPIC_DCRT_DIV1: u32 = 0x0b;
/// `LAPIC_DCRT_DIV2`.
pub const LAPIC_DCRT_DIV2: u32 = 0x00;
/// `LAPIC_DCRT_DIV4`.
pub const LAPIC_DCRT_DIV4: u32 = 0x01;
/// `LAPIC_DCRT_DIV8`.
pub const LAPIC_DCRT_DIV8: u32 = 0x02;
/// `LAPIC_DCRT_DIV16`.
pub const LAPIC_DCRT_DIV16: u32 = 0x03;
/// `LAPIC_DCRT_DIV32`.
pub const LAPIC_DCRT_DIV32: u32 = 0x08;
/// `LAPIC_DCRT_DIV64`.
pub const LAPIC_DCRT_DIV64: u32 = 0x09;
/// `LAPIC_DCRT_DIV128`.
pub const LAPIC_DCRT_DIV128: u32 = 0x0a;

/// `LAPIC_BASE`: the architectural default physical address.
pub const LAPIC_BASE: usize = 0xfee0_0000;

/// `LAPIC_IRQ_MASK(i)`.
pub const fn lapic_irq_mask(i: u32) -> u32 {
    1 << (i + 1)
}

/// `MSR_X2APIC_BASE`: the x2APIC's MSR window; register `r` is at `MSR_X2APIC_BASE + r/16`.
pub const MSR_X2APIC_BASE: u32 = 0x800;
/// `MSR_X2APIC_ID`: ID. R.
pub const MSR_X2APIC_ID: u32 = MSR_X2APIC_BASE + 0x02;
/// `X2APIC_ID_MASK`.
pub const X2APIC_ID_MASK: u32 = 0xff;
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    /// The C header's values, read from `$OPENBSD_SRC` (`just test-ref`).
    #[test]
    #[ignore]
    fn matches_reference() {
        let want = [
            ("LAPIC_ID", i64::from(LAPIC_ID)),
            ("LAPIC_SVR", i64::from(LAPIC_SVR)),
            ("LAPIC_SVR_ENABLE", i64::from(LAPIC_SVR_ENABLE)),
            ("LAPIC_DLMODE_EXTINT", i64::from(LAPIC_DLMODE_EXTINT)),
            ("LAPIC_DLMODE_NMI", i64::from(LAPIC_DLMODE_NMI)),
            ("LAPIC_LVTT", i64::from(LAPIC_LVTT)),
            ("LAPIC_LVINT0", i64::from(LAPIC_LVINT0)),
            ("LAPIC_LVINT1", i64::from(LAPIC_LVINT1)),
            ("LAPIC_LVT_MASKED", i64::from(LAPIC_LVT_MASKED)),
            ("LAPIC_CCR_TIMER", i64::from(LAPIC_CCR_TIMER)),
            ("LAPIC_DCR_TIMER", i64::from(LAPIC_DCR_TIMER)),
            ("LAPIC_BASE", LAPIC_BASE as i64),
            ("MSR_X2APIC_BASE", i64::from(MSR_X2APIC_BASE)),
            ("LAPIC_ICRLO", i64::from(LAPIC_ICRLO)),
            ("LAPIC_ICRHI", i64::from(LAPIC_ICRHI)),
            ("LAPIC_DLSTAT_BUSY", i64::from(LAPIC_DLSTAT_BUSY)),
            ("LAPIC_DEST_MASK", i64::from(LAPIC_DEST_MASK)),
            ("LAPIC_DEST_ALLEXCL", i64::from(LAPIC_DEST_ALLEXCL)),
            ("LAPIC_DLMODE_INIT", i64::from(LAPIC_DLMODE_INIT)),
            ("LAPIC_LVL_ASSERT", i64::from(LAPIC_LVL_ASSERT)),
            ("LAPIC_LVL_TRIG", i64::from(LAPIC_LVL_TRIG)),
        ];
        let defs = crate::reftest::defines("sys/arch/amd64/include/i82489reg.h");
        for (name, value) in want {
            assert_eq!(crate::reftest::int(&defs, name), Some(value), "{name}");
        }
    }
}
/* </TESTS> */
