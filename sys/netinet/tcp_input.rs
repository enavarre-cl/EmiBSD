/*	$OpenBSD: tcp_input.c,v 1.469 2026/09/18 22:14:46 bluhm Exp $	*/
/*	$NetBSD: tcp_input.c,v 1.23 1996/02/13 23:43:44 christos Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1982, 1986, 1988, 1990, 1993, 1994
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
 *	@(#)COPYRIGHT	1.1 (NRL) 17 January 1995
 *
 * NRL grants permission for redistribution and use in source and binary
 * forms, with or without modification, of the software and documentation
 * created at NRL provided that the following conditions are met:
 *
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. All advertising materials mentioning features or use of this software
 *    must display the following acknowledgements:
 *	This product includes software developed by the University of
 *	California, Berkeley and its contributors.
 *	This product includes software developed at the Information
 *	Technology Division, US Naval Research Laboratory.
 * 4. Neither the name of the NRL nor the names of its contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THE SOFTWARE PROVIDED BY NRL IS PROVIDED BY NRL AND CONTRIBUTORS ``AS
 * IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
 * TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A
 * PARTICULAR PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL NRL OR
 * CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL,
 * EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO,
 * PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR
 * PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF
 * LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING
 * NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
 * SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 *
 * The views and conclusions contained in the software and documentation
 * are those of the authors and should not be interpreted as representing
 * official policies, either expressed or implied, of the US Naval
 * Research Laboratory (NRL).
 */
/* </LICENSES> */

/* <CODE> */
//! TCP input: `netinet/tcp_input.c`.
//!
//! Upstream: sys/netinet/tcp_input.c @ 3ce1f3f79392
//!
//! `tcp_input` follows pages 65-76 of the protocol specification dated September, 1981 very
//! closely: it checks a segment's checksum and data offset, finds the connection's control
//! block (or the listening one), runs header prediction for the two common cases of a
//! uni-directional transfer, and otherwise trims the segment to the receive window, processes
//! RST, SYN and ACK, updates the send window, the urgent pointer and the data (in sequence or
//! through the reassembly queue, `tcp_reass`), and the FIN. Segments queued on a softnet
//! thread's `ns_tcp_ml` are processed by `tcp_input_mlist`, which keeps the socket locked
//! across consecutive segments of one connection.
//!
//! Beside the input path the file holds the option parser (`tcp_dooptions`, with the TCP MD5
//! signature check), the receiver's SACK report (`tcp_update_sack_list`) and the sender's
//! scoreboard of SACK holes (`tcp_sack_option`, `tcp_del_sackholes`), the round-trip timer
//! (`tcp_xmit_timer`), the maximum segment size (`tcp_mss`, `tcp_mss_update`, `tcp_mss_adv`),
//! the SYN cache (the compressed state of embryonic connections in `SYN_RECEIVED`: two hash
//! tables of `struct syn_cache`, the active one and the one being drained), and the software
//! large receive offload that glues TCP segments of one flow together (`tcp_softlro_glue`).
//!
//! Locks used to protect global data and struct members: \[a\] atomic operations, \[N\] net
//! lock, \[S\] `syn_cache_mtx`.
//!
//! ## Deviations
//! - The TCP header is copied out of the mbuf after `ip6_exthdr_get` (unaligned) and the
//!   C's in-place `NTOHL`/`NTOHS` of `th_seq`, `th_ack`, `th_win` and `th_urp` are done on
//!   that copy, which then plays the C's `struct tcphdr *th`. The mbuf keeps the header in
//!   network order: after the conversion nothing reads the TCP header from the mbuf again
//!   (`tcp_respond` and `tcp_signature` take the header as an argument, the data is trimmed
//!   past it). The options are copied out likewise (at most `MAX_TCPOPTLEN` bytes).
//! - The network header is copied once (`ip` or `ip6`, the C's two pointers, as `Option`s);
//!   `saveti` is the bytes of the C's union of `struct tcpiphdr` and `struct tcpipv6hdr`.
//!   The SYN cache reads and writes the `sin6` member of `union syn_cache_sa` with
//!   `sa_sin6`/`sa_from_sin6`. `tcp_respond`'s template is the first 40 bytes of the packet
//!   (an IPv6 header, or the IPv4 one and what follows it).
//! - `tcp_input_solocked`'s labels (`badsyn`, `dropafterack_ratelim`, `dropafterack`,
//!   `dropwithreset_ratelim`, `dropwithreset`, `drop` and the common `return` that hands the
//!   socket back) are the variants of `TcpInputExit`, returned by the body and run by
//!   `tcp_input_solocked` over the state the labels read (`TcpInputState`: the mbuf, control
//!   blocks, socket, header copy, flags and length). `findpcb` is a loop, `step6` and
//!   `dodata` labelled blocks, the ACK switch's fall-through from `SYN_RECEIVED` an `if`
//!   before the shared arm. `saveti` (`struct tcpiphdr`) is its 40 bytes, which `tcp_trace`
//!   takes as a slice.
//! - Return values: `tcp_reass` returns `Ok(TH_FIN or 0)` or `Err(ENOBUFS)` for the C's -1
//!   (the segment was dropped for lack of a queue entry); `tcp_dooptions` and
//!   `syn_cache_add` return `true` for the C's 0 and `false` for -1; `syn_cache_get` returns
//!   `SynCacheGet` for the C's NULL, `(struct socket *)-1` and the new socket;
//!   `syn_cache_respond` returns `Result<(), Errno>`. `tcp_dooptions` and `tcp_sack_option`
//!   take the options as a slice (the C's pointer and count; NULL is the empty slice).
//! - The SYN cache functions take the addresses as `SynCacheSa` (`union syn_cache_sa`, a
//!   byte image) rather than `struct sockaddr *`; `syn_cache_unreach`, called from
//!   `tcp_ctlinput` with raw pointers, copies them in. `syn_cache_lookup` does not return
//!   the bucket through `headp`: no caller reads it. The bucket arrays are `malloc`ed slices
//!   (`scs_buckethead`); `sc_buckethead` is a raw pointer into one, valid while the entry is
//!   linked. `scs_random` is filled with `arc4random_buf` through a byte array.
//! - `tcp_rst_ppslim_count`/`tcp_rst_ppslim_last` and `tcp_ackdrop_ppslim_count`/
//!   `tcp_ackdrop_ppslim_last` are pairs in `StaticCell`s that only `ppsratecheck` touches
//!   (the `icmperrppslim` idiom of `ip_icmp.rs`). `tcp_ackdrop_ppslim` is an `AtomicI32`.
//! - `syn_cache_add`'s `struct tcpcb tb` on the stack is a `Tcpcb::new(tp.t_inpcb)`: the
//!   control block needs a back pointer, which `tcp_dooptions` never follows.
//! - Where the C dereferences a pointer that cannot be NULL there, the port takes the
//!   harmless way out instead of a wild read: `syn_cache_get` treats a listening socket
//!   without a control block as a miss and sends a socket from `sonewconn` without one (or
//!   without a TCP control block) to `resetandabort`; `syn_cache_add` fails without a TCP
//!   control block; `tcp_pulloutofband` skips the `t_iobc` store without one;
//!   `tcp_softlro_compare` refuses a head without a TCP header; the listener's flags
//!   `syn_cache_get` copies are 0 without its control block.
//! - The functions `tcp_var.h` does not declare stay private: `tcp_input_solocked`,
//!   `tcp_flush_queue`, `tcp_sack_partialack`, `tcp_newreno_partialack`, `tcp_mss_adv`, the
//!   SYN cache internals and the soft LRO helpers.
//! - `syn_cache_respond` without a listening control block uses `ip6_defhlim`, what the
//!   C's `in6_selecthlim(NULL)` returns (`in6_selecthlim` takes a `&Inpcb` here).
//! - `INET6` is configured (feature `inet6`): `tcb6table`, `ns_tcp6_ml`,
//!   `in6_pcblookup*`, `in6_cksum`, `ip6_output`, `route6_mpath`, the IPv6 header checks and
//!   the IPv6 branches of `tcp_mss`, `tcp_hdrsz`, `tcp_mss_adv`, `tcp_dooptions` and the
//!   SYN cache. The soft LRO's IPv6 cases are not `#ifdef INET6` in the C and compile
//!   always. Configured (GENERIC): `TCP_ECN`, `TCP_SIGNATURE`,
//!   `IPSEC`, `NPF` (`pf_inp_lookup`, `pf_inp_link`, `pf_inp_unlink`, `pf_find_divert`).
//!   `SMALL_KERNEL` is not set, so the soft LRO is compiled; `tcp_softlro_glue` has no
//!   caller yet (its drivers, `ixl`, `ice`, `bnxt`, `cnmac`, are not ported). `DIAGNOSTIC` is
//!   feature `diagnostic` (the `TCPS_LISTEN` panic, the impossible SYN cache overflows, the
//!   `max_linkhdr` check of `syn_cache_respond`).

use core::cmp::{max, min};
use core::ffi::c_void;
use core::mem::{size_of, size_of_val};
use core::ptr::{self, NonNull};
use core::slice;
use core::sync::atomic::{AtomicI32, AtomicUsize, Ordering};

use libkern::{StaticCell, timingsafe_bcmp};

use crate::dev::rnd::{arc4random, arc4random_buf};
use crate::kassert;
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_malloc::{free, mallocarray};
use crate::kern::kern_synch::{refcnt_init_trace, refcnt_rele, refcnt_take};
use crate::kern::kern_time::ppsratecheck_shared;
use crate::kern::kern_timeout::{timeout_add_msec, timeout_del, timeout_set_flags};
use crate::kern::subr_pool::{pool_get, pool_init, pool_put};
use crate::kern::subr_prf::panic;
use crate::kern::uipc_mbuf::{
    MAX_LINKHDR, m_adj, m_copydata, m_free, m_freem, m_gethdr, ml_dequeue, ml_enqueue,
};
use crate::kern::uipc_mbuf2::m_tag_find;
use crate::kern::uipc_socket::{sohasoutofband, sorwakeup, sowwakeup};
use crate::kern::uipc_socket2::{
    SB_MAX_VAR, sbappendstream, sbdrop, sbreserve, soassertlocked, socantrcvmore, soisconnected,
    soisdisconnected, sonewconn,
};
use crate::machine::intr::IPL_SOFTNET;
use crate::net::if_::{IFF_LOOPBACK, IFXF_LRO, if_get, if_put, unhandled_af};
use crate::net::if_ethersubr::ether_extract_headers;
use crate::net::if_var::{Ifnet, Netstack};
use crate::net::pf::{pf_find_divert, pf_inp_link, pf_inp_lookup, pf_inp_unlink};
use crate::net::route::{Rtentry, route_mpath, rtfree};
use crate::net::rtable::rtable_l2;
use crate::netinet::if_ether::{
    EtherExtracted, EtherHeader, EtherVlanHeader, evl_prioftag, evl_vlanoftag,
};
use crate::netinet::in_::{INADDR_ANY, IPPROTO_DONE, IPPROTO_TCP, SockaddrIn};
use crate::netinet::in_pcb::{
    INP_IPV6, Inpcb, in_pcblookup, in_pcblookup_listen, in_pcbref, in_pcbrtentry, in_pcbset_addr,
    in_pcbsolock, in_pcbsounlock, in_pcbunref, sotoinpcb,
};
use crate::netinet::in4_cksum::in4_cksum;
use crate::netinet::ip::{IP_MAXPACKET, IPTOS_ECN_CE, IPTOS_ECN_MASK, Ip};
use crate::netinet::ip_input::{IP_DEFTTL, ip_mtudisc, ip_srcroute};
use crate::netinet::ip_ipsp::{
    IPSEC_IN_USE, IPSP_DIRECTION_IN, SockaddrUnion, Tdb, TdbIdent, gettdb, gettdbbysrcdst,
    tdb_unref,
};
use crate::netinet::ip_output::ip_output;
use crate::netinet::ip_spd::ipsp_spd_lookup;
use crate::netinet::ip_var::{IP_MTUDISC, mtod_ip, mtod_ip_store};
use crate::netinet::ip6::{IPV6_MAXPACKET, IPV6_MMTU, Ip6Frag, Ip6Hdr, ip6_exthdr_get};
use crate::netinet::tcp::{
    MAX_SACK_BLKS, MAX_TCPOPTLEN, TCP_MAX_WINSHIFT, TCP_MAXWIN, TCP_SACKHOLE_LIMIT, TCPOLEN_MAXSEG,
    TCPOLEN_SACK, TCPOLEN_SACK_PERMITTED, TCPOLEN_SIGLEN, TCPOLEN_SIGNATURE, TCPOLEN_TIMESTAMP,
    TCPOLEN_TSTAMP_APPA, TCPOLEN_WINDOW, TCPOPT_EOL, TCPOPT_MAXSEG, TCPOPT_NOP, TCPOPT_SACK,
    TCPOPT_SACK_PERMIT_HDR, TCPOPT_SACK_PERMITTED, TCPOPT_SIGNATURE, TCPOPT_TIMESTAMP,
    TCPOPT_TSTAMP_HDR, TCPOPT_WINDOW, TH_ACK, TH_CWR, TH_ECE, TH_FIN, TH_PUSH, TH_RST, TH_SYN,
    TH_URG, TcpSeq, Tcphdr,
};
use crate::netinet::tcp_debug::{TA_DROP, TA_INPUT, tcp_trace};
use crate::netinet::tcp_fsm::{
    TCPS_CLOSE_WAIT, TCPS_CLOSED, TCPS_CLOSING, TCPS_ESTABLISHED, TCPS_FIN_WAIT_1, TCPS_FIN_WAIT_2,
    TCPS_LAST_ACK, TCPS_LISTEN, TCPS_SYN_RECEIVED, TCPS_SYN_SENT, TCPS_TIME_WAIT,
    tcps_haveestablished, tcps_havercvdfin, tcps_havercvdsyn,
};
use crate::netinet::tcp_output::tcp_output;
use crate::netinet::tcp_seq::{seq_geq, seq_gt, seq_leq, seq_lt, tcp_rcvseqinit, tcp_sendseqinit};
use crate::netinet::tcp_subr::{
    SACKHL_POOL, TCP_ACK_ON_PUSH, TCP_DO_ECN, TCP_DO_RFC1323, TCP_DO_RFC3390, TCP_MSSDFLT,
    TCPQE_POOL, tcp_close, tcp_drop, tcp_freeq, tcp_respond, tcp_signature, tcp_template,
};
use crate::netinet::tcp_timer::{
    TCP_BACKOFF, TCP_KEEPIDLE, TCP_KEEPINIT, TCP_MAXRXTSHIFT, TCPT_2MSL, TCPT_DELACK, TCPT_KEEP,
    TCPT_PERSIST, TCPT_REXMT, TCPTV_KEEPCNT, TCPTV_MIN, TCPTV_MSL, TCPTV_REXMTMAX, TCPTV_SRTTDFLT,
    tcp_canceltimers, tcp_delack_msecs, tcp_timer_arm, tcp_timer_disarm, tcp_timer_isarmed,
    tcpt_rangeset,
};
use crate::netinet::tcp_usrreq::{TCBTABLE, tcp_update_rcvspace, tcp_update_sndspace};
use crate::netinet::tcp_var::{
    SCF_ECN_PERMIT, SCF_SACK_PERMIT, SCF_SIGNATURE, SCF_TIMESTAMP, SCF_UNREACH, Sackblk, Sackhole,
    ScTpq, SynCache, SynCacheHead, SynCacheSa, SynCacheSet, TCP_RTT_BASE_SHIFT, TCP_RTT_MAX,
    TCP_RTT_SHIFT, TCP_RTTVAR_SHIFT, TCP_SYN_BUCKET_SIZE, TCP_SYN_HASH_SIZE, TCPOOB_HADDATA,
    TCPOOB_HAVEDATA, TF_ACKNOW, TF_DISABLE_ECN, TF_ECN_PERMIT, TF_NEEDOUTPUT, TF_NODELAY, TF_NOOPT,
    TF_NOPUSH, TF_PMTUD_PEND, TF_RCVD_CE, TF_RCVD_SCALE, TF_RCVD_TSTMP, TF_REQ_SCALE, TF_REQ_TSTMP,
    TF_SACK_PERMIT, TF_SEND_CWR, TF_SIGNATURE, TcpOptInfo, Tcpcb, Tcpqehead, Tcpqent,
    TcpstatCounters, intotcpcb, sototcpcb, tcp_now, tcp_rexmtval, tcp_time, tcpstat_add,
    tcpstat_inc, tcpstat_pkt,
};
use crate::netinet6::in6::in6_are_addr_equal;
use crate::sys::endian::{htonl, htons, ntohl, ntohs};
use crate::sys::errno::Errno;
use crate::sys::kernel::HZ;
use crate::sys::malloc::{M_NOWAIT, M_SYNCACHE, M_WAITOK, M_ZERO};
use crate::sys::mbuf::{
    M_BCAST, M_DONTWAIT, M_EXT, M_FLOWID, M_IPV4_CSUM_IN_OK, M_IPV4_CSUM_OUT, M_MCAST, M_PKTHDR,
    M_TCP_CSUM_IN_BAD, M_TCP_CSUM_IN_OK, M_TCP_CSUM_OUT, M_TCP_TSO, M_VLANTAG, MHLEN, MT_DATA,
    Mbuf, MbufList, PACKET_TAG_IPSEC_IN_DONE, PF_TAG_DIVERTED, m_freemp, mclget, mtod,
};
use crate::sys::mutex::{Mutex, mutex_assert_locked};
use crate::sys::pool::{PR_NOWAIT, PR_ZERO, Pool};
use crate::sys::queue::ListHead;
use crate::sys::refcnt::DT_REFCNT_IDX_SYNCACHE;
use crate::sys::socket::{AF_INET, AF_INET6, SO_ACCEPTCONN, SO_DEBUG, SO_OOBINLINE, Sockaddr};
use crate::sys::socketvar::{
    SS_CANTRCVMORE, SS_ISCONNECTED, SS_NOFDREF, SS_RCVATMARK, Socket, sb_notify, sbspace,
};
use crate::sys::systm::{net_assert_locked, net_lock_shared, net_unlock_shared};
use crate::sys::time::Timeval;
use crate::sys::timeout::{KCLOCK_NONE, TIMEOUT_MPSAFE, TIMEOUT_PROC};
#[cfg(feature = "inet6")]
use crate::{
    net::route::route6_mpath,
    netinet::ip6::{IPV6_VERSION, IPV6_VERSION_MASK},
    netinet::tcp_usrreq::TCB6TABLE,
    netinet6::in6::{SockaddrIn6, in6_is_addr_multicast, in6_is_addr_unspecified},
    netinet6::in6_cksum::in6_cksum,
    netinet6::in6_pcb::{in6_pcblookup, in6_pcblookup_listen},
    netinet6::in6_proto::IP6_DEFHLIM,
    netinet6::in6_src::in6_selecthlim,
    netinet6::ip6_output::ip6_output,
    netinet6::ip6_var::{mtod_ip6, mtod_ip6_store},
};

/// `tcprexmtthresh`.
pub const TCPREXMTTHRESH: i32 = 3;

/// `TCP_PAWS_IDLE`.
const TCP_PAWS_IDLE: i32 = tcp_time(24 * 24 * 60 * 60);

/// `sizeof(struct ip)`.
const IP_HDR_LEN: usize = size_of::<Ip>();
/// `sizeof(struct tcphdr)`.
const TCP_HDR_LEN: usize = size_of::<Tcphdr>();
/// `sizeof(struct ip6_hdr)`.
const IP6_HDR_LEN: usize = size_of::<Ip6Hdr>();
/// `sizeof(saveti)`: the union of `struct tcpiphdr` and `struct tcpipv6hdr` that
/// `tcp_trace` reads.
const SAVETI_LEN: usize = IP6_HDR_LEN + TCP_HDR_LEN;
/// `AF_INET` as the `int` of `af` and `tp->pf`.
const AF_INET_I32: i32 = AF_INET as i32;
/// `AF_INET6` as the `int` of `af` and `tp->pf`.
const AF_INET6_I32: i32 = AF_INET6 as i32;

/// A rate limiter's state: `tcp_rst_ppslim_last` and `tcp_rst_ppslim_count`, or the
/// `tcp_ackdrop_ppslim` pair.
struct Ppslim {
    last: Timeval,
    count: i32,
}

/// Where `tcp_input_solocked` goes once the body has looked at the segment: the C's labels
/// and its common return.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TcpInputExit {
    /// `return IPPROTO_DONE` with nothing held (the header was too short).
    Return,
    /// The socket goes back to the caller (or is unlocked) and the control block reference is
    /// dropped: the end of every path that processed the segment.
    Done,
    /// `badsyn`.
    BadSyn,
    /// `dropafterack_ratelim`.
    DropAfterAckRatelim,
    /// `dropafterack`.
    DropAfterAck,
    /// `dropwithreset_ratelim`.
    DropWithResetRatelim,
    /// `dropwithreset`.
    DropWithReset,
    /// `drop`.
    Drop,
}

/// The locals of `tcp_input_solocked` that its labels read.
struct TcpInputState {
    /// `m`: the segment, `None` once consumed.
    m: Option<&'static Mbuf>,
    /// `inp`: referenced.
    inp: Option<&'static Inpcb>,
    /// `so`: locked.
    so: Option<&'static Socket>,
    /// `tp`.
    tp: Option<&'static Tcpcb>,
    /// `otp`: the control block traced under `SO_DEBUG` (its address only), or null.
    otp: *const Tcpcb,
    /// `ostate`.
    ostate: i32,
    /// `saveti`: the IP and TCP headers for `tcp_trace`.
    saveti: [u8; SAVETI_LEN],
    /// `*th`: the TCP header, host order once converted.
    th: Tcphdr,
    /// `tiflags`.
    tiflags: u8,
    /// `tlen`.
    tlen: i32,
    /// `now`.
    now: u64,
}

/// What `syn_cache_get` found.
enum SynCacheGet {
    /// `NULL`: the SYN was not in the cache; the caller sends an RST.
    NotFound,
    /// `(struct socket *)(-1)`: the connection could not be created and is aborted; the mbuf
    /// was consumed.
    Aborted,
    /// The new socket, locked, its control block referenced.
    Socket(&'static Socket),
}

/// \[a\] `tcp_rst_ppslim`: maximum outgoing RST packets per second.
pub static TCP_RST_PPSLIM: AtomicI32 = AtomicI32::new(100);
/// `tcp_rst_ppslim_count` and `tcp_rst_ppslim_last`: only `ppsratecheck` touches them,
/// under its mutex.
static TCP_RST_PPS: StaticCell<Ppslim> = StaticCell::new(Ppslim {
    last: Timeval {
        tv_sec: 0,
        tv_usec: 0,
    },
    count: 0,
});

/// `tcp_ackdrop_ppslim`: 100pps.
pub static TCP_ACKDROP_PPSLIM: AtomicI32 = AtomicI32::new(100);
/// `tcp_ackdrop_ppslim_count` and `tcp_ackdrop_ppslim_last`, as [`TCP_RST_PPS`].
static TCP_ACKDROP_PPS: StaticCell<Ppslim> = StaticCell::new(Ppslim {
    last: Timeval {
        tv_sec: 0,
        tv_usec: 0,
    },
    count: 0,
});

// TCP compressed state engine. Currently used to hold compressed state for SYN_RECEIVED.

/// \[S\] `tcp_syn_hash_size`: size of hash table.
#[allow(non_upper_case_globals)] // TCP_SYN_HASH_SIZE is a constant of <netinet/tcp_var.h>
pub static tcp_syn_hash_size: AtomicI32 = AtomicI32::new(TCP_SYN_HASH_SIZE);
/// \[a\] `tcp_syn_cache_limit`: global entry limit.
pub static TCP_SYN_CACHE_LIMIT: AtomicI32 = AtomicI32::new(TCP_SYN_HASH_SIZE * TCP_SYN_BUCKET_SIZE);
/// \[a\] `tcp_syn_bucket_limit`: per bucket limit.
pub static TCP_SYN_BUCKET_LIMIT: AtomicI32 = AtomicI32::new(3 * TCP_SYN_BUCKET_SIZE);
/// \[S\] `tcp_syn_use_limit`: reseed after uses.
pub static TCP_SYN_USE_LIMIT: AtomicI32 = AtomicI32::new(100000);

/// `syn_cache_pool`.
pub static SYN_CACHE_POOL: Pool = Pool::new();
/// \[S\] `tcp_syn_cache`: the active set and the passive one.
pub static TCP_SYN_CACHE: [SynCacheSet; 2] = [SynCacheSet::new(), SynCacheSet::new()];
/// \[S\] `tcp_syn_cache_active`: 0 or 1.
pub static TCP_SYN_CACHE_ACTIVE: AtomicUsize = AtomicUsize::new(0);
/// `syn_cache_mtx`: tcp syn cache global mutex.
pub static SYN_CACHE_MTX: Mutex = Mutex::new(IPL_SOFTNET);

/// `TSTMP_LT(a, b)`: modulo comparison of timestamps.
const fn tstmp_lt(a: u32, b: u32) -> bool {
    (a.wrapping_sub(b) as i32) < 0
}

/// `TSTMP_GEQ(a, b)`.
const fn tstmp_geq(a: u32, b: u32) -> bool {
    (a.wrapping_sub(b) as i32) >= 0
}

/// `SEQ_MIN(a, b)`: for TCP SACK comparisons.
const fn seq_min(a: TcpSeq, b: TcpSeq) -> TcpSeq {
    if seq_lt(a, b) { a } else { b }
}

/// `SEQ_MAX(a, b)`.
const fn seq_max(a: TcpSeq, b: TcpSeq) -> TcpSeq {
    if seq_gt(a, b) { a } else { b }
}

// ECN (Explicit Congestion Notification) support based on RFC3168 implementation note:
// snd_last is used to track a recovery phase. when cwnd is reduced, snd_last is set to
// snd_max. while snd_last > snd_una, the sender is in a recovery phase and its cwnd should
// not be reduced again. snd_last follows snd_una when not in a recovery phase.

/// `TCP_SETUP_ACK(tp, tiflags, m)`: compute ACK transmission behavior. Delay the ACK unless
/// we have already delayed an ACK (must send an ACK every two segments). We also ACK
/// immediately if we received a PUSH and the ACK-on-PUSH option is enabled or when the packet
/// is coming from a loopback interface.
fn tcp_setup_ack(tp: &Tcpcb, tiflags: u8, m: Option<&Mbuf>) {
    let mut ifp = None;
    if let Some(m) = m
        && m.m_flags().get() & M_PKTHDR != 0
    {
        ifp = if_get(m.m_pkthdr().ph_ifidx.get());
    }
    if tcp_timer_isarmed(tp, TCPT_DELACK)
        || (TCP_ACK_ON_PUSH.load(Ordering::Relaxed) != 0 && tiflags & TH_PUSH != 0)
        || ifp.is_some_and(|i| i.if_flags.get() & IFF_LOOPBACK != 0)
    {
        tp.set_flags(TF_ACKNOW);
    } else {
        tcp_timer_arm(tp, TCPT_DELACK, tcp_delack_msecs as u64);
    }
    if_put(ifp);
}

/// The bytes of `th` as they lie in memory (`struct tcphdr` has no padding).
fn th_bytes(th: &Tcphdr) -> [u8; TCP_HDR_LEN] {
    let mut b = [0u8; TCP_HDR_LEN];
    b[0..2].copy_from_slice(&th.th_sport.to_ne_bytes());
    b[2..4].copy_from_slice(&th.th_dport.to_ne_bytes());
    b[4..8].copy_from_slice(&th.th_seq.to_ne_bytes());
    b[8..12].copy_from_slice(&th.th_ack.to_ne_bytes());
    b[12] = th.th_x2_off;
    b[13] = th.th_flags;
    b[14..16].copy_from_slice(&th.th_win.to_ne_bytes());
    b[16..18].copy_from_slice(&th.th_sum.to_ne_bytes());
    b[18..20].copy_from_slice(&th.th_urp.to_ne_bytes());
    b
}

/// The IP or IPv6 header at the front of `m`, as the bytes `tcp_respond` takes for its
/// template (`mtod(m, caddr_t)`; it reads `sizeof(struct ip)` or `sizeof(struct ip6_hdr)`).
fn ip_template(m: &Mbuf) -> [u8; IP6_HDR_LEN] {
    let mut b = [0u8; IP6_HDR_LEN];
    let n = (m.m_pkthdr().len.get().max(0) as usize).min(IP6_HDR_LEN);
    m_copydata(m, 0, &mut b[..n]);
    b
}

/// A `union syn_cache_sa` holding `sin6`.
#[cfg(feature = "inet6")]
fn sa_from_sin6(sin6: &SockaddrIn6) -> SynCacheSa {
    let mut su = SynCacheSa::new();
    // SAFETY: `SockaddrIn6` is `#[repr(C)]` without padding, as large as the union.
    let b = unsafe {
        core::slice::from_raw_parts(ptr::from_ref(sin6).cast::<u8>(), size_of::<SockaddrIn6>())
    };
    su.as_bytes_mut().copy_from_slice(b);
    su
}

/// `satosin6_const(&sa)`: the union read as a `sockaddr_in6`.
#[cfg(feature = "inet6")]
fn sa_sin6(su: &SynCacheSa) -> SockaddrIn6 {
    // SAFETY: the union is `sizeof(struct sockaddr_in6)` initialised bytes; `SockaddrIn6` is
    // plain old data of that size, read unaligned.
    unsafe {
        su.as_bytes()
            .as_ptr()
            .cast::<SockaddrIn6>()
            .read_unaligned()
    }
}

/// Writes `bytes` at offset `off` of the first mbuf of `m`, whose length covers them.
fn mbuf_write(m: &Mbuf, off: usize, bytes: &[u8]) {
    if off + bytes.len() > m.m_len().get() as usize {
        panic(format_args!("tcp_input: write past the mbuf"));
    }
    // SAFETY: the bytes lie inside the mbuf's data, checked just above.
    unsafe {
        ptr::copy_nonoverlapping(bytes.as_ptr(), mtod::<u8>(m).add(off), bytes.len());
    }
}

/// `pool_get(&tcpqe_pool, PR_NOWAIT)`: a queue entry, empty.
fn tcpqe_get() -> Option<&'static Tcpqent> {
    let raw = pool_get(&TCPQE_POOL, PR_NOWAIT)?.cast::<Tcpqent>();
    // SAFETY: a fresh, suitably aligned `tcpqe_pool` item (`tcp_init` sizes the pool for a
    // `Tcpqent`), written once before anything else reads it; it stays allocated until the
    // `pool_put`.
    unsafe {
        raw.as_ptr().write(Tcpqent::new(Tcphdr::default(), None));
        Some(&*raw.as_ptr())
    }
}

