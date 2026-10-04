/*	$OpenBSD: nd6.h,v 1.106 2026/03/23 13:12:39 jsg Exp $	*/
/*	$KAME: nd6.h,v 1.95 2002/06/08 11:31:06 itojun Exp $	*/
/*	$OpenBSD: nd6.c,v 1.305 2025/11/27 21:54:28 bluhm Exp $	*/
/*	$KAME: nd6.c,v 1.280 2002/06/08 19:52:07 itojun Exp $	*/
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
/* </LICENSES> */

//! IPv6 Neighbor Discovery: the neighbor cache, the per-interface ND information and the
//! address resolution: `<netinet6/nd6.h>` and `netinet6/nd6.c`.
//!
//! Upstream: sys/netinet6/nd6.h @ 3ce1f3f79392
//! Upstream: sys/netinet6/nd6.c @ 3ce1f3f79392
//!
//! `nd6.c` shares the module with the header. Locks: \[I\] immutable after creation, \[K\]
//! kernel lock, \[m\] `nd6_mtx` (needed when the net lock is shared), \[N\] net lock,
//! \[a\] atomic operations.
//!
//! As ARP does for IPv4 (`netinet/if_ether.rs`), the neighbor cache lives in the routing
//! table: a cloning route of an on-link prefix makes a host route per neighbor
//! (`RTF_LLINFO`), whose gateway is a `sockaddr_dl` holding the neighbor's link-layer
//! address and whose `rt_llinfo` is a [`LlinfoNd6`] (the reachability state of RFC 2461
//! 7.3.2, the packets held while the address is resolved, the probes sent). `nd6_resolve`
//! answers `ether_output` from it or holds the packet and solicits, `nd6_nbr.rs` fills it
//! from solicitations and advertisements (`nd6_cache_lladdr`, `nd6_na_input`), and
//! `nd6_timer` walks it through INCOMPLETE, REACHABLE, STALE, DELAY and PROBE.
//!
//! ## Deviations
//! - `struct ifnet`'s `if_nd` (`struct nd_ifinfo *`) is `Cell<Option<NonNull<NdIfinfo>>>`
//!   (`net/if_var.rs`); [`if_nd`] reads it as the `Cell<NdIfinfo>` the net lock guards.
//!   `NdIfinfo` itself is plain data, the layout `SIOCGIFINFO_IN6` copies out in
//!   `struct in6_ndireq`. `nd6_ifdetach` also clears the pointer it frees; `nd6_slowtimo`
//!   and `SIOCGIFINFO_IN6` skip an interface without one (the C reads every `if_nd`).
//! - `struct llinfo_nd6` is [`LlinfoNd6`] with `Cell` members; `struct llinfo_nd6_iterator`
//!   (the C's smaller marker with the same first two members) is an `LlinfoNd6` without a
//!   route, as `if_ether.rs` does for `llinfo_arp`. The markers of `nd6_timer` and
//!   `nd6_purge` are stack variables as in C: the walk always runs to its end, which
//!   unlinks them.
//! - `struct nd_opts` holds `Option<NonNull<NdOptHdr>>`: the options inside the packet.
//!   `nd6_options` takes the options as a byte slice (the C's pointer and length) and
//!   answers `bool` (the C's 0/-1).
//! - `ND6_LLINFO_PERMANENT(n)` and `ND_COMPUTE_RTIME(x)` are functions. `nd6_gctimer`
//!   (a C `const int`) is [`ND6_GCTIMER`]; `nd6_timer_next`, `nd6_expire_next`,
//!   `nd6_maxndopt` and `nd6_inuse` are private atomics.
//! - `nd6_cache_lladdr` takes the link-layer address as a slice (the C's pointer and
//!   `lladdrlen`, which the C never reads); the callers pass the bytes after the option
//!   header and checked their length against `if_addrlen`. Its last argument is the C's
//!   `i_am_router`.
//! - `nd6_lookup` takes `Option<&Ifnet>`: it only reads the interface.
//! - `nd6_free` reads the router flag only when the route still has its `llinfo_nd6` (the
//!   C dereferences `rt_llinfo` even on `nd6_cache_lladdr`'s `fail` path, where it may be
//!   NULL).
//! - `inet_ntop(AF_INET6, ...)` (`netinet/inet_ntop.c`) is not ported: the log lines print
//!   addresses through [`In6Ntop`], which writes what it writes for `AF_INET6`.
//! - `KERNEL_LOCK()` around `in6_addmulti`/`in6_delmulti` is nothing without
//!   `MULTIPROCESSOR`.
//! - The casts and inline code the C repeats in `nd6.c`, `nd6_nbr.c` and `nd6_rtr.c` are
//!   crate-visible helpers here: `rt_ln` (`(struct llinfo_nd6 *)rt->rt_llinfo`),
//!   `rt_key_in6` (`satosin6(rt_key(rt))->sin6_addr`), `nd6_opt_lladdr` (an option's
//!   address and `lladdrlen`), `nd6_llchanged`/`nd6_llstore` (the `bcmp`/`bcopy` of
//!   `if_addrlen` bytes against `LLADDR(sdl)`), `nd6_ether_sprintf`; `nd6_llsol` is the
//!   solicited-node address `nd6_rtrequest` computes twice.
//! - `nd6_init` also empties `nd6_list` (the C's static initializer), as `arpinit` does.

use core::cell::Cell;
use core::ffi::c_void;
use core::fmt;
use core::mem::size_of;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicI32, AtomicI64, AtomicU32, Ordering};

use crate::dev::rnd::arc4random;
use crate::kassert;
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_synch::{refcnt_init, refcnt_rele, refcnt_take};
use crate::kern::kern_task::{task_add, task_set};
use crate::kern::kern_tc::{gettime, getuptime};
use crate::kern::kern_timeout::{timeout_add_sec, timeout_set, timeout_set_proc};
use crate::kern::subr_pool::{pool_get, pool_init, pool_put};
use crate::kern::subr_prf::{Str, panic, printf};
use crate::kern::uipc_mbuf::{m_freem, ml_dequeue, mq_delist, mq_init, mq_purge, mq_push};
use crate::log;
use crate::machine::intr::IPL_SOFTNET;
use crate::net::if_::{
    IFF_MULTICAST, IFNAMSIZ, IFNETLIST, if_get, if_output_mq, if_put, ifaof_ifpforaddr, net_tq,
};
use crate::net::if_dl::{SockaddrDl, lladdr, satosdl, sdltosa};
use crate::net::if_ethersubr::ether_sprintf;
use crate::net::if_types::{IFT_CARP, IFT_ETHER, IFT_IEEE80211};
use crate::net::if_var::{Ifaddr, Ifnet};
use crate::net::route::{
    RT_RESOLVE, RTAX_DST, RTAX_GATEWAY, RTF_ANNOUNCE, RTF_CACHED, RTF_CLONING, RTF_GATEWAY,
    RTF_HOST, RTF_LLINFO, RTF_LOCAL, RTF_MPLS, RTF_MULTICAST, RTF_REJECT, RTF_STATIC, RTM_ADD,
    RTM_DELETE, RTM_INVALIDATE, RTM_RESOLVE, RTP_CONNECTED, RtAddrinfo, Rtentry, rt_getll, rtalloc,
    rtdeletemsg, rtfree, rtref, rtrequest,
};
use crate::net::rtable::rt_key;
use crate::netinet::icmp6::{
    ICMP6_DST_UNREACH, ICMP6_DST_UNREACH_ADDR, Icmp6statCounters, ND_NEIGHBOR_SOLICIT,
    ND_OPT_DNSSL, ND_OPT_MTU, ND_OPT_PREFIX_INFORMATION, ND_OPT_RDNSS, ND_OPT_REDIRECTED_HEADER,
    ND_OPT_SOURCE_LINKADDR, ND_OPT_TARGET_LINKADDR, ND_REDIRECT, ND_REDIRECT_ROUTER,
    ND_ROUTER_ADVERT, ND_ROUTER_SOLICIT, NdOptHdr, icmp6stat_inc,
};
use crate::netinet::if_ether::{ETHER_ADDR_LEN, ether_map_ipv6_multicast};
use crate::netinet::ip6::Ip6Hdr;
use crate::netinet6::icmp6::icmp6_error;
use crate::netinet6::in6::{
    In6Addr, SockaddrIn6, ifa6_is_deprecated, ifa6_is_invalid, ifatoia6, in6_addmulti,
    in6_delmulti, in6_is_addr_linklocal, in6_is_addr_mc_linklocal, in6_is_addr_unspecified,
    in6_lookupmulti, in6_purgeaddr, in6ifa_ifpwithaddr, satosin6_const, sin6tosa_const,
};
use crate::netinet6::in6_proto::{IP6_FORWARDING, IP6_NEIGHBORGCTHRESH};
use crate::netinet6::in6_var::{
    IN6_IFF_AUTOCONF, IN6_IFF_DEPRECATED, In6Ifaddr, SIOCGIFINFO_IN6, SIOCGNBRINFO_IN6,
    in6_are_masked_addr_equal,
};
use crate::netinet6::ip6_var::mtod_ip6;
use crate::netinet6::nd6_nbr::nd6_ns_output;
use crate::netinet6::nd6_rtr::rt6_flush;
use crate::queue_adapter;
use crate::sys::endian::{htonl, htons, ntohs};
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_IP6NDP, M_WAITOK, M_ZERO};
use crate::sys::mbuf::{M_MCAST, Mbuf, MbufList, MbufQueue, ml_len, mq_len};
use crate::sys::mutex::{Mutex, mutex_assert_locked};
use crate::sys::pool::{PR_NOWAIT, PR_ZERO, Pool};
use crate::sys::queue::{TailqEntry, TailqHead};
use crate::sys::refcnt::Refcnt;
use crate::sys::socket::{AF_INET6, AF_LINK, Sockaddr};
use crate::sys::syslog::{LOG_DEBUG, LOG_INFO};
use crate::sys::systm::{
    net_assert_locked, net_assert_locked_exclusive, net_lock, net_lock_shared, net_unlock,
    net_unlock_shared,
};
use crate::sys::task::Task;
use crate::sys::timeout::{Timeout, timeout_pending};
use crate::sys::types::Time;

