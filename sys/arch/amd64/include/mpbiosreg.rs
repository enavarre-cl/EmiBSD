/*	$OpenBSD: mpbiosreg.h,v 1.5 2023/04/10 04:21:20 jsg Exp $	*/
/* 	$NetBSD: mpbiosreg.h,v 1.3 2003/03/04 23:27:32 fvdl Exp $ */
/* <LICENSES> */
/*-
 * Copyright (c) 2000 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by RedBack Networks Inc.
 *
 * Author: Bill Sommerfeld
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
//! amd64 `<machine/mpbiosreg.h>`: the Intel MultiProcessor Specification's table formats,
//! whose interrupt-entry flags the ACPI MADT and `_PRT` reuse.
//!
//! Upstream: sys/arch/amd64/include/mpbiosreg.h @ 3ce1f3f79392
//!
//! Status: `wip`. M13 ports the interrupt types and flags (`MPS_INTTYPE_*`, `MPS_INTPO_*`,
//! `MPS_INTTR_*`, `MPS_INT`) and `MPS_ALL_APICS`, which `acpimadt`, `acpiprt`, `ioapic.c`
//! and `lapic.c` use. The table structures (`struct mpbios_fps`, `mpbios_cth`,
//! `mpbios_proc`, `mpbios_bus`, `mpbios_ioapic`, `mpbios_int`) and their constants come
//! with `mpbios.c`.
//!
//! ## Deviations
//! - `MPS_INT(p,t)` is a `const fn` with a lower-case name.

/// `MPS_INTTYPE_INT`.
pub const MPS_INTTYPE_INT: i32 = 0;
/// `MPS_INTTYPE_NMI`.
pub const MPS_INTTYPE_NMI: i32 = 1;
/// `MPS_INTTYPE_SMI`.
pub const MPS_INTTYPE_SMI: i32 = 2;
/// `MPS_INTTYPE_ExtINT`.
#[allow(non_upper_case_globals)] // the C name
pub const MPS_INTTYPE_ExtINT: i32 = 3;

/// `MPS_INTPO_DEF`: polarity conforms to the bus.
pub const MPS_INTPO_DEF: i32 = 0;
/// `MPS_INTPO_ACTHI`.
pub const MPS_INTPO_ACTHI: i32 = 1;
/// `MPS_INTPO_ACTLO`.
pub const MPS_INTPO_ACTLO: i32 = 3;
/// `MPS_INTPO_SHIFT`.
pub const MPS_INTPO_SHIFT: i32 = 0;
/// `MPS_INTPO_MASK`.
pub const MPS_INTPO_MASK: i32 = 3;

/// `MPS_INTTR_DEF`: trigger mode conforms to the bus.
pub const MPS_INTTR_DEF: i32 = 0;
/// `MPS_INTTR_EDGE`.
pub const MPS_INTTR_EDGE: i32 = 1;
/// `MPS_INTTR_LEVEL`.
pub const MPS_INTTR_LEVEL: i32 = 3;
/// `MPS_INTTR_SHIFT`.
pub const MPS_INTTR_SHIFT: i32 = 2;
/// `MPS_INTTR_MASK`.
pub const MPS_INTTR_MASK: i32 = 3;

/// `MPS_INT(p,t)`: the flags of polarity `p` and trigger mode `t`.
pub const fn mps_int(p: i32, t: i32) -> i32 {
    ((p & MPS_INTPO_MASK) << MPS_INTPO_SHIFT) | ((t & MPS_INTTR_MASK) << MPS_INTTR_SHIFT)
}

/// `MPS_ALL_APICS`: an interrupt entry for every (local or I/O) APIC.
pub const MPS_ALL_APICS: i32 = 0xff;
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn int_flags() {
        assert_eq!(mps_int(MPS_INTPO_ACTLO, MPS_INTTR_LEVEL), 0xf);
        assert_eq!(mps_int(MPS_INTPO_ACTHI, MPS_INTTR_EDGE), 0x5);
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/arch/amd64/include/mpbiosreg.h");
        let ours: &[(&str, i64)] = &[
            ("MPS_INTTYPE_ExtINT", i64::from(MPS_INTTYPE_ExtINT)),
            ("MPS_INTPO_ACTLO", i64::from(MPS_INTPO_ACTLO)),
            ("MPS_INTTR_LEVEL", i64::from(MPS_INTTR_LEVEL)),
            ("MPS_INTTR_SHIFT", i64::from(MPS_INTTR_SHIFT)),
            ("MPS_ALL_APICS", i64::from(MPS_ALL_APICS)),
        ];
        for (name, value) in ours {
            assert_eq!(crate::reftest::int(&defs, name), Some(*value), "{name}");
        }
    }
}
/* </TESTS> */
