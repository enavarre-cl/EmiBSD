/*	$OpenBSD: igmp.h,v 1.6 2003/06/02 23:28:13 millert Exp $	*/
/*	$NetBSD: igmp.h,v 1.6 1995/05/31 06:08:21 mycroft Exp $	*/
/*	$OpenBSD: igmp.c,v 1.100 2026/07/30 14:57:46 bluhm Exp $	*/
/*	$NetBSD: igmp.c,v 1.15 1996/02/13 23:41:25 christos Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1988 Stephen Deering.
 * Copyright (c) 1992, 1993
 *	The Regents of the University of California.  All rights reserved.
 *
 * This code is derived from software contributed to Berkeley by
 * Stephen Deering of Stanford University.
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
 *	@(#)igmp.h	8.1 (Berkeley) 6/10/93
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

/*
 * Copyright (c) 1988 Stephen Deering.
 * Copyright (c) 1992, 1993
 *	The Regents of the University of California.  All rights reserved.
 *
 * This code is derived from software contributed to Berkeley by
 * Stephen Deering of Stanford University.
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
 *	@(#)igmp.c	8.2 (Berkeley) 5/3/95
 */
/* </LICENSES> */

//! The Internet Group Management Protocol (IGMP): `<netinet/igmp.h>` (the packet format and
//! constants) and `netinet/igmp.c` (the protocol). Written by Steve Deering, Stanford, May
//! 1988. Modified by Rosen Sharma, Stanford, Aug 1994. Modified by Bill Fenner, Xerox PARC,
//! Feb 1995. MULTICAST Revision: 1.3.
//!
//! Upstream: sys/netinet/igmp.h @ 3ce1f3f79392
//! Upstream: sys/netinet/igmp.c @ 3ce1f3f79392
//!
//! A host joining a group (`in_addmulti`) reports its membership at once and again after a
//! random delay; a router's query restarts the delays of the groups it asks about, and
//! another host's report for one of our groups stops our timer. Local groups (224.0.0.X,
//! the all-hosts group `in_ifinit` joins among them) are never reported. The per-interface
//! router version (`struct router_info`) decides between v1 and v2 reports and leave
//! messages; a v1 query makes it v1 for `IGMP_AGE_THRESHOLD` slow timeouts.
//!
//! Locks used to protect global data and struct members: \[I\] immutable after creation,
//! \[a\] atomic, \[G\] the global igmp mutex `igmp_mtx`.
//!
//! ## Deviations
//! - `igmpcounters` (`struct cpumem *`) is the static array of atomics [`IGMPCOUNTERS`];
//!   `counters_alloc` in `igmp_init` has nothing left to do.
//! - `router_alert` (the Router Alert option of reports, made by `igmp_init`) is an
//!   `AtomicPtr` to the mbuf, NULL before `igmp_init`.
//! - `igmp_fasttimo`'s `struct igmp_pktlist` of `malloc(M_MRTABLE)`'d `igmp_pktinfo`s is a
//!   `Vec` of [`IgmpPktinfo`] values; so `igmp_checktimer` cannot fail to queue a report,
//!   where the C skips it when `malloc(M_NOWAIT)` fails.
//! - The IGMP header is read from and written into the packet as an unaligned copy, as the
//!   IP header is (`mtod_ip`, `mtod_ip_store`); the KLUDGE that rewrites a report's source
//!   address stores the IP header back.
//! - `igmp_sendpkt` zeroes the IP header fields the C leaves to `ip_output` (version, header
//!   length, identification, TTL, checksum), which `ip_output` then fills.
//! - `MROUTING` is not configured (the own-report check of v2 reports, `imo_loop` from
//!   `ip_mrouter_active`). `igmp_input` runs `igmp_input_if` under the kernel lock, as the C.

use alloc::vec::Vec;
use core::cell::Cell;
use core::mem::size_of;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicI32, AtomicPtr, AtomicU64, Ordering};

use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_rwlock::{
    rw_assert_anylock, rw_assert_wrlock, rw_enter_write, rw_exit_write,
};
use crate::kern::kern_sysctl::sysctl_rdstruct;
use crate::kern::uipc_mbuf::{MAX_LINKHDR, m_freem, m_get, m_gethdr, m_pullup};
use crate::machine::intr::IPL_SOFTNET;
use crate::net::if_::{IFF_LOOPBACK, IFNETLIST, if_get, if_put};
use crate::net::if_var::{Ifnet, Netstack};
use crate::netinet::igmp_var::{
    IGMPCTL_STATS, IGPS_NCOUNTERS, IgmpPktinfo, IgmpstatCounters, igmp_random_delay, igmpstat_inc,
};
use crate::netinet::in_::{
    IN_CLASSA_NET, INADDR_ALLHOSTS_GROUP, INADDR_ALLROUTERS_GROUP, INADDR_ANY, IPPROTO_DONE,
    IPPROTO_IGMP, InAddr, in_ifp2ia, in_local_group, in_lookupmulti, in_multicast,
};
use crate::netinet::in_cksum::in_cksum;
use crate::netinet::in_var::{InMulti, ifmatoinm};
use crate::netinet::ip::{IPOPT_RA, Ip};
use crate::netinet::ip_output::ip_output;
use crate::netinet::ip_var::{IpMoptions, Ipoption, MAX_IPOPTLEN, mtod_ip, mtod_ip_store};
use crate::netinet::raw_ip::rip_input;
use crate::sys::endian::{htons, ntohs};
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_MRTABLE, M_NOWAIT, M_WAITOK};
use crate::sys::mbuf::{M_DONTWAIT, M_EXT, M_WAIT, MT_DATA, MT_HEADER, Mbuf, m_freemp, mtod};
use crate::sys::mutex::{Mutex, mutex_assert_locked};
use crate::sys::protosw::PR_FASTHZ;
use crate::sys::queue::{ListEntry, ListHead};
use crate::sys::socket::AF_INET;
use crate::sys::systm::{kernel_lock, kernel_unlock, net_lock_shared, net_unlock_shared};

