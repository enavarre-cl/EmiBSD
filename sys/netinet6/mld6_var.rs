/*	$OpenBSD: mld6_var.h,v 1.10 2026/02/26 00:53:18 bluhm Exp $	*/
/*	$KAME: mld6_var.h,v 1.4 2000/03/25 07:23:54 sumikawa Exp $	*/
/* <LICENSES> */
/*
 * Copyright (C) 1998 WIDE Project.
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
/* </LICENSES> */

/* <CODE> */
//! Multicast Listener Discovery, implementation-specific definitions: the listening states
//! and the report a membership change asks for: `<netinet6/mld6_var.h>`.
//!
//! Upstream: sys/netinet6/mld6_var.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - `struct mld6_pktinfo` has no `mpi_list`: the list of reports `mld6_fasttimo` collects
//!   (`struct mld6_pktlist`) is a `Vec` of them (`netinet6/mld6.rs`), as `igmp_pktinfo`'s is
//!   (`netinet/igmp_var.rs`).
//! - `MLD_RANDOM_DELAY(X)` is [`mld_random_delay`]. The prototypes are `netinet6/mld6.rs`'s.

use crate::dev::rnd::arc4random_uniform;
use crate::netinet6::in6::In6Addr;

// States for MLD stop-listening processing

/// `MLD_OTHERLISTENER`.
pub const MLD_OTHERLISTENER: u32 = 0;
/// `MLD_IREPORTEDLAST`.
pub const MLD_IREPORTEDLAST: u32 = 1;

/// `struct mld6_pktinfo`: the report or done message a membership change asks
/// `mld6_sendpkt` to send (`mpi_ifidx` 0: none).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mld6Pktinfo {
    /// `mpi_addr`: the group.
    pub mpi_addr: In6Addr,
    /// `mpi_rdomain`.
    pub mpi_rdomain: u32,
    /// `mpi_ifidx`: the interface the message leaves on.
    pub mpi_ifidx: u32,
    /// `mpi_type`: the MLD message type.
    pub mpi_type: i32,
}

/// `MLD_RANDOM_DELAY(X)`: a random timer value between 1 and `x`.
pub fn mld_random_delay(x: u32) -> u32 {
    arc4random_uniform(x) + 1
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/netinet6/mld6_var.h");
        crate::reftest::assert_defines!(defs; MLD_OTHERLISTENER, MLD_IREPORTEDLAST);
    }
}
/* </TESTS> */
