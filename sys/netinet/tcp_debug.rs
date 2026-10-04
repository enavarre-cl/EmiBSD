/*	$OpenBSD: tcp_debug.h,v 1.11 2018/05/10 13:30:25 bluhm Exp $	*/
/*	$NetBSD: tcp_debug.h,v 1.5 1994/06/29 06:38:38 cgd Exp $	*/
/*	$OpenBSD: tcp_debug.c,v 1.33 2025/07/08 00:47:41 jsg Exp $	*/
/*	$NetBSD: tcp_debug.c,v 1.10 1996/02/13 23:43:36 christos Exp $	*/
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
 *	@(#)tcp_debug.h	8.1 (Berkeley) 6/10/93
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

//! TCP debugging: `<netinet/tcp_debug.h>` (the trace record and the header overlay) and
//! `netinet/tcp_debug.c` (`tcp_trace`).
//!
//! Upstream: sys/netinet/tcp_debug.h @ 3ce1f3f79392
//! Upstream: sys/netinet/tcp_debug.c @ 3ce1f3f79392
//!
//! With `SO_DEBUG` on a socket, the TCP input, output, user-request and timer paths call
//! `tcp_trace`, which records the action, the state before it, the control block and the
//! segment's headers in the `tcp_debug[]` ring of `TCP_NDEBUG` entries (read from kernel
//! memory by `trpt(8)`). With `TCPDEBUG` the record is also printed.
//!
//! Locks used to protect struct members in this file: \[D\] `tcp_debug_mtx`.
//!
//! ## Deviations
//! - `TCPDEBUG` is not configured (as in GENERIC): the ring is filled, the console printout
//!   (`tcpconsdebug`, `tanames[]`, `tcptimers[]`, `prurequests[]`) is not compiled. The
//!   module is compiled because `SMALL_KERNEL` is not set (`conf/files`: `!small_kernel`).
//! - `td_cb`, the C's structure copy of the whole `struct tcpcb`, is [`TcpDebugCb`]: the
//!   connection's state and sequence space, its timers' flags and its window and timing
//!   variables, copied member by member. The control block's queues, timeouts and pointers
//!   cannot be copied in Rust (and are meaningless in a snapshot).
//! - `td_tcb` (`caddr_t`) is the address of `otp` as a `usize`: callers may pass a control
//!   block `tcp_close` has already freed, whose address alone is recorded, so `tcp_trace`
//!   takes it as a raw pointer it never dereferences. `headers` is `Option<&[u8]>`, the IP
//!   and TCP header bytes (`struct tcpiphdr` for IPv4).
//! - `ostate` is an `i32`, the type of `t_state` here (`tcp_var.rs`); the record keeps the
//!   C's `short`.
//! - `headers` is `struct tcpiphdr` for IPv4 or `struct tcpipv6hdr` for IPv6; `td_ti6` is
//!   filled for `PF_INET6` (`INET6` is configured, feature `inet6`: the `IPV6_VERSION`
//!   case of the header sniffing and the `PF_INET6` copy).
//! - The ring and `tcp_debx` are one `StaticCell` changed only with `tcp_debug_mtx` held.

use core::mem::size_of;
use core::ptr;

use libkern::StaticCell;

use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::machine::intr::IPL_SOFTNET;
use crate::netinet::ip::IPVERSION;
use crate::netinet::ip_icmp::iptime;
use crate::netinet::ip_var::Ipovly;
#[cfg(feature = "inet6")]
use crate::netinet::ip6::IPV6_VERSION;
use crate::netinet::ip6::{IPV6_VERSION_MASK, Ip6Hdr};
use crate::netinet::tcp::{TcpSeq, Tcphdr};
use crate::netinet::tcp_var::Tcpcb;
use crate::sys::mutex::Mutex;
#[cfg(feature = "inet6")]
use crate::sys::socket::PF_INET6;
use crate::sys::socket::{PF_INET, PF_UNSPEC};

/// `TA_INPUT`.
pub const TA_INPUT: i16 = 0;
/// `TA_OUTPUT`.
pub const TA_OUTPUT: i16 = 1;
/// `TA_USER`.
pub const TA_USER: i16 = 2;
/// `TA_RESPOND`.
pub const TA_RESPOND: i16 = 3;
/// `TA_DROP`.
pub const TA_DROP: i16 = 4;
/// `TA_TIMER`.
pub const TA_TIMER: i16 = 5;

/// `TCP_NDEBUG`: the size of the ring.
pub const TCP_NDEBUG: usize = 100;

/// `struct tcpipv6hdr`: tcp+ip6 header.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tcpipv6hdr {
    /// `ti6_i`: the IPv6 header.
    pub ti6_i: Ip6Hdr,
    /// `ti6_t`: the TCP header.
    pub ti6_t: Tcphdr,
}

