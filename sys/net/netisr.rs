/* <LICENSES> */
/*
 * Copyright (c) 1980, 1986, 1989, 1993
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
 *	@(#)netisr.h	8.1 (Berkeley) 6/10/93
 */
/* </LICENSES> */

/* <CODE> */
//! Network software interrupt bits: `<net/netisr.h>`.
//!
//! Upstream: sys/net/netisr.h @ 3ce1f3f79392
//!
//! Each "pup-level-1" input queue has a bit in a "netisr" status word which is used to
//! de-multiplex a single software interrupt used for scheduling the network code to calls on
//! the lowest level routine of each protocol. The constants are bit numbers.
//!
//! Status: `ported` (M7b).
//!
//! ## Deviations
//! - `netisr` (the status word) and `if_input_task_locked` are defined in `net/if_.rs`, where
//!   `net/if.c` defines them, and re-exported here; `schednetisr(anisr)` is a function. The
//!   `*intr()` prototypes come with their `.c` files (`arpintr`, `ipintr` are not ported).
//! - `schednetisr` before `softnet_init` (no softnet task queue yet) sets the bit only; the C
//!   would dereference the NULL queue.

use core::sync::atomic::Ordering;

use crate::kern::kern_task::task_add;
use crate::net::if_::net_tq;

pub use crate::net::if_::{IF_INPUT_TASK_LOCKED, NETISR};

/// Same as `AF_INET`.
pub const NETISR_IP: i32 = 2;
/// Same as `AF_LINK`.
pub const NETISR_ARP: i32 = 18;
/// Same as `AF_INET6`.
pub const NETISR_IPV6: i32 = 24;
/// For PPP processing.
pub const NETISR_PPP: i32 = 28;
/// For bridge processing.
pub const NETISR_BRIDGE: i32 = 29;
/// For pppoe processing.
pub const NETISR_PPPOE: i32 = 30;

/// `schednetisr(anisr)`: marks protocol queue `anisr` (`NETISR_*`) for `if_netisr` and
/// queues that task on the first softnet task queue.
pub fn schednetisr(anisr: i32) {
    NETISR.fetch_or(1 << anisr, Ordering::Relaxed);
    if let Some(tq) = net_tq(0) {
        let _ = task_add(tq, &IF_INPUT_TASK_LOCKED);
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/net/netisr.h");
        let ours = crate::reftest::assert_defines!(defs;
            NETISR_IP, NETISR_ARP, NETISR_IPV6, NETISR_PPP, NETISR_BRIDGE, NETISR_PPPOE);
        crate::reftest::assert_complete(&defs, "NETISR_", &ours);
    }
}
/* </TESTS> */
