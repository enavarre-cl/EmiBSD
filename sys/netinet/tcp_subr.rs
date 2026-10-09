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
//! TCP support routines: `netinet/tcp_subr.c`.
//!
//! Upstream: sys/netinet/tcp_subr.c @ 3ce1f3f79392
//!
//! `tcp_init` sets up the pools, `tcbtable`, the statistics, the ISN secret and the SYN
//! cache. `tcp_template` builds the skeletal TCP/IP header a connection sends with;
//! `tcp_respond` sends a bare segment (an ACK, a RST, a keepalive probe) either from a
//! connection's template or in reply to a received segment. `tcp_newtcpcb`, `tcp_drop`,
//! `tcp_close` and `tcp_freeq` create and tear down control blocks. `tcp_ctlinput` takes
//! ICMP errors (path MTU discovery through `tcp_mtudisc`, soft errors through `tcp_notify`).
//! `tcp_set_iss_tsm` derives initial sequence numbers and timestamp modulators from a keyed
//! hash of the connection (RFC 1948). With `TCP_SIGNATURE`, `tcp_signature` computes the RFC
//! 2385 MD5 digest and the `tcp_signature_tdb_*` functions are the `XF_TCPSIGNATURE`
//! transform, which only stores the key.
//!
//! Locks used to protect struct members in this file: \[I\] immutable after creation,
//! \[T\] `tcp_timer_mtx` (global tcp timer data structures), \[a\] atomic.
//!
//! ## Deviations
//! - The sysctl-controlled `int`s are `AtomicI32`s (the C reads them with
//!   `atomic_load_int`); `tcpcounters` (`struct cpumem *`) is a static array of atomics;
//!   `tcp_iss` is an `AtomicU32` changed under `tcp_timer_mtx` as in C. `tcp_secret` and
//!   `tcp_secret_ctx` are written once by `tcp_init` (`StaticCell`); before that the context
//!   is the zeroed one the C's global would be.
//! - `tcp_respond`'s `caddr_t template` is a byte slice: the IP header, followed by the TCP
//!   header when `th0` is `None` (the template of a connection). The headers are built as
//!   local copies and written into the new mbuf unaligned. `flags` is the `u8` of
//!   `th_flags`.
//! - `tcp_drop`'s and the notify functions' `int errno` is an `Option<Errno>` (0 is `None`);
//!   `tcp_output`'s call `tcp_mtudisc(inp, -1)` passes `None`, which fails the `errno > 0`
//!   test as -1 does. `tcp_freeq` returns `bool` (the C's 0 or 1).
//! - `tcp_ctlinput` is an `unsafe fn` (`PrCtlinputFn`): it reads the returned IP header and
//!   the first eight bytes of the TCP header (ports and sequence number, all an ICMP error
//!   carries) through the raw argument, and the ICMP message around it through an `IcmpPkt`
//!   view. `tcp_notify`, `tcp_mtudisc` and `tcp_mtudisc_increase` return early for a
//!   control block without a `tcpcb`, where the C would dereference NULL (it never happens:
//!   `tcp_close` clears `inp_ppcb` and detaches under the same socket lock).
//! - `tcp_signature` returns `bool` (`true` for the C's 0); its `char *sig` is the
//!   16-byte digest array; the pseudo header and the TCP header are hashed as the bytes the
//!   C's structures hold, written out field by field (no `unsafe` view of a struct).
//!   `tcp_signature_apply` is the closure handed to `m_apply`.
//! - `INET6` is configured (feature `inet6`): the IPv6 cases of `tcp_init` (the
//!   `max_protohdr`/`MHLEN` checks, `icmp6_mtudisc_callback_register`), `tcp_template`,
//!   `tcp_respond` (its two header pointers are the enum `RespondHdr`), `tcp_newtcpcb` and
//!   `tcp_signature`. `tcp6_ctlinput` and `tcp6_mtudisc_callback` compile always, as
//!   `netinet6` does (`inet6sw` names the former), like `route6_mpath`.
//! - `tcp_respond` without a control block uses `ip6_defhlim`, what the C's
//!   `in6_selecthlim(NULL)` returns (`in6_selecthlim` takes a `&Inpcb` here).

use core::ffi::c_void;
use core::mem::{offset_of, size_of};
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicI32, AtomicU32, AtomicU64, Ordering};

use libkern::StaticCell;

use crate::crypto::md5::{MD5_DIGEST_LENGTH, MD5Final, MD5Init, MD5Update, Md5Ctx};
use crate::crypto::sha2::{SHA512_DIGEST_LENGTH, SHA512Final, SHA512Init, SHA512Update, Sha2Ctx};
use crate::dev::rnd::arc4random_buf;
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_malloc::malloc;
use crate::kern::kern_synch::wakeup;
use crate::kern::subr_pool::{pool_get, pool_init, pool_put, pool_sethardlimit};
use crate::kern::uipc_mbuf::{MAX_LINKHDR, m_apply, m_copydata, m_free, m_freem, m_get, m_gethdr};
use crate::kern::uipc_socket::{sorwakeup, sowwakeup};
use crate::kern::uipc_socket2::{soassertlocked, soisdisconnected};
use crate::machine::intr::IPL_SOFTNET;
use crate::net::if_::unhandled_af;
use crate::net::if_var::Netstack;
use crate::net::route::{RTF_HOST, RTV_MTU};
use crate::net::rtable::rtable_l2;
use crate::netinet::in_::{INADDR_ANY, IPPROTO_DONE, IPPROTO_TCP, SockaddrIn};
use crate::netinet::in_pcb::{
    InpNotifyFn, Inpcb, in_pcbdetach, in_pcbinit, in_pcblookup, in_pcbnotifyall, in_pcbrtchange,
    in_pcbrtentry, in_pcbsolock, in_pcbsounlock, in_pcbunref,
};
use crate::netinet::ip::{Ip, Ippseudo};
use crate::netinet::ip_icmp::{Icmp, IcmpPkt, icmp_mtudisc};
use crate::netinet::ip_input::{INETCTLERRMAP, IP_DEFTTL, ip_mtudisc};
use crate::netinet::ip_ipsp::{IpsecInit, Tdb, Xformsw};
use crate::netinet::ip_output::ip_output;
use crate::netinet::ip_var::{IP_MTUDISC, Ipovly, mtod_ip};
#[cfg(feature = "inet6")]
use crate::netinet::ip6::IPV6_FLOWLABEL_MASK;
use crate::netinet::tcp::{
    TCP_MAX_WINSHIFT, TCP_MAXWIN, TCP_MSS, TCPOLEN_TSTAMP_APPA, TCPOPT_TSTAMP_HDR, TH_ACK, TH_RST,
    TcpSeq, Tcphdr,
};
use crate::netinet::tcp_fsm::{
    TCPS_CLOSED, TCPS_ESTABLISHED, tcps_haveestablished, tcps_havercvdsyn,
};
use crate::netinet::tcp_input::{
    syn_cache_cleanup, syn_cache_init, syn_cache_unreach, tcp_hdrsz, tcp_mss,
};
use crate::netinet::tcp_output::tcp_output;
use crate::netinet::tcp_seq::{seq_geq, seq_lt};
use crate::netinet::tcp_timer::{
    TCPT_NTIMERS, TCPTV_MIN, TCPTV_REXMTMAX, TCPTV_SRTTBASE, TCPTV_SRTTDFLT, tcp_canceltimers,
    tcp_timer_init, tcpt_rangeset,
};
use crate::netinet::tcp_usrreq::{TCB6TABLE, TCBTABLE};
use crate::netinet::tcp_var::{
    Sackhole, TCP_RTT_BASE_SHIFT, TCP_RTTVAR_SHIFT, TCPS_NCOUNTERS, TF_NOOPT, TF_PMTUD_PEND,
    TF_RCVD_TSTMP, TF_REQ_SCALE, TF_REQ_TSTMP, Tcpcb, Tcpqent, TcpstatCounters, intotcpcb,
    tcp_rexmtval, tcpstat_inc,
};
use crate::netinet6::icmp6::icmp6_mtudisc_update;
use crate::netinet6::in6::{
    SA6_ANY, SockaddrIn6, in6_is_addr_unspecified, in6_is_addr_v4mapped, satosin6_const,
    sin6tosa_const,
};
use crate::netinet6::in6_pcb::{in6_pcblookup, in6_pcbnotify};
use crate::netinet6::ip6_input::INET6CTLERRMAP;
use crate::netinet6::ip6protosw::Ip6ctlparam;
use crate::sys::endian::{htonl, htons, ntohl, ntohs};
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_NOWAIT, M_XDATA};
use crate::sys::mbuf::{M_DONTWAIT, M_TCP_CSUM_OUT, M_WAIT, MT_HEADER, Mbuf, m_freemp, mtod};
use crate::sys::mutex::Mutex;
use crate::sys::param::NMBCLUSTERS;
use crate::sys::pool::{PR_NOWAIT, PR_WAITOK, PR_ZERO, Pool};
use crate::sys::protosw::{
    PRC_HOSTDEAD, PRC_MSGSIZE, PRC_MTUINC, PRC_NCMDS, PRC_QUENCH, prc_is_redirect,
};
use crate::sys::socket::{AF_INET, AF_INET6, PF_INET, Sockaddr};
#[cfg(feature = "inet6")]
use crate::{
    kern::uipc_mbuf::MAX_PROTOHDR,
    netinet::in_pcb::INP_IPV6,
    netinet::ip6::{Ip6Hdr, Ip6HdrPseudo},
    netinet6::icmp6::icmp6_mtudisc_callback_register,
    netinet6::in6_proto::IP6_DEFHLIM,
    netinet6::in6_src::{in6_clearscope, in6_selecthlim},
    netinet6::ip6_output::ip6_output,
    netinet6::ip6_var::mtod_ip6,
    sys::mbuf::MHLEN,
    sys::socket::PF_INET6,
};

