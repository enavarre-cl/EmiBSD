/*	$OpenBSD: tcp_var.h,v 1.196 2025/09/16 17:29:35 bluhm Exp $	*/
/*	$NetBSD: tcp_var.h,v 1.17 1996/02/13 23:44:24 christos Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1982, 1986, 1993, 1994
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
 *	@(#)tcp_var.h	8.3 (Berkeley) 4/10/94
 */
/* </LICENSES> */

//! Kernel variables for TCP: `<netinet/tcp_var.h>`.
//!
//! Upstream: sys/netinet/tcp_var.h @ 3ce1f3f79392
//!
//! The TCP control block ([`Tcpcb`], one per connection, hung off the internet control block
//! as `inp_ppcb`), the reassembly queue entries, the SACK blocks and holes, the SYN cache, the
//! statistics with their per-counter enumeration, and the `net.inet.tcp` sysctl names.
//!
//! Locks used to protect global data and struct members: \[I\] immutable after creation,
//! \[N\] net lock, \[S\] `syn_cache_mtx`, \[s\] `so_lock` of the listen socket.
//!
//! ## Deviations
//! - `struct tcpcb` is a `tcpcb_pool` item of `Cell`s (the `struct mbuf` idiom of
//!   `docs/C_TO_RUST.md`): every layer changes it through a shared pointer under the socket
//!   lock. `t_inpcb` is set once by `tcp_newtcpcb` and is a `&'static Inpcb`. `t_state` is an
//!   `i32` (a `short` in C) so that it compares with the `TCPS_*` constants without casts;
//!   `t_force` is a `bool` (a `char` used as a flag); `t_softerror` is an `Option<Errno>` (a
//!   `short` holding an errno or 0). `sackblks[]` is an array of `Cell<Sackblk>`.
//! - `struct tcpqent`'s `tcpqe_tcp`, the C's pointer to the segment's TCP header inside the
//!   mbuf, is a copy of the header (`Cell<Tcphdr>`), host order as `tcp_input` leaves it:
//!   only its sequence number, the length kept in `th_reseqlen` and the flags are read or
//!   changed, and the mbuf data no longer covers the header once `tcp_input` has trimmed it.
//! - `struct tcp_opt_info`'s `ts_present` is a `bool`.
//! - `union syn_cache_sa` has the members and the size of `union sockaddr_union`; it is that
//!   type ([`SockaddrUnion`], a byte image with accessors, `netinet/ip_ipsp.rs`).
//! - `struct syn_cache`'s `sc_request_r_scale:4` and `sc_requested_s_scale:4` bit-fields are
//!   two `u8`s (the structure is kernel internal). `sc_buckethead` is a raw pointer into the
//!   set's bucket array, `sc_set` a reference to one of the two static sets.
//!   `struct syn_cache_set`'s `scs_buckethead` is the `malloc`ed bucket array as a slice, as
//!   `inpt_hashtbl` is in `in_pcb.rs`.
//! - `tcpcounters` (`struct cpumem *`) is the static array of atomics `TCPCOUNTERS`
//!   (`tcp_subr.rs`); `tcpstat_inc`, `tcpstat_add` and `tcpstat_pkt` bump it without the
//!   `splnet()` (atomics, as for `udpstat_inc`).
//! - `TCP_REXMTVAL(tp)` and `TCP_TIME(sec)` are the functions `tcp_rexmtval` and `tcp_time`
//!   (the C macro row of `docs/C_TO_RUST.md`); `intotcpcb`/`sototcpcb` return `Option`.
//! - `TCPCTL_NAMES` is for `sysctl(8)` and is not compiled; `struct tcp_ident_mapping` has
//!   its trailing padding as a named member (`AbiPod`).
//! - The `INET6` declarations (`tcp6_usrreqs`, `tcb6table`, `tcp6_ctlinput`,
//!   `tcp6_mtudisc_callback`) are the items of `tcp_usrreq.rs` and `tcp_subr.rs`, compiled
//!   always, as `netinet6` is. The `sin6` member of `union syn_cache_sa` is read and written
//!   through `tcp_input.rs`'s `sa_sin6`/`sa_from_sin6`. `TCP_ECN` and `TCP_SIGNATURE` are (GENERIC), so the `TF_ECN_*`
//!   flags and `tcp_signature` exist. `SMALL_KERNEL` is not set: `tcp_trace` is the function of
//!   `tcp_debug.rs`.

use core::cell::Cell;
use core::ptr;
use core::sync::atomic::Ordering;

use crate::kern::kern_tc::getnsecruntime;
use crate::machine::copy::AbiPod;
use crate::net::route::Route;
use crate::netinet::in_pcb::{Inpcb, sotoinpcb};
use crate::netinet::ip_ipsp::SockaddrUnion;
use crate::netinet::tcp::{MAX_SACK_BLKS, TcpSeq, Tcphdr};
use crate::netinet::tcp_subr::{TCP_STARTTIME, TCPCOUNTERS};
use crate::netinet::tcp_timer::TCPT_NTIMERS;
use crate::queue_adapter;
use crate::sys::errno::Errno;
use crate::sys::mbuf::Mbuf;
use crate::sys::queue::{ListEntry, ListHead, TailqEntry, TailqHead};
use crate::sys::refcnt::Refcnt;
use crate::sys::socket::SockaddrStorage;
use crate::sys::socketvar::Socket;
use crate::sys::timeout::Timeout;

// t_flags.

/// Ack peer immediately.
pub const TF_ACKNOW: u32 = 0x0001;
/// Don't delay packets to coalesce.
pub const TF_NODELAY: u32 = 0x0004;
/// Don't use tcp options.
pub const TF_NOOPT: u32 = 0x0008;
/// Have sent FIN.
pub const TF_SENTFIN: u32 = 0x0010;
/// Have/will request window scaling.
pub const TF_REQ_SCALE: u32 = 0x0020;
/// Other side has requested scaling.
pub const TF_RCVD_SCALE: u32 = 0x0040;
/// Have/will request timestamps.
pub const TF_REQ_TSTMP: u32 = 0x0080;
/// A timestamp was received in SYN.
pub const TF_RCVD_TSTMP: u32 = 0x0100;
/// Other side said I could SACK.
pub const TF_SACK_PERMIT: u32 = 0x0200;
/// Require TCP MD5 signature.
pub const TF_SIGNATURE: u32 = 0x0400;
/// Other side said I could ECN (`TCP_ECN`).
pub const TF_ECN_PERMIT: u32 = 0x0000_8000;
/// Send ECE in subsequent segs (`TCP_ECN`).
pub const TF_RCVD_CE: u32 = 0x0001_0000;
/// Send CWR in next seg (`TCP_ECN`).
pub const TF_SEND_CWR: u32 = 0x0002_0000;
/// Disable ECN for this connection (`TCP_ECN`).
pub const TF_DISABLE_ECN: u32 = 0x0004_0000;
/// No outstanding ACK on last send.
pub const TF_LASTIDLE: u32 = 0x0010_0000;
/// Path MTU Discovery pending.
pub const TF_PMTUD_PEND: u32 = 0x0040_0000;
/// Call tcp_output after tcp_input.
pub const TF_NEEDOUTPUT: u32 = 0x0080_0000;
/// Don't push.
pub const TF_NOPUSH: u32 = 0x0200_0000;
/// Retransmit timer armed.
pub const TF_TMR_REXMT: u32 = 0x0400_0000;
/// Retransmit persistence timer armed.
pub const TF_TMR_PERSIST: u32 = 0x0800_0000;
/// Keep alive timer armed.
pub const TF_TMR_KEEP: u32 = 0x1000_0000;
/// 2*msl quiet time timer armed.
pub const TF_TMR_2MSL: u32 = 0x2000_0000;
/// Delayed ack timer armed.
pub const TF_TMR_DELACK: u32 = 0x4000_0000;
/// Used to shift with TCPT values.
pub const TF_TIMER: u32 = TF_TMR_REXMT;

