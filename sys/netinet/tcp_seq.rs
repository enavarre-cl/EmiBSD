/*	$OpenBSD: tcp_seq.h,v 1.6 2007/06/15 18:23:06 markus Exp $	*/
/*	$NetBSD: tcp_seq.h,v 1.6 1995/03/26 20:32:35 jtc Exp $	*/
/* <LICENSES> */
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
 *	@(#)tcp_seq.h	8.1 (Berkeley) 6/10/93
 */
/* </LICENSES> */

//! TCP sequence number comparisons: `<netinet/tcp_seq.h>`.
//!
//! Upstream: sys/netinet/tcp_seq.h @ 3ce1f3f79392
//!
//! TCP sequence numbers are 32 bit integers operated on with modular arithmetic. These
//! functions compare such integers: `a` is before `b` when `b - a`, taken modulo 2^32, is
//! less than 2^31.
//!
//! Status: `ported`.
//!
//! ## Deviations
//! - `SEQ_LT`, `SEQ_LEQ`, `SEQ_GT`, `SEQ_GEQ` are `const fn`s over [`TcpSeq`]. The C's
//!   `(int)((a)-(b))` is `a.wrapping_sub(b) as i32`: the unsigned difference wraps and is read
//!   as a signed number, exactly as the C cast does.
//! - `tcp_rcvseqinit(tp)` and `tcp_sendseqinit(tp)` are functions over the control block of
//!   `netinet/tcp_var.rs`.
//! - `extern tcp_seq tcp_iss` is `TCP_ISS` in `netinet/tcp_subr.rs`, where the C defines it.

use crate::netinet::tcp::TcpSeq;
use crate::netinet::tcp_var::Tcpcb;

/// `TCP_ISSINCR`: increment for `tcp_iss` each second.
pub const TCP_ISSINCR: u32 = 125 * 1024;
/// `TCP_ISSINCR2`: increment for `tcp_iss` each second.
pub const TCP_ISSINCR2: u32 = 1024 * 1024;

/// `SEQ_LT(a, b)`: `a` comes before `b`.
#[inline]
pub const fn seq_lt(a: TcpSeq, b: TcpSeq) -> bool {
    (a.wrapping_sub(b) as i32) < 0
}

/// `SEQ_LEQ(a, b)`: `a` comes before `b` or is `b`.
#[inline]
pub const fn seq_leq(a: TcpSeq, b: TcpSeq) -> bool {
    (a.wrapping_sub(b) as i32) <= 0
}

/// `SEQ_GT(a, b)`: `a` comes after `b`.
#[inline]
pub const fn seq_gt(a: TcpSeq, b: TcpSeq) -> bool {
    (a.wrapping_sub(b) as i32) > 0
}

/// `SEQ_GEQ(a, b)`: `a` comes after `b` or is `b`.
#[inline]
pub const fn seq_geq(a: TcpSeq, b: TcpSeq) -> bool {
    (a.wrapping_sub(b) as i32) >= 0
}

/// `tcp_rcvseqinit(tp)`: initialize the receive sequence numbers from the initial receive
/// sequence number.
pub fn tcp_rcvseqinit(tp: &Tcpcb) {
    let v = tp.irs.get().wrapping_add(1);
    tp.rcv_nxt.set(v);
    tp.rcv_adv.set(v);
}

/// `tcp_sendseqinit(tp)`: initialize the send sequence numbers from the initial send
/// sequence number.
pub fn tcp_sendseqinit(tp: &Tcpcb) {
    let iss = tp.iss.get();
    tp.snd_up.set(iss);
    tp.snd_max.set(iss);
    tp.snd_nxt.set(iss);
    tp.snd_una.set(iss);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comparisons_wrap_around() {
        assert!(seq_lt(1, 2) && seq_leq(1, 2) && !seq_gt(1, 2) && !seq_geq(1, 2));
        assert!(seq_leq(7, 7) && seq_geq(7, 7) && !seq_lt(7, 7) && !seq_gt(7, 7));
        // 0xffff_fff0 is 32 before 0x10 once the counter wraps.
        assert!(seq_lt(0xffff_fff0, 0x10) && seq_gt(0x10, 0xffff_fff0));
        assert!(seq_leq(u32::MAX, 0) && seq_geq(0, u32::MAX));
        // Half the space away: the C's (int) of 0x8000_0000 is negative.
        assert!(seq_lt(0, 0x8000_0000) && seq_lt(0x8000_0000, 0));
        assert!(seq_gt(0, 0x7fff_ffff + 2) && seq_lt(0, 0x7fff_ffff));
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/netinet/tcp_seq.h");
        let ours = crate::reftest::assert_defines!(defs; TCP_ISSINCR, TCP_ISSINCR2);
        crate::reftest::assert_complete(&defs, "TCP_", &ours);
    }
}