/// `ND6_SLOWTIMER_INTERVAL`: 1 hour.
const ND6_SLOWTIMER_INTERVAL: i32 = 60 * 60;
/// `ND6_RECALC_REACHTM_INTERVAL`: 2 hours.
const ND6_RECALC_REACHTM_INTERVAL: i32 = 60 * 120;
/// `nd6_gctimer`: 1 day: garbage collection timer.
pub const ND6_GCTIMER: i32 = 60 * 60 * 24;

/// `ND6_LLINFO_PURGE`.
pub const ND6_LLINFO_PURGE: i16 = -3;
/// `ND6_LLINFO_NOSTATE`.
pub const ND6_LLINFO_NOSTATE: i16 = -2;
/// `ND6_LLINFO_INCOMPLETE`.
pub const ND6_LLINFO_INCOMPLETE: i16 = 0;
/// `ND6_LLINFO_REACHABLE`.
pub const ND6_LLINFO_REACHABLE: i16 = 1;
/// `ND6_LLINFO_STALE`.
pub const ND6_LLINFO_STALE: i16 = 2;
/// `ND6_LLINFO_DELAY`.
pub const ND6_LLINFO_DELAY: i16 = 3;
/// `ND6_LLINFO_PROBE`.
pub const ND6_LLINFO_PROBE: i16 = 4;

// protocol constants

/// 1sec.
pub const MAX_RTR_SOLICITATION_DELAY: i32 = 1;
/// 4sec.
pub const RTR_SOLICITATION_INTERVAL: i32 = 4;
/// `MAX_RTR_SOLICITATIONS`.
pub const MAX_RTR_SOLICITATIONS: i32 = 3;

/// `ND6_INFINITE_LIFETIME`.
pub const ND6_INFINITE_LIFETIME: u32 = 0xffff_ffff;

/// `LN_HOLD_QUEUE`: packets held per neighbor until resolved.
pub const LN_HOLD_QUEUE: u32 = 10;
/// `LN_HOLD_TOTAL`: packets held in all of the nd6 queues.
pub const LN_HOLD_TOTAL: u32 = 100;

// node constants

/// msec.
pub const REACHABLE_TIME: u32 = 30000;
/// msec.
pub const RETRANS_TIMER: u32 = 1000;
/// 1024 * 0.5.
pub const MIN_RANDOM_FACTOR: u32 = 512;
/// 1024 * 1.5.
pub const MAX_RANDOM_FACTOR: u32 = 1536;

/// `struct nd_ifinfo`: the Neighbor Discovery state of an interface.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NdIfinfo {
    /// \[N\] Reachable Time.
    pub reachable: u32,
    /// \[N\] BaseReachable recalc timer.
    pub recalctm: i32,
}

/// `struct in6_nbrinfo`: the argument of `SIOCGNBRINFO_IN6`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct In6Nbrinfo {
    /// If name, e.g. "en0".
    pub ifname: [u8; IFNAMSIZ],
    /// IPv6 address of the neighbor.
    pub addr: In6Addr,
    /// Lifetime for NDP state transition.
    pub expire: Time,
    /// Number of queries already sent for addr.
    pub asked: i64,
    /// If it acts as a router.
    pub isrouter: i32,
    /// Reachability state.
    pub state: i32,
}

/// `struct in6_ndireq`: the argument of `SIOCGIFINFO_IN6`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct In6Ndireq {
    /// `ifname`.
    pub ifname: [u8; IFNAMSIZ],
    /// `ndi`.
    pub ndi: NdIfinfo,
}

/// `struct llinfo_nd6`: the link-level information of an `AF_INET6` route (a neighbor
/// cache entry).
pub struct LlinfoNd6 {
    /// \[m\] `ln_list`: global `nd6_list`.
    pub ln_list: TailqEntry<LlinfoNd6>,
    /// \[I\] `ln_rt`: backpointer to rtentry (always `None` for an iterator).
    pub ln_rt: Cell<Option<&'static Rtentry>>,
    /// `ln_refcnt`: entry referenced by list.
    pub ln_refcnt: Refcnt,
    /// `ln_mq`: hold packets until resolved.
    pub ln_mq: MbufQueue,
    /// `ln_saddr6`: source of prompting packet.
    pub ln_saddr6: Cell<In6Addr>,
    /// `ln_asked`: number of queries already sent for addr.
    pub ln_asked: Cell<i64>,
    /// `ln_state`: reachability state.
    pub ln_state: Cell<i16>,
    /// `ln_router`: 2^0: ND6 router bit.
    pub ln_router: Cell<i16>,
}

// SAFETY: the members change under `nd6_mtx` or the net lock, as in C.
unsafe impl Sync for LlinfoNd6 {}

impl LlinfoNd6 {
    /// An entry with no route: what `struct llinfo_nd6_iterator` is in C (see the module's
    /// deviations), and the initial value `nd6_rtrequest` writes into a pool item.
    const fn iterator() -> Self {
        Self {
            ln_list: TailqEntry::new(),
            ln_rt: Cell::new(None),
            ln_refcnt: Refcnt::new(),
            ln_mq: MbufQueue::new(0, IPL_SOFTNET),
            ln_saddr6: Cell::new(In6Addr::new([0; 16])),
            ln_asked: Cell::new(0),
            ln_state: Cell::new(0),
            ln_router: Cell::new(0),
        }
    }
}

queue_adapter!(
    /// `TAILQ_HEAD(llinfo_nd6_head, llinfo_nd6)` through `ln_list`: `nd6_list`.
    pub LlinfoNd6List: LlinfoNd6, ln_list => TailqEntry<LlinfoNd6>
);

/// `struct nd_opts`: the options `nd6_options` found in a message.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NdOpts {
    /// `nd_opts_src_lladdr`.
    pub nd_opts_src_lladdr: Option<NonNull<NdOptHdr>>,
    /// `nd_opts_tgt_lladdr`.
    pub nd_opts_tgt_lladdr: Option<NonNull<NdOptHdr>>,
}

/// `nd6_list`, made `Sync`: changed and walked under `nd6_mtx`.
struct Nd6ListHead(TailqHead<LlinfoNd6List>);

// SAFETY: see the type's doc.
unsafe impl Sync for Nd6ListHead {}

/// An IPv6 address printed as `inet_ntop(AF_INET6, ...)` writes it: lower-case hex words,
/// the first longest run of two or more zero words as `::`, and the IPv4 tail of a
/// compatible or mapped address as a dotted quad (see the module's deviations).
pub struct In6Ntop(pub In6Addr);

impl fmt::Display for In6Ntop {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let b = self.0.s6_addr;
        let words: [u16; 8] =
            core::array::from_fn(|i| u16::from_be_bytes([b[2 * i], b[2 * i + 1]]));

        // Find the longest run of 0x00's in words[] for :: shorthanding.
        let mut best: Option<(usize, usize)> = None;
        let mut cur: Option<(usize, usize)> = None;
        for (i, w) in words.iter().enumerate() {
            if *w == 0 {
                cur = Some(match cur {
                    None => (i, 1),
                    Some((base, len)) => (base, len + 1),
                });
            } else if let Some(c) = cur.take()
                && best.is_none_or(|b| c.1 > b.1)
            {
                best = Some(c);
            }
        }
        if let Some(c) = cur
            && best.is_none_or(|b| c.1 > b.1)
        {
            best = Some(c);
        }
        let best = best.filter(|b| b.1 >= 2);

        for (i, w) in words.iter().enumerate() {
            // Are we inside the best run of 0x00's?
            if let Some((base, len)) = best
                && i >= base
                && i < base + len
            {
                if i == base {
                    f.write_str(":")?;
                }
                continue;
            }
            // Are we following an initial run of 0x00s or any real hex?
            if i != 0 {
                f.write_str(":")?;
            }
            // Is this address an encapsulated IPv4?
            if i == 6
                && let Some((0, len)) = best
                && (len == 6 || (len == 5 && words[5] == 0xffff))
            {
                return write!(f, "{}.{}.{}.{}", b[12], b[13], b[14], b[15]);
            }
            write!(f, "{w:x}")?;
        }
        // Was it a trailing run of 0x00's?
        if let Some((base, len)) = best
            && base + len == 8
        {
            f.write_str(":")?;
        }
        Ok(())
    }
}

/// \[N\] `nd6_timer_next`: at which uptime `nd6_timer` runs.
static ND6_TIMER_NEXT: AtomicI64 = AtomicI64::new(-1);
/// `nd6_expire_next`: at which uptime `nd6_expire` runs.
static ND6_EXPIRE_NEXT: AtomicI64 = AtomicI64::new(-1);

