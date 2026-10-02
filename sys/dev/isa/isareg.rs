/*	$OpenBSD: isareg.h,v 1.5 2025/07/14 10:13:54 jsg Exp $	*/
/*	$NetBSD: isareg.h,v 1.5 1995/04/17 12:09:13 cgd Exp $	*/
/* <LICENSES> */
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

//! ISA bus conventions: `<dev/isa/isareg.h>`.
//!
//! Upstream: sys/dev/isa/isareg.h @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M2 needs the timer and RTC ports for `delay(9)`; the rest of the
//! port assignments, the ISA memory hole and the IRQ names arrive with the ISA bus (M4+).

/// `IO_ICU1`: 8259A Interrupt Controller #1.
pub const IO_ICU1: u16 = 0x020;
/// `IO_ICU2`: 8259A Interrupt Controller #2.
pub const IO_ICU2: u16 = 0x0a0;
/// `IO_ICUSIZE`: 8259A interrupt controllers.
pub const IO_ICUSIZE: u16 = 16;
/// 8253 Timer #1.
pub const IO_TIMER1: u16 = 0x040;
/// RTC.
pub const IO_RTC: u16 = 0x070;
/// NMI Control.
pub const IO_NMI: u16 = IO_RTC;

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
        assert_eq!(crate::reftest::int(&defs, "IO_RTC"), Some(IO_RTC as i64));
        assert_eq!(crate::reftest::int(&defs, "IO_NMI"), Some(IO_NMI as i64));
    }
}
