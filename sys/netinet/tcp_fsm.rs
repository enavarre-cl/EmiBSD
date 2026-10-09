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
 * Copyright (c) 1982, 1986, 1993
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
 *	@(#)tcp_fsm.h	8.1 (Berkeley) 6/10/93
 */
/* </LICENSES> */

/* <CODE> */
//! TCP FSM state definitions, per RFC 793, September 1981: `<netinet/tcp_fsm.h>`.
//!
//! Upstream: sys/netinet/tcp_fsm.h @ 3ce1f3f79392
//!
//! Status: `ported`.
//!
//! ## Deviations
//! - `TCPS_HAVERCVDSYN(s)`, `TCPS_HAVEESTABLISHED(s)` and `TCPS_HAVERCVDFIN(s)` are `const fn`s
//!   ([`tcps_havercvdsyn`], ...).
//! - The tables the C compiles only into the file that defines `TCPOUTFLAGS` (`tcp_output.c`)
//!   or `TCPSTATES` (`tcp_debug.c`) are always present: `tcp_outflags[]` is
//!   [`TCP_OUTFLAGS`] and `tcpstates[]` is [`TCPSTATES`], a table of byte strings without the
//!   NUL (kernel strings are bytes, `docs/C_TO_RUST.md`).

use crate::netinet::tcp::{TH_ACK, TH_FIN, TH_RST, TH_SYN};

/// `TCP_NSTATES`: the number of states.
pub const TCP_NSTATES: usize = 11;

/// Closed.
pub const TCPS_CLOSED: i32 = 0;
/// Listening for connection.
pub const TCPS_LISTEN: i32 = 1;
/// Active, have sent syn.
pub const TCPS_SYN_SENT: i32 = 2;
/// Have sent and received syn.
pub const TCPS_SYN_RECEIVED: i32 = 3;
// States < TCPS_ESTABLISHED are those where connections not established.
/// Established.
pub const TCPS_ESTABLISHED: i32 = 4;
/// Received fin, waiting for close.
pub const TCPS_CLOSE_WAIT: i32 = 5;
// States > TCPS_CLOSE_WAIT are those where user has closed.
/// Have closed, sent fin.
pub const TCPS_FIN_WAIT_1: i32 = 6;
/// Closed, exchanged FIN; await ACK.
pub const TCPS_CLOSING: i32 = 7;
/// Had fin and close; await FIN ACK.
pub const TCPS_LAST_ACK: i32 = 8;
// States > TCPS_CLOSE_WAIT && < TCPS_FIN_WAIT_2 await ACK of FIN.
/// Have closed, fin is acked.
pub const TCPS_FIN_WAIT_2: i32 = 9;
/// In 2*msl quiet wait after close.
pub const TCPS_TIME_WAIT: i32 = 10;

/// `TCPS_HAVERCVDSYN(s)`: the connection has received a SYN.
#[inline]
pub const fn tcps_havercvdsyn(s: i32) -> bool {
    s >= TCPS_SYN_RECEIVED
}

/// `TCPS_HAVEESTABLISHED(s)`: the connection has been established.
#[inline]
pub const fn tcps_haveestablished(s: i32) -> bool {
    s >= TCPS_ESTABLISHED
}

/// `TCPS_HAVERCVDFIN(s)`: the connection has received a FIN.
#[inline]
pub const fn tcps_havercvdfin(s: i32) -> bool {
    s >= TCPS_TIME_WAIT
}

/// `tcp_outflags[]`: flags used when sending segments in `tcp_output`, by state. Basic flags
/// (`TH_RST`, `TH_ACK`, `TH_SYN`, `TH_FIN`) are totally determined by state, with the proviso
/// that `TH_FIN` is sent only if all data queued for output is included in the segment.
pub static TCP_OUTFLAGS: [u8; TCP_NSTATES] = [
    TH_RST | TH_ACK,
    0,
    TH_SYN,
    TH_SYN | TH_ACK,
    TH_ACK,
    TH_ACK,
    TH_FIN | TH_ACK,
    TH_FIN | TH_ACK,
    TH_FIN | TH_ACK,
    TH_ACK,
    TH_ACK,
];

/// `tcpstates[]`: the name of each state, indexed by `TCPS_*`.
pub static TCPSTATES: [&[u8]; TCP_NSTATES] = [
    b"CLOSED",
    b"LISTEN",
    b"SYN_SENT",
    b"SYN_RCVD",
    b"ESTABLISHED",
    b"CLOSE_WAIT",
    b"FIN_WAIT_1",
    b"CLOSING",
    b"LAST_ACK",
    b"FIN_WAIT_2",
    b"TIME_WAIT",
];
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn predicates_and_tables() {
        assert!(!tcps_havercvdsyn(TCPS_SYN_SENT) && tcps_havercvdsyn(TCPS_SYN_RECEIVED));
        assert!(!tcps_haveestablished(TCPS_SYN_RECEIVED));
        assert!(tcps_haveestablished(TCPS_ESTABLISHED));
        assert!(!tcps_havercvdfin(TCPS_FIN_WAIT_2) && tcps_havercvdfin(TCPS_TIME_WAIT));
        assert_eq!(TCPSTATES[TCPS_SYN_RECEIVED as usize], b"SYN_RCVD");
        assert_eq!(TCPSTATES[TCPS_TIME_WAIT as usize], b"TIME_WAIT");
        assert_eq!(TCP_OUTFLAGS[TCPS_CLOSED as usize], TH_RST | TH_ACK);
        assert_eq!(TCP_OUTFLAGS[TCPS_LAST_ACK as usize], TH_FIN | TH_ACK);
        assert_eq!(TCPS_TIME_WAIT as usize + 1, TCP_NSTATES);
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/netinet/tcp_fsm.h");
        let ours = crate::reftest::assert_defines!(defs;
            TCPS_CLOSED, TCPS_LISTEN, TCPS_SYN_SENT, TCPS_SYN_RECEIVED, TCPS_ESTABLISHED,
            TCPS_CLOSE_WAIT, TCPS_FIN_WAIT_1, TCPS_CLOSING, TCPS_LAST_ACK, TCPS_FIN_WAIT_2,
            TCPS_TIME_WAIT);
        crate::reftest::assert_complete(&defs, "TCPS_", &ours);
        crate::reftest::assert_defines!(defs; TCP_NSTATES);
    }
}
/* </TESTS> */