/// `pool_put(&tcpqe_pool, q)`.
fn tcpqe_put(q: &Tcpqent) {
    pool_put(&TCPQE_POOL, NonNull::from(q).cast());
}

/// `pool_get(&sackhl_pool, PR_NOWAIT)`: a hole, zeroed.
fn sackhole_get() -> Option<&'static Sackhole> {
    let raw = pool_get(&SACKHL_POOL, PR_NOWAIT)?.cast::<Sackhole>();
    // SAFETY: as in `tcpqe_get`, for a `sackhl_pool` item.
    unsafe {
        raw.as_ptr().write(Sackhole::new());
        Some(&*raw.as_ptr())
    }
}

/// `pool_put(&sackhl_pool, p)`.
fn sackhole_put(p: &Sackhole) {
    pool_put(&SACKHL_POOL, NonNull::from(p).cast());
}

/// Whether two holes (or two NULLs) are the same.
fn same_hole(a: Option<&Sackhole>, b: Option<&Sackhole>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => ptr::eq(a, b),
        (None, None) => true,
        _ => false,
    }
}

/// `tcp_reass`: insert segment `th` into the reassembly queue of `tp`. Return `TH_FIN` if
/// reassembly now includes a segment with FIN, or `ENOBUFS` (the C's -1) if the input segment
/// was discarded due to memory pressure. The common case (segment is the next to be received
/// on an established connection, and the queue is empty) is done inline by `tcp_input`,
/// avoiding linkage into and removal from the queue and repetition of various conversions.
/// Set DELACK for segments received in order, but ack immediately when segments are out of
/// order (so fast retransmit can work).
pub fn tcp_reass(
    tp: &'static Tcpcb,
    th: &mut Tcphdr,
    m: &'static Mbuf,
    tlen: &mut i32,
) -> Result<u8, Errno> {
    // Allocate a new queue entry, before we throw away any data. If we can't, just drop the
    // packet. XXX
    let tiqe = match tcpqe_get() {
        Some(q) => q,
        None => {
            // Any SACK report for data we are about to discard is stale.
            if tp.sack_enable.get() != 0 && tp.rcv_numsacks.get() != 0 {
                tcp_clean_sackreport(tp);
            }
            let last = tp.t_segq.last();
            if let Some(q) = last
                && th.th_seq == tp.rcv_nxt.get()
            {
                // Reuse last entry since new segment fills a hole
                m_freem(q.tcpqe_m.get());
                // SAFETY: `q` is the last entry of this queue.
                unsafe { tp.t_segq.remove(q) };
            }
            match last {
                Some(q) if th.th_seq == tp.rcv_nxt.get() => q,
                _ => {
                    // Flush segment queue for this connection
                    tcp_freeq(tp);
                    tcpstat_inc(TcpstatCounters::TcpsRcvmemdrop);
                    m_freem(m);
                    return Err(Errno::ENOBUFS);
                }
            }
        }
    };

    // Find a segment which begins after this one does.
    let mut p: Option<&'static Tcpqent> = None;
    let mut q = tp.t_segq.first();
    while let Some(e) = q {
        if seq_gt(e.tcpqe_tcp.get().th_seq, th.th_seq) {
            break;
        }
        p = Some(e);
        q = Tcpqehead::next(e);
    }

    // If there is a preceding segment, it may provide some of our data already. If so, drop
    // the data from the incoming segment. If it provides all of our data, drop us.
    if let Some(p) = p {
        let phdr = p.tcpqe_tcp.get();
        // conversion to int (in i) handles seq wraparound
        let i = phdr
            .th_seq
            .wrapping_add(u32::from(phdr.th_reseqlen()))
            .wrapping_sub(th.th_seq) as i32;
        if i > 0 {
            if i >= *tlen {
                tcpstat_pkt(
                    TcpstatCounters::TcpsRcvduppack,
                    TcpstatCounters::TcpsRcvdupbyte,
                    *tlen as u64,
                );
                m_freem(m);
                tcpqe_put(tiqe);
                return Ok(0);
            }
            m_adj(m, i);
            *tlen -= i;
            th.th_seq = th.th_seq.wrapping_add(i as u32);
        }
    }
    tcpstat_pkt(
        TcpstatCounters::TcpsRcvoopack,
        TcpstatCounters::TcpsRcvoobyte,
        *tlen as u64,
    );
    tp.t_rcvoopack.set(tp.t_rcvoopack.get().wrapping_add(1));

    // While we overlap succeeding segments trim them or, if they are completely covered,
    // dequeue them.
    while let Some(e) = q {
        let mut qhdr = e.tcpqe_tcp.get();
        let i = th
            .th_seq
            .wrapping_add(*tlen as u32)
            .wrapping_sub(qhdr.th_seq) as i32;

        if i <= 0 {
            break;
        }
        if i < i32::from(qhdr.th_reseqlen()) {
            qhdr.th_seq = qhdr.th_seq.wrapping_add(i as u32);
            qhdr.set_th_reseqlen(qhdr.th_reseqlen() - i as u16);
            e.tcpqe_tcp.set(qhdr);
            m_adj(e.tcpqe_m.get(), i);
            break;
        }
        let nq = Tcpqehead::next(e);
        m_freem(e.tcpqe_m.get());
        // SAFETY: `e` is in this queue.
        unsafe { tp.t_segq.remove(e) };
        tcpqe_put(e);
        q = nq;
    }

    // Insert the new segment queue entry into place.
    tiqe.tcpqe_m.set(Some(m));
    th.set_th_reseqlen(*tlen as u16);
    tiqe.tcpqe_tcp.set(*th);
    match p {
        // SAFETY: `tiqe` is a pool item in no queue (fresh, or just removed); it stays in
        // place until `tcp_flush_queue` or `tcp_freeq` takes it off.
        None => unsafe { tp.t_segq.insert_head(tiqe) },
        // SAFETY: as above; `p` is in this queue.
        Some(p) => unsafe { tp.t_segq.insert_after(p, tiqe) },
    }

    if th.th_seq != tp.rcv_nxt.get() {
        return Ok(0);
    }

    Ok(tcp_flush_queue(tp))
}

/// `tcp_flush_queue`: present data to user, advancing `rcv_nxt` through completed sequence
/// space; `TH_FIN` when the last segment presented carries a FIN.
fn tcp_flush_queue(tp: &'static Tcpcb) -> u8 {
    let so = tp.socket();

    if !tcps_haveestablished(tp.t_state.get()) {
        return 0;
    }
    let mut q = tp.t_segq.first();
    match q {
        Some(e) if e.tcpqe_tcp.get().th_seq == tp.rcv_nxt.get() => {}
        _ => return 0,
    }
    if let Some(e) = q
        && tp.t_state.get() == TCPS_SYN_RECEIVED
        && e.tcpqe_tcp.get().th_reseqlen() != 0
    {
        return 0;
    }
    let mut flags = 0;
    // do { ... } while (q != NULL && q->tcpqe_tcp->th_seq == tp->rcv_nxt && !flags): the
    // first entry is the one checked above.
    while let Some(e) = q {
        let hdr = e.tcpqe_tcp.get();
        tp.rcv_nxt
            .set(tp.rcv_nxt.get().wrapping_add(u32::from(hdr.th_reseqlen())));
        flags = hdr.th_flags & TH_FIN;

        let nq = Tcpqehead::next(e);
        // SAFETY: `e` is in this queue.
        unsafe { tp.t_segq.remove(e) };
        if so.so_rcv.has_state(SS_CANTRCVMORE) {
            m_freem(e.tcpqe_m.get());
        } else if let Some(m) = e.tcpqe_m.get() {
            mtx_enter(&so.so_rcv.sb_mtx);
            sbappendstream(&so.so_rcv, m);
            mtx_leave(&so.so_rcv.sb_mtx);
        }
        tcpqe_put(e);
        q = nq;
        if !q.is_some_and(|n| n.tcpqe_tcp.get().th_seq == tp.rcv_nxt.get()) || flags != 0 {
            break;
        }
    }
    sorwakeup(so);
    flags
}

/// `tcp_input`: a TCP segment from IP. Without a softnet thread's stack (`ns`) it is
/// processed now; otherwise it is queued on the stack's list for `tcp_input_mlist`, with the
/// header offset in `ph_cookie`.
pub fn tcp_input(
    mp: &mut Option<&'static Mbuf>,
    offp: &mut i32,
    proto: i32,
    af: i32,
    ns: Option<&Netstack>,
) -> i32 {
    let Some(ns) = ns else {
        return tcp_input_solocked(mp, offp, proto, af, None);
    };
    let Some(m) = *mp else {
        return IPPROTO_DONE;
    };
    m.m_pkthdr()
        .ph_cookie
        .set(ptr::without_provenance_mut(*offp as usize));
    match af {
        AF_INET_I32 => ml_enqueue(&ns.ns_tcp_ml, m),
        #[cfg(feature = "inet6")]
        AF_INET6_I32 => ml_enqueue(&ns.ns_tcp6_ml, m),
        _ => {
            m_freemp(mp);
        }
    }
    *mp = None;
    IPPROTO_DONE
}

/// `tcp_input_mlist`: processes the segments queued by `tcp_input`, keeping the socket locked
/// while consecutive segments belong to one connection.
pub fn tcp_input_mlist(ml: &MbufList, af: i32) {
    let mut so: Option<&'static Socket> = None;

    while let Some(m) = ml_dequeue(ml) {
        let mut off = m.m_pkthdr().ph_cookie.get().addr() as i32;
        m.m_pkthdr().ph_cookie.set(ptr::null_mut());
        let mut mp = Some(m);
        let nxt = tcp_input_solocked(&mut mp, &mut off, IPPROTO_TCP, af, Some(&mut so));
        kassert!(nxt == IPPROTO_DONE);
    }

    in_pcbsounlock(None, so);
}

/// `tcp_input_solocked`: TCP input routine, follows pages 65-76 of the protocol specification
/// dated September, 1981 very closely. With `solocked`, the socket stays locked for the
/// caller's next segment and one it already holds is reused or unlocked.
fn tcp_input_solocked(
    mp: &mut Option<&'static Mbuf>,
    offp: &mut i32,
    _proto: i32,
    af: i32,
    mut solocked: Option<&mut Option<&'static Socket>>,
) -> i32 {
    let mut st = TcpInputState {
        m: *mp,
        inp: None,
        so: None,
        tp: None,
        otp: ptr::null(),
        ostate: 0,
        saveti: [0; SAVETI_LEN],
        th: Tcphdr::default(),
        tiflags: 0,
        tlen: 0,
        now: 0,
    };

    let mut exit = tcp_input_body(&mut st, mp, *offp, af, solocked.as_deref_mut());
    loop {
        match exit {
            TcpInputExit::Return => return IPPROTO_DONE,
            TcpInputExit::Done => {
                match solocked {
                    Some(sl) => *sl = st.so,
                    None => in_pcbsounlock(st.inp, st.so),
                }
                in_pcbunref(st.inp);
                return IPPROTO_DONE;
            }
            TcpInputExit::BadSyn => {
                // Received a bad SYN. Increment counters and dropwithreset.
                tcpstat_inc(TcpstatCounters::TcpsBadsyn);
                st.tp = None;
                exit = TcpInputExit::DropWithReset;
            }
            TcpInputExit::DropAfterAckRatelim => {
                let pps = TCP_ACKDROP_PPS.as_ptr();
                // SAFETY: the pair lives forever and is touched only here, through
                // `ppsratecheck_shared`, which dereferences it only inside
                // `ppsratecheck_mtx`.
                let ok = unsafe {
                    ppsratecheck_shared(
                        &raw mut (*pps).last,
                        &raw mut (*pps).count,
                        TCP_ACKDROP_PPSLIM.load(Ordering::Relaxed),
                    )
                };
                exit = if ok {
                    // ...fall into dropafterack...
                    TcpInputExit::DropAfterAck
                } else {
                    // XXX stat
                    TcpInputExit::Drop
                };
            }
            TcpInputExit::DropAfterAck => {
                // Generate an ACK dropping incoming segment if it occupies sequence space,
                // where the ACK reflects our state.
                if st.tiflags & TH_RST != 0 {
                    exit = TcpInputExit::Drop;
                    continue;
                }
                m_freem(st.m.take());
                if let Some(tp) = st.tp {
                    tp.set_flags(TF_ACKNOW);
                    let _ = tcp_output(tp);
                }
                exit = TcpInputExit::Done;
            }
            TcpInputExit::DropWithResetRatelim => {
                // We may want to rate-limit RSTs in certain situations, particularly if we
                // are sending an RST in response to an attempt to connect to or otherwise
                // communicate with a port for which we have no socket.
                let pps = TCP_RST_PPS.as_ptr();
                // SAFETY: as for `TCP_ACKDROP_PPS`.
                let ok = unsafe {
                    ppsratecheck_shared(
                        &raw mut (*pps).last,
                        &raw mut (*pps).count,
                        TCP_RST_PPSLIM.load(Ordering::Relaxed),
                    )
                };
                exit = if ok {
                    // ...fall into dropwithreset...
                    TcpInputExit::DropWithReset
                } else {
                    // XXX stat
                    TcpInputExit::Drop
                };
            }
            TcpInputExit::DropWithReset => {
                // Generate a RST, dropping incoming segment. Make ACK acceptable to
                // originator of segment. Don't bother to respond to RST.
                if st.tiflags & TH_RST != 0 {
                    exit = TcpInputExit::Drop;
                    continue;
                }
                if let Some(m) = st.m {
                    let rtableid = m.m_pkthdr().ph_rtableid.get();
                    let template = ip_template(m);
                    if st.tiflags & TH_ACK != 0 {
                        tcp_respond(
                            st.tp,
                            &template,
                            Some(&st.th),
                            0,
                            st.th.th_ack,
                            TH_RST,
                            rtableid,
                            st.now,
                        );
                    } else {
                        if st.tiflags & TH_SYN != 0 {
                            st.tlen += 1;
                        }
                        tcp_respond(
                            st.tp,
                            &template,
                            Some(&st.th),
                            st.th.th_seq.wrapping_add(st.tlen as u32),
                            0,
                            TH_RST | TH_ACK,
                            rtableid,
                            st.now,
                        );
                    }
                }
                m_freem(st.m.take());
                in_pcbsounlock(st.inp, st.so);
                in_pcbunref(st.inp);
                return IPPROTO_DONE;
            }
            TcpInputExit::Drop => {
                // Drop space held by incoming segment and return.
                if !st.otp.is_null() {
                    tcp_trace(
                        TA_DROP,
                        st.ostate,
                        st.tp,
                        st.otp,
                        Some(&st.saveti),
                        0,
                        st.tlen,
                    );
                }

                m_freem(st.m.take());
                in_pcbsounlock(st.inp, st.so);
                in_pcbunref(st.inp);
                return IPPROTO_DONE;
            }
        }
    }
}