/// `TCB_INITIAL_HASH_SIZE`.
const TCB_INITIAL_HASH_SIZE: i32 = 128;

/// `AF_INET` as the `int` of `tp->pf` and of the functions' `af` arguments.
const AF_INET_I32: i32 = AF_INET as i32;
/// `AF_INET6` as the `int` of `tp->pf` and of the functions' `af` arguments.
#[cfg(feature = "inet6")]
const AF_INET6_I32: i32 = AF_INET6 as i32;

/// `TCP_ISS_CONN_INC`: the step of `tcp_iss` per connection.
const TCP_ISS_CONN_INC: u32 = 4096;

/// `tcp_timer_mtx`: \[T\] global tcp timer data structures.
pub static TCP_TIMER_MTX: Mutex = Mutex::new(IPL_SOFTNET);

// patchable/settable parameters for tcp

/// \[a\] `tcp_mssdflt`: default maximum segment size.
pub static TCP_MSSDFLT: AtomicI32 = AtomicI32::new(TCP_MSS);
/// `tcp_rttdflt`.
pub static TCP_RTTDFLT: AtomicI32 = AtomicI32::new(TCPTV_SRTTDFLT);

// values controllable via sysctl

/// \[a\] `tcp_do_rfc1323`.
pub static TCP_DO_RFC1323: AtomicI32 = AtomicI32::new(1);
/// \[a\] `tcp_do_sack`: RFC 2018 selective ACKs.
pub static TCP_DO_SACK: AtomicI32 = AtomicI32::new(1);
/// \[a\] `tcp_ack_on_push`: set to enable immediate ACK-on-PUSH.
pub static TCP_ACK_ON_PUSH: AtomicI32 = AtomicI32::new(0);
/// \[a\] `tcp_do_ecn`: RFC3168 ECN enabled/disabled? (`TCP_ECN`).
pub static TCP_DO_ECN: AtomicI32 = AtomicI32::new(0);
/// \[a\] `tcp_do_rfc3390`: increase TCP's Initial Window to 10*mss.
pub static TCP_DO_RFC3390: AtomicI32 = AtomicI32::new(2);
/// \[a\] `tcp_do_tso`: TCP segmentation offload for output.
pub static TCP_DO_TSO: AtomicI32 = AtomicI32::new(1);

/// `tcp_reass_limit`: hardlimit for `tcpqe_pool`.
pub static TCP_REASS_LIMIT: AtomicI32 = AtomicI32::new((NMBCLUSTERS / 8) as i32);
/// `tcp_sackhole_limit`: hardlimit for `sackhl_pool`.
#[allow(non_upper_case_globals)] // TCP_SACKHOLE_LIMIT is a constant of tcp.h
pub static tcp_sackhole_limit: AtomicI32 = AtomicI32::new(32 * 1024);

/// `tcpcb_pool`.
pub static TCPCB_POOL: Pool = Pool::new();
/// `tcpqe_pool`.
pub static TCPQE_POOL: Pool = Pool::new();
/// `sackhl_pool`.
pub static SACKHL_POOL: Pool = Pool::new();

/// `tcpcounters`: tcp statistics.
pub static TCPCOUNTERS: [AtomicU64; TCPS_NCOUNTERS] = [const { AtomicU64::new(0) }; TCPS_NCOUNTERS];

/// \[I\] `tcp_secret`.
static TCP_SECRET: StaticCell<[u8; 16]> = StaticCell::new([0; 16]);
/// \[I\] `tcp_secret_ctx`.
static TCP_SECRET_CTX: StaticCell<Option<Sha2Ctx>> = StaticCell::new(None);
/// \[T\] `tcp_iss`: updated by timer and connection.
pub static TCP_ISS: AtomicU32 = AtomicU32::new(0);
/// \[I\] `tcp_starttime`: random offset for `tcp_now()`.
pub static TCP_STARTTIME: AtomicU64 = AtomicU64::new(0);

