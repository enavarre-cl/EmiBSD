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
//! amd64 `<machine/i82093reg.h>`: the Intel 82093AA I/O APIC's registers.
//!
//! Upstream: sys/arch/amd64/include/i82093reg.h @ 3ce1f3f79392
//!
//! The registers are reached indirectly: the register number is written to `IOAPIC_REG`
//! and the value read or written through `IOAPIC_DATA`, both 32 bits wide.
//!
//! ## Deviations
//! - `IOAPIC_REDHI(pin)`/`IOAPIC_REDLO(pin)` are `const fn`s with lower-case names.
//! - The assembler macros `ioapic_asm_ack`, `ioapic_mask` and `ioapic_unmask` (`_KERNEL`)
//!   are `.macro`s of `amd64/vector.S`, next to the stubs that use them.

/// `IOAPIC_BASE_DEFAULT`: typically, the first apic lives here.
pub const IOAPIC_BASE_DEFAULT: u64 = 0xfec0_0000;

// Memory-space registers. The externally visible registers are all 32 bits wide; store the
// register number of interest in IOAPIC_REG, and store/fetch the real value in IOAPIC_DATA.

/// `IOAPIC_REG`: the register select.
pub const IOAPIC_REG: usize = 0x0000;
/// `IOAPIC_DATA`: the selected register's window.
pub const IOAPIC_DATA: usize = 0x0010;

// Internal I/O APIC registers.

/// `IOAPIC_ID`.
pub const IOAPIC_ID: i32 = 0x00;
/// `IOAPIC_ID_SHIFT`.
pub const IOAPIC_ID_SHIFT: u32 = 24;
/// `IOAPIC_ID_MASK`.
pub const IOAPIC_ID_MASK: u32 = 0x0f00_0000;

/// `IOAPIC_VER`: version, and maximum interrupt pin number.
pub const IOAPIC_VER: i32 = 0x01;
/// `IOAPIC_VER_SHIFT`.
pub const IOAPIC_VER_SHIFT: u32 = 0;
/// `IOAPIC_VER_MASK`.
pub const IOAPIC_VER_MASK: u32 = 0x0000_00ff;
/// `IOAPIC_MAX_SHIFT`.
pub const IOAPIC_MAX_SHIFT: u32 = 16;
/// `IOAPIC_MAX_MASK`.
pub const IOAPIC_MAX_MASK: u32 = 0x00ff_0000;

/// `IOAPIC_ARB`: arbitration ID. Same format as `IOAPIC_ID` register.
pub const IOAPIC_ARB: i32 = 0x02;

// Redirection table registers.

/// `IOAPIC_REDHI(pin)`.
pub const fn ioapic_redhi(pin: i32) -> i32 {
    0x11 + (pin << 1)
}

/// `IOAPIC_REDLO(pin)`.
pub const fn ioapic_redlo(pin: i32) -> i32 {
    0x10 + (pin << 1)
}

/// `IOAPIC_REDHI_DEST_SHIFT`: destination.
pub const IOAPIC_REDHI_DEST_SHIFT: u32 = 24;
/// `IOAPIC_REDHI_DEST_MASK`.
pub const IOAPIC_REDHI_DEST_MASK: u32 = 0xff00_0000;

/// `IOAPIC_REDLO_MASK`: 0=enabled; 1=masked.
pub const IOAPIC_REDLO_MASK: u32 = 0x0001_0000;

/// `IOAPIC_REDLO_LEVEL`: 0=edge, 1=level.
pub const IOAPIC_REDLO_LEVEL: u32 = 0x0000_8000;
/// `IOAPIC_REDLO_RIRR`: remote IRR; read only.
pub const IOAPIC_REDLO_RIRR: u32 = 0x0000_4000;
/// `IOAPIC_REDLO_ACTLO`: 0=act. hi; 1=act. lo.
pub const IOAPIC_REDLO_ACTLO: u32 = 0x0000_2000;
/// `IOAPIC_REDLO_DELSTS`: 0=idle; 1=send pending.
pub const IOAPIC_REDLO_DELSTS: u32 = 0x0000_1000;
/// `IOAPIC_REDLO_DSTMOD`: 0=physical; 1=logical.
pub const IOAPIC_REDLO_DSTMOD: u32 = 0x0000_0800;