// t_oobflags.

/// `TCPOOB_HAVEDATA`.
pub const TCPOOB_HAVEDATA: u8 = 0x01;
/// `TCPOOB_HADDATA`.
pub const TCPOOB_HADDATA: u8 = 0x02;

/// `TCP_SYN_HASH_SIZE`.
pub const TCP_SYN_HASH_SIZE: i32 = 293;
/// `TCP_SYN_BUCKET_SIZE`.
pub const TCP_SYN_BUCKET_SIZE: i32 = 35;

/// We've had an unreach error (`sc_dynflags`).
pub const SCF_UNREACH: u32 = 0x0001;
/// Peer will do timestamps (`sc_fixflags`).
pub const SCF_TIMESTAMP: u16 = 0x0010;
/// Permit sack.
pub const SCF_SACK_PERMIT: u16 = 0x0020;
/// Permit ecn.
pub const SCF_ECN_PERMIT: u16 = 0x0040;
/// Enforce tcp signatures.
pub const SCF_SIGNATURE: u16 = 0x0080;

// The smoothed round-trip time and estimated variance are stored as fixed point numbers
// scaled by the values below. For convenience, these scales are also used in smoothing the
// average (smoothed = (1/scale)sample + ((scale-1)/scale)smoothed). With these scales, srtt
// has 5 bits to the right of the binary point, and thus an "ALPHA" of 0.875. rttvar has 4
// bits to the right of the binary point, and is smoothed with an ALPHA of 0.75.

/// Shift for srtt; 5 bits frac.
pub const TCP_RTT_SHIFT: i32 = 3;
/// Shift for rttvar; 4 bits.
pub const TCP_RTTVAR_SHIFT: i32 = 2;
/// Remaining 2 bit shift.
pub const TCP_RTT_BASE_SHIFT: i32 = 2;
/// Maximum rtt.
pub const TCP_RTT_MAX: i32 = 1 << 18;

// Names for TCP sysctl objects.

/// Enable RFC1323 timestamps/scaling.
pub const TCPCTL_RFC1323: i32 = 1;
/// TCPT_KEEP value.
pub const TCPCTL_KEEPINITTIME: i32 = 2;
/// Allow tcp_keepidle to be changed.
pub const TCPCTL_KEEPIDLE: i32 = 3;
/// Allow tcp_keepintvl to be changed.
pub const TCPCTL_KEEPINTVL: i32 = 4;
/// Return kernel idea of PR_SLOWHZ.
pub const TCPCTL_SLOWHZ: i32 = 5;
/// Return bad dynamic port bitmap.
pub const TCPCTL_BADDYNAMIC: i32 = 6;
/// Receive buffer space.
pub const TCPCTL_RECVSPACE: i32 = 7;
/// Send buffer space.
pub const TCPCTL_SENDSPACE: i32 = 8;
/// Get connection owner.
pub const TCPCTL_IDENT: i32 = 9;
/// Selective acknowledgement, rfc 2018.
pub const TCPCTL_SACK: i32 = 10;
/// Default maximum segment size.
pub const TCPCTL_MSSDFLT: i32 = 11;
/// RST pps limit.
pub const TCPCTL_RSTPPSLIMIT: i32 = 12;
/// ACK immediately on PUSH.
pub const TCPCTL_ACK_ON_PUSH: i32 = 13;
/// RFC3168 ECN.
pub const TCPCTL_ECN: i32 = 14;
/// Max size of comp. state engine.
pub const TCPCTL_SYN_CACHE_LIMIT: i32 = 15;
/// Max size of hash bucket.
pub const TCPCTL_SYN_BUCKET_LIMIT: i32 = 16;
/// Enable/disable RFC3390 increased cwnd.
pub const TCPCTL_RFC3390: i32 = 17;
/// Max entries for tcp reass queues.
pub const TCPCTL_REASS_LIMIT: i32 = 18;
/// Drop tcp connection.
pub const TCPCTL_DROP: i32 = 19;
/// Max entries for tcp sack queues.
pub const TCPCTL_SACKHOLE_LIMIT: i32 = 20;
/// TCP statistics.
pub const TCPCTL_STATS: i32 = 21;
/// Assume SO_KEEPALIVE is always set.
pub const TCPCTL_ALWAYS_KEEPALIVE: i32 = 22;
/// Number of uses before reseeding hash.
pub const TCPCTL_SYN_USE_LIMIT: i32 = 23;
/// Return root only port bitmap.
pub const TCPCTL_ROOTONLY: i32 = 24;
/// Number of buckets in the hash.
pub const TCPCTL_SYN_HASH_SIZE: i32 = 25;
/// Enable TCP segmentation offload.
pub const TCPCTL_TSO: i32 = 26;
/// `TCPCTL_MAXID`.
pub const TCPCTL_MAXID: i32 = 27;

/// `struct sackblk`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sackblk {
    /// Start seq no. of sack block.
    pub start: TcpSeq,
    /// End seq no.
    pub end: TcpSeq,
}

/// `struct sackhole`: a `sackhl_pool` item on the sender's sorted list of holes.
pub struct Sackhole {
    /// Start seq no. of hole.
    pub start: Cell<TcpSeq>,
    /// End seq no.
    pub end: Cell<TcpSeq>,
    /// Number of dup(s)acks for this hole.
    pub dups: Cell<i32>,
    /// Next seq. no in hole to be retransmitted.
    pub rxmit: Cell<TcpSeq>,
    /// Next in list.
    pub next: Cell<Option<&'static Sackhole>>,
}

impl Sackhole {
    /// A zeroed hole, as `pool_get(PR_ZERO)` would hand it out.
    pub const fn new() -> Self {
        Self {
            start: Cell::new(0),
            end: Cell::new(0),
            dups: Cell::new(0),
            rxmit: Cell::new(0),
            next: Cell::new(None),
        }
    }
}

impl Default for Sackhole {
    fn default() -> Self {
        Self::new()
    }
}

queue_adapter!(
    /// `TAILQ_ENTRY(tcpqent) tcpqe_q`.
    pub TcpqeQ: Tcpqent, tcpqe_q => TailqEntry<Tcpqent>
);

/// `TAILQ_HEAD(tcpqehead, tcpqent)`: the TCP sequence queue.
pub type Tcpqehead = TailqHead<TcpqeQ>;

/// `struct tcpqent`: a `tcpqe_pool` item holding an out-of-order segment.
pub struct Tcpqent {
    /// `tcpqe_q`.
    pub tcpqe_q: TailqEntry<Tcpqent>,
    /// `tcpqe_tcp`: the segment's TCP header, host order (see the deviations).
    pub tcpqe_tcp: Cell<Tcphdr>,
    /// Mbuf contains packet.
    pub tcpqe_m: Cell<Option<&'static Mbuf>>,
}

impl Tcpqent {
    /// An entry for `m` with header `th`.
    pub const fn new(th: Tcphdr, m: Option<&'static Mbuf>) -> Self {
        Self {
            tcpqe_q: TailqEntry::new(),
            tcpqe_tcp: Cell::new(th),
            tcpqe_m: Cell::new(m),
        }
    }
}