/// `IGMP_MINLEN`.
pub const IGMP_MINLEN: usize = 8;

/// `IGMP_HOST_MEMBERSHIP_QUERY`: membership query.
pub const IGMP_HOST_MEMBERSHIP_QUERY: u8 = 0x11;
/// `IGMP_v1_HOST_MEMBERSHIP_REPORT`: v1 membership report.
#[allow(non_upper_case_globals)] // the C name
pub const IGMP_v1_HOST_MEMBERSHIP_REPORT: u8 = 0x12;
/// `IGMP_DVMRP`: DVMRP routing message.
pub const IGMP_DVMRP: u8 = 0x13;
/// `IGMP_PIM`: PIM routing message.
pub const IGMP_PIM: u8 = 0x14;
/// `IGMP_v2_HOST_MEMBERSHIP_REPORT`: v2 membership report.
#[allow(non_upper_case_globals)] // the C name
pub const IGMP_v2_HOST_MEMBERSHIP_REPORT: u8 = 0x16;
/// `IGMP_HOST_LEAVE_MESSAGE`: leave-group message.
pub const IGMP_HOST_LEAVE_MESSAGE: u8 = 0x17;
/// `IGMP_MTRACE_REPLY`: traceroute reply.
pub const IGMP_MTRACE_REPLY: u8 = 0x1e;
/// `IGMP_MTRACE_QUERY`: traceroute query.
pub const IGMP_MTRACE_QUERY: u8 = 0x1f;

/// `IGMP_MAX_HOST_REPORT_DELAY`: max delay for response to query (in seconds).
pub const IGMP_MAX_HOST_REPORT_DELAY: u32 = 10;

/// `IGMP_TIMER_SCALE`: denominator for `igmp_timer`.
pub const IGMP_TIMER_SCALE: u32 = 10;

/// `IGMP_DELAYING_MEMBER`: a state of the IGMP v2 state table (`inm_state`).
pub const IGMP_DELAYING_MEMBER: u32 = 1;
/// `IGMP_IDLE_MEMBER`.
pub const IGMP_IDLE_MEMBER: u32 = 2;
/// `IGMP_LAZY_MEMBER`.
pub const IGMP_LAZY_MEMBER: u32 = 3;
/// `IGMP_SLEEPING_MEMBER`.
pub const IGMP_SLEEPING_MEMBER: u32 = 4;
/// `IGMP_AWAKENING_MEMBER`.
pub const IGMP_AWAKENING_MEMBER: u32 = 5;

/// `IGMP_v1_ROUTER`: a state of the IGMP router version cache (`rti_type`).
#[allow(non_upper_case_globals)] // the C name
pub const IGMP_v1_ROUTER: i32 = 1;
/// `IGMP_v2_ROUTER`.
#[allow(non_upper_case_globals)] // the C name
pub const IGMP_v2_ROUTER: i32 = 2;

/// `IGMP_AGE_THRESHOLD`: revert to v2 if we haven't heard from the router in this amount of
/// time.
pub const IGMP_AGE_THRESHOLD: i32 = 540;

/// `IP_MULTICASTOPTS`: the `ip_output` flags of a report.
const IP_MULTICASTOPTS: i32 = 0;

/// `struct igmp`: IGMP packet format.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Igmp {
    /// `igmp_type`: version & type of IGMP message.
    pub igmp_type: u8,
    /// `igmp_code`: code for routing sub-messages.
    pub igmp_code: u8,
    /// `igmp_cksum`: IP-style checksum.
    pub igmp_cksum: u16,
    /// `igmp_group`: group address being reported (zero for queries).
    pub igmp_group: InAddr,
}

/// `struct router_info`: per-interface router version information.
pub struct RouterInfo {
    /// \[G\] `rti_list`.
    pub rti_list: ListEntry<RouterInfo>,
    /// \[I\] `rti_ifidx`.
    pub rti_ifidx: u32,
    /// \[G\] `rti_type`: type of router on interface.
    pub rti_type: Cell<i32>,
    /// \[G\] `rti_age`: time since last v1 query.
    pub rti_age: Cell<i32>,
}