/// `tcp_init`: TCP initialization.
pub fn tcp_init() {
    TCP_ISS.store(1, Ordering::Relaxed); // wrong
    // 0 is treated special so add 1, 63 bits to count is enough
    let mut start = [0u8; 8];
    arc4random_buf(&mut start);
    TCP_STARTTIME.store(1 + u64::from_ne_bytes(start) / 2, Ordering::Relaxed);
    pool_init(
        &TCPCB_POOL,
        size_of::<Tcpcb>(),
        0,
        IPL_SOFTNET,
        0,
        "tcpcb",
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
    // The C ignores the result, as below: the limit only caps the pool.
    let _ = pool_sethardlimit(&TCPQE_POOL, TCP_REASS_LIMIT.load(Ordering::Relaxed) as u32);
    pool_init(
        &SACKHL_POOL,
        size_of::<Sackhole>(),
        0,
        IPL_SOFTNET,
        0,
        "sackhl",
        None,
    );
    let _ = pool_sethardlimit(
        &SACKHL_POOL,
        tcp_sackhole_limit.load(Ordering::Relaxed) as u32,
    );
    in_pcbinit(&TCBTABLE, TCB_INITIAL_HASH_SIZE);
    #[cfg(feature = "inet6")]
    in_pcbinit(&TCB6TABLE, TCB_INITIAL_HASH_SIZE);
    // tcpcounters = counters_alloc(tcps_ncounters): a static array of atomics.

    // SAFETY: `tcp_init` runs once, from `domaininit` at boot, before any connection reads
    // the secret; nothing else writes it.
    unsafe {
        let secret = TCP_SECRET.get_mut();
        arc4random_buf(secret);
        let mut ctx = Sha2Ctx::default();
        SHA512Init(&mut ctx);
        SHA512Update(&mut ctx, secret);
        *TCP_SECRET_CTX.get_mut() = Some(ctx);
    }

    #[cfg(feature = "inet6")]
    {
        // Since sizeof(struct ip6_hdr) > sizeof(struct ip), we do max length
        // checks/computations only on the former.
        let hdrs = (size_of::<Ip6Hdr>() + size_of::<Tcphdr>()) as i32;
        if MAX_PROTOHDR.load(Ordering::Relaxed) < hdrs {
            MAX_PROTOHDR.store(hdrs, Ordering::Relaxed);
        }
        if (MAX_LINKHDR.load(Ordering::Relaxed) + hdrs) as usize > MHLEN {
            crate::kern::subr_prf::panic(format_args!("tcp_init"));
        }

        icmp6_mtudisc_callback_register(tcp6_mtudisc_callback);
    }

    // Initialize the compressed state engine.
    syn_cache_init();
}

/// Writes `v` at byte `off` of `m`'s data, unaligned.
///
/// # Safety
///
/// `m`'s first mbuf has `off + size_of::<T>()` bytes of storage at its data pointer.
unsafe fn mbuf_put<T>(m: &Mbuf, off: usize, v: T) {
    // SAFETY: the caller's contract.
    unsafe { ptr::write_unaligned(mtod::<u8>(m).add(off).cast::<T>(), v) };
}

/// Reads a `T` from the front of `bytes`, unaligned.
fn read_from<T: Copy>(bytes: &[u8]) -> T {
    if bytes.len() < size_of::<T>() {
        crate::kern::subr_prf::panic(format_args!(
            "tcp: template of {} bytes, {} needed",
            bytes.len(),
            size_of::<T>()
        ));
    }
    // SAFETY: at least `size_of::<T>()` bytes (checked); the headers read here are integers,
    // valid for any bytes.
    unsafe { ptr::read_unaligned(bytes.as_ptr().cast::<T>()) }
}

/// `tcp_template`: create template to be used to send tcp packets on a connection. Call
/// after host entry created, allocates an mbuf and fills in a skeletal tcp/ip header,
/// minimizing the amount of work necessary when the connection is used.
///
/// To support IPv6 in addition to IPv4 and considering that the sizes of the IPv4 and IPv6
/// headers are not the same, we now use a separate pointer for the TCP header. Also, we made
/// the former tcpiphdr header pointer into just an IP overlay pointer, with casting as
/// appropriate for v6. rja
pub fn tcp_template(tp: &Tcpcb) -> Option<&'static Mbuf> {
    let inp = tp.t_inpcb;

    let m = match tp.t_template.get() {
        Some(m) => m,
        None => {
            let m = m_get(M_DONTWAIT, MT_HEADER)?;
            match tp.pf.get() {
                // default to PF_INET
                0 | AF_INET_I32 => m.m_len().set(size_of::<Ip>() as u32),
                #[cfg(feature = "inet6")]
                AF_INET6_I32 => m.m_len().set(size_of::<Ip6Hdr>() as u32),
                _ => {}
            }
            m.m_len().set(m.m_len().get() + size_of::<Tcphdr>() as u32);
            m
        }
    };

    let thoff = match tp.pf.get() {
        AF_INET_I32 => {
            let ipovly = Ipovly {
                ih_x1: [0; 9],
                ih_pr: IPPROTO_TCP as u8,
                ih_len: htons(size_of::<Tcphdr>() as u16),
                ih_src: inp.inp_laddr.get(),
                ih_dst: inp.inp_faddr.get(),
            };
            // SAFETY: an `MT_HEADER` mbuf holds `MLEN` bytes, more than an IP and a TCP
            // header (the C's CTASSERT).
            unsafe { mbuf_put(m, 0, ipovly) };
            size_of::<Ip>()
        }
        #[cfg(feature = "inet6")]
        AF_INET6_I32 => {
            let ip6 = Ip6Hdr {
                ip6_src: inp.inp_laddr6.get(),
                ip6_dst: inp.inp_faddr6.get(),
                ip6_flow: htonl(0x6000_0000) | (inp.inp_flowinfo() & IPV6_FLOWLABEL_MASK),
                ip6_nxt: IPPROTO_TCP as u8,
                ip6_plen: htons(size_of::<Tcphdr>() as u16), // XXX
                ip6_hlim: in6_selecthlim(inp) as u8,         // XXX
            };
            // SAFETY: an `MT_HEADER` mbuf holds `MLEN` bytes, more than an IPv6 and a TCP
            // header (the C's CTASSERT).
            unsafe { mbuf_put(m, 0, ip6) };
            size_of::<Ip6Hdr>()
        }
        pf => unhandled_af(pf),
    };

    let mut th = Tcphdr {
        th_sport: inp.inp_lport.get(),
        th_dport: inp.inp_fport.get(),
        ..Tcphdr::default()
    };
    th.set_th_x2(0);
    th.set_th_off(5);
    // SAFETY: as above.
    unsafe { mbuf_put(m, thoff, th) };
    Some(m)
}

/// The network header `tcp_respond` builds: the C's `ip` or `ip6` pointer into the mbuf.
enum RespondHdr {
    /// `struct ip`.
    V4(Ip),
    /// `struct ip6_hdr` (`INET6`).
    #[cfg(feature = "inet6")]
    V6(Ip6Hdr),
}