queue_adapter!(
    /// `LIST_ENTRY(syn_cache) sc_tpq`: list of entries by same tp.
    pub ScTpq: SynCache, sc_tpq => ListEntry<SynCache>
);

queue_adapter!(
    /// `TAILQ_ENTRY(syn_cache) sc_bucketq`: link on bucket list.
    pub ScBucketq: SynCache, sc_bucketq => TailqEntry<SynCache>
);

/// `struct tcpcb`: TCP control block, one per tcp. Every member is changed under the socket
/// lock of the connection.
pub struct Tcpcb {
    /// Sequencing queue.
    pub t_segq: Tcpqehead,
    /// Tcp timers.
    pub t_timer: [Timeout; TCPT_NTIMERS],
    /// State of this connection (`TCPS_*`).
    pub t_state: Cell<i32>,
    /// log(2) of rexmt exp. backoff.
    pub t_rxtshift: Cell<i16>,
    /// Current retransmit value.
    pub t_rxtcur: Cell<i32>,
    /// Consecutive dup acks recd.
    pub t_dupacks: Cell<i16>,
    /// Maximum segment size.
    pub t_maxseg: Cell<u16>,
    /// Forcing out a byte.
    pub t_force: Cell<bool>,
    /// `TF_*`.
    pub t_flags: Cell<u32>,
    /// Skeletal packet for transmit.
    pub t_template: Cell<Option<&'static Mbuf>>,
    /// Back pointer to internet pcb.
    pub t_inpcb: &'static Inpcb,

    // The following fields are used as in the protocol specification. See RFC793, Dec.
    // 1981, page 21.

    // send sequence variables
    /// Send unacknowledged.
    pub snd_una: Cell<TcpSeq>,
    /// Send next.
    pub snd_nxt: Cell<TcpSeq>,
    /// Send urgent pointer.
    pub snd_up: Cell<TcpSeq>,
    /// Window update seg seq number.
    pub snd_wl1: Cell<TcpSeq>,
    /// Window update seg ack number.
    pub snd_wl2: Cell<TcpSeq>,
    /// Initial send sequence number.
    pub iss: Cell<TcpSeq>,
    /// Send window.
    pub snd_wnd: Cell<u64>,
    /// Enable SACK for this connection.
    pub sack_enable: Cell<i32>,
    /// Number of holes seen by sender.
    pub snd_numholes: Cell<i32>,
    /// Linked list of holes (sorted).
    pub snd_holes: Cell<Option<&'static Sackhole>>,
    /// For use in fast recovery.
    pub snd_last: Cell<TcpSeq>,
    // receive sequence variables
    /// Receive window.
    pub rcv_wnd: Cell<u64>,
    /// Receive next.
    pub rcv_nxt: Cell<TcpSeq>,
    /// Receive urgent pointer.
    pub rcv_up: Cell<TcpSeq>,
    /// Initial receive sequence number.
    pub irs: Cell<TcpSeq>,
    /// Last seq number(+1) sack'd by rcv'r.
    pub rcv_lastsack: Cell<TcpSeq>,
    /// # distinct sack blks present.
    pub rcv_numsacks: Cell<i32>,
    /// Seq nos. of sack blocks.
    pub sackblks: [Cell<Sackblk>; MAX_SACK_BLKS],

    // Additional variables for this implementation.

    // receive variables
    /// Advertised window.
    pub rcv_adv: Cell<TcpSeq>,
    // retransmit variables
    /// Highest sequence number sent; used to recognize retransmits.
    pub snd_max: Cell<TcpSeq>,
    // congestion control (for slow start, source quench, retransmit after loss)
    /// Congestion-controlled window.
    pub snd_cwnd: Cell<u64>,
    /// snd_cwnd size threshold for for slow start exponential to linear switch.
    pub snd_ssthresh: Cell<u64>,

    // auto-sizing variables
    /// Recv buffer autoscaling time stamp.
    pub rfbuf_ts: Cell<u64>,
    /// Recv buffer autoscaling byte count.
    pub rfbuf_cnt: Cell<u32>,

    /// Mss plus options.
    pub t_maxopd: Cell<u16>,
    /// Peer's maximum segment size.
    pub t_peermss: Cell<u16>,

    // transmit timing stuff. See below for scale of srtt and rttvar. "Variance" is actually
    // smoothed difference.
    /// Time last segment received.
    pub t_rcvtime: Cell<u64>,
    /// Time last ack received.
    pub t_rcvacktime: Cell<u64>,
    /// Time last segment sent.
    pub t_sndtime: Cell<u64>,
    /// Time last ack sent.
    pub t_sndacktime: Cell<u64>,
    /// Time we started measuring rtt.
    pub t_rtttime: Cell<u64>,
    /// Sequence number being timed.
    pub t_rtseq: Cell<TcpSeq>,
    /// Smoothed round-trip time.
    pub t_srtt: Cell<i32>,
    /// Variance in round-trip time.
    pub t_rttvar: Cell<i32>,
    /// Minimum rtt allowed.
    pub t_rttmin: Cell<u32>,
    /// Largest window peer has offered.
    pub max_sndwnd: Cell<u64>,

    // out-of-band data
    /// Have some (`TCPOOB_*`).
    pub t_oobflags: Cell<u8>,
    /// Input character.
    pub t_iobc: Cell<u8>,
    /// Possible error not yet reported.
    pub t_softerror: Cell<Option<Errno>>,

    // RFC 1323 variables
    /// Window scaling for send window.
    pub snd_scale: Cell<u8>,
    /// Window scaling for recv window.
    pub rcv_scale: Cell<u8>,
    /// Pending window scaling.
    pub request_r_scale: Cell<u8>,
    /// `requested_s_scale`.
    pub requested_s_scale: Cell<u8>,
    /// Timestamp echo data.
    pub ts_recent: Cell<u32>,
    /// Modulation on timestamp.
    pub ts_modulate: Cell<u32>,
    /// When last updated.
    pub ts_recent_age: Cell<u64>,
    /// `last_ack_sent`.
    pub last_ack_sent: Cell<TcpSeq>,

    /// Pointer for syn cache entries: list of entries by this tcb.
    pub t_sc: ListHead<ScTpq>,

    // Path-MTU Discovery Information
    /// MSS acked, lower bound for MTU.
    pub t_pmtud_mss_acked: Cell<u32>,
    /// MTU used, upper bound for MTU.
    pub t_pmtud_mtu_sent: Cell<u32>,
    /// TCP SEQ from ICMP payload.
    pub t_pmtud_th_seq: Cell<TcpSeq>,
    /// Advertised Next-Hop MTU from ICMP (network order, as `icmp_nextmtu`).
    pub t_pmtud_nextmtu: Cell<u32>,
    /// IP length from ICMP payload (network order).
    pub t_pmtud_ip_len: Cell<u16>,
    /// IP header length from ICMP payload.
    pub t_pmtud_ip_hl: Cell<u16>,

    /// `pf`: the protocol family (`PF_INET`; 0 until attached).
    pub pf: Cell<i32>,

    // maintain a few stats per connection:
    /// Out-of-order packets received.
    pub t_rcvoopack: Cell<u32>,
    /// Retransmit packets sent.
    pub t_sndrexmitpack: Cell<u32>,
    /// Zero-window updates sent.
    pub t_sndzerowin: Cell<u32>,
}

