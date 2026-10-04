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
//! - The callouts take the `void *` of `timeout(9)` (`TimeoutFn`) and turn it back into the
//!   internet control block the timer holds a reference on. `tcp_timer_enter` returns the
//!   locked socket and the control block, or (`Err`) only what `tcp_timer_leave` must undo;
//!   the C's `goto out` is a labelled block. `otp`/`ostate` for `tcp_trace` are taken only
//!   with `SO_DEBUG`, as in C (`trace_start`).
//! - `tcp_timer_rexmt` builds its fake ICMP message in a local byte buffer viewed as an
//!   `IcmpPkt` (`icmp_mtudisc` reads the next-hop MTU and the returned IP header from it).
//!   `tcp_timer_keep` hands `tcp_respond` the template mbuf's bytes as a slice; without a
//!   template (never, once connected) no probe is sent.
//! - `tcp_backoff[]` is a `static` array; `tcp_totbackoff` a `const`. The keepalive
//!   parameters are `AtomicI32`s (the C reads them with `atomic_load_int`); `tcp_delack_msecs`
//!   (\[I\]) is a plain `static`.

use core::ffi::c_void;
use core::mem::size_of;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicI32, Ordering};

use crate::kassert;
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_timeout::{timeout_add_msec, timeout_del, timeout_set_flags};
use crate::kern::subr_pool::pool_put;
use crate::net::route::{RTF_HOST, RTV_MTU, rtfree};
use crate::netinet::in_::SockaddrIn;
use crate::netinet::in_pcb::{
    INP_IPV6, Inpcb, in_losing, in_pcbnotifyall, in_pcbref, in_pcbrtchange, in_pcbrtentry,
    in_pcbsolock, in_pcbsounlock, in_pcbunref,
};
use crate::netinet::ip::Ip;
use crate::netinet::ip_icmp::{ICMP_ADVLENMIN, IcmpPkt, icmp_mtudisc, icmp_mtudisc_clone};
use crate::netinet::ip_input::ip_mtudisc;
use crate::netinet::tcp::Tcphdr;
use crate::netinet::tcp_debug::{TA_TIMER, tcp_trace};
use crate::netinet::tcp_fsm::{
    TCPS_CLOSING, TCPS_SYN_RECEIVED, TCPS_SYN_SENT, TCPS_TIME_WAIT, tcps_haveestablished,
};
use crate::netinet::tcp_output::{tcp_output, tcp_setpersist};
use crate::netinet::tcp_seq::{TCP_ISSINCR2, seq_geq, seq_lt};
use crate::netinet::tcp_subr::{
    SACKHL_POOL, TCP_DO_ECN, TCP_ISS, TCP_TIMER_MTX, tcp_close, tcp_drop, tcp_mtudisc, tcp_respond,
};
use crate::netinet::tcp_usrreq::TCBTABLE;
use crate::netinet::tcp_var::{
    TCP_RTT_SHIFT, TF_ACKNOW, TF_DISABLE_ECN, TF_PMTUD_PEND, TF_SEND_CWR, TF_TIMER, Tcpcb,
    TcpstatCounters, intotcpcb, tcp_now, tcp_rexmtval, tcp_time, tcpstat_inc,
};
use crate::sys::errno::Errno;
use crate::sys::mbuf::mtod;
use crate::sys::protosw::PR_SLOWHZ;
use crate::sys::socket::{AF_INET, PF_INET, SO_DEBUG, SO_KEEPALIVE};
use crate::sys::socketvar::Socket;
use crate::sys::systm::{net_lock_shared, net_unlock_shared};
use crate::sys::timeout::{KCLOCK_NONE, TIMEOUT_MPSAFE, TIMEOUT_PROC, TimeoutFn, timeout_pending};

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

/// `tcp_totbackoff`: sum of `tcp_backoff[]`.
pub const TCP_TOTBACKOFF: i32 = 511;