/// `IOAPIC_REDLO_DEL_MASK`: del. mode mask.
pub const IOAPIC_REDLO_DEL_MASK: u32 = 0x0000_0700;
/// `IOAPIC_REDLO_DEL_SHIFT`.
pub const IOAPIC_REDLO_DEL_SHIFT: u32 = 8;

/// `IOAPIC_REDLO_DEL_FIXED`.
pub const IOAPIC_REDLO_DEL_FIXED: u32 = 0;
/// `IOAPIC_REDLO_DEL_LOPRI`.
pub const IOAPIC_REDLO_DEL_LOPRI: u32 = 1;
/// `IOAPIC_REDLO_DEL_SMI`.
pub const IOAPIC_REDLO_DEL_SMI: u32 = 2;
/// `IOAPIC_REDLO_DEL_NMI`.
pub const IOAPIC_REDLO_DEL_NMI: u32 = 4;
/// `IOAPIC_REDLO_DEL_INIT`.
pub const IOAPIC_REDLO_DEL_INIT: u32 = 5;
/// `IOAPIC_REDLO_DEL_EXTINT`.
pub const IOAPIC_REDLO_DEL_EXTINT: u32 = 7;

/// `IOAPIC_REDLO_VECTOR_MASK`: delivery vector.
pub const IOAPIC_REDLO_VECTOR_MASK: u32 = 0x0000_00ff;

/// `IMCR_ADDR`: the interrupt mode configuration register's address port.
pub const IMCR_ADDR: u16 = 0x22;
/// `IMCR_DATA`.
pub const IMCR_DATA: u16 = 0x23;

/// `IMCR_REGISTER`.
pub const IMCR_REGISTER: u8 = 0x70;
/// `IMCR_PIC`.
pub const IMCR_PIC: u8 = 0x00;
/// `IMCR_APIC`.
pub const IMCR_APIC: u8 = 0x01;
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redirection_registers() {
        assert_eq!(ioapic_redlo(0), 0x10);
        assert_eq!(ioapic_redhi(0), 0x11);
        assert_eq!(ioapic_redlo(23), 0x3e);
        assert_eq!(ioapic_redhi(23), 0x3f);
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/arch/amd64/include/i82093reg.h");
        let ours: &[(&str, i64)] = &[
            ("IOAPIC_BASE_DEFAULT", IOAPIC_BASE_DEFAULT as i64),
            ("IOAPIC_REG", IOAPIC_REG as i64),
            ("IOAPIC_DATA", IOAPIC_DATA as i64),
            ("IOAPIC_ID_MASK", i64::from(IOAPIC_ID_MASK)),
            ("IOAPIC_MAX_MASK", i64::from(IOAPIC_MAX_MASK)),
            ("IOAPIC_REDLO_MASK", i64::from(IOAPIC_REDLO_MASK)),
            ("IOAPIC_REDLO_LEVEL", i64::from(IOAPIC_REDLO_LEVEL)),
            ("IOAPIC_REDLO_RIRR", i64::from(IOAPIC_REDLO_RIRR)),
            ("IOAPIC_REDLO_ACTLO", i64::from(IOAPIC_REDLO_ACTLO)),
            ("IOAPIC_REDLO_DSTMOD", i64::from(IOAPIC_REDLO_DSTMOD)),
            ("IOAPIC_REDLO_DEL_MASK", i64::from(IOAPIC_REDLO_DEL_MASK)),
            ("IOAPIC_REDLO_DEL_NMI", i64::from(IOAPIC_REDLO_DEL_NMI)),
            (
                "IOAPIC_REDLO_DEL_EXTINT",
                i64::from(IOAPIC_REDLO_DEL_EXTINT),
            ),
            ("IMCR_REGISTER", i64::from(IMCR_REGISTER)),
        ];
        for (name, value) in ours {
            assert_eq!(crate::reftest::int(&defs, name), Some(*value), "{name}");
        }
    }
}
/* </TESTS> */