impl Tcpcb {
    /// A zeroed control block of `inp`, as `pool_get(PR_ZERO)` and the `t_inpcb` assignment
    /// of `tcp_newtcpcb` leave it.
    pub const fn new(inp: &'static Inpcb) -> Self {
        Self {
            t_segq: TailqHead::new(),
            t_timer: [const { Timeout::zeroed() }; TCPT_NTIMERS],
            t_state: Cell::new(0),
            t_rxtshift: Cell::new(0),
            t_rxtcur: Cell::new(0),
            t_dupacks: Cell::new(0),
            t_maxseg: Cell::new(0),
            t_force: Cell::new(false),
            t_flags: Cell::new(0),
            t_template: Cell::new(None),
            t_inpcb: inp,
            snd_una: Cell::new(0),
            snd_nxt: Cell::new(0),
            snd_up: Cell::new(0),
            snd_wl1: Cell::new(0),
            snd_wl2: Cell::new(0),
            iss: Cell::new(0),
            snd_wnd: Cell::new(0),
            sack_enable: Cell::new(0),
            snd_numholes: Cell::new(0),
            snd_holes: Cell::new(None),
            snd_last: Cell::new(0),
            rcv_wnd: Cell::new(0),
            rcv_nxt: Cell::new(0),
            rcv_up: Cell::new(0),
            irs: Cell::new(0),
            rcv_lastsack: Cell::new(0),
            rcv_numsacks: Cell::new(0),
            sackblks: [const { Cell::new(Sackblk { start: 0, end: 0 }) }; MAX_SACK_BLKS],
            rcv_adv: Cell::new(0),
            snd_max: Cell::new(0),
            snd_cwnd: Cell::new(0),
            snd_ssthresh: Cell::new(0),
            rfbuf_ts: Cell::new(0),
            rfbuf_cnt: Cell::new(0),
            t_maxopd: Cell::new(0),
            t_peermss: Cell::new(0),
            t_rcvtime: Cell::new(0),
            t_rcvacktime: Cell::new(0),
            t_sndtime: Cell::new(0),
            t_sndacktime: Cell::new(0),
            t_rtttime: Cell::new(0),
            t_rtseq: Cell::new(0),
            t_srtt: Cell::new(0),
            t_rttvar: Cell::new(0),
            t_rttmin: Cell::new(0),
            max_sndwnd: Cell::new(0),
            t_oobflags: Cell::new(0),
            t_iobc: Cell::new(0),
            t_softerror: Cell::new(None),
            snd_scale: Cell::new(0),
            rcv_scale: Cell::new(0),
            request_r_scale: Cell::new(0),
            requested_s_scale: Cell::new(0),
            ts_recent: Cell::new(0),
            ts_modulate: Cell::new(0),
            ts_recent_age: Cell::new(0),
            last_ack_sent: Cell::new(0),
            t_sc: ListHead::new(),
            t_pmtud_mss_acked: Cell::new(0),
            t_pmtud_mtu_sent: Cell::new(0),
            t_pmtud_th_seq: Cell::new(0),
            t_pmtud_nextmtu: Cell::new(0),
            t_pmtud_ip_len: Cell::new(0),
            t_pmtud_ip_hl: Cell::new(0),
            pf: Cell::new(0),
            t_rcvoopack: Cell::new(0),
            t_sndrexmitpack: Cell::new(0),
            t_sndzerowin: Cell::new(0),
        }
    }

    /// `ISSET(tp->t_flags, bits)`: any of `bits`.
    pub fn has_flags(&self, bits: u32) -> bool {
        self.t_flags.get() & bits != 0
    }

    /// `tp->t_flags |= bits`.
    pub fn set_flags(&self, bits: u32) {
        self.t_flags.set(self.t_flags.get() | bits);
    }

    /// `tp->t_flags &= ~bits`.
    pub fn clear_flags(&self, bits: u32) {
        self.t_flags.set(self.t_flags.get() & !bits);
    }

    /// `tp->t_inpcb->inp_socket`.
    pub fn socket(&self) -> &'static Socket {
        self.t_inpcb.socket()
    }
}

/// `struct tcp_opt_info`: handy way of passing around TCP option info.
#[derive(Clone, Copy, Debug, Default)]
pub struct TcpOptInfo {
    /// `ts_present`.
    pub ts_present: bool,
    /// `ts_val`.
    pub ts_val: u32,
    /// `ts_ecr`.
    pub ts_ecr: u32,
    /// `maxseg`.
    pub maxseg: u16,
}

/// `union syn_cache_sa`: a `struct sockaddr`, `sockaddr_in` or `sockaddr_in6` (see the
/// deviations).
pub type SynCacheSa = SockaddrUnion;

/// `struct syn_cache`: an embryonic connection, a `syn_cache_pool` item.
pub struct SynCache {
    /// \[S\] Link on bucket list.
    pub sc_bucketq: TailqEntry<SynCache>,
    /// Ref count list and timer.
    pub sc_refcnt: Refcnt,
    /// Rexmt timer.
    pub sc_timer: Timeout,
    /// \[s\] Cached route.
    pub sc_route: Route,
    /// \[I\] Advertised window.
    pub sc_win: Cell<i64>,
    /// \[S\] Our bucket index.
    pub sc_buckethead: Cell<*const SynCacheHead>,
    /// \[S\] Our syn cache set.
    pub sc_set: Cell<Option<&'static SynCacheSet>>,
    /// \[s\] Timestamp from SYN.
    pub sc_timestamp: Cell<u64>,
    /// \[S\] `sc_hash`.
    pub sc_hash: Cell<u32>,
    /// \[I\] Our timestamp modulator.
    pub sc_modulate: Cell<u32>,
    /// \[I\] `sc_src`.
    pub sc_src: Cell<SynCacheSa>,
    /// \[I\] `sc_dst`.
    pub sc_dst: Cell<SynCacheSa>,
    /// \[I\] `sc_irs`.
    pub sc_irs: Cell<TcpSeq>,
    /// \[I\] `sc_iss`.
    pub sc_iss: Cell<TcpSeq>,
    /// \[I\] `sc_rtableid`.
    pub sc_rtableid: Cell<u32>,
    /// \[S\] Current rxt timeout.
    pub sc_rxtcur: Cell<u32>,
    /// \[S\] Total time spend on queues.
    pub sc_rxttot: Cell<u32>,
    /// \[S\] For computing backoff.
    pub sc_rxtshift: Cell<u32>,
    /// \[S\] Flags accessed with mutex (`SCF_UNREACH`).
    pub sc_dynflags: Cell<u32>,
    /// \[I\] Set during initialization (`SCF_TIMESTAMP`, ...).
    pub sc_fixflags: Cell<u16>,
    /// \[s\] IP options.
    pub sc_ipopts: Cell<Option<&'static Mbuf>>,
    /// \[I\] `sc_peermaxseg`.
    pub sc_peermaxseg: Cell<u16>,
    /// \[I\] `sc_ourmaxseg`.
    pub sc_ourmaxseg: Cell<u16>,
    /// \[I\] `sc_request_r_scale:4`.
    pub sc_request_r_scale: Cell<u8>,
    /// \[I\] `sc_requested_s_scale:4`.
    pub sc_requested_s_scale: Cell<u8>,
    /// \[S\] Inpcb for listening socket.
    pub sc_inplisten: Cell<Option<&'static Inpcb>>,
    /// \[S\] List of entries by same tp.
    pub sc_tpq: ListEntry<SynCache>,
}