/// `nd6_delay`: delay first probe time 5 second.
pub static ND6_DELAY: AtomicI32 = AtomicI32::new(5);
/// \[a\] `nd6_umaxtries`: maximum unicast query.
pub static ND6_UMAXTRIES: AtomicI32 = AtomicI32::new(3);
/// \[a\] `nd6_mmaxtries`: maximum multicast query.
pub static ND6_MMAXTRIES: AtomicI32 = AtomicI32::new(3);
/// \[a\] `ln_hold_total`: packets currently in the nd6 queue.
#[allow(non_upper_case_globals)] // the C name; `LN_HOLD_TOTAL` is the limit beside it
pub static ln_hold_total: AtomicU32 = AtomicU32::new(0);

/// `nd6_maxndopt`: max # of ND options allowed (preventing too many loops in ND option
/// parsing).
static ND6_MAXNDOPT: AtomicI32 = AtomicI32::new(10);

/// `nd6_mtx`: llinfo_nd6 live time, `rt_llinfo` and `RTF_LLINFO` are protected by it.
static ND6_MTX: Mutex = Mutex::new(IPL_SOFTNET);

/// \[m\] `nd6_list`: list of `llinfo_nd6` structures.
static ND6_LIST: Nd6ListHead = Nd6ListHead(TailqHead::new());
/// \[I\] `nd6_pool`: pool for `llinfo_nd6` structures.
static ND6_POOL: Pool = Pool::new();
/// \[m\] `nd6_inuse`: limit neighbor discovery routes.
static ND6_INUSE: AtomicI32 = AtomicI32::new(0);

/// `nd6_timer_to`.
static ND6_TIMER_TO: Timeout = Timeout::new(nd6_timer, ptr::null_mut());
/// `nd6_slowtimo_ch`.
static ND6_SLOWTIMO_CH: Timeout = Timeout::new(nd6_slowtimo, ptr::null_mut());
/// `nd6_expire_timeout`.
static ND6_EXPIRE_TIMEOUT: Timeout = Timeout::new(nd6_expire_timer, ptr::null_mut());
/// `nd6_expire_task`.
static ND6_EXPIRE_TASK: Task = Task::new(nd6_expire, ptr::null_mut());

/// `ND6_LLINFO_PERMANENT(n)`: whether the entry's route never expires.
pub fn nd6_llinfo_permanent(n: &LlinfoNd6) -> bool {
    match n.ln_rt.get() {
        Some(rt) => rt.rt_expire().get() == 0,
        None => panic(format_args!("ND6_LLINFO_PERMANENT: no route")),
    }
}

/// `ND_COMPUTE_RTIME(x)`: a random reachable time around `x` milliseconds (between 0.5 and
/// 1.5 times it), in seconds.
pub fn nd_compute_rtime(x: u32) -> u32 {
    (MIN_RANDOM_FACTOR * (x >> 10)
        + (arc4random() & ((MAX_RANDOM_FACTOR - MIN_RANDOM_FACTOR) * (x >> 10))))
        / 1000
}

/// `ifp->if_nd`: the interface's Neighbor Discovery information, which `nd6_ifattach`
/// allocated; `None` before it or after `nd6_ifdetach`.
pub fn if_nd(ifp: &Ifnet) -> Option<&'static Cell<NdIfinfo>> {
    // SAFETY: `if_nd` is `nd6_ifattach`'s allocation, freed only by `nd6_ifdetach` when the
    // interface goes away; a `Cell<T>` has the layout of `T`, and the net lock serializes
    // the writers, as in C.
    ifp.if_nd
        .get()
        .map(|nd| unsafe { &*nd.as_ptr().cast::<Cell<NdIfinfo>>() })
}

/// `(struct llinfo_nd6 *)rt->rt_llinfo`: the neighbor cache entry of a route, `None` when
/// it has none.
pub(crate) fn rt_ln(rt: &Rtentry) -> Option<&'static LlinfoNd6> {
    // SAFETY: an inet6 route's `rt_llinfo` is NULL or the entry `nd6_rtrequest` made, which
    // lives until its last `ln_refcnt` reference (the callers hold `nd6_mtx` or the net
    // lock, as in C).
    unsafe { (rt.rt_llinfo.get() as *const LlinfoNd6).as_ref() }
}

/// `pool_put(&nd6_pool, ln)`.
fn ln_put(ln: &LlinfoNd6) {
    pool_put(&ND6_POOL, NonNull::from(ln).cast());
}

/// `satosin6(rt_key(rt))->sin6_addr`: the destination of an inet6 route.
pub(crate) fn rt_key_in6(rt: &Rtentry) -> In6Addr {
    // SAFETY: the key of a route in an inet6 table is a readable `sockaddr_in6`; it may sit
    // at any alignment, so it is copied out.
    unsafe { ptr::read_unaligned(satosin6_const(rt_key(rt))).sin6_addr }
}

/// An interface address on a list, with the lifetime the C's pointers have.
fn ifa_static(ifa: &Ifaddr) -> &'static Ifaddr {
    // SAFETY: an address on an interface's list lives until `ifa_del` and its last
    // `ifafree`, as the C's pointers do (`docs/C_TO_RUST.md`, reference-counted pool objects).
    unsafe { &*ptr::from_ref(ifa) }
}

/// The family of an interface address.
fn ifa_family(ifa: &Ifaddr) -> u8 {
    // SAFETY: an interface address's `ifa_addr` is a readable socket address.
    unsafe { (*ifa.ifa_addr.get()).sa_family }
}

/// `nd6_init`: the neighbor cache pool, the expiry task and the ND timers.
pub fn nd6_init() {
    pool_init(
        &ND6_POOL,
        size_of::<LlinfoNd6>(),
        0,
        IPL_SOFTNET,
        0,
        "nd6",
        None,
    );
    ND6_LIST.0.init();

    task_set(&ND6_EXPIRE_TASK, nd6_expire, ptr::null_mut());

    // start timer
    timeout_set_proc(&ND6_TIMER_TO, nd6_timer, ptr::null_mut());
    timeout_set_proc(&ND6_SLOWTIMO_CH, nd6_slowtimo, ptr::null_mut());
    let _ = timeout_add_sec(&ND6_SLOWTIMO_CH, ND6_SLOWTIMER_INTERVAL);
    timeout_set(&ND6_EXPIRE_TIMEOUT, nd6_expire_timer, ptr::null_mut());
}

/// `nd6_ifattach`: allocates the Neighbor Discovery information of `ifp` (`if_nd`).
pub fn nd6_ifattach(ifp: &Ifnet) {
    let Some(mem) = malloc(size_of::<NdIfinfo>(), M_IP6NDP, M_WAITOK | M_ZERO) else {
        panic(format_args!("nd6_ifattach: no memory"));
    };
    let nd = mem.cast::<NdIfinfo>();
    // SAFETY: a fresh allocation of `size_of::<NdIfinfo>()` bytes, aligned by `malloc`.
    unsafe {
        nd.as_ptr().write(NdIfinfo {
            reachable: nd_compute_rtime(REACHABLE_TIME),
            recalctm: 0,
        })
    };

    ifp.if_nd.set(Some(nd));
}

/// `nd6_ifdetach`: frees the Neighbor Discovery information of `ifp`.
pub fn nd6_ifdetach(ifp: &Ifnet) {
    if let Some(nd) = ifp.if_nd.take() {
        free(nd.cast(), M_IP6NDP, size_of::<NdIfinfo>());
    }
}

/// `nd6_options`: parses multiple ND options in `opt` (the rest of the message) into
/// `ndopts`; `false` for an invalid option (the C's -1). This function is much easier to
/// use, for ND routines that do not need multiple options of the same type.
pub fn nd6_options(opt: &[u8], ndopts: &mut NdOpts) -> bool {
    *ndopts = NdOpts::default();

    if opt.is_empty() {
        return true;
    }

    let last_opt = opt.len();
    let mut next_opt = Some(0usize);
    let mut i = 0;

    while let Some(off) = next_opt {
        // make sure nd_opt_len is inside the buffer
        if off + 1 >= last_opt {
            return nd6_options_invalid(ndopts);
        }

        // every option must have a length greater than zero
        let olen = usize::from(opt[off + 1]) << 3;
        if olen == 0 {
            return nd6_options_invalid(ndopts);
        }

        let next = off + olen;
        if next > last_opt {
            // option overruns the end of buffer
            return nd6_options_invalid(ndopts);
        }
        next_opt = (next != last_opt).then_some(next);
        // (next == last_opt: reached the end of options chain)

        let nd_opt = NonNull::from(&opt[off]).cast::<NdOptHdr>();
        match opt[off] {
            ND_OPT_SOURCE_LINKADDR => {
                if ndopts.nd_opts_src_lladdr.is_none() {
                    ndopts.nd_opts_src_lladdr = Some(nd_opt);
                }
            }
            ND_OPT_TARGET_LINKADDR => {
                if ndopts.nd_opts_tgt_lladdr.is_none() {
                    ndopts.nd_opts_tgt_lladdr = Some(nd_opt);
                }
            }
            ND_OPT_MTU
            | ND_OPT_REDIRECTED_HEADER
            | ND_OPT_PREFIX_INFORMATION
            | ND_OPT_DNSSL
            | ND_OPT_RDNSS => {
                // Don't warn, not used by kernel
            }
            _ => {
                // Unknown options must be silently ignored, to accommodate future extension
                // to the protocol.
            }
        }

        i += 1;
        if i > ND6_MAXNDOPT.load(Ordering::Relaxed) {
            icmp6stat_inc(Icmp6statCounters::Icp6sNdToomanyopt);
            break;
        }
    }

    true
}