// SAFETY: the mutable members change under `igmp_mtx`, as in C.
unsafe impl Sync for RouterInfo {}

crate::queue_adapter!(
    /// `LIST_HEAD(, router_info)` through `rti_list`.
    pub RtiList: RouterInfo, rti_list => ListEntry<RouterInfo>
);

/// `rti_head`'s type.
pub struct RtiHead(ListHead<RtiList>);

// SAFETY: changed only under `igmp_mtx`.
unsafe impl Sync for RtiHead {}

/// \[a\] `igmp_timers_are_running`: shortcut for fast timer.
pub static IGMP_TIMERS_ARE_RUNNING: AtomicI32 = AtomicI32::new(0);
/// `igmp_mtx`.
pub static IGMP_MTX: Mutex = Mutex::new(IPL_SOFTNET);
/// \[G\] `rti_head`.
static RTI_HEAD: RtiHead = RtiHead(ListHead::new());
/// `router_alert`.
static ROUTER_ALERT: AtomicPtr<Mbuf> = AtomicPtr::new(ptr::null_mut());
/// `igmpcounters`.
pub static IGMPCOUNTERS: [AtomicU64; IGPS_NCOUNTERS] =
    [const { AtomicU64::new(0) }; IGPS_NCOUNTERS];

/// `igmp_init`.
pub fn igmp_init() {
    IGMP_TIMERS_ARE_RUNNING.store(0, Ordering::Relaxed);
    RTI_HEAD.0.init();

    // igmpcounters = counters_alloc(igps_ncounters): a static array here.
    let Some(router_alert) = m_get(M_WAIT, MT_DATA) else {
        // m_get(M_WAIT) sleeps rather than fail; without the option, reports go without
        // it, as ip_output takes a NULL opt.
        return;
    };

    // Construct a Router Alert option (RAO) to use in report messages as required by
    // RFC2236. This option has the following format:
    //
    //	| 10010100 | 00000100 |  2 octet value  |
    //
    // where a value of "0" indicates that routers shall examine the packet.
    let mut ra = Ipoption {
        ipopt_dst: InAddr { s_addr: INADDR_ANY },
        ipopt_list: [0; MAX_IPOPTLEN],
    };
    ra.ipopt_list[0] = IPOPT_RA as i8;
    ra.ipopt_list[1] = 0x04;
    ra.ipopt_list[2] = 0x00;
    ra.ipopt_list[3] = 0x00;
    // SAFETY: a fresh mbuf has `MLEN` (more than `sizeof(struct ipoption)`) bytes at its
    // data pointer; the write is unaligned-safe.
    unsafe { mtod::<Ipoption>(router_alert).write_unaligned(ra) };
    router_alert
        .m_len()
        .set((size_of::<InAddr>() + ra.ipopt_list[1] as usize) as u32);
    ROUTER_ALERT.store(ptr::from_ref(router_alert).cast_mut(), Ordering::Release);
}

/// `router_alert`, once `igmp_init` made it.
fn router_alert() -> Option<&'static Mbuf> {
    // SAFETY: NULL or the mbuf `igmp_init` made, which is never freed.
    unsafe { ROUTER_ALERT.load(Ordering::Acquire).as_ref() }
}

/// `rti_find`: the router information of interface `ifidx`. Called holding `igmp_mtx`.
fn rti_find(ifidx: u32) -> Option<&'static RouterInfo> {
    mutex_assert_locked(&IGMP_MTX, "rti_find");

    RTI_HEAD.0.iter().find(|rti| rti.rti_ifidx == ifidx)
}

/// Allocates a router information record for `ifidx` (`malloc(sizeof(*rti), M_MRTABLE,
/// flags)` and the C's member stores), on no list.
fn rti_alloc(ifidx: u32, flags: i32) -> Option<&'static RouterInfo> {
    let mem = malloc(size_of::<RouterInfo>(), M_MRTABLE, flags)?;
    let rti = mem.as_ptr().cast::<RouterInfo>();
    // SAFETY: a fresh block of the structure's size, aligned by `malloc`.
    unsafe {
        rti.write(RouterInfo {
            rti_list: ListEntry::new(),
            rti_ifidx: ifidx,
            rti_type: Cell::new(IGMP_v2_ROUTER),
            rti_age: Cell::new(0),
        });
    }
    // SAFETY: initialised above; freed by `rti_delete` or `rti_free`.
    Some(unsafe { &*rti })
}

/// `free(rti, M_MRTABLE, sizeof(*rti))`.
fn rti_free(rti: &'static RouterInfo) {
    free(
        NonNull::from(rti).cast(),
        M_MRTABLE,
        size_of::<RouterInfo>(),
    );
}