impl SynCache {
    /// A zeroed entry, as `pool_get(PR_ZERO)` hands it out.
    pub const fn new() -> Self {
        Self {
            sc_bucketq: TailqEntry::new(),
            sc_refcnt: Refcnt::new(),
            sc_timer: Timeout::zeroed(),
            sc_route: Route::new(),
            sc_win: Cell::new(0),
            sc_buckethead: Cell::new(ptr::null()),
            sc_set: Cell::new(None),
            sc_timestamp: Cell::new(0),
            sc_hash: Cell::new(0),
            sc_modulate: Cell::new(0),
            sc_src: Cell::new(SockaddrUnion::new()),
            sc_dst: Cell::new(SockaddrUnion::new()),
            sc_irs: Cell::new(0),
            sc_iss: Cell::new(0),
            sc_rtableid: Cell::new(0),
            sc_rxtcur: Cell::new(0),
            sc_rxttot: Cell::new(0),
            sc_rxtshift: Cell::new(0),
            sc_dynflags: Cell::new(0),
            sc_fixflags: Cell::new(0),
            sc_ipopts: Cell::new(None),
            sc_peermaxseg: Cell::new(0),
            sc_ourmaxseg: Cell::new(0),
            sc_request_r_scale: Cell::new(0),
            sc_requested_s_scale: Cell::new(0),
            sc_inplisten: Cell::new(None),
            sc_tpq: ListEntry::new(),
        }
    }
}

impl Default for SynCache {
    fn default() -> Self {
        Self::new()
    }
}

/// `struct syn_cache_head`: one bucket of a set.
pub struct SynCacheHead {
    /// \[S\] Bucket entries.
    pub sch_bucket: TailqHead<ScBucketq>,
    /// \[S\] # entries in bucket.
    pub sch_length: Cell<u16>,
}

impl SynCacheHead {
    /// An empty bucket.
    pub const fn new() -> Self {
        Self {
            sch_bucket: TailqHead::new(),
            sch_length: Cell::new(0),
        }
    }
}

impl Default for SynCacheHead {
    fn default() -> Self {
        Self::new()
    }
}

/// `struct syn_cache_set`: a hash table of embryonic connections; there are two, the
/// active one and the one being drained (`tcp_syn_cache[2]`).
pub struct SynCacheSet {
    /// \[S\] The bucket array (`malloc`ed, `scs_size` entries).
    pub scs_buckethead: Cell<&'static [SynCacheHead]>,
    /// \[S\] Uses left before the set is reseeded.
    pub scs_use: Cell<i64>,
    /// \[S\] Current size of hash table.
    pub scs_size: Cell<i32>,
    /// \[S\] `scs_count`.
    pub scs_count: Cell<i32>,
    /// \[S\] Hash seed.
    pub scs_random: Cell<[u32; 5]>,
}

impl SynCacheSet {
    /// An empty set (the static initialiser).
    pub const fn new() -> Self {
        Self {
            scs_buckethead: Cell::new(&[]),
            scs_use: Cell::new(0),
            scs_size: Cell::new(0),
            scs_count: Cell::new(0),
            scs_random: Cell::new([0; 5]),
        }
    }
}

impl Default for SynCacheSet {
    fn default() -> Self {
        Self::new()
    }
}

// SAFETY: every member is read and written only with `syn_cache_mtx` held (\[S\]), which
// serialises all access to the two static sets.
unsafe impl Sync for SynCacheSet {}

