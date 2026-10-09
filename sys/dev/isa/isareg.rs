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
 * Copyright (c) 1990 The Regents of the University of California.
 * All rights reserved.
 *
 * This code is derived from software contributed to Berkeley by
 * William Jolitz.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. Neither the name of the University nor the names of its contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE REGENTS AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE REGENTS OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 *	@(#)isa.h	5.7 (Berkeley) 5/9/91
 */
/* </LICENSES> */

/* <CODE> */
//! ISA bus conventions: `<dev/isa/isareg.h>`.
//!
//! Upstream: sys/dev/isa/isareg.h @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M2 needs the timer and RTC ports for `delay(9)`; M7b the ISA
//! memory hole (`bus_space_map`); the ISA bus (M8) adds the DMA controllers' ports
//! (`isaattach` maps the delay port among the page registers); `pckbc(4)` (M16d) the
//! keyboard controller's `IO_KBD`. The other port assignments
//! and the IRQ names come with their drivers.

/// `IO_DMA1`: 8237A DMA Controller #1.
pub const IO_DMA1: u16 = 0x000;
/// `IO_ICU1`: 8259A Interrupt Controller #1.
pub const IO_ICU1: u16 = 0x020;
/// `IO_ICU2`: 8259A Interrupt Controller #2.
pub const IO_ICU2: u16 = 0x0a0;
/// `IO_ICUSIZE`: 8259A interrupt controllers.
pub const IO_ICUSIZE: u16 = 16;
/// 8253 Timer #1.
pub const IO_TIMER1: u16 = 0x040;
/// `IO_KBD`: 8042 Keyboard.
pub const IO_KBD: u16 = 0x060;
/// `IO_PPI`: Programmable Peripheral Interface.
pub const IO_PPI: u16 = 0x061;
/// RTC.
pub const IO_RTC: u16 = 0x070;
/// NMI Control.
pub const IO_NMI: u16 = IO_RTC;
/// `IO_DMAPG`: DMA Page Registers.
pub const IO_DMAPG: u16 = 0x080;
/// `IO_DMA2`: 8237A DMA Controller #2.
pub const IO_DMA2: u16 = 0x0c0;

/// `IOM_BEGIN`: start of I/O Memory "hole".
pub const IOM_BEGIN: usize = 0x0a0000;
/// `IOM_END`: end of I/O Memory "hole".
pub const IOM_END: usize = 0x100000;
/// `IOM_SIZE`.
pub const IOM_SIZE: usize = IOM_END - IOM_BEGIN;
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/isa/isareg.h");
        assert_eq!(
            crate::reftest::int(&defs, "IO_TIMER1"),
            Some(IO_TIMER1 as i64)
        );
        assert_eq!(crate::reftest::int(&defs, "IO_ICU1"), Some(IO_ICU1 as i64));
        assert_eq!(crate::reftest::int(&defs, "IO_ICU2"), Some(IO_ICU2 as i64));
        assert_eq!(crate::reftest::int(&defs, "IO_KBD"), Some(IO_KBD as i64));
        assert_eq!(crate::reftest::int(&defs, "IO_PPI"), Some(IO_PPI as i64));
        assert_eq!(crate::reftest::int(&defs, "IO_RTC"), Some(IO_RTC as i64));
        assert_eq!(crate::reftest::int(&defs, "IO_NMI"), Some(IO_NMI as i64));
        assert_eq!(
            crate::reftest::int(&defs, "IOM_BEGIN"),
            Some(IOM_BEGIN as i64)
        );
        assert_eq!(crate::reftest::int(&defs, "IOM_END"), Some(IOM_END as i64));
        assert_eq!(crate::reftest::int(&defs, "IO_DMA1"), Some(IO_DMA1 as i64));
        assert_eq!(
            crate::reftest::int(&defs, "IO_DMAPG"),
            Some(IO_DMAPG as i64)
        );
        assert_eq!(crate::reftest::int(&defs, "IO_DMA2"), Some(IO_DMA2 as i64));
    }
}
/* </TESTS> */