/// `rti_fill`: the report type for interface `ifidx`, making its router information (a v2
/// router) if it has none.
pub fn rti_fill(ifidx: u32) -> i32 {
    mtx_enter(&IGMP_MTX);
    if let Some(rti) = rti_find(ifidx) {
        let type_ = rti.rti_type.get();
        mtx_leave(&IGMP_MTX);
        return if type_ == IGMP_v1_ROUTER {
            i32::from(IGMP_v1_HOST_MEMBERSHIP_REPORT)
        } else {
            i32::from(IGMP_v2_HOST_MEMBERSHIP_REPORT)
        };
    }
    mtx_leave(&IGMP_MTX);

    let new_rti = rti_alloc(ifidx, M_WAITOK);

    mtx_enter(&IGMP_MTX);
    // check again after unlock and lock
    if let Some(rti) = rti_find(ifidx) {
        let type_ = rti.rti_type.get();
        mtx_leave(&IGMP_MTX);

        if let Some(new_rti) = new_rti {
            rti_free(new_rti);
        }
        return if type_ == IGMP_v1_ROUTER {
            i32::from(IGMP_v1_HOST_MEMBERSHIP_REPORT)
        } else {
            i32::from(IGMP_v2_HOST_MEMBERSHIP_REPORT)
        };
    }
    if let Some(rti) = new_rti {
        // SAFETY: a new record, on no list; under `igmp_mtx`.
        unsafe { RTI_HEAD.0.insert_head(rti) };
    }
    mtx_leave(&IGMP_MTX);

    i32::from(IGMP_v2_HOST_MEMBERSHIP_REPORT)
}

/// `rti_type`: router version of the interface, `IGMP_v2_ROUTER` if unknown.
pub fn rti_type(ifidx: u32) -> i32 {
    let mut type_ = IGMP_v2_ROUTER;

    mtx_enter(&IGMP_MTX);
    if let Some(rti) = rti_find(ifidx) {
        type_ = rti.rti_type.get();
    }
    mtx_leave(&IGMP_MTX);

    type_
}

/// `rti_reset`: a v1 query arrived on `ifp`: its router is v1, aged 0.
pub fn rti_reset(ifp: &Ifnet) -> Result<(), Errno> {
    mtx_enter(&IGMP_MTX);
    let rti = match rti_find(ifp.if_index.get()) {
        Some(rti) => rti,
        None => {
            let Some(rti) = rti_alloc(ifp.if_index.get(), M_NOWAIT) else {
                mtx_leave(&IGMP_MTX);
                return Err(Errno::ENOBUFS);
            };
            // SAFETY: a new record, on no list; under `igmp_mtx`.
            unsafe { RTI_HEAD.0.insert_head(rti) };
            rti
        }
    };
    rti.rti_type.set(IGMP_v1_ROUTER);
    rti.rti_age.set(0);
    mtx_leave(&IGMP_MTX);

    Ok(())
}

/// `rti_delete`: forgets the router information of an interface going away.
pub fn rti_delete(ifp: &Ifnet) {
    mtx_enter(&IGMP_MTX);
    let rti = rti_find(ifp.if_index.get());
    if let Some(rti) = rti {
        // SAFETY: `rti` is on `rti_head` (`rti_find`); under `igmp_mtx`.
        unsafe { ListHead::<RtiList>::remove(rti) };
    }
    mtx_leave(&IGMP_MTX);

    if let Some(rti) = rti {
        rti_free(rti);
    }
}

/// `igmp_input`: the IGMP entry of `inetsw[]`.
pub fn igmp_input(
    mp: &mut Option<&'static Mbuf>,
    offp: &mut i32,
    proto: i32,
    af: i32,
    ns: Option<&Netstack>,
) -> i32 {
    igmpstat_inc(IgmpstatCounters::IgpsRcvTotal);

    let Some(m) = *mp else {
        return IPPROTO_DONE;
    };
    let Some(ifp) = if_get(m.m_pkthdr().ph_ifidx.get()) else {
        m_freemp(mp);
        return IPPROTO_DONE;
    };

    kernel_lock();
    let proto = igmp_input_if(ifp, mp, offp, proto, af, ns);
    kernel_unlock();
    if_put(ifp);
    proto
}

/// The IGMP header of `m`, `iphlen` bytes in.
fn igmp_hdr(m: &Mbuf, iphlen: usize) -> Igmp {
    // SAFETY: the caller pulled up `iphlen + IGMP_MINLEN` bytes into the first mbuf; the
    // read is unaligned-safe.
    unsafe { mtod::<u8>(m).add(iphlen).cast::<Igmp>().read_unaligned() }
}