/// `tcp_respond`: send a single message to the TCP at address specified by the given TCP/IP
/// header. If `th0` is `None`, then we make a copy of the tcpiphdr at `template` and send
/// directly to the addressed host. This is used to force keep alive messages out using the
/// TCP template for a connection `tp->t_template`. If `th0` is given then we send a message
/// back to the TCP which originated the segment, and discard the mbuf containing it and any
/// other attached mbufs.
///
/// In any case the ack and sequence number of the transmitted segment are as specified by
/// the parameters.
#[allow(clippy::too_many_arguments)]
pub fn tcp_respond(
    tp: Option<&'static Tcpcb>,
    template: &[u8],
    th0: Option<&Tcphdr>,
    ack: TcpSeq,
    seq: TcpSeq,
    flags: u8,
    rtableid: u32,
    now: u64,
) {
    let mut flags = flags;
    let mut win: i64 = 0;
    // af on wire
    let af = match tp {
        Some(tp) => {
            let so = tp.socket();
            win = crate::sys::socketvar::sbspace(&so.so_rcv);
            // If this is called with an unconnected socket/tp/pcb (tp->pf is 0), we lose.
            tp.pf.get()
        }
        None => {
            let v = read_from::<Ip>(template).ip_v();
            if v == 6 {
                i32::from(AF_INET6)
            } else {
                AF_INET_I32
            }
        }
    };

    let Some(m) = m_gethdr(M_DONTWAIT, MT_HEADER) else {
        return;
    };
    m.m_data().set(
        m.m_data()
            .get()
            .wrapping_add(MAX_LINKHDR.load(Ordering::Relaxed) as usize),
    );

    let (nh, mut th, mut tlen, thoff) = match af {
        #[cfg(feature = "inet6")]
        AF_INET6_I32 => {
            let tlen = size_of::<Ip6Hdr>() + size_of::<Tcphdr>();
            let mut ip6: Ip6Hdr = read_from(template);
            let th = match th0 {
                Some(th0) => {
                    core::mem::swap(&mut ip6.ip6_dst, &mut ip6.ip6_src);
                    *th0
                }
                None => read_from(&template[size_of::<Ip6Hdr>().min(template.len())..]),
            };
            (RespondHdr::V6(ip6), th, tlen, size_of::<Ip6Hdr>())
        }
        AF_INET_I32 => {
            let tlen = size_of::<Ip>() + size_of::<Tcphdr>();
            let mut ip: Ip = read_from(template);
            let th = match th0 {
                Some(th0) => {
                    core::mem::swap(&mut ip.ip_dst.s_addr, &mut ip.ip_src.s_addr);
                    *th0
                }
                None => read_from(&template[size_of::<Ip>().min(template.len())..]),
            };
            (RespondHdr::V4(ip), th, tlen, size_of::<Ip>())
        }
        af => unhandled_af(af),
    };
    if th0.is_some() {
        core::mem::swap(&mut th.th_dport, &mut th.th_sport);
    } else {
        flags = TH_ACK;
    }

    th.th_seq = htonl(seq);
    th.th_ack = htonl(ack);
    th.set_th_x2(0);
    th.set_th_off((size_of::<Tcphdr>() >> 2) as u8);
    th.th_flags = flags;
    if let Some(tp) = tp {
        win >>= tp.rcv_scale.get();
    }
    if win > i64::from(TCP_MAXWIN) {
        win = i64::from(TCP_MAXWIN);
    }
    th.th_win = htons(win as u16);
    th.th_urp = 0;

    let ipoff = 0;
    if let Some(tp) = tp
        && tp.t_flags.get() & (TF_REQ_TSTMP | TF_NOOPT) == TF_REQ_TSTMP
        && flags & TH_RST == 0
        && tp.has_flags(TF_RCVD_TSTMP)
    {
        // Form timestamp option as shown in appendix A of RFC 1323.
        let lp = thoff + size_of::<Tcphdr>();
        let words = [
            htonl(TCPOPT_TSTAMP_HDR),
            htonl((now as u32).wrapping_add(tp.ts_modulate.get())),
            htonl(tp.ts_recent.get()),
        ];
        // SAFETY: a packet header mbuf holds `MHLEN` bytes after `max_linkhdr`, more than
        // the headers and the 12 option bytes.
        unsafe { mbuf_put(m, lp, words) };
        tlen += usize::from(TCPOLEN_TSTAMP_APPA);
        th.set_th_off(((size_of::<Tcphdr>() + usize::from(TCPOLEN_TSTAMP_APPA)) >> 2) as u8);
    }

    m.m_len().set(tlen as u32);
    let ph = m.m_pkthdr();
    ph.len.set(tlen as i32);
    ph.ph_ifidx.set(0);
    ph.csum_flags.set(ph.csum_flags.get() | M_TCP_CSUM_OUT);

    // force routing table
    match tp {
        Some(tp) => ph.ph_rtableid.set(tp.t_inpcb.inp_rtableid.get()),
        None => ph.ph_rtableid.set(rtableid),
    }

    let seclevel = tp.map(|tp| tp.t_inpcb.inp_seclevel.get());
    match nh {
        #[cfg(feature = "inet6")]
        RespondHdr::V6(mut ip6) => {
            ip6.ip6_flow = htonl(0x6000_0000);
            ip6.ip6_nxt = IPPROTO_TCP as u8;
            // XXX; `in6_selecthlim(NULL)` is `ip6_defhlim`
            ip6.ip6_hlim = match tp {
                Some(tp) => in6_selecthlim(tp.t_inpcb),
                None => IP6_DEFHLIM.load(Ordering::Relaxed),
            } as u8;
            ip6.ip6_plen = htons((tlen - size_of::<Ip6Hdr>()) as u16);
            // SAFETY: as for the option above.
            unsafe {
                mbuf_put(m, ipoff, ip6);
                mbuf_put(m, thoff, th);
            }
            let opts = tp.and_then(|tp| tp.t_inpcb.inp_outputopts6.get()).map(|o| {
                // SAFETY: the options are the socket's own allocation (`ip6_setpktopts`),
                // freed only under the socket lock the caller holds with `tp`.
                unsafe { o.as_ref() }
            });
            // The C ignores the result: a lost reply is a lost segment.
            let _ = ip6_output(
                m,
                opts,
                tp.map(|tp| &tp.t_inpcb.inp_route),
                0,
                None,
                seclevel.as_ref(),
            );
        }
        RespondHdr::V4(mut ip) => {
            ip.ip_len = htons(tlen as u16);
            ip.ip_ttl = IP_DEFTTL.load(Ordering::Relaxed) as u8;
            ip.ip_tos = 0;
            // SAFETY: as for the option above.
            unsafe {
                mbuf_put(m, ipoff, ip);
                mbuf_put(m, thoff, th);
            }
            // The C ignores the result: a lost reply is a lost segment.
            let _ = ip_output(
                m,
                None,
                tp.map(|tp| &tp.t_inpcb.inp_route),
                if ip_mtudisc.load(Ordering::Relaxed) != 0 {
                    IP_MTUDISC
                } else {
                    0
                },
                None,
                seclevel.as_ref(),
                0,
            );
        }
    }
}