/// \[a\] `tcp_always_keepalive`: assume `SO_KEEPALIVE` always set.
pub static TCP_ALWAYS_KEEPALIVE: AtomicI32 = AtomicI32::new(0);
/// \[a\] `tcp_keepinit`: time to keep alive initial SYN packet.
pub static TCP_KEEPINIT: AtomicI32 = AtomicI32::new(TCPTV_KEEPINIT);
/// \[a\] `tcp_keepidle`: time before keepalive probes begin.
pub static TCP_KEEPIDLE: AtomicI32 = AtomicI32::new(TCPTV_KEEPIDLE);
/// \[a\] `tcp_keepintvl`: time between keepalive probes.
pub static TCP_KEEPINTVL: AtomicI32 = AtomicI32::new(TCPTV_KEEPINTVL);
/// \[a\] `tcp_keepinit_sec`: copy of `tcp_keepinit` in seconds for sysctl.
pub static TCP_KEEPINIT_SEC: AtomicI32 = AtomicI32::new(TCPTV_KEEPINIT / tcp_time(1));
/// \[a\] `tcp_keepidle_sec`: copy of `tcp_keepidle` in seconds for sysctl.
pub static TCP_KEEPIDLE_SEC: AtomicI32 = AtomicI32::new(TCPTV_KEEPIDLE / tcp_time(1));
/// \[a\] `tcp_keepintvl_sec`: copy of `tcp_keepintvl` in seconds for sysctl.
pub static TCP_KEEPINTVL_SEC: AtomicI32 = AtomicI32::new(TCPTV_KEEPINTVL / tcp_time(1));
/// `tcp_maxpersistidle`: max idle time in persist.
pub static TCP_MAXPERSISTIDLE: AtomicI32 = AtomicI32::new(TCPTV_KEEPIDLE);
/// \[I\] `tcp_delack_msecs`: time to delay the ACK.
#[allow(non_upper_case_globals)] // TCP_DELACK_MSECS is a constant of tcp_timer.h
pub static tcp_delack_msecs: i32 = TCP_DELACK_MSECS;

/// `tcp_timer_funcs[]`: the callout of each timer, indexed by `TCPT_*`.
pub static TCP_TIMER_FUNCS: [TimeoutFn; TCPT_NTIMERS] = [
    tcp_timer_rexmt,
    tcp_timer_persist,
    tcp_timer_keep,
    tcp_timer_2msl,
    tcp_timer_delack,
];

/// `tcp_backoff[]`.
pub static TCP_BACKOFF: [i32; TCP_MAXRXTSHIFT as usize + 1] =
    [1, 2, 4, 8, 16, 32, 64, 64, 64, 64, 64, 64, 64];

/// `TCP_TIMER_INIT(tp, timer)`.
pub fn tcp_timer_init(tp: &Tcpcb, timer: usize) {
    timeout_set_flags(
        &tp.t_timer[timer],
        TCP_TIMER_FUNCS[timer],
        ptr::from_ref(tp.t_inpcb).cast_mut().cast(),
        KCLOCK_NONE,
        TIMEOUT_PROC | TIMEOUT_MPSAFE,
    );
}

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

/// What `tcp_timer_enter` found: the locked socket and the control block whose timer
/// fired, or (`Err`) the socket to unlock, if any, when there is nothing to do.
type TimerEnter = Result<(&'static Socket, &'static Tcpcb), Option<&'static Socket>>;

/// `tcp_timer_enter`: takes the shared net lock and the socket lock; `Err` for a socket gone,
/// a control block gone, or a timeout that was canceled or rescheduled meanwhile.
fn tcp_timer_enter(inp: &'static Inpcb, timer: usize) -> TimerEnter {
    kassert!(timer < TCPT_NTIMERS);

    net_lock_shared();
    let Some(so) = in_pcbsolock(inp) else {
        return Err(None);
    };
    let Some(tp) = intotcpcb(inp) else {
        return Err(Some(so));
    };
    // Ignore canceled timeouts or timeouts that have been rescheduled.
    if !tp.has_flags(TF_TIMER << timer) || timeout_pending(&tp.t_timer[timer]) {
        return Err(Some(so));
    }
    tp.clear_flags(TF_TIMER << timer);

    Ok((so, tp))
}

/// `tcp_timer_leave`: undoes `tcp_timer_enter` and drops the reference the armed timer held.
fn tcp_timer_leave(inp: &'static Inpcb, so: Option<&'static Socket>) {
    in_pcbsounlock(Some(inp), so);
    net_unlock_shared();
    in_pcbunref(Some(inp));
}

/// The internet control block a TCP timer was set with.
fn timer_inpcb(arg: *mut c_void) -> &'static Inpcb {
    // SAFETY: `tcp_timer_init` sets every timer's argument to `tp->t_inpcb`, and
    // `tcp_timer_arm` took a reference on it when the timer was added, which only
    // `tcp_timer_leave` (or `tcp_timer_disarm`) gives back: the block is alive here.
    unsafe { &*arg.cast::<Inpcb>().cast_const() }
}