/// `nd6_options`'s `invalid:` label.
fn nd6_options_invalid(ndopts: &mut NdOpts) -> bool {
    *ndopts = NdOpts::default();
    icmp6stat_inc(Icmp6statCounters::Icp6sNdBadopt);
    false
}

/// The link-layer address an option `nd6_options` found carries: the bytes after its
/// header, and the C's `lladdrlen` (the whole option's length, `nd_opt_len << 3`).
///
/// # Safety
///
/// `opt` is an option `nd6_options` found in a buffer that is still alive and unchanged
/// (its whole length lies inside that buffer).
pub(crate) unsafe fn nd6_opt_lladdr<'a>(opt: NonNull<NdOptHdr>) -> (&'a [u8], i32) {
    // SAFETY: the caller's contract: the option's `nd_opt_len << 3` bytes (at least 8) are
    // inside the buffer; `NdOptHdr` has alignment 1.
    unsafe {
        let len = usize::from(opt.as_ref().nd_opt_len) << 3;
        let data = opt.as_ptr().cast::<u8>().add(size_of::<NdOptHdr>());
        (
            core::slice::from_raw_parts(data, len - size_of::<NdOptHdr>()),
            len as i32,
        )
    }
}

/// `nd6_llinfo_settimer`: ND6 timer routine to handle ND6 entries: (re)arms the neighbor
/// cache timer of `ln` in `secs` seconds.
pub fn nd6_llinfo_settimer(ln: &LlinfoNd6, secs: u32) {
    let expire = getuptime() + Time::from(secs);

    net_assert_locked("nd6_llinfo_settimer");
    let Some(rt) = ln.ln_rt.get() else {
        panic(format_args!("nd6_llinfo_settimer: no route"));
    };
    kassert!(rt.rt_flags.get() & RTF_LOCAL == 0);

    rt.rt_expire().set(expire);
    if !timeout_pending(&ND6_TIMER_TO) || expire < ND6_TIMER_NEXT.load(Ordering::Relaxed) {
        ND6_TIMER_NEXT.store(expire, Ordering::Relaxed);
        let _ = timeout_add_sec(&ND6_TIMER_TO, secs as i32);
    }
}

/// `nd6_iterator`: the entry after `ln` (or the first, without `ln`) that has a route, with
/// a reference; `iter` marks the position in `nd6_list` while `nd6_mtx` is released.
fn nd6_iterator(ln: Option<&'static LlinfoNd6>, iter: &LlinfoNd6) -> Option<&'static LlinfoNd6> {
    mutex_assert_locked(&ND6_MTX, "nd6_iterator");

    let mut tmp = match ln {
        Some(_) => TailqHead::<LlinfoNd6List>::next(iter),
        None => ND6_LIST.0.first(),
    };

    while let Some(t) = tmp {
        if t.ln_rt.get().is_some() {
            break;
        }
        tmp = TailqHead::<LlinfoNd6List>::next(t);
    }
    // SAFETY: an entry on `nd6_list` lives while referenced (taken below, under `nd6_mtx`).
    let tmp: Option<&'static LlinfoNd6> = tmp.map(|t| unsafe { &*ptr::from_ref(t) });

    if let Some(ln) = ln {
        // SAFETY: the iterator is on the list (inserted below by the previous call).
        unsafe { ND6_LIST.0.remove(iter) };
        if refcnt_rele(&ln.ln_refcnt) {
            ln_put(ln);
        }
    }
    if let Some(t) = tmp {
        // SAFETY: `t` is on the list; the iterator is on no list and stays in place until
        // the walk that owns it ends, which unlinks it.
        unsafe { ND6_LIST.0.insert_after(t, iter) };
        refcnt_take(&t.ln_refcnt);
    }

    tmp
}

/// `nd6_timer`: the neighbor cache timeout: runs the state machine of every entry whose
/// timer expired, then rearms itself for the next one.
fn nd6_timer(_unused: *mut c_void) {
    let iter = LlinfoNd6::iterator();
    let mut ln = None;
    let i_am_router = IP6_FORWARDING.load(Ordering::Relaxed) != 0;

    let uptime = getuptime();
    let mut expire = uptime + Time::from(ND6_GCTIMER);

    mtx_enter(&ND6_MTX);
    loop {
        ln = nd6_iterator(ln, &iter);
        let Some(l) = ln else {
            break;
        };
        let Some(rt) = l.ln_rt.get() else {
            continue;
        };

        let e = rt.rt_expire().get();
        if e != 0 && e <= uptime {
            rtref(rt);
            mtx_leave(&ND6_MTX);
            net_lock();
            if !nd6_llinfo_timer(rt, i_am_router) {
                let e = rt.rt_expire().get();
                if e != 0 && e < expire {
                    expire = e;
                }
            }
            net_unlock();
            rtfree(Some(rt));
            mtx_enter(&ND6_MTX);
        } else if e != 0 && e < expire {
            expire = e;
        }
    }
    mtx_leave(&ND6_MTX);

    let secs = (expire - uptime).max(1) as i32;

    net_lock();
    if !ND6_LIST.0.is_empty() {
        ND6_TIMER_NEXT.store(uptime + Time::from(secs), Ordering::Relaxed);
        let _ = timeout_add_sec(&ND6_TIMER_TO, secs);
    }
    net_unlock();
}

/// `nd6_llinfo_timer`: ND timer state handling. Returns `true` if `rt` should no longer be
/// used.
fn nd6_llinfo_timer(rt: &'static Rtentry, i_am_router: bool) -> bool {
    net_assert_locked_exclusive("nd6_llinfo_timer");

    // might have been freed between leave nd6_mtx and enter net lock
    if rt.rt_flags.get() & RTF_LLINFO == 0 {
        return false;
    }
    let Some(ln) = rt_ln(rt) else {
        return false;
    };
    let dst = rt_key_in6(rt);

    let Some(ifp) = if_get(rt.rt_ifidx.get()) else {
        return true;
    };

    let mut freed = false;
    match ln.ln_state.get() {
        ND6_LLINFO_INCOMPLETE => {
            if ln.ln_asked.get() < i64::from(ND6_MMAXTRIES.load(Ordering::Relaxed)) {
                ln.ln_asked.set(ln.ln_asked.get() + 1);
                nd6_llinfo_settimer(ln, RETRANS_TIMER / 1000);
                let saddr6 = ln.ln_saddr6.get();
                nd6_ns_output(ifp, None, &dst, Some(&saddr6), false);
            } else {
                let ml = MbufList::new();
                mq_delist(&ln.ln_mq, &ml);
                let len = ml_len(&ml);
                while let Some(m) = ml_dequeue(&ml) {
                    // Fake rcvif to make the ICMP error more helpful in diagnosing for the
                    // receiver.
                    // XXX: should we consider older rcvif?
                    m.m_pkthdr().ph_ifidx.set(rt.rt_ifidx.get());

                    icmp6_error(m, ICMP6_DST_UNREACH, ICMP6_DST_UNREACH_ADDR, 0);
                }

                // XXXSMP we also discard if other CPU enqueues
                if mq_len(&ln.ln_mq) > 0 {
                    // mbuf is back in queue. Discard.
                    ln_hold_total.fetch_sub(len + mq_purge(&ln.ln_mq), Ordering::Relaxed);
                } else {
                    ln_hold_total.fetch_sub(len, Ordering::Relaxed);
                }

                nd6_free(rt, ifp, i_am_router);
                freed = true;
            }
        }

        ND6_LLINFO_REACHABLE => {
            if !nd6_llinfo_permanent(ln) {
                ln.ln_state.set(ND6_LLINFO_STALE);
                nd6_llinfo_settimer(ln, ND6_GCTIMER as u32);
            }
        }

        ND6_LLINFO_STALE | ND6_LLINFO_PURGE => {
            // Garbage Collection(RFC 2461 5.3)
            if !nd6_llinfo_permanent(ln) {
                nd6_free(rt, ifp, i_am_router);
                freed = true;
            }
        }

        ND6_LLINFO_DELAY => {
            // We need NUD
            ln.ln_asked.set(1);
            ln.ln_state.set(ND6_LLINFO_PROBE);
            nd6_llinfo_settimer(ln, RETRANS_TIMER / 1000);
            let saddr6 = ln.ln_saddr6.get();
            nd6_ns_output(ifp, Some(&dst), &dst, Some(&saddr6), false);
        }

        ND6_LLINFO_PROBE => {
            if ln.ln_asked.get() < i64::from(ND6_UMAXTRIES.load(Ordering::Relaxed)) {
                ln.ln_asked.set(ln.ln_asked.get() + 1);
                nd6_llinfo_settimer(ln, RETRANS_TIMER / 1000);
                let saddr6 = ln.ln_saddr6.get();
                nd6_ns_output(ifp, Some(&dst), &dst, Some(&saddr6), false);
            } else {
                nd6_free(rt, ifp, i_am_router);
                freed = true;
            }
        }

        _ => {}
    }

    if_put(ifp);

    freed
}

/// `nd6_expire_timer_update`: rearms the address lifetime timer for `ia6`.
pub fn nd6_expire_timer_update(ia6: &In6Ifaddr) {
    let lt = ia6.ia6_lifetime.get();
    let mut expire_time = Time::MAX;

    if lt.ia6t_vltime != ND6_INFINITE_LIFETIME {
        expire_time = lt.ia6t_expire;
    }

    if ia6.ia6_flags.get() & IN6_IFF_DEPRECATED == 0
        && lt.ia6t_pltime != ND6_INFINITE_LIFETIME
        && expire_time > lt.ia6t_preferred
    {
        expire_time = lt.ia6t_preferred;
    }

    if expire_time == Time::MAX {
        return;
    }

    // IFA6_IS_INVALID() and IFA6_IS_DEPRECATED() check for uptime greater than ia6t_expire
    // or ia6t_preferred, not greater or equal. Schedule timeout one second later so that
    // either IFA6_IS_INVALID() or IFA6_IS_DEPRECATED() is true.
    expire_time += 1;

    if !timeout_pending(&ND6_EXPIRE_TIMEOUT)
        || ND6_EXPIRE_NEXT.load(Ordering::Relaxed) > expire_time
    {
        let secs = (expire_time - getuptime()).max(0) as i32;

        let _ = timeout_add_sec(&ND6_EXPIRE_TIMEOUT, secs);
        ND6_EXPIRE_NEXT.store(expire_time, Ordering::Relaxed);
    }
}

/// `nd6_expire`: expire interface addresses.
fn nd6_expire(_unused: *mut c_void) {
    net_lock();

    for ifp in IFNETLIST.0.iter() {
        for ifa in ifp.if_addrlist.iter() {
            if ifa_family(ifa) != AF_INET6 {
                continue;
            }
            let ia6 = ifatoia6(ifa_static(ifa));
            // check address lifetime
            if ifa6_is_invalid(ia6) {
                in6_purgeaddr(&ia6.ia_ifa);
            } else {
                if ifa6_is_deprecated(ia6) {
                    ia6.ia6_flags.set(ia6.ia6_flags.get() | IN6_IFF_DEPRECATED);
                }
                nd6_expire_timer_update(ia6);
            }
        }
    }

    net_unlock();
}

/// `nd6_expire_timer`: the address lifetime timeout: runs `nd6_expire` on the network
/// task queue.
fn nd6_expire_timer(_unused: *mut c_void) {
    if let Some(tq) = net_tq(0) {
        let _ = task_add(tq, &ND6_EXPIRE_TASK);
    }
}

/// `nd6_purge`: nuke neighbor cache/prefix/default router management table, right before
/// `ifp` goes away.
pub fn nd6_purge(ifp: &Ifnet) {
    let iter = LlinfoNd6::iterator();
    let mut ln = None;
    let i_am_router = IP6_FORWARDING.load(Ordering::Relaxed) != 0;

    // Nuke neighbor cache entries for the ifp.
    mtx_enter(&ND6_MTX);
    loop {
        ln = nd6_iterator(ln, &iter);
        let Some(l) = ln else {
            break;
        };
        let Some(rt) = l.ln_rt.get() else {
            continue;
        };
        let gate = rt.rt_gateway.get();
        // SAFETY: a route's gateway is NULL or a readable socket address; an `AF_LINK` one
        // is a `sockaddr_dl`.
        let ours = !gate.is_null()
            && unsafe {
                (*gate).sa_family == AF_LINK
                    && u32::from((*satosdl(gate)).sdl_index) == ifp.if_index.get()
            };
        if ours {
            rtref(rt);
            mtx_leave(&ND6_MTX);
            nd6_free(rt, ifp, i_am_router);
            rtfree(Some(rt));
            mtx_enter(&ND6_MTX);
        }
    }
    mtx_leave(&ND6_MTX);
}

/// `nd6_lookup`: the neighbor cache entry of `addr6` (on `ifp` when given) in `rtableid`;
/// with `create`, a host route made for it when there is none.
pub fn nd6_lookup(
    addr6: &In6Addr,
    create: bool,
    ifp: Option<&Ifnet>,
    rtableid: u32,
) -> Option<&'static Rtentry> {
    let sin6 = SockaddrIn6::with_addr(*addr6);
    let flags = if create { RT_RESOLVE } else { 0 };

    // SAFETY: a local `sockaddr_in6`.
    let mut rt = unsafe { rtalloc(sin6tosa_const(&sin6), flags, rtableid) };
    if let Some(r) = rt
        && r.rt_flags.get() & RTF_LLINFO == 0
    {
        // This is the case for the default route. If we want to create a neighbor cache
        // for the address, we should free the route for the destination and allocate an
        // interface route.
        if create {
            rtfree(rt);
            rt = None;
        }
    }
    let rt = match rt {
        Some(r) => r,
        None => {
            let ifp = ifp.filter(|_| create)?;
            // If no route is available and create is set, we allocate a host route for
            // the destination and treat it like an interface route. This hack is
            // necessary for a neighbor which can't be covered by our own prefix.
            // SAFETY: a local `sockaddr_in6`.
            let ifa = unsafe { ifaof_ifpforaddr(sin6tosa_const(&sin6), ifp) }?;

            // Create a new route. RTF_LLINFO is necessary to create a Neighbor Cache entry
            // for the destination in nd6_rtrequest which will be called in rtrequest.
            let mut info = RtAddrinfo::new();
            info.rti_ifa = Some(ifa);
            info.rti_flags = RTF_HOST | RTF_LLINFO;
            info.rti_info[RTAX_DST] = sin6tosa_const(&sin6);
            info.rti_info[RTAX_GATEWAY] = sdltosa(ifp.if_sadl.get());
            let mut nrt = None;
            // SAFETY: the destination is a local `sockaddr_in6` and the gateway the
            // interface's `sockaddr_dl`, both readable for their lengths.
            if unsafe { rtrequest(RTM_ADD, &mut info, RTP_CONNECTED, Some(&mut nrt), rtableid) }
                .is_err()
            {
                return None;
            }
            let r = nrt?;
            mtx_enter(&ND6_MTX);
            if let Some(ln) = rt_ln(r) {
                ln.ln_state.set(ND6_LLINFO_NOSTATE);
            }
            mtx_leave(&ND6_MTX);
            r
        }
    };
    // Validation for the entry. Note that the check for rt_llinfo is necessary because a
    // cloned route from a parent route that has the L flag (e.g. the default route to a
    // p2p interface) may have the flag, too, while the destination is not actually a
    // neighbor.
    let gate = rt.rt_gateway.get();
    if rt.rt_flags.get() & RTF_GATEWAY != 0
        || rt.rt_flags.get() & RTF_LLINFO == 0
        // SAFETY: a route's gateway is a readable socket address.
        || unsafe { (*gate).sa_family } != AF_LINK
        || rt.rt_llinfo.get().is_null()
        || ifp.is_some_and(|ifp| rt.rt_ifidx.get() != ifp.if_index.get())
    {
        rtfree(Some(rt));
        return None;
    }
    Some(rt)
}