/// The body of `tcp_input_solocked`, up to the label it ends at.
fn tcp_input_body(
    st: &mut TcpInputState,
    mp: &mut Option<&'static Mbuf>,
    iphlen: i32,
    af: i32,
    mut solocked: Option<&mut Option<&'static Socket>>,
) -> TcpInputExit {
    use TcpInputExit::{
        BadSyn, Done, Drop, DropAfterAck, DropAfterAckRatelim, DropWithReset, DropWithResetRatelim,
        Return,
    };

    let mut optbuf = [0u8; MAX_TCPOPTLEN];
    let mut optlen = 0usize;
    let mut have_optp = false;
    let mut reuse: Option<TcpSeq> = None;
    let mut opti = TcpOptInfo::default();

    tcpstat_inc(TcpstatCounters::TcpsRcvtotal);

    opti.ts_present = false;
    opti.maxseg = 0;
    st.now = tcp_now();
    let now = st.now;
    let do_ecn = TCP_DO_ECN.load(Ordering::Relaxed) != 0;

    let Some(m) = st.m else {
        return Return;
    };

    // RFC1122 4.2.3.10, p. 104: discard bcast/mcast SYN
    if m.m_flags().get() & (M_BCAST | M_MCAST) != 0 {
        return Drop;
    }

    // Get IP and TCP header together in first mbuf. Note: IP leaves IP header in first mbuf.
    let Some(thp) = ip6_exthdr_get(mp, iphlen, TCP_HDR_LEN as i32) else {
        st.m = None;
        tcpstat_inc(TcpstatCounters::TcpsRcvshort);
        return Return;
    };
    // SAFETY: `ip6_exthdr_get` made the header contiguous at `thp`, inside `m`.
    let mut th = unsafe { thp.cast::<Tcphdr>().read_unaligned() };

    st.tlen = m.m_pkthdr().len.get() - iphlen;
    // `ip` or `ip6`, copies of the network header; `iptos` its TOS byte.
    let (ip, ip6, iptos): (Option<Ip>, Option<Ip6Hdr>, u8) = match af {
        AF_INET_I32 => {
            let ip = mtod_ip(m);
            // save ip_tos before clearing it for checksum
            (Some(ip), None, ip.ip_tos)
        }
        #[cfg(feature = "inet6")]
        AF_INET6_I32 => {
            let ip6 = mtod_ip6(m);
            let iptos = ((ntohl(ip6.ip6_flow) >> 20) & 0xff) as u8;

            // Be proactive about unspecified IPv6 address in source. As we use all-zero to
            // indicate unbounded/unconnected pcb, unspecified IPv6 address can be used to
            // confuse us.
            //
            // Note that packets with unspecified IPv6 destination is already dropped in
            // ip6_input.
            if in6_is_addr_unspecified(&ip6.ip6_src) {
                // XXX stat
                return Drop;
            }

            // Discard packets to multicast
            if in6_is_addr_multicast(&ip6.ip6_dst) {
                // XXX stat
                return Drop;
            }
            (None, Some(ip6), iptos)
        }
        _ => unhandled_af(af),
    };

    // Checksum extended TCP header and data.
    if m.m_pkthdr().csum_flags.get() & M_TCP_CSUM_IN_OK == 0 {
        if m.m_pkthdr().csum_flags.get() & M_TCP_CSUM_IN_BAD != 0 {
            tcpstat_inc(TcpstatCounters::TcpsRcvbadsum);
            return Drop;
        }
        tcpstat_inc(TcpstatCounters::TcpsInswcsum);
        let sum = match af {
            #[cfg(feature = "inet6")]
            AF_INET6_I32 => in6_cksum(m, IPPROTO_TCP as u8, iphlen as u32, st.tlen as u32),
            _ => in4_cksum(m, IPPROTO_TCP as u8, iphlen, st.tlen),
        };
        if sum != 0 {
            tcpstat_inc(TcpstatCounters::TcpsRcvbadsum);
            return Drop;
        }
    }

    // Check that TCP offset makes sense, pull out TCP options and adjust length. XXX
    let off = i32::from(th.th_off()) << 2;
    if off < TCP_HDR_LEN as i32 || off > st.tlen {
        tcpstat_inc(TcpstatCounters::TcpsRcvbadoff);
        return Drop;
    }
    st.tlen -= off;
    if off > TCP_HDR_LEN as i32 {
        let Some(thp) = ip6_exthdr_get(mp, iphlen, off) else {
            st.m = None;
            tcpstat_inc(TcpstatCounters::TcpsRcvshort);
            return Return;
        };
        optlen = off as usize - TCP_HDR_LEN;
        // SAFETY: `ip6_exthdr_get` made the `off` bytes of header and options contiguous at
        // `thp`, inside `m`; `optlen` is at most `MAX_TCPOPTLEN` (`th_off` is four bits).
        unsafe {
            th = thp.cast::<Tcphdr>().read_unaligned();
            ptr::copy_nonoverlapping(thp.add(TCP_HDR_LEN), optbuf.as_mut_ptr(), optlen);
        }
        have_optp = true;
        // Do quick retrieval of timestamp options ("options prediction?"). If timestamp is
        // the only option and it's formatted as recommended in RFC 1323 appendix A, we
        // quickly get the values now and not bother calling tcp_dooptions(), etc.
        let appa = usize::from(TCPOLEN_TSTAMP_APPA);
        if (optlen == appa || (optlen > appa && optbuf[appa] == TCPOPT_EOL))
            && u32::from_ne_bytes([optbuf[0], optbuf[1], optbuf[2], optbuf[3]])
                == htonl(TCPOPT_TSTAMP_HDR)
            && th.th_flags & TH_SYN == 0
        {
            opti.ts_present = true;
            opti.ts_val = u32::from_be_bytes([optbuf[4], optbuf[5], optbuf[6], optbuf[7]]);
            opti.ts_ecr = u32::from_be_bytes([optbuf[8], optbuf[9], optbuf[10], optbuf[11]]);
            have_optp = false; // we've parsed the options
        }
    }
    st.tiflags = th.th_flags;

    // Convert TCP protocol specific fields to host format.
    th.th_seq = ntohl(th.th_seq);
    th.th_ack = ntohl(th.th_ack);
    th.th_win = ntohs(th.th_win);
    th.th_urp = ntohs(th.th_urp);
    st.th = th;

    if st.th.th_dport == 0 {
        tcpstat_inc(TcpstatCounters::TcpsNoport);
        return DropWithResetRatelim;
    }

    let rtableid = m.m_pkthdr().ph_rtableid.get();

    // Locate pcb for segment.
    st.inp = pf_inp_lookup(m);
    let (so, tp, tiwin, skip_to_step6) = 'findpcb: loop {
        if st.inp.is_none() {
            #[cfg(feature = "inet6")]
            if let Some(ip6) = ip6 {
                st.inp = in6_pcblookup(
                    &TCB6TABLE,
                    &ip6.ip6_src,
                    st.th.th_sport,
                    &ip6.ip6_dst,
                    st.th.th_dport,
                    rtableid,
                );
            }
            if let Some(ip) = ip {
                st.inp = in_pcblookup(
                    &TCBTABLE,
                    ip.ip_src,
                    st.th.th_sport,
                    ip.ip_dst,
                    st.th.th_dport,
                    rtableid,
                );
            }
        }
        if st.inp.is_none() {
            tcpstat_inc(TcpstatCounters::TcpsPcbhashmiss);
            #[cfg(feature = "inet6")]
            if let Some(ip6) = ip6 {
                st.inp = in6_pcblookup_listen(
                    &TCB6TABLE,
                    &ip6.ip6_dst,
                    st.th.th_dport,
                    Some(m),
                    rtableid,
                );
            }
            if let Some(ip) = ip {
                st.inp =
                    in_pcblookup_listen(&TCBTABLE, ip.ip_dst, st.th.th_dport, Some(m), rtableid);
            }
            // If the state is CLOSED (i.e., TCB does not exist) then all data in the
            // incoming segment is discarded. If the TCB exists but is in CLOSED state, it is
            // embryonic, but should either do a listen or a connect soon.
        }
        if IPSEC_IN_USE.load(Ordering::Relaxed) != 0 {
            // Find most recent IPsec tag
            let tdb = match m_tag_find(m, PACKET_TAG_IPSEC_IN_DONE, None) {
                Some(mtag) => {
                    // SAFETY: `IPSEC_IN_DONE` tags carry a `struct tdb_ident`.
                    let tdbi = unsafe { TdbIdent::read(mtag.data()) };
                    gettdb(tdbi.rdomain, tdbi.spi, &tdbi.dst, tdbi.proto)
                }
                None => None,
            };
            let seclevel = st.inp.map(|i| i.inp_seclevel.get());
            let error = ipsp_spd_lookup(
                m,
                af,
                iphlen,
                IPSP_DIRECTION_IN,
                tdb,
                seclevel.as_ref(),
                None,
                None,
            );
            tdb_unref(tdb);
            if error.is_err() {
                tcpstat_inc(TcpstatCounters::TcpsRcvnosec);
                return Drop;
            }
        }

        let Some(inp) = st.inp else {
            tcpstat_inc(TcpstatCounters::TcpsNoport);
            return DropWithResetRatelim;
        };
        // Avoid needless lock and unlock operation when handling multiple TCP packets from
        // the same stream consecutively.
        st.so = match solocked.as_deref_mut() {
            Some(sl) if sl.is_some_and(|s| sotoinpcb(s).is_some_and(|i| ptr::eq(i, inp))) => {
                sl.take()
            }
            Some(sl) => {
                if let Some(s) = sl.take() {
                    in_pcbsounlock(None, Some(s));
                }
                in_pcbsolock(inp)
            }
            None => in_pcbsolock(inp),
        };
        let Some(mut so) = st.so else {
            tcpstat_inc(TcpstatCounters::TcpsClosing);
            return DropWithResetRatelim;
        };
        kassert!(sotoinpcb(inp.socket()).is_some_and(|i| ptr::eq(i, inp)));
        kassert!(intotcpcb(inp).is_none_or(|t| ptr::eq(t.t_inpcb, inp)));
        soassertlocked(inp.socket());

        // Check the minimum TTL for socket.
        if let Some(ip) = ip {
            let minttl = inp.inp_ip_minttl.get();
            if minttl != 0 && minttl > ip.ip_ttl {
                return Drop;
            }
        }
        #[cfg(feature = "inet6")]
        if let Some(ip6) = ip6 {
            let minhlim = inp.inp_ip6_minhlim().get();
            if minhlim != 0 && minhlim > ip6.ip6_hlim {
                return Drop;
            }
        }

        let Some(mut tp) = intotcpcb(inp) else {
            return DropWithResetRatelim;
        };
        st.tp = Some(tp);
        if tp.t_state.get() == TCPS_CLOSED {
            return Drop;
        }
        let mut inp = inp;

        // Unscale the window into a 32-bit value.
        let tiwin: u64 = if st.tiflags & TH_SYN == 0 {
            u64::from(st.th.th_win) << tp.snd_scale.get()
        } else {
            u64::from(st.th.th_win)
        };

        if so.has_options(SO_DEBUG | SO_ACCEPTCONN) {
            let mut src = SockaddrUnion::new();
            let mut dst = SockaddrUnion::new();
            if let Some(ip) = ip {
                src.set_sin(&SockaddrIn {
                    sin_len: size_of::<SockaddrIn>() as u8,
                    sin_family: AF_INET,
                    sin_port: st.th.th_sport,
                    sin_addr: ip.ip_src,
                    ..SockaddrIn::default()
                });
                dst.set_sin(&SockaddrIn {
                    sin_len: size_of::<SockaddrIn>() as u8,
                    sin_family: AF_INET,
                    sin_port: st.th.th_dport,
                    sin_addr: ip.ip_dst,
                    ..SockaddrIn::default()
                });
            }
            #[cfg(feature = "inet6")]
            if let Some(ip6) = ip6 {
                src = sa_from_sin6(&SockaddrIn6 {
                    sin6_port: st.th.th_sport,
                    ..SockaddrIn6::with_addr(ip6.ip6_src)
                });
                dst = sa_from_sin6(&SockaddrIn6 {
                    sin6_port: st.th.th_dport,
                    ..SockaddrIn6::with_addr(ip6.ip6_dst)
                });
            }

            if so.has_options(SO_DEBUG) {
                st.otp = ptr::from_ref(tp);
                st.ostate = tp.t_state.get();
                let hlen = if ip6.is_some() {
                    IP6_HDR_LEN
                } else {
                    IP_HDR_LEN
                };
                m_copydata(m, 0, &mut st.saveti[..hlen]);
                st.saveti[hlen..hlen + TCP_HDR_LEN].copy_from_slice(&th_bytes(&st.th));
            }
            if so.has_options(SO_ACCEPTCONN) {
                match st.tiflags & (TH_RST | TH_SYN | TH_ACK) {
                    // TH_SYN|TH_ACK|TH_RST, TH_SYN|TH_RST, TH_ACK|TH_RST, TH_RST
                    f if f & TH_RST != 0 => {
                        syn_cache_reset(&src, &dst, &st.th, inp.inp_rtableid.get());
                        return Drop;
                    }

                    f if f == TH_SYN | TH_ACK => {
                        // Received a SYN,ACK. This should never happen while we are in
                        // LISTEN. Send an RST.
                        return BadSyn;
                    }

                    TH_ACK => {
                        let th = st.th;
                        match syn_cache_get(&src, &dst, &th, iphlen, st.tlen, so, m, now, do_ecn) {
                            SynCacheGet::NotFound => {
                                // We don't have a SYN for this ACK; send an RST.
                                st.so = None;
                                return BadSyn;
                            }
                            SynCacheGet::Aborted => {
                                // We were unable to create the connection. If the 3-way
                                // handshake was completed, and RST has been sent to the peer.
                                // Since the mbuf might be in use for the reply, do not free
                                // it.
                                st.so = None;
                                st.m = None;
                                *mp = None;
                                return Drop;
                            }
                            SynCacheGet::Socket(nso) => {
                                // We have created a full-blown connection.
                                st.so = Some(nso);
                                so = nso;
                                in_pcbunref(st.inp);
                                // syn_cache_get() has refcounted inp
                                st.inp = sotoinpcb(nso);
                                st.tp = st.inp.and_then(intotcpcb);
                                let (Some(ninp), Some(ntp)) = (st.inp, st.tp) else {
                                    return BadSyn; // XXX
                                };
                                inp = ninp;
                                tp = ntp;
                            }
                        }
                    }

                    TH_SYN => {
                        // Received a SYN.

                        // LISTEN socket received a SYN from itself? This can't possibly be
                        // valid; drop the packet.
                        if st.th.th_dport == st.th.th_sport {
                            #[cfg(feature = "inet6")]
                            if let Some(ip6) = ip6
                                && in6_are_addr_equal(&ip6.ip6_src, &ip6.ip6_dst)
                            {
                                tcpstat_inc(TcpstatCounters::TcpsBadsyn);
                                return Drop;
                            }
                            if let Some(ip) = ip
                                && ip.ip_dst.s_addr == ip.ip_src.s_addr
                            {
                                tcpstat_inc(TcpstatCounters::TcpsBadsyn);
                                return Drop;
                            }
                        }

                        // SYN looks ok; create compressed TCP state for it.
                        let optp = have_optp.then_some(&optbuf[..optlen]);
                        if so.so_qlen.get() > so.so_qlimit.get()
                            || !syn_cache_add(
                                &src, &dst, &st.th, iphlen, so, m, optp, &mut opti, reuse, now,
                                do_ecn,
                            )
                        {
                            tcpstat_inc(TcpstatCounters::TcpsDropsyn);
                            return Drop;
                        }
                        // syn_cache_respond consumed the mbuf.
                        st.m = None;
                        return Done;
                    }

                    _ => {
                        // None of RST, SYN or ACK was set. This is an invalid packet for a
                        // TCB in LISTEN state. Send a RST.
                        return BadSyn;
                    }
                }
            }
        }

        // Should not happen now that all embryonic connections are handled with compressed
        // state.
        #[cfg(feature = "diagnostic")]
        if tp.t_state.get() == TCPS_LISTEN {
            panic(format_args!("tcp_input: TCPS_LISTEN"));
        }

        pf_inp_link(m, Some(inp));

        // Segment received on connection. Reset idle time and keep-alive timer.
        tp.t_rcvtime.set(now);
        if tcps_haveestablished(tp.t_state.get()) {
            tcp_timer_arm(tp, TCPT_KEEP, TCP_KEEPIDLE.load(Ordering::Relaxed) as u64);
        }

        if tp.sack_enable.get() != 0 {
            tcp_del_sackholes(tp, &st.th); // Delete stale SACK holes
        }

        // Process options.
        if have_optp || tp.has_flags(TF_SIGNATURE) {
            let optp: &[u8] = if have_optp { &optbuf[..optlen] } else { &[] };
            if !tcp_dooptions(tp, optp, &st.th, m, iphlen, &mut opti, rtableid, now) {
                return Drop;
            }
        }

        if opti.ts_present && opti.ts_ecr != 0 {
            // subtract out the tcp timestamp modulator
            opti.ts_ecr = opti.ts_ecr.wrapping_sub(tp.ts_modulate.get());

            // make sure ts_ecr is sensible
            let rtt_test = now.wrapping_sub(u64::from(opti.ts_ecr)) as i32;
            if !(0..=TCP_RTT_MAX).contains(&rtt_test) {
                opti.ts_ecr = 0;
            }
        }

        // if congestion experienced, set ECE bit in subsequent packets.
        if iptos & IPTOS_ECN_MASK == IPTOS_ECN_CE {
            tp.set_flags(TF_RCVD_CE);
            tcpstat_inc(TcpstatCounters::TcpsEcnRcvce);
        }

        // Header prediction: check for the two common cases of a uni-directional data xfer.
        // If the packet has no control flags, is in-sequence, the window didn't change and
        // we're not retransmitting, it's a candidate. If the length is zero and the ack moved
        // forward, we're the sender side of the xfer. Just free the data acked & wake any
        // higher level process that was blocked waiting for space. If the length is non-zero
        // and the ack didn't move, we're the receiver side. If we're getting packets in-order
        // (the reassembly queue is empty), add the data to the socket buffer and note that we
        // need a delayed ack.
        if tp.t_state.get() == TCPS_ESTABLISHED
            && st.tiflags & (TH_SYN | TH_FIN | TH_RST | TH_URG | TH_ECE | TH_CWR | TH_ACK) == TH_ACK
            && (!opti.ts_present || tstmp_geq(opti.ts_val, tp.ts_recent.get()))
            && st.th.th_seq == tp.rcv_nxt.get()
            && tiwin != 0
            && tiwin == tp.snd_wnd.get()
            && tp.snd_nxt.get() == tp.snd_max.get()
        {
            // If last ACK falls within this segment's sequence numbers, record the
            // timestamp. Fix from Braden, see Stevens p. 870
            if opti.ts_present && seq_leq(st.th.th_seq, tp.last_ack_sent.get()) {
                tp.ts_recent_age.set(now);
                tp.ts_recent.set(opti.ts_val);
            }

            if st.tlen == 0 {
                if seq_gt(st.th.th_ack, tp.snd_una.get())
                    && seq_leq(st.th.th_ack, tp.snd_max.get())
                    && tp.snd_cwnd.get() >= tp.snd_wnd.get()
                    && tp.t_dupacks.get() == 0
                {
                    // this is a pure ack for outstanding data.
                    tcpstat_inc(TcpstatCounters::TcpsPredack);
                    if opti.ts_present && opti.ts_ecr != 0 {
                        tcp_xmit_timer(tp, now.wrapping_sub(u64::from(opti.ts_ecr)) as i32);
                    } else if tp.t_rtttime.get() != 0 && seq_gt(st.th.th_ack, tp.t_rtseq.get()) {
                        tcp_xmit_timer(tp, now.wrapping_sub(tp.t_rtttime.get()) as i32);
                    }
                    let acked = st.th.th_ack.wrapping_sub(tp.snd_una.get()) as i32;
                    tcpstat_pkt(
                        TcpstatCounters::TcpsRcvackpack,
                        TcpstatCounters::TcpsRcvackbyte,
                        acked as u64,
                    );
                    tp.t_rcvacktime.set(now);

                    mtx_enter(&so.so_snd.sb_mtx);
                    sbdrop(&so.so_snd, acked);
                    mtx_leave(&so.so_snd.sb_mtx);

                    // If we had a pending ICMP message that refers to data that have just
                    // been acknowledged, disregard the recorded ICMP message.
                    if tp.has_flags(TF_PMTUD_PEND) && seq_gt(st.th.th_ack, tp.t_pmtud_th_seq.get())
                    {
                        tp.clear_flags(TF_PMTUD_PEND);
                    }

                    // Keep track of the largest chunk of data acknowledged since last PMTU
                    // update
                    if tp.t_pmtud_mss_acked.get() < acked as u32 {
                        tp.t_pmtud_mss_acked.set(acked as u32);
                    }

                    tp.snd_una.set(st.th.th_ack);
                    // Pull snd_wl2 up to prevent seq wrap.
                    tp.snd_wl2.set(st.th.th_ack);
                    // We want snd_last to track snd_una so as to avoid sequence wraparound
                    // problems for very large transfers.
                    if seq_gt(tp.snd_una.get(), tp.snd_last.get()) {
                        tp.snd_last.set(tp.snd_una.get());
                    }
                    m_freem(st.m.take());

                    // If all outstanding data are acked, stop retransmit timer, otherwise
                    // restart timer using current (possibly backed-off) value. If process is
                    // waiting for space, wakeup/selwakeup/signal. If data are ready to send,
                    // let tcp_output decide between more output or persist.
                    if tp.snd_una.get() == tp.snd_max.get() {
                        tcp_timer_disarm(tp, TCPT_REXMT);
                    } else if !tcp_timer_isarmed(tp, TCPT_PERSIST) {
                        tcp_timer_arm(tp, TCPT_REXMT, tp.t_rxtcur.get() as u64);
                    }

                    tcp_update_sndspace(tp);
                    if sb_notify(&so.so_snd) {
                        sowwakeup(so);
                    }
                    if so.so_snd.sb_cc.get() != 0 || tp.has_flags(TF_NEEDOUTPUT) {
                        let _ = tcp_output(tp);
                    }
                    return Done;
                }
            } else if st.th.th_ack == tp.snd_una.get()
                && tp.t_segq.is_empty()
                && i64::from(st.tlen) <= sbspace(&so.so_rcv)
            {
                // This is a pure, in-sequence data packet with nothing on the reassembly
                // queue and we have enough buffer space to take it.
                // Clean receiver SACK report if present
                if tp.sack_enable.get() != 0 && tp.rcv_numsacks.get() != 0 {
                    tcp_clean_sackreport(tp);
                }
                tcpstat_inc(TcpstatCounters::TcpsPreddat);
                tp.rcv_nxt
                    .set(tp.rcv_nxt.get().wrapping_add(st.tlen as u32));
                // Pull snd_wl1 and rcv_up up to prevent seq wrap.
                tp.snd_wl1.set(st.th.th_seq);
                // Packet has most recent segment, no urgent exists.
                tp.rcv_up.set(tp.rcv_nxt.get());
                tcpstat_pkt(
                    TcpstatCounters::TcpsRcvpack,
                    TcpstatCounters::TcpsRcvbyte,
                    st.tlen as u64,
                );

                tcp_setup_ack(tp, st.tiflags, Some(m));
                // Drop TCP, IP headers and TCP options then add data to socket buffer.
                if so.so_rcv.has_state(SS_CANTRCVMORE) {
                    m_freem(m);
                } else {
                    let srtt = tp.t_srtt.get();
                    if srtt != 0
                        && tp.rfbuf_ts.get() != 0
                        && now.wrapping_sub(tp.rfbuf_ts.get())
                            > (srtt >> (TCP_RTT_SHIFT + TCP_RTT_BASE_SHIFT)) as u64
                    {
                        tcp_update_rcvspace(tp);
                        // Start over with next RTT.
                        tp.rfbuf_cnt.set(0);
                        tp.rfbuf_ts.set(0);
                    } else {
                        tp.rfbuf_cnt
                            .set(tp.rfbuf_cnt.get().wrapping_add(st.tlen as u32));
                    }
                    m_adj(m, iphlen + off);
                    mtx_enter(&so.so_rcv.sb_mtx);
                    sbappendstream(&so.so_rcv, m);
                    mtx_leave(&so.so_rcv.sb_mtx);
                }
                st.m = None;
                sorwakeup(so);
                if tp.has_flags(TF_ACKNOW | TF_NEEDOUTPUT) {
                    let _ = tcp_output(tp);
                }
                return Done;
            }
        }

        // Calculate amount of space in receive window, and then do TCP input processing.
        // Receive window is amount of space in rcv queue, but not less than advertised
        // window.
        {
            let mut win = sbspace(&so.so_rcv) as i32;
            if win < 0 {
                win = 0;
            }
            tp.rcv_wnd
                .set(max(win, tp.rcv_adv.get().wrapping_sub(tp.rcv_nxt.get()) as i32) as u64);
        }

        match tp.t_state.get() {
            // If the state is SYN_RECEIVED: if seg contains SYN/ACK, send an RST. if seg
            // contains an ACK, but not for our SYN/ACK, send an RST
            TCPS_SYN_RECEIVED => {
                if st.tiflags & TH_ACK != 0 {
                    if st.tiflags & TH_SYN != 0 {
                        tcpstat_inc(TcpstatCounters::TcpsBadsyn);
                        return DropWithReset;
                    }
                    if seq_leq(st.th.th_ack, tp.snd_una.get())
                        || seq_gt(st.th.th_ack, tp.snd_max.get())
                    {
                        return DropWithReset;
                    }
                }
            }

            // If the state is SYN_SENT: if seg contains an ACK, but not for our SYN, drop the
            // input. if seg contains a RST, then drop the connection. if seg does not contain
            // SYN, then drop it. Otherwise this is an acceptable SYN segment: initialize
            // tp->rcv_nxt and tp->irs; if seg contains ack then advance tp->snd_una; if SYN
            // has been acked change to ESTABLISHED else SYN_RCVD state; arrange for segment
            // to be acked (eventually); continue processing rest of data/controls, beginning
            // with URG
            TCPS_SYN_SENT => {
                if st.tiflags & TH_ACK != 0
                    && (seq_leq(st.th.th_ack, tp.iss.get())
                        || seq_gt(st.th.th_ack, tp.snd_max.get()))
                {
                    return DropWithReset;
                }
                if st.tiflags & TH_RST != 0 {
                    // if ECN is enabled, fall back to non-ecn at rexmit
                    if do_ecn && !tp.has_flags(TF_DISABLE_ECN) {
                        return Drop;
                    }
                    if st.tiflags & TH_ACK != 0 {
                        st.tp = tcp_drop(tp, Some(Errno::ECONNREFUSED));
                    }
                    return Drop;
                }
                if st.tiflags & TH_SYN == 0 {
                    return Drop;
                }
                if st.tiflags & TH_ACK != 0 {
                    tp.snd_una.set(st.th.th_ack);
                    if seq_lt(tp.snd_nxt.get(), tp.snd_una.get()) {
                        tp.snd_nxt.set(tp.snd_una.get());
                    }
                }
                tcp_timer_disarm(tp, TCPT_REXMT);
                tp.irs.set(st.th.th_seq);
                tcp_mss(tp, i32::from(opti.maxseg));
                // Reset initial window to 1 segment for retransmit
                if tp.t_rxtshift.get() > 0 {
                    tp.snd_cwnd.set(u64::from(tp.t_maxseg.get()));
                }
                tcp_rcvseqinit(tp);
                tp.set_flags(TF_ACKNOW);
                // If we've sent a SACK_PERMITTED option, and the peer also replied with one,
                // then TF_SACK_PERMIT should have been set in tcp_dooptions(). If it was
                // not, disable SACKs.
                if tp.sack_enable.get() != 0 {
                    tp.sack_enable
                        .set((tp.t_flags.get() & TF_SACK_PERMIT) as i32);
                }
                // if ECE is set but CWR is not set for SYN-ACK, or both ECE and CWR are set
                // for simultaneous open, peer is ECN capable.
                if do_ecn {
                    let f = st.tiflags & (TH_ACK | TH_ECE | TH_CWR);
                    if f == TH_ACK | TH_ECE || f == TH_ECE | TH_CWR {
                        tp.set_flags(TF_ECN_PERMIT);
                        st.tiflags &= !(TH_ECE | TH_CWR);
                        tcpstat_inc(TcpstatCounters::TcpsEcnAccepts);
                    }
                }

                if st.tiflags & TH_ACK != 0 && seq_gt(tp.snd_una.get(), tp.iss.get()) {
                    tcpstat_inc(TcpstatCounters::TcpsConnects);
                    soisconnected(so);
                    tp.t_state.set(TCPS_ESTABLISHED);
                    tcp_timer_arm(tp, TCPT_KEEP, TCP_KEEPIDLE.load(Ordering::Relaxed) as u64);
                    // Do window scaling on this connection?
                    if tp.t_flags.get() & (TF_RCVD_SCALE | TF_REQ_SCALE)
                        == (TF_RCVD_SCALE | TF_REQ_SCALE)
                    {
                        tp.snd_scale.set(tp.requested_s_scale.get());
                        tp.rcv_scale.set(tp.request_r_scale.get());
                    }
                    tcp_flush_queue(tp);

                    // if we didn't have to retransmit the SYN, use its rtt as our initial
                    // srtt & rtt var.
                    if tp.t_rtttime.get() != 0 {
                        tcp_xmit_timer(tp, now.wrapping_sub(tp.t_rtttime.get()) as i32);
                    }
                    // Since new data was acked (the SYN), open the congestion window by one
                    // MSS. We do this here, because we won't go through the normal ACK
                    // processing below. And since this is the start of the connection, we
                    // know we are in the exponential phase of slow-start.
                    tp.snd_cwnd
                        .set(tp.snd_cwnd.get() + u64::from(tp.t_maxseg.get()));
                } else {
                    tp.t_state.set(TCPS_SYN_RECEIVED);
                }

                // trimthenstep6 (an `#if 0` label in the C):
                // Advance th->th_seq to correspond to first data byte. If data, trim to stay
                // within window, dropping FIN if necessary.
                st.th.th_seq = st.th.th_seq.wrapping_add(1);
                if st.tlen as u64 > tp.rcv_wnd.get() {
                    let todrop = st.tlen - tp.rcv_wnd.get() as i32;
                    m_adj(m, -todrop);
                    st.tlen = tp.rcv_wnd.get() as i32;
                    st.tiflags &= !TH_FIN;
                    tcpstat_pkt(
                        TcpstatCounters::TcpsRcvpackafterwin,
                        TcpstatCounters::TcpsRcvbyteafterwin,
                        todrop as u64,
                    );
                }
                tp.snd_wl1.set(st.th.th_seq.wrapping_sub(1));
                tp.rcv_up.set(st.th.th_seq);
                break 'findpcb (so, tp, tiwin, true);
            }

            // If a new connection request is received while in TIME_WAIT, drop the old
            // connection and start over if the if the timestamp or the sequence numbers are
            // above the previous ones.
            TCPS_TIME_WAIT
                if st.tiflags & (TH_SYN | TH_ACK) == TH_SYN
                    && ((opti.ts_present && tstmp_lt(tp.ts_recent.get(), opti.ts_val))
                        || seq_gt(st.th.th_seq, tp.rcv_nxt.get())) =>
            {
                // The socket will be recreated but the new state has already been linked to
                // the socket. Remove the link between old socket and new state.
                pf_inp_unlink(inp);
                // Advance the iss by at least 32768, but clear the msb in order to make sure
                // that SEG_LT(snd_nxt, iss).
                let iss = tp
                    .snd_nxt
                    .get()
                    .wrapping_add((arc4random() & 0x7fff_ffff) | 0x8000);
                reuse = Some(iss);
                st.tp = tcp_close(tp);
                in_pcbsounlock(Some(inp), Some(so));
                st.so = None;
                in_pcbunref(Some(inp));
                st.inp = None;
                continue 'findpcb;
            }

            _ => {}
        }
        break 'findpcb (so, tp, tiwin, false);
    };
    let mut tiwin = tiwin;

    // Compute mbuf offset to TCP data segment.
    let mut hdroptlen = iphlen + off;

    if !skip_to_step6 {
        // States other than LISTEN or SYN_SENT. First check timestamp, if present. Then check
        // that at least some bytes of segment are within receive window. If segment begins
        // before rcv_nxt, drop leading data (and SYN); if nothing left, just ack.
        //
        // RFC 1323 PAWS: If we have a timestamp reply on this segment and it's less than
        // opti.ts_recent, drop it.
        if opti.ts_present
            && st.tiflags & TH_RST == 0
            && tp.ts_recent.get() != 0
            && tstmp_lt(opti.ts_val, tp.ts_recent.get())
        {
            // Check to see if ts_recent is over 24 days old.
            if now.wrapping_sub(tp.ts_recent_age.get()) > TCP_PAWS_IDLE as u64 {
                // Invalidate ts_recent. If this segment updates ts_recent, the age will be
                // reset later and ts_recent will get a valid value. If it does not, setting
                // ts_recent to zero will at least satisfy the requirement that zero be placed
                // in the timestamp echo reply when ts_recent isn't valid. The age isn't reset
                // until we get a valid ts_recent because we don't want out-of-order segments
                // to be dropped when ts_recent is old.
                tp.ts_recent.set(0);
            } else {
                tcpstat_pkt(
                    TcpstatCounters::TcpsRcvduppack,
                    TcpstatCounters::TcpsRcvdupbyte,
                    st.tlen as u64,
                );
                tcpstat_inc(TcpstatCounters::TcpsPawsdrop);
                if st.tlen != 0 {
                    return DropAfterAck;
                }
                return Drop;
            }
        }

        let mut todrop = tp.rcv_nxt.get().wrapping_sub(st.th.th_seq) as i32;
        if todrop > 0 {
            if st.tiflags & TH_SYN != 0 {
                st.tiflags &= !TH_SYN;
                st.th.th_seq = st.th.th_seq.wrapping_add(1);
                if st.th.th_urp > 1 {
                    st.th.th_urp -= 1;
                } else {
                    st.tiflags &= !TH_URG;
                }
                todrop -= 1;
            }
            if todrop > st.tlen || (todrop == st.tlen && st.tiflags & TH_FIN == 0) {
                // Any valid FIN must be to the left of the window. At this point, FIN must be
                // a duplicate or out-of-sequence, so drop it.
                st.tiflags &= !TH_FIN;
                // Send ACK to resynchronize, and drop any data, but keep on processing for
                // RST or ACK.
                tp.set_flags(TF_ACKNOW);
                todrop = st.tlen;
                tcpstat_pkt(
                    TcpstatCounters::TcpsRcvduppack,
                    TcpstatCounters::TcpsRcvdupbyte,
                    todrop as u64,
                );
            } else {
                tcpstat_pkt(
                    TcpstatCounters::TcpsRcvpartduppack,
                    TcpstatCounters::TcpsRcvpartdupbyte,
                    todrop as u64,
                );
            }
            hdroptlen += todrop; // drop from head afterwards
            st.th.th_seq = st.th.th_seq.wrapping_add(todrop as u32);
            st.tlen -= todrop;
            if i32::from(st.th.th_urp) > todrop {
                st.th.th_urp -= todrop as u16;
            } else {
                st.tiflags &= !TH_URG;
                st.th.th_urp = 0;
            }
        }

        // If new data are received on a connection after the user processes are gone, then
        // RST the other end.
        if so.has_state(SS_NOFDREF) && tp.t_state.get() > TCPS_CLOSE_WAIT && st.tlen != 0 {
            st.tp = tcp_close(tp);
            tcpstat_inc(TcpstatCounters::TcpsRcvafterclose);
            return DropWithReset;
        }

        // If segment ends after window, drop trailing data (and PUSH and FIN); if nothing
        // left, just ACK.
        let todrop = st
            .th
            .th_seq
            .wrapping_add(st.tlen as u32)
            .wrapping_sub(tp.rcv_nxt.get().wrapping_add(tp.rcv_wnd.get() as u32))
            as i32;
        if todrop > 0 {
            tcpstat_inc(TcpstatCounters::TcpsRcvpackafterwin);
            if todrop >= st.tlen {
                tcpstat_add(TcpstatCounters::TcpsRcvbyteafterwin, st.tlen as u64);
                // If window is closed can only take segments at window edge, and have to
                // drop data and PUSH from incoming segments. Continue processing, but
                // remember to ack. Otherwise, drop segment and ack.
                if tp.rcv_wnd.get() == 0 && st.th.th_seq == tp.rcv_nxt.get() {
                    tp.set_flags(TF_ACKNOW);
                    tcpstat_inc(TcpstatCounters::TcpsRcvwinprobe);
                } else {
                    return DropAfterAck;
                }
            } else {
                tcpstat_add(TcpstatCounters::TcpsRcvbyteafterwin, todrop as u64);
            }
            m_adj(m, -todrop);
            st.tlen -= todrop;
            st.tiflags &= !(TH_PUSH | TH_FIN);
        }

        // If last ACK falls within this segment's sequence numbers, record its timestamp if
        // it's more recent. NOTE that the test is modified according to the latest proposal
        // of the tcplw@cray.com list (Braden 1993/04/26).
        if opti.ts_present
            && tstmp_geq(opti.ts_val, tp.ts_recent.get())
            && seq_leq(st.th.th_seq, tp.last_ack_sent.get())
        {
            tp.ts_recent_age.set(now);
            tp.ts_recent.set(opti.ts_val);
        }

        // If the RST bit is set examine the state:
        //    SYN_RECEIVED STATE:
        //     If passive open, return to LISTEN state.
        //     If active open, inform user that connection was refused.
        //    ESTABLISHED, FIN_WAIT_1, FIN_WAIT2, CLOSE_WAIT STATES:
        //     Inform user that connection was reset, and close tcb.
        //    CLOSING, LAST_ACK, TIME_WAIT STATES
        //     Close the tcb.
        if st.tiflags & TH_RST != 0 {
            if st.th.th_seq != tp.last_ack_sent.get()
                && st.th.th_seq != tp.rcv_nxt.get()
                && st.th.th_seq != tp.rcv_nxt.get().wrapping_add(1)
            {
                return Drop;
            }

            let close = match tp.t_state.get() {
                TCPS_SYN_RECEIVED => {
                    // if ECN is enabled, fall back to non-ecn at rexmit
                    if do_ecn && !tp.has_flags(TF_DISABLE_ECN) {
                        return Drop;
                    }
                    so.set_error(Some(Errno::ECONNREFUSED));
                    true
                }

                TCPS_ESTABLISHED | TCPS_FIN_WAIT_1 | TCPS_FIN_WAIT_2 | TCPS_CLOSE_WAIT => {
                    so.set_error(Some(Errno::ECONNRESET));
                    true
                }
                TCPS_CLOSING | TCPS_LAST_ACK | TCPS_TIME_WAIT => {
                    st.tp = tcp_close(tp);
                    return Drop;
                }
                _ => false,
            };
            if close {
                // close:
                tp.t_state.set(TCPS_CLOSED);
                tcpstat_inc(TcpstatCounters::TcpsDrops);
                st.tp = tcp_close(tp);
                return Drop;
            }
        }

        // If a SYN is in the window, then this is an error and we ACK and drop the packet.
        if st.tiflags & TH_SYN != 0 {
            return DropAfterAckRatelim;
        }

        // If the ACK bit is off we drop the segment and return.
        if st.tiflags & TH_ACK == 0 {
            if tp.has_flags(TF_ACKNOW) {
                return DropAfterAck;
            } else {
                return Drop;
            }
        }

        // Ack processing.
        'ack: {
            let state = tp.t_state.get();
            if !matches!(
                state,
                TCPS_SYN_RECEIVED
                    | TCPS_ESTABLISHED
                    | TCPS_FIN_WAIT_1
                    | TCPS_FIN_WAIT_2
                    | TCPS_CLOSE_WAIT
                    | TCPS_CLOSING
                    | TCPS_LAST_ACK
                    | TCPS_TIME_WAIT
            ) {
                break 'ack;
            }

            // In SYN_RECEIVED state, the ack ACKs our SYN, so enter ESTABLISHED state and
            // continue processing. The ACK was checked above.
            if state == TCPS_SYN_RECEIVED {
                tcpstat_inc(TcpstatCounters::TcpsConnects);
                soisconnected(so);
                tp.t_state.set(TCPS_ESTABLISHED);
                tcp_timer_arm(tp, TCPT_KEEP, TCP_KEEPIDLE.load(Ordering::Relaxed) as u64);
                // Do window scaling?
                if tp.t_flags.get() & (TF_RCVD_SCALE | TF_REQ_SCALE)
                    == (TF_RCVD_SCALE | TF_REQ_SCALE)
                {
                    tp.snd_scale.set(tp.requested_s_scale.get());
                    tp.rcv_scale.set(tp.request_r_scale.get());
                    tiwin = u64::from(st.th.th_win) << tp.snd_scale.get();
                }
                tcp_flush_queue(tp);
                tp.snd_wl1.set(st.th.th_seq.wrapping_sub(1));
                // fall into ...
            }

            // In ESTABLISHED state: drop duplicate ACKs; ACK out of range ACKs. If the ack is
            // in the range tp->snd_una < th->th_ack <= tp->snd_max then advance tp->snd_una
            // to th->th_ack and drop data from the retransmission queue. If this ACK reflects
            // more up to date window information we update our window information.

            // if we receive ECE and are not already in recovery phase, reduce cwnd by half
            // but don't slow-start. advance snd_last to snd_max not to reduce cwnd again
            // until all outstanding packets are acked.
            if do_ecn && st.tiflags & TH_ECE != 0 {
                if tp.has_flags(TF_ECN_PERMIT) && seq_geq(tp.snd_una.get(), tp.snd_last.get()) {
                    let win = min(tp.snd_wnd.get() as u32, tp.snd_cwnd.get() as u32)
                        / u32::from(tp.t_maxseg.get());
                    if win > 1 {
                        tp.snd_ssthresh
                            .set(u64::from(win / 2 * u32::from(tp.t_maxseg.get())));
                        tp.snd_cwnd.set(tp.snd_ssthresh.get());
                        tp.snd_last.set(tp.snd_max.get());
                        tp.set_flags(TF_SEND_CWR);
                        tcpstat_inc(TcpstatCounters::TcpsCwrEcn);
                    }
                }
                tcpstat_inc(TcpstatCounters::TcpsEcnRcvece);
            }
            // if we receive CWR, we know that the peer has reduced its congestion window.
            // stop sending ecn-echo.
            if st.tiflags & TH_CWR != 0 {
                tp.clear_flags(TF_RCVD_CE);
                tcpstat_inc(TcpstatCounters::TcpsEcnRcvcwr);
            }

            if seq_leq(st.th.th_ack, tp.snd_una.get()) {
                // Duplicate/old ACK processing.
                // Increments t_dupacks:
                //     Pure duplicate (same seq/ack/window, no data)
                // Doesn't affect t_dupacks:
                //     Data packets.
                //     Normal window updates (window opens)
                // Resets t_dupacks:
                //     New data ACKed.
                //     Window shrinks
                //     Old ACK
                if st.tlen != 0 {
                    // Drop very old ACKs unless th_seq matches
                    if st.th.th_seq != tp.rcv_nxt.get()
                        && seq_lt(
                            st.th.th_ack,
                            tp.snd_una.get().wrapping_sub(tp.max_sndwnd.get() as u32),
                        )
                    {
                        tcpstat_inc(TcpstatCounters::TcpsRcvacktooold);
                        return Drop;
                    }
                    break 'ack;
                }
                // If we get an old ACK, there is probably packet reordering going on. Be
                // conservative and reset t_dupacks so that we are less aggressive in doing a
                // fast retransmit.
                if st.th.th_ack != tp.snd_una.get() {
                    tp.t_dupacks.set(0);
                    break 'ack;
                }
                if tiwin == tp.snd_wnd.get() {
                    tcpstat_inc(TcpstatCounters::TcpsRcvdupack);
                    // If we have outstanding data (other than a window probe), this is a
                    // completely duplicate ack (ie, window info didn't change), the ack is
                    // the biggest we've seen and we've seen exactly our rexmt threshold of
                    // them, assume a packet has been dropped and retransmit it. Kludge
                    // snd_nxt & the congestion window so we send only this one packet.
                    //
                    // We know we're losing at the current window size so do congestion
                    // avoidance (set ssthresh to half the current window and pull our
                    // congestion window back to the new ssthresh).
                    //
                    // Dup acks mean that packets have left the network (they're now cached at
                    // the receiver) so bump cwnd by the amount in the receiver to keep a
                    // constant cwnd packets in the network.
                    if !tcp_timer_isarmed(tp, TCPT_REXMT) {
                        tp.t_dupacks.set(0);
                    } else {
                        tp.t_dupacks.set(tp.t_dupacks.get().wrapping_add(1));
                        if i32::from(tp.t_dupacks.get()) == TCPREXMTTHRESH {
                            let onxt = tp.snd_nxt.get();
                            let maxseg = u64::from(tp.t_maxseg.get());
                            let mut win = min(tp.snd_wnd.get(), tp.snd_cwnd.get()) / 2 / maxseg;

                            if seq_lt(st.th.th_ack, tp.snd_last.get()) {
                                // False fast retx after timeout. Do not cut window.
                                tp.t_dupacks.set(0);
                                return Drop;
                            }
                            if win < 2 {
                                win = 2;
                            }
                            tp.snd_ssthresh.set(win * maxseg);
                            tp.snd_last.set(tp.snd_max.get());
                            let dupacks = i32::from(tp.t_dupacks.get());
                            if tp.sack_enable.get() != 0 {
                                tcp_timer_disarm(tp, TCPT_REXMT);
                                tp.t_rtttime.set(0);
                                tp.set_flags(TF_SEND_CWR);
                                tcpstat_inc(TcpstatCounters::TcpsCwrFrecovery);
                                tcpstat_inc(TcpstatCounters::TcpsSackRecoveryEpisode);
                                // tcp_output() will send oldest SACK-eligible rtx.
                                let _ = tcp_output(tp);
                                tp.snd_cwnd.set(
                                    tp.snd_ssthresh.get()
                                        + (i32::from(tp.t_maxseg.get()) * dupacks) as u64,
                                );
                                return Drop;
                            }
                            tcp_timer_disarm(tp, TCPT_REXMT);
                            tp.t_rtttime.set(0);
                            tp.snd_nxt.set(st.th.th_ack);
                            tp.snd_cwnd.set(maxseg);
                            tp.set_flags(TF_SEND_CWR);
                            tcpstat_inc(TcpstatCounters::TcpsCwrFrecovery);
                            tcpstat_inc(TcpstatCounters::TcpsSndrexmitfast);
                            let _ = tcp_output(tp);

                            tp.snd_cwnd.set(
                                tp.snd_ssthresh.get()
                                    + (i32::from(tp.t_maxseg.get()) * dupacks) as u64,
                            );
                            if seq_gt(onxt, tp.snd_nxt.get()) {
                                tp.snd_nxt.set(onxt);
                            }
                            return Drop;
                        } else if i32::from(tp.t_dupacks.get()) > TCPREXMTTHRESH {
                            tp.snd_cwnd
                                .set(tp.snd_cwnd.get() + u64::from(tp.t_maxseg.get()));
                            let _ = tcp_output(tp);
                            return Drop;
                        }
                    }
                } else if tiwin < tp.snd_wnd.get() {
                    // The window was retracted! Previous dup ACKs may have been due to
                    // packets arriving after the shrunken window, not a missing packet, so
                    // play it safe and reset t_dupacks
                    tp.t_dupacks.set(0);
                }
                break 'ack;
            }
            // If the congestion window was inflated to account for the other side's cached
            // packets, retract it.
            if i32::from(tp.t_dupacks.get()) >= TCPREXMTTHRESH {
                // Check for a partial ACK
                if seq_lt(st.th.th_ack, tp.snd_last.get()) {
                    if tp.sack_enable.get() != 0 {
                        tcp_sack_partialack(tp, &st.th);
                    } else {
                        tcp_newreno_partialack(tp, &st.th);
                    }
                } else {
                    // Out of fast recovery
                    tp.snd_cwnd.set(tp.snd_ssthresh.get());
                    let outstanding =
                        tcp_seq_subtract(u64::from(tp.snd_max.get()), u64::from(st.th.th_ack));
                    if outstanding < tp.snd_ssthresh.get() {
                        tp.snd_cwnd.set(outstanding);
                    }
                    tp.t_dupacks.set(0);
                }
            } else {
                // Reset the duplicate ACK counter if we were not in fast recovery.
                tp.t_dupacks.set(0);
            }
            if seq_gt(st.th.th_ack, tp.snd_max.get()) {
                tcpstat_inc(TcpstatCounters::TcpsRcvacktoomuch);
                return DropAfterAckRatelim;
            }
            let acked = st.th.th_ack.wrapping_sub(tp.snd_una.get()) as i32;
            tcpstat_pkt(
                TcpstatCounters::TcpsRcvackpack,
                TcpstatCounters::TcpsRcvackbyte,
                acked as u64,
            );
            tp.t_rcvacktime.set(now);

            // If we have a timestamp reply, update smoothed round trip time. If no timestamp
            // is present but transmit timer is running and timed sequence number was acked,
            // update smoothed round trip time. Since we now have an rtt measurement, cancel
            // the timer backoff (cf., Phil Karn's retransmit alg.). Recompute the initial
            // retransmit timer.
            if opti.ts_present && opti.ts_ecr != 0 {
                tcp_xmit_timer(tp, now.wrapping_sub(u64::from(opti.ts_ecr)) as i32);
            } else if tp.t_rtttime.get() != 0 && seq_gt(st.th.th_ack, tp.t_rtseq.get()) {
                tcp_xmit_timer(tp, now.wrapping_sub(tp.t_rtttime.get()) as i32);
            }

            // If all outstanding data is acked, stop retransmit timer and remember to
            // restart (more output or persist). If there is more data to be acked, restart
            // retransmit timer, using current (possibly backed-off) value.
            if st.th.th_ack == tp.snd_max.get() {
                tcp_timer_disarm(tp, TCPT_REXMT);
                tp.set_flags(TF_NEEDOUTPUT);
            } else if !tcp_timer_isarmed(tp, TCPT_PERSIST) {
                tcp_timer_arm(tp, TCPT_REXMT, tp.t_rxtcur.get() as u64);
            }
            // When new data is acked, open the congestion window. If the window gives us less
            // than ssthresh packets in flight, open exponentially (maxseg per packet).
            // Otherwise open linearly: maxseg per window (maxseg^2 / cwnd per packet).
            {
                let cw = tp.snd_cwnd.get() as u32;
                let mut incr = u32::from(tp.t_maxseg.get());

                if u64::from(cw) > tp.snd_ssthresh.get() {
                    incr = max(incr.wrapping_mul(incr) / cw, 1);
                }
                if i32::from(tp.t_dupacks.get()) < TCPREXMTTHRESH {
                    tp.snd_cwnd.set(min(
                        u64::from(cw.wrapping_add(incr)),
                        u64::from(TCP_MAXWIN << tp.snd_scale.get()),
                    ));
                }
            }
            let sb_cc = so.so_snd.sb_cc.get();
            let ourfinisacked = if acked as u64 > sb_cc {
                if tp.snd_wnd.get() > sb_cc {
                    tp.snd_wnd.set(tp.snd_wnd.get() - sb_cc);
                } else {
                    tp.snd_wnd.set(0);
                }
                mtx_enter(&so.so_snd.sb_mtx);
                sbdrop(&so.so_snd, so.so_snd.sb_cc.get() as i32);
                mtx_leave(&so.so_snd.sb_mtx);
                true
            } else {
                mtx_enter(&so.so_snd.sb_mtx);
                sbdrop(&so.so_snd, acked);
                mtx_leave(&so.so_snd.sb_mtx);
                if tp.snd_wnd.get() > acked as u64 {
                    tp.snd_wnd.set(tp.snd_wnd.get() - acked as u64);
                } else {
                    tp.snd_wnd.set(0);
                }
                false
            };

            tcp_update_sndspace(tp);
            if sb_notify(&so.so_snd) {
                sowwakeup(so);
            }

            // If we had a pending ICMP message that referred to data that have just been
            // acknowledged, disregard the recorded ICMP message.
            if tp.has_flags(TF_PMTUD_PEND) && seq_gt(st.th.th_ack, tp.t_pmtud_th_seq.get()) {
                tp.clear_flags(TF_PMTUD_PEND);
            }

            // Keep track of the largest chunk of data acknowledged since last PMTU update
            if tp.t_pmtud_mss_acked.get() < acked as u32 {
                tp.t_pmtud_mss_acked.set(acked as u32);
            }

            tp.snd_una.set(st.th.th_ack);
            // sync snd_last with snd_una
            if seq_gt(tp.snd_una.get(), tp.snd_last.get()) {
                tp.snd_last.set(tp.snd_una.get());
            }
            if seq_lt(tp.snd_nxt.get(), tp.snd_una.get()) {
                tp.snd_nxt.set(tp.snd_una.get());
            }

            match tp.t_state.get() {
                // In FIN_WAIT_1 STATE in addition to the processing for the ESTABLISHED state
                // if our FIN is now acknowledged then enter FIN_WAIT_2.
                TCPS_FIN_WAIT_1 => {
                    if ourfinisacked {
                        // If we can't receive any more data, then closing user can proceed.
                        // Starting the timer is contrary to the specification, but if we
                        // don't get a FIN we'll hang forever.
                        if so.so_rcv.has_state(SS_CANTRCVMORE) {
                            soisdisconnected(so);
                            let maxidle = TCPTV_KEEPCNT * TCP_KEEPIDLE.load(Ordering::Relaxed);
                            tcp_timer_arm(tp, TCPT_2MSL, maxidle as u64);
                        }
                        tp.t_state.set(TCPS_FIN_WAIT_2);
                    }
                }

                // In CLOSING STATE in addition to the processing for the ESTABLISHED state if
                // the ACK acknowledges our FIN then enter the TIME-WAIT state, otherwise
                // ignore the segment.
                TCPS_CLOSING => {
                    if ourfinisacked {
                        tp.t_state.set(TCPS_TIME_WAIT);
                        tcp_canceltimers(tp);
                        tcp_timer_arm(tp, TCPT_2MSL, (2 * TCPTV_MSL) as u64);
                        soisdisconnected(so);
                    }
                }

                // In LAST_ACK, we may still be waiting for data to drain and/or to be acked,
                // as well as for the ack of our FIN. If our FIN is now acknowledged, delete
                // the TCB, enter the closed state and return.
                TCPS_LAST_ACK => {
                    if ourfinisacked {
                        st.tp = tcp_close(tp);
                        return Drop;
                    }
                }

                // In TIME_WAIT state the only thing that should arrive is a retransmission of
                // the remote FIN. Acknowledge it and restart the finack timer.
                TCPS_TIME_WAIT => {
                    tcp_timer_arm(tp, TCPT_2MSL, (2 * TCPTV_MSL) as u64);
                    return DropAfterAck;
                }
                _ => {}
            }
        }
    }

    // step6:
    // Update window information. Don't look at window if no ACK: TAC's send garbage on first
    // SYN.
    if st.tiflags & TH_ACK != 0
        && (seq_lt(tp.snd_wl1.get(), st.th.th_seq)
            || (tp.snd_wl1.get() == st.th.th_seq
                && (seq_lt(tp.snd_wl2.get(), st.th.th_ack)
                    || (tp.snd_wl2.get() == st.th.th_ack && tiwin > tp.snd_wnd.get()))))
    {
        // keep track of pure window updates
        if st.tlen == 0 && tp.snd_wl2.get() == st.th.th_ack && tiwin > tp.snd_wnd.get() {
            tcpstat_inc(TcpstatCounters::TcpsRcvwinupd);
        }
        tp.snd_wnd.set(tiwin);
        tp.snd_wl1.set(st.th.th_seq);
        tp.snd_wl2.set(st.th.th_ack);
        if tp.snd_wnd.get() > tp.max_sndwnd.get() {
            tp.max_sndwnd.set(tp.snd_wnd.get());
        }
        tp.set_flags(TF_NEEDOUTPUT);
    }

    // Process segments with URG.
    'dodata: {
        if st.tiflags & TH_URG != 0 && st.th.th_urp != 0 && !tcps_havercvdfin(tp.t_state.get()) {
            // This is a kludge, but if we receive and accept random urgent pointers, we'll
            // crash in soreceive. It's hard to imagine someone actually wanting to send this
            // much urgent data.
            mtx_enter(&so.so_rcv.sb_mtx);
            let urgent = u64::from(st.th.th_urp) + so.so_rcv.sb_cc.get();
            mtx_leave(&so.so_rcv.sb_mtx);

            if urgent > SB_MAX_VAR.load(Ordering::Relaxed) {
                st.th.th_urp = 0; // XXX
                st.tiflags &= !TH_URG; // XXX
                break 'dodata; // XXX
            }
            // If this segment advances the known urgent pointer, then mark the data stream.
            // This should not happen in CLOSE_WAIT, CLOSING, LAST_ACK or TIME_WAIT STATES
            // since a FIN has been received from the remote side. In these states we ignore
            // the URG.
            //
            // According to RFC961 (Assigned Protocols), the urgent pointer points to the last
            // octet of urgent data. We continue, however, to consider it to indicate the
            // first octet of data past the urgent section as the original spec states (in one
            // of two places).
            let urp_seq = st.th.th_seq.wrapping_add(u32::from(st.th.th_urp));
            if seq_gt(urp_seq, tp.rcv_up.get()) {
                tp.rcv_up.set(urp_seq);
                mtx_enter(&so.so_rcv.sb_mtx);
                so.so_oobmark.set(
                    so.so_rcv
                        .sb_cc
                        .get()
                        .wrapping_add(u64::from(tp.rcv_up.get().wrapping_sub(tp.rcv_nxt.get())))
                        .wrapping_sub(1),
                );
                if so.so_oobmark.get() == 0 {
                    so.so_rcv.set_state(SS_RCVATMARK);
                }
                mtx_leave(&so.so_rcv.sb_mtx);
                sohasoutofband(so);
                tp.t_oobflags
                    .set(tp.t_oobflags.get() & !(TCPOOB_HAVEDATA | TCPOOB_HADDATA));
            }
            // Remove out of band data so doesn't get presented to user. This can happen
            // independent of advancing the URG pointer, but if two URG's are pending at once,
            // some out-of-band data may creep in... ick.
            if st.th.th_urp <= st.tlen as u16 && !so.has_options(SO_OOBINLINE) {
                tcp_pulloutofband(so, u32::from(st.th.th_urp), m, hdroptlen);
            }
        } else if seq_gt(tp.rcv_nxt.get(), tp.rcv_up.get()) {
            // If no out of band data is expected, pull receive urgent pointer along with the
            // receive window.
            tp.rcv_up.set(tp.rcv_nxt.get());
        }
    }
    // dodata: XXX

    // Process the segment text, merging it into the TCP sequencing queue, and arranging for
    // acknowledgment of receipt if necessary. This process logically involves adjusting
    // tp->rcv_wnd as data is presented to the user (this happens in tcp_usrreq.c, case
    // PRU_RCVD). If a FIN has already been received on this connection then we just ignore
    // the text.
    if (st.tlen != 0 || st.tiflags & TH_FIN != 0) && !tcps_havercvdfin(tp.t_state.get()) {
        let laststart = st.th.th_seq;
        let lastend = st.th.th_seq.wrapping_add(st.tlen as u32);
        let mut reass_failed = false;

        if st.th.th_seq == tp.rcv_nxt.get()
            && tp.t_segq.is_empty()
            && tp.t_state.get() == TCPS_ESTABLISHED
        {
            tcp_setup_ack(tp, st.tiflags, Some(m));
            tp.rcv_nxt
                .set(tp.rcv_nxt.get().wrapping_add(st.tlen as u32));
            st.tiflags = st.th.th_flags & TH_FIN;
            tcpstat_pkt(
                TcpstatCounters::TcpsRcvpack,
                TcpstatCounters::TcpsRcvbyte,
                st.tlen as u64,
            );
            if so.so_rcv.has_state(SS_CANTRCVMORE) {
                m_freem(m);
            } else {
                m_adj(m, hdroptlen);
                mtx_enter(&so.so_rcv.sb_mtx);
                sbappendstream(&so.so_rcv, m);
                mtx_leave(&so.so_rcv.sb_mtx);
            }
            sorwakeup(so);
        } else {
            m_adj(m, hdroptlen);
            match tcp_reass(tp, &mut st.th, m, &mut st.tlen) {
                Ok(flags) => st.tiflags = flags,
                Err(_) => {
                    st.tiflags = 0;
                    reass_failed = true;
                }
            }
            tp.set_flags(TF_ACKNOW);
        }
        st.m = None;
        if tp.sack_enable.get() != 0 && !reass_failed {
            tcp_update_sack_list(tp, laststart, lastend);
        }

        // variable len never referenced again in modern BSD, so why bother computing it ??
        // (the C keeps `len = so->so_rcv.sb_hiwat - (tp->rcv_adv - tp->rcv_nxt)`, the amount
        // of data the peer has sent into our window, under `#if 0`.)
    } else {
        m_freem(st.m.take());
        st.tiflags &= !TH_FIN;
    }

    // If FIN is received ACK the FIN and let the user know that the connection is closing.
    // Ignore a FIN received before the connection is fully established.
    if st.tiflags & TH_FIN != 0 && tcps_haveestablished(tp.t_state.get()) {
        if !tcps_havercvdfin(tp.t_state.get()) {
            socantrcvmore(so);
            tp.set_flags(TF_ACKNOW);
            tp.rcv_nxt.set(tp.rcv_nxt.get().wrapping_add(1));
        }
        match tp.t_state.get() {
            // In ESTABLISHED STATE enter the CLOSE_WAIT state.
            TCPS_ESTABLISHED => tp.t_state.set(TCPS_CLOSE_WAIT),

            // If still in FIN_WAIT_1 STATE FIN has not been acked so enter the CLOSING state.
            TCPS_FIN_WAIT_1 => tp.t_state.set(TCPS_CLOSING),

            // In FIN_WAIT_2 state enter the TIME_WAIT state, starting the time-wait timer,
            // turning off the other standard timers.
            TCPS_FIN_WAIT_2 => {
                tp.t_state.set(TCPS_TIME_WAIT);
                tcp_canceltimers(tp);
                tcp_timer_arm(tp, TCPT_2MSL, (2 * TCPTV_MSL) as u64);
                soisdisconnected(so);
            }

            // In TIME_WAIT state restart the 2 MSL time_wait timer.
            TCPS_TIME_WAIT => tcp_timer_arm(tp, TCPT_2MSL, (2 * TCPTV_MSL) as u64),
            _ => {}
        }
    }
    if !st.otp.is_null() {
        tcp_trace(
            TA_INPUT,
            st.ostate,
            Some(tp),
            st.otp,
            Some(&st.saveti),
            0,
            st.tlen,
        );
    }

    // Return any desired output.
    if tp.has_flags(TF_ACKNOW | TF_NEEDOUTPUT) {
        let _ = tcp_output(tp);
    }
    Done
}