/// `igmp_input_if`: validates an IGMP message from `ifp` and acts on a query or a report,
/// then passes it to the raw IGMP sockets.
pub fn igmp_input_if(
    ifp: &'static Ifnet,
    mp: &mut Option<&'static Mbuf>,
    offp: &mut i32,
    proto: i32,
    af: i32,
    ns: Option<&Netstack>,
) -> i32 {
    let Some(mut m) = *mp else {
        return IPPROTO_DONE;
    };
    let iphlen = *offp;
    let mut ip = mtod_ip(m);
    let mut running = false;

    let igmplen = i32::from(ntohs(ip.ip_len)) - iphlen;

    // Validate lengths
    if igmplen < IGMP_MINLEN as i32 {
        igmpstat_inc(IgmpstatCounters::IgpsRcvTooshort);
        m_freem(m);
        *mp = None;
        return IPPROTO_DONE;
    }
    let minlen = iphlen + IGMP_MINLEN as i32;
    if m.m_flags().get() & M_EXT != 0 || (m.m_len().get() as i32) < minlen {
        *mp = m_pullup(m, minlen);
        let Some(n) = *mp else {
            igmpstat_inc(IgmpstatCounters::IgpsRcvTooshort);
            return IPPROTO_DONE;
        };
        m = n;
    }

    // Validate checksum
    m.m_data()
        .set(m.m_data().get().wrapping_add(iphlen as usize));
    m.m_len().set(m.m_len().get() - iphlen as u32);
    let igmp = igmp_hdr(m, 0);
    if in_cksum(m, igmplen) != 0 {
        igmpstat_inc(IgmpstatCounters::IgpsRcvBadsum);
        m_freem(m);
        *mp = None;
        return IPPROTO_DONE;
    }
    m.m_data()
        .set(m.m_data().get().wrapping_sub(iphlen as usize));
    m.m_len().set(m.m_len().get() + iphlen as u32);
    ip = mtod_ip(m);

    match igmp.igmp_type {
        IGMP_HOST_MEMBERSHIP_QUERY => 'query: {
            igmpstat_inc(IgmpstatCounters::IgpsRcvQueries);

            if ifp.if_flags.get() & IFF_LOOPBACK != 0 {
                break 'query;
            }

            if igmp.igmp_code == 0 {
                if rti_reset(ifp).is_err() {
                    m_freem(m);
                    *mp = None;
                    return IPPROTO_DONE;
                }

                if ip.ip_dst.s_addr != INADDR_ALLHOSTS_GROUP {
                    igmpstat_inc(IgmpstatCounters::IgpsRcvBadqueries);
                    m_freem(m);
                    *mp = None;
                    return IPPROTO_DONE;
                }

                // Start the timers in all of our membership records for the interface on
                // which the query arrived, except those that are already running and those
                // that belong to a "local" group (224.0.0.X).
                rw_enter_write(&ifp.if_maddrlock);
                for inm in inet_memberships(ifp) {
                    if inm.inm_timer.get() == 0 && !in_local_group(inm.inm_addr().s_addr) {
                        inm.inm_state.set(IGMP_DELAYING_MEMBER);
                        inm.inm_timer.set(igmp_random_delay(
                            IGMP_MAX_HOST_REPORT_DELAY * PR_FASTHZ as u32,
                        ));
                        running = true;
                    }
                }
                rw_exit_write(&ifp.if_maddrlock);
            } else {
                if !in_multicast(ip.ip_dst.s_addr) {
                    igmpstat_inc(IgmpstatCounters::IgpsRcvBadqueries);
                    m_freem(m);
                    *mp = None;
                    return IPPROTO_DONE;
                }

                let mut timer = u32::from(igmp.igmp_code) * PR_FASTHZ as u32 / IGMP_TIMER_SCALE;
                if timer == 0 {
                    timer = 1;
                }

                // Start the timers in all of our membership records for the interface on
                // which the query arrived, except those that are already running and those
                // that belong to a "local" group (224.0.0.X). For timers already running,
                // check if they need to be reset.
                rw_enter_write(&ifp.if_maddrlock);
                for inm in inet_memberships(ifp) {
                    if !in_local_group(inm.inm_addr().s_addr)
                        && (ip.ip_dst.s_addr == INADDR_ALLHOSTS_GROUP
                            || ip.ip_dst.s_addr == inm.inm_addr().s_addr)
                    {
                        match inm.inm_state.get() {
                            IGMP_DELAYING_MEMBER if inm.inm_timer.get() <= timer => {}
                            IGMP_DELAYING_MEMBER
                            | IGMP_IDLE_MEMBER
                            | IGMP_LAZY_MEMBER
                            | IGMP_AWAKENING_MEMBER => {
                                inm.inm_state.set(IGMP_DELAYING_MEMBER);
                                inm.inm_timer.set(igmp_random_delay(timer));
                                running = true;
                            }
                            IGMP_SLEEPING_MEMBER => {
                                inm.inm_state.set(IGMP_AWAKENING_MEMBER);
                            }
                            _ => {}
                        }
                    }
                }
                rw_exit_write(&ifp.if_maddrlock);
            }
        }

        t if t == IGMP_v1_HOST_MEMBERSHIP_REPORT => 'report: {
            igmpstat_inc(IgmpstatCounters::IgpsRcvReports);

            if ifp.if_flags.get() & IFF_LOOPBACK != 0 {
                break 'report;
            }

            if !in_multicast(igmp.igmp_group.s_addr) || igmp.igmp_group.s_addr != ip.ip_dst.s_addr {
                igmpstat_inc(IgmpstatCounters::IgpsRcvBadreports);
                m_freem(m);
                *mp = None;
                return IPPROTO_DONE;
            }

            // KLUDGE: if the IP source address of the report has an unspecified (i.e., zero)
            // subnet number, as is allowed for a booting host, replace it with the correct
            // subnet number so that a process-level multicast routing daemon can determine
            // which subnet it arrived from. This is necessary to compensate for the lack of
            // any way for a process to determine the arrival interface of an incoming packet.
            if ip.ip_src.s_addr & IN_CLASSA_NET == 0
                && let Some(ia) = in_ifp2ia(ifp)
            {
                ip.ip_src.s_addr = ia.ia_net.get();
                mtod_ip_store(m, &ip);
            }

            // If we belong to the group being reported, stop our timer for that group.
            rw_enter_write(&ifp.if_maddrlock);
            if let Some(inm) = in_lookupmulti(&igmp.igmp_group, ifp) {
                inm.inm_timer.set(0);
                igmpstat_inc(IgmpstatCounters::IgpsRcvOurreports);

                match inm.inm_state.get() {
                    IGMP_IDLE_MEMBER
                    | IGMP_LAZY_MEMBER
                    | IGMP_AWAKENING_MEMBER
                    | IGMP_SLEEPING_MEMBER => inm.inm_state.set(IGMP_SLEEPING_MEMBER),
                    IGMP_DELAYING_MEMBER => {
                        if rti_type(inm.inm_ifidx().get()) == IGMP_v1_ROUTER {
                            inm.inm_state.set(IGMP_LAZY_MEMBER);
                        } else {
                            inm.inm_state.set(IGMP_SLEEPING_MEMBER);
                        }
                    }
                    _ => {}
                }
            }
            rw_exit_write(&ifp.if_maddrlock);
        }

        t if t == IGMP_v2_HOST_MEMBERSHIP_REPORT => 'report: {
            // MROUTING: make sure we don't hear our own membership report (fast leave
            // requires knowing that we are the only member of a group); not configured.

            igmpstat_inc(IgmpstatCounters::IgpsRcvReports);

            if ifp.if_flags.get() & IFF_LOOPBACK != 0 {
                break 'report;
            }

            if !in_multicast(igmp.igmp_group.s_addr) || igmp.igmp_group.s_addr != ip.ip_dst.s_addr {
                igmpstat_inc(IgmpstatCounters::IgpsRcvBadreports);
                m_freem(m);
                *mp = None;
                return IPPROTO_DONE;
            }

            // KLUDGE: as for v1 reports, a zero subnet number in the source becomes ours.
            if ip.ip_src.s_addr & IN_CLASSA_NET == 0
                && let Some(ia) = in_ifp2ia(ifp)
            {
                ip.ip_src.s_addr = ia.ia_net.get();
                mtod_ip_store(m, &ip);
            }

            // If we belong to the group being reported, stop our timer for that group.
            rw_enter_write(&ifp.if_maddrlock);
            if let Some(inm) = in_lookupmulti(&igmp.igmp_group, ifp) {
                inm.inm_timer.set(0);
                igmpstat_inc(IgmpstatCounters::IgpsRcvOurreports);

                match inm.inm_state.get() {
                    IGMP_DELAYING_MEMBER | IGMP_IDLE_MEMBER | IGMP_AWAKENING_MEMBER => {
                        inm.inm_state.set(IGMP_LAZY_MEMBER);
                    }
                    _ => {}
                }
            }
            rw_exit_write(&ifp.if_maddrlock);
        }

        _ => {}
    }

    if running {
        IGMP_TIMERS_ARE_RUNNING.store(1, Ordering::Relaxed);
    }

    // Pass all valid IGMP packets up to any process(es) listening on a raw IGMP socket.
    rip_input(mp, offp, proto, af, ns)
}