/// `nd6_is_addr_neighbor`: whether a given IPv6 address identifies a neighbor on a given
/// link. XXX: should take care of the destination of a p2p link?
pub fn nd6_is_addr_neighbor(addr: &SockaddrIn6, ifp: &Ifnet) -> bool {
    // A link-local address is always a neighbor.
    // XXX: we should use the sin6_scope_id field rather than the embedded interface index.
    // XXX: a link does not necessarily specify a single interface.
    if in6_is_addr_linklocal(&addr.sin6_addr)
        && u32::from(ntohs(addr.sin6_addr.s6_addr16(1))) == ifp.if_index.get()
    {
        return true;
    }

    for ifa in ifp.if_addrlist.iter() {
        if ifa_family(ifa) != AF_INET6 {
            continue;
        }

        let ia6 = ifatoia6(ifa);

        // Prefix check down below.
        if ia6.ia6_flags.get() & IN6_IFF_AUTOCONF != 0 {
            continue;
        }

        if in6_are_masked_addr_equal(
            &addr.sin6_addr,
            &ia6.ia_addr.get().sin6_addr,
            &ia6.ia_prefixmask.get().sin6_addr,
        ) {
            return true;
        }
    }

    // Even if the address matches none of our addresses, it might be in the neighbor
    // cache.
    let rt = nd6_lookup(&addr.sin6_addr, false, Some(ifp), ifp.if_rdomain.get());
    if rt.is_some() {
        rtfree(rt);
        return true;
    }

    false
}

/// `nd6_invalidate`: forgets the link-layer address of neighbor route `rt` and its held
/// packets; the entry goes back to INCOMPLETE.
fn nd6_invalidate(rt: &Rtentry) {
    let sdl = satosdl(rt.rt_gateway.get());

    mtx_enter(&ND6_MTX);
    let Some(ln) = rt_ln(rt) else {
        mtx_leave(&ND6_MTX);
        return;
    };
    ln_hold_total.fetch_sub(mq_purge(&ln.ln_mq), Ordering::Relaxed);
    // SAFETY: a neighbor route's gateway is a `sockaddr_dl` (`nd6_rtrequest` checked it).
    unsafe { (*sdl).sdl_alen = 0 };
    ln.ln_state.set(ND6_LLINFO_INCOMPLETE);
    ln.ln_asked.set(0);
    mtx_leave(&ND6_MTX);
}

/// `nd6_free`: free an nd6 llinfo entry: forgets the neighbor and deletes its route.
fn nd6_free(rt: &'static Rtentry, ifp: &Ifnet, i_am_router: bool) {
    let ln = rt_ln(rt);
    let in6 = rt_key_in6(rt);

    net_assert_locked_exclusive("nd6_free");

    if !i_am_router && ln.is_some_and(|ln| ln.ln_router.get() != 0) {
        // rt6_flush must be called whether or not the neighbor is in the Default Router
        // List. See a corresponding comment in nd6_na_input().
        let _ = rt6_flush(&in6, ifp);
    }

    kassert!(rt.rt_flags.get() & RTF_LOCAL == 0);
    nd6_invalidate(rt);

    // Detach the route from the routing tree and the list of neighbor caches, and disable
    // the route entry not to be used in already cached routes.
    if rt.rt_flags.get() & (RTF_STATIC | RTF_CACHED) == 0 {
        let _ = rtdeletemsg(rt, ifp, ifp.if_rdomain.get());
    }
}