/// `tcp_dooptions`: parses the TCP options `cp` of segment `th` for `tp` into `oi` (and the
/// window scale, timestamp and SACK permitted flags of `tp`), feeds SACK blocks to
/// `tcp_sack_option`, and checks the TCP MD5 signature. `false` (the C's -1) drops the
/// segment.
#[allow(clippy::too_many_arguments)] // the C's prototype
pub fn tcp_dooptions(
    tp: &Tcpcb,
    cp: &[u8],
    th: &Tcphdr,
    m: &Mbuf,
    iphlen: i32,
    oi: &mut TcpOptInfo,
    rtableid: u32,
    now: u64,
) -> bool {
    let mut sigp: Option<[u8; 16]> = None;
    let mut tdb: Option<&'static Tdb> = None;

    let ok = 'bad: {
        let mut cp = cp;
        while !cp.is_empty() {
            let opt = cp[0];
            if opt == TCPOPT_EOL {
                break;
            }
            let optlen = if opt == TCPOPT_NOP {
                1
            } else {
                if cp.len() < 2 {
                    break;
                }
                let optlen = usize::from(cp[1]);
                if optlen < 2 || optlen > cp.len() {
                    break;
                }
                optlen
            };
            let o = &cp[..optlen];
            let syn_ok = th.th_flags & TH_SYN != 0 && !tcps_havercvdsyn(tp.t_state.get());
            match opt {
                TCPOPT_MAXSEG => {
                    if optlen == usize::from(TCPOLEN_MAXSEG) && syn_ok {
                        oi.maxseg = u16::from_be_bytes([o[2], o[3]]);
                    }
                }

                TCPOPT_WINDOW => {
                    if optlen == usize::from(TCPOLEN_WINDOW) && syn_ok {
                        tp.set_flags(TF_RCVD_SCALE);
                        tp.requested_s_scale.set(min(o[2], TCP_MAX_WINSHIFT));
                    }
                }

                TCPOPT_TIMESTAMP => {
                    if optlen == usize::from(TCPOLEN_TIMESTAMP) {
                        oi.ts_present = true;
                        oi.ts_val = u32::from_be_bytes([o[2], o[3], o[4], o[5]]);
                        oi.ts_ecr = u32::from_be_bytes([o[6], o[7], o[8], o[9]]);

                        if syn_ok {
                            // A timestamp received in a SYN makes it ok to send timestamp
                            // requests and replies.
                            tp.set_flags(TF_RCVD_TSTMP);
                            tp.ts_recent.set(oi.ts_val);
                            tp.ts_recent_age.set(now);
                        }
                    }
                }

                TCPOPT_SACK_PERMITTED => {
                    if tp.sack_enable.get() != 0
                        && optlen == usize::from(TCPOLEN_SACK_PERMITTED)
                        && syn_ok
                    {
                        // MUST only be set on SYN
                        tp.set_flags(TF_SACK_PERMIT);
                    }
                }
                TCPOPT_SACK => tcp_sack_option(tp, th, o),
                TCPOPT_SIGNATURE if optlen == usize::from(TCPOLEN_SIGNATURE) => {
                    if let Some(s) = sigp
                        && timingsafe_bcmp(&s, &o[2..18])
                    {
                        break 'bad false;
                    }

                    let mut s = [0u8; 16];
                    s.copy_from_slice(&o[2..18]);
                    sigp = Some(s);
                }
                _ => {}
            }
            cp = &cp[optlen..];
        }

        if tp.has_flags(TF_SIGNATURE) {
            let mut src = SockaddrUnion::new();
            let mut dst = SockaddrUnion::new();

            let pf = tp.pf.get();
            if pf == 0 || pf == i32::from(AF_INET) {
                let ip = mtod_ip(m);
                let mut sin = SockaddrIn {
                    sin_len: size_of::<SockaddrIn>() as u8,
                    sin_family: AF_INET,
                    sin_addr: ip.ip_src,
                    ..SockaddrIn::default()
                };
                src.set_sin(&sin);
                sin.sin_addr = ip.ip_dst;
                dst.set_sin(&sin);
            }
            #[cfg(feature = "inet6")]
            if pf == AF_INET6_I32 {
                let ip6 = mtod_ip6(m);
                src = sa_from_sin6(&SockaddrIn6::with_addr(ip6.ip6_src));
                dst = sa_from_sin6(&SockaddrIn6::with_addr(ip6.ip6_dst));
            }

            tdb = gettdbbysrcdst(rtable_l2(rtableid), 0, &src, &dst, IPPROTO_TCP as u8);

            // We don't have an SA for this peer, so we turn off TF_SIGNATURE on the listen
            // socket
            if tdb.is_none() && tp.t_state.get() == TCPS_LISTEN {
                tp.clear_flags(TF_SIGNATURE);
            }
        }

        if sigp.is_some() != tp.has_flags(TF_SIGNATURE) {
            tcpstat_inc(TcpstatCounters::TcpsRcvbadsig);
            break 'bad false;
        }

        if let Some(sigp) = sigp {
            let Some(t) = tdb else {
                tcpstat_inc(TcpstatCounters::TcpsRcvbadsig);
                break 'bad false;
            };

            let mut sig = [0u8; 16];
            if !tcp_signature(t, tp.pf.get(), m, th, iphlen, true, &mut sig) {
                break 'bad false;
            }

            if timingsafe_bcmp(&sig, &sigp) {
                tcpstat_inc(TcpstatCounters::TcpsRcvbadsig);
                break 'bad false;
            }

            tcpstat_inc(TcpstatCounters::TcpsRcvgoodsig);
        }
        true
    };

    tdb_unref(tdb);
    ok
}

/// `tcp_seq_subtract`: `a - b` of two sequence numbers widened to `u_long`, wrapping (the
/// C's `(long)(a - b)` returned as `u_long`).
pub fn tcp_seq_subtract(a: u64, b: u64) -> u64 {
    a.wrapping_sub(b)
}

