/* <LICENSES> */
/*
 * Copyright (c) 2020 Theo de Raadt
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
/* </LICENSES> */

/* <CODE> */
//! `mdrandom()`: mix the time stamp counter and the CPU's RDRAND and RDSEED into the seed.
//!
//! Upstream: sys/arch/amd64/stand/libsa/mdrandom.c @ 3ce1f3f79392

use core::arch::asm;

use crate::libsa_md::{CPUIDECX_RDRAND, SEFF0EBX_RDSEED, cpuid, cpuid_leaf};

/// One `rdrand` (`seed == false`) or `rdseed`, retried up to ten times; 0 if none worked.
fn rd(seed: bool) -> u64 {
    let mut rand: u64 = 0;
    for _ in 0..10 {
        let valid: u8;
        // SAFETY: the instructions only write the named registers and the flags; the caller
        // checked CPUID says the CPU has them.
        unsafe {
            if seed {
                asm!("rdseed {r}", "setc {v}", r = out(reg) rand, v = out(reg_byte) valid,
                    options(nomem, nostack));
            } else {
                asm!("rdrand {r}", "setc {v}", r = out(reg) rand, v = out(reg_byte) valid,
                    options(nomem, nostack));
            }
        }
        if valid != 0 {
            break;
        }
    }
    rand
}

/// XORs `rand` into the `i`th 8-byte word of `buf`.
fn xor_word(buf: &mut [u8], i: usize, rand: u64) {
    for (b, r) in buf[8 * i..8 * i + 8].iter_mut().zip(rand.to_ne_bytes()) {
        *b ^= r;
    }
}

/// `mdrandom(buf, buflen)`: always 0.
pub fn mdrandom(buf: &mut [u8]) -> i32 {
    for b in buf.iter_mut() {
        let (hi, lo): (u32, u32);
        // SAFETY: rdtsc only writes edx:eax.
        unsafe { asm!("rdtsc", out("edx") hi, out("eax") lo, options(nomem, nostack)) };
        let mut acc = hi ^ lo;
        acc ^= acc >> 16;
        acc ^= acc >> 8;
        *b ^= acc as u8;
    }

    let (_, _, ecx, _) = cpuid(1);
    if ecx & CPUIDECX_RDRAND != 0 {
        for i in 0..buf.len() / 8 {
            xor_word(buf, i, rd(false));
        }
    }

    let (eax, _, _, _) = cpuid(0);
    if eax >= 7 {
        let (_, ebx, _, _) = cpuid_leaf(7, 0);
        if ebx & SEFF0EBX_RDSEED != 0 {
            for i in 0..buf.len() / 8 {
                xor_word(buf, i, rd(true));
            }
        }
    }
    0
}
/* </CODE> */