impl Tcpipv6hdr {
    /// An all-zero header pair.
    pub const fn zeroed() -> Self {
        Self {
            ti6_i: Ip6Hdr::zeroed(),
            ti6_t: Tcpiphdr::zeroed().ti_t,
        }
    }
}

/// `struct tcpiphdr`: tcp+ip header, after ip options removed.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tcpiphdr {
    /// Overlaid ip structure.
    pub ti_i: Ipovly,
    /// Tcp header.
    pub ti_t: Tcphdr,
}

impl Tcpiphdr {
    /// An all-zero header pair.
    pub const fn zeroed() -> Self {
        Self {
            ti_i: Ipovly {
                ih_x1: [0; 9],
                ih_pr: 0,
                ih_len: 0,
                ih_src: crate::netinet::in_::InAddr { s_addr: 0 },
                ih_dst: crate::netinet::in_::InAddr { s_addr: 0 },
            },
            ti_t: Tcphdr {
                th_sport: 0,
                th_dport: 0,
                th_seq: 0,
                th_ack: 0,
                th_x2_off: 0,
                th_flags: 0,
                th_win: 0,
                th_sum: 0,
                th_urp: 0,
            },
        }
    }
}

/// The part of `struct tcpcb` a trace record keeps (see the deviations).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TcpDebugCb {
    /// `t_state`.
    pub t_state: i32,
    /// `t_rxtshift`.
    pub t_rxtshift: i16,
    /// `t_rxtcur`.
    pub t_rxtcur: i32,
    /// `t_dupacks`.
    pub t_dupacks: i16,
    /// `t_maxseg`.
    pub t_maxseg: u16,
    /// `t_force`.
    pub t_force: bool,
    /// `t_flags`.
    pub t_flags: u32,
    /// `snd_una`.
    pub snd_una: TcpSeq,
    /// `snd_nxt`.
    pub snd_nxt: TcpSeq,
    /// `snd_up`.
    pub snd_up: TcpSeq,
    /// `snd_wl1`.
    pub snd_wl1: TcpSeq,
    /// `snd_wl2`.
    pub snd_wl2: TcpSeq,
    /// `iss`.
    pub iss: TcpSeq,
    /// `snd_wnd`.
    pub snd_wnd: u64,
    /// `rcv_wnd`.
    pub rcv_wnd: u64,
    /// `rcv_nxt`.
    pub rcv_nxt: TcpSeq,
    /// `rcv_up`.
    pub rcv_up: TcpSeq,
    /// `irs`.
    pub irs: TcpSeq,
    /// `rcv_adv`.
    pub rcv_adv: TcpSeq,
    /// `snd_max`.
    pub snd_max: TcpSeq,
    /// `snd_cwnd`.
    pub snd_cwnd: u64,
    /// `snd_ssthresh`.
    pub snd_ssthresh: u64,
    /// `t_srtt`.
    pub t_srtt: i32,
    /// `t_rttvar`.
    pub t_rttvar: i32,
    /// `max_sndwnd`.
    pub max_sndwnd: u64,
    /// `snd_scale`.
    pub snd_scale: u8,
    /// `rcv_scale`.
    pub rcv_scale: u8,
    /// `pf`.
    pub pf: i32,
}