/// The solicited-node multicast address of `addr` with the interface index of `ifp`
/// embedded (`ff02:<ifidx>::1:ffXX:XXXX`), as `nd6_rtrequest` computes it inline.
fn nd6_llsol(addr: &In6Addr, ifp: &Ifnet) -> In6Addr {
    let mut llsol = *addr;
    llsol.set_s6_addr16(0, htons(0xff02));
    llsol.set_s6_addr16(1, htons(ifp.if_index.get() as u16));
    llsol.set_s6_addr32(1, 0);
    llsol.set_s6_addr32(2, htonl(1));
    llsol.s6_addr[12] = 0xff;
    llsol
}

/// `nd6_rtrequest`: the `AF_INET6` part of `ether_rtrequest`: sets up and tears down the
/// neighbor cache entries of routes.
pub fn nd6_rtrequest(ifp: &'static Ifnet, req: i32, rt: &'static Rtentry) {
    let gate = rt.rt_gateway.get();

    if rt.rt_flags.get() & (RTF_GATEWAY | RTF_MULTICAST | RTF_MPLS) != 0 {
        return;
    }

    if !nd6_need_cache(ifp) && rt.rt_flags.get() & RTF_HOST == 0 {
        // This is probably an interface direct route for a link which does not need
        // neighbor caches (e.g. fe80::%lo0/64). We do not need special treatment below for
        // such a route. Moreover, the RTF_LLINFO flag which would be set below would annoy
        // the ndp(8) command.
        return;
    }

    let req = req as u8;
    if req == RTM_RESOLVE && !nd6_need_cache(ifp) {
        // For routing daemons like ospf6d we allow neighbor discovery based on the cloning
        // route only. This allows us to send packets directly into a network without
        // having an address with matching prefix on the interface. If the cloning route is
        // used for an 6to4 interface, we would mistakenly make a neighbor cache for the
        // host route, and would see strange neighbor solicitation for the corresponding
        // destination. In order to avoid confusion, we check if the interface is suitable
        // for neighbor discovery, and stop the process if not. Additionally, we remove the
        // LLINFO flag so that ndp(8) will not try to get the neighbor information of the
        // destination.
        rt.rt_flags.set(rt.rt_flags.get() & !RTF_LLINFO);
        return;
    }

    if req == RTM_ADD || req == RTM_RESOLVE {
        if req == RTM_ADD {
            if rt.rt_flags.get() & RTF_CLONING != 0 {
                rt.rt_expire().set(0);
                return;
            }
            if rt.rt_flags.get() & RTF_LOCAL != 0 && rt.rt_llinfo.get().is_null() {
                rt.rt_expire().set(0);
            }
        }
        // FALLTHROUGH (RTM_RESOLVE)
        // SAFETY: a route's gateway is a readable socket address.
        let (gfam, glen) = unsafe { ((*gate).sa_family, usize::from((*gate).sa_len)) };
        if gfam != AF_LINK || glen < size_of::<SockaddrDl>() {
            log!(
                LOG_DEBUG,
                "nd6_rtrequest: bad gateway value: {}\n",
                Str(&ifp.if_xname.get())
            );
            return;
        }
        // SAFETY: an `AF_LINK` gateway of at least a `sockaddr_dl`'s size.
        unsafe {
            (*satosdl(gate)).sdl_type = ifp.if_type.get();
            (*satosdl(gate)).sdl_index = ifp.if_index.get() as u16;
        }
        // Case 2: This route may come from cloning, or a manual route add with a LL
        // address.
        let Some(mem) = pool_get(&ND6_POOL, PR_NOWAIT | PR_ZERO) else {
            log!(LOG_DEBUG, "nd6_rtrequest: pool get failed\n");
            return;
        };
        let lp = mem.as_ptr().cast::<LlinfoNd6>();

        mtx_enter(&ND6_MTX);
        if !rt.rt_llinfo.get().is_null() {
            // we lost the race, another thread has entered it
            mtx_leave(&ND6_MTX);
            pool_put(&ND6_POOL, mem);
            return;
        }
        ND6_INUSE.fetch_add(1, Ordering::Relaxed);
        // SAFETY: a fresh pool item of `size_of::<LlinfoNd6>()` bytes, written whole; it
        // lives until its last reference.
        let ln: &'static LlinfoNd6 = unsafe {
            lp.write(LlinfoNd6::iterator());
            &*lp
        };
        refcnt_init(&ln.ln_refcnt);
        mq_init(&ln.ln_mq, LN_HOLD_QUEUE, IPL_SOFTNET);
        rt.rt_llinfo.set(ptr::from_ref(ln).cast_mut().cast());
        ln.ln_rt.set(Some(rt));
        rt.rt_flags.set(rt.rt_flags.get() | RTF_LLINFO);
        // SAFETY: `ln` is on no list; `nd6_list` is changed under `nd6_mtx`.
        unsafe { ND6_LIST.0.insert_head(ln) };
        // this is required for "ndp" command. - shin
        if req == RTM_ADD {
            // gate should have some valid AF_LINK entry, and ln expire should have some
            // lifetime which is specified by ndp command.
            ln.ln_state.set(ND6_LLINFO_REACHABLE);
        } else {
            // When req == RTM_RESOLVE, rt is created and initialized in rtrequest(), so
            // rt_expire is 0.
            ln.ln_state.set(ND6_LLINFO_NOSTATE);
            nd6_llinfo_settimer(ln, 0);
        }

        // If we have too many cache entries, initiate immediate purging for some "less
        // recently used" entries. Note that we cannot directly call nd6_free() here because
        // it would cause re-entering rtable related routines triggering
        // lock-order-reversal problems.
        if ND6_INUSE.load(Ordering::Relaxed) >= IP6_NEIGHBORGCTHRESH.load(Ordering::Relaxed) {
            for _ in 0..10 {
                let Some(ln_end) = ND6_LIST.0.last() else {
                    break;
                };
                if ptr::eq(ln_end, ln) {
                    break;
                }
                // cannot move the iterator, try next time
                if ln_end.ln_rt.get().is_none() {
                    break;
                }

                // Move this entry to the head
                // SAFETY: `ln_end` is on `nd6_list`, under `nd6_mtx`.
                unsafe {
                    ND6_LIST.0.remove(ln_end);
                    ND6_LIST.0.insert_head(ln_end);
                }

                if nd6_llinfo_permanent(ln_end) {
                    continue;
                }

                if ln_end.ln_state.get() > ND6_LLINFO_INCOMPLETE {
                    ln_end.ln_state.set(ND6_LLINFO_STALE);
                } else {
                    ln_end.ln_state.set(ND6_LLINFO_PURGE);
                }
                nd6_llinfo_settimer(ln_end, 0);
            }
        }

        // check if rt_key(rt) is one of my address assigned to the interface.
        let key = rt_key_in6(rt);
        let ifa6 = in6ifa_ifpwithaddr(ifp, &key);
        if ifa6.is_some() || rt.rt_flags.get() & RTF_ANNOUNCE != 0 {
            ln.ln_state.set(ND6_LLINFO_REACHABLE);
            rt.rt_expire().set(0);
        }
        mtx_leave(&ND6_MTX);

        // join solicited node multicast for proxy ND
        if ifa6.is_none()
            && rt.rt_flags.get() & RTF_ANNOUNCE != 0
            && ifp.if_flags.get() & IFF_MULTICAST != 0
        {
            let llsol = nd6_llsol(&key, ifp);

            // KERNEL_LOCK(): no kernel lock without MULTIPROCESSOR.
            let _ = in6_addmulti(&llsol, ifp);
        }
    } else if req == RTM_DELETE {
        mtx_enter(&ND6_MTX);
        let Some(ln) = rt_ln(rt) else {
            // we lost the race, another thread has removed it
            mtx_leave(&ND6_MTX);
            return;
        };
        ND6_INUSE.fetch_sub(1, Ordering::Relaxed);
        // SAFETY: the entry is on `nd6_list`, under `nd6_mtx`.
        unsafe { ND6_LIST.0.remove(ln) };
        rt.rt_expire().set(0);
        rt.rt_llinfo.set(ptr::null_mut());
        rt.rt_flags.set(rt.rt_flags.get() & !RTF_LLINFO);
        ln_hold_total.fetch_sub(mq_purge(&ln.ln_mq), Ordering::Relaxed);
        mtx_leave(&ND6_MTX);

        if refcnt_rele(&ln.ln_refcnt) {
            ln_put(ln);
        }

        // leave from solicited node multicast for proxy ND
        if rt.rt_flags.get() & RTF_ANNOUNCE != 0 && ifp.if_flags.get() & IFF_MULTICAST != 0 {
            let llsol = nd6_llsol(&rt_key_in6(rt), ifp);

            // KERNEL_LOCK(): no kernel lock without MULTIPROCESSOR.
            if let Some(in6m) = in6_lookupmulti(&llsol, ifp) {
                in6_delmulti(in6m);
            }
        }
    } else if req == RTM_INVALIDATE && rt.rt_flags.get() & RTF_LOCAL == 0 {
        nd6_invalidate(rt);
    }
}