/// `otp`/`ostate` of the callouts: the control block and its state before the timer, when
/// the socket has `SO_DEBUG`.
fn trace_start(so: &Socket, tp: &'static Tcpcb) -> Option<(*const Tcpcb, i32)> {
    so.has_options(SO_DEBUG)
        .then(|| (ptr::from_ref(tp), tp.t_state.get()))
}

/// `tcp_timer_delack`: callout to process delayed ACKs for a TCPCB.
pub fn tcp_timer_delack(arg: *mut c_void) {
    let inp = timer_inpcb(arg);

    // If tcp_output() wasn't able to transmit the ACK for whatever reason, it will restart
    // the delayed ACK callout.
    let so = match tcp_timer_enter(inp, TCPT_DELACK) {
        Ok((so, tp)) => {
            let otp = trace_start(so, tp);
            tp.set_flags(TF_ACKNOW);
            let _ = tcp_output(tp);
            if let Some((otp, ostate)) = otp {
                tcp_trace(TA_TIMER, ostate, Some(tp), otp, None, TCPT_DELACK as i32, 0);
            }
            Some(so)
        }
        Err(so) => so,
    };
    tcp_timer_leave(inp, so);
}

/// `tcp_slowtimo`: tcp protocol timeout routine called every 500 ms.
pub fn tcp_slowtimo() {
    mtx_enter(&TCP_TIMER_MTX);
    // increment iss
    TCP_ISS.store(
        TCP_ISS
            .load(Ordering::Relaxed)
            .wrapping_add(TCP_ISSINCR2 / PR_SLOWHZ as u32),
        Ordering::Relaxed,
    );
    mtx_leave(&TCP_TIMER_MTX);
}

/// `tcp_canceltimers`: cancel all timers for TCP `tp`.
pub fn tcp_canceltimers(tp: &Tcpcb) {
    for i in 0..TCPT_NTIMERS {
        tcp_timer_disarm(tp, i);
    }
}

/// `tcp_timer_freesack`: free SACK holes for 2MSL and REXMT timers.
pub fn tcp_timer_freesack(tp: &Tcpcb) {
    let mut q = tp.snd_holes.get();
    while let Some(p) = q {
        q = p.next.get();
        pool_put(&SACKHL_POOL, NonNull::from(p).cast());
    }
    tp.snd_holes.set(None);
}

