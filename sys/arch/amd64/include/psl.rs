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
 *	@(#)psl.h	5.2 (Berkeley) 1/18/91
 */
/* </LICENSES> */

/* <CODE> */
//! amd64 `<machine/psl.h>`: the processor status longword (`RFLAGS`).
//!
//! Upstream: sys/arch/amd64/include/psl.h @ 3ce1f3f79392

/// `PSL_C`: carry flag.
pub const PSL_C: u64 = 0x0000_0001;
/// `PSL_PF`: parity flag.
pub const PSL_PF: u64 = 0x0000_0004;
/// `PSL_AF`: auxiliary carry flag.
pub const PSL_AF: u64 = 0x0000_0010;
/// `PSL_Z`: zero flag.
pub const PSL_Z: u64 = 0x0000_0040;
/// `PSL_N`: sign flag.
pub const PSL_N: u64 = 0x0000_0080;
/// `PSL_T`: trap flag.
pub const PSL_T: u64 = 0x0000_0100;
/// `PSL_I`: interrupt enable flag.
pub const PSL_I: u64 = 0x0000_0200;
/// `PSL_D`: direction flag.
pub const PSL_D: u64 = 0x0000_0400;
/// `PSL_V`: overflow flag.
pub const PSL_V: u64 = 0x0000_0800;
/// `PSL_IOPL`: i/o privilege level.
pub const PSL_IOPL: u64 = 0x0000_3000;
/// `PSL_NT`: nested task.
pub const PSL_NT: u64 = 0x0000_4000;
/// `PSL_RF`: resume flag.
pub const PSL_RF: u64 = 0x0001_0000;
/// `PSL_VM`: virtual 8086 mode.
pub const PSL_VM: u64 = 0x0002_0000;
/// `PSL_AC`: alignment check flag.
pub const PSL_AC: u64 = 0x0004_0000;
/// `PSL_VIF`: virtual interrupt enable flag.
pub const PSL_VIF: u64 = 0x0008_0000;
/// `PSL_VIP`: virtual interrupt pending flag.
pub const PSL_VIP: u64 = 0x0010_0000;
/// `PSL_ID`: identification flag.
pub const PSL_ID: u64 = 0x0020_0000;

/// `PSL_MBO`: must be one bits.
pub const PSL_MBO: u64 = 0x0000_0002;
/// `PSL_MBZ`: must be zero bits.
pub const PSL_MBZ: u64 = 0xffc0_8028;

/// `PSL_USERSET`: the flags set for a user context.
pub const PSL_USERSET: u64 = PSL_MBO | PSL_I;
/// `PSL_USERSTATIC`: the flags user context may not change.
pub const PSL_USERSTATIC: u64 =
    PSL_MBO | PSL_MBZ | PSL_I | PSL_IOPL | PSL_NT | PSL_VM | PSL_VIF | PSL_VIP;
/// `PSL_USER`: the flags user context may set.
pub const PSL_USER: u64 = PSL_C | PSL_MBO | PSL_PF | PSL_AF | PSL_Z | PSL_N | PSL_V;
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/arch/amd64/include/psl.h");
        let ours: &[(&str, i64)] = &[
            ("PSL_C", PSL_C as i64),
            ("PSL_T", PSL_T as i64),
            ("PSL_I", PSL_I as i64),
            ("PSL_D", PSL_D as i64),
            ("PSL_NT", PSL_NT as i64),
            ("PSL_AC", PSL_AC as i64),
            ("PSL_MBO", PSL_MBO as i64),
            ("PSL_MBZ", PSL_MBZ as i64),
        ];
        for (name, value) in ours {
            assert_eq!(crate::reftest::int(&defs, name), Some(*value), "{name}");
        }
    }
}
/* </TESTS> */