/// `tcp_update_sack_list`: called upon receipt of new valid data (while not in header
/// prediction mode), it updates the ordered list of sacks.
pub fn tcp_update_sack_list(tp: &Tcpcb, rcv_laststart: TcpSeq, rcv_lastend: TcpSeq) {
    // First reported block MUST be the most recent one. Subsequent blocks SHOULD be in the
    // order in which they arrived at the receiver. These two conditions make the
    // implementation fully compliant with RFC 2018.
    let mut j = 0usize;
    let mut count = 0;
    let mut lastpos: i32 = -1;
    let mut temp = [Sackblk::default(); MAX_SACK_BLKS];
    let zero = Sackblk { start: 0, end: 0 };

    // First clean up current list of sacks
    for i in 0..tp.rcv_numsacks.get() as usize {
        let sack = tp.sackblks[i].get();
        if sack.start == 0 && sack.end == 0 {
            count += 1; // count = number of blocks to be discarded
            continue;
        }
        if seq_leq(sack.end, tp.rcv_nxt.get()) {
            tp.sackblks[i].set(zero);
            count += 1;
        } else {
            temp[j] = sack;
            j += 1;
        }
    }
    tp.rcv_numsacks.set(tp.rcv_numsacks.get() - count);
    if tp.rcv_numsacks.get() == 0 {
        // no sack blocks currently (fast path)
        tcp_clean_sackreport(tp);
        if seq_lt(tp.rcv_nxt.get(), rcv_laststart) {
            // ==> need first sack block
            tp.sackblks[0].set(Sackblk {
                start: rcv_laststart,
                end: rcv_lastend,
            });
            tp.rcv_numsacks.set(1);
        }
        return;
    }
    // Otherwise, sack blocks are already present.
    let numsacks = tp.rcv_numsacks.get() as usize;
    for (blk, sack) in tp.sackblks.iter().zip(&temp).take(numsacks) {
        blk.set(*sack); // first copy back sack list
    }
    if seq_geq(tp.rcv_nxt.get(), rcv_lastend) {
        return; // sack list remains unchanged
    }
    // From here, segment just received should be (part of) the 1st sack. Go through list,
    // possibly coalescing sack block entries.
    let mut firstsack = Sackblk {
        start: rcv_laststart,
        end: rcv_lastend,
    };
    for i in 0..numsacks {
        let sack = tp.sackblks[i].get();
        if seq_lt(sack.end, firstsack.start) || seq_gt(sack.start, firstsack.end) {
            continue; // no overlap
        }
        if sack.start == firstsack.start && sack.end == firstsack.end {
            // identical block; delete it here since we will move it to the front of the
            // list.
            tp.sackblks[i].set(zero);
            lastpos = i as i32; // last posn with a zero entry
            continue;
        }
        if seq_leq(sack.start, firstsack.start) {
            firstsack.start = sack.start; // merge blocks
        }
        if seq_geq(sack.end, firstsack.end) {
            firstsack.end = sack.end; // merge blocks
        }
        tp.sackblks[i].set(zero);
        lastpos = i as i32; // last posn with a zero entry
    }
    if lastpos != -1 {
        // at least one merge
        let mut j = 1usize;
        for i in 0..numsacks {
            let sack = tp.sackblks[i].get();
            if sack.start == 0 && sack.end == 0 {
                continue;
            }
            temp[j] = sack;
            j += 1;
        }
        tp.rcv_numsacks.set(j as i32); // including first blk (added later)
        for (blk, sack) in tp.sackblks.iter().zip(&temp).take(j).skip(1) {
            blk.set(*sack); // now copy back
        }
    } else {
        // no merges -- shift sacks by 1
        if (tp.rcv_numsacks.get() as usize) < MAX_SACK_BLKS {
            tp.rcv_numsacks.set(tp.rcv_numsacks.get() + 1);
        }
        for i in (1..tp.rcv_numsacks.get() as usize).rev() {
            tp.sackblks[i].set(tp.sackblks[i - 1].get());
        }
    }
    tp.sackblks[0].set(firstsack);
}

/// `tcp_sack_option`: process the TCP SACK option `cp` (kind and length included).
/// `tp->snd_holes` is an ordered list of holes (oldest to newest, in terms of the sequence
/// space).
pub fn tcp_sack_option(tp: &Tcpcb, th: &Tcphdr, cp: &[u8]) {
    let optlen = cp.len();

    if tp.sack_enable.get() == 0 {
        return;
    }
    // SACK without ACK doesn't make sense.
    if th.th_flags & TH_ACK == 0 {
        return;
    }
    // Make sure the ACK on this segment is in [snd_una, snd_max].
    if seq_lt(th.th_ack, tp.snd_una.get()) || seq_gt(th.th_ack, tp.snd_max.get()) {
        return;
    }
    // Note: TCPOLEN_SACK must be 2*sizeof(tcp_seq)
    let sacklen = usize::from(TCPOLEN_SACK);
    if optlen <= 2 || !(optlen - 2).is_multiple_of(sacklen) {
        return;
    }
    // Note: TCPOLEN_SACK must be 2*sizeof(tcp_seq)
    let mut tmp_cp = &cp[2..];
    tcpstat_inc(TcpstatCounters::TcpsSackRcvOpts);
    if tp.snd_numholes.get() < 0 {
        tp.snd_numholes.set(0);
    }
    if tp.t_maxseg.get() == 0 {
        panic(format_args!("tcp_sack_option")); // Should never happen
    }
    let maxseg = u32::from(tp.t_maxseg.get());
    let thresh = TCPREXMTTHRESH as u32;
    'dropped: {
        while !tmp_cp.is_empty() {
            let sack = Sackblk {
                start: u32::from_be_bytes([tmp_cp[0], tmp_cp[1], tmp_cp[2], tmp_cp[3]]),
                end: u32::from_be_bytes([tmp_cp[4], tmp_cp[5], tmp_cp[6], tmp_cp[7]]),
            };
            tmp_cp = &tmp_cp[sacklen..];
            if seq_leq(sack.end, sack.start) {
                continue; // bad SACK fields
            }
            if seq_leq(sack.end, tp.snd_una.get()) {
                continue; // old block
            }
            if seq_gt(th.th_ack, tp.snd_una.get()) && seq_lt(sack.start, th.th_ack) {
                continue;
            }
            if seq_gt(sack.end, tp.snd_max.get()) {
                continue;
            }
            if seq_lt(sack.start, tp.snd_una.get()) {
                continue;
            }
            let Some(first) = tp.snd_holes.get() else {
                // first hole
                let Some(cur) = sackhole_get() else {
                    // ENOBUFS, so ignore SACKed block for now
                    break 'dropped;
                };
                tp.snd_holes.set(Some(cur));
                cur.start.set(th.th_ack);
                cur.end.set(sack.start);
                cur.rxmit.set(cur.start.get());
                cur.next.set(None);
                tp.snd_numholes.set(1);
                tp.rcv_lastsack.set(sack.end);
                // dups is at least one. If more data has been SACKed, it can be greater
                // than one.
                cur.dups
                    .set(min(thresh, sack.end.wrapping_sub(cur.end.get()) / maxseg) as i32);
                if cur.dups.get() < 1 {
                    cur.dups.set(1);
                }
                continue; // with next sack block
            };
            // Go thru list of holes: p = previous, cur = current
            let mut p: Option<&'static Sackhole> = Some(first);
            let mut cur: Option<&'static Sackhole> = Some(first);
            while let Some(c) = cur {
                if seq_leq(sack.end, c.start.get()) {
                    // SACKs data before the current hole
                    break; // no use going through more holes
                }
                if seq_geq(sack.start, c.end.get()) {
                    // SACKs data beyond the current hole
                    c.dups.set(c.dups.get() + 1);
                    if sack.end.wrapping_sub(c.end.get()) / maxseg >= thresh {
                        c.dups.set(TCPREXMTTHRESH);
                    }
                    p = cur;
                    cur = c.next.get();
                    continue;
                }
                if seq_leq(sack.start, c.start.get()) {
                    // Data acks at least the beginning of hole
                    if seq_geq(sack.end, c.end.get()) {
                        // Acks entire hole, so delete hole
                        if !same_hole(p, cur) {
                            if let Some(pp) = p {
                                pp.next.set(c.next.get());
                            }
                            sackhole_put(c);
                            cur = p.and_then(|pp| pp.next.get());
                        } else {
                            cur = c.next.get();
                            sackhole_put(c);
                            p = cur;
                            tp.snd_holes.set(p);
                        }
                        tp.snd_numholes.set(tp.snd_numholes.get() - 1);
                        continue;
                    }
                    // otherwise, move start of hole forward
                    c.start.set(sack.end);
                    c.rxmit.set(seq_max(c.rxmit.get(), c.start.get()));
                    p = cur;
                    cur = c.next.get();
                    continue;
                }
                // move end of hole backward
                if seq_geq(sack.end, c.end.get()) {
                    c.end.set(sack.start);
                    c.rxmit.set(seq_min(c.rxmit.get(), c.end.get()));
                    c.dups.set(c.dups.get() + 1);
                    if sack.end.wrapping_sub(c.end.get()) / maxseg >= thresh {
                        c.dups.set(TCPREXMTTHRESH);
                    }
                    p = cur;
                    cur = c.next.get();
                    continue;
                }
                if seq_lt(c.start.get(), sack.start) && seq_gt(c.end.get(), sack.end) {
                    // ACKs some data in middle of a hole; need to split current hole
                    if tp.snd_numholes.get() >= TCP_SACKHOLE_LIMIT {
                        break 'dropped;
                    }
                    let Some(temp) = sackhole_get() else {
                        break 'dropped; // ENOBUFS
                    };
                    temp.next.set(c.next.get());
                    temp.start.set(sack.end);
                    temp.end.set(c.end.get());
                    temp.dups.set(c.dups.get());
                    temp.rxmit.set(seq_max(c.rxmit.get(), temp.start.get()));
                    c.end.set(sack.start);
                    c.rxmit.set(seq_min(c.rxmit.get(), c.end.get()));
                    c.dups.set(c.dups.get() + 1);
                    if sack.end.wrapping_sub(c.end.get()) / maxseg >= thresh {
                        c.dups.set(TCPREXMTTHRESH);
                    }
                    c.next.set(Some(temp));
                    p = Some(temp);
                    cur = temp.next.get();
                    tp.snd_numholes.set(tp.snd_numholes.get() + 1);
                }
            }
            // At this point, p points to the last hole on the list
            if let Some(last) = p
                && seq_lt(tp.rcv_lastsack.get(), sack.start)
            {
                // Need to append new hole at end. Last hole is p (and it's not NULL).
                if tp.snd_numholes.get() >= TCP_SACKHOLE_LIMIT {
                    break 'dropped;
                }
                let Some(temp) = sackhole_get() else {
                    break 'dropped; // ENOBUFS
                };
                temp.start.set(tp.rcv_lastsack.get());
                temp.end.set(sack.start);
                temp.dups
                    .set(min(thresh, sack.end.wrapping_sub(sack.start) / maxseg) as i32);
                if temp.dups.get() < 1 {
                    temp.dups.set(1);
                }
                temp.rxmit.set(temp.start.get());
                temp.next.set(None);
                last.next.set(Some(temp));
                tp.rcv_lastsack.set(sack.end);
                tp.snd_numholes.set(tp.snd_numholes.get() + 1);
            }
        }
        return;
    }
    // dropped:
    tcpstat_inc(TcpstatCounters::TcpsSackDropOpts);
}

/// `tcp_del_sackholes`: delete stale (i.e, cumulatively ack'd) holes. Hole is deleted only if
/// it is completely acked; otherwise, `tcp_sack_option()`, called from `tcp_dooptions()`,
/// will fix up the hole.
pub fn tcp_del_sackholes(tp: &Tcpcb, th: &Tcphdr) {
    if tp.sack_enable.get() != 0 && tp.t_state.get() != TCPS_LISTEN {
        // max because this could be an older ack just arrived
        let lastack = if seq_gt(th.th_ack, tp.snd_una.get()) {
            th.th_ack
        } else {
            tp.snd_una.get()
        };
        let mut cur = tp.snd_holes.get();
        while let Some(c) = cur {
            if seq_leq(c.end.get(), lastack) {
                cur = c.next.get();
                sackhole_put(c);
                tp.snd_numholes.set(tp.snd_numholes.get() - 1);
            } else if seq_lt(c.start.get(), lastack) {
                c.start.set(lastack);
                if seq_lt(c.rxmit.get(), c.start.get()) {
                    c.rxmit.set(c.start.get());
                }
                break;
            } else {
                break;
            }
        }
        tp.snd_holes.set(cur);
    }
}

/// `tcp_clean_sackreport`: delete all receiver-side SACK information.
pub fn tcp_clean_sackreport(tp: &Tcpcb) {
    tp.rcv_numsacks.set(0);
    for b in &tp.sackblks {
        b.set(Sackblk { start: 0, end: 0 });
    }
}

/// `tcp_sack_partialack`: partial ack handling within a sack recovery episode. When a
/// partial ack arrives, turn off retransmission timer, deflate the window, do not clear
/// `tp->t_dupacks`.
fn tcp_sack_partialack(tp: &Tcpcb, th: &Tcphdr) {
    // Turn off retx. timer (will start again next segment)
    tcp_timer_disarm(tp, TCPT_REXMT);
    tp.t_rtttime.set(0);
    // Partial window deflation. This statement relies on the fact that tp->snd_una has not
    // been updated yet.
    let acked = u64::from(th.th_ack.wrapping_sub(tp.snd_una.get()));
    let maxseg = u64::from(tp.t_maxseg.get());
    if tp.snd_cwnd.get() > acked {
        tp.snd_cwnd.set(tp.snd_cwnd.get() - acked);
        tp.snd_cwnd.set(tp.snd_cwnd.get() + maxseg);
    } else {
        tp.snd_cwnd.set(maxseg);
    }
    tp.snd_cwnd.set(tp.snd_cwnd.get() + maxseg);
    tp.set_flags(TF_NEEDOUTPUT);
}

/// `tcp_pulloutofband`: pull out of band byte out of a segment so it doesn't appear in the
/// user's data queue. It is still reflected in the segment length for sequencing purposes.
pub fn tcp_pulloutofband(so: &Socket, urgent: u32, m: &'static Mbuf, off: i32) {
    let mut cnt = off.wrapping_add(urgent as i32) - 1;
    let mut m = m;

    while cnt >= 0 {
        let len = m.m_len().get() as i32;
        if len > cnt {
            // SAFETY: `cnt < m_len`: the byte and the `m_len - cnt - 1` after it lie inside
            // the mbuf's data.
            unsafe {
                let cp = mtod::<u8>(m).add(cnt as usize);
                if let Some(tp) = sototcpcb(so) {
                    tp.t_iobc.set(*cp);
                    tp.t_oobflags.set(tp.t_oobflags.get() | TCPOOB_HAVEDATA);
                }
                ptr::copy(cp.add(1), cp, (len - cnt - 1) as usize);
            }
            m.m_len().set(m.m_len().get() - 1);
            return;
        }
        cnt -= len;
        match m.m_next().get() {
            Some(n) => m = n,
            None => break,
        }
    }
    panic(format_args!("tcp_pulloutofband"));
}

/// `tcp_xmit_timer`: collect new round-trip time estimate and update averages and current
/// timeout.
pub fn tcp_xmit_timer(tp: &Tcpcb, rtt: i32) {
    let rtt = rtt.clamp(0, TCP_RTT_MAX);

    tcpstat_inc(TcpstatCounters::TcpsRttupdated);
    if tp.t_srtt.get() != 0 {
        // delta is fixed point with 2 (TCP_RTT_BASE_SHIFT) bits after the binary point
        // (scaled by 4), whereas srtt is stored as fixed point with 5 bits after the binary
        // point (i.e., scaled by 32). The following magic is equivalent to the smoothing
        // algorithm in rfc793 with an alpha of .875 (srtt = rtt/8 + srtt*7/8 in fixed
        // point).
        let mut delta = (rtt << TCP_RTT_BASE_SHIFT) - (tp.t_srtt.get() >> TCP_RTT_SHIFT);
        tp.t_srtt.set(tp.t_srtt.get() + delta);
        if tp.t_srtt.get() <= 0 {
            tp.t_srtt.set(1 << TCP_RTT_BASE_SHIFT);
        }
        // We accumulate a smoothed rtt variance (actually, a smoothed mean difference), then
        // set the retransmit timer to smoothed rtt + 4 times the smoothed variance. rttvar is
        // stored as fixed point with 4 bits after the binary point (scaled by 16). The
        // following is equivalent to rfc793 smoothing with an alpha of .75 (rttvar =
        // rttvar*3/4 + |delta| / 4). This replaces rfc793's wired-in beta.
        if delta < 0 {
            delta = -delta;
        }
        delta -= tp.t_rttvar.get() >> TCP_RTTVAR_SHIFT;
        tp.t_rttvar.set(tp.t_rttvar.get() + delta);
        if tp.t_rttvar.get() <= 0 {
            tp.t_rttvar.set(1 << TCP_RTT_BASE_SHIFT);
        }
    } else {
        // No rtt measurement yet - use the unsmoothed rtt. Set the variance to half the rtt
        // (so our first retransmit happens at 3*rtt).
        tp.t_srtt
            .set((rtt + 1) << (TCP_RTT_SHIFT + TCP_RTT_BASE_SHIFT));
        tp.t_rttvar
            .set((rtt + 1) << (TCP_RTTVAR_SHIFT + TCP_RTT_BASE_SHIFT - 1));
    }
    tp.t_rtttime.set(0);
    tp.t_rxtshift.set(0);

    // the retransmit should happen at rtt + 4 * rttvar. Because of the way we do the
    // smoothing, srtt and rttvar will each average +1/2 tick of bias. When we compute the
    // retransmit timer, we want 1/2 tick of rounding and 1 extra tick because of +-1/2 tick
    // uncertainty in the firing of the timer. The bias will give us exactly the 1.5 tick we
    // need. But, because the bias is statistical, we have to test that we don't drop below
    // the minimum feasible timer (which is 2 ticks).
    let rttmin = min(
        max(tp.t_rttmin.get(), (rtt + 2 * (tcp_time(1) / HZ)) as u32),
        TCPTV_REXMTMAX as u32,
    ) as i32;
    tp.t_rxtcur
        .set(tcpt_rangeset(tcp_rexmtval(tp), rttmin, TCPTV_REXMTMAX));

    // We received an ack for a packet that wasn't retransmitted; it is probably safe to
    // discard any error indications we've received recently. This isn't quite right, but
    // close enough for now (a route might have failed after we sent a segment, and the
    // return path might not be symmetrical).
    tp.t_softerror.set(None);
}

/// `tcp_mss`: determine a reasonable value for maxseg size. If the route is known, check
/// route for mtu. If none, use an mss that can be handled on the outgoing interface without
/// forcing IP to fragment; if bigger than an mbuf cluster (MCLBYTES), round down to nearest
/// multiple of MCLBYTES to utilize large mbufs. If no route is found, route has no mtu, or
/// the destination isn't local, use a default, hopefully conservative size (usually 512 or
/// the default IP max size, but no more than the mtu of the interface), as we can't discover
/// anything about intervening gateways or networks. We also initialize the congestion/slow
/// start window to be a single segment if the destination isn't local. While looking at the
/// routing entry, we also initialize other path-dependent parameters from pre-set or cached
/// values in the routing entry.
///
/// Also take into account the space needed for options that we send regularly. Make maxseg
/// shorter by that amount to assure that we can send maxseg amount of data even when the
/// options are present. Store the upper limit of the length of options plus data in maxopd.
///
/// NOTE: `offer == -1` indicates that the maxseg size changed due to Path MTU discovery.
pub fn tcp_mss(tp: &'static Tcpcb, offer: i32) -> i32 {
    let mssdflt = TCP_MSSDFLT.load(Ordering::Relaxed);
    let mut mss = mssdflt;
    let mut mssopt = mssdflt;
    let th_len = TCP_HDR_LEN as i32;

    'out: {
        let Some(rt) = in_pcbrtentry(tp.t_inpcb) else {
            break 'out;
        };

        let Some(ifp) = if_get(rt.rt_ifidx.get()) else {
            break 'out;
        };

        let pf = tp.pf.get();
        let iphlen = match pf {
            AF_INET_I32 => IP_HDR_LEN as i32,
            #[cfg(feature = "inet6")]
            AF_INET6_I32 => IP6_HDR_LEN as i32,
            _ => unhandled_af(pf),
        };

        // if there's an mtu associated with the route and we support path MTU discovery for
        // the underlying protocol family, use it.
        let rtmtu = rt.rt_mtu().load(Ordering::Relaxed);
        let if_mtu = ifp.if_mtu.get() as i32;
        if rtmtu != 0 {
            // One may wish to lower MSS to take into account options, especially
            // security-related options.
            if pf == AF_INET6_I32 && rtmtu < IPV6_MMTU {
                // RFC2460 section 5, last paragraph: if path MTU is smaller than 1280, use
                // 1280 as packet size and attach fragment header.
                mss = IPV6_MMTU as i32 - iphlen - size_of::<Ip6Frag>() as i32 - th_len;
            } else {
                mss = rtmtu as i32 - iphlen - th_len;
            }
        } else if ifp.if_flags.get() & IFF_LOOPBACK != 0 {
            mss = if_mtu - iphlen - th_len;
        } else if pf == AF_INET_I32 {
            if ip_mtudisc.load(Ordering::Relaxed) != 0 {
                mss = if_mtu - iphlen - th_len;
            }
        } else if cfg!(feature = "inet6") && pf == AF_INET6_I32 {
            // for IPv6, path MTU discovery is always turned on, or the node must use packet
            // size <= 1280.
            mss = if_mtu - iphlen - th_len;
        }

        // Calculate the value that we offer in TCPOPT_MAXSEG
        if offer != -1 {
            mssopt = if_mtu - iphlen - th_len;
            mssopt = max(mssopt, mssdflt);
        }
        if_put(ifp);
    }
    // out:
    // The current mss, t_maxseg, is initialized to the default value. If we compute a
    // smaller value, reduce the current mss. If we compute a larger value, return it for use
    // in sending a max seg size option, but don't store it for use unless we received an
    // offer at least that large from peer.
    //
    // However, do not accept offers lower than the minimum of the interface MTU and 216.
    if offer > 0 {
        tp.t_peermss.set(offer as u16);
    }
    if tp.t_peermss.get() != 0 {
        mss = min(mss, max(i32::from(tp.t_peermss.get()), 216));
    }

    // sanity - at least max opt. space
    mss = max(mss, 64);

    // maxopd stores the maximum length of data AND options in a segment; maxseg is the
    // amount of data in a normal segment. We need to store this value (maxopd) apart from
    // maxseg, because now every segment carries options and thus we normally have somewhat
    // less data in segments.
    tp.t_maxopd.set(mss as u16);

    if tp.t_flags.get() & (TF_REQ_TSTMP | TF_NOOPT) == TF_REQ_TSTMP
        && tp.t_flags.get() & TF_RCVD_TSTMP == TF_RCVD_TSTMP
    {
        mss -= i32::from(TCPOLEN_TSTAMP_APPA);
    }
    if tp.has_flags(TF_SIGNATURE) {
        mss -= i32::from(TCPOLEN_SIGLEN);
    }

    let do_rfc3390 = TCP_DO_RFC3390.load(Ordering::Relaxed);
    let mssl = mss as u64;
    if offer == -1 {
        // mss changed due to Path MTU discovery
        tp.clear_flags(TF_PMTUD_PEND);
        tp.t_pmtud_mtu_sent.set(0);
        tp.t_pmtud_mss_acked.set(0);
        if mss < i32::from(tp.t_maxseg.get()) {
            // Follow suggestion in RFC 2414 to reduce the congestion window by the ratio of
            // the old segment size to the new segment size.
            tp.snd_cwnd.set(max(
                (tp.snd_cwnd.get() / u64::from(tp.t_maxseg.get())) * mssl,
                mssl,
            ));
        }
    } else if do_rfc3390 == 2 {
        // increase initial window
        tp.snd_cwnd.set(min(10 * mssl, max(2 * mssl, 14600)));
    } else if do_rfc3390 != 0 {
        // increase initial window
        tp.snd_cwnd.set(min(4 * mssl, max(2 * mssl, 4380)));
    } else {
        tp.snd_cwnd.set(mssl);
    }

    tp.t_maxseg.set(mss as u16);

    if offer != -1 { mssopt } else { mss }
}

/// `tcp_hdrsz`: the length of the IP and TCP headers with the options sent on every segment.
pub fn tcp_hdrsz(tp: &Tcpcb) -> u32 {
    let mut hlen = match tp.pf.get() {
        #[cfg(feature = "inet6")]
        AF_INET6_I32 => IP6_HDR_LEN as u32,
        AF_INET_I32 => IP_HDR_LEN as u32,
        _ => 0,
    };
    hlen += TCP_HDR_LEN as u32;

    if tp.t_flags.get() & (TF_REQ_TSTMP | TF_NOOPT) == TF_REQ_TSTMP
        && tp.t_flags.get() & TF_RCVD_TSTMP == TF_RCVD_TSTMP
    {
        hlen += u32::from(TCPOLEN_TSTAMP_APPA);
    }
    if tp.has_flags(TF_SIGNATURE) {
        hlen += u32::from(TCPOLEN_SIGLEN);
    }
    hlen
}

/// `tcp_mss_update`: set connection variables based on the effective MSS. We are passed the
/// TCPCB for the actual connection. If we are the server, we are called by the compressed
/// state engine when the 3-way handshake is complete. If we are the client, we are called
/// when we receive the SYN,ACK from the server.
///
/// NOTE: The `t_maxseg` value must be initialized in the TCPCB before this routine is
/// called!
pub fn tcp_mss_update(tp: &'static Tcpcb) {
    let so = tp.socket();
    let mut mss = u64::from(tp.t_maxseg.get());
    let sb_max = SB_MAX_VAR.load(Ordering::Relaxed);

    if in_pcbrtentry(tp.t_inpcb).is_none() {
        return;
    }

    mtx_enter(&so.so_snd.sb_mtx);
    let mut bufsize = so.so_snd.sb_hiwat.get();
    if bufsize < mss {
        mtx_leave(&so.so_snd.sb_mtx);
        mss = bufsize;
        // Update t_maxseg and t_maxopd
        tcp_mss(tp, mss as i32);
    } else {
        bufsize = bufsize.div_ceil(mss) * mss;
        if bufsize > sb_max {
            bufsize = sb_max;
        }
        let _ = sbreserve(&so.so_snd, bufsize);
        mtx_leave(&so.so_snd.sb_mtx);
    }

    mtx_enter(&so.so_rcv.sb_mtx);
    let mut bufsize = so.so_rcv.sb_hiwat.get();
    if bufsize > mss {
        bufsize = bufsize.div_ceil(mss) * mss;
        if bufsize > sb_max {
            bufsize = sb_max;
        }
        let _ = sbreserve(&so.so_rcv, bufsize);
    }
    mtx_leave(&so.so_rcv.sb_mtx);
}

/// `tcp_newreno_partialack`: when a partial ack arrives, force the retransmission of the next
/// unacknowledged segment. Do not clear `tp->t_dupacks`. By setting `snd_nxt` to `ti_ack`,
/// this forces retransmission timer to be started again.
fn tcp_newreno_partialack(tp: &'static Tcpcb, th: &Tcphdr) {
    // snd_una has not been updated and the socket send buffer not yet drained of the acked
    // data, so we have to leave snd_una as it was to get the correct data offset in
    // tcp_output().
    let onxt = tp.snd_nxt.get();
    let ocwnd = tp.snd_cwnd.get();
    let acked = u64::from(th.th_ack.wrapping_sub(tp.snd_una.get()));
    let maxseg = u64::from(tp.t_maxseg.get());

    tcp_timer_disarm(tp, TCPT_REXMT);
    tp.t_rtttime.set(0);
    tp.snd_nxt.set(th.th_ack);
    // Set snd_cwnd to one segment beyond acknowledged offset (tp->snd_una not yet updated
    // when this function is called)
    tp.snd_cwnd.set(maxseg + acked);
    let _ = tcp_output(tp);
    tp.snd_cwnd.set(ocwnd);
    if seq_gt(onxt, tp.snd_nxt.get()) {
        tp.snd_nxt.set(onxt);
    }
    // Partial window deflation. Relies on fact that tp->snd_una not updated yet.
    if tp.snd_cwnd.get() > acked {
        tp.snd_cwnd.set(tp.snd_cwnd.get() - acked);
    } else {
        tp.snd_cwnd.set(0);
    }
    tp.snd_cwnd.set(tp.snd_cwnd.get() + maxseg);
}

/// `tcp_mss_adv`: the MSS to advertise for a connection routed over `rt`: the interface's
/// MTU less the headers, at least `tcp_mssdflt`.
fn tcp_mss_adv(rt: Option<&Rtentry>, af: i32) -> i32 {
    let mssdflt = TCP_MSSDFLT.load(Ordering::Relaxed);

    let Some(rt) = rt else {
        return mssdflt;
    };

    let Some(ifp) = if_get(rt.rt_ifidx.get()) else {
        return mssdflt;
    };

    let iphlen = match af {
        AF_INET_I32 => IP_HDR_LEN as i32,
        #[cfg(feature = "inet6")]
        AF_INET6_I32 => IP6_HDR_LEN as i32,
        _ => unhandled_af(af),
    };
    let mss = ifp.if_mtu.get() as i32 - iphlen - TCP_HDR_LEN as i32;
    if_put(ifp);

    max(mss, mssdflt)
}

/// `syn_cache_hash`: the bucket hash of a connection from `src` to `dst` under the set's
/// seed `rand`.
fn syn_cache_hash(src: &SynCacheSa, dst: &SynCacheSa, rand: &[u32; 5]) -> u32 {
    match src.sa_family() {
        AF_INET => {
            let src_port = u32::from(src.sin().sin_port);
            let dst_port = u32::from(dst.sin().sin_port);
            let src_addr = src.sin_addr().s_addr;

            (((dst_port << 16).wrapping_add(src_port)) ^ rand[4]).wrapping_mul(src_addr ^ rand[0])
        }
        #[cfg(feature = "inet6")]
        AF_INET6 => {
            let src6 = sa_sin6(src);
            let src_port = u32::from(src6.sin6_port);
            let dst_port = u32::from(sa_sin6(dst).sin6_port);
            let a = &src6.sin6_addr;

            (((dst_port << 16).wrapping_add(src_port)) ^ rand[4])
                .wrapping_mul(a.s6_addr32(0) ^ rand[0])
                .wrapping_mul(a.s6_addr32(1) ^ rand[1])
                .wrapping_mul(a.s6_addr32(2) ^ rand[2])
                .wrapping_mul(a.s6_addr32(3) ^ rand[3])
        }
        family => unhandled_af(i32::from(family)),
    }
}

