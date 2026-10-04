/*	$OpenBSD: tcp_timer.h,v 1.28 2025/12/31 03:47:04 jsg Exp $	*/
/*	$NetBSD: tcp_timer.h,v 1.6 1995/03/26 20:32:37 jtc Exp $	*/
/*	$OpenBSD: tcp_timer.c,v 1.88 2025/09/17 17:29:14 bluhm Exp $	*/
/*	$NetBSD: tcp_timer.c,v 1.14 1996/02/13 23:44:09 christos Exp $	*/
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
 *	@(#)tcp_timer.h	8.1 (Berkeley) 6/10/93
 */

/*
 * Copyright (c) 1982, 1986, 1988, 1990, 1993
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
 *	@(#)tcp_timer.c	8.1 (Berkeley) 6/10/93
 */
/* </LICENSES> */

//! TCP timers: `<netinet/tcp_timer.h>` (the timers, their constants and the arm/disarm
//! helpers) and `netinet/tcp_timer.c` (the timer callouts).
//!
//! Upstream: sys/netinet/tcp_timer.h @ 3ce1f3f79392
//! Upstream: sys/netinet/tcp_timer.c @ 3ce1f3f79392
//!
//! The `TCPT_REXMT` timer is used to force retransmissions. The TCP has the `TCPT_REXMT`
//! timer set whenever segments have been sent for which ACKs are expected but not yet
//! received. If an ACK is received which advances `tp->snd_una`, then the retransmit timer is
//! cleared (if there are no more outstanding segments) or reset to the base value (if there
//! are more ACKs expected). Whenever the retransmit timer goes off, we retransmit one
//! unacknowledged segment, and do a backoff on the retransmit timer.
//!
//! The `TCPT_PERSIST` timer is used to keep window size information flowing even if the
//! window goes shut. If all previous transmissions have been acknowledged (so that there are
//! no retransmissions in progress), and the window is too small to bother sending anything,
//! then we start the `TCPT_PERSIST` timer. When it expires, if the window is nonzero, we go
//! to transmit state. Otherwise, at intervals send a single byte into the peer's window to
//! force him to update our window information. We do this at most as often as
//! `TCPT_PERSMIN` time intervals, but no more frequently than the current estimate of
//! round-trip packet time. The `TCPT_PERSIST` timer is cleared whenever we receive a window
//! update from the peer.
//!
//! The `TCPT_KEEP` timer is used to keep connections alive. If an connection is idle (no
//! segments received) for `TCPTV_KEEP_INIT` amount of time, but not yet established, then we
//! drop the connection. Once the connection is established, if the connection is idle for
//! `TCPTV_KEEP_IDLE` time (and keepalives have been enabled on the socket), we begin to
//! probe the connection. We force the peer to send us a segment by sending
//! `<SEQ=SND.UNA-1><ACK=RCV.NXT><CTL=ACK>`. This segment is (deliberately) outside the
//! window, and should elicit an ack segment in response from the peer. If, despite the
//! `TCPT_KEEP` initiated segments we cannot elicit a response from a peer in `TCPT_MAXIDLE`
//! amount of time probing, then we drop the connection.
//!
//! Locks used to protect struct members in the `.c` file: \[T\] `tcp_timer_mtx`.
//!
//! ## Deviations
//! - `TCP_TIMER_INIT`, `TCP_TIMER_ARM`, `TCP_TIMER_DISARM`, `TCP_TIMER_ISARMED` and
//!   `TCPT_RANGESET` are the functions `tcp_timer_init`, `tcp_timer_arm`,
//!   `tcp_timer_disarm`, `tcp_timer_isarmed` and `tcpt_rangeset` (the C macro row of
//!   `docs/C_TO_RUST.md`); the range macro returns the value instead of assigning an lvalue.
//!   Timer numbers are `usize` (they index `t_timer[]`), times in milliseconds `i32` (the
//!   C's `int` constants); `tcp_timer_arm` takes the `u64` of `timeout_add_msec`.
//! - The `tcptimers[]` names (behind `TCPTIMERS`) are only compiled by `tcp_debug.c` with
//!   `TCPDEBUG`, which is not configured.

use crate::kern::kern_timeout::{timeout_add_msec, timeout_del};
use crate::netinet::in_pcb::{in_pcbref, in_pcbunref};
use crate::netinet::tcp_var::{TF_TIMER, Tcpcb, tcp_time};

// Definitions of the TCP timers.

/// Retransmit.
pub const TCPT_REXMT: usize = 0;
/// Retransmit persistence.
pub const TCPT_PERSIST: usize = 1;
/// Keep alive.
pub const TCPT_KEEP: usize = 2;
/// 2*msl quiet time timer.
pub const TCPT_2MSL: usize = 3;
/// Delayed ack timeout.
pub const TCPT_DELACK: usize = 4;

/// `TCPT_NTIMERS`.
pub const TCPT_NTIMERS: usize = 5;

// Time constants.

/// Max seg lifetime (hah!).
pub const TCPTV_MSL: i32 = tcp_time(30);
/// Base roundtrip time; if 0, no idea yet.
pub const TCPTV_SRTTBASE: i32 = 0;
/// Assumed RTT if no info.
pub const TCPTV_SRTTDFLT: i32 = tcp_time(3);

/// Retransmit persistence.
pub const TCPTV_PERSMIN: i32 = tcp_time(5);
/// Maximum persist interval.
pub const TCPTV_PERSMAX: i32 = tcp_time(60);

/// Initial connect keep alive.
pub const TCPTV_KEEPINIT: i32 = tcp_time(75);
/// Dflt time before probing.
pub const TCPTV_KEEPIDLE: i32 = tcp_time(120 * 60);
/// Default probe interval.
pub const TCPTV_KEEPINTVL: i32 = tcp_time(75);
/// Max probes before drop.
pub const TCPTV_KEEPCNT: i32 = 8;

/// Minimum allowable value.
pub const TCPTV_MIN: i32 = tcp_time(1);
/// Max allowable REXMT value.
pub const TCPTV_REXMTMAX: i32 = tcp_time(64);

/// Linger at most 2 minutes.
pub const TCP_LINGERTIME: i16 = 120;

/// Maximum retransmits.
pub const TCP_MAXRXTSHIFT: i32 = 12;

/// Time to delay ACK.
pub const TCP_DELACK_MSECS: i32 = 200;

/// `TCP_TIMER_ARM(tp, timer, msecs)`: a newly pending timer holds a reference on the internet
/// control block, which the callout or `tcp_timer_disarm` gives back.
pub fn tcp_timer_arm(tp: &Tcpcb, timer: usize, msecs: u64) {
    tp.set_flags(TF_TIMER << timer);
    if timeout_add_msec(&tp.t_timer[timer], msecs) {
        in_pcbref(Some(tp.t_inpcb));
    }
}

/// `TCP_TIMER_DISARM(tp, timer)`.
pub fn tcp_timer_disarm(tp: &Tcpcb, timer: usize) {
    tp.clear_flags(TF_TIMER << timer);
    if timeout_del(&tp.t_timer[timer]) {
        in_pcbunref(Some(tp.t_inpcb));
    }
}

/// `TCP_TIMER_ISARMED(tp, timer)`.
pub fn tcp_timer_isarmed(tp: &Tcpcb, timer: usize) -> bool {
    tp.has_flags(TF_TIMER << timer)
}

/// `TCPT_RANGESET(tv, value, tvmin, tvmax)`: force a time value to be in a certain range;
/// returns what the C assigns to `tv`.
pub fn tcpt_rangeset<T: PartialOrd>(value: T, tvmin: T, tvmax: T) -> T {
    if value < tvmin {
        tvmin
    } else if value > tvmax {
        tvmax
    } else {
        value
    }
}