/// `tcp_timer_rexmt`: the retransmit timer.
pub fn tcp_timer_rexmt(arg: *mut c_void) {
    let inp = timer_inpcb(arg);

    let (so, tp) = match tcp_timer_enter(inp, TCPT_REXMT) {
        Ok(v) => v,
        Err(so) => return tcp_timer_leave(inp, so),
    };

    if tp.has_flags(TF_PMTUD_PEND)
        && seq_geq(tp.t_pmtud_th_seq.get(), tp.snd_una.get())
        && seq_lt(
            tp.t_pmtud_th_seq.get(),
            tp.snd_una.get().wrapping_add(u32::from(tp.t_maxseg.get())),
        )
    {
        // TF_PMTUD_PEND is set in tcp_ctlinput() which is IPv4 only
        kassert!(!inp.has_flags(INP_IPV6));
        tp.clear_flags(TF_PMTUD_PEND);

        let rtableid = inp.inp_rtableid.get();

        // XXX create fake icmp message with relevant entries
        let mut icmp = [0u8; ICMP_ADVLENMIN];
        // SAFETY: a local buffer of `ICMP_ADVLENMIN` bytes, used while it lives.
        let icp = unsafe { IcmpPkt::new(icmp.as_mut_ptr(), icmp.len()) };
        icp.set_icmp_nextmtu(tp.t_pmtud_nextmtu.get() as u16);
        let mut oip = icp.icmp_ip();
        oip.ip_len = tp.t_pmtud_ip_len.get();
        oip.set_ip_hl(tp.t_pmtud_ip_hl.get() as u8);
        oip.ip_dst = inp.inp_faddr.get();
        // SAFETY: `icmp_ip_ptr` is in bounds of the local buffer; an unaligned write.
        unsafe { ptr::write_unaligned(icp.icmp_ip_ptr(), oip) };

        // Notify all connections to the same peer about new mss and trigger retransmit.
        let sin = SockaddrIn {
            sin_len: size_of::<SockaddrIn>() as u8,
            sin_family: AF_INET,
            sin_addr: inp.inp_faddr.get(),
            ..SockaddrIn::default()
        };

        in_pcbsounlock(Some(inp), Some(so));
        in_pcbunref(Some(inp));

        icmp_mtudisc(&icp, rtableid);
        in_pcbnotifyall(
            &TCBTABLE,
            &sin,
            rtableid,
            Some(Errno::EMSGSIZE),
            Some(tcp_mtudisc),
        );

        net_unlock_shared();
        return;
    }

    tcp_timer_freesack(tp);
    tp.t_rxtshift.set(tp.t_rxtshift.get() + 1);
    if i32::from(tp.t_rxtshift.get()) > TCP_MAXRXTSHIFT {
        tp.t_rxtshift.set(TCP_MAXRXTSHIFT as i16);
        tcpstat_inc(TcpstatCounters::TcpsTimeoutdrop);
        let _ = tcp_drop(tp, Some(tp.t_softerror.get().unwrap_or(Errno::ETIMEDOUT)));
        return tcp_timer_leave(inp, Some(so));
    }
    let otp = trace_start(so, tp);
    tcpstat_inc(TcpstatCounters::TcpsRexmttimeo);
    let rto = (tcp_rexmtval(tp) as u32).max(tp.t_rttmin.get());
    tp.t_rxtcur.set(tcpt_rangeset(
        rto.wrapping_mul(TCP_BACKOFF[tp.t_rxtshift.get() as usize] as u32),
        tp.t_rttmin.get(),
        TCPTV_REXMTMAX as u32,
    ) as i32);
    tcp_timer_arm(tp, TCPT_REXMT, tp.t_rxtcur.get() as u64);

    // If we are losing and we are trying path MTU discovery, try turning it off. This will
    // avoid black holes in the network which suppress or fail to send "packet too big" ICMP
    // messages. We should ideally do lots more sophisticated searching to find the right
    // value here...
    if ip_mtudisc.load(Ordering::Relaxed) != 0
        && tcps_haveestablished(tp.t_state.get())
        && i32::from(tp.t_rxtshift.get()) > TCP_MAXRXTSHIFT / 6
    {
        'leave: {
            // No data to send means path mtu is not a problem
            if so.so_snd.sb_cc.get() == 0 {
                break 'leave;
            }

            let rt = in_pcbrtentry(inp);
            // Check if path MTU discovery is disabled already
            if let Some(rt) = rt
                && rt.rt_flags.get() & RTF_HOST != 0
                && rt.rt_locks().get() & RTV_MTU != 0
            {
                break 'leave;
            }

            let rt = match tp.pf.get() {
                // We can not turn off path MTU for IPv6. Do nothing for now, maybe lower to
                // minimum MTU.
                #[cfg(feature = "inet6")]
                pf if pf == i32::from(crate::sys::socket::PF_INET6) => None,
                pf if pf == i32::from(PF_INET) => {
                    icmp_mtudisc_clone(inp.inp_faddr.get(), inp.inp_rtableid.get(), false)
                }
                _ => None,
            };
            if let Some(rt) = rt {
                // Disable path MTU discovery
                if rt.rt_locks().get() & RTV_MTU == 0 {
                    rt.rt_locks().set(rt.rt_locks().get() | RTV_MTU);
                    in_pcbrtchange(inp, None);
                }

                rtfree(Some(rt));
            }
        }
    }

    // If losing, let the lower level know and try for a better route. Also, if we backed off
    // this far, our srtt estimate is probably bogus. Clobber it so we'll take the next rtt
    // measurement as our srtt; move the current srtt into rttvar to keep the current
    // retransmit times until then.
    if i32::from(tp.t_rxtshift.get()) > TCP_MAXRXTSHIFT / 4 {
        in_losing(inp);
        tp.t_rttvar
            .set(tp.t_rttvar.get() + (tp.t_srtt.get() >> TCP_RTT_SHIFT));
        tp.t_srtt.set(0);
    }
    tp.snd_nxt.set(tp.snd_una.get());
    // Note: We overload snd_last to function also as the snd_last variable described in RFC
    // 2582
    tp.snd_last.set(tp.snd_max.get());
    // If timing a segment in this window, stop the timer.
    tp.t_rtttime.set(0);
    // TCP_ECN: if ECN is enabled, there might be a broken firewall which blocks ecn packets.
    // fall back to non-ecn.
    if (tp.t_state.get() == TCPS_SYN_SENT || tp.t_state.get() == TCPS_SYN_RECEIVED)
        && TCP_DO_ECN.load(Ordering::Relaxed) != 0
        && !tp.has_flags(TF_DISABLE_ECN)
    {
        tp.set_flags(TF_DISABLE_ECN);
    }
    // Close the congestion window down to one segment (we'll open it by one segment for each
    // ack we get). Since we probably have a window's worth of unacked data accumulated, this
    // "slow start" keeps us from dumping all that data as back-to-back packets (which might
    // overwhelm an intermediate gateway).
    //
    // There are two phases to the opening: Initially we open by one mss on each ack. This
    // makes the window size increase exponentially with time. If the window is larger than
    // the path can handle, this exponential growth results in dropped packet(s) almost
    // immediately. To get more time between drops but still "push" the network to take
    // advantage of improving conditions, we switch from exponential to linear window opening
    // at some threshold size. For a threshold, we use half the current window size,
    // truncated to a multiple of the mss.
    //
    // (the minimum cwnd that will give us exponential growth is 2 mss. We don't allow the
    // threshold to go below this.)
    {
        let maxseg = u64::from(tp.t_maxseg.get());
        let win = (tp.snd_wnd.get().min(tp.snd_cwnd.get()) / 2 / maxseg).max(2);
        tp.snd_cwnd.set(maxseg);
        tp.snd_ssthresh.set(win * maxseg);
        tp.t_dupacks.set(0);
        // TCP_ECN
        tp.snd_last.set(tp.snd_max.get());
        tp.set_flags(TF_SEND_CWR);
        // #if 1 /* TCP_ECN */
        tcpstat_inc(TcpstatCounters::TcpsCwrTimeout);
    }
    let _ = tcp_output(tp);
    if let Some((otp, ostate)) = otp {
        tcp_trace(TA_TIMER, ostate, Some(tp), otp, None, TCPT_REXMT as i32, 0);
    }
    tcp_timer_leave(inp, Some(so));
}

