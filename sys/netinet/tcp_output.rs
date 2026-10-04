/*	$OpenBSD: tcp_output.c,v 1.158 2026/01/01 05:28:23 jsg Exp $	*/
/*	$NetBSD: tcp_output.c,v 1.16 1997/06/03 16:17:09 kml Exp $	*/
/* <LICENSES> */
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

//! TCP output: `netinet/tcp_output.c`.
//!
//! Upstream: sys/netinet/tcp_output.c @ 3ce1f3f79392
//!
//! [`tcp_output`] figures out what a connection should send (new data within the send and
//! congestion windows, a SACK retransmission, a window update, an ACK, a SYN, FIN or RST)
//! and sends it: the IP and TCP headers come from the connection's template
//! (`tp->t_template`, an `ipovly` and a `tcphdr`), the options (MSS, SACK permitted, window
//! scale, timestamps, the TCP MD5 signature, SACK blocks) are built in a local buffer as in
//! C, the data is copied out of the send buffer, and `ip_output` takes the packet; the TCP
//! checksum is left to the interface or `in_proto_cksum_out`. A large send may be handed down
//! as one TSO packet; [`tcp_softtso_chop`] cuts such a packet into segments when the
//! interface cannot, and [`tcp_if_output_tso`] chooses between the two for `if_output_tso`.
//! [`tcp_setpersist`] (re)starts the persist timer, [`tcp_sack_output`] and
//! [`tcp_sack_adjust`] walk the sender's SACK holes.
//!
//! ## Deviations
//! - `tcp_output` returns `Result<(), Errno>`; the C's error tests (`error == ENOBUFS`,
//!   `EMSGSIZE`, `EHOSTUNREACH`/`ENETDOWN`) are a `match` on the error after the `out:`
//!   label. The `goto send` decision is a labelled block, `goto again` the function's
//!   `loop`, `goto out` a labelled block whose value is the error, `goto timer` an `if` around
//!   the code it skips.
//! - `sack_rxmit` and `p` (the hole being retransmitted) are one
//!   `Option<&'static Sackhole>`: the C sets the flag exactly when it sets the pointer, and
//!   reads the pointer only under the flag.
//! - The TCP header is a local [`Tcphdr`] read from the copied template and written back
//!   into the packet (unaligned) once its fields are final, before the signature, the trace
//!   and `ip_output` read the packet; the options are a `[u8; MAX_TCPOPTLEN]` written with
//!   big-endian stores (the C's `htonl` through a `u_int32_t *`). Writes into the header
//!   mbuf are bounds-checked against its `m_len` and panic past it, where the C would write
//!   past the header.
//! - `tcp_signature` writes the digest into a local array, which is copied into the option
//!   (the C hands it a pointer into the packet); the digest does not cover the options, so
//!   the bytes it reads are the same.
//! - `tp->t_inpcb->inp_outputopts6` shares `inp_options`'s union slot: the TSO test reads the
//!   one member.
//! - `TCP_SACK_DEBUG` (not in GENERIC) is the cargo feature `tcp_sack_debug`: `tcp_print_holes`
//!   and its call in `tcp_sack_output`.
//! - `tcp_softtso_chop` reads the IP header once as a copy (`mtod_ip`, which panics on a
//!   first mbuf shorter than an IP header, where the C reads past it) and writes it back into
//!   the first mbuf as it is after `m_pullup` (the C keeps a pointer taken before the
//!   pullup); each new segment's TCP header and options are copied with `m_copydata` (the C
//!   copies from the pointer, assuming the options are contiguous).
//! - `tcp_if_output_tso` calls `ifp->if_output` through the hook in `if_output` (a missing
//!   hook panics, where the C would jump through NULL); a NULL `*mp` returns 0, which the C
//!   does not check.
//! - `INET6` is configured (feature `inet6`): the `PF_INET6`/`AF_INET6` cases with
//!   `ip6_output` and `in6_selecthlim`, and the IPv6 cases of `tcp_softtso_chop` and
//!   `tcp_if_output_tso` (`in6_proto_cksum_out`, `IFCAP_TSOv6`).
//! - Not configured, a comment at its site: `TCPDEBUG` (the
//!   `SO_DEBUG` trace calls `tcp_trace` of `tcp_debug.rs`, as the C does without it).
//!   Configured: `TCP_ECN`, `TCP_SIGNATURE` (with `IPSEC`: `gettdbbysrcdst`), `NPF`
//!   (`pf_mbuf_link_inpcb`), `NSTOEPLITZ` (the flow id; pf needs `stoeplitz`). `DIAGNOSTIC`
//!   is the cargo feature `diagnostic`.

use core::cmp::min;
use core::mem::size_of;
use core::ptr;
use core::slice;
use core::sync::atomic::Ordering;

use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::subr_prf::{Str, panic};
use crate::kern::uipc_mbuf::{
    MAX_LINKHDR, m_adj, m_align, m_copydata, m_copym, m_dup_pkthdr, m_free, m_freem, m_gethdr,
    m_pullup, m_trailingspace, ml_dequeue, ml_enqueue, ml_init, ml_purge,
};
use crate::net::if_::{IFCAP_TSOv4, if_output_ml};
use crate::net::if_var::Ifnet;
use crate::net::pf::pf_mbuf_link_inpcb;
use crate::net::route::Rtentry;
use crate::net::rtable::rtable_l2;
use crate::netinet::in_::{IPPROTO_TCP, InAddr, SockaddrIn};
use crate::netinet::ip::{IP_MAXPACKET, IP_MF, IP_OFFMASK, IPTOS_ECN_ECT0, Ip};
use crate::netinet::ip_id::ip_randomid;
use crate::netinet::ip_input::ip_mtudisc;
use crate::netinet::ip_ipsp::{SockaddrUnion, gettdbbysrcdst, tdb_unref};
use crate::netinet::ip_output::{in_hdr_cksum_out, in_ifcap_cksum, in_proto_cksum_out, ip_output};
use crate::netinet::ip_var::{IP_MTUDISC, mtod_ip, mtod_ip_store};
use crate::netinet::ip6::Ip6Hdr;
use crate::netinet::tcp::{
    MAX_TCPOPTLEN, TCP_MAX_SACK, TCP_MAXWIN, TCPOLEN_SACK, TCPOLEN_SIGLEN, TCPOLEN_SIGNATURE,
    TCPOLEN_TSTAMP_APPA, TCPOLEN_WINDOW, TCPOPT_MAXSEG, TCPOPT_NOP, TCPOPT_SACK_HDR,
    TCPOPT_SACK_PERMIT_HDR, TCPOPT_SIGNATURE, TCPOPT_TSTAMP_HDR, TCPOPT_WINDOW, TH_ACK, TH_CWR,
    TH_ECE, TH_FIN, TH_PUSH, TH_RST, TH_SYN, TH_URG, Tcphdr,
};
use crate::netinet::tcp_debug::{TA_OUTPUT, tcp_trace};
use crate::netinet::tcp_fsm::{TCP_OUTFLAGS, TCPS_ESTABLISHED, tcps_havercvdsyn};
use crate::netinet::tcp_input::{TCPREXMTTHRESH, tcp_mss, tcp_mss_update};
use crate::netinet::tcp_seq::{seq_geq, seq_gt, seq_lt};
use crate::netinet::tcp_subr::{TCP_DO_ECN, TCP_DO_TSO, tcp_mtudisc, tcp_signature};
use crate::netinet::tcp_timer::{
    TCP_BACKOFF, TCP_MAXRXTSHIFT, TCPT_DELACK, TCPT_PERSIST, TCPT_REXMT, TCPTV_PERSMAX,
    TCPTV_PERSMIN, tcp_delack_msecs, tcp_timer_arm, tcp_timer_disarm, tcp_timer_isarmed,
    tcpt_rangeset,
};
use crate::netinet::tcp_usrreq::tcp_update_sndspace;
use crate::netinet::tcp_var::{
    Sackhole, TCP_RTT_BASE_SHIFT, TF_ACKNOW, TF_DISABLE_ECN, TF_ECN_PERMIT, TF_LASTIDLE,
    TF_NODELAY, TF_NOOPT, TF_NOPUSH, TF_RCVD_CE, TF_RCVD_SCALE, TF_RCVD_TSTMP, TF_REQ_SCALE,
    TF_REQ_TSTMP, TF_SACK_PERMIT, TF_SEND_CWR, TF_SENTFIN, TF_SIGNATURE, Tcpcb, TcpstatCounters,
    tcp_now, tcpstat_add, tcpstat_inc, tcpstat_pkt,
};
use crate::sys::endian::{htonl, htons, ntohl};
use crate::sys::errno::Errno;
use crate::sys::malloc::M_NOWAIT;
use crate::sys::mbuf::{
    M_DONTWAIT, M_EXT, M_FLOWID, M_PKTHDR, M_TCP_CSUM_OUT, M_TCP_TSO, MAXMCLBYTES, MHLEN,
    MT_HEADER, Mbuf, MbufList, mclget, ml_len, mtod,
};
use crate::sys::socket::{AF_INET, PF_INET, SO_DEBUG, Sockaddr};
use crate::sys::socketvar::{Socket, sbspace_locked, soissending};
#[cfg(feature = "inet6")]
use crate::{
    net::if_::IFCAP_TSOv6,
    netinet6::in6::{In6Addr, SockaddrIn6},
    netinet6::in6_src::in6_selecthlim,
    netinet6::ip6_output::{in6_proto_cksum_out, ip6_output},
    netinet6::ip6_var::{mtod_ip6, mtod_ip6_store},
    sys::socket::{AF_INET6, PF_INET6},
};