/// `nd6_ioctl`: `SIOCGIFINFO_IN6` and `SIOCGNBRINFO_IN6`.
///
/// # Safety
///
/// `data` points at the kernel copy of the request (`struct in6_ndireq` or
/// `struct in6_nbrinfo`), readable, writable and aligned for it.
pub unsafe fn nd6_ioctl(cmd: u64, data: *mut u8, ifp: &'static Ifnet) -> Result<(), Errno> {
    let ndi = data.cast::<In6Ndireq>();
    let nbi = data.cast::<In6Nbrinfo>();

    match cmd {
        SIOCGIFINFO_IN6 => {
            net_lock_shared();
            if let Some(nd) = if_nd(ifp) {
                // SAFETY: the caller's contract.
                unsafe { (*ndi).ndi = nd.get() };
            }
            net_unlock_shared();
            Ok(())
        }
        SIOCGNBRINFO_IN6 => {
            // SAFETY: the caller's contract.
            let mut nb_addr = unsafe { (*nbi).addr }; // make local for safety

            net_lock_shared();
            // XXX: KAME specific hack for scoped addresses
            //      XXXX: for other scopes than link-local?
            if (in6_is_addr_linklocal(&nb_addr) || in6_is_addr_mc_linklocal(&nb_addr))
                && nb_addr.s6_addr16(1) == 0
            {
                nb_addr.set_s6_addr16(1, htons(ifp.if_index.get() as u16));
            }

            let rt = nd6_lookup(&nb_addr, false, Some(ifp), ifp.if_rdomain.get());
            mtx_enter(&ND6_MTX);
            let Some(ln) = rt.and_then(rt_ln) else {
                mtx_leave(&ND6_MTX);
                rtfree(rt);
                net_unlock_shared();
                return Err(Errno::EINVAL);
            };
            let mut expire = ln.ln_rt.get().map_or(0, |r| r.rt_expire().get());
            if expire != 0 {
                expire -= getuptime();
                expire += gettime();
            }

            // SAFETY: the caller's contract.
            unsafe {
                (*nbi).state = i32::from(ln.ln_state.get());
                (*nbi).asked = ln.ln_asked.get();
                (*nbi).isrouter = i32::from(ln.ln_router.get());
                (*nbi).expire = expire;
            }
            mtx_leave(&ND6_MTX);

            rtfree(rt);
            net_unlock_shared();
            Ok(())
        }
        _ => Ok(()),
    }
}

/// The first `if_addrlen` bytes of a link-layer address (all of it when shorter).
fn lladdr_bytes<'a>(lladdr: &'a [u8], ifp: &Ifnet) -> &'a [u8] {
    &lladdr[..usize::from(ifp.if_addrlen.get()).min(lladdr.len())]
}

/// A link-layer address as `ether_sprintf` prints it.
pub(crate) fn nd6_ether_sprintf(lladdr: &[u8]) -> [u8; ETHER_ADDR_LEN * 3] {
    let mut ea = [0u8; ETHER_ADDR_LEN];
    let n = lladdr.len().min(ETHER_ADDR_LEN);
    ea[..n].copy_from_slice(&lladdr[..n]);
    ether_sprintf(&ea)
}

/// Whether `ll` differs from the link-layer address in `sdl` (the C's
/// `bcmp(lladdr, LLADDR(sdl), ifp->if_addrlen)`).
///
/// # Safety
///
/// `sdl` is a readable `sockaddr_dl` with room for `if_addrlen` bytes of address.
pub(crate) unsafe fn nd6_llchanged(ll: &[u8], sdl: *mut SockaddrDl, ifp: &Ifnet) -> bool {
    let l = lladdr_bytes(ll, ifp);
    // SAFETY: the caller's contract.
    let cur = unsafe { core::slice::from_raw_parts(lladdr(sdl), l.len()) };
    l != cur
}

/// Records `ll` in `sdl` (the C's `sdl->sdl_alen = ifp->if_addrlen; bcopy(lladdr,
/// LLADDR(sdl), ifp->if_addrlen)`).
///
/// # Safety
///
/// As for [`nd6_llchanged`], and `sdl` is writable.
pub(crate) unsafe fn nd6_llstore(ll: &[u8], sdl: *mut SockaddrDl, ifp: &Ifnet) {
    let l = lladdr_bytes(ll, ifp);
    // SAFETY: the caller's contract.
    unsafe {
        (*sdl).sdl_alen = ifp.if_addrlen.get();
        ptr::copy_nonoverlapping(l.as_ptr(), lladdr(sdl), l.len());
    }
}

/// `nd6_cache_lladdr`: create neighbor cache entry and cache link-layer address, on
/// reception of inbound ND6 packets (RS/RA/NS/redirect). `type_` is the ICMP6 type,
/// `code` the type dependent information, `i_am_router` whether we forward.
pub fn nd6_cache_lladdr(
    ifp: &'static Ifnet,
    from: &In6Addr,
    lladdr: Option<&[u8]>,
    type_: i32,
    code: i32,
    i_am_router: i32,
) {
    net_assert_locked_exclusive("nd6_cache_lladdr");
    let i_am_router = i_am_router != 0;

    // nothing must be updated for unspecified address
    if in6_is_addr_unspecified(from) {
        return;
    }

    // Validation about ifp->if_addrlen and lladdrlen must be done in the caller.
    //
    // XXX If the link does not have link-layer address, what should we do?
    // (ifp->if_addrlen == 0) Spec says nothing in sections for RA, RS and NA. There's
    // small description on it in NS section (RFC 2461 7.2.3).

    let (rt, is_newentry) = match nd6_lookup(from, false, Some(ifp), ifp.if_rdomain.get()) {
        None => (
            nd6_lookup(from, true, Some(ifp), ifp.if_rdomain.get()),
            true,
        ),
        Some(rt) => {
            // do not overwrite local or static entry
            if rt.rt_flags.get() & (RTF_STATIC | RTF_LOCAL) != 0 {
                rtfree(Some(rt));
                return;
            }
            (Some(rt), false)
        }
    };

    let Some(rt) = rt else {
        return;
    };
    let gate = rt.rt_gateway.get();
    let ln = rt_ln(rt);
    let ok = rt.rt_flags.get() & (RTF_GATEWAY | RTF_LLINFO) == RTF_LLINFO
        && ln.is_some()
        && !gate.is_null()
        // SAFETY: a non-NULL gateway is a readable socket address.
        && unsafe { (*gate).sa_family } == AF_LINK;
    let Some(ln) = ln.filter(|_| ok) else {
        // fail:
        nd6_free(rt, ifp, i_am_router);
        rtfree(Some(rt));
        return;
    };
    let sdl = satosdl(gate);

    // SAFETY: an `AF_LINK` gateway of a neighbor route is a `sockaddr_dl` with room for the
    // interface's address (`nd6_rtrequest` checked its size).
    let olladdr = unsafe { (*sdl).sdl_alen } != 0;
    let llchange = match lladdr {
        // SAFETY: as above.
        Some(l) if olladdr => unsafe { nd6_llchanged(l, sdl, ifp) },
        _ => false,
    };

    // newentry olladdr  lladdr  llchange	(*=record)
    //	0	n	n	--	(1)
    //	0	y	n	--	(2)
    //	0	n	y	--	(3) * STALE
    //	0	y	y	n	(4) *
    //	0	y	y	y	(5) * STALE
    //	1	--	n	--	(6)   NOSTATE(= PASSIVE)
    //	1	--	y	--	(7) * STALE

    if llchange && let Some(l) = lladdr {
        let s = nd6_ether_sprintf(l);
        log!(
            LOG_INFO,
            "ndp info overwritten for {} by {} on {}\n",
            In6Ntop(*from),
            Str(&s),
            Str(&ifp.if_xname.get())
        );
    }
    if let Some(l) = lladdr {
        // (3-5) and (7)
        // Record source link-layer address
        // XXX is it dependent to ifp->if_type?
        // SAFETY: as above.
        unsafe { nd6_llstore(l, sdl, ifp) };
    }

    let (do_update, newstate) = if !is_newentry {
        if (!olladdr && lladdr.is_some()) // (3)
            || (olladdr && lladdr.is_some() && llchange)
        // (5)
        {
            (true, ND6_LLINFO_STALE)
        } else {
            // (1-2,4)
            (false, ND6_LLINFO_INCOMPLETE)
        }
    } else if lladdr.is_none() {
        // (6)
        (true, ND6_LLINFO_NOSTATE)
    } else {
        // (7)
        (true, ND6_LLINFO_STALE)
    };

    if do_update {
        // Update the state of the neighbor cache.
        ln.ln_state.set(newstate);

        if ln.ln_state.get() == ND6_LLINFO_STALE {
            // Since nd6_resolve() in ifp->if_output() will cause state transition to DELAY
            // and reset the timer, we must set the timer now, although it is actually
            // meaningless.
            nd6_llinfo_settimer(ln, ND6_GCTIMER as u32);
            // SAFETY: the route's key is its readable `sockaddr_in6`.
            let _ = unsafe { if_output_mq(ifp, &ln.ln_mq, &ln_hold_total, rt_key(rt), Some(rt)) };
        } else if ln.ln_state.get() == ND6_LLINFO_INCOMPLETE {
            // probe right away
            nd6_llinfo_settimer(ln, 0);
        }
    }

    // ICMP6 type dependent behavior.
    //
    // NS: clear IsRouter if new entry
    // RS: clear IsRouter
    // RA: set IsRouter if there's lladdr
    // redir: clear IsRouter if new entry
    //
    // RA case, (1):
    // The spec says that we must set IsRouter in the following cases:
    // - If lladdr exist, set IsRouter.  This means (1-5).
    // - If it is old entry (!newentry), set IsRouter.  This means (7).
    // So, based on the spec, in (1-5) and (7) cases we must set IsRouter.
    // A question arises for (1) case.  (1) case has no lladdr in the neighbor cache, this is
    // similar to (6). This case is rare but we figured that we MUST NOT set IsRouter.
    //
    // newentry olladdr  lladdr  llchange	    NS  RS  RA	redir
    //							D R
    //	0	n	n	--	(1)	c   ?     s
    //	0	y	n	--	(2)	c   s     s
    //	0	n	y	--	(3)	c   s     s
    //	0	y	y	n	(4)	c   s     s
    //	0	y	y	y	(5)	c   s     s
    //	1	--	n	--	(6) c	c	c s
    //	1	--	y	--	(7) c	c   s	c s
    //
    //					(c=clear s=set)
    match (type_ & 0xff) as u8 {
        ND_NEIGHBOR_SOLICIT => {
            // New entry must have is_router flag cleared.
            if is_newentry {
                // (6-7)
                ln.ln_router.set(0);
            }
        }
        ND_REDIRECT => {
            // If the icmp is a redirect to a better router, always set the is_router flag.
            // Otherwise, if the entry is newly created, clear the flag. [RFC 2461, sec 8.3]
            if code == i32::from(ND_REDIRECT_ROUTER) {
                ln.ln_router.set(1);
            } else if is_newentry {
                // (6-7)
                ln.ln_router.set(0);
            }
        }
        ND_ROUTER_SOLICIT => {
            // is_router flag must always be cleared.
            ln.ln_router.set(0);
        }
        // Mark an entry with lladdr as a router: (2-5) and (7).
        ND_ROUTER_ADVERT
            if (!is_newentry && (olladdr || lladdr.is_some()))
                || (is_newentry && lladdr.is_some()) =>
        {
            ln.ln_router.set(1);
        }
        _ => {}
    }

    rtfree(Some(rt));
}