/// `mallocarray(n, sizeof(struct syn_cache_head), M_SYNCACHE, flags|M_ZERO)` and the
/// `TAILQ_INIT` of every bucket.
fn syn_cache_buckets(n: i32, flags: i32) -> Option<&'static [SynCacheHead]> {
    let n = usize::try_from(n).ok()?;
    let p = mallocarray(n, size_of::<SynCacheHead>(), M_SYNCACHE, flags | M_ZERO)?
        .cast::<SynCacheHead>();
    for i in 0..n {
        // SAFETY: `n` buckets were just allocated at `p`.
        unsafe { p.as_ptr().add(i).write(SynCacheHead::new()) };
    }
    // SAFETY: the buckets are initialised; the array stays allocated until the set is
    // resized while empty (`syn_cache_insert`).
    Some(unsafe { slice::from_raw_parts(p.as_ptr(), n) })
}

/// `syn_cache_rm`: unlinks `sc` from its bucket and its listener's list, and stops its timer.
fn syn_cache_rm(sc: &'static SynCache) {
    mutex_assert_locked(&SYN_CACHE_MTX, "syn_cache_rm");

    // SAFETY: a linked entry's `sc_buckethead` points into its set's bucket array, which
    // stays allocated while the set holds entries; `syn_cache_mtx` is held.
    let head = unsafe { &*sc.sc_buckethead.get() };
    // SAFETY: `sc` is in this bucket (linked by `syn_cache_insert`).
    unsafe { head.sch_bucket.remove(sc) };
    in_pcbunref(sc.sc_inplisten.get());
    sc.sc_inplisten.set(None);
    // SAFETY: `sc` is on its listener's `t_sc` list (linked by `syn_cache_insert`).
    unsafe { ListHead::<ScTpq>::remove(sc) };
    let _ = refcnt_rele(&sc.sc_refcnt);
    head.sch_length.set(head.sch_length.get() - 1);
    if timeout_del(&sc.sc_timer) {
        let _ = refcnt_rele(&sc.sc_refcnt);
    }
    if let Some(set) = sc.sc_set.get() {
        set.scs_count.set(set.scs_count.get() - 1);
    }
}

/// `syn_cache_put`: drops a reference to `sc`; the last one frees it.
fn syn_cache_put(sc: &SynCache) {
    if !refcnt_rele(&sc.sc_refcnt) {
        return;
    }

    // Dealing with last reference, no lock needed.
    m_free(sc.sc_ipopts.get());
    rtfree(sc.sc_route.ro_rt.get());

    pool_put(&SYN_CACHE_POOL, NonNull::from(sc).cast());
}

/// `syn_cache_init`: initialize the hash buckets and the syn cache pool.
pub fn syn_cache_init() {
    let size = tcp_syn_hash_size.load(Ordering::Relaxed);

    // Initialize the hash buckets.
    for set in &TCP_SYN_CACHE {
        let Some(heads) = syn_cache_buckets(size, M_WAITOK) else {
            panic(format_args!("syn_cache_init: mallocarray"));
        };
        set.scs_buckethead.set(heads);
        set.scs_size.set(size);
    }

    // Initialize the syn cache pool.
    pool_init(
        &SYN_CACHE_POOL,
        size_of::<SynCache>(),
        0,
        IPL_SOFTNET,
        0,
        "syncache",
        None,
    );
}

/// `pool_get(&syn_cache_pool, PR_NOWAIT|PR_ZERO)`: a zeroed entry.
fn syn_cache_alloc() -> Option<&'static SynCache> {
    let raw = pool_get(&SYN_CACHE_POOL, PR_NOWAIT | PR_ZERO)?.cast::<SynCache>();
    // SAFETY: a fresh, suitably aligned `syn_cache_pool` item (`syn_cache_init` sizes the
    // pool for a `SynCache`), written once before anything else reads it; it stays allocated
    // until the last `syn_cache_put`.
    unsafe {
        raw.as_ptr().write(SynCache::new());
        Some(&*raw.as_ptr())
    }
}

/// `syn_cache_insert`: puts `sc` into the active set and on `tp`'s list, making room in a
/// full bucket or a full cache, and arms its retransmit timer.
fn syn_cache_insert(sc: &'static SynCache, tp: &'static Tcpcb) {
    net_assert_locked("syn_cache_insert");
    mutex_assert_locked(&SYN_CACHE_MTX, "syn_cache_insert");

    let active = TCP_SYN_CACHE_ACTIVE.load(Ordering::Relaxed);
    let set: &'static SynCacheSet = &TCP_SYN_CACHE[active];

    // If there are no entries in the hash table, reinitialize the hash secrets. To avoid
    // useless cache swaps and reinitialization, use it until the limit is reached. An empty
    // cache is also the opportunity to resize the hash.
    if set.scs_count.get() == 0 && set.scs_use.get() <= 0 {
        set.scs_use
            .set(i64::from(TCP_SYN_USE_LIMIT.load(Ordering::Relaxed)));
        let hash_size = tcp_syn_hash_size.load(Ordering::Relaxed);
        if set.scs_size.get() != hash_size {
            match syn_cache_buckets(hash_size, M_NOWAIT) {
                None => {
                    // Try again next time.
                    set.scs_use.set(0);
                }
                Some(scp) => {
                    let old = set.scs_buckethead.get();
                    if !old.is_empty() {
                        free(NonNull::from(old).cast(), M_SYNCACHE, size_of_val(old));
                    }
                    set.scs_buckethead.set(scp);
                    set.scs_size.set(hash_size);
                }
            }
        }
        let mut bytes = [0u8; 20];
        arc4random_buf(&mut bytes);
        let mut random = [0u32; 5];
        for (r, b) in random.iter_mut().zip(bytes.as_chunks::<4>().0) {
            *r = u32::from_ne_bytes(*b);
        }
        set.scs_random.set(random);
        tcpstat_inc(TcpstatCounters::TcpsScSeedrandom);
    }

    let hash = syn_cache_hash(&sc.sc_src.get(), &sc.sc_dst.get(), &set.scs_random.get());
    sc.sc_hash.set(hash);
    let heads = set.scs_buckethead.get();
    let idx = (hash % set.scs_size.get() as u32) as usize;
    let scp = &heads[idx];
    sc.sc_buckethead.set(scp);

    // Make sure that we don't overflow the per-bucket limit or the total cache size limit.
    if i32::from(scp.sch_length.get()) >= TCP_SYN_BUCKET_LIMIT.load(Ordering::Relaxed) {
        tcpstat_inc(TcpstatCounters::TcpsScBucketoverflow);
        // Someone might attack our bucket hash function. Reseed with random as soon as the
        // passive syn cache gets empty.
        set.scs_use.set(0);
        // The bucket is full. Toss the oldest element in the bucket. This will be the first
        // entry in the bucket.
        let sc2 = scp.sch_bucket.first();
        // This should never happen; we should always find an entry in our bucket.
        #[cfg(feature = "diagnostic")]
        if sc2.is_none() {
            panic(format_args!("syn_cache_insert: bucketoverflow: impossible"));
        }
        if let Some(sc2) = sc2 {
            syn_cache_rm(sc2);
            syn_cache_put(sc2);
        }
    } else if set.scs_count.get() >= TCP_SYN_CACHE_LIMIT.load(Ordering::Relaxed) {
        tcpstat_inc(TcpstatCounters::TcpsScOverflowed);
        // The cache is full. Toss the oldest entry in the first non-empty bucket we can
        // find.
        //
        // XXX We would really like to toss the oldest entry in the cache, but we hope that
        // this condition doesn't happen very often.
        let mut i2 = idx;
        if heads[i2].sch_bucket.is_empty() {
            i2 += 1;
            while i2 != idx {
                if i2 >= heads.len() {
                    i2 = 0;
                }
                if !heads[i2].sch_bucket.is_empty() {
                    break;
                }
                i2 += 1;
            }
            // This should never happen; we should always find a non-empty bucket.
            #[cfg(feature = "diagnostic")]
            if i2 == idx {
                panic(format_args!("syn_cache_insert: cacheoverflow: impossible"));
            }
        }
        if let Some(sc2) = heads[i2].sch_bucket.first() {
            syn_cache_rm(sc2);
            syn_cache_put(sc2);
        }
    }

    // Initialize the entry's timer. We don't estimate RTT with SYNs, so each packet starts
    // with the default RTT and each timer step has a fixed timeout value.
    sc.sc_rxttot.set(0);
    sc.sc_rxtshift.set(0);
    sc.sc_rxtcur.set(tcpt_rangeset(
        TCPTV_SRTTDFLT * TCP_BACKOFF[sc.sc_rxtshift.get() as usize],
        TCPTV_MIN,
        TCPTV_REXMTMAX,
    ) as u32);
    if timeout_add_msec(&sc.sc_timer, u64::from(sc.sc_rxtcur.get())) {
        refcnt_take(&sc.sc_refcnt);
    }

    // Link it from tcpcb entry
    refcnt_take(&sc.sc_refcnt);
    // SAFETY: `sc` is a pool item on no `t_sc` list; it stays in place until `syn_cache_rm`
    // unlinks it.
    unsafe { tp.t_sc.insert_head(sc) };

    // Put it into the bucket.
    // SAFETY: as above, for the bucket, which stays in place while it holds entries.
    unsafe { scp.sch_bucket.insert_tail(sc) };
    scp.sch_length.set(scp.sch_length.get() + 1);
    sc.sc_set.set(Some(set));
    set.scs_count.set(set.scs_count.get() + 1);
    set.scs_use.set(set.scs_use.get() - 1);

    tcpstat_inc(TcpstatCounters::TcpsScAdded);

    // If the active cache has exceeded its use limit and the passive syn cache is empty,
    // exchange their roles.
    if set.scs_use.get() <= 0 && TCP_SYN_CACHE[active ^ 1].scs_count.get() == 0 {
        TCP_SYN_CACHE_ACTIVE.store(active ^ 1, Ordering::Relaxed);
    }
}

/// `syn_cache_timer`: the retransmit timer of one entry. Retransmits its SYN,ACK, or, after
/// the maximum number of retransmissions or the keep-alive time, expires the entry.
fn syn_cache_timer(arg: *mut c_void) {
    // SAFETY: the timer was set with its entry as the argument and holds a reference to it
    // (taken when armed), so the entry is alive until the `syn_cache_put` below.
    let sc: &'static SynCache = unsafe { &*arg.cast::<SynCache>() };

    mtx_enter(&SYN_CACHE_MTX);
    let inp = in_pcbref(sc.sc_inplisten.get());
    'freeit: {
        let Some(inp) = inp else {
            break 'freeit;
        };

        'dropit: {
            if sc.sc_rxtshift.get() == TCP_MAXRXTSHIFT as u32 {
                // Drop it -- too many retransmissions.
                break 'dropit;
            }

            // Compute the total amount of time this entry has been on a queue. If this entry
            // has been on longer than the keep alive timer would allow, expire it.
            sc.sc_rxttot
                .set(sc.sc_rxttot.get().wrapping_add(sc.sc_rxtcur.get()));
            if sc.sc_rxttot.get() >= TCP_KEEPINIT.load(Ordering::Relaxed) as u32 {
                break 'dropit;
            }

            // Advance the timer back-off.
            sc.sc_rxtshift.set(sc.sc_rxtshift.get() + 1);
            sc.sc_rxtcur.set(tcpt_rangeset(
                TCPTV_SRTTDFLT * TCP_BACKOFF[sc.sc_rxtshift.get() as usize],
                TCPTV_MIN,
                TCPTV_REXMTMAX,
            ) as u32);
            if timeout_add_msec(&sc.sc_timer, u64::from(sc.sc_rxtcur.get())) {
                refcnt_take(&sc.sc_refcnt);
            }
            mtx_leave(&SYN_CACHE_MTX);

            net_lock_shared();
            let so = in_pcbsolock(inp);
            if so.is_some() {
                let now = tcp_now();
                let do_ecn = TCP_DO_ECN.load(Ordering::Relaxed) != 0;
                let _ = syn_cache_respond(sc, None, now, do_ecn);
                tcpstat_inc(TcpstatCounters::TcpsScRetransmitted);
            }
            in_pcbsounlock(Some(inp), so);
            net_unlock_shared();

            in_pcbunref(Some(inp));
            syn_cache_put(sc);
            return;
        }

        // dropit:
        tcpstat_inc(TcpstatCounters::TcpsScTimedOut);
        syn_cache_rm(sc);
        in_pcbunref(Some(inp));
        // Decrement reference of the timer and free object after remove.
        let lastref = refcnt_rele(&sc.sc_refcnt);
        kassert!(!lastref);
    }
    // freeit:
    mtx_leave(&SYN_CACHE_MTX);
    syn_cache_put(sc);
}

/// `syn_cache_cleanup`: remove syn cache created by the specified tcb entry, because this
/// does not make sense to keep them (if there's no tcb entry, syn cache entry will never be
/// used)
pub fn syn_cache_cleanup(tp: &'static Tcpcb) {
    net_assert_locked("syn_cache_cleanup");

    mtx_enter(&SYN_CACHE_MTX);
    let mut sc = tp.t_sc.first();
    while let Some(s) = sc {
        let nsc = ListHead::<ScTpq>::next(s);
        kassert!(s.sc_inplisten.get().is_some_and(|i| ptr::eq(i, tp.t_inpcb)));
        syn_cache_rm(s);
        syn_cache_put(s);
        sc = nsc;
    }
    mtx_leave(&SYN_CACHE_MTX);

    kassert!(tp.t_sc.is_empty());
}

/// `syn_cache_lookup`: find an entry in the syn cache.
fn syn_cache_lookup(
    src: &SynCacheSa,
    dst: &SynCacheSa,
    rtableid: u32,
) -> Option<&'static SynCache> {
    net_assert_locked("syn_cache_lookup");
    mutex_assert_locked(&SYN_CACHE_MTX, "syn_cache_lookup");

    // Check the active cache first, the passive cache is likely empty.
    let active = TCP_SYN_CACHE_ACTIVE.load(Ordering::Relaxed);
    let sets: [&'static SynCacheSet; 2] = [&TCP_SYN_CACHE[active], &TCP_SYN_CACHE[active ^ 1]];
    let srclen = usize::from(src.sa_len()).min(size_of::<SynCacheSa>());
    let dstlen = usize::from(dst.sa_len()).min(size_of::<SynCacheSa>());
    for set in sets {
        if set.scs_count.get() == 0 {
            continue;
        }
        let hash = syn_cache_hash(src, dst, &set.scs_random.get());
        let heads = set.scs_buckethead.get();
        let scp = &heads[(hash % set.scs_size.get() as u32) as usize];
        for sc in scp.sch_bucket.iter() {
            if sc.sc_hash.get() != hash {
                continue;
            }
            if sc.sc_src.get().as_bytes()[..srclen] == src.as_bytes()[..srclen]
                && sc.sc_dst.get().as_bytes()[..dstlen] == dst.as_bytes()[..dstlen]
                && rtable_l2(rtableid) == rtable_l2(sc.sc_rtableid.get())
            {
                return Some(sc);
            }
        }
    }
    None
}

/// `syn_cache_get`: called when we receive an ACK for a socket in the LISTEN state. We look
/// up the connection in the syn cache, and if its there, we pull it out of the cache and
/// turn it into a full-blown connection in the SYN-RECEIVED state. See [`SynCacheGet`] for
/// the three outcomes; on all of them the listening socket `so` has been unlocked.
#[allow(clippy::too_many_arguments)] // the C's prototype
fn syn_cache_get(
    src: &SynCacheSa,
    dst: &SynCacheSa,
    th: &Tcphdr,
    _hlen: i32,
    _tlen: i32,
    so: &'static Socket,
    m: &'static Mbuf,
    now: u64,
    do_ecn: bool,
) -> SynCacheGet {
    net_assert_locked("syn_cache_get");

    let Some(listeninp) = sotoinpcb(so) else {
        in_pcbsounlock(None, Some(so));
        return SynCacheGet::NotFound;
    };

    mtx_enter(&SYN_CACHE_MTX);
    let Some(sc) = syn_cache_lookup(src, dst, listeninp.inp_rtableid.get()) else {
        mtx_leave(&SYN_CACHE_MTX);
        in_pcbsounlock(Some(listeninp), Some(so));
        return SynCacheGet::NotFound;
    };

    // Verify the sequence and ack numbers. Try getting the correct response again.
    if th.th_ack != sc.sc_iss.get().wrapping_add(1)
        || seq_leq(th.th_seq, sc.sc_irs.get())
        || seq_gt(
            th.th_seq,
            sc.sc_irs
                .get()
                .wrapping_add(1)
                .wrapping_add(sc.sc_win.get() as u32),
        )
    {
        refcnt_take(&sc.sc_refcnt);
        mtx_leave(&SYN_CACHE_MTX);
        let _ = syn_cache_respond(sc, Some(m), now, do_ecn);
        in_pcbsounlock(Some(listeninp), Some(so));
        syn_cache_put(sc);
        return SynCacheGet::Aborted;
    }

    // Remove this cache entry
    syn_cache_rm(sc);
    mtx_leave(&SYN_CACHE_MTX);

    // Ok, create the full blown connection, and set things up as they would have been set up
    // if we had created the connection when the SYN arrived. If we can't create the
    // connection, abort it.
    let listenso = so;
    let mut inp: Option<&'static Inpcb> = None;
    let mut tp: Option<&'static Tcpcb> = None;
    let so = sonewconn(listenso, SS_ISCONNECTED, M_DONTWAIT);

    let reset = 'abort: {
        let Some(nso) = so else {
            break 'abort true;
        };
        soassertlocked(nso);
        // inpcb does refcount socket, both so and inp cannot go away
        inp = in_pcbref(sotoinpcb(nso));
        tp = inp.and_then(intotcpcb);
        let (Some(ninp), Some(ntp)) = (inp, tp) else {
            break 'abort true;
        };

        // We need to copy the required security levels from the listen pcb. Ditto for any
        // other IPsec-related information.
        ninp.inp_seclevel.set(listeninp.inp_seclevel.get());
        #[cfg(feature = "inet6")]
        let inet6 = ninp.has_flags(INP_IPV6);
        #[cfg(not(feature = "inet6"))]
        let inet6 = false;
        if inet6 {
            kassert!(listeninp.has_flags(INP_IPV6));

            let mut ip6 = ninp.inp_ipv6.get();
            ip6.ip6_hlim = listeninp.inp_ipv6.get().ip6_hlim;
            ninp.inp_ipv6.set(ip6);
            ninp.inp_hops.set(listeninp.inp_hops.get());
        } else {
            kassert!(!listeninp.has_flags(INP_IPV6));

            let mut ip = ninp.inp_ip.get();
            ip.ip_ttl = listeninp.inp_ip.get().ip_ttl;
            ninp.inp_ip.set(ip);
            ninp.inp_options.set(ip_srcroute(m));
            if ninp.inp_options.get().is_none() {
                ninp.inp_options.set(sc.sc_ipopts.get());
                sc.sc_ipopts.set(None);
            }
        }

        // inherit rtable from listening socket
        let mut rtableid = sc.sc_rtableid.get();
        if m.m_pkthdr().pf.flags.get() & PF_TAG_DIVERTED != 0 {
            let divert = pf_find_divert(m);
            kassert!(divert.is_some());
            if let Some(divert) = divert {
                rtableid = u32::from(divert.rdomain);
            }
        }
        if in_pcbset_addr(ninp, src, dst, rtableid).is_err() {
            break 'abort true;
        }

        // Give the new socket our cached route reference.
        let ro = &ninp.inp_route; // struct assignment
        ro.ro_rt.set(sc.sc_route.ro_rt.get());
        ro.ro_generation.set(sc.sc_route.ro_generation.get());
        ro.ro_tableid.set(sc.sc_route.ro_tableid.get());
        ro.ro_dst.set(sc.sc_route.ro_dst.get());
        ro.ro_src.set(sc.sc_route.ro_src.get());
        sc.sc_route.ro_rt.set(None);

        let listenflags = intotcpcb(listeninp).map_or(0, |l| l.t_flags.get());
        ntp.t_flags.set(listenflags & (TF_NOPUSH | TF_NODELAY));
        if sc.sc_request_r_scale.get() != 15 {
            ntp.requested_s_scale.set(sc.sc_requested_s_scale.get());
            ntp.request_r_scale.set(sc.sc_request_r_scale.get());
            ntp.set_flags(TF_REQ_SCALE | TF_RCVD_SCALE);
        }
        let fixflags = sc.sc_fixflags.get();
        if fixflags & SCF_TIMESTAMP != 0 {
            ntp.set_flags(TF_REQ_TSTMP | TF_RCVD_TSTMP);
        }

        ntp.t_template.set(tcp_template(ntp));
        if ntp.t_template.get().is_none() {
            break 'abort false;
        }
        ntp.sack_enable.set(i32::from(fixflags & SCF_SACK_PERMIT));
        ntp.ts_modulate.set(sc.sc_modulate.get());
        ntp.ts_recent.set(sc.sc_timestamp.get() as u32);
        ntp.iss.set(sc.sc_iss.get());
        ntp.irs.set(sc.sc_irs.get());
        tcp_sendseqinit(ntp);
        ntp.snd_last.set(ntp.snd_una.get());
        if fixflags & SCF_ECN_PERMIT != 0 {
            ntp.set_flags(TF_ECN_PERMIT);
            tcpstat_inc(TcpstatCounters::TcpsEcnAccepts);
        }
        if fixflags & SCF_SACK_PERMIT != 0 {
            ntp.set_flags(TF_SACK_PERMIT);
        }
        if fixflags & SCF_SIGNATURE != 0 {
            ntp.set_flags(TF_SIGNATURE);
        }
        tcp_rcvseqinit(ntp);
        ntp.t_state.set(TCPS_SYN_RECEIVED);
        ntp.t_rcvtime.set(now);
        ntp.t_sndtime.set(now);
        ntp.t_rcvacktime.set(now);
        ntp.t_sndacktime.set(now);
        tcp_timer_arm(ntp, TCPT_KEEP, TCP_KEEPINIT.load(Ordering::Relaxed) as u64);
        tcpstat_inc(TcpstatCounters::TcpsAccepts);

        tcp_mss(ntp, i32::from(sc.sc_peermaxseg.get())); // sets t_maxseg
        if sc.sc_peermaxseg.get() != 0 {
            tcp_mss_update(ntp);
        }
        // Reset initial window to 1 segment for retransmit
        if sc.sc_rxtshift.get() > 0 {
            ntp.snd_cwnd.set(u64::from(ntp.t_maxseg.get()));
        }
        ntp.snd_wl1.set(sc.sc_irs.get());
        ntp.rcv_up.set(sc.sc_irs.get().wrapping_add(1));

        // This is what would have happened in tcp_output() when the SYN,ACK was sent.
        ntp.snd_up.set(ntp.snd_una.get());
        ntp.snd_nxt.set(ntp.iss.get().wrapping_add(1));
        ntp.snd_max.set(ntp.snd_nxt.get());
        tcp_timer_arm(ntp, TCPT_REXMT, ntp.t_rxtcur.get() as u64);
        let win = sc.sc_win.get() as u32;
        if sc.sc_win.get() > 0 && seq_gt(ntp.rcv_nxt.get().wrapping_add(win), ntp.rcv_adv.get()) {
            ntp.rcv_adv.set(ntp.rcv_nxt.get().wrapping_add(win));
        }
        ntp.last_ack_sent.set(ntp.rcv_nxt.get());

        in_pcbsounlock(Some(listeninp), Some(listenso));
        tcpstat_inc(TcpstatCounters::TcpsScCompleted);
        syn_cache_put(sc);
        return SynCacheGet::Socket(nso);
    };

    if reset {
        // resetandabort:
        tcp_respond(
            None,
            &ip_template(m),
            Some(th),
            0,
            th.th_ack,
            TH_RST,
            m.m_pkthdr().ph_rtableid.get(),
            now,
        );
    }
    // abort:
    if let Some(t) = tp {
        let _ = tcp_drop(t, Some(Errno::ECONNABORTED)); // destroys socket
    }
    in_pcbsounlock(inp, so);
    in_pcbsounlock(Some(listeninp), Some(listenso));
    in_pcbunref(inp);
    m_freem(m);
    syn_cache_put(sc);
    tcpstat_inc(TcpstatCounters::TcpsScAborted);
    SynCacheGet::Aborted
}

/// `syn_cache_reset`: called when we get a RST for a non-existent connection, so that we can
/// see if the connection is in the syn cache. If it is, zap it.
fn syn_cache_reset(src: &SynCacheSa, dst: &SynCacheSa, th: &Tcphdr, rtableid: u32) {
    net_assert_locked("syn_cache_reset");

    mtx_enter(&SYN_CACHE_MTX);
    let Some(sc) = syn_cache_lookup(src, dst, rtableid) else {
        mtx_leave(&SYN_CACHE_MTX);
        return;
    };
    if seq_lt(th.th_seq, sc.sc_irs.get()) || seq_gt(th.th_seq, sc.sc_irs.get().wrapping_add(1)) {
        mtx_leave(&SYN_CACHE_MTX);
        return;
    }
    syn_cache_rm(sc);
    mtx_leave(&SYN_CACHE_MTX);
    tcpstat_inc(TcpstatCounters::TcpsScReset);
    syn_cache_put(sc);
}

/// `syn_cache_unreach`: an ICMP unreachable for the embryonic connection from `src` to `dst`
/// whose returned TCP header is `th` (network order); the second one after three
/// retransmissions removes the entry.
///
/// # Safety
///
/// `src` and `dst` point to socket addresses readable for their `sa_len` bytes.
pub unsafe fn syn_cache_unreach(
    src: *const Sockaddr,
    dst: *const Sockaddr,
    th: &Tcphdr,
    rtableid: u32,
) {
    let mut s = SynCacheSa::new();
    let mut d = SynCacheSa::new();
    // SAFETY: the caller's contract.
    unsafe {
        s.copy_from_sa(src);
        d.copy_from_sa(dst);
    }

    net_assert_locked("syn_cache_unreach");

    mtx_enter(&SYN_CACHE_MTX);
    let Some(sc) = syn_cache_lookup(&s, &d, rtableid) else {
        mtx_leave(&SYN_CACHE_MTX);
        return;
    };
    // If the sequence number != sc_iss, then it's a bogus ICMP msg
    if ntohl(th.th_seq) != sc.sc_iss.get() {
        mtx_leave(&SYN_CACHE_MTX);
        return;
    }

    // If we've retransmitted 3 times and this is our second error, we remove the entry.
    // Otherwise, we allow it to continue on. This prevents us from incorrectly nuking an
    // entry during a spurious network outage.
    //
    // See tcp_notify().
    if sc.sc_dynflags.get() & SCF_UNREACH == 0 || sc.sc_rxtshift.get() < 3 {
        sc.sc_dynflags.set(sc.sc_dynflags.get() | SCF_UNREACH);
        mtx_leave(&SYN_CACHE_MTX);
        return;
    }

    syn_cache_rm(sc);
    mtx_leave(&SYN_CACHE_MTX);
    tcpstat_inc(TcpstatCounters::TcpsScUnreach);
    syn_cache_put(sc);
}