/// `tcp_print_holes` (`TCP_SACK_DEBUG`): prints the sender's SACK holes.
#[cfg(feature = "tcp_sack_debug")]
pub fn tcp_print_holes(tp: &Tcpcb) {
    let mut p = tp.snd_holes.get();
    if p.is_none() {
        return;
    }
    crate::kprintf!("Hole report: start--end dups rxmit\n");
    while let Some(h) = p {
        crate::kprintf!(
            "{:x}--{:x} d {} r {:x}\n",
            h.start.get(),
            h.end.get(),
            h.dups.get(),
            h.rxmit.get()
        );
        p = h.next.get();
    }
    crate::kprintf!("\n");
}

/// `tcp_sack_output`: the first SACK hole with pending retransmissions (enough duplicate
/// ACKs, data left to resend at or past `snd_una`), or `None`.
pub fn tcp_sack_output(tp: &Tcpcb) -> Option<&'static Sackhole> {
    if tp.sack_enable.get() == 0 {
        return None;
    }
    let mut p = tp.snd_holes.get();
    while let Some(h) = p {
        if h.dups.get() >= TCPREXMTTHRESH && seq_lt(h.rxmit.get(), h.end.get()) {
            if seq_lt(h.rxmit.get(), tp.snd_una.get()) {
                // old SACK hole
                p = h.next.get();
                continue;
            }
            #[cfg(feature = "tcp_sack_debug")]
            tcp_print_holes(tp);
            return Some(h);
        }
        p = h.next.get();
    }
    None
}

/// `tcp_sack_adjust`: after a timeout, the SACK list may be rebuilt. This SACK information
/// should be used to avoid retransmitting SACKed data. This function traverses the SACK list
/// to see if `snd_nxt` should be moved forward.
pub fn tcp_sack_adjust(tp: &Tcpcb) {
    let Some(mut cur) = tp.snd_holes.get() else {
        return; // No holes
    };
    if seq_geq(tp.snd_nxt.get(), tp.rcv_lastsack.get()) {
        return; // We're already beyond any SACKed blocks
    }
    // Two cases for which we want to advance snd_nxt:
    // i) snd_nxt lies between end of one hole and beginning of another
    // ii) snd_nxt lies between end of last hole and rcv_lastsack
    while let Some(next) = cur.next.get() {
        if seq_lt(tp.snd_nxt.get(), cur.end.get()) {
            return;
        }
        if seq_geq(tp.snd_nxt.get(), next.start.get()) {
            cur = next;
        } else {
            tp.snd_nxt.set(next.start.get());
            return;
        }
    }
    if seq_lt(tp.snd_nxt.get(), cur.end.get()) {
        return;
    }
    tp.snd_nxt.set(tp.rcv_lastsack.get());
}

/// Stores `v` in network order at byte `off` of the option buffer (the C's
/// `*(u_int32_t *)(opt + off) = htonl(v)`).
fn opt_put32(opt: &mut [u8; MAX_TCPOPTLEN], off: u32, v: u32) {
    let off = off as usize;
    opt[off..off + 4].copy_from_slice(&v.to_be_bytes());
}

/// Panics unless `m`'s first mbuf holds `len` bytes at offset `off`.
fn m_hdr_check(m: &Mbuf, off: usize, len: usize, what: &str) {
    if off + len > m.m_len().get() as usize {
        panic(format_args!("{}: header past m_len", what));
    }
}

/// Copies `src` into `m`'s first mbuf at byte `off` (within its `m_len`).
fn m_hdr_write(m: &Mbuf, off: usize, src: &[u8]) {
    m_hdr_check(m, off, src.len(), "tcp_output");
    // SAFETY: the first mbuf's data holds `m_len` bytes, which cover `off..off + len`
    // (checked); `src` is not inside the mbuf.
    unsafe { ptr::copy_nonoverlapping(src.as_ptr(), mtod::<u8>(m).add(off), src.len()) };
}

/// The TCP header at byte `off` of `m`'s first mbuf, as a copy (the data need not be
/// aligned).
fn th_load(m: &Mbuf, off: usize) -> Tcphdr {
    m_hdr_check(m, off, size_of::<Tcphdr>(), "th_load");
    // SAFETY: the bytes are inside the first mbuf (checked); a `Tcphdr` is integers, valid
    // for any bytes.
    unsafe { ptr::read_unaligned(mtod::<u8>(m).add(off).cast::<Tcphdr>()) }
}

/// Writes `th` as the TCP header at byte `off` of `m`'s first mbuf.
fn th_store(m: &Mbuf, off: usize, th: &Tcphdr) {
    m_hdr_check(m, off, size_of::<Tcphdr>(), "th_store");
    // SAFETY: as in `th_load`.
    unsafe { ptr::write_unaligned(mtod::<u8>(m).add(off).cast::<Tcphdr>(), *th) };
}

/// `MGETHDR(m, M_DONTWAIT, MT_HEADER)`, with a cluster when the link, IP and TCP headers do
/// not fit in `MHLEN`, its data moved past `max_linkhdr` and `m_len` set to `hdrlen`; `None`
/// when out of mbufs (the C's two copies of this sequence in `tcp_output`).
fn tcp_output_gethdr(max_linkhdr: u32, hdrlen: u32) -> Option<&'static Mbuf> {
    let m = m_gethdr(M_DONTWAIT, MT_HEADER)?;
    if (max_linkhdr + hdrlen) as usize > MHLEN {
        mclget(m, M_DONTWAIT);
        if m.m_flags().get() & M_EXT == 0 {
            m_freem(m);
            return None;
        }
    }
    m.m_data()
        .set(m.m_data().get().wrapping_add(max_linkhdr as usize));
    m.m_len().set(hdrlen);
    Some(m)
}

/// `so->so_snd.sb_mb`, which is not NULL while there is data to send.
fn sb_mb(so: &Socket) -> &'static Mbuf {
    match so.so_snd.sb_mb.get() {
        Some(m) => m,
        None => panic(format_args!("tcp_output: no data in the send buffer")),
    }
}

