/* <LICENSES> */
/*
 * Copyright (C) 1995, 1996, 1997, and 1998 WIDE Project.
 * All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. Neither the name of the project nor the names of its contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE PROJECT AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE PROJECT OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 */

/*
 * Copyright (c) 1982, 1986, 1990, 1993
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
 *	@(#)in_pcb.h	8.1 (Berkeley) 6/10/93
 */

/*
 * Copyright (c) 1982, 1986, 1991, 1993
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
//! Internet protocol control blocks: `<netinet/in_pcb.h>` and `netinet/in_pcb.c`.
//!
//! Upstream: sys/netinet/in_pcb.h @ 3ce1f3f79392
//! Upstream: sys/netinet/in_pcb.c @ 3ce1f3f79392
//!
//! Every UDP and raw IP socket has an [`Inpcb`]: the local and foreign addresses and ports,
//! the cached route, the IP header prototype and the socket options of the IP layer. The
//! control blocks of one protocol live in an [`Inpcbtable`]: a queue of all of them and two
//! SipHash tables, one on the complete address quadruple (`in_pcblookup`, connected sockets)
//! and one on the local port (`in_pcblookup_local_lock`, bind(2)'s checks and the wildcard
//! matches). `in_pcblookup_listen` finds a socket bound to the local address or the wildcard
//! address. Raw IP and UDP input walk the queue with an iterator that marks its place
//! (`in_pcb_iterator`), so the table mutex can be dropped while a packet is appended to a
//! socket.
//!
//! The pcb table mutex guarantees that all inpcb are consistent and that bind(2) and
//! connect(2) create unique combinations of laddr/faddr/lport/fport/rtableid. It protects both
//! address consistency and inpcb lookup during protocol input. All writes to
//! `inp_[lf]addr` take the table mutex. A per socket lock is needed so that the socket layer
//! input has a consistent view of these values. In `soconnect()` and `sosend()` a per pcb
//! mutex cannot be used: they eventually call IP output, which takes sleeping locks, and
//! connect(2) does a route lookup for source selection. Protocol input should not switch
//! process per packet, so it spins on the mutex. So there are three locks: the table mutex
//! for writing `inp_[lf]addr/port` and lookup, the socket rw-lock to separate sockets in
//! system calls, and the socket buffer mutex for the receive buffer. Changing
//! `inp_[lf]addr/port` takes both the per socket rw-lock and the global table mutex;
//! protocol input only reads them during lookup.
//!
//! Locks used to protect struct members: \[I\] immutable after creation, \[N\] net lock,
//! \[t\] `inpt_mtx` (pcb table mutex), \[s\] `so_lock` (socket rwlock), \[a\] atomic.
//!
//! ## Deviations
//! - An `Inpcb` is an `inpcb_pool` item handed around as `&'static Inpcb`, valid while a
//!   reference is held (`in_pcbref`/`in_pcbunref`, the table mutex, or the socket's
//!   `so_pcb`); the members the C changes through a pointer are `Cell`s under the locks above.
//! - The C's unions (`inp_faddru`, `inp_laddru`, the header prototype `inp_hu`, the options
//!   and the multicast options) are their two members side by side: `inp_faddr`/`inp_faddr6`,
//!   `inp_laddr`/`inp_laddr6`, `inp_ip`/`inp_ipv6`, `inp_options`/`inp_outputopts6`,
//!   `inp_moptions`/`inp_moptions6`; a pcb uses the members of its family (`INP_IPV6`), as
//!   the C reads the union through the macro of its family. `inp_ip6_minhlim` (the
//!   `inp_ip_minttl` alias) and `inp_flowinfo` (`inp_ipv6.ip6_flow`) are methods.
//! - The `in6_*` pcb functions are `netinet6/in6_pcb.rs`'s; the `ISSET(inp_flags, INP_IPV6)`
//!   branches of this file (`#ifdef INET6`, feature `inet6`) call them.
//! - The `const void *laddr` of `in_pcbbind_locked`, `in_pcbpickport` and
//!   `in_pcblookup_local_lock` is an [`Inpaddru`] (`union inpaddru` by value), read through
//!   `iau_addr()`/`iau_addr6()` as the C reads the pointer as the address of the family
//!   `INP_IPV6`/`INPLOOKUP_IPV6` says; `zeroin46_addr` is [`ZEROIN46_ADDR`].
//! - `sotoinpcb` panics on a socket of neither `inetdomain` nor `inet6domain` (the C macro
//!   casts `so_pcb` blindly).
//! - `inp_pf_sk` is pf(4)'s state key. `inp_seclevel` is a `Cell` (`IPSEC` is configured
//!   since M9c).
//! - `struct inpcb_iterator` is [`InpcbIterator`], a whole `Inpcb` whose `inp_table` and
//!   `inp_socket` are `None`, so it can sit in the table's queue like the C's prefix-compatible
//!   structure. [`in_pcb_iterator`] is an `unsafe fn`: the iterator must stay in place until
//!   the walk ends. `inp_socket` is an `Option` for that reason only; [`Inpcb::socket`]
//!   returns the socket of a real control block.
//! - `in_pcbunref`'s three `KASSERT`s that the links are cleared are not made: the queue
//!   links are private to `sys/queue.rs`.
//! - `baddynamicports` and `rootonlyports` hold atomic words (written under the net lock by
//!   `ip_init` and the sysctls, read during port selection); `DEFBADDYNAMICPORTS_*` and
//!   `DEFROOTONLYPORTS_*` are slices without the C's terminating 0.
//! - `hashinit(M_WAITOK)` cannot fail in C; here its failure in `in_pcbinit` panics.
//! - `in_pcbaddrisavail_lock` clears the port and `sin_zero` of a copy of the address (the C
//!   clears them in the caller's mbuf and puts the port back).
//! - `in_pcbset_addr` takes its generic `struct sockaddr *`s as [`SockaddrUnion`]s (the
//!   SYN cache's `union syn_cache_sa`, its only caller), read by family.
//! - `NSTOEPLITZ` (`pseudo-device pf` needs `stoeplitz`) and `NPF` (pf(4)) are configured:
//!   the flow id of a connected or bound socket, `pf_remove_divert_state`, `pf_inp_unlink`,
//!   and the divert and redirected-localhost keys of `in_pcblookup_listen`. So is `IPSEC`
//!   (M9c): `in_baddynamic` refuses the `udpencap_port`.
//! - The `DIAGNOSTIC` `in_pcbnotifymiss` printfs are behind the `diagnostic` feature.

use core::cell::Cell;
use core::ffi::c_void;
use core::mem::size_of;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicI32, AtomicU32, Ordering};

use crate::crypto::siphash::{
    SipHash24_End, SipHash24_Init, SipHash24_Update, SiphashCtx, SiphashKey,
};
use crate::dev::rnd::{arc4random_buf, arc4random_uniform};
use crate::kassert;
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_prot::suser;
use crate::kern::kern_rwlock::{rw_enter_write, rw_exit_write};
use crate::kern::kern_subr::{hashfree, hashinit};
use crate::kern::kern_synch::{refcnt_init, refcnt_rele, refcnt_take};
use crate::kern::subr_pool::{pool_get, pool_init, pool_put};
use crate::kern::subr_prf::panic;
use crate::kern::uipc_mbuf::m_freem;
use crate::kern::uipc_socket::{sofree, sorele};
use crate::kern::uipc_socket2::soassertlocked;
use crate::machine::cpu::curproc;
use crate::machine::intr::IPL_SOFTNET;
#[cfg(feature = "inet6")]
use crate::net::if_::unhandled_af;
use crate::net::if_::{IFF_UP, if_get, if_put, ifa_ifwithaddr};
use crate::net::if_var::Netstack;
use crate::net::pf::{pf_find_divert, pf_inp_unlink, pf_remove_divert_state};
use crate::net::pfvar::{PF_DIVERT_REPLY, PF_DIVERT_TO};
use crate::net::route::{
    RTF_DYNAMIC, RTF_GATEWAY, Route, Rtentry, route_mpath, rtdeletemsg, rtfree,
};
use crate::net::rtable::{rtable_exists, rtable_getsource, rtable_l2};
use crate::net::toeplitz::stoeplitz_ip4port;
use crate::netinet::icmp6::Icmp6Filter;
use crate::netinet::in_::{
    INADDR_ANY, INADDR_BROADCAST, IPPORT_HIFIRSTAUTO, IPPORT_HILASTAUTO, IPPORT_RESERVED,
    IPPORT_USERRESERVED, IPPROTO_TCP, IPPROTO_UDP, IPSEC_AUTH_LEVEL_DEFAULT,
    IPSEC_ESP_NETWORK_LEVEL_DEFAULT, IPSEC_ESP_TRANS_LEVEL_DEFAULT, IPSEC_IPCOMP_LEVEL_DEFAULT,
    InAddr, SockaddrIn, in_broadcast, in_ifp2ia, in_multicast, in_nam2sin, satosin_const, sintosa,
};
use crate::netinet::in_var::ifatoia;
use crate::netinet::ip::Ip;
use crate::netinet::ip_ipsp::{IpsecLevel, SockaddrUnion};
use crate::netinet::ip_output::ip_freemoptions;
use crate::netinet::ip_var::IpMoptions;
use crate::netinet::ip6::Ip6Hdr;
use crate::netinet::ipsec_output::UDPENCAP_PORT;
#[cfg(feature = "inet6")]
use crate::netinet6::in6::{
    IN6ADDR_ANY, SockaddrIn6, in6_are_addr_equal, in6_is_addr_unspecified, in6_nam2sin6,
};
use crate::netinet6::in6::{IN6ADDR_ANY_INIT, In6Addr};
#[cfg(feature = "inet6")]
use crate::netinet6::in6_pcb::{
    in6_pcbaddrisavail_lock, in6_pcbconnect, in6_pcbhash, in6_pcbrtentry, in6_pcbset_addr,
    in6_setpeeraddr, in6_setsockaddr,
};
#[cfg(feature = "inet6")]
use crate::netinet6::ip6_output::{ip6_freemoptions, ip6_freepcbopts};
use crate::netinet6::ip6_var::{Ip6Moptions, Ip6Pktopts};
use crate::queue_adapter;
use crate::sys::endian::htons;
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_NOWAIT, M_PCB, M_WAITOK};
use crate::sys::mbuf::{M_WAIT, Mbuf, PF_TAG_DIVERTED, PF_TAG_TRANSLATE_LOCALHOST, mtod};
use crate::sys::mutex::{Mutex, mutex_assert_locked};
use crate::sys::pool::{PR_NOWAIT, PR_WAITOK, PR_ZERO, Pool};
use crate::sys::proc::Proc;
use crate::sys::protosw::PR_CONNREQUIRED;
use crate::sys::queue::{ListEntry, ListHead, TailqEntry, TailqHead};
use crate::sys::refcnt::Refcnt;
use crate::sys::socket::{
    AF_INET, SO_ACCEPTCONN, SO_BINDANY, SO_REUSEADDR, SO_REUSEPORT, SOCK_DGRAM,
};
#[cfg(feature = "inet6")]
use crate::sys::socket::{AF_INET6, PF_INET, PF_INET6};
use crate::sys::socketvar::{SS_NOFDREF, Socket, soref};
use crate::sys::systm::net_assert_locked;

// flags in inp_flags:

/// `INP_RECVOPTS`: receive incoming IP options.
pub const INP_RECVOPTS: i32 = 0x001;
/// `INP_RECVRETOPTS`: receive IP options for reply.
pub const INP_RECVRETOPTS: i32 = 0x002;
/// `INP_RECVDSTADDR`: receive IP dst address.
pub const INP_RECVDSTADDR: i32 = 0x004;

/// `INP_RXDSTOPTS`.
pub const INP_RXDSTOPTS: i32 = INP_RECVOPTS;
/// `INP_RXHOPOPTS`.
pub const INP_RXHOPOPTS: i32 = INP_RECVRETOPTS;
/// `INP_RXINFO`.
pub const INP_RXINFO: i32 = INP_RECVDSTADDR;
/// `INP_RXSRCRT`.
pub const INP_RXSRCRT: i32 = 0x010;
/// `INP_HOPLIMIT`.
pub const INP_HOPLIMIT: i32 = 0x020;

/// `INP_HDRINCL`: user supplies entire IP header.
pub const INP_HDRINCL: i32 = 0x008;
/// `INP_HIGHPORT`: user wants "high" port binding.
pub const INP_HIGHPORT: i32 = 0x010;
/// `INP_LOWPORT`: user wants "low" port binding.
pub const INP_LOWPORT: i32 = 0x020;
/// `INP_RECVIF`: receive incoming interface.
pub const INP_RECVIF: i32 = 0x080;
/// `INP_RECVTTL`: receive incoming IP TTL.
pub const INP_RECVTTL: i32 = 0x040;
/// `INP_RECVDSTPORT`: receive IP dst addr before rdr.
pub const INP_RECVDSTPORT: i32 = 0x200;
/// `INP_RECVRTABLE`: receive routing table.
pub const INP_RECVRTABLE: i32 = 0x400;
/// `INP_IPSECFLOWINFO`: receive IPsec flow info.
pub const INP_IPSECFLOWINFO: i32 = 0x800;

/// `INP_CONTROLOPTS`.
pub const INP_CONTROLOPTS: i32 = INP_RECVOPTS
    | INP_RECVRETOPTS
    | INP_RECVDSTADDR
    | INP_RXSRCRT
    | INP_HOPLIMIT
    | INP_RECVIF
    | INP_RECVTTL
    | INP_RECVDSTPORT
    | INP_RECVRTABLE;

// These flags' values should be determined by either the transport protocol at PRU_BIND,
// PRU_LISTEN, PRU_CONNECT, etc, or by in_pcb*().

/// `INP_IPV6`: socket, proto, domain, family is `PF_INET6`.
pub const INP_IPV6: i32 = 0x100;

// Flags in inp_flags for IPV6

/// `IN6P_HIGHPORT`: user wants "high" port.
pub const IN6P_HIGHPORT: i32 = INP_HIGHPORT;
/// `IN6P_LOWPORT`: user wants "low" port.
pub const IN6P_LOWPORT: i32 = INP_LOWPORT;
/// `IN6P_RECVDSTPORT`: receive IP dst addr before rdr.
pub const IN6P_RECVDSTPORT: i32 = INP_RECVDSTPORT;
/// `IN6P_PKTINFO`: receive IP6 dst and I/F.
pub const IN6P_PKTINFO: i32 = 0x010000;
/// `IN6P_HOPLIMIT`: receive hoplimit.
pub const IN6P_HOPLIMIT: i32 = 0x020000;
/// `IN6P_HOPOPTS`: receive hop-by-hop options.
pub const IN6P_HOPOPTS: i32 = 0x040000;
/// `IN6P_DSTOPTS`: receive dst options after rthdr.
pub const IN6P_DSTOPTS: i32 = 0x080000;
/// `IN6P_RTHDR`: receive routing header.
pub const IN6P_RTHDR: i32 = 0x100000;
/// `IN6P_TCLASS`: receive traffic class value.
pub const IN6P_TCLASS: i32 = 0x400000;
/// `IN6P_AUTOFLOWLABEL`: attach flowlabel automatically.
pub const IN6P_AUTOFLOWLABEL: i32 = 0x800000;

/// `IN6P_ANONPORT`: port chosen for user.
pub const IN6P_ANONPORT: i32 = 0x4000000;
/// `IN6P_RFC2292`: used RFC2292 API on the socket.
pub const IN6P_RFC2292: i32 = 0x40000000;
/// `IN6P_MTU`: receive path MTU (bit 31 of the C's `int`).
pub const IN6P_MTU: i32 = 0x80000000_u32 as i32;

/// `IN6P_MINMTU`: use minimum MTU.
pub const IN6P_MINMTU: i32 = 0x20000000;

/// `IN6P_CONTROLOPTS`.
pub const IN6P_CONTROLOPTS: i32 = IN6P_PKTINFO
    | IN6P_HOPLIMIT
    | IN6P_HOPOPTS
    | IN6P_DSTOPTS
    | IN6P_RTHDR
    | IN6P_TCLASS
    | IN6P_AUTOFLOWLABEL
    | IN6P_RFC2292
    | IN6P_MTU
    | IN6P_RECVDSTPORT;

/// `INPLOOKUP_WILDCARD`.
pub const INPLOOKUP_WILDCARD: i32 = 1;
/// `INPLOOKUP_SETLOCAL`.
pub const INPLOOKUP_SETLOCAL: i32 = 2;
/// `INPLOOKUP_IPV6`.
pub const INPLOOKUP_IPV6: i32 = 4;

// macros for handling bitmap of ports not to allocate dynamically

/// `DP_MAPBITS`: bits per map word.
pub const DP_MAPBITS: usize = u32::BITS as usize;
/// `DP_MAPSIZE`: words per map.
pub const DP_MAPSIZE: usize = 65536_usize.div_ceil(DP_MAPBITS);

/// `DEFBADDYNAMICPORTS_TCP`: default values for `baddynamicports` (see `ip_init()`).
pub const DEFBADDYNAMICPORTS_TCP: &[u16] = &[
    587, 749, 750, 751, 853, 871, 2049, 6000, 6001, 6002, 6003, 6004, 6005, 6006, 6007, 6008, 6009,
    6010,
];
/// `DEFBADDYNAMICPORTS_UDP` (3784, 3785, 7784: BFD/S-BFD ports).
pub const DEFBADDYNAMICPORTS_UDP: &[u16] = &[623, 664, 749, 750, 751, 2049, 3784, 3785, 7784];

/// `DEFROOTONLYPORTS_TCP`.
pub const DEFROOTONLYPORTS_TCP: &[u16] = &[2049];
/// `DEFROOTONLYPORTS_UDP`.
pub const DEFROOTONLYPORTS_UDP: &[u16] = &[2049];

/// `IN_PCBLOCK_HOLD`: the caller holds the table mutex.
pub const IN_PCBLOCK_HOLD: i32 = 1;
/// `IN_PCBLOCK_GRAB`: the lookup takes the table mutex and a reference.
pub const IN_PCBLOCK_GRAB: i32 = 2;

/// `INPCBHASH_LOADFACTOR(x)`.
const fn inpcbhash_loadfactor(x: i32) -> i32 {
    (x * 3) / 4
}

/// `inp_upcall`: a hook that sees UDP datagrams before the socket buffer
/// (`(arg, m, ip, ip6, uh, hlen, ns)`); returns the packet to append, or `None` when it took
/// it.
///
/// # Safety
///
/// `arg` is the pcb's `inp_upcall_arg`; `ip` (or `ip6`) points at the packet's IP header and
/// `uh` at its UDP header, both readable for the call.
pub type InpUpcallFn = unsafe fn(
    *mut c_void,
    &'static Mbuf,
    *const Ip,
    *const c_void,
    *const c_void,
    i32,
    Option<&Netstack>,
) -> Option<&'static Mbuf>;

/// The per-pcb hook of `in_pcbnotifyall` and the protocols' `ctlinput`s (`udp_notify`,
/// `in_pcbrtchange`): the control block and the errno of the event (`None` for 0).
pub type InpNotifyFn = fn(&'static Inpcb, Option<Errno>);

/// `union inpaddru`: an IPv4 or IPv6 address, the `const void *laddr` the port lookups take
/// and read as the address of the family their `INP_IPV6`/`INPLOOKUP_IPV6` says.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Inpaddru {
    /// The bytes of the union: `iau_addr6`, whose first four bytes are `iau_addr`.
    bytes: In6Addr,
}

impl Inpaddru {
    /// A union holding the IPv4 address `a` (`iau_addr`).
    pub const fn from_addr(a: InAddr) -> Self {
        Self {
            bytes: In6Addr::from_s6_addr32([a.s_addr, 0, 0, 0]),
        }
    }

    /// A union holding the IPv6 address `a6` (`iau_addr6`).
    pub const fn from_addr6(a6: In6Addr) -> Self {
        Self { bytes: a6 }
    }

    /// `iau_addr`: the union read as an IPv4 address.
    pub const fn iau_addr(&self) -> InAddr {
        InAddr {
            s_addr: self.bytes.s6_addr32(0),
        }
    }

    /// `iau_addr6`: the union read as an IPv6 address.
    pub const fn iau_addr6(&self) -> In6Addr {
        self.bytes
    }
}

/// `struct inpcb`: common structure pcb for internet protocol implementation. Here are
/// stored pointers to local and foreign host table entries, local and foreign socket numbers,
/// and pointers up (to a socket structure) and down (to a protocol-specific) control block.
pub struct Inpcb {
    /// \[I\] `inp_table`: inet queue/hash table (`None` for an iterator).
    pub inp_table: Option<&'static Inpcbtable>,
    /// \[t\] `inp_queue`: inet PCB queue.
    pub inp_queue: TailqEntry<Inpcb>,
    /// \[t\] `inp_hash`: local and foreign hash.
    pub inp_hash: ListEntry<Inpcb>,
    /// \[t\] `inp_lhash`: local port hash.
    pub inp_lhash: ListEntry<Inpcb>,
    /// \[t\] `inp_faddr`: foreign address (`inp_faddru.iau_addr`).
    pub inp_faddr: Cell<InAddr>,
    /// \[t\] `inp_faddr6`: foreign IPv6 address (`inp_faddru.iau_addr6`).
    pub inp_faddr6: Cell<In6Addr>,
    /// \[t\] `inp_laddr`: local address (`inp_laddru.iau_addr`).
    pub inp_laddr: Cell<InAddr>,
    /// \[t\] `inp_laddr6`: local IPv6 address (`inp_laddru.iau_addr6`).
    pub inp_laddr6: Cell<In6Addr>,
    /// \[t\] `inp_fport`: foreign port, network order.
    pub inp_fport: Cell<u16>,
    /// \[t\] `inp_lport`: local port, network order.
    pub inp_lport: Cell<u16>,
    /// \[I\] `inp_socket`: back pointer to socket (`None` for an iterator).
    pub inp_socket: Option<&'static Socket>,
    /// \[s\] `inp_ppcb`: pointer to per-protocol pcb.
    pub inp_ppcb: Cell<*mut c_void>,
    /// \[s\] `inp_route`: cached route.
    pub inp_route: Route,
    /// `inp_refcnt`: refcount PCB, delay memory free.
    pub inp_refcnt: Refcnt,
    /// `inp_flags`: generic IP/datagram flags.
    pub inp_flags: Cell<i32>,
    /// `inp_ip`: header prototype (`inp_hu.hu_ip`).
    pub inp_ip: Cell<Ip>,
    /// `inp_ipv6`: IPv6 header prototype (`inp_hu.hu_ipv6`).
    pub inp_ipv6: Cell<Ip6Hdr>,
    /// `inp_options`: IPv4 options.
    pub inp_options: Cell<Option<&'static Mbuf>>,
    /// `inp_outputopts6`: IPv6 options (`malloc(M_IP6OPT)`).
    pub inp_outputopts6: Cell<Option<NonNull<Ip6Pktopts>>>,
    /// `inp_hops`.
    pub inp_hops: Cell<i32>,
    /// \[N\] `inp_moptions`: IPv4 multicast options (`malloc(M_IPMOPTS)`).
    pub inp_moptions: Cell<Option<NonNull<IpMoptions>>>,
    /// \[N\] `inp_moptions6`: IPv6 multicast options (`malloc(M_IPMOPTS)`).
    pub inp_moptions6: Cell<Option<NonNull<Ip6Moptions>>>,
    /// \[N\] `inp_seclevel`: IPsec level of socket.
    pub inp_seclevel: Cell<IpsecLevel>,
    /// `inp_ip_minttl`: minimum TTL or drop.
    pub inp_ip_minttl: Cell<u8>,
    /// `inp_cksum6`.
    pub inp_cksum6: Cell<i32>,
    /// `inp_icmp6filt`: the `ICMP6_FILTER` of a raw ICMPv6 socket (`malloc(M_PCB)`).
    pub inp_icmp6filt: Cell<Option<NonNull<Icmp6Filter>>>,
    /// `inp_upcall`.
    pub inp_upcall: Cell<Option<InpUpcallFn>>,
    /// `inp_upcall_arg`.
    pub inp_upcall_arg: Cell<*mut c_void>,
    /// \[t\] `inp_rtableid`.
    pub inp_rtableid: Cell<u32>,
    /// `inp_pipex`: pipex indication.
    pub inp_pipex: Cell<i32>,
    /// \[s\] `inp_flowid`.
    pub inp_flowid: Cell<u16>,
    /// \[L\] `inp_pf_sk`: the pf(4) state key the socket's packets match (`pf_inp_mtx`).
    pub inp_pf_sk: Cell<Option<&'static crate::net::pfvar_priv::PfStateKey>>,
}

impl Inpcb {
    /// A zeroed control block of `table` and `so`, as `pool_get(PR_ZERO)` and the two
    /// assignments of `in_pcballoc` leave it; both `None` make an iterator.
    pub const fn new(table: Option<&'static Inpcbtable>, so: Option<&'static Socket>) -> Self {
        Self {
            inp_table: table,
            inp_queue: TailqEntry::new(),
            inp_hash: ListEntry::new(),
            inp_lhash: ListEntry::new(),
            inp_faddr: Cell::new(InAddr { s_addr: 0 }),
            inp_faddr6: Cell::new(IN6ADDR_ANY_INIT),
            inp_laddr: Cell::new(InAddr { s_addr: 0 }),
            inp_laddr6: Cell::new(IN6ADDR_ANY_INIT),
            inp_fport: Cell::new(0),
            inp_lport: Cell::new(0),
            inp_socket: so,
            inp_ppcb: Cell::new(ptr::null_mut()),
            inp_route: Route::new(),
            inp_refcnt: Refcnt::new(),
            inp_flags: Cell::new(0),
            inp_ip: Cell::new(Ip {
                ip_vhl: 0,
                ip_tos: 0,
                ip_len: 0,
                ip_id: 0,
                ip_off: 0,
                ip_ttl: 0,
                ip_p: 0,
                ip_sum: 0,
                ip_src: InAddr { s_addr: 0 },
                ip_dst: InAddr { s_addr: 0 },
            }),
            inp_ipv6: Cell::new(Ip6Hdr::zeroed()),
            inp_options: Cell::new(None),
            inp_outputopts6: Cell::new(None),
            inp_hops: Cell::new(0),
            inp_moptions: Cell::new(None),
            inp_moptions6: Cell::new(None),
            inp_seclevel: Cell::new(IpsecLevel {
                sl_auth: 0,
                sl_esp_trans: 0,
                sl_esp_network: 0,
                sl_ipcomp: 0,
            }),
            inp_ip_minttl: Cell::new(0),
            inp_cksum6: Cell::new(0),
            inp_icmp6filt: Cell::new(None),
            inp_upcall: Cell::new(None),
            inp_upcall_arg: Cell::new(ptr::null_mut()),
            inp_rtableid: Cell::new(0),
            inp_pipex: Cell::new(0),
            inp_flowid: Cell::new(0),
            inp_pf_sk: Cell::new(None),
        }
    }

    /// `inp->inp_socket` of a control block (not an iterator).
    pub fn socket(&self) -> &'static Socket {
        match self.inp_socket {
            Some(so) => so,
            None => panic(format_args!("inpcb {:p}: no socket", self)),
        }
    }

    /// `inp->inp_table` of a control block (not an iterator).
    pub fn table(&self) -> &'static Inpcbtable {
        match self.inp_table {
            Some(t) => t,
            None => panic(format_args!("inpcb {:p}: no table", self)),
        }
    }

    /// `inp->inp_flags & bits`.
    pub fn has_flags(&self, bits: i32) -> bool {
        self.inp_flags.get() & bits != 0
    }

    /// `inp->inp_flags |= bits`.
    pub fn set_flags(&self, bits: i32) {
        self.inp_flags.set(self.inp_flags.get() | bits);
    }

    /// `inp->inp_flags &= ~bits`.
    pub fn clear_flags(&self, bits: i32) {
        self.inp_flags.set(self.inp_flags.get() & !bits);
    }

    /// `inp_ip6_minhlim`: minimum Hop Limit or drop (the `inp_ip_minttl` member).
    pub fn inp_ip6_minhlim(&self) -> &Cell<u8> {
        &self.inp_ip_minttl
    }

    /// `inp_flowinfo` (`inp_ipv6.ip6_flow`): the flow information of the IPv6 prototype.
    pub fn inp_flowinfo(&self) -> u32 {
        self.inp_ipv6.get().ip6_flow
    }

    /// `inp_flowinfo = flow`.
    pub fn set_inp_flowinfo(&self, flow: u32) {
        let mut ip6 = self.inp_ipv6.get();
        ip6.ip6_flow = flow;
        self.inp_ipv6.set(ip6);
    }

    /// `inp->inp_moptions` as `ip_output` takes it.
    pub fn moptions(&self) -> Option<&IpMoptions> {
        // SAFETY: the options are `ip_setmoptions`'s allocation, owned by this control block
        // and freed only by `ip_setmoptions` or `in_pcbdetach` under the socket lock, which
        // the callers of `ip_output` hold for the call.
        self.inp_moptions.get().map(|imo| unsafe { imo.as_ref() })
    }
}

queue_adapter!(
    /// `TAILQ_HEAD(inpthead, inpcb)`: a table's queue, through `inp_queue`.
    pub InpQueue: Inpcb, inp_queue => TailqEntry<Inpcb>
);

queue_adapter!(
    /// `LIST_HEAD(inpcbhead, inpcb)` of the local and foreign hash, through `inp_hash`.
    pub InpHash: Inpcb, inp_hash => ListEntry<Inpcb>
);

queue_adapter!(
    /// `LIST_HEAD(inpcbhead, inpcb)` of the local port hash, through `inp_lhash`.
    pub InpLhash: Inpcb, inp_lhash => ListEntry<Inpcb>
);

/// `struct inpcb_iterator`: a place holder in a table's queue (an `Inpcb` without table).
pub struct InpcbIterator(Inpcb);

impl InpcbIterator {
    /// `{ .inp_table = NULL }`.
    pub const fn new() -> Self {
        Self(Inpcb::new(None, None))
    }
}

impl Default for InpcbIterator {
    fn default() -> Self {
        Self::new()
    }
}

/// `struct inpcbtable`.
pub struct Inpcbtable {
    /// `inpt_mtx`: protect queue and hash.
    pub inpt_mtx: Mutex,
    /// \[t\] `inpt_queue`: inet PCB queue.
    pub inpt_queue: TailqHead<InpQueue>,
    /// \[t\] `inpt_hashtbl`: local and foreign hash.
    pub inpt_hashtbl: Cell<&'static [ListHead<InpHash>]>,
    /// \[t\] `inpt_lhashtbl`: local port hash.
    pub inpt_lhashtbl: Cell<&'static [ListHead<InpLhash>]>,
    /// \[I\] `inpt_key`: secret for the hash.
    pub inpt_key: Cell<SiphashKey>,
    /// \[I\] `inpt_lkey`: secret for the local port hash.
    pub inpt_lkey: Cell<SiphashKey>,
    /// \[t\] `inpt_mask`: hash mask.
    pub inpt_mask: Cell<u64>,
    /// \[t\] `inpt_lmask`: local port hash mask.
    pub inpt_lmask: Cell<u64>,
    /// \[t\] `inpt_count`: queue count.
    pub inpt_count: Cell<i32>,
    /// \[t\] `inpt_size`: hash size.
    pub inpt_size: Cell<i32>,
}

impl Inpcbtable {
    /// An empty table; `in_pcbinit` sets it up.
    pub const fn new() -> Self {
        Self {
            inpt_mtx: Mutex::new(IPL_SOFTNET),
            inpt_queue: TailqHead::new(),
            inpt_hashtbl: Cell::new(&[]),
            inpt_lhashtbl: Cell::new(&[]),
            inpt_key: Cell::new(SiphashKey { k0: 0, k1: 0 }),
            inpt_lkey: Cell::new(SiphashKey { k0: 0, k1: 0 }),
            inpt_mask: Cell::new(0),
            inpt_lmask: Cell::new(0),
            inpt_count: Cell::new(0),
            inpt_size: Cell::new(0),
        }
    }

    /// `&table->inpt_hashtbl[hash & table->inpt_mask]`.
    fn hash_head(&self, hash: u64) -> &'static ListHead<InpHash> {
        &self.inpt_hashtbl.get()[(hash & self.inpt_mask.get()) as usize]
    }

    /// `&table->inpt_lhashtbl[lhash & table->inpt_lmask]`.
    fn lhash_head(&self, lhash: u64) -> &'static ListHead<InpLhash> {
        &self.inpt_lhashtbl.get()[(lhash & self.inpt_lmask.get()) as usize]
    }
}

impl Default for Inpcbtable {
    fn default() -> Self {
        Self::new()
    }
}

// SAFETY: the queue, the hash tables and the counts are read and changed only under
// `inpt_mtx` ([t]); the keys are written once by `in_pcbinit` before any lookup.
unsafe impl Sync for Inpcbtable {}

/// `struct baddynamicports`: bitmaps of ports, one bit per port, for TCP and UDP.
pub struct Baddynamicports {
    /// `tcp`.
    pub tcp: [AtomicU32; DP_MAPSIZE],
    /// `udp`.
    pub udp: [AtomicU32; DP_MAPSIZE],
}

impl Baddynamicports {
    /// Empty maps (the C's zeroed globals).
    pub const fn new() -> Self {
        Self {
            tcp: [const { AtomicU32::new(0) }; DP_MAPSIZE],
            udp: [const { AtomicU32::new(0) }; DP_MAPSIZE],
        }
    }
}

impl Default for Baddynamicports {
    fn default() -> Self {
        Self::new()
    }
}

/// `zeroin_addr`.
pub static ZEROIN_ADDR: InAddr = InAddr { s_addr: 0 };
/// `zeroin46_addr`: the zero address of either family.
pub static ZEROIN46_ADDR: Inpaddru = Inpaddru::from_addr6(IN6ADDR_ANY_INIT);

// These configure the range of local port addresses assigned to "unspecified" outgoing
// connections/packets/whatever.

/// \[a\] `ipport_firstauto`.
pub static IPPORT_FIRSTAUTO: AtomicI32 = AtomicI32::new(IPPORT_RESERVED);
/// \[a\] `ipport_lastauto`.
pub static IPPORT_LASTAUTO: AtomicI32 = AtomicI32::new(IPPORT_USERRESERVED);
/// \[a\] `ipport_hifirstauto`.
#[allow(non_upper_case_globals)] // the C name; `IPPORT_HIFIRSTAUTO` is `in.h`'s default
pub static ipport_hifirstauto: AtomicI32 = AtomicI32::new(IPPORT_HIFIRSTAUTO);
/// \[a\] `ipport_hilastauto`.
#[allow(non_upper_case_globals)] // the C name; `IPPORT_HILASTAUTO` is `in.h`'s default
pub static ipport_hilastauto: AtomicI32 = AtomicI32::new(IPPORT_HILASTAUTO);

/// `baddynamicports`: ports not to allocate dynamically.
pub static BADDYNAMICPORTS: Baddynamicports = Baddynamicports::new();
/// `rootonlyports`: ports only root can bind to.
pub static ROOTONLYPORTS: Baddynamicports = Baddynamicports::new();
/// `inpcb_pool`.
pub static INPCB_POOL: Pool = Pool::new();

#[cfg(feature = "diagnostic")]
/// `in_pcbnotifymiss` (`DIAGNOSTIC`): print the lookups that miss.
pub static IN_PCBNOTIFYMISS: AtomicI32 = AtomicI32::new(0);

/// `in_pcb_is_iterator(inp)`.
pub fn in_pcb_is_iterator(inp: &Inpcb) -> bool {
    inp.inp_table.is_none()
}

/// `sotoinpcb(so)`: the control block of an Internet socket, `None` once detached.
pub fn sotoinpcb(so: &Socket) -> Option<&'static Inpcb> {
    let family = so.dom_family();
    #[cfg(feature = "inet6")]
    let inet6 = family == i32::from(AF_INET6);
    #[cfg(not(feature = "inet6"))]
    let inet6 = false;
    if family != i32::from(AF_INET) && !inet6 {
        panic(format_args!(
            "sotoinpcb: socket {:p} of family {}",
            so,
            so.dom_family()
        ));
    }
    // SAFETY: an `inetdomain` or `inet6domain` socket's `so_pcb` is NULL or the `inpcb_pool` item
    // `in_pcballoc` set; `in_pcbdetach` clears it before the item can go back to the pool.
    unsafe { so.so_pcb.get().cast::<Inpcb>().cast_const().as_ref() }
}

/// `DP_SET(m, p)`.
pub fn dp_set(m: &[AtomicU32; DP_MAPSIZE], p: u16) {
    let p = usize::from(p);
    m[p / DP_MAPBITS].fetch_or(1 << (p % DP_MAPBITS), Ordering::Relaxed);
}

/// `DP_CLR(m, p)`.
pub fn dp_clr(m: &[AtomicU32; DP_MAPSIZE], p: u16) {
    let p = usize::from(p);
    m[p / DP_MAPBITS].fetch_and(!(1 << (p % DP_MAPBITS)), Ordering::Relaxed);
}

/// `DP_ISSET(m, p)`.
pub fn dp_isset(m: &[AtomicU32; DP_MAPSIZE], p: u16) -> bool {
    let p = usize::from(p);
    m[p / DP_MAPBITS].load(Ordering::Relaxed) & (1 << (p % DP_MAPBITS)) != 0
}

/// A control block linked in a table, with the lifetime of its pool item.
fn inp_static(inp: &Inpcb) -> &'static Inpcb {
    // SAFETY: a control block in a table's lists is an `inpcb_pool` item that stays
    // allocated until its last `in_pcbunref`; the callers hold the table mutex while they
    // look and a reference (`in_pcbref`) for as long as they keep it, as in C.
    unsafe { &*ptr::from_ref(inp) }
}

/// `curproc`, which the socket requests run as.
fn curproc_or_panic(func: &str) -> &'static Proc {
    match curproc() {
        Some(p) => p,
        None => panic(format_args!("{}: no curproc", func)),
    }
}

/// `in_init`: the `inpcb` pool. `in_pcb` is used for inet and inet6, `in6_pcb` only contains
/// special IPv6 cases, so the internet initializer is used for both domains.
pub fn in_init() {
    pool_init(
        &INPCB_POOL,
        size_of::<Inpcb>(),
        0,
        IPL_SOFTNET,
        0,
        "inpcb",
        None,
    );
}

/// `in_pcbhash`: the hash of the complete address quadruple in routing domain `rdomain`.
pub fn in_pcbhash(
    table: &Inpcbtable,
    rdomain: u32,
    faddr: &InAddr,
    fport: u16,
    laddr: &InAddr,
    lport: u16,
) -> u64 {
    let mut ctx = SiphashCtx::default();
    let nrdom = rdomain.to_be_bytes();

    SipHash24_Init(&mut ctx, &table.inpt_key.get());
    SipHash24_Update(&mut ctx, &nrdom);
    SipHash24_Update(&mut ctx, &faddr.s_addr.to_ne_bytes());
    SipHash24_Update(&mut ctx, &fport.to_ne_bytes());
    SipHash24_Update(&mut ctx, &laddr.s_addr.to_ne_bytes());
    SipHash24_Update(&mut ctx, &lport.to_ne_bytes());
    SipHash24_End(&mut ctx)
}

/// `in_pcblhash`: the hash of local port `lport` in routing domain `rdomain`.
pub fn in_pcblhash(table: &Inpcbtable, rdomain: u32, lport: u16) -> u64 {
    let mut ctx = SiphashCtx::default();
    let nrdom = rdomain.to_be_bytes();

    SipHash24_Init(&mut ctx, &table.inpt_lkey.get());
    SipHash24_Update(&mut ctx, &nrdom);
    SipHash24_Update(&mut ctx, &lport.to_ne_bytes());
    SipHash24_End(&mut ctx)
}

/// A fresh random hash key.
fn random_key() -> SiphashKey {
    let mut b = [0u8; 16];
    arc4random_buf(&mut b);
    let [k0, k1] = [&b[..8], &b[8..]].map(|w| {
        let mut word = [0u8; 8];
        word.copy_from_slice(w);
        u64::from_ne_bytes(word)
    });
    SiphashKey { k0, k1 }
}

/// `in_pcbinit`: sets up `table` with hash tables of `hashsize` buckets and fresh keys.
pub fn in_pcbinit(table: &Inpcbtable, hashsize: i32) {
    table.inpt_queue.init();
    let (Some(hashtbl), Some(lhashtbl)) = (
        hashinit::<InpHash>(hashsize, M_PCB, M_WAITOK),
        hashinit::<InpLhash>(hashsize, M_PCB, M_WAITOK),
    ) else {
        panic(format_args!("in_pcbinit: hashinit"));
    };
    table.inpt_hashtbl.set(hashtbl);
    table.inpt_mask.set(hashtbl.len() as u64 - 1);
    table.inpt_lhashtbl.set(lhashtbl);
    table.inpt_lmask.set(lhashtbl.len() as u64 - 1);
    table.inpt_count.set(0);
    table.inpt_size.set(hashsize);
    table.inpt_key.set(random_key());
    table.inpt_lkey.set(random_key());
}

/// `in_baddynamic`: whether `port` is invalid for dynamic allocation.
pub fn in_baddynamic(port: u16, proto: u16) -> bool {
    match i32::from(proto) {
        IPPROTO_TCP => dp_isset(&BADDYNAMICPORTS.tcp, port),
        IPPROTO_UDP => {
            // Cannot preset this as it is a sysctl
            i32::from(port) == UDPENCAP_PORT.load(Ordering::Relaxed)
                || dp_isset(&BADDYNAMICPORTS.udp, port)
        }
        _ => false,
    }
}

/// `in_rootonly`: whether only root may bind `port`.
pub fn in_rootonly(port: u16, proto: u16) -> bool {
    match i32::from(proto) {
        IPPROTO_TCP => i32::from(port) < IPPORT_RESERVED || dp_isset(&ROOTONLYPORTS.tcp, port),
        IPPROTO_UDP => i32::from(port) < IPPORT_RESERVED || dp_isset(&ROOTONLYPORTS.udp, port),
        _ => false,
    }
}

/// `in_pcballoc`: a new control block for `so` in `table`.
pub fn in_pcballoc(
    so: &'static Socket,
    table: &'static Inpcbtable,
    wait: i32,
) -> Result<(), Errno> {
    let Some(mem) = pool_get(
        &INPCB_POOL,
        (if wait == M_WAIT { PR_WAITOK } else { PR_NOWAIT }) | PR_ZERO,
    ) else {
        return Err(Errno::ENOBUFS);
    };
    let raw = mem.cast::<Inpcb>().as_ptr();
    // SAFETY: a fresh, suitably aligned `inpcb_pool` item, written once before anything else
    // sees it.
    unsafe { raw.write(Inpcb::new(Some(table), soref(Some(so)))) };
    // SAFETY: as above; the item stays allocated until the last `in_pcbunref`.
    let inp: &'static Inpcb = unsafe { &*raw };
    refcnt_init(&inp.inp_refcnt); // refcnt_init_trace(DT_REFCNT_IDX_INPCB): dt(4)
    inp.inp_seclevel.set(IpsecLevel {
        sl_auth: IPSEC_AUTH_LEVEL_DEFAULT as u8,
        sl_esp_trans: IPSEC_ESP_TRANS_LEVEL_DEFAULT as u8,
        sl_esp_network: IPSEC_ESP_NETWORK_LEVEL_DEFAULT as u8,
        sl_ipcomp: IPSEC_IPCOMP_LEVEL_DEFAULT as u8,
    });
    inp.inp_rtableid.set(
        curproc_or_panic("in_pcballoc")
            .process()
            .ps_rtableid
            .load(Ordering::Relaxed),
    );
    inp.inp_hops.set(-1);
    #[cfg(feature = "inet6")]
    {
        match so.dom_family() {
            f if f == i32::from(PF_INET6) => inp.inp_flags.set(INP_IPV6),
            f if f == i32::from(PF_INET) => {} // inp->inp_flags is initialized to 0
            f => unhandled_af(f),
        }
        inp.inp_cksum6.set(-1);
    }

    mtx_enter(&table.inpt_mtx);
    let count = table.inpt_count.get();
    table.inpt_count.set(count + 1);
    if count > inpcbhash_loadfactor(table.inpt_size.get()) {
        let _ = in_pcbresize(table, table.inpt_size.get() * 2);
    }
    // SAFETY: the table mutex is held; the control block is new and stays in place until
    // `in_pcbdetach` unlinks it.
    unsafe { table.inpt_queue.insert_head(inp) };
    in_pcbhash_insert(inp);
    mtx_leave(&table.inpt_mtx);

    so.so_pcb.set(raw.cast());

    Ok(())
}

/// The `sockaddr_in` of an address mbuf, read out of it.
fn nam_sin(nam: &Mbuf) -> Result<SockaddrIn, Errno> {
    let sin = in_nam2sin(nam)?;
    // SAFETY: `in_nam2sin` checked the mbuf holds a whole `sockaddr_in`; mbuf data need not be
    // aligned, so it is read unaligned.
    Ok(unsafe { sin.read_unaligned() })
}

/// The `sockaddr_in6` of an address mbuf, read out of it.
#[cfg(feature = "inet6")]
fn nam_sin6(nam: &Mbuf) -> Result<SockaddrIn6, Errno> {
    let sin6 = in6_nam2sin6(nam)?;
    // SAFETY: `in6_nam2sin6` checked the mbuf holds a whole `sockaddr_in6`; mbuf data need not
    // be aligned, so it is read unaligned.
    Ok(unsafe { sin6.read_unaligned() })
}

/// `satosin6_const(sa)` of a generic socket address image, read out of it.
#[cfg(feature = "inet6")]
fn su_sin6(su: &SockaddrUnion) -> SockaddrIn6 {
    // SAFETY: the union is `sizeof(struct sockaddr_in6)` initialised bytes; `SockaddrIn6` is
    // plain old data of that size, read unaligned.
    unsafe {
        su.as_bytes()
            .as_ptr()
            .cast::<SockaddrIn6>()
            .read_unaligned()
    }
}

/// `in_pcbbind_locked`: binds `inp` to the address in `nam` (or `laddr` and a port picked
/// here when `nam` is `None`), with the table mutex held.
pub fn in_pcbbind_locked(
    inp: &'static Inpcb,
    nam: Option<&Mbuf>,
    laddr: &Inpaddru,
    p: &Proc,
) -> Result<(), Errno> {
    let so = inp.socket();
    let mut lport: u16 = 0;
    let mut wild = 0;
    let mut laddr = *laddr;

    if inp.inp_lport.get() != 0 {
        return Err(Errno::EINVAL);
    }

    if !so.has_options(SO_REUSEADDR | SO_REUSEPORT)
        && (!so.pr_flags(PR_CONNREQUIRED) || !so.has_options(SO_ACCEPTCONN))
    {
        wild = INPLOOKUP_WILDCARD;
    }

    #[cfg(feature = "inet6")]
    let inet6 = inp.has_flags(INP_IPV6);
    #[cfg(not(feature = "inet6"))]
    let inet6 = false;
    if inet6 {
        #[cfg(feature = "inet6")]
        {
            if !in6_is_addr_unspecified(&inp.inp_laddr6.get()) {
                return Err(Errno::EINVAL);
            }
            wild |= INPLOOKUP_IPV6;

            if let Some(nam) = nam {
                let mut sin6 = nam_sin6(nam)?;
                in6_pcbaddrisavail_lock(inp, &mut sin6, wild, p, IN_PCBLOCK_HOLD)?;
                laddr = Inpaddru::from_addr6(sin6.sin6_addr);
                lport = sin6.sin6_port;
            }
        }
    } else {
        if inp.inp_laddr.get().s_addr != INADDR_ANY {
            return Err(Errno::EINVAL);
        }

        if let Some(nam) = nam {
            let mut sin = nam_sin(nam)?;
            in_pcbaddrisavail_lock(inp, &mut sin, wild, p, IN_PCBLOCK_HOLD)?;
            laddr = Inpaddru::from_addr(sin.sin_addr);
            lport = sin.sin_port;
        }
    }

    if lport == 0 {
        in_pcbpickport(&mut lport, &laddr, wild, inp, p)?;
    } else if in_rootonly(u16::from_be(lport), so.so_proto.pr_protocol as u16) && suser(p).is_err()
    {
        return Err(Errno::EACCES);
    }
    if nam.is_some() {
        if inet6 {
            inp.inp_laddr6.set(laddr.iau_addr6());
        } else {
            inp.inp_laddr.set(laddr.iau_addr());
        }
    }
    inp.inp_lport.set(lport);
    in_pcbrehash(inp);

    Ok(())
}

/// `in_pcbbind`: binds `inp` to `nam`, or to a port picked here.
pub fn in_pcbbind(inp: &'static Inpcb, nam: Option<&Mbuf>, p: &Proc) -> Result<(), Errno> {
    let table = inp.table();

    // keep lookup, modification, and rehash in sync
    mtx_enter(&table.inpt_mtx);
    let error = in_pcbbind_locked(inp, nam, &ZEROIN46_ADDR, p);
    mtx_leave(&table.inpt_mtx);

    error
}

/// `in_pcbaddrisavail_lock`: whether `inp` may bind to `sin` (an address of ours and a port
/// nobody else has, as `wild` and the reuse options allow); `lock` says whether the table
/// mutex is held (`IN_PCBLOCK_HOLD`) or taken here (`IN_PCBLOCK_GRAB`).
pub fn in_pcbaddrisavail_lock(
    inp: &Inpcb,
    sin: &mut SockaddrIn,
    wild: i32,
    _p: &Proc,
    lock: i32,
) -> Result<(), Errno> {
    let so = inp.socket();
    let table = inp.table();
    let lport = sin.sin_port;
    let mut reuseport = so.so_options.get() & SO_REUSEPORT;

    if in_multicast(sin.sin_addr.s_addr) {
        // Treat SO_REUSEADDR as SO_REUSEPORT for multicast; allow complete duplication of
        // binding if SO_REUSEPORT is set, or if SO_REUSEADDR is set and a multicast address is
        // bound on both new and duplicated sockets.
        if so.has_options(SO_REUSEADDR | SO_REUSEPORT) {
            reuseport = SO_REUSEADDR | SO_REUSEPORT;
        }
    } else if sin.sin_addr.s_addr != INADDR_ANY {
        // we must check that we are binding to an address we own except when:
        // - SO_BINDANY is set or
        // - we are binding a UDP socket to 255.255.255.255 or
        // - we are binding a UDP socket to one of our broadcast addresses
        if !so.has_options(SO_BINDANY)
            && !(so.so_type.get() == SOCK_DGRAM && sin.sin_addr.s_addr == INADDR_BROADCAST)
            && !(so.so_type.get() == SOCK_DGRAM
                && in_broadcast(sin.sin_addr, inp.inp_rtableid.get()))
        {
            sin.sin_port = 0;
            sin.sin_zero = [0; 8];
            // SAFETY: a local `sockaddr_in`, read for the call.
            let ia = unsafe { ifa_ifwithaddr(sintosa(sin), inp.inp_rtableid.get()) };
            sin.sin_port = lport;

            if ia.is_none() {
                return Err(Errno::EADDRNOTAVAIL);
            }
        }
    }
    if lport != 0 {
        let mut error = Ok(());

        if so.so_euid.get() != 0 && !in_multicast(sin.sin_addr.s_addr) {
            let t = in_pcblookup_local_lock(
                table,
                &Inpaddru::from_addr(sin.sin_addr),
                lport,
                INPLOOKUP_WILDCARD,
                inp.inp_rtableid.get(),
                lock,
            );
            if t.is_some_and(|t| so.so_euid.get() != t.socket().so_euid.get()) {
                error = Err(Errno::EADDRINUSE);
            }
            if lock == IN_PCBLOCK_GRAB {
                in_pcbunref(t);
            }
            error?;
        }
        let t = in_pcblookup_local_lock(
            table,
            &Inpaddru::from_addr(sin.sin_addr),
            lport,
            wild,
            inp.inp_rtableid.get(),
            lock,
        );
        if t.is_some_and(|t| reuseport & t.socket().so_options.get() == 0) {
            error = Err(Errno::EADDRINUSE);
        }
        if lock == IN_PCBLOCK_GRAB {
            in_pcbunref(t);
        }
        error?;
    }

    Ok(())
}

/// `in_pcbaddrisavail`: `in_pcbaddrisavail_lock` taking the table mutex.
pub fn in_pcbaddrisavail(
    inp: &Inpcb,
    sin: &mut SockaddrIn,
    wild: i32,
    p: &Proc,
) -> Result<(), Errno> {
    in_pcbaddrisavail_lock(inp, sin, wild, p, IN_PCBLOCK_GRAB)
}

/// `in_pcbpickport`: a free local port for `inp` from the range its flags and the sysctls
/// give, into `lport` (network order).
pub fn in_pcbpickport(
    lport: &mut u16,
    laddr: &Inpaddru,
    wild: i32,
    inp: &Inpcb,
    p: &Proc,
) -> Result<(), Errno> {
    let so = inp.socket();
    let table = inp.table();

    mutex_assert_locked(&table.inpt_mtx, "in_pcbpickport");

    let (first, last): (u16, u16) = if inp.has_flags(INP_HIGHPORT) {
        (
            ipport_hifirstauto.load(Ordering::Relaxed) as u16, // sysctl
            ipport_hilastauto.load(Ordering::Relaxed) as u16,
        )
    } else if inp.has_flags(INP_LOWPORT) {
        if suser(p).is_err() {
            return Err(Errno::EACCES);
        }
        (IPPORT_RESERVED as u16 - 1, 600) // 1023; not IPPORT_RESERVED/2
    } else {
        (
            IPPORT_FIRSTAUTO.load(Ordering::Relaxed) as u16, // sysctl
            IPPORT_LASTAUTO.load(Ordering::Relaxed) as u16,
        )
    };
    let (lower, higher) = if first < last {
        (first, last)
    } else {
        (last, first)
    };

    // Simple check to ensure all ports are not used up causing a deadlock here.
    let mut count = i32::from(higher - lower);
    let mut candidate = lower.wrapping_add(arc4random_uniform(count as u32) as u16);

    let localport = loop {
        let localport = loop {
            if count < 0 {
                // completely used?
                return Err(Errno::EADDRNOTAVAIL);
            }
            count -= 1;
            candidate = candidate.wrapping_add(1);
            if candidate < lower || candidate > higher {
                candidate = lower;
            }
            let localport = htons(candidate);
            if !in_baddynamic(candidate, so.so_proto.pr_protocol as u16) {
                break localport;
            }
        };
        let t = in_pcblookup_local_lock(
            table,
            laddr,
            localport,
            wild,
            inp.inp_rtableid.get(),
            IN_PCBLOCK_HOLD,
        );
        if t.is_none() {
            break localport;
        }
    };
    *lport = localport;

    Ok(())
}

/// `in_pcbconnect`: connects `inp` to the address in `nam`; both address and port must be
/// specified. If the socket has no local address yet, one is picked.
pub fn in_pcbconnect(inp: &'static Inpcb, nam: &Mbuf) -> Result<(), Errno> {
    let table = inp.table();
    let mut ina = InAddr::default();

    #[cfg(feature = "inet6")]
    if inp.has_flags(INP_IPV6) {
        return in6_pcbconnect(inp, nam);
    }

    let sin = nam_sin(nam)?;
    if sin.sin_port == 0 {
        return Err(Errno::EADDRNOTAVAIL);
    }
    in_pcbselsrc(&mut ina, &sin, inp)?;

    // keep lookup, modification, and rehash in sync
    mtx_enter(&table.inpt_mtx);

    let t = in_pcblookup_lock(
        table,
        sin.sin_addr,
        sin.sin_port,
        ina,
        inp.inp_lport.get(),
        inp.inp_rtableid.get(),
        IN_PCBLOCK_HOLD,
    );
    if t.is_some() {
        mtx_leave(&table.inpt_mtx);
        return Err(Errno::EADDRINUSE);
    }

    kassert!(inp.inp_laddr.get().s_addr == INADDR_ANY || inp.inp_lport.get() != 0);

    if inp.inp_laddr.get().s_addr == INADDR_ANY {
        if inp.inp_lport.get() == 0 {
            if let Err(e) = in_pcbbind_locked(
                inp,
                None,
                &Inpaddru::from_addr(ina),
                curproc_or_panic("in_pcbconnect"),
            ) {
                mtx_leave(&table.inpt_mtx);
                return Err(e);
            }
            let t = in_pcblookup_lock(
                table,
                sin.sin_addr,
                sin.sin_port,
                ina,
                inp.inp_lport.get(),
                inp.inp_rtableid.get(),
                IN_PCBLOCK_HOLD,
            );
            if t.is_some() {
                inp.inp_lport.set(0);
                mtx_leave(&table.inpt_mtx);
                return Err(Errno::EADDRINUSE);
            }
        }
        inp.inp_laddr.set(ina);
    }
    inp.inp_faddr.set(sin.sin_addr);
    inp.inp_fport.set(sin.sin_port);
    in_pcbrehash(inp);

    mtx_leave(&table.inpt_mtx);

    inp.inp_flowid.set(stoeplitz_ip4port(
        inp.inp_faddr.get().s_addr,
        inp.inp_laddr.get().s_addr,
        inp.inp_fport.get(),
        inp.inp_lport.get(),
    ));
    Ok(())
}

/// `in_pcbdisconnect`: forgets the flow; a socket without a file reference goes too.
pub fn in_pcbdisconnect(inp: &'static Inpcb) {
    pf_remove_divert_state(inp);
    pf_inp_unlink(inp);
    inp.inp_flowid.set(0);
    if inp.socket().has_state(SS_NOFDREF) {
        in_pcbdetach(inp);
    }
}

/// `in_pcbdetach`: unlinks `inp` from its socket and its table and drops the table's
/// reference.
pub fn in_pcbdetach(inp: &'static Inpcb) {
    let so = inp.socket();
    let table = inp.table();

    soassertlocked(so);

    so.so_pcb.set(ptr::null_mut());
    sofree(so, true);
    if let Some(rt) = inp.inp_route.ro_rt.take() {
        rtfree(Some(rt));
    }
    #[cfg(feature = "inet6")]
    let inet6 = inp.has_flags(INP_IPV6);
    #[cfg(not(feature = "inet6"))]
    let inet6 = false;
    if inet6 {
        #[cfg(feature = "inet6")]
        {
            // SAFETY: the options are this control block's own allocations
            // (`ip6_setpktopts`, `ip6_setmoptions`), taken out of it here and never used again.
            unsafe {
                ip6_freepcbopts(inp.inp_outputopts6.take());
                ip6_freemoptions(inp.inp_moptions6.take());
            }
        }
    } else {
        m_freem(inp.inp_options.take());
        // SAFETY: the options are this control block's own allocation (`ip_setmoptions`),
        // taken out of it here and never used again.
        unsafe { ip_freemoptions(inp.inp_moptions.take()) };
    }
    pf_remove_divert_state(inp);
    pf_inp_unlink(inp);
    mtx_enter(&table.inpt_mtx);
    // SAFETY: the table mutex is held and the control block is on the three lists since
    // `in_pcballoc`.
    unsafe {
        ListHead::<InpLhash>::remove(inp);
        ListHead::<InpHash>::remove(inp);
        table.inpt_queue.remove(inp);
    }
    table.inpt_count.set(table.inpt_count.get() - 1);
    mtx_leave(&table.inpt_mtx);

    in_pcbunref(Some(inp));
}

/// `in_pcbsolock`: locks the socket of `inp`; `None` when it is detached.
pub fn in_pcbsolock(inp: &Inpcb) -> Option<&'static Socket> {
    let so = inp.inp_socket;

    net_assert_locked("in_pcbsolock");

    let so = so?;
    rw_enter_write(&so.so_lock);
    if so.so_pcb.get().is_null() {
        rw_exit_write(&so.so_lock);
        return None;
    }
    kassert!(sotoinpcb(so).is_some_and(|i| ptr::eq(i, inp)));
    Some(so)
}

/// `in_pcbsounlock`: unlocks what `in_pcbsolock` locked.
pub fn in_pcbsounlock(inp: Option<&Inpcb>, so: Option<&Socket>) {
    let Some(so) = so else {
        return;
    };
    if let Some(inp) = inp
        && !so.so_pcb.get().is_null()
    {
        kassert!(
            inp.inp_socket.is_some_and(|s| ptr::eq(s, so))
                && ptr::eq(so.so_pcb.get().cast_const(), ptr::from_ref(inp).cast())
        );
    }
    rw_exit_write(&so.so_lock);
}

/// `in_pcbref`: takes a reference to `inp`.
pub fn in_pcbref(inp: Option<&'static Inpcb>) -> Option<&'static Inpcb> {
    let inp = inp?;
    refcnt_take(&inp.inp_refcnt);
    Some(inp)
}

/// `in_pcbunref`: drops a reference to `inp`; the last one releases the socket and the
/// control block.
pub fn in_pcbunref(inp: Option<&Inpcb>) {
    let Some(inp) = inp else {
        return;
    };
    if !refcnt_rele(&inp.inp_refcnt) {
        return;
    }
    sorele(inp.socket());
    // KASSERTs that the links are cleared: see the module's deviations.
    pool_put(&INPCB_POOL, NonNull::from(inp).cast());
}

/// `in_pcb_iterator`: the control block after `inp` (the first one for `None`) in `table`,
/// referenced, with `iter` marking the place; drops the reference to `inp`. `None` ends the
/// walk.
///
/// # Safety
///
/// The table mutex is held. `iter` stays valid and in place, and is used for this walk only,
/// until this function returns `None` or `in_pcb_iterator_abort` is called (it sits in the
/// table's queue in between).
pub unsafe fn in_pcb_iterator(
    table: &Inpcbtable,
    inp: Option<&Inpcb>,
    iter: &InpcbIterator,
) -> Option<&'static Inpcb> {
    mutex_assert_locked(&table.inpt_mtx, "in_pcb_iterator");

    let mut tmp = if inp.is_some() {
        TailqHead::<InpQueue>::next(&iter.0)
    } else {
        table.inpt_queue.first()
    };

    while let Some(t) = tmp
        && t.inp_table.is_none()
    {
        tmp = TailqHead::<InpQueue>::next(t);
    }
    let tmp = tmp.map(inp_static);

    if inp.is_some() {
        // SAFETY: the mutex is held and `iter` is in the queue since the previous call.
        unsafe { table.inpt_queue.remove(&iter.0) };
        in_pcbunref(inp);
    }
    if let Some(tmp) = tmp {
        // SAFETY: the mutex is held; `tmp` is in the queue and `iter` stays in place until it
        // is removed (the caller's contract).
        unsafe { table.inpt_queue.insert_after(tmp, &iter.0) };
        in_pcbref(Some(tmp));
    }

    tmp
}

/// `in_pcb_iterator_abort`: ends a walk before `in_pcb_iterator` returned `None`.
///
/// # Safety
///
/// The table mutex is held; `iter` is the iterator of the walk that returned `inp`.
pub unsafe fn in_pcb_iterator_abort(table: &Inpcbtable, inp: Option<&Inpcb>, iter: &InpcbIterator) {
    mutex_assert_locked(&table.inpt_mtx, "in_pcb_iterator_abort");

    if inp.is_some() {
        // SAFETY: the caller's contract: `iter` is in the queue after `inp`.
        unsafe { table.inpt_queue.remove(&iter.0) };
        in_pcbunref(inp);
    }
}

/// Writes `sin` as the address in `nam`.
fn set_nam(nam: &Mbuf, sin: SockaddrIn) {
    nam.m_len().set(size_of::<SockaddrIn>() as u32);
    // SAFETY: `nam` is an `MT_SONAME` mbuf of `MLEN` bytes, more than a `sockaddr_in`;
    // written unaligned (mbuf data need not be aligned).
    unsafe { mtod::<SockaddrIn>(nam).write_unaligned(sin) };
}

/// `in_setsockaddr`: the local address of `inp` into `nam`.
pub fn in_setsockaddr(inp: &Inpcb, nam: &Mbuf) {
    #[cfg(feature = "inet6")]
    if inp.has_flags(INP_IPV6) {
        in6_setsockaddr(inp, nam);
        return;
    }
    set_nam(
        nam,
        SockaddrIn {
            sin_family: AF_INET,
            sin_len: size_of::<SockaddrIn>() as u8,
            sin_port: inp.inp_lport.get(),
            sin_addr: inp.inp_laddr.get(),
            ..SockaddrIn::default()
        },
    );
}

/// `in_setpeeraddr`: the foreign address of `inp` into `nam`.
pub fn in_setpeeraddr(inp: &Inpcb, nam: &Mbuf) {
    #[cfg(feature = "inet6")]
    if inp.has_flags(INP_IPV6) {
        in6_setpeeraddr(inp, nam);
        return;
    }
    set_nam(
        nam,
        SockaddrIn {
            sin_family: AF_INET,
            sin_len: size_of::<SockaddrIn>() as u8,
            sin_port: inp.inp_fport.get(),
            sin_addr: inp.inp_faddr.get(),
            ..SockaddrIn::default()
        },
    );
}

/// `sotoinpcb(so)` of a socket the C knows to be attached.
fn inpcb_of(so: &Socket) -> &'static Inpcb {
    match sotoinpcb(so) {
        Some(inp) => inp,
        None => panic(format_args!("socket {:p}: no inpcb", so)),
    }
}

/// `in_sockaddr`: the `pru_sockaddr` of the Internet protocols.
pub fn in_sockaddr(so: &'static Socket, nam: &'static Mbuf) -> Result<(), Errno> {
    in_setsockaddr(inpcb_of(so), nam);

    Ok(())
}

/// `in_peeraddr`: the `pru_peeraddr` of the Internet protocols.
pub fn in_peeraddr(so: &'static Socket, nam: &'static Mbuf) -> Result<(), Errno> {
    in_setpeeraddr(inpcb_of(so), nam);

    Ok(())
}

/// `in_flowid`: the `pru_flowid` of the Internet protocols.
pub fn in_flowid(so: &'static Socket) -> i32 {
    match sotoinpcb(so) {
        Some(inp) => i32::from(inp.inp_flowid.get()),
        None => 0,
    }
}

/// `in_pcbnotifyall`: passes some notification to all connections of a protocol associated
/// with address `dst`. The "usual action" will be taken, depending on the ctlinput cmd. The
/// caller must filter any cmds that are uninteresting (e.g., no error in the map). Calls the
/// protocol specific routine (if any) to report any errors for each matching socket.
pub fn in_pcbnotifyall(
    table: &Inpcbtable,
    dst: &SockaddrIn,
    rtable: u32,
    errno: Option<Errno>,
    notify: Option<InpNotifyFn>,
) {
    let iter = InpcbIterator::new();
    let mut inp: Option<&'static Inpcb> = None;

    if dst.sin_addr.s_addr == INADDR_ANY {
        return;
    }
    let Some(notify) = notify else {
        return;
    };

    let rdomain = rtable_l2(rtable);
    mtx_enter(&table.inpt_mtx);
    // SAFETY: the mutex is held around every call; `iter` lives on this frame until the walk
    // ends with `None`.
    while let Some(i) = unsafe { in_pcb_iterator(table, inp, &iter) } {
        inp = Some(i);
        kassert!(!i.has_flags(INP_IPV6));

        if i.inp_faddr.get().s_addr != dst.sin_addr.s_addr
            || rtable_l2(i.inp_rtableid.get()) != rdomain
        {
            continue;
        }
        mtx_leave(&table.inpt_mtx);
        let so = in_pcbsolock(i);
        if so.is_some() {
            notify(i, errno);
        }
        in_pcbsounlock(Some(i), so);
        mtx_enter(&table.inpt_mtx);
    }
    mtx_leave(&table.inpt_mtx);
}

/// `in_losing`: checks for alternatives when higher level complains about service problems.
/// For now, invalidate cached routing information. If the route was created dynamically (by
/// a redirect), time to try a default gateway again.
pub fn in_losing(inp: &Inpcb) {
    let Some(rt) = inp.inp_route.ro_rt.get() else {
        return;
    };
    inp.inp_route.ro_rt.set(None);

    if rt.rt_flags.get() & RTF_DYNAMIC != 0 {
        let ifp = if_get(rt.rt_ifidx.get());
        // If the interface is gone, all its attached route entries have been removed from
        // the table, so we're dealing with a stale cache and have nothing to do.
        if let Some(ifp) = ifp {
            let _ = rtdeletemsg(rt, ifp, inp.inp_rtableid.get());
        }
        if_put(ifp);
    }
    // A new route can be allocated the next time output is attempted. rtfree() needs to be
    // called in anycase because the inp is still holding a reference to rt.
    rtfree(Some(rt));
}

/// `in_pcbrtchange`: after a routing change, flushes the old route; a (hopefully) better one
/// is allocated the next time output is attempted.
pub fn in_pcbrtchange(inp: &'static Inpcb, _errno: Option<Errno>) {
    soassertlocked(inp.socket());

    if let Some(rt) = inp.inp_route.ro_rt.take() {
        rtfree(Some(rt));
    }
}

/// `in_pcblookup_local_lock`: the control block of `table` bound to local port `lport_arg`
/// and address `laddr` with the fewest wildcards (`INPLOOKUP_WILDCARD` allows some); with
/// `IN_PCBLOCK_GRAB` the table mutex is taken here and the result referenced.
pub fn in_pcblookup_local_lock(
    table: &Inpcbtable,
    laddrp: &Inpaddru,
    lport_arg: u16,
    flags: i32,
    rtable: u32,
    lock: i32,
) -> Option<&'static Inpcb> {
    let mut matched: Option<&'static Inpcb> = None;
    let mut matchwild = 3;
    let lport = lport_arg;
    let laddr = laddrp.iau_addr();
    #[cfg(feature = "inet6")]
    let laddr6 = laddrp.iau_addr6();

    let rdomain = rtable_l2(rtable);
    let lhash = in_pcblhash(table, rdomain, lport);

    if lock == IN_PCBLOCK_GRAB {
        mtx_enter(&table.inpt_mtx);
    } else {
        kassert!(lock == IN_PCBLOCK_HOLD);
        mutex_assert_locked(&table.inpt_mtx, "in_pcblookup_local_lock");
    }
    for inp in table.lhash_head(lhash).iter() {
        if rtable_l2(inp.inp_rtableid.get()) != rdomain {
            continue;
        }
        if inp.inp_lport.get() != lport {
            continue;
        }
        let mut wildcard = 0;
        #[cfg(feature = "inet6")]
        let inet6 = flags & INPLOOKUP_IPV6 != 0;
        #[cfg(not(feature = "inet6"))]
        let inet6 = false;
        if inet6 {
            #[cfg(feature = "inet6")]
            {
                kassert!(inp.has_flags(INP_IPV6));

                if !in6_is_addr_unspecified(&inp.inp_faddr6.get()) {
                    wildcard += 1;
                }

                if !in6_are_addr_equal(&inp.inp_laddr6.get(), &laddr6) {
                    if in6_is_addr_unspecified(&inp.inp_laddr6.get())
                        || in6_is_addr_unspecified(&laddr6)
                    {
                        wildcard += 1;
                    } else {
                        continue;
                    }
                }
            }
        } else {
            kassert!(!inp.has_flags(INP_IPV6));

            if inp.inp_faddr.get().s_addr != INADDR_ANY {
                wildcard += 1;
            }

            if inp.inp_laddr.get().s_addr != laddr.s_addr {
                if inp.inp_laddr.get().s_addr == INADDR_ANY || laddr.s_addr == INADDR_ANY {
                    wildcard += 1;
                } else {
                    continue;
                }
            }
        }
        if (wildcard == 0 || flags & INPLOOKUP_WILDCARD != 0) && wildcard < matchwild {
            matched = Some(inp_static(inp));
            matchwild = wildcard;
            if matchwild == 0 {
                break;
            }
        }
    }
    if lock == IN_PCBLOCK_GRAB {
        in_pcbref(matched);
        mtx_leave(&table.inpt_mtx);
    }

    matched
}

/// `in_pcbrtentry`: the route to the foreign address of `inp`, from its cache.
pub fn in_pcbrtentry(inp: &Inpcb) -> Option<&'static Rtentry> {
    soassertlocked(inp.socket());

    #[cfg(feature = "inet6")]
    if inp.has_flags(INP_IPV6) {
        return in6_pcbrtentry(inp);
    }

    if inp.inp_faddr.get().s_addr == INADDR_ANY {
        return None;
    }
    let faddr = inp.inp_faddr.get();
    let laddr = inp.inp_laddr.get();
    route_mpath(&inp.inp_route, &faddr, Some(&laddr), inp.inp_rtableid.get())
}

/// `in_pcbselsrc`: the IPv4 address most appropriate as the source for destination
/// `dstsock`, into `insrc`. If necessary, the routing table is looked up and the entry cached
/// in `inp`.
pub fn in_pcbselsrc(insrc: &mut InAddr, dstsock: &SockaddrIn, inp: &Inpcb) -> Result<(), Errno> {
    let dst = &dstsock.sin_addr;
    let laddr = inp.inp_laddr.get();
    let rtableid = inp.inp_rtableid.get();
    let mut ia = None;

    // If the socket(if any) is already bound, use that bound address unless it is
    // INADDR_ANY or INADDR_BROADCAST.
    if laddr.s_addr != INADDR_ANY && laddr.s_addr != INADDR_BROADCAST {
        *insrc = laddr;
        return Ok(());
    }

    // If the destination address is multicast or limited broadcast (255.255.255.255) and an
    // outgoing interface has been set as a multicast option, use the address of that
    // interface as our source address.
    if (in_multicast(dst.s_addr) || dst.s_addr == INADDR_BROADCAST)
        && let Some(mopts) = inp.moptions()
    {
        let ifp = if_get(u32::from(mopts.imo_ifidx));
        if let Some(ifp) = ifp {
            if ifp.if_rdomain.get() == rtable_l2(rtableid) {
                ia = in_ifp2ia(ifp);
            }
            let Some(ia) = ia else {
                if_put(Some(ifp));
                return Err(Errno::EADDRNOTAVAIL);
            };

            *insrc = ia.ia_addr.get().sin_addr;
            if_put(Some(ifp));
            return Ok(());
        }
    }

    // If route is known or can be allocated now, our src addr is taken from the i/f, else
    // punt.
    let rt = route_mpath(&inp.inp_route, dst, None, rtableid);

    // If we found a route, use the address corresponding to the outgoing interface.
    if let Some(rt) = rt {
        ia = Some(ifatoia(rt.ifa()));
    }

    // Use preferred source address if :
    // - destination is not onlink
    // - preferred source address is set
    // - output interface is UP
    if let Some(rt) = rt
        && rt.rt_flags.get() & RTF_GATEWAY != 0
    {
        let ip4_source = rtable_getsource(rtableid, AF_INET);
        if !ip4_source.is_null() {
            // SAFETY: a source address set on the table is a live `sockaddr_in`, read for
            // the call.
            let ifa = unsafe { ifa_ifwithaddr(ip4_source, rtableid) };
            if let Some(ifa) = ifa
                && ifa
                    .ifa_ifp
                    .get()
                    .is_some_and(|ifp| ifp.if_flags.get() & IFF_UP != 0)
            {
                // SAFETY: as above.
                *insrc = unsafe { (*satosin_const(ip4_source)).sin_addr };
                return Ok(());
            }
        }
    }

    let Some(ia) = ia else {
        return Err(Errno::EADDRNOTAVAIL);
    };

    *insrc = ia.ia_addr.get().sin_addr;
    Ok(())
}

/// `in_pcbrehash`: moves `inp` to the hash buckets of its current addresses.
pub fn in_pcbrehash(inp: &'static Inpcb) {
    // SAFETY: the callers hold the table mutex; `inp` is on both hash lists.
    unsafe {
        ListHead::<InpLhash>::remove(inp);
        ListHead::<InpHash>::remove(inp);
    }
    in_pcbhash_insert(inp);
}

/// `in_pcbhash_insert`: links `inp` into the hash buckets of its addresses.
pub fn in_pcbhash_insert(inp: &'static Inpcb) {
    let table = inp.table();

    mutex_assert_locked(&table.inpt_mtx, "in_pcbhash_insert");

    let lhash = in_pcblhash(table, inp.inp_rtableid.get(), inp.inp_lport.get());
    // SAFETY: the table mutex is held; the control block is in no local port list and stays
    // in place until `in_pcbdetach` unlinks it.
    unsafe { table.lhash_head(lhash).insert_head(inp) };
    #[cfg(feature = "inet6")]
    let hash = if inp.has_flags(INP_IPV6) {
        in6_pcbhash(
            table,
            rtable_l2(inp.inp_rtableid.get()),
            &inp.inp_faddr6.get(),
            inp.inp_fport.get(),
            &inp.inp_laddr6.get(),
            inp.inp_lport.get(),
        )
    } else {
        in_pcbhash(
            table,
            rtable_l2(inp.inp_rtableid.get()),
            &inp.inp_faddr.get(),
            inp.inp_fport.get(),
            &inp.inp_laddr.get(),
            inp.inp_lport.get(),
        )
    };
    #[cfg(not(feature = "inet6"))]
    let hash = in_pcbhash(
        table,
        rtable_l2(inp.inp_rtableid.get()),
        &inp.inp_faddr.get(),
        inp.inp_fport.get(),
        &inp.inp_laddr.get(),
        inp.inp_lport.get(),
    );
    // SAFETY: as above, for the address hash.
    unsafe { table.hash_head(hash).insert_head(inp) };
}

/// `in_pcbhash_lookup`: the control block with exactly these addresses, moved to the head of
/// its chain.
pub fn in_pcbhash_lookup(
    table: &Inpcbtable,
    hash: u64,
    rdomain: u32,
    faddr: &InAddr,
    fport: u16,
    laddr: &InAddr,
    lport: u16,
) -> Option<&'static Inpcb> {
    mutex_assert_locked(&table.inpt_mtx, "in_pcbhash_lookup");

    let head = table.hash_head(hash);
    let inp = head
        .iter()
        .find(|inp| {
            kassert!(!inp.has_flags(INP_IPV6));

            inp.inp_fport.get() == fport
                && inp.inp_lport.get() == lport
                && inp.inp_faddr.get().s_addr == faddr.s_addr
                && inp.inp_laddr.get().s_addr == laddr.s_addr
                && rtable_l2(inp.inp_rtableid.get()) == rdomain
        })
        .map(inp_static)?;
    // Move this PCB to the head of hash chain so that repeated accesses are quicker. This is
    // analogous to the historic single-entry PCB cache.
    if !head.first().is_some_and(|f| ptr::eq(f, inp)) {
        // SAFETY: the table mutex is held; `inp` is on this chain and stays on it.
        unsafe {
            ListHead::<InpHash>::remove(inp);
            head.insert_head(inp);
        }
    }
    Some(inp)
}

/// `in_pcbresize`: rehashes `table` into tables of `hashsize` buckets.
pub fn in_pcbresize(table: &Inpcbtable, hashsize: i32) -> Result<(), Errno> {
    mutex_assert_locked(&table.inpt_mtx, "in_pcbresize");

    let ohashtbl = table.inpt_hashtbl.get();
    let olhashtbl = table.inpt_lhashtbl.get();
    let osize = table.inpt_size.get();

    let Some(nhashtbl) = hashinit::<InpHash>(hashsize, M_PCB, M_NOWAIT) else {
        return Err(Errno::ENOBUFS);
    };
    let Some(nlhashtbl) = hashinit::<InpLhash>(hashsize, M_PCB, M_NOWAIT) else {
        // SAFETY: just made by hashinit with these arguments, empty and never used.
        unsafe { hashfree(nhashtbl, hashsize, M_PCB) };
        return Err(Errno::ENOBUFS);
    };
    table.inpt_hashtbl.set(nhashtbl);
    table.inpt_lhashtbl.set(nlhashtbl);
    table.inpt_mask.set(nhashtbl.len() as u64 - 1);
    table.inpt_lmask.set(nlhashtbl.len() as u64 - 1);
    table.inpt_size.set(hashsize);

    for inp in table.inpt_queue.iter() {
        if in_pcb_is_iterator(inp) {
            continue;
        }
        let inp = inp_static(inp);
        // SAFETY: the table mutex is held; `inp` is on the old chains.
        unsafe {
            ListHead::<InpLhash>::remove(inp);
            ListHead::<InpHash>::remove(inp);
        }
        in_pcbhash_insert(inp);
    }
    // SAFETY: every control block moved to the new tables, so the old ones are empty; they
    // came from hashinit with `osize`, and the table no longer points at them.
    unsafe {
        hashfree(ohashtbl, osize, M_PCB);
        hashfree(olhashtbl, osize, M_PCB);
    }

    Ok(())
}

/// `in_pcblookup_lock`: the connected control block for `faddr.fport <-> laddr.lport`; no
/// wildcard matching is done, so listening sockets are not found (`in_pcblookup_listen`
/// finds those). With `IN_PCBLOCK_GRAB` the table mutex is taken here and the result
/// referenced.
pub fn in_pcblookup_lock(
    table: &Inpcbtable,
    faddr: InAddr,
    fport: u16,
    laddr: InAddr,
    lport: u16,
    rtable: u32,
    lock: i32,
) -> Option<&'static Inpcb> {
    let rdomain = rtable_l2(rtable);
    let hash = in_pcbhash(table, rdomain, &faddr, fport, &laddr, lport);

    if lock == IN_PCBLOCK_GRAB {
        mtx_enter(&table.inpt_mtx);
    } else {
        kassert!(lock == IN_PCBLOCK_HOLD);
        mutex_assert_locked(&table.inpt_mtx, "in_pcblookup_lock");
    }
    let inp = in_pcbhash_lookup(table, hash, rdomain, &faddr, fport, &laddr, lport);
    if lock == IN_PCBLOCK_GRAB {
        in_pcbref(inp);
        mtx_leave(&table.inpt_mtx);
    }

    #[cfg(feature = "diagnostic")]
    if inp.is_none() && IN_PCBNOTIFYMISS.load(Ordering::Relaxed) != 0 {
        crate::kprintf!(
            "in_pcblookup_lock: faddr={:08x} fport={} laddr={:08x} lport={} rdom={}\n",
            u32::from_be(faddr.s_addr),
            u16::from_be(fport),
            u32::from_be(laddr.s_addr),
            u16::from_be(lport),
            rdomain
        );
    }
    inp
}

/// `in_pcblookup`: `in_pcblookup_lock` taking the table mutex; the result is referenced.
pub fn in_pcblookup(
    table: &Inpcbtable,
    faddr: InAddr,
    fport: u16,
    laddr: InAddr,
    lport: u16,
    rtable: u32,
) -> Option<&'static Inpcb> {
    in_pcblookup_lock(table, faddr, fport, laddr, lport, rtable, IN_PCBLOCK_GRAB)
}

/// `in_pcblookup_listen`: the listening control block for `laddr.lport`: unspecified foreign
/// address and port, bound to `laddr` or to the wildcard address. The result is referenced.
pub fn in_pcblookup_listen(
    table: &Inpcbtable,
    laddr: InAddr,
    lport_arg: u16,
    m: Option<&Mbuf>,
    rtable: u32,
) -> Option<&'static Inpcb> {
    let mut key1 = &laddr;
    let mut key2 = &ZEROIN_ADDR;
    let mut lport = lport_arg;

    let divert_addr;
    if let Some(m) = m
        && m.m_pkthdr().pf.flags.get() & PF_TAG_DIVERTED != 0
    {
        let divert = pf_find_divert(m);
        kassert!(divert.is_some());
        let divert = divert?;
        match divert.type_ {
            PF_DIVERT_TO => {
                divert_addr = divert.addr.v4();
                key1 = &divert_addr;
                key2 = &divert_addr;
                lport = divert.port;
            }
            PF_DIVERT_REPLY => return None,
            t => panic(format_args!(
                "in_pcblookup_listen: unknown divert type {t}, mbuf {m:p}"
            )),
        }
    } else if let Some(m) = m
        && m.m_pkthdr().pf.flags.get() & PF_TAG_TRANSLATE_LOCALHOST != 0
    {
        // Redirected connections should not be treated the same as connections directed to
        // 127.0.0.0/8 since localhost can only be accessed from the host itself. For example
        // portmap(8) grants more permissions for connections to the socket bound to
        // 127.0.0.1 than to the * socket.
        key1 = &ZEROIN_ADDR;
        key2 = &laddr;
    }

    let rdomain = rtable_l2(rtable);
    let hash = in_pcbhash(table, rdomain, &ZEROIN_ADDR, 0, key1, lport);

    mtx_enter(&table.inpt_mtx);
    let mut inp = in_pcbhash_lookup(table, hash, rdomain, &ZEROIN_ADDR, 0, key1, lport);
    if inp.is_none() && key1.s_addr != key2.s_addr {
        let hash = in_pcbhash(table, rdomain, &ZEROIN_ADDR, 0, key2, lport);
        inp = in_pcbhash_lookup(table, hash, rdomain, &ZEROIN_ADDR, 0, key2, lport);
    }
    in_pcbref(inp);
    mtx_leave(&table.inpt_mtx);

    #[cfg(feature = "diagnostic")]
    if inp.is_none() && IN_PCBNOTIFYMISS.load(Ordering::Relaxed) != 0 {
        crate::kprintf!(
            "in_pcblookup_listen: laddr={:08x} lport={} rdom={}\n",
            u32::from_be(laddr.s_addr),
            u16::from_be(lport),
            rdomain
        );
    }
    inp
}

/// `in_pcbset_rtableid`: moves an unbound `inp` to routing table `rtableid`.
pub fn in_pcbset_rtableid(inp: &'static Inpcb, rtableid: u32) -> Result<(), Errno> {
    let table = inp.table();

    // table must exist
    if !rtable_exists(rtableid) {
        return Err(Errno::EINVAL);
    }

    mtx_enter(&table.inpt_mtx);
    if inp.inp_lport.get() != 0 {
        mtx_leave(&table.inpt_mtx);
        return Err(Errno::EBUSY);
    }
    inp.inp_rtableid.set(rtableid);
    in_pcbrehash(inp);
    mtx_leave(&table.inpt_mtx);

    Ok(())
}

/// `in_pcbset_addr`: gives `inp` both addresses at once (a connection accepted from a
/// listener), unless another control block has them.
pub fn in_pcbset_addr(
    inp: &'static Inpcb,
    fsa: &SockaddrUnion,
    lsa: &SockaddrUnion,
    rtableid: u32,
) -> Result<(), Errno> {
    let table = inp.table();

    #[cfg(feature = "inet6")]
    if inp.has_flags(INP_IPV6) {
        kassert!(fsa.sa_family() == AF_INET6);
        kassert!(lsa.sa_family() == AF_INET6);
        return in6_pcbset_addr(inp, &su_sin6(fsa), &su_sin6(lsa), rtableid);
    }
    kassert!(fsa.sa_family() == AF_INET);
    kassert!(lsa.sa_family() == AF_INET);
    let fsin = fsa.sin();
    let lsin = lsa.sin();

    mtx_enter(&table.inpt_mtx);

    let t = in_pcblookup_lock(
        table,
        fsin.sin_addr,
        fsin.sin_port,
        lsin.sin_addr,
        lsin.sin_port,
        rtableid,
        IN_PCBLOCK_HOLD,
    );
    if t.is_some() {
        mtx_leave(&table.inpt_mtx);
        return Err(Errno::EADDRINUSE);
    }

    inp.inp_rtableid.set(rtableid);
    inp.inp_laddr.set(lsin.sin_addr);
    inp.inp_lport.set(lsin.sin_port);
    inp.inp_faddr.set(fsin.sin_addr);
    inp.inp_fport.set(fsin.sin_port);
    in_pcbrehash(inp);

    mtx_leave(&table.inpt_mtx);

    inp.inp_flowid.set(stoeplitz_ip4port(
        inp.inp_faddr.get().s_addr,
        inp.inp_laddr.get().s_addr,
        inp.inp_fport.get(),
        inp.inp_lport.get(),
    ));
    Ok(())
}

/// `in_pcbunset_faddr`: forgets the foreign address.
pub fn in_pcbunset_faddr(inp: &'static Inpcb) {
    let table = inp.table();

    mtx_enter(&table.inpt_mtx);
    #[cfg(feature = "inet6")]
    if inp.has_flags(INP_IPV6) {
        inp.inp_faddr6.set(IN6ADDR_ANY);
    } else {
        inp.inp_faddr.set(InAddr { s_addr: INADDR_ANY });
    }
    #[cfg(not(feature = "inet6"))]
    inp.inp_faddr.set(InAddr { s_addr: INADDR_ANY });
    inp.inp_fport.set(0);
    in_pcbrehash(inp);
    mtx_leave(&table.inpt_mtx);
}

/// `in_pcbunset_laddr`: forgets both addresses (the local port stays).
pub fn in_pcbunset_laddr(inp: &'static Inpcb) {
    let table = inp.table();

    mtx_enter(&table.inpt_mtx);
    #[cfg(feature = "inet6")]
    let inet6 = inp.has_flags(INP_IPV6);
    #[cfg(not(feature = "inet6"))]
    let inet6 = false;
    if inet6 {
        #[cfg(feature = "inet6")]
        {
            inp.inp_faddr6.set(IN6ADDR_ANY);
            inp.inp_laddr6.set(IN6ADDR_ANY);
        }
    } else {
        inp.inp_faddr.set(InAddr { s_addr: INADDR_ANY });
        inp.inp_laddr.set(InAddr { s_addr: INADDR_ANY });
    }
    inp.inp_fport.set(0);
    in_pcbrehash(inp);
    mtx_leave(&table.inpt_mtx);
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
pub(crate) mod tests {
    // Host tests for the Internet control blocks: the port bitmaps, binding (explicit ports,
    // picked ports, `EADDRINUSE` and `SO_REUSEPORT`), the local port, connection and listen
    // lookups, a table that grows past its load factor, the iterator and detaching.
    //
    // [`setup`] is the network setup of `ip_input`'s tests plus a thread with credentials made
    // `curproc` (sockets read it), the socket pool and `in_init`; the UDP and raw IP tests use it
    // too. [`teardown`] clears `curproc`.

    use std::boxed::Box;
    use std::sync::MutexGuard;
    use std::vec::Vec;
    use std::{assert, assert_eq};

    use super::*;
    use crate::kern::kern_prot::{crget, crhold};
    use crate::kern::uipc_mbuf::m_get;
    use crate::kern::uipc_socket::{soalloc, soinit};
    use crate::kern::uipc_socket2::{solock, sounlock};
    use crate::machine::Machine;
    use crate::machine::cpu::Cpu;
    use crate::netinet::in_proto::INETSW;
    use crate::sys::mbuf::{M_DONTWAIT, MT_SONAME};
    use crate::sys::proc::Process;

    /// The network setup, a `curproc` with credentials, sockets and control blocks.
    pub(crate) fn setup() -> (
        MutexGuard<'static, ()>,
        MutexGuard<'static, ()>,
        &'static Proc,
    ) {
        let (g, t) = crate::netinet::ip_input::tests::setup();
        crate::kern::kern_proc::procinit();
        let pr: &'static Process = Box::leak(Box::new(Process::new()));
        let p: &'static Proc = Box::leak(Box::new(Proc::new()));
        p.p_p.set(pr);
        pr.ps_mainproc.set(p);
        let cr = crget();
        p.p_ucred.set(cr);
        pr.ps_ucred.set(crhold(cr));
        Machine::set_curproc(Machine::curcpu(), p);
        soinit();
        in_init();
        (g, t, p)
    }

    /// Undoes what outlives the reset memory: `curproc`.
    pub(crate) fn teardown() {
        Machine::set_curproc(Machine::curcpu(), ptr::null());
    }

    /// An address mbuf holding `sin`.
    pub(crate) fn nam(sin: SockaddrIn) -> &'static Mbuf {
        let m = m_get(M_DONTWAIT, MT_SONAME).expect("mbuf");
        m.m_len().set(size_of::<SockaddrIn>() as u32);
        // SAFETY: a fresh mbuf of `MLEN` bytes.
        unsafe { mtod::<SockaddrIn>(m).write_unaligned(sin) };
        m
    }

    /// `addr:port` as a `sockaddr_in`.
    fn sin(addr: [u8; 4], port: u16) -> SockaddrIn {
        SockaddrIn {
            sin_len: size_of::<SockaddrIn>() as u8,
            sin_family: AF_INET,
            sin_port: htons(port),
            sin_addr: InAddr {
                s_addr: u32::from_ne_bytes(addr),
            },
            ..SockaddrIn::default()
        }
    }

    /// A UDP socket with a control block in `table`.
    fn pcb(table: &'static Inpcbtable) -> &'static Inpcb {
        let so = soalloc(&INETSW[1], M_WAIT).expect("socket");
        so.so_type.set(SOCK_DGRAM);
        in_pcballoc(so, table, M_WAIT).expect("in_pcballoc");
        sotoinpcb(so).expect("attached")
    }

    /// Detaches the control block and lets the socket go, as `udp_detach` and `soclose` do
    /// (holding a reference of their own across the unlock).
    fn release(inp: &'static Inpcb) {
        let so = inp.socket();
        let _ = soref(Some(so));
        solock(so);
        so.set_state(SS_NOFDREF);
        in_pcbdetach(inp);
        sounlock(so);
        sorele(so);
    }

    /// A table of `hashsize` buckets.
    fn table(hashsize: i32) -> &'static Inpcbtable {
        let t: &'static Inpcbtable = Box::leak(Box::new(Inpcbtable::new()));
        in_pcbinit(t, hashsize);
        t
    }

    #[test]
    fn the_port_bitmaps_and_their_defaults() {
        let (_g, _t, _p) = setup();
        let m: Box<[AtomicU32; DP_MAPSIZE]> = Box::new([const { AtomicU32::new(0) }; DP_MAPSIZE]);
        assert_eq!(DP_MAPSIZE, 2048);
        dp_set(&m, 65535);
        dp_set(&m, 33);
        assert!(dp_isset(&m, 65535) && dp_isset(&m, 33) && !dp_isset(&m, 32));
        dp_clr(&m, 33);
        assert!(!dp_isset(&m, 33));

        // ip_init filled the defaults.
        assert!(in_baddynamic(2049, IPPROTO_TCP as u16));
        assert!(in_baddynamic(7784, IPPROTO_UDP as u16));
        assert!(!in_baddynamic(7784, IPPROTO_TCP as u16));
        assert!(!in_baddynamic(2049, 1));
        assert!(in_rootonly(80, IPPROTO_UDP as u16));
        assert!(in_rootonly(2049, IPPROTO_UDP as u16));
        assert!(!in_rootonly(5353, IPPROTO_UDP as u16));
        teardown();
    }

    #[test]
    fn bind_lookups_and_the_iterator() {
        let (_g, _t, p) = setup();
        let t = table(1);

        // An explicit port on the wildcard address.
        let a = pcb(t);
        let n = nam(sin([0; 4], 5000));
        in_pcbbind(a, Some(n), p).expect("bind");
        assert_eq!(a.inp_lport.get(), htons(5000));
        let found = in_pcblookup_local_lock(t, &ZEROIN46_ADDR, htons(5000), 0, 0, IN_PCBLOCK_GRAB);
        assert!(found.is_some_and(|f| ptr::eq(f, a)));
        in_pcbunref(found);
        let any = Inpaddru::from_addr(sin([10, 0, 2, 15], 0).sin_addr);
        assert!(in_pcblookup_local_lock(t, &any, htons(5000), 0, 0, IN_PCBLOCK_GRAB).is_none());
        let wild =
            in_pcblookup_local_lock(t, &any, htons(5000), INPLOOKUP_WILDCARD, 0, IN_PCBLOCK_GRAB);
        assert!(wild.is_some_and(|f| ptr::eq(f, a)));
        in_pcbunref(wild);
        // Bound already.
        assert_eq!(in_pcbbind(a, Some(n), p), Err(Errno::EINVAL));

        // The same port again (a second socket grows the table past its load factor).
        let b = pcb(t);
        assert!(t.inpt_size.get() > 1, "resized");
        assert_eq!(in_pcbbind(b, Some(n), p), Err(Errno::EADDRINUSE));
        // ... unless both sockets allow it.
        a.socket().so_options.set(SO_REUSEPORT);
        b.socket().so_options.set(SO_REUSEPORT);
        in_pcbbind(b, Some(n), p).expect("SO_REUSEPORT bind");
        // An address that is not ours.
        let c = pcb(t);
        let other = nam(sin([192, 0, 2, 1], 5001));
        assert_eq!(in_pcbbind(c, Some(other), p), Err(Errno::EADDRNOTAVAIL));
        // A picked port, from the default range.
        in_pcbbind(c, None, p).expect("anonymous bind");
        let port = i32::from(u16::from_be(c.inp_lport.get()));
        assert!(
            (IPPORT_RESERVED..=IPPORT_USERRESERVED).contains(&port),
            "port {port}"
        );

        // A connection's quadruple, then the exact and the listen lookups.
        let fsin = sin([10, 0, 2, 2], 53);
        let lsin = sin([10, 0, 2, 15], 6000);
        let d = pcb(t);
        let (fsu, lsu) = (
            SockaddrUnion::from_sin(&fsin),
            SockaddrUnion::from_sin(&lsin),
        );
        in_pcbset_addr(d, &fsu, &lsu, 0).expect("set_addr");
        assert_eq!(
            in_pcbset_addr(pcb(t), &fsu, &lsu, 0),
            Err(Errno::EADDRINUSE)
        );
        let hit = in_pcblookup(
            t,
            fsin.sin_addr,
            fsin.sin_port,
            lsin.sin_addr,
            lsin.sin_port,
            0,
        );
        assert!(hit.is_some_and(|h| ptr::eq(h, d)));
        in_pcbunref(hit);
        let listen = in_pcblookup_listen(t, lsin.sin_addr, htons(5000), None, 0);
        assert!(listen.is_some_and(|l| ptr::eq(l, a) || ptr::eq(l, b)));
        in_pcbunref(listen);
        assert!(in_pcblookup_listen(t, lsin.sin_addr, htons(5002), None, 0).is_none());

        // in_pcbunset_laddr forgets the quadruple; the exact lookup misses then.
        in_pcbunset_laddr(d);
        assert!(
            in_pcblookup(
                t,
                fsin.sin_addr,
                fsin.sin_port,
                lsin.sin_addr,
                lsin.sin_port,
                0
            )
            .is_none()
        );

        // The iterator sees every control block once, with an aborted walk in between.
        let iter = InpcbIterator::new();
        let mut seen = Vec::new();
        let mut inp = None;
        mtx_enter(&t.inpt_mtx);
        // SAFETY: the mutex is held; `iter` lives until the walk ends.
        while let Some(i) = unsafe { in_pcb_iterator(t, inp, &iter) } {
            inp = Some(i);
            seen.push(ptr::from_ref(i));
        }
        let abort = InpcbIterator::new();
        // SAFETY: as above, ended by the abort.
        let first = unsafe { in_pcb_iterator(t, None, &abort) };
        // SAFETY: the same walk.
        unsafe { in_pcb_iterator_abort(t, first, &abort) };
        mtx_leave(&t.inpt_mtx);
        assert_eq!(seen.len() as i32, t.inpt_count.get());

        for i in [a, b, c, d] {
            release(i);
        }
        assert_eq!(t.inpt_count.get(), 1, "the refused one is left");
        teardown();
    }

    /// IPv6 helpers shared by the UDP and TCP tests (a test interface with `fd00:77::1/64` whose
    /// output the tests read, address mbufs, packets from `fd00:77::2`), and the `INP_IPV6`
    /// branches of binding, port picking, connecting and the lookups.
    #[cfg(feature = "inet6")]
    pub(crate) mod inet6 {
        use std::sync::Mutex as StdMutex;
        use std::{assert, assert_eq, assert_ne, vec, vec::Vec};

        use core::mem::size_of;
        use core::ptr;
        use core::sync::atomic::Ordering;

        use super::{setup, teardown};
        use crate::kern::uipc_mbuf::{m_freem, m_get};
        use crate::kern::uipc_socket::{soclose, socreate};
        use crate::kern::uipc_socket2::{solock, sounlock};
        use crate::net::if_::tests::{test_ifnet, test_packet};
        use crate::net::if_var::Ifnet;
        use crate::net::route::Rtentry;
        use crate::netinet::in_pcb::*;
        use crate::netinet6::in6::tests::a6;
        use crate::netinet6::in6::{
            IN6ADDR_ANY, In6Addr, SockaddrIn6, in6_ioctl, in6_prefixlen2mask, in6ifa_ifpwithaddr,
        };
        use crate::netinet6::in6_cksum::in6_cksum;
        use crate::netinet6::in6_pcb::in6_pcblookup;
        use crate::netinet6::in6_var::{IN6_IFF_TENTATIVE, In6Aliasreq, SIOCAIFADDR_IN6};
        use crate::netinet6::nd6::ND6_INFINITE_LIFETIME;
        use crate::sys::endian::htons;
        use crate::sys::errno::Errno;
        use crate::sys::mbuf::{M_DONTWAIT, MT_SONAME, Mbuf, mtod};
        use crate::sys::socket::{AF_INET6, SOCK_DGRAM, Sockaddr};

        /// What the IPv6 test interface was asked to send, as bytes from the IPv6 header on.
        pub(crate) static SENT6: StdMutex<Vec<Vec<u8>>> = StdMutex::new(Vec::new());

        /// Takes what the test interface sent so far.
        pub(crate) fn take_sent6() -> Vec<Vec<u8>> {
            core::mem::take(&mut *SENT6.lock().unwrap_or_else(|e| e.into_inner()))
        }

        /// A driver `ioctl` that accepts the address and multicast requests (and brings the
        /// interface up on the first address).
        ///
        /// # Safety
        ///
        /// `IfIoctlFn`'s contract.
        unsafe fn accepting_ioctl(
            ifp: &'static Ifnet,
            cmd: u64,
            _data: *mut u8,
        ) -> Result<(), Errno> {
            use crate::net::if_::{IFF_RUNNING, IFF_UP};
            use crate::sys::sockio::{SIOCADDMULTI, SIOCDELMULTI, SIOCSIFADDR};
            match cmd {
                SIOCSIFADDR => {
                    ifp.if_flags.set(ifp.if_flags.get() | IFF_UP | IFF_RUNNING);
                    Ok(())
                }
                SIOCADDMULTI | SIOCDELMULTI => Ok(()),
                _ => Err(Errno::ENOTTY),
            }
        }

        /// A driver output that keeps the bytes of what it is handed.
        ///
        /// # Safety
        ///
        /// `IfOutputFn`'s contract.
        unsafe fn capturing_output(
            _ifp: &'static Ifnet,
            m: &'static Mbuf,
            _dst: *const Sockaddr,
            _rt: Option<&'static Rtentry>,
        ) -> Result<(), Errno> {
            let b = crate::netinet::ip_input::tests::bytes(m);
            SENT6.lock().unwrap_or_else(|e| e.into_inner()).push(b);
            m_freem(m);
            Ok(())
        }

        /// An attached Ethernet-like interface `name` with `fd00:77::1/64`, usable at once (its
        /// duplicate address detection skipped), whose output lands in [`SENT6`].
        pub(crate) fn test_if6(name: &[u8]) -> &'static Ifnet {
            let ifp = test_ifnet(name);
            ifp.if_ioctl.set(Some(accepting_ioctl));
            ifp.if_output.set(Some(capturing_output));
            ifp.if_type.set(crate::net::if_types::IFT_ETHER);
            ifp.if_flags.set(crate::net::if_::IFF_MULTICAST);
            ifp.if_mtu.set(1500);
            let mut sdl = crate::net::if_dl::SockaddrDl {
                sdl_alen: 6,
                ..Default::default()
            };
            sdl.sdl_data[..6].copy_from_slice(&crate::netinet::ip_input::tests::OURS);
            ifp.if_sadl
                .set(std::boxed::Box::leak(std::boxed::Box::new(sdl)));
            crate::net::if_::if_attach(ifp);

            let mut ifra = In6Aliasreq::zeroed();
            ifra.ifra_name = ifp.if_xname.get();
            *ifra.ifra_addr_mut() = SockaddrIn6::with_addr(a6("fd00:77::1"));
            ifra.ifra_prefixmask = SockaddrIn6::with_addr(IN6ADDR_ANY);
            in6_prefixlen2mask(&mut ifra.ifra_prefixmask.sin6_addr, 64);
            ifra.ifra_lifetime.ia6t_vltime = ND6_INFINITE_LIFETIME;
            ifra.ifra_lifetime.ia6t_pltime = ND6_INFINITE_LIFETIME;
            // SAFETY: an `in6_aliasreq`, from the kernel itself, aligned as a local.
            unsafe {
                in6_ioctl(
                    SIOCAIFADDR_IN6,
                    ptr::from_mut(&mut ifra).cast(),
                    Some(ifp),
                    true,
                )
            }
            .expect("SIOCAIFADDR_IN6");
            let ia = in6ifa_ifpwithaddr(ifp, &a6("fd00:77::1")).expect("the address");
            ia.ia6_flags.set(ia.ia6_flags.get() & !IN6_IFF_TENTATIVE);
            let _ = take_sent6();
            ifp
        }

        /// `[addr]:port` (host order port).
        pub(crate) fn sin6(addr: In6Addr, port: u16) -> SockaddrIn6 {
            SockaddrIn6 {
                sin6_port: htons(port),
                ..SockaddrIn6::with_addr(addr)
            }
        }

        /// An address mbuf holding `sin6`.
        pub(crate) fn nam6(sin6: SockaddrIn6) -> &'static Mbuf {
            let m = m_get(M_DONTWAIT, MT_SONAME).expect("mbuf");
            m.m_len().set(size_of::<SockaddrIn6>() as u32);
            // SAFETY: a fresh mbuf of `MLEN` bytes.
            unsafe { mtod::<SockaddrIn6>(m).write_unaligned(sin6) };
            m
        }

        /// The packet `fd00:77::2 -> fd00:77::1` of next header `nxt` carrying `payload` (a
        /// transport header and its data), received on `ifp`; the transport checksum at
        /// `cksum_off` in `payload` is filled in unless `cksum_off` is `None`.
        pub(crate) fn packet6(
            ifp: &Ifnet,
            nxt: u8,
            payload: &[u8],
            cksum_off: Option<usize>,
        ) -> &'static Mbuf {
            let mut v = vec![0x60, 0, 0, 0];
            v.extend_from_slice(&(payload.len() as u16).to_be_bytes());
            v.extend_from_slice(&[nxt, 64]);
            v.extend_from_slice(&a6("fd00:77::2").s6_addr);
            v.extend_from_slice(&a6("fd00:77::1").s6_addr);
            v.extend_from_slice(payload);
            if let Some(off) = cksum_off {
                let m = test_packet(&v);
                let sum = in6_cksum(m, nxt, 40, payload.len() as u32);
                m_freem(m);
                v[40 + off..42 + off].copy_from_slice(&sum.to_ne_bytes());
            }
            let m = test_packet(&v);
            m.m_pkthdr().ph_ifidx.set(u32::from(ifp.if_index.get()));
            m
        }

        #[test]
        fn inet6_bind_pick_connect_and_lookups() {
            let (_g, _t, p) = setup();
            crate::netinet::udp_usrreq::udp_init();
            let _ifp = test_if6(b"tpcb6");
            let ours = a6("fd00:77::1");

            let so = socreate(i32::from(AF_INET6), SOCK_DGRAM, 0).expect("socket");
            let inp = sotoinpcb(so).expect("attached");
            assert!(inp.has_flags(INP_IPV6), "in_pcballoc marks a PF_INET6 pcb");
            assert_eq!(inp.inp_cksum6.get(), -1);
            let t = inp.table();

            // An explicit port on our address, then the local port lookups.
            in_pcbbind(inp, Some(nam6(sin6(ours, 5000))), p).expect("bind");
            assert_eq!(inp.inp_laddr6.get(), ours);
            assert_eq!(inp.inp_lport.get(), htons(5000));
            let found = in_pcblookup_local_lock(
                t,
                &Inpaddru::from_addr6(ours),
                htons(5000),
                INPLOOKUP_IPV6,
                0,
                IN_PCBLOCK_GRAB,
            );
            assert!(found.is_some_and(|f| ptr::eq(f, inp)));
            in_pcbunref(found);
            let any = Inpaddru::from_addr6(IN6ADDR_ANY);
            let lookup = |flags| {
                let f = in_pcblookup_local_lock(t, &any, htons(5000), flags, 0, IN_PCBLOCK_GRAB);
                let hit = f.is_some_and(|f| ptr::eq(f, inp));
                in_pcbunref(f);
                hit
            };
            assert!(!lookup(INPLOOKUP_IPV6), "an exact match only");
            assert!(lookup(INPLOOKUP_IPV6 | INPLOOKUP_WILDCARD));
            assert_eq!(
                in_pcbbind(inp, Some(nam6(sin6(ours, 5001))), p),
                Err(Errno::EINVAL),
                "bound already"
            );

            // The same address and port again; an address that is not ours.
            let so2 = socreate(i32::from(AF_INET6), SOCK_DGRAM, 0).expect("socket");
            let inp2 = sotoinpcb(so2).expect("attached");
            assert_eq!(
                in_pcbbind(inp2, Some(nam6(sin6(ours, 5000))), p),
                Err(Errno::EADDRINUSE)
            );
            assert_eq!(
                in_pcbbind(inp2, Some(nam6(sin6(a6("fd00:77::9"), 5000))), p),
                Err(Errno::EADDRNOTAVAIL)
            );
            // A picked port from the default range, on the unspecified address.
            in_pcbbind(inp2, None, p).expect("anonymous bind");
            let port = i32::from(u16::from_be(inp2.inp_lport.get()));
            assert!(
                (IPPORT_FIRSTAUTO.load(Ordering::Relaxed)
                    ..=IPPORT_LASTAUTO.load(Ordering::Relaxed))
                    .contains(&port),
                "port {port}"
            );
            assert_eq!(inp2.inp_laddr6.get(), IN6ADDR_ANY);

            // connect(2) of an unbound socket: in6_pcbconnect binds it to the selected source
            // and a picked port (in_pcbbind_locked with an IPv6 local address).
            let so3 = socreate(i32::from(AF_INET6), SOCK_DGRAM, 0).expect("socket");
            let inp3 = sotoinpcb(so3).expect("attached");
            let peer = a6("fd00:77::2");
            solock(so3);
            in_pcbconnect(inp3, nam6(sin6(peer, 53))).expect("connect");
            assert!(
                in_pcbrtentry(inp3).is_some(),
                "in6_pcbrtentry: the route to the peer"
            );
            sounlock(so3);
            assert_eq!(
                inp3.inp_laddr6.get(),
                ours,
                "the source in6_pcbselsrc picked"
            );
            assert_ne!(inp3.inp_lport.get(), 0);
            assert_eq!(inp3.inp_faddr6.get(), peer);
            let hit = in6_pcblookup(t, &peer, htons(53), &ours, inp3.inp_lport.get(), 0);
            assert!(hit.is_some_and(|h| ptr::eq(h, inp3)));
            in_pcbunref(hit);

            // getpeername(2) and getsockname(2) of an INP_IPV6 pcb.
            let m = m_get(M_DONTWAIT, MT_SONAME).expect("mbuf");
            in_setpeeraddr(inp3, m);
            // SAFETY: `in6_setpeeraddr` wrote a `sockaddr_in6`.
            let got = unsafe { mtod::<SockaddrIn6>(m).read_unaligned() };
            assert_eq!((got.sin6_family, got.sin6_port), (AF_INET6, htons(53)));
            assert_eq!(got.sin6_addr, peer);
            in_setsockaddr(inp3, m);
            // SAFETY: as above, `in6_setsockaddr`'s.
            let got = unsafe { mtod::<SockaddrIn6>(m).read_unaligned() };
            assert_eq!(got.sin6_addr, ours);
            m_freem(Some(m));

            // in_pcbunset_laddr forgets both IPv6 addresses.
            in_pcbunset_laddr(inp3);
            assert_eq!(
                (inp3.inp_laddr6.get(), inp3.inp_faddr6.get()),
                (IN6ADDR_ANY, IN6ADDR_ANY)
            );

            for so in [so, so2, so3] {
                soclose(so, 0).expect("close");
            }
            let _ = take_sent6();
            teardown();
        }

        #[test]
        fn kern_file_reports_an_inet6_socket() {
            use crate::kern::kern_sysctl::fill_file;
            use crate::netinet::udp_usrreq::{udp_bind, udp_init};
            use crate::sys::sysctl::KinfoFile;

            let (_g, _t, p) = setup();
            udp_init();
            let _ifp = test_if6(b"tkf6");
            let so = socreate(i32::from(AF_INET6), SOCK_DGRAM, 0).expect("socket");
            solock(so);
            udp_bind(so, nam6(sin6(a6("fd00:77::1"), 5353)), p).expect("bind");
            let mut kf = KinfoFile::zeroed();
            fill_file(&mut kf, None, None, 0, None, None, p, Some(so), false);
            sounlock(so);

            assert_eq!(kf.so_family, u32::from(AF_INET6));
            assert_eq!(kf.inp_lport, u32::from(htons(5353)));
            let ours = a6("fd00:77::1");
            for (i, w) in kf.inp_laddru.iter().enumerate() {
                assert_eq!(*w, ours.s6_addr32(i), "inp_laddru[{i}]");
            }
            assert_eq!(kf.inp_faddru, [0; 4]);
            assert_eq!(kf.inp_ppcb, 0, "no pointers without show_pointers");

            soclose(so, 0).expect("close");
            teardown();
        }
    }
}
/* </TESTS> */