/// `syn_cache_add`: given a LISTEN socket and an inbound SYN request, add this to the syn
/// cache, and send back a segment: `<SEQ=ISS><ACK=RCV_NXT><CTL=SYN,ACK>` to the source.
/// `false` (the C's -1) leaves `m` to the caller; otherwise it was consumed.
///
/// IMPORTANT NOTE: We do _NOT_ ACK data that might accompany the SYN. Doing so would require
/// that we hold onto the data and deliver it to the application. However, if we are the
/// target of a SYN-flood DoS attack, an attacker could send data which would eventually
/// consume all available buffer space if it were ACKed. By not ACKing the data, we avoid
/// this DoS scenario.
#[allow(clippy::too_many_arguments)] // the C's prototype
fn syn_cache_add(
    src: &SynCacheSa,
    dst: &SynCacheSa,
    th: &Tcphdr,
    iphlen: i32,
    so: &'static Socket,
    m: &'static Mbuf,
    optp: Option<&[u8]>,
    oi: &mut TcpOptInfo,
    issp: Option<TcpSeq>,
    now: u64,
    do_ecn: bool,
) -> bool {
    soassertlocked(so);

    let Some(tp) = sototcpcb(so) else {
        return false;
    };
    let rtableid = tp.t_inpcb.inp_rtableid.get();

    // RFC1122 4.2.3.10, p. 104: discard bcast/mcast SYN
    //
    // Note this check is performed in tcp_input() very early on.

    // Initialize some local state.
    let mut win = sbspace(&so.so_rcv);
    if win > i64::from(TCP_MAXWIN) {
        win = i64::from(TCP_MAXWIN);
    }

    let tb = Tcpcb::new(tp.t_inpcb);
    if optp.is_some() || tp.has_flags(TF_SIGNATURE) {
        tb.pf.set(tp.pf.get());
        tb.sack_enable.set(tp.sack_enable.get());
        tb.t_flags
            .set(if TCP_DO_RFC1323.load(Ordering::Relaxed) != 0 {
                TF_REQ_SCALE | TF_REQ_TSTMP
            } else {
                0
            });
        if tp.has_flags(TF_SIGNATURE) {
            tb.set_flags(TF_SIGNATURE);
        }
        tb.t_state.set(TCPS_LISTEN);
        if !tcp_dooptions(&tb, optp.unwrap_or(&[]), th, m, iphlen, oi, rtableid, now) {
            return false;
        }
    }

    // Remember the IP options, if any.
    let ipopts = if src.sa_family() == AF_INET {
        ip_srcroute(m)
    } else {
        None
    };

    // See if we already have an entry for this connection. If we do, resend the SYN,ACK. We
    // do not count this as a retransmission (XXX though maybe we should).
    mtx_enter(&SYN_CACHE_MTX);
    if let Some(sc) = syn_cache_lookup(src, dst, rtableid) {
        refcnt_take(&sc.sc_refcnt);
        mtx_leave(&SYN_CACHE_MTX);
        tcpstat_inc(TcpstatCounters::TcpsScDupesyn);
        if ipopts.is_some() {
            // If we were remembering a previous source route, forget it and use the new one
            // we've been given.
            m_free(sc.sc_ipopts.get());
            sc.sc_ipopts.set(ipopts);
        }
        sc.sc_timestamp.set(u64::from(tb.ts_recent.get()));
        if syn_cache_respond(sc, Some(m), now, do_ecn).is_ok() {
            tcpstat_inc(TcpstatCounters::TcpsSndacks);
            tcpstat_inc(TcpstatCounters::TcpsSndtotal);
        }
        syn_cache_put(sc);
        return true;
    }
    mtx_leave(&SYN_CACHE_MTX);

    let Some(sc) = syn_cache_alloc() else {
        m_free(ipopts);
        return false;
    };
    refcnt_init_trace(&sc.sc_refcnt, DT_REFCNT_IDX_SYNCACHE);
    timeout_set_flags(
        &sc.sc_timer,
        syn_cache_timer,
        ptr::from_ref(sc).cast_mut().cast(),
        KCLOCK_NONE,
        TIMEOUT_PROC | TIMEOUT_MPSAFE,
    );

    // Fill in the cache, and put the necessary IP and TCP options into the reply.
    let mut s = SynCacheSa::new();
    let srclen = usize::from(src.sa_len()).min(size_of::<SynCacheSa>());
    s.as_bytes_mut()[..srclen].copy_from_slice(&src.as_bytes()[..srclen]);
    sc.sc_src.set(s);
    let mut d = SynCacheSa::new();
    let dstlen = usize::from(dst.sa_len()).min(size_of::<SynCacheSa>());
    d.as_bytes_mut()[..dstlen].copy_from_slice(&dst.as_bytes()[..dstlen]);
    sc.sc_dst.set(d);
    sc.sc_rtableid.set(rtableid);
    let mut rt = None;
    if s.sa_family() == AF_INET {
        let src_addr = s.sin_addr();
        if src_addr.s_addr != INADDR_ANY {
            rt = route_mpath(
                &sc.sc_route,
                &src_addr,
                Some(&d.sin_addr()),
                sc.sc_rtableid.get(),
            );
        }
    }
    #[cfg(feature = "inet6")]
    if s.sa_family() == AF_INET6 {
        let src6 = sa_sin6(&s).sin6_addr;
        if !in6_is_addr_unspecified(&src6) {
            rt = route6_mpath(
                &sc.sc_route,
                &src6,
                Some(&sa_sin6(&d).sin6_addr),
                sc.sc_rtableid.get(),
            );
        }
    }
    sc.sc_ipopts.set(ipopts);
    sc.sc_irs.set(th.th_seq);

    sc.sc_iss.set(issp.unwrap_or_else(arc4random));
    sc.sc_peermaxseg.set(oi.maxseg);
    sc.sc_ourmaxseg
        .set(tcp_mss_adv(rt, i32::from(s.sa_family())) as u16);
    sc.sc_win.set(win);
    sc.sc_timestamp.set(u64::from(tb.ts_recent.get()));
    if tb.t_flags.get() & (TF_REQ_TSTMP | TF_RCVD_TSTMP) == (TF_REQ_TSTMP | TF_RCVD_TSTMP) {
        sc.sc_fixflags.set(sc.sc_fixflags.get() | SCF_TIMESTAMP);
        sc.sc_modulate.set(arc4random());
    }
    if tb.t_flags.get() & (TF_RCVD_SCALE | TF_REQ_SCALE) == (TF_RCVD_SCALE | TF_REQ_SCALE) {
        sc.sc_requested_s_scale.set(tb.requested_s_scale.get());
        sc.sc_request_r_scale.set(0);
        // Pick the smallest possible scaling factor that will still allow us to scale up to
        // sb_max.
        //
        // We do this because there are broken firewalls that will corrupt the window scale
        // option, leading to the other endpoint believing that our advertised window is
        // unscaled. At scale factors larger than 5 the unscaled window will drop below 1500
        // bytes, leading to serious problems when traversing these broken firewalls.
        //
        // With the default sbmax of 256K, a scale factor of 3 will be chosen by this
        // algorithm. Those who choose a larger sbmax should watch out for the compatibility
        // problems mentioned above.
        //
        // RFC1323: The Window field in a SYN (i.e., a <SYN> or <SYN,ACK>) segment itself is
        // never scaled.
        let sb_max = SB_MAX_VAR.load(Ordering::Relaxed);
        while sc.sc_request_r_scale.get() < TCP_MAX_WINSHIFT
            && u64::from(TCP_MAXWIN << sc.sc_request_r_scale.get()) < sb_max
        {
            sc.sc_request_r_scale.set(sc.sc_request_r_scale.get() + 1);
        }
    } else {
        sc.sc_requested_s_scale.set(15);
        sc.sc_request_r_scale.set(15);
    }
    // if both ECE and CWR flag bits are set, peer is ECN capable.
    if do_ecn && th.th_flags & (TH_ECE | TH_CWR) == (TH_ECE | TH_CWR) {
        sc.sc_fixflags.set(sc.sc_fixflags.get() | SCF_ECN_PERMIT);
    }
    // Set SCF_SACK_PERMIT if peer did send a SACK_PERMITTED option (i.e., if tcp_dooptions()
    // did set TF_SACK_PERMIT).
    if tb.sack_enable.get() != 0 && tb.has_flags(TF_SACK_PERMIT) {
        sc.sc_fixflags.set(sc.sc_fixflags.get() | SCF_SACK_PERMIT);
    }
    if tb.has_flags(TF_SIGNATURE) {
        sc.sc_fixflags.set(sc.sc_fixflags.get() | SCF_SIGNATURE);
    }
    sc.sc_inplisten.set(in_pcbref(Some(tp.t_inpcb)));
    if syn_cache_respond(sc, Some(m), now, do_ecn).is_ok() {
        mtx_enter(&SYN_CACHE_MTX);
        // Socket lock prevents another insert after our syn_cache_lookup() and before
        // syn_cache_insert().
        syn_cache_insert(sc, tp);
        mtx_leave(&SYN_CACHE_MTX);
        tcpstat_inc(TcpstatCounters::TcpsSndacks);
        tcpstat_inc(TcpstatCounters::TcpsSndtotal);
    } else {
        in_pcbunref(sc.sc_inplisten.get());
        syn_cache_put(sc);
        tcpstat_inc(TcpstatCounters::TcpsScDropped);
    }

    true
}

/// `syn_cache_respond`: builds the SYN,ACK of `sc` from scratch (freeing `m`, the segment it
/// answers) and sends it.
fn syn_cache_respond(
    sc: &SynCache,
    m: Option<&'static Mbuf>,
    now: u64,
    do_ecn: bool,
) -> Result<(), Errno> {
    net_assert_locked("syn_cache_respond");

    let src = sc.sc_src.get();
    let dst = sc.sc_dst.get();
    let family = src.sa_family();
    let hlen = match family {
        AF_INET => IP_HDR_LEN,
        #[cfg(feature = "inet6")]
        AF_INET6 => IP6_HDR_LEN,
        _ => {
            m_freem(m);
            return Err(Errno::EAFNOSUPPORT);
        }
    };

    // Compute the size of the TCP options.
    let fixflags = sc.sc_fixflags.get();
    let optlen =
        4 + if sc.sc_request_r_scale.get() != 15 {
            4
        } else {
            0
        } + if fixflags & SCF_SACK_PERMIT != 0 {
            4
        } else {
            0
        } + if fixflags & SCF_SIGNATURE != 0 {
            usize::from(TCPOLEN_SIGLEN)
        } else {
            0
        } + if fixflags & SCF_TIMESTAMP != 0 {
            usize::from(TCPOLEN_TSTAMP_APPA)
        } else {
            0
        };

    let tlen = hlen + TCP_HDR_LEN + optlen;

    // Create the IP+TCP header from scratch.
    m_freem(m);
    let max_linkhdr = MAX_LINKHDR.load(Ordering::Relaxed) as usize;
    #[cfg(feature = "diagnostic")]
    if max_linkhdr + tlen > crate::sys::mbuf::MCLBYTES {
        return Err(Errno::ENOBUFS);
    }
    let mut m = m_gethdr(M_DONTWAIT, MT_DATA);
    if let Some(mm) = m
        && max_linkhdr + tlen > MHLEN
    {
        mclget(mm, M_DONTWAIT);
        if mm.m_flags().get() & M_EXT == 0 {
            m_freem(mm);
            m = None;
        }
    }
    let Some(m) = m else {
        return Err(Errno::ENOBUFS);
    };

    // Fixup the mbuf.
    m.m_data().set(m.m_data().get().wrapping_add(max_linkhdr));
    m.m_len().set(tlen as u32);
    m.m_pkthdr().len.set(tlen as i32);
    m.m_pkthdr().ph_ifidx.set(0);
    m.m_pkthdr().ph_rtableid.set(sc.sc_rtableid.get());
    // SAFETY: the mbuf (a cluster if the header did not fit) holds `tlen` bytes past the
    // `max_linkhdr` reserved in front.
    unsafe { ptr::write_bytes(mtod::<u8>(m), 0, tlen) };

    let (th_dport, th_sport) = match family {
        #[cfg(feature = "inet6")]
        AF_INET6 => {
            let ssin6 = sa_sin6(&src);
            let dsin6 = sa_sin6(&dst);
            let mut ip6 = mtod_ip6(m);
            ip6.ip6_dst = ssin6.sin6_addr;
            ip6.ip6_src = dsin6.sin6_addr;
            ip6.ip6_nxt = IPPROTO_TCP as u8;
            mtod_ip6_store(m, &ip6);
            (ssin6.sin6_port, dsin6.sin6_port)
        }
        _ => {
            let ssin = src.sin();
            let dsin = dst.sin();
            let mut ip = mtod_ip(m);
            ip.ip_dst = ssin.sin_addr;
            ip.ip_src = dsin.sin_addr;
            ip.ip_p = IPPROTO_TCP as u8;
            mtod_ip_store(m, &ip);
            (ssin.sin_port, dsin.sin_port)
        }
    };
    let mut th = Tcphdr {
        th_dport,
        th_sport,
        ..Tcphdr::default()
    };

    th.th_seq = htonl(sc.sc_iss.get());
    th.th_ack = htonl(sc.sc_irs.get().wrapping_add(1));
    th.set_th_off(((TCP_HDR_LEN + optlen) >> 2) as u8);
    th.th_flags = TH_SYN | TH_ACK;
    // Set ECE for SYN-ACK if peer supports ECN.
    if do_ecn && fixflags & SCF_ECN_PERMIT != 0 {
        th.th_flags |= TH_ECE;
    }
    th.th_win = htons(sc.sc_win.get() as u16);
    // th_sum already 0
    // th_urp already 0
    mbuf_write(m, hlen, &th_bytes(&th));

    // Tack on the TCP options.
    let mut opt = [0u8; MAX_TCPOPTLEN];
    let mut o = 0usize;
    let ourmaxseg = sc.sc_ourmaxseg.get();
    opt[..4].copy_from_slice(&[TCPOPT_MAXSEG, 4, (ourmaxseg >> 8) as u8, ourmaxseg as u8]);
    o += 4;

    // Include SACK_PERMIT_HDR option if peer has already done so.
    if fixflags & SCF_SACK_PERMIT != 0 {
        opt[o..o + 4].copy_from_slice(&TCPOPT_SACK_PERMIT_HDR.to_be_bytes());
        o += 4;
    }

    if sc.sc_request_r_scale.get() != 15 {
        let w = u32::from(TCPOPT_NOP) << 24
            | u32::from(TCPOPT_WINDOW) << 16
            | u32::from(TCPOLEN_WINDOW) << 8
            | u32::from(sc.sc_request_r_scale.get());
        opt[o..o + 4].copy_from_slice(&w.to_be_bytes());
        o += 4;
    }

    if fixflags & SCF_TIMESTAMP != 0 {
        // Form timestamp option as shown in appendix A of RFC 1323.
        opt[o..o + 4].copy_from_slice(&TCPOPT_TSTAMP_HDR.to_be_bytes());
        let tsval = now.wrapping_add(u64::from(sc.sc_modulate.get())) as u32;
        opt[o + 4..o + 8].copy_from_slice(&tsval.to_be_bytes());
        opt[o + 8..o + 12].copy_from_slice(&(sc.sc_timestamp.get() as u32).to_be_bytes());
        o += usize::from(TCPOLEN_TSTAMP_APPA);
    }
    mbuf_write(m, hlen + TCP_HDR_LEN, &opt[..o]);

    if fixflags & SCF_SIGNATURE != 0 {
        let mut su_src = SockaddrUnion::new();
        let mut su_dst = SockaddrUnion::new();
        su_src.set_sa_len(src.sa_len());
        su_src.set_sa_family(src.sa_family());
        su_dst.set_sa_len(dst.sa_len());
        su_dst.set_sa_family(dst.sa_family());

        match family {
            #[cfg(feature = "inet6")]
            AF_INET6 => {
                let ip6 = mtod_ip6(m);
                let mut sin6 = sa_sin6(&su_src);
                sin6.sin6_addr = ip6.ip6_src;
                su_src = sa_from_sin6(&sin6);
                let mut sin6 = sa_sin6(&su_dst);
                sin6.sin6_addr = ip6.ip6_dst;
                su_dst = sa_from_sin6(&sin6);
            }
            // case 0: default to PF_INET; AF_INET
            _ => {
                let ip = mtod_ip(m);
                let mut sin = su_src.sin();
                sin.sin_addr = ip.ip_src;
                su_src.set_sin(&sin);
                let mut sin = su_dst.sin();
                sin.sin_addr = ip.ip_dst;
                su_dst.set_sin(&sin);
            }
        }

        let Some(tdb) = gettdbbysrcdst(
            rtable_l2(sc.sc_rtableid.get()),
            0,
            &su_src,
            &su_dst,
            IPPROTO_TCP as u8,
        ) else {
            m_freem(m);
            return Err(Errno::EPERM);
        };

        // Send signature option
        opt[o] = TCPOPT_SIGNATURE;
        opt[o + 1] = TCPOLEN_SIGNATURE;
        o += 2;

        let mut sig = [0u8; 16];
        if !tcp_signature(tdb, i32::from(family), m, &th, hlen as i32, false, &mut sig) {
            m_freem(m);
            tdb_unref(Some(tdb));
            return Err(Errno::EINVAL);
        }
        tdb_unref(Some(tdb));
        opt[o..o + 16].copy_from_slice(&sig);
        o += 16;

        // Pad options list to the next 32 bit boundary and terminate it.
        opt[o] = TCPOPT_NOP;
        opt[o + 1] = TCPOPT_EOL;
        o += 2;
        mbuf_write(m, hlen + TCP_HDR_LEN, &opt[..o]);
    }

    m.m_pkthdr()
        .csum_flags
        .set(m.m_pkthdr().csum_flags.get() | M_TCP_CSUM_OUT);

    // use IPsec policy and ttl from listening socket, on SYN ACK
    mtx_enter(&SYN_CACHE_MTX);
    let inp = in_pcbref(sc.sc_inplisten.get());
    mtx_leave(&SYN_CACHE_MTX);

    // Fill in some straggling IP bits. Note the stack expects ip_len to be in host order,
    // for convenience.
    let seclevel = inp.map(|i| i.inp_seclevel.get());
    let error = match family {
        #[cfg(feature = "inet6")]
        AF_INET6 => {
            let mut ip6 = mtod_ip6(m);
            ip6.set_ip6_vfc((ip6.ip6_vfc() & !IPV6_VERSION_MASK) | IPV6_VERSION);
            // ip6_plen will be updated in ip6_output()
            // `in6_selecthlim(NULL)` is `ip6_defhlim`
            ip6.ip6_hlim = match inp {
                Some(i) => in6_selecthlim(i),
                None => IP6_DEFHLIM.load(Ordering::Relaxed),
            } as u8;
            // leave flowlabel = 0, it is legal and require no state mgmt
            mtod_ip6_store(m, &ip6);

            ip6_output(
                m,
                None, /* XXX */
                Some(&sc.sc_route),
                0,
                None,
                seclevel.as_ref(),
            )
        }
        _ => {
            let mut ip = mtod_ip(m);
            ip.ip_len = htons(tlen as u16);
            ip.ip_ttl = match inp {
                Some(i) => i.inp_ip.get().ip_ttl,
                None => IP_DEFTTL.load(Ordering::Relaxed) as u8,
            };
            if let Some(i) = inp {
                ip.ip_tos = i.inp_ip.get().ip_tos;
            }
            mtod_ip_store(m, &ip);

            ip_output(
                m,
                sc.sc_ipopts.get(),
                Some(&sc.sc_route),
                if ip_mtudisc.load(Ordering::Relaxed) != 0 {
                    IP_MTUDISC
                } else {
                    0
                },
                None,
                seclevel.as_ref(),
                0,
            )
        }
    };
    in_pcbunref(inp);
    error
}

/// The TCP header at `tcp`, read unaligned.
///
/// # Safety
///
/// `tcp` points at a TCP header inside a live mbuf (an `ether_extract_headers` result).
unsafe fn softlro_th(tcp: *const u8) -> Tcphdr {
    // SAFETY: the caller's contract.
    unsafe { tcp.cast::<Tcphdr>().read_unaligned() }
}

/// The `i`th 32-bit word of the TCP options after the header at `tcp`, as it lies in memory.
///
/// # Safety
///
/// `tcp` points at a TCP header inside a live mbuf whose options cover word `i`.
unsafe fn softlro_optword(tcp: *const u8, i: usize) -> u32 {
    // SAFETY: the caller's contract.
    unsafe { tcp.add(TCP_HDR_LEN + 4 * i).cast::<u32>().read_unaligned() }
}

/// The VLAN tag of the header at `evh`, as it lies in memory.
///
/// # Safety
///
/// `evh` points at an `ether_vlan_header` inside a live mbuf.
unsafe fn softlro_evl_tag(evh: *const EtherVlanHeader) -> u16 {
    // SAFETY: the caller's contract.
    unsafe { ptr::addr_of!((*evh).evl_tag).read_unaligned() }
}

/// `tcp_softlro_check`: whether the segment in `m`, whose headers `ext` describes, may be
/// merged at all.
fn tcp_softlro_check(m: &Mbuf, ext: &EtherExtracted) -> bool {
    let csum_flags = m.m_pkthdr().csum_flags.get();

    // Don't merge packets with invalid TCP checksum.
    if csum_flags & M_TCP_CSUM_IN_OK == 0 {
        return false;
    }

    if !ext.ip4.is_null() {
        // Don't merge packets with invalid IP header checksum.
        if csum_flags & M_IPV4_CSUM_IN_OK == 0 {
            return false;
        }

        // Don't merge IPv4 packets with IP options.
        if ext.iphlen as usize != IP_HDR_LEN {
            return false;
        }
    }

    // Check TCP protocol and header.
    if ext.tcp.is_null() {
        return false;
    }

    // Don't merge empty TCP segments.
    if ext.paylen == 0 {
        return false;
    }

    // SAFETY: `ether_extract_headers` found `tcphlen` bytes of TCP header at `ext.tcp`.
    let th = unsafe { softlro_th(ext.tcp) };

    // Just ACK and PUSH TCP flags are allowed.
    if th.th_flags & (TH_ACK | TH_PUSH) != th.th_flags {
        return false;
    }

    // TCP ACK flag has to be set.
    if th.th_flags & TH_ACK == 0 {
        return false;
    }

    // Either no TCP options or timestamp as in RFC 1323 appendix A.
    if ext.tcphlen as usize > TCP_HDR_LEN {
        let optlen = ext.tcphlen as usize - TCP_HDR_LEN;
        let appa = usize::from(TCPOLEN_TSTAMP_APPA);

        // Same logic as in TCP input quick retrieval.
        // SAFETY: the options are `optlen` bytes after the header; the byte at `appa` is read
        // only when `optlen > appa`, the first word only when `optlen >= appa`.
        let bad = unsafe {
            (optlen != appa && (optlen <= appa || *ext.tcp.add(TCP_HDR_LEN + appa) != TCPOPT_EOL))
                || softlro_optword(ext.tcp, 0) != htonl(TCPOPT_TSTAMP_HDR)
        };
        if bad {
            return false;
        }
    }

    true
}

/// `tcp_softlro_compare`: whether `tail` continues `head` (same VLAN, ports, addresses and
/// options, contiguous sequence numbers, not too long together).
fn tcp_softlro_compare(head: &EtherExtracted, tail: &EtherExtracted) -> bool {
    // Don't merge packets inside and outside of VLANs
    if !head.evh.is_null() && !tail.evh.is_null() {
        // SAFETY: `ether_extract_headers` found both VLAN headers inside their mbufs.
        let (htag, ttag) = unsafe { (softlro_evl_tag(head.evh), softlro_evl_tag(tail.evh)) };
        // Don't merge packets of different VLANs
        if evl_vlanoftag(htag) != evl_vlanoftag(ttag) {
            return false;
        }

        // Don't merge packets of different priorities
        if evl_prioftag(htag) != evl_prioftag(ttag) {
            return false;
        }
    } else if !head.evh.is_null() || !tail.evh.is_null() {
        return false;
    }

    // A head whose TCP header was not found cannot match (the C dereferences it; only a
    // segment that passed `tcp_softlro_check` has a non-zero `ph_mss`, so it is found).
    if head.tcp.is_null() || tail.tcp.is_null() {
        return false;
    }
    // SAFETY: both TCP headers were found by `ether_extract_headers` inside their mbufs.
    let (hth, tth) = unsafe { (softlro_th(head.tcp), softlro_th(tail.tcp)) };

    // Check TCP ports.
    if hth.th_sport != tth.th_sport || hth.th_dport != tth.th_dport {
        return false;
    }

    // Check IP header.
    if !head.ip4.is_null() && !tail.ip4.is_null() {
        // SAFETY: `ether_extract_headers` found both IP headers inside their mbufs.
        let (hip, tip) = unsafe { (head.ip4.read_unaligned(), tail.ip4.read_unaligned()) };
        // Check IPv4 addresses.
        if hip.ip_src.s_addr != tip.ip_src.s_addr || hip.ip_dst.s_addr != tip.ip_dst.s_addr {
            return false;
        }

        // Check max. IPv4 length.
        let max_linkhdr = MAX_LINKHDR.load(Ordering::Relaxed) as usize;
        if (head.iplen + tail.iplen) as usize > IP_MAXPACKET.wrapping_sub(max_linkhdr) {
            return false;
        }
    } else if !head.ip6.is_null() && !tail.ip6.is_null() {
        // SAFETY: `ether_extract_headers` found both IPv6 headers inside their mbufs.
        let (hip6, tip6) = unsafe { (head.ip6.read_unaligned(), tail.ip6.read_unaligned()) };
        // Check IPv6 addresses.
        if !in6_are_addr_equal(&hip6.ip6_src, &tip6.ip6_src)
            || !in6_are_addr_equal(&hip6.ip6_dst, &tip6.ip6_dst)
        {
            return false;
        }

        // Check max. IPv6 length.
        let max_linkhdr = MAX_LINKHDR.load(Ordering::Relaxed) as usize;
        if ((head.iplen - head.iphlen) + (tail.iplen - tail.iphlen)) as usize
            > IPV6_MAXPACKET.wrapping_sub(max_linkhdr)
        {
            return false;
        }
    } else {
        // Address family does not match.
        return false;
    }

    // Check for contiguous segments.
    if ntohl(hth.th_seq).wrapping_add(head.paylen) != ntohl(tth.th_seq) {
        return false;
    }

    // Ignore segments with different TCP options.
    if head.tcphlen != tail.tcphlen {
        return false;
    }

    // TCP timestamp options must match, type and length already checked.
    if head.tcphlen as usize > TCP_HDR_LEN {
        // SAFETY: both carry the appendix A timestamp option (`tcp_softlro_check`), three
        // words after the header.
        let (h1, t1, h2, t2) = unsafe {
            (
                softlro_optword(head.tcp, 1),
                softlro_optword(tail.tcp, 1),
                softlro_optword(head.tcp, 2),
                softlro_optword(tail.tcp, 2),
            )
        };

        // Tail timestamps must be more recent.
        if tstmp_lt(ntohl(h1), ntohl(t1)) || tstmp_lt(ntohl(h2), ntohl(t2)) {
            return false;
        }
    }

    true
}

/// `tcp_softlro_concat`: appends the payload of `mtail` to `mhead` and fixes up the head's
/// headers and offload flags.
fn tcp_softlro_concat(
    mhead: &'static Mbuf,
    head: &EtherExtracted,
    mtail: &'static Mbuf,
    tail: &EtherExtracted,
) {
    // Adjust IP header length.
    if !head.ip4.is_null() {
        // SAFETY: `ether_extract_headers` found the head's IP header inside its mbuf.
        unsafe {
            let mut ip = head.ip4.read_unaligned();
            ip.ip_len = htons((head.iplen + tail.paylen) as u16);
            head.ip4.write_unaligned(ip);
        }
    } else if !head.ip6.is_null() {
        // SAFETY: `ether_extract_headers` found the head's IPv6 header inside its mbuf.
        unsafe {
            let mut ip6 = head.ip6.read_unaligned();
            ip6.ip6_plen = htons((head.iplen - head.iphlen + tail.paylen) as u16);
            head.ip6.write_unaligned(ip6);
        }
    }

    // SAFETY: the head's TCP header lies inside its mbuf (`ether_extract_headers`), the
    // tail's too; they are different mbufs.
    unsafe {
        let mut hth = softlro_th(head.tcp);
        let tth = softlro_th(tail.tcp);

        // Combine TCP flags from head and tail.
        if tth.th_flags & TH_PUSH != 0 {
            hth.th_flags |= TH_PUSH;
        }

        // Adjust TCP header.
        hth.th_win = tth.th_win;
        hth.th_ack = tth.th_ack;
        head.tcp.cast::<Tcphdr>().write_unaligned(hth);

        // Use more recent timestamps from tail.
        if head.tcphlen as usize > TCP_HDR_LEN {
            for i in 1..3 {
                let w = softlro_optword(tail.tcp, i);
                head.tcp
                    .add(TCP_HDR_LEN + 4 * i)
                    .cast::<u32>()
                    .write_unaligned(w);
            }
        }
    }

    // Calculate header length of tail packet.
    let mut hdrlen = size_of::<EtherHeader>() as u32;
    if !tail.evh.is_null() {
        hdrlen = size_of::<EtherVlanHeader>() as u32;
    }
    hdrlen += tail.iphlen;
    hdrlen += tail.tcphlen;

    // Skip protocol headers in tail.
    m_adj(mtail, hdrlen as i32);
    mtail.m_flags().set(mtail.m_flags().get() & !M_PKTHDR);

    // Concatenate
    let mut m = mhead;
    while let Some(n) = m.m_next().get() {
        m = n;
    }
    m.m_next().set(Some(mtail));
    mhead
        .m_pkthdr()
        .len
        .set(mhead.m_pkthdr().len.get() + tail.paylen as i32);

    // Flag mbuf as TSO packet with MSS.
    let ph = mhead.m_pkthdr();
    if ph.csum_flags.get() & M_TCP_TSO == 0 {
        // Set CSUM_OUT flags in case of forwarding.
        ph.csum_flags.set(ph.csum_flags.get() | M_TCP_CSUM_OUT);
        // SAFETY: as above, for the checksum fields of the head's headers.
        unsafe {
            let mut hth = softlro_th(head.tcp);
            hth.th_sum = 0;
            head.tcp.cast::<Tcphdr>().write_unaligned(hth);
        }
        if !head.ip4.is_null() {
            ph.csum_flags.set(ph.csum_flags.get() | M_IPV4_CSUM_OUT);
            // SAFETY: as above.
            unsafe {
                let mut ip = head.ip4.read_unaligned();
                ip.ip_sum = 0;
                head.ip4.write_unaligned(ip);
            }
        }

        ph.csum_flags.set(ph.csum_flags.get() | M_TCP_TSO);
        ph.ph_mss.set(head.paylen as u16);
        tcpstat_inc(TcpstatCounters::TcpsInswlro);
        tcpstat_inc(TcpstatCounters::TcpsInpktlro); // count head
    }
    ph.ph_mss.set(max(ph.ph_mss.get(), tail.paylen as u16));
    tcpstat_inc(TcpstatCounters::TcpsInpktlro); // count tail
}