/// `tcp_output`: TCP output routine: figure out what should be sent and send it.
pub fn tcp_output(tp: &'static Tcpcb) -> Result<(), Errno> {
    let so = tp.socket();
    let inp = tp.t_inpcb;
    let mut opt = [0u8; MAX_TCPOPTLEN];
    let mut len: i64 = 0;
    let mut sendalot = false;
    let mut sack_rxmit: Option<&'static Sackhole> = None;
    // TCP_ECN
    let do_ecn = TCP_DO_ECN.load(Ordering::Relaxed) != 0;

    // TCP_SIGNATURE && DIAGNOSTIC
    #[cfg(feature = "diagnostic")]
    if tp.sack_enable.get() != 0 && tp.has_flags(TF_SIGNATURE) {
        return Err(Errno::EINVAL);
    }

    let now = tcp_now();

    mtx_enter(&so.so_snd.sb_mtx);
    let doing_sosend = soissending(so);
    mtx_leave(&so.so_snd.sb_mtx);

    // Determine length of data that should be transmitted, and flags that will be used. If
    // there is some data or critical controls (SYN, RST) to send, then transmit; otherwise,
    // investigate further.
    let mut idle = tp.has_flags(TF_LASTIDLE) || tp.snd_max.get() == tp.snd_una.get();
    if idle && now.wrapping_sub(tp.t_rcvtime.get()) >= tp.t_rxtcur.get() as u64 {
        // We have been idle for "a while" and no acks are expected to clock out any data we
        // send -- slow start to get ack "clock" running again.
        tp.snd_cwnd.set(2 * u64::from(tp.t_maxseg.get()));
    }

    // remember 'idle' for next invocation of tcp_output
    if idle && doing_sosend {
        tp.set_flags(TF_LASTIDLE);
        idle = false;
    } else {
        tp.clear_flags(TF_LASTIDLE);
    }

    loop {
        // again:
        // If we've recently taken a timeout, snd_max will be greater than snd_nxt. There may
        // be SACK information that allows us to avoid resending already delivered data.
        // Adjust snd_nxt accordingly.
        if tp.sack_enable.get() != 0 && seq_lt(tp.snd_nxt.get(), tp.snd_max.get()) {
            tcp_sack_adjust(tp);
        }
        let mut off = tp.snd_nxt.get().wrapping_sub(tp.snd_una.get()) as i32;
        let mut win = min(tp.snd_wnd.get(), tp.snd_cwnd.get()) as i64;

        let mut flags = TCP_OUTFLAGS[tp.t_state.get() as usize];

        // Send any SACK-generated retransmissions. If we're explicitly trying to send out new
        // data (when sendalot is 1), bypass this function. If we retransmit in fast recovery
        // mode, decrement snd_cwnd, since we're replacing a (future) new transmission with a
        // retransmission now, and we previously incremented snd_cwnd in tcp_input().
        if tp.sack_enable.get() != 0
            && !sendalot
            && i32::from(tp.t_dupacks.get()) >= TCPREXMTTHRESH
            && let Some(p) = tcp_sack_output(tp)
        {
            off = p.rxmit.get().wrapping_sub(tp.snd_una.get()) as i32;
            sack_rxmit = Some(p);
            // Coalesce holes into a single retransmission
            len = i64::from(min(
                u32::from(tp.t_maxseg.get()),
                p.end.get().wrapping_sub(p.rxmit.get()),
            ));
            if seq_lt(tp.snd_una.get(), tp.snd_last.get()) {
                tp.snd_cwnd
                    .set(tp.snd_cwnd.get().wrapping_sub(u64::from(tp.t_maxseg.get())));
            }
        }

        sendalot = false;
        let mut tso = false;
        // If in persist timeout with window of 0, send 1 byte. Otherwise, if window is small
        // but nonzero and timer expired, we will send what we can and go to transmit state.
        if tp.t_force.get() {
            if win == 0 {
                // If we still have some data to send, then clear the FIN bit. Usually this
                // would happen below when it realizes that we aren't sending all the data.
                // However, if we have exactly 1 byte of unset data, then it won't clear the
                // FIN bit below, and if we are in persist state, we wind up sending the
                // packet without recording that we sent the FIN bit.
                //
                // We can't just blindly clear the FIN bit, because if we don't have any more
                // data to send then the probe will be the FIN itself.
                if (off as u64) < so.so_snd.sb_cc.get() {
                    flags &= !TH_FIN;
                }
                win = 1;
            } else {
                tcp_timer_disarm(tp, TCPT_PERSIST);
                tp.t_rxtshift.set(0);
            }
        }

        if sack_rxmit.is_none() {
            len = min(so.so_snd.sb_cc.get(), win as u64).wrapping_sub(off as u64) as i64;
        }

        if len < 0 {
            // If FIN has been sent but not acked, but we haven't been called to retransmit,
            // len will be -1. Otherwise, window shrank after we sent into it. If window
            // shrank to 0, cancel pending retransmit, pull snd_nxt back to (closed) window,
            // and set the persist timer if it isn't already going. If the window didn't close
            // completely, just wait for an ACK.
            len = 0;
            if win == 0 {
                tcp_timer_disarm(tp, TCPT_REXMT);
                tp.t_rxtshift.set(0);
                tp.snd_nxt.set(tp.snd_una.get());
                if !tcp_timer_isarmed(tp, TCPT_PERSIST) {
                    tcp_setpersist(tp);
                }
            }
        }

        // Never send more than half a buffer full. This insures that we can always keep 2
        // packets on the wire, no matter what SO_SNDBUF is, and therefore acks will never be
        // delayed unless we run out of data to transmit.
        let txmaxseg = min(so.so_snd.sb_hiwat.get() / 2, u64::from(tp.t_maxseg.get())) as i64;

        if len > txmaxseg {
            let maxseg = i64::from(tp.t_maxseg.get());
            // `inp_outputopts6` is `inp_options`'s union slot: one test covers both.
            if TCP_DO_TSO.load(Ordering::Relaxed) != 0
                && inp.inp_options.get().is_none()
                && !tp.has_flags(TF_SIGNATURE)
                && len >= 2 * maxseg
                && tp.rcv_numsacks.get() == 0
                && sack_rxmit.is_none()
                && flags & (TH_SYN | TH_RST | TH_FIN) == 0
            {
                tso = true;
                // avoid small chopped packets
                if len > (len / maxseg) * maxseg {
                    len = (len / maxseg) * maxseg;
                    sendalot = true;
                }
            } else {
                len = txmaxseg;
                sendalot = true;
            }
        }
        if ((i64::from(off) + len) as u64) < so.so_snd.sb_cc.get() {
            flags &= !TH_FIN;
        }

        mtx_enter(&so.so_rcv.sb_mtx);
        win = sbspace_locked(&so.so_rcv);
        let rcv_hiwat = so.so_rcv.sb_hiwat.get() as i64;
        mtx_leave(&so.so_rcv.sb_mtx);

        'send: {
            // Sender silly window avoidance. If connection is idle and can send all data, a
            // maximum segment, at least a maximum default-size segment do it, or are forced,
            // do it; otherwise don't bother. If peer's buffer is tiny, then send when window
            // is at least half open. If retransmitting (possibly after persist timer forced
            // us to send into a small window), then must resend.
            if len != 0 {
                if len >= txmaxseg {
                    break 'send;
                }
                if (idle || tp.has_flags(TF_NODELAY))
                    && ((len + i64::from(off)) as u64) >= so.so_snd.sb_cc.get()
                    && !doing_sosend
                    && !tp.has_flags(TF_NOPUSH)
                {
                    break 'send;
                }
                if tp.t_force.get() {
                    break 'send;
                }
                if (len as u64) >= tp.max_sndwnd.get() / 2 && tp.max_sndwnd.get() > 0 {
                    break 'send;
                }
                if seq_lt(tp.snd_nxt.get(), tp.snd_max.get()) {
                    break 'send;
                }
                if sack_rxmit.is_some() {
                    break 'send;
                }
            }

            // Compare available window to amount of window known to peer (as advertised
            // window less next expected input). If the difference is at least two max size
            // segments, or at least 50% of the maximum possible window, then want to send a
            // window update to peer.
            if win > 0 {
                // "adv" is the amount we can increase the window, taking into account that we
                // are limited by TCP_MAXWIN << tp->rcv_scale.
                let adv = min(win, i64::from(TCP_MAXWIN) << tp.rcv_scale.get())
                    - i64::from(tp.rcv_adv.get().wrapping_sub(tp.rcv_nxt.get()));

                if adv >= 2 * i64::from(tp.t_maxseg.get()) {
                    break 'send;
                }
                if 2 * adv >= rcv_hiwat {
                    break 'send;
                }
            }

            // Send if we owe peer an ACK.
            if tp.has_flags(TF_ACKNOW) {
                break 'send;
            }
            if flags & (TH_SYN | TH_RST) != 0 {
                break 'send;
            }
            if seq_gt(tp.snd_up.get(), tp.snd_una.get()) {
                break 'send;
            }
            // If our state indicates that FIN should be sent and we have not yet done so, or
            // we're retransmitting the FIN, then we need to send.
            if flags & TH_FIN != 0
                && (!tp.has_flags(TF_SENTFIN) || tp.snd_nxt.get() == tp.snd_una.get())
            {
                break 'send;
            }
            // In SACK, it is possible for tcp_output to fail to send a segment after the
            // retransmission timer has been turned off. Make sure that the retransmission
            // timer is set.
            if seq_gt(tp.snd_max.get(), tp.snd_una.get())
                && !tcp_timer_isarmed(tp, TCPT_REXMT)
                && !tcp_timer_isarmed(tp, TCPT_PERSIST)
            {
                tcp_timer_arm(tp, TCPT_REXMT, tp.t_rxtcur.get() as u64);
                return Ok(());
            }

            // TCP window updates are not reliable, rather a polling protocol using
            // ``persist'' packets is used to insure receipt of window updates. The three
            // ``states'' for the output side are:
            //	idle			not doing retransmits or persists
            //	persisting		to move a small or zero window
            //	(re)transmitting	and thereby not persisting
            //
            // tp->t_timer[TCPT_PERSIST]
            //	is set when we are in persist state.
            // tp->t_force
            //	is set when we are called to send a persist packet.
            // tp->t_timer[TCPT_REXMT]
            //	is set when we are retransmitting
            // The output side is idle when both timers are zero.
            //
            // If send window is too small, there is data to transmit, and no retransmit or
            // persist is pending, then go to persist state. If nothing happens soon, send
            // when timer expires: if window is nonzero, transmit what we can, otherwise force
            // out a byte.
            if so.so_snd.sb_cc.get() != 0
                && !tcp_timer_isarmed(tp, TCPT_REXMT)
                && !tcp_timer_isarmed(tp, TCPT_PERSIST)
            {
                tp.t_rxtshift.set(0);
                tcp_setpersist(tp);
            }

            // No reason to send a segment, just return.
            return Ok(());
        }

        // send:
        // Before ESTABLISHED, force sending of initial options unless TCP set not to do any
        // options. NOTE: we assume that the IP/TCP header plus TCP options always fit in a
        // single mbuf, leaving room for a maximum link header, i.e.
        //	max_linkhdr + sizeof(network header) + sizeof(struct tcphdr +
        //		optlen <= MHLEN
        let mut optlen: u32 = 0;

        let mut hdrlen = match tp.pf.get() {
            // 0: default to PF_INET
            pf if pf == 0 || pf == i32::from(PF_INET) => {
                (size_of::<Ip>() + size_of::<Tcphdr>()) as u32
            }
            #[cfg(feature = "inet6")]
            pf if pf == i32::from(PF_INET6) => (size_of::<Ip6Hdr>() + size_of::<Tcphdr>()) as u32,
            _ => return Err(Errno::EPFNOSUPPORT),
        };

        if flags & TH_SYN != 0 {
            tp.snd_nxt.set(tp.iss.get());
            if !tp.has_flags(TF_NOOPT) {
                opt[0] = TCPOPT_MAXSEG;
                opt[1] = 4;
                let mss = tcp_mss(tp, 0) as u16;
                opt[2..4].copy_from_slice(&mss.to_be_bytes());
                optlen = 4;

                if flags & TH_ACK != 0 {
                    tcp_mss_update(tp);
                }
                // If this is the first SYN of connection (not a SYN ACK), include
                // SACK_PERMIT_HDR option. If this is a SYN ACK, include SACK_PERMIT_HDR
                // option if peer has already done so.
                if tp.sack_enable.get() != 0
                    && (flags & TH_ACK == 0 || tp.has_flags(TF_SACK_PERMIT))
                {
                    opt_put32(&mut opt, optlen, TCPOPT_SACK_PERMIT_HDR);
                    optlen += 4;
                }
                if tp.has_flags(TF_REQ_SCALE)
                    && (flags & TH_ACK == 0 || tp.has_flags(TF_RCVD_SCALE))
                {
                    opt_put32(
                        &mut opt,
                        optlen,
                        u32::from(TCPOPT_NOP) << 24
                            | u32::from(TCPOPT_WINDOW) << 16
                            | u32::from(TCPOLEN_WINDOW) << 8
                            | u32::from(tp.request_r_scale.get()),
                    );
                    optlen += 4;
                }
            }
        }

        // Send a timestamp and echo-reply if this is a SYN and our side wants to use
        // timestamps (TF_REQ_TSTMP is set) or both our side and our peer have sent timestamps
        // in our SYN's.
        if tp.t_flags.get() & (TF_REQ_TSTMP | TF_NOOPT) == TF_REQ_TSTMP
            && flags & TH_RST == 0
            && (flags & (TH_SYN | TH_ACK) == TH_SYN || tp.has_flags(TF_RCVD_TSTMP))
        {
            // Form timestamp option as shown in appendix A of RFC 1323.
            opt_put32(&mut opt, optlen, TCPOPT_TSTAMP_HDR);
            opt_put32(
                &mut opt,
                optlen + 4,
                (now as u32).wrapping_add(tp.ts_modulate.get()),
            );
            opt_put32(&mut opt, optlen + 8, tp.ts_recent.get());
            optlen += u32::from(TCPOLEN_TSTAMP_APPA);
        }
        // Set receive buffer autosizing timestamp.
        if tp.rfbuf_ts.get() == 0 {
            tp.rfbuf_ts.set(now);
            tp.rfbuf_cnt.set(0);
        }

        // TCP_SIGNATURE
        let mut sigoff: u32 = 0;
        if tp.has_flags(TF_SIGNATURE) {
            let bp = optlen as usize;

            // Send signature option
            opt[bp] = TCPOPT_SIGNATURE;
            opt[bp + 1] = TCPOLEN_SIGNATURE;
            sigoff = optlen + 2;

            opt[bp + 2..bp + 18].fill(0);

            // Pad options list to the next 32 bit boundary and terminate it.
            opt[bp + 18] = TCPOPT_NOP;
            opt[bp + 19] = TCPOPT_NOP;

            optlen += u32::from(TCPOLEN_SIGLEN);
        }

        // Send SACKs if necessary. This should be the last option processed. Only as many
        // SACKs are sent as are permitted by the maximum options size. No more than three
        // SACKs are sent.
        if tp.sack_enable.get() != 0
            && tp.t_state.get() == TCPS_ESTABLISHED
            && tp.t_flags.get() & (TF_SACK_PERMIT | TF_NOOPT) == TF_SACK_PERMIT
            && tp.rcv_numsacks.get() != 0
        {
            let olp = optlen;
            let mut lp = optlen + 4;
            let mut count: u32 = 0; // actual number of SACKs inserted
            let maxsack = (MAX_TCPOPTLEN as u32).wrapping_sub(optlen + 4) / u32::from(TCPOLEN_SACK);

            tcpstat_inc(TcpstatCounters::TcpsSackSndOpts);
            let maxsack = min(maxsack, TCP_MAX_SACK as u32);
            for i in 0..tp.rcv_numsacks.get().max(0) as usize {
                if count >= maxsack {
                    break;
                }
                let sack = tp.sackblks[i].get();
                if sack.start == 0 && sack.end == 0 {
                    continue;
                }
                opt_put32(&mut opt, lp, sack.start);
                opt_put32(&mut opt, lp + 4, sack.end);
                lp += 8;
                count += 1;
            }
            opt_put32(
                &mut opt,
                olp,
                TCPOPT_SACK_HDR | (u32::from(TCPOLEN_SACK) * count + 2),
            );
            optlen += u32::from(TCPOLEN_SACK) * count + 4; // including leading NOPs
        }

        #[cfg(feature = "diagnostic")]
        if optlen as usize > MAX_TCPOPTLEN {
            panic(format_args!("tcp_output: options too long"));
        }

        hdrlen += optlen;

        let max_linkhdr = MAX_LINKHDR.load(Ordering::Relaxed) as u32;

        // Adjust data length if insertion of options will bump the packet length beyond the
        // t_maxopd length. Clear the FIN bit because we cut off the tail of the segment.
        let maxopd = u32::from(tp.t_maxopd.get()).wrapping_sub(optlen);
        if len > i64::from(maxopd) {
            if tso {
                if len + i64::from(hdrlen) + i64::from(max_linkhdr) > MAXMCLBYTES as i64 {
                    len = i64::from(
                        (MAXMCLBYTES as u32)
                            .wrapping_sub(hdrlen)
                            .wrapping_sub(max_linkhdr),
                    );
                    sendalot = true;
                }
            } else {
                len = i64::from(maxopd);
                sendalot = true;
            }
            flags &= !TH_FIN;
        }

        #[cfg(feature = "diagnostic")]
        if (max_linkhdr + hdrlen) as usize > crate::sys::mbuf::MCLBYTES {
            panic(format_args!("tcphdr too big"));
        }

        let mut packetlen: u32 = 0;
        let error: Result<(), Errno> = 'out: {
            // Grab a header mbuf, attaching a copy of data to be transmitted, and initialize
            // the header from the template for sends on this connection.
            let m = if len != 0 {
                if tp.t_force.get() && len == 1 {
                    tcpstat_inc(TcpstatCounters::TcpsSndprobe);
                } else if seq_lt(tp.snd_nxt.get(), tp.snd_max.get()) {
                    tcpstat_pkt(
                        TcpstatCounters::TcpsSndrexmitpack,
                        TcpstatCounters::TcpsSndrexmitbyte,
                        len as u64,
                    );
                    tp.t_sndrexmitpack
                        .set(tp.t_sndrexmitpack.get().wrapping_add(1));
                } else {
                    tcpstat_pkt(
                        TcpstatCounters::TcpsSndpack,
                        TcpstatCounters::TcpsSndbyte,
                        len as u64,
                    );
                }
                let Some(m) = tcp_output_gethdr(max_linkhdr, hdrlen) else {
                    break 'out Err(Errno::ENOBUFS);
                };
                let sb_mb = sb_mb(so);
                if len <= i64::from(m_trailingspace(m)) {
                    // SAFETY: `m_trailingspace` bytes follow the `hdrlen` bytes of data in
                    // the first mbuf, and `len` fits in them.
                    let dst = unsafe {
                        slice::from_raw_parts_mut(mtod::<u8>(m).add(hdrlen as usize), len as usize)
                    };
                    m_copydata(sb_mb, off, dst);
                    m.m_len().set(m.m_len().get() + len as u32);
                } else {
                    let next = m_copym(sb_mb, off, len as i32, M_NOWAIT);
                    m.m_next().set(next);
                    if next.is_none() {
                        let _ = m_free(m);
                        break 'out Err(Errno::ENOBUFS);
                    }
                }
                if sb_mb.m_flags().get() & M_PKTHDR != 0 {
                    m.m_pkthdr()
                        .ph_loopcnt
                        .set(sb_mb.m_pkthdr().ph_loopcnt.get());
                }
                // If we're sending everything we've got, set PUSH. (This will keep happy those
                // implementations which only give data to the user when a buffer fills or a
                // PUSH comes in.)
                if (i64::from(off) + len) as u64 == so.so_snd.sb_cc.get() && !doing_sosend {
                    flags |= TH_PUSH;
                }
                tp.t_sndtime.set(now);
                m
            } else {
                if tp.has_flags(TF_ACKNOW) {
                    tcpstat_inc(TcpstatCounters::TcpsSndacks);
                } else if flags & (TH_SYN | TH_FIN | TH_RST) != 0 {
                    tcpstat_inc(TcpstatCounters::TcpsSndctrl);
                } else if seq_gt(tp.snd_up.get(), tp.snd_una.get()) {
                    tcpstat_inc(TcpstatCounters::TcpsSndurg);
                } else {
                    tcpstat_inc(TcpstatCounters::TcpsSndwinup);
                }

                let Some(m) = tcp_output_gethdr(max_linkhdr, hdrlen) else {
                    break 'out Err(Errno::ENOBUFS);
                };
                m
            };
            m.m_pkthdr().ph_ifidx.set(0);
            m.m_pkthdr().len.set((i64::from(hdrlen) + len) as i32);

            // Enable TSO and specify the size of the resulting segments.
            if tso {
                let ph = m.m_pkthdr();
                ph.csum_flags.set(ph.csum_flags.get() | M_TCP_TSO);
                ph.ph_mss.set(tp.t_maxseg.get());
            }

            let Some(template) = tp.t_template.get() else {
                panic(format_args!("tcp_output"));
            };
            let tlen = template.m_len().get();
            #[cfg(feature = "diagnostic")]
            if tlen != hdrlen - optlen {
                panic(format_args!("tcp_output: template len != hdrlen - optlen"));
            }
            // SAFETY: the template's first mbuf holds `m_len` bytes of data.
            let tbytes = unsafe { slice::from_raw_parts(mtod::<u8>(template), tlen as usize) };
            m_hdr_write(m, 0, tbytes);
            let thoff = (tlen as usize).wrapping_sub(size_of::<Tcphdr>());
            let mut th = th_load(m, thoff);

            // Fill in fields, remembering maximum advertised window for use in delaying
            // messages about window sizes. If resending a FIN, be sure not to use a new
            // sequence number.
            if flags & TH_FIN != 0
                && tp.has_flags(TF_SENTFIN)
                && tp.snd_nxt.get() == tp.snd_max.get()
            {
                tp.snd_nxt.set(tp.snd_nxt.get().wrapping_sub(1));
            }
            // If we are doing retransmissions, then snd_nxt will not reflect the first unsent
            // octet. For ACK only packets, we do not want the sequence number of the
            // retransmitted packet, we want the sequence number of the next unsent octet.
            // So, if there is no data (and no SYN or FIN), use snd_max instead of snd_nxt
            // when filling in ti_seq. But if we are in persist state, snd_max might reflect
            // one byte beyond the right edge of the window, so use snd_nxt in that case,
            // since we know we aren't doing a retransmission. (retransmit and persist are
            // mutually exclusive...)
            if len != 0 || flags & (TH_SYN | TH_FIN) != 0 || tcp_timer_isarmed(tp, TCPT_PERSIST) {
                th.th_seq = htonl(tp.snd_nxt.get());
            } else {
                th.th_seq = htonl(tp.snd_max.get());
            }

            if let Some(p) = sack_rxmit {
                // If sendalot was turned on (due to option stuffing), turn it off. Properly
                // set th_seq field. Advance the ret'x pointer by len.
                sendalot = false;
                th.th_seq = htonl(p.rxmit.get());
                p.rxmit.set(p.rxmit.get().wrapping_add(len as u32));
                tcpstat_pkt(
                    TcpstatCounters::TcpsSackRexmits,
                    TcpstatCounters::TcpsSackRexmitBytes,
                    len as u64,
                );
            }

            th.th_ack = htonl(tp.rcv_nxt.get());
            if optlen != 0 {
                m_hdr_write(m, thoff + size_of::<Tcphdr>(), &opt[..optlen as usize]);
                th.set_th_off(((size_of::<Tcphdr>() as u32 + optlen) >> 2) as u8);
            }
            // TCP_ECN
            if do_ecn {
                // if we have received congestion experienced segs, set ECE bit.
                if tp.has_flags(TF_RCVD_CE) {
                    flags |= TH_ECE;
                    tcpstat_inc(TcpstatCounters::TcpsEcnSndece);
                }
                if !tp.has_flags(TF_DISABLE_ECN) {
                    // if this is a SYN seg, set ECE and CWR. set only ECE for SYN-ACK if peer
                    // supports ECN.
                    if flags & (TH_SYN | TH_ACK) == TH_SYN {
                        flags |= TH_ECE | TH_CWR;
                    } else if tp.has_flags(TF_ECN_PERMIT)
                        && flags & (TH_SYN | TH_ACK) == (TH_SYN | TH_ACK)
                    {
                        flags |= TH_ECE;
                    }
                }
                // if we have reduced the congestion window, notify the peer by setting CWR
                // bit.
                if tp.has_flags(TF_ECN_PERMIT) && tp.has_flags(TF_SEND_CWR) {
                    flags |= TH_CWR;
                    tp.clear_flags(TF_SEND_CWR);
                    tcpstat_inc(TcpstatCounters::TcpsEcnSndcwr);
                }
            }
            th.th_flags = flags;

            // Calculate receive window. Don't shrink window, but avoid silly window syndrome.
            if win < rcv_hiwat / 4 && win < i64::from(tp.t_maxseg.get()) {
                win = 0;
            }
            if win > i64::from(TCP_MAXWIN) << tp.rcv_scale.get() {
                win = i64::from(TCP_MAXWIN) << tp.rcv_scale.get();
            }
            let advertised = i64::from(tp.rcv_adv.get().wrapping_sub(tp.rcv_nxt.get()) as i32);
            if win < advertised {
                win = advertised;
            }
            if flags & TH_RST != 0 {
                win = 0;
            }
            th.th_win = htons((win >> tp.rcv_scale.get()) as u16);
            if th.th_win == 0 {
                tp.t_sndzerowin.set(tp.t_sndzerowin.get().wrapping_add(1));
            }
            if seq_gt(tp.snd_up.get(), tp.snd_nxt.get()) {
                let urp = min(
                    tp.snd_up.get().wrapping_sub(tp.snd_nxt.get()),
                    IP_MAXPACKET as u32,
                );
                th.th_urp = htons(urp as u16);
                th.th_flags |= TH_URG;
            } else {
                // If no urgent pointer to send, then we pull the urgent pointer to the left
                // edge of the send window so that it doesn't drift into the send window on
                // sequence number wraparound.
                tp.snd_up.set(tp.snd_una.get()); // drag it along
            }
            th_store(m, thoff, &th);

            // TCP_SIGNATURE
            if tp.has_flags(TF_SIGNATURE) {
                // tp->pf is 0 (default to PF_INET), AF_INET or AF_INET6: the switch above
                // returned EPFNOSUPPORT for any other.
                let (iphlen, src, dst) = match tp.pf.get() {
                    #[cfg(feature = "inet6")]
                    pf if pf == i32::from(AF_INET6) => {
                        let ip6 = mtod_ip6(m);
                        let su = |addr: In6Addr| {
                            let sin6 = SockaddrIn6::with_addr(addr);
                            let mut su = SockaddrUnion::new();
                            // SAFETY: `SockaddrIn6` is `#[repr(C)]` without padding, as
                            // large as the union.
                            su.as_bytes_mut().copy_from_slice(unsafe {
                                slice::from_raw_parts(
                                    ptr::from_ref(&sin6).cast::<u8>(),
                                    size_of::<SockaddrIn6>(),
                                )
                            });
                            su
                        };
                        (size_of::<Ip6Hdr>() as i32, su(ip6.ip6_src), su(ip6.ip6_dst))
                    }
                    _ => {
                        let ip = mtod_ip(m);
                        let su = |addr: InAddr| {
                            SockaddrUnion::from_sin(&SockaddrIn {
                                sin_len: size_of::<SockaddrIn>() as u8,
                                sin_family: AF_INET,
                                sin_addr: addr,
                                ..SockaddrIn::default()
                            })
                        };
                        (size_of::<Ip>() as i32, su(ip.ip_src), su(ip.ip_dst))
                    }
                };

                let Some(tdb) = gettdbbysrcdst(
                    rtable_l2(inp.inp_rtableid.get()),
                    0,
                    &src,
                    &dst,
                    IPPROTO_TCP as u8,
                ) else {
                    m_freem(m);
                    return Err(Errno::EPERM);
                };

                let mut sig = [0u8; 16];
                if !tcp_signature(tdb, tp.pf.get(), m, &th, iphlen, false, &mut sig) {
                    m_freem(m);
                    tdb_unref(Some(tdb));
                    return Err(Errno::EINVAL);
                }
                m_hdr_write(m, (hdrlen - optlen + sigoff) as usize, &sig);
                tdb_unref(Some(tdb));
            }

            // Defer checksumming until later (ip_output() or hardware)
            let ph = m.m_pkthdr();
            ph.csum_flags.set(ph.csum_flags.get() | M_TCP_CSUM_OUT);

            // In transmit state, time the transmission and arrange for the retransmit. In
            // persist state, just set snd_max.
            if !tp.t_force.get() || !tcp_timer_isarmed(tp, TCPT_PERSIST) {
                let startseq = tp.snd_nxt.get();

                // Advance snd_nxt over sequence space of this segment.
                if flags & (TH_SYN | TH_FIN) != 0 {
                    if flags & TH_SYN != 0 {
                        tp.snd_nxt.set(tp.snd_nxt.get().wrapping_add(1));
                    }
                    if flags & TH_FIN != 0 {
                        tp.snd_nxt.set(tp.snd_nxt.get().wrapping_add(1));
                        tp.set_flags(TF_SENTFIN);
                    }
                }
                // goto timer: a SACK retransmission that does not end at snd_nxt.
                let skip = tp.sack_enable.get() != 0
                    && sack_rxmit.is_some_and(|p| p.rxmit.get() != tp.snd_nxt.get());
                if !skip {
                    tp.snd_nxt.set(tp.snd_nxt.get().wrapping_add(len as u32));
                    if seq_gt(tp.snd_nxt.get(), tp.snd_max.get()) {
                        tp.snd_max.set(tp.snd_nxt.get());
                        // Time this transmission if not a retransmission and not currently
                        // timing anything.
                        if tp.t_rtttime.get() == 0 {
                            tp.t_rtttime.set(now);
                            tp.t_rtseq.set(startseq);
                            tcpstat_inc(TcpstatCounters::TcpsSegstimed);
                        }
                    }
                }

                // Set retransmit timer if not currently set, and not doing an ack or a
                // keep-alive probe. Initial value for retransmit timer is smoothed round-trip
                // time + 2 * round-trip time variance. Initialize shift counter which is used
                // for backoff of retransmit time.
                // timer:
                if tp.sack_enable.get() != 0
                    && sack_rxmit.is_some()
                    && !tcp_timer_isarmed(tp, TCPT_REXMT)
                    && tp.snd_nxt.get() != tp.snd_max.get()
                {
                    tcp_timer_arm(tp, TCPT_REXMT, tp.t_rxtcur.get() as u64);
                    if tcp_timer_isarmed(tp, TCPT_PERSIST) {
                        tcp_timer_disarm(tp, TCPT_PERSIST);
                        tp.t_rxtshift.set(0);
                    }
                }

                if !tcp_timer_isarmed(tp, TCPT_REXMT) && tp.snd_nxt.get() != tp.snd_una.get() {
                    tcp_timer_arm(tp, TCPT_REXMT, tp.t_rxtcur.get() as u64);
                    if tcp_timer_isarmed(tp, TCPT_PERSIST) {
                        tcp_timer_disarm(tp, TCPT_PERSIST);
                        tp.t_rxtshift.set(0);
                    }
                }

                if len == 0
                    && so.so_snd.sb_cc.get() != 0
                    && !tcp_timer_isarmed(tp, TCPT_REXMT)
                    && !tcp_timer_isarmed(tp, TCPT_PERSIST)
                {
                    // Avoid a situation where we do not set persist timer after a zero window
                    // condition. For example:
                    // 1) A -> B: packet with enough data to fill the window
                    // 2) B -> A: ACK for #1 + new data (0 window advertisement)
                    // 3) A -> B: ACK for #2, 0 len packet
                    //
                    // In this case, A will not activate the persist timer, because it chose
                    // to send a packet. Unless tcp_output is called for some other reason
                    // (delayed ack timer, another input packet from B, socket syscall), A
                    // will not send zero window probes.
                    //
                    // So, if you send a 0-length packet, but there is data in the socket
                    // buffer, and neither the rexmt or persist timer is already set, then
                    // activate the persist timer.
                    tp.t_rxtshift.set(0);
                    tcp_setpersist(tp);
                }
            } else if seq_gt(tp.snd_nxt.get().wrapping_add(len as u32), tp.snd_max.get()) {
                tp.snd_max.set(tp.snd_nxt.get().wrapping_add(len as u32));
            }

            tcp_update_sndspace(tp);

            // Trace.
            if so.so_options.get() & SO_DEBUG != 0 {
                // SAFETY: the first mbuf holds the `hdrlen` bytes of the IP and TCP headers
                // (its `m_len` is at least that).
                let headers = unsafe { slice::from_raw_parts(mtod::<u8>(m), hdrlen as usize) };
                tcp_trace(
                    TA_OUTPUT,
                    tp.t_state.get(),
                    Some(tp),
                    ptr::from_ref(tp),
                    Some(headers),
                    0,
                    len as i32,
                );
            }

            // Fill in IP length and desired time to live and send to IP level. There should
            // be a better way to handle ttl and tos; we could keep them in the template, but
            // need a way to checksum without them.

            // TCP_ECN: if peer is ECN capable, set the ECT bit in the IP header. but don't
            // set ECT for a pure ack, a retransmit or a window probe.
            let mut needect = false;
            if do_ecn && tp.has_flags(TF_ECN_PERMIT) {
                if len == 0
                    || seq_lt(tp.snd_nxt.get(), tp.snd_max.get())
                    || (tp.t_force.get() && len == 1)
                {
                    // don't set ECT
                } else {
                    needect = true;
                    tcpstat_inc(TcpstatCounters::TcpsEcnSndect);
                }
            }

            // force routing table
            m.m_pkthdr().ph_rtableid.set(inp.inp_rtableid.get());

            // NPF
            pf_mbuf_link_inpcb(m, Some(inp));
            // NSTOEPLITZ
            m.m_pkthdr().ph_flowid.set(inp.inp_flowid.get());
            let ph = m.m_pkthdr();
            ph.csum_flags.set(ph.csum_flags.get() | M_FLOWID);

            #[cfg(feature = "inet6")]
            if tp.pf.get() == i32::from(AF_INET6) {
                let mut ip6 = mtod_ip6(m);
                ip6.ip6_plen = (m.m_pkthdr().len.get() as usize - size_of::<Ip6Hdr>()) as u16;
                packetlen = m.m_pkthdr().len.get() as u32;
                ip6.ip6_nxt = IPPROTO_TCP as u8;
                ip6.ip6_hlim = in6_selecthlim(inp) as u8;
                if needect {
                    ip6.ip6_flow |= htonl(u32::from(IPTOS_ECN_ECT0) << 20);
                }
                mtod_ip6_store(m, &ip6);
                // SAFETY: the options are the socket's own allocation (`ip6_setpktopts`),
                // freed only under the socket lock `tcp_output` runs with.
                let opts = inp.inp_outputopts6.get().map(|o| unsafe { o.as_ref() });
                break 'out ip6_output(
                    m,
                    opts,
                    Some(&inp.inp_route),
                    0,
                    None,
                    Some(&inp.inp_seclevel.get()),
                );
            }
            // case 0: default to PF_INET; AF_INET
            let mut ip = mtod_ip(m);
            ip.ip_len = htons(m.m_pkthdr().len.get() as u16);
            packetlen = m.m_pkthdr().len.get() as u32;
            ip.ip_ttl = inp.inp_ip.get().ip_ttl;
            ip.ip_tos = inp.inp_ip.get().ip_tos;
            if needect {
                ip.ip_tos |= IPTOS_ECN_ECT0;
            }
            mtod_ip_store(m, &ip);
            ip_output(
                m,
                inp.inp_options.get(),
                Some(&inp.inp_route),
                if ip_mtudisc.load(Ordering::Relaxed) != 0 {
                    IP_MTUDISC
                } else {
                    0
                },
                None,
                Some(&inp.inp_seclevel.get()),
                0,
            )
        };

        if let Err(error) = error {
            // out:
            match error {
                Errno::ENOBUFS => {
                    // If the interface queue is full, or IP cannot get an mbuf, trigger TCP
                    // slow start.
                    tp.snd_cwnd.set(u64::from(tp.t_maxseg.get()));
                    return Ok(());
                }
                Errno::EMSGSIZE => {
                    // ip_output() will have already fixed the route for us. tcp_mtudisc()
                    // will, as its last action, initiate retransmission, so it is important
                    // to not do so here.
                    tcp_mtudisc(inp, None);
                    return Ok(());
                }
                Errno::EHOSTUNREACH | Errno::ENETDOWN if tcps_havercvdsyn(tp.t_state.get()) => {
                    tp.t_softerror.set(Some(error));
                    return Ok(());
                }
                _ => {}
            }

            // Restart the delayed ACK timer, if necessary.
            if tcp_timer_isarmed(tp, TCPT_DELACK) {
                tcp_timer_arm(tp, TCPT_DELACK, tcp_delack_msecs as u64);
            }

            return Err(error);
        }

        if packetlen > tp.t_pmtud_mtu_sent.get() {
            tp.t_pmtud_mtu_sent.set(packetlen);
        }

        tcpstat_inc(TcpstatCounters::TcpsSndtotal);
        if tcp_timer_isarmed(tp, TCPT_DELACK) {
            tcpstat_inc(TcpstatCounters::TcpsDelack);
        }

        // Data sent (as far as we can tell). If this advertises a larger window than any
        // other segment, then remember the size of the advertised window. Any pending ACK has
        // now been sent.
        if win > 0 && seq_gt(tp.rcv_nxt.get().wrapping_add(win as u32), tp.rcv_adv.get()) {
            tp.rcv_adv.set(tp.rcv_nxt.get().wrapping_add(win as u32));
        }
        tp.last_ack_sent.set(tp.rcv_nxt.get());
        tp.t_sndacktime.set(now);
        tp.clear_flags(TF_ACKNOW);
        tcp_timer_disarm(tp, TCPT_DELACK);
        if !sendalot {
            return Ok(());
        }
    }
}