/// `struct tcpstat`: TCP statistics, as `sysctl(2)` returns them. Many of these should be
/// kept per connection, but that's inconvenient at the moment.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tcpstat {
    /// `tcps_connattempt`: Connections initiated.
    pub tcps_connattempt: u32,
    /// `tcps_accepts`: Connections accepted.
    pub tcps_accepts: u32,
    /// `tcps_connects`: Connections established.
    pub tcps_connects: u32,
    /// `tcps_drops`: Connections dropped.
    pub tcps_drops: u32,
    /// `tcps_conndrops`: Embryonic connections dropped.
    pub tcps_conndrops: u32,
    /// `tcps_closed`: Conn. closed (includes drops).
    pub tcps_closed: u32,
    /// `tcps_segstimed`: Segs where we tried to get rtt.
    pub tcps_segstimed: u32,
    /// `tcps_rttupdated`: Times we succeeded.
    pub tcps_rttupdated: u32,
    /// `tcps_delack`: Delayed acks sent.
    pub tcps_delack: u32,
    /// `tcps_timeoutdrop`: Conn. dropped in rxmt timeout.
    pub tcps_timeoutdrop: u32,
    /// `tcps_rexmttimeo`: Retransmit timeouts.
    pub tcps_rexmttimeo: u32,
    /// `tcps_persisttimeo`: Persist timeouts.
    pub tcps_persisttimeo: u32,
    /// `tcps_persistdrop`: Connections dropped in persist.
    pub tcps_persistdrop: u32,
    /// `tcps_keeptimeo`: Keepalive timeouts.
    pub tcps_keeptimeo: u32,
    /// `tcps_keepprobe`: Keepalive probes sent.
    pub tcps_keepprobe: u32,
    /// `tcps_keepdrops`: Connections dropped in keepalive.
    pub tcps_keepdrops: u32,
    /// `tcps_sndtotal`: Total packets sent.
    pub tcps_sndtotal: u32,
    /// `tcps_sndpack`: Data packets sent.
    pub tcps_sndpack: u32,
    /// `tcps_sndbyte`: Data bytes sent.
    pub tcps_sndbyte: u64,
    /// `tcps_sndrexmitpack`: Data packets retransmitted.
    pub tcps_sndrexmitpack: u32,
    /// `tcps_sndrexmitbyte`: Data bytes retransmitted.
    pub tcps_sndrexmitbyte: u64,
    /// `tcps_sndrexmitfast`: Fast retransmits.
    pub tcps_sndrexmitfast: u64,
    /// `tcps_sndacks`: Ack-only packets sent.
    pub tcps_sndacks: u32,
    /// `tcps_sndprobe`: Window probes sent.
    pub tcps_sndprobe: u32,
    /// `tcps_sndurg`: Packets sent with URG only.
    pub tcps_sndurg: u32,
    /// `tcps_sndwinup`: Window update-only packets sent.
    pub tcps_sndwinup: u32,
    /// `tcps_sndctrl`: Control (SYN|FIN|RST) packets sent.
    pub tcps_sndctrl: u32,
    /// `tcps_rcvtotal`: Total packets received.
    pub tcps_rcvtotal: u32,
    /// `tcps_rcvpack`: Packets received in sequence.
    pub tcps_rcvpack: u32,
    /// `tcps_rcvbyte`: Bytes received in sequence.
    pub tcps_rcvbyte: u64,
    /// `tcps_rcvbadsum`: Packets received with ccksum errs.
    pub tcps_rcvbadsum: u32,
    /// `tcps_rcvbadoff`: Packets received with bad offset.
    pub tcps_rcvbadoff: u32,
    /// `tcps_rcvmemdrop`: Packets dropped for lack of memory.
    pub tcps_rcvmemdrop: u32,
    /// `tcps_rcvnosec`: Packets dropped for lack of ipsec.
    pub tcps_rcvnosec: u32,
    /// `tcps_rcvshort`: Packets received too short.
    pub tcps_rcvshort: u32,
    /// `tcps_rcvduppack`: Duplicate-only packets received.
    pub tcps_rcvduppack: u32,
    /// `tcps_rcvdupbyte`: Duplicate-only bytes received.
    pub tcps_rcvdupbyte: u64,
    /// `tcps_rcvpartduppack`: Packets with some duplicate data.
    pub tcps_rcvpartduppack: u32,
    /// `tcps_rcvpartdupbyte`: Dup. bytes in part-dup. packets.
    pub tcps_rcvpartdupbyte: u64,
    /// `tcps_rcvoopack`: Out-of-order packets received.
    pub tcps_rcvoopack: u32,
    /// `tcps_rcvoobyte`: Out-of-order bytes received.
    pub tcps_rcvoobyte: u64,
    /// `tcps_rcvpackafterwin`: Packets with data after window.
    pub tcps_rcvpackafterwin: u32,
    /// `tcps_rcvbyteafterwin`: Bytes rcvd after window.
    pub tcps_rcvbyteafterwin: u64,
    /// `tcps_rcvafterclose`: Packets rcvd after "close".
    pub tcps_rcvafterclose: u32,
    /// `tcps_rcvwinprobe`: Rcvd window probe packets.
    pub tcps_rcvwinprobe: u32,
    /// `tcps_rcvdupack`: Rcvd duplicate acks.
    pub tcps_rcvdupack: u32,
    /// `tcps_rcvacktoomuch`: Rcvd acks for unsent data.
    pub tcps_rcvacktoomuch: u32,
    /// `tcps_rcvacktooold`: Rcvd acks for old data.
    pub tcps_rcvacktooold: u32,
    /// `tcps_rcvackpack`: Rcvd ack packets.
    pub tcps_rcvackpack: u32,
    /// `tcps_rcvackbyte`: Bytes acked by rcvd acks.
    pub tcps_rcvackbyte: u64,
    /// `tcps_rcvwinupd`: Rcvd window update packets.
    pub tcps_rcvwinupd: u32,
    /// `tcps_pawsdrop`: Segments dropped due to PAWS.
    pub tcps_pawsdrop: u32,
    /// `tcps_predack`: Times hdr predict ok for acks.
    pub tcps_predack: u32,
    /// `tcps_preddat`: Times hdr predict ok for data pkts.
    pub tcps_preddat: u32,
    /// `tcps_pcbhashmiss`: Input packets missing pcb hash.
    pub tcps_pcbhashmiss: u32,
    /// `tcps_noport`: No socket on port.
    pub tcps_noport: u32,
    /// `tcps_closing`: Inpcb exists, socket is closing.
    pub tcps_closing: u32,
    /// `tcps_badsyn`: SYN packet with src==dst rcv'ed.
    pub tcps_badsyn: u32,
    /// `tcps_dropsyn`: SYN packet dropped.
    pub tcps_dropsyn: u32,
    /// `tcps_rcvbadsig`: Rcvd bad/missing TCP signatures.
    pub tcps_rcvbadsig: u32,
    /// `tcps_rcvgoodsig`: Rcvd good TCP signatures.
    pub tcps_rcvgoodsig: u64,
    /// `tcps_inswcsum`: Input software-checksummed pkts.
    pub tcps_inswcsum: u32,
    /// `tcps_outswcsum`: Output software-checksummed pkts.
    pub tcps_outswcsum: u32,
    // ECN stats
    /// `tcps_ecn_accepts`: Ecn connections accepted.
    pub tcps_ecn_accepts: u32,
    /// `tcps_ecn_rcvece`: # of rcvd ece.
    pub tcps_ecn_rcvece: u32,
    /// `tcps_ecn_rcvcwr`: # of rcvd cwr.
    pub tcps_ecn_rcvcwr: u32,
    /// `tcps_ecn_rcvce`: # of rcvd ce in ip header.
    pub tcps_ecn_rcvce: u32,
    /// `tcps_ecn_sndect`: # of cwr sent.
    pub tcps_ecn_sndect: u32,
    /// `tcps_ecn_sndece`: # of ece sent.
    pub tcps_ecn_sndece: u32,
    /// `tcps_ecn_sndcwr`: # of cwr sent.
    pub tcps_ecn_sndcwr: u32,
    /// `tcps_cwr_ecn`: # of cwnd reduced by ecn.
    pub tcps_cwr_ecn: u32,
    /// `tcps_cwr_frecovery`: # of cwnd reduced by fastrecovery.
    pub tcps_cwr_frecovery: u32,
    /// `tcps_cwr_timeout`: # of cwnd reduced by timeout.
    pub tcps_cwr_timeout: u32,
    // These statistics deal with the SYN cache.
    /// `tcps_sc_added`: # of entries added.
    pub tcps_sc_added: u64,
    /// `tcps_sc_completed`: # of connections completed.
    pub tcps_sc_completed: u64,
    /// `tcps_sc_timed_out`: # of entries timed out.
    pub tcps_sc_timed_out: u64,
    /// `tcps_sc_overflowed`: # dropped due to overflow.
    pub tcps_sc_overflowed: u64,
    /// `tcps_sc_reset`: # dropped due to RST.
    pub tcps_sc_reset: u64,
    /// `tcps_sc_unreach`: # dropped due to ICMP unreach.
    pub tcps_sc_unreach: u64,
    /// `tcps_sc_bucketoverflow`: # dropped due to bucket overflow.
    pub tcps_sc_bucketoverflow: u64,
    /// `tcps_sc_aborted`: # of entries aborted (no mem).
    pub tcps_sc_aborted: u64,
    /// `tcps_sc_dupesyn`: # of duplicate SYNs received.
    pub tcps_sc_dupesyn: u64,
    /// `tcps_sc_dropped`: # of SYNs dropped (no route/mem).
    pub tcps_sc_dropped: u64,
    /// `tcps_sc_collisions`: # of hash collisions.
    pub tcps_sc_collisions: u64,
    /// `tcps_sc_retransmitted`: # of retransmissions.
    pub tcps_sc_retransmitted: u64,
    /// `tcps_sc_seedrandom`: # of syn cache seeds with random.
    pub tcps_sc_seedrandom: u64,
    /// `tcps_sc_hash_size`: Hash buckets in current syn cache.
    pub tcps_sc_hash_size: u64,
    /// `tcps_sc_entry_count`: # of entries in current syn cache.
    pub tcps_sc_entry_count: u64,
    /// `tcps_sc_entry_limit`: Limit of syn cache entries.
    pub tcps_sc_entry_limit: u64,
    /// `tcps_sc_bucket_maxlen`: Maximum # of entries in any bucket.
    pub tcps_sc_bucket_maxlen: u64,
    /// `tcps_sc_bucket_limit`: Limit of syn cache bucket list.
    pub tcps_sc_bucket_limit: u64,
    /// `tcps_sc_uses_left`: Use counter of current syn cache.
    pub tcps_sc_uses_left: i64,
    /// `tcps_conndrained`: # of connections drained.
    pub tcps_conndrained: u64,
    /// `tcps_sack_recovery_episode`: SACK recovery episodes.
    pub tcps_sack_recovery_episode: u64,
    /// `tcps_sack_rexmits`: SACK rexmit segments.
    pub tcps_sack_rexmits: u64,
    /// `tcps_sack_rexmit_bytes`: SACK rexmit bytes.
    pub tcps_sack_rexmit_bytes: u64,
    /// `tcps_sack_rcv_opts`: SACK options received.
    pub tcps_sack_rcv_opts: u64,
    /// `tcps_sack_snd_opts`: SACK options sent.
    pub tcps_sack_snd_opts: u64,
    /// `tcps_sack_drop_opts`: SACK options dropped.
    pub tcps_sack_drop_opts: u64,
    /// `tcps_outswtso`: Output tso chopped in software.
    pub tcps_outswtso: u32,
    /// `tcps_outhwtso`: Output tso processed by hardware.
    pub tcps_outhwtso: u32,
    /// `tcps_outpkttso`: Packets generated by tso.
    pub tcps_outpkttso: u32,
    /// `tcps_outbadtso`: Output tso failed, packet dropped.
    pub tcps_outbadtso: u32,
    /// `tcps_inswlro`: Input lro on pseudo device.
    pub tcps_inswlro: u32,
    /// `tcps_inhwlro`: Input lro from hardware.
    pub tcps_inhwlro: u32,
    /// `tcps_inpktlro`: Packets coalesced by hardware lro.
    pub tcps_inpktlro: u32,
    /// `tcps_inbadlro`: Input bad lro packets.
    pub tcps_inbadlro: u32,
}