/// `tcp_timer_persist`: the persist timer.
pub fn tcp_timer_persist(arg: *mut c_void) {
    let inp = timer_inpcb(arg);

    let so = 'out: {
        let (so, tp) = match tcp_timer_enter(inp, TCPT_PERSIST) {
            Ok(v) => v,
            Err(so) => break 'out so,
        };

        if tcp_timer_isarmed(tp, TCPT_REXMT) {
            break 'out Some(so);
        }

        let otp = trace_start(so, tp);
        tcpstat_inc(TcpstatCounters::TcpsPersisttimeo);
        // Hack: if the peer is dead/unreachable, we do not time out if the window is
        // closed. After a full backoff, drop the connection if the idle time (no responses
        // to probes) reaches the maximum backoff that we would use if retransmitting.
        let rto = (tcp_rexmtval(tp) as u32).max(tp.t_rttmin.get());
        let now = tcp_now();
        let idle = now.wrapping_sub(tp.t_rcvtime.get());
        if i32::from(tp.t_rxtshift.get()) == TCP_MAXRXTSHIFT
            && (idle >= TCP_MAXPERSISTIDLE.load(Ordering::Relaxed) as u64
                || idle >= u64::from(rto) * TCP_TOTBACKOFF as u64)
        {
            tcpstat_inc(TcpstatCounters::TcpsPersistdrop);
            let _ = tcp_drop(tp, Some(Errno::ETIMEDOUT));
            break 'out Some(so);
        }
        tcp_setpersist(tp);
        tp.t_force.set(true);
        let _ = tcp_output(tp);
        tp.t_force.set(false);
        if let Some((otp, ostate)) = otp {
            tcp_trace(
                TA_TIMER,
                ostate,
                Some(tp),
                otp,
                None,
                TCPT_PERSIST as i32,
                0,
            );
        }
        Some(so)
    };
    tcp_timer_leave(inp, so);
}

