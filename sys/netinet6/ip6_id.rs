/*	$KAME: ip6_id.c,v 1.8 2003/09/06 13:41:06 itojun Exp $	*/
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

/*
 * Copyright (C) 2003 WIDE Project.
 * All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. Neither the name of the project nor the names of its contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE PROJECT AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE PROJECT OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 */

/*
 * Copyright 1998 Niels Provos <provos@citi.umich.edu>
 * All rights reserved.
 *
 * Theo de Raadt <deraadt@openbsd.org> came up with the idea of using
 * such a mathematical system to generate more random (yet non-repeating)
 * ids to solve the resolver/named problem.  But Niels designed the
 * actual system based on the constraints.
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

/*
 * seed = random (bits - 1) bit
 * n = prime, g0 = generator to n,
 * j = random so that gcd(j,n-1) == 1
 * g = g0^j mod n will be a generator again.
 *
 * X[0] = random seed.
 * X[n] = a*X[n-1]+b mod m is a Linear Congruential Generator
 * with a = 7^(even random) mod m,
 *      b = random with gcd(b,m) == 1
 *      m = constant and a maximal period of m-1.
 *
 * The transaction id is determined by:
 * id[n] = seed xor (g^X[n] mod n)
 *
 * Effectively the id is restricted to the lower (bits - 1) bits, thus
 * yielding two different cycles by toggling the msb on and off.
 * This avoids reuse issues caused by reseeding.
 */
/* </LICENSES> */

/* <CODE> */
//! Random IPv6 flow labels: `netinet6/ip6_id.c`. A linear congruential generator walks a
//! cycle modulo `ru_m`; each step is mapped through `g^x mod n` (a generator of the prime
//! `n`) and xored with a seed, so the 20-bit labels do not repeat within `ru_max` draws.
//! The top bit flips at every reseed, giving two disjoint cycles.
//!
//! Upstream: sys/netinet6/ip6_id.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `randomtab_20` is a `StaticCell` guarded by a private mutex, `randomtab_mtx`. The C
//!   updates it without a lock (callers ran under the exclusive net lock); here
//!   `ip6_randomflowlabel` may run under the shared net lock on several CPUs.
//! - The constant part of `struct randomtab` (`ru_bits` .. `pfacts`) and its state are one
//!   struct with plain fields; the C marks the constants `const`.

use crate::dev::rnd::{arc4random, arc4random_uniform};
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_tc::getuptime;
use crate::machine::intr::IPL_SOFTNET;
use crate::sys::mutex::Mutex;
use crate::sys::types::Time;
use libkern::StaticCell;

/// `struct randomtab`: the parameters and state of one identifier generator.
pub struct Randomtab {
    /// `ru_bits`: resulting bits.
    pub ru_bits: u32,
    /// `ru_out`: time after which it is reseeded.
    pub ru_out: Time,
    /// `ru_max`: unique cycle, avoids blackjack prediction.
    pub ru_max: u32,
    /// `ru_gen`: starting generator.
    pub ru_gen: u32,
    /// `ru_n`: a prime; `ru_n - 1` is the product of `pfacts`.
    pub ru_n: u32,
    /// `ru_agen`: `ru_a` is `ru_agen^(2*rand)`.
    pub ru_agen: u32,
    /// `ru_m`: `2^x*3^y`.
    pub ru_m: u32,
    /// `pfacts`: the prime factors of `ru_n - 1`, zero-terminated.
    pub pfacts: [u32; 4],

    /// `ru_counter`: identifiers handed out since the last reseed.
    pub ru_counter: u32,
    /// `ru_msb`: the top bit of this cycle.
    pub ru_msb: u32,

    /// `ru_x`: the LCG state.
    pub ru_x: u32,
    /// `ru_seed`: xored into the result.
    pub ru_seed: u32,
    /// `ru_seed2`: added to the exponent.
    pub ru_seed2: u32,
    /// `ru_a`: LCG multiplier.
    pub ru_a: u32,
    /// `ru_b`: LCG increment.
    pub ru_b: u32,
    /// `ru_g`: the generator of this cycle.
    pub ru_g: u32,
    /// `ru_reseed`: uptime of the next reseed.
    pub ru_reseed: Time,
}

impl Randomtab {
    /// A generator with the given constants and a zero state.
    #[allow(clippy::too_many_arguments)] // the C initializer's eight constants
    pub const fn new(
        ru_bits: u32,
        ru_out: Time,
        ru_max: u32,
        ru_gen: u32,
        ru_n: u32,
        ru_agen: u32,
        ru_m: u32,
        pfacts: [u32; 4],
    ) -> Self {
        Self {
            ru_bits,
            ru_out,
            ru_max,
            ru_gen,
            ru_n,
            ru_agen,
            ru_m,
            pfacts,
            ru_counter: 0,
            ru_msb: 0,
            ru_x: 0,
            ru_seed: 0,
            ru_seed2: 0,
            ru_a: 0,
            ru_b: 0,
            ru_g: 0,
            ru_reseed: 0,
        }
    }
}

/// `randomtab_mtx` (not in C, see the deviations): guards [`RANDOMTAB_20`].
static RANDOMTAB_MTX: Mutex = Mutex::new(IPL_SOFTNET);