/// The `AF_INET` multicast records of `ifp` (`TAILQ_FOREACH(ifma, &ifp->if_maddrlist,
/// ifma_list)` with the family test and `ifmatoinm`). Called holding `if_maddrlock`.
fn inet_memberships(ifp: &Ifnet) -> impl Iterator<Item = &InMulti> {
    ifp.if_maddrlist.iter().filter_map(|ifma| {
        let sa = ifma.ifma_addr.get();
        // SAFETY: a record's address is readable (its protocol set it).
        (!sa.is_null() && unsafe { (*sa).sa_family } == AF_INET).then(|| ifmatoinm(ifma))
    })
}

/// `igmp_joingroup`: we joined `inm` on `ifp`: start reporting it (fills `pkt` with the
/// first report) unless it is a local group or the interface is a loopback. Called holding
/// `if_maddrlock` for writing.
pub fn igmp_joingroup(inm: &InMulti, ifp: &Ifnet, pkt: &mut IgmpPktinfo) {
    let mut running = false;

    rw_assert_wrlock(&ifp.if_maddrlock);

    inm.inm_state.set(IGMP_IDLE_MEMBER);

    if !in_local_group(inm.inm_addr().s_addr) && ifp.if_flags.get() & IFF_LOOPBACK == 0 {
        inm.inm_state.set(IGMP_DELAYING_MEMBER);
        inm.inm_timer.set(igmp_random_delay(
            IGMP_MAX_HOST_REPORT_DELAY * PR_FASTHZ as u32,
        ));
        pkt.ipi_addr = inm.inm_addr();
        pkt.ipi_rdomain = ifp.if_rdomain.get();
        pkt.ipi_ifidx = inm.inm_ifidx().get();
        pkt.ipi_type = rti_fill(inm.inm_ifidx().get());
        running = true;
    } else {
        inm.inm_timer.set(0);
    }

    if running {
        IGMP_TIMERS_ARE_RUNNING.store(1, Ordering::Relaxed);
    }
}

