/*	$OpenBSD: trap.h,v 1.6 2025/06/23 11:33:39 bluhm Exp $	*/
/*	$NetBSD: trap.h,v 1.4 1994/10/27 04:16:30 cgd Exp $	*/
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
 *	@(#)trap.h	5.4 (Berkeley) 5/9/91
 */
/* </LICENSES> */

//! amd64 `<machine/trap.h>`: trap type values, also known in `trap.c` for name strings.
//!
//! Upstream: sys/arch/amd64/include/trap.h @ 3ce1f3f79392

/// `T_PRIVINFLT`: privileged instruction.
pub const T_PRIVINFLT: i32 = 0;
/// `T_BPTFLT`: breakpoint trap.
pub const T_BPTFLT: i32 = 1;
/// `T_ARITHTRAP`: arithmetic trap.
pub const T_ARITHTRAP: i32 = 2;
/// `T_RESERVED`: reserved fault base.
pub const T_RESERVED: i32 = 3;
/// `T_PROTFLT`: protection fault.
pub const T_PROTFLT: i32 = 4;
/// `T_TRCTRAP`: trace trap.
pub const T_TRCTRAP: i32 = 5;
/// `T_PAGEFLT`: page fault.
pub const T_PAGEFLT: i32 = 6;
/// `T_ALIGNFLT`: alignment fault.
pub const T_ALIGNFLT: i32 = 7;
/// `T_DIVIDE`: integer divide fault.
pub const T_DIVIDE: i32 = 8;
/// `T_NMI`: non-maskable interrupt.
pub const T_NMI: i32 = 9;
/// `T_OFLOW`: overflow trap.
pub const T_OFLOW: i32 = 10;
/// `T_BOUND`: bounds check fault.
pub const T_BOUND: i32 = 11;
/// `T_DNA`: device not available fault.
pub const T_DNA: i32 = 12;
/// `T_DOUBLEFLT`: double fault.
pub const T_DOUBLEFLT: i32 = 13;
/// `T_FPOPFLT`: fp coprocessor operand fetch fault (!\[P\]Pro).
pub const T_FPOPFLT: i32 = 14;
/// `T_TSSFLT`: invalid tss fault.
pub const T_TSSFLT: i32 = 15;
/// `T_SEGNPFLT`: segment not present fault.
pub const T_SEGNPFLT: i32 = 16;
/// `T_STKFLT`: stack fault.
pub const T_STKFLT: i32 = 17;
/// `T_MCA`: machine check (\[P\]Pro).
pub const T_MCA: i32 = 18;
/// `T_XMM`: SSE FP exception.
pub const T_XMM: i32 = 19;
/// `T_VE`: virtualization exception.
pub const T_VE: i32 = 20;
/// `T_CP`: control protection exception.
pub const T_CP: i32 = 21;
/// `T_VC`: VMM communication exception.
pub const T_VC: i32 = 29;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/arch/amd64/include/trap.h");
        let ours: &[(&str, i64)] = &[
            ("T_PRIVINFLT", i64::from(T_PRIVINFLT)),
            ("T_BPTFLT", i64::from(T_BPTFLT)),
            ("T_PROTFLT", i64::from(T_PROTFLT)),
            ("T_TRCTRAP", i64::from(T_TRCTRAP)),
            ("T_PAGEFLT", i64::from(T_PAGEFLT)),
            ("T_NMI", i64::from(T_NMI)),
            ("T_DOUBLEFLT", i64::from(T_DOUBLEFLT)),
            ("T_MCA", i64::from(T_MCA)),
            ("T_CP", i64::from(T_CP)),
            ("T_VC", i64::from(T_VC)),
        ];
        for (name, value) in ours {
            assert_eq!(crate::reftest::int(&defs, name), Some(*value), "{name}");
        }
    }
}