/// `tcp_setpersist`: start or restart the persistence timer, backed off by `t_rxtshift`
/// within `TCPTV_PERSMIN`..`TCPTV_PERSMAX`.
pub fn tcp_setpersist(tp: &Tcpcb) {
    let mut t = ((tp.t_srtt.get() >> 2) + tp.t_rttvar.get()) >> (1 + TCP_RTT_BASE_SHIFT);

    if tcp_timer_isarmed(tp, TCPT_REXMT) {
        panic(format_args!("tcp_output REXMT"));
    }
    // Start/restart persistence timer.
    if (t as u32) < tp.t_rttmin.get() {
        t = tp.t_rttmin.get() as i32;
    }
    let msec = tcpt_rangeset(
        t * TCP_BACKOFF[tp.t_rxtshift.get() as usize],
        TCPTV_PERSMIN,
        TCPTV_PERSMAX,
    );
    tcp_timer_arm(tp, TCPT_PERSIST, msec as u64);
    if i32::from(tp.t_rxtshift.get()) < TCP_MAXRXTSHIFT {
        tp.t_rxtshift.set(tp.t_rxtshift.get() + 1);
    }
}

/// `tcp_softtso_chop`: cuts the TSO packet `m0` (an IPv4 TCP segment without IP options or
/// fragmentation) into segments of at most `mss` bytes of payload on `ml`, with their own IP
/// and TCP headers and checksums for `ifp`. Consumes `m0`; on error `ml` is purged.
pub fn tcp_softtso_chop(
    ml: &MbufList,
    m0: &'static Mbuf,
    ifp: &'static Ifnet,
    mss: u32,
) -> Result<(), Errno> {
    let mut m0 = m0;

    ml_init(ml);
    ml_enqueue(ml, m0);

    let error: Errno = 'bad: {
        if mss == 0 {
            break 'bad Errno::EINVAL;
        }

        let ip0 = mtod_ip(m0);
        let (ip, ip6, iphlen): (Option<Ip>, Option<Ip6Hdr>, usize) = match ip0.ip_v() {
            4 => {
                let iphlen = usize::from(ip0.ip_hl()) << 2;
                if ip0.ip_off & htons(IP_OFFMASK | IP_MF) != 0
                    || iphlen != size_of::<Ip>()
                    || i32::from(ip0.ip_p) != IPPROTO_TCP
                {
                    // only TCP without fragment or IP option supported
                    break 'bad Errno::EPROTOTYPE;
                }
                (Some(ip0), None, iphlen)
            }
            #[cfg(feature = "inet6")]
            6 => {
                let ip6 = mtod_ip6(m0);
                if i32::from(ip6.ip6_nxt) != IPPROTO_TCP {
                    // only TCP without IPv6 header chain supported
                    break 'bad Errno::EPROTOTYPE;
                }
                (None, Some(ip6), size_of::<Ip6Hdr>())
            }
            v => panic(format_args!("tcp_softtso_chop: unknown ip version {}", v)),
        };
        #[cfg(not(feature = "inet6"))]
        let _ = ip6; // always `None` without INET6

        let tlen = m0.m_pkthdr().len.get();
        if (tlen as usize) < iphlen + size_of::<Tcphdr>() {
            break 'bad Errno::ENOPROTOOPT;
        }
        // IP and TCP header should be contiguous, this check is paranoia
        if (m0.m_len().get() as usize) < iphlen + size_of::<Tcphdr>() {
            let _ = ml_dequeue(ml);
            let Some(m) = m_pullup(m0, (iphlen + size_of::<Tcphdr>()) as i32) else {
                break 'bad Errno::ENOBUFS;
            };
            m0 = m;
            ml_enqueue(ml, m0);
        }
        let mut th = th_load(m0, iphlen);
        let hlen = (iphlen + (usize::from(th.th_off()) << 2)) as i32;
        if tlen < hlen {
            break 'bad Errno::ENOPROTOOPT;
        }
        let firstlen = min((tlen - hlen) as u32, mss) as i32;

        let ph = m0.m_pkthdr();
        ph.csum_flags.set(ph.csum_flags.get() & !M_TCP_TSO);

        // Loop through length of payload after first segment, make new header and copy data
        // of each part and link onto chain.
        let mut off = hlen + firstlen;
        while off < tlen {
            let len = min((tlen - off) as u32, mss) as i32;

            let Some(m) = m_gethdr(M_DONTWAIT, MT_HEADER) else {
                break 'bad Errno::ENOBUFS;
            };
            ml_enqueue(ml, m);
            if let Err(e) = m_dup_pkthdr(m, m0, M_DONTWAIT) {
                break 'bad e;
            }

            // IP and TCP header to the end, space for link layer header
            m.m_len().set(hlen as u32);
            m_align(m, hlen);

            // copy and adjust TCP header
            // SAFETY: `m_align` placed `hlen` bytes of data in the new mbuf, `m_len` covers
            // them.
            let thdst = unsafe {
                slice::from_raw_parts_mut(mtod::<u8>(m).add(iphlen), hlen as usize - iphlen)
            };
            m_copydata(m0, iphlen as i32, thdst);
            let mut mhth = th_load(m, iphlen);
            mhth.th_seq = htonl(ntohl(th.th_seq).wrapping_add((off - hlen) as u32));
            if off + len < tlen {
                mhth.th_flags &= !(TH_PUSH | TH_FIN);
            }
            th_store(m, iphlen, &mhth);

            // add mbuf chain with payload
            m.m_pkthdr().len.set(hlen + len);
            let next = m_copym(m0, off, len, M_DONTWAIT);
            m.m_next().set(next);
            if next.is_none() {
                break 'bad Errno::ENOBUFS;
            }

            // copy and adjust IP header, calculate checksum
            let ph = m.m_pkthdr();
            ph.csum_flags.set(ph.csum_flags.get() | M_TCP_CSUM_OUT);
            if let Some(ip) = ip {
                let mut mhip = ip;
                mhip.ip_len = htons((hlen + len) as u16);
                mhip.ip_id = htons(ip_randomid());
                mtod_ip_store(m, &mhip);
                in_hdr_cksum_out(m, Some(ifp));
                in_proto_cksum_out(m, Some(ifp));
            }
            #[cfg(feature = "inet6")]
            if let Some(ip6) = ip6 {
                let mut mhip6 = ip6;
                mhip6.ip6_plen = htons((hlen - iphlen as i32 + len) as u16);
                mtod_ip6_store(m, &mhip6);
                in6_proto_cksum_out(m, Some(ifp));
            }

            off += mss as i32;
        }

        // Update first segment by trimming what's been copied out and updating header, then
        // send each segment (in order).
        if hlen + firstlen < tlen {
            m_adj(m0, hlen + firstlen - tlen);
            th.th_flags &= !(TH_PUSH | TH_FIN);
            th_store(m0, iphlen, &th);
        }
        // adjust IP header, calculate checksum
        let ph = m0.m_pkthdr();
        ph.csum_flags.set(ph.csum_flags.get() | M_TCP_CSUM_OUT);
        if let Some(mut ip) = ip {
            ip.ip_len = htons(m0.m_pkthdr().len.get() as u16);
            mtod_ip_store(m0, &ip);
            in_hdr_cksum_out(m0, Some(ifp));
            in_proto_cksum_out(m0, Some(ifp));
        }
        #[cfg(feature = "inet6")]
        if let Some(mut ip6) = ip6 {
            ip6.ip6_plen = htons((m0.m_pkthdr().len.get() as usize - iphlen) as u16);
            mtod_ip6_store(m0, &ip6);
            in6_proto_cksum_out(m0, Some(ifp));
        }

        tcpstat_add(TcpstatCounters::TcpsOutpkttso, u64::from(ml_len(ml)));
        return Ok(());
    };

    // bad:
    tcpstat_inc(TcpstatCounters::TcpsOutbadtso);
    let _ = ml_purge(ml);
    Err(error)
}