/// `igmp_leavegroup`: we leave `inm` on `ifp`: a leave message to the all-routers group
/// (in `pkt`) when we may have been the last reporter and the router speaks v2. Called
/// holding `if_maddrlock`.
pub fn igmp_leavegroup(inm: &InMulti, ifp: &Ifnet, pkt: &mut IgmpPktinfo) {
    rw_assert_anylock(&ifp.if_maddrlock);

    match inm.inm_state.get() {
        IGMP_DELAYING_MEMBER | IGMP_IDLE_MEMBER
            if !in_local_group(inm.inm_addr().s_addr)
                && ifp.if_flags.get() & IFF_LOOPBACK == 0
                && rti_type(inm.inm_ifidx().get()) != IGMP_v1_ROUTER =>
        {
            pkt.ipi_addr.s_addr = INADDR_ALLROUTERS_GROUP;
            pkt.ipi_rdomain = ifp.if_rdomain.get();
            pkt.ipi_ifidx = inm.inm_ifidx().get();
            pkt.ipi_type = i32::from(IGMP_HOST_LEAVE_MESSAGE);
        }
        // A local group, a loopback or a v1 router; IGMP_LAZY_MEMBER, IGMP_AWAKENING_MEMBER,
        // IGMP_SLEEPING_MEMBER.
        _ => {}
    }
}

/// `igmp_fasttimo`: counts the membership timers down (`PR_FASTHZ` times a second) and sends
/// the reports of those that expire.
pub fn igmp_fasttimo() {
    let mut running = false;

    // Quick check to see if any work needs to be done, in order to minimize the overhead of
    // fasttimo processing. Variable igmp_timers_are_running is read atomically, but without
    // lock intentionally. In case it is not set due to MP races, we may miss to check the
    // timers. Then run the loop at next fast timeout.
    if IGMP_TIMERS_ARE_RUNNING.load(Ordering::Relaxed) == 0 {
        return;
    }
    IGMP_TIMERS_ARE_RUNNING.store(0, Ordering::Relaxed);

    net_lock_shared();

    let mut pktlist: Vec<IgmpPktinfo> = Vec::new();
    for ifp in IFNETLIST.0.iter() {
        if igmp_checktimer(ifp, &mut pktlist) {
            running = true;
        }
    }

    for pkt in &pktlist {
        igmp_sendpkt(pkt);
    }

    net_unlock_shared();

    if running {
        IGMP_TIMERS_ARE_RUNNING.store(1, Ordering::Relaxed);
    }
}

/// `igmp_checktimer`: one tick of the timers of `ifp`'s groups; queues the reports of those
/// that expire on `pktlist`. `true` while some still run.
pub fn igmp_checktimer(ifp: &Ifnet, pktlist: &mut Vec<IgmpPktinfo>) -> bool {
    let mut running = false;
    let mut type_ = 0;

    rw_enter_write(&ifp.if_maddrlock);
    for inm in inet_memberships(ifp) {
        if inm.inm_timer.get() == 0 {
            // do nothing
        } else {
            inm.inm_timer.set(inm.inm_timer.get() - 1);
            if inm.inm_timer.get() == 0 {
                if inm.inm_state.get() == IGMP_DELAYING_MEMBER {
                    inm.inm_state.set(IGMP_IDLE_MEMBER);
                    if type_ == 0 {
                        type_ = rti_type(ifp.if_index.get());
                    }
                    pktlist.push(IgmpPktinfo {
                        ipi_addr: inm.inm_addr(),
                        ipi_rdomain: ifp.if_rdomain.get(),
                        ipi_ifidx: inm.inm_ifidx().get(),
                        ipi_type: if type_ == IGMP_v1_ROUTER {
                            i32::from(IGMP_v1_HOST_MEMBERSHIP_REPORT)
                        } else {
                            i32::from(IGMP_v2_HOST_MEMBERSHIP_REPORT)
                        },
                    });
                }
            } else {
                running = true;
            }
        }
    }
    rw_exit_write(&ifp.if_maddrlock);

    running
}

