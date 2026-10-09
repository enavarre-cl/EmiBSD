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
 *	@(#)icu.h	5.6 (Berkeley) 5/9/91
 */
/* </LICENSES> */

/* <CODE> */
//! amd64 `<machine/i8259.h>`: the legacy 8259A interrupt controllers.
//!
//! Upstream: sys/arch/amd64/include/i8259.h @ 3ce1f3f79392
//!
//! Status: `ported`. `i8259_imen` and `i8259_default_setup` are `amd64/i8259.rs`; the
//! `i8259_asm_*` macros are GAS macros in `amd64/vector.S`.

/// `IRQ_SLAVE`: the slave's line on the master.
pub const IRQ_SLAVE: i32 = 2;

/// `ICU_OFFSET`: interrupt control offset into the IDT; 0-31 are processor exceptions.
pub const ICU_OFFSET: i32 = 32;
/// `ICU_LEN`: 32-47 are ISA interrupts.
pub const ICU_LEN: i32 = 16;

/// `IRQ_BIT(num)`: the bit of `num` in its controller's mask.
pub const fn irq_bit(num: i32) -> u8 {
    1 << (num % 8)
}

/// `IRQ_BYTE(num)`: which controller's mask byte holds `num`.
pub const fn irq_byte(num: i32) -> i32 {
    num >> 3
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn irq_bits() {
        assert_eq!(irq_bit(4), 0x10);
        assert_eq!(irq_byte(4), 0);
        assert_eq!(irq_bit(12), 0x10);
        assert_eq!(irq_byte(12), 1);
        assert_eq!(ICU_OFFSET + ICU_LEN, 48);
    }
}
/* </TESTS> */