/// `tcp_if_output_tso`: sends the TSO packet `*mp` on `ifp`, by the hardware when it can do
/// TSO (`ifcap`), else chopped by [`tcp_softtso_chop`]; `*mp` is consumed then. A packet
/// that did not ask for TSO, or whose segments exceed `mtu` (its TSO flag cleared), is left
/// in `*mp` for the caller to send, fragment or drop.
///
/// # Safety
///
/// As for `IfOutputFn` (`net/if_var.rs`): `dst` is a socket address readable for its
/// `sa_len` bytes, as `ifp`'s output routine reads it.
pub unsafe fn tcp_if_output_tso(
    ifp: &'static Ifnet,
    mp: &mut Option<&'static Mbuf>,
    dst: *const Sockaddr,
    rt: Option<&'static Rtentry>,
    ifcap: u32,
    mtu: u32,
) -> Result<(), Errno> {
    let Some(m) = *mp else {
        return Ok(());
    };
    let ph = m.m_pkthdr();

    // caller must fail later or fragment
    if ph.csum_flags.get() & M_TCP_TSO == 0 {
        return Ok(());
    }
    if u32::from(ph.ph_mss.get()) > mtu {
        ph.csum_flags.set(ph.csum_flags.get() & !M_TCP_TSO);
        return Ok(());
    }

    let error: Result<(), Errno> = 'done: {
        // network interface hardware will do TSO
        if in_ifcap_cksum(m, Some(ifp), ifcap) {
            if ifcap & IFCAP_TSOv4 != 0 {
                in_hdr_cksum_out(m, Some(ifp));
                in_proto_cksum_out(m, Some(ifp));
            }
            #[cfg(feature = "inet6")]
            if ifcap & IFCAP_TSOv6 != 0 {
                in6_proto_cksum_out(m, Some(ifp));
            }
            let error = match ifp.if_output.get() {
                // SAFETY: the caller's contract is the hook's.
                Some(output) => unsafe { output(ifp, m, dst, rt) },
                None => panic(format_args!("{}: no if_output", Str(&ifp.if_xname.get()))),
            };
            if error.is_ok() {
                tcpstat_inc(TcpstatCounters::TcpsOuthwtso);
            }
            break 'done error;
        }

        // as fallback do TSO in software
        let ml = MbufList::new();
        if let Err(e) = tcp_softtso_chop(&ml, m, ifp, u32::from(ph.ph_mss.get())) {
            break 'done Err(e);
        }
        // SAFETY: the caller's contract.
        if let Err(e) = unsafe { if_output_ml(ifp, &ml, dst, rt) } {
            break 'done Err(e);
        }
        tcpstat_inc(TcpstatCounters::TcpsOutswtso);
        Ok(())
    };

    // done:
    *mp = None;
    error
}

#[cfg(test)]
mod tests;
