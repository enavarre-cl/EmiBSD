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
//! ## Deviations
//! - `struct ifnet`'s `if_nd` (`struct nd_ifinfo *`) is `Cell<Option<NonNull<NdIfinfo>>>`
//!   (`net/if_var.rs`); [`if_nd`] reads it as the `Cell<NdIfinfo>` the net lock guards.
//!   `NdIfinfo` itself is plain data, the layout `SIOCGIFINFO_IN6` copies out in
//!   `struct in6_ndireq`.
//! - `struct llinfo_nd6` is [`LlinfoNd6`] with `Cell` members; `struct llinfo_nd6_iterator`
//!   is an `LlinfoNd6` without a route, as `if_ether.rs` does for `llinfo_arp`.
//! - `struct nd_opts` holds `Option<NonNull<NdOptHdr>>`: the options inside the packet.
//! - `ND6_LLINFO_PERMANENT(n)` and `ND_COMPUTE_RTIME(x)` are functions.

use crate::netinet::if_ether::ETHER_ADDR_LEN;
use crate::netinet6::in6::SockaddrIn6;
use crate::netinet6::in6_var::In6Ifaddr;
use crate::sys::errno::Errno;
use crate::sys::mbuf::Mbuf;
use crate::sys::socket::Sockaddr;
use core::cell::Cell;
use core::mem::size_of;
use core::ptr::NonNull;
use core::sync::atomic::{AtomicI32, AtomicU32};

use crate::dev::rnd::arc4random;
use crate::kern::subr_prf::panic;
use crate::net::if_::IFNAMSIZ;
use crate::net::if_var::Ifnet;
use crate::net::route::Rtentry;
use crate::netinet::icmp6::NdOptHdr;
use crate::netinet6::in6::In6Addr;
use crate::queue_adapter;
use crate::sys::mbuf::MbufQueue;
use crate::sys::queue::TailqEntry;
use crate::sys::refcnt::Refcnt;
use crate::sys::types::Time;

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

/// `nd6_delay`: delay first probe time 5 second.
pub static ND6_DELAY: AtomicI32 = AtomicI32::new(5);
/// \[a\] `nd6_umaxtries`: maximum unicast query.
pub static ND6_UMAXTRIES: AtomicI32 = AtomicI32::new(3);
/// \[a\] `nd6_mmaxtries`: maximum multicast query.
pub static ND6_MMAXTRIES: AtomicI32 = AtomicI32::new(3);
/// \[a\] `ln_hold_total`: packets currently in the nd6 queue.
#[allow(non_upper_case_globals)] // the C name; `LN_HOLD_TOTAL` is the limit beside it
pub static ln_hold_total: AtomicU32 = AtomicU32::new(0);

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

/// `nd6_init`: the neighbor cache pool, the expiry task and the ND timers.
pub fn nd6_init() {
    let _ = crate::unported!("nd6_init: placeholder");
}

/// `nd6_ifattach`: allocates the Neighbor Discovery information of `ifp` (`if_nd`).
pub fn nd6_ifattach(ifp: &Ifnet) {
    let _ = ifp;
    let _ = crate::unported!("nd6_ifattach: placeholder");
}

/// `nd6_ifdetach`: frees the Neighbor Discovery information of `ifp`.
pub fn nd6_ifdetach(ifp: &Ifnet) {
    let _ = ifp;
    let _ = crate::unported!("nd6_ifdetach: placeholder");
}

/// `nd6_options`: parses the ND options in `opt` (the rest of the message) into `ndopts`;
/// `false` for an invalid option (the C's -1).
pub fn nd6_options(opt: &[u8], ndopts: &mut NdOpts) -> bool {
    let _ = (opt, ndopts);
    let _ = crate::unported!("nd6_options: placeholder");
    false
}

/// `nd6_llinfo_settimer`: (re)arms the neighbor cache timer of `ln` in `secs` seconds.
pub fn nd6_llinfo_settimer(ln: &LlinfoNd6, secs: u32) {
    let _ = (ln, secs);
    let _ = crate::unported!("nd6_llinfo_settimer: placeholder");
}

/// `nd6_purge`: removes the neighbor cache entries of `ifp`.
pub fn nd6_purge(ifp: &Ifnet) {
    let _ = ifp;
    let _ = crate::unported!("nd6_purge: placeholder");
}

