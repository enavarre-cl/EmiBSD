/*	$OpenBSD: random.c,v 1.10 2017/09/08 05:36:53 deraadt Exp $	*/
/*	$NetBSD: random.c,v 1.2 1994/10/26 06:42:42 cgd Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (c) 1992, 1993
 *	The Regents of the University of California.  All rights reserved.
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
 *	@(#)random.c	8.1 (Berkeley) 6/10/93
 */
/* </LICENSES> */

//! `random()`: pseudo-random number generator for randomizing the profiling clock. The
//! result is uniform on [0, 2^31 - 1].
//!
//! Upstream: sys/lib/libkern/random.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - The C reads and writes `curcpu()->ci_randseed`; this leaf crate has no `curcpu()`, so
//!   the caller passes that field (`Machine::ci_randseed(curcpu())`).

use core::cell::Cell;

/// `random`: the next value of the Park-Miller generator seeded by `ci_randseed`.
pub fn random(ci_randseed: &Cell<u32>) -> u32 {
    // Compute x[n + 1] = (7^5 * x[n]) mod (2^31 - 1).
    // From "Random number generators: good ones are hard to find",
    // Park and Miller, Communications of the ACM, vol. 31, no. 10,
    // October 1988, p. 1195.
    let x = ci_randseed.get() as i32;
    let hi = x / 127_773;
    let lo = x % 127_773;
    let mut t = 16807i32
        .wrapping_mul(lo)
        .wrapping_sub(2836i32.wrapping_mul(hi));
    if t <= 0 {
        t = t.wrapping_add(0x7fff_ffff);
    }
    ci_randseed.set(t as u32);
    t as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn park_miller_from_a_known_seed() {
        // The minimal standard: seed 1 gives 16807, then 282475249.
        let seed = Cell::new(1);
        assert_eq!(random(&seed), 16807);
        assert_eq!(random(&seed), 282_475_249);
        assert_eq!(seed.get(), 282_475_249);
    }

    #[test]
    fn a_zero_seed_leaves_zero_behind_and_stays_in_range() {
        let seed = Cell::new(0);
        let v = random(&seed);
        assert_eq!(v, 0x7fff_ffff);
        for _ in 0..1000 {
            let v = random(&seed);
            assert!(v > 0 && v <= 0x7fff_ffff);
        }
    }
}