impl TcpDebugCb {
    /// An all-zero snapshot (the C's `bzero` of `td_cb`).
    pub const fn zeroed() -> Self {
        Self {
            t_state: 0,
            t_rxtshift: 0,
            t_rxtcur: 0,
            t_dupacks: 0,
            t_maxseg: 0,
            t_force: false,
            t_flags: 0,
            snd_una: 0,
            snd_nxt: 0,
            snd_up: 0,
            snd_wl1: 0,
            snd_wl2: 0,
            iss: 0,
            snd_wnd: 0,
            rcv_wnd: 0,
            rcv_nxt: 0,
            rcv_up: 0,
            irs: 0,
            rcv_adv: 0,
            snd_max: 0,
            snd_cwnd: 0,
            snd_ssthresh: 0,
            t_srtt: 0,
            t_rttvar: 0,
            max_sndwnd: 0,
            snd_scale: 0,
            rcv_scale: 0,
            pf: 0,
        }
    }

    /// The snapshot of `tp` (`td->td_cb = *tp`).
    pub fn of(tp: &Tcpcb) -> Self {
        Self {
            t_state: tp.t_state.get(),
            t_rxtshift: tp.t_rxtshift.get(),
            t_rxtcur: tp.t_rxtcur.get(),
            t_dupacks: tp.t_dupacks.get(),
            t_maxseg: tp.t_maxseg.get(),
            t_force: tp.t_force.get(),
            t_flags: tp.t_flags.get(),
            snd_una: tp.snd_una.get(),
            snd_nxt: tp.snd_nxt.get(),
            snd_up: tp.snd_up.get(),
            snd_wl1: tp.snd_wl1.get(),
            snd_wl2: tp.snd_wl2.get(),
            iss: tp.iss.get(),
            snd_wnd: tp.snd_wnd.get(),
            rcv_wnd: tp.rcv_wnd.get(),
            rcv_nxt: tp.rcv_nxt.get(),
            rcv_up: tp.rcv_up.get(),
            irs: tp.irs.get(),
            rcv_adv: tp.rcv_adv.get(),
            snd_max: tp.snd_max.get(),
            snd_cwnd: tp.snd_cwnd.get(),
            snd_ssthresh: tp.snd_ssthresh.get(),
            t_srtt: tp.t_srtt.get(),
            t_rttvar: tp.t_rttvar.get(),
            max_sndwnd: tp.max_sndwnd.get(),
            snd_scale: tp.snd_scale.get(),
            rcv_scale: tp.rcv_scale.get(),
            pf: tp.pf.get(),
        }
    }
}

/// `struct tcp_debug`: one trace record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TcpDebug {
    /// `td_time`.
    pub td_time: u32,
    /// `td_act` (`TA_*`).
    pub td_act: i16,
    /// `td_ostate`.
    pub td_ostate: i16,
    /// `td_tcb`: the address of the control block (see the deviations).
    pub td_tcb: usize,
    /// `td_ti`.
    pub td_ti: Tcpiphdr,
    /// `td_ti6`.
    pub td_ti6: Tcpipv6hdr,
    /// `td_req`.
    pub td_req: i16,
    /// `td_cb`.
    pub td_cb: TcpDebugCb,
}

impl TcpDebug {
    /// An all-zero record.
    pub const fn zeroed() -> Self {
        Self {
            td_time: 0,
            td_act: 0,
            td_ostate: 0,
            td_tcb: 0,
            td_ti: Tcpiphdr::zeroed(),
            td_ti6: Tcpipv6hdr::zeroed(),
            td_req: 0,
            td_cb: TcpDebugCb::zeroed(),
        }
    }
}

/// `tcp_debug[]` and `tcp_debx`.
pub struct TcpDebugRing {
    /// \[D\] `tcp_debug[TCP_NDEBUG]`.
    pub tcp_debug: [TcpDebug; TCP_NDEBUG],
    /// \[D\] `tcp_debx`: the next record to write.
    pub tcp_debx: usize,
}

/// `tcp_debug_mtx`: TCP debug global mutex.
pub static TCP_DEBUG_MTX: Mutex = Mutex::new(IPL_SOFTNET);

/// \[D\] `tcp_debug[]`, `tcp_debx`.
pub static TCP_DEBUG: StaticCell<TcpDebugRing> = StaticCell::new(TcpDebugRing {
    tcp_debug: [const { TcpDebug::zeroed() }; TCP_NDEBUG],
    tcp_debx: 0,
});