/// `tcp_timer_keep`: the keepalive (and connection establishment) timer.
pub fn tcp_timer_keep(arg: *mut c_void) {
    let inp = timer_inpcb(arg);

    let so = 'out: {
        let (so, tp) = match tcp_timer_enter(inp, TCPT_KEEP) {
            Ok(v) => v,
            Err(so) => break 'out so,
        };

        let otp = trace_start(so, tp);
        tcpstat_inc(TcpstatCounters::TcpsKeeptimeo);
        if !tcps_haveestablished(tp.t_state.get()) {
            tcpstat_inc(TcpstatCounters::TcpsKeepdrops);
            let _ = tcp_drop(tp, Some(Errno::ETIMEDOUT));
            break 'out Some(so);
        }
        if (TCP_ALWAYS_KEEPALIVE.load(Ordering::Relaxed) != 0 || so.has_options(SO_KEEPALIVE))
            && tp.t_state.get() <= TCPS_CLOSING
        {
            let keepidle = TCP_KEEPIDLE.load(Ordering::Relaxed);
            let keepintvl = TCP_KEEPINTVL.load(Ordering::Relaxed);
            let maxidle = TCPTV_KEEPCNT * keepintvl;
            let now = tcp_now();
            if maxidle > 0 && now.wrapping_sub(tp.t_rcvtime.get()) >= (keepidle + maxidle) as u64 {
                tcpstat_inc(TcpstatCounters::TcpsKeepdrops);
                let _ = tcp_drop(tp, Some(Errno::ETIMEDOUT));
                break 'out Some(so);
            }
            // Send a packet designed to force a response if the peer is up and reachable:
            // either an ACK if the connection is still alive, or an RST if the peer has
            // closed the connection due to timeout or reboot. Using sequence number
            // tp->snd_una-1 causes the transmitted zero-length segment to lie outside the
            // receive window; by the protocol spec, this requires the correspondent TCP to
            // respond.
            tcpstat_inc(TcpstatCounters::TcpsKeepprobe);
            if let Some(t) = tp.t_template.get() {
                let len = (t.m_len().get() as usize).min(size_of::<Ip>() + size_of::<Tcphdr>());
                // SAFETY: the template mbuf holds `m_len` bytes at its data pointer
                // (`tcp_template` filled them) and lives with the control block.
                let template = unsafe { core::slice::from_raw_parts(mtod::<u8>(t), len) };
                tcp_respond(
                    Some(tp),
                    template,
                    None,
                    tp.rcv_nxt.get(),
                    tp.snd_una.get().wrapping_sub(1),
                    0,
                    0,
                    now,
                );
            }
            tcp_timer_arm(tp, TCPT_KEEP, keepintvl as u64);
        } else {
            tcp_timer_arm(tp, TCPT_KEEP, TCP_KEEPIDLE.load(Ordering::Relaxed) as u64);
        }
        if let Some((otp, ostate)) = otp {
            tcp_trace(TA_TIMER, ostate, Some(tp), otp, None, TCPT_KEEP as i32, 0);
        }
        Some(so)
    };
    tcp_timer_leave(inp, so);
}

/// `tcp_timer_2msl`: the 2*MSL (TIME_WAIT) and FIN_WAIT_2 timer.
pub fn tcp_timer_2msl(arg: *mut c_void) {
    let inp = timer_inpcb(arg);

    let so = match tcp_timer_enter(inp, TCPT_2MSL) {
        Ok((so, tp)) => {
            let otp = trace_start(so, tp);
            tcp_timer_freesack(tp);

            let keepintvl = TCP_KEEPINTVL.load(Ordering::Relaxed);
            let maxidle = TCPTV_KEEPCNT * keepintvl;
            let now = tcp_now();
            let tp = if tp.t_state.get() != TCPS_TIME_WAIT
                && (maxidle == 0 || now.wrapping_sub(tp.t_rcvtime.get()) <= maxidle as u64)
            {
                tcp_timer_arm(tp, TCPT_2MSL, keepintvl as u64);
                Some(tp)
            } else {
                tcp_close(tp)
            };
            if let Some((otp, ostate)) = otp {
                tcp_trace(TA_TIMER, ostate, tp, otp, None, TCPT_2MSL as i32, 0);
            }
            Some(so)
        }
        Err(so) => so,
    };
    tcp_timer_leave(inp, so);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rangeset_clamps_as_the_macro() {
        assert_eq!(tcpt_rangeset(5, 10, 20), 10);
        assert_eq!(tcpt_rangeset(25, 10, 20), 20);
        assert_eq!(tcpt_rangeset(15, 10, 20), 15);
        // The macro tests the minimum first: an empty range yields the minimum.
        assert_eq!(tcpt_rangeset(15, 30, 20), 30);
    }

    #[test]
    fn backoff_sums_to_the_total() {
        assert_eq!(TCP_BACKOFF.iter().sum::<i32>(), TCP_TOTBACKOFF);
        assert_eq!(TCPTV_KEEPIDLE, 7_200_000);
        assert_eq!(TCP_KEEPINTVL_SEC.load(Ordering::Relaxed), 75);
    }
}