/// `tcp_softlro_glue`: a driver's receive path hands each packet `mtail` of interface `ifp`
/// here before queueing it on `ml`; with `IFXF_LRO` a TCP segment that continues one already
/// on the list is appended to it instead.
pub fn tcp_softlro_glue(ml: &MbufList, mtail: &'static Mbuf, ifp: &Ifnet) {
    'dontmerge: {
        if ifp.if_xflags.get() & IFXF_LRO == 0 {
            break 'dontmerge;
        }

        mtail.m_pkthdr().ph_mss.set(0);

        let mut tail = EtherExtracted::new();
        ether_extract_headers(mtail, &mut tail);

        if !tail.tcp.is_null() {
            // Remove possible ethernet padding at the end.
            let tcpdatalen = tail
                .iplen
                .wrapping_sub(tail.iphlen)
                .wrapping_sub(tail.tcphlen);
            if tcpdatalen < tail.paylen {
                m_adj(mtail, tcpdatalen.wrapping_sub(tail.paylen) as i32);
                tail.paylen = tcpdatalen;
            }
        }

        if !tcp_softlro_check(mtail, &tail) {
            break 'dontmerge;
        }

        mtail.m_pkthdr().ph_mss.set(tail.paylen as u16);

        let mut mhead = ml.ml_head.get();
        while let Some(h) = mhead {
            mhead = h.m_nextpkt().get();
            let hph = h.m_pkthdr();
            let tph = mtail.m_pkthdr();

            // This packet has been checked and was not mergable before.
            if hph.ph_mss.get() == 0 {
                continue;
            }

            // Use RSS hash to skip packets of different connections.
            if hph.csum_flags.get() & M_FLOWID != 0
                && tph.csum_flags.get() & M_FLOWID != 0
                && hph.ph_flowid.get() != tph.ph_flowid.get()
            {
                continue;
            }

            // Don't merge packets inside and outside of VLANs
            let hvlan = h.m_flags().get() & M_VLANTAG != 0;
            if hvlan != (mtail.m_flags().get() & M_VLANTAG != 0) {
                continue;
            }

            if hvlan {
                // Don't merge packets of different VLANs
                if evl_vlanoftag(hph.ether_vtag.get()) != evl_vlanoftag(tph.ether_vtag.get()) {
                    continue;
                }

                // Don't merge packets of different priorities
                if evl_prioftag(hph.ether_vtag.get()) != evl_prioftag(tph.ether_vtag.get()) {
                    continue;
                }
            }

            let mut head = EtherExtracted::new();
            ether_extract_headers(h, &mut head);
            if !tcp_softlro_compare(&head, &tail) {
                continue;
            }

            tcp_softlro_concat(h, &head, mtail, &tail);
            return;
        }
    }
    // dontmerge:
    ml_enqueue(ml, mtail);
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for TCP input: the option parser (SYN options, options after the handshake,
    // malformed lists, a signature where none is expected), sequence subtraction, the receiver's
    // SACK report and the sender's SACK holes, the round-trip timer, the header size,
    // reassembly into a socket, the timestamp and sequence helpers, and the SYN cache (hash,
    // insert, lookup, reset, cleanup).

    use std::boxed::Box;
    use std::{assert, assert_eq};

    use super::*;
    use crate::kern::uipc_socket::{soclose, socreate};
    use crate::kern::uipc_socket2::{solock, sounlock};
    use crate::net::if_::tests::test_packet;
    use crate::netinet::in_pcb::tests::{setup, teardown};
    use crate::netinet::udp_usrreq::udp_init;
    use crate::sys::socket::SOCK_DGRAM;
    use crate::sys::systm::{net_lock_shared, net_unlock_shared};

    /// A control block of a detached internet control block, as `tcp_newtcpcb` leaves it.
    fn tcpcb() -> &'static Tcpcb {
        let inp: &'static Inpcb = Box::leak(Box::new(Inpcb::new(None, None)));
        Box::leak(Box::new(Tcpcb::new(inp)))
    }

    /// A header with `flags` and acknowledgement `ack` (host order, as `tcp_input` leaves it).
    fn th(flags: u8, seq: TcpSeq, ack: TcpSeq) -> Tcphdr {
        Tcphdr {
            th_seq: seq,
            th_ack: ack,
            th_flags: flags,
            ..Tcphdr::default()
        }
    }

    /// The pools the SACK holes and the reassembly queue come from, over fresh memory.
    fn pools() {
        pool_init(
            &SACKHL_POOL,
            size_of::<Sackhole>(),
            0,
            IPL_SOFTNET,
            0,
            "sackhlpl",
            None,
        );
        pool_init(
            &TCPQE_POOL,
            size_of::<Tcpqent>(),
            0,
            IPL_SOFTNET,
            0,
            "tcpqe",
            None,
        );
    }

    /// The options of a SYN: MSS 1460, NOP, window scale 7, SACK permitted, timestamp 1000/0.
    const SYN_OPTIONS: [u8; 20] = [
        TCPOPT_MAXSEG,
        4,
        0x05,
        0xb4, //
        TCPOPT_NOP,
        TCPOPT_WINDOW,
        3,
        7, //
        TCPOPT_SACK_PERMITTED,
        2, //
        TCPOPT_TIMESTAMP,
        10,
        0,
        0,
        0x03,
        0xe8,
        0,
        0,
        0,
        0,
    ];

    #[test]
    fn dooptions_takes_the_syn_options() {
        let _g = crate::kern::uipc_mbuf::tests::setup();
        let tp = tcpcb();
        tp.t_state.set(TCPS_LISTEN);
        tp.sack_enable.set(1);
        let m = test_packet(&[0; 40]);
        let mut oi = TcpOptInfo::default();

        assert!(tcp_dooptions(
            tp,
            &SYN_OPTIONS,
            &th(TH_SYN, 1, 0),
            m,
            20,
            &mut oi,
            0,
            77
        ));
        assert_eq!(oi.maxseg, 1460);
        assert!(oi.ts_present);
        assert_eq!((oi.ts_val, oi.ts_ecr), (1000, 0));
        let want = TF_RCVD_SCALE | TF_SACK_PERMIT | TF_RCVD_TSTMP;
        assert_eq!(tp.t_flags.get() & want, want);
        assert_eq!(tp.requested_s_scale.get(), 7);
        assert_eq!((tp.ts_recent.get(), tp.ts_recent_age.get()), (1000, 77));
        m_freem(m);
    }

    #[test]
    fn dooptions_ignores_syn_options_once_synchronized() {
        let _g = crate::kern::uipc_mbuf::tests::setup();
        let tp = tcpcb();
        tp.t_state.set(TCPS_ESTABLISHED);
        tp.sack_enable.set(1);
        let m = test_packet(&[0; 40]);
        let mut oi = TcpOptInfo::default();

        // A window scale of 20 is also clamped to TCP_MAX_WINSHIFT on a SYN; here it is ignored.
        let mut opts = SYN_OPTIONS;
        opts[7] = 20;
        assert!(tcp_dooptions(
            tp,
            &opts,
            &th(TH_SYN, 1, 0),
            m,
            20,
            &mut oi,
            0,
            77
        ));
        assert_eq!(oi.maxseg, 0);
        // The timestamp is read on any segment, but only a SYN's is remembered.
        assert!(oi.ts_present);
        assert_eq!(tp.t_flags.get(), 0);
        assert_eq!(tp.ts_recent.get(), 0);

        // On a SYN in LISTEN the scale is clamped.
        tp.t_state.set(TCPS_LISTEN);
        assert!(tcp_dooptions(
            tp,
            &opts,
            &th(TH_SYN, 1, 0),
            m,
            20,
            &mut oi,
            0,
            77
        ));
        assert_eq!(tp.requested_s_scale.get(), TCP_MAX_WINSHIFT);
        m_freem(m);
    }

    #[test]
    fn dooptions_stops_at_a_malformed_option() {
        let _g = crate::kern::uipc_mbuf::tests::setup();
        let tp = tcpcb();
        tp.t_state.set(TCPS_LISTEN);
        let m = test_packet(&[0; 40]);

        // A length below 2 ends the walk before the MSS behind it.
        let mut oi = TcpOptInfo::default();
        let opts = [TCPOPT_WINDOW, 1, TCPOPT_MAXSEG, 4, 0x02, 0x00];
        assert!(tcp_dooptions(
            tp,
            &opts,
            &th(TH_SYN, 1, 0),
            m,
            20,
            &mut oi,
            0,
            0
        ));
        assert_eq!(oi.maxseg, 0);

        // A length past the end too.
        let opts = [TCPOPT_MAXSEG, 8, 0x02, 0x00];
        assert!(tcp_dooptions(
            tp,
            &opts,
            &th(TH_SYN, 1, 0),
            m,
            20,
            &mut oi,
            0,
            0
        ));
        assert_eq!(oi.maxseg, 0);

        // A wrong length for the kind is skipped, and EOL ends the list.
        let opts = [
            TCPOPT_MAXSEG,
            3,
            0,
            TCPOPT_MAXSEG,
            4,
            0x02,
            0x18,
            TCPOPT_EOL,
            TCPOPT_MAXSEG,
            4,
            0,
            1,
        ];
        assert!(tcp_dooptions(
            tp,
            &opts,
            &th(TH_SYN, 1, 0),
            m,
            20,
            &mut oi,
            0,
            0
        ));
        assert_eq!(oi.maxseg, 0x218);

        // A lone kind byte at the end.
        let opts = [TCPOPT_NOP, TCPOPT_MAXSEG];
        let mut oi = TcpOptInfo::default();
        assert!(tcp_dooptions(
            tp,
            &opts,
            &th(TH_SYN, 1, 0),
            m,
            20,
            &mut oi,
            0,
            0
        ));
        assert_eq!(oi.maxseg, 0);
        m_freem(m);
    }

    #[test]
    fn dooptions_rejects_an_unexpected_signature() {
        let _g = crate::kern::uipc_mbuf::tests::setup();
        let tp = tcpcb();
        tp.t_state.set(TCPS_ESTABLISHED);
        let m = test_packet(&[0; 40]);
        let mut oi = TcpOptInfo::default();

        let mut opts = [0u8; 18];
        opts[0] = TCPOPT_SIGNATURE;
        opts[1] = TCPOLEN_SIGNATURE;
        assert!(!tcp_dooptions(
            tp,
            &opts,
            &th(TH_ACK, 1, 1),
            m,
            20,
            &mut oi,
            0,
            0
        ));
        // The wrong length is not a signature at all.
        opts[1] = 17;
        assert!(tcp_dooptions(
            tp,
            &opts[..17],
            &th(TH_ACK, 1, 1),
            m,
            20,
            &mut oi,
            0,
            0
        ));
        m_freem(m);
    }

    #[test]
    fn seq_subtract_and_timestamp_helpers_wrap() {
        assert_eq!(tcp_seq_subtract(10, 3), 7);
        assert_eq!(tcp_seq_subtract(3, 10), u64::MAX - 6);
        assert!(tstmp_lt(0xffff_fff0, 0x10));
        assert!(tstmp_geq(0x10, 0xffff_fff0));
        assert!(tstmp_geq(5, 5));
        assert_eq!(seq_min(0xffff_fff0, 0x10), 0xffff_fff0);
        assert_eq!(seq_max(0xffff_fff0, 0x10), 0x10);
    }

    /// The receiver's SACK blocks, `rcv_numsacks` of them.
    fn blocks(tp: &Tcpcb) -> std::vec::Vec<(TcpSeq, TcpSeq)> {
        (0..tp.rcv_numsacks.get() as usize)
            .map(|i| {
                let b = tp.sackblks[i].get();
                (b.start, b.end)
            })
            .collect()
    }

    #[test]
    fn update_sack_list_keeps_the_newest_block_first() {
        let tp = tcpcb();
        tp.rcv_nxt.set(1000);

        // In-order data needs no block.
        tcp_update_sack_list(tp, 1000, 1100);
        assert_eq!(blocks(tp), []);

        tcp_update_sack_list(tp, 2000, 2100);
        assert_eq!(blocks(tp), [(2000, 2100)]);
        tcp_update_sack_list(tp, 3000, 3100);
        assert_eq!(blocks(tp), [(3000, 3100), (2000, 2100)]);
        // Adjacent to the older block: merged, and moved to the front.
        tcp_update_sack_list(tp, 2100, 2200);
        assert_eq!(blocks(tp), [(2000, 2200), (3000, 3100)]);
        // Once rcv_nxt passes a block it is dropped.
        tp.rcv_nxt.set(2200);
        tcp_update_sack_list(tp, 4000, 4100);
        assert_eq!(blocks(tp), [(4000, 4100), (3000, 3100)]);
        // A repeated block moves to the front without growing the list.
        tcp_update_sack_list(tp, 3000, 3100);
        assert_eq!(blocks(tp), [(3000, 3100), (4000, 4100)]);

        // At most MAX_SACK_BLKS are kept: the oldest falls off.
        for i in 0..10u32 {
            tcp_update_sack_list(tp, 10000 + 200 * i, 10100 + 200 * i);
        }
        assert_eq!(blocks(tp).len(), MAX_SACK_BLKS);
        assert_eq!(blocks(tp)[0], (11800, 11900));

        tcp_clean_sackreport(tp);
        assert_eq!(blocks(tp), []);
        assert!(tp.sackblks.iter().all(|b| b.get() == Sackblk::default()));
    }

    /// The sender's holes, in list order.
    fn holes(tp: &Tcpcb) -> std::vec::Vec<(TcpSeq, TcpSeq, i32, TcpSeq)> {
        let mut v = std::vec::Vec::new();
        let mut cur = tp.snd_holes.get();
        while let Some(h) = cur {
            v.push((h.start.get(), h.end.get(), h.dups.get(), h.rxmit.get()));
            cur = h.next.get();
        }
        v
    }

    /// A SACK option holding `blocks`.
    fn sack_option(blocks: &[(TcpSeq, TcpSeq)]) -> std::vec::Vec<u8> {
        let mut o = std::vec![TCPOPT_SACK, (2 + 8 * blocks.len()) as u8];
        for &(s, e) in blocks {
            o.extend_from_slice(&s.to_be_bytes());
            o.extend_from_slice(&e.to_be_bytes());
        }
        o
    }

    #[test]
    fn sack_option_tracks_the_holes() {
        let _g = crate::kern::uipc_mbuf::tests::setup();
        pools();
        let tp = tcpcb();
        tp.sack_enable.set(1);
        tp.t_state.set(TCPS_ESTABLISHED);
        tp.snd_una.set(1000);
        tp.snd_max.set(10000);
        tp.t_maxseg.set(100);
        let ack = th(TH_ACK, 1, 1000);

        // A SACK without ACK, or with a malformed length, is ignored.
        tcp_sack_option(tp, &th(0, 1, 1000), &sack_option(&[(2000, 3000)]));
        tcp_sack_option(tp, &ack, &sack_option(&[(2000, 3000)])[..9]);
        assert_eq!(holes(tp), []);

        // The first block opens a hole from the ACK up to it.
        tcp_sack_option(tp, &ack, &sack_option(&[(2000, 3000)]));
        assert_eq!(holes(tp), [(1000, 2000, 3, 1000)]);
        assert_eq!(tp.rcv_lastsack.get(), 3000);
        // A block past the last one appends a hole.
        tcp_sack_option(tp, &ack, &sack_option(&[(4000, 4100)]));
        // The hole before it counts one more dup, capped at tcprexmtthresh.
        assert_eq!(holes(tp), [(1000, 2000, 3, 1000), (3000, 4000, 1, 3000)]);
        assert_eq!(tp.snd_numholes.get(), 2);
        // The beginning of the first hole arrives, and the middle of the second splits it.
        tcp_sack_option(tp, &ack, &sack_option(&[(1000, 1500), (3400, 3600)]));
        assert_eq!(
            holes(tp),
            [
                (1500, 2000, 3, 1500),
                (3000, 3400, 2, 3000),
                (3600, 4000, 1, 3600)
            ]
        );
        assert_eq!(tp.snd_numholes.get(), 3);
        // A block covering a whole hole deletes it.
        tcp_sack_option(tp, &ack, &sack_option(&[(3000, 3400)]));
        assert_eq!(holes(tp), [(1500, 2000, 3, 1500), (3600, 4000, 1, 3600)]);

        // A cumulative ACK deletes the holes below it and trims the one it falls into.
        tcp_del_sackholes(tp, &th(TH_ACK, 1, 3700));
        assert_eq!(holes(tp), [(3700, 4000, 1, 3700)]);
        assert_eq!(tp.snd_numholes.get(), 1);
        tcp_del_sackholes(tp, &th(TH_ACK, 1, 4000));
        assert_eq!(holes(tp), []);
        assert_eq!(tp.snd_numholes.get(), 0);
    }

    #[test]
    fn xmit_timer_smooths_the_rtt() {
        let tp = tcpcb();
        tp.t_rxtshift.set(3);
        tp.t_softerror.set(Some(Errno::EHOSTUNREACH));

        // The first sample: srtt = (rtt + 1) << 5, rttvar = (rtt + 1) << 3.
        tcp_xmit_timer(tp, 100);
        assert_eq!((tp.t_srtt.get(), tp.t_rttvar.get()), (101 << 5, 101 << 3));
        assert_eq!(tp.t_rxtcur.get(), ((404 + 808) >> 2));
        assert_eq!(tp.t_rxtshift.get(), 0);
        assert_eq!(tp.t_softerror.get(), None);

        // The second: delta = 800 - 404 = 396, rttvar += 396 - 202.
        tcp_xmit_timer(tp, 200);
        assert_eq!(
            (tp.t_srtt.get(), tp.t_rttvar.get()),
            (3232 + 396, 808 + 194)
        );
        assert_eq!(tp.t_rxtcur.get(), (((3232 + 396) >> 3) + 1002) >> 2);

        // Negative samples count as 0; the timeout never drops below rtt + 2 ticks.
        let tp = tcpcb();
        tcp_xmit_timer(tp, -5);
        assert_eq!(tp.t_srtt.get(), 1 << 5);
        assert_eq!(tp.t_rxtcur.get(), 2 * (tcp_time(1) / HZ));
        // Huge samples are clamped to TCP_RTT_MAX, the timeout to TCPTV_REXMTMAX.
        let tp = tcpcb();
        tcp_xmit_timer(tp, i32::MAX);
        assert_eq!(tp.t_srtt.get(), (TCP_RTT_MAX + 1) << 5);
        assert_eq!(tp.t_rxtcur.get(), TCPTV_REXMTMAX);
    }

    #[test]
    fn hdrsz_counts_the_options_sent_on_every_segment() {
        let tp = tcpcb();
        tp.pf.set(i32::from(AF_INET));
        assert_eq!(tcp_hdrsz(tp), 40);
        tp.set_flags(TF_REQ_TSTMP | TF_RCVD_TSTMP);
        assert_eq!(tcp_hdrsz(tp), 52);
        tp.set_flags(TF_NOOPT);
        assert_eq!(tcp_hdrsz(tp), 40);
        tp.set_flags(TF_SIGNATURE);
        assert_eq!(tcp_hdrsz(tp), 60);
        tp.pf.set(0);
        assert_eq!(tcp_hdrsz(tp), 40);
    }

    /// A segment of `len` data bytes valued `v`.
    fn segment(len: usize, v: u8) -> &'static Mbuf {
        test_packet(&std::vec![v; len])
    }

    #[test]
    fn reass_queues_out_of_order_data_until_the_hole_fills() {
        let (_g, _t, _p) = setup();
        udp_init();
        pools();
        let so = socreate(i32::from(AF_INET), SOCK_DGRAM, 0).expect("socket");
        let inp = sotoinpcb(so).expect("inpcb");
        let tp: &'static Tcpcb = Box::leak(Box::new(Tcpcb::new(inp)));
        tp.t_state.set(TCPS_ESTABLISHED);
        tp.rcv_nxt.set(1000);
        solock(so);

        // 1010..1020 and 1030..1040 wait for 1000..1010.
        let mut h = th(TH_ACK, 1010, 0);
        let mut len = 10;
        assert_eq!(tcp_reass(tp, &mut h, segment(10, 2), &mut len), Ok(0));
        let mut h = th(TH_ACK | TH_FIN, 1030, 0);
        let mut len = 10;
        assert_eq!(tcp_reass(tp, &mut h, segment(10, 4), &mut len), Ok(0));
        assert_eq!(tp.rcv_nxt.get(), 1000);
        assert_eq!(tp.t_rcvoopack.get(), 2);

        // 1015..1035 overlaps both: its head is trimmed by the earlier segment, and the later
        // one is trimmed in turn to start where it ends.
        let mut h = th(TH_ACK, 1015, 0);
        let mut len = 20;
        assert_eq!(tcp_reass(tp, &mut h, segment(20, 3), &mut len), Ok(0));
        assert_eq!((h.th_seq, len), (1020, 15));
        let seqs: std::vec::Vec<(TcpSeq, u16)> = tp
            .t_segq
            .iter()
            .map(|q| (q.tcpqe_tcp.get().th_seq, q.tcpqe_tcp.get().th_reseqlen()))
            .collect();
        assert_eq!(seqs, [(1010, 10), (1020, 15), (1035, 5)]);

        // A duplicate of queued data is dropped.
        let mut h = th(TH_ACK, 1012, 0);
        let mut len = 5;
        assert_eq!(tcp_reass(tp, &mut h, segment(5, 9), &mut len), Ok(0));
        assert_eq!(tp.t_segq.iter().count(), 3);

        // The missing head flushes everything, up to and with the FIN.
        let mut h = th(TH_ACK, 1000, 0);
        let mut len = 10;
        assert_eq!(tcp_reass(tp, &mut h, segment(10, 1), &mut len), Ok(TH_FIN));
        assert_eq!(tp.rcv_nxt.get(), 1040);
        assert!(tp.t_segq.is_empty());
        assert_eq!(so.so_rcv.sb_cc.get(), 40);
        let mut data = [0u8; 40];
        m_copydata(so.so_rcv.sb_mb.get().expect("data"), 0, &mut data);
        assert_eq!(data[..10], [1; 10]);
        assert_eq!(data[10..20], [2; 10]);
        assert_eq!(data[20..35], [3; 15]);
        assert_eq!(data[35..], [4; 5]);

        sounlock(so);
        soclose(so, 0).expect("close");
        teardown();
    }

    #[test]
    fn reass_waits_in_syn_received_for_data() {
        let (_g, _t, _p) = setup();
        udp_init();
        pools();
        let so = socreate(i32::from(AF_INET), SOCK_DGRAM, 0).expect("socket");
        let inp = sotoinpcb(so).expect("inpcb");
        let tp: &'static Tcpcb = Box::leak(Box::new(Tcpcb::new(inp)));
        tp.rcv_nxt.set(500);
        solock(so);

        // Before ESTABLISHED nothing is presented, even in sequence.
        tp.t_state.set(TCPS_SYN_RECEIVED);
        let mut h = th(TH_ACK, 500, 0);
        let mut len = 8;
        assert_eq!(tcp_reass(tp, &mut h, segment(8, 7), &mut len), Ok(0));
        assert_eq!(tp.rcv_nxt.get(), 500);
        assert_eq!(tcp_flush_queue(tp), 0);

        // tcp_flush_queue presents it once the connection is established.
        tp.t_state.set(TCPS_ESTABLISHED);
        assert_eq!(tcp_flush_queue(tp), 0);
        assert_eq!(tp.rcv_nxt.get(), 508);
        assert_eq!(so.so_rcv.sb_cc.get(), 8);

        sounlock(so);
        soclose(so, 0).expect("close");
        teardown();
    }

    /// `addr:port` as the SYN cache keeps it.
    fn sa(addr: [u8; 4], port: u16) -> SynCacheSa {
        SockaddrUnion::from_sin(&SockaddrIn {
            sin_len: size_of::<SockaddrIn>() as u8,
            sin_family: AF_INET,
            sin_port: port.to_be(),
            sin_addr: crate::netinet::in_::InAddr {
                s_addr: u32::from_ne_bytes(addr),
            },
            ..SockaddrIn::default()
        })
    }

    #[test]
    fn syn_cache_hash_mixes_ports_and_source() {
        let src = sa([10, 0, 0, 1], 1234);
        let dst = sa([10, 0, 0, 2], 80);
        let rand = [0x1111_1111, 2, 3, 4, 0x5555_5555];
        let sport = u32::from(1234u16.to_be());
        let dport = u32::from(80u16.to_be());
        let addr = u32::from_ne_bytes([10, 0, 0, 1]);
        assert_eq!(
            syn_cache_hash(&src, &dst, &rand),
            (((dport << 16).wrapping_add(sport)) ^ rand[4]).wrapping_mul(addr ^ rand[0])
        );
        // Only the source address enters the product.
        let other = sa([10, 0, 0, 3], 80);
        assert_eq!(
            syn_cache_hash(&src, &dst, &rand),
            syn_cache_hash(&src, &other, &rand)
        );
    }

    /// A fresh entry for `src -> dst`, listening on `tp`, as `syn_cache_add` fills it.
    fn entry(
        tp: &'static Tcpcb,
        src: &SynCacheSa,
        dst: &SynCacheSa,
        irs: TcpSeq,
    ) -> &'static SynCache {
        let sc = syn_cache_alloc().expect("syn cache entry");
        refcnt_init_trace(&sc.sc_refcnt, DT_REFCNT_IDX_SYNCACHE);
        timeout_set_flags(
            &sc.sc_timer,
            syn_cache_timer,
            ptr::from_ref(sc).cast_mut().cast(),
            KCLOCK_NONE,
            TIMEOUT_PROC | TIMEOUT_MPSAFE,
        );
        sc.sc_src.set(*src);
        sc.sc_dst.set(*dst);
        sc.sc_irs.set(irs);
        sc.sc_inplisten.set(in_pcbref(Some(tp.t_inpcb)));
        sc
    }

    #[test]
    fn syn_cache_inserts_finds_resets_and_cleans_up() {
        let (_g, _t, _p) = setup();
        udp_init();
        net_lock_shared();
        TCP_SYN_CACHE_ACTIVE.store(0, Ordering::Relaxed);
        for set in &TCP_SYN_CACHE {
            set.scs_count.set(0);
            set.scs_use.set(0);
        }
        syn_cache_init();
        let so = socreate(i32::from(AF_INET), SOCK_DGRAM, 0).expect("socket");
        let inp = sotoinpcb(so).expect("inpcb");
        let tp: &'static Tcpcb = Box::leak(Box::new(Tcpcb::new(inp)));

        let src = sa([10, 0, 0, 1], 1234);
        let dst = sa([10, 0, 0, 2], 80);
        let src2 = sa([10, 0, 0, 1], 1235);

        mtx_enter(&SYN_CACHE_MTX);
        syn_cache_insert(entry(tp, &src, &dst, 5000), tp);
        syn_cache_insert(entry(tp, &src2, &dst, 6000), tp);
        let set = &TCP_SYN_CACHE[0];
        assert_eq!(set.scs_count.get(), 2);
        assert_eq!(
            set.scs_use.get(),
            i64::from(TCP_SYN_USE_LIMIT.load(Ordering::Relaxed)) - 2
        );
        let sc = syn_cache_lookup(&src, &dst, 0).expect("found");
        assert_eq!(sc.sc_irs.get(), 5000);
        assert!(sc.sc_set.get().is_some_and(|s| ptr::eq(s, set)));
        // The retransmit timer starts at the default RTT.
        assert_eq!(sc.sc_rxtcur.get(), TCPTV_SRTTDFLT as u32);
        assert!(syn_cache_lookup(&dst, &src, 0).is_none());
        mtx_leave(&SYN_CACHE_MTX);

        // A RST outside [irs, irs + 1] leaves the entry; one inside removes it.
        syn_cache_reset(&src, &dst, &th(TH_RST, 4000, 0), 0);
        assert_eq!(set.scs_count.get(), 2);
        syn_cache_reset(&src, &dst, &th(TH_RST, 5001, 0), 0);
        assert_eq!(set.scs_count.get(), 1);
        mtx_enter(&SYN_CACHE_MTX);
        assert!(syn_cache_lookup(&src, &dst, 0).is_none());
        assert!(syn_cache_lookup(&src2, &dst, 0).is_some());
        mtx_leave(&SYN_CACHE_MTX);

        // The listener going away takes its entries along.
        syn_cache_cleanup(tp);
        assert_eq!(set.scs_count.get(), 0);
        assert!(tp.t_sc.is_empty());

        net_unlock_shared();
        soclose(so, 0).expect("close");
        teardown();
    }
}
/* </TESTS> */