/// `igmp_slowtimo`: ages the v1 routers; one silent for `IGMP_AGE_THRESHOLD` slow timeouts
/// is v2 again.
pub fn igmp_slowtimo() {
    if RTI_HEAD.0.is_empty() {
        return;
    }

    mtx_enter(&IGMP_MTX);
    for rti in RTI_HEAD.0.iter() {
        if rti.rti_type.get() == IGMP_v1_ROUTER {
            rti.rti_age.set(rti.rti_age.get() + 1);
            if rti.rti_age.get() >= IGMP_AGE_THRESHOLD {
                rti.rti_type.set(IGMP_v2_ROUTER);
            }
        }
    }
    mtx_leave(&IGMP_MTX);
}

/// `igmp_sendpkt`: sends the report or leave message `pkt` describes, with the Router Alert
/// option, TTL 1, out of `pkt.ipi_ifidx`.
pub fn igmp_sendpkt(pkt: &IgmpPktinfo) {
    let Some(m) = m_gethdr(M_DONTWAIT, MT_HEADER) else {
        return;
    };

    // Assume max_linkhdr + sizeof(struct ip) + IGMP_MINLEN is smaller than mbuf size
    // returned by MGETHDR.
    let max_linkhdr = MAX_LINKHDR.load(Ordering::Relaxed) as usize;
    m.m_data().set(m.m_data().get().wrapping_add(max_linkhdr));
    m.m_len().set((size_of::<Ip>() + IGMP_MINLEN) as u32);
    m.m_pkthdr().len.set((size_of::<Ip>() + IGMP_MINLEN) as i32);

    let ip = Ip {
        ip_tos: 0,
        ip_len: htons((size_of::<Ip>() + IGMP_MINLEN) as u16),
        ip_off: 0,
        ip_p: IPPROTO_IGMP as u8,
        ip_src: InAddr { s_addr: INADDR_ANY },
        ip_dst: pkt.ipi_addr,
        ..Ip::default()
    };
    mtod_ip_store(m, &ip);

    m.m_data()
        .set(m.m_data().get().wrapping_add(size_of::<Ip>()));
    m.m_len().set(m.m_len().get() - size_of::<Ip>() as u32);
    let mut igmp = Igmp {
        igmp_type: pkt.ipi_type as u8,
        igmp_code: 0,
        igmp_group: pkt.ipi_addr,
        igmp_cksum: 0,
    };
    // SAFETY: the mbuf has `IGMP_MINLEN` bytes at its data pointer now; unaligned-safe.
    unsafe { mtod::<Igmp>(m).write_unaligned(igmp) };
    igmp.igmp_cksum = in_cksum(m, IGMP_MINLEN as i32);
    // SAFETY: as above.
    unsafe { mtod::<Igmp>(m).write_unaligned(igmp) };
    m.m_data()
        .set(m.m_data().get().wrapping_sub(size_of::<Ip>()));
    m.m_len().set(m.m_len().get() + size_of::<Ip>() as u32);

    m.m_pkthdr().ph_rtableid.set(pkt.ipi_rdomain);
    let imo = IpMoptions {
        imo_membership: ptr::null_mut(),
        imo_ifidx: pkt.ipi_ifidx as u16,
        imo_ttl: 1,
        // Request loopback of the report if we are acting as a multicast router, so that
        // the process-level routing daemon can hear it. MROUTING: ip_mrouter_active; not
        // configured.
        imo_loop: 0,
        imo_num_memberships: 0,
        imo_max_memberships: 0,
    };

    let _ = ip_output(
        m,
        router_alert(),
        None,
        IP_MULTICASTOPTS,
        Some(&imo),
        None,
        0,
    );

    igmpstat_inc(IgmpstatCounters::IgpsSndReports);
}

/// `igmp_sysctl`: sysctl for igmp variables (`net.inet.igmp.*`).
pub fn igmp_sysctl(
    name: &[i32],
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    _newlen: usize,
) -> Result<(), Errno> {
    // All sysctl names at this level are terminal.
    let [n] = name else {
        return Err(Errno::ENOTDIR);
    };

    match *n {
        IGMPCTL_STATS => igmp_sysctl_igmpstat(oldp, oldlenp, newp),
        _ => Err(Errno::EOPNOTSUPP),
    }
}

/// `igmp_sysctl_igmpstat`: `net.inet.igmp.stats`, the counters as a `struct igmpstat`.
fn igmp_sysctl_igmpstat(oldp: usize, oldlenp: &mut usize, newp: usize) -> Result<(), Errno> {
    let mut bytes = [0u8; IGPS_NCOUNTERS * size_of::<u64>()];
    for (i, c) in IGMPCOUNTERS.iter().enumerate() {
        bytes[i * 8..i * 8 + 8].copy_from_slice(&c.load(Ordering::Relaxed).to_ne_bytes());
    }

    sysctl_rdstruct(oldp, oldlenp, newp, &bytes)
}

// The wire layout.
const _: () = assert!(size_of::<Igmp>() == IGMP_MINLEN);

#[cfg(test)]
mod tests;