/// `struct tcp_ident_mapping`: the `net.inet.tcp.ident` and `drop` request.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct TcpIdentMapping {
    /// `faddr`.
    pub faddr: SockaddrStorage,
    /// `laddr`.
    pub laddr: SockaddrStorage,
    /// `euid`.
    pub euid: i32,
    /// `ruid`.
    pub ruid: i32,
    /// `rdomain`.
    pub rdomain: u32,
    /// The C compiler's trailing padding (the storage is 8-byte aligned).
    pub _pad: u32,
}

impl TcpIdentMapping {
    /// An all-zero mapping.
    pub const fn zeroed() -> Self {
        Self {
            faddr: SockaddrStorage::zeroed(),
            laddr: SockaddrStorage::zeroed(),
            euid: 0,
            ruid: 0,
            rdomain: 0,
            _pad: 0,
        }
    }
}

// SAFETY: `#[repr(C)]` integers and byte arrays with the C's trailing hole as a named member
// (pinned below), so the structure has no implicit padding and any bytes are a valid value.
unsafe impl AbiPod for TcpIdentMapping {}

/// `enum tcpstat_counters`: the index of each counter in `tcpcounters`, in the order of the
/// members of [`Tcpstat`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum TcpstatCounters {
    /// `tcps_connattempt`.
    TcpsConnattempt,
    /// `tcps_accepts`.
    TcpsAccepts,
    /// `tcps_connects`.
    TcpsConnects,
    /// `tcps_drops`.
    TcpsDrops,
    /// `tcps_conndrops`.
    TcpsConndrops,
    /// `tcps_closed`.
    TcpsClosed,
    /// `tcps_segstimed`.
    TcpsSegstimed,
    /// `tcps_rttupdated`.
    TcpsRttupdated,
    /// `tcps_delack`.
    TcpsDelack,
    /// `tcps_timeoutdrop`.
    TcpsTimeoutdrop,
    /// `tcps_rexmttimeo`.
    TcpsRexmttimeo,
    /// `tcps_persisttimeo`.
    TcpsPersisttimeo,
    /// `tcps_persistdrop`.
    TcpsPersistdrop,
    /// `tcps_keeptimeo`.
    TcpsKeeptimeo,
    /// `tcps_keepprobe`.
    TcpsKeepprobe,
    /// `tcps_keepdrops`.
    TcpsKeepdrops,
    /// `tcps_sndtotal`.
    TcpsSndtotal,
    /// `tcps_sndpack`.
    TcpsSndpack,
    /// `tcps_sndbyte`.
    TcpsSndbyte,
    /// `tcps_sndrexmitpack`.
    TcpsSndrexmitpack,
    /// `tcps_sndrexmitbyte`.
    TcpsSndrexmitbyte,
    /// `tcps_sndrexmitfast`.
    TcpsSndrexmitfast,
    /// `tcps_sndacks`.
    TcpsSndacks,
    /// `tcps_sndprobe`.
    TcpsSndprobe,
    /// `tcps_sndurg`.
    TcpsSndurg,
    /// `tcps_sndwinup`.
    TcpsSndwinup,
    /// `tcps_sndctrl`.
    TcpsSndctrl,
    /// `tcps_rcvtotal`.
    TcpsRcvtotal,
    /// `tcps_rcvpack`.
    TcpsRcvpack,
    /// `tcps_rcvbyte`.
    TcpsRcvbyte,
    /// `tcps_rcvbadsum`.
    TcpsRcvbadsum,
    /// `tcps_rcvbadoff`.
    TcpsRcvbadoff,
    /// `tcps_rcvmemdrop`.
    TcpsRcvmemdrop,
    /// `tcps_rcvnosec`.
    TcpsRcvnosec,
    /// `tcps_rcvshort`.
    TcpsRcvshort,
    /// `tcps_rcvduppack`.
    TcpsRcvduppack,
    /// `tcps_rcvdupbyte`.
    TcpsRcvdupbyte,
    /// `tcps_rcvpartduppack`.
    TcpsRcvpartduppack,
    /// `tcps_rcvpartdupbyte`.
    TcpsRcvpartdupbyte,
    /// `tcps_rcvoopack`.
    TcpsRcvoopack,
    /// `tcps_rcvoobyte`.
    TcpsRcvoobyte,
    /// `tcps_rcvpackafterwin`.
    TcpsRcvpackafterwin,
    /// `tcps_rcvbyteafterwin`.
    TcpsRcvbyteafterwin,
    /// `tcps_rcvafterclose`.
    TcpsRcvafterclose,
    /// `tcps_rcvwinprobe`.
    TcpsRcvwinprobe,
    /// `tcps_rcvdupack`.
    TcpsRcvdupack,
    /// `tcps_rcvacktoomuch`.
    TcpsRcvacktoomuch,
    /// `tcps_rcvacktooold`.
    TcpsRcvacktooold,
    /// `tcps_rcvackpack`.
    TcpsRcvackpack,
    /// `tcps_rcvackbyte`.
    TcpsRcvackbyte,
    /// `tcps_rcvwinupd`.
    TcpsRcvwinupd,
    /// `tcps_pawsdrop`.
    TcpsPawsdrop,
    /// `tcps_predack`.
    TcpsPredack,
    /// `tcps_preddat`.
    TcpsPreddat,
    /// `tcps_pcbhashmiss`.
    TcpsPcbhashmiss,
    /// `tcps_noport`.
    TcpsNoport,
    /// `tcps_closing`.
    TcpsClosing,
    /// `tcps_badsyn`.
    TcpsBadsyn,
    /// `tcps_dropsyn`.
    TcpsDropsyn,
    /// `tcps_rcvbadsig`.
    TcpsRcvbadsig,
    /// `tcps_rcvgoodsig`.
    TcpsRcvgoodsig,
    /// `tcps_inswcsum`.
    TcpsInswcsum,
    /// `tcps_outswcsum`.
    TcpsOutswcsum,
    /// `tcps_ecn_accepts`.
    TcpsEcnAccepts,
    /// `tcps_ecn_rcvece`.
    TcpsEcnRcvece,
    /// `tcps_ecn_rcvcwr`.
    TcpsEcnRcvcwr,
    /// `tcps_ecn_rcvce`.
    TcpsEcnRcvce,
    /// `tcps_ecn_sndect`.
    TcpsEcnSndect,
    /// `tcps_ecn_sndece`.
    TcpsEcnSndece,
    /// `tcps_ecn_sndcwr`.
    TcpsEcnSndcwr,
    /// `tcps_cwr_ecn`.
    TcpsCwrEcn,
    /// `tcps_cwr_frecovery`.
    TcpsCwrFrecovery,
    /// `tcps_cwr_timeout`.
    TcpsCwrTimeout,
    /// `tcps_sc_added`.
    TcpsScAdded,
    /// `tcps_sc_completed`.
    TcpsScCompleted,
    /// `tcps_sc_timed_out`.
    TcpsScTimedOut,
    /// `tcps_sc_overflowed`.
    TcpsScOverflowed,
    /// `tcps_sc_reset`.
    TcpsScReset,
    /// `tcps_sc_unreach`.
    TcpsScUnreach,
    /// `tcps_sc_bucketoverflow`.
    TcpsScBucketoverflow,
    /// `tcps_sc_aborted`.
    TcpsScAborted,
    /// `tcps_sc_dupesyn`.
    TcpsScDupesyn,
    /// `tcps_sc_dropped`.
    TcpsScDropped,
    /// `tcps_sc_collisions`.
    TcpsScCollisions,
    /// `tcps_sc_retransmitted`.
    TcpsScRetransmitted,
    /// `tcps_sc_seedrandom`.
    TcpsScSeedrandom,
    /// `tcps_sc_hash_size`.
    TcpsScHashSize,
    /// `tcps_sc_entry_count`.
    TcpsScEntryCount,
    /// `tcps_sc_entry_limit`.
    TcpsScEntryLimit,
    /// `tcps_sc_bucket_maxlen`.
    TcpsScBucketMaxlen,
    /// `tcps_sc_bucket_limit`.
    TcpsScBucketLimit,
    /// `tcps_sc_uses_left`.
    TcpsScUsesLeft,
    /// `tcps_conndrained`.
    TcpsConndrained,
    /// `tcps_sack_recovery_episode`.
    TcpsSackRecoveryEpisode,
    /// `tcps_sack_rexmits`.
    TcpsSackRexmits,
    /// `tcps_sack_rexmit_bytes`.
    TcpsSackRexmitBytes,
    /// `tcps_sack_rcv_opts`.
    TcpsSackRcvOpts,
    /// `tcps_sack_snd_opts`.
    TcpsSackSndOpts,
    /// `tcps_sack_drop_opts`.
    TcpsSackDropOpts,
    /// `tcps_outswtso`.
    TcpsOutswtso,
    /// `tcps_outhwtso`.
    TcpsOuthwtso,
    /// `tcps_outpkttso`.
    TcpsOutpkttso,
    /// `tcps_outbadtso`.
    TcpsOutbadtso,
    /// `tcps_inswlro`.
    TcpsInswlro,
    /// `tcps_inhwlro`.
    TcpsInhwlro,
    /// `tcps_inpktlro`.
    TcpsInpktlro,
    /// `tcps_inbadlro`.
    TcpsInbadlro,
    /// `tcps_ncounters`.
    TcpsNcounters,
}