/// `tcp_newtcpcb`: create a new TCP control block, making an empty reassembly queue and
/// hooking it to the argument protocol control block.
pub fn tcp_newtcpcb(inp: &'static Inpcb, wait: i32) -> Option<&'static Tcpcb> {
    let mem = pool_get(
        &TCPCB_POOL,
        (if wait == M_WAIT { PR_WAITOK } else { PR_NOWAIT }) | PR_ZERO,
    )?;
    let raw = mem.cast::<Tcpcb>().as_ptr();
    // SAFETY: a fresh, suitably aligned `tcpcb_pool` item, written once before anything else
    // sees it.
    unsafe { raw.write(Tcpcb::new(inp)) };
    // SAFETY: as above; the item stays allocated until `tcp_close` puts it back.
    let tp: &'static Tcpcb = unsafe { &*raw };
    tp.t_segq.init();
    tp.t_maxseg.set(TCP_MSSDFLT.load(Ordering::Relaxed) as u16);
    tp.t_maxopd.set(0);

    for i in 0..TCPT_NTIMERS {
        tcp_timer_init(tp, i);
    }

    tp.sack_enable.set(TCP_DO_SACK.load(Ordering::Relaxed));
    tp.t_flags
        .set(if TCP_DO_RFC1323.load(Ordering::Relaxed) != 0 {
            TF_REQ_SCALE | TF_REQ_TSTMP
        } else {
            0
        });
    // Init srtt to TCPTV_SRTTBASE (0), so we can tell that we have no rtt estimate. Set
    // rttvar so that srtt + 2 * rttvar gives reasonable initial retransmit time.
    tp.t_srtt.set(TCPTV_SRTTBASE);
    tp.t_rttvar
        .set(TCP_RTTDFLT.load(Ordering::Relaxed) << (TCP_RTTVAR_SHIFT + TCP_RTT_BASE_SHIFT - 1));
    tp.t_rttmin.set(TCPTV_MIN as u32);
    tp.t_rxtcur
        .set(tcpt_rangeset(tcp_rexmtval(tp), TCPTV_MIN, TCPTV_REXMTMAX));
    tp.snd_cwnd.set(u64::from(TCP_MAXWIN) << TCP_MAX_WINSHIFT);
    tp.snd_ssthresh
        .set(u64::from(TCP_MAXWIN) << TCP_MAX_WINSHIFT);

    tp.t_pmtud_mtu_sent.set(0);
    tp.t_pmtud_mss_acked.set(0);

    #[cfg(feature = "inet6")]
    let inet6 = inp.has_flags(INP_IPV6);
    #[cfg(not(feature = "inet6"))]
    let inet6 = false;
    if inet6 {
        #[cfg(feature = "inet6")]
        {
            tp.pf.set(i32::from(PF_INET6));
            let mut ip6 = inp.inp_ipv6.get();
            ip6.ip6_hlim = IP6_DEFHLIM.load(Ordering::Relaxed) as u8;
            inp.inp_ipv6.set(ip6);
        }
    } else {
        tp.pf.set(i32::from(PF_INET));
        let mut ip = inp.inp_ip.get();
        ip.ip_ttl = IP_DEFTTL.load(Ordering::Relaxed) as u8;
        inp.inp_ip.set(ip);
    }

    inp.inp_ppcb.set(raw.cast());
    Some(tp)
}

/// `tcp_drop`: drop a TCP connection, reporting the specified error. If connection is
/// synchronized, then send a RST to peer.
pub fn tcp_drop(tp: &'static Tcpcb, errno: Option<Errno>) -> Option<&'static Tcpcb> {
    let so = tp.socket();

    if tcps_havercvdsyn(tp.t_state.get()) {
        tp.t_state.set(TCPS_CLOSED);
        // The RST is best effort, as the C's (void).
        let _ = tcp_output(tp);
        tcpstat_inc(TcpstatCounters::TcpsDrops);
    } else {
        tcpstat_inc(TcpstatCounters::TcpsConndrops);
    }
    let mut errno = errno;
    if errno == Some(Errno::ETIMEDOUT) && tp.t_softerror.get().is_some() {
        errno = tp.t_softerror.get();
    }
    so.set_error(errno);
    tcp_close(tp)
}

/// `tcp_close`: close a TCP control block: discard all space held by the tcp, discard
/// internet protocol block, wake up any sleepers. Always `None`.
pub fn tcp_close(tp: &'static Tcpcb) -> Option<&'static Tcpcb> {
    let inp = tp.t_inpcb;
    let so = inp.socket();

    // free the reassembly queue, if any
    tcp_freeq(tp);

    tcp_canceltimers(tp);
    syn_cache_cleanup(tp);

    // Free SACK holes.
    let mut p = tp.snd_holes.get();
    while let Some(h) = p {
        p = h.next.get();
        pool_put(&SACKHL_POOL, NonNull::from(h).cast());
    }

    let _ = m_free(tp.t_template.get());
    inp.inp_ppcb.set(ptr::null_mut());
    pool_put(&TCPCB_POOL, NonNull::from(tp).cast());
    soisdisconnected(so);
    in_pcbdetach(inp);
    tcpstat_inc(TcpstatCounters::TcpsClosed);
    None
}

/// `tcp_freeq`: frees the reassembly queue; `true` when it held a segment.
pub fn tcp_freeq(tp: &Tcpcb) -> bool {
    let mut rv = false;

    while let Some(qe) = tp.t_segq.first() {
        // SAFETY: the entry is on this queue; the socket lock serialises the queue.
        unsafe { tp.t_segq.remove(qe) };
        m_freem(qe.tcpqe_m.get());
        pool_put(&TCPQE_POOL, NonNull::from(qe).cast());
        rv = true;
    }
    rv
}

/// `tcp_rscale`: compute proper scaling value for receiver window from buffer space.
pub fn tcp_rscale(tp: &Tcpcb, hiwat: u64) {
    tp.request_r_scale.set(0);
    while tp.request_r_scale.get() < TCP_MAX_WINSHIFT
        && u64::from(TCP_MAXWIN) << tp.request_r_scale.get() < hiwat
    {
        tp.request_r_scale.set(tp.request_r_scale.get() + 1);
    }
}

/// `tcp_notify`: notify a tcp user of an asynchronous error; store error as soft error, but
/// wake up user (for now, won't do anything until can select for soft error).
pub fn tcp_notify(inp: &'static Inpcb, error: Option<Errno>) {
    let Some(tp) = intotcpcb(inp) else {
        return;
    };
    let so = inp.socket();

    soassertlocked(so);

    // Ignore some errors if we are hooked up. If connection hasn't completed, has
    // retransmitted several times, and receives a second error, give up now. This is better
    // than waiting a long time to establish a connection that can never complete.
    if tp.t_state.get() == TCPS_ESTABLISHED
        && matches!(
            error,
            Some(Errno::EHOSTUNREACH | Errno::ENETUNREACH | Errno::EHOSTDOWN)
        )
    {
        return;
    } else if !tcps_haveestablished(tp.t_state.get())
        && tp.t_rxtshift.get() > 3
        && tp.t_softerror.get().is_some()
    {
        so.set_error(error);
    } else {
        tp.t_softerror.set(error);
    }
    wakeup(so.timeo_chan());
    sorwakeup(so);
    sowwakeup(so);
}

/// `tcp6_ctlinput`: an ICMPv6 error about a segment we sent; `d` is the `Ip6ctlparam` of
/// `icmp6_notify_error` (or NULL). `INET6`; compiled always, as `netinet6` is, for
/// `inet6sw`.
///
/// # Safety
///
/// `sa` points to a readable socket address of its `sa_len` bytes; `d` is NULL or the
/// `Ip6ctlparam` of the ICMPv6 error, valid for the call.
pub unsafe fn tcp6_ctlinput(cmd: i32, sa: *const Sockaddr, rdomain: u32, d: *mut c_void) {
    let mut d = d;
    let mut notify: InpNotifyFn = tcp_notify;

    // SAFETY: the caller's contract: a readable socket address.
    let (family, len) = unsafe { ((*sa).sa_family, (*sa).sa_len) };
    if family != AF_INET6 || usize::from(len) != size_of::<SockaddrIn6>() {
        return;
    }
    // SAFETY: a whole `sockaddr_in6` (checked), read unaligned.
    let sa6 = unsafe { satosin6_const(sa).read_unaligned() };
    if in6_is_addr_unspecified(&sa6.sin6_addr) || in6_is_addr_v4mapped(&sa6.sin6_addr) {
        return;
    }
    if cmd as u32 as usize >= PRC_NCMDS {
        return;
    } else if cmd == PRC_QUENCH {
        // Don't honor ICMP Source Quench messages meant for TCP connections.
        // XXX there's no PRC_QUENCH in IPv6
        return;
    } else if prc_is_redirect(cmd) {
        notify = in_pcbrtchange;
        d = ptr::null_mut();
    } else if cmd == PRC_MSGSIZE {
        // special code is present, see below
    } else if cmd == PRC_HOSTDEAD {
        d = ptr::null_mut();
    } else if INET6CTLERRMAP[cmd as usize].is_none() {
        return;
    }

    // if the parameter is from icmp6, decode it.
    let ip6cp: Option<&Ip6ctlparam> = if d.is_null() {
        None
    } else {
        // SAFETY: the caller's contract: a non-NULL `d` is the ICMPv6 error's parameter.
        Some(unsafe { &*d.cast::<Ip6ctlparam>() })
    };
    let (m, ip6, off, sa6_src) = match ip6cp {
        Some(p) => (
            p.ip6c_m,
            p.ip6c_ip6,
            p.ip6c_off,
            // SAFETY: `icmp6_notify_error` points `ip6c_src` at its source address, valid
            // for the call; read unaligned.
            (!p.ip6c_src.is_null()).then(|| unsafe { p.ip6c_src.read_unaligned() }),
        ),
        None => (None, ptr::null_mut(), 0, Some(SA6_ANY)),
    };
    let sa6_src = sa6_src.unwrap_or(SA6_ANY);

    if !ip6.is_null() {
        // XXX: We assume that when ip6 is non NULL, M and OFF are valid.
        let Some(m) = m else {
            return;
        };

        // check if we can safely examine src and dst ports
        if (m.m_pkthdr().len.get() as usize) < off as usize + 8 {
            return;
        }

        let mut b = [0u8; 8];
        m_copydata(m, off, &mut b);
        // SAFETY: eight readable bytes.
        let th = unsafe { th_read8(b.as_ptr()) };

        // Check to see if we have a valid TCP connection corresponding to the address in
        // the ICMPv6 message payload.
        let inp = in6_pcblookup(
            &TCB6TABLE,
            &sa6.sin6_addr,
            th.th_dport,
            &sa6_src.sin6_addr,
            th.th_sport,
            rdomain,
        );
        if cmd == PRC_MSGSIZE {
            // Depending on the value of "valid" and routing table size (mtudisc_{hi,lo}wat),
            // we will:
            // - recalculate the new MTU and create the corresponding routing entry, or
            // - ignore the MTU change notification.
            if let Some(p) = ip6cp {
                icmp6_mtudisc_update(p, inp.is_some());
            }
            in_pcbunref(inp);
            return;
        }
        let so = inp.and_then(in_pcbsolock);
        let mut tp = None;
        if so.is_some() {
            tp = inp.and_then(intotcpcb);
        }
        if let (Some(t), Some(i)) = (tp, inp) {
            let seq = ntohl(th.th_seq);
            if seq_geq(seq, t.snd_una.get()) && seq_lt(seq, t.snd_max.get()) {
                notify(i, INET6CTLERRMAP[cmd as usize]);
            }
        }
        in_pcbsounlock(inp, so);
        in_pcbunref(inp);

        let err = INET6CTLERRMAP[cmd as usize];
        if tp.is_none()
            && matches!(
                err,
                Some(Errno::EHOSTUNREACH | Errno::ENETUNREACH | Errno::EHOSTDOWN)
            )
        {
            // SAFETY: both addresses are readable `sockaddr_in6`s (a local and the caller's).
            unsafe { syn_cache_unreach(sin6tosa_const(ptr::from_ref(&sa6_src)), sa, &th, rdomain) };
        }
    } else {
        in6_pcbnotify(
            &TCB6TABLE,
            &sa6,
            0,
            Some(&sa6_src),
            0,
            rdomain,
            cmd,
            ptr::null_mut(),
            Some(notify),
        );
    }
}

/// The returned TCP header's first eight bytes (ports and sequence number) at `p`, as a
/// header whose other members are zero.
///
/// # Safety
///
/// `p` points at eight readable bytes.
unsafe fn th_read8(p: *const u8) -> Tcphdr {
    // SAFETY: the caller's contract; the members are integers, read unaligned.
    unsafe {
        Tcphdr {
            th_sport: ptr::read_unaligned(p.cast::<u16>()),
            th_dport: ptr::read_unaligned(p.add(2).cast::<u16>()),
            th_seq: ptr::read_unaligned(p.add(4).cast::<u32>()),
            ..Tcphdr::default()
        }
    }
}

/// `tcp_ctlinput`: an ICMP error about a segment we sent.
///
/// # Safety
///
/// `sa` points to a readable socket address; `v` is NULL or the IP header returned in an
/// ICMP error message, followed by at least the first eight bytes of its TCP header, inside
/// the contiguous ICMP message (`icmp_input` pulled up `ICMP_ADVLENMIN` bytes and more).
pub unsafe fn tcp_ctlinput(cmd: i32, sa: *const Sockaddr, rdomain: u32, v: *mut c_void) {
    let mut ip = v.cast::<u8>();
    let mut notify: InpNotifyFn = tcp_notify;

    // SAFETY: the caller's contract: a readable socket address.
    if unsafe { (*sa).sa_family } != AF_INET {
        return;
    }
    // SAFETY: an AF_INET address is a `sockaddr_in`, read unaligned.
    let sin = unsafe { sa.cast::<SockaddrIn>().read_unaligned() };
    let faddr = sin.sin_addr;
    if faddr.s_addr == INADDR_ANY {
        return;
    }

    if cmd as u32 as usize >= PRC_NCMDS {
        return;
    }
    let errno = INETCTLERRMAP[cmd as usize];
    if cmd == PRC_QUENCH {
        // Don't honor ICMP Source Quench messages meant for TCP connections.
        return;
    } else if prc_is_redirect(cmd) {
        notify = in_pcbrtchange;
        ip = ptr::null_mut();
    } else if cmd == PRC_MSGSIZE && ip_mtudisc.load(Ordering::Relaxed) != 0 && !ip.is_null() {
        // SAFETY: the caller's contract: the returned IP header ...
        let iph = unsafe { ip.cast::<Ip>().read_unaligned() };
        // SAFETY: ... followed by the TCP ports and sequence number.
        let th = unsafe { th_read8(ip.add(usize::from(iph.ip_hl()) << 2)) };

        // Verify that the packet in the icmp payload refers to an existing TCP connection.
        let seq = u32::from_be(th.th_seq);
        let inp = in_pcblookup(
            &TCBTABLE,
            iph.ip_dst,
            th.th_dport,
            iph.ip_src,
            th.th_sport,
            rdomain,
        );
        let so = inp.and_then(in_pcbsolock);
        let tp = match (so, inp) {
            (Some(_), Some(i)) => intotcpcb(i),
            _ => None,
        };
        let done = |inp: Option<&'static Inpcb>| {
            in_pcbsounlock(inp, so);
            in_pcbunref(inp);
        };
        match (tp, inp) {
            (Some(tp), Some(i))
                if seq_geq(seq, tp.snd_una.get()) && seq_lt(seq, tp.snd_max.get()) =>
            {
                let advoff = offset_of!(Icmp, icmp_dun);
                // SAFETY: the caller's contract: the IP header sits at `icmp_ip` inside the
                // contiguous ICMP message, whose header and returned TCP bytes are there too.
                let icp = unsafe {
                    IcmpPkt::new(
                        ip.wrapping_sub(advoff),
                        advoff + (usize::from(iph.ip_hl()) << 2) + 8,
                    )
                };

                // If the ICMP message advertises a Next-Hop MTU equal or larger than the
                // maximum packet size we have ever sent, drop the message.
                let mtu = u32::from(ntohs(icp.icmp_nextmtu()));
                if mtu >= tp.t_pmtud_mtu_sent.get() {
                    done(inp);
                    return;
                }
                if mtu >= tcp_hdrsz(tp) + tp.t_pmtud_mss_acked.get() {
                    // Calculate new MTU, and create corresponding route (traditional PMTUD).
                    tp.clear_flags(TF_PMTUD_PEND);
                    icmp_mtudisc(&icp, i.inp_rtableid.get());
                } else {
                    // Record the information got in the ICMP message; act on it later. If
                    // we had already recorded an ICMP message, replace the old one only if
                    // the new message refers to an older TCP segment
                    if tp.has_flags(TF_PMTUD_PEND) {
                        if seq_lt(tp.t_pmtud_th_seq.get(), seq) {
                            done(inp);
                            return;
                        }
                    } else {
                        tp.set_flags(TF_PMTUD_PEND);
                    }
                    tp.t_pmtud_th_seq.set(seq);
                    tp.t_pmtud_nextmtu.set(u32::from(icp.icmp_nextmtu()));
                    tp.t_pmtud_ip_len.set(icp.icmp_ip().ip_len);
                    tp.t_pmtud_ip_hl.set(u16::from(icp.icmp_ip().ip_hl()));
                    done(inp);
                    return;
                }
            }
            _ => {
                // ignore if we don't have a matching connection
                done(inp);
                return;
            }
        }
        done(inp);
        notify = tcp_mtudisc;
        ip = ptr::null_mut();
    } else if cmd == PRC_MTUINC {
        notify = tcp_mtudisc_increase;
        ip = ptr::null_mut();
    } else if cmd == PRC_HOSTDEAD {
        ip = ptr::null_mut();
    } else if errno.is_none() {
        return;
    }

    if !ip.is_null() {
        // SAFETY: the caller's contract, as above.
        let iph = unsafe { ip.cast::<Ip>().read_unaligned() };
        // SAFETY: as above.
        let th = unsafe { th_read8(ip.add(usize::from(iph.ip_hl()) << 2)) };
        let inp = in_pcblookup(
            &TCBTABLE,
            iph.ip_dst,
            th.th_dport,
            iph.ip_src,
            th.th_sport,
            rdomain,
        );
        let so = inp.and_then(in_pcbsolock);
        let tp = match (so, inp) {
            (Some(_), Some(i)) => intotcpcb(i),
            _ => None,
        };
        if let (Some(tp), Some(i)) = (tp, inp) {
            let seq = u32::from_be(th.th_seq);
            if seq_geq(seq, tp.snd_una.get()) && seq_lt(seq, tp.snd_max.get()) {
                notify(i, errno);
            }
        }
        in_pcbsounlock(inp, so);
        in_pcbunref(inp);

        if tp.is_none()
            && matches!(
                INETCTLERRMAP[cmd as usize],
                Some(Errno::EHOSTUNREACH | Errno::ENETUNREACH | Errno::EHOSTDOWN)
            )
        {
            let src = SockaddrIn {
                sin_len: size_of::<SockaddrIn>() as u8,
                sin_family: AF_INET,
                sin_port: th.th_sport,
                sin_addr: iph.ip_src,
                ..SockaddrIn::default()
            };
            // SAFETY: both addresses are readable `sockaddr_in`s (a local and the caller's).
            unsafe { syn_cache_unreach(ptr::from_ref(&src).cast::<Sockaddr>(), sa, &th, rdomain) };
        }
    } else {
        in_pcbnotifyall(&TCBTABLE, &sin, rdomain, errno, Some(notify));
    }
}

/// `tcp6_mtudisc_callback`: path MTU discovery handler of `icmp6_mtudisc_update`.
/// `INET6`; compiled always, as `netinet6` is.
pub fn tcp6_mtudisc_callback(sin6: &SockaddrIn6, rdomain: u32) {
    in6_pcbnotify(
        &TCB6TABLE,
        sin6,
        0,
        Some(&SA6_ANY),
        0,
        rdomain,
        PRC_MSGSIZE,
        ptr::null_mut(),
        Some(tcp_mtudisc),
    );
}

/// `tcp_mtudisc`: on receipt of path MTU corrections, flush old route and replace it with the
/// new one. Retransmit all unacknowledged packets, to ensure that all packets will be
/// received.
pub fn tcp_mtudisc(inp: &'static Inpcb, errno: Option<Errno>) {
    let Some(tp) = intotcpcb(inp) else {
        return;
    };
    let mut change = false;

    let orig_maxseg = tp.t_maxseg.get();

    if let Some(mut rt) = in_pcbrtentry(inp) {
        let orig_mtulock = rt.rt_locks().get() & RTV_MTU;

        // If this was not a host route, remove and realloc.
        if rt.rt_flags.get() & RTF_HOST == 0 {
            in_pcbrtchange(inp, errno);
            match in_pcbrtentry(inp) {
                Some(r) => rt = r,
                None => return,
            }
        }
        if orig_mtulock < rt.rt_locks().get() & RTV_MTU {
            change = true;
        }
    }
    tcp_mss(tp, -1);
    if orig_maxseg > tp.t_maxseg.get() {
        change = true;
    }

    // Resend unacknowledged packets
    tp.snd_nxt.set(tp.snd_una.get());
    if change || errno.is_some_and(|e| e.as_i32() > 0) {
        let _ = tcp_output(tp);
    }
}

/// `tcp_mtudisc_increase`.
pub fn tcp_mtudisc_increase(inp: &'static Inpcb, errno: Option<Errno>) {
    let tp = intotcpcb(inp);
    let rt = in_pcbrtentry(inp);

    if let (Some(tp), Some(rt)) = (tp, rt) {
        // If this was a host route, remove and realloc.
        if rt.rt_flags.get() & RTF_HOST != 0 {
            in_pcbrtchange(inp, errno);
        }

        // also takes care of congestion window
        tcp_mss(tp, -1);
    }
}

/// `tcp_set_iss_tsm`: generate new ISNs with a method based on RFC1948.
pub fn tcp_set_iss_tsm(tp: &Tcpcb) {
    let inp = tp.t_inpcb;
    let rdomain = rtable_l2(inp.inp_rtableid.get());

    mtx_enter(&TCP_TIMER_MTX);
    let iss = TCP_ISS
        .load(Ordering::Relaxed)
        .wrapping_add(TCP_ISS_CONN_INC);
    TCP_ISS.store(iss, Ordering::Relaxed);
    mtx_leave(&TCP_TIMER_MTX);

    // SAFETY: written once by `tcp_init` before any connection exists, then only read.
    let mut ctx = unsafe { TCP_SECRET_CTX.read() }.unwrap_or_default();
    SHA512Update(&mut ctx, &rdomain.to_ne_bytes());
    SHA512Update(&mut ctx, &inp.inp_lport.get().to_ne_bytes());
    SHA512Update(&mut ctx, &inp.inp_fport.get().to_ne_bytes());
    if tp.pf.get() == i32::from(AF_INET6) {
        SHA512Update(&mut ctx, &inp.inp_laddr6.get().s6_addr);
        SHA512Update(&mut ctx, &inp.inp_faddr6.get().s6_addr);
    } else {
        SHA512Update(&mut ctx, &inp.inp_laddr.get().s_addr.to_ne_bytes());
        SHA512Update(&mut ctx, &inp.inp_faddr.get().s_addr.to_ne_bytes());
    }
    let mut digest = [0u8; SHA512_DIGEST_LENGTH];
    SHA512Final(&mut digest, &mut ctx);
    let word =
        |i: usize| u32::from_ne_bytes([digest[i], digest[i + 1], digest[i + 2], digest[i + 3]]);
    tp.iss.set(word(0).wrapping_add(iss));
    tp.ts_modulate.set(word(4));
}

/// `tcp_signature_tdb_attach` (`TCP_SIGNATURE`).
pub fn tcp_signature_tdb_attach() -> i32 {
    0
}

/// `tcp_signature_tdb_init`: keeps a copy of the key.
pub fn tcp_signature_tdb_init(
    tdbp: &Tdb,
    _xsp: &'static Xformsw,
    ii: &mut IpsecInit<'_>,
) -> Result<(), Errno> {
    if ii.ii_authkeylen < 1 || ii.ii_authkeylen > 80 {
        return Err(Errno::EINVAL);
    }

    let len = usize::from(ii.ii_authkeylen);
    let key = &ii.ii_authkey[..len.min(ii.ii_authkey.len())];
    let Some(p) = malloc(len, M_XDATA, M_NOWAIT) else {
        return Err(Errno::ENOMEM);
    };
    // SAFETY: a fresh allocation of `len` bytes; the key holds at most `len`.
    unsafe { ptr::copy_nonoverlapping(key.as_ptr(), p.as_ptr(), key.len()) };
    tdbp.tdb_amxkey.set(p.as_ptr());
    tdbp.tdb_amxkeylen.set(ii.ii_authkeylen);

    Ok(())
}

/// `tcp_signature_tdb_zeroize`.
pub fn tcp_signature_tdb_zeroize(tdbp: &Tdb) -> Result<(), Errno> {
    if let Some(p) = NonNull::new(tdbp.tdb_amxkey.get()) {
        let len = usize::from(tdbp.tdb_amxkeylen.get());
        // SAFETY: the key of `len` bytes `tcp_signature_tdb_init` allocated.
        libkern::explicit_bzero(unsafe { core::slice::from_raw_parts_mut(p.as_ptr(), len) });
        crate::kern::kern_malloc::free(p, M_XDATA, len);
        tdbp.tdb_amxkey.set(ptr::null_mut());
    }

    Ok(())
}

/// `tcp_signature_tdb_input`: the transform carries no packets.
pub fn tcp_signature_tdb_input(
    mp: &mut Option<&'static Mbuf>,
    _tdbp: &'static Tdb,
    _skip: i32,
    _protoff: i32,
    _ns: Option<&Netstack>,
) -> i32 {
    m_freemp(mp);
    IPPROTO_DONE
}

/// `tcp_signature_tdb_output`: the transform carries no packets.
pub fn tcp_signature_tdb_output(
    m: &'static Mbuf,
    _tdbp: &'static Tdb,
    _skip: i32,
    _protoff: i32,
) -> Result<(), Errno> {
    m_freem(m);
    Err(Errno::EINVAL)
}

/// The 20 bytes of a TCP header as the C structure holds them.
fn tcphdr_bytes(th: &Tcphdr) -> [u8; 20] {
    let mut b = [0u8; 20];
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

/// `tcp_signature`: the RFC 2385 MD5 digest of a segment (pseudo header, TCP header with a
/// zero checksum, data, key) into `sig`; `false` when the data cannot be walked. With
/// `doswap` the header's sequence numbers, window and urgent pointer are in host order and
/// are swapped back first.
pub fn tcp_signature(
    tdb: &Tdb,
    af: i32,
    m: &Mbuf,
    th: &Tcphdr,
    iphlen: i32,
    doswap: bool,
    sig: &mut [u8; MD5_DIGEST_LENGTH],
) -> bool {
    let mut ctx = Md5Ctx::default();

    MD5Init(&mut ctx);

    match af {
        0 | AF_INET_I32 => {
            let ip = mtod_ip(m);
            let ippseudo = Ippseudo {
                ippseudo_src: ip.ip_src,
                ippseudo_dst: ip.ip_dst,
                ippseudo_pad: 0,
                ippseudo_p: IPPROTO_TCP as u8,
                ippseudo_len: htons((m.m_pkthdr().len.get() - iphlen) as u16),
            };
            let mut b = [0u8; 12];
            b[0..4].copy_from_slice(&ippseudo.ippseudo_src.s_addr.to_ne_bytes());
            b[4..8].copy_from_slice(&ippseudo.ippseudo_dst.s_addr.to_ne_bytes());
            b[8] = ippseudo.ippseudo_pad;
            b[9] = ippseudo.ippseudo_p;
            b[10..12].copy_from_slice(&ippseudo.ippseudo_len.to_ne_bytes());
            MD5Update(&mut ctx, &b);
        }
        #[cfg(feature = "inet6")]
        AF_INET6_I32 => {
            let ip6 = mtod_ip6(m);
            let mut ip6pseudo = Ip6HdrPseudo {
                ip6ph_src: ip6.ip6_src,
                ip6ph_dst: ip6.ip6_dst,
                ip6ph_len: htonl((m.m_pkthdr().len.get() - iphlen) as u32),
                ip6ph_zero: [0; 3],
                ip6ph_nxt: IPPROTO_TCP as u8,
            };
            in6_clearscope(&mut ip6pseudo.ip6ph_src);
            in6_clearscope(&mut ip6pseudo.ip6ph_dst);
            let mut b = [0u8; 40];
            b[0..16].copy_from_slice(&ip6pseudo.ip6ph_src.s6_addr);
            b[16..32].copy_from_slice(&ip6pseudo.ip6ph_dst.s6_addr);
            b[32..36].copy_from_slice(&ip6pseudo.ip6ph_len.to_ne_bytes());
            b[36..39].copy_from_slice(&ip6pseudo.ip6ph_zero);
            b[39] = ip6pseudo.ip6ph_nxt;
            MD5Update(&mut ctx, &b);
        }
        _ => {}
    }

    let mut th0 = *th;
    th0.th_sum = 0;

    if doswap {
        th0.th_seq = htonl(th0.th_seq);
        th0.th_ack = htonl(th0.th_ack);
        th0.th_win = htons(th0.th_win);
        th0.th_urp = htons(th0.th_urp);
    }
    MD5Update(&mut ctx, &tcphdr_bytes(&th0));

    let thlen = i32::from(th.th_off()) * size_of::<u32>() as i32;
    let len = m.m_pkthdr().len.get() - iphlen - thlen;

    // tcp_signature_apply
    if len > 0
        && m_apply(m, iphlen + thlen, len, |data| {
            MD5Update(&mut ctx, data);
            Ok(())
        })
        .is_err()
    {
        return false;
    }

    // SAFETY: the caller holds the TDB, whose key lives until its `xf_zeroize`.
    MD5Update(&mut ctx, unsafe { tdb.tdb_amxkey() });
    MD5Final(sig, &mut ctx);

    true
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests of `tcp_subr.rs`: window scaling, the header bytes the signature hashes, the
    // notify rules and a control block's life.

    use std::boxed::Box;

    use super::*;
    use crate::netinet::tcp::TH_SYN;

    fn test_tcpcb() -> &'static Tcpcb {
        let inp: &'static Inpcb = Box::leak(Box::new(Inpcb::new(None, None)));
        Box::leak(Box::new(Tcpcb::new(inp)))
    }

    #[test]
    fn rscale_requests_the_smallest_sufficient_shift() {
        let tp = test_tcpcb();
        tcp_rscale(tp, 16 * 1024);
        assert_eq!(tp.request_r_scale.get(), 0);
        tcp_rscale(tp, 65536);
        assert_eq!(tp.request_r_scale.get(), 1);
        tcp_rscale(tp, 2 * 1024 * 1024);
        assert_eq!(tp.request_r_scale.get(), 6);
        tcp_rscale(tp, u64::MAX);
        assert_eq!(tp.request_r_scale.get(), TCP_MAX_WINSHIFT);
    }

    #[test]
    fn header_bytes_follow_the_c_layout() {
        let mut th = Tcphdr {
            th_sport: htons(1234),
            th_dport: htons(80),
            th_seq: htonl(0x0102_0304),
            th_ack: htonl(0x0a0b_0c0d),
            th_flags: TH_SYN,
            th_win: htons(512),
            th_sum: 0xffff,
            th_urp: 0,
            ..Tcphdr::default()
        };
        th.set_th_off(5);
        let b = tcphdr_bytes(&th);
        assert_eq!(&b[0..4], &[0x04, 0xd2, 0x00, 0x50]);
        assert_eq!(&b[4..12], &[1, 2, 3, 4, 0x0a, 0x0b, 0x0c, 0x0d]);
        assert_eq!(b[12], 0x50);
        assert_eq!(b[13], TH_SYN);
        assert_eq!(&b[14..18], &[0x02, 0x00, 0xff, 0xff]);
    }
}
/* </TESTS> */