/// `randomtab_20`: the 20-bit flow-label generator; touched only under [`RANDOMTAB_MTX`].
static RANDOMTAB_20: StaticCell<Randomtab> = StaticCell::new(Randomtab::new(
    20,     // resulting bits
    180,    // Time after which will be reseeded
    200000, // Uniq cycle, avoid blackjack prediction
    2,      // Starting generator
    524269, // RU_N-1 = 2^2*3^2*14563
    7,      // determine ru_a as RU_AGEN^(2*rand)
    279936, // RU_M = 2^7*3^7 - don't change
    [2, 3, 14563, 0],
));

/// `ip6id_pmod`: fast modular exponentiation, `gen^expo mod mod_`, in `0..mod_`.
pub fn ip6id_pmod(gen_: u32, expo: u32, mod_: u32) -> u32 {
    let m = u64::from(mod_);
    let mut s: u64 = 1;
    let mut t = u64::from(gen_);
    let mut u = expo;

    while u != 0 {
        if u & 1 != 0 {
            s = (s * t) % m;
        }
        u >>= 1;
        t = (t * t) % m;
    }
    s as u32
}

/// `ip6id_initid`: a new seed and generator for `p`; toggles the top bit, so that the two
/// cycles of numbers before and after a reseed are distinct. Called from
/// [`ip6id_randomid`] when needed.
pub fn ip6id_initid(p: &mut Randomtab) {
    p.ru_x = arc4random_uniform(p.ru_m);

    // (bits - 1) bits of random seed
    let seedmask = !0u32 >> (32 - p.ru_bits + 1);
    p.ru_seed = arc4random() & seedmask;
    p.ru_seed2 = arc4random() & seedmask;

    // Determine the LCG we use
    let bitmask = !0u32 >> (32 - p.ru_bits);
    p.ru_b = (arc4random() & bitmask) | 1;
    p.ru_a = ip6id_pmod(p.ru_agen, (arc4random() & bitmask) & !1u32, p.ru_m);
    while p.ru_b.is_multiple_of(3) {
        p.ru_b += 2;
    }

    let mut j = arc4random_uniform(p.ru_n);

    // Fast gcd(j, RU_N - 1): find a j coprime to RU_N - 1, which makes RU_GEN^j mod RU_N
    // a generator again.
    loop {
        let divisible = p
            .pfacts
            .iter()
            .take_while(|&&f| f > 0)
            .any(|&f| j.is_multiple_of(f));
        if !divisible {
            break;
        }
        j = (j + 1) % p.ru_n;
    }

    p.ru_g = ip6id_pmod(p.ru_gen, j, p.ru_n);
    p.ru_counter = 0;

    p.ru_reseed = getuptime() + p.ru_out;
    p.ru_msb = if p.ru_msb != 0 {
        0
    } else {
        1u32 << (p.ru_bits - 1)
    };
}

/// `ip6id_randomid`: the next identifier of `p`.
pub fn ip6id_randomid(p: &mut Randomtab) -> u32 {
    if p.ru_counter >= p.ru_max || getuptime() > p.ru_reseed {
        ip6id_initid(p);
    }

    // Skip a random number of ids
    let n = arc4random() & 0x3;
    if p.ru_counter + n >= p.ru_max {
        ip6id_initid(p);
    }

    for _ in 0..=n {
        // Linear Congruential Generator
        p.ru_x = ((u64::from(p.ru_a) * u64::from(p.ru_x) + u64::from(p.ru_b)) % u64::from(p.ru_m))
            as u32;
    }

    p.ru_counter += n + 1;

    (p.ru_seed ^ ip6id_pmod(p.ru_g, p.ru_seed2.wrapping_add(p.ru_x), p.ru_n)) | p.ru_msb
}

/// `ip6_randomflowlabel`: a random 20-bit flow label, not repeating quickly.
pub fn ip6_randomflowlabel() -> u32 {
    mtx_enter(&RANDOMTAB_MTX);
    // SAFETY: RANDOMTAB_20 is touched only here, with RANDOMTAB_MTX held; no other
    // reference to it exists while this one lives.
    let id = ip6id_randomid(unsafe { RANDOMTAB_20.get_mut() });
    mtx_leave(&RANDOMTAB_MTX);
    id & 0xfffff
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn ip6id_pmod_matches_naive() {
        for &(g, e, m) in &[
            (2u32, 10u32, 1000u32),
            (7, 123, 279936),
            (3, 0, 17),
            (5, 40, 524269),
        ] {
            let naive = (0..e).fold(1u64, |s, _| s * u64::from(g) % u64::from(m));
            assert_eq!(u64::from(ip6id_pmod(g, e, m)), naive);
        }
    }

    #[test]
    fn ip6id_randomid_no_repeat_within_cycle() {
        let mut p = Randomtab::new(20, 180, 200000, 2, 524269, 7, 279936, [2, 3, 14563, 0]);
        ip6id_initid(&mut p);
        let msb = p.ru_msb;
        let mut seen = HashSet::new();
        for _ in 0..10000 {
            let id = ip6id_randomid(&mut p);
            assert_eq!(id & (1 << 19), msb);
            assert!(seen.insert(id), "id {id:#x} repeated");
        }
    }

    #[test]
    fn ip6_randomflowlabel_range() {
        for _ in 0..1000 {
            assert!(ip6_randomflowlabel() <= 0xfffff);
        }
    }
}
/* </TESTS> */