/// `tcp_trace`: tcp debug routine: records action `act` on `tp` (state `ostate` before it)
/// with the segment's `headers`.
pub fn tcp_trace(
    act: i16,
    ostate: i32,
    tp: Option<&Tcpcb>,
    otp: *const Tcpcb,
    headers: Option<&[u8]>,
    req: i32,
    len: i32,
) {
    let mut pf = i32::from(PF_UNSPEC);

    mtx_enter(&TCP_DEBUG_MTX);

    // SAFETY: `tcp_debug_mtx` is held, which serialises every access to the ring.
    let ring = unsafe { TCP_DEBUG.get_mut() };
    let td = &mut ring.tcp_debug[ring.tcp_debx];
    ring.tcp_debx += 1;
    if ring.tcp_debx == TCP_NDEBUG {
        ring.tcp_debx = 0;
    }
    td.td_time = iptime();
    td.td_act = act;
    td.td_ostate = ostate as i16;
    td.td_tcb = otp as usize;
    match tp {
        Some(tp) => {
            pf = tp.pf.get();
            td.td_cb = TcpDebugCb::of(tp);
        }
        None => td.td_cb = TcpDebugCb::zeroed(),
    }

    td.td_ti6 = Tcpipv6hdr::zeroed();
    td.td_ti = Tcpiphdr::zeroed();
    if let Some(h) = headers {
        // The address family may be in tcpcb or ip header.
        if pf == i32::from(PF_UNSPEC) {
            match h.first().map(|b| b & IPV6_VERSION_MASK) {
                #[cfg(feature = "inet6")]
                Some(IPV6_VERSION) => pf = i32::from(PF_INET6),
                Some(v) if v == IPVERSION << 4 => pf = i32::from(PF_INET),
                _ => {}
            }
        }
        #[cfg(feature = "inet6")]
        if pf == i32::from(PF_INET6) && h.len() >= size_of::<Tcpipv6hdr>() {
            // SAFETY: at least `size_of::<Tcpipv6hdr>()` bytes (checked); the structure is
            // integers, valid for any bytes, read unaligned.
            td.td_ti6 = unsafe { ptr::read_unaligned(h.as_ptr().cast::<Tcpipv6hdr>()) };
            td.td_ti6.ti6_i.ip6_plen = len as u16;
        }
        if pf == i32::from(PF_INET) && h.len() >= size_of::<Tcpiphdr>() {
            // SAFETY: at least `size_of::<Tcpiphdr>()` bytes (checked); the structure is
            // integers, valid for any bytes, read unaligned.
            td.td_ti = unsafe { ptr::read_unaligned(h.as_ptr().cast::<Tcpiphdr>()) };
            td.td_ti.ti_i.ih_len = len as u16;
        }
    }

    td.td_req = req as i16;
    // TCPDEBUG: the console printout of the record; not configured.
    mtx_leave(&TCP_DEBUG_MTX);
}

const _: () = assert!(size_of::<Tcpiphdr>() == 40);
const _: () = assert!(size_of::<Tcpipv6hdr>() == 60);

#[cfg(test)]
mod tests {
    use std::boxed::Box;

    use super::*;
    use crate::netinet::in_pcb::Inpcb;

    #[test]
    fn trace_fills_the_ring_and_wraps() {
        let inp: &'static Inpcb = Box::leak(Box::new(Inpcb::new(None, None)));
        let tp = Tcpcb::new(inp);
        tp.t_state.set(4);
        tp.snd_una.set(77);
        let mut hdr = [0u8; 40];
        hdr[0] = 0x45;
        for _ in 0..TCP_NDEBUG {
            tcp_trace(TA_INPUT, 3, Some(&tp), &tp, Some(&hdr), 0, 12);
        }
        // SAFETY: a single test thread touches the ring.
        let ring = unsafe { TCP_DEBUG.get() };
        let td = &ring.tcp_debug[(ring.tcp_debx + TCP_NDEBUG - 1) % TCP_NDEBUG];
        assert_eq!(td.td_cb.snd_una, 77);
        assert_eq!(td.td_ostate, 3);
        assert_eq!(td.td_ti.ti_i.ih_len, 12);
    }
}