/// `tcps_ncounters`.
pub const TCPS_NCOUNTERS: usize = TcpstatCounters::TcpsNcounters as usize;

/// `tcpstat_inc`.
pub fn tcpstat_inc(c: TcpstatCounters) {
    TCPCOUNTERS[c as usize].fetch_add(1, Ordering::Relaxed);
}

/// `tcpstat_add`.
pub fn tcpstat_add(c: TcpstatCounters, v: u64) {
    TCPCOUNTERS[c as usize].fetch_add(v, Ordering::Relaxed);
}

/// `tcpstat_pkt`: one more packet in `pcounter`, `v` more bytes in `bcounter`
/// (`counters_pkt`).
pub fn tcpstat_pkt(pcounter: TcpstatCounters, bcounter: TcpstatCounters, v: u64) {
    TCPCOUNTERS[pcounter as usize].fetch_add(1, Ordering::Relaxed);
    TCPCOUNTERS[bcounter as usize].fetch_add(v, Ordering::Relaxed);
}

/// `tcp_now`: TCP time ticks in 63 bit milliseconds with 63 bit random offset.
pub fn tcp_now() -> u64 {
    TCP_STARTTIME
        .load(Ordering::Relaxed)
        .wrapping_add(getnsecruntime() / 1_000_000)
}

/// `TCP_TIME(sec)`: `tcp_now()` is in milliseconds.
pub const fn tcp_time(sec: i32) -> i32 {
    sec * 1000
}

/// `TCP_REXMTVAL(tp)`: the initial retransmission should happen at rtt + 4 * rttvar.
/// Because of the way we do the smoothing, srtt and rttvar will each average +1/2 tick of
/// bias. When we compute the retransmit timer, we want 1/2 tick of rounding and 1 extra tick
/// because of +-1/2 tick uncertainty in the firing of the timer. The bias will give us
/// exactly the 1.5 tick we need. But, because the bias is statistical, we have to test that
/// we don't drop below the minimum feasible timer (which is 2 ticks). This assumes that the
/// value of `1 << TCP_RTTVAR_SHIFT` is the same as the multiplier for rttvar.
pub fn tcp_rexmtval(tp: &Tcpcb) -> i32 {
    ((tp.t_srtt.get() >> TCP_RTT_SHIFT) + tp.t_rttvar.get()) >> TCP_RTT_BASE_SHIFT
}

/// `intotcpcb(inp)`: the TCP control block of an internet control block, `None` before
/// `tcp_newtcpcb` and after `tcp_close`.
pub fn intotcpcb(inp: &Inpcb) -> Option<&'static Tcpcb> {
    // SAFETY: a `tcbtable` control block's `inp_ppcb` is NULL or the `tcpcb_pool` item
    // `tcp_newtcpcb` stored there; `tcp_close` clears it before the item goes back to the
    // pool.
    unsafe { inp.inp_ppcb.get().cast::<Tcpcb>().cast_const().as_ref() }
}

/// `sototcpcb(so)`.
pub fn sototcpcb(so: &Socket) -> Option<&'static Tcpcb> {
    intotcpcb(sotoinpcb(so)?)
}

const _: () = {
    assert!(core::mem::size_of::<TcpIdentMapping>() == 528);
    assert!(core::mem::offset_of!(TcpIdentMapping, rdomain) == 520);
    assert!(core::mem::offset_of!(Tcpstat, tcps_sndbyte) == 72);
    assert!(core::mem::offset_of!(Tcpstat, tcps_sc_added) == 352);
    assert!(core::mem::size_of::<Tcpstat>() == 592);
};

#[cfg(test)]
mod tests {
    use std::boxed::Box;

    use super::*;

    #[test]
    fn rexmtval_matches_the_macro() {
        // srtt 8 << 3 (one second scaled), rttvar 4: ((64 >> 3) + 4) >> 2 = 3.
        let inp: &'static Inpcb = Box::leak(Box::new(Inpcb::new(None, None)));
        let tp = Tcpcb::new(inp);
        tp.t_srtt.set(64);
        tp.t_rttvar.set(4);
        assert_eq!(tcp_rexmtval(&tp), 3);
        assert_eq!(tcp_time(30), 30_000);
    }

    #[test]
    fn counters_cover_the_structure() {
        // Every member of struct tcpstat has a counter; the last one is tcps_inbadlro.
        assert_eq!(TCPS_NCOUNTERS, 107);
        assert_eq!(TcpstatCounters::TcpsInbadlro as usize, 106);
    }
}
