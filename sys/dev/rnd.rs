/* <LICENSES> */
/*
 * Copyright (c) 2011,2020 Theo de Raadt.
 * Copyright (c) 2008 Damien Miller.
 * Copyright (c) 1996, 1997, 2000-2002 Michael Shalayeff.
 * Copyright (c) 2013 Markus Friedl.
 * Copyright Theodore Ts'o, 1994, 1995, 1996, 1997, 1998, 1999.
 * All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, and the entire permission notice in its entirety,
 *    including the disclaimer of warranties.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. The name of the author may not be used to endorse or promote
 *    products derived from this software without specific prior
 *    written permission.
 *
 * ALTERNATIVELY, this product may be distributed under the terms of
 * the GNU Public License, in which case the provisions of the GPL are
 * required INSTEAD OF the above restrictions.  (This clause is
 * necessary due to a potential bad interaction between the GPL and
 * the restrictions contained in a BSD-style copyright.)
 *
 * THIS SOFTWARE IS PROVIDED ``AS IS'' AND ANY EXPRESS OR IMPLIED
 * WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES
 * OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE, ALL OF
 * WHICH ARE HEREBY DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR BE
 * LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
 * CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
 * SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR
 * BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF
 * LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING
 * NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
 * SOFTWARE, EVEN IF NOT ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! Random number generation for the kernel: `dev/rnd.c`.
//!
//! Upstream: sys/dev/rnd.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M3 ports the interfaces the allocators call (`arc4random`,
//! `arc4random_buf`, `arc4random_uniform`) over a placeholder stream. The entropy pool
//! (`enqueue_randomness`, the ring, the timeouts), the ChaCha20 keystream (`_rs_*`), the
//! rekeying, `random_start`, the `randomread`/`randomwrite` device and the sysctls arrive with
//! M5, which brings the clock, timeouts and the entropy sources.
//!
//! M8: `sys_getentropy` (`getentropy(2)`, `GETENTROPY_MAX` bytes of the stream below).
//!
//! ## Deviations
//! - NOT RANDOM YET. The stream behind `arc4random` is SplitMix64 from a constant seed: it
//!   gives the pools their freelist order and page magics and `XSIMPLEQ` its cookies with the
//!   C's interfaces, and nothing more. Nothing security-relevant may rely on it until the
//!   ChaCha20 generator and the entropy pool land; `random_start` reports the gap.
//! - `rndlock` (M5) is not here; the generator is one atomic word, which `fetch_add`
//!   advances safely from any CPU (`MULTIPROCESSOR`) without the lock.

use core::sync::atomic::{AtomicU64, Ordering};

use crate::machine::copy::copyout;
use crate::sys::errno::Errno;
use crate::sys::proc::Proc;
use crate::sys::syscallargs::SysGetentropyArgs;
use crate::sys::syslimits::GETENTROPY_MAX;
use crate::sys::systm::{SysArgs, sysargs};
use crate::sys::types::Register;
use crate::unported;

/// The constant seed of the placeholder stream (see the module's deviations).
const PLACEHOLDER_SEED: u64 = 0x0b5d_7a2f_4c3e_9d81;
/// SplitMix64's increment.
const SPLITMIX_GAMMA: u64 = 0x9e37_79b9_7f4a_7c15;

/// The placeholder stream's state (`rs` in the C, a ChaCha20 context).
static RS_STATE: AtomicU64 = AtomicU64::new(PLACEHOLDER_SEED);

/// `_rs_random_u32`'s stand-in: the next 64 bits of the placeholder stream.
fn rs_random_u64() -> u64 {
    let mut z = RS_STATE
        .fetch_add(SPLITMIX_GAMMA, Ordering::Relaxed)
        .wrapping_add(SPLITMIX_GAMMA);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// `arc4random`: a 32-bit value from the stream.
pub fn arc4random() -> u32 {
    // mtx_enter(&rndlock): not needed by the one-word placeholder state (deviations).
    rs_random_u64() as u32
}

/// `arc4random_buf`: fills a buffer of arbitrary length from the stream.
pub fn arc4random_buf(buf: &mut [u8]) {
    // mtx_enter(&rndlock): not needed by the one-word placeholder state (deviations).
    for chunk in buf.chunks_mut(8) {
        let word = rs_random_u64().to_le_bytes();
        chunk.copy_from_slice(&word[..chunk.len()]);
    }
}

/// `arc4random_uniform`: a value in `[0, upper_bound)` without modulo bias.
pub fn arc4random_uniform(upper_bound: u32) -> u32 {
    if upper_bound < 2 {
        return 0;
    }

    // 2**32 % x == (2**32 - x) % x
    let min = upper_bound.wrapping_neg() % upper_bound;

    // This could theoretically loop forever but each retry has p > 0.5 (worst case, usually
    // far better) of selecting a number inside the range we need, so it should rarely need
    // to re-roll.
    loop {
        let r = arc4random();
        if r >= min {
            return r % upper_bound;
        }
    }
}

/// `enqueue_randomness`: feeds an entropy sample; the pool arrives with M5.
pub fn enqueue_randomness(_val: u32) {
    let _ = unported!("enqueue_randomness (the entropy pool, M5)");
}

/// `random_start`: starts the generator from the entropy pool; the pool arrives with M5.
pub fn random_start(_goodseed: bool) {
    let _ = unported!("random_start (ChaCha20 and the entropy pool, M5)");
}

/// `getentropy(2)`: at most `GETENTROPY_MAX` bytes from the kernel's generator (see the
/// module's deviations: not random yet).
pub fn sys_getentropy(_p: &Proc, v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysGetentropyArgs = sysargs(v);
    let mut buf = [0u8; GETENTROPY_MAX];
    let nbyte = uap.nbyte.get();

    if nbyte > buf.len() {
        return Err(Errno::EINVAL);
    }
    arc4random_buf(&mut buf[..nbyte]);
    copyout(&buf[..nbyte], uap.buf.get() as usize)?;
    libkern::explicit_bzero(&mut buf);
    retval[0] = 0;
    Ok(())
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_moves_and_fills() {
        let a = arc4random();
        let b = arc4random();
        assert_ne!(a, b);
        let mut buf = [0u8; 13];
        arc4random_buf(&mut buf);
        assert!(buf.iter().any(|&x| x != 0));
    }

    #[test]
    fn uniform_stays_below_the_bound() {
        assert_eq!(arc4random_uniform(0), 0);
        assert_eq!(arc4random_uniform(1), 0);
        for _ in 0..1000 {
            assert!(arc4random_uniform(7) < 7);
        }
    }
}
/* </TESTS> */
