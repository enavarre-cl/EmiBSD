/* <LICENSES> */
/*
 * Copyright (c) 1982, 1986, 1988, 1993
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
//! TCP user requests and socket options: `netinet/tcp_usrreq.c`.
//!
//! Upstream: sys/netinet/tcp_usrreq.c @ 3ce1f3f79392
//!
//! The `pr_usrreqs` of TCP sockets: attaching allocates the internet and TCP control blocks
//! and the socket buffers; listen, connect (send a SYN), accept, send (append to the send
//! buffer and call `tcp_output`), shutdown and disconnect (walk the FIN states through
//! `tcp_usrclosed`), abort (`tcp_drop`), out-of-band data, the addresses. `tcp_ctloutput`
//! handles the `IPPROTO_TCP` socket options (`TCP_NODELAY`, `TCP_NOPUSH`, `TCP_MAXSEG`,
//! `TCP_SACK_ENABLE`, `TCP_INFO`, `TCP_MD5SIG`) and passes the rest to IP. `tcp_sysctl` is
//! `net.inet.tcp`, `tcp_ident` its `ident` and `drop` requests. `tcp_update_sndspace` and
//! `tcp_update_rcvspace` scale the socket buffers with the connection.
//!
//! Locks used to protect global variables in this file: \[I\] immutable after creation.
//!
//! ## Deviations
//! - `tcp_sogetpcb` returns the pair or the errno; the user requests keep the C's
//!   `SO_DEBUG` tracing with `otp` as the address of the control block before the request
//!   (`tcp_trace` never dereferences it: the request may have freed the block).
//! - `tcp_sendspace`, `tcp_recvspace` and `tcp_autorcvbuf_inc` are \[I\] `u32` statics; the
//!   first two keep their lowercase names (`TCP_SENDSPACE`/`TCP_RECVSPACE` are this file's
//!   macros). `tcpctl_vars[]` points at the `AtomicI32`s of the files that define them.
//! - `tcp_fill_info` builds a `TcpInfo` and copies it into the mbuf unaligned (the
//!   structure is integers without padding); `MCLGETL` is `mclgetl`.
//! - `tcp_ident`'s `struct tcp_ident_mapping` is copied in and out whole (`AbiPod`, its
//!   trailing hole named); the C's `copyin`/`copyout` of the same bytes.
//! - `tcp_sysctl`'s port bitmaps are copied through a byte buffer on the stack (the C's
//!   `malloc(M_SYSCTL)`), as `udp_sysctl` does; `tcp_sysctl_tcpstat` writes `struct tcpstat`
//!   member by member at its `offset_of!` positions (the structure has holes before its
//!   64-bit members).
//! - `INET6` is configured (feature `inet6`): the `PF_INET6` attach, `ip6_ctloutput`, the
//!   IPv6 checks of `tcp_connect` and `tcp_ident`'s `AF_INET6` case. `tcp6_usrreqs` and
//!   `tcb6table` compile always, as `netinet6` does (`inet6sw` names the former), like
//!   `route6_mpath`. `TCP_ECN` and `TCP_SIGNATURE` are configured. `SMALL_KERNEL` is not set: `tcpctl_vars`,
//!   `tcp_sysctl_tcpstat` and `tcp_sysctl` are compiled.

use core::mem::{offset_of, size_of};
use core::ptr;
use core::sync::atomic::{AtomicI32, Ordering};

use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_prot::suser;
use crate::kern::kern_rwlock::{rw_enter, rw_exit};
use crate::kern::kern_sysctl::{
    SECURELEVEL, SYSCTL_LOCK, sysctl_bounded_arr, sysctl_int, sysctl_int_bounded, sysctl_rdstruct,
    sysctl_struct,
};
use crate::kern::subr_pool::pool_sethardlimit;
use crate::kern::subr_prf::panic;
use crate::kern::uipc_mbuf::m_freem;
use crate::kern::uipc_socket2::{
    SB_MAX_VAR, sbappendstream, sbchecklowmem, sbcheckreserve, sbflush, sbreserve, soassertlocked,
    socantsendmore, soisconnecting, soisdisconnected, soisdisconnecting, soreserve,
};
use crate::machine::copy::{copyin_obj, copyout_obj};
use crate::machine::cpu::curproc;
use crate::netinet::in_::{
    INADDR_ANY, INADDR_BROADCAST, IPPROTO_TCP, SockaddrIn, in_broadcast, in_control, in_multicast,
    in_nam2sin,
};
#[cfg(feature = "inet6")]
use crate::netinet::in_pcb::INP_IPV6;
use crate::netinet::in_pcb::{
    BADDYNAMICPORTS, DP_MAPSIZE, Inpcb, Inpcbtable, ROOTONLYPORTS, in_flowid, in_pcballoc,
    in_pcbbind, in_pcbconnect, in_pcbdetach, in_pcbdisconnect, in_pcblookup, in_pcblookup_listen,
    in_pcbsolock, in_pcbsounlock, in_pcbunref, in_pcbunset_faddr, in_setpeeraddr, in_setsockaddr,
    sotoinpcb,
};
use crate::netinet::ip_output::ip_ctloutput;
use crate::netinet::tcp::{
    TCP_INFO, TCP_MAXSEG, TCP_MD5SIG, TCP_MSS, TCP_NODELAY, TCP_NOPUSH, TCP_SACK_ENABLE,
    TCPI_OPT_ECN, TCPI_OPT_SACK, TCPI_OPT_TIMESTAMPS, TCPI_OPT_WSCALE, TcpInfo,
};
use crate::netinet::tcp_debug::{TA_USER, tcp_trace};
use crate::netinet::tcp_fsm::{
    TCPS_CLOSE_WAIT, TCPS_CLOSED, TCPS_ESTABLISHED, TCPS_FIN_WAIT_1, TCPS_FIN_WAIT_2,
    TCPS_LAST_ACK, TCPS_LISTEN, TCPS_SYN_RECEIVED, TCPS_SYN_SENT, tcps_haveestablished,
};
use crate::netinet::tcp_input::{
    SYN_CACHE_MTX, TCP_RST_PPSLIM, TCP_SYN_BUCKET_LIMIT, TCP_SYN_CACHE, TCP_SYN_CACHE_ACTIVE,
    TCP_SYN_CACHE_LIMIT, TCP_SYN_USE_LIMIT, tcp_syn_hash_size,
};
use crate::netinet::tcp_output::tcp_output;
use crate::netinet::tcp_seq::tcp_sendseqinit;
use crate::netinet::tcp_subr::{
    SACKHL_POOL, TCP_ACK_ON_PUSH, TCP_DO_ECN, TCP_DO_RFC1323, TCP_DO_RFC3390, TCP_DO_SACK,
    TCP_DO_TSO, TCP_MSSDFLT, TCP_REASS_LIMIT, TCPCOUNTERS, TCPQE_POOL, tcp_close, tcp_drop,
    tcp_newtcpcb, tcp_rscale, tcp_sackhole_limit, tcp_set_iss_tsm, tcp_template,
};
use crate::netinet::tcp_timer::{
    TCP_ALWAYS_KEEPALIVE, TCP_KEEPIDLE, TCP_KEEPIDLE_SEC, TCP_KEEPINIT, TCP_KEEPINIT_SEC,
    TCP_KEEPINTVL, TCP_KEEPINTVL_SEC, TCP_LINGERTIME, TCPT_2MSL, TCPT_KEEP, TCPTV_KEEPCNT,
    TCPTV_KEEPIDLE, TCPTV_KEEPINIT, TCPTV_KEEPINTVL, tcp_timer_arm,
};
use crate::netinet::tcp_var::{
    TCP_RTT_BASE_SHIFT, TCP_RTT_SHIFT, TCP_RTTVAR_SHIFT, TCPCTL_ACK_ON_PUSH,
    TCPCTL_ALWAYS_KEEPALIVE, TCPCTL_BADDYNAMIC, TCPCTL_DROP, TCPCTL_ECN, TCPCTL_IDENT,
    TCPCTL_KEEPIDLE, TCPCTL_KEEPINITTIME, TCPCTL_KEEPINTVL, TCPCTL_MSSDFLT, TCPCTL_REASS_LIMIT,
    TCPCTL_RFC1323, TCPCTL_RFC3390, TCPCTL_ROOTONLY, TCPCTL_RSTPPSLIMIT, TCPCTL_SACK,
    TCPCTL_SACKHOLE_LIMIT, TCPCTL_STATS, TCPCTL_SYN_BUCKET_LIMIT, TCPCTL_SYN_CACHE_LIMIT,
    TCPCTL_SYN_HASH_SIZE, TCPCTL_SYN_USE_LIMIT, TCPCTL_TSO, TCPOOB_HADDATA, TCPOOB_HAVEDATA,
    TCPS_NCOUNTERS, TF_ECN_PERMIT, TF_NODELAY, TF_NOPUSH, TF_RCVD_SCALE, TF_RCVD_TSTMP,
    TF_REQ_SCALE, TF_REQ_TSTMP, TF_SACK_PERMIT, TF_SIGNATURE, TcpIdentMapping, Tcpcb, Tcpstat,
    TcpstatCounters, intotcpcb, tcp_now, tcp_time, tcpstat_inc,
};
use crate::netinet6::in6::in6_control;
#[cfg(feature = "inet6")]
use crate::netinet6::{
    in6::{
        IN6ADDR_ANY, In6Addr, SockaddrIn6, in6_is_addr_multicast, in6_is_addr_unspecified,
        in6_nam2sin6,
    },
    in6_pcb::{in6_pcblookup, in6_pcblookup_listen},
    in6_src::in6_embedscope,
    ip6_output::ip6_ctloutput,
};
use crate::sys::errno::Errno;
use crate::sys::mbuf::{M_EXT, M_WAIT, MLEN, Mbuf, mclgetl, mtod};
use crate::sys::proc::Proc;
use crate::sys::protosw::{
    PRCO_GETOPT, PRCO_SETOPT, PRU_ABORT, PRU_ACCEPT, PRU_ATTACH, PRU_BIND, PRU_CONNECT, PRU_DETACH,
    PRU_DISCONNECT, PRU_LISTEN, PRU_PEERADDR, PRU_RCVD, PRU_RCVOOB, PRU_SEND, PRU_SENDOOB,
    PRU_SENSE, PRU_SHUTDOWN, PRU_SOCKADDR, PrUsrreqs,
};
use crate::sys::rwlock::{RW_INTR, RW_WRITE};
use crate::sys::socket::{
    AF_INET, MSG_PEEK, PF_INET, SO_ACCEPTCONN, SO_DEBUG, SO_LINGER, SO_OOBINLINE,
};
#[cfg(feature = "inet6")]
use crate::sys::socket::{AF_INET6, PF_INET6};
use crate::sys::socketvar::{
    SS_CANTSENDMORE, SS_CONNECTOUT, SS_ISCONNECTED, SS_ISCONNECTING, SS_NOFDREF, SS_RCVATMARK,
    Socket, sbspace, sbspace_locked,
};
use crate::sys::stat::Stat;
use crate::sys::sysctl::SysctlBoundedArgs;
use crate::sys::systm::{net_lock, net_lock_shared, net_unlock, net_unlock_shared};

/// `TCP_SENDSPACE`.
const TCP_SENDSPACE: u32 = 1024 * 16;
/// `TCP_RECVSPACE`.
const TCP_RECVSPACE: u32 = 1024 * 16;

/// `tcp_usrreqs`.
pub static TCP_USRREQS: PrUsrreqs = PrUsrreqs {
    pru_attach: Some(tcp_attach),
    pru_detach: Some(tcp_detach),
    pru_bind: Some(tcp_bind),
    pru_listen: Some(tcp_listen),
    pru_connect: Some(tcp_connect),
    pru_accept: Some(tcp_accept),
    pru_disconnect: Some(tcp_disconnect),
    pru_shutdown: Some(tcp_shutdown),
    pru_rcvd: Some(tcp_rcvd),
    pru_send: Some(tcp_send),
    pru_abort: Some(tcp_abort),
    pru_sense: Some(tcp_sense),
    pru_rcvoob: Some(tcp_rcvoob),
    pru_sendoob: Some(tcp_sendoob),
    pru_control: Some(in_control),
    pru_sockaddr: Some(tcp_sockaddr),
    pru_peeraddr: Some(tcp_peeraddr),
    pru_flowid: Some(in_flowid),
    ..PrUsrreqs::NONE
};

/// `tcp6_usrreqs` (`INET6`; compiled always, as `netinet6` is, for `inet6sw`).
pub static TCP6_USRREQS: PrUsrreqs = PrUsrreqs {
    pru_attach: Some(tcp_attach),
    pru_detach: Some(tcp_detach),
    pru_bind: Some(tcp_bind),
    pru_listen: Some(tcp_listen),
    pru_connect: Some(tcp_connect),
    pru_accept: Some(tcp_accept),
    pru_disconnect: Some(tcp_disconnect),
    pru_shutdown: Some(tcp_shutdown),
    pru_rcvd: Some(tcp_rcvd),
    pru_send: Some(tcp_send),
    pru_abort: Some(tcp_abort),
    pru_sense: Some(tcp_sense),
    pru_rcvoob: Some(tcp_rcvoob),
    pru_sendoob: Some(tcp_sendoob),
    pru_control: Some(in6_control),
    pru_sockaddr: Some(tcp_sockaddr),
    pru_peeraddr: Some(tcp_peeraddr),
    pru_flowid: Some(in_flowid),
    ..PrUsrreqs::NONE
};

/// \[I\] `tcp_sendspace`.
#[allow(non_upper_case_globals)] // TCP_SENDSPACE is a constant of this file
pub static tcp_sendspace: u32 = TCP_SENDSPACE;
/// \[I\] `tcp_recvspace`.
#[allow(non_upper_case_globals)] // TCP_RECVSPACE is a constant of this file
pub static tcp_recvspace: u32 = TCP_RECVSPACE;
/// \[I\] `tcp_autorcvbuf_inc`.
pub static TCP_AUTORCVBUF_INC: u32 = 16 * 1024;

/// `tcpctl_vars[]`.
static TCPCTL_VARS: [SysctlBoundedArgs; 14] = [
    SysctlBoundedArgs::new(
        TCPCTL_KEEPINITTIME,
        &TCP_KEEPINIT_SEC,
        1,
        3 * TCPTV_KEEPINIT / tcp_time(1),
    ),
    SysctlBoundedArgs::new(
        TCPCTL_KEEPIDLE,
        &TCP_KEEPIDLE_SEC,
        1,
        5 * TCPTV_KEEPIDLE / tcp_time(1),
    ),
    SysctlBoundedArgs::new(
        TCPCTL_KEEPINTVL,
        &TCP_KEEPINTVL_SEC,
        1,
        3 * TCPTV_KEEPINTVL / tcp_time(1),
    ),
    SysctlBoundedArgs::new(TCPCTL_RFC1323, &TCP_DO_RFC1323, 0, 1),
    SysctlBoundedArgs::new(TCPCTL_SACK, &TCP_DO_SACK, 0, 1),
    SysctlBoundedArgs::new(TCPCTL_MSSDFLT, &TCP_MSSDFLT, TCP_MSS, 65535),
    SysctlBoundedArgs::new(TCPCTL_RSTPPSLIMIT, &TCP_RST_PPSLIM, 1, 1000 * 1000),
    SysctlBoundedArgs::new(TCPCTL_ACK_ON_PUSH, &TCP_ACK_ON_PUSH, 0, 1),
    // TCP_ECN
    SysctlBoundedArgs::new(TCPCTL_ECN, &TCP_DO_ECN, 0, 1),
    SysctlBoundedArgs::new(TCPCTL_SYN_CACHE_LIMIT, &TCP_SYN_CACHE_LIMIT, 1, 1000 * 1000),
    SysctlBoundedArgs::new(TCPCTL_SYN_BUCKET_LIMIT, &TCP_SYN_BUCKET_LIMIT, 1, i32::MAX),
    SysctlBoundedArgs::new(TCPCTL_RFC3390, &TCP_DO_RFC3390, 0, 2),
    SysctlBoundedArgs::new(TCPCTL_ALWAYS_KEEPALIVE, &TCP_ALWAYS_KEEPALIVE, 0, 1),
    SysctlBoundedArgs::new(TCPCTL_TSO, &TCP_DO_TSO, 0, 1),
];

/// `tcbtable`.
pub static TCBTABLE: Inpcbtable = Inpcbtable::new();
/// `tcb6table` (`INET6`; compiled always, for `tcp6_ctlinput`).
pub static TCB6TABLE: Inpcbtable = Inpcbtable::new();

/// `curproc`, which the socket requests run as.
fn curproc_or_panic(func: &str) -> &'static Proc {
    match curproc() {
        Some(p) => p,
        None => panic(format_args!("{}: no curproc", func)),
    }
}

/// `tcp_sogetpcb`: when a TCP is attached to a socket, then there will be a (struct inpcb)
/// pointed at by the socket, and this structure will point at a subsidiary (struct tcpcb).
fn tcp_sogetpcb(so: &Socket) -> Result<(&'static Inpcb, &'static Tcpcb), Errno> {
    match sotoinpcb(so).and_then(|inp| Some((inp, intotcpcb(inp)?))) {
        Some(pair) => Ok(pair),
        None => Err(so.error().unwrap_or(Errno::EINVAL)),
    }
}

/// `otp`/`ostate` of a user request: the control block and its state before the request,
/// when the socket has `SO_DEBUG`.
fn trace_start(so: &Socket, tp: &'static Tcpcb) -> Option<(*const Tcpcb, i32)> {
    so.has_options(SO_DEBUG)
        .then(|| (ptr::from_ref(tp), tp.t_state.get()))
}

/// The `tcp_trace(TA_USER, ostate, tp, otp, NULL, req, 0)` at the end of a request.
fn trace_end(start: Option<(*const Tcpcb, i32)>, tp: Option<&Tcpcb>, req: i32) {
    if let Some((otp, ostate)) = start {
        tcp_trace(TA_USER, ostate, tp, otp, None, req, 0);
    }
}

/// `tcp_fill_info`: export internal TCP state information via a struct tcp_info without
/// leaking any sensitive information. Sequence numbers are reported relative to the initial
/// sequence number.
fn tcp_fill_info(tp: &Tcpcb, so: &Socket, m: &'static Mbuf) -> Result<(), Errno> {
    let p = curproc_or_panic("tcp_fill_info");
    // msec => usec
    let t: u64 = 1000;

    if size_of::<TcpInfo>() > MLEN {
        let _ = mclgetl(m, M_WAIT, size_of::<TcpInfo>() as u32);
        if m.m_flags().get() & M_EXT == 0 {
            return Err(Errno::ENOMEM);
        }
    }
    let mut ti = TcpInfo::default();
    let now = tcp_now();
    let msec = |since: u64| (now.wrapping_sub(since).wrapping_mul(t)) as u32;

    ti.tcpi_state = tp.t_state.get() as u8;
    if tp.has_flags(TF_REQ_TSTMP) && tp.has_flags(TF_RCVD_TSTMP) {
        ti.tcpi_options |= TCPI_OPT_TIMESTAMPS;
    }
    if tp.has_flags(TF_SACK_PERMIT) {
        ti.tcpi_options |= TCPI_OPT_SACK;
    }
    if tp.has_flags(TF_REQ_SCALE) && tp.has_flags(TF_RCVD_SCALE) {
        ti.tcpi_options |= TCPI_OPT_WSCALE;
        ti.tcpi_snd_wscale = tp.snd_scale.get();
        ti.tcpi_rcv_wscale = tp.rcv_scale.get();
    }
    // TCP_ECN
    if tp.has_flags(TF_ECN_PERMIT) {
        ti.tcpi_options |= TCPI_OPT_ECN;
    }

    ti.tcpi_rto = (tp.t_rxtcur.get() as u64 * t) as u32;
    ti.tcpi_snd_mss = u32::from(tp.t_maxseg.get());
    ti.tcpi_rcv_mss = u32::from(tp.t_peermss.get());

    ti.tcpi_last_data_sent = msec(tp.t_sndtime.get());
    ti.tcpi_last_ack_sent = msec(tp.t_sndacktime.get());
    ti.tcpi_last_data_recv = msec(tp.t_rcvtime.get());
    ti.tcpi_last_ack_recv = msec(tp.t_rcvacktime.get());

    ti.tcpi_rtt = ((tp.t_srtt.get() as u64 * t) >> (TCP_RTT_SHIFT + TCP_RTT_BASE_SHIFT)) as u32;
    ti.tcpi_rttvar =
        ((tp.t_rttvar.get() as u64 * t) >> (TCP_RTTVAR_SHIFT + TCP_RTT_BASE_SHIFT)) as u32;
    ti.tcpi_snd_ssthresh = tp.snd_ssthresh.get() as u32;
    ti.tcpi_snd_cwnd = tp.snd_cwnd.get() as u32;

    ti.tcpi_rcv_space = tp.rcv_wnd.get() as u32;

    // Provide only minimal information for unprivileged processes.
    if suser(p).is_ok() {
        // FreeBSD-specific extension fields for tcp_info.
        ti.tcpi_snd_wnd = tp.snd_wnd.get() as u32;
        ti.tcpi_snd_nxt = tp.snd_nxt.get().wrapping_sub(tp.iss.get());
        ti.tcpi_rcv_nxt = tp.rcv_nxt.get().wrapping_sub(tp.irs.get());
        // missing tcpi_toe_tid
        ti.tcpi_snd_rexmitpack = tp.t_sndrexmitpack.get();
        ti.tcpi_rcv_ooopack = tp.t_rcvoopack.get();
        ti.tcpi_snd_zerowin = tp.t_sndzerowin.get();

        // OpenBSD extensions
        ti.tcpi_rttmin = (u64::from(tp.t_rttmin.get()) * t) as u32;
        ti.tcpi_max_sndwnd = tp.max_sndwnd.get() as u32;
        ti.tcpi_rcv_adv = tp.rcv_adv.get().wrapping_sub(tp.irs.get());
        ti.tcpi_rcv_up = tp.rcv_up.get().wrapping_sub(tp.irs.get());
        ti.tcpi_snd_una = tp.snd_una.get().wrapping_sub(tp.iss.get());
        ti.tcpi_snd_up = tp.snd_up.get().wrapping_sub(tp.iss.get());
        ti.tcpi_snd_wl1 = tp.snd_wl1.get().wrapping_sub(tp.iss.get());
        ti.tcpi_snd_wl2 = tp.snd_wl2.get().wrapping_sub(tp.iss.get());
        ti.tcpi_snd_max = tp.snd_max.get().wrapping_sub(tp.iss.get());

        ti.tcpi_ts_recent = tp.ts_recent.get(); // XXX value from the wire
        ti.tcpi_ts_recent_age = msec(tp.ts_recent_age.get());
        ti.tcpi_rfbuf_cnt = tp.rfbuf_cnt.get();
        ti.tcpi_rfbuf_ts = msec(tp.rfbuf_ts.get());

        mtx_enter(&so.so_rcv.sb_mtx);
        ti.tcpi_so_rcv_sb_cc = so.so_rcv.sb_cc.get() as u32;
        ti.tcpi_so_rcv_sb_hiwat = so.so_rcv.sb_hiwat.get() as u32;
        ti.tcpi_so_rcv_sb_lowat = so.so_rcv.sb_lowat.get() as u32;
        ti.tcpi_so_rcv_sb_wat = so.so_rcv.sb_wat.get() as u32;
        mtx_leave(&so.so_rcv.sb_mtx);
        mtx_enter(&so.so_snd.sb_mtx);
        ti.tcpi_so_snd_sb_cc = so.so_snd.sb_cc.get() as u32;
        ti.tcpi_so_snd_sb_hiwat = so.so_snd.sb_hiwat.get() as u32;
        ti.tcpi_so_snd_sb_lowat = so.so_snd.sb_lowat.get() as u32;
        ti.tcpi_so_snd_sb_wat = so.so_snd.sb_wat.get() as u32;
        mtx_leave(&so.so_snd.sb_mtx);
    }

    m.m_len().set(size_of::<TcpInfo>() as u32);
    // SAFETY: the mbuf holds `size_of::<TcpInfo>()` bytes (`MLEN`, or the cluster checked
    // above); `TcpInfo` is integers without padding, written unaligned.
    unsafe { ptr::write_unaligned(mtod::<TcpInfo>(m), ti) };
    Ok(())
}

/// The `int` an option mbuf carries (`*mtod(m, int *)`), `None` for a missing or short one.
fn opt_int(m: Option<&Mbuf>) -> Option<i32> {
    let m = m?;
    if (m.m_len().get() as usize) < size_of::<i32>() {
        return None;
    }
    // SAFETY: the mbuf holds at least an `int` (checked), read unaligned.
    Some(unsafe { ptr::read_unaligned(mtod::<i32>(m)) })
}

/// Stores an `int` option value in `m` (`m->m_len = sizeof(int); *mtod(m, int *) = v`).
fn opt_set_int(m: Option<&Mbuf>, v: i32) {
    let Some(m) = m else {
        panic(format_args!("tcp_ctloutput: no mbuf for PRCO_GETOPT"));
    };
    m.m_len().set(size_of::<i32>() as u32);
    // SAFETY: every mbuf holds at least `MLEN` bytes, more than an `int`.
    unsafe { ptr::write_unaligned(mtod::<i32>(m), v) };
}

/// `tcp_ctloutput`: the `IPPROTO_TCP` socket options; other levels go to IP.
pub fn tcp_ctloutput(
    op: i32,
    so: &'static Socket,
    level: i32,
    optname: i32,
    m: Option<&'static Mbuf>,
) -> Result<(), Errno> {
    let Some(inp) = sotoinpcb(so) else {
        return Err(Errno::ECONNRESET);
    };
    if level != IPPROTO_TCP {
        #[cfg(feature = "inet6")]
        if inp.has_flags(INP_IPV6) {
            return ip6_ctloutput(op, so, level, optname, m);
        }
        return ip_ctloutput(op, so, level, optname, m);
    }
    let Some(tp) = intotcpcb(inp) else {
        // The C dereferences the control block unchecked; a socket being closed has none.
        return Err(Errno::ECONNRESET);
    };

    match op {
        PRCO_SETOPT => match optname {
            TCP_NODELAY => match opt_int(m) {
                None => Err(Errno::EINVAL),
                Some(0) => {
                    tp.clear_flags(TF_NODELAY);
                    Ok(())
                }
                Some(_) => {
                    tp.set_flags(TF_NODELAY);
                    Ok(())
                }
            },

            TCP_NOPUSH => match opt_int(m) {
                None => Err(Errno::EINVAL),
                Some(0) => {
                    if tp.has_flags(TF_NOPUSH) {
                        tp.clear_flags(TF_NOPUSH);
                        if tcps_haveestablished(tp.t_state.get()) {
                            return tcp_output(tp);
                        }
                    }
                    Ok(())
                }
                Some(_) => {
                    tp.set_flags(TF_NOPUSH);
                    Ok(())
                }
            },

            TCP_MAXSEG => {
                let Some(i) = opt_int(m) else {
                    return Err(Errno::EINVAL);
                };
                if i > 0 && i <= i32::from(tp.t_maxseg.get()) {
                    tp.t_maxseg.set(i as u16);
                    Ok(())
                } else {
                    Err(Errno::EINVAL)
                }
            }

            TCP_SACK_ENABLE => {
                let Some(i) = opt_int(m) else {
                    return Err(Errno::EINVAL);
                };
                if tcps_haveestablished(tp.t_state.get()) {
                    return Err(Errno::EPERM);
                }
                if tp.has_flags(TF_SIGNATURE) {
                    return Err(Errno::EPERM);
                }
                tp.sack_enable.set(i32::from(i != 0));
                Ok(())
            }
            // TCP_SIGNATURE
            TCP_MD5SIG => {
                let Some(i) = opt_int(m) else {
                    return Err(Errno::EINVAL);
                };
                if tcps_haveestablished(tp.t_state.get()) {
                    return Err(Errno::EPERM);
                }
                if i != 0 {
                    tp.set_flags(TF_SIGNATURE);
                    tp.sack_enable.set(0);
                } else {
                    tp.clear_flags(TF_SIGNATURE);
                }
                Ok(())
            }
            _ => Err(Errno::ENOPROTOOPT),
        },

        PRCO_GETOPT => match optname {
            TCP_NODELAY => {
                opt_set_int(m, (tp.t_flags.get() & TF_NODELAY) as i32);
                Ok(())
            }
            TCP_NOPUSH => {
                opt_set_int(m, (tp.t_flags.get() & TF_NOPUSH) as i32);
                Ok(())
            }
            TCP_MAXSEG => {
                opt_set_int(m, i32::from(tp.t_maxseg.get()));
                Ok(())
            }
            TCP_SACK_ENABLE => {
                opt_set_int(m, tp.sack_enable.get());
                Ok(())
            }
            TCP_INFO => match m {
                Some(m) => tcp_fill_info(tp, so, m),
                None => panic(format_args!("tcp_ctloutput: no mbuf for TCP_INFO")),
            },
            // TCP_SIGNATURE
            TCP_MD5SIG => {
                opt_set_int(m, (tp.t_flags.get() & TF_SIGNATURE) as i32);
                Ok(())
            }
            _ => Err(Errno::ENOPROTOOPT),
        },

        _ => Ok(()),
    }
}

/// `tcp_attach`: attach TCP protocol to socket, allocating internet protocol control block,
/// tcp control block, buffer space, and entering LISTEN state to accept connections.
pub fn tcp_attach(so: &'static Socket, _proto: i32, wait: i32) -> Result<(), Errno> {
    if !so.so_pcb.get().is_null() {
        return Err(Errno::EISCONN);
    }
    if so.so_snd.sb_hiwat.get() == 0
        || so.so_rcv.sb_hiwat.get() == 0
        || sbcheckreserve(so.so_snd.sb_wat.get(), u64::from(tcp_sendspace)).is_err()
        || sbcheckreserve(so.so_rcv.sb_wat.get(), u64::from(tcp_recvspace)).is_err()
    {
        soreserve(so, u64::from(tcp_sendspace), u64::from(tcp_recvspace))?;
    }

    #[cfg(feature = "inet6")]
    let table = if so.dom_family() == i32::from(PF_INET6) {
        &TCB6TABLE
    } else {
        &TCBTABLE
    };
    #[cfg(not(feature = "inet6"))]
    let table = &TCBTABLE;
    in_pcballoc(so, table, wait)?;
    let Some(inp) = sotoinpcb(so) else {
        panic(format_args!("tcp_attach: no inpcb after in_pcballoc"));
    };
    let Some(tp) = tcp_newtcpcb(inp, wait) else {
        let nofd = so.so_state.get() & SS_NOFDREF; // XXX

        so.clear_state(SS_NOFDREF); // don't free the socket yet
        in_pcbdetach(inp);
        so.set_state(nofd);
        return Err(Errno::ENOBUFS);
    };
    tp.t_state.set(TCPS_CLOSED);
    #[cfg(feature = "inet6")]
    if inp.has_flags(INP_IPV6) {
        tp.pf.set(i32::from(PF_INET6));
    } else {
        tp.pf.set(i32::from(PF_INET));
    }
    #[cfg(not(feature = "inet6"))]
    tp.pf.set(i32::from(PF_INET));
    if so.has_options(SO_LINGER) && so.so_linger.get() == 0 {
        so.so_linger.set(TCP_LINGERTIME);
    }

    if so.has_options(SO_DEBUG) {
        tcp_trace(TA_USER, TCPS_CLOSED, Some(tp), tp, None, PRU_ATTACH, 0);
    }
    Ok(())
}

/// `tcp_detach`.
pub fn tcp_detach(so: &'static Socket) -> Result<(), Errno> {
    soassertlocked(so);

    let (_inp, tp) = tcp_sogetpcb(so)?;
    let start = trace_start(so, tp);

    // Detach the TCP protocol from the socket. If the protocol state is non-embryonic, then
    // can't do this directly: have to initiate a PRU_DISCONNECT, which may finish later;
    // embryonic TCB's can just be discarded here.
    let tp = tcp_dodisconnect(tp);

    trace_end(start, tp, PRU_DETACH);
    Ok(())
}

/// `tcp_bind`: give the socket an address.
pub fn tcp_bind(so: &'static Socket, nam: &'static Mbuf, p: &Proc) -> Result<(), Errno> {
    soassertlocked(so);

    let (inp, tp) = tcp_sogetpcb(so)?;
    let start = trace_start(so, tp);

    let error = in_pcbbind(inp, Some(nam), p);

    trace_end(start, Some(tp), PRU_BIND);
    error
}

/// `tcp_listen`: prepare to accept connections.
pub fn tcp_listen(so: &'static Socket) -> Result<(), Errno> {
    soassertlocked(so);

    let (inp, tp) = tcp_sogetpcb(so)?;
    let start = trace_start(so, tp);

    let error = (|| {
        if inp.inp_lport.get() == 0 {
            in_pcbbind(inp, None, curproc_or_panic("tcp_listen"))?;
        }

        // If the in_pcbbind() above is called, the tp->pf should still be whatever it was
        // before.
        tp.t_state.set(TCPS_LISTEN);
        Ok(())
    })();

    trace_end(start, Some(tp), PRU_LISTEN);
    error
}

/// `tcp_connect`: initiate connection to peer. Create a template for use in transmissions
/// on this connection. Enter SYN_SENT state, and mark socket as connecting. Start keep-alive
/// timer, and seed output sequence space. Send initial segment on connection.
pub fn tcp_connect(so: &'static Socket, nam: &'static Mbuf) -> Result<(), Errno> {
    soassertlocked(so);

    let (inp, tp) = tcp_sogetpcb(so)?;
    let start = trace_start(so, tp);

    let error = (|| {
        #[cfg(feature = "inet6")]
        let inet6 = inp.has_flags(INP_IPV6);
        #[cfg(not(feature = "inet6"))]
        let inet6 = false;
        if inet6 {
            #[cfg(feature = "inet6")]
            {
                let sin6p = in6_nam2sin6(nam)?;
                // SAFETY: `in6_nam2sin6` checked that the mbuf holds a whole `sockaddr_in6`.
                let sin6: SockaddrIn6 = unsafe { ptr::read_unaligned(sin6p) };
                if in6_is_addr_unspecified(&sin6.sin6_addr)
                    || in6_is_addr_multicast(&sin6.sin6_addr)
                {
                    return Err(Errno::EINVAL);
                }
            }
        } else {
            let sinp = in_nam2sin(nam)?;
            // SAFETY: `in_nam2sin` checked that the mbuf holds a whole `sockaddr_in`.
            let sin: SockaddrIn = unsafe { ptr::read_unaligned(sinp) };
            let a = sin.sin_addr.s_addr;
            if a == INADDR_ANY
                || a == INADDR_BROADCAST
                || in_multicast(a)
                || in_broadcast(sin.sin_addr, inp.inp_rtableid.get())
            {
                return Err(Errno::EINVAL);
            }
        }
        in_pcbconnect(inp, nam)?;

        tp.t_template.set(tcp_template(tp));
        if tp.t_template.get().is_none() {
            in_pcbunset_faddr(inp);
            in_pcbdisconnect(inp);
            return Err(Errno::ENOBUFS);
        }

        so.set_state(SS_CONNECTOUT);

        // Compute window scaling to request.
        tcp_rscale(tp, SB_MAX_VAR.load(Ordering::Relaxed));

        soisconnecting(so);
        tcpstat_inc(TcpstatCounters::TcpsConnattempt);
        tp.t_state.set(TCPS_SYN_SENT);
        tcp_timer_arm(tp, TCPT_KEEP, TCP_KEEPINIT.load(Ordering::Relaxed) as u64);
        tcp_set_iss_tsm(tp);
        tcp_sendseqinit(tp);
        tp.snd_last.set(tp.snd_una.get());
        tcp_output(tp)
    })();

    trace_end(start, Some(tp), PRU_CONNECT);
    error
}

/// `tcp_accept`: accept a connection. Essentially all the work is done at higher levels;
/// just return the address of the peer, storing through addr.
pub fn tcp_accept(so: &'static Socket, nam: &'static Mbuf) -> Result<(), Errno> {
    soassertlocked(so);

    let (inp, tp) = tcp_sogetpcb(so)?;

    in_setpeeraddr(inp, nam);

    if so.has_options(SO_DEBUG) {
        tcp_trace(TA_USER, tp.t_state.get(), Some(tp), tp, None, PRU_ACCEPT, 0);
    }
    Ok(())
}

/// `tcp_disconnect`: initiate disconnect from peer. If connection never passed embryonic
/// stage, just drop; else if don't need to let data drain, then can just drop anyways, else
/// have to begin TCP shutdown process: mark socket disconnecting, drain unread data, state
/// switch to reflect user close, and send segment (e.g. FIN) to peer. Socket will be really
/// disconnected when peer sends FIN and acks ours.
///
/// SHOULD IMPLEMENT LATER PRU_CONNECT VIA REALLOC TCPCB.
pub fn tcp_disconnect(so: &'static Socket) -> Result<(), Errno> {
    soassertlocked(so);

    let (_inp, tp) = tcp_sogetpcb(so)?;
    let start = trace_start(so, tp);

    let tp = tcp_dodisconnect(tp);

    trace_end(start, tp, PRU_DISCONNECT);
    Ok(())
}

/// `tcp_shutdown`: mark the connection as being incapable of further output.
pub fn tcp_shutdown(so: &'static Socket) -> Result<(), Errno> {
    soassertlocked(so);

    let (_inp, tp) = tcp_sogetpcb(so)?;
    let start = trace_start(so, tp);

    let mut tp = Some(tp);
    let mut error = Ok(());
    if !so.so_snd.has_state(SS_CANTSENDMORE) {
        socantsendmore(so);
        tp = tp.and_then(tcp_usrclosed);
        if let Some(tp) = tp {
            error = tcp_output(tp);
        }
    }

    trace_end(start, tp, PRU_SHUTDOWN);
    error
}

/// `tcp_rcvd`: after a receive, possibly send window update to peer.
pub fn tcp_rcvd(so: &'static Socket) {
    soassertlocked(so);

    let Ok((_inp, tp)) = tcp_sogetpcb(so) else {
        return;
    };
    let ostate = tp.t_state.get();

    // soreceive() calls this function when a user receives ancillary data on a listening
    // socket. We don't call tcp_output in such a case, since there is no header template
    // for a listening socket and hence the kernel will panic.
    if so.so_state.get() & (SS_ISCONNECTED | SS_ISCONNECTING) != 0 {
        let _ = tcp_output(tp);
    }

    if so.has_options(SO_DEBUG) {
        tcp_trace(TA_USER, ostate, Some(tp), tp, None, PRU_RCVD, 0);
    }
}

/// `tcp_send`: do a send by putting data in output queue and updating urgent marker if URG
/// set. Possibly send more data.
pub fn tcp_send(
    so: &'static Socket,
    m: Option<&'static Mbuf>,
    _nam: Option<&'static Mbuf>,
    control: Option<&'static Mbuf>,
) -> Result<(), Errno> {
    soassertlocked(so);

    let mut m = m;
    let error = (|| {
        if control.is_some_and(|c| c.m_len().get() != 0) {
            return Err(Errno::EINVAL);
        }

        let (_inp, tp) = tcp_sogetpcb(so)?;
        let ostate = tp.t_state.get();

        if let Some(data) = m.take() {
            mtx_enter(&so.so_snd.sb_mtx);
            sbappendstream(&so.so_snd, data);
            mtx_leave(&so.so_snd.sb_mtx);
        }

        let error = tcp_output(tp);

        if so.has_options(SO_DEBUG) {
            tcp_trace(TA_USER, ostate, Some(tp), tp, None, PRU_SEND, 0);
        }
        error
    })();

    m_freem(control);
    m_freem(m);

    error
}

/// `tcp_abort`: abort the TCP.
pub fn tcp_abort(so: &'static Socket) {
    soassertlocked(so);

    let Ok((_inp, tp)) = tcp_sogetpcb(so) else {
        return;
    };
    let start = trace_start(so, tp);

    let tp = tcp_drop(tp, Some(Errno::ECONNABORTED));

    trace_end(start, tp, PRU_ABORT);
}

/// `tcp_sense`.
pub fn tcp_sense(so: &'static Socket, ub: &mut Stat) -> Result<(), Errno> {
    soassertlocked(so);

    let (_inp, tp) = tcp_sogetpcb(so)?;

    mtx_enter(&so.so_snd.sb_mtx);
    ub.st_blksize = so.so_snd.sb_hiwat.get() as _;
    mtx_leave(&so.so_snd.sb_mtx);

    if so.has_options(SO_DEBUG) {
        tcp_trace(TA_USER, tp.t_state.get(), Some(tp), tp, None, PRU_SENSE, 0);
    }
    Ok(())
}

/// `tcp_rcvoob`.
pub fn tcp_rcvoob(so: &'static Socket, m: &'static Mbuf, flags: i32) -> Result<(), Errno> {
    soassertlocked(so);

    let (_inp, tp) = tcp_sogetpcb(so)?;

    let error = if (so.so_oobmark.get() == 0 && !so.so_rcv.has_state(SS_RCVATMARK))
        || so.has_options(SO_OOBINLINE)
        || tp.t_oobflags.get() & TCPOOB_HADDATA != 0
    {
        Err(Errno::EINVAL)
    } else if tp.t_oobflags.get() & TCPOOB_HAVEDATA == 0 {
        Err(Errno::EWOULDBLOCK)
    } else {
        m.m_len().set(1);
        // SAFETY: every mbuf holds at least `MLEN` bytes.
        unsafe { mtod::<u8>(m).write(tp.t_iobc.get()) };
        if flags & MSG_PEEK == 0 {
            tp.t_oobflags
                .set(tp.t_oobflags.get() ^ (TCPOOB_HAVEDATA | TCPOOB_HADDATA));
        }
        Ok(())
    };
    if so.has_options(SO_DEBUG) {
        tcp_trace(TA_USER, tp.t_state.get(), Some(tp), tp, None, PRU_RCVOOB, 0);
    }
    error
}

/// `tcp_sendoob`.
pub fn tcp_sendoob(
    so: &'static Socket,
    m: Option<&'static Mbuf>,
    _nam: Option<&'static Mbuf>,
    control: Option<&'static Mbuf>,
) -> Result<(), Errno> {
    soassertlocked(so);

    let mut m = m;
    let error = (|| {
        if control.is_some_and(|c| c.m_len().get() != 0) {
            return Err(Errno::EINVAL);
        }

        let (_inp, tp) = tcp_sogetpcb(so)?;
        let ostate = tp.t_state.get();

        let error = if sbspace(&so.so_snd) < -512 {
            Err(Errno::ENOBUFS)
        } else {
            // According to RFC961 (Assigned Protocols), the urgent pointer points to the last
            // octet of urgent data. We continue, however, to consider it to indicate the
            // first octet of data past the urgent section. Otherwise, snd_up should be one
            // lower.
            mtx_enter(&so.so_snd.sb_mtx);
            if let Some(data) = m.take() {
                sbappendstream(&so.so_snd, data);
            }
            mtx_leave(&so.so_snd.sb_mtx);
            tp.snd_up
                .set(tp.snd_una.get().wrapping_add(so.so_snd.sb_cc.get() as u32));
            tp.t_force.set(true);
            let error = tcp_output(tp);
            tp.t_force.set(false);
            error
        };

        if so.has_options(SO_DEBUG) {
            tcp_trace(TA_USER, ostate, Some(tp), tp, None, PRU_SENDOOB, 0);
        }
        error
    })();

    m_freem(control);
    m_freem(m);

    error
}

/// `tcp_sockaddr`.
pub fn tcp_sockaddr(so: &'static Socket, nam: &'static Mbuf) -> Result<(), Errno> {
    soassertlocked(so);

    let (inp, tp) = tcp_sogetpcb(so)?;

    in_setsockaddr(inp, nam);

    if so.has_options(SO_DEBUG) {
        tcp_trace(
            TA_USER,
            tp.t_state.get(),
            Some(tp),
            tp,
            None,
            PRU_SOCKADDR,
            0,
        );
    }
    Ok(())
}

/// `tcp_peeraddr`.
pub fn tcp_peeraddr(so: &'static Socket, nam: &'static Mbuf) -> Result<(), Errno> {
    soassertlocked(so);

    let (inp, tp) = tcp_sogetpcb(so)?;

    in_setpeeraddr(inp, nam);

    if so.has_options(SO_DEBUG) {
        tcp_trace(
            TA_USER,
            tp.t_state.get(),
            Some(tp),
            tp,
            None,
            PRU_PEERADDR,
            0,
        );
    }
    Ok(())
}

/// `tcp_dodisconnect`: initiate (or continue) disconnect. If embryonic state, just send
/// reset (once). If in ``let data drain'' option and linger null, just drop. Otherwise
/// (hard), mark socket disconnecting and drop current input data; switch states based on
/// user close, and send segment to peer (with FIN).
pub fn tcp_dodisconnect(tp: &'static Tcpcb) -> Option<&'static Tcpcb> {
    let so = tp.socket();

    if !tcps_haveestablished(tp.t_state.get()) {
        tcp_close(tp)
    } else if so.has_options(SO_LINGER) && so.so_linger.get() == 0 {
        tcp_drop(tp, None)
    } else {
        soisdisconnecting(so);
        mtx_enter(&so.so_rcv.sb_mtx);
        sbflush(&so.so_rcv);
        mtx_leave(&so.so_rcv.sb_mtx);
        let tp = tcp_usrclosed(tp);
        if let Some(tp) = tp {
            let _ = tcp_output(tp);
        }
        tp
    }
}

/// `tcp_usrclosed`: user issued close, and wish to trail through shutdown states: if never
/// received SYN, just forget it. If got a SYN from peer, but haven't sent FIN, then go to
/// FIN_WAIT_1 state to send peer a FIN. If already got a FIN from peer, then almost done; go
/// to LAST_ACK state. In all other cases, have already sent FIN to peer (e.g. after
/// PRU_SHUTDOWN), and just have to play tedious game waiting for peer to send FIN or not
/// respond to keep-alives, etc. We can let the user exit from the close as soon as the FIN
/// is acked.
pub fn tcp_usrclosed(tp: &'static Tcpcb) -> Option<&'static Tcpcb> {
    let tp = match tp.t_state.get() {
        TCPS_CLOSED | TCPS_LISTEN | TCPS_SYN_SENT => {
            tp.t_state.set(TCPS_CLOSED);
            tcp_close(tp)
        }

        TCPS_SYN_RECEIVED | TCPS_ESTABLISHED => {
            tp.t_state.set(TCPS_FIN_WAIT_1);
            Some(tp)
        }

        TCPS_CLOSE_WAIT => {
            tp.t_state.set(TCPS_LAST_ACK);
            Some(tp)
        }
        _ => Some(tp),
    };
    if let Some(tp) = tp
        && tp.t_state.get() >= TCPS_FIN_WAIT_2
    {
        soisdisconnected(tp.socket());
        // If we are in FIN_WAIT_2, we arrived here because the application did a shutdown
        // of the send side. Like the case of a transition from FIN_WAIT_1 to FIN_WAIT_2
        // after a full close, we start a timer to make sure sockets are not left in
        // FIN_WAIT_2 forever.
        if tp.t_state.get() == TCPS_FIN_WAIT_2 {
            let maxidle = TCPTV_KEEPCNT * TCP_KEEPIDLE.load(Ordering::Relaxed);
            tcp_timer_arm(tp, TCPT_2MSL, maxidle as u64);
        }
    }
    tp
}

/// `tcp_ident`: look up a socket for ident or tcpdrop, ...
fn tcp_ident(
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    newlen: usize,
    dodrop: bool,
) -> Result<(), Errno> {
    let mut tir: TcpIdentMapping = if dodrop {
        if oldp != 0 || *oldlenp != 0 {
            return Err(Errno::EINVAL);
        }
        if newp == 0 {
            return Err(Errno::EPERM);
        }
        if newlen < size_of::<TcpIdentMapping>() {
            return Err(Errno::ENOMEM);
        }
        copyin_obj(newp)?
    } else {
        if oldp == 0 {
            return Err(Errno::EINVAL);
        }
        if *oldlenp < size_of::<TcpIdentMapping>() {
            return Err(Errno::ENOMEM);
        }
        if newp != 0 || newlen != 0 {
            return Err(Errno::EINVAL);
        }
        copyin_obj(oldp)?
    };

    net_lock_shared();

    /// The looked-up addresses: `fin`/`lin`, or `fin6`/`lin6` with their embedded scopes
    /// (`f6`, `l6`).
    enum IdentAddrs {
        /// `AF_INET`.
        V4(SockaddrIn, SockaddrIn),
        /// `AF_INET6`.
        #[cfg(feature = "inet6")]
        V6 {
            /// `fin6->sin6_port`.
            fport: u16,
            /// `f6`.
            f6: In6Addr,
            /// `lin6->sin6_port`.
            lport: u16,
            /// `l6`.
            l6: In6Addr,
        },
    }

    let addrs = match tir.faddr.ss_family {
        #[cfg(feature = "inet6")]
        AF_INET6 => {
            if tir.laddr.ss_family != AF_INET6 {
                net_unlock_shared();
                return Err(Errno::EAFNOSUPPORT);
            }
            // SAFETY: a `sockaddr_storage` is larger than a `sockaddr_in6`; both are
            // integers.
            let fin6 =
                unsafe { ptr::read_unaligned(ptr::from_ref(&tir.faddr).cast::<SockaddrIn6>()) };
            let mut f6 = IN6ADDR_ANY;
            if in6_embedscope(&mut f6, &fin6, None, None).is_err() {
                net_unlock_shared();
                return Err(Errno::EINVAL); // ?
            }
            // SAFETY: as above.
            let lin6 =
                unsafe { ptr::read_unaligned(ptr::from_ref(&tir.laddr).cast::<SockaddrIn6>()) };
            let mut l6 = IN6ADDR_ANY;
            if in6_embedscope(&mut l6, &lin6, None, None).is_err() {
                net_unlock_shared();
                return Err(Errno::EINVAL); // ?
            }
            IdentAddrs::V6 {
                fport: fin6.sin6_port,
                f6,
                lport: lin6.sin6_port,
                l6,
            }
        }
        AF_INET => {
            if tir.laddr.ss_family != AF_INET {
                net_unlock_shared();
                return Err(Errno::EAFNOSUPPORT);
            }
            // SAFETY: a `sockaddr_storage` is larger than a `sockaddr_in`; both are integers.
            let fin =
                unsafe { ptr::read_unaligned(ptr::from_ref(&tir.faddr).cast::<SockaddrIn>()) };
            // SAFETY: as above.
            let lin =
                unsafe { ptr::read_unaligned(ptr::from_ref(&tir.laddr).cast::<SockaddrIn>()) };
            IdentAddrs::V4(fin, lin)
        }
        _ => {
            net_unlock_shared();
            return Err(Errno::EAFNOSUPPORT);
        }
    };

    let mut inp = match addrs {
        #[cfg(feature = "inet6")]
        IdentAddrs::V6 {
            fport,
            f6,
            lport,
            l6,
        } => in6_pcblookup(&TCB6TABLE, &f6, fport, &l6, lport, tir.rdomain),
        IdentAddrs::V4(fin, lin) => in_pcblookup(
            &TCBTABLE,
            fin.sin_addr,
            fin.sin_port,
            lin.sin_addr,
            lin.sin_port,
            tir.rdomain,
        ),
    };

    if dodrop {
        let mut so = None;
        let mut tp = None;
        if let Some(i) = inp {
            so = in_pcbsolock(i);
            if so.is_some() {
                tp = intotcpcb(i);
            }
        }
        let error = match (tp, so) {
            (Some(tp), Some(s)) if !s.has_options(SO_ACCEPTCONN) => {
                let _ = tcp_drop(tp, Some(Errno::ECONNABORTED));
                Ok(())
            }
            _ => Err(Errno::ESRCH),
        };

        in_pcbsounlock(inp, so);
        net_unlock_shared();
        in_pcbunref(inp);
        return error;
    }

    if inp.is_none() {
        tcpstat_inc(TcpstatCounters::TcpsPcbhashmiss);
        inp = match addrs {
            #[cfg(feature = "inet6")]
            IdentAddrs::V6 { lport, l6, .. } => {
                in6_pcblookup_listen(&TCB6TABLE, &l6, lport, None, tir.rdomain)
            }
            IdentAddrs::V4(_, lin) => {
                in_pcblookup_listen(&TCBTABLE, lin.sin_addr, lin.sin_port, None, tir.rdomain)
            }
        };
    }

    let so = inp.and_then(in_pcbsolock);

    match so {
        Some(so) if so.has_state(SS_CONNECTOUT) => {
            tir.ruid = so.so_ruid.get() as i32;
            tir.euid = so.so_euid.get() as i32;
        }
        _ => {
            tir.ruid = -1;
            tir.euid = -1;
        }
    }

    in_pcbsounlock(inp, so);
    net_unlock_shared();
    in_pcbunref(inp);

    *oldlenp = size_of::<TcpIdentMapping>();
    copyout_obj(&tir, oldp)
}

/// `ASSIGN(field)` of `tcp_sysctl_tcpstat` for every member in counter order, and the byte
/// image `sysctl_rdstruct` copies out (each member at its offset, the holes zero).
macro_rules! tcpstat_fields {
    ($($f:ident),* $(,)?) => {
        /// The members of `struct tcpstat` from the counters, in `tcpstat_counters` order.
        fn tcpstat_from_counters(counters: &[u64; TCPS_NCOUNTERS]) -> Tcpstat {
            let mut tcpstat = Tcpstat::default();
            let mut i = 0;
            $(
                tcpstat.$f = counters[i] as _;
                i += 1;
            )*
            let _ = i;
            tcpstat
        }

        /// The bytes of `struct tcpstat` as the C structure holds them.
        fn tcpstat_bytes(tcpstat: &Tcpstat) -> [u8; size_of::<Tcpstat>()] {
            let mut b = [0u8; size_of::<Tcpstat>()];
            $({
                let v = tcpstat.$f.to_ne_bytes();
                let o = offset_of!(Tcpstat, $f);
                b[o..o + v.len()].copy_from_slice(&v);
            })*
            b
        }
    };
}

tcpstat_fields!(
    tcps_connattempt,
    tcps_accepts,
    tcps_connects,
    tcps_drops,
    tcps_conndrops,
    tcps_closed,
    tcps_segstimed,
    tcps_rttupdated,
    tcps_delack,
    tcps_timeoutdrop,
    tcps_rexmttimeo,
    tcps_persisttimeo,
    tcps_persistdrop,
    tcps_keeptimeo,
    tcps_keepprobe,
    tcps_keepdrops,
    tcps_sndtotal,
    tcps_sndpack,
    tcps_sndbyte,
    tcps_sndrexmitpack,
    tcps_sndrexmitbyte,
    tcps_sndrexmitfast,
    tcps_sndacks,
    tcps_sndprobe,
    tcps_sndurg,
    tcps_sndwinup,
    tcps_sndctrl,
    tcps_rcvtotal,
    tcps_rcvpack,
    tcps_rcvbyte,
    tcps_rcvbadsum,
    tcps_rcvbadoff,
    tcps_rcvmemdrop,
    tcps_rcvnosec,
    tcps_rcvshort,
    tcps_rcvduppack,
    tcps_rcvdupbyte,
    tcps_rcvpartduppack,
    tcps_rcvpartdupbyte,
    tcps_rcvoopack,
    tcps_rcvoobyte,
    tcps_rcvpackafterwin,
    tcps_rcvbyteafterwin,
    tcps_rcvafterclose,
    tcps_rcvwinprobe,
    tcps_rcvdupack,
    tcps_rcvacktoomuch,
    tcps_rcvacktooold,
    tcps_rcvackpack,
    tcps_rcvackbyte,
    tcps_rcvwinupd,
    tcps_pawsdrop,
    tcps_predack,
    tcps_preddat,
    tcps_pcbhashmiss,
    tcps_noport,
    tcps_closing,
    tcps_badsyn,
    tcps_dropsyn,
    tcps_rcvbadsig,
    tcps_rcvgoodsig,
    tcps_inswcsum,
    tcps_outswcsum,
    tcps_ecn_accepts,
    tcps_ecn_rcvece,
    tcps_ecn_rcvcwr,
    tcps_ecn_rcvce,
    tcps_ecn_sndect,
    tcps_ecn_sndece,
    tcps_ecn_sndcwr,
    tcps_cwr_ecn,
    tcps_cwr_frecovery,
    tcps_cwr_timeout,
    tcps_sc_added,
    tcps_sc_completed,
    tcps_sc_timed_out,
    tcps_sc_overflowed,
    tcps_sc_reset,
    tcps_sc_unreach,
    tcps_sc_bucketoverflow,
    tcps_sc_aborted,
    tcps_sc_dupesyn,
    tcps_sc_dropped,
    tcps_sc_collisions,
    tcps_sc_retransmitted,
    tcps_sc_seedrandom,
    tcps_sc_hash_size,
    tcps_sc_entry_count,
    tcps_sc_entry_limit,
    tcps_sc_bucket_maxlen,
    tcps_sc_bucket_limit,
    tcps_sc_uses_left,
    tcps_conndrained,
    tcps_sack_recovery_episode,
    tcps_sack_rexmits,
    tcps_sack_rexmit_bytes,
    tcps_sack_rcv_opts,
    tcps_sack_snd_opts,
    tcps_sack_drop_opts,
    tcps_outswtso,
    tcps_outhwtso,
    tcps_outpkttso,
    tcps_outbadtso,
    tcps_inswlro,
    tcps_inhwlro,
    tcps_inpktlro,
    tcps_inbadlro,
);

/// `tcp_sysctl_tcpstat`: `net.inet.tcp.stats`, the counters and the SYN cache's state.
fn tcp_sysctl_tcpstat(oldp: usize, oldlenp: &mut usize, newp: usize) -> Result<(), Errno> {
    let mut counters = [0u64; TCPS_NCOUNTERS];
    for (c, v) in counters.iter_mut().zip(TCPCOUNTERS.iter()) {
        *c = v.load(Ordering::Relaxed);
    }
    let mut tcpstat = tcpstat_from_counters(&counters);

    mtx_enter(&SYN_CACHE_MTX);
    let set = &TCP_SYN_CACHE[TCP_SYN_CACHE_ACTIVE.load(Ordering::Relaxed)];
    tcpstat.tcps_sc_hash_size = set.scs_size.get() as u64;
    tcpstat.tcps_sc_entry_count = set.scs_count.get() as u64;
    tcpstat.tcps_sc_entry_limit = TCP_SYN_CACHE_LIMIT.load(Ordering::Relaxed) as u64;
    tcpstat.tcps_sc_bucket_maxlen = 0;
    for head in set.scs_buckethead.get().iter() {
        tcpstat.tcps_sc_bucket_maxlen = tcpstat
            .tcps_sc_bucket_maxlen
            .max(u64::from(head.sch_length.get()));
    }
    tcpstat.tcps_sc_bucket_limit = TCP_SYN_BUCKET_LIMIT.load(Ordering::Relaxed) as u64;
    tcpstat.tcps_sc_uses_left = set.scs_use.get();
    mtx_leave(&SYN_CACHE_MTX);

    sysctl_rdstruct(oldp, oldlenp, newp, &tcpstat_bytes(&tcpstat))
}

/// `tcp_sysctl`: sysctl for tcp variables.
pub fn tcp_sysctl(
    name: &[i32],
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    newlen: usize,
) -> Result<(), Errno> {
    // All sysctl names at this level are terminal.
    let [n] = name else {
        return Err(Errno::ENOTDIR);
    };

    match *n {
        TCPCTL_ROOTONLY | TCPCTL_BADDYNAMIC => {
            if *n == TCPCTL_ROOTONLY && newp != 0 && SECURELEVEL.load(Ordering::Relaxed) > 0 {
                return Err(Errno::EPERM);
            }
            let ports = if *n == TCPCTL_ROOTONLY {
                &ROOTONLYPORTS
            } else {
                &BADDYNAMICPORTS
            };
            let mut buf = [0u8; DP_MAPSIZE * size_of::<u32>()];

            net_lock_shared();
            for (i, w) in ports.tcp.iter().enumerate() {
                buf[i * 4..i * 4 + 4].copy_from_slice(&w.load(Ordering::Relaxed).to_ne_bytes());
            }
            net_unlock_shared();

            let error = sysctl_struct(oldp, oldlenp, newp, newlen, &mut buf);

            if error.is_ok() && newp != 0 {
                net_lock();
                for (i, w) in ports.tcp.iter().enumerate() {
                    let mut b = [0u8; 4];
                    b.copy_from_slice(&buf[i * 4..i * 4 + 4]);
                    w.store(u32::from_ne_bytes(b), Ordering::Relaxed);
                }
                net_unlock();
            }

            error
        }
        TCPCTL_IDENT => tcp_ident(oldp, oldlenp, newp, newlen, false),

        TCPCTL_DROP => tcp_ident(oldp, oldlenp, newp, newlen, true),

        TCPCTL_REASS_LIMIT | TCPCTL_SACKHOLE_LIMIT => {
            let (pool, var) = if *n == TCPCTL_REASS_LIMIT {
                (&TCPQE_POOL, &TCP_REASS_LIMIT)
            } else {
                (&SACKHL_POOL, &tcp_sackhole_limit)
            };

            let oval = var.load(Ordering::Relaxed);
            let nval = AtomicI32::new(oval);
            sysctl_int(oldp, oldlenp, newp, newlen, &nval)?;
            let nval = nval.into_inner();

            if oval != nval {
                rw_enter(&SYSCTL_LOCK, RW_WRITE | RW_INTR)?;
                let mut error = Ok(());
                if nval != var.load(Ordering::Relaxed) {
                    error = pool_sethardlimit(pool, nval as u32);
                    if error.is_ok() {
                        var.store(nval, Ordering::Relaxed);
                    }
                }
                rw_exit(&SYSCTL_LOCK);
                return error;
            }

            Ok(())
        }
        TCPCTL_STATS => tcp_sysctl_tcpstat(oldp, oldlenp, newp),

        TCPCTL_SYN_USE_LIMIT => {
            let oval = TCP_SYN_USE_LIMIT.load(Ordering::Relaxed);
            let nval = AtomicI32::new(oval);
            sysctl_int_bounded(oldp, oldlenp, newp, newlen, &nval, 0, i32::MAX)?;
            let nval = nval.into_inner();
            if oval != nval {
                // Global tcp_syn_use_limit is used when reseeding a new cache. Also update
                // the value in active cache.
                mtx_enter(&SYN_CACHE_MTX);
                for set in &TCP_SYN_CACHE {
                    if set.scs_use.get() > i64::from(nval) {
                        set.scs_use.set(i64::from(nval));
                    }
                }
                TCP_SYN_USE_LIMIT.store(nval, Ordering::Relaxed);
                mtx_leave(&SYN_CACHE_MTX);
            }
            Ok(())
        }

        TCPCTL_SYN_HASH_SIZE => {
            let oval = tcp_syn_hash_size.load(Ordering::Relaxed);
            let nval = AtomicI32::new(oval);
            sysctl_int_bounded(oldp, oldlenp, newp, newlen, &nval, 1, 100000)?;
            let nval = nval.into_inner();
            if oval != nval {
                // If global hash size has been changed, switch sets as soon as possible.
                // Then the actual hash array will be reallocated.
                mtx_enter(&SYN_CACHE_MTX);
                for set in &TCP_SYN_CACHE {
                    if set.scs_size.get() != nval {
                        set.scs_use.set(0);
                    }
                }
                tcp_syn_hash_size.store(nval, Ordering::Relaxed);
                mtx_leave(&SYN_CACHE_MTX);
            }
            Ok(())
        }

        _ => {
            let error = sysctl_bounded_arr(&TCPCTL_VARS, name, oldp, oldlenp, newp, newlen);
            match *n {
                TCPCTL_KEEPINITTIME => TCP_KEEPINIT.store(
                    TCP_KEEPINIT_SEC.load(Ordering::Relaxed) * tcp_time(1),
                    Ordering::Relaxed,
                ),
                TCPCTL_KEEPIDLE => TCP_KEEPIDLE.store(
                    TCP_KEEPIDLE_SEC.load(Ordering::Relaxed) * tcp_time(1),
                    Ordering::Relaxed,
                ),
                TCPCTL_KEEPINTVL => TCP_KEEPINTVL.store(
                    TCP_KEEPINTVL_SEC.load(Ordering::Relaxed) * tcp_time(1),
                    Ordering::Relaxed,
                ),
                _ => {}
            }
            error
        }
    }
}

/// `tcp_update_sndspace`: scale the send buffer so that inflight data is not accounted
/// against the limit. The buffer will scale with the congestion window, if the the receiver
/// stops acking data the window will shrink and therefore the buffer size will shrink as
/// well. In low memory situation try to shrink the buffer to the initial size disabling the
/// send buffer scaling as long as the situation persists.
pub fn tcp_update_sndspace(tp: &Tcpcb) {
    let so = tp.socket();
    let sb = &so.so_snd;

    mtx_enter(&sb.sb_mtx);

    let mut nmax = sb.sb_hiwat.get();

    if sbchecklowmem() {
        // low on memory try to get rid of some
        if u64::from(tcp_sendspace) < nmax {
            nmax = u64::from(tcp_sendspace);
        }
    } else if sb.sb_wat.get() != u64::from(tcp_sendspace) {
        // user requested buffer size, auto-scaling disabled
        nmax = sb.sb_wat.get();
    } else {
        // automatic buffer scaling
        nmax = SB_MAX_VAR.load(Ordering::Relaxed).min(
            sb.sb_wat
                .get()
                .wrapping_add(u64::from(tp.snd_max.get().wrapping_sub(tp.snd_una.get()))),
        );
    }

    // a writable socket must be preserved because of poll(2) semantics
    if sbspace_locked(sb) >= sb.sb_lowat.get() {
        let lowat = sb.sb_lowat.get() as u64;
        if nmax < sb.sb_cc.get() + lowat {
            nmax = sb.sb_cc.get() + lowat;
        }
        // keep in sync with sbreserve() calculation
        if nmax * 8 < sb.sb_mbcnt.get() + lowat {
            nmax = (sb.sb_mbcnt.get() + lowat).div_ceil(8);
        }
    }

    // round to MSS boundary
    nmax = nmax.next_multiple_of(u64::from(tp.t_maxseg.get()));

    if nmax != sb.sb_hiwat.get() {
        // The C ignores the result: a refused size leaves the buffer as it was.
        let _ = sbreserve(sb, nmax);
    }

    mtx_leave(&sb.sb_mtx);
}

/// `tcp_update_rcvspace`: scale the recv buffer by looking at how much data was transferred
/// in one approximated RTT. If more than a big part of the recv buffer was transferred
/// during that time we increase the buffer by a constant. In low memory situation try to
/// shrink the buffer to the initial size.
pub fn tcp_update_rcvspace(tp: &Tcpcb) {
    let so = tp.socket();
    let sb = &so.so_rcv;

    mtx_enter(&sb.sb_mtx);

    let mut nmax = sb.sb_hiwat.get();

    if sbchecklowmem() {
        // low on memory try to get rid of some
        if u64::from(tcp_recvspace) < nmax {
            nmax = u64::from(tcp_recvspace);
        }
    } else if sb.sb_wat.get() != u64::from(tcp_recvspace) {
        // user requested buffer size, auto-scaling disabled
        nmax = sb.sb_wat.get();
    } else {
        // automatic buffer scaling
        if u64::from(tp.rfbuf_cnt.get()) > sb.sb_hiwat.get() / 8 * 7 {
            nmax = SB_MAX_VAR
                .load(Ordering::Relaxed)
                .min(sb.sb_hiwat.get() + u64::from(TCP_AUTORCVBUF_INC));
        }
    }

    // a readable socket must be preserved because of poll(2) semantics
    mtx_enter(&so.so_snd.sb_mtx);
    if sb.sb_cc.get() as i64 >= sb.sb_lowat.get() && (nmax as i64) < so.so_snd.sb_lowat.get() {
        nmax = so.so_snd.sb_lowat.get() as u64;
    }
    mtx_leave(&so.so_snd.sb_mtx);

    if nmax != sb.sb_hiwat.get() {
        // round to MSS boundary
        nmax = nmax.next_multiple_of(u64::from(tp.t_maxseg.get()));
        // The C ignores the result, as above.
        let _ = sbreserve(sb, nmax);
    }

    mtx_leave(&sb.sb_mtx);
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests of TCP through its user requests, over the test Ethernet interface: an active
    // open (SYN out, SYN-ACK in, ACK out: ESTABLISHED), data both ways, the FIN exchange of a
    // close; a passive open through the SYN cache (SYN in, SYN-ACK out, ACK in: a new socket on
    // the listener's queue); a RST for a segment to a closed port; and the sysctl counters.

    use std::{assert, assert_eq, boxed::Box, vec, vec::Vec};

    use super::*;
    use crate::kern::uipc_mbuf::{m_freem, m_gethdr};
    use crate::kern::uipc_socket::{soclose, socreate};
    use crate::kern::uipc_socket2::{solock, sounlock};
    use crate::net::ethertypes::{ETHERTYPE_ARP, ETHERTYPE_IP};
    use crate::net::if_::tests::test_packet;
    use crate::net::if_arp::{ARPHRD_ETHER, ARPOP_REPLY, Arphdr};
    use crate::net::if_ethersubr::ether_input;
    use crate::net::if_var::Ifnet;
    use crate::net::ifq::ifq_dequeue;
    use crate::netinet::if_ether::{EtherArp, EtherHeader, arpintr};
    use crate::netinet::in_cksum::in_cksum;
    use crate::netinet::in_pcb::tests::{nam, setup, teardown};
    use crate::netinet::in4_cksum::in4_cksum;
    use crate::netinet::ip_input::ipintr;
    use crate::netinet::ip_input::tests::{
        ADDR, GATEWAY, OURS, PEER, bytes, configure, frame, sin, test_ether,
    };
    use crate::netinet::ip_var::{mtod_ip, mtod_ip_store};
    use crate::netinet::tcp::{TH_ACK, TH_FIN, TH_RST, TH_SYN};
    use crate::netinet::tcp_fsm::{TCPS_FIN_WAIT_1, TCPS_FIN_WAIT_2, TCPS_TIME_WAIT};
    use crate::netinet::tcp_subr::tcp_init;
    use crate::netinet::tcp_var::sototcpcb;
    use crate::sys::mbuf::M_DONTWAIT;
    use crate::sys::mbuf::MT_DATA;
    use crate::sys::socket::SOCK_STREAM;
    use crate::sys::socketvar::SS_CANTRCVMORE;

    /// The interface with our address, its queue drained; TCP set up.
    fn net() -> &'static Ifnet {
        tcp_init();
        let ifp = test_ether();
        configure(ifp, ADDR, [255, 255, 255, 0]);
        drain(ifp);
        ifp
    }

    /// Drops whatever the interface was asked to send.
    fn drain(ifp: &Ifnet) {
        while let Some(m) = ifq_dequeue(&ifp.if_snd) {
            m_freem(m);
        }
    }

    /// The gateway answers our ARP request.
    fn answer_arp(ifp: &'static Ifnet) {
        let reply = EtherArp {
            ea_hdr: Arphdr {
                ar_hrd: ARPHRD_ETHER.to_be(),
                ar_pro: ETHERTYPE_IP.to_be(),
                ar_hln: 6,
                ar_pln: 4,
                ar_op: ARPOP_REPLY.to_be(),
            },
            arp_sha: PEER,
            arp_spa: GATEWAY,
            arp_tha: OURS,
            arp_tpa: ADDR,
        };
        // SAFETY: an `ether_arp` is plain bytes.
        let arp = unsafe {
            core::slice::from_raw_parts(ptr::from_ref(&reply).cast::<u8>(), size_of::<EtherArp>())
        };
        ether_input(ifp, frame(ifp, OURS, ETHERTYPE_ARP, arp), None);
        arpintr();
    }

    /// A segment the interface sent: its IP and TCP bytes.
    struct Seg(Vec<u8>);

    impl Seg {
        fn flags(&self) -> u8 {
            self.0[33]
        }
        fn seq(&self) -> u32 {
            u32::from_be_bytes([self.0[24], self.0[25], self.0[26], self.0[27]])
        }
        fn ack(&self) -> u32 {
            u32::from_be_bytes([self.0[28], self.0[29], self.0[30], self.0[31]])
        }
        fn sport(&self) -> u16 {
            u16::from_be_bytes([self.0[20], self.0[21]])
        }
        fn dport(&self) -> u16 {
            u16::from_be_bytes([self.0[22], self.0[23]])
        }
        fn payload(&self) -> &[u8] {
            let off = 20 + usize::from(self.0[32] >> 4) * 4;
            &self.0[off..]
        }
    }

    /// The next segment on the interface's send queue, its checksums verified.
    fn next_seg(ifp: &Ifnet) -> Option<Seg> {
        let out = ifq_dequeue(&ifp.if_snd)?;
        let b = bytes(out);
        m_freem(out);
        let ipb = b[size_of::<EtherHeader>()..].to_vec();
        assert_eq!(ipb[9], IPPROTO_TCP as u8);
        let m = test_packet(&ipb);
        assert_eq!(in_cksum(m, 20), 0, "ip header checksum");
        // The checksum may be left to the interface (M_TCP_CSUM_OUT): in_proto_cksum_out filled
        // it when the test interface has no offload.
        assert_eq!(
            in4_cksum(m, IPPROTO_TCP as u8, 20, (ipb.len() - 20) as i32),
            0,
            "tcp checksum"
        );
        m_freem(m);
        Some(Seg(ipb))
    }

    /// The segment `GATEWAY:sport -> ADDR:dport` with `flags`, `seq`, `ack`, options and data.
    fn segment(
        sport: u16,
        dport: u16,
        flags: u8,
        seq: u32,
        ack: u32,
        opts: &[u8],
        data: &[u8],
    ) -> Vec<u8> {
        let thlen = 20 + opts.len();
        let len = 20 + thlen + data.len();
        let mut p = vec![0x45, 0];
        p.extend_from_slice(&(len as u16).to_be_bytes());
        p.extend_from_slice(&[0, 0, 0, 0, 64, IPPROTO_TCP as u8, 0, 0]);
        p.extend_from_slice(&GATEWAY);
        p.extend_from_slice(&ADDR);
        p.extend_from_slice(&sport.to_be_bytes());
        p.extend_from_slice(&dport.to_be_bytes());
        p.extend_from_slice(&seq.to_be_bytes());
        p.extend_from_slice(&ack.to_be_bytes());
        p.push(((thlen / 4) as u8) << 4);
        p.push(flags);
        p.extend_from_slice(&16384u16.to_be_bytes());
        p.extend_from_slice(&[0, 0, 0, 0]);
        p.extend_from_slice(opts);
        p.extend_from_slice(data);
        let m = test_packet(&p);
        let mut ip = mtod_ip(m);
        ip.ip_sum = in_cksum(m, 20);
        mtod_ip_store(m, &ip);
        let sum = in4_cksum(m, IPPROTO_TCP as u8, 20, (len - 20) as i32);
        let mut b = bytes(m);
        m_freem(m);
        b[36..38].copy_from_slice(&sum.to_ne_bytes());
        b
    }

    /// Hands `datagram` to the interface as the gateway's and runs the IP input queue.
    fn receive(ifp: &'static Ifnet, datagram: &[u8]) {
        ether_input(ifp, frame(ifp, OURS, ETHERTYPE_IP, datagram), None);
        ipintr();
    }

    /// `addr:port`.
    fn sinp(addr: [u8; 4], port: u16) -> SockaddrIn {
        let mut s = sin(addr);
        s.sin_port = port.to_be();
        s
    }

    /// A data mbuf holding `data`.
    fn data_mbuf(data: &[u8]) -> &'static Mbuf {
        let m = m_gethdr(M_DONTWAIT, MT_DATA).expect("mbuf");
        m.m_len().set(data.len() as u32);
        m.m_pkthdr().len.set(data.len() as i32);
        // SAFETY: a fresh packet header mbuf holds `MHLEN` bytes.
        unsafe { ptr::copy_nonoverlapping(data.as_ptr(), mtod::<u8>(m), data.len()) };
        m
    }

    /// The bytes queued in `so`'s receive buffer.
    fn received(so: &Socket) -> Vec<u8> {
        let mut v = Vec::new();
        let mut m = so.so_rcv.sb_mb.get();
        while let Some(mm) = m {
            let mut d = Some(mm);
            while let Some(x) = d {
                // SAFETY: an mbuf holds `m_len` bytes at its data pointer.
                v.extend_from_slice(unsafe {
                    core::slice::from_raw_parts(mtod::<u8>(x), x.m_len().get() as usize)
                });
                d = x.m_next().get();
            }
            m = mm.m_nextpkt().get();
        }
        v
    }

    /// The counter of `c`.
    fn tcpstat(c: TcpstatCounters) -> u64 {
        TCPCOUNTERS[c as usize].load(Ordering::Relaxed)
    }

    #[test]
    fn active_open_data_and_close() {
        let (_g, _t, _p) = setup();
        let ifp = net();

        let so = socreate(i32::from(AF_INET), SOCK_STREAM, 0).expect("socket");
        let attempts = tcpstat(TcpstatCounters::TcpsConnattempt);
        solock(so);
        tcp_connect(so, nam(sinp(GATEWAY, 80))).expect("connect");
        sounlock(so);
        assert_eq!(tcpstat(TcpstatCounters::TcpsConnattempt), attempts + 1);
        let tp = sototcpcb(so).expect("tcpcb");
        assert_eq!(tp.t_state.get(), TCPS_SYN_SENT);
        assert!(so.has_state(SS_ISCONNECTING));

        // The SYN waits for ARP.
        drain(ifp);
        answer_arp(ifp);
        let syn = next_seg(ifp).expect("the SYN");
        assert_eq!(syn.flags(), TH_SYN);
        assert_eq!(syn.dport(), 80);
        let lport = syn.sport();
        let iss = syn.seq();
        // MSS option first.
        assert_eq!(&syn.0[40..42], &[2, 4]);

        // SYN-ACK with an MSS of 1000.
        let peer_iss = 0x1000_0000u32;
        receive(
            ifp,
            &segment(
                80,
                lport,
                TH_SYN | TH_ACK,
                peer_iss,
                iss.wrapping_add(1),
                &[2, 4, 0x03, 0xe8],
                &[],
            ),
        );
        assert_eq!(tp.t_state.get(), TCPS_ESTABLISHED);
        assert!(so.has_state(SS_ISCONNECTED));
        assert_eq!(tp.t_maxseg.get(), 1000);
        let ack = next_seg(ifp).expect("the ACK");
        assert_eq!(ack.flags(), TH_ACK);
        assert_eq!(ack.ack(), peer_iss.wrapping_add(1));

        // Data out.
        solock(so);
        tcp_send(so, Some(data_mbuf(b"hello")), None, None).expect("send");
        sounlock(so);
        let d = next_seg(ifp).expect("data");
        assert_eq!(d.flags() & TH_ACK, TH_ACK);
        assert_eq!(d.seq(), iss.wrapping_add(1));
        assert_eq!(d.payload(), b"hello");

        // Data in, acknowledging ours.
        receive(
            ifp,
            &segment(
                80,
                lport,
                TH_ACK,
                peer_iss.wrapping_add(1),
                iss.wrapping_add(6),
                &[],
                b"world",
            ),
        );
        assert_eq!(received(so), b"world");
        assert_eq!(tp.snd_una.get(), iss.wrapping_add(6));
        assert_eq!(tp.rcv_nxt.get(), peer_iss.wrapping_add(6));

        // shutdown(2): our FIN; the peer acknowledges it and sends its own.
        solock(so);
        tcp_shutdown(so).expect("shutdown");
        sounlock(so);
        assert_eq!(tp.t_state.get(), TCPS_FIN_WAIT_1);
        let mut fin = next_seg(ifp).expect("our FIN");
        // A delayed ACK of the data may come first.
        if fin.flags() & TH_FIN == 0 {
            fin = next_seg(ifp).expect("our FIN");
        }
        assert_eq!(fin.flags() & TH_FIN, TH_FIN);
        assert_eq!(fin.seq(), iss.wrapping_add(6));
        receive(
            ifp,
            &segment(
                80,
                lport,
                TH_ACK,
                peer_iss.wrapping_add(6),
                iss.wrapping_add(7),
                &[],
                &[],
            ),
        );
        assert_eq!(tp.t_state.get(), TCPS_FIN_WAIT_2);
        receive(
            ifp,
            &segment(
                80,
                lport,
                TH_FIN | TH_ACK,
                peer_iss.wrapping_add(6),
                iss.wrapping_add(7),
                &[],
                &[],
            ),
        );
        assert_eq!(tp.t_state.get(), TCPS_TIME_WAIT);
        assert!(so.so_rcv.has_state(SS_CANTRCVMORE));
        let last = next_seg(ifp).expect("the ACK of the FIN");
        assert_eq!(last.ack(), peer_iss.wrapping_add(7));

        let _ = soclose(so, 0);
        drain(ifp);
        teardown();
    }

    #[test]
    fn passive_open_through_the_syn_cache() {
        let (_g, _t, p) = setup();
        let ifp = net();
        // The peer's MAC is known (no ARP wait for the SYN-ACK).
        answer_arp(ifp);
        drain(ifp);

        let l = socreate(i32::from(AF_INET), SOCK_STREAM, 0).expect("socket");
        solock(l);
        tcp_bind(l, nam(sinp(ADDR, 8080)), p).expect("bind");
        crate::kern::uipc_socket::solisten(l, 5).expect("listen");
        sounlock(l);
        assert_eq!(sototcpcb(l).expect("tcpcb").t_state.get(), TCPS_LISTEN);

        let peer_iss = 0x2000_0000u32;
        receive(
            ifp,
            &segment(40000, 8080, TH_SYN, peer_iss, 0, &[2, 4, 0x05, 0xb4], &[]),
        );
        let synack = next_seg(ifp).expect("the SYN-ACK");
        assert_eq!(synack.flags(), TH_SYN | TH_ACK);
        assert_eq!(synack.ack(), peer_iss.wrapping_add(1));
        assert_eq!(synack.sport(), 8080);
        assert_eq!(l.so_qlen.get(), 0, "embryonic: in the syn cache only");

        receive(
            ifp,
            &segment(
                40000,
                8080,
                TH_ACK,
                peer_iss.wrapping_add(1),
                synack.seq().wrapping_add(1),
                &[],
                b"hi",
            ),
        );
        assert_eq!(l.so_qlen.get(), 1, "a connection to accept");
        let so = l.so_q.first().expect("the new socket");
        let tp = sototcpcb(so).expect("its tcpcb");
        assert_eq!(tp.t_state.get(), TCPS_ESTABLISHED);
        assert_eq!(received(so), b"hi");

        let _ = soclose(l, 0);
        drain(ifp);
        teardown();
    }

    #[test]
    fn a_segment_to_a_closed_port_is_reset() {
        let (_g, _t, _p) = setup();
        let ifp = net();
        answer_arp(ifp);
        drain(ifp);

        let noport = tcpstat(TcpstatCounters::TcpsNoport);
        receive(ifp, &segment(40001, 9, TH_SYN, 77, 0, &[], &[]));
        assert_eq!(tcpstat(TcpstatCounters::TcpsNoport), noport + 1);
        let rst = next_seg(ifp).expect("a RST");
        assert_eq!(rst.flags(), TH_RST | TH_ACK);
        assert_eq!(rst.ack(), 78);
        assert_eq!(rst.dport(), 40001);
        teardown();
    }

    #[test]
    fn sysctl_reads_the_statistics_and_the_variables() {
        let (_g, _t, _p) = setup();
        let _ifp = net();

        let mut buf = Box::new([0u8; size_of::<Tcpstat>()]);
        let mut len = buf.len();
        // A kernel buffer stands for the user one on the host (copyout is a memcpy there).
        tcp_sysctl(&[TCPCTL_STATS], buf.as_mut_ptr() as usize, &mut len, 0, 0).expect("stats");
        assert_eq!(len, size_of::<Tcpstat>());
        let mut v = 0i32;
        let mut len = size_of::<i32>();
        tcp_sysctl(
            &[TCPCTL_MSSDFLT],
            ptr::from_mut(&mut v) as usize,
            &mut len,
            0,
            0,
        )
        .expect("mssdflt");
        assert_eq!(v, TCP_MSS);
        assert_eq!(
            tcp_sysctl(&[TCPCTL_STATS, 1], 0, &mut len, 0, 0),
            Err(Errno::ENOTDIR)
        );
        teardown();
    }

    /// The TCP header `sport -> dport` with `flags`, `seq`, `ack`, options and data, checksum
    /// zero.
    #[cfg(feature = "inet6")]
    fn tcp6_payload(
        sport: u16,
        dport: u16,
        flags: u8,
        seq: u32,
        ack: u32,
        opts: &[u8],
        data: &[u8],
    ) -> Vec<u8> {
        let thlen = 20 + opts.len();
        let mut p = Vec::new();
        p.extend_from_slice(&sport.to_be_bytes());
        p.extend_from_slice(&dport.to_be_bytes());
        p.extend_from_slice(&seq.to_be_bytes());
        p.extend_from_slice(&ack.to_be_bytes());
        p.push(((thlen / 4) as u8) << 4);
        p.push(flags);
        p.extend_from_slice(&16384u16.to_be_bytes());
        p.extend_from_slice(&[0, 0, 0, 0]);
        p.extend_from_slice(opts);
        p.extend_from_slice(data);
        p
    }

    /// `tcp_input` of the IPv6 packet `m`, the TCP header after the IPv6 header.
    #[cfg(feature = "inet6")]
    fn input6(m: &'static Mbuf) {
        let mut mp = Some(m);
        let mut off = 40;
        assert_eq!(
            crate::netinet::tcp_input::tcp_input(
                &mut mp,
                &mut off,
                IPPROTO_TCP,
                i32::from(crate::sys::socket::AF_INET6),
                None
            ),
            crate::netinet::in_::IPPROTO_DONE
        );
    }

    /// The one segment the IPv6 test interface sent: its IPv6 and TCP bytes, the TCP checksum
    /// verified, to `fd00:77::2` from `fd00:77::1`.
    #[cfg(feature = "inet6")]
    fn next_seg6() -> Vec<u8> {
        use crate::netinet::in_pcb::tests::inet6::take_sent6;
        use crate::netinet6::in6::tests::a6;
        use crate::netinet6::in6_cksum::in6_cksum;

        let mut sent = take_sent6();
        assert_eq!(sent.len(), 1, "one segment");
        let b = sent.remove(0);
        assert_eq!(b[0] >> 4, 6);
        assert_eq!(b[6], IPPROTO_TCP as u8);
        assert_eq!(&b[8..24], &a6("fd00:77::1").s6_addr);
        assert_eq!(&b[24..40], &a6("fd00:77::2").s6_addr);
        assert_eq!(usize::from(u16::from_be_bytes([b[4], b[5]])), b.len() - 40);
        let m = test_packet(&b);
        assert_eq!(
            in6_cksum(m, IPPROTO_TCP as u8, 40, (b.len() - 40) as u32),
            0,
            "tcp checksum"
        );
        m_freem(m);
        b
    }

    #[cfg(feature = "inet6")]
    #[test]
    fn inet6_passive_open_through_the_syn_cache_and_a_reset() {
        use crate::netinet::in_pcb::INP_IPV6;
        use crate::netinet::in_pcb::tests::inet6::{nam6, packet6, sin6, take_sent6, test_if6};
        use crate::netinet6::in6::IN6ADDR_ANY;
        use crate::netinet6::in6::tests::a6;
        use crate::sys::socket::AF_INET6;

        let (_g, _t, p) = setup();
        tcp_init();
        let ifp = test_if6(b"ttcp6");
        // The seg helper reads the TCP header after a 20-byte IP header; shift by 20 for IPv6.
        let seg = |b: Vec<u8>| Seg(b[20..].to_vec());

        let l = socreate(i32::from(AF_INET6), SOCK_STREAM, 0).expect("socket");
        let linp = crate::netinet::in_pcb::sotoinpcb(l).expect("inpcb");
        assert!(linp.has_flags(INP_IPV6));
        assert_eq!(
            sototcpcb(l).expect("tcpcb").pf.get(),
            i32::from(AF_INET6),
            "tcp_attach: PF_INET6"
        );
        solock(l);
        tcp_bind(l, nam6(sin6(IN6ADDR_ANY, 8080)), p).expect("bind");
        crate::kern::uipc_socket::solisten(l, 5).expect("listen");
        sounlock(l);

        // SYN in: the SYN cache answers with a SYN-ACK from our address.
        let peer_iss = 0x3000_0000u32;
        let syn = tcp6_payload(40000, 8080, TH_SYN, peer_iss, 0, &[2, 4, 0x05, 0xa0], &[]);
        input6(packet6(ifp, IPPROTO_TCP as u8, &syn, Some(16)));
        let synack = seg(next_seg6());
        assert_eq!(synack.flags(), TH_SYN | TH_ACK);
        assert_eq!(synack.ack(), peer_iss.wrapping_add(1));
        assert_eq!((synack.sport(), synack.dport()), (8080, 40000));
        assert_eq!(l.so_qlen.get(), 0, "embryonic: in the syn cache only");

        // ACK with data: syn_cache_get makes the connection (in6_pcbset_addr).
        let ack = tcp6_payload(
            40000,
            8080,
            TH_ACK,
            peer_iss.wrapping_add(1),
            synack.seq().wrapping_add(1),
            &[],
            b"hi",
        );
        input6(packet6(ifp, IPPROTO_TCP as u8, &ack, Some(16)));
        assert_eq!(l.so_qlen.get(), 1, "a connection to accept");
        let so = l.so_q.first().expect("the new socket");
        let tp = sototcpcb(so).expect("its tcpcb");
        assert_eq!(tp.t_state.get(), TCPS_ESTABLISHED);
        assert_eq!(tp.pf.get(), i32::from(AF_INET6));
        let inp = tp.t_inpcb;
        assert!(inp.has_flags(INP_IPV6));
        assert_eq!(inp.inp_laddr6.get(), a6("fd00:77::1"));
        assert_eq!(inp.inp_faddr6.get(), a6("fd00:77::2"));
        assert_eq!(inp.inp_fport.get(), 40000u16.to_be());
        assert_eq!(received(so), b"hi");
        let _ = take_sent6();

        // A segment to a closed port: tcp_respond sends a RST over IPv6.
        let noport = tcpstat(TcpstatCounters::TcpsNoport);
        let stray = tcp6_payload(40001, 9, TH_SYN, 77, 0, &[], &[]);
        input6(packet6(ifp, IPPROTO_TCP as u8, &stray, Some(16)));
        assert_eq!(tcpstat(TcpstatCounters::TcpsNoport), noport + 1);
        let rst = seg(next_seg6());
        assert_eq!(rst.flags(), TH_RST | TH_ACK);
        assert_eq!(rst.ack(), 78);
        assert_eq!((rst.sport(), rst.dport()), (9, 40001));

        let _ = soclose(l, 0);
        let _ = take_sent6();
        teardown();
    }
}
/* </TESTS> */