/// `nd6_slowtimo`: recomputes the random Reachable Time of every interface every few
/// hours.
fn nd6_slowtimo(_ignored_arg: *mut c_void) {
    net_lock();

    let _ = timeout_add_sec(&ND6_SLOWTIMO_CH, ND6_SLOWTIMER_INTERVAL);

    for ifp in IFNETLIST.0.iter() {
        let Some(nd6if) = if_nd(ifp) else {
            continue;
        };
        let mut nd = nd6if.get();
        nd.recalctm -= ND6_SLOWTIMER_INTERVAL;
        if nd.recalctm <= 0 {
            // Since reachable time rarely changes by router advertisements, we SHOULD
            // insure that a new random value gets recomputed at least once every few
            // hours. (RFC 2461, 6.3.4)
            nd.recalctm = ND6_RECALC_REACHTM_INTERVAL;
            nd.reachable = nd_compute_rtime(REACHABLE_TIME);
        }
        nd6if.set(nd);
    }
    net_unlock();
}

/// `nd6_resolve`: the link-layer address of `dst` for `m` in `desten`; `Err(EAGAIN)` when
/// the packet was queued until the neighbor answers, any other error when it was freed.
///
/// # Safety
///
/// `dst` points at a readable `sockaddr_in6`.
pub unsafe fn nd6_resolve(
    ifp: &'static Ifnet,
    rt0: Option<&'static Rtentry>,
    m: &'static Mbuf,
    dst: *const Sockaddr,
    desten: &mut [u8; ETHER_ADDR_LEN],
) -> Result<(), Errno> {
    // SAFETY: the caller's contract; a generic socket address may sit at any alignment.
    let dst6 = unsafe { ptr::read_unaligned(satosin6_const(dst)) }.sin6_addr;

    if m.m_flags().get() & M_MCAST != 0 {
        *desten = ether_map_ipv6_multicast(&dst6.s6_addr);
        return Ok(());
    }

    let uptime = getuptime();
    let rt = rt0.and_then(rt_getll);

    let Some(rt) = rt.filter(|r| {
        !(r.rt_flags.get() & RTF_REJECT != 0
            && (r.rt_expire().get() == 0 || r.rt_expire().get() > uptime))
    }) else {
        m_freem(m);
        let same = match (rt, rt0) {
            (Some(a), Some(b)) => ptr::eq(a, b),
            (None, None) => true,
            _ => false,
        };
        return Err(if same {
            Errno::EHOSTDOWN
        } else {
            Errno::EHOSTUNREACH
        });
    };

    'bad: {
        // Address resolution or Neighbor Unreachability Detection for the next hop. At
        // this point, the destination of the packet must be a unicast or an anycast
        // address(i.e. not a multicast).
        if rt.rt_flags.get() & RTF_LLINFO == 0 {
            log!(
                LOG_DEBUG,
                "nd6_resolve: {}: route contains no ND information\n",
                In6Ntop(rt_key_in6(rt))
            );
            break 'bad;
        }

        // SAFETY: a route's gateway is a readable socket address.
        if unsafe { (*rt.rt_gateway.get()).sa_family } != AF_LINK {
            printf(format_args!("nd6_resolve: something odd happens\n"));
            break 'bad;
        }

        mtx_enter(&ND6_MTX);
        let Some(ln) = rt_ln(rt) else {
            mtx_leave(&ND6_MTX);
            break 'bad;
        };

        // Move this entry to the head of the queue so that it is less likely for this
        // entry to be a target of forced garbage collection (see nd6_rtrequest()).
        // SAFETY: the entry is on `nd6_list`, under `nd6_mtx`.
        unsafe {
            ND6_LIST.0.remove(ln);
            ND6_LIST.0.insert_head(ln);
        }

        // The first time we send a packet to a neighbor whose entry is STALE, we have to
        // change the state to DELAY and set a timer to expire in DELAY_FIRST_PROBE_TIME
        // seconds to ensure we do neighbor unreachability detection on expiration.
        // (RFC 2461 7.3.3)
        if ln.ln_state.get() == ND6_LLINFO_STALE {
            ln.ln_asked.set(0);
            ln.ln_state.set(ND6_LLINFO_DELAY);
            nd6_llinfo_settimer(ln, ND6_DELAY.load(Ordering::Relaxed) as u32);
        }

        // If the neighbor cache entry has a state other than INCOMPLETE (i.e. its
        // link-layer address is already resolved), just send the packet.
        if ln.ln_state.get() > ND6_LLINFO_INCOMPLETE {
            mtx_leave(&ND6_MTX);

            let sdl = satosdl(rt.rt_gateway.get());
            // SAFETY: an `AF_LINK` gateway is a `sockaddr_dl`.
            let alen = unsafe { (*sdl).sdl_alen };
            if usize::from(alen) != ETHER_ADDR_LEN {
                log!(
                    LOG_DEBUG,
                    "nd6_resolve: {}: incorrect nd6 information\n",
                    In6Ntop(dst6)
                );
                break 'bad;
            }

            // SAFETY: the link address is `alen` (6) bytes after the name.
            unsafe { ptr::copy_nonoverlapping(lladdr(sdl), desten.as_mut_ptr(), ETHER_ADDR_LEN) };
            return Ok(());
        }

        // There is a neighbor cache entry, but no ethernet address response yet. Insert
        // mbuf in hold queue if below limit. If above the limit free the queue without
        // queuing the new packet.
        if ln.ln_state.get() == ND6_LLINFO_NOSTATE {
            ln.ln_state.set(ND6_LLINFO_INCOMPLETE);
        }
        // source address of prompting packet is needed by nd6_ns_output()
        if m.m_len().get() as usize >= size_of::<Ip6Hdr>() {
            ln.ln_saddr6.set(mtod_ip6(m).ip6_src);
        }
        if ln_hold_total.fetch_add(1, Ordering::Relaxed) < LN_HOLD_TOTAL {
            if mq_push(&ln.ln_mq, m) {
                ln_hold_total.fetch_sub(1, Ordering::Relaxed);
            }
        } else {
            ln_hold_total.fetch_sub(mq_purge(&ln.ln_mq) + 1, Ordering::Relaxed);
            m_freem(m);
        }

        // If there has been no NS for the neighbor after entering the INCOMPLETE state,
        // send the first solicitation.
        let mut solicit = None;
        if !nd6_llinfo_permanent(ln) && ln.ln_asked.get() == 0 {
            ln.ln_asked.set(ln.ln_asked.get() + 1);
            nd6_llinfo_settimer(ln, RETRANS_TIMER / 1000);
            solicit = Some(ln.ln_saddr6.get());
        }
        mtx_leave(&ND6_MTX);

        if let Some(saddr6) = solicit {
            nd6_ns_output(ifp, None, &dst6, Some(&saddr6), false);
        }
        return Err(Errno::EAGAIN);
    }
    // bad:
    m_freem(m);
    Err(Errno::EINVAL)
}

/// `nd6_need_cache`: whether `ifp` needs a neighbor cache. RFC2893 says: unidirectional
/// tunnels needs no ND.
pub fn nd6_need_cache(ifp: &Ifnet) -> bool {
    matches!(ifp.if_type.get(), IFT_ETHER | IFT_IEEE80211 | IFT_CARP)
}

// LP64 sizes of the user-visible structures.
const _: () = {
    assert!(size_of::<NdIfinfo>() == 8);
    assert!(size_of::<In6Nbrinfo>() == 56);
    assert!(size_of::<In6Ndireq>() == 24);
};

#[cfg(test)]
pub(crate) mod tests;
