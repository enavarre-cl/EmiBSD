/*	$OpenBSD: ppbreg.h,v 1.6 2020/05/23 07:58:24 patrick Exp $	*/
/*	$NetBSD: ppbreg.h,v 1.3 2001/07/06 18:07:16 mcr Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1996 Christopher G. Demetriou.  All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. All advertising materials mentioning features or use of this software
 *    must display the following acknowledgement:
 *      This product includes software developed by Christopher G. Demetriou
 *	for the NetBSD Project.
 * 4. The name of the author may not be used to endorse or promote products
 *    derived from this software without specific prior written permission
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
//! `<dev/pci/ppbreg.h>`: PCI-PCI bridge chip register definitions and macros, from the "PCI
//! to PCI Bridge Architecture Specification, Revision 1.0, April 5, 1994" (XXX much is
//! missing).
//!
//! Upstream: sys/dev/pci/ppbreg.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - Function-like macros are `const fn`s with the lower-case name; register offsets are
//!   `i32` (`int reg`), register contents `u32` (`pcireg_t`), as in `pcireg.rs`.

/// `PPB_INTERFACE_SUBTRACTIVE`: the PCI programming interface of a subtractive bridge.
pub const PPB_INTERFACE_SUBTRACTIVE: u32 = 0x01;

/// `PPB_REG_BASE0`: base addr reg. 0.
pub const PPB_REG_BASE0: i32 = 0x10;
/// `PPB_REG_BASE1`: base addr reg. 1.
pub const PPB_REG_BASE1: i32 = 0x14;
/// `PPB_REG_BUSINFO`: bus information.
pub const PPB_REG_BUSINFO: i32 = 0x18;
/// `PPB_REG_IOSTATUS`: I/O base+lim & sec stat.
pub const PPB_REG_IOSTATUS: i32 = 0x1c;
/// `PPB_REG_MEM`: memory base/limit.
pub const PPB_REG_MEM: i32 = 0x20;
/// `PPB_REG_PREFMEM`: pref mem base/limit.
pub const PPB_REG_PREFMEM: i32 = 0x24;
/// `PPB_REG_PREFBASE_HI32`: pref mem base high bits.
pub const PPB_REG_PREFBASE_HI32: i32 = 0x28;
/// `PPB_REG_PREFLIM_HI32`: pref mem lim high bits.
pub const PPB_REG_PREFLIM_HI32: i32 = 0x2c;
/// `PPB_REG_IO_HI`: I/O base+lim high bits.
pub const PPB_REG_IO_HI: i32 = 0x30;
/// `PPB_REG_BRIDGECONTROL`: bridge control register.
pub const PPB_REG_BRIDGECONTROL: i32 = 0x3c;

/// `PPB_BUSINFO_PRIMARY(bir)`: the bus the bridge is on.
pub const fn ppb_businfo_primary(bir: u32) -> u32 {
    bir & 0xff
}

/// `PPB_BUSINFO_SECONDARY(bir)`: the bus behind the bridge.
pub const fn ppb_businfo_secondary(bir: u32) -> u32 {
    (bir >> 8) & 0xff
}

/// `PPB_BUSINFO_SUBORDINATE(bir)`: the highest bus number behind the bridge.
pub const fn ppb_businfo_subordinate(bir: u32) -> u32 {
    (bir >> 16) & 0xff
}

/// `PPB_BUSINFO_SECLAT(bir)`: the secondary latency timer.
pub const fn ppb_businfo_seclat(bir: u32) -> u32 {
    (bir >> 24) & 0xff
}

/// `PPB_INTERRUPT_SWIZZLE(pin, device)`: the primary bus interrupt pin of a secondary bus
/// device's pin.
pub const fn ppb_interrupt_swizzle(pin: i32, device: i32) -> i32 {
    ((pin + device - 1) % 4) + 1
}

/// `PPB_IOBASE_SHIFT`: secondary bus I/O base and limits.
pub const PPB_IOBASE_SHIFT: u32 = 0;
/// `PPB_IOLIMIT_SHIFT`.
pub const PPB_IOLIMIT_SHIFT: u32 = 8;
/// `PPB_IO_MASK`.
pub const PPB_IO_MASK: u32 = 0xf000;
/// `PPB_IO_32BIT`.
pub const PPB_IO_32BIT: u32 = 0x0001;
/// `PPB_IO_SHIFT`.
pub const PPB_IO_SHIFT: u32 = 8;
/// `PPB_IO_MIN`.
pub const PPB_IO_MIN: u32 = 4096;

/// `PPB_MEMBASE_SHIFT`: secondary bus memory base and limits.
pub const PPB_MEMBASE_SHIFT: u32 = 0;
/// `PPB_MEMLIMIT_SHIFT`.
pub const PPB_MEMLIMIT_SHIFT: u32 = 16;
/// `PPB_MEM_MASK`.
pub const PPB_MEM_MASK: u32 = 0xfff0_0000;
/// `PPB_MEM_SHIFT`.
pub const PPB_MEM_SHIFT: u32 = 16;
/// `PPB_MEM_MIN`.
pub const PPB_MEM_MIN: u32 = 0x0010_0000;

/// `PPB_BC_BITBASE`: the bridge control bits are in the upper 16 bits of the register (the
/// bottom 16 are the interrupt line and pin), table 3.9 of ppb rev. 1.1.
pub const PPB_BC_BITBASE: u32 = 16;

/// `PPB_BC_PARITYERRORRESPONSE_ENABLE`.
pub const PPB_BC_PARITYERRORRESPONSE_ENABLE: u32 = 1 << PPB_BC_BITBASE;
/// `PPB_BC_SERR_ENABLE`.
pub const PPB_BC_SERR_ENABLE: u32 = 1 << (1 + PPB_BC_BITBASE);
/// `PPB_BC_ISA_ENABLE`.
pub const PPB_BC_ISA_ENABLE: u32 = 1 << (2 + PPB_BC_BITBASE);
/// `PPB_BC_VGA_ENABLE`.
pub const PPB_BC_VGA_ENABLE: u32 = 1 << (3 + PPB_BC_BITBASE);
/// `PPB_BC_MASTER_ABORT_MODE`.
pub const PPB_BC_MASTER_ABORT_MODE: u32 = 1 << (5 + PPB_BC_BITBASE);
/// `PPB_BC_SECONDARY_RESET`.
pub const PPB_BC_SECONDARY_RESET: u32 = 1 << (6 + PPB_BC_BITBASE);
/// `PPB_BC_FAST_B2B_ENABLE`.
pub const PPB_BC_FAST_B2B_ENABLE: u32 = 1 << (7 + PPB_BC_BITBASE);
/// `PPB_BC_PRIMARY_DISCARD_TIMEOUT` (PCI 2.2).
pub const PPB_BC_PRIMARY_DISCARD_TIMEOUT: u32 = 1 << (8 + PPB_BC_BITBASE);
/// `PPB_BC_SECONDARY_DISCARD_TIMEOUT` (PCI 2.2).
pub const PPB_BC_SECONDARY_DISCARD_TIMEOUT: u32 = 1 << (9 + PPB_BC_BITBASE);
/// `PPB_BC_DISCARD_TIMER_STATUS` (PCI 2.2).
pub const PPB_BC_DISCARD_TIMER_STATUS: u32 = 1 << (10 + PPB_BC_BITBASE);
/// `PPB_BC_DISCARD_TIMER_SERR_ENABLE` (PCI 2.2).
pub const PPB_BC_DISCARD_TIMER_SERR_ENABLE: u32 = 1 << (11 + PPB_BC_BITBASE);
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swizzle_and_businfo() {
        // Device 1's pin A appears as pin B on the primary bus; pin D wraps to pin A.
        assert_eq!(ppb_interrupt_swizzle(1, 1), 2);
        assert_eq!(ppb_interrupt_swizzle(4, 1), 1);
        let bir = 0x40_05_03_02;
        assert_eq!(ppb_businfo_primary(bir), 2);
        assert_eq!(ppb_businfo_secondary(bir), 3);
        assert_eq!(ppb_businfo_subordinate(bir), 5);
        assert_eq!(ppb_businfo_seclat(bir), 0x40);
        assert_eq!(PPB_BC_VGA_ENABLE, 0x0008_0000);
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/pci/ppbreg.h");
        for (name, value) in [
            ("PPB_REG_BUSINFO", i64::from(PPB_REG_BUSINFO)),
            ("PPB_REG_IOSTATUS", i64::from(PPB_REG_IOSTATUS)),
            ("PPB_REG_MEM", i64::from(PPB_REG_MEM)),
            ("PPB_REG_PREFMEM", i64::from(PPB_REG_PREFMEM)),
            ("PPB_REG_PREFBASE_HI32", i64::from(PPB_REG_PREFBASE_HI32)),
            ("PPB_REG_PREFLIM_HI32", i64::from(PPB_REG_PREFLIM_HI32)),
            ("PPB_REG_IO_HI", i64::from(PPB_REG_IO_HI)),
            ("PPB_REG_BRIDGECONTROL", i64::from(PPB_REG_BRIDGECONTROL)),
            ("PPB_MEM_MASK", i64::from(PPB_MEM_MASK)),
            ("PPB_BC_BITBASE", i64::from(PPB_BC_BITBASE)),
        ] {
            assert_eq!(crate::reftest::int(&defs, name), Some(value), "{name}");
        }
    }
}
/* </TESTS> */