/// `nd6_lookup`: the neighbor cache entry of `addr6` on `ifp` in `rtableid`, created if
/// `create`.
pub fn nd6_lookup(
    addr6: &In6Addr,
    create: bool,
    ifp: Option<&'static Ifnet>,
    rtableid: u32,
) -> Option<&'static Rtentry> {
    let _ = (addr6, create, ifp, rtableid);
    let _ = crate::unported!("nd6_lookup: placeholder");
    None
}

/// `nd6_is_addr_neighbor`: whether `addr` identifies a neighbor on the link of `ifp`.
pub fn nd6_is_addr_neighbor(addr: &SockaddrIn6, ifp: &Ifnet) -> bool {
    let _ = (addr, ifp);
    let _ = crate::unported!("nd6_is_addr_neighbor: placeholder");
    false
}

/// `nd6_rtrequest`: the `AF_INET6` part of `ether_rtrequest`: sets up and tears down the
/// neighbor cache entries of routes.
pub fn nd6_rtrequest(ifp: &'static Ifnet, req: i32, rt: &'static Rtentry) {
    let _ = (ifp, req, rt);
    let _ = crate::unported!("nd6_rtrequest: placeholder");
}

/// `nd6_ioctl`: `SIOCGIFINFO_IN6` and `SIOCGNBRINFO_IN6`.
///
/// # Safety
///
/// `data` points at the kernel copy of the request (`struct in6_ndireq` or
/// `struct in6_nbrinfo`), readable, writable and aligned for it.
pub unsafe fn nd6_ioctl(cmd: u64, data: *mut u8, ifp: &'static Ifnet) -> Result<(), Errno> {
    let _ = (cmd, data, ifp);
    Err(crate::unported!("nd6_ioctl: placeholder"))
}

/// `nd6_cache_lladdr`: creates or updates the neighbor cache entry of `from` with the
/// link-layer address `lladdr` learned from an ND message of `type_`/`code`
/// (`router`: the router flag of an advertisement).
pub fn nd6_cache_lladdr(
    ifp: &'static Ifnet,
    from: &In6Addr,
    lladdr: Option<&[u8]>,
    type_: i32,
    code: i32,
    router: i32,
) {
    let _ = (ifp, from, lladdr, type_, code, router);
    let _ = crate::unported!("nd6_cache_lladdr: placeholder");
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
    let _ = (ifp, rt0, m, dst, desten);
    Err(crate::unported!("nd6_resolve: placeholder"))
}

/// `nd6_need_cache`: whether `ifp` needs a neighbor cache (Ethernet-like links).
pub fn nd6_need_cache(ifp: &Ifnet) -> bool {
    let _ = ifp;
    let _ = crate::unported!("nd6_need_cache: placeholder");
    false
}

/// `nd6_expire_timer_update`: rearms the address lifetime timer for `ia6`.
pub fn nd6_expire_timer_update(ia6: &In6Ifaddr) {
    let _ = ia6;
    let _ = crate::unported!("nd6_expire_timer_update: placeholder");
}

// LP64 sizes of the user-visible structures.
const _: () = {
    assert!(size_of::<NdIfinfo>() == 8);
    assert!(size_of::<In6Nbrinfo>() == 56);
    assert!(size_of::<In6Ndireq>() == 24);
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/netinet6/nd6.h");
        let ll = crate::reftest::assert_defines!(defs;
            ND6_LLINFO_PURGE, ND6_LLINFO_NOSTATE, ND6_LLINFO_INCOMPLETE, ND6_LLINFO_REACHABLE,
            ND6_LLINFO_STALE, ND6_LLINFO_DELAY, ND6_LLINFO_PROBE, ND6_INFINITE_LIFETIME);
        crate::reftest::assert_complete(&defs, "ND6_", &ll);
        crate::reftest::assert_defines!(defs;
            MAX_RTR_SOLICITATION_DELAY, RTR_SOLICITATION_INTERVAL, MAX_RTR_SOLICITATIONS,
            LN_HOLD_QUEUE, LN_HOLD_TOTAL, REACHABLE_TIME, RETRANS_TIMER, MIN_RANDOM_FACTOR,
            MAX_RANDOM_FACTOR);
    }
}
