/* <LICENSES> */
/*
 * Copyright (c) 2008 Theo de Raadt, Ryan McBride
 *
 * Slightly different algorithm from the one designed by
 * Matthew Dillon <dillon@backplane.com> for The DragonFly Project
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
//! Random IP sequence number generator: `ip_randomid`.
//!
//! Upstream: sys/netinet/ip_id.c @ 3ce1f3f79392
//!
//! Use the system PRNG to shuffle the 65536 entry ID space. We reshuffle the ID we pick out
//! of the array into the previous 32767 cells, providing a guarantee that an ID will not be
//! reused for at least 32768 calls.
//!
//! Status: `ported` (M7b).
//!
//! ## Deviations
//! - `ip_shuffle` and `isindex` are atomics: the C updates them without a lock from every
//!   output path; relaxed loads and stores are the same racy updates, without a `static mut`.

use core::sync::atomic::{AtomicU16, AtomicU32, Ordering};

use crate::dev::rnd::{arc4random_buf, arc4random_uniform};

/// `ip_shuffle[]`: a permutation of the ID space.
static IP_SHUFFLE: [AtomicU16; 65536] = [const { AtomicU16::new(0) }; 65536];
/// `isindex`.
static ISINDEX: AtomicU32 = AtomicU32::new(0);

/// `ip_randomid`: a random IP id. Shuffle the new value we get into the previous half of the
/// `ip_shuffle` ring (-32767 or swap with ourself), to avoid duplicates occurring too quickly
/// but also still be random. 0 is a special IP ID -- don't return it.
pub fn ip_randomid() -> u16 {
    loop {
        let mut si = [0u8; 2];
        arc4random_buf(&mut si);
        let si = u16::from_ne_bytes(si);
        let isindex = ISINDEX.load(Ordering::Relaxed);
        let i = (isindex & 0xffff) as usize;
        let i2 = (isindex.wrapping_sub(u32::from(si & 0x7fff)) & 0xffff) as usize;
        let r = IP_SHUFFLE[i].load(Ordering::Relaxed);
        IP_SHUFFLE[i].store(IP_SHUFFLE[i2].load(Ordering::Relaxed), Ordering::Relaxed);
        IP_SHUFFLE[i2].store(r, Ordering::Relaxed);
        ISINDEX.store(isindex.wrapping_add(1), Ordering::Relaxed);
        if r != 0 {
            return r;
        }
    }
}

/// `ip_randomid_init`: initialize with a random permutation. Do so using Knuth which avoids
/// the exchange in the Durstenfeld shuffle (see "The Art of Computer Programming, Vol 2" 3rd
/// ed, pg. 145).
pub fn ip_randomid_init() {
    for (i, slot) in IP_SHUFFLE.iter().enumerate() {
        let i2 = arc4random_uniform(i as u32 + 1) as usize;
        slot.store(IP_SHUFFLE[i2].load(Ordering::Relaxed), Ordering::Relaxed);
        IP_SHUFFLE[i2].store(i as u16, Ordering::Relaxed);
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_a_permutation_and_never_zero() {
        // The network tests that send packets draw ids too: serialise with them.
        let _g = crate::kern::uipc_mbuf::tests::setup();
        ip_randomid_init();
        let mut seen = std::vec![false; 65536];
        for s in &IP_SHUFFLE {
            let v = usize::from(s.load(Ordering::Relaxed));
            assert!(!seen[v], "{v} twice");
            seen[v] = true;
        }
        let mut last = std::vec::Vec::new();
        for _ in 0..1000 {
            let id = ip_randomid();
            assert_ne!(id, 0);
            assert!(!last.contains(&id), "{id} reused within 1000 calls");
            last.push(id);
        }
    }
}
/* </TESTS> */
